use std::fmt::{Debug, Formatter};
use std::num::NonZeroU64;

use automation_runtime_controller::{
    plan_runtime_action_v1, RuntimeControllerActionV1, RuntimeControllerPlanError,
    RuntimeConvergenceMutationV1, RuntimeConvergenceSessionError, RuntimeConvergenceSessionStateV1,
    RuntimeDisconnectServingV1, RuntimeExecutionGuardV1, RuntimeMutationRequestV1,
    RuntimeObservePreviousServingV1, RuntimePreviousServingLeaseEvidenceV1,
    RuntimePreviousServingObservationReceiptV1, RuntimePreviousServingStateV1,
    RuntimeServingReceiptV1, RuntimeServingUpdateReceiptV1,
};
use automation_runtime_convergence::{
    DrainAttestationV1, RuntimeDeploymentPhaseV1, RuntimeProcessIdentityV1,
};
use chrono::{DateTime, TimeDelta, Utc};

use super::hydration::RuntimeHydratedCoreV2;
use super::{
    RuntimeConvergenceFutureV2, RuntimeConvergenceMutationPortV2, RuntimeExactTargetEvidenceV2,
    RuntimeRouteLifecycleV2, RuntimeRouteWitnessV2, RuntimeStagedConvergenceV2,
};
use crate::RuntimeServingSlotWorkErrorV2;

pub enum RuntimeReplacementExecutionErrorV2<R, E, F> {
    Port { retained: Box<R>, source: E },
    Failed(F),
}

impl<R, E, F> RuntimeReplacementExecutionErrorV2<R, E, F> {
    pub fn retained(&self) -> Option<&R> {
        match self {
            Self::Port { retained, .. } => Some(retained),
            Self::Failed(_) => None,
        }
    }

    pub fn into_retained(self) -> Option<(R, E)> {
        match self {
            Self::Port { retained, source } => Some((*retained, source)),
            Self::Failed(_) => None,
        }
    }

    pub fn failure(&self) -> Option<&F> {
        match self {
            Self::Port { .. } => None,
            Self::Failed(failure) => Some(failure),
        }
    }
}

impl<R, E, F> Debug for RuntimeReplacementExecutionErrorV2<R, E, F> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Port { .. } => {
                formatter.write_str("RuntimeReplacementExecutionErrorV2::Port(<redacted>)")
            }
            Self::Failed(_) => {
                formatter.write_str("RuntimeReplacementExecutionErrorV2::Failed(<redacted>)")
            }
        }
    }
}

pub type RuntimeReplacementResultV2<T, R, E, F> =
    Result<T, RuntimeReplacementExecutionErrorV2<R, E, F>>;

pub type RuntimeReplacementFutureV2<'a, T, R, E, F> =
    RuntimeConvergenceFutureV2<'a, RuntimeReplacementResultV2<T, R, E, F>>;

pub type RuntimeBarrierAPauseFutureV2<'a, H, S, P, E> = RuntimeReplacementFutureV2<
    'a,
    RuntimeBarrierAPausedConvergenceV2<H, S, P>,
    RuntimeObservedPreviousServingConvergenceV2<H, S>,
    E,
    RuntimeBarrierAPauseErrorV2,
>;

pub type RuntimePredecessorTransitionResultV2<H, S, P, E> = RuntimeReplacementResultV2<
    RuntimeRoutePredecessorDrainingConvergenceV2<H, S, P>,
    RuntimeBarrierAPausedConvergenceV2<H, S, P>,
    E,
    RuntimeRoutePredecessorTransitionErrorV2,
>;

pub type RuntimeBarrierAResumeFutureV2<'a, H, S, P, R, E> = RuntimeReplacementFutureV2<
    'a,
    RuntimeBarrierAResumedConvergenceV2<H, S, R>,
    RuntimeRoutePredecessorDrainingConvergenceV2<H, S, P>,
    E,
    RuntimeBarrierAResumeErrorV2,
>;

pub type RuntimePreviousServingDisconnectFutureV2<'a, H, S, R, E> = RuntimeReplacementFutureV2<
    'a,
    RuntimePredecessorRetirementReadyConvergenceV2<H, S, R>,
    RuntimeBarrierAResumedConvergenceV2<H, S, R>,
    E,
    RuntimePreviousServingDisconnectErrorV2,
>;

pub type RuntimePredecessorRetirementFutureV2<'a, H, S, R, E> = RuntimeReplacementFutureV2<
    'a,
    RuntimePredecessorRemovedConvergenceV2<H, S, R>,
    RuntimePredecessorRetirementReadyConvergenceV2<H, S, R>,
    E,
    RuntimePredecessorRetirementErrorV2,
>;

pub type RuntimeAcceptDrainFutureV2<'a, H, S, R, E> = RuntimeReplacementFutureV2<
    'a,
    RuntimeDrainedConvergenceV2<H, S, R>,
    RuntimePredecessorRemovedConvergenceV2<H, S, R>,
    E,
    RuntimeAcceptDrainMutationErrorV2,
>;

struct RuntimeReplacementCoreV2<H, S> {
    staged: S,
    witness: RuntimeRouteWitnessV2,
    core: RuntimeHydratedCoreV2<H>,
}

impl<H, S> RuntimeReplacementCoreV2<H, S> {
    fn ensure_active(&self) -> Result<(), RuntimeServingSlotWorkErrorV2> {
        self.core.claimed.ensure_active()
    }

    fn into_staged(self) -> RuntimeStagedConvergenceV2<H, S> {
        RuntimeStagedConvergenceV2 {
            staged: self.staged,
            witness: self.witness,
            core: self.core,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeRequestDrainMutationErrorV2 {
    #[error("runtime route replacement slot work authority is not active")]
    SlotWork(RuntimeServingSlotWorkErrorV2),
    #[error("runtime route replacement session mutation failed")]
    Session(RuntimeConvergenceSessionError),
    #[error("runtime route replacement requires a staged route")]
    StagedRouteRequired,
    #[error("runtime route replacement requires PreflightReady")]
    PreflightReadyRequired,
    #[error("runtime route replacement recovery requires DrainRequested")]
    DrainRequestedRequired,
    #[error("runtime route replacement completion time is invalid")]
    InvalidCompletionTime,
    #[error("runtime route replacement successor cannot be planned")]
    Plan(RuntimeControllerPlanError),
    #[error("runtime route replacement requires controller renewal")]
    RenewalRequired,
    #[error("runtime route replacement received an unexpected successor plan")]
    UnexpectedPlan,
}

pub struct RuntimeRequestDrainMutationV2<H, S> {
    replacement: RuntimeReplacementCoreV2<H, S>,
    request: RuntimeMutationRequestV1,
}

impl<H, S> RuntimeRequestDrainMutationV2<H, S> {
    pub fn request(&self) -> &RuntimeMutationRequestV1 {
        &self.request
    }

    pub fn execute<'a, P>(
        self,
        port: &'a P,
        completed_at: DateTime<Utc>,
    ) -> RuntimeReplacementFutureV2<
        'a,
        RuntimeDrainRequestedConvergenceV2<H, S>,
        RuntimeStagedConvergenceV2<H, S>,
        P::Error,
        RuntimeRequestDrainMutationErrorV2,
    >
    where
        H: Send + 'a,
        S: Send + 'a,
        P: RuntimeConvergenceMutationPortV2 + Sync + 'a,
        P::Error: Send + 'a,
    {
        Box::pin(async move {
            let mut replacement = self.replacement;
            replacement.ensure_active().map_err(|error| {
                RuntimeReplacementExecutionErrorV2::Failed(
                    RuntimeRequestDrainMutationErrorV2::SlotWork(error),
                )
            })?;
            let receipt = match port.mutate(&self.request).await {
                Ok(receipt) => receipt,
                Err(source) => {
                    if let Err(error) = replacement
                        .core
                        .claimed
                        .session
                        .abort_action(self.request.action_id)
                    {
                        return Err(RuntimeReplacementExecutionErrorV2::Failed(
                            RuntimeRequestDrainMutationErrorV2::Session(error),
                        ));
                    }
                    return Err(RuntimeReplacementExecutionErrorV2::Port {
                        retained: Box::new(replacement.into_staged()),
                        source,
                    });
                }
            };
            replacement.ensure_active().map_err(|error| {
                RuntimeReplacementExecutionErrorV2::Failed(
                    RuntimeRequestDrainMutationErrorV2::SlotWork(error),
                )
            })?;
            let state = replacement
                .core
                .claimed
                .session
                .apply_mutation(receipt)
                .map_err(|error| {
                    RuntimeReplacementExecutionErrorV2::Failed(
                        RuntimeRequestDrainMutationErrorV2::Session(error),
                    )
                })?;
            if state != RuntimeConvergenceSessionStateV1::Active {
                return Err(RuntimeReplacementExecutionErrorV2::Failed(
                    RuntimeRequestDrainMutationErrorV2::Session(
                        RuntimeConvergenceSessionError::InactiveSession,
                    ),
                ));
            }
            if !matches!(
                replacement.core.claimed.session.snapshot().phase,
                RuntimeDeploymentPhaseV1::DrainRequested
            ) {
                return Err(RuntimeReplacementExecutionErrorV2::Failed(
                    RuntimeRequestDrainMutationErrorV2::PreflightReadyRequired,
                ));
            }
            if completed_at < replacement.core.evidence.observed_database_now()
                || completed_at >= replacement.core.claimed.session.expires_at()
            {
                return Err(RuntimeReplacementExecutionErrorV2::Failed(
                    RuntimeRequestDrainMutationErrorV2::InvalidCompletionTime,
                ));
            }
            match plan_runtime_action_v1(
                replacement.core.claimed.session.snapshot(),
                replacement.core.claimed.session.controller_id(),
                completed_at,
                &replacement.core.claimed.config,
            )
            .map_err(|error| {
                RuntimeReplacementExecutionErrorV2::Failed(
                    RuntimeRequestDrainMutationErrorV2::Plan(error),
                )
            })? {
                RuntimeControllerActionV1::DrainPreviousRuntime { .. } => {}
                RuntimeControllerActionV1::RenewControllerLease { .. } => {
                    return Err(RuntimeReplacementExecutionErrorV2::Failed(
                        RuntimeRequestDrainMutationErrorV2::RenewalRequired,
                    ));
                }
                _ => {
                    return Err(RuntimeReplacementExecutionErrorV2::Failed(
                        RuntimeRequestDrainMutationErrorV2::UnexpectedPlan,
                    ));
                }
            }
            replacement.ensure_active().map_err(|error| {
                RuntimeReplacementExecutionErrorV2::Failed(
                    RuntimeRequestDrainMutationErrorV2::SlotWork(error),
                )
            })?;
            Ok(RuntimeDrainRequestedConvergenceV2 {
                replacement,
                requested_at: completed_at,
            })
        })
    }
}

impl<H, S> Debug for RuntimeRequestDrainMutationV2<H, S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRequestDrainMutationV2(<redacted>)")
    }
}

