use std::time::Duration;

use design_harness::{
    simulate_draft, validate_draft, BurstOutcome, DesignSession, ToolFailure, ToolResult,
};
use serde_json::{json, Value};

pub async fn report<C>(
    session: &DesignSession<C>,
    outcome: &BurstOutcome,
    elapsed: Duration,
) -> Value {
    let (outcome_name, message, question, completed, halt_code, last_error) = match outcome {
        BurstOutcome::AwaitingHuman { question } => (
            "awaiting_human",
            question.as_str(),
            Some(question.as_str()),
            false,
            None,
            None,
        ),
        BurstOutcome::Completed { summary } => {
            ("completed", summary.as_str(), None, true, None, None)
        }
        BurstOutcome::Halted(report) => (
            "halted",
            report.message.as_str(),
            None,
            false,
            Some(report.code.as_str()),
            report.last_error.as_ref(),
        ),
    };
    let draft = session.draft();
    let mut final_draft = draft.clone();
    let final_validation = validate_draft(&mut final_draft);
    let final_simulation = if final_validation.is_ok() {
        Some(simulate_draft(&mut final_draft).await)
    } else {
        None
    };
    let failure_signatures = &session.observability().failure_signatures;
    let repeated_error_signatures = failure_signatures
        .iter()
        .filter_map(|(signature, count)| (*count > 1).then_some(signature.clone()))
        .collect::<Vec<_>>();
    let max_repeat_count = failure_signatures.values().copied().max().unwrap_or(0);

    json!({
        "schema_version": 1,
        "outcome": outcome_name,
        "completed": completed,
        "message": message,
        "question": question,
        "halt_code": halt_code,
        "draft_revision": draft.draft_revision,
        "validated_revision": draft.validated_revision,
        "simulated_revision": draft.simulated_revision,
        "validation_current": draft.validated_revision == Some(draft.draft_revision),
        "simulation_current": draft.simulated_revision == Some(draft.draft_revision),
        "draft": draft.summary(),
        "ruleset": &draft.ruleset,
        "observability": session.observability(),
        "final_validate_passed": final_validation.is_ok(),
        "final_validate_error": failure(&final_validation),
        "final_simulate_passed": final_simulation.as_ref().is_some_and(ToolResult::is_ok),
        "final_simulate_error": final_simulation.as_ref().and_then(failure),
        "repeated_error_signatures": repeated_error_signatures,
        "max_repeat_count": max_repeat_count,
        "elapsed_ms": elapsed.as_millis(),
        "last_error": last_error
    })
}

fn failure(result: &ToolResult) -> Option<&ToolFailure> {
    result.failure()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use design_harness::{BurstOutcome, DesignSession};

    use super::report;

    #[tokio::test]
    async fn report_rechecks_final_gates_without_mutating_the_session() {
        let session = DesignSession::new(());
        let outcome = BurstOutcome::AwaitingHuman {
            question: "Need a name?".to_string(),
        };

        let document = report(&session, &outcome, Duration::from_millis(17)).await;

        assert_eq!(document["schema_version"], 1);
        assert_eq!(document["outcome"], "awaiting_human");
        assert_eq!(document["question"], "Need a name?");
        assert_eq!(document["elapsed_ms"], 17);
        assert!(document["final_validate_passed"].is_boolean());
        assert!(document["final_simulate_passed"].is_boolean());
        assert_eq!(session.draft().validated_revision, None);
        assert_eq!(session.draft().simulated_revision, None);
    }
}
