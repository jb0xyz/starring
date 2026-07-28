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

use super::{
    RuntimeDrainAcknowledgementSourceV2, RuntimeDrainIntentMutationOutcomeV2,
    RuntimeDrainIntentReceiptErrorV2, RuntimeDrainIntentReceiptV2, RuntimeDrainRefenceSourceV2,
    RuntimeDrainSuccessionAcknowledgementExpectationV2,
    RuntimeDrainSuccessionAcknowledgementSourceV2, RuntimeRouteAbsentDrainIntentSourceV2,
};
use crate::{
    GatewayShardIdV1, RuntimeBarrierIdV1, RuntimeBarrierPauseWitnessV2, RuntimeBuildRevisionV1,
    RuntimeCanonicalProductDrainV2, RuntimeCertificationIntentFingerprintV2,
    RuntimeCertificationOperationIdV2, RuntimeClosedRecoveryRouteWitnessV2,
    RuntimeDrainCertificationResolutionV2, RuntimeDrainClaimProgressV2,
    RuntimeDrainClaimSealWitnessV2, RuntimeDrainClaimV2, RuntimeDrainIntentIdV2,
    RuntimeDrainIntentV2, RuntimeExactLocalRouteIdentityV2, RuntimeGatewayAdmissionSequenceV2,
    RuntimeGatewayOwnerLeaseIdV1, RuntimePersistedProductDrainRootV2,
    RuntimeProductDrainOperationV2, RuntimeProductMutationKindV2, RuntimeProductMutationPreimageV2,
    RuntimeProductOperationIdV2, RuntimeProductSemanticRequestDigestV2, RuntimeRecoveryIdV2,
    RuntimeRouteAbsentAcknowledgementV2, RuntimeRouteMutationProvenanceV2, RuntimeServingSlotV2,
};

const PRODUCT_OPERATION_ID: &str = "00112233445566778899aabbccddeeff";
const DRAIN_INTENT_ID: &str = "ffeeddccbbaa99887766554433221100";
const FOREIGN_DRAIN_INTENT_ID: &str = "11112222333344445555666677778888";

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

