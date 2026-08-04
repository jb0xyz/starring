use std::fmt::{Debug, Formatter};

use automation_runtime_controller::{RuntimeBarrierIdV1, RuntimeCertificationOperationIdV2};

use crate::identity_encoding::{
    encode_runtime_identity_lower_hex_v1, RUNTIME_IDENTITY_ENTROPY_BYTES,
};

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeCertificationOperationIdGenerationErrorV2 {
    #[error("runtime certification operation identity entropy is unavailable")]
    EntropyUnavailable,
    #[error("generated runtime certification operation identifier is invalid")]
    InvalidGeneratedValue,
}

impl RuntimeCertificationOperationIdGenerationErrorV2 {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::EntropyUnavailable => "runtime_certification_operation_id_entropy_unavailable",
            Self::InvalidGeneratedValue => {
                "runtime_certification_operation_id_invalid_generated_value"
            }
        }
    }

    pub(crate) const fn context(self) -> Option<&'static str> {
        None
    }
}

impl Debug for RuntimeCertificationOperationIdGenerationErrorV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCertificationOperationIdGenerationErrorV2(<redacted>)")
    }
}

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeBarrierIdGenerationErrorV1 {
    #[error("runtime barrier identity entropy is unavailable")]
    EntropyUnavailable,
    #[error("generated runtime barrier identifier is invalid")]
    InvalidGeneratedValue,
}

impl RuntimeBarrierIdGenerationErrorV1 {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::EntropyUnavailable => "runtime_barrier_id_entropy_unavailable",
            Self::InvalidGeneratedValue => "runtime_barrier_id_invalid_generated_value",
        }
    }

    pub(crate) const fn context(self) -> Option<&'static str> {
        None
    }
}

impl Debug for RuntimeBarrierIdGenerationErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeBarrierIdGenerationErrorV1(<redacted>)")
    }
}

pub(crate) struct RuntimeCertificationOperationIdGenerationAuthorityV2 {
    _private: (),
}

impl RuntimeCertificationOperationIdGenerationAuthorityV2 {
    pub(crate) const fn new() -> Self {
        Self { _private: () }
    }

    #[allow(dead_code)]
    pub(crate) fn generate_v2(
        &mut self,
    ) -> Result<RuntimeCertificationOperationIdV2, RuntimeCertificationOperationIdGenerationErrorV2>
    {
        generate_runtime_certification_operation_id_with_v2(|bytes| {
            getrandom::fill(bytes)
                .map_err(|_| RuntimeCertificationOperationIdGenerationErrorV2::EntropyUnavailable)
        })
    }
}

impl Debug for RuntimeCertificationOperationIdGenerationAuthorityV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCertificationOperationIdGenerationAuthorityV2(<redacted>)")
    }
}

pub(crate) struct RuntimeBarrierIdGenerationAuthorityV1 {
    _private: (),
}

impl RuntimeBarrierIdGenerationAuthorityV1 {
    pub(crate) const fn new() -> Self {
        Self { _private: () }
    }

    #[allow(dead_code)]
    pub(crate) fn generate_v1(
        &mut self,
    ) -> Result<RuntimeBarrierIdV1, RuntimeBarrierIdGenerationErrorV1> {
        generate_runtime_barrier_id_with_v1(|bytes| {
            getrandom::fill(bytes)
                .map_err(|_| RuntimeBarrierIdGenerationErrorV1::EntropyUnavailable)
        })
    }
}

impl Debug for RuntimeBarrierIdGenerationAuthorityV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeBarrierIdGenerationAuthorityV1(<redacted>)")
    }
}

fn generate_runtime_certification_operation_id_with_v2<F>(
    fill: F,
) -> Result<RuntimeCertificationOperationIdV2, RuntimeCertificationOperationIdGenerationErrorV2>
where
    F: FnOnce(
        &mut [u8; RUNTIME_IDENTITY_ENTROPY_BYTES],
    ) -> Result<(), RuntimeCertificationOperationIdGenerationErrorV2>,
{
    let mut bytes = [0_u8; RUNTIME_IDENTITY_ENTROPY_BYTES];
    fill(&mut bytes)?;
    RuntimeCertificationOperationIdV2::parse(encode_runtime_identity_lower_hex_v1(bytes))
        .map_err(|_| RuntimeCertificationOperationIdGenerationErrorV2::InvalidGeneratedValue)
}

