use std::fmt::{Debug, Formatter};
use std::marker::PhantomData;

use automation_runtime_execution_postgres::{
    PostgresPreparedRuntimeCertificationV2, PostgresRuntimeCertificationCommitRecoveryV2,
    RuntimeExecutionPersistenceErrorV1,
};
use automation_runtime_worker::{
    RuntimeCertificationFinalizationOutcomeV2, RuntimeCertificationFinalizerJobV2,
    RuntimeCertificationFinalizerPortV2, RuntimeCertificationFinalizerRegistrationV2,
    RuntimeCertificationFinalizerRejectionV2,
};

use super::execution::RuntimeProductionPendingDrainFinalizerEnvironmentV3;
use super::pending_drain_finalizer::{
    execute_pending_drain_finalizer_job_v3, RuntimePendingDrainFinalizerFailedV3,
    RuntimePendingDrainFinalizerJobV3, RuntimePendingDrainFinalizerSettledV3,
    RuntimePendingDrainMutationEnvironmentV3,
};
use crate::mutation_finalizer::{
    RuntimeMutationFinalizerCompletionResultV1, RuntimeMutationFinalizerCompletionV1,
    RuntimeMutationFinalizerJobIdV1, RuntimeMutationFinalizerJobV1, RuntimeMutationFinalizerPortV1,
    RuntimeMutationFinalizerProcessSupervisorV1,
    RuntimeMutationFinalizerRegistrationRejectionReasonV1,
    RuntimeMutationFinalizerReservedProcessSlotV1, RuntimeMutationFinalizerSupervisorV1,
    RuntimeSupervisorExitV1,
};

pub(crate) type RuntimeProductionCertificationFinalizerJobV2 =
    RuntimeCertificationFinalizerJobV2<PostgresPreparedRuntimeCertificationV2>;

pub(crate) type RuntimeProductionCertificationFinalizationOutcomeV2 =
    RuntimeCertificationFinalizationOutcomeV2<
        RuntimeExecutionPersistenceErrorV1,
        PostgresRuntimeCertificationCommitRecoveryV2,
        (),
    >;

pub(crate) enum RuntimeProcessMutationFinalizerJobV3<E> {
    StartupPendingDrain(Box<RuntimePendingDrainFinalizerJobV3<E>>),
    Certification(Box<RuntimeProductionCertificationFinalizerJobV2>),
}

impl<E> Debug for RuntimeProcessMutationFinalizerJobV3<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessMutationFinalizerJobV3(<redacted>)")
    }
}

#[allow(dead_code)]
pub(crate) enum RuntimeProcessMutationFinalizerOutputV3<E> {
    StartupPendingDrain(Box<RuntimePendingDrainFinalizerSettledV3<E>>),
    Certification(Box<RuntimeProductionCertificationFinalizationOutcomeV2>),
}

impl<E> Debug for RuntimeProcessMutationFinalizerOutputV3<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessMutationFinalizerOutputV3(<redacted>)")
    }
}

pub(crate) enum RuntimeProcessMutationFinalizerErrorV3<E> {
    StartupPendingDrain(Box<RuntimePendingDrainFinalizerFailedV3<E>>),
}

impl<E> Debug for RuntimeProcessMutationFinalizerErrorV3<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessMutationFinalizerErrorV3(<redacted>)")
    }
}

pub(crate) struct RuntimeProcessMutationFinalizerPortV3<E> {
    environment: PhantomData<fn() -> E>,
}

impl<E> RuntimeProcessMutationFinalizerPortV3<E> {
    pub(crate) const fn new() -> Self {
        Self {
            environment: PhantomData,
        }
    }
}

impl<E> Debug for RuntimeProcessMutationFinalizerPortV3<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessMutationFinalizerPortV3(<redacted>)")
    }
}

impl<E> RuntimeMutationFinalizerPortV1 for RuntimeProcessMutationFinalizerPortV3<E>
where
    E: RuntimePendingDrainMutationEnvironmentV3,
{
    type Job = RuntimeProcessMutationFinalizerJobV3<E>;
    type Output = RuntimeProcessMutationFinalizerOutputV3<E>;
    type Error = RuntimeProcessMutationFinalizerErrorV3<E>;

    async fn execute(
        &self,
        job: RuntimeMutationFinalizerJobV1<Self::Job>,
    ) -> Result<Self::Output, Self::Error> {
        match job.into_inner() {
            RuntimeProcessMutationFinalizerJobV3::StartupPendingDrain(job) => {
                execute_pending_drain_finalizer_job_v3(*job)
                    .await
                    .map(|settled| {
                        RuntimeProcessMutationFinalizerOutputV3::StartupPendingDrain(Box::new(
                            settled,
                        ))
                    })
                    .map_err(|failed| {
                        RuntimeProcessMutationFinalizerErrorV3::StartupPendingDrain(Box::new(
                            failed,
                        ))
                    })
            }
            RuntimeProcessMutationFinalizerJobV3::Certification(job) => {
                Ok(RuntimeProcessMutationFinalizerOutputV3::Certification(
                    Box::new((*job).run().await),
                ))
            }
        }
    }
}

