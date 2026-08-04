use std::fmt::{Debug, Display, Formatter};
use std::num::NonZeroU64;

use sha2::{Digest, Sha256};

use crate::effect::{
    InteractionEffectAttemptOutcomeV1, InteractionEffectAttemptV1,
    InteractionEffectCompensationClassV1, InteractionEffectCompensationObservationOutcomeV1,
    InteractionEffectCompensationOutcomeV1, InteractionEffectCorrelationClassV1,
    InteractionEffectCorrelationV1, InteractionEffectDefinitionV1,
    InteractionEffectIdentityErrorV1, InteractionEffectIndeterminateClassV1,
    InteractionEffectInstanceStateV1, InteractionEffectKindV1,
    InteractionEffectKnownFailureClassV1, InteractionEffectKnownFailureV1,
    InteractionEffectObservationEvidenceV1, InteractionEffectObservationOutcomeV1,
    InteractionEffectObservedOutputV1, InteractionEffectOutputClassV1,
    InteractionEffectOverwriteTargetV1, InteractionEffectPermissionStateV1,
    InteractionEffectPreimageV1, InteractionEffectRecoveryBindingV1, InteractionEffectTargetV1,
};
use crate::effect_plan::{
    InteractionEffectPlanDefinitionV1, InteractionEffectPlannedChannelReferenceV1,
    InteractionEffectPlannedOverwriteTargetV1, InteractionEffectPlannedPermissionTargetV1,
    InteractionEffectPlannedPreimageV1, InteractionEffectPlannedRecoveryInputV1,
    InteractionEffectPlannedRoleMembershipTargetV1, InteractionEffectPlannedRoleReferenceV1,
    InteractionEffectPlannedTargetV1, InteractionEffectResolvedInputV1,
};

const CANONICAL_VERSION_V1: u16 = 1;
const EFFECT_IDENTITY_DOMAIN_V1: &[u8] = b"starring.runtime.interaction.effect.identity.v1\0";
const EFFECT_PLANNED_IDENTITY_DOMAIN_V1: &[u8] =
    b"starring.runtime.interaction.effect.planned_identity.v1\0";
const EFFECT_EXPECTED_POSTIMAGE_DOMAIN_V1: &[u8] =
    b"starring.runtime.interaction.effect.expected_postimage.v1\0";
const EFFECT_PLANNED_PREIMAGE_DOMAIN_V1: &[u8] =
    b"starring.runtime.interaction.effect.planned_preimage_digest.v1\0";
const EFFECT_PLANNED_RECOVERY_INPUT_DOMAIN_V1: &[u8] =
    b"starring.runtime.interaction.effect.planned_recovery_input.v1\0";
const EFFECT_RESOLVED_INPUT_DOMAIN_V1: &[u8] =
    b"starring.runtime.interaction.effect.resolved_input.v1\0";
const EFFECT_CORRELATION_DOMAIN_V1: &[u8] = b"starring.runtime.interaction.effect.correlation.v1\0";
const EFFECT_INTENT_DOMAIN_V1: &[u8] = b"starring.runtime.interaction.effect.intent.v1\0";
const EFFECT_RESULT_DOMAIN_V1: &[u8] = b"starring.runtime.interaction.effect.result.v1\0";
const EFFECT_OUTPUT_DOMAIN_V1: &[u8] = b"starring.runtime.interaction.effect.output.v1\0";
const EFFECT_PREIMAGE_DOMAIN_V1: &[u8] = b"starring.runtime.interaction.effect.preimage.v1\0";
const EFFECT_OBSERVATION_DOMAIN_V1: &[u8] = b"starring.runtime.interaction.effect.observation.v1\0";
const EFFECT_COMPENSATION_INTENT_DOMAIN_V1: &[u8] =
    b"starring.runtime.interaction.effect.compensation.intent.v1\0";
const EFFECT_COMPENSATION_RESULT_DOMAIN_V1: &[u8] =
    b"starring.runtime.interaction.effect.compensation.result.v1\0";
const EFFECT_COMPENSATION_OBSERVATION_DOMAIN_V1: &[u8] =
    b"starring.runtime.interaction.effect.compensation.observation.v1\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InteractionEffectDigestErrorV1 {
    #[error("interaction effect digest must contain exactly 64 characters")]
    Length,
    #[error("interaction effect digest must be lowercase hexadecimal")]
    LowerHex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InteractionEffectDigestBuildErrorV1 {
    #[error("interaction effect correlation does not match its identity")]
    CorrelationMismatch,
    #[error("interaction effect output does not match its identity")]
    OutputMismatch,
    #[error("interaction effect cannot be compensated")]
    NotCompensable,
    #[error("interaction effect compensation did not bind the exact preimage")]
    PreimageMismatch,
}

macro_rules! define_digest {
    ($name:ident) => {
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self, InteractionEffectDigestErrorV1> {
                let value = value.into();
                validate_digest_v1(&value)?;
                Ok(Self(value))
            }

            pub fn from_canonical_bytes(bytes: &[u8]) -> Self {
                Self(lower_hex_v1(&Sha256::digest(bytes)))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl Debug for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(concat!(stringify!($name), "(<redacted>)"))
            }
        }
    };
}

define_digest!(InteractionEffectActionDigestV1);
define_digest!(InteractionEffectInputDigestV1);
define_digest!(InteractionEffectOpaqueIdentityDigestV1);
define_digest!(InteractionEffectPayloadDigestV1);
define_digest!(InteractionEffectObservationEvidenceDigestV1);
define_digest!(InteractionEffectPlannedIdentityDigestV1);
define_digest!(InteractionEffectExpectedPostimageDigestV1);
define_digest!(InteractionEffectPlannedPreimageDigestV1);
define_digest!(InteractionEffectPlannedRecoveryInputDigestV1);
define_digest!(InteractionEffectResolvedInputDigestV1);
define_digest!(InteractionEffectIdentityDigestV1);
define_digest!(InteractionEffectOutputDigestV1);
define_digest!(InteractionEffectPreimageDigestV1);
define_digest!(InteractionEffectCorrelationDigestV1);
define_digest!(InteractionEffectIntentDigestV1);
define_digest!(InteractionEffectResultDigestV1);
define_digest!(InteractionEffectObservationDigestV1);
define_digest!(InteractionEffectCompensationIntentDigestV1);
define_digest!(InteractionEffectCompensationResultDigestV1);
define_digest!(InteractionEffectCompensationObservationDigestV1);

