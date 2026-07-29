use std::future::Future;
use std::time::{Duration, Instant};

use automation_runtime_worker::{
    RuntimeAcceptedPendingDrainSelectionV3, RuntimeAuthorizedPendingDrainAcknowledgementV2,
    RuntimeAuthorizedPendingDrainClaimV2, RuntimeAuthorizedPendingDrainSelectionV3,
    RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3,
    RuntimePendingDrainAcknowledgementExecutionPortV2, RuntimePendingDrainAcknowledgementReceiptV2,
    RuntimePendingDrainClaimExecutionPortV2, RuntimePendingDrainClaimReceiptV2,
    RuntimePendingDrainCompoundErrorV2, RuntimePendingDrainNoCandidateReceiptV2,
    RuntimePendingDrainNoCandidateRecorderPortV2, RuntimePendingDrainSelectionPortV3,
    RuntimePendingDrainSelectionReceiptV3,
    RuntimePendingDrainSuccessionAcknowledgementExecutionPortV3,
    RuntimePendingDrainSuccessionAcknowledgementReceiptV3,
    RuntimeSelectedPendingDrainNoCandidateV2, RuntimeStartupRecoveryClassV2,
    RuntimeStartupRecoveryContinuationV2, RuntimeStartupRecoveryExecutionPortV2,
    RuntimeStartupRecoveryExecutionReceiptOutcomeV2,
};
use tokio::time::{sleep_until, Instant as TokioInstant};

use super::connected::{
    discord_transition_failure_v1, map_discord_transition_exit_v1,
    RuntimeProcessPausedConnectedTransitionFailureV1,
};
use super::observation::RuntimeStartupRecoveryContinueProcessV2;
use super::pending_drain_finalizer::{
    complete_pending_drain_job_v3, register_pending_drain_job_v3,
    RuntimePendingDrainFinalizerDispatchFailureV3, RuntimePendingDrainFinalizerJobV3,
    RuntimePendingDrainMutationEnvironmentV3, RuntimePendingDrainMutationOutputV3,
    RuntimePendingDrainMutationStageV3,
};
use super::startup_loop::RuntimeProcessStartupRecoveryLoopFailureV2;
use super::RuntimeProcessFoundationV1;
use crate::closed_recovery::RuntimeClosedRecoverySessionV2;
use crate::discord::{RuntimeDiscordGatewayObservationV1, RuntimeDiscordGatewaySupervisorV1};

pub(crate) struct RuntimeStartupRecoveryExecutionCompletionV2 {
    retry_after: Option<Duration>,
}

pub(crate) enum RuntimeOwnedStartupRecoveryExecutionOutcomeV3 {
    Completed(
        RuntimeStartupRecoveryContinueProcessV2,
        RuntimeStartupRecoveryExecutionCompletionV2,
    ),
    Failed(
        RuntimeStartupRecoveryContinueProcessV2,
        RuntimeProcessStartupRecoveryLoopFailureV2,
    ),
    Terminal(super::startup_loop::RuntimeProcessStartupRecoveryLoopErrorV2),
}

impl RuntimeStartupRecoveryExecutionCompletionV2 {
    fn completed_v2() -> Self {
        Self { retry_after: None }
    }

    fn retry_after_v2(retry_after: Duration) -> Self {
        Self {
            retry_after: Some(retry_after),
        }
    }

    pub(super) fn bounded_retry_after_v2(&self) -> Option<Duration> {
        self.retry_after
    }
}

pub(crate) enum RuntimeStartupRecoveryExecutionAwaitFailureV2<E> {
    Transition(RuntimeProcessStartupRecoveryLoopFailureV2),
    Database(E),
}

pub(crate) trait RuntimePendingDrainRecoveryEnvironmentV2 {
    fn current_transition_v2(
        &self,
        session: &RuntimeClosedRecoverySessionV2,
    ) -> Option<RuntimeProcessStartupRecoveryLoopFailureV2>;

    fn select_pending_drain_v3<'a>(
        &'a mut self,
        session: &'a RuntimeClosedRecoverySessionV2,
        authorization: &'a RuntimeAuthorizedPendingDrainSelectionV3,
    ) -> impl Future<
        Output = Result<
            RuntimePendingDrainSelectionReceiptV3,
            RuntimeStartupRecoveryExecutionAwaitFailureV2<
                automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1,
            >,
        >,
    > + Send
           + 'a;

    #[cfg_attr(not(test), allow(dead_code))]
    fn record_pending_drain_no_candidate_v2<'a>(
        &'a mut self,
        session: &'a RuntimeClosedRecoverySessionV2,
        selection: &'a RuntimeSelectedPendingDrainNoCandidateV2,
    ) -> impl Future<
        Output = Result<
            RuntimePendingDrainNoCandidateReceiptV2,
            RuntimeStartupRecoveryExecutionAwaitFailureV2<
                automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1,
            >,
        >,
    > + Send
           + 'a;

    #[cfg_attr(not(test), allow(dead_code))]
    fn execute_pending_drain_claim_v2<'a>(
        &'a mut self,
        session: &'a RuntimeClosedRecoverySessionV2,
        authorization: &'a RuntimeAuthorizedPendingDrainClaimV2,
    ) -> impl Future<
        Output = Result<
            RuntimePendingDrainClaimReceiptV2,
            RuntimeStartupRecoveryExecutionAwaitFailureV2<
                automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1,
            >,
        >,
    > + Send
           + 'a;

    #[cfg_attr(not(test), allow(dead_code))]
    fn execute_pending_drain_acknowledgement_v2<'a>(
        &'a mut self,
        session: &'a RuntimeClosedRecoverySessionV2,
        authorization: &'a RuntimeAuthorizedPendingDrainAcknowledgementV2,
    ) -> impl Future<
        Output = Result<
            RuntimePendingDrainAcknowledgementReceiptV2,
            RuntimeStartupRecoveryExecutionAwaitFailureV2<
                automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1,
            >,
        >,
    > + Send
           + 'a;

    #[cfg_attr(not(test), allow(dead_code))]
    fn execute_pending_drain_succession_v3<'a>(
        &'a mut self,
        session: &'a RuntimeClosedRecoverySessionV2,
        authorization: &'a RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3,
    ) -> impl Future<
        Output = Result<
            RuntimePendingDrainSuccessionAcknowledgementReceiptV3,
            RuntimeStartupRecoveryExecutionAwaitFailureV2<
                automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1,
            >,
        >,
    > + Send
           + 'a;
}

struct RuntimeProductionPendingDrainRecoveryEnvironmentV2<'a> {
    discord: &'a mut RuntimeDiscordGatewaySupervisorV1,
    foundation: &'a RuntimeProcessFoundationV1,
}

pub(super) struct RuntimeProductionPendingDrainFinalizerEnvironmentV3 {
    discord: RuntimeDiscordGatewayObservationV1,
    database: crate::database::RuntimePendingDrainMutationDatabaseV3,
    operation_cutoff: Instant,
    shutdown: crate::process_supervisor::RuntimeProcessShutdownTriggerV1,
    shutdown_observer: crate::shutdown::RuntimeShutdownObserverV1,
}

impl RuntimeProductionPendingDrainFinalizerEnvironmentV3 {
    fn new(
        discord: &RuntimeDiscordGatewaySupervisorV1,
        foundation: &RuntimeProcessFoundationV1,
    ) -> Self {
        Self {
            discord: discord.observation_v1(),
            database: foundation.databases.pending_drain_mutation_v3(),
            operation_cutoff: foundation.startup_budget.operation_cutoff(),
            shutdown: foundation.shutdown_trigger_v1(),
            shutdown_observer: foundation.shutdown_observer_v1(),
        }
    }
}

impl RuntimeStartupRecoveryContinueProcessV2 {
    pub(super) async fn execute_startup_recovery_owned_v3(
        mut self,
        class: RuntimeStartupRecoveryClassV2,
    ) -> RuntimeOwnedStartupRecoveryExecutionOutcomeV3 {
        if class == RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent {
            return execute_pending_drain_recovery_owned_v3(self).await;
        }
        match self.execute_startup_recovery_in_place_v2(class).await {
            Ok(completion) => {
                RuntimeOwnedStartupRecoveryExecutionOutcomeV3::Completed(self, completion)
            }
            Err(failure) => RuntimeOwnedStartupRecoveryExecutionOutcomeV3::Failed(self, failure),
        }
    }

