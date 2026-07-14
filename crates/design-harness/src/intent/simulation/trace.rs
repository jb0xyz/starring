use std::collections::BTreeMap;

use automation_core::{
    interpret, run, ActionPlan, AutomationServices, EventKind, MockInteractionResponder,
    MutationCall, ResolvedInstanceContext, ResponderCall, RuntimeContext, RuntimeEvent,
    SanitizeContext,
};
use automation_instance::{AutomationInstance, InstanceId};
use automation_state::InteractionRule;
use resource_resolution::ResourceBindingMap;

use crate::draft::Draft;
use crate::errors::StructuredError;

use super::super::model::ResolvedManagedPrivateRoomV1;
use super::oracle::{
    assert_created_resources, assert_submit_mutations, expect_responses, load_and_verify_instance,
};
use super::support::{
    identity_error, instance_manifest_error, render_literal, render_pattern, runtime_error,
    simulation_error, trace_error, CLOSER_ID, CREATOR_ID, GUILD_ID, JOINER_ID, MEMBER_ROLE_ALIAS,
    ROOM_NAME_INPUT, ROOM_NAME_VALUE,
};
use super::{CloseKeys, RecipeKeys, SimulationRuntime};

pub(super) async fn run_open_trace(
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

pub(super) async fn run_submit_trace(
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

pub(super) async fn run_help_trace(
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

pub(super) async fn run_join_trace(
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

pub(super) async fn run_close_trace(
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
