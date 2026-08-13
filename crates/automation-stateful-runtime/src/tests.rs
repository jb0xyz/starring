use std::collections::BTreeMap;

use automation_instance::InstanceId;
use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    BindingRevision, DeploymentId, FencingToken, InstallationId, ProcessInstanceId,
    RuntimeDeploymentTargetV1, RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use automation_runtime_interaction::{
    build_interaction_request_digest_v1, DiscordApplicationIdV1, DiscordInteractionIdV1,
    InteractionActionPlanDigestV1, InteractionExpectedRouteV1, InteractionGatewayOwnerIdentityV1,
    InteractionGatewayOwnerLeaseEpochV1, InteractionGatewayOwnerRevisionV1,
    InteractionGatewayShardIdentityV1, InteractionInstanceManifestDigestV1,
    InteractionProductScopeV1, InteractionReceiptClaimCandidateV1, InteractionReceiptIdentityV1,
    InteractionRequestDigestInputV1, InteractionRequestMaterialV1, InteractionRequestPayloadV1,
    InteractionRouteAttestationDigestV1, InteractionRouteBindingV1, InteractionRouteIncarnationV1,
    InteractionRuntimeBuildRevisionV1, InteractionServingLeaseEpochV1,
    InteractionServingLeaseRevisionV1, InteractionServingRouteIdentityV1,
    VerifiedInteractionRequestV1,
};
use automation_spec::{
    ModalDefinitionV1, ModalFieldDefinitionV1, ModalFieldStyleV1, ModalInputPolicyV1,
};
use automation_stateful_compiler::{compile_stateful_spec_bundle_v1, CompiledStatefulBundleV1};
use automation_stateful_spec::{
    ActionNodeV1, ActionV1, IntegerComparisonV1, StateScopeV1, StateSetNodeV1, StateValueTypeV1,
    StateValueV1, StateVariableV1, StatefulBranchV1, StatefulConditionExprV1,
    StatefulResponseNodeV1, StatefulSpecV1, StatefulValueExprV1, StatefulWorkflowV1, TriggerV1,
    STATEFUL_SPEC_KIND_V1,
};
use discord_model::{ChannelId, GuildId, UserId};
use resource_resolution::ResourceBindingFingerprint;

use super::*;

const GUILD_ID: u64 = 107;
const CHANNEL_ID: u64 = 210;
const ACTOR_ID: u64 = 220;

fn stateful_spec(extra_variable: bool) -> StatefulSpecV1 {
    let mut state_variables = vec![StateVariableV1 {
        id: "count".to_string(),
        scope: StateScopeV1::Actor,
        value_type: StateValueTypeV1::Integer { min: 0, max: 100 },
        initial_value: StateValueV1::Integer { value: 0 },
    }];
    if extra_variable {
        state_variables.push(StateVariableV1 {
            id: "label".to_string(),
            scope: StateScopeV1::Actor,
            value_type: StateValueTypeV1::Text { max_utf8_bytes: 20 },
            initial_value: StateValueV1::Text {
                value: String::new(),
            },
        });
    }
    StatefulSpecV1 {
        schema_version: 1,
        kind: STATEFUL_SPEC_KIND_V1.to_string(),
        key: "counter_program".to_string(),
        display_name: "Counter".to_string(),
        description: "State runtime contract test".to_string(),
        panels: vec![],
        modals: vec![ModalDefinitionV1 {
            id: "counter_form".to_string(),
            title: "Counter".to_string(),
            fields: vec![ModalFieldDefinitionV1 {
                id: "note".to_string(),
                label: "Note".to_string(),
                style: ModalFieldStyleV1::Short,
                required: false,
                min_length: None,
                max_length: Some(100),
                input_policy: ModalInputPolicyV1::TrimUnicodeWhitespace,
            }],
        }],
        stateless_workflows: vec![],
        state_variables,
        stateful_workflows: vec![StatefulWorkflowV1 {
            id: "increment".to_string(),
            trigger: TriggerV1::ModalSubmit {
                modal_id: "counter_form".to_string(),
            },
            condition: StatefulConditionExprV1::IntegerCompare {
                left: StatefulValueExprV1::State {
                    variable_id: "count".to_string(),
                },
                operator: IntegerComparisonV1::LessThan,
                right: StatefulValueExprV1::Literal {
                    value: StateValueV1::Integer { value: 100 },
                },
            },
            on_true: StatefulBranchV1 {
                state_actions: vec![StateSetNodeV1 {
                    id: "increment_count".to_string(),
                    variable_id: "count".to_string(),
                    value: StatefulValueExprV1::CheckedAdd {
                        left: Box::new(StatefulValueExprV1::State {
                            variable_id: "count".to_string(),
                        }),
                        right: Box::new(StatefulValueExprV1::Literal {
                            value: StateValueV1::Integer { value: 1 },
                        }),
                    },
                }],
                effects: vec![],
                response: StatefulResponseNodeV1 {
                    id: "increment_response".to_string(),
                    content: "Incremented".to_string(),
                },
            },
            on_false: StatefulBranchV1 {
                state_actions: vec![],
                effects: vec![],
                response: StatefulResponseNodeV1 {
                    id: "limit_response".to_string(),
                    content: "At limit".to_string(),
                },
            },
        }],
    }
}

