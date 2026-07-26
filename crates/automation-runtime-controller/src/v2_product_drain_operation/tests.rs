use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    ActivationRequestId, BindingRevision, DeploymentId, DeploymentRevision, InstallationId,
    PromotionId, RuntimeDeployment, RuntimeDeploymentIdentityV1, RuntimeDeploymentPhaseV1,
    RuntimeDeploymentSnapshotV1, RuntimeDeploymentTargetV1, RuntimeGeneration, TenantId,
};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;

use super::{
    RuntimePersistedProductDrainRootV2, RuntimeProductDrainOperationBuildErrorV2,
    RuntimeProductDrainOperationFieldV2, RuntimeProductDrainOperationPersistenceErrorV2,
    RuntimeProductDrainOperationV2, RuntimeProductDrainReplayErrorV2,
    RuntimeProductDrainScopeLookupV2,
};
use crate::{
    RuntimeCanonicalProductDrainV2, RuntimeDeploymentScopeV1, RuntimeDrainIntentIdV2,
    RuntimeProductDrainCanonicalErrorV2, RuntimeProductDrainCanonicalRootV2,
    RuntimeProductMutationKindV2, RuntimeProductMutationPreimageV2, RuntimeProductOperationIdV2,
    RuntimeProductSemanticRequestDigestV2, RuntimeServingSlotV2,
};

const PRODUCT_OPERATION_ID: &str = "00112233445566778899aabbccddeeff";
const DRAIN_INTENT_ID: &str = "ffeeddccbbaa99887766554433221100";

