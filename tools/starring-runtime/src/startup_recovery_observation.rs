use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::time::Instant;

use automation_runtime_worker::{
    RuntimeAcceptedStartupRecoveryOutcomeV2, RuntimeAuthorizedStartupRecoveryObservationV2,
    RuntimeCompletedStartupRecoveryObservationV2, RuntimeStartupRecoveryObservationPortV2,
};
use tokio::time::{sleep_until, Instant as TokioInstant};

#[cfg(test)]
use crate::closed_recovery::RuntimeClosedRecoveryReadinessRefreshErrorV2;
use crate::closed_recovery::{
    revalidate_committed_recovery_v2, RuntimeClosedRecoveryCommitErrorV2,
    RuntimeClosedRecoveryFixedPointV2, RuntimeClosedRecoveryReadyIterationV2,
    RuntimeClosedRecoverySessionV2, RuntimeClosedRecoveryStartupIterationOutcomeV2,
};
use crate::gateway::RuntimeGatewayRecoverySectionErrorV2;
use crate::gateway_owner_startup_watchdog::{
    RuntimeGatewayOwnerClosedRecoveryCommitErrorV2, RuntimeGatewayOwnerClosedRecoverySupervisorV2,
};
use crate::registry::RuntimeRegistryRecoveryObservationErrorV1;

#[derive(PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeClosedRecoveryStartupObservationErrorV2<E> {
    #[error("runtime closed recovery startup observation deadline elapsed")]
    DeadlineElapsed,
    #[error("runtime closed recovery startup observation failed")]
    Observer(E),
    #[error("runtime closed recovery startup observation gateway section failed")]
    Gateway(RuntimeGatewayRecoverySectionErrorV2),
    #[error("runtime closed recovery startup observation registry binding failed")]
    Registry(RuntimeRegistryRecoveryObservationErrorV1),
    #[error("runtime closed recovery startup observation owner failed")]
    Owner(RuntimeGatewayOwnerClosedRecoveryCommitErrorV2),
}

impl<E> Debug for RuntimeClosedRecoveryStartupObservationErrorV2<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeClosedRecoveryStartupObservationErrorV2(<redacted>)")
    }
}

pub(crate) enum RuntimeClosedRecoveryStartupObservationAttemptErrorV2<E, I> {
    Observation(RuntimeClosedRecoveryStartupObservationErrorV2<E>),
    Interrupted(I),
}

impl<E, I> Debug for RuntimeClosedRecoveryStartupObservationAttemptErrorV2<E, I> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeClosedRecoveryStartupObservationAttemptErrorV2(<redacted>)")
    }
}

pub(crate) struct RuntimeClosedRecoveryStartupObservationCleanupV2 {
    owner: RuntimeGatewayOwnerClosedRecoverySupervisorV2,
}

impl RuntimeClosedRecoveryStartupObservationCleanupV2 {
    pub(crate) async fn abort_and_shutdown_until_v2(
        self,
        cleanup_deadline: Instant,
    ) -> Result<
        crate::RuntimeGatewayOwnerStartupWatchdogExitV1,
        crate::gateway_owner_startup_watchdog::RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1,
    > {
        self.owner
            .abort_and_shutdown_until_v2(cleanup_deadline)
            .await
    }
}

pub(crate) struct RuntimeClosedRecoveryStartupObservationFailureV2<E, I> {
    error: RuntimeClosedRecoveryStartupObservationAttemptErrorV2<E, I>,
    cleanup: RuntimeClosedRecoveryStartupObservationCleanupV2,
}

pub(crate) struct RuntimeClosedRecoveryStartupObservationCompletionV2 {
    completed: RuntimeCompletedStartupRecoveryObservationV2,
    observation_cutoff: Instant,
}

impl<E, I> Debug for RuntimeClosedRecoveryStartupObservationFailureV2<E, I> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeClosedRecoveryStartupObservationFailureV2(<redacted>)")
    }
}

impl Debug for RuntimeClosedRecoveryStartupObservationCompletionV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeClosedRecoveryStartupObservationCompletionV2(<redacted>)")
    }
}