pub struct RuntimeDrainRequestedConvergenceV2<H, S> {
    replacement: RuntimeReplacementCoreV2<H, S>,
    requested_at: DateTime<Utc>,
}

impl<H, S> RuntimeDrainRequestedConvergenceV2<H, S> {
    pub fn witness(&self) -> &RuntimeRouteWitnessV2 {
        &self.replacement.witness
    }

    pub fn hydrated(&self) -> &H {
        &self.replacement.core.hydrated
    }

    pub fn staged(&self) -> &S {
        &self.replacement.staged
    }

    pub fn requested_at(&self) -> DateTime<Utc> {
        self.requested_at
    }

    pub fn begin_previous_serving_observation(
        mut self,
    ) -> Result<RuntimeExactPreviousServingV2<H, S>, RuntimeExactPreviousServingErrorV2> {
        self.replacement
            .ensure_active()
            .map_err(RuntimeExactPreviousServingErrorV2::SlotWork)?;
        let request = self
            .replacement
            .core
            .claimed
            .session
            .begin_previous_serving_observation()
            .map_err(RuntimeExactPreviousServingErrorV2::Session)?;
        self.replacement
            .ensure_active()
            .map_err(RuntimeExactPreviousServingErrorV2::SlotWork)?;
        Ok(RuntimeExactPreviousServingV2 {
            drain_requested: self,
            request,
        })
    }
}

impl<H, S> Debug for RuntimeDrainRequestedConvergenceV2<H, S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDrainRequestedConvergenceV2(<redacted>)")
    }
}

pub trait RuntimeExactPreviousServingObservationPortV2 {
    type Error;

    fn observe_previous_serving<'a>(
        &'a self,
        request: &'a RuntimeObservePreviousServingV1,
    ) -> RuntimeConvergenceFutureV2<
        'a,
        Result<RuntimePreviousServingObservationReceiptV1, Self::Error>,
    >;
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeExactPreviousServingEvidenceErrorV2 {
    #[error("runtime previous serving observation predates RequestDrain")]
    ObservationBeforeDrainRequest,
    #[error("runtime previous serving lease is fresh and belongs to another process")]
    FreshForeignPredecessor,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeExactPreviousServingErrorV2 {
    #[error("runtime previous serving observation slot work authority is not active")]
    SlotWork(RuntimeServingSlotWorkErrorV2),
    #[error("runtime previous serving observation session failed")]
    Session(RuntimeConvergenceSessionError),
    #[error("runtime previous serving evidence is invalid")]
    Evidence(RuntimeExactPreviousServingEvidenceErrorV2),
}

pub struct RuntimeExactPreviousServingV2<H, S> {
    drain_requested: RuntimeDrainRequestedConvergenceV2<H, S>,
    request: RuntimeObservePreviousServingV1,
}

impl<H, S> RuntimeExactPreviousServingV2<H, S> {
    pub fn request(&self) -> &RuntimeObservePreviousServingV1 {
        &self.request
    }

    pub fn execute<'a, P>(
        self,
        port: &'a P,
    ) -> RuntimeReplacementFutureV2<
        'a,
        RuntimeObservedPreviousServingConvergenceV2<H, S>,
        RuntimeDrainRequestedConvergenceV2<H, S>,
        P::Error,
        RuntimeExactPreviousServingErrorV2,
    >
    where
        H: Send + 'a,
        S: Send + 'a,
        P: RuntimeExactPreviousServingObservationPortV2 + Sync + 'a,
        P::Error: Send + 'a,
    {
        Box::pin(async move {
            let mut drain_requested = self.drain_requested;
            drain_requested
                .replacement
                .ensure_active()
                .map_err(|error| {
                    RuntimeReplacementExecutionErrorV2::Failed(
                        RuntimeExactPreviousServingErrorV2::SlotWork(error),
                    )
                })?;
            let receipt = match port.observe_previous_serving(&self.request).await {
                Ok(receipt) => receipt,
                Err(source) => {
                    if let Err(error) = drain_requested
                        .replacement
                        .core
                        .claimed
                        .session
                        .abort_action(self.request.action_id)
                    {
                        return Err(RuntimeReplacementExecutionErrorV2::Failed(
                            RuntimeExactPreviousServingErrorV2::Session(error),
                        ));
                    }
                    return Err(RuntimeReplacementExecutionErrorV2::Port {
                        retained: Box::new(drain_requested),
                        source,
                    });
                }
            };
            drain_requested
                .replacement
                .ensure_active()
                .map_err(|error| {
                    RuntimeReplacementExecutionErrorV2::Failed(
                        RuntimeExactPreviousServingErrorV2::SlotWork(error),
                    )
                })?;
            let receipt = drain_requested
                .replacement
                .core
                .claimed
                .session
                .apply_previous_serving_observation(receipt)
                .map_err(|error| {
                    RuntimeReplacementExecutionErrorV2::Failed(
                        RuntimeExactPreviousServingErrorV2::Session(error),
                    )
                })?;
            if receipt.observed_at < drain_requested.requested_at {
                return Err(RuntimeReplacementExecutionErrorV2::Failed(
                    RuntimeExactPreviousServingErrorV2::Evidence(
                        RuntimeExactPreviousServingEvidenceErrorV2::ObservationBeforeDrainRequest,
                    ),
                ));
            }
            if let RuntimePreviousServingStateV1::Serving { lease, .. } = &receipt.state {
                if lease.identity.process.process_instance_id
                    != drain_requested
                        .replacement
                        .core
                        .claimed
                        .process_identity
                        .process_instance_id
                {
                    return Err(RuntimeReplacementExecutionErrorV2::Failed(
                        RuntimeExactPreviousServingErrorV2::Evidence(
                            RuntimeExactPreviousServingEvidenceErrorV2::FreshForeignPredecessor,
                        ),
                    ));
                }
            }
            drain_requested
                .replacement
                .ensure_active()
                .map_err(|error| {
                    RuntimeReplacementExecutionErrorV2::Failed(
                        RuntimeExactPreviousServingErrorV2::SlotWork(error),
                    )
                })?;
            Ok(RuntimeObservedPreviousServingConvergenceV2 {
                replacement: drain_requested.replacement,
                requested_at: drain_requested.requested_at,
                previous_serving: receipt,
            })
        })
    }
}

impl<H, S> Debug for RuntimeExactPreviousServingV2<H, S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeExactPreviousServingV2(<redacted>)")
    }
}

pub struct RuntimeObservedPreviousServingConvergenceV2<H, S> {
    replacement: RuntimeReplacementCoreV2<H, S>,
    requested_at: DateTime<Utc>,
    previous_serving: RuntimePreviousServingObservationReceiptV1,
}

impl<H, S> RuntimeObservedPreviousServingConvergenceV2<H, S> {
    pub fn previous_serving(&self) -> &RuntimePreviousServingObservationReceiptV1 {
        &self.previous_serving
    }

    pub fn witness(&self) -> &RuntimeRouteWitnessV2 {
        &self.replacement.witness
    }

    pub fn begin_barrier_a_pause(
        self,
        started_at: DateTime<Utc>,
    ) -> Result<RuntimeBarrierAPauseV2<H, S>, RuntimeBarrierAPauseErrorV2> {
        self.replacement
            .ensure_active()
            .map_err(RuntimeBarrierAPauseErrorV2::SlotWork)?;
        if started_at < self.previous_serving.observed_at
            || started_at < self.requested_at
            || started_at >= self.replacement.core.claimed.session.expires_at()
        {
            return Err(RuntimeBarrierAPauseErrorV2::InvalidTimeWindow);
        }
        let timeout = TimeDelta::from_std(self.replacement.core.claimed.config.drain_timeout)
            .map_err(|_| RuntimeBarrierAPauseErrorV2::InvalidTimeWindow)?;
        let deadline = started_at
            .checked_add_signed(timeout)
            .ok_or(RuntimeBarrierAPauseErrorV2::InvalidTimeWindow)?;
        let renew_before =
            TimeDelta::from_std(self.replacement.core.claimed.config.controller_renew_before)
                .map_err(|_| RuntimeBarrierAPauseErrorV2::InvalidTimeWindow)?;
        let renew_at = self
            .replacement
            .core
            .claimed
            .session
            .expires_at()
            .checked_sub_signed(renew_before)
            .ok_or(RuntimeBarrierAPauseErrorV2::InvalidTimeWindow)?;
        if deadline >= renew_at {
            return Err(RuntimeBarrierAPauseErrorV2::RenewalRequired);
        }
        let execution_guard = self
            .replacement
            .core
            .claimed
            .session
            .execution_guard()
            .map_err(RuntimeBarrierAPauseErrorV2::Session)?;
        let correlation = RuntimeBarrierACorrelationV2 {
            execution_guard,
            successor_identity: self.replacement.witness.identity.clone(),
            successor_route_incarnation: self.replacement.witness.route_incarnation,
            expected_previous_runtime: self
                .replacement
                .core
                .claimed
                .session
                .snapshot()
                .previous_runtime
                .clone(),
        };
        Ok(RuntimeBarrierAPauseV2 {
            observed: self,
            request: RuntimeBarrierAPauseRequestV2 {
                correlation,
                started_at,
                deadline,
            },
        })
    }
}

impl<H, S> Debug for RuntimeObservedPreviousServingConvergenceV2<H, S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeObservedPreviousServingConvergenceV2(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeBarrierACorrelationV2 {
    pub execution_guard: RuntimeExecutionGuardV1,
    pub successor_identity: RuntimeProcessIdentityV1,
    pub successor_route_incarnation: NonZeroU64,
    pub expected_previous_runtime: Option<RuntimeProcessIdentityV1>,
}

pub struct RuntimeBarrierAPauseRequestV2 {
    correlation: RuntimeBarrierACorrelationV2,
    started_at: DateTime<Utc>,
    deadline: DateTime<Utc>,
}

impl RuntimeBarrierAPauseRequestV2 {
    pub fn correlation(&self) -> &RuntimeBarrierACorrelationV2 {
        &self.correlation
    }

    pub fn started_at(&self) -> DateTime<Utc> {
        self.started_at
    }

    pub fn deadline(&self) -> DateTime<Utc> {
        self.deadline
    }
}

impl Debug for RuntimeBarrierAPauseRequestV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeBarrierAPauseRequestV2(<redacted>)")
    }
}

pub struct RuntimeBarrierAPauseObservationV2<P> {
    pub correlation: RuntimeBarrierACorrelationV2,
    pub coordinator_generation: NonZeroU64,
    pub connection_epoch: NonZeroU64,
    pub admission_revision: NonZeroU64,
    pub connected_event_sequence: NonZeroU64,
    pub pause_sequence: NonZeroU64,
    pub paused_at: DateTime<Utc>,
    pub paused: P,
}

