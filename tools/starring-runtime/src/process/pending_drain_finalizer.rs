use std::fmt::{Debug, Formatter};
use std::future::Future;

use automation_runtime_worker::{
    RuntimeAuthorizedPendingDrainAcknowledgementV2, RuntimeAuthorizedPendingDrainClaimV2,
    RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3,
    RuntimeCompletedStartupRecoveryExecutionV2, RuntimePendingDrainAcknowledgementReceiptV2,
    RuntimePendingDrainClaimReceiptV2, RuntimePendingDrainCompoundErrorV2,
    RuntimePendingDrainNoCandidateReceiptV2, RuntimePendingDrainSuccessionAcknowledgementReceiptV3,
    RuntimeSelectedPendingDrainNoCandidateV2,
};

use super::certification_finalizer::{
    RuntimeProcessMutationFinalizerCompletionV3, RuntimeProcessMutationFinalizerErrorV3,
    RuntimeProcessMutationFinalizerJobV3, RuntimeProcessMutationFinalizerOutputV3,
    RuntimeProcessMutationFinalizerPortV3, RuntimeProcessMutationFinalizerSupervisorV3,
};
use super::execution::RuntimeStartupRecoveryExecutionAwaitFailureV2;
use super::startup_loop::RuntimeProcessStartupRecoveryLoopFailureV2;
use crate::closed_recovery::RuntimeClosedRecoverySessionV2;
use crate::{
    RuntimeMutationFinalizerCompletionResultV1, RuntimeMutationFinalizerConfigV1,
    RuntimeMutationFinalizerJobIdV1, RuntimeMutationFinalizerJobV1,
    RuntimeMutationFinalizerRegistrationRejectionReasonV1, RuntimeSupervisorExitV1,
};

pub(crate) trait RuntimePendingDrainMutationEnvironmentV3: Send + 'static {
    fn current_transition_v3(
        &self,
        session: &RuntimeClosedRecoverySessionV2,
    ) -> Option<RuntimeProcessStartupRecoveryLoopFailureV2>;

    fn record_no_candidate_v3<'a>(
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

    fn execute_claim_v3<'a>(
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

    fn execute_acknowledgement_v3<'a>(
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

    fn execute_succession_v3<'a>(
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

pub(crate) enum RuntimePendingDrainMutationStageV3 {
    NoCandidate(Box<RuntimeSelectedPendingDrainNoCandidateV2>),
    Claim(Box<RuntimeAuthorizedPendingDrainClaimV2>),
    Acknowledgement(Box<RuntimeAuthorizedPendingDrainAcknowledgementV2>),
    Succession(Box<RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3>),
}

impl Debug for RuntimePendingDrainMutationStageV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainMutationStageV3(<redacted>)")
    }
}

pub(crate) struct RuntimePendingDrainFinalizerJobV3<E> {
    session: RuntimeClosedRecoverySessionV2,
    environment: E,
    stage: RuntimePendingDrainMutationStageV3,
}

impl<E> RuntimePendingDrainFinalizerJobV3<E> {
    pub(crate) fn new(
        session: RuntimeClosedRecoverySessionV2,
        environment: E,
        stage: RuntimePendingDrainMutationStageV3,
    ) -> Self {
        Self {
            session,
            environment,
            stage,
        }
    }

    pub(super) fn into_parts(
        self,
    ) -> (
        RuntimeClosedRecoverySessionV2,
        E,
        RuntimePendingDrainMutationStageV3,
    ) {
        (self.session, self.environment, self.stage)
    }
}

impl<E> Debug for RuntimePendingDrainFinalizerJobV3<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainFinalizerJobV3(<redacted>)")
    }
}

pub(crate) enum RuntimePendingDrainMutationOutputV3 {
    Completed(RuntimeCompletedStartupRecoveryExecutionV2),
    ClaimAccepted(RuntimeAuthorizedPendingDrainAcknowledgementV2),
}

impl Debug for RuntimePendingDrainMutationOutputV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainMutationOutputV3(<redacted>)")
    }
}

pub(crate) struct RuntimePendingDrainFinalizerSettledV3<E> {
    session: RuntimeClosedRecoverySessionV2,
    environment: E,
    output: RuntimePendingDrainMutationOutputV3,
}

impl<E> RuntimePendingDrainFinalizerSettledV3<E> {
    pub(crate) fn into_parts(
        self,
    ) -> (
        RuntimeClosedRecoverySessionV2,
        E,
        RuntimePendingDrainMutationOutputV3,
    ) {
        (self.session, self.environment, self.output)
    }
}

impl<E> Debug for RuntimePendingDrainFinalizerSettledV3<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainFinalizerSettledV3(<redacted>)")
    }
}

