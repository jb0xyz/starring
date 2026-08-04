use std::time::Duration;

use automation_runtime_controller::{
    RuntimeConvergenceErrorClassV1, RuntimeDisconnectServingV1, RuntimeHeartbeatServingV1,
    RuntimeServingLeasePort, RuntimeServingReceiptV1, RuntimeServingUpdateReceiptV1,
};
use sqlx::{PgConnection, PgPool};

use crate::connection::ServingConnectionGuardV1;
use crate::contract::{DISCONNECT_QUERY, HEARTBEAT_QUERY};
use crate::database::{
    begin_serving_mutation_transaction, verify_runtime_serving_binding_v1,
    verify_runtime_serving_database_with_timeouts_v1,
};
use crate::error::{
    map_mutation_commit_error, map_mutation_error, map_query_error, validate_millisecond_duration,
};
use crate::row::RuntimeServingMutationRowV1;
use crate::{
    RuntimeServingDatabaseExpectationV1, RuntimeServingDatabaseReadinessV1,
    RuntimeServingDatabaseTimeoutsV1, RuntimeServingPersistenceErrorV1,
};

pub const MIN_RUNTIME_SERVING_LEASE_DURATION: Duration = Duration::from_secs(1);
pub const MAX_RUNTIME_SERVING_LEASE_DURATION: Duration = Duration::from_secs(300);

#[derive(Clone)]
pub struct PostgresRuntimeServingLeaseV1 {
    pub(crate) pool: PgPool,
    pub(crate) expectation: RuntimeServingDatabaseExpectationV1,
    pub(crate) timeouts: RuntimeServingDatabaseTimeoutsV1,
    initial_readiness: RuntimeServingDatabaseReadinessV1,
}

