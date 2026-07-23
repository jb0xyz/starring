mod query;
mod row;

use std::future::Future;

use automation_runtime_controller::{
    RuntimeProductDrainScopeLookupV2, RuntimeProductDrainScopeObservationV2,
};
use automation_runtime_worker::RuntimeProductDrainObservationPortV2;

use self::query::OBSERVE_PRODUCT_DRAIN_SCOPE_QUERY;
use self::row::RuntimeProductDrainObservationRowV2;
use crate::connection::ExecutionConnectionGuardV1;
use crate::database::{
    begin_execution_locked_observation_transaction, verify_runtime_execution_binding_v1,
};
use crate::error::map_query_error;
use crate::{PostgresRuntimeExecutionV1, RuntimeExecutionPersistenceErrorV1};

impl PostgresRuntimeExecutionV1 {
    async fn observe_product_drain_scope_v2(
        &self,
        lookup: RuntimeProductDrainScopeLookupV2,
    ) -> Result<RuntimeProductDrainScopeObservationV2, RuntimeExecutionPersistenceErrorV1> {
        let deadline = tokio::time::Instant::now() + self.timeouts.statement_timeout();
        let connection = tokio::time::timeout_at(deadline, self.pool.acquire())
            .await
            .map_err(|_| RuntimeExecutionPersistenceErrorV1::Timeout)?
            .map_err(map_query_error)?;
        let mut connection = ExecutionConnectionGuardV1::new(connection);
        let database_connection = connection
            .connection_mut()
            .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
        let mut transaction = tokio::time::timeout_at(
            deadline,
            begin_execution_locked_observation_transaction(database_connection, self.timeouts),
        )
        .await
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::Timeout)??;
        tokio::time::timeout_at(
            deadline,
            verify_runtime_execution_binding_v1(&mut transaction, &self.expectation),
        )
        .await
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::Timeout)??;
        let product_scope = lookup.product_operation_scope();
        let drain_scope = lookup.drain_intent_scope();
        let expected_revision = i64::try_from(product_scope.expected_revision().get())
            .map_err(|_| RuntimeExecutionPersistenceErrorV1::InvalidInput)?;
        if drain_scope.expected_revision() != product_scope.expected_revision()
            || drain_scope.scope() != product_scope.scope()
        {
            return Err(RuntimeExecutionPersistenceErrorV1::InvalidInput);
        }
        let rows = tokio::time::timeout_at(
            deadline,
            sqlx::query_as::<_, RuntimeProductDrainObservationRowV2>(
                OBSERVE_PRODUCT_DRAIN_SCOPE_QUERY,
            )
            .bind(product_scope.scope().tenant_id.as_str())
            .bind(product_scope.scope().installation_id.as_str())
            .bind(product_scope.scope().deployment_id.as_str())
            .bind(expected_revision)
            .bind(drain_scope.slot().guild_id.to_string())
            .bind(drain_scope.slot().ruleset_key.as_str())
            .fetch_all(&mut *transaction),
        )
        .await
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::Timeout)?
        .map_err(map_query_error)?;
        let [row] = rows.as_slice() else {
            return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
        };
        let observation = row.clone().decode(lookup)?;
        tokio::time::timeout_at(deadline, transaction.commit())
            .await
            .map_err(|_| RuntimeExecutionPersistenceErrorV1::Timeout)?
            .map_err(map_query_error)?;
        connection.release_to_pool();
        Ok(observation)
    }
}

impl RuntimeProductDrainObservationPortV2 for PostgresRuntimeExecutionV1 {
    type Error = RuntimeExecutionPersistenceErrorV1;

    fn observe_product_drain_scope(
        &self,
        lookup: RuntimeProductDrainScopeLookupV2,
    ) -> impl Future<Output = Result<RuntimeProductDrainScopeObservationV2, Self::Error>> + Send
    {
        self.observe_product_drain_scope_v2(lookup)
    }
}