pub(crate) struct RuntimePendingDrainFinalizerFailedV3<E> {
    session: RuntimeClosedRecoverySessionV2,
    environment: E,
    failure: RuntimeProcessStartupRecoveryLoopFailureV2,
}

impl<E> RuntimePendingDrainFinalizerFailedV3<E> {
    pub(crate) fn into_parts(
        self,
    ) -> (
        RuntimeClosedRecoverySessionV2,
        E,
        RuntimeProcessStartupRecoveryLoopFailureV2,
    ) {
        (self.session, self.environment, self.failure)
    }
}

impl<E> Debug for RuntimePendingDrainFinalizerFailedV3<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainFinalizerFailedV3(<redacted>)")
    }
}

pub(crate) type RuntimePendingDrainFinalizerSupervisorV3<E> =
    RuntimeProcessMutationFinalizerSupervisorV3<E>;

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) type RuntimePendingDrainFinalizerPortV3<E> = RuntimeProcessMutationFinalizerPortV3<E>;

pub(crate) enum RuntimePendingDrainFinalizerDispatchFailureV3<E> {
    Rejected {
        job: Box<RuntimePendingDrainFinalizerJobV3<E>>,
        reason: RuntimeMutationFinalizerRegistrationRejectionReasonV1,
    },
    Failed(Box<RuntimePendingDrainFinalizerFailedV3<E>>),
    Undispatched {
        job: Box<RuntimePendingDrainFinalizerJobV3<E>>,
        exit: RuntimeSupervisorExitV1,
    },
    DispatchedTerminal(RuntimeSupervisorExitV1),
    CompletionChannelClosed,
    CompletionIdentityMismatch {
        expected: RuntimeMutationFinalizerJobIdV1,
        actual: RuntimeMutationFinalizerJobIdV1,
    },
}

impl<E> Debug for RuntimePendingDrainFinalizerDispatchFailureV3<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainFinalizerDispatchFailureV3(<redacted>)")
    }
}

pub(crate) struct RuntimePendingDrainRegisteredJobV3 {
    job_id: RuntimeMutationFinalizerJobIdV1,
}

impl RuntimePendingDrainRegisteredJobV3 {
    pub(crate) const fn job_id(&self) -> RuntimeMutationFinalizerJobIdV1 {
        self.job_id
    }
}

impl Debug for RuntimePendingDrainRegisteredJobV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainRegisteredJobV3(<redacted>)")
    }
}

type RuntimePendingDrainFinalizerCompletionV3<E> = RuntimeProcessMutationFinalizerCompletionV3<E>;

pub(crate) fn register_pending_drain_job_v3<E>(
    supervisor: &RuntimePendingDrainFinalizerSupervisorV3<E>,
    job: RuntimePendingDrainFinalizerJobV3<E>,
) -> Result<RuntimePendingDrainRegisteredJobV3, RuntimePendingDrainFinalizerDispatchFailureV3<E>>
where
    E: RuntimePendingDrainMutationEnvironmentV3,
{
    match supervisor
        .intake()
        .try_register(RuntimeMutationFinalizerJobV1::StartupPendingDrain(
            RuntimeProcessMutationFinalizerJobV3::StartupPendingDrain(Box::new(job)),
        )) {
        Ok(waiter) => {
            let job_id = waiter.job_id();
            drop(waiter);
            Ok(RuntimePendingDrainRegisteredJobV3 { job_id })
        }
        Err(rejected) => {
            let reason = rejected.reason();
            let job = rejected.into_job().into_startup_pending_drain();
            let RuntimeProcessMutationFinalizerJobV3::StartupPendingDrain(job) = job else {
                unreachable!()
            };
            Err(RuntimePendingDrainFinalizerDispatchFailureV3::Rejected { job, reason })
        }
    }
}

#[cfg(test)]
pub(crate) async fn complete_registered_pending_drain_job_v3<E>(
    supervisor: &mut RuntimePendingDrainFinalizerSupervisorV3<E>,
    registered: RuntimePendingDrainRegisteredJobV3,
) -> Result<
    RuntimePendingDrainFinalizerSettledV3<E>,
    RuntimePendingDrainFinalizerDispatchFailureV3<E>,
>
where
    E: RuntimePendingDrainMutationEnvironmentV3,
{
    let Some(completion) = supervisor.next_completion().await else {
        return Err(RuntimePendingDrainFinalizerDispatchFailureV3::CompletionChannelClosed);
    };
    complete_pending_drain_job_v3(registered, completion)
}

pub(crate) fn complete_pending_drain_job_v3<E>(
    registered: RuntimePendingDrainRegisteredJobV3,
    completion: RuntimePendingDrainFinalizerCompletionV3<E>,
) -> Result<
    RuntimePendingDrainFinalizerSettledV3<E>,
    RuntimePendingDrainFinalizerDispatchFailureV3<E>,