fn operation_for(intent_id: &str) -> RuntimeProductDrainOperationV2 {
    let snapshot = snapshot();
    let product = RuntimeProductMutationPreimageV2 {
        operation_id: RuntimeProductOperationIdV2::parse(PRODUCT_OPERATION_ID).unwrap(),
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
        RuntimeDrainIntentIdV2::parse(intent_id).unwrap(),
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

fn provenance(seed: &str) -> RuntimeRouteMutationProvenanceV2 {
    RuntimeRouteMutationProvenanceV2::Ordinary {
        barrier_id: RuntimeBarrierIdV1::parse(seed).unwrap(),
        pause: RuntimeBarrierPauseWitnessV2 {
            coordinator_generation: non_zero(4),
            connection_epoch: non_zero(5),
            paused_admission_revision: non_zero(6),
            pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(7)),
        },
    }
}

fn route_for(
    process_instance_id: ProcessInstanceId,
    fence: u64,
    incarnation: u64,
) -> RuntimeExactLocalRouteIdentityV2 {
    RuntimeExactLocalRouteIdentityV2 {
        identity: RuntimeProcessIdentityV1 {
            target: target(),
            runtime_generation: RuntimeGeneration::new(4).unwrap(),
            process_instance_id,
        },
        controller_fencing_token: FencingToken::new(fence).unwrap(),
        route_incarnation: non_zero(incarnation),
    }
}

fn route(fence: u64, incarnation: u64) -> RuntimeExactLocalRouteIdentityV2 {
    route_for(process_id(), fence, incarnation)
}

#[derive(Clone)]
struct ClaimIdentityParts {
    owner: RuntimeGatewayOwnerLeaseIdV1,
    observed_owner_revision: NonZeroU64,
    process_instance_id: ProcessInstanceId,
    controller_id: ControllerId,
    controller_fencing_token: FencingToken,
    claim_epoch: NonZeroU64,
    expires_at: DateTime<Utc>,
}

fn standard_claim_identity() -> ClaimIdentityParts {
    ClaimIdentityParts {
        owner: owner(),
        observed_owner_revision: non_zero(10),
        process_instance_id: process_id(),
        controller_id: ControllerId::parse("controller:1").unwrap(),
        controller_fencing_token: FencingToken::new(11).unwrap(),
        claim_epoch: non_zero(12),
        expires_at: at(500),
    }
}

fn claim_identity_from(source: &RuntimeDrainClaimV2) -> ClaimIdentityParts {
    ClaimIdentityParts {
        owner: source.gateway_owner_lease_id().clone(),
        observed_owner_revision: source.observed_owner_revision(),
        process_instance_id: source.process_instance_id().clone(),
        controller_id: source.controller_id().clone(),
        controller_fencing_token: source.controller_fencing_token(),
        claim_epoch: source.claim_epoch(),
        expires_at: source.expires_at(),
    }
}

fn claimed_with_identity(
    operation: &RuntimeProductDrainOperationV2,
    expected_route: Option<RuntimeExactLocalRouteIdentityV2>,
    claim_revision: u64,
    seal_generation: u64,
    identity: ClaimIdentityParts,
) -> RuntimeDrainClaimV2 {
    let key = &operation.canonical().drain_preimage().key;
    let seal = RuntimeDrainClaimSealWitnessV2::new(
        key,
        identity.process_instance_id.clone(),
        non_zero(seal_generation),
        expected_route,
        non_zero(9),
    )
    .unwrap();
    RuntimeDrainClaimV2::new(
        key,
        identity.owner,
        identity.observed_owner_revision,
        identity.process_instance_id,
        identity.controller_id,
        identity.controller_fencing_token,
        identity.claim_epoch,
        non_zero(claim_revision),
        identity.expires_at,
        RuntimeDrainClaimProgressV2::claimed(seal),
    )
    .unwrap()
}

fn claimed(
    operation: &RuntimeProductDrainOperationV2,
    expected_route: Option<RuntimeExactLocalRouteIdentityV2>,
    claim_revision: u64,
    observed_owner_revision: u64,
    seal_generation: u64,
) -> RuntimeDrainClaimV2 {
    let mut identity = standard_claim_identity();
    identity.observed_owner_revision = non_zero(observed_owner_revision);
    claimed_with_identity(
        operation,
        expected_route,
        claim_revision,
        seal_generation,
        identity,
    )
}

fn refenced_with_identity(
    operation: &RuntimeProductDrainOperationV2,
    source: &RuntimeDrainClaimV2,
    claim_revision: u64,
    seal_generation: u64,
    identity: ClaimIdentityParts,
) -> RuntimeDrainClaimV2 {
    let mut old_route = source.progress().seal().expected_route().unwrap().clone();
    old_route.identity.process_instance_id = identity.process_instance_id.clone();
    let seal = RuntimeDrainClaimSealWitnessV2::new(
        &operation.canonical().drain_preimage().key,
        identity.process_instance_id.clone(),
        non_zero(seal_generation),
        Some(old_route.clone()),
        source.progress().seal().registry_observation_sequence(),
    )
    .unwrap();
    let mut removal_target = old_route.clone();
    removal_target.controller_fencing_token = identity.controller_fencing_token;
    let progress = RuntimeDrainClaimProgressV2::refenced(
        seal,
        provenance("9999aaaabbbbccccddddeeeeffff0000"),
        old_route,
        removal_target,
        non_zero(15),
        at(400),
    )
    .unwrap();
    RuntimeDrainClaimV2::new(
        &operation.canonical().drain_preimage().key,
        identity.owner,
        identity.observed_owner_revision,
        identity.process_instance_id,
        identity.controller_id,
        identity.controller_fencing_token,
        identity.claim_epoch,
        non_zero(claim_revision),
        identity.expires_at,
        progress,
    )
    .unwrap()
}

fn refenced_from(
    operation: &RuntimeProductDrainOperationV2,
    source: &RuntimeDrainClaimV2,
    claim_revision: u64,
    observed_owner_revision: u64,
) -> RuntimeDrainClaimV2 {
    let mut identity = claim_identity_from(source);
    identity.observed_owner_revision = non_zero(observed_owner_revision);
    refenced_with_identity(
        operation,
        source,
        claim_revision,
        source.progress().seal().seal_generation().get(),
        identity,
    )
}

fn pending(
    operation: &RuntimeProductDrainOperationV2,
    intent_revision: u64,
    claim: Option<RuntimeDrainClaimV2>,
) -> RuntimeDrainIntentV2 {
    RuntimeDrainIntentV2::pending_from_persisted(&root(operation), non_zero(intent_revision), claim)
        .unwrap()
}

fn acknowledgement(
    operation: &RuntimeProductDrainOperationV2,
    claim: RuntimeDrainClaimV2,
) -> RuntimeRouteAbsentAcknowledgementV2 {
    acknowledgement_at(operation, claim, at(600))
}

fn acknowledgement_at(
    operation: &RuntimeProductDrainOperationV2,
    claim: RuntimeDrainClaimV2,
    acknowledged_at: DateTime<Utc>,
) -> RuntimeRouteAbsentAcknowledgementV2 {
    let expected_route = claim.progress().removal_target().cloned();
    let sequence = if expected_route.is_some() { 16 } else { 9 };
    RuntimeRouteAbsentAcknowledgementV2::new(
        &operation.canonical().drain_preimage().key,
        claim,
        expected_route,
        provenance("0000ffffeeeeddddccccbbbbaaaa9999"),
        non_zero(sequence),
        RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
        acknowledged_at,
    )
    .unwrap()
}

fn acknowledged(
    operation: &RuntimeProductDrainOperationV2,
    intent_revision: u64,
    claim: RuntimeDrainClaimV2,
) -> RuntimeDrainIntentV2 {
    RuntimeDrainIntentV2::route_absent_acknowledged_from_persisted(
        &root(operation),
        non_zero(intent_revision),
        acknowledgement(operation, claim),
    )
    .unwrap()
}

fn succession_witness() -> RuntimeClosedRecoveryRouteWitnessV2 {
    RuntimeClosedRecoveryRouteWitnessV2 {
        recovery_id: RuntimeRecoveryIdV2::parse("22223333444455556666777788889999").unwrap(),
        originating_emergency_generation: non_zero(20),
        recovery_generation: non_zero(21),
        recovery_authority_revision: non_zero(22),
        gateway_owner_lease_id: successor_owner(),
        observed_owner_revision: non_zero(23),
        owner_expires_at: at(1_000),
        process_instance_id: successor_process_id(),
        connection_epoch: non_zero(24),
        paused_admission_revision: non_zero(25),
        connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(26)),
        pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(27)),
    }
}