pub(crate) type RuntimeProcessMutationFinalizerSupervisorV3<E> =
    RuntimeMutationFinalizerSupervisorV1<RuntimeProcessMutationFinalizerPortV3<E>>;

pub(crate) type RuntimeProcessMutationFinalizerProcessSupervisorV3<E> =
    RuntimeMutationFinalizerProcessSupervisorV1<RuntimeProcessMutationFinalizerPortV3<E>>;

pub(crate) struct RuntimeProcessCertificationFinalizerPortV2<'a> {
    supervisor: &'a RuntimeProcessMutationFinalizerProcessSupervisorV3<
        RuntimeProductionPendingDrainFinalizerEnvironmentV3,
    >,
}

impl<'a> RuntimeProcessCertificationFinalizerPortV2<'a> {
    pub(super) const fn new(
        supervisor: &'a RuntimeProcessMutationFinalizerProcessSupervisorV3<
            RuntimeProductionPendingDrainFinalizerEnvironmentV3,
        >,
    ) -> Self {
        Self { supervisor }
    }

    #[allow(dead_code)]
    pub(crate) fn reserve_certification_finalizer_slot_v2(
        &self,
    ) -> Result<
        RuntimeReservedCertificationFinalizerSlotV2,
        RuntimeMutationFinalizerRegistrationRejectionReasonV1,
    > {
        self.supervisor
            .try_reserve_process_job_slot()
            .map(|slot| RuntimeReservedCertificationFinalizerSlotV2 { slot })
    }
}

impl Debug for RuntimeProcessCertificationFinalizerPortV2<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessCertificationFinalizerPortV2(<redacted>)")
    }
}

impl RuntimeCertificationFinalizerPortV2<PostgresPreparedRuntimeCertificationV2>
    for RuntimeProcessCertificationFinalizerPortV2<'_>
{
    type Error = RuntimeMutationFinalizerRegistrationRejectionReasonV1;
    type Accepted = RuntimeRegisteredCertificationFinalizerJobV2;

    fn accept_certification_finalizer(
        &self,
        registration: RuntimeCertificationFinalizerRegistrationV2<
            PostgresPreparedRuntimeCertificationV2,
        >,
    ) -> Result<
        Self::Accepted,
        RuntimeCertificationFinalizerRejectionV2<
            PostgresPreparedRuntimeCertificationV2,
            Self::Error,
        >,
    > {
        let slot = match self.reserve_certification_finalizer_slot_v2() {
            Ok(slot) => slot,
            Err(source) => {
                return Err(RuntimeCertificationFinalizerRejectionV2 {
                    registration: Box::new(registration),
                    source,
                });
            }
        };
        slot.submit_certification_finalizer_v2(registration)
    }
}

#[must_use]
#[allow(dead_code)]
pub(crate) struct RuntimeReservedCertificationFinalizerSlotV2 {
    slot: RuntimeMutationFinalizerReservedProcessSlotV1<
        RuntimeProcessMutationFinalizerPortV3<RuntimeProductionPendingDrainFinalizerEnvironmentV3>,
    >,
}

#[allow(dead_code)]
impl RuntimeReservedCertificationFinalizerSlotV2 {
    pub(crate) fn submit_certification_finalizer_v2(
        self,
        registration: RuntimeCertificationFinalizerRegistrationV2<
            PostgresPreparedRuntimeCertificationV2,
        >,
    ) -> Result<
        RuntimeRegisteredCertificationFinalizerJobV2,
        RuntimeCertificationFinalizerRejectionV2<
            PostgresPreparedRuntimeCertificationV2,
            RuntimeMutationFinalizerRegistrationRejectionReasonV1,
        >,
    > {
        let job = registration.into_owned_job();
        let job = RuntimeProcessMutationFinalizerJobV3::Certification(Box::new(job));
        match self.slot.submit_process_job(job) {
            Ok(waiter) => Ok(RuntimeRegisteredCertificationFinalizerJobV2 {
                job_id: waiter.job_id(),
            }),
            Err(rejected) => {
                let source = rejected.reason();
                let RuntimeProcessMutationFinalizerJobV3::Certification(job) = rejected.into_job()
                else {
                    unreachable!()
                };
                Err(RuntimeCertificationFinalizerRejectionV2 {
                    registration: Box::new((*job).into_registration()),
                    source,
                })
            }
        }
    }
}

impl Debug for RuntimeReservedCertificationFinalizerSlotV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeReservedCertificationFinalizerSlotV2(<redacted>)")
    }
}

#[must_use]
#[allow(dead_code)]
pub(crate) struct RuntimeRegisteredCertificationFinalizerJobV2 {
    job_id: RuntimeMutationFinalizerJobIdV1,
}