> {
    let expected = registered.job_id();
    let actual = completion.job_id();
    if actual != expected {
        return Err(
            RuntimePendingDrainFinalizerDispatchFailureV3::CompletionIdentityMismatch {
                expected,
                actual,
            },
        );
    }
    match completion.into_result() {
        RuntimeMutationFinalizerCompletionResultV1::Settled(
            RuntimeProcessMutationFinalizerOutputV3::StartupPendingDrain(settled),
        ) => Ok(*settled),
        RuntimeMutationFinalizerCompletionResultV1::Failed(
            RuntimeProcessMutationFinalizerErrorV3::StartupPendingDrain(failed),
        ) => Err(RuntimePendingDrainFinalizerDispatchFailureV3::Failed(
            failed,
        )),
        RuntimeMutationFinalizerCompletionResultV1::Undispatched {
            job:
                RuntimeMutationFinalizerJobV1::StartupPendingDrain(
                    RuntimeProcessMutationFinalizerJobV3::StartupPendingDrain(job),
                ),
            exit,
        } => Err(RuntimePendingDrainFinalizerDispatchFailureV3::Undispatched { job, exit }),
        RuntimeMutationFinalizerCompletionResultV1::DispatchedTerminal(exit) => {
            Err(RuntimePendingDrainFinalizerDispatchFailureV3::DispatchedTerminal(exit))
        }
        _ => Err(
            RuntimePendingDrainFinalizerDispatchFailureV3::DispatchedTerminal(
                RuntimeSupervisorExitV1::ProtocolViolation,
            ),
        ),
    }
}

#[cfg(test)]
pub(crate) async fn register_and_complete_pending_drain_job_v3<E>(
    supervisor: &mut RuntimePendingDrainFinalizerSupervisorV3<E>,
    job: RuntimePendingDrainFinalizerJobV3<E>,
) -> Result<
    RuntimePendingDrainFinalizerSettledV3<E>,
    RuntimePendingDrainFinalizerDispatchFailureV3<E>,
>
where
    E: RuntimePendingDrainMutationEnvironmentV3,
{
    let registered = register_pending_drain_job_v3(supervisor, job)?;
    complete_registered_pending_drain_job_v3(supervisor, registered).await
}

pub(super) async fn execute_pending_drain_finalizer_job_v3<E>(
    job: RuntimePendingDrainFinalizerJobV3<E>,
) -> Result<RuntimePendingDrainFinalizerSettledV3<E>, RuntimePendingDrainFinalizerFailedV3<E>>
where
    E: RuntimePendingDrainMutationEnvironmentV3,
{
    let (mut session, mut environment, stage) = job.into_parts();
    let result =
        execute_pending_drain_mutation_stage_v3(&mut session, &mut environment, stage).await;
    match result {
        Ok(output) => Ok(RuntimePendingDrainFinalizerSettledV3 {
            session,
            environment,
            output,
        }),
        Err(failure) => Err(RuntimePendingDrainFinalizerFailedV3 {
            session,
            environment,
            failure,
        }),
    }
}