fn route(instance: bool) -> InteractionRouteBindingV1 {
    route_with_fence(instance, 7)
}

fn route_with_fence(instance: bool, fence: u64) -> InteractionRouteBindingV1 {
    route_with_fence_hash(
        instance,
        fence,
        RuleSetContentHash::parse_hex(&"7".repeat(64)).unwrap(),
    )
}

fn route_with_fence_hash(
    instance: bool,
    fence: u64,
    content_hash: RuleSetContentHash,
) -> InteractionRouteBindingV1 {
    let process_identity = RuntimeProcessIdentityV1 {
        target: RuntimeDeploymentTargetV1 {
            guild_id: GuildId(GUILD_ID),
            ruleset_key: RuleSetKey::parse("counter_program").unwrap(),
            version: RuleSetVersionId::FIRST,
            content_hash,
            binding_revision: BindingRevision::new(7).unwrap(),
            binding_fingerprint: ResourceBindingFingerprint::parse(&"a".repeat(64)).unwrap(),
        },
        runtime_generation: RuntimeGeneration::new(7).unwrap(),
        process_instance_id: ProcessInstanceId::parse("process-7").unwrap(),
    };
    let serving_identity = InteractionServingRouteIdentityV1::new(
        InteractionRouteAttestationDigestV1::parse("b".repeat(64)).unwrap(),
        InteractionServingLeaseEpochV1::new(7).unwrap(),
        InteractionServingLeaseRevisionV1::new(7).unwrap(),
        InteractionGatewayOwnerIdentityV1::new(
            InteractionGatewayShardIdentityV1::parse("gateway-shard-7").unwrap(),
            InteractionGatewayOwnerLeaseEpochV1::new(7).unwrap(),
            InteractionGatewayOwnerRevisionV1::new(7).unwrap(),
            InteractionRuntimeBuildRevisionV1::parse("build-7").unwrap(),
        ),
        FencingToken::new(fence).unwrap(),
        InteractionRouteIncarnationV1::new(fence).unwrap(),
    );
    let scope = InteractionProductScopeV1::new(
        TenantId::parse("tenant-7").unwrap(),
        InstallationId::parse("installation-7").unwrap(),
        DeploymentId::parse("deployment-7").unwrap(),
    );
    if instance {
        InteractionRouteBindingV1::new_instance(
            scope,
            process_identity,
            serving_identity,
            InstanceId::parse("instance-7").unwrap(),
            RuleSetVersionId::FIRST,
            content_hash,
            InteractionInstanceManifestDigestV1::parse("c".repeat(64)).unwrap(),
        )
        .unwrap()
    } else {
        InteractionRouteBindingV1::new_static(scope, process_identity, serving_identity).unwrap()
    }
}

fn identity(interaction_id: u64) -> InteractionReceiptIdentityV1 {
    InteractionReceiptIdentityV1::new(
        DiscordApplicationIdV1::new(11).unwrap(),
        DiscordInteractionIdV1::new(interaction_id).unwrap(),
    )
}

fn verified_modal(
    interaction_id: u64,
    custom_id: &str,
) -> Result<
    VerifiedInteractionRequestV1,
    automation_runtime_interaction::VerifiedInteractionRequestErrorV1,
> {
    verified_modal_with_fence(interaction_id, custom_id, 7)
}

fn verified_modal_with_fence(
    interaction_id: u64,
    custom_id: &str,
    fence: u64,
) -> Result<
    VerifiedInteractionRequestV1,
    automation_runtime_interaction::VerifiedInteractionRequestErrorV1,
> {
    verified_modal_with_actor_fence(interaction_id, custom_id, ACTOR_ID, fence)
}

fn verified_modal_with_actor_fence(
    interaction_id: u64,
    custom_id: &str,
    actor_id: u64,
    fence: u64,
) -> Result<
    VerifiedInteractionRequestV1,
    automation_runtime_interaction::VerifiedInteractionRequestErrorV1,
