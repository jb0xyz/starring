use std::collections::BTreeMap;
use std::time::Duration;

use design_harness::{
    simulate_draft, validate_draft, BurstOutcome, DesignSession, Draft, IntentRecipeStatusV1,
    Observability, SessionConfig, ToolFailure, ToolResult, SESSION_SNAPSHOT_VERSION,
};
use serde_json::{json, Value};

pub struct TurnReportInput<'a> {
    pub id: &'a str,
    pub input: &'a str,
    pub before: &'a Draft,
    pub after: &'a Draft,
    pub observability_before: &'a Observability,
    pub observability_after: &'a Observability,
    pub outcome: &'a BurstOutcome,
    pub elapsed: Duration,
    pub injected_control_calls_before: usize,
    pub injected_control_calls_after: usize,
    pub delegated_model_calls_before: usize,
    pub delegated_model_calls_after: usize,
}

pub fn turn_report(input: TurnReportInput<'_>) -> Value {
    let outcome = outcome_fields(input.outcome);
    json!({
        "id": input.id,
        "input": input.input,
        "outcome": outcome.name,
        "completed": outcome.completed,
        "message": outcome.message,
        "question": outcome.question,
        "halt_code": outcome.halt_code,
        "last_error": outcome.last_error,
        "draft_revision_before": input.before.draft_revision,
        "draft_revision_after": input.after.draft_revision,
        "draft_changed": input.before.draft_revision != input.after.draft_revision,
        "draft_before": draft_state(input.before),
        "draft_after": draft_state(input.after),
        "actual_gates": actual_gates(input.after),
        "observability_delta": observability_delta(input.observability_before, input.observability_after),
        "injected_control_calls": input.injected_control_calls_after.saturating_sub(input.injected_control_calls_before),
        "delegated_model_calls": input.delegated_model_calls_after.saturating_sub(input.delegated_model_calls_before),
        "elapsed_ms": input.elapsed.as_millis()
    })
}

pub struct IntentTurnReportInput<'a> {
    pub id: &'a str,
    pub input: &'a str,
    pub before: &'a Draft,
    pub after: &'a Draft,
    pub status_before: &'a IntentRecipeStatusV1,
    pub status_after: &'a IntentRecipeStatusV1,
    pub observability_before: &'a Observability,
    pub observability_after: &'a Observability,
    pub outcome: &'a BurstOutcome,
    pub elapsed: Duration,
    pub restart_after: bool,
    pub restart_performed: bool,
}

pub fn intent_turn_report(input: IntentTurnReportInput<'_>) -> Value {
    let outcome = outcome_fields(input.outcome);
    let model_calls = input
        .observability_after
        .model_calls
        .saturating_sub(input.observability_before.model_calls);
    let model_tool_calls = input
        .observability_after
        .tool_calls
        .saturating_sub(input.observability_before.tool_calls);
    let deterministic_operations = input
        .observability_after
        .intent_compiled_operations
        .saturating_sub(input.observability_before.intent_compiled_operations);
    json!({
        "id": input.id,
        "input": input.input,
        "outcome": outcome.name,
        "completed": outcome.completed,
        "message": outcome.message,
        "question": outcome.question,
        "halt_code": outcome.halt_code,
        "last_error": outcome.last_error,
        "elapsed_ms": input.elapsed.as_millis(),
        "model_calls": model_calls,
        "model_tool_calls": model_tool_calls,
        "deterministic_operations": deterministic_operations,
        "intent_counters": intent_observability_delta(
            input.observability_before,
            input.observability_after
        ),
        "stage_before": intent_stage(input.status_before),
        "stage_after": intent_stage(input.status_after),
        "intent_revision_before": intent_revision(input.status_before),
        "intent_revision_after": intent_revision(input.status_after),
        "draft_revision_before": input.before.draft_revision,
        "draft_revision_after": input.after.draft_revision,
        "draft_changed": input.before != input.after,
        "actual_gates": actual_gates(input.after),
        "restart_after": input.restart_after,
        "restart_performed": input.restart_performed
    })
}

pub struct IntentReportMetadata<'a> {
    pub gateway_id: &'a str,
    pub declared_context_tokens: u64,
    pub source_commit: &'a str,
    pub source_dirty: bool,
    pub build_source_commit: &'a str,
    pub build_source_dirty: bool,
    pub binary_sha256: &'a str,
    pub run_id: &'a str,
    pub run_order: u64,
    pub started_at_unix_ms: u64,
    pub ended_at_unix_ms: u64,
    pub requested_model: &'a str,
    pub session_config: &'a SessionConfig,
}

