mod deployment;
mod serving;
mod status;

use std::time::Duration;

use automation_ruleset::{
    content_hash, RuleSetContentHash, RuleSetSchemaVersion, CURRENT_RULESET_SCHEMA_VERSION,
};
use automation_runtime_convergence::{
    ControllerId, FencingToken, RuntimeDeployment, RuntimeDeploymentError,
    RuntimeDeploymentSnapshotV1, RuntimeGeneration,
};
use automation_state::InteractionRuleSet;
use chrono::{DateTime, TimeDelta, Utc};
use serde_json::Value;
use sqlx::types::Json;
use sqlx::{PgPool, Postgres, Transaction};

use crate::error::database;
use crate::model::{PostgresRuntimeConvergenceConfigV1, RuntimeDeploymentScopeV1};
use crate::row::{
    runtime_i64, DeploymentProjection, DeploymentRow, PersistedDeployment, ServingLeaseRow,
    DEPLOYMENT_COLUMNS, SERVING_LEASE_COLUMNS,
};
use crate::RuntimeConvergenceStoreError;

#[derive(Clone)]
pub struct PostgresRuntimeConvergence {
    pool: PgPool,
    config: PostgresRuntimeConvergenceConfigV1,
}

#[derive(sqlx::FromRow)]
struct RuntimeTargetArtifactRow {
    schema_version: i64,
    definition: Option<Json<Value>>,
    content_hash: String,
    canonical_content_hash: Option<String>,
}

