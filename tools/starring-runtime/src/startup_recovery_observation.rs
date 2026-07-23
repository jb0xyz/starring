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
use crate::gateway_owner_startup_watchdog::RuntimeGatewayOwnerClosedRecoveryCommitErrorV2;
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
        self.revalidate_v2()
            .map_err(map_commit_observation_error_v2)?;
        let observation_cutoff = self
            .operation_cutoff
            .min(self.owner.observation().safety_deadline());
        if Instant::now() >= observation_cutoff {
            return Err(RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed);
        }
        let RuntimeClosedRecoveryReadyIterationV2 {
            owner,
            mut gateway,
            registry,
            operation_cutoff,
            iteration,
        } = self;
        let authorization = gateway
            .begin_startup_recovery_observation_v2(&owner, iteration)
            .map_err(RuntimeClosedRecoveryStartupObservationErrorV2::Gateway)?;
        if Instant::now() >= observation_cutoff {
            return Err(RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed);
        }
        let completed = tokio::select! {
            biased;
            _ = sleep_until(TokioInstant::from_std(observation_cutoff)) => {
                return Err(RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed);
            }
            result = observe(authorization, observation_cutoff) => {
                match result {
                    Ok(completed) => completed,
                    Err(error) => {
                        if Instant::now() >= observation_cutoff {
                            return Err(
                                RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed
                            );
                        }
                        gateway.invalidate_capability_not_ready_v2();
                        return Err(
                            RuntimeClosedRecoveryStartupObservationErrorV2::Observer(error)
                        );
                    }
                }
            }
        };
        if Instant::now() >= observation_cutoff {
            return Err(RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed);
        }
        revalidate_committed_recovery_v2(&owner, &gateway, &registry, operation_cutoff)
            .map_err(map_commit_observation_error_v2)?;
        if Instant::now() >= observation_cutoff {
            return Err(RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed);
        }
        let (gateway, outcome) = gateway
            .into_startup_recovery_observation_successor_v2(&owner, completed)
            .map_err(RuntimeClosedRecoveryStartupObservationErrorV2::Gateway)?;
        post_complete();
        if Instant::now() >= observation_cutoff {
            return Err(RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed);
        }
        revalidate_committed_recovery_v2(&owner, &gateway, &registry, operation_cutoff)
            .map_err(map_commit_observation_error_v2)?;
        post_revalidate();
        if Instant::now() >= observation_cutoff {
            return Err(RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed);
        }
        revalidate_committed_recovery_v2(&owner, &gateway, &registry, operation_cutoff)
            .map_err(map_commit_observation_error_v2)?;
        if Instant::now() >= observation_cutoff {
            return Err(RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed);
        }
        match outcome {
            RuntimeAcceptedStartupRecoveryOutcomeV2::Continue(continuation) => {
                Ok(RuntimeClosedRecoveryStartupIterationOutcomeV2::Continue {
                    session: RuntimeClosedRecoverySessionV2 {
                        owner,
                        gateway,
                        registry,
                        operation_cutoff,
                    },
                    continuation,
                })
            }
            RuntimeAcceptedStartupRecoveryOutcomeV2::FixedPoint(proof) => {
                gateway
                    .validate_startup_recovery_fixed_point_v2(&owner, &proof)
                    .map_err(RuntimeClosedRecoveryStartupObservationErrorV2::Gateway)?;
                if Instant::now() >= observation_cutoff {
                    return Err(RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed);
                }
                let fixed_point = RuntimeClosedRecoveryFixedPointV2 {
                    owner,
                    gateway,
                    registry,
                    operation_cutoff,
                    proof,
                };
                fixed_point
                    .revalidate_v2()
                    .map_err(map_commit_observation_error_v2)?;
                if Instant::now() >= observation_cutoff {
                    return Err(RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed);
                }
                Ok(RuntimeClosedRecoveryStartupIterationOutcomeV2::FixedPoint(
                    fixed_point,
                ))
            }
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
