mod query;
mod row;

use std::future::Future;
use std::time::Instant;

use automation_runtime_controller::RuntimeStartupRecoveryObservationRequestV2;
use automation_runtime_worker::{
    RuntimeAuthorizedStartupRecoveryObservationV2, RuntimeCompletedStartupRecoveryObservationV2,
    RuntimeStartupRecoveryObservationPortV2,
};
use sqlx::PgConnection;

use self::query::OBSERVE_STARTUP_RECOVERY_QUERY;
use self::row::{
    RuntimeStartupRecoveryObservationDecodeOutcomeV2, RuntimeStartupRecoveryObservationRowV2,
};
use crate::connection::ExecutionConnectionGuardV1;
use crate::database::{begin_execution_mutation_transaction, verify_runtime_execution_binding_v1};
use crate::error::map_query_error;
use crate::{PostgresRuntimeExecutionV1, RuntimeExecutionPersistenceErrorV1};

impl PostgresRuntimeExecutionV1 {
    async fn observe_startup_recovery_v2(
        &self,
        authorization: RuntimeAuthorizedStartupRecoveryObservationV2,
        operation_cutoff: Instant,
    ) -> Result<RuntimeCompletedStartupRecoveryObservationV2, RuntimeExecutionPersistenceErrorV1>
    {
        if Instant::now() >= operation_cutoff {
            return Err(RuntimeExecutionPersistenceErrorV1::Timeout);
        }
        let bindings =
            RuntimeStartupRecoveryObservationBindingsV2::from_request(authorization.request())?;
        let statement_cutoff = Instant::now()
            .checked_add(self.timeouts.statement_timeout())
            .ok_or(RuntimeExecutionPersistenceErrorV1::InvalidInput)?;
        let effective_cutoff = operation_cutoff.min(statement_cutoff);
        let deadline = tokio::time::Instant::from_std(effective_cutoff);
        let connection = match tokio::time::timeout_at(deadline, self.pool.acquire()).await {
            Ok(Ok(connection)) => connection,
            Ok(Err(error)) => {
                if Instant::now() >= effective_cutoff {
                    return Err(RuntimeExecutionPersistenceErrorV1::Timeout);
                }
                return Err(map_query_error(error));
            }
            Err(_) => return Err(RuntimeExecutionPersistenceErrorV1::Timeout),
        };
        let mut connection = ExecutionConnectionGuardV1::new(connection);
        if Instant::now() >= effective_cutoff {
            return Err(RuntimeExecutionPersistenceErrorV1::Timeout);
        }
        let database_connection = connection
            .connection_mut()
            .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
        let result = tokio::time::timeout_at(
            deadline,
            self.observe_startup_recovery_on_connection_v2(
                database_connection,
                authorization.request(),
                bindings,
            ),
        )
        .await;
        if Instant::now() >= effective_cutoff {
            return Err(RuntimeExecutionPersistenceErrorV1::Timeout);
        }
        let outcome = match result {
            Ok(Ok(outcome)) => {
                connection.release_to_pool();
                outcome
            }
            Ok(Err(error)) => return Err(error),
            Err(_) => return Err(RuntimeExecutionPersistenceErrorV1::Timeout),
        };
        match outcome {
            RuntimeStartupRecoveryObservationDecodeOutcomeV2::Observed(receipt) => {
                Ok(authorization.complete(*receipt))
            }
            RuntimeStartupRecoveryObservationDecodeOutcomeV2::NotCurrent => {
                Err(RuntimeExecutionPersistenceErrorV1::OwnershipLost)
            }
            RuntimeStartupRecoveryObservationDecodeOutcomeV2::Ambiguous => {
                Err(RuntimeExecutionPersistenceErrorV1::ObservationAmbiguous)
            }
        }
    }

