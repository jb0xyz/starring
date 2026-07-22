use std::fmt::{Display, Formatter};
use std::num::NonZeroU64;

use serde::{Deserialize, Serialize};

use crate::RuntimeControllerDtoError;

macro_rules! define_runtime_id_v2 {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self, RuntimeControllerDtoError> {
                let value = value.into();
                if value.len() != 32
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                {
                    return Err(RuntimeControllerDtoError::InvalidText);
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

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

define_runtime_id_v2!(RuntimeBarrierIdV1);
define_runtime_id_v2!(RuntimeCertificationOperationIdV2);
define_runtime_id_v2!(RuntimeRecoveryIdV2);
define_runtime_id_v2!(RuntimeDrainIntentIdV2);
define_runtime_id_v2!(RuntimeProductOperationIdV2);
define_runtime_id_v2!(RuntimeSuspensionIdV2);

impl RuntimeDrainIntentIdV2 {
    pub fn canonical_bytes(&self) -> [u8; 16] {
        let encoded = self.0.as_bytes();
        let mut decoded = [0_u8; 16];
        for (index, pair) in encoded.chunks_exact(2).enumerate() {
            decoded[index] =
                (decode_lower_hex_digit(pair[0]) << 4) | decode_lower_hex_digit(pair[1]);
        }
        decoded
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuntimeGatewayAdmissionSequenceV2(NonZeroU64);

impl RuntimeGatewayAdmissionSequenceV2 {
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0.get()
    }

    pub const fn into_non_zero(self) -> NonZeroU64 {
        self.0
    }
}

fn decode_lower_hex_digit(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use super::{
        RuntimeBarrierIdV1, RuntimeCertificationOperationIdV2, RuntimeDrainIntentIdV2,
        RuntimeGatewayAdmissionSequenceV2, RuntimeProductOperationIdV2, RuntimeRecoveryIdV2,
        RuntimeSuspensionIdV2,
    };

    const VALID_ID: &str = "00112233445566778899aabbccddeeff";

    #[test]
    fn runtime_ids_accept_the_canonical_lowercase_hex_form() {
        let barrier = RuntimeBarrierIdV1::parse(VALID_ID).unwrap();
        let certification = RuntimeCertificationOperationIdV2::parse(VALID_ID).unwrap();
        let recovery = RuntimeRecoveryIdV2::parse(VALID_ID).unwrap();
        let drain = RuntimeDrainIntentIdV2::parse(VALID_ID).unwrap();
        let product = RuntimeProductOperationIdV2::parse(VALID_ID).unwrap();
        let suspension = RuntimeSuspensionIdV2::parse(VALID_ID).unwrap();

        assert_eq!(barrier.as_str(), VALID_ID);
        assert_eq!(certification.as_str(), VALID_ID);
        assert_eq!(recovery.as_str(), VALID_ID);
        assert_eq!(drain.as_str(), VALID_ID);
        assert_eq!(product.as_str(), VALID_ID);
        assert_eq!(suspension.as_str(), VALID_ID);
        assert_eq!(barrier.to_string(), VALID_ID);
        assert_eq!(certification.to_string(), VALID_ID);
        assert_eq!(recovery.to_string(), VALID_ID);
        assert_eq!(drain.to_string(), VALID_ID);
        assert_eq!(product.to_string(), VALID_ID);
        assert_eq!(suspension.to_string(), VALID_ID);
    }

    #[test]
    fn runtime_ids_reject_invalid_lengths_alphabets_and_case() {
        for invalid in [
            "",
            "00112233445566778899aabbccddeef",
            "00112233445566778899aabbccddeeff0",
            "00112233445566778899aabbccddeefg",
            "00112233445566778899AABBCCDDEEFF",
            "00112233445566778899aabbccddee-1",
        ] {
            assert!(RuntimeBarrierIdV1::parse(invalid).is_err());
            assert!(RuntimeCertificationOperationIdV2::parse(invalid).is_err());
            assert!(RuntimeRecoveryIdV2::parse(invalid).is_err());
            assert!(RuntimeDrainIntentIdV2::parse(invalid).is_err());
            assert!(RuntimeProductOperationIdV2::parse(invalid).is_err());
            assert!(RuntimeSuspensionIdV2::parse(invalid).is_err());
        }
    }

    #[test]
    fn runtime_ids_round_trip_as_transparent_json_strings() {
        let barrier = RuntimeBarrierIdV1::parse(VALID_ID).unwrap();
        let certification = RuntimeCertificationOperationIdV2::parse(VALID_ID).unwrap();
        let recovery = RuntimeRecoveryIdV2::parse(VALID_ID).unwrap();
        let drain = RuntimeDrainIntentIdV2::parse(VALID_ID).unwrap();
        let product = RuntimeProductOperationIdV2::parse(VALID_ID).unwrap();
        let suspension = RuntimeSuspensionIdV2::parse(VALID_ID).unwrap();

        let barrier_json = serde_json::to_string(&barrier).unwrap();
        let certification_json = serde_json::to_string(&certification).unwrap();
        let recovery_json = serde_json::to_string(&recovery).unwrap();
        let drain_json = serde_json::to_string(&drain).unwrap();
        let product_json = serde_json::to_string(&product).unwrap();
        let suspension_json = serde_json::to_string(&suspension).unwrap();

        assert_eq!(barrier_json, format!("\"{VALID_ID}\""));
        assert_eq!(certification_json, format!("\"{VALID_ID}\""));
        assert_eq!(recovery_json, format!("\"{VALID_ID}\""));
        assert_eq!(drain_json, format!("\"{VALID_ID}\""));
        assert_eq!(product_json, format!("\"{VALID_ID}\""));
        assert_eq!(suspension_json, format!("\"{VALID_ID}\""));
        assert_eq!(
            serde_json::from_str::<RuntimeBarrierIdV1>(&barrier_json).unwrap(),
            barrier
        );
        assert_eq!(
            serde_json::from_str::<RuntimeCertificationOperationIdV2>(&certification_json).unwrap(),
            certification
        );
        assert_eq!(
            serde_json::from_str::<RuntimeRecoveryIdV2>(&recovery_json).unwrap(),
            recovery
        );
        assert_eq!(
            serde_json::from_str::<RuntimeDrainIntentIdV2>(&drain_json).unwrap(),
            drain
        );
        assert_eq!(
            serde_json::from_str::<RuntimeProductOperationIdV2>(&product_json).unwrap(),
            product
        );
        assert_eq!(
            serde_json::from_str::<RuntimeSuspensionIdV2>(&suspension_json).unwrap(),
            suspension
        );
    }

    #[test]
    fn runtime_id_json_rejects_noncanonical_strings_and_non_strings() {
        for invalid in [
            "\"00112233445566778899AABBCCDDEEFF\"",
            "\"00112233445566778899aabbccddeef\"",
            "\"00112233445566778899aabbccddeefg\"",
            "null",
            "42",
            "{}",
        ] {
            assert!(serde_json::from_str::<RuntimeBarrierIdV1>(invalid).is_err());
            assert!(serde_json::from_str::<RuntimeCertificationOperationIdV2>(invalid).is_err());
            assert!(serde_json::from_str::<RuntimeRecoveryIdV2>(invalid).is_err());
            assert!(serde_json::from_str::<RuntimeDrainIntentIdV2>(invalid).is_err());
            assert!(serde_json::from_str::<RuntimeProductOperationIdV2>(invalid).is_err());
            assert!(serde_json::from_str::<RuntimeSuspensionIdV2>(invalid).is_err());
        }
    }

    #[test]
    fn drain_intent_id_decodes_to_the_canonical_128_bits() {
        let drain = RuntimeDrainIntentIdV2::parse(VALID_ID).unwrap();

        assert_eq!(
            drain.canonical_bytes(),
            [
                0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
                0xee, 0xff,
            ]
        );
    }

    #[test]
    fn gateway_admission_sequence_has_checked_construction_and_access() {
        let value = NonZeroU64::new(9).unwrap();
        let sequence = RuntimeGatewayAdmissionSequenceV2::new(value);

        assert_eq!(sequence.get(), 9);
        assert_eq!(sequence.into_non_zero(), value);
    }

    #[test]
    fn gateway_admission_sequence_round_trips_as_a_transparent_json_number() {
        let sequence = RuntimeGatewayAdmissionSequenceV2::new(NonZeroU64::new(17).unwrap());
        let encoded = serde_json::to_string(&sequence).unwrap();

        assert_eq!(encoded, "17");
        assert_eq!(
            serde_json::from_str::<RuntimeGatewayAdmissionSequenceV2>(&encoded).unwrap(),
            sequence
        );
    }

    #[test]
    fn gateway_admission_sequence_json_rejects_zero_and_non_integer_values() {
        for invalid in ["0", "-1", "1.5", "\"1\"", "null"] {
            assert!(serde_json::from_str::<RuntimeGatewayAdmissionSequenceV2>(invalid).is_err());
        }
    }
}
