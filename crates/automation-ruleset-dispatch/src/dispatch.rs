use std::collections::BTreeMap;

use automation_core::{
    interpret, run, ActionPlan, AutomationServices, DiscordMutationAdapter, HandleOutcome,
    InteractionResponder, PlannedAction, ResolvedInstanceContext, RunningRuleSetIdentity,
    RuntimeContext, RuntimeEvent, SanitizeContext, TemplateString,
};
use automation_instance::{
    AutomationInstance, InstanceId, InstanceIdGenerator, InstanceStatus, InstanceStore,
};
use automation_instance_teardown::InstanceTeardownService;
use automation_ruleset::{RuleSetKey, RuleSetStore, RuleSetVersionId};
use automation_ruleset_readiness::{
    build_readiness_context, check_readiness, RuleSetReadinessInput,
};
use discord_model::RoleId;
use resource_resolution::ResourceBindingMap;

use crate::error::{DispatchError, DispatchFailure, FailureResponseOutcome};
use crate::snapshot::GuildRoleSnapshotProvider;

#[allow(clippy::too_many_arguments)]
pub async fn dispatch_instance_action<M, R, S, G, T, RS, P>(
    event: &RuntimeEvent,
    instance_id: &InstanceId,
    action: &str,
    ruleset_store: &RS,
    snapshot_provider: &P,
    bindings: &ResourceBindingMap,
    services: &AutomationServices<'_, M, R, S, G, T>,
    failure_message: &str,
) -> Result<HandleOutcome, DispatchFailure>
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceStore,
    G: InstanceIdGenerator,
    T: InstanceTeardownService,
    RS: RuleSetStore,
    P: GuildRoleSnapshotProvider,
{
    if let Err(error) = services.responder.defer_ephemeral().await {
        return Err(DispatchFailure {
            cause: DispatchError::DeferFailed(error),
            failure_response: FailureResponseOutcome::NotAttempted,
        });
    }
    match run_pinned(
        event,
        instance_id,
        action,
        ruleset_store,
        snapshot_provider,
        bindings,
        services,
    )
    .await
    {
        Ok(outcome) => Ok(outcome),
        Err(cause) => {
            let failure_response = emit_failure(services, failure_message).await;
            Err(DispatchFailure {
                cause,
                failure_response,
            })
        }
    }
}

async fn run_pinned<M, R, S, G, T, RS, P>(
    event: &RuntimeEvent,
    instance_id: &InstanceId,
    action: &str,
    ruleset_store: &RS,
    snapshot_provider: &P,
    bindings: &ResourceBindingMap,
    services: &AutomationServices<'_, M, R, S, G, T>,
) -> Result<HandleOutcome, DispatchError>
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceStore,
    G: InstanceIdGenerator,
    T: InstanceTeardownService,
    RS: RuleSetStore,
    P: GuildRoleSnapshotProvider,
{
    let instance = services
        .instances
        .get(event.guild_id, instance_id)
        .await
        .map_err(DispatchError::InstanceLookup)?
        .ok_or(DispatchError::InstanceNotFound)?;
    ensure_active(&instance)?;

    let key =
        RuleSetKey::parse(&instance.ruleset_key).map_err(|_| DispatchError::PinnedKeyInvalid)?;
    let version_id = RuleSetVersionId::new(instance.ruleset_version.get())
        .map_err(|_| DispatchError::PinnedKeyInvalid)?;
    let identity = RunningRuleSetIdentity {
        key: instance.ruleset_key.clone(),
        version: instance.ruleset_version,
    };

    let artifact = ruleset_store
        .get_version(event.guild_id, &key, version_id)
        .await
        .map_err(DispatchError::VersionLookup)?
        .ok_or(DispatchError::PinnedVersionMissing)?;
    if artifact.guild_id != event.guild_id
        || artifact.ruleset_key != key
        || artifact.version != version_id
    {
        return Err(DispatchError::PinnedVersionMissing);
    }

    let snapshot = snapshot_provider
        .snapshot(event.guild_id)
        .await
        .map_err(DispatchError::SnapshotFailed)?;
    let bot_roles: Vec<RoleId> = snapshot.bot_role_ids.iter().copied().collect();
    let (guild_capabilities, role_permissions) =
        build_readiness_context(event.guild_id, bindings, &snapshot.roles, &bot_roles)
            .map_err(DispatchError::ContextInvalid)?;

    let runtime_ruleset = check_readiness(RuleSetReadinessInput {
        artifact: &artifact,
        bindings,
        guild_capabilities: &guild_capabilities,
        role_permissions: &role_permissions,
    })
    .map_err(DispatchError::NotReady)?;

    let plan = interpret(event, &runtime_ruleset.definition, bindings).ok_or_else(|| {
        DispatchError::NoMatchingRule {
            action: action.to_string(),
        }
    })?;
    let mut steps = plan.steps;
    if matches!(steps.first(), Some(PlannedAction::DeferEphemeral)) {
        steps.remove(0);
    }

    let mut context = RuntimeContext::from_event(event, &identity);
    context.instance = Some(ResolvedInstanceContext {
        instance,
        action: action.to_string(),
    });
    run(&context, &ActionPlan { steps }, services)
        .await
        .map_err(DispatchError::Execution)?;
    Ok(HandleOutcome::Executed)
}

fn ensure_active(instance: &AutomationInstance) -> Result<(), DispatchError> {
    if instance.status != InstanceStatus::Active {
        return Err(DispatchError::InstanceInactive(instance.status));
    }
    Ok(())
}

async fn emit_failure<M, R, S, G, T>(
    services: &AutomationServices<'_, M, R, S, G, T>,
    failure_message: &str,
) -> FailureResponseOutcome
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceStore,
    G: InstanceIdGenerator,
    T: InstanceTeardownService,
{
    let inputs: BTreeMap<String, String> = BTreeMap::new();
    let rendered = match TemplateString::parse(failure_message)
        .and_then(|template| template.render(&inputs, SanitizeContext::EphemeralMessageContent))
    {
        Ok(text) => text,
        Err(_) => return FailureResponseOutcome::NotAttempted,
    };
    match services.responder.edit_response(rendered).await {
        Ok(()) => FailureResponseOutcome::Sent,
        Err(error) => FailureResponseOutcome::Failed(error),
    }
}
