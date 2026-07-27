use std::num::NonZeroU32;

use automation_runtime_controller::{
    RuntimeCertificationDivergenceV2, RuntimeCertificationIntentFingerprintV2,
    RuntimeCertificationIntentReservationOutcomeV2, RuntimeCertificationOperationIdV2,
    RuntimeCertificationOperationScopeV2, RuntimeCertificationReservationScopeLookupV2,
    RuntimeCertificationReservationScopeObservationV2, RuntimeReservedCertificationIntentV2,
};
use automation_runtime_convergence::{
    RuntimeDeployment, RuntimeDeploymentPhaseV1, RuntimeDeploymentSnapshotV1,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::types::Json;

use crate::RuntimeExecutionPersistenceErrorV1;

#[derive(Clone, Debug, sqlx::FromRow)]
pub(crate) struct RuntimeCertificationReservationRowV2 {
    outcome_name: String,
    locked_snapshot: Option<Json<Value>>,
    locked_convergence_attempt_no: Option<i64>,
    observed_at: DateTime<Utc>,
    operation_id: Option<String>,
    tenant_id: Option<String>,
    installation_id: Option<String>,
    deployment_id: Option<String>,
    deployment_revision: Option<i64>,
    convergence_attempt_no: Option<i64>,
    certification_intent_bytes: Option<Vec<u8>>,
    intent_fingerprint: Option<String>,
}

impl RuntimeCertificationReservationRowV2 {
    pub(crate) fn decode_reservation(
        self,
        proposed: RuntimeReservedCertificationIntentV2,
    ) -> Result<RuntimeCertificationIntentReservationOutcomeV2, RuntimeExecutionPersistenceErrorV1>
    {
        let locked = self.decode_exact_locked_state(proposed.operation_scope())?;
        match self.outcome_name.as_str() {
            "reserved" => {
                let persisted = self.decode_persisted_reservation(proposed.operation_scope())?;
                persisted
                    .require_byte_exact_replay(&proposed)
                    .map_err(|_| invalid())?;
                if !reservation_matches_snapshot(&locked.snapshot, &persisted) {
                    return Err(invalid());
                }
                Ok(RuntimeCertificationIntentReservationOutcomeV2::Reserved(
                    persisted,
                ))
            }
            "diverged" => {
                self.require_empty_payload()?;
                if !reservation_matches_snapshot(&locked.snapshot, &proposed) {
                    return Err(invalid());
                }
                Ok(RuntimeCertificationIntentReservationOutcomeV2::Diverged(
                    RuntimeCertificationDivergenceV2::ReservationMismatch,
                ))
            }
            _ => Err(invalid()),
        }
    }

    pub(crate) fn decode_observation(
        self,
        lookup: RuntimeCertificationReservationScopeLookupV2,
    ) -> Result<RuntimeCertificationReservationScopeObservationV2, RuntimeExecutionPersistenceErrorV1>
    {
        match self.outcome_name.as_str() {
            "absent" => {
                self.require_empty_payload()?;
                let locked = self.decode_exact_locked_state(lookup.operation_scope())?;
                RuntimeCertificationReservationScopeObservationV2::absent(
                    lookup,
                    locked.snapshot,
                    self.observed_at,
                )
                .map_err(|_| invalid())
            }
            "reserved" => {
                let locked = self.decode_exact_locked_state(lookup.operation_scope())?;
                let persisted = self.decode_persisted_reservation(lookup.operation_scope())?;
                RuntimeCertificationReservationScopeObservationV2::reserved(
                    lookup,
                    locked.snapshot,
                    persisted,
                    self.observed_at,
                )
                .map_err(|_| invalid())
            }
            "diverged" => self.decode_observation_divergence(lookup),
            _ => Err(invalid()),
        }
    }

    fn decode_exact_locked_state(
        &self,
        expected: &RuntimeCertificationOperationScopeV2,
    ) -> Result<RuntimeCertificationLockedStateV2, RuntimeExecutionPersistenceErrorV1> {
        let snapshot =
            decode_snapshot(self.locked_snapshot.as_ref().ok_or_else(invalid)?.0.clone())?;
        let convergence_attempt =
            positive_u32(self.locked_convergence_attempt_no.ok_or_else(invalid)?)?;
        if !expected.scope().matches(&snapshot.identity)
            || expected.deployment_revision() != snapshot.revision
            || expected.convergence_attempt() != convergence_attempt
        {
            return Err(invalid());
        }
        Ok(RuntimeCertificationLockedStateV2 { snapshot })
    }

    fn decode_persisted_reservation(
        &self,
        expected: &RuntimeCertificationOperationScopeV2,
    ) -> Result<RuntimeReservedCertificationIntentV2, RuntimeExecutionPersistenceErrorV1> {
        if self.payload_shape() != RuntimeCertificationReservationPayloadShapeV2::Complete
            || self.tenant_id.as_deref() != Some(expected.scope().tenant_id.as_str())
            || self.installation_id.as_deref() != Some(expected.scope().installation_id.as_str())
            || self.deployment_id.as_deref() != Some(expected.scope().deployment_id.as_str())
            || self.deployment_revision != i64_value(expected.deployment_revision().get())
            || self.convergence_attempt_no != Some(i64::from(expected.convergence_attempt().get()))
        {
            return Err(invalid());
        }
        let operation_id = RuntimeCertificationOperationIdV2::parse(
            self.operation_id.clone().ok_or_else(invalid)?,
        )
        .map_err(|_| invalid())?;
        let fingerprint = RuntimeCertificationIntentFingerprintV2::parse(
            self.intent_fingerprint.clone().ok_or_else(invalid)?,
        )
        .map_err(|_| invalid())?;
        RuntimeReservedCertificationIntentV2::from_persisted(
            expected.scope().clone(),
            expected.deployment_revision(),
            expected.convergence_attempt(),
            &operation_id,
            self.certification_intent_bytes
                .as_deref()
                .ok_or_else(invalid)?,
            &fingerprint,
        )
        .map_err(|_| invalid())
    }

    fn decode_observation_divergence(
        self,
        lookup: RuntimeCertificationReservationScopeLookupV2,
    ) -> Result<RuntimeCertificationReservationScopeObservationV2, RuntimeExecutionPersistenceErrorV1>
    {
        self.require_empty_payload()?;
        let divergence = match (
            self.locked_snapshot.as_ref(),
            self.locked_convergence_attempt_no,
        ) {
            (None, None) => RuntimeCertificationDivergenceV2::OwnershipLost,
            (Some(snapshot), Some(convergence_attempt)) => {
                let snapshot = decode_snapshot(snapshot.0.clone())?;
                let convergence_attempt = positive_u32(convergence_attempt)?;
                classify_locked_divergence(lookup.operation_scope(), snapshot, convergence_attempt)?
            }
            _ => return Err(invalid()),
        };
        Ok(RuntimeCertificationReservationScopeObservationV2::diverged(
            divergence,
        ))
    }

    fn require_empty_payload(&self) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
        if self.payload_shape() == RuntimeCertificationReservationPayloadShapeV2::Empty {
            Ok(())
        } else {
            Err(invalid())
        }
    }

    fn payload_shape(&self) -> RuntimeCertificationReservationPayloadShapeV2 {
        let fields = [
            self.operation_id.is_some(),
            self.tenant_id.is_some(),
            self.installation_id.is_some(),
            self.deployment_id.is_some(),
            self.deployment_revision.is_some(),
            self.convergence_attempt_no.is_some(),
            self.certification_intent_bytes.is_some(),
            self.intent_fingerprint.is_some(),
        ];
        if fields.iter().all(|present| *present) {
            RuntimeCertificationReservationPayloadShapeV2::Complete
        } else if fields.iter().all(|present| !*present) {
            RuntimeCertificationReservationPayloadShapeV2::Empty
        } else {
            RuntimeCertificationReservationPayloadShapeV2::Partial
        }
    }
}

