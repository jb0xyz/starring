mod support;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use design_harness::{
    tool_definitions, BurstOutcome, DesignSession, LimitKind, LlmClient, LlmError, LlmResponse,
    Message, MessageRole, SessionConfig, ToolCall, DEFAULT_SYSTEM_PROMPT,
};
use futures::executor::block_on;
use serde_json::{json, Value};

#[derive(Clone)]
struct ScriptedClient {
    responses: Arc<Mutex<VecDeque<Result<LlmResponse, LlmError>>>>,
    seen: Arc<Mutex<Vec<Vec<Message>>>>,
}

impl ScriptedClient {
    fn new(responses: Vec<LlmResponse>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses.into_iter().map(Ok).collect())),
            seen: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn seen(&self) -> Vec<Vec<Message>> {
        self.seen.lock().unwrap().clone()
    }
}

impl LlmClient for ScriptedClient {
    async fn complete(
        &self,
        messages: &[Message],
        _tools: &[design_harness::ToolDefinition],
    ) -> Result<LlmResponse, LlmError> {
        self.seen.lock().unwrap().push(messages.to_vec());
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

#[test]
fn tool_calls_execute_serially_before_the_next_model_call() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![
                call(
                    "1",
                    "add_panel",
                    json!({"key":"p","channel":"study_hub","content":"Panel"}),
                ),
                call(
                    "2",
                    "add_button",
                    json!({
                        "panel_key":"p",
                        "label":"Open",
                        "route":{"kind":"static","key":"open"}
                    }),
                ),
            ]),
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
        assert_eq!(probe.seen().len(), 2);
        assert!(probe.seen()[1]
            .iter()
            .filter(|message| message.role == MessageRole::Tool)
            .all(|message| message.content.contains("\"ok\":true")));
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

        let calls = support::golden_calls()
            .into_iter()
            .enumerate()
            .map(|(index, (name, arguments))| call(&index.to_string(), name, arguments))
            .collect();
        let expected = support::golden_draft().await;
        let done_client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(calls),
            LlmResponse::ToolCalls(vec![
                call("validate", "validate_draft", json!({})),
                call("simulate", "simulate_draft", json!({})),
            ]),
            LlmResponse::Text("DONE: StudyRoom design is complete".to_string()),
        ]);
        let mut done_session = DesignSession::with_config(done_client, large_config());

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
fn session_observes_revision_invalidation_and_validate_before_simulate() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            LlmResponse::ToolCalls(vec![call("1", "simulate_draft", json!({}))]),
            LlmResponse::Text("QUESTION: Should I validate?".to_string()),
        ]);
        let mut session = DesignSession::with_config(client, large_config());
        let outcome = session.run_burst("Simulate").await;
        assert!(matches!(outcome, BurstOutcome::AwaitingHuman { .. }));
        assert_eq!(session.observability().simulation_failures, 1);
        assert!(session
            .messages()
            .iter()
            .any(|message| message.content.contains("DRAFT_NOT_VALIDATED")));

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

        let gate_client = ScriptedClient::new(vec![LlmResponse::ToolCalls(vec![call(
            "1",
            "simulate_draft",
            json!({}),
        )])]);
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
        assert_eq!(gate_report.last_error.unwrap().code, "DRAFT_NOT_VALIDATED");
    });
}

#[test]
fn context_trims_old_tool_results_and_halts_when_the_anchor_cannot_fit() {
    block_on(async {
        let tools = tool_definitions();
        let tool_chars = serde_json::to_string(&tools).unwrap().len();
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
        assert!(seen[1][1].content.starts_with("DRAFT_STATE:"));

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
