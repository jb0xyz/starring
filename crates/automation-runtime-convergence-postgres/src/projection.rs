use automation_runtime_convergence::{RuntimeDeploymentPhaseV1, RuntimePendingConditionV1};
use chrono::{DateTime, Utc};

use crate::model::{
    DeploymentAvailabilityV1, RuntimeDeploymentScopeV1, RuntimeDeploymentStatusV1,
    StrictLiveProjectionV1,
};
use crate::row::{
    metadata, runtime_i64, PersistedAttestation, PersistedDeployment, ServingLeaseRow,
};
use crate::RuntimeConvergenceStoreError;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CurrentAuthorityOutcome {
    NotEvaluated,
    Exact,
    ScopeMismatch,
    BindingMismatch,
    ActiveMismatch,
    LifecycleInactive,
}

pub(crate) struct StatusProjectionEvidence {
    pub persisted: PersistedDeployment,
    pub authority: CurrentAuthorityOutcome,
    pub attestation: Option<PersistedAttestation>,
    pub serving: Option<ServingLeaseRow>,
}

pub(crate) fn project_status(
    scope: &RuntimeDeploymentScopeV1,
    observed_at: DateTime<Utc>,
    evidence: StatusProjectionEvidence,
) -> Result<RuntimeDeploymentStatusV1, RuntimeConvergenceStoreError> {
    let snapshot = evidence.persisted.deployment.snapshot();
    let convergence_attempt = evidence.persisted.convergence_attempt;
    if snapshot.identity.tenant_id != scope.tenant_id
        || snapshot.identity.installation_id != scope.installation_id
        || snapshot.identity.deployment_id != scope.deployment_id
    {
        return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
            "deployment scope projection",
        ));
    }
    let desired_target_digest = evidence.persisted.desired_target_digest;
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
        return Ok(status_projection(
            snapshot,
            observed_at,
            availability,
            reason_code,
            None,
            desired_target_digest,
        ));
    }
    if evidence.authority != CurrentAuthorityOutcome::Exact {
        let (availability, reason_code) = authority_status(evidence.authority)?;
        return Ok(status_projection(
            snapshot,
            observed_at,
            availability,
            reason_code,
            None,
            desired_target_digest,
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
        return Ok(status_projection(
            snapshot,
            observed_at,
            availability,
            reason_code,
            None,
            desired_target_digest,
        ));
    }
    let Some(expected_attestation_id) = evidence.persisted.live_attestation_id else {
        return Ok(status_projection(
            snapshot,
            observed_at,
            DeploymentAvailabilityV1::RuntimePending,
            "live_attestation_missing",
            None,
            desired_target_digest,
        ));
    };
    let Some(attestation) = evidence.attestation else {
        return Ok(status_projection(
            snapshot,
            observed_at,
            DeploymentAvailabilityV1::RuntimePending,
            "live_attestation_missing",
            None,
            desired_target_digest,
        ));
    };
    let identity = &snapshot.identity;
    if convergence_attempt != attestation.convergence_attempt.map(Into::into)
        || attestation.id != expected_attestation_id
        || attestation.deployment_id != identity.deployment_id.as_str()
        || attestation.tenant_id != identity.tenant_id.as_str()
        || attestation.installation_id != identity.installation_id.as_str()
        || attestation.promotion_id != identity.promotion_id.as_str()
        || attestation.activation_request_id != identity.activation_request_id.as_str()
        || snapshot.live.as_ref() != Some(&attestation.record.live)
        || attestation.record.deployment_revision != snapshot.revision
    {
        return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
            "Live deployment and attestation differ",
        ));
    }
    if attestation.record.live.certified_at > observed_at {
        return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
            "attestation observation time",
        ));
    }
    if let Some(serving) = evidence.serving.as_ref() {
        serving.validate()?;
        if serving.acquired_at > observed_at || serving.last_heartbeat_at > observed_at {
            return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "serving lease observation time",
            ));
        }
    }
    let live = evidence
        .serving
        .as_ref()
        .filter(|serving| {
            serving.guild_id == snapshot.target.guild_id.to_string()
                && serving.ruleset_key == snapshot.target.ruleset_key.as_str()
                && serving.tenant_id == scope.tenant_id.as_str()
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
    } else if evidence.serving.is_none() {
        (
            DeploymentAvailabilityV1::RuntimePending,
            "serving_lease_missing",
        )
    } else if evidence
        .serving
        .as_ref()
        .is_some_and(|lease| !lease.connected || !lease.serving)
    {
        (
            DeploymentAvailabilityV1::RuntimePending,
            "gateway_not_serving",
        )
    } else if evidence
        .serving
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
    Ok(status_projection(
        snapshot,
        observed_at,
        availability,
        reason_code,
        live,
        desired_target_digest,
    ))
}

fn authority_status(
    outcome: CurrentAuthorityOutcome,
) -> Result<(DeploymentAvailabilityV1, &'static str), RuntimeConvergenceStoreError> {
    match outcome {
        CurrentAuthorityOutcome::ActiveMismatch => Ok((
            DeploymentAvailabilityV1::Superseded,
            "active_target_changed",
        )),
        CurrentAuthorityOutcome::BindingMismatch => Ok((
            DeploymentAvailabilityV1::Superseded,
            "binding_authority_changed",
        )),
        CurrentAuthorityOutcome::LifecycleInactive => Ok((
            DeploymentAvailabilityV1::Blocked,
            "product_authority_inactive",
        )),
        CurrentAuthorityOutcome::ScopeMismatch => Ok((
            DeploymentAvailabilityV1::Blocked,
            "product_authority_not_current",
        )),
        CurrentAuthorityOutcome::NotEvaluated | CurrentAuthorityOutcome::Exact => Err(
            RuntimeConvergenceStoreError::InvalidPersistedState("authority status outcome"),
        ),
    }
}

fn status_projection(
    snapshot: automation_runtime_convergence::RuntimeDeploymentSnapshotV1,
    observed_at: DateTime<Utc>,
    availability: DeploymentAvailabilityV1,
    reason_code: &'static str,
    live: Option<StrictLiveProjectionV1>,
    desired_target_digest: crate::model::RuntimeDigestV1,
) -> RuntimeDeploymentStatusV1 {
    RuntimeDeploymentStatusV1 {
        snapshot,
        observed_at,
        availability,
        reason_code,
        live,
        desired_target_digest,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_authority_failures_never_preserve_pending_availability() {
        let cases = [
            (
                CurrentAuthorityOutcome::ActiveMismatch,
                DeploymentAvailabilityV1::Superseded,
                "active_target_changed",
            ),
            (
                CurrentAuthorityOutcome::BindingMismatch,
                DeploymentAvailabilityV1::Superseded,
                "binding_authority_changed",
            ),
            (
                CurrentAuthorityOutcome::LifecycleInactive,
                DeploymentAvailabilityV1::Blocked,
                "product_authority_inactive",
            ),
            (
                CurrentAuthorityOutcome::ScopeMismatch,
                DeploymentAvailabilityV1::Blocked,
                "product_authority_not_current",
            ),
        ];
        for (outcome, expected_availability, expected_reason) in cases {
            let (availability, reason) = authority_status(outcome).unwrap();
            assert_eq!(availability, expected_availability);
            assert_ne!(availability, DeploymentAvailabilityV1::RuntimePending);
            assert_eq!(reason, expected_reason);
        }
    }

    #[test]
    fn unevaluated_authority_is_only_valid_for_terminal_projection() {
        assert!(matches!(
            authority_status(CurrentAuthorityOutcome::NotEvaluated),
            Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "authority status outcome"
            ))
        ));
    }
}
