use std::num::{NonZeroU32, NonZeroU64};
use std::time::Duration;

use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    ActivationAttestationV1, ActivationOutcomeKindV1, ActivationRequestId, BindingRevision,
    CommandGuardV1, ControllerId, DeploymentId, DrainAttestationV1, FencingToken,
    GatewayReadyAttestationV1, GatewayReadyKindV1, InstallationId, LeaseRequestV1,
    PanelCertificateId, PanelCertificateV1, PanelReportDigestV1, PreflightAttestationV1,
    ProcessInstanceId, PromotionId, RuntimeDeployment, RuntimeDeploymentIdentityV1,
    RuntimeDeploymentPhaseV1, RuntimeDeploymentSnapshotV1, RuntimeDeploymentTargetV1,
    RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use chrono::{DateTime, Utc};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;

use super::{
    RuntimeCertificationReservationAuthorityV2, RuntimeCertificationReservationInputV2,
    RuntimeCertificationSessionErrorV2,
};
use crate::{
    GatewayShardIdV1, RuntimeBarrierIdV1, RuntimeBarrierPauseWitnessV2, RuntimeBindingPinV1,
    RuntimeBuildRevisionV1, RuntimeCertificationDivergenceV2,
    RuntimeCertificationIntentReservationOutcomeV2, RuntimeCertificationOperationIdV2,
    RuntimeCertificationReceiptV2, RuntimeCertificationRequestV2,
    RuntimeCertificationReservationScopeLookupV2,
    RuntimeCertificationReservationScopeObservationV2, RuntimeConvergenceMutationV1,
    RuntimeConvergenceSessionError, RuntimeConvergenceSessionStateV1, RuntimeConvergenceSessionV1,
    RuntimeDeploymentScopeV1, RuntimeExecutionReceiptV1, RuntimeGatewayAdmissionSequenceV2,
    RuntimeGatewayOwnerLeaseIdV1, RuntimeGatewayReadyAttestationV2, RuntimeGatewayReadyKindV2,
    RuntimeLiveAttestationRecordV2, RuntimePanelEvidenceV2, RuntimeRouteAdmissionAttestationV2,
    RuntimeServingIdentityV2, RuntimeServingReceiptV2, RuntimeServingRouteAttestationV2,
};

fn at(second: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(second, 0).unwrap()
}

fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn target() -> RuntimeDeploymentTargetV1 {
    RuntimeDeploymentTargetV1 {
        guild_id: GuildId(7),
        ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
        version: RuleSetVersionId::FIRST,
        content_hash: RuleSetContentHash::parse_hex(&"b".repeat(64)).unwrap(),
        binding_revision: BindingRevision::new(3).unwrap(),
        binding_fingerprint: ResourceBindingFingerprint::parse(&"a".repeat(64)).unwrap(),
    }
}

