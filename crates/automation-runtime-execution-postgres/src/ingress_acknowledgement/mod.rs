mod query;
mod row;

use std::future::Future;

use automation_runtime_controller::{
    RuntimeObserveIngressOpenAcknowledgementV2, RuntimeObservedIngressOpenAcknowledgementV2,
    RuntimePublishIngressOpenAcknowledgementOutcomeV2, RuntimePublishIngressOpenAcknowledgementV2,
};
use automation_runtime_worker::{
    RuntimeAuthorizedIngressOpenAcknowledgementV2,
    RuntimeIngressOpenAcknowledgementAttemptCompletionV2,
    RuntimeIngressOpenAcknowledgementAttemptV2, RuntimeIngressOpenAcknowledgementMutationErrorV2,
    RuntimeIngressOpenAcknowledgementObservationErrorClassV2,
    RuntimeIngressOpenAcknowledgementPortV2,
    RuntimeIngressOpenAcknowledgementPredecessorObservationAuthorizationV2,
};
use sqlx::{Postgres, Transaction};

use self::query::{
    OBSERVE_INGRESS_OPEN_ACKNOWLEDGEMENT_QUERY, PUBLISH_INGRESS_OPEN_ACKNOWLEDGEMENT_QUERY,
};
use self::row::RuntimeIngressOpenAcknowledgementOperationRowV2;
use crate::connection::ExecutionConnectionGuardV1;
use crate::database::{
    begin_execution_mutation_transaction, begin_execution_serializable_observation_transaction,
    verify_runtime_execution_binding_v1,
};
use crate::error::{map_mutation_commit_error, map_query_error};
use crate::{PostgresRuntimeExecutionV1, RuntimeExecutionPersistenceErrorV1};

const MIN_CANONICAL_REQUEST_BYTES_WITHOUT_SOURCE: usize = 197;
const MAX_CANONICAL_REQUEST_BYTES_WITHOUT_SOURCE: usize = 578;
const MIN_CANONICAL_REQUEST_BYTES_WITH_SOURCE: usize = 205;
const MAX_CANONICAL_REQUEST_BYTES_WITH_SOURCE: usize = 586;

impl PostgresRuntimeExecutionV1 {
    async fn observe_ingress_open_acknowledgement_v2(
        &self,
        request: &RuntimeObserveIngressOpenAcknowledgementV2,
    ) -> Result<RuntimeObservedIngressOpenAcknowledgementV2, RuntimeExecutionPersistenceErrorV1>
    {
        validate_gateway_shard(&request.gateway_shard_id)?;
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
            begin_execution_serializable_observation_transaction(
                database_connection,
                self.timeouts,
            ),
        )
        .await
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::Timeout)??;
        tokio::time::timeout_at(
            deadline,
            verify_runtime_execution_binding_v1(&mut transaction, &self.expectation),
        )
        .await
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::Timeout)??;
        let observation = tokio::time::timeout_at(
            deadline,
            execute_observe_query(&mut transaction, request.gateway_shard_id.as_str()),
        )
        .await
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::Timeout)??;
        tokio::time::timeout_at(deadline, transaction.commit())
            .await
            .map_err(|_| RuntimeExecutionPersistenceErrorV1::Timeout)?
            .map_err(map_query_error)?;
        connection.release_to_pool();
        Ok(observation)
    }

    async fn publish_ingress_open_acknowledgement_v2(
        &self,
        authorization: &RuntimeAuthorizedIngressOpenAcknowledgementV2,
    ) -> Result<
        RuntimePublishIngressOpenAcknowledgementOutcomeV2,
        RuntimeIngressOpenAcknowledgementMutationErrorV2<RuntimeExecutionPersistenceErrorV1>,
    > {
        let request = authorization.request();
        validate_publish_request(request).map_err(definitely_not_applied)?;
        let deadline = tokio::time::Instant::now() + self.timeouts.statement_timeout();
        let connection = tokio::time::timeout_at(deadline, self.pool.acquire())
            .await
            .map_err(|_| definitely_not_applied(RuntimeExecutionPersistenceErrorV1::Timeout))?
            .map_err(|error| definitely_not_applied(map_query_error(error)))?;
        let mut connection = ExecutionConnectionGuardV1::new(connection);
        let database_connection = connection.connection_mut().ok_or_else(|| {
            definitely_not_applied(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        })?;
        let mut transaction = tokio::time::timeout_at(
            deadline,
            begin_execution_mutation_transaction(database_connection, self.timeouts),
        )
        .await
        .map_err(|_| definitely_not_applied(RuntimeExecutionPersistenceErrorV1::Timeout))?
        .map_err(definitely_not_applied)?;
        tokio::time::timeout_at(
            deadline,
            verify_runtime_execution_binding_v1(&mut transaction, &self.expectation),
        )
        .await
        .map_err(|_| definitely_not_applied(RuntimeExecutionPersistenceErrorV1::Timeout))?
        .map_err(definitely_not_applied)?;
        let outcome =
            tokio::time::timeout_at(deadline, execute_publish_query(&mut transaction, request))
                .await
                .map_err(|_| outcome_unknown(RuntimeExecutionPersistenceErrorV1::Timeout))?
                .map_err(definitely_not_applied)?;
        tokio::time::timeout_at(deadline, transaction.commit())
            .await
            .map_err(|_| outcome_unknown(RuntimeExecutionPersistenceErrorV1::Indeterminate))?
            .map_err(|error| outcome_unknown(map_mutation_commit_error(error)))?;
        connection.release_to_pool();
        Ok(outcome)
    }
}

