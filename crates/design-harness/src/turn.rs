mod plan;
mod plan_input;
mod protocol;
mod scope;

pub(crate) use plan::{normalize_turn_plan, validate_final_planned_action_order};
pub(crate) use plan_input::{
    assign_repeat_targets as assign_turn_plan_repeat_targets,
    derive_instance_manifests as derive_turn_plan_instance_manifests,
    merge_extension_action_lanes as merge_turn_plan_extension_action_lanes,
    missing_instance_registration_owners as missing_turn_plan_instance_registration_owners,
    rebase_outline_ids as rebase_turn_plan_outline_ids,
    resolve_created_reference_kinds as resolve_turn_plan_created_reference_kinds,
    resolve_extension_outline_parent_owners as resolve_turn_plan_extension_outline_parent_owners,
    resolve_outline_parent_owners as resolve_turn_plan_outline_parent_owners,
    resolve_owners as resolve_turn_plan_owners,
    resolve_response_lifecycle_actions as resolve_turn_plan_response_lifecycle_actions,
    resolve_unique_instance_aliases as resolve_turn_plan_unique_instance_aliases,
    validate_new_rule_action_coverage as validate_turn_plan_new_rule_action_coverage, PlanOp,
    PlanOutlineItem, TurnPlanSubmission, MAX_PLAN_GOAL_TOTAL_CHARS, MAX_PLAN_ITEMS,
    MAX_PLAN_PACKET_ITEMS,
};
pub(crate) use protocol::{
    parse_planned_turn_brief, parse_turn_plan, parse_turn_plan_packet_scoped,
    parse_turn_plan_review, parse_turn_plan_review_oracle, plan_packet_definition,
    plan_review_definition, planned_control_tool_definitions,
};

pub use protocol::{
    control_tool_definitions, parse_empty_control, parse_finish_turn, parse_turn_brief,
    render_preview, AdaptivePhase, AdaptiveTurnState, BlockingDecision, DraftPreview, FinishTurn,
    FinishTurnKind, RequestedOutcome, SimulationProfile, TurnBrief, TurnIntent, TurnVerification,
};
pub use scope::{
    check_scope, required_mutation_tools, ActionKind, ScopeAction, ScopeActionTarget,
    ScopeButtonRoute, ScopeCheck, ScopeInstanceRef, ScopeInstanceResources, ScopeManifestEntry,
    ScopeModalField, ScopeModalFieldStyle, ScopeOverwriteTarget, ScopePermission,
    ScopePostPanelButton, ScopePostPanelButtonRoute, ScopeRequirement, ScopeResourceRef,
    ScopeRoleRef, ScopeTrigger,
};
