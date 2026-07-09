pub mod adapter;
pub mod execute;
pub mod mock;
pub mod request;
pub mod result;

pub use adapter::{AdapterError, AdapterErrorKind, ChannelSpec, DiscordAdapter, RoleSpec};
pub use execute::Executor;
pub use mock::{AdapterCall, MockDiscordAdapter};
pub use request::{ApprovedExecutionRequest, ExecutorError};
pub use result::{CreatedResource, JobResult, JobStatus, RollbackAction, StepOutcome, StepResult};