pub struct IntentPersistenceEvidence {
    pub store_writes: usize,
    pub connection_reopens: usize,
    pub final_generation: u64,
}

pub fn intent_report<C>(
    session: &DesignSession<C>,
    turns: Vec<Value>,
    elapsed: Duration,
    persistence: IntentPersistenceEvidence,
    metadata: IntentReportMetadata<'_>,
) -> Value {
    let draft = session.draft();
    let status = session.intent_recipe_status();
    let receipt = status.as_ref().and_then(|status| match status {
        IntentRecipeStatusV1::PreviewReady { receipt, .. } => Some(receipt),
        IntentRecipeStatusV1::Empty { .. } | IntentRecipeStatusV1::AwaitingDecision { .. } => None,
    });
    let final_stage = status.as_ref().map(intent_stage);
    let terminal = turns.last();
    let served_model = (session.observability().tool_calls > 0).then_some(metadata.requested_model);
    json!({
        "schema_version": 3,
        "input_schema_version": 3,
        "mode": "intent_recipe",
        "requested_model": metadata.requested_model,
        "served_model": served_model,
        "gateway_id": metadata.gateway_id,
        "declared_context_tokens": metadata.declared_context_tokens,
        "context_declaration_source": "evaluation_provider",
        "gateway_context_observed_tokens": Value::Null,
        "provenance": {
            "source_commit": metadata.source_commit,
            "source_dirty": metadata.source_dirty,
            "build_source_commit": metadata.build_source_commit,
            "build_source_dirty": metadata.build_source_dirty,
            "binary_sha256": metadata.binary_sha256,
            "attestation_kind": "local_unsigned",
            "run_id": metadata.run_id,
            "run_order": metadata.run_order,
            "started_at_unix_ms": metadata.started_at_unix_ms,
            "ended_at_unix_ms": metadata.ended_at_unix_ms
        },
        "oracle": {
            "enabled": false,
            "injected_control_calls": 0
        },
        "session_config": {
            "max_model_calls": metadata.session_config.max_model_calls,
            "max_tool_calls": metadata.session_config.max_tool_calls,
            "max_gate_failures": metadata.session_config.max_gate_failures,
            "context_char_budget": metadata.session_config.context_char_budget
        },
        "outcome": terminal.and_then(|turn| turn.get("outcome")).cloned().unwrap_or(Value::Null),
        "completed": terminal.and_then(|turn| turn.get("completed")).and_then(Value::as_bool).unwrap_or(false),
        "message": terminal.and_then(|turn| turn.get("message")).cloned().unwrap_or(Value::Null),
        "question": terminal.and_then(|turn| turn.get("question")).cloned().unwrap_or(Value::Null),
        "halt_code": terminal.and_then(|turn| turn.get("halt_code")).cloned().unwrap_or(Value::Null),
        "turns": turns,
        "draft_revision": draft.draft_revision,
        "ruleset": &draft.ruleset,
        "actual_gates": actual_gates(draft),
        "observability": session.observability(),
        "final_intent": {
            "status": final_stage,
            "public_status": status,
            "receipt": receipt,
            "binding_fingerprint": session.intent_recipe_binding_fingerprint()
        },
        "persistence": {
            "backend": "sqlite_file",
            "store_writes": persistence.store_writes,
            "connection_reopen_count": persistence.connection_reopens,
            "final_generation": persistence.final_generation,
            "snapshot_schema_version": SESSION_SNAPSHOT_VERSION,
            "roundtrip_verified": persistence.connection_reopens > 0
        },
        "elapsed_ms": elapsed.as_millis()
    })
}

