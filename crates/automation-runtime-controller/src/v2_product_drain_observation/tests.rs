use std::num::NonZeroU64;

use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    ActivationRequestId, BindingRevision, ControllerId, DeploymentId, DeploymentRevision,
    FencingToken, InstallationId, LeaseRequestV1, ProcessInstanceId, PromotionId,
    RuntimeDeployment, RuntimeDeploymentIdentityV1, RuntimeDeploymentPhaseV1,
    RuntimeDeploymentSnapshotV1, RuntimeDeploymentTargetV1, RuntimeGeneration, TenantId,
};
use chrono::{DateTime, Utc};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;

use super::{
    RuntimeObservedProductDrainV2, RuntimeProductDrainNaturalScopeV2,
    RuntimeProductDrainScopeCorruptionV2, RuntimeProductDrainScopeObservationErrorV2,
    RuntimeProductDrainScopeObservationFieldV2, RuntimeProductDrainScopeObservationKindV2,
    RuntimeProductDrainScopeObservationV2,
};
use crate::{
    GatewayShardIdV1, RuntimeBarrierIdV1, RuntimeBarrierPauseWitnessV2, RuntimeBuildRevisionV1,
    RuntimeCanonicalProductDrainV2, RuntimeCanonicalValueErrorV2,
    RuntimeDrainCertificationResolutionV2, RuntimeDrainClaimProgressV2,
    RuntimeDrainClaimSealWitnessV2, RuntimeDrainClaimV2, RuntimeDrainIntentIdV2,
    RuntimeDrainIntentV2, RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayOwnerLeaseIdV1,
    RuntimePersistedProductDrainRootV2, RuntimeProductDrainOperationV2,
    RuntimeProductDrainReplayErrorV2, RuntimeProductDrainScopeLookupV2,
    RuntimeProductMutationKindV2, RuntimeProductMutationPreimageV2, RuntimeProductOperationIdV2,
    RuntimeProductSemanticRequestDigestV2, RuntimeRouteAbsentAcknowledgementV2,
    RuntimeRouteMutationProvenanceV2, RuntimeServingSlotV2,
};

const PRODUCT_OPERATION_ID: &str = "00112233445566778899aabbccddeeff";
const DRAIN_INTENT_ID: &str = "ffeeddccbbaa99887766554433221100";
const FOREIGN_PRODUCT_OPERATION_ID: &str = "11112233445566778899aabbccddeeff";
const FOREIGN_DRAIN_INTENT_ID: &str = "22222222333344445555666677778888";

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

fn deployment(deployment_id: &str, target: RuntimeDeploymentTargetV1) -> RuntimeDeployment {
    RuntimeDeployment::request(
        identity(deployment_id),
        target,
        RuntimeGeneration::new(4).unwrap(),
        None,
        at(100),
    )
    .unwrap()
}

fn snapshot() -> RuntimeDeploymentSnapshotV1 {
    deployment("deployment:1", target("studyroom", 1, 'b')).snapshot()
}

fn advanced_snapshot() -> RuntimeDeploymentSnapshotV1 {
    let mut deployment = deployment("deployment:1", target("studyroom", 1, 'b'));
    deployment
        .acquire_lease(LeaseRequestV1 {
            expected_revision: deployment.revision(),
            controller_id: ControllerId::parse("controller:advance").unwrap(),
            fencing_token: FencingToken::new(1).unwrap(),
            now: at(101),
            expires_at: at(200),
        })
        .unwrap();
    deployment.snapshot()
}

fn operation_with(
    snapshot: &RuntimeDeploymentSnapshotV1,
    operation_id: &str,
    intent_id: &str,
    semantic: char,
) -> RuntimeProductDrainOperationV2 {
    let product = RuntimeProductMutationPreimageV2 {
        operation_id: RuntimeProductOperationIdV2::parse(operation_id).unwrap(),
        scope: crate::RuntimeDeploymentScopeV1::from_identity(&snapshot.identity),
        expected_revision: snapshot.revision,
        slot: RuntimeServingSlotV2::from_target(&snapshot.target),
        expected_target: snapshot.target.clone(),
        mutation_kind: RuntimeProductMutationKindV2::AuthorityChange,
        product_semantic_request_digest: RuntimeProductSemanticRequestDigestV2::parse(
            semantic.to_string().repeat(64),
        )
        .unwrap(),
    };
    let canonical = RuntimeCanonicalProductDrainV2::new(
        product,
        RuntimeDrainIntentIdV2::parse(intent_id).unwrap(),
    )
    .unwrap();
    RuntimeProductDrainOperationV2::new(snapshot, canonical).unwrap()
}