impl<E, I> RuntimeClosedRecoveryStartupObservationFailureV2<E, I> {
    pub(crate) fn into_parts(
        self,
    ) -> (
        RuntimeClosedRecoveryStartupObservationAttemptErrorV2<E, I>,
        RuntimeClosedRecoveryStartupObservationCleanupV2,
    ) {
        (self.error, self.cleanup)
    }
}

impl RuntimeClosedRecoveryReadyIterationV2 {
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "runtime startup fixed-point composition consumes the atomic observation"
        )
    )]
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) async fn observe_startup_recovery_v2<P>(
        self,
        observer: &P,
    ) -> Result<
        RuntimeClosedRecoveryStartupIterationOutcomeV2,
        RuntimeClosedRecoveryStartupObservationErrorV2<P::Error>,
    >
    where
        P: RuntimeStartupRecoveryObservationPortV2 + Sync,
    {
        self.observe_startup_recovery_with_v2(
            |authorization, cutoff| observer.observe_startup_recovery(authorization, cutoff),
            || {},
            || {},
        )
        .await
    }

    async fn observe_startup_recovery_with_v2<
        Observe,
        Observation,
        PostComplete,
        PostRevalidate,
        E,
    >(
        self,
        observe: Observe,
        post_complete: PostComplete,
        post_revalidate: PostRevalidate,
    ) -> Result<
        RuntimeClosedRecoveryStartupIterationOutcomeV2,
        RuntimeClosedRecoveryStartupObservationErrorV2<E>,
    >
    where
        Observe: FnOnce(RuntimeAuthorizedStartupRecoveryObservationV2, Instant) -> Observation,
        Observation: Future<Output = Result<RuntimeCompletedStartupRecoveryObservationV2, E>>,
        PostComplete: FnOnce(),
        PostRevalidate: FnOnce(),
    {
        let mut ready = self;
        let completion = ready
            .observe_startup_recovery_interruptible_in_place_with_v2(
                observe,
                std::future::pending::<std::convert::Infallible>(),
            )
            .await
            .map_err(unwrap_observation_attempt_v2)?;
        ready
            .finalize_startup_recovery_observation_with_v2(
                completion,
                post_complete,
                post_revalidate,
            )
            .map_err(unwrap_observation_finalize_failure_v2::<E>)
    }

    pub(crate) async fn observe_startup_recovery_interruptible_in_place_v2<P, Interrupt, I>(
        &mut self,
        observer: &P,
        interrupt: Interrupt,
    ) -> Result<
        RuntimeClosedRecoveryStartupObservationCompletionV2,
        RuntimeClosedRecoveryStartupObservationAttemptErrorV2<P::Error, I>,
    >
    where
        P: RuntimeStartupRecoveryObservationPortV2 + Sync,
        Interrupt: Future<Output = I>,
    {
        self.observe_startup_recovery_interruptible_in_place_with_v2(
            |authorization, cutoff| observer.observe_startup_recovery(authorization, cutoff),
            interrupt,
        )
        .await
    }

    async fn observe_startup_recovery_interruptible_in_place_with_v2<
        Observe,
        Observation,
        Interrupt,
        E,
        I,
    >(
        &mut self,
        observe: Observe,
        interrupt: Interrupt,
    ) -> Result<
        RuntimeClosedRecoveryStartupObservationCompletionV2,
        RuntimeClosedRecoveryStartupObservationAttemptErrorV2<E, I>,
    >
    where
        Observe: FnOnce(RuntimeAuthorizedStartupRecoveryObservationV2, Instant) -> Observation,
        Observation: Future<Output = Result<RuntimeCompletedStartupRecoveryObservationV2, E>>,
        Interrupt: Future<Output = I>,
    {
        self.revalidate_v2()
            .map_err(map_commit_observation_error_v2)
            .map_err(RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation)?;
        let observation_cutoff = self
            .operation_cutoff
            .min(self.owner.observation().safety_deadline());
        if Instant::now() >= observation_cutoff {
            return Err(
                RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(
                    RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed,
                ),
            );
        }
        let iteration = self.iteration.take().ok_or(
            RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(
                RuntimeClosedRecoveryStartupObservationErrorV2::Gateway(
                    RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation,
                ),
            ),
        )?;
        let authorization = match self
            .gateway
            .begin_startup_recovery_observation_v2(&self.owner, iteration)
        {
            Ok(authorization) => authorization,
            Err(error) => {
                return Err(
                    RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(
                        RuntimeClosedRecoveryStartupObservationErrorV2::Gateway(error),
                    ),
                );
            }
        };
        if Instant::now() >= observation_cutoff {
            self.gateway.invalidate_capability_not_ready_v2();
            return Err(
                RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(
                    RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed,
                ),
            );
        }
        tokio::pin!(interrupt);
        let observed = observe(authorization, observation_cutoff);
        tokio::pin!(observed);
        let completed = tokio::select! {
            biased;
            _ = sleep_until(TokioInstant::from_std(observation_cutoff)) => {
                self.gateway.invalidate_capability_not_ready_v2();
                return Err(
                    RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(
                        RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed,
                    ),
                );
            }
            interrupted = &mut interrupt => {
                self.gateway.invalidate_capability_not_ready_v2();
                return Err(
                    RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Interrupted(interrupted),
                );
            }
            result = &mut observed => {
                match result {
                    Ok(completed) => completed,
                    Err(error) => {
                        if Instant::now() >= observation_cutoff {
                            self.gateway.invalidate_capability_not_ready_v2();
                            return Err(
                                RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(
                                    RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed,
                                ),
                            );
                        }
                        self.gateway.invalidate_capability_not_ready_v2();
                        return Err(
                            RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(
                                RuntimeClosedRecoveryStartupObservationErrorV2::Observer(error),
                            ),
                        );
                    }
                }
            }
        };
        if Instant::now() >= observation_cutoff {
            self.gateway.invalidate_capability_not_ready_v2();
            return Err(
                RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(
                    RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed,
                ),
            );
        }
        revalidate_committed_recovery_v2(
            &self.owner,
            &self.gateway,
            &self.registry,
            self.operation_cutoff,
        )
        .map_err(map_commit_observation_error_v2)
        .map_err(RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation)?;
        Ok(RuntimeClosedRecoveryStartupObservationCompletionV2 {
            completed,
            observation_cutoff,
        })
    }

    pub(crate) fn into_startup_recovery_observation_outcome_v2(
        self,
        completion: RuntimeClosedRecoveryStartupObservationCompletionV2,
    ) -> Result<
        RuntimeClosedRecoveryStartupIterationOutcomeV2,
        Box<
            RuntimeClosedRecoveryStartupObservationFailureV2<
                std::convert::Infallible,
                std::convert::Infallible,
            >,
        >,
    > {
        self.finalize_startup_recovery_observation_with_v2(completion, || {}, || {})
    }

    fn finalize_startup_recovery_observation_with_v2<PostComplete, PostRevalidate>(
        self,
        completion: RuntimeClosedRecoveryStartupObservationCompletionV2,
        post_complete: PostComplete,
        post_revalidate: PostRevalidate,
    ) -> Result<
        RuntimeClosedRecoveryStartupIterationOutcomeV2,
        Box<
            RuntimeClosedRecoveryStartupObservationFailureV2<
                std::convert::Infallible,
                std::convert::Infallible,
            >,
        >,
    >
    where
        PostComplete: FnOnce(),
        PostRevalidate: FnOnce(),
    {
        let RuntimeClosedRecoveryReadyIterationV2 {
            owner,
            gateway,
            registry,
            operation_cutoff,
            iteration,
        } = self;
        if iteration.is_some() {
            return Err(retain_observation_failure_v2(
                owner,
                RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(
                    RuntimeClosedRecoveryStartupObservationErrorV2::Gateway(
                        RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation,
                    ),
                ),
            ));
        }
        let RuntimeClosedRecoveryStartupObservationCompletionV2 {
            completed,
            observation_cutoff,
        } = completion;
        if Instant::now() >= observation_cutoff {
            return Err(retain_observation_failure_v2(
                owner,
                RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(
                    RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed,
                ),
            ));
        }
        if let Err(error) =
            revalidate_committed_recovery_v2(&owner, &gateway, &registry, operation_cutoff)
        {
            return Err(retain_observation_failure_v2(
                owner,
                RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(
                    map_commit_observation_error_v2(error),
                ),
            ));
        }
        let (gateway, outcome) =
            match gateway.into_startup_recovery_observation_successor_v2(&owner, completed) {
                Ok(successor) => successor,
                Err(error) => {
                    return Err(retain_observation_failure_v2(
                        owner,
                        RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(
                            RuntimeClosedRecoveryStartupObservationErrorV2::Gateway(error),
                        ),
                    ));
                }
            };
        post_complete();
        if Instant::now() >= observation_cutoff {
            return Err(retain_observation_failure_v2(
                owner,
                RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(
                    RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed,
                ),
            ));
        }
        if let Err(error) =
            revalidate_committed_recovery_v2(&owner, &gateway, &registry, operation_cutoff)
        {
            return Err(retain_observation_failure_v2(
                owner,
                RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(
                    map_commit_observation_error_v2(error),
                ),
            ));
        }
        post_revalidate();
        if Instant::now() >= observation_cutoff {
            return Err(retain_observation_failure_v2(
                owner,
                RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(
                    RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed,
                ),
            ));
        }
        if let Err(error) =
            revalidate_committed_recovery_v2(&owner, &gateway, &registry, operation_cutoff)
        {
            return Err(retain_observation_failure_v2(
                owner,
                RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(
                    map_commit_observation_error_v2(error),
                ),
            ));
        }
        if Instant::now() >= observation_cutoff {
            return Err(retain_observation_failure_v2(
                owner,
                RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(
                    RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed,
                ),
            ));
        }
        match outcome {
            RuntimeAcceptedStartupRecoveryOutcomeV2::Continue(continuation) => {
                Ok(RuntimeClosedRecoveryStartupIterationOutcomeV2::Continue {
                    session: RuntimeClosedRecoverySessionV2 {
                        owner,
                        gateway,
                        registry,
                        operation_cutoff,
                        readiness: super::RuntimeClosedRecoveryReadinessStateV2::Available,
                    },
                    continuation,
                })
            }
            RuntimeAcceptedStartupRecoveryOutcomeV2::FixedPoint(proof) => {
                if let Err(error) = gateway.validate_startup_recovery_fixed_point_v2(&owner, &proof)
                {
                    return Err(retain_observation_failure_v2(
                        owner,
                        RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(
                            RuntimeClosedRecoveryStartupObservationErrorV2::Gateway(error),
                        ),
                    ));
                }
                if Instant::now() >= observation_cutoff {
                    return Err(retain_observation_failure_v2(
                        owner,
                        RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(
                            RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed,
                        ),
                    ));
                }
                let fixed_point = RuntimeClosedRecoveryFixedPointV2 {
                    owner,
                    gateway,
                    registry,
                    operation_cutoff,
                    proof,
                };
                if let Err(error) = fixed_point.revalidate_v2() {
                    return Err(retain_fixed_point_observation_failure_v2(
                        fixed_point,
                        RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(
                            map_commit_observation_error_v2(error),
                        ),
                    ));
                }
                if Instant::now() >= observation_cutoff {
                    return Err(retain_fixed_point_observation_failure_v2(
                        fixed_point,
                        RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(
                            RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed,
                        ),
                    ));
                }
                Ok(RuntimeClosedRecoveryStartupIterationOutcomeV2::FixedPoint(
                    fixed_point,
                ))
            }
        }
    }
}

