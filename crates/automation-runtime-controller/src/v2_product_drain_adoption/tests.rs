use std::num::NonZeroU64;

use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    ActivationRequestId, BindingRevision, ControllerId, DeploymentId, DeploymentRevision,
    FencingToken, InstallationId, ProcessInstanceId, PromotionId, RuntimeDeployment,
    RuntimeDeploymentIdentityV1, RuntimeDeploymentPhaseV1, RuntimeDeploymentSnapshotV1,
    RuntimeDeploymentTargetV1, RuntimeGeneration, TenantId,
};
use chrono::{DateTime, Utc};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;

use super::{
    RuntimeProductDrainAdoptionDivergenceV2, RuntimeProductDrainAdoptionErrorV2,
    RuntimeProductDrainAdoptionKindV2, RuntimeProductDrainAdoptionV2,
    RuntimeProductDrainSemanticExpectationV2, RuntimeProductDrainSemanticFieldV2,
};
use crate::{
    GatewayShardIdV1, RuntimeBarrierIdV1, RuntimeBarrierPauseWitnessV2, RuntimeBuildRevisionV1,
    RuntimeCanonicalProductDrainV2, RuntimeDrainCertificationResolutionV2,
    RuntimeDrainClaimProgressV2, RuntimeDrainClaimSealWitnessV2, RuntimeDrainClaimV2,
    RuntimeDrainIntentIdV2, RuntimeDrainIntentV2, RuntimeGatewayAdmissionSequenceV2,
    RuntimeGatewayOwnerLeaseIdV1, RuntimeObservedProductDrainV2,
    RuntimePersistedProductDrainRootV2, RuntimeProductDrainNaturalScopeV2,
    RuntimeProductDrainOperationBuildErrorV2, RuntimeProductDrainOperationV2,
    RuntimeProductDrainScopeCorruptionV2, RuntimeProductDrainScopeLookupV2,
    RuntimeProductDrainScopeObservationV2, RuntimeProductMutationKindV2,
    RuntimeProductMutationPreimageV2, RuntimeProductOperationIdV2,
    RuntimeProductSemanticRequestDigestV2, RuntimeRouteAbsentAcknowledgementV2,
    RuntimeRouteMutationProvenanceV2, RuntimeServingSlotV2,
};

const PRODUCT_OPERATION_ID: &str = "00112233445566778899aabbccddeeff";
const DRAIN_INTENT_ID: &str = "ffeeddccbbaa99887766554433221100";
const OTHER_PRODUCT_OPERATION_ID: &str = "11112233445566778899aabbccddeeff";
const OTHER_DRAIN_INTENT_ID: &str = "22222222333344445555666677778888";

fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn at(second: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(second, 0).unwrap()
}

fn target(ruleset: &str, version: u32, content: char) -> RuntimeDeploymentTargetV1 {
    RuntimeDeploymentTargetV1 {
        guild_id: GuildId(7),
        ruleset_key: RuleSetKey::parse(ruleset).unwrap(),
        version: RuleSetVersionId::new(version).unwrap(),
        content_hash: RuleSetContentHash::parse_hex(&content.to_string().repeat(64)).unwrap(),
        binding_revision: BindingRevision::new(3).unwrap(),
        binding_fingerprint: ResourceBindingFingerprint::parse(&"a".repeat(64)).unwrap(),
    }
}

fn identity(deployment: &str) -> RuntimeDeploymentIdentityV1 {
    RuntimeDeploymentIdentityV1 {
        deployment_id: DeploymentId::parse(deployment).unwrap(),
        tenant_id: TenantId::parse("tenant:1").unwrap(),
        installation_id: InstallationId::parse("installation:1").unwrap(),
        promotion_id: PromotionId::parse("c".repeat(64)).unwrap(),
        activation_request_id: ActivationRequestId::parse("activation:1").unwrap(),
    }
}

fn snapshot_with(
    deployment: &str,
    target: RuntimeDeploymentTargetV1,
) -> RuntimeDeploymentSnapshotV1 {
    RuntimeDeployment::request(
        identity(deployment),
        target,
        RuntimeGeneration::new(4).unwrap(),
        None,
        at(100),
    )
    .unwrap()
    .snapshot()
}

