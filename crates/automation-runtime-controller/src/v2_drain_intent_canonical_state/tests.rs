use std::num::NonZeroU64;

use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    ActivationRequestId, BindingRevision, ControllerId, DeploymentId, DeploymentRevision,
    FencingToken, InstallationId, ProcessInstanceId, PromotionId, RuntimeDeployment,
    RuntimeDeploymentIdentityV1, RuntimeDeploymentSnapshotV1, RuntimeDeploymentTargetV1,
    RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use chrono::{DateTime, Utc};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;
use sha2::{Digest, Sha256};

use super::{
    validate_compact_pending_drain_succession_v2, RuntimeCanonicalDrainIntentStateV2,
    RuntimeClosedRecoveryEmptyRegistryPendingDrainClaimInputV2,
    RuntimeClosedRecoveryEmptyRegistryPendingDrainClaimTransitionV2,
    RuntimeClosedRecoveryPendingDrainAcknowledgementInputV2,
    RuntimeClosedRecoveryPendingDrainAcknowledgementTransitionV2,
    RuntimeClosedRecoveryPendingDrainSuccessionAcknowledgementInputV2,
    RuntimeClosedRecoveryPendingDrainSuccessionAcknowledgementTransitionV2,
    RuntimeCompactPendingDrainSuccessionValidationErrorV2,
    RuntimeCompactPendingDrainSuccessionValidationInputV2,
    RuntimeDrainIntentCanonicalStateCorrelationV2, RuntimeDrainIntentCanonicalStateErrorV2,
    RuntimeDrainIntentCanonicalStateFieldV2, RuntimeDrainIntentCanonicalStateKindV2,
    RuntimePersistedRefencedPendingDrainIntentV2,
    RuntimePersistedRouteAbsenceCandidateDrainIntentV2,
    RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2,
    RuntimePersistedRoutedClaimedPendingDrainIntentV2,
    RuntimePersistedUnclaimedPendingDrainIntentV2, RuntimeRoutedPendingDrainClaimInputV2,
    RuntimeRoutedPendingDrainClaimTransitionV2, RuntimeRoutedPendingDrainRefenceInputV2,
    RuntimeRoutedPendingDrainRefenceTransitionV2,
    RuntimeSameProcessRefencedDrainAcknowledgementInputV2,
    RuntimeSameProcessRefencedDrainAcknowledgementTransitionV2,
};
use crate::{
    GatewayShardIdV1, RuntimeBarrierIdV1, RuntimeBarrierPauseWitnessV2, RuntimeBuildRevisionV1,
    RuntimeCanonicalProductDrainV2, RuntimeCertificationIntentFingerprintV2,
    RuntimeCertificationOperationIdV2, RuntimeClosedRecoveryRouteWitnessV2,
    RuntimeDrainCertificationResolutionV2, RuntimeDrainClaimProgressV2,
    RuntimeDrainClaimSealWitnessV2, RuntimeDrainClaimV2, RuntimeDrainIntentIdV2,
    RuntimeDrainIntentReceiptErrorV2, RuntimeDrainIntentV2, RuntimeExactLocalRouteIdentityV2,
    RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeLiveAttestationDigestV2, RuntimePersistedProductDrainRootV2,
    RuntimeProductDrainOperationV2, RuntimeProductMutationKindV2, RuntimeProductMutationPreimageV2,
    RuntimeProductOperationIdV2, RuntimeProductSemanticRequestDigestV2, RuntimeRecoveryIdV2,
    RuntimeRouteAbsentAcknowledgementV2, RuntimeRouteMutationProvenanceV2,
    RuntimeServingIdentityV2, RuntimeServingSlotV2, RuntimeShutdownRouteWitnessV2,
};

fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn at(second: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(second, 0).unwrap()
}

fn process_id() -> ProcessInstanceId {
    ProcessInstanceId::parse("process:1").unwrap()
}

fn successor_process_id() -> ProcessInstanceId {
    ProcessInstanceId::parse("process:2").unwrap()
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

fn identity() -> RuntimeDeploymentIdentityV1 {
    RuntimeDeploymentIdentityV1 {
        deployment_id: DeploymentId::parse("deployment:1").unwrap(),
        tenant_id: TenantId::parse("tenant:1").unwrap(),
        installation_id: InstallationId::parse("installation:1").unwrap(),
        promotion_id: PromotionId::parse("c".repeat(64)).unwrap(),
        activation_request_id: ActivationRequestId::parse("activation:1").unwrap(),
    }
}

fn snapshot() -> RuntimeDeploymentSnapshotV1 {
    RuntimeDeployment::request(
        identity(),
        target(),
        RuntimeGeneration::new(4).unwrap(),
        None,
        at(100),
    )
    .unwrap()
    .snapshot()
}

fn operation() -> RuntimeProductDrainOperationV2 {
    let snapshot = snapshot();
    let product = RuntimeProductMutationPreimageV2 {
        operation_id: RuntimeProductOperationIdV2::parse("00112233445566778899aabbccddeeff")
            .unwrap(),
        scope: crate::RuntimeDeploymentScopeV1::from_identity(&snapshot.identity),
        expected_revision: snapshot.revision,
        slot: RuntimeServingSlotV2::from_target(&snapshot.target),
        expected_target: snapshot.target.clone(),
        mutation_kind: RuntimeProductMutationKindV2::AuthorityChange,
        product_semantic_request_digest: RuntimeProductSemanticRequestDigestV2::parse(
            "d".repeat(64),
        )
        .unwrap(),
    };
    let canonical = RuntimeCanonicalProductDrainV2::new(
        product,
        RuntimeDrainIntentIdV2::parse("ffeeddccbbaa99887766554433221100").unwrap(),
    )
    .unwrap();
    RuntimeProductDrainOperationV2::new(&snapshot, canonical).unwrap()
}

fn root(operation: &RuntimeProductDrainOperationV2) -> RuntimePersistedProductDrainRootV2 {
    RuntimePersistedProductDrainRootV2::from_persisted(
        operation.product_operation_scope().scope().clone(),
        operation.product_operation_scope().expected_revision(),
        operation.product_operation_id(),
        operation.drain_intent_scope().scope().clone(),
        operation.drain_intent_scope().slot().clone(),
        operation.drain_intent_scope().expected_revision(),
        operation.drain_intent_id(),
        &operation.canonical().product_preimage().expected_target,
        operation.product_mutation_request_bytes(),
        operation.product_mutation_digest(),
        operation.drain_intent_request_bytes(),
        operation.drain_intent_digest(),
    )
    .unwrap()
}

fn owner() -> RuntimeGatewayOwnerLeaseIdV1 {
    RuntimeGatewayOwnerLeaseIdV1 {
        gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        process_instance_id: process_id(),
        lease_epoch: non_zero(3),
        expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
    }
}

fn successor_owner() -> RuntimeGatewayOwnerLeaseIdV1 {
    RuntimeGatewayOwnerLeaseIdV1 {
        gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        process_instance_id: successor_process_id(),
        lease_epoch: non_zero(4),
        expected_build_revision: RuntimeBuildRevisionV1::parse("build:2").unwrap(),
    }
}

fn route(fence: u64) -> RuntimeExactLocalRouteIdentityV2 {
    RuntimeExactLocalRouteIdentityV2 {
        identity: RuntimeProcessIdentityV1 {
            target: target(),
            runtime_generation: RuntimeGeneration::new(6).unwrap(),
            process_instance_id: process_id(),
        },
        controller_fencing_token: FencingToken::new(fence).unwrap(),
        route_incarnation: non_zero(8),
    }
}

fn ordinary_provenance() -> RuntimeRouteMutationProvenanceV2 {
    RuntimeRouteMutationProvenanceV2::Ordinary {
        barrier_id: RuntimeBarrierIdV1::parse("9999aaaabbbbccccddddeeeeffff0000").unwrap(),
        pause: RuntimeBarrierPauseWitnessV2 {
            coordinator_generation: non_zero(4),
            connection_epoch: non_zero(5),
            paused_admission_revision: non_zero(6),
            pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(7)),
        },
    }
}

fn claim(operation: &RuntimeProductDrainOperationV2, refenced: bool) -> RuntimeDrainClaimV2 {
    let key = &operation.canonical().drain_preimage().key;
    let expected_route = refenced.then(|| route(19));
    let seal = RuntimeDrainClaimSealWitnessV2::new(
        key,
        process_id(),
        non_zero(14),
        expected_route.clone(),
        non_zero(15),
    )
    .unwrap();
    let progress = if let Some(old_route) = expected_route {
        RuntimeDrainClaimProgressV2::refenced(
            seal,
            ordinary_provenance(),
            old_route,
            route(20),
            non_zero(21),
            at(110),
        )
        .unwrap()
    } else {
        RuntimeDrainClaimProgressV2::claimed(seal)
    };
    RuntimeDrainClaimV2::new(
        key,
        owner(),
        non_zero(16),
        process_id(),
        ControllerId::parse("controller:1").unwrap(),
        FencingToken::new(20).unwrap(),
        non_zero(17),
        non_zero(18),
        at(130),
        progress,
    )
    .unwrap()
}

fn routed_claimed(operation: &RuntimeProductDrainOperationV2) -> RuntimeDrainClaimV2 {
    routed_claim_with_numbers(operation, 19, 20, 18)
}

