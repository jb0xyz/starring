use std::fmt::{Debug, Formatter};

use automation_runtime_controller::{
    plan_runtime_action_v1, RuntimeBindingPinV1, RuntimeControllerActionV1,
    RuntimeControllerPlanError, RuntimeConvergenceMutationV1, RuntimeConvergenceSessionError,
    RuntimeConvergenceSessionStateV1, RuntimeMutationReceiptV1, RuntimeMutationRequestV1,
};
use automation_runtime_convergence::{
    PreflightAttestationV1, RuntimeDeploymentPhaseV1, RuntimeDeploymentTargetV1, RuntimeGeneration,
    RuntimeProcessIdentityV1,
};
use chrono::{DateTime, TimeDelta, Utc};

use super::hydration::RuntimeHydratedCoreV2;
use super::{
    RuntimeConvergenceClaimKindV2, RuntimeConvergenceFutureV2, RuntimeHydratedConvergenceV2,
    RuntimeStageReadyConvergenceV2,
};
use crate::RuntimeServingSlotWorkErrorV2;

pub struct RuntimeDiscordPreflightRequestV2 {
    target: RuntimeDeploymentTargetV1,
    runtime_generation: RuntimeGeneration,
    durable_previous_runtime: Option<RuntimeProcessIdentityV1>,
    process_identity: RuntimeProcessIdentityV1,
    binding_pin: RuntimeBindingPinV1,
    started_at: DateTime<Utc>,
    deadline: DateTime<Utc>,
}

impl RuntimeDiscordPreflightRequestV2 {
    pub fn target(&self) -> &RuntimeDeploymentTargetV1 {
        &self.target
    }

    pub fn runtime_generation(&self) -> RuntimeGeneration {
        self.runtime_generation
    }

    pub fn process_identity(&self) -> &RuntimeProcessIdentityV1 {
        &self.process_identity
    }

    pub fn binding_pin(&self) -> &RuntimeBindingPinV1 {
        &self.binding_pin
    }

    pub fn started_at(&self) -> DateTime<Utc> {
        self.started_at
    }

    pub fn deadline(&self) -> DateTime<Utc> {
        self.deadline
    }
}

impl Debug for RuntimeDiscordPreflightRequestV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDiscordPreflightRequestV2(<redacted>)")
    }
}

pub struct RuntimeDiscordPreflightObservationV2 {
    pub target: RuntimeDeploymentTargetV1,
    pub runtime_generation: RuntimeGeneration,
    pub binding_pin: RuntimeBindingPinV1,
    pub checked_at: DateTime<Utc>,
}

impl Debug for RuntimeDiscordPreflightObservationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDiscordPreflightObservationV2(<redacted>)")
    }
}

pub trait RuntimeDiscordPreflightPortV2<H> {
    type Error;

    fn verify_preflight<'a>(
        &'a self,
        request: &'a RuntimeDiscordPreflightRequestV2,
        hydrated: &'a H,
    ) -> RuntimeConvergenceFutureV2<'a, Result<RuntimeDiscordPreflightObservationV2, Self::Error>>;
}

pub trait RuntimeConvergenceMutationPortV2 {
    type Error;

