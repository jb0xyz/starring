use std::num::NonZeroU32;

use chrono::{DateTime, Utc};

mod product_drain;
mod validation;

pub use product_drain::ProductDrainSourceSupersessionPermitV1;

use crate::{
    ActivationAttestationV1, CommandGuardV1, ControllerLeaseV1, DeploymentId, DeploymentRevision,
    DrainAttestationV1, GatewayReadyAttestationV1, LeaseRequestV1, LiveAttestationV1,
    LiveRecoveryAttestationV1, PanelCertificateV1, PreflightAttestationV1, RecoverBlockedRequestV1,
    RecoverLiveRequestV1, RuntimeDeploymentError, RuntimeDeploymentIdentityV1,
    RuntimeDeploymentPhaseKindV1, RuntimeDeploymentPhaseV1, RuntimeDeploymentSnapshotV1,
    RuntimeDeploymentTargetV1, RuntimeFailureDispositionV1, RuntimeFailureV1, RuntimeGeneration,
    RuntimePendingConditionV1, RuntimeProcessIdentityV1, SupersedingDeploymentV1,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionOutcomeV1 {
    Applied { revision: DeploymentRevision },
    Replayed { revision: DeploymentRevision },
}

impl TransitionOutcomeV1 {
    pub fn revision(self) -> DeploymentRevision {
        match self {
            Self::Applied { revision } | Self::Replayed { revision } => revision,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDeployment {
    identity: RuntimeDeploymentIdentityV1,
    target: RuntimeDeploymentTargetV1,
    runtime_generation: RuntimeGeneration,
    previous_runtime: Option<RuntimeProcessIdentityV1>,
    requested_at: DateTime<Utc>,
    revision: DeploymentRevision,
    phase: RuntimeDeploymentPhaseV1,
    controller_lease: Option<ControllerLeaseV1>,
    last_fencing_token: Option<crate::FencingToken>,
    preflight: Option<PreflightAttestationV1>,
    drain: Option<DrainAttestationV1>,
    activation: Option<ActivationAttestationV1>,
    panel_certificate: Option<PanelCertificateV1>,
    gateway_ready: Option<GatewayReadyAttestationV1>,
    live: Option<LiveAttestationV1>,
    last_live_recovery: Option<LiveRecoveryAttestationV1>,
    last_runtime_failure: Option<RuntimeFailureDispositionV1>,
}

impl RuntimeDeployment {
    pub fn request(
        identity: RuntimeDeploymentIdentityV1,
        target: RuntimeDeploymentTargetV1,
        runtime_generation: RuntimeGeneration,
        previous_runtime: Option<RuntimeProcessIdentityV1>,
        requested_at: DateTime<Utc>,
    ) -> Result<Self, RuntimeDeploymentError> {
        if let Some(previous) = &previous_runtime {
            if !target.same_slot(&previous.target) {
                return Err(RuntimeDeploymentError::PreviousRuntimeSlotMismatch);
            }
            if runtime_generation <= previous.runtime_generation {
                return Err(RuntimeDeploymentError::RuntimeGenerationNotMonotonic);
            }
        }
        Ok(Self {
            identity,
            target,
            runtime_generation,
            previous_runtime,
            requested_at,
            revision: DeploymentRevision::FIRST,
            phase: RuntimeDeploymentPhaseV1::Requested,
            controller_lease: None,
            last_fencing_token: None,
            preflight: None,
            drain: None,
            activation: None,
            panel_certificate: None,
            gateway_ready: None,
            live: None,
            last_live_recovery: None,
            last_runtime_failure: None,
        })
    }

    pub fn restore(snapshot: RuntimeDeploymentSnapshotV1) -> Result<Self, RuntimeDeploymentError> {
        let deployment = Self {
            identity: snapshot.identity,
            target: snapshot.target,
            runtime_generation: snapshot.runtime_generation,
            previous_runtime: snapshot.previous_runtime,
            requested_at: snapshot.requested_at,
            revision: snapshot.revision,
            phase: snapshot.phase,
            controller_lease: snapshot.controller_lease,
            last_fencing_token: snapshot.last_fencing_token,
            preflight: snapshot.preflight,
            drain: snapshot.drain,
            activation: snapshot.activation,
            panel_certificate: snapshot.panel_certificate,
            gateway_ready: snapshot.gateway_ready,
            live: snapshot.live,
            last_live_recovery: snapshot.last_live_recovery,
            last_runtime_failure: snapshot.last_runtime_failure,
        };
        deployment.validate_snapshot()?;
        Ok(deployment)
    }

    pub fn snapshot(&self) -> RuntimeDeploymentSnapshotV1 {
        RuntimeDeploymentSnapshotV1 {
            identity: self.identity.clone(),
            target: self.target.clone(),
            runtime_generation: self.runtime_generation,
            previous_runtime: self.previous_runtime.clone(),
            requested_at: self.requested_at,
            revision: self.revision,
            phase: self.phase.clone(),
            controller_lease: self.controller_lease.clone(),
            last_fencing_token: self.last_fencing_token,
            preflight: self.preflight.clone(),
            drain: self.drain.clone(),
            activation: self.activation.clone(),
            panel_certificate: self.panel_certificate.clone(),
            gateway_ready: self.gateway_ready.clone(),
            live: self.live.clone(),
            last_live_recovery: self.last_live_recovery.clone(),
            last_runtime_failure: self.last_runtime_failure.clone(),
        }
    }

    pub fn id(&self) -> &DeploymentId {
        &self.identity.deployment_id
    }

    pub fn identity(&self) -> &RuntimeDeploymentIdentityV1 {
        &self.identity
    }

    pub fn target(&self) -> &RuntimeDeploymentTargetV1 {
        &self.target
    }

    pub fn runtime_generation(&self) -> RuntimeGeneration {
        self.runtime_generation
    }

    pub fn revision(&self) -> DeploymentRevision {
        self.revision
    }

    pub fn phase(&self) -> &RuntimeDeploymentPhaseV1 {
        &self.phase
    }

    pub fn controller_lease(&self) -> Option<&ControllerLeaseV1> {
        self.controller_lease.as_ref()
    }

    pub fn live_attestation(&self) -> Option<&LiveAttestationV1> {
        self.live.as_ref()
    }

    pub fn last_live_recovery(&self) -> Option<&LiveRecoveryAttestationV1> {
        self.last_live_recovery.as_ref()
    }

    pub fn acquire_lease(
        &mut self,
        request: LeaseRequestV1,
    ) -> Result<TransitionOutcomeV1, RuntimeDeploymentError> {
        if let Some(lease) = &self.controller_lease {
            if lease.controller_id == request.controller_id
                && lease.fencing_token == request.fencing_token
                && lease.acquired_at == request.now
                && lease.expires_at == request.expires_at
            {
                return Ok(self.replayed());
            }
        }
        match &self.phase {
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Blocked { .. },
            } => return Err(self.invalid_transition("acquire_lease")),
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition:
                    RuntimePendingConditionV1::Retryable {
                        retry_not_before, ..
                    },
            } if *retry_not_before > request.now => {
                return Err(self.invalid_transition("acquire_lease"));
            }
            _ => {}
        }
        self.validate_lease_request(&request)?;
        self.install_lease(request);
        self.bump_revision()
    }

    pub fn recover_blocked(
        &mut self,
        request: RecoverBlockedRequestV1,
    ) -> Result<TransitionOutcomeV1, RuntimeDeploymentError> {
        let exact_failure = self
            .last_runtime_failure
            .as_ref()
            .is_some_and(|disposition| {
                matches!(
                    disposition,
                    RuntimeFailureDispositionV1::Blocked { failure }
                        if failure.failure_id == request.expected_failure_id
                )
            });
        if exact_failure
            && matches!(
                &self.phase,
                RuntimeDeploymentPhaseV1::RuntimePending {
                    condition: RuntimePendingConditionV1::Ready
                }
            )
            && self.controller_lease.as_ref().is_some_and(|lease| {
                lease.controller_id == request.controller_id
                    && lease.fencing_token == request.fencing_token
                    && lease.acquired_at == request.now
                    && lease.expires_at == request.expires_at
            })
        {
            return Ok(self.replayed());
        }
        self.require_revision(request.expected_revision)?;
        if !matches!(
            &self.phase,
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Blocked { failure }
            } if failure.failure_id == request.expected_failure_id
        ) {
            return Err(self.invalid_transition("recover_blocked"));
        }
        if self.controller_lease.is_some() || !exact_failure {
            return Err(RuntimeDeploymentError::InvalidSnapshot);
        }
        let lease = LeaseRequestV1 {
            expected_revision: request.expected_revision,
            controller_id: request.controller_id,
            fencing_token: request.fencing_token,
            now: request.now,
            expires_at: request.expires_at,
        };
        self.validate_lease_request(&lease)?;
        self.install_lease(lease);
        self.phase = RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Ready,
        };
        self.bump_revision()
    }

    fn validate_lease_request(
        &self,
        request: &LeaseRequestV1,
    ) -> Result<(), RuntimeDeploymentError> {
        self.require_revision(request.expected_revision)?;
        if self.phase.is_terminal() {
            return Err(self.invalid_transition("acquire_lease"));
        }
        if request.expires_at <= request.now {
            return Err(RuntimeDeploymentError::InvalidLeaseWindow);
        }
        if request.now < self.requested_at {
            return Err(RuntimeDeploymentError::InvalidLeaseWindow);
        }
        if let Some(lease) = &self.controller_lease {
            if lease.expires_at > request.now {
                if lease.controller_id != request.controller_id {
                    return Err(RuntimeDeploymentError::LeaseHeld {
                        controller_id: lease.controller_id.clone(),
                        expires_at: lease.expires_at,
                    });
                }
                if request.now < lease.acquired_at || request.expires_at <= lease.expires_at {
                    return Err(RuntimeDeploymentError::InvalidLeaseWindow);
                }
            }
        }
        if self
            .last_fencing_token
            .is_some_and(|token| request.fencing_token <= token)
        {
            return Err(RuntimeDeploymentError::FencingTokenNotMonotonic);
        }
        Ok(())
    }

    fn install_lease(&mut self, request: LeaseRequestV1) {
        self.controller_lease = Some(ControllerLeaseV1 {
            controller_id: request.controller_id,
            fencing_token: request.fencing_token,
            acquired_at: request.now,
            expires_at: request.expires_at,
        });
        self.last_fencing_token = Some(request.fencing_token);
    }

    pub fn accept_preflight(
        &mut self,
        guard: &CommandGuardV1,
        attestation: PreflightAttestationV1,
    ) -> Result<TransitionOutcomeV1, RuntimeDeploymentError> {
        if self.preflight.as_ref() == Some(&attestation)
            && self.has_reached(RuntimeDeploymentPhaseKindV1::PreflightReady)
        {
            return Ok(self.replayed());
        }
        self.require_guard(guard)?;
        self.require_phase(RuntimeDeploymentPhaseKindV1::Requested, "accept_preflight")?;
        self.validate_preflight(&attestation)?;
        self.preflight = Some(attestation);
        self.phase = RuntimeDeploymentPhaseV1::PreflightReady;
        self.bump_revision()
    }

    pub fn request_drain(
        &mut self,
        guard: &CommandGuardV1,
    ) -> Result<TransitionOutcomeV1, RuntimeDeploymentError> {
        if self.has_reached(RuntimeDeploymentPhaseKindV1::DrainRequested) {
            return Ok(self.replayed());
        }
        self.require_guard(guard)?;
        self.require_phase(
            RuntimeDeploymentPhaseKindV1::PreflightReady,
            "request_drain",
        )?;
        self.phase = RuntimeDeploymentPhaseV1::DrainRequested;
        self.bump_revision()
    }

    pub fn accept_drain(
        &mut self,
        guard: &CommandGuardV1,
        attestation: DrainAttestationV1,
    ) -> Result<TransitionOutcomeV1, RuntimeDeploymentError> {
        if self.drain.as_ref() == Some(&attestation)
            && self.has_reached(RuntimeDeploymentPhaseKindV1::Drained)
        {
            return Ok(self.replayed());
        }
        self.require_guard(guard)?;
        self.require_phase(RuntimeDeploymentPhaseKindV1::DrainRequested, "accept_drain")?;
        self.validate_drain(&attestation)?;
        self.drain = Some(attestation);
        self.phase = RuntimeDeploymentPhaseV1::Drained;
        self.bump_revision()
    }

    pub fn begin_activation(
        &mut self,
        guard: &CommandGuardV1,
    ) -> Result<TransitionOutcomeV1, RuntimeDeploymentError> {
        if self.has_reached(RuntimeDeploymentPhaseKindV1::ActivationApplying) {
            return Ok(self.replayed());
        }
        self.require_guard(guard)?;
        self.require_phase(RuntimeDeploymentPhaseKindV1::Drained, "begin_activation")?;
        self.phase = RuntimeDeploymentPhaseV1::ActivationApplying;
        self.bump_revision()
    }

    pub fn accept_activation(
        &mut self,
        guard: &CommandGuardV1,
        attestation: ActivationAttestationV1,
    ) -> Result<TransitionOutcomeV1, RuntimeDeploymentError> {
        if self.activation.as_ref() == Some(&attestation)
            && self.has_reached(RuntimeDeploymentPhaseKindV1::RuntimePending)
        {
            return Ok(self.replayed());
        }
        self.require_guard(guard)?;
        self.require_phase(
            RuntimeDeploymentPhaseKindV1::ActivationApplying,
            "accept_activation",
        )?;
        self.validate_activation(&attestation)?;
        self.activation = Some(attestation);
        self.phase = RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Ready,
        };
        self.bump_revision()
    }

    pub fn record_retryable_failure(
        &mut self,
        guard: &CommandGuardV1,
        failure: RuntimeFailureV1,
        attempt: NonZeroU32,
        retry_not_before: DateTime<Utc>,
    ) -> Result<TransitionOutcomeV1, RuntimeDeploymentError> {
        let condition = RuntimePendingConditionV1::Retryable {
            failure: failure.clone(),
            attempt,
            retry_not_before,
        };
        if matches!(
            &self.phase,
            RuntimeDeploymentPhaseV1::RuntimePending { condition: current } if current == &condition
        ) {
            return Ok(self.replayed());
        }
        self.require_guard(guard)?;
        self.require_runtime_failure_phase("record_retryable_failure")?;
        Self::validate_failure(&failure)?;
        if retry_not_before < failure.recorded_at
            || self
                .runtime_evidence_floor()
                .is_none_or(|floor| failure.recorded_at < floor)
        {
            return Err(RuntimeDeploymentError::InvalidFailure);
        }
        let disposition = RuntimeFailureDispositionV1::Retryable {
            failure,
            attempt,
            retry_not_before,
        };
        self.last_runtime_failure = Some(disposition);
        self.clear_unaccepted_runtime_evidence();
        self.controller_lease = None;
        self.phase = RuntimeDeploymentPhaseV1::RuntimePending { condition };
        self.bump_revision()
    }

    pub fn record_blocked_failure(
        &mut self,
        guard: &CommandGuardV1,
        failure: RuntimeFailureV1,
    ) -> Result<TransitionOutcomeV1, RuntimeDeploymentError> {
        let condition = RuntimePendingConditionV1::Blocked {
            failure: failure.clone(),
        };
        if matches!(
            &self.phase,
            RuntimeDeploymentPhaseV1::RuntimePending { condition: current } if current == &condition
        ) {
            return Ok(self.replayed());
        }
        self.require_guard(guard)?;
        self.require_runtime_failure_phase("record_blocked_failure")?;
        Self::validate_failure(&failure)?;
        if self
            .runtime_evidence_floor()
            .is_none_or(|floor| failure.recorded_at < floor)
        {
            return Err(RuntimeDeploymentError::InvalidFailure);
        }
        self.last_runtime_failure = Some(RuntimeFailureDispositionV1::Blocked { failure });
        self.clear_unaccepted_runtime_evidence();
        self.controller_lease = None;
        self.phase = RuntimeDeploymentPhaseV1::RuntimePending { condition };
        self.bump_revision()
    }

    pub fn resume_runtime_pending(
        &mut self,
        guard: &CommandGuardV1,
    ) -> Result<TransitionOutcomeV1, RuntimeDeploymentError> {
        if matches!(
            &self.phase,
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Ready
            }
        ) {
            return Ok(self.replayed());
        }
        self.require_guard(guard)?;
        if !matches!(&self.phase, RuntimeDeploymentPhaseV1::RuntimePending { .. }) {
            return Err(self.invalid_transition("resume_runtime_pending"));
        }
        self.phase = RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Ready,
        };
        self.bump_revision()
    }

    pub fn begin_panel_reconciliation(
        &mut self,
        guard: &CommandGuardV1,
    ) -> Result<TransitionOutcomeV1, RuntimeDeploymentError> {
        if self.has_reached(RuntimeDeploymentPhaseKindV1::ReconcilingPanels) {
            return Ok(self.replayed());
        }
        self.require_guard(guard)?;
        match &self.phase {
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Ready,
            } => {
                self.phase = RuntimeDeploymentPhaseV1::ReconcilingPanels;
                self.bump_revision()
            }
            _ => Err(self.invalid_transition("begin_panel_reconciliation")),
        }
    }

    pub fn accept_panel_certificate(
        &mut self,
        guard: &CommandGuardV1,
        certificate: PanelCertificateV1,
    ) -> Result<TransitionOutcomeV1, RuntimeDeploymentError> {
        if self.panel_certificate.as_ref() == Some(&certificate)
            && self.has_reached(RuntimeDeploymentPhaseKindV1::AwaitingGatewayReady)
        {
            return Ok(self.replayed());
        }
        self.require_guard(guard)?;
        self.require_phase(
            RuntimeDeploymentPhaseKindV1::ReconcilingPanels,
            "accept_panel_certificate",
        )?;
        self.validate_panel_certificate(&certificate)?;
        self.panel_certificate = Some(certificate);
        self.phase = RuntimeDeploymentPhaseV1::AwaitingGatewayReady;
        self.bump_revision()
    }

    pub fn certify_live(
        &mut self,
        guard: &CommandGuardV1,
        gateway_ready: GatewayReadyAttestationV1,
        certified_at: DateTime<Utc>,
    ) -> Result<TransitionOutcomeV1, RuntimeDeploymentError> {
        if let Some(live) = &self.live {
            if live.gateway_ready == gateway_ready && live.certified_at == certified_at {
                return Ok(self.replayed());
            }
        }
        self.require_guard(guard)?;
        self.require_phase(
            RuntimeDeploymentPhaseKindV1::AwaitingGatewayReady,
            "certify_live",
        )?;
        self.validate_gateway_ready(&gateway_ready)?;
        let activation = self
            .activation
            .clone()
            .ok_or(RuntimeDeploymentError::InvalidSnapshot)?;
        let panel_certificate = self
            .panel_certificate
            .clone()
            .ok_or(RuntimeDeploymentError::InvalidSnapshot)?;
        if gateway_ready.ready_at < panel_certificate.reconciled_at
            || certified_at < gateway_ready.ready_at
        {
            return Err(RuntimeDeploymentError::AttestationTimeRegression);
        }
        let live = LiveAttestationV1 {
            target: self.target.clone(),
            runtime_generation: self.runtime_generation,
            process_instance_id: gateway_ready.process_instance_id.clone(),
            activation,
            panel_certificate,
            gateway_ready: gateway_ready.clone(),
            certified_at,
        };
        self.gateway_ready = Some(gateway_ready);
        self.live = Some(live);
        self.controller_lease = None;
        self.phase = RuntimeDeploymentPhaseV1::Live;
        self.bump_revision()
    }

    pub fn recover_live(
        &mut self,
        request: RecoverLiveRequestV1,
    ) -> Result<TransitionOutcomeV1, RuntimeDeploymentError> {
        if self.last_live_recovery.as_ref().is_some_and(|recovery| {
            recovery.prior_live.runtime_generation == request.expected_runtime_generation
                && recovery.prior_live.process_instance_id == request.expected_process_instance_id
                && recovery.kind == request.kind
                && recovery.evidence_at == request.evidence_at
                && recovery.recovered_at == request.recovered_at
        }) && !matches!(self.phase, RuntimeDeploymentPhaseV1::Live)
        {
            return Ok(self.replayed());
        }
        self.require_revision(request.expected_revision)?;
        self.require_phase(RuntimeDeploymentPhaseKindV1::Live, "recover_live")?;
        if request.expected_runtime_generation != self.runtime_generation {
            return Err(RuntimeDeploymentError::RuntimeGenerationConflict {
                expected: self.runtime_generation,
                actual: request.expected_runtime_generation,
            });
        }
        let prior_live = self
            .live
            .clone()
            .ok_or(RuntimeDeploymentError::InvalidSnapshot)?;
        if prior_live.process_instance_id != request.expected_process_instance_id {
            return Err(RuntimeDeploymentError::ProcessInstanceMismatch);
        }
        if request.evidence_at < prior_live.certified_at
            || request.recovered_at < request.evidence_at
        {
            return Err(RuntimeDeploymentError::AttestationTimeRegression);
        }
        self.last_live_recovery = Some(LiveRecoveryAttestationV1 {
            prior_live,
            kind: request.kind,
            evidence_at: request.evidence_at,
            recovered_at: request.recovered_at,
        });
        self.clear_unaccepted_runtime_evidence();
        self.phase = RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Ready,
        };
        self.bump_revision()
    }

    pub fn supersede(
        &mut self,
        guard: &CommandGuardV1,
        by: SupersedingDeploymentV1,
        reason: String,
        superseded_at: DateTime<Utc>,
    ) -> Result<TransitionOutcomeV1, RuntimeDeploymentError> {
        if matches!(
            &self.phase,
            RuntimeDeploymentPhaseV1::Superseded {
                by: current,
                reason: current_reason,
                superseded_at: current_at
            } if current == &by && current_reason == &reason && *current_at == superseded_at
        ) {
            return Ok(self.replayed());
        }
        self.require_guard(guard)?;
        if self.phase.is_terminal() {
            return Err(self.invalid_transition("supersede"));
        }
        self.validate_supersession(&by, &reason, superseded_at)?;
        self.phase = RuntimeDeploymentPhaseV1::Superseded {
            by,
            reason,
            superseded_at,
        };
        self.controller_lease = None;
        self.bump_revision()
    }

    pub fn cancel(
        &mut self,
        guard: &CommandGuardV1,
        reason: String,
        cancelled_at: DateTime<Utc>,
    ) -> Result<TransitionOutcomeV1, RuntimeDeploymentError> {
        if matches!(
            &self.phase,
            RuntimeDeploymentPhaseV1::Cancelled {
                reason: current_reason,
                cancelled_at: current_at
            } if current_reason == &reason && *current_at == cancelled_at
        ) {
            return Ok(self.replayed());
        }
        self.require_guard(guard)?;
        if !matches!(
            self.phase.kind(),
            RuntimeDeploymentPhaseKindV1::Requested
                | RuntimeDeploymentPhaseKindV1::PreflightReady
                | RuntimeDeploymentPhaseKindV1::DrainRequested
        ) {
            return Err(self.invalid_transition("cancel"));
        }
        Self::validate_reason(&reason)?;
        if cancelled_at < self.requested_at {
            return Err(RuntimeDeploymentError::AttestationTimeRegression);
        }
        self.phase = RuntimeDeploymentPhaseV1::Cancelled {
            reason,
            cancelled_at,
        };
        self.controller_lease = None;
        self.bump_revision()
    }

    fn require_revision(&self, expected: DeploymentRevision) -> Result<(), RuntimeDeploymentError> {
        if expected != self.revision {
            return Err(RuntimeDeploymentError::RevisionConflict {
                expected,
                actual: self.revision,
            });
        }
        Ok(())
    }

    fn require_guard(&self, guard: &CommandGuardV1) -> Result<(), RuntimeDeploymentError> {
        self.require_revision(guard.expected_revision)?;
        if guard.runtime_generation != self.runtime_generation {
            return Err(RuntimeDeploymentError::RuntimeGenerationConflict {
                expected: self.runtime_generation,
                actual: guard.runtime_generation,
            });
        }
        let lease = self
            .controller_lease
            .as_ref()
            .ok_or(RuntimeDeploymentError::LeaseRequired)?;
        if lease.expires_at <= guard.now {
            return Err(RuntimeDeploymentError::LeaseExpired {
                expires_at: lease.expires_at,
            });
        }
        if lease.controller_id != guard.controller_id {
            return Err(RuntimeDeploymentError::ControllerMismatch);
        }
        if lease.fencing_token != guard.fencing_token {
            return Err(RuntimeDeploymentError::FencingTokenConflict {
                expected: lease.fencing_token,
                actual: guard.fencing_token,
            });
        }
        Ok(())
    }

    fn require_phase(
        &self,
        expected: RuntimeDeploymentPhaseKindV1,
        operation: &'static str,
    ) -> Result<(), RuntimeDeploymentError> {
        if self.phase.kind() != expected {
            return Err(self.invalid_transition(operation));
        }
        Ok(())
    }

    fn require_runtime_failure_phase(
        &self,
        operation: &'static str,
    ) -> Result<(), RuntimeDeploymentError> {
        if matches!(
            &self.phase,
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Ready
            } | RuntimeDeploymentPhaseV1::ReconcilingPanels
                | RuntimeDeploymentPhaseV1::AwaitingGatewayReady
        ) {
            Ok(())
        } else {
            Err(self.invalid_transition(operation))
        }
    }

    fn clear_unaccepted_runtime_evidence(&mut self) {
        self.panel_certificate = None;
        self.gateway_ready = None;
        self.live = None;
    }

    fn runtime_evidence_floor(&self) -> Option<DateTime<Utc>> {
        let activated_at = self
            .activation
            .as_ref()
            .map(|activation| activation.activated_at)?;
        Some(
            self.last_live_recovery
                .as_ref()
                .map_or(activated_at, |recovery| {
                    activated_at.max(recovery.recovered_at)
                }),
        )
    }

    fn has_reached(&self, expected: RuntimeDeploymentPhaseKindV1) -> bool {
        let current = Self::phase_rank(self.phase.kind());
        let expected = Self::phase_rank(expected);
        current.is_some() && expected.is_some() && current >= expected
    }

    fn phase_rank(kind: RuntimeDeploymentPhaseKindV1) -> Option<u8> {
        match kind {
            RuntimeDeploymentPhaseKindV1::Requested => Some(0),
            RuntimeDeploymentPhaseKindV1::PreflightReady => Some(1),
            RuntimeDeploymentPhaseKindV1::DrainRequested => Some(2),
            RuntimeDeploymentPhaseKindV1::Drained => Some(3),
            RuntimeDeploymentPhaseKindV1::ActivationApplying => Some(4),
            RuntimeDeploymentPhaseKindV1::RuntimePending => Some(5),
            RuntimeDeploymentPhaseKindV1::ReconcilingPanels => Some(6),
            RuntimeDeploymentPhaseKindV1::AwaitingGatewayReady => Some(7),
            RuntimeDeploymentPhaseKindV1::Live => Some(8),
            RuntimeDeploymentPhaseKindV1::Superseded | RuntimeDeploymentPhaseKindV1::Cancelled => {
                None
            }
        }
    }

    fn bump_revision(&mut self) -> Result<TransitionOutcomeV1, RuntimeDeploymentError> {
        self.revision = self
            .revision
            .next()
            .map_err(|_| RuntimeDeploymentError::RevisionOverflow)?;
        Ok(TransitionOutcomeV1::Applied {
            revision: self.revision,
        })
    }

    fn replayed(&self) -> TransitionOutcomeV1 {
        TransitionOutcomeV1::Replayed {
            revision: self.revision,
        }
    }

    fn invalid_transition(&self, operation: &'static str) -> RuntimeDeploymentError {
        RuntimeDeploymentError::InvalidTransition {
            current: self.phase.kind(),
            operation,
        }
    }
}