fn routed_claim_with_numbers(
    operation: &RuntimeProductDrainOperationV2,
    route_fence: u64,
    claim_fence: u64,
    claim_revision: u64,
) -> RuntimeDrainClaimV2 {
    let key = &operation.canonical().drain_preimage().key;
    let seal = RuntimeDrainClaimSealWitnessV2::new(
        key,
        process_id(),
        non_zero(14),
        Some(route(route_fence)),
        non_zero(15),
    )
    .unwrap();
    RuntimeDrainClaimV2::new(
        key,
        owner(),
        non_zero(16),
        process_id(),
        ControllerId::parse("controller:1").unwrap(),
        FencingToken::new(claim_fence).unwrap(),
        non_zero(17),
        non_zero(claim_revision),
        at(130),
        RuntimeDrainClaimProgressV2::claimed(seal),
    )
    .unwrap()
}

fn refenced_claim_with_numbers(
    operation: &RuntimeProductDrainOperationV2,
    route_fence: u64,
    removal_fence: u64,
    claim_revision: u64,
    provenance: RuntimeRouteMutationProvenanceV2,
) -> RuntimeDrainClaimV2 {
    let key = &operation.canonical().drain_preimage().key;
    let old_route = route(route_fence);
    let seal = RuntimeDrainClaimSealWitnessV2::new(
        key,
        process_id(),
        non_zero(14),
        Some(old_route.clone()),
        non_zero(15),
    )
    .unwrap();
    let progress = RuntimeDrainClaimProgressV2::refenced(
        seal,
        provenance,
        old_route,
        route(removal_fence),
        non_zero(21),
        at(110),
    )
    .unwrap();
    RuntimeDrainClaimV2::new(
        key,
        owner(),
        non_zero(16),
        process_id(),
        ControllerId::parse("controller:1").unwrap(),
        FencingToken::new(removal_fence).unwrap(),
        non_zero(17),
        non_zero(claim_revision),
        at(500),
        progress,
    )
    .unwrap()
}

fn route_absent_claim_with_numbers(
    operation: &RuntimeProductDrainOperationV2,
    claim_revision: u64,
    controller_fencing_token: u64,
) -> RuntimeDrainClaimV2 {
    let key = &operation.canonical().drain_preimage().key;
    let seal =
        RuntimeDrainClaimSealWitnessV2::new(key, process_id(), non_zero(14), None, non_zero(15))
            .unwrap();
    RuntimeDrainClaimV2::new(
        key,
        owner(),
        non_zero(16),
        process_id(),
        ControllerId::parse("controller:1").unwrap(),
        FencingToken::new(controller_fencing_token).unwrap(),
        non_zero(17),
        non_zero(claim_revision),
        at(130),
        RuntimeDrainClaimProgressV2::claimed(seal),
    )
    .unwrap()
}

fn acknowledged(
    operation: &RuntimeProductDrainOperationV2,
    certification: RuntimeDrainCertificationResolutionV2,
) -> RuntimeDrainIntentV2 {
    let key = &operation.canonical().drain_preimage().key;
    let acknowledgement = RuntimeRouteAbsentAcknowledgementV2::new(
        key,
        claim(operation, false),
        None,
        ordinary_provenance(),
        non_zero(22),
        certification,
        at(140),
    )
    .unwrap();
    RuntimeDrainIntentV2::route_absent_acknowledged_from_persisted(
        &root(operation),
        non_zero(3),
        acknowledgement,
    )
    .unwrap()
}

fn serving_identity() -> RuntimeServingIdentityV2 {
    RuntimeServingIdentityV2 {
        scope: crate::RuntimeDeploymentScopeV1::from_identity(&identity()),
        operation_id: RuntimeCertificationOperationIdV2::parse("11112222333344445555666677778888")
            .unwrap(),
        attestation_digest: RuntimeLiveAttestationDigestV2::parse("e".repeat(64)).unwrap(),
        process_identity: RuntimeProcessIdentityV1 {
            target: target(),
            runtime_generation: RuntimeGeneration::new(6).unwrap(),
            process_instance_id: process_id(),
        },
        lease_epoch: non_zero(23),
        revision: non_zero(24),
    }
}

fn acknowledged_committed(operation: &RuntimeProductDrainOperationV2) -> RuntimeDrainIntentV2 {
    let key = &operation.canonical().drain_preimage().key;
    let claim = claim(operation, false);
    let serving = serving_identity();
    let certification = RuntimeDrainCertificationResolutionV2::committed_and_disconnected(
        key,
        &claim,
        serving.operation_id.clone(),
        serving,
        non_zero(25),
    )
    .unwrap();
    let acknowledgement = RuntimeRouteAbsentAcknowledgementV2::new(
        key,
        claim,
        None,
        ordinary_provenance(),
        non_zero(22),
        certification,
        at(140),
    )
    .unwrap();
    RuntimeDrainIntentV2::route_absent_acknowledged_from_persisted(
        &root(operation),
        non_zero(3),
        acknowledgement,
    )
    .unwrap()
}

fn closed_recovery_witness() -> RuntimeClosedRecoveryRouteWitnessV2 {
    RuntimeClosedRecoveryRouteWitnessV2 {
        recovery_id: RuntimeRecoveryIdV2::parse("22223333444455556666777788889999").unwrap(),
        originating_emergency_generation: non_zero(30),
        recovery_generation: non_zero(31),
        recovery_authority_revision: non_zero(32),
        gateway_owner_lease_id: owner(),
        observed_owner_revision: non_zero(16),
        owner_expires_at: at(500),
        process_instance_id: process_id(),
        connection_epoch: non_zero(33),
        paused_admission_revision: non_zero(34),
        connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(35)),
        pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(36)),
    }
}

fn shutdown_witness() -> RuntimeShutdownRouteWitnessV2 {
    RuntimeShutdownRouteWitnessV2 {
        shutdown_generation: non_zero(38),
        gateway_owner_lease_id: owner(),
        observed_owner_revision: non_zero(16),
        owner_expires_at: at(500),
        process_instance_id: process_id(),
        connection_epoch: non_zero(33),
        paused_admission_revision: non_zero(34),
        connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(35)),
        pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(36)),
    }
}

fn routed_claim_input() -> RuntimeRoutedPendingDrainClaimInputV2 {
    RuntimeRoutedPendingDrainClaimInputV2 {
        gateway_owner_lease_id: owner(),
        observed_owner_revision: non_zero(16),
        controller_id: ControllerId::parse("controller:1").unwrap(),
        claim_epoch: non_zero(17),
        claim_expires_at: at(500),
        seal_generation: non_zero(14),
        seal_observation_sequence: non_zero(15),
        expected_route: route(19),
    }
}

fn routed_refence_input(
    provenance: RuntimeRouteMutationProvenanceV2,
) -> RuntimeRoutedPendingDrainRefenceInputV2 {
    RuntimeRoutedPendingDrainRefenceInputV2 {
        provenance,
        old_route: route(19),
        removal_target: route(20),
        registry_observation_sequence: non_zero(21),
        refenced_at: at(110),
    }
}

fn same_process_acknowledgement_input(
    provenance: RuntimeRouteMutationProvenanceV2,
) -> RuntimeSameProcessRefencedDrainAcknowledgementInputV2 {
    RuntimeSameProcessRefencedDrainAcknowledgementInputV2 {
        removed_route: route(20),
        provenance,
        registry_observation_sequence: non_zero(22),
        certification: RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
        acknowledged_at: at(120),
    }
}

fn succession_recovery_witness() -> RuntimeClosedRecoveryRouteWitnessV2 {
    RuntimeClosedRecoveryRouteWitnessV2 {
        recovery_id: RuntimeRecoveryIdV2::parse("3333444455556666777788889999aaaa").unwrap(),
        originating_emergency_generation: non_zero(40),
        recovery_generation: non_zero(41),
        recovery_authority_revision: non_zero(42),
        gateway_owner_lease_id: successor_owner(),
        observed_owner_revision: non_zero(43),
        owner_expires_at: at(500),
        process_instance_id: successor_process_id(),
        connection_epoch: non_zero(44),
        paused_admission_revision: non_zero(45),
        connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(46)),
        pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(47)),
    }
}

fn claim_input() -> RuntimeClosedRecoveryEmptyRegistryPendingDrainClaimInputV2 {
    RuntimeClosedRecoveryEmptyRegistryPendingDrainClaimInputV2 {
        recovery_witness: closed_recovery_witness(),
        controller_id: ControllerId::parse("controller:1").unwrap(),
        controller_fencing_token: FencingToken::new(20).unwrap(),
        claim_epoch: non_zero(17),
        claim_revision: non_zero(18),
        claim_expires_at: at(500),
        seal_generation: non_zero(14),
        seal_observation_sequence: non_zero(15),
    }
}

fn acknowledgement_input() -> RuntimeClosedRecoveryPendingDrainAcknowledgementInputV2 {
    RuntimeClosedRecoveryPendingDrainAcknowledgementInputV2 {
        acknowledgement_observation_sequence: non_zero(37),
        certification: RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
        acknowledged_at: at(400),
        recovery_witness: closed_recovery_witness(),
    }
}

fn succession_input() -> RuntimeClosedRecoveryPendingDrainSuccessionAcknowledgementInputV2 {
    RuntimeClosedRecoveryPendingDrainSuccessionAcknowledgementInputV2 {
        database_now: at(130),
        recovery_witness: succession_recovery_witness(),
        controller_id: ControllerId::parse("controller:2").unwrap(),
        seal_generation: non_zero(48),
        seal_observation_sequence: non_zero(100),
        acknowledgement_observation_sequence: non_zero(1),
        certification: RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
        acknowledged_at: at(131),
    }
}