#[allow(dead_code)]
impl RuntimeRegisteredCertificationFinalizerJobV2 {
    pub(crate) const fn job_id(&self) -> RuntimeMutationFinalizerJobIdV1 {
        self.job_id
    }
}

impl Debug for RuntimeRegisteredCertificationFinalizerJobV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegisteredCertificationFinalizerJobV2(<redacted>)")
    }
}

#[must_use]
#[allow(dead_code)]
pub(crate) enum RuntimeCertificationFinalizerCompletionFailureV2 {
    CompletionIdentityMismatch {
        expected: RuntimeMutationFinalizerJobIdV1,
        actual: RuntimeMutationFinalizerJobIdV1,
        completion: Box<RuntimeProductionMutationFinalizerCompletionV3>,
    },
    CompletionKindMismatch(Box<RuntimeProductionMutationFinalizerCompletionV3>),
    Undispatched {
        job: Box<RuntimeProductionCertificationFinalizerJobV2>,
        exit: RuntimeSupervisorExitV1,
    },
    DispatchedTerminal(RuntimeSupervisorExitV1),
}

impl Debug for RuntimeCertificationFinalizerCompletionFailureV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCertificationFinalizerCompletionFailureV2(<redacted>)")
    }
}

pub(crate) type RuntimeProcessMutationFinalizerCompletionV3<E> =
    RuntimeMutationFinalizerCompletionV1<
        RuntimeProcessMutationFinalizerJobV3<E>,
        RuntimeProcessMutationFinalizerOutputV3<E>,
        RuntimeProcessMutationFinalizerErrorV3<E>,
    >;

#[must_use]
#[allow(dead_code)]
pub(crate) struct RuntimeProductionMutationFinalizerCompletionV3 {
    completion: RuntimeProcessMutationFinalizerCompletionV3<
        RuntimeProductionPendingDrainFinalizerEnvironmentV3,
    >,
}

impl RuntimeProductionMutationFinalizerCompletionV3 {
    pub(super) fn new(
        completion: RuntimeProcessMutationFinalizerCompletionV3<
            RuntimeProductionPendingDrainFinalizerEnvironmentV3,
        >,
    ) -> Self {
        Self { completion }
    }
}

impl Debug for RuntimeProductionMutationFinalizerCompletionV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProductionMutationFinalizerCompletionV3(<redacted>)")
    }
}

#[allow(dead_code)]
pub(crate) fn complete_certification_finalizer_job_v2(
    registered: RuntimeRegisteredCertificationFinalizerJobV2,
    completion: RuntimeProductionMutationFinalizerCompletionV3,
) -> Result<
    RuntimeProductionCertificationFinalizationOutcomeV2,
    RuntimeCertificationFinalizerCompletionFailureV2,
> {
    let RuntimeProductionMutationFinalizerCompletionV3 { completion } = completion;
    let expected = registered.job_id();
    let actual = completion.job_id();
    if actual != expected {
        return Err(
            RuntimeCertificationFinalizerCompletionFailureV2::CompletionIdentityMismatch {
                expected,
                actual,
                completion: Box::new(RuntimeProductionMutationFinalizerCompletionV3::new(
                    completion,
                )),
            },
        );
    }
    let completion_kind_matches = matches!(
        completion.result(),
        RuntimeMutationFinalizerCompletionResultV1::Settled(
            RuntimeProcessMutationFinalizerOutputV3::Certification(_)
        ) | RuntimeMutationFinalizerCompletionResultV1::Undispatched {
            job: RuntimeMutationFinalizerJobV1::ProcessMutation(
                RuntimeProcessMutationFinalizerJobV3::Certification(_)
            ),
            ..
        } | RuntimeMutationFinalizerCompletionResultV1::DispatchedTerminal(_)
    );
    if !completion_kind_matches {
        return Err(
            RuntimeCertificationFinalizerCompletionFailureV2::CompletionKindMismatch(Box::new(
                RuntimeProductionMutationFinalizerCompletionV3::new(completion),
            )),
        );
    }
    match completion.into_result() {
        RuntimeMutationFinalizerCompletionResultV1::Settled(
            RuntimeProcessMutationFinalizerOutputV3::Certification(outcome),
        ) => Ok(*outcome),
        RuntimeMutationFinalizerCompletionResultV1::Undispatched {
            job:
                RuntimeMutationFinalizerJobV1::ProcessMutation(
                    RuntimeProcessMutationFinalizerJobV3::Certification(job),
                ),
            exit,
        } => Err(RuntimeCertificationFinalizerCompletionFailureV2::Undispatched { job, exit }),
        RuntimeMutationFinalizerCompletionResultV1::DispatchedTerminal(exit) => {
            Err(RuntimeCertificationFinalizerCompletionFailureV2::DispatchedTerminal(exit))
        }
        _ => unreachable!(),
    }
}
