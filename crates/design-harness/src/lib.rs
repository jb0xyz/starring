pub mod draft;
pub mod errors;
pub mod gates;
pub mod intent;
pub mod llm;
pub mod session;
mod strict_json;
pub mod tools;
pub mod turn;

pub use draft::{Draft, DraftSummary};
pub use errors::{
    translate_run_error, translate_validation_error, StructuredError, ToolFailure, ToolResult,
    ToolSuccess,
};
pub use gates::{simulate_draft, validate_draft};
pub use intent::{
    verify_preview_ruleset_v1, IntentRequestedOutcome, PreviewRulesetVerificationErrorV1,
};
pub use llm::{
    LlmClient, LlmCompletionProvenanceV1, LlmError, LlmResponse, Message, MessageRole, ToolCall,
};
pub use resource_resolution::ResourceBindingFingerprint;
pub use resource_resolution::ResourceBindingMap;
pub use session::{
    AuthoringContractV1, BurstOutcome, DesignSession, HaltReport, IntentDecisionSourceV2,
    IntentFallbackKind, IntentFallbackV1, IntentRecipeReceiptV2, IntentRecipeStatusV2,
    IntentRouteDecisionKindV2, IntentRouteDecisionV2, LimitKind, Observability,
    PinnedIntentRecipeV2, PreviewReadyArtifactError, PreviewReadyArtifactV1, RepairKind,
    RepairState, RepairTicket, SessionConfig, SessionSnapshot, SessionSnapshotError, TurnPhase,
    TurnState, DEFAULT_SYSTEM_PROMPT, INTENT_ADJUDICATOR_VERSION_V2, INTENT_ADJUDICATOR_VERSION_V3,
    INTENT_ADJUDICATOR_VERSION_V4, INTENT_RECIPE_PROTOCOL_VERSION_V2,
    INTENT_RECIPE_PROTOCOL_VERSION_V3, INTENT_RECIPE_PROTOCOL_VERSION_V4, SESSION_SNAPSHOT_VERSION,
};
pub use tools::{dispatch_tool, tool_definitions, ToolDefinition};
pub use turn::{
    check_scope, control_tool_definitions, interpret_intent_core_frontier,
    interpret_intent_turn_frontier, parse_finish_turn, parse_interpret_intent_core,
    parse_interpret_intent_turn, parse_private_study_room_details, parse_resolve_intent_decision,
    parse_route_intent_turn, parse_turn_brief, private_study_room_details_frontier, render_preview,
    resolve_intent_decision_frontier, route_intent_turn_frontier, ActionKind, AdaptivePhase,
    AdaptiveTurnState, BlockingDecision, CloseAuthorizationV2, DraftPreview, EconomyRequirementV2,
    FinishTurn, FinishTurnKind, IntentAutomationKindV2, IntentBoundaryRequestV2,
    IntentCoreInterpretationV4, IntentInterpretationV2, IntentLocaleHintV2,
    IntentRecipeDetailFacetV3, IntentRequestModeV2, IntentRouteInputV1, PersistenceRequirementV2,
    PrivateStudyRoomControlsInterpretationV2, PrivateStudyRoomDetailsV1, RequestedOutcome,
    ResolveIntentDecisionInputV1, RouteIntentTurnInputV1, RuntimeRequirementsV2, ScopeAction,
    ScopeActionTarget, ScopeButtonRoute, ScopeCheck, ScopeInstanceRef, ScopeInstanceResources,
    ScopeManifestEntry, ScopeModalField, ScopeModalFieldStyle, ScopeOverwriteTarget,
    ScopePermission, ScopePostPanelButton, ScopePostPanelButtonRoute, ScopeRequirement,
    ScopeResourceRef, ScopeRoleRef, ScopeTrigger, SimulationProfile, TimerRequirementV2, TurnBrief,
    TurnIntent, TurnVerification, EXTRACT_PRIVATE_STUDY_ROOM_DETAILS, INTERPRET_INTENT_CORE,
    INTERPRET_INTENT_TURN,
};