fn generate_runtime_barrier_id_with_v1<F>(
    fill: F,
) -> Result<RuntimeBarrierIdV1, RuntimeBarrierIdGenerationErrorV1>
where
    F: FnOnce(
        &mut [u8; RUNTIME_IDENTITY_ENTROPY_BYTES],
    ) -> Result<(), RuntimeBarrierIdGenerationErrorV1>,
{
    let mut bytes = [0_u8; RUNTIME_IDENTITY_ENTROPY_BYTES];
    fill(&mut bytes)?;
    RuntimeBarrierIdV1::parse(encode_runtime_identity_lower_hex_v1(bytes))
        .map_err(|_| RuntimeBarrierIdGenerationErrorV1::InvalidGeneratedValue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_entropy_maps_to_the_exact_certification_operation_id() {
        let identity = generate_runtime_certification_operation_id_with_v2(|destination| {
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
    fn certification_operation_generator_requests_one_exact_entropy_block() {
        let mut calls = 0;
        let identity = generate_runtime_certification_operation_id_with_v2(|destination| {
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
    fn certification_operation_entropy_failure_has_no_retry_or_fallback() {
        let mut calls = 0;
        let result = generate_runtime_certification_operation_id_with_v2(|destination| {
            calls += 1;
            assert_eq!(destination, &[0_u8; RUNTIME_IDENTITY_ENTROPY_BYTES]);
            Err(RuntimeCertificationOperationIdGenerationErrorV2::EntropyUnavailable)
        });

        assert_eq!(calls, 1);
        assert!(matches!(
            result,
            Err(RuntimeCertificationOperationIdGenerationErrorV2::EntropyUnavailable)
        ));
    }

    #[test]
    fn fixed_entropy_maps_to_the_exact_barrier_id() {
        let identity = generate_runtime_barrier_id_with_v1(|destination| {
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
    fn barrier_generator_requests_one_exact_entropy_block() {
        let mut calls = 0;
        let identity = generate_runtime_barrier_id_with_v1(|destination| {
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
    fn barrier_entropy_failure_has_no_retry_or_fallback() {
        let mut calls = 0;
        let result = generate_runtime_barrier_id_with_v1(|destination| {
            calls += 1;
            assert_eq!(destination, &[0_u8; RUNTIME_IDENTITY_ENTROPY_BYTES]);
            Err(RuntimeBarrierIdGenerationErrorV1::EntropyUnavailable)
        });

        assert_eq!(calls, 1);
        assert!(matches!(
            result,
            Err(RuntimeBarrierIdGenerationErrorV1::EntropyUnavailable)
        ));
    }

    #[test]
    fn generator_authorities_and_errors_have_redacted_diagnostics() {
        let operation_authority = RuntimeCertificationOperationIdGenerationAuthorityV2::new();
        let barrier_authority = RuntimeBarrierIdGenerationAuthorityV1::new();
        let operation_entropy =
            RuntimeCertificationOperationIdGenerationErrorV2::EntropyUnavailable;
        let operation_invalid =
            RuntimeCertificationOperationIdGenerationErrorV2::InvalidGeneratedValue;
        let barrier_entropy = RuntimeBarrierIdGenerationErrorV1::EntropyUnavailable;
        let barrier_invalid = RuntimeBarrierIdGenerationErrorV1::InvalidGeneratedValue;

        assert_eq!(
            format!("{operation_authority:?}"),
            "RuntimeCertificationOperationIdGenerationAuthorityV2(<redacted>)"
        );
        assert_eq!(
            format!("{barrier_authority:?}"),
            "RuntimeBarrierIdGenerationAuthorityV1(<redacted>)"
        );
        assert_eq!(
            operation_entropy.code(),
            "runtime_certification_operation_id_entropy_unavailable"
        );
        assert_eq!(
            operation_invalid.code(),
            "runtime_certification_operation_id_invalid_generated_value"
        );
        assert_eq!(
            barrier_entropy.code(),
            "runtime_barrier_id_entropy_unavailable"
        );
        assert_eq!(
            barrier_invalid.code(),
            "runtime_barrier_id_invalid_generated_value"
        );
        assert_eq!(operation_entropy.context(), None);
        assert_eq!(operation_invalid.context(), None);
        assert_eq!(barrier_entropy.context(), None);
        assert_eq!(barrier_invalid.context(), None);
        assert!(std::error::Error::source(&operation_entropy).is_none());
        assert!(std::error::Error::source(&operation_invalid).is_none());
        assert!(std::error::Error::source(&barrier_entropy).is_none());
        assert!(std::error::Error::source(&barrier_invalid).is_none());
        assert_eq!(
            format!("{operation_entropy:?}"),
            "RuntimeCertificationOperationIdGenerationErrorV2(<redacted>)"
        );
        assert_eq!(
            format!("{barrier_entropy:?}"),
            "RuntimeBarrierIdGenerationErrorV1(<redacted>)"
        );
    }
}
