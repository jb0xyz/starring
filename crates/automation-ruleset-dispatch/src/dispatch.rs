use std::collections::BTreeMap;

use automation_core::{
    interpret, run, ActionPlan, AutomationServices, DiscordMutationAdapter, HandleOutcome,
    InteractionResponder, PlannedAction, ResolvedInstanceContext, RunningRuleSetIdentity,
    RuntimeContext, RuntimeEvent, SanitizeContext, TemplateString,
};
use automation_instance::{
    AutomationInstance, InstanceId, InstanceIdGenerator, InstanceRegistrarV1, InstanceStatus,
    InstanceStore, LegacyInstanceStoreCapabilitiesV1,
};
use automation_instance_teardown::InstanceTeardownService;
use automation_ruleset::{RuleSetKey, RuleSetStore, RuleSetVersionId};
use automation_ruleset_readiness::{
    build_readiness_context, check_readiness, RuleSetReadinessInput,
};
use discord_model::RoleId;
use resource_resolution::ResourceBindingMap;

use crate::error::{DispatchError, DispatchFailure, FailureResponseOutcome};
use crate::resolver::{
    LegacyStoreBackedPinnedInstanceResolverV1, PinnedInstanceResolverErrorV1,
    PinnedInstanceResolverV1, ResolvedPinnedInstanceV1,
};
use crate::snapshot::{GuildRoleSnapshot, GuildRoleSnapshotProvider};

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
    let instances = LegacyInstanceStoreCapabilitiesV1::new(services.instances);
    let resolver = LegacyStoreBackedPinnedInstanceResolverV1::new(&instances, ruleset_store);
    let services = AutomationServices {
        mutation: services.mutation,
        responder: services.responder,
        instances: &instances,
        instance_ids: services.instance_ids,
        teardown: services.teardown,
    };
    dispatch_instance_action_with_route_v1(
        event,
        instance_id,
        action,
        None,
        &resolver,
        snapshot_provider,
        bindings,
        &services,
        failure_message,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn dispatch_instance_action_with_resolver_v1<M, R, S, G, T, PR, P>(
    event: &RuntimeEvent,
    instance_id: &InstanceId,
    action: &str,
    expected_ruleset_key: &str,
    resolver: &PR,
    snapshot_provider: &P,
    bindings: &ResourceBindingMap,
    services: &AutomationServices<'_, M, R, S, G, T>,
    failure_message: &str,
) -> Result<HandleOutcome, DispatchFailure>
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceRegistrarV1,
    G: InstanceIdGenerator,
    T: InstanceTeardownService,
    PR: PinnedInstanceResolverV1,
    P: GuildRoleSnapshotProvider,
{
    dispatch_instance_action_with_route_v1(
        event,
        instance_id,
        action,
        Some(expected_ruleset_key),
        resolver,
        snapshot_provider,
        bindings,
        services,
        failure_message,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_instance_action_with_route_v1<M, R, S, G, T, PR, P>(
    event: &RuntimeEvent,
    instance_id: &InstanceId,
    action: &str,
    expected_ruleset_key: Option<&str>,
    resolver: &PR,
    snapshot_provider: &P,
    bindings: &ResourceBindingMap,
    services: &AutomationServices<'_, M, R, S, G, T>,
    failure_message: &str,
) -> Result<HandleOutcome, DispatchFailure>
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceRegistrarV1,
    G: InstanceIdGenerator,
    T: InstanceTeardownService,
    PR: PinnedInstanceResolverV1,
    P: GuildRoleSnapshotProvider,
{
    if let Err(error) = services.responder.defer_ephemeral().await {
        return Err(DispatchFailure {
            cause: DispatchError::DeferFailed(error),
            failure_response: FailureResponseOutcome::NotAttempted,
        });
    }
    let prepared = match prepare_instance_action_with_route_v1(
        event,
        instance_id,
        action,
        expected_ruleset_key,
        resolver,
        snapshot_provider,
        bindings,
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(cause) => {
            let failure_response = emit_failure(services, failure_message).await;
            return Err(DispatchFailure {
                cause,
                failure_response,
            });
        }
    };
    match execute_prepared_instance_action_v1(prepared, services).await {
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

pub struct PreparedInstanceActionV1 {
    context: RuntimeContext,
    plan: ActionPlan,
    leading_defer_ephemeral: bool,
}

impl PreparedInstanceActionV1 {
    pub fn context(&self) -> &RuntimeContext {
        &self.context
    }

    pub fn plan(&self) -> &ActionPlan {
        &self.plan
    }

    pub fn leading_defer_ephemeral(&self) -> bool {
        self.leading_defer_ephemeral
    }

    pub fn into_parts(self) -> (RuntimeContext, ActionPlan, bool) {
        (self.context, self.plan, self.leading_defer_ephemeral)
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn prepare_instance_action_with_resolver_v1<PR, P>(
    event: &RuntimeEvent,
    instance_id: &InstanceId,
    action: &str,
    expected_ruleset_key: &str,
    resolver: &PR,
    snapshot_provider: &P,
    bindings: &ResourceBindingMap,
) -> Result<PreparedInstanceActionV1, DispatchError>
where
    PR: PinnedInstanceResolverV1,
    P: GuildRoleSnapshotProvider,
{
    prepare_instance_action_with_route_v1(
        event,
        instance_id,
        action,
        Some(expected_ruleset_key),
        resolver,
        snapshot_provider,
        bindings,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn prepare_instance_action_with_resolver_and_snapshot_v1<PR>(
    event: &RuntimeEvent,
    instance_id: &InstanceId,
    action: &str,
    expected_ruleset_key: &str,
    resolver: &PR,
    snapshot: &GuildRoleSnapshot,
    bindings: &ResourceBindingMap,
) -> Result<PreparedInstanceActionV1, DispatchError>
where
    PR: PinnedInstanceResolverV1,
{
    let expected_ruleset_key =
        RuleSetKey::parse(expected_ruleset_key).map_err(|_| DispatchError::PinnedKeyInvalid)?;
    let resolved = resolver
        .resolve_pinned_instance_v1(event.guild_id, instance_id)
        .await
        .map_err(map_resolver_error)?;
    prepare_resolved_instance_action_v1(
        event,
        instance_id,
        action,
        Some(&expected_ruleset_key),
        resolved,
        snapshot,
        bindings,
    )
}

#[allow(clippy::too_many_arguments)]
async fn prepare_instance_action_with_route_v1<PR, P>(
    event: &RuntimeEvent,
    instance_id: &InstanceId,
    action: &str,
    expected_ruleset_key: Option<&str>,
    resolver: &PR,
    snapshot_provider: &P,
    bindings: &ResourceBindingMap,
) -> Result<PreparedInstanceActionV1, DispatchError>
where
    PR: PinnedInstanceResolverV1,
    P: GuildRoleSnapshotProvider,
{
    let expected_ruleset_key = expected_ruleset_key
        .map(RuleSetKey::parse)
        .transpose()
        .map_err(|_| DispatchError::PinnedKeyInvalid)?;
    let resolved = resolver
        .resolve_pinned_instance_v1(event.guild_id, instance_id)
        .await
        .map_err(map_resolver_error)?;
    validate_resolved_instance_action_v1(
        event,
        instance_id,
        expected_ruleset_key.as_ref(),
        &resolved,
    )?;
    let snapshot = snapshot_provider
        .snapshot(event.guild_id)
        .await
        .map_err(DispatchError::SnapshotFailed)?;
    prepare_resolved_instance_action_v1(
        event,
        instance_id,
        action,
        expected_ruleset_key.as_ref(),
        resolved,
        &snapshot,
        bindings,
    )
}

fn prepare_resolved_instance_action_v1(
    event: &RuntimeEvent,
    instance_id: &InstanceId,
    action: &str,
    expected_ruleset_key: Option<&RuleSetKey>,
    resolved: ResolvedPinnedInstanceV1,
    snapshot: &GuildRoleSnapshot,
    bindings: &ResourceBindingMap,
) -> Result<PreparedInstanceActionV1, DispatchError> {
    validate_resolved_instance_action_v1(event, instance_id, expected_ruleset_key, &resolved)?;
    let instance = resolved.instance;
    let identity = RunningRuleSetIdentity {
        key: instance.ruleset_key.clone(),
        version: instance.ruleset_version,
    };
    let artifact = resolved.artifact;

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
    Ok(PreparedInstanceActionV1 {
        context,
        plan: ActionPlan { steps },
        leading_defer_ephemeral: true,
    })
}

fn validate_resolved_instance_action_v1(
    event: &RuntimeEvent,
    instance_id: &InstanceId,
    expected_ruleset_key: Option<&RuleSetKey>,
    resolved: &ResolvedPinnedInstanceV1,
) -> Result<(), DispatchError> {
    ensure_active(&resolved.instance)?;
    let key = RuleSetKey::parse(&resolved.instance.ruleset_key)
        .map_err(|_| DispatchError::PinnedKeyInvalid)?;
    let version = RuleSetVersionId::new(resolved.instance.ruleset_version.get())
        .map_err(|_| DispatchError::PinnedKeyInvalid)?;
    if resolved.instance.guild_id != event.guild_id
        || resolved.instance.id != *instance_id
        || expected_ruleset_key.is_some_and(|expected| expected != &key)
        || resolved.artifact.guild_id != event.guild_id
        || resolved.artifact.ruleset_key != key
        || resolved.artifact.version != version
    {
        return Err(DispatchError::PinnedVersionMissing);
    }
    Ok(())
}

pub async fn execute_prepared_instance_action_v1<M, R, S, G, T>(
    prepared: PreparedInstanceActionV1,
    services: &AutomationServices<'_, M, R, S, G, T>,
) -> Result<HandleOutcome, DispatchError>
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceRegistrarV1,
    G: InstanceIdGenerator,
    T: InstanceTeardownService,
{
    run(prepared.context(), prepared.plan(), services)
        .await
        .map_err(DispatchError::Execution)?;
    Ok(HandleOutcome::Executed)
}

fn map_resolver_error(error: PinnedInstanceResolverErrorV1) -> DispatchError {
    match error {
        PinnedInstanceResolverErrorV1::InstanceLookup(error) => {
            DispatchError::InstanceLookup(error)
        }
        PinnedInstanceResolverErrorV1::InstanceNotFound => DispatchError::InstanceNotFound,
        PinnedInstanceResolverErrorV1::InstanceInactive(status) => {
            DispatchError::InstanceInactive(status)
        }
        PinnedInstanceResolverErrorV1::PinnedKeyInvalid => DispatchError::PinnedKeyInvalid,
        PinnedInstanceResolverErrorV1::VersionLookup(error) => DispatchError::VersionLookup(error),
        PinnedInstanceResolverErrorV1::PinnedVersionMissing => DispatchError::PinnedVersionMissing,
    }
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
    S: InstanceRegistrarV1,
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
