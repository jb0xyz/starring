use automation_state::{ActionSpec, ButtonRoute, ChannelRef, InstanceRef};
use resource_resolution::ResourceBindingMap;
use serde_json::{json, Map, Value};

use crate::draft::Draft;
use crate::errors::{StructuredError, ToolResult};
use crate::tools::dispatch_tool;

use super::{
    check_scope, check_scope_with_bindings, validate_final_planned_action_order, ScopeAction,
    ScopeButtonRoute, ScopeInstanceRef, ScopeOverwriteTarget, ScopePostPanelButton,
    ScopePostPanelButtonRoute, ScopeRequirement, ScopeResourceRef, ScopeRoleRef, ScopeTrigger,
    TurnBrief,
};

const PENDING_INSTANCE_REFERENCE: &str = "__pending_instance__";

#[derive(Debug)]
pub(crate) struct ExecutionRecord {
    pub(crate) name: String,
    pub(crate) result: ToolResult,
}

#[derive(Debug)]
pub(crate) struct PlannedExecution {
    pub(crate) draft: Draft,
    pub(crate) records: Vec<ExecutionRecord>,
}

#[derive(Debug)]
pub(crate) struct PlannedExecutionFailure {
    pub(crate) error: StructuredError,
    pub(crate) records: Vec<ExecutionRecord>,
}

enum RequirementState {
    Exact,
    Provisional,
    Missing,
    Conflict,
}

pub(crate) async fn execute_plan_atomically(
    draft: &Draft,
    brief: &TurnBrief,
    max_calls: usize,
) -> Result<PlannedExecution, PlannedExecutionFailure> {
    execute_plan_atomically_for_bindings(draft, brief, None, max_calls).await
}

async fn execute_plan_atomically_for_bindings(
    draft: &Draft,
    brief: &TurnBrief,
    bindings: Option<&ResourceBindingMap>,
    max_calls: usize,
) -> Result<PlannedExecution, PlannedExecutionFailure> {
    let mut candidate = draft.clone();
    let mut records = Vec::new();
    for requirement in &brief.requirements {
        loop {
            match requirement_state(&candidate, brief, requirement, bindings) {
                RequirementState::Exact | RequirementState::Provisional => break,
                RequirementState::Conflict => {
                    return Err(PlannedExecutionFailure {
                        error: StructuredError::new(
                            "PLAN_TARGET_CONFLICT",
                            format!("turn.plan.{}", requirement.id()),
                            "A stable target already exists with different semantics",
                            "Revise the plan to update the existing target instead of adding a duplicate",
                        ),
                        records,
                    });
                }
                RequirementState::Missing => {}
            }
            let compiled = match compile_requirement(requirement) {
                Ok(compiled) => compiled,
                Err(error) => {
                    return Err(PlannedExecutionFailure { error, records });
                }
            };
            let Some((name, arguments)) = compiled else {
                break;
            };
            if records.len() >= max_calls {
                return Err(PlannedExecutionFailure {
                    error: StructuredError::new(
                        "PLAN_TOOL_CALL_LIMIT",
                        "turn.plan",
                        "The atomic plan needs more tool calls than remain in this turn",
                        "Reduce the plan packet or increase the per-turn tool call budget",
                    ),
                    records,
                });
            }
            let result = dispatch_tool(&mut candidate, name, &arguments.to_string()).await;
            let succeeded = result.is_ok();
            let failure = result.failure().map(|failure| {
                StructuredError::new(
                    failure.code.clone(),
                    failure.location.clone(),
                    failure.message.clone(),
                    failure.hint.clone(),
                )
            });
            records.push(ExecutionRecord {
                name: name.to_string(),
                result,
            });
            if !succeeded {
                return Err(PlannedExecutionFailure {
                    error: failure.unwrap_or_else(|| {
                        StructuredError::new(
                            "PLAN_WORK_ITEM_FAILED",
                            format!("turn.plan.{}", requirement.id()),
                            "A planned design operation failed",
                            "Revise the failing work item before retrying the plan",
                        )
                    }),
                    records,
                });
            }
            match requirement_state(&candidate, brief, requirement, bindings) {
                RequirementState::Exact | RequirementState::Provisional => {}
                RequirementState::Missing if repeatable_requirement(requirement) => continue,
                RequirementState::Missing | RequirementState::Conflict => {
                    return Err(PlannedExecutionFailure {
                        error: StructuredError::new(
                            "PLAN_POSTCONDITION_FAILED",
                            format!("turn.plan.{}", requirement.id()),
                            "The design tool succeeded without satisfying its planned semantic operation",
                            "Revise the work item so its exact payload matches the intended Draft change",
                        ),
                        records,
                    });
                }
            }
        }
    }
    let scope = bindings.map_or_else(
        || check_scope(&candidate, brief),
        |bindings| check_scope_with_bindings(&candidate, brief, bindings),
    );
    if !scope.ok {
        return Err(PlannedExecutionFailure {
            error: StructuredError::new(
                "PLAN_SCOPE_INCOMPLETE",
                "turn.plan",
                format!(
                    "The atomic plan did not satisfy requirements: {}",
                    scope.missing.join(", ")
                ),
                "Revise the plan to include the missing dependencies and exact final values",
            ),
            records,
        });
    }
    if let Err(error) = validate_final_planned_action_order(draft, &candidate, &brief.requirements)
    {
        return Err(PlannedExecutionFailure { error, records });
    }
    Ok(PlannedExecution {
        draft: candidate,
        records,
    })
}

