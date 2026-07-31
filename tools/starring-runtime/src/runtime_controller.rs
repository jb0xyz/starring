use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::num::NonZeroU64;
use std::time::{Duration, Instant};

use automation_runtime::{OwnedDiscordRuntimeOperationsV2, RuntimeDiscordPreflightErrorV1};
use automation_runtime_controller::{
    plan_runtime_action_v1, RuntimeBindingPinV1, RuntimeClaimNextExecutionV1,
    RuntimeControllerActionV1, RuntimeControllerConfigV1, RuntimeConvergenceErrorClassV1,
    RuntimeConvergenceMutationV1, RuntimeConvergenceSessionStateV1, RuntimeConvergenceSessionV1,
    RuntimeDeploymentScopeV1, RuntimeDisconnectServingV1, RuntimeExecutionReceiptV1,
    RuntimeMutationReceiptV1, RuntimeMutationRequestV1, RuntimeObservePreviousServingV1,
    RuntimePreviousServingObservationReceiptV1, RuntimeServingLeasePort, RuntimeServingReceiptV2,
    RuntimeServingSlotV2, RuntimeServingUpdateReceiptV1,
};
use automation_runtime_convergence::{
    ActivationAttestationV1, ActivationOutcomeKindV1, ControllerId, RuntimeDeploymentIdentityV1,
    RuntimeDeploymentPhaseV1, RuntimeDeploymentTargetV1, RuntimeFailureId, RuntimeFailureKindV1,
    RuntimeGeneration, RuntimePendingConditionV1, RuntimeProcessIdentityV1,
};
use automation_runtime_convergence_postgres::{RuntimeConvergenceStoreError, RuntimeExactTargetV1};
use automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1;
use automation_runtime_registry::{
    ExactServingRouteError, ExactServingRouteV1, SlotLifecycleV1, SlotRouteWitnessV1,
};
use automation_runtime_worker::{
    RuntimeAcceptPreflightMutationErrorV2, RuntimeAuthorityPayloadDigestV2,
    RuntimeBarrierACorrelationV2, RuntimeBarrierAPauseObservationV2, RuntimeBarrierAPausePortV2,
    RuntimeBarrierAPauseRequestV2, RuntimeBarrierAResumeObservationV2, RuntimeBarrierAResumePortV2,
    RuntimeBarrierAResumeRequestV2, RuntimeClaimedConvergenceV2, RuntimeConvergenceMutationPortV2,
    RuntimeDiscordPreflightErrorV2, RuntimeDiscordPreflightObservationV2,
    RuntimeDiscordPreflightOutcomeV2, RuntimeDiscordPreflightPortV2,
    RuntimeDiscordPreflightRequestV2, RuntimeDrainedConvergenceV2,
    RuntimeExactPreviousServingErrorV2, RuntimeExactPreviousServingEvidenceErrorV2,
    RuntimeExactPreviousServingObservationPortV2, RuntimeExactTargetEvidenceV2,
    RuntimeExactTargetHydrationErrorV2, RuntimeExactTargetHydrationPortV2,
    RuntimeExactTargetHydrationRequestV2, RuntimeExactTargetObservationV2,
    RuntimePredecessorRetirementObservationV2, RuntimePredecessorRetirementPortV2,
    RuntimePredecessorRetirementRequestV2, RuntimePreviousServingDisconnectOutcomeV2,
    RuntimePreviousServingDisconnectPortV2, RuntimeReplacementExecutionErrorV2,
    RuntimeRouteLifecycleV2, RuntimeRoutePredecessorRemovalObservationV2,
    RuntimeRoutePredecessorTransitionObservationV2, RuntimeRoutePredecessorTransitionPortV2,
    RuntimeRoutePredecessorTransitionRequestV2, RuntimeRouteStageObservationV2,
    RuntimeRouteStageOutcomeV2, RuntimeRouteStageRequestV2, RuntimeRouteWitnessV2,
    RuntimeServingSlotWorkErrorV2, RuntimeServingSlotWorkPermitV2, RuntimeStagedConvergenceV2,
    RuntimeStagedRecoveryHandoffV2, RuntimeStagedRecoveryRouteV2, RuntimeStagedRoutePortV2,
};
use chrono::{DateTime, TimeDelta, Utc};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::{JoinError, JoinHandle};
use tokio::time::{sleep, sleep_until, timeout_at, Instant as TokioInstant};

use crate::database::RuntimeControllerDatabaseV2;
use crate::panel_reconciliation::{
    RuntimeExactPanelReconciliationDispositionV2, RuntimeExactPanelReconciliationErrorV2,
    RuntimeExactPanelReconciliationRequestV2, RuntimeExactPanelReconciliationV2,
};
use crate::process_supervisor::RuntimeProcessShutdownTriggerV1;
use crate::registry::{
    RuntimeRegistryEmergencyTriggerV2, RuntimeRegistryPredecessorReplacementErrorV2,
    RuntimeRegistryReplacementRouteV2, RuntimeRegistryStagedInstallEvidenceV2,
    RuntimeRegistryStagedInstallOutcomeV2, RuntimeRegistryStagingErrorV2,
    RuntimeRegistryStagingPortV2,
};
use crate::RuntimeShutdownCauseV1;

const RUNTIME_CONTROLLER_COMMAND_CAPACITY_V2: usize = 1;
const RUNTIME_CONTROLLER_IDLE_BACKOFF_V2: Duration = Duration::from_secs(1);
const RUNTIME_CONTROLLER_RETRY_BACKOFF_V2: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub(crate) struct RuntimeServingControllerConfigV2 {
    inner: RuntimeControllerConfigV1,
}

impl RuntimeServingControllerConfigV2 {
    pub(crate) fn production_v2() -> Self {
        let inner = RuntimeControllerConfigV1::default();
        inner
            .validate()
            .expect("default runtime controller configuration must remain valid");
        Self { inner }
    }

    pub(crate) fn gateway_ready_timeout_v2(&self) -> Duration {
        self.inner.gateway_ready_timeout
    }

    fn into_inner_v2(self) -> RuntimeControllerConfigV1 {
        self.inner
    }
}

impl Debug for RuntimeServingControllerConfigV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingControllerConfigV2(<validated>)")
    }
}

type RuntimeSlotPermitReplyV2 =
    Result<RuntimeServingSlotWorkPermitV2, RuntimeServingSlotWorkErrorV2>;
type RuntimeBarrierAPauseReplyV2 = Result<
    RuntimeBarrierAPauseObservationV2<RuntimeControllerBarrierAPausedV2>,
    RuntimeControllerBarrierBridgeErrorV2,
>;
type RuntimeBarrierAResumeReplyV2 = Result<
    RuntimeBarrierAResumeObservationV2<RuntimeControllerBarrierAResumedV2>,
    RuntimeControllerBarrierBridgeErrorV2,
