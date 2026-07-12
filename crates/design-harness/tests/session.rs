mod support;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use automation_state::{ActionSpec, InteractionRule, OverwriteTargetSpec, TriggerSpec};
use design_harness::{
    tool_definitions, BurstOutcome, DesignSession, Draft, LimitKind, LlmClient, LlmError,
    LlmResponse, Message, MessageRole, SessionConfig, ToolCall, DEFAULT_SYSTEM_PROMPT,
};
use futures::executor::block_on;
use serde_json::{json, Value};

#[derive(Clone)]
struct ScriptedClient {
    responses: Arc<Mutex<VecDeque<Result<LlmResponse, LlmError>>>>,
    seen: Arc<Mutex<Vec<Vec<Message>>>>,
    seen_tools: Arc<Mutex<Vec<Vec<String>>>>,
}

impl ScriptedClient {
    fn new(responses: Vec<LlmResponse>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses.into_iter().map(Ok).collect())),
            seen: Arc::new(Mutex::new(Vec::new())),
            seen_tools: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn seen(&self) -> Vec<Vec<Message>> {
        self.seen.lock().unwrap().clone()
    }

    fn seen_tools(&self) -> Vec<Vec<String>> {
        self.seen_tools.lock().unwrap().clone()
    }
}

impl LlmClient for ScriptedClient {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[design_harness::ToolDefinition],
    ) -> Result<LlmResponse, LlmError> {
        self.seen.lock().unwrap().push(messages.to_vec());
        self.seen_tools
            .lock()
            .unwrap()
            .push(tools.iter().map(|tool| tool.name.clone()).collect());
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Ok(LlmResponse::Text("QUESTION: no response".to_string())))
    }
}

fn call(id: &str, name: &str, arguments: Value) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: name.to_string(),
        arguments: arguments.to_string(),
    }
}

fn large_config() -> SessionConfig {
    SessionConfig {
        context_char_budget: 100_000,
        ..SessionConfig::default()
    }
}

fn long_flow_config() -> SessionConfig {
    SessionConfig {
        max_model_calls: 64,
        max_tool_calls: 64,
        max_gate_failures: 4,
        context_char_budget: 500_000,
    }
}

fn names(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn structure_tool_names(has_panel: bool) -> Vec<String> {
    if has_panel {
        names(&["add_panel", "add_button", "add_modal", "begin_rule"])
    } else {
        names(&["add_panel", "add_modal", "begin_rule"])
    }
}

fn rule_tool_names(
    has_panel: bool,
    has_created_role: bool,
    has_ownable_action: bool,
    gate: Option<&str>,
) -> Vec<String> {
    let mut values = vec!["add_panel"];
    if has_panel {
        values.push("add_button");
    }
    values.extend(["add_modal", "begin_rule", "add_resource_action"]);
    if has_created_role {
        values.push("add_grant_role_action");
    }
    values.extend([
        "add_upsert_overwrite_action",
        "add_interaction_action",
        "add_post_panel_action",
    ]);
    if has_ownable_action {
        values.push("set_register_instance");
    }
    if let Some(gate) = gate {
        values.push(gate);
    }
    names(&values)
}

fn rule_design_tool_names() -> Vec<String> {
    rule_tool_names(true, true, true, Some("validate_draft"))
}

fn simulation_tool_names() -> Vec<String> {
    rule_tool_names(true, true, true, Some("simulate_draft"))
}

fn assert_complete_tool_pairs(messages: &[Message]) {
    for (assistant_index, message) in messages.iter().enumerate() {
        for call in &message.tool_calls {
            assert!(messages.iter().skip(assistant_index + 1).any(|candidate| {
                candidate.role == MessageRole::Tool
                    && candidate.tool_call_id.as_deref() == Some(call.id.as_str())
            }));
        }
    }
    for (tool_index, message) in messages.iter().enumerate() {
        if message.role != MessageRole::Tool {
            continue;
        }
        let tool_call_id = message.tool_call_id.as_deref().unwrap();
        assert!(messages[..tool_index].iter().any(|candidate| {
            candidate
                .tool_calls
                .iter()
                .any(|call| call.id == tool_call_id)
        }));
    }
}

#[test]
fn tool_calls_execute_serially_before_the_next_model_call() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "1",
                "add_panel",
                json!({"key":"p","channel":"study_hub","content":"Panel"}),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "2",
                "add_button",
                json!({
                    "panel_key":"p",
                    "label":"Open",
                    "route":{"kind":"static","key":"open"}
                }),
            )]),
            LlmResponse::Text("QUESTION: Which modal fields do you need?".to_string()),
        ]);
        let probe = client.clone();
        let mut session = DesignSession::with_config(client, large_config());

        let outcome = session.run_burst("Create a study room").await;

        assert!(matches!(
            outcome,
            BurstOutcome::AwaitingHuman { ref question }
                if question == "Which modal fields do you need?"
        ));
        assert_eq!(session.draft().ruleset.panels[0].buttons.len(), 1);
        assert_eq!(session.observability().tool_calls, 2);
        assert_eq!(probe.seen().len(), 3);
        assert!(probe.seen()[2]
            .iter()
            .filter(|message| message.role == MessageRole::Tool)
            .all(|message| message.content.contains("\"ok\":true")));
    });
}