> {
    let receipt_identity = identity(interaction_id);
    let inputs = BTreeMap::from([("note".to_string(), "  private note  ".to_string())]);
    let request_digest = build_interaction_request_digest_v1(InteractionRequestDigestInputV1 {
        receipt_identity,
        guild_id: GuildId(GUILD_ID),
        channel_id: ChannelId(CHANNEL_ID),
        actor_id: UserId(actor_id),
        locale: Some("ko"),
        payload: InteractionRequestPayloadV1::ModalSubmit {
            custom_id,
            inputs: &inputs,
        },
    })
    .unwrap();
    let route = route_with_fence(false, fence);
    let claim = InteractionReceiptClaimCandidateV1::new(
        receipt_identity,
        InteractionExpectedRouteV1::from_authoritative(&route),
        request_digest,
    )
    .bind_authoritative(route)
    .unwrap();
    VerifiedInteractionRequestV1::verify(
        claim,
        InteractionRequestMaterialV1::ModalSubmit {
            guild_id: GuildId(GUILD_ID),
            channel_id: ChannelId(CHANNEL_ID),
            actor_id: UserId(actor_id),
            locale: Some("ko".to_string()),
            custom_id: custom_id.to_string(),
            inputs,
        },
    )
}

fn verified_modal_for_bundle(
    bundle: &CompiledStatefulBundleV1,
    interaction_id: u64,
) -> VerifiedInteractionRequestV1 {
    let receipt_identity = identity(interaction_id);
    let custom_id = "starring:107:counter_program:modal:counter_form";
    let inputs = BTreeMap::from([("note".to_string(), "  private note  ".to_string())]);
    let request_digest = build_interaction_request_digest_v1(InteractionRequestDigestInputV1 {
        receipt_identity,
        guild_id: GuildId(GUILD_ID),
        channel_id: ChannelId(CHANNEL_ID),
        actor_id: UserId(ACTOR_ID),
        locale: Some("ko"),
        payload: InteractionRequestPayloadV1::ModalSubmit {
            custom_id,
            inputs: &inputs,
        },
    })
    .unwrap();
    let route = route_with_fence_hash(false, 7, bundle.filtered_legacy_target().content_hash);
    let claim = InteractionReceiptClaimCandidateV1::new(
        receipt_identity,
        InteractionExpectedRouteV1::from_authoritative(&route),
        request_digest,
    )
    .bind_authoritative(route)
    .unwrap();
    VerifiedInteractionRequestV1::verify(
        claim,
        InteractionRequestMaterialV1::ModalSubmit {
            guild_id: GuildId(GUILD_ID),
            channel_id: ChannelId(CHANNEL_ID),
            actor_id: UserId(ACTOR_ID),
            locale: Some("ko".to_string()),
            custom_id: custom_id.to_string(),
            inputs,
        },
    )
    .unwrap()
}

fn envelope(spec: &StatefulSpecV1, interaction_id: u64) -> EventEnvelopeV1 {
    envelope_with_fence(spec, interaction_id, 7)
}

fn envelope_with_fence(spec: &StatefulSpecV1, interaction_id: u64, fence: u64) -> EventEnvelopeV1 {
    envelope_with_actor_fence(spec, interaction_id, ACTOR_ID, fence)
}

fn envelope_with_actor_fence(
    spec: &StatefulSpecV1,
    interaction_id: u64,
    actor_id: u64,
    fence: u64,
) -> EventEnvelopeV1 {
    let legacy = LegacyRuleSetIdentityV1::new("counter_program", 1, "7".repeat(64)).unwrap();
    let program = StatefulProgramIdentityV1::from_validated_spec(
        spec,
        StatefulArtifactDigestV1::parse("d".repeat(64)).unwrap(),
        StateSchemaDigestV1::parse(if spec.state_variables.len() == 1 {
            "e".repeat(64)
        } else {
            "f".repeat(64)
        })
        .unwrap(),
        legacy,
    )
    .unwrap();
    let verified = verified_modal_with_actor_fence(
        interaction_id,
        "starring:107:counter_program:modal:counter_form",
        actor_id,
        fence,
    )
    .unwrap();
    EventEnvelopeV1::from_verified_request(spec, program, verified).unwrap()
}

fn compiled_event_and_request(
    spec: &StatefulSpecV1,
    interaction_id: u64,
) -> (
    CompiledStatefulBundleV1,
    EventEnvelopeV1,
    StateSnapshotRequestV1,
) {
    let bundle = compile_stateful_spec_bundle_v1(spec).unwrap();
    let verified = verified_modal_for_bundle(&bundle, interaction_id);
    let publication =
        StatefulBundlePublicationBindingV1::from_test_authority(&bundle, &verified).unwrap();
    let event = EventEnvelopeV1::from_compiled_bundle(&bundle, &publication, verified).unwrap();
    let request = StateSnapshotRequestV1::from_compiled_bundle(&bundle, &event).unwrap();
    (bundle, event, request)
}

