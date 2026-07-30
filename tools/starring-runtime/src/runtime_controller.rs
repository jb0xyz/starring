use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::num::NonZeroU64;
use std::time::{Duration, Instant};

use automation_runtime::{OwnedDiscordRuntimePreflightV1, RuntimeDiscordPreflightErrorV1};
use automation_runtime_controller::{
    plan_runtime_action_v1, RuntimeBindingPinV1, RuntimeClaimNextExecutionV1,
    RuntimeControllerActionV1, RuntimeControllerConfigV1, RuntimeConvergenceErrorClassV1,
    RuntimeConvergenceMutationV1, RuntimeConvergenceSessionStateV1, RuntimeConvergenceSessionV1,
    RuntimeExecutionReceiptV1, RuntimeMutationReceiptV1, RuntimeMutationRequestV1,
    RuntimeServingSlotV2,
};
use automation_runtime_convergence::ControllerId;
use automation_runtime_convergence_postgres::{RuntimeConvergenceStoreError, RuntimeExactTargetV1};
use automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1;
use automation_runtime_registry::{ExactServingRouteError, ExactServingRouteV1};
use automation_runtime_worker::{
    RuntimeAcceptPreflightMutationErrorV2, RuntimeAuthorityPayloadDigestV2,
    RuntimeClaimedConvergenceV2, RuntimeConvergenceMutationPortV2, RuntimeDiscordPreflightErrorV2,
    RuntimeDiscordPreflightObservationV2, RuntimeDiscordPreflightOutcomeV2,
    RuntimeDiscordPreflightPortV2, RuntimeDiscordPreflightRequestV2, RuntimeExactTargetEvidenceV2,
    RuntimeExactTargetHydrationErrorV2, RuntimeExactTargetHydrationPortV2,
    RuntimeExactTargetHydrationRequestV2, RuntimeExactTargetObservationV2, RuntimeRouteLifecycleV2,
    RuntimeRouteStageObservationV2, RuntimeRouteStageOutcomeV2, RuntimeRouteStageRequestV2,
    RuntimeRouteWitnessV2, RuntimeServingSlotWorkErrorV2, RuntimeServingSlotWorkPermitV2,
    RuntimeStagedRoutePortV2,
};
use chrono::{DateTime, TimeDelta, Utc};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::{JoinError, JoinHandle};
use tokio::time::{sleep, sleep_until, timeout_at, Instant as TokioInstant};

use crate::database::RuntimeControllerDatabaseV2;
use crate::process_supervisor::RuntimeProcessShutdownTriggerV1;
use crate::registry::{
    RuntimeRegistryEmergencyTriggerV2, RuntimeRegistryStagedInstallOutcomeV2,
    RuntimeRegistryStagedRouteV2, RuntimeRegistryStagingErrorV2, RuntimeRegistryStagingPortV2,
};
use crate::RuntimeShutdownCauseV1;

const RUNTIME_CONTROLLER_COMMAND_CAPACITY_V2: usize = 1;
const RUNTIME_CONTROLLER_IDLE_BACKOFF_V2: Duration = Duration::from_secs(1);
const RUNTIME_CONTROLLER_RETRY_BACKOFF_V2: Duration = Duration::from_secs(5);

type RuntimeSlotPermitReplyV2 =
    Result<RuntimeServingSlotWorkPermitV2, RuntimeServingSlotWorkErrorV2>;