#[test]
fn prompt_builder_keeps_a_fixed_prefix_and_appends_current_anchors() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "1",
                "add_panel",
                json!({"key":"p","channel":"study_hub","content":"Panel"}),
            )]),
            LlmResponse::Text("QUESTION: Continue?".to_string()),
        ]);
        let probe = client.clone();
        let mut session = DesignSession::with_config(client, large_config());

        assert_eq!(
            session.messages(),
            &[Message::system(DEFAULT_SYSTEM_PROMPT)]
        );

        let outcome = session.run_burst("Build a panel").await;

        assert!(matches!(outcome, BurstOutcome::AwaitingHuman { .. }));
        let seen = probe.seen();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0][0], Message::system(DEFAULT_SYSTEM_PROMPT));
        assert_eq!(seen[0][1], Message::user("Build a panel"));
        assert!(seen[0][2]
            .content
            .starts_with("DRAFT_STATE:{\"revision\":0,"));
        assert!(seen[1].starts_with(&seen[0]));
        assert!(seen[1]
            .last()
            .unwrap()
            .content
            .starts_with("DRAFT_STATE:{\"revision\":1,"));
        assert!(session.messages().starts_with(&seen[1]));
    });
}

#[test]
fn router_sends_exact_structure_and_rule_design_subsets() {
    block_on(async {
        let resolved_client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "panel",
                "add_panel",
                json!({"key":"p","channel":"study_hub","content":"Panel"}),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "button",
                "add_button",
                json!({
                    "panel_key":"p",
                    "label":"Open",
                    "route":{"kind":"static","key":"open"}
                }),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "rule",
                "begin_rule",
                json!({
                    "key":"open_rule",
                    "trigger":{"kind":"button_click","component":"open"}
                }),
            )]),
            LlmResponse::Text("QUESTION: Continue?".to_string()),
        ]);
        let resolved_probe = resolved_client.clone();
        let mut resolved = DesignSession::with_config(resolved_client, large_config());

        assert!(matches!(
            resolved.run_burst("Build it").await,
            BurstOutcome::AwaitingHuman { .. }
        ));
        assert_eq!(
            resolved_probe.seen_tools(),
            vec![
                structure_tool_names(false),
                structure_tool_names(true),
                structure_tool_names(true),
                rule_tool_names(true, false, false, None),
            ]
        );

        let unresolved_client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "rule",
                "begin_rule",
                json!({
                    "key":"missing_button",
                    "trigger":{"kind":"button_click","component":"missing"}
                }),
            )]),
            LlmResponse::Text("QUESTION: Which button?".to_string()),
        ]);
        let unresolved_probe = unresolved_client.clone();
        let mut unresolved = DesignSession::with_config(unresolved_client, large_config());

        assert!(matches!(
            unresolved.run_burst("Build it").await,
            BurstOutcome::AwaitingHuman { .. }
        ));
        assert_eq!(
            unresolved_probe.seen_tools(),
            vec![
                structure_tool_names(false),
                rule_tool_names(false, false, false, None),
            ]
        );
    });
}

#[test]
fn same_batch_add_panel_does_not_unlock_an_unexposed_add_button() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![
                call(
                    "panel",
                    "add_panel",
                    json!({"key":"p","channel":"study_hub","content":"Panel"}),
                ),
                call(
                    "button",
                    "add_button",
                    json!({
                        "panel_key":"p",
                        "label":"Open",
                        "route":{"kind":"static","key":"open"}
                    }),
                ),
            ]),
            LlmResponse::Text("QUESTION: Add the button next?".to_string()),
        ]);
        let probe = client.clone();
        let mut session = DesignSession::with_config(client, large_config());

        let outcome = session.run_burst("Build a panel").await;

        assert!(matches!(outcome, BurstOutcome::AwaitingHuman { .. }));
        assert!(session.draft().ruleset.panels[0].buttons.is_empty());
        assert!(session.messages().iter().any(|message| {
            message.role == MessageRole::Tool
                && message.tool_call_id.as_deref() == Some("button")
                && message
                    .content
                    .contains("TOOL_NOT_AVAILABLE_FOR_DRAFT_STATE")
        }));
        assert_eq!(
            probe.seen_tools(),
            vec![structure_tool_names(false), structure_tool_names(true)]
        );
    });
}

