use chrono::{DateTime, Utc};

use crate::{
    ActivationAttestationV1, DrainAttestationV1, GatewayReadyAttestationV1, PanelCertificateV1,
    PanelIneligibilityV1, PreflightAttestationV1, RuntimeDeploymentError, RuntimeDeploymentPhaseV1,
    RuntimeDeploymentTargetV1, RuntimeFailureDispositionV1, RuntimeFailureV1, RuntimeGeneration,
    RuntimePendingConditionV1, SupersedingDeploymentV1,
};

use super::RuntimeDeployment;

impl RuntimeDeployment {
    pub(super) fn validate_preflight(
        &self,
        attestation: &PreflightAttestationV1,
    ) -> Result<(), RuntimeDeploymentError> {
        self.validate_target_and_generation(&attestation.target, attestation.runtime_generation)?;
        if attestation.observed_runtime != self.previous_runtime {
            return Err(RuntimeDeploymentError::PreviousRuntimeMismatch);
        }
        if attestation.checked_at < self.requested_at {
            return Err(RuntimeDeploymentError::AttestationTimeRegression);
        }
        Ok(())
    }

    pub(super) fn validate_drain(
        &self,
        attestation: &DrainAttestationV1,
    ) -> Result<(), RuntimeDeploymentError> {
        self.validate_generation(attestation.target_runtime_generation)?;
        if attestation.previous_runtime != self.previous_runtime {
            return Err(RuntimeDeploymentError::PreviousRuntimeMismatch);
        }
        let preflight = self
            .preflight
            .as_ref()
            .ok_or(RuntimeDeploymentError::InvalidSnapshot)?;
        if attestation.drained_at < preflight.checked_at {
            return Err(RuntimeDeploymentError::AttestationTimeRegression);
        }
        Ok(())
    }

    pub(super) fn validate_activation(
        &self,
        attestation: &ActivationAttestationV1,
    ) -> Result<(), RuntimeDeploymentError> {
        self.validate_target_and_generation(&attestation.target, attestation.runtime_generation)?;
        if attestation.activation_request_id != self.identity.activation_request_id {
            return Err(RuntimeDeploymentError::ActivationRequestMismatch);
        }
        let drain = self
            .drain
            .as_ref()
            .ok_or(RuntimeDeploymentError::InvalidSnapshot)?;
        if attestation.activated_at < drain.drained_at {
            return Err(RuntimeDeploymentError::AttestationTimeRegression);
        }
        Ok(())
    }

    pub(super) fn validate_panel_certificate(
        &self,
        certificate: &PanelCertificateV1,
    ) -> Result<(), RuntimeDeploymentError> {
        self.validate_target_and_generation(&certificate.target, certificate.runtime_generation)?;
        let activation = self
            .activation
            .as_ref()
            .ok_or(RuntimeDeploymentError::InvalidSnapshot)?;
        if certificate.reconciled_at < activation.activated_at {
            return Err(RuntimeDeploymentError::AttestationTimeRegression);
        }
        if let Some(recovery) = &self.last_live_recovery {
            if certificate.reconciled_at < recovery.recovered_at {
                return Err(RuntimeDeploymentError::AttestationTimeRegression);
            }
            if certificate.process_instance_id == recovery.prior_live.process_instance_id {
                return Err(RuntimeDeploymentError::ProcessInstanceMismatch);
            }
        }
        Self::validate_panel_eligibility(certificate)
    }

    pub(super) fn validate_gateway_ready(
        &self,
        attestation: &GatewayReadyAttestationV1,
    ) -> Result<(), RuntimeDeploymentError> {
        self.validate_target_and_generation(&attestation.target, attestation.runtime_generation)?;
        let panel = self
            .panel_certificate
            .as_ref()
            .ok_or(RuntimeDeploymentError::InvalidSnapshot)?;
        if attestation.process_instance_id != panel.process_instance_id {
            return Err(RuntimeDeploymentError::ProcessInstanceMismatch);
        }
        Ok(())
    }

    fn validate_target_and_generation(
        &self,
        target: &RuntimeDeploymentTargetV1,
        runtime_generation: RuntimeGeneration,
    ) -> Result<(), RuntimeDeploymentError> {
        if target != &self.target {
            return Err(RuntimeDeploymentError::TargetMismatch);
        }
        self.validate_generation(runtime_generation)
    }

