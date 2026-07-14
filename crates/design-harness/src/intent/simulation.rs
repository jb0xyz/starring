use std::collections::BTreeMap;

use automation_core::{
    interpret, run, ActionPlan, AutomationServices, CreatedResource, EventKind,
    MockInstanceTeardownService, MockInteractionResponder, MockMutationAdapter, MutationCall,
    PostPanelButtonSpec, ResolvedButtonRoute, ResolvedInstanceContext, ResponderCall,
    RunningRuleSetIdentity, RuntimeContext, RuntimeEvent, SanitizeContext, TemplateString,
};
use automation_instance::{
    AutomationInstance, InMemoryInstanceStore, InstanceId, InstanceRuleSetVersion, InstanceStatus,
    InstanceStore, SequenceInstanceIdGenerator,
};
use automation_state::{ActionSpec, ButtonRoute, InteractionRule, TriggerSpec};
use resource_resolution::ResourceBindingMap;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::json;

use crate::draft::Draft;
use crate::errors::{translate_run_error, StructuredError};

use super::compile::{compile_intent, CompiledIntentV1};
use super::model::{
    ResolvedCloseControlV1, ResolvedFeatureConfigurationV1, ResolvedManagedPrivateRoomV1,
    RoomNamePatternV1, PRIVATE_STUDY_ROOM_RECIPE_ID, PRIVATE_STUDY_ROOM_RECIPE_VERSION,
};
use super::normalize::ValidatedIntentV1;

const GUILD_ID: &str = "1";
const CREATOR_ID: &str = "42";
const JOINER_ID: &str = "77";
const CLOSER_ID: &str = "88";
const ROOM_NAME_INPUT: &str = "room_name";
const ROOM_NAME_VALUE: &str = "algebra";
const INSTANCE_ID: &str = "study_001";
const MEMBER_ROLE_ALIAS: &str = "member_role";
const ROOM_CHANNEL_ALIAS: &str = "room_channel";
const WELCOME_PANEL_ALIAS: &str = "welcome_panel";
const HUB_PANEL_ALIAS: &str = "hub_panel";
const VIEW_CHANNEL_BIT: u64 = 1 << 10;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct IntentSimulationReportV1 {
    pub(crate) traces_run: u32,
    pub(crate) close_executed: bool,
}

struct RecipeKeys {
    create_button: String,
    modal: String,
    open_rule: String,
    submit_rule: String,
    member_role: String,
    room_channel: String,
    welcome_panel: String,
    hub_panel: String,
    instance: String,
    help_button: String,
    help_rule: String,
    join_action: String,
    join_rule: String,
    close: Option<CloseKeys>,
}

struct CloseKeys {
    action: String,
    rule: String,
}

struct SimulationRuntime<'a> {
    candidate: &'a Draft,
    bindings: &'a ResourceBindingMap,
    identity: &'a RunningRuleSetIdentity,
    mutation: &'a MockMutationAdapter,
    instances: &'a InMemoryInstanceStore,
    instance_ids: &'a SequenceInstanceIdGenerator,
    teardown: &'a MockInstanceTeardownService,
}

pub(crate) async fn simulate_compiled_intent(
    candidate: &mut Draft,
    intent: &ValidatedIntentV1,
    compiled: &CompiledIntentV1,
    bindings: &ResourceBindingMap,
) -> Result<IntentSimulationReportV1, StructuredError> {
    candidate.simulated_revision = None;
    let result = simulate_private_study_room(candidate, intent, compiled, bindings).await;
    if result.is_ok() {
        candidate.simulated_revision = Some(candidate.draft_revision);
    }
    result
}