    pub(super) async fn execute_startup_recovery_in_place_v2(
        &mut self,
        class: RuntimeStartupRecoveryClassV2,
    ) -> Result<
        RuntimeStartupRecoveryExecutionCompletionV2,
        RuntimeProcessStartupRecoveryLoopFailureV2,
    > {
        if !matches!(
            class,
            RuntimeStartupRecoveryClassV2::StaleLive
                | RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification
                | RuntimeStartupRecoveryClassV2::SuspendedLocalEffect
        ) {
            return Err(super::startup_loop::unavailable_recovery_failure_v2(class));
        }
        if let Some(transition) = current_startup_recovery_execution_transition_v2(self) {
            return Err(transition);
        }
        let authorization = match self.session.begin_startup_recovery_execution_v2(
            RuntimeStartupRecoveryContinuationV2::Recover(class),
        ) {
            Ok(authorization) => authorization,
            Err(error) => {
                let transition = prefer_current_startup_recovery_execution_failure_v2(
                    current_startup_recovery_execution_transition_v2(self),
                    || startup_recovery_execution_rejected_v2(class, error.into()),
                );
                self.session.invalidate_startup_recovery_execution_v2();
                return Err(transition);
            }
        };
        if let Some(transition) = current_startup_recovery_execution_transition_v2(self) {
            self.session.invalidate_startup_recovery_execution_v2();
            return Err(transition);
        }
        let operation_cutoff = self.foundation.startup_budget.operation_cutoff();
        let owner_safety_deadline = self.session.owner_safety_deadline_v2();
        let execution_cutoff = operation_cutoff.min(owner_safety_deadline);
        let deadline_failure = classify_startup_recovery_execution_deadline_v2(
            operation_cutoff,
            owner_safety_deadline,
        );
        let owner_terminal = self.session.owner_terminal_observation_v2();
        let discord_terminal = async {
            let transition =
                map_discord_transition_exit_v1(self.discord.wait_terminal().await.exit());
            self.foundation
                .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::DiscordTerminal);
            transition
        };
        let owner_terminal = async {
            let _exit = owner_terminal.await;
            self.foundation
                .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::GatewayOwnerTerminal);
        };
        let mut shutdown = self.foundation.shutdown_observer_v1();
        let database = self
            .foundation
            .databases
            .execution()
            .execute_startup_recovery(authorization, execution_cutoff);
        let completed = match await_startup_recovery_execution_v2(
            deadline_failure,
            sleep_until(TokioInstant::from_std(execution_cutoff)),
            discord_terminal,
            owner_terminal,
            database,
            async move { shutdown.wait().await },
        )
        .await
        {
            Ok(completed) => completed,
            Err(RuntimeStartupRecoveryExecutionAwaitFailureV2::Transition(transition)) => {
                self.session.invalidate_startup_recovery_execution_v2();
                return Err(transition);
            }
            Err(RuntimeStartupRecoveryExecutionAwaitFailureV2::Database(error)) => {
                let transition = prefer_current_startup_recovery_execution_failure_v2(
                    current_startup_recovery_execution_transition_v2(self),
                    || startup_recovery_execution_database_failure_v2(class, error),
                );
                self.session.invalidate_startup_recovery_execution_v2();
                return Err(transition);
            }
        };
        if let Some(transition) = current_startup_recovery_execution_transition_v2(self) {
            self.session.invalidate_startup_recovery_execution_v2();
            return Err(transition);
        }
        let accepted = match self
            .session
            .complete_startup_recovery_execution_v2(completed)
        {
            Ok(accepted) => accepted,
            Err(error) => {
                let transition = prefer_current_startup_recovery_execution_failure_v2(
                    current_startup_recovery_execution_transition_v2(self),
                    || startup_recovery_execution_rejected_v2(class, error.into()),
                );
                self.session.invalidate_startup_recovery_execution_v2();
                return Err(transition);
            }
        };
        if let Some(transition) = current_startup_recovery_execution_transition_v2(self) {
            self.session.invalidate_startup_recovery_execution_v2();
            return Err(transition);
        }
        if accepted.class() != class {
            self.session.invalidate_startup_recovery_execution_v2();
            return Err(startup_recovery_execution_rejected_v2(
                class,
                crate::RuntimeProcessClosedRecoveryCommitFailureV2::GatewayProtocolViolation,
            ));
        }
        match accepted.outcome() {
            RuntimeStartupRecoveryExecutionReceiptOutcomeV2::Progressed { .. }
            | RuntimeStartupRecoveryExecutionReceiptOutcomeV2::NoCandidate => {
                Ok(RuntimeStartupRecoveryExecutionCompletionV2::completed_v2())
            }
            RuntimeStartupRecoveryExecutionReceiptOutcomeV2::RetryAfter { .. } => {
                self.session.invalidate_startup_recovery_execution_v2();
                Err(startup_recovery_execution_retry_after_unsupported_v2(class))
            }
        }
    }
}

enum RuntimePendingDrainOwnedStageFailureV3 {
    Retained {
        session: Box<RuntimeClosedRecoverySessionV2>,
        failure: RuntimeProcessStartupRecoveryLoopFailureV2,
    },
    Lost,
}

