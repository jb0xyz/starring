use std::num::{NonZeroU32, NonZeroUsize};

use automation_instance::{
    AutomationInstance, InMemoryInstanceStore, InstanceId, InstanceKind, InstanceResources,
    InstanceRuleSetVersion, InstanceStatus, InstanceStore,
};
use automation_ruleset::{
    content_hash, RuleSetKey, RuleSetVersion, RuleSetVersionId, CURRENT_RULESET_SCHEMA_VERSION,
};
use automation_runtime_convergence::{
    ActivationRequestId, BindingRevision, DeploymentId, FencingToken, InstallationId,
    ProcessInstanceId, PromotionId, RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1,
    RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use automation_runtime_interaction::{
    InteractionGatewayShardIdentityV1, InteractionRuntimeBuildRevisionV1,
};
use automation_runtime_registry::{
    ExactServingRouteV1, ServingSlotRegistryConfigV1, ServingSlotRegistryV1,
};
use automation_state::InteractionRuleSet;
use discord_model::{ChannelId, GuildId, UserId};
use resource_resolution::{resource_binding_fingerprint_v2, ResourceBindingMap};
use static_assertions::assert_not_impl_any;

use crate::custom_id::{encode_button, encode_instance_action, encode_modal};
use crate::shared_gateway_admission::{
    SharedGatewayAdmissionBudgetV3, SharedGatewayAdmissionConfigV3,
    SharedGatewayAdmittedInteractionV3,
};
use crate::shared_gateway_control::{
    shared_gateway_control_channel_v3, GatewayControlConfigV3, GatewayReadyKindV3,
};
use crate::shared_gateway_dispatcher::{
    SharedGatewayInteractionApplicationIdV3, SharedGatewayInteractionEnvelopeV3,
    SharedGatewayInteractionIdV3, SharedGatewayInteractionIdentityV3,
    SharedGatewayInteractionTokenV3, SharedGatewayModalInputV3,
};
use crate::shared_gateway_router::SharedGatewayRouteHintV1;

use super::*;

const GUILD_ID: GuildId = GuildId(7101);
const RULESET_KEY: &str = "study";

fn deployment_identity() -> RuntimeDeploymentIdentityV1 {
    RuntimeDeploymentIdentityV1 {
        deployment_id: DeploymentId::parse("deployment:receipt-claim").unwrap(),
        tenant_id: TenantId::parse("tenant:receipt-claim").unwrap(),
        installation_id: InstallationId::parse("installation:receipt-claim").unwrap(),
        promotion_id: PromotionId::parse("a".repeat(64)).unwrap(),
        activation_request_id: ActivationRequestId::parse("activation:receipt-claim").unwrap(),
    }
}

fn install_route(
    registry: &ServingSlotRegistryV1,
    fencing_token: FencingToken,
) -> RuntimeProcessIdentityV1 {
    let ruleset_key = RuleSetKey::parse(RULESET_KEY).unwrap();
    let definition = InteractionRuleSet {
        version: 1,
        panels: Vec::new(),
        modals: Vec::new(),
        rules: Vec::new(),
    };
    let content_hash = content_hash(CURRENT_RULESET_SCHEMA_VERSION, &definition).unwrap();
    let bindings = ResourceBindingMap::default();
    let target = RuntimeDeploymentTargetV1 {
        guild_id: GUILD_ID,
        ruleset_key: ruleset_key.clone(),
        version: RuleSetVersionId::FIRST,
        content_hash,
        binding_revision: BindingRevision::FIRST,
        binding_fingerprint: resource_binding_fingerprint_v2(&bindings),
    };
    let process_identity = RuntimeProcessIdentityV1 {
        target: target.clone(),
        runtime_generation: RuntimeGeneration::new(17).unwrap(),
        process_instance_id: ProcessInstanceId::parse("process:receipt-claim").unwrap(),
    };
    let route = ExactServingRouteV1::new(
        deployment_identity(),
        process_identity.clone(),
        RuleSetVersion {
            guild_id: GUILD_ID,
            ruleset_key,
            version: target.version,
            schema_version: CURRENT_RULESET_SCHEMA_VERSION,
            definition,
            content_hash,
            created_by: UserId(81),
        },
        bindings,
    )
    .unwrap();
    let token = registry
        .install(route.slot_key(), route, fencing_token)
        .unwrap()
        .token;
    registry.activate(&token, &process_identity).unwrap();
    process_identity
}

async fn admitted(
    custom_id: &str,
    instance: bool,
    fencing_token: FencingToken,
) -> SharedGatewayAdmittedInteractionV3 {
    let registry = ServingSlotRegistryV1::new(ServingSlotRegistryConfigV1 {
        max_slots: NonZeroU32::new(2).unwrap(),
        max_active_interactions_per_slot: NonZeroU32::new(4).unwrap(),
        max_retired_routes_per_slot: NonZeroU32::new(2).unwrap(),
    });
    install_route(&registry, fencing_token);
    let instances = InMemoryInstanceStore::new();
    if instance {
        instances
            .register(AutomationInstance {
                id: InstanceId::parse("room_001").unwrap(),
                guild_id: GUILD_ID,
                ruleset_key: RULESET_KEY.to_string(),
                ruleset_version: InstanceRuleSetVersion::new(1).unwrap(),
                kind: InstanceKind("study_room".to_string()),
                created_by: UserId(82),
                resources: InstanceResources::default(),
                status: InstanceStatus::Active,
            })
            .await
            .unwrap();
    }
    let (control, mut runtime) =
        shared_gateway_control_channel_v3(GatewayControlConfigV3::default());
    let epoch = runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
    let lease = control.issue_ready_lease(epoch).unwrap();
    SharedGatewayAdmissionBudgetV3::new(
        SharedGatewayAdmissionConfigV3::new(NonZeroUsize::new(4).unwrap()).unwrap(),
    )
    .admit(&control, &lease, &registry, &instances, GUILD_ID, custom_id)
    .await
    .unwrap()
    .unwrap()
}

fn identity(
    guild_id: GuildId,
    application_id: u64,
    interaction_id: u64,
) -> SharedGatewayInteractionIdentityV3 {
    SharedGatewayInteractionIdentityV3::new(
        guild_id,
        ChannelId(7202),
        UserId(7303),
        SharedGatewayInteractionApplicationIdV3::new(application_id).unwrap(),
        SharedGatewayInteractionIdV3::new(interaction_id).unwrap(),
    )
    .unwrap()
}

fn token(value: &str) -> SharedGatewayInteractionTokenV3 {
    SharedGatewayInteractionTokenV3::new(value.to_string()).unwrap()
}

fn gateway_shard() -> InteractionGatewayShardIdentityV1 {
    InteractionGatewayShardIdentityV1::parse("gateway:shard/7").unwrap()
}

fn runtime_build() -> InteractionRuntimeBuildRevisionV1 {
    InteractionRuntimeBuildRevisionV1::parse("runtime-build:receipt-19").unwrap()
}

fn modal(
    inputs: Vec<SharedGatewayModalInputV3>,
    token_value: &str,
) -> SharedGatewayInteractionEnvelopeV3 {
    SharedGatewayInteractionEnvelopeV3::modal_submit_v3(
        identity(GUILD_ID, 7404, 7505),
        encode_modal(GUILD_ID, RULESET_KEY, "submit_room"),
        inputs,
        Some("ko".to_string()),
        token(token_value),
    )
    .unwrap()
}

fn input(component_id: i32, custom_id: &str, value: &str) -> SharedGatewayModalInputV3 {
    SharedGatewayModalInputV3::new(component_id, custom_id.to_string(), value.to_string()).unwrap()
}

#[tokio::test]
async fn modal_input_order_is_canonical_and_value_drift_changes_digest() {
    let custom_id = encode_modal(GUILD_ID, RULESET_KEY, "submit_room");
    let admitted = admitted(&custom_id, false, FencingToken::new(31).unwrap()).await;
    let first = modal(
        vec![
            input(2, "topic", "distributed systems"),
            input(1, "room", "night"),
        ],
        "modal-token-first",
    );
    let reordered = modal(
        vec![
            input(91, "room", "night"),
            input(92, "topic", "distributed systems"),
        ],
        "modal-token-second",
    );
    let changed = modal(
        vec![
            input(1, "room", "morning"),
            input(2, "topic", "distributed systems"),
        ],
        "modal-token-third",
    );
    let first = build_shared_gateway_durable_receipt_claim_input_v1(
        &first,
        &admitted,
        gateway_shard(),
        runtime_build(),
    )
    .unwrap();
    let reordered = build_shared_gateway_durable_receipt_claim_input_v1(
        &reordered,
        &admitted,
        gateway_shard(),
        runtime_build(),
    )
    .unwrap();
    let changed = build_shared_gateway_durable_receipt_claim_input_v1(
        &changed,
        &admitted,
        gateway_shard(),
        runtime_build(),
    )
    .unwrap();
    assert_eq!(
        first.candidate().request_digest(),
        reordered.candidate().request_digest()
    );
    assert_ne!(
        first.candidate().request_digest(),
        changed.candidate().request_digest()
    );
}

#[tokio::test]
async fn button_identity_locale_and_custom_id_drift_change_digest() {
    let baseline_id = encode_button(GUILD_ID, RULESET_KEY, "open_room");
    let admitted = admitted(&baseline_id, false, FencingToken::new(32).unwrap()).await;
    let envelope = |application_id, interaction_id, locale: Option<&str>, custom_id: String| {
        SharedGatewayInteractionEnvelopeV3::message_component_v3(
            identity(GUILD_ID, application_id, interaction_id),
            custom_id,
            locale.map(str::to_string),
            token("button-drift-token"),
        )
        .unwrap()
    };
    let baseline = envelope(7404, 7505, Some("ko"), baseline_id.clone());
    let changed_identity = envelope(7404, 7506, Some("ko"), baseline_id.clone());
    let changed_locale = envelope(7404, 7505, Some("en-US"), baseline_id.clone());
    let changed_custom_id = envelope(
        7404,
        7505,
        Some("ko"),
        encode_button(GUILD_ID, RULESET_KEY, "close_room"),
    );
    let digest = |envelope: &SharedGatewayInteractionEnvelopeV3| {
        build_shared_gateway_durable_receipt_claim_input_v1(
            envelope,
            &admitted,
            gateway_shard(),
            runtime_build(),
        )
        .unwrap()
        .candidate()
        .request_digest()
        .clone()
    };
    let baseline = digest(&baseline);
    assert_ne!(baseline, digest(&changed_identity));
    assert_ne!(baseline, digest(&changed_locale));
    assert_ne!(baseline, digest(&changed_custom_id));
}

#[tokio::test]
async fn static_and_instance_route_hints_remain_typed() {
    let static_id = encode_button(GUILD_ID, RULESET_KEY, "open_room");
    let static_admitted = admitted(&static_id, false, FencingToken::new(33).unwrap()).await;
    let static_envelope = SharedGatewayInteractionEnvelopeV3::message_component_v3(
        identity(GUILD_ID, 7404, 7505),
        static_id,
        None,
        token("static-route-token"),
    )
    .unwrap();
    let static_input = build_shared_gateway_durable_receipt_claim_input_v1(
        &static_envelope,
        &static_admitted,
        gateway_shard(),
        runtime_build(),
    )
    .unwrap();
    assert!(matches!(
        static_input.route_hint(),
        SharedGatewayRouteHintV1::Static(key)
            if key.guild_id() == GUILD_ID && key.ruleset_key().as_str() == RULESET_KEY
    ));

    let instance_id = encode_instance_action("room_001", "join").unwrap();
    let instance_admitted = admitted(&instance_id, true, FencingToken::new(34).unwrap()).await;
    let instance_envelope = SharedGatewayInteractionEnvelopeV3::message_component_v3(
        identity(GUILD_ID, 7404, 7506),
        instance_id,
        None,
        token("instance-route-token"),
    )
    .unwrap();
    let instance_input = build_shared_gateway_durable_receipt_claim_input_v1(
        &instance_envelope,
        &instance_admitted,
        gateway_shard(),
        runtime_build(),
    )
    .unwrap();
    assert!(matches!(
        instance_input.route_hint(),
        SharedGatewayRouteHintV1::Instance(instance_id)
            if instance_id == &InstanceId::parse("room_001").unwrap()
    ));
}

#[tokio::test]
async fn claim_uses_exact_receipt_scope_process_fence_incarnation_build_and_shard() {
    let custom_id = encode_button(GUILD_ID, RULESET_KEY, "open_room");
    let fence = FencingToken::new(47).unwrap();
    let admitted = admitted(&custom_id, false, fence).await;
    let envelope = SharedGatewayInteractionEnvelopeV3::message_component_v3(
        identity(GUILD_ID, 8404, 8505),
        custom_id,
        Some("ko".to_string()),
        token("exact-identity-token"),
    )
    .unwrap();
    let input = build_shared_gateway_durable_receipt_claim_input_v1(
        &envelope,
        &admitted,
        gateway_shard(),
        runtime_build(),
    )
    .unwrap();
    let candidate = input.candidate();
    assert_eq!(candidate.identity().application_id().get(), 8404);
    assert_eq!(candidate.identity().interaction_id().get(), 8505);
    let expected = candidate.expected_route();
    assert_eq!(
        expected.scope().tenant_id().as_str(),
        "tenant:receipt-claim"
    );
    assert_eq!(
        expected.scope().installation_id().as_str(),
        "installation:receipt-claim"
    );
    assert_eq!(
        expected.scope().deployment_id().as_str(),
        "deployment:receipt-claim"
    );
    assert_eq!(
        expected.process_identity(),
        admitted.route().process_identity()
    );
    assert_eq!(expected.route_fencing_token(), fence);
    assert_eq!(
        expected.route_incarnation().get(),
        admitted.token().route_incarnation().get()
    );
    assert_eq!(
        expected.gateway_shard_identity().as_str(),
        "gateway:shard/7"
    );
    assert_eq!(
        expected.runtime_build_revision().as_str(),
        "runtime-build:receipt-19"
    );
}

#[tokio::test]
async fn token_copy_dto_and_errors_are_redacted() {
    assert_not_impl_any!(SharedGatewayDurableReceiptClaimInputV1: Clone, serde::Serialize);
    let custom_id = encode_button(GUILD_ID, RULESET_KEY, "open_room");
    let admitted = admitted(&custom_id, false, FencingToken::new(48).unwrap()).await;
    let marker = "receipt-token-do-not-log-934875";
    let envelope = SharedGatewayInteractionEnvelopeV3::message_component_v3(
        identity(GUILD_ID, 9404, 9505),
        custom_id.clone(),
        None,
        token(marker),
    )
    .unwrap();
    let input = build_shared_gateway_durable_receipt_claim_input_v1(
        &envelope,
        &admitted,
        gateway_shard(),
        runtime_build(),
    )
    .unwrap();
    drop(envelope);
    assert_eq!(input.expose_interaction_token(), marker);
    let rendered = format!("{input:?}");
    assert_eq!(
        rendered,
        "SharedGatewayDurableReceiptClaimInputV1(<redacted>)"
    );
    assert!(!rendered.contains(marker));

    let invalid = SharedGatewayInteractionEnvelopeV3::message_component_v3(
        identity(GuildId(GUILD_ID.0 + 1), 9404, 9506),
        custom_id,
        None,
        token(marker),
    )
    .unwrap();
    let error = build_shared_gateway_durable_receipt_claim_input_v1(
        &invalid,
        &admitted,
        gateway_shard(),
        runtime_build(),
    )
    .unwrap_err();
    assert_eq!(
        error,
        SharedGatewayDurableReceiptClaimInputErrorV1::RouteHint
    );
    assert_eq!(
        error.code(),
        "shared_gateway_durable_receipt_route_hint_invalid"
    );
    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains(marker));
}
