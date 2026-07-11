pub mod adapter;
pub mod event;
pub mod interpret;
pub mod mock;
pub mod plan;
pub mod policy;
pub mod run;
pub mod template;
pub mod validate;

pub use adapter::{
    AdapterError, AdapterErrorKind, AutomationServices, CreateChannelSpec, CreateRoleSpec,
    DiscordMutationAdapter, InteractionResponder, PostPanelSpec,
};
pub use event::{EventKind, RuntimeContext, RuntimeEvent};
pub use interpret::interpret;
pub use mock::{MockInteractionResponder, MockMutationAdapter, MutationCall, ResponderCall};
pub use plan::{
    ActionPlan, CreatedResource, ModalPresentation, PlannedAction, PlannedChannel,
    PlannedOverwriteTarget, PlannedRole,
};
pub use policy::{analyze, privileged_mask, DynamicAction, PolicyFinding};
pub use run::{handle_event, run, HandleOutcome};
pub use template::{SanitizeContext, TemplateError, TemplateString};
pub use validate::{validate, ValidationError};