async fn execute_pending_drain_recovery_owned_v3(
    process: RuntimeStartupRecoveryContinueProcessV2,
) -> RuntimeOwnedStartupRecoveryExecutionOutcomeV3 {
    let RuntimeStartupRecoveryContinueProcessV2 {
        mut discord,
        mut foundation,
        mut session,
        continuation,
    } = process;
    let class = RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent;
    let selection = {
        let mut environment = RuntimeProductionPendingDrainRecoveryEnvironmentV2 {
            discord: &mut discord,
            foundation: &foundation,
        };
        if let Some(transition) = environment.current_transition_v2(&session) {
            return pending_drain_owned_failure_v3(
                discord,
                foundation,
                session,
                continuation,
                transition,
            );
        }
        let authorization = match session.begin_startup_recovery_execution_v2(
            RuntimeStartupRecoveryContinuationV2::Recover(class),
        ) {
            Ok(authorization) => authorization,
            Err(error) => {
                let failure = startup_recovery_execution_rejected_v2(class, error.into());
                return pending_drain_owned_failure_v3(
                    discord,
                    foundation,
                    session,
                    continuation,
                    failure,
                );
            }
        };
        let authorization = match authorization.into_pending_drain_selection_v3() {
            Ok(authorization) => authorization,
            Err(error) => {
                return pending_drain_owned_failure_v3(
                    discord,
                    foundation,
                    session,
                    continuation,
                    pending_drain_compound_failure_v2(error),
                );
            }
        };
        let receipt = match environment
            .select_pending_drain_v3(&session, &authorization)
            .await
        {
            Ok(receipt) => receipt,
            Err(error) => {
                let failure =
                    map_pending_drain_database_await_failure_v2(&environment, &session, error);
                return pending_drain_owned_failure_v3(
                    discord,
                    foundation,
                    session,
                    continuation,
                    failure,
                );
            }
        };
        if let Err(failure) = revalidate_pending_drain_stage_v2(&environment, &session) {
            return pending_drain_owned_failure_v3(
                discord,
                foundation,
                session,
                continuation,
                failure,
            );
        }
        match authorization.accept_selection(receipt) {
            Ok(selection) => selection,
            Err(error) => {
                return pending_drain_owned_failure_v3(
                    discord,
                    foundation,
                    session,
                    continuation,
                    pending_drain_compound_failure_v2(error),
                );
            }
        }
    };
    let completed = match selection {
        RuntimeAcceptedPendingDrainSelectionV3::NoCandidate(selection) => {
            let stage = RuntimePendingDrainMutationStageV3::NoCandidate(selection);
            let output = match execute_owned_pending_drain_stage_v3(
                &discord,
                &mut foundation,
                session,
                stage,
            )
            .await
            {
                Ok(result) => result,
                Err(RuntimePendingDrainOwnedStageFailureV3::Retained {
                    session: returned,
                    failure,
                }) => {
                    return pending_drain_owned_failure_v3(
                        discord,
                        foundation,
                        *returned,
                        continuation,
                        failure,
                    );
                }
                Err(RuntimePendingDrainOwnedStageFailureV3::Lost) => {
                    return pending_drain_owned_terminal_v3(discord, foundation).await;
                }
            };
            session = output.0;
            match output.1 {
                RuntimePendingDrainMutationOutputV3::Completed(completed) => completed,
                RuntimePendingDrainMutationOutputV3::ClaimAccepted(_) => {
                    return pending_drain_owned_failure_v3(
                        discord,
                        foundation,
                        session,
                        continuation,
                        RuntimeProcessStartupRecoveryLoopFailureV2::ProtocolViolation,
                    );
                }
            }
        }
        RuntimeAcceptedPendingDrainSelectionV3::Unclaimed(selection) => {
            let seal = match session.seal_pending_drain_candidate_v2(selection.candidate()) {
                Ok(seal) => seal,
                Err(error) => {
                    return pending_drain_owned_failure_v3(
                        discord,
                        foundation,
                        session,
                        continuation,
                        startup_recovery_execution_rejected_v2(class, error.into()),
                    );
                }
            };
            if let Some(transition) = current_startup_recovery_execution_transition_from_parts_v2(
                &foundation,
                &discord,
                &session,
            ) {
                return pending_drain_owned_failure_v3(
                    discord,
                    foundation,
                    session,
                    continuation,
                    transition,
                );
            }
            let claim = match selection.bind_registry_seal(seal) {
                Ok(claim) => claim,
                Err(error) => {
                    return pending_drain_owned_failure_v3(
                        discord,
                        foundation,
                        session,
                        continuation,
                        pending_drain_compound_failure_v2(error),
                    );
                }
            };
            let claim_output = match execute_owned_pending_drain_stage_v3(
                &discord,
                &mut foundation,
                session,
                RuntimePendingDrainMutationStageV3::Claim(Box::new(claim)),
            )
            .await
            {
                Ok(result) => result,
                Err(RuntimePendingDrainOwnedStageFailureV3::Retained {
                    session: returned,
                    failure,
                }) => {
                    return pending_drain_owned_failure_v3(
                        discord,
                        foundation,
                        *returned,
                        continuation,
                        failure,
                    );
                }
                Err(RuntimePendingDrainOwnedStageFailureV3::Lost) => {
                    return pending_drain_owned_terminal_v3(discord, foundation).await;
                }
            };
            session = claim_output.0;
            let acknowledgement = match claim_output.1 {
                RuntimePendingDrainMutationOutputV3::ClaimAccepted(acknowledgement) => {
                    acknowledgement
                }
                RuntimePendingDrainMutationOutputV3::Completed(_) => {
                    return pending_drain_owned_failure_v3(
                        discord,
                        foundation,
                        session,
                        continuation,
                        RuntimeProcessStartupRecoveryLoopFailureV2::ProtocolViolation,
                    );
                }
            };
            let acknowledgement_output = match execute_owned_pending_drain_stage_v3(
                &discord,
                &mut foundation,
                session,
                RuntimePendingDrainMutationStageV3::Acknowledgement(Box::new(acknowledgement)),
            )
            .await
            {
                Ok(result) => result,
                Err(RuntimePendingDrainOwnedStageFailureV3::Retained {
                    session: returned,
                    failure,
                }) => {
                    return pending_drain_owned_failure_v3(
                        discord,
                        foundation,
                        *returned,
                        continuation,
                        failure,
                    );
                }
                Err(RuntimePendingDrainOwnedStageFailureV3::Lost) => {
                    return pending_drain_owned_terminal_v3(discord, foundation).await;
                }
            };
            session = acknowledgement_output.0;
            match acknowledgement_output.1 {
                RuntimePendingDrainMutationOutputV3::Completed(completed) => completed,
                RuntimePendingDrainMutationOutputV3::ClaimAccepted(_) => {
                    return pending_drain_owned_failure_v3(
                        discord,
                        foundation,
                        session,
                        continuation,
                        RuntimeProcessStartupRecoveryLoopFailureV2::ProtocolViolation,
                    );
                }
            }
        }
        RuntimeAcceptedPendingDrainSelectionV3::FreshPreviousOwner(selection) => {
            selection.complete()
        }
        RuntimeAcceptedPendingDrainSelectionV3::ExpiredPreviousOwner(selection) => {
            let seal =
                match session.seal_pending_drain_succession_candidate_v3(selection.candidate()) {
                    Ok(seal) => seal,
                    Err(error) => {
                        return pending_drain_owned_failure_v3(
                            discord,
                            foundation,
                            session,
                            continuation,
                            startup_recovery_execution_rejected_v2(class, error.into()),
                        );
                    }
                };
            if let Some(transition) = current_startup_recovery_execution_transition_from_parts_v2(
                &foundation,
                &discord,
                &session,
            ) {
                return pending_drain_owned_failure_v3(
                    discord,
                    foundation,
                    session,
                    continuation,
                    transition,
                );
            }
            let succession = match selection.bind_registry_seal(seal) {
                Ok(succession) => succession,
                Err(error) => {
                    return pending_drain_owned_failure_v3(
                        discord,
                        foundation,
                        session,
                        continuation,
                        pending_drain_compound_failure_v2(error),
                    );
                }
            };
            let output = match execute_owned_pending_drain_stage_v3(
                &discord,
                &mut foundation,
                session,
                RuntimePendingDrainMutationStageV3::Succession(Box::new(succession)),
            )
            .await
            {
                Ok(result) => result,
                Err(RuntimePendingDrainOwnedStageFailureV3::Retained {
                    session: returned,
                    failure,
                }) => {
                    return pending_drain_owned_failure_v3(
                        discord,
                        foundation,
                        *returned,
                        continuation,
                        failure,
                    );
                }
                Err(RuntimePendingDrainOwnedStageFailureV3::Lost) => {
                    return pending_drain_owned_terminal_v3(discord, foundation).await;
                }
            };
            session = output.0;
            match output.1 {
                RuntimePendingDrainMutationOutputV3::Completed(completed) => completed,
                RuntimePendingDrainMutationOutputV3::ClaimAccepted(_) => {
                    return pending_drain_owned_failure_v3(
                        discord,
                        foundation,
                        session,
                        continuation,
                        RuntimeProcessStartupRecoveryLoopFailureV2::ProtocolViolation,
                    );
                }
            }
        }
    };
    if let Some(transition) =
        current_startup_recovery_execution_transition_from_parts_v2(&foundation, &discord, &session)
    {
        return pending_drain_owned_failure_v3(
            discord,
            foundation,
            session,
            continuation,
            transition,
        );
    }
    let accepted = match session.complete_startup_recovery_execution_v2(completed) {
        Ok(accepted) => accepted,
        Err(error) => {
            return pending_drain_owned_failure_v3(
                discord,
                foundation,
                session,
                continuation,
                startup_recovery_execution_rejected_v2(class, error.into()),
            );
        }
    };
    if accepted.class() != class {
        return pending_drain_owned_failure_v3(
            discord,
            foundation,
            session,
            continuation,
            RuntimeProcessStartupRecoveryLoopFailureV2::ProtocolViolation,
        );
    }
    let completion = match accepted.outcome() {
        RuntimeStartupRecoveryExecutionReceiptOutcomeV2::Progressed { .. }
        | RuntimeStartupRecoveryExecutionReceiptOutcomeV2::NoCandidate => {
            RuntimeStartupRecoveryExecutionCompletionV2::completed_v2()
        }
        RuntimeStartupRecoveryExecutionReceiptOutcomeV2::RetryAfter { retry_after } => {
            RuntimeStartupRecoveryExecutionCompletionV2::retry_after_v2(*retry_after)
        }
    };
    RuntimeOwnedStartupRecoveryExecutionOutcomeV3::Completed(
        RuntimeStartupRecoveryContinueProcessV2 {
            discord,
            foundation,
            session,
            continuation,
        },
        completion,
    )
}

async fn execute_owned_pending_drain_stage_v3(
    discord: &RuntimeDiscordGatewaySupervisorV1,
    foundation: &mut RuntimeProcessFoundationV1,
    session: RuntimeClosedRecoverySessionV2,
    stage: RuntimePendingDrainMutationStageV3,
) -> Result<
    (
        RuntimeClosedRecoverySessionV2,
        RuntimePendingDrainMutationOutputV3,
    ),
    RuntimePendingDrainOwnedStageFailureV3,
> {
    let environment = RuntimeProductionPendingDrainFinalizerEnvironmentV3::new(discord, foundation);
    let job = RuntimePendingDrainFinalizerJobV3::new(session, environment, stage);
    let registered = match foundation.mutation_finalizer.as_ref() {
        Some(supervisor) => match register_pending_drain_job_v3(supervisor, job) {
            Ok(registered) => registered,
            Err(failure) => return resolve_pending_drain_finalizer_dispatch_v3(Err(failure)),
        },
        None => {
            let (session, environment, _) = job.into_parts();
            drop(environment);
            return Err(RuntimePendingDrainOwnedStageFailureV3::Retained {
                session: Box::new(session),
                failure:
                    RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainRecoveryUnavailable,
            });
        }
    };
    let mut shutdown = foundation.root_supervisor.observer();
    let completion = {
        let supervisor = foundation
            .mutation_finalizer
            .as_mut()
            .expect("registered pending drain finalizer");
        tokio::select! {
            biased;
            completion = supervisor.next_completion() => Ok(completion),
            observation = shutdown.wait() => Err(observation),
        }
    };
    let dispatch = match completion {
        Ok(Some(completion)) => complete_pending_drain_job_v3(registered, completion),
        Ok(None) => Err(RuntimePendingDrainFinalizerDispatchFailureV3::CompletionChannelClosed),
        Err(observation) => {
            let shutdown_deadline = foundation.effective_shutdown_deadline_v1(observation);
            let supervisor = foundation
                .mutation_finalizer
                .take()
                .expect("registered pending drain finalizer");
            let report = supervisor.shutdown_until(shutdown_deadline).await;
            foundation.record_finalizer_shutdown_exit_v1(report.exit());
            let mut completions = report.into_completions();
            if completions.len() != 1 {
                return Err(RuntimePendingDrainOwnedStageFailureV3::Lost);
            }
            complete_pending_drain_job_v3(registered, completions.remove(0))
        }
    };
    resolve_pending_drain_finalizer_dispatch_v3(dispatch)
}

