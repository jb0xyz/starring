use automation_runtime_interaction::{
    EncryptedInteractionTokenV1, InteractionEffectCorrelationDigestV1,
    InteractionEffectExpectedPostimageDigestV1, InteractionEffectIdentityDigestV1,
    InteractionEffectIntentDigestV1, InteractionEffectPlannedIdentityDigestV1,
    InteractionEffectPlannedPreimageDigestV1, InteractionEffectPreimageDigestV1,
    InteractionEffectResultDigestV1, InteractionEffectStateV1,
    InteractionPreflightCertificateDigestV1, InteractionReceiptStateV1,
    InteractionTokenAuthenticatedDataDigestV1, InteractionTokenEnvelopeTimeV1,
};
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::effect::decode_effect_state_v1;
use crate::effect_row::{
    decode_action_index, decode_origin, nonnegative_u64, parse_digest, EffectOriginRowV1,
};
use crate::receipt::{
    unix_milliseconds, validate_database_time, validate_envelope_authenticated_data,
};
use crate::receipt_row::{bytes_to_lower_hex, decode_receipt_identity, positive_u64};
use crate::response_tail::{
    decode_observation_attempt_v1, response_payload_digest_v1,
    validate_response_preimage_document_v1, RuntimeInteractionEffectResponseTailCandidateV1,
    RuntimeInteractionEffectResponseTailClaimCheckpointV1,
    RuntimeInteractionEffectResponseTailClaimOutcomeV1,
    RuntimeInteractionEffectResponseTailClaimRequestV1,
    RuntimeInteractionEffectResponseTailClaimV1,
    RuntimeInteractionEffectResponseTailFinalizeDispositionV1,
    RuntimeInteractionEffectResponseTailFinalizeOutcomeV1,
    RuntimeInteractionEffectResponseTailFinalizeRequestV1,
    RuntimeInteractionEffectResponseTailRecoveryModeV1,
    RuntimeInteractionEffectResponseTailScanKeyV1,
    RuntimeInteractionEffectResponseTailUnrecoverableV1,
};
use crate::{RuntimeInteractionEffectMutationDispositionV1, RuntimeInteractionPersistenceErrorV1};

