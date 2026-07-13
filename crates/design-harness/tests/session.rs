mod support;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use automation_state::{ActionSpec, InteractionRule, OverwriteTargetSpec, TriggerSpec};
use design_harness::{
    dispatch_tool, tool_definitions, AdaptivePhase, BurstOutcome, DesignSession, Draft, LimitKind,
    LlmClient, LlmError, LlmResponse, Message, MessageRole, RepairState, SessionConfig,
    SessionSnapshotError, StructuredError, ToolCall, TurnPhase, DEFAULT_SYSTEM_PROMPT,
    SESSION_SNAPSHOT_VERSION,
};
use futures::executor::block_on;
use serde_json::{json, Value};

type SeenToolParameters = Vec<Vec<(String, Value)>>;

#[test]
fn adaptive_system_prompt_assigns_automatic_gates_to_the_harness() {
    assert!(DEFAULT_SYSTEM_PROMPT.contains(
        "The harness then automatically runs requested validation, harness-selected simulation, and preview steps"
    ));
    assert!(DEFAULT_SYSTEM_PROMPT
        .contains("When finish_turn is the only available tool, call it with kind ready"));
    assert!(!DEFAULT_SYSTEM_PROMPT.contains("render_preview for a validated preview"));
}

#[derive(Clone)]
struct ScriptedClient {
    responses: Arc<Mutex<VecDeque<Result<LlmResponse, LlmError>>>>,
    seen: Arc<Mutex<Vec<Vec<Message>>>>,
    seen_tools: Arc<Mutex<Vec<Vec<String>>>>,
    seen_tool_parameters: Arc<Mutex<SeenToolParameters>>,
}

impl ScriptedClient {
    fn new(responses: Vec<LlmResponse>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses.into_iter().map(Ok).collect())),
            seen: Arc::new(Mutex::new(Vec::new())),
            seen_tools: Arc::new(Mutex::new(Vec::new())),
            seen_tool_parameters: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn seen(&self) -> Vec<Vec<Message>> {
        self.seen.lock().unwrap().clone()
    }

    fn seen_tools(&self) -> Vec<Vec<String>> {
        self.seen_tools.lock().unwrap().clone()
    }

    fn seen_tool_parameters(&self) -> Vec<Vec<(String, Value)>> {
        self.seen_tool_parameters.lock().unwrap().clone()
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
        self.seen_tool_parameters.lock().unwrap().push(
            tools
                .iter()
                .map(|tool| (tool.name.clone(), tool.parameters.clone()))
                .collect(),
        );
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

async fn fixable_validation_draft() -> Draft {
    let mut draft = Draft::new();
    for (name, arguments) in [
        (
            "add_panel",
            json!({"key":"launcher","channel":"study_hub","content":"Launch"}),
        ),
        (
            "add_modal",
            json!({"key":"room_modal","title":"Room","fields":[]}),
        ),
        (
            "begin_rule",
            json!({
                "key":"open_room",
                "trigger_kind":"button_click",
                "trigger_ref":"open_room"
            }),
        ),
        (
            "add_interaction_action",
            json!({"rule_key":"open_room","kind":"open_modal","modal":"room_modal"}),
        ),
    ] {
        let result = dispatch_tool(&mut draft, name, &arguments.to_string()).await;
        assert!(result.is_ok(), "{}", result.as_json());
    }
    draft
}

async fn fixable_simulation_draft() -> Draft {
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
    draft
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
            BurstOutcome::NeedsInput { ref question }
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
fn prompt_builder_fast_path_sends_the_exact_append_only_canonical_prefix() {
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

        assert!(matches!(outcome, BurstOutcome::NeedsInput { .. }));
        let seen = probe.seen();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0][0], Message::system(DEFAULT_SYSTEM_PROMPT));
        assert_eq!(seen[0][1], Message::user("Build a panel"));
        assert!(seen[0][2]
            .content
            .starts_with("DRAFT_STATE:{\"revision\":0,"));
        assert!(seen[1].starts_with(&seen[0]));
        assert_eq!(
            seen[1]
                .iter()
                .filter(|message| message.content.starts_with("DRAFT_STATE:"))
                .count(),
            2
        );
        assert!(seen[1]
            .last()
            .unwrap()
            .content
            .starts_with("DRAFT_STATE:{\"revision\":1,"));
        assert_eq!(seen[1], session.messages()[..session.messages().len() - 1]);
        assert_eq!(
            session
                .messages()
                .iter()
                .filter(|message| message.content.starts_with("DRAFT_STATE:"))
                .count(),
            2
        );
        assert_complete_tool_pairs(&seen[1]);
    });
}

#[test]
fn snapshot_json_roundtrip_restores_and_continues_the_session() {
    block_on(async {
        let first_client = ScriptedClient::new(vec![LlmResponse::Text(
            "QUESTION: Which panel should I create?".to_string(),
        )]);
        let mut first = DesignSession::with_config(first_client, large_config());
        assert!(matches!(
            first.run_burst("Start a panel").await,
            BurstOutcome::NeedsInput { .. }
        ));
        let canonical = first.messages().to_vec();
        let json = serde_json::to_string(&first.snapshot()).unwrap();
        let snapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(first.snapshot().schema_version, SESSION_SNAPSHOT_VERSION);
        assert_eq!(first.turn_state().unwrap().sequence, 1);
        assert_eq!(first.turn_state().unwrap().phase, TurnPhase::NeedsInput);
        let second_client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "panel",
                "add_panel",
                json!({"key":"welcome","channel":"study_hub","content":"Welcome"}),
            )]),
            LlmResponse::Text("QUESTION: Add a button?".to_string()),
        ]);
        let mut restored = DesignSession::restore(second_client, large_config(), snapshot).unwrap();

        assert_eq!(restored.messages(), canonical);
        assert!(matches!(
            restored.run_burst("Create a welcome panel").await,
            BurstOutcome::NeedsInput { .. }
        ));
        assert_eq!(restored.draft().ruleset.panels[0].key, "welcome");
        assert_eq!(restored.observability().model_calls, 3);
        assert_eq!(restored.turn_state().unwrap().sequence, 2);
        assert_eq!(restored.turn_state().unwrap().phase, TurnPhase::NeedsInput);
        assert!(restored.messages().starts_with(&canonical));
    });
}

#[test]
fn snapshot_restore_rejects_versions_and_broken_invariants() {
    let session = DesignSession::new(());
    let mut future = session.snapshot();
    future.schema_version += 1;
    assert!(matches!(
        DesignSession::restore((), SessionConfig::default(), future),
        Err(SessionSnapshotError::UnsupportedVersion { .. })
    ));

    let mut broken = session.snapshot();
    broken.messages.clear();
    assert!(matches!(
        DesignSession::restore((), SessionConfig::default(), broken),
        Err(SessionSnapshotError::InvalidInvariant { .. })
    ));

    let mut invalid_role_fields = session.snapshot();
    invalid_role_fields.messages[0]
        .tool_calls
        .push(call("system-call", "add_panel", json!({})));
    assert!(matches!(
        DesignSession::restore((), SessionConfig::default(), invalid_role_fields),
        Err(SessionSnapshotError::InvalidInvariant { .. })
    ));

    let mut malformed_anchor = session.snapshot();
    malformed_anchor
        .messages
        .push(Message::system("DRAFT_STATE:not-json"));
    assert!(matches!(
        DesignSession::restore((), SessionConfig::default(), malformed_anchor),
        Err(SessionSnapshotError::InvalidInvariant { .. })
    ));

    let mut duplicate_ids = session.snapshot();
    duplicate_ids
        .messages
        .push(Message::assistant_tool_calls(vec![
            call("duplicate", "add_panel", json!({})),
            call("duplicate", "add_modal", json!({})),
        ]));
    duplicate_ids
        .messages
        .push(Message::tool("duplicate", json!({}).to_string()));
    assert!(matches!(
        DesignSession::restore((), SessionConfig::default(), duplicate_ids),
        Err(SessionSnapshotError::InvalidInvariant { .. })
    ));

    let mut empty_id = session.snapshot();
    empty_id
        .messages
        .push(Message::assistant_tool_calls(vec![call(
            "",
            "add_panel",
            json!({}),
        )]));
    empty_id
        .messages
        .push(Message::tool("", json!({}).to_string()));
    assert!(matches!(
        DesignSession::restore((), SessionConfig::default(), empty_id),
        Err(SessionSnapshotError::InvalidInvariant { .. })
    ));

    let client = ScriptedClient::new(vec![LlmResponse::Text("QUESTION: Continue?".to_string())]);
    let mut session = DesignSession::with_config(client, large_config());
    assert!(matches!(
        block_on(session.run_burst("Start")),
        BurstOutcome::NeedsInput { .. }
    ));
    let mut invalid_turn = session.snapshot();
    invalid_turn.turn_state.as_mut().unwrap().sequence = 0;
    assert!(matches!(
        DesignSession::restore((), SessionConfig::default(), invalid_turn),
        Err(SessionSnapshotError::InvalidInvariant { .. })
    ));
}

