mod query;
mod row;

use std::future::Future;
use std::num::NonZeroU64;
use std::time::Duration;

use automation_runtime_controller::{
    RuntimeAcquireGatewayOwnerLeaseOutcomeV1, RuntimeAcquireGatewayOwnerLeaseV1,
    RuntimeGatewayOwnerLeaseObservationV1, RuntimeObserveGatewayOwnerLeaseV1,
    RuntimeReleaseGatewayOwnerLeaseOutcomeV1, RuntimeReleaseGatewayOwnerLeaseV1,
    RuntimeRenewGatewayOwnerLeaseOutcomeV1, RuntimeRenewGatewayOwnerLeaseV1,
};
use automation_runtime_worker::{
    accept_gateway_owner_acquire_v1, accept_gateway_owner_observation_v1,
    accept_gateway_owner_release_v1, accept_gateway_owner_renew_v1, RuntimeGatewayOwnerLeasePortV1,
    RuntimeGatewayOwnerMutationErrorV1,
};
use sqlx::{Postgres, Transaction};

use self::query::{
    ACQUIRE_GATEWAY_OWNER_QUERY, OBSERVE_GATEWAY_OWNER_QUERY, RELEASE_GATEWAY_OWNER_QUERY,
    RENEW_GATEWAY_OWNER_QUERY,
};
use self::row::RuntimeGatewayOwnerOperationRowV1;
use crate::connection::ExecutionConnectionGuardV1;
use crate::database::{begin_execution_mutation_transaction, verify_runtime_execution_binding_v1};
use crate::error::{map_mutation_commit_error, map_query_error, validate_millisecond_duration};
use crate::{PostgresRuntimeExecutionV1, RuntimeExecutionPersistenceErrorV1};

pub const MIN_RUNTIME_GATEWAY_OWNER_LEASE_DURATION: Duration = Duration::from_secs(1);
pub const MAX_RUNTIME_GATEWAY_OWNER_LEASE_DURATION: Duration = Duration::from_secs(300);

enum RuntimeGatewayOwnerOperationV1<'a> {
    Observe(&'a RuntimeObserveGatewayOwnerLeaseV1),
    Acquire {
        request: &'a RuntimeAcquireGatewayOwnerLeaseV1,
        lease_milliseconds: i64,
    },
    Renew {
        request: &'a RuntimeRenewGatewayOwnerLeaseV1,
        lease_epoch: i64,
        expected_owner_revision: i64,
        lease_milliseconds: i64,
    },
    Release {
        request: &'a RuntimeReleaseGatewayOwnerLeaseV1,
        lease_epoch: i64,
    },
}

enum RuntimeGatewayOwnerOperationOutcomeV1 {
    Observation(RuntimeGatewayOwnerLeaseObservationV1),
    Acquire(RuntimeAcquireGatewayOwnerLeaseOutcomeV1),
    Renew(RuntimeRenewGatewayOwnerLeaseOutcomeV1),
    Release(RuntimeReleaseGatewayOwnerLeaseOutcomeV1),
}