impl<P> Debug for RuntimeBarrierAPauseObservationV2<P> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeBarrierAPauseObservationV2(<redacted>)")
    }
}

pub trait RuntimeBarrierAPausePortV2 {
    type Error;
    type Paused;

    fn pause_barrier_a<'a>(
        &'a self,
        request: &'a RuntimeBarrierAPauseRequestV2,
    ) -> RuntimeConvergenceFutureV2<
        'a,
        Result<RuntimeBarrierAPauseObservationV2<Self::Paused>, Self::Error>,
    >;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeBarrierAPauseEvidenceErrorV2 {
    #[error("runtime Barrier A pause correlation does not match")]
    CorrelationMismatch,
    #[error("runtime Barrier A pause time is outside its operation window")]
    ObservationTimeMismatch,
    #[error("runtime Barrier A pause gateway sequence is invalid")]
    GatewaySequenceMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeBarrierAPauseErrorV2 {
    #[error("runtime Barrier A pause slot work authority is not active")]
    SlotWork(RuntimeServingSlotWorkErrorV2),
    #[error("runtime Barrier A pause session guard is unavailable")]
    Session(RuntimeConvergenceSessionError),
    #[error("runtime Barrier A pause time window is invalid")]
    InvalidTimeWindow,
    #[error("runtime Barrier A pause requires controller renewal")]
    RenewalRequired,
    #[error("runtime Barrier A pause evidence is invalid")]
    Evidence(RuntimeBarrierAPauseEvidenceErrorV2),
}

pub struct RuntimeBarrierAPauseV2<H, S> {
    observed: RuntimeObservedPreviousServingConvergenceV2<H, S>,
    request: RuntimeBarrierAPauseRequestV2,
}

impl<H, S> RuntimeBarrierAPauseV2<H, S> {
    pub fn request(&self) -> &RuntimeBarrierAPauseRequestV2 {
        &self.request
    }

    pub fn execute<'a, P>(
        self,
        port: &'a P,
    ) -> RuntimeBarrierAPauseFutureV2<'a, H, S, P::Paused, P::Error>
    where
        H: Send + 'a,
        S: Send + 'a,
        P: RuntimeBarrierAPausePortV2 + Sync + 'a,
        P::Error: Send + 'a,
        P::Paused: Send + 'a,
    {
        Box::pin(async move {
            let observed = self.observed;
            observed.replacement.ensure_active().map_err(|error| {
                RuntimeReplacementExecutionErrorV2::Failed(RuntimeBarrierAPauseErrorV2::SlotWork(
                    error,
                ))
            })?;
            let observation = match port.pause_barrier_a(&self.request).await {
                Ok(observation) => observation,
                Err(source) => {
                    return Err(RuntimeReplacementExecutionErrorV2::Port {
                        retained: Box::new(observed),
                        source,
                    });
                }
            };
            observed.replacement.ensure_active().map_err(|error| {
                RuntimeReplacementExecutionErrorV2::Failed(RuntimeBarrierAPauseErrorV2::SlotWork(
                    error,
                ))
            })?;
            let (evidence, paused) = validate_pause_observation(&self.request, observation)
                .map_err(|error| {
                    RuntimeReplacementExecutionErrorV2::Failed(
                        RuntimeBarrierAPauseErrorV2::Evidence(error),
                    )
                })?;
            Ok(RuntimeBarrierAPausedConvergenceV2 {
                replacement: observed.replacement,
                previous_serving: observed.previous_serving,
                pause: evidence,
                paused,
            })
        })
    }
}

impl<H, S> Debug for RuntimeBarrierAPauseV2<H, S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeBarrierAPauseV2(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeBarrierAPauseEvidenceV2 {
    correlation: RuntimeBarrierACorrelationV2,
    coordinator_generation: NonZeroU64,
    connection_epoch: NonZeroU64,
    admission_revision: NonZeroU64,
    connected_event_sequence: NonZeroU64,
    pause_sequence: NonZeroU64,
    paused_at: DateTime<Utc>,
    deadline: DateTime<Utc>,
}

impl RuntimeBarrierAPauseEvidenceV2 {
    pub fn correlation(&self) -> &RuntimeBarrierACorrelationV2 {
        &self.correlation
    }

    pub fn coordinator_generation(&self) -> NonZeroU64 {
        self.coordinator_generation
    }

    pub fn connection_epoch(&self) -> NonZeroU64 {
        self.connection_epoch
    }

    pub fn admission_revision(&self) -> NonZeroU64 {
        self.admission_revision
    }

    pub fn connected_event_sequence(&self) -> NonZeroU64 {
        self.connected_event_sequence
    }

    pub fn pause_sequence(&self) -> NonZeroU64 {
        self.pause_sequence
    }

    pub fn paused_at(&self) -> DateTime<Utc> {
        self.paused_at
    }

    pub fn deadline(&self) -> DateTime<Utc> {
        self.deadline
    }
}

pub struct RuntimeBarrierAPausedConvergenceV2<H, S, P> {
    replacement: RuntimeReplacementCoreV2<H, S>,
    previous_serving: RuntimePreviousServingObservationReceiptV1,
    pause: RuntimeBarrierAPauseEvidenceV2,
    paused: P,
}

impl<H, S, P> RuntimeBarrierAPausedConvergenceV2<H, S, P> {
    pub fn pause_evidence(&self) -> &RuntimeBarrierAPauseEvidenceV2 {
        &self.pause
    }

    pub fn paused(&self) -> &P {
        &self.paused
    }

    pub fn begin_predecessor_transition(self) -> RuntimeRoutePredecessorTransitionV2<H, S, P> {
        let request = RuntimeRoutePredecessorTransitionRequestV2 {
            correlation: self.pause.correlation.clone(),
            initial_successor: self.replacement.witness.clone(),
            expected_previous_runtime: self
                .replacement
                .core
                .claimed
                .session
                .snapshot()
                .previous_runtime
                .clone(),
            paused_at: self.pause.paused_at,
            deadline: self.pause.deadline,
        };
        RuntimeRoutePredecessorTransitionV2 {
            paused: self,
            request,
        }
    }
}

impl<H, S, P> Debug for RuntimeBarrierAPausedConvergenceV2<H, S, P> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeBarrierAPausedConvergenceV2(<redacted>)")
    }
}

pub struct RuntimeRoutePredecessorTransitionRequestV2 {
    correlation: RuntimeBarrierACorrelationV2,
    initial_successor: RuntimeRouteWitnessV2,
    expected_previous_runtime: Option<RuntimeProcessIdentityV1>,
    paused_at: DateTime<Utc>,
    deadline: DateTime<Utc>,
}

impl RuntimeRoutePredecessorTransitionRequestV2 {
    pub fn correlation(&self) -> &RuntimeBarrierACorrelationV2 {
        &self.correlation
    }

    pub fn initial_successor(&self) -> &RuntimeRouteWitnessV2 {
        &self.initial_successor
    }

    pub fn expected_previous_runtime(&self) -> Option<&RuntimeProcessIdentityV1> {
        self.expected_previous_runtime.as_ref()
    }

    pub fn paused_at(&self) -> DateTime<Utc> {
        self.paused_at
    }

    pub fn deadline(&self) -> DateTime<Utc> {
        self.deadline
    }
}

impl Debug for RuntimeRoutePredecessorTransitionRequestV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRoutePredecessorTransitionRequestV2(<redacted>)")
    }
}

pub struct RuntimeRoutePredecessorTransitionObservationV2 {
    pub correlation: RuntimeBarrierACorrelationV2,
    pub predecessor: Option<RuntimeRouteWitnessV2>,
    pub successor: RuntimeRouteWitnessV2,
    pub transitioned_at: DateTime<Utc>,
}

impl Debug for RuntimeRoutePredecessorTransitionObservationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRoutePredecessorTransitionObservationV2(<redacted>)")
    }
}

pub trait RuntimeRoutePredecessorTransitionPortV2<H, S, P> {
    type Error;

