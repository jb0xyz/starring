use std::num::NonZeroU64;

use automation_runtime_controller::{
    GatewayShardIdV1, RuntimeBuildRevisionV1, RuntimeCanonicalDrainIntentStateV2,
    RuntimeCanonicalProductDrainV2, RuntimeClosedRecoveryRouteWitnessV2,
    RuntimeDrainCertificationResolutionV2, RuntimeDrainClaimProgressV2,
    RuntimeDrainClaimSealWitnessV2, RuntimeDrainClaimV2, RuntimeDrainIntentDigestV2,
    RuntimeDrainIntentV2, RuntimeExactLocalRouteIdentityV2, RuntimeGatewayAdmissionSequenceV2,
    RuntimeGatewayOwnerLeaseIdV1, RuntimeGatewayOwnerLeaseReceiptV1, RuntimeGatewayReadyKindV2,
    RuntimePersistedProductDrainRootV2, RuntimePersistedRefencedPendingDrainIntentV2,
    RuntimePersistedRoutedClaimedPendingDrainIntentV2,
    RuntimePersistedUnclaimedPendingDrainIntentV2,
    RuntimePreviousProcessDrainTeardownSuccessionInputV3,
    RuntimePreviousProcessDrainTeardownSuccessionTransitionV3, RuntimeProductMutationDigestV2,
    RuntimeRecoveryIdV2, RuntimeRouteMutationProvenanceV2, RuntimeStartupRecoveryStateV2,
    RuntimeStartupServingStateV2,
};
use automation_runtime_convergence::{
    ControllerId, FencingToken, ProcessInstanceId, RuntimeGeneration, RuntimeProcessIdentityV1,
};
use chrono::{DateTime, Utc};

use super::*;
use crate::{
    accept_runtime_registry_recovery_empty_observation_v2, RuntimeAcceptedStartupRecoveryOutcomeV2,
    RuntimeCapabilityReadinessKindV2, RuntimeCapabilityReadinessReceiptV2,
    RuntimeCapabilityReadinessSetV2, RuntimeClosedDrainRecoveryPermitV2,
    RuntimeClosedRecoveryInputV2, RuntimeClosedRecoveryRegistryEvidenceV2,
    RuntimeGatewayClosedLifecycleV2, RuntimePausedGatewayObservationV2,
    RuntimePausedGatewaySequenceV2, RuntimeRegistryGlobalObservationSequenceV2,
    RuntimeRegistryRecoveryObservationInputV2, RuntimeStartupRecoveryContinuationV2,
};

fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn at(second: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(second, 0).unwrap()
}

fn digest(value: u8) -> RuntimePendingDrainEvidenceDigestV4 {
    RuntimePendingDrainEvidenceDigestV4::new([value; 32]).unwrap()
}

fn terminal(value: u8) -> RuntimeStartupRecoveryExecutionTerminalDigestV2 {
    RuntimeStartupRecoveryExecutionTerminalDigestV2::new([value; 32]).unwrap()
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
                    expires_at: at(300),
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
    let request = observation.request();
    let receipt = automation_runtime_controller::RuntimeStartupRecoveryObservationReceiptV2 {
        correlation: request.correlation.clone(),
        owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1 {
            lease_id: request.gateway_owner_lease_id.clone(),
            owner_revision: request.expected_owner_revision,
            database_now: at(201),
            expires_at: request.expected_owner_expires_at,
        },
        state: RuntimeStartupRecoveryStateV2 {
            serving: RuntimeStartupServingStateV2::Empty,
            recoverable_awaiting_certification_count: 0,
            suspended_local_effect_count: 0,
            pending_runtime_drain_intent_count: 1,
            acknowledged_product_handoff_count: 0,
        },
    };
    let completed = observation.complete(receipt);
    let RuntimeAcceptedStartupRecoveryOutcomeV2::Continue(continuation) = lifecycle
        .complete_startup_recovery_observation(&mut permit, completed)
        .unwrap()
    else {
        panic!("expected continuation")
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

fn owner(
    process: &str,
    lease_epoch: u64,
    owner_revision: u64,
    database_now: i64,
    expires_at: i64,
) -> RuntimeGatewayOwnerLeaseReceiptV1 {
    RuntimeGatewayOwnerLeaseReceiptV1 {
        lease_id: RuntimeGatewayOwnerLeaseIdV1 {
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
            process_instance_id: ProcessInstanceId::parse(process).unwrap(),
            lease_epoch: non_zero(lease_epoch),
            expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
        },
        owner_revision: non_zero(owner_revision),
        database_now: at(database_now),
        expires_at: at(expires_at),
    }
}

fn request_owner(
    request: &crate::RuntimeStartupRecoveryExecutionRequestV2,
    database_now: i64,
) -> RuntimeGatewayOwnerLeaseReceiptV1 {
    RuntimeGatewayOwnerLeaseReceiptV1 {
        lease_id: request.gateway_owner_lease_id().clone(),
        owner_revision: request.expected_owner_revision(),
        database_now: at(database_now),
        expires_at: request.expected_owner_expires_at(),
    }
}

fn unclaimed_source(revision: u64) -> RuntimePersistedUnclaimedPendingDrainIntentV2 {
    let root = persisted_root();
    let intent =
        RuntimeDrainIntentV2::pending_from_persisted(&root, non_zero(revision), None).unwrap();
    let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(intent).unwrap();
    RuntimePersistedUnclaimedPendingDrainIntentV2::from_persisted(
        &root,
        non_zero(revision),
        canonical.persisted_state().unwrap(),
        canonical.state_bytes(),
    )
    .unwrap()
}

fn route(process: &str, fence: u64) -> RuntimeExactLocalRouteIdentityV2 {
    let key = canonical_product().drain_preimage().key.clone();
    RuntimeExactLocalRouteIdentityV2 {
        identity: RuntimeProcessIdentityV1 {
            target: key.expected_target,
            runtime_generation: RuntimeGeneration::new(4).unwrap(),
            process_instance_id: ProcessInstanceId::parse(process).unwrap(),
        },
        controller_fencing_token: FencingToken::new(fence).unwrap(),
        route_incarnation: non_zero(3),
    }
}

fn closed_provenance(
    process: &str,
    claim_owner: &RuntimeGatewayOwnerLeaseReceiptV1,
) -> RuntimeRouteMutationProvenanceV2 {
    RuntimeRouteMutationProvenanceV2::ClosedRecovery(RuntimeClosedRecoveryRouteWitnessV2 {
        recovery_id: RuntimeRecoveryIdV2::parse("0123456789abcdef0123456789abcdef").unwrap(),
        originating_emergency_generation: non_zero(1),
        recovery_generation: non_zero(2),
        recovery_authority_revision: non_zero(3),
        gateway_owner_lease_id: claim_owner.lease_id.clone(),
        observed_owner_revision: claim_owner.owner_revision,
        owner_expires_at: claim_owner.expires_at,
        process_instance_id: ProcessInstanceId::parse(process).unwrap(),
        connection_epoch: non_zero(4),
        paused_admission_revision: non_zero(5),
        connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(6)),
        pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(7)),
    })
}

