use std::fmt::{Debug, Formatter};
use std::time::{Duration, Instant};

const STARTUP_OPERATION_WINDOW: Duration = Duration::from_secs(35);
const STARTUP_TOTAL_WINDOW: Duration = Duration::from_secs(45);

pub struct RuntimeStartupBudgetV1 {
    operation_cutoff: Instant,
    cleanup_deadline: Instant,
}

impl RuntimeStartupBudgetV1 {
    pub fn begin() -> Self {
        Self::from_started_at(Instant::now())
    }

    fn from_started_at(started_at: Instant) -> Self {
        Self {
            operation_cutoff: started_at + STARTUP_OPERATION_WINDOW,
            cleanup_deadline: started_at + STARTUP_TOTAL_WINDOW,
        }
    }

    pub(crate) fn operation_cutoff(&self) -> Instant {
        self.operation_cutoff
    }

    pub(crate) fn cleanup_deadline(&self) -> Instant {
        self.cleanup_deadline
    }

    pub(crate) fn operation_is_open(&self) -> bool {
        self.operation_is_open_at(Instant::now())
    }

    fn operation_is_open_at(&self, now: Instant) -> bool {
        now < self.operation_cutoff
    }
}

impl Debug for RuntimeStartupBudgetV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeStartupBudgetV1(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_start_instant_derives_exact_operation_and_cleanup_windows() {
        let started_at = Instant::now();
        let budget = RuntimeStartupBudgetV1::from_started_at(started_at);

        assert_eq!(
            budget.operation_cutoff.duration_since(started_at),
            Duration::from_secs(35)
        );
        assert_eq!(
            budget.cleanup_deadline.duration_since(started_at),
            Duration::from_secs(45)
        );
        assert_eq!(
            budget
                .cleanup_deadline
                .duration_since(budget.operation_cutoff),
            Duration::from_secs(10)
        );
    }

    #[test]
    fn operation_window_is_strict_and_cleanup_tail_never_reopens_it() {
        let started_at = Instant::now();
        let budget = RuntimeStartupBudgetV1::from_started_at(started_at);

        assert!(budget.operation_is_open_at(started_at));
        assert!(budget.operation_is_open_at(budget.operation_cutoff - Duration::from_nanos(1)));
        assert!(!budget.operation_is_open_at(budget.operation_cutoff));
        assert!(!budget.operation_is_open_at(budget.operation_cutoff + Duration::from_secs(5)));
        assert!(!budget.operation_is_open_at(budget.cleanup_deadline));
    }

    #[test]
    fn startup_budget_has_no_diagnostic_time_surface() {
        let budget = RuntimeStartupBudgetV1::begin();

        assert_eq!(format!("{budget:?}"), "RuntimeStartupBudgetV1(<redacted>)");
    }
}