async fn simulate_private_study_room(
    candidate: &Draft,
    intent: &ValidatedIntentV1,
    compiled: &CompiledIntentV1,
    bindings: &ResourceBindingMap,
) -> Result<IntentSimulationReportV1, StructuredError> {
    if candidate.validated_revision != Some(candidate.draft_revision) {
        return Err(simulation_error(
            "INTENT_SIMULATION_DRAFT_NOT_VALIDATED",
            "intent.simulation.validation",
            "The compiled candidate has not passed validation at its current revision",
            "Validate the candidate with the same resource bindings before recipe simulation",
        ));
    }
    let expected_compilation = compile_intent(intent)?;
    if &expected_compilation != compiled {
        return Err(simulation_error(
            "INTENT_SIMULATION_COMPILATION_MISMATCH",
            "intent.simulation.compilation",
            "The compiled plan does not match the normalized intent",
            "Compile the current validated intent immediately before simulation",
        ));
    }
    if compiled.manifest.recipe_id != PRIVATE_STUDY_ROOM_RECIPE_ID
        || compiled.manifest.recipe_version != PRIVATE_STUDY_ROOM_RECIPE_VERSION
    {
        return Err(simulation_error(
            "INTENT_SIMULATION_RECIPE_UNSUPPORTED",
            "intent.simulation.recipe",
            "The compiled recipe is not supported by the deterministic simulator",
            "Register an exact simulator for the compiled recipe id and version",
        ));
    }
    let resolved = intent.resolved();
    let [feature] = resolved.features.as_slice() else {
        return Err(simulation_error(
            "INTENT_SIMULATION_FEATURE_CARDINALITY_INVALID",
            "intent.simulation.feature",
            "Recipe simulation requires exactly one normalized feature",
            "Resolve one supported feature before compiling and simulating",
        ));
    };
    if feature.feature_id.as_str() != compiled.manifest.feature_id
        || feature.recipe.id != compiled.manifest.recipe_id
        || feature.recipe.version != compiled.manifest.recipe_version
    {
        return Err(simulation_error(
            "INTENT_SIMULATION_MANIFEST_MISMATCH",
            "intent.simulation.manifest",
            "The compilation manifest does not identify the normalized feature",
            "Discard the stale compiled artifact and compile the current intent",
        ));
    }
    let ResolvedFeatureConfigurationV1::ManagedPrivateRoom(room) = &feature.configuration;
    let expected_bindings = [room.hub_channel.value.as_str().to_string()];
    if compiled.manifest.external_channel_bindings.as_slice() != expected_bindings.as_slice() {
        return Err(simulation_error(
            "INTENT_SIMULATION_BINDING_MANIFEST_MISMATCH",
            "intent.simulation.bindings",
            "The compiled external channel bindings do not match the normalized intent",
            "Compile the current hub channel into the recipe manifest",
        ));
    }
    ensure_hub_binding(room, bindings)?;
    let keys = recipe_keys(compiled, room)?;
    let identity = RunningRuleSetIdentity {
        key: format!("intent-simulation:{}", compiled.manifest.feature_id),
        version: InstanceRuleSetVersion::new(1).map_err(|_| identity_error())?,
    };
    let instance_id = InstanceId::parse(INSTANCE_ID).map_err(|_| identity_error())?;
    let mutation = MockMutationAdapter::new();
    let instances = InMemoryInstanceStore::new();
    let instance_ids = SequenceInstanceIdGenerator::new("study", 1);
    let teardown = MockInstanceTeardownService::new();
    let runtime = SimulationRuntime {
        candidate,
        bindings,
        identity: &identity,
        mutation: &mutation,
        instances: &instances,
        instance_ids: &instance_ids,
        teardown: &teardown,
    };

    run_open_trace(&runtime, &keys).await?;
    let instance = run_submit_trace(&runtime, room, &keys, &instance_id).await?;
    run_help_trace(&runtime, room, &keys).await?;
    run_join_trace(&runtime, room, &keys, &instance).await?;
    let close_executed = match (&room.controls.close, &keys.close) {
        (ResolvedCloseControlV1::Disabled { .. }, None) => {
            prove_close_disabled(candidate, compiled)?;
            false
        }
        (ResolvedCloseControlV1::AnyMember { response, .. }, Some(close)) => {
            run_close_trace(&runtime, response.value.as_str(), close, &instance).await?;
            true
        }
        _ => {
            return Err(simulation_error(
                "INTENT_SIMULATION_CLOSE_POLICY_MISMATCH",
                "intent.simulation.close",
                "The compiled close controls do not match the normalized close policy",
                "Recompile the current normalized intent before simulation",
            ));
        }
    };
    Ok(IntentSimulationReportV1 {
        traces_run: if close_executed { 5 } else { 4 },
        close_executed,
    })
}

fn recipe_keys(
    compiled: &CompiledIntentV1,
    room: &ResolvedManagedPrivateRoomV1,
) -> Result<RecipeKeys, StructuredError> {
    let generated = &compiled.manifest.generated_objects;
    let close = match &room.controls.close {
        ResolvedCloseControlV1::Disabled { .. } => {
            if generated.contains_key("close") || generated.contains_key("close_room") {
                return Err(manifest_object_error("close"));
            }
            None
        }
        ResolvedCloseControlV1::AnyMember { .. } => Some(CloseKeys {
            action: generated_object(generated, "close")?,
            rule: generated_object(generated, "close_room")?,
        }),
    };
    Ok(RecipeKeys {
        create_button: generated_object(generated, "create_study_room")?,
        modal: generated_object(generated, "study_modal")?,
        open_rule: generated_object(generated, "open_modal")?,
        submit_rule: generated_object(generated, "submit_room")?,
        member_role: generated_object(generated, "member_role")?,
        room_channel: generated_object(generated, "room_channel")?,
        welcome_panel: generated_object(generated, "welcome_panel")?,
        hub_panel: generated_object(generated, "hub_panel")?,
        instance: generated_object(generated, "study_instance")?,
        help_button: generated_object(generated, "study_help")?,
        help_rule: generated_object(generated, "show_help")?,
        join_action: generated_object(generated, "join")?,
        join_rule: generated_object(generated, "join_room")?,
        close,
    })
}

fn generated_object(
    generated: &BTreeMap<String, String>,
    symbol: &str,
) -> Result<String, StructuredError> {
    generated
        .get(symbol)
        .cloned()
        .ok_or_else(|| manifest_object_error(symbol))
}

fn manifest_object_error(symbol: &str) -> StructuredError {
    simulation_error(
        "INTENT_SIMULATION_MANIFEST_OBJECT_MISSING",
        format!("intent.simulation.manifest.generated_objects.{symbol}"),
        format!("The recipe manifest does not contain generated object {symbol}"),
        "Compile a complete deterministic recipe manifest before simulation",
    )
}