fn deployment_identity() -> RuntimeDeploymentIdentityV1 {
    RuntimeDeploymentIdentityV1 {
        deployment_id: DeploymentId::parse("deployment:1").unwrap(),
        tenant_id: TenantId::parse("tenant:1").unwrap(),
        installation_id: InstallationId::parse("installation:1").unwrap(),
        promotion_id: PromotionId::parse("9".repeat(64)).unwrap(),
        activation_request_id: ActivationRequestId::parse("activation:1").unwrap(),
    }
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

fn claimed_execution() -> RuntimeExecutionReceiptV1 {
    let mut deployment = RuntimeDeployment::request(
        deployment_identity(),
        target(),
        RuntimeGeneration::new(4).unwrap(),
        None,
        at(1),
    )
    .unwrap();
    let controller_id = ControllerId::parse("controller:1").unwrap();
    let fencing_token = FencingToken::new(3).unwrap();
    deployment
        .acquire_lease(LeaseRequestV1 {
            expected_revision: deployment.revision(),
            controller_id: controller_id.clone(),
            fencing_token,
            now: at(10),
            expires_at: at(100),
        })
        .unwrap();
    RuntimeExecutionReceiptV1 {
        snapshot: deployment.snapshot(),
        controller_id,
        fencing_token,
        convergence_attempt: NonZeroU32::new(5).unwrap(),
        acquired_at: at(10),
        expires_at: at(100),
    }
}

fn mutation_receipt(
    session: &RuntimeConvergenceSessionV1,
    request: &crate::RuntimeMutationRequestV1,
) -> crate::RuntimeMutationReceiptV1 {
    let mut deployment = RuntimeDeployment::restore(session.snapshot().clone()).unwrap();
    let guard = command_guard(
        &deployment,
        session.controller_id(),
        session.fencing_token(),
        at(20),
    );
    let outcome = match &request.mutation {
        RuntimeConvergenceMutationV1::AcceptPreflight(attestation) => deployment
            .accept_preflight(&guard, attestation.clone())
            .unwrap(),
        RuntimeConvergenceMutationV1::RequestDrain => deployment.request_drain(&guard).unwrap(),
        RuntimeConvergenceMutationV1::AcceptDrain(attestation) => deployment
            .accept_drain(&guard, attestation.clone())
            .unwrap(),
        RuntimeConvergenceMutationV1::BeginActivation => {
            deployment.begin_activation(&guard).unwrap()
        }
        RuntimeConvergenceMutationV1::AcceptActivation(attestation) => deployment
            .accept_activation(&guard, attestation.clone())
            .unwrap(),
        RuntimeConvergenceMutationV1::BeginPanelReconciliation => {
            deployment.begin_panel_reconciliation(&guard).unwrap()
        }
        RuntimeConvergenceMutationV1::AcceptPanelCertificate(certificate) => deployment
            .accept_panel_certificate(&guard, certificate.clone())
            .unwrap(),
        _ => panic!("unsupported test mutation"),
    };
    crate::RuntimeMutationReceiptV1 {
        action_id: request.action_id,
        outcome,
        snapshot: deployment.snapshot(),
        convergence_attempt: session.convergence_attempt(),
    }
}

fn apply(session: &mut RuntimeConvergenceSessionV1, mutation: RuntimeConvergenceMutationV1) {
    let request = session.begin_mutation(mutation).unwrap();
    let receipt = mutation_receipt(session, &request);
    session.apply_mutation(receipt).unwrap();
}

fn awaiting_session() -> RuntimeConvergenceSessionV1 {
    let mut session = RuntimeConvergenceSessionV1::from_claim(claimed_execution()).unwrap();
    apply(
        &mut session,
        RuntimeConvergenceMutationV1::AcceptPreflight(PreflightAttestationV1 {
            target: target(),
            runtime_generation: RuntimeGeneration::new(4).unwrap(),
            observed_runtime: None,
            checked_at: at(11),
        }),
    );
    apply(&mut session, RuntimeConvergenceMutationV1::RequestDrain);
    apply(
        &mut session,
        RuntimeConvergenceMutationV1::AcceptDrain(DrainAttestationV1 {
            previous_runtime: None,
            target_runtime_generation: RuntimeGeneration::new(4).unwrap(),
            drained_at: at(12),
        }),
    );
    apply(&mut session, RuntimeConvergenceMutationV1::BeginActivation);
    apply(
        &mut session,
        RuntimeConvergenceMutationV1::AcceptActivation(ActivationAttestationV1 {
            activation_request_id: deployment_identity().activation_request_id,
            target: target(),
            runtime_generation: RuntimeGeneration::new(4).unwrap(),
            kind: ActivationOutcomeKindV1::Activated,
            activated_at: at(13),
        }),
    );
    apply(
        &mut session,
        RuntimeConvergenceMutationV1::BeginPanelReconciliation,
    );
    apply(
        &mut session,
        RuntimeConvergenceMutationV1::AcceptPanelCertificate(PanelCertificateV1 {
            certificate_id: PanelCertificateId::parse("panel:1").unwrap(),
            report_digest: PanelReportDigestV1::parse("c".repeat(64)).unwrap(),
            target: target(),
            runtime_generation: RuntimeGeneration::new(4).unwrap(),
            process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
            declared_count: 0,
            installed_count: 0,
            unchanged_count: 0,
            skipped_transient_count: 0,
            skipped_unresolved_channel_count: 0,
            failed_count: 0,
            ambiguous_outcome_count: 0,
            stale_message_cleanup_pending_count: 0,
            orphan_message_cleanup_pending_count: 0,
            reposted_old_message_cleanup_pending_count: 0,
            reconciled_at: at(14),
        }),
    );
    assert!(matches!(
        session.snapshot().phase,
        RuntimeDeploymentPhaseV1::AwaitingGatewayReady
    ));
    session
}

fn input(operation_id: &str) -> RuntimeCertificationReservationInputV2 {
    let process_identity = RuntimeProcessIdentityV1 {
        target: target(),
        runtime_generation: RuntimeGeneration::new(4).unwrap(),
        process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
    };
    RuntimeCertificationReservationInputV2 {
        operation_id: RuntimeCertificationOperationIdV2::parse(operation_id).unwrap(),
        binding_pin: RuntimeBindingPinV1 {
            tenant_id: TenantId::parse("tenant:1").unwrap(),
            installation_id: InstallationId::parse("installation:1").unwrap(),
            installation_authority_revision: non_zero(6),
            binding_revision: BindingRevision::new(3).unwrap(),
            binding_fingerprint: ResourceBindingFingerprint::parse(&"a".repeat(64)).unwrap(),
        },
        gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1 {
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
            process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
            lease_epoch: non_zero(5),
            expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
        },
        observed_owner_revision: non_zero(7),
        runtime_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
        panel: RuntimePanelEvidenceV2 {
            certificate_id: PanelCertificateId::parse("panel:1").unwrap(),
            report_digest: PanelReportDigestV1::parse("c".repeat(64)).unwrap(),
            process_identity,
            controller_fencing_token: FencingToken::new(3).unwrap(),
        },
        serving_lease_for: Duration::from_secs(30),
    }
}

fn route_admission() -> RuntimeRouteAdmissionAttestationV2 {
    let input = input("00112233445566778899aabbccddeeff");
    RuntimeRouteAdmissionAttestationV2 {
        barrier_id: RuntimeBarrierIdV1::parse("ffeeddccbbaa99887766554433221100").unwrap(),
        pause: RuntimeBarrierPauseWitnessV2 {
            coordinator_generation: non_zero(8),
            connection_epoch: non_zero(9),
            paused_admission_revision: non_zero(10),
            pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(12)),
        },
        gateway: RuntimeGatewayReadyAttestationV2 {
            process_instance_id: input.panel.process_identity.process_instance_id.clone(),
            connection_epoch: non_zero(9),
            kind: RuntimeGatewayReadyKindV2::Resumed,
            admission_revision: non_zero(10),
            connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(11)),
            resume_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(13)),
        },
        gateway_owner_lease_id: input.gateway_owner_lease_id,
        attested_owner_revision: input.observed_owner_revision,
        route: RuntimeServingRouteAttestationV2 {
            identity: input.panel.process_identity,
            controller_fencing_token: FencingToken::new(3).unwrap(),
            route_incarnation: non_zero(14),
            activation_sequence: non_zero(15),
        },
    }
}

