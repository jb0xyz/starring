use std::time::Duration;

use automation_runtime_controller::RuntimeConvergenceErrorClassV1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RuntimeServingPersistenceErrorV1 {
    #[error("runtime serving input is invalid")]
    InvalidInput,
    #[error("runtime serving database authority does not match")]
    DatabaseAuthorityMismatch,
    #[error("runtime serving ownership was lost")]
    OwnershipLost,
    #[error("runtime serving product authority changed")]
    AuthorityChanged,
    #[error("runtime serving persistence state is corrupt")]
    PersistenceCorrupt,
    #[error("runtime serving database operation timed out")]
    Timeout,
    #[error("runtime serving database transaction must be retried")]
    Concurrency,
    #[error("runtime serving database is unavailable")]
    Unavailable,
    #[error("runtime serving database operation failed")]
    DatabaseFailure,
    #[error("runtime serving persistence outcome is indeterminate")]
    Indeterminate,
}

impl RuntimeServingPersistenceErrorV1 {
    pub fn class(self) -> RuntimeConvergenceErrorClassV1 {
        match self {
            Self::Timeout | Self::Concurrency | Self::Unavailable | Self::Indeterminate => {
                RuntimeConvergenceErrorClassV1::Retryable
            }
            Self::OwnershipLost => RuntimeConvergenceErrorClassV1::OwnershipLost,
            Self::AuthorityChanged => RuntimeConvergenceErrorClassV1::AuthorityBlocked,
            Self::InvalidInput
            | Self::DatabaseAuthorityMismatch
            | Self::PersistenceCorrupt
            | Self::DatabaseFailure => RuntimeConvergenceErrorClassV1::InvalidState,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidInput => "runtime_serving_invalid_input",
            Self::DatabaseAuthorityMismatch => "runtime_serving_database_authority_mismatch",
            Self::OwnershipLost => "runtime_serving_ownership_lost",
            Self::AuthorityChanged => "runtime_serving_authority_changed",
            Self::PersistenceCorrupt => "runtime_serving_persistence_corrupt",
            Self::Timeout => "runtime_serving_timeout",
            Self::Concurrency => "runtime_serving_concurrency",
            Self::Unavailable => "runtime_serving_unavailable",
            Self::DatabaseFailure => "runtime_serving_database_failure",
            Self::Indeterminate => "runtime_serving_indeterminate",
        }
    }
}

pub(crate) fn validate_millisecond_duration(
    duration: Duration,
    maximum: Duration,
) -> Result<i64, RuntimeServingPersistenceErrorV1> {
    if duration.is_zero()
        || duration > maximum
        || !duration.subsec_nanos().is_multiple_of(1_000_000)
        || duration.as_millis() > i64::MAX as u128
    {
        return Err(RuntimeServingPersistenceErrorV1::InvalidInput);
    }
    Ok(duration.as_millis() as i64)
}

pub(crate) fn map_query_error(error: sqlx::Error) -> RuntimeServingPersistenceErrorV1 {
    let code = error
        .as_database_error()
        .and_then(|database| database.code())
        .map(|code| code.into_owned());
    match code.as_deref() {
        Some("RS001") => RuntimeServingPersistenceErrorV1::OwnershipLost,
        Some("RS002") => RuntimeServingPersistenceErrorV1::InvalidInput,
        Some("RS003") => RuntimeServingPersistenceErrorV1::AuthorityChanged,
        Some("RS004") => RuntimeServingPersistenceErrorV1::PersistenceCorrupt,
        Some("RE001") => RuntimeServingPersistenceErrorV1::DatabaseAuthorityMismatch,
        Some("55P03" | "57014") => RuntimeServingPersistenceErrorV1::Timeout,
        Some("40001" | "40P01") => RuntimeServingPersistenceErrorV1::Concurrency,
        Some(code) if code.starts_with("08") => RuntimeServingPersistenceErrorV1::Unavailable,
        Some("53300" | "57P01" | "57P02" | "57P03") => {
            RuntimeServingPersistenceErrorV1::Unavailable
        }
        _ => match error {
            sqlx::Error::PoolTimedOut => RuntimeServingPersistenceErrorV1::Timeout,
            sqlx::Error::PoolClosed
            | sqlx::Error::WorkerCrashed
            | sqlx::Error::Io(_)
            | sqlx::Error::Tls(_)
            | sqlx::Error::Protocol(_) => RuntimeServingPersistenceErrorV1::Unavailable,
            sqlx::Error::RowNotFound
            | sqlx::Error::TypeNotFound { .. }
            | sqlx::Error::ColumnIndexOutOfBounds { .. }
            | sqlx::Error::ColumnNotFound(_)
            | sqlx::Error::ColumnDecode { .. }
            | sqlx::Error::Decode(_) => RuntimeServingPersistenceErrorV1::PersistenceCorrupt,
            _ => RuntimeServingPersistenceErrorV1::DatabaseFailure,
        },
    }
}

pub(crate) fn map_mutation_error(error: sqlx::Error) -> RuntimeServingPersistenceErrorV1 {
    if mutation_outcome_is_indeterminate(&error) {
        RuntimeServingPersistenceErrorV1::Indeterminate
    } else {
        map_query_error(error)
    }
}

pub(crate) fn map_mutation_commit_error(_error: sqlx::Error) -> RuntimeServingPersistenceErrorV1 {
    RuntimeServingPersistenceErrorV1::Indeterminate
}

fn mutation_outcome_is_indeterminate(error: &sqlx::Error) -> bool {
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
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed => true,
        sqlx::Error::PoolTimedOut
        | sqlx::Error::Configuration(_)
        | sqlx::Error::RowNotFound
        | sqlx::Error::TypeNotFound { .. }
        | sqlx::Error::ColumnIndexOutOfBounds { .. }
        | sqlx::Error::ColumnNotFound(_)
        | sqlx::Error::ColumnDecode { .. }
        | sqlx::Error::Decode(_)
        | sqlx::Error::AnyDriverError(_)
        | sqlx::Error::Migrate(_) => false,
        _ => false,
    }
}
