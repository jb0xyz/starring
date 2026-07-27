use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::num::NonZeroU64;
use std::time::{Duration, Instant};

use automation_runtime_controller::{
    RuntimeGatewayOwnerLeaseIdV1, RuntimeGatewayOwnerLeaseReceiptV1, RuntimeRecoveryIdV2,
};
use automation_runtime_convergence::ProcessInstanceId;
use chrono::{DateTime, Utc};

use crate::closed_recovery::{
    RuntimeClosedRecoveryOperationAuthorityV2, RuntimePendingStartupRecoveryExecutionV2,
};
use crate::{
    RuntimeCapabilityReadinessSetV2, RuntimeClosedDrainRecoveryPermitV2,
    RuntimeClosedRecoveryAuthorityRevisionV2, RuntimePausedGatewayObservationV2,
    RuntimeRegistryGlobalObservationSequenceV2, RuntimeRegistryRecoveryEmptyObservationV2,
    RuntimeStartupRecoveryClassV2, RuntimeStartupRecoveryContinuationV2,
};

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeStartupRecoveryExecutionCorrelationV2 {
    recovery_id: RuntimeRecoveryIdV2,
    originating_emergency_generation: NonZeroU64,
    coordinator_generation: NonZeroU64,
    authority_revision: NonZeroU64,
    selection_authority_revision: NonZeroU64,
}

impl RuntimeStartupRecoveryExecutionCorrelationV2 {
    pub fn recovery_id(&self) -> &RuntimeRecoveryIdV2 {
        &self.recovery_id
    }

    pub fn originating_emergency_generation(&self) -> NonZeroU64 {
        self.originating_emergency_generation
    }

    pub fn coordinator_generation(&self) -> NonZeroU64 {
        self.coordinator_generation
    }

    pub fn authority_revision(&self) -> NonZeroU64 {
        self.authority_revision
    }

    pub fn selection_authority_revision(&self) -> NonZeroU64 {
        self.selection_authority_revision
    }

    #[cfg(test)]
    pub(crate) fn replace_authority_revision_for_test(&mut self, authority_revision: NonZeroU64) {
        self.authority_revision = authority_revision;
    }
}

impl Debug for RuntimeStartupRecoveryExecutionCorrelationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeStartupRecoveryExecutionCorrelationV2(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeStartupRecoveryExecutionActionIdentityV2 {
    correlation: RuntimeStartupRecoveryExecutionCorrelationV2,
    class: RuntimeStartupRecoveryClassV2,
}

impl RuntimeStartupRecoveryExecutionActionIdentityV2 {
    pub fn correlation(&self) -> &RuntimeStartupRecoveryExecutionCorrelationV2 {
        &self.correlation
    }

    pub fn class(&self) -> RuntimeStartupRecoveryClassV2 {
        self.class
    }

    pub(crate) fn pending_drain_acknowledgement_successor(&self) -> Option<Self> {
        if self.class != RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent {
            return None;
        }
        let authority_revision = self
            .correlation
            .authority_revision
            .get()
            .checked_add(1)
            .filter(|revision| *revision <= i64::MAX as u64)
            .and_then(NonZeroU64::new)?;
        let mut correlation = self.correlation.clone();
        correlation.selection_authority_revision = correlation.authority_revision;
        correlation.authority_revision = authority_revision;
        Some(Self {
            correlation,
            class: self.class,
        })
    }
}

impl Debug for RuntimeStartupRecoveryExecutionActionIdentityV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeStartupRecoveryExecutionActionIdentityV2(<redacted>)")
    }
}

#[derive(PartialEq, Eq)]
pub struct RuntimeStartupRecoveryExecutionTerminalDigestV2([u8; 32]);

