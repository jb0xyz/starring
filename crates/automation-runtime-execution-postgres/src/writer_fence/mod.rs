mod query;
mod row;

use std::future::Future;

use automation_runtime_controller::{RuntimeObserveWriterFenceV1, RuntimeWriterFenceObservationV1};
use automation_runtime_worker::RuntimeWriterFenceObservationPortV1;

use self::query::OBSERVE_WRITER_FENCE_QUERY;
use self::row::RuntimeWriterFenceObservationRowV1;
use crate::connection::ExecutionConnectionGuardV1;
use crate::database::{begin_execution_mutation_transaction, verify_runtime_execution_binding_v1};
use crate::error::map_query_error;
use crate::{PostgresRuntimeExecutionV1, RuntimeExecutionPersistenceErrorV1};

impl PostgresRuntimeExecutionV1 {
    async fn observe_writer_fence_v1(
        &self,
        _request: RuntimeObserveWriterFenceV1,
    ) -> Result<RuntimeWriterFenceObservationV1, RuntimeExecutionPersistenceErrorV1> {
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
            begin_execution_mutation_transaction(database_connection, self.timeouts),
        )
        .await
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::Timeout)??;
        tokio::time::timeout_at(
            deadline,
            verify_runtime_execution_binding_v1(&mut transaction, &self.expectation),
        )
        .await
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::Timeout)??;
        let rows = tokio::time::timeout_at(
            deadline,
            sqlx::query_as::<_, RuntimeWriterFenceObservationRowV1>(OBSERVE_WRITER_FENCE_QUERY)
                .fetch_all(&mut *transaction),
        )
        .await
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::Timeout)?
        .map_err(map_query_error)?;
        let [row] = rows.as_slice() else {
            return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
        };
        let observation = row.clone().decode()?;
        tokio::time::timeout_at(deadline, transaction.commit())
            .await
            .map_err(|_| RuntimeExecutionPersistenceErrorV1::Timeout)?
            .map_err(map_query_error)?;
        connection.release_to_pool();
        Ok(observation)
    }
}

impl RuntimeWriterFenceObservationPortV1 for PostgresRuntimeExecutionV1 {
    type Error = RuntimeExecutionPersistenceErrorV1;

    fn observe_writer_fence(
        &self,
        request: RuntimeObserveWriterFenceV1,
    ) -> impl Future<Output = Result<RuntimeWriterFenceObservationV1, Self::Error>> + Send {
        self.observe_writer_fence_v1(request)
    }
}