>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeControllerBarrierAPauseCommandV2 {
    pub(crate) correlation: RuntimeBarrierACorrelationV2,
    pub(crate) started_at: DateTime<Utc>,
    pub(crate) deadline: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeControllerBarrierAResumeCommandV2 {
    pub(crate) correlation: RuntimeBarrierACorrelationV2,
    pub(crate) coordinator_generation: NonZeroU64,
    pub(crate) connection_epoch: NonZeroU64,
    pub(crate) pause_admission_revision: NonZeroU64,
    pub(crate) connected_event_sequence: NonZeroU64,
    pub(crate) pause_sequence: NonZeroU64,
    pub(crate) transitioned_at: DateTime<Utc>,
    pub(crate) deadline: DateTime<Utc>,
    pub(crate) paused: RuntimeControllerBarrierAPausedV2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeControllerBarrierAPausedV2 {
    pub(crate) handle: NonZeroU64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeControllerBarrierAResumedV2 {
    pub(crate) handle: NonZeroU64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeControllerBarrierBridgeErrorV2 {
    #[error("runtime controller Barrier A bridge is unavailable")]
    Unavailable,
    #[error("runtime controller Barrier A deadline elapsed")]
    DeadlineElapsed,
    #[error("runtime controller Barrier A authority is stale")]
    StaleAuthority,
    #[error("runtime controller Barrier A outcome is indeterminate")]
    Indeterminate,
}

pub(crate) enum RuntimeServingControllerEventV2 {
    AcquireSlot {
        slot: RuntimeServingSlotV2,
        reply: oneshot::Sender<RuntimeSlotPermitReplyV2>,
    },
    PauseBarrierA {
        command: RuntimeControllerBarrierAPauseCommandV2,
        reply: oneshot::Sender<RuntimeBarrierAPauseReplyV2>,
    },
    ResumeBarrierA {
        command: RuntimeControllerBarrierAResumeCommandV2,
        reply: oneshot::Sender<RuntimeBarrierAResumeReplyV2>,
    },
    Certification {
        handoff: Box<RuntimeControllerCertificationHandoffV2>,
        reply: oneshot::Sender<RuntimeControllerCertificationHandoffReplyV2>,
    },
    Terminal,
}

impl Debug for RuntimeServingControllerEventV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingControllerEventV2(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeServingControllerActorExitV2 {
    Commanded,
    Failed,
    CleanupFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeServingControllerShutdownErrorV2 {
    #[error("runtime serving controller terminated unexpectedly")]
    UnexpectedExit,
    #[error("runtime serving controller shutdown deadline elapsed")]
    DeadlineElapsed,
}

pub(crate) struct RuntimeServingControllerSupervisorV2 {
    events: mpsc::Receiver<RuntimeServingControllerCommandV2>,
    stop: watch::Sender<bool>,
    join: Option<JoinHandle<RuntimeServingControllerActorExitV2>>,
    terminal: Option<RuntimeServingControllerActorExitV2>,
}

impl RuntimeServingControllerSupervisorV2 {
    pub(crate) fn start_v2(
        database: RuntimeControllerDatabaseV2,
        discord_token: String,
        registry: RuntimeRegistryStagingPortV2,
        controller_id: ControllerId,
        shutdown: RuntimeProcessShutdownTriggerV1,
        config: RuntimeServingControllerConfigV2,
    ) -> Self {
        let config = config.into_inner_v2();
        config
            .validate()
            .expect("validated runtime controller configuration must remain valid");
        let (events_tx, events) = mpsc::channel(RUNTIME_CONTROLLER_COMMAND_CAPACITY_V2);
        let (stop, stop_rx) = watch::channel(false);
        let actor = RuntimeServingControllerActorV2 {
            database,
            discord: RuntimeControllerDiscordPreflightPortV2 {
                inner: OwnedDiscordRuntimeOperationsV2::new(discord_token),
            },
            registry,
            controller_id,
            shutdown,
            config,
            events: events_tx,
            stop: stop_rx,
        };
        let join = tokio::spawn(actor.run_v2());
        Self {
            events,
            stop,
            join: Some(join),
            terminal: None,
        }
    }

    pub(crate) async fn next_event_v2(&mut self) -> RuntimeServingControllerEventV2 {
        if self.terminal.is_some() || self.join.is_none() {
            return RuntimeServingControllerEventV2::Terminal;
        }
        enum SelectedV2 {
            Command(Box<RuntimeServingControllerCommandV2>),
            Closed,
            Join(Result<RuntimeServingControllerActorExitV2, JoinError>),
        }
        let selected = {
            let join = self
                .join
                .as_mut()
                .expect("live runtime controller supervisor must retain its task");
            tokio::select! {
                biased;
                command = self.events.recv() => match command {
                    Some(command) => SelectedV2::Command(Box::new(command)),
                    None => SelectedV2::Closed,
                },
                result = join => SelectedV2::Join(result),
            }
        };
        match selected {
            SelectedV2::Command(command) => match *command {
                RuntimeServingControllerCommandV2::AcquireSlot { slot, reply } => {
                    RuntimeServingControllerEventV2::AcquireSlot { slot, reply }
                }
                RuntimeServingControllerCommandV2::PauseBarrierA { command, reply } => {
                    RuntimeServingControllerEventV2::PauseBarrierA { command, reply }
                }
                RuntimeServingControllerCommandV2::ResumeBarrierA { command, reply } => {
                    RuntimeServingControllerEventV2::ResumeBarrierA { command, reply }
                }
                RuntimeServingControllerCommandV2::Certification { handoff, reply } => {
                    RuntimeServingControllerEventV2::Certification { handoff, reply }
                }
            },
            SelectedV2::Closed => {
                self.observe_join_v2().await;
                RuntimeServingControllerEventV2::Terminal
            }
            SelectedV2::Join(result) => {
                self.join.take();
                self.terminal = Some(classify_actor_join_v2(result));
                RuntimeServingControllerEventV2::Terminal
            }
        }
    }

    pub(crate) async fn shutdown_until_v2(
        &mut self,
        deadline: Instant,
    ) -> Result<(), RuntimeServingControllerShutdownErrorV2> {
        let _ = self.stop.send(true);
        if let Some(terminal) = self.terminal {
            return accept_commanded_exit_v2(terminal);
        }
        let Some(mut join) = self.join.take() else {
            return Err(RuntimeServingControllerShutdownErrorV2::UnexpectedExit);
        };
        let result = timeout_at(TokioInstant::from_std(deadline), &mut join).await;
        match result {
            Ok(result) => {
                let terminal = classify_actor_join_v2(result);
                self.terminal = Some(terminal);
                accept_commanded_exit_v2(terminal)
            }
            Err(_) => {
                join.abort();
                let _ = join.await;
                self.terminal = Some(RuntimeServingControllerActorExitV2::CleanupFailed);
                Err(RuntimeServingControllerShutdownErrorV2::DeadlineElapsed)
            }
        }
    }

    async fn observe_join_v2(&mut self) {
        let Some(join) = self.join.take() else {
            return;
        };
        self.terminal = Some(classify_actor_join_v2(join.await));
    }
}

impl Drop for RuntimeServingControllerSupervisorV2 {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        if let Some(join) = self.join.take() {
            join.abort();
        }
    }
}

impl Debug for RuntimeServingControllerSupervisorV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingControllerSupervisorV2(<redacted>)")
    }
}

enum RuntimeServingControllerCommandV2 {
    AcquireSlot {
        slot: RuntimeServingSlotV2,
        reply: oneshot::Sender<RuntimeSlotPermitReplyV2>,
    },
    PauseBarrierA {
        command: RuntimeControllerBarrierAPauseCommandV2,
        reply: oneshot::Sender<RuntimeBarrierAPauseReplyV2>,
    },
    ResumeBarrierA {
        command: RuntimeControllerBarrierAResumeCommandV2,
        reply: oneshot::Sender<RuntimeBarrierAResumeReplyV2>,
    },
    Certification {
        handoff: Box<RuntimeControllerCertificationHandoffV2>,
        reply: oneshot::Sender<RuntimeControllerCertificationHandoffReplyV2>,
    },
}

struct RuntimeServingControllerActorV2 {
    database: RuntimeControllerDatabaseV2,
    discord: RuntimeControllerDiscordPreflightPortV2,
    registry: RuntimeRegistryStagingPortV2,
    controller_id: ControllerId,
    shutdown: RuntimeProcessShutdownTriggerV1,
    config: RuntimeControllerConfigV1,
    events: mpsc::Sender<RuntimeServingControllerCommandV2>,
    stop: watch::Receiver<bool>,
}

impl RuntimeServingControllerActorV2 {
    async fn run_v2(mut self) -> RuntimeServingControllerActorExitV2 {
        loop {
            let claim = runtime_operation_until_stop_v2(
                &mut self.stop,
                self.database
                    .execution()
                    .claim_next_execution(RuntimeClaimNextExecutionV1 {
                        controller_id: self.controller_id.clone(),
                        lease_for: self.config.controller_lease_for,
                    }),
            )
            .await;
            let receipt = match claim {
                Err(()) => return RuntimeServingControllerActorExitV2::Commanded,
                Ok(Ok(Some(receipt))) => receipt,
                Ok(Ok(None)) => {
                    if runtime_wait_until_stop_v2(
                        &mut self.stop,
                        RUNTIME_CONTROLLER_IDLE_BACKOFF_V2,
                    )
                    .await
                    {
                        return RuntimeServingControllerActorExitV2::Commanded;
                    }
                    continue;
                }
                Ok(Err(error)) if retryable_execution_error_v2(error) => {
                    if runtime_wait_until_stop_v2(
                        &mut self.stop,
                        RUNTIME_CONTROLLER_RETRY_BACKOFF_V2,
                    )
                    .await
                    {
                        return RuntimeServingControllerActorExitV2::Commanded;
                    }
                    continue;
                }
                Ok(Err(_)) => return RuntimeServingControllerActorExitV2::Failed,
            };
            let mut retained_receipt = receipt;
            loop {
                let receipt = match self.prepare_claim_v2(retained_receipt).await {
                    RuntimeControllerPreparedClaimV2::Ready(receipt) => *receipt,
                    RuntimeControllerPreparedClaimV2::Retry => {
                        if runtime_wait_until_stop_v2(
                            &mut self.stop,
                            RUNTIME_CONTROLLER_RETRY_BACKOFF_V2,
                        )
                        .await
                        {
                            return RuntimeServingControllerActorExitV2::Commanded;
                        }
                        break;
                    }
                    RuntimeControllerPreparedClaimV2::Commanded => {
                        return RuntimeServingControllerActorExitV2::Commanded;
                    }
                    RuntimeControllerPreparedClaimV2::Failed => {
                        return RuntimeServingControllerActorExitV2::Failed;
                    }
                };
                match self.converge_claim_v2(receipt).await {
                    RuntimeControllerAttemptV2::Drained(drained) => {
                        let held = match self.advance_drained_v2(*drained).await {
                            RuntimeHeldAdvanceOutcomeV2::Ready(held) => *held,
                            RuntimeHeldAdvanceOutcomeV2::Retry => {
                                if runtime_wait_until_stop_v2(
                                    &mut self.stop,
                                    RUNTIME_CONTROLLER_RETRY_BACKOFF_V2,
                                )
                                .await
                                {
                                    return RuntimeServingControllerActorExitV2::Commanded;
                                }
                                break;
                            }
                            RuntimeHeldAdvanceOutcomeV2::Continue => break,
                            RuntimeHeldAdvanceOutcomeV2::Exit(exit) => return exit,
                        };
                        match self.handoff_certification_v2(held).await {
                            RuntimeHeldRouteOutcomeV2::Continue => break,
                            RuntimeHeldRouteOutcomeV2::Retry => {
                                if runtime_wait_until_stop_v2(
                                    &mut self.stop,
                                    RUNTIME_CONTROLLER_RETRY_BACKOFF_V2,
                                )
                                .await
                                {
                                    return RuntimeServingControllerActorExitV2::Commanded;
                                }
                                break;
                            }
                            RuntimeHeldRouteOutcomeV2::Exit(exit) => return exit,
                        }
                    }
                    RuntimeControllerAttemptV2::RetryRetained(receipt) => {
                        if runtime_wait_until_stop_v2(
                            &mut self.stop,
                            RUNTIME_CONTROLLER_RETRY_BACKOFF_V2,
                        )
                        .await
                        {
                            return RuntimeServingControllerActorExitV2::Commanded;
                        }
                        retained_receipt = *receipt;
                    }
                    RuntimeControllerAttemptV2::Retry => {
                        if runtime_wait_until_stop_v2(
                            &mut self.stop,
                            RUNTIME_CONTROLLER_RETRY_BACKOFF_V2,
                        )
                        .await
                        {
                            return RuntimeServingControllerActorExitV2::Commanded;
                        }
                        break;
                    }
                    RuntimeControllerAttemptV2::Commanded => {
                        return RuntimeServingControllerActorExitV2::Commanded;
                    }
                    RuntimeControllerAttemptV2::Failed => {
                        return RuntimeServingControllerActorExitV2::Failed;
                    }
                }
            }
        }
    }

    async fn prepare_claim_v2(
        &mut self,
        receipt: RuntimeExecutionReceiptV1,
    ) -> RuntimeControllerPreparedClaimV2 {
        let now = Utc::now();
        let action = match plan_runtime_action_v1(
            &receipt.snapshot,
            &receipt.controller_id,
            now,
            &self.config,
        ) {
            Ok(action) => action,
            Err(_) => return RuntimeControllerPreparedClaimV2::Retry,
        };
        let lease_for = match action {
            RuntimeControllerActionV1::RenewControllerLease { lease_for } => lease_for,
            RuntimeControllerActionV1::VerifyPreflight { .. }
            | RuntimeControllerActionV1::RequestDrain
            | RuntimeControllerActionV1::DrainPreviousRuntime { .. }
            | RuntimeControllerActionV1::BeginActivation
            | RuntimeControllerActionV1::VerifyActiveTarget { .. }
            | RuntimeControllerActionV1::BeginPanelReconciliation
            | RuntimeControllerActionV1::ReconcilePanels { .. }
            | RuntimeControllerActionV1::StartGatewayAndCertifyLive { .. }
                if runtime_claim_requires_renewal_v2(receipt.expires_at, &self.config, now) =>
            {
                self.config.controller_lease_for
            }
            _ => {
                return RuntimeControllerPreparedClaimV2::Ready(Box::new(receipt));
            }
        };
        if lease_for.is_zero() {
            return RuntimeControllerPreparedClaimV2::Failed;
        }
        let mut session = match RuntimeConvergenceSessionV1::from_claim(receipt) {
            Ok(session) => session,
            Err(_) => return RuntimeControllerPreparedClaimV2::Failed,
        };
        let renewal = match session.begin_renewal(lease_for) {
            Ok(renewal) => renewal,
            Err(_) => return RuntimeControllerPreparedClaimV2::Failed,
        };
        let renewal = match runtime_operation_until_stop_v2(
            &mut self.stop,
            self.database.execution().renew_execution(renewal),
        )
        .await
        {
            Err(()) => return RuntimeControllerPreparedClaimV2::Commanded,
            Ok(Ok(renewal)) => renewal,
            Ok(Err(error)) if retryable_execution_error_v2(error) => {
                return RuntimeControllerPreparedClaimV2::Retry;
            }
            Ok(Err(_)) => return RuntimeControllerPreparedClaimV2::Failed,
        };
        if session.apply_renewal(renewal).is_err() {
            return RuntimeControllerPreparedClaimV2::Failed;
        }
        match session.current_execution_receipt() {
            Ok(receipt) => RuntimeControllerPreparedClaimV2::Ready(Box::new(receipt)),
            Err(_) => RuntimeControllerPreparedClaimV2::Failed,
        }
    }

    async fn converge_claim_v2(
        &mut self,
        receipt: RuntimeExecutionReceiptV1,
    ) -> RuntimeControllerAttemptV2 {
        let preflight_failure_receipt = receipt.clone();
        let slot = RuntimeServingSlotV2::from_target(&receipt.snapshot.target);
        let (reply_tx, reply_rx) = oneshot::channel();
        let command = RuntimeServingControllerCommandV2::AcquireSlot {
            slot,
            reply: reply_tx,
        };
        match runtime_operation_until_stop_v2(&mut self.stop, self.events.send(command)).await {
            Err(()) => return RuntimeControllerAttemptV2::Commanded,
            Ok(Err(_)) => return RuntimeControllerAttemptV2::Failed,
            Ok(Ok(())) => {}
        }
        let permit = match runtime_operation_until_stop_v2(&mut self.stop, reply_rx).await {
            Err(()) => return RuntimeControllerAttemptV2::Commanded,
            Ok(Ok(Ok(permit))) => permit,
            Ok(Ok(Err(RuntimeServingSlotWorkErrorV2::SupervisorSealed))) => {
                return RuntimeControllerAttemptV2::Commanded;
            }
            Ok(Ok(Err(_))) | Ok(Err(_)) => return RuntimeControllerAttemptV2::Failed,
        };
        let claimed = match RuntimeClaimedConvergenceV2::from_claim(
            receipt,
            permit,
            Utc::now(),
            self.config.clone(),
        ) {
            Ok(claimed) => claimed,
            Err(automation_runtime_worker::RuntimeConvergenceStartErrorV2::RenewalRequired) => {
                return RuntimeControllerAttemptV2::RetryRetained(Box::new(
                    preflight_failure_receipt,
                ));
            }
            Err(_) => return RuntimeControllerAttemptV2::Failed,
        };
        let hydration = match claimed.begin_hydration() {
            Ok(hydration) => hydration,
            Err(_) => return RuntimeControllerAttemptV2::Failed,
        };
        let hydrated = match runtime_operation_until_stop_v2(
            &mut self.stop,
            hydration.execute(&self.database),
        )
        .await
        {
            Err(()) => return RuntimeControllerAttemptV2::Commanded,
            Ok(Ok(hydrated)) => hydrated,
            Ok(Err(RuntimeExactTargetHydrationErrorV2::Port(error)))
                if retryable_hydration_error_v2(&error) =>
            {
                return RuntimeControllerAttemptV2::RetryRetained(Box::new(
                    preflight_failure_receipt,
                ));
            }
            Ok(Err(_)) => return RuntimeControllerAttemptV2::Failed,
        };
        let preflight = match hydrated.begin_discord_preflight(Utc::now()) {
            Ok(preflight) => preflight,
            Err(RuntimeDiscordPreflightErrorV2::InvalidTimeWindow) => {
                return RuntimeControllerAttemptV2::RetryRetained(Box::new(
                    preflight_failure_receipt,
                ));
            }
            Err(_) => return RuntimeControllerAttemptV2::Failed,
        };
        let preflight =
            match runtime_operation_until_stop_v2(&mut self.stop, preflight.execute(&self.discord))
                .await
            {
                Err(()) => return RuntimeControllerAttemptV2::Commanded,
                Ok(Ok(preflight)) => preflight,
                Ok(Err(RuntimeDiscordPreflightErrorV2::Port(error))) => {
                    return match error.disposition_v2() {
                        RuntimeControllerDiscordPreflightDispositionV2::Retryable => {
                            RuntimeControllerAttemptV2::RetryRetained(Box::new(
                                preflight_failure_receipt,
                            ))
                        }
                        RuntimeControllerDiscordPreflightDispositionV2::DeploymentBlocked => {
                            if runtime_phase_allows_preflight_cancel_v2(
                                &preflight_failure_receipt.snapshot.phase,
                            ) {
                                self.cancel_blocked_preflight_v2(preflight_failure_receipt)
                                    .await
                            } else {
                                RuntimeControllerAttemptV2::Failed
                            }
                        }
                        RuntimeControllerDiscordPreflightDispositionV2::ProcessFatal => {
                            RuntimeControllerAttemptV2::Failed
                        }
                    };
                }
                Ok(Err(_)) => return RuntimeControllerAttemptV2::Failed,
            };
        let stage_ready = match preflight {
            RuntimeDiscordPreflightOutcomeV2::StageReady(stage_ready) => *stage_ready,
            RuntimeDiscordPreflightOutcomeV2::AcceptPreflight(preflighted) => {
                let mutation = match preflighted.begin_accept_preflight() {
                    Ok(mutation) => mutation,
                    Err(_) => return RuntimeControllerAttemptV2::Failed,
                };
                match runtime_operation_until_stop_v2(
                    &mut self.stop,
                    mutation.execute(&self.database, Utc::now()),
                )
                .await
                {
                    Err(()) => return RuntimeControllerAttemptV2::Commanded,
                    Ok(Ok(stage_ready)) => stage_ready,
                    Ok(Err(RuntimeAcceptPreflightMutationErrorV2::Port(error)))
                        if retryable_execution_error_v2(error) =>
                    {
                        return RuntimeControllerAttemptV2::Retry;
                    }
                    Ok(Err(RuntimeAcceptPreflightMutationErrorV2::RenewalRequired))
                    | Ok(Err(RuntimeAcceptPreflightMutationErrorV2::InvalidCompletionTime)) => {
                        return RuntimeControllerAttemptV2::Retry
                    }
                    Ok(Err(_)) => return RuntimeControllerAttemptV2::Failed,
                }
            }
        };
        let refresh = match stage_ready.begin_exact_hydration_refresh() {
            Ok(refresh) => refresh,
            Err(_) => return RuntimeControllerAttemptV2::Failed,
        };
        let stage_ready =
            match runtime_operation_until_stop_v2(&mut self.stop, refresh.execute(&self.database))
                .await
            {
                Err(()) => return RuntimeControllerAttemptV2::Commanded,
                Ok(Ok(stage_ready)) => stage_ready,
                Ok(Err(RuntimeExactTargetHydrationErrorV2::Port(error)))
                    if retryable_hydration_error_v2(&error) =>
                {
                    return RuntimeControllerAttemptV2::Retry;
                }
                Ok(Err(_)) => return RuntimeControllerAttemptV2::Failed,
            };
        let stage = RuntimeControllerStagePortV2 {
            registry: self.registry.clone(),
            emergency: RuntimeRegistryEmergencyTriggerV2::new({
                let shutdown = self.shutdown.clone();
                move || {
                    shutdown.trip(RuntimeShutdownCauseV1::SupervisorFailure);
                }
            }),
        };
        let staged = match stage_ready.stage(&stage, Utc::now()) {
            Ok(staged) => staged,
            Err(_) => return RuntimeControllerAttemptV2::Failed,
        };
        match staged.into_staged_recovery() {
            Ok(recovery) => match RuntimeHeldDrainedRouteV2::from_recovery(recovery) {
                Ok(held) => RuntimeControllerAttemptV2::Drained(Box::new(held)),
                Err(_) => RuntimeControllerAttemptV2::Failed,
            },
            Err(staged) => self.replace_staged_v2(*staged).await,
        }
    }

    async fn replace_staged_v2(
        &mut self,
        staged: RuntimeStagedConvergenceV2<RuntimeExactTargetV1, RuntimeRegistryReplacementRouteV2>,
    ) -> RuntimeControllerAttemptV2 {
        let drain_requested = match &staged.hydrated().snapshot.phase {
            RuntimeDeploymentPhaseV1::PreflightReady => {
                let request_drain = match staged.begin_request_drain() {
                    Ok(request) => request,
                    Err(_) => return RuntimeControllerAttemptV2::Failed,
                };
                match runtime_operation_until_stop_v2(
                    &mut self.stop,
                    request_drain.execute(&self.database, Utc::now()),
                )
                .await
                {
                    Err(()) => return RuntimeControllerAttemptV2::Commanded,
                    Ok(Ok(drain_requested)) => drain_requested,
                    Ok(Err(RuntimeReplacementExecutionErrorV2::Port { source, .. }))
                        if retryable_execution_error_v2(source) =>
                    {
                        return RuntimeControllerAttemptV2::Retry;
                    }
                    Ok(Err(_)) => return RuntimeControllerAttemptV2::Failed,
                }
            }
            RuntimeDeploymentPhaseV1::DrainRequested => {
                match staged.resume_drain_requested(Utc::now()) {
                    Ok(drain_requested) => drain_requested,
                    Err(
                        automation_runtime_worker::RuntimeRequestDrainMutationErrorV2::RenewalRequired
                        | automation_runtime_worker::RuntimeRequestDrainMutationErrorV2::InvalidCompletionTime,
                    ) => return RuntimeControllerAttemptV2::Retry,
                    Err(_) => return RuntimeControllerAttemptV2::Failed,
                }
            }
            _ => return RuntimeControllerAttemptV2::Failed,
        };
        let previous = match drain_requested.begin_previous_serving_observation() {
            Ok(previous) => previous,
            Err(_) => return RuntimeControllerAttemptV2::Failed,
        };
        let observed =
            match runtime_operation_until_stop_v2(&mut self.stop, previous.execute(&self.database))
                .await
            {
                Err(()) => return RuntimeControllerAttemptV2::Commanded,
                Ok(Ok(observed)) => observed,
                Ok(Err(RuntimeReplacementExecutionErrorV2::Port { source, .. }))
                    if retryable_execution_error_v2(source) =>
                {
                    return RuntimeControllerAttemptV2::Retry;
                }
                Ok(Err(RuntimeReplacementExecutionErrorV2::Failed(
                    RuntimeExactPreviousServingErrorV2::Evidence(
                        RuntimeExactPreviousServingEvidenceErrorV2::FreshForeignPredecessor,
                    ),
                ))) => return RuntimeControllerAttemptV2::Retry,
                Ok(Err(_)) => return RuntimeControllerAttemptV2::Failed,
            };
        let pause = match observed.begin_barrier_a_pause(Utc::now()) {
            Ok(pause) => pause,
            Err(_) => return RuntimeControllerAttemptV2::Retry,
        };
        let barrier = RuntimeControllerBarrierPortV2 {
            events: self.events.clone(),
        };
        let paused =
            match runtime_operation_until_stop_v2(&mut self.stop, pause.execute(&barrier)).await {
                Err(()) => return RuntimeControllerAttemptV2::Commanded,
                Ok(Ok(paused)) => paused,
                Ok(Err(RuntimeReplacementExecutionErrorV2::Port {
                    source:
                        RuntimeControllerBarrierBridgeErrorV2::DeadlineElapsed
                        | RuntimeControllerBarrierBridgeErrorV2::StaleAuthority,
                    ..
                })) => {
                    return RuntimeControllerAttemptV2::Retry;
                }
                Ok(Err(_)) => return RuntimeControllerAttemptV2::Failed,
            };
        let predecessor = RuntimeControllerPredecessorPortV2 {
            database: self.database.clone(),
        };
        let draining = match paused.begin_predecessor_transition().execute(&predecessor) {
            Ok(draining) => draining,
            Err(_) => return RuntimeControllerAttemptV2::Failed,
        };
        let resume = draining.begin_barrier_a_resume();
        let resumed =
            match runtime_operation_until_stop_v2(&mut self.stop, resume.execute(&barrier)).await {
                Err(()) => return RuntimeControllerAttemptV2::Commanded,
                Ok(Ok(resumed)) => resumed,
                Ok(Err(_)) => return RuntimeControllerAttemptV2::Failed,
            };
        let retirement_ready = match resumed.begin_previous_serving_disconnect() {
            Ok(RuntimePreviousServingDisconnectOutcomeV2::NotRequired(ready)) => ready,
            Ok(RuntimePreviousServingDisconnectOutcomeV2::Required(disconnect)) => {
                match runtime_operation_until_stop_v2(
                    &mut self.stop,
                    disconnect.execute(&self.database),
                )
                .await
                {
                    Err(()) => return RuntimeControllerAttemptV2::Commanded,
                    Ok(Ok(ready)) => ready,
                    Ok(Err(RuntimeReplacementExecutionErrorV2::Port { .. })) => {
                        return RuntimeControllerAttemptV2::Retry;
                    }
                    Ok(Err(_)) => return RuntimeControllerAttemptV2::Failed,
                }
            }
            Err(_) => return RuntimeControllerAttemptV2::Failed,
        };
        let retirement = match retirement_ready.begin_predecessor_retirement() {
            Ok(retirement) => retirement,
            Err(_) => return RuntimeControllerAttemptV2::Failed,
        };
        let removed =
            match runtime_operation_until_stop_v2(&mut self.stop, retirement.execute(&predecessor))
                .await
            {
                Err(()) => return RuntimeControllerAttemptV2::Commanded,
                Ok(Ok(removed)) => removed,
                Ok(Err(RuntimeReplacementExecutionErrorV2::Port {
                    source: RuntimeControllerPredecessorPortErrorV2::Database(error),
                    ..
                })) if retryable_execution_error_v2(error) => {
                    return RuntimeControllerAttemptV2::Retry;
                }
                Ok(Err(RuntimeReplacementExecutionErrorV2::Port {
                    source: RuntimeControllerPredecessorPortErrorV2::DeadlineElapsed,
                    ..
                })) => return RuntimeControllerAttemptV2::Retry,
                Ok(Err(_)) => return RuntimeControllerAttemptV2::Failed,
            };
        let accept = match removed.begin_accept_drain() {
            Ok(accept) => accept,
            Err(_) => return RuntimeControllerAttemptV2::Failed,
        };
        let drained = match runtime_operation_until_stop_v2(
            &mut self.stop,
            accept.execute(&self.database, Utc::now()),
        )
        .await
        {
            Err(()) => return RuntimeControllerAttemptV2::Commanded,
            Ok(Ok(drained)) => drained,
            Ok(Err(RuntimeReplacementExecutionErrorV2::Port { source, .. }))
                if retryable_execution_error_v2(source) =>
            {
                return RuntimeControllerAttemptV2::Retry;
            }
            Ok(Err(_)) => return RuntimeControllerAttemptV2::Failed,
        };
        RuntimeControllerAttemptV2::Drained(Box::new(RuntimeHeldDrainedRouteV2::from_convergence(
            drained,
        )))
    }

    async fn cancel_blocked_preflight_v2(
        &mut self,
        receipt: RuntimeExecutionReceiptV1,
    ) -> RuntimeControllerAttemptV2 {
        let mut session = match RuntimeConvergenceSessionV1::from_claim(receipt) {
            Ok(session) => session,
            Err(_) => return RuntimeControllerAttemptV2::Failed,
        };
        let mutation =
            match session.begin_mutation(runtime_discord_preflight_blocking_mutation_v2()) {
                Ok(mutation) => mutation,
                Err(_) => return RuntimeControllerAttemptV2::Failed,
            };
        let receipt = match runtime_operation_until_stop_v2(
            &mut self.stop,
            self.database.execution().mutate(mutation),
        )
        .await
        {
            Err(()) => return RuntimeControllerAttemptV2::Commanded,
            Ok(Ok(receipt)) => receipt,
            Ok(Err(error)) if retryable_execution_error_v2(error) => {
                return RuntimeControllerAttemptV2::Retry;
            }
            Ok(Err(_)) => return RuntimeControllerAttemptV2::Failed,
        };
        match session.apply_mutation(receipt) {
            Ok(RuntimeConvergenceSessionStateV1::Released) => RuntimeControllerAttemptV2::Retry,
            Ok(
                RuntimeConvergenceSessionStateV1::Active
                | RuntimeConvergenceSessionStateV1::CertifiedLive,
            )
            | Err(_) => RuntimeControllerAttemptV2::Failed,
        }
    }

    async fn advance_drained_v2(
        &mut self,
        mut held: RuntimeHeldDrainedRouteV2,
    ) -> RuntimeHeldAdvanceOutcomeV2 {
        if held.ensure_active_v2().is_err() {
            return held.finish_advance_v2(RuntimeHeldPostCleanupOutcomeV2::Retry);
        }
        if let Err(failure) = self.renew_held_v2(&mut held).await {
            return held.finish_advance_v2(failure.into());
        }
        let mut panel_observation = RuntimePanelObservationBudgetV2::default();
        loop {
            let Some(continuation) = runtime_held_continuation_v2(&held.session.snapshot().phase)
            else {
                return held.finish_advance_v2(RuntimeHeldPostCleanupOutcomeV2::Exit(
                    RuntimeServingControllerActorExitV2::Failed,
                ));
            };
            let result = match continuation {
                RuntimeHeldContinuationV2::BeginActivation => self
                    .mutate_held_v2(&mut held, RuntimeConvergenceMutationV1::BeginActivation)
                    .await
                    .map_err(RuntimeHeldStepFailureV2::Advance),
                RuntimeHeldContinuationV2::AcceptActivation => self
                    .accept_held_activation_v2(&mut held)
                    .await
                    .map_err(RuntimeHeldStepFailureV2::Advance),
                RuntimeHeldContinuationV2::BeginPanelReconciliation => self
                    .mutate_held_v2(
                        &mut held,
                        RuntimeConvergenceMutationV1::BeginPanelReconciliation,
                    )
                    .await
                    .map_err(RuntimeHeldStepFailureV2::Advance),
                RuntimeHeldContinuationV2::ReconcilePanels => {
                    self.reconcile_held_panels_v2(&mut held).await
                }
                RuntimeHeldContinuationV2::HoldAwaitingGatewayReady => {
                    return if held.ensure_active_v2().is_ok() {
                        RuntimeHeldAdvanceOutcomeV2::Ready(Box::new(held))
                    } else {
                        held.finish_advance_v2(RuntimeHeldPostCleanupOutcomeV2::Retry)
                    };
                }
            };
            match result {
                Ok(()) => {}
                Err(RuntimeHeldStepFailureV2::Advance(failure)) => {
                    return held.finish_advance_v2(failure.into());
                }
                Err(RuntimeHeldStepFailureV2::Panel(error)) => {
                    match panel_observation.next_v2(error.disposition_v2()) {
                        RuntimePanelFailureActionV2::Retry => {
                            return held.finish_advance_v2(RuntimeHeldPostCleanupOutcomeV2::Retry);
                        }
                        RuntimePanelFailureActionV2::ObserveJournal => {
                            if let Err(failure) = self.renew_held_v2(&mut held).await {
                                return held.finish_advance_v2(failure.into());
                            }
                        }
                        RuntimePanelFailureActionV2::BlockDeployment => {
                            let code = error.code();
                            return self.block_and_finish_held_panel_v2(held, code).await;
                        }
                        RuntimePanelFailureActionV2::FailProcess => {
                            return held.finish_advance_v2(RuntimeHeldPostCleanupOutcomeV2::Exit(
                                RuntimeServingControllerActorExitV2::Failed,
                            ));
                        }
                    }
                }
            }
        }
    }

    async fn accept_held_activation_v2(
        &mut self,
        held: &mut RuntimeHeldDrainedRouteV2,
    ) -> Result<(), RuntimeHeldAdvanceFailureV2> {
        let activation_execution = match held.session.current_execution_receipt() {
            Ok(receipt) => receipt,
            Err(_) => return Err(RuntimeHeldAdvanceFailureV2::Failed),
        };
        let hydrated = match runtime_operation_until_stop_v2(
            &mut self.stop,
            self.database
                .exact_target()
                .load_for_execution(&activation_execution),
        )
        .await
        {
            Err(()) => return Err(RuntimeHeldAdvanceFailureV2::Commanded),
            Ok(Ok(hydrated)) => hydrated,
            Ok(Err(error)) if retryable_hydration_error_v2(&error) => {
                return Err(RuntimeHeldAdvanceFailureV2::Retry);
            }
            Ok(Err(_)) => return Err(RuntimeHeldAdvanceFailureV2::Failed),
        };
        if hydrated.snapshot != activation_execution.snapshot
            || hydrated.snapshot.target != held.witness.identity.target
            || hydrated.snapshot.runtime_generation != held.witness.identity.runtime_generation
        {
            return Err(RuntimeHeldAdvanceFailureV2::Failed);
        }
        let activation = ActivationAttestationV1 {
            activation_request_id: hydrated.snapshot.identity.activation_request_id.clone(),
            target: hydrated.snapshot.target.clone(),
            runtime_generation: hydrated.snapshot.runtime_generation,
            kind: ActivationOutcomeKindV1::AlreadyActive,
            activated_at: Utc::now().max(hydrated.database_observed_at),
        };
        self.mutate_held_v2(
            held,
            RuntimeConvergenceMutationV1::AcceptActivation(activation),
        )
        .await
    }

    async fn reconcile_held_panels_v2(
        &mut self,
        held: &mut RuntimeHeldDrainedRouteV2,
    ) -> Result<(), RuntimeHeldStepFailureV2> {
        let execution = match held.session.current_execution_receipt() {
            Ok(receipt) => receipt,
            Err(_) => {
                return Err(RuntimeHeldStepFailureV2::Advance(
                    RuntimeHeldAdvanceFailureV2::Failed,
                ));
            }
        };
        let now = Utc::now();
        let operation_budget =
            TimeDelta::from_std(self.config.panel_reconciliation_timeout).unwrap_or(TimeDelta::MAX);
        let side_effect_headroom =
            TimeDelta::from_std(Duration::from_secs(21)).unwrap_or(TimeDelta::MAX);
        let desired_deadline = now.checked_add_signed(operation_budget);
        let latest_deadline = execution
            .expires_at
            .checked_sub_signed(side_effect_headroom);
        let Some(deadline) = desired_deadline
            .zip(latest_deadline)
            .map(
                |(desired, latest)| {
                    if desired < latest {
                        desired
                    } else {
                        latest
                    }
                },
            )
        else {
            return Err(RuntimeHeldStepFailureV2::Advance(
                RuntimeHeldAdvanceFailureV2::Retry,
            ));
        };
        if deadline <= now {
            return Err(RuntimeHeldStepFailureV2::Advance(
                RuntimeHeldAdvanceFailureV2::Retry,
            ));
        }
        let panels = RuntimeExactPanelReconciliationV2::new(
            self.database.clone(),
            self.discord.inner.clone(),
        );
        let request = RuntimeExactPanelReconciliationRequestV2::new(
            execution,
            held.witness.identity.clone(),
            deadline,
        );
        let certificate = match runtime_operation_until_stop_v2(
            &mut self.stop,
            panels.reconcile_exact_v2(request),
        )
        .await
        {
            Err(()) => {
                return Err(RuntimeHeldStepFailureV2::Advance(
                    RuntimeHeldAdvanceFailureV2::Commanded,
                ));
            }
            Ok(Ok(certificate)) => certificate,
            Ok(Err(error)) => return Err(RuntimeHeldStepFailureV2::Panel(error)),
        };
        self.mutate_held_v2(
            held,
            RuntimeConvergenceMutationV1::AcceptPanelCertificate(certificate.certificate().clone()),
        )
        .await
        .map_err(RuntimeHeldStepFailureV2::Advance)
    }

    async fn block_and_finish_held_panel_v2(
        &mut self,
        held: RuntimeHeldDrainedRouteV2,
        code: &'static str,
    ) -> RuntimeHeldAdvanceOutcomeV2 {
        let mut route_removed = match held.remove_staged_v2() {
            Ok(route_removed) => route_removed,
            Err(outcome) => return outcome,
        };
        let outcome = match self
            .block_held_panel_session_v2(&mut route_removed.session, code)
            .await
        {
            Ok(()) => RuntimeHeldPostCleanupOutcomeV2::Continue,
            Err(failure) => failure.into(),
        };
        route_removed.finish_advance_v2(outcome)
    }

    async fn block_held_panel_session_v2(
        &mut self,
        session: &mut RuntimeConvergenceSessionV1,
        code: &'static str,
    ) -> Result<(), RuntimeHeldAdvanceFailureV2> {
        let failure_id = runtime_panel_failure_id_v2(session)
            .map_err(|_| RuntimeHeldAdvanceFailureV2::Failed)?;
        let request = session
            .begin_mutation(RuntimeConvergenceMutationV1::RecordBlockedFailure {
                failure_id,
                kind: RuntimeFailureKindV1::PanelReconciliation,
                code: code.to_owned(),
            })
            .map_err(|_| RuntimeHeldAdvanceFailureV2::Failed)?;
        let receipt = match runtime_operation_until_stop_v2(
            &mut self.stop,
            self.database.execution().mutate(request),
        )
        .await
        {
            Err(()) => return Err(RuntimeHeldAdvanceFailureV2::Commanded),
            Ok(Ok(receipt)) => receipt,
            Ok(Err(error)) => {
                return Err(runtime_panel_block_mutation_failure_v2(error));
            }
        };
        match session.apply_mutation(receipt) {
            Ok(RuntimeConvergenceSessionStateV1::Released) => Ok(()),
            Ok(
                RuntimeConvergenceSessionStateV1::Active
                | RuntimeConvergenceSessionStateV1::CertifiedLive,
            )
            | Err(_) => Err(RuntimeHeldAdvanceFailureV2::Failed),
        }
    }

    async fn renew_held_v2(
        &mut self,
        held: &mut RuntimeHeldDrainedRouteV2,
    ) -> Result<(), RuntimeHeldAdvanceFailureV2> {
        let renewal = held
            .session
            .begin_renewal(self.config.controller_lease_for)
            .map_err(|_| RuntimeHeldAdvanceFailureV2::Failed)?;
        let renewal = match runtime_operation_until_stop_v2(
            &mut self.stop,
            self.database.execution().renew_execution(renewal),
        )
        .await
        {
            Err(()) => return Err(RuntimeHeldAdvanceFailureV2::Commanded),
            Ok(Ok(renewal)) => renewal,
            Ok(Err(error)) if retryable_execution_error_v2(error) => {
                return Err(RuntimeHeldAdvanceFailureV2::Retry);
            }
            Ok(Err(_)) => return Err(RuntimeHeldAdvanceFailureV2::Failed),
        };
        held.session
            .apply_renewal(renewal)
            .map_err(|_| RuntimeHeldAdvanceFailureV2::Failed)?;
        let evidence = held
            .staged
            .advance_authority_v2(held.session.fencing_token())
            .map_err(|_| RuntimeHeldAdvanceFailureV2::Failed)?;
        held.apply_staged_evidence_v2(&evidence);
        held.ensure_active_v2()
            .map_err(|_| RuntimeHeldAdvanceFailureV2::Failed)
    }

    async fn mutate_held_v2(
        &mut self,
        held: &mut RuntimeHeldDrainedRouteV2,
        mutation: RuntimeConvergenceMutationV1,
    ) -> Result<(), RuntimeHeldAdvanceFailureV2> {
        let request = held
            .session
            .begin_mutation(mutation)
            .map_err(|_| RuntimeHeldAdvanceFailureV2::Failed)?;
        let receipt = match runtime_operation_until_stop_v2(
            &mut self.stop,
            self.database.execution().mutate(request),
        )
        .await
        {
            Err(()) => return Err(RuntimeHeldAdvanceFailureV2::Commanded),
            Ok(Ok(receipt)) => receipt,
            Ok(Err(error)) if retryable_execution_error_v2(error) => {
                return Err(RuntimeHeldAdvanceFailureV2::Retry);
            }
            Ok(Err(_)) => return Err(RuntimeHeldAdvanceFailureV2::Failed),
        };
        match held.session.apply_mutation(receipt) {
            Ok(RuntimeConvergenceSessionStateV1::Active) => {}
            Ok(
                RuntimeConvergenceSessionStateV1::Released
                | RuntimeConvergenceSessionStateV1::CertifiedLive,
            )
            | Err(_) => return Err(RuntimeHeldAdvanceFailureV2::Failed),
        }
        held.ensure_active_v2()
            .map_err(|_| RuntimeHeldAdvanceFailureV2::Failed)
    }

    async fn handoff_certification_v2(
        &mut self,
        held: RuntimeHeldDrainedRouteV2,
    ) -> RuntimeHeldRouteOutcomeV2 {
        let acceptance_deadline = match runtime_certification_acceptance_deadline_v2(
            held.session.expires_at(),
            &self.config,
            Utc::now(),
        ) {
            Some(deadline) => deadline,
            None => return held.finish_retry_v2(),
        };
        let handoff = match RuntimeControllerCertificationHandoffV2::from_ready_held_v2(
            Box::new(held),
            self.config.serving_lease_for,
            acceptance_deadline,
        ) {
            Ok(handoff) => handoff,
            Err(held) => {
                return RuntimeHeldRouteOutcomeV2::Exit(
                    held.finish_v2(RuntimeServingControllerActorExitV2::Failed),
                );
            }
        };
        let expected =
            match RuntimeControllerCertificationReceiptExpectationV2::from_handoff_v2(&handoff) {
                Ok(expected) => expected,
                Err(()) => {
                    return RuntimeHeldRouteOutcomeV2::Exit(
                        handoff
                            .into_held_v2()
                            .finish_v2(RuntimeServingControllerActorExitV2::Failed),
                    );
                }
            };
        let deadline = runtime_datetime_deadline_v2(acceptance_deadline);
        let permit = tokio::select! {
            biased;
            _ = runtime_wait_for_stop_v2(&mut self.stop) => {
                return RuntimeHeldRouteOutcomeV2::Exit(
                    handoff
                        .into_held_v2()
                        .finish_v2(RuntimeServingControllerActorExitV2::Commanded),
                );
            }
            _ = sleep_until(deadline) => {
                return handoff
                    .into_held_v2()
                    .finish_retry_v2();
            }
            permit = self.events.reserve() => match permit {
                Ok(permit) => permit,
                Err(_) => {
                    self.shutdown.trip(RuntimeShutdownCauseV1::SupervisorFailure);
                    return RuntimeHeldRouteOutcomeV2::Exit(
                        handoff
                            .into_held_v2()
                            .finish_v2(RuntimeServingControllerActorExitV2::Failed),
                    );
                }
            },
        };
        let (reply, response) = oneshot::channel();
        permit.send(RuntimeServingControllerCommandV2::Certification { handoff, reply });
        let response = tokio::select! {
            biased;
            _ = runtime_wait_for_stop_v2(&mut self.stop) => None,
            _ = sleep_until(deadline) => None,
            response = response => response.ok(),
        };
        let Some(response) = response else {
            self.shutdown
                .trip(RuntimeShutdownCauseV1::SupervisorFailure);
            return RuntimeHeldRouteOutcomeV2::Exit(RuntimeServingControllerActorExitV2::Failed);
        };
        match response.into_controller_outcome_v2() {
            Ok(serving) if expected.matches_v2(&serving) => RuntimeHeldRouteOutcomeV2::Continue,
            Ok(_) => {
                self.shutdown
                    .trip(RuntimeShutdownCauseV1::SupervisorFailure);
                RuntimeHeldRouteOutcomeV2::Exit(RuntimeServingControllerActorExitV2::Failed)
            }
            Err(rejected) => {
                let (held, source) = *rejected;
                if held.ensure_active_v2().is_err() {
                    return RuntimeHeldRouteOutcomeV2::Exit(
                        held.finish_v2(RuntimeServingControllerActorExitV2::Failed),
                    );
                }
                match source {
                    RuntimeControllerCertificationHandoffRejectionV2::Unavailable
                    | RuntimeControllerCertificationHandoffRejectionV2::DeadlineElapsed => {
                        held.finish_retry_v2()
                    }
                    RuntimeControllerCertificationHandoffRejectionV2::StaleAuthority => {
                        RuntimeHeldRouteOutcomeV2::Exit(
                            held.finish_v2(RuntimeServingControllerActorExitV2::Failed),
                        )
                    }
                }
            }
        }
    }
}

enum RuntimeControllerAttemptV2 {
    Drained(Box<RuntimeHeldDrainedRouteV2>),
    RetryRetained(Box<RuntimeExecutionReceiptV1>),
    Retry,
    Commanded,
    Failed,
}

enum RuntimeControllerPreparedClaimV2 {
    Ready(Box<RuntimeExecutionReceiptV1>),
    Retry,
    Commanded,
    Failed,
}

enum RuntimeHeldRouteOutcomeV2 {
    Continue,
    Retry,
    Exit(RuntimeServingControllerActorExitV2),
}

enum RuntimeHeldAdvanceOutcomeV2 {
    Ready(Box<RuntimeHeldDrainedRouteV2>),
    Retry,
    Continue,
    Exit(RuntimeServingControllerActorExitV2),
}

enum RuntimeHeldAdvanceFailureV2 {
    Retry,
    Commanded,
    Failed,
}

enum RuntimeHeldStepFailureV2 {
    Advance(RuntimeHeldAdvanceFailureV2),
    Panel(RuntimeExactPanelReconciliationErrorV2),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeHeldPostCleanupOutcomeV2 {
    Retry,
    Continue,
    Exit(RuntimeServingControllerActorExitV2),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimePanelFailureActionV2 {
    Retry,
    ObserveJournal,
    BlockDeployment,
    FailProcess,
}

#[derive(Default)]
struct RuntimePanelObservationBudgetV2 {
    observation_consumed: bool,
}

impl RuntimePanelObservationBudgetV2 {
    fn next_v2(
        &mut self,
        disposition: RuntimeExactPanelReconciliationDispositionV2,
    ) -> RuntimePanelFailureActionV2 {
        match disposition {
            RuntimeExactPanelReconciliationDispositionV2::RetryableInfrastructure => {
                RuntimePanelFailureActionV2::Retry
            }
            RuntimeExactPanelReconciliationDispositionV2::ObservationRequired
                if !self.observation_consumed =>
            {
                self.observation_consumed = true;
                RuntimePanelFailureActionV2::ObserveJournal
            }
            RuntimeExactPanelReconciliationDispositionV2::ObservationRequired
            | RuntimeExactPanelReconciliationDispositionV2::DeploymentBlocked
            | RuntimeExactPanelReconciliationDispositionV2::AuthorityDrift => {
                RuntimePanelFailureActionV2::BlockDeployment
            }
            RuntimeExactPanelReconciliationDispositionV2::ProcessInvariant => {
                RuntimePanelFailureActionV2::FailProcess
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeHeldContinuationV2 {
    BeginActivation,
    AcceptActivation,
    BeginPanelReconciliation,
    ReconcilePanels,
    HoldAwaitingGatewayReady,
}

fn runtime_held_continuation_v2(
    phase: &RuntimeDeploymentPhaseV1,
) -> Option<RuntimeHeldContinuationV2> {
    match phase {
        RuntimeDeploymentPhaseV1::Drained => Some(RuntimeHeldContinuationV2::BeginActivation),
        RuntimeDeploymentPhaseV1::ActivationApplying => {
            Some(RuntimeHeldContinuationV2::AcceptActivation)
        }
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Ready,
        } => Some(RuntimeHeldContinuationV2::BeginPanelReconciliation),
        RuntimeDeploymentPhaseV1::ReconcilingPanels => {
            Some(RuntimeHeldContinuationV2::ReconcilePanels)
        }
        RuntimeDeploymentPhaseV1::AwaitingGatewayReady => {
            Some(RuntimeHeldContinuationV2::HoldAwaitingGatewayReady)
        }
        RuntimeDeploymentPhaseV1::Requested
        | RuntimeDeploymentPhaseV1::PreflightReady
        | RuntimeDeploymentPhaseV1::DrainRequested
        | RuntimeDeploymentPhaseV1::RuntimePending { .. }
        | RuntimeDeploymentPhaseV1::Live
        | RuntimeDeploymentPhaseV1::Superseded { .. }
        | RuntimeDeploymentPhaseV1::Cancelled { .. } => None,
    }
}

impl From<RuntimeHeldAdvanceFailureV2> for RuntimeHeldPostCleanupOutcomeV2 {
    fn from(value: RuntimeHeldAdvanceFailureV2) -> Self {
        match value {
            RuntimeHeldAdvanceFailureV2::Retry => Self::Retry,
            RuntimeHeldAdvanceFailureV2::Commanded => {
                Self::Exit(RuntimeServingControllerActorExitV2::Commanded)
            }
            RuntimeHeldAdvanceFailureV2::Failed => {
                Self::Exit(RuntimeServingControllerActorExitV2::Failed)
            }
        }
    }
}

pub(crate) struct RuntimeHeldDrainedRouteV2 {
    staged: RuntimeRegistryReplacementRouteV2,
    session: RuntimeConvergenceSessionV1,
    permit: RuntimeServingSlotWorkPermitV2,
    evidence: RuntimeExactTargetEvidenceV2,
    hydrated: RuntimeExactTargetV1,
    witness: RuntimeRouteWitnessV2,
}

#[must_use]
#[allow(dead_code)]
pub(crate) struct RuntimeControllerCertificationHandoffV2 {
    held: RuntimeHeldDrainedRouteV2,
    serving_lease_for: Duration,
    acceptance_deadline: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[allow(dead_code)]
pub(crate) enum RuntimeControllerCertificationHandoffRejectionV2 {
    #[error("runtime certification handoff receiver is unavailable")]
    Unavailable,
    #[error("runtime certification handoff acceptance deadline elapsed")]
    DeadlineElapsed,
    #[error("runtime certification handoff authority became stale before acceptance")]
    StaleAuthority,
}

#[must_use]
#[allow(dead_code)]
pub(crate) enum RuntimeControllerCertificationHandoffReplyV2 {
    Rejected {
        handoff: Box<RuntimeControllerCertificationHandoffV2>,
        source: RuntimeControllerCertificationHandoffRejectionV2,
    },
    Accepted(Box<RuntimeServingReceiptV2>),
}

#[allow(dead_code)]
pub(crate) type RuntimeControllerAcceptedCertificationPartsV2 = (
    RuntimeRegistryReplacementRouteV2,
    RuntimeConvergenceSessionV1,
    RuntimeServingSlotWorkPermitV2,
    RuntimeExactTargetEvidenceV2,
    RuntimeExactTargetV1,
    RuntimeRouteWitnessV2,
);

#[allow(dead_code)]
type RuntimeControllerCertificationHandoffPreparationV2 =
    Result<Box<RuntimeControllerCertificationHandoffV2>, Box<RuntimeHeldDrainedRouteV2>>;

#[allow(dead_code)]
type RuntimeControllerCertificationHandoffControllerOutcomeV2 = Result<
    RuntimeServingReceiptV2,
    Box<(
        RuntimeHeldDrainedRouteV2,
        RuntimeControllerCertificationHandoffRejectionV2,
    )>,
>;

struct RuntimeHeldRouteRemovedV2 {
    session: RuntimeConvergenceSessionV1,
    permit: RuntimeServingSlotWorkPermitV2,
    evidence: RuntimeExactTargetEvidenceV2,
    hydrated: RuntimeExactTargetV1,
    witness: RuntimeRouteWitnessV2,
}

#[allow(dead_code)]
impl RuntimeControllerCertificationHandoffV2 {
    fn from_ready_held_v2(
        held: Box<RuntimeHeldDrainedRouteV2>,
        serving_lease_for: Duration,
        acceptance_deadline: DateTime<Utc>,
    ) -> RuntimeControllerCertificationHandoffPreparationV2 {
        if held.ensure_active_v2().is_err()
            || runtime_held_continuation_v2(&held.session.snapshot().phase)
                != Some(RuntimeHeldContinuationV2::HoldAwaitingGatewayReady)
            || serving_lease_for.is_zero()
            || acceptance_deadline >= held.session.expires_at()
        {
            return Err(held);
        }
        Ok(Box::new(Self {
            held: *held,
            serving_lease_for,
            acceptance_deadline,
        }))
    }

    pub(crate) fn ensure_exact_awaiting_v2(&self) -> Result<(), ()> {
        self.held.ensure_active_v2()?;
        if runtime_held_continuation_v2(&self.held.session.snapshot().phase)
            != Some(RuntimeHeldContinuationV2::HoldAwaitingGatewayReady)
        {
            return Err(());
        }
        Ok(())
    }

    pub(crate) fn session_v2(&self) -> &RuntimeConvergenceSessionV1 {
        &self.held.session
    }

    pub(crate) fn exact_target_evidence_v2(&self) -> &RuntimeExactTargetEvidenceV2 {
        &self.held.evidence
    }

    pub(crate) fn hydrated_target_v2(&self) -> &RuntimeExactTargetV1 {
        &self.held.hydrated
    }

    pub(crate) fn route_witness_v2(&self) -> &RuntimeRouteWitnessV2 {
        &self.held.witness
    }

    pub(crate) fn serving_lease_for_v2(&self) -> Duration {
        self.serving_lease_for
    }

    pub(crate) fn acceptance_deadline_v2(&self) -> DateTime<Utc> {
        self.acceptance_deadline
    }

    pub(crate) fn reject_v2(
        self,
        source: RuntimeControllerCertificationHandoffRejectionV2,
    ) -> RuntimeControllerCertificationHandoffReplyV2 {
        RuntimeControllerCertificationHandoffReplyV2::Rejected {
            handoff: Box::new(self),
            source,
        }
    }

    pub(crate) fn accept_v2(self) -> RuntimeControllerAcceptedCertificationPartsV2 {
        let Self {
            held:
                RuntimeHeldDrainedRouteV2 {
                    staged,
                    session,
                    permit,
                    evidence,
                    hydrated,
                    witness,
                },
            serving_lease_for: _,
            acceptance_deadline: _,
        } = self;
        (staged, session, permit, evidence, hydrated, witness)
    }

    fn into_held_v2(self) -> RuntimeHeldDrainedRouteV2 {
        self.held
    }
}

struct RuntimeControllerCertificationReceiptExpectationV2 {
    scope: RuntimeDeploymentScopeV1,
    process_identity: RuntimeProcessIdentityV1,
    serving_lease_for: Duration,
    acceptance_deadline: DateTime<Utc>,
}

impl RuntimeControllerCertificationReceiptExpectationV2 {
    fn from_handoff_v2(handoff: &RuntimeControllerCertificationHandoffV2) -> Result<Self, ()> {
        handoff.ensure_exact_awaiting_v2()?;
        let snapshot = handoff.session_v2().snapshot();
        let panel = snapshot.panel_certificate.as_ref().ok_or(())?;
        let process_identity = RuntimeProcessIdentityV1 {
            target: snapshot.target.clone(),
            runtime_generation: snapshot.runtime_generation,
            process_instance_id: panel.process_instance_id.clone(),
        };
        if panel.target != process_identity.target
            || panel.runtime_generation != process_identity.runtime_generation
        {
            return Err(());
        }
        Ok(Self {
            scope: RuntimeDeploymentScopeV1::from_identity(&snapshot.identity),
            process_identity,
            serving_lease_for: handoff.serving_lease_for_v2(),
            acceptance_deadline: handoff.acceptance_deadline_v2(),
        })
    }

    fn matches_v2(&self, receipt: &RuntimeServingReceiptV2) -> bool {
        receipt.identity.scope == self.scope
            && receipt.identity.process_identity == self.process_identity
            && receipt.connected
            && receipt.serving
            && receipt.acquired_at <= receipt.last_heartbeat_at
            && receipt.last_heartbeat_at <= self.acceptance_deadline
            && receipt.last_heartbeat_at < receipt.expires_at
            && receipt
                .expires_at
                .signed_duration_since(receipt.last_heartbeat_at)
                .to_std()
                .ok()
                == Some(self.serving_lease_for)
    }
}

#[allow(dead_code)]
impl RuntimeControllerCertificationHandoffReplyV2 {
    pub(crate) fn accepted_v2(serving: RuntimeServingReceiptV2) -> Self {
        Self::Accepted(Box::new(serving))
    }

    fn into_controller_outcome_v2(
        self,
    ) -> RuntimeControllerCertificationHandoffControllerOutcomeV2 {
        match self {
            Self::Rejected { handoff, source } => Err(Box::new((handoff.into_held_v2(), source))),
            Self::Accepted(serving) => Ok(*serving),
        }
    }
}

impl Debug for RuntimeControllerCertificationHandoffV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeControllerCertificationHandoffV2(<redacted>)")
    }
}

impl Debug for RuntimeControllerCertificationHandoffReplyV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeControllerCertificationHandoffReplyV2(<redacted>)")
    }
}

impl RuntimeHeldDrainedRouteV2 {
    fn from_convergence(
        drained: RuntimeDrainedConvergenceV2<
            RuntimeExactTargetV1,
            RuntimeRegistryReplacementRouteV2,
            RuntimeControllerBarrierAResumedV2,
        >,
    ) -> Self {
        let (
            staged,
            session,
            permit,
            evidence,
            hydrated,
            witness,
            previous_serving,
            previous_serving_disconnect,
            barrier,
            resumed,
            removal,
        ) = drained.into_handoff();
        drop((
            previous_serving,
            previous_serving_disconnect,
            barrier,
            resumed,
            removal,
        ));
        Self {
            staged,
            session,
            permit,
            evidence,
            hydrated,
            witness,
        }
    }

    fn from_recovery(
        recovery: RuntimeStagedRecoveryHandoffV2<
            RuntimeExactTargetV1,
            RuntimeRegistryReplacementRouteV2,
        >,
    ) -> Result<Self, RuntimeRegistryPredecessorReplacementErrorV2> {
        let route = match recovery {
            RuntimeStagedRecoveryHandoffV2::Drained(route)
            | RuntimeStagedRecoveryHandoffV2::ActivationApplying(route)
            | RuntimeStagedRecoveryHandoffV2::RuntimePendingReady(route)
            | RuntimeStagedRecoveryHandoffV2::ReconcilingPanels(route)
            | RuntimeStagedRecoveryHandoffV2::AwaitingGatewayReady(route) => route,
        };
        Self::from_recovery_route(route)
    }

    fn from_recovery_route(
        route: RuntimeStagedRecoveryRouteV2<
            RuntimeExactTargetV1,
            RuntimeRegistryReplacementRouteV2,
        >,
    ) -> Result<Self, RuntimeRegistryPredecessorReplacementErrorV2> {
        let (staged, session, permit, evidence, hydrated, witness) = route.into_handoff();
        let successor = staged.finalize_empty_recovery_predecessor_v2()?;
        let mut held = Self {
            staged,
            session,
            permit,
            evidence,
            hydrated,
            witness,
        };
        held.apply_staged_evidence_v2(&successor);
        Ok(held)
    }

    fn ensure_active_v2(&self) -> Result<(), ()> {
        self.permit.ensure_active().map_err(|_| ())?;
        self.staged.ensure_staged_v2().map_err(|_| ())?;
        if self.witness.identity != *self.staged.identity_v2()
            || self.staged.fencing_token_v2() != Ok(self.witness.controller_fencing_token)
            || self.evidence.execution().snapshot.target != self.hydrated.snapshot.target
        {
            return Err(());
        }
        Ok(())
    }

    fn apply_staged_evidence_v2(&mut self, evidence: &RuntimeRegistryStagedInstallEvidenceV2) {
        self.witness.identity = evidence.identity_v2().clone();
        self.witness.controller_fencing_token = evidence.fencing_token_v2();
        self.witness.route_incarnation = evidence.route_incarnation_v2();
        self.witness.active_interactions = evidence.active_interactions_v2();
        self.witness.admission_generation = evidence.admission_generation_v2();
        self.witness.registry_observation_sequence =
            evidence.registry_observation_sequence_v2().as_non_zero();
    }

    fn remove_staged_v2(self) -> Result<RuntimeHeldRouteRemovedV2, RuntimeHeldAdvanceOutcomeV2> {
        let Self {
            staged,
            session,
            permit,
            evidence,
            hydrated,
            witness,
        } = self;
        if staged.remove_v2().is_err() {
            drop((session, permit, evidence, hydrated, witness));
            return Err(RuntimeHeldAdvanceOutcomeV2::Exit(
                RuntimeServingControllerActorExitV2::CleanupFailed,
            ));
        }
        Ok(RuntimeHeldRouteRemovedV2 {
            session,
            permit,
            evidence,
            hydrated,
            witness,
        })
    }

    fn finish_v2(
        self,
        success: RuntimeServingControllerActorExitV2,
    ) -> RuntimeServingControllerActorExitV2 {
        if self.cleanup_v2().is_ok() {
            success
        } else {
            RuntimeServingControllerActorExitV2::CleanupFailed
        }
    }

    fn finish_advance_v2(
        self,
        success: RuntimeHeldPostCleanupOutcomeV2,
    ) -> RuntimeHeldAdvanceOutcomeV2 {
        runtime_held_cleanup_outcome_v2(self.cleanup_v2(), success)
    }

    fn cleanup_v2(self) -> Result<(), RuntimeRegistryStagingErrorV2> {
        let Self {
            staged,
            session,
            permit,
            evidence,
            hydrated,
            witness,
        } = self;
        let cleanup = staged.remove_v2();
        drop((session, permit, evidence, hydrated, witness));
        cleanup
    }

    fn finish_retry_v2(self) -> RuntimeHeldRouteOutcomeV2 {
        match self.finish_v2(RuntimeServingControllerActorExitV2::Failed) {
            RuntimeServingControllerActorExitV2::Failed => RuntimeHeldRouteOutcomeV2::Retry,
            exit => RuntimeHeldRouteOutcomeV2::Exit(exit),
        }
    }
}

impl RuntimeHeldRouteRemovedV2 {
    fn finish_advance_v2(
        self,
        success: RuntimeHeldPostCleanupOutcomeV2,
    ) -> RuntimeHeldAdvanceOutcomeV2 {
        let Self {
            session,
            permit,
            evidence,
            hydrated,
            witness,
        } = self;
        drop((session, permit, evidence, hydrated, witness));
        runtime_held_cleanup_outcome_v2(Ok(()), success)
    }
}

fn runtime_held_cleanup_outcome_v2(
    cleanup: Result<(), RuntimeRegistryStagingErrorV2>,
    success: RuntimeHeldPostCleanupOutcomeV2,
) -> RuntimeHeldAdvanceOutcomeV2 {
    if cleanup.is_err() {
        return RuntimeHeldAdvanceOutcomeV2::Exit(
            RuntimeServingControllerActorExitV2::CleanupFailed,
        );
    }
    match success {
        RuntimeHeldPostCleanupOutcomeV2::Retry => RuntimeHeldAdvanceOutcomeV2::Retry,
        RuntimeHeldPostCleanupOutcomeV2::Continue => RuntimeHeldAdvanceOutcomeV2::Continue,
        RuntimeHeldPostCleanupOutcomeV2::Exit(exit) => RuntimeHeldAdvanceOutcomeV2::Exit(exit),
    }
}

impl RuntimeExactTargetHydrationPortV2 for RuntimeControllerDatabaseV2 {
    type Error = RuntimeConvergenceStoreError;
    type Hydrated = RuntimeExactTargetV1;

    fn load_exact_target<'a>(
        &'a self,
        request: &'a RuntimeExactTargetHydrationRequestV2,
    ) -> automation_runtime_worker::RuntimeConvergenceFutureV2<
        'a,
        Result<RuntimeExactTargetObservationV2<Self::Hydrated>, Self::Error>,
    > {
        Box::pin(async move {
            let hydrated = self
                .exact_target()
                .load_for_execution(request.execution())
                .await?;
            let installation_authority_revision = NonZeroU64::new(
                hydrated.installation_authority_revision,
            )
            .ok_or(RuntimeConvergenceStoreError::InvalidPersistedState(
                "runtime installation authority revision",
            ))?;
            let current_authority_revision = NonZeroU64::new(hydrated.current_authority_revision)
                .ok_or(
                RuntimeConvergenceStoreError::InvalidPersistedState(
                    "runtime current authority revision",
                ),
            )?;
            let installation_authority_payload_digest = RuntimeAuthorityPayloadDigestV2::parse(
                hydrated
                    .installation_authority_payload_digest
                    .as_str()
                    .to_owned(),
            )
            .map_err(|_| {
                RuntimeConvergenceStoreError::InvalidPersistedState(
                    "runtime installation authority payload digest",
                )
            })?;
            let current_authority_payload_digest = RuntimeAuthorityPayloadDigestV2::parse(
                hydrated
                    .current_authority_payload_digest
                    .as_str()
                    .to_owned(),
            )
            .map_err(|_| {
                RuntimeConvergenceStoreError::InvalidPersistedState(
                    "runtime current authority payload digest",
                )
            })?;
            let snapshot = &request.execution().snapshot;
            let binding_pin = RuntimeBindingPinV1 {
                tenant_id: snapshot.identity.tenant_id.clone(),
                installation_id: snapshot.identity.installation_id.clone(),
                installation_authority_revision,
                binding_revision: snapshot.target.binding_revision,
                binding_fingerprint: snapshot.target.binding_fingerprint.clone(),
            };
            Ok(RuntimeExactTargetObservationV2 {
                execution: request.execution().clone(),
                persisted_desired_target_digest: hydrated.desired_target_digest.clone(),
                installation_authority_revision,
                current_authority_revision,
                installation_authority_payload_digest,
                current_authority_payload_digest,
                artifact_target: hydrated.snapshot.target.clone(),
                binding_pin,
                observed_database_now: hydrated.database_observed_at,
                hydrated,
            })
        })
    }
}

impl RuntimeConvergenceMutationPortV2 for RuntimeControllerDatabaseV2 {
    type Error = RuntimeExecutionPersistenceErrorV1;

    fn mutate<'a>(
        &'a self,
        request: &'a RuntimeMutationRequestV1,
    ) -> automation_runtime_worker::RuntimeConvergenceFutureV2<
        'a,
        Result<RuntimeMutationReceiptV1, Self::Error>,
    > {
        Box::pin(async move { self.execution().mutate(request.clone()).await })
    }
}

impl RuntimeExactPreviousServingObservationPortV2 for RuntimeControllerDatabaseV2 {
    type Error = RuntimeExecutionPersistenceErrorV1;

    fn observe_previous_serving<'a>(
        &'a self,
        request: &'a RuntimeObservePreviousServingV1,
    ) -> automation_runtime_worker::RuntimeConvergenceFutureV2<
        'a,
        Result<RuntimePreviousServingObservationReceiptV1, Self::Error>,
    > {
        Box::pin(async move {
            self.execution()
                .observe_previous_serving(request.clone())
                .await
        })
    }
}

impl RuntimePreviousServingDisconnectPortV2 for RuntimeControllerDatabaseV2 {
    type Error = automation_runtime_serving_postgres::RuntimeServingPersistenceErrorV1;

    fn disconnect_previous_serving<'a>(
        &'a self,
        request: &'a RuntimeDisconnectServingV1,
    ) -> automation_runtime_worker::RuntimeConvergenceFutureV2<
        'a,
        Result<RuntimeServingUpdateReceiptV1, Self::Error>,
    > {
        Box::pin(async move {
            self.serving()
                .mark_serving_disconnected(request.clone())
                .await
        })
    }
}

struct RuntimeControllerPredecessorPortV2 {
    database: RuntimeControllerDatabaseV2,
}

#[derive(Debug, thiserror::Error)]
enum RuntimeControllerPredecessorPortErrorV2 {
    #[error("runtime predecessor replacement database operation failed")]
    Database(RuntimeExecutionPersistenceErrorV1),
    #[error("runtime predecessor replacement registry operation failed")]
    Registry(RuntimeRegistryPredecessorReplacementErrorV2),
    #[error("runtime predecessor replacement evidence does not match")]
    EvidenceMismatch,
    #[error("runtime predecessor replacement deadline elapsed")]
    DeadlineElapsed,
}

impl
    RuntimeRoutePredecessorTransitionPortV2<
        RuntimeExactTargetV1,
        RuntimeRegistryReplacementRouteV2,
        RuntimeControllerBarrierAPausedV2,
    > for RuntimeControllerPredecessorPortV2
{
    type Error = RuntimeControllerPredecessorPortErrorV2;

    fn transition_predecessor_to_draining(
        &self,
        request: &RuntimeRoutePredecessorTransitionRequestV2,
        hydrated: &RuntimeExactTargetV1,
        staged: &RuntimeRegistryReplacementRouteV2,
        paused: &RuntimeControllerBarrierAPausedV2,
    ) -> Result<RuntimeRoutePredecessorTransitionObservationV2, Self::Error> {
        if Utc::now() > request.deadline()
            || hydrated.snapshot.target != request.initial_successor().identity.target
            || staged.identity_v2() != &request.initial_successor().identity
        {
            return Err(RuntimeControllerPredecessorPortErrorV2::EvidenceMismatch);
        }
        let _paused_handle = paused.handle;
        let local_predecessor = request.expected_previous_runtime().filter(|predecessor| {
            predecessor.process_instance_id
                == request.initial_successor().identity.process_instance_id
        });
        let observation = staged
            .transition_predecessor_to_draining_v2(local_predecessor)
            .map_err(RuntimeControllerPredecessorPortErrorV2::Registry)?;
        let successor = runtime_route_witness_from_staged_v2(observation.successor_v2());
        let predecessor = observation.predecessor_v2().map(|witness| {
            runtime_route_witness_from_slot_v2(
                witness,
                observation.initial_active_interactions_v2(),
                successor.admission_generation,
                successor.registry_observation_sequence,
            )
        });
        Ok(RuntimeRoutePredecessorTransitionObservationV2 {
            correlation: request.correlation().clone(),
            predecessor,
            successor,
            transitioned_at: Utc::now(),
        })
    }
}

impl
    RuntimePredecessorRetirementPortV2<
        RuntimeExactTargetV1,
        RuntimeRegistryReplacementRouteV2,
        RuntimeControllerBarrierAResumedV2,
    > for RuntimeControllerPredecessorPortV2
{
    type Error = RuntimeControllerPredecessorPortErrorV2;

    fn retire_predecessor<'a>(
        &'a self,
        request: &'a RuntimePredecessorRetirementRequestV2,
        hydrated: &'a RuntimeExactTargetV1,
        staged: &'a RuntimeRegistryReplacementRouteV2,
        resumed: &'a RuntimeControllerBarrierAResumedV2,
    ) -> automation_runtime_worker::RuntimeConvergenceFutureV2<
        'a,
        Result<RuntimePredecessorRetirementObservationV2, Self::Error>,
    > {
        Box::pin(async move {
            if hydrated.snapshot.target != request.successor().identity.target
                || staged.identity_v2() != &request.successor().identity
            {
                return Err(RuntimeControllerPredecessorPortErrorV2::EvidenceMismatch);
            }
            let _resumed_handle = resumed.handle;
            let previous_serving = self
                .database
                .execution()
                .observe_previous_serving(request.previous_serving_request().clone())
                .await
                .map_err(RuntimeControllerPredecessorPortErrorV2::Database)?;
            loop {
                if Utc::now() > request.deadline() {
                    return Err(RuntimeControllerPredecessorPortErrorV2::DeadlineElapsed);
                }
                let drain = staged
                    .observe_predecessor_drain_v2()
                    .map_err(RuntimeControllerPredecessorPortErrorV2::Registry)?;
                if drain.drained_v2() {
                    break;
                }
                let _active = drain.active_interactions_v2();
                sleep(Duration::from_millis(10)).await;
            }
            let removal = staged
                .remove_drained_predecessor_v2()
                .map_err(RuntimeControllerPredecessorPortErrorV2::Registry)?;
            let successor = runtime_route_witness_from_staged_v2(removal.successor_v2());
            let removed_predecessor = removal.removed_predecessor_v2().map(|witness| {
                runtime_route_witness_from_slot_v2(
                    witness,
                    0,
                    successor.admission_generation,
                    successor.registry_observation_sequence,
                )
            });
            Ok(RuntimePredecessorRetirementObservationV2 {
                previous_serving,
                route: RuntimeRoutePredecessorRemovalObservationV2 {
                    removed_predecessor,
                    successor,
                    observed_at: Utc::now(),
                },
            })
        })
    }
}