    fn validate_generation(
        &self,
        runtime_generation: RuntimeGeneration,
    ) -> Result<(), RuntimeDeploymentError> {
        if runtime_generation != self.runtime_generation {
            return Err(RuntimeDeploymentError::RuntimeGenerationConflict {
                expected: self.runtime_generation,
                actual: runtime_generation,
            });
        }
        Ok(())
    }

    fn validate_panel_eligibility(
        certificate: &PanelCertificateV1,
    ) -> Result<(), RuntimeDeploymentError> {
        if certificate.skipped_transient_count != 0 {
            return Err(RuntimeDeploymentError::PanelIneligible(
                PanelIneligibilityV1::TransientSkipped,
            ));
        }
        if certificate.skipped_unresolved_channel_count != 0 {
            return Err(RuntimeDeploymentError::PanelIneligible(
                PanelIneligibilityV1::UnresolvedChannelSkipped,
            ));
        }
        if certificate.failed_count != 0 {
            return Err(RuntimeDeploymentError::PanelIneligible(
                PanelIneligibilityV1::Failed,
            ));
        }
        if certificate.ambiguous_outcome_count != 0 {
            return Err(RuntimeDeploymentError::PanelIneligible(
                PanelIneligibilityV1::AmbiguousOutcome,
            ));
        }
        if certificate.stale_message_cleanup_pending_count != 0 {
            return Err(RuntimeDeploymentError::PanelIneligible(
                PanelIneligibilityV1::StaleCleanupPending,
            ));
        }
        if certificate.orphan_message_cleanup_pending_count != 0 {
            return Err(RuntimeDeploymentError::PanelIneligible(
                PanelIneligibilityV1::OrphanCleanupPending,
            ));
        }
        if certificate.reposted_old_message_cleanup_pending_count != 0 {
            return Err(RuntimeDeploymentError::PanelIneligible(
                PanelIneligibilityV1::RepostedOldMessageCleanupPending,
            ));
        }
        let accounted = certificate
            .installed_count
            .checked_add(certificate.unchanged_count)
            .ok_or(RuntimeDeploymentError::PanelIneligible(
                PanelIneligibilityV1::CountOverflow,
            ))?;
        if accounted != certificate.declared_count {
            return Err(RuntimeDeploymentError::PanelIneligible(
                PanelIneligibilityV1::Incomplete,
            ));
        }
        Ok(())
    }

    pub(super) fn validate_failure(
        failure: &RuntimeFailureV1,
    ) -> Result<(), RuntimeDeploymentError> {
        let code_valid = !failure.code.is_empty()
            && failure.code.len() <= 64
            && failure
                .code
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
        let message_valid = !failure.message.trim().is_empty() && failure.message.len() <= 1024;
        if code_valid && message_valid {
            Ok(())
        } else {
            Err(RuntimeDeploymentError::InvalidFailure)
        }
    }

    pub(super) fn validate_reason(reason: &str) -> Result<(), RuntimeDeploymentError> {
        if reason.trim().is_empty() || reason.len() > 1024 {
            Err(RuntimeDeploymentError::InvalidReason)
        } else {
            Ok(())
        }
    }

    pub(super) fn validate_supersession(
        &self,
        by: &SupersedingDeploymentV1,
        reason: &str,
        superseded_at: DateTime<Utc>,
    ) -> Result<(), RuntimeDeploymentError> {
        Self::validate_reason(reason)?;
        if by.runtime_generation <= self.runtime_generation {
            return Err(RuntimeDeploymentError::RuntimeGenerationNotMonotonic);
        }
        if by.identity.deployment_id == self.identity.deployment_id {
            return Err(RuntimeDeploymentError::SupersedingDeploymentIdentityConflict);
        }
        if !by.identity.same_product_scope(&self.identity) {
            return Err(RuntimeDeploymentError::SupersedingDeploymentScopeMismatch);
        }
        if !by.target.same_slot(&self.target) {
            return Err(RuntimeDeploymentError::PreviousRuntimeSlotMismatch);
        }
        if superseded_at < self.requested_at {
            return Err(RuntimeDeploymentError::AttestationTimeRegression);
        }
        Ok(())
    }