async fn run_open_trace(
    runtime: &SimulationRuntime<'_>,
    keys: &RecipeKeys,
) -> Result<(), StructuredError> {
    let responder = MockInteractionResponder::new();
    let services = AutomationServices {
        mutation: runtime.mutation,
        responder: &responder,
        instances: runtime.instances,
        instance_ids: runtime.instance_ids,
        teardown: runtime.teardown,
    };
    let event = RuntimeEvent {
        guild_id: GUILD_ID.parse().map_err(|_| identity_error())?,
        actor: CREATOR_ID.parse().map_err(|_| identity_error())?,
        kind: EventKind::ButtonClick {
            component: keys.create_button.clone(),
        },
    };
    let plan = exact_plan(
        runtime.candidate,
        runtime.bindings,
        &keys.open_rule,
        &event,
        "open",
    )?;
    let context = RuntimeContext::from_event(&event, runtime.identity);
    run(&context, &plan, &services)
        .await
        .map_err(|error| runtime_error("open", &error))?;
    expect_responses(
        "open",
        &responder.calls(),
        &[ResponderCall::OpenModal {
            modal: keys.modal.clone(),
        }],
    )?;
    if !runtime.mutation.calls().is_empty() {
        return Err(trace_error(
            "open",
            "Opening the room modal performed a Discord mutation",
            "Keep the open rule limited to the compiled OpenModal action",
        ));
    }
    Ok(())
}

async fn run_submit_trace(
    runtime: &SimulationRuntime<'_>,
    room: &ResolvedManagedPrivateRoomV1,
    keys: &RecipeKeys,
    instance_id: &InstanceId,
) -> Result<AutomationInstance, StructuredError> {
    let responder = MockInteractionResponder::new();
    let services = AutomationServices {
        mutation: runtime.mutation,
        responder: &responder,
        instances: runtime.instances,
        instance_ids: runtime.instance_ids,
        teardown: runtime.teardown,
    };
    let inputs = BTreeMap::from([(ROOM_NAME_INPUT.to_string(), ROOM_NAME_VALUE.to_string())]);
    let event = RuntimeEvent {
        guild_id: GUILD_ID.parse().map_err(|_| identity_error())?,
        actor: CREATOR_ID.parse().map_err(|_| identity_error())?,
        kind: EventKind::ModalSubmit {
            modal: keys.modal.clone(),
            inputs: inputs.clone(),
        },
    };
    let plan = exact_plan(
        runtime.candidate,
        runtime.bindings,
        &keys.submit_rule,
        &event,
        "submit",
    )?;
    let context = RuntimeContext::from_event(&event, runtime.identity);
    let result = run(&context, &plan, &services)
        .await
        .map_err(|error| runtime_error("submit", &error))?;
    let expected_response = render_pattern(
        &room.copy.completed_response.value,
        &inputs,
        SanitizeContext::EphemeralMessageContent,
        "submit.response",
    )?;
    expect_responses(
        "submit",
        &responder.calls(),
        &[
            ResponderCall::DeferEphemeral,
            ResponderCall::EditResponse {
                content: expected_response,
            },
        ],
    )?;
    assert_submit_mutations(
        &runtime.mutation.calls(),
        room,
        keys,
        runtime.bindings,
        instance_id,
        &inputs,
    )?;
    assert_created_resources(&result.created, keys, instance_id)?;
    load_and_verify_instance(
        runtime.instances,
        runtime.identity,
        room,
        runtime.bindings,
        instance_id,
        &result.created,
    )
    .await
}

async fn run_help_trace(
    runtime: &SimulationRuntime<'_>,
    room: &ResolvedManagedPrivateRoomV1,
    keys: &RecipeKeys,
) -> Result<(), StructuredError> {
    let before_mutations = runtime.mutation.calls().len();
    let responder = MockInteractionResponder::new();
    let services = AutomationServices {
        mutation: runtime.mutation,
        responder: &responder,
        instances: runtime.instances,
        instance_ids: runtime.instance_ids,
        teardown: runtime.teardown,
    };
    let event = RuntimeEvent {
        guild_id: GUILD_ID.parse().map_err(|_| identity_error())?,
        actor: CREATOR_ID.parse().map_err(|_| identity_error())?,
        kind: EventKind::ButtonClick {
            component: keys.help_button.clone(),
        },
    };
    let plan = exact_plan(
        runtime.candidate,
        runtime.bindings,
        &keys.help_rule,
        &event,
        "help",
    )?;
    let context = RuntimeContext::from_event(&event, runtime.identity);
    run(&context, &plan, &services)
        .await
        .map_err(|error| runtime_error("help", &error))?;
    let expected = render_literal(
        room.controls.help.response.value.as_str(),
        SanitizeContext::EphemeralMessageContent,
        "help.response",
    )?;
    expect_responses(
        "help",
        &responder.calls(),
        &[ResponderCall::RespondEphemeral { content: expected }],
    )?;
    if runtime.mutation.calls().len() != before_mutations {
        return Err(trace_error(
            "help",
            "The help control performed a Discord mutation",
            "Keep the help rule limited to the exact ephemeral response",
        ));
    }
    Ok(())
}

