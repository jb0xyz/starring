use std::fmt::{Debug, Formatter};
use std::num::NonZeroU64;

use automation_runtime_controller::{
    runtime_desired_target_digest_v1, RuntimeBindingPinV1, RuntimeDeploymentScopeV1,
    RuntimeDesiredTargetDigestV1, RuntimeExecutionReceiptV1,
};
use automation_runtime_convergence::{RuntimeDeploymentTargetV1, RuntimeProcessIdentityV1};
use chrono::{DateTime, Utc};

use super::{
    RuntimeClaimedConvergenceV2, RuntimeConvergenceFutureV2,
    RuntimeRefreshedStageReadyConvergenceV2, RuntimeStageReadyConvergenceV2,
};
use crate::{RuntimeRegistryGlobalObservationSequenceV2, RuntimeServingSlotWorkErrorV2};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeAuthorityPayloadDigestErrorV2 {
    #[error("runtime authority payload digest is invalid")]
    InvalidDigest,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeAuthorityPayloadDigestV2(String);

impl RuntimeAuthorityPayloadDigestV2 {
    pub fn parse(value: impl Into<String>) -> Result<Self, RuntimeAuthorityPayloadDigestErrorV2> {
        let value = value.into();
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(RuntimeAuthorityPayloadDigestErrorV2::InvalidDigest);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub struct RuntimeExactTargetHydrationRequestV2 {
    execution: RuntimeExecutionReceiptV1,
    process_identity: RuntimeProcessIdentityV1,
    route_set_sequence: RuntimeRegistryGlobalObservationSequenceV2,
}

impl RuntimeExactTargetHydrationRequestV2 {
    pub fn execution(&self) -> &RuntimeExecutionReceiptV1 {
        &self.execution
    }

    pub fn process_identity(&self) -> &RuntimeProcessIdentityV1 {
        &self.process_identity
    }

    pub fn route_set_sequence(&self) -> RuntimeRegistryGlobalObservationSequenceV2 {
        self.route_set_sequence
    }
}

impl Debug for RuntimeExactTargetHydrationRequestV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeExactTargetHydrationRequestV2(<redacted>)")
    }
}

pub struct RuntimeExactTargetObservationV2<H> {
    pub execution: RuntimeExecutionReceiptV1,
    pub persisted_desired_target_digest: RuntimeDesiredTargetDigestV1,
    pub installation_authority_revision: NonZeroU64,
    pub current_authority_revision: NonZeroU64,
    pub installation_authority_payload_digest: RuntimeAuthorityPayloadDigestV2,
    pub current_authority_payload_digest: RuntimeAuthorityPayloadDigestV2,
    pub artifact_target: RuntimeDeploymentTargetV1,
    pub binding_pin: RuntimeBindingPinV1,
    pub observed_database_now: DateTime<Utc>,
    pub hydrated: H,
}

impl<H> Debug for RuntimeExactTargetObservationV2<H> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeExactTargetObservationV2(<redacted>)")
    }
}

pub trait RuntimeExactTargetHydrationPortV2 {
    type Error;
    type Hydrated;