    pub(super) fn validate_snapshot(&self) -> Result<(), RuntimeDeploymentError> {
        if let Some(previous) = &self.previous_runtime {
            if !self.target.same_slot(&previous.target)
                || self.runtime_generation <= previous.runtime_generation
            {
                return Err(RuntimeDeploymentError::InvalidSnapshot);
            }
        }
        match (&self.controller_lease, self.last_fencing_token) {
            (Some(lease), Some(last))
                if lease.fencing_token == last && lease.expires_at > lease.acquired_at => {}
            (None, _) => {}
            _ => return Err(RuntimeDeploymentError::InvalidSnapshot),
        }
        if self.phase.is_terminal() && self.controller_lease.is_some() {
            return Err(RuntimeDeploymentError::InvalidSnapshot);
        }
        if let Some(lease) = &self.controller_lease {
            if lease.acquired_at < self.requested_at {
                return Err(RuntimeDeploymentError::InvalidSnapshot);
            }
        }
        if let Some(disposition) = &self.last_runtime_failure {
            let (failure, retry_not_before) = match disposition {
                RuntimeFailureDispositionV1::Retryable {
                    failure,
                    retry_not_before,
                    ..
                } => (failure, Some(*retry_not_before)),
                RuntimeFailureDispositionV1::Blocked { failure } => (failure, None),
            };
            let failure_is_current = matches!(
                &self.phase,
                RuntimeDeploymentPhaseV1::RuntimePending { condition }
                    if condition.disposition().as_ref() == Some(disposition)
            );
            let evidence_floor = if failure_is_current {
                self.runtime_evidence_floor()
            } else {
                self.activation
                    .as_ref()
                    .map(|activation| activation.activated_at)
            };
            if Self::validate_failure(failure).is_err()
                || failure.recorded_at < self.requested_at
                || evidence_floor.is_none_or(|floor| failure.recorded_at < floor)
                || retry_not_before.is_some_and(|retry_at| retry_at < failure.recorded_at)
            {
                return Err(RuntimeDeploymentError::InvalidSnapshot);
            }
        }
        if let Some(preflight) = &self.preflight {
            self.validate_preflight(preflight)
                .map_err(|_| RuntimeDeploymentError::InvalidSnapshot)?;
        }
        if let Some(drain) = &self.drain {
            if self.preflight.is_none() {
                return Err(RuntimeDeploymentError::InvalidSnapshot);
            }
            self.validate_drain(drain)
                .map_err(|_| RuntimeDeploymentError::InvalidSnapshot)?;
        }
        if let Some(activation) = &self.activation {
            if self.drain.is_none() {
                return Err(RuntimeDeploymentError::InvalidSnapshot);
            }
            self.validate_activation(activation)
                .map_err(|_| RuntimeDeploymentError::InvalidSnapshot)?;
        }
        if let Some(panel) = &self.panel_certificate {
            if self.activation.is_none() {
                return Err(RuntimeDeploymentError::InvalidSnapshot);
            }
            self.validate_panel_certificate(panel)
                .map_err(|_| RuntimeDeploymentError::InvalidSnapshot)?;
        }
        if let Some(gateway) = &self.gateway_ready {
            if self.panel_certificate.is_none() {
                return Err(RuntimeDeploymentError::InvalidSnapshot);
            }
            self.validate_gateway_ready(gateway)
                .map_err(|_| RuntimeDeploymentError::InvalidSnapshot)?;
        }
        if let Some(live) = &self.live {
            if self.gateway_ready.is_none()
                || live.target != self.target
                || live.runtime_generation != self.runtime_generation
                || self.activation.as_ref() != Some(&live.activation)
                || self.panel_certificate.as_ref() != Some(&live.panel_certificate)
                || self.gateway_ready.as_ref() != Some(&live.gateway_ready)
                || live.process_instance_id != live.gateway_ready.process_instance_id
                || live.certified_at < live.gateway_ready.ready_at
            {
                return Err(RuntimeDeploymentError::InvalidSnapshot);
            }
        }
        if let Some(recovery) = &self.last_live_recovery {
            let prior = &recovery.prior_live;
            let recovery_phase_valid = matches!(
                self.phase,
                RuntimeDeploymentPhaseV1::RuntimePending { .. }
                    | RuntimeDeploymentPhaseV1::ReconcilingPanels
                    | RuntimeDeploymentPhaseV1::AwaitingGatewayReady
                    | RuntimeDeploymentPhaseV1::Live
                    | RuntimeDeploymentPhaseV1::Superseded { .. }
            );
            if !recovery_phase_valid
                || prior.target != self.target
                || prior.runtime_generation != self.runtime_generation
                || self.activation.as_ref() != Some(&prior.activation)
                || prior.panel_certificate.target != self.target
                || prior.panel_certificate.runtime_generation != self.runtime_generation
                || prior.gateway_ready.target != self.target
                || prior.gateway_ready.runtime_generation != self.runtime_generation
                || prior.process_instance_id != prior.panel_certificate.process_instance_id
                || prior.process_instance_id != prior.gateway_ready.process_instance_id
                || prior.panel_certificate.reconciled_at < prior.activation.activated_at
                || prior.gateway_ready.ready_at < prior.panel_certificate.reconciled_at
                || prior.certified_at < prior.gateway_ready.ready_at
                || Self::validate_panel_eligibility(&prior.panel_certificate).is_err()
                || recovery.evidence_at < recovery.prior_live.certified_at
                || recovery.recovered_at < recovery.evidence_at
                || self
                    .live
                    .as_ref()
                    .is_some_and(|live| live.certified_at < recovery.recovered_at)
            {
                return Err(RuntimeDeploymentError::InvalidSnapshot);
            }
        }
        self.validate_phase_evidence()
    }

