use std::num::NonZeroU64;

use automation_runtime_controller::{
    GatewayShardIdV1, RuntimeBuildRevisionV1, RuntimeCanonicalDrainIntentStateV2,
    RuntimeCanonicalProductDrainV2, RuntimeDrainClaimProgressV2, RuntimeDrainClaimSealWitnessV2,
    RuntimeDrainClaimV2, RuntimeDrainIntentDigestV2, RuntimeDrainIntentV2,
    RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeGatewayOwnerLeaseReceiptV1, RuntimeGatewayReadyKindV2,
    RuntimePersistedProductDrainRootV2, RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2,
    RuntimeProductMutationDigestV2, RuntimeRecoveryIdV2, RuntimeStartupRecoveryStateV2,
    RuntimeStartupServingStateV2,
};
use automation_runtime_convergence::{
    ControllerId, FencingToken, ProcessInstanceId, RuntimeDeploymentTargetV1,
};
use automation_runtime_registry::{ServingSlotKeyV1, SlotAdmissionStateV2, SlotSealKeyV2};
use automation_runtime_worker::{
    accept_runtime_registry_recovery_empty_observation_v2, RuntimeAcceptedPendingDrainSelectionV3,
    RuntimeAcceptedStartupRecoveryOutcomeV2, RuntimeCapabilityReadinessKindV2,
    RuntimeCapabilityReadinessReceiptV2, RuntimeCapabilityReadinessSetV2,
    RuntimeClosedDrainRecoveryPermitV2, RuntimeClosedRecoveryInputV2,
    RuntimeClosedRecoveryRegistryEvidenceV2, RuntimeGatewayClosedLifecycleV2,
    RuntimePausedGatewayObservationV2, RuntimePausedGatewaySequenceV2,
    RuntimePendingDrainPreviousOwnerClaimedCandidateInputV3,
    RuntimePendingDrainPreviousOwnerClaimedCandidateV3,
    RuntimePendingDrainRegistrySealWitnessInputV2, RuntimePendingDrainRegistrySealWitnessV2,
    RuntimePendingDrainSelectionOutcomeV3, RuntimePendingDrainSelectionReceiptV3,
    RuntimePendingDrainStateDigestV2, RuntimeRegistryGlobalObservationSequenceV2,
    RuntimeRegistryRecoveryObservationInputV2, RuntimeSelectedPendingDrainSuccessionV3,
    RuntimeStartupRecoveryClassV2, RuntimeStartupRecoveryContinuationV2,
    RuntimeStartupRecoveryExecutionReceiptOutcomeV2,
    RuntimeStartupRecoveryExecutionTerminalDigestV2,
};
use chrono::{DateTime, Utc};

use super::{
    compose_runtime_registry_bootstrap_v1, successor_persistence_non_zero_u64_v2,
    RuntimeRegistryBootstrapV1, RuntimeRegistryPendingDrainSuccessionSealBindingV3,
    RuntimeRegistryRecoveryObservationErrorV1,
};
use crate::GatewayResourceConfigV1;

fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn at(second: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(second, 0).unwrap()
}

fn readiness(checked_at: i64) -> RuntimeCapabilityReadinessSetV2 {
    let receipt = |kind, role, offset| {
        RuntimeCapabilityReadinessReceiptV2::new(
            kind,
            "01234567-89ab-cdef-8123-456789abcdef",
            "starring",
            role,
            at(checked_at + offset),
        )
        .unwrap()
    };
    RuntimeCapabilityReadinessSetV2::new(
        receipt(RuntimeCapabilityReadinessKindV2::Convergence, "role_a", 0),
        receipt(RuntimeCapabilityReadinessKindV2::ExactTarget, "role_b", 1),
        receipt(RuntimeCapabilityReadinessKindV2::Panel, "role_c", 2),
        receipt(RuntimeCapabilityReadinessKindV2::Serving, "role_d", 3),
        receipt(RuntimeCapabilityReadinessKindV2::Interaction, "role_e", 4),
    )
    .unwrap()
}