    fn load_exact_target<'a>(
        &'a self,
        request: &'a RuntimeExactTargetHydrationRequestV2,
    ) -> RuntimeConvergenceFutureV2<
        'a,
        Result<RuntimeExactTargetObservationV2<Self::Hydrated>, Self::Error>,
    >;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeExactTargetEvidenceErrorV2 {
    #[error("runtime exact target execution receipt does not match")]
    ExecutionMismatch,
    #[error("runtime exact target process identity does not match")]
    ProcessIdentityMismatch,
    #[error("runtime exact target artifact does not match")]
    ArtifactMismatch,
    #[error("runtime exact target binding pin does not match")]
    BindingPinMismatch,
    #[error("runtime exact target installation authority revision does not match")]
    InstallationAuthorityRevisionMismatch,
    #[error("runtime exact target current authority revision regressed")]
    CurrentAuthorityRevisionRegression,
    #[error("runtime exact target desired digest does not match")]
    DesiredTargetDigestMismatch,
    #[error("runtime exact target database observation is outside the claim lease")]
    DatabaseObservationOutsideLease,
    #[error("runtime exact target database observation predates stage readiness")]
    DatabaseObservationBeforeStageReady,
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeExactTargetHydrationErrorV2<E> {
    #[error("runtime exact target hydration slot work authority is not active")]
    SlotWork(RuntimeServingSlotWorkErrorV2),
    #[error("runtime exact target hydration port failed")]
    Port(E),
    #[error("runtime exact target hydration evidence is invalid")]
    Evidence(RuntimeExactTargetEvidenceErrorV2),
}

pub type RuntimeExactTargetHydrationResultV2<H, E> =
    Result<RuntimeHydratedConvergenceV2<H>, RuntimeExactTargetHydrationErrorV2<E>>;
pub type RuntimeStageReadyHydrationRefreshResultV2<H, E> =
    Result<RuntimeRefreshedStageReadyConvergenceV2<H>, RuntimeExactTargetHydrationErrorV2<E>>;

pub struct RuntimeExactTargetEvidenceV2 {
    execution: RuntimeExecutionReceiptV1,
    persisted_desired_target_digest: RuntimeDesiredTargetDigestV1,
    installation_authority_revision: NonZeroU64,
    current_authority_revision: NonZeroU64,
    installation_authority_payload_digest: RuntimeAuthorityPayloadDigestV2,
    current_authority_payload_digest: RuntimeAuthorityPayloadDigestV2,
    binding_pin: RuntimeBindingPinV1,
    observed_database_now: DateTime<Utc>,
}

impl RuntimeExactTargetEvidenceV2 {
    pub fn execution(&self) -> &RuntimeExecutionReceiptV1 {
        &self.execution
    }

    pub fn persisted_desired_target_digest(&self) -> &RuntimeDesiredTargetDigestV1 {
        &self.persisted_desired_target_digest
    }

    pub fn installation_authority_revision(&self) -> NonZeroU64 {
        self.installation_authority_revision
    }

    pub fn current_authority_revision(&self) -> NonZeroU64 {
        self.current_authority_revision
    }

    pub fn installation_authority_payload_digest(&self) -> &RuntimeAuthorityPayloadDigestV2 {
        &self.installation_authority_payload_digest
    }

    pub fn current_authority_payload_digest(&self) -> &RuntimeAuthorityPayloadDigestV2 {
        &self.current_authority_payload_digest
    }

    pub fn binding_pin(&self) -> &RuntimeBindingPinV1 {
        &self.binding_pin
    }

    pub fn observed_database_now(&self) -> DateTime<Utc> {
        self.observed_database_now
    }
}

impl Debug for RuntimeExactTargetEvidenceV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeExactTargetEvidenceV2(<redacted>)")
    }
}

pub(super) struct RuntimeHydratedCoreV2<H> {
    pub(super) claimed: RuntimeClaimedConvergenceV2,
    pub(super) evidence: RuntimeExactTargetEvidenceV2,
    pub(super) hydrated: H,
}

pub struct RuntimeExactTargetHydrationV2 {
    claimed: RuntimeClaimedConvergenceV2,
    request: RuntimeExactTargetHydrationRequestV2,
}

pub struct RuntimeStageReadyHydrationRefreshV2<H> {
    stage_ready: RuntimeStageReadyConvergenceV2<H>,
    request: RuntimeExactTargetHydrationRequestV2,
}

impl RuntimeExactTargetHydrationV2 {
    pub fn request(&self) -> &RuntimeExactTargetHydrationRequestV2 {
        &self.request
    }

    pub fn execute<'a, P>(
        self,
        port: &'a P,
    ) -> RuntimeConvergenceFutureV2<'a, RuntimeExactTargetHydrationResultV2<P::Hydrated, P::Error>>
    where
        P: RuntimeExactTargetHydrationPortV2 + Sync + 'a,
        P::Error: Send + 'a,
        P::Hydrated: Send + 'a,
    {
        Box::pin(async move {
            self.claimed
                .ensure_active()
                .map_err(RuntimeExactTargetHydrationErrorV2::SlotWork)?;
            let observation = port
                .load_exact_target(&self.request)
                .await
                .map_err(RuntimeExactTargetHydrationErrorV2::Port)?;
            self.claimed
                .ensure_active()
                .map_err(RuntimeExactTargetHydrationErrorV2::SlotWork)?;
            let evidence = validate_observation(&self.request, &observation)
                .map_err(RuntimeExactTargetHydrationErrorV2::Evidence)?;
            self.claimed
                .ensure_active()
                .map_err(RuntimeExactTargetHydrationErrorV2::SlotWork)?;
            Ok(RuntimeHydratedConvergenceV2 {
                core: RuntimeHydratedCoreV2 {
                    claimed: self.claimed,
                    evidence,
                    hydrated: observation.hydrated,
                },
            })
        })
    }
}

impl Debug for RuntimeExactTargetHydrationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeExactTargetHydrationV2(<redacted>)")
    }
}

impl<H> RuntimeStageReadyHydrationRefreshV2<H> {
    pub fn request(&self) -> &RuntimeExactTargetHydrationRequestV2 {
        &self.request
    }

    pub fn execute<'a, P>(
        self,
        port: &'a P,
    ) -> RuntimeConvergenceFutureV2<
        'a,
        RuntimeStageReadyHydrationRefreshResultV2<P::Hydrated, P::Error>,
    >
    where
        H: Send + 'a,
        P: RuntimeExactTargetHydrationPortV2 + Sync + 'a,
        P::Error: Send + 'a,
        P::Hydrated: Send + 'a,
    {
        Box::pin(async move {
            let RuntimeStageReadyConvergenceV2 { core, ready_at } = self.stage_ready;
            let RuntimeHydratedCoreV2 {
                claimed,
                evidence,
                hydrated,
            } = core;
            drop((evidence, hydrated));
            claimed
                .ensure_active()
                .map_err(RuntimeExactTargetHydrationErrorV2::SlotWork)?;
            let observation = port
                .load_exact_target(&self.request)
                .await
                .map_err(RuntimeExactTargetHydrationErrorV2::Port)?;
            claimed
                .ensure_active()
                .map_err(RuntimeExactTargetHydrationErrorV2::SlotWork)?;
            let evidence = validate_observation(&self.request, &observation)
                .map_err(RuntimeExactTargetHydrationErrorV2::Evidence)?;
            if evidence.observed_database_now() < ready_at {
                return Err(RuntimeExactTargetHydrationErrorV2::Evidence(
                    RuntimeExactTargetEvidenceErrorV2::DatabaseObservationBeforeStageReady,
                ));
            }
            claimed
                .ensure_active()
                .map_err(RuntimeExactTargetHydrationErrorV2::SlotWork)?;
            let refreshed_at = evidence.observed_database_now();
            Ok(RuntimeRefreshedStageReadyConvergenceV2::from_refresh(
                RuntimeHydratedCoreV2 {
                    claimed,
                    evidence,
                    hydrated: observation.hydrated,
                },
                refreshed_at,
            ))
        })
    }
}

impl<H> Debug for RuntimeStageReadyHydrationRefreshV2<H> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeStageReadyHydrationRefreshV2(<redacted>)")
    }
}

pub struct RuntimeHydratedConvergenceV2<H> {
    pub(super) core: RuntimeHydratedCoreV2<H>,
}

impl<H> RuntimeHydratedConvergenceV2<H> {
    pub fn evidence(&self) -> &RuntimeExactTargetEvidenceV2 {
        &self.core.evidence
    }

    pub fn hydrated(&self) -> &H {
        &self.core.hydrated
    }

    pub fn process_identity(&self) -> &RuntimeProcessIdentityV1 {
        &self.core.claimed.process_identity
    }

    pub fn ensure_active(&self) -> Result<(), RuntimeServingSlotWorkErrorV2> {
        self.core.claimed.ensure_active()
    }
}

impl<H> Debug for RuntimeHydratedConvergenceV2<H> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeHydratedConvergenceV2(<redacted>)")
    }
}

impl RuntimeClaimedConvergenceV2 {
    pub fn begin_hydration(
        self,
    ) -> Result<RuntimeExactTargetHydrationV2, RuntimeExactTargetHydrationErrorV2<()>> {
        self.ensure_active()
            .map_err(RuntimeExactTargetHydrationErrorV2::SlotWork)?;
        let request = hydration_request(&self)?;
        Ok(RuntimeExactTargetHydrationV2 {
            claimed: self,
            request,
        })
    }
}