fn retain_fixed_point_observation_failure_v2<E, I>(
    fixed_point: RuntimeClosedRecoveryFixedPointV2,
    error: RuntimeClosedRecoveryStartupObservationAttemptErrorV2<E, I>,
) -> Box<RuntimeClosedRecoveryStartupObservationFailureV2<E, I>> {
    let RuntimeClosedRecoveryFixedPointV2 {
        owner,
        gateway,
        registry,
        operation_cutoff,
        proof,
    } = fixed_point;
    drop((gateway, registry, operation_cutoff, proof));
    retain_observation_failure_v2(owner, error)
}

fn retain_observation_failure_v2<E, I>(
    owner: RuntimeGatewayOwnerClosedRecoverySupervisorV2,
    error: RuntimeClosedRecoveryStartupObservationAttemptErrorV2<E, I>,
) -> Box<RuntimeClosedRecoveryStartupObservationFailureV2<E, I>> {
    Box::new(RuntimeClosedRecoveryStartupObservationFailureV2 {
        error,
        cleanup: RuntimeClosedRecoveryStartupObservationCleanupV2 { owner },
    })
}

fn unwrap_observation_attempt_v2<E>(
    attempt: RuntimeClosedRecoveryStartupObservationAttemptErrorV2<E, std::convert::Infallible>,
) -> RuntimeClosedRecoveryStartupObservationErrorV2<E> {
    match attempt {
        RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(error) => error,
        RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Interrupted(never) => match never {},
    }
}

