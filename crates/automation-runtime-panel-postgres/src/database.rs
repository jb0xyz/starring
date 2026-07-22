use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

use crate::contract::DATABASE_READINESS_QUERY;
use crate::error::{map_query_error, validate_millisecond_duration};
use crate::RuntimePanelPersistenceErrorV1;

pub const DEFAULT_RUNTIME_PANEL_STATEMENT_TIMEOUT: Duration = Duration::from_secs(2);
pub const DEFAULT_RUNTIME_PANEL_LOCK_TIMEOUT: Duration = Duration::from_secs(1);
pub const MAX_RUNTIME_PANEL_DATABASE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimePanelDatabaseTimeoutsV1 {
    statement_timeout: Duration,
    lock_timeout: Duration,
}

impl RuntimePanelDatabaseTimeoutsV1 {
    pub fn new(
        statement_timeout: Duration,
        lock_timeout: Duration,
    ) -> Result<Self, RuntimePanelPersistenceErrorV1> {
        validate_millisecond_duration(statement_timeout, MAX_RUNTIME_PANEL_DATABASE_TIMEOUT)?;
        validate_millisecond_duration(lock_timeout, MAX_RUNTIME_PANEL_DATABASE_TIMEOUT)?;
        if lock_timeout > statement_timeout {
            return Err(RuntimePanelPersistenceErrorV1::InvalidDuration);
        }
        statement_timeout
            .checked_mul(2)
            .ok_or(RuntimePanelPersistenceErrorV1::InvalidDuration)?;
        Ok(Self {
            statement_timeout,
            lock_timeout,
        })
    }

    pub fn statement_timeout(self) -> Duration {
        self.statement_timeout
    }

    pub fn lock_timeout(self) -> Duration {
        self.lock_timeout
    }
}

