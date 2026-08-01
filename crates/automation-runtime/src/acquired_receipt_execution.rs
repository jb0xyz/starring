use std::fmt::{Debug, Formatter};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Duration;

use automation_core::preflight::{
    preflight_action_plan_v1, prepare_action_plan_v1, ActionPlanSnapshotIdentityV1,
    ActionPlanSnapshotRequestV1, ActionPlanSnapshotV1, PreflightedActionPlanV1,
};
#[cfg(test)]
use automation_core::DiscordMutationAdapter;
use automation_core::{
    prepare_event_execution, ActionPlan, AdapterError, AutomationServices, EventKind,
    InteractionResponder, RunningRuleSetIdentity, RuntimeContext, RuntimeEvent,
};
use automation_instance::{InstanceIdGenerator, InstanceRegistrarV1};
use automation_instance_teardown::DurableInstanceTeardownServiceV1;
use automation_ruleset_dispatch::{
    prepare_instance_action_with_resolver_and_snapshot_v1, GuildRoleSnapshot,
    GuildRoleSnapshotProvider, PinnedInstanceResolverV1,
};
use automation_runtime_interaction::{
    InteractionActionPlanDigestV1, InteractionExecutionRouteV1, InteractionPreflightPlanDigestV1,
    InteractionPreflightSnapshotDigestV1, InteractionReceiptClaimRootV1,
    InteractionReceiptIdentityV1, InteractionReceiptStateV1, InteractionRequestDigestV1,
    InteractionRouteBindingV1,
};
use sha2::{Digest, Sha256};
use tokio::time::{sleep_until, Instant};

use crate::action_plan_digest::build_interaction_action_plan_digest_v1;
use crate::action_plan_preflight_certificate::InteractionActionPreflightCertificateV1;
use crate::action_plan_wire_preflight::{preflight_action_plan_wire_v1, ActionPlanWirePreflightV1};
use crate::convert::interaction_to_event;
use crate::discord_effects::RecoverableDiscordMutationAdapterV1;
use crate::effect_journal::{
    InteractionEffectIntentDispositionV1, InteractionEffectJournalIntendV1,
    InteractionEffectJournalPlanEntryV1, InteractionEffectJournalPlanV1,
    InteractionEffectJournalPortV1, InteractionEffectPlanBindDispositionV1,
};
use crate::interaction_effect_plan::build_interaction_effect_execution_plan_v1;
use crate::journaled_action_executor::{
    execute_journaled_action_plan_v1, ExactInteractionTeardownSetV1,
    JournaledActionExecutionOutcomeV1, JournaledActionExecutionServicesV1,
    JournaledActionExecutionStageV1, JournaledActionExecutionStopReasonV1,
};
use crate::receipt_fenced_effects::{
    InteractionEffectPermitV1, InteractionInitialResponseIntentDispositionV1,
    InteractionInitialResponseIntentV1, InteractionInitialResponseKindV1,
    InteractionInitialResponseResultKindV1, InteractionInitialResponseResultV1,
    ReceiptFencedInteractionResponderV1,
};
use crate::shared_gateway_admission::SharedGatewayAdmittedInteractionV3;
use crate::shared_gateway_dispatcher::SharedGatewayInteractionEnvelopeV3;
use crate::shared_gateway_executor::execution_inputs;

const TERMINAL_DIGEST_DOMAIN_V1: &[u8] = b"starring.runtime.acquired_receipt.terminal.v1\0";
const TERMINAL_DIGEST_VERSION_V1: u16 = 1;
const COMBINED_PREFLIGHT_DIGEST_DOMAIN_V1: &[u8] =
    b"starring.runtime.interaction.combined_preflight_plan.v1\0";
const MAX_COMBINED_PREFLIGHT_DIGEST_MATERIAL_BYTES_V1: usize = 2_098_176;
const ACQUIRED_INTERACTION_CLAIM_LEASE_SECONDS_V1: u64 = 5 * 60;
const ACQUIRED_INTERACTION_EXECUTION_DEADLINE_SECONDS_V1: u64 = 4 * 60;
const ACQUIRED_INTERACTION_RECOVERY_MARGIN_SECONDS_V1: u64 = 60;
pub const ACQUIRED_INTERACTION_CLAIM_LEASE_V1: Duration =
    Duration::from_secs(ACQUIRED_INTERACTION_CLAIM_LEASE_SECONDS_V1);
pub const ACQUIRED_INTERACTION_EXECUTION_DEADLINE_V1: Duration =
    Duration::from_secs(ACQUIRED_INTERACTION_EXECUTION_DEADLINE_SECONDS_V1);
const _: () = assert!(
    ACQUIRED_INTERACTION_EXECUTION_DEADLINE_SECONDS_V1
        + ACQUIRED_INTERACTION_RECOVERY_MARGIN_SECONDS_V1
        <= ACQUIRED_INTERACTION_CLAIM_LEASE_SECONDS_V1
);

pub struct AuthoritativeInteractionClaimV1<'a> {
    claim_root: &'a InteractionReceiptClaimRootV1,
}

impl<'a> AuthoritativeInteractionClaimV1<'a> {
    pub const fn new(claim_root: &'a InteractionReceiptClaimRootV1) -> Self {
        Self { claim_root }
    }

    pub fn identity(&self) -> InteractionReceiptIdentityV1 {
        self.claim_root.identity()
    }

    pub fn route(&self) -> &InteractionRouteBindingV1 {
        self.claim_root.route()
    }

    pub fn request_digest(&self) -> &InteractionRequestDigestV1 {
        self.claim_root.request_digest()
    }
}

impl Debug for AuthoritativeInteractionClaimV1<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AuthoritativeInteractionClaimV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcquiredInteractionTerminalOutcomeV1 {
    StaticCompleted,
    InstanceCompleted,
    ProvisioningCompletedResponseUnconfirmed,
    EventConversionFailed,
    NoMatchingRule,
    StaticPreparationFailed,
    InstancePreparationFailed,
    ExecutionFailedBeforeEffect,
    ExecutionKnownFailed,
    ExecutionRecoveryRequired,
}

