use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::draft::{Draft, DraftSummary};
use crate::errors::{StructuredError, ToolResult};
use crate::llm::{LlmClient, LlmResponse, Message, MessageRole, ToolCall};
use crate::tools::{dispatch_tool, ToolDefinition};
use crate::turn::{
    assign_turn_plan_repeat_targets, check_scope, derive_turn_plan_instance_manifests,
    merge_turn_plan_extension_action_lanes, missing_turn_plan_instance_registration_owners,
    normalize_turn_plan, parse_empty_control, parse_finish_turn, parse_planned_turn_brief,
    parse_turn_brief, parse_turn_plan, parse_turn_plan_packet_scoped, parse_turn_plan_review,
    parse_turn_plan_review_oracle, rebase_turn_plan_outline_ids, render_preview,
    resolve_turn_plan_created_reference_kinds, resolve_turn_plan_extension_outline_parent_owners,
    resolve_turn_plan_outline_parent_owners, resolve_turn_plan_owners,
    resolve_turn_plan_response_lifecycle_actions, resolve_turn_plan_unique_instance_aliases,
    validate_turn_plan_new_rule_action_coverage, AdaptivePhase, AdaptiveTurnState, FinishTurnKind,
    PlanOp, PlanOutlineItem, RequestedOutcome, ScopeAction, ScopeRequirement, SimulationProfile,
    TurnBrief, TurnIntent, TurnPlanSubmission, MAX_PLAN_GOAL_TOTAL_CHARS, MAX_PLAN_ITEMS,
    MAX_PLAN_PACKET_ITEMS,
};

mod adaptive;
mod context;
mod frontier;
mod intent_routing;
mod repair;
mod routing;
mod snapshot;

use adaptive::simulation_profile_for_current_human_turn;
use context::compact_text;
use intent_routing::IntentRecipeRuntime;
pub use intent_routing::{
    IntentDecisionSourceV2, IntentFallbackKind, IntentFallbackV1, IntentRecipeReceiptV1,
    IntentRecipeStatusV1, IntentRouteDecisionKindV2, IntentRouteDecisionV2, PinnedIntentRecipeV2,
    INTENT_ADJUDICATOR_VERSION_V2,
};
use repair::is_argument_failure;
use routing::{
    all_tool_definitions, is_control_tool, is_mutation_tool, legacy_tool_definitions,
    tool_is_available,
};
use snapshot::validate_snapshot;

pub const DEFAULT_SYSTEM_PROMPT: &str = "Design Discord automations only with the provided tools. Never touch live Discord, publish, deploy, or activate. At the start of every human turn call set_turn_brief with only a concise intent, objective, requested outcome, assumptions, and whether validation is required. Classify adding new Draft structure as build and changing or removing existing structure as modify. Build turns use set_turn_plan for exact ordered semantic requirements. A modify turn may offer Draft-legal update or remove tools beside set_turn_plan; use set_turn_plan when the requested work is actually additive, and use update or remove only for a true edit or removal. After attempting set_turn_plan, repair only through set_turn_plan. The harness executes accepted plans deterministically. The harness deterministically enables StudyRoom simulation from the exact human message; set validate to true whenever the human explicitly says StudyRoom. Use discussion or brainstorm for design conversation or a missing structural decision, then finish_turn with one focused question or response without changing the Draft. For build or modify, continue in the same turn with the staged design tools and call check_turn_scope after the requested Draft change is complete. For an unchanged existing Draft verification turn, use inspect with validated_preview and validate true; the harness scopes the current revision and automatically validates, runs the selected simulation, and renders the preview without a mutation. The harness then automatically runs requested validation, harness-selected simulation, and preview steps. When finish_turn is the only available tool, call it with kind ready and summarize the result. Use safe defaults only for non-blocking details. Actual edits and removals must use update or remove tools instead of creating duplicates. Reference created resources by alias. Never ask whether to continue, stop, validate, or review. Legacy QUESTION, PROGRESSED, and READY text are accepted only for compatibility; prefer finish_turn.";
pub(super) const PLANNED_SYSTEM_PROMPT: &str = "Design Discord automations only with the provided tools. Never touch live Discord, publish, deploy, or activate. At the start of every human turn call set_turn_brief with only a strategy, concise objective, requested outcome, assumptions, and whether validation is required. Use additive_plan whenever the request adds any new panel, button, modal, rule, or action, even when it preserves and extends an existing Draft. Use edit_existing only when every requested mutation updates or removes a target that already exists. For additive work call set_turn_plan with a complete ordered outline containing exactly the missing mutations needed for the current request; omit every existing object or action mentioned only to preserve it. Include one op and a goal with the literal keys, values, references, and semantics needed for every new requested object and action. Preserve duplicate ops and the requested action order. Put every new top-level button immediately after its persistent panel and every action immediately after its rule. Every outline step must include owner. Use draft for panel, modal, and rule; use the parent panel key for a button; use the parent rule key for every action, never a role, channel, panel, modal, action target, or resource key. Panel, button, and modal ops declare one persistent object. The button op is only for a persistent panel declared by panel. A post_panel action contains its complete embedded button list inside its packet, so never add separate button ops for buttons embedded in post_panel. A rule op declares only a trigger and never includes an action. Every rule action is a separate op: open_modal opens a modal, respond_ephemeral sends one response, defer_ephemeral defers, create_role creates a role, create_channel creates a channel, upsert_overwrite sets one permission target, grant_role grants a role, post_panel posts one panel, register_instance declares only its instance key and kind, edit_response edits the response, and teardown_instance removes the current instance. The harness derives the complete canonical register_instance manifest from created resources; do not add another op or enumerate manifest resources. The harness assigns stable ids and then exposes fill_turn_plan_packet with a small exact schema for the current packet. For each currently exposed packet, call fill_turn_plan_packet exactly once and fill every required property. While that tool remains exposed, the overall plan is incomplete, so continue with the newly exposed packet until the harness reports completion. The harness injects explicit or backward-compatible inferred owners, assembles, validates, and executes the completed plan atomically. An edit_existing turn may offer Draft-legal update or remove tools; never use those tools to add a missing key and never mix them with a plan. After a plan-path failure use only the replanning frontier exposed by the harness. The harness deterministically enables StudyRoom simulation from the exact human message; set validate to true whenever the human explicitly says StudyRoom. Use discussion or brainstorm for design conversation or a missing structural decision, then finish_turn with one focused question or response without changing the Draft. For an unchanged existing Draft verification turn, use inspect with validated_preview and validate true; the harness scopes the current revision and automatically validates, runs the selected simulation, and renders the preview without a mutation. The harness automatically runs requested validation, harness-selected simulation, and preview steps. When finish_turn is the only available tool, call it with kind ready and summarize the result. Use safe defaults only for non-blocking details. Actual edits and removals must use update or remove tools instead of creating duplicates. Reference created resources by stable key alias, never the rendered role or channel name. Never ask whether to continue, stop, validate, or review. Legacy QUESTION, PROGRESSED, and READY text are accepted only for compatibility; prefer finish_turn.";

const NUDGE: &str = "Call a design tool to change the Draft; use QUESTION: only for a blocking decision; use PROGRESSED: after useful changes when another user turn is appropriate; use READY: only after validate_draft passes on the current revision.";
const REPAIR_REQUIRED_PREFIX: &str = "REPAIR_REQUIRED:";
const COVERAGE_REVIEW_PREFIX: &str = "TURN_PLAN_COVERAGE_REVIEW:";
const PACKET_CONTINUE_PREFIX: &str = "TURN_PLAN_PACKET_CONTINUE:";
const PLAN_FRONTIER_RETRY_PREFIX: &str = "TURN_PLAN_FRONTIER_RETRY:";
const PLAN_REVIEW_RETRY_PREFIX: &str = "TURN_PLAN_REVIEW_RETRY:";
const MAX_INTENT_MEMORY_ITEMS: usize = 6;
const MAX_BRIEF_MEMORY_ITEMS: usize = 3;
const MAX_INTENT_MEMORY_CHARS: usize = 240;
const MAX_ERROR_MEMORY_CHARS: usize = 360;
const MAX_REVIEW_RETRY_ERROR_FIELD_CHARS: usize = 448;