fn compiled_evaluation(
    spec: &StatefulSpecV1,
    interaction_id: u64,
    store: &InMemoryAtomicStateOutboxStoreV1,
) -> PreparedStatefulEvaluationV1 {
    let (bundle, event, request) = compiled_event_and_request(spec, interaction_id);
    let snapshot = store.read_snapshot_v1(&request).unwrap();
    PreparedStatefulEvaluationV1::prepare(&bundle, &event, snapshot).unwrap()
}

fn build_plan(
    spec: &StatefulSpecV1,
    envelope: &EventEnvelopeV1,
    store: &InMemoryAtomicStateOutboxStoreV1,
    after: i64,
    payload_bytes: &[u8],
) -> PreparedStatefulCommitV1 {
    let dependencies = CompiledWorkflowDependenciesV1::from_event_spec(spec, envelope).unwrap();
    let request = StateSnapshotRequestV1::from_event(envelope, &dependencies).unwrap();
    let snapshot = store.read_snapshot_v1(&request).unwrap();
    let definition =
        CompiledStateVariableV1::from_program_spec(spec, envelope.program(), "count").unwrap();
    let write = ResolvedStateWriteV1::set(
        &definition,
        &snapshot.reads()[0],
        "increment_count",
        0,
        StateValueV1::Integer { value: after },
    )
    .unwrap();
    PreparedStatefulCommitV1::prepare_test_scaffold(
        envelope,
        EvaluationTraceDigestV1::parse("1".repeat(64)).unwrap(),
        snapshot,
        vec![write],
        InteractionActionPlanDigestV1::parse("2".repeat(64)).unwrap(),
        StatefulOutboxPayloadV1::from_canonical_effect_plan(
            vec!["increment_response".to_string()],
            payload_bytes.to_vec(),
        )
        .unwrap(),
    )
    .unwrap()
}

fn claim_request(
    result: &StatefulAtomicCommitResultV1,
    envelope: &EventEnvelopeV1,
    now_ms: u64,
) -> OutboxClaimRequestV1 {
    OutboxClaimRequestV1 {
        receipt_identity: result.receipt().receipt_identity(),
        expected_head_revision: result.outbox().head_revision(),
        claimant_id: OutboxClaimantIdV1::parse("worker-1").unwrap(),
        authority: OutboxDispatchAuthorityV1::from_envelope(envelope),
        now_ms,
        lease_duration_ms: 100,
    }
}

#[test]
fn verified_event_rejects_custom_id_route_and_kind_mismatch() {
    let spec = stateful_spec(false);
    let legacy = LegacyRuleSetIdentityV1::new("counter_program", 1, "7".repeat(64)).unwrap();
    let program = StatefulProgramIdentityV1::from_validated_spec(
        &spec,
        StatefulArtifactDigestV1::parse("d".repeat(64)).unwrap(),
        StateSchemaDigestV1::parse("e".repeat(64)).unwrap(),
        legacy,
    )
    .unwrap();
    let verified = verified_modal(12, "starring:107:other_program:modal:counter_form").unwrap();
    assert!(matches!(
        EventEnvelopeV1::from_verified_request(&spec, program, verified),
        Err(EventEnvelopeErrorV1::RouteMismatch)
    ));
}

#[test]
fn additive_schema_keeps_existing_variable_declaration_identity_stable() {
    let v1 = stateful_spec(false);
    let v2 = stateful_spec(true);
    let e1 = envelope(&v1, 20);
    let e2 = envelope(&v2, 21);
    let d1 = CompiledStateVariableV1::from_program_spec(&v1, e1.program(), "count").unwrap();
    let d2 = CompiledStateVariableV1::from_program_spec(&v2, e2.program(), "count").unwrap();
    assert!(d1.declaration_digest() == d2.declaration_digest());

    let mut changed = v2.clone();
    changed.state_variables[0].initial_value = StateValueV1::Integer { value: 1 };
    let changed_envelope = envelope(&changed, 22);
    let changed_definition =
        CompiledStateVariableV1::from_program_spec(&changed, changed_envelope.program(), "count")
            .unwrap();
    assert!(d1.declaration_digest() != changed_definition.declaration_digest());
}

#[test]
fn absent_first_write_noop_advances_revision_and_stale_cas_fails() {
    let spec = stateful_spec(false);
    let event = envelope(&spec, 30);
    let mut store = InMemoryAtomicStateOutboxStoreV1::default();
    let first = build_plan(&spec, &event, &store, 0, b"effect-plan-v1");
    let result = store.atomic_commit_v1(first).unwrap();
    assert_eq!(result.disposition(), AtomicCommitDispositionV1::Applied);
    let transition = &store.state_transition_ledger()[0];
    assert_eq!(transition.before_revision(), None);
    assert_eq!(transition.after_revision().get(), 1);
    assert!(!transition.changed());
    let next_event = envelope(&spec, 31);
    let next = build_plan(&spec, &next_event, &store, 1, b"effect-plan-v2");
    store.atomic_commit_v1(next).unwrap();
    assert_eq!(store.state_transition_ledger()[1].after_revision().get(), 2);
}