#[test]
fn created_role_unlocks_grant_register_and_validate_on_the_next_call() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "rule",
                "begin_rule",
                json!({
                    "key":"create_role",
                    "trigger":{"kind":"instance_action","action":"create"}
                }),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "role",
                "add_resource_action",
                json!({
                    "rule_key":"create_role",
                    "kind":"create_role",
                    "key":"member",
                    "name":"Member"
                }),
            )]),
            LlmResponse::Text("QUESTION: Grant and register it?".to_string()),
        ]);
        let probe = client.clone();
        let mut session = DesignSession::with_config(client, large_config());

        let outcome = session.run_burst("Create a role").await;

        assert!(matches!(outcome, BurstOutcome::AwaitingHuman { .. }));
        assert_eq!(
            probe.seen_tools(),
            vec![
                structure_tool_names(false),
                rule_tool_names(false, false, false, None),
                rule_tool_names(false, true, true, Some("validate_draft")),
            ]
        );
    });
}

#[test]
fn validate_stays_hidden_until_every_rule_has_an_action() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "rule",
                "begin_rule",
                json!({
                    "key":"join",
                    "trigger":{"kind":"instance_action","action":"join"}
                }),
            )]),
            LlmResponse::ToolCalls(vec![call("validate", "validate_draft", json!({}))]),
            LlmResponse::ToolCalls(vec![call(
                "action",
                "add_interaction_action",
                json!({"rule_key":"join","kind":"defer_ephemeral"}),
            )]),
            LlmResponse::Text("QUESTION: Validate now?".to_string()),
        ]);
        let probe = client.clone();
        let mut session = DesignSession::with_config(client, large_config());

        let outcome = session.run_burst("Build join").await;

        assert!(matches!(outcome, BurstOutcome::AwaitingHuman { .. }));
        assert_eq!(session.observability().validation_failures, 0);
        assert!(session.messages().iter().any(|message| {
            message.role == MessageRole::Tool
                && message.tool_call_id.as_deref() == Some("validate")
                && message
                    .content
                    .contains("TOOL_NOT_AVAILABLE_FOR_DRAFT_STATE")
        }));
        assert_eq!(
            probe.seen_tools(),
            vec![
                structure_tool_names(false),
                rule_tool_names(false, false, false, None),
                rule_tool_names(false, false, false, None),
                rule_tool_names(false, false, false, Some("validate_draft")),
            ]
        );
    });
}

#[test]
fn router_keeps_mutations_available_after_simulation() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call("simulate", "simulate_draft", json!({}))]),
            LlmResponse::Text("QUESTION: Review the result?".to_string()),
        ]);
        let probe = client.clone();
        let mut session = DesignSession::with_config(client, large_config());
        let mut draft = support::golden_draft().await;
        draft.validated_revision = Some(draft.draft_revision);
        *session.draft_mut() = draft;

        let outcome = session.run_burst("Simulate it").await;

        assert!(matches!(outcome, BurstOutcome::AwaitingHuman { .. }));
        assert_eq!(
            probe.seen_tools(),
            vec![simulation_tool_names(), simulation_tool_names()]
        );
    });
}

#[test]
fn failed_simulation_keeps_mutation_tools_available_for_repair() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call("simulate", "simulate_draft", json!({}))]),
            LlmResponse::Text("QUESTION: Should I repair the overwrite?".to_string()),
        ]);
        let probe = client.clone();
        let mut session = DesignSession::with_config(client, large_config());
        let mut draft = support::golden_draft().await;
        let submit_rule = draft
            .ruleset
            .rules
            .iter_mut()
            .find(|rule| rule.key == "submit_room")
            .unwrap();
        let overwrite = submit_rule
            .actions
            .iter()
            .position(|action| {
                matches!(
                    action,
                    ActionSpec::UpsertOverwrite {
                        target: OverwriteTargetSpec::Everyone,
                        ..
                    }
                )
            })
            .unwrap();
        submit_rule.actions.remove(overwrite);
        draft.validated_revision = Some(draft.draft_revision);
        *session.draft_mut() = draft;

        let outcome = session.run_burst("Simulate it").await;

        assert!(matches!(outcome, BurstOutcome::AwaitingHuman { .. }));
        assert_eq!(session.observability().simulation_failures, 1);
        assert_eq!(
            probe.seen_tools(),
            vec![simulation_tool_names(), simulation_tool_names()]
        );
    });
}