fn begin_execution(
    process: ProcessInstanceId,
    registry_sequence: RuntimeRegistryGlobalObservationSequenceV2,
    retained_slot_count: u64,
    retained_empty_tombstone_count: u64,
) -> (
    RuntimeGatewayClosedLifecycleV2,
    RuntimeClosedDrainRecoveryPermitV2,
    automation_runtime_worker::RuntimeAuthorizedStartupRecoveryExecutionV2,
) {
    let mut lifecycle = RuntimeGatewayClosedLifecycleV2::starting();
    let generation = lifecycle.snapshot().generation();
    let paused = RuntimePausedGatewayObservationV2::new(
        generation,
        process.clone(),
        non_zero(2),
        RuntimeGatewayReadyKindV2::Ready,
        non_zero(3),
        RuntimePausedGatewaySequenceV2::new(
            RuntimeGatewayAdmissionSequenceV2::new(non_zero(5)),
            RuntimeGatewayAdmissionSequenceV2::new(non_zero(4)),
            None,
        )
        .unwrap(),
    );
    let registry = accept_runtime_registry_recovery_empty_observation_v2(
        process.clone(),
        RuntimeRegistryRecoveryObservationInputV2 {
            observation_sequence: registry_sequence,
            retained_slot_count,
            retained_empty_tombstone_count,
            staged_route_count: 0,
            serving_route_count: 0,
            draining_route_count: 0,
            sealed_slot_count: 0,
            active_interaction_count: 0,
            failed_closed_slot_count: 0,
            registry_failed_closed: false,
        },
    )
    .unwrap();
    let (_, mut permit) = lifecycle
        .begin_recovery(
            generation,
            RuntimeClosedRecoveryInputV2::new(
                RuntimeRecoveryIdV2::parse("0123456789abcdef0123456789abcdef").unwrap(),
                RuntimeGatewayOwnerLeaseReceiptV1 {
                    lease_id: RuntimeGatewayOwnerLeaseIdV1 {
                        gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
                        process_instance_id: process,
                        lease_epoch: non_zero(7),
                        expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
                    },
                    owner_revision: non_zero(8),
                    database_now: at(100),
                    expires_at: at(200),
                },
                readiness(100),
                paused,
                RuntimeClosedRecoveryRegistryEvidenceV2::Empty(registry),
            ),
        )
        .unwrap();
    let iteration = lifecycle
        .refresh_recovery_readiness(&mut permit, readiness(200))
        .unwrap();
    let observation = lifecycle
        .begin_startup_recovery_observation(&mut permit, iteration)
        .unwrap();
    let receipt = {
        let request = observation.request();
        automation_runtime_controller::RuntimeStartupRecoveryObservationReceiptV2 {
            correlation: request.correlation.clone(),
            owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1 {
                lease_id: request.gateway_owner_lease_id.clone(),
                owner_revision: request.expected_owner_revision,
                database_now: at(101),
                expires_at: request.expected_owner_expires_at,
            },
            state: RuntimeStartupRecoveryStateV2 {
                serving: RuntimeStartupServingStateV2::Empty,
                recoverable_awaiting_certification_count: 0,
                suspended_local_effect_count: 0,
                pending_runtime_drain_intent_count: 1,
                acknowledged_product_handoff_count: 0,
            },
        }
    };
    let completed = observation.complete(receipt);
    let RuntimeAcceptedStartupRecoveryOutcomeV2::Continue(continuation) = lifecycle
        .complete_startup_recovery_observation(&mut permit, completed)
        .unwrap()
    else {
        panic!("expected pending drain continuation")
    };
    assert_eq!(
        continuation,
        RuntimeStartupRecoveryContinuationV2::Recover(
            RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent
        )
    );
    let authorization = lifecycle
        .begin_startup_recovery_execution(&mut permit, continuation)
        .unwrap();
    (lifecycle, permit, authorization)
}

