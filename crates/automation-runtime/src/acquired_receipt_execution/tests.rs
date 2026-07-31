use std::collections::{BTreeMap, BTreeSet};
use std::num::{NonZeroU32, NonZeroUsize};
use std::sync::{Arc, Mutex};

use automation_core::{
    AdapterErrorKind, CreateRoleSpec, MockInstanceTeardownService, PostPanelSpec,
};
use automation_instance::{
    AutomationInstance, InMemoryInstanceStore, InstanceId, InstanceKind, InstanceResources,
    InstanceRuleSetVersion, InstanceStore, SequenceInstanceIdGenerator,
};
use automation_instance_teardown::{InstanceTeardownService, TeardownError, TeardownOutcome};
use automation_ruleset::{
    content_hash, RuleSetKey, RuleSetVersion, RuleSetVersionId, CURRENT_RULESET_SCHEMA_VERSION,
};
use automation_ruleset_dispatch::{
    GuildRoleSnapshot, PinnedInstanceResolverErrorV1, ResolvedPinnedInstanceV1, SnapshotError,
};
use automation_runtime_convergence::{
    ActivationRequestId, BindingRevision, DeploymentId, FencingToken, InstallationId,
    ProcessInstanceId, PromotionId, RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1,
    RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use automation_runtime_interaction::{
    InteractionExpectedRouteV1, InteractionGatewayOwnerIdentityV1,
    InteractionGatewayOwnerLeaseEpochV1, InteractionGatewayOwnerRevisionV1,
    InteractionGatewayShardIdentityV1, InteractionInstanceManifestDigestV1,
    InteractionProductScopeV1, InteractionReceiptClaimCandidateV1, InteractionReceiptIdentityV1,
    InteractionRouteAttestationDigestV1, InteractionRouteBindingV1, InteractionRouteIncarnationV1,
    InteractionRuntimeBuildRevisionV1, InteractionServingLeaseEpochV1,
    InteractionServingLeaseRevisionV1, InteractionServingRouteIdentityV1,
};
use automation_runtime_registry::{
    ExactServingRouteV1, ServingSlotRegistryConfigV1, ServingSlotRegistryV1,
};
use automation_state::{
    ActionSpec, InstanceRef, InteractionRule, InteractionRuleSet, ModalFieldSpec, ModalFieldStyle,
    ModalInputPolicy, ModalSpec, TriggerSpec,
};
use discord_model::{ChannelId, GuildId, MessageId, OverwriteTarget, Permissions, RoleId, UserId};
use resource_resolution::{resource_binding_fingerprint_v2, ResourceBindingMap};

use crate::custom_id::{encode_button, encode_instance_action, encode_modal};
use crate::shared_gateway_admission::{
    SharedGatewayAdmissionBudgetV3, SharedGatewayAdmissionConfigV3,
};
use crate::shared_gateway_control::{
    shared_gateway_control_channel_v3, GatewayControlConfigV3, GatewayReadyKindV3,
};
use crate::shared_gateway_dispatcher::{
    SharedGatewayInteractionApplicationIdV3, SharedGatewayInteractionIdV3,
    SharedGatewayInteractionIdentityV3, SharedGatewayInteractionTokenV3, SharedGatewayModalInputV3,
};

use super::*;

const GUILD_ID: GuildId = GuildId(41_001);
const RULESET_KEY: &str = "receipt_flow";

#[derive(Clone, Copy, Default)]
struct PermitFailuresV1 {
    initial_intent: bool,
    suppress_initial_intent: bool,
    initial_result: bool,
    bind: bool,
    execution_intent: bool,
    finish: bool,
}

#[derive(Debug)]
struct PermitErrorV1;

#[derive(Default)]
struct PermitStateV1 {
    trace: Mutex<Vec<String>>,
    intents: Mutex<Vec<InteractionInitialResponseIntentV1>>,
    results: Mutex<Vec<InteractionInitialResponseResultV1>>,
    binds: Mutex<Vec<InteractionActionPlanDigestV1>>,
    finishes: Mutex<Vec<InteractionTerminalFinishV1>>,
}

struct FakeLifecyclePermitV1 {
    root: InteractionReceiptClaimRootV1,
    state: Arc<PermitStateV1>,
    failures: PermitFailuresV1,
}

impl InteractionEffectPermitV1 for FakeLifecyclePermitV1 {
    type Error = PermitErrorV1;

    async fn commit_initial_response_intent_v1(
        &self,
        intent: &InteractionInitialResponseIntentV1,
    ) -> Result<InteractionInitialResponseIntentDispositionV1, Self::Error> {
        self.state
            .trace
            .lock()
            .unwrap()
            .push(format!("permit.intent:{}", intent.kind().code()));
        if self.failures.initial_intent {
            return Err(PermitErrorV1);
        }
        self.state.intents.lock().unwrap().push(intent.clone());
        if self.failures.suppress_initial_intent {
            Ok(InteractionInitialResponseIntentDispositionV1::ExactReplaySuppressed)
        } else {
            Ok(InteractionInitialResponseIntentDispositionV1::ExternalCallAuthorized)
        }
    }

    async fn commit_initial_response_result_v1(
        &self,
        result: &InteractionInitialResponseResultV1,
    ) -> Result<(), Self::Error> {
        self.state
            .trace
            .lock()
            .unwrap()
            .push(format!("permit.result:{}", result.result().code()));
        if self.failures.initial_result {
            return Err(PermitErrorV1);
        }
        self.state.results.lock().unwrap().push(result.clone());
        Ok(())
    }

    async fn commit_idempotent_execution_intent_v1(&self) -> Result<(), Self::Error> {
        self.state
            .trace
            .lock()
            .unwrap()
            .push("permit.execution_intent".to_string());
        if self.failures.execution_intent {
            return Err(PermitErrorV1);
        }
        Ok(())
    }
}

impl AcquiredInteractionLifecyclePermitV1 for FakeLifecyclePermitV1 {
    fn authoritative_claim_v1(&self) -> AuthoritativeInteractionClaimV1<'_> {
        AuthoritativeInteractionClaimV1::new(&self.root)
    }

    async fn bind_action_plan_digest_v1(
        &self,
        digest: &InteractionActionPlanDigestV1,
    ) -> Result<(), Self::Error> {
        self.state
            .trace
            .lock()
            .unwrap()
            .push("permit.bind".to_string());
        if self.failures.bind {
            return Err(PermitErrorV1);
        }
        self.state.binds.lock().unwrap().push(digest.clone());
        Ok(())
    }

    async fn finish_interaction_v1(
        &self,
        finish: &InteractionTerminalFinishV1,
    ) -> Result<(), Self::Error> {
        self.state
            .trace
            .lock()
            .unwrap()
            .push(format!("permit.finish:{}", finish.outcome_code()));
        if self.failures.finish {
            return Err(PermitErrorV1);
        }
        self.state.finishes.lock().unwrap().push(finish.clone());
        Ok(())
    }
}

struct FakeResponderV1 {
    state: Arc<PermitStateV1>,
    initial_error: Option<AdapterError>,
    edit_error: Option<AdapterError>,
}