fn resolve_pending_drain_finalizer_dispatch_v3(
    dispatch: Result<
        super::pending_drain_finalizer::RuntimePendingDrainFinalizerSettledV3<
            RuntimeProductionPendingDrainFinalizerEnvironmentV3,
        >,
        RuntimePendingDrainFinalizerDispatchFailureV3<
            RuntimeProductionPendingDrainFinalizerEnvironmentV3,
        >,
    >,
) -> Result<
    (
        RuntimeClosedRecoverySessionV2,
        RuntimePendingDrainMutationOutputV3,
    ),
    RuntimePendingDrainOwnedStageFailureV3,
> {
    match dispatch {
        Ok(settled) => {
            let (session, environment, output) = settled.into_parts();
            drop(environment);
            Ok((session, output))
        }
        Err(RuntimePendingDrainFinalizerDispatchFailureV3::Rejected { job, reason }) => {
            let (session, environment, _) = (*job).into_parts();
            if matches!(
                reason,
                crate::RuntimeMutationFinalizerRegistrationRejectionReasonV1::SupervisorTerminal(_)
            ) {
                environment
                    .shutdown
                    .trip(crate::RuntimeShutdownCauseV1::FinalizerTerminal);
            }
            drop(environment);
            Err(RuntimePendingDrainOwnedStageFailureV3::Retained {
                session: Box::new(session),
                failure: map_pending_drain_finalizer_rejection_v3(reason),
            })
        }
        Err(RuntimePendingDrainFinalizerDispatchFailureV3::Failed(failed)) => {
            let (session, environment, failure) = (*failed).into_parts();
            drop(environment);
            Err(RuntimePendingDrainOwnedStageFailureV3::Retained {
                session: Box::new(session),
                failure,
            })
        }
        Err(RuntimePendingDrainFinalizerDispatchFailureV3::Undispatched { job, exit }) => {
            let (session, environment, _) = (*job).into_parts();
            environment
                .shutdown
                .trip(crate::RuntimeShutdownCauseV1::FinalizerTerminal);
            drop(environment);
            Err(RuntimePendingDrainOwnedStageFailureV3::Retained {
                session: Box::new(session),
                failure: map_pending_drain_finalizer_exit_v3(exit),
            })
        }
        Err(RuntimePendingDrainFinalizerDispatchFailureV3::DispatchedTerminal(exit)) => {
            let _failure = map_pending_drain_finalizer_exit_v3(exit);
            Err(RuntimePendingDrainOwnedStageFailureV3::Lost)
        }
        Err(RuntimePendingDrainFinalizerDispatchFailureV3::CompletionChannelClosed) => {
            Err(RuntimePendingDrainOwnedStageFailureV3::Lost)
        }
        Err(RuntimePendingDrainFinalizerDispatchFailureV3::CompletionIdentityMismatch {
            expected,
            actual,
        }) => {
            let _identity = (expected, actual);
            Err(RuntimePendingDrainOwnedStageFailureV3::Lost)
        }
    }
}

fn map_pending_drain_finalizer_rejection_v3(
    reason: crate::RuntimeMutationFinalizerRegistrationRejectionReasonV1,
) -> RuntimeProcessStartupRecoveryLoopFailureV2 {
    match reason {
        crate::RuntimeMutationFinalizerRegistrationRejectionReasonV1::Busy
        | crate::RuntimeMutationFinalizerRegistrationRejectionReasonV1::IntakeSealed => {
            RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainRecoveryUnavailable
        }
        crate::RuntimeMutationFinalizerRegistrationRejectionReasonV1::SupervisorTerminal(exit) => {
            map_pending_drain_finalizer_exit_v3(exit)
        }
    }
}

fn map_pending_drain_finalizer_exit_v3(
    exit: crate::RuntimeSupervisorExitV1,
) -> RuntimeProcessStartupRecoveryLoopFailureV2 {
    match exit {
        crate::RuntimeSupervisorExitV1::DependencyTerminal
        | crate::RuntimeSupervisorExitV1::DeadlineElapsed => {
            RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainRecoveryUnavailable
        }
        crate::RuntimeSupervisorExitV1::Commanded
        | crate::RuntimeSupervisorExitV1::ProtocolViolation
        | crate::RuntimeSupervisorExitV1::Panicked
        | crate::RuntimeSupervisorExitV1::Aborted => {
            RuntimeProcessStartupRecoveryLoopFailureV2::ProtocolViolation
        }
    }
}

fn pending_drain_owned_failure_v3(
    discord: RuntimeDiscordGatewaySupervisorV1,
    foundation: RuntimeProcessFoundationV1,
    session: RuntimeClosedRecoverySessionV2,
    continuation: RuntimeStartupRecoveryContinuationV2,
    failure: RuntimeProcessStartupRecoveryLoopFailureV2,
) -> RuntimeOwnedStartupRecoveryExecutionOutcomeV3 {
    session.invalidate_startup_recovery_execution_v2();
    RuntimeOwnedStartupRecoveryExecutionOutcomeV3::Failed(
        RuntimeStartupRecoveryContinueProcessV2 {
            discord,
            foundation,
            session,
            continuation,
        },
        failure,
    )
}

async fn pending_drain_owned_terminal_v3(
    discord: RuntimeDiscordGatewaySupervisorV1,
    foundation: RuntimeProcessFoundationV1,
) -> RuntimeOwnedStartupRecoveryExecutionOutcomeV3 {
    foundation.trip_shutdown_v1(crate::RuntimeShutdownCauseV1::FinalizerTerminal);
    super::connected::shutdown_paused_foundation_discord_v1(foundation, discord).await;
    RuntimeOwnedStartupRecoveryExecutionOutcomeV3::Terminal(
        super::startup_loop::RuntimeProcessStartupRecoveryLoopErrorV2::Transition(
            RuntimeProcessStartupRecoveryLoopFailureV2::ProtocolViolation,
        ),
    )
}

#[cfg(test)]
pub(crate) async fn execute_pending_drain_recovery_with_environment_v2<Environment>(
    session: &mut RuntimeClosedRecoverySessionV2,
    environment: &mut Environment,
) -> Result<RuntimeStartupRecoveryExecutionCompletionV2, RuntimeProcessStartupRecoveryLoopFailureV2>
where
    Environment: RuntimePendingDrainRecoveryEnvironmentV2,
{
    let result = try_execute_pending_drain_recovery_with_environment_v2(session, environment).await;
    if result.is_err() {
        session.invalidate_startup_recovery_execution_v2();
    }
    result
}

