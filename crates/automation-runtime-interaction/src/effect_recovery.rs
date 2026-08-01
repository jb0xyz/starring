use crate::effect::{
    InteractionEffectActionIndexV1, InteractionEffectCompensationClassV1,
    InteractionEffectCompensationObservationOutcomeV1, InteractionEffectCorrelationClassV1,
    InteractionEffectDefinitionV1, InteractionEffectObservationEvidenceV1,
    InteractionEffectObservationOutcomeV1, InteractionEffectRecoveryBindingV1,
    InteractionEffectStateV1,
};
use crate::effect_digest::build_interaction_effect_preimage_digest_v1;

#[cfg(test)]
use crate::effect::{
    InteractionEffectAttemptV1, InteractionEffectRecoveryRequiredReasonV1,
    MAX_INTERACTION_EFFECT_ATTEMPTS_V1,
};

#[cfg(test)]
const MAX_RETRY_DELAY_MILLISECONDS_V1: u64 = 60_000;
#[cfg(test)]
const MAX_RECOVERY_AGE_MILLISECONDS_V1: u64 = 900_000;
#[cfg(test)]
const MAX_JITTER_BASIS_POINTS_V1: u16 = 2_500;
#[cfg(test)]
const BASIS_POINTS_DENOMINATOR_V1: u128 = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionEffectObservationStrategyV1 {
    AuditLogCreateRole,
    AuditLogCreateChannel,
    AuditLogRoleGrantAndMembership,
    AuditLogPermissionAndPostimage,
    UnsupportedPostPanelCorrelation,
    InternalRegistrationRecord,
    InternalTeardownRecord,
    OriginalInteractionResponse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionEffectAbsenceProofV1 {
    NeverAutomatic,
    Authoritative,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InteractionEffectObservationProfileV1 {
    strategy: InteractionEffectObservationStrategyV1,
    correlation_class: InteractionEffectCorrelationClassV1,
    require_target_identity: bool,
    require_actor_identity: bool,
    require_postimage: bool,
    absence_proof: InteractionEffectAbsenceProofV1,
}

impl InteractionEffectObservationProfileV1 {
    pub fn strategy(self) -> InteractionEffectObservationStrategyV1 {
        self.strategy
    }

    pub fn correlation_class(self) -> InteractionEffectCorrelationClassV1 {
        self.correlation_class
    }

    pub fn require_target_identity(self) -> bool {
        self.require_target_identity
    }

    pub fn require_actor_identity(self) -> bool {
        self.require_actor_identity
    }

    pub fn require_postimage(self) -> bool {
        self.require_postimage
    }

    pub fn absence_proof(self) -> InteractionEffectAbsenceProofV1 {
        self.absence_proof
    }
}

pub fn interaction_effect_observation_profile_v1(
    definition: &InteractionEffectDefinitionV1,
) -> InteractionEffectObservationProfileV1 {
    interaction_effect_observation_profile_for_kind_v1(definition.action().kind())
}

fn interaction_effect_observation_profile_for_kind_v1(
    kind: crate::effect::InteractionEffectKindV1,
) -> InteractionEffectObservationProfileV1 {
    use crate::effect::InteractionEffectKindV1;
    match kind {
        InteractionEffectKindV1::CreateRole => InteractionEffectObservationProfileV1 {
            strategy: InteractionEffectObservationStrategyV1::AuditLogCreateRole,
            correlation_class: InteractionEffectCorrelationClassV1::AuditLogReason,
            require_target_identity: true,
            require_actor_identity: true,
            require_postimage: false,
            absence_proof: InteractionEffectAbsenceProofV1::NeverAutomatic,
        },
        InteractionEffectKindV1::CreateChannel => InteractionEffectObservationProfileV1 {
            strategy: InteractionEffectObservationStrategyV1::AuditLogCreateChannel,
            correlation_class: InteractionEffectCorrelationClassV1::AuditLogReason,
            require_target_identity: true,
            require_actor_identity: true,
            require_postimage: false,
            absence_proof: InteractionEffectAbsenceProofV1::NeverAutomatic,
        },
        InteractionEffectKindV1::GrantRole => InteractionEffectObservationProfileV1 {
            strategy: InteractionEffectObservationStrategyV1::AuditLogRoleGrantAndMembership,
            correlation_class: InteractionEffectCorrelationClassV1::AuditLogReason,
            require_target_identity: true,
            require_actor_identity: true,
            require_postimage: true,
            absence_proof: InteractionEffectAbsenceProofV1::NeverAutomatic,
        },
        InteractionEffectKindV1::UpsertOverwrite => InteractionEffectObservationProfileV1 {
            strategy: InteractionEffectObservationStrategyV1::AuditLogPermissionAndPostimage,
            correlation_class: InteractionEffectCorrelationClassV1::AuditLogReason,
            require_target_identity: true,
            require_actor_identity: true,
            require_postimage: true,
            absence_proof: InteractionEffectAbsenceProofV1::NeverAutomatic,
        },
        InteractionEffectKindV1::PostPanel => InteractionEffectObservationProfileV1 {
            strategy: InteractionEffectObservationStrategyV1::UnsupportedPostPanelCorrelation,
            correlation_class: InteractionEffectCorrelationClassV1::Unsupported,
            require_target_identity: true,
            require_actor_identity: true,
            require_postimage: true,
            absence_proof: InteractionEffectAbsenceProofV1::NeverAutomatic,
        },
        InteractionEffectKindV1::RegisterInstance => InteractionEffectObservationProfileV1 {
            strategy: InteractionEffectObservationStrategyV1::InternalRegistrationRecord,
            correlation_class: InteractionEffectCorrelationClassV1::InternalIdempotencyKey,
            require_target_identity: true,
            require_actor_identity: false,
            require_postimage: true,
            absence_proof: InteractionEffectAbsenceProofV1::Authoritative,
        },
        InteractionEffectKindV1::TeardownInstance => InteractionEffectObservationProfileV1 {
            strategy: InteractionEffectObservationStrategyV1::InternalTeardownRecord,
            correlation_class: InteractionEffectCorrelationClassV1::InternalIdempotencyKey,
            require_target_identity: true,
            require_actor_identity: false,
            require_postimage: true,
            absence_proof: InteractionEffectAbsenceProofV1::Authoritative,
        },
        InteractionEffectKindV1::EditResponse => InteractionEffectObservationProfileV1 {
            strategy: InteractionEffectObservationStrategyV1::OriginalInteractionResponse,
            correlation_class: InteractionEffectCorrelationClassV1::InteractionReceipt,
            require_target_identity: true,
            require_actor_identity: true,
            require_postimage: true,
            absence_proof: InteractionEffectAbsenceProofV1::Authoritative,
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InteractionEffectObservationValidationErrorV1 {
    #[error("interaction effect observation used the wrong correlation class")]
    CorrelationClass,
    #[error("interaction effect observation did not prove one unique exact correlation")]
    UniqueCorrelation,
    #[error("interaction effect observation did not prove the exact target identity")]
    TargetIdentity,
    #[error("interaction effect observation did not prove the expected actor identity")]
    ActorIdentity,
    #[error("interaction effect observation did not prove the expected postimage")]
    Postimage,
    #[error("interaction effect observation output does not match the bound action")]
    Output,
    #[error("interaction effect observation cannot prove absence for this action")]
    AbsenceNotConclusive,
    #[error("interaction effect conflict outcome lacks conflicting evidence")]
    ConflictEvidence,
    #[error("interaction effect compensation observation did not restore the exact preimage")]
    Preimage,
}

pub fn validate_interaction_effect_observation_v1(
    definition: &InteractionEffectDefinitionV1,
    outcome: &InteractionEffectObservationOutcomeV1,
) -> Result<(), InteractionEffectObservationValidationErrorV1> {
    let profile = interaction_effect_observation_profile_v1(definition);
    validate_evidence_class_v1(profile, outcome.evidence())?;
    match outcome {
        InteractionEffectObservationOutcomeV1::ExactMatch { output, evidence } => {
            validate_exact_evidence_v1(profile, evidence)?;
            definition
                .validate_observed_output(output)
                .map_err(|_| InteractionEffectObservationValidationErrorV1::Output)
        }
        InteractionEffectObservationOutcomeV1::ExactAbsence { evidence } => {
            if profile.absence_proof() != InteractionEffectAbsenceProofV1::Authoritative {
                return Err(InteractionEffectObservationValidationErrorV1::AbsenceNotConclusive);
            }
            if evidence.exact_correlation_matches() != 0 || evidence.conflicting_matches() != 0 {
                return Err(InteractionEffectObservationValidationErrorV1::UniqueCorrelation);
            }
            Ok(())
        }
        InteractionEffectObservationOutcomeV1::Pending { evidence } => {
            if evidence.exact_correlation_matches() > 1 || evidence.conflicting_matches() > 0 {
                return Err(InteractionEffectObservationValidationErrorV1::ConflictEvidence);
            }
            Ok(())
        }
        InteractionEffectObservationOutcomeV1::Conflict { evidence } => {
            if evidence_has_conflict_v1(profile, evidence) {
                Ok(())
            } else {
                Err(InteractionEffectObservationValidationErrorV1::ConflictEvidence)
            }
        }
        InteractionEffectObservationOutcomeV1::Unsupported { .. } => Ok(()),
    }
}

pub fn validate_interaction_effect_compensation_observation_v1(
    definition: &InteractionEffectDefinitionV1,
    outcome: &InteractionEffectCompensationObservationOutcomeV1,
) -> Result<(), InteractionEffectObservationValidationErrorV1> {
    let profile = interaction_effect_observation_profile_v1(definition);
    validate_evidence_class_v1(profile, outcome.evidence())?;
    match outcome {
        InteractionEffectCompensationObservationOutcomeV1::Restored {
            restored_preimage_digest,
            evidence,
        } => {
            validate_restored_evidence_v1(profile, evidence)?;
            if *restored_preimage_digest
                != build_interaction_effect_preimage_digest_v1(definition.preimage())
            {
                return Err(InteractionEffectObservationValidationErrorV1::Preimage);
            }
            Ok(())
        }
        InteractionEffectCompensationObservationOutcomeV1::Pending { evidence } => {
            if evidence.exact_correlation_matches() > 1 || evidence.conflicting_matches() > 0 {
                return Err(InteractionEffectObservationValidationErrorV1::ConflictEvidence);
            }
            Ok(())
        }
        InteractionEffectCompensationObservationOutcomeV1::Conflict { evidence } => {
            if compensation_evidence_has_conflict_v1(profile, evidence) {
                Ok(())
            } else {
                Err(InteractionEffectObservationValidationErrorV1::ConflictEvidence)
            }
        }
        InteractionEffectCompensationObservationOutcomeV1::Unsupported { .. } => Ok(()),
    }
}

pub fn validate_interaction_effect_recovery_observation_v1(
    binding: &InteractionEffectRecoveryBindingV1,
    outcome: &InteractionEffectObservationOutcomeV1,
) -> Result<(), InteractionEffectObservationValidationErrorV1> {
    let profile = interaction_effect_observation_profile_for_kind_v1(binding.kind());
    validate_evidence_class_v1(profile, outcome.evidence())?;
    match outcome {
        InteractionEffectObservationOutcomeV1::ExactMatch { output, evidence } => {
            if binding.kind() == crate::effect::InteractionEffectKindV1::PostPanel {
                return Err(InteractionEffectObservationValidationErrorV1::Output);
            }
            validate_exact_evidence_v1(profile, evidence)?;
            binding
                .validate_observed_output(output)
                .map_err(|_| InteractionEffectObservationValidationErrorV1::Output)
        }
        InteractionEffectObservationOutcomeV1::ExactAbsence { evidence } => {
            if profile.absence_proof() != InteractionEffectAbsenceProofV1::Authoritative {
                return Err(InteractionEffectObservationValidationErrorV1::AbsenceNotConclusive);
            }
            if evidence.exact_correlation_matches() != 0 || evidence.conflicting_matches() != 0 {
                return Err(InteractionEffectObservationValidationErrorV1::UniqueCorrelation);
            }
            Ok(())
        }
        InteractionEffectObservationOutcomeV1::Pending { evidence } => {
            if evidence.exact_correlation_matches() > 1 || evidence.conflicting_matches() > 0 {
                return Err(InteractionEffectObservationValidationErrorV1::ConflictEvidence);
            }
            Ok(())
        }
        InteractionEffectObservationOutcomeV1::Conflict { evidence } => {
            if evidence_has_conflict_v1(profile, evidence) {
                Ok(())
            } else {
                Err(InteractionEffectObservationValidationErrorV1::ConflictEvidence)
            }
        }
        InteractionEffectObservationOutcomeV1::Unsupported { .. } => Ok(()),
    }
}

pub fn validate_interaction_effect_recovery_compensation_observation_v1(
    binding: &InteractionEffectRecoveryBindingV1,
    outcome: &InteractionEffectCompensationObservationOutcomeV1,
) -> Result<(), InteractionEffectObservationValidationErrorV1> {
    let profile = interaction_effect_observation_profile_for_kind_v1(binding.kind());
    validate_evidence_class_v1(profile, outcome.evidence())?;
    match outcome {
        InteractionEffectCompensationObservationOutcomeV1::Restored {
            restored_preimage_digest,
            evidence,
        } => {
            validate_restored_evidence_v1(profile, evidence)?;
            if restored_preimage_digest != binding.preimage_digest() {
                return Err(InteractionEffectObservationValidationErrorV1::Preimage);
            }
            Ok(())
        }
        InteractionEffectCompensationObservationOutcomeV1::Pending { evidence } => {
            if evidence.exact_correlation_matches() > 1 || evidence.conflicting_matches() > 0 {
                return Err(InteractionEffectObservationValidationErrorV1::ConflictEvidence);
            }
            Ok(())
        }
        InteractionEffectCompensationObservationOutcomeV1::Conflict { evidence } => {
            if compensation_evidence_has_conflict_v1(profile, evidence) {
                Ok(())
            } else {
                Err(InteractionEffectObservationValidationErrorV1::ConflictEvidence)
            }
        }
        InteractionEffectCompensationObservationOutcomeV1::Unsupported { .. } => Ok(()),
    }
}

fn validate_evidence_class_v1(
    profile: InteractionEffectObservationProfileV1,
    evidence: &InteractionEffectObservationEvidenceV1,
) -> Result<(), InteractionEffectObservationValidationErrorV1> {
    if profile.correlation_class() != evidence.correlation_class() {
        return Err(InteractionEffectObservationValidationErrorV1::CorrelationClass);
    }
    Ok(())
}

fn validate_exact_evidence_v1(
    profile: InteractionEffectObservationProfileV1,
    evidence: &InteractionEffectObservationEvidenceV1,
) -> Result<(), InteractionEffectObservationValidationErrorV1> {
    if evidence.exact_correlation_matches() != 1 || evidence.conflicting_matches() != 0 {
        return Err(InteractionEffectObservationValidationErrorV1::UniqueCorrelation);
    }
    if profile.require_target_identity() && !evidence.target_identity_matches() {
        return Err(InteractionEffectObservationValidationErrorV1::TargetIdentity);
    }
    if profile.require_actor_identity() && !evidence.actor_identity_matches() {
        return Err(InteractionEffectObservationValidationErrorV1::ActorIdentity);
    }
    if profile.require_postimage() && !evidence.postimage_matches() {
        return Err(InteractionEffectObservationValidationErrorV1::Postimage);
    }
    Ok(())
}

fn validate_restored_evidence_v1(
    profile: InteractionEffectObservationProfileV1,
    evidence: &InteractionEffectObservationEvidenceV1,
) -> Result<(), InteractionEffectObservationValidationErrorV1> {
    if evidence.exact_correlation_matches() > 1 || evidence.conflicting_matches() != 0 {
        return Err(InteractionEffectObservationValidationErrorV1::UniqueCorrelation);
    }
    if !evidence.target_identity_matches() {
        return Err(InteractionEffectObservationValidationErrorV1::TargetIdentity);
    }
    if evidence.exact_correlation_matches() == 1
        && profile.require_actor_identity()
        && !evidence.actor_identity_matches()
    {
        return Err(InteractionEffectObservationValidationErrorV1::ActorIdentity);
    }
    if !evidence.postimage_matches() {
        return Err(InteractionEffectObservationValidationErrorV1::Postimage);
    }
    Ok(())
}

fn evidence_has_conflict_v1(
    profile: InteractionEffectObservationProfileV1,
    evidence: &InteractionEffectObservationEvidenceV1,
) -> bool {
    evidence.exact_correlation_matches() > 1
        || evidence.conflicting_matches() > 0
        || (evidence.exact_correlation_matches() == 1
            && ((profile.require_target_identity() && !evidence.target_identity_matches())
                || (profile.require_actor_identity() && !evidence.actor_identity_matches())
                || (profile.require_postimage() && !evidence.postimage_matches())))
}

fn compensation_evidence_has_conflict_v1(
    profile: InteractionEffectObservationProfileV1,
    evidence: &InteractionEffectObservationEvidenceV1,
) -> bool {
    evidence.exact_correlation_matches() > 1
        || evidence.conflicting_matches() > 0
        || !evidence.target_identity_matches()
        || !evidence.postimage_matches()
        || (evidence.exact_correlation_matches() == 1
            && profile.require_actor_identity()
            && !evidence.actor_identity_matches())
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InteractionEffectRetryOperationV1 {
    Observation,
    Compensation,
    CompensationObservation,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InteractionEffectRetryPolicyV1 {
    max_observation_attempts: u16,
    max_compensation_attempts: u16,
    max_compensation_observation_attempts: u16,
    initial_delay_milliseconds: u64,
    max_delay_milliseconds: u64,
    max_age_milliseconds: u64,
    multiplier: u16,
    jitter_basis_points: u16,
}

#[cfg(test)]
impl Default for InteractionEffectRetryPolicyV1 {
    fn default() -> Self {
        Self {
            max_observation_attempts: 6,
            max_compensation_attempts: 1,
            max_compensation_observation_attempts: 6,
            initial_delay_milliseconds: 250,
            max_delay_milliseconds: 5_000,
            max_age_milliseconds: 120_000,
            multiplier: 2,
            jitter_basis_points: 1_000,
        }
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InteractionEffectRetryPolicyErrorV1 {
    #[error("interaction effect retry attempt bound is invalid")]
    AttemptBound,
    #[error("interaction effect retry delay bound is invalid")]
    DelayBound,
    #[error("interaction effect retry age bound is invalid")]
    AgeBound,
    #[error("interaction effect retry multiplier is invalid")]
    Multiplier,
    #[error("interaction effect retry jitter is invalid")]
    Jitter,
    #[error("interaction effect retry attempt is exhausted")]
    AttemptExhausted,
    #[error("interaction effect retry age is exhausted")]
    AgeExhausted,
    #[error("interaction effect retry arithmetic overflowed")]
    Overflow,
}

#[cfg(test)]
impl InteractionEffectRetryPolicyV1 {
    pub fn validate(self) -> Result<(), InteractionEffectRetryPolicyErrorV1> {
        if self.max_observation_attempts == 0
            || self.max_compensation_attempts == 0
            || self.max_compensation_observation_attempts == 0
            || self.max_observation_attempts > MAX_INTERACTION_EFFECT_ATTEMPTS_V1
            || self.max_compensation_attempts > MAX_INTERACTION_EFFECT_ATTEMPTS_V1
            || self.max_compensation_observation_attempts > MAX_INTERACTION_EFFECT_ATTEMPTS_V1
        {
            return Err(InteractionEffectRetryPolicyErrorV1::AttemptBound);
        }
        if self.initial_delay_milliseconds == 0
            || self.initial_delay_milliseconds > self.max_delay_milliseconds
            || self.max_delay_milliseconds > MAX_RETRY_DELAY_MILLISECONDS_V1
        {
            return Err(InteractionEffectRetryPolicyErrorV1::DelayBound);
        }
        if self.max_age_milliseconds < self.max_delay_milliseconds
            || self.max_age_milliseconds > MAX_RECOVERY_AGE_MILLISECONDS_V1
        {
            return Err(InteractionEffectRetryPolicyErrorV1::AgeBound);
        }
        if !(1..=4).contains(&self.multiplier) {
            return Err(InteractionEffectRetryPolicyErrorV1::Multiplier);
        }
        if self.jitter_basis_points > MAX_JITTER_BASIS_POINTS_V1 {
            return Err(InteractionEffectRetryPolicyErrorV1::Jitter);
        }
        Ok(())
    }

    pub fn max_attempts(self, operation: InteractionEffectRetryOperationV1) -> u16 {
        match operation {
            InteractionEffectRetryOperationV1::Observation => self.max_observation_attempts,
            InteractionEffectRetryOperationV1::Compensation => self.max_compensation_attempts,
            InteractionEffectRetryOperationV1::CompensationObservation => {
                self.max_compensation_observation_attempts
            }
        }
    }

    pub fn max_age_milliseconds(self) -> u64 {
        self.max_age_milliseconds
    }

    pub fn retry_after_v1(
        self,
        operation: InteractionEffectRetryOperationV1,
        completed_attempts: u16,
        elapsed_milliseconds: u64,
        entropy: u64,
    ) -> Result<InteractionEffectScheduledRetryV1, InteractionEffectRetryPolicyErrorV1> {
        self.validate()?;
        if elapsed_milliseconds >= self.max_age_milliseconds {
            return Err(InteractionEffectRetryPolicyErrorV1::AgeExhausted);
        }
        let next = completed_attempts
            .checked_add(1)
            .ok_or(InteractionEffectRetryPolicyErrorV1::Overflow)?;
        if next > self.max_attempts(operation) {
            return Err(InteractionEffectRetryPolicyErrorV1::AttemptExhausted);
        }
        let attempt = InteractionEffectAttemptV1::new(next)
            .map_err(|_| InteractionEffectRetryPolicyErrorV1::AttemptBound)?;
        let delay = self.delay_milliseconds_v1(attempt, entropy)?;
        if elapsed_milliseconds.saturating_add(delay) > self.max_age_milliseconds {
            return Err(InteractionEffectRetryPolicyErrorV1::AgeExhausted);
        }
        Ok(InteractionEffectScheduledRetryV1 {
            attempt,
            delay_milliseconds: delay,
        })
    }

    pub fn delay_milliseconds_v1(
        self,
        attempt: InteractionEffectAttemptV1,
        entropy: u64,
    ) -> Result<u64, InteractionEffectRetryPolicyErrorV1> {
        self.validate()?;
        let mut delay = u128::from(self.initial_delay_milliseconds);
        for _ in 1..attempt.get() {
            delay = delay
                .checked_mul(u128::from(self.multiplier))
                .ok_or(InteractionEffectRetryPolicyErrorV1::Overflow)?;
            delay = delay.min(u128::from(self.max_delay_milliseconds));
        }
        let jitter_range = delay
            .checked_mul(u128::from(self.jitter_basis_points))
            .ok_or(InteractionEffectRetryPolicyErrorV1::Overflow)?
            / BASIS_POINTS_DENOMINATOR_V1;
        let width = jitter_range
            .checked_mul(2)
            .and_then(|value| value.checked_add(1))
            .ok_or(InteractionEffectRetryPolicyErrorV1::Overflow)?;
        let offset = if width == 0 {
            0
        } else {
            i128::try_from(u128::from(entropy) % width)
                .map_err(|_| InteractionEffectRetryPolicyErrorV1::Overflow)?
                - i128::try_from(jitter_range)
                    .map_err(|_| InteractionEffectRetryPolicyErrorV1::Overflow)?
        };
        let jittered = i128::try_from(delay)
            .map_err(|_| InteractionEffectRetryPolicyErrorV1::Overflow)?
            .checked_add(offset)
            .ok_or(InteractionEffectRetryPolicyErrorV1::Overflow)?
            .max(1);
        u64::try_from(jittered.min(i128::from(self.max_delay_milliseconds)))
            .map_err(|_| InteractionEffectRetryPolicyErrorV1::Overflow)
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InteractionEffectScheduledRetryV1 {
    attempt: InteractionEffectAttemptV1,
    delay_milliseconds: u64,
}

#[cfg(test)]
impl InteractionEffectScheduledRetryV1 {
    pub fn attempt(self) -> InteractionEffectAttemptV1 {
        self.attempt
    }

    pub fn delay_milliseconds(self) -> u64 {
        self.delay_milliseconds
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InteractionEffectRecoveryProgressV1 {
    observation_attempts: u16,
    compensation_attempts: u16,
    compensation_observation_attempts: u16,
    elapsed_milliseconds: u64,
}

#[cfg(test)]
impl InteractionEffectRecoveryProgressV1 {
    pub fn new(
        observation_attempts: u16,
        compensation_attempts: u16,
        compensation_observation_attempts: u16,
        elapsed_milliseconds: u64,
    ) -> Result<Self, InteractionEffectRetryPolicyErrorV1> {
        if observation_attempts > MAX_INTERACTION_EFFECT_ATTEMPTS_V1
            || compensation_attempts > MAX_INTERACTION_EFFECT_ATTEMPTS_V1
            || compensation_observation_attempts > MAX_INTERACTION_EFFECT_ATTEMPTS_V1
        {
            return Err(InteractionEffectRetryPolicyErrorV1::AttemptBound);
        }
        Ok(Self {
            observation_attempts,
            compensation_attempts,
            compensation_observation_attempts,
            elapsed_milliseconds,
        })
    }

    pub fn observation_attempts(self) -> u16 {
        self.observation_attempts
    }

    pub fn compensation_attempts(self) -> u16 {
        self.compensation_attempts
    }

    pub fn compensation_observation_attempts(self) -> u16 {
        self.compensation_observation_attempts
    }

    pub fn elapsed_milliseconds(self) -> u64 {
        self.elapsed_milliseconds
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionEffectRecoveryDecisionV1 {
    ConvergedNoExternalEffect,
    ConvergedCompensated,
    ObserveEffect(InteractionEffectScheduledRetryV1),
    BeginCompensation(InteractionEffectScheduledRetryV1),
    ObserveCompensation(InteractionEffectScheduledRetryV1),
    RecoveryRequired(InteractionEffectRecoveryRequiredReasonV1),
    RemainRecoveryRequired,
}

#[cfg(test)]
pub fn decide_interaction_effect_recovery_v1(
    definition: &InteractionEffectDefinitionV1,
    state: InteractionEffectStateV1,
    progress: InteractionEffectRecoveryProgressV1,
    policy: InteractionEffectRetryPolicyV1,
    entropy: u64,
) -> InteractionEffectRecoveryDecisionV1 {
    match state {
        InteractionEffectStateV1::Planned | InteractionEffectStateV1::KnownFailed => {
            InteractionEffectRecoveryDecisionV1::ConvergedNoExternalEffect
        }
        InteractionEffectStateV1::Compensated => {
            InteractionEffectRecoveryDecisionV1::ConvergedCompensated
        }
        InteractionEffectStateV1::RecoveryRequired => {
            InteractionEffectRecoveryDecisionV1::RemainRecoveryRequired
        }
        InteractionEffectStateV1::Intended
        | InteractionEffectStateV1::Indeterminate
        | InteractionEffectStateV1::Observing
        | InteractionEffectStateV1::ObservationPending => schedule_recovery_v1(
            policy,
            InteractionEffectRetryOperationV1::Observation,
            progress.observation_attempts(),
            progress.elapsed_milliseconds(),
            entropy,
            InteractionEffectRecoveryRequiredReasonV1::ObservationBudgetExhausted,
            InteractionEffectRecoveryDecisionV1::ObserveEffect,
        ),
        InteractionEffectStateV1::KnownSucceeded
        | InteractionEffectStateV1::ReconciledSucceeded => {
            if definition.compensation_class()
                == InteractionEffectCompensationClassV1::NotCompensable
            {
                return InteractionEffectRecoveryDecisionV1::RecoveryRequired(
                    InteractionEffectRecoveryRequiredReasonV1::NonCompensableSuccess,
                );
            }
            schedule_recovery_v1(
                policy,
                InteractionEffectRetryOperationV1::Compensation,
                progress.compensation_attempts(),
                progress.elapsed_milliseconds(),
                entropy,
                InteractionEffectRecoveryRequiredReasonV1::CompensationBudgetExhausted,
                InteractionEffectRecoveryDecisionV1::BeginCompensation,
            )
        }
        InteractionEffectStateV1::CompensationIntended
        | InteractionEffectStateV1::CompensationIndeterminate
        | InteractionEffectStateV1::CompensationObserving
        | InteractionEffectStateV1::CompensationObservationPending => schedule_recovery_v1(
            policy,
            InteractionEffectRetryOperationV1::CompensationObservation,
            progress.compensation_observation_attempts(),
            progress.elapsed_milliseconds(),
            entropy,
            InteractionEffectRecoveryRequiredReasonV1::CompensationBudgetExhausted,
            InteractionEffectRecoveryDecisionV1::ObserveCompensation,
        ),
    }
}

#[cfg(test)]
fn schedule_recovery_v1(
    policy: InteractionEffectRetryPolicyV1,
    operation: InteractionEffectRetryOperationV1,
    completed_attempts: u16,
    elapsed_milliseconds: u64,
    entropy: u64,
    exhausted_reason: InteractionEffectRecoveryRequiredReasonV1,
    build: fn(InteractionEffectScheduledRetryV1) -> InteractionEffectRecoveryDecisionV1,
) -> InteractionEffectRecoveryDecisionV1 {
    match policy.retry_after_v1(operation, completed_attempts, elapsed_milliseconds, entropy) {
        Ok(retry) => build(retry),
        Err(_) => InteractionEffectRecoveryDecisionV1::RecoveryRequired(exhausted_reason),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InteractionEffectCompensationOrderErrorV1 {
    #[error("interaction effect compensation input contains duplicate action indices")]
    DuplicateActionIndex,
}

pub fn build_interaction_effect_compensation_order_v1(
    effects: &[(&InteractionEffectDefinitionV1, InteractionEffectStateV1)],
) -> Result<Vec<InteractionEffectActionIndexV1>, InteractionEffectCompensationOrderErrorV1> {
    let mut ordered = effects
        .iter()
        .filter_map(|(definition, state)| {
            (state.has_known_success()
                && definition.compensation_class()
                    != InteractionEffectCompensationClassV1::NotCompensable)
                .then_some(definition.action().action_index())
        })
        .collect::<Vec<_>>();
    ordered.sort_unstable();
    if ordered.windows(2).any(|window| window[0] == window[1]) {
        return Err(InteractionEffectCompensationOrderErrorV1::DuplicateActionIndex);
    }
    ordered.reverse();
    Ok(ordered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::{
        InteractionEffectActionIdentityV1, InteractionEffectActionIndexV1,
        InteractionEffectChannelIdV1, InteractionEffectGuildIdV1, InteractionEffectInstanceStateV1,
        InteractionEffectInstanceTargetV1, InteractionEffectKindV1,
        InteractionEffectObservedOutputV1, InteractionEffectOverwriteTargetV1,
        InteractionEffectPermissionStateV1, InteractionEffectPermissionTargetV1,
        InteractionEffectPermissionValueV1, InteractionEffectPreimageV1, InteractionEffectRoleIdV1,
        InteractionEffectRoleMembershipTargetV1, InteractionEffectTargetV1,
        InteractionEffectTransitionErrorV1, InteractionEffectTransitionV1,
        InteractionEffectUserIdV1,
    };
    use crate::effect_digest::{
        InteractionEffectActionDigestV1, InteractionEffectInputDigestV1,
        InteractionEffectObservationEvidenceDigestV1, InteractionEffectOpaqueIdentityDigestV1,
        InteractionEffectPayloadDigestV1,
    };
    use crate::{
        validate_interaction_effect_transition_v1, DiscordApplicationIdV1, DiscordInteractionIdV1,
        InteractionActionPlanDigestV1, InteractionPreflightCertificateDigestV1,
        InteractionReceiptIdentityV1,
    };

    fn hex(value: char) -> String {
        value.to_string().repeat(64)
    }

    fn receipt() -> InteractionReceiptIdentityV1 {
        InteractionReceiptIdentityV1::new(
            DiscordApplicationIdV1::new(1).unwrap(),
            DiscordInteractionIdV1::new(2).unwrap(),
        )
    }

    fn action(index: u16, kind: InteractionEffectKindV1) -> InteractionEffectActionIdentityV1 {
        InteractionEffectActionIdentityV1::new(
            receipt(),
            InteractionActionPlanDigestV1::parse(hex('a')).unwrap(),
            InteractionPreflightCertificateDigestV1::parse(hex('b')).unwrap(),
            InteractionEffectActionIndexV1::new(index).unwrap(),
            kind,
            InteractionEffectActionDigestV1::parse(hex('c')).unwrap(),
            InteractionEffectInputDigestV1::parse(hex('d')).unwrap(),
        )
    }

    fn role_definition(index: u16) -> InteractionEffectDefinitionV1 {
        InteractionEffectDefinitionV1::new(
            action(index, InteractionEffectKindV1::CreateRole),
            InteractionEffectTargetV1::CreateRole {
                guild_id: InteractionEffectGuildIdV1::new(10).unwrap(),
            },
            InteractionEffectPreimageV1::None,
            Vec::new(),
        )
        .unwrap()
    }

    fn registration_definition(index: u16) -> InteractionEffectDefinitionV1 {
        let target = InteractionEffectInstanceTargetV1::new(
            InteractionEffectGuildIdV1::new(10).unwrap(),
            InteractionEffectOpaqueIdentityDigestV1::parse(hex('e')).unwrap(),
        );
        InteractionEffectDefinitionV1::new(
            action(index, InteractionEffectKindV1::RegisterInstance),
            InteractionEffectTargetV1::RegisterInstance {
                target: target.clone(),
                kind: automation_instance::InstanceKind("study".to_string()),
                manifest_digest: InteractionEffectPayloadDigestV1::parse(hex('f')).unwrap(),
            },
            InteractionEffectPreimageV1::InstanceRegistration {
                target,
                before: InteractionEffectInstanceStateV1::Absent,
            },
            Vec::new(),
        )
        .unwrap()
    }

    fn role_output() -> InteractionEffectObservedOutputV1 {
        InteractionEffectObservedOutputV1::CreatedRole {
            guild_id: InteractionEffectGuildIdV1::new(10).unwrap(),
            role_id: InteractionEffectRoleIdV1::new(11).unwrap(),
        }
    }

    fn evidence(
        class: InteractionEffectCorrelationClassV1,
        exact: u16,
        conflicts: u16,
        target: bool,
        actor: bool,
        postimage: bool,
    ) -> InteractionEffectObservationEvidenceV1 {
        InteractionEffectObservationEvidenceV1::new(
            InteractionEffectObservationEvidenceDigestV1::from_canonical_bytes(b"evidence"),
            class,
            exact,
            conflicts,
            target,
            actor,
            postimage,
        )
    }

    #[test]
    fn observation_matrix_requires_exact_action_specific_evidence() {
        let role = role_definition(0);
        let profile = interaction_effect_observation_profile_v1(&role);
        assert_eq!(
            profile.strategy(),
            InteractionEffectObservationStrategyV1::AuditLogCreateRole
        );
        assert_eq!(
            profile.absence_proof(),
            InteractionEffectAbsenceProofV1::NeverAutomatic
        );
        let registration = registration_definition(1);
        assert_eq!(
            interaction_effect_observation_profile_v1(&registration).absence_proof(),
            InteractionEffectAbsenceProofV1::Authoritative
        );
    }

    #[test]
    fn name_or_attribute_only_match_is_never_adopted() {
        let definition = role_definition(0);
        let outcome = InteractionEffectObservationOutcomeV1::ExactMatch {
            output: role_output(),
            evidence: evidence(
                InteractionEffectCorrelationClassV1::AuditLogReason,
                0,
                0,
                true,
                true,
                true,
            ),
        };
        assert_eq!(
            validate_interaction_effect_observation_v1(&definition, &outcome),
            Err(InteractionEffectObservationValidationErrorV1::UniqueCorrelation)
        );
    }

    #[test]
    fn duplicate_correlation_and_wrong_actor_fail_closed() {
        let definition = role_definition(0);
        let duplicate = InteractionEffectObservationOutcomeV1::ExactMatch {
            output: role_output(),
            evidence: evidence(
                InteractionEffectCorrelationClassV1::AuditLogReason,
                2,
                0,
                true,
                true,
                true,
            ),
        };
        assert_eq!(
            validate_interaction_effect_observation_v1(&definition, &duplicate),
            Err(InteractionEffectObservationValidationErrorV1::UniqueCorrelation)
        );
        let wrong_actor = InteractionEffectObservationOutcomeV1::ExactMatch {
            output: role_output(),
            evidence: evidence(
                InteractionEffectCorrelationClassV1::AuditLogReason,
                1,
                0,
                true,
                false,
                true,
            ),
        };
        assert_eq!(
            validate_interaction_effect_observation_v1(&definition, &wrong_actor),
            Err(InteractionEffectObservationValidationErrorV1::ActorIdentity)
        );
    }

    #[test]
    fn audit_absence_is_not_conclusive_but_internal_absence_is() {
        let audit = role_definition(0);
        let audit_absence = InteractionEffectObservationOutcomeV1::ExactAbsence {
            evidence: evidence(
                InteractionEffectCorrelationClassV1::AuditLogReason,
                0,
                0,
                false,
                false,
                false,
            ),
        };
        assert_eq!(
            validate_interaction_effect_observation_v1(&audit, &audit_absence),
            Err(InteractionEffectObservationValidationErrorV1::AbsenceNotConclusive)
        );
        let registration = registration_definition(1);
        let internal_absence = InteractionEffectObservationOutcomeV1::ExactAbsence {
            evidence: evidence(
                InteractionEffectCorrelationClassV1::InternalIdempotencyKey,
                0,
                0,
                false,
                false,
                false,
            ),
        };
        assert!(
            validate_interaction_effect_observation_v1(&registration, &internal_absence).is_ok()
        );
    }

    #[test]
    fn compensation_restoration_accepts_exact_state_without_fabricated_correlation() {
        let definition = role_definition(0);
        let outcome = InteractionEffectCompensationObservationOutcomeV1::Restored {
            restored_preimage_digest: build_interaction_effect_preimage_digest_v1(
                definition.preimage(),
            ),
            evidence: evidence(
                InteractionEffectCorrelationClassV1::AuditLogReason,
                0,
                0,
                true,
                false,
                true,
            ),
        };
        assert!(
            validate_interaction_effect_compensation_observation_v1(&definition, &outcome).is_ok()
        );
    }

    #[test]
    fn compensation_restoration_rejects_ambiguous_or_inexact_state() {
        let definition = role_definition(0);
        let preimage = build_interaction_effect_preimage_digest_v1(definition.preimage());
        let cases = [
            (
                evidence(
                    InteractionEffectCorrelationClassV1::AuditLogReason,
                    2,
                    0,
                    true,
                    true,
                    true,
                ),
                InteractionEffectObservationValidationErrorV1::UniqueCorrelation,
            ),
            (
                evidence(
                    InteractionEffectCorrelationClassV1::AuditLogReason,
                    0,
                    1,
                    true,
                    false,
                    true,
                ),
                InteractionEffectObservationValidationErrorV1::UniqueCorrelation,
            ),
            (
                evidence(
                    InteractionEffectCorrelationClassV1::AuditLogReason,
                    0,
                    0,
                    false,
                    false,
                    true,
                ),
                InteractionEffectObservationValidationErrorV1::TargetIdentity,
            ),
            (
                evidence(
                    InteractionEffectCorrelationClassV1::AuditLogReason,
                    0,
                    0,
                    true,
                    false,
                    false,
                ),
                InteractionEffectObservationValidationErrorV1::Postimage,
            ),
            (
                evidence(
                    InteractionEffectCorrelationClassV1::AuditLogReason,
                    1,
                    0,
                    true,
                    false,
                    true,
                ),
                InteractionEffectObservationValidationErrorV1::ActorIdentity,
            ),
        ];
        for (evidence, expected) in cases {
            let outcome = InteractionEffectCompensationObservationOutcomeV1::Restored {
                restored_preimage_digest: preimage.clone(),
                evidence,
            };
            assert_eq!(
                validate_interaction_effect_compensation_observation_v1(&definition, &outcome),
                Err(expected)
            );
        }
    }

    #[test]
    fn compensation_conflict_can_be_proven_by_exact_target_state_mismatch() {
        let definition = role_definition(0);
        let outcome = InteractionEffectCompensationObservationOutcomeV1::Conflict {
            evidence: evidence(
                InteractionEffectCorrelationClassV1::AuditLogReason,
                0,
                0,
                true,
                false,
                false,
            ),
        };
        assert!(
            validate_interaction_effect_compensation_observation_v1(&definition, &outcome).is_ok()
        );
    }

    fn recovery_binding(
        target: crate::effect::InteractionEffectRecoveryTargetV1,
    ) -> InteractionEffectRecoveryBindingV1 {
        let planned =
            crate::effect_digest::InteractionEffectPlannedIdentityDigestV1::parse(hex('1'))
                .unwrap();
        let correlation = crate::effect_digest::build_interaction_effect_recovery_correlation_v1(
            &planned,
            match target.kind() {
                InteractionEffectKindV1::PostPanel => {
                    InteractionEffectCorrelationClassV1::Unsupported
                }
                InteractionEffectKindV1::EditResponse => {
                    InteractionEffectCorrelationClassV1::InteractionReceipt
                }
                _ => InteractionEffectCorrelationClassV1::AuditLogReason,
            },
        );
        InteractionEffectRecoveryBindingV1::new(
            target,
            InteractionEffectPreimageV1::None,
            planned,
            crate::effect_digest::InteractionEffectIdentityDigestV1::parse(hex('2')).unwrap(),
            crate::effect_digest::InteractionEffectExpectedPostimageDigestV1::parse(hex('3'))
                .unwrap(),
            correlation,
        )
        .unwrap()
    }

    #[test]
    fn response_tail_recovery_binds_exact_receipt_and_persisted_payload_digest() {
        let payload = InteractionEffectPayloadDigestV1::parse(hex('4')).unwrap();
        let binding = recovery_binding(
            crate::effect::InteractionEffectRecoveryTargetV1::EditResponse {
                receipt_identity: receipt(),
                payload_digest: payload.clone(),
            },
        );
        let exact = InteractionEffectObservationOutcomeV1::ExactMatch {
            output: InteractionEffectObservedOutputV1::OriginalResponse {
                receipt_identity: receipt(),
                payload_digest: payload,
            },
            evidence: evidence(
                InteractionEffectCorrelationClassV1::InteractionReceipt,
                1,
                0,
                true,
                true,
                true,
            ),
        };
        assert!(validate_interaction_effect_recovery_observation_v1(&binding, &exact).is_ok());
        let tampered = InteractionEffectObservationOutcomeV1::ExactMatch {
            output: InteractionEffectObservedOutputV1::OriginalResponse {
                receipt_identity: receipt(),
                payload_digest: InteractionEffectPayloadDigestV1::parse(hex('5')).unwrap(),
            },
            evidence: exact.evidence().clone(),
        };
        assert_eq!(
            validate_interaction_effect_recovery_observation_v1(&binding, &tampered),
            Err(InteractionEffectObservationValidationErrorV1::Output)
        );
    }

    #[test]
    fn post_panel_recovery_never_adopts_success_without_exact_correlation() {
        let payload = InteractionEffectPayloadDigestV1::parse(hex('6')).unwrap();
        let binding = recovery_binding(
            crate::effect::InteractionEffectRecoveryTargetV1::PostPanel {
                guild_id: InteractionEffectGuildIdV1::new(10).unwrap(),
                channel_id: crate::effect::InteractionEffectChannelIdV1::new(11).unwrap(),
                payload_digest: payload.clone(),
            },
        );
        let outcome = InteractionEffectObservationOutcomeV1::ExactMatch {
            output: InteractionEffectObservedOutputV1::PostedMessage {
                guild_id: InteractionEffectGuildIdV1::new(10).unwrap(),
                channel_id: crate::effect::InteractionEffectChannelIdV1::new(11).unwrap(),
                message_id: crate::effect::InteractionEffectMessageIdV1::new(12).unwrap(),
                payload_digest: payload,
            },
            evidence: evidence(
                InteractionEffectCorrelationClassV1::Unsupported,
                1,
                0,
                true,
                true,
                true,
            ),
        };
        assert_eq!(
            validate_interaction_effect_recovery_observation_v1(&binding, &outcome),
            Err(InteractionEffectObservationValidationErrorV1::Output)
        );
    }

    #[test]
    fn retry_policy_is_deterministic_bounded_and_age_limited() {
        let policy = InteractionEffectRetryPolicyV1::default();
        let first = policy
            .retry_after_v1(InteractionEffectRetryOperationV1::Observation, 0, 0, 7)
            .unwrap();
        assert_eq!(
            first,
            policy
                .retry_after_v1(InteractionEffectRetryOperationV1::Observation, 0, 0, 7)
                .unwrap()
        );
        assert_eq!(first.attempt().get(), 1);
        assert!(first.delay_milliseconds() <= 5_000);
        assert_eq!(
            policy.retry_after_v1(InteractionEffectRetryOperationV1::Observation, 6, 1_000, 7,),
            Err(InteractionEffectRetryPolicyErrorV1::AttemptExhausted)
        );
        assert_eq!(
            policy.retry_after_v1(
                InteractionEffectRetryOperationV1::Observation,
                0,
                policy.max_age_milliseconds(),
                7,
            ),
            Err(InteractionEffectRetryPolicyErrorV1::AgeExhausted)
        );
    }

    #[test]
    fn recovery_never_replays_an_unfinalized_forward_intent() {
        let definition = role_definition(0);
        let progress = InteractionEffectRecoveryProgressV1::new(0, 0, 0, 0).unwrap();
        assert!(matches!(
            decide_interaction_effect_recovery_v1(
                &definition,
                InteractionEffectStateV1::Intended,
                progress,
                InteractionEffectRetryPolicyV1::default(),
                1,
            ),
            InteractionEffectRecoveryDecisionV1::ObserveEffect(_)
        ));
        assert!(matches!(
            decide_interaction_effect_recovery_v1(
                &definition,
                InteractionEffectStateV1::KnownSucceeded,
                progress,
                InteractionEffectRetryPolicyV1::default(),
                1,
            ),
            InteractionEffectRecoveryDecisionV1::BeginCompensation(_)
        ));
        assert!(matches!(
            decide_interaction_effect_recovery_v1(
                &definition,
                InteractionEffectStateV1::CompensationIntended,
                progress,
                InteractionEffectRetryPolicyV1::default(),
                1,
            ),
            InteractionEffectRecoveryDecisionV1::ObserveCompensation(_)
        ));
    }

    #[test]
    fn every_effect_kind_preserves_crash_and_compensation_recovery_boundaries() {
        let guild = InteractionEffectGuildIdV1::new(10).unwrap();
        let member = InteractionEffectRoleMembershipTargetV1::new(
            guild,
            InteractionEffectUserIdV1::new(12).unwrap(),
            InteractionEffectRoleIdV1::new(13).unwrap(),
        );
        let permission = InteractionEffectPermissionTargetV1::new(
            guild,
            InteractionEffectChannelIdV1::new(14).unwrap(),
            InteractionEffectOverwriteTargetV1::Role(InteractionEffectRoleIdV1::new(13).unwrap()),
        );
        let instance = InteractionEffectInstanceTargetV1::new(
            guild,
            InteractionEffectOpaqueIdentityDigestV1::parse(hex('e')).unwrap(),
        );
        let payload = InteractionEffectPayloadDigestV1::parse(hex('f')).unwrap();
        let definitions = vec![
            InteractionEffectDefinitionV1::new(
                action(0, InteractionEffectKindV1::CreateRole),
                InteractionEffectTargetV1::CreateRole { guild_id: guild },
                InteractionEffectPreimageV1::None,
                Vec::new(),
            )
            .unwrap(),
            InteractionEffectDefinitionV1::new(
                action(1, InteractionEffectKindV1::CreateChannel),
                InteractionEffectTargetV1::CreateChannel { guild_id: guild },
                InteractionEffectPreimageV1::None,
                Vec::new(),
            )
            .unwrap(),
            InteractionEffectDefinitionV1::new(
                action(2, InteractionEffectKindV1::GrantRole),
                InteractionEffectTargetV1::GrantRole { target: member },
                InteractionEffectPreimageV1::RoleMembership {
                    target: member,
                    present: false,
                },
                Vec::new(),
            )
            .unwrap(),
            InteractionEffectDefinitionV1::new(
                action(3, InteractionEffectKindV1::UpsertOverwrite),
                InteractionEffectTargetV1::UpsertOverwrite {
                    target: permission,
                    desired: InteractionEffectPermissionValueV1::new(1, 2).unwrap(),
                },
                InteractionEffectPreimageV1::PermissionOverwrite {
                    target: permission,
                    before: InteractionEffectPermissionStateV1::Absent,
                },
                Vec::new(),
            )
            .unwrap(),
            InteractionEffectDefinitionV1::new(
                action(4, InteractionEffectKindV1::PostPanel),
                InteractionEffectTargetV1::PostPanel {
                    guild_id: guild,
                    channel_id: InteractionEffectChannelIdV1::new(14).unwrap(),
                    payload_digest: payload.clone(),
                },
                InteractionEffectPreimageV1::None,
                Vec::new(),
            )
            .unwrap(),
            InteractionEffectDefinitionV1::new(
                action(5, InteractionEffectKindV1::RegisterInstance),
                InteractionEffectTargetV1::RegisterInstance {
                    target: instance.clone(),
                    kind: automation_instance::InstanceKind("study".to_string()),
                    manifest_digest: payload.clone(),
                },
                InteractionEffectPreimageV1::InstanceRegistration {
                    target: instance.clone(),
                    before: InteractionEffectInstanceStateV1::Absent,
                },
                Vec::new(),
            )
            .unwrap(),
            InteractionEffectDefinitionV1::new(
                action(6, InteractionEffectKindV1::TeardownInstance),
                InteractionEffectTargetV1::TeardownInstance {
                    target: instance.clone(),
                },
                InteractionEffectPreimageV1::InstanceRegistration {
                    target: instance,
                    before: InteractionEffectInstanceStateV1::Present {
                        manifest_digest: payload.clone(),
                    },
                },
                Vec::new(),
            )
            .unwrap(),
            InteractionEffectDefinitionV1::new(
                action(7, InteractionEffectKindV1::EditResponse),
                InteractionEffectTargetV1::EditResponse {
                    receipt_identity: receipt(),
                    payload_digest: payload,
                },
                InteractionEffectPreimageV1::None,
                Vec::new(),
            )
            .unwrap(),
        ];
        let progress = InteractionEffectRecoveryProgressV1::new(0, 0, 0, 0).unwrap();
        let policy = InteractionEffectRetryPolicyV1::default();
        for definition in definitions {
            for state in [
                InteractionEffectStateV1::Intended,
                InteractionEffectStateV1::Indeterminate,
                InteractionEffectStateV1::Observing,
                InteractionEffectStateV1::ObservationPending,
            ] {
                assert!(matches!(
                    decide_interaction_effect_recovery_v1(
                        &definition,
                        state,
                        progress,
                        policy,
                        u64::from(definition.action().action_index().get()),
                    ),
                    InteractionEffectRecoveryDecisionV1::ObserveEffect(_)
                ));
                assert_eq!(
                    validate_interaction_effect_transition_v1(
                        &definition,
                        state,
                        InteractionEffectTransitionV1::RecordIntent,
                    ),
                    Err(InteractionEffectTransitionErrorV1::InvalidTransition)
                );
            }
            if definition.compensation_class()
                == InteractionEffectCompensationClassV1::NotCompensable
            {
                assert_eq!(
                    decide_interaction_effect_recovery_v1(
                        &definition,
                        InteractionEffectStateV1::KnownSucceeded,
                        progress,
                        policy,
                        8,
                    ),
                    InteractionEffectRecoveryDecisionV1::RecoveryRequired(
                        InteractionEffectRecoveryRequiredReasonV1::NonCompensableSuccess,
                    )
                );
            } else {
                assert!(matches!(
                    decide_interaction_effect_recovery_v1(
                        &definition,
                        InteractionEffectStateV1::KnownSucceeded,
                        progress,
                        policy,
                        8,
                    ),
                    InteractionEffectRecoveryDecisionV1::BeginCompensation(_)
                ));
                for state in [
                    InteractionEffectStateV1::CompensationIntended,
                    InteractionEffectStateV1::CompensationIndeterminate,
                    InteractionEffectStateV1::CompensationObserving,
                    InteractionEffectStateV1::CompensationObservationPending,
                ] {
                    assert!(matches!(
                        decide_interaction_effect_recovery_v1(
                            &definition,
                            state,
                            progress,
                            policy,
                            8,
                        ),
                        InteractionEffectRecoveryDecisionV1::ObserveCompensation(_)
                    ));
                }
            }
        }
    }

    #[test]
    fn compensation_order_is_reverse_action_order() {
        let first = role_definition(0);
        let second = role_definition(1);
        let order = build_interaction_effect_compensation_order_v1(&[
            (&first, InteractionEffectStateV1::KnownSucceeded),
            (&second, InteractionEffectStateV1::ReconciledSucceeded),
        ])
        .unwrap();
        assert_eq!(order[0].get(), 1);
        assert_eq!(order[1].get(), 0);
    }
}
