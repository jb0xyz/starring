use std::fmt::{Debug, Formatter};

use automation_runtime_convergence::ProcessInstanceId;

const PROCESS_INSTANCE_ENTROPY_BYTES: usize = 16;
const LOWER_HEX: &[u8; 16] = b"0123456789abcdef";

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
    mut fill: F,
) -> Result<ProcessInstanceId, RuntimeProcessInstanceIdGenerationErrorV1>
where
    F: FnMut(&mut [u8]) -> Result<(), RuntimeProcessInstanceIdGenerationErrorV1>,
{
    let mut bytes = [0_u8; PROCESS_INSTANCE_ENTROPY_BYTES];
    fill(&mut bytes)?;
    ProcessInstanceId::parse(encode_lower_hex_v1(bytes))
        .map_err(|_| RuntimeProcessInstanceIdGenerationErrorV1::InvalidGeneratedValue)
}

fn encode_lower_hex_v1(bytes: [u8; PROCESS_INSTANCE_ENTROPY_BYTES]) -> String {
    let mut encoded = String::with_capacity(PROCESS_INSTANCE_ENTROPY_BYTES * 2);
    for byte in bytes {
        encoded.push(LOWER_HEX[usize::from(byte >> 4)] as char);
        encoded.push(LOWER_HEX[usize::from(byte & 0x0f)] as char);
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_entropy_vectors_encode_exact_lower_hex() {
        for (bytes, expected) in [
            ([0_u8; 16], "00000000000000000000000000000000"),
            ([u8::MAX; 16], "ffffffffffffffffffffffffffffffff"),
            (
                [
                    0x00, 0x01, 0x0a, 0x0f, 0x10, 0x7f, 0x80, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba,
                    0x98, 0x76, 0x54,
                ],
                "00010a0f107f80abcdeffedcba987654",
            ),
        ] {
            let identity = generate_runtime_process_instance_id_with_v1(|destination| {
                destination.copy_from_slice(&bytes);
                Ok(())
            })
            .unwrap();

            assert_eq!(identity.as_str(), expected);
            assert_eq!(identity.as_str().len(), 32);
            assert!(identity
                .as_str()
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
        }
    }

    #[test]
    fn generator_requests_one_exact_entropy_block() {
        let mut calls = 0;
        let identity = generate_runtime_process_instance_id_with_v1(|destination| {
            calls += 1;
            assert_eq!(destination.len(), PROCESS_INSTANCE_ENTROPY_BYTES);
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
            assert_eq!(destination, &[0_u8; PROCESS_INSTANCE_ENTROPY_BYTES]);
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