#[test]
fn expanded_anchor_carries_structure_aliases_and_failure_history() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "missing-panel",
                "add_button",
                json!({
                    "panel_key":"missing",
                    "label":"Open",
                    "route":{"kind":"static","key":"open"}
                }),
            )]),
            LlmResponse::Text("QUESTION: Should I repair the panel reference?".to_string()),
        ]);
        let probe = client.clone();
        let mut session = DesignSession::with_config(client, large_config());
        *session.draft_mut() = support::golden_draft().await;

        assert!(matches!(
            session.run_burst("Keep the StudyRoom design").await,
            BurstOutcome::NeedsInput { .. }
        ));
        let seen = probe.seen();
        let anchor = seen[1]
            .last()
            .unwrap()
            .content
            .strip_prefix("DRAFT_STATE:")
            .unwrap();
        let state: Value = serde_json::from_str(anchor).unwrap();

        assert_eq!(state["panels"][0]["key"], "study_panel");
        assert_eq!(state["modals"][0]["key"], "study_modal");
        assert_eq!(state["modals"][0]["fields"][0], "room_name");
        assert!(state["rules"].as_array().unwrap().iter().any(|rule| {
            rule["key"] == "submit_room" && rule["trigger"] == "modal_submit:study_modal"
        }));
        assert!(state["created_aliases"]["roles"]
            .as_array()
            .unwrap()
            .contains(&json!("member_role")));
        assert!(state["created_aliases"]["channels"]
            .as_array()
            .unwrap()
            .contains(&json!("room_channel")));
        assert!(state["created_aliases"]["messages"]
            .as_array()
            .unwrap()
            .contains(&json!("welcome_panel")));
        assert!(state["created_aliases"]["instances"]
            .as_array()
            .unwrap()
            .contains(&json!("study_instance")));
        assert_eq!(
            state["failure_signatures"]["PANEL_NOT_FOUND@panel.missing"],
            1
        );
        assert_eq!(state["last_error"]["code"], "PANEL_NOT_FOUND");
        assert_eq!(state["last_error"]["location"], "panel.missing");
        assert_eq!(
            state["last_error"]["hint"],
            "Call add_panel before add_button"
        );
        assert!(state["recent_human_intent"].as_array().unwrap().is_empty());
        assert_eq!(state["current_human_intent"], "Keep the StudyRoom design");
    });
}

#[test]
fn argument_failure_gets_one_same_tool_repair_with_exact_schema() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "bad-panel",
                "add_panel",
                json!({"key":"welcome","channel":"study_hub"}),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "fixed-panel",
                "add_panel",
                json!({"key":"welcome","channel":"study_hub","content":"Welcome"}),
            )]),
            LlmResponse::Text("QUESTION: Add a button?".to_string()),
        ]);
        let probe = client.clone();
        let mut session = DesignSession::with_config(client, large_config());

        let outcome = session.run_burst("Build a welcome panel").await;

        assert!(matches!(outcome, BurstOutcome::NeedsInput { .. }));
        assert_eq!(session.draft().ruleset.panels[0].key, "welcome");
        assert_eq!(session.observability().repair_attempts, 1);
        assert_eq!(session.observability().repair_successes, 1);
        assert_eq!(session.observability().repair_failures, 0);
        assert!(session.snapshot().repair_state.is_none());
        assert!(session.snapshot().last_error.is_none());
        assert_eq!(probe.seen_tools()[1], names(&["add_panel"]));
        let add_panel_schema = tool_definitions()
            .into_iter()
            .find(|tool| tool.name == "add_panel")
            .unwrap()
            .parameters;
        let directive_index = session
            .messages()
            .iter()
            .position(|message| message.content.starts_with("REPAIR_REQUIRED:"))
            .unwrap();
        assert_eq!(
            session.messages()[directive_index - 1].role,
            MessageRole::Tool
        );
        let directive: Value = serde_json::from_str(
            session.messages()[directive_index]
                .content
                .strip_prefix("REPAIR_REQUIRED:")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(directive["attempts_remaining"], 1);
        assert_eq!(directive["original"]["tool"]["name"], "add_panel");
        assert_eq!(
            directive["original"]["tool"]["arguments"],
            json!({"key":"welcome","channel":"study_hub"}).to_string()
        );
        assert_eq!(directive["expected_argument_schema"], add_panel_schema);
        assert_eq!(directive["allowed_repair_tools"], json!(["add_panel"]));
        assert_eq!(
            probe.seen_tool_parameters()[1],
            vec![("add_panel".to_string(), add_panel_schema)]
        );
        let seen = probe.seen();
        let repair_anchor = seen[1].last().unwrap().content.as_str();
        let memory: Value =
            serde_json::from_str(repair_anchor.strip_prefix("DRAFT_STATE:").unwrap()).unwrap();
        assert_eq!(memory["current_human_intent"], "Build a welcome panel");
        assert_eq!(memory["repair_state"]["stage"], "awaiting_attempt");
        assert_eq!(memory["repair_state"]["kind"], "arguments");
        assert_eq!(memory["repair_state"]["original_tool"], "add_panel");
        assert_eq!(
            memory["repair_state"]["error"]["code"],
            "MISSING_REQUIRED_FIELD"
        );
        assert_eq!(
            memory["repair_state"]["error"]["location"],
            "tool.add_panel.arguments.content"
        );
        assert_eq!(
            memory["repair_state"]["allowed_tools"],
            json!(["add_panel"])
        );
        assert_eq!(
            memory["repair_state"]["verification_path"],
            json!(["add_panel"])
        );
        assert_eq!(memory["repair_state"]["attempts_remaining"], 1);
        assert_eq!(memory["repair_state"]["root_revision"], 0);
        let repair_memory = memory["repair_state"].as_object().unwrap();
        assert!(!repair_memory.contains_key("original_call"));
        assert!(!repair_memory.contains_key("arguments"));
        assert!(!repair_memory.contains_key("expected_argument_schema"));
        assert!(!repair_memory.contains_key("ticket"));
        assert!(!repair_anchor.contains("expected_argument_schema"));
        assert!(
            !repair_anchor.contains(&json!({"key":"welcome","channel":"study_hub"}).to_string())
        );
    });
}

#[test]
fn anchor_bounds_error_memory_on_character_boundaries_without_truncating_snapshot() {
    block_on(async {
        let message = "가".repeat(500);
        let hint = "나".repeat(500);
        let mut snapshot = DesignSession::new(()).snapshot();
        snapshot.last_error = Some(StructuredError::new(
            "LONG_ERROR",
            "test.long_error",
            message.clone(),
            hint.clone(),
        ));
        let client =
            ScriptedClient::new(vec![LlmResponse::Text("QUESTION: Continue?".to_string())]);
        let probe = client.clone();
        let mut session = DesignSession::restore(client, large_config(), snapshot).unwrap();

        assert!(matches!(
            session.run_burst("Continue the design").await,
            BurstOutcome::NeedsInput { .. }
        ));

        let seen = probe.seen();
        let anchor = seen[0]
            .last()
            .unwrap()
            .content
            .strip_prefix("DRAFT_STATE:")
            .unwrap();
        let memory: Value = serde_json::from_str(anchor).unwrap();
        let compact_message = memory["last_error"]["message"].as_str().unwrap();
        let compact_hint = memory["last_error"]["hint"].as_str().unwrap();
        assert_eq!(compact_message.chars().count(), 361);
        assert_eq!(compact_hint.chars().count(), 361);
        assert!(compact_message.ends_with('…'));
        assert!(compact_hint.ends_with('…'));

        let persisted = session.snapshot();
        assert_eq!(persisted.last_error.as_ref().unwrap().message, message);
        assert_eq!(persisted.last_error.as_ref().unwrap().hint, hint);
        let round_trip: design_harness::SessionSnapshot =
            serde_json::from_str(&serde_json::to_string(&persisted).unwrap()).unwrap();
        DesignSession::restore((), large_config(), round_trip).unwrap();
    });
}

