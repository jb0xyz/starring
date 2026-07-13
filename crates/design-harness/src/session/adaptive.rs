use crate::errors::{StructuredError, ToolResult};
use crate::llm::{Message, ToolCall};
use crate::tools::dispatch_tool;
use crate::turn::{check_scope, AdaptivePhase, SimulationProfile};

use super::routing::{definitions_named, is_mutation_tool};
use super::{
    is_genuine_human_message, BurstOutcome, DesignSession, LimitKind, RepairKind, RepairState,
};

impl<C> DesignSession<C> {
    pub(super) fn advance_adaptive_after_draft_tool(&mut self, name: &str, succeeded: bool) {
        if !succeeded {
            return;
        }
        let Some(state) = self.adaptive_turn.as_mut() else {
            return;
        };
        if is_mutation_tool(name) {
            state.scoped_revision = None;
            state.previewed_revision = None;
        } else if name == "validate_draft" {
            let scope_ok = state
                .brief
                .as_ref()
                .is_some_and(|brief| check_scope(&self.draft, brief).ok);
            if !scope_ok {
                state.phase = AdaptivePhase::Build;
                return;
            }
            state.scoped_revision = Some(self.draft.draft_revision);
            if state.phase == AdaptivePhase::Verify {
                let simulation = state
                    .brief
                    .as_ref()
                    .map_or(SimulationProfile::None, |brief| {
                        brief.verification.simulation
                    });
                state.phase = if simulation == SimulationProfile::StudyRoom {
                    AdaptivePhase::Simulate
                } else {
                    AdaptivePhase::Preview
                };
            }
        } else if name == "simulate_draft" && state.phase == AdaptivePhase::Simulate {
            state.phase = AdaptivePhase::Preview;
        }
    }

    pub(super) fn append_phase_transition_not_executed(&mut self, calls: &[ToolCall]) {
        for call in calls {
            let result = ToolResult::failure_from(
                &self.draft,
                StructuredError::new(
                    "NOT_EXECUTED_AFTER_PHASE_TRANSITION",
                    "tool.batch",
                    "This tool call was not executed because the adaptive turn phase changed",
                    "Continue with the tools routed for the new turn phase",
                ),
            );
            self.messages
                .push(Message::tool(call.id.clone(), result.as_json()));
        }
    }
}

impl<C: crate::llm::LlmClient> DesignSession<C> {
    fn automatic_call(&self, name: &str) -> ToolCall {
        let turn = self.turn_state.as_ref().map_or(0, |state| state.sequence);
        ToolCall {
            id: format!("harness-{turn}-{}-{name}", self.draft.draft_revision),
            name: name.to_string(),
            arguments: "{}".to_string(),
        }
    }

    async fn run_automatic_gate(&mut self, name: &str) -> Result<bool, BurstOutcome> {
        if self.turn_tool_calls() >= self.config.max_tool_calls {
            self.rollback_planned_root(StructuredError::new(
                "PLAN_TOOL_CALL_LIMIT",
                "turn.plan",
                "The planned verification path exhausted the tool call budget",
                "Retry the request with a smaller plan or a larger tool call budget",
            ));
            return Err(self.halt(
                "TOOL_CALL_LIMIT_EXHAUSTED",
                "The session exhausted its executed tool call budget",
                Some(LimitKind::ToolCalls),
            ));
        }
        self.record_tool_call();
        let call = self.automatic_call(name);
        let request_tools = definitions_named(&self.tools, &[name]);
        let result = dispatch_tool(&mut self.draft, name, &call.arguments).await;
        if result.is_ok() {
            self.last_error = None;
        }
        self.advance_adaptive_after_draft_tool(name, result.is_ok());
        self.record_failure(Some(name), &result);
        if result.is_ok() {
            return Ok(true);
        }
        let planned_error = result.failure().map(|failure| {
            StructuredError::new(
                failure.code.clone(),
                failure.location.clone(),
                failure.message.clone(),
                failure.hint.clone(),
            )
        });
        if let Some(error) = planned_error {
            if let Some(recovery) = self.recover_planned_phase_failure(error) {
                return recovery;
            }
        }
        if self.turn_gate_failures() >= self.config.max_gate_failures {
            return Err(self.halt(
                "GATE_FAILURE_LIMIT_EXHAUSTED",
                "The session exhausted its validation and simulation failure budget",
                Some(LimitKind::GateFailures),
            ));
        }
        if let Some(state) = self.repair_state.clone() {
            let error = result.failure().map_or_else(
                || {
                    StructuredError::new(
                        "REPAIR_GATE_FAILED",
                        format!("repair.{name}"),
                        "The automatic repair verification gate failed",
                        "Escalate to a human before continuing the design",
                    )
                },
                |failure| {
                    StructuredError::new(
                        failure.code.clone(),
                        failure.location.clone(),
                        failure.message.clone(),
                        failure.hint.clone(),
                    )
                },
            );
            return Err(self.fail_repair(state.ticket().clone(), error, false));
        }
        if let Some(ticket) = self.root_repair_ticket(&call, &result, &request_tools) {
            self.append_repair_directive(&ticket);
            self.repair_state = Some(RepairState::AwaitingAttempt(ticket));
        }
        Ok(false)
    }