fn unwrap_observation_finalize_failure_v2<E>(
    failure: Box<
        RuntimeClosedRecoveryStartupObservationFailureV2<
            std::convert::Infallible,
            std::convert::Infallible,
        >,
    >,
) -> RuntimeClosedRecoveryStartupObservationErrorV2<E> {
    let (attempt, cleanup) = (*failure).into_parts();
    drop(cleanup);
    match attempt {
        RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(error) => {
            widen_infallible_observation_error_v2(error)
        }
        RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Interrupted(never) => match never {},
    }
}

fn widen_infallible_observation_error_v2<E>(
    error: RuntimeClosedRecoveryStartupObservationErrorV2<std::convert::Infallible>,
) -> RuntimeClosedRecoveryStartupObservationErrorV2<E> {
    match error {
        RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed => {
            RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed
        }
        RuntimeClosedRecoveryStartupObservationErrorV2::Observer(never) => match never {},
        RuntimeClosedRecoveryStartupObservationErrorV2::Gateway(error) => {
            RuntimeClosedRecoveryStartupObservationErrorV2::Gateway(error)
        }
        RuntimeClosedRecoveryStartupObservationErrorV2::Registry(error) => {
            RuntimeClosedRecoveryStartupObservationErrorV2::Registry(error)
        }
        RuntimeClosedRecoveryStartupObservationErrorV2::Owner(error) => {
            RuntimeClosedRecoveryStartupObservationErrorV2::Owner(error)
        }
    }
}