impl AcquiredInteractionTerminalOutcomeV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::StaticCompleted => "interaction_static_completed",
            Self::InstanceCompleted => "interaction_instance_completed",
            Self::ProvisioningCompletedResponseUnconfirmed => {
                "interaction_provisioning_completed_response_unconfirmed"
            }
            Self::EventConversionFailed => "interaction_event_conversion_failed",
            Self::NoMatchingRule => "interaction_no_matching_rule",
            Self::StaticPreparationFailed => "interaction_static_preparation_failed",
            Self::InstancePreparationFailed => "interaction_instance_preparation_failed",
            Self::ExecutionFailedBeforeEffect => "interaction_execution_failed_before_effect",
            Self::ExecutionKnownFailed => "interaction_execution_known_failed",
            Self::ExecutionRecoveryRequired => "interaction_execution_recovery_required",
        }
    }

    pub const fn state(self) -> InteractionReceiptStateV1 {
        match self {
            Self::StaticCompleted
            | Self::InstanceCompleted
            | Self::ProvisioningCompletedResponseUnconfirmed => {
                InteractionReceiptStateV1::Completed
            }
            Self::EventConversionFailed
            | Self::NoMatchingRule
            | Self::StaticPreparationFailed
            | Self::InstancePreparationFailed
            | Self::ExecutionFailedBeforeEffect
            | Self::ExecutionKnownFailed => InteractionReceiptStateV1::Failed,
            Self::ExecutionRecoveryRequired => InteractionReceiptStateV1::RecoveryRequired,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct InteractionTerminalDigestV1([u8; 32]);

impl InteractionTerminalDigestV1 {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Debug for InteractionTerminalDigestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("InteractionTerminalDigestV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct InteractionTerminalFinishV1 {
    state: InteractionReceiptStateV1,
    outcome: AcquiredInteractionTerminalOutcomeV1,
    action_plan_digest: Option<InteractionActionPlanDigestV1>,
    terminal_digest: InteractionTerminalDigestV1,
}

impl InteractionTerminalFinishV1 {
    pub const fn state(&self) -> InteractionReceiptStateV1 {
        self.state
    }

    pub const fn outcome(&self) -> AcquiredInteractionTerminalOutcomeV1 {
        self.outcome
    }

    pub fn outcome_code(&self) -> &'static str {
        self.outcome.code()
    }

    pub fn action_plan_digest(&self) -> Option<&InteractionActionPlanDigestV1> {
        self.action_plan_digest.as_ref()
    }

    pub fn terminal_digest(&self) -> &InteractionTerminalDigestV1 {
        &self.terminal_digest
    }
}

impl Debug for InteractionTerminalFinishV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("InteractionTerminalFinishV1(<redacted>)")
    }
}

#[allow(async_fn_in_trait)]
pub trait AcquiredInteractionLifecyclePermitV1: InteractionEffectPermitV1 {
    fn authoritative_claim_v1(&self) -> AuthoritativeInteractionClaimV1<'_>;

    async fn bind_action_plan_digest_v1(
        &self,
        digest: &InteractionActionPlanDigestV1,
    ) -> Result<(), Self::Error>;

    async fn finish_interaction_v1(
        &self,
        finish: &InteractionTerminalFinishV1,
    ) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcquiredInteractionPersistenceStageV1 {
    InitialResponseIntent,
    InitialResponseResult,
    ActionPlanBind,
    ExecutionIntent,
    EffectPlanBind,
    EffectIntent,
    EffectResult,
    TerminalFinish,
}

#[derive(Clone, PartialEq, Eq)]
pub enum AcquiredInteractionExecutionOutcomeV1 {
    Terminalized(InteractionTerminalFinishV1),
    AcknowledgementTerminalized {
        state: InteractionReceiptStateV1,
        result: InteractionInitialResponseResultKindV1,
    },
    AuthorityRejected,
    ExecutionDeadlineElapsed,
    EffectRecoveryPending,
    PersistenceFailed {
        stage: AcquiredInteractionPersistenceStageV1,
        external_effect_may_have_occurred: bool,
    },
}

impl Debug for AcquiredInteractionExecutionOutcomeV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AcquiredInteractionExecutionOutcomeV1(<redacted>)")
    }
}

pub struct AcquiredInteractionExecutionServicesV1<'a, M: ?Sized, R: ?Sized, S, G, T, PR, SP> {
    mutation: &'a M,
    responder: &'a R,
    instances: &'a S,
    instance_ids: &'a G,
    teardown: &'a T,
    pinned_resolver: &'a PR,
    snapshot_provider: &'a SP,
}

impl<'a, M: ?Sized, R: ?Sized, S, G, T, PR, SP>
    AcquiredInteractionExecutionServicesV1<'a, M, R, S, G, T, PR, SP>
{
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        mutation: &'a M,
        responder: &'a R,
        instances: &'a S,
        instance_ids: &'a G,
        teardown: &'a T,
        pinned_resolver: &'a PR,
        snapshot_provider: &'a SP,
    ) -> Self {
        Self {
            mutation,
            responder,
            instances,
            instance_ids,
            teardown,
            pinned_resolver,
            snapshot_provider,
        }
    }
}