    fn validate_phase_evidence(&self) -> Result<(), RuntimeDeploymentError> {
        let evidence = (
            self.preflight.is_some(),
            self.drain.is_some(),
            self.activation.is_some(),
            self.panel_certificate.is_some(),
            self.gateway_ready.is_some(),
            self.live.is_some(),
        );
        let valid = match &self.phase {
            RuntimeDeploymentPhaseV1::Requested => {
                evidence == (false, false, false, false, false, false)
            }
            RuntimeDeploymentPhaseV1::PreflightReady | RuntimeDeploymentPhaseV1::DrainRequested => {
                evidence == (true, false, false, false, false, false)
            }
            RuntimeDeploymentPhaseV1::Drained | RuntimeDeploymentPhaseV1::ActivationApplying => {
                evidence == (true, true, false, false, false, false)
            }
            RuntimeDeploymentPhaseV1::RuntimePending { condition } => {
                let condition_valid = match condition {
                    RuntimePendingConditionV1::Ready => true,
                    RuntimePendingConditionV1::Retryable {
                        failure,
                        retry_not_before,
                        ..
                    } => {
                        Self::validate_failure(failure).is_ok()
                            && *retry_not_before >= failure.recorded_at
                            && condition.disposition() == self.last_runtime_failure
                    }
                    RuntimePendingConditionV1::Blocked { failure } => {
                        Self::validate_failure(failure).is_ok()
                            && condition.disposition() == self.last_runtime_failure
                    }
                };
                evidence == (true, true, true, false, false, false) && condition_valid
            }
            RuntimeDeploymentPhaseV1::ReconcilingPanels => {
                evidence == (true, true, true, false, false, false)
            }
            RuntimeDeploymentPhaseV1::AwaitingGatewayReady => {
                evidence == (true, true, true, true, false, false)
            }
            RuntimeDeploymentPhaseV1::Live => evidence == (true, true, true, true, true, true),
            RuntimeDeploymentPhaseV1::Superseded {
                by,
                reason,
                superseded_at,
            } => {
                Self::validate_reason(reason).is_ok()
                    && by.runtime_generation > self.runtime_generation
                    && by.identity.deployment_id != self.identity.deployment_id
                    && by.identity.same_product_scope(&self.identity)
                    && by.target.same_slot(&self.target)
                    && *superseded_at >= self.requested_at
                    && Self::evidence_is_prefix(evidence)
            }
            RuntimeDeploymentPhaseV1::Cancelled {
                reason,
                cancelled_at,
            } => {
                Self::validate_reason(reason).is_ok()
                    && *cancelled_at >= self.requested_at
                    && Self::evidence_is_prefix(evidence)
            }
        };
        if valid {
            Ok(())
        } else {
            Err(RuntimeDeploymentError::InvalidSnapshot)
        }
    }

    fn evidence_is_prefix(evidence: (bool, bool, bool, bool, bool, bool)) -> bool {
        matches!(
            evidence,
            (false, false, false, false, false, false)
                | (true, false, false, false, false, false)
                | (true, true, false, false, false, false)
                | (true, true, true, false, false, false)
                | (true, true, true, true, false, false)
        )
    }
}