fn compile_requirement(
    requirement: &ScopeRequirement,
) -> Result<Option<(&'static str, Value)>, StructuredError> {
    let compiled = match requirement {
        ScopeRequirement::Panel {
            key,
            channel,
            content,
            ..
        } => Some((
            "add_panel",
            json!({"key":key,"channel":channel,"content":content}),
        )),
        ScopeRequirement::Button {
            panel_key,
            label,
            route,
            ..
        } => Some((
            "add_button",
            json!({"panel_key":panel_key,"label":label,"route":button_route(route)}),
        )),
        ScopeRequirement::Modal {
            key, title, fields, ..
        } => Some((
            "add_modal",
            json!({"key":key,"title":title,"fields":fields}),
        )),
        ScopeRequirement::Rule { key, trigger, .. } => {
            let (trigger_kind, trigger_ref) = trigger_parts(trigger);
            Some((
                "begin_rule",
                json!({"key":key,"trigger_kind":trigger_kind,"trigger_ref":trigger_ref}),
            ))
        }
        ScopeRequirement::Action {
            rule_key, action, ..
        } => Some(compile_action(requirement.id(), rule_key, action)?),
        ScopeRequirement::NoUnresolvedReferences { .. } => None,
    };
    Ok(compiled)
}

fn compile_action(
    requirement_id: &str,
    rule_key: &str,
    action: &ScopeAction,
) -> Result<(&'static str, Value), StructuredError> {
    let compiled = match action {
        ScopeAction::CreateRole { key, name } => (
            "add_resource_action",
            json!({"rule_key":rule_key,"kind":"create_role","key":key,"name":name}),
        ),
        ScopeAction::CreateChannel { key, name } => (
            "add_resource_action",
            json!({"rule_key":rule_key,"kind":"create_channel","key":key,"name":name}),
        ),
        ScopeAction::GrantRole { role, target } => (
            "add_grant_role_action",
            json!({"rule_key":rule_key,"role":role_reference(requirement_id, role)?,"target":target}),
        ),
        ScopeAction::RespondEphemeral { content } => (
            "add_interaction_action",
            json!({"rule_key":rule_key,"kind":"respond_ephemeral","content":content}),
        ),
        ScopeAction::OpenModal { modal } => (
            "add_interaction_action",
            json!({"rule_key":rule_key,"kind":"open_modal","modal":modal}),
        ),
        ScopeAction::UpsertOverwrite {
            channel,
            target,
            allow,
            deny,
        } => {
            let mut arguments = Map::from_iter([
                ("rule_key".to_string(), json!(rule_key)),
                ("channel".to_string(), resource_reference(channel)),
                ("allow".to_string(), json!(allow)),
                ("deny".to_string(), json!(deny)),
            ]);
            match target {
                ScopeOverwriteTarget::Everyone => {
                    arguments.insert("target_kind".to_string(), json!("everyone"));
                }
                ScopeOverwriteTarget::Role { role } => {
                    arguments.insert("target_kind".to_string(), json!("role"));
                    arguments.insert("role".to_string(), role_reference(requirement_id, role)?);
                }
            }
            ("add_upsert_overwrite_action", Value::Object(arguments))
        }
        ScopeAction::PostPanel {
            key,
            channel,
            content,
            buttons,
        } => (
            "add_post_panel_action",
            json!({
                "rule_key":rule_key,
                "key":key,
                "channel":resource_reference(channel),
                "content":content,
                "buttons":buttons
                    .iter()
                    .map(|button| post_panel_button(requirement_id, button))
                    .collect::<Result<Vec<_>, _>>()?
            }),
        ),
        ScopeAction::DeferEphemeral => (
            "add_interaction_action",
            json!({"rule_key":rule_key,"kind":"defer_ephemeral"}),
        ),
        ScopeAction::EditResponse { content } => (
            "add_interaction_action",
            json!({"rule_key":rule_key,"kind":"edit_response","content":content}),
        ),
        ScopeAction::RegisterInstance {
            key,
            instance_kind,
            resources,
        } => (
            "set_register_instance",
            json!({
                "rule_key":rule_key,
                "instance_key":key,
                "kind":instance_kind,
                "roles":resources.roles,
                "channels":resources.channels,
                "messages":resources.messages
            }),
        ),
        ScopeAction::TeardownInstance { instance } => {
            if *instance != ScopeInstanceRef::Event {
                return Err(unsupported(requirement_id, "created instance teardown"));
            }
            (
                "add_interaction_action",
                json!({"rule_key":rule_key,"kind":"teardown_instance"}),
            )
        }
    };
    Ok(compiled)
}

