use std::fmt::{Debug, Formatter};

use automation_runtime_controller::RuntimeRecoveryIdV2;

use crate::identity_encoding::{
    encode_runtime_identity_lower_hex_v1, RUNTIME_IDENTITY_ENTROPY_BYTES,
};

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeRecoveryIdGenerationErrorV2 {
    #[error("runtime recovery identity entropy is unavailable")]
    EntropyUnavailable,
    #[error("generated runtime recovery identifier is invalid")]
    InvalidGeneratedValue,
}

impl RuntimeRecoveryIdGenerationErrorV2 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::EntropyUnavailable => "runtime_recovery_id_entropy_unavailable",
            Self::InvalidGeneratedValue => "runtime_recovery_id_invalid_generated_value",
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        None
    }
}

impl Debug for RuntimeRecoveryIdGenerationErrorV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRecoveryIdGenerationErrorV2(<redacted>)")
    }
}

pub(crate) fn generate_runtime_recovery_id_v2(
) -> Result<RuntimeRecoveryIdV2, RuntimeRecoveryIdGenerationErrorV2> {
    generate_runtime_recovery_id_with_v2(|bytes| {
        getrandom::fill(bytes).map_err(|_| RuntimeRecoveryIdGenerationErrorV2::EntropyUnavailable)
    })
}

fn generate_runtime_recovery_id_with_v2<F>(
    fill: F,
) -> Result<RuntimeRecoveryIdV2, RuntimeRecoveryIdGenerationErrorV2>
where
    F: FnOnce(
        &mut [u8; RUNTIME_IDENTITY_ENTROPY_BYTES],
    ) -> Result<(), RuntimeRecoveryIdGenerationErrorV2>,
{
    let mut bytes = [0_u8; RUNTIME_IDENTITY_ENTROPY_BYTES];
    fill(&mut bytes)?;
    RuntimeRecoveryIdV2::parse(encode_runtime_identity_lower_hex_v1(bytes))
        .map_err(|_| RuntimeRecoveryIdGenerationErrorV2::InvalidGeneratedValue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_entropy_roundtrips_as_an_exact_recovery_id() {
        let identity = generate_runtime_recovery_id_with_v2(|destination| {
            destination.copy_from_slice(&[
                0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
                0xcd, 0xef,
            ]);
            Ok(())
        })
        .unwrap();

        assert_eq!(identity.as_str(), "fedcba98765432100123456789abcdef");
    }

    #[test]
    fn generator_requests_one_exact_entropy_block() {
        let mut calls = 0;
        let identity = generate_runtime_recovery_id_with_v2(|destination| {
            calls += 1;
            assert_eq!(destination.len(), RUNTIME_IDENTITY_ENTROPY_BYTES);
            destination.fill(0x3c);
            Ok(())
        })
        .unwrap();

        assert_eq!(calls, 1);
        assert_eq!(identity.as_str(), "3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c");
    }

    #[test]
    fn entropy_failure_has_no_retry_or_fallback() {
        let mut calls = 0;
        let result = generate_runtime_recovery_id_with_v2(|destination| {
            calls += 1;
            assert_eq!(destination, &[0_u8; RUNTIME_IDENTITY_ENTROPY_BYTES]);
            Err(RuntimeRecoveryIdGenerationErrorV2::EntropyUnavailable)
        });

        assert_eq!(calls, 1);
        assert!(matches!(
            result,
            Err(RuntimeRecoveryIdGenerationErrorV2::EntropyUnavailable)
        ));
    }

    #[test]
    fn public_errors_have_finite_codes_and_redacted_diagnostics() {
        let entropy = RuntimeRecoveryIdGenerationErrorV2::EntropyUnavailable;
        let invalid = RuntimeRecoveryIdGenerationErrorV2::InvalidGeneratedValue;

        assert_eq!(entropy.code(), "runtime_recovery_id_entropy_unavailable");
        assert_eq!(
            invalid.code(),
            "runtime_recovery_id_invalid_generated_value"
        );
        assert_eq!(entropy.context(), None);
        assert_eq!(invalid.context(), None);
        assert!(!entropy.to_string().is_empty());
        assert!(!invalid.to_string().is_empty());
        assert!(std::error::Error::source(&entropy).is_none());
        assert!(std::error::Error::source(&invalid).is_none());
        assert_eq!(
            format!("{entropy:?}"),
            "RuntimeRecoveryIdGenerationErrorV2(<redacted>)"
        );
    }
}