fn canonical_product() -> RuntimeCanonicalProductDrainV2 {
    let product_bytes = format!(
        concat!(
            "{{\"format_version\":2,\"operation_id\":\"{}\",",
            "\"scope\":{{\"tenant_id\":\"tenant:1\",\"installation_id\":",
            "\"installation:1\",\"deployment_id\":\"deployment:1\"}},",
            "\"expected_revision\":11,\"slot\":{{\"guild_id\":\"9223372036854775808\",",
            "\"ruleset_key\":\"studyroom\"}},\"expected_target\":{{\"guild_id\":",
            "\"9223372036854775808\",\"ruleset_key\":\"studyroom\",\"version\":1,",
            "\"content_hash\":\"{}\",\"binding_revision\":3,",
            "\"binding_fingerprint\":\"{}\"}},\"mutation_kind\":",
            "\"authority_change\",\"product_semantic_request_digest\":\"{}\"}}"
        ),
        "00112233445566778899aabbccddeeff",
        "b".repeat(64),
        "a".repeat(64),
        "c".repeat(64),
    )
    .into_bytes();
    let product_digest = RuntimeProductMutationDigestV2::parse(
        "e35c1116d5bee2949184cceff540ee2575ac389461270f96f525ccd9c193166d",
    )
    .unwrap();
    let drain_bytes = format!(
        concat!(
            "{{\"format_version\":2,\"key\":{{\"intent_id\":\"{}\",",
            "\"product_operation_id\":\"{}\",",
            "\"product_mutation_digest\":\"{}\",",
            "\"scope\":{{\"tenant_id\":\"tenant:1\",\"installation_id\":",
            "\"installation:1\",\"deployment_id\":\"deployment:1\"}},",
            "\"expected_revision\":11,\"slot\":{{\"guild_id\":\"9223372036854775808\",",
            "\"ruleset_key\":\"studyroom\"}},\"expected_target\":{{\"guild_id\":",
            "\"9223372036854775808\",\"ruleset_key\":\"studyroom\",\"version\":1,",
            "\"content_hash\":\"{}\",\"binding_revision\":3,",
            "\"binding_fingerprint\":\"{}\"}},\"mutation_kind\":",
            "\"authority_change\"}}}}"
        ),
        "ffeeddccbbaa99887766554433221100",
        "00112233445566778899aabbccddeeff",
        product_digest.as_str(),
        "b".repeat(64),
        "a".repeat(64),
    )
    .into_bytes();
    let drain_digest = RuntimeDrainIntentDigestV2::parse(
        "edf1671e7c1395205cae7962d6cf043610c51b5ed49b2d4528d72351bed287fc",
    )
    .unwrap();
    RuntimeCanonicalProductDrainV2::from_persisted(
        &product_bytes,
        &product_digest,
        &drain_bytes,
        &drain_digest,
    )
    .unwrap()
}

fn persisted_root() -> RuntimePersistedProductDrainRootV2 {
    let canonical = canonical_product();
    let product = canonical.product_preimage();
    let drain = canonical.drain_preimage();
    RuntimePersistedProductDrainRootV2::from_persisted(
        product.scope.clone(),
        product.expected_revision,
        &product.operation_id,
        drain.key.scope.clone(),
        drain.key.slot.clone(),
        drain.key.expected_revision,
        &drain.key.intent_id,
        &drain.key.expected_target,
        canonical.product_mutation_request_bytes(),
        canonical.product_mutation_digest(),
        canonical.drain_intent_request_bytes(),
        canonical.drain_intent_digest(),
    )
    .unwrap()
}

pub(crate) fn candidate(
    source_revision: u64,
) -> RuntimePendingDrainPreviousOwnerClaimedCandidateV3 {
    candidate_with_claim_expiry(source_revision, at(120))
}

pub(crate) fn candidate_with_claim_expiry(
    source_revision: u64,
    claim_expires_at: DateTime<Utc>,
) -> RuntimePendingDrainPreviousOwnerClaimedCandidateV3 {
    let key = canonical_product().drain_preimage().key.clone();
    let root = persisted_root();
    let process = ProcessInstanceId::parse("process:old").unwrap();
    let seal =
        RuntimeDrainClaimSealWitnessV2::new(&key, process.clone(), non_zero(2), None, non_zero(3))
            .unwrap();
    let claim = RuntimeDrainClaimV2::new(
        &key,
        RuntimeGatewayOwnerLeaseIdV1 {
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
            process_instance_id: process.clone(),
            lease_epoch: non_zero(6),
            expected_build_revision: RuntimeBuildRevisionV1::parse("build:old").unwrap(),
        },
        non_zero(4),
        process,
        ControllerId::parse("recovery:old").unwrap(),
        FencingToken::new(9).unwrap(),
        non_zero(5),
        non_zero(8),
        claim_expires_at,
        RuntimeDrainClaimProgressV2::claimed(seal),
    )
    .unwrap();
    let intent =
        RuntimeDrainIntentV2::pending_from_persisted(&root, non_zero(source_revision), Some(claim))
            .unwrap();
    let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(intent).unwrap();
    let source = RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2::from_persisted(
        &root,
        non_zero(source_revision),
        canonical.persisted_state().unwrap(),
        canonical.state_bytes(),
    )
    .unwrap();
    RuntimePendingDrainPreviousOwnerClaimedCandidateV3::new(
        RuntimePendingDrainPreviousOwnerClaimedCandidateInputV3 {
            source,
            source_state_digest: RuntimePendingDrainStateDigestV2::new([1; 32]).unwrap(),
            predecessor_claim_terminal_digest:
                RuntimeStartupRecoveryExecutionTerminalDigestV2::new([2; 32]).unwrap(),
            product_mutation_request_sha256: [3; 32],
            drain_intent_request_sha256: [4; 32],
        },
    )
    .unwrap()
}