#[test]
fn second_argument_failure_halts_after_two_model_calls_and_human_can_resume() {
    block_on(async {
        let bad = || {
            call(
                "bad-panel",
                "add_panel",
                json!({"key":"welcome","channel":"study_hub"}),
            )
        };
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![bad()]),
            LlmResponse::ToolCalls(vec![bad()]),
        ]);
        let mut session = DesignSession::with_config(client, large_config());

        let BurstOutcome::Halted(report) = session.run_burst("Build a panel").await else {
            panic!("expected repair halt")
        };

        assert_eq!(report.code, "REPAIR_ATTEMPT_FAILED");
        assert_eq!(session.observability().model_calls, 2);
        assert_eq!(session.observability().repair_attempts, 1);
        assert_eq!(session.observability().repair_failures, 1);
        assert!(session.draft().ruleset.panels.is_empty());
        let snapshot_json = serde_json::to_string(&session.snapshot()).unwrap();
        let snapshot_value: Value = serde_json::from_str(&snapshot_json).unwrap();
        assert_eq!(
            snapshot_value["repair_state"]["ticket"]["original_call"]["arguments"],
            json!({"key":"welcome","channel":"study_hub"}).to_string()
        );
        assert!(snapshot_value["repair_state"]["ticket"]["expected_argument_schema"].is_object());
        let snapshot: design_harness::SessionSnapshot =
            serde_json::from_str(&snapshot_json).unwrap();
        let resume_client = ScriptedClient::new(vec![LlmResponse::Text(
            "QUESTION: What content should it use?".to_string(),
        )]);
        let mut restored = DesignSession::restore(resume_client, large_config(), snapshot).unwrap();

        assert!(matches!(
            restored.run_burst("Let me choose the content").await,
            BurstOutcome::NeedsInput { .. }
        ));
        assert!(restored.snapshot().repair_state.is_none());
        assert_eq!(restored.observability().repair_escalations, 1);
    });
}

#[test]
fn multi_call_repair_dispatches_nothing_and_keeps_complete_pairs() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "bad-panel",
                "add_panel",
                json!({"key":"welcome","channel":"study_hub"}),
            )]),
            LlmResponse::ToolCalls(vec![
                call(
                    "repair-one",
                    "add_panel",
                    json!({"key":"one","channel":"study_hub","content":"One"}),
                ),
                call(
                    "repair-two",
                    "add_panel",
                    json!({"key":"two","channel":"study_hub","content":"Two"}),
                ),
            ]),
        ]);
        let mut session = DesignSession::with_config(client, large_config());

        let BurstOutcome::Halted(report) = session.run_burst("Build a panel").await else {
            panic!("expected repair halt")
        };

        assert_eq!(report.code, "REPAIR_ATTEMPT_FAILED");
        assert!(session.draft().ruleset.panels.is_empty());
        assert_eq!(session.observability().tool_calls, 1);
        assert_eq!(session.observability().repair_attempts, 1);
        assert_eq!(session.observability().repair_failures, 1);
        for id in ["repair-one", "repair-two"] {
            assert!(session.messages().iter().any(|message| {
                message.role == MessageRole::Tool
                    && message.tool_call_id.as_deref() == Some(id)
                    && message.content.contains("REPAIR_RESPONSE_REJECTED")
            }));
        }
        assert_complete_tool_pairs(session.messages());
        assert!(DesignSession::restore((), large_config(), session.snapshot()).is_ok());
    });
}

#[test]
fn malformed_text_empty_name_and_wrong_tool_repairs_halt_without_dispatch() {
    block_on(async {
        let cases = vec![
            LlmResponse::ToolCalls(Vec::new()),
            LlmResponse::Text("DONE: repaired".to_string()),
            LlmResponse::Text("I repaired it".to_string()),
            LlmResponse::ToolCalls(vec![call(
                "wrong-tool",
                "add_modal",
                json!({"key":"m","title":"M","fields":[]}),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "empty-name",
                "",
                json!({"key":"p","channel":"study_hub","content":"P"}),
            )]),
        ];
        for response in cases {
            let client = ScriptedClient::new(vec![
                LlmResponse::ToolCalls(vec![call(
                    "bad-panel",
                    "add_panel",
                    json!({"key":"welcome","channel":"study_hub"}),
                )]),
                response,
            ]);
            let mut session = DesignSession::with_config(client, large_config());

            let BurstOutcome::Halted(report) = session.run_burst("Build a panel").await else {
                panic!("expected malformed repair halt")
            };

            assert_eq!(report.code, "REPAIR_ATTEMPT_FAILED");
            assert_eq!(session.observability().tool_calls, 1);
            assert_eq!(session.observability().repair_attempts, 1);
            assert_eq!(session.observability().repair_failures, 1);
            assert!(session.draft().ruleset.panels.is_empty());
            assert_complete_tool_pairs(session.messages());
            assert!(DesignSession::restore((), large_config(), session.snapshot()).is_ok());
        }
    });
}

#[test]
fn malformed_gate_arguments_use_argument_repair_without_counting_gate_failures() {
    block_on(async {
        let validate_client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "bad-validate",
                "validate_draft",
                json!({"unexpected":true}),
            )]),
            LlmResponse::ToolCalls(vec![call("fixed-validate", "validate_draft", json!({}))]),
            LlmResponse::Text("QUESTION: Simulate?".to_string()),
        ]);
        let validate_probe = validate_client.clone();
        let mut validate_session = DesignSession::with_config(validate_client, long_flow_config());
        *validate_session.draft_mut() = support::golden_draft().await;

        assert!(matches!(
            validate_session.run_burst("Validate it").await,
            BurstOutcome::NeedsInput { .. }
        ));
        assert_eq!(validate_session.observability().validation_failures, 0);
        assert_eq!(validate_session.observability().repair_successes, 1);
        assert_eq!(validate_probe.seen_tools()[1], names(&["validate_draft"]));

        let simulate_client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "bad-simulate",
                "simulate_draft",
                json!({"unexpected":true}),
            )]),
            LlmResponse::ToolCalls(vec![call("fixed-simulate", "simulate_draft", json!({}))]),
            LlmResponse::Text("QUESTION: Review?".to_string()),
        ]);
        let simulate_probe = simulate_client.clone();
        let mut simulate_session = DesignSession::with_config(simulate_client, long_flow_config());
        let mut draft = support::golden_draft().await;
        draft.validated_revision = Some(draft.draft_revision);
        *simulate_session.draft_mut() = draft;

        assert!(matches!(
            simulate_session.run_burst("Simulate it").await,
            BurstOutcome::NeedsInput { .. }
        ));
        assert_eq!(simulate_session.observability().simulation_failures, 0);
        assert_eq!(simulate_session.observability().repair_successes, 1);
        assert_eq!(simulate_probe.seen_tools()[1], names(&["simulate_draft"]));
    });
}