fn committed_completion(
    awaiting: &RuntimeDeploymentSnapshotV1,
    reservation: &crate::RuntimeReservedCertificationIntentV2,
) -> (
    crate::RuntimeCanonicalLiveAttestationV2,
    RuntimeCertificationReceiptV2,
) {
    let request = RuntimeCertificationRequestV2 {
        intent: reservation.canonical_intent().intent().clone(),
        intent_fingerprint: reservation.intent_fingerprint().clone(),
        must_commit_before: at(90),
        route_admission: route_admission(),
    };
    let canonical = reservation
        .canonical_intent()
        .bind_live_record(RuntimeLiveAttestationRecordV2::from_request(request).unwrap())
        .unwrap();
    let intent = canonical.request().intent.clone();
    let mut deployment = RuntimeDeployment::restore(awaiting.clone()).unwrap();
    let certified_at = at(21);
    let outcome = deployment
        .certify_live(
            &CommandGuardV1 {
                expected_revision: intent.guard.expected_revision,
                controller_id: intent.guard.controller_id.clone(),
                fencing_token: intent.guard.fencing_token,
                runtime_generation: intent.guard.runtime_generation,
                now: certified_at,
            },
            GatewayReadyAttestationV1 {
                target: intent.target.clone(),
                runtime_generation: intent.guard.runtime_generation,
                process_instance_id: intent.process_identity.process_instance_id.clone(),
                kind: GatewayReadyKindV1::DiscordResumed,
                ready_at: at(20),
            },
            certified_at,
        )
        .unwrap();
    let receipt = RuntimeCertificationReceiptV2 {
        action_id: intent.action_id,
        outcome,
        snapshot: deployment.snapshot(),
        convergence_attempt: intent.guard.convergence_attempt,
        operation_id: intent.operation_id.clone(),
        intent_fingerprint: canonical.intent_fingerprint().clone(),
        request_digest: canonical.request_digest().clone(),
        attestation_digest: canonical.live_attestation_digest().clone(),
        route_admission: canonical.request().route_admission.clone(),
        serving: RuntimeServingReceiptV2 {
            identity: RuntimeServingIdentityV2 {
                scope: RuntimeDeploymentScopeV1::from_identity(deployment.identity()),
                operation_id: intent.operation_id,
                attestation_digest: canonical.live_attestation_digest().clone(),
                process_identity: intent.process_identity,
                lease_epoch: non_zero(16),
                revision: non_zero(17),
            },
            acquired_at: certified_at,
            last_heartbeat_at: certified_at,
            expires_at: at(51),
            connected: true,
            serving: true,
        },
        certified_at,
    };
    (canonical, receipt)
}