impl FakeResponderV1 {
    fn successful(state: Arc<PermitStateV1>) -> Self {
        Self {
            state,
            initial_error: None,
            edit_error: None,
        }
    }
}

impl InteractionResponder for FakeResponderV1 {
    async fn respond_ephemeral(&self, _: String) -> Result<(), AdapterError> {
        self.state
            .trace
            .lock()
            .unwrap()
            .push("external.respond".to_string());
        self.initial_error.clone().map_or(Ok(()), Err)
    }

    async fn open_modal(&self, _: &automation_core::ModalPresentation) -> Result<(), AdapterError> {
        self.state
            .trace
            .lock()
            .unwrap()
            .push("external.modal".to_string());
        self.initial_error.clone().map_or(Ok(()), Err)
    }

    async fn defer_ephemeral(&self) -> Result<(), AdapterError> {
        self.state
            .trace
            .lock()
            .unwrap()
            .push("external.defer".to_string());
        self.initial_error.clone().map_or(Ok(()), Err)
    }

    async fn edit_response(&self, _: String) -> Result<(), AdapterError> {
        self.state
            .trace
            .lock()
            .unwrap()
            .push("external.edit".to_string());
        self.edit_error.clone().map_or(Ok(()), Err)
    }
}

struct FakeMutationV1 {
    state: Arc<PermitStateV1>,
    error: Option<AdapterError>,
}

struct FakeTeardownV1 {
    state: Arc<PermitStateV1>,
}

impl InstanceTeardownService for FakeTeardownV1 {
    async fn teardown(&self, _: GuildId, _: InstanceId) -> Result<TeardownOutcome, TeardownError> {
        self.state
            .trace
            .lock()
            .unwrap()
            .push("external.teardown".to_string());
        Ok(TeardownOutcome::Completed)
    }
}

impl FakeMutationV1 {
    fn successful(state: Arc<PermitStateV1>) -> Self {
        Self { state, error: None }
    }
}

impl DiscordMutationAdapter for FakeMutationV1 {
    async fn grant_role(&self, _: GuildId, _: UserId, _: RoleId) -> Result<(), AdapterError> {
        self.state
            .trace
            .lock()
            .unwrap()
            .push("external.grant_role".to_string());
        self.error.clone().map_or(Ok(()), Err)
    }

    async fn create_role(&self, _: GuildId, _: CreateRoleSpec) -> Result<RoleId, AdapterError> {
        self.state
            .trace
            .lock()
            .unwrap()
            .push("external.create_role".to_string());
        match &self.error {
            Some(error) => Err(error.clone()),
            None => Ok(RoleId(55_001)),
        }
    }

    async fn post_panel(
        &self,
        _: GuildId,
        _: ChannelId,
        _: PostPanelSpec,
    ) -> Result<MessageId, AdapterError> {
        self.state
            .trace
            .lock()
            .unwrap()
            .push("external.post_panel".to_string());
        match &self.error {
            Some(error) => Err(error.clone()),
            None => Ok(MessageId(55_002)),
        }
    }

    async fn upsert_overwrite(
        &self,
        _: GuildId,
        _: ChannelId,
        _: OverwriteTarget,
        _: Permissions,
        _: Permissions,
    ) -> Result<(), AdapterError> {
        self.state
            .trace
            .lock()
            .unwrap()
            .push("external.upsert_overwrite".to_string());
        self.error.clone().map_or(Ok(()), Err)
    }
}

struct FakePinnedResolverV1 {
    state: Arc<PermitStateV1>,
    resolved: Option<ResolvedPinnedInstanceV1>,
}

impl PinnedInstanceResolverV1 for FakePinnedResolverV1 {
    async fn resolve_pinned_instance_v1(
        &self,
        _: GuildId,
        _: &InstanceId,
    ) -> Result<ResolvedPinnedInstanceV1, PinnedInstanceResolverErrorV1> {
        self.state
            .trace
            .lock()
            .unwrap()
            .push("prepare.resolve".to_string());
        self.resolved
            .clone()
            .ok_or(PinnedInstanceResolverErrorV1::InstanceNotFound)
    }
}

struct FakeSnapshotProviderV1 {
    state: Arc<PermitStateV1>,
    fail: bool,
}

impl GuildRoleSnapshotProvider for FakeSnapshotProviderV1 {
    async fn snapshot(&self, _: GuildId) -> Result<GuildRoleSnapshot, SnapshotError> {
        self.state
            .trace
            .lock()
            .unwrap()
            .push("prepare.snapshot".to_string());
        if self.fail {
            return Err(SnapshotError::new("snapshot-secret"));
        }
        Ok(GuildRoleSnapshot {
            roles: BTreeMap::from([(RoleId(GUILD_ID.0), Permissions::ADMINISTRATOR)]),
            bot_role_ids: BTreeSet::new(),
        })
    }
}

struct FakeTeardownSnapshotProviderV1 {
    state: Arc<PermitStateV1>,
}

impl GuildRoleSnapshotProvider for FakeTeardownSnapshotProviderV1 {
    async fn snapshot(&self, _: GuildId) -> Result<GuildRoleSnapshot, SnapshotError> {
        self.state
            .trace
            .lock()
            .unwrap()
            .push("prepare.snapshot".to_string());
        let bot_role = RoleId(41_006);
        Ok(GuildRoleSnapshot {
            roles: BTreeMap::from([
                (RoleId(GUILD_ID.0), Permissions::empty()),
                (bot_role, Permissions::ADMINISTRATOR),
            ]),
            bot_role_ids: BTreeSet::from([bot_role]),
        })
    }
}

fn deployment_identity() -> RuntimeDeploymentIdentityV1 {
    RuntimeDeploymentIdentityV1 {
        deployment_id: DeploymentId::parse("deployment:receipt-flow").unwrap(),
        tenant_id: TenantId::parse("tenant:receipt-flow").unwrap(),
        installation_id: InstallationId::parse("installation:receipt-flow").unwrap(),
        promotion_id: PromotionId::parse("a".repeat(64)).unwrap(),
        activation_request_id: ActivationRequestId::parse("activation:receipt-flow").unwrap(),
    }
}

fn artifact(definition: InteractionRuleSet) -> RuleSetVersion {
    let ruleset_key = RuleSetKey::parse(RULESET_KEY).unwrap();
    let hash = content_hash(CURRENT_RULESET_SCHEMA_VERSION, &definition).unwrap();
    RuleSetVersion {
        guild_id: GUILD_ID,
        ruleset_key,
        version: RuleSetVersionId::FIRST,
        schema_version: CURRENT_RULESET_SCHEMA_VERSION,
        definition,
        content_hash: hash,
        created_by: UserId(41_002),
    }
}