#[test]
fn exact_replay_keeps_full_payload_and_drift_conflicts() {
    let spec = stateful_spec(false);
    let event = envelope(&spec, 40);
    let mut store = InMemoryAtomicStateOutboxStoreV1::default();
    let plan = build_plan(&spec, &event, &store, 1, b"durable-private-payload");
    let replay = plan.clone();
    let identity = plan.receipt_identity();
    let first = store.atomic_commit_v1(plan).unwrap();
    let repeated = store.atomic_commit_v1(replay).unwrap();
    assert_eq!(
        repeated.disposition(),
        AtomicCommitDispositionV1::ExactReplay
    );
    assert_eq!(store.state_transition_ledger().len(), 1);
    assert_eq!(
        store
            .stored_payload(identity)
            .unwrap()
            .canonical_effect_plan_bytes(),
        b"durable-private-payload"
    );
    assert_eq!(first.outbox().state(), OutboxStateV1::Queued);
}

#[test]
fn expired_claim_reclaims_without_intent_and_rejects_stale_finish() {
    let spec = stateful_spec(false);
    let event = envelope(&spec, 50);
    let mut store = InMemoryAtomicStateOutboxStoreV1::default();
    let committed = store
        .atomic_commit_v1(build_plan(&spec, &event, &store, 1, b"dispatch"))
        .unwrap();
    let first = store
        .claim_outbox_v1(claim_request(&committed, &event, 1_000))
        .unwrap();
    let expired = store
        .expire_claim_v1(
            committed.receipt().receipt_identity(),
            first.token().head_revision(),
            &OutboxDispatchAuthorityV1::from_envelope(&event),
            1_100,
            1_100,
        )
        .unwrap();
    let second = store
        .claim_outbox_v1(OutboxClaimRequestV1 {
            receipt_identity: committed.receipt().receipt_identity(),
            expected_head_revision: expired.head_revision(),
            claimant_id: OutboxClaimantIdV1::parse("worker-2").unwrap(),
            authority: OutboxDispatchAuthorityV1::from_envelope(&event),
            now_ms: 1_101,
            lease_duration_ms: 100,
        })
        .unwrap();
    assert_eq!(
        store.complete_outbox_v1(first.token(), 1_101),
        Err(StatefulStoreErrorV1::StaleClaim)
    );
    store
        .record_external_intent_v1(second.token(), 1_102)
        .unwrap();
    assert_eq!(
        store
            .complete_outbox_v1(second.token(), 1_103)
            .unwrap()
            .state(),
        OutboxStateV1::Completed
    );
}

#[test]
fn intent_expiration_enters_terminal_waiting_recovery_not_blind_requeue() {
    let spec = stateful_spec(false);
    let event = envelope(&spec, 60);
    let mut store = InMemoryAtomicStateOutboxStoreV1::default();
    let committed = store
        .atomic_commit_v1(build_plan(&spec, &event, &store, 1, b"dispatch"))
        .unwrap();
    let claimed = store
        .claim_outbox_v1(claim_request(&committed, &event, 2_000))
        .unwrap();
    store
        .record_external_intent_v1(claimed.token(), 2_001)
        .unwrap();
    let waiting = store
        .expire_claim_v1(
            committed.receipt().receipt_identity(),
            claimed.token().head_revision(),
            &OutboxDispatchAuthorityV1::from_envelope(&event),
            2_100,
            2_100,
        )
        .unwrap();
    assert_eq!(waiting.state(), OutboxStateV1::WaitingEffectRecovery);
    assert!(store.due_outbox_v1(9_999, 10).unwrap().is_empty());
}

#[test]
fn ordered_external_nodes_change_plan_and_payload_digest() {
    let first = StatefulOutboxPayloadV1::from_canonical_effect_plan(
        vec!["business_effect".to_string(), "final_response".to_string()],
        b"same-plan".to_vec(),
    )
    .unwrap();
    let second = StatefulOutboxPayloadV1::from_canonical_effect_plan(
        vec!["final_response".to_string(), "business_effect".to_string()],
        b"same-plan".to_vec(),
    )
    .unwrap();
    assert!(first.digest() != second.digest());
    assert!(StatefulOutboxPayloadV1::from_canonical_effect_plan(
        vec!["duplicate".to_string(), "duplicate".to_string()],
        b"plan".to_vec(),
    )
    .is_err());
}