#[derive(Clone, Copy, Debug)]
pub enum InteractionEffectSuccessBindingV1<'a> {
    AttemptResult(&'a InteractionEffectResultDigestV1),
    Observation(&'a InteractionEffectObservationDigestV1),
}

pub fn build_interaction_effect_identity_digest_v1(
    definition: &InteractionEffectDefinitionV1,
) -> InteractionEffectIdentityDigestV1 {
    InteractionEffectIdentityDigestV1::from_canonical_bytes(&encode_effect_identity_v1(definition))
}

pub fn build_interaction_effect_planned_identity_digest_v1(
    definition: &InteractionEffectPlanDefinitionV1,
) -> InteractionEffectPlannedIdentityDigestV1 {
    InteractionEffectPlannedIdentityDigestV1::from_canonical_bytes(
        &encode_planned_effect_identity_v1(definition),
    )
}

pub fn build_interaction_effect_expected_postimage_digest_v1(
    definition: &InteractionEffectPlanDefinitionV1,
) -> InteractionEffectExpectedPostimageDigestV1 {
    let mut frame = CanonicalFrameV1::new(EFFECT_EXPECTED_POSTIMAGE_DOMAIN_V1);
    frame.digest(
        3,
        build_interaction_effect_planned_identity_digest_v1(definition).as_str(),
    );
    InteractionEffectExpectedPostimageDigestV1::from_canonical_bytes(&frame.finish())
}

pub fn build_interaction_effect_planned_preimage_digest_v1(
    preimage: &InteractionEffectPlannedPreimageV1,
) -> InteractionEffectPlannedPreimageDigestV1 {
    let mut frame = CanonicalFrameV1::new(EFFECT_PLANNED_PREIMAGE_DOMAIN_V1);
    frame.nested(3, encode_planned_preimage_v1(preimage));
    InteractionEffectPlannedPreimageDigestV1::from_canonical_bytes(&frame.finish())
}

pub fn build_interaction_effect_planned_recovery_input_digest_v1(
    input: &InteractionEffectPlannedRecoveryInputV1,
) -> InteractionEffectPlannedRecoveryInputDigestV1 {
    let mut frame = CanonicalFrameV1::new(EFFECT_PLANNED_RECOVERY_INPUT_DOMAIN_V1);
    frame.nested(3, encode_planned_target_v1(input.target()));
    frame.nested(4, encode_planned_preimage_v1(input.preimage()));
    InteractionEffectPlannedRecoveryInputDigestV1::from_canonical_bytes(&frame.finish())
}

pub fn build_interaction_effect_resolved_input_digest_v1(
    input: &InteractionEffectResolvedInputV1,
) -> InteractionEffectResolvedInputDigestV1 {
    let mut frame = CanonicalFrameV1::new(EFFECT_RESOLVED_INPUT_DOMAIN_V1);
    frame.nested(3, encode_target_v1(input.target()));
    let mut preimage =
        CanonicalFrameV1::new(b"starring.runtime.interaction.effect.resolved_preimage.v1\0");
    encode_preimage_v1(&mut preimage, input.preimage());
    frame.nested(4, preimage);
    InteractionEffectResolvedInputDigestV1::from_canonical_bytes(&frame.finish())
}

pub fn build_interaction_effect_output_digest_v1(
    output: &InteractionEffectObservedOutputV1,
) -> InteractionEffectOutputDigestV1 {
    let mut frame = CanonicalFrameV1::new(EFFECT_OUTPUT_DOMAIN_V1);
    encode_observed_output_v1(&mut frame, output);
    InteractionEffectOutputDigestV1::from_canonical_bytes(&frame.finish())
}

pub fn build_interaction_effect_preimage_digest_v1(
    preimage: &InteractionEffectPreimageV1,
) -> InteractionEffectPreimageDigestV1 {
    let mut frame = CanonicalFrameV1::new(EFFECT_PREIMAGE_DOMAIN_V1);
    encode_preimage_v1(&mut frame, preimage);
    InteractionEffectPreimageDigestV1::from_canonical_bytes(&frame.finish())
}

pub fn build_interaction_effect_correlation_v1(
    definition: &InteractionEffectDefinitionV1,
) -> InteractionEffectCorrelationV1 {
    build_interaction_effect_correlation_from_identity_v1(
        definition.planned_identity_digest().as_str(),
        definition.correlation_class(),
    )
}

pub fn build_interaction_effect_planned_correlation_v1(
    definition: &InteractionEffectPlanDefinitionV1,
) -> InteractionEffectCorrelationV1 {
    let identity = build_interaction_effect_planned_identity_digest_v1(definition);
    build_interaction_effect_correlation_from_identity_v1(
        identity.as_str(),
        definition.correlation_class(),
    )
}

pub fn build_interaction_effect_recovery_correlation_v1(
    planned_identity_digest: &InteractionEffectPlannedIdentityDigestV1,
    class: InteractionEffectCorrelationClassV1,
) -> InteractionEffectCorrelationV1 {
    build_interaction_effect_correlation_from_identity_v1(planned_identity_digest.as_str(), class)
}

fn build_interaction_effect_correlation_from_identity_v1(
    identity: &str,
    class: InteractionEffectCorrelationClassV1,
) -> InteractionEffectCorrelationV1 {
    let mut frame = CanonicalFrameV1::new(EFFECT_CORRELATION_DOMAIN_V1);
    frame.digest(3, identity);
    frame.u8(4, correlation_class_discriminant_v1(class));
    let canonical = frame.finish();
    let marker_digest = InteractionEffectCorrelationDigestV1::from_canonical_bytes(&canonical);
    let message_nonce = (class == InteractionEffectCorrelationClassV1::MessageNonce)
        .then(|| deterministic_nonzero_u64_v1(&canonical));
    InteractionEffectCorrelationV1::new(class, marker_digest, message_nonce)
}

pub fn build_interaction_effect_intent_digest_v1(
    definition: &InteractionEffectDefinitionV1,
    correlation: &InteractionEffectCorrelationV1,
) -> Result<InteractionEffectIntentDigestV1, InteractionEffectDigestBuildErrorV1> {
    if &build_interaction_effect_correlation_v1(definition) != correlation {
        return Err(InteractionEffectDigestBuildErrorV1::CorrelationMismatch);
    }
    let mut frame = CanonicalFrameV1::new(EFFECT_INTENT_DOMAIN_V1);
    frame.digest(
        3,
        build_interaction_effect_identity_digest_v1(definition).as_str(),
    );
    encode_correlation_v1(&mut frame, correlation);
    Ok(InteractionEffectIntentDigestV1::from_canonical_bytes(
        &frame.finish(),
    ))
}

pub fn build_interaction_effect_result_digest_v1(
    definition: &InteractionEffectDefinitionV1,
    intent_digest: &InteractionEffectIntentDigestV1,
    outcome: &InteractionEffectAttemptOutcomeV1,
) -> Result<InteractionEffectResultDigestV1, InteractionEffectDigestBuildErrorV1> {
    if let InteractionEffectAttemptOutcomeV1::KnownSucceeded(output) = outcome {
        definition
            .validate_observed_output(output)
            .map_err(|_| InteractionEffectDigestBuildErrorV1::OutputMismatch)?;
    }
    let mut frame = CanonicalFrameV1::new(EFFECT_RESULT_DOMAIN_V1);
    frame.digest(
        3,
        build_interaction_effect_identity_digest_v1(definition).as_str(),
    );
    frame.digest(4, intent_digest.as_str());
    encode_attempt_outcome_v1(&mut frame, outcome);
    Ok(InteractionEffectResultDigestV1::from_canonical_bytes(
        &frame.finish(),
    ))
}

pub fn build_interaction_effect_observation_digest_v1(
    definition: &InteractionEffectDefinitionV1,
    intent_digest: &InteractionEffectIntentDigestV1,
    attempt: InteractionEffectAttemptV1,
    outcome: &InteractionEffectObservationOutcomeV1,
) -> Result<InteractionEffectObservationDigestV1, InteractionEffectDigestBuildErrorV1> {
    if let InteractionEffectObservationOutcomeV1::ExactMatch { output, .. } = outcome {
        definition
            .validate_observed_output(output)
            .map_err(|_| InteractionEffectDigestBuildErrorV1::OutputMismatch)?;
    }
    let mut frame = CanonicalFrameV1::new(EFFECT_OBSERVATION_DOMAIN_V1);
    frame.digest(
        3,
        build_interaction_effect_identity_digest_v1(definition).as_str(),
    );
    frame.digest(4, intent_digest.as_str());
    frame.u16(5, attempt.get());
    encode_observation_outcome_v1(&mut frame, outcome);
    Ok(InteractionEffectObservationDigestV1::from_canonical_bytes(
        &frame.finish(),
    ))
}

pub fn build_interaction_effect_compensation_intent_digest_v1(
    definition: &InteractionEffectDefinitionV1,
    success: InteractionEffectSuccessBindingV1<'_>,
    successful_output: &InteractionEffectObservedOutputV1,
    attempt: InteractionEffectAttemptV1,
) -> Result<InteractionEffectCompensationIntentDigestV1, InteractionEffectDigestBuildErrorV1> {
    if definition.compensation_class() == InteractionEffectCompensationClassV1::NotCompensable {
        return Err(InteractionEffectDigestBuildErrorV1::NotCompensable);
    }
    definition
        .validate_observed_output(successful_output)
        .map_err(|_| InteractionEffectDigestBuildErrorV1::OutputMismatch)?;
    let mut frame = CanonicalFrameV1::new(EFFECT_COMPENSATION_INTENT_DOMAIN_V1);
    frame.digest(
        3,
        build_interaction_effect_identity_digest_v1(definition).as_str(),
    );
    match success {
        InteractionEffectSuccessBindingV1::AttemptResult(digest) => {
            frame.u8(4, 1);
            frame.digest(5, digest.as_str());
        }
        InteractionEffectSuccessBindingV1::Observation(digest) => {
            frame.u8(4, 2);
            frame.digest(5, digest.as_str());
        }
    }
    frame.digest(
        6,
        build_interaction_effect_output_digest_v1(successful_output).as_str(),
    );
    frame.u8(
        7,
        compensation_class_discriminant_v1(definition.compensation_class()),
    );
    frame.digest(
        8,
        build_interaction_effect_preimage_digest_v1(definition.preimage()).as_str(),
    );
    frame.u16(9, attempt.get());
    Ok(InteractionEffectCompensationIntentDigestV1::from_canonical_bytes(&frame.finish()))
}

pub fn build_interaction_effect_compensation_result_digest_v1(
    definition: &InteractionEffectDefinitionV1,
    intent_digest: &InteractionEffectCompensationIntentDigestV1,
    outcome: &InteractionEffectCompensationOutcomeV1,
) -> Result<InteractionEffectCompensationResultDigestV1, InteractionEffectDigestBuildErrorV1> {
    validate_compensation_preimage_v1(definition, outcome)?;
    let mut frame = CanonicalFrameV1::new(EFFECT_COMPENSATION_RESULT_DOMAIN_V1);
    frame.digest(
        3,
        build_interaction_effect_identity_digest_v1(definition).as_str(),
    );
    frame.digest(4, intent_digest.as_str());
    encode_compensation_outcome_v1(&mut frame, outcome);
    Ok(InteractionEffectCompensationResultDigestV1::from_canonical_bytes(&frame.finish()))
}

pub fn build_interaction_effect_compensation_observation_digest_v1(
    definition: &InteractionEffectDefinitionV1,
    intent_digest: &InteractionEffectCompensationIntentDigestV1,
    attempt: InteractionEffectAttemptV1,
    outcome: &InteractionEffectCompensationObservationOutcomeV1,
) -> Result<InteractionEffectCompensationObservationDigestV1, InteractionEffectDigestBuildErrorV1> {
    if let InteractionEffectCompensationObservationOutcomeV1::Restored {
        restored_preimage_digest,
        ..
    } = outcome
    {
        if *restored_preimage_digest
            != build_interaction_effect_preimage_digest_v1(definition.preimage())
        {
            return Err(InteractionEffectDigestBuildErrorV1::PreimageMismatch);
        }
    }
    let mut frame = CanonicalFrameV1::new(EFFECT_COMPENSATION_OBSERVATION_DOMAIN_V1);
    frame.digest(
        3,
        build_interaction_effect_identity_digest_v1(definition).as_str(),
    );
    frame.digest(4, intent_digest.as_str());
    frame.u16(5, attempt.get());
    encode_compensation_observation_outcome_v1(&mut frame, outcome);
    Ok(InteractionEffectCompensationObservationDigestV1::from_canonical_bytes(&frame.finish()))
}

pub fn build_interaction_effect_recovery_observation_digest_v1(
    binding: &InteractionEffectRecoveryBindingV1,
    intent_digest: &InteractionEffectIntentDigestV1,
    attempt: InteractionEffectAttemptV1,
    outcome: &InteractionEffectObservationOutcomeV1,
) -> Result<InteractionEffectObservationDigestV1, InteractionEffectDigestBuildErrorV1> {
    if let InteractionEffectObservationOutcomeV1::ExactMatch { output, .. } = outcome {
        binding
            .validate_observed_output(output)
            .map_err(|_| InteractionEffectDigestBuildErrorV1::OutputMismatch)?;
    }
    let mut frame = CanonicalFrameV1::new(EFFECT_OBSERVATION_DOMAIN_V1);
    frame.digest(3, binding.resolved_identity_digest().as_str());
    frame.digest(4, intent_digest.as_str());
    frame.u16(5, attempt.get());
    encode_observation_outcome_v1(&mut frame, outcome);
    Ok(InteractionEffectObservationDigestV1::from_canonical_bytes(
        &frame.finish(),
    ))
}

pub fn build_interaction_effect_recovery_compensation_intent_digest_v1(
    binding: &InteractionEffectRecoveryBindingV1,
    success: InteractionEffectSuccessBindingV1<'_>,
    successful_output: &InteractionEffectObservedOutputV1,
    attempt: InteractionEffectAttemptV1,
) -> Result<InteractionEffectCompensationIntentDigestV1, InteractionEffectDigestBuildErrorV1> {
    if binding.compensation_class() == InteractionEffectCompensationClassV1::NotCompensable {
        return Err(InteractionEffectDigestBuildErrorV1::NotCompensable);
    }
    binding
        .validate_observed_output(successful_output)
        .map_err(|_| InteractionEffectDigestBuildErrorV1::OutputMismatch)?;
    let mut frame = CanonicalFrameV1::new(EFFECT_COMPENSATION_INTENT_DOMAIN_V1);
    frame.digest(3, binding.resolved_identity_digest().as_str());
    match success {
        InteractionEffectSuccessBindingV1::AttemptResult(digest) => {
            frame.u8(4, 1);
            frame.digest(5, digest.as_str());
        }
        InteractionEffectSuccessBindingV1::Observation(digest) => {
            frame.u8(4, 2);
            frame.digest(5, digest.as_str());
        }
    }
    frame.digest(
        6,
        build_interaction_effect_output_digest_v1(successful_output).as_str(),
    );
    frame.u8(
        7,
        compensation_class_discriminant_v1(binding.compensation_class()),
    );
    frame.digest(8, binding.preimage_digest().as_str());
    frame.u16(9, attempt.get());
    Ok(InteractionEffectCompensationIntentDigestV1::from_canonical_bytes(&frame.finish()))
}

pub fn build_interaction_effect_recovery_compensation_result_digest_v1(
    binding: &InteractionEffectRecoveryBindingV1,
    intent_digest: &InteractionEffectCompensationIntentDigestV1,
    outcome: &InteractionEffectCompensationOutcomeV1,
) -> Result<InteractionEffectCompensationResultDigestV1, InteractionEffectDigestBuildErrorV1> {
    validate_recovery_compensation_preimage_v1(binding, outcome)?;
    let mut frame = CanonicalFrameV1::new(EFFECT_COMPENSATION_RESULT_DOMAIN_V1);
    frame.digest(3, binding.resolved_identity_digest().as_str());
    frame.digest(4, intent_digest.as_str());
    encode_compensation_outcome_v1(&mut frame, outcome);
    Ok(InteractionEffectCompensationResultDigestV1::from_canonical_bytes(&frame.finish()))
}

pub fn build_interaction_effect_recovery_compensation_observation_digest_v1(
    binding: &InteractionEffectRecoveryBindingV1,
    intent_digest: &InteractionEffectCompensationIntentDigestV1,
    attempt: InteractionEffectAttemptV1,
    outcome: &InteractionEffectCompensationObservationOutcomeV1,
) -> Result<InteractionEffectCompensationObservationDigestV1, InteractionEffectDigestBuildErrorV1> {
    if let InteractionEffectCompensationObservationOutcomeV1::Restored {
        restored_preimage_digest,
        ..
    } = outcome
    {
        if restored_preimage_digest != binding.preimage_digest() {
            return Err(InteractionEffectDigestBuildErrorV1::PreimageMismatch);
        }
    }
    let mut frame = CanonicalFrameV1::new(EFFECT_COMPENSATION_OBSERVATION_DOMAIN_V1);
    frame.digest(3, binding.resolved_identity_digest().as_str());
    frame.digest(4, intent_digest.as_str());
    frame.u16(5, attempt.get());
    encode_compensation_observation_outcome_v1(&mut frame, outcome);
    Ok(InteractionEffectCompensationObservationDigestV1::from_canonical_bytes(&frame.finish()))
}

fn encode_effect_identity_v1(definition: &InteractionEffectDefinitionV1) -> Vec<u8> {
    let action = definition.action();
    let mut frame = CanonicalFrameV1::new(EFFECT_IDENTITY_DOMAIN_V1);
    frame.u64(3, action.receipt_identity().application_id().get());
    frame.u64(4, action.receipt_identity().interaction_id().get());
    frame.digest(5, action.action_plan_digest().as_str());
    frame.digest(6, action.preflight_certificate_digest().as_str());
    frame.u16(7, action.action_index().get());
    frame.u8(8, effect_kind_discriminant_v1(action.kind()));
    frame.digest(9, action.action_digest().as_str());
    frame.digest(10, action.input_digest().as_str());
    frame.nested(11, encode_target_v1(definition.target()));
    frame.u8(12, output_class_discriminant_v1(definition.output_class()));
    frame.u8(
        13,
        correlation_class_discriminant_v1(definition.correlation_class()),
    );
    frame.u8(
        14,
        compensation_class_discriminant_v1(definition.compensation_class()),
    );
    frame.digest(
        15,
        build_interaction_effect_preimage_digest_v1(definition.preimage()).as_str(),
    );
    frame.u16(16, definition.dependencies().len() as u16);
    for dependency in definition.dependencies() {
        let mut nested =
            CanonicalFrameV1::new(b"starring.runtime.interaction.effect.dependency.v1\0");
        nested.u16(3, dependency.action_index().get());
        nested.digest(4, dependency.producer_identity_digest().as_str());
        nested.u8(5, output_class_discriminant_v1(dependency.output_class()));
        nested.digest(6, dependency.output_digest().as_str());
        frame.nested(17, nested);
    }
    frame.finish()
}

fn encode_planned_effect_identity_v1(definition: &InteractionEffectPlanDefinitionV1) -> Vec<u8> {
    let action = definition.action();
    let mut frame = CanonicalFrameV1::new(EFFECT_PLANNED_IDENTITY_DOMAIN_V1);
    frame.u64(3, action.receipt_identity().application_id().get());
    frame.u64(4, action.receipt_identity().interaction_id().get());
    frame.digest(5, action.action_plan_digest().as_str());
    frame.digest(6, action.preflight_certificate_digest().as_str());
    frame.u16(7, action.action_index().get());
    frame.u8(8, effect_kind_discriminant_v1(action.kind()));
    frame.digest(9, action.action_digest().as_str());
    frame.digest(10, action.input_digest().as_str());
    frame.nested(
        11,
        encode_planned_target_v1(definition.recovery_input().target()),
    );
    frame.u8(12, output_class_discriminant_v1(definition.output_class()));
    frame.u8(
        13,
        correlation_class_discriminant_v1(definition.correlation_class()),
    );
    frame.u8(
        14,
        compensation_class_discriminant_v1(definition.compensation_class()),
    );
    frame.nested(
        15,
        encode_planned_preimage_v1(definition.recovery_input().preimage()),
    );
    frame.u16(16, definition.dependencies().len() as u16);
    for dependency in definition.dependencies() {
        let mut nested =
            CanonicalFrameV1::new(b"starring.runtime.interaction.effect.planned_dependency.v1\0");
        nested.u16(3, dependency.action_index().get());
        nested.digest(4, dependency.producer_identity_digest().as_str());
        nested.u8(5, output_class_discriminant_v1(dependency.output_class()));
        frame.nested(17, nested);
    }
    frame.finish()
}

fn encode_planned_target_v1(target: &InteractionEffectPlannedTargetV1) -> CanonicalFrameV1 {
    let mut frame =
        CanonicalFrameV1::new(b"starring.runtime.interaction.effect.planned_target.v1\0");
    frame.u8(3, effect_kind_discriminant_v1(target.kind()));
    match target {
        InteractionEffectPlannedTargetV1::CreateRole { guild_id }
        | InteractionEffectPlannedTargetV1::CreateChannel { guild_id } => {
            frame.u64(4, guild_id.get());
        }
        InteractionEffectPlannedTargetV1::GrantRole { target } => {
            encode_planned_role_membership_target_v1(&mut frame, 4, target);
        }
        InteractionEffectPlannedTargetV1::UpsertOverwrite { target, desired } => {
            encode_planned_permission_target_v1(&mut frame, 4, target);
            frame.u64(10, desired.allow());
            frame.u64(11, desired.deny());
        }
        InteractionEffectPlannedTargetV1::PostPanel {
            guild_id,
            channel,
            payload_digest,
        } => {
            frame.u64(4, guild_id.get());
            encode_planned_channel_reference_v1(&mut frame, 5, channel);
            frame.digest(9, payload_digest.as_str());
        }
        InteractionEffectPlannedTargetV1::RegisterInstance {
            target,
            kind,
            manifest_digest,
        } => {
            encode_planned_instance_target_v1(&mut frame, 4, target);
            frame.digest(7, manifest_digest.as_str());
            frame.field(8, kind.0.as_bytes());
        }
        InteractionEffectPlannedTargetV1::TeardownInstance { target } => {
            encode_planned_instance_target_v1(&mut frame, 4, target);
        }
        InteractionEffectPlannedTargetV1::EditResponse {
            receipt_identity,
            payload_digest,
        } => {
            frame.u64(4, receipt_identity.application_id().get());
            frame.u64(5, receipt_identity.interaction_id().get());
            frame.digest(6, payload_digest.as_str());
        }
    }
    frame
}

fn encode_planned_preimage_v1(preimage: &InteractionEffectPlannedPreimageV1) -> CanonicalFrameV1 {
    let mut frame =
        CanonicalFrameV1::new(b"starring.runtime.interaction.effect.planned_preimage.v1\0");
    match preimage {
        InteractionEffectPlannedPreimageV1::None => frame.u8(3, 1),
        InteractionEffectPlannedPreimageV1::RoleMembership { target, present } => {
            frame.u8(3, 2);
            encode_planned_role_membership_target_v1(&mut frame, 4, target);
            frame.boolean(10, *present);
        }
        InteractionEffectPlannedPreimageV1::PermissionOverwrite { target, before } => {
            frame.u8(3, 3);
            encode_planned_permission_target_v1(&mut frame, 4, target);
            encode_permission_state_v1(&mut frame, 10, before);
        }
        InteractionEffectPlannedPreimageV1::InstanceRegistration { target, before } => {
            frame.u8(3, 4);
            encode_planned_instance_target_v1(&mut frame, 4, target);
            encode_instance_state_v1(&mut frame, 7, before);
        }
    }
    frame
}

fn encode_planned_role_membership_target_v1(
    frame: &mut CanonicalFrameV1,
    first_tag: u16,
    target: &InteractionEffectPlannedRoleMembershipTargetV1,
) {
    frame.u64(first_tag, target.guild_id().get());
    frame.u64(first_tag + 1, target.user_id().get());
    encode_planned_role_reference_v1(frame, first_tag + 2, target.role());
}

fn encode_planned_permission_target_v1(
    frame: &mut CanonicalFrameV1,
    first_tag: u16,
    target: &InteractionEffectPlannedPermissionTargetV1,
) {
    frame.u64(first_tag, target.guild_id().get());
    encode_planned_channel_reference_v1(frame, first_tag + 1, target.channel());
    match target.target() {
        InteractionEffectPlannedOverwriteTargetV1::Role(role) => {
            frame.u8(first_tag + 4, 1);
            encode_planned_role_reference_v1(frame, first_tag + 5, role);
        }
        InteractionEffectPlannedOverwriteTargetV1::Member(user_id) => {
            frame.u8(first_tag + 4, 2);
            frame.u64(first_tag + 5, user_id.get());
        }
    }
}

fn encode_planned_role_reference_v1(
    frame: &mut CanonicalFrameV1,
    first_tag: u16,
    reference: &InteractionEffectPlannedRoleReferenceV1,
) {
    match reference {
        InteractionEffectPlannedRoleReferenceV1::Existing(role_id) => {
            frame.u8(first_tag, 1);
            frame.u64(first_tag + 1, role_id.get());
        }
        InteractionEffectPlannedRoleReferenceV1::Produced(dependency) => {
            frame.u8(first_tag, 2);
            frame.u16(first_tag + 1, dependency.action_index().get());
            frame.digest(
                first_tag + 2,
                dependency.producer_identity_digest().as_str(),
            );
        }
    }
}

fn encode_planned_channel_reference_v1(
    frame: &mut CanonicalFrameV1,
    first_tag: u16,
    reference: &InteractionEffectPlannedChannelReferenceV1,
) {
    match reference {
        InteractionEffectPlannedChannelReferenceV1::Existing(channel_id) => {
            frame.u8(first_tag, 1);
            frame.u64(first_tag + 1, channel_id.get());
        }
        InteractionEffectPlannedChannelReferenceV1::Produced(dependency) => {
            frame.u8(first_tag, 2);
            frame.u16(first_tag + 1, dependency.action_index().get());
            frame.digest(
                first_tag + 2,
                dependency.producer_identity_digest().as_str(),
            );
        }
    }
}

fn encode_planned_instance_target_v1(
    frame: &mut CanonicalFrameV1,
    first_tag: u16,
    target: &crate::effect_plan::InteractionEffectPlannedInstanceTargetV1,
) {
    frame.u64(first_tag, target.guild_id().get());
    frame.field(first_tag + 1, target.instance_id().as_str().as_bytes());
    frame.digest(first_tag + 2, target.instance_identity_digest().as_str());
}

fn encode_target_v1(target: &InteractionEffectTargetV1) -> CanonicalFrameV1 {
    let mut frame = CanonicalFrameV1::new(b"starring.runtime.interaction.effect.target.v1\0");
    frame.u8(3, effect_kind_discriminant_v1(target.kind()));
    match target {
        InteractionEffectTargetV1::CreateRole { guild_id }
        | InteractionEffectTargetV1::CreateChannel { guild_id } => {
            frame.u64(4, guild_id.get());
        }
        InteractionEffectTargetV1::GrantRole { target } => {
            encode_role_membership_target_v1(&mut frame, 4, target);
        }
        InteractionEffectTargetV1::UpsertOverwrite { target, desired } => {
            encode_permission_target_v1(&mut frame, 4, target);
            frame.u64(8, desired.allow());
            frame.u64(9, desired.deny());
        }
        InteractionEffectTargetV1::PostPanel {
            guild_id,
            channel_id,
            payload_digest,
        } => {
            frame.u64(4, guild_id.get());
            frame.u64(5, channel_id.get());
            frame.digest(6, payload_digest.as_str());
        }
        InteractionEffectTargetV1::RegisterInstance {
            target,
            kind,
            manifest_digest,
        } => {
            encode_instance_target_v1(&mut frame, 4, target);
            frame.digest(6, manifest_digest.as_str());
            frame.field(7, kind.0.as_bytes());
        }
        InteractionEffectTargetV1::TeardownInstance { target } => {
            encode_instance_target_v1(&mut frame, 4, target);
        }
        InteractionEffectTargetV1::EditResponse {
            receipt_identity,
            payload_digest,
        } => {
            frame.u64(4, receipt_identity.application_id().get());
            frame.u64(5, receipt_identity.interaction_id().get());
            frame.digest(6, payload_digest.as_str());
        }
    }
    frame
}

fn encode_preimage_v1(frame: &mut CanonicalFrameV1, preimage: &InteractionEffectPreimageV1) {
    match preimage {
        InteractionEffectPreimageV1::None => frame.u8(3, 1),
        InteractionEffectPreimageV1::RoleMembership { target, present } => {
            frame.u8(3, 2);
            encode_role_membership_target_v1(frame, 4, target);
            frame.boolean(8, *present);
        }
        InteractionEffectPreimageV1::PermissionOverwrite { target, before } => {
            frame.u8(3, 3);
            encode_permission_target_v1(frame, 4, target);
            encode_permission_state_v1(frame, 8, before);
        }
        InteractionEffectPreimageV1::InstanceRegistration { target, before } => {
            frame.u8(3, 4);
            encode_instance_target_v1(frame, 4, target);
            encode_instance_state_v1(frame, 6, before);
        }
    }
}

fn encode_observed_output_v1(
    frame: &mut CanonicalFrameV1,
    output: &InteractionEffectObservedOutputV1,
) {
    frame.u8(3, output_class_discriminant_v1(output.class()));
    match output {
        InteractionEffectObservedOutputV1::CreatedRole { guild_id, role_id } => {
            frame.u64(4, guild_id.get());
            frame.u64(5, role_id.get());
        }
        InteractionEffectObservedOutputV1::CreatedChannel {
            guild_id,
            channel_id,
        } => {
            frame.u64(4, guild_id.get());
            frame.u64(5, channel_id.get());
        }
        InteractionEffectObservedOutputV1::RoleMembership { target, present } => {
            encode_role_membership_target_v1(frame, 4, target);
            frame.boolean(8, *present);
        }
        InteractionEffectObservedOutputV1::PermissionOverwrite { target, state } => {
            encode_permission_target_v1(frame, 4, target);
            encode_permission_state_v1(frame, 8, state);
        }
        InteractionEffectObservedOutputV1::PostedMessage {
            guild_id,
            channel_id,
            message_id,
            payload_digest,
        } => {
            frame.u64(4, guild_id.get());
            frame.u64(5, channel_id.get());
            frame.u64(6, message_id.get());
            frame.digest(7, payload_digest.as_str());
        }
        InteractionEffectObservedOutputV1::InstanceState { target, state } => {
            encode_instance_target_v1(frame, 4, target);
            encode_instance_state_v1(frame, 6, state);
        }
        InteractionEffectObservedOutputV1::OriginalResponse {
            receipt_identity,
            payload_digest,
        } => {
            frame.u64(4, receipt_identity.application_id().get());
            frame.u64(5, receipt_identity.interaction_id().get());
            frame.digest(6, payload_digest.as_str());
        }
    }
}

fn encode_attempt_outcome_v1(
    frame: &mut CanonicalFrameV1,
    outcome: &InteractionEffectAttemptOutcomeV1,
) {
    match outcome {
        InteractionEffectAttemptOutcomeV1::KnownSucceeded(output) => {
            frame.u8(20, 1);
            frame.digest(
                21,
                build_interaction_effect_output_digest_v1(output).as_str(),
            );
        }
        InteractionEffectAttemptOutcomeV1::KnownFailed(failure) => {
            frame.u8(20, 2);
            encode_known_failure_v1(frame, 21, *failure);
        }
        InteractionEffectAttemptOutcomeV1::Indeterminate(class) => {
            frame.u8(20, 3);
            frame.u8(21, indeterminate_class_discriminant_v1(*class));
        }
    }
}

fn encode_observation_outcome_v1(
    frame: &mut CanonicalFrameV1,
    outcome: &InteractionEffectObservationOutcomeV1,
) {
    match outcome {
        InteractionEffectObservationOutcomeV1::ExactMatch { output, evidence } => {
            frame.u8(20, 1);
            frame.digest(
                21,
                build_interaction_effect_output_digest_v1(output).as_str(),
            );
            encode_observation_evidence_v1(frame, 22, evidence);
        }
        InteractionEffectObservationOutcomeV1::ExactAbsence { evidence } => {
            frame.u8(20, 2);
            encode_observation_evidence_v1(frame, 21, evidence);
        }
        InteractionEffectObservationOutcomeV1::Pending { evidence } => {
            frame.u8(20, 3);
            encode_observation_evidence_v1(frame, 21, evidence);
        }
        InteractionEffectObservationOutcomeV1::Conflict { evidence } => {
            frame.u8(20, 4);
            encode_observation_evidence_v1(frame, 21, evidence);
        }
        InteractionEffectObservationOutcomeV1::Unsupported { evidence } => {
            frame.u8(20, 5);
            encode_observation_evidence_v1(frame, 21, evidence);
        }
    }
}

fn encode_compensation_outcome_v1(
    frame: &mut CanonicalFrameV1,
    outcome: &InteractionEffectCompensationOutcomeV1,
) {
    match outcome {
        InteractionEffectCompensationOutcomeV1::Succeeded {
            restored_preimage_digest,
        } => {
            frame.u8(20, 1);
            frame.digest(21, restored_preimage_digest.as_str());
        }
        InteractionEffectCompensationOutcomeV1::KnownFailed(failure) => {
            frame.u8(20, 2);
            encode_known_failure_v1(frame, 21, *failure);
        }
        InteractionEffectCompensationOutcomeV1::Indeterminate(class) => {
            frame.u8(20, 3);
            frame.u8(21, indeterminate_class_discriminant_v1(*class));
        }
    }
}

fn encode_compensation_observation_outcome_v1(
    frame: &mut CanonicalFrameV1,
    outcome: &InteractionEffectCompensationObservationOutcomeV1,
) {
    match outcome {
        InteractionEffectCompensationObservationOutcomeV1::Restored {
            restored_preimage_digest,
            evidence,
        } => {
            frame.u8(20, 1);
            frame.digest(21, restored_preimage_digest.as_str());
            encode_observation_evidence_v1(frame, 22, evidence);
        }
        InteractionEffectCompensationObservationOutcomeV1::Pending { evidence } => {
            frame.u8(20, 2);
            encode_observation_evidence_v1(frame, 21, evidence);
        }
        InteractionEffectCompensationObservationOutcomeV1::Conflict { evidence } => {
            frame.u8(20, 3);
            encode_observation_evidence_v1(frame, 21, evidence);
        }
        InteractionEffectCompensationObservationOutcomeV1::Unsupported { evidence } => {
            frame.u8(20, 4);
            encode_observation_evidence_v1(frame, 21, evidence);
        }
    }
}

fn encode_observation_evidence_v1(
    frame: &mut CanonicalFrameV1,
    first_tag: u16,
    evidence: &InteractionEffectObservationEvidenceV1,
) {
    frame.digest(first_tag, evidence.digest().as_str());
    frame.u8(
        first_tag + 1,
        correlation_class_discriminant_v1(evidence.correlation_class()),
    );
    frame.u16(first_tag + 2, evidence.exact_correlation_matches());
    frame.u16(first_tag + 3, evidence.conflicting_matches());
    frame.boolean(first_tag + 4, evidence.target_identity_matches());
    frame.boolean(first_tag + 5, evidence.actor_identity_matches());
    frame.boolean(first_tag + 6, evidence.postimage_matches());
}

fn encode_correlation_v1(
    frame: &mut CanonicalFrameV1,
    correlation: &InteractionEffectCorrelationV1,
) {
    frame.u8(20, correlation_class_discriminant_v1(correlation.class()));
    frame.digest(21, correlation.marker_digest().as_str());
    match correlation.message_nonce() {
        Some(nonce) => {
            frame.boolean(22, true);
            frame.u64(23, nonce.get());
        }
        None => frame.boolean(22, false),
    }
}

fn encode_role_membership_target_v1(
    frame: &mut CanonicalFrameV1,
    first_tag: u16,
    target: &crate::effect::InteractionEffectRoleMembershipTargetV1,
) {
    frame.u64(first_tag, target.guild_id().get());
    frame.u64(first_tag + 1, target.user_id().get());
    frame.u64(first_tag + 2, target.role_id().get());
}

fn encode_permission_target_v1(
    frame: &mut CanonicalFrameV1,
    first_tag: u16,
    target: &crate::effect::InteractionEffectPermissionTargetV1,
) {
    frame.u64(first_tag, target.guild_id().get());
    frame.u64(first_tag + 1, target.channel_id().get());
    match target.target() {
        InteractionEffectOverwriteTargetV1::Role(role_id) => {
            frame.u8(first_tag + 2, 1);
            frame.u64(first_tag + 3, role_id.get());
        }
        InteractionEffectOverwriteTargetV1::Member(user_id) => {
            frame.u8(first_tag + 2, 2);
            frame.u64(first_tag + 3, user_id.get());
        }
    }
}

fn encode_permission_state_v1(
    frame: &mut CanonicalFrameV1,
    first_tag: u16,
    state: &InteractionEffectPermissionStateV1,
) {
    match state {
        InteractionEffectPermissionStateV1::Absent => frame.u8(first_tag, 1),
        InteractionEffectPermissionStateV1::Present(value) => {
            frame.u8(first_tag, 2);
            frame.u64(first_tag + 1, value.allow());
            frame.u64(first_tag + 2, value.deny());
        }
    }
}

fn encode_instance_target_v1(
    frame: &mut CanonicalFrameV1,
    first_tag: u16,
    target: &crate::effect::InteractionEffectInstanceTargetV1,
) {
    frame.u64(first_tag, target.guild_id().get());
    frame.digest(first_tag + 1, target.instance_identity_digest().as_str());
}

fn encode_instance_state_v1(
    frame: &mut CanonicalFrameV1,
    first_tag: u16,
    state: &InteractionEffectInstanceStateV1,
) {
    match state {
        InteractionEffectInstanceStateV1::Absent => frame.u8(first_tag, 1),
        InteractionEffectInstanceStateV1::Present { manifest_digest } => {
            frame.u8(first_tag, 2);
            frame.digest(first_tag + 1, manifest_digest.as_str());
        }
    }
}

fn encode_known_failure_v1(
    frame: &mut CanonicalFrameV1,
    first_tag: u16,
    failure: InteractionEffectKnownFailureV1,
) {
    frame.u8(
        first_tag,
        known_failure_class_discriminant_v1(failure.class()),
    );
    match failure.http_status() {
        Some(status) => {
            frame.boolean(first_tag + 1, true);
            frame.u16(first_tag + 2, status);
        }
        None => frame.boolean(first_tag + 1, false),
    }
}

fn validate_compensation_preimage_v1(
    definition: &InteractionEffectDefinitionV1,
    outcome: &InteractionEffectCompensationOutcomeV1,
) -> Result<(), InteractionEffectDigestBuildErrorV1> {
    if let InteractionEffectCompensationOutcomeV1::Succeeded {
        restored_preimage_digest,
    } = outcome
    {
        if *restored_preimage_digest
            != build_interaction_effect_preimage_digest_v1(definition.preimage())
        {
            return Err(InteractionEffectDigestBuildErrorV1::PreimageMismatch);
        }
    }
    Ok(())
}

fn validate_recovery_compensation_preimage_v1(
    binding: &InteractionEffectRecoveryBindingV1,
    outcome: &InteractionEffectCompensationOutcomeV1,
) -> Result<(), InteractionEffectDigestBuildErrorV1> {
    if let InteractionEffectCompensationOutcomeV1::Succeeded {
        restored_preimage_digest,
    } = outcome
    {
        if restored_preimage_digest != binding.preimage_digest() {
            return Err(InteractionEffectDigestBuildErrorV1::PreimageMismatch);
        }
    }
    Ok(())
}

fn effect_kind_discriminant_v1(kind: InteractionEffectKindV1) -> u8 {
    match kind {
        InteractionEffectKindV1::CreateRole => 1,
        InteractionEffectKindV1::CreateChannel => 2,
        InteractionEffectKindV1::GrantRole => 3,
        InteractionEffectKindV1::UpsertOverwrite => 4,
        InteractionEffectKindV1::PostPanel => 5,
        InteractionEffectKindV1::RegisterInstance => 6,
        InteractionEffectKindV1::TeardownInstance => 7,
        InteractionEffectKindV1::EditResponse => 8,
    }
}

fn output_class_discriminant_v1(class: InteractionEffectOutputClassV1) -> u8 {
    match class {
        InteractionEffectOutputClassV1::CreatedRole => 1,
        InteractionEffectOutputClassV1::CreatedChannel => 2,
        InteractionEffectOutputClassV1::RoleMembership => 3,
        InteractionEffectOutputClassV1::PermissionOverwrite => 4,
        InteractionEffectOutputClassV1::PostedMessage => 5,
        InteractionEffectOutputClassV1::InstanceState => 6,
        InteractionEffectOutputClassV1::OriginalResponse => 7,
    }
}

fn correlation_class_discriminant_v1(class: InteractionEffectCorrelationClassV1) -> u8 {
    match class {
        InteractionEffectCorrelationClassV1::AuditLogReason => 1,
        InteractionEffectCorrelationClassV1::MessageNonce => 2,
        InteractionEffectCorrelationClassV1::InternalIdempotencyKey => 3,
        InteractionEffectCorrelationClassV1::InteractionReceipt => 4,
        InteractionEffectCorrelationClassV1::Unsupported => 5,
    }
}

fn compensation_class_discriminant_v1(class: InteractionEffectCompensationClassV1) -> u8 {
    match class {
        InteractionEffectCompensationClassV1::DeleteCreatedRole => 1,
        InteractionEffectCompensationClassV1::DeleteCreatedChannel => 2,
        InteractionEffectCompensationClassV1::RestoreRoleMembership => 3,
        InteractionEffectCompensationClassV1::RestorePermissionOverwrite => 4,
        InteractionEffectCompensationClassV1::DeletePostedMessage => 5,
        InteractionEffectCompensationClassV1::RestoreInstanceRegistration => 6,
        InteractionEffectCompensationClassV1::NotCompensable => 7,
    }
}

fn known_failure_class_discriminant_v1(class: InteractionEffectKnownFailureClassV1) -> u8 {
    match class {
        InteractionEffectKnownFailureClassV1::Rejected => 1,
        InteractionEffectKnownFailureClassV1::Forbidden => 2,
        InteractionEffectKnownFailureClassV1::NotFound => 3,
        InteractionEffectKnownFailureClassV1::RateLimitedBeforeDispatch => 4,
        InteractionEffectKnownFailureClassV1::Conflict => 5,
        InteractionEffectKnownFailureClassV1::InvalidRequest => 6,
    }
}

fn indeterminate_class_discriminant_v1(class: InteractionEffectIndeterminateClassV1) -> u8 {
    match class {
        InteractionEffectIndeterminateClassV1::DeadlineElapsed => 1,
        InteractionEffectIndeterminateClassV1::ConnectionLost => 2,
        InteractionEffectIndeterminateClassV1::Cancelled => 3,
        InteractionEffectIndeterminateClassV1::MalformedResponse => 4,
        InteractionEffectIndeterminateClassV1::PersistenceCommit => 5,
        InteractionEffectIndeterminateClassV1::ProviderUnavailable => 6,
        InteractionEffectIndeterminateClassV1::Unknown => 7,
    }
}

fn deterministic_nonzero_u64_v1(bytes: &[u8]) -> NonZeroU64 {
    let digest = Sha256::digest(bytes);
    let value = u64::from_be_bytes(
        digest[..8]
            .try_into()
            .expect("SHA-256 prefix has eight bytes"),
    );
    NonZeroU64::new(value).unwrap_or(NonZeroU64::MIN)
}

fn validate_digest_v1(value: &str) -> Result<(), InteractionEffectDigestErrorV1> {
    if value.len() != 64 {
        return Err(InteractionEffectDigestErrorV1::Length);
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(InteractionEffectDigestErrorV1::LowerHex);
    }
    Ok(())
}

fn lower_hex_v1(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").expect("writing hexadecimal to String cannot fail");
    }
    output
}