async fn run_join_trace(
    runtime: &SimulationRuntime<'_>,
    room: &ResolvedManagedPrivateRoomV1,
    keys: &RecipeKeys,
    instance: &AutomationInstance,
) -> Result<(), StructuredError> {
    let before_mutations = runtime.mutation.calls();
    let responder = MockInteractionResponder::new();
    let services = AutomationServices {
        mutation: runtime.mutation,
        responder: &responder,
        instances: runtime.instances,
        instance_ids: runtime.instance_ids,
        teardown: runtime.teardown,
    };
    let event = RuntimeEvent {
        guild_id: GUILD_ID.parse().map_err(|_| identity_error())?,
        actor: JOINER_ID.parse().map_err(|_| identity_error())?,
        kind: EventKind::InstanceAction {
            instance_id: instance.id.clone(),
            action: keys.join_action.clone(),
        },
    };
    let plan = exact_plan(
        runtime.candidate,
        runtime.bindings,
        &keys.join_rule,
        &event,
        "join",
    )?;
    let mut context = RuntimeContext::from_event(&event, runtime.identity);
    context.instance = Some(ResolvedInstanceContext {
        instance: instance.clone(),
        action: keys.join_action.clone(),
    });
    run(&context, &plan, &services)
        .await
        .map_err(|error| runtime_error("join", &error))?;
    let expected = render_literal(
        room.controls.join.response.value.as_str(),
        SanitizeContext::EphemeralMessageContent,
        "join.response",
    )?;
    expect_responses(
        "join",
        &responder.calls(),
        &[
            ResponderCall::DeferEphemeral,
            ResponderCall::EditResponse { content: expected },
        ],
    )?;
    let after_mutations = runtime.mutation.calls();
    let added = after_mutations
        .get(before_mutations.len()..)
        .unwrap_or_default();
    let expected_role = instance
        .resources
        .roles
        .get(MEMBER_ROLE_ALIAS)
        .ok_or_else(instance_manifest_error)?;
    match added {
        [MutationCall::GrantRole { member, role, .. }]
            if member.to_string() == JOINER_ID && role == expected_role => {}
        _ => {
            return Err(trace_error(
                "join",
                "The join control did not grant the event instance member role exactly once to actor 77",
                "Resolve member_role from InstanceRef::Event and grant it to the triggering actor",
            ));
        }
    }
    Ok(())
}

async fn run_close_trace(
    runtime: &SimulationRuntime<'_>,
    response: &str,
    close: &CloseKeys,
    instance: &AutomationInstance,
) -> Result<(), StructuredError> {
    let before_mutations = runtime.mutation.calls().len();
    let before_teardowns = runtime.teardown.calls().len();
    let responder = MockInteractionResponder::new();
    let services = AutomationServices {
        mutation: runtime.mutation,
        responder: &responder,
        instances: runtime.instances,
        instance_ids: runtime.instance_ids,
        teardown: runtime.teardown,
    };
    let event = RuntimeEvent {
        guild_id: GUILD_ID.parse().map_err(|_| identity_error())?,
        actor: CLOSER_ID.parse().map_err(|_| identity_error())?,
        kind: EventKind::InstanceAction {
            instance_id: instance.id.clone(),
            action: close.action.clone(),
        },
    };
    let plan = exact_plan(
        runtime.candidate,
        runtime.bindings,
        &close.rule,
        &event,
        "close",
    )?;
    let mut context = RuntimeContext::from_event(&event, runtime.identity);
    context.instance = Some(ResolvedInstanceContext {
        instance: instance.clone(),
        action: close.action.clone(),
    });
    let result = run(&context, &plan, &services)
        .await
        .map_err(|error| runtime_error("close", &error))?;
    let expected = render_literal(
        response,
        SanitizeContext::EphemeralMessageContent,
        "close.response",
    )?;
    expect_responses(
        "close",
        &responder.calls(),
        &[
            ResponderCall::DeferEphemeral,
            ResponderCall::EditResponse { content: expected },
        ],
    )?;
    let teardown_calls = runtime.teardown.calls();
    let added_teardowns = teardown_calls.get(before_teardowns..).unwrap_or_default();
    if added_teardowns.len() != 1 || added_teardowns[0].1 != instance.id {
        return Err(trace_error(
            "close",
            "The close control did not teardown the event instance exactly once",
            "Use one TeardownInstance action with InstanceRef::Event",
        ));
    }
    if result.teardowns.len() != 1 || result.teardowns[0].instance_id != instance.id {
        return Err(trace_error(
            "close",
            "The close trace did not report exactly one completed teardown",
            "Keep one teardown action before the exact close response",
        ));
    }
    if runtime.mutation.calls().len() != before_mutations {
        return Err(trace_error(
            "close",
            "The close control performed an unexpected Discord mutation",
            "Keep close limited to deferred response, teardown, and final response",
        ));
    }
    Ok(())
}