impl RuntimeStartupRecoveryExecutionTerminalDigestV2 {
    pub fn new(value: [u8; 32]) -> Result<Self, RuntimeStartupRecoveryExecutionDigestErrorV2> {
        if value == [0; 32] {
            return Err(RuntimeStartupRecoveryExecutionDigestErrorV2::Zero);
        }
        Ok(Self(value))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Debug for RuntimeStartupRecoveryExecutionTerminalDigestV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeStartupRecoveryExecutionTerminalDigestV2(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeStartupRecoveryExecutionDigestErrorV2 {
    #[error("runtime startup recovery execution terminal digest is zero")]
    Zero,
}

#[derive(PartialEq, Eq)]
pub struct RuntimeStartupRecoveryExecutionRequestV2 {
    correlation: RuntimeStartupRecoveryExecutionCorrelationV2,
    class: RuntimeStartupRecoveryClassV2,
    action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
    gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,
    expected_owner_revision: NonZeroU64,
    expected_owner_expires_at: DateTime<Utc>,
    minimum_database_now: DateTime<Utc>,
    readiness: RuntimeCapabilityReadinessSetV2,
    paused_gateway: RuntimePausedGatewayObservationV2,
    registry_process_instance_id: ProcessInstanceId,
    registry_observation_sequence: RuntimeRegistryGlobalObservationSequenceV2,
    registry_retained_slot_count: u64,
    registry_retained_empty_tombstone_count: u64,
}

impl RuntimeStartupRecoveryExecutionRequestV2 {
    pub fn correlation(&self) -> &RuntimeStartupRecoveryExecutionCorrelationV2 {
        &self.correlation
    }

    pub fn class(&self) -> RuntimeStartupRecoveryClassV2 {
        self.class
    }

    pub fn action_identity(&self) -> &RuntimeStartupRecoveryExecutionActionIdentityV2 {
        &self.action_identity
    }

    pub fn gateway_owner_lease_id(&self) -> &RuntimeGatewayOwnerLeaseIdV1 {
        &self.gateway_owner_lease_id
    }

    pub fn expected_owner_revision(&self) -> NonZeroU64 {
        self.expected_owner_revision
    }

    pub fn expected_owner_expires_at(&self) -> DateTime<Utc> {
        self.expected_owner_expires_at
    }

    pub fn minimum_database_now(&self) -> DateTime<Utc> {
        self.minimum_database_now
    }

    pub fn readiness(&self) -> &RuntimeCapabilityReadinessSetV2 {
        &self.readiness
    }

    pub fn paused_gateway(&self) -> &RuntimePausedGatewayObservationV2 {
        &self.paused_gateway
    }

    pub fn registry_process_instance_id(&self) -> &ProcessInstanceId {
        &self.registry_process_instance_id
    }

    pub fn registry_observation_sequence(&self) -> RuntimeRegistryGlobalObservationSequenceV2 {
        self.registry_observation_sequence
    }

    pub fn registry_retained_slot_count(&self) -> u64 {
        self.registry_retained_slot_count
    }

    pub fn registry_retained_empty_tombstone_count(&self) -> u64 {
        self.registry_retained_empty_tombstone_count
    }
}

impl Debug for RuntimeStartupRecoveryExecutionRequestV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeStartupRecoveryExecutionRequestV2(<redacted>)")
    }
}

pub struct RuntimeAuthorizedStartupRecoveryExecutionV2 {
    request: RuntimeStartupRecoveryExecutionRequestV2,
    operation_authority: RuntimeClosedRecoveryOperationAuthorityV2,
}

impl RuntimeAuthorizedStartupRecoveryExecutionV2 {
    pub fn request(&self) -> &RuntimeStartupRecoveryExecutionRequestV2 {
        &self.request
    }

    pub fn complete(
        self,
        receipt: RuntimeStartupRecoveryExecutionReceiptV2,
    ) -> RuntimeCompletedStartupRecoveryExecutionV2 {
        RuntimeCompletedStartupRecoveryExecutionV2 {
            authorization: self,
            receipt,
            pending_drain_proof: None,
            pending_registry_successor: None,
        }
    }

    pub fn into_pending_drain_selection(
        self,
    ) -> Result<
        crate::RuntimeAuthorizedPendingDrainSelectionV2,
        crate::RuntimePendingDrainCompoundErrorV2,
    > {
        crate::startup_pending_drain::authorize_pending_drain_selection_v2(self)
    }