fn succession_expectation() -> RuntimeDrainSuccessionAcknowledgementExpectationV2 {
    RuntimeDrainSuccessionAcknowledgementExpectationV2 {
        database_now: at(500),
        recovery_witness: succession_witness(),
        controller_id: ControllerId::parse("controller:2").unwrap(),
        seal_generation: non_zero(28),
        seal_observation_sequence: non_zero(29),
        acknowledgement_observation_sequence: non_zero(30),
        certification: RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
        acknowledged_at: at(501),
    }
}

fn succession_claim(
    operation: &RuntimeProductDrainOperationV2,
    expectation: &RuntimeDrainSuccessionAcknowledgementExpectationV2,
    claim_revision: u64,
    controller_fencing_token: u64,
    seal_generation: u64,
    seal_observation_sequence: u64,
) -> RuntimeDrainClaimV2 {
    let witness = &expectation.recovery_witness;
    let key = &operation.canonical().drain_preimage().key;
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
        witness.observed_owner_revision,
        witness.process_instance_id.clone(),
        expectation.controller_id.clone(),
        FencingToken::new(controller_fencing_token).unwrap(),
        witness.recovery_generation,
        non_zero(claim_revision),
        witness.owner_expires_at,
        RuntimeDrainClaimProgressV2::claimed(seal),
    )
    .unwrap()
}

fn succession_result(
    operation: &RuntimeProductDrainOperationV2,
    intent_revision: u64,
    claim: RuntimeDrainClaimV2,
    expectation: &RuntimeDrainSuccessionAcknowledgementExpectationV2,
) -> RuntimeDrainIntentV2 {
    let acknowledgement = RuntimeRouteAbsentAcknowledgementV2::new(
        &operation.canonical().drain_preimage().key,
        claim,
        None,
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(expectation.recovery_witness.clone()),
        expectation.acknowledgement_observation_sequence,
        expectation.certification.clone(),
        expectation.acknowledged_at,
    )
    .unwrap();
    RuntimeDrainIntentV2::route_absent_acknowledged_from_persisted(
        &root(operation),
        non_zero(intent_revision),
        acknowledgement,
    )
    .unwrap()
}

fn succession_source(
    operation: &RuntimeProductDrainOperationV2,
    expectation: RuntimeDrainSuccessionAcknowledgementExpectationV2,
) -> RuntimeDrainSuccessionAcknowledgementSourceV2 {
    let predecessor = claimed(operation, None, 13, 10, 8);
    RuntimeDrainSuccessionAcknowledgementSourceV2::from_expired_route_absent_claimed(
        pending(operation, 20, Some(predecessor)),
        expectation,
    )
    .unwrap()
}

#[test]
fn inserted_and_replayed_receipts_bind_the_exact_operation_roots() {
    let operation = operation_for(DRAIN_INTENT_ID);
    let inserted_intent = RuntimeDrainIntentV2::from_inserted(&operation, non_zero(17)).unwrap();
    let inserted =
        RuntimeDrainIntentReceiptV2::inserted(&operation, inserted_intent.clone()).unwrap();
    assert_eq!(
        inserted.outcome(),
        RuntimeDrainIntentMutationOutcomeV2::Inserted
    );
    assert_eq!(inserted.intent(), &inserted_intent);

    let claimed_intent = pending(&operation, 18, Some(claimed(&operation, None, 13, 10, 8)));
    assert_eq!(
        RuntimeDrainIntentReceiptV2::inserted(&operation, claimed_intent.clone()),
        Err(RuntimeDrainIntentReceiptErrorV2::InitialStateMismatch)
    );

    let absent_claim = claimed(&operation, None, 13, 10, 8);
    let acknowledged_intent = acknowledged(&operation, 19, absent_claim);
    let persisted_root = root(&operation);
    let consumed_intent = RuntimeDrainIntentV2::consumed_from_persisted(
        &persisted_root,
        non_zero(20),
        DeploymentRevision::new(21).unwrap(),
        at(-100),
    )
    .unwrap();
    let cancelled_intent =
        RuntimeDrainIntentV2::cancelled_from_persisted(&persisted_root, non_zero(22), at(1_000))
            .unwrap();
    for replayable in [
        inserted_intent.clone(),
        claimed_intent.clone(),
        acknowledged_intent.clone(),
        consumed_intent.clone(),
        cancelled_intent.clone(),
    ] {
        let replayed =
            RuntimeDrainIntentReceiptV2::replayed(&operation, replayable.clone()).unwrap();
        assert_eq!(
            replayed.outcome(),
            RuntimeDrainIntentMutationOutcomeV2::Replayed
        );
        assert_eq!(replayed.intent(), &replayable);
    }

    for terminal in [acknowledged_intent, consumed_intent, cancelled_intent] {
        assert_eq!(
            RuntimeDrainIntentReceiptV2::inserted(&operation, terminal),
            Err(RuntimeDrainIntentReceiptErrorV2::InitialStateMismatch)
        );
    }

    let foreign = operation_for(FOREIGN_DRAIN_INTENT_ID);
    assert_eq!(
        RuntimeDrainIntentReceiptV2::inserted(&foreign, inserted_intent.clone()),
        Err(RuntimeDrainIntentReceiptErrorV2::OperationMismatch)
    );
    assert_eq!(
        RuntimeDrainIntentReceiptV2::replayed(&foreign, claimed_intent),
        Err(RuntimeDrainIntentReceiptErrorV2::OperationMismatch)
    );
}