    fn transition_predecessor_to_draining(
        &self,
        request: &RuntimeRoutePredecessorTransitionRequestV2,
        hydrated: &H,
        staged: &S,
        paused: &P,
    ) -> Result<RuntimeRoutePredecessorTransitionObservationV2, Self::Error>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeRoutePredecessorTransitionEvidenceErrorV2 {
    #[error("runtime predecessor transition correlation does not match")]
    CorrelationMismatch,
    #[error("runtime predecessor transition successor does not match")]
    SuccessorMismatch,
    #[error("runtime predecessor transition successor is not staged")]
    SuccessorLifecycleMismatch,
    #[error("runtime predecessor transition successor has active interactions")]
    SuccessorActiveInteractions,
    #[error("runtime predecessor transition successor observation regressed")]
    SuccessorObservationRegression,
    #[error("runtime predecessor transition local predecessor is missing")]
    LocalPredecessorMissing,
    #[error("runtime predecessor transition unexpectedly observed a local predecessor")]
    UnexpectedLocalPredecessor,
    #[error("runtime predecessor transition predecessor identity does not match")]
    PredecessorIdentityMismatch,
    #[error("runtime predecessor transition predecessor is not Draining")]
    PredecessorLifecycleMismatch,
    #[error("runtime predecessor transition time is outside its operation window")]
    ObservationTimeMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeRoutePredecessorTransitionErrorV2 {
    #[error("runtime predecessor transition slot work authority is not active")]
    SlotWork(RuntimeServingSlotWorkErrorV2),
    #[error("runtime predecessor transition evidence is invalid")]
    Evidence(RuntimeRoutePredecessorTransitionEvidenceErrorV2),
}

pub struct RuntimeRoutePredecessorTransitionV2<H, S, P> {
    paused: RuntimeBarrierAPausedConvergenceV2<H, S, P>,
    request: RuntimeRoutePredecessorTransitionRequestV2,
}

impl<H, S, P> RuntimeRoutePredecessorTransitionV2<H, S, P> {
    pub fn request(&self) -> &RuntimeRoutePredecessorTransitionRequestV2 {
        &self.request
    }

    pub fn execute<T>(self, port: &T) -> RuntimePredecessorTransitionResultV2<H, S, P, T::Error>
    where
        T: RuntimeRoutePredecessorTransitionPortV2<H, S, P>,
    {
        let paused = self.paused;
        paused.replacement.ensure_active().map_err(|error| {
            RuntimeReplacementExecutionErrorV2::Failed(
                RuntimeRoutePredecessorTransitionErrorV2::SlotWork(error),
            )
        })?;
        let observation = match port.transition_predecessor_to_draining(
            &self.request,
            &paused.replacement.core.hydrated,
            &paused.replacement.staged,
            &paused.paused,
        ) {
            Ok(observation) => observation,
            Err(source) => {
                return Err(RuntimeReplacementExecutionErrorV2::Port {
                    retained: Box::new(paused),
                    source,
                });
            }
        };
        paused.replacement.ensure_active().map_err(|error| {
            RuntimeReplacementExecutionErrorV2::Failed(
                RuntimeRoutePredecessorTransitionErrorV2::SlotWork(error),
            )
        })?;
        let evidence =
            validate_transition_observation(&self.request, observation).map_err(|error| {
                RuntimeReplacementExecutionErrorV2::Failed(
                    RuntimeRoutePredecessorTransitionErrorV2::Evidence(error),
                )
            })?;
        Ok(RuntimeRoutePredecessorDrainingConvergenceV2 {
            replacement: paused.replacement,
            previous_serving: paused.previous_serving,
            pause: paused.pause,
            paused: paused.paused,
            transition: evidence,
        })
    }
}

impl<H, S, P> Debug for RuntimeRoutePredecessorTransitionV2<H, S, P> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRoutePredecessorTransitionV2(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeRoutePredecessorTransitionEvidenceV2 {
    predecessor: Option<RuntimeRouteWitnessV2>,
    successor: RuntimeRouteWitnessV2,
    transitioned_at: DateTime<Utc>,
}

impl RuntimeRoutePredecessorTransitionEvidenceV2 {
    pub fn predecessor(&self) -> Option<&RuntimeRouteWitnessV2> {
        self.predecessor.as_ref()
    }

    pub fn successor(&self) -> &RuntimeRouteWitnessV2 {
        &self.successor
    }

    pub fn transitioned_at(&self) -> DateTime<Utc> {
        self.transitioned_at
    }
}

pub struct RuntimeRoutePredecessorDrainingConvergenceV2<H, S, P> {
    replacement: RuntimeReplacementCoreV2<H, S>,
    previous_serving: RuntimePreviousServingObservationReceiptV1,
    pause: RuntimeBarrierAPauseEvidenceV2,
    paused: P,
    transition: RuntimeRoutePredecessorTransitionEvidenceV2,
}

impl<H, S, P> RuntimeRoutePredecessorDrainingConvergenceV2<H, S, P> {
    pub fn transition_evidence(&self) -> &RuntimeRoutePredecessorTransitionEvidenceV2 {
        &self.transition
    }

    pub fn begin_barrier_a_resume(self) -> RuntimeBarrierAResumeRequestStateV2<H, S, P> {
        let request = RuntimeBarrierAResumeRequestV2 {
            correlation: self.pause.correlation.clone(),
            coordinator_generation: self.pause.coordinator_generation,
            connection_epoch: self.pause.connection_epoch,
            pause_admission_revision: self.pause.admission_revision,
            connected_event_sequence: self.pause.connected_event_sequence,
            pause_sequence: self.pause.pause_sequence,
            transitioned_at: self.transition.transitioned_at,
            deadline: self.pause.deadline,
        };
        RuntimeBarrierAResumeRequestStateV2 {
            draining: self,
            request,
        }
    }
}

impl<H, S, P> Debug for RuntimeRoutePredecessorDrainingConvergenceV2<H, S, P> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRoutePredecessorDrainingConvergenceV2(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeAdmissionDispositionV2 {
    Closed,
    Open,
}

pub struct RuntimeBarrierAResumeRequestV2 {
    correlation: RuntimeBarrierACorrelationV2,
    coordinator_generation: NonZeroU64,
    connection_epoch: NonZeroU64,
    pause_admission_revision: NonZeroU64,
    connected_event_sequence: NonZeroU64,
    pause_sequence: NonZeroU64,
    transitioned_at: DateTime<Utc>,
    deadline: DateTime<Utc>,
}

impl RuntimeBarrierAResumeRequestV2 {
    pub fn correlation(&self) -> &RuntimeBarrierACorrelationV2 {
        &self.correlation
    }

    pub fn coordinator_generation(&self) -> NonZeroU64 {
        self.coordinator_generation
    }

    pub fn connection_epoch(&self) -> NonZeroU64 {
        self.connection_epoch
    }

    pub fn pause_admission_revision(&self) -> NonZeroU64 {
        self.pause_admission_revision
    }

    pub fn connected_event_sequence(&self) -> NonZeroU64 {
        self.connected_event_sequence
    }

    pub fn pause_sequence(&self) -> NonZeroU64 {
        self.pause_sequence
    }

    pub fn transitioned_at(&self) -> DateTime<Utc> {
        self.transitioned_at
    }

    pub fn deadline(&self) -> DateTime<Utc> {
        self.deadline
    }
}

impl Debug for RuntimeBarrierAResumeRequestV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeBarrierAResumeRequestV2(<redacted>)")
    }
}

pub struct RuntimeBarrierAResumeObservationV2<R> {
    pub correlation: RuntimeBarrierACorrelationV2,
    pub coordinator_generation: NonZeroU64,
    pub connection_epoch: NonZeroU64,
    pub admission_revision: NonZeroU64,
    pub connected_event_sequence: NonZeroU64,
    pub pause_sequence: NonZeroU64,
    pub resume_sequence: NonZeroU64,
    pub admission: RuntimeAdmissionDispositionV2,
    pub resumed_at: DateTime<Utc>,
    pub resumed: R,
}

impl<R> Debug for RuntimeBarrierAResumeObservationV2<R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeBarrierAResumeObservationV2(<redacted>)")
    }
}

pub trait RuntimeBarrierAResumePortV2<P> {
    type Error;
    type Resumed;

    fn resume_barrier_a_closed<'a>(
        &'a self,
        request: &'a RuntimeBarrierAResumeRequestV2,
        paused: &'a P,
    ) -> RuntimeConvergenceFutureV2<
        'a,
        Result<RuntimeBarrierAResumeObservationV2<Self::Resumed>, Self::Error>,
    >;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeBarrierAResumeEvidenceErrorV2 {
    #[error("runtime Barrier A resume correlation does not match")]
    CorrelationMismatch,
    #[error("runtime Barrier A resume generation or epoch does not match")]
    GatewayEpochMismatch,
    #[error("runtime Barrier A resume admission revision does not match")]
    AdmissionRevisionMismatch,
    #[error("runtime Barrier A resume gateway sequence does not match")]
    GatewaySequenceMismatch,
    #[error("runtime Barrier A resume opened public admission")]
    PublicAdmissionOpened,
    #[error("runtime Barrier A resume time is outside its operation window")]
    ObservationTimeMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeBarrierAResumeErrorV2 {
    #[error("runtime Barrier A resume slot work authority is not active")]
    SlotWork(RuntimeServingSlotWorkErrorV2),
    #[error("runtime Barrier A resume evidence is invalid")]
    Evidence(RuntimeBarrierAResumeEvidenceErrorV2),
}

pub struct RuntimeBarrierAResumeRequestStateV2<H, S, P> {
    draining: RuntimeRoutePredecessorDrainingConvergenceV2<H, S, P>,
    request: RuntimeBarrierAResumeRequestV2,
}

impl<H, S, P> RuntimeBarrierAResumeRequestStateV2<H, S, P> {
    pub fn request(&self) -> &RuntimeBarrierAResumeRequestV2 {
        &self.request
    }

    pub fn execute<'a, T>(
        self,
        port: &'a T,
    ) -> RuntimeBarrierAResumeFutureV2<'a, H, S, P, T::Resumed, T::Error>
    where
        H: Send + 'a,
        S: Send + 'a,
        P: Send + Sync + 'a,
        T: RuntimeBarrierAResumePortV2<P> + Sync + 'a,
        T::Error: Send + 'a,
        T::Resumed: Send + 'a,
    {
        Box::pin(async move {
            let draining = self.draining;
            draining.replacement.ensure_active().map_err(|error| {
                RuntimeReplacementExecutionErrorV2::Failed(RuntimeBarrierAResumeErrorV2::SlotWork(
                    error,
                ))
            })?;
            let observation = match port
                .resume_barrier_a_closed(&self.request, &draining.paused)
                .await
            {
                Ok(observation) => observation,
                Err(source) => {
                    return Err(RuntimeReplacementExecutionErrorV2::Port {
                        retained: Box::new(draining),
                        source,
                    });
                }
            };
            draining.replacement.ensure_active().map_err(|error| {
                RuntimeReplacementExecutionErrorV2::Failed(RuntimeBarrierAResumeErrorV2::SlotWork(
                    error,
                ))
            })?;
            let (resume, resumed) = validate_resume_observation(&self.request, observation)
                .map_err(|error| {
                    RuntimeReplacementExecutionErrorV2::Failed(
                        RuntimeBarrierAResumeErrorV2::Evidence(error),
                    )
                })?;
            drop(draining.paused);
            Ok(RuntimeBarrierAResumedConvergenceV2 {
                replacement: draining.replacement,
                initial_previous_serving: draining.previous_serving,
                barrier: RuntimeBarrierAEvidenceV2 {
                    pause: draining.pause,
                    transition: draining.transition,
                    resume,
                },
                resumed,
            })
        })
    }
}

