use std::time::Duration;

use automation_runtime_controller::RuntimeConvergenceErrorClassV1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RuntimeExecutionPersistenceErrorV1 {
    #[error("runtime execution input is invalid")]
    InvalidInput,
    #[error("runtime execution database authority does not match")]
    DatabaseAuthorityMismatch,
    #[error("runtime execution ownership was lost")]
    OwnershipLost,
    #[error("runtime execution product authority changed")]
    AuthorityChanged,
    #[error("runtime execution persistence state is corrupt")]
    PersistenceCorrupt,
    #[error("runtime execution is not ready")]
    RetryNotReady,
    #[error("runtime execution target was superseded")]
    Superseded,
    #[error("runtime execution database operation timed out")]
    Timeout,
    #[error("runtime execution database transaction must be retried")]
    Concurrency,
    #[error("runtime execution database is unavailable")]
    Unavailable,
    #[error("runtime execution database operation failed")]
    DatabaseFailure,
    #[error("runtime execution persistence outcome is indeterminate")]
    Indeterminate,
}

impl RuntimeExecutionPersistenceErrorV1 {
    pub fn class(self) -> RuntimeConvergenceErrorClassV1 {
        match self {
            Self::Timeout | Self::Concurrency | Self::Unavailable | Self::Indeterminate => {
                RuntimeConvergenceErrorClassV1::Retryable
            }
            Self::RetryNotReady => RuntimeConvergenceErrorClassV1::RetryNotReady,
            Self::OwnershipLost => RuntimeConvergenceErrorClassV1::OwnershipLost,
            Self::Superseded => RuntimeConvergenceErrorClassV1::Superseded,
            Self::AuthorityChanged => RuntimeConvergenceErrorClassV1::AuthorityBlocked,
            Self::InvalidInput
            | Self::DatabaseAuthorityMismatch
            | Self::PersistenceCorrupt
            | Self::DatabaseFailure => RuntimeConvergenceErrorClassV1::InvalidState,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidInput => "runtime_execution_invalid_input",
            Self::DatabaseAuthorityMismatch => "runtime_execution_database_authority_mismatch",
            Self::OwnershipLost => "runtime_execution_ownership_lost",
            Self::AuthorityChanged => "runtime_execution_authority_changed",
            Self::PersistenceCorrupt => "runtime_execution_persistence_corrupt",
            Self::RetryNotReady => "runtime_execution_retry_not_ready",
            Self::Superseded => "runtime_execution_superseded",
            Self::Timeout => "runtime_execution_timeout",
            Self::Concurrency => "runtime_execution_concurrency",
            Self::Unavailable => "runtime_execution_unavailable",
            Self::DatabaseFailure => "runtime_execution_database_failure",
            Self::Indeterminate => "runtime_execution_indeterminate",
        }
    }
}

pub(crate) fn validate_millisecond_duration(
    duration: Duration,
    maximum: Duration,
) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
    if duration.is_zero()
        || duration > maximum
        || !duration.subsec_nanos().is_multiple_of(1_000_000)
        || duration.as_millis() > i64::MAX as u128
    {
        return Err(RuntimeExecutionPersistenceErrorV1::InvalidInput);
    }
    Ok(duration.as_millis() as i64)
}

pub(crate) fn map_query_error(error: sqlx::Error) -> RuntimeExecutionPersistenceErrorV1 {
    let code = error
        .as_database_error()
        .and_then(|database| database.code())
        .map(|code| code.into_owned());
    if let Some(mapped) = code.as_deref().and_then(map_operation_sqlstate) {
        return mapped;
    }
    match error {
        sqlx::Error::PoolTimedOut => RuntimeExecutionPersistenceErrorV1::Timeout,
        sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed
        | sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::Protocol(_) => RuntimeExecutionPersistenceErrorV1::Unavailable,
        sqlx::Error::RowNotFound
        | sqlx::Error::TypeNotFound { .. }
        | sqlx::Error::ColumnIndexOutOfBounds { .. }
        | sqlx::Error::ColumnNotFound(_)
        | sqlx::Error::ColumnDecode { .. }
        | sqlx::Error::Decode(_) => RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt,
        _ => RuntimeExecutionPersistenceErrorV1::DatabaseFailure,
    }
}

pub(crate) fn map_readiness_query_error(error: sqlx::Error) -> RuntimeExecutionPersistenceErrorV1 {
    if error
        .as_database_error()
        .and_then(|database| database.code())
        .is_some_and(|code| code.as_ref() == "RE001")
    {
        RuntimeExecutionPersistenceErrorV1::DatabaseAuthorityMismatch
    } else {
        map_query_error(error)
    }
}

pub(crate) fn map_mutation_commit_error(_error: sqlx::Error) -> RuntimeExecutionPersistenceErrorV1 {
    RuntimeExecutionPersistenceErrorV1::Indeterminate
}

fn map_operation_sqlstate(code: &str) -> Option<RuntimeExecutionPersistenceErrorV1> {
    match code {
        "RX001" => Some(RuntimeExecutionPersistenceErrorV1::OwnershipLost),
        "RX002" => Some(RuntimeExecutionPersistenceErrorV1::InvalidInput),
        "RX003" => Some(RuntimeExecutionPersistenceErrorV1::AuthorityChanged),
        "RX004" => Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt),
        "RX005" => Some(RuntimeExecutionPersistenceErrorV1::RetryNotReady),
        "RX006" => Some(RuntimeExecutionPersistenceErrorV1::Superseded),
        "RX007" => Some(RuntimeExecutionPersistenceErrorV1::RetryNotReady),
        "55P03" | "57014" => Some(RuntimeExecutionPersistenceErrorV1::Timeout),
        "40001" | "40P01" => Some(RuntimeExecutionPersistenceErrorV1::Concurrency),
        "53300" | "57P01" | "57P02" | "57P03" => {
            Some(RuntimeExecutionPersistenceErrorV1::Unavailable)
        }
        code if code.starts_with("08") => Some(RuntimeExecutionPersistenceErrorV1::Unavailable),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_sqlstate_contract_is_closed() {
        let expected = [
            ("RX001", RuntimeExecutionPersistenceErrorV1::OwnershipLost),
            ("RX002", RuntimeExecutionPersistenceErrorV1::InvalidInput),
            (
                "RX003",
                RuntimeExecutionPersistenceErrorV1::AuthorityChanged,
            ),
            (
                "RX004",
                RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt,
            ),
            ("RX005", RuntimeExecutionPersistenceErrorV1::RetryNotReady),
            ("RX006", RuntimeExecutionPersistenceErrorV1::Superseded),
            ("RX007", RuntimeExecutionPersistenceErrorV1::RetryNotReady),
        ];
        for (code, error) in expected {
            assert_eq!(map_operation_sqlstate(code), Some(error));
        }
        for code in ["RE001", "RX008", "XX000"] {
            assert_eq!(map_operation_sqlstate(code), None);
        }
    }

    #[test]
    fn every_commit_error_is_indeterminate() {
        assert_eq!(
            map_mutation_commit_error(sqlx::Error::RowNotFound),
            RuntimeExecutionPersistenceErrorV1::Indeterminate
        );
        assert_eq!(
            map_mutation_commit_error(sqlx::Error::PoolClosed),
            RuntimeExecutionPersistenceErrorV1::Indeterminate
        );
    }
}
