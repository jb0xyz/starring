use automation_runtime_convergence::{RuntimeDeploymentPhaseV1, RuntimePendingConditionV1};

use crate::error::database;
use crate::model::{
    DeploymentAvailabilityV1, RuntimeDeploymentScopeV1, RuntimeDeploymentStatusV1, RuntimeDigestV1,
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
        let desired_target_digest = persisted.desired_target_digest.clone();
        let terminal = match &snapshot.phase {
            RuntimeDeploymentPhaseV1::Cancelled { .. } => {
                Some((DeploymentAvailabilityV1::Cancelled, "deployment_cancelled"))
            }
            RuntimeDeploymentPhaseV1::Superseded { .. } => Some((
                DeploymentAvailabilityV1::Superseded,
                "deployment_superseded",
            )),
            _ => None,
        };
        if let Some((availability, reason_code)) = terminal {
            let observed_at = Self::database_now(&mut transaction).await?;
            transaction.commit().await.map_err(database)?;
            return Ok(status_projection(
                snapshot,
                observed_at,
                availability,
                reason_code,
                None,
                &desired_target_digest,
            ));
        }
        if let Err(error) =
            Self::assert_current_deployment_authority(&mut transaction, &persisted).await
        {
            let (availability, reason_code) = authority_failure_status(error)?;
            let observed_at = Self::database_now(&mut transaction).await?;
            transaction.commit().await.map_err(database)?;
            return Ok(status_projection(
                snapshot,
                observed_at,
                availability,
                reason_code,
                None,
                &desired_target_digest,
            ));
        }
        let non_live = match &snapshot.phase {
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
            return Ok(status_projection(
                snapshot,
                observed_at,
                availability,
                reason_code,
                None,
                &desired_target_digest,
            ));
        }
        let Some(attestation_id) = persisted.live_attestation_id else {
            let observed_at = Self::database_now(&mut transaction).await?;
            transaction.commit().await.map_err(database)?;
            return Ok(status_projection(
                snapshot,
                observed_at,
                DeploymentAvailabilityV1::RuntimePending,
                "live_attestation_missing",
                None,
                &desired_target_digest,
            ));
        };
        let Some(attestation) =
            Self::load_attestation(&mut transaction, scope, &attestation_id, false).await?
        else {
            let observed_at = Self::database_now(&mut transaction).await?;
            transaction.commit().await.map_err(database)?;
            return Ok(status_projection(
                snapshot,
                observed_at,
                DeploymentAvailabilityV1::RuntimePending,
                "live_attestation_missing",
                None,
                &desired_target_digest,
            ));
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
        Ok(status_projection(
            snapshot,
            observed_at,
            availability,
            reason_code,
            live,
            &desired_target_digest,
        ))
    }
}

fn authority_failure_status(
    error: RuntimeConvergenceStoreError,
) -> Result<(DeploymentAvailabilityV1, &'static str), RuntimeConvergenceStoreError> {
    match error {
        RuntimeConvergenceStoreError::ActiveTargetMismatch => Ok((
            DeploymentAvailabilityV1::Superseded,
            "active_target_changed",
        )),
        RuntimeConvergenceStoreError::BindingAuthorityMismatch => Ok((
            DeploymentAvailabilityV1::Superseded,
            "binding_authority_changed",
        )),
        RuntimeConvergenceStoreError::ProductAuthorityInactive => Ok((
            DeploymentAvailabilityV1::Blocked,
            "product_authority_inactive",
        )),
        RuntimeConvergenceStoreError::ScopeMismatch => Ok((
            DeploymentAvailabilityV1::Blocked,
            "product_authority_not_current",
        )),
        error => Err(error),
    }
}

fn status_projection(
    snapshot: automation_runtime_convergence::RuntimeDeploymentSnapshotV1,
    observed_at: chrono::DateTime<chrono::Utc>,
    availability: DeploymentAvailabilityV1,
    reason_code: &'static str,
    live: Option<StrictLiveProjectionV1>,
    desired_target_digest: &RuntimeDigestV1,
) -> RuntimeDeploymentStatusV1 {
    RuntimeDeploymentStatusV1 {
        snapshot,
        observed_at,
        availability,
        reason_code,
        live,
        desired_target_digest: desired_target_digest.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_authority_failures_never_preserve_pending_availability() {
        let cases = [
            (
                RuntimeConvergenceStoreError::ActiveTargetMismatch,
                DeploymentAvailabilityV1::Superseded,
                "active_target_changed",
            ),
            (
                RuntimeConvergenceStoreError::BindingAuthorityMismatch,
                DeploymentAvailabilityV1::Superseded,
                "binding_authority_changed",
            ),
            (
                RuntimeConvergenceStoreError::ProductAuthorityInactive,
                DeploymentAvailabilityV1::Blocked,
                "product_authority_inactive",
            ),
            (
                RuntimeConvergenceStoreError::ScopeMismatch,
                DeploymentAvailabilityV1::Blocked,
                "product_authority_not_current",
            ),
        ];
        for (error, expected_availability, expected_reason) in cases {
            let (availability, reason) = authority_failure_status(error).unwrap();
            assert_eq!(availability, expected_availability);
            assert_ne!(availability, DeploymentAvailabilityV1::RuntimePending);
            assert_eq!(reason, expected_reason);
        }
    }

    #[test]
    fn unrelated_runtime_errors_remain_errors() {
        assert!(matches!(
            authority_failure_status(RuntimeConvergenceStoreError::DatabaseTimeout),
            Err(RuntimeConvergenceStoreError::DatabaseTimeout)
        ));
    }
}