#[test]
fn source_classifiers_accept_only_their_exact_mutable_states() {
    let operation = operation_for(DRAIN_INTENT_ID);
    let unclaimed = pending(&operation, 1, None);
    let absent_claim = claimed(&operation, None, 13, 10, 8);
    let routed_claim = claimed(&operation, Some(route(10, 5)), 13, 10, 8);
    let refenced_claim = refenced_from(&operation, &routed_claim, 14, 10);
    let absent = pending(&operation, 2, Some(absent_claim.clone()));
    let routed = pending(&operation, 2, Some(routed_claim));
    let refenced = pending(&operation, 3, Some(refenced_claim.clone()));
    let acknowledged = acknowledged(&operation, 4, refenced_claim);

    assert_eq!(
        RuntimeDrainRefenceSourceV2::from_claimed(unclaimed.clone()),
        Err(RuntimeDrainIntentReceiptErrorV2::SourceStateMismatch)
    );
    assert_eq!(
        RuntimeDrainRefenceSourceV2::from_claimed(absent.clone()),
        Err(RuntimeDrainIntentReceiptErrorV2::SourceStateMismatch)
    );
    assert_eq!(
        RuntimeDrainRefenceSourceV2::from_claimed(refenced.clone()),
        Err(RuntimeDrainIntentReceiptErrorV2::SourceStateMismatch)
    );
    assert_eq!(
        RuntimeDrainRefenceSourceV2::from_claimed(routed.clone())
            .unwrap()
            .source(),
        &routed
    );

    assert_eq!(
        RuntimeDrainAcknowledgementSourceV2::from_route_absence_candidate(unclaimed),
        Err(RuntimeDrainIntentReceiptErrorV2::SourceStateMismatch)
    );
    assert_eq!(
        RuntimeDrainAcknowledgementSourceV2::from_route_absence_candidate(routed),
        Err(RuntimeDrainIntentReceiptErrorV2::SourceStateMismatch)
    );
    assert_eq!(
        RuntimeDrainAcknowledgementSourceV2::from_route_absence_candidate(absent.clone())
            .unwrap()
            .source(),
        &absent
    );
    assert_eq!(
        RuntimeDrainAcknowledgementSourceV2::from_route_absence_candidate(refenced.clone())
            .unwrap()
            .source(),
        &refenced
    );

    assert_eq!(
        RuntimeRouteAbsentDrainIntentSourceV2::from_acknowledged(refenced),
        Err(RuntimeDrainIntentReceiptErrorV2::SourceStateMismatch)
    );
    assert_eq!(
        RuntimeRouteAbsentDrainIntentSourceV2::from_acknowledged(acknowledged.clone())
            .unwrap()
            .source(),
        &acknowledged
    );
}

#[test]
fn claim_replay_requires_an_exact_unchanged_claimed_aggregate() {
    let operation = operation_for(DRAIN_INTENT_ID);
    let source = pending(&operation, 29, Some(claimed(&operation, None, 13, 10, 8)));
    let receipt =
        RuntimeDrainIntentReceiptV2::claim_replayed(source.clone(), source.clone()).unwrap();
    assert_eq!(
        receipt.outcome(),
        RuntimeDrainIntentMutationOutcomeV2::Claimed
    );
    assert_eq!(receipt.intent(), &source);

    let changed_revision = pending(&operation, 30, Some(claimed(&operation, None, 13, 10, 8)));
    assert_eq!(
        RuntimeDrainIntentReceiptV2::claim_replayed(source, changed_revision),
        Err(RuntimeDrainIntentReceiptErrorV2::ClaimReplayMismatch)
    );

    let unclaimed = pending(&operation, 1, None);
    assert_eq!(
        RuntimeDrainIntentReceiptV2::claim_replayed(unclaimed.clone(), unclaimed),
        Err(RuntimeDrainIntentReceiptErrorV2::SourceStateMismatch)
    );
}