fn persisted_unclaimed(
    root: &RuntimePersistedProductDrainRootV2,
    revision: NonZeroU64,
) -> RuntimePersistedUnclaimedPendingDrainIntentV2 {
    let intent = RuntimeDrainIntentV2::pending_from_persisted(root, revision, None).unwrap();
    let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(intent).unwrap();
    RuntimePersistedUnclaimedPendingDrainIntentV2::from_persisted(
        root,
        revision,
        "pending",
        canonical.state_bytes(),
    )
    .unwrap()
}

fn persisted_routed_claimed(
    operation: &RuntimeProductDrainOperationV2,
    transition: &RuntimeRoutedPendingDrainClaimTransitionV2,
) -> RuntimePersistedRoutedClaimedPendingDrainIntentV2 {
    RuntimePersistedRoutedClaimedPendingDrainIntentV2::from_persisted(
        &root(operation),
        transition.result().intent().intent_revision(),
        "pending",
        transition.result().state_bytes(),
    )
    .unwrap()
}

fn persisted_refenced(
    operation: &RuntimeProductDrainOperationV2,
    transition: &RuntimeRoutedPendingDrainRefenceTransitionV2,
) -> RuntimePersistedRefencedPendingDrainIntentV2 {
    RuntimePersistedRefencedPendingDrainIntentV2::from_persisted(
        &root(operation),
        transition.result().intent().intent_revision(),
        "pending",
        transition.result().state_bytes(),
    )
    .unwrap()
}

fn persisted_route_absent_claimed(
    operation: &RuntimeProductDrainOperationV2,
    revision: NonZeroU64,
    claim: RuntimeDrainClaimV2,
) -> RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2 {
    let persisted_root = root(operation);
    let intent =
        RuntimeDrainIntentV2::pending_from_persisted(&persisted_root, revision, Some(claim))
            .unwrap();
    let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(intent).unwrap();
    RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2::from_persisted(
        &persisted_root,
        revision,
        "pending",
        canonical.state_bytes(),
    )
    .unwrap()
}

fn build_succession(
    operation: &RuntimeProductDrainOperationV2,
    revision: NonZeroU64,
    claim: RuntimeDrainClaimV2,
    input: RuntimeClosedRecoveryPendingDrainSuccessionAcknowledgementInputV2,
) -> Result<
    RuntimeClosedRecoveryPendingDrainSuccessionAcknowledgementTransitionV2,
    RuntimeDrainIntentCanonicalStateErrorV2,
> {
    RuntimeClosedRecoveryPendingDrainSuccessionAcknowledgementTransitionV2::build(
        persisted_route_absent_claimed(operation, revision, claim),
        input,
    )
}

fn compact_source_digest(
    operation: &RuntimeProductDrainOperationV2,
    revision: NonZeroU64,
    predecessor: RuntimeDrainClaimV2,
) -> [u8; 32] {
    let persisted_root = root(operation);
    let intent =
        RuntimeDrainIntentV2::pending_from_persisted(&persisted_root, revision, Some(predecessor))
            .unwrap();
    let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(intent).unwrap();
    Sha256::digest(canonical.state_bytes()).into()
}

fn compact_predecessor_claim_source_digest(
    operation: &RuntimeProductDrainOperationV2,
    source_revision: NonZeroU64,
) -> [u8; 32] {
    let predecessor_revision =
        NonZeroU64::new(source_revision.get().checked_sub(1).unwrap()).unwrap();
    let intent =
        RuntimeDrainIntentV2::pending_from_persisted(&root(operation), predecessor_revision, None)
            .unwrap();
    let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(intent).unwrap();
    Sha256::digest(canonical.state_bytes()).into()
}

fn validate_compact_succession(
    operation: &RuntimeProductDrainOperationV2,
    source_revision: NonZeroU64,
    source_digest: [u8; 32],
    predecessor: &RuntimeDrainClaimV2,
    succession: &RuntimeClosedRecoveryPendingDrainSuccessionAcknowledgementInputV2,
    successor_bytes: &[u8],
) -> Result<
    super::RuntimeValidatedCompactPendingDrainSuccessionV2,
    RuntimeCompactPendingDrainSuccessionValidationErrorV2,
> {
    validate_compact_pending_drain_succession_v2(
        RuntimeCompactPendingDrainSuccessionValidationInputV2 {
            source_intent_revision: source_revision,
            source_state_digest: source_digest,
            predecessor_claim_source_digest: compact_predecessor_claim_source_digest(
                operation,
                source_revision,
            ),
            predecessor_claim: predecessor,
            succession,
            successor_state_bytes: successor_bytes,
        },
    )
}

fn compact_successor_claim(
    operation: &RuntimeProductDrainOperationV2,
    succession: &RuntimeClosedRecoveryPendingDrainSuccessionAcknowledgementInputV2,
    owner_revision: u64,
    claim_revision: u64,
    fence: u64,
    seal_generation: u64,
    seal_observation_sequence: u64,
) -> RuntimeDrainClaimV2 {
    let key = &operation.canonical().drain_preimage().key;
    let witness = &succession.recovery_witness;
    let seal = RuntimeDrainClaimSealWitnessV2::new(
        key,
        witness.process_instance_id.clone(),
        non_zero(seal_generation),
        None,
        non_zero(seal_observation_sequence),
    )
    .unwrap();
    RuntimeDrainClaimV2::new(
        key,
        witness.gateway_owner_lease_id.clone(),
        non_zero(owner_revision),
        witness.process_instance_id.clone(),
        succession.controller_id.clone(),
        FencingToken::new(fence).unwrap(),
        witness.recovery_generation,
        non_zero(claim_revision),
        witness.owner_expires_at,
        RuntimeDrainClaimProgressV2::claimed(seal),
    )
    .unwrap()
}

fn compact_successor_bytes(
    operation: &RuntimeProductDrainOperationV2,
    revision: NonZeroU64,
    claim: RuntimeDrainClaimV2,
    provenance: RuntimeRouteMutationProvenanceV2,
    certification: RuntimeDrainCertificationResolutionV2,
    acknowledged_at: DateTime<Utc>,
) -> Vec<u8> {
    let key = &operation.canonical().drain_preimage().key;
    let acknowledgement = RuntimeRouteAbsentAcknowledgementV2::new(
        key,
        claim,
        None,
        provenance,
        non_zero(1),
        certification,
        acknowledged_at,
    )
    .unwrap();
    let intent = RuntimeDrainIntentV2::route_absent_acknowledged_from_persisted(
        &root(operation),
        revision,
        acknowledgement,
    )
    .unwrap();
    RuntimeCanonicalDrainIntentStateV2::from_intent(intent)
        .unwrap()
        .state_bytes()
        .to_vec()
}

fn assert_roundtrip(
    root: &RuntimePersistedProductDrainRootV2,
    intent: RuntimeDrainIntentV2,
    expected_kind: RuntimeDrainIntentCanonicalStateKindV2,
) {
    let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(intent.clone()).unwrap();
    assert_eq!(canonical.state_kind().unwrap(), expected_kind);
    let restored = RuntimeCanonicalDrainIntentStateV2::from_persisted(
        root,
        intent.intent_revision(),
        canonical.persisted_state().unwrap(),
        canonical.state_bytes(),
    )
    .unwrap();
    assert_eq!(restored.intent(), &intent);
    assert_eq!(restored.state_bytes(), canonical.state_bytes());
    assert_eq!(restored.state_kind().unwrap(), expected_kind);
}

#[test]
fn all_state_and_pending_progress_variants_roundtrip_exactly() {
    let operation = operation();
    let root = root(&operation);
    let unclaimed = RuntimeDrainIntentV2::pending_from_persisted(&root, non_zero(1), None).unwrap();
    let claimed = RuntimeDrainIntentV2::pending_from_persisted(
        &root,
        non_zero(2),
        Some(claim(&operation, false)),
    )
    .unwrap();
    let refenced = RuntimeDrainIntentV2::pending_from_persisted(
        &root,
        non_zero(3),
        Some(claim(&operation, true)),
    )
    .unwrap();
    let no_operation = acknowledged(
        &operation,
        RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
    );
    let no_attestation = acknowledged(
        &operation,
        RuntimeDrainCertificationResolutionV2::no_attestation_for_reserved_operation(
            RuntimeCertificationOperationIdV2::parse("11112222333344445555666677778888").unwrap(),
            RuntimeCertificationIntentFingerprintV2::parse("f".repeat(64)).unwrap(),
        ),
    );
    let committed = acknowledged_committed(&operation);
    let consumed = RuntimeDrainIntentV2::consumed_from_persisted(
        &root,
        non_zero(4),
        DeploymentRevision::new(5).unwrap(),
        DateTime::from_timestamp_micros(-1).unwrap(),
    )
    .unwrap();
    let cancelled = RuntimeDrainIntentV2::cancelled_from_persisted(
        &root,
        non_zero(5),
        DateTime::from_timestamp_micros(400_000_001).unwrap(),
    )
    .unwrap();

    for (intent, kind) in [
        (
            unclaimed,
            RuntimeDrainIntentCanonicalStateKindV2::PendingUnclaimed,
        ),
        (
            claimed,
            RuntimeDrainIntentCanonicalStateKindV2::PendingClaimed,
        ),
        (
            refenced,
            RuntimeDrainIntentCanonicalStateKindV2::PendingRefenced,
        ),
        (
            no_operation,
            RuntimeDrainIntentCanonicalStateKindV2::RouteAbsentAcknowledged,
        ),
        (
            no_attestation,
            RuntimeDrainIntentCanonicalStateKindV2::RouteAbsentAcknowledged,
        ),
        (
            committed,
            RuntimeDrainIntentCanonicalStateKindV2::RouteAbsentAcknowledged,
        ),
        (consumed, RuntimeDrainIntentCanonicalStateKindV2::Consumed),
        (cancelled, RuntimeDrainIntentCanonicalStateKindV2::Cancelled),
    ] {
        assert_roundtrip(&root, intent, kind);
    }
}

