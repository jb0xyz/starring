use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::{Connection, PgConnection, PgPool, Postgres, Transaction};

use crate::connection::ServingConnectionGuardV1;
use crate::contract::{
    DATABASE_BINDING_QUERY, DATABASE_READINESS_DEFINITION_QUERY, DATABASE_READINESS_QUERY,
};
use crate::error::{map_query_error, validate_millisecond_duration};
use crate::RuntimeServingPersistenceErrorV1;

pub const DEFAULT_RUNTIME_SERVING_STATEMENT_TIMEOUT: Duration = Duration::from_secs(2);
pub const DEFAULT_RUNTIME_SERVING_LOCK_TIMEOUT: Duration = Duration::from_secs(1);
pub const MAX_RUNTIME_SERVING_DATABASE_TIMEOUT: Duration = Duration::from_secs(30);
const RUNTIME_SERVING_READINESS_DEFINITION_DIGEST_V1: &str =
    "1d7bb5b18129f99ef87b5ad0dfe712b4e6beac33a0461218fedf67fa6990ac3b";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeServingDatabaseTimeoutsV1 {
    statement_timeout: Duration,
    lock_timeout: Duration,
}

impl RuntimeServingDatabaseTimeoutsV1 {
    pub fn new(
        statement_timeout: Duration,
        lock_timeout: Duration,
    ) -> Result<Self, RuntimeServingPersistenceErrorV1> {
        validate_millisecond_duration(statement_timeout, MAX_RUNTIME_SERVING_DATABASE_TIMEOUT)?;
        validate_millisecond_duration(lock_timeout, MAX_RUNTIME_SERVING_DATABASE_TIMEOUT)?;
        if lock_timeout >= statement_timeout {
            return Err(RuntimeServingPersistenceErrorV1::InvalidInput);
        }
        statement_timeout
            .checked_mul(2)
            .ok_or(RuntimeServingPersistenceErrorV1::InvalidInput)?;
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

impl Default for RuntimeServingDatabaseTimeoutsV1 {
    fn default() -> Self {
        Self {
            statement_timeout: DEFAULT_RUNTIME_SERVING_STATEMENT_TIMEOUT,
            lock_timeout: DEFAULT_RUNTIME_SERVING_LOCK_TIMEOUT,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeServingDatabaseExpectationV1 {
    database_identity: String,
    database_name: String,
    executor_role: String,
}

impl RuntimeServingDatabaseExpectationV1 {
    pub fn new(
        database_identity: impl Into<String>,
        database_name: impl Into<String>,
        executor_role: impl Into<String>,
    ) -> Result<Self, RuntimeServingPersistenceErrorV1> {
        let database_identity = database_identity.into();
        let database_name = database_name.into();
        let executor_role = executor_role.into();
        if !canonical_database_identity(&database_identity)
            || !valid_database_identifier(&database_name)
            || !valid_database_identifier(&executor_role)
        {
            return Err(RuntimeServingPersistenceErrorV1::DatabaseAuthorityMismatch);
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
pub struct RuntimeServingDatabaseReadinessV1 {
    pub database_identity: String,
    pub database_name: String,
    pub executor_role: String,
    pub checked_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct RuntimeServingDatabaseReadinessRowV1 {
    database_identity: String,
    database_name: String,
    executor_role: String,
    checked_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct RuntimeServingDatabaseBindingRowV1 {
    database_identity: String,
    database_name: String,
    executor_role: String,
}

pub async fn verify_runtime_serving_database_v1(
    pool: &PgPool,
    expectation: &RuntimeServingDatabaseExpectationV1,
) -> Result<RuntimeServingDatabaseReadinessV1, RuntimeServingPersistenceErrorV1> {
    verify_runtime_serving_database_with_timeouts_v1(
        pool,
        expectation,
        RuntimeServingDatabaseTimeoutsV1::default(),
    )
    .await
}

pub async fn verify_runtime_serving_database_with_timeouts_v1(
    pool: &PgPool,
    expectation: &RuntimeServingDatabaseExpectationV1,
    timeouts: RuntimeServingDatabaseTimeoutsV1,
) -> Result<RuntimeServingDatabaseReadinessV1, RuntimeServingPersistenceErrorV1> {
    let deadline = tokio::time::Instant::now() + timeouts.statement_timeout;
    let connection = tokio::time::timeout_at(deadline, pool.acquire())
        .await
        .map_err(|_| RuntimeServingPersistenceErrorV1::Timeout)?
        .map_err(map_query_error)?;
    let mut connection = ServingConnectionGuardV1::new(connection);
    let database_connection = connection
        .connection_mut()
        .ok_or(RuntimeServingPersistenceErrorV1::PersistenceCorrupt)?;
    let result = tokio::time::timeout_at(
        deadline,
        verify_runtime_serving_database_on_connection_v1(
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
        Err(_) => Err(RuntimeServingPersistenceErrorV1::Timeout),
    }
}

async fn verify_runtime_serving_database_on_connection_v1(
    connection: &mut PgConnection,
    expectation: &RuntimeServingDatabaseExpectationV1,
    timeouts: RuntimeServingDatabaseTimeoutsV1,
) -> Result<RuntimeServingDatabaseReadinessV1, RuntimeServingPersistenceErrorV1> {
    let mut transaction = begin_serving_read_transaction(connection, timeouts).await?;
    let definition_digests =
        sqlx::query_scalar::<_, Option<String>>(DATABASE_READINESS_DEFINITION_QUERY)
            .fetch_all(&mut *transaction)
            .await
            .map_err(map_query_error)?;
    let [Some(definition_digest)] = definition_digests.as_slice() else {
        return Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt);
    };
    if definition_digest != RUNTIME_SERVING_READINESS_DEFINITION_DIGEST_V1 {
        return Err(RuntimeServingPersistenceErrorV1::DatabaseAuthorityMismatch);
    }
    let rows = sqlx::query_as::<_, RuntimeServingDatabaseReadinessRowV1>(DATABASE_READINESS_QUERY)
        .fetch_all(&mut *transaction)
        .await
        .map_err(map_query_error)?;
    let [row] = rows.as_slice() else {
        return Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt);
    };
    verify_database_authority(
        &row.database_identity,
        &row.database_name,
        &row.executor_role,
        expectation,
    )?;
    let readiness = RuntimeServingDatabaseReadinessV1 {
        database_identity: row.database_identity.clone(),
        database_name: row.database_name.clone(),
        executor_role: row.executor_role.clone(),
        checked_at: row.checked_at,
    };
    transaction.commit().await.map_err(map_query_error)?;
    Ok(readiness)
}

pub(crate) async fn verify_runtime_serving_binding_v1(
    transaction: &mut Transaction<'_, Postgres>,
    expectation: &RuntimeServingDatabaseExpectationV1,
) -> Result<(), RuntimeServingPersistenceErrorV1> {
    let rows = sqlx::query_as::<_, RuntimeServingDatabaseBindingRowV1>(DATABASE_BINDING_QUERY)
        .fetch_all(&mut **transaction)
        .await
        .map_err(map_query_error)?;
    let [row] = rows.as_slice() else {
        return Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt);
    };
    verify_database_authority(
        &row.database_identity,
        &row.database_name,
        &row.executor_role,
        expectation,
    )
}

pub(crate) async fn begin_serving_mutation_transaction(
    connection: &mut PgConnection,
    timeouts: RuntimeServingDatabaseTimeoutsV1,
) -> Result<Transaction<'_, Postgres>, RuntimeServingPersistenceErrorV1> {
    begin_serving_transaction(connection, timeouts, false).await
}

async fn begin_serving_read_transaction(
    connection: &mut PgConnection,
    timeouts: RuntimeServingDatabaseTimeoutsV1,
) -> Result<Transaction<'_, Postgres>, RuntimeServingPersistenceErrorV1> {
    begin_serving_transaction(connection, timeouts, true).await
}

async fn begin_serving_transaction(
    connection: &mut PgConnection,
    timeouts: RuntimeServingDatabaseTimeoutsV1,
    read_only: bool,
) -> Result<Transaction<'_, Postgres>, RuntimeServingPersistenceErrorV1> {
    let mut transaction = connection.begin().await.map_err(map_query_error)?;
    let transaction_mode = if read_only {
        "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY"
    } else {
        "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ WRITE"
    };
    sqlx::query(transaction_mode)
        .execute(&mut *transaction)
        .await
        .map_err(map_query_error)?;
    let idle_timeout = timeouts
        .statement_timeout
        .checked_mul(2)
        .ok_or(RuntimeServingPersistenceErrorV1::InvalidInput)?;
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
    .map_err(map_query_error)?;
    Ok(transaction)
}

fn verify_database_authority(
    database_identity: &str,
    database_name: &str,
    executor_role: &str,
    expectation: &RuntimeServingDatabaseExpectationV1,
) -> Result<(), RuntimeServingPersistenceErrorV1> {
    if database_identity != expectation.database_identity
        || database_name != expectation.database_name
        || executor_role != expectation.executor_role
        || !canonical_database_identity(database_identity)
        || !valid_database_identifier(database_name)
        || !valid_database_identifier(executor_role)
    {
        return Err(RuntimeServingPersistenceErrorV1::DatabaseAuthorityMismatch);
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
        assert!(RuntimeServingDatabaseTimeoutsV1::new(
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
                RuntimeServingDatabaseTimeoutsV1::new(statement, lock),
                Err(RuntimeServingPersistenceErrorV1::InvalidInput)
            );
        }
    }

    #[test]
    fn database_expectation_is_strict() {
        let valid = RuntimeServingDatabaseExpectationV1::new(
            "01234567-89ab-cdef-8123-456789abcdef",
            "starring_runtime",
            "starring_runtime_serving",
        )
        .unwrap();
        assert_eq!(valid.database_name(), "starring_runtime");
        for identity in [
            "00000000-0000-0000-0000-000000000000",
            "01234567-89AB-cdef-8123-456789abcdef",
            "not-an-identity",
        ] {
            assert_eq!(
                RuntimeServingDatabaseExpectationV1::new(identity, "database", "role"),
                Err(RuntimeServingPersistenceErrorV1::DatabaseAuthorityMismatch)
            );
        }
    }
}
