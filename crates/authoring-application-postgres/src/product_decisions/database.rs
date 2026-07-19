use authoring_application::ProductControlPortError;

use super::config::PostgresProductDecisionsConfig;
use crate::ProductDatabaseFailureV1;

pub(super) async fn configure_mutation_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    config: &PostgresProductDecisionsConfig,
) -> Result<(), ProductControlPortError> {
    configure_apply_transaction(transaction, config)
        .await
        .map_err(database_backend)
}

pub(super) async fn configure_apply_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    config: &PostgresProductDecisionsConfig,
) -> Result<(), sqlx::Error> {
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut **transaction)
        .await?;
    sqlx::query("SELECT pg_catalog.set_config('statement_timeout', $1, true)")
        .bind(config.statement_timeout())
        .execute(&mut **transaction)
        .await?;
    sqlx::query("SELECT pg_catalog.set_config('lock_timeout', $1, true)")
        .bind(config.lock_timeout())
        .execute(&mut **transaction)
        .await?;
    sqlx::query("SELECT pg_catalog.set_config('idle_in_transaction_session_timeout', $1, true)")
        .bind(config.statement_timeout())
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

pub(super) fn is_safe_transaction_retry(error: &sqlx::Error) -> bool {
    commit_failure_proves_rollback(error)
}

pub(super) fn commit_failure_proves_rollback(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|error| error.code())
        .is_some_and(|code| matches!(code.as_ref(), "40001" | "40P01"))
}

fn commit_outcome_is_uncertain(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(database) => database.code().is_some_and(|code| {
            code.as_ref() == "40003"
                || code.as_ref().starts_with("08")
                || matches!(
                    code.as_ref(),
                    "57P01" | "57P02" | "57P03" | "57P04" | "58030"
                )
        }),
        sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::Protocol(_)
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed => true,
        _ => false,
    }
}

pub(super) fn database_backend(error: sqlx::Error) -> ProductControlPortError {
    ProductControlPortError::Backend(ProductDatabaseFailureV1::classify(&error).to_string())
}

pub(super) fn database_commit(
    error: sqlx::Error,
    operation: &'static str,
) -> ProductControlPortError {
    if commit_outcome_is_uncertain(&error) {
        ProductControlPortError::Indeterminate(operation.to_string())
    } else {
        database_backend(error)
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::error::Error;
    use std::fmt::{Display, Formatter};
    use std::io;

    use sqlx::error::{DatabaseError, ErrorKind};

    use super::*;

    const COMMIT_MESSAGE: &str = "product apply commit outcome is unavailable";

    #[derive(Debug)]
    struct TestDatabaseError {
        code: &'static str,
        message: &'static str,
    }

    impl Display for TestDatabaseError {
        fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(self.message)
        }
    }

    impl Error for TestDatabaseError {}

    impl DatabaseError for TestDatabaseError {
        fn message(&self) -> &str {
            self.message
        }

        fn code(&self) -> Option<Cow<'_, str>> {
            Some(Cow::Borrowed(self.code))
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
        sqlx::Error::Database(Box::new(TestDatabaseError {
            code,
            message: "secret database detail",
        }))
    }

    #[test]
    fn only_explicit_transaction_rollback_states_are_retryable() {
        for code in ["40001", "40P01"] {
            assert!(commit_failure_proves_rollback(&database_error(code)));
            assert!(is_safe_transaction_retry(&database_error(code)));
        }
        for code in [
            "40003", "08006", "57P01", "57P04", "58030", "23514", "XX000",
        ] {
            assert!(!commit_failure_proves_rollback(&database_error(code)));
            assert!(!is_safe_transaction_retry(&database_error(code)));
        }
        for error in [
            sqlx::Error::Io(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "secret io detail",
            )),
            sqlx::Error::Tls(Box::new(io::Error::other("secret tls detail"))),
            sqlx::Error::Protocol("secret protocol detail".to_string()),
            sqlx::Error::PoolTimedOut,
            sqlx::Error::PoolClosed,
            sqlx::Error::WorkerCrashed,
        ] {
            assert!(!commit_failure_proves_rollback(&error));
            assert!(!is_safe_transaction_retry(&error));
        }
    }

    #[test]
    fn uncertain_commit_failures_are_stable_redacted_and_indeterminate() {
        for error in [
            database_error("40003"),
            database_error("08006"),
            database_error("57P01"),
            database_error("57P04"),
            database_error("58030"),
            sqlx::Error::Io(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "secret io detail",
            )),
            sqlx::Error::Tls(Box::new(io::Error::other("secret tls detail"))),
            sqlx::Error::Protocol("secret protocol detail".to_string()),
            sqlx::Error::PoolTimedOut,
            sqlx::Error::PoolClosed,
            sqlx::Error::WorkerCrashed,
        ] {
            let mapped = database_commit(error, COMMIT_MESSAGE);
            assert_eq!(
                mapped,
                ProductControlPortError::Indeterminate(COMMIT_MESSAGE.to_string())
            );
            assert!(!mapped.to_string().contains("secret"));
        }
    }

    #[test]
    fn determinate_commit_failures_remain_redacted_backend_failures() {
        for code in ["40001", "40P01", "23514", "XX000"] {
            let mapped = database_commit(database_error(code), COMMIT_MESSAGE);
            assert!(matches!(mapped, ProductControlPortError::Backend(_)));
            assert!(!mapped.to_string().contains("secret"));
        }
    }
}