impl PostgresRuntimeConvergence {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            config: PostgresRuntimeConvergenceConfigV1::default(),
        }
    }

    pub fn with_config(
        pool: PgPool,
        config: PostgresRuntimeConvergenceConfigV1,
    ) -> Result<Self, RuntimeConvergenceStoreError> {
        config.validate()?;
        Ok(Self { pool, config })
    }

    async fn begin(&self) -> Result<Transaction<'_, Postgres>, RuntimeConvergenceStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database)?;
        self.configure_transaction(&mut transaction).await?;
        Ok(transaction)
    }

    async fn begin_status(
        &self,
    ) -> Result<Transaction<'_, Postgres>, RuntimeConvergenceStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *transaction)
            .await
            .map_err(database)?;
        self.configure_transaction(&mut transaction).await?;
        Ok(transaction)
    }

    async fn configure_transaction(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
    ) -> Result<(), RuntimeConvergenceStoreError> {
        let idle_timeout = self.config.statement_timeout.checked_mul(2).ok_or(
            RuntimeConvergenceStoreError::InvalidInput("idle transaction timeout"),
        )?;
        sqlx::query(
            "SELECT pg_catalog.set_config('statement_timeout', $1, TRUE), \
                    pg_catalog.set_config('lock_timeout', $2, TRUE), \
                    pg_catalog.set_config('idle_in_transaction_session_timeout', $3, TRUE)",
        )
        .bind(format!("{}ms", self.config.statement_timeout.as_millis()))
        .bind(format!("{}ms", self.config.lock_timeout.as_millis()))
        .bind(format!("{}ms", idle_timeout.as_millis()))
        .execute(&mut **transaction)
        .await
        .map_err(database)?;
        Ok(())
    }

    async fn database_now(
        transaction: &mut Transaction<'_, Postgres>,
    ) -> Result<DateTime<Utc>, RuntimeConvergenceStoreError> {
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&mut **transaction)
            .await
            .map_err(database)
    }

    async fn mutation_now(
        transaction: &mut Transaction<'_, Postgres>,
    ) -> Result<DateTime<Utc>, RuntimeConvergenceStoreError> {
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT public.starring_runtime_mutation_clock()")
            .fetch_one(&mut **transaction)
            .await
            .map_err(database)
    }

    fn bounded_duration(
        duration: Duration,
        maximum: Duration,
        field: &'static str,
    ) -> Result<TimeDelta, RuntimeConvergenceStoreError> {
        if duration.is_zero() || duration > maximum {
            return Err(RuntimeConvergenceStoreError::InvalidInput(field));
        }
        TimeDelta::from_std(duration).map_err(|_| RuntimeConvergenceStoreError::InvalidInput(field))
    }

    fn bounded_lease_duration(
        &self,
        duration: Duration,
        maximum: Duration,
        field: &'static str,
    ) -> Result<TimeDelta, RuntimeConvergenceStoreError> {
        let minimum = self
            .config
            .statement_timeout
            .checked_add(self.config.lock_timeout)
            .ok_or(RuntimeConvergenceStoreError::InvalidInput(field))?;
        if duration <= minimum {
            return Err(RuntimeConvergenceStoreError::InvalidInput(field));
        }
        Self::bounded_duration(duration, maximum, field)
    }

    fn ensure_not_future(
        &self,
        observed_at: DateTime<Utc>,
        database_now: DateTime<Utc>,
    ) -> Result<(), RuntimeConvergenceStoreError> {
        let skew = TimeDelta::from_std(self.config.maximum_future_clock_skew)
            .map_err(|_| RuntimeConvergenceStoreError::InvalidInput("maximum future clock skew"))?;
        let maximum = database_now.checked_add_signed(skew).ok_or(
            RuntimeConvergenceStoreError::InvalidInput("future clock skew overflow"),
        )?;
        if observed_at > maximum {
            Err(RuntimeConvergenceStoreError::InvalidInput(
                "attestation timestamp is too far in the future",
            ))
        } else {
            Ok(())
        }
    }

    fn ensure_gateway_ready_fresh(
        &self,
        ready_at: DateTime<Utc>,
        database_now: DateTime<Utc>,
    ) -> Result<(), RuntimeConvergenceStoreError> {
        self.ensure_not_future(ready_at, database_now)?;
        let maximum_age = TimeDelta::from_std(self.config.maximum_gateway_ready_age)
            .map_err(|_| RuntimeConvergenceStoreError::InvalidInput("gateway Ready age"))?;
        let oldest = database_now.checked_sub_signed(maximum_age).ok_or(
            RuntimeConvergenceStoreError::InvalidInput("gateway Ready age overflow"),
        )?;
        if ready_at < oldest {
            Err(RuntimeConvergenceStoreError::InvalidInput(
                "gateway Ready evidence is stale",
            ))
        } else {
            Ok(())
        }
    }

    async fn load_scoped_for_update(
        transaction: &mut Transaction<'_, Postgres>,
        scope: &RuntimeDeploymentScopeV1,
    ) -> Result<PersistedDeployment, RuntimeConvergenceStoreError> {
        let row = sqlx::query_as::<_, DeploymentRow>(&format!(
            "SELECT {DEPLOYMENT_COLUMNS} FROM public.runtime_deployments \
             WHERE tenant_id = $1 AND installation_id = $2 AND deployment_id = $3 FOR UPDATE"
        ))
        .bind(scope.tenant_id.as_str())
        .bind(scope.installation_id.as_str())
        .bind(scope.deployment_id.as_str())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(database)?
        .ok_or(RuntimeConvergenceStoreError::NotFound)?;
        row.decode()
    }

    async fn load_scoped_for_share(
        transaction: &mut Transaction<'_, Postgres>,
        scope: &RuntimeDeploymentScopeV1,
    ) -> Result<PersistedDeployment, RuntimeConvergenceStoreError> {
        let row = sqlx::query_as::<_, DeploymentRow>(&format!(
            "SELECT {DEPLOYMENT_COLUMNS} FROM public.runtime_deployments \
             WHERE tenant_id = $1 AND installation_id = $2 AND deployment_id = $3 FOR SHARE"
        ))
        .bind(scope.tenant_id.as_str())
        .bind(scope.installation_id.as_str())
        .bind(scope.deployment_id.as_str())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(database)?
        .ok_or(RuntimeConvergenceStoreError::NotFound)?;
        row.decode()
    }

    async fn persist_deployment(
        transaction: &mut Transaction<'_, Postgres>,
        scope: &RuntimeDeploymentScopeV1,
        previous_revision: u64,
        deployment: &RuntimeDeployment,
        live_attestation_id: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<(), RuntimeConvergenceStoreError> {
        let snapshot = deployment.snapshot();
        let projection = DeploymentProjection::from_snapshot(&snapshot)?;
        let updated = sqlx::query(
            "UPDATE public.runtime_deployments SET snapshot = $4, revision = $5, phase = $6, \
             controller_id = $7, controller_fencing_token = $8, controller_acquired_at = $9, \
             controller_lease_expires_at = $10, last_fencing_token = $11, next_retry_at = $12, \
             last_stable_error_code = $13, live_attestation_id = $14, live_at = $15, \
             blocked_at = $16, superseded_at = $17, cancelled_at = $18, \
             updated_at = GREATEST($20, updated_at + INTERVAL '1 microsecond') \
             WHERE tenant_id = $1 AND installation_id = $2 AND deployment_id = $3 \
             AND revision = $19",
        )
        .bind(scope.tenant_id.as_str())
        .bind(scope.installation_id.as_str())
        .bind(scope.deployment_id.as_str())
        .bind(projection.snapshot)
        .bind(runtime_i64(snapshot.revision.get())?)
        .bind(projection.phase)
        .bind(projection.controller_id)
        .bind(projection.controller_fencing_token)
        .bind(projection.controller_acquired_at)
        .bind(projection.controller_lease_expires_at)
        .bind(projection.last_fencing_token)
        .bind(projection.next_retry_at)
        .bind(projection.last_stable_error_code)
        .bind(live_attestation_id)
        .bind(projection.live_at)
        .bind(projection.blocked_at)
        .bind(projection.superseded_at)
        .bind(projection.cancelled_at)
        .bind(runtime_i64(previous_revision)?)
        .bind(now)
        .execute(&mut **transaction)
        .await
        .map_err(database)?;
        if updated.rows_affected() != 1 {
            return Err(RuntimeConvergenceStoreError::RevisionConflict);
        }
        Ok(())
    }

    fn require_controller(
        deployment: &RuntimeDeployment,
        _expected_revision: automation_runtime_convergence::DeploymentRevision,
        controller_id: &ControllerId,
        fencing_token: FencingToken,
        runtime_generation: RuntimeGeneration,
        now: DateTime<Utc>,
    ) -> Result<(), RuntimeConvergenceStoreError> {
        if deployment.runtime_generation() != runtime_generation {
            return Err(RuntimeDeploymentError::RuntimeGenerationConflict {
                expected: deployment.runtime_generation(),
                actual: runtime_generation,
            }
            .into());
        }
        let lease = deployment
            .controller_lease()
            .ok_or(RuntimeDeploymentError::LeaseRequired)?;
        if lease.expires_at <= now {
            return Err(RuntimeDeploymentError::LeaseExpired {
                expires_at: lease.expires_at,
            }
            .into());
        }
        if lease.controller_id != *controller_id {
            return Err(RuntimeDeploymentError::ControllerMismatch.into());
        }
        if lease.fencing_token != fencing_token {
            return Err(RuntimeDeploymentError::FencingTokenConflict {
                expected: lease.fencing_token,
                actual: fencing_token,
            }
            .into());
        }
        Ok(())
    }

    async fn assert_current_snapshot_authority(
        transaction: &mut Transaction<'_, Postgres>,
        snapshot: &RuntimeDeploymentSnapshotV1,
        installation_authority_revision: u64,
    ) -> Result<(), RuntimeConvergenceStoreError> {
        Self::lock_current_snapshot_authority(
            transaction,
            snapshot,
            installation_authority_revision,
        )
        .await?;
        let artifact = sqlx::query_as::<_, RuntimeTargetArtifactRow>(
            "SELECT version.schema_version, \
             CASE WHEN pg_catalog.octet_length(version.definition::TEXT) <= 524288 \
                  THEN version.definition END AS definition, \
             version.content_hash, version.canonical_content_hash \
             FROM public.automation_ruleset_versions AS version \
             WHERE version.guild_id = $1 AND version.ruleset_key = $2 \
               AND version.version = $3 AND version.content_hash = $4 FOR SHARE",
        )
        .bind(snapshot.target.guild_id.to_string())
        .bind(snapshot.target.ruleset_key.as_str())
        .bind(i64::from(snapshot.target.version.get()))
        .bind(snapshot.target.content_hash.to_hex())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(database)?;
        if artifact.as_ref().is_some_and(|artifact| {
            runtime_target_artifact_is_valid(artifact, &snapshot.target.content_hash)
        }) {
            Ok(())
        } else {
            Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "RuleSet artifact integrity",
            ))
        }
    }

    async fn lock_current_snapshot_authority(
        transaction: &mut Transaction<'_, Postgres>,
        snapshot: &RuntimeDeploymentSnapshotV1,
        installation_authority_revision: u64,
    ) -> Result<(), RuntimeConvergenceStoreError> {
        let outcome = sqlx::query_scalar::<_, String>(
            "SELECT public.starring_runtime_lock_current_authority(\
                 $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(snapshot.identity.activation_request_id.as_str())
        .bind(snapshot.identity.promotion_id.as_str())
        .bind(snapshot.identity.tenant_id.as_str())
        .bind(snapshot.identity.installation_id.as_str())
        .bind(runtime_i64(installation_authority_revision)?)
        .bind(snapshot.target.guild_id.to_string())
        .bind(snapshot.target.ruleset_key.as_str())
        .bind(i64::from(snapshot.target.version.get()))
        .bind(snapshot.target.content_hash.to_hex())
        .bind(runtime_i64(snapshot.target.binding_revision.get())?)
        .bind(snapshot.target.binding_fingerprint.as_str())
        .fetch_one(&mut **transaction)
        .await
        .map_err(database)?;
        match outcome.as_str() {
            "exact" => Ok(()),
            "scope_mismatch" => Err(RuntimeConvergenceStoreError::ScopeMismatch),
            "binding_mismatch" => Err(RuntimeConvergenceStoreError::BindingAuthorityMismatch),
            "active_mismatch" => Err(RuntimeConvergenceStoreError::ActiveTargetMismatch),
            "lifecycle_inactive" => Err(RuntimeConvergenceStoreError::ProductAuthorityInactive),
            _ => Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "authority lock result",
            )),
        }
    }

    async fn assert_current_deployment_authority(
        transaction: &mut Transaction<'_, Postgres>,
        persisted: &PersistedDeployment,
    ) -> Result<(), RuntimeConvergenceStoreError> {
        let snapshot = persisted.deployment.snapshot();
        Self::assert_current_snapshot_authority(
            transaction,
            &snapshot,
            persisted.installation_authority_revision,
        )
        .await
    }

    async fn assert_current_deployment_authority_canonical(
        transaction: &mut Transaction<'_, Postgres>,
        persisted: &PersistedDeployment,
    ) -> Result<(), RuntimeConvergenceStoreError> {
        let snapshot = persisted.deployment.snapshot();
        Self::lock_current_snapshot_authority(
            transaction,
            &snapshot,
            persisted.installation_authority_revision,
        )
        .await?;
        let exact = sqlx::query_scalar::<_, bool>(
            "SELECT COALESCE(\
              version.canonical_content_hash = $4 \
              AND version.content_hash = $4 \
              AND version.schema_version = $5, FALSE) \
             FROM public.automation_ruleset_versions AS version \
             WHERE version.guild_id = $1 AND version.ruleset_key = $2 \
               AND version.version = $3 FOR SHARE",
        )
        .bind(snapshot.target.guild_id.to_string())
        .bind(snapshot.target.ruleset_key.as_str())
        .bind(i64::from(snapshot.target.version.get()))
        .bind(snapshot.target.content_hash.to_hex())
        .bind(i64::from(CURRENT_RULESET_SCHEMA_VERSION.get()))
        .fetch_optional(&mut **transaction)
        .await
        .map_err(database)?;
        if exact == Some(true) {
            Ok(())
        } else {
            Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "RuleSet artifact integrity",
            ))
        }
    }

    async fn assert_previous_runtime_and_now(
        transaction: &mut Transaction<'_, Postgres>,
        snapshot: &RuntimeDeploymentSnapshotV1,
    ) -> Result<DateTime<Utc>, RuntimeConvergenceStoreError> {
        let serving = sqlx::query_as::<_, ServingLeaseRow>(&format!(
            "SELECT {SERVING_LEASE_COLUMNS} FROM public.runtime_serving_leases \
             WHERE guild_id = $1 AND ruleset_key = $2 FOR UPDATE"
        ))
        .bind(snapshot.target.guild_id.to_string())
        .bind(snapshot.target.ruleset_key.as_str())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(database)?;
        let now = Self::mutation_now(transaction).await?;
        let active_serving = serving
            .as_ref()
            .filter(|serving| serving.connected && serving.serving && serving.expires_at > now);
        let exact = match (snapshot.previous_runtime.as_ref(), active_serving) {
            (None, None) => true,
            (Some(previous), Some(serving)) => {
                serving.process_instance_id == previous.process_instance_id.as_str()
                    && serving.runtime_generation == runtime_i64(previous.runtime_generation.get())?
                    && serving.guild_id == previous.target.guild_id.to_string()
                    && serving.ruleset_key == previous.target.ruleset_key.as_str()
                    && serving.target_version == i64::from(previous.target.version.get())
                    && serving.target_content_hash == previous.target.content_hash.to_hex()
                    && serving.binding_revision
                        == runtime_i64(previous.target.binding_revision.get())?
                    && serving.binding_fingerprint == previous.target.binding_fingerprint.as_str()
            }
            _ => false,
        };
        if exact {
            Ok(now)
        } else {
            Err(RuntimeDeploymentError::PreviousRuntimeMismatch.into())
        }
    }
}