#[test]
fn refence_receipt_allows_only_progress_and_strictly_newer_database_revisions() {
    let operation = operation_for(DRAIN_INTENT_ID);
    let source_claim = claimed(&operation, Some(route(10, 5)), 13, 10, 8);
    let source_intent = pending(&operation, 20, Some(source_claim.clone()));
    let source = RuntimeDrainRefenceSourceV2::from_claimed(source_intent).unwrap();
    let maximum = i64::MAX as u64;
    let result_claim = refenced_from(&operation, &source_claim, maximum, 10);
    let result = pending(&operation, maximum, Some(result_claim.clone()));

    let receipt = RuntimeDrainIntentReceiptV2::refenced(&source, result.clone()).unwrap();
    assert_eq!(
        receipt.outcome(),
        RuntimeDrainIntentMutationOutcomeV2::Refenced
    );
    assert_eq!(receipt.intent(), &result);
    assert_eq!(
        receipt.intent().state().pending_claim(),
        Some(&result_claim)
    );
}

#[test]
fn refence_receipt_rejects_root_state_revision_identity_and_seal_drift() {
    let operation = operation_for(DRAIN_INTENT_ID);
    let source_claim = claimed(&operation, Some(route(10, 5)), 13, 10, 8);
    let source_intent = pending(&operation, 20, Some(source_claim.clone()));
    let source = RuntimeDrainRefenceSourceV2::from_claimed(source_intent.clone()).unwrap();

    let foreign = operation_for(FOREIGN_DRAIN_INTENT_ID);
    let foreign_claim = claimed(&foreign, Some(route(10, 5)), 13, 10, 8);
    let foreign_result = pending(
        &foreign,
        21,
        Some(refenced_from(&foreign, &foreign_claim, 14, 10)),
    );
    assert_eq!(
        RuntimeDrainIntentReceiptV2::refenced(&source, foreign_result),
        Err(RuntimeDrainIntentReceiptErrorV2::ImmutableRootMismatch)
    );

    assert_eq!(
        RuntimeDrainIntentReceiptV2::refenced(&source, source_intent),
        Err(RuntimeDrainIntentReceiptErrorV2::ResultStateMismatch)
    );

    let valid_refenced = refenced_from(&operation, &source_claim, 14, 10);
    assert_eq!(
        RuntimeDrainIntentReceiptV2::refenced(
            &source,
            pending(&operation, 20, Some(valid_refenced.clone())),
        ),
        Err(RuntimeDrainIntentReceiptErrorV2::IntentRevisionNotNewer)
    );
    assert_eq!(
        RuntimeDrainIntentReceiptV2::refenced(
            &source,
            pending(&operation, 19, Some(valid_refenced.clone())),
        ),
        Err(RuntimeDrainIntentReceiptErrorV2::IntentRevisionNotNewer)
    );
    assert_eq!(
        RuntimeDrainIntentReceiptV2::refenced(
            &source,
            pending(
                &operation,
                21,
                Some(refenced_from(&operation, &source_claim, 13, 10)),
            ),
        ),
        Err(RuntimeDrainIntentReceiptErrorV2::ClaimRevisionNotNewer)
    );
    assert_eq!(
        RuntimeDrainIntentReceiptV2::refenced(
            &source,
            pending(
                &operation,
                21,
                Some(refenced_from(&operation, &source_claim, 12, 10)),
            ),
        ),
        Err(RuntimeDrainIntentReceiptErrorV2::ClaimRevisionNotNewer)
    );

    let mut owner_drift = claim_identity_from(&source_claim);
    owner_drift.owner.gateway_shard_id = GatewayShardIdV1::parse("shard:1").unwrap();
    let mut observed_revision_drift = claim_identity_from(&source_claim);
    observed_revision_drift.observed_owner_revision = non_zero(99);
    let mut process_drift = claim_identity_from(&source_claim);
    process_drift.process_instance_id = ProcessInstanceId::parse("process:2").unwrap();
    process_drift.owner.process_instance_id = process_drift.process_instance_id.clone();
    let mut controller_drift = claim_identity_from(&source_claim);
    controller_drift.controller_id = ControllerId::parse("controller:2").unwrap();
    let mut fence_drift = claim_identity_from(&source_claim);
    fence_drift.controller_fencing_token = FencingToken::new(12).unwrap();
    let mut epoch_drift = claim_identity_from(&source_claim);
    epoch_drift.claim_epoch = non_zero(99);
    let mut expiry_drift = claim_identity_from(&source_claim);
    expiry_drift.expires_at = at(501);

    for (field, identity) in [
        ("gateway_owner_lease_id", owner_drift),
        ("observed_owner_revision", observed_revision_drift),
        ("process_instance_id", process_drift),
        ("controller_id", controller_drift),
        ("controller_fencing_token", fence_drift),
        ("claim_epoch", epoch_drift),
        ("expires_at", expiry_drift),
    ] {
        let drifted = refenced_with_identity(&operation, &source_claim, 14, 8, identity);
        assert_eq!(
            RuntimeDrainIntentReceiptV2::refenced(&source, pending(&operation, 21, Some(drifted)),),
            Err(RuntimeDrainIntentReceiptErrorV2::ClaimIdentityMismatch),
            "{field}"
        );
    }

    let different_seal_claim = claimed(&operation, Some(route(10, 5)), 13, 10, 80);
    let different_seal_result = refenced_from(&operation, &different_seal_claim, 14, 10);
    assert_eq!(
        RuntimeDrainIntentReceiptV2::refenced(
            &source,
            pending(&operation, 21, Some(different_seal_result)),
        ),
        Err(RuntimeDrainIntentReceiptErrorV2::ClaimProgressMismatch)
    );
}