fn routed_source(
    revision: u64,
    claim_owner: &RuntimeGatewayOwnerLeaseReceiptV1,
    expires_at: i64,
) -> RuntimePersistedRoutedClaimedPendingDrainIntentV2 {
    routed_source_with_seal_observation(revision, claim_owner, expires_at, 12)
}

fn route_absent_source(
    revision: u64,
    claim_owner: &RuntimeGatewayOwnerLeaseReceiptV1,
    expires_at: i64,
) -> RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2 {
    let root = persisted_root();
    let key = canonical_product().drain_preimage().key.clone();
    let seal = RuntimeDrainClaimSealWitnessV2::new(
        &key,
        claim_owner.lease_id.process_instance_id.clone(),
        non_zero(2),
        None,
        non_zero(12),
    )
    .unwrap();
    let claim = RuntimeDrainClaimV2::new(
        &key,
        claim_owner.lease_id.clone(),
        claim_owner.owner_revision,
        claim_owner.lease_id.process_instance_id.clone(),
        ControllerId::parse("controller:claim").unwrap(),
        FencingToken::new(11).unwrap(),
        non_zero(5),
        non_zero(1),
        at(expires_at),
        RuntimeDrainClaimProgressV2::claimed(seal),
    )
    .unwrap();
    let intent =
        RuntimeDrainIntentV2::pending_from_persisted(&root, non_zero(revision), Some(claim))
            .unwrap();
    let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(intent).unwrap();
    RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2::from_persisted(
        &root,
        non_zero(revision),
        canonical.persisted_state().unwrap(),
        canonical.state_bytes(),
    )
    .unwrap()
}

fn routed_source_with_seal_observation(
    revision: u64,
    claim_owner: &RuntimeGatewayOwnerLeaseReceiptV1,
    expires_at: i64,
    seal_observation: u64,
) -> RuntimePersistedRoutedClaimedPendingDrainIntentV2 {
    let root = persisted_root();
    let key = canonical_product().drain_preimage().key.clone();
    let old_route = route(claim_owner.lease_id.process_instance_id.as_str(), 10);
    let seal = RuntimeDrainClaimSealWitnessV2::new(
        &key,
        claim_owner.lease_id.process_instance_id.clone(),
        non_zero(2),
        Some(old_route),
        non_zero(seal_observation),
    )
    .unwrap();
    let claim = RuntimeDrainClaimV2::new(
        &key,
        claim_owner.lease_id.clone(),
        claim_owner.owner_revision,
        claim_owner.lease_id.process_instance_id.clone(),
        ControllerId::parse("controller:claim").unwrap(),
        FencingToken::new(11).unwrap(),
        non_zero(5),
        non_zero(1),
        at(expires_at),
        RuntimeDrainClaimProgressV2::claimed(seal),
    )
    .unwrap();
    let intent =
        RuntimeDrainIntentV2::pending_from_persisted(&root, non_zero(revision), Some(claim))
            .unwrap();
    let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(intent).unwrap();
    RuntimePersistedRoutedClaimedPendingDrainIntentV2::from_persisted(
        &root,
        non_zero(revision),
        canonical.persisted_state().unwrap(),
        canonical.state_bytes(),
    )
    .unwrap()
}

fn refenced_source(
    revision: u64,
    claim_owner: &RuntimeGatewayOwnerLeaseReceiptV1,
    expires_at: i64,
) -> RuntimePersistedRefencedPendingDrainIntentV2 {
    let root = persisted_root();
    let key = canonical_product().drain_preimage().key.clone();
    let old_route = route(claim_owner.lease_id.process_instance_id.as_str(), 10);
    let seal = RuntimeDrainClaimSealWitnessV2::new(
        &key,
        claim_owner.lease_id.process_instance_id.clone(),
        non_zero(2),
        Some(old_route.clone()),
        non_zero(12),
    )
    .unwrap();
    let progress = RuntimeDrainClaimProgressV2::refenced(
        seal,
        closed_provenance(
            claim_owner.lease_id.process_instance_id.as_str(),
            claim_owner,
        ),
        old_route.clone(),
        RuntimeExactLocalRouteIdentityV2 {
            controller_fencing_token: FencingToken::new(11).unwrap(),
            ..old_route
        },
        non_zero(13),
        at(220),
    )
    .unwrap();
    let claim = RuntimeDrainClaimV2::new(
        &key,
        claim_owner.lease_id.clone(),
        claim_owner.owner_revision,
        claim_owner.lease_id.process_instance_id.clone(),
        ControllerId::parse("controller:claim").unwrap(),
        FencingToken::new(11).unwrap(),
        non_zero(5),
        non_zero(2),
        at(expires_at),
        progress,
    )
    .unwrap();
    let intent =
        RuntimeDrainIntentV2::pending_from_persisted(&root, non_zero(revision), Some(claim))
            .unwrap();
    let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(intent).unwrap();
    RuntimePersistedRefencedPendingDrainIntentV2::from_persisted(
        &root,
        non_zero(revision),
        canonical.persisted_state().unwrap(),
        canonical.state_bytes(),
    )
    .unwrap()
}

fn action_identity() -> RuntimeStartupRecoveryExecutionActionIdentityV2 {
    let (_, _, authorization) = begin_execution();
    authorization
        .into_pending_drain_selection_v4()
        .unwrap()
        .request()
        .action_identity()
        .clone()
}

fn journal(
    stage: RuntimePendingDrainJournalStageV4,
    source: (u64, RuntimeDrainCanonicalStateDigestV3),
    successor: (u64, RuntimeDrainCanonicalStateDigestV3),
    claim_owner: &RuntimeGatewayOwnerLeaseReceiptV1,
    claim_revision: u64,
    terminal_value: u8,
) -> RuntimePendingDrainActionJournalEvidenceV4 {
    journal_at(
        stage,
        source,
        successor,
        claim_owner,
        claim_revision,
        terminal_value,
        209,
    )
}