fn owner_receipt(
    request: &automation_runtime_worker::RuntimeStartupRecoveryExecutionRequestV2,
    database_now: DateTime<Utc>,
) -> RuntimeGatewayOwnerLeaseReceiptV1 {
    RuntimeGatewayOwnerLeaseReceiptV1 {
        lease_id: request.gateway_owner_lease_id().clone(),
        owner_revision: request.expected_owner_revision(),
        database_now,
        expires_at: request.expected_owner_expires_at(),
    }
}

fn select_succession(
    authorization: automation_runtime_worker::RuntimeAuthorizedStartupRecoveryExecutionV2,
    candidate: &RuntimePendingDrainPreviousOwnerClaimedCandidateV3,
) -> RuntimeSelectedPendingDrainSuccessionV3 {
    let selection = authorization.into_pending_drain_selection_v3().unwrap();
    let receipt = RuntimePendingDrainSelectionReceiptV3::new(
        selection.request().correlation().clone(),
        owner_receipt(selection.request(), at(120)),
        RuntimePendingDrainSelectionOutcomeV3::ExpiredPreviousOwner(candidate.clone()),
    );
    let RuntimeAcceptedPendingDrainSelectionV3::ExpiredPreviousOwner(selected) =
        selection.accept_selection(receipt).unwrap()
    else {
        panic!("expected expired predecessor")
    };
    *selected
}

struct SealedFixture {
    bootstrap: RuntimeRegistryBootstrapV1,
    lifecycle: RuntimeGatewayClosedLifecycleV2,
    permit: RuntimeClosedDrainRecoveryPermitV2,
    candidate: RuntimePendingDrainPreviousOwnerClaimedCandidateV3,
    selected: RuntimeSelectedPendingDrainSuccessionV3,
    sealed: RuntimeRegistryPendingDrainSuccessionSealBindingV3,
    witness: RuntimePendingDrainRegistrySealWitnessV2,
    source_sequence: u64,
    source_retained_slot_count: u64,
}

fn seal_fixture(tombstone: bool, source_revision: u64) -> SealedFixture {
    let process = ProcessInstanceId::parse("process:current").unwrap();
    let bootstrap =
        compose_runtime_registry_bootstrap_v1(process.clone(), GatewayResourceConfigV1::default())
            .unwrap();
    let candidate = candidate(source_revision);
    let key = ServingSlotKeyV1::new(
        candidate.slot().guild_id,
        candidate.slot().ruleset_key.clone(),
    );
    if tombstone {
        let expected = bootstrap.registry.atomic_observation_v2(&key).unwrap();
        let (seal, _) = bootstrap
            .registry
            .seal_drain_claim_v2(
                &key,
                SlotSealKeyV2::try_from([9_u8; 16].as_slice()).unwrap(),
                expected.as_ref(),
            )
            .unwrap();
        bootstrap.registry.unseal_drain_claim_v2(seal).unwrap();
    }
    let source = bootstrap.observe_recovery_empty_projection_v2().unwrap();
    let source_sequence = source.observation_sequence().get();
    let source_retained_slot_count = source.retained_slot_count();
    let binding = bootstrap
        .recovery_observation_guard_unordered_v2()
        .unwrap()
        .into_empty_binding_v2()
        .unwrap();
    let (lifecycle, permit, authorization) = begin_execution(
        process,
        source.observation_sequence(),
        source.retained_slot_count(),
        source.retained_empty_tombstone_count(),
    );
    let selected = select_succession(authorization, &candidate);
    let (sealed, witness) = binding
        .into_pending_drain_succession_seal_binding_v3(&candidate)
        .unwrap();
    SealedFixture {
        bootstrap,
        lifecycle,
        permit,
        candidate,
        selected,
        sealed,
        witness,
        source_sequence,
        source_retained_slot_count,
    }
}