#[test]
fn acknowledgement_receipt_accepts_initial_absence_and_durable_refence() {
    let operation = operation_for(DRAIN_INTENT_ID);
    let absent_claim = claimed(&operation, None, 13, 10, 8);
    let absent_source_intent = pending(&operation, 20, Some(absent_claim.clone()));
    let absent_source =
        RuntimeDrainAcknowledgementSourceV2::from_route_absence_candidate(absent_source_intent)
            .unwrap();
    let absent_result = acknowledged(&operation, 31, absent_claim);
    let absent_receipt =
        RuntimeDrainIntentReceiptV2::acknowledged(&absent_source, absent_result.clone()).unwrap();
    assert_eq!(
        absent_receipt.outcome(),
        RuntimeDrainIntentMutationOutcomeV2::Acknowledged
    );
    assert_eq!(absent_receipt.intent(), &absent_result);

    let routed_claim = claimed(&operation, Some(route(10, 5)), 13, 10, 8);
    let refenced_claim = refenced_from(&operation, &routed_claim, 14, 10);
    let refenced_source_intent = pending(&operation, 50, Some(refenced_claim.clone()));
    let refenced_source =
        RuntimeDrainAcknowledgementSourceV2::from_route_absence_candidate(refenced_source_intent)
            .unwrap();
    let refenced_result = acknowledged(&operation, 90, refenced_claim);
    assert!(RuntimeDrainIntentReceiptV2::acknowledged(&refenced_source, refenced_result).is_ok());
}

#[test]
fn acknowledgement_receipt_rejects_root_revision_state_and_claim_drift() {
    let operation = operation_for(DRAIN_INTENT_ID);
    let source_claim = claimed(&operation, None, 13, 10, 8);
    let source_intent = pending(&operation, 20, Some(source_claim.clone()));
    let source =
        RuntimeDrainAcknowledgementSourceV2::from_route_absence_candidate(source_intent.clone())
            .unwrap();

    assert_eq!(
        RuntimeDrainIntentReceiptV2::acknowledged(&source, source_intent),
        Err(RuntimeDrainIntentReceiptErrorV2::ResultStateMismatch)
    );
    assert_eq!(
        RuntimeDrainIntentReceiptV2::acknowledged(
            &source,
            acknowledged(&operation, 20, source_claim.clone()),
        ),
        Err(RuntimeDrainIntentReceiptErrorV2::IntentRevisionNotNewer)
    );
    assert_eq!(
        RuntimeDrainIntentReceiptV2::acknowledged(
            &source,
            acknowledged(&operation, 19, source_claim.clone()),
        ),
        Err(RuntimeDrainIntentReceiptErrorV2::IntentRevisionNotNewer)
    );
    assert_eq!(
        RuntimeDrainIntentReceiptV2::acknowledged(
            &source,
            acknowledged(&operation, 21, claimed(&operation, None, 14, 10, 8),),
        ),
        Err(RuntimeDrainIntentReceiptErrorV2::AcknowledgementMismatch)
    );

    let foreign = operation_for(FOREIGN_DRAIN_INTENT_ID);
    let foreign_claim = claimed(&foreign, None, 13, 10, 8);
    assert_eq!(
        RuntimeDrainIntentReceiptV2::acknowledged(
            &source,
            acknowledged(&foreign, 21, foreign_claim),
        ),
        Err(RuntimeDrainIntentReceiptErrorV2::ImmutableRootMismatch)
    );
}

#[test]
fn transition_receipts_accept_canonical_timestamps_without_host_clock_ordering() {
    let operation = operation_for(DRAIN_INTENT_ID);
    let mut identity = standard_claim_identity();
    identity.expires_at = at(-500);
    let source_claim = claimed_with_identity(&operation, Some(route(10, 5)), 13, 8, identity);
    let source_intent = pending(&operation, 20, Some(source_claim.clone()));
    let refence_source = RuntimeDrainRefenceSourceV2::from_claimed(source_intent).unwrap();
    let refenced_claim = refenced_from(&operation, &source_claim, 14, 10);
    let refenced_intent = pending(&operation, 21, Some(refenced_claim.clone()));
    assert!(
        RuntimeDrainIntentReceiptV2::refenced(&refence_source, refenced_intent.clone()).is_ok()
    );

    let acknowledgement_source =
        RuntimeDrainAcknowledgementSourceV2::from_route_absence_candidate(refenced_intent).unwrap();
    let acknowledgement = acknowledgement_at(&operation, refenced_claim, at(-1_000));
    let acknowledged_intent = RuntimeDrainIntentV2::route_absent_acknowledged_from_persisted(
        &root(&operation),
        non_zero(22),
        acknowledgement,
    )
    .unwrap();
    assert!(RuntimeDrainIntentReceiptV2::acknowledged(
        &acknowledgement_source,
        acknowledged_intent,
    )
    .is_ok());
}

