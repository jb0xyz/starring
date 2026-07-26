use std::time::Duration;

use automation_runtime_controller::{
    RuntimeCertificationReceiptV1, RuntimeCertificationRequestV1, RuntimeClaimNextExecutionV1,
    RuntimeExecutionReceiptV1, RuntimeExecutionUpdateReceiptV1, RuntimeMutationReceiptV1,
    RuntimeMutationRequestV1, RuntimeObservePreviousServingV1,
    RuntimePreviousServingObservationReceiptV1, RuntimeRenewExecutionV1,
    RuntimeStaleLiveRecoveryReceiptV1,
};
use sqlx::types::Json;
use sqlx::{PgConnection, PgPool};

use crate::certification::{execute_certification_v1, RuntimeCertificationBindingsV1};
use crate::connection::ExecutionConnectionGuardV1;
use crate::database::{begin_execution_mutation_transaction, verify_runtime_execution_binding_v1};
use crate::error::{map_mutation_commit_error, map_query_error, validate_millisecond_duration};
use crate::mutation::{encode_runtime_mutation_v1, EncodedRuntimeMutationV1};
use crate::observation::{
    execute_observe_previous_serving_v1, RuntimeObservePreviousServingBindingsV1,
};
use crate::proof::{prove_claim_next_v1, prove_mutation_v1, prove_renew_v1};
use crate::query::{CLAIM_NEXT_QUERY, MUTATE_QUERY, RENEW_QUERY};
use crate::recovery::execute_recover_next_stale_live_v1;
use crate::row::{
    RuntimeClaimOperationRowV1, RuntimeExecutionOperationRowV1, RuntimeMutationOperationRowV1,
};
use crate::{
    verify_runtime_execution_database_with_timeouts_v1, RuntimeExecutionDatabaseExpectationV1,
    RuntimeExecutionDatabaseReadinessV1, RuntimeExecutionDatabaseTimeoutsV1,
    RuntimeExecutionPersistenceErrorV1,
};

pub const MIN_RUNTIME_EXECUTION_LEASE_DURATION: Duration = Duration::from_secs(1);
pub const MAX_RUNTIME_EXECUTION_LEASE_DURATION: Duration = Duration::from_secs(600);

#[derive(Clone)]
pub struct PostgresRuntimeExecutionV1 {
    pub(crate) pool: PgPool,
    pub(crate) expectation: RuntimeExecutionDatabaseExpectationV1,
    pub(crate) timeouts: RuntimeExecutionDatabaseTimeoutsV1,
    initial_readiness: RuntimeExecutionDatabaseReadinessV1,
}

impl PostgresRuntimeExecutionV1 {
    pub async fn connect_verified(
        pool: PgPool,
        expectation: RuntimeExecutionDatabaseExpectationV1,
        timeouts: RuntimeExecutionDatabaseTimeoutsV1,
    ) -> Result<Self, RuntimeExecutionPersistenceErrorV1> {
        let initial_readiness =
            verify_runtime_execution_database_with_timeouts_v1(&pool, &expectation, timeouts)
                .await?;
        Ok(Self {
            pool,
            expectation,
            timeouts,
            initial_readiness,
        })
    }

    pub async fn connect_verified_default(
        pool: PgPool,
        expectation: RuntimeExecutionDatabaseExpectationV1,
    ) -> Result<Self, RuntimeExecutionPersistenceErrorV1> {
        Self::connect_verified(
            pool,
            expectation,
            RuntimeExecutionDatabaseTimeoutsV1::default(),
        )
        .await
    }

    pub fn initial_readiness(&self) -> &RuntimeExecutionDatabaseReadinessV1 {
        &self.initial_readiness
    }

    pub async fn verify_database_v1(
        &self,
    ) -> Result<RuntimeExecutionDatabaseReadinessV1, RuntimeExecutionPersistenceErrorV1> {
        verify_runtime_execution_database_with_timeouts_v1(
            &self.pool,
            &self.expectation,
            self.timeouts,
        )
        .await
    }