pub const SESSION_SNAPSHOT_VERSION: u32 = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionConfig {
    pub max_model_calls: usize,
    pub max_tool_calls: usize,
    pub max_gate_failures: usize,
    pub context_char_budget: usize,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_model_calls: 12,
            max_tool_calls: 24,
            max_gate_failures: 4,
            context_char_budget: 44_000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitKind {
    ModelCalls,
    ToolCalls,
    GateFailures,
    ContextChars,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observability {
    pub model_calls: usize,
    pub tool_calls: usize,
    pub distinct_mutation_tools: BTreeSet<String>,
    #[serde(default)]
    pub mutation_tool_calls: BTreeMap<String, usize>,
    pub clarification_count: usize,
    pub validation_failures: usize,
    pub simulation_failures: usize,
    pub failure_signatures: BTreeMap<String, usize>,
    pub repeated_errors: usize,
    pub repair_attempts: usize,
    pub repair_successes: usize,
    pub repair_failures: usize,
    pub repair_escalations: usize,
    pub nudge_count: usize,
    #[serde(default)]
    pub plan_submissions: usize,
    #[serde(default)]
    pub plan_acceptances: usize,
    #[serde(default)]
    pub planned_requirements: usize,
    #[serde(default)]
    pub plan_compiled_tool_calls: usize,
    #[serde(default)]
    pub plan_execution_failures: usize,
    #[serde(default)]
    pub plan_rollbacks: usize,
    #[serde(default)]
    pub plan_commits: usize,
    #[serde(default)]
    pub plan_conflicts: usize,
    #[serde(default)]
    pub intent_route_calls: usize,
    #[serde(default)]
    pub intent_proposal_acceptances: usize,
    #[serde(default)]
    pub intent_resolution_acceptances: usize,
    #[serde(default)]
    pub intent_compile_attempts: usize,
    #[serde(default)]
    pub intent_compile_successes: usize,
    #[serde(default)]
    pub intent_commits: usize,
    #[serde(default)]
    pub intent_rollbacks: usize,
    #[serde(default)]
    pub intent_conflicts: usize,
    #[serde(default)]
    pub intent_stale_revision_rejections: usize,
    #[serde(default)]
    pub intent_extraction_failures: usize,
    #[serde(default)]
    pub intent_fallback_routes: BTreeMap<String, usize>,
    #[serde(default)]
    pub intent_compiled_operations: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HaltReport {
    pub code: String,
    pub message: String,
    pub exhausted_limit: Option<LimitKind>,
    pub draft: DraftSummary,
    pub last_error: Option<StructuredError>,
    pub observability: Observability,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BurstOutcome {
    NeedsInput { question: String },
    Progressed { summary: String },
    Ready { summary: String },
    Routed { fallback: IntentFallbackV1 },
    Halted(Box<HaltReport>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnPhase {
    Active,
    NeedsInput,
    Progressed,
    Ready,
    Routed,
    Halted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnState {
    pub sequence: u64,
    pub phase: TurnPhase,
    pub human_message: String,
    pub started_revision: u64,
    pub current_revision: u64,
    pub model_calls: usize,
    pub tool_calls: usize,
    pub gate_failures: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairKind {
    Arguments,
    Validation,
    Simulation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairTicket {
    pub kind: RepairKind,
    pub original_call: ToolCall,
    pub original_error: StructuredError,
    pub expected_argument_schema: Option<Value>,
    pub allowed_repair_tools: Vec<String>,
    pub verification_path: Vec<String>,
    pub root_revision: u64,
    pub attempts_remaining: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stage", content = "ticket", rename_all = "snake_case")]
pub enum RepairState {
    AwaitingAttempt(RepairTicket),
    VerifyValidation(RepairTicket),
    VerifySimulation(RepairTicket),
    Failed(RepairTicket),
}

impl RepairState {
    fn ticket(&self) -> &RepairTicket {
        match self {
            Self::AwaitingAttempt(ticket)
            | Self::VerifyValidation(ticket)
            | Self::VerifySimulation(ticket)
            | Self::Failed(ticket) => ticket,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionSnapshot {
    pub schema_version: u32,
    pub draft: Draft,
    pub messages: Vec<Message>,
    pub observability: Observability,
    pub last_error: Option<StructuredError>,
    pub prose_nudged: bool,
    pub repair_state: Option<RepairState>,
    #[serde(default)]
    pub turn_state: Option<TurnState>,
    #[serde(default)]
    pub adaptive_turn: Option<AdaptiveTurnState>,
    #[serde(default)]
    pub adaptive_enabled: bool,
    #[serde(default)]
    pub brief_history: Vec<TurnBrief>,
    #[serde(default)]
    pub(crate) intent_recipe: Option<intent_routing::IntentRecipeSessionSnapshotV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum SessionSnapshotError {
    #[error("unsupported session snapshot version {found}; expected {expected}")]
    UnsupportedVersion { expected: u32, found: u32 },
    #[error("invalid session snapshot: {message}")]
    InvalidInvariant { message: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PlanAssembly {
    root_revision: u64,
    outline: Vec<PlanOutlineItem>,
    packet_ends: Vec<usize>,
    review_pending: bool,
    coverage_extension_pending: bool,
    review_coverage_extended: bool,
    structural_coverage_extended: bool,
    coverage_issue_summary: Option<String>,
    coverage_obligation: Option<CoverageObligation>,
    packet_refined: bool,
    packet_refinement_pending: bool,
    cursor: usize,
    requirements: Vec<ScopeRequirement>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum CoverageObligation {
    ReviewerMissing,
    InstanceRegistrations(BTreeSet<String>),
}

impl PlanAssembly {
    fn new(root_revision: u64, outline: Vec<PlanOutlineItem>) -> Self {
        let packet_ends = plan_packet_ends(&outline);
        Self {
            root_revision,
            outline,
            packet_ends,
            review_pending: false,
            coverage_extension_pending: false,
            review_coverage_extended: false,
            structural_coverage_extended: false,
            coverage_issue_summary: None,
            coverage_obligation: None,
            packet_refined: false,
            packet_refinement_pending: false,
            cursor: 0,
            requirements: Vec::new(),
        }
    }

    fn frontier_name(&self) -> &'static str {
        if self.coverage_extension_pending {
            "set_turn_plan"
        } else if self.review_pending {
            "review_turn_plan"
        } else {
            "fill_turn_plan_packet"
        }
    }

    fn begin_coverage_extension(
        &mut self,
        issue_summary: String,
        obligation: CoverageObligation,
    ) -> Result<(), StructuredError> {
        let already_extended = match &obligation {
            CoverageObligation::ReviewerMissing => self.review_coverage_extended,
            CoverageObligation::InstanceRegistrations(_) => self.structural_coverage_extended,
        };
        if self.coverage_extension_pending || already_extended {
            return Err(StructuredError::new(
                "TURN_PLAN_COVERAGE_EXTENSION_EXHAUSTED",
                "turn.plan.coverage_extension",
                "The single coverage extension has already been opened or consumed",
                "Replace the candidate through the remaining semantic replan frontier or halt",
            ));
        }
        self.requirements.retain(|requirement| {
            !matches!(requirement, ScopeRequirement::NoUnresolvedReferences { .. })
        });
        self.review_pending = false;
        self.coverage_extension_pending = true;
        self.coverage_issue_summary = Some(issue_summary);
        self.coverage_obligation = Some(obligation);
        Ok(())
    }

    fn append_coverage_outline(
        &mut self,
        mut extension: Vec<PlanOutlineItem>,
    ) -> Result<(), StructuredError> {
        if !self.coverage_extension_pending {
            return Err(StructuredError::new(
                "TURN_PLAN_COVERAGE_EXTENSION_NOT_PENDING",
                "tool.set_turn_plan",
                "The retained candidate is not awaiting a coverage extension",
                "Use the sole routed plan frontier",
            ));
        }
        if let Some(CoverageObligation::InstanceRegistrations(expected)) = &self.coverage_obligation
        {
            let actual = extension
                .iter()
                .filter(|item| item.op == PlanOp::RegisterInstance)
                .map(|item| item.owner.clone())
                .collect::<BTreeSet<_>>();
            if actual != *expected || extension.len() != expected.len() {
                return Err(StructuredError::new(
                    "TURN_PLAN_INSTANCE_COVERAGE_SCOPE",
                    "tool.set_turn_plan.arguments.steps",
                    format!(
                        "The structural extension must contain exactly one register_instance for each required rule: {}",
                        expected.iter().cloned().collect::<Vec<_>>().join(", ")
                    ),
                    "Remove every other operation and provide each listed rule exactly once as the register_instance owner",
                ));
            }
        }
        let combined_items = self.outline.len().saturating_add(extension.len());
        if combined_items > MAX_PLAN_ITEMS {
            return Err(StructuredError::new(
                "TURN_PLAN_OUTLINE_SIZE",
                "tool.set_turn_plan.arguments.steps",
                format!(
                    "The retained candidate plus coverage extension contains {combined_items} items; the maximum is {MAX_PLAN_ITEMS}"
                ),
                "Add only the concrete missing operations from the coverage issues",
            ));
        }
        let combined_goal_chars = self
            .outline
            .iter()
            .chain(&extension)
            .map(|item| item.goal.chars().count())
            .sum::<usize>();
        if combined_goal_chars > MAX_PLAN_GOAL_TOTAL_CHARS {
            return Err(StructuredError::new(
                "TURN_PLAN_OUTLINE_TEXT_SIZE",
                "tool.set_turn_plan.arguments.steps",
                format!(
                    "The retained candidate plus coverage extension contains {combined_goal_chars} goal characters; the maximum is {MAX_PLAN_GOAL_TOTAL_CHARS}"
                ),
                "Keep the missing-operation goals concise and put typed values in the packet",
            ));
        }
        let offset = self.outline.len();
        rebase_turn_plan_outline_ids(&mut extension, offset);
        if self.packet_refined {
            self.packet_ends
                .extend((offset + 1)..=(offset + extension.len()));
        } else {
            self.packet_ends.extend(
                plan_packet_ends(&extension)
                    .into_iter()
                    .map(|end| offset + end),
            );
        }
        self.outline.extend(extension);
        self.coverage_extension_pending = false;
        match self.coverage_obligation.take() {
            Some(CoverageObligation::ReviewerMissing) => self.review_coverage_extended = true,
            Some(CoverageObligation::InstanceRegistrations(_)) => {
                self.structural_coverage_extended = true;
            }
            None => {}
        }
        self.coverage_issue_summary = None;
        self.review_pending = false;
        Ok(())
    }

    fn has_coverage_extension(&self) -> bool {
        self.review_coverage_extended || self.structural_coverage_extended
    }

    fn coverage_extension_context(&self) -> String {
        let retained_ids = self
            .requirements
            .iter()
            .map(ScopeRequirement::id)
            .collect::<Vec<_>>()
            .join(", ");
        let obligation = match &self.coverage_obligation {
            Some(CoverageObligation::InstanceRegistrations(owners)) => format!(
                "; required register_instance owners: [{}]",
                owners.iter().cloned().collect::<Vec<_>>().join(", ")
            ),
            _ => String::new(),
        };
        format!(
            "Retained typed candidate IDs: [{retained_ids}]. Coverage report: {}{obligation}",
            self.coverage_issue_summary.as_deref().unwrap_or("missing")
        )
    }

    fn current_packet(&self) -> &[PlanOutlineItem] {
        let end = self
            .packet_ends
            .iter()
            .copied()
            .find(|end| *end > self.cursor)
            .unwrap_or(self.outline.len());
        &self.outline[self.cursor..end]
    }

    fn refine_remaining_packets(&mut self) -> bool {
        if self.packet_refined || self.current_packet().len() <= 1 {
            return false;
        }
        self.packet_ends.retain(|end| *end <= self.cursor);
        self.packet_ends
            .extend((self.cursor + 1)..=self.outline.len());
        self.packet_refined = true;
        self.packet_refinement_pending = true;
        true
    }

    fn take_packet_refinement(&mut self) -> bool {
        std::mem::take(&mut self.packet_refinement_pending)
    }

    fn completed_packet_count(&self) -> usize {
        self.packet_ends
            .iter()
            .filter(|end| **end <= self.cursor)
            .count()
    }

    fn packet_count(&self) -> usize {
        self.packet_ends.len()
    }

    fn created_alias_summary(&self) -> String {
        let mut roles = Vec::new();
        let mut channels = Vec::new();
        let mut instances = Vec::new();
        for requirement in &self.requirements {
            let ScopeRequirement::Action { action, .. } = requirement else {
                continue;
            };
            match action {
                ScopeAction::CreateRole { key, .. } => roles.push(key.as_str()),
                ScopeAction::CreateChannel { key, .. } => channels.push(key.as_str()),
                ScopeAction::RegisterInstance { key, .. } => instances.push(key.as_str()),
                _ => {}
            }
        }
        format!(
            "created role aliases=[{}], channel aliases=[{}], instance aliases=[{}]",
            roles.join(", "),
            channels.join(", "),
            instances.join(", ")
        )
    }
}

fn plan_packet_ends(outline: &[PlanOutlineItem]) -> Vec<usize> {
    let first_rule = outline
        .iter()
        .position(|item| item.op == PlanOp::Rule)
        .unwrap_or(outline.len());
    let mut ends = Vec::new();
    let mut cursor = 0;
    while cursor < first_rule {
        cursor = cursor.saturating_add(MAX_PLAN_PACKET_ITEMS).min(first_rule);
        ends.push(cursor);
    }
    let mut segment_start = first_rule;
    while segment_start < outline.len() {
        let segment_end = outline[segment_start + 1..]
            .iter()
            .position(|item| item.op == PlanOp::Rule)
            .map_or(outline.len(), |offset| segment_start + offset + 1);
        let mut packet_start = segment_start;
        while segment_end.saturating_sub(packet_start) > MAX_PLAN_PACKET_ITEMS * 2 {
            packet_start += MAX_PLAN_PACKET_ITEMS;
            ends.push(packet_start);
        }
        let remaining = segment_end.saturating_sub(packet_start);
        if remaining > MAX_PLAN_PACKET_ITEMS {
            packet_start += if remaining >= MAX_PLAN_PACKET_ITEMS + 3
                && preserves_lifecycle_tail(outline, packet_start, segment_end)
            {
                remaining - MAX_PLAN_PACKET_ITEMS
            } else {
                MAX_PLAN_PACKET_ITEMS
            };
            ends.push(packet_start);
        }
        ends.push(segment_end);
        segment_start = segment_end;
    }
    refine_complex_packet_ends(outline, &ends)
}

fn refine_complex_packet_ends(outline: &[PlanOutlineItem], ends: &[usize]) -> Vec<usize> {
    let mut refined = Vec::new();
    let mut start = 0;
    for end in ends.iter().copied() {
        let packet = &outline[start..end];
        let mut distinct = Vec::new();
        for item in packet {
            if !distinct.contains(&item.op) {
                distinct.push(item.op);
            }
        }
        if packet.len() == MAX_PLAN_PACKET_ITEMS && distinct.len() >= 3 {
            refined.extend((start + 1)..=end);
        } else {
            refined.push(end);
        }
        start = end;
    }
    refined
}

fn preserves_lifecycle_tail(
    outline: &[PlanOutlineItem],
    packet_start: usize,
    segment_end: usize,
) -> bool {
    if segment_end.saturating_sub(packet_start) < MAX_PLAN_PACKET_ITEMS {
        return false;
    }
    let tail = &outline[segment_end - MAX_PLAN_PACKET_ITEMS..segment_end];
    tail[tail.len() - 2].op == PlanOp::RegisterInstance
        && tail[tail.len() - 1].op == PlanOp::EditResponse
        && tail[..tail.len() - 2]
            .iter()
            .all(|item| item.op == PlanOp::PostPanel)
}

fn bounded_review_retry_error_field(value: &str) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut characters = normalized.chars();
    let prefix = characters
        .by_ref()
        .take(MAX_REVIEW_RETRY_ERROR_FIELD_CHARS)
        .collect::<String>();
    if characters.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

pub struct DesignSession<C> {
    client: C,
    draft: Draft,
    messages: Vec<Message>,
    tools: Vec<ToolDefinition>,
    config: SessionConfig,
    observability: Observability,
    last_error: Option<StructuredError>,
    prose_nudged: bool,
    repair_state: Option<RepairState>,
    turn_state: Option<TurnState>,
    adaptive_turn: Option<AdaptiveTurnState>,
    adaptive_enabled: bool,
    planned_enabled: bool,
    legacy_plan_enabled: bool,
    planned_execution_attempts: u8,
    brief_correction_remaining: u8,
    outline_correction_remaining: u8,
    packet_correction_remaining: u8,
    review_correction_remaining: u8,
    planned_correction_remaining: u8,
    planned_review_coverage_extension_used: bool,
    planned_structural_extension_used: bool,
    planned_root_draft: Option<Draft>,
    plan_assembly: Option<PlanAssembly>,
    current_human_message_index: Option<usize>,
    brief_history: Vec<TurnBrief>,
    intent_recipe: Option<IntentRecipeRuntime>,
}

impl<C> DesignSession<C> {
    pub fn new(client: C) -> Self {
        Self::with_config(client, SessionConfig::default())
    }

    pub fn with_config(client: C, config: SessionConfig) -> Self {
        Self::build(client, config, false, false, false)
    }

    pub fn with_adaptive_config(client: C, config: SessionConfig) -> Self {
        Self::build(client, config, true, false, false)
    }

    pub fn with_planned_config(client: C, config: SessionConfig) -> Self {
        Self::build(client, config, true, true, false)
    }

    pub fn with_planned_oracle_config(client: C, config: SessionConfig) -> Self {
        Self::build(client, config, true, true, true)
    }

    fn build(
        client: C,
        config: SessionConfig,
        adaptive_enabled: bool,
        planned_enabled: bool,
        legacy_plan_enabled: bool,
    ) -> Self {
        let draft = Draft::new();
        let messages = vec![Message::system(if planned_enabled {
            PLANNED_SYSTEM_PROMPT
        } else {
            DEFAULT_SYSTEM_PROMPT
        })];
        Self {
            client,
            draft,
            messages,
            tools: if adaptive_enabled {
                all_tool_definitions(planned_enabled)
            } else {
                legacy_tool_definitions()
            },
            config,
            observability: Observability::default(),
            last_error: None,
            prose_nudged: false,
            repair_state: None,
            turn_state: None,
            adaptive_turn: None,
            adaptive_enabled,
            planned_enabled,
            legacy_plan_enabled,
            planned_execution_attempts: 0,
            brief_correction_remaining: 1,
            outline_correction_remaining: 1,
            packet_correction_remaining: 1,
            review_correction_remaining: 1,
            planned_correction_remaining: 1,
            planned_review_coverage_extension_used: false,
            planned_structural_extension_used: false,
            planned_root_draft: None,
            plan_assembly: None,
            current_human_message_index: None,
            brief_history: Vec::new(),
            intent_recipe: None,
        }
    }

    pub fn draft(&self) -> &Draft {
        &self.draft
    }

    pub fn draft_mut(&mut self) -> &mut Draft {
        &mut self.draft
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn observability(&self) -> &Observability {
        &self.observability
    }

    pub fn turn_state(&self) -> Option<&TurnState> {
        self.turn_state.as_ref()
    }

    pub fn adaptive_turn(&self) -> Option<&AdaptiveTurnState> {
        self.adaptive_turn.as_ref()
    }

    pub fn adaptive_enabled(&self) -> bool {
        self.adaptive_enabled
    }

    pub fn planned_enabled(&self) -> bool {
        self.planned_enabled
    }

    pub fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            schema_version: SESSION_SNAPSHOT_VERSION,
            draft: self.draft.clone(),
            messages: self.messages.clone(),
            observability: self.observability.clone(),
            last_error: self.last_error.clone(),
            prose_nudged: self.prose_nudged,
            repair_state: self.repair_state.clone(),
            turn_state: self.turn_state.clone().map(|mut state| {
                state.current_revision = self.draft.draft_revision;
                state
            }),
            adaptive_turn: self.adaptive_turn.clone(),
            adaptive_enabled: self.adaptive_enabled,
            brief_history: self.brief_history.clone(),
            intent_recipe: self
                .intent_recipe
                .as_ref()
                .map(IntentRecipeRuntime::snapshot),
        }
    }

    pub fn restore(
        client: C,
        config: SessionConfig,
        snapshot: SessionSnapshot,
    ) -> Result<Self, SessionSnapshotError> {
        Self::restore_with_mode(client, config, snapshot, false)
    }

    pub fn restore_planned(
        client: C,
        config: SessionConfig,
        snapshot: SessionSnapshot,
    ) -> Result<Self, SessionSnapshotError> {
        Self::restore_with_mode(client, config, snapshot, true)
    }

    fn restore_with_mode(
        client: C,
        config: SessionConfig,
        mut snapshot: SessionSnapshot,
        planned_enabled: bool,
    ) -> Result<Self, SessionSnapshotError> {
        validate_snapshot(&snapshot)?;
        if snapshot.intent_recipe.is_some() {
            return Err(SessionSnapshotError::InvalidInvariant {
                message: "intent recipe snapshots require restore_intent_recipe with the original resource bindings".to_string(),
            });
        }
        let adaptive_enabled = snapshot.adaptive_enabled || planned_enabled;
        let prompt = if planned_enabled {
            PLANNED_SYSTEM_PROMPT
        } else {
            DEFAULT_SYSTEM_PROMPT
        };
        if let Some(message) = snapshot
            .messages
            .iter_mut()
            .find(|message| message.role == MessageRole::System)
        {
            message.content = prompt.to_string();
        } else {
            snapshot.messages.insert(0, Message::system(prompt));
        }
        Ok(Self {
            client,
            draft: snapshot.draft,
            messages: snapshot.messages,
            tools: if adaptive_enabled {
                all_tool_definitions(planned_enabled)
            } else {
                legacy_tool_definitions()
            },
            config,
            observability: snapshot.observability,
            last_error: snapshot.last_error,
            prose_nudged: snapshot.prose_nudged,
            repair_state: snapshot.repair_state,
            turn_state: snapshot.turn_state,
            adaptive_turn: snapshot.adaptive_turn,
            adaptive_enabled,
            planned_enabled,
            legacy_plan_enabled: false,
            planned_execution_attempts: 0,
            brief_correction_remaining: 1,
            outline_correction_remaining: 1,
            packet_correction_remaining: 1,
            review_correction_remaining: 1,
            planned_correction_remaining: 1,
            planned_review_coverage_extension_used: false,
            planned_structural_extension_used: false,
            planned_root_draft: None,
            plan_assembly: None,
            current_human_message_index: None,
            brief_history: snapshot.brief_history,
            intent_recipe: None,
        })
    }

    fn add_nudge(&mut self) {
        self.messages.push(Message::user(NUDGE));
        self.observability.nudge_count += 1;
    }

    fn add_planned_nudge(&mut self, frontier: &str) {
        let instruction = match frontier {
            "set_turn_brief" => "Retry exactly one set_turn_brief using only strategy, objective, requested_outcome, assumptions, and validate"
                .to_string(),
            "fill_turn_plan_packet" => self.plan_assembly.as_ref().map_or_else(
                || "Call exactly one set_turn_plan to replace the discarded candidate plan"
                    .to_string(),
                |assembly| {
                    let packet = assembly
                        .current_packet()
                        .iter()
                        .map(|item| format!("{}:{}", item.id, item.op.name()))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(
                        "Retry exactly one fill_turn_plan_packet for the same pending packet and fill only these required items: {packet}. Accepted packets remain intact and the canonical Draft is unchanged"
                    )
                },
            ),
            "review_turn_plan" => "Call exactly one review_turn_plan with the required covered_ids, reference_verdict, issue_kind, and detail fields. covered_ids must contain every exact typed candidate id once. Judge the baseline plus delta result and never require preserved baseline operations to be repeated in the delta. Compare every advertised reference-audit value against the human request and set reference_verdict to match only when all are correct. Set both reference_verdict and issue_kind to mismatch when a reference is wrong. Use issue_kind=none when complete and issue_kind=missing for absent new operations; both omit issue_id, issue_path, and expected_json. A mismatch must add exact issue_id, JSON Pointer issue_path, and JSON-encoded expected_json. An extra mutation must use issue_kind=extra and add only its exact issue_id"
                .to_string(),
            "set_turn_plan"
                if self
                    .plan_assembly
                    .as_ref()
                    .is_some_and(|assembly| assembly.coverage_extension_pending) =>
            {
                let context = self
                    .plan_assembly
                    .as_ref()
                    .map(PlanAssembly::coverage_extension_context)
                    .unwrap_or_else(|| "Coverage report: missing".to_string());
                format!(
                    "The accepted typed candidate is retained. {context}. Call exactly one set_turn_plan containing only the concrete missing operations listed in the coverage report. Do not repeat accepted operations. Use the same logical parent owners. This is the sole semantic coverage extension"
                )
            }
            _ => "Call exactly one set_turn_plan using exactly {\"steps\":[{\"op\":\"post_panel\",\"owner\":\"submit_room\",\"goal\":\"post panel key join_panel in the created channel with its complete embedded button list\"}]}. Include only missing mutations and omit existing items mentioned only to preserve them. Every step must include owner: draft for panel, modal, and rule; the panel key for a top-level button; the parent rule key for every action, never a resource key. A post_panel contains all embedded buttons in its packet, so remove separate button steps for those buttons. Every op must be an exact string from the schema enum; there is no generic action op. Never send type or action fields, a nested op object, or typed action arguments because packet tools receive typed values later"
                .to_string(),
        };
        let prefix = if frontier == "review_turn_plan" {
            PLAN_REVIEW_RETRY_PREFIX
        } else {
            PLAN_FRONTIER_RETRY_PREFIX
        };
        let error_capsule = if frontier == "review_turn_plan" {
            self.last_error.as_ref().map_or_else(String::new, |error| {
                let capsule = serde_json::json!({
                    "code": bounded_review_retry_error_field(&error.code),
                    "location": bounded_review_retry_error_field(&error.location),
                    "message": bounded_review_retry_error_field(&error.message),
                    "hint": bounded_review_retry_error_field(&error.hint),
                });
                format!(" Last review error capsule: {capsule}.")
            })
        } else {
            String::new()
        };
        self.messages.push(Message::user(format!(
            "{prefix}{instruction}.{error_capsule} This is the only correction for the current frontier; do not call any stale or unavailable tool."
        )));
        self.observability.nudge_count += 1;
    }

    fn record_failure(&mut self, name: Option<&str>, result: &ToolResult) {
        let Some(failure) = result.failure() else {
            return;
        };
        let signature = format!("{}@{}", failure.code, failure.location);
        let count = self
            .observability
            .failure_signatures
            .entry(signature)
            .or_default();
        if *count > 0 {
            self.observability.repeated_errors += 1;
        }
        *count += 1;
        self.last_error = Some(StructuredError::new(
            failure.code.clone(),
            failure.location.clone(),
            failure.message.clone(),
            failure.hint.clone(),
        ));
        match name {
            Some("validate_draft") if !is_argument_failure(&failure.code) => {
                self.observability.validation_failures += 1;
                if let Some(state) = self.turn_state.as_mut() {
                    state.gate_failures += 1;
                }
            }
            Some("simulate_draft") if !is_argument_failure(&failure.code) => {
                self.observability.simulation_failures += 1;
                if let Some(state) = self.turn_state.as_mut() {
                    state.gate_failures += 1;
                }
            }
            _ => {}
        }
    }

    fn begin_turn(&mut self, human_message: &str) {
        if self.adaptive_enabled {
            if let Some(brief) = self
                .adaptive_turn
                .as_ref()
                .and_then(|state| state.brief.clone())
            {
                self.brief_history.push(brief);
            }
        }
        let sequence = self
            .turn_state
            .as_ref()
            .map_or(1, |state| state.sequence.saturating_add(1));
        self.turn_state = Some(TurnState {
            sequence,
            phase: TurnPhase::Active,
            human_message: compact_text(human_message),
            started_revision: self.draft.draft_revision,
            current_revision: self.draft.draft_revision,
            model_calls: 0,
            tool_calls: 0,
            gate_failures: 0,
        });
        self.adaptive_turn = self.adaptive_enabled.then(AdaptiveTurnState::default);
        self.planned_execution_attempts = 0;
        self.brief_correction_remaining = 1;
        self.outline_correction_remaining = 1;
        self.packet_correction_remaining = 1;
        self.review_correction_remaining = 1;
        self.planned_correction_remaining = 1;
        self.planned_review_coverage_extension_used = false;
        self.planned_structural_extension_used = false;
        self.planned_root_draft = None;
        self.plan_assembly = None;
        self.current_human_message_index = None;
    }

    fn turn_model_calls(&self) -> usize {
        self.turn_state
            .as_ref()
            .map_or(0, |state| state.model_calls)
    }

    fn turn_tool_calls(&self) -> usize {
        self.turn_state.as_ref().map_or(0, |state| state.tool_calls)
    }

    fn turn_gate_failures(&self) -> usize {
        self.turn_state
            .as_ref()
            .map_or(0, |state| state.gate_failures)
    }

    fn record_model_call(&mut self) {
        self.observability.model_calls += 1;
        if let Some(state) = self.turn_state.as_mut() {
            state.model_calls += 1;
        }
    }

    fn record_tool_call(&mut self) {
        self.observability.tool_calls += 1;
        if let Some(state) = self.turn_state.as_mut() {
            state.tool_calls += 1;
        }
    }

    fn finish_turn(&mut self, phase: TurnPhase) {
        self.plan_assembly = None;
        if let Some(state) = self.turn_state.as_mut() {
            state.phase = phase;
            state.current_revision = self.draft.draft_revision;
        }
    }

    fn needs_input(&mut self, question: String) -> BurstOutcome {
        self.finish_turn(TurnPhase::NeedsInput);
        BurstOutcome::NeedsInput { question }
    }

    fn progressed(&mut self, summary: String) -> BurstOutcome {
        self.finish_turn(TurnPhase::Progressed);
        BurstOutcome::Progressed { summary }
    }

    fn ready(&mut self, summary: String) -> BurstOutcome {
        self.finish_turn(TurnPhase::Ready);
        BurstOutcome::Ready { summary }
    }

    fn routed(&mut self, fallback: IntentFallbackV1) -> BurstOutcome {
        self.finish_turn(TurnPhase::Routed);
        BurstOutcome::Routed { fallback }
    }

    fn halt(
        &mut self,
        code: &str,
        message: &str,
        exhausted_limit: Option<LimitKind>,
    ) -> BurstOutcome {
        self.finish_turn(TurnPhase::Halted);
        BurstOutcome::Halted(Box::new(HaltReport {
            code: code.to_string(),
            message: message.to_string(),
            exhausted_limit,
            draft: self.draft.summary(),
            last_error: self.last_error.clone(),
            observability: self.observability.clone(),
        }))
    }

    fn not_executed_result(&self) -> ToolResult {
        ToolResult::failure_from(
            &self.draft,
            StructuredError::new(
                "NOT_EXECUTED_AFTER_PREVIOUS_FAILURE",
                "tool.batch",
                "This tool call was not executed because an earlier call failed",
                "Correct the previous failure before retrying this change",
            ),
        )
    }

    fn unavailable_tool_result(&self, name: &str, tools: &[ToolDefinition]) -> ToolResult {
        let available = tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        ToolResult::failure_from(
            &self.draft,
            StructuredError::new(
                "TOOL_NOT_AVAILABLE_FOR_DRAFT_STATE",
                format!("tool.{name}"),
                "The requested design tool was not exposed for this model call or is no longer available for the current Draft state",
                format!("Use one of the currently available tools: {available}"),
            ),
        )
    }

    fn planned_frontier_name(&self) -> Option<&'static str> {
        if !self.planned_enabled {
            return None;
        }
        let state = self.adaptive_turn.as_ref()?;
        if state.phase != AdaptivePhase::Build {
            return None;
        }
        let brief = state.brief.as_ref()?;
        if !brief.requirements.is_empty() {
            return None;
        }
        match brief.intent {
            TurnIntent::Build => Some(
                self.plan_assembly
                    .as_ref()
                    .map_or("set_turn_plan", PlanAssembly::frontier_name),
            ),
            TurnIntent::Modify if self.plan_assembly.is_some() => {
                self.plan_assembly.as_ref().map(PlanAssembly::frontier_name)
            }
            TurnIntent::Modify if self.planned_execution_attempts > 0 => Some("set_turn_plan"),
            _ => None,
        }
    }

    fn awaiting_planned_modify_choice(&self) -> bool {
        self.planned_enabled
            && self.planned_execution_attempts == 0
            && self.adaptive_turn.as_ref().is_some_and(|state| {
                state.phase == AdaptivePhase::Build
                    && state.brief.as_ref().is_some_and(|brief| {
                        brief.intent == TurnIntent::Modify && brief.requirements.is_empty()
                    })
            })
            && self
                .turn_state
                .as_ref()
                .is_some_and(|turn| turn.started_revision == self.draft.draft_revision)
    }

    fn reject_mixed_planned_modify_batch(&mut self, calls: &[ToolCall]) -> Option<BurstOutcome> {
        self.messages
            .push(Message::assistant_tool_calls(calls.to_vec()));
        self.planned_execution_attempts = self.planned_execution_attempts.saturating_add(1);
        self.observability.plan_submissions = self.observability.plan_submissions.saturating_add(1);
        let result = ToolResult::failure_from(
            &self.draft,
            StructuredError::new(
                "TURN_PLAN_MIXED_EXECUTION_PATHS",
                "turn.plan.response",
                "A modify-labeled response combined set_turn_plan with another tool call",
                "Submit exactly one set_turn_plan call for additive work, or use only update and remove tools for a true modification",
            ),
        );
        self.record_failure(Some("set_turn_plan"), &result);
        let mut reported = false;
        for call in calls {
            let content = if call.name == "set_turn_plan" && !reported {
                reported = true;
                result.as_json()
            } else {
                self.not_executed_result().as_json()
            };
            self.messages.push(Message::tool(call.id.clone(), content));
        }
        self.consume_planned_response_failure("set_turn_plan", None)
    }

    fn consume_planned_response_failure(
        &mut self,
        frontier: &str,
        error: Option<StructuredError>,
    ) -> Option<BurstOutcome> {
        if let Some(error) = error {
            let result = ToolResult::failure_from(&self.draft, error);
            self.record_failure(None, &result);
        }
        if frontier == "fill_turn_plan_packet"
            && self
                .plan_assembly
                .as_mut()
                .is_some_and(PlanAssembly::take_packet_refinement)
        {
            self.add_planned_nudge(frontier);
            return None;
        }
        let remaining = match frontier {
            "fill_turn_plan_packet" => &mut self.packet_correction_remaining,
            "review_turn_plan" => &mut self.review_correction_remaining,
            _ => &mut self.outline_correction_remaining,
        };
        if *remaining == 0 {
            return Some(self.halt(
                "PLAN_REPAIR_FAILED",
                "The single automatic correction for this turn-plan frontier failed",
                None,
            ));
        }
        *remaining -= 1;
        self.add_planned_nudge(frontier);
        None
    }

    fn consume_planned_replan_failure(&mut self) -> Option<BurstOutcome> {
        if self.planned_correction_remaining == 0 {
            return Some(self.halt(
                "PLAN_REPAIR_FAILED",
                "The single semantic turn-plan replan failed",
                None,
            ));
        }
        self.planned_correction_remaining -= 1;
        self.plan_assembly = None;
        if let Some(brief) = self
            .adaptive_turn
            .as_mut()
            .and_then(|state| state.brief.as_mut())
        {
            brief.requirements.clear();
        }
        self.reset_planned_frontier_corrections();
        self.add_planned_nudge("set_turn_plan");
        None
    }

    fn consume_planned_coverage_extension_failure(&mut self) -> Option<BurstOutcome> {
        if self.planned_correction_remaining == 0 {
            return Some(self.halt(
                "PLAN_REPAIR_FAILED",
                "The single semantic coverage extension failed",
                None,
            ));
        }
        self.planned_correction_remaining -= 1;
        self.reset_planned_frontier_corrections();
        self.add_planned_nudge("set_turn_plan");
        None
    }

    pub(super) fn reset_planned_frontier_corrections(&mut self) {
        self.outline_correction_remaining = 1;
        self.packet_correction_remaining = 1;
        self.review_correction_remaining = 1;
    }

    fn accept_planned_requirements(&mut self, requirements: Vec<ScopeRequirement>) {
        self.observability.plan_acceptances = self.observability.plan_acceptances.saturating_add(1);
        self.observability.planned_requirements = self
            .observability
            .planned_requirements
            .saturating_add(requirements.len());
        if let Some(brief) = self
            .adaptive_turn
            .as_mut()
            .and_then(|state| state.brief.as_mut())
        {
            brief.requirements = requirements;
        }
    }

    fn append_plan_review_directive(&mut self) {
        self.messages.push(Message::user(format!(
            "{COVERAGE_REVIEW_PREFIX}Independently audit the complete original human request against the resulting baseline-plus-delta design in the sole review_turn_plan tool. Existing operations mentioned only for preservation or non-duplication belong in the baseline and must not be repeated in the delta. Verify exact new literals, references, permissions, repeated counts, action order, and the typed harness-derived instance manifest. Return every exact typed candidate id once in covered_ids. Always submit the required covered_ids, reference_verdict, issue_kind, and detail fields. Compare every advertised reference-audit value against the human request and set reference_verdict to match only when all are correct. Set both reference_verdict and issue_kind to mismatch when a reference is wrong. Use issue_kind=none when complete and issue_kind=missing for absent new operations; both omit issue_id, issue_path, and expected_json. A mismatch must add exact issue_id, JSON Pointer issue_path, and JSON-encoded expected_json. An extra mutation must use issue_kind=extra and add only its exact issue_id. The harness derives the verdict. Do not return prose."
        )));
    }

    fn append_plan_packet_directive(&mut self) {
        if self
            .plan_assembly
            .as_ref()
            .is_some_and(|assembly| assembly.coverage_extension_pending)
        {
            self.add_planned_nudge("set_turn_plan");
            return;
        }
        let Some(assembly) = self.plan_assembly.as_ref() else {
            return;
        };
        if assembly.review_pending
            || assembly.cursor == 0
            || assembly.cursor >= assembly.outline.len()
        {
            return;
        }
        let completed = assembly.completed_packet_count();
        let total = assembly.packet_count();
        let next = assembly
            .current_packet()
            .iter()
            .map(|item| format!("{}:{}", item.id, item.op.name()))
            .collect::<Vec<_>>()
            .join(", ");
        let aliases = assembly.created_alias_summary();
        self.messages.push(Message::user(format!(
            "{PACKET_CONTINUE_PREFIX}Packet {completed} of {total} is accepted and the canonical Draft is still unchanged. Call exactly one fill_turn_plan_packet for packet {} of {total} and fill every newly required property: {next}. Available references use stable keys, never rendered names: {aliases}. Do not finish, review, replan, or repeat an accepted item.",
            completed + 1
        )));
    }

    fn dispatch_control_tool(
        &mut self,
        name: &str,
        arguments: &str,
    ) -> (ToolResult, Option<BurstOutcome>) {
        match name {
            "set_turn_brief" => {
                let parsed = if self.planned_enabled {
                    parse_planned_turn_brief(arguments)
                } else {
                    parse_turn_brief(arguments)
                };
                let mut brief = match parsed {
                    Ok(brief) => brief,
                    Err(error) => return (ToolResult::failure_from(&self.draft, error), None),
                };
                brief.verification.simulation =
                    simulation_profile_for_current_human_turn(&self.messages);
                if brief.objective.trim().is_empty() {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "EMPTY_TURN_OBJECTIVE",
                                "tool.set_turn_brief.objective",
                                "The turn objective is empty",
                                "Provide a concise objective grounded in the human request",
                            ),
                        ),
                        None,
                    );
                }
                if brief.requested_outcome == RequestedOutcome::ValidatedPreview
                    && !brief.verification.validate
                {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "PREVIEW_REQUIRES_VALIDATION",
                                "tool.set_turn_brief.validate",
                                "A validated preview requires validation",
                                "Set validate to true",
                            ),
                        ),
                        None,
                    );
                }
                if brief.verification.simulation == SimulationProfile::StudyRoom
                    && !brief.verification.validate
                {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "SIMULATION_REQUIRES_VALIDATION",
                                "tool.set_turn_brief.validate",
                                "StudyRoom simulation requires validation",
                                "Set validate to true",
                            ),
                        ),
                        None,
                    );
                }
                let verification_only = brief.intent == TurnIntent::Inspect
                    && brief.requested_outcome == RequestedOutcome::ValidatedPreview
                    && brief.verification.validate;
                let phase = if verification_only {
                    AdaptivePhase::Verify
                } else if brief.requested_outcome == RequestedOutcome::Discussion
                    || matches!(brief.intent, TurnIntent::Brainstorm | TurnIntent::Inspect)
                {
                    AdaptivePhase::Reply
                } else {
                    AdaptivePhase::Build
                };
                self.adaptive_turn = Some(AdaptiveTurnState {
                    phase,
                    brief: Some(brief),
                    scoped_revision: verification_only.then_some(self.draft.draft_revision),
                    previewed_revision: None,
                });
                (
                    ToolResult::success(&self.draft, "Recorded the current turn brief"),
                    None,
                )
            }
            "set_turn_plan" => {
                if !self.planned_enabled {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "TURN_PLAN_MODE_REQUIRED",
                                "tool.set_turn_plan",
                                "Typed turn plans are not enabled for this session",
                                "Use a planned design session before setting a turn plan",
                            ),
                        ),
                        None,
                    );
                }
                self.planned_execution_attempts = self.planned_execution_attempts.saturating_add(1);
                let submission = match parse_turn_plan(arguments) {
                    Ok(submission) => submission,
                    Err(error) => return (ToolResult::failure_from(&self.draft, error), None),
                };
                let Some(brief) = self
                    .adaptive_turn
                    .as_ref()
                    .and_then(|state| state.brief.as_ref())
                    .cloned()
                else {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "TURN_BRIEF_REQUIRED",
                                "tool.set_turn_plan",
                                "No active turn brief exists",
                                "Call set_turn_brief before setting the turn plan",
                            ),
                        ),
                        None,
                    );
                };
                match submission {
                    TurnPlanSubmission::Outline(mut outline) => {
                        let extending = self
                            .plan_assembly
                            .as_ref()
                            .is_some_and(|assembly| assembly.coverage_extension_pending);
                        if extending {
                            let (root_revision, retained_requirements) = self
                                .plan_assembly
                                .as_ref()
                                .map(|assembly| {
                                    (assembly.root_revision, assembly.requirements.clone())
                                })
                                .unwrap_or_default();
                            if root_revision != self.draft.draft_revision {
                                return (
                                    ToolResult::failure_from(
                                        &self.draft,
                                        StructuredError::new(
                                            "TURN_PLAN_ROOT_REVISION_CHANGED",
                                            "turn.plan.root_revision",
                                            "The canonical Draft changed while the candidate awaited a coverage extension",
                                            "Discard the candidate and create a new plan for the current Draft",
                                        ),
                                    ),
                                    None,
                                );
                            }
                            if let Err(error) = resolve_turn_plan_extension_outline_parent_owners(
                                &self.draft,
                                &retained_requirements,
                                &mut outline,
                            ) {
                                return (ToolResult::failure_from(&self.draft, error), None);
                            }
                            let result = self.plan_assembly.as_mut().map_or_else(
                                || {
                                    Err(StructuredError::new(
                                        "TURN_PLAN_OUTLINE_REQUIRED",
                                        "tool.set_turn_plan",
                                        "The retained candidate is unavailable",
                                        "Create a new plan for the current Draft",
                                    ))
                                },
                                |assembly| assembly.append_coverage_outline(outline),
                            );
                            return match result {
                                Ok(()) => (
                                    ToolResult::success(
                                        &self.draft,
                                        "Accepted the missing-coverage outline; the retained candidate remains uncommitted",
                                    ),
                                    None,
                                ),
                                Err(error) => {
                                    (ToolResult::failure_from(&self.draft, error), None)
                                }
                            };
                        }
                        if let Err(error) =
                            resolve_turn_plan_outline_parent_owners(&self.draft, &mut outline)
                        {
                            return (ToolResult::failure_from(&self.draft, error), None);
                        }
                        self.plan_assembly =
                            Some(PlanAssembly::new(self.draft.draft_revision, outline));
                        (
                            ToolResult::success(&self.draft, "Accepted the ordered plan outline"),
                            None,
                        )
                    }
                    TurnPlanSubmission::Complete(requirements) => {
                        if self
                            .plan_assembly
                            .as_ref()
                            .is_some_and(|assembly| assembly.coverage_extension_pending)
                        {
                            return (
                                ToolResult::failure_from(
                                    &self.draft,
                                    StructuredError::new(
                                        "TURN_PLAN_COVERAGE_EXTENSION_OUTLINE_REQUIRED",
                                        "tool.set_turn_plan.arguments.requirements",
                                        "A retained typed candidate can only be extended with an ordered outline",
                                        "Submit only the concrete missing operations in steps",
                                    ),
                                ),
                                None,
                            );
                        }
                        if !self.legacy_plan_enabled {
                            return (
                                ToolResult::failure_from(
                                    &self.draft,
                                    StructuredError::new(
                                        "TURN_PLAN_LEGACY_FORBIDDEN",
                                        "tool.set_turn_plan.arguments.requirements",
                                        "Legacy complete requirements are isolated from the production model path",
                                        "Submit the advertised ordered outline in steps",
                                    ),
                                ),
                                None,
                            );
                        }
                        let requirements =
                            match normalize_turn_plan(&self.draft, &brief, requirements) {
                                Ok(requirements) => requirements,
                                Err(error) => {
                                    if frontier::is_plan_conflict_code(&error.code) {
                                        self.observability.plan_conflicts =
                                            self.observability.plan_conflicts.saturating_add(1);
                                    }
                                    return (ToolResult::failure_from(&self.draft, error), None);
                                }
                            };
                        if let Err(error) =
                            validate_turn_plan_new_rule_action_coverage(&self.draft, &requirements)
                        {
                            return (ToolResult::failure_from(&self.draft, error), None);
                        }
                        self.accept_planned_requirements(requirements);
                        (
                            ToolResult::success(
                                &self.draft,
                                "Accepted the deterministic turn plan",
                            ),
                            None,
                        )
                    }
                }
            }
            "review_turn_plan" => {
                if !self.planned_enabled {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "TURN_PLAN_MODE_REQUIRED",
                                "tool.review_turn_plan",
                                "Typed turn plans are not enabled for this session",
                                "Use a planned design session before reviewing a plan",
                            ),
                        ),
                        None,
                    );
                }
                let Some(assembly) = self.plan_assembly.as_ref() else {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "TURN_PLAN_OUTLINE_REQUIRED",
                                "tool.review_turn_plan",
                                "No active plan outline exists",
                                "Call set_turn_plan before reviewing its coverage",
                            ),
                        ),
                        None,
                    );
                };
                if !assembly.review_pending {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "TURN_PLAN_REVIEW_NOT_PENDING",
                                "tool.review_turn_plan",
                                "The active outline does not require another coverage review",
                                "Use the sole packet tool exposed by the harness",
                            ),
                        ),
                        None,
                    );
                }
                if assembly.root_revision != self.draft.draft_revision {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "TURN_PLAN_ROOT_REVISION_CHANGED",
                                "turn.plan.root_revision",
                                "The canonical Draft changed while the outline awaited review",
                                "Discard the outline and create a new plan for the current Draft",
                            ),
                        ),
                        None,
                    );
                }
                let requirements = assembly.requirements.clone();
                let review = if self.legacy_plan_enabled {
                    parse_turn_plan_review_oracle(&requirements, arguments)
                } else {
                    parse_turn_plan_review(&requirements, arguments)
                };
                if let Err(error) = review {
                    if error.code == "TURN_PLAN_REVIEW_COVERAGE_INCOMPLETE" {
                        if self.planned_review_coverage_extension_used {
                            self.plan_assembly = None;
                            return (
                                ToolResult::failure_from(
                                    &self.draft,
                                    StructuredError::new(
                                        "TURN_PLAN_COVERAGE_EXTENSION_EXHAUSTED",
                                        "turn.plan.coverage_extension",
                                        "The independent review found another missing operation after the single coverage extension was consumed",
                                        "Replace the complete candidate through the remaining semantic replan frontier or halt",
                                    ),
                                ),
                                None,
                            );
                        }
                        let opened = self.plan_assembly.as_mut().map_or_else(
                            || {
                                Err(StructuredError::new(
                                    "TURN_PLAN_OUTLINE_REQUIRED",
                                    "tool.review_turn_plan",
                                    "The reviewed candidate is unavailable",
                                    "Create a new plan for the current Draft",
                                ))
                            },
                            |assembly| {
                                assembly.begin_coverage_extension(
                                    error.message.clone(),
                                    CoverageObligation::ReviewerMissing,
                                )
                            },
                        );
                        if let Err(open_error) = opened {
                            self.plan_assembly = None;
                            return (ToolResult::failure_from(&self.draft, open_error), None);
                        }
                        self.planned_review_coverage_extension_used = true;
                    } else if error.code == "TURN_PLAN_REVIEW_CANDIDATE_MISMATCH" {
                        self.plan_assembly = None;
                    }
                    return (ToolResult::failure_from(&self.draft, error), None);
                }
                if let Err(error) =
                    validate_turn_plan_new_rule_action_coverage(&self.draft, &requirements)
                {
                    self.plan_assembly = None;
                    return (ToolResult::failure_from(&self.draft, error), None);
                }
                let requirements = self
                    .plan_assembly
                    .take()
                    .map(|assembly| assembly.requirements)
                    .unwrap_or_default();
                self.accept_planned_requirements(requirements);
                (
                    ToolResult::success(
                        &self.draft,
                        "Accepted the independent typed candidate review",
                    ),
                    None,
                )
            }
            "fill_turn_plan_packet" => {
                if !self.planned_enabled {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "TURN_PLAN_MODE_REQUIRED",
                                "tool.fill_turn_plan_packet",
                                "Typed turn plans are not enabled for this session",
                                "Use a planned design session before filling a plan packet",
                            ),
                        ),
                        None,
                    );
                }
                let Some(assembly) = self.plan_assembly.as_ref() else {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "TURN_PLAN_OUTLINE_REQUIRED",
                                "tool.fill_turn_plan_packet",
                                "No active plan outline exists",
                                "Call set_turn_plan before filling a plan packet",
                            ),
                        ),
                        None,
                    );
                };
                if assembly.root_revision != self.draft.draft_revision {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "TURN_PLAN_ROOT_REVISION_CHANGED",
                                "turn.plan.root_revision",
                                "The canonical Draft changed while the plan outline was being filled",
                                "Discard the outline and create a new plan for the current Draft",
                            ),
                        ),
                        None,
                    );
                }
                let packet = assembly.current_packet().to_vec();
                let accepted_requirements = assembly.requirements.clone();
                let requirements = match parse_turn_plan_packet_scoped(
                    &self.draft,
                    &accepted_requirements,
                    &packet,
                    arguments,
                ) {
                    Ok(requirements) => requirements,
                    Err(failure) => {
                        let prior_template_dependency = failure.is_prior_template_dependency();
                        let mut error = failure.into_error();
                        let repeated_prior_template_dependency = prior_template_dependency
                            && self.packet_correction_remaining == 0
                            && self.last_error.as_ref().is_some_and(|previous| {
                                previous.code == error.code && previous.location == error.location
                            });
                        if error.code == "TURN_PLAN_REFERENCE_MISSING"
                            || repeated_prior_template_dependency
                        {
                            self.plan_assembly = None;
                            if repeated_prior_template_dependency {
                                error.hint = "The isolated packet correction repeated the same error against a dependency accepted in an earlier packet; replace the whole turn plan and correct the modal fields or rule trigger before this template action".to_string();
                            }
                        } else if let Some(assembly) = self.plan_assembly.as_mut() {
                            assembly.refine_remaining_packets();
                        }
                        return (ToolResult::failure_from(&self.draft, error), None);
                    }
                };
                let (candidate_cursor, mut candidate_requirements, outline_len) =
                    self.plan_assembly.as_ref().map_or_else(
                        || (0, Vec::new(), 0),
                        |assembly| {
                            (
                                assembly.cursor.saturating_add(packet.len()),
                                assembly.requirements.clone(),
                                assembly.outline.len(),
                            )
                        },
                    );
                candidate_requirements.extend(requirements);
                let finished = candidate_cursor == outline_len;
                if !finished {
                    if let Some(assembly) = self.plan_assembly.as_mut() {
                        assembly.requirements = candidate_requirements;
                        assembly.cursor = candidate_cursor;
                    }
                    let (completed, total) = self
                        .plan_assembly
                        .as_ref()
                        .map(|assembly| {
                            (assembly.completed_packet_count(), assembly.packet_count())
                        })
                        .unwrap_or_default();
                    return (
                        ToolResult::success(
                            &self.draft,
                            format!(
                                "Accepted plan packet {completed} of {total}; the plan remains incomplete"
                            ),
                        ),
                        None,
                    );
                }
                if let Err(error) = resolve_turn_plan_owners(&mut candidate_requirements) {
                    self.plan_assembly = None;
                    return (ToolResult::failure_from(&self.draft, error), None);
                }
                resolve_turn_plan_created_reference_kinds(&self.draft, &mut candidate_requirements);
                resolve_turn_plan_response_lifecycle_actions(
                    &self.draft,
                    &mut candidate_requirements,
                );
                if self
                    .plan_assembly
                    .as_ref()
                    .is_some_and(PlanAssembly::has_coverage_extension)
                {
                    merge_turn_plan_extension_action_lanes(&mut candidate_requirements);
                }
                if let Err(error) = resolve_turn_plan_unique_instance_aliases(
                    &self.draft,
                    &mut candidate_requirements,
                ) {
                    if error.code == "TURN_PLAN_INSTANCE_REGISTRATION_REQUIRED" {
                        if self.planned_structural_extension_used {
                            self.plan_assembly = None;
                            return (
                                ToolResult::failure_from(
                                    &self.draft,
                                    StructuredError::new(
                                        "TURN_PLAN_COVERAGE_EXTENSION_EXHAUSTED",
                                        "turn.plan.coverage_extension",
                                        "The candidate still requires instance registration after the single coverage extension was consumed",
                                        "Replace the complete candidate through the remaining semantic replan frontier or halt",
                                    ),
                                ),
                                None,
                            );
                        }
                        let owners = missing_turn_plan_instance_registration_owners(
                            &self.draft,
                            &candidate_requirements,
                        );
                        if owners.is_empty() {
                            self.plan_assembly = None;
                            return (ToolResult::failure_from(&self.draft, error), None);
                        }
                        if let Some(assembly) = self.plan_assembly.as_mut() {
                            assembly.requirements = candidate_requirements;
                            assembly.cursor = candidate_cursor;
                            if let Err(open_error) = assembly.begin_coverage_extension(
                                error.message.clone(),
                                CoverageObligation::InstanceRegistrations(owners),
                            ) {
                                self.plan_assembly = None;
                                return (ToolResult::failure_from(&self.draft, open_error), None);
                            }
                        }
                        self.planned_structural_extension_used = true;
                        return (
                            ToolResult::success(
                                &self.draft,
                                format!(
                                    "Accepted the typed candidate with one structural coverage obligation: {}",
                                    error.message
                                ),
                            ),
                            None,
                        );
                    }
                    self.plan_assembly = None;
                    return (ToolResult::failure_from(&self.draft, error), None);
                }
                derive_turn_plan_instance_manifests(&self.draft, &mut candidate_requirements);
                let Some(brief) = self
                    .adaptive_turn
                    .as_ref()
                    .and_then(|state| state.brief.as_ref())
                    .cloned()
                else {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "TURN_BRIEF_REQUIRED",
                                "tool.fill_turn_plan_packet",
                                "No active turn brief exists",
                                "Call set_turn_brief before filling a plan packet",
                            ),
                        ),
                        None,
                    );
                };
                assign_turn_plan_repeat_targets(&self.draft, &mut candidate_requirements);
                let requirements =
                    match normalize_turn_plan(&self.draft, &brief, candidate_requirements) {
                        Ok(requirements) => requirements,
                        Err(error) => {
                            if frontier::is_plan_conflict_code(&error.code) {
                                self.observability.plan_conflicts =
                                    self.observability.plan_conflicts.saturating_add(1);
                            }
                            self.plan_assembly = None;
                            return (ToolResult::failure_from(&self.draft, error), None);
                        }
                    };
                if let Some(assembly) = self.plan_assembly.as_mut() {
                    assembly.requirements = requirements;
                    assembly.cursor = candidate_cursor;
                    assembly.review_pending = true;
                }
                (
                    ToolResult::success(
                        &self.draft,
                        "Accepted the final typed packet; the candidate now awaits independent coverage review",
                    ),
                    None,
                )
            }
            "check_turn_scope" => {
                if let Err(error) = parse_empty_control(name, arguments) {
                    return (ToolResult::failure_from(&self.draft, error), None);
                }
                let Some(brief) = self
                    .adaptive_turn
                    .as_ref()
                    .and_then(|state| state.brief.as_ref())
                    .cloned()
                else {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "TURN_BRIEF_REQUIRED",
                                "tool.check_turn_scope",
                                "No active turn brief exists",
                                "Call set_turn_brief before checking scope",
                            ),
                        ),
                        None,
                    );
                };
                let scope = check_scope(&self.draft, &brief);
                let requires_change =
                    matches!(brief.intent, TurnIntent::Build | TurnIntent::Modify);
                let changed = self
                    .turn_state
                    .as_ref()
                    .is_some_and(|turn| turn.started_revision < self.draft.draft_revision);
                if !scope.ok || requires_change && !changed {
                    let mut missing = scope.missing;
                    if requires_change && !changed {
                        missing.push("draft_change".to_string());
                    }
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "TURN_SCOPE_INCOMPLETE",
                                "turn.requirements",
                                format!(
                                    "The current Draft is missing requirements: {}",
                                    missing.join(", ")
                                ),
                                "Use the routed mutation tools to make the requested Draft change and satisfy every missing requirement",
                            ),
                        ),
                        None,
                    );
                }
                let phase = if brief.verification.validate {
                    AdaptivePhase::Verify
                } else if brief.requested_outcome == RequestedOutcome::ValidatedPreview {
                    AdaptivePhase::Preview
                } else {
                    AdaptivePhase::Reply
                };
                if let Some(state) = self.adaptive_turn.as_mut() {
                    state.scoped_revision = Some(self.draft.draft_revision);
                    state.phase = phase;
                }
                (
                    ToolResult::success(
                        &self.draft,
                        format!("Turn scope satisfied: {}", scope.satisfied.join(", ")),
                    ),
                    None,
                )
            }
            "render_preview" => {
                if let Err(error) = parse_empty_control(name, arguments) {
                    return (ToolResult::failure_from(&self.draft, error), None);
                }
                let Some(brief) = self
                    .adaptive_turn
                    .as_ref()
                    .and_then(|state| state.brief.as_ref())
                    .cloned()
                else {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "TURN_BRIEF_REQUIRED",
                                "tool.render_preview",
                                "No active turn brief exists",
                                "Call set_turn_brief before rendering a preview",
                            ),
                        ),
                        None,
                    );
                };
                if brief.verification.validate
                    && self.draft.validated_revision != Some(self.draft.draft_revision)
                {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "PREVIEW_REQUIRES_CURRENT_VALIDATION",
                                "tool.render_preview",
                                "The current Draft revision is not validated",
                                "Call validate_draft before rendering the preview",
                            ),
                        ),
                        None,
                    );
                }
                let preview = render_preview(&self.draft);
                if let Some(state) = self.adaptive_turn.as_mut() {
                    state.previewed_revision = Some(self.draft.draft_revision);
                    state.phase = AdaptivePhase::Reply;
                }
                let change = serde_json::to_string(&preview)
                    .map(|value| format!("Rendered preview {value}"))
                    .unwrap_or_else(|_| "Rendered preview".to_string());
                (ToolResult::success(&self.draft, change), None)
            }
            "finish_turn" => {
                let finish = match parse_finish_turn(arguments) {
                    Ok(finish) => finish,
                    Err(error) => return (ToolResult::failure_from(&self.draft, error), None),
                };
                let Some(state) = self.adaptive_turn.as_ref() else {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "TURN_BRIEF_REQUIRED",
                                "tool.finish_turn",
                                "No active adaptive turn exists",
                                "Call set_turn_brief before finishing the turn",
                            ),
                        ),
                        None,
                    );
                };
                let Some(brief) = state.brief.as_ref() else {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "TURN_BRIEF_REQUIRED",
                                "tool.finish_turn",
                                "No active turn brief exists",
                                "Call set_turn_brief before finishing the turn",
                            ),
                        ),
                        None,
                    );
                };
                let outcome = match finish.kind {
                    FinishTurnKind::NeedsInput => {
                        let question = finish.question.as_deref().unwrap_or("").trim();
                        let question_allowed = brief.requested_outcome
                            == RequestedOutcome::Discussion
                            || brief.intent == TurnIntent::Brainstorm
                            || brief.intent == TurnIntent::Inspect;
                        if question.is_empty() || !question_allowed {
                            return (
                                ToolResult::failure_from(
                                    &self.draft,
                                    StructuredError::new(
                                        "UNNECESSARY_TURN_QUESTION",
                                        "tool.finish_turn.question",
                                        "The turn does not contain a justified blocking question",
                                        "Continue building or finish with progressed or ready",
                                    ),
                                ),
                                None,
                            );
                        }
                        self.observability.clarification_count += 1;
                        self.needs_input(question.to_string())
                    }
                    FinishTurnKind::Progressed => {
                        if brief.requested_outcome == RequestedOutcome::ValidatedPreview
                            && brief.blocking_decisions.is_empty()
                        {
                            return (
                                ToolResult::failure_from(
                                    &self.draft,
                                    StructuredError::new(
                                        "PREMATURE_TURN_PROGRESS",
                                        "tool.finish_turn.kind",
                                        "A fully specified validated-preview request cannot stop as partial progress",
                                        "Complete scope checking, validation, preview, and finish the turn as ready",
                                    ),
                                ),
                                None,
                            );
                        }
                        let changed = self
                            .turn_state
                            .as_ref()
                            .is_some_and(|turn| turn.started_revision < self.draft.draft_revision);
                        if !changed {
                            return (
                                ToolResult::failure_from(
                                    &self.draft,
                                    StructuredError::new(
                                        "PROGRESS_REQUIRES_CHANGE",
                                        "tool.finish_turn.kind",
                                        "The Draft did not change during this turn",
                                        "Make a useful Draft change or ask a justified question",
                                    ),
                                ),
                                None,
                            );
                        }
                        self.progressed(finish.message.clone())
                    }
                    FinishTurnKind::Ready => {
                        let scope_current =
                            state.scoped_revision == Some(self.draft.draft_revision);
                        let validation_current = !brief.verification.validate
                            || self.draft.validated_revision == Some(self.draft.draft_revision);
                        let simulation_current = brief.verification.simulation
                            != SimulationProfile::StudyRoom
                            || self.draft.simulated_revision == Some(self.draft.draft_revision);
                        let preview_current = brief.requested_outcome
                            != RequestedOutcome::ValidatedPreview
                            || state.previewed_revision == Some(self.draft.draft_revision);
                        if state.phase != AdaptivePhase::Reply
                            || !scope_current
                            || !validation_current
                            || !simulation_current
                            || !preview_current
                        {
                            return (
                                ToolResult::failure_from(
                                    &self.draft,
                                    StructuredError::new(
                                        "TURN_NOT_READY",
                                        "tool.finish_turn.kind",
                                        "The current turn has not completed its scope and verification path",
                                        "Finish scope checking, validation, supported simulation, and preview before ready",
                                    ),
                                ),
                                None,
                            );
                        }
                        self.ready(finish.message.clone())
                    }
                };
                (
                    ToolResult::success(&self.draft, "Prepared the human-facing turn response"),
                    Some(outcome),
                )
            }
            _ => (
                ToolResult::failure_from(
                    &self.draft,
                    StructuredError::new(
                        "UNKNOWN_CONTROL_TOOL",
                        "tool",
                        "The requested turn control tool does not exist",
                        "Use one of the routed turn control tools",
                    ),
                ),
                None,
            ),
        }
    }

    fn append_not_executed(&mut self, calls: &[ToolCall]) {
        for call in calls {
            let result = self.not_executed_result();
            self.messages
                .push(Message::tool(call.id.clone(), result.as_json()));
        }
    }
}