#[test]
fn pending_repair_limits_persist_failed_state_without_resetting_attempt_budget() {
    block_on(async {
        let model_limited_client = ScriptedClient::new(vec![LlmResponse::ToolCalls(vec![call(
            "bad-panel",
            "add_panel",
            json!({"key":"welcome","channel":"study_hub"}),
        )])]);
        let mut model_limited = DesignSession::with_config(
            model_limited_client,
            SessionConfig {
                max_model_calls: 1,
                context_char_budget: 100_000,
                ..SessionConfig::default()
            },
        );

        let BurstOutcome::Halted(report) = model_limited.run_burst("Build a panel").await else {
            panic!("expected repair model limit halt")
        };
        assert_eq!(report.code, "REPAIR_ATTEMPT_FAILED");
        assert_eq!(model_limited.observability().repair_attempts, 0);
        assert_eq!(model_limited.observability().repair_failures, 1);
        assert!(matches!(
            model_limited.snapshot().repair_state,
            Some(RepairState::Failed(_))
        ));
        assert!(DesignSession::restore((), large_config(), model_limited.snapshot()).is_ok());

        let failed_client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "bad-panel",
                "add_panel",
                json!({"key":"welcome","channel":"study_hub"}),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "bad-panel-again",
                "add_panel",
                json!({"key":"welcome","channel":"study_hub"}),
            )]),
        ]);
        let mut failed = DesignSession::with_config(failed_client, large_config());
        assert!(matches!(
            failed.run_burst("Build a panel").await,
            BurstOutcome::Halted(_)
        ));
        let mut pending = failed.snapshot();
        let Some(RepairState::Failed(mut ticket)) = pending.repair_state.take() else {
            panic!("expected failed ticket")
        };
        ticket.attempts_remaining = 1;
        pending.repair_state = Some(RepairState::AwaitingAttempt(ticket.clone()));
        pending.last_error = Some(ticket.original_error.clone());
        pending.observability.repair_attempts = 0;
        pending.observability.repair_failures = 0;
        let restored = DesignSession::restore((), large_config(), pending.clone()).unwrap();
        assert!(matches!(
            restored.snapshot().repair_state,
            Some(RepairState::AwaitingAttempt(_))
        ));
        let context_client = ScriptedClient::new(vec![]);
        let mut context_limited = DesignSession::restore(
            context_client,
            SessionConfig {
                context_char_budget: 1,
                ..large_config()
            },
            pending,
        )
        .unwrap();

        let BurstOutcome::Halted(report) = context_limited.run_burst("Continue").await else {
            panic!("expected repair context halt")
        };
        assert_eq!(report.code, "REPAIR_ATTEMPT_FAILED");
        assert_eq!(context_limited.observability().repair_attempts, 0);
        assert_eq!(context_limited.observability().repair_failures, 1);
        assert!(DesignSession::restore((), large_config(), context_limited.snapshot()).is_ok());
    });
}

#[test]
fn repair_snapshot_invariants_reject_budget_schema_root_and_verify_corruption() {
    block_on(async {
        let argument_client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "bad-panel",
                "add_panel",
                json!({"key":"welcome","channel":"study_hub"}),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "bad-panel-again",
                "add_panel",
                json!({"key":"welcome","channel":"study_hub"}),
            )]),
        ]);
        let mut argument = DesignSession::with_config(argument_client, large_config());
        assert!(matches!(
            argument.run_burst("Build a panel").await,
            BurstOutcome::Halted(_)
        ));
        let valid = argument.snapshot();

        let mut reset_budget = valid.clone();
        let Some(RepairState::Failed(ticket)) = reset_budget.repair_state.as_mut() else {
            panic!("expected failed argument repair")
        };
        ticket.attempts_remaining = 1;
        assert!(matches!(
            DesignSession::restore((), large_config(), reset_budget),
            Err(SessionSnapshotError::InvalidInvariant { .. })
        ));

        let mut wrong_schema = valid.clone();
        let Some(RepairState::Failed(ticket)) = wrong_schema.repair_state.as_mut() else {
            panic!("expected failed argument repair")
        };
        ticket.expected_argument_schema = Some(Value::Null);
        assert!(matches!(
            DesignSession::restore((), large_config(), wrong_schema),
            Err(SessionSnapshotError::InvalidInvariant { .. })
        ));

        let mut overflow = valid.clone();
        let Some(RepairState::Failed(ticket)) = overflow.repair_state.as_mut() else {
            panic!("expected failed argument repair")
        };
        ticket.root_revision = u64::MAX;
        overflow.draft.draft_revision = u64::MAX;
        assert!(matches!(
            DesignSession::restore((), large_config(), overflow),
            Err(SessionSnapshotError::InvalidInvariant { .. })
        ));

        let mut missing_root = valid;
        missing_root.observability.failure_signatures.clear();
        missing_root.observability.repeated_errors = 0;
        assert!(matches!(
            DesignSession::restore((), large_config(), missing_root),
            Err(SessionSnapshotError::InvalidInvariant { .. })
        ));

        let validation_client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call("root-validate", "validate_draft", json!({}))]),
            LlmResponse::ToolCalls(vec![call(
                "irrelevant-panel",
                "add_panel",
                json!({"key":"other","channel":"study_hub","content":"Other"}),
            )]),
            LlmResponse::ToolCalls(vec![call("verify-validate", "validate_draft", json!({}))]),
        ]);
        let mut validation = DesignSession::with_config(validation_client, long_flow_config());
        *validation.draft_mut() = fixable_validation_draft().await;
        assert!(matches!(
            validation.run_burst("Validate it").await,
            BurstOutcome::Halted(_)
        ));
        let mut verify = validation.snapshot();
        let Some(RepairState::Failed(ticket)) = verify.repair_state.take() else {
            panic!("expected failed validation repair")
        };
        verify.repair_state = Some(RepairState::VerifyValidation(ticket));
        verify.observability.repair_failures = 0;
        verify.last_error = None;
        assert!(DesignSession::restore((), long_flow_config(), verify.clone()).is_ok());
        verify.observability.repair_attempts = verify.observability.repair_successes;
        assert!(matches!(
            DesignSession::restore((), long_flow_config(), verify),
            Err(SessionSnapshotError::InvalidInvariant { .. })
        ));
    });
}

#[test]
fn validation_repair_routes_one_mutation_then_exact_validation() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call("root-validate", "validate_draft", json!({}))]),
            LlmResponse::ToolCalls(vec![call(
                "repair-button",
                "add_button",
                json!({
                    "panel_key":"launcher",
                    "label":"Open",
                    "route":{"kind":"static","key":"open_room"}
                }),
            )]),
            LlmResponse::ToolCalls(vec![call("verify-validate", "validate_draft", json!({}))]),
            LlmResponse::Text("QUESTION: Simulate it?".to_string()),
        ]);
        let probe = client.clone();
        let mut session = DesignSession::with_config(client, long_flow_config());
        *session.draft_mut() = fixable_validation_draft().await;

        let outcome = session.run_burst("Validate the launcher").await;

        assert!(matches!(outcome, BurstOutcome::NeedsInput { .. }));
        assert_eq!(session.observability().repair_attempts, 1);
        assert_eq!(session.observability().repair_successes, 1);
        assert_eq!(session.observability().repair_failures, 0);
        assert_eq!(session.observability().validation_failures, 1);
        assert_eq!(
            session.draft().validated_revision,
            Some(session.draft().draft_revision)
        );
        assert!(session.snapshot().repair_state.is_none());
        assert!(session.snapshot().last_error.is_none());
        assert_eq!(
            probe.seen_tools()[1],
            rule_tool_names(true, false, false, None)
        );
        assert_eq!(probe.seen_tools()[2], names(&["validate_draft"]));
        assert_eq!(
            probe.seen_tools()[3],
            rule_tool_names(true, false, false, Some("simulate_draft"))
        );
    });
}

#[test]
fn failed_repair_validation_halts_without_a_second_mutation() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call("root-validate", "validate_draft", json!({}))]),
            LlmResponse::ToolCalls(vec![call(
                "irrelevant-panel",
                "add_panel",
                json!({"key":"other","channel":"study_hub","content":"Other"}),
            )]),
            LlmResponse::ToolCalls(vec![call("verify-validate", "validate_draft", json!({}))]),
            LlmResponse::ToolCalls(vec![call(
                "never-run",
                "add_button",
                json!({
                    "panel_key":"launcher",
                    "label":"Open",
                    "route":{"kind":"static","key":"open_room"}
                }),
            )]),
        ]);
        let mut session = DesignSession::with_config(client, long_flow_config());
        *session.draft_mut() = fixable_validation_draft().await;

        let BurstOutcome::Halted(report) = session.run_burst("Validate it").await else {
            panic!("expected repair verification halt")
        };

        assert_eq!(report.code, "REPAIR_ATTEMPT_FAILED");
        assert_eq!(session.observability().model_calls, 3);
        assert_eq!(session.observability().repair_attempts, 1);
        assert_eq!(session.observability().repair_failures, 1);
        assert_eq!(session.observability().validation_failures, 2);
        assert!(session
            .draft()
            .ruleset
            .panels
            .iter()
            .all(|panel| panel.buttons.is_empty()));
        assert!(DesignSession::restore((), long_flow_config(), session.snapshot()).is_ok());
    });
}

