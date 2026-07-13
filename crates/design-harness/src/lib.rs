pub mod draft;
pub mod errors;
pub mod gates;
pub mod llm;
pub mod session;
pub mod tools;
pub mod turn;

pub use draft::{Draft, DraftSummary};
pub use errors::{
    translate_run_error, translate_validation_error, StructuredError, ToolFailure, ToolResult,
    ToolSuccess,
};
pub use gates::{simulate_draft, validate_draft};
pub use llm::{LlmClient, LlmError, LlmResponse, Message, MessageRole, ToolCall};
pub use session::{
    BurstOutcome, DesignSession, HaltReport, LimitKind, Observability, RepairKind, RepairState,
    RepairTicket, SessionConfig, SessionSnapshot, SessionSnapshotError, TurnPhase, TurnState,
    DEFAULT_SYSTEM_PROMPT, SESSION_SNAPSHOT_VERSION,
};
pub use tools::{dispatch_tool, tool_definitions, ToolDefinition};
pub use turn::{
    check_scope, control_tool_definitions, parse_finish_turn, parse_turn_brief, render_preview,
    ActionKind, AdaptivePhase, AdaptiveTurnState, BlockingDecision, DraftPreview, FinishTurn,
    FinishTurnKind, RequestedOutcome, ScopeAction, ScopeActionTarget, ScopeButtonRoute, ScopeCheck,
    ScopeInstanceRef, ScopeInstanceResources, ScopeManifestEntry, ScopeModalField,
    ScopeModalFieldStyle, ScopeOverwriteTarget, ScopePermission, ScopePostPanelButton,
    ScopePostPanelButtonRoute, ScopeRequirement, ScopeResourceRef, ScopeRoleRef, ScopeTrigger,
    SimulationProfile, TurnBrief, TurnIntent, TurnVerification,
};