async fn admitted_v1(
    artifact: RuleSetVersion,
    custom_id: &str,
    instance: Option<AutomationInstance>,
) -> SharedGatewayAdmittedInteractionV3 {
    let bindings = ResourceBindingMap::default();
    let target = RuntimeDeploymentTargetV1 {
        guild_id: GUILD_ID,
        ruleset_key: artifact.ruleset_key.clone(),
        version: artifact.version,
        content_hash: artifact.content_hash,
        binding_revision: BindingRevision::FIRST,
        binding_fingerprint: resource_binding_fingerprint_v2(&bindings),
    };
    let process = RuntimeProcessIdentityV1 {
        target: target.clone(),
        runtime_generation: RuntimeGeneration::new(9).unwrap(),
        process_instance_id: ProcessInstanceId::parse("process:receipt-flow").unwrap(),
    };
    let route =
        ExactServingRouteV1::new(deployment_identity(), process.clone(), artifact, bindings)
            .unwrap();
    let registry = ServingSlotRegistryV1::new(ServingSlotRegistryConfigV1 {
        max_slots: NonZeroU32::new(2).unwrap(),
        max_active_interactions_per_slot: NonZeroU32::new(4).unwrap(),
        max_retired_routes_per_slot: NonZeroU32::new(2).unwrap(),
    });
    let token = registry
        .install(route.slot_key(), route, FencingToken::new(7).unwrap())
        .unwrap()
        .token;
    registry.activate(&token, &process).unwrap();
    let instances = InMemoryInstanceStore::new();
    if let Some(instance) = instance {
        instances.register(instance).await.unwrap();
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

fn envelope_v1(custom_id: String, interaction_id: u64) -> SharedGatewayInteractionEnvelopeV3 {
    SharedGatewayInteractionEnvelopeV3::message_component_v3(
        interaction_identity_v1(interaction_id),
        custom_id,
        Some("ko".to_string()),
        SharedGatewayInteractionTokenV3::new("receipt-flow-token".to_string()).unwrap(),
    )
    .unwrap()
}

fn modal_envelope_v1(
    custom_id: String,
    interaction_id: u64,
    inputs: Vec<SharedGatewayModalInputV3>,
) -> SharedGatewayInteractionEnvelopeV3 {
    SharedGatewayInteractionEnvelopeV3::modal_submit_v3(
        interaction_identity_v1(interaction_id),
        custom_id,
        inputs,
        Some("ko".to_string()),
        SharedGatewayInteractionTokenV3::new("receipt-flow-token".to_string()).unwrap(),
    )
    .unwrap()
}

fn interaction_identity_v1(interaction_id: u64) -> SharedGatewayInteractionIdentityV3 {
    SharedGatewayInteractionIdentityV3::new(
        GUILD_ID,
        ChannelId(41_003),
        UserId(41_004),
        SharedGatewayInteractionApplicationIdV3::new(41_005).unwrap(),
        SharedGatewayInteractionIdV3::new(interaction_id).unwrap(),
    )
    .unwrap()
}

fn claim_root_v1(
    admitted: &SharedGatewayAdmittedInteractionV3,
    envelope: &SharedGatewayInteractionEnvelopeV3,
    instance_id: Option<&InstanceId>,
) -> InteractionReceiptClaimRootV1 {
    let route = admitted.route();
    let serving = InteractionServingRouteIdentityV1::new(
        InteractionRouteAttestationDigestV1::parse("b".repeat(64)).unwrap(),
        InteractionServingLeaseEpochV1::new(3).unwrap(),
        InteractionServingLeaseRevisionV1::new(4).unwrap(),
        InteractionGatewayOwnerIdentityV1::new(
            InteractionGatewayShardIdentityV1::parse("gateway-shard-receipt-flow").unwrap(),
            InteractionGatewayOwnerLeaseEpochV1::new(5).unwrap(),
            InteractionGatewayOwnerRevisionV1::new(6).unwrap(),
            InteractionRuntimeBuildRevisionV1::parse("build-receipt-flow").unwrap(),
        ),
        admitted.token().fencing_token(),
        InteractionRouteIncarnationV1::new(admitted.token().route_incarnation().get()).unwrap(),
    );
    let scope = InteractionProductScopeV1::from_deployment_identity(route.deployment_identity());
    let authoritative = match instance_id {
        Some(instance_id) => InteractionRouteBindingV1::new_instance(
            scope,
            route.process_identity().clone(),
            serving,
            instance_id.clone(),
            route.process_identity().target.version,
            route.process_identity().target.content_hash,
            InteractionInstanceManifestDigestV1::parse("c".repeat(64)).unwrap(),
        )
        .unwrap(),
        None => {
            InteractionRouteBindingV1::new_static(scope, route.process_identity().clone(), serving)
                .unwrap()
        }
    };
    let envelope_identity = envelope.identity_v3();
    let identity = InteractionReceiptIdentityV1::new(
        automation_runtime_interaction::DiscordApplicationIdV1::new(
            envelope_identity.application_id().get(),
        )
        .unwrap(),
        automation_runtime_interaction::DiscordInteractionIdV1::new(
            envelope_identity.interaction_id().get(),
        )
        .unwrap(),
    );
    let request = envelope.receipt_request_digest_v1(identity).unwrap();
    InteractionReceiptClaimCandidateV1::new(
        identity,
        InteractionExpectedRouteV1::from_authoritative(&authoritative),
        request,
    )
    .bind_authoritative(authoritative)
    .unwrap()
}

fn permit_v1(
    root: InteractionReceiptClaimRootV1,
    failures: PermitFailuresV1,
) -> (FakeLifecyclePermitV1, Arc<PermitStateV1>) {
    let state = Arc::new(PermitStateV1::default());
    (
        FakeLifecyclePermitV1 {
            root,
            state: Arc::clone(&state),
            failures,
        },
        state,
    )
}

fn services_v1<'a>(
    responder: &'a FakeResponderV1,
    mutation: &'a FakeMutationV1,
    instances: &'a InMemoryInstanceStore,
    instance_ids: &'a SequenceInstanceIdGenerator,
    teardown: &'a MockInstanceTeardownService,
    resolver: &'a FakePinnedResolverV1,
    snapshot: &'a FakeSnapshotProviderV1,
) -> AcquiredInteractionExecutionServicesV1<
    'a,
    FakeMutationV1,
    FakeResponderV1,
    InMemoryInstanceStore,
    SequenceInstanceIdGenerator,
    MockInstanceTeardownService,
    FakePinnedResolverV1,
    FakeSnapshotProviderV1,
> {
    AcquiredInteractionExecutionServicesV1::new(
        mutation,
        responder,
        instances,
        instance_ids,
        teardown,
        resolver,
        snapshot,
    )
}

fn static_ruleset(actions: Vec<ActionSpec>) -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: Vec::new(),
        modals: Vec::new(),
        rules: vec![InteractionRule {
            key: "static".to_string(),
            trigger: TriggerSpec::ButtonClick {
                component: "go".to_string(),
            },
            actions,
        }],
    }
}

fn modal_definition(actions: Vec<ActionSpec>) -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: Vec::new(),
        modals: vec![ModalSpec {
            key: "room".to_string(),
            title: "Room".to_string(),
            fields: vec![ModalFieldSpec {
                key: "room_name".to_string(),
                label: "Room name".to_string(),
                style: ModalFieldStyle::Short,
                required: true,
                min_length: Some(2),
                max_length: Some(40),
                input_policy: ModalInputPolicy::TrimUnicodeWhitespace,
            }],
        }],
        rules: vec![InteractionRule {
            key: "modal".to_string(),
            trigger: TriggerSpec::ModalSubmit {
                modal: "room".to_string(),
            },
            actions,
        }],
    }
}