    pub async fn claim_next_execution(
        &self,
        request: RuntimeClaimNextExecutionV1,
    ) -> Result<Option<RuntimeExecutionReceiptV1>, RuntimeExecutionPersistenceErrorV1> {
        let lease_milliseconds = validate_lease_duration(request.lease_for)?;
        match self
            .execute_operation(RuntimeExecutionOperationV1::ClaimNext {
                request: &request,
                lease_milliseconds,
            })
            .await?
        {
            RuntimeExecutionOperationReceiptV1::ClaimNext(receipt) => Ok(receipt),
            RuntimeExecutionOperationReceiptV1::Renew(_)
            | RuntimeExecutionOperationReceiptV1::Mutate(_)
            | RuntimeExecutionOperationReceiptV1::Certify(_)
            | RuntimeExecutionOperationReceiptV1::Observe(_)
            | RuntimeExecutionOperationReceiptV1::Recover(_) => {
                Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
            }
        }
    }

    pub async fn renew_execution(
        &self,
        request: RuntimeRenewExecutionV1,
    ) -> Result<RuntimeExecutionUpdateReceiptV1, RuntimeExecutionPersistenceErrorV1> {
        let lease_milliseconds = validate_lease_duration(request.lease_for)?;
        let bindings = RuntimeRenewBindingsV1::from_request(&request)?;
        let action_id = request.action_id;
        match self
            .execute_operation(RuntimeExecutionOperationV1::Renew {
                request: &request,
                lease_milliseconds,
                bindings,
            })
            .await?
        {
            RuntimeExecutionOperationReceiptV1::Renew(execution) => {
                Ok(RuntimeExecutionUpdateReceiptV1 {
                    action_id,
                    execution,
                })
            }
            RuntimeExecutionOperationReceiptV1::ClaimNext(_)
            | RuntimeExecutionOperationReceiptV1::Mutate(_)
            | RuntimeExecutionOperationReceiptV1::Certify(_)
            | RuntimeExecutionOperationReceiptV1::Observe(_)
            | RuntimeExecutionOperationReceiptV1::Recover(_) => {
                Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
            }
        }
    }

    pub async fn mutate(
        &self,
        request: RuntimeMutationRequestV1,
    ) -> Result<RuntimeMutationReceiptV1, RuntimeExecutionPersistenceErrorV1> {
        let bindings = RuntimeMutationBindingsV1::from_request(&request)?;
        let mutation = encode_runtime_mutation_v1(&request.mutation, &request.guard)?;
        match self
            .execute_operation(RuntimeExecutionOperationV1::Mutate {
                request: &request,
                mutation: &mutation,
                bindings,
            })
            .await?
        {
            RuntimeExecutionOperationReceiptV1::Mutate(receipt) => Ok(receipt),
            RuntimeExecutionOperationReceiptV1::ClaimNext(_)
            | RuntimeExecutionOperationReceiptV1::Renew(_)
            | RuntimeExecutionOperationReceiptV1::Certify(_)
            | RuntimeExecutionOperationReceiptV1::Observe(_)
            | RuntimeExecutionOperationReceiptV1::Recover(_) => {
                Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
            }
        }
    }

    pub async fn certify_live(
        &self,
        request: RuntimeCertificationRequestV1,
    ) -> Result<RuntimeCertificationReceiptV1, RuntimeExecutionPersistenceErrorV1> {
        let bindings = RuntimeCertificationBindingsV1::from_request(&request)?;
        match self
            .execute_operation(RuntimeExecutionOperationV1::Certify {
                request: &request,
                bindings,
            })
            .await?
        {
            RuntimeExecutionOperationReceiptV1::Certify(receipt) => Ok(*receipt),
            RuntimeExecutionOperationReceiptV1::ClaimNext(_)
            | RuntimeExecutionOperationReceiptV1::Renew(_)
            | RuntimeExecutionOperationReceiptV1::Mutate(_)
            | RuntimeExecutionOperationReceiptV1::Observe(_)
            | RuntimeExecutionOperationReceiptV1::Recover(_) => {
                Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
            }
        }
    }

