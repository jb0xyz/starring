use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use futures::executor::block_on;
use serde_json::json;

use crate::llm::{LlmError, LlmResponse};
use crate::{dispatch_tool, BurstOutcome, LlmClient, Message, ToolCall, ToolDefinition, TurnPhase};

use super::*;

type SeenCalls = Vec<(Vec<Message>, Vec<ToolDefinition>)>;

#[derive(Clone)]
struct ScriptedClient {
    responses: Arc<Mutex<VecDeque<Result<LlmResponse, LlmError>>>>,
    calls: Arc<Mutex<SeenCalls>>,
}

impl ScriptedClient {
    fn new(responses: Vec<Result<LlmResponse, LlmError>>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses.into())),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn calls(&self) -> Vec<(Vec<Message>, Vec<ToolDefinition>)> {
        self.calls.lock().unwrap().clone()
    }
}

impl LlmClient for ScriptedClient {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> Result<LlmResponse, LlmError> {
        self.calls
            .lock()
            .unwrap()
            .push((messages.to_vec(), tools.to_vec()));
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("scripted response")
    }
}

fn tool_call(id: &str, name: &str, arguments: serde_json::Value) -> LlmResponse {
    LlmResponse::ToolCalls(vec![ToolCall {
        id: id.to_string(),
        name: name.to_string(),
        arguments: arguments.to_string(),
    }])
}

fn bindings(channel: &str, id: &str) -> ResourceBindingMap {
    let mut bindings = ResourceBindingMap::default();
    bindings.channel_bindings.insert(
        serde_json::from_value(json!(channel)).unwrap(),
        id.parse().unwrap(),
    );
    bindings
}

fn route_private_room(expected_revision: u64, hub: Option<&str>) -> LlmResponse {
    let mut proposal = json!({
        "objective": "Create managed private study rooms",
        "requested_outcome": "validated_preview",
        "locale": "en"
    });
    if let Some(hub) = hub {
        proposal["hub_channel"] = json!(hub);
    }
    tool_call(
        "route",
        "route_intent_turn",
        json!({
            "expected_revision": expected_revision,
            "route": {
                "kind": "private_study_room",
                "proposal": proposal
            }
        }),
    )
}

fn resolve_channel(expected_revision: u64, channel: &str) -> LlmResponse {
    tool_call(
        "resolve",
        "resolve_intent_decision",
        json!({
            "expected_revision": expected_revision,
            "channel": channel
        }),
    )
}

fn receipt<C>(session: &DesignSession<C>) -> IntentRecipeReceiptV1 {
    let Some(IntentRecipeStatusV1::PreviewReady { receipt, .. }) = session.intent_recipe_status()
    else {
        panic!("expected preview receipt")
    };
    receipt
}

#[test]
fn one_shot_uses_one_model_call_one_frontier_and_no_plan_tool_budget() {
    block_on(async {
        let client = ScriptedClient::new(vec![Ok(route_private_room(0, Some("community_hub")))]);
        let probe = client.clone();
        let mut session =
            DesignSession::with_intent_recipe(client, bindings("community_hub", "700"));

        assert!(matches!(
            session.run_burst("Create private study rooms").await,
            BurstOutcome::Ready { .. }
        ));

        let calls = probe.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1.len(), 1);
        assert_eq!(calls[0].1[0].name, "route_intent_turn");
        assert!(calls[0]
            .0
            .last()
            .unwrap()
            .content
            .starts_with(INTENT_STATE_PREFIX));
        assert_eq!(session.observability.model_calls, 1);
        assert_eq!(session.observability.tool_calls, 1);
        assert_eq!(session.observability.plan_compiled_tool_calls, 0);
        assert_eq!(session.observability.plan_submissions, 0);
        assert_eq!(session.observability.intent_commits, 1);
        assert!(session.observability.intent_compiled_operations > 20);
        assert_eq!(session.turn_state().unwrap().phase, TurnPhase::Ready);
        assert_eq!(
            session.draft.validated_revision,
            Some(session.draft.draft_revision)
        );
        assert_eq!(
            session.draft.simulated_revision,
            Some(session.draft.draft_revision)
        );
    });
}

