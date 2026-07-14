use automation_instance::{InstanceIdGenerator, InstanceStore};
use automation_instance_teardown::InstanceTeardownService;
use automation_state::InteractionRuleSet;
use resource_resolution::ResourceBindingMap;

use crate::adapter::{
    AdapterError, AutomationServices, DiscordMutationAdapter, InteractionResponder,
};
use crate::event::{RunningRuleSetIdentity, RuntimeContext, RuntimeEvent};
use crate::execution::{execute_plan, execute_prepared_event};
use crate::plan::{ActionPlan, RunResult};
use crate::prepare::prepare_event_execution;

pub async fn run<M, R, S, G, T>(
    context: &RuntimeContext,
    plan: &ActionPlan,
    services: &AutomationServices<'_, M, R, S, G, T>,
) -> Result<RunResult, AdapterError>
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceStore,
    G: InstanceIdGenerator,
    T: InstanceTeardownService,
{
    execute_plan(context, plan, services).await
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleOutcome {
    Executed,
    NoOp,
}

pub async fn handle_event<M, R, S, G, T>(
    event: &RuntimeEvent,
    ruleset: &InteractionRuleSet,
    bindings: &ResourceBindingMap,
    services: &AutomationServices<'_, M, R, S, G, T>,
    failure_message: &str,
    identity: &RunningRuleSetIdentity,
) -> Result<HandleOutcome, AdapterError>
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceStore,
    G: InstanceIdGenerator,
    T: InstanceTeardownService,
{
    let Some(prepared) = prepare_event_execution(event, ruleset, bindings, identity)? else {
        return Ok(HandleOutcome::NoOp);
    };
    execute_prepared_event(prepared, services, failure_message).await?;
    Ok(HandleOutcome::Executed)
}