#[cfg(test)]
async fn try_execute_pending_drain_recovery_with_environment_v2<Environment>(
    session: &mut RuntimeClosedRecoverySessionV2,
    environment: &mut Environment,
) -> Result<RuntimeStartupRecoveryExecutionCompletionV2, RuntimeProcessStartupRecoveryLoopFailureV2>
where
    Environment: RuntimePendingDrainRecoveryEnvironmentV2,
{
    let class = RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent;
    if let Some(transition) = environment.current_transition_v2(session) {
        return Err(transition);
    }
    let authorization = session
        .begin_startup_recovery_execution_v2(RuntimeStartupRecoveryContinuationV2::Recover(class))
        .map_err(|error| startup_recovery_execution_rejected_v2(class, error.into()))?
        .into_pending_drain_selection_v3()
        .map_err(pending_drain_compound_failure_v2)?;
    let selection_receipt = environment
        .select_pending_drain_v3(session, &authorization)
        .await
        .map_err(|error| {
            map_pending_drain_database_await_failure_v2(environment, session, error)
        })?;
    revalidate_pending_drain_stage_v2(environment, session)?;
    let selection = authorization
        .accept_selection(selection_receipt)
        .map_err(pending_drain_compound_failure_v2)?;
    let completed = match selection {
        RuntimeAcceptedPendingDrainSelectionV3::NoCandidate(selection) => {
            let receipt = match environment
                .record_pending_drain_no_candidate_v2(session, &selection)
                .await
            {
                Err(error) if pending_drain_requires_exact_finalization_v2(&error) => {
                    revalidate_pending_drain_stage_v2(environment, session)?;
                    environment
                        .record_pending_drain_no_candidate_v2(session, &selection)
                        .await
                }
                result => result,
            }
            .map_err(|error| {
                map_pending_drain_database_await_failure_v2(environment, session, error)
            })?;
            revalidate_pending_drain_stage_v2(environment, session)?;
            selection
                .complete(receipt)
                .map_err(pending_drain_compound_failure_v2)?
        }
        RuntimeAcceptedPendingDrainSelectionV3::Unclaimed(selection) => {
            let seal = session
                .seal_pending_drain_candidate_v2(selection.candidate())
                .map_err(|error| startup_recovery_execution_rejected_v2(class, error.into()))?;
            if let Some(transition) = environment.current_transition_v2(session) {
                return Err(transition);
            }
            let claim = selection
                .bind_registry_seal(seal)
                .map_err(pending_drain_compound_failure_v2)?;
            let claim_receipt = match environment
                .execute_pending_drain_claim_v2(session, &claim)
                .await
            {
                Err(error) if pending_drain_requires_exact_finalization_v2(&error) => {
                    revalidate_pending_drain_stage_v2(environment, session)?;
                    environment
                        .execute_pending_drain_claim_v2(session, &claim)
                        .await
                }
                result => result,
            }
            .map_err(|error| {
                map_pending_drain_database_await_failure_v2(environment, session, error)
            })?;
            revalidate_pending_drain_stage_v2(environment, session)?;
            let acknowledgement = claim
                .complete(claim_receipt)
                .map_err(pending_drain_compound_failure_v2)?;
            let acknowledgement_receipt = match environment
                .execute_pending_drain_acknowledgement_v2(session, &acknowledgement)
                .await
            {
                Err(error) if pending_drain_requires_exact_finalization_v2(&error) => {
                    revalidate_pending_drain_stage_v2(environment, session)?;
                    environment
                        .execute_pending_drain_acknowledgement_v2(session, &acknowledgement)
                        .await
                }
                result => result,
            }
            .map_err(|error| {
                map_pending_drain_database_await_failure_v2(environment, session, error)
            })?;
            revalidate_pending_drain_stage_v2(environment, session)?;
            let durable = acknowledgement
                .complete(acknowledgement_receipt)
                .map_err(pending_drain_compound_failure_v2)?;
            let unseal = session
                .unseal_pending_drain_after_durable_ack_v2(&durable)
                .map_err(|error| startup_recovery_execution_rejected_v2(class, error.into()))?;
            durable
                .complete_registry_rollover(unseal)
                .map_err(pending_drain_compound_failure_v2)?
        }
        RuntimeAcceptedPendingDrainSelectionV3::FreshPreviousOwner(selection) => {
            selection.complete()
        }
        RuntimeAcceptedPendingDrainSelectionV3::ExpiredPreviousOwner(selection) => {
            let seal = session
                .seal_pending_drain_succession_candidate_v3(selection.candidate())
                .map_err(|error| startup_recovery_execution_rejected_v2(class, error.into()))?;
            if let Some(transition) = environment.current_transition_v2(session) {
                return Err(transition);
            }
            let succession = selection
                .bind_registry_seal(seal)
                .map_err(pending_drain_compound_failure_v2)?;
            let receipt = match environment
                .execute_pending_drain_succession_v3(session, &succession)
                .await
            {
                Err(error) if pending_drain_requires_exact_finalization_v2(&error) => {
                    revalidate_pending_drain_stage_v2(environment, session)?;
                    environment
                        .execute_pending_drain_succession_v3(session, &succession)
                        .await
                }
                result => result,
            }
            .map_err(|error| {
                map_pending_drain_database_await_failure_v2(environment, session, error)
            })?;
            revalidate_pending_drain_stage_v2(environment, session)?;
            let durable = succession
                .complete(receipt)
                .map_err(pending_drain_compound_failure_v2)?;
            revalidate_pending_drain_stage_v2(environment, session)?;
            let unseal = session
                .unseal_pending_drain_after_durable_succession_v3(&durable)
                .map_err(|error| startup_recovery_execution_rejected_v2(class, error.into()))?;
            durable
                .complete_registry_rollover(unseal)
                .map_err(pending_drain_compound_failure_v2)?
        }
    };
    if let Some(transition) = environment.current_transition_v2(session) {
        return Err(transition);
    }
    let accepted = session
        .complete_startup_recovery_execution_v2(completed)
        .map_err(|error| startup_recovery_execution_rejected_v2(class, error.into()))?;
    if accepted.class() != class {
        return Err(startup_recovery_execution_rejected_v2(
            class,
            crate::RuntimeProcessClosedRecoveryCommitFailureV2::GatewayProtocolViolation,
        ));
    }
    match accepted.outcome() {
        RuntimeStartupRecoveryExecutionReceiptOutcomeV2::Progressed { .. }
        | RuntimeStartupRecoveryExecutionReceiptOutcomeV2::NoCandidate => {
            Ok(RuntimeStartupRecoveryExecutionCompletionV2::completed_v2())
        }
        RuntimeStartupRecoveryExecutionReceiptOutcomeV2::RetryAfter { retry_after } => Ok(
            RuntimeStartupRecoveryExecutionCompletionV2::retry_after_v2(*retry_after),
        ),
    }
}

#[cfg(test)]
fn pending_drain_requires_exact_finalization_v2(
    error: &RuntimeStartupRecoveryExecutionAwaitFailureV2<
        automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1,
    >,
) -> bool {
    matches!(
        error,
        RuntimeStartupRecoveryExecutionAwaitFailureV2::Database(
            automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1::Indeterminate
        )
    )
}

fn revalidate_pending_drain_stage_v2<Environment>(
    environment: &Environment,
    session: &RuntimeClosedRecoverySessionV2,
) -> Result<(), RuntimeProcessStartupRecoveryLoopFailureV2>
where
    Environment: RuntimePendingDrainRecoveryEnvironmentV2,
{
    if let Some(transition) = environment.current_transition_v2(session) {
        return Err(transition);
    }
    session.revalidate_v2().map_err(|error| {
        startup_recovery_execution_rejected_v2(
            RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent,
            error.into(),
        )
    })
}

fn map_pending_drain_database_await_failure_v2<Environment>(
    environment: &Environment,
    session: &RuntimeClosedRecoverySessionV2,
    error: RuntimeStartupRecoveryExecutionAwaitFailureV2<
        automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1,
    >,
) -> RuntimeProcessStartupRecoveryLoopFailureV2
where
    Environment: RuntimePendingDrainRecoveryEnvironmentV2,
{
    match error {
        RuntimeStartupRecoveryExecutionAwaitFailureV2::Transition(transition) => transition,
        RuntimeStartupRecoveryExecutionAwaitFailureV2::Database(error) => {
            prefer_current_startup_recovery_execution_failure_v2(
                environment.current_transition_v2(session),
                || {
                    startup_recovery_execution_database_failure_v2(
                        RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent,
                        error,
                    )
                },
            )
        }
    }
}

impl RuntimePendingDrainRecoveryEnvironmentV2
    for RuntimeProductionPendingDrainRecoveryEnvironmentV2<'_>
{
    fn current_transition_v2(
        &self,
        session: &RuntimeClosedRecoverySessionV2,
    ) -> Option<RuntimeProcessStartupRecoveryLoopFailureV2> {
        current_startup_recovery_execution_transition_from_parts_v2(
            self.foundation,
            self.discord,
            session,
        )
    }

    async fn select_pending_drain_v3(
        &mut self,
        session: &RuntimeClosedRecoverySessionV2,
        authorization: &RuntimeAuthorizedPendingDrainSelectionV3,
    ) -> Result<
        RuntimePendingDrainSelectionReceiptV3,
        RuntimeStartupRecoveryExecutionAwaitFailureV2<
            automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1,
        >,
    > {
        let database = self.foundation.databases.execution().clone();
        let execution_cutoff = pending_drain_execution_cutoff_v2(self.foundation, session);
        await_pending_drain_database_v2(
            self.foundation,
            self.discord,
            session,
            database.select_pending_drain_v3(authorization, execution_cutoff),
        )
        .await
    }

    async fn record_pending_drain_no_candidate_v2(
        &mut self,
        session: &RuntimeClosedRecoverySessionV2,
        selection: &RuntimeSelectedPendingDrainNoCandidateV2,
    ) -> Result<
        RuntimePendingDrainNoCandidateReceiptV2,
        RuntimeStartupRecoveryExecutionAwaitFailureV2<
            automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1,
        >,
    > {
        let database = self.foundation.databases.execution().clone();
        let execution_cutoff = pending_drain_execution_cutoff_v2(self.foundation, session);
        await_pending_drain_database_v2(
            self.foundation,
            self.discord,
            session,
            database.record_pending_drain_no_candidate(selection, execution_cutoff),
        )
        .await
    }

    async fn execute_pending_drain_claim_v2(
        &mut self,
        session: &RuntimeClosedRecoverySessionV2,
        authorization: &RuntimeAuthorizedPendingDrainClaimV2,
    ) -> Result<
        RuntimePendingDrainClaimReceiptV2,
        RuntimeStartupRecoveryExecutionAwaitFailureV2<
            automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1,
        >,
    > {
        let database = self.foundation.databases.execution().clone();
        let execution_cutoff = pending_drain_execution_cutoff_v2(self.foundation, session);
        await_pending_drain_database_v2(
            self.foundation,
            self.discord,
            session,
            database.execute_pending_drain_claim(authorization, execution_cutoff),
        )
        .await
    }

    async fn execute_pending_drain_acknowledgement_v2(
        &mut self,
        session: &RuntimeClosedRecoverySessionV2,
        authorization: &RuntimeAuthorizedPendingDrainAcknowledgementV2,
    ) -> Result<
        RuntimePendingDrainAcknowledgementReceiptV2,
        RuntimeStartupRecoveryExecutionAwaitFailureV2<
            automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1,
        >,
    > {
        let database = self.foundation.databases.execution().clone();
        let execution_cutoff = pending_drain_execution_cutoff_v2(self.foundation, session);
        await_pending_drain_database_v2(
            self.foundation,
            self.discord,
            session,
            database.execute_pending_drain_acknowledgement(authorization, execution_cutoff),
        )
        .await
    }

    async fn execute_pending_drain_succession_v3(
        &mut self,
        session: &RuntimeClosedRecoverySessionV2,
        authorization: &RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3,
    ) -> Result<
        RuntimePendingDrainSuccessionAcknowledgementReceiptV3,
        RuntimeStartupRecoveryExecutionAwaitFailureV2<
            automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1,
        >,
    > {
        let database = self.foundation.databases.execution().clone();
        let execution_cutoff = pending_drain_execution_cutoff_v2(self.foundation, session);
        await_pending_drain_database_v2(
            self.foundation,
            self.discord,
            session,
            database
                .execute_pending_drain_succession_acknowledgement(authorization, execution_cutoff),
        )
        .await
    }
}

