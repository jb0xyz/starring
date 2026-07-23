use std::num::NonZeroU64;

use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    ActivationRequestId, BindingRevision, ControllerId, DeploymentId, DeploymentRevision,
    FencingToken, InstallationId, ProcessInstanceId, PromotionId, RuntimeDeployment,
    RuntimeDeploymentIdentityV1, RuntimeDeploymentSnapshotV1, RuntimeDeploymentTargetV1,
    RuntimeGeneration, TenantId,
};
use chrono::{DateTime, Utc};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;

use super::{
    RuntimeDrainIntentStateErrorV2, RuntimeDrainIntentStateFieldV2, RuntimeDrainIntentStateKindV2,
    RuntimeDrainIntentV2,
};
use crate::{
    GatewayShardIdV1, RuntimeBarrierIdV1, RuntimeBarrierPauseWitnessV2, RuntimeBuildRevisionV1,
    RuntimeCanonicalProductDrainV2, RuntimeCanonicalValueErrorV2,
    RuntimeDrainCertificationResolutionV2, RuntimeDrainClaimErrorV2, RuntimeDrainClaimProgressV2,
    RuntimeDrainClaimSealWitnessV2, RuntimeDrainClaimV2, RuntimeDrainIntentIdV2,
    RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayOwnerLeaseIdV1,
    RuntimePersistedProductDrainRootV2, RuntimeProductDrainOperationV2,
    RuntimeProductMutationKindV2, RuntimeProductMutationPreimageV2, RuntimeProductOperationIdV2,
    RuntimeProductSemanticRequestDigestV2, RuntimeRouteAbsentAcknowledgementV2,
    RuntimeRouteMutationProvenanceV2, RuntimeServingSlotV2,
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

fn at_microseconds(value: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(value).unwrap()
}

fn process_id() -> ProcessInstanceId {
    ProcessInstanceId::parse("process:1").unwrap()
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

fn owner() -> RuntimeGatewayOwnerLeaseIdV1 {
    RuntimeGatewayOwnerLeaseIdV1 {
        gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        process_instance_id: process_id(),
        lease_epoch: non_zero(3),
        expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
    }
}

fn provenance() -> RuntimeRouteMutationProvenanceV2 {
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
    let key = &operation.canonical().drain_preimage().key;
    RuntimeRouteAbsentAcknowledgementV2::new(
        key,
        claim(operation),
        None,
        provenance(),
        non_zero(9),
        RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
        at(300),
    )
    .unwrap()
}

#[test]
fn inserted_state_is_unclaimed_pending_and_retains_the_exact_canonical_roots() {
    let operation = operation_for(DRAIN_INTENT_ID);
    let intent = RuntimeDrainIntentV2::from_inserted(&operation, non_zero(17)).unwrap();

    assert_eq!(intent.canonical(), operation.canonical());
    assert_eq!(intent.key(), &operation.canonical().drain_preimage().key);
    assert_eq!(
        intent.product_mutation_request_bytes(),
        operation.product_mutation_request_bytes()
    );
    assert_eq!(
        intent.product_mutation_digest(),
        operation.product_mutation_digest()
    );
    assert_eq!(
        intent.drain_intent_request_bytes(),
        operation.drain_intent_request_bytes()
    );
    assert_eq!(
        intent.drain_intent_digest(),
        operation.drain_intent_digest()
    );
    assert_eq!(intent.intent_revision(), non_zero(17));
    assert_eq!(
        intent.state().kind(),
        RuntimeDrainIntentStateKindV2::Pending
    );
    assert!(intent.state().freezes_serving_slot());
    assert!(!intent.state().is_runtime_terminal());
    assert!(intent.state().pending_claim().is_none());
    assert!(intent.state().acknowledgement().is_none());
    assert!(intent.state().resulting_revision().is_none());
    assert!(intent.state().consumed_at().is_none());
    assert!(intent.state().cancelled_at().is_none());
}

#[test]
fn persisted_constructors_restore_all_four_closed_state_variants() {
    let operation = operation_for(DRAIN_INTENT_ID);
    let root = persisted_root(&operation);
    let claim = claim(&operation);
    let acknowledgement = acknowledgement(&operation);

    let pending =
        RuntimeDrainIntentV2::pending_from_persisted(&root, non_zero(18), Some(claim.clone()))
            .unwrap();
    assert_eq!(
        pending.state().kind(),
        RuntimeDrainIntentStateKindV2::Pending
    );
    assert!(pending.state().freezes_serving_slot());
    assert!(!pending.state().is_runtime_terminal());
    assert_eq!(pending.state().pending_claim(), Some(&claim));
    assert!(pending.state().acknowledgement().is_none());

    let acknowledged = RuntimeDrainIntentV2::route_absent_acknowledged_from_persisted(
        &root,
        non_zero(19),
        acknowledgement.clone(),
    )
    .unwrap();
    assert_eq!(
        acknowledged.state().kind(),
        RuntimeDrainIntentStateKindV2::RouteAbsentAcknowledged
    );
    assert!(acknowledged.state().freezes_serving_slot());
    assert!(acknowledged.state().is_runtime_terminal());
    assert_eq!(
        acknowledged.state().acknowledgement(),
        Some(&acknowledgement)
    );
    assert!(acknowledged.state().pending_claim().is_none());

    let consumed_at = at_microseconds(-17);
    let consumed = RuntimeDrainIntentV2::consumed_from_persisted(
        &root,
        non_zero(20),
        DeploymentRevision::new(21).unwrap(),
        consumed_at,
    )
    .unwrap();
    assert_eq!(
        consumed.state().kind(),
        RuntimeDrainIntentStateKindV2::Consumed
    );
    assert!(!consumed.state().freezes_serving_slot());
    assert!(consumed.state().is_runtime_terminal());
    assert_eq!(
        consumed.state().resulting_revision(),
        Some(DeploymentRevision::new(21).unwrap())
    );
    assert_eq!(consumed.state().consumed_at(), Some(consumed_at));
    assert!(consumed.state().cancelled_at().is_none());

    let cancelled_at = at_microseconds(300_000_001);
    let cancelled =
        RuntimeDrainIntentV2::cancelled_from_persisted(&root, non_zero(22), cancelled_at).unwrap();
    assert_eq!(
        cancelled.state().kind(),
        RuntimeDrainIntentStateKindV2::Cancelled
    );
    assert!(!cancelled.state().freezes_serving_slot());
    assert!(cancelled.state().is_runtime_terminal());
    assert_eq!(cancelled.state().cancelled_at(), Some(cancelled_at));
    assert!(cancelled.state().resulting_revision().is_none());

    for intent in [pending, acknowledged, consumed, cancelled] {
        assert_eq!(intent.canonical(), root.canonical());
        assert_eq!(intent.key(), &root.canonical().drain_preimage().key);
        assert_eq!(
            intent.product_mutation_request_bytes(),
            root.product_mutation_request_bytes()
        );
        assert_eq!(
            intent.product_mutation_digest(),
            root.product_mutation_digest()
        );
        assert_eq!(
            intent.drain_intent_request_bytes(),
            root.drain_intent_request_bytes()
        );
        assert_eq!(intent.drain_intent_digest(), root.drain_intent_digest());
    }
}

#[test]
fn persisted_pending_and_acknowledged_states_reject_foreign_evidence() {
    let operation = operation_for(DRAIN_INTENT_ID);
    let root = persisted_root(&operation);
    let foreign = operation_for(FOREIGN_DRAIN_INTENT_ID);

    assert_eq!(
        RuntimeDrainIntentV2::pending_from_persisted(&root, non_zero(1), Some(claim(&foreign)),),
        Err(RuntimeDrainIntentStateErrorV2::Claim(
            RuntimeDrainClaimErrorV2::IntentMismatch,
        ))
    );
    assert_eq!(
        RuntimeDrainIntentV2::route_absent_acknowledged_from_persisted(
            &root,
            non_zero(1),
            acknowledgement(&foreign),
        ),
        Err(RuntimeDrainIntentStateErrorV2::Claim(
            RuntimeDrainClaimErrorV2::IntentMismatch,
        ))
    );
}

#[test]
fn revisions_use_the_full_database_integer_range_without_successor_assumptions() {
    let operation = operation_for(DRAIN_INTENT_ID);
    let root = persisted_root(&operation);

    for revision in [1, 71, i64::MAX as u64] {
        let intent = RuntimeDrainIntentV2::from_inserted(&operation, non_zero(revision)).unwrap();
        assert_eq!(intent.intent_revision().get(), revision);

        let consumed = RuntimeDrainIntentV2::consumed_from_persisted(
            &root,
            non_zero(revision),
            DeploymentRevision::new(revision).unwrap(),
            at(1),
        )
        .unwrap();
        assert_eq!(
            consumed.state().resulting_revision(),
            Some(DeploymentRevision::new(revision).unwrap())
        );
    }

    let overflow = non_zero(i64::MAX as u64 + 1);
    assert_eq!(
        RuntimeDrainIntentV2::from_inserted(&operation, overflow),
        Err(RuntimeDrainIntentStateErrorV2::CanonicalValue {
            field: RuntimeDrainIntentStateFieldV2::IntentRevision,
            reason: RuntimeCanonicalValueErrorV2::PersistenceIntegerOutOfRange,
        })
    );
    assert_eq!(
        RuntimeDrainIntentV2::consumed_from_persisted(
            &root,
            non_zero(1),
            DeploymentRevision::new(overflow.get()).unwrap(),
            at(1),
        ),
        Err(RuntimeDrainIntentStateErrorV2::CanonicalValue {
            field: RuntimeDrainIntentStateFieldV2::ResultingRevision,
            reason: RuntimeCanonicalValueErrorV2::PersistenceIntegerOutOfRange,
        })
    );
}

#[test]
fn terminal_timestamps_are_canonical_without_host_clock_ordering() {
    let operation = operation_for(DRAIN_INTENT_ID);
    let root = persisted_root(&operation);
    let supported = [
        at_microseconds(-62_135_596_800_000_000),
        at_microseconds(-1),
        at_microseconds(253_402_300_799_999_999),
    ];

    for timestamp in supported {
        assert!(RuntimeDrainIntentV2::consumed_from_persisted(
            &root,
            non_zero(1),
            DeploymentRevision::new(1).unwrap(),
            timestamp,
        )
        .is_ok());
        assert!(
            RuntimeDrainIntentV2::cancelled_from_persisted(&root, non_zero(1), timestamp).is_ok()
        );
    }

    let sub_microsecond = DateTime::from_timestamp(1, 1).unwrap();
    assert_eq!(
        RuntimeDrainIntentV2::consumed_from_persisted(
            &root,
            non_zero(1),
            DeploymentRevision::new(1).unwrap(),
            sub_microsecond,
        ),
        Err(RuntimeDrainIntentStateErrorV2::CanonicalValue {
            field: RuntimeDrainIntentStateFieldV2::ConsumedAt,
            reason: RuntimeCanonicalValueErrorV2::TimestampSubMicrosecond,
        })
    );
    assert_eq!(
        RuntimeDrainIntentV2::cancelled_from_persisted(&root, non_zero(1), sub_microsecond,),
        Err(RuntimeDrainIntentStateErrorV2::CanonicalValue {
            field: RuntimeDrainIntentStateFieldV2::CancelledAt,
            reason: RuntimeCanonicalValueErrorV2::TimestampSubMicrosecond,
        })
    );
}

#[test]
fn state_surface_is_cloneable_data_without_wire_or_mutation_authority() {
    let source = include_str!("../v2_drain_intent_state.rs");

    for forbidden in [
        "serde",
        "Serialize",
        "Deserialize",
        "Default",
        "sqlx",
        "rusqlite",
        "twilight",
        "Port",
        "Permit",
        "Authorized",
        "impl Future",
        "async fn",
    ] {
        assert!(!source.contains(forbidden), "{forbidden}");
    }
    for declaration in [
        "#[derive(Clone, Debug, PartialEq, Eq)]\npub struct RuntimeDrainIntentStateV2",
        "#[derive(Clone, Debug, PartialEq, Eq)]\npub struct RuntimeDrainIntentV2",
    ] {
        assert!(source.contains(declaration), "{declaration}");
    }
    for closed_field in [
        "pub struct RuntimeDrainIntentStateV2 {\n    value:",
        "pub struct RuntimeDrainIntentV2 {\n    canonical:",
    ] {
        assert!(source.contains(closed_field), "{closed_field}");
    }
}
