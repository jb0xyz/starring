mod execution;
mod intent_core;
#[cfg(test)]
mod intent_core_tests;
mod intent_interpretation;
#[cfg(test)]
mod intent_interpretation_tests;
mod intent_protocol;
mod intent_recipe_details;
#[cfg(test)]
mod intent_recipe_details_tests;
mod intent_text;
mod plan;
mod plan_input;
mod protocol;
mod schema;
mod scope;

pub(crate) use execution::{
    execute_plan_atomically, execute_plan_atomically_with_bindings, ExecutionRecord,
};
pub use intent_core::{
    interpret_intent_core_frontier, parse_interpret_intent_core, IntentCoreInterpretationV3,
    IntentRecipeDetailFacetV3, INTERPRET_INTENT_CORE,
};
pub use intent_interpretation::{
    interpret_intent_turn_frontier, parse_interpret_intent_turn, CloseAuthorizationV2,
    EconomyRequirementV2, IntentAutomationKindV2, IntentBoundaryRequestV2, IntentInterpretationV2,
    IntentLocaleHintV2, IntentRequestModeV2, PersistenceRequirementV2,
    PrivateStudyRoomControlsInterpretationV2, RuntimeRequirementsV2, TimerRequirementV2,
    INTERPRET_INTENT_TURN,
};
pub(crate) use intent_protocol::RESOLVE_INTENT_DECISION;
pub use intent_protocol::{
    parse_resolve_intent_decision, parse_route_intent_turn, resolve_intent_decision_frontier,
    route_intent_turn_frontier, IntentRouteInputV1, ResolveIntentDecisionInputV1,
    RouteIntentTurnInputV1,
};
pub use intent_recipe_details::{
    parse_private_study_room_details, private_study_room_details_frontier,
    PrivateStudyRoomDetailsV1, EXTRACT_PRIVATE_STUDY_ROOM_DETAILS,
};
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
    plan_review_definition, planned_control_tool_definitions, render_preview_with_bindings,
};
pub(crate) use scope::check_scope_with_bindings;

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