fn snapshot() -> RuntimeDeploymentSnapshotV1 {
    snapshot_with("deployment:1", target("studyroom", 1, 'b'))
}

fn semantic_digest(value: char) -> RuntimeProductSemanticRequestDigestV2 {
    RuntimeProductSemanticRequestDigestV2::parse(value.to_string().repeat(64)).unwrap()
}

fn operation_with(
    snapshot: &RuntimeDeploymentSnapshotV1,
    operation_id: &str,
    intent_id: &str,
    mutation_kind: RuntimeProductMutationKindV2,
    semantic: char,
) -> RuntimeProductDrainOperationV2 {
    let product = RuntimeProductMutationPreimageV2 {
        operation_id: RuntimeProductOperationIdV2::parse(operation_id).unwrap(),
        scope: crate::RuntimeDeploymentScopeV1::from_identity(&snapshot.identity),
        expected_revision: snapshot.revision,
        slot: RuntimeServingSlotV2::from_target(&snapshot.target),
        expected_target: snapshot.target.clone(),
        mutation_kind,
        product_semantic_request_digest: semantic_digest(semantic),
    };
    let canonical = RuntimeCanonicalProductDrainV2::new(
        product,
        RuntimeDrainIntentIdV2::parse(intent_id).unwrap(),
    )
    .unwrap();
    RuntimeProductDrainOperationV2::new(snapshot, canonical).unwrap()
}

fn operation() -> RuntimeProductDrainOperationV2 {
    operation_with(
        &snapshot(),
        PRODUCT_OPERATION_ID,
        DRAIN_INTENT_ID,
        RuntimeProductMutationKindV2::AuthorityChange,
        'd',
    )
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

fn process_id() -> ProcessInstanceId {
    ProcessInstanceId::parse("process:1").unwrap()
}

fn owner() -> RuntimeGatewayOwnerLeaseIdV1 {
    RuntimeGatewayOwnerLeaseIdV1 {
        gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        process_instance_id: process_id(),
        lease_epoch: non_zero(3),
        expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
    }
}

fn claim(operation: &RuntimeProductDrainOperationV2) -> RuntimeDrainClaimV2 {
    let key = &operation.canonical().drain_preimage().key;
    let seal =
        RuntimeDrainClaimSealWitnessV2::new(key, process_id(), non_zero(8), None, non_zero(9))
            .unwrap();
    RuntimeDrainClaimV2::new(
        key,
        owner(),
        non_zero(10),
        process_id(),
        ControllerId::parse("controller:1").unwrap(),
        FencingToken::new(11).unwrap(),
        non_zero(12),
        non_zero(13),
        at(500),
        RuntimeDrainClaimProgressV2::claimed(seal),
    )
    .unwrap()
}

fn acknowledgement(
    operation: &RuntimeProductDrainOperationV2,
) -> RuntimeRouteAbsentAcknowledgementV2 {
    RuntimeRouteAbsentAcknowledgementV2::new(
        &operation.canonical().drain_preimage().key,
        claim(operation),
        None,
        RuntimeRouteMutationProvenanceV2::Ordinary {
            barrier_id: RuntimeBarrierIdV1::parse("9999aaaabbbbccccddddeeeeffff0000").unwrap(),
            pause: RuntimeBarrierPauseWitnessV2 {
                coordinator_generation: non_zero(4),
                connection_epoch: non_zero(5),
                paused_admission_revision: non_zero(6),
                pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(7)),
            },
        },
        non_zero(9),
        RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
        at(300),
    )
    .unwrap()
}

fn observed(
    root: RuntimePersistedProductDrainRootV2,
    intent: RuntimeDrainIntentV2,
) -> RuntimeObservedProductDrainV2 {
    RuntimeObservedProductDrainV2::from_exact_parts(root, intent).unwrap()
}