    pub(crate) fn complete_pending_drain(
        self,
        receipt: RuntimeStartupRecoveryExecutionReceiptV2,
        proof: crate::startup_pending_drain::RuntimePendingDrainExecutionProofV2,
        registry_successor: Option<RuntimeRegistryRecoveryEmptyObservationV2>,
    ) -> RuntimeCompletedStartupRecoveryExecutionV2 {
        RuntimeCompletedStartupRecoveryExecutionV2 {
            authorization: self,
            receipt,
            pending_drain_proof: Some(proof),
            pending_registry_successor: registry_successor,
        }
    }
}

impl Debug for RuntimeAuthorizedStartupRecoveryExecutionV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAuthorizedStartupRecoveryExecutionV2(<redacted>)")
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeStartupRecoveryExecutionReceiptOutcomeV2 {
    Progressed {
        action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
        terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
    },
    NoCandidate,
    RetryAfter {
        retry_after: Duration,
    },
}

pub struct RuntimeStartupRecoveryExecutionReceiptV2 {
    pub correlation: RuntimeStartupRecoveryExecutionCorrelationV2,
    pub class: RuntimeStartupRecoveryClassV2,
    pub owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    pub outcome: RuntimeStartupRecoveryExecutionReceiptOutcomeV2,
}

impl Debug for RuntimeStartupRecoveryExecutionReceiptV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeStartupRecoveryExecutionReceiptV2(<redacted>)")
    }
}

pub struct RuntimeCompletedStartupRecoveryExecutionV2 {
    authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    receipt: RuntimeStartupRecoveryExecutionReceiptV2,
    pending_drain_proof: Option<crate::startup_pending_drain::RuntimePendingDrainExecutionProofV2>,
    pending_registry_successor: Option<RuntimeRegistryRecoveryEmptyObservationV2>,
}

impl Debug for RuntimeCompletedStartupRecoveryExecutionV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCompletedStartupRecoveryExecutionV2(<redacted>)")
    }
}

pub struct RuntimeAcceptedStartupRecoveryExecutionOutcomeV2 {
    class: RuntimeStartupRecoveryClassV2,
    outcome: RuntimeStartupRecoveryExecutionReceiptOutcomeV2,
    successor_authority_revision: RuntimeClosedRecoveryAuthorityRevisionV2,
    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    pending_drain_proof: Option<crate::startup_pending_drain::RuntimePendingDrainExecutionProofV2>,
}

impl RuntimeAcceptedStartupRecoveryExecutionOutcomeV2 {
    pub fn class(&self) -> RuntimeStartupRecoveryClassV2 {
        self.class
    }

    pub fn outcome(&self) -> &RuntimeStartupRecoveryExecutionReceiptOutcomeV2 {
        &self.outcome
    }

    pub fn successor_authority_revision(&self) -> RuntimeClosedRecoveryAuthorityRevisionV2 {
        self.successor_authority_revision
    }

    pub fn owner_receipt(&self) -> &RuntimeGatewayOwnerLeaseReceiptV1 {
        &self.owner_receipt
    }

    pub fn pending_drain_proof(
        &self,
    ) -> Option<&crate::startup_pending_drain::RuntimePendingDrainExecutionProofV2> {
        self.pending_drain_proof.as_ref()
    }
}

impl Debug for RuntimeAcceptedStartupRecoveryExecutionOutcomeV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAcceptedStartupRecoveryExecutionOutcomeV2(<redacted>)")
    }
}

pub trait RuntimeStartupRecoveryExecutionPortV2 {
    type Error;