#[test]
fn hidden_tool_calls_fail_structurally_before_dispatch() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call("validate", "validate_draft", json!({}))]),
            LlmResponse::Text("QUESTION: Should I add a rule first?".to_string()),
        ]);
        let probe = client.clone();
        let mut session = DesignSession::with_config(client, large_config());

        let outcome = session.run_burst("Validate it").await;

        assert!(matches!(outcome, BurstOutcome::AwaitingHuman { .. }));
        assert_eq!(session.draft().draft_revision, 0);
        assert_eq!(session.observability().validation_failures, 0);
        assert!(session.messages().iter().any(|message| {
            message.role == MessageRole::Tool
                && message
                    .content
                    .contains("TOOL_NOT_AVAILABLE_FOR_DRAFT_STATE")
                && message.content.contains("add_panel, add_modal, begin_rule")
        }));
        assert_eq!(
            probe.seen_tools(),
            vec![structure_tool_names(false), structure_tool_names(false),]
        );
    });
}

#[test]
fn routed_schemas_fit_a_budget_that_the_full_registry_cannot() {
    block_on(async {
        let registry = tool_definitions();
        let structure = structure_tool_names(false);
        let routed = registry
            .iter()
            .filter(|tool| structure.contains(&tool.name))
            .cloned()
            .collect::<Vec<_>>();
        let full_chars = serde_json::to_string(&registry).unwrap().len();
        let routed_chars = serde_json::to_string(&routed).unwrap().len();
        assert!(routed_chars + 2_000 < full_chars);

        let client = ScriptedClient::new(vec![LlmResponse::Text(
            "QUESTION: What should I build?".to_string(),
        )]);
        let probe = client.clone();
        let mut session = DesignSession::with_config(
            client,
            SessionConfig {
                context_char_budget: full_chars,
                ..SessionConfig::default()
            },
        );

        let outcome = session.run_burst("Build").await;

        assert!(matches!(outcome, BurstOutcome::AwaitingHuman { .. }));
        assert_eq!(session.observability().model_calls, 1);
        assert_eq!(probe.seen_tools(), vec![structure_tool_names(false)]);
    });
}

#[test]
fn one_tool_per_model_call_completes_the_full_golden_flow() {
    block_on(async {
        let mut responses = support::golden_calls()
            .into_iter()
            .enumerate()
            .map(|(index, (name, arguments))| {
                LlmResponse::ToolCalls(vec![call(&index.to_string(), name, arguments)])
            })
            .collect::<Vec<_>>();
        responses.push(LlmResponse::ToolCalls(vec![call(
            "validate",
            "validate_draft",
            json!({}),
        )]));
        responses.push(LlmResponse::ToolCalls(vec![call(
            "simulate",
            "simulate_draft",
            json!({}),
        )]));
        responses.push(LlmResponse::Text(
            "DONE: StudyRoom design is complete".to_string(),
        ));
        let client = ScriptedClient::new(responses);
        let probe = client.clone();
        let mut session = DesignSession::with_config(client, long_flow_config());

        let outcome = session.run_burst("Build StudyRoom").await;

        assert!(matches!(outcome, BurstOutcome::Completed { .. }));
        assert_eq!(session.observability().model_calls, 19);
        assert_eq!(session.observability().tool_calls, 18);
        assert_eq!(probe.seen_tools().len(), 19);
        assert!(session
            .messages()
            .iter()
            .filter(|message| !message.tool_calls.is_empty())
            .all(|message| message.tool_calls.len() == 1));
    });
}

#[test]
fn one_tool_per_model_call_builds_and_validates_a_simple_modal() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "modal",
                "add_modal",
                json!({
                    "key":"feedback",
                    "title":"Feedback",
                    "fields":[{
                        "key":"message",
                        "label":"Message",
                        "style":"paragraph",
                        "required":true
                    }]
                }),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "rule",
                "begin_rule",
                json!({
                    "key":"submit_feedback",
                    "trigger":{"kind":"modal_submit","modal":"feedback"}
                }),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "defer",
                "add_interaction_action",
                json!({"rule_key":"submit_feedback","kind":"defer_ephemeral"}),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "edit",
                "add_interaction_action",
                json!({
                    "rule_key":"submit_feedback",
                    "kind":"edit_response",
                    "content":"Thanks for the feedback"
                }),
            )]),
            LlmResponse::ToolCalls(vec![call("validate", "validate_draft", json!({}))]),
            LlmResponse::Text("QUESTION: Should I add an opener?".to_string()),
        ]);
        let probe = client.clone();
        let mut session = DesignSession::with_config(client, long_flow_config());

        let outcome = session.run_burst("Build a feedback modal").await;

        assert!(matches!(outcome, BurstOutcome::AwaitingHuman { .. }));
        assert_eq!(
            session.draft().validated_revision,
            Some(session.draft().draft_revision)
        );
        assert_eq!(session.observability().tool_calls, 5);
        assert_eq!(probe.seen_tools()[0], structure_tool_names(false));
        assert_eq!(
            probe.seen_tools()[2],
            rule_tool_names(false, false, false, None)
        );
        assert_eq!(
            probe.seen_tools()[5],
            rule_tool_names(false, false, false, Some("simulate_draft"))
        );
    });
}