fn runtime_route_witness_from_staged_v2(
    evidence: &RuntimeRegistryStagedInstallEvidenceV2,
) -> RuntimeRouteWitnessV2 {
    RuntimeRouteWitnessV2 {
        identity: evidence.identity_v2().clone(),
        controller_fencing_token: evidence.fencing_token_v2(),
        route_incarnation: evidence.route_incarnation_v2(),
        lifecycle: RuntimeRouteLifecycleV2::Staged,
        active_interactions: evidence.active_interactions_v2(),
        admission_generation: evidence.admission_generation_v2(),
        registry_observation_sequence: evidence.registry_observation_sequence_v2().as_non_zero(),
    }
}

fn runtime_route_witness_from_slot_v2(
    witness: &SlotRouteWitnessV1,
    active_interactions: u32,
    admission_generation: NonZeroU64,
    registry_observation_sequence: NonZeroU64,
) -> RuntimeRouteWitnessV2 {
    RuntimeRouteWitnessV2 {
        identity: witness.identity.clone(),
        controller_fencing_token: witness.fencing_token,
        route_incarnation: witness.incarnation,
        lifecycle: match witness.lifecycle {
            SlotLifecycleV1::Staged => RuntimeRouteLifecycleV2::Staged,
            SlotLifecycleV1::Serving => RuntimeRouteLifecycleV2::Serving,
            SlotLifecycleV1::Draining => RuntimeRouteLifecycleV2::Draining,
        },
        active_interactions,
        admission_generation,
        registry_observation_sequence,
    }
}