fn instance_definition() -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: Vec::new(),
        modals: Vec::new(),
        rules: vec![InteractionRule {
            key: "join".to_string(),
            trigger: TriggerSpec::InstanceAction {
                action: "join".to_string(),
            },
            actions: vec![
                ActionSpec::DeferEphemeral,
                ActionSpec::EditResponse {
                    content: "joined".to_string(),
                },
            ],
        }],
    }
}

fn instance_teardown_definition() -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: Vec::new(),
        modals: Vec::new(),
        rules: vec![InteractionRule {
            key: "close".to_string(),
            trigger: TriggerSpec::InstanceAction {
                action: "close".to_string(),
            },
            actions: vec![
                ActionSpec::DeferEphemeral,
                ActionSpec::TeardownInstance {
                    instance: InstanceRef::Event,
                },
                ActionSpec::EditResponse {
                    content: "closed".to_string(),
                },
            ],
        }],
    }
}

fn active_instance_v1() -> AutomationInstance {
    AutomationInstance {
        id: InstanceId::parse("room_001").unwrap(),
        guild_id: GUILD_ID,
        ruleset_key: RULESET_KEY.to_string(),
        ruleset_version: InstanceRuleSetVersion::new(1).unwrap(),
        kind: InstanceKind("study_room".to_string()),
        created_by: UserId(41_004),
        resources: InstanceResources::default(),
        status: automation_instance::InstanceStatus::Active,
    }
}

fn assert_terminalized_v1(
    outcome: AcquiredInteractionExecutionOutcomeV1,
    expected: AcquiredInteractionTerminalOutcomeV1,
) -> InteractionTerminalFinishV1 {
    match outcome {
        AcquiredInteractionExecutionOutcomeV1::Terminalized(finish) => {
            assert_eq!(finish.outcome(), expected);
            assert_eq!(finish.state(), expected.state());
            finish
        }
        other => panic!("unexpected outcome {other:?}"),
    }
}

#[tokio::test]
async fn static_response_binds_before_http_and_terminalizes() {
    let definition = static_ruleset(vec![ActionSpec::RespondEphemeral {
        content: "hello".to_string(),
    }]);
    let admitted = admitted_v1(
        artifact(definition),
        &encode_button(GUILD_ID, RULESET_KEY, "go"),
        None,
    )
    .await;
    let envelope = envelope_v1(encode_button(GUILD_ID, RULESET_KEY, "go"), 51_001);
    let root = claim_root_v1(&admitted, &envelope, None);
    let (permit, state) = permit_v1(root, PermitFailuresV1::default());
    let responder = FakeResponderV1::successful(Arc::clone(&state));
    let mutation = FakeMutationV1::successful(Arc::clone(&state));
    let instances = InMemoryInstanceStore::new();
    let ids = SequenceInstanceIdGenerator::new("receipt", 1);
    let teardown = MockInstanceTeardownService::new();
    let resolver = FakePinnedResolverV1 {
        state: Arc::clone(&state),
        resolved: None,
    };
    let snapshot = FakeSnapshotProviderV1 {
        state: Arc::clone(&state),
        fail: false,
    };

    let outcome = execute_acquired_interaction_v1(
        admitted,
        envelope,
        permit,
        services_v1(
            &responder, &mutation, &instances, &ids, &teardown, &resolver, &snapshot,
        ),
    )
    .await;
    let finish = assert_terminalized_v1(
        outcome,
        AcquiredInteractionTerminalOutcomeV1::StaticCompleted,
    );
    assert!(finish.action_plan_digest().is_some());
    assert_eq!(
        *state.trace.lock().unwrap(),
        [
            "permit.bind",
            "permit.intent:respond_ephemeral",
            "external.respond",
            "permit.result:succeeded",
            "permit.finish:interaction_static_completed",
        ]
    );
}

#[tokio::test]
async fn exact_replay_disposition_crosses_tracking_without_discord_or_result_commit() {
    let definition = static_ruleset(vec![ActionSpec::RespondEphemeral {
        content: "already-succeeded".to_string(),
    }]);
    let admitted = admitted_v1(
        artifact(definition),
        &encode_button(GUILD_ID, RULESET_KEY, "go"),
        None,
    )
    .await;
    let envelope = envelope_v1(encode_button(GUILD_ID, RULESET_KEY, "go"), 51_021);
    let root = claim_root_v1(&admitted, &envelope, None);
    let (permit, state) = permit_v1(
        root,
        PermitFailuresV1 {
            suppress_initial_intent: true,
            ..PermitFailuresV1::default()
        },
    );
    let responder = FakeResponderV1::successful(Arc::clone(&state));
    let mutation = FakeMutationV1::successful(Arc::clone(&state));
    let instances = InMemoryInstanceStore::new();
    let ids = SequenceInstanceIdGenerator::new("receipt", 1);
    let teardown = MockInstanceTeardownService::new();
    let resolver = FakePinnedResolverV1 {
        state: Arc::clone(&state),
        resolved: None,
    };
    let snapshot = FakeSnapshotProviderV1 {
        state: Arc::clone(&state),
        fail: false,
    };

    let outcome = execute_acquired_interaction_v1(
        admitted,
        envelope,
        permit,
        services_v1(
            &responder, &mutation, &instances, &ids, &teardown, &resolver, &snapshot,
        ),
    )
    .await;

    assert_terminalized_v1(
        outcome,
        AcquiredInteractionTerminalOutcomeV1::StaticCompleted,
    );
    assert_eq!(
        *state.trace.lock().unwrap(),
        [
            "permit.bind",
            "permit.intent:respond_ephemeral",
            "permit.finish:interaction_static_completed",
        ]
    );
    assert!(state.results.lock().unwrap().is_empty());
}

#[tokio::test]
async fn modal_response_uses_the_same_bound_fence() {
    let mut definition = static_ruleset(vec![ActionSpec::OpenModal {
        modal: "room".to_string(),
    }]);
    definition.modals = modal_definition(Vec::new()).modals;
    let admitted = admitted_v1(
        artifact(definition),
        &encode_button(GUILD_ID, RULESET_KEY, "go"),
        None,
    )
    .await;
    let envelope = envelope_v1(encode_button(GUILD_ID, RULESET_KEY, "go"), 51_002);
    let root = claim_root_v1(&admitted, &envelope, None);
    let (permit, state) = permit_v1(root, PermitFailuresV1::default());
    let responder = FakeResponderV1::successful(Arc::clone(&state));
    let mutation = FakeMutationV1::successful(Arc::clone(&state));
    let instances = InMemoryInstanceStore::new();
    let ids = SequenceInstanceIdGenerator::new("receipt", 1);
    let teardown = MockInstanceTeardownService::new();
    let resolver = FakePinnedResolverV1 {
        state: Arc::clone(&state),
        resolved: None,
    };
    let snapshot = FakeSnapshotProviderV1 {
        state: Arc::clone(&state),
        fail: false,
    };

    let outcome = execute_acquired_interaction_v1(
        admitted,
        envelope,
        permit,
        services_v1(
            &responder, &mutation, &instances, &ids, &teardown, &resolver, &snapshot,
        ),
    )
    .await;
    assert_terminalized_v1(
        outcome,
        AcquiredInteractionTerminalOutcomeV1::StaticCompleted,
    );
    assert_eq!(
        *state.trace.lock().unwrap(),
        [
            "permit.bind",
            "permit.intent:open_modal",
            "external.modal",
            "permit.result:succeeded",
            "permit.finish:interaction_static_completed",
        ]
    );
}

