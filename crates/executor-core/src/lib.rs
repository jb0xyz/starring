pub mod adapter;
pub mod execute;
pub mod mock;
pub mod reader;
pub mod request;
pub mod result;

pub use adapter::{AdapterError, AdapterErrorKind, ChannelSpec, DiscordAdapter, RoleSpec};
pub use execute::Executor;
pub use mock::{AdapterCall, MockDiscordAdapter};
pub use reader::GuildStateReader;
pub use request::{ApprovedExecutionRequest, ExecutorError};
pub use result::{
    CreatedResource, JobResult, JobRun, JobStatus, RollbackAction, RollbackOutcome, RollbackReport,
    RollbackStatus, RollbackStepResult, StepOutcome, StepResult,
};
