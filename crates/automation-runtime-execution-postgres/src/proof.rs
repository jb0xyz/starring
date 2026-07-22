use std::time::Duration;

use automation_runtime_controller::{
    RuntimeClaimNextExecutionV1, RuntimeExecutionGuardV1, RuntimeExecutionReceiptV1,
};
use automation_runtime_convergence::{FencingToken, LeaseRequestV1, TransitionOutcomeV1};

use crate::row::{DecodedRuntimeExecutionRowV1, RuntimeExecutionOutcomeV1};
use crate::RuntimeExecutionPersistenceErrorV1;

pub(crate) fn prove_claim_next_v1(
    row: DecodedRuntimeExecutionRowV1,
    request: &RuntimeClaimNextExecutionV1,
) -> Result<RuntimeExecutionReceiptV1, RuntimeExecutionPersistenceErrorV1> {
    if row.controller_id != request.controller_id {
        return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
    }
    if row.outcome == RuntimeExecutionOutcomeV1::Applied {
        let expected_fence = match row.previous.snapshot().last_fencing_token {
            Some(value) => value.next().ok(),
            None => Some(FencingToken::FIRST),
        };
        if expected_fence != Some(row.fencing_token) {
            return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
        }
    }
    prove_transition_v1(row, request.lease_for)
}

pub(crate) fn prove_renew_v1(
    row: DecodedRuntimeExecutionRowV1,
    guard: &RuntimeExecutionGuardV1,
    lease_for: Duration,
) -> Result<RuntimeExecutionReceiptV1, RuntimeExecutionPersistenceErrorV1> {
    let invalid = || RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt;
    let expected_revision = guard.expected_revision.next().map_err(|_| invalid())?;
    let expected_fencing_token = guard.fencing_token.next().map_err(|_| invalid())?;
    let current_snapshot = row.current.snapshot();
    if row.controller_id != guard.controller_id
        || row.fencing_token != expected_fencing_token
        || row.convergence_attempt != guard.convergence_attempt
        || current_snapshot.identity.tenant_id != guard.scope.tenant_id
        || current_snapshot.identity.installation_id != guard.scope.installation_id
        || current_snapshot.identity.deployment_id != guard.scope.deployment_id
        || current_snapshot.runtime_generation != guard.runtime_generation
        || current_snapshot.revision != expected_revision
    {
        return Err(invalid());
    }
    if row.outcome == RuntimeExecutionOutcomeV1::Applied {
        let previous = row.previous.snapshot();
        let Some(previous_lease) = previous.controller_lease.as_ref() else {
            return Err(invalid());
        };
        if previous.revision != guard.expected_revision
            || previous.runtime_generation != guard.runtime_generation
            || previous.identity != current_snapshot.identity
            || previous.last_fencing_token != Some(guard.fencing_token)
            || previous_lease.controller_id != guard.controller_id
            || previous_lease.fencing_token != guard.fencing_token
            || previous_lease.expires_at <= row.acquired_at
        {
            return Err(invalid());
        }
    }
    prove_transition_v1(row, lease_for)
}

