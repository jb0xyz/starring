pub mod draft;
pub mod errors;
pub mod gates;
pub mod llm;
pub mod session;
pub mod tools;

pub use draft::{Draft, DraftSummary};
pub use errors::{
    translate_run_error, translate_validation_error, StructuredError, ToolFailure, ToolResult,
    ToolSuccess,
};
pub use gates::{simulate_draft, validate_draft};
pub use llm::{LlmClient, LlmError, LlmResponse, Message, MessageRole, ToolCall};
pub use session::{
    BurstOutcome, DesignSession, HaltReport, LimitKind, Observability, SessionConfig,
    DEFAULT_SYSTEM_PROMPT,
};
pub use tools::{dispatch_tool, tool_definitions, ToolDefinition};
