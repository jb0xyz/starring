use std::fmt::{Debug, Formatter};

use automation_runtime_convergence::ControllerId;

use crate::identity_encoding::{
    encode_runtime_identity_lower_hex_v1, RUNTIME_IDENTITY_ENTROPY_BYTES,
};

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeControllerIdGenerationErrorV1 {
    #[error("runtime controller identity entropy is unavailable")]
    EntropyUnavailable,
    #[error("generated runtime controller identifier is invalid")]
    InvalidGeneratedValue,
}

impl RuntimeControllerIdGenerationErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::EntropyUnavailable => "runtime_controller_id_entropy_unavailable",
            Self::InvalidGeneratedValue => "runtime_controller_id_invalid_generated_value",
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        None
    }
}

impl Debug for RuntimeControllerIdGenerationErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeControllerIdGenerationErrorV1(<redacted>)")
    }
}

pub(crate) fn generate_runtime_controller_id_v1(
) -> Result<ControllerId, RuntimeControllerIdGenerationErrorV1> {
    generate_runtime_controller_id_with_v1(|bytes| {
        getrandom::fill(bytes).map_err(|_| RuntimeControllerIdGenerationErrorV1::EntropyUnavailable)
    })
}

fn generate_runtime_controller_id_with_v1<F>(
    fill: F,
) -> Result<ControllerId, RuntimeControllerIdGenerationErrorV1>
where
    F: FnOnce(
        &mut [u8; RUNTIME_IDENTITY_ENTROPY_BYTES],
    ) -> Result<(), RuntimeControllerIdGenerationErrorV1>,
{
    let mut bytes = [0_u8; RUNTIME_IDENTITY_ENTROPY_BYTES];
    fill(&mut bytes)?;
    ControllerId::parse(encode_runtime_identity_lower_hex_v1(bytes))
        .map_err(|_| RuntimeControllerIdGenerationErrorV1::InvalidGeneratedValue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_entropy_roundtrips_as_an_exact_controller_id() {
        let identity = generate_runtime_controller_id_with_v1(|destination| {
            destination.copy_from_slice(&[
                0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54,
                0x32, 0x10,
            ]);
            Ok(())
        })
        .unwrap();

        assert_eq!(identity.as_str(), "0123456789abcdeffedcba9876543210");
    }

    #[test]
    fn generator_requests_one_exact_entropy_block() {
        let mut calls = 0;
        let identity = generate_runtime_controller_id_with_v1(|destination| {
            calls += 1;
            assert_eq!(destination.len(), RUNTIME_IDENTITY_ENTROPY_BYTES);
            destination.fill(0xa5);
            Ok(())
        })
        .unwrap();

        assert_eq!(calls, 1);
        assert_eq!(identity.as_str(), "a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5");
    }

    #[test]
    fn entropy_failure_has_no_retry_or_fallback() {
        let mut calls = 0;
        let result = generate_runtime_controller_id_with_v1(|destination| {
            calls += 1;
            assert_eq!(destination, &[0_u8; RUNTIME_IDENTITY_ENTROPY_BYTES]);
            Err(RuntimeControllerIdGenerationErrorV1::EntropyUnavailable)
        });

        assert_eq!(calls, 1);
        assert!(matches!(
            result,
            Err(RuntimeControllerIdGenerationErrorV1::EntropyUnavailable)
        ));
    }

    #[test]
    fn public_errors_have_finite_codes_and_redacted_diagnostics() {
        let entropy = RuntimeControllerIdGenerationErrorV1::EntropyUnavailable;
        let invalid = RuntimeControllerIdGenerationErrorV1::InvalidGeneratedValue;

        assert_eq!(entropy.code(), "runtime_controller_id_entropy_unavailable");
        assert_eq!(
            invalid.code(),
            "runtime_controller_id_invalid_generated_value"
        );
        assert_eq!(entropy.context(), None);
        assert_eq!(invalid.context(), None);
        assert_eq!(
            entropy.to_string(),
            "runtime controller identity entropy is unavailable"
        );
        assert_eq!(
            invalid.to_string(),
            "generated runtime controller identifier is invalid"
        );
        assert!(std::error::Error::source(&entropy).is_none());
        assert!(std::error::Error::source(&invalid).is_none());
        assert_eq!(
            format!("{entropy:?}"),
            "RuntimeControllerIdGenerationErrorV1(<redacted>)"
        );
    }
}