fn exact_plan(
    candidate: &Draft,
    bindings: &ResourceBindingMap,
    rule_key: &str,
    event: &RuntimeEvent,
    trace: &str,
) -> Result<ActionPlan, StructuredError> {
    let mut matches = candidate
        .ruleset
        .rules
        .iter()
        .filter(|rule| rule.key == rule_key);
    let rule = matches.next().ok_or_else(|| {
        simulation_error(
            "INTENT_SIMULATION_RULE_MISSING",
            format!("intent.simulation.{trace}.rule"),
            format!("Compiled rule {rule_key} is missing from the candidate"),
            "Apply the complete compiled recipe before simulation",
        )
    })?;
    if matches.next().is_some() {
        return Err(simulation_error(
            "INTENT_SIMULATION_RULE_DUPLICATED",
            format!("intent.simulation.{trace}.rule"),
            format!("Compiled rule {rule_key} appears more than once"),
            "Keep exactly one rule for each compiled recipe object",
        ));
    }
    let mut exact_ruleset = candidate.ruleset.clone();
    exact_ruleset.rules = vec![InteractionRule {
        key: rule.key.clone(),
        trigger: rule.trigger.clone(),
        actions: rule.actions.clone(),
    }];
    interpret(event, &exact_ruleset, bindings).ok_or_else(|| {
        simulation_error(
            "INTENT_SIMULATION_TRIGGER_MISMATCH",
            format!("intent.simulation.{trace}.trigger"),
            format!("Compiled rule {rule_key} does not match its deterministic recipe event"),
            "Restore the trigger emitted by the deterministic recipe compiler",
        )
    })
}

fn assert_submit_mutations(
    calls: &[MutationCall],
    room: &ResolvedManagedPrivateRoomV1,
    keys: &RecipeKeys,
    bindings: &ResourceBindingMap,
    instance_id: &InstanceId,
    inputs: &BTreeMap<String, String>,
) -> Result<(), StructuredError> {
    let [MutationCall::CreateRole {
        name: role_name, ..
    }, MutationCall::CreateChannel {
        name: channel_name, ..
    }, MutationCall::UpsertOverwrite {
        channel: denied_channel,
        target: everyone_target,
        allow: everyone_allow,
        deny: everyone_deny,
        ..
    }, MutationCall::UpsertOverwrite {
        channel: allowed_channel,
        target: member_target,
        allow: member_allow,
        deny: member_deny,
        ..
    }, MutationCall::GrantRole {
        member: creator,
        role: granted_role,
        ..
    }, MutationCall::PostPanel {
        channel: welcome_channel,
        content: welcome_content,
        buttons: welcome_buttons,
        ..
    }, MutationCall::PostPanel {
        channel: hub_channel,
        content: hub_content,
        buttons: hub_buttons,
        ..
    }] = calls
    else {
        return Err(trace_error(
            "submit",
            "The submit trace did not perform the exact seven recipe mutations in order",
            "Restore role, channel, privacy, creator grant, welcome panel, and hub panel actions",
        ));
    };
    let expected_role_name = render_pattern(
        &room.naming.member_role_name.value,
        inputs,
        SanitizeContext::RoleName,
        "submit.member_role",
    )?;
    let expected_channel_name = render_pattern(
        &room.naming.channel_name.value,
        inputs,
        SanitizeContext::ChannelName,
        "submit.room_channel",
    )?;
    if role_name != &expected_role_name || channel_name != &expected_channel_name {
        return Err(trace_error(
            "submit",
            "Created role or channel names differ from the normalized naming intent",
            "Compile both names directly from their normalized room-name patterns",
        ));
    }
    if denied_channel != allowed_channel
        || !everyone_allow.is_empty()
        || everyone_deny.bits() != VIEW_CHANNEL_BIT
        || member_allow.bits() != VIEW_CHANNEL_BIT
        || !member_deny.is_empty()
    {
        return Err(trace_error(
            "submit",
            "The created room does not have the exact private visibility overwrites",
            "Deny view_channel for everyone and allow it for the created member role",
        ));
    }
    let everyone_json = serde_json::to_value(everyone_target).ok();
    if everyone_json != Some(json!({"type":"role","id":GUILD_ID})) {
        return Err(trace_error(
            "submit",
            "The private deny overwrite does not target the guild everyone role",
            "Use the deterministic everyone overwrite target",
        ));
    }
    let granted_role_id = granted_role.to_string();
    let member_target_json = serde_json::to_value(member_target).ok();
    if member_target_json != Some(json!({"type":"role","id":granted_role_id}))
        || creator.to_string() != CREATOR_ID
    {
        return Err(trace_error(
            "submit",
            "The creator grant or member overwrite did not use the same created member role",
            "Use one created member role for privacy and grant it to the submitting actor",
        ));
    }
    if welcome_channel != denied_channel {
        return Err(trace_error(
            "submit",
            "The welcome panel was not posted in the created private channel",
            "Post the welcome panel to the compiled room_channel reference",
        ));
    }
    let bound_hub = bound_hub_channel(room, bindings)?;
    if hub_channel.to_string() != bound_hub {
        return Err(trace_error(
            "submit",
            "The discovery panel was not posted to the normalized hub binding",
            "Resolve the hub channel from the compilation manifest binding",
        ));
    }
    let expected_welcome = render_pattern(
        &room.copy.welcome_content.value,
        inputs,
        SanitizeContext::EphemeralMessageContent,
        "submit.welcome_panel",
    )?;
    let expected_hub = render_pattern(
        &room.copy.hub_announcement.value,
        inputs,
        SanitizeContext::EphemeralMessageContent,
        "submit.hub_panel",
    )?;
    if welcome_content != &expected_welcome || hub_content != &expected_hub {
        return Err(trace_error(
            "submit",
            "Posted panel content differs from the normalized copy intent",
            "Compile welcome and discovery copy directly from normalized patterns",
        ));
    }
    let mut expected_welcome_buttons = vec![PostPanelButtonSpec {
        label: room.controls.help.label.value.clone(),
        route: ResolvedButtonRoute::Static {
            key: keys.help_button.clone(),
        },
    }];
    if let (ResolvedCloseControlV1::AnyMember { label, .. }, Some(close)) =
        (&room.controls.close, &keys.close)
    {
        expected_welcome_buttons.push(PostPanelButtonSpec {
            label: label.value.clone(),
            route: ResolvedButtonRoute::InstanceAction {
                instance_id: instance_id.clone(),
                action: close.action.clone(),
            },
        });
    }
    let expected_hub_buttons = vec![PostPanelButtonSpec {
        label: room.controls.join.label.value.clone(),
        route: ResolvedButtonRoute::InstanceAction {
            instance_id: instance_id.clone(),
            action: keys.join_action.clone(),
        },
    }];
    if welcome_buttons != &expected_welcome_buttons || hub_buttons != &expected_hub_buttons {
        return Err(trace_error(
            "submit",
            "Posted panel controls differ from the compiled recipe routes",
            "Restore exact help, join, and optional close routes from the manifest",
        ));
    }
    Ok(())
}

