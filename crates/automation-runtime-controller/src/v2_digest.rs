use std::fmt::{Display, Formatter};

use sha2::{Digest, Sha256};

use crate::RuntimeControllerDtoError;

const CERTIFICATION_INTENT_DOMAIN_V2: &[u8] = b"starring.runtime.certification_intent.v2\0";
const CERTIFICATION_REQUEST_DOMAIN_V2: &[u8] = b"starring.runtime.certification_request.v2\0";
const LIVE_ATTESTATION_DOMAIN_V2: &[u8] = b"starring.runtime.live_attestation.v2\0";
const PRODUCT_MUTATION_DOMAIN_V2: &[u8] = b"starring.runtime.product_mutation.v2\0";
const DRAIN_INTENT_DOMAIN_V2: &[u8] = b"starring.runtime.drain_intent.v2\0";
const SUSPEND_ATTEMPT_DOMAIN_V2: &[u8] = b"starring.runtime.suspend_attempt.v2\0";

macro_rules! define_runtime_digest_v2 {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self, RuntimeControllerDtoError> {
                let value = value.into();
                if value.len() != 64
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                {
                    return Err(RuntimeControllerDtoError::InvalidDigest);
                }
                Ok(Self(value))
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
    };
}

macro_rules! impl_computed_runtime_digest_v2 {
    ($name:ident) => {
        impl $name {
            fn from_sha256(bytes: [u8; 32]) -> Self {
                Self(lower_hex(bytes))
            }
        }
    };
}

define_runtime_digest_v2!(RuntimeCertificationIntentFingerprintV2);
define_runtime_digest_v2!(RuntimeCertificationRequestDigestV2);
define_runtime_digest_v2!(RuntimeLiveAttestationDigestV2);
define_runtime_digest_v2!(RuntimeProductSemanticRequestDigestV2);
define_runtime_digest_v2!(RuntimeProductMutationDigestV2);
define_runtime_digest_v2!(RuntimeDrainIntentDigestV2);
define_runtime_digest_v2!(RuntimeSuspendAttemptDigestV2);

impl_computed_runtime_digest_v2!(RuntimeCertificationIntentFingerprintV2);
impl_computed_runtime_digest_v2!(RuntimeCertificationRequestDigestV2);
impl_computed_runtime_digest_v2!(RuntimeLiveAttestationDigestV2);
impl_computed_runtime_digest_v2!(RuntimeProductMutationDigestV2);
impl_computed_runtime_digest_v2!(RuntimeDrainIntentDigestV2);
impl_computed_runtime_digest_v2!(RuntimeSuspendAttemptDigestV2);

pub(crate) fn certification_intent_fingerprint_v2(
    payload: &[u8],
) -> RuntimeCertificationIntentFingerprintV2 {
    RuntimeCertificationIntentFingerprintV2::from_sha256(framed_sha256(
        CERTIFICATION_INTENT_DOMAIN_V2,
        payload,
    ))
}

pub(crate) fn certification_request_digest_v2(
    payload: &[u8],
) -> RuntimeCertificationRequestDigestV2 {
    RuntimeCertificationRequestDigestV2::from_sha256(framed_sha256(
        CERTIFICATION_REQUEST_DOMAIN_V2,
        payload,
    ))
}

pub(crate) fn live_attestation_digest_v2(payload: &[u8]) -> RuntimeLiveAttestationDigestV2 {
    RuntimeLiveAttestationDigestV2::from_sha256(framed_sha256(LIVE_ATTESTATION_DOMAIN_V2, payload))
}

pub(crate) fn product_mutation_digest_v2(payload: &[u8]) -> RuntimeProductMutationDigestV2 {
    RuntimeProductMutationDigestV2::from_sha256(framed_sha256(PRODUCT_MUTATION_DOMAIN_V2, payload))
}

pub(crate) fn drain_intent_digest_v2(payload: &[u8]) -> RuntimeDrainIntentDigestV2 {
    RuntimeDrainIntentDigestV2::from_sha256(framed_sha256(DRAIN_INTENT_DOMAIN_V2, payload))
}

pub(crate) fn suspend_attempt_digest_v2(payload: &[u8]) -> RuntimeSuspendAttemptDigestV2 {
    RuntimeSuspendAttemptDigestV2::from_sha256(framed_sha256(SUSPEND_ATTEMPT_DOMAIN_V2, payload))
}

fn framed_sha256(domain: &[u8], payload: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(
        u64::try_from(domain.len())
            .expect("runtime digest domain length must fit u64")
            .to_be_bytes(),
    );
    digest.update(domain);
    digest.update(
        u64::try_from(payload.len())
            .expect("runtime digest payload length must fit u64")
            .to_be_bytes(),
    );
    digest.update(payload);
    digest.finalize().into()
}