fn journal_at(
    stage: RuntimePendingDrainJournalStageV4,
    source: (u64, RuntimeDrainCanonicalStateDigestV3),
    successor: (u64, RuntimeDrainCanonicalStateDigestV3),
    claim_owner: &RuntimeGatewayOwnerLeaseReceiptV1,
    claim_revision: u64,
    terminal_value: u8,
    committed_at: i64,
) -> RuntimePendingDrainActionJournalEvidenceV4 {
    let action_identity = match stage {
        RuntimePendingDrainJournalStageV4::RoutedClaim => action_identity(),
        RuntimePendingDrainJournalStageV4::RefenceProgress => action_identity()
            .pending_drain_acknowledgement_successor()
            .unwrap(),
    };
    RuntimePendingDrainActionJournalEvidenceV4::new(
        RuntimePendingDrainActionJournalEvidenceInputV4 {
            stage,
            intent_id: canonical_product().drain_preimage().key.intent_id.clone(),
            action_identity,
            owner_lease_id: claim_owner.lease_id.clone(),
            owner_revision: claim_owner.owner_revision,
            process_instance_id: claim_owner.lease_id.process_instance_id.clone(),
            claim_epoch: non_zero(5),
            claim_revision: non_zero(claim_revision),
            controller_fence: FencingToken::new(11).unwrap(),
            source_intent_revision: non_zero(source.0),
            source_state_digest: source.1,
            successor_intent_revision: non_zero(successor.0),
            successor_state_digest: successor.1,
            terminal_digest: terminal(terminal_value),
            committed_at: at(committed_at),
        },
    )
    .unwrap()
}

fn unclaimed_evidence(
    source: &RuntimePersistedUnclaimedPendingDrainIntentV2,
    current_owner: RuntimeGatewayOwnerLeaseReceiptV1,
) -> RuntimePendingDrainCandidateEvidenceInputV4 {
    let observed_at = current_owner.database_now;
    RuntimePendingDrainCandidateEvidenceInputV4 {
        source_state_digest: RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
            source.canonical().state_bytes(),
        ),
        source_deployment_fence: FencingToken::new(10).unwrap(),
        selection_database_now: current_owner.database_now,
        current_owner,
        claim_journal: None,
        refence_journal: None,
        serving: RuntimePendingDrainServingEvidenceV4::absent(observed_at, digest(20)),
        certification: RuntimePendingDrainCertificationEvidenceV4::no_operation_reserved(digest(
            21,
        )),
    }
}

fn routed_evidence(
    source: &RuntimePersistedRoutedClaimedPendingDrainIntentV2,
    current_owner: RuntimeGatewayOwnerLeaseReceiptV1,
) -> RuntimePendingDrainCandidateEvidenceInputV4 {
    let observed_at = current_owner.database_now;
    let unclaimed = unclaimed_source(source.canonical().intent().intent_revision().get() - 1);
    RuntimePendingDrainCandidateEvidenceInputV4 {
        source_state_digest: RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
            source.canonical().state_bytes(),
        ),
        source_deployment_fence: FencingToken::new(11).unwrap(),
        selection_database_now: current_owner.database_now,
        current_owner,
        claim_journal: Some(journal(
            RuntimePendingDrainJournalStageV4::RoutedClaim,
            (
                unclaimed.canonical().intent().intent_revision().get(),
                RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
                    unclaimed.canonical().state_bytes(),
                ),
            ),
            (
                source.canonical().intent().intent_revision().get(),
                RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
                    source.canonical().state_bytes(),
                ),
            ),
            source
                .canonical()
                .intent()
                .state()
                .pending_claim()
                .map(|claim| RuntimeGatewayOwnerLeaseReceiptV1 {
                    lease_id: claim.gateway_owner_lease_id().clone(),
                    owner_revision: claim.observed_owner_revision(),
                    database_now: at(210),
                    expires_at: at(300),
                })
                .as_ref()
                .unwrap(),
            1,
            30,
        )),
        refence_journal: None,
        serving: RuntimePendingDrainServingEvidenceV4::absent(observed_at, digest(31)),
        certification: RuntimePendingDrainCertificationEvidenceV4::no_operation_reserved(digest(
            32,
        )),
    }
}

fn route_absent_evidence(
    source: &RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2,
    current_owner: RuntimeGatewayOwnerLeaseReceiptV1,
) -> RuntimePendingDrainCandidateEvidenceInputV4 {
    let unclaimed = unclaimed_source(source.canonical().intent().intent_revision().get() - 1);
    let claim = source.canonical().intent().state().pending_claim().unwrap();
    let claim_owner = RuntimeGatewayOwnerLeaseReceiptV1 {
        lease_id: claim.gateway_owner_lease_id().clone(),
        owner_revision: claim.observed_owner_revision(),
        database_now: current_owner.database_now,
        expires_at: current_owner.expires_at,
    };
    let observed_at = current_owner.database_now;
    RuntimePendingDrainCandidateEvidenceInputV4 {
        source_state_digest: RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
            source.canonical().state_bytes(),
        ),
        source_deployment_fence: FencingToken::new(11).unwrap(),
        selection_database_now: observed_at,
        current_owner,
        claim_journal: Some(journal(
            RuntimePendingDrainJournalStageV4::RoutedClaim,
            (
                unclaimed.canonical().intent().intent_revision().get(),
                RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
                    unclaimed.canonical().state_bytes(),
                ),
            ),
            (
                source.canonical().intent().intent_revision().get(),
                RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
                    source.canonical().state_bytes(),
                ),
            ),
            &claim_owner,
            1,
            30,
        )),
        refence_journal: None,
        serving: RuntimePendingDrainServingEvidenceV4::absent(observed_at, digest(31)),
        certification: RuntimePendingDrainCertificationEvidenceV4::no_operation_reserved(digest(
            32,
        )),
    }
}

fn refenced_evidence(
    source: &RuntimePersistedRefencedPendingDrainIntentV2,
    current_owner: RuntimeGatewayOwnerLeaseReceiptV1,
) -> RuntimePendingDrainCandidateEvidenceInputV4 {
    let observed_at = current_owner.database_now;
    let claim_owner = source
        .canonical()
        .intent()
        .state()
        .pending_claim()
        .map(|claim| RuntimeGatewayOwnerLeaseReceiptV1 {
            lease_id: claim.gateway_owner_lease_id().clone(),
            owner_revision: claim.observed_owner_revision(),
            database_now: at(210),
            expires_at: at(300),
        })
        .unwrap();
    let claimed = routed_source(
        source.canonical().intent().intent_revision().get() - 1,
        &claim_owner,
        250,
    );
    let unclaimed = unclaimed_source(claimed.canonical().intent().intent_revision().get() - 1);
    let claimed_digest =
        RuntimeDrainCanonicalStateDigestV3::from_state_bytes(claimed.canonical().state_bytes());
    let refenced_digest =
        RuntimeDrainCanonicalStateDigestV3::from_state_bytes(source.canonical().state_bytes());
    RuntimePendingDrainCandidateEvidenceInputV4 {
        source_state_digest: refenced_digest.clone(),
        source_deployment_fence: FencingToken::new(11).unwrap(),
        selection_database_now: current_owner.database_now,
        current_owner,
        claim_journal: Some(journal(
            RuntimePendingDrainJournalStageV4::RoutedClaim,
            (
                unclaimed.canonical().intent().intent_revision().get(),
                RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
                    unclaimed.canonical().state_bytes(),
                ),
            ),
            (
                claimed.canonical().intent().intent_revision().get(),
                claimed_digest.clone(),
            ),
            &claim_owner,
            1,
            40,
        )),
        refence_journal: Some(journal(
            RuntimePendingDrainJournalStageV4::RefenceProgress,
            (
                claimed.canonical().intent().intent_revision().get(),
                claimed_digest,
            ),
            (
                source.canonical().intent().intent_revision().get(),
                refenced_digest,
            ),
            &claim_owner,
            2,
            41,
        )),
        serving: RuntimePendingDrainServingEvidenceV4::absent(observed_at, digest(42)),
        certification: RuntimePendingDrainCertificationEvidenceV4::no_operation_reserved(digest(
            43,
        )),
    }
}