fn reserve(
    session: &mut RuntimeConvergenceSessionV1,
    operation_id: &str,
) -> crate::RuntimeReservedCertificationIntentV2 {
    session
        .begin_certification_reservation_v2(input(operation_id))
        .unwrap()
}

#[test]
fn begin_mints_and_binds_the_exact_current_awaiting_action() {
    let mut session = awaiting_session();
    let before = session.current_execution_receipt().unwrap();
    let reservation = reserve(&mut session, "00112233445566778899aabbccddeeff");
    let intent = reservation.canonical_intent().intent();

    assert_eq!(intent.action_id.get(), 8);
    assert_eq!(session.in_flight_action(), Some(intent.action_id));
    assert_eq!(reservation.operation_scope().scope(), &intent.guard.scope);
    assert_eq!(
        reservation.operation_scope().deployment_revision(),
        before.snapshot.revision
    );
    assert_eq!(
        reservation.operation_scope().convergence_attempt(),
        before.convergence_attempt
    );
    assert_eq!(intent.guard.expected_revision, before.snapshot.revision);
    assert_eq!(intent.guard.controller_id, before.controller_id);
    assert_eq!(intent.guard.fencing_token, before.fencing_token);
    assert_eq!(
        intent.guard.runtime_generation,
        before.snapshot.runtime_generation
    );
    assert_eq!(intent.target, before.snapshot.target);
    assert_eq!(
        intent.process_identity,
        reservation
            .canonical_intent()
            .intent()
            .panel
            .process_identity
    );
    assert!(!reservation.certification_intent_bytes().is_empty());
    assert_eq!(
        reservation.intent_fingerprint(),
        reservation.canonical_intent().intent_fingerprint()
    );
}

#[test]
fn begin_requires_awaiting_phase_and_an_empty_action_slot() {
    let mut requested = RuntimeConvergenceSessionV1::from_claim(claimed_execution()).unwrap();
    assert_eq!(
        requested.begin_certification_reservation_v2(input("00112233445566778899aabbccddeeff")),
        Err(RuntimeCertificationSessionErrorV2::Session(
            RuntimeConvergenceSessionError::InvalidMutationForPhase
        ))
    );
    assert!(requested.in_flight_action().is_none());

    let mut awaiting = awaiting_session();
    let first = reserve(&mut awaiting, "00112233445566778899aabbccddeeff");
    assert_eq!(
        awaiting.begin_certification_reservation_v2(input("ffeeddccbbaa99887766554433221100")),
        Err(RuntimeCertificationSessionErrorV2::Session(
            RuntimeConvergenceSessionError::ActionInFlight
        ))
    );
    assert_eq!(
        awaiting.in_flight_action(),
        Some(first.canonical_intent().intent().action_id)
    );
}