    fn mutate<'a>(
        &'a self,
        request: &'a RuntimeMutationRequestV1,
    ) -> RuntimeConvergenceFutureV2<'a, Result<RuntimeMutationReceiptV1, Self::Error>>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeDiscordPreflightEvidenceErrorV2 {
    #[error("runtime Discord preflight target does not match")]
    TargetMismatch,
    #[error("runtime Discord preflight generation does not match")]
    RuntimeGenerationMismatch,
    #[error("runtime Discord preflight binding pin does not match")]
    BindingPinMismatch,
    #[error("runtime Discord preflight observation time is invalid")]
    ObservationTimeMismatch,
    #[error("runtime Discord preflight durable attestation does not match")]
    DurablePreflightMismatch,
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeDiscordPreflightErrorV2<E> {
    #[error("runtime Discord preflight slot work authority is not active")]
    SlotWork(RuntimeServingSlotWorkErrorV2),
    #[error("runtime Discord preflight time window is invalid")]
    InvalidTimeWindow,
    #[error("runtime Discord preflight port failed")]
    Port(E),
    #[error("runtime Discord preflight evidence is invalid")]
    Evidence(RuntimeDiscordPreflightEvidenceErrorV2),
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeAcceptPreflightMutationErrorV2<E> {
    #[error("runtime preflight mutation slot work authority is not active")]
    SlotWork(RuntimeServingSlotWorkErrorV2),
    #[error("runtime preflight mutation could not be prepared or applied")]
    Session(RuntimeConvergenceSessionError),
    #[error("runtime preflight mutation port failed")]
    Port(E),
    #[error("runtime preflight mutation did not retain an active session")]
    InactiveSession,
    #[error("runtime preflight mutation did not reach PreflightReady")]
    PreflightReadyRequired,
    #[error("runtime preflight mutation requires a Requested claim")]
    RequestedClaimRequired,
    #[error("runtime preflight mutation completion time is invalid")]
    InvalidCompletionTime,
    #[error("runtime preflight successor cannot be planned")]
    Plan(RuntimeControllerPlanError),
    #[error("runtime preflight successor requires controller renewal")]
    RenewalRequired,
    #[error("runtime preflight successor received an unexpected plan")]
    UnexpectedPlan,
}

pub struct RuntimeDiscordPreflightV2<H> {
    core: RuntimeHydratedCoreV2<H>,
    request: RuntimeDiscordPreflightRequestV2,
}

impl<H> RuntimeDiscordPreflightV2<H> {
    pub fn request(&self) -> &RuntimeDiscordPreflightRequestV2 {
        &self.request
    }

    pub fn execute<'a, P>(
        self,
        port: &'a P,
    ) -> RuntimeConvergenceFutureV2<
        'a,
        Result<RuntimeDiscordPreflightOutcomeV2<H>, RuntimeDiscordPreflightErrorV2<P::Error>>,
    >
    where
        H: Send + Sync + 'a,
        P: RuntimeDiscordPreflightPortV2<H> + Sync + 'a,
        P::Error: Send + 'a,
    {
        Box::pin(async move {
            self.core
                .claimed
                .ensure_active()
                .map_err(RuntimeDiscordPreflightErrorV2::SlotWork)?;
            let observation = port
                .verify_preflight(&self.request, &self.core.hydrated)
                .await
                .map_err(RuntimeDiscordPreflightErrorV2::Port)?;
            self.core
                .claimed
                .ensure_active()
                .map_err(RuntimeDiscordPreflightErrorV2::SlotWork)?;
            let attestation = validate_observation(&self.request, &observation)
                .map_err(RuntimeDiscordPreflightErrorV2::Evidence)?;
            self.core
                .claimed
                .ensure_active()
                .map_err(RuntimeDiscordPreflightErrorV2::SlotWork)?;
            match self.core.claimed.claim_kind {
                RuntimeConvergenceClaimKindV2::Requested => {
                    Ok(RuntimeDiscordPreflightOutcomeV2::AcceptPreflight(Box::new(
                        RuntimePreflightedConvergenceV2 {
                            core: self.core,
                            attestation,
                        },
                    )))
                }
                RuntimeConvergenceClaimKindV2::PreflightReady
                | RuntimeConvergenceClaimKindV2::DrainRequested
                | RuntimeConvergenceClaimKindV2::StagedRecovery(_) => {
                    validate_durable_preflight(&self.core, &observation)
                        .map_err(RuntimeDiscordPreflightErrorV2::Evidence)?;
                    Ok(RuntimeDiscordPreflightOutcomeV2::StageReady(Box::new(
                        RuntimeStageReadyConvergenceV2::from_preflight(
                            self.core,
                            observation.checked_at,
                        ),
                    )))
                }
            }
        })
    }
}

impl<H> Debug for RuntimeDiscordPreflightV2<H> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDiscordPreflightV2(<redacted>)")
    }
}

pub enum RuntimeDiscordPreflightOutcomeV2<H> {
    AcceptPreflight(Box<RuntimePreflightedConvergenceV2<H>>),
    StageReady(Box<RuntimeStageReadyConvergenceV2<H>>),
}

impl<H> Debug for RuntimeDiscordPreflightOutcomeV2<H> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDiscordPreflightOutcomeV2(<redacted>)")
    }
}