pub async fn report<C>(
    session: &DesignSession<C>,
    turns: Vec<Value>,
    elapsed: Duration,
    input_schema_version: u32,
    mode: &str,
    injected_control_calls: usize,
    delegated_model_calls: usize,
) -> Value {
    let draft = session.draft();
    let mut checked_draft = draft.clone();
    let final_validation = validate_draft(&mut checked_draft);
    let final_simulation = if final_validation.is_ok() {
        Some(simulate_draft(&mut checked_draft).await)
    } else {
        None
    };
    let failure_signatures = &session.observability().failure_signatures;
    let repeated_error_signatures = failure_signatures
        .iter()
        .filter_map(|(signature, count)| (*count > 1).then_some(signature.clone()))
        .collect::<Vec<_>>();
    let max_repeat_count = failure_signatures.values().copied().max().unwrap_or(0);
    let terminal = turns.last();

    json!({
        "schema_version": 2,
        "input_schema_version": input_schema_version,
        "mode": mode,
        "outcome": terminal.and_then(|turn| turn.get("outcome")).cloned().unwrap_or(Value::Null),
        "completed": terminal.and_then(|turn| turn.get("completed")).and_then(Value::as_bool).unwrap_or(false),
        "message": terminal.and_then(|turn| turn.get("message")).cloned().unwrap_or(Value::Null),
        "question": terminal.and_then(|turn| turn.get("question")).cloned().unwrap_or(Value::Null),
        "halt_code": terminal.and_then(|turn| turn.get("halt_code")).cloned().unwrap_or(Value::Null),
        "turns": turns,
        "draft_revision": draft.draft_revision,
        "draft": draft.summary(),
        "ruleset": &draft.ruleset,
        "actual_gates": actual_gates(draft),
        "observability": session.observability(),
        "injected_control_calls": injected_control_calls,
        "delegated_model_calls": delegated_model_calls,
        "postcheck": {
            "validate_passed": final_validation.is_ok(),
            "validate_error": failure(&final_validation),
            "simulate_attempted": final_simulation.is_some(),
            "simulate_passed": final_simulation.as_ref().is_some_and(ToolResult::is_ok),
            "simulate_error": final_simulation.as_ref().and_then(failure)
        },
        "repeated_error_signatures": repeated_error_signatures,
        "max_repeat_count": max_repeat_count,
        "elapsed_ms": elapsed.as_millis()
    })
}

struct OutcomeFields<'a> {
    name: &'static str,
    message: &'a str,
    question: Option<&'a str>,
    completed: bool,
    halt_code: Option<&'a str>,
    last_error: Option<&'a design_harness::StructuredError>,
}

fn outcome_fields(outcome: &BurstOutcome) -> OutcomeFields<'_> {
    match outcome {
        BurstOutcome::NeedsInput { question } => OutcomeFields {
            name: "needs_input",
            message: question,
            question: Some(question),
            completed: false,
            halt_code: None,
            last_error: None,
        },
        BurstOutcome::Progressed { summary } => OutcomeFields {
            name: "progressed",
            message: summary,
            question: None,
            completed: false,
            halt_code: None,
            last_error: None,
        },
        BurstOutcome::Ready { summary } => OutcomeFields {
            name: "ready",
            message: summary,
            question: None,
            completed: true,
            halt_code: None,
            last_error: None,
        },
        BurstOutcome::Routed { fallback, .. } => OutcomeFields {
            name: "routed",
            message: fallback.response(),
            question: None,
            completed: false,
            halt_code: None,
            last_error: None,
        },
        BurstOutcome::Halted(report) => OutcomeFields {
            name: "halted",
            message: &report.message,
            question: None,
            completed: false,
            halt_code: Some(&report.code),
            last_error: report.last_error.as_ref(),
        },
    }
}

fn actual_gates(draft: &Draft) -> Value {
    json!({
        "validated_revision": draft.validated_revision,
        "simulated_revision": draft.simulated_revision,
        "validation_current": draft.validated_revision == Some(draft.draft_revision),
        "simulation_current": draft.simulated_revision == Some(draft.draft_revision)
    })
}

fn draft_state(draft: &Draft) -> Value {
    json!({
        "revision": draft.draft_revision,
        "validated_revision": draft.validated_revision,
        "simulated_revision": draft.simulated_revision,
        "summary": draft.summary(),
        "ruleset": &draft.ruleset
    })
}

