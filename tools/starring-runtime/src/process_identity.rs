use std::fmt::{Debug, Formatter};

use automation_runtime_convergence::ProcessInstanceId;

use crate::identity_encoding::{
    encode_runtime_identity_lower_hex_v1, RUNTIME_IDENTITY_ENTROPY_BYTES,
};

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeProcessInstanceIdGenerationErrorV1 {
    #[error("runtime process instance entropy is unavailable")]
    EntropyUnavailable,
    #[error("generated runtime process instance identifier is invalid")]
    InvalidGeneratedValue,
}

impl RuntimeProcessInstanceIdGenerationErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::EntropyUnavailable => "runtime_process_instance_id_entropy_unavailable",
            Self::InvalidGeneratedValue => "runtime_process_instance_id_invalid_generated_value",
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        None
    }
}

impl Debug for RuntimeProcessInstanceIdGenerationErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessInstanceIdGenerationErrorV1(<redacted>)")
    }
}

pub(crate) fn generate_runtime_process_instance_id_v1(
) -> Result<ProcessInstanceId, RuntimeProcessInstanceIdGenerationErrorV1> {
    generate_runtime_process_instance_id_with_v1(|bytes| {
        getrandom::fill(bytes)
            .map_err(|_| RuntimeProcessInstanceIdGenerationErrorV1::EntropyUnavailable)
    })
}

fn generate_runtime_process_instance_id_with_v1<F>(
    fill: F,
) -> Result<ProcessInstanceId, RuntimeProcessInstanceIdGenerationErrorV1>
where
    F: FnOnce(
        &mut [u8; RUNTIME_IDENTITY_ENTROPY_BYTES],
    ) -> Result<(), RuntimeProcessInstanceIdGenerationErrorV1>,
{
    let mut bytes = [0_u8; RUNTIME_IDENTITY_ENTROPY_BYTES];
    fill(&mut bytes)?;
    ProcessInstanceId::parse(encode_runtime_identity_lower_hex_v1(bytes))
        .map_err(|_| RuntimeProcessInstanceIdGenerationErrorV1::InvalidGeneratedValue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_entropy_roundtrips_as_an_exact_process_instance_id() {
        let identity = generate_runtime_process_instance_id_with_v1(|destination| {
            destination.copy_from_slice(&[
                0x00, 0x01, 0x0a, 0x0f, 0x10, 0x7f, 0x80, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98,
                0x76, 0x54,
            ]);
            Ok(())
        })
        .unwrap();

        assert_eq!(identity.as_str(), "00010a0f107f80abcdeffedcba987654");
    }

    #[test]
    fn generator_requests_one_exact_entropy_block() {
        let mut calls = 0;
        let identity = generate_runtime_process_instance_id_with_v1(|destination| {
            calls += 1;
            assert_eq!(destination.len(), RUNTIME_IDENTITY_ENTROPY_BYTES);
            destination.fill(0x5a);
            Ok(())
        })
        .unwrap();

        assert_eq!(calls, 1);
        assert_eq!(identity.as_str(), "5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a");
    }

    #[test]
    fn entropy_failure_has_no_retry_or_fallback() {
        let mut calls = 0;
        let result = generate_runtime_process_instance_id_with_v1(|destination| {
            calls += 1;
            assert_eq!(destination, &[0_u8; RUNTIME_IDENTITY_ENTROPY_BYTES]);
            Err(RuntimeProcessInstanceIdGenerationErrorV1::EntropyUnavailable)
        });

        assert_eq!(calls, 1);
        assert!(matches!(
            result,
            Err(RuntimeProcessInstanceIdGenerationErrorV1::EntropyUnavailable)
        ));
    }

    #[test]
    fn public_errors_have_finite_codes_and_redacted_debug() {
        let entropy = RuntimeProcessInstanceIdGenerationErrorV1::EntropyUnavailable;
        let invalid = RuntimeProcessInstanceIdGenerationErrorV1::InvalidGeneratedValue;

        assert_eq!(
            entropy.code(),
            "runtime_process_instance_id_entropy_unavailable"
        );
        assert_eq!(
            invalid.code(),
            "runtime_process_instance_id_invalid_generated_value"
        );
        assert_eq!(entropy.context(), None);
        assert_eq!(invalid.context(), None);
        assert_eq!(
            entropy.to_string(),
            "runtime process instance entropy is unavailable"
        );
        assert_eq!(
            invalid.to_string(),
            "generated runtime process instance identifier is invalid"
        );
        assert!(std::error::Error::source(&entropy).is_none());
        assert!(std::error::Error::source(&invalid).is_none());
        assert_eq!(
            format!("{entropy:?}"),
            "RuntimeProcessInstanceIdGenerationErrorV1(<redacted>)"
        );
    }
}