fn routed_seal(
    candidate: &RuntimeUnclaimedPendingDrainCandidateV4,
    active_guards: u64,
) -> Result<RuntimeRoutedSealedWitnessV4, RuntimePendingDrainV4Error> {
    RuntimeRoutedSealedWitnessV4::new(RuntimeRoutedSealedWitnessInputV4 {
        registry_lifetime_digest: digest(50),
        process_instance_id: candidate
            .current_owner()
            .lease_id
            .process_instance_id
            .clone(),
        intent_id: candidate.intent_id().clone(),
        slot: candidate.slot().clone(),
        seal_key: candidate.intent_id().canonical_bytes(),
        seal_generation: non_zero(2),
        admission_generation: non_zero(3),
        route: route(
            candidate
                .current_owner()
                .lease_id
                .process_instance_id
                .as_str(),
            10,
        ),
        slot_observation_sequence: non_zero(12),
        registry_observation_sequence: non_zero(13),
        active_guards,
    })
}

#[derive(Clone, Copy)]
enum TeardownReceiptDrift {
    None,
    RecoveryAuthority,
    ClaimJournalDigest,
    SealObservation,
    RegistryObservation,
    AcknowledgedAt,
}

fn routed_teardown_receipt(
    drift: TeardownReceiptDrift,
) -> (
    RuntimeAuthorizedPreviousProcessDrainTeardownV4,
    RuntimePreviousProcessDrainTeardownReceiptV4,
) {
    let (_, _, authorization) = begin_execution();
    let selection = authorization.into_pending_drain_selection_v4().unwrap();
    let current_owner = request_owner(selection.request(), 260);
    let previous_owner = owner("process:previous", 6, 4, 260, 300);
    let source = routed_source(6, &previous_owner, 250);
    let candidate = RuntimeRoutedClaimedPendingDrainCandidateV4::new(
        source.clone(),
        routed_evidence(&source, current_owner.clone()),
    )
    .unwrap();
    let predecessor_route = candidate.source_route().clone();
    let receipt = RuntimePendingDrainSelectionReceiptV4::new(
        selection.request().correlation().clone(),
        RuntimePendingDrainSelectionOutcomeV4::ExpiredPreviousOwnerRoutedClaimed(candidate),
    );
    let RuntimeAcceptedPendingDrainSelectionV4::ExpiredPreviousOwnerRoutedClaimed(selected) =
        selection.accept_selection(receipt).unwrap()
    else {
        panic!("expected expired routed selection")
    };
    let successor_identity = RuntimeProcessIdentityV1 {
        target: predecessor_route.identity.target.clone(),
        runtime_generation: RuntimeGeneration::new(5).unwrap(),
        process_instance_id: current_owner.lease_id.process_instance_id.clone(),
    };
    let seal =
        RuntimeEmptySuccessionSealedWitnessV4::new(RuntimeEmptySuccessionSealedWitnessInputV4 {
            registry_lifetime_digest: digest(90),
            process_instance_id: current_owner.lease_id.process_instance_id.clone(),
            successor_identity,
            intent_id: source.canonical().intent().key().intent_id.clone(),
            slot: source.canonical().intent().key().slot.clone(),
            seal_key: source
                .canonical()
                .intent()
                .key()
                .intent_id
                .canonical_bytes(),
            seal_generation: non_zero(2),
            admission_generation: non_zero(3),
            predecessor_route,
            possible_route_fence_ceiling: FencingToken::new(11).unwrap(),
            successor_fence: FencingToken::new(12).unwrap(),
            slot_observation_sequence: non_zero(12),
            registry_observation_sequence: non_zero(14),
            active_guards: 0,
        })
        .unwrap();
    let certification = RuntimeDrainCertificationResolutionV2::no_operation_reserved();
    let authorization = selected
        .bind_empty_succession_seal(seal, certification.clone())
        .unwrap();
    let RuntimeRouteMutationProvenanceV2::ClosedRecovery(mut recovery_witness) =
        expected_teardown_provenance(authorization.request(), authorization.action_identity())
            .unwrap()
    else {
        panic!("expected closed recovery provenance")
    };
    assert_eq!(
        recovery_witness.originating_emergency_generation.get() + 1,
        recovery_witness.recovery_generation.get()
    );
    assert_eq!(
        recovery_witness.gateway_owner_lease_id.process_instance_id,
        recovery_witness.process_instance_id
    );
    assert!(
        recovery_witness.pause_sequence.get() > recovery_witness.connected_event_sequence.get()
    );
    if matches!(drift, TeardownReceiptDrift::RecoveryAuthority) {
        recovery_witness.recovery_authority_revision =
            non_zero(recovery_witness.recovery_authority_revision.get() + 1);
    }
    let claim_digest = if matches!(drift, TeardownReceiptDrift::ClaimJournalDigest) {
        terminal_digest_v3(&terminal(31)).unwrap()
    } else {
        terminal_digest_v3(&terminal(30)).unwrap()
    };
    let seal_observation = if matches!(drift, TeardownReceiptDrift::SealObservation) {
        non_zero(13)
    } else {
        non_zero(12)
    };
    let registry_observation = if matches!(drift, TeardownReceiptDrift::RegistryObservation) {
        non_zero(15)
    } else {
        non_zero(14)
    };
    let acknowledged_at = if matches!(drift, TeardownReceiptDrift::AcknowledgedAt) {
        at(262)
    } else {
        at(261)
    };
    let transition =
        RuntimePreviousProcessDrainTeardownSuccessionTransitionV3::from_routed_claimed(
            source.clone(),
            RuntimePreviousProcessDrainTeardownSuccessionInputV3 {
                database_now: at(260),
                recovery_witness,
                controller_id: ControllerId::parse("controller:successor").unwrap(),
                seal_generation: non_zero(2),
                seal_observation_sequence: seal_observation,
                registry_observation_sequence: registry_observation,
                predecessor_claim_terminal_digest: claim_digest,
                predecessor_refence_terminal_digest: None,
                certification,
                acknowledged_at,
            },
        )
        .unwrap();
    let result = transition.result().clone();
    let mutation =
        RuntimePendingDrainMutationReceiptV4::new(RuntimePendingDrainMutationReceiptInputV4 {
            action_identity: authorization.action_identity().clone(),
            source_intent_revision: source.canonical().intent().intent_revision(),
            source_state_digest: RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
                source.canonical().state_bytes(),
            ),
            result_intent_revision: result.intent_revision(),
            result_state_bytes: result.state_bytes().to_vec().into_boxed_slice(),
            result_state_digest: RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
                result.state_bytes(),
            ),
            owner_receipt: current_owner,
            terminal_digest: terminal(91),
            committed_at: at(261),
        })
        .unwrap();
    let receipt = RuntimePreviousProcessDrainTeardownReceiptV4::new(mutation, result).unwrap();
    (authorization, receipt)
}

