use std::num::NonZeroU32;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicyV1 {
    pub initial_delay: Duration,
    pub maximum_delay: Duration,
    pub jitter_basis_points: u16,
}

impl Default for RetryPolicyV1 {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_secs(1),
            maximum_delay: Duration::from_secs(300),
            jitter_basis_points: 2_000,
        }
    }
}

impl RetryPolicyV1 {
    pub fn validate(self) -> Result<(), RetryPolicyError> {
        if self.initial_delay.is_zero() || self.maximum_delay.is_zero() {
            return Err(RetryPolicyError::ZeroDelay);
        }
        if self.initial_delay > self.maximum_delay
            || self.maximum_delay > Duration::from_secs(86_400)
        {
            return Err(RetryPolicyError::InvalidDelayRange);
        }
        if self.initial_delay.as_millis() == 0 || self.jitter_basis_points > 5_000 {
            return Err(RetryPolicyError::InvalidJitter);
        }
        Ok(())
    }

    pub fn delay(self, attempt: NonZeroU32, entropy: u64) -> Result<Duration, RetryPolicyError> {
        self.validate()?;
        let initial = self.initial_delay.as_millis();
        let maximum = self.maximum_delay.as_millis();
        let shift = attempt.get().saturating_sub(1).min(63);
        let exponential = initial.saturating_mul(1_u128 << shift).min(maximum);
        let span = exponential.saturating_mul(u128::from(self.jitter_basis_points)) / 10_000;
        let width = span.saturating_mul(2).saturating_add(1);
        let offset = u128::from(entropy) % width;
        let jittered = exponential
            .saturating_sub(span)
            .saturating_add(offset)
            .clamp(1, maximum);
        let milliseconds = u64::try_from(jittered).map_err(|_| RetryPolicyError::DelayOverflow)?;
        Ok(Duration::from_millis(milliseconds))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RetryPolicyError {
    #[error("runtime retry delay must be non-zero")]
    ZeroDelay,
    #[error("runtime retry delay range is invalid")]
    InvalidDelayRange,
    #[error("runtime retry jitter is invalid")]
    InvalidJitter,
    #[error("runtime retry delay overflowed")]
    DelayOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponential_delay_is_bounded_and_deterministic() {
        let policy = RetryPolicyV1::default();
        let first = policy.delay(NonZeroU32::new(1).unwrap(), 10).unwrap();
        let fifth = policy.delay(NonZeroU32::new(5).unwrap(), 10).unwrap();
        let huge = policy
            .delay(NonZeroU32::new(u32::MAX).unwrap(), u64::MAX)
            .unwrap();
        assert_eq!(
            first,
            policy.delay(NonZeroU32::new(1).unwrap(), 10).unwrap()
        );
        assert!(fifth > first);
        assert!(huge <= policy.maximum_delay);
        assert!(!huge.is_zero());
    }

    #[test]
    fn jitter_stays_inside_the_configured_window() {
        let policy = RetryPolicyV1 {
            initial_delay: Duration::from_secs(10),
            maximum_delay: Duration::from_secs(100),
            jitter_basis_points: 2_000,
        };
        let attempt = NonZeroU32::new(1).unwrap();
        for entropy in [0, 1, u64::MAX / 2, u64::MAX] {
            let delay = policy.delay(attempt, entropy).unwrap();
            assert!(delay >= Duration::from_secs(8));
            assert!(delay <= Duration::from_secs(12));
        }
    }

    #[test]
    fn unsafe_policy_is_rejected() {
        let policy = RetryPolicyV1 {
            initial_delay: Duration::from_secs(2),
            maximum_delay: Duration::from_secs(1),
            jitter_basis_points: 0,
        };
        assert_eq!(policy.validate(), Err(RetryPolicyError::InvalidDelayRange));
    }
}