impl PostgresRuntimeExecutionV1 {
    async fn observe_gateway_owner_v1(
        &self,
        request: RuntimeObserveGatewayOwnerLeaseV1,
    ) -> Result<RuntimeGatewayOwnerLeaseObservationV1, RuntimeExecutionPersistenceErrorV1> {
        validate_gateway_owner_shard(&request.gateway_shard_id)?;
        match self
            .execute_gateway_owner_read_v1(RuntimeGatewayOwnerOperationV1::Observe(&request))
            .await?
        {
            RuntimeGatewayOwnerOperationOutcomeV1::Observation(observation) => {
                accept_gateway_owner_observation_v1(&request, observation)
                    .map_err(|_| RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
            }
            RuntimeGatewayOwnerOperationOutcomeV1::Acquire(_)
            | RuntimeGatewayOwnerOperationOutcomeV1::Renew(_)
            | RuntimeGatewayOwnerOperationOutcomeV1::Release(_) => {
                Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
            }
        }
    }

    async fn acquire_gateway_owner_v1(
        &self,
        request: RuntimeAcquireGatewayOwnerLeaseV1,
    ) -> Result<
        RuntimeAcquireGatewayOwnerLeaseOutcomeV1,
        RuntimeGatewayOwnerMutationErrorV1<RuntimeExecutionPersistenceErrorV1>,
    > {
        validate_gateway_owner_shard(&request.gateway_shard_id).map_err(definitely_not_applied)?;
        let lease_milliseconds = gateway_owner_lease_milliseconds(request.lease_for.get())
            .map_err(definitely_not_applied)?;
        let operation = RuntimeGatewayOwnerOperationV1::Acquire {
            request: &request,
            lease_milliseconds,
        };
        match self.execute_gateway_owner_mutation_v1(operation).await? {
            RuntimeGatewayOwnerOperationOutcomeV1::Acquire(outcome) => {
                accept_gateway_owner_acquire_v1(&request, outcome.clone()).map_err(|_| {
                    outcome_unknown(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
                })?;
                Ok(outcome)
            }
            RuntimeGatewayOwnerOperationOutcomeV1::Observation(_)
            | RuntimeGatewayOwnerOperationOutcomeV1::Renew(_)
            | RuntimeGatewayOwnerOperationOutcomeV1::Release(_) => Err(outcome_unknown(
                RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt,
            )),
        }
    }

    async fn renew_gateway_owner_v1(
        &self,
        request: RuntimeRenewGatewayOwnerLeaseV1,
    ) -> Result<
        RuntimeRenewGatewayOwnerLeaseOutcomeV1,
        RuntimeGatewayOwnerMutationErrorV1<RuntimeExecutionPersistenceErrorV1>,
    > {
        validate_gateway_owner_shard(&request.lease_id.gateway_shard_id)
            .map_err(definitely_not_applied)?;
        let lease_milliseconds = gateway_owner_lease_milliseconds(request.lease_for.get())
            .map_err(definitely_not_applied)?;
        let lease_epoch =
            positive_i64(request.lease_id.lease_epoch).map_err(definitely_not_applied)?;
        let expected_owner_revision =
            incrementable_i64(request.expected_owner_revision).map_err(definitely_not_applied)?;
        let operation = RuntimeGatewayOwnerOperationV1::Renew {
            request: &request,
            lease_epoch,
            expected_owner_revision,
            lease_milliseconds,
        };
        match self.execute_gateway_owner_mutation_v1(operation).await? {
            RuntimeGatewayOwnerOperationOutcomeV1::Renew(outcome) => {
                accept_gateway_owner_renew_v1(&request, outcome.clone()).map_err(|_| {
                    outcome_unknown(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
                })?;
                if let RuntimeRenewGatewayOwnerLeaseOutcomeV1::Renewed(receipt) = &outcome {
                    if receipt.database_lease_duration() != Some(request.lease_for.get()) {
                        return Err(outcome_unknown(
                            RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt,
                        ));
                    }
                }
                Ok(outcome)
            }
            RuntimeGatewayOwnerOperationOutcomeV1::Observation(_)
            | RuntimeGatewayOwnerOperationOutcomeV1::Acquire(_)
            | RuntimeGatewayOwnerOperationOutcomeV1::Release(_) => Err(outcome_unknown(
                RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt,
            )),
        }
    }

    async fn release_gateway_owner_v1(
        &self,
        request: RuntimeReleaseGatewayOwnerLeaseV1,
    ) -> Result<
        RuntimeReleaseGatewayOwnerLeaseOutcomeV1,
        RuntimeGatewayOwnerMutationErrorV1<RuntimeExecutionPersistenceErrorV1>,
    > {
        validate_gateway_owner_shard(&request.lease_id.gateway_shard_id)
            .map_err(definitely_not_applied)?;
        let lease_epoch =
            positive_i64(request.lease_id.lease_epoch).map_err(definitely_not_applied)?;
        let operation = RuntimeGatewayOwnerOperationV1::Release {
            request: &request,
            lease_epoch,
        };
        match self.execute_gateway_owner_mutation_v1(operation).await? {
            RuntimeGatewayOwnerOperationOutcomeV1::Release(outcome) => {
                accept_gateway_owner_release_v1(&request, outcome.clone()).map_err(|_| {
                    outcome_unknown(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
                })?;
                Ok(outcome)
            }
            RuntimeGatewayOwnerOperationOutcomeV1::Observation(_)
            | RuntimeGatewayOwnerOperationOutcomeV1::Acquire(_)
            | RuntimeGatewayOwnerOperationOutcomeV1::Renew(_) => Err(outcome_unknown(
                RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt,
            )),
        }
    }

    async fn execute_gateway_owner_read_v1(
        &self,
        operation: RuntimeGatewayOwnerOperationV1<'_>,
    ) -> Result<RuntimeGatewayOwnerOperationOutcomeV1, RuntimeExecutionPersistenceErrorV1> {
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
        let outcome = tokio::time::timeout_at(
            deadline,
            execute_gateway_owner_query_v1(&mut transaction, operation),
        )
        .await
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::Timeout)??;
        tokio::time::timeout_at(deadline, transaction.commit())
            .await
            .map_err(|_| RuntimeExecutionPersistenceErrorV1::Timeout)?
            .map_err(map_query_error)?;
        connection.release_to_pool();
        Ok(outcome)
    }

    async fn execute_gateway_owner_mutation_v1(
        &self,
        operation: RuntimeGatewayOwnerOperationV1<'_>,
    ) -> Result<
        RuntimeGatewayOwnerOperationOutcomeV1,
        RuntimeGatewayOwnerMutationErrorV1<RuntimeExecutionPersistenceErrorV1>,
    > {
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
        let outcome = tokio::time::timeout_at(
            deadline,
            execute_gateway_owner_query_v1(&mut transaction, operation),
        )
        .await
        .map_err(|_| outcome_unknown(RuntimeExecutionPersistenceErrorV1::Timeout))?
        .map_err(outcome_unknown)?;
        tokio::time::timeout_at(deadline, transaction.commit())
            .await
            .map_err(|_| outcome_unknown(RuntimeExecutionPersistenceErrorV1::Indeterminate))?
            .map_err(|error| outcome_unknown(map_mutation_commit_error(error)))?;
        connection.release_to_pool();
        Ok(outcome)
    }
}

impl RuntimeGatewayOwnerLeasePortV1 for PostgresRuntimeExecutionV1 {
    type Error = RuntimeExecutionPersistenceErrorV1;

    fn observe_gateway_owner(
        &self,
        request: RuntimeObserveGatewayOwnerLeaseV1,
    ) -> impl Future<Output = Result<RuntimeGatewayOwnerLeaseObservationV1, Self::Error>> + Send
    {
        self.observe_gateway_owner_v1(request)
    }

    fn acquire_gateway_owner(
        &self,
        request: RuntimeAcquireGatewayOwnerLeaseV1,
    ) -> impl Future<
        Output = Result<
            RuntimeAcquireGatewayOwnerLeaseOutcomeV1,
            RuntimeGatewayOwnerMutationErrorV1<Self::Error>,
        >,
    > + Send {
        self.acquire_gateway_owner_v1(request)
    }

    fn renew_gateway_owner(
        &self,
        request: RuntimeRenewGatewayOwnerLeaseV1,
    ) -> impl Future<
        Output = Result<
            RuntimeRenewGatewayOwnerLeaseOutcomeV1,
            RuntimeGatewayOwnerMutationErrorV1<Self::Error>,
        >,
    > + Send {
        self.renew_gateway_owner_v1(request)
    }

    fn release_gateway_owner(
        &self,
        request: RuntimeReleaseGatewayOwnerLeaseV1,
    ) -> impl Future<
        Output = Result<
            RuntimeReleaseGatewayOwnerLeaseOutcomeV1,
            RuntimeGatewayOwnerMutationErrorV1<Self::Error>,
        >,
    > + Send {
        self.release_gateway_owner_v1(request)
    }
}

async fn execute_gateway_owner_query_v1(
    transaction: &mut Transaction<'_, Postgres>,
    operation: RuntimeGatewayOwnerOperationV1<'_>,
) -> Result<RuntimeGatewayOwnerOperationOutcomeV1, RuntimeExecutionPersistenceErrorV1> {
    let mut rows = match operation {
        RuntimeGatewayOwnerOperationV1::Observe(request) => {
            sqlx::query_as::<_, RuntimeGatewayOwnerOperationRowV1>(OBSERVE_GATEWAY_OWNER_QUERY)
                .bind(request.gateway_shard_id.as_str())
                .fetch_all(&mut **transaction)
                .await
                .map_err(map_query_error)?
        }
        RuntimeGatewayOwnerOperationV1::Acquire {
            request,
            lease_milliseconds,
        } => sqlx::query_as::<_, RuntimeGatewayOwnerOperationRowV1>(ACQUIRE_GATEWAY_OWNER_QUERY)
            .bind(request.gateway_shard_id.as_str())
            .bind(request.process_instance_id.as_str())
            .bind(request.expected_build_revision.as_str())
            .bind(lease_milliseconds)
            .fetch_all(&mut **transaction)
            .await
            .map_err(map_query_error)?,
        RuntimeGatewayOwnerOperationV1::Renew {
            request,
            lease_epoch,
            expected_owner_revision,
            lease_milliseconds,
        } => sqlx::query_as::<_, RuntimeGatewayOwnerOperationRowV1>(RENEW_GATEWAY_OWNER_QUERY)
            .bind(request.lease_id.gateway_shard_id.as_str())
            .bind(request.lease_id.process_instance_id.as_str())
            .bind(lease_epoch)
            .bind(request.lease_id.expected_build_revision.as_str())
            .bind(expected_owner_revision)
            .bind(lease_milliseconds)
            .fetch_all(&mut **transaction)
            .await
            .map_err(map_query_error)?,
        RuntimeGatewayOwnerOperationV1::Release {
            request,
            lease_epoch,
        } => sqlx::query_as::<_, RuntimeGatewayOwnerOperationRowV1>(RELEASE_GATEWAY_OWNER_QUERY)
            .bind(request.lease_id.gateway_shard_id.as_str())
            .bind(request.lease_id.process_instance_id.as_str())
            .bind(lease_epoch)
            .bind(request.lease_id.expected_build_revision.as_str())
            .fetch_all(&mut **transaction)
            .await
            .map_err(map_query_error)?,
    };
    if rows.len() != 1 {
        return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
    }
    let row = rows
        .pop()
        .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
    match operation {
        RuntimeGatewayOwnerOperationV1::Observe(_) => row
            .decode_observation()
            .map(RuntimeGatewayOwnerOperationOutcomeV1::Observation),
        RuntimeGatewayOwnerOperationV1::Acquire { .. } => row
            .decode_acquire()
            .map(RuntimeGatewayOwnerOperationOutcomeV1::Acquire),
        RuntimeGatewayOwnerOperationV1::Renew { .. } => row
            .decode_renew()
            .map(RuntimeGatewayOwnerOperationOutcomeV1::Renew),
        RuntimeGatewayOwnerOperationV1::Release { .. } => row
            .decode_release()
            .map(RuntimeGatewayOwnerOperationOutcomeV1::Release),
    }
}

fn gateway_owner_lease_milliseconds(
    duration: Duration,
) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
    if duration < MIN_RUNTIME_GATEWAY_OWNER_LEASE_DURATION {
        return Err(RuntimeExecutionPersistenceErrorV1::InvalidInput);
    }
    validate_millisecond_duration(duration, MAX_RUNTIME_GATEWAY_OWNER_LEASE_DURATION)
}

fn validate_gateway_owner_shard(
    shard: &automation_runtime_controller::GatewayShardIdV1,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    if shard.as_str() == "shard:0" {
        Ok(())
    } else {
        Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
    }
}

fn positive_i64(value: NonZeroU64) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
    i64::try_from(value.get()).map_err(|_| RuntimeExecutionPersistenceErrorV1::InvalidInput)
}

fn incrementable_i64(value: NonZeroU64) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
    let value = positive_i64(value)?;
    if value == i64::MAX {
        Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
    } else {
        Ok(value)
    }
}