#[test]
fn begin_rejects_scope_process_panel_and_duration_tampering_without_an_action() {
    for tamper in 0..5 {
        let mut session = awaiting_session();
        let mut proposed = input("00112233445566778899aabbccddeeff");
        match tamper {
            0 => {
                proposed.binding_pin.installation_id =
                    InstallationId::parse("installation:other").unwrap()
            }
            1 => {
                proposed.panel.process_identity.process_instance_id =
                    ProcessInstanceId::parse("process:other").unwrap()
            }
            2 => proposed.panel.report_digest = PanelReportDigestV1::parse("d".repeat(64)).unwrap(),
            3 => proposed.panel.controller_fencing_token = FencingToken::new(4).unwrap(),
            _ => proposed.serving_lease_for = Duration::ZERO,
        }
        assert!(session
            .begin_certification_reservation_v2(proposed)
            .is_err());
        assert!(session.in_flight_action().is_none());
    }
}

#[test]
fn apply_accepts_only_the_exact_reserved_operation_and_mints_authority() {
    let mut session = awaiting_session();
    let expected = reserve(&mut session, "00112233445566778899aabbccddeeff");
    let expected_bytes = expected.certification_intent_bytes().to_vec();
    let expected_fingerprint = expected.intent_fingerprint().clone();
    let authority = session
        .apply_certification_reservation_v2(
            RuntimeCertificationIntentReservationOutcomeV2::Reserved(expected.clone()),
        )
        .unwrap();

    assert_authority(
        &authority,
        "00112233445566778899aabbccddeeff",
        &expected_bytes,
        &expected_fingerprint,
    );
    assert_eq!(
        format!("{authority:?}"),
        "RuntimeCertificationReservationAuthorityV2(<redacted>)"
    );
    assert_eq!(
        session.in_flight_action(),
        Some(expected.canonical_intent().intent().action_id)
    );
    assert_eq!(
        session.begin_renewal(Duration::from_secs(30)).unwrap_err(),
        RuntimeConvergenceSessionError::ActionInFlight
    );
    assert_eq!(
        session
            .apply_certification_reservation_v2(
                RuntimeCertificationIntentReservationOutcomeV2::Reserved(expected.clone())
            )
            .unwrap_err(),
        RuntimeCertificationSessionErrorV2::Session(RuntimeConvergenceSessionError::ActionMismatch)
    );
    assert_eq!(authority.into_reserved_intent(), expected);
}

#[test]
fn observed_persisted_reservation_restores_its_original_action_and_authority() {
    let mut original = awaiting_session();
    let execution = original.current_execution_receipt().unwrap();
    let lookup =
        RuntimeCertificationReservationScopeLookupV2::from_awaiting_execution(&execution).unwrap();
    let reservation = reserve(&mut original, "00112233445566778899aabbccddeeff");
    assert!(reservation.canonical_intent().intent().action_id.get() > 1);
    let observation = RuntimeCertificationReservationScopeObservationV2::reserved(
        lookup,
        execution.snapshot.clone(),
        reservation.clone(),
        at(20),
    )
    .unwrap();
    let mut restored = RuntimeConvergenceSessionV1::from_claim(execution).unwrap();

    let authority = restored
        .apply_observed_certification_reservation_v2(observation)
        .unwrap();

    assert_authority(
        &authority,
        "00112233445566778899aabbccddeeff",
        reservation.certification_intent_bytes(),
        reservation.intent_fingerprint(),
    );
    assert_eq!(
        restored.in_flight_action(),
        Some(reservation.canonical_intent().intent().action_id)
    );
    assert_eq!(
        restored.begin_renewal(Duration::from_secs(30)).unwrap_err(),
        RuntimeConvergenceSessionError::ActionInFlight
    );
}