fn observed_states(
    operation: &RuntimeProductDrainOperationV2,
) -> Vec<RuntimeObservedProductDrainV2> {
    let persisted_root = root(operation);
    vec![
        observed(
            persisted_root.clone(),
            RuntimeDrainIntentV2::pending_from_persisted(&persisted_root, non_zero(10), None)
                .unwrap(),
        ),
        observed(
            persisted_root.clone(),
            RuntimeDrainIntentV2::route_absent_acknowledged_from_persisted(
                &persisted_root,
                non_zero(11),
                acknowledgement(operation),
            )
            .unwrap(),
        ),
        observed(
            persisted_root.clone(),
            RuntimeDrainIntentV2::consumed_from_persisted(
                &persisted_root,
                non_zero(12),
                DeploymentRevision::new(13).unwrap(),
                at(-100),
            )
            .unwrap(),
        ),
        observed(
            persisted_root.clone(),
            RuntimeDrainIntentV2::cancelled_from_persisted(
                &persisted_root,
                non_zero(14),
                at(1_000),
            )
            .unwrap(),
        ),
    ]
}

fn present_observation(
    snapshot: &RuntimeDeploymentSnapshotV1,
    operation: &RuntimeProductDrainOperationV2,
    persisted: RuntimeObservedProductDrainV2,
) -> RuntimeProductDrainScopeObservationV2 {
    RuntimeProductDrainScopeObservationV2::present(
        operation.scope_lookup(),
        snapshot.clone(),
        persisted,
        at(200),
    )
    .unwrap()
}

fn expectation(snapshot: &RuntimeDeploymentSnapshotV1) -> RuntimeProductDrainSemanticExpectationV2 {
    RuntimeProductDrainSemanticExpectationV2::from_locked_snapshot(
        snapshot,
        RuntimeProductMutationKindV2::AuthorityChange,
        semantic_digest('d'),
    )
    .unwrap()
}

#[test]
fn semantic_expectation_is_id_free_and_derives_only_locked_product_inputs() {
    let snapshot = snapshot();
    let proposed = operation();
    let locked = expectation(&snapshot);
    let proposed_expectation = RuntimeProductDrainSemanticExpectationV2::from_proposed(&proposed);

    assert_eq!(locked, proposed_expectation);
    assert_eq!(locked.lookup(), &proposed.scope_lookup());
    assert_eq!(locked.expected_target(), &snapshot.target);
    assert_eq!(
        locked.mutation_kind(),
        RuntimeProductMutationKindV2::AuthorityChange
    );
    assert_eq!(
        locked.product_semantic_request_digest(),
        &semantic_digest('d')
    );

    let mut invalid = snapshot;
    invalid.phase = RuntimeDeploymentPhaseV1::Live;
    assert_eq!(
        RuntimeProductDrainSemanticExpectationV2::from_locked_snapshot(
            &invalid,
            RuntimeProductMutationKindV2::AuthorityChange,
            semantic_digest('d'),
        ),
        Err(RuntimeProductDrainOperationBuildErrorV2::InvalidSnapshot)
    );
}

#[test]
fn exact_proposed_root_classification_preserves_the_complete_observation() {
    let snapshot = snapshot();
    let proposed = operation();
    let persisted = observed(
        root(&proposed),
        RuntimeDrainIntentV2::from_inserted(&proposed, non_zero(17)).unwrap(),
    );
    let observation = present_observation(&snapshot, &proposed, persisted.clone());
    let expected_observation = observation.clone();
    let adoption =
        RuntimeProductDrainAdoptionV2::classify_proposed(&proposed, observation).unwrap();

    assert_eq!(
        adoption.kind(),
        RuntimeProductDrainAdoptionKindV2::ExactProposedRoot
    );
    assert_eq!(
        adoption.expectation(),
        &RuntimeProductDrainSemanticExpectationV2::from_proposed(&proposed)
    );
    assert_eq!(adoption.observation(), &expected_observation);
    assert_eq!(adoption.persisted(), Some(&persisted));
    assert!(adoption.divergence().is_none());
    assert_eq!(adoption.into_observation(), expected_observation);
}

