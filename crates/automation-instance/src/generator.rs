use std::sync::atomic::{AtomicU64, Ordering};

use crate::id::{InstanceId, InstanceIdError};

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

#[cfg(test)]
mod tests {
    use crate::InstanceIdError;

    use super::{InstanceIdGenerationError, InstanceIdGenerator, SequenceInstanceIdGenerator};

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
}