fn observability_delta(before: &Observability, after: &Observability) -> Value {
    let distinct_mutation_tools = after
        .distinct_mutation_tools
        .difference(&before.distinct_mutation_tools)
        .cloned()
        .collect::<Vec<_>>();
    let failure_signatures = after
        .failure_signatures
        .iter()
        .filter_map(|(signature, count)| {
            let delta =
                count.saturating_sub(*before.failure_signatures.get(signature).unwrap_or(&0));
            (delta > 0).then_some((signature.clone(), delta))
        })
        .collect::<BTreeMap<_, _>>();
    let mutation_tool_calls = after
        .mutation_tool_calls
        .iter()
        .filter_map(|(name, count)| {
            let delta = count.saturating_sub(*before.mutation_tool_calls.get(name).unwrap_or(&0));
            (delta > 0).then_some((name.clone(), delta))
        })
        .collect::<BTreeMap<_, _>>();

    json!({
        "model_calls": after.model_calls.saturating_sub(before.model_calls),
        "tool_calls": after.tool_calls.saturating_sub(before.tool_calls),
        "distinct_mutation_tools": distinct_mutation_tools,
        "mutation_tool_calls": mutation_tool_calls,
        "clarification_count": after.clarification_count.saturating_sub(before.clarification_count),
        "validation_failures": after.validation_failures.saturating_sub(before.validation_failures),
        "simulation_failures": after.simulation_failures.saturating_sub(before.simulation_failures),
        "failure_signatures": failure_signatures,
        "repeated_errors": after.repeated_errors.saturating_sub(before.repeated_errors),
        "repair_attempts": after.repair_attempts.saturating_sub(before.repair_attempts),
        "repair_successes": after.repair_successes.saturating_sub(before.repair_successes),
        "repair_failures": after.repair_failures.saturating_sub(before.repair_failures),
        "repair_escalations": after.repair_escalations.saturating_sub(before.repair_escalations),
        "nudge_count": after.nudge_count.saturating_sub(before.nudge_count),
        "plan_submissions": after.plan_submissions.saturating_sub(before.plan_submissions),
        "plan_acceptances": after.plan_acceptances.saturating_sub(before.plan_acceptances),
        "planned_requirements": after.planned_requirements.saturating_sub(before.planned_requirements),
        "plan_compiled_tool_calls": after.plan_compiled_tool_calls.saturating_sub(before.plan_compiled_tool_calls),
        "plan_execution_failures": after.plan_execution_failures.saturating_sub(before.plan_execution_failures),
        "plan_rollbacks": after.plan_rollbacks.saturating_sub(before.plan_rollbacks),
        "plan_commits": after.plan_commits.saturating_sub(before.plan_commits),
        "plan_conflicts": after.plan_conflicts.saturating_sub(before.plan_conflicts)
    })
}

fn intent_observability_delta(before: &Observability, after: &Observability) -> Value {
    let fallback_routes = after
        .intent_fallback_routes
        .iter()
        .filter_map(|(kind, count)| {
            let delta =
                count.saturating_sub(*before.intent_fallback_routes.get(kind).unwrap_or(&0));
            (delta > 0).then_some((kind.clone(), delta))
        })
        .collect::<BTreeMap<_, _>>();
    json!({
        "route_calls": after.intent_route_calls.saturating_sub(before.intent_route_calls),
        "proposal_acceptances": after.intent_proposal_acceptances.saturating_sub(before.intent_proposal_acceptances),
        "resolution_acceptances": after.intent_resolution_acceptances.saturating_sub(before.intent_resolution_acceptances),
        "compile_attempts": after.intent_compile_attempts.saturating_sub(before.intent_compile_attempts),
        "compile_successes": after.intent_compile_successes.saturating_sub(before.intent_compile_successes),
        "commits": after.intent_commits.saturating_sub(before.intent_commits),
        "rollbacks": after.intent_rollbacks.saturating_sub(before.intent_rollbacks),
        "conflicts": after.intent_conflicts.saturating_sub(before.intent_conflicts),
        "stale_revision_rejections": after.intent_stale_revision_rejections.saturating_sub(before.intent_stale_revision_rejections),
        "extraction_failures": after.intent_extraction_failures.saturating_sub(before.intent_extraction_failures),
        "fallback_routes": fallback_routes
    })
}

fn intent_stage(status: &IntentRecipeStatusV1) -> &'static str {
    match status {
        IntentRecipeStatusV1::Empty { .. } => "empty",
        IntentRecipeStatusV1::AwaitingDecision { .. } => "awaiting_decision",
        IntentRecipeStatusV1::PreviewReady { .. } => "preview_ready",
    }
}

fn intent_revision(status: &IntentRecipeStatusV1) -> u64 {
    match status {
        IntentRecipeStatusV1::Empty { expected_revision } => *expected_revision,
        IntentRecipeStatusV1::AwaitingDecision {
            workspace_revision, ..
        } => *workspace_revision,
        IntentRecipeStatusV1::PreviewReady { receipt, .. } => receipt.intent_revision,
    }
}