#[test]
fn selection_class_is_the_exact_closed_set_of_eleven() {
    let classes = [
        RuntimePendingDrainSelectionClassV4::NoCandidate,
        RuntimePendingDrainSelectionClassV4::Unclaimed,
        RuntimePendingDrainSelectionClassV4::CurrentOwnerRouteAbsentClaimed,
        RuntimePendingDrainSelectionClassV4::CurrentOwnerRoutedClaimed,
        RuntimePendingDrainSelectionClassV4::CurrentOwnerRefenced,
        RuntimePendingDrainSelectionClassV4::FreshPreviousOwnerRouteAbsentClaimed,
        RuntimePendingDrainSelectionClassV4::ExpiredPreviousOwnerRouteAbsentClaimed,
        RuntimePendingDrainSelectionClassV4::FreshPreviousOwnerRoutedClaimed,
        RuntimePendingDrainSelectionClassV4::ExpiredPreviousOwnerRoutedClaimed,
        RuntimePendingDrainSelectionClassV4::FreshPreviousOwnerRefenced,
        RuntimePendingDrainSelectionClassV4::ExpiredPreviousOwnerRefenced,
    ];
    assert_eq!(classes.len(), 11);
    for (index, class) in classes.iter().enumerate() {
        assert!(!classes[..index].contains(class));
    }
}

fn accept_selection_class(
    class: RuntimePendingDrainSelectionClassV4,
) -> RuntimeAcceptedPendingDrainSelectionV4 {
    let (_, _, authorization) = begin_execution();
    let selection = authorization.into_pending_drain_selection_v4().unwrap();
    let database_now = match class {
        RuntimePendingDrainSelectionClassV4::ExpiredPreviousOwnerRouteAbsentClaimed
        | RuntimePendingDrainSelectionClassV4::ExpiredPreviousOwnerRoutedClaimed
        | RuntimePendingDrainSelectionClassV4::ExpiredPreviousOwnerRefenced => 260,
        _ => 210,
    };
    let current_owner = request_owner(selection.request(), database_now);
    let previous_owner = owner("process:previous", 6, 4, database_now, 300);
    let outcome = match class {
        RuntimePendingDrainSelectionClassV4::NoCandidate => {
            RuntimePendingDrainSelectionOutcomeV4::NoCandidate(current_owner)
        }
        RuntimePendingDrainSelectionClassV4::Unclaimed => {
            let source = unclaimed_source(5);
            RuntimePendingDrainSelectionOutcomeV4::Unclaimed(
                RuntimeUnclaimedPendingDrainCandidateV4::new(
                    source.clone(),
                    unclaimed_evidence(&source, current_owner),
                )
                .unwrap(),
            )
        }
        RuntimePendingDrainSelectionClassV4::CurrentOwnerRouteAbsentClaimed => {
            let source = route_absent_source(6, &current_owner, 280);
            RuntimePendingDrainSelectionOutcomeV4::CurrentOwnerRouteAbsentClaimed(
                RuntimeRouteAbsentClaimedPendingDrainCandidateV4::new(
                    source.clone(),
                    route_absent_evidence(&source, current_owner),
                )
                .unwrap(),
            )
        }
        RuntimePendingDrainSelectionClassV4::CurrentOwnerRoutedClaimed => {
            let source = routed_source(6, &current_owner, 280);
            RuntimePendingDrainSelectionOutcomeV4::CurrentOwnerRoutedClaimed(
                RuntimeRoutedClaimedPendingDrainCandidateV4::new(
                    source.clone(),
                    routed_evidence(&source, current_owner),
                )
                .unwrap(),
            )
        }
        RuntimePendingDrainSelectionClassV4::CurrentOwnerRefenced => {
            let source = refenced_source(7, &current_owner, 280);
            RuntimePendingDrainSelectionOutcomeV4::CurrentOwnerRefenced(
                RuntimeRefencedPendingDrainCandidateV4::new(
                    source.clone(),
                    refenced_evidence(&source, current_owner),
                )
                .unwrap(),
            )
        }
        RuntimePendingDrainSelectionClassV4::FreshPreviousOwnerRouteAbsentClaimed => {
            let source = route_absent_source(6, &previous_owner, 250);
            RuntimePendingDrainSelectionOutcomeV4::FreshPreviousOwnerRouteAbsentClaimed(
                RuntimeRouteAbsentClaimedPendingDrainCandidateV4::new(
                    source.clone(),
                    route_absent_evidence(&source, current_owner),
                )
                .unwrap(),
            )
        }
        RuntimePendingDrainSelectionClassV4::ExpiredPreviousOwnerRouteAbsentClaimed => {
            let source = route_absent_source(6, &previous_owner, 250);
            RuntimePendingDrainSelectionOutcomeV4::ExpiredPreviousOwnerRouteAbsentClaimed(
                RuntimeRouteAbsentClaimedPendingDrainCandidateV4::new(
                    source.clone(),
                    route_absent_evidence(&source, current_owner),
                )
                .unwrap(),
            )
        }
        RuntimePendingDrainSelectionClassV4::FreshPreviousOwnerRoutedClaimed => {
            let source = routed_source(6, &previous_owner, 250);
            RuntimePendingDrainSelectionOutcomeV4::FreshPreviousOwnerRoutedClaimed(
                RuntimeRoutedClaimedPendingDrainCandidateV4::new(
                    source.clone(),
                    routed_evidence(&source, current_owner),
                )
                .unwrap(),
            )
        }
        RuntimePendingDrainSelectionClassV4::ExpiredPreviousOwnerRoutedClaimed => {
            let source = routed_source(6, &previous_owner, 250);
            RuntimePendingDrainSelectionOutcomeV4::ExpiredPreviousOwnerRoutedClaimed(
                RuntimeRoutedClaimedPendingDrainCandidateV4::new(
                    source.clone(),
                    routed_evidence(&source, current_owner),
                )
                .unwrap(),
            )
        }
        RuntimePendingDrainSelectionClassV4::FreshPreviousOwnerRefenced => {
            let source = refenced_source(7, &previous_owner, 250);
            RuntimePendingDrainSelectionOutcomeV4::FreshPreviousOwnerRefenced(
                RuntimeRefencedPendingDrainCandidateV4::new(
                    source.clone(),
                    refenced_evidence(&source, current_owner),
                )
                .unwrap(),
            )
        }
        RuntimePendingDrainSelectionClassV4::ExpiredPreviousOwnerRefenced => {
            let source = refenced_source(7, &previous_owner, 250);
            RuntimePendingDrainSelectionOutcomeV4::ExpiredPreviousOwnerRefenced(
                RuntimeRefencedPendingDrainCandidateV4::new(
                    source.clone(),
                    refenced_evidence(&source, current_owner),
                )
                .unwrap(),
            )
        }
    };
    let correlation = selection.request().correlation().clone();
    selection
        .accept_selection(RuntimePendingDrainSelectionReceiptV4::new(
            correlation,
            outcome,
        ))
        .unwrap()
}