struct RuntimeCertificationLockedStateV2 {
    snapshot: RuntimeDeploymentSnapshotV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeCertificationReservationPayloadShapeV2 {
    Empty,
    Complete,
    Partial,
}

fn classify_locked_divergence(
    expected: &RuntimeCertificationOperationScopeV2,
    snapshot: RuntimeDeploymentSnapshotV1,
    convergence_attempt: NonZeroU32,
) -> Result<RuntimeCertificationDivergenceV2, RuntimeExecutionPersistenceErrorV1> {
    if !expected.scope().matches(&snapshot.identity) {
        return Err(invalid());
    }
    if snapshot.revision < expected.deployment_revision() {
        return Err(invalid());
    }
    if snapshot.revision > expected.deployment_revision() {
        return Ok(match snapshot.phase {
            RuntimeDeploymentPhaseV1::Superseded { .. } => {
                RuntimeCertificationDivergenceV2::Superseded { snapshot }
            }
            RuntimeDeploymentPhaseV1::Live | RuntimeDeploymentPhaseV1::Cancelled { .. } => {
                RuntimeCertificationDivergenceV2::Terminal { snapshot }
            }
            _ => RuntimeCertificationDivergenceV2::DeploymentAdvanced { snapshot },
        });
    }
    if !matches!(
        snapshot.phase,
        RuntimeDeploymentPhaseV1::AwaitingGatewayReady
    ) {
        return Err(invalid());
    }
    if convergence_attempt != expected.convergence_attempt() {
        return Ok(RuntimeCertificationDivergenceV2::AuthorityChanged { snapshot });
    }
    Ok(RuntimeCertificationDivergenceV2::PersistenceCorrupt)
}

fn reservation_matches_snapshot(
    snapshot: &RuntimeDeploymentSnapshotV1,
    reservation: &RuntimeReservedCertificationIntentV2,
) -> bool {
    let intent = reservation.canonical_intent().intent();
    let guard = &intent.guard;
    let Some(lease) = snapshot.controller_lease.as_ref() else {
        return false;
    };
    let Some(panel) = snapshot.panel_certificate.as_ref() else {
        return false;
    };
    RuntimeDeployment::restore(snapshot.clone()).is_ok()
        && matches!(
            snapshot.phase,
            RuntimeDeploymentPhaseV1::AwaitingGatewayReady
        )
        && reservation
            .operation_scope()
            .scope()
            .matches(&snapshot.identity)
        && reservation.operation_scope().deployment_revision() == snapshot.revision
        && guard.scope.matches(&snapshot.identity)
        && guard.expected_revision == snapshot.revision
        && guard.convergence_attempt == reservation.operation_scope().convergence_attempt()
        && guard.controller_id == lease.controller_id
        && guard.fencing_token == lease.fencing_token
        && snapshot.last_fencing_token == Some(guard.fencing_token)
        && guard.runtime_generation == snapshot.runtime_generation
        && intent.target == snapshot.target
        && intent.binding_pin.matches(&guard.scope, &intent.target)
        && intent.process_identity.target == snapshot.target
        && intent.process_identity.runtime_generation == snapshot.runtime_generation
        && intent.panel.certificate_id == panel.certificate_id
        && intent.panel.report_digest == panel.report_digest
        && intent.panel.process_identity.target == panel.target
        && intent.panel.process_identity.runtime_generation == panel.runtime_generation
        && intent.panel.process_identity.process_instance_id == panel.process_instance_id
        && intent.panel.controller_fencing_token == lease.fencing_token
}

fn decode_snapshot(
    value: Value,
) -> Result<RuntimeDeploymentSnapshotV1, RuntimeExecutionPersistenceErrorV1> {
    let snapshot =
        serde_json::from_value::<RuntimeDeploymentSnapshotV1>(value).map_err(|_| invalid())?;
    RuntimeDeployment::restore(snapshot.clone()).map_err(|_| invalid())?;
    Ok(snapshot)
}

fn positive_u32(value: i64) -> Result<NonZeroU32, RuntimeExecutionPersistenceErrorV1> {
    u32::try_from(value)
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or_else(invalid)
}

fn i64_value(value: u64) -> Option<i64> {
    i64::try_from(value).ok()
}

fn invalid() -> RuntimeExecutionPersistenceErrorV1 {
    RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU32, NonZeroU64};
    use std::time::Duration;

