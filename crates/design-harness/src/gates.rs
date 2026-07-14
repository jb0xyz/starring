use std::collections::BTreeMap;

use automation_core::{
    interpret, run, validate, AutomationServices, EventKind, MockInstanceTeardownService,
    MockInteractionResponder, MockMutationAdapter, MutationCall, ResolvedButtonRoute,
    RunningRuleSetIdentity, RuntimeContext, RuntimeEvent,
};
use automation_instance::{
    InMemoryInstanceStore, InstanceId, InstanceRuleSetVersion, InstanceStore,
    SequenceInstanceIdGenerator,
};
use automation_state::{ActionSpec, ButtonRoute, TriggerSpec};
use resource_resolution::ResourceBindingMap;
use serde_json::{json, Value};

use crate::draft::Draft;
use crate::errors::{translate_run_error, translate_validation_error, StructuredError, ToolResult};

pub fn validate_draft(draft: &mut Draft) -> ToolResult {
    let bindings = match fixed_study_hub_bindings() {
        Ok(bindings) => bindings,
        Err(error) => return ToolResult::failure_from(draft, error),
    };
    match validate_candidate_with_bindings(draft, &bindings) {
        Ok(()) => ToolResult::success(draft, "Draft validation passed"),
        Err(error) => ToolResult::failure_from(draft, error),
    }
}

pub(crate) fn validate_candidate_with_bindings(
    draft: &mut Draft,
    bindings: &ResourceBindingMap,
) -> Result<(), StructuredError> {
    match validate(&draft.ruleset, bindings) {
        Ok(()) => {
            draft.validated_revision = Some(draft.draft_revision);
            draft.simulated_revision = None;
            Ok(())
        }
        Err(errors) => {
            draft.validated_revision = None;
            draft.simulated_revision = None;
            Err(errors
                .first()
                .map(|error| translate_validation_error(&draft.ruleset, error))
                .unwrap_or_else(|| {
                    StructuredError::new(
                        "VALIDATION_FAILED",
                        "draft",
                        "Draft validation failed",
                        "Review the Draft summary and correct the latest change",
                    )
                }))
        }
    }
}

pub async fn simulate_draft(draft: &mut Draft) -> ToolResult {
    let bindings = match fixed_study_hub_bindings() {
        Ok(bindings) => bindings,
        Err(error) => return ToolResult::failure_from(draft, error),
    };
    simulate_draft_with_bindings(draft, &bindings).await
}

pub(crate) async fn simulate_draft_with_bindings(
    draft: &mut Draft,
    bindings: &ResourceBindingMap,
) -> ToolResult {
    if draft.validated_revision != Some(draft.draft_revision) {
        return ToolResult::failure_from(
            draft,
            StructuredError::new(
                "DRAFT_NOT_VALIDATED",
                "draft.validation",
                "The current Draft revision has not passed validation",
                "Call validate_draft before simulate_draft",
            ),
        );
    }

    match run_golden_trace_with_bindings(draft, bindings).await {
        Ok(()) => {
            draft.simulated_revision = Some(draft.draft_revision);
            ToolResult::success(draft, "Golden StudyRoom trace passed")
        }
        Err(error) => {
            draft.simulated_revision = None;
            ToolResult::failure_from(draft, error)
        }
    }
}