    pub async fn observe_previous_serving(
        &self,
        request: RuntimeObservePreviousServingV1,
    ) -> Result<RuntimePreviousServingObservationReceiptV1, RuntimeExecutionPersistenceErrorV1>
    {
        let bindings = RuntimeObservePreviousServingBindingsV1::from_request(&request)?;
        match self
            .execute_operation(RuntimeExecutionOperationV1::Observe {
                request: Box::new(request),
                bindings,
            })
            .await?
        {
            RuntimeExecutionOperationReceiptV1::Observe(receipt) => Ok(*receipt),
            RuntimeExecutionOperationReceiptV1::ClaimNext(_)
            | RuntimeExecutionOperationReceiptV1::Renew(_)
            | RuntimeExecutionOperationReceiptV1::Mutate(_)
            | RuntimeExecutionOperationReceiptV1::Certify(_)
            | RuntimeExecutionOperationReceiptV1::Recover(_) => {
                Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
            }
        }
    }

    pub async fn recover_next_stale_live(
        &self,
    ) -> Result<Option<RuntimeStaleLiveRecoveryReceiptV1>, RuntimeExecutionPersistenceErrorV1> {
        match self
            .execute_operation(RuntimeExecutionOperationV1::Recover)
            .await?
        {
            RuntimeExecutionOperationReceiptV1::Recover(receipt) => {
                Ok(receipt.map(|receipt| *receipt))
            }
            RuntimeExecutionOperationReceiptV1::ClaimNext(_)
            | RuntimeExecutionOperationReceiptV1::Renew(_)
            | RuntimeExecutionOperationReceiptV1::Mutate(_)
            | RuntimeExecutionOperationReceiptV1::Certify(_)
            | RuntimeExecutionOperationReceiptV1::Observe(_) => {
                Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
            }
        }
    }

    async fn execute_operation(
        &self,
        operation: RuntimeExecutionOperationV1<'_>,
    ) -> Result<RuntimeExecutionOperationReceiptV1, RuntimeExecutionPersistenceErrorV1> {
        let mutates = operation.mutates();
        let deadline = tokio::time::Instant::now() + self.timeouts.statement_timeout();
        let connection = tokio::time::timeout_at(deadline, self.pool.acquire())
            .await
            .map_err(|_| RuntimeExecutionPersistenceErrorV1::Timeout)?
            .map_err(map_query_error)?;
        let mut connection = ExecutionConnectionGuardV1::new(connection);
        let database_connection = connection
            .connection_mut()
            .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
        let result = tokio::time::timeout_at(
            deadline,
            self.execute_operation_on_connection(database_connection, operation),
        )
        .await;
        match result {
            Ok(result) => {
                connection.release_to_pool();
                result
            }
            Err(_) if mutates => Err(RuntimeExecutionPersistenceErrorV1::Indeterminate),
            Err(_) => Err(RuntimeExecutionPersistenceErrorV1::Timeout),
        }
    }

