use authoring_application::ProductControlPortError;

const RUNTIME_ID_BYTES: usize = 16;
const RUNTIME_DRAIN_IDENTITY_BYTES: usize = RUNTIME_ID_BYTES * 2;

pub(super) struct RuntimeDrainCandidateIdsV2 {
    pub product_operation_id: String,
    pub drain_intent_id: String,
}

impl RuntimeDrainCandidateIdsV2 {
    pub(super) fn generate() -> Result<Self, ProductControlPortError> {
        let mut bytes = [0_u8; RUNTIME_DRAIN_IDENTITY_BYTES];
        getrandom::fill(&mut bytes).map_err(|_| identity_generation_unavailable())?;
        let product_operation_id = lower_hex(&bytes[..RUNTIME_ID_BYTES]);
        let drain_intent_id = lower_hex(&bytes[RUNTIME_ID_BYTES..]);
        if product_operation_id == drain_intent_id {
            return Err(identity_generation_unavailable());
        }
        Ok(Self {
            product_operation_id,
            drain_intent_id,
        })
    }
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn identity_generation_unavailable() -> ProductControlPortError {
    ProductControlPortError::Backend(
        "runtime Product drain identity generation is unavailable".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_identity_encoding_is_exact_lowercase_hex() {
        assert_eq!(
            lower_hex(&[
                0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
                0xee, 0xff
            ]),
            "00112233445566778899aabbccddeeff"
        );
    }

    #[test]
    fn generated_runtime_id_candidates_are_distinct_checked_shapes() {
        let candidates = RuntimeDrainCandidateIdsV2::generate().unwrap();
        assert_eq!(candidates.product_operation_id.len(), 32);
        assert_eq!(candidates.drain_intent_id.len(), 32);
        assert_ne!(candidates.product_operation_id, candidates.drain_intent_id);
        assert!(candidates
            .product_operation_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
        assert!(candidates
            .drain_intent_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    }
}