#[test]
fn simulation_repair_forces_mutation_validation_and_simulation_in_order() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call("root-simulate", "simulate_draft", json!({}))]),
            LlmResponse::ToolCalls(vec![call(
                "repair-overwrite",
                "add_upsert_overwrite_action",
                json!({
                    "rule_key":"submit_room",
                    "channel":{"kind":"created","name":"room_channel"},
                    "target_kind":"everyone",
                    "allow":[],
                    "deny":["view_channel"]
                }),
            )]),
            LlmResponse::ToolCalls(vec![call("verify-validate", "validate_draft", json!({}))]),
            LlmResponse::ToolCalls(vec![call("verify-simulate", "simulate_draft", json!({}))]),
            LlmResponse::Text("QUESTION: Review the completed design?".to_string()),
        ]);
        let probe = client.clone();
        let mut session = DesignSession::with_config(client, long_flow_config());
        *session.draft_mut() = fixable_simulation_draft().await;

        let outcome = session.run_burst("Repair the simulation").await;

        assert!(matches!(outcome, BurstOutcome::NeedsInput { .. }));
        assert_eq!(session.observability().repair_attempts, 1);
        assert_eq!(session.observability().repair_successes, 1);
        assert_eq!(session.observability().simulation_failures, 1);
        assert_eq!(
            session.draft().simulated_revision,
            Some(session.draft().draft_revision)
        );
        assert_eq!(
            probe.seen_tools()[1],
            rule_tool_names(true, true, true, None)
        );
        assert_eq!(probe.seen_tools()[2], names(&["validate_draft"]));
        assert_eq!(probe.seen_tools()[3], names(&["simulate_draft"]));
        assert_eq!(probe.seen_tools()[4], simulation_tool_names());
        assert!(session.snapshot().last_error.is_none());
    });
}

#[test]
fn adaptive_simulation_repair_restamps_scope_before_ready() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "brief",
                "set_turn_brief",
                json!({
                    "intent":"modify",
                    "objective":"Repair and verify the StudyRoom design",
                    "requested_outcome":"validated_preview",
                    "assumptions":[],
                    "validate":true
                }),
            )]),
            LlmResponse::ToolCalls(vec![
                call(
                    "update-panel",
                    "update_panel",
                    json!({"key":"study_panel","content":"Create a private study room"}),
                ),
                call("scope", "check_turn_scope", json!({})),
            ]),
            LlmResponse::ToolCalls(vec![call(
                "repair-overwrite",
                "add_upsert_overwrite_action",
                json!({
                    "rule_key":"submit_room",
                    "channel":{"kind":"created","name":"room_channel"},
                    "target_kind":"everyone",
                    "allow":[],
                    "deny":["view_channel"]
                }),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "finish",
                "finish_turn",
                json!({
                    "kind":"ready",
                    "message":"The repaired StudyRoom design is ready.",
                    "question":null
                }),
            )]),
        ]);
        let probe = client.clone();
        let mut session = DesignSession::with_adaptive_config(client, long_flow_config());
        let mut draft = fixable_simulation_draft().await;
        draft.validated_revision = None;
        *session.draft_mut() = draft;

        let outcome = session
            .run_burst("Repair and finish the StudyRoom design")
            .await;

        assert!(
            matches!(
                outcome,
                BurstOutcome::Ready { ref summary }
                    if summary == "The repaired StudyRoom design is ready."
            ),
            "{outcome:?}"
        );
        let revision = session.draft().draft_revision;
        let adaptive = session.adaptive_turn().unwrap();
        assert_eq!(adaptive.scoped_revision, Some(revision));
        assert_eq!(adaptive.previewed_revision, Some(revision));
        assert_eq!(session.draft().validated_revision, Some(revision));
        assert_eq!(session.draft().simulated_revision, Some(revision));
        assert_eq!(session.observability().repair_successes, 1);
        assert_eq!(session.observability().repair_attempts, 1);
        assert_eq!(session.observability().simulation_failures, 1);
        assert_eq!(session.observability().model_calls, 4);
        assert!(probe.seen_tools()[2].contains(&"add_upsert_overwrite_action".to_string()));
        assert_eq!(probe.seen_tools()[3], names(&["finish_turn"]));
        assert!(!probe.seen_tools().iter().flatten().any(|name| {
            matches!(
                name.as_str(),
                "validate_draft" | "simulate_draft" | "render_preview"
            )
        }));
    });
}

#[test]
fn repair_question_escalates_and_internal_directive_never_replaces_human_intent() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "bad-panel",
                "add_panel",
                json!({"key":"welcome","channel":"study_hub"}),
            )]),
            LlmResponse::Text("QUESTION: What content should I use?".to_string()),
            LlmResponse::ToolCalls(vec![call(
                "human-fixed-panel",
                "add_panel",
                json!({"key":"welcome","channel":"study_hub","content":"Hello"}),
            )]),
            LlmResponse::Text("QUESTION: Anything else?".to_string()),
        ]);
        let probe = client.clone();
        let mut session = DesignSession::with_config(client, large_config());

        assert!(matches!(
            session.run_burst("Build my welcome panel").await,
            BurstOutcome::NeedsInput { .. }
        ));
        assert_eq!(session.observability().repair_attempts, 0);
        assert_eq!(session.observability().repair_escalations, 1);
        assert!(session.snapshot().repair_state.is_none());
        let seen = probe.seen();
        let anchor: Value = serde_json::from_str(
            seen[1]
                .last()
                .unwrap()
                .content
                .strip_prefix("DRAFT_STATE:")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(anchor["current_human_intent"], "Build my welcome panel");
        assert!(anchor["recent_human_intent"]
            .as_array()
            .unwrap()
            .iter()
            .all(|intent| !intent.as_str().unwrap().starts_with("REPAIR_REQUIRED:")));

        assert!(matches!(
            session.run_burst("Use a short greeting").await,
            BurstOutcome::NeedsInput { .. }
        ));
        assert_eq!(session.observability().repair_escalations, 1);
        assert!(session.snapshot().last_error.is_none());
        let seen = probe.seen();
        let resolved_anchor: Value = serde_json::from_str(
            seen[3]
                .last()
                .unwrap()
                .content
                .strip_prefix("DRAFT_STATE:")
                .unwrap(),
        )
        .unwrap();
        assert!(resolved_anchor["last_error"].is_null());
    });
}

#[test]
fn not_executed_batch_results_never_record_failures_or_open_tickets() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![
                call(
                    "bad-panel",
                    "add_panel",
                    json!({"key":"welcome","channel":"study_hub"}),
                ),
                call(
                    "skipped-modal",
                    "add_modal",
                    json!({"key":"skipped","title":"Skipped","fields":[]}),
                ),
            ]),
            LlmResponse::Text("QUESTION: Should I correct the panel?".to_string()),
        ]);
        let mut session = DesignSession::with_config(client, large_config());

        assert!(matches!(
            session.run_burst("Build it").await,
            BurstOutcome::NeedsInput { .. }
        ));

        assert_eq!(session.observability().failure_signatures.len(), 1);
        assert!(session
            .observability()
            .failure_signatures
            .keys()
            .all(|signature| !signature.contains("NOT_EXECUTED")));
        assert!(session.draft().ruleset.modals.is_empty());
        let directive_index = session
            .messages()
            .iter()
            .position(|message| message.content.starts_with("REPAIR_REQUIRED:"))
            .unwrap();
        assert_eq!(
            session.messages()[directive_index - 1]
                .tool_call_id
                .as_deref(),
            Some("skipped-modal")
        );
    });
}