#[test]
fn missing_hub_asks_once_then_resumes_with_one_model_call_per_turn() {
    block_on(async {
        let client = ScriptedClient::new(vec![
            Ok(route_private_room(0, None)),
            Ok(resolve_channel(1, "community_hub")),
        ]);
        let probe = client.clone();
        let mut session =
            DesignSession::with_intent_recipe(client, bindings("community_hub", "700"));

        let BurstOutcome::NeedsInput { question } =
            session.run_burst("Create private study rooms").await
        else {
            panic!("expected one question")
        };
        assert!(!question.is_empty());
        assert_eq!(session.observability.clarification_count, 1);
        assert_eq!(session.draft.draft_revision, 0);
        let Some(IntentRecipeStatusV1::AwaitingDecision {
            workspace_revision,
            available_channel_keys,
            ..
        }) = session.intent_recipe_status()
        else {
            panic!("expected pending decision")
        };
        assert_eq!(workspace_revision, 1);
        assert_eq!(available_channel_keys, vec!["community_hub"]);

        assert!(matches!(
            session.run_burst("Use the community hub").await,
            BurstOutcome::Ready { .. }
        ));
        assert_eq!(probe.calls().len(), 2);
        assert_eq!(probe.calls()[0].1[0].name, "route_intent_turn");
        assert_eq!(probe.calls()[1].1[0].name, "resolve_intent_decision");
        assert_eq!(session.observability.model_calls, 2);
        assert_eq!(session.observability.tool_calls, 2);
        assert_eq!(session.observability.clarification_count, 1);
        assert_eq!(session.observability.intent_resolution_acceptances, 1);
    });
}

#[test]
fn one_shot_and_resumed_routes_compile_to_the_same_semantics_plan_and_draft() {
    block_on(async {
        let mut one_shot = DesignSession::with_intent_recipe(
            ScriptedClient::new(vec![Ok(route_private_room(0, Some("community_hub")))]),
            bindings("community_hub", "700"),
        );
        assert!(matches!(
            one_shot.run_burst("Build it in one turn").await,
            BurstOutcome::Ready { .. }
        ));

        let mut resumed = DesignSession::with_intent_recipe(
            ScriptedClient::new(vec![
                Ok(route_private_room(0, None)),
                Ok(resolve_channel(1, "community_hub")),
            ]),
            bindings("community_hub", "700"),
        );
        assert!(matches!(
            resumed.run_burst("Build it conversationally").await,
            BurstOutcome::NeedsInput { .. }
        ));
        assert!(matches!(
            resumed.run_burst("Use community_hub").await,
            BurstOutcome::Ready { .. }
        ));

        let one_shot_receipt = receipt(&one_shot);
        let resumed_receipt = receipt(&resumed);
        assert_eq!(
            one_shot_receipt.semantic_intent_hash,
            resumed_receipt.semantic_intent_hash
        );
        assert_eq!(
            one_shot_receipt.compiled_plan_hash,
            resumed_receipt.compiled_plan_hash
        );
        assert_ne!(
            one_shot_receipt.input_intent_hash,
            resumed_receipt.input_intent_hash
        );
        assert_eq!(one_shot.draft, resumed.draft);
    });
}

#[test]
fn awaiting_decision_snapshot_restarts_and_binding_drift_fails_closed() {
    block_on(async {
        let initial_bindings = bindings("community_hub", "700");
        let mut session = DesignSession::with_intent_recipe(
            ScriptedClient::new(vec![Ok(route_private_room(0, None))]),
            initial_bindings.clone(),
        );
        assert!(matches!(
            session.run_burst("Create rooms").await,
            BurstOutcome::NeedsInput { .. }
        ));
        let original_fingerprint = session
            .intent_recipe_binding_fingerprint()
            .unwrap()
            .to_string();
        let snapshot_json = serde_json::to_string(&session.snapshot()).unwrap();
        let snapshot: SessionSnapshot = serde_json::from_str(&snapshot_json).unwrap();
        assert!(matches!(
            DesignSession::restore_intent_recipe(
                ScriptedClient::new(Vec::new()),
                SessionConfig::default(),
                snapshot.clone(),
                bindings("community_hub", "701"),
            ),
            Err(SessionSnapshotError::InvalidInvariant { .. })
        ));
        assert!(matches!(
            DesignSession::restore(
                ScriptedClient::new(Vec::new()),
                SessionConfig::default(),
                snapshot.clone(),
            ),
            Err(SessionSnapshotError::InvalidInvariant { .. })
        ));
        let mut restored = DesignSession::restore_intent_recipe(
            ScriptedClient::new(vec![Ok(resolve_channel(1, "community_hub"))]),
            SessionConfig::default(),
            snapshot,
            initial_bindings,
        )
        .unwrap();
        assert_eq!(
            restored.intent_recipe_binding_fingerprint(),
            Some(original_fingerprint.as_str())
        );
        assert!(matches!(
            restored.run_burst("Use community_hub").await,
            BurstOutcome::Ready { .. }
        ));
    });
}