    use automation_runtime_controller::{
        GatewayShardIdV1, RuntimeBindingPinV1, RuntimeBuildRevisionV1,
        RuntimeCanonicalCertificationIntentV2, RuntimeCertificationIntentV2,
        RuntimeCertificationReservationScopeObservationKindV2, RuntimeConvergenceSessionV1,
        RuntimeDeploymentScopeV1, RuntimeExecutionGuardV1, RuntimeExecutionReceiptV1,
        RuntimeGatewayOwnerLeaseIdV1, RuntimePanelEvidenceV2,
    };
    use automation_runtime_convergence::{
        ActivationAttestationV1, ActivationOutcomeKindV1, CommandGuardV1, ControllerId,
        DrainAttestationV1, FencingToken, GatewayReadyAttestationV1, GatewayReadyKindV1,
        LeaseRequestV1, PanelCertificateId, PanelCertificateV1, PanelReportDigestV1,
        PreflightAttestationV1, ProcessInstanceId, RuntimeDeploymentIdentityV1,
        RuntimeDeploymentTargetV1, RuntimeFailureId, RuntimeFailureKindV1, RuntimeFailureV1,
        RuntimeGeneration, RuntimeProcessIdentityV1, SupersedingDeploymentV1,
    };
    use serde_json::json;

    use super::*;

    struct FixtureV2 {
        execution: RuntimeExecutionReceiptV1,
        reservation: RuntimeReservedCertificationIntentV2,
        lookup: RuntimeCertificationReservationScopeLookupV2,
    }