#[test]
fn every_selection_class_has_a_valid_success_path() {
    let classes = [
        RuntimePendingDrainSelectionClassV4::NoCandidate,
        RuntimePendingDrainSelectionClassV4::Unclaimed,
        RuntimePendingDrainSelectionClassV4::CurrentOwnerRouteAbsentClaimed,
        RuntimePendingDrainSelectionClassV4::CurrentOwnerRoutedClaimed,
        RuntimePendingDrainSelectionClassV4::CurrentOwnerRefenced,
        RuntimePendingDrainSelectionClassV4::FreshPreviousOwnerRouteAbsentClaimed,
        RuntimePendingDrainSelectionClassV4::ExpiredPreviousOwnerRouteAbsentClaimed,
        RuntimePendingDrainSelectionClassV4::FreshPreviousOwnerRoutedClaimed,
        RuntimePendingDrainSelectionClassV4::ExpiredPreviousOwnerRoutedClaimed,
        RuntimePendingDrainSelectionClassV4::FreshPreviousOwnerRefenced,
        RuntimePendingDrainSelectionClassV4::ExpiredPreviousOwnerRefenced,
    ];
    for class in classes {
        assert_eq!(accept_selection_class(class).class(), class);
    }
}

#[test]
fn candidate_rejects_digest_clock_journal_and_overflow_drift() {
    assert_eq!(
        RuntimePendingDrainEvidenceDigestV4::new([0; 32]).unwrap_err(),
        RuntimePendingDrainV4Error::ZeroDigest
    );
    let source = unclaimed_source(5);
    let current_owner = owner("process:current", 7, 8, 210, 300);
    let mut wrong_digest = unclaimed_evidence(&source, current_owner.clone());
    wrong_digest.source_state_digest =
        RuntimeDrainCanonicalStateDigestV3::from_state_bytes(b"different");
    assert_eq!(
        RuntimeUnclaimedPendingDrainCandidateV4::new(source.clone(), wrong_digest).unwrap_err(),
        RuntimePendingDrainV4Error::SourceDigestMismatch
    );
    let mut wrong_clock = unclaimed_evidence(&source, current_owner.clone());
    wrong_clock.selection_database_now = at(211);
    assert_eq!(
        RuntimeUnclaimedPendingDrainCandidateV4::new(source.clone(), wrong_clock).unwrap_err(),
        RuntimePendingDrainV4Error::DatabaseClockMismatch
    );
    let mut unexpected = unclaimed_evidence(&source, current_owner.clone());
    unexpected.claim_journal = Some(journal(
        RuntimePendingDrainJournalStageV4::RoutedClaim,
        (
            4,
            RuntimeDrainCanonicalStateDigestV3::from_state_bytes(b"source"),
        ),
        (
            5,
            RuntimeDrainCanonicalStateDigestV3::from_state_bytes(source.canonical().state_bytes()),
        ),
        &current_owner,
        1,
        60,
    ));
    assert_eq!(
        RuntimeUnclaimedPendingDrainCandidateV4::new(source, unexpected).unwrap_err(),
        RuntimePendingDrainV4Error::UnexpectedJournal
    );
    let overflow = unclaimed_source(i64::MAX as u64 - 2);
    assert_eq!(
        RuntimeUnclaimedPendingDrainCandidateV4::new(
            overflow.clone(),
            unclaimed_evidence(&overflow, current_owner),
        )
        .unwrap_err(),
        RuntimePendingDrainV4Error::IntentRevisionOverflow
    );
}

#[test]
fn candidate_rejects_database_time_serving_and_journal_correlation_drift() {
    let source = unclaimed_source(5);
    let mut sub_microsecond_owner = owner("process:current", 7, 8, 210, 300);
    sub_microsecond_owner.database_now = DateTime::from_timestamp(210, 1).unwrap();
    assert_eq!(
        RuntimeUnclaimedPendingDrainCandidateV4::new(
            source.clone(),
            unclaimed_evidence(&source, sub_microsecond_owner),
        )
        .unwrap_err(),
        RuntimePendingDrainV4Error::DatabaseTimeOutOfRange
    );

    let current_owner = owner("process:current", 7, 8, 210, 300);
    let mut stale_serving = unclaimed_evidence(&source, current_owner.clone());
    stale_serving.serving = RuntimePendingDrainServingEvidenceV4::absent(at(209), digest(20));
    assert_eq!(
        RuntimeUnclaimedPendingDrainCandidateV4::new(source, stale_serving).unwrap_err(),
        RuntimePendingDrainV4Error::ServingEvidenceMismatch
    );

    let routed = routed_source(6, &current_owner, 250);
    let unclaimed = unclaimed_source(5);
    let mut future_claim = routed_evidence(&routed, current_owner.clone());
    future_claim.claim_journal = Some(journal_at(
        RuntimePendingDrainJournalStageV4::RoutedClaim,
        (
            5,
            RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
                unclaimed.canonical().state_bytes(),
            ),
        ),
        (
            6,
            RuntimeDrainCanonicalStateDigestV3::from_state_bytes(routed.canonical().state_bytes()),
        ),
        &current_owner,
        1,
        30,
        211,
    ));
    assert_eq!(
        RuntimeRoutedClaimedPendingDrainCandidateV4::new(routed, future_claim).unwrap_err(),
        RuntimePendingDrainV4Error::ClaimJournalMismatch
    );

    let refenced = refenced_source(7, &current_owner, 250);
    let claimed = routed_source(6, &current_owner, 250);
    let mut duplicate_terminal = refenced_evidence(&refenced, current_owner.clone());
    duplicate_terminal.refence_journal = Some(journal(
        RuntimePendingDrainJournalStageV4::RefenceProgress,
        (
            6,
            RuntimeDrainCanonicalStateDigestV3::from_state_bytes(claimed.canonical().state_bytes()),
        ),
        (
            7,
            RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
                refenced.canonical().state_bytes(),
            ),
        ),
        &current_owner,
        2,
        40,
    ));
    assert_eq!(
        RuntimeRefencedPendingDrainCandidateV4::new(refenced, duplicate_terminal).unwrap_err(),
        RuntimePendingDrainV4Error::RefenceJournalMismatch
    );
}