impl<C: LlmClient> DesignSession<C> {
    pub async fn run_burst(&mut self, human_message: &str) -> BurstOutcome {
        if self.intent_recipe.is_some() {
            return self.run_intent_recipe_burst(human_message).await;
        }
        self.begin_turn(human_message);
        if matches!(self.repair_state, Some(RepairState::Failed(_))) {
            self.repair_state = None;
            self.observability.repair_escalations += 1;
        }
        self.current_human_message_index = Some(self.messages.len());
        self.messages.push(Message::user(human_message));
        self.prose_nudged = false;
        loop {
            if self.turn_model_calls() >= self.config.max_model_calls {
                if let Some(state) = self.repair_state.clone() {
                    let error = StructuredError::new(
                        "REPAIR_MODEL_CALL_LIMIT",
                        "repair.model_calls",
                        "The repair could not continue because the model call budget is exhausted",
                        "Escalate to a human before continuing the design",
                    );
                    return self.fail_repair(state.ticket().clone(), error, true);
                }
                return self.halt(
                    "MODEL_CALL_LIMIT_EXHAUSTED",
                    "The session exhausted its model call budget",
                    Some(LimitKind::ModelCalls),
                );
            }
            self.append_anchor();
            let routed_tools = self.routed_tools();
            let outbound_messages = if self.planned_frontier_name() == Some("review_turn_plan") {
                self.fit_plan_review_context(&routed_tools)
            } else {
                self.fit_context(&routed_tools)
            };
            let Some(outbound_messages) = outbound_messages else {
                if let Some(state) = self.repair_state.clone() {
                    let error = StructuredError::new(
                        "REPAIR_CONTEXT_LIMIT",
                        "repair.context",
                        "The repair directive and current Draft do not fit the context budget",
                        "Increase the context budget or escalate to a human",
                    );
                    return self.fail_repair(state.ticket().clone(), error, true);
                }
                return self.halt(
                    "CONTEXT_CHAR_LIMIT_EXHAUSTED",
                    "The system prompt, tool schemas, and current Draft anchor do not fit",
                    Some(LimitKind::ContextChars),
                );
            };

            self.record_model_call();
            let response = match self
                .client
                .complete(&outbound_messages, &routed_tools)
                .await
            {
                Ok(response) => response,
                Err(_) => {
                    let error = StructuredError::new(
                        "LLM_CLIENT_ERROR",
                        "llm",
                        "The model request failed",
                        "Stop the burst and retry after the model gateway is available",
                    );
                    if let Some(state) = self.repair_state.clone() {
                        return self.fail_repair(state.ticket().clone(), error, true);
                    }
                    self.last_error = Some(error);
                    return self.halt("LLM_CLIENT_ERROR", "The model client failed", None);
                }
            };

            if self.repair_state.is_some() {
                if let Some(outcome) = self.handle_repair_response(response, &routed_tools).await {
                    return outcome;
                }
                continue;
            }

            match response {
                LlmResponse::ToolCalls(calls) => {
                    let plan_frontier = self.planned_frontier_name().map(str::to_string);
                    let awaiting_plan = plan_frontier.is_some();
                    let modify_selects_plan = self.awaiting_planned_modify_choice()
                        && calls.iter().any(|call| call.name == "set_turn_plan");
                    if calls.is_empty() {
                        if awaiting_plan {
                            self.messages.push(Message::assistant(
                                "TURN_PLAN_RESPONSE_REJECTED: empty tool call batch",
                            ));
                            let error = StructuredError::new(
                                "TURN_PLAN_RESPONSE_REQUIRED",
                                "turn.plan.response",
                                format!(
                                    "The planning response did not submit {}",
                                    plan_frontier.as_deref().unwrap_or("the planned frontier")
                                ),
                                format!(
                                    "Call the sole routed {} tool",
                                    plan_frontier.as_deref().unwrap_or("planned frontier")
                                ),
                            );
                            if let Some(outcome) = self.consume_planned_response_failure(
                                plan_frontier.as_deref().unwrap_or("set_turn_plan"),
                                Some(error),
                            ) {
                                return outcome;
                            }
                            continue;
                        }
                        return self.halt(
                            "EMPTY_TOOL_CALL_BATCH",
                            "The model returned an empty tool call batch",
                            None,
                        );
                    }
                    if (awaiting_plan || modify_selects_plan)
                        && !valid_planned_tool_call_ids(&calls)
                    {
                        self.messages.push(Message::assistant(
                            "TURN_PLAN_RESPONSE_REJECTED: invalid tool call identifiers",
                        ));
                        let error = StructuredError::new(
                            "TURN_PLAN_RESPONSE_REJECTED",
                            "turn.plan.response",
                            "The planning response contained empty or duplicate tool call identifiers",
                            format!(
                                "Return one {} call with a non-empty unique identifier",
                                plan_frontier.as_deref().unwrap_or("set_turn_plan")
                            ),
                        );
                        if let Some(outcome) = self.consume_planned_response_failure(
                            plan_frontier.as_deref().unwrap_or("set_turn_plan"),
                            Some(error),
                        ) {
                            return outcome;
                        }
                        continue;
                    }
                    if let Some(frontier) = plan_frontier.as_deref() {
                        if calls.len() != 1 || calls[0].name != frontier {
                            self.messages
                                .push(Message::assistant_tool_calls(calls.clone()));
                            let error = StructuredError::new(
                                "TURN_PLAN_RESPONSE_REJECTED",
                                "turn.plan.response",
                                format!(
                                    "The planning response did not contain exactly one {frontier} call"
                                ),
                                format!("Call only the sole routed {frontier} tool"),
                            );
                            let result = ToolResult::failure_from(&self.draft, error.clone());
                            self.record_failure(None, &result);
                            for call in &calls {
                                self.messages
                                    .push(Message::tool(call.id.clone(), result.as_json()));
                            }
                            if let Some(outcome) =
                                self.consume_planned_response_failure(frontier, None)
                            {
                                return outcome;
                            }
                            continue;
                        }
                    }
                    if modify_selects_plan && (calls.len() != 1 || calls[0].name != "set_turn_plan")
                    {
                        if let Some(outcome) = self.reject_mixed_planned_modify_batch(&calls) {
                            return outcome;
                        }
                        continue;
                    }
                    self.prose_nudged = false;
                    self.messages
                        .push(Message::assistant_tool_calls(calls.clone()));
                    let mut failed = false;
                    let mut submitted_frontier = false;
                    let mut failed_planned_frontier = None;
                    for (index, call) in calls.iter().enumerate() {
                        if failed {
                            let result = self.not_executed_result();
                            self.messages
                                .push(Message::tool(call.id.clone(), result.as_json()));
                            continue;
                        }
                        if self.turn_tool_calls() >= self.config.max_tool_calls {
                            self.append_not_executed(&calls[index..]);
                            return self.halt(
                                "TOOL_CALL_LIMIT_EXHAUSTED",
                                "The session exhausted its executed tool call budget",
                                Some(LimitKind::ToolCalls),
                            );
                        }
                        self.record_tool_call();
                        let phase_before = self.adaptive_turn.as_ref().map(|state| state.phase);
                        let control = is_control_tool(&call.name);
                        let available = routed_tools.iter().any(|tool| tool.name == call.name)
                            && (control || tool_is_available(&self.draft, &call.name));
                        if available && call.name == "set_turn_plan" {
                            self.observability.plan_submissions =
                                self.observability.plan_submissions.saturating_add(1);
                        }
                        let (result, control_outcome) = if available && control {
                            self.dispatch_control_tool(&call.name, &call.arguments)
                        } else if available {
                            (
                                dispatch_tool(&mut self.draft, &call.name, &call.arguments).await,
                                None,
                            )
                        } else {
                            (
                                self.unavailable_tool_result(&call.name, &self.routed_tools()),
                                None,
                            )
                        };
                        if result.is_ok() && is_mutation_tool(&call.name) {
                            self.observability
                                .distinct_mutation_tools
                                .insert(call.name.clone());
                            *self
                                .observability
                                .mutation_tool_calls
                                .entry(call.name.clone())
                                .or_default() += 1;
                        }
                        if result.is_ok() {
                            self.last_error = None;
                        }
                        self.advance_adaptive_after_draft_tool(&call.name, result.is_ok());
                        self.record_failure(available.then_some(call.name.as_str()), &result);
                        let is_failure = !result.is_ok();
                        if is_failure
                            && available
                            && matches!(
                                call.name.as_str(),
                                "set_turn_plan" | "review_turn_plan" | "fill_turn_plan_packet"
                            )
                        {
                            failed_planned_frontier = Some(call.name.clone());
                        }
                        if available
                            && (plan_frontier.as_deref() == Some(call.name.as_str())
                                || modify_selects_plan && call.name == "set_turn_plan")
                            && result.is_ok()
                        {
                            submitted_frontier = true;
                        }
                        self.messages
                            .push(Message::tool(call.id.clone(), result.as_json()));
                        if call.name == "fill_turn_plan_packet" && result.is_ok() {
                            self.packet_correction_remaining = 1;
                        }
                        if call.name == "fill_turn_plan_packet"
                            && result.is_ok()
                            && self.plan_assembly.is_some()
                        {
                            if self
                                .plan_assembly
                                .as_ref()
                                .is_some_and(|assembly| assembly.review_pending)
                            {
                                self.append_plan_review_directive();
                            } else {
                                self.append_plan_packet_directive();
                            }
                        }
                        if call.name == "set_turn_brief" && !result.is_ok() {
                            if self.brief_correction_remaining == 0 {
                                self.append_not_executed(&calls[index + 1..]);
                                return self.halt(
                                    "TURN_BRIEF_REPAIR_FAILED",
                                    "The single automatic turn-brief repair failed",
                                    None,
                                );
                            }
                            self.brief_correction_remaining -= 1;
                            self.add_planned_nudge("set_turn_brief");
                        }
                        if self.adaptive_enabled
                            && matches!(
                                call.name.as_str(),
                                "set_turn_brief"
                                    | "set_turn_plan"
                                    | "review_turn_plan"
                                    | "fill_turn_plan_packet"
                                    | "check_turn_scope"
                            )
                            && result.is_ok()
                        {
                            let outcome = if matches!(
                                call.name.as_str(),
                                "set_turn_plan" | "review_turn_plan" | "fill_turn_plan_packet"
                            ) {
                                self.run_automatic_planned_execution().await
                            } else {
                                self.run_automatic_adaptive_phases().await
                            };
                            if let Some(outcome) = outcome {
                                self.append_phase_transition_not_executed(&calls[index + 1..]);
                                return outcome;
                            }
                            if matches!(
                                call.name.as_str(),
                                "set_turn_plan" | "review_turn_plan" | "fill_turn_plan_packet"
                            ) {
                                self.append_phase_transition_not_executed(&calls[index + 1..]);
                                break;
                            }
                        }
                        if let Some(outcome) = control_outcome {
                            self.append_phase_transition_not_executed(&calls[index + 1..]);
                            return outcome;
                        }
                        if is_failure {
                            failed = true;
                            self.append_not_executed(&calls[index + 1..]);
                            if self.turn_gate_failures() >= self.config.max_gate_failures {
                                return self.halt(
                                    "GATE_FAILURE_LIMIT_EXHAUSTED",
                                    "The session exhausted its validation and simulation failure budget",
                                    Some(LimitKind::GateFailures),
                                );
                            }
                            if available && !control {
                                if let Some(ticket) =
                                    self.root_repair_ticket(call, &result, &routed_tools)
                                {
                                    self.append_repair_directive(&ticket);
                                    self.repair_state = Some(RepairState::AwaitingAttempt(ticket));
                                }
                            }
                            break;
                        }
                        let phase_after = self.adaptive_turn.as_ref().map(|state| state.phase);
                        if phase_before != phase_after {
                            self.append_phase_transition_not_executed(&calls[index + 1..]);
                            break;
                        }
                    }
                    if (awaiting_plan || modify_selects_plan) && !submitted_frontier {
                        let repair_frontier = self
                            .planned_frontier_name()
                            .unwrap_or_else(|| plan_frontier.as_deref().unwrap_or("set_turn_plan"))
                            .to_string();
                        let semantic_replan = matches!(
                            failed_planned_frontier.as_deref(),
                            Some("fill_turn_plan_packet" | "review_turn_plan")
                        ) && self.plan_assembly.is_none();
                        let coverage_extension = failed_planned_frontier.as_deref()
                            == Some("review_turn_plan")
                            && self
                                .plan_assembly
                                .as_ref()
                                .is_some_and(|assembly| assembly.coverage_extension_pending);
                        let outcome = if coverage_extension {
                            self.consume_planned_coverage_extension_failure()
                        } else if semantic_replan {
                            self.consume_planned_replan_failure()
                        } else {
                            self.consume_planned_response_failure(&repair_frontier, None)
                        };
                        if let Some(outcome) = outcome {
                            return outcome;
                        }
                    }
                    if failed {
                        continue;
                    }
                }
                LlmResponse::Text(text) => {
                    let plan_frontier = self.planned_frontier_name().map(str::to_string);
                    let awaiting_plan = plan_frontier.is_some();
                    self.messages.push(Message::assistant(text.clone()));
                    if awaiting_plan {
                        let prose = compact_text(&text);
                        let error = StructuredError::new(
                            "TURN_PLAN_RESPONSE_REQUIRED",
                            "turn.plan.response",
                            format!(
                                "The planning response returned prose instead of {}: {prose}",
                                plan_frontier.as_deref().unwrap_or("the planned frontier"),
                            ),
                            format!(
                                "Call the sole routed {} tool",
                                plan_frontier.as_deref().unwrap_or("planned frontier")
                            ),
                        );
                        if let Some(outcome) = self.consume_planned_response_failure(
                            plan_frontier.as_deref().unwrap_or("set_turn_plan"),
                            Some(error),
                        ) {
                            return outcome;
                        }
                        continue;
                    }
                    if self.adaptive_enabled {
                        if !self.prose_nudged {
                            self.prose_nudged = true;
                            self.add_nudge();
                            continue;
                        }
                        return self.halt("UNSTRUCTURED_MODEL_TEXT", &text, None);
                    }
                    if let Some(question) = text.strip_prefix("QUESTION:") {
                        self.observability.clarification_count += 1;
                        return self.needs_input(question.trim().to_string());
                    }
                    if let Some(summary) = text.strip_prefix("PROGRESSED:") {
                        if self
                            .turn_state
                            .as_ref()
                            .is_some_and(|state| state.started_revision < self.draft.draft_revision)
                        {
                            return self.progressed(summary.trim().to_string());
                        }
                        self.add_nudge();
                        continue;
                    }
                    if let Some(summary) = text
                        .strip_prefix("READY:")
                        .or_else(|| text.strip_prefix("DONE:"))
                    {
                        if self.draft.validated_revision == Some(self.draft.draft_revision) {
                            return self.ready(summary.trim().to_string());
                        }
                        self.add_nudge();
                        continue;
                    }
                    if !self.prose_nudged {
                        self.prose_nudged = true;
                        self.add_nudge();
                        continue;
                    }
                    return self.halt("UNSTRUCTURED_MODEL_TEXT", &text, None);
                }
            }
        }
    }
}