fn role_reference(
    requirement_id: &str,
    reference: &ScopeRoleRef,
) -> Result<Value, StructuredError> {
    match reference {
        ScopeRoleRef::Created { name } => Ok(json!({"kind":"created","name":name})),
        ScopeRoleRef::Existing { name } => Ok(json!({"kind":"existing","name":name})),
        ScopeRoleRef::Instance {
            instance: ScopeInstanceRef::Event,
            alias,
        } => Ok(json!({"kind":"instance_event","alias":alias})),
        ScopeRoleRef::Instance {
            instance: ScopeInstanceRef::Created { .. },
            ..
        } => Err(unsupported(
            requirement_id,
            "created instance role reference",
        )),
    }
}

fn resource_reference(reference: &ScopeResourceRef) -> Value {
    match reference {
        ScopeResourceRef::Created { name } => json!({"kind":"created","name":name}),
        ScopeResourceRef::Existing { name } => json!({"kind":"existing","name":name}),
    }
}

fn button_route(route: &ScopeButtonRoute) -> Value {
    match route {
        ScopeButtonRoute::Static { key } => json!({"kind":"static","key":key}),
        ScopeButtonRoute::InstanceAction { action } => {
            json!({"kind":"instance_action","action":action})
        }
    }
}

fn post_panel_button(
    requirement_id: &str,
    button: &ScopePostPanelButton,
) -> Result<Value, StructuredError> {
    let route = match &button.route {
        ScopePostPanelButtonRoute::Static { key } => json!({"kind":"static","key":key}),
        ScopePostPanelButtonRoute::InstanceAction { instance, action } => {
            if !matches!(instance, ScopeInstanceRef::Created { .. }) {
                return Err(unsupported(
                    requirement_id,
                    "event instance post-panel route",
                ));
            }
            json!({"kind":"instance_action","action":action})
        }
    };
    Ok(json!({"label":button.label,"route":route}))
}