impl<H, S, P> Debug for RuntimeBarrierAResumeRequestStateV2<H, S, P> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeBarrierAResumeRequestStateV2(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeBarrierAResumeEvidenceV2 {
    correlation: RuntimeBarrierACorrelationV2,
    coordinator_generation: NonZeroU64,
    connection_epoch: NonZeroU64,
    admission_revision: NonZeroU64,
    connected_event_sequence: NonZeroU64,
    pause_sequence: NonZeroU64,
    resume_sequence: NonZeroU64,
    resumed_at: DateTime<Utc>,
}

impl RuntimeBarrierAResumeEvidenceV2 {
    pub fn correlation(&self) -> &RuntimeBarrierACorrelationV2 {
        &self.correlation
    }

    pub fn coordinator_generation(&self) -> NonZeroU64 {
        self.coordinator_generation
    }

    pub fn connection_epoch(&self) -> NonZeroU64 {
        self.connection_epoch
    }

    pub fn admission_revision(&self) -> NonZeroU64 {
        self.admission_revision
    }

    pub fn connected_event_sequence(&self) -> NonZeroU64 {
        self.connected_event_sequence
    }

    pub fn pause_sequence(&self) -> NonZeroU64 {
        self.pause_sequence
    }

    pub fn resume_sequence(&self) -> NonZeroU64 {
        self.resume_sequence
    }

    pub fn resumed_at(&self) -> DateTime<Utc> {
        self.resumed_at
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeBarrierAEvidenceV2 {
    pause: RuntimeBarrierAPauseEvidenceV2,
    transition: RuntimeRoutePredecessorTransitionEvidenceV2,
    resume: RuntimeBarrierAResumeEvidenceV2,
}

impl RuntimeBarrierAEvidenceV2 {
    pub fn pause(&self) -> &RuntimeBarrierAPauseEvidenceV2 {
        &self.pause
    }

    pub fn transition(&self) -> &RuntimeRoutePredecessorTransitionEvidenceV2 {
        &self.transition
    }

    pub fn resume(&self) -> &RuntimeBarrierAResumeEvidenceV2 {
        &self.resume
    }
}

pub struct RuntimeBarrierAResumedConvergenceV2<H, S, R> {
    replacement: RuntimeReplacementCoreV2<H, S>,
    initial_previous_serving: RuntimePreviousServingObservationReceiptV1,
    barrier: RuntimeBarrierAEvidenceV2,
    resumed: R,
}

impl<H, S, R> RuntimeBarrierAResumedConvergenceV2<H, S, R> {
    pub fn barrier_evidence(&self) -> &RuntimeBarrierAEvidenceV2 {
        &self.barrier
    }

    pub fn resumed(&self) -> &R {
        &self.resumed
    }

    pub fn begin_previous_serving_disconnect(
        mut self,
    ) -> Result<
        RuntimePreviousServingDisconnectOutcomeV2<H, S, R>,
        RuntimePreviousServingDisconnectErrorV2,
    > {
        self.replacement
            .ensure_active()
            .map_err(RuntimePreviousServingDisconnectErrorV2::SlotWork)?;
        if matches!(
            self.initial_previous_serving.state,
            RuntimePreviousServingStateV1::Serving { .. }
        ) {
            let request = self
                .replacement
                .core
                .claimed
                .session
                .begin_previous_serving_disconnect(&self.initial_previous_serving)
                .map_err(RuntimePreviousServingDisconnectErrorV2::Session)?;
            Ok(RuntimePreviousServingDisconnectOutcomeV2::Required(
                RuntimePreviousServingDisconnectV2 {
                    resumed: self,
                    request,
                },
            ))
        } else {
            Ok(RuntimePreviousServingDisconnectOutcomeV2::NotRequired(
                RuntimePredecessorRetirementReadyConvergenceV2 {
                    resumed: self,
                    previous_serving_disconnect: None,
                },
            ))
        }
    }
}

impl<H, S, R> Debug for RuntimeBarrierAResumedConvergenceV2<H, S, R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeBarrierAResumedConvergenceV2(<redacted>)")
    }
}

pub enum RuntimePreviousServingDisconnectOutcomeV2<H, S, R> {
    NotRequired(RuntimePredecessorRetirementReadyConvergenceV2<H, S, R>),
    Required(RuntimePreviousServingDisconnectV2<H, S, R>),
}

impl<H, S, R> Debug for RuntimePreviousServingDisconnectOutcomeV2<H, S, R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePreviousServingDisconnectOutcomeV2(<redacted>)")
    }
}

pub trait RuntimePreviousServingDisconnectPortV2 {
    type Error;

    fn disconnect_previous_serving<'a>(
        &'a self,
        request: &'a RuntimeDisconnectServingV1,
    ) -> RuntimeConvergenceFutureV2<'a, Result<RuntimeServingUpdateReceiptV1, Self::Error>>;
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimePreviousServingDisconnectErrorV2 {
    #[error("runtime previous serving disconnect slot work authority is not active")]
    SlotWork(RuntimeServingSlotWorkErrorV2),
    #[error("runtime previous serving disconnect session failed")]
    Session(RuntimeConvergenceSessionError),
}

pub struct RuntimePreviousServingDisconnectV2<H, S, R> {
    resumed: RuntimeBarrierAResumedConvergenceV2<H, S, R>,
    request: RuntimeDisconnectServingV1,
}

impl<H, S, R> RuntimePreviousServingDisconnectV2<H, S, R> {
    pub fn request(&self) -> &RuntimeDisconnectServingV1 {
        &self.request
    }

    pub fn execute<'a, P>(
        self,
        port: &'a P,
    ) -> RuntimePreviousServingDisconnectFutureV2<'a, H, S, R, P::Error>
    where
        H: Send + 'a,
        S: Send + 'a,
        R: Send + 'a,
        P: RuntimePreviousServingDisconnectPortV2 + Sync + 'a,
        P::Error: Send + 'a,
    {
        Box::pin(async move {
            let mut resumed = self.resumed;
            resumed.replacement.ensure_active().map_err(|error| {
                RuntimeReplacementExecutionErrorV2::Failed(
                    RuntimePreviousServingDisconnectErrorV2::SlotWork(error),
                )
            })?;
            let receipt = match port.disconnect_previous_serving(&self.request).await {
                Ok(receipt) => receipt,
                Err(source) => {
                    if let Err(error) = resumed
                        .replacement
                        .core
                        .claimed
                        .session
                        .abort_action(self.request.action_id)
                    {
                        return Err(RuntimeReplacementExecutionErrorV2::Failed(
                            RuntimePreviousServingDisconnectErrorV2::Session(error),
                        ));
                    }
                    return Err(RuntimeReplacementExecutionErrorV2::Port {
                        retained: Box::new(resumed),
                        source,
                    });
                }
            };
            resumed.replacement.ensure_active().map_err(|error| {
                RuntimeReplacementExecutionErrorV2::Failed(
                    RuntimePreviousServingDisconnectErrorV2::SlotWork(error),
                )
            })?;
            let previous_serving_disconnect = resumed
                .replacement
                .core
                .claimed
                .session
                .apply_previous_serving_disconnect(receipt)
                .map_err(|error| {
                    RuntimeReplacementExecutionErrorV2::Failed(
                        RuntimePreviousServingDisconnectErrorV2::Session(error),
                    )
                })?;
            resumed.replacement.ensure_active().map_err(|error| {
                RuntimeReplacementExecutionErrorV2::Failed(
                    RuntimePreviousServingDisconnectErrorV2::SlotWork(error),
                )
            })?;
            Ok(RuntimePredecessorRetirementReadyConvergenceV2 {
                resumed,
                previous_serving_disconnect: Some(previous_serving_disconnect),
            })
        })
    }
}

impl<H, S, R> Debug for RuntimePreviousServingDisconnectV2<H, S, R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePreviousServingDisconnectV2(<redacted>)")
    }
}

pub struct RuntimePredecessorRetirementReadyConvergenceV2<H, S, R> {
    resumed: RuntimeBarrierAResumedConvergenceV2<H, S, R>,
    previous_serving_disconnect: Option<RuntimeServingReceiptV1>,
}

impl<H, S, R> RuntimePredecessorRetirementReadyConvergenceV2<H, S, R> {
    pub fn previous_serving_disconnect(&self) -> Option<&RuntimeServingReceiptV1> {
        self.previous_serving_disconnect.as_ref()
    }

    pub fn begin_predecessor_retirement(
        mut self,
    ) -> Result<RuntimePredecessorRetirementV2<H, S, R>, RuntimePredecessorRetirementErrorV2> {
        self.resumed
            .replacement
            .ensure_active()
            .map_err(RuntimePredecessorRetirementErrorV2::SlotWork)?;
        let previous_serving_request = self
            .resumed
            .replacement
            .core
            .claimed
            .session
            .begin_previous_serving_observation()
            .map_err(RuntimePredecessorRetirementErrorV2::Session)?;
        let request = RuntimePredecessorRetirementRequestV2 {
            previous_serving_request,
            previous_serving_disconnect: self.previous_serving_disconnect,
            correlation: self.resumed.barrier.pause.correlation.clone(),
            predecessor: self.resumed.barrier.transition.predecessor.clone(),
            successor: self.resumed.barrier.transition.successor.clone(),
            resumed_at: self.resumed.barrier.resume.resumed_at,
            deadline: self.resumed.barrier.pause.deadline,
        };
        Ok(RuntimePredecessorRetirementV2 {
            resumed: self.resumed,
            request,
        })
    }
}

impl<H, S, R> Debug for RuntimePredecessorRetirementReadyConvergenceV2<H, S, R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePredecessorRetirementReadyConvergenceV2(<redacted>)")
    }
}

pub struct RuntimePredecessorRetirementRequestV2 {
    previous_serving_request: RuntimeObservePreviousServingV1,
    previous_serving_disconnect: Option<RuntimeServingReceiptV1>,
    correlation: RuntimeBarrierACorrelationV2,
    predecessor: Option<RuntimeRouteWitnessV2>,
    successor: RuntimeRouteWitnessV2,
    resumed_at: DateTime<Utc>,
    deadline: DateTime<Utc>,
}

impl RuntimePredecessorRetirementRequestV2 {
    pub fn previous_serving_request(&self) -> &RuntimeObservePreviousServingV1 {
        &self.previous_serving_request
    }

    pub fn previous_serving_disconnect(&self) -> Option<&RuntimeServingReceiptV1> {
        self.previous_serving_disconnect.as_ref()
    }

    pub fn correlation(&self) -> &RuntimeBarrierACorrelationV2 {
        &self.correlation
    }

    pub fn predecessor(&self) -> Option<&RuntimeRouteWitnessV2> {
        self.predecessor.as_ref()
    }

    pub fn successor(&self) -> &RuntimeRouteWitnessV2 {
        &self.successor
    }

    pub fn resumed_at(&self) -> DateTime<Utc> {
        self.resumed_at
    }

    pub fn deadline(&self) -> DateTime<Utc> {
        self.deadline
    }
}

impl Debug for RuntimePredecessorRetirementRequestV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePredecessorRetirementRequestV2(<redacted>)")
    }
}

pub struct RuntimeRoutePredecessorRemovalObservationV2 {
    pub removed_predecessor: Option<RuntimeRouteWitnessV2>,
    pub successor: RuntimeRouteWitnessV2,
    pub observed_at: DateTime<Utc>,
}

impl Debug for RuntimeRoutePredecessorRemovalObservationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRoutePredecessorRemovalObservationV2(<redacted>)")
    }
}

pub struct RuntimePredecessorRetirementObservationV2 {
    pub previous_serving: RuntimePreviousServingObservationReceiptV1,
    pub route: RuntimeRoutePredecessorRemovalObservationV2,
}

impl Debug for RuntimePredecessorRetirementObservationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePredecessorRetirementObservationV2(<redacted>)")
    }
}

pub trait RuntimePredecessorRetirementPortV2<H, S, R> {
    type Error;