#[test]
fn same_semantics_with_different_proposed_ids_is_canonical_mismatch() {
    let snapshot = snapshot();
    let persisted_operation = operation();
    let persisted = observed(
        root(&persisted_operation),
        RuntimeDrainIntentV2::from_inserted(&persisted_operation, non_zero(17)).unwrap(),
    );
    let observation = present_observation(&snapshot, &persisted_operation, persisted);
    for (product_operation_id, drain_intent_id) in [
        (OTHER_PRODUCT_OPERATION_ID, DRAIN_INTENT_ID),
        (PRODUCT_OPERATION_ID, OTHER_DRAIN_INTENT_ID),
        (OTHER_PRODUCT_OPERATION_ID, OTHER_DRAIN_INTENT_ID),
    ] {
        let proposed = operation_with(
            &snapshot,
            product_operation_id,
            drain_intent_id,
            RuntimeProductMutationKindV2::AuthorityChange,
            'd',
        );
        let adoption =
            RuntimeProductDrainAdoptionV2::classify_proposed(&proposed, observation.clone())
                .unwrap();

        assert_eq!(adoption.kind(), RuntimeProductDrainAdoptionKindV2::Diverged);
        assert_eq!(
            adoption.divergence(),
            Some(RuntimeProductDrainAdoptionDivergenceV2::CanonicalMismatch)
        );
        assert!(adoption.persisted().is_none());
        assert_eq!(
            adoption
                .observation()
                .persisted()
                .unwrap()
                .intent()
                .key()
                .intent_id
                .as_str(),
            DRAIN_INTENT_ID
        );
    }
}

#[test]
fn proposed_path_reports_semantic_mismatch_before_canonical_mismatch() {
    let snapshot = snapshot();
    let persisted_operation = operation();
    let persisted = observed(
        root(&persisted_operation),
        RuntimeDrainIntentV2::from_inserted(&persisted_operation, non_zero(17)).unwrap(),
    );
    let observation = present_observation(&snapshot, &persisted_operation, persisted);
    let other_target_snapshot = snapshot_with("deployment:1", target("studyroom", 2, 'e'));
    let cases = [
        (
            operation_with(
                &other_target_snapshot,
                PRODUCT_OPERATION_ID,
                DRAIN_INTENT_ID,
                RuntimeProductMutationKindV2::AuthorityChange,
                'd',
            ),
            RuntimeProductDrainSemanticFieldV2::ExpectedTarget,
        ),
        (
            operation_with(
                &snapshot,
                PRODUCT_OPERATION_ID,
                DRAIN_INTENT_ID,
                RuntimeProductMutationKindV2::Teardown,
                'd',
            ),
            RuntimeProductDrainSemanticFieldV2::MutationKind,
        ),
        (
            operation_with(
                &snapshot,
                PRODUCT_OPERATION_ID,
                DRAIN_INTENT_ID,
                RuntimeProductMutationKindV2::AuthorityChange,
                'e',
            ),
            RuntimeProductDrainSemanticFieldV2::ProductSemanticRequestDigest,
        ),
    ];

    for (proposed, field) in cases {
        let adoption =
            RuntimeProductDrainAdoptionV2::classify_proposed(&proposed, observation.clone())
                .unwrap();
        assert_eq!(adoption.kind(), RuntimeProductDrainAdoptionKindV2::Diverged);
        assert_eq!(
            adoption.divergence(),
            Some(RuntimeProductDrainAdoptionDivergenceV2::SemanticMismatch { field })
        );
        assert!(adoption.persisted().is_none());
    }
}

#[test]
fn semantic_recovery_adopts_persisted_ids_roots_and_all_current_states() {
    let snapshot = snapshot();
    let persisted_operation = operation();
    let expectation = expectation(&snapshot);

    for persisted in observed_states(&persisted_operation) {
        let expected = persisted.clone();
        let observation = present_observation(&snapshot, &persisted_operation, persisted);
        let adoption = RuntimeProductDrainAdoptionV2::classify_semantic_recovery(
            expectation.clone(),
            observation,
        )
        .unwrap();
        assert_eq!(
            adoption.kind(),
            RuntimeProductDrainAdoptionKindV2::PersistedRoot
        );
        assert_eq!(adoption.persisted(), Some(&expected));
        assert_eq!(
            adoption
                .persisted()
                .unwrap()
                .intent()
                .key()
                .intent_id
                .as_str(),
            DRAIN_INTENT_ID
        );
        assert_eq!(
            adoption
                .persisted()
                .unwrap()
                .root()
                .product_operation_id()
                .as_str(),
            PRODUCT_OPERATION_ID
        );
        assert_eq!(
            adoption.persisted().unwrap().intent().canonical(),
            expected.root().canonical()
        );
        assert!(adoption.divergence().is_none());
    }
}

