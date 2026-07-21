use std::num::{NonZeroU32, NonZeroU64};
use std::time::Duration;

use automation_runtime_convergence::{
    ControllerId, DeploymentRevision, FencingToken, RuntimeDeployment,
    RuntimeDeploymentPhaseKindV1, RuntimeDeploymentPhaseV1, RuntimeDeploymentSnapshotV1,
    RuntimeFailureDispositionV1, RuntimePendingConditionV1,
};

use crate::{
    RuntimeCertificationReceiptV1, RuntimeCertificationRequestV1, RuntimeConvergenceMutationV1,
    RuntimeDisconnectServingV1, RuntimeExecutionGuardV1, RuntimeExecutionReceiptV1,
    RuntimeExecutionUpdateReceiptV1, RuntimeHeartbeatServingV1, RuntimeLiveMetadataV1,
    RuntimeMutationReceiptV1, RuntimeMutationRequestV1, RuntimeRenewExecutionV1,
    RuntimeServingReceiptV1, RuntimeServingUpdateReceiptV1, RuntimeSessionActionIdV1,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeConvergenceSessionStateV1 {
    Active,
    Released,
    CertifiedLive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeServingSessionStateV1 {
    Serving,
    Disconnected,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeConvergenceSessionError {
    #[error("runtime convergence session snapshot is invalid")]
    InvalidSnapshot,
    #[error("runtime convergence execution receipt is invalid")]
    InvalidExecutionReceipt,
    #[error("runtime convergence serving receipt is invalid")]
    InvalidServingReceipt,
    #[error("runtime convergence session is not active")]
    InactiveSession,
    #[error("runtime convergence session already has an action in flight")]
    ActionInFlight,
    #[error("runtime convergence session has no action in flight")]
    NoActionInFlight,
    #[error("runtime convergence action identity does not match")]
    ActionMismatch,
    #[error("runtime convergence action identity overflowed")]
    ActionSequenceOverflow,
    #[error("runtime convergence duration is invalid")]
    InvalidDuration,
    #[error("runtime convergence mutation is invalid for the current phase")]
    InvalidMutationForPhase,
    #[error("runtime convergence attempt does not match")]
    ConvergenceAttemptMismatch,
    #[error("runtime convergence controller does not match")]
    ControllerMismatch,
    #[error("runtime convergence receipt is stale")]
    StaleReceipt,
    #[error("runtime convergence receipt skipped a revision")]
    RevisionGap,
    #[error("runtime convergence fencing token did not advance exactly once")]
    FencingTokenNotAdvanced,
    #[error("runtime convergence receipt does not match the requested action")]
    ReceiptMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ExecutionActionV1 {
    Renew(RuntimeRenewExecutionV1),
    Mutate(RuntimeMutationRequestV1),
    Certify(RuntimeCertificationRequestV1),
}

impl ExecutionActionV1 {
    fn id(&self) -> RuntimeSessionActionIdV1 {
        match self {
            Self::Renew(request) => request.action_id,
            Self::Mutate(request) => request.action_id,
            Self::Certify(request) => request.action_id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ServingActionV1 {
    Heartbeat(RuntimeHeartbeatServingV1),
    Disconnect(RuntimeDisconnectServingV1),
}

impl ServingActionV1 {
    fn id(&self) -> RuntimeSessionActionIdV1 {
        match self {
            Self::Heartbeat(request) => request.action_id,
            Self::Disconnect(request) => request.action_id,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RuntimeConvergenceSessionV1 {
    snapshot: RuntimeDeploymentSnapshotV1,
    controller_id: ControllerId,
    fencing_token: FencingToken,
    convergence_attempt: NonZeroU32,
    acquired_at: chrono::DateTime<chrono::Utc>,
    expires_at: chrono::DateTime<chrono::Utc>,
    next_action_id: u64,
    in_flight: Option<ExecutionActionV1>,
    state: RuntimeConvergenceSessionStateV1,
}

impl RuntimeConvergenceSessionV1 {
    pub fn from_claim(
        receipt: RuntimeExecutionReceiptV1,
    ) -> Result<Self, RuntimeConvergenceSessionError> {
        validate_execution_receipt(&receipt)?;
        if receipt.snapshot.phase.is_terminal()
            || matches!(
                &receipt.snapshot.phase,
                RuntimeDeploymentPhaseV1::RuntimePending {
                    condition: RuntimePendingConditionV1::Blocked { .. }
                }
            )
        {
            return Err(RuntimeConvergenceSessionError::InvalidExecutionReceipt);
        }
        if matches!(
            &receipt.snapshot.phase,
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Retryable { attempt, .. }
            } if *attempt >= receipt.convergence_attempt
        ) {
            return Err(RuntimeConvergenceSessionError::ConvergenceAttemptMismatch);
        }
        Ok(Self {
            snapshot: receipt.snapshot,
            controller_id: receipt.controller_id,
            fencing_token: receipt.fencing_token,
            convergence_attempt: receipt.convergence_attempt,
            acquired_at: receipt.acquired_at,
            expires_at: receipt.expires_at,
            next_action_id: 1,
            in_flight: None,
            state: RuntimeConvergenceSessionStateV1::Active,
        })
    }

    pub fn snapshot(&self) -> &RuntimeDeploymentSnapshotV1 {
        &self.snapshot
    }

    pub fn state(&self) -> RuntimeConvergenceSessionStateV1 {
        self.state
    }

    pub fn controller_id(&self) -> &ControllerId {
        &self.controller_id
    }

    pub fn fencing_token(&self) -> FencingToken {
        self.fencing_token
    }

    pub fn convergence_attempt(&self) -> NonZeroU32 {
        self.convergence_attempt
    }

    pub fn acquired_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.acquired_at
    }

    pub fn expires_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.expires_at
    }

    pub fn in_flight_action(&self) -> Option<RuntimeSessionActionIdV1> {
        self.in_flight.as_ref().map(ExecutionActionV1::id)
    }

    pub fn execution_guard(
        &self,
    ) -> Result<RuntimeExecutionGuardV1, RuntimeConvergenceSessionError> {
        self.require_action_slot()?;
        Ok(self.guard())
    }

    pub fn current_execution_receipt(
        &self,
    ) -> Result<RuntimeExecutionReceiptV1, RuntimeConvergenceSessionError> {
        self.require_action_slot()?;
        Ok(RuntimeExecutionReceiptV1 {
            snapshot: self.snapshot.clone(),
            controller_id: self.controller_id.clone(),
            fencing_token: self.fencing_token,
            convergence_attempt: self.convergence_attempt,
            acquired_at: self.acquired_at,
            expires_at: self.expires_at,
        })
    }

    pub fn begin_renewal(
        &mut self,
        lease_for: Duration,
    ) -> Result<RuntimeRenewExecutionV1, RuntimeConvergenceSessionError> {
        self.require_action_slot()?;
        if lease_for.is_zero() {
            return Err(RuntimeConvergenceSessionError::InvalidDuration);
        }
        let request = RuntimeRenewExecutionV1 {
            action_id: self.allocate_action_id()?,
            guard: self.guard(),
            lease_for,
        };
        self.in_flight = Some(ExecutionActionV1::Renew(request.clone()));
        Ok(request)
    }

    pub fn apply_renewal(
        &mut self,
        receipt: RuntimeExecutionUpdateReceiptV1,
    ) -> Result<(), RuntimeConvergenceSessionError> {
        let request = match self.in_flight.as_ref() {
            Some(ExecutionActionV1::Renew(request)) if request.action_id == receipt.action_id => {
                request.clone()
            }
            Some(_) => return Err(RuntimeConvergenceSessionError::ActionMismatch),
            None => return Err(RuntimeConvergenceSessionError::NoActionInFlight),
        };
        self.validate_guard(&request.guard)?;
        validate_execution_receipt(&receipt.execution)?;
        validate_next_revision(self.snapshot.revision, receipt.execution.snapshot.revision)?;
        if !immutable_deployment_fields_match(&self.snapshot, &receipt.execution.snapshot)
            || !renewal_payload_matches(&self.snapshot, &receipt.execution.snapshot)
            || receipt.execution.controller_id != self.controller_id
            || receipt.execution.convergence_attempt != self.convergence_attempt
        {
            return Err(RuntimeConvergenceSessionError::ReceiptMismatch);
        }
        let expected_fence = self
            .fencing_token
            .next()
            .map_err(|_| RuntimeConvergenceSessionError::FencingTokenNotAdvanced)?;
        if receipt.execution.fencing_token != expected_fence {
            return Err(RuntimeConvergenceSessionError::FencingTokenNotAdvanced);
        }
        if receipt.execution.acquired_at < self.acquired_at
            || receipt.execution.expires_at <= self.expires_at
        {
            return Err(RuntimeConvergenceSessionError::ReceiptMismatch);
        }
        self.snapshot = receipt.execution.snapshot;
        self.fencing_token = receipt.execution.fencing_token;
        self.acquired_at = receipt.execution.acquired_at;
        self.expires_at = receipt.execution.expires_at;
        self.in_flight = None;
        Ok(())
    }

    pub fn begin_mutation(
        &mut self,
        mutation: RuntimeConvergenceMutationV1,
    ) -> Result<RuntimeMutationRequestV1, RuntimeConvergenceSessionError> {
        self.require_action_slot()?;
        validate_mutation_request(&self.snapshot, self.convergence_attempt, &mutation)?;
        let request = RuntimeMutationRequestV1 {
            action_id: self.allocate_action_id()?,
            guard: self.guard(),
            mutation,
        };
        self.in_flight = Some(ExecutionActionV1::Mutate(request.clone()));
        Ok(request)
    }

    pub fn apply_mutation(
        &mut self,
        receipt: RuntimeMutationReceiptV1,
    ) -> Result<RuntimeConvergenceSessionStateV1, RuntimeConvergenceSessionError> {
        let request = match self.in_flight.as_ref() {
            Some(ExecutionActionV1::Mutate(request)) if request.action_id == receipt.action_id => {
                request.clone()
            }
            Some(_) => return Err(RuntimeConvergenceSessionError::ActionMismatch),
            None => return Err(RuntimeConvergenceSessionError::NoActionInFlight),
        };
        self.validate_guard(&request.guard)?;
        validate_transition_receipt(
            &self.snapshot,
            self.convergence_attempt,
            &request.mutation,
            &receipt,
        )?;
        let releases = mutation_releases_execution(&request.mutation);
        if releases {
            if receipt.snapshot.controller_lease.is_some()
                || receipt.snapshot.last_fencing_token != Some(self.fencing_token)
            {
                return Err(RuntimeConvergenceSessionError::ReceiptMismatch);
            }
        } else if receipt.snapshot.controller_lease != self.snapshot.controller_lease
            || receipt.snapshot.last_fencing_token != self.snapshot.last_fencing_token
        {
            return Err(RuntimeConvergenceSessionError::ReceiptMismatch);
        }
        self.snapshot = receipt.snapshot;
        self.in_flight = None;
        if releases {
            self.state = RuntimeConvergenceSessionStateV1::Released;
        }
        Ok(self.state)
    }

    pub fn begin_certification(
        &mut self,
        gateway_ready: automation_runtime_convergence::GatewayReadyAttestationV1,
        metadata: RuntimeLiveMetadataV1,
        serving_lease_for: Duration,
    ) -> Result<RuntimeCertificationRequestV1, RuntimeConvergenceSessionError> {
        self.require_action_slot()?;
        if serving_lease_for.is_zero()
            || !matches!(
                &self.snapshot.phase,
                RuntimeDeploymentPhaseV1::AwaitingGatewayReady
            )
            || gateway_ready.target != self.snapshot.target
            || gateway_ready.runtime_generation != self.snapshot.runtime_generation
            || self
                .snapshot
                .panel_certificate
                .as_ref()
                .is_none_or(|panel| panel.process_instance_id != gateway_ready.process_instance_id)
        {
            return Err(RuntimeConvergenceSessionError::InvalidMutationForPhase);
        }
        let request = RuntimeCertificationRequestV1 {
            action_id: self.allocate_action_id()?,
            guard: self.guard(),
            gateway_ready,
            metadata,
            serving_lease_for,
        };
        self.in_flight = Some(ExecutionActionV1::Certify(request.clone()));
        Ok(request)
    }

    pub fn apply_certification(
        &mut self,
        receipt: RuntimeCertificationReceiptV1,
    ) -> Result<RuntimeServingSessionV1, RuntimeConvergenceSessionError> {
        let request = match self.in_flight.as_ref() {
            Some(ExecutionActionV1::Certify(request)) if request.action_id == receipt.action_id => {
                request.clone()
            }
            Some(_) => return Err(RuntimeConvergenceSessionError::ActionMismatch),
            None => return Err(RuntimeConvergenceSessionError::NoActionInFlight),
        };
        self.validate_guard(&request.guard)?;
        validate_next_revision(self.snapshot.revision, receipt.snapshot.revision)?;
        if receipt.outcome.revision() != receipt.snapshot.revision
            || receipt.convergence_attempt != self.convergence_attempt
            || receipt.metadata != request.metadata
            || !immutable_deployment_fields_match(&self.snapshot, &receipt.snapshot)
        {
            return Err(RuntimeConvergenceSessionError::ReceiptMismatch);
        }
        validate_snapshot(&receipt.snapshot)?;
        let live = match &receipt.snapshot.phase {
            RuntimeDeploymentPhaseV1::Live => receipt
                .snapshot
                .live
                .as_ref()
                .ok_or(RuntimeConvergenceSessionError::ReceiptMismatch)?,
            _ => return Err(RuntimeConvergenceSessionError::ReceiptMismatch),
        };
        if receipt.snapshot.preflight != self.snapshot.preflight
            || receipt.snapshot.drain != self.snapshot.drain
            || receipt.snapshot.activation != self.snapshot.activation
            || receipt.snapshot.panel_certificate != self.snapshot.panel_certificate
            || receipt.snapshot.gateway_ready.as_ref() != Some(&request.gateway_ready)
            || live.gateway_ready != request.gateway_ready
            || receipt.snapshot.last_live_recovery != self.snapshot.last_live_recovery
            || receipt.snapshot.last_runtime_failure != self.snapshot.last_runtime_failure
            || receipt.snapshot.controller_lease.is_some()
            || receipt.snapshot.last_fencing_token != Some(self.fencing_token)
        {
            return Err(RuntimeConvergenceSessionError::ReceiptMismatch);
        }
        validate_serving_receipt(&receipt.snapshot, &receipt.serving, true)?;
        self.snapshot = receipt.snapshot.clone();
        self.in_flight = None;
        self.state = RuntimeConvergenceSessionStateV1::CertifiedLive;
        RuntimeServingSessionV1::restore(receipt.snapshot, receipt.serving)
    }

    pub fn abort_action(
        &mut self,
        action_id: RuntimeSessionActionIdV1,
    ) -> Result<(), RuntimeConvergenceSessionError> {
        match self.in_flight.as_ref() {
            Some(action) if action.id() == action_id => {
                self.in_flight = None;
                Ok(())
            }
            Some(_) => Err(RuntimeConvergenceSessionError::ActionMismatch),
            None => Err(RuntimeConvergenceSessionError::NoActionInFlight),
        }
    }

    fn guard(&self) -> RuntimeExecutionGuardV1 {
        RuntimeExecutionGuardV1 {
            scope: crate::RuntimeDeploymentScopeV1::from_identity(&self.snapshot.identity),
            expected_revision: self.snapshot.revision,
            controller_id: self.controller_id.clone(),
            fencing_token: self.fencing_token,
            runtime_generation: self.snapshot.runtime_generation,
            convergence_attempt: self.convergence_attempt,
        }
    }

    fn validate_guard(
        &self,
        guard: &RuntimeExecutionGuardV1,
    ) -> Result<(), RuntimeConvergenceSessionError> {
        if !guard.scope.matches(&self.snapshot.identity)
            || guard.expected_revision != self.snapshot.revision
            || guard.runtime_generation != self.snapshot.runtime_generation
            || guard.convergence_attempt != self.convergence_attempt
        {
            return Err(RuntimeConvergenceSessionError::ReceiptMismatch);
        }
        if guard.controller_id != self.controller_id || guard.fencing_token != self.fencing_token {
            return Err(RuntimeConvergenceSessionError::ControllerMismatch);
        }
        Ok(())
    }

    fn require_action_slot(&self) -> Result<(), RuntimeConvergenceSessionError> {
        if self.state != RuntimeConvergenceSessionStateV1::Active {
            return Err(RuntimeConvergenceSessionError::InactiveSession);
        }
        if self.in_flight.is_some() {
            return Err(RuntimeConvergenceSessionError::ActionInFlight);
        }
        Ok(())
    }

    fn allocate_action_id(
        &mut self,
    ) -> Result<RuntimeSessionActionIdV1, RuntimeConvergenceSessionError> {
        let value = NonZeroU64::new(self.next_action_id)
            .ok_or(RuntimeConvergenceSessionError::ActionSequenceOverflow)?;
        self.next_action_id = self
            .next_action_id
            .checked_add(1)
            .ok_or(RuntimeConvergenceSessionError::ActionSequenceOverflow)?;
        Ok(RuntimeSessionActionIdV1::new(value))
    }
}

#[derive(Clone, Debug)]
pub struct RuntimeServingSessionV1 {
    snapshot: RuntimeDeploymentSnapshotV1,
    ownership: RuntimeServingReceiptV1,
    next_action_id: u64,
    in_flight: Option<ServingActionV1>,
    state: RuntimeServingSessionStateV1,
}

impl RuntimeServingSessionV1 {
    pub fn restore(
        snapshot: RuntimeDeploymentSnapshotV1,
        ownership: RuntimeServingReceiptV1,
    ) -> Result<Self, RuntimeConvergenceSessionError> {
        validate_snapshot(&snapshot)?;
        validate_serving_receipt(&snapshot, &ownership, false)?;
        let state = if ownership.connected {
            RuntimeServingSessionStateV1::Serving
        } else {
            RuntimeServingSessionStateV1::Disconnected
        };
        Ok(Self {
            snapshot,
            ownership,
            next_action_id: 1,
            in_flight: None,
            state,
        })
    }

    pub fn snapshot(&self) -> &RuntimeDeploymentSnapshotV1 {
        &self.snapshot
    }

    pub fn ownership(&self) -> &RuntimeServingReceiptV1 {
        &self.ownership
    }

    pub fn state(&self) -> RuntimeServingSessionStateV1 {
        self.state
    }

    pub fn in_flight_action(&self) -> Option<RuntimeSessionActionIdV1> {
        self.in_flight.as_ref().map(ServingActionV1::id)
    }

    pub fn begin_heartbeat(
        &mut self,
        lease_for: Duration,
    ) -> Result<RuntimeHeartbeatServingV1, RuntimeConvergenceSessionError> {
        self.require_action_slot()?;
        if lease_for.is_zero() {
            return Err(RuntimeConvergenceSessionError::InvalidDuration);
        }
        let request = RuntimeHeartbeatServingV1 {
            action_id: self.allocate_action_id()?,
            identity: self.ownership.identity.clone(),
            lease_for,
        };
        self.in_flight = Some(ServingActionV1::Heartbeat(request.clone()));
        Ok(request)
    }

    pub fn apply_heartbeat(
        &mut self,
        receipt: RuntimeServingUpdateReceiptV1,
    ) -> Result<(), RuntimeConvergenceSessionError> {
        let request = match self.in_flight.as_ref() {
            Some(ServingActionV1::Heartbeat(request)) if request.action_id == receipt.action_id => {
                request.clone()
            }
            Some(_) => return Err(RuntimeConvergenceSessionError::ActionMismatch),
            None => return Err(RuntimeConvergenceSessionError::NoActionInFlight),
        };
        if request.identity != self.ownership.identity {
            return Err(RuntimeConvergenceSessionError::ReceiptMismatch);
        }
        validate_serving_advance(&self.snapshot, &self.ownership, &receipt.serving)?;
        if !receipt.serving.connected
            || !receipt.serving.serving
            || receipt.serving.last_heartbeat_at <= self.ownership.last_heartbeat_at
            || receipt.serving.expires_at < self.ownership.expires_at
        {
            return Err(RuntimeConvergenceSessionError::ReceiptMismatch);
        }
        self.ownership = receipt.serving;
        self.in_flight = None;
        Ok(())
    }

    pub fn begin_disconnect(
        &mut self,
    ) -> Result<RuntimeDisconnectServingV1, RuntimeConvergenceSessionError> {
        self.require_action_slot()?;
        let request = RuntimeDisconnectServingV1 {
            action_id: self.allocate_action_id()?,
            identity: self.ownership.identity.clone(),
        };
        self.in_flight = Some(ServingActionV1::Disconnect(request.clone()));
        Ok(request)
    }

    pub fn apply_disconnect(
        &mut self,
        receipt: RuntimeServingUpdateReceiptV1,
    ) -> Result<(), RuntimeConvergenceSessionError> {
        let request = match self.in_flight.as_ref() {
            Some(ServingActionV1::Disconnect(request))
                if request.action_id == receipt.action_id =>
            {
                request.clone()
            }
            Some(_) => return Err(RuntimeConvergenceSessionError::ActionMismatch),
            None => return Err(RuntimeConvergenceSessionError::NoActionInFlight),
        };
        if request.identity != self.ownership.identity {
            return Err(RuntimeConvergenceSessionError::ReceiptMismatch);
        }
        validate_serving_advance(&self.snapshot, &self.ownership, &receipt.serving)?;
        if receipt.serving.connected
            || receipt.serving.serving
            || receipt.serving.last_heartbeat_at < self.ownership.last_heartbeat_at
            || receipt.serving.expires_at != receipt.serving.last_heartbeat_at
        {
            return Err(RuntimeConvergenceSessionError::ReceiptMismatch);
        }
        self.ownership = receipt.serving;
        self.in_flight = None;
        self.state = RuntimeServingSessionStateV1::Disconnected;
        Ok(())
    }

    pub fn abort_action(
        &mut self,
        action_id: RuntimeSessionActionIdV1,
    ) -> Result<(), RuntimeConvergenceSessionError> {
        match self.in_flight.as_ref() {
            Some(action) if action.id() == action_id => {
                self.in_flight = None;
                Ok(())
            }
            Some(_) => Err(RuntimeConvergenceSessionError::ActionMismatch),
            None => Err(RuntimeConvergenceSessionError::NoActionInFlight),
        }
    }

    fn require_action_slot(&self) -> Result<(), RuntimeConvergenceSessionError> {
        if self.state != RuntimeServingSessionStateV1::Serving {
            return Err(RuntimeConvergenceSessionError::InactiveSession);
        }
        if self.in_flight.is_some() {
            return Err(RuntimeConvergenceSessionError::ActionInFlight);
        }
        Ok(())
    }

    fn allocate_action_id(
        &mut self,
    ) -> Result<RuntimeSessionActionIdV1, RuntimeConvergenceSessionError> {
        let value = NonZeroU64::new(self.next_action_id)
            .ok_or(RuntimeConvergenceSessionError::ActionSequenceOverflow)?;
        self.next_action_id = self
            .next_action_id
            .checked_add(1)
            .ok_or(RuntimeConvergenceSessionError::ActionSequenceOverflow)?;
        Ok(RuntimeSessionActionIdV1::new(value))
    }
}

fn validate_execution_receipt(
    receipt: &RuntimeExecutionReceiptV1,
) -> Result<(), RuntimeConvergenceSessionError> {
    validate_snapshot(&receipt.snapshot)?;
    let lease = receipt
        .snapshot
        .controller_lease
        .as_ref()
        .ok_or(RuntimeConvergenceSessionError::InvalidExecutionReceipt)?;
    if lease.controller_id != receipt.controller_id
        || lease.fencing_token != receipt.fencing_token
        || lease.acquired_at != receipt.acquired_at
        || lease.expires_at != receipt.expires_at
        || receipt.snapshot.last_fencing_token != Some(receipt.fencing_token)
        || receipt.expires_at <= receipt.acquired_at
    {
        return Err(RuntimeConvergenceSessionError::InvalidExecutionReceipt);
    }
    Ok(())
}

fn validate_snapshot(
    snapshot: &RuntimeDeploymentSnapshotV1,
) -> Result<(), RuntimeConvergenceSessionError> {
    RuntimeDeployment::restore(snapshot.clone())
        .map(|_| ())
        .map_err(|_| RuntimeConvergenceSessionError::InvalidSnapshot)
}

fn validate_next_revision(
    current: DeploymentRevision,
    next: DeploymentRevision,
) -> Result<(), RuntimeConvergenceSessionError> {
    if next <= current {
        return Err(RuntimeConvergenceSessionError::StaleReceipt);
    }
    let expected = current
        .next()
        .map_err(|_| RuntimeConvergenceSessionError::RevisionGap)?;
    if next != expected {
        return Err(RuntimeConvergenceSessionError::RevisionGap);
    }
    Ok(())
}

fn immutable_deployment_fields_match(
    current: &RuntimeDeploymentSnapshotV1,
    next: &RuntimeDeploymentSnapshotV1,
) -> bool {
    current.identity == next.identity
        && current.target == next.target
        && current.runtime_generation == next.runtime_generation
        && current.previous_runtime == next.previous_runtime
        && current.requested_at == next.requested_at
}

fn renewal_payload_matches(
    current: &RuntimeDeploymentSnapshotV1,
    next: &RuntimeDeploymentSnapshotV1,
) -> bool {
    current.phase == next.phase
        && current.preflight == next.preflight
        && current.drain == next.drain
        && current.activation == next.activation
        && current.panel_certificate == next.panel_certificate
        && current.gateway_ready == next.gateway_ready
        && current.live == next.live
        && current.last_live_recovery == next.last_live_recovery
        && current.last_runtime_failure == next.last_runtime_failure
}

fn validate_mutation_request(
    snapshot: &RuntimeDeploymentSnapshotV1,
    convergence_attempt: NonZeroU32,
    mutation: &RuntimeConvergenceMutationV1,
) -> Result<(), RuntimeConvergenceSessionError> {
    let allowed = match (snapshot.phase.kind(), mutation) {
        (
            RuntimeDeploymentPhaseKindV1::Requested,
            RuntimeConvergenceMutationV1::AcceptPreflight(attestation),
        ) => {
            attestation.target == snapshot.target
                && attestation.runtime_generation == snapshot.runtime_generation
                && attestation.observed_runtime == snapshot.previous_runtime
        }
        (
            RuntimeDeploymentPhaseKindV1::PreflightReady,
            RuntimeConvergenceMutationV1::RequestDrain,
        ) => true,
        (
            RuntimeDeploymentPhaseKindV1::DrainRequested,
            RuntimeConvergenceMutationV1::AcceptDrain(attestation),
        ) => {
            attestation.previous_runtime == snapshot.previous_runtime
                && attestation.target_runtime_generation == snapshot.runtime_generation
        }
        (RuntimeDeploymentPhaseKindV1::Drained, RuntimeConvergenceMutationV1::BeginActivation) => {
            true
        }
        (
            RuntimeDeploymentPhaseKindV1::ActivationApplying,
            RuntimeConvergenceMutationV1::AcceptActivation(attestation),
        ) => {
            attestation.activation_request_id == snapshot.identity.activation_request_id
                && attestation.target == snapshot.target
                && attestation.runtime_generation == snapshot.runtime_generation
        }
        (
            RuntimeDeploymentPhaseKindV1::RuntimePending,
            RuntimeConvergenceMutationV1::ResumeRuntimePending,
        ) => matches!(
            &snapshot.phase,
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Retryable { .. }
            }
        ),
        (
            RuntimeDeploymentPhaseKindV1::RuntimePending,
            RuntimeConvergenceMutationV1::BeginPanelReconciliation,
        ) => matches!(
            &snapshot.phase,
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Ready
            }
        ),
        (
            RuntimeDeploymentPhaseKindV1::ReconcilingPanels,
            RuntimeConvergenceMutationV1::AcceptPanelCertificate(certificate),
        ) => {
            certificate.target == snapshot.target
                && certificate.runtime_generation == snapshot.runtime_generation
        }
        (
            RuntimeDeploymentPhaseKindV1::RuntimePending
            | RuntimeDeploymentPhaseKindV1::ReconcilingPanels
            | RuntimeDeploymentPhaseKindV1::AwaitingGatewayReady,
            RuntimeConvergenceMutationV1::RecordRetryableFailure {
                code,
                attempt,
                retry_after,
                ..
            },
        ) => {
            convergence_attempt == *attempt
                && !retry_after.is_zero()
                && valid_failure_code(code)
                && !matches!(
                    &snapshot.phase,
                    RuntimeDeploymentPhaseV1::RuntimePending {
                        condition: RuntimePendingConditionV1::Retryable { .. }
                            | RuntimePendingConditionV1::Blocked { .. }
                    }
                )
        }
        (
            RuntimeDeploymentPhaseKindV1::RuntimePending
            | RuntimeDeploymentPhaseKindV1::ReconcilingPanels
            | RuntimeDeploymentPhaseKindV1::AwaitingGatewayReady,
            RuntimeConvergenceMutationV1::RecordBlockedFailure { code, .. },
        ) => {
            valid_failure_code(code)
                && !matches!(
                    &snapshot.phase,
                    RuntimeDeploymentPhaseV1::RuntimePending {
                        condition: RuntimePendingConditionV1::Retryable { .. }
                            | RuntimePendingConditionV1::Blocked { .. }
                    }
                )
        }
        (
            RuntimeDeploymentPhaseKindV1::Requested
            | RuntimeDeploymentPhaseKindV1::PreflightReady
            | RuntimeDeploymentPhaseKindV1::DrainRequested
            | RuntimeDeploymentPhaseKindV1::Drained
            | RuntimeDeploymentPhaseKindV1::ActivationApplying
            | RuntimeDeploymentPhaseKindV1::RuntimePending
            | RuntimeDeploymentPhaseKindV1::ReconcilingPanels
            | RuntimeDeploymentPhaseKindV1::AwaitingGatewayReady,
            RuntimeConvergenceMutationV1::Supersede { .. },
        ) => true,
        (
            RuntimeDeploymentPhaseKindV1::Requested
            | RuntimeDeploymentPhaseKindV1::PreflightReady
            | RuntimeDeploymentPhaseKindV1::DrainRequested,
            RuntimeConvergenceMutationV1::Cancel { .. },
        ) => true,
        _ => false,
    };
    if allowed {
        Ok(())
    } else {
        Err(RuntimeConvergenceSessionError::InvalidMutationForPhase)
    }
}

fn valid_failure_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 64
        && code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn mutation_releases_execution(mutation: &RuntimeConvergenceMutationV1) -> bool {
    matches!(
        mutation,
        RuntimeConvergenceMutationV1::RecordRetryableFailure { .. }
            | RuntimeConvergenceMutationV1::RecordBlockedFailure { .. }
            | RuntimeConvergenceMutationV1::Supersede { .. }
            | RuntimeConvergenceMutationV1::Cancel { .. }
    )
}

fn validate_transition_receipt(
    current: &RuntimeDeploymentSnapshotV1,
    convergence_attempt: NonZeroU32,
    mutation: &RuntimeConvergenceMutationV1,
    receipt: &RuntimeMutationReceiptV1,
) -> Result<(), RuntimeConvergenceSessionError> {
    validate_next_revision(current.revision, receipt.snapshot.revision)?;
    if receipt.outcome.revision() != receipt.snapshot.revision
        || receipt.convergence_attempt != convergence_attempt
        || !immutable_deployment_fields_match(current, &receipt.snapshot)
    {
        return Err(RuntimeConvergenceSessionError::ReceiptMismatch);
    }
    validate_snapshot(&receipt.snapshot)?;
    if mutation_receipt_matches(current, &receipt.snapshot, mutation)? {
        Ok(())
    } else {
        Err(RuntimeConvergenceSessionError::ReceiptMismatch)
    }
}

fn mutation_receipt_matches(
    current: &RuntimeDeploymentSnapshotV1,
    next: &RuntimeDeploymentSnapshotV1,
    mutation: &RuntimeConvergenceMutationV1,
) -> Result<bool, RuntimeConvergenceSessionError> {
    let evidence_unchanged = || all_evidence_match(current, next);
    let stable_runtime_evidence = || {
        current.preflight == next.preflight
            && current.drain == next.drain
            && current.activation == next.activation
            && current.last_live_recovery == next.last_live_recovery
    };
    let matches = match mutation {
        RuntimeConvergenceMutationV1::AcceptPreflight(attestation) => {
            matches!(next.phase, RuntimeDeploymentPhaseV1::PreflightReady)
                && next.preflight.as_ref() == Some(attestation)
                && current.drain == next.drain
                && current.activation == next.activation
                && current.panel_certificate == next.panel_certificate
                && current.gateway_ready == next.gateway_ready
                && current.live == next.live
                && current.last_live_recovery == next.last_live_recovery
                && current.last_runtime_failure == next.last_runtime_failure
        }
        RuntimeConvergenceMutationV1::RequestDrain => {
            matches!(next.phase, RuntimeDeploymentPhaseV1::DrainRequested) && evidence_unchanged()
        }
        RuntimeConvergenceMutationV1::AcceptDrain(attestation) => {
            matches!(next.phase, RuntimeDeploymentPhaseV1::Drained)
                && current.preflight == next.preflight
                && next.drain.as_ref() == Some(attestation)
                && current.activation == next.activation
                && current.panel_certificate == next.panel_certificate
                && current.gateway_ready == next.gateway_ready
                && current.live == next.live
                && current.last_live_recovery == next.last_live_recovery
                && current.last_runtime_failure == next.last_runtime_failure
        }
        RuntimeConvergenceMutationV1::BeginActivation => {
            matches!(next.phase, RuntimeDeploymentPhaseV1::ActivationApplying)
                && evidence_unchanged()
        }
        RuntimeConvergenceMutationV1::AcceptActivation(attestation) => {
            matches!(
                next.phase,
                RuntimeDeploymentPhaseV1::RuntimePending {
                    condition: RuntimePendingConditionV1::Ready
                }
            ) && current.preflight == next.preflight
                && current.drain == next.drain
                && next.activation.as_ref() == Some(attestation)
                && current.panel_certificate == next.panel_certificate
                && current.gateway_ready == next.gateway_ready
                && current.live == next.live
                && current.last_live_recovery == next.last_live_recovery
                && current.last_runtime_failure == next.last_runtime_failure
        }
        RuntimeConvergenceMutationV1::RecordRetryableFailure {
            failure_id,
            kind,
            code,
            attempt,
            retry_after,
        } => {
            let condition = match &next.phase {
                RuntimeDeploymentPhaseV1::RuntimePending {
                    condition:
                        RuntimePendingConditionV1::Retryable {
                            failure,
                            attempt: actual_attempt,
                            retry_not_before,
                        },
                } => Some((failure, actual_attempt, retry_not_before)),
                _ => None,
            };
            let delay = chrono::TimeDelta::from_std(*retry_after)
                .map_err(|_| RuntimeConvergenceSessionError::ReceiptMismatch)?;
            condition.is_some_and(|(failure, actual_attempt, retry_not_before)| {
                failure.failure_id == *failure_id
                    && failure.kind == *kind
                    && failure.code == *code
                    && *actual_attempt == *attempt
                    && *retry_not_before == failure.recorded_at + delay
                    && next.last_runtime_failure
                        == Some(RuntimeFailureDispositionV1::Retryable {
                            failure: failure.clone(),
                            attempt: *actual_attempt,
                            retry_not_before: *retry_not_before,
                        })
            }) && stable_runtime_evidence()
                && next.panel_certificate.is_none()
                && next.gateway_ready.is_none()
                && next.live.is_none()
        }
        RuntimeConvergenceMutationV1::RecordBlockedFailure {
            failure_id,
            kind,
            code,
        } => {
            let failure = match &next.phase {
                RuntimeDeploymentPhaseV1::RuntimePending {
                    condition: RuntimePendingConditionV1::Blocked { failure },
                } => Some(failure),
                _ => None,
            };
            failure.is_some_and(|failure| {
                failure.failure_id == *failure_id
                    && failure.kind == *kind
                    && failure.code == *code
                    && next.last_runtime_failure
                        == Some(RuntimeFailureDispositionV1::Blocked {
                            failure: failure.clone(),
                        })
            }) && stable_runtime_evidence()
                && next.panel_certificate.is_none()
                && next.gateway_ready.is_none()
                && next.live.is_none()
        }
        RuntimeConvergenceMutationV1::ResumeRuntimePending => {
            matches!(
                next.phase,
                RuntimeDeploymentPhaseV1::RuntimePending {
                    condition: RuntimePendingConditionV1::Ready
                }
            ) && evidence_unchanged()
        }
        RuntimeConvergenceMutationV1::BeginPanelReconciliation => {
            matches!(next.phase, RuntimeDeploymentPhaseV1::ReconcilingPanels)
                && evidence_unchanged()
        }
        RuntimeConvergenceMutationV1::AcceptPanelCertificate(certificate) => {
            matches!(next.phase, RuntimeDeploymentPhaseV1::AwaitingGatewayReady)
                && current.preflight == next.preflight
                && current.drain == next.drain
                && current.activation == next.activation
                && next.panel_certificate.as_ref() == Some(certificate)
                && current.gateway_ready == next.gateway_ready
                && current.live == next.live
                && current.last_live_recovery == next.last_live_recovery
                && current.last_runtime_failure == next.last_runtime_failure
        }
        RuntimeConvergenceMutationV1::Supersede { by, reason } => {
            matches!(
                &next.phase,
                RuntimeDeploymentPhaseV1::Superseded {
                    by: actual_by,
                    reason: actual_reason,
                    ..
                } if actual_by == by && actual_reason == reason
            ) && evidence_unchanged()
        }
        RuntimeConvergenceMutationV1::Cancel { reason } => {
            matches!(
                &next.phase,
                RuntimeDeploymentPhaseV1::Cancelled {
                    reason: actual_reason,
                    ..
                } if actual_reason == reason
            ) && evidence_unchanged()
        }
    };
    Ok(matches)
}

fn all_evidence_match(
    current: &RuntimeDeploymentSnapshotV1,
    next: &RuntimeDeploymentSnapshotV1,
) -> bool {
    current.preflight == next.preflight
        && current.drain == next.drain
        && current.activation == next.activation
        && current.panel_certificate == next.panel_certificate
        && current.gateway_ready == next.gateway_ready
        && current.live == next.live
        && current.last_live_recovery == next.last_live_recovery
        && current.last_runtime_failure == next.last_runtime_failure
}

fn validate_serving_receipt(
    snapshot: &RuntimeDeploymentSnapshotV1,
    serving: &RuntimeServingReceiptV1,
    require_fresh: bool,
) -> Result<(), RuntimeConvergenceSessionError> {
    let live = match &snapshot.phase {
        RuntimeDeploymentPhaseV1::Live => snapshot
            .live
            .as_ref()
            .ok_or(RuntimeConvergenceSessionError::InvalidServingReceipt)?,
        _ => return Err(RuntimeConvergenceSessionError::InvalidServingReceipt),
    };
    if !serving.identity.scope.matches(&snapshot.identity)
        || serving.identity.process_instance_id != live.process_instance_id
        || serving.identity.runtime_generation != serving.runtime_generation
        || serving.runtime_generation != live.runtime_generation
        || serving.acquired_at != live.certified_at
        || serving.acquired_at > serving.last_heartbeat_at
        || serving.last_heartbeat_at > serving.expires_at
        || serving.connected != serving.serving
        || (serving.connected && serving.expires_at <= serving.last_heartbeat_at)
        || (!serving.connected && serving.expires_at != serving.last_heartbeat_at)
        || (require_fresh
            && (!serving.connected
                || serving.last_heartbeat_at != serving.acquired_at
                || serving.expires_at <= serving.acquired_at))
    {
        return Err(RuntimeConvergenceSessionError::InvalidServingReceipt);
    }
    Ok(())
}

fn validate_serving_advance(
    snapshot: &RuntimeDeploymentSnapshotV1,
    current: &RuntimeServingReceiptV1,
    next: &RuntimeServingReceiptV1,
) -> Result<(), RuntimeConvergenceSessionError> {
    validate_serving_receipt(snapshot, next, false)?;
    let current_revision = current.identity.expected_revision.get();
    let next_revision = next.identity.expected_revision.get();
    if next_revision <= current_revision {
        return Err(RuntimeConvergenceSessionError::StaleReceipt);
    }
    if next_revision
        != current_revision
            .checked_add(1)
            .ok_or(RuntimeConvergenceSessionError::RevisionGap)?
    {
        return Err(RuntimeConvergenceSessionError::RevisionGap);
    }
    if current.identity.scope != next.identity.scope
        || current.identity.attestation_id != next.identity.attestation_id
        || current.identity.process_instance_id != next.identity.process_instance_id
        || current.identity.lease_epoch != next.identity.lease_epoch
        || current.runtime_generation != next.runtime_generation
        || current.acquired_at != next.acquired_at
    {
        return Err(RuntimeConvergenceSessionError::ReceiptMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU32, NonZeroU64};

    use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
    use automation_runtime_convergence::{
        ActivationAttestationV1, ActivationOutcomeKindV1, ActivationRequestId, BindingRevision,
        CommandGuardV1, ControllerId, DeploymentId, DrainAttestationV1, FencingToken,
        GatewayReadyAttestationV1, GatewayReadyKindV1, InstallationId, LeaseRequestV1,
        PanelCertificateId, PanelCertificateV1, PreflightAttestationV1, ProcessInstanceId,
        PromotionId, RuntimeDeployment, RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1,
        RuntimeGeneration, TenantId,
    };
    use chrono::{DateTime, Utc};
    use discord_model::GuildId;
    use resource_resolution::ResourceBindingFingerprint;

    use super::*;
    use crate::{
        GatewayShardIdV1, PanelReportDigestV1, RuntimeAttestationIdV1, RuntimeBuildRevisionV1,
        RuntimeDeploymentScopeV1, RuntimeServingIdentityV1,
    };

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(second, 0).unwrap()
    }

    fn target() -> RuntimeDeploymentTargetV1 {
        RuntimeDeploymentTargetV1 {
            guild_id: GuildId(1),
            ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
            version: RuleSetVersionId::FIRST,
            content_hash: RuleSetContentHash::parse_hex(&"2".repeat(64)).unwrap(),
            binding_revision: BindingRevision::FIRST,
            binding_fingerprint: ResourceBindingFingerprint::parse(&"3".repeat(64)).unwrap(),
        }
    }

    fn claimed() -> RuntimeExecutionReceiptV1 {
        let mut deployment = RuntimeDeployment::request(
            RuntimeDeploymentIdentityV1 {
                deployment_id: DeploymentId::parse("deployment").unwrap(),
                tenant_id: TenantId::parse("tenant").unwrap(),
                installation_id: InstallationId::parse("installation").unwrap(),
                promotion_id: PromotionId::parse("1".repeat(64)).unwrap(),
                activation_request_id: ActivationRequestId::parse("activation").unwrap(),
            },
            target(),
            RuntimeGeneration::FIRST,
            None,
            at(1),
        )
        .unwrap();
        let controller_id = ControllerId::parse("controller").unwrap();
        let fencing_token = FencingToken::FIRST;
        deployment
            .acquire_lease(LeaseRequestV1 {
                expected_revision: deployment.revision(),
                controller_id: controller_id.clone(),
                fencing_token,
                now: at(10),
                expires_at: at(100),
            })
            .unwrap();
        RuntimeExecutionReceiptV1 {
            snapshot: deployment.snapshot(),
            controller_id,
            fencing_token,
            convergence_attempt: NonZeroU32::new(1).unwrap(),
            acquired_at: at(10),
            expires_at: at(100),
        }
    }

    fn guard(snapshot: &RuntimeDeploymentSnapshotV1) -> CommandGuardV1 {
        let lease = snapshot.controller_lease.as_ref().unwrap();
        CommandGuardV1 {
            expected_revision: snapshot.revision,
            controller_id: lease.controller_id.clone(),
            fencing_token: lease.fencing_token,
            runtime_generation: snapshot.runtime_generation,
            now: at(20),
        }
    }

    fn mutation_receipt(
        request: &RuntimeMutationRequestV1,
        snapshot: &RuntimeDeploymentSnapshotV1,
    ) -> RuntimeMutationReceiptV1 {
        let mut deployment = RuntimeDeployment::restore(snapshot.clone()).unwrap();
        let command_guard = guard(snapshot);
        let outcome = match &request.mutation {
            RuntimeConvergenceMutationV1::AcceptPreflight(attestation) => deployment
                .accept_preflight(&command_guard, attestation.clone())
                .unwrap(),
            RuntimeConvergenceMutationV1::RequestDrain => {
                deployment.request_drain(&command_guard).unwrap()
            }
            RuntimeConvergenceMutationV1::AcceptDrain(attestation) => deployment
                .accept_drain(&command_guard, attestation.clone())
                .unwrap(),
            RuntimeConvergenceMutationV1::BeginActivation => {
                deployment.begin_activation(&command_guard).unwrap()
            }
            RuntimeConvergenceMutationV1::AcceptActivation(attestation) => deployment
                .accept_activation(&command_guard, attestation.clone())
                .unwrap(),
            RuntimeConvergenceMutationV1::ResumeRuntimePending => {
                deployment.resume_runtime_pending(&command_guard).unwrap()
            }
            RuntimeConvergenceMutationV1::BeginPanelReconciliation => deployment
                .begin_panel_reconciliation(&command_guard)
                .unwrap(),
            RuntimeConvergenceMutationV1::AcceptPanelCertificate(certificate) => deployment
                .accept_panel_certificate(&command_guard, certificate.clone())
                .unwrap(),
            _ => panic!("unsupported test mutation"),
        };
        RuntimeMutationReceiptV1 {
            action_id: request.action_id,
            outcome,
            snapshot: deployment.snapshot(),
            convergence_attempt: request.guard.convergence_attempt,
        }
    }

    fn apply(session: &mut RuntimeConvergenceSessionV1, mutation: RuntimeConvergenceMutationV1) {
        let before = session.snapshot().clone();
        let request = session.begin_mutation(mutation).unwrap();
        let receipt = mutation_receipt(&request, &before);
        session.apply_mutation(receipt).unwrap();
    }

    fn advance_to_awaiting(session: &mut RuntimeConvergenceSessionV1) -> ProcessInstanceId {
        apply(
            session,
            RuntimeConvergenceMutationV1::AcceptPreflight(PreflightAttestationV1 {
                target: target(),
                runtime_generation: RuntimeGeneration::FIRST,
                observed_runtime: None,
                checked_at: at(11),
            }),
        );
        apply(session, RuntimeConvergenceMutationV1::RequestDrain);
        apply(
            session,
            RuntimeConvergenceMutationV1::AcceptDrain(DrainAttestationV1 {
                previous_runtime: None,
                target_runtime_generation: RuntimeGeneration::FIRST,
                drained_at: at(12),
            }),
        );
        apply(session, RuntimeConvergenceMutationV1::BeginActivation);
        apply(
            session,
            RuntimeConvergenceMutationV1::AcceptActivation(ActivationAttestationV1 {
                activation_request_id: ActivationRequestId::parse("activation").unwrap(),
                target: target(),
                runtime_generation: RuntimeGeneration::FIRST,
                kind: ActivationOutcomeKindV1::AlreadyActive,
                activated_at: at(13),
            }),
        );
        apply(
            session,
            RuntimeConvergenceMutationV1::BeginPanelReconciliation,
        );
        let process = ProcessInstanceId::parse("process").unwrap();
        apply(
            session,
            RuntimeConvergenceMutationV1::AcceptPanelCertificate(PanelCertificateV1 {
                certificate_id: PanelCertificateId::parse("panel").unwrap(),
                target: target(),
                runtime_generation: RuntimeGeneration::FIRST,
                process_instance_id: process.clone(),
                declared_count: 0,
                installed_count: 0,
                unchanged_count: 0,
                skipped_transient_count: 0,
                skipped_unresolved_channel_count: 0,
                failed_count: 0,
                ambiguous_outcome_count: 0,
                stale_message_cleanup_pending_count: 0,
                orphan_message_cleanup_pending_count: 0,
                reposted_old_message_cleanup_pending_count: 0,
                reconciled_at: at(14),
            }),
        );
        process
    }

    fn metadata() -> RuntimeLiveMetadataV1 {
        RuntimeLiveMetadataV1 {
            runtime_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
            panel_report_digest: PanelReportDigestV1::parse("4".repeat(64)).unwrap(),
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        }
    }

    #[test]
    fn claim_receipt_must_match_its_embedded_lease() {
        let mut invalid = claimed();
        invalid.fencing_token = FencingToken::new(2).unwrap();
        assert!(matches!(
            RuntimeConvergenceSessionV1::from_claim(invalid),
            Err(RuntimeConvergenceSessionError::InvalidExecutionReceipt)
        ));
    }

    #[test]
    fn renewal_preserves_attempt_and_advances_revision_and_fence() {
        let mut session = RuntimeConvergenceSessionV1::from_claim(claimed()).unwrap();
        let request = session.begin_renewal(Duration::from_secs(90)).unwrap();
        let mut deployment = RuntimeDeployment::restore(session.snapshot().clone()).unwrap();
        let fencing_token = session.fencing_token().next().unwrap();
        deployment
            .acquire_lease(LeaseRequestV1 {
                expected_revision: session.snapshot().revision,
                controller_id: session.controller_id().clone(),
                fencing_token,
                now: at(30),
                expires_at: at(130),
            })
            .unwrap();
        let receipt = RuntimeExecutionUpdateReceiptV1 {
            action_id: request.action_id,
            execution: RuntimeExecutionReceiptV1 {
                snapshot: deployment.snapshot(),
                controller_id: session.controller_id().clone(),
                fencing_token,
                convergence_attempt: session.convergence_attempt(),
                acquired_at: at(30),
                expires_at: at(130),
            },
        };
        let stale = receipt.clone();
        session.apply_renewal(receipt).unwrap();
        assert_eq!(session.fencing_token(), FencingToken::new(2).unwrap());
        assert_eq!(session.convergence_attempt(), NonZeroU32::new(1).unwrap());
        let next_request = session.begin_renewal(Duration::from_secs(90)).unwrap();
        let mut stale = stale;
        stale.action_id = next_request.action_id;
        assert_eq!(
            session.apply_renewal(stale),
            Err(RuntimeConvergenceSessionError::StaleReceipt)
        );
    }

    #[test]
    fn renewal_rejects_mutation_advanced_claim_replay_with_old_fence() {
        let mut session = RuntimeConvergenceSessionV1::from_claim(claimed()).unwrap();
        let request = session.begin_renewal(Duration::from_secs(90)).unwrap();
        let before = session.snapshot().clone();
        let mut deployment = RuntimeDeployment::restore(before.clone()).unwrap();
        deployment
            .accept_preflight(
                &guard(&before),
                PreflightAttestationV1 {
                    target: target(),
                    runtime_generation: RuntimeGeneration::FIRST,
                    observed_runtime: None,
                    checked_at: at(21),
                },
            )
            .unwrap();
        let receipt = RuntimeExecutionUpdateReceiptV1 {
            action_id: request.action_id,
            execution: RuntimeExecutionReceiptV1 {
                snapshot: deployment.snapshot(),
                controller_id: session.controller_id().clone(),
                fencing_token: session.fencing_token(),
                convergence_attempt: session.convergence_attempt(),
                acquired_at: session.acquired_at(),
                expires_at: session.expires_at(),
            },
        };
        assert_eq!(
            session.apply_renewal(receipt),
            Err(RuntimeConvergenceSessionError::ReceiptMismatch)
        );
    }

    #[test]
    fn renewal_rejects_an_unchanged_fencing_token() {
        let mut session = RuntimeConvergenceSessionV1::from_claim(claimed()).unwrap();
        let request = session.begin_renewal(Duration::from_secs(90)).unwrap();
        let mut snapshot = session.snapshot().clone();
        snapshot.revision = snapshot.revision.next().unwrap();
        let lease = snapshot.controller_lease.as_mut().unwrap();
        lease.acquired_at = at(30);
        lease.expires_at = at(130);
        let receipt = RuntimeExecutionUpdateReceiptV1 {
            action_id: request.action_id,
            execution: RuntimeExecutionReceiptV1 {
                snapshot,
                controller_id: session.controller_id().clone(),
                fencing_token: session.fencing_token(),
                convergence_attempt: session.convergence_attempt(),
                acquired_at: at(30),
                expires_at: at(130),
            },
        };
        assert_eq!(
            session.apply_renewal(receipt),
            Err(RuntimeConvergenceSessionError::FencingTokenNotAdvanced)
        );
    }

    #[test]
    fn execution_guard_is_available_only_between_actions() {
        let mut session = RuntimeConvergenceSessionV1::from_claim(claimed()).unwrap();
        let guard = session.execution_guard().unwrap();
        assert!(guard.scope.matches(&session.snapshot().identity));
        assert_eq!(guard.expected_revision, session.snapshot().revision);
        assert_eq!(guard.controller_id, *session.controller_id());
        assert_eq!(guard.fencing_token, session.fencing_token());
        assert_eq!(
            guard.runtime_generation,
            session.snapshot().runtime_generation
        );
        assert_eq!(guard.convergence_attempt, session.convergence_attempt());

        let current_target = session.snapshot().target.clone();
        let request = session
            .begin_mutation(RuntimeConvergenceMutationV1::AcceptPreflight(
                PreflightAttestationV1 {
                    target: current_target,
                    runtime_generation: RuntimeGeneration::FIRST,
                    observed_runtime: None,
                    checked_at: at(20),
                },
            ))
            .unwrap();
        assert_eq!(
            session.execution_guard(),
            Err(RuntimeConvergenceSessionError::ActionInFlight)
        );
        session.abort_action(request.action_id).unwrap();
        assert_eq!(session.execution_guard(), Ok(guard));
    }

    #[test]
    fn current_execution_receipt_tracks_the_checked_session_authority() {
        let mut session = RuntimeConvergenceSessionV1::from_claim(claimed()).unwrap();
        let initial = session.current_execution_receipt().unwrap();
        assert_eq!(initial.snapshot, *session.snapshot());
        assert_eq!(initial.controller_id, *session.controller_id());
        assert_eq!(initial.fencing_token, session.fencing_token());
        assert_eq!(initial.convergence_attempt, session.convergence_attempt());
        assert_eq!(initial.acquired_at, session.acquired_at());
        assert_eq!(initial.expires_at, session.expires_at());

        let before = session.snapshot().clone();
        let request = session
            .begin_mutation(RuntimeConvergenceMutationV1::AcceptPreflight(
                PreflightAttestationV1 {
                    target: before.target.clone(),
                    runtime_generation: RuntimeGeneration::FIRST,
                    observed_runtime: None,
                    checked_at: at(20),
                },
            ))
            .unwrap();
        assert_eq!(
            session.current_execution_receipt(),
            Err(RuntimeConvergenceSessionError::ActionInFlight)
        );
        let receipt = mutation_receipt(&request, &before);
        session.apply_mutation(receipt).unwrap();
        let current = session.current_execution_receipt().unwrap();
        assert_eq!(current.snapshot, *session.snapshot());
        assert!(current.snapshot.revision > initial.snapshot.revision);
        assert_eq!(current.controller_id, initial.controller_id);
        assert_eq!(current.fencing_token, initial.fencing_token);
        assert_eq!(current.convergence_attempt, initial.convergence_attempt);
        assert_eq!(current.acquired_at, initial.acquired_at);
        assert_eq!(current.expires_at, initial.expires_at);
    }

    #[test]
    fn session_allows_only_the_phase_safe_mutation() {
        let mut session = RuntimeConvergenceSessionV1::from_claim(claimed()).unwrap();
        assert_eq!(
            session.begin_mutation(RuntimeConvergenceMutationV1::RequestDrain),
            Err(RuntimeConvergenceSessionError::InvalidMutationForPhase)
        );
        let request = session
            .begin_mutation(RuntimeConvergenceMutationV1::AcceptPreflight(
                PreflightAttestationV1 {
                    target: target(),
                    runtime_generation: RuntimeGeneration::FIRST,
                    observed_runtime: None,
                    checked_at: at(11),
                },
            ))
            .unwrap();
        assert_eq!(
            session.begin_renewal(Duration::from_secs(90)),
            Err(RuntimeConvergenceSessionError::ActionInFlight)
        );
        session.abort_action(request.action_id).unwrap();
        assert!(session.in_flight_action().is_none());
    }

    #[test]
    fn stale_mutation_receipt_is_rejected_after_progress() {
        let mut session = RuntimeConvergenceSessionV1::from_claim(claimed()).unwrap();
        let before = session.snapshot().clone();
        let request = session
            .begin_mutation(RuntimeConvergenceMutationV1::AcceptPreflight(
                PreflightAttestationV1 {
                    target: target(),
                    runtime_generation: RuntimeGeneration::FIRST,
                    observed_runtime: None,
                    checked_at: at(11),
                },
            ))
            .unwrap();
        let receipt = mutation_receipt(&request, &before);
        let stale = receipt.clone();
        session.apply_mutation(receipt).unwrap();
        let next = session
            .begin_mutation(RuntimeConvergenceMutationV1::RequestDrain)
            .unwrap();
        let mut stale = stale;
        stale.action_id = next.action_id;
        assert_eq!(
            session.apply_mutation(stale),
            Err(RuntimeConvergenceSessionError::StaleReceipt)
        );
    }

    #[test]
    fn certification_transfers_exact_serving_ownership() {
        let mut session = RuntimeConvergenceSessionV1::from_claim(claimed()).unwrap();
        let process = advance_to_awaiting(&mut session);
        let gateway_ready = GatewayReadyAttestationV1 {
            target: target(),
            runtime_generation: RuntimeGeneration::FIRST,
            process_instance_id: process.clone(),
            kind: GatewayReadyKindV1::DiscordReady,
            ready_at: at(15),
        };
        let request = session
            .begin_certification(gateway_ready.clone(), metadata(), Duration::from_secs(45))
            .unwrap();
        let before = session.snapshot().clone();
        let mut deployment = RuntimeDeployment::restore(before.clone()).unwrap();
        let outcome = deployment
            .certify_live(&guard(&before), gateway_ready, at(16))
            .unwrap();
        let snapshot = deployment.snapshot();
        let serving = RuntimeServingReceiptV1 {
            identity: RuntimeServingIdentityV1 {
                scope: RuntimeDeploymentScopeV1::from_identity(&snapshot.identity),
                attestation_id: RuntimeAttestationIdV1::parse("5".repeat(64)).unwrap(),
                process_instance_id: process,
                runtime_generation: RuntimeGeneration::FIRST,
                lease_epoch: NonZeroU64::new(1).unwrap(),
                expected_revision: NonZeroU64::new(1).unwrap(),
            },
            runtime_generation: RuntimeGeneration::FIRST,
            acquired_at: at(16),
            last_heartbeat_at: at(16),
            expires_at: at(61),
            connected: true,
            serving: true,
        };
        let live = session
            .apply_certification(RuntimeCertificationReceiptV1 {
                action_id: request.action_id,
                outcome,
                snapshot,
                convergence_attempt: session.convergence_attempt(),
                metadata: request.metadata,
                serving,
            })
            .unwrap();
        assert_eq!(
            session.state(),
            RuntimeConvergenceSessionStateV1::CertifiedLive
        );
        assert_eq!(
            session.current_execution_receipt(),
            Err(RuntimeConvergenceSessionError::InactiveSession)
        );
        assert_eq!(live.state(), RuntimeServingSessionStateV1::Serving);
    }

    #[test]
    fn serving_updates_require_the_next_revision_and_exact_identity() {
        let mut session = RuntimeConvergenceSessionV1::from_claim(claimed()).unwrap();
        let process = advance_to_awaiting(&mut session);
        let gateway_ready = GatewayReadyAttestationV1 {
            target: target(),
            runtime_generation: RuntimeGeneration::FIRST,
            process_instance_id: process.clone(),
            kind: GatewayReadyKindV1::DiscordReady,
            ready_at: at(15),
        };
        let request = session
            .begin_certification(gateway_ready.clone(), metadata(), Duration::from_secs(45))
            .unwrap();
        let before = session.snapshot().clone();
        let mut deployment = RuntimeDeployment::restore(before.clone()).unwrap();
        let outcome = deployment
            .certify_live(&guard(&before), gateway_ready, at(16))
            .unwrap();
        let snapshot = deployment.snapshot();
        let serving = RuntimeServingReceiptV1 {
            identity: RuntimeServingIdentityV1 {
                scope: RuntimeDeploymentScopeV1::from_identity(&snapshot.identity),
                attestation_id: RuntimeAttestationIdV1::parse("5".repeat(64)).unwrap(),
                process_instance_id: process,
                runtime_generation: RuntimeGeneration::FIRST,
                lease_epoch: NonZeroU64::new(1).unwrap(),
                expected_revision: NonZeroU64::new(1).unwrap(),
            },
            runtime_generation: RuntimeGeneration::FIRST,
            acquired_at: at(16),
            last_heartbeat_at: at(16),
            expires_at: at(61),
            connected: true,
            serving: true,
        };
        let mut live = session
            .apply_certification(RuntimeCertificationReceiptV1 {
                action_id: request.action_id,
                outcome,
                snapshot,
                convergence_attempt: session.convergence_attempt(),
                metadata: request.metadata,
                serving,
            })
            .unwrap();
        let heartbeat = live.begin_heartbeat(Duration::from_secs(45)).unwrap();
        let mut renewed = live.ownership().clone();
        renewed.identity.expected_revision = NonZeroU64::new(2).unwrap();
        renewed.last_heartbeat_at = at(30);
        renewed.expires_at = at(75);
        let stale = renewed.clone();
        live.apply_heartbeat(RuntimeServingUpdateReceiptV1 {
            action_id: heartbeat.action_id,
            serving: renewed,
        })
        .unwrap();
        let disconnect = live.begin_disconnect().unwrap();
        let mut stale = stale;
        stale.identity.expected_revision = NonZeroU64::new(2).unwrap();
        stale.connected = false;
        stale.serving = false;
        stale.last_heartbeat_at = at(31);
        stale.expires_at = at(31);
        assert_eq!(
            live.apply_disconnect(RuntimeServingUpdateReceiptV1 {
                action_id: disconnect.action_id,
                serving: stale,
            }),
            Err(RuntimeConvergenceSessionError::StaleReceipt)
        );
    }
}