#[tokio::test]
async fn defer_first_instance_defers_before_resolve_snapshot_and_bind() {
    let definition = instance_definition();
    let pinned_artifact = artifact(definition.clone());
    let instance = active_instance_v1();
    let custom_id = encode_instance_action(instance.id.as_str(), "join").unwrap();
    let admitted = admitted_v1(artifact(definition), &custom_id, Some(instance.clone())).await;
    let envelope = envelope_v1(custom_id, 51_003);
    let root = claim_root_v1(&admitted, &envelope, Some(&instance.id));
    let (permit, state) = permit_v1(root, PermitFailuresV1::default());
    let responder = FakeResponderV1::successful(Arc::clone(&state));
    let mutation = FakeMutationV1::successful(Arc::clone(&state));
    let instances = InMemoryInstanceStore::new();
    let ids = SequenceInstanceIdGenerator::new("receipt", 1);
    let teardown = MockInstanceTeardownService::new();
    let resolver = FakePinnedResolverV1 {
        state: Arc::clone(&state),
        resolved: Some(ResolvedPinnedInstanceV1 {
            instance,
            artifact: pinned_artifact,
        }),
    };
    let snapshot = FakeSnapshotProviderV1 {
        state: Arc::clone(&state),
        fail: false,
    };

    let outcome = execute_acquired_interaction_v1(
        admitted,
        envelope,
        permit,
        services_v1(
            &responder, &mutation, &instances, &ids, &teardown, &resolver, &snapshot,
        ),
    )
    .await;
    assert_terminalized_v1(
        outcome,
        AcquiredInteractionTerminalOutcomeV1::InstanceCompleted,
    );
    assert_eq!(
        *state.trace.lock().unwrap(),
        [
            "permit.intent:defer_ephemeral",
            "external.defer",
            "permit.result:succeeded",
            "prepare.resolve",
            "prepare.snapshot",
            "permit.bind",
            "permit.execution_intent",
            "external.edit",
            "permit.finish:interaction_instance_completed",
        ]
    );
}

#[tokio::test]
async fn instance_teardown_crosses_execution_fence_before_the_service() {
    let definition = instance_teardown_definition();
    let pinned_artifact = artifact(definition.clone());
    let instance = active_instance_v1();
    let custom_id = encode_instance_action(instance.id.as_str(), "close").unwrap();
    let admitted = admitted_v1(artifact(definition), &custom_id, Some(instance.clone())).await;
    let envelope = envelope_v1(custom_id, 51_022);
    let root = claim_root_v1(&admitted, &envelope, Some(&instance.id));
    let (permit, state) = permit_v1(root, PermitFailuresV1::default());
    let responder = FakeResponderV1::successful(Arc::clone(&state));
    let mutation = FakeMutationV1::successful(Arc::clone(&state));
    let instances = InMemoryInstanceStore::new();
    let ids = SequenceInstanceIdGenerator::new("receipt", 1);
    let teardown = FakeTeardownV1 {
        state: Arc::clone(&state),
    };
    let resolver = FakePinnedResolverV1 {
        state: Arc::clone(&state),
        resolved: Some(ResolvedPinnedInstanceV1 {
            instance,
            artifact: pinned_artifact,
        }),
    };
    let snapshot = FakeTeardownSnapshotProviderV1 {
        state: Arc::clone(&state),
    };

    let outcome = execute_acquired_interaction_v1(
        admitted,
        envelope,
        permit,
        AcquiredInteractionExecutionServicesV1::new(
            &mutation, &responder, &instances, &ids, &teardown, &resolver, &snapshot,
        ),
    )
    .await;

    assert_terminalized_v1(
        outcome,
        AcquiredInteractionTerminalOutcomeV1::InstanceCompleted,
    );
    assert_eq!(
        *state.trace.lock().unwrap(),
        [
            "permit.intent:defer_ephemeral",
            "external.defer",
            "permit.result:succeeded",
            "prepare.resolve",
            "prepare.snapshot",
            "permit.bind",
            "permit.execution_intent",
            "external.teardown",
            "permit.execution_intent",
            "external.edit",
            "permit.finish:interaction_instance_completed",
        ]
    );
}

#[tokio::test]
async fn new_permit_execution_replay_denial_has_zero_teardown_effects() {
    let definition = instance_teardown_definition();
    let pinned_artifact = artifact(definition.clone());
    let instance = active_instance_v1();
    let custom_id = encode_instance_action(instance.id.as_str(), "close").unwrap();
    let admitted = admitted_v1(artifact(definition), &custom_id, Some(instance.clone())).await;
    let envelope = envelope_v1(custom_id, 51_023);
    let root = claim_root_v1(&admitted, &envelope, Some(&instance.id));
    let (permit, state) = permit_v1(
        root,
        PermitFailuresV1 {
            execution_intent: true,
            ..PermitFailuresV1::default()
        },
    );
    let responder = FakeResponderV1::successful(Arc::clone(&state));
    let mutation = FakeMutationV1::successful(Arc::clone(&state));
    let instances = InMemoryInstanceStore::new();
    let ids = SequenceInstanceIdGenerator::new("receipt", 1);
    let teardown = FakeTeardownV1 {
        state: Arc::clone(&state),
    };
    let resolver = FakePinnedResolverV1 {
        state: Arc::clone(&state),
        resolved: Some(ResolvedPinnedInstanceV1 {
            instance,
            artifact: pinned_artifact,
        }),
    };
    let snapshot = FakeTeardownSnapshotProviderV1 {
        state: Arc::clone(&state),
    };

    let outcome = execute_acquired_interaction_v1(
        admitted,
        envelope,
        permit,
        AcquiredInteractionExecutionServicesV1::new(
            &mutation, &responder, &instances, &ids, &teardown, &resolver, &snapshot,
        ),
    )
    .await;

    assert_eq!(
        outcome,
        AcquiredInteractionExecutionOutcomeV1::PersistenceFailed {
            stage: AcquiredInteractionPersistenceStageV1::ExecutionIntent,
            external_effect_may_have_occurred: true,
        }
    );
    assert_eq!(
        *state.trace.lock().unwrap(),
        [
            "permit.intent:defer_ephemeral",
            "external.defer",
            "permit.result:succeeded",
            "prepare.resolve",
            "prepare.snapshot",
            "permit.bind",
            "permit.execution_intent",
        ]
    );
}