impl<H> RuntimeStageReadyConvergenceV2<H> {
    pub fn begin_exact_hydration_refresh(
        self,
    ) -> Result<RuntimeStageReadyHydrationRefreshV2<H>, RuntimeExactTargetHydrationErrorV2<()>>
    {
        self.core
            .claimed
            .ensure_active()
            .map_err(RuntimeExactTargetHydrationErrorV2::SlotWork)?;
        let request = hydration_request(&self.core.claimed)?;
        Ok(RuntimeStageReadyHydrationRefreshV2 {
            stage_ready: self,
            request,
        })
    }
}

fn hydration_request(
    claimed: &RuntimeClaimedConvergenceV2,
) -> Result<RuntimeExactTargetHydrationRequestV2, RuntimeExactTargetHydrationErrorV2<()>> {
    let execution = claimed.session.current_execution_receipt().map_err(|_| {
        RuntimeExactTargetHydrationErrorV2::Evidence(
            RuntimeExactTargetEvidenceErrorV2::ExecutionMismatch,
        )
    })?;
    Ok(RuntimeExactTargetHydrationRequestV2 {
        execution,
        process_identity: claimed.process_identity.clone(),
        route_set_sequence: claimed.permit.route_set_sequence(),
    })
}

fn validate_observation<H>(
    request: &RuntimeExactTargetHydrationRequestV2,
    observation: &RuntimeExactTargetObservationV2<H>,
) -> Result<RuntimeExactTargetEvidenceV2, RuntimeExactTargetEvidenceErrorV2> {
    if observation.execution != request.execution {
        return Err(RuntimeExactTargetEvidenceErrorV2::ExecutionMismatch);
    }
    let snapshot = &request.execution.snapshot;
    if request.process_identity.target != snapshot.target
        || request.process_identity.runtime_generation != snapshot.runtime_generation
    {
        return Err(RuntimeExactTargetEvidenceErrorV2::ProcessIdentityMismatch);
    }
    if observation.artifact_target != snapshot.target {
        return Err(RuntimeExactTargetEvidenceErrorV2::ArtifactMismatch);
    }
    let scope = RuntimeDeploymentScopeV1::from_identity(&snapshot.identity);
    if !observation.binding_pin.matches(&scope, &snapshot.target) {
        return Err(RuntimeExactTargetEvidenceErrorV2::BindingPinMismatch);
    }
    if observation.binding_pin.installation_authority_revision
        != observation.installation_authority_revision
    {
        return Err(RuntimeExactTargetEvidenceErrorV2::InstallationAuthorityRevisionMismatch);
    }
    if observation.current_authority_revision < observation.installation_authority_revision {
        return Err(RuntimeExactTargetEvidenceErrorV2::CurrentAuthorityRevisionRegression);
    }
    let expected_digest = runtime_desired_target_digest_v1(
        &snapshot.identity,
        &snapshot.target,
        snapshot.runtime_generation.get(),
        observation.installation_authority_revision.get(),
        snapshot.previous_runtime.as_ref(),
    );
    if observation.persisted_desired_target_digest != expected_digest {
        return Err(RuntimeExactTargetEvidenceErrorV2::DesiredTargetDigestMismatch);
    }
    if observation.observed_database_now < request.execution.acquired_at
        || observation.observed_database_now >= request.execution.expires_at
    {
        return Err(RuntimeExactTargetEvidenceErrorV2::DatabaseObservationOutsideLease);
    }
    Ok(RuntimeExactTargetEvidenceV2 {
        execution: observation.execution.clone(),
        persisted_desired_target_digest: observation.persisted_desired_target_digest.clone(),
        installation_authority_revision: observation.installation_authority_revision,
        current_authority_revision: observation.current_authority_revision,
        installation_authority_payload_digest: observation
            .installation_authority_payload_digest
            .clone(),
        current_authority_payload_digest: observation.current_authority_payload_digest.clone(),
        binding_pin: observation.binding_pin.clone(),
        observed_database_now: observation.observed_database_now,
    })
}