#[test]
fn anchor_separates_bounded_current_intent_from_prior_intents() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::Text("QUESTION: First?".to_string()),
            LlmResponse::Text("QUESTION: Second?".to_string()),
        ]);
        let probe = client.clone();
        let mut session = DesignSession::with_config(client, large_config());

        assert!(matches!(
            session.run_burst("First intent").await,
            BurstOutcome::NeedsInput { .. }
        ));
        let long_intent = "x".repeat(300);
        assert!(matches!(
            session.run_burst(&long_intent).await,
            BurstOutcome::NeedsInput { .. }
        ));

        let seen = probe.seen();
        let anchor = seen[1]
            .last()
            .unwrap()
            .content
            .strip_prefix("DRAFT_STATE:")
            .unwrap();
        let state: Value = serde_json::from_str(anchor).unwrap();
        assert_eq!(state["recent_human_intent"], json!(["First intent"]));
        assert_eq!(
            state["current_human_intent"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            241
        );
        assert!(state["current_human_intent"]
            .as_str()
            .unwrap()
            .ends_with('…'));
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
                    "trigger_kind":"button_click",
                    "trigger_ref":"open"
                }),
            )]),
            LlmResponse::Text("QUESTION: Continue?".to_string()),
        ]);
        let resolved_probe = resolved_client.clone();
        let mut resolved = DesignSession::with_config(resolved_client, large_config());

        assert!(matches!(
            resolved.run_burst("Build it").await,
            BurstOutcome::NeedsInput { .. }
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
                    "trigger_kind":"button_click",
                    "trigger_ref":"missing"
                }),
            )]),
            LlmResponse::Text("QUESTION: Which button?".to_string()),
        ]);
        let unresolved_probe = unresolved_client.clone();
        let mut unresolved = DesignSession::with_config(unresolved_client, large_config());

        assert!(matches!(
            unresolved.run_burst("Build it").await,
            BurstOutcome::NeedsInput { .. }
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

        assert!(matches!(outcome, BurstOutcome::NeedsInput { .. }));
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
                    "trigger_kind":"instance_action",
                    "trigger_ref":"create"
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

        assert!(matches!(outcome, BurstOutcome::NeedsInput { .. }));
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
                    "trigger_kind":"instance_action",
                    "trigger_ref":"join"
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

        assert!(matches!(outcome, BurstOutcome::NeedsInput { .. }));
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

        assert!(matches!(outcome, BurstOutcome::NeedsInput { .. }));
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

        assert!(matches!(outcome, BurstOutcome::NeedsInput { .. }));
        assert_eq!(session.observability().simulation_failures, 1);
        assert_eq!(
            probe.seen_tools(),
            vec![
                simulation_tool_names(),
                rule_tool_names(true, true, true, None)
            ]
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

        assert!(matches!(outcome, BurstOutcome::NeedsInput { .. }));
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

        assert!(matches!(outcome, BurstOutcome::NeedsInput { .. }));
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

        assert!(matches!(outcome, BurstOutcome::Ready { .. }), "{outcome:?}");
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
                    "trigger_kind":"modal_submit",
                    "trigger_ref":"feedback"
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

        assert!(matches!(outcome, BurstOutcome::NeedsInput { .. }));
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
                    "trigger_kind":"button_click",
                    "trigger_ref":"open"
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

        assert!(matches!(outcome, BurstOutcome::NeedsInput { .. }));
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

        assert!(matches!(outcome, BurstOutcome::NeedsInput { .. }));
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
            BurstOutcome::NeedsInput { .. }
        ));
        assert!(matches!(
            session.run_burst("Add another panel").await,
            BurstOutcome::NeedsInput { .. }
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

        assert!(matches!(outcome, BurstOutcome::NeedsInput { .. }));
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

        assert!(matches!(outcome, BurstOutcome::NeedsInput { .. }));
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

        assert!(matches!(outcome, BurstOutcome::NeedsInput { .. }));
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

        assert!(matches!(outcome, BurstOutcome::NeedsInput { .. }));
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
            BurstOutcome::NeedsInput { ref question }
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
            BurstOutcome::Ready { ref summary }
                if summary == "StudyRoom design is complete"
        ));
        assert_eq!(
            done_session.draft().simulated_revision,
            Some(done_session.draft().draft_revision)
        );
        assert_eq!(done_session.draft().ruleset, expected.ruleset);
    });
}

#[test]
fn progressed_yields_after_a_draft_change_and_the_session_continues() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "panel",
                "add_panel",
                json!({"key":"welcome","channel":"lobby","content":"Welcome"}),
            )]),
            LlmResponse::Text("PROGRESSED: Added the welcome panel".to_string()),
            LlmResponse::Text("QUESTION: What should the button say?".to_string()),
        ]);
        let mut session = DesignSession::with_config(client, large_config());

        let first = session.run_burst("Create a welcome flow").await;

        assert!(matches!(
            first,
            BurstOutcome::Progressed { ref summary }
                if summary == "Added the welcome panel"
        ));
        assert_eq!(session.turn_state().unwrap().phase, TurnPhase::Progressed);
        assert_eq!(session.turn_state().unwrap().model_calls, 2);
        assert_eq!(session.turn_state().unwrap().tool_calls, 1);

        let second = session.run_burst("Continue").await;

        assert!(matches!(second, BurstOutcome::NeedsInput { .. }));
        assert_eq!(session.turn_state().unwrap().sequence, 2);
        assert_eq!(session.turn_state().unwrap().phase, TurnPhase::NeedsInput);
        assert_eq!(session.observability().model_calls, 3);
    });
}

#[test]
fn ready_requires_current_validation_but_not_mutation_tool_diversity_or_simulation() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call("validate", "validate_draft", json!({}))]),
            LlmResponse::Text("READY: Preview is ready".to_string()),
        ]);
        let mut session = DesignSession::with_config(client, long_flow_config());
        *session.draft_mut() = support::golden_draft().await;

        let outcome = session.run_burst("Show me the finished preview").await;

        assert!(matches!(
            outcome,
            BurstOutcome::Ready { ref summary } if summary == "Preview is ready"
        ));
        assert_eq!(session.turn_state().unwrap().phase, TurnPhase::Ready);
        assert_eq!(
            session.draft().validated_revision,
            Some(session.draft().draft_revision)
        );
        assert_eq!(session.draft().simulated_revision, None);
        assert!(session.observability().distinct_mutation_tools.is_empty());
    });
}

#[test]
fn model_and_tool_limits_reset_each_turn_while_observability_accumulates() {
    block_on(async {
        let model_client = ScriptedClient::new(vec![
            LlmResponse::Text("QUESTION: First decision?".to_string()),
            LlmResponse::Text("QUESTION: Second decision?".to_string()),
        ]);
        let mut model_session = DesignSession::with_config(
            model_client,
            SessionConfig {
                max_model_calls: 1,
                context_char_budget: 100_000,
                ..SessionConfig::default()
            },
        );

        assert!(matches!(
            model_session.run_burst("First").await,
            BurstOutcome::NeedsInput { .. }
        ));
        assert!(matches!(
            model_session.run_burst("Second").await,
            BurstOutcome::NeedsInput { .. }
        ));
        assert_eq!(model_session.observability().model_calls, 2);
        assert_eq!(model_session.turn_state().unwrap().model_calls, 1);

        let tool_client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "panel",
                "add_panel",
                json!({"key":"p","channel":"lobby","content":"Panel"}),
            )]),
            LlmResponse::Text("PROGRESSED: Added a panel".to_string()),
            LlmResponse::ToolCalls(vec![call(
                "modal",
                "add_modal",
                json!({"key":"m","title":"Modal","fields":[]}),
            )]),
            LlmResponse::Text("PROGRESSED: Added a modal".to_string()),
        ]);
        let mut tool_session = DesignSession::with_config(
            tool_client,
            SessionConfig {
                max_tool_calls: 1,
                context_char_budget: 100_000,
                ..SessionConfig::default()
            },
        );

        assert!(matches!(
            tool_session.run_burst("Panel").await,
            BurstOutcome::Progressed { .. }
        ));
        assert!(matches!(
            tool_session.run_burst("Modal").await,
            BurstOutcome::Progressed { .. }
        ));
        assert_eq!(tool_session.observability().tool_calls, 2);
        assert_eq!(tool_session.turn_state().unwrap().tool_calls, 1);
    });
}