#[test]
fn succession_receipt_accepts_only_the_exact_atomic_successor() {
    let operation = operation_for(DRAIN_INTENT_ID);
    let expectation = succession_expectation();
    let source = succession_source(&operation, expectation.clone());
    let successor = succession_claim(&operation, &expectation, 14, 12, 28, 29);
    let result = succession_result(&operation, 21, successor, &expectation);
    let receipt =
        RuntimeDrainIntentReceiptV2::succession_acknowledged(&source, result.clone()).unwrap();

    assert_eq!(
        receipt.outcome(),
        RuntimeDrainIntentMutationOutcomeV2::Acknowledged
    );
    assert_eq!(receipt.intent(), &result);
}

#[test]
fn succession_receipt_rejects_root_state_and_intent_revision_drift() {
    let operation = operation_for(DRAIN_INTENT_ID);
    let expectation = succession_expectation();
    let source = succession_source(&operation, expectation.clone());

    assert_eq!(
        RuntimeDrainIntentReceiptV2::succession_acknowledged(
            &source,
            pending(
                &operation,
                21,
                Some(succession_claim(&operation, &expectation, 14, 12, 28, 29)),
            ),
        ),
        Err(RuntimeDrainIntentReceiptErrorV2::ResultStateMismatch)
    );
    for revision in [20, 22] {
        assert_eq!(
            RuntimeDrainIntentReceiptV2::succession_acknowledged(
                &source,
                succession_result(
                    &operation,
                    revision,
                    succession_claim(&operation, &expectation, 14, 12, 28, 29),
                    &expectation,
                ),
            ),
            Err(RuntimeDrainIntentReceiptErrorV2::SuccessionIntentRevisionMismatch)
        );
    }

    let foreign = operation_for(FOREIGN_DRAIN_INTENT_ID);
    assert_eq!(
        RuntimeDrainIntentReceiptV2::succession_acknowledged(
            &source,
            succession_result(
                &foreign,
                21,
                succession_claim(&foreign, &expectation, 14, 12, 28, 29),
                &expectation,
            ),
        ),
        Err(RuntimeDrainIntentReceiptErrorV2::ImmutableRootMismatch)
    );
}

#[test]
fn succession_receipt_rejects_claim_revision_fence_and_identity_drift() {
    let operation = operation_for(DRAIN_INTENT_ID);
    let expectation = succession_expectation();
    let source = succession_source(&operation, expectation.clone());

    assert_eq!(
        RuntimeDrainIntentReceiptV2::succession_acknowledged(
            &source,
            acknowledged(&operation, 21, claimed(&operation, None, 13, 10, 8)),
        ),
        Err(RuntimeDrainIntentReceiptErrorV2::SuccessionClaimMismatch)
    );
    assert_eq!(
        RuntimeDrainIntentReceiptV2::succession_acknowledged(
            &source,
            succession_result(
                &operation,
                21,
                succession_claim(&operation, &expectation, 15, 12, 28, 29),
                &expectation,
            ),
        ),
        Err(RuntimeDrainIntentReceiptErrorV2::SuccessionClaimRevisionMismatch)
    );
    assert_eq!(
        RuntimeDrainIntentReceiptV2::succession_acknowledged(
            &source,
            succession_result(
                &operation,
                21,
                succession_claim(&operation, &expectation, 14, 13, 28, 29),
                &expectation,
            ),
        ),
        Err(RuntimeDrainIntentReceiptErrorV2::SuccessionFenceMismatch)
    );

    let mut identity_drift = expectation.clone();
    identity_drift.controller_id = ControllerId::parse("controller:3").unwrap();
    assert_eq!(
        RuntimeDrainIntentReceiptV2::succession_acknowledged(
            &source,
            succession_result(
                &operation,
                21,
                succession_claim(&operation, &identity_drift, 14, 12, 28, 29),
                &expectation,
            ),
        ),
        Err(RuntimeDrainIntentReceiptErrorV2::SuccessionClaimMismatch)
    );
}