fn map_commit_observation_error_v2<E>(
    error: RuntimeClosedRecoveryCommitErrorV2,
) -> RuntimeClosedRecoveryStartupObservationErrorV2<E> {
    match error {
        RuntimeClosedRecoveryCommitErrorV2::DeadlineElapsed => {
            RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed
        }
        RuntimeClosedRecoveryCommitErrorV2::Gateway(error) => {
            RuntimeClosedRecoveryStartupObservationErrorV2::Gateway(error)
        }
        RuntimeClosedRecoveryCommitErrorV2::Registry(error) => {
            RuntimeClosedRecoveryStartupObservationErrorV2::Registry(error)
        }
        RuntimeClosedRecoveryCommitErrorV2::Owner(error) => {
            RuntimeClosedRecoveryStartupObservationErrorV2::Owner(error)
        }
    }
}

#[cfg(test)]
impl RuntimeClosedRecoverySessionV2 {
    pub(crate) async fn observe_startup_recovery_with_test_observer_v2<Observe, Observation, E>(
        self,
        observe: Observe,
    ) -> Result<
        RuntimeClosedRecoveryStartupIterationOutcomeV2,
        RuntimeClosedRecoveryStartupObservationErrorV2<E>,
    >
    where
        Observe: FnOnce(RuntimeAuthorizedStartupRecoveryObservationV2, Instant) -> Observation,
        Observation: Future<Output = Result<RuntimeCompletedStartupRecoveryObservationV2, E>>,
    {
        let ready = self
            .refresh_iteration_readiness_with_v2(
                |_| {
                    std::future::ready(Ok(
                        crate::database::runtime_database_readiness_refresh_for_test_v2(),
                    ))
                },
                || {},
            )
            .await
            .map_err(map_readiness_observation_error_v2)?;
        ready
            .observe_startup_recovery_with_v2(observe, || {}, || {})
            .await
    }