#[test]
fn simple_pending_encoding_is_a_fixed_order_golden() {
    let operation = operation();
    let root = root(&operation);
    let intent = RuntimeDrainIntentV2::pending_from_persisted(&root, non_zero(1), None).unwrap();
    let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(intent).unwrap();
    let encoded = std::str::from_utf8(canonical.state_bytes()).unwrap();

    assert_eq!(
        encoded,
        concat!(
            "{\"format_version\":2,\"root\":{\"key\":{\"intent_id\":\"ffeeddccbbaa99887766554433221100\",",
            "\"product_operation_id\":\"00112233445566778899aabbccddeeff\",",
            "\"product_mutation_digest\":\"a6efc3c0d5db217271b68c3a86656b2ed4858d50e8fea4382829ee3a07351f26\",",
            "\"scope\":{\"tenant_id\":\"tenant:1\",\"installation_id\":\"installation:1\",\"deployment_id\":\"deployment:1\"},",
            "\"expected_revision\":1,\"slot\":{\"guild_id\":\"7\",\"ruleset_key\":\"studyroom\"},",
            "\"expected_target\":{\"guild_id\":\"7\",\"ruleset_key\":\"studyroom\",\"version\":1,",
            "\"content_hash\":\"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\",",
            "\"binding_revision\":3,\"binding_fingerprint\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"},",
            "\"mutation_kind\":\"authority_change\"},",
            "\"drain_intent_digest\":\"2a7e31070942aa83336441e6a944b07f6167803409e38d0d369ef69df524052d\"},",
            "\"intent_revision\":1,\"state\":{\"kind\":\"pending_unclaimed\"}}"
        )
    );
}

#[test]
fn decoder_rejects_unknown_noncanonical_and_mismatched_root_state() {
    let operation = operation();
    let root = root(&operation);
    let intent = RuntimeDrainIntentV2::pending_from_persisted(&root, non_zero(1), None).unwrap();
    let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(intent).unwrap();
    let encoded = std::str::from_utf8(canonical.state_bytes()).unwrap();

    let unknown = encoded.replacen(
        "{\"format_version\":2,",
        "{\"format_version\":2,\"unknown\":true,",
        1,
    );
    assert_eq!(
        RuntimeCanonicalDrainIntentStateV2::from_persisted(
            &root,
            non_zero(1),
            "pending",
            unknown.as_bytes(),
        ),
        Err(RuntimeDrainIntentCanonicalStateErrorV2::Decoding)
    );

    let spaced = format!(" {encoded}");
    assert_eq!(
        RuntimeCanonicalDrainIntentStateV2::from_persisted(
            &root,
            non_zero(1),
            "pending",
            spaced.as_bytes(),
        ),
        Err(RuntimeDrainIntentCanonicalStateErrorV2::NonCanonicalEncoding)
    );

    let wrong_digest = encoded.replace(root.drain_intent_digest().as_str(), &"0".repeat(64));
    assert_eq!(
        RuntimeCanonicalDrainIntentStateV2::from_persisted(
            &root,
            non_zero(1),
            "pending",
            wrong_digest.as_bytes(),
        ),
        Err(
            RuntimeDrainIntentCanonicalStateErrorV2::CorrelationMismatch {
                field: RuntimeDrainIntentCanonicalStateCorrelationV2::ImmutableRoot,
            }
        )
    );

    assert_eq!(
        RuntimeCanonicalDrainIntentStateV2::from_persisted(
            &root,
            non_zero(1),
            "consumed",
            canonical.state_bytes(),
        ),
        Err(
            RuntimeDrainIntentCanonicalStateErrorV2::CorrelationMismatch {
                field: RuntimeDrainIntentCanonicalStateCorrelationV2::PersistedState,
            }
        )
    );
}

#[test]
fn pending_subtype_and_nested_provenance_corruption_fail_closed() {
    let operation = operation();
    let root = root(&operation);
    let claimed = RuntimeDrainIntentV2::pending_from_persisted(
        &root,
        non_zero(2),
        Some(claim(&operation, false)),
    )
    .unwrap();
    let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(claimed).unwrap();
    let refenced_tag = std::str::from_utf8(canonical.state_bytes())
        .unwrap()
        .replace("\"pending_claimed\"", "\"pending_refenced\"");
    assert_eq!(
        RuntimeCanonicalDrainIntentStateV2::from_persisted(
            &root,
            non_zero(2),
            "pending",
            refenced_tag.as_bytes(),
        ),
        Err(
            RuntimeDrainIntentCanonicalStateErrorV2::CorrelationMismatch {
                field: RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress,
            }
        )
    );

    let refenced = RuntimeDrainIntentV2::pending_from_persisted(
        &root,
        non_zero(3),
        Some(claim(&operation, true)),
    )
    .unwrap();
    let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(refenced).unwrap();
    let corrupted = std::str::from_utf8(canonical.state_bytes())
        .unwrap()
        .replace("\\\"ordinary\\\"", "\\\"unknown\\\"");
    assert!(RuntimeCanonicalDrainIntentStateV2::from_persisted(
        &root,
        non_zero(3),
        "pending",
        corrupted.as_bytes(),
    )
    .is_err());
}

#[test]
fn payload_limit_matches_the_one_mebibyte_execution_frame_cap() {
    let operation = operation();
    let root = root(&operation);
    let oversized = vec![b' '; 1_048_577];
    assert_eq!(
        RuntimeCanonicalDrainIntentStateV2::from_persisted(
            &root,
            non_zero(1),
            "pending",
            &oversized,
        ),
        Err(RuntimeDrainIntentCanonicalStateErrorV2::PayloadTooLarge)
    );
}

#[test]
fn routed_claim_refence_and_acknowledgement_are_exact_canonical_successors() {
    let operation = operation();
    let persisted_root = root(&operation);
    let claim_transition = RuntimeRoutedPendingDrainClaimTransitionV2::build(
        persisted_unclaimed(&persisted_root, non_zero(1)),
        routed_claim_input(),
    )
    .unwrap();
    let claimed = claim_transition
        .result()
        .intent()
        .state()
        .pending_claim()
        .unwrap();

    assert_eq!(
        claim_transition.result().intent().intent_revision(),
        non_zero(2)
    );
    assert_eq!(claimed.claim_revision(), NonZeroU64::MIN);
    assert_eq!(
        claimed.controller_fencing_token(),
        FencingToken::new(20).unwrap()
    );
    assert_eq!(claimed.progress().seal().expected_route(), Some(&route(19)));

    let refence_transition = RuntimeRoutedPendingDrainRefenceTransitionV2::build(
        persisted_routed_claimed(&operation, &claim_transition),
        routed_refence_input(RuntimeRouteMutationProvenanceV2::ClosedRecovery(
            closed_recovery_witness(),
        )),
    )
    .unwrap();
    let refenced = refence_transition
        .result()
        .intent()
        .state()
        .pending_claim()
        .unwrap();

    assert_eq!(
        refence_transition.result().intent().intent_revision(),
        non_zero(3)
    );
    assert_eq!(refenced.claim_revision(), non_zero(2));
    assert_eq!(
        refenced.controller_fencing_token(),
        claimed.controller_fencing_token()
    );
    assert_eq!(refenced.progress().seal(), claimed.progress().seal());
    assert_eq!(refenced.progress().old_route(), Some(&route(19)));
    assert_eq!(refenced.progress().removal_target(), Some(&route(20)));

    let acknowledgement_transition =
        RuntimeSameProcessRefencedDrainAcknowledgementTransitionV2::build(
            persisted_refenced(&operation, &refence_transition),
            same_process_acknowledgement_input(RuntimeRouteMutationProvenanceV2::ClosedRecovery(
                closed_recovery_witness(),
            )),
        )
        .unwrap();
    let acknowledgement = acknowledgement_transition
        .result()
        .intent()
        .state()
        .acknowledgement()
        .unwrap();

    assert_eq!(
        acknowledgement_transition
            .result()
            .intent()
            .intent_revision(),
        non_zero(4)
    );
    assert_eq!(acknowledgement.claim(), refenced);
    assert_eq!(acknowledgement.expected_route(), Some(&route(20)));
    assert_eq!(
        (
            format!(
                "{:x}",
                Sha256::digest(claim_transition.result().state_bytes())
            ),
            format!(
                "{:x}",
                Sha256::digest(refence_transition.result().state_bytes())
            ),
            format!(
                "{:x}",
                Sha256::digest(acknowledgement_transition.result().state_bytes())
            ),
        ),
        (
            "9d462f6113830ef927aba5fcdbf7bcaf4e0e896e8e3984f4af5672ef95ff7cb2".to_string(),
            "92887e9c75386bac771c4a531858be70249484efeb28e0401645ccaadba6a884".to_string(),
            "22146efe9ef92dd51a80a7005617dd8fabaf516a510b3aca4e708db02f7681c2".to_string(),
        )
    );

    for canonical in [
        claim_transition.result(),
        refence_transition.result(),
        acknowledgement_transition.result(),
    ] {
        let restored = RuntimeCanonicalDrainIntentStateV2::from_persisted(
            &persisted_root,
            canonical.intent().intent_revision(),
            canonical.persisted_state().unwrap(),
            canonical.state_bytes(),
        )
        .unwrap();
        assert_eq!(&restored, canonical);
    }
}

