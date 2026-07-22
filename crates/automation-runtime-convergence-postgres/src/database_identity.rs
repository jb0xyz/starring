use std::time::Duration;

use sqlx::{PgPool, Postgres, Transaction};

use crate::error::database;
use crate::RuntimeConvergenceStoreError;

const DATABASE_IDENTITY_QUERY: &str =
    "SELECT * FROM public.starring_runtime_convergence_database_identity_v1()";
const DEFAULT_STATEMENT_TIMEOUT: Duration = Duration::from_secs(2);
const DEFAULT_LOCK_TIMEOUT: Duration = Duration::from_secs(1);
const MAXIMUM_DATABASE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct PostgresRuntimeConvergenceDatabaseIdentityReader {
    pool: PgPool,
    statement_timeout: Duration,
    lock_timeout: Duration,
}

impl PostgresRuntimeConvergenceDatabaseIdentityReader {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            statement_timeout: DEFAULT_STATEMENT_TIMEOUT,
            lock_timeout: DEFAULT_LOCK_TIMEOUT,
        }
    }

    pub fn with_timeouts(
        pool: PgPool,
        statement_timeout: Duration,
        lock_timeout: Duration,
    ) -> Result<Self, RuntimeConvergenceStoreError> {
        validate_timeouts(statement_timeout, lock_timeout)?;
        Ok(Self {
            pool,
            statement_timeout,
            lock_timeout,
        })
    }

    pub async fn database_identity(&self) -> Result<String, RuntimeConvergenceStoreError> {
        let mut transaction = self.begin().await?;
        let identities = sqlx::query_scalar::<_, Option<String>>(DATABASE_IDENTITY_QUERY)
            .fetch_all(&mut *transaction)
            .await
            .map_err(database)?;
        let [Some(identity)] = identities.as_slice() else {
            return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "runtime convergence database identity",
            ));
        };
        if !canonical_database_identity(identity) {
            return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "runtime convergence database identity",
            ));
        }
        transaction.commit().await.map_err(database)?;
        Ok(identity.clone())
    }

    async fn begin(&self) -> Result<Transaction<'_, Postgres>, RuntimeConvergenceStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(database)?;
        let idle_timeout = self.statement_timeout.checked_mul(2).ok_or(
            RuntimeConvergenceStoreError::InvalidInput(
                "runtime convergence database identity timeouts",
            ),
        )?;
        sqlx::query(
            "SELECT pg_catalog.set_config('statement_timeout', $1, TRUE), \
                    pg_catalog.set_config('lock_timeout', $2, TRUE), \
                    pg_catalog.set_config('idle_in_transaction_session_timeout', $3, TRUE), \
                    pg_catalog.set_config('search_path', 'pg_catalog', TRUE)",
        )
        .bind(format!("{}ms", self.statement_timeout.as_millis()))
        .bind(format!("{}ms", self.lock_timeout.as_millis()))
        .bind(format!("{}ms", idle_timeout.as_millis()))
        .execute(&mut *transaction)
        .await
        .map_err(database)?;
        Ok(transaction)
    }
}

fn validate_timeouts(
    statement_timeout: Duration,
    lock_timeout: Duration,
) -> Result<(), RuntimeConvergenceStoreError> {
    if statement_timeout.is_zero()
        || lock_timeout.is_zero()
        || statement_timeout.as_millis() == 0
        || lock_timeout.as_millis() == 0
        || statement_timeout > MAXIMUM_DATABASE_TIMEOUT
        || lock_timeout > statement_timeout
        || statement_timeout.checked_mul(2).is_none()
    {
        return Err(RuntimeConvergenceStoreError::InvalidInput(
            "runtime convergence database identity timeouts",
        ));
    }
    Ok(())
}

fn canonical_database_identity(value: &str) -> bool {
    if value.len() != 36 || value == "00000000-0000-0000-0000-000000000000" {
        return false;
    }
    value.bytes().enumerate().all(|(index, byte)| match index {
        8 | 13 | 18 | 23 => byte == b'-',
        _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_database_identity_is_lowercase_nonzero_uuid_text() {
        assert!(canonical_database_identity(
            "01234567-89ab-cdef-8123-456789abcdef"
        ));
        for invalid in [
            "00000000-0000-0000-0000-000000000000",
            "01234567-89AB-cdef-8123-456789abcdef",
            "0123456789ab-cdef-8123-456789abcdef",
            "01234567-89ab-cdef-8123-456789abcdeg",
            " 1234567-89ab-cdef-8123-456789abcdef",
        ] {
            assert!(!canonical_database_identity(invalid));
        }
    }

    #[test]
    fn database_identity_timeouts_are_bounded_and_ordered() {
        assert!(validate_timeouts(Duration::from_secs(2), Duration::from_secs(1)).is_ok());
        for (statement_timeout, lock_timeout) in [
            (Duration::ZERO, Duration::from_secs(1)),
            (Duration::from_secs(1), Duration::ZERO),
            (Duration::from_secs(1), Duration::from_secs(2)),
            (Duration::from_secs(31), Duration::from_secs(1)),
            (Duration::from_nanos(1), Duration::from_nanos(1)),
        ] {
            assert!(validate_timeouts(statement_timeout, lock_timeout).is_err());
        }
    }
}