    fn execute_startup_recovery(
        &self,
        authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
        operation_cutoff: Instant,
    ) -> impl Future<Output = Result<RuntimeCompletedStartupRecoveryExecutionV2, Self::Error>> + Send;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeStartupRecoveryExecutionAcceptanceErrorV2 {
    #[error("runtime startup recovery execution correlation does not match")]
    CorrelationMismatch,
    #[error("runtime startup recovery execution class does not match")]
    ClassMismatch,
    #[error("runtime startup recovery execution owner does not match")]
    OwnerMismatch,
    #[error("runtime startup recovery execution database clock does not match")]
    DatabaseClockMismatch,
    #[error("runtime startup recovery execution database clock regressed")]
    DatabaseClockRegressed,
    #[error("runtime startup recovery execution owner is not current")]
    OwnerNotCurrent,
    #[error("runtime startup recovery execution capability readiness does not match")]
    CapabilityReadinessMismatch,
    #[error("runtime startup recovery execution paused gateway evidence does not match")]
    PausedGatewayMismatch,
    #[error("runtime startup recovery execution registry evidence does not match")]
    RegistryMismatch,
    #[error("runtime startup recovery execution retry delay is invalid")]
    InvalidRetryAfter,
    #[error("runtime startup recovery execution progress proof does not match")]
    ProgressProofMismatch,
}

pub(crate) struct RuntimeValidatedStartupRecoveryExecutionV2 {
    operation_authority: RuntimeClosedRecoveryOperationAuthorityV2,
    request: RuntimeStartupRecoveryExecutionRequestV2,
    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    outcome: RuntimeStartupRecoveryExecutionReceiptOutcomeV2,
    pending_drain_proof: Option<crate::startup_pending_drain::RuntimePendingDrainExecutionProofV2>,
    pending_registry_successor: Option<RuntimeRegistryRecoveryEmptyObservationV2>,
}

pub(crate) fn authorize_startup_recovery_execution_v2(
    permit: &mut RuntimeClosedDrainRecoveryPermitV2,
    continuation: RuntimeStartupRecoveryContinuationV2,
) -> Option<RuntimeAuthorizedStartupRecoveryExecutionV2> {
    let RuntimeStartupRecoveryContinuationV2::Recover(class) = continuation else {
        return None;
    };
    let pending = permit.pending_startup_recovery_execution()?;
    if pending.class() != class {
        return None;
    }
    let request = startup_recovery_execution_request_v2(permit, pending);
    let operation_authority = permit.take_operation_authority()?;
    Some(RuntimeAuthorizedStartupRecoveryExecutionV2 {
        request,
        operation_authority,
    })
}

pub(crate) fn validate_startup_recovery_execution_v2(
    permit: &RuntimeClosedDrainRecoveryPermitV2,
    completed: RuntimeCompletedStartupRecoveryExecutionV2,
) -> Result<
    RuntimeValidatedStartupRecoveryExecutionV2,
    RuntimeStartupRecoveryExecutionAcceptanceErrorV2,
> {
    let RuntimeCompletedStartupRecoveryExecutionV2 {
        authorization,
        receipt,
        pending_drain_proof,
        pending_registry_successor,
    } = completed;
    let RuntimeAuthorizedStartupRecoveryExecutionV2 {
        request,
        operation_authority,
    } = authorization;
    validate_execution_request_binding_v2(permit, &request)?;
    let requires_pending_drain_proof =
        request.class == RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent;
    if requires_pending_drain_proof != pending_drain_proof.is_some()
        || pending_drain_proof.as_ref().is_some_and(|proof| {
            !proof.matches_request(&request)
                || proof.requires_registry_successor() != pending_registry_successor.is_some()
        })
        || (pending_drain_proof.is_none() && pending_registry_successor.is_some())
    {
        return Err(RuntimeStartupRecoveryExecutionAcceptanceErrorV2::ProgressProofMismatch);
    }
    if receipt.correlation != request.correlation {
        return Err(RuntimeStartupRecoveryExecutionAcceptanceErrorV2::CorrelationMismatch);
    }
    if receipt.class != request.class {
        return Err(RuntimeStartupRecoveryExecutionAcceptanceErrorV2::ClassMismatch);
    }
    let observed_owner = &receipt.owner_receipt;
    if observed_owner.lease_id != request.gateway_owner_lease_id
        || observed_owner.owner_revision != request.expected_owner_revision
        || observed_owner.expires_at != request.expected_owner_expires_at
    {
        return Err(RuntimeStartupRecoveryExecutionAcceptanceErrorV2::OwnerMismatch);
    }
    if observed_owner.database_now < request.minimum_database_now {
        return Err(RuntimeStartupRecoveryExecutionAcceptanceErrorV2::DatabaseClockRegressed);
    }
    let Some(available) = observed_owner.database_lease_duration() else {
        return Err(RuntimeStartupRecoveryExecutionAcceptanceErrorV2::OwnerNotCurrent);
    };
    if matches!(
        &receipt.outcome,
        RuntimeStartupRecoveryExecutionReceiptOutcomeV2::Progressed {
            action_identity,
            ..
        } if action_identity != &request.action_identity
    ) {
        return Err(RuntimeStartupRecoveryExecutionAcceptanceErrorV2::ProgressProofMismatch);
    }
    if matches!(
        &receipt.outcome,
        RuntimeStartupRecoveryExecutionReceiptOutcomeV2::RetryAfter { retry_after }
            if retry_after.is_zero() || *retry_after > available
    ) {
        return Err(RuntimeStartupRecoveryExecutionAcceptanceErrorV2::InvalidRetryAfter);
    }
    Ok(RuntimeValidatedStartupRecoveryExecutionV2 {
        operation_authority,
        request,
        owner_receipt: receipt.owner_receipt,
        outcome: receipt.outcome,
        pending_drain_proof,
        pending_registry_successor,
    })
}

pub(crate) fn accept_validated_startup_recovery_execution_v2(
    permit: &mut RuntimeClosedDrainRecoveryPermitV2,
    validated: RuntimeValidatedStartupRecoveryExecutionV2,
) -> Option<(
    RuntimeClosedRecoveryAuthorityRevisionV2,
    RuntimeAcceptedStartupRecoveryExecutionOutcomeV2,
)> {
    let RuntimeValidatedStartupRecoveryExecutionV2 {
        operation_authority,
        request,
        owner_receipt,
        outcome,
        pending_drain_proof,
        pending_registry_successor,
    } = validated;
    let database_now = owner_receipt.database_now;
    let authority_revision = match pending_registry_successor {
        Some(registry_successor) => permit.restore_after_startup_pending_drain_execution(
            operation_authority,
            request.correlation.selection_authority_revision,
            database_now,
            registry_successor,
        )?,
        None => permit.restore_after_startup_recovery_execution(
            operation_authority,
            request.class,
            request.correlation.selection_authority_revision,
            database_now,
        )?,
    };
    Some((
        authority_revision,
        RuntimeAcceptedStartupRecoveryExecutionOutcomeV2 {
            class: request.class,
            outcome,
            successor_authority_revision: authority_revision,
            owner_receipt,
            pending_drain_proof,
        },
    ))
}

fn validate_execution_request_binding_v2(
    permit: &RuntimeClosedDrainRecoveryPermitV2,
    request: &RuntimeStartupRecoveryExecutionRequestV2,
) -> Result<(), RuntimeStartupRecoveryExecutionAcceptanceErrorV2> {
    let Some(pending) = permit.pending_startup_recovery_execution() else {
        return Err(RuntimeStartupRecoveryExecutionAcceptanceErrorV2::ClassMismatch);
    };
    let correlation = &request.correlation;
    if correlation.recovery_id != *permit.recovery_id()
        || correlation.originating_emergency_generation
            != generation_value(permit.originating_emergency_generation())
        || correlation.coordinator_generation != generation_value(permit.coordinator_generation())
        || correlation.authority_revision.get() != permit.authority_revision().get()
        || correlation.selection_authority_revision
            != pending.selection_correlation().authority_revision
        || pending.selection_correlation().recovery_id != *permit.recovery_id()
        || pending
            .selection_correlation()
            .originating_emergency_generation
            != correlation.originating_emergency_generation
        || pending.selection_correlation().coordinator_generation
            != correlation.coordinator_generation
        || correlation
            .selection_authority_revision
            .get()
            .checked_add(1)
            != Some(correlation.authority_revision.get())
    {
        return Err(RuntimeStartupRecoveryExecutionAcceptanceErrorV2::CorrelationMismatch);
    }
    if request.class != pending.class() {
        return Err(RuntimeStartupRecoveryExecutionAcceptanceErrorV2::ClassMismatch);
    }
    let owner = permit.owner_receipt();
    if request.gateway_owner_lease_id != owner.lease_id
        || request.expected_owner_revision != owner.owner_revision
        || request.expected_owner_expires_at != owner.expires_at
    {
        return Err(RuntimeStartupRecoveryExecutionAcceptanceErrorV2::OwnerMismatch);
    }
    if request.minimum_database_now != minimum_database_now_v2(permit) {
        return Err(RuntimeStartupRecoveryExecutionAcceptanceErrorV2::DatabaseClockMismatch);
    }
    if request.readiness != *permit.readiness() {
        return Err(RuntimeStartupRecoveryExecutionAcceptanceErrorV2::CapabilityReadinessMismatch);
    }
    if request.paused_gateway != *permit.paused_gateway() {
        return Err(RuntimeStartupRecoveryExecutionAcceptanceErrorV2::PausedGatewayMismatch);
    }
    let registry = permit.registry_evidence().empty_observation();
    if request.registry_process_instance_id != *registry.process_instance_id()
        || request.registry_observation_sequence != registry.observation_sequence()
        || request.registry_retained_slot_count != registry.retained_slot_count()
        || request.registry_retained_empty_tombstone_count
            != registry.retained_empty_tombstone_count()
    {
        return Err(RuntimeStartupRecoveryExecutionAcceptanceErrorV2::RegistryMismatch);
    }
    Ok(())
}

fn startup_recovery_execution_request_v2(
    permit: &RuntimeClosedDrainRecoveryPermitV2,
    pending: &RuntimePendingStartupRecoveryExecutionV2,
) -> RuntimeStartupRecoveryExecutionRequestV2 {
    let owner = permit.owner_receipt();
    let registry = permit.registry_evidence().empty_observation();
    let correlation = RuntimeStartupRecoveryExecutionCorrelationV2 {
        recovery_id: permit.recovery_id().clone(),
        originating_emergency_generation: generation_value(
            permit.originating_emergency_generation(),
        ),
        coordinator_generation: generation_value(permit.coordinator_generation()),
        authority_revision: NonZeroU64::new(permit.authority_revision().get())
            .expect("closed recovery authority revision is nonzero"),
        selection_authority_revision: pending.selection_correlation().authority_revision,
    };
    RuntimeStartupRecoveryExecutionRequestV2 {
        action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2 {
            correlation: correlation.clone(),
            class: pending.class(),
        },
        correlation,
        class: pending.class(),
        gateway_owner_lease_id: owner.lease_id.clone(),
        expected_owner_revision: owner.owner_revision,
        expected_owner_expires_at: owner.expires_at,
        minimum_database_now: minimum_database_now_v2(permit),
        readiness: permit.readiness().clone(),
        paused_gateway: permit.paused_gateway().clone(),
        registry_process_instance_id: registry.process_instance_id().clone(),
        registry_observation_sequence: registry.observation_sequence(),
        registry_retained_slot_count: registry.retained_slot_count(),
        registry_retained_empty_tombstone_count: registry.retained_empty_tombstone_count(),
    }
}

fn minimum_database_now_v2(permit: &RuntimeClosedDrainRecoveryPermitV2) -> DateTime<Utc> {
    permit
        .last_startup_observation_database_now()
        .unwrap_or(permit.owner_receipt().database_now)
        .max(permit.owner_receipt().database_now)
}

fn generation_value(generation: crate::RuntimeGatewayCoordinatorGenerationV2) -> NonZeroU64 {
    NonZeroU64::new(generation.get()).expect("gateway coordinator generation is nonzero")
}