fn lower_hex(bytes: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::{
        certification_intent_fingerprint_v2, certification_request_digest_v2,
        drain_intent_digest_v2, live_attestation_digest_v2, product_mutation_digest_v2,
        suspend_attempt_digest_v2, RuntimeCertificationIntentFingerprintV2,
        RuntimeCertificationRequestDigestV2, RuntimeDrainIntentDigestV2,
        RuntimeLiveAttestationDigestV2, RuntimeProductMutationDigestV2,
        RuntimeProductSemanticRequestDigestV2, RuntimeSuspendAttemptDigestV2,
    };
    use crate::RuntimeControllerDtoError;

    const VALID_DIGEST: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
    const PAYLOAD: &[u8] = br#"{"format_version":2}"#;

    #[test]
    fn digest_types_accept_only_canonical_lowercase_sha256_text() {
        assert_eq!(
            RuntimeCertificationIntentFingerprintV2::parse(VALID_DIGEST)
                .unwrap()
                .as_str(),
            VALID_DIGEST
        );
        assert_eq!(
            RuntimeCertificationRequestDigestV2::parse(VALID_DIGEST)
                .unwrap()
                .as_str(),
            VALID_DIGEST
        );
        assert_eq!(
            RuntimeLiveAttestationDigestV2::parse(VALID_DIGEST)
                .unwrap()
                .as_str(),
            VALID_DIGEST
        );
        assert_eq!(
            RuntimeProductSemanticRequestDigestV2::parse(VALID_DIGEST)
                .unwrap()
                .as_str(),
            VALID_DIGEST
        );
        assert_eq!(
            RuntimeProductMutationDigestV2::parse(VALID_DIGEST)
                .unwrap()
                .as_str(),
            VALID_DIGEST
        );
        assert_eq!(
            RuntimeDrainIntentDigestV2::parse(VALID_DIGEST)
                .unwrap()
                .as_str(),
            VALID_DIGEST
        );
        assert_eq!(
            RuntimeSuspendAttemptDigestV2::parse(VALID_DIGEST)
                .unwrap()
                .as_str(),
            VALID_DIGEST
        );

        for invalid in [
            "",
            "0",
            "00112233445566778899aabbccddeeff00112233445566778899aabbccddeef",
            "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff0",
            "00112233445566778899AABBCCDDEEFF00112233445566778899AABBCCDDEEFF",
            "00112233445566778899aabbccddeeff00112233445566778899aabbccddeefg",
        ] {
            assert!(RuntimeCertificationIntentFingerprintV2::parse(invalid).is_err());
            assert!(RuntimeCertificationRequestDigestV2::parse(invalid).is_err());
            assert!(RuntimeLiveAttestationDigestV2::parse(invalid).is_err());
            assert!(RuntimeProductSemanticRequestDigestV2::parse(invalid).is_err());
            assert!(RuntimeProductMutationDigestV2::parse(invalid).is_err());
            assert!(RuntimeDrainIntentDigestV2::parse(invalid).is_err());
            assert!(RuntimeSuspendAttemptDigestV2::parse(invalid).is_err());
        }

        assert_eq!(
            RuntimeProductSemanticRequestDigestV2::parse("not-a-digest").unwrap_err(),
            RuntimeControllerDtoError::InvalidDigest
        );
    }

    #[test]
    fn typed_domains_match_the_canonical_framing_goldens() {
        let digests = [
            certification_intent_fingerprint_v2(PAYLOAD).to_string(),
            certification_request_digest_v2(PAYLOAD).to_string(),
            live_attestation_digest_v2(PAYLOAD).to_string(),
            product_mutation_digest_v2(PAYLOAD).to_string(),
            drain_intent_digest_v2(PAYLOAD).to_string(),
            suspend_attempt_digest_v2(PAYLOAD).to_string(),
        ];

        assert_eq!(
            digests,
            [
                "2065f317b4f1ff6e4b66dfc47ea8d77db8e825984c00c3acc8dd24681cf40bd6",
                "d50aa91c84f365fa336357c307b8f2613c1be377cee6f5db82510ffc195c0a6d",
                "8216ef56961340a2f4220a43bded1079fed038af95261bbd46f91e4df8ecc759",
                "558cb8a7f9190dfc7a7784750bf4e0d053ed7c2bb6c36c6ba6b7fd80c39bff81",
                "08ae4fb2781f1d8f841912af5b0397468ba19fb2f41278933cce30f229943564",
                "4d36fe1ee130959adbf77dd0df4ae5c49b36b188a12bfeb25fa0325a63e72c85",
            ]
        );

        for (index, digest) in digests.iter().enumerate() {
            assert!(!digests[..index].contains(digest));
        }

        assert_ne!(
            digests[0],
            super::lower_hex(super::framed_sha256(
                b"starring.runtime.certification_intent.v2",
                PAYLOAD,
            ))
        );
        assert_ne!(
            digests[0],
            certification_intent_fingerprint_v2(br#"{"format_version":3}"#).to_string()
        );
    }
}