#[test]
fn routed_and_refenced_classifiers_require_exact_initial_lineage() {
    let operation = operation();
    let persisted_root = root(&operation);
    let classify_routed = |claim| {
        let intent =
            RuntimeDrainIntentV2::pending_from_persisted(&persisted_root, non_zero(2), Some(claim))
                .unwrap();
        let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(intent).unwrap();
        RuntimePersistedRoutedClaimedPendingDrainIntentV2::from_persisted(
            &persisted_root,
            non_zero(2),
            "pending",
            canonical.state_bytes(),
        )
    };
    let classify_refenced = |claim| {
        let intent =
            RuntimeDrainIntentV2::pending_from_persisted(&persisted_root, non_zero(3), Some(claim))
                .unwrap();
        let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(intent).unwrap();
        RuntimePersistedRefencedPendingDrainIntentV2::from_persisted(
            &persisted_root,
            non_zero(3),
            "pending",
            canonical.state_bytes(),
        )
    };

    assert!(classify_routed(routed_claim_with_numbers(&operation, 19, 20, 1)).is_ok());
    assert!(classify_routed(routed_claim_with_numbers(&operation, 19, 20, 2)).is_err());
    assert!(classify_routed(routed_claim_with_numbers(&operation, 19, 21, 1)).is_err());
    assert!(classify_refenced(refenced_claim_with_numbers(
        &operation,
        19,
        20,
        2,
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(closed_recovery_witness()),
    ))
    .is_ok());
    assert!(classify_refenced(refenced_claim_with_numbers(
        &operation,
        19,
        20,
        3,
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(closed_recovery_witness()),
    ))
    .is_err());
    assert!(classify_refenced(refenced_claim_with_numbers(
        &operation,
        19,
        21,
        2,
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(closed_recovery_witness()),
    ))
    .is_err());
    assert!(classify_refenced(refenced_claim_with_numbers(
        &operation,
        19,
        20,
        2,
        ordinary_provenance(),
    ))
    .is_err());
}

#[test]
fn same_process_transitions_reject_route_owner_provenance_and_observation_drift() {
    let operation = operation();
    let persisted_root = root(&operation);
    let unclaimed = || persisted_unclaimed(&persisted_root, non_zero(1));

    let mut foreign_owner = routed_claim_input();
    foreign_owner.gateway_owner_lease_id.process_instance_id = successor_process_id();
    assert!(RuntimeRoutedPendingDrainClaimTransitionV2::build(unclaimed(), foreign_owner).is_err());

    let mut exhausted_route = routed_claim_input();
    exhausted_route.expected_route.controller_fencing_token =
        FencingToken::new(i64::MAX as u64).unwrap();
    assert!(
        RuntimeRoutedPendingDrainClaimTransitionV2::build(unclaimed(), exhausted_route).is_err()
    );

    let exhausted_intent = persisted_unclaimed(&persisted_root, non_zero(i64::MAX as u64));
    assert!(RuntimeRoutedPendingDrainClaimTransitionV2::build(
        exhausted_intent,
        routed_claim_input(),
    )
    .is_err());

    let claim_transition =
        RuntimeRoutedPendingDrainClaimTransitionV2::build(unclaimed(), routed_claim_input())
            .unwrap();
    let routed_source = || persisted_routed_claimed(&operation, &claim_transition);
    let routed_claim = claim_transition
        .result()
        .intent()
        .state()
        .pending_claim()
        .unwrap()
        .clone();
    let exhausted_routed_intent = RuntimeDrainIntentV2::pending_from_persisted(
        &persisted_root,
        non_zero(i64::MAX as u64),
        Some(routed_claim),
    )
    .unwrap();
    let exhausted_routed =
        RuntimeCanonicalDrainIntentStateV2::from_intent(exhausted_routed_intent).unwrap();
    let exhausted_routed_source =
        RuntimePersistedRoutedClaimedPendingDrainIntentV2::from_persisted(
            &persisted_root,
            non_zero(i64::MAX as u64),
            "pending",
            exhausted_routed.state_bytes(),
        )
        .unwrap();
    assert!(RuntimeRoutedPendingDrainRefenceTransitionV2::build(
        exhausted_routed_source,
        routed_refence_input(RuntimeRouteMutationProvenanceV2::ClosedRecovery(
            closed_recovery_witness(),
        )),
    )
    .is_err());

    let mut wrong_old_route = routed_refence_input(
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(closed_recovery_witness()),
    );
    wrong_old_route.old_route = route(18);
    assert!(
        RuntimeRoutedPendingDrainRefenceTransitionV2::build(routed_source(), wrong_old_route,)
            .is_err()
    );

    let mut skipped_fence = routed_refence_input(RuntimeRouteMutationProvenanceV2::ClosedRecovery(
        closed_recovery_witness(),
    ));
    skipped_fence.removal_target = route(21);
    assert!(
        RuntimeRoutedPendingDrainRefenceTransitionV2::build(routed_source(), skipped_fence)
            .is_err()
    );

    let mut wrong_incarnation = routed_refence_input(
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(closed_recovery_witness()),
    );
    wrong_incarnation.removal_target.route_incarnation = non_zero(9);
    assert!(RuntimeRoutedPendingDrainRefenceTransitionV2::build(
        routed_source(),
        wrong_incarnation,
    )
    .is_err());

    let mut stale_observation = routed_refence_input(
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(closed_recovery_witness()),
    );
    stale_observation.registry_observation_sequence = non_zero(15);
    assert!(RuntimeRoutedPendingDrainRefenceTransitionV2::build(
        routed_source(),
        stale_observation,
    )
    .is_err());

    assert!(RuntimeRoutedPendingDrainRefenceTransitionV2::build(
        routed_source(),
        routed_refence_input(ordinary_provenance()),
    )
    .is_err());

    let mut owner_drift = closed_recovery_witness();
    owner_drift.observed_owner_revision = non_zero(99);
    assert!(RuntimeRoutedPendingDrainRefenceTransitionV2::build(
        routed_source(),
        routed_refence_input(RuntimeRouteMutationProvenanceV2::ClosedRecovery(
            owner_drift
        )),
    )
    .is_err());

    let shutdown_refence = RuntimeRoutedPendingDrainRefenceTransitionV2::build(
        routed_source(),
        routed_refence_input(RuntimeRouteMutationProvenanceV2::Shutdown(
            shutdown_witness(),
        )),
    )
    .unwrap();
    let refenced_source = || persisted_refenced(&operation, &shutdown_refence);

    let mut wrong_removed_route = same_process_acknowledgement_input(
        RuntimeRouteMutationProvenanceV2::Shutdown(shutdown_witness()),
    );
    wrong_removed_route.removed_route = route(19);
    assert!(
        RuntimeSameProcessRefencedDrainAcknowledgementTransitionV2::build(
            refenced_source(),
            wrong_removed_route,
        )
        .is_err()
    );

    let mut stale_absence = same_process_acknowledgement_input(
        RuntimeRouteMutationProvenanceV2::Shutdown(shutdown_witness()),
    );
    stale_absence.registry_observation_sequence = non_zero(21);
    assert!(
        RuntimeSameProcessRefencedDrainAcknowledgementTransitionV2::build(
            refenced_source(),
            stale_absence,
        )
        .is_err()
    );

    assert!(
        RuntimeSameProcessRefencedDrainAcknowledgementTransitionV2::build(
            refenced_source(),
            same_process_acknowledgement_input(ordinary_provenance()),
        )
        .is_err()
    );

    let mut acknowledgement_owner_drift = shutdown_witness();
    acknowledgement_owner_drift.observed_owner_revision = non_zero(99);
    assert!(
        RuntimeSameProcessRefencedDrainAcknowledgementTransitionV2::build(
            refenced_source(),
            same_process_acknowledgement_input(RuntimeRouteMutationProvenanceV2::Shutdown(
                acknowledgement_owner_drift,
            )),
        )
        .is_err()
    );

    let refenced_claim = shutdown_refence
        .result()
        .intent()
        .state()
        .pending_claim()
        .unwrap()
        .clone();
    let exhausted_refenced_intent = RuntimeDrainIntentV2::pending_from_persisted(
        &persisted_root,
        non_zero(i64::MAX as u64),
        Some(refenced_claim),
    )
    .unwrap();
    let exhausted_refenced =
        RuntimeCanonicalDrainIntentStateV2::from_intent(exhausted_refenced_intent).unwrap();
    let exhausted_refenced_source = RuntimePersistedRefencedPendingDrainIntentV2::from_persisted(
        &persisted_root,
        non_zero(i64::MAX as u64),
        "pending",
        exhausted_refenced.state_bytes(),
    )
    .unwrap();
    assert!(
        RuntimeSameProcessRefencedDrainAcknowledgementTransitionV2::build(
            exhausted_refenced_source,
            same_process_acknowledgement_input(RuntimeRouteMutationProvenanceV2::Shutdown(
                shutdown_witness(),
            )),
        )
        .is_err()
    );
}