impl<M: ?Sized, R: ?Sized, S, G, T, PR, SP> Debug
    for AcquiredInteractionExecutionServicesV1<'_, M, R, S, G, T, PR, SP>
{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AcquiredInteractionExecutionServicesV1(<redacted>)")
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn execute_acquired_interaction_v1<P, M, R, S, G, T, PR, SP>(
    admitted: SharedGatewayAdmittedInteractionV3,
    envelope: SharedGatewayInteractionEnvelopeV3,
    permit: P,
    services: AcquiredInteractionExecutionServicesV1<'_, M, R, S, G, T, PR, SP>,
) -> AcquiredInteractionExecutionOutcomeV1
where
    P: AcquiredInteractionLifecyclePermitV1
        + InteractionEffectJournalPortV1<Error = <P as InteractionEffectPermitV1>::Error>,
    M: RecoverableDiscordMutationAdapterV1,
    R: InteractionResponder + ?Sized,
    S: InstanceRegistrarV1,
    G: InstanceIdGenerator,
    T: DurableInstanceTeardownServiceV1,
    PR: PinnedInstanceResolverV1,
    SP: GuildRoleSnapshotProvider,
{
    execute_acquired_interaction_until_v1(
        admitted,
        envelope,
        permit,
        services,
        Instant::now() + ACQUIRED_INTERACTION_EXECUTION_DEADLINE_V1,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn execute_acquired_interaction_until_v1<P, M, R, S, G, T, PR, SP>(
    admitted: SharedGatewayAdmittedInteractionV3,
    envelope: SharedGatewayInteractionEnvelopeV3,
    permit: P,
    services: AcquiredInteractionExecutionServicesV1<'_, M, R, S, G, T, PR, SP>,
    execution_deadline: Instant,
) -> AcquiredInteractionExecutionOutcomeV1
where
    P: AcquiredInteractionLifecyclePermitV1
        + InteractionEffectJournalPortV1<Error = <P as InteractionEffectPermitV1>::Error>,
    M: RecoverableDiscordMutationAdapterV1,
    R: InteractionResponder + ?Sized,
    S: InstanceRegistrarV1,
    G: InstanceIdGenerator,
    T: DurableInstanceTeardownServiceV1,
    PR: PinnedInstanceResolverV1,
    SP: GuildRoleSnapshotProvider,
{
    if Instant::now() >= execution_deadline {
        drop(envelope);
        drop(permit);
        drop(admitted);
        return AcquiredInteractionExecutionOutcomeV1::ExecutionDeadlineElapsed;
    }
    let outcome = {
        let execution =
            execute_acquired_interaction_inner_v1(&admitted, &envelope, &permit, &services);
        tokio::pin!(execution);
        tokio::select! {
            biased;
            _ = sleep_until(execution_deadline) => {
                AcquiredInteractionExecutionOutcomeV1::ExecutionDeadlineElapsed
            }
            outcome = &mut execution => outcome,
        }
    };
    drop(envelope);
    drop(permit);
    drop(admitted);
    outcome
}

async fn execute_acquired_interaction_inner_v1<P, M, R, S, G, T, PR, SP>(
    admitted: &SharedGatewayAdmittedInteractionV3,
    envelope: &SharedGatewayInteractionEnvelopeV3,
    permit: &P,
    services: &AcquiredInteractionExecutionServicesV1<'_, M, R, S, G, T, PR, SP>,
) -> AcquiredInteractionExecutionOutcomeV1
where
    P: AcquiredInteractionLifecyclePermitV1
        + InteractionEffectJournalPortV1<Error = <P as InteractionEffectPermitV1>::Error>,
    M: RecoverableDiscordMutationAdapterV1,
    R: InteractionResponder + ?Sized,
    S: InstanceRegistrarV1,
    G: InstanceIdGenerator,
    T: DurableInstanceTeardownServiceV1,
    PR: PinnedInstanceResolverV1,
    SP: GuildRoleSnapshotProvider,
{
    if !authoritative_claim_matches_v1(admitted, envelope, permit) {
        return AcquiredInteractionExecutionOutcomeV1::AuthorityRejected;
    }
    let (identity, artifact, bindings) = execution_inputs(admitted.route());
    let interaction = envelope.twilight_interaction_v3();
    let event = interaction_to_event(interaction.as_interaction_v3(), &identity.key);
    drop(interaction);
    let Some(event) = event else {
        return finish_terminal_v1(
            permit,
            None,
            AcquiredInteractionTerminalOutcomeV1::EventConversionFailed,
            false,
        )
        .await;
    };
    if !authoritative_execution_route_matches_v1(&event, permit) {
        return AcquiredInteractionExecutionOutcomeV1::AuthorityRejected;
    }
    let tracking = TrackingInteractionEffectPermitV1::new(permit);
    let responder =
        ReceiptFencedInteractionResponderV1::initial_response_only(services.responder, &tracking);
    let execution_services = AutomationServices {
        mutation: services.mutation,
        responder: &responder,
        instances: services.instances,
        instance_ids: services.instance_ids,
        teardown: services.teardown,
    };
    match &event.kind {
        EventKind::ButtonClick { .. } | EventKind::ModalSubmit { .. } => {
            execute_static_v1(
                permit,
                &tracking,
                &event,
                &identity,
                &artifact.definition,
                &bindings,
                services.snapshot_provider,
                &execution_services,
            )
            .await
        }
        EventKind::InstanceAction {
            instance_id,
            action,
        } => {
            execute_instance_v1(
                permit,
                &tracking,
                &event,
                &identity,
                instance_id,
                action,
                &bindings,
                services.pinned_resolver,
                services.snapshot_provider,
                &execution_services,
            )
            .await
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn execute_static_v1<P, M, R, S, G, T>(
    permit: &P,
    tracking: &TrackingInteractionEffectPermitV1<'_, P>,
    event: &RuntimeEvent,
    identity: &RunningRuleSetIdentity,
    ruleset: &automation_state::InteractionRuleSet,
    bindings: &resource_resolution::ResourceBindingMap,
    snapshot_provider: &impl GuildRoleSnapshotProvider,
    services: &AutomationServices<'_, M, R, S, G, T>,
) -> AcquiredInteractionExecutionOutcomeV1
where
    P: AcquiredInteractionLifecyclePermitV1
        + InteractionEffectJournalPortV1<Error = <P as InteractionEffectPermitV1>::Error>,
    M: RecoverableDiscordMutationAdapterV1,
    R: InteractionResponder,
    S: InstanceRegistrarV1,
    G: InstanceIdGenerator,
    T: DurableInstanceTeardownServiceV1,
{
    let prepared = match prepare_event_execution(event, ruleset, bindings, identity) {
        Ok(Some(prepared)) => prepared,
        Ok(None) => {
            return finish_terminal_v1(
                permit,
                None,
                AcquiredInteractionTerminalOutcomeV1::NoMatchingRule,
                false,
            )
            .await;
        }
        Err(_) => {
            return finish_terminal_v1(
                permit,
                None,
                AcquiredInteractionTerminalOutcomeV1::StaticPreparationFailed,
                false,
            )
            .await;
        }
    };
    let (context, plan, leading_defer_ephemeral) = prepared.into_parts();
    let Some(action_plan_digest) =
        build_action_plan_digest_v1(permit, &context, &plan, leading_defer_ephemeral)
    else {
        return finish_terminal_v1(
            permit,
            None,
            AcquiredInteractionTerminalOutcomeV1::StaticPreparationFailed,
            false,
        )
        .await;
    };
    let Some(preflight) = build_preflight_execution_v1(
        permit,
        &context,
        &plan,
        &action_plan_digest,
        services.instance_ids,
        snapshot_provider,
        None,
    )
    .await
    else {
        return finish_terminal_v1(
            permit,
            Some(&action_plan_digest),
            AcquiredInteractionTerminalOutcomeV1::StaticPreparationFailed,
            false,
        )
        .await;
    };
    if permit
        .bind_action_plan_digest_v1(&action_plan_digest)
        .await
        .is_err()
    {
        return persistence_failed_v1(
            AcquiredInteractionPersistenceStageV1::ActionPlanBind,
            tracking.any_external_attempt_v1(),
        );
    }
    if leading_defer_ephemeral {
        if let Err(error) = services.responder.defer_ephemeral().await {
            return handle_execution_error_v1(permit, tracking, error, None).await;
        }
    }
    execute_bound_preflighted_plan_v1(
        permit,
        tracking,
        preflight,
        &action_plan_digest,
        leading_defer_ephemeral,
        AcquiredInteractionTerminalOutcomeV1::StaticCompleted,
        services,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn execute_instance_v1<P, M, R, S, G, T, PR, SP>(
    permit: &P,
    tracking: &TrackingInteractionEffectPermitV1<'_, P>,
    event: &RuntimeEvent,
    identity: &RunningRuleSetIdentity,
    instance_id: &automation_instance::InstanceId,
    action: &str,
    bindings: &resource_resolution::ResourceBindingMap,
    pinned_resolver: &PR,
    snapshot_provider: &SP,
    services: &AutomationServices<'_, M, R, S, G, T>,
) -> AcquiredInteractionExecutionOutcomeV1
where
    P: AcquiredInteractionLifecyclePermitV1
        + InteractionEffectJournalPortV1<Error = <P as InteractionEffectPermitV1>::Error>,
    M: RecoverableDiscordMutationAdapterV1,
    R: InteractionResponder,
    S: InstanceRegistrarV1,
    G: InstanceIdGenerator,
    T: DurableInstanceTeardownServiceV1,
    PR: PinnedInstanceResolverV1,
    SP: GuildRoleSnapshotProvider,
{
    let complete_snapshot_request =
        ActionPlanSnapshotRequestV1::complete(event.guild_id, event.actor);
    let action_snapshot = match snapshot_provider
        .action_plan_snapshot_v1(&complete_snapshot_request)
        .await
    {
        Ok(snapshot) => snapshot,
        Err(_) => {
            return finish_terminal_v1(
                permit,
                None,
                AcquiredInteractionTerminalOutcomeV1::InstancePreparationFailed,
                false,
            )
            .await;
        }
    };
    let readiness_snapshot = match GuildRoleSnapshot::from_action_plan_snapshot_v1(&action_snapshot)
    {
        Ok(snapshot) => snapshot,
        Err(_) => {
            return finish_terminal_v1(
                permit,
                None,
                AcquiredInteractionTerminalOutcomeV1::InstancePreparationFailed,
                false,
            )
            .await;
        }
    };
    let prepared = match prepare_instance_action_with_resolver_and_snapshot_v1(
        event,
        instance_id,
        action,
        &identity.key,
        pinned_resolver,
        &readiness_snapshot,
        bindings,
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(_) => {
            return finish_terminal_v1(
                permit,
                None,
                AcquiredInteractionTerminalOutcomeV1::InstancePreparationFailed,
                false,
            )
            .await;
        }
    };
    let (context, plan, leading_defer_ephemeral) = prepared.into_parts();
    let Some(action_plan_digest) =
        build_action_plan_digest_v1(permit, &context, &plan, leading_defer_ephemeral)
    else {
        return finish_terminal_v1(
            permit,
            None,
            AcquiredInteractionTerminalOutcomeV1::InstancePreparationFailed,
            tracking.any_external_attempt_v1(),
        )
        .await;
    };
    let Some(preflight) = build_preflight_execution_v1(
        permit,
        &context,
        &plan,
        &action_plan_digest,
        services.instance_ids,
        snapshot_provider,
        Some(action_snapshot),
    )
    .await
    else {
        return finish_terminal_v1(
            permit,
            Some(&action_plan_digest),
            AcquiredInteractionTerminalOutcomeV1::InstancePreparationFailed,
            false,
        )
        .await;
    };
    if permit
        .bind_action_plan_digest_v1(&action_plan_digest)
        .await
        .is_err()
    {
        return persistence_failed_v1(AcquiredInteractionPersistenceStageV1::ActionPlanBind, false);
    }
    if leading_defer_ephemeral {
        if let Err(error) = services.responder.defer_ephemeral().await {
            return handle_execution_error_v1(permit, tracking, error, None).await;
        }
    }
    execute_bound_preflighted_plan_v1(
        permit,
        tracking,
        preflight,
        &action_plan_digest,
        leading_defer_ephemeral,
        AcquiredInteractionTerminalOutcomeV1::InstanceCompleted,
        services,
    )
    .await
}

struct PreflightExecutionEnvelopeV1 {
    plan: PreflightedActionPlanV1,
    snapshot: ActionPlanSnapshotV1,
    wire: ActionPlanWirePreflightV1,
    expected_snapshot_identity: ActionPlanSnapshotIdentityV1,
    certificate: InteractionActionPreflightCertificateV1,
}

async fn build_preflight_execution_v1<P, G, SP>(
    permit: &P,
    context: &RuntimeContext,
    plan: &ActionPlan,
    action_plan_digest: &InteractionActionPlanDigestV1,
    instance_ids: &G,
    snapshot_provider: &SP,
    captured_snapshot: Option<ActionPlanSnapshotV1>,
) -> Option<PreflightExecutionEnvelopeV1>
where
    P: AcquiredInteractionLifecyclePermitV1,
    G: InstanceIdGenerator,
    SP: GuildRoleSnapshotProvider,
{
    let prepared = prepare_action_plan_v1(context, plan, instance_ids).ok()?;
    let wire = preflight_action_plan_wire_v1(&prepared).ok()?;
    let claim = permit.authoritative_claim_v1();
    if !authoritative_teardown_manifest_matches_v1(claim.route(), &wire) {
        return None;
    }
    let combined_preflight_material = combined_preflight_digest_material_v1(
        prepared.digest_material_v1(),
        wire.wire_digest_material_v1(),
    )?;
    let preflight_plan_digest =
        InteractionPreflightPlanDigestV1::from_canonical_bytes(&combined_preflight_material);
    let snapshot = match captured_snapshot {
        Some(snapshot) => snapshot,
        None if prepared.snapshot_request().observations().is_empty() => {
            no_observation_snapshot_v1(prepared.snapshot_request()).ok()?
        }
        None => snapshot_provider
            .action_plan_snapshot_v1(prepared.snapshot_request())
            .await
            .ok()?,
    };
    let expected_snapshot_identity = snapshot.identity.clone();
    let retained_snapshot = snapshot.clone();
    let snapshot_digest = InteractionPreflightSnapshotDigestV1::from_canonical_bytes(
        expected_snapshot_identity.as_str().as_bytes(),
    );
    let plan = preflight_action_plan_v1(prepared, snapshot).ok()?;
    if plan.snapshot_identity() != &expected_snapshot_identity {
        return None;
    }
    let claim = permit.authoritative_claim_v1();
    if !authoritative_teardown_manifest_matches_v1(claim.route(), &wire) {
        return None;
    }
    let certificate = InteractionActionPreflightCertificateV1::issue(
        claim.claim_root,
        action_plan_digest.clone(),
        preflight_plan_digest.clone(),
        snapshot_digest.clone(),
    );
    certificate
        .verify(
            claim.claim_root,
            action_plan_digest,
            &preflight_plan_digest,
            &snapshot_digest,
        )
        .ok()?;
    Some(PreflightExecutionEnvelopeV1 {
        plan,
        snapshot: retained_snapshot,
        wire,
        expected_snapshot_identity,
        certificate,
    })
}

fn combined_preflight_digest_material_v1(semantic: &[u8], wire: &[u8]) -> Option<Vec<u8>> {
    let domain_len = u32::try_from(COMBINED_PREFLIGHT_DIGEST_DOMAIN_V1.len()).ok()?;
    let semantic_len = u32::try_from(semantic.len()).ok()?;
    let wire_len = u32::try_from(wire.len()).ok()?;
    let capacity = 18usize
        .checked_add(COMBINED_PREFLIGHT_DIGEST_DOMAIN_V1.len())?
        .checked_add(semantic.len())?
        .checked_add(wire.len())?;
    if capacity > MAX_COMBINED_PREFLIGHT_DIGEST_MATERIAL_BYTES_V1 {
        return None;
    }
    let mut material = Vec::with_capacity(capacity);
    material.extend_from_slice(&0u16.to_be_bytes());
    material.extend_from_slice(&domain_len.to_be_bytes());
    material.extend_from_slice(COMBINED_PREFLIGHT_DIGEST_DOMAIN_V1);
    material.extend_from_slice(&1u16.to_be_bytes());
    material.extend_from_slice(&semantic_len.to_be_bytes());
    material.extend_from_slice(semantic);
    material.extend_from_slice(&2u16.to_be_bytes());
    material.extend_from_slice(&wire_len.to_be_bytes());
    material.extend_from_slice(wire);
    Some(material)
}

fn authoritative_teardown_manifest_matches_v1(
    route: &InteractionRouteBindingV1,
    wire: &ActionPlanWirePreflightV1,
) -> bool {
    let mut manifests = wire.teardown_manifests();
    let Some(manifest) = manifests.next() else {
        return true;
    };
    if manifests.next().is_some() {
        return false;
    }
    match route.execution_route() {
        InteractionExecutionRouteV1::Instance {
            instance_id,
            resource_manifest_digest,
            ..
        } => manifest.instance_id() == instance_id && manifest.digest() == resource_manifest_digest,
        InteractionExecutionRouteV1::Static { .. } => false,
    }
}

fn no_observation_snapshot_v1(
    request: &ActionPlanSnapshotRequestV1,
) -> Result<ActionPlanSnapshotV1, automation_core::preflight::ActionPlanPreflightErrorV1> {
    let mut bytes = Vec::with_capacity(64);
    bytes.extend_from_slice(b"starring.runtime.no_observation_snapshot.v1\0");
    bytes.extend_from_slice(&request.guild_id().0.to_be_bytes());
    bytes.extend_from_slice(&request.actor().0.to_be_bytes());
    let digest = Sha256::digest(bytes);
    let mut identity = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        write!(&mut identity, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok(ActionPlanSnapshotV1 {
        guild_id: request.guild_id(),
        identity: ActionPlanSnapshotIdentityV1::new(identity)?,
        roles: None,
        channels: None,
        bot_member: None,
        actor_member: None,
    })
}

async fn execute_bound_preflighted_plan_v1<P, M, R, S, G, T>(
    permit: &P,
    tracking: &TrackingInteractionEffectPermitV1<'_, P>,
    preflight: PreflightExecutionEnvelopeV1,
    action_plan_digest: &InteractionActionPlanDigestV1,
    leading_defer_ephemeral: bool,
    success: AcquiredInteractionTerminalOutcomeV1,
    services: &AutomationServices<'_, M, R, S, G, T>,
) -> AcquiredInteractionExecutionOutcomeV1
where
    P: AcquiredInteractionLifecyclePermitV1
        + InteractionEffectJournalPortV1<Error = <P as InteractionEffectPermitV1>::Error>,
    M: RecoverableDiscordMutationAdapterV1,
    R: InteractionResponder,
    S: InstanceRegistrarV1,
    G: InstanceIdGenerator,
    T: DurableInstanceTeardownServiceV1,
{
    let Some(combined_preflight_material) = combined_preflight_digest_material_v1(
        preflight.plan.digest_material_v1(),
        preflight.wire.wire_digest_material_v1(),
    ) else {
        return finish_terminal_v1(
            permit,
            Some(action_plan_digest),
            AcquiredInteractionTerminalOutcomeV1::ExecutionFailedBeforeEffect,
            tracking.any_external_attempt_v1(),
        )
        .await;
    };
    let preflight_plan_digest =
        InteractionPreflightPlanDigestV1::from_canonical_bytes(&combined_preflight_material);
    let snapshot_digest = InteractionPreflightSnapshotDigestV1::from_canonical_bytes(
        preflight.expected_snapshot_identity.as_str().as_bytes(),
    );
    let claim = permit.authoritative_claim_v1();
    if !authoritative_teardown_manifest_matches_v1(claim.route(), &preflight.wire)
        || preflight
            .certificate
            .verify(
                claim.claim_root,
                action_plan_digest,
                &preflight_plan_digest,
                &snapshot_digest,
            )
            .is_err()
    {
        return finish_terminal_v1(
            permit,
            Some(action_plan_digest),
            AcquiredInteractionTerminalOutcomeV1::ExecutionFailedBeforeEffect,
            tracking.any_external_attempt_v1(),
        )
        .await;
    }
    let effect_plan = match build_interaction_effect_execution_plan_v1(
        &preflight.plan,
        &preflight.snapshot,
        &preflight.wire,
        &preflight.certificate,
        action_plan_digest,
        claim.identity(),
    ) {
        Ok(plan) => plan,
        Err(_) => {
            return finish_terminal_v1(
                permit,
                Some(action_plan_digest),
                AcquiredInteractionTerminalOutcomeV1::ExecutionFailedBeforeEffect,
                tracking.any_external_attempt_v1(),
            )
            .await;
        }
    };
    let exact_teardown_requests = preflight
        .wire
        .exact_teardown_requests_v1(preflight.plan.context().guild_id)
        .collect::<Vec<_>>();
    if exact_teardown_requests.len() > 1
        || effect_plan.snapshot_digest() != preflight.certificate.snapshot_digest()
        || effect_plan
            .entries()
            .iter()
            .enumerate()
            .any(|(index, entry)| {
                entry.action_entry() != entry.action().entry()
                    || usize::from(entry.definition().action().action_index().get()) != index
                    || entry.expected_postimage_digest().as_str().len() != 64
            })
    {
        return finish_terminal_v1(
            permit,
            Some(action_plan_digest),
            AcquiredInteractionTerminalOutcomeV1::ExecutionFailedBeforeEffect,
            tracking.any_external_attempt_v1(),
        )
        .await;
    }
    let exact_teardowns = match ExactInteractionTeardownSetV1::new(exact_teardown_requests) {
        Ok(exact) => exact,
        Err(_) => {
            return finish_terminal_v1(
                permit,
                Some(action_plan_digest),
                AcquiredInteractionTerminalOutcomeV1::ExecutionFailedBeforeEffect,
                tracking.any_external_attempt_v1(),
            )
            .await;
        }
    };
    let journal_plan = InteractionEffectJournalPlanV1::new(
        preflight.certificate.digest().clone(),
        effect_plan.snapshot_digest().clone(),
        effect_plan
            .entries()
            .iter()
            .map(|entry| {
                InteractionEffectJournalPlanEntryV1::new(
                    entry.definition().clone(),
                    entry.expected_postimage_digest().clone(),
                )
            })
            .collect(),
    );
    let journaled_services = JournaledActionExecutionServicesV1 {
        journal: tracking,
        mutation: services.mutation,
        responder: services.responder,
        instances: services.instances,
        teardown: services.teardown,
        exact_teardowns: &exact_teardowns,
    };
    match execute_journaled_action_plan_v1(
        &preflight.plan,
        &effect_plan,
        &journal_plan,
        leading_defer_ephemeral,
        &journaled_services,
    )
    .await
    {
        JournaledActionExecutionOutcomeV1::Completed(_) => {
            finish_terminal_v1(
                permit,
                Some(action_plan_digest),
                success,
                !journal_plan.entries().is_empty() || tracking.any_external_attempt_v1(),
            )
            .await
        }
        JournaledActionExecutionOutcomeV1::Stopped { stop, .. } => {
            let effect_stage = matches!(
                stop.stage(),
                JournaledActionExecutionStageV1::Materialization
                    | JournaledActionExecutionStageV1::EffectIntent
                    | JournaledActionExecutionStageV1::EffectCall
                    | JournaledActionExecutionStageV1::EffectFinish
            );
            if effect_stage && (stop.action_entry().is_none() || stop.effect_index().is_none()) {
                return finish_terminal_v1(
                    permit,
                    Some(action_plan_digest),
                    AcquiredInteractionTerminalOutcomeV1::ExecutionRecoveryRequired,
                    tracking.any_external_attempt_v1(),
                )
                .await;
            }
            if stop.reason() == JournaledActionExecutionStopReasonV1::JournalUnavailable {
                let stage = tracking
                    .persistence_failure_stage_v1()
                    .unwrap_or(match stop.stage() {
                        JournaledActionExecutionStageV1::PlanBind => {
                            AcquiredInteractionPersistenceStageV1::EffectPlanBind
                        }
                        JournaledActionExecutionStageV1::EffectIntent => {
                            AcquiredInteractionPersistenceStageV1::EffectIntent
                        }
                        JournaledActionExecutionStageV1::EffectFinish => {
                            AcquiredInteractionPersistenceStageV1::EffectResult
                        }
                        JournaledActionExecutionStageV1::Projection
                        | JournaledActionExecutionStageV1::Materialization
                        | JournaledActionExecutionStageV1::EffectCall
                        | JournaledActionExecutionStageV1::InitialResponse => {
                            AcquiredInteractionPersistenceStageV1::EffectResult
                        }
                    });
                return persistence_failed_v1(
                    stage,
                    stop.stage() == JournaledActionExecutionStageV1::EffectFinish
                        || tracking.any_external_attempt_v1(),
                );
            }
            if stop.reason() == JournaledActionExecutionStopReasonV1::ResponseFailure {
                return handle_execution_error_v1(
                    permit,
                    tracking,
                    AdapterError::new(
                        automation_core::AdapterErrorKind::Unknown,
                        "Discord initial response failed",
                    ),
                    Some(action_plan_digest),
                )
                .await;
            }
            if stop.recovery_scope()
                == Some(
                    automation_runtime_interaction::InteractionEffectRecoveryScopeV1::ResponseTail,
                )
                && matches!(
                    stop.reason(),
                    JournaledActionExecutionStopReasonV1::Indeterminate
                        | JournaledActionExecutionStopReasonV1::ExactReplaySuppressed
                )
            {
                return AcquiredInteractionExecutionOutcomeV1::EffectRecoveryPending;
            }
            let outcome = if stop.recovery_scope()
                == Some(
                    automation_runtime_interaction::InteractionEffectRecoveryScopeV1::ResponseTail,
                ) {
                AcquiredInteractionTerminalOutcomeV1::ProvisioningCompletedResponseUnconfirmed
            } else if stop.reason() == JournaledActionExecutionStopReasonV1::KnownFailure
                && !stop.rollback_requested()
            {
                AcquiredInteractionTerminalOutcomeV1::ExecutionKnownFailed
            } else if stop.reason() == JournaledActionExecutionStopReasonV1::ProtocolViolation
                && !stop.rollback_requested()
                && stop.stage() != JournaledActionExecutionStageV1::EffectCall
            {
                AcquiredInteractionTerminalOutcomeV1::ExecutionFailedBeforeEffect
            } else {
                AcquiredInteractionTerminalOutcomeV1::ExecutionRecoveryRequired
            };
            let external_effect_may_have_occurred = tracking.any_external_attempt_v1()
                || matches!(
                    stop.stage(),
                    JournaledActionExecutionStageV1::EffectCall
                        | JournaledActionExecutionStageV1::EffectFinish
                )
                || stop.rollback_requested()
                || stop.reason() == JournaledActionExecutionStopReasonV1::ExactReplaySuppressed;
            finish_terminal_v1(
                permit,
                Some(action_plan_digest),
                outcome,
                external_effect_may_have_occurred,
            )
            .await
        }
    }
}

async fn handle_execution_error_v1<P: AcquiredInteractionLifecyclePermitV1>(
    permit: &P,
    tracking: &TrackingInteractionEffectPermitV1<'_, P>,
    _error: AdapterError,
    action_plan_digest: Option<&InteractionActionPlanDigestV1>,
) -> AcquiredInteractionExecutionOutcomeV1 {
    if let Some(result) = tracking.committed_initial_response_result_v1() {
        if result != InteractionInitialResponseResultKindV1::Succeeded {
            let state = match result {
                InteractionInitialResponseResultKindV1::DefinitiveFailure
                    if !tracking.execution_intended_v1() =>
                {
                    InteractionReceiptStateV1::Failed
                }
                InteractionInitialResponseResultKindV1::DefinitiveFailure
                | InteractionInitialResponseResultKindV1::Indeterminate => {
                    InteractionReceiptStateV1::RecoveryRequired
                }
                InteractionInitialResponseResultKindV1::Succeeded => unreachable!(),
            };
            return AcquiredInteractionExecutionOutcomeV1::AcknowledgementTerminalized {
                state,
                result,
            };
        }
    }
    if let Some(stage) = tracking.persistence_failure_stage_v1() {
        return persistence_failed_v1(stage, tracking.any_external_attempt_v1());
    }
    let outcome = if tracking.execution_effect_attempted_v1() {
        AcquiredInteractionTerminalOutcomeV1::ExecutionRecoveryRequired
    } else {
        AcquiredInteractionTerminalOutcomeV1::ExecutionFailedBeforeEffect
    };
    finish_terminal_v1(
        permit,
        action_plan_digest,
        outcome,
        tracking.any_external_attempt_v1(),
    )
    .await
}

fn build_action_plan_digest_v1<P: AcquiredInteractionLifecyclePermitV1>(
    permit: &P,
    context: &RuntimeContext,
    plan: &ActionPlan,
    leading_defer_ephemeral: bool,
) -> Option<InteractionActionPlanDigestV1> {
    let claim = permit.authoritative_claim_v1();
    build_interaction_action_plan_digest_v1(
        claim.route(),
        claim.request_digest(),
        context,
        plan,
        leading_defer_ephemeral,
    )
    .ok()
}

async fn finish_terminal_v1<P: AcquiredInteractionLifecyclePermitV1>(
    permit: &P,
    action_plan_digest: Option<&InteractionActionPlanDigestV1>,
    outcome: AcquiredInteractionTerminalOutcomeV1,
    external_effect_may_have_occurred: bool,
) -> AcquiredInteractionExecutionOutcomeV1 {
    let finish = {
        let claim = permit.authoritative_claim_v1();
        build_terminal_finish_v1(&claim, action_plan_digest, outcome)
    };
    match permit.finish_interaction_v1(&finish).await {
        Ok(()) => AcquiredInteractionExecutionOutcomeV1::Terminalized(finish),
        Err(_) => persistence_failed_v1(
            AcquiredInteractionPersistenceStageV1::TerminalFinish,
            external_effect_may_have_occurred,
        ),
    }
}

fn build_terminal_finish_v1(
    claim: &AuthoritativeInteractionClaimV1<'_>,
    action_plan_digest: Option<&InteractionActionPlanDigestV1>,
    outcome: AcquiredInteractionTerminalOutcomeV1,
) -> InteractionTerminalFinishV1 {
    let mut frame = CanonicalTerminalFrameV1::new();
    let identity = claim.identity();
    frame.u64(3, identity.application_id().get());
    frame.u64(4, identity.interaction_id().get());
    frame.text(5, claim.request_digest().as_str());
    frame.u8(6, u8::from(action_plan_digest.is_some()));
    if let Some(digest) = action_plan_digest {
        frame.text(7, digest.as_str());
    }
    frame.text(8, receipt_state_code_v1(outcome.state()));
    frame.text(9, outcome.code());
    InteractionTerminalFinishV1 {
        state: outcome.state(),
        outcome,
        action_plan_digest: action_plan_digest.cloned(),
        terminal_digest: InteractionTerminalDigestV1(Sha256::digest(frame.finish()).into()),
    }
}

fn receipt_state_code_v1(state: InteractionReceiptStateV1) -> &'static str {
    match state {
        InteractionReceiptStateV1::Completed => "completed",
        InteractionReceiptStateV1::Failed => "failed",
        InteractionReceiptStateV1::RecoveryRequired => "recovery_required",
        InteractionReceiptStateV1::Claimed
        | InteractionReceiptStateV1::Acknowledging
        | InteractionReceiptStateV1::Deferred
        | InteractionReceiptStateV1::Prepared
        | InteractionReceiptStateV1::Executing => {
            unreachable!("terminal finish state is always terminal")
        }
    }
}

fn authoritative_claim_matches_v1<P: AcquiredInteractionLifecyclePermitV1>(
    admitted: &SharedGatewayAdmittedInteractionV3,
    envelope: &SharedGatewayInteractionEnvelopeV3,
    permit: &P,
) -> bool {
    let claim = permit.authoritative_claim_v1();
    let identity = envelope.identity_v3();
    if claim.identity().application_id().get() != identity.application_id().get()
        || claim.identity().interaction_id().get() != identity.interaction_id().get()
        || claim.route().process_identity() != admitted.route().process_identity()
        || claim.route().scope()
            != &automation_runtime_interaction::InteractionProductScopeV1::from_deployment_identity(
                admitted.route().deployment_identity(),
            )
    {
        return false;
    }
    envelope
        .receipt_request_digest_v1(claim.identity())
        .is_ok_and(|digest| &digest == claim.request_digest())
}

fn authoritative_execution_route_matches_v1<P: AcquiredInteractionLifecyclePermitV1>(
    event: &RuntimeEvent,
    permit: &P,
) -> bool {
    let claim = permit.authoritative_claim_v1();
    match (&event.kind, claim.route().execution_route()) {
        (
            EventKind::ButtonClick { .. } | EventKind::ModalSubmit { .. },
            automation_runtime_interaction::InteractionExecutionRouteV1::Static { .. },
        ) => true,
        (
            EventKind::InstanceAction { instance_id, .. },
            automation_runtime_interaction::InteractionExecutionRouteV1::Instance {
                instance_id: expected,
                ..
            },
        ) => instance_id == expected,
        _ => false,
    }
}

fn persistence_failed_v1(
    stage: AcquiredInteractionPersistenceStageV1,
    external_effect_may_have_occurred: bool,
) -> AcquiredInteractionExecutionOutcomeV1 {
    AcquiredInteractionExecutionOutcomeV1::PersistenceFailed {
        stage,
        external_effect_may_have_occurred,
    }
}

struct TrackingInteractionEffectPermitV1<'a, P> {
    permit: &'a P,
    initial_response_attempted: AtomicBool,
    non_defer_response_attempted: AtomicBool,
    execution_intended: AtomicBool,
    persistence_failure_stage: AtomicU8,
    committed_initial_response_result: AtomicU8,
}

impl<'a, P> TrackingInteractionEffectPermitV1<'a, P> {
    fn new(permit: &'a P) -> Self {
        Self {
            permit,
            initial_response_attempted: AtomicBool::new(false),
            non_defer_response_attempted: AtomicBool::new(false),
            execution_intended: AtomicBool::new(false),
            persistence_failure_stage: AtomicU8::new(0),
            committed_initial_response_result: AtomicU8::new(0),
        }
    }

    fn committed_initial_response_result_v1(
        &self,
    ) -> Option<InteractionInitialResponseResultKindV1> {
        match self
            .committed_initial_response_result
            .load(Ordering::SeqCst)
        {
            1 => Some(InteractionInitialResponseResultKindV1::Succeeded),
            2 => Some(InteractionInitialResponseResultKindV1::DefinitiveFailure),
            3 => Some(InteractionInitialResponseResultKindV1::Indeterminate),
            _ => None,
        }
    }

    fn execution_effect_attempted_v1(&self) -> bool {
        self.execution_intended.load(Ordering::SeqCst)
            || self.non_defer_response_attempted.load(Ordering::SeqCst)
    }

    fn persistence_failure_stage_v1(&self) -> Option<AcquiredInteractionPersistenceStageV1> {
        match self.persistence_failure_stage.load(Ordering::SeqCst) {
            1 => Some(AcquiredInteractionPersistenceStageV1::InitialResponseIntent),
            2 => Some(AcquiredInteractionPersistenceStageV1::InitialResponseResult),
            3 => Some(AcquiredInteractionPersistenceStageV1::ExecutionIntent),
            _ => None,
        }
    }

    fn execution_intended_v1(&self) -> bool {
        self.execution_intended.load(Ordering::SeqCst)
    }

    fn any_external_attempt_v1(&self) -> bool {
        self.initial_response_attempted.load(Ordering::SeqCst)
    }
}

impl<P: InteractionEffectPermitV1> InteractionEffectPermitV1
    for TrackingInteractionEffectPermitV1<'_, P>
{
    type Error = P::Error;

    fn initial_response_deadline_v1(&self) -> Instant {
        self.permit.initial_response_deadline_v1()
    }

    async fn commit_initial_response_intent_v1(
        &self,
        intent: &InteractionInitialResponseIntentV1,
    ) -> Result<InteractionInitialResponseIntentDispositionV1, Self::Error> {
        let disposition = match self.permit.commit_initial_response_intent_v1(intent).await {
            Ok(disposition) => disposition,
            Err(error) => {
                self.persistence_failure_stage.store(1, Ordering::SeqCst);
                return Err(error);
            }
        };
        self.initial_response_attempted
            .store(true, Ordering::SeqCst);
        if intent.kind() != InteractionInitialResponseKindV1::DeferEphemeral {
            self.non_defer_response_attempted
                .store(true, Ordering::SeqCst);
        }
        Ok(disposition)
    }

    async fn commit_initial_response_result_v1(
        &self,
        result: &InteractionInitialResponseResultV1,
    ) -> Result<(), Self::Error> {
        if let Err(error) = self.permit.commit_initial_response_result_v1(result).await {
            self.persistence_failure_stage.store(2, Ordering::SeqCst);
            return Err(error);
        }
        self.committed_initial_response_result.store(
            match result.result() {
                InteractionInitialResponseResultKindV1::Succeeded => 1,
                InteractionInitialResponseResultKindV1::DefinitiveFailure => 2,
                InteractionInitialResponseResultKindV1::Indeterminate => 3,
            },
            Ordering::SeqCst,
        );
        Ok(())
    }

    async fn commit_idempotent_execution_intent_v1(&self) -> Result<(), Self::Error> {
        if let Err(error) = self.permit.commit_idempotent_execution_intent_v1().await {
            self.persistence_failure_stage.store(3, Ordering::SeqCst);
            return Err(error);
        }
        self.execution_intended.store(true, Ordering::SeqCst);
        Ok(())
    }
}

impl<P> InteractionEffectJournalPortV1 for TrackingInteractionEffectPermitV1<'_, P>
where
    P: InteractionEffectPermitV1
        + InteractionEffectJournalPortV1<Error = <P as InteractionEffectPermitV1>::Error>,
{
    type Error = <P as InteractionEffectPermitV1>::Error;
    type IntentPermit = <P as InteractionEffectJournalPortV1>::IntentPermit;

    async fn bind_effect_plan_v1(
        &self,
        plan: &InteractionEffectJournalPlanV1,
    ) -> Result<InteractionEffectPlanBindDispositionV1, Self::Error> {
        self.permit.bind_effect_plan_v1(plan).await
    }

    async fn intend_effect_v1(
        &self,
        intent: InteractionEffectJournalIntendV1<'_>,
    ) -> Result<InteractionEffectIntentDispositionV1<Self::IntentPermit>, Self::Error> {
        self.commit_idempotent_execution_intent_v1().await?;
        self.permit.intend_effect_v1(intent).await
    }

    async fn finish_effect_v1(
        &self,
        permit: &Self::IntentPermit,
        materialized: &automation_runtime_interaction::InteractionEffectMaterializedPlanV1,
        outcome: &automation_runtime_interaction::InteractionEffectAttemptOutcomeV1,
    ) -> Result<(), Self::Error> {
        self.permit
            .finish_effect_v1(permit, materialized, outcome)
            .await
    }
}

struct CanonicalTerminalFrameV1 {
    bytes: Vec<u8>,
}

impl CanonicalTerminalFrameV1 {
    fn new() -> Self {
        let mut frame = Self {
            bytes: Vec::with_capacity(256),
        };
        frame.bytes(1, TERMINAL_DIGEST_DOMAIN_V1);
        frame.u16(2, TERMINAL_DIGEST_VERSION_V1);
        frame
    }

    fn bytes(&mut self, tag: u16, value: &[u8]) {
        self.bytes.extend_from_slice(&tag.to_be_bytes());
        self.bytes.extend_from_slice(
            &u64::try_from(value.len())
                .expect("terminal canonical field length fits u64")
                .to_be_bytes(),
        );
        self.bytes.extend_from_slice(value);
    }

    fn text(&mut self, tag: u16, value: &str) {
        self.bytes(tag, value.as_bytes());
    }

    fn u8(&mut self, tag: u16, value: u8) {
        self.bytes(tag, &[value]);
    }

    fn u16(&mut self, tag: u16, value: u16) {
        self.bytes(tag, &value.to_be_bytes());
    }

    fn u64(&mut self, tag: u16, value: u64) {
        self.bytes(tag, &value.to_be_bytes());
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

#[cfg(test)]
mod tests;