    pub(crate) async fn observe_startup_recovery_after_test_hook_v2<
        Observe,
        Observation,
        PostComplete,
        E,
    >(
        self,
        observe: Observe,
        post_complete: PostComplete,
    ) -> Result<
        RuntimeClosedRecoveryStartupIterationOutcomeV2,
        RuntimeClosedRecoveryStartupObservationErrorV2<E>,
    >
    where
        Observe: FnOnce(RuntimeAuthorizedStartupRecoveryObservationV2, Instant) -> Observation,
        Observation: Future<Output = Result<RuntimeCompletedStartupRecoveryObservationV2, E>>,
        PostComplete: FnOnce(),
    {
        let ready = self
            .refresh_iteration_readiness_with_v2(
                |_| {
                    std::future::ready(Ok(
                        crate::database::runtime_database_readiness_refresh_for_test_v2(),
                    ))
                },
                || {},
            )
            .await
            .map_err(map_readiness_observation_error_v2)?;
        ready
            .observe_startup_recovery_with_v2(observe, post_complete, || {})
            .await
    }

    pub(crate) async fn observe_startup_recovery_after_final_revalidation_test_hook_v2<
        Observe,
        Observation,
        PostRevalidate,
        E,
    >(
        self,
        observe: Observe,
        post_revalidate: PostRevalidate,
    ) -> Result<
        RuntimeClosedRecoveryStartupIterationOutcomeV2,
        RuntimeClosedRecoveryStartupObservationErrorV2<E>,
    >
    where
        Observe: FnOnce(RuntimeAuthorizedStartupRecoveryObservationV2, Instant) -> Observation,
        Observation: Future<Output = Result<RuntimeCompletedStartupRecoveryObservationV2, E>>,
        PostRevalidate: FnOnce(),
    {
        let ready = self
            .refresh_iteration_readiness_with_v2(
                |_| {
                    std::future::ready(Ok(
                        crate::database::runtime_database_readiness_refresh_for_test_v2(),
                    ))
                },
                || {},
            )
            .await
            .map_err(map_readiness_observation_error_v2)?;
        ready
            .observe_startup_recovery_with_v2(observe, || {}, post_revalidate)
            .await
    }
}

#[cfg(test)]
impl RuntimeClosedRecoveryReadyIterationV2 {
    pub(crate) async fn observe_startup_recovery_with_test_observer_v2<Observe, Observation, E>(
        self,
        observe: Observe,
    ) -> Result<
        RuntimeClosedRecoveryStartupIterationOutcomeV2,
        RuntimeClosedRecoveryStartupObservationErrorV2<E>,
    >
    where
        Observe: FnOnce(RuntimeAuthorizedStartupRecoveryObservationV2, Instant) -> Observation,
        Observation: Future<Output = Result<RuntimeCompletedStartupRecoveryObservationV2, E>>,
    {
        self.observe_startup_recovery_with_v2(observe, || {}, || {})
            .await
    }
}

#[cfg(test)]
fn map_readiness_observation_error_v2<E>(
    error: RuntimeClosedRecoveryReadinessRefreshErrorV2,
) -> RuntimeClosedRecoveryStartupObservationErrorV2<E> {
    match error {
        RuntimeClosedRecoveryReadinessRefreshErrorV2::DeadlineElapsed => {
            RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed
        }
        RuntimeClosedRecoveryReadinessRefreshErrorV2::Database(_) => {
            RuntimeClosedRecoveryStartupObservationErrorV2::Gateway(
                RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation,
            )
        }
        RuntimeClosedRecoveryReadinessRefreshErrorV2::Gateway(error) => {
            RuntimeClosedRecoveryStartupObservationErrorV2::Gateway(error)
        }
        RuntimeClosedRecoveryReadinessRefreshErrorV2::Registry(error) => {
            RuntimeClosedRecoveryStartupObservationErrorV2::Registry(error)
        }
        RuntimeClosedRecoveryReadinessRefreshErrorV2::Owner(error) => {
            RuntimeClosedRecoveryStartupObservationErrorV2::Owner(error)
        }
    }
}
