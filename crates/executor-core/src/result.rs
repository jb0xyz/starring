use serde::{Deserialize, Serialize};

use desired_state::ResourceKey;
use discord_model::{Channel, ChannelId, OverwriteTarget, PermissionOverwrite, Role, RoleId};
use operation_graph::OpId;

use crate::adapter::AdapterError;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepOutcome {
    Success,
    FailedRetryable(AdapterError),
    FailedFatal(AdapterError),
    Skipped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CreatedResource {
    Role { key: ResourceKey, id: RoleId },
    Channel { key: ResourceKey, id: ChannelId },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RollbackAction {
    DeleteRole {
        id: RoleId,
    },
    RestoreRole {
        id: RoleId,
        before: Role,
    },
    RecreateRole {
        before: Role,
    },
    DeleteChannel {
        id: ChannelId,
    },
    RestoreChannel {
        id: ChannelId,
        before: Channel,
    },
    RecreateChannel {
        before: Channel,
    },
    RestoreOverwrite {
        channel: ChannelId,
        target: OverwriteTarget,
        before: Option<PermissionOverwrite>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepResult {
    pub op_id: OpId,
    pub outcome: StepOutcome,
    pub created: Option<CreatedResource>,
    pub rollback: Option<RollbackAction>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobResult {
    pub status: JobStatus,
    pub steps: Vec<StepResult>,
}
