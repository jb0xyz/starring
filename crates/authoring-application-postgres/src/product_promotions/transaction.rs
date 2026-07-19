use authoring_application::{
    AuthorizedPromotionBackendFailureV1, AuthorizedPromotionSubmissionErrorV1,
};

use super::config::PostgresProductPromotionsConfig;
use crate::ProductDatabaseFailureV1;

pub(super) async fn configure_product_promotion_transaction_v1(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    config: &PostgresProductPromotionsConfig,
) -> Result<(), sqlx::Error> {
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED, READ WRITE")
        .execute(&mut **transaction)
        .await?;
    sqlx::query(
        "SELECT pg_catalog.set_config('statement_timeout', $1, true), \
         pg_catalog.set_config('lock_timeout', $2, true), \
         pg_catalog.set_config('idle_in_transaction_session_timeout', $3, true), \
         pg_catalog.set_config('search_path', 'pg_catalog', true), \
         pg_catalog.set_config('quote_all_identifiers', 'off', true)",
    )
    .bind(config.statement_timeout())
    .bind(config.lock_timeout())
    .bind(config.idle_transaction_timeout())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(super) fn retryable_rollback_v1(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|database| database.code())
        .is_some_and(|code| matches!(code.as_ref(), "40001" | "40P01"))
}

pub(super) fn map_product_promotion_backend_v1(
    error: &sqlx::Error,
) -> AuthorizedPromotionSubmissionErrorV1 {
    if durable_invariant_failure_v1(error) {
        return AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt;
    }
    let failure = match ProductDatabaseFailureV1::classify(error) {
        ProductDatabaseFailureV1::Timeout => AuthorizedPromotionBackendFailureV1::Timeout,
        ProductDatabaseFailureV1::Retryable => AuthorizedPromotionBackendFailureV1::Retryable,
        ProductDatabaseFailureV1::Unavailable => AuthorizedPromotionBackendFailureV1::Unavailable,
    };
    AuthorizedPromotionSubmissionErrorV1::Backend(failure)
}

pub(super) fn map_product_promotion_query_v1(
    error: &sqlx::Error,
) -> AuthorizedPromotionSubmissionErrorV1 {
    if matches!(
        error,
        sqlx::Error::RowNotFound
            | sqlx::Error::TypeNotFound { .. }
            | sqlx::Error::ColumnIndexOutOfBounds { .. }
            | sqlx::Error::ColumnNotFound(_)
            | sqlx::Error::ColumnDecode { .. }
            | sqlx::Error::Decode(_)
    ) {
        AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt
    } else {
        map_product_promotion_backend_v1(error)
    }
}

fn durable_invariant_failure_v1(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|database| database.code())
        .is_some_and(|code| {
            code.as_ref().starts_with("22")
                || code.as_ref().starts_with("23")
                || code.as_ref() == "P0001"
        })
}

pub(super) fn map_product_promotion_commit_v1(
    error: &sqlx::Error,
) -> AuthorizedPromotionSubmissionErrorV1 {
    if commit_outcome_is_uncertain_v1(error) {
        AuthorizedPromotionSubmissionErrorV1::Indeterminate
    } else {
        map_product_promotion_backend_v1(error)
    }
}

fn commit_outcome_is_uncertain_v1(error: &sqlx::Error) -> bool {
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

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::error::Error;
    use std::fmt::{Display, Formatter};
    use std::io;

    use sqlx::error::{DatabaseError, ErrorKind};

    use super::*;

    #[derive(Debug)]
    struct TestDatabaseError {
        code: &'static str,
    }

    impl Display for TestDatabaseError {
        fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("secret database detail")
        }
    }

    impl Error for TestDatabaseError {}

    impl DatabaseError for TestDatabaseError {
        fn message(&self) -> &str {
            "secret database detail"
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
        sqlx::Error::Database(Box::new(TestDatabaseError { code }))
    }

    #[test]
    fn only_proven_transaction_rollbacks_are_automatically_retryable() {
        for code in ["40001", "40P01"] {
            assert!(retryable_rollback_v1(&database_error(code)));
        }
        for code in ["40003", "08006", "57014", "55P03", "23514", "XX000"] {
            assert!(!retryable_rollback_v1(&database_error(code)));
        }
    }

    #[test]
    fn uncertain_commit_is_indeterminate_and_redacted() {
        for error in [
            database_error("40003"),
            database_error("08006"),
            database_error("57P01"),
            database_error("57P04"),
            database_error("58030"),
            sqlx::Error::Io(io::Error::new(io::ErrorKind::ConnectionReset, "secret io")),
            sqlx::Error::Protocol("secret protocol".to_string()),
            sqlx::Error::PoolTimedOut,
            sqlx::Error::PoolClosed,
            sqlx::Error::WorkerCrashed,
        ] {
            let mapped = map_product_promotion_commit_v1(&error);
            assert_eq!(mapped, AuthorizedPromotionSubmissionErrorV1::Indeterminate);
            assert!(!mapped.to_string().contains("secret"));
        }
    }

    #[test]
    fn backend_errors_use_stable_classes_without_database_detail() {
        let cases = [
            (
                database_error("57014"),
                AuthorizedPromotionBackendFailureV1::Timeout,
            ),
            (
                database_error("40001"),
                AuthorizedPromotionBackendFailureV1::Retryable,
            ),
        ];
        for (error, expected) in cases {
            let mapped = map_product_promotion_backend_v1(&error);
            assert_eq!(
                mapped,
                AuthorizedPromotionSubmissionErrorV1::Backend(expected)
            );
            assert!(!mapped.to_string().contains("secret"));
        }
    }

    #[test]
    fn durable_invariant_failures_are_persistence_corruption() {
        for code in ["22003", "23503", "23505", "23514", "P0001"] {
            let mapped = map_product_promotion_backend_v1(&database_error(code));
            assert_eq!(
                mapped,
                AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt
            );
            assert!(!mapped.to_string().contains("secret"));
        }
    }

    #[test]
    fn result_contract_decode_failures_are_persistence_corruption() {
        for error in [
            sqlx::Error::RowNotFound,
            sqlx::Error::TypeNotFound {
                type_name: "secret_type".to_string(),
            },
            sqlx::Error::ColumnIndexOutOfBounds { index: 9, len: 1 },
            sqlx::Error::ColumnNotFound("secret_column".to_string()),
            sqlx::Error::Decode(Box::new(std::io::Error::other("secret decode"))),
        ] {
            let mapped = map_product_promotion_query_v1(&error);
            assert_eq!(
                mapped,
                AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt
            );
            assert!(!mapped.to_string().contains("secret"));
        }
    }
}