#[test]
fn selection_rejects_cross_class_owner_and_expiry_claims() {
    let (_, _, authorization) = begin_execution();
    let selection = authorization.into_pending_drain_selection_v4().unwrap();
    let current_owner = request_owner(selection.request(), 210);
    let previous = owner("process:previous", 6, 4, 210, 300);
    let source = routed_source(6, &previous, 250);
    let candidate = RuntimeRoutedClaimedPendingDrainCandidateV4::new(
        source,
        routed_evidence(&routed_source(6, &previous, 250), current_owner.clone()),
    )
    .unwrap();
    let receipt = RuntimePendingDrainSelectionReceiptV4::new(
        selection.request().correlation().clone(),
        RuntimePendingDrainSelectionOutcomeV4::CurrentOwnerRoutedClaimed(candidate),
    );
    assert_eq!(
        selection.accept_selection(receipt).unwrap_err(),
        RuntimePendingDrainV4Error::CurrentOwnerClaimMismatch
    );

    let (_, _, authorization) = begin_execution();
    let selection = authorization.into_pending_drain_selection_v4().unwrap();
    let current_owner = request_owner(selection.request(), 210);
    let source = routed_source(6, &current_owner, 250);
    let candidate = RuntimeRoutedClaimedPendingDrainCandidateV4::new(
        source,
        routed_evidence(
            &routed_source(6, &current_owner, 250),
            current_owner.clone(),
        ),
    )
    .unwrap();
    let receipt = RuntimePendingDrainSelectionReceiptV4::new(
        selection.request().correlation().clone(),
        RuntimePendingDrainSelectionOutcomeV4::FreshPreviousOwnerRoutedClaimed(candidate),
    );
    assert_eq!(
        selection.accept_selection(receipt).unwrap_err(),
        RuntimePendingDrainV4Error::StableOwnerProcessMismatch
    );

    let (_, _, authorization) = begin_execution();
    let selection = authorization.into_pending_drain_selection_v4().unwrap();
    let current_owner = request_owner(selection.request(), 260);
    let previous = owner("process:previous", 6, 4, 260, 300);
    let source = routed_source(6, &previous, 250);
    let evidence = routed_evidence(&source, current_owner);
    let candidate = RuntimeRoutedClaimedPendingDrainCandidateV4::new(source, evidence).unwrap();
    let receipt = RuntimePendingDrainSelectionReceiptV4::new(
        selection.request().correlation().clone(),
        RuntimePendingDrainSelectionOutcomeV4::FreshPreviousOwnerRoutedClaimed(candidate),
    );
    assert_eq!(
        selection.accept_selection(receipt).unwrap_err(),
        RuntimePendingDrainV4Error::ClaimExpiryClassificationMismatch
    );
}

#[test]
fn routed_claim_authorization_requires_exact_zero_guard_seal() {
    let (_, _, authorization) = begin_execution();
    let selection = authorization.into_pending_drain_selection_v4().unwrap();
    let current_owner = request_owner(selection.request(), 210);
    let source = unclaimed_source(5);
    let candidate = RuntimeUnclaimedPendingDrainCandidateV4::new(
        source,
        unclaimed_evidence(&unclaimed_source(5), current_owner),
    )
    .unwrap();
    assert_eq!(
        routed_seal(&candidate, 1).unwrap_err(),
        RuntimePendingDrainV4Error::ActiveGuards
    );
    let seal = routed_seal(&candidate, 0).unwrap();
    let receipt = RuntimePendingDrainSelectionReceiptV4::new(
        selection.request().correlation().clone(),
        RuntimePendingDrainSelectionOutcomeV4::Unclaimed(candidate),
    );
    let RuntimeAcceptedPendingDrainSelectionV4::Unclaimed(selected) =
        selection.accept_selection(receipt).unwrap()
    else {
        panic!("expected unclaimed selection")
    };
    let authorization = selected.bind_routed_seal(seal).unwrap();
    assert_eq!(
        authorization.action_identity().stage(),
        RuntimePendingDrainActionStageV4::RoutedClaim
    );
    assert_eq!(
        authorization.seal().route().controller_fencing_token.get(),
        10
    );
}

#[test]
fn determinate_non_commit_is_the_only_routed_rollback_authority() {
    let (_, _, authorization) = begin_execution();
    let selection = authorization.into_pending_drain_selection_v4().unwrap();
    let current_owner = request_owner(selection.request(), 210);
    let source = unclaimed_source(5);
    let candidate = RuntimeUnclaimedPendingDrainCandidateV4::new(
        source.clone(),
        unclaimed_evidence(&source, current_owner.clone()),
    )
    .unwrap();
    let seal = routed_seal(&candidate, 0).unwrap();
    let receipt = RuntimePendingDrainSelectionReceiptV4::new(
        selection.request().correlation().clone(),
        RuntimePendingDrainSelectionOutcomeV4::Unclaimed(candidate),
    );
    let RuntimeAcceptedPendingDrainSelectionV4::Unclaimed(selected) =
        selection.accept_selection(receipt).unwrap()
    else {
        panic!("expected unclaimed selection")
    };
    let authorization = selected.bind_routed_seal(seal).unwrap();
    let observation = RuntimeRoutedDrainDeterminateNonCommitObservationV4::new(
        RuntimeRoutedDrainDeterminateNonCommitObservationInputV4 {
            action_identity: authorization.action_identity().clone(),
            source: source.clone(),
            source_state_digest: RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
                source.canonical().state_bytes(),
            ),
            owner: current_owner,
            registry_lifetime_digest: digest(50),
            seal_generation: non_zero(2),
            route: route("process:current", 10),
            slot_observation_sequence: non_zero(12),
            registry_observation_sequence: non_zero(14),
            observation_digest: digest(61),
            observed_at: at(220),
        },
    )
    .unwrap();
    let permit = authorization
        .authorize_determinate_non_commit_rollback(observation)
        .unwrap();
    assert_eq!(permit.seal().seal_generation(), non_zero(2));
    assert_eq!(permit.observation_digest(), digest(61));
}