#[derive(Clone)]
struct RuntimeControllerDiscordPreflightPortV2 {
    inner: OwnedDiscordRuntimeOperationsV2,
}

impl RuntimeDiscordPreflightPortV2<RuntimeExactTargetV1>
    for RuntimeControllerDiscordPreflightPortV2
{
    type Error = RuntimeControllerDiscordPreflightPortErrorV2;

    fn verify_preflight<'a>(
        &'a self,
        request: &'a RuntimeDiscordPreflightRequestV2,
        hydrated: &'a RuntimeExactTargetV1,
    ) -> automation_runtime_worker::RuntimeConvergenceFutureV2<
        'a,
        Result<RuntimeDiscordPreflightObservationV2, Self::Error>,
    > {
        Box::pin(async move {
            let deadline = runtime_datetime_deadline_v2(request.deadline());
            timeout_at(
                deadline,
                self.inner.preflight(
                    request.target().guild_id,
                    &hydrated.artifact,
                    &hydrated.bindings,
                ),
            )
            .await
            .map_err(|_| RuntimeControllerDiscordPreflightPortErrorV2::DeadlineElapsed)?
            .map_err(RuntimeControllerDiscordPreflightPortErrorV2::Discord)?;
            let checked_at = Utc::now();
            if checked_at > request.deadline() {
                return Err(RuntimeControllerDiscordPreflightPortErrorV2::DeadlineElapsed);
            }
            Ok(RuntimeDiscordPreflightObservationV2 {
                target: request.target().clone(),
                runtime_generation: request.runtime_generation(),
                binding_pin: request.binding_pin().clone(),
                checked_at,
            })
        })
    }
}