#[test]
fn unclaimed_closed_recovery_requires_two_persisted_cas_transitions() {
    let operation = operation();
    let root = root(&operation);
    let unclaimed = persisted_unclaimed(&root, non_zero(1));
    let claim_transition = RuntimeClosedRecoveryEmptyRegistryPendingDrainClaimTransitionV2::build(
        unclaimed,
        claim_input(),
    )
    .unwrap();

    assert_eq!(
        claim_transition.result().state_kind().unwrap(),
        RuntimeDrainIntentCanonicalStateKindV2::PendingClaimed
    );
    assert_eq!(
        claim_transition.result().intent().intent_revision(),
        non_zero(2)
    );

    assert_eq!(
        RuntimePersistedRouteAbsenceCandidateDrainIntentV2::from_persisted(
            &root,
            non_zero(1),
            "pending",
            RuntimeCanonicalDrainIntentStateV2::from_intent(
                RuntimeDrainIntentV2::pending_from_persisted(&root, non_zero(1), None).unwrap(),
            )
            .unwrap()
            .state_bytes(),
        ),
        Err(
            RuntimeDrainIntentCanonicalStateErrorV2::CorrelationMismatch {
                field: RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress,
            }
        )
    );

    let persisted_claim = RuntimePersistedRouteAbsenceCandidateDrainIntentV2::from_persisted(
        &root,
        non_zero(2),
        "pending",
        claim_transition.result().state_bytes(),
    )
    .unwrap();
    let acknowledgement_transition =
        RuntimeClosedRecoveryPendingDrainAcknowledgementTransitionV2::build(
            persisted_claim,
            acknowledgement_input(),
        )
        .unwrap();

    assert_eq!(
        acknowledgement_transition.result().state_kind().unwrap(),
        RuntimeDrainIntentCanonicalStateKindV2::RouteAbsentAcknowledged
    );
    assert_eq!(
        acknowledgement_transition
            .result()
            .intent()
            .intent_revision(),
        non_zero(3)
    );
    let acknowledged_claim = acknowledgement_transition
        .result()
        .intent()
        .state()
        .acknowledgement()
        .unwrap()
        .claim();
    assert_eq!(
        Some(acknowledged_claim),
        acknowledgement_transition
            .source()
            .canonical()
            .intent()
            .state()
            .pending_claim()
    );
    assert!(acknowledged_claim
        .progress()
        .seal()
        .expected_route()
        .is_none());
    assert!(acknowledgement_transition
        .result()
        .intent()
        .state()
        .acknowledgement()
        .unwrap()
        .expected_route()
        .is_none());
}

#[test]
fn persisted_refenced_candidate_acknowledges_the_exact_removal_target() {
    let operation = operation();
    let root = root(&operation);
    let refenced = RuntimeDrainIntentV2::pending_from_persisted(
        &root,
        non_zero(7),
        Some(claim(&operation, true)),
    )
    .unwrap();
    let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(refenced).unwrap();
    let persisted = RuntimePersistedRouteAbsenceCandidateDrainIntentV2::from_persisted(
        &root,
        non_zero(7),
        "pending",
        canonical.state_bytes(),
    )
    .unwrap();
    let transition = RuntimeClosedRecoveryPendingDrainAcknowledgementTransitionV2::build(
        persisted,
        acknowledgement_input(),
    )
    .unwrap();

    assert_eq!(transition.result().intent().intent_revision(), non_zero(8));
    let source_claim = transition
        .source()
        .canonical()
        .intent()
        .state()
        .pending_claim()
        .unwrap();
    let acknowledgement = transition
        .result()
        .intent()
        .state()
        .acknowledgement()
        .unwrap();
    assert_eq!(acknowledgement.claim(), source_claim);
    assert_eq!(
        acknowledgement.expected_route(),
        source_claim.progress().removal_target()
    );
}

#[test]
fn routed_claimed_state_is_not_a_route_absence_candidate() {
    let operation = operation();
    let root = root(&operation);
    let routed = RuntimeDrainIntentV2::pending_from_persisted(
        &root,
        non_zero(7),
        Some(routed_claimed(&operation)),
    )
    .unwrap();
    let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(routed).unwrap();

    assert_eq!(
        RuntimePersistedRouteAbsenceCandidateDrainIntentV2::from_persisted(
            &root,
            non_zero(7),
            "pending",
            canonical.state_bytes(),
        ),
        Err(
            RuntimeDrainIntentCanonicalStateErrorV2::CorrelationMismatch {
                field: RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress,
            }
        )
    );
}

#[test]
fn closed_recovery_builder_rejects_owner_drift_and_revision_overflow() {
    let operation = operation();
    let root = root(&operation);
    let unclaimed = persisted_unclaimed(&root, non_zero(1));
    let claim_transition = RuntimeClosedRecoveryEmptyRegistryPendingDrainClaimTransitionV2::build(
        unclaimed,
        claim_input(),
    )
    .unwrap();
    let persisted_claim = RuntimePersistedRouteAbsenceCandidateDrainIntentV2::from_persisted(
        &root,
        non_zero(2),
        "pending",
        claim_transition.result().state_bytes(),
    )
    .unwrap();
    let mut input = acknowledgement_input();
    input.recovery_witness.observed_owner_revision = non_zero(99);
    assert!(
        RuntimeClosedRecoveryPendingDrainAcknowledgementTransitionV2::build(
            persisted_claim,
            input,
        )
        .is_err()
    );

    let exhausted = persisted_unclaimed(&root, non_zero(i64::MAX as u64));
    assert!(matches!(
        RuntimeClosedRecoveryEmptyRegistryPendingDrainClaimTransitionV2::build(
            exhausted,
            claim_input()
        ),
        Err(RuntimeDrainIntentCanonicalStateErrorV2::CanonicalValue { .. })
    ));
}

#[test]
fn expired_route_absent_claim_succeeds_directly_with_exact_current_evidence() {
    let operation = operation();
    let input = succession_input();
    let expected_witness = input.recovery_witness.clone();
    let transition = build_succession(
        &operation,
        non_zero(7),
        claim(&operation, false),
        input.clone(),
    )
    .unwrap();

    assert_eq!(
        transition.result().state_kind().unwrap(),
        RuntimeDrainIntentCanonicalStateKindV2::RouteAbsentAcknowledged
    );
    assert_eq!(transition.result().intent().intent_revision(), non_zero(8));
    assert_eq!(
        transition.result().intent().canonical(),
        transition.source().canonical().intent().canonical()
    );
    let predecessor = transition
        .source()
        .canonical()
        .intent()
        .state()
        .pending_claim()
        .unwrap();
    let acknowledgement = transition
        .result()
        .intent()
        .state()
        .acknowledgement()
        .unwrap();
    let successor = acknowledgement.claim();
    assert_ne!(successor, predecessor);
    assert_eq!(
        successor.gateway_owner_lease_id(),
        &expected_witness.gateway_owner_lease_id
    );
    assert_eq!(
        successor.observed_owner_revision(),
        expected_witness.observed_owner_revision
    );
    assert_eq!(
        successor.process_instance_id(),
        &expected_witness.process_instance_id
    );
    assert_eq!(
        successor.controller_id(),
        &ControllerId::parse("controller:2").unwrap()
    );
    assert_eq!(
        successor.controller_fencing_token(),
        FencingToken::new(21).unwrap()
    );
    assert_eq!(
        successor.claim_epoch(),
        expected_witness.recovery_generation
    );
    assert_eq!(successor.claim_revision(), non_zero(19));
    assert_eq!(successor.expires_at(), expected_witness.owner_expires_at);
    assert_eq!(successor.progress().seal().seal_generation(), non_zero(48));
    assert_eq!(
        successor.progress().seal().registry_observation_sequence(),
        non_zero(100)
    );
    assert!(successor.progress().seal().expected_route().is_none());
    assert_eq!(
        acknowledgement.provenance(),
        &RuntimeRouteMutationProvenanceV2::ClosedRecovery(expected_witness)
    );
    assert_eq!(acknowledgement.registry_observation_sequence(), non_zero(1));
    assert_eq!(acknowledgement.acknowledged_at(), at(131));
    let restored = RuntimeCanonicalDrainIntentStateV2::from_persisted(
        &root(&operation),
        non_zero(8),
        "route_absent_acknowledged",
        transition.result().state_bytes(),
    )
    .unwrap();
    assert_eq!(restored, transition.result().clone());
}

#[test]
fn succession_predecessor_classifier_accepts_only_route_absent_claimed() {
    let operation = operation();
    let persisted_root = root(&operation);
    let expected = Err(
        RuntimeDrainIntentCanonicalStateErrorV2::CorrelationMismatch {
            field: RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress,
        },
    );

    let unclaimed =
        RuntimeDrainIntentV2::pending_from_persisted(&persisted_root, non_zero(7), None).unwrap();
    let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(unclaimed).unwrap();
    assert_eq!(
        RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2::from_persisted(
            &persisted_root,
            non_zero(7),
            "pending",
            canonical.state_bytes(),
        ),
        expected
    );

    let routed = RuntimeDrainIntentV2::pending_from_persisted(
        &persisted_root,
        non_zero(7),
        Some(routed_claimed(&operation)),
    )
    .unwrap();
    let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(routed).unwrap();
    assert_eq!(
        RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2::from_persisted(
            &persisted_root,
            non_zero(7),
            "pending",
            canonical.state_bytes(),
        ),
        expected
    );

    let refenced = RuntimeDrainIntentV2::pending_from_persisted(
        &persisted_root,
        non_zero(7),
        Some(claim(&operation, true)),
    )
    .unwrap();
    let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(refenced).unwrap();
    assert_eq!(
        RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2::from_persisted(
            &persisted_root,
            non_zero(7),
            "pending",
            canonical.state_bytes(),
        ),
        expected
    );
}