#[test]
fn one_tool_per_model_call_can_add_new_structure_after_a_rule() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "panel-1",
                "add_panel",
                json!({"key":"p1","channel":"study_hub","content":"First"}),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "button-1",
                "add_button",
                json!({
                    "panel_key":"p1",
                    "label":"Open",
                    "route":{"kind":"static","key":"open"}
                }),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "rule",
                "begin_rule",
                json!({
                    "key":"open_rule",
                    "trigger":{"kind":"button_click","component":"open"}
                }),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "modal",
                "add_modal",
                json!({"key":"later_modal","title":"Later","fields":[]}),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "panel-2",
                "add_panel",
                json!({"key":"p2","channel":"study_hub","content":"Second"}),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "button-2",
                "add_button",
                json!({
                    "panel_key":"p2",
                    "label":"Later",
                    "route":{"kind":"static","key":"later"}
                }),
            )]),
            LlmResponse::Text("QUESTION: Continue?".to_string()),
        ]);
        let probe = client.clone();
        let mut session = DesignSession::with_config(client, long_flow_config());

        let outcome = session.run_burst("Build in steps").await;

        assert!(matches!(outcome, BurstOutcome::AwaitingHuman { .. }));
        assert_eq!(session.draft().ruleset.panels.len(), 2);
        assert_eq!(session.draft().ruleset.modals.len(), 1);
        assert_eq!(session.draft().ruleset.panels[1].buttons.len(), 1);
        assert_eq!(
            probe.seen_tools()[3],
            rule_tool_names(true, false, false, None)
        );
        assert_eq!(
            probe.seen_tools()[5],
            rule_tool_names(true, false, false, None)
        );
    });
}

#[test]
fn validated_add_panel_keeps_add_button_available_on_the_next_call() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "panel",
                "add_panel",
                json!({"key":"later","channel":"study_hub","content":"Later"}),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "button",
                "add_button",
                json!({
                    "panel_key":"later",
                    "label":"Open",
                    "route":{"kind":"static","key":"later_open"}
                }),
            )]),
            LlmResponse::Text("QUESTION: Continue?".to_string()),
        ]);
        let probe = client.clone();
        let mut session = DesignSession::with_config(client, long_flow_config());
        let mut draft = Draft::new();
        draft.ruleset.rules.push(InteractionRule {
            key: "existing".to_string(),
            trigger: TriggerSpec::InstanceAction {
                action: "existing".to_string(),
            },
            actions: vec![ActionSpec::DeferEphemeral],
        });
        draft.draft_revision = 1;
        draft.validated_revision = Some(1);
        *session.draft_mut() = draft;

        let outcome = session.run_burst("Add another panel").await;

        assert!(matches!(outcome, BurstOutcome::AwaitingHuman { .. }));
        let panel = session
            .draft()
            .ruleset
            .panels
            .iter()
            .find(|panel| panel.key == "later")
            .unwrap();
        assert_eq!(panel.buttons.len(), 1);
        assert_eq!(
            probe.seen_tools()[0],
            rule_tool_names(false, false, false, Some("simulate_draft"))
        );
        assert_eq!(
            probe.seen_tools()[1],
            rule_tool_names(true, false, false, Some("validate_draft"))
        );
    });
}

#[test]
fn simulated_draft_accepts_a_mutation_after_a_human_question() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::Text("QUESTION: What should change?".to_string()),
            LlmResponse::ToolCalls(vec![call(
                "panel",
                "add_panel",
                json!({"key":"later","channel":"study_hub","content":"Later"}),
            )]),
            LlmResponse::Text("QUESTION: Anything else?".to_string()),
        ]);
        let probe = client.clone();
        let mut session = DesignSession::with_config(client, long_flow_config());
        let mut draft = support::golden_draft().await;
        draft.validated_revision = Some(draft.draft_revision);
        draft.simulated_revision = Some(draft.draft_revision);
        *session.draft_mut() = draft;

        assert!(matches!(
            session.run_burst("Review it").await,
            BurstOutcome::AwaitingHuman { .. }
        ));
        assert!(matches!(
            session.run_burst("Add another panel").await,
            BurstOutcome::AwaitingHuman { .. }
        ));

        assert!(session
            .draft()
            .ruleset
            .panels
            .iter()
            .any(|panel| panel.key == "later"));
        assert_eq!(probe.seen_tools()[0], simulation_tool_names());
        assert_eq!(probe.seen_tools()[1], simulation_tool_names());
        assert_eq!(probe.seen_tools()[2], rule_design_tool_names());
    });
}