fn assert_created_resources(
    created: &[CreatedResource],
    keys: &RecipeKeys,
    instance_id: &InstanceId,
) -> Result<(), StructuredError> {
    let [CreatedResource::Role { key: role, .. }, CreatedResource::Channel { key: channel, .. }, CreatedResource::Message { key: welcome, .. }, CreatedResource::Message { key: hub, .. }, CreatedResource::Instance {
        key: instance, id, ..
    }] = created
    else {
        return Err(trace_error(
            "submit",
            "The submit trace did not create the exact recipe resource set",
            "Create and register one role, channel, two panels, and one instance",
        ));
    };
    if role != &keys.member_role
        || channel != &keys.room_channel
        || welcome != &keys.welcome_panel
        || hub != &keys.hub_panel
        || instance != &keys.instance
        || id != instance_id
    {
        return Err(trace_error(
            "submit",
            "Created resources do not match the compilation manifest ownership keys",
            "Use only the generated object keys recorded by the recipe compiler",
        ));
    }
    Ok(())
}

async fn load_and_verify_instance(
    instances: &InMemoryInstanceStore,
    identity: &RunningRuleSetIdentity,
    room: &ResolvedManagedPrivateRoomV1,
    bindings: &ResourceBindingMap,
    instance_id: &InstanceId,
    created: &[CreatedResource],
) -> Result<AutomationInstance, StructuredError> {
    let guild_id = GUILD_ID.parse().map_err(|_| identity_error())?;
    let instance = instances
        .get(guild_id, instance_id)
        .await
        .map_err(|_| instance_manifest_error())?
        .ok_or_else(instance_manifest_error)?;
    let [CreatedResource::Role {
        id: created_role, ..
    }, CreatedResource::Channel {
        id: created_channel,
        ..
    }, CreatedResource::Message {
        channel: welcome_channel,
        id: welcome_message,
        ..
    }, CreatedResource::Message {
        channel: hub_channel,
        id: hub_message,
        ..
    }, CreatedResource::Instance { .. }] = created
    else {
        return Err(instance_manifest_error());
    };
    let hub = bound_hub_channel(room, bindings)?;
    let valid_resources = instance.resources.roles.len() == 1
        && instance.resources.channels.len() == 1
        && instance.resources.messages.len() == 2
        && instance.resources.roles.get(MEMBER_ROLE_ALIAS) == Some(created_role)
        && instance.resources.channels.get(ROOM_CHANNEL_ALIAS) == Some(created_channel)
        && instance
            .resources
            .messages
            .get(WELCOME_PANEL_ALIAS)
            .is_some_and(|message| {
                message.channel == *welcome_channel
                    && message.channel == *created_channel
                    && message.id == *welcome_message
            })
        && instance
            .resources
            .messages
            .get(HUB_PANEL_ALIAS)
            .is_some_and(|message| {
                message.channel == *hub_channel
                    && message.channel.to_string() == hub
                    && message.id == *hub_message
            });
    if instance.id != *instance_id
        || instance.ruleset_key != identity.key
        || instance.ruleset_version != identity.version
        || instance.kind.0 != "study_room"
        || instance.created_by.to_string() != CREATOR_ID
        || instance.status != InstanceStatus::Active
        || !valid_resources
    {
        return Err(instance_manifest_error());
    }
    Ok(instance)
}