#[test]
fn observed_reservation_rejects_absence_expiry_and_divergence_without_authority() {
    let mut source = awaiting_session();
    let execution = source.current_execution_receipt().unwrap();
    let lookup =
        RuntimeCertificationReservationScopeLookupV2::from_awaiting_execution(&execution).unwrap();
    let reservation = reserve(&mut source, "00112233445566778899aabbccddeeff");

    let absent = RuntimeCertificationReservationScopeObservationV2::absent(
        lookup.clone(),
        execution.snapshot.clone(),
        at(20),
    )
    .unwrap();
    let mut absent_session = RuntimeConvergenceSessionV1::from_claim(execution.clone()).unwrap();
    assert_eq!(
        absent_session
            .apply_observed_certification_reservation_v2(absent)
            .unwrap_err(),
        RuntimeCertificationSessionErrorV2::Session(
            RuntimeConvergenceSessionError::ReceiptMismatch
        )
    );
    assert!(absent_session.in_flight_action().is_none());

    let expired = RuntimeCertificationReservationScopeObservationV2::reserved(
        lookup,
        execution.snapshot.clone(),
        reservation,
        at(101),
    )
    .unwrap();
    let mut expired_session = RuntimeConvergenceSessionV1::from_claim(execution.clone()).unwrap();
    assert_eq!(
        expired_session
            .apply_observed_certification_reservation_v2(expired)
            .unwrap_err(),
        RuntimeCertificationSessionErrorV2::Session(
            RuntimeConvergenceSessionError::ReceiptMismatch
        )
    );
    assert!(expired_session.in_flight_action().is_none());

    let divergence = RuntimeCertificationDivergenceV2::PersistenceCorrupt;
    let mut diverged_session = RuntimeConvergenceSessionV1::from_claim(execution).unwrap();
    assert_eq!(
        diverged_session
            .apply_observed_certification_reservation_v2(
                RuntimeCertificationReservationScopeObservationV2::diverged(divergence.clone())
            )
            .unwrap_err(),
        RuntimeCertificationSessionErrorV2::Diverged {
            divergence: Box::new(divergence)
        }
    );
    assert!(diverged_session.in_flight_action().is_none());
}

fn assert_authority(
    authority: &RuntimeCertificationReservationAuthorityV2,
    operation_id: &str,
    bytes: &[u8],
    fingerprint: &crate::RuntimeCertificationIntentFingerprintV2,
) {
    assert_eq!(authority.operation_id().as_str(), operation_id);
    assert_eq!(authority.certification_intent_bytes(), bytes);
    assert_eq!(authority.intent_fingerprint(), fingerprint);
    assert_eq!(
        authority.operation_scope(),
        authority.reserved_intent().operation_scope()
    );
    assert_eq!(
        authority.action_id(),
        authority
            .reserved_intent()
            .canonical_intent()
            .intent()
            .action_id
    );
}

#[test]
fn exact_committed_completion_is_the_only_path_out_of_finalizing() {
    let mut session = awaiting_session();
    let awaiting = session.snapshot().clone();
    let reservation = reserve(&mut session, "00112233445566778899aabbccddeeff");
    let action_id = reservation.canonical_intent().intent().action_id;
    session
        .apply_certification_reservation_v2(
            RuntimeCertificationIntentReservationOutcomeV2::Reserved(reservation.clone()),
        )
        .unwrap();
    let (canonical, receipt) = committed_completion(&awaiting, &reservation);
    let expected_serving = receipt.serving.clone();

    assert_eq!(
        session.apply_certification_v2(canonical, receipt).unwrap(),
        expected_serving
    );
    assert_eq!(
        session.state(),
        RuntimeConvergenceSessionStateV1::CertifiedLive
    );
    assert!(session.in_flight_action().is_none());
    assert!(matches!(
        session.snapshot().phase,
        RuntimeDeploymentPhaseV1::Live
    ));
    assert_eq!(
        session.abort_action(action_id),
        Err(RuntimeConvergenceSessionError::NoActionInFlight)
    );
}

#[test]
fn mismatched_completion_keeps_the_exact_action_frozen() {
    let mut session = awaiting_session();
    let awaiting = session.snapshot().clone();
    let reservation = reserve(&mut session, "00112233445566778899aabbccddeeff");
    let action_id = reservation.canonical_intent().intent().action_id;
    session
        .apply_certification_reservation_v2(
            RuntimeCertificationIntentReservationOutcomeV2::Reserved(reservation.clone()),
        )
        .unwrap();
    let (canonical, mut receipt) = committed_completion(&awaiting, &reservation);
    receipt.serving.expires_at = at(52);

    assert_eq!(
        session
            .apply_certification_v2(canonical, receipt)
            .unwrap_err(),
        RuntimeCertificationSessionErrorV2::Session(
            RuntimeConvergenceSessionError::ReceiptMismatch
        )
    );
    assert_eq!(session.in_flight_action(), Some(action_id));
    assert_eq!(session.state(), RuntimeConvergenceSessionStateV1::Active);
}

