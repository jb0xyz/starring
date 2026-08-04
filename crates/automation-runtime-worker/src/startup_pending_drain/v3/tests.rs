use std::num::NonZeroU64;
use std::time::Duration;

use automation_runtime_controller::{
    GatewayShardIdV1, RuntimeBuildRevisionV1, RuntimeCanonicalDrainIntentStateV2,
    RuntimeCanonicalProductDrainV2, RuntimeCertificationOperationIdV2, RuntimeDeploymentScopeV1,
    RuntimeDrainClaimProgressV2, RuntimeDrainClaimSealWitnessV2, RuntimeDrainClaimV2,
    RuntimeDrainIntentDigestV2, RuntimeDrainIntentIdV2, RuntimeDrainIntentV2,
    RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeGatewayOwnerLeaseReceiptV1, RuntimeGatewayReadyKindV2, RuntimeLiveAttestationDigestV2,
    RuntimePersistedProductDrainRootV2, RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2,
    RuntimeProductMutationDigestV2, RuntimeRecoveryIdV2, RuntimeServingIdentityV2,
    RuntimeServingReceiptV2, RuntimeStartupRecoveryStateV2, RuntimeStartupServingStateV2,
};
use automation_runtime_convergence::{
    ControllerId, DeploymentId, FencingToken, InstallationId, ProcessInstanceId, RuntimeGeneration,
    RuntimeProcessIdentityV1, TenantId,
};
use chrono::{DateTime, TimeDelta, Utc};

use super::*;
use crate::RuntimePendingDrainServingSourceCorrelationV3;
use crate::{
    accept_runtime_registry_recovery_empty_observation_v2, RuntimeAcceptedStartupRecoveryOutcomeV2,
    RuntimeCapabilityReadinessKindV2, RuntimeCapabilityReadinessReceiptV2,
    RuntimeCapabilityReadinessSetV2, RuntimeClosedDrainRecoveryPermitV2,
    RuntimeClosedRecoveryInputV2, RuntimeClosedRecoveryRegistryEvidenceV2,
    RuntimeGatewayClosedLifecycleV2, RuntimePausedGatewayObservationV2,
    RuntimePausedGatewaySequenceV2, RuntimePendingDrainRegistrySealWitnessInputV2,
    RuntimeRegistryGlobalObservationSequenceV2, RuntimeRegistryRecoveryObservationInputV2,
    RuntimeStartupRecoveryContinuationV2,
};

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