fn prove_close_disabled(
    candidate: &Draft,
    compiled: &CompiledIntentV1,
) -> Result<(), StructuredError> {
    let close_action = format!("{}__close", compiled.manifest.feature_id);
    let close_rule = format!("{}__close_room", compiled.manifest.feature_id);
    let top_level_route = candidate.ruleset.panels.iter().any(|panel| {
        panel.buttons.iter().any(|button| {
            matches!(
                &button.route,
                ButtonRoute::InstanceAction { action, .. } if action == &close_action
            )
        })
    });
    let dynamic_route = candidate.ruleset.rules.iter().any(|rule| {
        rule.actions.iter().any(|action| match action {
            ActionSpec::PostPanel { buttons, .. } => buttons.iter().any(|button| {
                matches!(
                    &button.route,
                    ButtonRoute::InstanceAction { action, .. } if action == &close_action
                )
            }),
            _ => false,
        })
    });
    let handler = candidate.ruleset.rules.iter().any(|rule| {
        rule.key == close_rule
            || matches!(
                &rule.trigger,
                TriggerSpec::InstanceAction { action } if action == &close_action
            )
    });
    if top_level_route || dynamic_route || handler {
        return Err(trace_error(
            "close",
            "Disabled close policy still renders or handles the generated close action",
            "Remove both the close route and its instance-action handler",
        ));
    }
    Ok(())
}

fn expect_responses(
    trace: &str,
    actual: &[ResponderCall],
    expected: &[ResponderCall],
) -> Result<(), StructuredError> {
    if actual != expected {
        return Err(trace_error(
            trace,
            "The interaction response sequence differs from the normalized recipe",
            "Restore the exact response lifecycle and normalized response copy",
        ));
    }
    Ok(())
}

fn ensure_hub_binding(
    room: &ResolvedManagedPrivateRoomV1,
    bindings: &ResourceBindingMap,
) -> Result<(), StructuredError> {
    bound_hub_channel(room, bindings).map(|_| ())
}

fn bound_hub_channel(
    room: &ResolvedManagedPrivateRoomV1,
    bindings: &ResourceBindingMap,
) -> Result<String, StructuredError> {
    bindings
        .channel_bindings
        .iter()
        .find_map(|(key, channel)| {
            (key.0 == room.hub_channel.value.as_str()).then(|| channel.to_string())
        })
        .ok_or_else(|| {
            simulation_error(
                "INTENT_SIMULATION_BINDING_MISSING",
                "intent.simulation.bindings.hub_channel",
                format!(
                    "Hub channel binding {} is missing",
                    room.hub_channel.value.as_str()
                ),
                "Provide the exact external channel binding recorded by the normalized intent",
            )
        })
}

fn render_pattern(
    pattern: &RoomNamePatternV1,
    inputs: &BTreeMap<String, String>,
    context: SanitizeContext,
    location: &str,
) -> Result<String, StructuredError> {
    render_source(
        format!(
            "{}${{input.{ROOM_NAME_INPUT}}}{}",
            pattern.prefix, pattern.suffix
        )
        .as_str(),
        inputs,
        context,
        location,
    )
}

fn render_literal(
    source: &str,
    context: SanitizeContext,
    location: &str,
) -> Result<String, StructuredError> {
    render_source(source, &BTreeMap::new(), context, location)
}

fn render_source(
    source: &str,
    inputs: &BTreeMap<String, String>,
    context: SanitizeContext,
    location: &str,
) -> Result<String, StructuredError> {
    TemplateString::parse(source)
        .and_then(|template| template.render(inputs, context))
        .map_err(|_| {
            simulation_error(
                "INTENT_SIMULATION_ORACLE_RENDER_FAILED",
                format!("intent.simulation.{location}"),
                "Normalized recipe copy could not be rendered by the runtime template engine",
                "Reject invalid room-name input or normalize copy within runtime limits",
            )
        })
}

fn runtime_error(trace: &str, error: &automation_core::AdapterError) -> StructuredError {
    let translated = translate_run_error(error);
    StructuredError::new(
        translated.code,
        format!("intent.simulation.{trace}"),
        translated.message,
        translated.hint,
    )
}

fn trace_error(trace: &str, message: &str, hint: &str) -> StructuredError {
    simulation_error(
        "INTENT_SIMULATION_TRACE_MISMATCH",
        format!("intent.simulation.{trace}"),
        message,
        hint,
    )
}

fn instance_manifest_error() -> StructuredError {
    simulation_error(
        "INTENT_SIMULATION_INSTANCE_MANIFEST_INVALID",
        "intent.simulation.submit.instance",
        "The created instance does not contain the exact compiled resource manifest",
        "Register member_role, room_channel, welcome_panel, and hub_panel exactly once",
    )
}

fn identity_error() -> StructuredError {
    simulation_error(
        "INTENT_SIMULATION_IDENTITY_SETUP_FAILED",
        "intent.simulation.identity",
        "The deterministic simulation identity could not be created",
        "Stop candidate processing and report the harness configuration error",
    )
}