struct CanonicalFrameV1 {
    bytes: Vec<u8>,
}

impl CanonicalFrameV1 {
    fn new(domain: &[u8]) -> Self {
        let mut frame = Self {
            bytes: Vec::with_capacity(512),
        };
        frame.field(1, domain);
        frame.u16(2, CANONICAL_VERSION_V1);
        frame
    }

    fn field(&mut self, tag: u16, value: &[u8]) {
        self.bytes.extend_from_slice(&tag.to_be_bytes());
        self.bytes
            .extend_from_slice(&(value.len() as u64).to_be_bytes());
        self.bytes.extend_from_slice(value);
    }

    fn boolean(&mut self, tag: u16, value: bool) {
        self.u8(tag, u8::from(value));
    }

    fn u8(&mut self, tag: u16, value: u8) {
        self.field(tag, &[value]);
    }

    fn u16(&mut self, tag: u16, value: u16) {
        self.field(tag, &value.to_be_bytes());
    }

    fn u64(&mut self, tag: u16, value: u64) {
        self.field(tag, &value.to_be_bytes());
    }

    fn digest(&mut self, tag: u16, value: &str) {
        self.field(tag, value.as_bytes());
    }

    fn nested(&mut self, tag: u16, value: CanonicalFrameV1) {
        self.field(tag, &value.finish());
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

impl From<InteractionEffectIdentityErrorV1> for InteractionEffectDigestBuildErrorV1 {
    fn from(_: InteractionEffectIdentityErrorV1) -> Self {
        Self::OutputMismatch
    }
}

#[cfg(test)]
mod tests {
    use automation_instance::{InstanceId, InstanceKind};

    use super::*;
    use crate::effect::{
        InteractionEffectActionIdentityV1, InteractionEffectActionIndexV1,
        InteractionEffectCompensationObservationOutcomeV1, InteractionEffectCorrelationClassV1,
        InteractionEffectDefinitionV1, InteractionEffectGuildIdV1,
        InteractionEffectInstanceStateV1, InteractionEffectInstanceTargetV1,
        InteractionEffectKindV1, InteractionEffectObservationEvidenceV1,
        InteractionEffectObservationOutcomeV1, InteractionEffectObservedOutputV1,
        InteractionEffectPreimageV1, InteractionEffectRoleIdV1, InteractionEffectTargetV1,
    };
    use crate::{
        DiscordApplicationIdV1, DiscordInteractionIdV1, InteractionActionPlanDigestV1,
        InteractionPreflightCertificateDigestV1, InteractionReceiptIdentityV1,
    };

    fn hex(value: char) -> String {
        value.to_string().repeat(64)
    }

    fn definition(index: u16) -> InteractionEffectDefinitionV1 {
        let receipt = InteractionReceiptIdentityV1::new(
            DiscordApplicationIdV1::new(10).unwrap(),
            DiscordInteractionIdV1::new(20).unwrap(),
        );
        let action = InteractionEffectActionIdentityV1::new(
            receipt,
            InteractionActionPlanDigestV1::parse(hex('a')).unwrap(),
            InteractionPreflightCertificateDigestV1::parse(hex('b')).unwrap(),
            InteractionEffectActionIndexV1::new(index).unwrap(),
            InteractionEffectKindV1::CreateRole,
            InteractionEffectActionDigestV1::parse(hex('c')).unwrap(),
            InteractionEffectInputDigestV1::parse(hex('d')).unwrap(),
        );
        InteractionEffectDefinitionV1::new(
            action,
            InteractionEffectTargetV1::CreateRole {
                guild_id: InteractionEffectGuildIdV1::new(30).unwrap(),
            },
            InteractionEffectPreimageV1::None,
            Vec::new(),
        )
        .unwrap()
    }

    fn output() -> InteractionEffectObservedOutputV1 {
        InteractionEffectObservedOutputV1::CreatedRole {
            guild_id: InteractionEffectGuildIdV1::new(30).unwrap(),
            role_id: InteractionEffectRoleIdV1::new(40).unwrap(),
        }
    }

    fn registration_definition(kind: &str) -> InteractionEffectDefinitionV1 {
        let receipt = InteractionReceiptIdentityV1::new(
            DiscordApplicationIdV1::new(10).unwrap(),
            DiscordInteractionIdV1::new(20).unwrap(),
        );
        let action = InteractionEffectActionIdentityV1::new(
            receipt,
            InteractionActionPlanDigestV1::parse(hex('a')).unwrap(),
            InteractionPreflightCertificateDigestV1::parse(hex('b')).unwrap(),
            InteractionEffectActionIndexV1::new(0).unwrap(),
            InteractionEffectKindV1::RegisterInstance,
            InteractionEffectActionDigestV1::parse(hex('c')).unwrap(),
            InteractionEffectInputDigestV1::parse(hex('d')).unwrap(),
        );
        let target = InteractionEffectInstanceTargetV1::new(
            InteractionEffectGuildIdV1::new(30).unwrap(),
            InteractionEffectOpaqueIdentityDigestV1::parse(hex('e')).unwrap(),
        );
        InteractionEffectDefinitionV1::new(
            action,
            InteractionEffectTargetV1::RegisterInstance {
                target: target.clone(),
                kind: InstanceKind(kind.to_string()),
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

    fn registration_plan_definition(kind: &str) -> InteractionEffectPlanDefinitionV1 {
        let receipt = InteractionReceiptIdentityV1::new(
            DiscordApplicationIdV1::new(10).unwrap(),
            DiscordInteractionIdV1::new(20).unwrap(),
        );
        let action = InteractionEffectActionIdentityV1::new(
            receipt,
            InteractionActionPlanDigestV1::parse(hex('a')).unwrap(),
            InteractionPreflightCertificateDigestV1::parse(hex('b')).unwrap(),
            InteractionEffectActionIndexV1::new(0).unwrap(),
            InteractionEffectKindV1::RegisterInstance,
            InteractionEffectActionDigestV1::parse(hex('c')).unwrap(),
            InteractionEffectInputDigestV1::parse(hex('d')).unwrap(),
        );
        let target = crate::effect_plan::InteractionEffectPlannedInstanceTargetV1::new(
            InteractionEffectGuildIdV1::new(30).unwrap(),
            InstanceId::parse("room-1").unwrap(),
        );
        let recovery = InteractionEffectPlannedRecoveryInputV1::new(
            InteractionEffectPlannedTargetV1::RegisterInstance {
                target: target.clone(),
                kind: InstanceKind(kind.to_string()),
                manifest_digest: InteractionEffectPayloadDigestV1::parse(hex('f')).unwrap(),
            },
            InteractionEffectPlannedPreimageV1::InstanceRegistration {
                target,
                before: InteractionEffectInstanceStateV1::Absent,
            },
        )
        .unwrap();
        InteractionEffectPlanDefinitionV1::new(action, recovery, Vec::new()).unwrap()
    }

    fn evidence() -> InteractionEffectObservationEvidenceV1 {
        InteractionEffectObservationEvidenceV1::new(
            InteractionEffectObservationEvidenceDigestV1::from_canonical_bytes(b"evidence"),
            InteractionEffectCorrelationClassV1::AuditLogReason,
            1,
            0,
            true,
            true,
            true,
        )
    }

    #[test]
    fn identity_digest_is_deterministic_and_binds_action_index() {
        let first = build_interaction_effect_identity_digest_v1(&definition(0));
        assert_eq!(
            first,
            build_interaction_effect_identity_digest_v1(&definition(0))
        );
        assert_ne!(
            first,
            build_interaction_effect_identity_digest_v1(&definition(1))
        );
        assert_eq!(first.as_str().len(), 64);
        assert_eq!(
            format!("{first:?}"),
            "InteractionEffectIdentityDigestV1(<redacted>)"
        );
    }

    #[test]
    fn registration_identity_digest_binds_instance_kind() {
        assert_ne!(
            build_interaction_effect_identity_digest_v1(&registration_definition("study_room")),
            build_interaction_effect_identity_digest_v1(&registration_definition("game_room")),
        );
        assert_ne!(
            build_interaction_effect_planned_identity_digest_v1(&registration_plan_definition(
                "study_room",
            )),
            build_interaction_effect_planned_identity_digest_v1(&registration_plan_definition(
                "game_room",
            )),
        );
    }

    #[test]
    fn correlation_and_intent_are_derived_from_exact_identity() {
        let bound = definition(0);
        let correlation = build_interaction_effect_correlation_v1(&bound);
        assert_eq!(
            correlation.class(),
            InteractionEffectCorrelationClassV1::AuditLogReason
        );
        assert_eq!(correlation.message_nonce(), None);
        let intent = build_interaction_effect_intent_digest_v1(&bound, &correlation).unwrap();
        assert_eq!(
            intent,
            build_interaction_effect_intent_digest_v1(&bound, &correlation).unwrap()
        );
        let wrong = build_interaction_effect_correlation_v1(&definition(1));
        assert_eq!(
            build_interaction_effect_intent_digest_v1(&bound, &wrong),
            Err(InteractionEffectDigestBuildErrorV1::CorrelationMismatch)
        );
    }

    #[test]
    fn result_and_observation_bind_typed_output_and_evidence() {
        let definition = definition(0);
        let correlation = build_interaction_effect_correlation_v1(&definition);
        let intent = build_interaction_effect_intent_digest_v1(&definition, &correlation).unwrap();
        let outcome =
            InteractionEffectAttemptOutcomeV1::known_succeeded(&definition, output()).unwrap();
        let result =
            build_interaction_effect_result_digest_v1(&definition, &intent, &outcome).unwrap();
        let observation = InteractionEffectObservationOutcomeV1::ExactMatch {
            output: output(),
            evidence: evidence(),
        };
        let first = build_interaction_effect_observation_digest_v1(
            &definition,
            &intent,
            InteractionEffectAttemptV1::new(1).unwrap(),
            &observation,
        )
        .unwrap();
        let second = build_interaction_effect_observation_digest_v1(
            &definition,
            &intent,
            InteractionEffectAttemptV1::new(2).unwrap(),
            &observation,
        )
        .unwrap();
        assert_ne!(first, second);
        assert_ne!(result.as_str(), first.as_str());
    }

    #[test]
    fn compensation_digests_require_the_exact_preimage() {
        let definition = definition(0);
        let correlation = build_interaction_effect_correlation_v1(&definition);
        let intent = build_interaction_effect_intent_digest_v1(&definition, &correlation).unwrap();
        let outcome =
            InteractionEffectAttemptOutcomeV1::known_succeeded(&definition, output()).unwrap();
        let result =
            build_interaction_effect_result_digest_v1(&definition, &intent, &outcome).unwrap();
        let compensation = build_interaction_effect_compensation_intent_digest_v1(
            &definition,
            InteractionEffectSuccessBindingV1::AttemptResult(&result),
            &output(),
            InteractionEffectAttemptV1::new(1).unwrap(),
        )
        .unwrap();
        let expected = build_interaction_effect_preimage_digest_v1(definition.preimage());
        let success = InteractionEffectCompensationOutcomeV1::Succeeded {
            restored_preimage_digest: expected.clone(),
        };
        assert!(build_interaction_effect_compensation_result_digest_v1(
            &definition,
            &compensation,
            &success,
        )
        .is_ok());
        let wrong = InteractionEffectCompensationOutcomeV1::Succeeded {
            restored_preimage_digest: InteractionEffectPreimageDigestV1::from_canonical_bytes(
                b"wrong",
            ),
        };
        assert_eq!(
            build_interaction_effect_compensation_result_digest_v1(
                &definition,
                &compensation,
                &wrong,
            ),
            Err(InteractionEffectDigestBuildErrorV1::PreimageMismatch)
        );
        let restored = InteractionEffectCompensationObservationOutcomeV1::Restored {
            restored_preimage_digest: expected,
            evidence: evidence(),
        };
        assert!(build_interaction_effect_compensation_observation_digest_v1(
            &definition,
            &compensation,
            InteractionEffectAttemptV1::new(1).unwrap(),
            &restored,
        )
        .is_ok());
    }

    #[test]
    fn digest_parsing_rejects_noncanonical_values() {
        assert_eq!(
            InteractionEffectIdentityDigestV1::parse("a".repeat(63)),
            Err(InteractionEffectDigestErrorV1::Length)
        );
        assert_eq!(
            InteractionEffectIdentityDigestV1::parse("A".repeat(64)),
            Err(InteractionEffectDigestErrorV1::LowerHex)
        );
    }
}