fn durable_succession(
    selected: RuntimeSelectedPendingDrainSuccessionV3,
    candidate: &RuntimePendingDrainPreviousOwnerClaimedCandidateV3,
    seal: RuntimePendingDrainRegistrySealWitnessV2,
) -> automation_runtime_worker::RuntimeDurablyAcknowledgedPendingDrainSuccessionV3 {
    let succession = selected.bind_registry_seal(seal.clone()).unwrap();
    let action_identity = succession.action_identity().clone();
    let receipt =
        automation_runtime_worker::RuntimePendingDrainSuccessionAcknowledgementReceiptV3::new(
            action_identity,
            candidate.clone(),
            seal,
            non_zero(candidate.source_intent_revision().get() + 1),
            RuntimePendingDrainStateDigestV2::new([3; 32]).unwrap(),
            RuntimeStartupRecoveryExecutionTerminalDigestV2::new([4; 32]).unwrap(),
            owner_receipt(succession.request(), at(121)),
        );
    succession.complete(receipt).unwrap()
}

#[test]
fn exact_v3_durable_receipt_rolls_absent_s0_s1_s2_once() {
    let mut fixture = seal_fixture(false, 5);
    assert_eq!(
        fixture.bootstrap.observe_recovery_empty_projection_v2(),
        Err(RuntimeRegistryRecoveryObservationErrorV1::NotEmpty)
    );
    assert_eq!(
        fixture.witness.seal_key(),
        &[
            0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x99, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22,
            0x11, 0x00
        ]
    );
    assert_eq!(
        format!("{:?}", fixture.sealed),
        "RuntimeRegistryPendingDrainSuccessionSealBindingV3(<redacted>)"
    );
    let durable = durable_succession(fixture.selected, &fixture.candidate, fixture.witness);
    let (empty, unseal) = fixture
        .sealed
        .into_empty_binding_after_durable_succession_v3(&durable)
        .unwrap();
    let completed = durable.complete_registry_rollover(unseal).unwrap();
    let accepted = fixture
        .lifecycle
        .complete_startup_recovery_execution(&mut fixture.permit, completed)
        .unwrap();
    assert!(matches!(
        accepted.outcome(),
        RuntimeStartupRecoveryExecutionReceiptOutcomeV2::Progressed { .. }
    ));
    assert!(matches!(
        accepted.pending_drain_proof(),
        Some(automation_runtime_worker::RuntimePendingDrainExecutionProofV2::Succession(_))
    ));
    let successor = empty.revalidate_empty_projection_unordered_v2().unwrap();
    assert_eq!(
        successor.observation_sequence().get(),
        fixture.source_sequence + 2
    );
    assert_eq!(successor.retained_slot_count(), 1);
    assert_eq!(successor.retained_empty_tombstone_count(), 1);
}

#[test]
fn exact_v3_durable_receipt_preserves_tombstone_generation_and_counts() {
    let mut fixture = seal_fixture(true, 5);
    assert!(fixture.witness.pre_slot_observation().is_some());
    assert_eq!(
        fixture.witness.post_slot_admission_generation().get(),
        fixture
            .witness
            .pre_slot_observation()
            .unwrap()
            .admission_generation
            .get()
            + 1
    );
    let durable = durable_succession(fixture.selected, &fixture.candidate, fixture.witness);
    let (empty, unseal) = fixture
        .sealed
        .into_empty_binding_after_durable_succession_v3(&durable)
        .unwrap();
    let completed = durable.complete_registry_rollover(unseal).unwrap();
    fixture
        .lifecycle
        .complete_startup_recovery_execution(&mut fixture.permit, completed)
        .unwrap();
    let successor = empty.revalidate_empty_projection_unordered_v2().unwrap();
    assert_eq!(
        successor.observation_sequence().get(),
        fixture.source_sequence + 2
    );
    assert_eq!(
        successor.retained_slot_count(),
        fixture.source_retained_slot_count
    );
    assert_eq!(
        successor.retained_empty_tombstone_count(),
        fixture.source_retained_slot_count
    );
}