impl RuntimeIngressOpenAcknowledgementPortV2 for PostgresRuntimeExecutionV1 {
    type Error = RuntimeExecutionPersistenceErrorV1;

    fn classify_observation_error(
        error: &Self::Error,
    ) -> RuntimeIngressOpenAcknowledgementObservationErrorClassV2 {
        match error {
            RuntimeExecutionPersistenceErrorV1::Timeout
            | RuntimeExecutionPersistenceErrorV1::Concurrency
            | RuntimeExecutionPersistenceErrorV1::Unavailable
            | RuntimeExecutionPersistenceErrorV1::Indeterminate
            | RuntimeExecutionPersistenceErrorV1::RetryNotReady => {
                RuntimeIngressOpenAcknowledgementObservationErrorClassV2::Retryable
            }
            RuntimeExecutionPersistenceErrorV1::OwnershipLost
            | RuntimeExecutionPersistenceErrorV1::AuthorityChanged
            | RuntimeExecutionPersistenceErrorV1::Superseded => {
                RuntimeIngressOpenAcknowledgementObservationErrorClassV2::AuthorityLost
            }
            RuntimeExecutionPersistenceErrorV1::InvalidInput
            | RuntimeExecutionPersistenceErrorV1::DatabaseAuthorityMismatch
            | RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt
            | RuntimeExecutionPersistenceErrorV1::DatabaseFailure
            | RuntimeExecutionPersistenceErrorV1::ObservationAmbiguous => {
                RuntimeIngressOpenAcknowledgementObservationErrorClassV2::ProtocolViolation
            }
        }
    }

    fn observe_ingress_open_acknowledgement_predecessor(
        &self,
        authorization: &RuntimeIngressOpenAcknowledgementPredecessorObservationAuthorizationV2,
    ) -> impl Future<Output = Result<RuntimeObservedIngressOpenAcknowledgementV2, Self::Error>> + Send
    {
        self.observe_ingress_open_acknowledgement_v2(authorization.request())
    }