pub(crate) async fn run_golden_trace_with_bindings(
    draft: &Draft,
    bindings: &ResourceBindingMap,
) -> Result<(), StructuredError> {
    let button = find_open_button(draft)?;
    let modal = find_submit_modal(draft)?;
    let identity = RunningRuleSetIdentity {
        key: "draft".to_string(),
        version: InstanceRuleSetVersion::new(1).map_err(|_| identity_setup_error())?,
    };
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let instances = InMemoryInstanceStore::new();
    let instance_ids = SequenceInstanceIdGenerator::new("study", 1);
    let teardown = MockInstanceTeardownService::new();
    let services = AutomationServices {
        mutation: &mutation,
        responder: &responder,
        instances: &instances,
        instance_ids: &instance_ids,
        teardown: &teardown,
    };

    let open_event = RuntimeEvent {
        guild_id: "1".parse().map_err(|_| identity_setup_error())?,
        actor: "42".parse().map_err(|_| identity_setup_error())?,
        kind: EventKind::ButtonClick { component: button },
    };
    let open_plan = interpret(&open_event, &draft.ruleset, bindings).ok_or_else(|| {
        StructuredError::new(
            "GOLDEN_TRACE_RULE_NOT_FOUND",
            "simulation.open_modal",
            "The golden trace button did not match a rule",
            "Add a matching button rule that opens the study modal",
        )
    })?;
    let open_context = RuntimeContext::from_event(&open_event, &identity);
    run(&open_context, &open_plan, &services)
        .await
        .map_err(|error| translate_run_error(&error))?;

    let submit_event = RuntimeEvent {
        guild_id: "1".parse().map_err(|_| identity_setup_error())?,
        actor: "42".parse().map_err(|_| identity_setup_error())?,
        kind: EventKind::ModalSubmit {
            modal,
            inputs: BTreeMap::from([("room_name".to_string(), "algebra".to_string())]),
        },
    };
    let submit_plan = interpret(&submit_event, &draft.ruleset, bindings).ok_or_else(|| {
        StructuredError::new(
            "GOLDEN_TRACE_RULE_NOT_FOUND",
            "simulation.modal_submit",
            "The golden trace modal submit did not match a rule",
            "Add a matching modal submit rule",
        )
    })?;
    let submit_context = RuntimeContext::from_event(&submit_event, &identity);
    run(&submit_context, &submit_plan, &services)
        .await
        .map_err(|error| translate_run_error(&error))?;

    assert_mutation_trace(&mutation.calls())?;
    assert_instance_manifest(&instances).await?;
    Ok(())
}

fn fixed_study_hub_bindings() -> Result<ResourceBindingMap, StructuredError> {
    let mut bindings = ResourceBindingMap::default();
    let key = serde_json::from_value(Value::String("study_hub".to_string()))
        .map_err(|_| binding_setup_error())?;
    let channel = "700".parse().map_err(|_| binding_setup_error())?;
    bindings.channel_bindings.insert(key, channel);
    Ok(bindings)
}

fn find_open_button(draft: &Draft) -> Result<String, StructuredError> {
    for panel in &draft.ruleset.panels {
        for button in &panel.buttons {
            if let ButtonRoute::Static { key } = &button.route {
                let opens_modal = draft.ruleset.rules.iter().any(|rule| {
                    matches!(
                        &rule.trigger,
                        TriggerSpec::ButtonClick { component } if component == key
                    ) && rule
                        .actions
                        .iter()
                        .any(|action| matches!(action, ActionSpec::OpenModal { .. }))
                });
                if opens_modal {
                    return Ok(key.clone());
                }
            }
        }
    }
    Err(StructuredError::new(
        "GOLDEN_TRACE_OPEN_BUTTON_MISSING",
        "simulation.open_modal",
        "No declared button opens the study modal",
        "Add a panel button and matching OpenModal rule",
    ))
}

fn find_submit_modal(draft: &Draft) -> Result<String, StructuredError> {
    draft
        .ruleset
        .rules
        .iter()
        .find_map(|rule| match &rule.trigger {
            TriggerSpec::ModalSubmit { modal } => Some(modal.clone()),
            _ => None,
        })
        .ok_or_else(|| {
            StructuredError::new(
                "GOLDEN_TRACE_SUBMIT_RULE_MISSING",
                "simulation.modal_submit",
                "No modal submit rule exists",
                "Begin a rule with the modal_submit trigger",
            )
        })
}