#[test]
fn semantic_recovery_reports_each_id_independent_mismatch_precisely() {
    let snapshot = snapshot();
    let persisted_operation = operation();
    let persisted = observed(
        root(&persisted_operation),
        RuntimeDrainIntentV2::from_inserted(&persisted_operation, non_zero(17)).unwrap(),
    );

    let other_target_snapshot = snapshot_with("deployment:1", target("studyroom", 2, 'e'));
    let cases = [
        (
            RuntimeProductDrainSemanticExpectationV2::from_locked_snapshot(
                &other_target_snapshot,
                RuntimeProductMutationKindV2::AuthorityChange,
                semantic_digest('d'),
            )
            .unwrap(),
            RuntimeProductDrainSemanticFieldV2::ExpectedTarget,
        ),
        (
            RuntimeProductDrainSemanticExpectationV2::from_locked_snapshot(
                &snapshot,
                RuntimeProductMutationKindV2::Teardown,
                semantic_digest('d'),
            )
            .unwrap(),
            RuntimeProductDrainSemanticFieldV2::MutationKind,
        ),
        (
            RuntimeProductDrainSemanticExpectationV2::from_locked_snapshot(
                &snapshot,
                RuntimeProductMutationKindV2::AuthorityChange,
                semantic_digest('e'),
            )
            .unwrap(),
            RuntimeProductDrainSemanticFieldV2::ProductSemanticRequestDigest,
        ),
    ];

    for (expectation, field) in cases {
        let observation = present_observation(&snapshot, &persisted_operation, persisted.clone());
        let adoption =
            RuntimeProductDrainAdoptionV2::classify_semantic_recovery(expectation, observation)
                .unwrap();
        assert_eq!(adoption.kind(), RuntimeProductDrainAdoptionKindV2::Diverged);
        assert_eq!(
            adoption.divergence(),
            Some(RuntimeProductDrainAdoptionDivergenceV2::SemanticMismatch { field })
        );
        assert!(adoption.persisted().is_none());
    }
}

#[test]
fn classification_rejects_an_observation_from_another_natural_lookup() {
    let snapshot = snapshot();
    let other_snapshot = snapshot_with("deployment:2", target("studyroom", 1, 'b'));
    let other_operation = operation_with(
        &other_snapshot,
        PRODUCT_OPERATION_ID,
        DRAIN_INTENT_ID,
        RuntimeProductMutationKindV2::AuthorityChange,
        'd',
    );
    let observation = RuntimeProductDrainScopeObservationV2::absent(
        other_operation.scope_lookup(),
        other_snapshot,
        at(1),
    )
    .unwrap();

    assert_eq!(
        RuntimeProductDrainAdoptionV2::classify_semantic_recovery(
            expectation(&snapshot),
            observation,
        ),
        Err(RuntimeProductDrainAdoptionErrorV2::ObservationLookupMismatch)
    );
}

#[test]
fn absent_observation_stays_absent_in_both_classification_paths() {
    let snapshot = snapshot();
    let proposed = operation();
    let observation = RuntimeProductDrainScopeObservationV2::absent(
        proposed.scope_lookup(),
        snapshot.clone(),
        at(1),
    )
    .unwrap();

    let proposed_adoption =
        RuntimeProductDrainAdoptionV2::classify_proposed(&proposed, observation.clone()).unwrap();
    assert_eq!(
        proposed_adoption.kind(),
        RuntimeProductDrainAdoptionKindV2::Absent
    );
    assert!(proposed_adoption.persisted().is_none());
    assert!(proposed_adoption.divergence().is_none());

    let recovery = RuntimeProductDrainAdoptionV2::classify_semantic_recovery(
        expectation(&snapshot),
        observation,
    )
    .unwrap();
    assert_eq!(recovery.kind(), RuntimeProductDrainAdoptionKindV2::Absent);
    assert!(recovery.persisted().is_none());
    assert!(recovery.divergence().is_none());
}

