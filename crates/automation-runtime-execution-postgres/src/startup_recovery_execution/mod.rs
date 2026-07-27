mod digest;
mod projection;
mod query;
mod row;
mod semantic;

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use automation_runtime_worker::{
    RuntimeAuthorizedStartupRecoveryExecutionV2, RuntimeCompletedStartupRecoveryExecutionV2,
    RuntimeStartupRecoveryClassV2, RuntimeStartupRecoveryExecutionPortV2,
    RuntimeStartupRecoveryExecutionReceiptOutcomeV2, RuntimeStartupRecoveryExecutionReceiptV2,
    RuntimeStartupRecoveryExecutionRequestV2,
};
use sqlx::{PgConnection, Postgres, Transaction};

use self::query::EXECUTE_STALE_LIVE_STARTUP_RECOVERY_QUERY;
use self::row::{
    RuntimeStartupRecoveryExecutionDatabaseOutcomeV2, RuntimeStartupRecoveryExecutionExpectedV2,
    RuntimeStartupRecoveryExecutionRowV2,
};
use crate::connection::ExecutionConnectionGuardV1;
use crate::database::{begin_execution_mutation_transaction, verify_runtime_execution_binding_v1};
use crate::error::{map_mutation_commit_error, map_query_error};
use crate::{PostgresRuntimeExecutionV1, RuntimeExecutionPersistenceErrorV1};

impl PostgresRuntimeExecutionV1 {
    async fn execute_startup_recovery_v2(
        &self,
        authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
        operation_cutoff: Instant,
    ) -> Result<RuntimeCompletedStartupRecoveryExecutionV2, RuntimeExecutionPersistenceErrorV1>
    {
        if authorization.request().class() != RuntimeStartupRecoveryClassV2::StaleLive {
            return Err(RuntimeExecutionPersistenceErrorV1::RetryNotReady);
        }
        if Instant::now() >= operation_cutoff {
            return Err(RuntimeExecutionPersistenceErrorV1::Timeout);
        }
        let bindings =
            RuntimeStartupRecoveryExecutionBindingsV2::from_request(authorization.request())?;
        let expected = bindings.expected(authorization.request());
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
            connection.release_to_pool();
            return Err(RuntimeExecutionPersistenceErrorV1::Timeout);
        }
        let database_connection = connection
            .connection_mut()
            .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
        let transaction = match tokio::time::timeout_at(
            deadline,
            self.prepare_startup_recovery_transaction_v2(database_connection),
        )
        .await
        {
            Ok(Ok(transaction)) => transaction,
            Ok(Err(error)) => return Err(error),
            Err(_) => return Err(client_cutoff_error(false)),
        };
        if Instant::now() >= effective_cutoff {
            return Err(client_cutoff_error(false));
        }
        let mutation_dispatched = AtomicBool::new(false);
        let result = tokio::time::timeout_at(
            deadline,
            self.execute_startup_recovery_dispatched_v2(
                transaction,
                authorization.request(),
                &expected,
                bindings,
                &mutation_dispatched,
            ),
        )
        .await;
        let database_receipt = match result {
            Ok(Ok(receipt)) => {
                if Instant::now() >= effective_cutoff {
                    return Err(client_cutoff_error(true));
                }
                connection.release_to_pool();
                receipt
            }
            Ok(Err(error)) => return Err(error),
            Err(_) => {
                return Err(client_cutoff_error(
                    mutation_dispatched.load(Ordering::Acquire),
                ));
            }
        };
        let outcome = match database_receipt.outcome {
            RuntimeStartupRecoveryExecutionDatabaseOutcomeV2::NoCandidate => {
                RuntimeStartupRecoveryExecutionReceiptOutcomeV2::NoCandidate
            }
            RuntimeStartupRecoveryExecutionDatabaseOutcomeV2::Progressed(terminal_digest) => {
                RuntimeStartupRecoveryExecutionReceiptOutcomeV2::Progressed {
                    action_identity: authorization.request().action_identity().clone(),
                    terminal_digest,
                }
            }
        };
        let receipt = RuntimeStartupRecoveryExecutionReceiptV2 {
            correlation: authorization.request().correlation().clone(),
            class: authorization.request().class(),
            owner_receipt: database_receipt.owner_receipt,
            outcome,
        };
        Ok(authorization.complete(receipt))
    }

    async fn prepare_startup_recovery_transaction_v2<'connection>(
        &self,
        connection: &'connection mut PgConnection,
    ) -> Result<Transaction<'connection, Postgres>, RuntimeExecutionPersistenceErrorV1> {
        let mut transaction =
            begin_execution_mutation_transaction(connection, self.timeouts).await?;
        verify_runtime_execution_binding_v1(&mut transaction, &self.expectation).await?;
        Ok(transaction)
    }

    async fn execute_startup_recovery_dispatched_v2(
        &self,
        mut transaction: Transaction<'_, Postgres>,
        request: &RuntimeStartupRecoveryExecutionRequestV2,
        expected: &RuntimeStartupRecoveryExecutionExpectedV2,
        bindings: RuntimeStartupRecoveryExecutionBindingsV2,
        mutation_dispatched: &AtomicBool,
    ) -> Result<
        row::RuntimeStartupRecoveryExecutionDatabaseReceiptV2,
        RuntimeExecutionPersistenceErrorV1,
    > {
        let correlation = request.correlation();
        let owner = request.gateway_owner_lease_id();
        mutation_dispatched.store(true, Ordering::Release);
        let mut rows = sqlx::query_as::<_, RuntimeStartupRecoveryExecutionRowV2>(
            EXECUTE_STALE_LIVE_STARTUP_RECOVERY_QUERY,
        )
        .bind(correlation.recovery_id().as_str())
        .bind(bindings.originating_emergency_generation)
        .bind(bindings.coordinator_generation)
        .bind(bindings.action_authority_revision)
        .bind(bindings.selection_authority_revision)
        .bind(owner.gateway_shard_id.as_str())
        .bind(owner.process_instance_id.as_str())
        .bind(bindings.owner_lease_epoch)
        .bind(owner.expected_build_revision.as_str())
        .bind(bindings.owner_revision)
        .bind(request.expected_owner_expires_at())
        .bind(request.minimum_database_now())
        .fetch_all(&mut *transaction)
        .await
        .map_err(map_mutation_dispatch_error)?;
        if rows.len() != 1 {
            return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
        }
        let receipt = rows
            .pop()
            .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?
            .decode(expected)?;
        transaction
            .commit()
            .await
            .map_err(map_mutation_commit_error)?;
        Ok(receipt)
    }
}