fn runtime_target_artifact_is_valid(
    artifact: &RuntimeTargetArtifactRow,
    expected_hash: &RuleSetContentHash,
) -> bool {
    let Some(schema_version) = u32::try_from(artifact.schema_version)
        .ok()
        .and_then(|value| RuleSetSchemaVersion::new(value).ok())
    else {
        return false;
    };
    if schema_version != CURRENT_RULESET_SCHEMA_VERSION {
        return false;
    }
    let Some(definition) = artifact.definition.as_ref().and_then(|definition| {
        serde_json::from_value::<InteractionRuleSet>(definition.0.clone()).ok()
    }) else {
        return false;
    };
    artifact.canonical_content_hash.as_deref() == Some(artifact.content_hash.as_str())
        && RuleSetContentHash::parse_hex(&artifact.content_hash).as_ref() == Some(expected_hash)
        && automation_core::validate_structural(&definition).is_ok()
        && content_hash(schema_version, &definition).ok().as_ref() == Some(expected_hash)
}

#[cfg(test)]
mod tests {
    use automation_ruleset::{content_hash, RuleSetSchemaVersion, CURRENT_RULESET_SCHEMA_VERSION};
    use automation_state::InteractionRuleSet;
    use sqlx::types::Json;

    use super::{runtime_target_artifact_is_valid, RuntimeTargetArtifactRow};

