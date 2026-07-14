use crate::errors::{StructuredError, ToolResult};
use crate::turn::{
    execute_plan_atomically, AdaptivePhase, ExecutionRecord, RequestedOutcome, TurnIntent,
};

use super::routing::is_mutation_tool;
use super::{BurstOutcome, DesignSession, LimitKind};

impl<C: crate::llm::LlmClient> DesignSession<C> {
    pub(super) async fn run_automatic_planned_execution(&mut self) -> Option<BurstOutcome> {
        if !self.planned_enabled {
            return None;
        }
        let brief = self
            .adaptive_turn
            .as_ref()
            .and_then(|state| state.brief.as_ref())
            .filter(|brief| {
                matches!(brief.intent, TurnIntent::Build | TurnIntent::Modify)
                    && !brief.requirements.is_empty()
            })
            .cloned()?;
        let remaining_calls = self
            .config
            .max_tool_calls
            .saturating_sub(self.turn_tool_calls());
        match execute_plan_atomically(&self.draft, &brief, remaining_calls).await {
            Ok(execution) => {
                self.record_execution_records(&execution.records);
                self.planned_root_draft = brief.verification.validate.then(|| self.draft.clone());
                self.draft = execution.draft;
                self.last_error = None;
                let phase = if brief.verification.validate {
                    AdaptivePhase::Verify
                } else if brief.requested_outcome == RequestedOutcome::ValidatedPreview {
                    AdaptivePhase::Preview
                } else {
                    AdaptivePhase::Reply
                };
                if let Some(state) = self.adaptive_turn.as_mut() {
                    state.scoped_revision = Some(self.draft.draft_revision);
                    state.previewed_revision = None;
                    state.phase = phase;
                }
                let outcome = self.run_automatic_adaptive_phases().await;
                if self
                    .adaptive_turn
                    .as_ref()
                    .is_some_and(|state| state.phase == AdaptivePhase::Reply)
                {
                    self.planned_root_draft = None;
                    self.observability.plan_commits =
                        self.observability.plan_commits.saturating_add(1);
                }
                outcome
            }
            Err(failure) => {
                let exhausted = failure.error.code == "PLAN_TOOL_CALL_LIMIT";
                let only_successes = failure.records.iter().all(|record| record.result.is_ok());
                self.observability.plan_execution_failures =
                    self.observability.plan_execution_failures.saturating_add(1);
                self.observability.plan_rollbacks =
                    self.observability.plan_rollbacks.saturating_add(1);
                if is_plan_conflict_code(&failure.error.code) {
                    self.observability.plan_conflicts =
                        self.observability.plan_conflicts.saturating_add(1);
                }
                self.record_execution_records(&failure.records);
                if only_successes {
                    let result = ToolResult::failure_from(&self.draft, failure.error.clone());
                    self.record_failure(None, &result);
                } else {
                    self.last_error = Some(failure.error.clone());
                }
                if exhausted {
                    return Some(self.halt(
                        "TOOL_CALL_LIMIT_EXHAUSTED",
                        "The atomic turn plan exhausted its executed tool call budget",
                        Some(LimitKind::ToolCalls),
                    ));
                }
                if self.planned_correction_remaining == 0 {
                    return Some(self.halt(
                        "PLAN_REPAIR_FAILED",
                        "The single automatic turn-plan repair failed",
                        None,
                    ));
                }
                self.planned_correction_remaining -= 1;
                self.reset_planned_frontier_corrections();
                if let Some(brief) = self
                    .adaptive_turn
                    .as_mut()
                    .and_then(|state| state.brief.as_mut())
                {
                    brief.requirements.clear();
                }
                self.add_planned_nudge("set_turn_plan");
                None
            }
        }
    }

    fn record_execution_records(&mut self, records: &[ExecutionRecord]) {
        self.observability.plan_compiled_tool_calls = self
            .observability
            .plan_compiled_tool_calls
            .saturating_add(records.len());
        for record in records {
            self.record_tool_call();
            if record.result.is_ok() && is_mutation_tool(&record.name) {
                self.observability
                    .distinct_mutation_tools
                    .insert(record.name.clone());
                *self
                    .observability
                    .mutation_tool_calls
                    .entry(record.name.clone())
                    .or_default() += 1;
            }
            self.record_failure(Some(&record.name), &record.result);
        }
    }

    pub(super) fn recover_planned_phase_failure(
        &mut self,
        error: StructuredError,
    ) -> Option<Result<bool, BurstOutcome>> {
        if !self.rollback_planned_root(error) {
            return None;
        }
        if self.planned_correction_remaining == 0 {
            return Some(Err(self.halt(
                "PLAN_REPAIR_FAILED",
                "The single automatic turn-plan repair failed",
                None,
            )));
        }
        self.planned_correction_remaining -= 1;
        self.reset_planned_frontier_corrections();
        self.add_planned_nudge("set_turn_plan");
        Some(Ok(false))
    }

    pub(super) fn rollback_planned_root(&mut self, error: StructuredError) -> bool {
        let Some(root) = self.planned_root_draft.take() else {
            return false;
        };
        self.observability.plan_execution_failures =
            self.observability.plan_execution_failures.saturating_add(1);
        self.observability.plan_rollbacks = self.observability.plan_rollbacks.saturating_add(1);
        if is_plan_conflict_code(&error.code) {
            self.observability.plan_conflicts = self.observability.plan_conflicts.saturating_add(1);
        }
        self.draft = root;
        self.plan_assembly = None;
        self.last_error = Some(error);
        if let Some(state) = self.adaptive_turn.as_mut() {
            if let Some(brief) = state.brief.as_mut() {
                brief.requirements.clear();
            }
            state.scoped_revision = None;
            state.previewed_revision = None;
            state.phase = AdaptivePhase::Build;
        }
        true
    }
}

pub(super) fn is_plan_conflict_code(code: &str) -> bool {
    code.contains("CONFLICT")
}