    async fn execute_operation_on_connection(
        &self,
        connection: &mut PgConnection,
        operation: RuntimeExecutionOperationV1<'_>,
    ) -> Result<RuntimeExecutionOperationReceiptV1, RuntimeExecutionPersistenceErrorV1> {
        let mutates = operation.mutates();
        let mut transaction =
            begin_execution_mutation_transaction(connection, self.timeouts).await?;
        verify_runtime_execution_binding_v1(&mut transaction, &self.expectation).await?;
        let receipt = match operation {
            RuntimeExecutionOperationV1::ClaimNext {
                request,
                lease_milliseconds,
            } => {
                let rows = sqlx::query_as::<_, RuntimeClaimOperationRowV1>(CLAIM_NEXT_QUERY)
                    .bind(request.controller_id.as_str())
                    .bind(lease_milliseconds)
                    .fetch_all(&mut *transaction)
                    .await
                    .map_err(map_query_error)?;
                let receipt = match rows.len() {
                    0 => None,
                    1 => Some(prove_claim_next_v1(
                        rows.into_iter()
                            .next()
                            .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?
                            .decode()?,
                        request,
                    )?),
                    _ => return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt),
                };
                RuntimeExecutionOperationReceiptV1::ClaimNext(receipt)
            }
            RuntimeExecutionOperationV1::Renew {
                request,
                lease_milliseconds,
                bindings,
            } => {
                let guard = &request.guard;
                let mut rows = sqlx::query_as::<_, RuntimeExecutionOperationRowV1>(RENEW_QUERY)
                    .bind(guard.scope.tenant_id.as_str())
                    .bind(guard.scope.installation_id.as_str())
                    .bind(guard.scope.deployment_id.as_str())
                    .bind(bindings.expected_revision)
                    .bind(guard.controller_id.as_str())
                    .bind(bindings.fencing_token)
                    .bind(bindings.convergence_attempt)
                    .bind(bindings.runtime_generation)
                    .bind(lease_milliseconds)
                    .fetch_all(&mut *transaction)
                    .await
                    .map_err(map_query_error)?;
                if rows.len() != 1 {
                    return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
                }
                let receipt = prove_renew_v1(
                    rows.pop()
                        .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?
                        .decode()?,
                    guard,
                    request.lease_for,
                )?;
                RuntimeExecutionOperationReceiptV1::Renew(receipt)
            }
            RuntimeExecutionOperationV1::Mutate {
                request,
                mutation,
                bindings,
            } => {
                let guard = &request.guard;
                let mut rows = sqlx::query_as::<_, RuntimeMutationOperationRowV1>(MUTATE_QUERY)
                    .bind(guard.scope.tenant_id.as_str())
                    .bind(guard.scope.installation_id.as_str())
                    .bind(guard.scope.deployment_id.as_str())
                    .bind(bindings.expected_revision)
                    .bind(guard.controller_id.as_str())
                    .bind(bindings.fencing_token)
                    .bind(bindings.convergence_attempt)
                    .bind(bindings.runtime_generation)
                    .bind(mutation.kind)
                    .bind(Json(mutation.payload.clone()))
                    .fetch_all(&mut *transaction)
                    .await
                    .map_err(map_query_error)?;
                if rows.len() != 1 {
                    return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
                }
                let receipt = prove_mutation_v1(
                    rows.pop()
                        .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?
                        .decode()?,
                    request,
                )?;
                RuntimeExecutionOperationReceiptV1::Mutate(receipt)
            }
            RuntimeExecutionOperationV1::Certify { request, bindings } => {
                RuntimeExecutionOperationReceiptV1::Certify(Box::new(
                    execute_certification_v1(&mut transaction, request, &bindings).await?,
                ))
            }
            RuntimeExecutionOperationV1::Observe { request, bindings } => {
                RuntimeExecutionOperationReceiptV1::Observe(Box::new(
                    execute_observe_previous_serving_v1(&mut transaction, *request, bindings)
                        .await?,
                ))
            }
            RuntimeExecutionOperationV1::Recover => RuntimeExecutionOperationReceiptV1::Recover(
                execute_recover_next_stale_live_v1(&mut transaction)
                    .await?
                    .map(Box::new),
            ),
        };
        let commit = transaction.commit().await;
        if mutates {
            commit.map_err(map_mutation_commit_error)?;
        } else {
            commit.map_err(map_query_error)?;
        }
        Ok(receipt)
    }
}

enum RuntimeExecutionOperationV1<'a> {
    ClaimNext {
        request: &'a RuntimeClaimNextExecutionV1,
        lease_milliseconds: i64,
    },
    Renew {
        request: &'a RuntimeRenewExecutionV1,
        lease_milliseconds: i64,
        bindings: RuntimeRenewBindingsV1,
    },
    Mutate {
        request: &'a RuntimeMutationRequestV1,
        mutation: &'a EncodedRuntimeMutationV1,
        bindings: RuntimeMutationBindingsV1,
    },
    Certify {
        request: &'a RuntimeCertificationRequestV1,
        bindings: RuntimeCertificationBindingsV1,
    },
    Observe {
        request: Box<RuntimeObservePreviousServingV1>,
        bindings: RuntimeObservePreviousServingBindingsV1,
    },
    Recover,
}

