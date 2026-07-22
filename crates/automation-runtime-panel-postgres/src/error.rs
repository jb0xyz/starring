use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimePanelErrorClassV1 {
    InvalidInput,
    OwnershipLost,
    AuthorityChanged,
    Conflict,
    Capacity,
    PersistenceCorrupt,
    Timeout,
    Unavailable,
    Indeterminate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimePanelLatchedErrorV1 {
    OwnershipLost,
    AuthorityChanged,
    Conflict,
    Indeterminate,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimePanelPersistenceErrorV1 {
    #[error("runtime panel authority is invalid")]
    InvalidAuthority,
    #[error("runtime panel duration is invalid")]
    InvalidDuration,
    #[error("runtime panel session identity generation is unavailable")]
    RandomnessUnavailable,
    #[error("runtime panel session ownership was lost")]
    OwnershipLost,
    #[error("runtime panel product authority changed")]
    AuthorityChanged,
    #[error("runtime panel persistence compare-and-swap conflicted")]
    Conflict,
    #[error("runtime panel slot capacity was exceeded")]
    Capacity,
    #[error("runtime panel persistence state is corrupt")]
    PersistenceCorrupt,
    #[error("runtime panel persistence timed out")]
    Timeout,
    #[error("runtime panel persistence is unavailable")]
    Unavailable,
    #[error("runtime panel persistence outcome is indeterminate")]
    Indeterminate,
}

impl RuntimePanelPersistenceErrorV1 {
    pub fn class(&self) -> RuntimePanelErrorClassV1 {
        match self {
            Self::InvalidAuthority | Self::InvalidDuration | Self::RandomnessUnavailable => {
                RuntimePanelErrorClassV1::InvalidInput
            }
            Self::OwnershipLost => RuntimePanelErrorClassV1::OwnershipLost,
            Self::AuthorityChanged => RuntimePanelErrorClassV1::AuthorityChanged,
            Self::Conflict => RuntimePanelErrorClassV1::Conflict,
            Self::Capacity => RuntimePanelErrorClassV1::Capacity,
            Self::PersistenceCorrupt => RuntimePanelErrorClassV1::PersistenceCorrupt,
            Self::Timeout => RuntimePanelErrorClassV1::Timeout,
            Self::Unavailable => RuntimePanelErrorClassV1::Unavailable,
            Self::Indeterminate => RuntimePanelErrorClassV1::Indeterminate,
        }
    }

    pub(crate) fn latch(&self) -> Option<RuntimePanelLatchedErrorV1> {
        match self {
            Self::OwnershipLost => Some(RuntimePanelLatchedErrorV1::OwnershipLost),
            Self::AuthorityChanged => Some(RuntimePanelLatchedErrorV1::AuthorityChanged),
            Self::Conflict => Some(RuntimePanelLatchedErrorV1::Conflict),
            Self::Indeterminate => Some(RuntimePanelLatchedErrorV1::Indeterminate),
            _ => None,
        }
    }
}

pub(crate) fn validate_millisecond_duration(
    duration: Duration,
    maximum: Duration,
) -> Result<i64, RuntimePanelPersistenceErrorV1> {
    if duration.is_zero()
        || duration > maximum
        || !duration.subsec_nanos().is_multiple_of(1_000_000)
        || duration.as_millis() > i64::MAX as u128
    {
        return Err(RuntimePanelPersistenceErrorV1::InvalidDuration);
    }
    Ok(duration.as_millis() as i64)
}

pub(crate) fn map_query_error(error: &sqlx::Error) -> RuntimePanelPersistenceErrorV1 {
    let code = error
        .as_database_error()
        .and_then(|database| database.code());
    match code.as_deref() {
        Some("RP001") => RuntimePanelPersistenceErrorV1::OwnershipLost,
        Some("RP002") => RuntimePanelPersistenceErrorV1::Conflict,
        Some("RP003") => RuntimePanelPersistenceErrorV1::Capacity,
        Some("RP004") => RuntimePanelPersistenceErrorV1::PersistenceCorrupt,
        Some("RP005") => RuntimePanelPersistenceErrorV1::AuthorityChanged,
        Some("57014") | Some("55P03") => RuntimePanelPersistenceErrorV1::Timeout,
        Some(code) if code.starts_with("08") => RuntimePanelPersistenceErrorV1::Unavailable,
        _ => match error {
            sqlx::Error::PoolTimedOut => RuntimePanelPersistenceErrorV1::Timeout,
            sqlx::Error::PoolClosed
            | sqlx::Error::WorkerCrashed
            | sqlx::Error::Io(_)
            | sqlx::Error::Tls(_)
            | sqlx::Error::Protocol(_) => RuntimePanelPersistenceErrorV1::Unavailable,
            sqlx::Error::RowNotFound
            | sqlx::Error::TypeNotFound { .. }
            | sqlx::Error::ColumnIndexOutOfBounds { .. }
            | sqlx::Error::ColumnNotFound(_)
            | sqlx::Error::ColumnDecode { .. }
            | sqlx::Error::Decode(_) => RuntimePanelPersistenceErrorV1::PersistenceCorrupt,
            _ => RuntimePanelPersistenceErrorV1::Unavailable,
        },
    }
}

pub(crate) fn map_mutation_error(error: &sqlx::Error) -> RuntimePanelPersistenceErrorV1 {
    if mutation_outcome_is_indeterminate(error) {
        RuntimePanelPersistenceErrorV1::Indeterminate
    } else {
        map_query_error(error)
    }
}

pub(crate) fn map_mutation_commit_error(_error: &sqlx::Error) -> RuntimePanelPersistenceErrorV1 {
    RuntimePanelPersistenceErrorV1::Indeterminate
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

pub(crate) fn stable_error_code(error: &RuntimePanelPersistenceErrorV1) -> &'static str {
    match error {
        RuntimePanelPersistenceErrorV1::InvalidAuthority => "runtime_panel_invalid_authority",
        RuntimePanelPersistenceErrorV1::InvalidDuration => "runtime_panel_invalid_duration",
        RuntimePanelPersistenceErrorV1::RandomnessUnavailable => {
            "runtime_panel_randomness_unavailable"
        }
        RuntimePanelPersistenceErrorV1::OwnershipLost => "runtime_panel_ownership_lost",
        RuntimePanelPersistenceErrorV1::AuthorityChanged => "runtime_panel_authority_changed",
        RuntimePanelPersistenceErrorV1::Conflict => "runtime_panel_conflict",
        RuntimePanelPersistenceErrorV1::Capacity => "runtime_panel_capacity",
        RuntimePanelPersistenceErrorV1::PersistenceCorrupt => "runtime_panel_persistence_corrupt",
        RuntimePanelPersistenceErrorV1::Timeout => "runtime_panel_timeout",
        RuntimePanelPersistenceErrorV1::Unavailable => "runtime_panel_unavailable",
        RuntimePanelPersistenceErrorV1::Indeterminate => "runtime_panel_indeterminate",
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

    fn database(code: &'static str) -> sqlx::Error {
        sqlx::Error::Database(Box::new(TestDatabaseError(code)))
    }

    #[test]
    fn private_capability_states_map_to_distinct_stable_classes() {
        let cases = [
            ("RP001", RuntimePanelPersistenceErrorV1::OwnershipLost),
            ("RP002", RuntimePanelPersistenceErrorV1::Conflict),
            ("RP003", RuntimePanelPersistenceErrorV1::Capacity),
            ("RP004", RuntimePanelPersistenceErrorV1::PersistenceCorrupt),
            ("RP005", RuntimePanelPersistenceErrorV1::AuthorityChanged),
        ];
        for (code, expected) in cases {
            assert_eq!(map_query_error(&database(code)), expected);
        }
    }

    #[test]
    fn response_loss_is_indeterminate_only_for_mutations() {
        let query = sqlx::Error::Protocol("private detail".to_string());
        let mutation = sqlx::Error::Protocol("private detail".to_string());
        assert_eq!(
            map_query_error(&query),
            RuntimePanelPersistenceErrorV1::Unavailable
        );
        assert_eq!(
            map_mutation_error(&mutation),
            RuntimePanelPersistenceErrorV1::Indeterminate
        );
    }

    #[test]
    fn every_mutation_commit_failure_is_indeterminate() {
        for error in [database("57014"), database("55P03"), database("40001")] {
            assert_eq!(
                map_mutation_commit_error(&error),
                RuntimePanelPersistenceErrorV1::Indeterminate
            );
        }
    }
}