#[test]
fn succession_requires_expired_predecessor_distinct_newer_current_owner() {
    let operation = operation();
    let predecessor = || claim(&operation, false);

    let mut fresh = succession_input();
    fresh.database_now = at(129);
    assert_eq!(
        build_succession(&operation, non_zero(7), predecessor(), fresh),
        Err(RuntimeDrainIntentCanonicalStateErrorV2::Receipt(
            RuntimeDrainIntentReceiptErrorV2::SuccessionPredecessorNotExpired
        ))
    );

    let mut noncanonical_database_time = succession_input();
    noncanonical_database_time.database_now = DateTime::from_timestamp(130, 1).unwrap();
    assert_eq!(
        build_succession(
            &operation,
            non_zero(7),
            predecessor(),
            noncanonical_database_time,
        ),
        Err(RuntimeDrainIntentCanonicalStateErrorV2::Receipt(
            RuntimeDrainIntentReceiptErrorV2::SuccessionDatabaseTimeInvalid
        ))
    );

    let mut same_process = succession_input();
    same_process.recovery_witness.process_instance_id = process_id();
    same_process
        .recovery_witness
        .gateway_owner_lease_id
        .process_instance_id = process_id();
    assert_eq!(
        build_succession(&operation, non_zero(7), predecessor(), same_process),
        Err(RuntimeDrainIntentCanonicalStateErrorV2::Receipt(
            RuntimeDrainIntentReceiptErrorV2::SuccessionProcessNotDistinct
        ))
    );

    let mut foreign_shard = succession_input();
    foreign_shard
        .recovery_witness
        .gateway_owner_lease_id
        .gateway_shard_id = GatewayShardIdV1::parse("shard:1").unwrap();
    assert_eq!(
        build_succession(&operation, non_zero(7), predecessor(), foreign_shard),
        Err(RuntimeDrainIntentCanonicalStateErrorV2::Receipt(
            RuntimeDrainIntentReceiptErrorV2::SuccessionShardMismatch
        ))
    );

    let mut stale_epoch = succession_input();
    stale_epoch
        .recovery_witness
        .gateway_owner_lease_id
        .lease_epoch = non_zero(3);
    assert_eq!(
        build_succession(&operation, non_zero(7), predecessor(), stale_epoch),
        Err(RuntimeDrainIntentCanonicalStateErrorV2::Receipt(
            RuntimeDrainIntentReceiptErrorV2::SuccessionOwnerEpochNotNewer
        ))
    );

    let mut expired_owner = succession_input();
    expired_owner.database_now = at(500);
    assert_eq!(
        build_succession(&operation, non_zero(7), predecessor(), expired_owner),
        Err(RuntimeDrainIntentCanonicalStateErrorV2::Receipt(
            RuntimeDrainIntentReceiptErrorV2::SuccessionOwnerExpired
        ))
    );

    let mut skipped_epoch = succession_input();
    skipped_epoch
        .recovery_witness
        .gateway_owner_lease_id
        .lease_epoch = non_zero(9);
    assert!(build_succession(&operation, non_zero(7), predecessor(), skipped_epoch).is_ok());
}

#[test]
fn succession_rejects_committed_certification_and_invalid_current_provenance() {
    let operation = operation();
    let mut no_attestation = succession_input();
    no_attestation.certification =
        RuntimeDrainCertificationResolutionV2::no_attestation_for_reserved_operation(
            RuntimeCertificationOperationIdV2::parse("444455556666777788889999aaaabbbb").unwrap(),
            RuntimeCertificationIntentFingerprintV2::parse("f".repeat(64)).unwrap(),
        );
    assert!(build_succession(
        &operation,
        non_zero(7),
        claim(&operation, false),
        no_attestation,
    )
    .is_ok());

    let mut committed = succession_input();
    committed.certification = acknowledged_committed(&operation)
        .state()
        .acknowledgement()
        .unwrap()
        .certification()
        .clone();
    assert_eq!(
        build_succession(&operation, non_zero(7), claim(&operation, false), committed,),
        Err(RuntimeDrainIntentCanonicalStateErrorV2::Receipt(
            RuntimeDrainIntentReceiptErrorV2::SuccessionCertificationMismatch
        ))
    );

    let mut invalid_provenance = succession_input();
    invalid_provenance
        .recovery_witness
        .gateway_owner_lease_id
        .process_instance_id = ProcessInstanceId::parse("process:3").unwrap();
    assert_eq!(
        build_succession(
            &operation,
            non_zero(7),
            claim(&operation, false),
            invalid_provenance,
        ),
        Err(RuntimeDrainIntentCanonicalStateErrorV2::Receipt(
            RuntimeDrainIntentReceiptErrorV2::SuccessionOwnerMismatch
        ))
    );

    let mut generation_drift = succession_input();
    generation_drift
        .recovery_witness
        .originating_emergency_generation = non_zero(39);
    assert_eq!(
        build_succession(
            &operation,
            non_zero(7),
            claim(&operation, false),
            generation_drift,
        ),
        Err(RuntimeDrainIntentCanonicalStateErrorV2::Receipt(
            RuntimeDrainIntentReceiptErrorV2::SuccessionRecoveryGenerationMismatch
        ))
    );

    let mut sequence_drift = succession_input();
    sequence_drift.recovery_witness.pause_sequence =
        sequence_drift.recovery_witness.connected_event_sequence;
    assert_eq!(
        build_succession(
            &operation,
            non_zero(7),
            claim(&operation, false),
            sequence_drift,
        ),
        Err(RuntimeDrainIntentCanonicalStateErrorV2::Receipt(
            RuntimeDrainIntentReceiptErrorV2::SuccessionPauseSequenceMismatch
        ))
    );
}

#[test]
fn succession_rejects_each_persistence_successor_overflow() {
    let operation = operation();
    let maximum = i64::MAX as u64;

    assert_eq!(
        build_succession(
            &operation,
            non_zero(maximum),
            claim(&operation, false),
            succession_input(),
        ),
        Err(RuntimeDrainIntentCanonicalStateErrorV2::CanonicalValue {
            field: RuntimeDrainIntentCanonicalStateFieldV2::IntentRevision,
            reason: crate::RuntimeCanonicalValueErrorV2::PersistenceIntegerOutOfRange,
        })
    );
    assert_eq!(
        build_succession(
            &operation,
            non_zero(7),
            route_absent_claim_with_numbers(&operation, maximum, 20),
            succession_input(),
        ),
        Err(RuntimeDrainIntentCanonicalStateErrorV2::CanonicalValue {
            field: RuntimeDrainIntentCanonicalStateFieldV2::ClaimRevision,
            reason: crate::RuntimeCanonicalValueErrorV2::PersistenceIntegerOutOfRange,
        })
    );
    assert_eq!(
        build_succession(
            &operation,
            non_zero(7),
            route_absent_claim_with_numbers(&operation, 18, maximum),
            succession_input(),
        ),
        Err(RuntimeDrainIntentCanonicalStateErrorV2::CanonicalValue {
            field: RuntimeDrainIntentCanonicalStateFieldV2::ControllerFencingToken,
            reason: crate::RuntimeCanonicalValueErrorV2::PersistenceIntegerOutOfRange,
        })
    );
}

#[test]
fn compact_succession_validator_returns_exact_canonical_result() {
    let operation = operation();
    let predecessor = claim(&operation, false);
    let succession = succession_input();
    let source_revision = non_zero(7);
    let source_digest = compact_source_digest(&operation, source_revision, predecessor.clone());
    let transition = build_succession(
        &operation,
        source_revision,
        predecessor.clone(),
        succession.clone(),
    )
    .unwrap();
    let successor_bytes = transition.result().state_bytes();
    let validated = validate_compact_succession(
        &operation,
        source_revision,
        source_digest,
        &predecessor,
        &succession,
        successor_bytes,
    )
    .unwrap();

    assert_eq!(validated.key(), &operation.canonical().drain_preimage().key);
    assert_eq!(
        validated.drain_intent_digest(),
        operation.drain_intent_digest()
    );
    assert_eq!(validated.successor_intent_revision(), non_zero(8));
    assert_eq!(
        validated.successor_state_digest(),
        &<[u8; 32]>::from(Sha256::digest(successor_bytes))
    );
    assert_eq!(
        validated.certification(),
        &RuntimeDrainCertificationResolutionV2::no_operation_reserved()
    );
}