pub struct RuntimePreflightedConvergenceV2<H> {
    core: RuntimeHydratedCoreV2<H>,
    attestation: PreflightAttestationV1,
}

impl<H> RuntimePreflightedConvergenceV2<H> {
    pub fn attestation(&self) -> &PreflightAttestationV1 {
        &self.attestation
    }

    pub fn begin_accept_preflight(
        mut self,
    ) -> Result<RuntimeAcceptPreflightMutationV2<H>, RuntimeAcceptPreflightMutationErrorV2<()>>
    {
        self.core
            .claimed
            .ensure_active()
            .map_err(RuntimeAcceptPreflightMutationErrorV2::SlotWork)?;
        if self.core.claimed.claim_kind != RuntimeConvergenceClaimKindV2::Requested {
            return Err(RuntimeAcceptPreflightMutationErrorV2::RequestedClaimRequired);
        }
        let request = self
            .core
            .claimed
            .session
            .begin_mutation(RuntimeConvergenceMutationV1::AcceptPreflight(
                self.attestation.clone(),
            ))
            .map_err(RuntimeAcceptPreflightMutationErrorV2::Session)?;
        self.core
            .claimed
            .ensure_active()
            .map_err(RuntimeAcceptPreflightMutationErrorV2::SlotWork)?;
        Ok(RuntimeAcceptPreflightMutationV2 {
            core: self.core,
            request,
            checked_at: self.attestation.checked_at,
        })
    }
}

impl<H> Debug for RuntimePreflightedConvergenceV2<H> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePreflightedConvergenceV2(<redacted>)")
    }
}

pub struct RuntimeAcceptPreflightMutationV2<H> {
    core: RuntimeHydratedCoreV2<H>,
    request: RuntimeMutationRequestV1,
    checked_at: DateTime<Utc>,
}

impl<H> RuntimeAcceptPreflightMutationV2<H> {
    pub fn request(&self) -> &RuntimeMutationRequestV1 {
        &self.request
    }

    pub fn execute<'a, P>(
        self,
        port: &'a P,
        completed_at: DateTime<Utc>,
    ) -> RuntimeConvergenceFutureV2<
        'a,
        Result<RuntimeStageReadyConvergenceV2<H>, RuntimeAcceptPreflightMutationErrorV2<P::Error>>,
    >
    where
        H: Send + 'a,
        P: RuntimeConvergenceMutationPortV2 + Sync + 'a,
        P::Error: Send + 'a,
    {
        Box::pin(async move {
            let mut core = self.core;
            core.claimed
                .ensure_active()
                .map_err(RuntimeAcceptPreflightMutationErrorV2::SlotWork)?;
            let receipt = port
                .mutate(&self.request)
                .await
                .map_err(RuntimeAcceptPreflightMutationErrorV2::Port)?;
            core.claimed
                .ensure_active()
                .map_err(RuntimeAcceptPreflightMutationErrorV2::SlotWork)?;
            let state = core
                .claimed
                .session
                .apply_mutation(receipt)
                .map_err(RuntimeAcceptPreflightMutationErrorV2::Session)?;
            if state != RuntimeConvergenceSessionStateV1::Active {
                return Err(RuntimeAcceptPreflightMutationErrorV2::InactiveSession);
            }
            if !matches!(
                core.claimed.session.snapshot().phase,
                RuntimeDeploymentPhaseV1::PreflightReady
            ) {
                return Err(RuntimeAcceptPreflightMutationErrorV2::PreflightReadyRequired);
            }
            core.claimed
                .ensure_active()
                .map_err(RuntimeAcceptPreflightMutationErrorV2::SlotWork)?;
            if completed_at < self.checked_at || completed_at >= core.claimed.session.expires_at() {
                return Err(RuntimeAcceptPreflightMutationErrorV2::InvalidCompletionTime);
            }
            let action = plan_runtime_action_v1(
                core.claimed.session.snapshot(),
                core.claimed.session.controller_id(),
                completed_at,
                &core.claimed.config,
            )
            .map_err(RuntimeAcceptPreflightMutationErrorV2::Plan)?;
            match action {
                RuntimeControllerActionV1::RequestDrain => {}
                RuntimeControllerActionV1::RenewControllerLease { .. } => {
                    return Err(RuntimeAcceptPreflightMutationErrorV2::RenewalRequired);
                }
                _ => return Err(RuntimeAcceptPreflightMutationErrorV2::UnexpectedPlan),
            }
            core.claimed
                .ensure_active()
                .map_err(RuntimeAcceptPreflightMutationErrorV2::SlotWork)?;
            core.claimed.claim_kind = RuntimeConvergenceClaimKindV2::PreflightReady;
            Ok(RuntimeStageReadyConvergenceV2::from_preflight(
                core,
                completed_at,
            ))
        })
    }
}