#[derive(Debug, thiserror::Error)]
enum RuntimeControllerDiscordPreflightPortErrorV2 {
    #[error("runtime Discord preflight deadline elapsed")]
    DeadlineElapsed,
    #[error("runtime Discord preflight failed")]
    Discord(RuntimeDiscordPreflightErrorV1),
}

impl RuntimeControllerDiscordPreflightPortErrorV2 {
    fn disposition_v2(&self) -> RuntimeControllerDiscordPreflightDispositionV2 {
        match self {
            Self::DeadlineElapsed => RuntimeControllerDiscordPreflightDispositionV2::Retryable,
            Self::Discord(error) if error.is_retryable() => {
                RuntimeControllerDiscordPreflightDispositionV2::Retryable
            }
            Self::Discord(RuntimeDiscordPreflightErrorV1::Target(_)) => {
                RuntimeControllerDiscordPreflightDispositionV2::DeploymentBlocked
            }
            Self::Discord(RuntimeDiscordPreflightErrorV1::Snapshot(error))
                if !matches!(
                    error,
                    automation_runtime::RuntimeReadinessSnapshotErrorV1::BotIdentityInvalid
                ) =>
            {
                RuntimeControllerDiscordPreflightDispositionV2::DeploymentBlocked
            }
            Self::Discord(
                RuntimeDiscordPreflightErrorV1::Snapshot(_)
                | RuntimeDiscordPreflightErrorV1::TargetGuildMismatch,
            ) => RuntimeControllerDiscordPreflightDispositionV2::ProcessFatal,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeControllerDiscordPreflightDispositionV2 {
    Retryable,
    DeploymentBlocked,
    ProcessFatal,
}

fn runtime_discord_preflight_blocking_mutation_v2() -> RuntimeConvergenceMutationV1 {
    RuntimeConvergenceMutationV1::Cancel {
        reason: "runtime_discord_preflight_blocked".to_owned(),
    }
}

#[derive(Clone)]
struct RuntimeControllerBarrierPortV2 {
    events: mpsc::Sender<RuntimeServingControllerCommandV2>,
}

impl RuntimeBarrierAPausePortV2 for RuntimeControllerBarrierPortV2 {
    type Error = RuntimeControllerBarrierBridgeErrorV2;
    type Paused = RuntimeControllerBarrierAPausedV2;

    fn pause_barrier_a<'a>(
        &'a self,
        request: &'a RuntimeBarrierAPauseRequestV2,
    ) -> automation_runtime_worker::RuntimeConvergenceFutureV2<
        'a,
        Result<RuntimeBarrierAPauseObservationV2<Self::Paused>, Self::Error>,
    > {
        let events = self.events.clone();
        let command = RuntimeControllerBarrierAPauseCommandV2 {
            correlation: request.correlation().clone(),
            started_at: request.started_at(),
            deadline: request.deadline(),
        };
        Box::pin(async move {
            let (reply, response) = oneshot::channel();
            events
                .send(RuntimeServingControllerCommandV2::PauseBarrierA { command, reply })
                .await
                .map_err(|_| RuntimeControllerBarrierBridgeErrorV2::Unavailable)?;
            response
                .await
                .map_err(|_| RuntimeControllerBarrierBridgeErrorV2::Unavailable)?
        })
    }
}

impl RuntimeBarrierAResumePortV2<RuntimeControllerBarrierAPausedV2>
    for RuntimeControllerBarrierPortV2
{
    type Error = RuntimeControllerBarrierBridgeErrorV2;
    type Resumed = RuntimeControllerBarrierAResumedV2;

    fn resume_barrier_a_closed<'a>(
        &'a self,
        request: &'a RuntimeBarrierAResumeRequestV2,
        paused: &'a RuntimeControllerBarrierAPausedV2,
    ) -> automation_runtime_worker::RuntimeConvergenceFutureV2<
        'a,
        Result<RuntimeBarrierAResumeObservationV2<Self::Resumed>, Self::Error>,
    > {
        let events = self.events.clone();
        let command = RuntimeControllerBarrierAResumeCommandV2 {
            correlation: request.correlation().clone(),
            coordinator_generation: request.coordinator_generation(),
            connection_epoch: request.connection_epoch(),
            pause_admission_revision: request.pause_admission_revision(),
            connected_event_sequence: request.connected_event_sequence(),
            pause_sequence: request.pause_sequence(),
            transitioned_at: request.transitioned_at(),
            deadline: request.deadline(),
            paused: *paused,
        };
        Box::pin(async move {
            let (reply, response) = oneshot::channel();
            events
                .send(RuntimeServingControllerCommandV2::ResumeBarrierA { command, reply })
                .await
                .map_err(|_| RuntimeControllerBarrierBridgeErrorV2::Unavailable)?;
            response
                .await
                .map_err(|_| RuntimeControllerBarrierBridgeErrorV2::Unavailable)?
        })
    }
}

struct RuntimeControllerStagePortV2 {
    registry: RuntimeRegistryStagingPortV2,
    emergency: RuntimeRegistryEmergencyTriggerV2,
}

impl RuntimeStagedRoutePortV2<RuntimeExactTargetV1> for RuntimeControllerStagePortV2 {
    type Error = RuntimeControllerStagePortErrorV2;
    type Staged = RuntimeRegistryReplacementRouteV2;

