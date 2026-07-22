use std::num::NonZeroU32;

use automation_runtime_controller::RuntimeStaleLiveRecoveryReceiptV1;
use automation_runtime_convergence::{
    LiveLossKindV1, RecoverLiveRequestV1, RuntimeDeployment, RuntimeDeploymentPhaseV1,
    RuntimeDeploymentSnapshotV1, RuntimeFailureDispositionV1, TransitionOutcomeV1,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::types::Json;
use sqlx::{Postgres, Transaction};

use crate::error::map_query_error;
use crate::RuntimeExecutionPersistenceErrorV1;

const RECOVER_STALE_LIVE_QUERY: &str =
    "SELECT * FROM public.starring_runtime_execution_recover_stale_live_v1()";

pub(crate) async fn execute_recover_next_stale_live_v1(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<Option<RuntimeStaleLiveRecoveryReceiptV1>, RuntimeExecutionPersistenceErrorV1> {
    let rows = sqlx::query_as::<_, RuntimeStaleLiveRecoveryRowV1>(RECOVER_STALE_LIVE_QUERY)
        .fetch_all(&mut **transaction)
        .await
        .map_err(map_query_error)?;
    match rows.len() {
        0 => Ok(None),
        1 => Ok(Some(prove_stale_live_recovery_v1(
            rows.into_iter()
                .next()
                .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?
                .decode()?,
        )?)),
        _ => Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt),
    }
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct RuntimeStaleLiveRecoveryRowV1 {
    outcome_name: Option<String>,
    observed_snapshot: Option<Json<Value>>,
    deployment_snapshot: Option<Json<Value>>,
    convergence_attempt_no: Option<i64>,
    loss_kind: Option<String>,
    evidence_at: Option<DateTime<Utc>>,
    recovered_at: Option<DateTime<Utc>>,
}

struct DecodedRuntimeStaleLiveRecoveryRowV1 {
    previous: RuntimeDeployment,
    current: RuntimeDeploymentSnapshotV1,
    convergence_attempt: NonZeroU32,
    loss_kind: LiveLossKindV1,
    evidence_at: DateTime<Utc>,
    recovered_at: DateTime<Utc>,
}

impl RuntimeStaleLiveRecoveryRowV1 {
    fn decode(
        self,
    ) -> Result<DecodedRuntimeStaleLiveRecoveryRowV1, RuntimeExecutionPersistenceErrorV1> {
        let invalid = || RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt;
        if self.outcome_name.as_deref() != Some("applied") {
            return Err(invalid());
        }
        let previous = decode_deployment(self.observed_snapshot.ok_or_else(invalid)?.0)?;
        let current = decode_snapshot(self.deployment_snapshot.ok_or_else(invalid)?.0)?;
        let convergence_attempt = positive_u32(self.convergence_attempt_no.ok_or_else(invalid)?)?;
        let loss_kind = match self.loss_kind.as_deref() {
            Some("serving_lease_expired") => LiveLossKindV1::ServingLeaseExpired,
            Some("serving_disconnected") => LiveLossKindV1::ServingDisconnected,
            _ => return Err(invalid()),
        };
        Ok(DecodedRuntimeStaleLiveRecoveryRowV1 {
            previous,
            current,
            convergence_attempt,
            loss_kind,
            evidence_at: self.evidence_at.ok_or_else(invalid)?,
            recovered_at: self.recovered_at.ok_or_else(invalid)?,
        })
    }
}

fn prove_stale_live_recovery_v1(
    row: DecodedRuntimeStaleLiveRecoveryRowV1,
) -> Result<RuntimeStaleLiveRecoveryReceiptV1, RuntimeExecutionPersistenceErrorV1> {
    let invalid = || RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt;
    let previous_snapshot = row.previous.snapshot();
    if !matches!(previous_snapshot.phase, RuntimeDeploymentPhaseV1::Live)
        || previous_snapshot.controller_lease.is_some()
        || previous_snapshot.live.is_none()
        || previous_snapshot
            .last_runtime_failure
            .as_ref()
            .is_some_and(|failure| {
                matches!(
                    failure,
                    RuntimeFailureDispositionV1::Retryable { attempt, .. }
                        if *attempt > row.convergence_attempt
                )
            })
    {
        return Err(invalid());
    }
    let live = previous_snapshot.live.as_ref().ok_or_else(invalid)?;
    let expected_revision = previous_snapshot.revision;
    let expected_outcome = TransitionOutcomeV1::Applied {
        revision: expected_revision.next().map_err(|_| invalid())?,
    };
    let mut reconstructed = row.previous;
    let outcome = reconstructed
        .recover_live(RecoverLiveRequestV1 {
            expected_revision,
            expected_runtime_generation: previous_snapshot.runtime_generation,
            expected_process_instance_id: live.process_instance_id.clone(),
            kind: row.loss_kind,
            evidence_at: row.evidence_at,
            recovered_at: row.recovered_at,
        })
        .map_err(|_| invalid())?;
    if outcome != expected_outcome || reconstructed.snapshot() != row.current {
        return Err(invalid());
    }
    Ok(RuntimeStaleLiveRecoveryReceiptV1 {
        outcome,
        snapshot: row.current,
    })
}

fn decode_deployment(
    value: Value,
) -> Result<RuntimeDeployment, RuntimeExecutionPersistenceErrorV1> {
    RuntimeDeployment::restore(decode_snapshot(value)?)
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
}

fn decode_snapshot(
    value: Value,
) -> Result<RuntimeDeploymentSnapshotV1, RuntimeExecutionPersistenceErrorV1> {
    serde_json::from_value(value)
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
}

fn positive_u32(value: i64) -> Result<NonZeroU32, RuntimeExecutionPersistenceErrorV1> {
    u32::try_from(value)
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
}

#[cfg(test)]
mod tests {
    use automation_runtime_convergence::{
        ActivationAttestationV1, ActivationOutcomeKindV1, CommandGuardV1, ControllerId,
        DrainAttestationV1, FencingToken, GatewayReadyAttestationV1, GatewayReadyKindV1,
        LeaseRequestV1, PanelCertificateId, PanelCertificateV1, PanelReportDigestV1,
        PreflightAttestationV1, ProcessInstanceId, RuntimeDeploymentIdentityV1,
        RuntimeDeploymentTargetV1, RuntimeGeneration,
    };
    use serde_json::json;

    use super::*;

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + second, 0).unwrap()
    }

    fn live_deployment() -> RuntimeDeployment {
        let target: RuntimeDeploymentTargetV1 = serde_json::from_value(json!({
            "guild_id": "42",
            "ruleset_key": "studyroom",
            "version": 1,
            "content_hash": "2".repeat(64),
            "binding_revision": 1,
            "binding_fingerprint": "3".repeat(64)
        }))
        .unwrap();
        let identity: RuntimeDeploymentIdentityV1 = serde_json::from_value(json!({
            "deployment_id": "deployment",
            "tenant_id": "tenant",
            "installation_id": "installation",
            "promotion_id": "1".repeat(64),
            "activation_request_id": "activation"
        }))
        .unwrap();
        let runtime_generation = RuntimeGeneration::FIRST;
        let controller = ControllerId::parse("controller").unwrap();
        let process = ProcessInstanceId::parse("process").unwrap();
        let mut deployment =
            RuntimeDeployment::request(identity, target.clone(), runtime_generation, None, at(0))
                .unwrap();
        deployment
            .acquire_lease(LeaseRequestV1 {
                expected_revision: deployment.revision(),
                controller_id: controller.clone(),
                fencing_token: FencingToken::FIRST,
                now: at(1),
                expires_at: at(100),
            })
            .unwrap();
        let guard = |deployment: &RuntimeDeployment, now| CommandGuardV1 {
            expected_revision: deployment.revision(),
            controller_id: controller.clone(),
            fencing_token: FencingToken::FIRST,
            runtime_generation,
            now,
        };
        deployment
            .accept_preflight(
                &guard(&deployment, at(2)),
                PreflightAttestationV1 {
                    target: target.clone(),
                    runtime_generation,
                    observed_runtime: None,
                    checked_at: at(2),
                },
            )
            .unwrap();
        deployment
            .request_drain(&guard(&deployment, at(3)))
            .unwrap();
        deployment
            .accept_drain(
                &guard(&deployment, at(4)),
                DrainAttestationV1 {
                    previous_runtime: None,
                    target_runtime_generation: runtime_generation,
                    drained_at: at(4),
                },
            )
            .unwrap();
        deployment
            .begin_activation(&guard(&deployment, at(5)))
            .unwrap();
        deployment
            .accept_activation(
                &guard(&deployment, at(6)),
                ActivationAttestationV1 {
                    activation_request_id:
                        automation_runtime_convergence::ActivationRequestId::parse("activation")
                            .unwrap(),
                    target: target.clone(),
                    runtime_generation,
                    kind: ActivationOutcomeKindV1::Activated,
                    activated_at: at(6),
                },
            )
            .unwrap();
        deployment
            .begin_panel_reconciliation(&guard(&deployment, at(7)))
            .unwrap();
        deployment
            .accept_panel_certificate(
                &guard(&deployment, at(8)),
                PanelCertificateV1 {
                    certificate_id: PanelCertificateId::parse("panel").unwrap(),
                    report_digest: PanelReportDigestV1::parse("4".repeat(64)).unwrap(),
                    target: target.clone(),
                    runtime_generation,
                    process_instance_id: process.clone(),
                    declared_count: 1,
                    installed_count: 1,
                    unchanged_count: 0,
                    skipped_transient_count: 0,
                    skipped_unresolved_channel_count: 0,
                    failed_count: 0,
                    ambiguous_outcome_count: 0,
                    stale_message_cleanup_pending_count: 0,
                    orphan_message_cleanup_pending_count: 0,
                    reposted_old_message_cleanup_pending_count: 0,
                    reconciled_at: at(8),
                },
            )
            .unwrap();
        deployment
            .certify_live(
                &guard(&deployment, at(10)),
                GatewayReadyAttestationV1 {
                    target,
                    runtime_generation,
                    process_instance_id: process,
                    kind: GatewayReadyKindV1::DiscordReady,
                    ready_at: at(9),
                },
                at(10),
            )
            .unwrap();
        deployment
    }

    fn applied_row() -> RuntimeStaleLiveRecoveryRowV1 {
        let previous = live_deployment();
        let previous_snapshot = previous.snapshot();
        let live = previous_snapshot.live.as_ref().unwrap();
        let mut current = previous.clone();
        current
            .recover_live(RecoverLiveRequestV1 {
                expected_revision: previous_snapshot.revision,
                expected_runtime_generation: previous_snapshot.runtime_generation,
                expected_process_instance_id: live.process_instance_id.clone(),
                kind: LiveLossKindV1::ServingLeaseExpired,
                evidence_at: at(11),
                recovered_at: at(12),
            })
            .unwrap();
        RuntimeStaleLiveRecoveryRowV1 {
            outcome_name: Some("applied".to_string()),
            observed_snapshot: Some(Json(serde_json::to_value(previous_snapshot).unwrap())),
            deployment_snapshot: Some(Json(serde_json::to_value(current.snapshot()).unwrap())),
            convergence_attempt_no: Some(1),
            loss_kind: Some("serving_lease_expired".to_string()),
            evidence_at: Some(at(11)),
            recovered_at: Some(at(12)),
        }
    }

    #[test]
    fn recovery_proof_reconstructs_the_exact_domain_transition() {
        let receipt = prove_stale_live_recovery_v1(applied_row().decode().unwrap()).unwrap();
        assert!(matches!(
            receipt.outcome,
            TransitionOutcomeV1::Applied { .. }
        ));
        assert!(matches!(
            receipt.snapshot.phase,
            RuntimeDeploymentPhaseV1::RuntimePending { .. }
        ));
    }

    #[test]
    fn recovery_decoder_and_proof_reject_closed_projection_forgery() {
        let mut wrong_outcome = applied_row();
        wrong_outcome.outcome_name = Some("replayed".to_string());
        assert_eq!(
            wrong_outcome.decode().err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );
        let mut wrong_kind = applied_row();
        wrong_kind.loss_kind = Some("unknown".to_string());
        assert_eq!(
            wrong_kind.decode().err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );
        let mut forged_snapshot = applied_row();
        forged_snapshot.deployment_snapshot.as_mut().unwrap().0["last_live_recovery"]
            ["recovered_at"] = json!(at(13));
        assert_eq!(
            prove_stale_live_recovery_v1(forged_snapshot.decode().unwrap()).err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );
    }

    #[test]
    fn recovery_projection_requires_a_bounded_started_attempt() {
        for invalid_attempt in [0, -1, i64::from(u32::MAX) + 1] {
            let mut row = applied_row();
            row.convergence_attempt_no = Some(invalid_attempt);
            assert_eq!(
                row.decode().err(),
                Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
            );
        }
    }

    #[test]
    fn recovery_query_is_function_only_and_argument_free() {
        assert_eq!(RECOVER_STALE_LIVE_QUERY.matches('$').count(), 0);
        for forbidden in [
            "runtime_deployments",
            "runtime_serving_leases",
            "INSERT ",
            "UPDATE ",
            "DELETE ",
        ] {
            assert!(!RECOVER_STALE_LIVE_QUERY.contains(forbidden));
        }
    }
}
