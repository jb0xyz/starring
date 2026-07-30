use std::sync::atomic::{AtomicU64, Ordering};

use crate::id::{InstanceId, InstanceIdError};

const SECURE_INSTANCE_ID_ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstanceIdGenerationError {
    Invalid(InstanceIdError),
    Entropy,
}

pub trait InstanceIdGenerator {
    fn generate(&self) -> Result<InstanceId, InstanceIdGenerationError>;
}

pub struct SequenceInstanceIdGenerator {
    prefix: String,
    next: AtomicU64,
}

impl SequenceInstanceIdGenerator {
    pub fn new(prefix: &str, start: u64) -> Self {
        Self {
            prefix: prefix.to_string(),
            next: AtomicU64::new(start),
        }
    }
}

impl InstanceIdGenerator for SequenceInstanceIdGenerator {
    fn generate(&self) -> Result<InstanceId, InstanceIdGenerationError> {
        let value = self.next.fetch_add(1, Ordering::SeqCst);
        InstanceId::parse(&format!("{}_{:03}", self.prefix, value))
            .map_err(InstanceIdGenerationError::Invalid)
    }
}

pub struct SecureRandomInstanceIdGenerator;

impl SecureRandomInstanceIdGenerator {
    pub const fn new() -> Self {
        Self
    }
}

impl Default for SecureRandomInstanceIdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl InstanceIdGenerator for SecureRandomInstanceIdGenerator {
    fn generate(&self) -> Result<InstanceId, InstanceIdGenerationError> {
        let mut entropy = [0u8; 15];
        getrandom::fill(&mut entropy).map_err(|_| InstanceIdGenerationError::Entropy)?;
        InstanceId::parse(&format!("i_{}", encode_secure_instance_id(entropy)))
            .map_err(InstanceIdGenerationError::Invalid)
    }
}

fn encode_secure_instance_id(entropy: [u8; 15]) -> String {
    let mut encoded = String::with_capacity(24);
    let mut buffer = 0u16;
    let mut buffered_bits = 0u8;
    for byte in entropy {
        buffer = (buffer << 8) | u16::from(byte);
        buffered_bits += 8;
        while buffered_bits >= 5 {
            buffered_bits -= 5;
            let index = usize::from((buffer >> buffered_bits) & 0x1f);
            encoded.push(char::from(SECURE_INSTANCE_ID_ALPHABET[index]));
            buffer &= (1u16 << buffered_bits).wrapping_sub(1);
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use crate::{InstanceId, InstanceIdError};

    use super::{
        encode_secure_instance_id, InstanceIdGenerationError, InstanceIdGenerator,
        SecureRandomInstanceIdGenerator, SequenceInstanceIdGenerator, SECURE_INSTANCE_ID_ALPHABET,
    };

    #[test]
    fn sequence_generator_increments_from_start() {
        let generator = SequenceInstanceIdGenerator::new("room", 7);

        assert_eq!(generator.generate().unwrap().as_str(), "room_007");
        assert_eq!(generator.generate().unwrap().as_str(), "room_008");
    }

    #[test]
    fn sequence_generator_reports_invalid_id() {
        let generator = SequenceInstanceIdGenerator::new("invalid prefix", 1);

        assert_eq!(
            generator.generate(),
            Err(InstanceIdGenerationError::Invalid(
                InstanceIdError::InvalidChar
            ))
        );
    }

    #[test]
    fn secure_encoder_has_fixed_width_crockford_shape() {
        let zero = encode_secure_instance_id([0; 15]);
        let maximum = encode_secure_instance_id([u8::MAX; 15]);

        assert_eq!(zero, "000000000000000000000000");
        assert_eq!(maximum, "zzzzzzzzzzzzzzzzzzzzzzzz");
        assert_eq!(zero.len(), 24);
        assert!(maximum
            .bytes()
            .all(|byte| SECURE_INSTANCE_ID_ALPHABET.contains(&byte)));
    }

    #[test]
    fn secure_generator_produces_parseable_nonsequential_ids() {
        let generator = SecureRandomInstanceIdGenerator::new();
        let first = generator.generate().unwrap();
        let second = generator.generate().unwrap();

        assert!(first.as_str().starts_with("i_"));
        assert_eq!(first.as_str().len(), 26);
        assert_eq!(InstanceId::parse(first.as_str()).unwrap(), first);
        assert_ne!(first, second);
    }
}
