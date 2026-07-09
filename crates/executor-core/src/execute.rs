use diff_engine::{InMemoryMatchResolver, ResourceResolver};
use discord_model::{GuildId, GuildState};
use operation_graph::{OpId, Operation};
use resource_resolution::{ResolutionError, ResourceResolutionContext};

use crate::adapter::{AdapterError, AdapterErrorKind, ChannelSpec, DiscordAdapter, RoleSpec};
use crate::request::{ApprovedExecutionRequest, ExecutorError};
use crate::result::{
    CreatedResource, JobResult, JobStatus, RollbackAction, StepOutcome, StepResult,
};

pub struct Executor<A: DiscordAdapter> {
    adapter: A,
}

impl<A: DiscordAdapter> Executor<A> {
    pub fn new(adapter: A) -> Self {
        Self { adapter }
    }

    pub fn adapter(&self) -> &A {
        &self.adapter
    }

    pub async fn execute(
        &self,
        request: &ApprovedExecutionRequest,
    ) -> Result<JobResult, ExecutorError> {
        if !request.approval.can_execute() {
            return Err(ExecutorError::NotApproved);
        }
        let order = request
            .operation_graph
            .topological_order()
            .map_err(|_| ExecutorError::GraphCycle)?;

        let resolver = InMemoryMatchResolver::new(&request.snapshot);
        let mut ctx =
            ResourceResolutionContext::new(&request.normalized, &resolver, request.guild_id);

        let mut steps = Vec::new();
        let mut stopped = false;
        for id in order {
            let operation = match request.operation_graph.nodes.iter().find(|n| n.id == id) {
                Some(node) => &node.operation,
                None => continue,
            };
            if stopped {
                steps.push(StepResult {
                    op_id: id,
                    outcome: StepOutcome::Skipped,
                    created: None,
                    rollback: None,
                });
                continue;
            }
            let step = self
                .run_op(id, operation, &mut ctx, &request.snapshot, request.guild_id)
                .await;
            if !matches!(step.outcome, StepOutcome::Success) {
                stopped = true;
            }
            steps.push(step);
        }
        let status = if stopped {
            JobStatus::Failed
        } else {
            JobStatus::Succeeded
        };
        Ok(JobResult { status, steps })
    }