#[test]
fn same_batch_mutation_blocks_simulate_after_state_changes() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![
                call(
                    "panel",
                    "add_panel",
                    json!({"key":"later","channel":"study_hub","content":"Later"}),
                ),
                call("simulate", "simulate_draft", json!({})),
            ]),
            LlmResponse::Text("QUESTION: Should I validate again?".to_string()),
        ]);
        let mut session = DesignSession::with_config(client, long_flow_config());
        let mut draft = support::golden_draft().await;
        draft.validated_revision = Some(draft.draft_revision);
        *session.draft_mut() = draft;

        let outcome = session.run_burst("Change and simulate").await;

        assert!(matches!(outcome, BurstOutcome::AwaitingHuman { .. }));
        assert_eq!(session.draft().validated_revision, None);
        assert_eq!(session.draft().simulated_revision, None);
        assert_eq!(session.observability().simulation_failures, 0);
        assert!(session.messages().iter().any(|message| {
            message.role == MessageRole::Tool
                && message.tool_call_id.as_deref() == Some("simulate")
                && message
                    .content
                    .contains("TOOL_NOT_AVAILABLE_FOR_DRAFT_STATE")
        }));
    });
}

#[test]
fn same_batch_newly_opened_tool_stays_blocked_when_not_exposed() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![
                call("validate", "validate_draft", json!({})),
                call("simulate", "simulate_draft", json!({})),
            ]),
            LlmResponse::Text("QUESTION: Simulate on the next call?".to_string()),
        ]);
        let mut session = DesignSession::with_config(client, long_flow_config());
        *session.draft_mut() = support::golden_draft().await;

        let outcome = session.run_burst("Validate and simulate").await;

        assert!(matches!(outcome, BurstOutcome::AwaitingHuman { .. }));
        assert_eq!(
            session.draft().validated_revision,
            Some(session.draft().draft_revision)
        );
        assert_eq!(session.draft().simulated_revision, None);
        assert!(session.messages().iter().any(|message| {
            message.role == MessageRole::Tool
                && message.tool_call_id.as_deref() == Some("simulate")
                && message
                    .content
                    .contains("TOOL_NOT_AVAILABLE_FOR_DRAFT_STATE")
        }));
    });
}

#[test]
fn failed_tool_stops_the_remaining_batch() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![
                call(
                    "1",
                    "add_button",
                    json!({
                        "panel_key":"missing",
                        "label":"Open",
                        "route":{"kind":"static","key":"open"}
                    }),
                ),
                call(
                    "2",
                    "add_modal",
                    json!({"key":"should_not_exist","title":"Modal","fields":[]}),
                ),
            ]),
            LlmResponse::Text("QUESTION: Should I add the panel first?".to_string()),
        ]);
        let mut session = DesignSession::with_config(client, large_config());

        let outcome = session.run_burst("Continue").await;

        assert!(matches!(outcome, BurstOutcome::AwaitingHuman { .. }));
        assert!(session.draft().ruleset.modals.is_empty());
        assert_eq!(session.observability().tool_calls, 1);
        assert!(session.messages().iter().any(|message| {
            message.role == MessageRole::Tool
                && message
                    .content
                    .contains("NOT_EXECUTED_AFTER_PREVIOUS_FAILURE")
        }));
    });
}

#[test]
fn repeated_failure_signatures_are_counted_across_model_calls() {
    block_on(async {
        let invalid = || {
            call(
                "missing-panel",
                "add_button",
                json!({
                    "panel_key":"missing",
                    "label":"Open",
                    "route":{"kind":"static","key":"open"}
                }),
            )
        };
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![invalid()]),
            LlmResponse::ToolCalls(vec![invalid()]),
            LlmResponse::Text("QUESTION: Should I create the panel?".to_string()),
        ]);
        let mut session = DesignSession::with_config(client, large_config());

        let outcome = session.run_burst("Continue").await;

        assert!(matches!(outcome, BurstOutcome::AwaitingHuman { .. }));
        assert_eq!(session.observability().repeated_errors, 1);
        assert_eq!(session.observability().failure_signatures.len(), 1);
        assert_eq!(
            session.observability().failure_signatures.values().next(),
            Some(&2)
        );
    });
}

