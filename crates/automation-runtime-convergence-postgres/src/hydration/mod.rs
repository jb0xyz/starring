mod bindings;
mod contract;
mod row;

use std::time::Duration;

use automation_ruleset::RuleSetVersion;
use automation_runtime_convergence::RuntimeDeploymentSnapshotV1;
use resource_resolution::ResourceBindingMap;
use sqlx::{PgPool, Postgres, Transaction};

use crate::error::database;
use crate::model::ClaimExecutionReceiptV1;
use crate::row::runtime_i64;
use crate::RuntimeConvergenceStoreError;

use self::contract::{DATABASE_IDENTITY_QUERY, EXACT_TARGET_QUERY};
use self::row::RuntimeExactTargetRow;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeExactTargetV1 {
    pub snapshot: RuntimeDeploymentSnapshotV1,
    pub installation_authority_revision: u64,
    pub current_authority_revision: u64,
    pub artifact: RuleSetVersion,
    pub bindings: ResourceBindingMap,
}

#[derive(Clone)]
pub struct PostgresRuntimeExactTargetReader {
    pool: PgPool,
    statement_timeout: Duration,
    lock_timeout: Duration,
}

impl PostgresRuntimeExactTargetReader {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            statement_timeout: Duration::from_secs(2),
            lock_timeout: Duration::from_secs(1),
        }
    }

    pub fn with_timeouts(
        pool: PgPool,
        statement_timeout: Duration,
        lock_timeout: Duration,
    ) -> Result<Self, RuntimeConvergenceStoreError> {
        if statement_timeout.is_zero()
            || lock_timeout.is_zero()
            || statement_timeout.as_millis() == 0
            || lock_timeout.as_millis() == 0
            || statement_timeout > Duration::from_secs(30)
            || lock_timeout > statement_timeout
        {
            return Err(RuntimeConvergenceStoreError::InvalidInput(
                "runtime hydration timeouts",
            ));
        }
        Ok(Self {
            pool,
            statement_timeout,
            lock_timeout,
        })
    }

    pub async fn database_identity(&self) -> Result<String, RuntimeConvergenceStoreError> {
        let mut transaction = self.begin().await?;
        let identities = sqlx::query_scalar::<_, String>(DATABASE_IDENTITY_QUERY)
            .fetch_all(&mut *transaction)
            .await
            .map_err(database)?;
        let [identity] = identities.as_slice() else {
            return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "runtime hydration database identity",
            ));
        };
        transaction.commit().await.map_err(database)?;
        Ok(identity.clone())
    }

    pub async fn load_for_claim(
        &self,
        claim: &ClaimExecutionReceiptV1,
    ) -> Result<RuntimeExactTargetV1, RuntimeConvergenceStoreError> {
        let snapshot = &claim.snapshot;
        let identity = &snapshot.identity;
        let target = &snapshot.target;
        let mut transaction = self.begin().await?;
        let rows = sqlx::query_as::<_, RuntimeExactTargetRow>(EXACT_TARGET_QUERY)
            .bind(identity.tenant_id.as_str())
            .bind(identity.installation_id.as_str())
            .bind(identity.deployment_id.as_str())
            .bind(identity.promotion_id.as_str())
            .bind(identity.activation_request_id.as_str())
            .bind(runtime_i64(snapshot.revision.get())?)
            .bind(claim.controller_id.as_str())
            .bind(runtime_i64(claim.fencing_token.get())?)
            .bind(i64::from(claim.convergence_attempt.get()))
            .bind(runtime_i64(snapshot.runtime_generation.get())?)
            .bind(target.guild_id.to_string())
            .bind(target.ruleset_key.as_str())
            .bind(i64::from(target.version.get()))
            .bind(target.content_hash.to_hex())
            .bind(runtime_i64(target.binding_revision.get())?)
            .bind(target.binding_fingerprint.as_str())
            .fetch_all(&mut *transaction)
            .await
            .map_err(database)?;
        let [row] = rows.as_slice() else {
            transaction.commit().await.map_err(database)?;
            return if rows.is_empty() {
                Err(RuntimeConvergenceStoreError::ExecutionClaimStale)
            } else {
                Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                    "runtime hydration result cardinality",
                ))
            };
        };
        let hydrated = row.decode(claim)?;
        transaction.commit().await.map_err(database)?;
        Ok(hydrated)
    }

    async fn begin(&self) -> Result<Transaction<'_, Postgres>, RuntimeConvergenceStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(database)?;
        let idle_timeout = self.statement_timeout.checked_mul(2).ok_or(
            RuntimeConvergenceStoreError::InvalidInput("runtime hydration idle timeout"),
        )?;
        sqlx::query(
            "SELECT pg_catalog.set_config('statement_timeout', $1, TRUE), \
                    pg_catalog.set_config('lock_timeout', $2, TRUE), \
                    pg_catalog.set_config('idle_in_transaction_session_timeout', $3, TRUE)",
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