#[test]
fn stale_snapshot_from_other_receipt_loses_concurrent_first_write_cas() {
    let spec = stateful_spec(false);
    let first_event = envelope(&spec, 70);
    let second_event = envelope(&spec, 71);
    let mut store = InMemoryAtomicStateOutboxStoreV1::default();
    let first_plan = build_plan(&spec, &first_event, &store, 1, b"first");
    let second_plan = build_plan(&spec, &second_event, &store, 2, b"second");
    let state_key = first_plan.writes()[0].key().clone();

    store.atomic_commit_v1(first_plan).unwrap();
    assert!(matches!(
        store.atomic_commit_v1(second_plan),
        Err(StatefulStoreErrorV1::StateConflict)
    ));
    let (revision, value, _) = store.state_value_v1(&state_key).unwrap();
    assert_eq!(revision.get(), 1);
    assert_eq!(value, &StateValueV1::Integer { value: 1 });
}

#[test]
fn same_receipt_payload_drift_conflicts_without_partial_mutation() {
    let spec = stateful_spec(false);
    let event = envelope(&spec, 80);
    let mut store = InMemoryAtomicStateOutboxStoreV1::default();
    let first = build_plan(&spec, &event, &store, 1, b"payload-a");
    let drift = build_plan(&spec, &event, &store, 1, b"payload-b");
    let state_key = first.writes()[0].key().clone();
    store.atomic_commit_v1(first).unwrap();

    assert!(matches!(
        store.atomic_commit_v1(drift),
        Err(StatefulStoreErrorV1::ReceiptConflict)
    ));
    assert_eq!(store.state_transition_ledger().len(), 1);
    let (revision, value, _) = store.state_value_v1(&state_key).unwrap();
    assert_eq!(revision.get(), 1);
    assert_eq!(value, &StateValueV1::Integer { value: 1 });
}

#[test]
fn wrong_dispatch_authority_cannot_claim_payload() {
    let spec = stateful_spec(false);
    let event = envelope(&spec, 90);
    let other_event = envelope_with_fence(&spec, 91, 8);
    let mut store = InMemoryAtomicStateOutboxStoreV1::default();
    let committed = store
        .atomic_commit_v1(build_plan(&spec, &event, &store, 1, b"dispatch"))
        .unwrap();
    let mut request = claim_request(&committed, &event, 3_000);
    request.authority = OutboxDispatchAuthorityV1::from_envelope(&other_event);
    assert!(matches!(
        store.claim_outbox_v1(request),
        Err(StatefulStoreErrorV1::AuthorityMismatch)
    ));
}

#[test]
fn zero_read_dependency_proof_still_rejects_a_different_spec() {
    let spec = stateful_spec(false);
    let event = envelope(&spec, 95);
    let mut different = spec.clone();
    different.stateful_workflows[0].condition = StatefulConditionExprV1::Always;
    different.stateful_workflows[0]
        .on_true
        .state_actions
        .clear();
    assert!(matches!(
        CompiledWorkflowDependenciesV1::from_event_spec(&different, &event),
        Err(StatefulStateContractErrorV1::ProgramMismatch)
    ));
}

#[test]
fn snapshot_from_another_actor_cannot_be_prepared_for_this_event() {
    let spec = stateful_spec(false);
    let actor_a = envelope_with_actor_fence(&spec, 100, ACTOR_ID, 7);
    let actor_b = envelope_with_actor_fence(&spec, 101, ACTOR_ID + 1, 7);
    let store = InMemoryAtomicStateOutboxStoreV1::default();
    let dependencies = CompiledWorkflowDependenciesV1::from_event_spec(&spec, &actor_a).unwrap();
    let request = StateSnapshotRequestV1::from_event(&actor_a, &dependencies).unwrap();
    let snapshot = store.read_snapshot_v1(&request).unwrap();
    let definition =
        CompiledStateVariableV1::from_program_spec(&spec, actor_a.program(), "count").unwrap();
    let write = ResolvedStateWriteV1::set(
        &definition,
        &snapshot.reads()[0],
        "increment_count",
        0,
        StateValueV1::Integer { value: 1 },
    )
    .unwrap();
    assert!(matches!(
        PreparedStatefulCommitV1::prepare_test_scaffold(
            &actor_b,
            EvaluationTraceDigestV1::parse("1".repeat(64)).unwrap(),
            snapshot,
            vec![write],
            InteractionActionPlanDigestV1::parse("2".repeat(64)).unwrap(),
            StatefulOutboxPayloadV1::from_canonical_effect_plan(
                vec!["increment_response".to_string()],
                b"effect-plan".to_vec(),
            )
            .unwrap(),
        ),
        Err(StatefulExecutionPlanErrorV1::InvalidInput)
    ));
}