    async fn run_op<R: ResourceResolver>(
        &self,
        op_id: OpId,
        op: &Operation,
        ctx: &mut ResourceResolutionContext<'_, R>,
        snapshot: &GuildState,
        guild: GuildId,
    ) -> StepResult {
        let (outcome, created, rollback) = match op {
            Operation::CreateRole {
                key,
                name,
                permissions,
            } => {
                let spec = RoleSpec {
                    name: name.clone(),
                    permissions: *permissions,
                };
                match self.adapter.create_role(guild, spec).await {
                    Ok(id) => {
                        ctx.bind_role(key.clone(), id);
                        (
                            StepOutcome::Success,
                            Some(CreatedResource::Role {
                                key: key.clone(),
                                id,
                            }),
                            Some(RollbackAction::DeleteRole { id }),
                        )
                    }
                    Err(e) => (fail_outcome(e), None, None),
                }
            }
            Operation::UpdateRole {
                key,
                name,
                permissions,
            } => {
                let id = match ctx.resolve_role_key(key) {
                    Ok(id) => id,
                    Err(e) => return resolution_step(op_id, e),
                };
                let before = snapshot.roles.iter().find(|r| r.id == id).cloned();
                let spec = RoleSpec {
                    name: name.clone(),
                    permissions: *permissions,
                };
                match self.adapter.update_role(guild, id, spec).await {
                    Ok(()) => (
                        StepOutcome::Success,
                        None,
                        before.map(|b| RollbackAction::RestoreRole { id, before: b }),
                    ),
                    Err(e) => (fail_outcome(e), None, None),
                }
            }
            Operation::DeleteRole { key } => {
                let id = match ctx.resolve_role_key(key) {
                    Ok(id) => id,
                    Err(e) => return resolution_step(op_id, e),
                };
                let before = snapshot.roles.iter().find(|r| r.id == id).cloned();
                match self.adapter.delete_role(guild, id).await {
                    Ok(()) => (
                        StepOutcome::Success,
                        None,
                        before.map(|b| RollbackAction::RecreateRole { before: b }),
                    ),
                    Err(e) => (fail_outcome(e), None, None),
                }
            }
            Operation::CreateChannel {
                key,
                name,
                channel_type,
                parent,
            } => {
                let parent_id = match parent {
                    Some(pk) => match ctx.resolve_channel_key(pk) {
                        Ok(id) => Some(id),
                        Err(e) => return resolution_step(op_id, e),
                    },
                    None => None,
                };
                let spec = ChannelSpec {
                    name: name.clone(),
                    channel_type: *channel_type,
                    parent_id,
                };
                match self.adapter.create_channel(guild, spec).await {
                    Ok(id) => {
                        ctx.bind_channel(key.clone(), id);
                        (
                            StepOutcome::Success,
                            Some(CreatedResource::Channel {
                                key: key.clone(),
                                id,
                            }),
                            Some(RollbackAction::DeleteChannel { id }),
                        )
                    }
                    Err(e) => (fail_outcome(e), None, None),
                }
            }
            Operation::UpdateChannel {
                key,
                name,
                channel_type,
            } => {
                let id = match ctx.resolve_channel_key(key) {
                    Ok(id) => id,
                    Err(e) => return resolution_step(op_id, e),
                };
                let before = snapshot.channels.iter().find(|c| c.id == id).cloned();
                let spec = ChannelSpec {
                    name: name.clone(),
                    channel_type: *channel_type,
                    parent_id: None,
                };
                match self.adapter.update_channel(guild, id, spec).await {
                    Ok(()) => (
                        StepOutcome::Success,
                        None,
                        before.map(|b| RollbackAction::RestoreChannel { id, before: b }),
                    ),
                    Err(e) => (fail_outcome(e), None, None),
                }
            }
            Operation::DeleteChannel { key } => {
                let id = match ctx.resolve_channel_key(key) {
                    Ok(id) => id,
                    Err(e) => return resolution_step(op_id, e),
                };
                let before = snapshot.channels.iter().find(|c| c.id == id).cloned();
                match self.adapter.delete_channel(guild, id).await {
                    Ok(()) => (
                        StepOutcome::Success,
                        None,
                        before.map(|b| RollbackAction::RecreateChannel { before: b }),
                    ),
                    Err(e) => (fail_outcome(e), None, None),
                }
            }
            Operation::CreateOverwrite {
                channel,
                target,
                allow,
                deny,
            }
            | Operation::UpdateOverwrite {
                channel,
                target,
                allow,
                deny,
            } => {
                let channel_id = match ctx.resolve_channel_key(channel) {
                    Ok(id) => id,
                    Err(e) => return resolution_step(op_id, e),
                };
                let ow_target = match ctx.resolve_target(target) {
                    Ok(t) => t,
                    Err(e) => return resolution_step(op_id, e),
                };
                let before = snapshot
                    .channels
                    .iter()
                    .find(|c| c.id == channel_id)
                    .and_then(|c| c.overwrites.iter().find(|o| o.target == ow_target).cloned());
                match self
                    .adapter
                    .upsert_overwrite(guild, channel_id, ow_target, *allow, *deny)
                    .await
                {
                    Ok(()) => (
                        StepOutcome::Success,
                        None,
                        Some(RollbackAction::RestoreOverwrite {
                            channel: channel_id,
                            target: ow_target,
                            before,
                        }),
                    ),
                    Err(e) => (fail_outcome(e), None, None),
                }
            }
        };
        StepResult {
            op_id,
            outcome,
            created,
            rollback,
        }
    }
}

fn fail_outcome(e: AdapterError) -> StepOutcome {
    if e.is_retryable() {
        StepOutcome::FailedRetryable(e)
    } else {
        StepOutcome::FailedFatal(e)
    }
}

fn resolution_step(op_id: OpId, e: ResolutionError) -> StepResult {
    StepResult {
        op_id,
        outcome: StepOutcome::FailedFatal(AdapterError::new(
            AdapterErrorKind::Unknown,
            format!("unresolved: {e}"),
        )),
        created: None,
        rollback: None,
    }
}