#[test]
fn gate_failure_limit_resets_each_turn_while_failures_accumulate() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call("validate-1", "validate_draft", json!({}))]),
            LlmResponse::Text("QUESTION: Fix the missing button?".to_string()),
            LlmResponse::ToolCalls(vec![call("validate-2", "validate_draft", json!({}))]),
            LlmResponse::Text("QUESTION: Still fix the missing button?".to_string()),
        ]);
        let mut session = DesignSession::with_config(
            client,
            SessionConfig {
                max_gate_failures: 2,
                context_char_budget: 100_000,
                ..SessionConfig::default()
            },
        );
        *session.draft_mut() = fixable_validation_draft().await;

        assert!(matches!(
            session.run_burst("Validate once").await,
            BurstOutcome::NeedsInput { .. }
        ));
        assert_eq!(session.turn_state().unwrap().gate_failures, 1);
        assert!(matches!(
            session.run_burst("Validate again").await,
            BurstOutcome::NeedsInput { .. }
        ));
        assert_eq!(session.observability().validation_failures, 2);
        assert_eq!(session.turn_state().unwrap().gate_failures, 1);
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
        assert!(matches!(stale_outcome, BurstOutcome::NeedsInput { .. }));
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
        assert!(matches!(outcome, BurstOutcome::NeedsInput { .. }));
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
                    "trigger_kind":"button_click",
                    "trigger_ref":"missing"
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

        let mut calls = (0..8)
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
            .collect::<Vec<_>>();
        calls.push(call(
            "hidden-button",
            "add_button",
            json!({
                "panel_key":"panel_0",
                "label":"Open",
                "route":{"kind":"static","key":"open"}
            }),
        ));
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
        assert!(matches!(outcome, BurstOutcome::NeedsInput { .. }));
        let seen = probe.seen();
        let second_tool_results = seen[1]
            .iter()
            .filter(|message| message.role == MessageRole::Tool)
            .count();
        assert!(second_tool_results < 8);
        assert_eq!(seen[1][0].role, MessageRole::System);
        assert_eq!(seen[1][0].content, DEFAULT_SYSTEM_PROMPT);
        let anchor = seen[1]
            .last()
            .unwrap()
            .content
            .strip_prefix("DRAFT_STATE:")
            .unwrap();
        let memory: Value = serde_json::from_str(anchor).unwrap();
        assert_eq!(memory["revision"], 8);
        assert!(memory["recent_human_intent"].as_array().unwrap().is_empty());
        assert_eq!(memory["current_human_intent"], "Build panels");
        assert_eq!(memory["panels"].as_array().unwrap().len(), 8);
        assert_eq!(
            memory["last_error"]["code"],
            "TOOL_NOT_AVAILABLE_FOR_DRAFT_STATE"
        );
        assert_eq!(memory["last_error"]["location"], "tool.add_button");
        assert!(memory["last_error"]["hint"]
            .as_str()
            .unwrap()
            .contains("currently available tools"));
        assert_complete_tool_pairs(&seen[1]);
        assert_eq!(
            session
                .messages()
                .iter()
                .filter(|message| message.role == MessageRole::Tool)
                .count(),
            9
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

#[test]
fn adaptive_complete_request_builds_checks_validates_previews_and_returns_ready() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "brief",
                "set_turn_brief",
                json!({
                    "intent":"build",
                    "objective":"Create a feedback modal acknowledgement",
                    "requested_outcome":"validated_preview",
                    "assumptions":[],
                    "validate":true
                }),
            )]),
            LlmResponse::ToolCalls(vec![
                call(
                    "modal",
                    "add_modal",
                    json!({"key":"feedback_modal","title":"Feedback","fields":[{"key":"message","label":"Message","style":"paragraph","required":true}]}),
                ),
                call(
                    "rule",
                    "begin_rule",
                    json!({"key":"ack_feedback","trigger_kind":"modal_submit","trigger_ref":"feedback_modal"}),
                ),
            ]),
            LlmResponse::ToolCalls(vec![
                call(
                    "defer",
                    "add_interaction_action",
                    json!({"kind":"defer_ephemeral","rule_key":"ack_feedback"}),
                ),
                call(
                    "edit",
                    "add_interaction_action",
                    json!({"kind":"edit_response","rule_key":"ack_feedback","content":"Thanks, ${input.message}"}),
                ),
                call("scope", "check_turn_scope", json!({})),
            ]),
            LlmResponse::ToolCalls(vec![call(
                "finish",
                "finish_turn",
                json!({
                    "kind":"ready",
                    "message":"The feedback automation is ready for review.",
                    "question":null
                }),
            )]),
        ]);
        let probe = client.clone();
        let mut session = DesignSession::with_adaptive_config(client, long_flow_config());

        let outcome = session
            .run_burst("Create a Feedback modal and acknowledge its message")
            .await;

        assert!(matches!(
            outcome,
            BurstOutcome::Ready { ref summary }
                if summary == "The feedback automation is ready for review."
        ));
        assert_eq!(session.draft().validated_revision, Some(4));
        let adaptive = session.adaptive_turn().unwrap();
        assert_eq!(adaptive.phase, AdaptivePhase::Reply);
        assert_eq!(adaptive.scoped_revision, Some(4));
        assert_eq!(adaptive.previewed_revision, Some(4));
        assert_eq!(session.observability().model_calls, 4);
        assert_eq!(probe.seen_tools()[3], names(&["finish_turn"]));
        assert!(!probe.seen_tools().iter().flatten().any(|name| {
            matches!(
                name.as_str(),
                "validate_draft" | "simulate_draft" | "render_preview"
            )
        }));
    });
}

#[test]
fn adaptive_success_alias_cannot_bypass_ready_gates() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "brief",
                "set_turn_brief",
                json!({
                    "intent":"build",
                    "objective":"Create a feedback automation",
                    "requested_outcome":"validated_preview",
                    "assumptions":[],
                    "validate":true
                }),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "finish",
                "finish_turn",
                json!({"kind":"success","message":"Ready."}),
            )]),
        ]);
        let mut session = DesignSession::with_adaptive_config(
            client,
            SessionConfig {
                max_model_calls: 2,
                ..large_config()
            },
        );

        let outcome = session.run_burst("Create a feedback automation").await;

        let BurstOutcome::Halted(report) = outcome else {
            panic!("expected the premature success alias to halt")
        };
        assert_eq!(report.code, "MODEL_CALL_LIMIT_EXHAUSTED");
        assert_eq!(report.last_error.unwrap().code, "TURN_NOT_READY");
        assert_eq!(session.draft().draft_revision, 0);
        assert_eq!(session.turn_state().unwrap().phase, TurnPhase::Halted);
    });
}

#[test]
fn adaptive_ambiguous_request_asks_one_structural_question_without_mutating() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "brief",
                "set_turn_brief",
                json!({
                    "intent":"brainstorm",
                    "objective":"Design a study room automation",
                    "requested_outcome":"discussion",
                    "assumptions":[],
                    "validate":false
                }),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "finish",
                "finish_turn",
                json!({
                    "kind":"needs_input",
                    "message":"I need one structural decision.",
                    "question":"Should rooms be private or publicly visible?"
                }),
            )]),
        ]);
        let mut session = DesignSession::with_adaptive_config(client, large_config());

        let outcome = session.run_burst("Make a study room feature").await;

        assert!(matches!(
            outcome,
            BurstOutcome::NeedsInput { ref question }
                if question == "Should rooms be private or publicly visible?"
        ));
        assert_eq!(session.draft().draft_revision, 0);
        assert_eq!(session.observability().clarification_count, 1);
        assert_eq!(
            session
                .adaptive_turn()
                .unwrap()
                .brief
                .as_ref()
                .unwrap()
                .verification
                .simulation,
            design_harness::SimulationProfile::None
        );
    });
}