#[test]
fn compact_succession_validator_rejects_source_chain_and_root_tampering() {
    let operation = operation();
    let predecessor = claim(&operation, false);
    let succession = succession_input();
    let source_revision = non_zero(7);
    let source_digest = compact_source_digest(&operation, source_revision, predecessor.clone());
    let transition = build_succession(
        &operation,
        source_revision,
        predecessor.clone(),
        succession.clone(),
    )
    .unwrap();
    let successor_bytes = transition.result().state_bytes().to_vec();

    let mut wrong_source_digest = source_digest;
    wrong_source_digest[0] ^= 1;
    assert_eq!(
        validate_compact_succession(
            &operation,
            source_revision,
            wrong_source_digest,
            &predecessor,
            &succession,
            &successor_bytes,
        ),
        Err(RuntimeCompactPendingDrainSuccessionValidationErrorV2::SourceStateDigestMismatch)
    );

    let mut wrong_predecessor_source_digest =
        compact_predecessor_claim_source_digest(&operation, source_revision);
    wrong_predecessor_source_digest[0] ^= 1;
    assert_eq!(
        validate_compact_pending_drain_succession_v2(
            RuntimeCompactPendingDrainSuccessionValidationInputV2 {
                source_intent_revision: source_revision,
                source_state_digest: source_digest,
                predecessor_claim_source_digest: wrong_predecessor_source_digest,
                predecessor_claim: &predecessor,
                succession: &succession,
                successor_state_bytes: &successor_bytes,
            },
        ),
        Err(
            RuntimeCompactPendingDrainSuccessionValidationErrorV2::PredecessorClaimSourceDigestMismatch
        )
    );

    assert_eq!(
        validate_compact_pending_drain_succession_v2(
            RuntimeCompactPendingDrainSuccessionValidationInputV2 {
                source_intent_revision: NonZeroU64::MIN,
                source_state_digest: source_digest,
                predecessor_claim_source_digest: [1; 32],
                predecessor_claim: &predecessor,
                succession: &succession,
                successor_state_bytes: &successor_bytes,
            },
        ),
        Err(
            RuntimeCompactPendingDrainSuccessionValidationErrorV2::PredecessorClaimSourceRevisionMissing
        )
    );

    let tampered_root = String::from_utf8(successor_bytes.clone())
        .unwrap()
        .replacen(
            "ffeeddccbbaa99887766554433221100",
            "00112233445566778899aabbccddeeff",
            1,
        )
        .into_bytes();
    assert!(validate_compact_succession(
        &operation,
        source_revision,
        source_digest,
        &predecessor,
        &succession,
        &tampered_root,
    )
    .is_err());

    let tampered_mutation_kind = String::from_utf8(successor_bytes)
        .unwrap()
        .replacen(
            "\"mutation_kind\":\"authority_change\"",
            "\"mutation_kind\":\"teardown\"",
            1,
        )
        .into_bytes();
    assert!(validate_compact_succession(
        &operation,
        source_revision,
        source_digest,
        &predecessor,
        &succession,
        &tampered_mutation_kind,
    )
    .is_err());
}

#[test]
fn compact_succession_validator_rejects_successor_encoding_and_revision_tampering() {
    let operation = operation();
    let predecessor = claim(&operation, false);
    let succession = succession_input();
    let source_revision = non_zero(7);
    let source_digest = compact_source_digest(&operation, source_revision, predecessor.clone());
    let transition = build_succession(
        &operation,
        source_revision,
        predecessor.clone(),
        succession.clone(),
    )
    .unwrap();
    let successor_bytes = transition.result().state_bytes().to_vec();

    let mut noncanonical = successor_bytes.clone();
    noncanonical.push(b' ');
    assert!(matches!(
        validate_compact_succession(
            &operation,
            source_revision,
            source_digest,
            &predecessor,
            &succession,
            &noncanonical,
        ),
        Err(
            RuntimeCompactPendingDrainSuccessionValidationErrorV2::CanonicalState(
                RuntimeDrainIntentCanonicalStateErrorV2::NonCanonicalEncoding
            )
        )
    ));

    let wrong_revision = String::from_utf8(successor_bytes)
        .unwrap()
        .replacen("\"intent_revision\":8", "\"intent_revision\":9", 1)
        .into_bytes();
    assert_eq!(
        validate_compact_succession(
            &operation,
            source_revision,
            source_digest,
            &predecessor,
            &succession,
            &wrong_revision,
        ),
        Err(RuntimeCompactPendingDrainSuccessionValidationErrorV2::SuccessorIntentRevisionMismatch)
    );
}

#[test]
fn compact_succession_validator_rejects_claim_fence_and_seal_tampering() {
    let operation = operation();
    let predecessor = claim(&operation, false);
    let succession = succession_input();
    let source_revision = non_zero(7);
    let source_digest = compact_source_digest(&operation, source_revision, predecessor.clone());
    let owner_revision = succession.recovery_witness.observed_owner_revision.get();
    let provenance =
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(succession.recovery_witness.clone());

    for (claim, expected) in [
        (
            compact_successor_claim(&operation, &succession, owner_revision, 20, 21, 48, 100),
            RuntimeDrainIntentReceiptErrorV2::SuccessionClaimRevisionMismatch,
        ),
        (
            compact_successor_claim(&operation, &succession, owner_revision, 19, 22, 48, 100),
            RuntimeDrainIntentReceiptErrorV2::SuccessionFenceMismatch,
        ),
        (
            compact_successor_claim(&operation, &succession, owner_revision, 19, 21, 49, 100),
            RuntimeDrainIntentReceiptErrorV2::SuccessionSealMismatch,
        ),
        (
            compact_successor_claim(&operation, &succession, owner_revision, 19, 21, 48, 101),
            RuntimeDrainIntentReceiptErrorV2::SuccessionSealMismatch,
        ),
    ] {
        let successor_bytes = compact_successor_bytes(
            &operation,
            non_zero(8),
            claim,
            provenance.clone(),
            succession.certification.clone(),
            succession.acknowledged_at,
        );
        assert_eq!(
            validate_compact_succession(
                &operation,
                source_revision,
                source_digest,
                &predecessor,
                &succession,
                &successor_bytes,
            ),
            Err(RuntimeCompactPendingDrainSuccessionValidationErrorV2::Succession(expected))
        );
    }
}

#[test]
fn compact_succession_validator_rejects_provenance_certification_and_time_tampering() {
    let operation = operation();
    let predecessor = claim(&operation, false);
    let succession = succession_input();
    let source_revision = non_zero(7);
    let source_digest = compact_source_digest(&operation, source_revision, predecessor.clone());
    let owner_revision = succession.recovery_witness.observed_owner_revision.get();
    let successor_claim =
        || compact_successor_claim(&operation, &succession, owner_revision, 19, 21, 48, 100);
    let closed =
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(succession.recovery_witness.clone());

    let wrong_provenance = compact_successor_bytes(
        &operation,
        non_zero(8),
        successor_claim(),
        ordinary_provenance(),
        succession.certification.clone(),
        succession.acknowledged_at,
    );
    assert_eq!(
        validate_compact_succession(
            &operation,
            source_revision,
            source_digest,
            &predecessor,
            &succession,
            &wrong_provenance,
        ),
        Err(
            RuntimeCompactPendingDrainSuccessionValidationErrorV2::Succession(
                RuntimeDrainIntentReceiptErrorV2::SuccessionAcknowledgementMismatch
            )
        )
    );

    let wrong_certification = compact_successor_bytes(
        &operation,
        non_zero(8),
        successor_claim(),
        closed.clone(),
        RuntimeDrainCertificationResolutionV2::no_attestation_for_reserved_operation(
            RuntimeCertificationOperationIdV2::parse("444455556666777788889999aaaabbbb").unwrap(),
            RuntimeCertificationIntentFingerprintV2::parse("f".repeat(64)).unwrap(),
        ),
        succession.acknowledged_at,
    );
    assert_eq!(
        validate_compact_succession(
            &operation,
            source_revision,
            source_digest,
            &predecessor,
            &succession,
            &wrong_certification,
        ),
        Err(
            RuntimeCompactPendingDrainSuccessionValidationErrorV2::Succession(
                RuntimeDrainIntentReceiptErrorV2::SuccessionCertificationMismatch
            )
        )
    );

    let wrong_time = compact_successor_bytes(
        &operation,
        non_zero(8),
        successor_claim(),
        closed,
        succession.certification.clone(),
        at(132),
    );
    assert_eq!(
        validate_compact_succession(
            &operation,
            source_revision,
            source_digest,
            &predecessor,
            &succession,
            &wrong_time,
        ),
        Err(
            RuntimeCompactPendingDrainSuccessionValidationErrorV2::Succession(
                RuntimeDrainIntentReceiptErrorV2::SuccessionAcknowledgementMismatch
            )
        )
    );

    let mut fresh = succession;
    fresh.database_now = at(129);
    let valid_transition = build_succession(
        &operation,
        source_revision,
        predecessor.clone(),
        succession_input(),
    )
    .unwrap();
    assert_eq!(
        validate_compact_succession(
            &operation,
            source_revision,
            source_digest,
            &predecessor,
            &fresh,
            valid_transition.result().state_bytes(),
        ),
        Err(
            RuntimeCompactPendingDrainSuccessionValidationErrorV2::Succession(
                RuntimeDrainIntentReceiptErrorV2::SuccessionPredecessorNotExpired
            )
        )
    );
}

#[test]
fn canonical_state_module_remains_pure_and_dependency_free() {
    let source = include_str!("../v2_drain_intent_canonical_state.rs");
    let wire = include_str!("wire.rs");
    for forbidden in [
        "sqlx",
        "rusqlite",
        "twilight",
        "async fn",
        "impl Future",
        "std::fs",
        "std::net",
    ] {
        assert!(!source.contains(forbidden), "{forbidden}");
        assert!(!wire.contains(forbidden), "{forbidden}");
    }
}