    async fn run_automatic_preview(&mut self) -> Result<bool, BurstOutcome> {
        if self.turn_tool_calls() >= self.config.max_tool_calls {
            self.rollback_planned_root(StructuredError::new(
                "PLAN_TOOL_CALL_LIMIT",
                "turn.plan",
                "The planned preview path exhausted the tool call budget",
                "Retry the request with a smaller plan or a larger tool call budget",
            ));
            return Err(self.halt(
                "TOOL_CALL_LIMIT_EXHAUSTED",
                "The session exhausted its executed tool call budget",
                Some(LimitKind::ToolCalls),
            ));
        }
        self.record_tool_call();
        let (result, _) = self.dispatch_control_tool("render_preview", "{}");
        if result.is_ok() {
            self.last_error = None;
            return Ok(true);
        }
        self.record_failure(Some("render_preview"), &result);
        let error = result.failure().map_or_else(
            || {
                StructuredError::new(
                    "AUTOMATIC_PREVIEW_FAILED",
                    "tool.render_preview",
                    "The deterministic preview step failed",
                    "Inspect the validated Draft and preview state before retrying",
                )
            },
            |failure| {
                StructuredError::new(
                    failure.code.clone(),
                    failure.location.clone(),
                    failure.message.clone(),
                    failure.hint.clone(),
                )
            },
        );
        if let Some(recovery) = self.recover_planned_phase_failure(error.clone()) {
            return recovery;
        }
        self.last_error = Some(error);
        Err(self.halt(
            "AUTOMATIC_PREVIEW_FAILED",
            "The deterministic preview step failed",
            None,
        ))
    }

    pub(super) async fn run_automatic_adaptive_phases(&mut self) -> Option<BurstOutcome> {
        if !self.adaptive_enabled || self.repair_state.is_some() {
            return None;
        }
        loop {
            let phase = self.adaptive_turn.as_ref().map(|state| state.phase);
            match phase {
                Some(AdaptivePhase::Verify) => {
                    match self.run_automatic_gate("validate_draft").await {
                        Ok(true) => {}
                        Ok(false) => return None,
                        Err(outcome) => return Some(outcome),
                    }
                }
                Some(AdaptivePhase::Simulate) => {
                    match self.run_automatic_gate("simulate_draft").await {
                        Ok(true) => {}
                        Ok(false) => return None,
                        Err(outcome) => return Some(outcome),
                    }
                }
                Some(AdaptivePhase::Preview) => match self.run_automatic_preview().await {
                    Ok(true) => {}
                    Ok(false) => return None,
                    Err(outcome) => return Some(outcome),
                },
                _ => return None,
            }
        }
    }

    pub(super) async fn run_automatic_repair_verification(&mut self) -> Option<BurstOutcome> {
        if !self.adaptive_enabled {
            return None;
        }
        loop {
            let Some(state) = self.repair_state.clone() else {
                return self.run_automatic_adaptive_phases().await;
            };
            let ticket = state.ticket().clone();
            match state {
                RepairState::VerifyValidation(_) => {
                    match self.run_automatic_gate("validate_draft").await {
                        Ok(true) => match ticket.kind {
                            RepairKind::Validation => {
                                self.repair_state = None;
                                self.last_error = None;
                                self.observability.repair_successes += 1;
                            }
                            RepairKind::Simulation => {
                                self.repair_state = Some(RepairState::VerifySimulation(ticket));
                            }
                            RepairKind::Arguments => {
                                let error = StructuredError::new(
                                    "REPAIR_STATE_INVALID",
                                    "repair.state",
                                    "Argument repair entered validation verification",
                                    "Escalate to a human and restart the repair",
                                );
                                return Some(self.fail_repair(ticket, error, true));
                            }
                        },
                        Ok(false) => return None,
                        Err(outcome) => return Some(outcome),
                    }
                }
                RepairState::VerifySimulation(_) => {
                    match self.run_automatic_gate("simulate_draft").await {
                        Ok(true) => {
                            self.repair_state = None;
                            self.last_error = None;
                            self.observability.repair_successes += 1;
                        }
                        Ok(false) => return None,
                        Err(outcome) => return Some(outcome),
                    }
                }
                _ => return None,
            }
        }
    }
}

pub(super) fn simulation_profile_for_current_human_turn(messages: &[Message]) -> SimulationProfile {
    let has_study_room = messages
        .iter()
        .rev()
        .find(|message| is_genuine_human_message(message))
        .is_some_and(|message| message.content.to_ascii_lowercase().contains("studyroom"));
    if has_study_room {
        SimulationProfile::StudyRoom
    } else {
        SimulationProfile::None
    }
}
