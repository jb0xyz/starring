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
    RuntimeMutationReceiptV1, RuntimeMutationRequestV1, RuntimeObservePreviousServingV1,
    RuntimePreviousServingLeaseEvidenceV1, RuntimePreviousServingObservationReceiptV1,
    RuntimePreviousServingStateV1, RuntimeRenewExecutionV1, RuntimeServingReceiptV1,
    RuntimeServingUpdateReceiptV1, RuntimeSessionActionIdV1,
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
    ObservePreviousServing(RuntimeObservePreviousServingV1),
    DisconnectPreviousServing {
        request: RuntimeDisconnectServingV1,
        observation: Box<RuntimePreviousServingObservationReceiptV1>,
    },
    Mutate(RuntimeMutationRequestV1),
    Certify(RuntimeCertificationRequestV1),
}

impl ExecutionActionV1 {
    fn id(&self) -> RuntimeSessionActionIdV1 {
        match self {
            Self::Renew(request) => request.action_id,
            Self::ObservePreviousServing(request) => request.action_id,
            Self::DisconnectPreviousServing { request, .. } => request.action_id,
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
    validated_previous_serving: Option<RuntimePreviousServingObservationReceiptV1>,
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
            validated_previous_serving: None,
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
        self.validated_previous_serving = None;
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

    pub fn begin_previous_serving_observation(
        &mut self,
    ) -> Result<RuntimeObservePreviousServingV1, RuntimeConvergenceSessionError> {
        self.require_action_slot()?;
        if !matches!(
            self.snapshot.phase,
            RuntimeDeploymentPhaseV1::DrainRequested
        ) {
            return Err(RuntimeConvergenceSessionError::InvalidMutationForPhase);
        }
        let request = RuntimeObservePreviousServingV1 {
            action_id: self.allocate_action_id()?,
            guard: self.guard(),
            expected_target: self.snapshot.target.clone(),
            expected_previous_runtime: self.snapshot.previous_runtime.clone(),
        };
        self.in_flight = Some(ExecutionActionV1::ObservePreviousServing(request.clone()));
        Ok(request)
    }

    pub fn apply_previous_serving_observation(
        &mut self,
        receipt: RuntimePreviousServingObservationReceiptV1,
    ) -> Result<RuntimePreviousServingObservationReceiptV1, RuntimeConvergenceSessionError> {
        let request = match self.in_flight.as_ref() {
            Some(ExecutionActionV1::ObservePreviousServing(request))
                if request.action_id == receipt.action_id =>
            {
                request.clone()
            }
            Some(_) => return Err(RuntimeConvergenceSessionError::ActionMismatch),
            None => return Err(RuntimeConvergenceSessionError::NoActionInFlight),
        };
        self.validate_guard(&request.guard)?;
        validate_previous_serving_observation(
            &self.snapshot,
            self.acquired_at,
            self.expires_at,
            &request,
            &receipt,
        )?;
        self.in_flight = None;
        self.validated_previous_serving = Some(receipt.clone());
        Ok(receipt)
    }

    pub fn begin_previous_serving_disconnect(
        &mut self,
        observation: &RuntimePreviousServingObservationReceiptV1,
    ) -> Result<RuntimeDisconnectServingV1, RuntimeConvergenceSessionError> {
        self.require_action_slot()?;
        if self.validated_previous_serving.as_ref() != Some(observation) {
            return Err(RuntimeConvergenceSessionError::ReceiptMismatch);
        }
        let observation_request = previous_serving_observation_request(observation);
        validate_previous_serving_observation(
            &self.snapshot,
            self.acquired_at,
            self.expires_at,
            &observation_request,
            observation,
        )?;
        let lease = match &observation.state {
            RuntimePreviousServingStateV1::Serving { lease, .. } => lease,
            RuntimePreviousServingStateV1::Absent
            | RuntimePreviousServingStateV1::Disconnected { .. }
            | RuntimePreviousServingStateV1::Expired { .. } => {
                return Err(RuntimeConvergenceSessionError::InvalidServingReceipt);
            }
        };
        let request = RuntimeDisconnectServingV1 {
            action_id: self.allocate_action_id()?,
            identity: previous_serving_identity(lease),
        };
        self.in_flight = Some(ExecutionActionV1::DisconnectPreviousServing {
            request: request.clone(),
            observation: Box::new(observation.clone()),
        });
        self.validated_previous_serving = None;
        Ok(request)
    }

    pub fn apply_previous_serving_disconnect(
        &mut self,
        receipt: RuntimeServingUpdateReceiptV1,
    ) -> Result<RuntimeServingReceiptV1, RuntimeConvergenceSessionError> {
        let (request, observation) = match self.in_flight.as_ref() {
            Some(ExecutionActionV1::DisconnectPreviousServing {
                request,
                observation,
            }) if request.action_id == receipt.action_id => {
                (request.clone(), observation.as_ref().clone())
            }
            Some(_) => return Err(RuntimeConvergenceSessionError::ActionMismatch),
            None => return Err(RuntimeConvergenceSessionError::NoActionInFlight),
        };
        self.validate_guard(&observation.guard)?;
        validate_previous_serving_disconnect_receipt(
            &self.snapshot,
            self.acquired_at,
            self.expires_at,
            &observation,
            &request,
            &receipt,
        )?;
        self.in_flight = None;
        Ok(receipt.serving)
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
        self.validated_previous_serving = None;
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
                .is_none_or(|panel| {
                    panel.process_instance_id != gateway_ready.process_instance_id
                        || panel.report_digest != metadata.panel_report_digest
                })
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
        self.validated_previous_serving = None;
        self.state = RuntimeConvergenceSessionStateV1::CertifiedLive;
        RuntimeServingSessionV1::restore(receipt.snapshot, receipt.serving)
    }

    pub fn abort_action(
        &mut self,
        action_id: RuntimeSessionActionIdV1,
    ) -> Result<(), RuntimeConvergenceSessionError> {
        match self.in_flight.as_ref() {
            Some(action) if action.id() == action_id => {
                if let ExecutionActionV1::DisconnectPreviousServing { observation, .. } = action {
                    self.validated_previous_serving = Some(observation.as_ref().clone());
                }
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

fn validate_previous_serving_observation(
    snapshot: &RuntimeDeploymentSnapshotV1,
    execution_acquired_at: chrono::DateTime<chrono::Utc>,
    execution_expires_at: chrono::DateTime<chrono::Utc>,
    request: &RuntimeObservePreviousServingV1,
    receipt: &RuntimePreviousServingObservationReceiptV1,
) -> Result<(), RuntimeConvergenceSessionError> {
    let preflight = snapshot
        .preflight
        .as_ref()
        .ok_or(RuntimeConvergenceSessionError::ReceiptMismatch)?;
    if !matches!(snapshot.phase, RuntimeDeploymentPhaseV1::DrainRequested)
        || !request.guard.scope.matches(&snapshot.identity)
        || request.guard.expected_revision != snapshot.revision
        || request.guard.runtime_generation != snapshot.runtime_generation
        || request.expected_target != snapshot.target
        || request.expected_previous_runtime != snapshot.previous_runtime
        || receipt.guard != request.guard
        || receipt.expected_target != request.expected_target
        || receipt.expected_previous_runtime != request.expected_previous_runtime
        || receipt.observed_at < execution_acquired_at
        || receipt.observed_at < preflight.checked_at
        || receipt.observed_at >= execution_expires_at
    {
        return Err(RuntimeConvergenceSessionError::ReceiptMismatch);
    }
    let valid = match &receipt.state {
        RuntimePreviousServingStateV1::Absent => request.expected_previous_runtime.is_none(),
        RuntimePreviousServingStateV1::Disconnected {
            lease,
            disconnected_at,
        } => {
            previous_serving_lease_matches(snapshot, request, lease)
                && lease.acquired_at <= lease.last_heartbeat_at
                && lease.last_heartbeat_at == *disconnected_at
                && *disconnected_at >= snapshot.requested_at
                && *disconnected_at <= receipt.observed_at
        }
        RuntimePreviousServingStateV1::Expired { lease, expires_at } => {
            previous_serving_lease_matches(snapshot, request, lease)
                && lease.acquired_at <= lease.last_heartbeat_at
                && lease.last_heartbeat_at < *expires_at
                && *expires_at > snapshot.requested_at
                && *expires_at <= receipt.observed_at
        }
        RuntimePreviousServingStateV1::Serving { lease, expires_at } => {
            previous_serving_lease_matches(snapshot, request, lease)
                && lease.acquired_at <= lease.last_heartbeat_at
                && lease.last_heartbeat_at <= receipt.observed_at
                && receipt.observed_at < *expires_at
        }
    };
    if valid {
        Ok(())
    } else {
        Err(RuntimeConvergenceSessionError::ReceiptMismatch)
    }
}

fn previous_serving_observation_request(
    receipt: &RuntimePreviousServingObservationReceiptV1,
) -> RuntimeObservePreviousServingV1 {
    RuntimeObservePreviousServingV1 {
        action_id: receipt.action_id,
        guard: receipt.guard.clone(),
        expected_target: receipt.expected_target.clone(),
        expected_previous_runtime: receipt.expected_previous_runtime.clone(),
    }
}

fn previous_serving_identity(
    lease: &RuntimePreviousServingLeaseEvidenceV1,
) -> crate::RuntimeServingIdentityV1 {
    crate::RuntimeServingIdentityV1 {
        scope: lease.identity.scope.clone(),
        attestation_id: lease.identity.attestation_id.clone(),
        process_instance_id: lease.identity.process.process_instance_id.clone(),
        runtime_generation: lease.identity.process.runtime_generation,
        lease_epoch: lease.identity.lease_epoch,
        expected_revision: lease.identity.revision,
    }
}

fn validate_previous_serving_disconnect_receipt(
    snapshot: &RuntimeDeploymentSnapshotV1,
    execution_acquired_at: chrono::DateTime<chrono::Utc>,
    execution_expires_at: chrono::DateTime<chrono::Utc>,
    observation: &RuntimePreviousServingObservationReceiptV1,
    request: &RuntimeDisconnectServingV1,
    receipt: &RuntimeServingUpdateReceiptV1,
) -> Result<(), RuntimeConvergenceSessionError> {
    let observation_request = previous_serving_observation_request(observation);
    validate_previous_serving_observation(
        snapshot,
        execution_acquired_at,
        execution_expires_at,
        &observation_request,
        observation,
    )?;
    let lease = match &observation.state {
        RuntimePreviousServingStateV1::Serving { lease, .. } => lease,
        RuntimePreviousServingStateV1::Absent
        | RuntimePreviousServingStateV1::Disconnected { .. }
        | RuntimePreviousServingStateV1::Expired { .. } => {
            return Err(RuntimeConvergenceSessionError::InvalidServingReceipt);
        }
    };
    let expected_identity = previous_serving_identity(lease);
    if request.identity != expected_identity
        || receipt.action_id != request.action_id
        || receipt.serving.identity.scope != expected_identity.scope
        || receipt.serving.identity.attestation_id != expected_identity.attestation_id
        || receipt.serving.identity.process_instance_id != expected_identity.process_instance_id
        || receipt.serving.identity.runtime_generation != expected_identity.runtime_generation
        || receipt.serving.identity.lease_epoch != expected_identity.lease_epoch
        || receipt.serving.runtime_generation != expected_identity.runtime_generation
        || receipt.serving.acquired_at != lease.acquired_at
        || receipt.serving.connected
        || receipt.serving.serving
        || receipt.serving.last_heartbeat_at < lease.last_heartbeat_at
        || receipt.serving.last_heartbeat_at < observation.observed_at
        || receipt.serving.last_heartbeat_at >= execution_expires_at
        || receipt.serving.expires_at != receipt.serving.last_heartbeat_at
    {
        return Err(RuntimeConvergenceSessionError::ReceiptMismatch);
    }
    let current_revision = expected_identity.expected_revision.get();
    let next_revision = receipt.serving.identity.expected_revision.get();
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
    Ok(())
}

fn previous_serving_lease_matches(
    snapshot: &RuntimeDeploymentSnapshotV1,
    request: &RuntimeObservePreviousServingV1,
    lease: &RuntimePreviousServingLeaseEvidenceV1,
) -> bool {
    request.expected_previous_runtime.as_ref() == Some(&lease.identity.process)
        && lease.acquired_at <= snapshot.requested_at
        && lease.identity.scope.tenant_id == request.guard.scope.tenant_id
        && lease.identity.scope.installation_id == request.guard.scope.installation_id
        && lease.identity.scope.deployment_id != request.guard.scope.deployment_id
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
    use std::future::Future;
    use std::num::{NonZeroU32, NonZeroU64};
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
    use automation_runtime_convergence::{
        ActivationAttestationV1, ActivationOutcomeKindV1, ActivationRequestId, BindingRevision,
        CommandGuardV1, ControllerId, DeploymentId, DrainAttestationV1, FencingToken,
        GatewayReadyAttestationV1, GatewayReadyKindV1, InstallationId, LeaseRequestV1,
        PanelCertificateId, PanelCertificateV1, PreflightAttestationV1, ProcessInstanceId,
        PromotionId, RuntimeDeployment, RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1,
        RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
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
        claimed_with(RuntimeGeneration::FIRST, None)
    }

    fn claimed_with(
        runtime_generation: RuntimeGeneration,
        previous_runtime: Option<RuntimeProcessIdentityV1>,
    ) -> RuntimeExecutionReceiptV1 {
        let mut deployment = RuntimeDeployment::request(
            RuntimeDeploymentIdentityV1 {
                deployment_id: DeploymentId::parse("deployment").unwrap(),
                tenant_id: TenantId::parse("tenant").unwrap(),
                installation_id: InstallationId::parse("installation").unwrap(),
                promotion_id: PromotionId::parse("1".repeat(64)).unwrap(),
                activation_request_id: ActivationRequestId::parse("activation").unwrap(),
            },
            target(),
            runtime_generation,
            previous_runtime,
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

    fn previous_runtime() -> RuntimeProcessIdentityV1 {
        RuntimeProcessIdentityV1 {
            target: target(),
            runtime_generation: RuntimeGeneration::FIRST,
            process_instance_id: ProcessInstanceId::parse("previous-process").unwrap(),
        }
    }

    fn advance_to_drain_requested(session: &mut RuntimeConvergenceSessionV1) {
        apply(
            session,
            RuntimeConvergenceMutationV1::AcceptPreflight(PreflightAttestationV1 {
                target: session.snapshot().target.clone(),
                runtime_generation: session.snapshot().runtime_generation,
                observed_runtime: session.snapshot().previous_runtime.clone(),
                checked_at: at(11),
            }),
        );
        apply(session, RuntimeConvergenceMutationV1::RequestDrain);
    }

    fn previous_lease(
        request: &RuntimeObservePreviousServingV1,
    ) -> RuntimePreviousServingLeaseEvidenceV1 {
        RuntimePreviousServingLeaseEvidenceV1 {
            identity: crate::RuntimePreviousServingLeaseIdentityV1 {
                scope: RuntimeDeploymentScopeV1 {
                    tenant_id: request.guard.scope.tenant_id.clone(),
                    installation_id: request.guard.scope.installation_id.clone(),
                    deployment_id: DeploymentId::parse("previous-deployment").unwrap(),
                },
                attestation_id: RuntimeAttestationIdV1::parse("6".repeat(64)).unwrap(),
                process: request.expected_previous_runtime.clone().unwrap(),
                lease_epoch: NonZeroU64::new(3).unwrap(),
                revision: NonZeroU64::new(7).unwrap(),
            },
            acquired_at: at(0),
            last_heartbeat_at: at(18),
        }
    }

    fn observation_receipt(
        request: &RuntimeObservePreviousServingV1,
        state: RuntimePreviousServingStateV1,
    ) -> RuntimePreviousServingObservationReceiptV1 {
        RuntimePreviousServingObservationReceiptV1 {
            action_id: request.action_id,
            guard: request.guard.clone(),
            observed_at: at(20),
            expected_target: request.expected_target.clone(),
            expected_previous_runtime: request.expected_previous_runtime.clone(),
            state,
        }
    }

    fn serving_observation(
        session: &mut RuntimeConvergenceSessionV1,
    ) -> RuntimePreviousServingObservationReceiptV1 {
        let request = session.begin_previous_serving_observation().unwrap();
        let receipt = observation_receipt(
            &request,
            RuntimePreviousServingStateV1::Serving {
                lease: previous_lease(&request),
                expires_at: at(30),
            },
        );
        session
            .apply_previous_serving_observation(receipt.clone())
            .unwrap();
        receipt
    }

    fn previous_disconnect_receipt(
        request: &RuntimeDisconnectServingV1,
    ) -> RuntimeServingUpdateReceiptV1 {
        let mut identity = request.identity.clone();
        identity.expected_revision =
            NonZeroU64::new(identity.expected_revision.get().checked_add(1).unwrap()).unwrap();
        RuntimeServingUpdateReceiptV1 {
            action_id: request.action_id,
            serving: RuntimeServingReceiptV1 {
                identity,
                runtime_generation: request.identity.runtime_generation,
                acquired_at: at(0),
                last_heartbeat_at: at(21),
                expires_at: at(21),
                connected: false,
                serving: false,
            },
        }
    }

    fn block_on_ready<F: Future>(future: F) -> F::Output {
        let mut context = Context::from_waker(Waker::noop());
        let mut future = pin!(future);
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("test future unexpectedly pending"),
        }
    }

    struct FakeObservationPort {
        expected: RuntimeObservePreviousServingV1,
        receipt: RuntimePreviousServingObservationReceiptV1,
    }

    impl crate::RuntimeExecutionConvergencePort for FakeObservationPort {
        type Error = ();

        async fn claim_next_execution(
            &self,
            _request: crate::RuntimeClaimNextExecutionV1,
        ) -> Result<Option<RuntimeExecutionReceiptV1>, Self::Error> {
            panic!("unexpected claim")
        }

        async fn renew_execution(
            &self,
            _request: RuntimeRenewExecutionV1,
        ) -> Result<RuntimeExecutionUpdateReceiptV1, Self::Error> {
            panic!("unexpected renewal")
        }

        async fn mutate(
            &self,
            _request: RuntimeMutationRequestV1,
        ) -> Result<RuntimeMutationReceiptV1, Self::Error> {
            panic!("unexpected mutation")
        }

        async fn certify_live(
            &self,
            _request: RuntimeCertificationRequestV1,
        ) -> Result<RuntimeCertificationReceiptV1, Self::Error> {
            panic!("unexpected certification")
        }

        async fn recover_next_stale_live(
            &self,
        ) -> Result<Option<crate::RuntimeStaleLiveRecoveryReceiptV1>, Self::Error> {
            panic!("unexpected recovery")
        }

        fn classify_error(_error: &Self::Error) -> crate::RuntimeConvergenceErrorClassV1 {
            crate::RuntimeConvergenceErrorClassV1::InvalidState
        }
    }

    impl crate::RuntimePreviousServingObservationPort for FakeObservationPort {
        async fn observe_previous_serving(
            &self,
            request: RuntimeObservePreviousServingV1,
        ) -> Result<RuntimePreviousServingObservationReceiptV1, Self::Error> {
            assert_eq!(request, self.expected);
            Ok(self.receipt.clone())
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
                report_digest: PanelReportDigestV1::parse("4".repeat(64)).unwrap(),
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
    fn previous_serving_observation_is_available_only_in_drain_requested() {
        let mut session = RuntimeConvergenceSessionV1::from_claim(claimed()).unwrap();
        assert_eq!(
            session.begin_previous_serving_observation(),
            Err(RuntimeConvergenceSessionError::InvalidMutationForPhase)
        );
        advance_to_drain_requested(&mut session);
        let request = session.begin_previous_serving_observation().unwrap();
        assert_eq!(request.guard, session.guard());
        assert_eq!(request.expected_target, session.snapshot().target);
        assert_eq!(
            request.expected_previous_runtime,
            session.snapshot().previous_runtime
        );
        assert_eq!(session.in_flight_action(), Some(request.action_id));
        assert_eq!(
            session.begin_renewal(Duration::from_secs(90)),
            Err(RuntimeConvergenceSessionError::ActionInFlight)
        );
        session.abort_action(request.action_id).unwrap();
        assert!(session.in_flight_action().is_none());
    }

    #[test]
    fn fake_port_round_trip_accepts_exact_absent_observation() {
        let mut session = RuntimeConvergenceSessionV1::from_claim(claimed()).unwrap();
        advance_to_drain_requested(&mut session);
        let request = session.begin_previous_serving_observation().unwrap();
        let expected = observation_receipt(&request, RuntimePreviousServingStateV1::Absent);
        let port = FakeObservationPort {
            expected: request.clone(),
            receipt: expected.clone(),
        };
        let receipt = block_on_ready(
            <FakeObservationPort as crate::RuntimePreviousServingObservationPort>::observe_previous_serving(
                &port, request,
            ),
        )
        .unwrap();
        let validated = session.apply_previous_serving_observation(receipt).unwrap();
        assert_eq!(validated, expected);
        assert!(session.in_flight_action().is_none());
        assert_eq!(
            session.snapshot().phase,
            RuntimeDeploymentPhaseV1::DrainRequested
        );
    }

    #[test]
    fn exact_previous_serving_closed_states_are_accepted() {
        let previous = previous_runtime();
        let mut session = RuntimeConvergenceSessionV1::from_claim(claimed_with(
            RuntimeGeneration::FIRST.next().unwrap(),
            Some(previous),
        ))
        .unwrap();
        advance_to_drain_requested(&mut session);

        let request = session.begin_previous_serving_observation().unwrap();
        let disconnected = observation_receipt(
            &request,
            RuntimePreviousServingStateV1::Disconnected {
                lease: previous_lease(&request),
                disconnected_at: at(18),
            },
        );
        assert_eq!(
            session
                .apply_previous_serving_observation(disconnected.clone())
                .unwrap(),
            disconnected
        );

        let request = session.begin_previous_serving_observation().unwrap();
        let expired = observation_receipt(
            &request,
            RuntimePreviousServingStateV1::Expired {
                lease: previous_lease(&request),
                expires_at: at(19),
            },
        );
        assert_eq!(
            session
                .apply_previous_serving_observation(expired.clone())
                .unwrap(),
            expired
        );

        let request = session.begin_previous_serving_observation().unwrap();
        let serving = observation_receipt(
            &request,
            RuntimePreviousServingStateV1::Serving {
                lease: previous_lease(&request),
                expires_at: at(30),
            },
        );
        assert_eq!(
            session
                .apply_previous_serving_observation(serving.clone())
                .unwrap(),
            serving
        );
    }

    #[test]
    fn previous_serving_disconnect_is_derived_from_and_applies_the_exact_observation() {
        let previous = previous_runtime();
        let mut session = RuntimeConvergenceSessionV1::from_claim(claimed_with(
            RuntimeGeneration::FIRST.next().unwrap(),
            Some(previous),
        ))
        .unwrap();
        advance_to_drain_requested(&mut session);
        let observation = serving_observation(&mut session);
        let lease = match &observation.state {
            RuntimePreviousServingStateV1::Serving { lease, .. } => lease,
            _ => panic!("expected serving observation"),
        };

        let request = session
            .begin_previous_serving_disconnect(&observation)
            .unwrap();
        assert_eq!(request.identity, previous_serving_identity(lease));
        assert_eq!(session.in_flight_action(), Some(request.action_id));
        let receipt = previous_disconnect_receipt(&request);
        let expected = receipt.serving.clone();
        let applied = session.apply_previous_serving_disconnect(receipt).unwrap();

        assert_eq!(applied, expected);
        assert!(!applied.connected);
        assert!(!applied.serving);
        assert_eq!(
            applied.identity.expected_revision.get(),
            lease.identity.revision.get() + 1
        );
        assert!(session.in_flight_action().is_none());
        assert_eq!(
            session.snapshot().phase,
            RuntimeDeploymentPhaseV1::DrainRequested
        );
    }

    #[test]
    fn previous_serving_disconnect_rejects_unvalidated_tampered_and_closed_evidence() {
        let previous = previous_runtime();
        let claim = claimed_with(RuntimeGeneration::FIRST.next().unwrap(), Some(previous));
        let mut validated = RuntimeConvergenceSessionV1::from_claim(claim.clone()).unwrap();
        advance_to_drain_requested(&mut validated);
        let observation = serving_observation(&mut validated);

        let mut other = RuntimeConvergenceSessionV1::from_claim(claim).unwrap();
        advance_to_drain_requested(&mut other);
        assert_eq!(
            other.begin_previous_serving_disconnect(&observation),
            Err(RuntimeConvergenceSessionError::ReceiptMismatch)
        );

        let mut tampered = observation.clone();
        tampered.observed_at = at(21);
        assert_eq!(
            validated.begin_previous_serving_disconnect(&tampered),
            Err(RuntimeConvergenceSessionError::ReceiptMismatch)
        );

        let request = validated
            .begin_previous_serving_disconnect(&observation)
            .unwrap();
        validated.abort_action(request.action_id).unwrap();
        assert!(validated
            .begin_previous_serving_disconnect(&observation)
            .is_ok());

        let mut closed = RuntimeConvergenceSessionV1::from_claim(claimed_with(
            RuntimeGeneration::FIRST.next().unwrap(),
            Some(previous_runtime()),
        ))
        .unwrap();
        advance_to_drain_requested(&mut closed);
        let observation_request = closed.begin_previous_serving_observation().unwrap();
        let disconnected = observation_receipt(
            &observation_request,
            RuntimePreviousServingStateV1::Disconnected {
                lease: previous_lease(&observation_request),
                disconnected_at: at(18),
            },
        );
        closed
            .apply_previous_serving_observation(disconnected.clone())
            .unwrap();
        assert_eq!(
            closed.begin_previous_serving_disconnect(&disconnected),
            Err(RuntimeConvergenceSessionError::InvalidServingReceipt)
        );
    }

    #[test]
    fn previous_serving_disconnect_receipt_rejects_identity_revision_and_state_drift() {
        for drift in 0..5 {
            let mut session = RuntimeConvergenceSessionV1::from_claim(claimed_with(
                RuntimeGeneration::FIRST.next().unwrap(),
                Some(previous_runtime()),
            ))
            .unwrap();
            advance_to_drain_requested(&mut session);
            let observation = serving_observation(&mut session);
            let request = session
                .begin_previous_serving_disconnect(&observation)
                .unwrap();
            let mut receipt = previous_disconnect_receipt(&request);
            let expected_error = match drift {
                0 => {
                    receipt.serving.identity.process_instance_id =
                        ProcessInstanceId::parse("other-process").unwrap();
                    RuntimeConvergenceSessionError::ReceiptMismatch
                }
                1 => {
                    receipt.serving.identity.expected_revision = request.identity.expected_revision;
                    RuntimeConvergenceSessionError::StaleReceipt
                }
                2 => {
                    receipt.serving.identity.expected_revision = NonZeroU64::new(
                        request
                            .identity
                            .expected_revision
                            .get()
                            .checked_add(2)
                            .unwrap(),
                    )
                    .unwrap();
                    RuntimeConvergenceSessionError::RevisionGap
                }
                3 => {
                    receipt.serving.connected = true;
                    RuntimeConvergenceSessionError::ReceiptMismatch
                }
                _ => {
                    receipt.serving.last_heartbeat_at = session.expires_at();
                    receipt.serving.expires_at = session.expires_at();
                    RuntimeConvergenceSessionError::ReceiptMismatch
                }
            };
            assert_eq!(
                session.apply_previous_serving_disconnect(receipt),
                Err(expected_error)
            );
            assert_eq!(session.in_flight_action(), Some(request.action_id));
        }
    }

    #[test]
    fn deployment_progress_invalidates_a_validated_previous_serving_observation() {
        let previous = previous_runtime();
        let mut session = RuntimeConvergenceSessionV1::from_claim(claimed_with(
            RuntimeGeneration::FIRST.next().unwrap(),
            Some(previous.clone()),
        ))
        .unwrap();
        advance_to_drain_requested(&mut session);
        let observation = serving_observation(&mut session);
        apply(
            &mut session,
            RuntimeConvergenceMutationV1::AcceptDrain(DrainAttestationV1 {
                previous_runtime: Some(previous),
                target_runtime_generation: RuntimeGeneration::FIRST.next().unwrap(),
                drained_at: at(21),
            }),
        );
        assert_eq!(
            session.begin_previous_serving_disconnect(&observation),
            Err(RuntimeConvergenceSessionError::ReceiptMismatch)
        );
    }

    #[test]
    fn previous_serving_observation_rejects_cross_incarnation_and_time_boundaries() {
        let previous = previous_runtime();
        let mut session = RuntimeConvergenceSessionV1::from_claim(claimed_with(
            RuntimeGeneration::FIRST.next().unwrap(),
            Some(previous),
        ))
        .unwrap();
        advance_to_drain_requested(&mut session);

        let request = session.begin_previous_serving_observation().unwrap();
        let mut wrong_lease = previous_lease(&request);
        wrong_lease.identity.process.process_instance_id =
            ProcessInstanceId::parse("reused-process").unwrap();
        let wrong_process = observation_receipt(
            &request,
            RuntimePreviousServingStateV1::Expired {
                lease: wrong_lease,
                expires_at: at(19),
            },
        );
        assert_eq!(
            session.apply_previous_serving_observation(wrong_process),
            Err(RuntimeConvergenceSessionError::ReceiptMismatch)
        );
        assert_eq!(session.in_flight_action(), Some(request.action_id));
        session.abort_action(request.action_id).unwrap();

        let request = session.begin_previous_serving_observation().unwrap();
        let expired_at_heartbeat = observation_receipt(
            &request,
            RuntimePreviousServingStateV1::Expired {
                lease: previous_lease(&request),
                expires_at: at(18),
            },
        );
        assert_eq!(
            session.apply_previous_serving_observation(expired_at_heartbeat),
            Err(RuntimeConvergenceSessionError::ReceiptMismatch)
        );
        session.abort_action(request.action_id).unwrap();

        let request = session.begin_previous_serving_observation().unwrap();
        let serving_at_expiry = observation_receipt(
            &request,
            RuntimePreviousServingStateV1::Serving {
                lease: previous_lease(&request),
                expires_at: at(20),
            },
        );
        assert_eq!(
            session.apply_previous_serving_observation(serving_at_expiry),
            Err(RuntimeConvergenceSessionError::ReceiptMismatch)
        );
    }

    #[test]
    fn previous_serving_observation_rejects_a_lease_acquired_after_the_request() {
        let previous = previous_runtime();
        let mut session = RuntimeConvergenceSessionV1::from_claim(claimed_with(
            RuntimeGeneration::FIRST.next().unwrap(),
            Some(previous),
        ))
        .unwrap();
        advance_to_drain_requested(&mut session);

        for state_kind in 0..3 {
            let request = session.begin_previous_serving_observation().unwrap();
            let mut lease = previous_lease(&request);
            lease.acquired_at = at(2);
            let state = match state_kind {
                0 => RuntimePreviousServingStateV1::Disconnected {
                    lease,
                    disconnected_at: at(18),
                },
                1 => RuntimePreviousServingStateV1::Expired {
                    lease,
                    expires_at: at(19),
                },
                _ => RuntimePreviousServingStateV1::Serving {
                    lease,
                    expires_at: at(30),
                },
            };
            assert_eq!(
                session.apply_previous_serving_observation(observation_receipt(&request, state)),
                Err(RuntimeConvergenceSessionError::ReceiptMismatch)
            );
            session.abort_action(request.action_id).unwrap();
        }
    }

    #[test]
    fn previous_serving_observation_rejects_pre_request_closure_evidence() {
        let previous = previous_runtime();
        let mut session = RuntimeConvergenceSessionV1::from_claim(claimed_with(
            RuntimeGeneration::FIRST.next().unwrap(),
            Some(previous),
        ))
        .unwrap();
        advance_to_drain_requested(&mut session);

        let request = session.begin_previous_serving_observation().unwrap();
        let mut disconnected_lease = previous_lease(&request);
        disconnected_lease.last_heartbeat_at = at(0);
        let disconnected = RuntimePreviousServingStateV1::Disconnected {
            lease: disconnected_lease,
            disconnected_at: at(0),
        };
        assert_eq!(
            session
                .apply_previous_serving_observation(observation_receipt(&request, disconnected,)),
            Err(RuntimeConvergenceSessionError::ReceiptMismatch)
        );
        session.abort_action(request.action_id).unwrap();

        let request = session.begin_previous_serving_observation().unwrap();
        let mut expired_lease = previous_lease(&request);
        expired_lease.last_heartbeat_at = at(0);
        let expired = RuntimePreviousServingStateV1::Expired {
            lease: expired_lease,
            expires_at: session.snapshot().requested_at,
        };
        assert_eq!(
            session.apply_previous_serving_observation(observation_receipt(&request, expired)),
            Err(RuntimeConvergenceSessionError::ReceiptMismatch)
        );
    }

    #[test]
    fn previous_serving_observation_rejects_tampered_execution_evidence() {
        let mut session = RuntimeConvergenceSessionV1::from_claim(claimed()).unwrap();
        advance_to_drain_requested(&mut session);

        let request = session.begin_previous_serving_observation().unwrap();
        let mut wrong_guard = observation_receipt(&request, RuntimePreviousServingStateV1::Absent);
        wrong_guard.guard.expected_revision = wrong_guard.guard.expected_revision.next().unwrap();
        assert_eq!(
            session.apply_previous_serving_observation(wrong_guard),
            Err(RuntimeConvergenceSessionError::ReceiptMismatch)
        );
        session.abort_action(request.action_id).unwrap();

        let request = session.begin_previous_serving_observation().unwrap();
        let mut wrong_previous =
            observation_receipt(&request, RuntimePreviousServingStateV1::Absent);
        wrong_previous.expected_previous_runtime = Some(previous_runtime());
        assert_eq!(
            session.apply_previous_serving_observation(wrong_previous),
            Err(RuntimeConvergenceSessionError::ReceiptMismatch)
        );
        session.abort_action(request.action_id).unwrap();

        let request = session.begin_previous_serving_observation().unwrap();
        let mut expired_execution =
            observation_receipt(&request, RuntimePreviousServingStateV1::Absent);
        expired_execution.observed_at = session.expires_at();
        assert_eq!(
            session.apply_previous_serving_observation(expired_execution),
            Err(RuntimeConvergenceSessionError::ReceiptMismatch)
        );
    }

    #[test]
    fn previous_serving_state_must_match_snapshot_previous_identity() {
        let previous = previous_runtime();
        let mut with_previous = RuntimeConvergenceSessionV1::from_claim(claimed_with(
            RuntimeGeneration::FIRST.next().unwrap(),
            Some(previous),
        ))
        .unwrap();
        advance_to_drain_requested(&mut with_previous);
        let request = with_previous.begin_previous_serving_observation().unwrap();
        assert_eq!(
            with_previous.apply_previous_serving_observation(observation_receipt(
                &request,
                RuntimePreviousServingStateV1::Absent,
            )),
            Err(RuntimeConvergenceSessionError::ReceiptMismatch)
        );

        let mut absent = RuntimeConvergenceSessionV1::from_claim(claimed()).unwrap();
        advance_to_drain_requested(&mut absent);
        let request = absent.begin_previous_serving_observation().unwrap();
        let lease = RuntimePreviousServingLeaseEvidenceV1 {
            identity: crate::RuntimePreviousServingLeaseIdentityV1 {
                scope: RuntimeDeploymentScopeV1 {
                    tenant_id: request.guard.scope.tenant_id.clone(),
                    installation_id: request.guard.scope.installation_id.clone(),
                    deployment_id: DeploymentId::parse("previous-deployment").unwrap(),
                },
                attestation_id: RuntimeAttestationIdV1::parse("6".repeat(64)).unwrap(),
                process: previous_runtime(),
                lease_epoch: NonZeroU64::new(1).unwrap(),
                revision: NonZeroU64::new(1).unwrap(),
            },
            acquired_at: at(2),
            last_heartbeat_at: at(18),
        };
        assert_eq!(
            absent.apply_previous_serving_observation(observation_receipt(
                &request,
                RuntimePreviousServingStateV1::Serving {
                    lease,
                    expires_at: at(30),
                },
            )),
            Err(RuntimeConvergenceSessionError::ReceiptMismatch)
        );
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
    fn certification_rejects_a_digest_from_another_panel_report() {
        let mut session = RuntimeConvergenceSessionV1::from_claim(claimed()).unwrap();
        let process = advance_to_awaiting(&mut session);
        let gateway_ready = GatewayReadyAttestationV1 {
            target: target(),
            runtime_generation: RuntimeGeneration::FIRST,
            process_instance_id: process,
            kind: GatewayReadyKindV1::DiscordReady,
            ready_at: at(15),
        };
        let mut mismatched = metadata();
        mismatched.panel_report_digest = PanelReportDigestV1::parse("5".repeat(64)).unwrap();
        assert_eq!(
            session.begin_certification(gateway_ready.clone(), mismatched, Duration::from_secs(45)),
            Err(RuntimeConvergenceSessionError::InvalidMutationForPhase)
        );
        assert!(session.in_flight_action().is_none());
        assert!(session
            .begin_certification(gateway_ready, metadata(), Duration::from_secs(45))
            .is_ok());
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
