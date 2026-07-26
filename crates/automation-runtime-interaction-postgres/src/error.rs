use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionErrorClassV1 {
    InvalidInput,
    InvalidAuthority,
    Conflict,
    PersistenceCorrupt,
    Timeout,
    Unavailable,
    Indeterminate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeInteractionPersistenceErrorV1 {
    #[error("runtime interaction input is invalid")]
    InvalidInput,
    #[error("runtime interaction database authority is invalid")]
    InvalidAuthority,
    #[error("runtime interaction persistence conflicted")]
    Conflict,
    #[error("runtime interaction persistence state is corrupt")]
    PersistenceCorrupt,
    #[error("runtime interaction persistence timed out")]
    Timeout,
    #[error("runtime interaction persistence is unavailable")]
    Unavailable,
    #[error("runtime interaction persistence outcome is indeterminate")]
    Indeterminate,
}

impl RuntimeInteractionPersistenceErrorV1 {
    pub fn class(self) -> RuntimeInteractionErrorClassV1 {
        match self {
            Self::InvalidInput => RuntimeInteractionErrorClassV1::InvalidInput,
            Self::InvalidAuthority => RuntimeInteractionErrorClassV1::InvalidAuthority,
            Self::Conflict => RuntimeInteractionErrorClassV1::Conflict,
            Self::PersistenceCorrupt => RuntimeInteractionErrorClassV1::PersistenceCorrupt,
            Self::Timeout => RuntimeInteractionErrorClassV1::Timeout,
            Self::Unavailable => RuntimeInteractionErrorClassV1::Unavailable,
            Self::Indeterminate => RuntimeInteractionErrorClassV1::Indeterminate,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidInput => "runtime_interaction_invalid_input",
            Self::InvalidAuthority => "runtime_interaction_invalid_authority",
            Self::Conflict => "runtime_interaction_conflict",
            Self::PersistenceCorrupt => "runtime_interaction_persistence_corrupt",
            Self::Timeout => "runtime_interaction_timeout",
            Self::Unavailable => "runtime_interaction_unavailable",
            Self::Indeterminate => "runtime_interaction_indeterminate",
        }
    }
}

pub(crate) fn validate_millisecond_duration(
    duration: Duration,
    maximum: Duration,
) -> Result<i64, RuntimeInteractionPersistenceErrorV1> {
    if duration.is_zero()
        || duration > maximum
        || !duration.subsec_nanos().is_multiple_of(1_000_000)
        || duration.as_millis() > i64::MAX as u128
    {
        return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
    }
    Ok(duration.as_millis() as i64)
}

pub(crate) fn map_query_error(error: &sqlx::Error) -> RuntimeInteractionPersistenceErrorV1 {
    let code = error
        .as_database_error()
        .and_then(|database| database.code());
    match code.as_deref() {
        Some("RI001") => RuntimeInteractionPersistenceErrorV1::Conflict,
        Some("RI002") => RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt,
        Some("RI003") => RuntimeInteractionPersistenceErrorV1::InvalidInput,
        Some("RI004") => RuntimeInteractionPersistenceErrorV1::InvalidAuthority,
        Some("57014") | Some("55P03") => RuntimeInteractionPersistenceErrorV1::Timeout,
        Some(code) if code.starts_with("08") => RuntimeInteractionPersistenceErrorV1::Unavailable,
        _ => match error {
            sqlx::Error::PoolTimedOut => RuntimeInteractionPersistenceErrorV1::Timeout,
            sqlx::Error::PoolClosed
            | sqlx::Error::WorkerCrashed
            | sqlx::Error::Io(_)
            | sqlx::Error::Tls(_)
            | sqlx::Error::Protocol(_) => RuntimeInteractionPersistenceErrorV1::Unavailable,
            sqlx::Error::RowNotFound
            | sqlx::Error::TypeNotFound { .. }
            | sqlx::Error::ColumnIndexOutOfBounds { .. }
            | sqlx::Error::ColumnNotFound(_)
            | sqlx::Error::ColumnDecode { .. }
            | sqlx::Error::Decode(_) => RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt,
            _ => RuntimeInteractionPersistenceErrorV1::Unavailable,
        },
    }
}

pub(crate) fn map_mutation_error(error: &sqlx::Error) -> RuntimeInteractionPersistenceErrorV1 {
    if mutation_outcome_is_indeterminate(error) {
        RuntimeInteractionPersistenceErrorV1::Indeterminate
    } else {
        map_query_error(error)
    }
}

pub(crate) fn map_mutation_commit_error(
    _error: &sqlx::Error,
) -> RuntimeInteractionPersistenceErrorV1 {
    RuntimeInteractionPersistenceErrorV1::Indeterminate
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
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed => true,
        _ => false,
    }
}
