use std::fmt::{Debug, Formatter};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::process_supervisor::RuntimeProcessInvalidationTriggerV1;
use crate::RuntimeShutdownCauseV1;

pub(crate) trait RuntimeIngressAcknowledgementSafetyInvalidationPortV2:
    Clone + Send + Sync + 'static
{
    fn invalidate_ingress_acknowledgement_v2(&self);
}

impl RuntimeIngressAcknowledgementSafetyInvalidationPortV2 for RuntimeProcessInvalidationTriggerV1 {
    fn invalidate_ingress_acknowledgement_v2(&self) {
        self.trip(RuntimeShutdownCauseV1::ReadinessLost);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeIngressAcknowledgementSafetyStageV2 {
    Armed,
    Expired,
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RuntimeIngressAcknowledgementSafetySnapshotV2 {
    deadline: Instant,
    generation: u64,
    stage: RuntimeIngressAcknowledgementSafetyStageV2,
}

struct RuntimeIngressAcknowledgementSafetyStateV2 {
    snapshot: RuntimeIngressAcknowledgementSafetySnapshotV2,
    invalidation_claimed: bool,
}

struct RuntimeIngressAcknowledgementSafetySharedV2 {
    state: Mutex<RuntimeIngressAcknowledgementSafetyStateV2>,
    publisher: watch::Sender<RuntimeIngressAcknowledgementSafetySnapshotV2>,
}

struct RuntimeIngressAcknowledgementSafetyControlV2 {
    shared: Arc<RuntimeIngressAcknowledgementSafetySharedV2>,
}

struct RuntimeIngressAcknowledgementSafetyObserverV2 {
    shared: Arc<RuntimeIngressAcknowledgementSafetySharedV2>,
    receiver: watch::Receiver<RuntimeIngressAcknowledgementSafetySnapshotV2>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeIngressAcknowledgementSafetyWaitV2 {
    Expired,
    Stopped,
    ControlClosed,
}

pub(crate) struct RuntimeIngressAcknowledgementSafetyMonitorV2<
    I: RuntimeIngressAcknowledgementSafetyInvalidationPortV2 = RuntimeProcessInvalidationTriggerV1,
> {
    control: RuntimeIngressAcknowledgementSafetyControlV2,
    invalidation: I,
    task: Option<JoinHandle<RuntimeIngressAcknowledgementSafetyWaitV2>>,
    stopped: bool,
}

impl RuntimeIngressAcknowledgementSafetyControlV2 {
    fn new_v2(deadline: Instant) -> (Self, RuntimeIngressAcknowledgementSafetyObserverV2) {
        let snapshot = RuntimeIngressAcknowledgementSafetySnapshotV2 {
            deadline,
            generation: 1,
            stage: RuntimeIngressAcknowledgementSafetyStageV2::Armed,
        };
        let (publisher, receiver) = watch::channel(snapshot);
        let shared = Arc::new(RuntimeIngressAcknowledgementSafetySharedV2 {
            state: Mutex::new(RuntimeIngressAcknowledgementSafetyStateV2 {
                snapshot,
                invalidation_claimed: false,
            }),
            publisher,
        });
        (
            Self {
                shared: shared.clone(),
            },
            RuntimeIngressAcknowledgementSafetyObserverV2 { shared, receiver },
        )
    }

    fn rearm_at_v2(&self, deadline: Instant, now: Instant) -> bool {
        let (snapshot, accepted) = {
            let mut state = self.lock_state_v2();
            let current = state.snapshot;
            if current.stage != RuntimeIngressAcknowledgementSafetyStageV2::Armed
                || now >= current.deadline
                || now >= deadline
                || deadline <= current.deadline
            {
                if current.stage == RuntimeIngressAcknowledgementSafetyStageV2::Armed
                    && now >= current.deadline
                {
                    state.snapshot.stage = RuntimeIngressAcknowledgementSafetyStageV2::Expired;
                }
                (state.snapshot, false)
            } else if let Some(generation) = current.generation.checked_add(1) {
                state.snapshot.deadline = deadline;
                state.snapshot.generation = generation;
                (state.snapshot, true)
            } else {
                state.snapshot.stage = RuntimeIngressAcknowledgementSafetyStageV2::Expired;
                (state.snapshot, false)
            }
        };
        self.shared.publisher.send_replace(snapshot);
        accepted
    }

    fn stop_v2(&self) -> bool {
        let snapshot = {
            let mut state = self.lock_state_v2();
            if state.snapshot.stage == RuntimeIngressAcknowledgementSafetyStageV2::Armed {
                state.snapshot.stage = RuntimeIngressAcknowledgementSafetyStageV2::Stopped;
            }
            state.snapshot
        };
        self.shared.publisher.send_replace(snapshot);
        snapshot.stage == RuntimeIngressAcknowledgementSafetyStageV2::Stopped
    }

    fn expire_v2(&self) {
        let snapshot = {
            let mut state = self.lock_state_v2();
            if state.snapshot.stage == RuntimeIngressAcknowledgementSafetyStageV2::Armed {
                state.snapshot.stage = RuntimeIngressAcknowledgementSafetyStageV2::Expired;
            }
            state.snapshot
        };
        self.shared.publisher.send_replace(snapshot);
    }

    fn claim_invalidation_v2(&self) -> bool {
        let mut state = self.lock_state_v2();
        if state.invalidation_claimed {
            false
        } else {
            state.invalidation_claimed = true;
            true
        }
    }

    fn lock_state_v2(
        &self,
    ) -> std::sync::MutexGuard<'_, RuntimeIngressAcknowledgementSafetyStateV2> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl RuntimeIngressAcknowledgementSafetyObserverV2 {
    async fn wait_v2(&mut self) -> RuntimeIngressAcknowledgementSafetyWaitV2 {
        loop {
            let snapshot = *self.receiver.borrow_and_update();
            match snapshot.stage {
                RuntimeIngressAcknowledgementSafetyStageV2::Expired => {
                    return RuntimeIngressAcknowledgementSafetyWaitV2::Expired;
                }
                RuntimeIngressAcknowledgementSafetyStageV2::Stopped => {
                    return RuntimeIngressAcknowledgementSafetyWaitV2::Stopped;
                }
                RuntimeIngressAcknowledgementSafetyStageV2::Armed => {}
            }
            let sleep = tokio::time::sleep_until(tokio::time::Instant::from_std(snapshot.deadline));
            tokio::pin!(sleep);
            tokio::select! {
                biased;
                changed = self.receiver.changed() => {
                    if changed.is_err() {
                        return RuntimeIngressAcknowledgementSafetyWaitV2::ControlClosed;
                    }
                }
                _ = &mut sleep => {
                    if self.expire_if_current_v2(
                        snapshot,
                        tokio::time::Instant::now().into_std(),
                    ) {
                        return RuntimeIngressAcknowledgementSafetyWaitV2::Expired;
                    }
                }
            }
        }
    }

    fn expire_if_current_v2(
        &self,
        observed: RuntimeIngressAcknowledgementSafetySnapshotV2,
        now: Instant,
    ) -> bool {
        let snapshot = {
            let mut state = self
                .shared
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.snapshot == observed
                && state.snapshot.stage == RuntimeIngressAcknowledgementSafetyStageV2::Armed
                && now >= state.snapshot.deadline
            {
                state.snapshot.stage = RuntimeIngressAcknowledgementSafetyStageV2::Expired;
            }
            state.snapshot
        };
        self.shared.publisher.send_replace(snapshot);
        snapshot.stage == RuntimeIngressAcknowledgementSafetyStageV2::Expired
    }

    fn claim_invalidation_v2(&self) -> bool {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.invalidation_claimed {
            false
        } else {
            state.invalidation_claimed = true;
            true
        }
    }
}

impl<I> RuntimeIngressAcknowledgementSafetyMonitorV2<I>
where
    I: RuntimeIngressAcknowledgementSafetyInvalidationPortV2,
{
    pub(crate) fn start_v2(deadline: Instant, invalidation: I) -> Self {
        let (control, mut observer) =
            RuntimeIngressAcknowledgementSafetyControlV2::new_v2(deadline);
        let task_invalidation = invalidation.clone();
        let task = tokio::spawn(async move {
            let outcome = observer.wait_v2().await;
            if matches!(
                outcome,
                RuntimeIngressAcknowledgementSafetyWaitV2::Expired
                    | RuntimeIngressAcknowledgementSafetyWaitV2::ControlClosed
            ) && observer.claim_invalidation_v2()
            {
                task_invalidation.invalidate_ingress_acknowledgement_v2();
            }
            outcome
        });
        Self {
            control,
            invalidation,
            task: Some(task),
            stopped: false,
        }
    }

    pub(crate) fn rearm_v2(&self, deadline: Instant) -> bool {
        self.rearm_at_v2(deadline, Instant::now())
    }

    fn rearm_at_v2(&self, deadline: Instant, now: Instant) -> bool {
        let accepted = self.control.rearm_at_v2(deadline, now);
        if !accepted && self.control.claim_invalidation_v2() {
            self.invalidation.invalidate_ingress_acknowledgement_v2();
        }
        accepted
    }

    pub(crate) async fn stop_v2(mut self) {
        if !self.control.stop_v2() && self.control.claim_invalidation_v2() {
            self.invalidation.invalidate_ingress_acknowledgement_v2();
        }
        self.stopped = true;
        if let Some(task) = self.task.take() {
            task.abort();
            let _joined = task.await;
        }
    }
}

impl<I> Drop for RuntimeIngressAcknowledgementSafetyMonitorV2<I>
where
    I: RuntimeIngressAcknowledgementSafetyInvalidationPortV2,
{
    fn drop(&mut self) {
        if !self.stopped {
            self.control.expire_v2();
            if self.control.claim_invalidation_v2() {
                self.invalidation.invalidate_ingress_acknowledgement_v2();
            }
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl<I> Debug for RuntimeIngressAcknowledgementSafetyMonitorV2<I>
where
    I: RuntimeIngressAcknowledgementSafetyInvalidationPortV2,
{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeIngressAcknowledgementSafetyMonitorV2(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;

    #[derive(Clone)]
    struct FakeInvalidationV2 {
        count: Arc<AtomicUsize>,
    }

    impl FakeInvalidationV2 {
        fn new() -> Self {
            Self {
                count: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn count_v2(&self) -> usize {
            self.count.load(Ordering::Acquire)
        }
    }

    impl RuntimeIngressAcknowledgementSafetyInvalidationPortV2 for FakeInvalidationV2 {
        fn invalidate_ingress_acknowledgement_v2(&self) {
            self.count.fetch_add(1, Ordering::AcqRel);
        }
    }

    async fn wait_for_count_v2(invalidation: &FakeInvalidationV2, expected: usize) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while invalidation.count_v2() < expected {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("safety invalidation");
    }

    #[tokio::test]
    async fn armed_monitor_expires_without_a_runtime_loop_poll() {
        let invalidation = FakeInvalidationV2::new();
        let monitor = RuntimeIngressAcknowledgementSafetyMonitorV2::start_v2(
            Instant::now() + Duration::from_millis(10),
            invalidation.clone(),
        );
        wait_for_count_v2(&invalidation, 1).await;
        drop(monitor);
        assert_eq!(invalidation.count_v2(), 1);
    }

    #[tokio::test]
    async fn initial_acknowledgement_deadline_trips_while_post_acknowledgement_work_is_blocked() {
        let invalidation = FakeInvalidationV2::new();
        let monitor = RuntimeIngressAcknowledgementSafetyMonitorV2::start_v2(
            Instant::now() + Duration::from_millis(10),
            invalidation.clone(),
        );
        let blocked = pending::<()>();
        tokio::pin!(blocked);
        tokio::select! {
            () = &mut blocked => panic!("post acknowledgement work completed"),
            () = wait_for_count_v2(&invalidation, 1) => {}
        }
        monitor.stop_v2().await;
        assert_eq!(invalidation.count_v2(), 1);
    }

    #[tokio::test]
    async fn rearm_keeps_the_old_deadline_live_until_one_strict_successor_is_installed() {
        let invalidation = FakeInvalidationV2::new();
        let old_deadline = Instant::now() + Duration::from_millis(80);
        let monitor = RuntimeIngressAcknowledgementSafetyMonitorV2::start_v2(
            old_deadline,
            invalidation.clone(),
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
        let new_deadline = old_deadline + Duration::from_millis(80);
        assert!(monitor.rearm_v2(new_deadline));
        tokio::time::sleep_until(tokio::time::Instant::from_std(old_deadline)).await;
        assert_eq!(invalidation.count_v2(), 0);
        wait_for_count_v2(&invalidation, 1).await;
        drop(monitor);
        assert_eq!(invalidation.count_v2(), 1);
    }

    #[tokio::test]
    async fn expiry_boundary_rejects_rearm_and_invalidates_exactly_once() {
        let invalidation = FakeInvalidationV2::new();
        let started_at = Instant::now();
        let deadline = started_at + Duration::from_secs(10);
        let monitor =
            RuntimeIngressAcknowledgementSafetyMonitorV2::start_v2(deadline, invalidation.clone());
        assert!(!monitor.rearm_at_v2(deadline + Duration::from_secs(10), deadline,));
        assert_eq!(invalidation.count_v2(), 1);
        drop(monitor);
        assert_eq!(invalidation.count_v2(), 1);
    }

    #[tokio::test]
    async fn stop_aborts_and_joins_without_invalidating() {
        let invalidation = FakeInvalidationV2::new();
        let monitor = RuntimeIngressAcknowledgementSafetyMonitorV2::start_v2(
            Instant::now() + Duration::from_secs(10),
            invalidation.clone(),
        );
        monitor.stop_v2().await;
        assert_eq!(invalidation.count_v2(), 0);
    }

    #[tokio::test]
    async fn drop_is_synchronous_fail_closed_and_does_not_duplicate() {
        let invalidation = FakeInvalidationV2::new();
        let monitor = RuntimeIngressAcknowledgementSafetyMonitorV2::start_v2(
            Instant::now() + Duration::from_secs(10),
            invalidation.clone(),
        );
        drop(monitor);
        assert_eq!(invalidation.count_v2(), 1);
        tokio::task::yield_now().await;
        assert_eq!(invalidation.count_v2(), 1);
    }

    #[tokio::test]
    async fn accepted_acknowledgement_cancellation_before_transfer_fails_closed_synchronously() {
        let invalidation = FakeInvalidationV2::new();
        let accepted_acknowledgement_safety =
            Some(RuntimeIngressAcknowledgementSafetyMonitorV2::start_v2(
                Instant::now() + Duration::from_secs(10),
                invalidation.clone(),
            ));
        drop(accepted_acknowledgement_safety);
        assert_eq!(invalidation.count_v2(), 1);
        tokio::task::yield_now().await;
        assert_eq!(invalidation.count_v2(), 1);
    }
}