#[test]
fn durable_routed_claim_receipt_binds_typed_result_and_registry_lineage() {
    let (_, _, authorization) = begin_execution();
    let selection = authorization.into_pending_drain_selection_v4().unwrap();
    let current_owner = request_owner(selection.request(), 210);
    let source = unclaimed_source(5);
    let candidate = RuntimeUnclaimedPendingDrainCandidateV4::new(
        source.clone(),
        unclaimed_evidence(&source, current_owner.clone()),
    )
    .unwrap();
    let seal = routed_seal(&candidate, 0).unwrap();
    let receipt = RuntimePendingDrainSelectionReceiptV4::new(
        selection.request().correlation().clone(),
        RuntimePendingDrainSelectionOutcomeV4::Unclaimed(candidate),
    );
    let RuntimeAcceptedPendingDrainSelectionV4::Unclaimed(selected) =
        selection.accept_selection(receipt).unwrap()
    else {
        panic!("expected unclaimed selection")
    };
    let authorization = selected.bind_routed_seal(seal).unwrap();
    let result = routed_source_with_seal_observation(6, &current_owner, 260, 13);
    let result_digest =
        RuntimeDrainCanonicalStateDigestV3::from_state_bytes(result.canonical().state_bytes());
    let mutation =
        RuntimePendingDrainMutationReceiptV4::new(RuntimePendingDrainMutationReceiptInputV4 {
            action_identity: authorization.action_identity().clone(),
            source_intent_revision: non_zero(5),
            source_state_digest: RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
                source.canonical().state_bytes(),
            ),
            result_intent_revision: non_zero(6),
            result_state_bytes: result.canonical().state_bytes().to_vec().into_boxed_slice(),
            result_state_digest: result_digest,
            owner_receipt: current_owner,
            terminal_digest: terminal(70),
            committed_at: at(220),
        })
        .unwrap();
    let receipt = RuntimeRoutedDrainClaimReceiptV4::new(mutation, result).unwrap();
    let durable = authorization.accept_durable_receipt(receipt).unwrap();
    assert_eq!(durable.claim_fence(), FencingToken::new(11).unwrap());
    assert_eq!(durable.terminal_digest().as_bytes(), &[70; 32]);
    assert_eq!(durable.source_seal().registry_lifetime_digest(), digest(50));
}

#[test]
fn t6_reconstruction_requires_both_exact_journal_receipts() {
    let (_, _, authorization) = begin_execution();
    let selection = authorization.into_pending_drain_selection_v4().unwrap();
    let current_owner = request_owner(selection.request(), 230);
    let source = refenced_source(7, &current_owner, 280);
    let candidate = RuntimeRefencedPendingDrainCandidateV4::new(
        source,
        refenced_evidence(
            &refenced_source(7, &current_owner, 280),
            current_owner.clone(),
        ),
    )
    .unwrap();
    let old_route = route("process:current", 10);
    let routed = RuntimeRoutedSealedWitnessV4::new(RuntimeRoutedSealedWitnessInputV4 {
        registry_lifetime_digest: digest(80),
        process_instance_id: current_owner.lease_id.process_instance_id.clone(),
        intent_id: candidate.intent_id().clone(),
        slot: candidate.slot().clone(),
        seal_key: candidate.intent_id().canonical_bytes(),
        seal_generation: non_zero(2),
        admission_generation: non_zero(3),
        route: old_route.clone(),
        slot_observation_sequence: non_zero(12),
        registry_observation_sequence: non_zero(12),
        active_guards: 0,
    })
    .unwrap();
    let claimed =
        RuntimeRoutedClaimedSealedWitnessV4::new(RuntimeRoutedClaimedSealedWitnessInputV4 {
            routed_seal: routed,
            claim_fence: FencingToken::new(11).unwrap(),
            claim_receipt_digest: digest(40),
        })
        .unwrap();
    let canonical_claim = candidate.claim();
    let local =
        RuntimeLocallyRefencedSealedWitnessV4::new(RuntimeLocallyRefencedSealedWitnessInputV4 {
            claimed,
            old_route,
            removal_target: candidate.removal_target().clone(),
            provenance: canonical_claim.progress().provenance().unwrap().clone(),
            registry_observation_sequence: non_zero(13),
            refenced_at: canonical_claim.progress().refenced_at().unwrap(),
            active_guards: 0,
        })
        .unwrap();
    let durable =
        RuntimeDurablyRefencedSealedWitnessV4::new(RuntimeDurablyRefencedSealedWitnessInputV4 {
            locally_refenced: local,
            refence_receipt_digest: digest(41),
        });
    let receipt = RuntimePendingDrainSelectionReceiptV4::new(
        selection.request().correlation().clone(),
        RuntimePendingDrainSelectionOutcomeV4::CurrentOwnerRefenced(candidate),
    );
    let RuntimeAcceptedPendingDrainSelectionV4::CurrentOwnerRefenced(selected) =
        selection.accept_selection(receipt).unwrap()
    else {
        panic!("expected refenced selection")
    };
    let reconstructed = selected.reconstruct_durable_refence(durable).unwrap();
    assert_eq!(
        reconstructed.durable_witness().refence_receipt_digest(),
        digest(41)
    );
    assert_eq!(
        reconstructed.candidate().claim_terminal_digest().as_bytes(),
        &[40; 32]
    );
}

#[test]
fn t7_receipt_requires_exact_recovery_journals_seal_observations_and_time() {
    let (authorization, receipt) = routed_teardown_receipt(TeardownReceiptDrift::None);
    let durable = authorization.accept_durable_receipt(receipt).unwrap();
    assert_eq!(durable.successor_fence(), FencingToken::new(12).unwrap());
    assert_eq!(durable.terminal_digest().as_bytes(), &[91; 32]);

    for drift in [
        TeardownReceiptDrift::RecoveryAuthority,
        TeardownReceiptDrift::ClaimJournalDigest,
        TeardownReceiptDrift::SealObservation,
        TeardownReceiptDrift::RegistryObservation,
        TeardownReceiptDrift::AcknowledgedAt,
    ] {
        let (authorization, receipt) = routed_teardown_receipt(drift);
        assert_eq!(
            authorization.accept_durable_receipt(receipt).unwrap_err(),
            RuntimePendingDrainV4Error::MutationReceiptMismatch
        );
    }
}

#[test]
fn terminal_unknown_allows_one_replay_and_second_unknown_closes() {
    let action = RuntimePendingDrainActionIdentityV4::successor(
        &action_identity(),
        RuntimePendingDrainActionStageV4::PreviousProcessTeardown,
        1,
    )
    .unwrap();
    let identity = RuntimePendingDrainTerminalIdentityV4::new(
        action,
        non_zero(5),
        RuntimeDrainCanonicalStateDigestV3::from_state_bytes(b"source"),
    )
    .unwrap();
    let unknown: RuntimePendingDrainUnknownResultV4<
        &str,
        RuntimePreviousProcessTeardownMutationStageV4,
    > = RuntimePendingDrainUnknownResultV4::new("authorization", identity.clone());
    let RuntimePendingDrainUnknownResolutionV4::Replay(replay) = unknown
        .accept_observation(RuntimePendingDrainTerminalObservationV4::new(
            identity,
            RuntimePendingDrainTerminalObservationOutcomeV4::NotCommitted,
        ))
        .unwrap()
    else {
        panic!("expected one replay")
    };
    assert_eq!(replay.authorization(), &"authorization");
    let RuntimePendingDrainReplayResolutionV4::Closed(closed) = replay
        .accept_replay(RuntimePendingDrainReplayResultV4::Unknown)
        .unwrap()
    else {
        panic!("expected terminal closed result")
    };
    assert_eq!(
        closed.terminal_identity().source_intent_revision,
        non_zero(5)
    );
}
