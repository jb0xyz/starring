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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RollbackStatus {
    NotRequired,
    Succeeded,
    Partial,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RollbackOutcome {
    Undone,
    Failed(AdapterError),
    Skipped { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollbackStepResult {
    pub source_op_id: OpId,
    pub action: RollbackAction,
    pub outcome: RollbackOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollbackReport {
    pub status: RollbackStatus,
    pub steps: Vec<RollbackStepResult>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobRun {
    pub job: JobResult,
    pub rollback: RollbackReport,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::AdapterErrorKind;

    #[test]
    fn rollback_report_types_are_constructible() {
        let error = AdapterError::new(AdapterErrorKind::Forbidden, "no");
        let action = RollbackAction::DeleteRole { id: RoleId(1) };
        let report = RollbackReport {
            status: RollbackStatus::Failed,
            steps: vec![RollbackStepResult {
                source_op_id: OpId(1),
                action: action.clone(),
                outcome: RollbackOutcome::Failed(error),
            }],
        };
        let run = JobRun {
            job: JobResult {
                status: JobStatus::Failed,
                steps: vec![],
            },
            rollback: report,
        };
        assert_eq!(run.rollback.status, RollbackStatus::Failed);
        assert_eq!(run.rollback.steps[0].action, action);
    }
}