fn operation() -> RuntimeProductDrainOperationV2 {
    operation_with(&snapshot(), PRODUCT_OPERATION_ID, DRAIN_INTENT_ID, 'd')
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

fn persisted_states(
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

fn persisted_error(
    field: RuntimeProductDrainScopeObservationFieldV2,
) -> RuntimeProductDrainScopeObservationErrorV2 {
    RuntimeProductDrainScopeObservationErrorV2::PersistedMismatch { field }
}

fn lookup_error(
    field: RuntimeProductDrainScopeObservationFieldV2,
) -> RuntimeProductDrainScopeObservationErrorV2 {
    RuntimeProductDrainScopeObservationErrorV2::LookupMismatch { field }
}

#[test]
fn observed_pair_requires_exact_root_and_intent_canonical_identity() {
    let operation = operation();
    let persisted_root = root(&operation);
    let intent = RuntimeDrainIntentV2::from_inserted(&operation, non_zero(17)).unwrap();
    let exact =
        RuntimeObservedProductDrainV2::from_exact_parts(persisted_root.clone(), intent.clone())
            .unwrap();

    assert_eq!(exact.root(), &persisted_root);
    assert_eq!(exact.intent(), &intent);
    assert_eq!(exact.clone().into_intent(), intent);

    let foreign = operation_with(
        &snapshot(),
        FOREIGN_PRODUCT_OPERATION_ID,
        FOREIGN_DRAIN_INTENT_ID,
        'e',
    );
    let foreign_intent = RuntimeDrainIntentV2::from_inserted(&foreign, non_zero(17)).unwrap();
    assert_eq!(
        RuntimeObservedProductDrainV2::from_exact_parts(persisted_root, foreign_intent),
        Err(persisted_error(
            RuntimeProductDrainScopeObservationFieldV2::ImmutableRoots,
        ))
    );
}

#[test]
fn absent_observation_binds_the_combined_lookup_snapshot_and_database_time() {
    let snapshot = snapshot();
    let lookup = RuntimeProductDrainScopeLookupV2::from_locked_snapshot(&snapshot).unwrap();
    let observation =
        RuntimeProductDrainScopeObservationV2::absent(lookup.clone(), snapshot.clone(), at(200))
            .unwrap();

    assert_eq!(
        observation.kind(),
        RuntimeProductDrainScopeObservationKindV2::Absent
    );
    assert_eq!(observation.lookup(), &lookup);
    assert_eq!(observation.locked_snapshot(), &snapshot);
    assert_eq!(observation.observed_at(), at(200));
    assert!(observation.persisted().is_none());
    assert!(observation.corruption().is_none());
    assert!(observation.into_persisted().is_none());
}

#[test]
fn present_observation_adopts_all_four_persisted_mutable_states() {
    let snapshot = snapshot();
    let operation = operation();
    let lookup = operation.scope_lookup();

    for persisted in persisted_states(&operation) {
        let expected = persisted.clone();
        let observation = RuntimeProductDrainScopeObservationV2::present(
            lookup.clone(),
            snapshot.clone(),
            persisted,
            at(201),
        )
        .unwrap();
        assert_eq!(
            observation.kind(),
            RuntimeProductDrainScopeObservationKindV2::Present
        );
        assert_eq!(observation.persisted(), Some(&expected));
        assert!(observation.corruption().is_none());
        assert_eq!(observation.into_persisted(), Some(expected));
    }
}

#[test]
fn lookup_validation_rejects_invalid_or_different_locked_snapshots() {
    let snapshot = snapshot();
    let lookup = RuntimeProductDrainScopeLookupV2::from_locked_snapshot(&snapshot).unwrap();
    let mut invalid = snapshot.clone();
    invalid.phase = RuntimeDeploymentPhaseV1::Live;
    assert_eq!(
        RuntimeProductDrainScopeObservationV2::absent(lookup.clone(), invalid, at(1)),
        Err(RuntimeProductDrainScopeObservationErrorV2::InvalidSnapshot)
    );

    let other_scope = deployment("deployment:2", target("studyroom", 1, 'b')).snapshot();
    let other_scope_lookup =
        RuntimeProductDrainScopeLookupV2::from_locked_snapshot(&other_scope).unwrap();
    assert_eq!(
        RuntimeProductDrainScopeObservationV2::absent(other_scope_lookup, snapshot.clone(), at(1),),
        Err(lookup_error(
            RuntimeProductDrainScopeObservationFieldV2::ProductScope,
        ))
    );

    let advanced_lookup =
        RuntimeProductDrainScopeLookupV2::from_locked_snapshot(&advanced_snapshot()).unwrap();
    assert_eq!(
        RuntimeProductDrainScopeObservationV2::absent(advanced_lookup, snapshot.clone(), at(1),),
        Err(lookup_error(
            RuntimeProductDrainScopeObservationFieldV2::ProductExpectedRevision,
        ))
    );

    let other_slot = deployment("deployment:1", target("other", 1, 'b')).snapshot();
    let other_slot_lookup =
        RuntimeProductDrainScopeLookupV2::from_locked_snapshot(&other_slot).unwrap();
    assert_eq!(
        RuntimeProductDrainScopeObservationV2::absent(other_slot_lookup, snapshot, at(1),),
        Err(lookup_error(
            RuntimeProductDrainScopeObservationFieldV2::DrainSlot,
        ))
    );
}

#[test]
fn present_observation_rejects_each_reachable_root_snapshot_mismatch() {
    let snapshot = snapshot();
    let lookup = RuntimeProductDrainScopeLookupV2::from_locked_snapshot(&snapshot).unwrap();

    let other_scope_snapshot = deployment("deployment:2", target("studyroom", 1, 'b')).snapshot();
    let other_scope = operation_with(
        &other_scope_snapshot,
        PRODUCT_OPERATION_ID,
        DRAIN_INTENT_ID,
        'd',
    );
    assert_eq!(
        RuntimeProductDrainScopeObservationV2::present(
            lookup.clone(),
            snapshot.clone(),
            observed(
                root(&other_scope),
                RuntimeDrainIntentV2::from_inserted(&other_scope, non_zero(1)).unwrap(),
            ),
            at(1),
        ),
        Err(persisted_error(
            RuntimeProductDrainScopeObservationFieldV2::ProductScope,
        ))
    );

    let advanced_snapshot = advanced_snapshot();
    let advanced = operation_with(
        &advanced_snapshot,
        PRODUCT_OPERATION_ID,
        DRAIN_INTENT_ID,
        'd',
    );
    assert_eq!(
        RuntimeProductDrainScopeObservationV2::present(
            lookup.clone(),
            snapshot.clone(),
            observed(
                root(&advanced),
                RuntimeDrainIntentV2::from_inserted(&advanced, non_zero(1)).unwrap(),
            ),
            at(1),
        ),
        Err(persisted_error(
            RuntimeProductDrainScopeObservationFieldV2::ProductExpectedRevision,
        ))
    );

    let other_slot_snapshot = deployment("deployment:1", target("other", 1, 'b')).snapshot();
    let other_slot = operation_with(
        &other_slot_snapshot,
        PRODUCT_OPERATION_ID,
        DRAIN_INTENT_ID,
        'd',
    );
    assert_eq!(
        RuntimeProductDrainScopeObservationV2::present(
            lookup.clone(),
            snapshot.clone(),
            observed(
                root(&other_slot),
                RuntimeDrainIntentV2::from_inserted(&other_slot, non_zero(1)).unwrap(),
            ),
            at(1),
        ),
        Err(persisted_error(
            RuntimeProductDrainScopeObservationFieldV2::DrainSlot,
        ))
    );

    let other_target_snapshot = deployment("deployment:1", target("studyroom", 2, 'e')).snapshot();
    let other_target = operation_with(
        &other_target_snapshot,
        PRODUCT_OPERATION_ID,
        DRAIN_INTENT_ID,
        'd',
    );
    assert_eq!(
        RuntimeProductDrainScopeObservationV2::present(
            lookup,
            snapshot,
            observed(
                root(&other_target),
                RuntimeDrainIntentV2::from_inserted(&other_target, non_zero(1)).unwrap(),
            ),
            at(1),
        ),
        Err(persisted_error(
            RuntimeProductDrainScopeObservationFieldV2::ExpectedTarget,
        ))
    );
}

#[test]
fn every_physical_corruption_classification_is_closed_and_inert() {
    let snapshot = snapshot();
    let lookup = RuntimeProductDrainScopeLookupV2::from_locked_snapshot(&snapshot).unwrap();
    let reasons = [
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

    for reason in reasons {
        let observation = RuntimeProductDrainScopeObservationV2::persistence_corrupt(
            lookup.clone(),
            snapshot.clone(),
            reason,
            at(2),
        )
        .unwrap();
        assert_eq!(
            observation.kind(),
            RuntimeProductDrainScopeObservationKindV2::PersistenceCorrupt
        );
        assert_eq!(observation.corruption(), Some(reason));
        assert!(observation.persisted().is_none());
        assert!(observation.into_persisted().is_none());
    }
}

#[test]
fn ambiguous_scope_classification_never_selects_a_first_row() {
    let snapshot = snapshot();
    let lookup = RuntimeProductDrainScopeLookupV2::from_locked_snapshot(&snapshot).unwrap();

    for scope in [
        RuntimeProductDrainNaturalScopeV2::ProductOperation,
        RuntimeProductDrainNaturalScopeV2::DrainIntent,
    ] {
        let observation = RuntimeProductDrainScopeObservationV2::persistence_corrupt(
            lookup.clone(),
            snapshot.clone(),
            RuntimeProductDrainScopeCorruptionV2::Ambiguous(scope),
            at(2),
        )
        .unwrap();
        assert_eq!(
            observation.kind(),
            RuntimeProductDrainScopeObservationKindV2::PersistenceCorrupt
        );
        assert_eq!(
            observation.corruption(),
            Some(RuntimeProductDrainScopeCorruptionV2::Ambiguous(scope))
        );
        assert!(observation.persisted().is_none());
        assert!(observation.into_persisted().is_none());
    }
}

#[test]
fn observed_time_is_canonical_without_host_clock_ordering() {
    let snapshot = snapshot();
    let lookup = RuntimeProductDrainScopeLookupV2::from_locked_snapshot(&snapshot).unwrap();
    for observed_at in [
        DateTime::from_timestamp_micros(-62_135_596_800_000_000).unwrap(),
        DateTime::from_timestamp_micros(-1).unwrap(),
        DateTime::from_timestamp_micros(253_402_300_799_999_999).unwrap(),
    ] {
        assert!(RuntimeProductDrainScopeObservationV2::absent(
            lookup.clone(),
            snapshot.clone(),
            observed_at,
        )
        .is_ok());
    }

    let sub_microsecond = DateTime::from_timestamp(1, 1).unwrap();
    assert_eq!(
        RuntimeProductDrainScopeObservationV2::absent(lookup, snapshot, sub_microsecond,),
        Err(
            RuntimeProductDrainScopeObservationErrorV2::InvalidObservedAt {
                reason: RuntimeCanonicalValueErrorV2::TimestampSubMicrosecond,
            },
        )
    );
}

#[test]
fn observed_pair_replay_is_exact_and_does_not_change_the_adopted_identity() {
    let operation = operation();
    let persisted_root = root(&operation);
    let intent = RuntimeDrainIntentV2::from_inserted(&operation, non_zero(17)).unwrap();
    let observed = RuntimeObservedProductDrainV2::from_exact_parts(persisted_root, intent).unwrap();

    assert_eq!(observed.require_byte_exact_replay(&operation), Ok(()));

    let competing = operation_with(
        &snapshot(),
        FOREIGN_PRODUCT_OPERATION_ID,
        FOREIGN_DRAIN_INTENT_ID,
        'e',
    );
    assert_eq!(
        observed.require_byte_exact_replay(&competing),
        Err(RuntimeProductDrainReplayErrorV2::CreationMismatch)
    );
    assert_eq!(observed.intent().key().intent_id.as_str(), DRAIN_INTENT_ID);
    assert_eq!(
        observed.root().product_operation_id().as_str(),
        PRODUCT_OPERATION_ID
    );
}

#[test]
fn combined_observation_surface_has_no_retry_or_persistence_authority() {
    let source = include_str!("../v2_product_drain_observation.rs");

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
        "pub enum RuntimeProductDrainNaturalScopeV2",
        "pub enum RuntimeProductDrainScopeCorruptionV2",
        "pub enum RuntimeProductDrainScopeObservationKindV2",
        "pub struct RuntimeObservedProductDrainV2",
        "pub struct RuntimeProductDrainScopeObservationV2",
        "pub fn from_exact_parts(",
        "pub fn absent(",
        "pub fn present(",
        "pub fn persistence_corrupt(",
        "pub fn into_persisted(",
    ] {
        assert!(source.contains(declaration), "{declaration}");
    }
    for closed in [
        "pub struct RuntimeObservedProductDrainV2 {\n    root:",
        "pub struct RuntimeProductDrainScopeObservationV2 {\n    lookup:",
    ] {
        assert!(source.contains(closed), "{closed}");
    }
}