#[test]
fn v3_binding_drop_and_foreign_durable_receipt_leave_s1_closed() {
    let fixture = seal_fixture(false, 5);
    let foreign_seal = RuntimePendingDrainRegistrySealWitnessV2::new(
        RuntimePendingDrainRegistrySealWitnessInputV2 {
            process_instance_id: fixture.witness.process_instance_id().clone(),
            slot: fixture.witness.slot().clone(),
            pre_slot_observation: fixture.witness.pre_slot_observation(),
            seal_key: *fixture.witness.seal_key(),
            seal_generation: non_zero(fixture.witness.seal_generation().get() + 1),
            post_slot_admission_generation: fixture.witness.post_slot_admission_generation(),
            post_slot_observation_sequence: fixture.witness.post_slot_observation_sequence(),
            pre_registry_observation_sequence: fixture.witness.pre_registry_observation_sequence(),
            pre_registry_retained_slot_count: fixture.witness.pre_registry_retained_slot_count(),
            pre_registry_retained_empty_tombstone_count: fixture
                .witness
                .pre_registry_retained_empty_tombstone_count(),
            post_registry_observation: fixture.witness.post_registry_observation(),
        },
    )
    .unwrap();
    let durable = durable_succession(fixture.selected, &fixture.candidate, foreign_seal);
    assert_eq!(
        fixture
            .sealed
            .into_empty_binding_after_durable_succession_v3(&durable)
            .unwrap_err(),
        RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation
    );
    assert_eq!(
        fixture.bootstrap.observe_recovery_empty_projection_v2(),
        Err(RuntimeRegistryRecoveryObservationErrorV1::NotEmpty)
    );
    let key = ServingSlotKeyV1::new(
        fixture.candidate.slot().guild_id,
        fixture.candidate.slot().ruleset_key.clone(),
    );
    let observation = fixture
        .bootstrap
        .registry
        .atomic_observation_v2(&key)
        .unwrap()
        .unwrap();
    assert!(matches!(
        observation.admission_state,
        SlotAdmissionStateV2::DrainClaimSealed { .. }
    ));

    let dropped = seal_fixture(false, 5);
    drop(dropped.sealed);
    assert_eq!(
        dropped.bootstrap.observe_recovery_empty_projection_v2(),
        Err(RuntimeRegistryRecoveryObservationErrorV1::NotEmpty)
    );
}

#[test]
fn v3_s1_rejects_foreign_registry_advance_before_unseal() {
    let fixture = seal_fixture(false, 5);
    let durable = durable_succession(fixture.selected, &fixture.candidate, fixture.witness);
    let foreign_target: RuntimeDeploymentTargetV1 = serde_json::from_value(serde_json::json!({
        "guild_id": "77",
        "ruleset_key": "foreign",
        "version": 1,
        "content_hash": "5".repeat(64),
        "binding_revision": 1,
        "binding_fingerprint": "6".repeat(64)
    }))
    .unwrap();
    let foreign_key = ServingSlotKeyV1::from_target(&foreign_target);
    let (foreign_seal, _) = fixture
        .bootstrap
        .registry
        .seal_drain_claim_v2(
            &foreign_key,
            SlotSealKeyV2::try_from([8_u8; 16].as_slice()).unwrap(),
            None,
        )
        .unwrap();
    assert_eq!(
        fixture
            .sealed
            .into_empty_binding_after_durable_succession_v3(&durable)
            .unwrap_err(),
        RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation
    );
    drop(foreign_seal);
    let key = ServingSlotKeyV1::new(
        fixture.candidate.slot().guild_id,
        fixture.candidate.slot().ruleset_key.clone(),
    );
    let observation = fixture
        .bootstrap
        .registry
        .atomic_observation_v2(&key)
        .unwrap()
        .unwrap();
    assert!(matches!(
        observation.admission_state,
        SlotAdmissionStateV2::DrainClaimSealed { .. }
    ));
}

#[test]
fn v3_seal_keeps_its_one_revision_boundary_without_v2_conversion() {
    let fixture = seal_fixture(false, i64::MAX as u64 - 1);
    assert_eq!(
        fixture.candidate.source_intent_revision().get(),
        i64::MAX as u64 - 1
    );
    fixture.sealed.revalidate_sealed_v3().unwrap();
    assert_eq!(
        fixture.bootstrap.observe_recovery_empty_projection_v2(),
        Err(RuntimeRegistryRecoveryObservationErrorV1::NotEmpty)
    );
}

#[test]
fn local_rollover_persistence_headroom_is_checked_before_mutation() {
    assert_eq!(
        successor_persistence_non_zero_u64_v2(non_zero(i64::MAX as u64 - 1))
            .unwrap()
            .get(),
        i64::MAX as u64
    );
    assert_eq!(
        successor_persistence_non_zero_u64_v2(non_zero(i64::MAX as u64)),
        Err(RuntimeRegistryRecoveryObservationErrorV1::ObservationSequenceOutOfRange)
    );
}
