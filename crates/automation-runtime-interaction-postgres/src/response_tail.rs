use std::fmt::{Debug, Formatter};
use std::num::{NonZeroU16, NonZeroUsize};
use std::time::Duration;

use automation_runtime_interaction::{
    build_interaction_effect_planned_preimage_digest_v1,
    build_interaction_effect_preimage_digest_v1, build_interaction_effect_recovery_correlation_v1,
    build_interaction_effect_recovery_observation_digest_v1,
    validate_interaction_effect_recovery_observation_v1, EncryptedInteractionTokenV1,
    InteractionEffectAttemptV1, InteractionEffectCorrelationClassV1,
    InteractionEffectCorrelationDigestV1, InteractionEffectExpectedPostimageDigestV1,
    InteractionEffectIdentityDigestV1, InteractionEffectIntentDigestV1,
    InteractionEffectObservationOutcomeV1, InteractionEffectPayloadDigestV1,
    InteractionEffectPlannedIdentityDigestV1, InteractionEffectPlannedPreimageDigestV1,
    InteractionEffectPlannedPreimageV1, InteractionEffectPreimageDigestV1,
    InteractionEffectPreimageV1, InteractionEffectRecoveryBindingV1,
    InteractionEffectRecoveryTargetV1, InteractionEffectResultDigestV1, InteractionEffectStateV1,
    InteractionExpectedRouteV1, InteractionReceiptIdentityV1, InteractionReceiptStateV1,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::effect::{
    RuntimeInteractionEffectMutationDispositionV1, RuntimeInteractionEffectOriginV1,
    RuntimeInteractionEffectRecoveryBlockReasonV1, RuntimeInteractionEffectRecoveryScanCursorV1,
    RuntimeInteractionEffectRecoveryScanKeyV1,
};
use crate::receipt::{
    RuntimeInteractionReceiptClaimLeaseV1, RuntimeInteractionReceiptOpaqueDigestV1,
};
use crate::RuntimeInteractionPersistenceErrorV1;

pub type RuntimeInteractionEffectResponseTailScanCursorV1 =
    RuntimeInteractionEffectRecoveryScanCursorV1;
pub type RuntimeInteractionEffectResponseTailScanKeyV1 = RuntimeInteractionEffectRecoveryScanKeyV1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionEffectResponseTailRecoveryModeV1 {
    CloseKnown,
    Observe,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionEffectResponseTailCandidateV1 {
    pub(crate) key: RuntimeInteractionEffectResponseTailScanKeyV1,
    pub(crate) state: InteractionEffectStateV1,
    pub(crate) effect_head_revision: u64,
    pub(crate) recovery_claim_revision: u64,
    pub(crate) observation_attempt_count: u16,
    pub(crate) planned_identity_digest: InteractionEffectPlannedIdentityDigestV1,
    pub(crate) expected_postimage_digest: InteractionEffectExpectedPostimageDigestV1,
    pub(crate) payload_digest: InteractionEffectPayloadDigestV1,
    pub(crate) planned_preimage_digest: InteractionEffectPlannedPreimageDigestV1,
    pub(crate) resolved_preimage_digest: Option<InteractionEffectPreimageDigestV1>,
    pub(crate) resolved_effect_identity_digest: Option<InteractionEffectIdentityDigestV1>,
    pub(crate) correlation_digest: InteractionEffectCorrelationDigestV1,
    pub(crate) intent_digest: Option<InteractionEffectIntentDigestV1>,
    pub(crate) result_digest: Option<InteractionEffectResultDigestV1>,
    pub(crate) receipt_head_revision: u64,
    pub(crate) receipt_claim_revision: u64,
    pub(crate) receipt_claim_expires_at: DateTime<Utc>,
    pub(crate) token_expires_at: Option<DateTime<Utc>>,
    pub(crate) preflight_certificate_digest:
        automation_runtime_interaction::InteractionPreflightCertificateDigestV1,
    pub(crate) origin: RuntimeInteractionEffectOriginV1,
}

impl RuntimeInteractionEffectResponseTailCandidateV1 {
    pub fn key(&self) -> &RuntimeInteractionEffectResponseTailScanKeyV1 {
        &self.key
    }

    pub fn identity(&self) -> InteractionReceiptIdentityV1 {
        self.key.identity()
    }

    pub fn state(&self) -> InteractionEffectStateV1 {
        self.state
    }

    pub fn recovery_mode(&self) -> RuntimeInteractionEffectResponseTailRecoveryModeV1 {
        if matches!(
            self.state,
            InteractionEffectStateV1::Intended
                | InteractionEffectStateV1::Indeterminate
                | InteractionEffectStateV1::Observing
                | InteractionEffectStateV1::ObservationPending
        ) {
            RuntimeInteractionEffectResponseTailRecoveryModeV1::Observe
        } else {
            RuntimeInteractionEffectResponseTailRecoveryModeV1::CloseKnown
        }
    }

    pub fn effect_head_revision(&self) -> u64 {
        self.effect_head_revision
    }

    pub fn recovery_claim_revision(&self) -> u64 {
        self.recovery_claim_revision
    }

    pub fn receipt_head_revision(&self) -> u64 {
        self.receipt_head_revision
    }

    pub fn receipt_claim_revision(&self) -> u64 {
        self.receipt_claim_revision
    }

    pub fn receipt_claim_expires_at(&self) -> DateTime<Utc> {
        self.receipt_claim_expires_at
    }

    pub fn token_expires_at(&self) -> Option<DateTime<Utc>> {
        self.token_expires_at
    }

    pub fn expected_postimage_digest(&self) -> &InteractionEffectExpectedPostimageDigestV1 {
        &self.expected_postimage_digest
    }

    pub fn payload_digest(&self) -> &InteractionEffectPayloadDigestV1 {
        &self.payload_digest
    }

    pub fn preflight_certificate_digest(
        &self,
    ) -> &automation_runtime_interaction::InteractionPreflightCertificateDigestV1 {
        &self.preflight_certificate_digest
    }

    pub fn origin(&self) -> &RuntimeInteractionEffectOriginV1 {
        &self.origin
    }

    pub fn strict_recovery_binding_v1(
        &self,
    ) -> Result<InteractionEffectRecoveryBindingV1, RuntimeInteractionPersistenceErrorV1> {
        if self.recovery_mode() != RuntimeInteractionEffectResponseTailRecoveryModeV1::Observe {
            return Err(RuntimeInteractionPersistenceErrorV1::Conflict);
        }
        let resolved_identity = self
            .resolved_effect_identity_digest
            .clone()
            .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        let resolved_preimage = self
            .resolved_preimage_digest
            .as_ref()
            .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        let preimage = InteractionEffectPreimageV1::None;
        if build_interaction_effect_planned_preimage_digest_v1(
            &InteractionEffectPlannedPreimageV1::None,
        ) != self.planned_preimage_digest
            || build_interaction_effect_preimage_digest_v1(&preimage) != *resolved_preimage
        {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        let correlation = build_interaction_effect_recovery_correlation_v1(
            &self.planned_identity_digest,
            InteractionEffectCorrelationClassV1::InteractionReceipt,
        );
        if correlation.marker_digest() != &self.correlation_digest {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        InteractionEffectRecoveryBindingV1::new(
            InteractionEffectRecoveryTargetV1::EditResponse {
                receipt_identity: self.identity(),
                payload_digest: self.payload_digest.clone(),
            },
            preimage,
            self.planned_identity_digest.clone(),
            resolved_identity,
            self.expected_postimage_digest.clone(),
            correlation,
        )
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
    }
}

impl Debug for RuntimeInteractionEffectResponseTailCandidateV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectResponseTailCandidateV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionEffectResponseTailScanPageV1 {
    candidates: Vec<RuntimeInteractionEffectResponseTailCandidateV1>,
    through: Option<RuntimeInteractionEffectResponseTailScanKeyV1>,
    observed_database_now: Option<DateTime<Utc>>,
    requested_limit: NonZeroUsize,
}

impl RuntimeInteractionEffectResponseTailScanPageV1 {
    pub(crate) fn new(
        candidates: Vec<RuntimeInteractionEffectResponseTailCandidateV1>,
        through: Option<RuntimeInteractionEffectResponseTailScanKeyV1>,
        observed_database_now: Option<DateTime<Utc>>,
        requested_limit: NonZeroUsize,
    ) -> Self {
        Self {
            candidates,
            through,
            observed_database_now,
            requested_limit,
        }
    }

    pub fn candidates(&self) -> &[RuntimeInteractionEffectResponseTailCandidateV1] {
        &self.candidates
    }

    pub fn observed_database_now(&self) -> Option<DateTime<Utc>> {
        self.observed_database_now
    }

    pub fn next_cursor(&self) -> Option<RuntimeInteractionEffectResponseTailScanCursorV1> {
        let through = self.through.clone()?;
        let after = self
            .candidates
            .last()
            .map(|candidate| candidate.key.clone());
        RuntimeInteractionEffectResponseTailScanCursorV1::new(after, Some(through)).ok()
    }

    pub fn exhausted(&self) -> bool {
        self.candidates.is_empty()
            || self.candidates.len() < self.requested_limit.get()
            || self.candidates.last().map(|candidate| &candidate.key) == self.through.as_ref()
    }
}

impl Debug for RuntimeInteractionEffectResponseTailScanPageV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectResponseTailScanPageV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionEffectResponseTailClaimRequestV1 {
    pub(crate) candidate: RuntimeInteractionEffectResponseTailCandidateV1,
    pub(crate) expected_route: InteractionExpectedRouteV1,
    pub(crate) claim_lease: RuntimeInteractionReceiptClaimLeaseV1,
    pub(crate) unrecoverable_digest: RuntimeInteractionReceiptOpaqueDigestV1,
}

impl RuntimeInteractionEffectResponseTailClaimRequestV1 {
    pub fn new(
        candidate: RuntimeInteractionEffectResponseTailCandidateV1,
        expected_route: InteractionExpectedRouteV1,
        claim_lease: RuntimeInteractionReceiptClaimLeaseV1,
        unrecoverable_digest: RuntimeInteractionReceiptOpaqueDigestV1,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        if candidate.recovery_mode() != RuntimeInteractionEffectResponseTailRecoveryModeV1::Observe
        {
            return Err(RuntimeInteractionPersistenceErrorV1::Conflict);
        }
        validate_response_route_v1(&candidate, &expected_route)?;
        candidate.strict_recovery_binding_v1()?;
        Ok(Self {
            candidate,
            expected_route,
            claim_lease,
            unrecoverable_digest,
        })
    }
}

impl Debug for RuntimeInteractionEffectResponseTailClaimRequestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectResponseTailClaimRequestV1(<redacted>)")
    }
}

pub struct RuntimeInteractionEffectResponseTailClaimV1 {
    candidate: RuntimeInteractionEffectResponseTailCandidateV1,
    expected_route: InteractionExpectedRouteV1,
    disposition: RuntimeInteractionEffectMutationDispositionV1,
    effect_head_revision: u64,
    recovery_claim_revision: u64,
    observation_attempt: InteractionEffectAttemptV1,
    recovery_claim_expires_at: DateTime<Utc>,
    receipt_head_revision: u64,
    encrypted_token: EncryptedInteractionTokenV1,
    observed_database_now: DateTime<Utc>,
}

pub(crate) struct RuntimeInteractionEffectResponseTailClaimCheckpointV1 {
    pub(crate) disposition: RuntimeInteractionEffectMutationDispositionV1,
    pub(crate) effect_head_revision: u64,
    pub(crate) recovery_claim_revision: u64,
    pub(crate) observation_attempt: InteractionEffectAttemptV1,
    pub(crate) recovery_claim_expires_at: DateTime<Utc>,
    pub(crate) receipt_head_revision: u64,
    pub(crate) observed_database_now: DateTime<Utc>,
}

impl RuntimeInteractionEffectResponseTailClaimV1 {
    pub(crate) fn new(
        request: RuntimeInteractionEffectResponseTailClaimRequestV1,
        checkpoint: RuntimeInteractionEffectResponseTailClaimCheckpointV1,
        encrypted_token: EncryptedInteractionTokenV1,
    ) -> Self {
        Self {
            candidate: request.candidate,
            expected_route: request.expected_route,
            disposition: checkpoint.disposition,
            effect_head_revision: checkpoint.effect_head_revision,
            recovery_claim_revision: checkpoint.recovery_claim_revision,
            observation_attempt: checkpoint.observation_attempt,
            recovery_claim_expires_at: checkpoint.recovery_claim_expires_at,
            receipt_head_revision: checkpoint.receipt_head_revision,
            encrypted_token,
            observed_database_now: checkpoint.observed_database_now,
        }
    }

    pub fn candidate(&self) -> &RuntimeInteractionEffectResponseTailCandidateV1 {
        &self.candidate
    }

    pub fn expected_route(&self) -> &InteractionExpectedRouteV1 {
        &self.expected_route
    }

    pub fn disposition(&self) -> RuntimeInteractionEffectMutationDispositionV1 {
        self.disposition
    }

    pub fn effect_head_revision(&self) -> u64 {
        self.effect_head_revision
    }

    pub fn recovery_claim_revision(&self) -> u64 {
        self.recovery_claim_revision
    }

    pub fn observation_attempt(&self) -> InteractionEffectAttemptV1 {
        self.observation_attempt
    }

    pub fn recovery_claim_expires_at(&self) -> DateTime<Utc> {
        self.recovery_claim_expires_at
    }

    pub fn receipt_head_revision(&self) -> u64 {
        self.receipt_head_revision
    }

    pub fn encrypted_token(&self) -> &EncryptedInteractionTokenV1 {
        &self.encrypted_token
    }

    pub fn observed_database_now(&self) -> DateTime<Utc> {
        self.observed_database_now
    }

    pub fn into_encrypted_token(self) -> EncryptedInteractionTokenV1 {
        self.encrypted_token
    }
}

impl Debug for RuntimeInteractionEffectResponseTailClaimV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectResponseTailClaimV1(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeInteractionEffectResponseTailUnrecoverableV1 {
    effect_head_revision: u64,
    recovery_claim_revision: u64,
    receipt_head_revision: u64,
    observed_database_now: DateTime<Utc>,
}

impl RuntimeInteractionEffectResponseTailUnrecoverableV1 {
    pub(crate) fn new(
        effect_head_revision: u64,
        recovery_claim_revision: u64,
        receipt_head_revision: u64,
        observed_database_now: DateTime<Utc>,
    ) -> Self {
        Self {
            effect_head_revision,
            recovery_claim_revision,
            receipt_head_revision,
            observed_database_now,
        }
    }

    pub fn effect_head_revision(&self) -> u64 {
        self.effect_head_revision
    }

    pub fn recovery_claim_revision(&self) -> u64 {
        self.recovery_claim_revision
    }

    pub fn receipt_head_revision(&self) -> u64 {
        self.receipt_head_revision
    }

    pub fn observed_database_now(&self) -> DateTime<Utc> {
        self.observed_database_now
    }
}

pub enum RuntimeInteractionEffectResponseTailClaimOutcomeV1 {
    Claimed(Box<RuntimeInteractionEffectResponseTailClaimV1>),
    Unrecoverable(RuntimeInteractionEffectResponseTailUnrecoverableV1),
}

impl Debug for RuntimeInteractionEffectResponseTailClaimOutcomeV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectResponseTailClaimOutcomeV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionEffectResponseTailFinalizeDispositionV1 {
    EffectsCompleted,
    ResponseUnconfirmed,
    ResponseUnrecoverable,
    Deferred,
    ExactReplay,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionEffectResponseTailFinalizeRequestV1 {
    pub(crate) identity: InteractionReceiptIdentityV1,
    pub(crate) action_index: automation_runtime_interaction::InteractionEffectActionIndexV1,
    pub(crate) receipt_head_revision: u64,
    pub(crate) effect_head_revision: u64,
    pub(crate) recovery_claim_revision: u64,
    pub(crate) initial_effect_state: InteractionEffectStateV1,
    pub(crate) expected_route: InteractionExpectedRouteV1,
    pub(crate) preflight_certificate_digest:
        automation_runtime_interaction::InteractionPreflightCertificateDigestV1,
    pub(crate) expected_postimage_digest: InteractionEffectExpectedPostimageDigestV1,
    pub(crate) observation_outcome_code: &'static str,
    pub(crate) observation_digest: [u8; 32],
    pub(crate) terminal_result_digest: [u8; 32],
    pub(crate) retry_delay_milliseconds: i64,
}

impl RuntimeInteractionEffectResponseTailFinalizeRequestV1 {
    pub(crate) fn expected_effect_state_v1(&self) -> Option<InteractionEffectStateV1> {
        if self.observation_outcome_code == "close_known_state" {
            Some(self.initial_effect_state)
        } else {
            None
        }
    }

    pub fn close_known(
        candidate: &RuntimeInteractionEffectResponseTailCandidateV1,
        expected_route: InteractionExpectedRouteV1,
        terminal_result_digest: RuntimeInteractionReceiptOpaqueDigestV1,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        if candidate.recovery_mode()
            != RuntimeInteractionEffectResponseTailRecoveryModeV1::CloseKnown
        {
            return Err(RuntimeInteractionPersistenceErrorV1::Conflict);
        }
        validate_response_route_v1(candidate, &expected_route)?;
        Ok(Self {
            identity: candidate.identity(),
            action_index: candidate.key.action_index(),
            receipt_head_revision: candidate.receipt_head_revision,
            effect_head_revision: candidate.effect_head_revision,
            recovery_claim_revision: candidate.recovery_claim_revision,
            initial_effect_state: candidate.state,
            expected_route,
            preflight_certificate_digest: candidate.preflight_certificate_digest.clone(),
            expected_postimage_digest: candidate.expected_postimage_digest.clone(),
            observation_outcome_code: "close_known_state",
            observation_digest: *terminal_result_digest.as_bytes(),
            terminal_result_digest: *terminal_result_digest.as_bytes(),
            retry_delay_milliseconds: 1_000,
        })
    }

    pub fn from_observation(
        claim: &RuntimeInteractionEffectResponseTailClaimV1,
        outcome: InteractionEffectObservationOutcomeV1,
        retry_delay: Duration,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        let binding = claim.candidate.strict_recovery_binding_v1()?;
        validate_interaction_effect_recovery_observation_v1(&binding, &outcome)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
        let intent = claim
            .candidate
            .intent_digest
            .as_ref()
            .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        let observation = build_interaction_effect_recovery_observation_digest_v1(
            &binding,
            intent,
            claim.observation_attempt,
            &outcome,
        )
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
        let observation_digest = decode_hex_32_v1(observation.as_str())?;
        let (observation_outcome_code, terminal_result_digest) = match outcome {
            InteractionEffectObservationOutcomeV1::ExactMatch { .. } => (
                "exact_success",
                decode_hex_32_v1(claim.candidate.expected_postimage_digest.as_str())?,
            ),
            InteractionEffectObservationOutcomeV1::ExactAbsence { .. } => {
                ("exact_absence", observation_digest)
            }
            InteractionEffectObservationOutcomeV1::Pending { .. } => {
                ("deferred", observation_digest)
            }
            InteractionEffectObservationOutcomeV1::Conflict { .. } => {
                ("conflict", observation_digest)
            }
            InteractionEffectObservationOutcomeV1::Unsupported { .. } => {
                ("unsupported", observation_digest)
            }
        };
        Ok(Self {
            identity: claim.candidate.identity(),
            action_index: claim.candidate.key.action_index(),
            receipt_head_revision: claim.receipt_head_revision,
            effect_head_revision: claim.effect_head_revision,
            recovery_claim_revision: claim.recovery_claim_revision,
            initial_effect_state: claim.candidate.state,
            expected_route: claim.expected_route.clone(),
            preflight_certificate_digest: claim.candidate.preflight_certificate_digest.clone(),
            expected_postimage_digest: claim.candidate.expected_postimage_digest.clone(),
            observation_outcome_code,
            observation_digest,
            terminal_result_digest,
            retry_delay_milliseconds: retry_delay_milliseconds_v1(retry_delay)?,
        })
    }

    pub fn token_unrecoverable(
        claim: &RuntimeInteractionEffectResponseTailClaimV1,
        digest: RuntimeInteractionReceiptOpaqueDigestV1,
    ) -> Self {
        Self {
            identity: claim.candidate.identity(),
            action_index: claim.candidate.key.action_index(),
            receipt_head_revision: claim.receipt_head_revision,
            effect_head_revision: claim.effect_head_revision,
            recovery_claim_revision: claim.recovery_claim_revision,
            initial_effect_state: claim.candidate.state,
            expected_route: claim.expected_route.clone(),
            preflight_certificate_digest: claim.candidate.preflight_certificate_digest.clone(),
            expected_postimage_digest: claim.candidate.expected_postimage_digest.clone(),
            observation_outcome_code: "token_unrecoverable",
            observation_digest: *digest.as_bytes(),
            terminal_result_digest: *digest.as_bytes(),
            retry_delay_milliseconds: 1_000,
        }
    }

    pub fn recovery_blocked(
        claim: &RuntimeInteractionEffectResponseTailClaimV1,
        reason: RuntimeInteractionEffectRecoveryBlockReasonV1,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        if !reason.allows_response_tail() {
            return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
        }
        let identity = claim.candidate.identity();
        let expected = &claim.expected_route;
        let document = format!(
            "starring-runtime-interaction-response-tail-recovery-block-v1|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            identity.application_id().get(),
            identity.interaction_id().get(),
            claim.candidate.key.action_index().get(),
            claim.receipt_head_revision,
            claim.effect_head_revision,
            claim.recovery_claim_revision,
            expected.process_identity().process_instance_id.as_str(),
            expected.gateway_shard_identity().as_str(),
            expected.runtime_build_revision().as_str(),
            expected.process_identity().runtime_generation.get(),
            expected.route_fencing_token().get(),
            expected.route_incarnation().get(),
            claim.candidate.preflight_certificate_digest.as_str(),
            reason.code()
        );
        let digest: [u8; 32] = Sha256::digest(document.as_bytes()).into();
        Ok(Self {
            identity,
            action_index: claim.candidate.key.action_index(),
            receipt_head_revision: claim.receipt_head_revision,
            effect_head_revision: claim.effect_head_revision,
            recovery_claim_revision: claim.recovery_claim_revision,
            initial_effect_state: claim.candidate.state,
            expected_route: claim.expected_route.clone(),
            preflight_certificate_digest: claim.candidate.preflight_certificate_digest.clone(),
            expected_postimage_digest: claim.candidate.expected_postimage_digest.clone(),
            observation_outcome_code: reason.code(),
            observation_digest: digest,
            terminal_result_digest: digest,
            retry_delay_milliseconds: 1_000,
        })
    }
}

impl Debug for RuntimeInteractionEffectResponseTailFinalizeRequestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectResponseTailFinalizeRequestV1(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeInteractionEffectResponseTailFinalizeOutcomeV1 {
    disposition: RuntimeInteractionEffectResponseTailFinalizeDispositionV1,
    effect_state: InteractionEffectStateV1,
    effect_head_revision: u64,
    receipt_state: InteractionReceiptStateV1,
    receipt_head_revision: u64,
    recovery_at: Option<DateTime<Utc>>,
    observed_database_now: DateTime<Utc>,
}

impl RuntimeInteractionEffectResponseTailFinalizeOutcomeV1 {
    pub(crate) fn new(
        disposition: RuntimeInteractionEffectResponseTailFinalizeDispositionV1,
        effect_state: InteractionEffectStateV1,
        effect_head_revision: u64,
        receipt_state: InteractionReceiptStateV1,
        receipt_head_revision: u64,
        recovery_at: Option<DateTime<Utc>>,
        observed_database_now: DateTime<Utc>,
    ) -> Self {
        Self {
            disposition,
            effect_state,
            effect_head_revision,
            receipt_state,
            receipt_head_revision,
            recovery_at,
            observed_database_now,
        }
    }

    pub fn disposition(&self) -> RuntimeInteractionEffectResponseTailFinalizeDispositionV1 {
        self.disposition
    }

    pub fn effect_state(&self) -> InteractionEffectStateV1 {
        self.effect_state
    }

    pub fn effect_head_revision(&self) -> u64 {
        self.effect_head_revision
    }

    pub fn receipt_state(&self) -> InteractionReceiptStateV1 {
        self.receipt_state
    }

    pub fn receipt_head_revision(&self) -> u64 {
        self.receipt_head_revision
    }

    pub fn recovery_at(&self) -> Option<DateTime<Utc>> {
        self.recovery_at
    }

    pub fn observed_database_now(&self) -> DateTime<Utc> {
        self.observed_database_now
    }
}

pub(crate) fn response_payload_digest_v1(
    planned: &Value,
    resolved: Option<&Value>,
) -> Result<InteractionEffectPayloadDigestV1, RuntimeInteractionPersistenceErrorV1> {
    let planned = response_payload_document_v1(planned)?;
    if let Some(resolved) = resolved {
        let resolved = response_payload_document_v1(resolved)?;
        if resolved != planned {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
    }
    Ok(planned)
}

pub(crate) fn validate_response_preimage_document_v1(
    value: &Value,
) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    let object = value
        .as_object()
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    if object.len() != 1 || object.get("kind").and_then(Value::as_str) != Some("none") {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    Ok(())
}

fn response_payload_document_v1(
    value: &Value,
) -> Result<InteractionEffectPayloadDigestV1, RuntimeInteractionPersistenceErrorV1> {
    let object = value
        .as_object()
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    if object.len() != 2
        || !object
            .get("references")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
    {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    InteractionEffectPayloadDigestV1::parse(
        object
            .get("payload_digest")
            .and_then(Value::as_str)
            .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
    )
    .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
}

fn validate_response_route_v1(
    candidate: &RuntimeInteractionEffectResponseTailCandidateV1,
    expected: &InteractionExpectedRouteV1,
) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    let origin = candidate.origin.claim_root().route();
    if origin.scope() != expected.scope()
        || origin.process_identity().target != expected.process_identity().target
        || origin.process_identity().runtime_generation
            != expected.process_identity().runtime_generation
        || origin.serving_identity().route_fencing_token() != expected.route_fencing_token()
        || origin.serving_identity().route_incarnation() != expected.route_incarnation()
    {
        return Err(RuntimeInteractionPersistenceErrorV1::InvalidAuthority);
    }
    Ok(())
}

fn retry_delay_milliseconds_v1(
    delay: Duration,
) -> Result<i64, RuntimeInteractionPersistenceErrorV1> {
    if !(Duration::from_secs(1)..=Duration::from_secs(60)).contains(&delay)
        || !delay.subsec_nanos().is_multiple_of(1_000_000)
    {
        return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
    }
    i64::try_from(delay.as_millis()).map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)
}

fn decode_hex_32_v1(value: &str) -> Result<[u8; 32], RuntimeInteractionPersistenceErrorV1> {
    if value.len() != 64 {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    let mut output = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = decode_hex_nibble_v1(pair[0])?;
        let low = decode_hex_nibble_v1(pair[1])?;
        output[index] = high << 4 | low;
    }
    Ok(output)
}

fn decode_hex_nibble_v1(value: u8) -> Result<u8, RuntimeInteractionPersistenceErrorV1> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
    }
}

pub(crate) fn decode_observation_attempt_v1(
    value: i32,
) -> Result<InteractionEffectAttemptV1, RuntimeInteractionPersistenceErrorV1> {
    let value = u16::try_from(value)
        .ok()
        .and_then(NonZeroU16::new)
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    InteractionEffectAttemptV1::new(value.get())
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
}
