use serde::{Deserialize, Serialize};
use thiserror::Error;

use approval_manager::ApprovalRequest;
use desired_compiler::NormalizedDesiredState;
use discord_model::{GuildId, GuildState, UserId};
use operation_graph::OperationGraph;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovedExecutionRequest {
    pub operation_graph: OperationGraph,
    pub normalized: NormalizedDesiredState,
    pub approval: ApprovalRequest,
    pub snapshot: GuildState,
    pub guild_id: GuildId,
    pub requested_by: UserId,
    pub approved_by: Vec<UserId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum ExecutorError {
    #[error("request is not approved for execution")]
    NotApproved,
    #[error("operation graph has a cycle")]
    GraphCycle,
}
