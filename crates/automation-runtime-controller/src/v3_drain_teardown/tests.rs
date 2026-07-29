use std::num::NonZeroU64;

use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    ActivationRequestId, BindingRevision, ControllerId, DeploymentId, DeploymentRevision,
    FencingToken, InstallationId, ProcessInstanceId, PromotionId, RuntimeDeployment,
    RuntimeDeploymentIdentityV1, RuntimeDeploymentSnapshotV1, RuntimeDeploymentTargetV1,
    RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use chrono::{DateTime, Timelike, Utc};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;
use sha2::{Digest, Sha256};

use super::{
    RuntimeCanonicalDrainIntentStateV3, RuntimeDrainActionDigestV3,
    RuntimeDrainCanonicalStateDigestV3, RuntimeDrainIntentCanonicalStateKindV3,
    RuntimeDrainTeardownCanonicalErrorV3, RuntimePreviousProcessDrainCertificationResolutionKindV3,
    RuntimePreviousProcessDrainProgressV3, RuntimePreviousProcessDrainTeardownSuccessionInputV3,
    RuntimePreviousProcessDrainTeardownSuccessionTransitionV3,
};
use crate::{
    GatewayShardIdV1, RuntimeBuildRevisionV1, RuntimeCanonicalDrainIntentStateV2,
    RuntimeCanonicalProductDrainV2, RuntimeCertificationIntentFingerprintV2,
    RuntimeCertificationOperationIdV2, RuntimeClosedRecoveryRouteWitnessV2,
    RuntimeDrainCertificationResolutionV2, RuntimeDrainClaimProgressV2,
    RuntimeDrainClaimSealWitnessV2, RuntimeDrainClaimV2, RuntimeDrainIntentCanonicalStateErrorV2,
    RuntimeDrainIntentIdV2, RuntimeDrainIntentV2, RuntimeExactLocalRouteIdentityV2,
    RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeLiveAttestationDigestV2, RuntimePersistedProductDrainRootV2,
    RuntimePersistedRefencedPendingDrainIntentV2,
    RuntimePersistedRoutedClaimedPendingDrainIntentV2,
    RuntimePersistedUnclaimedPendingDrainIntentV2, RuntimeProductDrainOperationV2,
    RuntimeProductMutationKindV2, RuntimeProductMutationPreimageV2, RuntimeProductOperationIdV2,
    RuntimeProductSemanticRequestDigestV2, RuntimeRecoveryIdV2, RuntimeRouteMutationProvenanceV2,
    RuntimeRoutedPendingDrainClaimInputV2, RuntimeRoutedPendingDrainClaimTransitionV2,
    RuntimeRoutedPendingDrainRefenceInputV2, RuntimeRoutedPendingDrainRefenceTransitionV2,
    RuntimeServingIdentityV2, RuntimeServingSlotV2,
};

fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn at(second: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(second, 0).unwrap()
}