#[test]
fn compiled_evaluator_selects_both_branches_and_matches_parallel_pre_state() {
    let mut spec = stateful_spec(false);
    spec.state_variables.push(StateVariableV1 {
        id: "mirror".to_string(),
        scope: StateScopeV1::Actor,
        value_type: StateValueTypeV1::Integer { min: 0, max: 100 },
        initial_value: StateValueV1::Integer { value: 0 },
    });
    spec.stateful_workflows[0]
        .on_true
        .state_actions
        .push(StateSetNodeV1 {
            id: "copy_old_count".to_string(),
            variable_id: "mirror".to_string(),
            value: StatefulValueExprV1::State {
                variable_id: "count".to_string(),
            },
        });
    let store = InMemoryAtomicStateOutboxStoreV1::default();
    let evaluation = compiled_evaluation(&spec, 110, &store);
    assert_eq!(evaluation.branch(), StatefulEvaluationBranchV1::True);
    assert_eq!(evaluation.writes().len(), 2);
    assert_eq!(
        evaluation
            .writes()
            .iter()
            .find(|write| write.key().variable_id() == "count")
            .unwrap()
            .after(),
        &StateValueV1::Integer { value: 1 }
    );
    assert_eq!(
        evaluation
            .writes()
            .iter()
            .find(|write| write.key().variable_id() == "mirror")
            .unwrap()
            .after(),
        &StateValueV1::Integer { value: 0 }
    );
    assert!(evaluation.verify());

    let (bundle, event, request) = compiled_event_and_request(&spec, 111);
    let mut snapshot = store.read_snapshot_v1(&request).unwrap();
    let count = snapshot
        .reads_mut_for_test()
        .iter_mut()
        .find(|read| read.key().variable_id() == "count")
        .unwrap();
    count.replace_value_for_test(StateValueV1::Integer { value: 100 });
    count.replace_revision_for_test(StateRowRevisionV1::new(1));
    let evaluation = PreparedStatefulEvaluationV1::prepare(&bundle, &event, snapshot).unwrap();
    assert_eq!(evaluation.branch(), StatefulEvaluationBranchV1::False);
    assert!(evaluation.writes().is_empty());
    assert_eq!(evaluation.external_nodes()[0].node_id(), "limit_response");
}

#[test]
fn publication_binding_is_exact_to_bundle_not_only_legacy_target() {
    let spec_a = stateful_spec(false);
    let mut spec_b = spec_a.clone();
    spec_b.stateful_workflows[0].on_false.response.content =
        "different stateful response, same filtered legacy target".to_string();
    let bundle_a = compile_stateful_spec_bundle_v1(&spec_a).unwrap();
    let bundle_b = compile_stateful_spec_bundle_v1(&spec_b).unwrap();
    assert_eq!(
        bundle_a.filtered_legacy_target().content_hash,
        bundle_b.filtered_legacy_target().content_hash
    );
    let verified_a = verified_modal_for_bundle(&bundle_a, 120);
    let publication_a =
        StatefulBundlePublicationBindingV1::from_test_authority(&bundle_a, &verified_a).unwrap();
    let verified_b = verified_modal_for_bundle(&bundle_a, 120);
    assert!(matches!(
        EventEnvelopeV1::from_compiled_bundle(&bundle_b, &publication_a, verified_b),
        Err(EventEnvelopeErrorV1::PublicationAuthorityMismatch)
    ));
}

#[test]
fn exact_snapshot_rejects_missing_extra_and_absent_nondefault_reads() {
    let spec = stateful_spec(false);
    let (bundle, event, request) = compiled_event_and_request(&spec, 130);
    let store = InMemoryAtomicStateOutboxStoreV1::default();
    let snapshot = store.read_snapshot_v1(&request).unwrap();

    let mut missing = snapshot.clone();
    missing.reads_mut_for_test().clear();
    assert!(matches!(
        PreparedStatefulEvaluationV1::prepare(&bundle, &event, missing),
        Err(StatefulEvaluationErrorV1::SnapshotMismatch)
    ));

    let mut extra = snapshot.clone();
    let duplicate = extra.reads()[0].clone();
    extra.reads_mut_for_test().push(duplicate);
    assert!(matches!(
        PreparedStatefulEvaluationV1::prepare(&bundle, &event, extra),
        Err(StatefulEvaluationErrorV1::SnapshotMismatch)
    ));

    let mut absent_nondefault = snapshot;
    absent_nondefault.reads_mut_for_test()[0]
        .replace_value_for_test(StateValueV1::Integer { value: 1 });
    assert!(matches!(
        PreparedStatefulEvaluationV1::prepare(&bundle, &event, absent_nondefault),
        Err(StatefulEvaluationErrorV1::SnapshotMismatch)
    ));
}

