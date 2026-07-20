use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::config::OAuthStartBudgetConfig;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OAuthStartAdmission {
    Admitted,
    Rejected { retry_after_seconds: u64 },
    Unavailable,
}

pub(super) struct OAuthStartBudget {
    capacity: u32,
    refill_interval: Duration,
    state: Mutex<OAuthStartBudgetState>,
}

struct OAuthStartBudgetState {
    available: u32,
    refilled_at: Instant,
}

impl OAuthStartBudget {
    pub(super) fn new(config: OAuthStartBudgetConfig) -> Self {
        Self::new_at(config.capacity(), config.refill_interval(), Instant::now())
    }

    fn new_at(capacity: u32, refill_interval: Duration, now: Instant) -> Self {
        Self {
            capacity,
            refill_interval,
            state: Mutex::new(OAuthStartBudgetState {
                available: capacity,
                refilled_at: now,
            }),
        }
    }

    pub(super) fn try_acquire(&self) -> OAuthStartAdmission {
        self.try_acquire_at(Instant::now())
    }

    fn try_acquire_at(&self, now: Instant) -> OAuthStartAdmission {
        let Ok(mut state) = self.state.lock() else {
            return OAuthStartAdmission::Unavailable;
        };
        self.refill(&mut state, now);
        if state.available > 0 {
            if state.available == self.capacity {
                state.refilled_at = now;
            }
            state.available -= 1;
            OAuthStartAdmission::Admitted
        } else {
            OAuthStartAdmission::Rejected {
                retry_after_seconds: retry_after_seconds(
                    self.refill_interval.saturating_sub(
                        now.checked_duration_since(state.refilled_at)
                            .unwrap_or_default(),
                    ),
                ),
            }
        }
    }

    fn refill(&self, state: &mut OAuthStartBudgetState, now: Instant) {
        let elapsed = now
            .checked_duration_since(state.refilled_at)
            .unwrap_or_default();
        let intervals = elapsed.as_nanos() / self.refill_interval.as_nanos();
        if intervals == 0 {
            return;
        }
        let missing = self.capacity - state.available;
        if intervals >= u128::from(missing) {
            state.available = self.capacity;
            state.refilled_at = now;
            return;
        }
        let restored = u32::try_from(intervals).unwrap_or(missing);
        state.available += restored;
        let advancement = self
            .refill_interval
            .checked_mul(restored)
            .unwrap_or(elapsed);
        state.refilled_at = state.refilled_at.checked_add(advancement).unwrap_or(now);
    }
}

fn retry_after_seconds(duration: Duration) -> u64 {
    let seconds = duration
        .as_nanos()
        .saturating_add(999_999_999)
        .checked_div(1_000_000_000)
        .unwrap_or(1);
    u64::try_from(seconds.max(1)).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::thread;

    use super::*;

    #[test]
    fn discrete_refill_is_bounded_and_monotonic() {
        let now = Instant::now();
        let budget = OAuthStartBudget::new_at(2, Duration::from_secs(2), now);

        assert_eq!(budget.try_acquire_at(now), OAuthStartAdmission::Admitted);
        assert_eq!(budget.try_acquire_at(now), OAuthStartAdmission::Admitted);
        assert_eq!(
            budget.try_acquire_at(now),
            OAuthStartAdmission::Rejected {
                retry_after_seconds: 2
            }
        );
        assert_eq!(
            budget.try_acquire_at(now + Duration::from_millis(1_999)),
            OAuthStartAdmission::Rejected {
                retry_after_seconds: 1
            }
        );
        assert_eq!(
            budget.try_acquire_at(now + Duration::from_secs(2)),
            OAuthStartAdmission::Admitted
        );
        assert_eq!(
            budget.try_acquire_at(now + Duration::from_secs(2)),
            OAuthStartAdmission::Rejected {
                retry_after_seconds: 2
            }
        );
        assert_eq!(
            budget.try_acquire_at(now + Duration::from_secs(20)),
            OAuthStartAdmission::Admitted
        );
        assert_eq!(
            budget.try_acquire_at(now + Duration::from_secs(20)),
            OAuthStartAdmission::Admitted
        );
        assert_eq!(
            budget.try_acquire_at(now + Duration::from_secs(20)),
            OAuthStartAdmission::Rejected {
                retry_after_seconds: 2
            }
        );
    }

    #[test]
    fn full_bucket_does_not_bank_refill_credit() {
        let now = Instant::now();
        let budget = OAuthStartBudget::new_at(2, Duration::from_secs(2), now);
        let first = now + Duration::from_millis(1_999);

        assert_eq!(budget.try_acquire_at(first), OAuthStartAdmission::Admitted);
        assert_eq!(budget.try_acquire_at(first), OAuthStartAdmission::Admitted);
        assert_eq!(
            budget.try_acquire_at(now + Duration::from_secs(2)),
            OAuthStartAdmission::Rejected {
                retry_after_seconds: 2
            }
        );
    }

    #[test]
    fn concurrent_admission_never_exceeds_capacity() {
        let now = Instant::now();
        let budget = Arc::new(OAuthStartBudget::new_at(10, Duration::from_secs(2), now));
        let barrier = Arc::new(Barrier::new(65));
        let handles = (0..64)
            .map(|_| {
                let budget = Arc::clone(&budget);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    budget.try_acquire_at(now)
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let admitted = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|outcome| *outcome == OAuthStartAdmission::Admitted)
            .count();

        assert_eq!(admitted, 10);
    }

    #[test]
    fn poisoned_state_fails_closed() {
        let now = Instant::now();
        let budget = Arc::new(OAuthStartBudget::new_at(10, Duration::from_secs(2), now));
        let poison = Arc::clone(&budget);
        let result = thread::spawn(move || {
            let _guard = poison.state.lock().unwrap();
            panic!("poison token bucket");
        })
        .join();

        assert!(result.is_err());
        assert_eq!(budget.try_acquire_at(now), OAuthStartAdmission::Unavailable);
    }
}