    fn retire_predecessor<'a>(
        &'a self,
        request: &'a RuntimePredecessorRetirementRequestV2,
        hydrated: &'a H,
        staged: &'a S,
        resumed: &'a R,
    ) -> RuntimeConvergenceFutureV2<
        'a,
        Result<RuntimePredecessorRetirementObservationV2, Self::Error>,
    >;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimePredecessorRetirementEvidenceErrorV2 {
    #[error("runtime predecessor retirement previous serving evidence regressed")]
    PreviousServingRegression,
    #[error("runtime predecessor retirement previous serving lease does not match")]
    PreviousServingLeaseMismatch,
    #[error("runtime predecessor retirement previous serving lease remains fresh")]
    PreviousServingStillFresh,
    #[error("runtime predecessor retirement route evidence time does not match")]
    ObservationTimeMismatch,
    #[error("runtime predecessor retirement removed route does not match")]
    RemovedPredecessorMismatch,
    #[error("runtime predecessor retirement removed route still has active interactions")]
    ActiveInteractionsPresent,
    #[error("runtime predecessor retirement successor does not match")]
    SuccessorMismatch,
    #[error("runtime predecessor retirement successor is not staged")]
    SuccessorLifecycleMismatch,
    #[error("runtime predecessor retirement successor observation regressed")]
    SuccessorObservationRegression,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimePredecessorRetirementErrorV2 {
    #[error("runtime predecessor retirement slot work authority is not active")]
    SlotWork(RuntimeServingSlotWorkErrorV2),
    #[error("runtime predecessor retirement session failed")]
    Session(RuntimeConvergenceSessionError),
    #[error("runtime predecessor retirement evidence is invalid")]
    Evidence(RuntimePredecessorRetirementEvidenceErrorV2),
}

pub struct RuntimePredecessorRetirementV2<H, S, R> {
    resumed: RuntimeBarrierAResumedConvergenceV2<H, S, R>,
    request: RuntimePredecessorRetirementRequestV2,
}

impl<H, S, R> RuntimePredecessorRetirementV2<H, S, R> {
    pub fn request(&self) -> &RuntimePredecessorRetirementRequestV2 {
        &self.request
    }

    pub fn execute<'a, P>(
        self,
        port: &'a P,
    ) -> RuntimePredecessorRetirementFutureV2<'a, H, S, R, P::Error>
    where
        H: Send + Sync + 'a,
        S: Send + Sync + 'a,
        R: Send + Sync + 'a,
        P: RuntimePredecessorRetirementPortV2<H, S, R> + Sync + 'a,
        P::Error: Send + 'a,
    {
        Box::pin(async move {
            let mut resumed = self.resumed;
            resumed.replacement.ensure_active().map_err(|error| {
                RuntimeReplacementExecutionErrorV2::Failed(
                    RuntimePredecessorRetirementErrorV2::SlotWork(error),
                )
            })?;
            let observation = match port
                .retire_predecessor(
                    &self.request,
                    &resumed.replacement.core.hydrated,
                    &resumed.replacement.staged,
                    &resumed.resumed,
                )
                .await
            {
                Ok(observation) => observation,
                Err(source) => {
                    if let Err(error) = resumed
                        .replacement
                        .core
                        .claimed
                        .session
                        .abort_action(self.request.previous_serving_request.action_id)
                    {
                        return Err(RuntimeReplacementExecutionErrorV2::Failed(
                            RuntimePredecessorRetirementErrorV2::Session(error),
                        ));
                    }
                    return Err(RuntimeReplacementExecutionErrorV2::Port {
                        retained: Box::new(RuntimePredecessorRetirementReadyConvergenceV2 {
                            resumed,
                            previous_serving_disconnect: self.request.previous_serving_disconnect,
                        }),
                        source,
                    });
                }
            };
            resumed.replacement.ensure_active().map_err(|error| {
                RuntimeReplacementExecutionErrorV2::Failed(
                    RuntimePredecessorRetirementErrorV2::SlotWork(error),
                )
            })?;
            let previous_serving = resumed
                .replacement
                .core
                .claimed
                .session
                .apply_previous_serving_observation(observation.previous_serving)
                .map_err(|error| {
                    RuntimeReplacementExecutionErrorV2::Failed(
                        RuntimePredecessorRetirementErrorV2::Session(error),
                    )
                })?;
            validate_retirement_observation(
                &self.request,
                &resumed.initial_previous_serving,
                &previous_serving,
                &observation.route,
            )
            .map_err(|error| {
                RuntimeReplacementExecutionErrorV2::Failed(
                    RuntimePredecessorRetirementErrorV2::Evidence(error),
                )
            })?;
            resumed.replacement.ensure_active().map_err(|error| {
                RuntimeReplacementExecutionErrorV2::Failed(
                    RuntimePredecessorRetirementErrorV2::SlotWork(error),
                )
            })?;
            Ok(RuntimePredecessorRemovedConvergenceV2 {
                replacement: resumed.replacement,
                previous_serving,
                previous_serving_disconnect: self.request.previous_serving_disconnect,
                barrier: resumed.barrier,
                resumed: resumed.resumed,
                removal: observation.route,
            })
        })
    }
}

impl<H, S, R> Debug for RuntimePredecessorRetirementV2<H, S, R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePredecessorRetirementV2(<redacted>)")
    }
}

pub struct RuntimePredecessorRemovedConvergenceV2<H, S, R> {
    replacement: RuntimeReplacementCoreV2<H, S>,
    previous_serving: RuntimePreviousServingObservationReceiptV1,
    previous_serving_disconnect: Option<RuntimeServingReceiptV1>,
    barrier: RuntimeBarrierAEvidenceV2,
    resumed: R,
    removal: RuntimeRoutePredecessorRemovalObservationV2,
}

impl<H, S, R> RuntimePredecessorRemovedConvergenceV2<H, S, R> {
    pub fn removal(&self) -> &RuntimeRoutePredecessorRemovalObservationV2 {
        &self.removal
    }

    pub fn previous_serving(&self) -> &RuntimePreviousServingObservationReceiptV1 {
        &self.previous_serving
    }

    pub fn previous_serving_disconnect(&self) -> Option<&RuntimeServingReceiptV1> {
        self.previous_serving_disconnect.as_ref()
    }

    pub fn barrier_evidence(&self) -> &RuntimeBarrierAEvidenceV2 {
        &self.barrier
    }

    pub fn begin_accept_drain(
        mut self,
    ) -> Result<RuntimeAcceptDrainMutationV2<H, S, R>, RuntimeAcceptDrainMutationErrorV2> {
        self.replacement
            .ensure_active()
            .map_err(RuntimeAcceptDrainMutationErrorV2::SlotWork)?;
        let snapshot = self.replacement.core.claimed.session.snapshot();
        let attestation = DrainAttestationV1 {
            previous_runtime: snapshot.previous_runtime.clone(),
            target_runtime_generation: snapshot.runtime_generation,
            drained_at: self.removal.observed_at,
        };
        let request = self
            .replacement
            .core
            .claimed
            .session
            .begin_mutation(RuntimeConvergenceMutationV1::AcceptDrain(attestation))
            .map_err(RuntimeAcceptDrainMutationErrorV2::Session)?;
        Ok(RuntimeAcceptDrainMutationV2 {
            removed: self,
            request,
        })
    }
}

impl<H, S, R> Debug for RuntimePredecessorRemovedConvergenceV2<H, S, R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePredecessorRemovedConvergenceV2(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeAcceptDrainMutationErrorV2 {
    #[error("runtime AcceptDrain slot work authority is not active")]
    SlotWork(RuntimeServingSlotWorkErrorV2),
    #[error("runtime AcceptDrain session failed")]
    Session(RuntimeConvergenceSessionError),
    #[error("runtime AcceptDrain did not retain an active session")]
    InactiveSession,
    #[error("runtime AcceptDrain did not reach Drained")]
    DrainedRequired,
    #[error("runtime AcceptDrain completion time is invalid")]
    InvalidCompletionTime,
    #[error("runtime AcceptDrain successor cannot be planned")]
    Plan(RuntimeControllerPlanError),
    #[error("runtime AcceptDrain requires controller renewal")]
    RenewalRequired,
    #[error("runtime AcceptDrain received an unexpected successor plan")]
    UnexpectedPlan,
}

pub struct RuntimeAcceptDrainMutationV2<H, S, R> {
    removed: RuntimePredecessorRemovedConvergenceV2<H, S, R>,
    request: RuntimeMutationRequestV1,
}

impl<H, S, R> RuntimeAcceptDrainMutationV2<H, S, R> {
    pub fn request(&self) -> &RuntimeMutationRequestV1 {
        &self.request
    }

    pub fn execute<'a, P>(
        self,
        port: &'a P,
        completed_at: DateTime<Utc>,
    ) -> RuntimeAcceptDrainFutureV2<'a, H, S, R, P::Error>
    where
        H: Send + 'a,
        S: Send + 'a,
        R: Send + 'a,
        P: RuntimeConvergenceMutationPortV2 + Sync + 'a,
        P::Error: Send + 'a,
    {
        Box::pin(async move {
            let mut removed = self.removed;
            removed.replacement.ensure_active().map_err(|error| {
                RuntimeReplacementExecutionErrorV2::Failed(
                    RuntimeAcceptDrainMutationErrorV2::SlotWork(error),
                )
            })?;
            let receipt = match port.mutate(&self.request).await {
                Ok(receipt) => receipt,
                Err(source) => {
                    if let Err(error) = removed
                        .replacement
                        .core
                        .claimed
                        .session
                        .abort_action(self.request.action_id)
                    {
                        return Err(RuntimeReplacementExecutionErrorV2::Failed(
                            RuntimeAcceptDrainMutationErrorV2::Session(error),
                        ));
                    }
                    return Err(RuntimeReplacementExecutionErrorV2::Port {
                        retained: Box::new(removed),
                        source,
                    });
                }
            };
            removed.replacement.ensure_active().map_err(|error| {
                RuntimeReplacementExecutionErrorV2::Failed(
                    RuntimeAcceptDrainMutationErrorV2::SlotWork(error),
                )
            })?;
            let state = removed
                .replacement
                .core
                .claimed
                .session
                .apply_mutation(receipt)
                .map_err(|error| {
                    RuntimeReplacementExecutionErrorV2::Failed(
                        RuntimeAcceptDrainMutationErrorV2::Session(error),
                    )
                })?;
            if state != RuntimeConvergenceSessionStateV1::Active {
                return Err(RuntimeReplacementExecutionErrorV2::Failed(
                    RuntimeAcceptDrainMutationErrorV2::InactiveSession,
                ));
            }
            if !matches!(
                removed.replacement.core.claimed.session.snapshot().phase,
                RuntimeDeploymentPhaseV1::Drained
            ) {
                return Err(RuntimeReplacementExecutionErrorV2::Failed(
                    RuntimeAcceptDrainMutationErrorV2::DrainedRequired,
                ));
            }
            if completed_at < removed.removal.observed_at
                || completed_at >= removed.replacement.core.claimed.session.expires_at()
            {
                return Err(RuntimeReplacementExecutionErrorV2::Failed(
                    RuntimeAcceptDrainMutationErrorV2::InvalidCompletionTime,
                ));
            }
            match plan_runtime_action_v1(
                removed.replacement.core.claimed.session.snapshot(),
                removed.replacement.core.claimed.session.controller_id(),
                completed_at,
                &removed.replacement.core.claimed.config,
            )
            .map_err(|error| {
                RuntimeReplacementExecutionErrorV2::Failed(RuntimeAcceptDrainMutationErrorV2::Plan(
                    error,
                ))
            })? {
                RuntimeControllerActionV1::BeginActivation => {}
                RuntimeControllerActionV1::RenewControllerLease { .. } => {
                    return Err(RuntimeReplacementExecutionErrorV2::Failed(
                        RuntimeAcceptDrainMutationErrorV2::RenewalRequired,
                    ));
                }
                _ => {
                    return Err(RuntimeReplacementExecutionErrorV2::Failed(
                        RuntimeAcceptDrainMutationErrorV2::UnexpectedPlan,
                    ));
                }
            }
            removed.replacement.ensure_active().map_err(|error| {
                RuntimeReplacementExecutionErrorV2::Failed(
                    RuntimeAcceptDrainMutationErrorV2::SlotWork(error),
                )
            })?;
            Ok(RuntimeDrainedConvergenceV2 {
                replacement: removed.replacement,
                previous_serving: removed.previous_serving,
                previous_serving_disconnect: removed.previous_serving_disconnect,
                barrier: removed.barrier,
                resumed: removed.resumed,
                removal: removed.removal,
            })
        })
    }
}