    fn artifact(
        schema_version: RuleSetSchemaVersion,
    ) -> (
        RuntimeTargetArtifactRow,
        automation_ruleset::RuleSetContentHash,
    ) {
        let definition = InteractionRuleSet {
            version: 1,
            panels: Vec::new(),
            modals: Vec::new(),
            rules: Vec::new(),
        };
        let expected_hash = content_hash(schema_version, &definition).unwrap();
        let content_hash = expected_hash.to_hex();
        (
            RuntimeTargetArtifactRow {
                schema_version: i64::from(schema_version.get()),
                definition: Some(Json(serde_json::to_value(definition).unwrap())),
                content_hash: content_hash.clone(),
                canonical_content_hash: Some(content_hash),
            },
            expected_hash,
        )
    }

    #[test]
    fn artifact_verifier_rejects_self_consistent_unsupported_schema() {
        let (current, current_hash) = artifact(CURRENT_RULESET_SCHEMA_VERSION);
        assert!(runtime_target_artifact_is_valid(&current, &current_hash));
        let (future, future_hash) =
            artifact(RuleSetSchemaVersion::new(CURRENT_RULESET_SCHEMA_VERSION.get() + 1).unwrap());
        assert!(!runtime_target_artifact_is_valid(&future, &future_hash));
    }
}