    async fn observe_startup_recovery_on_connection_v2(
        &self,
        connection: &mut PgConnection,
        request: &RuntimeStartupRecoveryObservationRequestV2,
        bindings: RuntimeStartupRecoveryObservationBindingsV2,
    ) -> Result<RuntimeStartupRecoveryObservationDecodeOutcomeV2, RuntimeExecutionPersistenceErrorV1>
    {
        let mut transaction =
            begin_execution_mutation_transaction(connection, self.timeouts).await?;
        verify_runtime_execution_binding_v1(&mut transaction, &self.expectation).await?;
        let mut rows = sqlx::query_as::<_, RuntimeStartupRecoveryObservationRowV2>(
            OBSERVE_STARTUP_RECOVERY_QUERY,
        )
        .bind(request.gateway_owner_lease_id.gateway_shard_id.as_str())
        .bind(request.gateway_owner_lease_id.process_instance_id.as_str())
        .bind(bindings.lease_epoch)
        .bind(
            request
                .gateway_owner_lease_id
                .expected_build_revision
                .as_str(),
        )
        .bind(bindings.owner_revision)
        .bind(request.expected_owner_expires_at)
        .fetch_all(&mut *transaction)
        .await
        .map_err(map_query_error)?;
        if rows.len() != 1 {
            return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
        }
        let outcome = rows
            .pop()
            .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?
            .decode(request)?;
        transaction.commit().await.map_err(map_query_error)?;
        Ok(outcome)
    }
}

impl RuntimeStartupRecoveryObservationPortV2 for PostgresRuntimeExecutionV1 {
    type Error = RuntimeExecutionPersistenceErrorV1;

    fn observe_startup_recovery(
        &self,
        authorization: RuntimeAuthorizedStartupRecoveryObservationV2,
        operation_cutoff: Instant,
    ) -> impl Future<Output = Result<RuntimeCompletedStartupRecoveryObservationV2, Self::Error>> + Send
    {
        self.observe_startup_recovery_v2(authorization, operation_cutoff)
    }
}

#[derive(Clone, Copy)]
struct RuntimeStartupRecoveryObservationBindingsV2 {
    lease_epoch: i64,
    owner_revision: i64,
}

impl RuntimeStartupRecoveryObservationBindingsV2 {
    fn from_request(
        request: &RuntimeStartupRecoveryObservationRequestV2,
    ) -> Result<Self, RuntimeExecutionPersistenceErrorV1> {
        Ok(Self {
            lease_epoch: i64::try_from(request.gateway_owner_lease_id.lease_epoch.get())
                .map_err(|_| RuntimeExecutionPersistenceErrorV1::InvalidInput)?,
            owner_revision: i64::try_from(request.expected_owner_revision.get())
                .map_err(|_| RuntimeExecutionPersistenceErrorV1::InvalidInput)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use automation_runtime_controller::{
        GatewayShardIdV1, RuntimeBuildRevisionV1, RuntimeGatewayOwnerLeaseIdV1,
        RuntimeRecoveryIdV2, RuntimeStartupRecoveryObservationCorrelationV2,
    };
    use automation_runtime_convergence::ProcessInstanceId;
    use chrono::{DateTime, Utc};

    use super::*;

    fn request(
        lease_epoch: u64,
        owner_revision: u64,
    ) -> RuntimeStartupRecoveryObservationRequestV2 {
        RuntimeStartupRecoveryObservationRequestV2 {
            correlation: RuntimeStartupRecoveryObservationCorrelationV2 {
                recovery_id: RuntimeRecoveryIdV2::parse("0123456789abcdef0123456789abcdef")
                    .unwrap(),
                originating_emergency_generation: NonZeroU64::MIN,
                coordinator_generation: NonZeroU64::MIN,
                authority_revision: NonZeroU64::MIN,
            },
            gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1 {
                gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
                process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
                lease_epoch: NonZeroU64::new(lease_epoch).unwrap(),
                expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
            },
            expected_owner_revision: NonZeroU64::new(owner_revision).unwrap(),
            expected_owner_expires_at: DateTime::<Utc>::from_timestamp(200, 0).unwrap(),
        }
    }

    #[test]
    fn request_bindings_accept_the_postgres_positive_range() {
        let bindings =
            RuntimeStartupRecoveryObservationBindingsV2::from_request(&request(5, 6)).unwrap();
        assert_eq!(bindings.lease_epoch, 5);
        assert_eq!(bindings.owner_revision, 6);
    }

    #[test]
    fn request_bindings_reject_values_outside_the_postgres_range() {
        let too_large = u64::try_from(i64::MAX).unwrap() + 1;
        assert!(matches!(
            RuntimeStartupRecoveryObservationBindingsV2::from_request(&request(too_large, 1)),
            Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
        ));
        assert!(matches!(
            RuntimeStartupRecoveryObservationBindingsV2::from_request(&request(1, too_large)),
            Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
        ));
    }
}