fn predecessor_process_id() -> ProcessInstanceId {
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

fn persisted_root(
    operation: &RuntimeProductDrainOperationV2,
) -> RuntimePersistedProductDrainRootV2 {
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

fn predecessor_owner() -> RuntimeGatewayOwnerLeaseIdV1 {
    RuntimeGatewayOwnerLeaseIdV1 {
        gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        process_instance_id: predecessor_process_id(),
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

fn route_with(fence: u64, incarnation: u64) -> RuntimeExactLocalRouteIdentityV2 {
    RuntimeExactLocalRouteIdentityV2 {
        identity: RuntimeProcessIdentityV1 {
            target: target(),
            runtime_generation: RuntimeGeneration::new(6).unwrap(),
            process_instance_id: predecessor_process_id(),
        },
        controller_fencing_token: FencingToken::new(fence).unwrap(),
        route_incarnation: non_zero(incarnation),
    }
}

fn route(fence: u64) -> RuntimeExactLocalRouteIdentityV2 {
    route_with(fence, 8)
}

fn predecessor_recovery_witness() -> RuntimeClosedRecoveryRouteWitnessV2 {
    RuntimeClosedRecoveryRouteWitnessV2 {
        recovery_id: RuntimeRecoveryIdV2::parse("22223333444455556666777788889999").unwrap(),
        originating_emergency_generation: non_zero(30),
        recovery_generation: non_zero(31),
        recovery_authority_revision: non_zero(32),
        gateway_owner_lease_id: predecessor_owner(),
        observed_owner_revision: non_zero(16),
        owner_expires_at: at(500),
        process_instance_id: predecessor_process_id(),
        connection_epoch: non_zero(33),
        paused_admission_revision: non_zero(34),
        connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(35)),
        pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(36)),
    }
}

fn successor_recovery_witness() -> RuntimeClosedRecoveryRouteWitnessV2 {
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

fn persisted_unclaimed(
    root: &RuntimePersistedProductDrainRootV2,
    intent_revision: NonZeroU64,
) -> RuntimePersistedUnclaimedPendingDrainIntentV2 {
    let intent = RuntimeDrainIntentV2::pending_from_persisted(root, intent_revision, None).unwrap();
    let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(intent).unwrap();
    RuntimePersistedUnclaimedPendingDrainIntentV2::from_persisted(
        root,
        intent_revision,
        "pending",
        canonical.state_bytes(),
    )
    .unwrap()
}

fn routed_claim_input(route_fence: u64) -> RuntimeRoutedPendingDrainClaimInputV2 {
    RuntimeRoutedPendingDrainClaimInputV2 {
        gateway_owner_lease_id: predecessor_owner(),
        observed_owner_revision: non_zero(16),
        controller_id: ControllerId::parse("controller:1").unwrap(),
        claim_epoch: non_zero(17),
        claim_expires_at: at(130),
        seal_generation: non_zero(14),
        seal_observation_sequence: non_zero(15),
        expected_route: route(route_fence),
    }
}

fn routed_source(
    operation: &RuntimeProductDrainOperationV2,
) -> RuntimePersistedRoutedClaimedPendingDrainIntentV2 {
    let root = persisted_root(operation);
    let transition = RuntimeRoutedPendingDrainClaimTransitionV2::build(
        persisted_unclaimed(&root, non_zero(1)),
        routed_claim_input(19),
    )
    .unwrap();
    RuntimePersistedRoutedClaimedPendingDrainIntentV2::from_persisted(
        &root,
        transition.result().intent().intent_revision(),
        "pending",
        transition.result().state_bytes(),
    )
    .unwrap()
}

fn refenced_source(
    operation: &RuntimeProductDrainOperationV2,
) -> RuntimePersistedRefencedPendingDrainIntentV2 {
    let root = persisted_root(operation);
    let routed = routed_source(operation);
    let transition = RuntimeRoutedPendingDrainRefenceTransitionV2::build(
        routed,
        RuntimeRoutedPendingDrainRefenceInputV2 {
            provenance: RuntimeRouteMutationProvenanceV2::ClosedRecovery(
                predecessor_recovery_witness(),
            ),
            old_route: route(19),
            removal_target: route(20),
            registry_observation_sequence: non_zero(21),
            refenced_at: at(110),
        },
    )
    .unwrap();
    RuntimePersistedRefencedPendingDrainIntentV2::from_persisted(
        &root,
        transition.result().intent().intent_revision(),
        "pending",
        transition.result().state_bytes(),
    )
    .unwrap()
}

fn action_digest(character: char) -> RuntimeDrainActionDigestV3 {
    RuntimeDrainActionDigestV3::parse(character.to_string().repeat(64)).unwrap()
}

fn succession_input(
    refenced: bool,
    certification: RuntimeDrainCertificationResolutionV2,
) -> RuntimePreviousProcessDrainTeardownSuccessionInputV3 {
    RuntimePreviousProcessDrainTeardownSuccessionInputV3 {
        database_now: at(130),
        recovery_witness: successor_recovery_witness(),
        controller_id: ControllerId::parse("controller:2").unwrap(),
        seal_generation: non_zero(48),
        seal_observation_sequence: non_zero(49),
        registry_observation_sequence: non_zero(50),
        predecessor_claim_terminal_digest: action_digest('c'),
        predecessor_refence_terminal_digest: refenced.then(|| action_digest('d')),
        certification,
        acknowledged_at: at(131),
    }
}

fn serving_identity(source: &RuntimeCanonicalDrainIntentStateV2) -> RuntimeServingIdentityV2 {
    let predecessor_claim = source.intent().state().pending_claim().unwrap();
    let process_identity = predecessor_claim
        .progress()
        .seal()
        .expected_route()
        .unwrap()
        .identity
        .clone();
    RuntimeServingIdentityV2 {
        scope: source.intent().key().scope.clone(),
        operation_id: RuntimeCertificationOperationIdV2::parse("11112222333344445555666677778888")
            .unwrap(),
        attestation_digest: RuntimeLiveAttestationDigestV2::parse("e".repeat(64)).unwrap(),
        process_identity,
        lease_epoch: non_zero(23),
        revision: non_zero(24),
    }
}

fn committed_certification(
    source: &RuntimeCanonicalDrainIntentStateV2,
) -> RuntimeDrainCertificationResolutionV2 {
    let claim = source.intent().state().pending_claim().unwrap();
    let serving = serving_identity(source);
    RuntimeDrainCertificationResolutionV2::committed_and_disconnected(
        source.intent().key(),
        claim,
        serving.operation_id.clone(),
        serving,
        non_zero(25),
    )
    .unwrap()
}

fn replace_once(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
    let source = std::str::from_utf8(bytes).unwrap();
    assert_eq!(source.matches(from).count(), 1);
    source.replacen(from, to, 1).into_bytes()
}

fn replace_all(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
    let source = std::str::from_utf8(bytes).unwrap();
    assert!(source.matches(from).count() > 1);
    source.replace(from, to).into_bytes()
}

fn replace_last(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
    let source = std::str::from_utf8(bytes).unwrap();
    let offset = source.rfind(from).unwrap();
    let mut result = String::with_capacity(source.len() - from.len() + to.len());
    result.push_str(&source[..offset]);
    result.push_str(to);
    result.push_str(&source[offset + from.len()..]);
    result.into_bytes()
}

fn routed_transition() -> (
    RuntimeProductDrainOperationV2,
    RuntimePreviousProcessDrainTeardownSuccessionTransitionV3,
) {
    let operation = operation();
    let transition =
        RuntimePreviousProcessDrainTeardownSuccessionTransitionV3::from_routed_claimed(
            routed_source(&operation),
            succession_input(
                false,
                RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
            ),
        )
        .unwrap();
    (operation, transition)
}

fn refenced_transition() -> (
    RuntimeProductDrainOperationV2,
    RuntimePreviousProcessDrainTeardownSuccessionTransitionV3,
) {
    let operation = operation();
    let source = refenced_source(&operation);
    let certification = committed_certification(source.canonical());
    let transition = RuntimePreviousProcessDrainTeardownSuccessionTransitionV3::from_refenced(
        source,
        succession_input(true, certification),
    )
    .unwrap();
    (operation, transition)
}

fn manual_routed_claim(
    operation: &RuntimeProductDrainOperationV2,
    route_fence: u64,
    claim_fence: u64,
    claim_revision: u64,
) -> RuntimeDrainClaimV2 {
    let key = &operation.canonical().drain_preimage().key;
    let seal = RuntimeDrainClaimSealWitnessV2::new(
        key,
        predecessor_process_id(),
        non_zero(14),
        Some(route(route_fence)),
        non_zero(15),
    )
    .unwrap();
    RuntimeDrainClaimV2::new(
        key,
        predecessor_owner(),
        non_zero(16),
        predecessor_process_id(),
        ControllerId::parse("controller:1").unwrap(),
        FencingToken::new(claim_fence).unwrap(),
        non_zero(17),
        non_zero(claim_revision),
        at(130),
        RuntimeDrainClaimProgressV2::claimed(seal),
    )
    .unwrap()
}

fn classify_routed(
    operation: &RuntimeProductDrainOperationV2,
    intent_revision: NonZeroU64,
    claim: RuntimeDrainClaimV2,
) -> Result<
    RuntimePersistedRoutedClaimedPendingDrainIntentV2,
    RuntimeDrainIntentCanonicalStateErrorV2,
> {
    let root = persisted_root(operation);
    let intent =
        RuntimeDrainIntentV2::pending_from_persisted(&root, intent_revision, Some(claim)).unwrap();
    let canonical = RuntimeCanonicalDrainIntentStateV2::from_intent(intent).unwrap();
    RuntimePersistedRoutedClaimedPendingDrainIntentV2::from_persisted(
        &root,
        intent_revision,
        "pending",
        canonical.state_bytes(),
    )
}

#[test]
fn routed_teardown_is_the_exact_current_owner_successor() {
    let (operation, transition) = routed_transition();
    let source = transition.source();
    let result = transition.result();
    let acknowledgement = result.acknowledgement().unwrap();
    let successor = acknowledgement.successor_claim();
    let basis = acknowledgement.absence_basis();

    assert_eq!(source.intent().intent_revision(), non_zero(2));
    assert_eq!(result.intent_revision(), non_zero(3));
    assert_eq!(
        result.state_kind(),
        RuntimeDrainIntentCanonicalStateKindV3::RouteAbsentAcknowledged
    );
    assert_eq!(successor.claim_revision(), non_zero(2));
    assert_eq!(
        successor.controller_fencing_token(),
        FencingToken::new(21).unwrap()
    );
    assert_eq!(successor.process_instance_id(), &successor_process_id());
    assert_eq!(successor.gateway_owner_lease_id(), &successor_owner());
    assert!(successor.progress().seal().expected_route().is_none());
    assert_eq!(basis.predecessor_intent_revision(), non_zero(2));
    assert_eq!(
        basis.predecessor_state_digest(),
        &RuntimeDrainCanonicalStateDigestV3::from_state_bytes(source.state_bytes())
    );
    assert_eq!(
        basis.predecessor_progress(),
        RuntimePreviousProcessDrainProgressV3::RoutedClaimed
    );
    assert_eq!(basis.route_identity(), &route(19).identity);
    assert_eq!(basis.route_incarnation(), non_zero(8));
    assert_eq!(basis.source_route_fence(), FencingToken::new(19).unwrap());
    assert_eq!(
        basis.possible_route_fence_ceiling(),
        FencingToken::new(20).unwrap()
    );
    assert_eq!(
        basis.predecessor_claim_terminal_digest(),
        &action_digest('c')
    );
    assert!(basis.predecessor_refence_terminal_digest().is_none());
    assert_eq!(
        acknowledgement.certification().kind(),
        RuntimePreviousProcessDrainCertificationResolutionKindV3::NoOperationReserved
    );
    assert_eq!(
        acknowledgement.provenance(),
        &RuntimeRouteMutationProvenanceV2::ClosedRecovery(successor_recovery_witness())
    );
    assert_eq!(
        acknowledgement.registry_observation_sequence(),
        non_zero(50)
    );
    assert_eq!(acknowledgement.acknowledged_at(), at(131));

    let root = persisted_root(&operation);
    let restored = transition
        .verify_persisted_result(
            &root,
            result.intent_revision(),
            result.persisted_state(),
            result.state_bytes(),
        )
        .unwrap();
    assert_eq!(&restored, result);
}

#[test]
fn refenced_teardown_preserves_refence_and_predecessor_certification() {
    let (operation, transition) = refenced_transition();
    let source = transition.source();
    let result = transition.result();
    let acknowledgement = result.acknowledgement().unwrap();
    let successor = acknowledgement.successor_claim();
    let basis = acknowledgement.absence_basis();
    let certification = acknowledgement.certification();

    assert_eq!(source.intent().intent_revision(), non_zero(3));
    assert_eq!(result.intent_revision(), non_zero(4));
    assert_eq!(successor.claim_revision(), non_zero(3));
    assert_eq!(
        successor.controller_fencing_token(),
        FencingToken::new(21).unwrap()
    );
    assert_eq!(
        basis.predecessor_progress(),
        RuntimePreviousProcessDrainProgressV3::Refenced
    );
    assert_eq!(basis.source_route_fence(), FencingToken::new(19).unwrap());
    assert_eq!(
        basis.possible_route_fence_ceiling(),
        FencingToken::new(20).unwrap()
    );
    assert_eq!(
        basis.predecessor_refence_terminal_digest(),
        Some(&action_digest('d'))
    );
    assert_eq!(
        certification.kind(),
        RuntimePreviousProcessDrainCertificationResolutionKindV3::CommittedAndDisconnected
    );
    assert_eq!(
        certification.serving_identity().unwrap().process_identity,
        route(19).identity
    );
    assert_ne!(
        certification
            .serving_identity()
            .unwrap()
            .process_identity
            .process_instance_id,
        successor.process_instance_id().clone()
    );
    assert_eq!(certification.disconnected_revision(), Some(non_zero(25)));

    let root = persisted_root(&operation);
    let restored = RuntimeCanonicalDrainIntentStateV3::from_persisted(
        &root,
        result.intent_revision(),
        result.persisted_state(),
        result.state_bytes(),
    )
    .unwrap();
    assert_eq!(&restored, result);
}

#[test]
fn no_attestation_certification_round_trips_without_successor_claim_rebinding() {
    let operation = operation();
    let operation_id =
        RuntimeCertificationOperationIdV2::parse("aaaa2222333344445555666677778888").unwrap();
    let fingerprint = RuntimeCertificationIntentFingerprintV2::parse("f".repeat(64)).unwrap();
    let certification =
        RuntimeDrainCertificationResolutionV2::no_attestation_for_reserved_operation(
            operation_id.clone(),
            fingerprint.clone(),
        );
    let transition =
        RuntimePreviousProcessDrainTeardownSuccessionTransitionV3::from_routed_claimed(
            routed_source(&operation),
            succession_input(false, certification),
        )
        .unwrap();
    let result = transition.result();
    let certification = result.acknowledgement().unwrap().certification();

    assert_eq!(
        certification.kind(),
        RuntimePreviousProcessDrainCertificationResolutionKindV3::NoAttestationForReservedOperation
    );
    assert_eq!(certification.operation_id(), Some(&operation_id));
    assert_eq!(certification.intent_fingerprint(), Some(&fingerprint));
    assert!(certification.serving_identity().is_none());
    assert!(certification.disconnected_revision().is_none());

    let restored = RuntimeCanonicalDrainIntentStateV3::from_persisted(
        &persisted_root(&operation),
        result.intent_revision(),
        result.persisted_state(),
        result.state_bytes(),
    )
    .unwrap();
    assert_eq!(&restored, result);
}

#[test]
fn v3_terminal_successors_are_closed_and_round_trip_exactly() {
    let operation = operation();
    let root = persisted_root(&operation);
    let consumed = RuntimeCanonicalDrainIntentStateV3::consumed_from_persisted(
        &root,
        non_zero(9),
        DeploymentRevision::new(10).unwrap(),
        at(200),
    )
    .unwrap();
    let cancelled =
        RuntimeCanonicalDrainIntentStateV3::cancelled_from_persisted(&root, non_zero(11), at(201))
            .unwrap();

    assert_eq!(
        consumed.state_kind(),
        RuntimeDrainIntentCanonicalStateKindV3::Consumed
    );
    assert_eq!(
        consumed.resulting_revision(),
        Some(DeploymentRevision::new(10).unwrap())
    );
    assert_eq!(consumed.consumed_at(), Some(at(200)));
    assert!(consumed.acknowledgement().is_none());
    assert_eq!(
        cancelled.state_kind(),
        RuntimeDrainIntentCanonicalStateKindV3::Cancelled
    );
    assert_eq!(cancelled.cancelled_at(), Some(at(201)));
    assert!(cancelled.acknowledgement().is_none());

    for canonical in [&consumed, &cancelled] {
        let restored = RuntimeCanonicalDrainIntentStateV3::from_persisted(
            &root,
            canonical.intent_revision(),
            canonical.persisted_state(),
            canonical.state_bytes(),
        )
        .unwrap();
        assert_eq!(&restored, canonical);
    }
}

#[test]
fn v3_decoder_dispatches_exactly_and_rejects_noncanonical_encodings() {
    let (operation, transition) = routed_transition();
    let root = persisted_root(&operation);
    let result = transition.result();
    let state_bytes = result.state_bytes();
    let version_two = replace_once(state_bytes, "\"format_version\":3", "\"format_version\":2");
    let version_four = replace_once(state_bytes, "\"format_version\":3", "\"format_version\":4");
    let unknown = replace_once(
        state_bytes,
        "\"format_version\":3,",
        "\"format_version\":3,\"unknown\":1,",
    );
    let duplicate = replace_once(
        state_bytes,
        "\"format_version\":3,",
        "\"format_version\":3,\"format_version\":3,",
    );
    let whitespace = [b" ".as_slice(), state_bytes].concat();
    let noncanonical_number = replace_once(
        state_bytes,
        "\"intent_revision\":3",
        "\"intent_revision\":3.0",
    );
    let pending = replace_once(
        state_bytes,
        "\"kind\":\"route_absent_acknowledged\"",
        "\"kind\":\"pending\"",
    );

    for bytes in [&version_two, &version_four] {
        assert_eq!(
            RuntimeCanonicalDrainIntentStateV3::from_persisted(
                &root,
                result.intent_revision(),
                result.persisted_state(),
                bytes,
            ),
            Err(RuntimeDrainTeardownCanonicalErrorV3::UnsupportedFormatVersion)
        );
    }
    for bytes in [&unknown, &duplicate, &noncanonical_number, &pending] {
        assert_eq!(
            RuntimeCanonicalDrainIntentStateV3::from_persisted(
                &root,
                result.intent_revision(),
                result.persisted_state(),
                bytes,
            ),
            Err(RuntimeDrainTeardownCanonicalErrorV3::Decoding)
        );
    }
    assert_eq!(
        RuntimeCanonicalDrainIntentStateV3::from_persisted(
            &root,
            result.intent_revision(),
            result.persisted_state(),
            &whitespace,
        ),
        Err(RuntimeDrainTeardownCanonicalErrorV3::NonCanonicalEncoding)
    );
    assert_eq!(
        RuntimeCanonicalDrainIntentStateV3::from_persisted(
            &root,
            result.intent_revision(),
            "consumed",
            state_bytes,
        ),
        Err(RuntimeDrainTeardownCanonicalErrorV3::PersistedStateMismatch)
    );
    assert_eq!(
        RuntimeCanonicalDrainIntentStateV3::from_persisted(
            &root,
            non_zero(4),
            result.persisted_state(),
            state_bytes,
        ),
        Err(RuntimeDrainTeardownCanonicalErrorV3::ImmutableRootMismatch)
    );
    assert!(RuntimeCanonicalDrainIntentStateV2::from_persisted(
        &root,
        result.intent_revision(),
        result.persisted_state(),
        state_bytes,
    )
    .is_err());
    assert_eq!(
        RuntimeCanonicalDrainIntentStateV3::from_persisted(
            &root,
            transition.source().intent().intent_revision(),
            transition.source().persisted_state().unwrap(),
            transition.source().state_bytes(),
        ),
        Err(RuntimeDrainTeardownCanonicalErrorV3::UnsupportedFormatVersion)
    );
}

#[test]
fn v3_decoder_rejects_missing_or_malformed_teardown_evidence() {
    let (operation, transition) = routed_transition();
    let root = persisted_root(&operation);
    let result = transition.result();
    let bytes = result.state_bytes();
    let missing_refence_digest =
        replace_once(bytes, ",\"predecessor_refence_terminal_digest\":null", "");
    let uppercase_digest = replace_once(bytes, &"c".repeat(64), &"C".repeat(64));
    let identical_refence_digest = replace_once(
        bytes,
        "\"predecessor_progress\":\"routed_claimed\"",
        "\"predecessor_progress\":\"refenced\"",
    );
    let skipped_fence = replace_once(
        bytes,
        "\"possible_route_fence_ceiling\":20",
        "\"possible_route_fence_ceiling\":22",
    );
    let wrong_successor_fence = replace_once(
        bytes,
        "\"controller_fencing_token\":21",
        "\"controller_fencing_token\":22",
    );
    let same_process_downgrade = replace_all(bytes, "process:2", "process:1");

    assert_eq!(
        RuntimeCanonicalDrainIntentStateV3::from_persisted(
            &root,
            result.intent_revision(),
            result.persisted_state(),
            &missing_refence_digest,
        ),
        Err(RuntimeDrainTeardownCanonicalErrorV3::Decoding)
    );
    assert_eq!(
        RuntimeCanonicalDrainIntentStateV3::from_persisted(
            &root,
            result.intent_revision(),
            result.persisted_state(),
            &uppercase_digest,
        ),
        Err(RuntimeDrainTeardownCanonicalErrorV3::InvalidDigest)
    );
    assert_eq!(
        RuntimeCanonicalDrainIntentStateV3::from_persisted(
            &root,
            result.intent_revision(),
            result.persisted_state(),
            &identical_refence_digest,
        ),
        Err(RuntimeDrainTeardownCanonicalErrorV3::JournalMismatch)
    );
    assert_eq!(
        RuntimeCanonicalDrainIntentStateV3::from_persisted(
            &root,
            result.intent_revision(),
            result.persisted_state(),
            &skipped_fence,
        ),
        Err(RuntimeDrainTeardownCanonicalErrorV3::RouteLineageMismatch)
    );
    assert_eq!(
        RuntimeCanonicalDrainIntentStateV3::from_persisted(
            &root,
            result.intent_revision(),
            result.persisted_state(),
            &wrong_successor_fence,
        ),
        Err(RuntimeDrainTeardownCanonicalErrorV3::AcknowledgementMismatch)
    );
    assert_eq!(
        RuntimeCanonicalDrainIntentStateV3::from_persisted(
            &root,
            result.intent_revision(),
            result.persisted_state(),
            &same_process_downgrade,
        ),
        Err(RuntimeDrainTeardownCanonicalErrorV3::AcknowledgementMismatch)
    );
}

#[test]
fn standalone_decoder_requires_exact_predecessor_intent_revision_successorship() {
    let (operation, transition) = routed_transition();
    let root = persisted_root(&operation);
    let result = transition.result();
    let state_bytes = result.state_bytes();
    let predecessor_drift = replace_once(
        state_bytes,
        "\"predecessor_intent_revision\":2",
        "\"predecessor_intent_revision\":7",
    );
    let outer_drift = replace_once(
        state_bytes,
        "\"intent_revision\":3",
        "\"intent_revision\":4",
    );
    let predecessor_overflow = replace_once(
        state_bytes,
        "\"predecessor_intent_revision\":2",
        "\"predecessor_intent_revision\":9223372036854775807",
    );

    assert_eq!(
        RuntimeCanonicalDrainIntentStateV3::from_persisted(
            &root,
            result.intent_revision(),
            result.persisted_state(),
            &predecessor_drift,
        ),
        Err(RuntimeDrainTeardownCanonicalErrorV3::IntentRevisionMismatch)
    );
    assert_eq!(
        RuntimeCanonicalDrainIntentStateV3::from_persisted(
            &root,
            non_zero(4),
            result.persisted_state(),
            &outer_drift,
        ),
        Err(RuntimeDrainTeardownCanonicalErrorV3::IntentRevisionMismatch)
    );
    assert_eq!(
        RuntimeCanonicalDrainIntentStateV3::from_persisted(
            &root,
            result.intent_revision(),
            result.persisted_state(),
            &predecessor_overflow,
        ),
        Err(RuntimeDrainTeardownCanonicalErrorV3::IntentRevisionOverflow)
    );
}

#[test]
fn checked_reconstruction_rejects_canonical_predecessor_and_journal_drift() {
    let (operation, transition) = routed_transition();
    let root = persisted_root(&operation);
    let result = transition.result();
    let state_bytes = result.state_bytes();
    let predecessor_digest = replace_once(
        state_bytes,
        result
            .acknowledgement()
            .unwrap()
            .absence_basis()
            .predecessor_state_digest()
            .as_str(),
        &"f".repeat(64),
    );
    let route_incarnation = replace_once(
        state_bytes,
        "\"route_incarnation\":8",
        "\"route_incarnation\":9",
    );
    let route_process = replace_once(state_bytes, "process:1", "process:9");
    let claim_journal = replace_once(state_bytes, &"c".repeat(64), &"a".repeat(64));

    for bytes in [
        predecessor_digest,
        route_incarnation,
        route_process,
        claim_journal,
    ] {
        let standalone = RuntimeCanonicalDrainIntentStateV3::from_persisted(
            &root,
            result.intent_revision(),
            result.persisted_state(),
            &bytes,
        )
        .unwrap();
        assert_ne!(&standalone, result);
        assert_eq!(
            transition.verify_persisted_result(
                &root,
                result.intent_revision(),
                result.persisted_state(),
                &bytes,
            ),
            Err(RuntimeDrainTeardownCanonicalErrorV3::PersistedResultMismatch)
        );
    }
}

#[test]
fn checked_reconstruction_rejects_canonical_successor_owner_process_and_time_drift() {
    let (operation, transition) = routed_transition();
    let root = persisted_root(&operation);
    let result = transition.result();
    let state_bytes = result.state_bytes();
    let owner_revision = replace_once(
        &replace_once(
            state_bytes,
            "\"observed_owner_revision\":43",
            "\"observed_owner_revision\":53",
        ),
        "\\\"observed_owner_revision\\\":43",
        "\\\"observed_owner_revision\\\":53",
    );
    let owner_epoch = replace_once(
        &replace_once(state_bytes, "\"lease_epoch\":4", "\"lease_epoch\":5"),
        "\\\"lease_epoch\\\":4",
        "\\\"lease_epoch\\\":5",
    );
    let process = replace_all(state_bytes, "process:2", "process:8");
    let owner_expiry = replace_once(
        &replace_once(
            state_bytes,
            "\"claim_expires_at_unix_microseconds\":500000000",
            "\"claim_expires_at_unix_microseconds\":510000000",
        ),
        "\\\"owner_expires_at_unix_microseconds\\\":500000000",
        "\\\"owner_expires_at_unix_microseconds\\\":510000000",
    );
    let acknowledged_at = replace_once(
        state_bytes,
        "\"acknowledged_at_unix_microseconds\":131000000",
        "\"acknowledged_at_unix_microseconds\":132000000",
    );

    for bytes in [
        owner_revision,
        owner_epoch,
        process,
        owner_expiry,
        acknowledged_at,
    ] {
        let standalone = RuntimeCanonicalDrainIntentStateV3::from_persisted(
            &root,
            result.intent_revision(),
            result.persisted_state(),
            &bytes,
        )
        .unwrap();
        assert_ne!(&standalone, result);
        assert_eq!(
            transition.verify_persisted_result(
                &root,
                result.intent_revision(),
                result.persisted_state(),
                &bytes,
            ),
            Err(RuntimeDrainTeardownCanonicalErrorV3::PersistedResultMismatch)
        );
    }
}

#[test]
fn committed_certification_decoder_rejects_serving_and_revision_drift() {
    let (operation, transition) = refenced_transition();
    let root = persisted_root(&operation);
    let result = transition.result();
    let bytes = result.state_bytes();
    let serving_process = replace_last(bytes, "process:1", "process:9");
    let serving_operation = replace_last(
        bytes,
        "11112222333344445555666677778888",
        "99992222333344445555666677778888",
    );
    let disconnected_revision = replace_once(
        bytes,
        "\"disconnected_revision\":25",
        "\"disconnected_revision\":26",
    );

    for drifted in [serving_process, serving_operation, disconnected_revision] {
        assert_eq!(
            RuntimeCanonicalDrainIntentStateV3::from_persisted(
                &root,
                result.intent_revision(),
                result.persisted_state(),
                &drifted,
            ),
            Err(RuntimeDrainTeardownCanonicalErrorV3::CertificationMismatch)
        );
    }
}

#[test]
fn succession_rejects_owner_process_epoch_time_and_provenance_drift() {
    let cases = [
        {
            let mut input = succession_input(
                false,
                RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
            );
            input.database_now = at(129);
            (
                input,
                RuntimeDrainTeardownCanonicalErrorV3::PredecessorNotExpired,
            )
        },
        {
            let mut input = succession_input(
                false,
                RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
            );
            input.database_now = at(130).with_nanosecond(1).unwrap();
            (
                input,
                RuntimeDrainTeardownCanonicalErrorV3::DatabaseTimeInvalid,
            )
        },
        {
            let mut input = succession_input(
                false,
                RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
            );
            input.recovery_witness.process_instance_id = predecessor_process_id();
            input
                .recovery_witness
                .gateway_owner_lease_id
                .process_instance_id = predecessor_process_id();
            (
                input,
                RuntimeDrainTeardownCanonicalErrorV3::ProcessNotDistinct,
            )
        },
        {
            let mut input = succession_input(
                false,
                RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
            );
            input
                .recovery_witness
                .gateway_owner_lease_id
                .process_instance_id = predecessor_process_id();
            (input, RuntimeDrainTeardownCanonicalErrorV3::OwnerMismatch)
        },
        {
            let mut input = succession_input(
                false,
                RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
            );
            input
                .recovery_witness
                .gateway_owner_lease_id
                .gateway_shard_id = GatewayShardIdV1::parse("shard:1").unwrap();
            (input, RuntimeDrainTeardownCanonicalErrorV3::ShardMismatch)
        },
        {
            let mut input = succession_input(
                false,
                RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
            );
            input.recovery_witness.gateway_owner_lease_id.lease_epoch = non_zero(3);
            (
                input,
                RuntimeDrainTeardownCanonicalErrorV3::OwnerEpochNotNewer,
            )
        },
        {
            let mut input = succession_input(
                false,
                RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
            );
            input.database_now = at(500);
            (input, RuntimeDrainTeardownCanonicalErrorV3::OwnerExpired)
        },
    ];

    for (input, expected) in cases {
        let operation = operation();
        assert_eq!(
            RuntimePreviousProcessDrainTeardownSuccessionTransitionV3::from_routed_claimed(
                routed_source(&operation),
                input,
            ),
            Err(expected)
        );
    }

    let operation = operation();
    let mut invalid_provenance = succession_input(
        false,
        RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
    );
    invalid_provenance.recovery_witness.pause_sequence =
        invalid_provenance.recovery_witness.connected_event_sequence;
    assert_eq!(
        RuntimePreviousProcessDrainTeardownSuccessionTransitionV3::from_routed_claimed(
            routed_source(&operation),
            invalid_provenance,
        ),
        Err(RuntimeDrainTeardownCanonicalErrorV3::ProvenanceMismatch)
    );
}

#[test]
fn succession_requires_exact_journal_shapes_and_predecessor_certification() {
    let operation = operation();
    let mut routed_with_refence = succession_input(
        false,
        RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
    );
    routed_with_refence.predecessor_refence_terminal_digest = Some(action_digest('d'));
    assert_eq!(
        RuntimePreviousProcessDrainTeardownSuccessionTransitionV3::from_routed_claimed(
            routed_source(&operation),
            routed_with_refence,
        ),
        Err(RuntimeDrainTeardownCanonicalErrorV3::JournalMismatch)
    );

    let mut refenced_without_refence = succession_input(
        true,
        RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
    );
    refenced_without_refence.predecessor_refence_terminal_digest = None;
    assert_eq!(
        RuntimePreviousProcessDrainTeardownSuccessionTransitionV3::from_refenced(
            refenced_source(&operation),
            refenced_without_refence,
        ),
        Err(RuntimeDrainTeardownCanonicalErrorV3::JournalMismatch)
    );

    let mut identical_journals = succession_input(
        true,
        RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
    );
    identical_journals.predecessor_refence_terminal_digest = Some(action_digest('c'));
    assert_eq!(
        RuntimePreviousProcessDrainTeardownSuccessionTransitionV3::from_refenced(
            refenced_source(&operation),
            identical_journals,
        ),
        Err(RuntimeDrainTeardownCanonicalErrorV3::JournalMismatch)
    );

    let refenced = refenced_source(&operation);
    let serving = serving_identity(refenced.canonical());
    let successor_key = refenced.canonical().intent().key();
    let successor_claim = {
        let seal = RuntimeDrainClaimSealWitnessV2::new(
            successor_key,
            successor_process_id(),
            non_zero(48),
            None,
            non_zero(49),
        )
        .unwrap();
        RuntimeDrainClaimV2::new(
            successor_key,
            successor_owner(),
            non_zero(43),
            successor_process_id(),
            ControllerId::parse("controller:2").unwrap(),
            FencingToken::new(21).unwrap(),
            non_zero(41),
            non_zero(3),
            at(500),
            RuntimeDrainClaimProgressV2::claimed(seal),
        )
        .unwrap()
    };
    assert!(
        RuntimeDrainCertificationResolutionV2::committed_and_disconnected(
            successor_key,
            &successor_claim,
            serving.operation_id.clone(),
            serving,
            non_zero(25),
        )
        .is_err()
    );
}

#[test]
fn succession_preflights_intent_fence_and_classifier_overflow_boundaries() {
    let operation = operation();
    let max_persisted = i64::MAX as u64;
    let max_revision_source = classify_routed(
        &operation,
        non_zero(max_persisted),
        manual_routed_claim(&operation, 19, 20, 1),
    )
    .unwrap();
    assert_eq!(
        RuntimePreviousProcessDrainTeardownSuccessionTransitionV3::from_routed_claimed(
            max_revision_source,
            succession_input(
                false,
                RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
            ),
        ),
        Err(RuntimeDrainTeardownCanonicalErrorV3::IntentRevisionOverflow)
    );

    let max_fence_source = classify_routed(
        &operation,
        non_zero(2),
        manual_routed_claim(&operation, max_persisted - 1, max_persisted, 1),
    )
    .unwrap();
    assert_eq!(
        RuntimePreviousProcessDrainTeardownSuccessionTransitionV3::from_routed_claimed(
            max_fence_source,
            succession_input(
                false,
                RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
            ),
        ),
        Err(RuntimeDrainTeardownCanonicalErrorV3::FenceOverflow)
    );

    assert!(classify_routed(
        &operation,
        non_zero(2),
        manual_routed_claim(&operation, 19, 20, max_persisted),
    )
    .is_err());
}

#[test]
fn routed_classifier_rejects_missing_or_drifted_durable_route_evidence() {
    let operation = operation();
    let key = &operation.canonical().drain_preimage().key;
    let empty_seal = RuntimeDrainClaimSealWitnessV2::new(
        key,
        predecessor_process_id(),
        non_zero(14),
        None,
        non_zero(15),
    )
    .unwrap();
    let empty_claim = RuntimeDrainClaimV2::new(
        key,
        predecessor_owner(),
        non_zero(16),
        predecessor_process_id(),
        ControllerId::parse("controller:1").unwrap(),
        FencingToken::new(20).unwrap(),
        non_zero(17),
        non_zero(1),
        at(130),
        RuntimeDrainClaimProgressV2::claimed(empty_seal),
    )
    .unwrap();

    assert!(classify_routed(&operation, non_zero(2), empty_claim).is_err());
    assert!(classify_routed(
        &operation,
        non_zero(2),
        manual_routed_claim(&operation, 19, 21, 1),
    )
    .is_err());
    assert!(classify_routed(
        &operation,
        non_zero(2),
        manual_routed_claim(&operation, 19, 20, 2),
    )
    .is_err());
}

#[test]
fn v3_payload_limit_matches_the_one_mebibyte_execution_frame_cap() {
    let operation = operation();
    let root = persisted_root(&operation);
    let oversized = vec![b' '; super::DRAIN_INTENT_STATE_MAX_OCTETS_V3 + 1];
    assert_eq!(
        RuntimeCanonicalDrainIntentStateV3::from_persisted(
            &root,
            non_zero(1),
            "route_absent_acknowledged",
            &oversized,
        ),
        Err(RuntimeDrainTeardownCanonicalErrorV3::CanonicalV2(
            RuntimeDrainIntentCanonicalStateErrorV2::PayloadTooLarge,
        ))
    );
}

#[test]
fn routed_and_refenced_teardown_bytes_match_independent_goldens() {
    let (_, routed) = routed_transition();
    let (_, refenced) = refenced_transition();
    let vectors = [
        (
            routed.result(),
            include_bytes!("goldens/routed_claimed_teardown_v3.json").as_slice(),
            "1deb34ccff32711691a1dbd125c8ce1e14850cc6480c7ca94571087773418f59",
        ),
        (
            refenced.result(),
            include_bytes!("goldens/refenced_teardown_v3.json").as_slice(),
            "5db6b51cf693a4ffb5ecb74569d299148492f7f64e42c5d5b25306bbadd446a1",
        ),
    ];
    for (result, golden, expected_digest) in vectors {
        let golden = golden.strip_suffix(b"\n").unwrap_or(golden);
        assert_eq!(result.state_bytes(), golden);
        assert_eq!(
            format!("{:x}", Sha256::digest(result.state_bytes())),
            expected_digest
        );
    }
}