impl Default for RuntimePanelDatabaseTimeoutsV1 {
    fn default() -> Self {
        Self {
            statement_timeout: DEFAULT_RUNTIME_PANEL_STATEMENT_TIMEOUT,
            lock_timeout: DEFAULT_RUNTIME_PANEL_LOCK_TIMEOUT,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePanelDatabaseExpectationV1 {
    database_identity: String,
    database_name: String,
    executor_role: String,
}

impl RuntimePanelDatabaseExpectationV1 {
    pub fn new(
        database_identity: impl Into<String>,
        database_name: impl Into<String>,
        executor_role: impl Into<String>,
    ) -> Result<Self, RuntimePanelPersistenceErrorV1> {
        let database_identity = database_identity.into();
        let database_name = database_name.into();
        let executor_role = executor_role.into();
        if !canonical_database_identity(&database_identity)
            || !valid_database_identifier(&database_name)
            || !valid_database_identifier(&executor_role)
        {
            return Err(RuntimePanelPersistenceErrorV1::InvalidAuthority);
        }
        Ok(Self {
            database_identity,
            database_name,
            executor_role,
        })
    }

    pub fn database_identity(&self) -> &str {
        &self.database_identity
    }

    pub fn database_name(&self) -> &str {
        &self.database_name
    }

    pub fn executor_role(&self) -> &str {
        &self.executor_role
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePanelDatabaseReadinessV1 {
    pub database_identity: String,
    pub database_name: String,
    pub executor_role: String,
    pub checked_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct RuntimePanelDatabaseReadinessRowV1 {
    database_identity: String,
    database_name: String,
    executor_role: String,
    checked_at: DateTime<Utc>,
}

pub async fn verify_runtime_panel_database_v1(
    pool: &PgPool,
    expectation: &RuntimePanelDatabaseExpectationV1,
) -> Result<RuntimePanelDatabaseReadinessV1, RuntimePanelPersistenceErrorV1> {
    verify_runtime_panel_database_with_timeouts_v1(
        pool,
        expectation,
        RuntimePanelDatabaseTimeoutsV1::default(),
    )
    .await
}

pub async fn verify_runtime_panel_database_with_timeouts_v1(
    pool: &PgPool,
    expectation: &RuntimePanelDatabaseExpectationV1,
    timeouts: RuntimePanelDatabaseTimeoutsV1,
) -> Result<RuntimePanelDatabaseReadinessV1, RuntimePanelPersistenceErrorV1> {
    let mut transaction = begin_panel_transaction(pool, timeouts).await?;
    let rows = sqlx::query_as::<_, RuntimePanelDatabaseReadinessRowV1>(DATABASE_READINESS_QUERY)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|error| map_query_error(&error))?;
    let [row] = rows.as_slice() else {
        return Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt);
    };
    if row.database_identity != expectation.database_identity
        || row.database_name != expectation.database_name
        || row.executor_role != expectation.executor_role
        || !canonical_database_identity(&row.database_identity)
        || !valid_database_identifier(&row.database_name)
        || !valid_database_identifier(&row.executor_role)
    {
        return Err(RuntimePanelPersistenceErrorV1::InvalidAuthority);
    }
    let readiness = RuntimePanelDatabaseReadinessV1 {
        database_identity: row.database_identity.clone(),
        database_name: row.database_name.clone(),
        executor_role: row.executor_role.clone(),
        checked_at: row.checked_at,
    };
    transaction
        .commit()
        .await
        .map_err(|error| map_query_error(&error))?;
    Ok(readiness)
}

pub(crate) async fn begin_panel_transaction(
    pool: &PgPool,
    timeouts: RuntimePanelDatabaseTimeoutsV1,
) -> Result<Transaction<'_, Postgres>, RuntimePanelPersistenceErrorV1> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| map_query_error(&error))?;
    let idle_timeout = timeouts
        .statement_timeout
        .checked_mul(2)
        .ok_or(RuntimePanelPersistenceErrorV1::InvalidDuration)?;
    sqlx::query(
        "SELECT pg_catalog.set_config('statement_timeout', $1, TRUE), \
                pg_catalog.set_config('lock_timeout', $2, TRUE), \
                pg_catalog.set_config('idle_in_transaction_session_timeout', $3, TRUE), \
                pg_catalog.set_config('search_path', 'pg_catalog', TRUE)",
    )
    .bind(format!("{}ms", timeouts.statement_timeout.as_millis()))
    .bind(format!("{}ms", timeouts.lock_timeout.as_millis()))
    .bind(format!("{}ms", idle_timeout.as_millis()))
    .execute(&mut *transaction)
    .await
    .map_err(|error| map_query_error(&error))?;
    Ok(transaction)
}

fn canonical_database_identity(value: &str) -> bool {
    if value.len() != 36 {
        return false;
    }
    value.bytes().enumerate().all(|(index, byte)| match index {
        8 | 13 | 18 | 23 => byte == b'-',
        _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
    }) && value != "00000000-0000-0000-0000-000000000000"
}

fn valid_database_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    value.len() <= 63
        && (first.is_ascii_lowercase() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_contract_is_bounded_and_ordered() {
        assert!(RuntimePanelDatabaseTimeoutsV1::new(
            Duration::from_secs(2),
            Duration::from_secs(1)
        )
        .is_ok());
        for (statement, lock) in [
            (Duration::ZERO, Duration::from_secs(1)),
            (Duration::from_secs(1), Duration::ZERO),
            (Duration::from_secs(1), Duration::from_secs(2)),
            (Duration::from_secs(31), Duration::from_secs(1)),
            (Duration::from_nanos(1), Duration::from_nanos(1)),
        ] {
            assert_eq!(
                RuntimePanelDatabaseTimeoutsV1::new(statement, lock),
                Err(RuntimePanelPersistenceErrorV1::InvalidDuration)
            );
        }
    }

    #[test]
    fn database_expectation_is_strict() {
        let valid = RuntimePanelDatabaseExpectationV1::new(
            "01234567-89ab-cdef-8123-456789abcdef",
            "starring_runtime",
            "starring_runtime_panel",
        )
        .unwrap();
        assert_eq!(valid.database_name(), "starring_runtime");
        for identity in [
            "00000000-0000-0000-0000-000000000000",
            "01234567-89AB-cdef-8123-456789abcdef",
            "not-an-identity",
        ] {
            assert_eq!(
                RuntimePanelDatabaseExpectationV1::new(identity, "database", "role"),
                Err(RuntimePanelPersistenceErrorV1::InvalidAuthority)
            );
        }
        for (database, role) in [
            ("Starring", "role"),
            ("starring-runtime", "role"),
            ("starring", "Runtime"),
            ("starring", "runtime-role"),
        ] {
            assert_eq!(
                RuntimePanelDatabaseExpectationV1::new(
                    "01234567-89ab-cdef-8123-456789abcdef",
                    database,
                    role,
                ),
                Err(RuntimePanelPersistenceErrorV1::InvalidAuthority)
            );
        }
    }
}