impl<H, S, R> Debug for RuntimeAcceptDrainMutationV2<H, S, R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAcceptDrainMutationV2(<redacted>)")
    }
}

pub struct RuntimeDrainedConvergenceV2<H, S, R> {
    replacement: RuntimeReplacementCoreV2<H, S>,
    previous_serving: RuntimePreviousServingObservationReceiptV1,
    previous_serving_disconnect: Option<RuntimeServingReceiptV1>,
    barrier: RuntimeBarrierAEvidenceV2,
    resumed: R,
    removal: RuntimeRoutePredecessorRemovalObservationV2,
}

pub type RuntimeDrainedConvergenceHandoffV2<H, S, R> = (
    S,
    automation_runtime_controller::RuntimeConvergenceSessionV1,
    crate::RuntimeServingSlotWorkPermitV2,
    RuntimeExactTargetEvidenceV2,
    H,
    RuntimeRouteWitnessV2,
    RuntimePreviousServingObservationReceiptV1,
    Option<RuntimeServingReceiptV1>,
    RuntimeBarrierAEvidenceV2,
    R,
    RuntimeRoutePredecessorRemovalObservationV2,
);

impl<H, S, R> RuntimeDrainedConvergenceV2<H, S, R> {
    pub fn staged(&self) -> &S {
        &self.replacement.staged
    }

    pub fn hydrated(&self) -> &H {
        &self.replacement.core.hydrated
    }

    pub fn witness(&self) -> &RuntimeRouteWitnessV2 {
        &self.replacement.witness
    }

    pub fn exact_target_evidence(&self) -> &RuntimeExactTargetEvidenceV2 {
        &self.replacement.core.evidence
    }

    pub fn previous_serving(&self) -> &RuntimePreviousServingObservationReceiptV1 {
        &self.previous_serving
    }

    pub fn previous_serving_disconnect(&self) -> Option<&RuntimeServingReceiptV1> {
        self.previous_serving_disconnect.as_ref()
    }

    pub fn barrier_evidence(&self) -> &RuntimeBarrierAEvidenceV2 {
        &self.barrier
    }

    pub fn resumed(&self) -> &R {
        &self.resumed
    }

    pub fn removal(&self) -> &RuntimeRoutePredecessorRemovalObservationV2 {
        &self.removal
    }

    pub fn into_handoff(self) -> RuntimeDrainedConvergenceHandoffV2<H, S, R> {
        (
            self.replacement.staged,
            self.replacement.core.claimed.session,
            self.replacement.core.claimed.permit,
            self.replacement.core.evidence,
            self.replacement.core.hydrated,
            self.replacement.witness,
            self.previous_serving,
            self.previous_serving_disconnect,
            self.barrier,
            self.resumed,
            self.removal,
        )
    }
}

impl<H, S, R> Debug for RuntimeDrainedConvergenceV2<H, S, R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDrainedConvergenceV2(<redacted>)")
    }
}

impl<H, S> RuntimeStagedConvergenceV2<H, S> {
    pub fn begin_request_drain(
        mut self,
    ) -> Result<RuntimeRequestDrainMutationV2<H, S>, RuntimeRequestDrainMutationErrorV2> {
        self.core
            .claimed
            .ensure_active()
            .map_err(RuntimeRequestDrainMutationErrorV2::SlotWork)?;
        if !matches!(self.witness.lifecycle, RuntimeRouteLifecycleV2::Staged)
            || self.witness.active_interactions != 0
        {
            return Err(RuntimeRequestDrainMutationErrorV2::StagedRouteRequired);
        }
        if !matches!(
            self.core.claimed.session.snapshot().phase,
            RuntimeDeploymentPhaseV1::PreflightReady
        ) {
            return Err(RuntimeRequestDrainMutationErrorV2::PreflightReadyRequired);
        }
        let request = self
            .core
            .claimed
            .session
            .begin_mutation(RuntimeConvergenceMutationV1::RequestDrain)
            .map_err(RuntimeRequestDrainMutationErrorV2::Session)?;
        Ok(RuntimeRequestDrainMutationV2 {
            replacement: RuntimeReplacementCoreV2 {
                staged: self.staged,
                witness: self.witness,
                core: self.core,
            },
            request,
        })
    }

    pub fn resume_drain_requested(
        self,
        resumed_at: DateTime<Utc>,
    ) -> Result<RuntimeDrainRequestedConvergenceV2<H, S>, RuntimeRequestDrainMutationErrorV2> {
        self.core
            .claimed
            .ensure_active()
            .map_err(RuntimeRequestDrainMutationErrorV2::SlotWork)?;
        if !matches!(self.witness.lifecycle, RuntimeRouteLifecycleV2::Staged)
            || self.witness.active_interactions != 0
        {
            return Err(RuntimeRequestDrainMutationErrorV2::StagedRouteRequired);
        }
        if !matches!(
            self.core.claimed.session.snapshot().phase,
            RuntimeDeploymentPhaseV1::DrainRequested
        ) {
            return Err(RuntimeRequestDrainMutationErrorV2::DrainRequestedRequired);
        }
        if resumed_at < self.core.evidence.observed_database_now()
            || resumed_at >= self.core.claimed.session.expires_at()
        {
            return Err(RuntimeRequestDrainMutationErrorV2::InvalidCompletionTime);
        }
        match plan_runtime_action_v1(
            self.core.claimed.session.snapshot(),
            self.core.claimed.session.controller_id(),
            resumed_at,
            &self.core.claimed.config,
        )
        .map_err(RuntimeRequestDrainMutationErrorV2::Plan)?
        {
            RuntimeControllerActionV1::DrainPreviousRuntime { .. } => {}
            RuntimeControllerActionV1::RenewControllerLease { .. } => {
                return Err(RuntimeRequestDrainMutationErrorV2::RenewalRequired);
            }
            _ => return Err(RuntimeRequestDrainMutationErrorV2::UnexpectedPlan),
        }
        self.core
            .claimed
            .ensure_active()
            .map_err(RuntimeRequestDrainMutationErrorV2::SlotWork)?;
        Ok(RuntimeDrainRequestedConvergenceV2 {
            replacement: RuntimeReplacementCoreV2 {
                staged: self.staged,
                witness: self.witness,
                core: self.core,
            },
            requested_at: resumed_at,
        })
    }
}

fn validate_pause_observation<P>(
    request: &RuntimeBarrierAPauseRequestV2,
    observation: RuntimeBarrierAPauseObservationV2<P>,
) -> Result<(RuntimeBarrierAPauseEvidenceV2, P), RuntimeBarrierAPauseEvidenceErrorV2> {
    if observation.correlation != request.correlation {
        return Err(RuntimeBarrierAPauseEvidenceErrorV2::CorrelationMismatch);
    }
    if observation.paused_at < request.started_at || observation.paused_at > request.deadline {
        return Err(RuntimeBarrierAPauseEvidenceErrorV2::ObservationTimeMismatch);
    }
    if observation.pause_sequence <= observation.connected_event_sequence {
        return Err(RuntimeBarrierAPauseEvidenceErrorV2::GatewaySequenceMismatch);
    }
    Ok((
        RuntimeBarrierAPauseEvidenceV2 {
            correlation: observation.correlation,
            coordinator_generation: observation.coordinator_generation,
            connection_epoch: observation.connection_epoch,
            admission_revision: observation.admission_revision,
            connected_event_sequence: observation.connected_event_sequence,
            pause_sequence: observation.pause_sequence,
            paused_at: observation.paused_at,
            deadline: request.deadline,
        },
        observation.paused,
    ))
}

fn validate_transition_observation(
    request: &RuntimeRoutePredecessorTransitionRequestV2,
    observation: RuntimeRoutePredecessorTransitionObservationV2,
) -> Result<
    RuntimeRoutePredecessorTransitionEvidenceV2,
    RuntimeRoutePredecessorTransitionEvidenceErrorV2,
> {
    if observation.correlation != request.correlation {
        return Err(RuntimeRoutePredecessorTransitionEvidenceErrorV2::CorrelationMismatch);
    }
    if observation.transitioned_at < request.paused_at
        || observation.transitioned_at > request.deadline
    {
        return Err(RuntimeRoutePredecessorTransitionEvidenceErrorV2::ObservationTimeMismatch);
    }
    validate_successor_transition(&request.initial_successor, &observation.successor)?;
    let local_previous = request
        .expected_previous_runtime
        .as_ref()
        .is_some_and(|previous| {
            previous.process_instance_id
                == request.correlation.successor_identity.process_instance_id
        });
    match (
        local_previous,
        request.expected_previous_runtime.as_ref(),
        observation.predecessor.as_ref(),
    ) {
        (true, Some(expected), Some(predecessor)) => {
            if predecessor.identity != *expected {
                return Err(
                    RuntimeRoutePredecessorTransitionEvidenceErrorV2::PredecessorIdentityMismatch,
                );
            }
            if !matches!(predecessor.lifecycle, RuntimeRouteLifecycleV2::Draining) {
                return Err(
                    RuntimeRoutePredecessorTransitionEvidenceErrorV2::PredecessorLifecycleMismatch,
                );
            }
            if predecessor.route_incarnation == observation.successor.route_incarnation {
                return Err(
                    RuntimeRoutePredecessorTransitionEvidenceErrorV2::PredecessorIdentityMismatch,
                );
            }
        }
        (true, _, None) => {
            return Err(RuntimeRoutePredecessorTransitionEvidenceErrorV2::LocalPredecessorMissing);
        }
        (false, _, Some(_)) => {
            return Err(
                RuntimeRoutePredecessorTransitionEvidenceErrorV2::UnexpectedLocalPredecessor,
            );
        }
        (false, _, None) => {}
        (true, None, Some(_)) => {
            return Err(
                RuntimeRoutePredecessorTransitionEvidenceErrorV2::PredecessorIdentityMismatch,
            );
        }
    }
    if local_previous
        && (observation.successor.admission_generation
            <= request.initial_successor.admission_generation
            || observation.successor.registry_observation_sequence
                <= request.initial_successor.registry_observation_sequence)
    {
        return Err(
            RuntimeRoutePredecessorTransitionEvidenceErrorV2::SuccessorObservationRegression,
        );
    }
    Ok(RuntimeRoutePredecessorTransitionEvidenceV2 {
        predecessor: observation.predecessor,
        successor: observation.successor,
        transitioned_at: observation.transitioned_at,
    })
}