    async fn publish_ingress_open_acknowledgement<'a>(
        &'a self,
        attempt: RuntimeIngressOpenAcknowledgementAttemptV2<'a>,
    ) -> RuntimeIngressOpenAcknowledgementAttemptCompletionV2<'a, Self::Error> {
        let result = self
            .publish_ingress_open_acknowledgement_v2(attempt.authorization())
            .await;
        RuntimeIngressOpenAcknowledgementAttemptCompletionV2::new(attempt, result)
    }

    fn observe_ingress_open_acknowledgement<'a>(
        &'a self,
        attempt: &'a RuntimeIngressOpenAcknowledgementAttemptV2<'_>,
    ) -> impl Future<Output = Result<RuntimeObservedIngressOpenAcknowledgementV2, Self::Error>> + Send
    {
        let request = attempt.authorization().observation_request();
        async move { self.observe_ingress_open_acknowledgement_v2(&request).await }
    }
}

async fn execute_observe_query(
    transaction: &mut Transaction<'_, Postgres>,
    gateway_shard_id: &str,
) -> Result<RuntimeObservedIngressOpenAcknowledgementV2, RuntimeExecutionPersistenceErrorV1> {
    let mut rows = sqlx::query_as::<_, RuntimeIngressOpenAcknowledgementOperationRowV2>(
        OBSERVE_INGRESS_OPEN_ACKNOWLEDGEMENT_QUERY,
    )
    .bind(gateway_shard_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(map_query_error)?;
    if rows.len() != 1 {
        return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
    }
    rows.pop()
        .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?
        .decode_observation()
}

async fn execute_publish_query(
    transaction: &mut Transaction<'_, Postgres>,
    request: &RuntimePublishIngressOpenAcknowledgementV2,
) -> Result<RuntimePublishIngressOpenAcknowledgementOutcomeV2, RuntimeExecutionPersistenceErrorV1> {
    let owner = request.owner_receipt();
    let ready = request.gateway_ready();
    let source_revision = request
        .source_acknowledgement_revision()
        .map(|revision| i64::try_from(revision.get()))
        .transpose()
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::InvalidInput)?;
    let mut rows = sqlx::query_as::<_, RuntimeIngressOpenAcknowledgementOperationRowV2>(
        PUBLISH_INGRESS_OPEN_ACKNOWLEDGEMENT_QUERY,
    )
    .bind(owner.lease_id.gateway_shard_id.as_str())
    .bind(source_revision)
    .bind(request.request_digest().as_bytes().as_slice())
    .bind(request.canonical_request_bytes())
    .bind(to_i64(request.fence_generation().get())?)
    .bind(to_i64(request.maintenance_gate_generation().get())?)
    .bind(owner.lease_id.process_instance_id.as_str())
    .bind(to_i64(owner.lease_id.lease_epoch.get())?)
    .bind(owner.lease_id.expected_build_revision.as_str())
    .bind(to_i64(owner.owner_revision.get())?)
    .bind(owner.database_now)
    .bind(owner.expires_at)
    .bind(to_i64(ready.connection_epoch.get())?)
    .bind(to_i64(ready.admission_revision.get())?)
    .bind(to_i64(ready.connected_event_sequence.get())?)
    .bind(to_i64(ready.resume_sequence.get())?)
    .bind(to_i64(request.lease_for().milliseconds())?)
    .fetch_all(&mut **transaction)
    .await
    .map_err(map_query_error)?;
    if rows.len() != 1 {
        return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
    }
    rows.pop()
        .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?
        .decode_publish(request)
}

fn validate_publish_request(
    request: &RuntimePublishIngressOpenAcknowledgementV2,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    validate_gateway_shard(&request.owner_receipt().lease_id.gateway_shard_id)?;
    let (minimum_request_bytes, maximum_request_bytes) =
        if request.source_acknowledgement_revision().is_some() {
            (
                MIN_CANONICAL_REQUEST_BYTES_WITH_SOURCE,
                MAX_CANONICAL_REQUEST_BYTES_WITH_SOURCE,
            )
        } else {
            (
                MIN_CANONICAL_REQUEST_BYTES_WITHOUT_SOURCE,
                MAX_CANONICAL_REQUEST_BYTES_WITHOUT_SOURCE,
            )
        };
    if request.canonical_request_bytes().len() < minimum_request_bytes
        || request.canonical_request_bytes().len() > maximum_request_bytes
        || request.owner_receipt().expires_at <= request.owner_receipt().database_now
    {
        return Err(RuntimeExecutionPersistenceErrorV1::InvalidInput);
    }
    Ok(())
}