async fn execute_pending_drain_mutation_stage_v3<E>(
    session: &mut RuntimeClosedRecoverySessionV2,
    environment: &mut E,
    stage: RuntimePendingDrainMutationStageV3,
) -> Result<RuntimePendingDrainMutationOutputV3, RuntimeProcessStartupRecoveryLoopFailureV2>
where
    E: RuntimePendingDrainMutationEnvironmentV3,
{
    revalidate_pending_drain_finalizer_stage_v3(environment, session)?;
    match stage {
        RuntimePendingDrainMutationStageV3::NoCandidate(selection) => {
            let first = environment
                .record_no_candidate_v3(session, &selection)
                .await;
            let receipt = match first {
                Err(error) if pending_drain_requires_exact_finalization_v3(&error) => {
                    revalidate_pending_drain_finalizer_stage_v3(environment, session)?;
                    environment
                        .record_no_candidate_v3(session, &selection)
                        .await
                }
                result => result,
            }
            .map_err(|error| {
                map_pending_drain_finalizer_await_failure_v3(environment, session, error)
            })?;
            revalidate_pending_drain_finalizer_stage_v3(environment, session)?;
            let completed = (*selection)
                .complete(receipt)
                .map_err(pending_drain_compound_failure_v3)?;
            Ok(RuntimePendingDrainMutationOutputV3::Completed(completed))
        }
        RuntimePendingDrainMutationStageV3::Claim(authorization) => {
            let first = environment.execute_claim_v3(session, &authorization).await;
            let receipt = match first {
                Err(error) if pending_drain_requires_exact_finalization_v3(&error) => {
                    revalidate_pending_drain_finalizer_stage_v3(environment, session)?;
                    environment.execute_claim_v3(session, &authorization).await
                }
                result => result,
            }
            .map_err(|error| {
                map_pending_drain_finalizer_await_failure_v3(environment, session, error)
            })?;
            revalidate_pending_drain_finalizer_stage_v3(environment, session)?;
            let acknowledgement = (*authorization)
                .complete(receipt)
                .map_err(pending_drain_compound_failure_v3)?;
            Ok(RuntimePendingDrainMutationOutputV3::ClaimAccepted(
                acknowledgement,
            ))
        }
        RuntimePendingDrainMutationStageV3::Acknowledgement(authorization) => {
            let first = environment
                .execute_acknowledgement_v3(session, &authorization)
                .await;
            let receipt = match first {
                Err(error) if pending_drain_requires_exact_finalization_v3(&error) => {
                    revalidate_pending_drain_finalizer_stage_v3(environment, session)?;
                    environment
                        .execute_acknowledgement_v3(session, &authorization)
                        .await
                }
                result => result,
            }
            .map_err(|error| {
                map_pending_drain_finalizer_await_failure_v3(environment, session, error)
            })?;
            revalidate_pending_drain_finalizer_stage_v3(environment, session)?;
            let durable = (*authorization)
                .complete(receipt)
                .map_err(pending_drain_compound_failure_v3)?;
            let unseal = session
                .unseal_pending_drain_after_durable_ack_v2(&durable)
                .map_err(|error| pending_drain_session_failure_v3(error.into()))?;
            let completed = durable
                .complete_registry_rollover(unseal)
                .map_err(pending_drain_compound_failure_v3)?;
            Ok(RuntimePendingDrainMutationOutputV3::Completed(completed))
        }
        RuntimePendingDrainMutationStageV3::Succession(authorization) => {
            let first = environment
                .execute_succession_v3(session, &authorization)
                .await;
            let receipt = match first {
                Err(error) if pending_drain_requires_exact_finalization_v3(&error) => {
                    revalidate_pending_drain_finalizer_stage_v3(environment, session)?;
                    environment
                        .execute_succession_v3(session, &authorization)
                        .await
                }
                result => result,
            }
            .map_err(|error| {
                map_pending_drain_finalizer_await_failure_v3(environment, session, error)
            })?;
            revalidate_pending_drain_finalizer_stage_v3(environment, session)?;
            let durable = (*authorization)
                .complete(receipt)
                .map_err(pending_drain_compound_failure_v3)?;
            revalidate_pending_drain_finalizer_stage_v3(environment, session)?;
            let unseal = session
                .unseal_pending_drain_after_durable_succession_v3(&durable)
                .map_err(|error| pending_drain_session_failure_v3(error.into()))?;
            let completed = durable
                .complete_registry_rollover(unseal)
                .map_err(pending_drain_compound_failure_v3)?;
            Ok(RuntimePendingDrainMutationOutputV3::Completed(completed))
        }
    }
}

fn pending_drain_requires_exact_finalization_v3(
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

fn revalidate_pending_drain_finalizer_stage_v3<E>(
    environment: &E,
    session: &RuntimeClosedRecoverySessionV2,
) -> Result<(), RuntimeProcessStartupRecoveryLoopFailureV2>
where
    E: RuntimePendingDrainMutationEnvironmentV3,
{
    if let Some(transition) = environment.current_transition_v3(session) {
        return Err(transition);
    }
    session
        .revalidate_v2()
        .map_err(|error| pending_drain_session_failure_v3(error.into()))
}

fn map_pending_drain_finalizer_await_failure_v3<E>(
    environment: &E,
    session: &RuntimeClosedRecoverySessionV2,
    error: RuntimeStartupRecoveryExecutionAwaitFailureV2<
        automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1,
    >,
) -> RuntimeProcessStartupRecoveryLoopFailureV2
where
    E: RuntimePendingDrainMutationEnvironmentV3,
{
    match error {
        RuntimeStartupRecoveryExecutionAwaitFailureV2::Transition(transition) => transition,
        RuntimeStartupRecoveryExecutionAwaitFailureV2::Database(error) => {
            environment.current_transition_v3(session).unwrap_or(
                RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainExecution(error),
            )
        }
    }
}

fn pending_drain_compound_failure_v3(
    _error: RuntimePendingDrainCompoundErrorV2,
) -> RuntimeProcessStartupRecoveryLoopFailureV2 {
    RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainCompound
}

fn pending_drain_session_failure_v3(
    failure: super::closed::RuntimeProcessClosedRecoveryCommitFailureV2,
) -> RuntimeProcessStartupRecoveryLoopFailureV2 {
    RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainExecutionRejected(failure)
}

pub(super) fn production_finalizer_config_v3() -> RuntimeMutationFinalizerConfigV1 {
    RuntimeMutationFinalizerConfigV1::new(1).expect("production mutation finalizer capacity")
}