fn assert_mutation_trace(calls: &[MutationCall]) -> Result<(), StructuredError> {
    let role_count = calls
        .iter()
        .filter(|call| matches!(call, MutationCall::CreateRole { .. }))
        .count();
    if role_count != 1 {
        return Err(golden_count_error("role", 1, role_count));
    }
    let channel_count = calls
        .iter()
        .filter(|call| matches!(call, MutationCall::CreateChannel { .. }))
        .count();
    if channel_count != 1 {
        return Err(golden_count_error("channel", 1, channel_count));
    }
    let grants_actor = calls.iter().any(|call| {
        matches!(
            call,
            MutationCall::GrantRole { member, .. } if member.to_string() == "42"
        )
    });
    if !grants_actor {
        return Err(StructuredError::new(
            "GOLDEN_TRACE_ACTOR_GRANT_MISSING",
            "simulation.grant_role",
            "The submitted actor did not receive the created role",
            "Grant the created member role to target actor",
        ));
    }
    let private_overwrite = calls.iter().any(|call| match call {
        MutationCall::UpsertOverwrite { target, deny, .. } => {
            serde_json::to_value(target).ok() == Some(json!({"type":"role","id":"1"}))
                && deny.bits() & (1 << 10) != 0
        }
        _ => false,
    });
    if !private_overwrite {
        return Err(StructuredError::new(
            "GOLDEN_TRACE_PRIVATE_OVERWRITE_MISSING",
            "simulation.overwrites",
            "The created channel is not private from everyone",
            "Deny view_channel for everyone on the created channel",
        ));
    }

    let panels: Vec<_> = calls
        .iter()
        .filter_map(|call| match call {
            MutationCall::PostPanel { buttons, .. } => Some(buttons),
            _ => None,
        })
        .collect();
    if panels.len() != 2 || panels.iter().any(|buttons| buttons.is_empty()) {
        return Err(StructuredError::new(
            "GOLDEN_TRACE_PANEL_ROUTE_MISSING",
            "simulation.panels",
            "The welcome and hub panels do not both have resolved routes",
            "Post both panels with their static or instance-action buttons",
        ));
    }
    let has_static = panels
        .iter()
        .flat_map(|buttons| buttons.as_slice().iter())
        .any(|button| matches!(button.route, ResolvedButtonRoute::Static { .. }));
    let has_instance = panels
        .iter()
        .flat_map(|buttons| buttons.as_slice().iter())
        .any(|button| matches!(button.route, ResolvedButtonRoute::InstanceAction { .. }));
    if !has_static || !has_instance {
        return Err(StructuredError::new(
            "GOLDEN_TRACE_PANEL_ROUTE_MISSING",
            "simulation.panels",
            "Panel button routes were not resolved",
            "Include a static help route and a created-instance action route",
        ));
    }
    Ok(())
}

async fn assert_instance_manifest(
    instances: &InMemoryInstanceStore,
) -> Result<(), StructuredError> {
    let id = InstanceId::parse("study_001").map_err(|_| identity_setup_error())?;
    let instance = instances
        .get("1".parse().map_err(|_| identity_setup_error())?, &id)
        .await
        .map_err(|_| instance_manifest_error())?
        .ok_or_else(instance_manifest_error)?;
    if instance.resources.roles.len() != 1
        || instance.resources.channels.len() != 1
        || instance.resources.messages.len() != 2
        || !instance.resources.roles.contains_key("member_role")
        || !instance.resources.channels.contains_key("room_channel")
        || !instance.resources.messages.contains_key("welcome_panel")
        || !instance.resources.messages.contains_key("hub_panel")
    {
        return Err(instance_manifest_error());
    }
    Ok(())
}

fn golden_count_error(kind: &str, expected: usize, actual: usize) -> StructuredError {
    StructuredError::new(
        "GOLDEN_TRACE_RESOURCE_COUNT_MISMATCH",
        format!("simulation.{kind}"),
        format!("Expected {expected} created {kind}, found {actual}"),
        format!("Create exactly one {kind} in the modal submit rule"),
    )
}

fn instance_manifest_error() -> StructuredError {
    StructuredError::new(
        "GOLDEN_TRACE_INSTANCE_MANIFEST_INCOMPLETE",
        "simulation.register_instance",
        "The registered instance manifest is incomplete",
        "Register the created role, channel, welcome panel, and hub panel exactly once",
    )
}

fn binding_setup_error() -> StructuredError {
    StructuredError::new(
        "HARNESS_BINDING_SETUP_FAILED",
        "draft.bindings",
        "The fixed study_hub binding could not be created",
        "Stop the session and report the harness configuration error",
    )
}

fn identity_setup_error() -> StructuredError {
    StructuredError::new(
        "HARNESS_IDENTITY_SETUP_FAILED",
        "simulation.identity",
        "The fixed golden trace identity could not be created",
        "Stop the session and report the harness configuration error",
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn validation_accepts_an_explicit_nonlegacy_channel_binding() {
        let mut draft = Draft::new();
        draft.ruleset = serde_json::from_value(json!({
            "version": 1,
            "panels": [{
                "key": "panel",
                "channel": "community_hub",
                "content": "Rooms",
                "buttons": []
            }],
            "modals": [],
            "rules": []
        }))
        .unwrap();
        draft.draft_revision = 1;
        let mut bindings = ResourceBindingMap::default();
        bindings.channel_bindings.insert(
            serde_json::from_value(json!("community_hub")).unwrap(),
            "700".parse().unwrap(),
        );

        validate_candidate_with_bindings(&mut draft, &bindings).unwrap();

        assert_eq!(draft.validated_revision, Some(1));
        assert_eq!(draft.simulated_revision, None);
    }
}