impl RuntimePendingDrainMutationEnvironmentV3
    for RuntimeProductionPendingDrainFinalizerEnvironmentV3
{
    fn current_transition_v3(
        &self,
        session: &RuntimeClosedRecoverySessionV2,
    ) -> Option<RuntimeProcessStartupRecoveryLoopFailureV2> {
        let discord = self
            .discord
            .terminal_status()
            .map(|terminal| map_discord_transition_exit_v1(terminal.exit()))
            .or_else(|| {
                self.discord
                    .is_finished()
                    .then_some(RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated)
            });
        let owner_terminal = session.owner_terminal_status_v2().is_some();
        if discord.is_some() {
            self.shutdown
                .trip(crate::RuntimeShutdownCauseV1::DiscordTerminal);
        }
        if owner_terminal {
            self.shutdown
                .trip(crate::RuntimeShutdownCauseV1::GatewayOwnerTerminal);
        }
        classify_current_pending_drain_finalizer_transition_v3(
            Instant::now(),
            self.operation_cutoff,
            session.owner_safety_deadline_v2(),
            discord,
            owner_terminal,
        )
    }

    async fn record_no_candidate_v3(
        &mut self,
        session: &RuntimeClosedRecoverySessionV2,
        selection: &RuntimeSelectedPendingDrainNoCandidateV2,
    ) -> Result<
        RuntimePendingDrainNoCandidateReceiptV2,
        RuntimeStartupRecoveryExecutionAwaitFailureV2<
            automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1,
        >,
    > {
        let database = self.database.clone();
        let execution_cutoff = self
            .operation_cutoff
            .min(session.owner_safety_deadline_v2());
        await_owned_pending_drain_database_v3(
            self,
            session,
            database.record_no_candidate_v3(selection, execution_cutoff),
        )
        .await
    }

    async fn execute_claim_v3(
        &mut self,
        session: &RuntimeClosedRecoverySessionV2,
        authorization: &RuntimeAuthorizedPendingDrainClaimV2,
    ) -> Result<
        RuntimePendingDrainClaimReceiptV2,
        RuntimeStartupRecoveryExecutionAwaitFailureV2<
            automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1,
        >,
    > {
        let database = self.database.clone();
        let execution_cutoff = self
            .operation_cutoff
            .min(session.owner_safety_deadline_v2());
        await_owned_pending_drain_database_v3(
            self,
            session,
            database.execute_claim_v3(authorization, execution_cutoff),
        )
        .await
    }

    async fn execute_acknowledgement_v3(
        &mut self,
        session: &RuntimeClosedRecoverySessionV2,
        authorization: &RuntimeAuthorizedPendingDrainAcknowledgementV2,
    ) -> Result<
        RuntimePendingDrainAcknowledgementReceiptV2,
        RuntimeStartupRecoveryExecutionAwaitFailureV2<
            automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1,
        >,
    > {
        let database = self.database.clone();
        let execution_cutoff = self
            .operation_cutoff
            .min(session.owner_safety_deadline_v2());
        await_owned_pending_drain_database_v3(
            self,
            session,
            database.execute_acknowledgement_v3(authorization, execution_cutoff),
        )
        .await
    }

    async fn execute_succession_v3(
        &mut self,
        session: &RuntimeClosedRecoverySessionV2,
        authorization: &RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3,
    ) -> Result<
        RuntimePendingDrainSuccessionAcknowledgementReceiptV3,
        RuntimeStartupRecoveryExecutionAwaitFailureV2<
            automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1,
        >,
    > {
        let database = self.database.clone();
        let execution_cutoff = self
            .operation_cutoff
            .min(session.owner_safety_deadline_v2());
        await_owned_pending_drain_database_v3(
            self,
            session,
            database.execute_succession_v3(authorization, execution_cutoff),
        )
        .await
    }
}

fn classify_current_pending_drain_finalizer_transition_v3(
    now: Instant,
    operation_cutoff: Instant,
    owner_safety_deadline: Instant,
    discord: Option<RuntimeProcessPausedConnectedTransitionFailureV1>,
    owner_terminal: bool,
) -> Option<RuntimeProcessStartupRecoveryLoopFailureV2> {
    if now >= operation_cutoff.min(owner_safety_deadline) {
        return Some(classify_startup_recovery_execution_deadline_v2(
            operation_cutoff,
            owner_safety_deadline,
        ));
    }
    if let Some(discord) = discord {
        return Some(RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(discord));
    }
    owner_terminal.then_some(
        RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
            RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
        ),
    )
}

async fn await_owned_pending_drain_database_v3<Execution, Completed>(
    environment: &mut RuntimeProductionPendingDrainFinalizerEnvironmentV3,
    session: &RuntimeClosedRecoverySessionV2,
    execution: Execution,
) -> Result<
    Completed,
    RuntimeStartupRecoveryExecutionAwaitFailureV2<
        automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1,
    >,
>
where
    Execution: Future<
            Output = Result<
                Completed,
                automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1,
            >,
        > + Send,
{
    let owner_safety_deadline = session.owner_safety_deadline_v2();
    let execution_cutoff = environment.operation_cutoff.min(owner_safety_deadline);
    let deadline_failure = classify_startup_recovery_execution_deadline_v2(
        environment.operation_cutoff,
        owner_safety_deadline,
    );
    let owner_terminal = session.owner_terminal_observation_v2();
    let discord_terminal = async {
        let transition =
            map_discord_transition_exit_v1(environment.discord.wait_terminal().await.exit());
        environment
            .shutdown
            .trip(crate::RuntimeShutdownCauseV1::DiscordTerminal);
        transition
    };
    let owner_terminal = async {
        let _exit = owner_terminal.await;
        environment
            .shutdown
            .trip(crate::RuntimeShutdownCauseV1::GatewayOwnerTerminal);
    };
    let mut shutdown = environment.shutdown_observer.clone();
    await_startup_recovery_execution_v2(
        deadline_failure,
        sleep_until(TokioInstant::from_std(execution_cutoff)),
        discord_terminal,
        owner_terminal,
        execution,
        async move { shutdown.wait().await },
    )
    .await
}

fn pending_drain_execution_cutoff_v2(
    foundation: &RuntimeProcessFoundationV1,
    session: &RuntimeClosedRecoverySessionV2,
) -> Instant {
    foundation
        .startup_budget
        .operation_cutoff()
        .min(session.owner_safety_deadline_v2())
}

async fn await_pending_drain_database_v2<Execution, Completed>(
    foundation: &RuntimeProcessFoundationV1,
    discord: &mut RuntimeDiscordGatewaySupervisorV1,
    session: &RuntimeClosedRecoverySessionV2,
    execution: Execution,
) -> Result<
    Completed,
    RuntimeStartupRecoveryExecutionAwaitFailureV2<
        automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1,
    >,