    fn non_zero(value: u64) -> NonZeroU64 {
        NonZeroU64::new(value).unwrap()
    }

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + second, 0).unwrap()
    }

    fn target() -> RuntimeDeploymentTargetV1 {
        serde_json::from_value(json!({
            "guild_id": "42",
            "ruleset_key": "studyroom",
            "version": 1,
            "content_hash": "2".repeat(64),
            "binding_revision": 1,
            "binding_fingerprint": "3".repeat(64)
        }))
        .unwrap()
    }

    fn deployment_identity(suffix: &str) -> RuntimeDeploymentIdentityV1 {
        serde_json::from_value(json!({
            "deployment_id": format!("deployment:{suffix}"),
            "tenant_id": format!("tenant:{suffix}"),
            "installation_id": format!("installation:{suffix}"),
            "promotion_id": "a".repeat(64),
            "activation_request_id": format!("activation:{suffix}")
        }))
        .unwrap()
    }

    fn command_guard(
        deployment: &RuntimeDeployment,
        controller_id: &ControllerId,
        fencing_token: FencingToken,
        now: DateTime<Utc>,
    ) -> CommandGuardV1 {
        CommandGuardV1 {
            expected_revision: deployment.revision(),
            controller_id: controller_id.clone(),
            fencing_token,
            runtime_generation: deployment.runtime_generation(),
            now,
        }
    }

    fn awaiting_execution(suffix: &str) -> RuntimeExecutionReceiptV1 {
        let target = target();
        let runtime_generation = RuntimeGeneration::new(4).unwrap();
        let controller_id = ControllerId::parse(format!("controller:{suffix}")).unwrap();
        let fencing_token = FencingToken::new(3).unwrap();
        let process_instance_id = ProcessInstanceId::parse(format!("process:{suffix}")).unwrap();
        let mut deployment = RuntimeDeployment::request(
            deployment_identity(suffix),
            target.clone(),
            runtime_generation,
            None,
            at(0),
        )
        .unwrap();
        deployment
            .acquire_lease(LeaseRequestV1 {
                expected_revision: deployment.revision(),
                controller_id: controller_id.clone(),
                fencing_token,
                now: at(1),
                expires_at: at(100),
            })
            .unwrap();
        deployment
            .accept_preflight(
                &command_guard(&deployment, &controller_id, fencing_token, at(2)),
                PreflightAttestationV1 {
                    target: target.clone(),
                    runtime_generation,
                    observed_runtime: None,
                    checked_at: at(2),
                },
            )
            .unwrap();
        deployment
            .request_drain(&command_guard(
                &deployment,
                &controller_id,
                fencing_token,
                at(3),
            ))
            .unwrap();
        deployment
            .accept_drain(
                &command_guard(&deployment, &controller_id, fencing_token, at(4)),
                DrainAttestationV1 {
                    previous_runtime: None,
                    target_runtime_generation: runtime_generation,
                    drained_at: at(4),
                },
            )
            .unwrap();
        deployment
            .begin_activation(&command_guard(
                &deployment,
                &controller_id,
                fencing_token,
                at(5),
            ))
            .unwrap();
        deployment
            .accept_activation(
                &command_guard(&deployment, &controller_id, fencing_token, at(6)),
                ActivationAttestationV1 {
                    activation_request_id: deployment.identity().activation_request_id.clone(),
                    target: target.clone(),
                    runtime_generation,
                    kind: ActivationOutcomeKindV1::Activated,
                    activated_at: at(6),
                },
            )
            .unwrap();
        deployment
            .begin_panel_reconciliation(&command_guard(
                &deployment,
                &controller_id,
                fencing_token,
                at(7),
            ))
            .unwrap();
        deployment
            .accept_panel_certificate(
                &command_guard(&deployment, &controller_id, fencing_token, at(8)),
                PanelCertificateV1 {
                    certificate_id: PanelCertificateId::parse(format!("panel:{suffix}")).unwrap(),
                    report_digest: PanelReportDigestV1::parse("4".repeat(64)).unwrap(),
                    target,
                    runtime_generation,
                    process_instance_id,
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
        RuntimeExecutionReceiptV1 {
            snapshot: deployment.snapshot(),
            controller_id,
            fencing_token,
            convergence_attempt: NonZeroU32::new(5).unwrap(),
            acquired_at: at(1),
            expires_at: at(100),
        }
    }

    fn reserved_intent(
        execution: &RuntimeExecutionReceiptV1,
    ) -> RuntimeReservedCertificationIntentV2 {
        let snapshot = &execution.snapshot;
        let panel = snapshot.panel_certificate.as_ref().unwrap();
        let action_id = RuntimeConvergenceSessionV1::from_claim(execution.clone())
            .unwrap()
            .begin_renewal(Duration::from_secs(1))
            .unwrap()
            .action_id;
        let process_identity = RuntimeProcessIdentityV1 {
            target: snapshot.target.clone(),
            runtime_generation: snapshot.runtime_generation,
            process_instance_id: panel.process_instance_id.clone(),
        };
        let intent = RuntimeCertificationIntentV2 {
            action_id,
            operation_id: RuntimeCertificationOperationIdV2::parse(
                "00112233445566778899aabbccddeeff",
            )
            .unwrap(),
            guard: RuntimeExecutionGuardV1 {
                scope: RuntimeDeploymentScopeV1::from_identity(&snapshot.identity),
                expected_revision: snapshot.revision,
                controller_id: execution.controller_id.clone(),
                fencing_token: execution.fencing_token,
                runtime_generation: snapshot.runtime_generation,
                convergence_attempt: execution.convergence_attempt,
            },
            target: snapshot.target.clone(),
            binding_pin: RuntimeBindingPinV1 {
                tenant_id: snapshot.identity.tenant_id.clone(),
                installation_id: snapshot.identity.installation_id.clone(),
                installation_authority_revision: non_zero(6),
                binding_revision: snapshot.target.binding_revision,
                binding_fingerprint: snapshot.target.binding_fingerprint.clone(),
            },
            process_identity: process_identity.clone(),
            gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1 {
                gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
                process_instance_id: process_identity.process_instance_id.clone(),
                lease_epoch: non_zero(5),
                expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
            },
            observed_owner_revision: non_zero(7),
            runtime_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
            panel: RuntimePanelEvidenceV2 {
                certificate_id: panel.certificate_id.clone(),
                report_digest: panel.report_digest.clone(),
                process_identity,
                controller_fencing_token: execution.fencing_token,
            },
            serving_lease_for: Duration::from_secs(30),
        };
        RuntimeReservedCertificationIntentV2::new(
            execution,
            RuntimeCanonicalCertificationIntentV2::new(intent).unwrap(),
        )
        .unwrap()
    }

    fn fixture(suffix: &str) -> FixtureV2 {
        let execution = awaiting_execution(suffix);
        let reservation = reserved_intent(&execution);
        let lookup =
            RuntimeCertificationReservationScopeLookupV2::from_awaiting_execution(&execution)
                .unwrap();
        FixtureV2 {
            execution,
            reservation,
            lookup,
        }
    }

    fn empty_row(fixture: &FixtureV2, outcome_name: &str) -> RuntimeCertificationReservationRowV2 {
        RuntimeCertificationReservationRowV2 {
            outcome_name: outcome_name.to_string(),
            locked_snapshot: Some(Json(
                serde_json::to_value(&fixture.execution.snapshot).unwrap(),
            )),
            locked_convergence_attempt_no: Some(i64::from(
                fixture.execution.convergence_attempt.get(),
            )),
            observed_at: at(20),
            operation_id: None,
            tenant_id: None,
            installation_id: None,
            deployment_id: None,
            deployment_revision: None,
            convergence_attempt_no: None,
            certification_intent_bytes: None,
            intent_fingerprint: None,
        }
    }

    fn reserved_row(fixture: &FixtureV2) -> RuntimeCertificationReservationRowV2 {
        let operation_scope = fixture.reservation.operation_scope();
        RuntimeCertificationReservationRowV2 {
            outcome_name: "reserved".to_string(),
            locked_snapshot: Some(Json(
                serde_json::to_value(&fixture.execution.snapshot).unwrap(),
            )),
            locked_convergence_attempt_no: Some(i64::from(
                operation_scope.convergence_attempt().get(),
            )),
            observed_at: at(20),
            operation_id: Some(fixture.reservation.operation_id().as_str().to_string()),
            tenant_id: Some(operation_scope.scope().tenant_id.as_str().to_string()),
            installation_id: Some(operation_scope.scope().installation_id.as_str().to_string()),
            deployment_id: Some(operation_scope.scope().deployment_id.as_str().to_string()),
            deployment_revision: Some(
                i64::try_from(operation_scope.deployment_revision().get()).unwrap(),
            ),
            convergence_attempt_no: Some(i64::from(operation_scope.convergence_attempt().get())),
            certification_intent_bytes: Some(
                fixture.reservation.certification_intent_bytes().to_vec(),
            ),
            intent_fingerprint: Some(
                fixture
                    .reservation
                    .intent_fingerprint()
                    .as_str()
                    .to_string(),
            ),
        }
    }

    fn assert_corrupt<T>(result: Result<T, RuntimeExecutionPersistenceErrorV1>) {
        assert!(matches!(
            result,
            Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        ));
    }

    #[test]
    fn reservation_decode_accepts_only_exact_reserved_or_closed_divergence_rows() {
        let fixture = fixture("one");
        let reserved = reserved_row(&fixture)
            .decode_reservation(fixture.reservation.clone())
            .unwrap();
        assert!(matches!(
            reserved,
            RuntimeCertificationIntentReservationOutcomeV2::Reserved(observed)
                if observed == fixture.reservation
        ));

        let diverged = empty_row(&fixture, "diverged")
            .decode_reservation(fixture.reservation.clone())
            .unwrap();
        assert_eq!(
            diverged,
            RuntimeCertificationIntentReservationOutcomeV2::Diverged(
                RuntimeCertificationDivergenceV2::ReservationMismatch
            )
        );

        assert_corrupt(
            empty_row(&fixture, "absent").decode_reservation(fixture.reservation.clone()),
        );
        assert_corrupt(
            empty_row(&fixture, "unknown").decode_reservation(fixture.reservation.clone()),
        );
        assert_corrupt(
            empty_row(&fixture, "reserved").decode_reservation(fixture.reservation.clone()),
        );
    }

    #[test]
    fn observation_decode_accepts_exact_absent_reserved_and_missing_scope_rows() {
        let fixture = fixture("one");
        let absent = empty_row(&fixture, "absent")
            .decode_observation(fixture.lookup.clone())
            .unwrap();
        assert!(matches!(
            absent.kind(),
            RuntimeCertificationReservationScopeObservationKindV2::Absent {
                lookup,
                snapshot,
                observed_at,
            } if lookup == &fixture.lookup
                && snapshot == &fixture.execution.snapshot
                && *observed_at == at(20)
        ));

        let reserved = reserved_row(&fixture)
            .decode_observation(fixture.lookup.clone())
            .unwrap();
        assert!(matches!(
            reserved.kind(),
            RuntimeCertificationReservationScopeObservationKindV2::Reserved {
                lookup,
                snapshot,
                reservation,
                observed_at,
            } if lookup == &fixture.lookup
                && snapshot == &fixture.execution.snapshot
                && reservation == &fixture.reservation
                && *observed_at == at(20)
        ));

        let mut missing = empty_row(&fixture, "diverged");
        missing.locked_snapshot = None;
        missing.locked_convergence_attempt_no = None;
        let missing = missing.decode_observation(fixture.lookup.clone()).unwrap();
        assert!(matches!(
            missing.kind(),
            RuntimeCertificationReservationScopeObservationKindV2::Diverged(
                RuntimeCertificationDivergenceV2::OwnershipLost
            )
        ));
    }

    #[test]
    fn every_partial_reservation_payload_is_rejected() {
        let fixture = fixture("one");
        let complete = reserved_row(&fixture);
        let empty = empty_row(&fixture, "absent");
        for mask in 1_u16..u16::from(u8::MAX) {
            let mut row = empty.clone();
            if mask & (1 << 0) != 0 {
                row.operation_id.clone_from(&complete.operation_id);
            }
            if mask & (1 << 1) != 0 {
                row.tenant_id.clone_from(&complete.tenant_id);
            }
            if mask & (1 << 2) != 0 {
                row.installation_id.clone_from(&complete.installation_id);
            }
            if mask & (1 << 3) != 0 {
                row.deployment_id.clone_from(&complete.deployment_id);
            }
            if mask & (1 << 4) != 0 {
                row.deployment_revision = complete.deployment_revision;
            }
            if mask & (1 << 5) != 0 {
                row.convergence_attempt_no = complete.convergence_attempt_no;
            }
            if mask & (1 << 6) != 0 {
                row.certification_intent_bytes
                    .clone_from(&complete.certification_intent_bytes);
            }
            if mask & (1 << 7) != 0 {
                row.intent_fingerprint
                    .clone_from(&complete.intent_fingerprint);
            }
            assert_corrupt(row.decode_observation(fixture.lookup.clone()));
        }
    }

    #[test]
    fn outcome_payload_and_locked_shapes_are_closed() {
        let fixture = fixture("one");

        let mut complete_absent = reserved_row(&fixture);
        complete_absent.outcome_name = "absent".to_string();
        assert_corrupt(complete_absent.decode_observation(fixture.lookup.clone()));

        let mut complete_diverged = reserved_row(&fixture);
        complete_diverged.outcome_name = "diverged".to_string();
        assert_corrupt(complete_diverged.decode_observation(fixture.lookup.clone()));

        let mut unknown = empty_row(&fixture, "unknown");
        assert_corrupt(unknown.clone().decode_observation(fixture.lookup.clone()));
        unknown.outcome_name = "reserved".to_string();
        assert_corrupt(unknown.decode_observation(fixture.lookup.clone()));

        let mut snapshot_only = empty_row(&fixture, "diverged");
        snapshot_only.locked_convergence_attempt_no = None;
        assert_corrupt(snapshot_only.decode_observation(fixture.lookup.clone()));

        let mut attempt_only = empty_row(&fixture, "diverged");
        attempt_only.locked_snapshot = None;
        assert_corrupt(attempt_only.decode_observation(fixture.lookup.clone()));

        let mut reserve_without_lock = empty_row(&fixture, "diverged");
        reserve_without_lock.locked_snapshot = None;
        reserve_without_lock.locked_convergence_attempt_no = None;
        assert_corrupt(reserve_without_lock.decode_reservation(fixture.reservation.clone()));
    }

    #[test]
    fn full_row_scope_attempt_snapshot_and_canonical_mismatches_are_rejected() {
        let other = fixture("other");
        let fixture = fixture("one");

        let mut wrong_locked_scope = reserved_row(&fixture);
        wrong_locked_scope.locked_snapshot = Some(Json(
            serde_json::to_value(&other.execution.snapshot).unwrap(),
        ));
        assert_corrupt(wrong_locked_scope.decode_reservation(fixture.reservation.clone()));

        let mut wrong_locked_attempt = reserved_row(&fixture);
        wrong_locked_attempt.locked_convergence_attempt_no = Some(6);
        assert_corrupt(wrong_locked_attempt.decode_observation(fixture.lookup.clone()));

        let mut natural_rows = Vec::new();
        let mut row = reserved_row(&fixture);
        row.tenant_id = Some("tenant:other".to_string());
        natural_rows.push(row);
        let mut row = reserved_row(&fixture);
        row.installation_id = Some("installation:other".to_string());
        natural_rows.push(row);
        let mut row = reserved_row(&fixture);
        row.deployment_id = Some("deployment:other".to_string());
        natural_rows.push(row);
        let mut row = reserved_row(&fixture);
        row.deployment_revision = row
            .deployment_revision
            .and_then(|value| value.checked_add(1));
        natural_rows.push(row);
        let mut row = reserved_row(&fixture);
        row.convergence_attempt_no = Some(6);
        natural_rows.push(row);
        for row in natural_rows {
            assert_corrupt(row.decode_observation(fixture.lookup.clone()));
        }

        let mut wrong_operation = reserved_row(&fixture);
        wrong_operation.operation_id = Some("ffeeddccbbaa99887766554433221100".to_string());
        assert_corrupt(wrong_operation.decode_observation(fixture.lookup.clone()));

        let mut wrong_bytes = reserved_row(&fixture);
        wrong_bytes
            .certification_intent_bytes
            .as_mut()
            .unwrap()
            .push(b' ');
        assert_corrupt(wrong_bytes.decode_observation(fixture.lookup.clone()));

        let mut wrong_fingerprint = reserved_row(&fixture);
        wrong_fingerprint.intent_fingerprint = Some("f".repeat(64));
        assert_corrupt(wrong_fingerprint.decode_observation(fixture.lookup.clone()));

        let mut wrong_panel = fixture.execution.snapshot.clone();
        wrong_panel
            .panel_certificate
            .as_mut()
            .unwrap()
            .report_digest = PanelReportDigestV1::parse("e".repeat(64)).unwrap();
        let mut wrong_panel_row = reserved_row(&fixture);
        wrong_panel_row.locked_snapshot = Some(Json(serde_json::to_value(wrong_panel).unwrap()));
        assert_corrupt(wrong_panel_row.decode_reservation(fixture.reservation.clone()));
    }

    #[test]
    fn locked_divergence_classification_is_closed_and_snapshot_backed() {
        let fixture = fixture("one");

        let exact = empty_row(&fixture, "diverged")
            .decode_observation(fixture.lookup.clone())
            .unwrap();
        assert!(matches!(
            exact.kind(),
            RuntimeCertificationReservationScopeObservationKindV2::Diverged(
                RuntimeCertificationDivergenceV2::PersistenceCorrupt
            )
        ));

        let mut authority = empty_row(&fixture, "diverged");
        authority.locked_convergence_attempt_no = Some(6);
        let authority = authority
            .decode_observation(fixture.lookup.clone())
            .unwrap();
        assert!(matches!(
            authority.kind(),
            RuntimeCertificationReservationScopeObservationKindV2::Diverged(
                RuntimeCertificationDivergenceV2::AuthorityChanged { snapshot }
            ) if snapshot == &fixture.execution.snapshot
        ));

        let advanced_snapshot = retryable_snapshot(&fixture.execution);
        let mut advanced = empty_row(&fixture, "diverged");
        advanced.locked_snapshot = Some(Json(serde_json::to_value(&advanced_snapshot).unwrap()));
        let advanced = advanced.decode_observation(fixture.lookup.clone()).unwrap();
        assert!(matches!(
            advanced.kind(),
            RuntimeCertificationReservationScopeObservationKindV2::Diverged(
                RuntimeCertificationDivergenceV2::DeploymentAdvanced { snapshot }
            ) if snapshot == &advanced_snapshot
        ));

        let live_snapshot = live_snapshot(&fixture.execution);
        let mut live = empty_row(&fixture, "diverged");
        live.locked_snapshot = Some(Json(serde_json::to_value(&live_snapshot).unwrap()));
        let live = live.decode_observation(fixture.lookup.clone()).unwrap();
        assert!(matches!(
            live.kind(),
            RuntimeCertificationReservationScopeObservationKindV2::Diverged(
                RuntimeCertificationDivergenceV2::Terminal { snapshot }
            ) if snapshot == &live_snapshot
        ));

        let superseded_snapshot = superseded_snapshot(&fixture.execution);
        let mut superseded = empty_row(&fixture, "diverged");
        superseded.locked_snapshot =
            Some(Json(serde_json::to_value(&superseded_snapshot).unwrap()));
        let superseded = superseded
            .decode_observation(fixture.lookup.clone())
            .unwrap();
        assert!(matches!(
            superseded.kind(),
            RuntimeCertificationReservationScopeObservationKindV2::Diverged(
                RuntimeCertificationDivergenceV2::Superseded { snapshot }
            ) if snapshot == &superseded_snapshot
        ));
    }

    fn retryable_snapshot(execution: &RuntimeExecutionReceiptV1) -> RuntimeDeploymentSnapshotV1 {
        let mut deployment = RuntimeDeployment::restore(execution.snapshot.clone()).unwrap();
        deployment
            .record_retryable_failure(
                &command_guard(
                    &deployment,
                    &execution.controller_id,
                    execution.fencing_token,
                    at(10),
                ),
                RuntimeFailureV1 {
                    failure_id: RuntimeFailureId::parse("failure:1").unwrap(),
                    kind: RuntimeFailureKindV1::GatewayStart,
                    code: "gateway_start".to_string(),
                    message: "gateway start failed".to_string(),
                    recorded_at: at(10),
                },
                NonZeroU32::new(6).unwrap(),
                at(11),
            )
            .unwrap();
        deployment.snapshot()
    }

    fn live_snapshot(execution: &RuntimeExecutionReceiptV1) -> RuntimeDeploymentSnapshotV1 {
        let mut deployment = RuntimeDeployment::restore(execution.snapshot.clone()).unwrap();
        let panel = execution.snapshot.panel_certificate.as_ref().unwrap();
        deployment
            .certify_live(
                &command_guard(
                    &deployment,
                    &execution.controller_id,
                    execution.fencing_token,
                    at(10),
                ),
                GatewayReadyAttestationV1 {
                    target: execution.snapshot.target.clone(),
                    runtime_generation: execution.snapshot.runtime_generation,
                    process_instance_id: panel.process_instance_id.clone(),
                    kind: GatewayReadyKindV1::DiscordReady,
                    ready_at: at(9),
                },
                at(10),
            )
            .unwrap();
        deployment.snapshot()
    }

    fn superseded_snapshot(execution: &RuntimeExecutionReceiptV1) -> RuntimeDeploymentSnapshotV1 {
        let mut deployment = RuntimeDeployment::restore(execution.snapshot.clone()).unwrap();
        let identity = &execution.snapshot.identity;
        deployment
            .supersede(
                &command_guard(
                    &deployment,
                    &execution.controller_id,
                    execution.fencing_token,
                    at(10),
                ),
                SupersedingDeploymentV1 {
                    identity: RuntimeDeploymentIdentityV1 {
                        deployment_id: automation_runtime_convergence::DeploymentId::parse(
                            "deployment:successor",
                        )
                        .unwrap(),
                        tenant_id: identity.tenant_id.clone(),
                        installation_id: identity.installation_id.clone(),
                        promotion_id: identity.promotion_id.clone(),
                        activation_request_id: identity.activation_request_id.clone(),
                    },
                    target: execution.snapshot.target.clone(),
                    runtime_generation: execution.snapshot.runtime_generation.next().unwrap(),
                },
                "replaced".to_string(),
                at(10),
            )
            .unwrap();
        deployment.snapshot()
    }
}