pub(crate) enum RuntimeServingControllerEventV2 {
    AcquireSlot {
        slot: RuntimeServingSlotV2,
        reply: oneshot::Sender<RuntimeSlotPermitReplyV2>,
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
    ) -> Self {
        let config = RuntimeControllerConfigV1::default();
        config
            .validate()
            .expect("default runtime controller configuration must remain valid");
        let (events_tx, events) = mpsc::channel(RUNTIME_CONTROLLER_COMMAND_CAPACITY_V2);
        let (stop, stop_rx) = watch::channel(false);
        let actor = RuntimeServingControllerActorV2 {
            database,
            discord: RuntimeControllerDiscordPreflightPortV2 {
                inner: OwnedDiscordRuntimePreflightV1::new(discord_token),
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
            Command(Option<RuntimeServingControllerCommandV2>),
            Join(Result<RuntimeServingControllerActorExitV2, JoinError>),
        }
        let selected = {
            let join = self
                .join
                .as_mut()
                .expect("live runtime controller supervisor must retain its task");
            tokio::select! {
                biased;
                command = self.events.recv() => SelectedV2::Command(command),
                result = join => SelectedV2::Join(result),
            }
        };
        match selected {
            SelectedV2::Command(Some(RuntimeServingControllerCommandV2::AcquireSlot {
                slot,
                reply,
            })) => RuntimeServingControllerEventV2::AcquireSlot { slot, reply },
            SelectedV2::Command(None) => {
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
                    RuntimeControllerAttemptV2::Staged(staged) => {
                        match self.hold_staged_v2(*staged).await {
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
                            self.cancel_blocked_preflight_v2(preflight_failure_receipt)
                                .await
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
        match stage_ready.stage(&stage, Utc::now()) {
            Ok(staged) => {
                let (session, permit, evidence, hydrated, witness, staged) = staged.into_handoff();
                RuntimeControllerAttemptV2::Staged(Box::new(RuntimeHeldStagedRouteV2 {
                    session,
                    permit,
                    evidence,
                    hydrated,
                    witness,
                    staged,
                }))
            }
            Err(_) => RuntimeControllerAttemptV2::Failed,
        }
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

    async fn hold_staged_v2(
        &mut self,
        mut held: RuntimeHeldStagedRouteV2,
    ) -> RuntimeHeldRouteOutcomeV2 {
        loop {
            if held.ensure_active_v2().is_err() {
                return RuntimeHeldRouteOutcomeV2::Exit(
                    held.finish_v2(RuntimeServingControllerActorExitV2::Failed),
                );
            }
            let renew_at = runtime_renewal_instant_v2(
                held.session.expires_at(),
                self.config.controller_renew_before,
            );
            if runtime_operation_until_stop_v2(&mut self.stop, sleep_until(renew_at))
                .await
                .is_err()
            {
                return RuntimeHeldRouteOutcomeV2::Exit(
                    held.finish_v2(RuntimeServingControllerActorExitV2::Commanded),
                );
            }
            let renewal = match held.session.begin_renewal(self.config.controller_lease_for) {
                Ok(renewal) => renewal,
                Err(_) => {
                    return RuntimeHeldRouteOutcomeV2::Exit(
                        held.finish_v2(RuntimeServingControllerActorExitV2::Failed),
                    )
                }
            };
            let renewal = match runtime_operation_until_stop_v2(
                &mut self.stop,
                self.database.execution().renew_execution(renewal),
            )
            .await
            {
                Err(()) => {
                    return RuntimeHeldRouteOutcomeV2::Exit(
                        held.finish_v2(RuntimeServingControllerActorExitV2::Commanded),
                    );
                }
                Ok(Ok(renewal)) => renewal,
                Ok(Err(error)) if retryable_execution_error_v2(error) => {
                    return held.finish_retry_v2();
                }
                Ok(Err(_)) => {
                    return RuntimeHeldRouteOutcomeV2::Exit(
                        held.finish_v2(RuntimeServingControllerActorExitV2::Failed),
                    );
                }
            };
            if held.session.apply_renewal(renewal).is_err() {
                return RuntimeHeldRouteOutcomeV2::Exit(
                    held.finish_v2(RuntimeServingControllerActorExitV2::Failed),
                );
            }
            let evidence = match held
                .staged
                .advance_authority_v2(held.session.fencing_token())
            {
                Ok(evidence) => evidence,
                Err(_) => {
                    return RuntimeHeldRouteOutcomeV2::Exit(
                        held.finish_v2(RuntimeServingControllerActorExitV2::Failed),
                    )
                }
            };
            held.witness.identity = evidence.identity_v2().clone();
            held.witness.controller_fencing_token = evidence.fencing_token_v2();
            held.witness.route_incarnation = evidence.route_incarnation_v2();
            held.witness.active_interactions = evidence.active_interactions_v2();
            held.witness.admission_generation = evidence.admission_generation_v2();
            held.witness.registry_observation_sequence =
                evidence.registry_observation_sequence_v2().as_non_zero();
        }
    }
}

enum RuntimeControllerAttemptV2 {
    Staged(Box<RuntimeHeldStagedRouteV2>),
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
    Retry,
    Exit(RuntimeServingControllerActorExitV2),
}

struct RuntimeHeldStagedRouteV2 {
    staged: RuntimeRegistryStagedRouteV2,
    session: RuntimeConvergenceSessionV1,
    permit: RuntimeServingSlotWorkPermitV2,
    evidence: RuntimeExactTargetEvidenceV2,
    hydrated: RuntimeExactTargetV1,
    witness: RuntimeRouteWitnessV2,
}

impl RuntimeHeldStagedRouteV2 {
    fn ensure_active_v2(&self) -> Result<(), ()> {
        self.permit.ensure_active().map_err(|_| ())?;
        self.staged.ensure_staged_v2().map_err(|_| ())?;
        if self.witness.identity != *self.staged.identity_v2()
            || self.witness.controller_fencing_token != self.staged.fencing_token_v2()
            || self.evidence.execution().snapshot.target != self.hydrated.snapshot.target
        {
            return Err(());
        }
        Ok(())
    }

    fn finish_v2(
        self,
        success: RuntimeServingControllerActorExitV2,
    ) -> RuntimeServingControllerActorExitV2 {
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
        if cleanup.is_ok() {
            success
        } else {
            RuntimeServingControllerActorExitV2::CleanupFailed
        }
    }

    fn finish_retry_v2(self) -> RuntimeHeldRouteOutcomeV2 {
        match self.finish_v2(RuntimeServingControllerActorExitV2::Failed) {
            RuntimeServingControllerActorExitV2::Failed => RuntimeHeldRouteOutcomeV2::Retry,
            exit => RuntimeHeldRouteOutcomeV2::Exit(exit),
        }
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

struct RuntimeControllerDiscordPreflightPortV2 {
    inner: OwnedDiscordRuntimePreflightV1,
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

struct RuntimeControllerStagePortV2 {
    registry: RuntimeRegistryStagingPortV2,
    emergency: RuntimeRegistryEmergencyTriggerV2,
}

impl RuntimeStagedRoutePortV2<RuntimeExactTargetV1> for RuntimeControllerStagePortV2 {
    type Error = RuntimeControllerStagePortErrorV2;
    type Staged = RuntimeRegistryStagedRouteV2;

    fn install_staged(
        &self,
        request: &RuntimeRouteStageRequestV2,
        hydrated: &RuntimeExactTargetV1,
    ) -> Result<RuntimeRouteStageObservationV2<Self::Staged>, Self::Error> {
        if hydrated.snapshot.target != request.process_identity().target
            || &hydrated.desired_target_digest != request.desired_target_digest()
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
            staged,
        })
    }
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

fn runtime_renewal_instant_v2(expires_at: DateTime<Utc>, renew_before: Duration) -> TokioInstant {
    let renew_before = TimeDelta::from_std(renew_before).unwrap_or(TimeDelta::MAX);
    let renew_at = expires_at
        .checked_sub_signed(renew_before)
        .unwrap_or(expires_at);
    runtime_datetime_deadline_v2(renew_at)
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
    use automation_runtime::RuntimeReadinessSnapshotErrorV1;

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(second, 0).unwrap()
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
    }
}