impl RuntimeStartupRecoveryExecutionPortV2 for PostgresRuntimeExecutionV1 {
    type Error = RuntimeExecutionPersistenceErrorV1;

    fn execute_startup_recovery(
        &self,
        authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
        operation_cutoff: Instant,
    ) -> impl Future<Output = Result<RuntimeCompletedStartupRecoveryExecutionV2, Self::Error>> + Send
    {
        self.execute_startup_recovery_v2(authorization, operation_cutoff)
    }
}

#[derive(Clone, Copy)]
struct RuntimeStartupRecoveryExecutionBindingsV2 {
    originating_emergency_generation: i64,
    coordinator_generation: i64,
    action_authority_revision: i64,
    selection_authority_revision: i64,
    owner_lease_epoch: i64,
    owner_revision: i64,
}

impl RuntimeStartupRecoveryExecutionBindingsV2 {
    fn from_request(
        request: &RuntimeStartupRecoveryExecutionRequestV2,
    ) -> Result<Self, RuntimeExecutionPersistenceErrorV1> {
        let correlation = request.correlation();
        let bindings = Self {
            originating_emergency_generation: positive_i64(
                correlation.originating_emergency_generation().get(),
            )?,
            coordinator_generation: positive_i64(correlation.coordinator_generation().get())?,
            action_authority_revision: positive_i64(correlation.authority_revision().get())?,
            selection_authority_revision: positive_i64(
                correlation.selection_authority_revision().get(),
            )?,
            owner_lease_epoch: positive_i64(request.gateway_owner_lease_id().lease_epoch.get())?,
            owner_revision: positive_i64(request.expected_owner_revision().get())?,
        };
        if bindings.selection_authority_revision.checked_add(1)
            != Some(bindings.action_authority_revision)
        {
            return Err(RuntimeExecutionPersistenceErrorV1::InvalidInput);
        }
        Ok(bindings)
    }

