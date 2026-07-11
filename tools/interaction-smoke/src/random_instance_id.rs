use automation_instance::{InstanceId, InstanceIdGenerationError, InstanceIdGenerator};

const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

pub fn encode_instance_id(bytes: [u8; 8]) -> String {
    let value = u64::from_be_bytes(bytes);
    let mut out = String::with_capacity(12);
    for position in 0..12 {
        let shift = 5 * (11 - position);
        let index = ((value >> shift) & 0x1f) as usize;
        out.push(ALPHABET[index] as char);
    }
    out
}

pub struct RandomInstanceIdGenerator;

impl RandomInstanceIdGenerator {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RandomInstanceIdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl InstanceIdGenerator for RandomInstanceIdGenerator {
    fn generate(&self) -> Result<InstanceId, InstanceIdGenerationError> {
        let mut bytes = [0u8; 8];
        getrandom::getrandom(&mut bytes).map_err(|_| InstanceIdGenerationError::Entropy)?;
        InstanceId::parse(&format!("i_{}", encode_instance_id(bytes)))
            .map_err(InstanceIdGenerationError::Invalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoder_fixed_vectors() {
        assert_eq!(encode_instance_id([0, 0, 0, 0, 0, 0, 0, 0]), "000000000000");
        assert_eq!(
            encode_instance_id([0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]),
            "zzzzzzzzzzzz"
        );
        assert_eq!(
            encode_instance_id([0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]),
            "4d2pf2dbsqqg"
        );
        assert_eq!(encode_instance_id([0, 0, 0, 0, 0, 0, 0, 1]), "000000000001");
    }

    #[test]
    fn encoder_uses_crockford_lowercase_without_padding() {
        let encoded = encode_instance_id([0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]);
        assert_eq!(encoded.len(), 12);
        assert!(encoded.bytes().all(|byte| ALPHABET.contains(&byte)));
        assert!(!encoded
            .chars()
            .any(|character| matches!(character, 'i' | 'l' | 'o' | 'u')));
        assert!(!encoded.contains('='));
    }

    #[test]
    fn generated_id_has_prefix_and_parses() {
        let generator = RandomInstanceIdGenerator::new();
        let id = generator.generate().unwrap();
        assert!(id.as_str().starts_with("i_"));
        assert_eq!(id.as_str().len(), 14);
        assert_eq!(InstanceId::parse(id.as_str()).unwrap(), id);
    }

    #[test]
    fn generated_ids_vary() {
        let generator = RandomInstanceIdGenerator::new();
        assert_ne!(generator.generate().unwrap(), generator.generate().unwrap());
    }
}