fn trigger_parts(trigger: &ScopeTrigger) -> (&'static str, &str) {
    match trigger {
        ScopeTrigger::ButtonClick { component } => ("button_click", component),
        ScopeTrigger::ModalSubmit { modal } => ("modal_submit", modal),
        ScopeTrigger::InstanceAction { action } => ("instance_action", action),
    }
}

fn unsupported(requirement_id: &str, operation: &str) -> StructuredError {
    StructuredError::new(
        "PLAN_OPERATION_UNSUPPORTED",
        format!("turn.plan.{requirement_id}"),
        format!("The current design tools cannot encode {operation}"),
        "Revise the plan to use a reference form supported by the existing design tools",
    )
}

fn requirement_state(
    draft: &Draft,
    brief: &TurnBrief,
    requirement: &ScopeRequirement,
    bindings: Option<&ResourceBindingMap>,
) -> RequirementState {
    if exact_requirement_satisfied(draft, brief, requirement, bindings) {
        return RequirementState::Exact;
    }
    if provisional_post_panel_satisfied(draft, requirement) {
        return RequirementState::Provisional;
    }
    if stable_target_exists(draft, requirement) {
        return RequirementState::Conflict;
    }
    RequirementState::Missing
}

fn exact_requirement_satisfied(
    draft: &Draft,
    brief: &TurnBrief,
    requirement: &ScopeRequirement,
    bindings: Option<&ResourceBindingMap>,
) -> bool {
    let mut single = brief.clone();
    single.requirements = vec![requirement.clone()];
    bindings.map_or_else(
        || check_scope(draft, &single).ok,
        |bindings| check_scope_with_bindings(draft, &single, bindings).ok,
    )
}

fn stable_target_exists(draft: &Draft, requirement: &ScopeRequirement) -> bool {
    match requirement {
        ScopeRequirement::Panel { key, .. } => {
            draft.ruleset.panels.iter().any(|panel| panel.key == *key)
        }
        ScopeRequirement::Button {
            panel_key, route, ..
        } => draft
            .ruleset
            .panels
            .iter()
            .find(|panel| panel.key == *panel_key)
            .is_some_and(|panel| {
                panel
                    .buttons
                    .iter()
                    .any(|button| declared_button_route_matches(&button.route, route))
            }),
        ScopeRequirement::Modal { key, .. } => {
            draft.ruleset.modals.iter().any(|modal| modal.key == *key)
        }
        ScopeRequirement::Rule { key, .. } => {
            draft.ruleset.rules.iter().any(|rule| rule.key == *key)
        }
        ScopeRequirement::Action {
            rule_key, action, ..
        } => action_key(action).is_some_and(|key| {
            draft
                .ruleset
                .rules
                .iter()
                .find(|rule| rule.key == *rule_key)
                .is_some_and(|rule| {
                    rule.actions
                        .iter()
                        .any(|candidate| action_spec_key(candidate) == Some(key))
                })
        }),
        ScopeRequirement::NoUnresolvedReferences { .. } => false,
    }
}

fn action_key(action: &ScopeAction) -> Option<&str> {
    match action {
        ScopeAction::CreateRole { key, .. }
        | ScopeAction::CreateChannel { key, .. }
        | ScopeAction::PostPanel { key, .. }
        | ScopeAction::RegisterInstance { key, .. } => Some(key),
        _ => None,
    }
}

fn repeatable_requirement(requirement: &ScopeRequirement) -> bool {
    matches!(
        requirement,
        ScopeRequirement::Action { action, .. } if action_key(action).is_none()
    )
}

fn action_spec_key(action: &ActionSpec) -> Option<&str> {
    match action {
        ActionSpec::CreateRole { key, .. }
        | ActionSpec::CreateChannel { key, .. }
        | ActionSpec::PostPanel { key, .. }
        | ActionSpec::RegisterInstance { key, .. } => Some(key),
        _ => None,
    }
}

fn declared_button_route_matches(route: &ButtonRoute, expected: &ScopeButtonRoute) -> bool {
    match (route, expected) {
        (ButtonRoute::Static { key }, ScopeButtonRoute::Static { key: expected }) => {
            key == expected
        }
        (
            ButtonRoute::InstanceAction { action, .. },
            ScopeButtonRoute::InstanceAction { action: expected },
        ) => action == expected,
        _ => false,
    }
}