fn begin_execution() -> (
    RuntimeGatewayClosedLifecycleV2,
    RuntimeClosedDrainRecoveryPermitV2,
    RuntimeAuthorizedStartupRecoveryExecutionV2,
) {
    let process = ProcessInstanceId::parse("process:current").unwrap();
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
            observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(non_zero(6)),
            retained_slot_count: 0,
            retained_empty_tombstone_count: 0,
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
    let mut state = RuntimeStartupRecoveryStateV2 {
        serving: RuntimeStartupServingStateV2::Empty,
        recoverable_awaiting_certification_count: 0,
        suspended_local_effect_count: 0,
        pending_runtime_drain_intent_count: 1,
        acknowledged_product_handoff_count: 0,
    };
    state.pending_runtime_drain_intent_count = 1;
    let observation_receipt = {
        let request = observation.request();
        automation_runtime_controller::RuntimeStartupRecoveryObservationReceiptV2 {
            correlation: request.correlation.clone(),
            owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1 {
                lease_id: request.gateway_owner_lease_id.clone(),
                owner_revision: request.expected_owner_revision,
                database_now: at(101),
                expires_at: request.expected_owner_expires_at,
            },
            state,
        }
    };
    let completed = observation.complete(observation_receipt);
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

fn canonical_key() -> automation_runtime_controller::RuntimeDrainIntentKeyV2 {
    canonical_product().drain_preimage().key.clone()
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

fn candidate(
    source_revision: u64,
    process: &str,
    shard: &str,
    lease_epoch: u64,
    claim_revision: u64,
    fence: u64,
    expires_at: DateTime<Utc>,
) -> Result<RuntimePendingDrainPreviousOwnerClaimedCandidateV3, RuntimePendingDrainCompoundErrorV2>
{
    let key = canonical_key();
    let process = ProcessInstanceId::parse(process).unwrap();
    let seal =
        RuntimeDrainClaimSealWitnessV2::new(&key, process.clone(), non_zero(2), None, non_zero(3))
            .unwrap();
    let claim = RuntimeDrainClaimV2::new(
        &key,
        RuntimeGatewayOwnerLeaseIdV1 {
            gateway_shard_id: GatewayShardIdV1::parse(shard).unwrap(),
            process_instance_id: process.clone(),
            lease_epoch: non_zero(lease_epoch),
            expected_build_revision: RuntimeBuildRevisionV1::parse("build:old").unwrap(),
        },
        non_zero(4),
        process,
        ControllerId::parse("recovery:old").unwrap(),
        FencingToken::new(fence).unwrap(),
        non_zero(5),
        non_zero(claim_revision),
        expires_at,
        RuntimeDrainClaimProgressV2::claimed(seal),
    )
    .unwrap();
    let root = persisted_root();
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
}

fn owner_receipt(
    request: &crate::RuntimeStartupRecoveryExecutionRequestV2,
    database_now: DateTime<Utc>,
) -> RuntimeGatewayOwnerLeaseReceiptV1 {
    RuntimeGatewayOwnerLeaseReceiptV1 {
        lease_id: request.gateway_owner_lease_id().clone(),
        owner_revision: request.expected_owner_revision(),
        database_now,
        expires_at: request.expected_owner_expires_at(),
    }
}

fn serving_receipt_v3(
    target: automation_runtime_convergence::RuntimeDeploymentTargetV1,
    revision: u64,
    heartbeat_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
) -> RuntimeServingReceiptV2 {
    RuntimeServingReceiptV2 {
        identity: RuntimeServingIdentityV2 {
            scope: RuntimeDeploymentScopeV1 {
                tenant_id: TenantId::parse("tenant:1").unwrap(),
                installation_id: InstallationId::parse("installation:1").unwrap(),
                deployment_id: DeploymentId::parse("deployment:1").unwrap(),
            },
            operation_id: RuntimeCertificationOperationIdV2::parse(
                "00112233445566778899aabbccddeeff",
            )
            .unwrap(),
            attestation_digest: RuntimeLiveAttestationDigestV2::parse("e".repeat(64)).unwrap(),
            process_identity: RuntimeProcessIdentityV1 {
                target,
                runtime_generation: RuntimeGeneration::new(4).unwrap(),
                process_instance_id: ProcessInstanceId::parse("runtime:source").unwrap(),
            },
            lease_epoch: non_zero(5),
            revision: non_zero(revision),
        },
        acquired_at: heartbeat_at - TimeDelta::seconds(1),
        last_heartbeat_at: heartbeat_at,
        expires_at,
        connected: true,
        serving: true,
    }
}

fn serving_source_correlation_v3(
    candidate: &RuntimePendingDrainCandidateV2,
) -> RuntimePendingDrainServingSourceCorrelationV3 {
    RuntimePendingDrainServingSourceCorrelationV3::new(
        candidate.intent_id().clone(),
        candidate.source_intent_revision(),
        candidate.source_state_digest().clone(),
    )
}

fn selected_unclaimed_v3(
    candidate: RuntimePendingDrainCandidateV2,
) -> RuntimeSelectedPendingDrainCandidateV2 {
    let (_, _, authorization) = begin_execution();
    let selection = authorization.into_pending_drain_selection_v3().unwrap();
    let receipt = RuntimePendingDrainSelectionReceiptV3::new(
        selection.request().correlation().clone(),
        owner_receipt(selection.request(), at(110)),
        RuntimePendingDrainSelectionOutcomeV3::Unclaimed(candidate),
    );
    let RuntimeAcceptedPendingDrainSelectionV3::Unclaimed(selected) =
        selection.accept_selection(receipt).unwrap()
    else {
        panic!("expected unclaimed candidate")
    };
    *selected
}

fn seal(
    request: &crate::RuntimeStartupRecoveryExecutionRequestV2,
    candidate: &RuntimePendingDrainPreviousOwnerClaimedCandidateV3,
) -> RuntimePendingDrainRegistrySealWitnessV2 {
    seal_for(request, candidate.intent_id(), candidate.slot())
}

fn seal_for(
    request: &crate::RuntimeStartupRecoveryExecutionRequestV2,
    intent_id: &RuntimeDrainIntentIdV2,
    slot: &automation_runtime_controller::RuntimeServingSlotV2,
) -> RuntimePendingDrainRegistrySealWitnessV2 {
    RuntimePendingDrainRegistrySealWitnessV2::new(RuntimePendingDrainRegistrySealWitnessInputV2 {
        process_instance_id: request.registry_process_instance_id().clone(),
        slot: slot.clone(),
        pre_slot_observation: None,
        seal_key: intent_id.canonical_bytes(),
        seal_generation: non_zero(1),
        post_slot_admission_generation: non_zero(1),
        post_slot_observation_sequence: non_zero(1),
        pre_registry_observation_sequence: request.registry_observation_sequence(),
        pre_registry_retained_slot_count: request.registry_retained_slot_count(),
        pre_registry_retained_empty_tombstone_count: request
            .registry_retained_empty_tombstone_count(),
        post_registry_observation: RuntimeRegistryRecoveryObservationInputV2 {
            observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(non_zero(
                request.registry_observation_sequence().get() + 1,
            )),
            retained_slot_count: request.registry_retained_slot_count() + 1,
            retained_empty_tombstone_count: request.registry_retained_empty_tombstone_count(),
            staged_route_count: 0,
            serving_route_count: 0,
            draining_route_count: 0,
            sealed_slot_count: 1,
            active_interaction_count: 0,
            failed_closed_slot_count: 0,
            registry_failed_closed: false,
        },
    })
    .unwrap()
}

fn unseal(
    request: &crate::RuntimeStartupRecoveryExecutionRequestV2,
    candidate: &RuntimePendingDrainPreviousOwnerClaimedCandidateV3,
) -> RuntimePendingDrainRegistryUnsealWitnessV2 {
    unseal_for(request, candidate.slot())
}

fn unseal_for(
    request: &crate::RuntimeStartupRecoveryExecutionRequestV2,
    slot: &automation_runtime_controller::RuntimeServingSlotV2,
) -> RuntimePendingDrainRegistryUnsealWitnessV2 {
    let observation = accept_runtime_registry_recovery_empty_observation_v2(
        request.registry_process_instance_id().clone(),
        RuntimeRegistryRecoveryObservationInputV2 {
            observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(non_zero(
                request.registry_observation_sequence().get() + 2,
            )),
            retained_slot_count: request.registry_retained_slot_count() + 1,
            retained_empty_tombstone_count: request.registry_retained_slot_count() + 1,
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
    RuntimePendingDrainRegistryUnsealWitnessV2::new(
        request.registry_process_instance_id().clone(),
        slot.clone(),
        non_zero(2),
        non_zero(2),
        observation,
    )
    .unwrap()
}

#[test]
fn previous_owner_candidate_has_exact_one_step_headroom_and_compact_evidence() {
    let valid = candidate(
        i64::MAX as u64 - 1,
        "process:old",
        "shard:0",
        6,
        8,
        9,
        at(120),
    )
    .unwrap();
    assert_eq!(
        valid.source_intent_revision(),
        non_zero(i64::MAX as u64 - 1)
    );
    assert_eq!(
        valid.predecessor_claim_terminal_digest().as_bytes(),
        &[2; 32]
    );
    assert_eq!(valid.product_mutation_request_sha256(), &[3; 32]);
    assert_eq!(valid.drain_intent_request_sha256(), &[4; 32]);
    assert_eq!(
        candidate(i64::MAX as u64, "process:old", "shard:0", 6, 8, 9, at(120)).unwrap_err(),
        RuntimePendingDrainCompoundErrorV2::IntentRevisionOverflow
    );
    assert_eq!(
        candidate(5, "process:old", "shard:0", 6, i64::MAX as u64, 9, at(120)).unwrap_err(),
        RuntimePendingDrainCompoundErrorV2::ClaimRevisionOverflow
    );
    assert_eq!(
        candidate(5, "process:old", "shard:0", 6, 8, i64::MAX as u64, at(120)).unwrap_err(),
        RuntimePendingDrainCompoundErrorV2::ControllerFenceOverflow
    );
}

#[test]
fn v3_no_candidate_and_unclaimed_handoff_preserve_existing_paths() {
    let (_, _, authorization) = begin_execution();
    let selection = authorization.into_pending_drain_selection_v3().unwrap();
    let action_identity = selection.action_identity().clone();
    let receipt = RuntimePendingDrainSelectionReceiptV3::new(
        selection.request().correlation().clone(),
        owner_receipt(selection.request(), at(110)),
        RuntimePendingDrainSelectionOutcomeV3::NoCandidate,
    );
    let RuntimeAcceptedPendingDrainSelectionV3::NoCandidate(selected) =
        selection.accept_selection(receipt).unwrap()
    else {
        panic!("expected no candidate")
    };
    assert_eq!(selected.request().action_identity(), &action_identity);

    let (mut lifecycle, mut permit, authorization) = begin_execution();
    let selection = authorization.into_pending_drain_selection_v3().unwrap();
    let key = canonical_key();
    let candidate = RuntimePendingDrainCandidateV2::new(
        key.intent_id,
        key.slot,
        key.expected_target,
        non_zero(5),
        RuntimePendingDrainStateDigestV2::new([5; 32]).unwrap(),
    )
    .unwrap();
    let receipt = RuntimePendingDrainSelectionReceiptV3::new(
        selection.request().correlation().clone(),
        owner_receipt(selection.request(), at(110)),
        RuntimePendingDrainSelectionOutcomeV3::Unclaimed(candidate.clone()),
    );
    let RuntimeAcceptedPendingDrainSelectionV3::Unclaimed(selected) =
        selection.accept_selection(receipt).unwrap()
    else {
        panic!("expected unclaimed candidate")
    };
    assert_eq!(selected.candidate(), &candidate);
    assert_eq!(
        selected
            .request()
            .action_identity()
            .correlation()
            .authority_revision(),
        non_zero(3)
    );
    let seal = seal_for(selected.request(), candidate.intent_id(), candidate.slot());
    let unseal = unseal_for(selected.request(), candidate.slot());
    let claim = selected.bind_registry_seal(seal.clone()).unwrap();
    let claim_action = claim.action_identity().clone();
    let claim_owner = owner_receipt(claim.request(), at(111));
    let acknowledgement = claim
        .complete(crate::RuntimePendingDrainClaimReceiptV2::new(
            claim_action.clone(),
            candidate.clone(),
            seal.clone(),
            non_zero(6),
            RuntimePendingDrainStateDigestV2::new([6; 32]).unwrap(),
            RuntimeStartupRecoveryExecutionTerminalDigestV2::new([7; 32]).unwrap(),
            claim_owner,
        ))
        .unwrap();
    let acknowledgement_action = acknowledgement.action_identity().clone();
    let acknowledgement_owner = owner_receipt(acknowledgement.request(), at(112));
    let durable = acknowledgement
        .complete(crate::RuntimePendingDrainAcknowledgementReceiptV2::new(
            acknowledgement_action,
            claim_action,
            candidate,
            seal,
            non_zero(6),
            RuntimePendingDrainStateDigestV2::new([6; 32]).unwrap(),
            RuntimeStartupRecoveryExecutionTerminalDigestV2::new([7; 32]).unwrap(),
            non_zero(7),
            RuntimePendingDrainStateDigestV2::new([8; 32]).unwrap(),
            RuntimeStartupRecoveryExecutionTerminalDigestV2::new([9; 32]).unwrap(),
            acknowledgement_owner,
        ))
        .unwrap();
    let accepted = lifecycle
        .complete_startup_recovery_execution(
            &mut permit,
            durable.complete_registry_rollover(unseal).unwrap(),
        )
        .unwrap();
    assert!(matches!(
        accepted.pending_drain_proof(),
        Some(RuntimePendingDrainExecutionProofV2::Compound(_))
    ));
}

#[test]
fn fresh_previous_owner_is_sealless_and_uses_exact_deterministic_retry() {
    let (mut lifecycle, mut permit, authorization) = begin_execution();
    let selection = authorization.into_pending_drain_selection_v3().unwrap();
    let candidate = candidate(5, "process:old", "shard:0", 6, 8, 9, at(120)).unwrap();
    let receipt = RuntimePendingDrainSelectionReceiptV3::new(
        selection.request().correlation().clone(),
        owner_receipt(selection.request(), at(110)),
        RuntimePendingDrainSelectionOutcomeV3::FreshPreviousOwner(candidate.clone()),
    );
    let RuntimeAcceptedPendingDrainSelectionV3::FreshPreviousOwner(selected) =
        selection.accept_selection(receipt).unwrap()
    else {
        panic!("expected fresh predecessor")
    };
    assert_eq!(selected.retry_after(), Duration::from_secs(1));
    assert_eq!(selected.candidate(), &candidate);
    let accepted = lifecycle
        .complete_startup_recovery_execution(&mut permit, selected.complete())
        .unwrap();
    let execution_proof = accepted.pending_drain_proof().unwrap();
    assert!(!execution_proof
        .matches_outcome(&RuntimeStartupRecoveryExecutionReceiptOutcomeV2::NoCandidate));
    let RuntimePendingDrainExecutionProofV2::Deferred(proof) = execution_proof else {
        panic!("expected deferred proof")
    };
    assert_eq!(proof.candidate(), &candidate);
    assert_eq!(proof.selection_database_now(), at(110));
    assert_eq!(proof.claim_expires_at(), at(120));
    assert_eq!(proof.retry_after(), Duration::from_secs(1));
    assert_eq!(
        permit
            .registry_evidence()
            .empty_observation()
            .observation_sequence()
            .get(),
        6
    );
}

#[test]
fn fresh_unclaimed_source_serving_is_sealless_and_defers_with_exact_evidence() {
    let (mut lifecycle, mut permit, authorization) = begin_execution();
    let selection = authorization.into_pending_drain_selection_v3().unwrap();
    let key = canonical_key();
    let candidate = RuntimePendingDrainCandidateV2::new(
        key.intent_id,
        key.slot,
        key.expected_target,
        non_zero(5),
        RuntimePendingDrainStateDigestV2::new([5; 32]).unwrap(),
    )
    .unwrap();
    let receipt = RuntimePendingDrainSelectionReceiptV3::new(
        selection.request().correlation().clone(),
        owner_receipt(selection.request(), at(110)),
        RuntimePendingDrainSelectionOutcomeV3::Unclaimed(candidate.clone()),
    );
    let RuntimeAcceptedPendingDrainSelectionV3::Unclaimed(selected) =
        selection.accept_selection(receipt).unwrap()
    else {
        panic!("expected unclaimed candidate")
    };
    let serving = serving_receipt_v3(candidate.expected_target().clone(), 7, at(111), at(115));
    let evidence = selected
        .check_fresh_serving_v3(
            serving_source_correlation_v3(&candidate),
            serving.clone(),
            at(112),
        )
        .unwrap();
    let completed = selected.defer_for_fresh_serving_v3(evidence).unwrap();
    let accepted = lifecycle
        .complete_startup_recovery_execution(&mut permit, completed)
        .unwrap();
    let RuntimePendingDrainExecutionProofV2::ServingDeferred(proof) =
        accepted.pending_drain_proof().unwrap()
    else {
        panic!("expected serving deferred proof")
    };
    assert_eq!(proof.candidate(), &candidate);
    assert_eq!(proof.serving(), &serving);
    assert_eq!(proof.observed_at(), at(112));
    assert_eq!(proof.retry_after(), Duration::from_secs(1));
    assert_eq!(
        permit
            .registry_evidence()
            .empty_observation()
            .observation_sequence()
            .get(),
        6
    );
}

#[test]
fn unclaimed_source_serving_deferral_rejects_expired_and_target_drift() {
    for drift_target in [false, true] {
        let (_, _, authorization) = begin_execution();
        let selection = authorization.into_pending_drain_selection_v3().unwrap();
        let key = canonical_key();
        let candidate = RuntimePendingDrainCandidateV2::new(
            key.intent_id,
            key.slot,
            key.expected_target,
            non_zero(5),
            RuntimePendingDrainStateDigestV2::new([5; 32]).unwrap(),
        )
        .unwrap();
        let receipt = RuntimePendingDrainSelectionReceiptV3::new(
            selection.request().correlation().clone(),
            owner_receipt(selection.request(), at(110)),
            RuntimePendingDrainSelectionOutcomeV3::Unclaimed(candidate.clone()),
        );
        let RuntimeAcceptedPendingDrainSelectionV3::Unclaimed(selected) =
            selection.accept_selection(receipt).unwrap()
        else {
            panic!("expected unclaimed candidate")
        };
        let mut target = candidate.expected_target().clone();
        let expires_at = if drift_target {
            target.version = target.version.next().unwrap();
            at(115)
        } else {
            at(112)
        };
        let error = selected
            .check_fresh_serving_v3(
                serving_source_correlation_v3(&candidate),
                serving_receipt_v3(target, 7, at(111), expires_at),
                at(112),
            )
            .unwrap_err();
        assert_eq!(
            error,
            if drift_target {
                RuntimePendingDrainCompoundErrorV2::ServingEvidenceMismatch
            } else {
                RuntimePendingDrainCompoundErrorV2::ServingClassificationMismatch
            }
        );
    }
}

#[test]
fn fresh_source_serving_check_rejects_source_drift_and_future_heartbeat() {
    for case in 0..4 {
        let (_, _, authorization) = begin_execution();
        let selection = authorization.into_pending_drain_selection_v3().unwrap();
        let key = canonical_key();
        let candidate = RuntimePendingDrainCandidateV2::new(
            key.intent_id,
            key.slot,
            key.expected_target,
            non_zero(5),
            RuntimePendingDrainStateDigestV2::new([5; 32]).unwrap(),
        )
        .unwrap();
        let receipt = RuntimePendingDrainSelectionReceiptV3::new(
            selection.request().correlation().clone(),
            owner_receipt(selection.request(), at(110)),
            RuntimePendingDrainSelectionOutcomeV3::Unclaimed(candidate.clone()),
        );
        let RuntimeAcceptedPendingDrainSelectionV3::Unclaimed(selected) =
            selection.accept_selection(receipt).unwrap()
        else {
            panic!("expected unclaimed candidate")
        };
        let heartbeat_at = if case == 3 { at(113) } else { at(111) };
        let source = RuntimePendingDrainServingSourceCorrelationV3::new(
            if case == 0 {
                RuntimeDrainIntentIdV2::parse("09".repeat(16)).unwrap()
            } else {
                candidate.intent_id().clone()
            },
            if case == 1 {
                non_zero(6)
            } else {
                candidate.source_intent_revision()
            },
            if case == 2 {
                RuntimePendingDrainStateDigestV2::new([6; 32]).unwrap()
            } else {
                candidate.source_state_digest().clone()
            },
        );
        let error = selected
            .check_fresh_serving_v3(
                source,
                serving_receipt_v3(
                    candidate.expected_target().clone(),
                    7,
                    heartbeat_at,
                    at(115),
                ),
                at(112),
            )
            .unwrap_err();
        assert_eq!(
            error,
            if case == 3 {
                RuntimePendingDrainCompoundErrorV2::ServingClassificationMismatch
            } else {
                RuntimePendingDrainCompoundErrorV2::ServingEvidenceMismatch
            }
        );
    }
}

#[test]
fn checked_fresh_serving_evidence_cannot_cross_candidate_correlation() {
    let key = canonical_key();
    let candidate = RuntimePendingDrainCandidateV2::new(
        key.intent_id,
        key.slot,
        key.expected_target,
        non_zero(5),
        RuntimePendingDrainStateDigestV2::new([5; 32]).unwrap(),
    )
    .unwrap();
    let selected = selected_unclaimed_v3(candidate.clone());
    let evidence = selected
        .check_fresh_serving_v3(
            serving_source_correlation_v3(&candidate),
            serving_receipt_v3(candidate.expected_target().clone(), 7, at(111), at(115)),
            at(112),
        )
        .unwrap();
    let crossed = RuntimePendingDrainCandidateV2::new(
        RuntimeDrainIntentIdV2::parse("09".repeat(16)).unwrap(),
        candidate.slot().clone(),
        candidate.expected_target().clone(),
        candidate.source_intent_revision(),
        candidate.source_state_digest().clone(),
    )
    .unwrap();
    assert_eq!(
        selected_unclaimed_v3(crossed)
            .defer_for_fresh_serving_v3(evidence)
            .unwrap_err(),
        RuntimePendingDrainCompoundErrorV2::ServingEvidenceMismatch
    );
}

#[test]
fn fresh_retry_preserves_positive_subsecond_expiry_remainder() {
    let (_, _, authorization) = begin_execution();
    let selection = authorization.into_pending_drain_selection_v3().unwrap();
    let database_now = at(110);
    let expires_at = database_now + TimeDelta::microseconds(250);
    let candidate = candidate(5, "process:old", "shard:0", 6, 8, 9, expires_at).unwrap();
    let receipt = RuntimePendingDrainSelectionReceiptV3::new(
        selection.request().correlation().clone(),
        owner_receipt(selection.request(), database_now),
        RuntimePendingDrainSelectionOutcomeV3::FreshPreviousOwner(candidate),
    );
    let RuntimeAcceptedPendingDrainSelectionV3::FreshPreviousOwner(selected) =
        selection.accept_selection(receipt).unwrap()
    else {
        panic!("expected fresh predecessor")
    };
    assert_eq!(selected.retry_after(), Duration::from_micros(250));
}

#[test]
fn fresh_retry_uses_current_owner_remaining_when_it_is_the_minimum() {
    let (_, _, authorization) = begin_execution();
    let selection = authorization.into_pending_drain_selection_v3().unwrap();
    let database_now = at(200) - TimeDelta::microseconds(250);
    let candidate = candidate(5, "process:old", "shard:0", 6, 8, 9, at(201)).unwrap();
    let receipt = RuntimePendingDrainSelectionReceiptV3::new(
        selection.request().correlation().clone(),
        owner_receipt(selection.request(), database_now),
        RuntimePendingDrainSelectionOutcomeV3::FreshPreviousOwner(candidate),
    );
    let RuntimeAcceptedPendingDrainSelectionV3::FreshPreviousOwner(selected) =
        selection.accept_selection(receipt).unwrap()
    else {
        panic!("expected fresh predecessor")
    };
    assert_eq!(selected.retry_after(), Duration::from_micros(250));
}

#[test]
fn database_time_equality_is_expired_and_fresh_classification_rejects() {
    let (_, _, authorization) = begin_execution();
    let selection = authorization.into_pending_drain_selection_v3().unwrap();
    let candidate = candidate(5, "process:old", "shard:0", 6, 8, 9, at(120)).unwrap();
    let receipt = RuntimePendingDrainSelectionReceiptV3::new(
        selection.request().correlation().clone(),
        owner_receipt(selection.request(), at(120)),
        RuntimePendingDrainSelectionOutcomeV3::FreshPreviousOwner(candidate),
    );
    assert_eq!(
        selection.accept_selection(receipt).unwrap_err(),
        RuntimePendingDrainCompoundErrorV2::PreviousClaimClassificationMismatch
    );
}

#[test]
fn expired_previous_owner_uses_one_action_and_durable_receipt_gates_s2() {
    let (mut lifecycle, mut permit, authorization) = begin_execution();
    let selection = authorization.into_pending_drain_selection_v3().unwrap();
    let candidate = candidate(5, "process:old", "shard:0", 6, 8, 9, at(120)).unwrap();
    let seal = seal(selection.request(), &candidate);
    let unseal = unseal(selection.request(), &candidate);
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
    let succession = selected.bind_registry_seal(seal.clone()).unwrap();
    assert_eq!(
        succession
            .action_identity()
            .correlation()
            .authority_revision(),
        non_zero(3)
    );
    let action_identity = succession.action_identity().clone();
    let durable_owner = owner_receipt(succession.request(), at(121));
    let durable = succession
        .complete(RuntimePendingDrainSuccessionAcknowledgementReceiptV3::new(
            action_identity.clone(),
            candidate.clone(),
            seal.clone(),
            non_zero(6),
            RuntimePendingDrainStateDigestV2::new([3; 32]).unwrap(),
            RuntimeStartupRecoveryExecutionTerminalDigestV2::new([4; 32]).unwrap(),
            durable_owner,
        ))
        .unwrap();
    assert_eq!(durable.candidate(), &candidate);
    assert_eq!(durable.seal_witness(), &seal);
    let accepted = lifecycle
        .complete_startup_recovery_execution(
            &mut permit,
            durable.complete_registry_rollover(unseal).unwrap(),
        )
        .unwrap();
    let execution_proof = accepted.pending_drain_proof().unwrap();
    assert!(!execution_proof.matches_outcome(
        &RuntimeStartupRecoveryExecutionReceiptOutcomeV2::RetryAfter {
            retry_after: Duration::from_secs(1),
        }
    ));
    let RuntimePendingDrainExecutionProofV2::Succession(proof) = execution_proof else {
        panic!("expected succession proof")
    };
    assert_eq!(proof.candidate(), &candidate);
    assert_eq!(proof.seal(), &seal);
    assert_eq!(proof.action_identity(), &action_identity);
    assert_eq!(proof.terminal_digest().as_bytes(), &[4; 32]);
    assert_eq!(proof.acknowledged_intent_revision(), non_zero(6));
    assert_eq!(proof.acknowledged_state_digest().as_bytes(), &[3; 32]);
    assert_eq!(
        proof
            .registry_rollover()
            .registry_observation_sequence()
            .get(),
        8
    );
}

#[test]
fn succession_durable_receipt_rejects_registry_rollover_mismatch() {
    let (_, _, authorization) = begin_execution();
    let selection = authorization.into_pending_drain_selection_v3().unwrap();
    let candidate = candidate(5, "process:old", "shard:0", 6, 8, 9, at(120)).unwrap();
    let seal = seal(selection.request(), &candidate);
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
    let succession = selected.bind_registry_seal(seal).unwrap();
    let action_identity = succession.action_identity().clone();
    let receipt_seal = succession.seal().clone();
    let durable_owner = owner_receipt(succession.request(), at(121));
    let durable = succession
        .complete(RuntimePendingDrainSuccessionAcknowledgementReceiptV3::new(
            action_identity,
            candidate.clone(),
            receipt_seal,
            non_zero(6),
            RuntimePendingDrainStateDigestV2::new([3; 32]).unwrap(),
            RuntimeStartupRecoveryExecutionTerminalDigestV2::new([4; 32]).unwrap(),
            durable_owner,
        ))
        .unwrap();
    let sealed = durable.seal_witness().post_registry_observation();
    let restored = accept_runtime_registry_recovery_empty_observation_v2(
        durable.seal_witness().process_instance_id().clone(),
        RuntimeRegistryRecoveryObservationInputV2 {
            observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(non_zero(
                sealed.observation_sequence.get() + 1,
            )),
            retained_slot_count: sealed.retained_slot_count,
            retained_empty_tombstone_count: sealed.retained_slot_count,
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
    let mismatched = RuntimePendingDrainRegistryUnsealWitnessV2::new(
        durable.seal_witness().process_instance_id().clone(),
        candidate.slot().clone(),
        non_zero(3),
        non_zero(2),
        restored,
    )
    .unwrap();
    assert_eq!(
        durable.complete_registry_rollover(mismatched).unwrap_err(),
        RuntimePendingDrainCompoundErrorV2::RegistryRolloverMismatch
    );
}

#[test]
fn previous_owner_process_shard_epoch_and_expiry_classification_are_closed() {
    for (candidate, expected) in [
        (
            candidate(5, "process:current", "shard:0", 6, 8, 9, at(120)).unwrap(),
            RuntimePendingDrainCompoundErrorV2::PreviousOwnerProcessNotDistinct,
        ),
        (
            candidate(5, "process:old", "shard:1", 6, 8, 9, at(120)).unwrap(),
            RuntimePendingDrainCompoundErrorV2::PreviousOwnerShardMismatch,
        ),
        (
            candidate(5, "process:old", "shard:0", 7, 8, 9, at(120)).unwrap(),
            RuntimePendingDrainCompoundErrorV2::PreviousOwnerEpochNotOlder,
        ),
    ] {
        let (_, _, authorization) = begin_execution();
        let selection = authorization.into_pending_drain_selection_v3().unwrap();
        let receipt = RuntimePendingDrainSelectionReceiptV3::new(
            selection.request().correlation().clone(),
            owner_receipt(selection.request(), at(120)),
            RuntimePendingDrainSelectionOutcomeV3::ExpiredPreviousOwner(candidate),
        );
        assert_eq!(selection.accept_selection(receipt).unwrap_err(), expected);
    }

    let (_, _, authorization) = begin_execution();
    let selection = authorization.into_pending_drain_selection_v3().unwrap();
    let skipped_epoch_candidate = candidate(5, "process:old", "shard:0", 3, 8, 9, at(120)).unwrap();
    let receipt = RuntimePendingDrainSelectionReceiptV3::new(
        selection.request().correlation().clone(),
        owner_receipt(selection.request(), at(120)),
        RuntimePendingDrainSelectionOutcomeV3::ExpiredPreviousOwner(skipped_epoch_candidate),
    );
    assert!(matches!(
        selection.accept_selection(receipt).unwrap(),
        RuntimeAcceptedPendingDrainSelectionV3::ExpiredPreviousOwner(_)
    ));

    let (_, _, authorization) = begin_execution();
    let selection = authorization.into_pending_drain_selection_v3().unwrap();
    let candidate = candidate(5, "process:old", "shard:0", 6, 8, 9, at(121)).unwrap();
    let receipt = RuntimePendingDrainSelectionReceiptV3::new(
        selection.request().correlation().clone(),
        owner_receipt(selection.request(), at(120)),
        RuntimePendingDrainSelectionOutcomeV3::ExpiredPreviousOwner(candidate),
    );
    assert_eq!(
        selection.accept_selection(receipt).unwrap_err(),
        RuntimePendingDrainCompoundErrorV2::PreviousClaimClassificationMismatch
    );
}

#[test]
fn succession_receipt_rejects_action_candidate_seal_revision_and_clock_drift() {
    let build = || {
        let (_, _, authorization) = begin_execution();
        let selection = authorization.into_pending_drain_selection_v3().unwrap();
        let candidate = candidate(5, "process:old", "shard:0", 6, 8, 9, at(120)).unwrap();
        let seal = seal(selection.request(), &candidate);
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
        (
            selected.bind_registry_seal(seal.clone()).unwrap(),
            candidate,
            seal,
        )
    };

    let (authorization, expected_candidate, seal) = build();
    let wrong_action = authorization
        .action_identity()
        .pending_drain_acknowledgement_successor()
        .unwrap();
    let request = authorization.request();
    let receipt = RuntimePendingDrainSuccessionAcknowledgementReceiptV3::new(
        wrong_action,
        expected_candidate,
        seal,
        non_zero(6),
        RuntimePendingDrainStateDigestV2::new([3; 32]).unwrap(),
        RuntimeStartupRecoveryExecutionTerminalDigestV2::new([4; 32]).unwrap(),
        owner_receipt(request, at(121)),
    );
    assert_eq!(
        authorization.complete(receipt).unwrap_err(),
        RuntimePendingDrainCompoundErrorV2::ActionMismatch
    );

    let (authorization, _, seal) = build();
    let foreign_candidate = candidate(4, "process:old", "shard:0", 6, 8, 9, at(120)).unwrap();
    let receipt = RuntimePendingDrainSuccessionAcknowledgementReceiptV3::new(
        authorization.action_identity().clone(),
        foreign_candidate,
        seal,
        non_zero(6),
        RuntimePendingDrainStateDigestV2::new([3; 32]).unwrap(),
        RuntimeStartupRecoveryExecutionTerminalDigestV2::new([4; 32]).unwrap(),
        owner_receipt(authorization.request(), at(121)),
    );
    assert_eq!(
        authorization.complete(receipt).unwrap_err(),
        RuntimePendingDrainCompoundErrorV2::CandidateMismatch
    );

    let (authorization, candidate, _) = build();
    let foreign_seal = RuntimePendingDrainRegistrySealWitnessV2::new(
        RuntimePendingDrainRegistrySealWitnessInputV2 {
            process_instance_id: authorization
                .request()
                .registry_process_instance_id()
                .clone(),
            slot: candidate.slot().clone(),
            pre_slot_observation: None,
            seal_key: [8; 16],
            seal_generation: non_zero(1),
            post_slot_admission_generation: non_zero(1),
            post_slot_observation_sequence: non_zero(1),
            pre_registry_observation_sequence: authorization
                .request()
                .registry_observation_sequence(),
            pre_registry_retained_slot_count: authorization
                .request()
                .registry_retained_slot_count(),
            pre_registry_retained_empty_tombstone_count: authorization
                .request()
                .registry_retained_empty_tombstone_count(),
            post_registry_observation: RuntimeRegistryRecoveryObservationInputV2 {
                observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(non_zero(7)),
                retained_slot_count: 1,
                retained_empty_tombstone_count: 0,
                staged_route_count: 0,
                serving_route_count: 0,
                draining_route_count: 0,
                sealed_slot_count: 1,
                active_interaction_count: 0,
                failed_closed_slot_count: 0,
                registry_failed_closed: false,
            },
        },
    )
    .unwrap();
    let receipt = RuntimePendingDrainSuccessionAcknowledgementReceiptV3::new(
        authorization.action_identity().clone(),
        candidate,
        foreign_seal,
        non_zero(6),
        RuntimePendingDrainStateDigestV2::new([3; 32]).unwrap(),
        RuntimeStartupRecoveryExecutionTerminalDigestV2::new([4; 32]).unwrap(),
        owner_receipt(authorization.request(), at(121)),
    );
    assert_eq!(
        authorization.complete(receipt).unwrap_err(),
        RuntimePendingDrainCompoundErrorV2::SealMismatch
    );

    let (authorization, candidate, seal) = build();
    let receipt = RuntimePendingDrainSuccessionAcknowledgementReceiptV3::new(
        authorization.action_identity().clone(),
        candidate,
        seal,
        non_zero(7),
        RuntimePendingDrainStateDigestV2::new([3; 32]).unwrap(),
        RuntimeStartupRecoveryExecutionTerminalDigestV2::new([4; 32]).unwrap(),
        owner_receipt(authorization.request(), at(121)),
    );
    assert_eq!(
        authorization.complete(receipt).unwrap_err(),
        RuntimePendingDrainCompoundErrorV2::SourceContinuityMismatch
    );

    let (authorization, candidate, seal) = build();
    let receipt = RuntimePendingDrainSuccessionAcknowledgementReceiptV3::new(
        authorization.action_identity().clone(),
        candidate,
        seal,
        non_zero(6),
        RuntimePendingDrainStateDigestV2::new([3; 32]).unwrap(),
        RuntimeStartupRecoveryExecutionTerminalDigestV2::new([4; 32]).unwrap(),
        owner_receipt(authorization.request(), at(119)),
    );
    assert_eq!(
        authorization.complete(receipt).unwrap_err(),
        RuntimePendingDrainCompoundErrorV2::DatabaseClockRegressed
    );
}