>
where
    Execution: Future<
            Output = Result<
                Completed,
                automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1,
            >,
        > + Send,
{
    let operation_cutoff = foundation.startup_budget.operation_cutoff();
    let owner_safety_deadline = session.owner_safety_deadline_v2();
    let execution_cutoff = operation_cutoff.min(owner_safety_deadline);
    let deadline_failure =
        classify_startup_recovery_execution_deadline_v2(operation_cutoff, owner_safety_deadline);
    let owner_terminal = session.owner_terminal_observation_v2();
    let discord_terminal = async {
        let transition = map_discord_transition_exit_v1(discord.wait_terminal().await.exit());
        foundation.trip_shutdown_v1(crate::RuntimeShutdownCauseV1::DiscordTerminal);
        transition
    };
    let owner_terminal = async {
        let _exit = owner_terminal.await;
        foundation.trip_shutdown_v1(crate::RuntimeShutdownCauseV1::GatewayOwnerTerminal);
    };
    let mut shutdown = foundation.shutdown_observer_v1();
    await_startup_recovery_execution_v2(
        deadline_failure,
        sleep_until(TokioInstant::from_std(execution_cutoff)),
        discord_terminal,
        owner_terminal,
        execution,
        async move { shutdown.wait().await },
    )
    .await
}

fn pending_drain_compound_failure_v2(
    _error: RuntimePendingDrainCompoundErrorV2,
) -> RuntimeProcessStartupRecoveryLoopFailureV2 {
    RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainCompound
}

fn startup_recovery_execution_database_failure_v2(
    class: RuntimeStartupRecoveryClassV2,
    error: automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1,
) -> RuntimeProcessStartupRecoveryLoopFailureV2 {
    match class {
        RuntimeStartupRecoveryClassV2::StaleLive => {
            RuntimeProcessStartupRecoveryLoopFailureV2::StaleLiveExecution(error)
        }
        RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification => {
            RuntimeProcessStartupRecoveryLoopFailureV2::ReservedAwaitingCertificationExecution(
                error,
            )
        }
        RuntimeStartupRecoveryClassV2::SuspendedLocalEffect => {
            RuntimeProcessStartupRecoveryLoopFailureV2::SuspendedLocalEffectExecution(error)
        }
        RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent => {
            RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainExecution(error)
        }
    }
}

fn startup_recovery_execution_rejected_v2(
    class: RuntimeStartupRecoveryClassV2,
    error: crate::RuntimeProcessClosedRecoveryCommitFailureV2,
) -> RuntimeProcessStartupRecoveryLoopFailureV2 {
    match class {
        RuntimeStartupRecoveryClassV2::StaleLive => {
            RuntimeProcessStartupRecoveryLoopFailureV2::StaleLiveExecutionRejected(error)
        }
        RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification => {
            RuntimeProcessStartupRecoveryLoopFailureV2::ReservedAwaitingCertificationExecutionRejected(
                error,
            )
        }
        RuntimeStartupRecoveryClassV2::SuspendedLocalEffect => {
            RuntimeProcessStartupRecoveryLoopFailureV2::SuspendedLocalEffectExecutionRejected(error)
        }
        RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent => {
            RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainExecutionRejected(error)
        }
    }
}

fn startup_recovery_execution_retry_after_unsupported_v2(
    class: RuntimeStartupRecoveryClassV2,
) -> RuntimeProcessStartupRecoveryLoopFailureV2 {
    match class {
        RuntimeStartupRecoveryClassV2::StaleLive => {
            RuntimeProcessStartupRecoveryLoopFailureV2::StaleLiveRetryAfterUnsupported
        }
        RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification => {
            RuntimeProcessStartupRecoveryLoopFailureV2::ReservedAwaitingCertificationRetryAfterUnsupported
        }
        RuntimeStartupRecoveryClassV2::SuspendedLocalEffect => {
            RuntimeProcessStartupRecoveryLoopFailureV2::SuspendedLocalEffectRetryAfterUnsupported
        }
        RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent => {
            super::startup_loop::unavailable_recovery_failure_v2(class)
        }
    }
}

fn prefer_current_startup_recovery_execution_failure_v2<Fallback>(
    current: Option<RuntimeProcessStartupRecoveryLoopFailureV2>,
    fallback: Fallback,
) -> RuntimeProcessStartupRecoveryLoopFailureV2
where
    Fallback: FnOnce() -> RuntimeProcessStartupRecoveryLoopFailureV2,
{
    current.unwrap_or_else(fallback)
}

fn current_startup_recovery_execution_transition_v2(
    process: &RuntimeStartupRecoveryContinueProcessV2,
) -> Option<RuntimeProcessStartupRecoveryLoopFailureV2> {
    current_startup_recovery_execution_transition_from_parts_v2(
        &process.foundation,
        &process.discord,
        &process.session,
    )
}

fn current_startup_recovery_execution_transition_from_parts_v2(
    foundation: &RuntimeProcessFoundationV1,
    discord: &RuntimeDiscordGatewaySupervisorV1,
    session: &RuntimeClosedRecoverySessionV2,
) -> Option<RuntimeProcessStartupRecoveryLoopFailureV2> {
    let discord = discord_transition_failure_v1(discord);
    let owner_terminal = session.owner_terminal_status_v2().is_some();
    if discord.is_some() {
        foundation.trip_shutdown_v1(crate::RuntimeShutdownCauseV1::DiscordTerminal);
    }
    if owner_terminal {
        foundation.trip_shutdown_v1(crate::RuntimeShutdownCauseV1::GatewayOwnerTerminal);
    }
    classify_current_startup_recovery_execution_transition_v2(
        Instant::now(),
        foundation.startup_budget.operation_cutoff(),
        session.owner_safety_deadline_v2(),
        discord,
        owner_terminal,
    )
}

fn classify_current_startup_recovery_execution_transition_v2(
    now: Instant,
    operation_cutoff: Instant,
    owner_safety_deadline: Instant,
    discord: Option<RuntimeProcessPausedConnectedTransitionFailureV1>,
    owner_terminal: bool,
) -> Option<RuntimeProcessStartupRecoveryLoopFailureV2> {
    if now >= operation_cutoff.min(owner_safety_deadline) {
        return Some(classify_startup_recovery_execution_deadline_v2(
            operation_cutoff,
            owner_safety_deadline,
        ));
    }
    if let Some(error) = discord {
        return Some(RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(error));
    }
    owner_terminal.then_some(
        RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
            RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
        ),
    )
}

fn classify_startup_recovery_execution_deadline_v2(
    operation_cutoff: Instant,
    owner_safety_deadline: Instant,
) -> RuntimeProcessStartupRecoveryLoopFailureV2 {
    if owner_safety_deadline <= operation_cutoff {
        RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
            RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
        )
    } else {
        RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed
    }
}

async fn await_startup_recovery_execution_v2<
    Deadline,
    DiscordTerminal,
    OwnerTerminal,
    Execution,
    Shutdown,
    Completed,
    E,