fn provisional_post_panel_satisfied(draft: &Draft, requirement: &ScopeRequirement) -> bool {
    let ScopeRequirement::Action {
        rule_key,
        action:
            ScopeAction::PostPanel {
                key,
                channel,
                content,
                buttons,
            },
        ..
    } = requirement
    else {
        return false;
    };
    draft
        .ruleset
        .rules
        .iter()
        .find(|rule| rule.key == *rule_key)
        .is_some_and(|rule| {
            rule.actions.iter().any(|candidate| {
                let ActionSpec::PostPanel {
                    key: candidate_key,
                    channel: candidate_channel,
                    content: candidate_content,
                    buttons: candidate_buttons,
                } = candidate
                else {
                    return false;
                };
                candidate_key == key
                    && channel_matches(candidate_channel, channel)
                    && candidate_content == content
                    && candidate_buttons.len() == buttons.len()
                    && candidate_buttons
                        .iter()
                        .zip(buttons)
                        .all(|(button, expected)| provisional_button_matches(button, expected))
            })
        })
}

fn channel_matches(channel: &ChannelRef, expected: &ScopeResourceRef) -> bool {
    match (channel, expected) {
        (ChannelRef::Created(reference), ScopeResourceRef::Created { name }) => {
            reference.created == *name
        }
        (ChannelRef::Existing(reference), ScopeResourceRef::Existing { name }) => {
            reference.0 == *name
        }
        _ => false,
    }
}

fn provisional_button_matches(
    button: &automation_state::ButtonSpec,
    expected: &ScopePostPanelButton,
) -> bool {
    if button.label != expected.label {
        return false;
    }
    match (&button.route, &expected.route) {
        (ButtonRoute::Static { key }, ScopePostPanelButtonRoute::Static { key: expected }) => {
            key == expected
        }
        (
            ButtonRoute::InstanceAction { instance, action },
            ScopePostPanelButtonRoute::InstanceAction {
                instance: expected_instance,
                action: expected_action,
            },
        ) => action == expected_action && provisional_instance_matches(instance, expected_instance),
        _ => false,
    }
}