#[test]
fn question_and_current_done_end_the_burst() {
    block_on(async {
        let question_client = ScriptedClient::new(vec![LlmResponse::Text(
            "QUESTION: What should the room be called?".to_string(),
        )]);
        let mut question_session = DesignSession::with_config(question_client, large_config());
        assert!(matches!(
            question_session.run_burst("Build it").await,
            BurstOutcome::AwaitingHuman { ref question }
                if question == "What should the room be called?"
        ));
        assert_eq!(question_session.observability().clarification_count, 1);

        let mut responses = support::golden_calls()
            .into_iter()
            .enumerate()
            .map(|(index, (name, arguments))| {
                LlmResponse::ToolCalls(vec![call(&index.to_string(), name, arguments)])
            })
            .collect::<Vec<_>>();
        responses.push(LlmResponse::ToolCalls(vec![call(
            "validate",
            "validate_draft",
            json!({}),
        )]));
        responses.push(LlmResponse::ToolCalls(vec![call(
            "simulate",
            "simulate_draft",
            json!({}),
        )]));
        responses.push(LlmResponse::Text(
            "DONE: StudyRoom design is complete".to_string(),
        ));
        let expected = support::golden_draft().await;
        let done_client = ScriptedClient::new(responses);
        let mut done_session = DesignSession::with_config(done_client, long_flow_config());

        let outcome = done_session.run_burst("Build StudyRoom").await;

        assert!(matches!(
            outcome,
            BurstOutcome::Completed { ref summary }
                if summary == "StudyRoom design is complete"
        ));
        assert_eq!(
            done_session.draft().simulated_revision,
            Some(done_session.draft().draft_revision)
        );
        assert_eq!(done_session.draft().ruleset, expected.ruleset);
        assert!(done_session.observability().distinct_mutation_tools.len() >= 2);
    });
}

#[test]
fn stale_done_and_unstructured_prose_use_the_nudge_protocol() {
    block_on(async {
        let stale_client = ScriptedClient::new(vec![
            LlmResponse::Text("DONE: finished".to_string()),
            LlmResponse::Text("QUESTION: May I validate first?".to_string()),
        ]);
        let mut stale_session = DesignSession::with_config(stale_client, large_config());
        let stale_outcome = stale_session.run_burst("Build it").await;
        assert!(matches!(stale_outcome, BurstOutcome::AwaitingHuman { .. }));
        assert_eq!(stale_session.observability().nudge_count, 1);

        let prose_client = ScriptedClient::new(vec![
            LlmResponse::Text("I will think about it".to_string()),
            LlmResponse::Text("Here is a prose plan".to_string()),
        ]);
        let mut prose_session = DesignSession::with_config(prose_client, large_config());
        let prose_outcome = prose_session.run_burst("Build it").await;
        let BurstOutcome::Halted(report) = prose_outcome else {
            panic!("expected halt")
        };
        assert_eq!(report.code, "UNSTRUCTURED_MODEL_TEXT");
        assert_eq!(report.message, "Here is a prose plan");
        assert_eq!(prose_session.observability().nudge_count, 1);
        assert_eq!(prose_session.observability().model_calls, 2);
    });
}

#[test]
fn session_observes_revision_invalidation_and_blocks_premature_simulate() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call("1", "simulate_draft", json!({}))]),
            LlmResponse::Text("QUESTION: Should I validate?".to_string()),
        ]);
        let mut session = DesignSession::with_config(client, large_config());
        let outcome = session.run_burst("Simulate").await;
        assert!(matches!(outcome, BurstOutcome::AwaitingHuman { .. }));
        assert_eq!(session.observability().simulation_failures, 0);
        assert!(session.messages().iter().any(|message| message
            .content
            .contains("TOOL_NOT_AVAILABLE_FOR_DRAFT_STATE")));

        let mutation_client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "1",
                "add_panel",
                json!({"key":"p","channel":"study_hub","content":"Panel"}),
            )]),
            LlmResponse::Text("QUESTION: Continue?".to_string()),
        ]);
        let mut mutation_session = DesignSession::with_config(mutation_client, large_config());
        mutation_session.draft_mut().validated_revision = Some(0);
        mutation_session.draft_mut().simulated_revision = Some(0);
        mutation_session.run_burst("Change it").await;
        assert_eq!(mutation_session.draft().validated_revision, None);
        assert_eq!(mutation_session.draft().simulated_revision, None);
    });
}

