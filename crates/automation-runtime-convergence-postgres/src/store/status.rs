use automation_runtime_convergence::{RuntimeDeploymentPhaseV1, RuntimePendingConditionV1};

use crate::error::database;
use crate::model::{
    DeploymentAvailabilityV1, RuntimeDeploymentScopeV1, RuntimeDeploymentStatusV1,
    StrictLiveProjectionV1,
};
use crate::row::{metadata, runtime_i64, ServingLeaseRow, SERVING_LEASE_COLUMNS};
use crate::{PostgresRuntimeConvergence, RuntimeConvergenceStoreError};

impl PostgresRuntimeConvergence {
    pub async fn status(
        &self,
        scope: &RuntimeDeploymentScopeV1,
    ) -> Result<RuntimeDeploymentStatusV1, RuntimeConvergenceStoreError> {
        let mut transaction = self.begin_status().await?;
        let persisted = Self::load_scoped_for_share(&mut transaction, scope).await?;
        let snapshot = persisted.deployment.snapshot();
        let non_live = match &snapshot.phase {
            RuntimeDeploymentPhaseV1::Cancelled { .. } => {
                Some((DeploymentAvailabilityV1::Cancelled, "deployment_cancelled"))
            }
            RuntimeDeploymentPhaseV1::Superseded { .. } => Some((
                DeploymentAvailabilityV1::Superseded,
                "deployment_superseded",
            )),
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Blocked { .. },
            } => Some((DeploymentAvailabilityV1::Blocked, "deployment_blocked")),
            RuntimeDeploymentPhaseV1::Live => None,
            _ => Some((
                DeploymentAvailabilityV1::RuntimePending,
                "convergence_in_progress",
            )),
        };
        if let Some((availability, reason_code)) = non_live {
            let observed_at = Self::database_now(&mut transaction).await?;
            transaction.commit().await.map_err(database)?;
            return Ok(RuntimeDeploymentStatusV1 {
                snapshot,
                observed_at,
                availability,
                reason_code,
                live: None,
            });
        }
        match Self::assert_current_deployment_authority(&mut transaction, &persisted).await {
            Ok(()) => {}
            Err(RuntimeConvergenceStoreError::ActiveTargetMismatch) => {
                let observed_at = Self::database_now(&mut transaction).await?;
                transaction.commit().await.map_err(database)?;
                return Ok(RuntimeDeploymentStatusV1 {
                    snapshot,
                    observed_at,
                    availability: DeploymentAvailabilityV1::Superseded,
                    reason_code: "active_target_changed",
                    live: None,
                });
            }
            Err(RuntimeConvergenceStoreError::BindingAuthorityMismatch) => {
                let observed_at = Self::database_now(&mut transaction).await?;
                transaction.commit().await.map_err(database)?;
                return Ok(RuntimeDeploymentStatusV1 {
                    snapshot,
                    observed_at,
                    availability: DeploymentAvailabilityV1::Superseded,
                    reason_code: "binding_authority_changed",
                    live: None,
                });
            }
            Err(RuntimeConvergenceStoreError::ProductAuthorityInactive) => {
                let observed_at = Self::database_now(&mut transaction).await?;
                transaction.commit().await.map_err(database)?;
                return Ok(RuntimeDeploymentStatusV1 {
                    snapshot,
                    observed_at,
                    availability: DeploymentAvailabilityV1::Blocked,
                    reason_code: "product_authority_inactive",
                    live: None,
                });
            }
            Err(RuntimeConvergenceStoreError::ScopeMismatch) => {
                let observed_at = Self::database_now(&mut transaction).await?;
                transaction.commit().await.map_err(database)?;
                return Ok(RuntimeDeploymentStatusV1 {
                    snapshot,
                    observed_at,
                    availability: DeploymentAvailabilityV1::RuntimePending,
                    reason_code: "product_authority_not_current",
                    live: None,
                });
            }
            Err(error) => return Err(error),
        }
        let Some(attestation_id) = persisted.live_attestation_id else {
            let observed_at = Self::database_now(&mut transaction).await?;
            transaction.commit().await.map_err(database)?;
            return Ok(RuntimeDeploymentStatusV1 {
                snapshot,
                observed_at,
                availability: DeploymentAvailabilityV1::RuntimePending,
                reason_code: "live_attestation_missing",
                live: None,
            });
        };
        let Some(attestation) =
            Self::load_attestation(&mut transaction, scope, &attestation_id, false).await?
        else {
            let observed_at = Self::database_now(&mut transaction).await?;
            transaction.commit().await.map_err(database)?;
            return Ok(RuntimeDeploymentStatusV1 {
                snapshot,
                observed_at,
                availability: DeploymentAvailabilityV1::RuntimePending,
                reason_code: "live_attestation_missing",
                live: None,
            });
        };
        if snapshot.live.as_ref() != Some(&attestation.record.live)
            || attestation.record.deployment_revision != snapshot.revision
        {
            return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "Live deployment and attestation differ",
            ));
        }
        let serving = sqlx::query_as::<_, ServingLeaseRow>(&format!(
            "SELECT {SERVING_LEASE_COLUMNS} FROM public.runtime_serving_leases \
             WHERE guild_id = $1 AND ruleset_key = $2 FOR SHARE"
        ))
        .bind(snapshot.target.guild_id.to_string())
        .bind(snapshot.target.ruleset_key.as_str())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database)?;
        let observed_at = Self::database_now(&mut transaction).await?;
        let live = serving
            .as_ref()
            .filter(|serving| {
                serving.tenant_id == scope.tenant_id.as_str()
                    && serving.installation_id == scope.installation_id.as_str()
                    && serving.deployment_id == scope.deployment_id.as_str()
                    && serving.attestation_id == attestation.id.as_str()
                    && serving.process_instance_id
                        == attestation.record.live.process_instance_id.as_str()
                    && serving.runtime_generation
                        == runtime_i64(snapshot.runtime_generation.get()).unwrap_or(-1)
                    && serving.target_version == i64::from(snapshot.target.version.get())
                    && serving.target_content_hash == snapshot.target.content_hash.to_hex()
                    && serving.binding_revision
                        == runtime_i64(snapshot.target.binding_revision.get()).unwrap_or(-1)
                    && serving.binding_fingerprint == snapshot.target.binding_fingerprint.as_str()
                    && serving.connected
                    && serving.serving
                    && serving.expires_at > observed_at
            })
            .map(|serving| {
                Ok::<StrictLiveProjectionV1, RuntimeConvergenceStoreError>(StrictLiveProjectionV1 {
                    attestation_id: attestation.id.clone(),
                    process_instance_id: attestation.record.live.process_instance_id.clone(),
                    runtime_generation: attestation.record.live.runtime_generation,
                    lease_epoch: serving.checked_epoch()?,
                    serving_revision: serving.checked_revision()?,
                    last_heartbeat_at: serving.last_heartbeat_at,
                    expires_at: serving.expires_at,
                    metadata: metadata(&attestation.record),
                })
            })
            .transpose()?;
        let (availability, reason_code) = if live.is_some() {
            (DeploymentAvailabilityV1::Live, "live")
        } else if serving.is_none() {
            (
                DeploymentAvailabilityV1::RuntimePending,
                "serving_lease_missing",
            )
        } else if serving
            .as_ref()
            .is_some_and(|lease| !lease.connected || !lease.serving)
        {
            (
                DeploymentAvailabilityV1::RuntimePending,
                "gateway_not_serving",
            )
        } else if serving
            .as_ref()
            .is_some_and(|lease| lease.expires_at <= observed_at)
        {
            (
                DeploymentAvailabilityV1::RuntimePending,
                "serving_lease_expired",
            )
        } else {
            (
                DeploymentAvailabilityV1::RuntimePending,
                "serving_identity_mismatch",
            )
        };
        transaction.commit().await.map_err(database)?;
        Ok(RuntimeDeploymentStatusV1 {
            snapshot,
            observed_at,
            availability,
            reason_code,
            live,
        })
    }
}
