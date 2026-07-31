use std::fmt::{Debug, Formatter};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use automation_core::{
    prepare_event_execution, run, ActionPlan, AdapterError, AutomationServices,
    DiscordMutationAdapter, EventKind, InteractionResponder, RunningRuleSetIdentity,
    RuntimeContext, RuntimeEvent,
};
use automation_instance::{InstanceIdGenerator, InstanceRegistrarV1};
use automation_instance_teardown::InstanceTeardownService;
use automation_ruleset_dispatch::{
    prepare_instance_action_with_resolver_v1, GuildRoleSnapshotProvider, PinnedInstanceResolverV1,
};
use automation_runtime_interaction::{
    InteractionActionPlanDigestV1, InteractionReceiptClaimRootV1, InteractionReceiptIdentityV1,
    InteractionReceiptStateV1, InteractionRequestDigestV1, InteractionRouteBindingV1,
};
use sha2::{Digest, Sha256};

use crate::action_plan_digest::build_interaction_action_plan_digest_v1;
use crate::convert::interaction_to_event;
use crate::receipt_fenced_effects::{
    InteractionEffectPermitV1, InteractionInitialResponseIntentDispositionV1,
    InteractionInitialResponseIntentV1, InteractionInitialResponseKindV1,
    InteractionInitialResponseResultKindV1, InteractionInitialResponseResultV1,
    ReceiptFencedDiscordMutationAdapterV1, ReceiptFencedInteractionResponderV1,
};
use crate::shared_gateway_admission::SharedGatewayAdmittedInteractionV3;
use crate::shared_gateway_dispatcher::SharedGatewayInteractionEnvelopeV3;
use crate::shared_gateway_executor::execution_inputs;

const TERMINAL_DIGEST_DOMAIN_V1: &[u8] = b"starring.runtime.acquired_receipt.terminal.v1\0";
const TERMINAL_DIGEST_VERSION_V1: u16 = 1;

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
    EventConversionFailed,
    NoMatchingRule,
    StaticPreparationFailed,
    InstancePreparationFailed,
    ExecutionFailedBeforeEffect,
    ExecutionRecoveryRequired,
}

impl AcquiredInteractionTerminalOutcomeV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::StaticCompleted => "interaction_static_completed",
            Self::InstanceCompleted => "interaction_instance_completed",
            Self::EventConversionFailed => "interaction_event_conversion_failed",
            Self::NoMatchingRule => "interaction_no_matching_rule",
            Self::StaticPreparationFailed => "interaction_static_preparation_failed",
            Self::InstancePreparationFailed => "interaction_instance_preparation_failed",
            Self::ExecutionFailedBeforeEffect => "interaction_execution_failed_before_effect",
            Self::ExecutionRecoveryRequired => "interaction_execution_recovery_required",
        }
    }

    pub const fn state(self) -> InteractionReceiptStateV1 {
        match self {
            Self::StaticCompleted | Self::InstanceCompleted => InteractionReceiptStateV1::Completed,
            Self::EventConversionFailed
            | Self::NoMatchingRule
            | Self::StaticPreparationFailed
            | Self::InstancePreparationFailed
            | Self::ExecutionFailedBeforeEffect => InteractionReceiptStateV1::Failed,
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
    P: AcquiredInteractionLifecyclePermitV1,
    M: DiscordMutationAdapter + ?Sized,
    R: InteractionResponder + ?Sized,
    S: InstanceRegistrarV1,
    G: InstanceIdGenerator,
    T: InstanceTeardownService,
    PR: PinnedInstanceResolverV1,
    SP: GuildRoleSnapshotProvider,
{
    let outcome =
        execute_acquired_interaction_inner_v1(&admitted, &envelope, &permit, &services).await;
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
    P: AcquiredInteractionLifecyclePermitV1,
    M: DiscordMutationAdapter + ?Sized,
    R: InteractionResponder + ?Sized,
    S: InstanceRegistrarV1,
    G: InstanceIdGenerator,
    T: InstanceTeardownService,
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
    let responder = ReceiptFencedInteractionResponderV1::new(services.responder, &tracking);
    let mutation = ReceiptFencedDiscordMutationAdapterV1::new(services.mutation, &tracking);
    let execution_services = AutomationServices {
        mutation: &mutation,
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
    services: &AutomationServices<'_, M, R, S, G, T>,
) -> AcquiredInteractionExecutionOutcomeV1
where
    P: AcquiredInteractionLifecyclePermitV1,
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceRegistrarV1,
    G: InstanceIdGenerator,
    T: InstanceTeardownService,
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
            .await
        }
        Err(_) => {
            return finish_terminal_v1(
                permit,
                None,
                AcquiredInteractionTerminalOutcomeV1::StaticPreparationFailed,
                false,
            )
            .await
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
    if leading_defer_ephemeral {
        if let Err(error) = services.responder.defer_ephemeral().await {
            return handle_execution_error_v1(permit, tracking, error, None).await;
        }
    }
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
    execute_bound_plan_v1(
        permit,
        tracking,
        &context,
        &plan,
        &action_plan_digest,
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
    P: AcquiredInteractionLifecyclePermitV1,
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceRegistrarV1,
    G: InstanceIdGenerator,
    T: InstanceTeardownService,
    PR: PinnedInstanceResolverV1,
    SP: GuildRoleSnapshotProvider,
{
    if let Err(error) = services.responder.defer_ephemeral().await {
        return handle_execution_error_v1(permit, tracking, error, None).await;
    }
    let prepared = match prepare_instance_action_with_resolver_v1(
        event,
        instance_id,
        action,
        &identity.key,
        pinned_resolver,
        snapshot_provider,
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
                tracking.any_external_attempt_v1(),
            )
            .await
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
    if permit
        .bind_action_plan_digest_v1(&action_plan_digest)
        .await
        .is_err()
    {
        return persistence_failed_v1(AcquiredInteractionPersistenceStageV1::ActionPlanBind, true);
    }
    execute_bound_plan_v1(
        permit,
        tracking,
        &context,
        &plan,
        &action_plan_digest,
        AcquiredInteractionTerminalOutcomeV1::InstanceCompleted,
        services,
    )
    .await
}

async fn execute_bound_plan_v1<P, M, R, S, G, T>(
    permit: &P,
    tracking: &TrackingInteractionEffectPermitV1<'_, P>,
    context: &RuntimeContext,
    plan: &ActionPlan,
    action_plan_digest: &InteractionActionPlanDigestV1,
    success: AcquiredInteractionTerminalOutcomeV1,
    services: &AutomationServices<'_, M, R, S, G, T>,
) -> AcquiredInteractionExecutionOutcomeV1
where
    P: AcquiredInteractionLifecyclePermitV1,
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceRegistrarV1,
    G: InstanceIdGenerator,
    T: InstanceTeardownService,
{
    match run(context, plan, services).await {
        Ok(_) => {
            finish_terminal_v1(
                permit,
                Some(action_plan_digest),
                success,
                tracking.any_external_attempt_v1(),
            )
            .await
        }
        Err(error) => {
            handle_execution_error_v1(permit, tracking, error, Some(action_plan_digest)).await
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
            || self.execution_intended.load(Ordering::SeqCst)
    }
}

impl<P: InteractionEffectPermitV1> InteractionEffectPermitV1
    for TrackingInteractionEffectPermitV1<'_, P>
{
    type Error = P::Error;

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