    fn expected(
        self,
        request: &RuntimeStartupRecoveryExecutionRequestV2,
    ) -> RuntimeStartupRecoveryExecutionExpectedV2 {
        RuntimeStartupRecoveryExecutionExpectedV2 {
            recovery_id: request.correlation().recovery_id().as_str().to_owned(),
            originating_emergency_generation: self.originating_emergency_generation,
            coordinator_generation: self.coordinator_generation,
            action_authority_revision: self.action_authority_revision,
            selection_authority_revision: self.selection_authority_revision,
            recovery_class: "stale_live",
            gateway_owner_lease_id: request.gateway_owner_lease_id().clone(),
            owner_revision: self.owner_revision,
            owner_expires_at: request.expected_owner_expires_at(),
            minimum_database_now: request.minimum_database_now(),
        }
    }
}

fn positive_i64(value: u64) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
    let value =
        i64::try_from(value).map_err(|_| RuntimeExecutionPersistenceErrorV1::InvalidInput)?;
    if value == 0 {
        Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
    } else {
        Ok(value)
    }
}

fn client_cutoff_error(mutation_dispatched: bool) -> RuntimeExecutionPersistenceErrorV1 {
    if mutation_dispatched {
        RuntimeExecutionPersistenceErrorV1::Indeterminate
    } else {
        RuntimeExecutionPersistenceErrorV1::Timeout
    }
}

fn map_mutation_dispatch_error(error: sqlx::Error) -> RuntimeExecutionPersistenceErrorV1 {
    let mapped = map_query_error(error);
    if mapped == RuntimeExecutionPersistenceErrorV1::Unavailable {
        RuntimeExecutionPersistenceErrorV1::Indeterminate
    } else {
        mapped
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::error::Error;
    use std::fmt::{Display, Formatter};

    use sqlx::error::{DatabaseError, ErrorKind};

    use super::*;

    #[derive(Debug)]
    struct TestDatabaseError(&'static str);

    impl Display for TestDatabaseError {
        fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("private database detail")
        }
    }

    impl Error for TestDatabaseError {}

    impl DatabaseError for TestDatabaseError {
        fn message(&self) -> &str {
            "private database detail"
        }

        fn code(&self) -> Option<Cow<'_, str>> {
            Some(Cow::Borrowed(self.0))
        }

        fn as_error(&self) -> &(dyn Error + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn Error + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn Error + Send + Sync + 'static> {
            self
        }

        fn kind(&self) -> ErrorKind {
            ErrorKind::Other
        }
    }

    fn database_error(code: &'static str) -> sqlx::Error {
        sqlx::Error::Database(Box::new(TestDatabaseError(code)))
    }

    #[test]
    fn positive_bindings_reject_zero_and_values_outside_postgres_range() {
        assert_eq!(positive_i64(1).unwrap(), 1);
        assert!(positive_i64(0).is_err());
        assert!(positive_i64(u64::try_from(i64::MAX).unwrap() + 1).is_err());
    }

    #[test]
    fn client_cutoff_changes_class_only_after_mutation_dispatch() {
        assert_eq!(
            client_cutoff_error(false),
            RuntimeExecutionPersistenceErrorV1::Timeout
        );
        assert_eq!(
            client_cutoff_error(true),
            RuntimeExecutionPersistenceErrorV1::Indeterminate
        );
    }

    #[test]
    fn dispatched_transport_is_indeterminate_but_server_cancellation_is_timeout() {
        assert_eq!(
            map_mutation_dispatch_error(database_error("08006")),
            RuntimeExecutionPersistenceErrorV1::Indeterminate
        );
        assert_eq!(
            map_mutation_dispatch_error(database_error("57P01")),
            RuntimeExecutionPersistenceErrorV1::Indeterminate
        );
        assert_eq!(
            map_mutation_dispatch_error(sqlx::Error::Io(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "transport",
            ))),
            RuntimeExecutionPersistenceErrorV1::Indeterminate
        );
        assert_eq!(
            map_mutation_dispatch_error(sqlx::Error::Protocol("transport".to_owned())),
            RuntimeExecutionPersistenceErrorV1::Indeterminate
        );
        assert_eq!(
            map_mutation_dispatch_error(database_error("57014")),
            RuntimeExecutionPersistenceErrorV1::Timeout
        );
        assert_eq!(
            map_mutation_dispatch_error(database_error("RX001")),
            RuntimeExecutionPersistenceErrorV1::OwnershipLost
        );
    }
}