fn definitely_not_applied(
    source: RuntimeExecutionPersistenceErrorV1,
) -> RuntimeGatewayOwnerMutationErrorV1<RuntimeExecutionPersistenceErrorV1> {
    RuntimeGatewayOwnerMutationErrorV1::DefinitelyNotApplied { source }
}

fn outcome_unknown(
    source: RuntimeExecutionPersistenceErrorV1,
) -> RuntimeGatewayOwnerMutationErrorV1<RuntimeExecutionPersistenceErrorV1> {
    RuntimeGatewayOwnerMutationErrorV1::OutcomeUnknown { source }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_owner_lease_duration_is_millisecond_exact_and_bounded() {
        for valid in [
            Duration::from_secs(1),
            Duration::from_millis(1_001),
            Duration::from_secs(300),
        ] {
            assert_eq!(
                gateway_owner_lease_milliseconds(valid).unwrap(),
                i64::try_from(valid.as_millis()).unwrap()
            );
        }
        for invalid in [
            Duration::ZERO,
            Duration::from_millis(999),
            Duration::from_nanos(1_000_000_001),
            Duration::from_millis(300_001),
        ] {
            assert_eq!(
                gateway_owner_lease_milliseconds(invalid),
                Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
            );
        }
    }

    #[test]
    fn gateway_owner_persistence_integer_bounds_fail_closed() {
        assert_eq!(positive_i64(NonZeroU64::MIN), Ok(1));
        assert_eq!(
            positive_i64(NonZeroU64::new(i64::MAX as u64).unwrap()),
            Ok(i64::MAX)
        );
        assert_eq!(
            positive_i64(NonZeroU64::new(i64::MAX as u64 + 1).unwrap()),
            Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
        );
        assert_eq!(
            incrementable_i64(NonZeroU64::new(i64::MAX as u64).unwrap()),
            Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
        );
    }

    #[test]
    fn first_runtime_accepts_only_the_canonical_gateway_shard() {
        assert_eq!(
            validate_gateway_owner_shard(
                &automation_runtime_controller::GatewayShardIdV1::parse("shard:0").unwrap()
            ),
            Ok(())
        );
        assert_eq!(
            validate_gateway_owner_shard(
                &automation_runtime_controller::GatewayShardIdV1::parse("shard:1").unwrap()
            ),
            Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
        );
    }
}