>(
    deadline_failure: RuntimeProcessStartupRecoveryLoopFailureV2,
    deadline: Deadline,
    discord_terminal: DiscordTerminal,
    owner_terminal: OwnerTerminal,
    execution: Execution,
    shutdown: Shutdown,
) -> Result<Completed, RuntimeStartupRecoveryExecutionAwaitFailureV2<E>>
where
    Deadline: Future<Output = ()>,
    DiscordTerminal: Future<Output = RuntimeProcessPausedConnectedTransitionFailureV1>,
    OwnerTerminal: Future<Output = ()>,
    Execution: Future<Output = Result<Completed, E>>,
    Shutdown: Future<Output = crate::RuntimeShutdownObservationV1>,
{
    tokio::pin!(deadline);
    tokio::pin!(discord_terminal);
    tokio::pin!(owner_terminal);
    tokio::pin!(execution);
    tokio::pin!(shutdown);
    tokio::select! {
        biased;
        observation = &mut shutdown => {
            Err(RuntimeStartupRecoveryExecutionAwaitFailureV2::Transition(
                RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
                    RuntimeProcessPausedConnectedTransitionFailureV1::ProcessShutdown(
                        observation.cause(),
                    ),
                ),
            ))
        }
        () = &mut deadline => {
            Err(RuntimeStartupRecoveryExecutionAwaitFailureV2::Transition(deadline_failure))
        }
        transition = &mut discord_terminal => {
            Err(RuntimeStartupRecoveryExecutionAwaitFailureV2::Transition(
                RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(transition),
            ))
        }
        () = &mut owner_terminal => {
            Err(RuntimeStartupRecoveryExecutionAwaitFailureV2::Transition(
                RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
                    RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
                ),
            ))
        }
        result = &mut execution => {
            result.map_err(RuntimeStartupRecoveryExecutionAwaitFailureV2::Database)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::{pending, ready};
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::task::{Context, Poll};
    use std::time::Duration;

    use super::*;

    struct TrackedPendingExecutionV2 {
        polled: Arc<AtomicBool>,
        dropped: Arc<AtomicBool>,
    }

    impl Future for TrackedPendingExecutionV2 {
        type Output = Result<(), ()>;

        fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
            self.polled.store(true, Ordering::Release);
            Poll::Pending
        }
    }

    impl Drop for TrackedPendingExecutionV2 {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::Release);
        }
    }

    #[tokio::test]
    async fn execution_wait_priority_is_cutoff_then_discord_then_owner_then_database() {
        let deadline = RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed;
        let all_ready = await_startup_recovery_execution_v2(
            deadline,
            ready(()),
            ready(RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated),
            ready(()),
            ready(Ok::<_, ()>(())),
            pending(),
        )
        .await;
        assert!(matches!(
            all_ready,
            Err(RuntimeStartupRecoveryExecutionAwaitFailureV2::Transition(
                RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed
            ))
        ));

        let discord_ready = await_startup_recovery_execution_v2(
            deadline,
            pending::<()>(),
            ready(RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated),
            ready(()),
            ready(Ok::<_, ()>(())),
            pending(),
        )
        .await;
        assert!(matches!(
            discord_ready,
            Err(RuntimeStartupRecoveryExecutionAwaitFailureV2::Transition(
                RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
                    RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated
                )
            ))
        ));

        let owner_ready = await_startup_recovery_execution_v2(
            deadline,
            pending::<()>(),
            pending::<RuntimeProcessPausedConnectedTransitionFailureV1>(),
            ready(()),
            ready(Ok::<_, ()>(())),
            pending(),
        )
        .await;
        assert!(matches!(
            owner_ready,
            Err(RuntimeStartupRecoveryExecutionAwaitFailureV2::Transition(
                RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
                    RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated
                )
            ))
        ));

        let database_ready = await_startup_recovery_execution_v2(
            deadline,
            pending::<()>(),
            pending::<RuntimeProcessPausedConnectedTransitionFailureV1>(),
            pending::<()>(),
            ready(Ok::<_, ()>(())),
            pending(),
        )
        .await;
        assert!(matches!(database_ready, Ok(())));

        let database_error = await_startup_recovery_execution_v2(
            deadline,
            ready(()),
            pending::<RuntimeProcessPausedConnectedTransitionFailureV1>(),
            pending::<()>(),
            ready(Err::<(), _>("database")),
            pending(),
        )
        .await;
        assert!(matches!(
            database_error,
            Err(RuntimeStartupRecoveryExecutionAwaitFailureV2::Transition(
                RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed
            ))
        ));
    }

    #[tokio::test]
    async fn interrupted_execution_future_is_dropped() {
        let polled = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicBool::new(false));
        let execution = TrackedPendingExecutionV2 {
            polled: polled.clone(),
            dropped: dropped.clone(),
        };
        let result = await_startup_recovery_execution_v2(
            RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed,
            ready(()),
            pending::<RuntimeProcessPausedConnectedTransitionFailureV1>(),
            pending::<()>(),
            execution,
            pending(),
        )
        .await;

        assert!(matches!(
            result,
            Err(RuntimeStartupRecoveryExecutionAwaitFailureV2::Transition(
                RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed
            ))
        ));
        assert!(!polled.load(Ordering::Acquire));
        assert!(dropped.load(Ordering::Acquire));
    }

    #[test]
    fn current_execution_state_and_deadline_cause_are_deterministic() {
        let now = Instant::now();
        let discord = Some(RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated);
        assert_eq!(
            classify_current_startup_recovery_execution_transition_v2(
                now,
                now,
                now + Duration::from_secs(1),
                discord,
                true,
            ),
            Some(RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed)
        );
        assert_eq!(
            classify_current_startup_recovery_execution_transition_v2(
                now,
                now + Duration::from_secs(1),
                now,
                discord,
                true,
            ),
            Some(
                RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
                    RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
                )
            )
        );
        assert_eq!(
            classify_current_startup_recovery_execution_transition_v2(
                now,
                now + Duration::from_secs(2),
                now + Duration::from_secs(1),
                discord,
                true,
            ),
            Some(
                RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
                    RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated,
                )
            )
        );
        assert_eq!(
            prefer_current_startup_recovery_execution_failure_v2(
                Some(RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed),
                || RuntimeProcessStartupRecoveryLoopFailureV2::StaleLiveExecutionRejected(
                    crate::RuntimeProcessClosedRecoveryCommitFailureV2::GatewayProtocolViolation,
                ),
            ),
            RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed
        );
    }

    #[test]
    fn only_indeterminate_database_outcomes_receive_exact_finalization() {
        use automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1;

        assert!(pending_drain_requires_exact_finalization_v2(
            &RuntimeStartupRecoveryExecutionAwaitFailureV2::Database(
                RuntimeExecutionPersistenceErrorV1::Indeterminate,
            ),
        ));
        for error in [
            RuntimeExecutionPersistenceErrorV1::InvalidInput,
            RuntimeExecutionPersistenceErrorV1::DatabaseAuthorityMismatch,
            RuntimeExecutionPersistenceErrorV1::OwnershipLost,
            RuntimeExecutionPersistenceErrorV1::AuthorityChanged,
            RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt,
            RuntimeExecutionPersistenceErrorV1::RetryNotReady,
            RuntimeExecutionPersistenceErrorV1::Superseded,
            RuntimeExecutionPersistenceErrorV1::Timeout,
            RuntimeExecutionPersistenceErrorV1::Concurrency,
            RuntimeExecutionPersistenceErrorV1::Unavailable,
            RuntimeExecutionPersistenceErrorV1::DatabaseFailure,
            RuntimeExecutionPersistenceErrorV1::ObservationAmbiguous,
        ] {
            assert!(!pending_drain_requires_exact_finalization_v2(
                &RuntimeStartupRecoveryExecutionAwaitFailureV2::Database(error),
            ));
        }
        assert!(!pending_drain_requires_exact_finalization_v2(
            &RuntimeStartupRecoveryExecutionAwaitFailureV2::Transition(
                RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed,
            ),
        ));
    }

    #[test]
    fn supported_recovery_classes_preserve_distinct_execution_failures() {
        let database_error =
            automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt;
        let protocol_error =
            crate::RuntimeProcessClosedRecoveryCommitFailureV2::GatewayProtocolViolation;

        assert_eq!(
            startup_recovery_execution_database_failure_v2(
                RuntimeStartupRecoveryClassV2::StaleLive,
                database_error,
            ),
            RuntimeProcessStartupRecoveryLoopFailureV2::StaleLiveExecution(database_error)
        );
        assert_eq!(
            startup_recovery_execution_database_failure_v2(
                RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification,
                database_error,
            ),
            RuntimeProcessStartupRecoveryLoopFailureV2::ReservedAwaitingCertificationExecution(
                database_error,
            )
        );
        assert_eq!(
            startup_recovery_execution_rejected_v2(
                RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification,
                protocol_error,
            ),
            RuntimeProcessStartupRecoveryLoopFailureV2::ReservedAwaitingCertificationExecutionRejected(
                protocol_error,
            )
        );
        assert_eq!(
            startup_recovery_execution_retry_after_unsupported_v2(
                RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification,
            ),
            RuntimeProcessStartupRecoveryLoopFailureV2::ReservedAwaitingCertificationRetryAfterUnsupported
        );
        assert_eq!(
            startup_recovery_execution_database_failure_v2(
                RuntimeStartupRecoveryClassV2::SuspendedLocalEffect,
                database_error,
            ),
            RuntimeProcessStartupRecoveryLoopFailureV2::SuspendedLocalEffectExecution(
                database_error,
            )
        );
        assert_eq!(
            startup_recovery_execution_rejected_v2(
                RuntimeStartupRecoveryClassV2::SuspendedLocalEffect,
                protocol_error,
            ),
            RuntimeProcessStartupRecoveryLoopFailureV2::SuspendedLocalEffectExecutionRejected(
                protocol_error,
            )
        );
        assert_eq!(
            startup_recovery_execution_retry_after_unsupported_v2(
                RuntimeStartupRecoveryClassV2::SuspendedLocalEffect,
            ),
            RuntimeProcessStartupRecoveryLoopFailureV2::SuspendedLocalEffectRetryAfterUnsupported
        );
        assert_eq!(
            startup_recovery_execution_database_failure_v2(
                RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent,
                database_error,
            ),
            RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainExecution(
                database_error
            )
        );
        assert_eq!(
            startup_recovery_execution_rejected_v2(
                RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent,
                protocol_error,
            ),
            RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainExecutionRejected(
                protocol_error,
            )
        );
    }
}