#[test]
fn adaptive_simulation_profile_uses_only_the_full_current_human_turn() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "first-brief",
                "set_turn_brief",
                json!({
                    "intent":"brainstorm",
                    "objective":"Discuss the requested design",
                    "requested_outcome":"discussion",
                    "assumptions":[],
                    "validate":true
                }),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "first-finish",
                "finish_turn",
                json!({
                    "kind":"needs_input",
                    "message":"Choose a direction.",
                    "question":"Which direction?"
                }),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "second-brief",
                "set_turn_brief",
                json!({
                    "intent":"brainstorm",
                    "objective":"Continue the regular design",
                    "requested_outcome":"discussion",
                    "assumptions":[],
                    "validate":false
                }),
            )]),
            LlmResponse::ToolCalls(vec![call(
                "second-finish",
                "finish_turn",
                json!({
                    "kind":"needs_input",
                    "message":"Choose one option.",
                    "question":"Which option?"
                }),
            )]),
        ]);
        let mut session = DesignSession::with_adaptive_config(client, large_config());
        let first_message = format!("{}sTuDyRoOm", "prefix ".repeat(80));

        assert!(matches!(
            session.run_burst(&first_message).await,
            BurstOutcome::NeedsInput { .. }
        ));
        assert_eq!(
            session
                .adaptive_turn()
                .unwrap()
                .brief
                .as_ref()
                .unwrap()
                .verification
                .simulation,
            design_harness::SimulationProfile::StudyRoom
        );

        assert!(matches!(
            session
                .run_burst("Continue without that named profile")
                .await,
            BurstOutcome::NeedsInput { .. }
        ));
        assert_eq!(
            session
                .adaptive_turn()
                .unwrap()
                .brief
                .as_ref()
                .unwrap()
                .verification
                .simulation,
            design_harness::SimulationProfile::None
        );
    });
}

#[test]
fn adaptive_studyroom_profile_requires_model_selected_validation() {
    block_on(async {
        let client = ScriptedClient::new(vec![LlmResponse::ToolCalls(vec![call(
            "brief",
            "set_turn_brief",
            json!({
                "intent":"build",
                "objective":"Build the requested automation",
                "requested_outcome":"draft_update",
                "assumptions":[],
                "validate":false
            }),
        )])]);
        let mut session = DesignSession::with_adaptive_config(
            client,
            SessionConfig {
                max_model_calls: 1,
                context_char_budget: 100_000,
                ..SessionConfig::default()
            },
        );

        assert!(matches!(
            session.run_burst("Build StudyRoom now").await,
            BurstOutcome::Halted(_)
        ));
        assert!(session.messages().iter().any(|message| {
            message.role == MessageRole::Tool
                && message.content.contains("SIMULATION_REQUIRES_VALIDATION")
        }));
        assert!(session
            .adaptive_turn()
            .is_some_and(|state| state.brief.is_none()));
    });
}

#[test]
fn adaptive_modification_uses_stable_action_update_and_revalidates() {
    block_on(async {
        let mut draft = Draft::new();
        for (name, arguments) in [
            (
                "add_modal",
                json!({"key":"feedback_modal","title":"Feedback","fields":[{"key":"message","label":"Message","style":"paragraph","required":true}]}),
            ),
            (
                "begin_rule",
                json!({"key":"ack_feedback","trigger_kind":"modal_submit","trigger_ref":"feedback_modal"}),
            ),
            (
                "add_interaction_action",
                json!({"kind":"defer_ephemeral","rule_key":"ack_feedback"}),
            ),
            (
                "add_interaction_action",
                json!({"kind":"edit_response","rule_key":"ack_feedback","content":"Old"}),
            ),
        ] {
            assert!(dispatch_tool(&mut draft, name, &arguments.to_string())
                .await
                .is_ok());
        }
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "brief",
                "set_turn_brief",
                json!({
                    "intent":"modify",
                    "objective":"Change the acknowledgement text",
                    "requested_outcome":"validated_preview",
                    "assumptions":[],
                    "validate":true
                }),
            )]),
            LlmResponse::ToolCalls(vec![
                call(
                    "update",
                    "update_action",
                    json!({
                        "rule_key":"ack_feedback",
                        "selector":{"kind":"by_kind","action":"edit_response","occurrence":0},
                        "patch":{"kind":"edit_response","content":"Thanks, ${input.message}"}
                    }),
                ),
                call("scope", "check_turn_scope", json!({})),
            ]),
            LlmResponse::ToolCalls(vec![call(
                "finish",
                "finish_turn",
                json!({"kind":"ready","message":"Updated."}),
            )]),
        ]);
        let probe = client.clone();
        let mut session = DesignSession::with_adaptive_config(client, long_flow_config());
        *session.draft_mut() = draft;

        let outcome = session
            .run_burst("Change the reply to include the message")
            .await;

        assert!(matches!(outcome, BurstOutcome::Ready { .. }), "{outcome:?}");
        let actions = &session.draft().ruleset.rules[0].actions;
        assert_eq!(actions.len(), 2);
        assert!(matches!(
            &actions[1],
            ActionSpec::EditResponse { content } if content == "Thanks, ${input.message}"
        ));
        assert!(probe.seen_tools()[1].contains(&"update_action".to_string()));
    });
}

#[test]
fn adaptive_phase_transition_stops_the_remaining_stale_batch() {
    block_on(async {
        let mut draft = Draft::new();
        for (name, arguments) in [
            (
                "add_modal",
                json!({"key":"feedback_modal","title":"Feedback","fields":[{"key":"message","label":"Message","style":"paragraph","required":true}]}),
            ),
            (
                "begin_rule",
                json!({"key":"ack_feedback","trigger_kind":"modal_submit","trigger_ref":"feedback_modal"}),
            ),
            (
                "add_interaction_action",
                json!({"kind":"defer_ephemeral","rule_key":"ack_feedback"}),
            ),
            (
                "add_interaction_action",
                json!({"kind":"edit_response","rule_key":"ack_feedback","content":"Thanks, ${input.message}"}),
            ),
        ] {
            assert!(dispatch_tool(&mut draft, name, &arguments.to_string())
                .await
                .is_ok());
        }
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call(
                "brief",
                "set_turn_brief",
                json!({
                    "intent":"build",
                    "objective":"Review the existing feedback design",
                    "requested_outcome":"validated_preview",
                    "assumptions":[],
                    "validate":true
                }),
            )]),
            LlmResponse::ToolCalls(vec![
                call(
                    "update",
                    "update_modal",
                    json!({"key":"feedback_modal","title":"Feedback Form"}),
                ),
                call("scope", "check_turn_scope", json!({})),
                call(
                    "stale",
                    "add_modal",
                    json!({"key":"duplicate","title":"Duplicate","fields":[]}),
                ),
            ]),
            LlmResponse::ToolCalls(vec![call(
                "finish",
                "finish_turn",
                json!({"kind":"ready","message":"Ready."}),
            )]),
        ]);
        let mut session = DesignSession::with_adaptive_config(client, long_flow_config());
        *session.draft_mut() = draft;

        let outcome = session.run_burst("Review it").await;

        assert!(matches!(outcome, BurstOutcome::Ready { .. }), "{outcome:?}");
        assert_eq!(session.draft().ruleset.modals.len(), 1);
        assert_eq!(session.draft().ruleset.modals[0].title, "Feedback Form");
        let stale_result = session
            .messages()
            .iter()
            .find(|message| message.tool_call_id.as_deref() == Some("stale"))
            .unwrap();
        assert!(stale_result
            .content
            .contains("NOT_EXECUTED_AFTER_PHASE_TRANSITION"));
    });
}

#[test]
fn adaptive_session_rejects_legacy_ready_text_without_scope() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::Text("READY: skipped".to_string()),
            LlmResponse::Text("READY: skipped again".to_string()),
        ]);
        let mut session = DesignSession::with_adaptive_config(client, large_config());

        let outcome = session.run_burst("Change the existing design").await;

        let BurstOutcome::Halted(report) = outcome else {
            panic!("expected adaptive text rejection")
        };
        assert_eq!(report.code, "UNSTRUCTURED_MODEL_TEXT");
        assert_eq!(session.draft().draft_revision, 0);
    });
}