#[test]
fn exact_absent_observation_is_the_only_pre_persist_release() {
    let mut session = awaiting_session();
    let execution = session.current_execution_receipt().unwrap();
    let lookup =
        RuntimeCertificationReservationScopeLookupV2::from_awaiting_execution(&execution).unwrap();
    reserve(&mut session, "00112233445566778899aabbccddeeff");
    let observation = RuntimeCertificationReservationScopeObservationV2::absent(
        lookup,
        execution.snapshot,
        at(20),
    )
    .unwrap();

    session
        .apply_absent_certification_reservation_v2(observation)
        .unwrap();
    assert!(session.in_flight_action().is_none());
    assert!(session.current_execution_receipt().is_ok());
}

#[test]
fn apply_rejects_foreign_and_byte_distinct_receipts_without_releasing_action() {
    let mut stale_session = awaiting_session();
    let stale = reserve(&mut stale_session, "00112233445566778899aabbccddeeff");
    let mut session = awaiting_session();
    let current = reserve(&mut session, "ffeeddccbbaa99887766554433221100");

    assert_eq!(
        session
            .apply_certification_reservation_v2(
                RuntimeCertificationIntentReservationOutcomeV2::Reserved(stale)
            )
            .unwrap_err(),
        RuntimeCertificationSessionErrorV2::Session(
            RuntimeConvergenceSessionError::ReceiptMismatch
        )
    );
    assert_eq!(
        session.in_flight_action(),
        Some(current.canonical_intent().intent().action_id)
    );

    let mut exact_session = awaiting_session();
    let exact = reserve(&mut exact_session, "ffeeddccbbaa99887766554433221100");
    let mut competing_session = awaiting_session();
    let competing = reserve(&mut competing_session, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    assert_eq!(
        exact_session
            .apply_certification_reservation_v2(
                RuntimeCertificationIntentReservationOutcomeV2::Reserved(competing)
            )
            .unwrap_err(),
        RuntimeCertificationSessionErrorV2::Session(
            RuntimeConvergenceSessionError::ReceiptMismatch
        )
    );
    assert_eq!(
        exact_session.in_flight_action(),
        Some(exact.canonical_intent().intent().action_id)
    );
}

#[test]
fn generic_abort_cannot_release_reserved_or_finalizing_certification() {
    let mut session = awaiting_session();
    let reservation = reserve(&mut session, "00112233445566778899aabbccddeeff");
    let action_id = reservation.canonical_intent().intent().action_id;
    assert_eq!(
        session.abort_action(action_id),
        Err(RuntimeConvergenceSessionError::ActionMismatch)
    );
    session
        .apply_certification_reservation_v2(
            RuntimeCertificationIntentReservationOutcomeV2::Reserved(reservation),
        )
        .unwrap();
    assert_eq!(
        session.abort_action(action_id),
        Err(RuntimeConvergenceSessionError::ActionMismatch)
    );
    assert_eq!(session.in_flight_action(), Some(action_id));
}

#[test]
fn divergence_keeps_the_current_action_frozen_without_minting_authority() {
    let mut session = awaiting_session();
    let reservation = reserve(&mut session, "00112233445566778899aabbccddeeff");
    let action_id = reservation.canonical_intent().intent().action_id;
    let divergence = RuntimeCertificationDivergenceV2::ReservationMismatch;

    assert_eq!(
        session
            .apply_certification_reservation_v2(
                RuntimeCertificationIntentReservationOutcomeV2::Diverged(divergence.clone())
            )
            .unwrap_err(),
        RuntimeCertificationSessionErrorV2::Diverged {
            divergence: Box::new(divergence)
        }
    );
    assert_eq!(session.in_flight_action(), Some(action_id));
}