#[tokio::test]
async fn mutation_before_response_is_fenced_in_plan_order() {
    let definition = static_ruleset(vec![
        ActionSpec::CreateRole {
            key: "member".to_string(),
            name: "Member".to_string(),
        },
        ActionSpec::RespondEphemeral {
            content: "created".to_string(),
        },
    ]);
    let admitted = admitted_v1(
        artifact(definition),
        &encode_button(GUILD_ID, RULESET_KEY, "go"),
        None,
    )
    .await;
    let envelope = envelope_v1(encode_button(GUILD_ID, RULESET_KEY, "go"), 51_004);
    let root = claim_root_v1(&admitted, &envelope, None);
    let (permit, state) = permit_v1(root, PermitFailuresV1::default());
    let responder = FakeResponderV1::successful(Arc::clone(&state));
    let mutation = FakeMutationV1::successful(Arc::clone(&state));
    let instances = InMemoryInstanceStore::new();
    let ids = SequenceInstanceIdGenerator::new("receipt", 1);
    let teardown = MockInstanceTeardownService::new();
    let resolver = FakePinnedResolverV1 {
        state: Arc::clone(&state),
        resolved: None,
    };
    let snapshot = FakeSnapshotProviderV1 {
        state: Arc::clone(&state),
        fail: false,
    };

    let outcome = execute_acquired_interaction_v1(
        admitted,
        envelope,
        permit,
        services_v1(
            &responder, &mutation, &instances, &ids, &teardown, &resolver, &snapshot,
        ),
    )
    .await;
    assert_terminalized_v1(
        outcome,
        AcquiredInteractionTerminalOutcomeV1::StaticCompleted,
    );
    assert_eq!(
        *state.trace.lock().unwrap(),
        [
            "permit.bind",
            "permit.execution_intent",
            "external.create_role",
            "permit.intent:respond_ephemeral",
            "external.respond",
            "permit.result:succeeded",
            "permit.finish:interaction_static_completed",
        ]
    );
}

#[tokio::test]
async fn mutation_only_plan_finishes_without_inventing_acknowledgement() {
    let definition = static_ruleset(vec![ActionSpec::CreateRole {
        key: "member".to_string(),
        name: "Member".to_string(),
    }]);
    let admitted = admitted_v1(
        artifact(definition),
        &encode_button(GUILD_ID, RULESET_KEY, "go"),
        None,
    )
    .await;
    let envelope = envelope_v1(encode_button(GUILD_ID, RULESET_KEY, "go"), 51_005);
    let root = claim_root_v1(&admitted, &envelope, None);
    let (permit, state) = permit_v1(root, PermitFailuresV1::default());
    let responder = FakeResponderV1::successful(Arc::clone(&state));
    let mutation = FakeMutationV1::successful(Arc::clone(&state));
    let instances = InMemoryInstanceStore::new();
    let ids = SequenceInstanceIdGenerator::new("receipt", 1);
    let teardown = MockInstanceTeardownService::new();
    let resolver = FakePinnedResolverV1 {
        state: Arc::clone(&state),
        resolved: None,
    };
    let snapshot = FakeSnapshotProviderV1 {
        state: Arc::clone(&state),
        fail: false,
    };

    let outcome = execute_acquired_interaction_v1(
        admitted,
        envelope,
        permit,
        services_v1(
            &responder, &mutation, &instances, &ids, &teardown, &resolver, &snapshot,
        ),
    )
    .await;
    assert_terminalized_v1(
        outcome,
        AcquiredInteractionTerminalOutcomeV1::StaticCompleted,
    );
    assert_eq!(
        *state.trace.lock().unwrap(),
        [
            "permit.bind",
            "permit.execution_intent",
            "external.create_role",
            "permit.finish:interaction_static_completed",
        ]
    );
    assert!(state.intents.lock().unwrap().is_empty());
}

#[tokio::test]
async fn no_match_and_preparation_failure_are_known_pre_effect_failures() {
    let no_match_definition = InteractionRuleSet {
        version: 1,
        panels: Vec::new(),
        modals: Vec::new(),
        rules: Vec::new(),
    };
    let no_match_admitted = admitted_v1(
        artifact(no_match_definition),
        &encode_button(GUILD_ID, RULESET_KEY, "go"),
        None,
    )
    .await;
    let no_match_envelope = envelope_v1(encode_button(GUILD_ID, RULESET_KEY, "go"), 51_006);
    let no_match_root = claim_root_v1(&no_match_admitted, &no_match_envelope, None);
    let (no_match_permit, no_match_state) = permit_v1(no_match_root, PermitFailuresV1::default());
    let no_match_responder = FakeResponderV1::successful(Arc::clone(&no_match_state));
    let no_match_mutation = FakeMutationV1::successful(Arc::clone(&no_match_state));
    let instances = InMemoryInstanceStore::new();
    let ids = SequenceInstanceIdGenerator::new("receipt", 1);
    let teardown = MockInstanceTeardownService::new();
    let resolver = FakePinnedResolverV1 {
        state: Arc::clone(&no_match_state),
        resolved: None,
    };
    let snapshot = FakeSnapshotProviderV1 {
        state: Arc::clone(&no_match_state),
        fail: false,
    };
    let outcome = execute_acquired_interaction_v1(
        no_match_admitted,
        no_match_envelope,
        no_match_permit,
        services_v1(
            &no_match_responder,
            &no_match_mutation,
            &instances,
            &ids,
            &teardown,
            &resolver,
            &snapshot,
        ),
    )
    .await;
    assert_terminalized_v1(
        outcome,
        AcquiredInteractionTerminalOutcomeV1::NoMatchingRule,
    );
    assert_eq!(
        *no_match_state.trace.lock().unwrap(),
        ["permit.finish:interaction_no_matching_rule"]
    );

    let definition = modal_definition(vec![ActionSpec::RespondEphemeral {
        content: "ok".to_string(),
    }]);
    let custom_id = encode_modal(GUILD_ID, RULESET_KEY, "room");
    let admitted = admitted_v1(artifact(definition), &custom_id, None).await;
    let envelope = modal_envelope_v1(
        custom_id,
        51_007,
        vec![
            SharedGatewayModalInputV3::new(1, "unexpected".to_string(), "value".to_string())
                .unwrap(),
        ],
    );
    let root = claim_root_v1(&admitted, &envelope, None);
    let (permit, state) = permit_v1(root, PermitFailuresV1::default());
    let responder = FakeResponderV1::successful(Arc::clone(&state));
    let mutation = FakeMutationV1::successful(Arc::clone(&state));
    let instances = InMemoryInstanceStore::new();
    let ids = SequenceInstanceIdGenerator::new("receipt", 1);
    let teardown = MockInstanceTeardownService::new();
    let resolver = FakePinnedResolverV1 {
        state: Arc::clone(&state),
        resolved: None,
    };
    let snapshot = FakeSnapshotProviderV1 {
        state: Arc::clone(&state),
        fail: false,
    };
    let outcome = execute_acquired_interaction_v1(
        admitted,
        envelope,
        permit,
        services_v1(
            &responder, &mutation, &instances, &ids, &teardown, &resolver, &snapshot,
        ),
    )
    .await;
    assert_terminalized_v1(
        outcome,
        AcquiredInteractionTerminalOutcomeV1::StaticPreparationFailed,
    );
    assert_eq!(
        *state.trace.lock().unwrap(),
        ["permit.finish:interaction_static_preparation_failed"]
    );
}