fn prove_transition_v1(
    row: DecodedRuntimeExecutionRowV1,
    lease_for: Duration,
) -> Result<RuntimeExecutionReceiptV1, RuntimeExecutionPersistenceErrorV1> {
    let invalid = || RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt;
    let expected_duration = chrono::TimeDelta::from_std(lease_for).map_err(|_| invalid())?;
    if row.expires_at.signed_duration_since(row.acquired_at) != expected_duration {
        return Err(invalid());
    }
    let current_snapshot = row.current.snapshot();
    let Some(current_lease) = current_snapshot.controller_lease.as_ref() else {
        return Err(invalid());
    };
    if current_snapshot.requested_at > row.acquired_at
        || current_lease.controller_id != row.controller_id
        || current_lease.fencing_token != row.fencing_token
        || current_lease.acquired_at != row.acquired_at
        || current_lease.expires_at != row.expires_at
        || current_snapshot.last_fencing_token != Some(row.fencing_token)
    {
        return Err(invalid());
    }
    let mut reconstructed = row.previous;
    let transition = reconstructed
        .acquire_lease(LeaseRequestV1 {
            expected_revision: reconstructed.revision(),
            controller_id: row.controller_id.clone(),
            fencing_token: row.fencing_token,
            now: row.acquired_at,
            expires_at: row.expires_at,
        })
        .map_err(|_| invalid())?;
    let expected_outcome = match row.outcome {
        RuntimeExecutionOutcomeV1::Applied => TransitionOutcomeV1::Applied {
            revision: current_snapshot.revision,
        },
        RuntimeExecutionOutcomeV1::Replayed => TransitionOutcomeV1::Replayed {
            revision: current_snapshot.revision,
        },
    };
    if transition != expected_outcome || reconstructed.snapshot() != current_snapshot {
        return Err(invalid());
    }
    Ok(RuntimeExecutionReceiptV1 {
        snapshot: current_snapshot,
        controller_id: row.controller_id,
        fencing_token: row.fencing_token,
        convergence_attempt: row.convergence_attempt,
        acquired_at: row.acquired_at,
        expires_at: row.expires_at,
    })
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use automation_runtime_controller::{RuntimeDeploymentScopeV1, RuntimeExecutionGuardV1};
    use automation_runtime_convergence::{
        ControllerId, DeploymentId, FencingToken, InstallationId, LeaseRequestV1,
        RuntimeDeployment, RuntimeDeploymentSnapshotV1, RuntimeGeneration, TenantId,
    };
    use chrono::{DateTime, Utc};
    use serde_json::{json, Value};

    use super::*;

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_753_142_400 + second, 0).unwrap()
    }

    fn snapshot_value() -> Value {
        json!({
            "identity": {
                "deployment_id": "deployment",
                "tenant_id": "tenant",
                "installation_id": "installation",
                "promotion_id": "1".repeat(64),
                "activation_request_id": "activation"
            },
            "target": {
                "guild_id": "42",
                "ruleset_key": "studyroom",
                "version": 1,
                "content_hash": "2".repeat(64),
                "binding_revision": 1,
                "binding_fingerprint": "3".repeat(64)
            },
            "runtime_generation": 1,
            "previous_runtime": null,
            "requested_at": at(0),
            "revision": 1,
            "phase": { "phase": "requested" },
            "controller_lease": null,
            "last_fencing_token": null,
            "preflight": null,
            "drain": null,
            "activation": null,
            "panel_certificate": null,
            "gateway_ready": null,
            "live": null,
            "last_live_recovery": null,
            "last_runtime_failure": null
        })
    }

    fn deployment(value: Value) -> RuntimeDeployment {
        let snapshot = serde_json::from_value::<RuntimeDeploymentSnapshotV1>(value).unwrap();
        RuntimeDeployment::restore(snapshot).unwrap()
    }

    fn request() -> RuntimeClaimNextExecutionV1 {
        RuntimeClaimNextExecutionV1 {
            controller_id: ControllerId::parse("controller").unwrap(),
            lease_for: Duration::from_secs(60),
        }
    }

    fn applied_claim() -> DecodedRuntimeExecutionRowV1 {
        let previous = deployment(snapshot_value());
        let mut current = previous.clone();
        current
            .acquire_lease(LeaseRequestV1 {
                expected_revision: current.revision(),
                controller_id: ControllerId::parse("controller").unwrap(),
                fencing_token: FencingToken::FIRST,
                now: at(1),
                expires_at: at(61),
            })
            .unwrap();
        DecodedRuntimeExecutionRowV1 {
            outcome: RuntimeExecutionOutcomeV1::Applied,
            previous,
            current,
            controller_id: ControllerId::parse("controller").unwrap(),
            fencing_token: FencingToken::FIRST,
            convergence_attempt: NonZeroU32::MIN,
            acquired_at: at(1),
            expires_at: at(61),
        }
    }

    fn replayed_claim() -> DecodedRuntimeExecutionRowV1 {
        let applied = applied_claim();
        DecodedRuntimeExecutionRowV1 {
            outcome: RuntimeExecutionOutcomeV1::Replayed,
            previous: applied.current.clone(),
            current: applied.current,
            controller_id: applied.controller_id,
            fencing_token: applied.fencing_token,
            convergence_attempt: applied.convergence_attempt,
            acquired_at: applied.acquired_at,
            expires_at: applied.expires_at,
        }
    }

    fn guard() -> RuntimeExecutionGuardV1 {
        let claimed = applied_claim();
        RuntimeExecutionGuardV1 {
            scope: RuntimeDeploymentScopeV1 {
                tenant_id: TenantId::parse("tenant").unwrap(),
                installation_id: InstallationId::parse("installation").unwrap(),
                deployment_id: DeploymentId::parse("deployment").unwrap(),
            },
            expected_revision: claimed.current.revision(),
            controller_id: claimed.controller_id,
            fencing_token: claimed.fencing_token,
            runtime_generation: RuntimeGeneration::FIRST,
            convergence_attempt: claimed.convergence_attempt,
        }
    }

    fn applied_renewal() -> DecodedRuntimeExecutionRowV1 {
        let claim = applied_claim();
        let previous = claim.current;
        let mut current = previous.clone();
        current
            .acquire_lease(LeaseRequestV1 {
                expected_revision: current.revision(),
                controller_id: ControllerId::parse("controller").unwrap(),
                fencing_token: FencingToken::new(2).unwrap(),
                now: at(10),
                expires_at: at(70),
            })
            .unwrap();
        DecodedRuntimeExecutionRowV1 {
            outcome: RuntimeExecutionOutcomeV1::Applied,
            previous,
            current,
            controller_id: ControllerId::parse("controller").unwrap(),
            fencing_token: FencingToken::new(2).unwrap(),
            convergence_attempt: NonZeroU32::MIN,
            acquired_at: at(10),
            expires_at: at(70),
        }
    }

    #[test]
    fn claim_proof_reconstructs_applied_and_replayed_outcomes() {
        let applied = prove_claim_next_v1(applied_claim(), &request()).unwrap();
        assert_eq!(applied.snapshot.revision.get(), 2);
        let replayed = prove_claim_next_v1(replayed_claim(), &request()).unwrap();
        assert_eq!(replayed, applied);
    }

    #[test]
    fn claim_proof_rejects_outcome_fence_duration_and_full_snapshot_forgery() {
        let mut wrong_outcome = applied_claim();
        wrong_outcome.outcome = RuntimeExecutionOutcomeV1::Replayed;
        assert_eq!(
            prove_claim_next_v1(wrong_outcome, &request()).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );

        let mut wrong_fence = applied_claim();
        wrong_fence.fencing_token = FencingToken::new(2).unwrap();
        assert_eq!(
            prove_claim_next_v1(wrong_fence, &request()).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );

        let mut wrong_duration = applied_claim();
        wrong_duration.expires_at = at(60);
        assert_eq!(
            prove_claim_next_v1(wrong_duration, &request()).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );

        let mut forged = applied_claim();
        let mut value = serde_json::to_value(forged.current.snapshot()).unwrap();
        value["target"]["ruleset_key"] = json!("other");
        forged.current = deployment(value);
        assert_eq!(
            prove_claim_next_v1(forged, &request()).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );
    }

    #[test]
    fn renewal_proof_requires_the_exact_guard_and_one_step_transition() {
        let renewal = applied_renewal();
        let receipt = prove_renew_v1(renewal, &guard(), Duration::from_secs(60)).unwrap();
        assert_eq!(receipt.snapshot.revision.get(), 3);
        assert_eq!(receipt.fencing_token.get(), 2);

        let mut wrong_attempt = guard();
        wrong_attempt.convergence_attempt = NonZeroU32::new(2).unwrap();
        assert_eq!(
            prove_renew_v1(applied_renewal(), &wrong_attempt, Duration::from_secs(60)).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );

        let mut wrong_scope = guard();
        wrong_scope.scope.deployment_id = DeploymentId::parse("other").unwrap();
        assert_eq!(
            prove_renew_v1(applied_renewal(), &wrong_scope, Duration::from_secs(60)).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );

        let mut wrong_outcome = applied_renewal();
        wrong_outcome.outcome = RuntimeExecutionOutcomeV1::Replayed;
        assert_eq!(
            prove_renew_v1(wrong_outcome, &guard(), Duration::from_secs(60)).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );
    }

    #[test]
    fn renewal_replay_proves_the_exact_post_state() {
        let applied = applied_renewal();
        let replay = DecodedRuntimeExecutionRowV1 {
            outcome: RuntimeExecutionOutcomeV1::Replayed,
            previous: applied.current.clone(),
            current: applied.current,
            controller_id: applied.controller_id,
            fencing_token: applied.fencing_token,
            convergence_attempt: applied.convergence_attempt,
            acquired_at: applied.acquired_at,
            expires_at: applied.expires_at,
        };
        let receipt = prove_renew_v1(replay, &guard(), Duration::from_secs(60)).unwrap();
        assert_eq!(receipt.snapshot.revision.get(), 3);
    }
}
