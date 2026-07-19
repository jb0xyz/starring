use std::time::Duration;

use sqlx::postgres::{PgPool, Postgres};

use crate::ProductDatabaseFailureV1;

const MAX_BATCH_LIMIT: u32 = 1000;
const MAX_STATEMENT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_LOCK_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
struct RetentionTimeouts {
    statement_timeout: Duration,
    lock_timeout: Duration,
}

impl RetentionTimeouts {
    fn new(statement_timeout: Duration, lock_timeout: Duration) -> Option<Self> {
        if statement_timeout.is_zero()
            || statement_timeout > MAX_STATEMENT_TIMEOUT
            || statement_timeout.as_millis() > i64::MAX as u128
            || lock_timeout.is_zero()
            || lock_timeout > MAX_LOCK_TIMEOUT
            || lock_timeout >= statement_timeout
            || lock_timeout.as_millis() > i64::MAX as u128
        {
            return None;
        }
        Some(Self {
            statement_timeout,
            lock_timeout,
        })
    }

    fn statement_timeout(&self) -> String {
        format!("{}ms", self.statement_timeout.as_millis())
    }

    fn lock_timeout(&self) -> String {
        format!("{}ms", self.lock_timeout.as_millis())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductIdentityRetentionConfigError {
    #[error("product identity retention statement timeout is invalid")]
    InvalidStatementTimeout,
    #[error("product identity retention lock timeout is invalid")]
    InvalidLockTimeout,
}

#[derive(Clone, Debug)]
pub struct PostgresProductIdentityRetentionConfig {
    timeouts: RetentionTimeouts,
}

impl PostgresProductIdentityRetentionConfig {
    pub fn new(
        statement_timeout: Duration,
        lock_timeout: Duration,
    ) -> Result<Self, ProductIdentityRetentionConfigError> {
        let timeouts =
            RetentionTimeouts::new(statement_timeout, lock_timeout).ok_or_else(|| {
                if statement_timeout.is_zero()
                    || statement_timeout > MAX_STATEMENT_TIMEOUT
                    || statement_timeout.as_millis() > i64::MAX as u128
                {
                    ProductIdentityRetentionConfigError::InvalidStatementTimeout
                } else {
                    ProductIdentityRetentionConfigError::InvalidLockTimeout
                }
            })?;
        Ok(Self { timeouts })
    }

    pub fn production() -> Self {
        Self::new(Duration::from_secs(5), Duration::from_millis(250))
            .expect("production product identity retention timeouts are valid")
    }

    fn statement_timeout(&self) -> String {
        self.timeouts.statement_timeout()
    }

    fn lock_timeout(&self) -> String {
        self.timeouts.lock_timeout()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProductIdentityRetentionReportV1 {
    deleted_sessions: u32,
    deleted_oauth_flows: u32,
    backlog_remaining: bool,
}

impl ProductIdentityRetentionReportV1 {
    pub fn deleted_sessions(&self) -> u32 {
        self.deleted_sessions
    }

    pub fn deleted_oauth_flows(&self) -> u32 {
        self.deleted_oauth_flows
    }

    pub fn backlog_remaining(&self) -> bool {
        self.backlog_remaining
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductIdentityRetentionError {
    #[error("product identity retention batch limit is invalid")]
    InvalidBatchLimit,
    #[error(transparent)]
    Database(#[from] ProductDatabaseFailureV1),
    #[error("product identity retention returned an invalid result")]
    InvalidResult,
    #[error("product identity retention commit outcome is indeterminate")]
    Indeterminate,
}

#[derive(Clone)]
pub struct PostgresProductIdentityRetention {
    pool: PgPool,
    config: PostgresProductIdentityRetentionConfig,
}

impl PostgresProductIdentityRetention {
    pub fn new(pool: PgPool) -> Self {
        Self::with_config(pool, PostgresProductIdentityRetentionConfig::production())
    }

    pub fn with_config(pool: PgPool, config: PostgresProductIdentityRetentionConfig) -> Self {
        Self { pool, config }
    }

    pub async fn purge(
        &self,
        batch_limit: u32,
    ) -> Result<ProductIdentityRetentionReportV1, ProductIdentityRetentionError> {
        if !(1..=MAX_BATCH_LIMIT).contains(&batch_limit) {
            return Err(ProductIdentityRetentionError::InvalidBatchLimit);
        }
        let mut transaction = self.pool.begin().await.map_err(database_failure)?;
        configure_transaction(
            &mut transaction,
            &self.config.statement_timeout(),
            &self.config.lock_timeout(),
        )
        .await
        .map_err(database_failure)?;
        let row = sqlx::query_as::<_, ProductIdentityRetentionRow>(
            "SELECT deleted_sessions, deleted_oauth_flows, backlog_remaining \
             FROM public.starring_purge_product_identity_v1($1)",
        )
        .bind(
            i32::try_from(batch_limit)
                .map_err(|_| ProductIdentityRetentionError::InvalidBatchLimit)?,
        )
        .fetch_one(&mut *transaction)
        .await
        .map_err(database_failure)?;
        let report = validate_report(row, batch_limit)?;
        transaction.commit().await.map_err(database_commit)?;
        Ok(report)
    }
}

async fn configure_transaction(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
    statement_timeout: &str,
    lock_timeout: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut **transaction)
        .await?;
    sqlx::query("SELECT pg_catalog.set_config('statement_timeout', $1, TRUE)")
        .bind(statement_timeout)
        .execute(&mut **transaction)
        .await?;
    sqlx::query("SELECT pg_catalog.set_config('lock_timeout', $1, TRUE)")
        .bind(lock_timeout)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct ProductIdentityRetentionRow {
    deleted_sessions: i32,
    deleted_oauth_flows: i32,
    backlog_remaining: bool,
}

fn validate_report(
    row: ProductIdentityRetentionRow,
    batch_limit: u32,
) -> Result<ProductIdentityRetentionReportV1, ProductIdentityRetentionError> {
    let deleted_sessions = u32::try_from(row.deleted_sessions)
        .map_err(|_| ProductIdentityRetentionError::InvalidResult)?;
    let deleted_oauth_flows = u32::try_from(row.deleted_oauth_flows)
        .map_err(|_| ProductIdentityRetentionError::InvalidResult)?;
    if deleted_sessions
        .checked_add(deleted_oauth_flows)
        .filter(|total| *total <= batch_limit)
        .is_none()
    {
        return Err(ProductIdentityRetentionError::InvalidResult);
    }
    Ok(ProductIdentityRetentionReportV1 {
        deleted_sessions,
        deleted_oauth_flows,
        backlog_remaining: row.backlog_remaining,
    })
}

fn database_failure(error: sqlx::Error) -> ProductIdentityRetentionError {
    ProductDatabaseFailureV1::classify(&error).into()
}

fn database_commit(error: sqlx::Error) -> ProductIdentityRetentionError {
    if matches!(&error, sqlx::Error::Database(_)) {
        database_failure(error)
    } else {
        ProductIdentityRetentionError::Indeterminate
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductActionRetentionConfigError {
    #[error("product action retention statement timeout is invalid")]
    InvalidStatementTimeout,
    #[error("product action retention lock timeout is invalid")]
    InvalidLockTimeout,
}

#[derive(Clone, Debug)]
pub struct PostgresProductActionRetentionConfig {
    timeouts: RetentionTimeouts,
}

impl PostgresProductActionRetentionConfig {
    pub fn new(
        statement_timeout: Duration,
        lock_timeout: Duration,
    ) -> Result<Self, ProductActionRetentionConfigError> {
        let timeouts =
            RetentionTimeouts::new(statement_timeout, lock_timeout).ok_or_else(|| {
                if statement_timeout.is_zero()
                    || statement_timeout > MAX_STATEMENT_TIMEOUT
                    || statement_timeout.as_millis() > i64::MAX as u128
                {
                    ProductActionRetentionConfigError::InvalidStatementTimeout
                } else {
                    ProductActionRetentionConfigError::InvalidLockTimeout
                }
            })?;
        Ok(Self { timeouts })
    }

    pub fn production() -> Self {
        Self::new(Duration::from_secs(5), Duration::from_millis(250))
            .expect("production product action retention timeouts are valid")
    }

    fn statement_timeout(&self) -> String {
        self.timeouts.statement_timeout()
    }

    fn lock_timeout(&self) -> String {
        self.timeouts.lock_timeout()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProductActionRetentionReportV1 {
    deleted_receipts: u32,
    deleted_aliases: u32,
    backlog_remaining: bool,
}

impl ProductActionRetentionReportV1 {
    pub fn deleted_receipts(&self) -> u32 {
        self.deleted_receipts
    }

    pub fn deleted_aliases(&self) -> u32 {
        self.deleted_aliases
    }

    pub fn backlog_remaining(&self) -> bool {
        self.backlog_remaining
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductActionRetentionError {
    #[error("product action retention batch limit is invalid")]
    InvalidBatchLimit,
    #[error(transparent)]
    Database(#[from] ProductDatabaseFailureV1),
    #[error("product action retention returned an invalid result")]
    InvalidResult,
    #[error("product action retention commit outcome is indeterminate")]
    Indeterminate,
}

#[derive(Clone)]
pub struct PostgresProductActionRetention {
    pool: PgPool,
    config: PostgresProductActionRetentionConfig,
}

impl PostgresProductActionRetention {
    pub fn new(pool: PgPool) -> Self {
        Self::with_config(pool, PostgresProductActionRetentionConfig::production())
    }

    pub fn with_config(pool: PgPool, config: PostgresProductActionRetentionConfig) -> Self {
        Self { pool, config }
    }

    pub async fn purge(
        &self,
        batch_limit: u32,
    ) -> Result<ProductActionRetentionReportV1, ProductActionRetentionError> {
        if !(1..=MAX_BATCH_LIMIT).contains(&batch_limit) {
            return Err(ProductActionRetentionError::InvalidBatchLimit);
        }
        let mut transaction = self.pool.begin().await.map_err(action_database_failure)?;
        configure_transaction(
            &mut transaction,
            &self.config.statement_timeout(),
            &self.config.lock_timeout(),
        )
        .await
        .map_err(action_database_failure)?;
        let row = sqlx::query_as::<_, ProductActionRetentionRow>(
            "SELECT deleted_receipts, deleted_aliases, backlog_remaining \
             FROM public.starring_purge_product_action_receipts_v1($1)",
        )
        .bind(
            i32::try_from(batch_limit)
                .map_err(|_| ProductActionRetentionError::InvalidBatchLimit)?,
        )
        .fetch_one(&mut *transaction)
        .await
        .map_err(action_database_failure)?;
        let report = validate_action_report(row, batch_limit)?;
        transaction.commit().await.map_err(action_database_commit)?;
        Ok(report)
    }
}

#[derive(sqlx::FromRow)]
struct ProductActionRetentionRow {
    deleted_receipts: i32,
    deleted_aliases: i32,
    backlog_remaining: bool,
}

fn validate_action_report(
    row: ProductActionRetentionRow,
    batch_limit: u32,
) -> Result<ProductActionRetentionReportV1, ProductActionRetentionError> {
    let deleted_receipts = u32::try_from(row.deleted_receipts)
        .map_err(|_| ProductActionRetentionError::InvalidResult)?;
    let deleted_aliases = u32::try_from(row.deleted_aliases)
        .map_err(|_| ProductActionRetentionError::InvalidResult)?;
    if deleted_receipts > batch_limit || deleted_aliases > deleted_receipts.saturating_mul(32) {
        return Err(ProductActionRetentionError::InvalidResult);
    }
    Ok(ProductActionRetentionReportV1 {
        deleted_receipts,
        deleted_aliases,
        backlog_remaining: row.backlog_remaining,
    })
}

fn action_database_failure(error: sqlx::Error) -> ProductActionRetentionError {
    ProductDatabaseFailureV1::classify(&error).into()
}

fn action_database_commit(error: sqlx::Error) -> ProductActionRetentionError {
    if matches!(&error, sqlx::Error::Database(_)) {
        action_database_failure(error)
    } else {
        ProductActionRetentionError::Indeterminate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_and_report_bounds_are_fail_closed() {
        assert!(PostgresProductIdentityRetentionConfig::new(
            Duration::from_secs(1),
            Duration::from_secs(1)
        )
        .is_err());
        assert_eq!(
            validate_report(
                ProductIdentityRetentionRow {
                    deleted_sessions: 800,
                    deleted_oauth_flows: 201,
                    backlog_remaining: false,
                },
                1000
            )
            .unwrap_err(),
            ProductIdentityRetentionError::InvalidResult
        );
        let report = validate_report(
            ProductIdentityRetentionRow {
                deleted_sessions: 600,
                deleted_oauth_flows: 400,
                backlog_remaining: true,
            },
            1000,
        )
        .unwrap();
        assert_eq!(report.deleted_sessions(), 600);
        assert_eq!(report.deleted_oauth_flows(), 400);
        assert!(report.backlog_remaining());
        assert!(PostgresProductActionRetentionConfig::new(
            Duration::from_secs(1),
            Duration::from_secs(1)
        )
        .is_err());
        assert_eq!(
            validate_action_report(
                ProductActionRetentionRow {
                    deleted_receipts: 1,
                    deleted_aliases: 33,
                    backlog_remaining: false,
                },
                1,
            )
            .unwrap_err(),
            ProductActionRetentionError::InvalidResult
        );
        let action_report = validate_action_report(
            ProductActionRetentionRow {
                deleted_receipts: 2,
                deleted_aliases: 64,
                backlog_remaining: true,
            },
            2,
        )
        .unwrap();
        assert_eq!(action_report.deleted_receipts(), 2);
        assert_eq!(action_report.deleted_aliases(), 64);
        assert!(action_report.backlog_remaining());
    }
}