#[tokio::test]
async fn bind_outage_has_zero_external_calls() {
    let definition = static_ruleset(vec![ActionSpec::RespondEphemeral {
        content: "never".to_string(),
    }]);
    let admitted = admitted_v1(
        artifact(definition),
        &encode_button(GUILD_ID, RULESET_KEY, "go"),
        None,
    )
    .await;
    let envelope = envelope_v1(encode_button(GUILD_ID, RULESET_KEY, "go"), 51_008);
    let root = claim_root_v1(&admitted, &envelope, None);
    let (permit, state) = permit_v1(
        root,
        PermitFailuresV1 {
            bind: true,
            ..PermitFailuresV1::default()
        },
    );
    let responder = FakeResponderV1::successful(Arc::clone(&state));
    let mutation = FakeMutationV1::successful(Arc::clone(&state));
    let instances = InMemoryInstanceStore::new();
    let ids = SequenceInstanceIdGenerator::new("receipt", 1);
    let teardown = MockInstanceTeardownService::new();
    let resolver = FakePinnedResolverV1 {
        state: Arc::clone(&state),
        resolved: None,
    };
    let snapshot = FakeSnapshotProviderV1 {
        state: Arc::clone(&state),
        fail: false,
    };

    let outcome = execute_acquired_interaction_v1(
        admitted,
        envelope,
        permit,
        services_v1(
            &responder, &mutation, &instances, &ids, &teardown, &resolver, &snapshot,
        ),
    )
    .await;
    assert_eq!(
        outcome,
        AcquiredInteractionExecutionOutcomeV1::PersistenceFailed {
            stage: AcquiredInteractionPersistenceStageV1::ActionPlanBind,
            external_effect_may_have_occurred: false,
        }
    );
    assert_eq!(*state.trace.lock().unwrap(), ["permit.bind"]);
}

#[tokio::test]
async fn fenced_persistence_outages_stop_before_the_external_call() {
    async fn run_case(
        definition: InteractionRuleSet,
        failures: PermitFailuresV1,
        interaction_id: u64,
    ) -> (AcquiredInteractionExecutionOutcomeV1, Arc<PermitStateV1>) {
        let admitted = admitted_v1(
            artifact(definition),
            &encode_button(GUILD_ID, RULESET_KEY, "go"),
            None,
        )
        .await;
        let envelope = envelope_v1(encode_button(GUILD_ID, RULESET_KEY, "go"), interaction_id);
        let root = claim_root_v1(&admitted, &envelope, None);
        let (permit, state) = permit_v1(root, failures);
        let responder = FakeResponderV1::successful(Arc::clone(&state));
        let mutation = FakeMutationV1::successful(Arc::clone(&state));
        let instances = InMemoryInstanceStore::new();
        let ids = SequenceInstanceIdGenerator::new("receipt", 1);
        let teardown = MockInstanceTeardownService::new();
        let resolver = FakePinnedResolverV1 {
            state: Arc::clone(&state),
            resolved: None,
        };
        let snapshot = FakeSnapshotProviderV1 {
            state: Arc::clone(&state),
            fail: false,
        };
        let outcome = execute_acquired_interaction_v1(
            admitted,
            envelope,
            permit,
            services_v1(
                &responder, &mutation, &instances, &ids, &teardown, &resolver, &snapshot,
            ),
        )
        .await;
        (outcome, state)
    }

    let (initial_outcome, initial_state) = run_case(
        static_ruleset(vec![ActionSpec::RespondEphemeral {
            content: "hello".to_string(),
        }]),
        PermitFailuresV1 {
            initial_intent: true,
            ..PermitFailuresV1::default()
        },
        51_014,
    )
    .await;
    assert_eq!(
        initial_outcome,
        AcquiredInteractionExecutionOutcomeV1::PersistenceFailed {
            stage: AcquiredInteractionPersistenceStageV1::InitialResponseIntent,
            external_effect_may_have_occurred: false,
        }
    );
    assert_eq!(
        *initial_state.trace.lock().unwrap(),
        ["permit.bind", "permit.intent:respond_ephemeral"]
    );

    let (execution_outcome, execution_state) = run_case(
        static_ruleset(vec![ActionSpec::CreateRole {
            key: "member".to_string(),
            name: "Member".to_string(),
        }]),
        PermitFailuresV1 {
            execution_intent: true,
            ..PermitFailuresV1::default()
        },
        51_015,
    )
    .await;
    assert_eq!(
        execution_outcome,
        AcquiredInteractionExecutionOutcomeV1::PersistenceFailed {
            stage: AcquiredInteractionPersistenceStageV1::ExecutionIntent,
            external_effect_may_have_occurred: false,
        }
    );
    assert_eq!(
        *execution_state.trace.lock().unwrap(),
        ["permit.bind", "permit.execution_intent"]
    );
}

#[tokio::test]
async fn stale_authoritative_permit_has_zero_external_calls() {
    let definition = static_ruleset(vec![ActionSpec::RespondEphemeral {
        content: "never".to_string(),
    }]);
    let admitted = admitted_v1(
        artifact(definition),
        &encode_button(GUILD_ID, RULESET_KEY, "go"),
        None,
    )
    .await;
    let envelope = envelope_v1(encode_button(GUILD_ID, RULESET_KEY, "go"), 51_009);
    let stale_envelope = envelope_v1(encode_button(GUILD_ID, RULESET_KEY, "go"), 61_009);
    let stale_root = claim_root_v1(&admitted, &stale_envelope, None);
    let (permit, state) = permit_v1(stale_root, PermitFailuresV1::default());
    let responder = FakeResponderV1::successful(Arc::clone(&state));
    let mutation = FakeMutationV1::successful(Arc::clone(&state));
    let instances = InMemoryInstanceStore::new();
    let ids = SequenceInstanceIdGenerator::new("receipt", 1);
    let teardown = MockInstanceTeardownService::new();
    let resolver = FakePinnedResolverV1 {
        state: Arc::clone(&state),
        resolved: None,
    };
    let snapshot = FakeSnapshotProviderV1 {
        state: Arc::clone(&state),
        fail: false,
    };

    let outcome = execute_acquired_interaction_v1(
        admitted,
        envelope,
        permit,
        services_v1(
            &responder, &mutation, &instances, &ids, &teardown, &resolver, &snapshot,
        ),
    )
    .await;
    assert_eq!(
        outcome,
        AcquiredInteractionExecutionOutcomeV1::AuthorityRejected
    );
    assert!(state.trace.lock().unwrap().is_empty());
}

