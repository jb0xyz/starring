use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::{Connection, PgConnection, PgPool, Postgres, Transaction};

use crate::error::{database, database_readiness};
use crate::RuntimeConvergenceStoreError;

use super::connection::ExactTargetConnectionGuardV1;
use super::contract::{
    DATABASE_BINDING_QUERY, DATABASE_READINESS_DEFINITION_QUERY, DATABASE_READINESS_QUERY,
};

pub const DEFAULT_RUNTIME_EXACT_TARGET_STATEMENT_TIMEOUT: Duration = Duration::from_secs(2);
pub const DEFAULT_RUNTIME_EXACT_TARGET_LOCK_TIMEOUT: Duration = Duration::from_secs(1);
pub const MAX_RUNTIME_EXACT_TARGET_DATABASE_TIMEOUT: Duration = Duration::from_secs(30);
const RUNTIME_EXACT_TARGET_READINESS_DEFINITION_DIGEST_V1: &str =
    "5eba72a786aebaa8afdc226d661b45132afc5aa053fab7be6a3b9737fdab0e8c";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeExactTargetDatabaseTimeoutsV1 {
    statement_timeout: Duration,
    lock_timeout: Duration,
}

impl RuntimeExactTargetDatabaseTimeoutsV1 {
    pub fn new(
        statement_timeout: Duration,
        lock_timeout: Duration,
    ) -> Result<Self, RuntimeConvergenceStoreError> {
        validate_millisecond_duration(statement_timeout)?;
        validate_millisecond_duration(lock_timeout)?;
        if lock_timeout >= statement_timeout {
            return Err(RuntimeConvergenceStoreError::InvalidInput(
                "runtime exact target database timeouts",
            ));
        }
        statement_timeout
            .checked_mul(2)
            .ok_or(RuntimeConvergenceStoreError::InvalidInput(
                "runtime exact target database timeouts",
            ))?;
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

impl Default for RuntimeExactTargetDatabaseTimeoutsV1 {
    fn default() -> Self {
        Self {
            statement_timeout: DEFAULT_RUNTIME_EXACT_TARGET_STATEMENT_TIMEOUT,
            lock_timeout: DEFAULT_RUNTIME_EXACT_TARGET_LOCK_TIMEOUT,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeExactTargetDatabaseExpectationV1 {
    database_identity: String,
    database_name: String,
    executor_role: String,
}

impl RuntimeExactTargetDatabaseExpectationV1 {
    pub fn new(
        database_identity: impl Into<String>,
        database_name: impl Into<String>,
        executor_role: impl Into<String>,
    ) -> Result<Self, RuntimeConvergenceStoreError> {
        let database_identity = database_identity.into();
        let database_name = database_name.into();
        let executor_role = executor_role.into();
        if !canonical_database_identity(&database_identity)
            || !valid_database_identifier(&database_name)
            || !valid_database_identifier(&executor_role)
        {
            return Err(RuntimeConvergenceStoreError::DatabaseAuthorityMismatch);
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
pub struct RuntimeExactTargetDatabaseReadinessV1 {
    pub database_identity: String,
    pub database_name: String,
    pub executor_role: String,
    pub checked_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct RuntimeExactTargetDatabaseReadinessRowV1 {
    database_identity: String,
    database_name: String,
    executor_role: String,
    checked_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct RuntimeExactTargetDatabaseBindingRowV1 {
    database_identity: String,
    database_name: String,
    executor_role: String,
}

pub async fn verify_runtime_exact_target_database_v1(
    pool: &PgPool,
    expectation: &RuntimeExactTargetDatabaseExpectationV1,
) -> Result<RuntimeExactTargetDatabaseReadinessV1, RuntimeConvergenceStoreError> {
    verify_runtime_exact_target_database_with_timeouts_v1(
        pool,
        expectation,
        RuntimeExactTargetDatabaseTimeoutsV1::default(),
    )
    .await
}

pub async fn verify_runtime_exact_target_database_with_timeouts_v1(
    pool: &PgPool,
    expectation: &RuntimeExactTargetDatabaseExpectationV1,
    timeouts: RuntimeExactTargetDatabaseTimeoutsV1,
) -> Result<RuntimeExactTargetDatabaseReadinessV1, RuntimeConvergenceStoreError> {
    let deadline = tokio::time::Instant::now() + timeouts.statement_timeout;
    let connection = tokio::time::timeout_at(deadline, pool.acquire())
        .await
        .map_err(|_| RuntimeConvergenceStoreError::DatabaseTimeout)?
        .map_err(database)?;
    let mut connection = ExactTargetConnectionGuardV1::new(connection);
    let Some(database_connection) = connection.connection_mut() else {
        return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
            "runtime exact target database connection",
        ));
    };
    let result = tokio::time::timeout_at(
        deadline,
        verify_runtime_exact_target_database_on_connection_v1(
            database_connection,
            expectation,
            timeouts,
        ),
    )
    .await;
    match result {
        Ok(result) => {
            connection.release_to_pool();
            result
        }
        Err(_) => Err(RuntimeConvergenceStoreError::DatabaseTimeout),
    }
}

async fn verify_runtime_exact_target_database_on_connection_v1(
    connection: &mut PgConnection,
    expectation: &RuntimeExactTargetDatabaseExpectationV1,
    timeouts: RuntimeExactTargetDatabaseTimeoutsV1,
) -> Result<RuntimeExactTargetDatabaseReadinessV1, RuntimeConvergenceStoreError> {
    let mut transaction = begin_exact_target_transaction(connection, timeouts).await?;
    let definition_digests =
        sqlx::query_scalar::<_, Option<String>>(DATABASE_READINESS_DEFINITION_QUERY)
            .fetch_all(&mut *transaction)
            .await
            .map_err(database_readiness)?;
    let [Some(definition_digest)] = definition_digests.as_slice() else {
        return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
            "runtime exact target database readiness definition",
        ));
    };
    if definition_digest != RUNTIME_EXACT_TARGET_READINESS_DEFINITION_DIGEST_V1 {
        return Err(RuntimeConvergenceStoreError::DatabaseAuthorityMismatch);
    }
    let rows =
        sqlx::query_as::<_, RuntimeExactTargetDatabaseReadinessRowV1>(DATABASE_READINESS_QUERY)
            .fetch_all(&mut *transaction)
            .await
            .map_err(database_readiness)?;
    let [row] = rows.as_slice() else {
        return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
            "runtime exact target database readiness cardinality",
        ));
    };
    if row.database_identity != expectation.database_identity
        || row.database_name != expectation.database_name
        || row.executor_role != expectation.executor_role
        || !canonical_database_identity(&row.database_identity)
        || !valid_database_identifier(&row.database_name)
        || !valid_database_identifier(&row.executor_role)
    {
        return Err(RuntimeConvergenceStoreError::DatabaseAuthorityMismatch);
    }
    let readiness = RuntimeExactTargetDatabaseReadinessV1 {
        database_identity: row.database_identity.clone(),
        database_name: row.database_name.clone(),
        executor_role: row.executor_role.clone(),
        checked_at: row.checked_at,
    };
    transaction.commit().await.map_err(database)?;
    Ok(readiness)
}

pub(super) async fn verify_runtime_exact_target_binding_v1(
    transaction: &mut Transaction<'_, Postgres>,
    expectation: &RuntimeExactTargetDatabaseExpectationV1,
) -> Result<(), RuntimeConvergenceStoreError> {
    let rows = sqlx::query_as::<_, RuntimeExactTargetDatabaseBindingRowV1>(DATABASE_BINDING_QUERY)
        .fetch_all(&mut **transaction)
        .await
        .map_err(database_readiness)?;
    let [row] = rows.as_slice() else {
        return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
            "runtime exact target database binding cardinality",
        ));
    };
    if row.database_identity != expectation.database_identity
        || row.database_name != expectation.database_name
        || row.executor_role != expectation.executor_role
    {
        return Err(RuntimeConvergenceStoreError::DatabaseAuthorityMismatch);
    }
    Ok(())
}

pub(super) async fn begin_exact_target_transaction(
    connection: &mut PgConnection,
    timeouts: RuntimeExactTargetDatabaseTimeoutsV1,
) -> Result<Transaction<'_, Postgres>, RuntimeConvergenceStoreError> {
    let mut transaction = connection.begin().await.map_err(database)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(database)?;
    let idle_timeout = timeouts.statement_timeout.checked_mul(2).ok_or(
        RuntimeConvergenceStoreError::InvalidInput("runtime exact target database timeouts"),
    )?;
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
    .map_err(database)?;
    Ok(transaction)
}

fn validate_millisecond_duration(duration: Duration) -> Result<(), RuntimeConvergenceStoreError> {
    if duration.is_zero()
        || duration > MAX_RUNTIME_EXACT_TARGET_DATABASE_TIMEOUT
        || !duration.subsec_nanos().is_multiple_of(1_000_000)
        || duration.as_millis() > i64::MAX as u128
    {
        return Err(RuntimeConvergenceStoreError::InvalidInput(
            "runtime exact target database timeouts",
        ));
    }
    Ok(())
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
        assert!(RuntimeExactTargetDatabaseTimeoutsV1::new(
            Duration::from_secs(2),
            Duration::from_secs(1)
        )
        .is_ok());
        for (statement, lock) in [
            (Duration::ZERO, Duration::from_secs(1)),
            (Duration::from_secs(1), Duration::ZERO),
            (Duration::from_secs(1), Duration::from_secs(1)),
            (Duration::from_secs(1), Duration::from_secs(2)),
            (Duration::from_secs(31), Duration::from_secs(1)),
            (Duration::from_nanos(1), Duration::from_nanos(1)),
        ] {
            assert!(RuntimeExactTargetDatabaseTimeoutsV1::new(statement, lock).is_err());
        }
    }

    #[test]
    fn database_expectation_is_strict() {
        let valid = RuntimeExactTargetDatabaseExpectationV1::new(
            "01234567-89ab-cdef-8123-456789abcdef",
            "starring_runtime",
            "starring_runtime_exact_target",
        )
        .unwrap();
        assert_eq!(valid.database_name(), "starring_runtime");
        for identity in [
            "00000000-0000-0000-0000-000000000000",
            "01234567-89AB-cdef-8123-456789abcdef",
            "not-an-identity",
        ] {
            assert!(
                RuntimeExactTargetDatabaseExpectationV1::new(identity, "database", "role").is_err()
            );
        }
        for (database, role) in [
            ("Starring", "role"),
            ("starring-runtime", "role"),
            ("starring", "Runtime"),
            ("starring", "runtime-role"),
        ] {
            assert!(RuntimeExactTargetDatabaseExpectationV1::new(
                "01234567-89ab-cdef-8123-456789abcdef",
                database,
                role,
            )
            .is_err());
        }
    }
}
