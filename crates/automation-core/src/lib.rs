pub mod adapter;
pub mod event;
mod execution;
pub mod interpret;
pub mod mock;
pub mod modal_input;
pub mod plan;
pub mod policy;
mod prepare;
pub mod run;
pub mod template;
pub mod validate;

pub use adapter::{
    AdapterError, AdapterErrorKind, AutomationServices, CreateChannelSpec, CreateRoleSpec,
    DiscordMutationAdapter, InteractionResponder, PostPanelButtonSpec, PostPanelSpec,
    ResolvedButtonRoute,
};
pub use event::{
    EventKind, ResolvedInstanceContext, RunningRuleSetIdentity, RuntimeContext, RuntimeEvent,
};
pub use execution::execute_prepared_event;
pub use interpret::interpret;
pub use mock::{
    MockInstanceTeardownService, MockInteractionResponder, MockMutationAdapter, MutationCall,
    ResponderCall,
};
pub use modal_input::{normalize_modal_submit_inputs, ModalInputError};
pub use plan::{
    ActionPlan, CreatedResource, ModalPresentation, PlannedAction, PlannedChannel,
    PlannedOverwriteTarget, PlannedRole, ResponseDeliveryOutcome, RunResult, TeardownActionResult,
};
pub use policy::{analyze, privileged_mask, DynamicAction, PolicyFinding};
pub use prepare::{prepare_event_execution, PreparedEventExecution};
pub use run::{handle_event, run, HandleOutcome};
pub use template::{SanitizeContext, TemplateError, TemplateString};
pub use validate::{validate, validate_bindings, validate_structural, ValidationError};