#[tokio::test]
async fn terminal_persistence_failure_after_response_reports_possible_effect() {
    let definition = static_ruleset(vec![ActionSpec::RespondEphemeral {
        content: "sent".to_string(),
    }]);
    let admitted = admitted_v1(
        artifact(definition),
        &encode_button(GUILD_ID, RULESET_KEY, "go"),
        None,
    )
    .await;
    let envelope = envelope_v1(encode_button(GUILD_ID, RULESET_KEY, "go"), 51_010);
    let root = claim_root_v1(&admitted, &envelope, None);
    let (permit, state) = permit_v1(
        root,
        PermitFailuresV1 {
            finish: true,
            ..PermitFailuresV1::default()
        },
    );
    let responder = FakeResponderV1::successful(Arc::clone(&state));
    let mutation = FakeMutationV1::successful(Arc::clone(&state));
    let instances = InMemoryInstanceStore::new();
    let ids = SequenceInstanceIdGenerator::new("receipt", 1);
    let teardown = MockInstanceTeardownService::new();
    let resolver = FakePinnedResolverV1 {
        state: Arc::clone(&state),
        resolved: None,
    };
    let snapshot = FakeSnapshotProviderV1 {
        state: Arc::clone(&state),
        fail: false,
    };

    let outcome = execute_acquired_interaction_v1(
        admitted,
        envelope,
        permit,
        services_v1(
            &responder, &mutation, &instances, &ids, &teardown, &resolver, &snapshot,
        ),
    )
    .await;
    assert_eq!(
        outcome,
        AcquiredInteractionExecutionOutcomeV1::PersistenceFailed {
            stage: AcquiredInteractionPersistenceStageV1::TerminalFinish,
            external_effect_may_have_occurred: true,
        }
    );
    assert_eq!(
        state.trace.lock().unwrap().last().map(String::as_str),
        Some("permit.finish:interaction_static_completed")
    );
}

#[tokio::test]
async fn definitive_acknowledgement_failure_returns_the_committed_terminal_state() {
    let definition = static_ruleset(vec![ActionSpec::RespondEphemeral {
        content: "hello".to_string(),
    }]);
    let admitted = admitted_v1(
        artifact(definition),
        &encode_button(GUILD_ID, RULESET_KEY, "go"),
        None,
    )
    .await;
    let envelope = envelope_v1(encode_button(GUILD_ID, RULESET_KEY, "go"), 51_013);
    let root = claim_root_v1(&admitted, &envelope, None);
    let (permit, state) = permit_v1(root, PermitFailuresV1::default());
    let responder = FakeResponderV1 {
        state: Arc::clone(&state),
        initial_error: Some(AdapterError::new(
            AdapterErrorKind::BadRequest,
            "backend-ack-secret",
        )),
        edit_error: None,
    };
    let mutation = FakeMutationV1::successful(Arc::clone(&state));
    let instances = InMemoryInstanceStore::new();
    let ids = SequenceInstanceIdGenerator::new("receipt", 1);
    let teardown = MockInstanceTeardownService::new();
    let resolver = FakePinnedResolverV1 {
        state: Arc::clone(&state),
        resolved: None,
    };
    let snapshot = FakeSnapshotProviderV1 {
        state: Arc::clone(&state),
        fail: false,
    };

    let outcome = execute_acquired_interaction_v1(
        admitted,
        envelope,
        permit,
        services_v1(
            &responder, &mutation, &instances, &ids, &teardown, &resolver, &snapshot,
        ),
    )
    .await;
    assert_eq!(
        outcome,
        AcquiredInteractionExecutionOutcomeV1::AcknowledgementTerminalized {
            state: InteractionReceiptStateV1::Failed,
            result: InteractionInitialResponseResultKindV1::DefinitiveFailure,
        }
    );
    assert!(state.finishes.lock().unwrap().is_empty());
    assert_eq!(
        *state.trace.lock().unwrap(),
        [
            "permit.bind",
            "permit.intent:respond_ephemeral",
            "external.respond",
            "permit.result:definitive_failure",
        ]
    );
    assert!(!format!("{outcome:?}").contains("backend-ack-secret"));
}

#[tokio::test]
async fn canonical_plan_and_terminal_digests_are_deterministic_and_payload_sensitive() {
    async fn run_once(
        content: &str,
    ) -> (InteractionActionPlanDigestV1, InteractionTerminalDigestV1) {
        let definition = static_ruleset(vec![ActionSpec::RespondEphemeral {
            content: content.to_string(),
        }]);
        let admitted = admitted_v1(
            artifact(definition),
            &encode_button(GUILD_ID, RULESET_KEY, "go"),
            None,
        )
        .await;
        let envelope = envelope_v1(encode_button(GUILD_ID, RULESET_KEY, "go"), 51_011);
        let root = claim_root_v1(&admitted, &envelope, None);
        let (permit, state) = permit_v1(root, PermitFailuresV1::default());
        let responder = FakeResponderV1::successful(Arc::clone(&state));
        let mutation = FakeMutationV1::successful(Arc::clone(&state));
        let instances = InMemoryInstanceStore::new();
        let ids = SequenceInstanceIdGenerator::new("receipt", 1);
        let teardown = MockInstanceTeardownService::new();
        let resolver = FakePinnedResolverV1 {
            state: Arc::clone(&state),
            resolved: None,
        };
        let snapshot = FakeSnapshotProviderV1 {
            state: Arc::clone(&state),
            fail: false,
        };
        let outcome = execute_acquired_interaction_v1(
            admitted,
            envelope,
            permit,
            services_v1(
                &responder, &mutation, &instances, &ids, &teardown, &resolver, &snapshot,
            ),
        )
        .await;
        let finish = assert_terminalized_v1(
            outcome,
            AcquiredInteractionTerminalOutcomeV1::StaticCompleted,
        );
        let plan = state.binds.lock().unwrap()[0].clone();
        (plan, finish.terminal_digest().clone())
    }

    let first = run_once("same").await;
    let repeated = run_once("same").await;
    let changed = run_once("changed").await;
    assert_eq!(first, repeated);
    assert_ne!(first.0, changed.0);
    assert_ne!(first.1, changed.1);
    assert_eq!(first.1.as_bytes().len(), 32);
}

#[tokio::test]
async fn mutation_failure_after_execution_intent_terminalizes_recovery_required() {
    let definition = static_ruleset(vec![ActionSpec::CreateRole {
        key: "member".to_string(),
        name: "Member".to_string(),
    }]);
    let admitted = admitted_v1(
        artifact(definition),
        &encode_button(GUILD_ID, RULESET_KEY, "go"),
        None,
    )
    .await;
    let envelope = envelope_v1(encode_button(GUILD_ID, RULESET_KEY, "go"), 51_012);
    let root = claim_root_v1(&admitted, &envelope, None);
    let (permit, state) = permit_v1(root, PermitFailuresV1::default());
    let responder = FakeResponderV1::successful(Arc::clone(&state));
    let mutation = FakeMutationV1 {
        state: Arc::clone(&state),
        error: Some(AdapterError::new(
            AdapterErrorKind::BadRequest,
            "backend-mutation-secret",
        )),
    };
    let instances = InMemoryInstanceStore::new();
    let ids = SequenceInstanceIdGenerator::new("receipt", 1);
    let teardown = MockInstanceTeardownService::new();
    let resolver = FakePinnedResolverV1 {
        state: Arc::clone(&state),
        resolved: None,
    };
    let snapshot = FakeSnapshotProviderV1 {
        state: Arc::clone(&state),
        fail: false,
    };

    let outcome = execute_acquired_interaction_v1(
        admitted,
        envelope,
        permit,
        services_v1(
            &responder, &mutation, &instances, &ids, &teardown, &resolver, &snapshot,
        ),
    )
    .await;
    assert_terminalized_v1(
        outcome,
        AcquiredInteractionTerminalOutcomeV1::ExecutionRecoveryRequired,
    );
    let rendered = format!("{:?}", state.finishes.lock().unwrap()[0]);
    assert!(!rendered.contains("backend-mutation-secret"));
}