#[test]
fn model_tool_and_gate_failure_limits_halt_the_session() {
    block_on(async {
        let model_client = ScriptedClient::new(vec![LlmResponse::ToolCalls(vec![call(
            "1",
            "add_panel",
            json!({"key":"p","channel":"study_hub","content":"Panel"}),
        )])]);
        let mut model_session = DesignSession::with_config(
            model_client,
            SessionConfig {
                max_model_calls: 1,
                context_char_budget: 100_000,
                ..SessionConfig::default()
            },
        );
        let BurstOutcome::Halted(model_report) = model_session.run_burst("Build").await else {
            panic!("expected model limit")
        };
        assert_eq!(model_report.exhausted_limit, Some(LimitKind::ModelCalls));

        let tool_client = ScriptedClient::new(vec![LlmResponse::ToolCalls(vec![
            call(
                "1",
                "add_panel",
                json!({"key":"p1","channel":"study_hub","content":"Panel"}),
            ),
            call(
                "2",
                "add_panel",
                json!({"key":"p2","channel":"study_hub","content":"Panel"}),
            ),
        ])]);
        let mut tool_session = DesignSession::with_config(
            tool_client,
            SessionConfig {
                max_tool_calls: 1,
                context_char_budget: 100_000,
                ..SessionConfig::default()
            },
        );
        let BurstOutcome::Halted(tool_report) = tool_session.run_burst("Build").await else {
            panic!("expected tool limit")
        };
        assert_eq!(tool_report.exhausted_limit, Some(LimitKind::ToolCalls));
        assert_eq!(tool_session.draft().ruleset.panels.len(), 1);

        let gate_client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "rule",
                "begin_rule",
                json!({
                    "key":"broken",
                    "trigger":{"kind":"button_click","component":"missing"}
                }),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "action",
                "add_interaction_action",
                json!({
                    "rule_key":"broken",
                    "kind":"open_modal",
                    "modal":"missing"
                }),
            )]),
            LlmResponse::ToolCalls(vec![call("validate", "validate_draft", json!({}))]),
        ]);
        let mut gate_session = DesignSession::with_config(
            gate_client,
            SessionConfig {
                max_gate_failures: 1,
                context_char_budget: 100_000,
                ..SessionConfig::default()
            },
        );
        let BurstOutcome::Halted(gate_report) = gate_session.run_burst("Simulate").await else {
            panic!("expected gate limit")
        };
        assert_eq!(gate_report.exhausted_limit, Some(LimitKind::GateFailures));
        assert_eq!(gate_report.observability.validation_failures, 1);
    });
}

#[test]
fn context_trims_old_tool_results_and_halts_when_the_anchor_cannot_fit() {
    block_on(async {
        let tools = tool_definitions();
        let structure_names = structure_tool_names(true);
        let routed_tools = tools
            .iter()
            .filter(|tool| structure_names.contains(&tool.name))
            .cloned()
            .collect::<Vec<_>>();
        let tool_chars = serde_json::to_string(&routed_tools).unwrap().len();
        assert!(
            tool_chars + DEFAULT_SYSTEM_PROMPT.len() < 16_000,
            "tool schema and prompt chars: {}",
            tool_chars + DEFAULT_SYSTEM_PROMPT.len()
        );

        let calls = (0..8)
            .map(|index| {
                call(
                    &index.to_string(),
                    "add_panel",
                    json!({
                        "key":format!("panel_{index}"),
                        "channel":"study_hub",
                        "content":"Panel"
                    }),
                )
            })
            .collect();
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(calls),
            LlmResponse::Text("QUESTION: Continue?".to_string()),
        ]);
        let probe = client.clone();
        let mut session = DesignSession::with_config(
            client,
            SessionConfig {
                context_char_budget: tool_chars + DEFAULT_SYSTEM_PROMPT.len() + 1_300,
                ..SessionConfig::default()
            },
        );
        let outcome = session.run_burst("Build panels").await;
        assert!(matches!(outcome, BurstOutcome::AwaitingHuman { .. }));
        let seen = probe.seen();
        let second_tool_results = seen[1]
            .iter()
            .filter(|message| message.role == MessageRole::Tool)
            .count();
        assert!(second_tool_results < 8);
        assert_eq!(seen[1][0].role, MessageRole::System);
        assert_eq!(seen[1][0].content, DEFAULT_SYSTEM_PROMPT);
        assert!(seen[1]
            .last()
            .unwrap()
            .content
            .starts_with("DRAFT_STATE:{\"revision\":8,"));
        assert_complete_tool_pairs(&seen[1]);
        assert_eq!(
            session
                .messages()
                .iter()
                .filter(|message| message.role == MessageRole::Tool)
                .count(),
            8
        );

        let impossible_client = ScriptedClient::new(vec![]);
        let mut impossible = DesignSession::with_config(
            impossible_client,
            SessionConfig {
                context_char_budget: 1,
                ..SessionConfig::default()
            },
        );
        let BurstOutcome::Halted(report) = impossible.run_burst("Build").await else {
            panic!("expected context halt")
        };
        assert_eq!(report.exhausted_limit, Some(LimitKind::ContextChars));
        assert_eq!(impossible.observability().model_calls, 0);
    });
}