    fn install_staged(
        &self,
        request: &RuntimeRouteStageRequestV2,
        hydrated: &RuntimeExactTargetV1,
    ) -> Result<RuntimeRouteStageObservationV2<Self::Staged>, Self::Error> {
        if !exact_staging_route_identity_matches_v2(
            &request.execution_guard().scope,
            &hydrated.snapshot.identity,
            &hydrated.snapshot.target,
            hydrated.snapshot.runtime_generation,
            request.process_identity(),
        ) || &hydrated.desired_target_digest != request.desired_target_digest()
            || hydrated.installation_authority_revision
                != request.installation_authority_revision().get()
            || hydrated.current_authority_revision != request.current_authority_revision().get()
            || hydrated.installation_authority_payload_digest.as_str()
                != request.installation_authority_payload_digest().as_str()
            || hydrated.current_authority_payload_digest.as_str()
                != request.current_authority_payload_digest().as_str()
            || request.binding_pin().installation_authority_revision
                != request.installation_authority_revision()
            || !request
                .binding_pin()
                .matches_target(&request.process_identity().target)
        {
            return Err(RuntimeControllerStagePortErrorV2::EvidenceMismatch);
        }
        let route = ExactServingRouteV1::new(
            hydrated.snapshot.identity.clone(),
            request.process_identity().clone(),
            hydrated.artifact.clone(),
            hydrated.bindings.clone(),
        )
        .map_err(RuntimeControllerStagePortErrorV2::Route)?;
        let install = self
            .registry
            .install_staged_route_v2(
                route,
                request.execution_guard().fencing_token,
                self.emergency.clone(),
            )
            .map_err(RuntimeControllerStagePortErrorV2::Registry)?;
        let outcome = match install.outcome_v2() {
            RuntimeRegistryStagedInstallOutcomeV2::Installed => {
                RuntimeRouteStageOutcomeV2::Installed
            }
            RuntimeRegistryStagedInstallOutcomeV2::ExactReplay => {
                RuntimeRouteStageOutcomeV2::ExactReplay
            }
        };
        let evidence = install.evidence_v2();
        let witness = RuntimeRouteWitnessV2 {
            identity: evidence.identity_v2().clone(),
            controller_fencing_token: evidence.fencing_token_v2(),
            route_incarnation: evidence.route_incarnation_v2(),
            lifecycle: RuntimeRouteLifecycleV2::Staged,
            active_interactions: evidence.active_interactions_v2(),
            admission_generation: evidence.admission_generation_v2(),
            registry_observation_sequence: evidence
                .registry_observation_sequence_v2()
                .as_non_zero(),
        };
        let (_, _, staged) = install.into_parts_v2();
        Ok(RuntimeRouteStageObservationV2 {
            outcome,
            witness,
            staged: staged.into_replacement_v2(),
        })
    }
}

fn exact_staging_route_identity_matches_v2(
    expected_scope: &RuntimeDeploymentScopeV1,
    deployment_identity: &RuntimeDeploymentIdentityV1,
    target: &RuntimeDeploymentTargetV1,
    runtime_generation: RuntimeGeneration,
    process_identity: &RuntimeProcessIdentityV1,
) -> bool {
    expected_scope.matches(deployment_identity)
        && target == &process_identity.target
        && runtime_generation == process_identity.runtime_generation
}

#[derive(Debug, thiserror::Error)]
enum RuntimeControllerStagePortErrorV2 {
    #[error("runtime exact staged route evidence does not match")]
    EvidenceMismatch,
    #[error("runtime exact staged route is invalid")]
    Route(ExactServingRouteError),
    #[error("runtime exact staged route registry operation failed")]
    Registry(RuntimeRegistryStagingErrorV2),
}

fn classify_actor_join_v2(
    result: Result<RuntimeServingControllerActorExitV2, JoinError>,
) -> RuntimeServingControllerActorExitV2 {
    result.unwrap_or(RuntimeServingControllerActorExitV2::Failed)
}

fn accept_commanded_exit_v2(
    terminal: RuntimeServingControllerActorExitV2,
) -> Result<(), RuntimeServingControllerShutdownErrorV2> {
    if terminal == RuntimeServingControllerActorExitV2::Commanded {
        Ok(())
    } else {
        Err(RuntimeServingControllerShutdownErrorV2::UnexpectedExit)
    }
}

fn retryable_execution_error_v2(error: RuntimeExecutionPersistenceErrorV1) -> bool {
    !matches!(error.class(), RuntimeConvergenceErrorClassV1::InvalidState)
}

fn runtime_panel_block_mutation_failure_v2(
    error: RuntimeExecutionPersistenceErrorV1,
) -> RuntimeHeldAdvanceFailureV2 {
    match error {
        RuntimeExecutionPersistenceErrorV1::InvalidInput
        | RuntimeExecutionPersistenceErrorV1::DatabaseAuthorityMismatch
        | RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt
        | RuntimeExecutionPersistenceErrorV1::DatabaseFailure => {
            RuntimeHeldAdvanceFailureV2::Failed
        }
        RuntimeExecutionPersistenceErrorV1::OwnershipLost
        | RuntimeExecutionPersistenceErrorV1::AuthorityChanged
        | RuntimeExecutionPersistenceErrorV1::RetryNotReady
        | RuntimeExecutionPersistenceErrorV1::Superseded
        | RuntimeExecutionPersistenceErrorV1::Timeout
        | RuntimeExecutionPersistenceErrorV1::Concurrency
        | RuntimeExecutionPersistenceErrorV1::Unavailable
        | RuntimeExecutionPersistenceErrorV1::Indeterminate
        | RuntimeExecutionPersistenceErrorV1::ObservationAmbiguous => {
            RuntimeHeldAdvanceFailureV2::Retry
        }
        _ => RuntimeHeldAdvanceFailureV2::Retry,
    }
}

fn runtime_panel_failure_id_v2(
    session: &RuntimeConvergenceSessionV1,
) -> Result<RuntimeFailureId, ()> {
    runtime_panel_failure_id_from_parts_v2(
        session.snapshot().identity.promotion_id.as_str(),
        session.convergence_attempt().get(),
    )
}

fn runtime_panel_failure_id_from_parts_v2(
    promotion_id: &str,
    convergence_attempt: u32,
) -> Result<RuntimeFailureId, ()> {
    RuntimeFailureId::parse(format!("panel:{}:{}", promotion_id, convergence_attempt))
        .map_err(|_| ())
}

fn retryable_hydration_error_v2(error: &RuntimeConvergenceStoreError) -> bool {
    error.is_retryable()
        || matches!(
            error,
            RuntimeConvergenceStoreError::ExecutionClaimStale
                | RuntimeConvergenceStoreError::ActiveTargetMismatch
                | RuntimeConvergenceStoreError::BindingAuthorityMismatch
                | RuntimeConvergenceStoreError::ProductAuthorityInactive
        )
}

fn runtime_certification_acceptance_deadline_v2(
    expires_at: DateTime<Utc>,
    config: &RuntimeControllerConfigV1,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let renewal_cutoff =
        expires_at.checked_sub_signed(TimeDelta::from_std(config.controller_renew_before).ok()?)?;
    let configured_cutoff =
        now.checked_add_signed(TimeDelta::from_std(config.gateway_ready_timeout).ok()?)?;
    let deadline = renewal_cutoff.min(configured_cutoff);
    (deadline > now).then_some(deadline)
}

fn runtime_claim_requires_renewal_v2(
    expires_at: DateTime<Utc>,
    config: &RuntimeControllerConfigV1,
    now: DateTime<Utc>,
) -> bool {
    let Some(preflight_budget) = config.preflight_timeout.checked_mul(2) else {
        return true;
    };
    let Some(required_runway) = config.controller_renew_before.checked_add(preflight_budget) else {
        return true;
    };
    let Ok(required_runway) = TimeDelta::from_std(required_runway) else {
        return true;
    };
    expires_at
        .checked_sub_signed(required_runway)
        .is_none_or(|latest_start| now >= latest_start)
}

fn runtime_phase_allows_preflight_cancel_v2(phase: &RuntimeDeploymentPhaseV1) -> bool {
    matches!(
        phase,
        RuntimeDeploymentPhaseV1::Requested
            | RuntimeDeploymentPhaseV1::PreflightReady
            | RuntimeDeploymentPhaseV1::DrainRequested
    )
}

fn runtime_datetime_deadline_v2(deadline: DateTime<Utc>) -> TokioInstant {
    let remaining = deadline
        .signed_duration_since(Utc::now())
        .to_std()
        .unwrap_or(Duration::ZERO);
    TokioInstant::now() + remaining
}

async fn runtime_wait_until_stop_v2(stop: &mut watch::Receiver<bool>, duration: Duration) -> bool {
    runtime_operation_until_stop_v2(stop, sleep(duration))
        .await
        .is_err()
}

async fn runtime_operation_until_stop_v2<T>(
    stop: &mut watch::Receiver<bool>,
    operation: impl Future<Output = T>,
) -> Result<T, ()> {
    tokio::select! {
        biased;
        _ = runtime_wait_for_stop_v2(stop) => Err(()),
        output = operation => Ok(output),
    }
}

async fn runtime_wait_for_stop_v2(stop: &mut watch::Receiver<bool>) {
    if *stop.borrow() {
        return;
    }
    loop {
        if stop.changed().await.is_err() || *stop.borrow() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel_reconciliation::RuntimePanelIneligibilityV2;
    use automation_runtime::RuntimeReadinessSnapshotErrorV1;
    use automation_runtime_convergence::{
        RuntimeFailureId, RuntimeFailureKindV1, RuntimeFailureV1,
    };
    use std::num::NonZeroU32;

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(second, 0).unwrap()
    }

    #[test]
    fn exact_staging_route_requires_product_scope_target_and_generation() {
        let identity: RuntimeDeploymentIdentityV1 = serde_json::from_value(serde_json::json!({
            "deployment_id": "deployment:stage-test",
            "tenant_id": "tenant:stage-test",
            "installation_id": "installation:stage-test",
            "promotion_id": "2".repeat(64),
            "activation_request_id": "activation:stage-test"
        }))
        .unwrap();
        let target: RuntimeDeploymentTargetV1 = serde_json::from_value(serde_json::json!({
            "guild_id": "42",
            "ruleset_key": "studyroom",
            "version": 1,
            "content_hash": "3".repeat(64),
            "binding_revision": 1,
            "binding_fingerprint": "4".repeat(64)
        }))
        .unwrap();
        let runtime_generation = RuntimeGeneration::new(7).unwrap();
        let process_identity: RuntimeProcessIdentityV1 =
            serde_json::from_value(serde_json::json!({
                "target": target,
                "runtime_generation": runtime_generation,
                "process_instance_id": "stage-test-process"
            }))
            .unwrap();
        let scope = RuntimeDeploymentScopeV1::from_identity(&identity);

        assert!(exact_staging_route_identity_matches_v2(
            &scope,
            &identity,
            &process_identity.target,
            runtime_generation,
            &process_identity,
        ));

        let mut foreign_identity = identity.clone();
        foreign_identity.tenant_id =
            automation_runtime_convergence::TenantId::parse("tenant:foreign").unwrap();
        assert!(!exact_staging_route_identity_matches_v2(
            &scope,
            &foreign_identity,
            &process_identity.target,
            runtime_generation,
            &process_identity,
        ));

        let mut foreign_target = process_identity.target.clone();
        foreign_target.guild_id = discord_model::GuildId(43);
        assert!(!exact_staging_route_identity_matches_v2(
            &scope,
            &identity,
            &foreign_target,
            runtime_generation,
            &process_identity,
        ));

        let mut foreign_route = process_identity.target.clone();
        foreign_route.ruleset_key = serde_json::from_value(serde_json::json!("welcome")).unwrap();
        assert!(!exact_staging_route_identity_matches_v2(
            &scope,
            &identity,
            &foreign_route,
            runtime_generation,
            &process_identity,
        ));

        assert!(!exact_staging_route_identity_matches_v2(
            &scope,
            &identity,
            &process_identity.target,
            RuntimeGeneration::new(8).unwrap(),
            &process_identity,
        ));
    }

    #[test]
    fn discord_preflight_disposition_is_installation_scoped_and_fail_closed() {
        let retryable = RuntimeControllerDiscordPreflightPortErrorV2::Discord(
            RuntimeDiscordPreflightErrorV1::Snapshot(
                RuntimeReadinessSnapshotErrorV1::GuildRolesUnavailable,
            ),
        );
        let blocked = RuntimeControllerDiscordPreflightPortErrorV2::Discord(
            RuntimeDiscordPreflightErrorV1::Snapshot(
                RuntimeReadinessSnapshotErrorV1::BoundRoleMissing,
            ),
        );
        let invalid_bot = RuntimeControllerDiscordPreflightPortErrorV2::Discord(
            RuntimeDiscordPreflightErrorV1::Snapshot(
                RuntimeReadinessSnapshotErrorV1::BotIdentityInvalid,
            ),
        );
        let wrong_guild = RuntimeControllerDiscordPreflightPortErrorV2::Discord(
            RuntimeDiscordPreflightErrorV1::TargetGuildMismatch,
        );

        assert_eq!(
            RuntimeControllerDiscordPreflightPortErrorV2::DeadlineElapsed.disposition_v2(),
            RuntimeControllerDiscordPreflightDispositionV2::Retryable
        );
        assert_eq!(
            retryable.disposition_v2(),
            RuntimeControllerDiscordPreflightDispositionV2::Retryable
        );
        assert_eq!(
            blocked.disposition_v2(),
            RuntimeControllerDiscordPreflightDispositionV2::DeploymentBlocked
        );
        assert_eq!(
            invalid_bot.disposition_v2(),
            RuntimeControllerDiscordPreflightDispositionV2::ProcessFatal
        );
        assert_eq!(
            wrong_guild.disposition_v2(),
            RuntimeControllerDiscordPreflightDispositionV2::ProcessFatal
        );
    }

    #[test]
    fn claim_runway_reserves_two_preflight_budgets_before_renewal_cutoff() {
        let config = RuntimeControllerConfigV1::default();

        assert!(!runtime_claim_requires_renewal_v2(at(100), &config, at(29)));
        assert!(runtime_claim_requires_renewal_v2(at(100), &config, at(30)));
    }

    #[test]
    fn deterministic_discord_preflight_failure_uses_a_legal_early_phase_terminal_mutation() {
        assert_eq!(
            runtime_discord_preflight_blocking_mutation_v2(),
            RuntimeConvergenceMutationV1::Cancel {
                reason: "runtime_discord_preflight_blocked".to_owned(),
            }
        );
        assert!(runtime_phase_allows_preflight_cancel_v2(
            &RuntimeDeploymentPhaseV1::Requested
        ));
        assert!(runtime_phase_allows_preflight_cancel_v2(
            &RuntimeDeploymentPhaseV1::PreflightReady
        ));
        assert!(runtime_phase_allows_preflight_cancel_v2(
            &RuntimeDeploymentPhaseV1::DrainRequested
        ));
        assert!(!runtime_phase_allows_preflight_cancel_v2(
            &RuntimeDeploymentPhaseV1::Drained
        ));
        assert!(!runtime_phase_allows_preflight_cancel_v2(
            &RuntimeDeploymentPhaseV1::AwaitingGatewayReady
        ));
    }

    #[test]
    fn durable_phase_continuation_never_replays_an_earlier_mutation() {
        assert_eq!(
            runtime_held_continuation_v2(&RuntimeDeploymentPhaseV1::Drained),
            Some(RuntimeHeldContinuationV2::BeginActivation)
        );
        assert_eq!(
            runtime_held_continuation_v2(&RuntimeDeploymentPhaseV1::ActivationApplying),
            Some(RuntimeHeldContinuationV2::AcceptActivation)
        );
        assert_eq!(
            runtime_held_continuation_v2(&RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Ready,
            }),
            Some(RuntimeHeldContinuationV2::BeginPanelReconciliation)
        );
        assert_eq!(
            runtime_held_continuation_v2(&RuntimeDeploymentPhaseV1::ReconcilingPanels),
            Some(RuntimeHeldContinuationV2::ReconcilePanels)
        );
        assert_eq!(
            runtime_held_continuation_v2(&RuntimeDeploymentPhaseV1::AwaitingGatewayReady),
            Some(RuntimeHeldContinuationV2::HoldAwaitingGatewayReady)
        );
    }