impl<H> Debug for RuntimeAcceptPreflightMutationV2<H> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAcceptPreflightMutationV2(<redacted>)")
    }
}

impl<H> RuntimeHydratedConvergenceV2<H> {
    pub fn begin_discord_preflight(
        self,
        started_at: DateTime<Utc>,
    ) -> Result<RuntimeDiscordPreflightV2<H>, RuntimeDiscordPreflightErrorV2<()>> {
        self.core
            .claimed
            .ensure_active()
            .map_err(RuntimeDiscordPreflightErrorV2::SlotWork)?;
        let timeout = TimeDelta::from_std(self.core.claimed.preflight_timeout)
            .map_err(|_| RuntimeDiscordPreflightErrorV2::InvalidTimeWindow)?;
        let deadline = started_at
            .checked_add_signed(timeout)
            .ok_or(RuntimeDiscordPreflightErrorV2::InvalidTimeWindow)?;
        if started_at < self.core.evidence.observed_database_now()
            || started_at < self.core.claimed.session.snapshot().requested_at
            || started_at >= self.core.claimed.session.expires_at()
            || deadline > self.core.claimed.session.expires_at()
        {
            return Err(RuntimeDiscordPreflightErrorV2::InvalidTimeWindow);
        }
        let snapshot = self.core.claimed.session.snapshot();
        let request = RuntimeDiscordPreflightRequestV2 {
            target: snapshot.target.clone(),
            runtime_generation: snapshot.runtime_generation,
            durable_previous_runtime: snapshot.previous_runtime.clone(),
            process_identity: self.core.claimed.process_identity.clone(),
            binding_pin: self.core.evidence.binding_pin().clone(),
            started_at,
            deadline,
        };
        Ok(RuntimeDiscordPreflightV2 {
            core: self.core,
            request,
        })
    }
}

fn validate_observation(
    request: &RuntimeDiscordPreflightRequestV2,
    observation: &RuntimeDiscordPreflightObservationV2,
) -> Result<PreflightAttestationV1, RuntimeDiscordPreflightEvidenceErrorV2> {
    if observation.target != request.target {
        return Err(RuntimeDiscordPreflightEvidenceErrorV2::TargetMismatch);
    }
    if observation.runtime_generation != request.runtime_generation {
        return Err(RuntimeDiscordPreflightEvidenceErrorV2::RuntimeGenerationMismatch);
    }
    if observation.binding_pin != request.binding_pin {
        return Err(RuntimeDiscordPreflightEvidenceErrorV2::BindingPinMismatch);
    }
    if observation.checked_at < request.started_at || observation.checked_at > request.deadline {
        return Err(RuntimeDiscordPreflightEvidenceErrorV2::ObservationTimeMismatch);
    }
    Ok(PreflightAttestationV1 {
        target: observation.target.clone(),
        runtime_generation: observation.runtime_generation,
        observed_runtime: request.durable_previous_runtime.clone(),
        checked_at: observation.checked_at,
    })
}

fn validate_durable_preflight<H>(
    core: &RuntimeHydratedCoreV2<H>,
    observation: &RuntimeDiscordPreflightObservationV2,
) -> Result<(), RuntimeDiscordPreflightEvidenceErrorV2> {
    let snapshot = core.claimed.session.snapshot();
    let Some(durable) = snapshot.preflight.as_ref() else {
        return Err(RuntimeDiscordPreflightEvidenceErrorV2::DurablePreflightMismatch);
    };
    if durable.target != snapshot.target
        || durable.runtime_generation != snapshot.runtime_generation
        || durable.observed_runtime != snapshot.previous_runtime
        || durable.checked_at > observation.checked_at
    {
        return Err(RuntimeDiscordPreflightEvidenceErrorV2::DurablePreflightMismatch);
    }
    Ok(())
}