#[test]
fn evaluation_proof_is_replay_deterministic_and_binds_event_and_state() {
    let spec = stateful_spec(false);
    let store = InMemoryAtomicStateOutboxStoreV1::default();
    let first = compiled_evaluation(&spec, 140, &store);
    let replay = compiled_evaluation(&spec, 140, &store);
    let other_event = compiled_evaluation(&spec, 141, &store);
    assert!(first.proof_digest() == replay.proof_digest());
    assert!(first.proof_digest() != other_event.proof_digest());
    assert_eq!(first.external_nodes()[0].execution_ordinal(), 2);
}

#[test]
fn commit_rejects_external_node_order_drift_from_evaluation() {
    let mut spec = stateful_spec(false);
    spec.stateful_workflows[0]
        .on_true
        .effects
        .push(ActionNodeV1 {
            id: "create_role".to_string(),
            action: ActionV1::CreateRole {
                output: "created_role".to_string(),
                name: "Created Role".to_string(),
            },
        });
    let store = InMemoryAtomicStateOutboxStoreV1::default();
    let evaluation = compiled_evaluation(&spec, 150, &store);
    let result = PreparedStatefulCommitV1::prepare(
        evaluation,
        InteractionActionPlanDigestV1::parse("2".repeat(64)).unwrap(),
        StatefulOutboxPayloadV1::from_canonical_effect_plan(
            vec!["increment_response".to_string(), "create_role".to_string()],
            b"typed-plan-placeholder".to_vec(),
        )
        .unwrap(),
    );
    assert!(matches!(
        result,
        Err(StatefulExecutionPlanErrorV1::InvalidInput)
    ));
}

#[test]
fn runtime_core_evaluates_legal_snapshot_larger_than_simulation_fixture_cap() {
    let mut spec = stateful_spec(false);
    spec.state_variables.clear();
    spec.stateful_workflows[0].condition = StatefulConditionExprV1::InputNonEmpty {
        input_id: "note".to_string(),
    };
    spec.stateful_workflows[0].on_true.state_actions.clear();
    spec.stateful_workflows[0].on_false.state_actions.clear();
    for index in 0..64 {
        let id = format!("text_{index}");
        spec.state_variables.push(StateVariableV1 {
            id: id.clone(),
            scope: StateScopeV1::Actor,
            value_type: StateValueTypeV1::Text {
                max_utf8_bytes: 4_000,
            },
            initial_value: StateValueV1::Text {
                value: String::new(),
            },
        });
        let action = StateSetNodeV1 {
            id: format!("keep_{index}"),
            variable_id: id.clone(),
            value: StatefulValueExprV1::State { variable_id: id },
        };
        if index < 32 {
            spec.stateful_workflows[0]
                .on_true
                .state_actions
                .push(action);
        } else {
            spec.stateful_workflows[0]
                .on_false
                .state_actions
                .push(action);
        }
    }
    let (bundle, event, request) = compiled_event_and_request(&spec, 155);
    assert_eq!(request.len(), 64);
    let store = InMemoryAtomicStateOutboxStoreV1::default();
    let mut snapshot = store.read_snapshot_v1(&request).unwrap();
    for read in snapshot.reads_mut_for_test() {
        read.replace_value_for_test(StateValueV1::Text {
            value: "x".repeat(4_000),
        });
        read.replace_revision_for_test(StateRowRevisionV1::new(1));
    }
    let encoded_values = snapshot
        .reads()
        .iter()
        .map(|read| match read.value() {
            StateValueV1::Text { value } => value.len(),
            _ => 0,
        })
        .sum::<usize>();
    assert!(encoded_values > 64 * 1_024);
    let evaluation = PreparedStatefulEvaluationV1::prepare(&bundle, &event, snapshot).unwrap();
    assert_eq!(evaluation.branch(), StatefulEvaluationBranchV1::True);
    assert_eq!(evaluation.writes().len(), 32);
    assert!(evaluation.verify());
}

#[test]
fn zero_state_conditional_flow_evaluates_without_caller_dependency_subset() {
    let mut spec = stateful_spec(false);
    spec.state_variables.clear();
    spec.stateful_workflows[0].condition = StatefulConditionExprV1::InputNonEmpty {
        input_id: "note".to_string(),
    };
    spec.stateful_workflows[0].on_true.state_actions.clear();
    let store = InMemoryAtomicStateOutboxStoreV1::default();
    let evaluation = compiled_evaluation(&spec, 160, &store);
    assert_eq!(evaluation.branch(), StatefulEvaluationBranchV1::True);
    assert!(evaluation.writes().is_empty());
    assert!(evaluation.verify());
}