fn failure(result: &ToolResult) -> Option<&ToolFailure> {
    result.failure()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use design_harness::{BurstOutcome, DesignSession, ResourceBindingMap, SessionConfig};

    use super::{
        intent_report, report, turn_report, IntentPersistenceEvidence, IntentReportMetadata,
        TurnReportInput,
    };

    #[test]
    fn intent_report_uses_v3_oracle_free_contract_and_null_model_without_calls() {
        let session = DesignSession::with_intent_recipe((), ResourceBindingMap::default());
        let session_config = SessionConfig::default();
        let document = intent_report(
            &session,
            Vec::new(),
            Duration::from_millis(9),
            IntentPersistenceEvidence {
                store_writes: 0,
                connection_reopens: 0,
                final_generation: 0,
            },
            IntentReportMetadata {
                gateway_id: "test-gateway",
                declared_context_tokens: 16_384,
                source_commit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                source_dirty: false,
                build_source_commit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                build_source_dirty: false,
                binary_sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                run_id: "run-1",
                run_order: 1,
                started_at_unix_ms: 10,
                ended_at_unix_ms: 19,
                requested_model: "gemma4:12b-mlx",
                session_config: &session_config,
            },
        );

        assert_eq!(document["schema_version"], 3);
        assert_eq!(document["mode"], "intent_recipe");
        assert_eq!(document["served_model"], serde_json::Value::Null);
        assert_eq!(document["oracle"]["enabled"], false);
        assert_eq!(document["oracle"]["injected_control_calls"], 0);
        assert_eq!(document["final_intent"]["status"], "empty");
        assert_eq!(document["final_intent"]["public_status"]["status"], "empty");
        assert_eq!(document["final_intent"]["receipt"], serde_json::Value::Null);
        assert!(document["final_intent"]["binding_fingerprint"].is_string());
        assert_eq!(document["persistence"]["connection_reopen_count"], 0);
        assert_eq!(document["persistence"]["roundtrip_verified"], false);
    }

    #[tokio::test]
    async fn report_separates_actual_gate_stamps_from_postchecks() {
        let session = DesignSession::new(());
        let before = session.draft().clone();
        let before_observability = session.observability().clone();
        let outcome = BurstOutcome::NeedsInput {
            question: "Need a name?".to_string(),
        };
        let turn = turn_report(TurnReportInput {
            id: "clarify",
            input: "Build it",
            before: &before,
            after: session.draft(),
            observability_before: &before_observability,
            observability_after: session.observability(),
            outcome: &outcome,
            elapsed: Duration::from_millis(7),
            injected_control_calls_before: 0,
            injected_control_calls_after: 0,
            delegated_model_calls_before: 0,
            delegated_model_calls_after: 0,
        });

        let document = report(
            &session,
            vec![turn],
            Duration::from_millis(17),
            1,
            "adaptive",
            0,
            0,
        )
        .await;

        assert_eq!(document["schema_version"], 2);
        assert_eq!(document["outcome"], "needs_input");
        assert_eq!(document["turns"][0]["question"], "Need a name?");
        assert_eq!(document["turns"][0]["elapsed_ms"], 7);
        assert_eq!(document["elapsed_ms"], 17);
        assert_eq!(document["input_schema_version"], 1);
        assert_eq!(document["mode"], "adaptive");
        assert_eq!(document["injected_control_calls"], 0);
        assert_eq!(document["delegated_model_calls"], 0);
        assert!(document["postcheck"]["validate_passed"].is_boolean());
        assert!(document["postcheck"]["simulate_passed"].is_boolean());
        assert_eq!(document["actual_gates"]["validation_current"], false);
        assert_eq!(session.draft().validated_revision, None);
        assert_eq!(session.draft().simulated_revision, None);
    }

    #[test]
    fn turn_report_records_revision_and_observability_deltas() {
        let before_session = DesignSession::new(());
        let before = before_session.draft().clone();
        let mut after_session = DesignSession::new(());
        after_session.draft_mut().draft_revision = 2;
        let mut before_observability = before_session.observability().clone();
        before_observability.model_calls = 3;
        let mut after_observability = before_observability.clone();
        after_observability.model_calls = 5;
        after_observability.tool_calls = 4;
        after_observability
            .distinct_mutation_tools
            .insert("add_modal".to_string());
        let outcome = BurstOutcome::Ready {
            summary: "Ready".to_string(),
        };

        let document = turn_report(TurnReportInput {
            id: "turn-2",
            input: "Continue",
            before: &before,
            after: after_session.draft(),
            observability_before: &before_observability,
            observability_after: &after_observability,
            outcome: &outcome,
            elapsed: Duration::from_millis(11),
            injected_control_calls_before: 1,
            injected_control_calls_after: 2,
            delegated_model_calls_before: 7,
            delegated_model_calls_after: 7,
        });

        assert_eq!(document["draft_revision_before"], 0);
        assert_eq!(document["draft_revision_after"], 2);
        assert_eq!(document["draft_changed"], true);
        assert_eq!(document["observability_delta"]["model_calls"], 2);
        assert_eq!(document["observability_delta"]["tool_calls"], 4);
        assert_eq!(document["injected_control_calls"], 1);
        assert_eq!(document["delegated_model_calls"], 0);
        assert_eq!(document["observability_delta"]["plan_submissions"], 0);
        assert_eq!(document["observability_delta"]["plan_commits"], 0);
        assert_eq!(
            document["observability_delta"]["distinct_mutation_tools"][0],
            "add_modal"
        );
    }
}