fn validate_gateway_shard(
    shard: &automation_runtime_controller::GatewayShardIdV1,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    if shard.as_str() == "shard:0" {
        Ok(())
    } else {
        Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
    }
}

fn to_i64(value: u64) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
    i64::try_from(value).map_err(|_| RuntimeExecutionPersistenceErrorV1::InvalidInput)
}

fn definitely_not_applied(
    source: RuntimeExecutionPersistenceErrorV1,
) -> RuntimeIngressOpenAcknowledgementMutationErrorV2<RuntimeExecutionPersistenceErrorV1> {
    RuntimeIngressOpenAcknowledgementMutationErrorV2::DefinitelyNotApplied { source }
}

fn outcome_unknown(
    source: RuntimeExecutionPersistenceErrorV1,
) -> RuntimeIngressOpenAcknowledgementMutationErrorV2<RuntimeExecutionPersistenceErrorV1> {
    RuntimeIngressOpenAcknowledgementMutationErrorV2::OutcomeUnknown { source }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingress_acknowledgement_error_classes_are_finite() {
        for error in [
            RuntimeExecutionPersistenceErrorV1::Timeout,
            RuntimeExecutionPersistenceErrorV1::Concurrency,
            RuntimeExecutionPersistenceErrorV1::Unavailable,
            RuntimeExecutionPersistenceErrorV1::Indeterminate,
            RuntimeExecutionPersistenceErrorV1::RetryNotReady,
        ] {
            assert_eq!(
                PostgresRuntimeExecutionV1::classify_observation_error(&error),
                RuntimeIngressOpenAcknowledgementObservationErrorClassV2::Retryable
            );
        }
        for error in [
            RuntimeExecutionPersistenceErrorV1::OwnershipLost,
            RuntimeExecutionPersistenceErrorV1::AuthorityChanged,
            RuntimeExecutionPersistenceErrorV1::Superseded,
        ] {
            assert_eq!(
                PostgresRuntimeExecutionV1::classify_observation_error(&error),
                RuntimeIngressOpenAcknowledgementObservationErrorClassV2::AuthorityLost
            );
        }
        for error in [
            RuntimeExecutionPersistenceErrorV1::InvalidInput,
            RuntimeExecutionPersistenceErrorV1::DatabaseAuthorityMismatch,
            RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt,
            RuntimeExecutionPersistenceErrorV1::DatabaseFailure,
            RuntimeExecutionPersistenceErrorV1::ObservationAmbiguous,
        ] {
            assert_eq!(
                PostgresRuntimeExecutionV1::classify_observation_error(&error),
                RuntimeIngressOpenAcknowledgementObservationErrorClassV2::ProtocolViolation
            );
        }
    }

    #[test]
    fn canonical_request_bound_is_narrow() {
        assert_eq!(MIN_CANONICAL_REQUEST_BYTES_WITHOUT_SOURCE, 197);
        assert_eq!(MAX_CANONICAL_REQUEST_BYTES_WITHOUT_SOURCE, 578);
        assert_eq!(MIN_CANONICAL_REQUEST_BYTES_WITH_SOURCE, 205);
        assert_eq!(MAX_CANONICAL_REQUEST_BYTES_WITH_SOURCE, 586);
        assert_eq!(to_i64(1), Ok(1));
        assert_eq!(
            to_i64(i64::MAX as u64 + 1),
            Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
        );
    }
}