fn provisional_instance_matches(instance: &InstanceRef, expected: &ScopeInstanceRef) -> bool {
    match (instance, expected) {
        (InstanceRef::Event, ScopeInstanceRef::Event) => true,
        (InstanceRef::Created(reference), ScopeInstanceRef::Created { name }) => {
            reference.created == *name || reference.created == PENDING_INSTANCE_REFERENCE
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use automation_state::InteractionRuleSet;
    use futures::executor::block_on;
    use serde_json::json;

    use crate::turn::{
        RequestedOutcome, ScopeActionTarget, ScopeInstanceResources, ScopeManifestEntry,
        ScopePermission, SimulationProfile, TurnIntent, TurnVerification,
    };

    use super::*;

    fn brief(requirements: Vec<ScopeRequirement>) -> TurnBrief {
        TurnBrief {
            intent: TurnIntent::Build,
            objective: "Build the requested automation".to_string(),
            requested_outcome: RequestedOutcome::DraftUpdate,
            requirements,
            assumptions: Vec::new(),
            blocking_decisions: Vec::new(),
            verification: TurnVerification {
                validate: false,
                simulation: SimulationProfile::None,
            },
        }
    }

    fn post_panel_requirement() -> ScopeRequirement {
        ScopeRequirement::Action {
            id: "welcome_panel".to_string(),
            rule_key: "submit_room".to_string(),
            action: ScopeAction::PostPanel {
                key: "welcome_panel".to_string(),
                channel: ScopeResourceRef::Created {
                    name: "room_channel".to_string(),
                },
                content: "Welcome".to_string(),
                buttons: vec![ScopePostPanelButton {
                    label: "Join".to_string(),
                    route: ScopePostPanelButtonRoute::InstanceAction {
                        instance: ScopeInstanceRef::Created {
                            name: "study_instance".to_string(),
                        },
                        action: "join".to_string(),
                    },
                }],
            },
            minimum: 1,
        }
    }

    fn register_requirement() -> ScopeRequirement {
        ScopeRequirement::Action {
            id: "register".to_string(),
            rule_key: "submit_room".to_string(),
            action: ScopeAction::RegisterInstance {
                key: "study_instance".to_string(),
                instance_kind: "study_room".to_string(),
                resources: ScopeInstanceResources {
                    roles: vec![ScopeManifestEntry {
                        alias: "member".to_string(),
                        created: "member_role".to_string(),
                    }],
                    channels: vec![ScopeManifestEntry {
                        alias: "room".to_string(),
                        created: "room_channel".to_string(),
                    }],
                    messages: vec![ScopeManifestEntry {
                        alias: "welcome".to_string(),
                        created: "welcome_panel".to_string(),
                    }],
                },
            },
            minimum: 1,
        }
    }

    fn resource_draft() -> Draft {
        let mut draft = Draft::new();
        draft.ruleset = serde_json::from_value::<InteractionRuleSet>(json!({
            "version":1,
            "panels":[],
            "modals":[],
            "rules":[{
                "key":"submit_room",
                "trigger":{"type":"instance_action","action":"submit"},
                "actions":[
                    {"type":"create_role","key":"member_role","name":"Member"},
                    {"type":"create_channel","key":"room_channel","name":"Room"}
                ]
            }]
        }))
        .unwrap();
        draft.draft_revision = 2;
        draft
    }

    #[test]
    fn atomic_plan_lowers_the_bounded_instance_event_role_reference() {
        block_on(async {
            let draft = Draft::new();
            let brief = brief(vec![
                ScopeRequirement::Rule {
                    id: "join_rule".to_string(),
                    key: "join_room".to_string(),
                    trigger: ScopeTrigger::InstanceAction {
                        action: "join".to_string(),
                    },
                },
                ScopeRequirement::Action {
                    id: "join_defer".to_string(),
                    rule_key: "join_room".to_string(),
                    action: ScopeAction::DeferEphemeral,
                    minimum: 1,
                },
                ScopeRequirement::Action {
                    id: "join_grant".to_string(),
                    rule_key: "join_room".to_string(),
                    action: ScopeAction::GrantRole {
                        role: ScopeRoleRef::Instance {
                            instance: ScopeInstanceRef::Event,
                            alias: "member_role".to_string(),
                        },
                        target: ScopeActionTarget::Actor,
                    },
                    minimum: 1,
                },
                ScopeRequirement::Action {
                    id: "join_response".to_string(),
                    rule_key: "join_room".to_string(),
                    action: ScopeAction::EditResponse {
                        content: "Joined".to_string(),
                    },
                    minimum: 1,
                },
                ScopeRequirement::NoUnresolvedReferences {
                    id: "refs".to_string(),
                },
            ]);

            let execution = execute_plan_atomically(&draft, &brief, 4)
                .await
                .expect("bounded instance event role should lower");

            assert!(check_scope(&execution.draft, &brief).ok);
            assert!(matches!(
                &execution.draft.ruleset.rules[0].actions[1],
                ActionSpec::GrantRole {
                    role: automation_state::RoleRef::Instance {
                        instance: InstanceRef::Event,
                        alias,
                    },
                    ..
                } if alias == "member_role"
            ));
        });
    }

    #[test]
    fn post_panel_provisional_reference_is_finalized_before_atomic_commit() {
        block_on(async {
            let draft = resource_draft();
            let brief = brief(vec![
                post_panel_requirement(),
                register_requirement(),
                ScopeRequirement::NoUnresolvedReferences {
                    id: "refs".to_string(),
                },
            ]);

            let execution = execute_plan_atomically(&draft, &brief, 4).await.unwrap();

            assert_eq!(
                execution
                    .records
                    .iter()
                    .map(|record| record.name.as_str())
                    .collect::<Vec<_>>(),
                ["add_post_panel_action", "set_register_instance"]
            );
            assert!(check_scope(&execution.draft, &brief).ok);
            assert_eq!(execution.draft.draft_revision, 4);
            assert_eq!(draft.draft_revision, 2);
            assert!(!serde_json::to_string(&execution.draft.ruleset)
                .unwrap()
                .contains(PENDING_INSTANCE_REFERENCE));
        });
    }

    #[test]
    fn divergent_stable_target_fails_without_dispatch_or_mutation() {
        block_on(async {
            let mut draft = Draft::new();
            dispatch_tool(
                &mut draft,
                "add_panel",
                &json!({"key":"panel","channel":"study_hub","content":"Old"}).to_string(),
            )
            .await;
            let before = draft.clone();
            let brief = brief(vec![
                ScopeRequirement::Panel {
                    id: "panel".to_string(),
                    key: "panel".to_string(),
                    channel: "study_hub".to_string(),
                    content: "New".to_string(),
                },
                ScopeRequirement::NoUnresolvedReferences {
                    id: "refs".to_string(),
                },
            ]);

            let failure = execute_plan_atomically(&draft, &brief, 4)
                .await
                .unwrap_err();

            assert_eq!(failure.error.code, "PLAN_TARGET_CONFLICT");
            assert!(failure.records.is_empty());
            assert_eq!(draft, before);
        });
    }

    #[test]
    fn later_work_item_failure_discards_every_candidate_change() {
        block_on(async {
            let draft = Draft::new();
            let before = draft.clone();
            let brief = brief(vec![
                ScopeRequirement::Panel {
                    id: "panel".to_string(),
                    key: "panel".to_string(),
                    channel: "study_hub".to_string(),
                    content: "Panel".to_string(),
                },
                ScopeRequirement::Action {
                    id: "role".to_string(),
                    rule_key: "missing_rule".to_string(),
                    action: ScopeAction::CreateRole {
                        key: "member".to_string(),
                        name: "Member".to_string(),
                    },
                    minimum: 1,
                },
                ScopeRequirement::NoUnresolvedReferences {
                    id: "refs".to_string(),
                },
            ]);

            let failure = execute_plan_atomically(&draft, &brief, 4)
                .await
                .unwrap_err();

            assert_eq!(failure.error.code, "RULE_NOT_FOUND");
            assert_eq!(failure.records.len(), 2);
            assert!(failure.records[0].result.is_ok());
            assert!(!failure.records[1].result.is_ok());
            assert_eq!(draft, before);
        });
    }

    #[test]
    fn repeatable_action_executes_only_the_absolute_minimum_deficit() {
        block_on(async {
            let mut draft = Draft::new();
            draft.ruleset = serde_json::from_value(json!({
                "version":1,
                "panels":[],
                "modals":[],
                "rules":[{
                    "key":"room",
                    "trigger":{"type":"instance_action","action":"join"},
                    "actions":[
                        {"type":"create_channel","key":"room_channel","name":"Room"},
                        {
                            "type":"upsert_overwrite",
                            "channel":{"created":"room_channel"},
                            "target":"everyone",
                            "allow":"0",
                            "deny":"1024"
                        }
                    ]
                }]
            }))
            .unwrap();
            let brief = brief(vec![ScopeRequirement::Action {
                id: "overwrite".to_string(),
                rule_key: "room".to_string(),
                action: ScopeAction::UpsertOverwrite {
                    channel: ScopeResourceRef::Created {
                        name: "room_channel".to_string(),
                    },
                    target: ScopeOverwriteTarget::Everyone,
                    allow: Vec::new(),
                    deny: vec![ScopePermission::ViewChannel],
                },
                minimum: 2,
            }]);

            let execution = execute_plan_atomically(&draft, &brief, 4).await.unwrap();

            assert_eq!(execution.records.len(), 1);
            assert_eq!(execution.records[0].name, "add_upsert_overwrite_action");
            assert_eq!(
                execution.draft.ruleset.rules[0]
                    .actions
                    .iter()
                    .filter(|action| matches!(action, ActionSpec::UpsertOverwrite { .. }))
                    .count(),
                2
            );
        });
    }
}
