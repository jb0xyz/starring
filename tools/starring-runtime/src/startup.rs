use std::fmt::{Debug, Formatter};
use std::time::{Duration, Instant};

const STARTUP_OPERATION_WINDOW: Duration = Duration::from_secs(35);
const STARTUP_TOTAL_WINDOW: Duration = Duration::from_secs(45);
const STARTUP_DISCORD_CLEANUP_RESERVE: Duration = Duration::from_secs(7);
const STARTUP_DATABASE_CLEANUP_RESERVE: Duration = Duration::from_secs(2);

pub(crate) struct RuntimeStartupBudgetV1 {
    operation_cutoff: Instant,
    cleanup_deadline: Instant,
}

impl RuntimeStartupBudgetV1 {
    pub(crate) fn begin() -> Self {
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

    pub(crate) fn discord_cleanup_deadline(&self) -> Instant {
        self.cleanup_deadline
            .checked_sub(STARTUP_DISCORD_CLEANUP_RESERVE)
            .expect("startup Discord cleanup deadline")
    }

    pub(crate) fn owner_cleanup_deadline(&self) -> Instant {
        self.cleanup_deadline
            .checked_sub(STARTUP_DATABASE_CLEANUP_RESERVE)
            .expect("startup owner cleanup deadline")
    }

    pub(crate) fn operation_is_open(&self) -> bool {
        self.operation_is_open_at(Instant::now())
    }

    fn operation_is_open_at(&self, now: Instant) -> bool {
        now < self.operation_cutoff
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeStartupSyncStageErrorV1<E> {
    OperationDeadlineElapsed,
    Stage(E),
}

pub(crate) fn run_runtime_startup_sync_stage_v1<T, E, C, S>(
    mut operation_is_open: C,
    stage: S,
) -> Result<T, RuntimeStartupSyncStageErrorV1<E>>
where
    C: FnMut() -> bool,
    S: FnOnce() -> Result<T, E>,
{
    if !operation_is_open() {
        return Err(RuntimeStartupSyncStageErrorV1::OperationDeadlineElapsed);
    }
    let result = stage();
    if !operation_is_open() {
        return Err(RuntimeStartupSyncStageErrorV1::OperationDeadlineElapsed);
    }
    result.map_err(RuntimeStartupSyncStageErrorV1::Stage)
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
        assert_eq!(
            budget
                .discord_cleanup_deadline()
                .duration_since(budget.operation_cutoff),
            Duration::from_secs(3)
        );
        assert_eq!(
            budget
                .owner_cleanup_deadline()
                .duration_since(budget.discord_cleanup_deadline()),
            Duration::from_secs(5)
        );
        assert_eq!(
            budget
                .cleanup_deadline()
                .duration_since(budget.owner_cleanup_deadline()),
            Duration::from_secs(2)
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

    #[test]
    fn closed_precheck_skips_the_sync_stage() {
        let mut calls = 0;
        let result = run_runtime_startup_sync_stage_v1(
            || false,
            || {
                calls += 1;
                Ok::<_, u8>(())
            },
        );

        assert_eq!(calls, 0);
        assert_eq!(
            result,
            Err(RuntimeStartupSyncStageErrorV1::OperationDeadlineElapsed)
        );
    }

    #[test]
    fn closed_postcheck_precedes_both_success_and_stage_error() {
        for stage_result in [Ok::<_, u8>(()), Err(7)] {
            let mut states = [true, false].into_iter();
            let result =
                run_runtime_startup_sync_stage_v1(|| states.next().unwrap(), || stage_result);

            assert_eq!(
                result,
                Err(RuntimeStartupSyncStageErrorV1::OperationDeadlineElapsed)
            );
            assert!(states.next().is_none());
        }
    }

    #[test]
    fn open_checks_preserve_success_and_stage_error() {
        let mut success_states = [true, true].into_iter();
        let success =
            run_runtime_startup_sync_stage_v1(|| success_states.next().unwrap(), || Ok::<_, u8>(9));
        let mut error_states = [true, true].into_iter();
        let error =
            run_runtime_startup_sync_stage_v1(|| error_states.next().unwrap(), || Err::<u8, _>(11));

        assert_eq!(success, Ok(9));
        assert_eq!(error, Err(RuntimeStartupSyncStageErrorV1::Stage(11)));
        assert!(success_states.next().is_none());
        assert!(error_states.next().is_none());
    }
}