fn validate_successor_transition(
    initial: &RuntimeRouteWitnessV2,
    successor: &RuntimeRouteWitnessV2,
) -> Result<(), RuntimeRoutePredecessorTransitionEvidenceErrorV2> {
    if !same_exact_route(initial, successor) {
        return Err(RuntimeRoutePredecessorTransitionEvidenceErrorV2::SuccessorMismatch);
    }
    if !matches!(successor.lifecycle, RuntimeRouteLifecycleV2::Staged) {
        return Err(RuntimeRoutePredecessorTransitionEvidenceErrorV2::SuccessorLifecycleMismatch);
    }
    if successor.active_interactions != 0 {
        return Err(RuntimeRoutePredecessorTransitionEvidenceErrorV2::SuccessorActiveInteractions);
    }
    if successor.admission_generation < initial.admission_generation
        || successor.registry_observation_sequence < initial.registry_observation_sequence
    {
        return Err(
            RuntimeRoutePredecessorTransitionEvidenceErrorV2::SuccessorObservationRegression,
        );
    }
    Ok(())
}

fn validate_resume_observation<R>(
    request: &RuntimeBarrierAResumeRequestV2,
    observation: RuntimeBarrierAResumeObservationV2<R>,
) -> Result<(RuntimeBarrierAResumeEvidenceV2, R), RuntimeBarrierAResumeEvidenceErrorV2> {
    if observation.correlation != request.correlation {
        return Err(RuntimeBarrierAResumeEvidenceErrorV2::CorrelationMismatch);
    }
    if observation.coordinator_generation != request.coordinator_generation
        || observation.connection_epoch != request.connection_epoch
    {
        return Err(RuntimeBarrierAResumeEvidenceErrorV2::GatewayEpochMismatch);
    }
    if observation.admission_revision != request.pause_admission_revision {
        return Err(RuntimeBarrierAResumeEvidenceErrorV2::AdmissionRevisionMismatch);
    }
    if observation.connected_event_sequence != request.connected_event_sequence
        || observation.pause_sequence != request.pause_sequence
        || observation.pause_sequence <= observation.connected_event_sequence
        || observation.resume_sequence <= observation.pause_sequence
    {
        return Err(RuntimeBarrierAResumeEvidenceErrorV2::GatewaySequenceMismatch);
    }
    if observation.admission != RuntimeAdmissionDispositionV2::Closed {
        return Err(RuntimeBarrierAResumeEvidenceErrorV2::PublicAdmissionOpened);
    }
    if observation.resumed_at < request.transitioned_at || observation.resumed_at > request.deadline
    {
        return Err(RuntimeBarrierAResumeEvidenceErrorV2::ObservationTimeMismatch);
    }
    Ok((
        RuntimeBarrierAResumeEvidenceV2 {
            correlation: observation.correlation,
            coordinator_generation: observation.coordinator_generation,
            connection_epoch: observation.connection_epoch,
            admission_revision: observation.admission_revision,
            connected_event_sequence: observation.connected_event_sequence,
            pause_sequence: observation.pause_sequence,
            resume_sequence: observation.resume_sequence,
            resumed_at: observation.resumed_at,
        },
        observation.resumed,
    ))
}

fn validate_retirement_observation(
    request: &RuntimePredecessorRetirementRequestV2,
    initial: &RuntimePreviousServingObservationReceiptV1,
    final_observation: &RuntimePreviousServingObservationReceiptV1,
    route: &RuntimeRoutePredecessorRemovalObservationV2,
) -> Result<(), RuntimePredecessorRetirementEvidenceErrorV2> {
    if final_observation.observed_at < initial.observed_at
        || final_observation.observed_at < request.resumed_at
    {
        return Err(RuntimePredecessorRetirementEvidenceErrorV2::PreviousServingRegression);
    }
    if let Some(disconnect) = request.previous_serving_disconnect.as_ref() {
        validate_disconnected_previous_serving(
            request.correlation.expected_previous_runtime.as_ref(),
            disconnect,
            &final_observation.state,
        )?;
    } else {
        validate_previous_serving_advance(&initial.state, &final_observation.state)?;
    }
    if route.observed_at < final_observation.observed_at
        || route.observed_at < request.resumed_at
        || route.observed_at > request.deadline
    {
        return Err(RuntimePredecessorRetirementEvidenceErrorV2::ObservationTimeMismatch);
    }
    validate_removed_predecessor(
        request.predecessor.as_ref(),
        route.removed_predecessor.as_ref(),
    )?;
    if !same_exact_route(&request.successor, &route.successor) {
        return Err(RuntimePredecessorRetirementEvidenceErrorV2::SuccessorMismatch);
    }
    if !matches!(route.successor.lifecycle, RuntimeRouteLifecycleV2::Staged) {
        return Err(RuntimePredecessorRetirementEvidenceErrorV2::SuccessorLifecycleMismatch);
    }
    if route.successor.active_interactions != 0 {
        return Err(RuntimePredecessorRetirementEvidenceErrorV2::ActiveInteractionsPresent);
    }
    if route.successor.admission_generation < request.successor.admission_generation
        || route.successor.registry_observation_sequence
            < request.successor.registry_observation_sequence
    {
        return Err(RuntimePredecessorRetirementEvidenceErrorV2::SuccessorObservationRegression);
    }
    if request.predecessor.is_some()
        && (route.successor.admission_generation <= request.successor.admission_generation
            || route.successor.registry_observation_sequence
                <= request.successor.registry_observation_sequence)
    {
        return Err(RuntimePredecessorRetirementEvidenceErrorV2::SuccessorObservationRegression);
    }
    Ok(())
}

fn validate_previous_serving_advance(
    initial: &RuntimePreviousServingStateV1,
    final_observation: &RuntimePreviousServingStateV1,
) -> Result<(), RuntimePredecessorRetirementEvidenceErrorV2> {
    match (initial, final_observation) {
        (RuntimePreviousServingStateV1::Absent, RuntimePreviousServingStateV1::Absent) => Ok(()),
        (
            RuntimePreviousServingStateV1::Disconnected { lease: initial, .. },
            RuntimePreviousServingStateV1::Disconnected {
                lease: final_lease, ..
            },
        )
        | (
            RuntimePreviousServingStateV1::Expired { lease: initial, .. },
            RuntimePreviousServingStateV1::Expired {
                lease: final_lease, ..
            },
        ) => validate_previous_lease_identity(initial, final_lease),
        (_, RuntimePreviousServingStateV1::Serving { .. }) => {
            Err(RuntimePredecessorRetirementEvidenceErrorV2::PreviousServingStillFresh)
        }
        _ => Err(RuntimePredecessorRetirementEvidenceErrorV2::PreviousServingLeaseMismatch),
    }
}

fn validate_disconnected_previous_serving(
    expected_previous_runtime: Option<&RuntimeProcessIdentityV1>,
    disconnect: &RuntimeServingReceiptV1,
    final_observation: &RuntimePreviousServingStateV1,
) -> Result<(), RuntimePredecessorRetirementEvidenceErrorV2> {
    let RuntimePreviousServingStateV1::Disconnected {
        lease,
        disconnected_at,
    } = final_observation
    else {
        return if matches!(
            final_observation,
            RuntimePreviousServingStateV1::Serving { .. }
        ) {
            Err(RuntimePredecessorRetirementEvidenceErrorV2::PreviousServingStillFresh)
        } else {
            Err(RuntimePredecessorRetirementEvidenceErrorV2::PreviousServingLeaseMismatch)
        };
    };
    if expected_previous_runtime != Some(&lease.identity.process)
        || disconnect.connected
        || disconnect.serving
        || disconnect.identity.scope != lease.identity.scope
        || disconnect.identity.attestation_id != lease.identity.attestation_id
        || disconnect.identity.process_instance_id != lease.identity.process.process_instance_id
        || disconnect.identity.runtime_generation != lease.identity.process.runtime_generation
        || disconnect.identity.lease_epoch != lease.identity.lease_epoch
        || disconnect.identity.expected_revision != lease.identity.revision
        || disconnect.runtime_generation != lease.identity.process.runtime_generation
        || disconnect.acquired_at != lease.acquired_at
        || disconnect.last_heartbeat_at != lease.last_heartbeat_at
        || disconnect.expires_at != lease.last_heartbeat_at
        || *disconnected_at != lease.last_heartbeat_at
    {
        return Err(RuntimePredecessorRetirementEvidenceErrorV2::PreviousServingLeaseMismatch);
    }
    Ok(())
}

fn validate_previous_lease_identity(
    initial: &RuntimePreviousServingLeaseEvidenceV1,
    final_lease: &RuntimePreviousServingLeaseEvidenceV1,
) -> Result<(), RuntimePredecessorRetirementEvidenceErrorV2> {
    if initial.identity != final_lease.identity
        || final_lease.acquired_at != initial.acquired_at
        || final_lease.last_heartbeat_at < initial.last_heartbeat_at
    {
        Err(RuntimePredecessorRetirementEvidenceErrorV2::PreviousServingLeaseMismatch)
    } else {
        Ok(())
    }
}

fn validate_removed_predecessor(
    expected: Option<&RuntimeRouteWitnessV2>,
    observed: Option<&RuntimeRouteWitnessV2>,
) -> Result<(), RuntimePredecessorRetirementEvidenceErrorV2> {
    match (expected, observed) {
        (None, None) => Ok(()),
        (Some(expected), Some(observed)) => {
            if !same_exact_route(expected, observed)
                || !matches!(observed.lifecycle, RuntimeRouteLifecycleV2::Draining)
            {
                return Err(
                    RuntimePredecessorRetirementEvidenceErrorV2::RemovedPredecessorMismatch,
                );
            }
            if observed.active_interactions != 0 {
                return Err(RuntimePredecessorRetirementEvidenceErrorV2::ActiveInteractionsPresent);
            }
            if observed.admission_generation < expected.admission_generation
                || observed.registry_observation_sequence < expected.registry_observation_sequence
            {
                return Err(
                    RuntimePredecessorRetirementEvidenceErrorV2::RemovedPredecessorMismatch,
                );
            }
            Ok(())
        }
        _ => Err(RuntimePredecessorRetirementEvidenceErrorV2::RemovedPredecessorMismatch),
    }
}

fn same_exact_route(left: &RuntimeRouteWitnessV2, right: &RuntimeRouteWitnessV2) -> bool {
    left.identity == right.identity
        && left.controller_fencing_token == right.controller_fencing_token
        && left.route_incarnation == right.route_incarnation
}