impl RuntimeExecutionOperationV1<'_> {
    fn mutates(&self) -> bool {
        !matches!(self, Self::Observe { .. })
    }
}

#[derive(Clone, Copy)]
struct RuntimeRenewBindingsV1 {
    expected_revision: i64,
    fencing_token: i64,
    convergence_attempt: i64,
    runtime_generation: i64,
}

impl RuntimeRenewBindingsV1 {
    fn from_request(
        request: &RuntimeRenewExecutionV1,
    ) -> Result<Self, RuntimeExecutionPersistenceErrorV1> {
        Ok(Self {
            expected_revision: runtime_incrementable_i64(request.guard.expected_revision.get())?,
            fencing_token: runtime_incrementable_i64(request.guard.fencing_token.get())?,
            convergence_attempt: i64::from(request.guard.convergence_attempt.get()),
            runtime_generation: runtime_i64(request.guard.runtime_generation.get())?,
        })
    }
}

#[derive(Clone, Copy)]
struct RuntimeMutationBindingsV1 {
    expected_revision: i64,
    fencing_token: i64,
    convergence_attempt: i64,
    runtime_generation: i64,
}

impl RuntimeMutationBindingsV1 {
    fn from_request(
        request: &RuntimeMutationRequestV1,
    ) -> Result<Self, RuntimeExecutionPersistenceErrorV1> {
        Ok(Self {
            expected_revision: runtime_incrementable_i64(request.guard.expected_revision.get())?,
            fencing_token: runtime_i64(request.guard.fencing_token.get())?,
            convergence_attempt: i64::from(request.guard.convergence_attempt.get()),
            runtime_generation: runtime_i64(request.guard.runtime_generation.get())?,
        })
    }
}

enum RuntimeExecutionOperationReceiptV1 {
    ClaimNext(Option<RuntimeExecutionReceiptV1>),
    Renew(RuntimeExecutionReceiptV1),
    Mutate(RuntimeMutationReceiptV1),
    Certify(Box<RuntimeCertificationReceiptV1>),
    Observe(Box<RuntimePreviousServingObservationReceiptV1>),
    Recover(Option<Box<RuntimeStaleLiveRecoveryReceiptV1>>),
}

fn validate_lease_duration(duration: Duration) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
    if duration < MIN_RUNTIME_EXECUTION_LEASE_DURATION {
        return Err(RuntimeExecutionPersistenceErrorV1::InvalidInput);
    }
    validate_millisecond_duration(duration, MAX_RUNTIME_EXECUTION_LEASE_DURATION)
}

fn runtime_i64(value: u64) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
    i64::try_from(value).map_err(|_| RuntimeExecutionPersistenceErrorV1::InvalidInput)
}

fn runtime_incrementable_i64(value: u64) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
    let value = runtime_i64(value)?;
    if value == i64::MAX {
        Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_lease_duration_is_millisecond_exact_and_closed() {
        for valid in [
            Duration::from_secs(1),
            Duration::from_millis(1_001),
            Duration::from_secs(600),
        ] {
            assert_eq!(
                validate_lease_duration(valid).unwrap(),
                i64::try_from(valid.as_millis()).unwrap()
            );
        }
        for invalid in [
            Duration::ZERO,
            Duration::from_millis(999),
            Duration::from_nanos(1_000_000_001),
            Duration::from_millis(600_001),
        ] {
            assert_eq!(
                validate_lease_duration(invalid),
                Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
            );
        }
    }

    #[test]
    fn postgres_integer_binding_is_checked() {
        assert_eq!(runtime_i64(1), Ok(1));
        assert_eq!(runtime_i64(i64::MAX as u64), Ok(i64::MAX));
        assert_eq!(
            runtime_i64(i64::MAX as u64 + 1),
            Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
        );
        assert_eq!(
            runtime_incrementable_i64(i64::MAX as u64 - 1),
            Ok(i64::MAX - 1)
        );
        assert_eq!(
            runtime_incrementable_i64(i64::MAX as u64),
            Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
        );
    }
}