#[test]
fn succession_receipt_rejects_seal_provenance_acknowledgement_and_certification_drift() {
    let operation = operation_for(DRAIN_INTENT_ID);
    let expectation = succession_expectation();
    let source = succession_source(&operation, expectation.clone());

    for (seal_generation, seal_observation) in [(31, 29), (28, 31)] {
        assert_eq!(
            RuntimeDrainIntentReceiptV2::succession_acknowledged(
                &source,
                succession_result(
                    &operation,
                    21,
                    succession_claim(
                        &operation,
                        &expectation,
                        14,
                        12,
                        seal_generation,
                        seal_observation,
                    ),
                    &expectation,
                ),
            ),
            Err(RuntimeDrainIntentReceiptErrorV2::SuccessionSealMismatch)
        );
    }

    let successor = || succession_claim(&operation, &expectation, 14, 12, 28, 29);
    let mut provenance_drift = expectation.clone();
    provenance_drift.recovery_witness.recovery_id =
        RuntimeRecoveryIdV2::parse("3333444455556666777788889999aaaa").unwrap();
    assert_eq!(
        RuntimeDrainIntentReceiptV2::succession_acknowledged(
            &source,
            succession_result(&operation, 21, successor(), &provenance_drift),
        ),
        Err(RuntimeDrainIntentReceiptErrorV2::SuccessionAcknowledgementMismatch)
    );

    let mut observation_drift = expectation.clone();
    observation_drift.acknowledgement_observation_sequence = non_zero(31);
    assert_eq!(
        RuntimeDrainIntentReceiptV2::succession_acknowledged(
            &source,
            succession_result(&operation, 21, successor(), &observation_drift),
        ),
        Err(RuntimeDrainIntentReceiptErrorV2::SuccessionAcknowledgementMismatch)
    );

    let mut timestamp_drift = expectation.clone();
    timestamp_drift.acknowledged_at = at(502);
    assert_eq!(
        RuntimeDrainIntentReceiptV2::succession_acknowledged(
            &source,
            succession_result(&operation, 21, successor(), &timestamp_drift),
        ),
        Err(RuntimeDrainIntentReceiptErrorV2::SuccessionAcknowledgementMismatch)
    );

    let mut certification_drift = expectation.clone();
    certification_drift.certification =
        RuntimeDrainCertificationResolutionV2::no_attestation_for_reserved_operation(
            RuntimeCertificationOperationIdV2::parse("444455556666777788889999aaaabbbb").unwrap(),
            RuntimeCertificationIntentFingerprintV2::parse("f".repeat(64)).unwrap(),
        );
    assert_eq!(
        RuntimeDrainIntentReceiptV2::succession_acknowledged(
            &source,
            succession_result(&operation, 21, successor(), &certification_drift),
        ),
        Err(RuntimeDrainIntentReceiptErrorV2::SuccessionCertificationMismatch)
    );
}

#[test]
fn receipt_surface_is_closed_data_without_claim_or_terminal_authority() {
    let source = include_str!("../v2_drain_intent_receipt.rs");

    for forbidden in [
        "serde",
        "Serialize",
        "Deserialize",
        "Default",
        "sqlx",
        "rusqlite",
        "twilight",
        "Authority",
        "Port",
        "Permit",
        "Authorized",
        "Instant",
        "next_intent_revision",
        "impl Future",
        "async fn",
        "pub fn new(",
        "pub fn from_result(",
        "pub fn claimed(",
        "pub fn claim_initial(",
        "pub fn claim_successor(",
        "pub fn consumed(",
        "pub fn cancelled(",
        "pub outcome:",
        "pub intent:",
        "pub source:",
    ] {
        assert!(!source.contains(forbidden), "{forbidden}");
    }
    for declaration in [
        "pub enum RuntimeDrainIntentMutationOutcomeV2",
        "pub struct RuntimeDrainRefenceSourceV2",
        "pub struct RuntimeDrainAcknowledgementSourceV2",
        "pub struct RuntimeDrainSuccessionAcknowledgementExpectationV2",
        "pub struct RuntimeDrainSuccessionAcknowledgementSourceV2",
        "pub struct RuntimeRouteAbsentDrainIntentSourceV2",
        "pub struct RuntimeDrainIntentReceiptV2",
        "pub fn from_expired_route_absent_claimed(",
        "pub fn inserted(",
        "pub fn replayed(",
        "pub fn claim_replayed(",
        "pub fn refenced(",
        "pub fn acknowledged(",
        "pub fn succession_acknowledged(",
    ] {
        assert!(source.contains(declaration), "{declaration}");
    }

    let outcomes = [
        RuntimeDrainIntentMutationOutcomeV2::Inserted,
        RuntimeDrainIntentMutationOutcomeV2::Replayed,
        RuntimeDrainIntentMutationOutcomeV2::Claimed,
        RuntimeDrainIntentMutationOutcomeV2::Refenced,
        RuntimeDrainIntentMutationOutcomeV2::Acknowledged,
        RuntimeDrainIntentMutationOutcomeV2::Consumed,
        RuntimeDrainIntentMutationOutcomeV2::Cancelled,
    ];
    assert_eq!(outcomes.len(), 7);

    let operation = operation_for(DRAIN_INTENT_ID);
    let acknowledged = acknowledged(&operation, 2, claimed(&operation, None, 13, 10, 8));
    let source = RuntimeRouteAbsentDrainIntentSourceV2::from_acknowledged(acknowledged).unwrap();
    let cloned = source.clone();
    assert_eq!(cloned, source);
    assert_eq!(
        source.source().state().resulting_revision(),
        None::<DeploymentRevision>
    );
}