fn is_genuine_human_message(message: &Message) -> bool {
    message.role == MessageRole::User
        && message.content != NUDGE
        && !message.content.starts_with(REPAIR_REQUIRED_PREFIX)
        && !message.content.starts_with(COVERAGE_REVIEW_PREFIX)
        && !message.content.starts_with(PACKET_CONTINUE_PREFIX)
        && !message.content.starts_with(PLAN_FRONTIER_RETRY_PREFIX)
        && !message.content.starts_with(PLAN_REVIEW_RETRY_PREFIX)
}

fn valid_planned_tool_call_ids(calls: &[ToolCall]) -> bool {
    let mut ids = BTreeSet::new();
    calls
        .iter()
        .all(|call| !call.id.trim().is_empty() && ids.insert(call.id.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outline_item(index: usize, op: PlanOp) -> PlanOutlineItem {
        PlanOutlineItem {
            id: format!("plan_{index:02}"),
            op,
            owner: if matches!(op, PlanOp::Panel | PlanOp::Modal | PlanOp::Rule) {
                "draft".to_string()
            } else {
                "rule".to_string()
            },
            goal: format!("item {index}"),
        }
    }

    #[test]
    fn semantic_packets_split_maximal_heterogeneous_frontiers() {
        let ops = [
            PlanOp::Panel,
            PlanOp::Button,
            PlanOp::Modal,
            PlanOp::Rule,
            PlanOp::OpenModal,
            PlanOp::Rule,
            PlanOp::DeferEphemeral,
            PlanOp::CreateRole,
            PlanOp::CreateChannel,
            PlanOp::UpsertOverwrite,
            PlanOp::UpsertOverwrite,
            PlanOp::GrantRole,
            PlanOp::PostPanel,
            PlanOp::PostPanel,
            PlanOp::RegisterInstance,
            PlanOp::EditResponse,
        ];
        let outline = ops
            .into_iter()
            .enumerate()
            .map(|(index, op)| outline_item(index + 1, op))
            .collect::<Vec<_>>();

        assert_eq!(
            plan_packet_ends(&outline),
            vec![3, 5, 6, 7, 8, 9, 12, 13, 14, 15, 16]
        );
    }

    #[test]
    fn resource_packet_exposes_created_aliases_before_their_consumers() {
        let ops = [
            PlanOp::Rule,
            PlanOp::DeferEphemeral,
            PlanOp::CreateRole,
            PlanOp::CreateChannel,
            PlanOp::UpsertOverwrite,
            PlanOp::UpsertOverwrite,
            PlanOp::GrantRole,
        ];
        let outline = ops
            .into_iter()
            .enumerate()
            .map(|(index, op)| outline_item(index + 1, op))
            .collect::<Vec<_>>();

        assert_eq!(plan_packet_ends(&outline), vec![1, 2, 3, 4, 7]);
    }

    #[test]
    fn failed_multi_item_packet_refines_every_remaining_item_to_a_single_frontier() {
        let ops = [PlanOp::Panel, PlanOp::Panel, PlanOp::Panel, PlanOp::Panel];
        let outline = ops
            .into_iter()
            .enumerate()
            .map(|(index, op)| outline_item(index + 1, op))
            .collect::<Vec<_>>();
        let mut assembly = PlanAssembly::new(5, outline);

        assert_eq!(assembly.current_packet().len(), 4);
        assert!(assembly.refine_remaining_packets());
        assert_eq!(assembly.packet_ends, vec![1, 2, 3, 4]);
        assert_eq!(assembly.current_packet().len(), 1);
        assert!(assembly.take_packet_refinement());
        assert!(!assembly.take_packet_refinement());
        assert!(!assembly.refine_remaining_packets());
    }

    #[test]
    fn coverage_extension_rebases_ids_and_inferred_owners_after_the_retained_candidate() {
        let initial = vec![
            outline_item(1, PlanOp::Rule),
            PlanOutlineItem {
                id: "plan_02_defer_ephemeral".to_string(),
                op: PlanOp::DeferEphemeral,
                owner: "__plan_owner__:plan_01".to_string(),
                goal: "defer".to_string(),
            },
        ];
        let mut assembly = PlanAssembly::new(0, initial);
        assembly.cursor = 2;
        assembly.review_pending = true;
        assembly
            .requirements
            .push(ScopeRequirement::NoUnresolvedReferences {
                id: "plan_no_unresolved_references".to_string(),
            });
        assembly
            .begin_coverage_extension(
                "missing response".to_string(),
                CoverageObligation::ReviewerMissing,
            )
            .unwrap();
        let extension = vec![
            PlanOutlineItem {
                id: "plan_01_rule".to_string(),
                op: PlanOp::Rule,
                owner: "draft".to_string(),
                goal: "second rule".to_string(),
            },
            PlanOutlineItem {
                id: "plan_02_respond_ephemeral".to_string(),
                op: PlanOp::RespondEphemeral,
                owner: "__plan_owner__:plan_01_rule".to_string(),
                goal: "respond".to_string(),
            },
        ];

        assembly.append_coverage_outline(extension).unwrap();

        assert_eq!(assembly.packet_ends, vec![2, 4]);
        assert_eq!(assembly.outline[2].id, "plan_03_rule");
        assert_eq!(assembly.outline[3].id, "plan_04_respond_ephemeral");
        assert_eq!(assembly.outline[3].owner, "__plan_owner__:plan_03_rule");
        assert_eq!(assembly.cursor, 2);
        assert_eq!(assembly.current_packet(), &assembly.outline[2..]);
        assert!(assembly.requirements.is_empty());
        assert!(!assembly.coverage_extension_pending);
        assert!(!assembly.review_pending);
    }

    #[test]
    fn instance_registration_coverage_extension_requires_every_owner_once() {
        let mut assembly = PlanAssembly::new(0, vec![outline_item(1, PlanOp::PostPanel)]);
        assembly.cursor = 1;
        let owners = BTreeSet::from(["alpha".to_string(), "beta".to_string()]);
        assembly
            .begin_coverage_extension(
                "missing registrations".to_string(),
                CoverageObligation::InstanceRegistrations(owners),
            )
            .unwrap();

        let incomplete = vec![PlanOutlineItem {
            id: "plan_01_register_instance".to_string(),
            op: PlanOp::RegisterInstance,
            owner: "alpha".to_string(),
            goal: "register alpha".to_string(),
        }];
        let error = assembly.append_coverage_outline(incomplete).unwrap_err();

        assert_eq!(error.code, "TURN_PLAN_INSTANCE_COVERAGE_SCOPE");
        assert!(assembly.coverage_extension_pending);
        let complete = vec![
            PlanOutlineItem {
                id: "plan_01_register_instance".to_string(),
                op: PlanOp::RegisterInstance,
                owner: "alpha".to_string(),
                goal: "register alpha".to_string(),
            },
            PlanOutlineItem {
                id: "plan_02_register_instance".to_string(),
                op: PlanOp::RegisterInstance,
                owner: "beta".to_string(),
                goal: "register beta".to_string(),
            },
        ];
        assembly.append_coverage_outline(complete).unwrap();

        assert!(assembly.structural_coverage_extended);
        assert!(!assembly.coverage_extension_pending);
        let error = assembly
            .begin_coverage_extension(
                "same structural gap".to_string(),
                CoverageObligation::InstanceRegistrations(BTreeSet::from(["alpha".to_string()])),
            )
            .unwrap_err();
        assert_eq!(error.code, "TURN_PLAN_COVERAGE_EXTENSION_EXHAUSTED");
        assembly
            .begin_coverage_extension(
                "review gap".to_string(),
                CoverageObligation::ReviewerMissing,
            )
            .unwrap();
        assert!(assembly.coverage_extension_pending);
        assert!(!assembly.review_coverage_extended);
    }

    #[test]
    fn adaptive_and_planned_session_surfaces_are_isolated() {
        let defaults = SessionConfig::default();
        assert_eq!(defaults.max_model_calls, 12);
        assert_eq!(defaults.max_tool_calls, 24);

        let adaptive = DesignSession::with_adaptive_config((), defaults.clone());
        let planned = DesignSession::with_planned_config((), defaults);
        assert_eq!(adaptive.messages[0].content, DEFAULT_SYSTEM_PROMPT);
        assert_eq!(planned.messages[0].content, PLANNED_SYSTEM_PROMPT);
        let adaptive_brief = adaptive
            .tools
            .iter()
            .find(|tool| tool.name == "set_turn_brief")
            .unwrap();
        let planned_brief = planned
            .tools
            .iter()
            .find(|tool| tool.name == "set_turn_brief")
            .unwrap();
        assert!(adaptive_brief
            .parameters
            .pointer("/properties/intent")
            .is_some());
        assert!(adaptive_brief
            .parameters
            .pointer("/properties/strategy")
            .is_none());
        assert!(planned_brief
            .parameters
            .pointer("/properties/strategy")
            .is_some());
        assert!(planned_brief
            .parameters
            .pointer("/properties/intent")
            .is_none());
        let adaptive_plan = adaptive
            .tools
            .iter()
            .find(|tool| tool.name == "set_turn_plan")
            .unwrap();
        let planned_plan = planned
            .tools
            .iter()
            .find(|tool| tool.name == "set_turn_plan")
            .unwrap();
        assert!(adaptive_plan
            .parameters
            .pointer("/properties/requirements")
            .is_some());
        assert!(planned_plan
            .parameters
            .pointer("/properties/steps")
            .is_some());
    }
}
