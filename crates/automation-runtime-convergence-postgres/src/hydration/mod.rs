mod bindings;
mod connection;
mod contract;
mod database;
mod row;

use automation_ruleset::RuleSetVersion;
use automation_runtime_controller::RuntimeExecutionReceiptV1;
use automation_runtime_convergence::{ControllerId, FencingToken, RuntimeDeploymentSnapshotV1};
use resource_resolution::ResourceBindingMap;
use sqlx::{PgConnection, PgPool};

use crate::error::database;
use crate::model::ClaimExecutionReceiptV1;
use crate::row::runtime_i64;
use crate::RuntimeConvergenceStoreError;

pub use self::database::{
    verify_runtime_exact_target_database_v1, verify_runtime_exact_target_database_with_timeouts_v1,
    RuntimeExactTargetDatabaseExpectationV1, RuntimeExactTargetDatabaseReadinessV1,
    RuntimeExactTargetDatabaseTimeoutsV1, DEFAULT_RUNTIME_EXACT_TARGET_LOCK_TIMEOUT,
    DEFAULT_RUNTIME_EXACT_TARGET_STATEMENT_TIMEOUT, MAX_RUNTIME_EXACT_TARGET_DATABASE_TIMEOUT,
};

use self::connection::ExactTargetConnectionGuardV1;
use self::contract::EXACT_TARGET_QUERY;
use self::database::{
    begin_exact_target_transaction, verify_runtime_exact_target_binding_v1,
    verify_runtime_exact_target_database_with_timeouts_v1 as verify_database_with_timeouts,
};
use self::row::RuntimeExactTargetRow;

pub(super) struct RuntimeExactTargetExecutionV1<'a> {
    pub(super) snapshot: &'a RuntimeDeploymentSnapshotV1,
    pub(super) controller_id: &'a ControllerId,
    pub(super) fencing_token: FencingToken,
    pub(super) convergence_attempt: std::num::NonZeroU32,
}

impl<'a> From<&'a ClaimExecutionReceiptV1> for RuntimeExactTargetExecutionV1<'a> {
    fn from(value: &'a ClaimExecutionReceiptV1) -> Self {
        Self {
            snapshot: &value.snapshot,
            controller_id: &value.controller_id,
            fencing_token: value.fencing_token,
            convergence_attempt: value.convergence_attempt,
        }
    }
}

impl<'a> From<&'a RuntimeExecutionReceiptV1> for RuntimeExactTargetExecutionV1<'a> {
    fn from(value: &'a RuntimeExecutionReceiptV1) -> Self {
        Self {
            snapshot: &value.snapshot,
            controller_id: &value.controller_id,
            fencing_token: value.fencing_token,
            convergence_attempt: value.convergence_attempt,
        }
    }
}

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
    expectation: RuntimeExactTargetDatabaseExpectationV1,
    timeouts: RuntimeExactTargetDatabaseTimeoutsV1,
    initial_readiness: RuntimeExactTargetDatabaseReadinessV1,
}

impl PostgresRuntimeExactTargetReader {
    pub async fn connect_verified(
        pool: PgPool,
        expectation: RuntimeExactTargetDatabaseExpectationV1,
        timeouts: RuntimeExactTargetDatabaseTimeoutsV1,
    ) -> Result<Self, RuntimeConvergenceStoreError> {
        let initial_readiness =
            verify_database_with_timeouts(&pool, &expectation, timeouts).await?;
        Ok(Self {
            pool,
            expectation,
            timeouts,
            initial_readiness,
        })
    }

    pub async fn connect_verified_default(
        pool: PgPool,
        expectation: RuntimeExactTargetDatabaseExpectationV1,
    ) -> Result<Self, RuntimeConvergenceStoreError> {
        Self::connect_verified(
            pool,
            expectation,
            RuntimeExactTargetDatabaseTimeoutsV1::default(),
        )
        .await
    }

    pub fn initial_readiness(&self) -> &RuntimeExactTargetDatabaseReadinessV1 {
        &self.initial_readiness
    }

    pub async fn verify_database_v1(
        &self,
    ) -> Result<RuntimeExactTargetDatabaseReadinessV1, RuntimeConvergenceStoreError> {
        verify_database_with_timeouts(&self.pool, &self.expectation, self.timeouts).await
    }

    pub async fn load_for_claim(
        &self,
        claim: &ClaimExecutionReceiptV1,
    ) -> Result<RuntimeExactTargetV1, RuntimeConvergenceStoreError> {
        self.load(RuntimeExactTargetExecutionV1::from(claim)).await
    }

    pub async fn load_for_execution(
        &self,
        execution: &RuntimeExecutionReceiptV1,
    ) -> Result<RuntimeExactTargetV1, RuntimeConvergenceStoreError> {
        self.load(RuntimeExactTargetExecutionV1::from(execution))
            .await
    }

    async fn load(
        &self,
        execution: RuntimeExactTargetExecutionV1<'_>,
    ) -> Result<RuntimeExactTargetV1, RuntimeConvergenceStoreError> {
        let deadline = tokio::time::Instant::now() + self.timeouts.statement_timeout();
        let connection = tokio::time::timeout_at(deadline, self.pool.acquire())
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
            self.load_on_connection(database_connection, execution),
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

    async fn load_on_connection(
        &self,
        connection: &mut PgConnection,
        execution: RuntimeExactTargetExecutionV1<'_>,
    ) -> Result<RuntimeExactTargetV1, RuntimeConvergenceStoreError> {
        let snapshot = execution.snapshot;
        let identity = &snapshot.identity;
        let target = &snapshot.target;
        let mut transaction = begin_exact_target_transaction(connection, self.timeouts).await?;
        verify_runtime_exact_target_binding_v1(&mut transaction, &self.expectation).await?;
        let rows = sqlx::query_as::<_, RuntimeExactTargetRow>(EXACT_TARGET_QUERY)
            .bind(identity.tenant_id.as_str())
            .bind(identity.installation_id.as_str())
            .bind(identity.deployment_id.as_str())
            .bind(identity.promotion_id.as_str())
            .bind(identity.activation_request_id.as_str())
            .bind(runtime_i64(snapshot.revision.get())?)
            .bind(execution.controller_id.as_str())
            .bind(runtime_i64(execution.fencing_token.get())?)
            .bind(i64::from(execution.convergence_attempt.get()))
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
        let hydrated = row.decode(&execution)?;
        transaction.commit().await.map_err(database)?;
        Ok(hydrated)
    }
}
