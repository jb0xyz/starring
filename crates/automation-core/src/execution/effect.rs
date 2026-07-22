use automation_instance::{InstanceIdGenerator, InstanceMessageRef, InstanceRegistrarV1};
use automation_instance_teardown::InstanceTeardownService;
use discord_model::GuildId;

use crate::adapter::{
    AdapterError, AdapterErrorKind, AutomationServices, CreateChannelSpec, CreateRoleSpec,
    DiscordMutationAdapter, InteractionResponder,
};
use crate::plan::{CreatedResource, ResponseDeliveryOutcome, RunResult, TeardownActionResult};

use super::prepare::PreparedAction;
use super::state::ExecutionState;

#[derive(Default)]
pub(super) struct ExecutionOutput {
    created: Vec<CreatedResource>,
    teardowns: Vec<TeardownActionResult>,
}

impl ExecutionOutput {
    pub(super) fn finish(self) -> RunResult {
        RunResult {
            created: self.created,
            teardowns: self.teardowns,
        }
    }
}

pub(super) async fn execute_action<M, R, S, G, T>(
    guild_id: GuildId,
    action: PreparedAction,
    services: &AutomationServices<'_, M, R, S, G, T>,
    state: &mut ExecutionState,
    output: &mut ExecutionOutput,
) -> Result<(), AdapterError>
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceRegistrarV1,
    G: InstanceIdGenerator,
    T: InstanceTeardownService,
{
    match action {
        PreparedAction::GrantRole { role, target } => {
            services.mutation.grant_role(guild_id, target, role).await?;
        }
        PreparedAction::RespondEphemeral { content } => {
            services.responder.respond_ephemeral(content).await?;
        }
        PreparedAction::OpenModal(modal) => {
            services.responder.open_modal(&modal).await?;
        }
        PreparedAction::CreateChannel {
            action_index,
            key,
            name,
        } => {
            let id = services
                .mutation
                .create_channel(guild_id, CreateChannelSpec { name: name.clone() })
                .await?;
            state.created_channels.insert(key.clone(), id);
            output.created.push(CreatedResource::Channel {
                action_index,
                key,
                name,
                id,
            });
        }
        PreparedAction::CreateRole {
            action_index,
            key,
            name,
        } => {
            let id = services
                .mutation
                .create_role(guild_id, CreateRoleSpec { name: name.clone() })
                .await?;
            state.created_roles.insert(key.clone(), id);
            output.created.push(CreatedResource::Role {
                action_index,
                key,
                name,
                id,
            });
        }
        PreparedAction::UpsertOverwrite {
            channel,
            target,
            allow,
            deny,
        } => {
            services
                .mutation
                .upsert_overwrite(guild_id, channel, target, allow, deny)
                .await?;
        }
        PreparedAction::PostPanel {
            action_index,
            key,
            channel,
            spec,
        } => {
            let id = services
                .mutation
                .post_panel(guild_id, channel, spec)
                .await?;
            state
                .created_messages
                .insert(key.clone(), InstanceMessageRef { channel, id });
            output.created.push(CreatedResource::Message {
                action_index,
                key,
                channel,
                id,
            });
        }
        PreparedAction::DeferEphemeral => {
            services.responder.defer_ephemeral().await?;
        }
        PreparedAction::EditResponse { content } => {
            if let Err(error) = services.responder.edit_response(content).await {
                if output.teardowns.is_empty() {
                    return Err(error);
                }
                for teardown in &mut output.teardowns {
                    teardown.response = ResponseDeliveryOutcome::Failed;
                }
            }
        }
        PreparedAction::RegisterInstance {
            action_index,
            key,
            id,
            instance,
        } => {
            services
                .instances
                .register_instance_v1(instance)
                .await
                .map_err(|error| {
                    AdapterError::new(
                        AdapterErrorKind::BadRequest,
                        format!("instance register error: {error:?}"),
                    )
                })?;
            state.created_instances.insert(key.clone(), id.clone());
            output.created.push(CreatedResource::Instance {
                action_index,
                key,
                id,
            });
        }
        PreparedAction::TeardownInstance {
            action_index,
            instance_id,
        } => {
            let outcome = services
                .teardown
                .teardown(guild_id, instance_id.clone())
                .await
                .map_err(teardown_error)?;
            output.teardowns.push(TeardownActionResult {
                action_index,
                instance_id,
                teardown: outcome,
                response: ResponseDeliveryOutcome::Sent,
            });
        }
    }
    Ok(())
}

fn teardown_error(error: automation_instance_teardown::TeardownError) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        format!("instance teardown error: {error:?}"),
    )
}
