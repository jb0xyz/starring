mod effect;
mod prepare;
mod state;

use automation_instance::{InstanceIdGenerator, InstanceRegistrarV1};
use automation_instance_teardown::InstanceTeardownService;

use crate::adapter::{
    AdapterError, AutomationServices, DiscordMutationAdapter, InteractionResponder,
};
use crate::event::RuntimeContext;
use crate::plan::{ActionPlan, RunResult};
use crate::prepare::PreparedEventExecution;

use self::effect::{execute_action, ExecutionOutput};
use self::prepare::{prepare_action, prepare_failure_message};
use self::state::ExecutionState;

pub(crate) async fn execute_plan<M, R, S, G, T>(
    context: &RuntimeContext,
    plan: &ActionPlan,
    services: &AutomationServices<'_, M, R, S, G, T>,
) -> Result<RunResult, AdapterError>
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceRegistrarV1,
    G: InstanceIdGenerator,
    T: InstanceTeardownService,
{
    let mut state = ExecutionState::prepare(plan, services.instance_ids)?;
    let mut output = ExecutionOutput::default();
    for (action_index, step) in plan.steps.iter().enumerate() {
        let action = prepare_action(action_index, step, context, &state)?;
        execute_action(context.guild_id, action, services, &mut state, &mut output).await?;
    }
    Ok(output.finish())
}

pub async fn execute_prepared_event<M, R, S, G, T>(
    prepared: PreparedEventExecution,
    services: &AutomationServices<'_, M, R, S, G, T>,
    failure_message: &str,
) -> Result<(), AdapterError>
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceRegistrarV1,
    G: InstanceIdGenerator,
    T: InstanceTeardownService,
{
    if prepared.leading_defer_ephemeral() {
        services.responder.defer_ephemeral().await?;
    }
    match execute_plan(prepared.context(), prepared.plan(), services).await {
        Ok(_) => Ok(()),
        Err(error) => {
            if prepared.leading_defer_ephemeral() {
                if let Ok(rendered) = prepare_failure_message(failure_message, prepared.context()) {
                    let _ = services.responder.edit_response(rendered).await;
                }
            }
            Err(error)
        }
    }
}