#[derive(sqlx::FromRow)]
pub(crate) struct EffectResponseTailScanRowV1 {
    pub(crate) application_id: String,
    pub(crate) interaction_id: String,
    pub(crate) action_index: i16,
    pub(crate) effect_state: String,
    pub(crate) effect_head_revision: i64,
    pub(crate) recovery_claim_revision: i64,
    pub(crate) observation_attempt_count: i32,
    pub(crate) planned_identity_digest: Vec<u8>,
    pub(crate) input_digest: Vec<u8>,
    pub(crate) expected_postimage_digest: Vec<u8>,
    pub(crate) planned_recovery_input: Value,
    pub(crate) planned_preimage_digest: Vec<u8>,
    pub(crate) planned_preimage: Value,
    pub(crate) resolved_input: Option<Value>,
    pub(crate) resolved_preimage_digest: Option<Vec<u8>>,
    pub(crate) resolved_preimage: Option<Value>,
    pub(crate) resolved_effect_identity_digest: Option<Vec<u8>>,
    pub(crate) intent_digest: Option<Vec<u8>>,
    pub(crate) result_digest: Option<Vec<u8>>,
    pub(crate) success_binding_kind: Option<String>,
    pub(crate) success_binding_digest: Option<Vec<u8>>,
    pub(crate) correlation_digest: Vec<u8>,
    pub(crate) action_plan_digest: Vec<u8>,
    pub(crate) preflight_certificate_digest: Vec<u8>,
    pub(crate) snapshot_digest: Vec<u8>,
    pub(crate) receipt_state: String,
    pub(crate) receipt_head_revision: i64,
    pub(crate) receipt_claim_revision: i64,
    pub(crate) receipt_claim_expires_at: DateTime<Utc>,
    pub(crate) token_expires_at: Option<DateTime<Utc>>,
    #[sqlx(flatten)]
    pub(crate) origin: EffectOriginRowV1,
    pub(crate) next_recovery_at: DateTime<Utc>,
    pub(crate) through_recovery_at: DateTime<Utc>,
    pub(crate) through_application_id: String,
    pub(crate) through_interaction_id: String,
    pub(crate) through_action_index: i16,
    pub(crate) observed_database_now: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct EffectResponseTailClaimRowV1 {
    pub(crate) outcome_name: String,
    pub(crate) effect_state: String,
    pub(crate) resulting_effect_head_revision: i64,
    pub(crate) resulting_recovery_claim_revision: i64,
    pub(crate) resulting_observation_attempt_count: i32,
    pub(crate) resulting_recovery_claim_expires_at: DateTime<Utc>,
    pub(crate) receipt_state: String,
    pub(crate) resulting_receipt_head_revision: i64,
    pub(crate) token_encryption_suite: Option<String>,
    pub(crate) token_suite_version: Option<i16>,
    pub(crate) token_key_id: Option<String>,
    pub(crate) token_nonce: Option<Vec<u8>>,
    pub(crate) token_ciphertext: Option<Vec<u8>>,
    pub(crate) token_aad_digest: Option<Vec<u8>>,
    pub(crate) token_issued_at: Option<DateTime<Utc>>,
    pub(crate) token_expires_at: Option<DateTime<Utc>>,
    pub(crate) observed_database_now: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct EffectResponseTailFinalizeRowV1 {
    pub(crate) outcome_name: String,
    pub(crate) effect_state: String,
    pub(crate) resulting_effect_head_revision: i64,
    pub(crate) receipt_state: String,
    pub(crate) resulting_receipt_head_revision: i64,
    pub(crate) resulting_recovery_at: Option<DateTime<Utc>>,
    pub(crate) observed_database_now: DateTime<Utc>,
}

impl EffectResponseTailScanRowV1 {
    pub(crate) fn decode(
        self,
    ) -> Result<
        (
            RuntimeInteractionEffectResponseTailCandidateV1,
            RuntimeInteractionEffectResponseTailScanKeyV1,
            DateTime<Utc>,
        ),
        RuntimeInteractionPersistenceErrorV1,
    > {
        validate_database_time(self.next_recovery_at, false)?;
        validate_database_time(self.through_recovery_at, false)?;
        validate_database_time(self.observed_database_now, false)?;
        validate_database_time(self.receipt_claim_expires_at, false)?;
        if let Some(token_expires_at) = self.token_expires_at {
            validate_database_time(token_expires_at, false)?;
        }
        if self.receipt_state != "executing"
            || self.next_recovery_at > self.observed_database_now
            || self.receipt_claim_expires_at > self.observed_database_now
            || self.input_digest.len() != 32
            || self.action_plan_digest.len() != 32
            || self.snapshot_digest.len() != 32
        {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        let identity = decode_receipt_identity(self.application_id, self.interaction_id)?;
        let action_index = decode_action_index(self.action_index)?;
        let key = RuntimeInteractionEffectResponseTailScanKeyV1::new(
            self.next_recovery_at,
            identity,
            action_index,
        )?;
        let through = RuntimeInteractionEffectResponseTailScanKeyV1::new(
            self.through_recovery_at,
            decode_receipt_identity(self.through_application_id, self.through_interaction_id)?,
            decode_action_index(self.through_action_index)?,
        )?;
        if key.cmp_c(&through).is_gt() {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        let state = decode_effect_state_v1(&self.effect_state)?;
        if !matches!(
            state,
            InteractionEffectStateV1::Planned
                | InteractionEffectStateV1::Intended
                | InteractionEffectStateV1::KnownSucceeded
                | InteractionEffectStateV1::KnownFailed
                | InteractionEffectStateV1::Indeterminate
                | InteractionEffectStateV1::Observing
                | InteractionEffectStateV1::ObservationPending
                | InteractionEffectStateV1::ReconciledSucceeded
        ) {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        validate_response_preimage_document_v1(&self.planned_preimage)?;
        if let Some(preimage) = self.resolved_preimage.as_ref() {
            validate_response_preimage_document_v1(preimage)?;
        }
        let resolved_shape = self.resolved_input.is_some()
            && self.resolved_preimage_digest.is_some()
            && self.resolved_preimage.is_some()
            && self.resolved_effect_identity_digest.is_some();
        let resolved_absent = self.resolved_input.is_none()
            && self.resolved_preimage_digest.is_none()
            && self.resolved_preimage.is_none()
            && self.resolved_effect_identity_digest.is_none();
        let planned = state == InteractionEffectStateV1::Planned;
        if planned != resolved_absent
            || !planned && !resolved_shape
            || planned
                && (self.intent_digest.is_some()
                    || self.result_digest.is_some()
                    || self.success_binding_kind.is_some()
                    || self.success_binding_digest.is_some()
                    || self.observation_attempt_count != 0)
        {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        validate_response_result_shape_v1(
            state,
            self.intent_digest.as_ref(),
            self.result_digest.as_ref(),
            self.success_binding_kind.as_deref(),
            self.success_binding_digest.as_ref(),
        )?;
        let payload_digest =
            response_payload_digest_v1(&self.planned_recovery_input, self.resolved_input.as_ref())?;
        let origin = decode_origin(&self.origin, identity)?;
        let candidate = RuntimeInteractionEffectResponseTailCandidateV1 {
            key,
            state,
            effect_head_revision: positive_u64(self.effect_head_revision)?,
            recovery_claim_revision: nonnegative_u64(self.recovery_claim_revision)?,
            observation_attempt_count: decode_nonnegative_attempt_v1(
                self.observation_attempt_count,
            )?,
            planned_identity_digest: parse_digest(
                self.planned_identity_digest,
                InteractionEffectPlannedIdentityDigestV1::parse,
            )?,
            expected_postimage_digest: parse_digest(
                self.expected_postimage_digest,
                InteractionEffectExpectedPostimageDigestV1::parse,
            )?,
            payload_digest,
            planned_preimage_digest: parse_digest(
                self.planned_preimage_digest,
                InteractionEffectPlannedPreimageDigestV1::parse,
            )?,
            resolved_preimage_digest: self
                .resolved_preimage_digest
                .map(|value| parse_digest(value, InteractionEffectPreimageDigestV1::parse))
                .transpose()?,
            resolved_effect_identity_digest: self
                .resolved_effect_identity_digest
                .map(|value| parse_digest(value, InteractionEffectIdentityDigestV1::parse))
                .transpose()?,
            correlation_digest: parse_digest(
                self.correlation_digest,
                InteractionEffectCorrelationDigestV1::parse,
            )?,
            intent_digest: self
                .intent_digest
                .map(|value| parse_digest(value, InteractionEffectIntentDigestV1::parse))
                .transpose()?,
            result_digest: self
                .result_digest
                .map(|value| parse_digest(value, InteractionEffectResultDigestV1::parse))
                .transpose()?,
            receipt_head_revision: positive_u64(self.receipt_head_revision)?,
            receipt_claim_revision: positive_u64(self.receipt_claim_revision)?,
            receipt_claim_expires_at: self.receipt_claim_expires_at,
            token_expires_at: self.token_expires_at,
            preflight_certificate_digest: parse_digest(
                self.preflight_certificate_digest,
                InteractionPreflightCertificateDigestV1::parse,
            )?,
            origin,
        };
        if candidate.recovery_mode() == RuntimeInteractionEffectResponseTailRecoveryModeV1::Observe
        {
            candidate.strict_recovery_binding_v1()?;
        }
        Ok((candidate, through, self.observed_database_now))
    }
}

impl EffectResponseTailClaimRowV1 {
    pub(crate) fn decode(
        self,
        request: RuntimeInteractionEffectResponseTailClaimRequestV1,
    ) -> Result<
        RuntimeInteractionEffectResponseTailClaimOutcomeV1,
        RuntimeInteractionPersistenceErrorV1,
    > {
        validate_database_time(self.resulting_recovery_claim_expires_at, false)?;
        validate_database_time(self.observed_database_now, false)?;
        let effect_head_revision = positive_u64(self.resulting_effect_head_revision)?;
        let recovery_claim_revision = nonnegative_u64(self.resulting_recovery_claim_revision)?;
        let receipt_head_revision = positive_u64(self.resulting_receipt_head_revision)?;
        let observation_attempt =
            decode_observation_attempt_v1(self.resulting_observation_attempt_count)?;
        match self.outcome_name.as_str() {
            "interaction_response_unrecoverable" => {
                let claimed_then_unrecoverable = effect_head_revision
                    == request.candidate.effect_head_revision + 2
                    && recovery_claim_revision == request.candidate.recovery_claim_revision + 1
                    && observation_attempt.get() == request.candidate.observation_attempt_count + 1;
                let budget_exhausted = effect_head_revision
                    == request.candidate.effect_head_revision + 1
                    && request.candidate.observation_attempt_count >= 64
                    && recovery_claim_revision == request.candidate.recovery_claim_revision
                    && observation_attempt.get() == request.candidate.observation_attempt_count;
                if self.effect_state != "recovery_required"
                    || self.receipt_state != "completed"
                    || token_payload_present_v1(&self)
                    || !(claimed_then_unrecoverable || budget_exhausted)
                    || receipt_head_revision != request.candidate.receipt_head_revision + 1
                    || self.resulting_recovery_claim_expires_at != self.observed_database_now
                {
                    return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
                }
                Ok(
                    RuntimeInteractionEffectResponseTailClaimOutcomeV1::Unrecoverable(
                        RuntimeInteractionEffectResponseTailUnrecoverableV1::new(
                            effect_head_revision,
                            recovery_claim_revision,
                            receipt_head_revision,
                            self.observed_database_now,
                        ),
                    ),
                )
            }
            "response_tail_claimed" | "response_tail_claim_replayed" => {
                let disposition = if self.outcome_name == "response_tail_claimed" {
                    RuntimeInteractionEffectMutationDispositionV1::Applied
                } else {
                    RuntimeInteractionEffectMutationDispositionV1::ExactReplay
                };
                if self.effect_state != "observing"
                    || self.receipt_state != "executing"
                    || self.resulting_recovery_claim_expires_at <= self.observed_database_now
                    || !response_claim_revisions_match_v1(
                        ResponseClaimRevisionShapeV1 {
                            effect_head_revision,
                            recovery_claim_revision,
                            observation_attempt: observation_attempt.get(),
                            receipt_head_revision,
                        },
                        ResponseClaimRevisionShapeV1 {
                            effect_head_revision: request.candidate.effect_head_revision,
                            recovery_claim_revision: request.candidate.recovery_claim_revision,
                            observation_attempt: request.candidate.observation_attempt_count,
                            receipt_head_revision: request.candidate.receipt_head_revision,
                        },
                    )
                {
                    return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
                }
                let token = decode_token_v1(&self, request.candidate.origin.claim_root())?;
                if !response_token_outlives_claim_v1(
                    token.time().expires_at_unix_milliseconds(),
                    self.resulting_recovery_claim_expires_at,
                )? {
                    return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
                }
                Ok(RuntimeInteractionEffectResponseTailClaimOutcomeV1::Claimed(
                    Box::new(RuntimeInteractionEffectResponseTailClaimV1::new(
                        request,
                        RuntimeInteractionEffectResponseTailClaimCheckpointV1 {
                            disposition,
                            effect_head_revision,
                            recovery_claim_revision,
                            observation_attempt,
                            recovery_claim_expires_at: self.resulting_recovery_claim_expires_at,
                            receipt_head_revision,
                            observed_database_now: self.observed_database_now,
                        },
                        token,
                    )),
                ))
            }
            _ => Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
        }
    }
}

impl EffectResponseTailFinalizeRowV1 {
    pub(crate) fn decode(
        self,
        request: &RuntimeInteractionEffectResponseTailFinalizeRequestV1,
    ) -> Result<
        RuntimeInteractionEffectResponseTailFinalizeOutcomeV1,
        RuntimeInteractionPersistenceErrorV1,
    > {
        validate_database_time(self.observed_database_now, false)?;
        if let Some(recovery_at) = self.resulting_recovery_at {
            validate_database_time(recovery_at, false)?;
        }
        let effect_state = decode_effect_state_v1(&self.effect_state)?;
        let receipt_state = decode_receipt_state_v1(&self.receipt_state)?;
        let effect_head_revision = positive_u64(self.resulting_effect_head_revision)?;
        let receipt_head_revision = positive_u64(self.resulting_receipt_head_revision)?;
        let disposition = match self.outcome_name.as_str() {
            "effects_recovered_completed" => {
                RuntimeInteractionEffectResponseTailFinalizeDispositionV1::EffectsCompleted
            }
            "provisioning_completed_response_unconfirmed" => {
                RuntimeInteractionEffectResponseTailFinalizeDispositionV1::ResponseUnconfirmed
            }
            "interaction_response_unrecoverable" => {
                RuntimeInteractionEffectResponseTailFinalizeDispositionV1::ResponseUnrecoverable
            }
            "deferred" => RuntimeInteractionEffectResponseTailFinalizeDispositionV1::Deferred,
            "exact_replay" => {
                RuntimeInteractionEffectResponseTailFinalizeDispositionV1::ExactReplay
            }
            _ => return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
        };
        let terminal = matches!(
            disposition,
            RuntimeInteractionEffectResponseTailFinalizeDispositionV1::EffectsCompleted
                | RuntimeInteractionEffectResponseTailFinalizeDispositionV1::ResponseUnconfirmed
                | RuntimeInteractionEffectResponseTailFinalizeDispositionV1::ResponseUnrecoverable
                | RuntimeInteractionEffectResponseTailFinalizeDispositionV1::ExactReplay
        );
        let exact_replay =
            disposition == RuntimeInteractionEffectResponseTailFinalizeDispositionV1::ExactReplay;
        let expected_terminal_disposition = match request.observation_outcome_code {
            "close_known_state" => match request.expected_effect_state_v1() {
                Some(InteractionEffectStateV1::KnownSucceeded)
                | Some(InteractionEffectStateV1::ReconciledSucceeded) => {
                    RuntimeInteractionEffectResponseTailFinalizeDispositionV1::EffectsCompleted
                }
                Some(InteractionEffectStateV1::Planned)
                | Some(InteractionEffectStateV1::KnownFailed) => {
                    RuntimeInteractionEffectResponseTailFinalizeDispositionV1::ResponseUnconfirmed
                }
                _ => return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
            },
            "exact_success" => {
                RuntimeInteractionEffectResponseTailFinalizeDispositionV1::EffectsCompleted
            }
            "exact_absence" => {
                RuntimeInteractionEffectResponseTailFinalizeDispositionV1::ResponseUnconfirmed
            }
            "conflict"
            | "unsupported"
            | "token_unrecoverable"
            | "recovery_blocked_discord_read_rejected"
            | "recovery_blocked_response_token_unavailable"
            | "recovery_blocked_observation_protocol"
            | "recovery_blocked_internal_conflict"
            | "recovery_blocked_discord_forbidden"
            | "recovery_blocked_internal_authority"
            | "recovery_blocked_attempt_budget_exhausted" => {
                RuntimeInteractionEffectResponseTailFinalizeDispositionV1::ResponseUnrecoverable
            }
            "deferred" => RuntimeInteractionEffectResponseTailFinalizeDispositionV1::Deferred,
            _ => return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
        };
        if (!exact_replay && disposition != expected_terminal_disposition)
            || (exact_replay
                && expected_terminal_disposition
                    == RuntimeInteractionEffectResponseTailFinalizeDispositionV1::Deferred)
        {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        let expected_effect_state = match request.observation_outcome_code {
            "close_known_state" => request
                .expected_effect_state_v1()
                .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
            "exact_success" => InteractionEffectStateV1::ReconciledSucceeded,
            "exact_absence" => InteractionEffectStateV1::KnownFailed,
            "conflict"
            | "unsupported"
            | "token_unrecoverable"
            | "recovery_blocked_discord_read_rejected"
            | "recovery_blocked_response_token_unavailable"
            | "recovery_blocked_observation_protocol"
            | "recovery_blocked_internal_conflict"
            | "recovery_blocked_discord_forbidden"
            | "recovery_blocked_internal_authority"
            | "recovery_blocked_attempt_budget_exhausted" => {
                InteractionEffectStateV1::RecoveryRequired
            }
            "deferred" => InteractionEffectStateV1::ObservationPending,
            _ => return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
        };
        let expected_effect_revision = if request.observation_outcome_code == "close_known_state" {
            request.effect_head_revision
        } else {
            request.effect_head_revision + 1
        };
        if terminal != (receipt_state == InteractionReceiptStateV1::Completed)
            || terminal != (receipt_head_revision == request.receipt_head_revision + 1)
            || terminal == self.resulting_recovery_at.is_some()
            || terminal
                && (effect_state != expected_effect_state
                    || effect_head_revision != expected_effect_revision)
            || !terminal
                && (receipt_state != InteractionReceiptStateV1::Executing
                    || receipt_head_revision != request.receipt_head_revision
                    || effect_state != InteractionEffectStateV1::ObservationPending
                    || effect_head_revision != request.effect_head_revision + 1)
        {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        Ok(RuntimeInteractionEffectResponseTailFinalizeOutcomeV1::new(
            disposition,
            effect_state,
            effect_head_revision,
            receipt_state,
            receipt_head_revision,
            self.resulting_recovery_at,
            self.observed_database_now,
        ))
    }
}

fn validate_response_result_shape_v1(
    state: InteractionEffectStateV1,
    intent: Option<&Vec<u8>>,
    result: Option<&Vec<u8>>,
    success_kind: Option<&str>,
    success_digest: Option<&Vec<u8>>,
) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    let success_matches_result = success_digest
        .zip(result)
        .is_some_and(|(left, right)| left == right);
    let valid = match state {
        InteractionEffectStateV1::Planned => {
            intent.is_none()
                && result.is_none()
                && success_kind.is_none()
                && success_digest.is_none()
        }
        InteractionEffectStateV1::Intended => {
            intent.is_some()
                && result.is_none()
                && success_kind.is_none()
                && success_digest.is_none()
        }
        InteractionEffectStateV1::KnownSucceeded => {
            intent.is_some()
                && result.is_some()
                && success_kind == Some("attempt_result")
                && success_matches_result
        }
        InteractionEffectStateV1::ReconciledSucceeded => {
            intent.is_some()
                && result.is_some()
                && success_kind == Some("observation")
                && success_digest.is_some()
        }
        InteractionEffectStateV1::KnownFailed | InteractionEffectStateV1::Indeterminate => {
            intent.is_some()
                && result.is_some()
                && success_kind.is_none()
                && success_digest.is_none()
        }
        InteractionEffectStateV1::Observing => {
            intent.is_some() && success_kind.is_none() && success_digest.is_none()
        }
        InteractionEffectStateV1::ObservationPending => {
            intent.is_some()
                && result.is_some()
                && success_kind.is_none()
                && success_digest.is_none()
        }
        _ => false,
    };
    if valid
        && intent.is_none_or(|value| value.len() == 32)
        && result.is_none_or(|value| value.len() == 32)
        && success_digest.is_none_or(|value| value.len() == 32)
    {
        Ok(())
    } else {
        Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
    }
}

fn decode_nonnegative_attempt_v1(value: i32) -> Result<u16, RuntimeInteractionPersistenceErrorV1> {
    u16::try_from(value)
        .ok()
        .filter(|value| *value <= 64)
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
}

fn decode_receipt_state_v1(
    value: &str,
) -> Result<InteractionReceiptStateV1, RuntimeInteractionPersistenceErrorV1> {
    match value {
        "executing" => Ok(InteractionReceiptStateV1::Executing),
        "completed" => Ok(InteractionReceiptStateV1::Completed),
        _ => Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
    }
}

fn token_payload_present_v1(row: &EffectResponseTailClaimRowV1) -> bool {
    row.token_encryption_suite.is_some()
        || row.token_suite_version.is_some()
        || row.token_key_id.is_some()
        || row.token_nonce.is_some()
        || row.token_ciphertext.is_some()
        || row.token_aad_digest.is_some()
        || row.token_issued_at.is_some()
        || row.token_expires_at.is_some()
}

fn decode_token_v1(
    row: &EffectResponseTailClaimRowV1,
    root: &automation_runtime_interaction::InteractionReceiptClaimRootV1,
) -> Result<EncryptedInteractionTokenV1, RuntimeInteractionPersistenceErrorV1> {
    let ciphertext = required_v1(&row.token_ciphertext)?.clone();
    if !(17..=4_112).contains(&ciphertext.len()) {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    let issued_at = *required_v1(&row.token_issued_at)?;
    let expires_at = *required_v1(&row.token_expires_at)?;
    validate_database_time(issued_at, false)?;
    validate_database_time(expires_at, false)?;
    let time = InteractionTokenEnvelopeTimeV1::new(
        unix_milliseconds(issued_at)?,
        unix_milliseconds(expires_at)?,
    )
    .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    let suite_version = u16::try_from(*required_v1(&row.token_suite_version)?)
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    let aad_digest = InteractionTokenAuthenticatedDataDigestV1::parse(bytes_to_lower_hex(
        required_v1(&row.token_aad_digest)?,
    ))
    .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    let token = EncryptedInteractionTokenV1::from_persisted_parts(
        ciphertext,
        required_v1(&row.token_nonce)?.clone(),
        required_v1(&row.token_key_id)?.clone(),
        required_v1(&row.token_encryption_suite)?.clone(),
        suite_version,
        time,
        aad_digest,
    )
    .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    validate_envelope_authenticated_data(root, &token)?;
    Ok(token)
}

#[derive(Clone, Copy)]
struct ResponseClaimRevisionShapeV1 {
    effect_head_revision: u64,
    recovery_claim_revision: u64,
    observation_attempt: u16,
    receipt_head_revision: u64,
}

fn response_claim_revisions_match_v1(
    observed: ResponseClaimRevisionShapeV1,
    candidate: ResponseClaimRevisionShapeV1,
) -> bool {
    observed.effect_head_revision == candidate.effect_head_revision + 1
        && observed.recovery_claim_revision == candidate.recovery_claim_revision + 1
        && observed.observation_attempt == candidate.observation_attempt + 1
        && observed.receipt_head_revision == candidate.receipt_head_revision
}

fn response_token_outlives_claim_v1(
    token_expires_at_unix_milliseconds: u64,
    claim_expires_at: DateTime<Utc>,
) -> Result<bool, RuntimeInteractionPersistenceErrorV1> {
    Ok(token_expires_at_unix_milliseconds > unix_milliseconds(claim_expires_at)?)
}

fn required_v1<T>(value: &Option<T>) -> Result<&T, RuntimeInteractionPersistenceErrorV1> {
    value
        .as_ref()
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
    use automation_runtime_convergence::{
        BindingRevision, DeploymentId, InstallationId, ProcessInstanceId,
        RuntimeDeploymentTargetV1, RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
    };
    use automation_runtime_interaction::{
        DiscordApplicationIdV1, DiscordInteractionIdV1, InteractionEffectActionIndexV1,
        InteractionExpectedRouteV1, InteractionGatewayShardIdentityV1,
        InteractionPreflightCertificateDigestV1, InteractionProductScopeV1,
        InteractionReceiptIdentityV1, InteractionRouteIncarnationV1,
        InteractionRuntimeBuildRevisionV1,
    };
    use chrono::TimeZone;
    use discord_model::GuildId;
    use resource_resolution::ResourceBindingFingerprint;

    fn expected_route() -> InteractionExpectedRouteV1 {
        InteractionExpectedRouteV1::new(
            InteractionProductScopeV1::new(
                TenantId::parse("tenant-response").unwrap(),
                InstallationId::parse("installation-response").unwrap(),
                DeploymentId::parse("deployment-response").unwrap(),
            ),
            RuntimeProcessIdentityV1 {
                target: RuntimeDeploymentTargetV1 {
                    guild_id: GuildId(33),
                    ruleset_key: RuleSetKey::parse("response").unwrap(),
                    version: RuleSetVersionId::FIRST,
                    content_hash: RuleSetContentHash::parse_hex(&"a".repeat(64)).unwrap(),
                    binding_revision: BindingRevision::new(1).unwrap(),
                    binding_fingerprint: ResourceBindingFingerprint::parse(&"b".repeat(64))
                        .unwrap(),
                },
                runtime_generation: RuntimeGeneration::new(1).unwrap(),
                process_instance_id: ProcessInstanceId::parse("process-response").unwrap(),
            },
            InteractionGatewayShardIdentityV1::parse("gateway-response").unwrap(),
            InteractionRuntimeBuildRevisionV1::parse("build-response").unwrap(),
            automation_runtime_convergence::FencingToken::new(1).unwrap(),
            InteractionRouteIncarnationV1::new(1).unwrap(),
        )
        .unwrap()
    }

    fn finalize_request(
        outcome: &'static str,
        initial_effect_state: InteractionEffectStateV1,
    ) -> RuntimeInteractionEffectResponseTailFinalizeRequestV1 {
        RuntimeInteractionEffectResponseTailFinalizeRequestV1 {
            identity: InteractionReceiptIdentityV1::new(
                DiscordApplicationIdV1::new(1).unwrap(),
                DiscordInteractionIdV1::new(2).unwrap(),
            ),
            action_index: InteractionEffectActionIndexV1::new(0).unwrap(),
            receipt_head_revision: 20,
            effect_head_revision: 10,
            recovery_claim_revision: 3,
            initial_effect_state,
            expected_route: expected_route(),
            preflight_certificate_digest: InteractionPreflightCertificateDigestV1::parse(
                "c".repeat(64),
            )
            .unwrap(),
            expected_postimage_digest: InteractionEffectExpectedPostimageDigestV1::parse(
                "d".repeat(64),
            )
            .unwrap(),
            observation_outcome_code: outcome,
            observation_digest: [0xe; 32],
            terminal_result_digest: [0xf; 32],
            retry_delay_milliseconds: 1_000,
        }
    }

    fn finalize_row(
        outcome: &str,
        effect_state: &str,
        effect_revision: i64,
        receipt_state: &str,
        receipt_revision: i64,
        recovery_at: Option<DateTime<Utc>>,
    ) -> EffectResponseTailFinalizeRowV1 {
        EffectResponseTailFinalizeRowV1 {
            outcome_name: outcome.to_string(),
            effect_state: effect_state.to_string(),
            resulting_effect_head_revision: effect_revision,
            receipt_state: receipt_state.to_string(),
            resulting_receipt_head_revision: receipt_revision,
            resulting_recovery_at: recovery_at,
            observed_database_now: Utc.timestamp_millis_opt(10_000).single().unwrap(),
        }
    }

    #[test]
    fn response_claim_requires_exact_locked_revisions_for_applied_and_replay() {
        let candidate = ResponseClaimRevisionShapeV1 {
            effect_head_revision: 10,
            recovery_claim_revision: 3,
            observation_attempt: 5,
            receipt_head_revision: 20,
        };
        assert!(response_claim_revisions_match_v1(
            ResponseClaimRevisionShapeV1 {
                effect_head_revision: 11,
                recovery_claim_revision: 4,
                observation_attempt: 6,
                receipt_head_revision: 20,
            },
            candidate,
        ));
        for tampered in [
            (12, 4, 6, 20),
            (11, 5, 6, 20),
            (11, 4, 7, 20),
            (11, 4, 6, 21),
        ] {
            assert!(!response_claim_revisions_match_v1(
                ResponseClaimRevisionShapeV1 {
                    effect_head_revision: tampered.0,
                    recovery_claim_revision: tampered.1,
                    observation_attempt: tampered.2,
                    receipt_head_revision: tampered.3,
                },
                candidate,
            ));
        }
    }

    #[test]
    fn response_token_must_strictly_outlive_the_claim() {
        let claim = Utc.timestamp_millis_opt(10_000).single().unwrap();
        assert_eq!(response_token_outlives_claim_v1(10_001, claim), Ok(true));
        assert_eq!(response_token_outlives_claim_v1(10_000, claim), Ok(false));
        assert_eq!(response_token_outlives_claim_v1(9_999, claim), Ok(false));
    }

    #[test]
    fn response_finalize_exact_replay_is_semantically_pinned() {
        let exact = finalize_request("exact_success", InteractionEffectStateV1::Observing);
        assert!(finalize_row(
            "exact_replay",
            "reconciled_succeeded",
            11,
            "completed",
            21,
            None,
        )
        .decode(&exact)
        .is_ok());
        for tampered in [
            finalize_row("exact_replay", "known_failed", 11, "completed", 21, None),
            finalize_row(
                "exact_replay",
                "reconciled_succeeded",
                12,
                "completed",
                21,
                None,
            ),
            finalize_row(
                "provisioning_completed_response_unconfirmed",
                "reconciled_succeeded",
                11,
                "completed",
                21,
                None,
            ),
        ] {
            assert_eq!(
                tampered.decode(&exact),
                Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
            );
        }
        let deferred = finalize_request("deferred", InteractionEffectStateV1::Observing);
        assert_eq!(
            finalize_row(
                "exact_replay",
                "observation_pending",
                11,
                "completed",
                21,
                None,
            )
            .decode(&deferred),
            Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
        );
    }

    #[test]
    fn response_finalize_close_known_preserves_the_locked_effect_head() {
        let request = finalize_request("close_known_state", InteractionEffectStateV1::KnownFailed);
        let outcome = finalize_row(
            "provisioning_completed_response_unconfirmed",
            "known_failed",
            10,
            "completed",
            21,
            None,
        )
        .decode(&request)
        .unwrap();
        assert_eq!(
            outcome.disposition(),
            RuntimeInteractionEffectResponseTailFinalizeDispositionV1::ResponseUnconfirmed
        );
        assert_eq!(outcome.effect_head_revision(), 10);
    }

    #[test]
    fn response_tail_debug_values_are_redacted() {
        let request = finalize_request("exact_success", InteractionEffectStateV1::Observing);
        assert_eq!(
            format!("{request:?}"),
            "RuntimeInteractionEffectResponseTailFinalizeRequestV1(<redacted>)"
        );
        assert!(!format!("{request:?}").contains(&"c".repeat(64)));
    }

    #[test]
    fn response_scan_result_shape_requires_observation_evidence() {
        let intent = vec![1; 32];
        let result = vec![2; 32];
        assert_eq!(
            validate_response_result_shape_v1(
                InteractionEffectStateV1::ObservationPending,
                Some(&intent),
                None,
                None,
                None,
            ),
            Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
        );
        assert_eq!(
            validate_response_result_shape_v1(
                InteractionEffectStateV1::ObservationPending,
                Some(&intent),
                Some(&result),
                None,
                None,
            ),
            Ok(())
        );
    }
}