fn simulation_error(
    code: impl Into<String>,
    location: impl Into<String>,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> StructuredError {
    StructuredError::new(code, location, message, hint)
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;
    use serde_json::json;

    use crate::gates::validate_candidate_with_bindings;
    use crate::intent::{
        propose_private_study_room, ClosePolicyV1, ExistingChannelKey, IntentLocaleV1,
        IntentProposalOutcomeV1, IntentRequestedOutcome, IntentResolutionContext,
        PrivateStudyRoomControlsProposalV1, PrivateStudyRoomCopyProposalV1,
        PrivateStudyRoomNamingProposalV1, PrivateStudyRoomProposalV1,
    };
    use crate::turn::{
        execute_plan_atomically_with_bindings, RequestedOutcome, SimulationProfile, TurnBrief,
        TurnIntent, TurnVerification,
    };

    use super::*;

    fn resolved_intent(hub: &str, close_policy: ClosePolicyV1) -> ValidatedIntentV1 {
        let context =
            IntentResolutionContext::from_channel_bindings([ExistingChannelKey(hub.to_string())]);
        let proposal = PrivateStudyRoomProposalV1 {
            objective: "Create a private study room".to_string(),
            requested_outcome: IntentRequestedOutcome::ValidatedPreview,
            hub_channel: Some(ExistingChannelKey(hub.to_string())),
            locale: Some(IntentLocaleV1::En),
            copy: PrivateStudyRoomCopyProposalV1::default(),
            naming: PrivateStudyRoomNamingProposalV1::default(),
            controls: PrivateStudyRoomControlsProposalV1 {
                close_policy: Some(close_policy),
                ..PrivateStudyRoomControlsProposalV1::default()
            },
        };
        let outcome = propose_private_study_room(proposal, &context)
            .expect("private room proposal should normalize");
        match outcome {
            IntentProposalOutcomeV1::Resolved { intent, .. } => intent,
            IntentProposalOutcomeV1::NeedsInput { decisions, .. } => {
                panic!("unexpected missing decisions: {decisions:?}")
            }
        }
    }

    fn bindings(hub: &str, channel: &str) -> ResourceBindingMap {
        let mut bindings = ResourceBindingMap::default();
        let key = serde_json::from_value(json!(hub)).expect("binding key should parse");
        let channel = channel.parse().expect("channel id should parse");
        bindings.channel_bindings.insert(key, channel);
        bindings
    }

    fn candidate(
        intent: &ValidatedIntentV1,
        compiled: &CompiledIntentV1,
        bindings: &ResourceBindingMap,
    ) -> Draft {
        let brief = TurnBrief {
            intent: TurnIntent::Build,
            objective: intent.objective().to_string(),
            requested_outcome: RequestedOutcome::ValidatedPreview,
            requirements: compiled.requirements.clone(),
            assumptions: vec![],
            blocking_decisions: vec![],
            verification: TurnVerification {
                validate: true,
                simulation: SimulationProfile::StudyRoom,
            },
        };
        let execution = block_on(execute_plan_atomically_with_bindings(
            &Draft::new(),
            &brief,
            bindings,
            32,
        ))
        .unwrap_or_else(|failure| panic!("candidate execution failed: {:?}", failure.error));
        let mut candidate = execution.draft;
        validate_candidate_with_bindings(&mut candidate, bindings)
            .expect("compiled candidate should validate");
        candidate
    }

    #[test]
    fn disabled_close_runs_four_traces_with_an_arbitrary_hub_binding() {
        let intent = resolved_intent("community_rooms", ClosePolicyV1::Disabled);
        let compiled = compile_intent(&intent).expect("intent should compile");
        let bindings = bindings("community_rooms", "902");
        let mut candidate = candidate(&intent, &compiled, &bindings);

        let report = block_on(simulate_compiled_intent(
            &mut candidate,
            &intent,
            &compiled,
            &bindings,
        ))
        .expect("disabled-close recipe should simulate");

        assert_eq!(
            report,
            IntentSimulationReportV1 {
                traces_run: 4,
                close_executed: false,
            }
        );
        assert_eq!(candidate.simulated_revision, Some(candidate.draft_revision));
    }

    #[test]
    fn any_member_close_runs_five_traces_and_exactly_one_teardown() {
        let intent = resolved_intent("study_hub", ClosePolicyV1::AnyMember);
        let compiled = compile_intent(&intent).expect("intent should compile");
        let bindings = bindings("study_hub", "700");
        let mut candidate = candidate(&intent, &compiled, &bindings);

        let report = block_on(simulate_compiled_intent(
            &mut candidate,
            &intent,
            &compiled,
            &bindings,
        ))
        .expect("any-member close recipe should simulate");

        assert_eq!(
            report,
            IntentSimulationReportV1 {
                traces_run: 5,
                close_executed: true,
            }
        );
        assert_eq!(candidate.simulated_revision, Some(candidate.draft_revision));
    }

    #[test]
    fn simulation_failure_clears_a_previous_simulation_revision() {
        let intent = resolved_intent("study_hub", ClosePolicyV1::Disabled);
        let compiled = compile_intent(&intent).expect("intent should compile");
        let bindings = bindings("study_hub", "700");
        let mut candidate = candidate(&intent, &compiled, &bindings);
        candidate.simulated_revision = Some(candidate.draft_revision);
        candidate
            .ruleset
            .rules
            .retain(|rule| rule.key != compiled.manifest.generated_objects["show_help"]);

        let error = block_on(simulate_compiled_intent(
            &mut candidate,
            &intent,
            &compiled,
            &bindings,
        ))
        .expect_err("missing deterministic help rule should fail");

        assert_eq!(error.code, "INTENT_SIMULATION_RULE_MISSING");
        assert_eq!(candidate.simulated_revision, None);
    }
}
