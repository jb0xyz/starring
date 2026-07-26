mod wire;

#[cfg(test)]
mod tests;

use crate::v2_digest::suspend_attempt_digest_v2;
use crate::{
    RuntimeAttemptDispositionV2, RuntimeCanonicalValueErrorV2, RuntimeDrainObligationV2,
    RuntimeExactLocalRouteIdentityV2, RuntimeLocalRouteEffectV2,
    RuntimePreviousServingLeaseIdentityV1, RuntimeRouteMutationProvenanceV2, RuntimeServingSlotV2,
    RuntimeSuspendAttemptDigestV2, RuntimeSuspendAttemptRequestV2,
};

const SUSPEND_ATTEMPT_MAX_OCTETS: usize = 131_072;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeSuspendAttemptCanonicalFieldV2 {
    SuspensionId,
    ActionId,
    GuardTenantId,
    GuardInstallationId,
    GuardDeploymentId,
    GuardExpectedRevision,
    GuardControllerId,
    GuardFencingToken,
    GuardRuntimeGeneration,
    GuardConvergenceAttempt,
    SourcePhase,
    FailureId,
    FailureKind,
    FailureCode,
    FailureMessage,
    FailureRecordedAtUnixMicroseconds,
    Disposition,
    RetryNotBeforeUnixMicroseconds,
    Checkpoint,
    LocalTargetGuildId,
    LocalTargetRuleSetKey,
    LocalTargetVersion,
    LocalTargetContentHash,
    LocalTargetBindingRevision,
    LocalTargetBindingFingerprint,
    LocalRuntimeGeneration,
    LocalProcessInstanceId,
    LocalControllerFencingToken,
    LocalRouteIncarnation,
    RouteLifecycle,
    RouteAbsentSlotGuildId,
    RouteAbsentSlotRuleSetKey,
    RouteAbsentObservedSequence,
    PreviousTenantId,
    PreviousInstallationId,
    PreviousDeploymentId,
    PreviousAttestationId,
    PreviousTargetGuildId,
    PreviousTargetRuleSetKey,
    PreviousTargetVersion,
    PreviousTargetContentHash,
    PreviousTargetBindingRevision,
    PreviousTargetBindingFingerprint,
    PreviousRuntimeGeneration,
    PreviousProcessInstanceId,
    PreviousLeaseEpoch,
    PreviousRevision,
    RouteProvenance,
    BarrierId,
    PauseCoordinatorGeneration,
    PauseConnectionEpoch,
    PauseAdmissionRevision,
    PauseSequence,
    RecoveryId,
    OriginatingEmergencyGeneration,
    RecoveryGeneration,
    RecoveryAuthorityRevision,
    ShutdownGeneration,
    GatewayShardId,
    GatewayProcessInstanceId,
    GatewayLeaseEpoch,
    GatewayExpectedBuildRevision,
    ObservedOwnerRevision,
    OwnerExpiresAtUnixMicroseconds,
    ProvenanceProcessInstanceId,
    ProvenanceConnectionEpoch,
    ProvenancePausedAdmissionRevision,
    ConnectedEventSequence,
    ProvenancePauseSequence,
    LocalEffect,
    DrainObligation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeSuspendAttemptCorrelationV2 {
    SourcePhaseCheckpoint,
    FailureDispositionTime,
    LocalRouteRuntimeGeneration,
    LocalRouteControllerFencingToken,
    LocalRouteIdentity,
    PreviousServingProductScope,
    PreviousServingRuntimeGeneration,
    ServingSlot,
    LocalEffectDrainObligation,
    RouteProvenanceProcess,
    RouteProvenanceGeneration,
    RouteProvenanceSequence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeSuspendAttemptCanonicalErrorV2 {
    #[error("runtime suspend-attempt canonical payload exceeds its size limit")]
    PayloadTooLarge,
    #[error("runtime suspend-attempt canonical payload encoding failed")]
    Encoding,
    #[error("runtime suspend-attempt canonical payload decoding failed")]
    Decoding,
    #[error("runtime suspend-attempt canonical payload format version is unsupported")]
    UnsupportedFormatVersion,
    #[error("runtime suspend-attempt canonical payload has a noncanonical representation")]
    NonCanonicalEncoding,
    #[error("runtime suspend-attempt canonical field {field:?} is invalid")]
    InvalidField {
        field: RuntimeSuspendAttemptCanonicalFieldV2,
    },
    #[error("runtime suspend-attempt canonical field {field:?} is invalid: {reason}")]
    CanonicalValue {
        field: RuntimeSuspendAttemptCanonicalFieldV2,
        reason: RuntimeCanonicalValueErrorV2,
    },
    #[error("runtime suspend-attempt fields disagree on {field:?}")]
    CorrelationMismatch {
        field: RuntimeSuspendAttemptCorrelationV2,
    },
    #[error("runtime suspend-attempt persisted digest does not match its canonical payload")]
    PersistedDigestMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCanonicalSuspendAttemptV2 {
    request: RuntimeSuspendAttemptRequestV2,
    bytes: Box<[u8]>,
    digest: RuntimeSuspendAttemptDigestV2,
}

impl RuntimeCanonicalSuspendAttemptV2 {
    pub fn new(
        request: RuntimeSuspendAttemptRequestV2,
    ) -> Result<Self, RuntimeSuspendAttemptCanonicalErrorV2> {
        let bytes = wire::encode_suspend_attempt(&request)?;
        let digest = suspend_attempt_digest_v2(&bytes);
        Ok(Self {
            request,
            bytes: bytes.into_boxed_slice(),
            digest,
        })
    }

    pub fn from_persisted(
        bytes: &[u8],
        persisted_digest: &RuntimeSuspendAttemptDigestV2,
    ) -> Result<Self, RuntimeSuspendAttemptCanonicalErrorV2> {
        let request = wire::decode_suspend_attempt(bytes)?;
        let digest = suspend_attempt_digest_v2(bytes);
        if digest != *persisted_digest {
            return Err(RuntimeSuspendAttemptCanonicalErrorV2::PersistedDigestMismatch);
        }
        Ok(Self {
            request,
            bytes: bytes.to_vec().into_boxed_slice(),
            digest,
        })
    }

    pub fn request(&self) -> &RuntimeSuspendAttemptRequestV2 {
        &self.request
    }

    pub fn suspend_attempt_request_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn suspend_attempt_digest(&self) -> &RuntimeSuspendAttemptDigestV2 {
        &self.digest
    }
}

fn validate_request(
    request: &RuntimeSuspendAttemptRequestV2,
) -> Result<(), RuntimeSuspendAttemptCanonicalErrorV2> {
    if request.source_phase.required_checkpoint() != request.checkpoint {
        return Err(correlation(
            RuntimeSuspendAttemptCorrelationV2::SourcePhaseCheckpoint,
        ));
    }
    validate_failure(request)?;
    validate_effect_and_obligation(request, &request.local_effect, &request.drain_obligation)
}

pub(crate) fn validate_suspend_attempt_mutable_state(
    request: &RuntimeSuspendAttemptRequestV2,
    local_effect: &RuntimeLocalRouteEffectV2,
    drain_obligation: &RuntimeDrainObligationV2,
) -> Result<(), RuntimeSuspendAttemptCanonicalErrorV2> {
    validate_effect_and_obligation(request, local_effect, drain_obligation)?;
    wire::validate_suspend_attempt_mutable_state(local_effect, drain_obligation)
}

fn validate_effect_and_obligation(
    request: &RuntimeSuspendAttemptRequestV2,
    local_effect: &RuntimeLocalRouteEffectV2,
    drain_obligation: &RuntimeDrainObligationV2,
) -> Result<(), RuntimeSuspendAttemptCanonicalErrorV2> {
    let mut slot = None;
    match (local_effect, drain_obligation) {
        (RuntimeLocalRouteEffectV2::None, RuntimeDrainObligationV2::None) => {}
        (RuntimeLocalRouteEffectV2::None, RuntimeDrainObligationV2::PreviousServing(previous)) => {
            validate_previous(previous, request, &mut slot)?
        }
        (
            RuntimeLocalRouteEffectV2::ExactRoute { route, .. },
            RuntimeDrainObligationV2::ExactLocalRoute(obligation),
        ) => {
            validate_matching_local_routes(route, obligation, request, &mut slot)?;
        }
        (
            RuntimeLocalRouteEffectV2::ExactRoute { route, .. },
            RuntimeDrainObligationV2::LocalAndPrevious { local, previous },
        ) => {
            validate_matching_local_routes(route, local, request, &mut slot)?;
            validate_previous(previous, request, &mut slot)?;
        }
        (
            RuntimeLocalRouteEffectV2::RouteAbsent {
                slot: absent_slot,
                expected_route,
                provenance,
                ..
            },
            RuntimeDrainObligationV2::None,
        ) => {
            merge_slot(&mut slot, absent_slot)?;
            validate_expected_route(expected_route.as_ref(), request, &mut slot)?;
            validate_provenance(provenance)?;
        }
        (
            RuntimeLocalRouteEffectV2::RouteAbsent {
                slot: absent_slot,
                expected_route,
                provenance,
                ..
            },
            RuntimeDrainObligationV2::PreviousServing(previous),
        ) => {
            merge_slot(&mut slot, absent_slot)?;
            validate_expected_route(expected_route.as_ref(), request, &mut slot)?;
            validate_provenance(provenance)?;
            validate_previous(previous, request, &mut slot)?;
        }
        _ => {
            return Err(correlation(
                RuntimeSuspendAttemptCorrelationV2::LocalEffectDrainObligation,
            ));
        }
    }
    Ok(())
}

fn validate_failure(
    request: &RuntimeSuspendAttemptRequestV2,
) -> Result<(), RuntimeSuspendAttemptCanonicalErrorV2> {
    let code = &request.failure.code;
    if code.is_empty()
        || code.len() > 64
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(invalid(RuntimeSuspendAttemptCanonicalFieldV2::FailureCode));
    }
    if request.failure.message.trim().is_empty() || request.failure.message.len() > 1024 {
        return Err(invalid(
            RuntimeSuspendAttemptCanonicalFieldV2::FailureMessage,
        ));
    }
    if matches!(
        &request.disposition,
        RuntimeAttemptDispositionV2::Retryable { retry_not_before }
            if *retry_not_before < request.failure.recorded_at
    ) {
        return Err(correlation(
            RuntimeSuspendAttemptCorrelationV2::FailureDispositionTime,
        ));
    }
    Ok(())
}

fn validate_matching_local_routes(
    effect: &RuntimeExactLocalRouteIdentityV2,
    obligation: &RuntimeExactLocalRouteIdentityV2,
    request: &RuntimeSuspendAttemptRequestV2,
    slot: &mut Option<RuntimeServingSlotV2>,
) -> Result<(), RuntimeSuspendAttemptCanonicalErrorV2> {
    if effect != obligation {
        return Err(correlation(
            RuntimeSuspendAttemptCorrelationV2::LocalRouteIdentity,
        ));
    }
    validate_local_route(effect, request, slot)
}

fn validate_expected_route(
    expected: Option<&RuntimeExactLocalRouteIdentityV2>,
    request: &RuntimeSuspendAttemptRequestV2,
    slot: &mut Option<RuntimeServingSlotV2>,
) -> Result<(), RuntimeSuspendAttemptCanonicalErrorV2> {
    if let Some(expected) = expected {
        validate_local_route(expected, request, slot)?;
    }
    Ok(())
}

fn validate_local_route(
    route: &RuntimeExactLocalRouteIdentityV2,
    request: &RuntimeSuspendAttemptRequestV2,
    slot: &mut Option<RuntimeServingSlotV2>,
) -> Result<(), RuntimeSuspendAttemptCanonicalErrorV2> {
    if route.identity.runtime_generation != request.guard.runtime_generation {
        return Err(correlation(
            RuntimeSuspendAttemptCorrelationV2::LocalRouteRuntimeGeneration,
        ));
    }
    if route.controller_fencing_token != request.guard.fencing_token {
        return Err(correlation(
            RuntimeSuspendAttemptCorrelationV2::LocalRouteControllerFencingToken,
        ));
    }
    merge_slot(slot, &route.slot())
}

fn validate_previous(
    previous: &RuntimePreviousServingLeaseIdentityV1,
    request: &RuntimeSuspendAttemptRequestV2,
    slot: &mut Option<RuntimeServingSlotV2>,
) -> Result<(), RuntimeSuspendAttemptCanonicalErrorV2> {
    if previous.scope.tenant_id != request.guard.scope.tenant_id
        || previous.scope.installation_id != request.guard.scope.installation_id
        || previous.scope.deployment_id == request.guard.scope.deployment_id
    {
        return Err(correlation(
            RuntimeSuspendAttemptCorrelationV2::PreviousServingProductScope,
        ));
    }
    if previous.process.runtime_generation >= request.guard.runtime_generation {
        return Err(correlation(
            RuntimeSuspendAttemptCorrelationV2::PreviousServingRuntimeGeneration,
        ));
    }
    merge_slot(
        slot,
        &RuntimeServingSlotV2::from_target(&previous.process.target),
    )
}

fn validate_provenance(
    provenance: &RuntimeRouteMutationProvenanceV2,
) -> Result<(), RuntimeSuspendAttemptCanonicalErrorV2> {
    match provenance {
        RuntimeRouteMutationProvenanceV2::Ordinary { .. } => Ok(()),
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(witness) => {
            if witness
                .originating_emergency_generation
                .get()
                .checked_add(1)
                != Some(witness.recovery_generation.get())
            {
                return Err(correlation(
                    RuntimeSuspendAttemptCorrelationV2::RouteProvenanceGeneration,
                ));
            }
            if witness.gateway_owner_lease_id.process_instance_id != witness.process_instance_id {
                return Err(correlation(
                    RuntimeSuspendAttemptCorrelationV2::RouteProvenanceProcess,
                ));
            }
            if witness.pause_sequence.get() <= witness.connected_event_sequence.get() {
                return Err(correlation(
                    RuntimeSuspendAttemptCorrelationV2::RouteProvenanceSequence,
                ));
            }
            Ok(())
        }
        RuntimeRouteMutationProvenanceV2::Shutdown(witness) => {
            if witness.gateway_owner_lease_id.process_instance_id != witness.process_instance_id {
                return Err(correlation(
                    RuntimeSuspendAttemptCorrelationV2::RouteProvenanceProcess,
                ));
            }
            if witness.pause_sequence.get() <= witness.connected_event_sequence.get() {
                return Err(correlation(
                    RuntimeSuspendAttemptCorrelationV2::RouteProvenanceSequence,
                ));
            }
            Ok(())
        }
    }
}

fn merge_slot(
    current: &mut Option<RuntimeServingSlotV2>,
    candidate: &RuntimeServingSlotV2,
) -> Result<(), RuntimeSuspendAttemptCanonicalErrorV2> {
    if current.as_ref().is_some_and(|slot| slot != candidate) {
        return Err(correlation(RuntimeSuspendAttemptCorrelationV2::ServingSlot));
    }
    if current.is_none() {
        current.replace(candidate.clone());
    }
    Ok(())
}

fn invalid(field: RuntimeSuspendAttemptCanonicalFieldV2) -> RuntimeSuspendAttemptCanonicalErrorV2 {
    RuntimeSuspendAttemptCanonicalErrorV2::InvalidField { field }
}

fn correlation(field: RuntimeSuspendAttemptCorrelationV2) -> RuntimeSuspendAttemptCanonicalErrorV2 {
    RuntimeSuspendAttemptCanonicalErrorV2::CorrelationMismatch { field }
}
