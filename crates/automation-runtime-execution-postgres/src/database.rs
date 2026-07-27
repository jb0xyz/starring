use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::{Connection, PgConnection, PgPool, Postgres, Transaction};

use crate::connection::ExecutionConnectionGuardV1;
use crate::contract::{
    capability_manifest_is_well_formed_v1, DATABASE_READINESS_DEFINITION_QUERY,
    DATABASE_READINESS_QUERY, RUNTIME_EXECUTION_READINESS_DEFINITION_DIGEST_V1,
};
use crate::error::{map_query_error, map_readiness_query_error, validate_millisecond_duration};
use crate::query::DATABASE_BINDING_QUERY;
use crate::RuntimeExecutionPersistenceErrorV1;

pub const DEFAULT_RUNTIME_EXECUTION_STATEMENT_TIMEOUT: Duration = Duration::from_secs(2);
pub const DEFAULT_RUNTIME_EXECUTION_LOCK_TIMEOUT: Duration = Duration::from_secs(1);
pub const MAX_RUNTIME_EXECUTION_DATABASE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeExecutionDatabaseTimeoutsV1 {
    statement_timeout: Duration,
    lock_timeout: Duration,
}

impl RuntimeExecutionDatabaseTimeoutsV1 {
    pub fn new(
        statement_timeout: Duration,
        lock_timeout: Duration,
    ) -> Result<Self, RuntimeExecutionPersistenceErrorV1> {
        validate_millisecond_duration(statement_timeout, MAX_RUNTIME_EXECUTION_DATABASE_TIMEOUT)?;
        validate_millisecond_duration(lock_timeout, MAX_RUNTIME_EXECUTION_DATABASE_TIMEOUT)?;
        if lock_timeout >= statement_timeout {
            return Err(RuntimeExecutionPersistenceErrorV1::InvalidInput);
        }
        statement_timeout
            .checked_mul(2)
            .ok_or(RuntimeExecutionPersistenceErrorV1::InvalidInput)?;
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

impl Default for RuntimeExecutionDatabaseTimeoutsV1 {
    fn default() -> Self {
        Self {
            statement_timeout: DEFAULT_RUNTIME_EXECUTION_STATEMENT_TIMEOUT,
            lock_timeout: DEFAULT_RUNTIME_EXECUTION_LOCK_TIMEOUT,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeExecutionDatabaseExpectationV1 {
    database_identity: String,
    database_name: String,
    executor_role: String,
}

impl RuntimeExecutionDatabaseExpectationV1 {
    pub fn new(
        database_identity: impl Into<String>,
        database_name: impl Into<String>,
        executor_role: impl Into<String>,
    ) -> Result<Self, RuntimeExecutionPersistenceErrorV1> {
        let database_identity = database_identity.into();
        let database_name = database_name.into();
        let executor_role = executor_role.into();
        if !canonical_database_identity(&database_identity)
            || !valid_database_identifier(&database_name)
            || !valid_database_identifier(&executor_role)
        {
            return Err(RuntimeExecutionPersistenceErrorV1::DatabaseAuthorityMismatch);
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
pub struct RuntimeExecutionDatabaseReadinessV1 {
    pub database_identity: String,
    pub database_name: String,
    pub executor_role: String,
    pub checked_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct RuntimeExecutionDatabaseReadinessRowV1 {
    database_identity: String,
    database_name: String,
    executor_role: String,
    checked_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct RuntimeExecutionDatabaseBindingRowV1 {
    pub(crate) database_identity: String,
    pub(crate) database_name: String,
    pub(crate) executor_role: String,
}

pub async fn verify_runtime_execution_database_v1(
    pool: &PgPool,
    expectation: &RuntimeExecutionDatabaseExpectationV1,
) -> Result<RuntimeExecutionDatabaseReadinessV1, RuntimeExecutionPersistenceErrorV1> {
    verify_runtime_execution_database_with_timeouts_v1(
        pool,
        expectation,
        RuntimeExecutionDatabaseTimeoutsV1::default(),
    )
    .await
}

pub async fn verify_runtime_execution_database_with_timeouts_v1(
    pool: &PgPool,
    expectation: &RuntimeExecutionDatabaseExpectationV1,
    timeouts: RuntimeExecutionDatabaseTimeoutsV1,
) -> Result<RuntimeExecutionDatabaseReadinessV1, RuntimeExecutionPersistenceErrorV1> {
    if !capability_manifest_is_well_formed_v1() {
        return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
    }
    let deadline = tokio::time::Instant::now() + timeouts.statement_timeout;
    let connection = tokio::time::timeout_at(deadline, pool.acquire())
        .await
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::Timeout)?
        .map_err(map_readiness_query_error)?;
    let mut connection = ExecutionConnectionGuardV1::new(connection);
    let database_connection = connection
        .connection_mut()
        .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
    let result = tokio::time::timeout_at(
        deadline,
        verify_runtime_execution_database_on_connection_v1(
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
        Err(_) => Err(RuntimeExecutionPersistenceErrorV1::Timeout),
    }
}

async fn verify_runtime_execution_database_on_connection_v1(
    connection: &mut PgConnection,
    expectation: &RuntimeExecutionDatabaseExpectationV1,
    timeouts: RuntimeExecutionDatabaseTimeoutsV1,
) -> Result<RuntimeExecutionDatabaseReadinessV1, RuntimeExecutionPersistenceErrorV1> {
    let mut transaction = connection
        .begin()
        .await
        .map_err(map_readiness_query_error)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(map_readiness_query_error)?;
    configure_read_transaction(&mut transaction, timeouts).await?;
    let definition_digests =
        sqlx::query_scalar::<_, Option<String>>(DATABASE_READINESS_DEFINITION_QUERY)
            .fetch_all(&mut *transaction)
            .await
            .map_err(map_readiness_query_error)?;
    let [Some(definition_digest)] = definition_digests.as_slice() else {
        return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
    };
    let Some(expected_definition_digest) = RUNTIME_EXECUTION_READINESS_DEFINITION_DIGEST_V1 else {
        return Err(RuntimeExecutionPersistenceErrorV1::DatabaseAuthorityMismatch);
    };
    if !canonical_sha256_digest(definition_digest)
        || !canonical_sha256_digest(expected_definition_digest)
        || definition_digest != expected_definition_digest
    {
        return Err(RuntimeExecutionPersistenceErrorV1::DatabaseAuthorityMismatch);
    }
    let rows =
        sqlx::query_as::<_, RuntimeExecutionDatabaseReadinessRowV1>(DATABASE_READINESS_QUERY)
            .fetch_all(&mut *transaction)
            .await
            .map_err(map_readiness_query_error)?;
    let [row] = rows.as_slice() else {
        return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
    };
    verify_database_authority(
        &row.database_identity,
        &row.database_name,
        &row.executor_role,
        expectation,
    )?;
    let readiness = RuntimeExecutionDatabaseReadinessV1 {
        database_identity: row.database_identity.clone(),
        database_name: row.database_name.clone(),
        executor_role: row.executor_role.clone(),
        checked_at: row.checked_at,
    };
    transaction
        .commit()
        .await
        .map_err(map_readiness_query_error)?;
    Ok(readiness)
}

pub(crate) async fn configure_read_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    timeouts: RuntimeExecutionDatabaseTimeoutsV1,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let idle_timeout = timeouts
        .statement_timeout
        .checked_mul(2)
        .ok_or(RuntimeExecutionPersistenceErrorV1::InvalidInput)?;
    sqlx::query(
        "SELECT pg_catalog.set_config('statement_timeout', $1, TRUE), \
                pg_catalog.set_config('lock_timeout', $2, TRUE), \
                pg_catalog.set_config('idle_in_transaction_session_timeout', $3, TRUE), \
                pg_catalog.set_config('search_path', 'pg_catalog', TRUE)",
    )
    .bind(format!("{}ms", timeouts.statement_timeout.as_millis()))
    .bind(format!("{}ms", timeouts.lock_timeout.as_millis()))
    .bind(format!("{}ms", idle_timeout.as_millis()))
    .execute(&mut **transaction)
    .await
    .map_err(map_readiness_query_error)?;
    Ok(())
}

pub(crate) async fn begin_execution_mutation_transaction(
    connection: &mut PgConnection,
    timeouts: RuntimeExecutionDatabaseTimeoutsV1,
) -> Result<Transaction<'_, Postgres>, RuntimeExecutionPersistenceErrorV1> {
    let mut transaction = connection.begin().await.map_err(map_query_error)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ WRITE")
        .execute(&mut *transaction)
        .await
        .map_err(map_query_error)?;
    configure_execution_transaction(&mut transaction, timeouts).await?;
    Ok(transaction)
}

pub(crate) async fn begin_execution_locked_observation_transaction(
    connection: &mut PgConnection,
    timeouts: RuntimeExecutionDatabaseTimeoutsV1,
) -> Result<Transaction<'_, Postgres>, RuntimeExecutionPersistenceErrorV1> {
    let mut transaction = connection.begin().await.map_err(map_query_error)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED READ WRITE")
        .execute(&mut *transaction)
        .await
        .map_err(map_query_error)?;
    configure_execution_transaction(&mut transaction, timeouts).await?;
    Ok(transaction)
}

async fn configure_execution_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    timeouts: RuntimeExecutionDatabaseTimeoutsV1,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let idle_timeout = timeouts
        .statement_timeout
        .checked_mul(2)
        .ok_or(RuntimeExecutionPersistenceErrorV1::InvalidInput)?;
    sqlx::query(
        "SELECT pg_catalog.set_config('statement_timeout', $1, TRUE), \
                pg_catalog.set_config('lock_timeout', $2, TRUE), \
                pg_catalog.set_config('idle_in_transaction_session_timeout', $3, TRUE), \
                pg_catalog.set_config('search_path', 'pg_catalog', TRUE)",
    )
    .bind(format!("{}ms", timeouts.statement_timeout.as_millis()))
    .bind(format!("{}ms", timeouts.lock_timeout.as_millis()))
    .bind(format!("{}ms", idle_timeout.as_millis()))
    .execute(&mut **transaction)
    .await
    .map_err(map_query_error)?;
    Ok(())
}

pub(crate) async fn verify_runtime_execution_binding_v1(
    transaction: &mut Transaction<'_, Postgres>,
    expectation: &RuntimeExecutionDatabaseExpectationV1,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let rows = sqlx::query_as::<_, RuntimeExecutionDatabaseBindingRowV1>(DATABASE_BINDING_QUERY)
        .fetch_all(&mut **transaction)
        .await
        .map_err(map_readiness_query_error)?;
    let [row] = rows.as_slice() else {
        return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
    };
    verify_database_authority(
        &row.database_identity,
        &row.database_name,
        &row.executor_role,
        expectation,
    )
}

pub(crate) fn verify_database_authority(
    database_identity: &str,
    database_name: &str,
    executor_role: &str,
    expectation: &RuntimeExecutionDatabaseExpectationV1,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    if database_identity != expectation.database_identity
        || database_name != expectation.database_name
        || executor_role != expectation.executor_role
        || !canonical_database_identity(database_identity)
        || !valid_database_identifier(database_name)
        || !valid_database_identifier(executor_role)
    {
        return Err(RuntimeExecutionPersistenceErrorV1::DatabaseAuthorityMismatch);
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

fn canonical_sha256_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_contract_is_bounded_and_ordered() {
        assert!(RuntimeExecutionDatabaseTimeoutsV1::new(
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
            assert_eq!(
                RuntimeExecutionDatabaseTimeoutsV1::new(statement, lock),
                Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
            );
        }
    }

    #[test]
    fn database_expectation_is_strict() {
        let valid = RuntimeExecutionDatabaseExpectationV1::new(
            "01234567-89ab-cdef-8123-456789abcdef",
            "starring_runtime",
            "starring_runtime_execution",
        )
        .unwrap();
        assert_eq!(valid.database_name(), "starring_runtime");
        for (identity, database, role) in [
            (
                "00000000-0000-0000-0000-000000000000",
                "starring_runtime",
                "starring_runtime_execution",
            ),
            (
                "01234567-89AB-cdef-8123-456789abcdef",
                "starring_runtime",
                "starring_runtime_execution",
            ),
            (
                "01234567-89ab-cdef-8123-456789abcdef",
                "StarringRuntime",
                "starring_runtime_execution",
            ),
            (
                "01234567-89ab-cdef-8123-456789abcdef",
                "starring_runtime",
                "starring-runtime-execution",
            ),
        ] {
            assert_eq!(
                RuntimeExecutionDatabaseExpectationV1::new(identity, database, role),
                Err(RuntimeExecutionPersistenceErrorV1::DatabaseAuthorityMismatch)
            );
        }
    }

    #[test]
    fn readiness_digest_anchor_is_canonical() {
        let digest = RUNTIME_EXECUTION_READINESS_DEFINITION_DIGEST_V1.unwrap();
        assert!(canonical_sha256_digest(digest));
        assert_eq!(
            digest,
            "7526d7365225da6514fcc589d76c316dd1363c40cad30e12e3f752b4c85e8044"
        );
    }
}