enum RuntimeServingMutationV1<'a> {
    Heartbeat(&'a RuntimeHeartbeatServingV1),
    Disconnect(&'a RuntimeDisconnectServingV1),
}

impl PostgresRuntimeServingLeaseV1 {
    pub async fn connect_verified(
        pool: PgPool,
        expectation: RuntimeServingDatabaseExpectationV1,
        timeouts: RuntimeServingDatabaseTimeoutsV1,
    ) -> Result<Self, RuntimeServingPersistenceErrorV1> {
        let initial_readiness =
            verify_runtime_serving_database_with_timeouts_v1(&pool, &expectation, timeouts).await?;
        Ok(Self {
            pool,
            expectation,
            timeouts,
            initial_readiness,
        })
    }

    pub async fn connect_verified_default(
        pool: PgPool,
        expectation: RuntimeServingDatabaseExpectationV1,
    ) -> Result<Self, RuntimeServingPersistenceErrorV1> {
        Self::connect_verified(
            pool,
            expectation,
            RuntimeServingDatabaseTimeoutsV1::default(),
        )
        .await
    }

    pub fn initial_readiness(&self) -> &RuntimeServingDatabaseReadinessV1 {
        &self.initial_readiness
    }

    pub async fn verify_database_v1(
        &self,
    ) -> Result<RuntimeServingDatabaseReadinessV1, RuntimeServingPersistenceErrorV1> {
        verify_runtime_serving_database_with_timeouts_v1(
            &self.pool,
            &self.expectation,
            self.timeouts,
        )
        .await
    }

    async fn execute_mutation(
        &self,
        mutation: RuntimeServingMutationV1<'_>,
    ) -> Result<RuntimeServingReceiptV1, RuntimeServingPersistenceErrorV1> {
        let deadline = tokio::time::Instant::now() + self.timeouts.statement_timeout();
        let connection = tokio::time::timeout_at(deadline, self.pool.acquire())
            .await
            .map_err(|_| RuntimeServingPersistenceErrorV1::Timeout)?
            .map_err(map_query_error)?;
        let mut connection = ServingConnectionGuardV1::new(connection);
        let database_connection = connection
            .connection_mut()
            .ok_or(RuntimeServingPersistenceErrorV1::PersistenceCorrupt)?;
        let result = tokio::time::timeout_at(
            deadline,
            self.execute_mutation_on_connection(database_connection, mutation),
        )
        .await;
        match result {
            Ok(result) => {
                connection.release_to_pool();
                result
            }
            Err(_) => Err(RuntimeServingPersistenceErrorV1::Indeterminate),
        }
    }

    async fn execute_mutation_on_connection(
        &self,
        connection: &mut PgConnection,
        mutation: RuntimeServingMutationV1<'_>,
    ) -> Result<RuntimeServingReceiptV1, RuntimeServingPersistenceErrorV1> {
        let mut transaction = begin_serving_mutation_transaction(connection, self.timeouts).await?;
        verify_runtime_serving_binding_v1(&mut transaction, &self.expectation).await?;
        let receipt = match mutation {
            RuntimeServingMutationV1::Heartbeat(request) => {
                let lease_milliseconds = validate_millisecond_duration(
                    request.lease_for,
                    MAX_RUNTIME_SERVING_LEASE_DURATION,
                )?;
                if request.lease_for < MIN_RUNTIME_SERVING_LEASE_DURATION {
                    return Err(RuntimeServingPersistenceErrorV1::InvalidInput);
                }
                let identity = &request.identity;
                let rows = sqlx::query_as::<_, RuntimeServingMutationRowV1>(HEARTBEAT_QUERY)
                    .bind(identity.scope.tenant_id.as_str())
                    .bind(identity.scope.installation_id.as_str())
                    .bind(identity.scope.deployment_id.as_str())
                    .bind(identity.attestation_id.as_str())
                    .bind(identity.process_instance_id.as_str())
                    .bind(runtime_i64(identity.runtime_generation.get())?)
                    .bind(runtime_i64(identity.lease_epoch.get())?)
                    .bind(runtime_i64(identity.expected_revision.get())?)
                    .bind(lease_milliseconds)
                    .fetch_all(&mut *transaction)
                    .await
                    .map_err(map_mutation_error)?;
                let [row] = rows.as_slice() else {
                    return Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt);
                };
                row.decode_heartbeat(request)?
            }
            RuntimeServingMutationV1::Disconnect(request) => {
                let identity = &request.identity;
                let rows = sqlx::query_as::<_, RuntimeServingMutationRowV1>(DISCONNECT_QUERY)
                    .bind(identity.scope.tenant_id.as_str())
                    .bind(identity.scope.installation_id.as_str())
                    .bind(identity.scope.deployment_id.as_str())
                    .bind(identity.attestation_id.as_str())
                    .bind(identity.process_instance_id.as_str())
                    .bind(runtime_i64(identity.runtime_generation.get())?)
                    .bind(runtime_i64(identity.lease_epoch.get())?)
                    .bind(runtime_i64(identity.expected_revision.get())?)
                    .fetch_all(&mut *transaction)
                    .await
                    .map_err(map_mutation_error)?;
                let [row] = rows.as_slice() else {
                    return Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt);
                };
                row.decode_disconnect(request)?
            }
        };
        transaction
            .commit()
            .await
            .map_err(map_mutation_commit_error)?;
        Ok(receipt)
    }
}

impl RuntimeServingLeasePort for PostgresRuntimeServingLeaseV1 {
    type Error = RuntimeServingPersistenceErrorV1;

    async fn heartbeat_serving(
        &self,
        request: RuntimeHeartbeatServingV1,
    ) -> Result<RuntimeServingUpdateReceiptV1, Self::Error> {
        let action_id = request.action_id;
        let serving = self
            .execute_mutation(RuntimeServingMutationV1::Heartbeat(&request))
            .await?;
        Ok(RuntimeServingUpdateReceiptV1 { action_id, serving })
    }

    async fn mark_serving_disconnected(
        &self,
        request: RuntimeDisconnectServingV1,
    ) -> Result<RuntimeServingUpdateReceiptV1, Self::Error> {
        let action_id = request.action_id;
        let serving = self
            .execute_mutation(RuntimeServingMutationV1::Disconnect(&request))
            .await?;
        Ok(RuntimeServingUpdateReceiptV1 { action_id, serving })
    }

    fn classify_error(error: &Self::Error) -> RuntimeConvergenceErrorClassV1 {
        error.class()
    }
}

fn runtime_i64(value: u64) -> Result<i64, RuntimeServingPersistenceErrorV1> {
    i64::try_from(value).map_err(|_| RuntimeServingPersistenceErrorV1::InvalidInput)
}