#[test]
fn empty_and_preview_ready_snapshots_restore_with_typed_status_and_receipt() {
    block_on(async {
        let resource_bindings = bindings("community_hub", "700");
        let empty = DesignSession::with_intent_recipe(
            ScriptedClient::new(Vec::new()),
            resource_bindings.clone(),
        );
        let empty_snapshot: SessionSnapshot =
            serde_json::from_str(&serde_json::to_string(&empty.snapshot()).unwrap()).unwrap();
        let empty_restored = DesignSession::restore_intent_recipe(
            ScriptedClient::new(Vec::new()),
            SessionConfig::default(),
            empty_snapshot,
            resource_bindings.clone(),
        )
        .unwrap();
        assert!(matches!(
            empty_restored.intent_recipe_status(),
            Some(IntentRecipeStatusV1::Empty {
                expected_revision: 0
            })
        ));

        let mut ready = DesignSession::with_intent_recipe(
            ScriptedClient::new(vec![Ok(route_private_room(0, Some("community_hub")))]),
            resource_bindings.clone(),
        );
        assert!(matches!(
            ready.run_burst("Create rooms").await,
            BurstOutcome::Ready { .. }
        ));
        let expected_draft = ready.draft.clone();
        let expected_receipt = receipt(&ready);
        let ready_snapshot: SessionSnapshot =
            serde_json::from_str(&serde_json::to_string(&ready.snapshot()).unwrap()).unwrap();
        let ready_restored = DesignSession::restore_intent_recipe(
            ScriptedClient::new(Vec::new()),
            SessionConfig::default(),
            ready_snapshot,
            resource_bindings,
        )
        .unwrap();
        assert_eq!(ready_restored.draft, expected_draft);
        assert_eq!(receipt(&ready_restored), expected_receipt);
    });
}

#[test]
fn stale_revision_wrong_tool_and_llm_failure_each_halt_without_mutation_or_retry() {
    block_on(async {
        let cases = [
            (
                ScriptedClient::new(vec![Ok(route_private_room(99, Some("community_hub")))]),
                "STALE_INTENT_WORKSPACE_REVISION",
            ),
            (
                ScriptedClient::new(vec![Ok(tool_call("wrong", "add_panel", json!({})))]),
                "INTENT_FRONTIER_VIOLATION",
            ),
            (
                ScriptedClient::new(vec![Err(LlmError::Client("offline".to_string()))]),
                "LLM_CLIENT_ERROR",
            ),
        ];
        for (client, expected_code) in cases {
            let probe = client.clone();
            let mut session =
                DesignSession::with_intent_recipe(client, bindings("community_hub", "700"));
            let root = session.draft.clone();
            let BurstOutcome::Halted(report) = session.run_burst("Build rooms").await else {
                panic!("expected halt")
            };
            assert_eq!(report.code, expected_code);
            assert_eq!(session.draft, root);
            assert_eq!(probe.calls().len(), 1);
            assert_eq!(session.observability.model_calls, 1);
            assert_eq!(session.observability.intent_commits, 0);
        }
    });
}

#[test]
fn candidate_conflict_rolls_back_every_compiled_operation() {
    block_on(async {
        let client = ScriptedClient::new(vec![Ok(route_private_room(1, Some("community_hub")))]);
        let mut session =
            DesignSession::with_intent_recipe(client, bindings("community_hub", "700"));
        let result = dispatch_tool(
            &mut session.draft,
            "add_panel",
            &json!({
                "key": "private_study_room__study_panel",
                "channel": "community_hub",
                "content": "Conflicting panel"
            })
            .to_string(),
        )
        .await;
        assert!(result.is_ok(), "{}", result.as_json());
        let root = session.draft.clone();

        let BurstOutcome::Halted(report) = session.run_burst("Build rooms").await else {
            panic!("expected conflict halt")
        };
        assert!(report.code.contains("CONFLICT"), "{}", report.code);
        assert_eq!(session.draft, root);
        assert_eq!(session.observability.intent_compile_attempts, 1);
        assert_eq!(session.observability.intent_compile_successes, 0);
        assert_eq!(session.observability.intent_rollbacks, 1);
        assert_eq!(session.observability.intent_conflicts, 1);
        assert_eq!(session.observability.intent_commits, 0);
        assert_eq!(session.observability.plan_compiled_tool_calls, 0);
    });
}

#[test]
fn typed_fallback_is_public_and_finishes_in_routed_phase() {
    block_on(async {
        let response = tool_call(
            "route",
            "route_intent_turn",
            json!({
                "expected_revision": 0,
                "route": {
                    "kind": "typed_planner",
                    "reason": "another supported automation",
                    "response": "I will continue with the typed planner."
                }
            }),
        );
        let mut session = DesignSession::with_intent_recipe(
            ScriptedClient::new(vec![Ok(response)]),
            bindings("community_hub", "700"),
        );
        let BurstOutcome::Routed { fallback } = session.run_burst("Build a game").await else {
            panic!("expected routed fallback")
        };
        assert_eq!(fallback.kind(), IntentFallbackKind::TypedPlanner);
        assert_eq!(session.turn_state().unwrap().phase, TurnPhase::Routed);
        assert_eq!(session.draft.draft_revision, 0);
        assert_eq!(
            session
                .observability
                .intent_fallback_routes
                .get("typed_planner"),
            Some(&1)
        );
    });
}