#[test]
fn every_physical_corruption_reason_is_preserved_as_divergence() {
    let snapshot = snapshot();
    let lookup = RuntimeProductDrainScopeLookupV2::from_locked_snapshot(&snapshot).unwrap();
    let reasons = [
        RuntimeProductDrainScopeCorruptionV2::Ambiguous(
            RuntimeProductDrainNaturalScopeV2::ProductOperation,
        ),
        RuntimeProductDrainScopeCorruptionV2::Ambiguous(
            RuntimeProductDrainNaturalScopeV2::DrainIntent,
        ),
        RuntimeProductDrainScopeCorruptionV2::PartialPair {
            present: RuntimeProductDrainNaturalScopeV2::ProductOperation,
        },
        RuntimeProductDrainScopeCorruptionV2::PartialPair {
            present: RuntimeProductDrainNaturalScopeV2::DrainIntent,
        },
        RuntimeProductDrainScopeCorruptionV2::PairMismatch,
        RuntimeProductDrainScopeCorruptionV2::PersistedRootInvalid,
        RuntimeProductDrainScopeCorruptionV2::PersistedIntentInvalid,
    ];

    for corruption in reasons {
        let observation = RuntimeProductDrainScopeObservationV2::persistence_corrupt(
            lookup.clone(),
            snapshot.clone(),
            corruption,
            at(1),
        )
        .unwrap();
        let adoption = RuntimeProductDrainAdoptionV2::classify_semantic_recovery(
            expectation(&snapshot),
            observation,
        )
        .unwrap();
        assert_eq!(adoption.kind(), RuntimeProductDrainAdoptionKindV2::Diverged);
        assert_eq!(
            adoption.divergence(),
            Some(RuntimeProductDrainAdoptionDivergenceV2::PersistenceCorrupt { corruption },)
        );
        assert!(adoption.persisted().is_none());
    }
}

#[test]
fn semantic_adoption_surface_is_inert_id_free_and_non_authorizing() {
    let source = include_str!("../v2_product_drain_adoption.rs");
    let expectation = source
        .split("pub struct RuntimeProductDrainSemanticExpectationV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();

    for forbidden in [
        "RuntimeProductOperationIdV2",
        "RuntimeDrainIntentIdV2",
        "RuntimeProductMutationDigestV2",
        "RuntimeDrainIntentDigestV2",
        "RuntimeCanonicalProductDrainV2",
        "RuntimePersistedProductDrainRootV2",
        "request_bytes",
        "canonical",
    ] {
        assert!(!expectation.contains(forbidden), "{forbidden}");
    }
    for required in [
        "lookup: RuntimeProductDrainScopeLookupV2",
        "expected_target: RuntimeDeploymentTargetV1",
        "mutation_kind: RuntimeProductMutationKindV2",
        "product_semantic_request_digest: RuntimeProductSemanticRequestDigestV2",
    ] {
        assert!(expectation.contains(required), "{required}");
    }

    for forbidden in [
        "serde",
        "Serialize",
        "Deserialize",
        "Default",
        "sqlx",
        "rusqlite",
        "twilight",
        "Port",
        "Authority",
        "Permit",
        "Authorized",
        "impl Future",
        "async fn",
        "pub fn new(",
        "retry",
        "Retry",
        "transaction",
        "Transaction",
        "SystemTime",
        "Instant",
        "Utc::now",
        "rand",
        "Uuid",
        "CSPRNG",
    ] {
        assert!(!source.contains(forbidden), "{forbidden}");
    }
    for declaration in [
        "pub struct RuntimeProductDrainSemanticExpectationV2",
        "pub enum RuntimeProductDrainAdoptionKindV2",
        "pub enum RuntimeProductDrainSemanticFieldV2",
        "pub enum RuntimeProductDrainAdoptionDivergenceV2",
        "pub struct RuntimeProductDrainAdoptionV2",
        "pub fn from_locked_snapshot(",
        "pub fn from_proposed(",
        "pub fn classify_proposed(",
        "pub fn classify_semantic_recovery(",
        "pub fn into_observation(",
    ] {
        assert!(source.contains(declaration), "{declaration}");
    }
}