fn target(ruleset_key: &str, version: u32, content_byte: char) -> RuntimeDeploymentTargetV1 {
    RuntimeDeploymentTargetV1 {
        guild_id: GuildId(7),
        ruleset_key: RuleSetKey::parse(ruleset_key).unwrap(),
        version: RuleSetVersionId::new(version).unwrap(),
        content_hash: RuleSetContentHash::parse_hex(&content_byte.to_string().repeat(64)).unwrap(),
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

fn snapshot_with_target(target: RuntimeDeploymentTargetV1) -> RuntimeDeploymentSnapshotV1 {
    RuntimeDeployment::request(
        identity(),
        target,
        RuntimeGeneration::new(4).unwrap(),
        None,
        chrono::DateTime::from_timestamp(100, 0).unwrap(),
    )
    .unwrap()
    .snapshot()
}

fn snapshot() -> RuntimeDeploymentSnapshotV1 {
    snapshot_with_target(target("studyroom", 1, 'b'))
}

fn product(
    snapshot: &RuntimeDeploymentSnapshotV1,
    operation_id: &str,
    semantic_byte: char,
) -> RuntimeProductMutationPreimageV2 {
    RuntimeProductMutationPreimageV2 {
        operation_id: RuntimeProductOperationIdV2::parse(operation_id).unwrap(),
        scope: RuntimeDeploymentScopeV1::from_identity(&snapshot.identity),
        expected_revision: snapshot.revision,
        slot: RuntimeServingSlotV2::from_target(&snapshot.target),
        expected_target: snapshot.target.clone(),
        mutation_kind: RuntimeProductMutationKindV2::AuthorityChange,
        product_semantic_request_digest: RuntimeProductSemanticRequestDigestV2::parse(
            semantic_byte.to_string().repeat(64),
        )
        .unwrap(),
    }
}

fn operation_with(
    snapshot: &RuntimeDeploymentSnapshotV1,
    operation_id: &str,
    intent_id: &str,
    semantic_byte: char,
) -> RuntimeProductDrainOperationV2 {
    let canonical = RuntimeCanonicalProductDrainV2::new(
        product(snapshot, operation_id, semantic_byte),
        RuntimeDrainIntentIdV2::parse(intent_id).unwrap(),
    )
    .unwrap();
    RuntimeProductDrainOperationV2::new(snapshot, canonical).unwrap()
}

fn operation() -> RuntimeProductDrainOperationV2 {
    operation_with(&snapshot(), PRODUCT_OPERATION_ID, DRAIN_INTENT_ID, 'd')
}

#[derive(Clone)]
struct PersistedIdentity {
    product_scope: RuntimeDeploymentScopeV1,
    product_expected_revision: DeploymentRevision,
    product_operation_id: RuntimeProductOperationIdV2,
    drain_scope: RuntimeDeploymentScopeV1,
    drain_slot: RuntimeServingSlotV2,
    drain_expected_revision: DeploymentRevision,
    drain_intent_id: RuntimeDrainIntentIdV2,
    expected_target: RuntimeDeploymentTargetV1,
}

fn persisted_identity(operation: &RuntimeProductDrainOperationV2) -> PersistedIdentity {
    PersistedIdentity {
        product_scope: operation.product_operation_scope().scope().clone(),
        product_expected_revision: operation.product_operation_scope().expected_revision(),
        product_operation_id: operation.product_operation_id().clone(),
        drain_scope: operation.drain_intent_scope().scope().clone(),
        drain_slot: operation.drain_intent_scope().slot().clone(),
        drain_expected_revision: operation.drain_intent_scope().expected_revision(),
        drain_intent_id: operation.drain_intent_id().clone(),
        expected_target: operation
            .canonical()
            .product_preimage()
            .expected_target
            .clone(),
    }
}

fn restore(
    identity: PersistedIdentity,
    operation: &RuntimeProductDrainOperationV2,
) -> Result<RuntimePersistedProductDrainRootV2, RuntimeProductDrainOperationPersistenceErrorV2> {
    RuntimePersistedProductDrainRootV2::from_persisted(
        identity.product_scope,
        identity.product_expected_revision,
        &identity.product_operation_id,
        identity.drain_scope,
        identity.drain_slot,
        identity.drain_expected_revision,
        &identity.drain_intent_id,
        &identity.expected_target,
        operation.product_mutation_request_bytes(),
        operation.product_mutation_digest(),
        operation.drain_intent_request_bytes(),
        operation.drain_intent_digest(),
    )
}

fn persistence_mismatch(
    field: RuntimeProductDrainOperationFieldV2,
) -> RuntimeProductDrainOperationPersistenceErrorV2 {
    RuntimeProductDrainOperationPersistenceErrorV2::PersistedCorrelationMismatch { field }
}

#[test]
fn operation_binds_both_exact_natural_scopes_to_the_locked_snapshot() {
    let snapshot = snapshot();
    let operation = operation_with(&snapshot, PRODUCT_OPERATION_ID, DRAIN_INTENT_ID, 'd');
    let expected_scope = RuntimeDeploymentScopeV1::from_identity(&snapshot.identity);
    let expected_slot = RuntimeServingSlotV2::from_target(&snapshot.target);

    assert_eq!(operation.product_operation_scope().scope(), &expected_scope);
    assert_eq!(
        operation.product_operation_scope().expected_revision(),
        snapshot.revision
    );
    assert_eq!(operation.drain_intent_scope().scope(), &expected_scope);
    assert_eq!(operation.drain_intent_scope().slot(), &expected_slot);
    assert_eq!(
        operation.drain_intent_scope().expected_revision(),
        snapshot.revision
    );
    assert_eq!(
        operation.product_operation_id().as_str(),
        PRODUCT_OPERATION_ID
    );
    assert_eq!(operation.drain_intent_id().as_str(), DRAIN_INTENT_ID);
}

#[test]
fn operation_rejects_an_invalid_locked_snapshot() {
    let valid = snapshot();
    let canonical = RuntimeCanonicalProductDrainV2::new(
        product(&valid, PRODUCT_OPERATION_ID, 'd'),
        RuntimeDrainIntentIdV2::parse(DRAIN_INTENT_ID).unwrap(),
    )
    .unwrap();
    let mut invalid = valid;
    invalid.phase = RuntimeDeploymentPhaseV1::Live;

    assert_eq!(
        RuntimeProductDrainOperationV2::new(&invalid, canonical),
        Err(RuntimeProductDrainOperationBuildErrorV2::InvalidSnapshot)
    );
}

#[test]
fn operation_rejects_every_locked_row_root_mismatch() {
    let snapshot = snapshot();

    let mut wrong_scope = product(&snapshot, PRODUCT_OPERATION_ID, 'd');
    wrong_scope.scope.tenant_id = TenantId::parse("tenant:other").unwrap();
    let canonical = RuntimeCanonicalProductDrainV2::new(
        wrong_scope,
        RuntimeDrainIntentIdV2::parse(DRAIN_INTENT_ID).unwrap(),
    )
    .unwrap();
    assert_eq!(
        RuntimeProductDrainOperationV2::new(&snapshot, canonical),
        Err(
            RuntimeProductDrainOperationBuildErrorV2::RootCorrelationMismatch {
                field: RuntimeProductDrainOperationFieldV2::ProductScope,
            },
        )
    );

    let mut wrong_revision = product(&snapshot, PRODUCT_OPERATION_ID, 'd');
    wrong_revision.expected_revision = snapshot.revision.next().unwrap();
    let canonical = RuntimeCanonicalProductDrainV2::new(
        wrong_revision,
        RuntimeDrainIntentIdV2::parse(DRAIN_INTENT_ID).unwrap(),
    )
    .unwrap();
    assert_eq!(
        RuntimeProductDrainOperationV2::new(&snapshot, canonical),
        Err(
            RuntimeProductDrainOperationBuildErrorV2::RootCorrelationMismatch {
                field: RuntimeProductDrainOperationFieldV2::ProductExpectedRevision,
            },
        )
    );

    let other_slot_snapshot = snapshot_with_target(target("other", 1, 'b'));
    let canonical = RuntimeCanonicalProductDrainV2::new(
        product(&other_slot_snapshot, PRODUCT_OPERATION_ID, 'd'),
        RuntimeDrainIntentIdV2::parse(DRAIN_INTENT_ID).unwrap(),
    )
    .unwrap();
    assert_eq!(
        RuntimeProductDrainOperationV2::new(&snapshot, canonical),
        Err(
            RuntimeProductDrainOperationBuildErrorV2::RootCorrelationMismatch {
                field: RuntimeProductDrainOperationFieldV2::ProductSlot,
            },
        )
    );

    let other_target_snapshot = snapshot_with_target(target("studyroom", 2, 'e'));
    let canonical = RuntimeCanonicalProductDrainV2::new(
        product(&other_target_snapshot, PRODUCT_OPERATION_ID, 'd'),
        RuntimeDrainIntentIdV2::parse(DRAIN_INTENT_ID).unwrap(),
    )
    .unwrap();
    assert_eq!(
        RuntimeProductDrainOperationV2::new(&snapshot, canonical),
        Err(
            RuntimeProductDrainOperationBuildErrorV2::RootCorrelationMismatch {
                field: RuntimeProductDrainOperationFieldV2::ExpectedTarget,
            },
        )
    );
}

#[test]
fn scope_lookup_contains_only_the_two_natural_scopes() {
    let operation = operation();
    let operation_lookup = operation.scope_lookup();
    let snapshot = snapshot();
    let snapshot_lookup =
        RuntimeProductDrainScopeLookupV2::from_locked_snapshot(&snapshot).unwrap();

    assert_eq!(operation_lookup, snapshot_lookup);
    assert_eq!(
        operation_lookup.product_operation_scope(),
        operation.product_operation_scope()
    );
    assert_eq!(
        operation_lookup.drain_intent_scope(),
        operation.drain_intent_scope()
    );

    let mut invalid = snapshot;
    invalid.phase = RuntimeDeploymentPhaseV1::Live;
    assert_eq!(
        RuntimeProductDrainScopeLookupV2::from_locked_snapshot(&invalid),
        Err(RuntimeProductDrainOperationBuildErrorV2::InvalidSnapshot)
    );
}

#[test]
fn persisted_root_reconstructs_both_exact_roots_and_normalized_scopes() {
    let operation = operation();
    let persisted = restore(persisted_identity(&operation), &operation).unwrap();

    assert_eq!(
        persisted.product_operation_scope(),
        operation.product_operation_scope()
    );
    assert_eq!(
        persisted.drain_intent_scope(),
        operation.drain_intent_scope()
    );
    assert_eq!(
        persisted.product_operation_id(),
        operation.product_operation_id()
    );
    assert_eq!(persisted.drain_intent_id(), operation.drain_intent_id());
    assert_eq!(
        persisted.product_mutation_request_bytes(),
        operation.product_mutation_request_bytes()
    );
    assert_eq!(
        persisted.product_mutation_digest(),
        operation.product_mutation_digest()
    );
    assert_eq!(
        persisted.drain_intent_request_bytes(),
        operation.drain_intent_request_bytes()
    );
    assert_eq!(
        persisted.drain_intent_digest(),
        operation.drain_intent_digest()
    );
    assert_eq!(persisted.canonical(), operation.canonical());
}

#[test]
fn persisted_root_rejects_every_normalized_identity_mismatch() {
    let operation = operation();

    let mut identity = persisted_identity(&operation);
    identity.product_scope.tenant_id = TenantId::parse("tenant:other").unwrap();
    assert_eq!(
        restore(identity, &operation),
        Err(persistence_mismatch(
            RuntimeProductDrainOperationFieldV2::ProductScope,
        ))
    );

    let mut identity = persisted_identity(&operation);
    identity.product_expected_revision = identity.product_expected_revision.next().unwrap();
    assert_eq!(
        restore(identity, &operation),
        Err(persistence_mismatch(
            RuntimeProductDrainOperationFieldV2::ProductExpectedRevision,
        ))
    );

    let mut identity = persisted_identity(&operation);
    identity.product_operation_id =
        RuntimeProductOperationIdV2::parse("11112233445566778899aabbccddeeff").unwrap();
    assert_eq!(
        restore(identity, &operation),
        Err(persistence_mismatch(
            RuntimeProductDrainOperationFieldV2::ProductOperationId,
        ))
    );

    let mut identity = persisted_identity(&operation);
    identity.drain_scope.installation_id = InstallationId::parse("installation:other").unwrap();
    assert_eq!(
        restore(identity, &operation),
        Err(persistence_mismatch(
            RuntimeProductDrainOperationFieldV2::DrainScope,
        ))
    );

    let mut identity = persisted_identity(&operation);
    identity.drain_slot =
        RuntimeServingSlotV2::new(GuildId(7), RuleSetKey::parse("other").unwrap());
    assert_eq!(
        restore(identity, &operation),
        Err(persistence_mismatch(
            RuntimeProductDrainOperationFieldV2::DrainSlot,
        ))
    );

    let mut identity = persisted_identity(&operation);
    identity.drain_expected_revision = identity.drain_expected_revision.next().unwrap();
    assert_eq!(
        restore(identity, &operation),
        Err(persistence_mismatch(
            RuntimeProductDrainOperationFieldV2::DrainExpectedRevision,
        ))
    );

    let mut identity = persisted_identity(&operation);
    identity.drain_intent_id =
        RuntimeDrainIntentIdV2::parse("11112233445566778899aabbccddeeff").unwrap();
    assert_eq!(
        restore(identity, &operation),
        Err(persistence_mismatch(
            RuntimeProductDrainOperationFieldV2::DrainIntentId,
        ))
    );

    let mut identity = persisted_identity(&operation);
    identity.expected_target.version = RuleSetVersionId::new(2).unwrap();
    assert_eq!(
        restore(identity, &operation),
        Err(persistence_mismatch(
            RuntimeProductDrainOperationFieldV2::ExpectedTarget,
        ))
    );
}

#[test]
fn persisted_root_rejects_canonical_corruption_in_either_root() {
    let operation = operation();
    let identity = persisted_identity(&operation);
    let wrong_product_digest =
        crate::RuntimeProductMutationDigestV2::parse("0".repeat(64)).unwrap();

    assert_eq!(
        RuntimePersistedProductDrainRootV2::from_persisted(
            identity.product_scope,
            identity.product_expected_revision,
            &identity.product_operation_id,
            identity.drain_scope,
            identity.drain_slot,
            identity.drain_expected_revision,
            &identity.drain_intent_id,
            &identity.expected_target,
            operation.product_mutation_request_bytes(),
            &wrong_product_digest,
            operation.drain_intent_request_bytes(),
            operation.drain_intent_digest(),
        ),
        Err(RuntimeProductDrainOperationPersistenceErrorV2::Canonical(
            RuntimeProductDrainCanonicalErrorV2::PersistedDigestMismatch {
                root: RuntimeProductDrainCanonicalRootV2::ProductMutation,
            },
        ))
    );

    let identity = persisted_identity(&operation);
    let wrong_drain_digest = crate::RuntimeDrainIntentDigestV2::parse("0".repeat(64)).unwrap();
    assert_eq!(
        RuntimePersistedProductDrainRootV2::from_persisted(
            identity.product_scope,
            identity.product_expected_revision,
            &identity.product_operation_id,
            identity.drain_scope,
            identity.drain_slot,
            identity.drain_expected_revision,
            &identity.drain_intent_id,
            &identity.expected_target,
            operation.product_mutation_request_bytes(),
            operation.product_mutation_digest(),
            operation.drain_intent_request_bytes(),
            &wrong_drain_digest,
        ),
        Err(RuntimeProductDrainOperationPersistenceErrorV2::Canonical(
            RuntimeProductDrainCanonicalErrorV2::PersistedDigestMismatch {
                root: RuntimeProductDrainCanonicalRootV2::DrainIntent,
            },
        ))
    );
}

#[test]
fn byte_exact_replay_accepts_only_the_original_scopes_ids_bytes_and_digests() {
    let operation = operation();
    let persisted = restore(persisted_identity(&operation), &operation).unwrap();

    assert_eq!(persisted.require_byte_exact_replay(&operation), Ok(()));

    let changed_product_id = operation_with(
        &snapshot(),
        "11112233445566778899aabbccddeeff",
        DRAIN_INTENT_ID,
        'd',
    );
    assert_eq!(
        persisted.require_byte_exact_replay(&changed_product_id),
        Err(RuntimeProductDrainReplayErrorV2::CreationMismatch)
    );

    let changed_intent_id = operation_with(
        &snapshot(),
        PRODUCT_OPERATION_ID,
        "11112233445566778899aabbccddeeff",
        'd',
    );
    assert_eq!(
        persisted.require_byte_exact_replay(&changed_intent_id),
        Err(RuntimeProductDrainReplayErrorV2::CreationMismatch)
    );

    let changed_semantic_request =
        operation_with(&snapshot(), PRODUCT_OPERATION_ID, DRAIN_INTENT_ID, 'e');
    assert_eq!(
        persisted.require_byte_exact_replay(&changed_semantic_request),
        Err(RuntimeProductDrainReplayErrorV2::CreationMismatch)
    );

    let changed_target_snapshot = snapshot_with_target(target("studyroom", 2, 'e'));
    let changed_target = operation_with(
        &changed_target_snapshot,
        PRODUCT_OPERATION_ID,
        DRAIN_INTENT_ID,
        'd',
    );
    assert_eq!(
        persisted.require_byte_exact_replay(&changed_target),
        Err(RuntimeProductDrainReplayErrorV2::CreationMismatch)
    );
}
