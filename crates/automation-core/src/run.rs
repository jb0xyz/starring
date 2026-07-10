use automation_state::InteractionRuleSet;
use resource_resolution::ResourceBindingMap;

use crate::adapter::{
    AdapterError, AdapterErrorKind, CreateChannelSpec, CreateRoleSpec, DiscordMutationAdapter,
    InteractionResponder,
};
use crate::event::{RuntimeContext, RuntimeEvent};
use crate::interpret::interpret;
use crate::plan::{ActionPlan, CreatedResource, PlannedAction};
use crate::template::{SanitizeContext, TemplateError, TemplateString};

pub async fn run(
    context: &RuntimeContext,
    plan: &ActionPlan,
    mutation: &impl DiscordMutationAdapter,
    responder: &impl InteractionResponder,
) -> Result<Vec<CreatedResource>, AdapterError> {
    let mut created = Vec::new();
    for (action_index, step) in plan.steps.iter().enumerate() {
        match step {
            PlannedAction::GrantRole { role, target } => {
                mutation
                    .grant_role(context.guild_id, *target, *role)
                    .await?;
            }
            PlannedAction::RespondEphemeral { content } => {
                let rendered = render(content, context, SanitizeContext::EphemeralMessageContent)?;
                responder.respond_ephemeral(rendered).await?;
            }
            PlannedAction::OpenModal(modal) => {
                responder.open_modal(modal).await?;
            }
            PlannedAction::CreateChannel { name } => {
                let rendered = render(name, context, SanitizeContext::ChannelName)?;
                let id = mutation
                    .create_channel(
                        context.guild_id,
                        CreateChannelSpec {
                            name: rendered.clone(),
                        },
                    )
                    .await?;
                created.push(CreatedResource::Channel {
                    action_index,
                    name: rendered,
                    id,
                });
            }
            PlannedAction::CreateRole { name } => {
                let rendered = render(name, context, SanitizeContext::RoleName)?;
                let id = mutation
                    .create_role(
                        context.guild_id,
                        CreateRoleSpec {
                            name: rendered.clone(),
                        },
                    )
                    .await?;
                created.push(CreatedResource::Role {
                    action_index,
                    name: rendered,
                    id,
                });
            }
        }
    }
    Ok(created)
}

fn render(
    source: &str,
    context: &RuntimeContext,
    sanitize: SanitizeContext,
) -> Result<String, AdapterError> {
    TemplateString::parse(source)
        .and_then(|template| template.render(&context.inputs, sanitize))
        .map_err(template_error)
}

fn template_error(error: TemplateError) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        format!("template error: {error:?}"),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleOutcome {
    Executed,
    NoOp,
}

pub async fn handle_event(
    event: &RuntimeEvent,
    ruleset: &InteractionRuleSet,
    bindings: &ResourceBindingMap,
    mutation: &impl DiscordMutationAdapter,
    responder: &impl InteractionResponder,
) -> Result<HandleOutcome, AdapterError> {
    match interpret(event, ruleset, bindings) {
        Some(plan) => {
            let context = RuntimeContext::from_event(event);
            run(&context, &plan, mutation, responder).await?;
            Ok(HandleOutcome::Executed)
        }
        None => Ok(HandleOutcome::NoOp),
    }
}