    #[test]
    fn non_ready_and_nonproduction_phases_have_no_held_continuation() {
        let failure = RuntimeFailureV1 {
            failure_id: RuntimeFailureId::parse("failure:held").unwrap(),
            kind: RuntimeFailureKindV1::InvariantViolation,
            code: "held_recovery".to_owned(),
            message: "held".to_owned(),
            recorded_at: at(10),
        };
        for phase in [
            RuntimeDeploymentPhaseV1::Requested,
            RuntimeDeploymentPhaseV1::PreflightReady,
            RuntimeDeploymentPhaseV1::DrainRequested,
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Retryable {
                    failure: failure.clone(),
                    attempt: NonZeroU32::MIN,
                    retry_not_before: at(20),
                },
            },
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Blocked {
                    failure: failure.clone(),
                },
            },
            RuntimeDeploymentPhaseV1::Live,
            RuntimeDeploymentPhaseV1::Cancelled {
                reason: "terminal".to_owned(),
                cancelled_at: at(20),
            },
        ] {
            assert_eq!(runtime_held_continuation_v2(&phase), None);
        }
    }

    #[test]
    fn observation_required_receives_one_fresh_journal_pass_before_blocking() {
        for reason in [
            RuntimePanelIneligibilityV2::Ambiguous,
            RuntimePanelIneligibilityV2::CleanupPending,
        ] {
            let error = RuntimeExactPanelReconciliationErrorV2::Ineligible { reason };
            let mut budget = RuntimePanelObservationBudgetV2::default();

            assert_eq!(
                budget.next_v2(error.disposition_v2()),
                RuntimePanelFailureActionV2::ObserveJournal
            );
            assert_eq!(
                budget.next_v2(error.disposition_v2()),
                RuntimePanelFailureActionV2::BlockDeployment
            );
            assert_eq!(
                budget.next_v2(error.disposition_v2()),
                RuntimePanelFailureActionV2::BlockDeployment
            );
        }
    }

    #[test]
    fn retryable_panel_failure_does_not_consume_observation_budget() {
        let mut budget = RuntimePanelObservationBudgetV2::default();

        assert_eq!(
            budget.next_v2(RuntimeExactPanelReconciliationDispositionV2::RetryableInfrastructure),
            RuntimePanelFailureActionV2::Retry
        );
        assert_eq!(
            budget.next_v2(RuntimeExactPanelReconciliationDispositionV2::ObservationRequired),
            RuntimePanelFailureActionV2::ObserveJournal
        );
    }

    #[test]
    fn staged_cleanup_failure_escalates_and_successful_block_keeps_controller_alive() {
        let cleanup_failed = runtime_held_cleanup_outcome_v2(
            Err(RuntimeRegistryStagingErrorV2::UnexpectedLifecycle),
            RuntimeHeldPostCleanupOutcomeV2::Continue,
        );
        assert!(matches!(
            cleanup_failed,
            RuntimeHeldAdvanceOutcomeV2::Exit(RuntimeServingControllerActorExitV2::CleanupFailed)
        ));

        let deployment_blocked =
            runtime_held_cleanup_outcome_v2(Ok(()), RuntimeHeldPostCleanupOutcomeV2::Continue);
        assert!(matches!(
            deployment_blocked,
            RuntimeHeldAdvanceOutcomeV2::Continue
        ));
    }

    #[test]
    fn panel_failure_identity_is_stable_and_bounded_per_attempt() {
        let promotion_id = "a".repeat(64);
        let first = runtime_panel_failure_id_from_parts_v2(&promotion_id, 7).unwrap();
        let replay = runtime_panel_failure_id_from_parts_v2(&promotion_id, 7).unwrap();
        let successor = runtime_panel_failure_id_from_parts_v2(&promotion_id, 8).unwrap();

        assert_eq!(first, replay);
        assert_ne!(first, successor);
        assert!(first.as_str().len() <= 128);
    }

    #[test]
    fn certification_handoff_contract_separates_rejection_from_accepted_ownership() {
        fn assert_send<T: Send>() {}
        let _: fn(
            Box<RuntimeHeldDrainedRouteV2>,
            Duration,
            DateTime<Utc>,
        ) -> RuntimeControllerCertificationHandoffPreparationV2 =
            RuntimeControllerCertificationHandoffV2::from_ready_held_v2;
        let _: fn(
            RuntimeControllerCertificationHandoffV2,
            RuntimeControllerCertificationHandoffRejectionV2,
        ) -> RuntimeControllerCertificationHandoffReplyV2 =
            RuntimeControllerCertificationHandoffV2::reject_v2;
        let _: fn(
            RuntimeControllerCertificationHandoffV2,
        ) -> RuntimeControllerAcceptedCertificationPartsV2 =
            RuntimeControllerCertificationHandoffV2::accept_v2;
        let _: fn(RuntimeServingReceiptV2) -> RuntimeControllerCertificationHandoffReplyV2 =
            RuntimeControllerCertificationHandoffReplyV2::accepted_v2;
        assert_send::<RuntimeControllerCertificationHandoffV2>();
        assert_send::<RuntimeControllerAcceptedCertificationPartsV2>();
        assert_send::<RuntimeControllerCertificationHandoffReplyV2>();
        assert_eq!(
            format!(
                "{:?}",
                RuntimeControllerCertificationHandoffRejectionV2::Unavailable
            ),
            "Unavailable"
        );
    }

    #[test]
    fn certification_acceptance_deadline_is_bounded_by_configured_gateway_budget() {
        let config = RuntimeControllerConfigV1::default();

        assert_eq!(
            runtime_certification_acceptance_deadline_v2(at(100), &config, at(10)),
            Some(at(40))
        );
    }

    #[test]
    fn certification_acceptance_deadline_never_crosses_lease_renewal_cutoff() {
        let config = RuntimeControllerConfigV1::default();

        assert_eq!(
            runtime_certification_acceptance_deadline_v2(at(100), &config, at(50)),
            Some(at(70))
        );
        assert_eq!(
            runtime_certification_acceptance_deadline_v2(at(100), &config, at(70)),
            None
        );
    }
}
