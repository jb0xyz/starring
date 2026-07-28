use std::fmt::{Debug, Formatter};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use automation_runtime_worker::RuntimeMaintenanceGateGenerationV2;
use tokio::sync::watch;
use tokio::time::Instant as TokioInstant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeMaintenanceIngressGateStageV2 {
    Closed,
    Opening,
    Open,
    Closing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeMaintenanceIngressGateSnapshotV2 {
    generation: RuntimeMaintenanceGateGenerationV2,
    stage: RuntimeMaintenanceIngressGateStageV2,
    active_permit_count: u64,
    terminal_error: Option<RuntimeMaintenanceIngressGateErrorV2>,
}

impl RuntimeMaintenanceIngressGateSnapshotV2 {
    pub(crate) fn generation(&self) -> RuntimeMaintenanceGateGenerationV2 {
        self.generation
    }

    pub(crate) fn stage(&self) -> RuntimeMaintenanceIngressGateStageV2 {
        self.stage
    }

    pub(crate) fn active_permit_count(&self) -> u64 {
        self.active_permit_count
    }

    pub(crate) fn terminal_error(&self) -> Option<RuntimeMaintenanceIngressGateErrorV2> {
        self.terminal_error
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeMaintenanceIngressGateErrorV2 {
    #[error("runtime maintenance ingress gate authority is stale")]
    StaleAuthority,
    #[error("runtime maintenance ingress gate is not closed")]
    NotClosed,
    #[error("runtime maintenance ingress gate is not opening")]
    NotOpening,
    #[error("runtime maintenance ingress gate is not open")]
    NotOpen,
    #[error("runtime maintenance ingress gate is still draining")]
    StillDraining,
    #[error("runtime maintenance ingress gate generation overflowed")]
    GenerationOverflow,
    #[error("runtime maintenance ingress gate permit count overflowed")]
    PermitCountOverflow,
    #[error("runtime maintenance ingress gate drain deadline elapsed")]
    DrainDeadlineElapsed,
    #[error("runtime maintenance ingress gate observer stopped")]
    ObserverStopped,
    #[error("runtime maintenance ingress gate state was poisoned")]
    Poisoned,
}

impl RuntimeMaintenanceIngressGateErrorV2 {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::StaleAuthority => "runtime_maintenance_ingress_gate_stale_authority",
            Self::NotClosed => "runtime_maintenance_ingress_gate_not_closed",
            Self::NotOpening => "runtime_maintenance_ingress_gate_not_opening",
            Self::NotOpen => "runtime_maintenance_ingress_gate_not_open",
            Self::StillDraining => "runtime_maintenance_ingress_gate_still_draining",
            Self::GenerationOverflow => "runtime_maintenance_ingress_gate_generation_overflow",
            Self::PermitCountOverflow => "runtime_maintenance_ingress_gate_permit_count_overflow",
            Self::DrainDeadlineElapsed => "runtime_maintenance_ingress_gate_drain_deadline_elapsed",
            Self::ObserverStopped => "runtime_maintenance_ingress_gate_observer_stopped",
            Self::Poisoned => "runtime_maintenance_ingress_gate_poisoned",
        }
    }
}

pub(crate) struct RuntimeMaintenanceIngressGateTransitionFailureV2<S> {
    state: S,
    error: RuntimeMaintenanceIngressGateErrorV2,
}

impl<S> RuntimeMaintenanceIngressGateTransitionFailureV2<S> {
    pub(crate) fn error(&self) -> RuntimeMaintenanceIngressGateErrorV2 {
        self.error
    }

    pub(crate) fn into_state(self) -> S {
        self.state
    }
}

impl<S> Debug for RuntimeMaintenanceIngressGateTransitionFailureV2<S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeMaintenanceIngressGateTransitionFailureV2(<redacted>)")
    }
}

struct RuntimeMaintenanceIngressGateStateV2 {
    snapshot: RuntimeMaintenanceIngressGateSnapshotV2,
    open_generation: Option<RuntimeMaintenanceGateGenerationV2>,
}

struct RuntimeMaintenanceIngressGateSharedV2 {
    state: Mutex<RuntimeMaintenanceIngressGateStateV2>,
    changes: watch::Sender<RuntimeMaintenanceIngressGateSnapshotV2>,
}

pub(crate) struct RuntimeMaintenanceIngressGateControllerV2 {
    shared: Arc<RuntimeMaintenanceIngressGateSharedV2>,
    generation: RuntimeMaintenanceGateGenerationV2,
}

impl RuntimeMaintenanceIngressGateControllerV2 {
    pub(crate) fn new_v2() -> (
        Self,
        RuntimeMaintenanceIngressGateObserverV2,
        RuntimeMaintenanceIngressGateShutdownHandleV2,
    ) {
        let generation =
            RuntimeMaintenanceGateGenerationV2::new(std::num::NonZeroU64::MIN).expect("bounded");
        let snapshot = RuntimeMaintenanceIngressGateSnapshotV2 {
            generation,
            stage: RuntimeMaintenanceIngressGateStageV2::Closed,
            active_permit_count: 0,
            terminal_error: None,
        };
        let (changes, observer) = watch::channel(snapshot);
        let shared = Arc::new(RuntimeMaintenanceIngressGateSharedV2 {
            state: Mutex::new(RuntimeMaintenanceIngressGateStateV2 {
                snapshot,
                open_generation: None,
            }),
            changes,
        });
        (
            Self {
                shared: shared.clone(),
                generation,
            },
            RuntimeMaintenanceIngressGateObserverV2 {
                changes: observer.clone(),
            },
            RuntimeMaintenanceIngressGateShutdownHandleV2 { shared },
        )
    }

    pub(crate) fn begin_open_v2(
        self,
    ) -> Result<
        RuntimeMaintenanceIngressGateOpeningAuthorityV2,
        RuntimeMaintenanceIngressGateTransitionFailureV2<Self>,
    > {
        let transition = {
            let mut state = lock_state_v2(&self.shared);
            if let Some(error) = state.snapshot.terminal_error {
                Err(error)
            } else if state.snapshot.generation != self.generation {
                Err(RuntimeMaintenanceIngressGateErrorV2::StaleAuthority)
            } else if state.snapshot.stage != RuntimeMaintenanceIngressGateStageV2::Closed {
                Err(RuntimeMaintenanceIngressGateErrorV2::NotClosed)
            } else {
                match next_generation_v2(self.generation) {
                    Ok(next) => {
                        state.snapshot = RuntimeMaintenanceIngressGateSnapshotV2 {
                            generation: next,
                            stage: RuntimeMaintenanceIngressGateStageV2::Opening,
                            active_permit_count: 0,
                            terminal_error: None,
                        };
                        state.open_generation = Some(next);
                        self.shared.changes.send_replace(state.snapshot);
                        Ok(next)
                    }
                    Err(error) => {
                        fail_closed_state_v2(&mut state, error);
                        self.shared.changes.send_replace(state.snapshot);
                        Err(error)
                    }
                }
            }
        };
        let next = match transition {
            Ok(next) => next,
            Err(error) => {
                return Err(RuntimeMaintenanceIngressGateTransitionFailureV2 {
                    state: self,
                    error,
                });
            }
        };
        Ok(RuntimeMaintenanceIngressGateOpeningAuthorityV2 {
            shared: Some(self.shared),
            generation: next,
        })
    }
}

impl Debug for RuntimeMaintenanceIngressGateControllerV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeMaintenanceIngressGateControllerV2(<redacted>)")
    }
}

pub(crate) struct RuntimeMaintenanceIngressGateOpeningAuthorityV2 {
    shared: Option<Arc<RuntimeMaintenanceIngressGateSharedV2>>,
    generation: RuntimeMaintenanceGateGenerationV2,
}

impl RuntimeMaintenanceIngressGateOpeningAuthorityV2 {
    pub(crate) fn generation(&self) -> RuntimeMaintenanceGateGenerationV2 {
        self.generation
    }

    pub(crate) fn commit_open_v2(
        mut self,
    ) -> Result<
        RuntimeMaintenanceIngressGateOpenAuthorityV2,
        RuntimeMaintenanceIngressGateTransitionFailureV2<Self>,
    > {
        let transition = {
            let shared = self.shared.as_ref().expect("opening gate authority");
            let mut state = lock_state_v2(shared);
            if let Some(error) = state.snapshot.terminal_error {
                Err(error)
            } else if state.snapshot.generation != self.generation {
                Err(RuntimeMaintenanceIngressGateErrorV2::StaleAuthority)
            } else if state.snapshot.stage != RuntimeMaintenanceIngressGateStageV2::Opening {
                Err(RuntimeMaintenanceIngressGateErrorV2::NotOpening)
            } else {
                state.snapshot.stage = RuntimeMaintenanceIngressGateStageV2::Open;
                shared.changes.send_replace(state.snapshot);
                Ok(())
            }
        };
        if let Err(error) = transition {
            return Err(RuntimeMaintenanceIngressGateTransitionFailureV2 { state: self, error });
        }
        Ok(RuntimeMaintenanceIngressGateOpenAuthorityV2 {
            shared: self.shared.take(),
            generation: self.generation,
        })
    }

    pub(crate) fn begin_close_v2(mut self) -> RuntimeMaintenanceIngressGateDrainHandleV2 {
        let shared = self.shared.take().expect("opening gate authority");
        let closing_generation = close_shared_v2(&shared, self.generation);
        RuntimeMaintenanceIngressGateDrainHandleV2 {
            shared,
            closing_generation,
        }
    }
}

impl Drop for RuntimeMaintenanceIngressGateOpeningAuthorityV2 {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.take() {
            close_shared_v2(&shared, self.generation);
        }
    }
}

impl Debug for RuntimeMaintenanceIngressGateOpeningAuthorityV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeMaintenanceIngressGateOpeningAuthorityV2(<redacted>)")
    }
}

pub(crate) struct RuntimeMaintenanceIngressGateOpenAuthorityV2 {
    shared: Option<Arc<RuntimeMaintenanceIngressGateSharedV2>>,
    generation: RuntimeMaintenanceGateGenerationV2,
}

impl RuntimeMaintenanceIngressGateOpenAuthorityV2 {
    pub(crate) fn generation(&self) -> RuntimeMaintenanceGateGenerationV2 {
        self.generation
    }

    pub(crate) fn begin_close_v2(mut self) -> RuntimeMaintenanceIngressGateDrainHandleV2 {
        let shared = self.shared.take().expect("open gate authority");
        let closing_generation = close_shared_v2(&shared, self.generation);
        RuntimeMaintenanceIngressGateDrainHandleV2 {
            shared,
            closing_generation,
        }
    }

    pub(crate) fn try_acquire_v2(
        &self,
    ) -> Result<RuntimeMaintenanceIngressGatePermitV2, RuntimeMaintenanceIngressGateErrorV2> {
        let shared = self.shared.as_ref().expect("open gate authority");
        let mut state = lock_state_v2(shared);
        if let Some(error) = state.snapshot.terminal_error {
            return Err(error);
        }
        if state.snapshot.generation != self.generation {
            return Err(RuntimeMaintenanceIngressGateErrorV2::StaleAuthority);
        }
        if state.snapshot.stage != RuntimeMaintenanceIngressGateStageV2::Open {
            return Err(RuntimeMaintenanceIngressGateErrorV2::NotOpen);
        }
        let next_count = state
            .snapshot
            .active_permit_count
            .checked_add(1)
            .filter(|count| *count <= i64::MAX as u64)
            .ok_or(RuntimeMaintenanceIngressGateErrorV2::PermitCountOverflow);
        let next_count = match next_count {
            Ok(next_count) => next_count,
            Err(error) => {
                fail_closed_state_v2(&mut state, error);
                shared.changes.send_replace(state.snapshot);
                return Err(error);
            }
        };
        state.snapshot.active_permit_count = next_count;
        shared.changes.send_replace(state.snapshot);
        Ok(RuntimeMaintenanceIngressGatePermitV2 {
            shared: Some(shared.clone()),
            open_generation: self.generation,
        })
    }
}

impl Drop for RuntimeMaintenanceIngressGateOpenAuthorityV2 {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.take() {
            close_shared_v2(&shared, self.generation);
        }
    }
}

impl Debug for RuntimeMaintenanceIngressGateOpenAuthorityV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeMaintenanceIngressGateOpenAuthorityV2(<redacted>)")
    }
}

pub(crate) struct RuntimeMaintenanceIngressGatePermitV2 {
    shared: Option<Arc<RuntimeMaintenanceIngressGateSharedV2>>,
    open_generation: RuntimeMaintenanceGateGenerationV2,
}

impl RuntimeMaintenanceIngressGatePermitV2 {
    pub(crate) fn generation(&self) -> RuntimeMaintenanceGateGenerationV2 {
        self.open_generation
    }
}

impl Drop for RuntimeMaintenanceIngressGatePermitV2 {
    fn drop(&mut self) {
        let Some(shared) = self.shared.take() else {
            return;
        };
        let mut state = lock_state_v2(&shared);
        if state.open_generation != Some(self.open_generation)
            || state.snapshot.active_permit_count == 0
        {
            return;
        }
        state.snapshot.active_permit_count -= 1;
        if state.snapshot.active_permit_count == 0
            && state.snapshot.stage == RuntimeMaintenanceIngressGateStageV2::Closing
        {
            state.snapshot.stage = RuntimeMaintenanceIngressGateStageV2::Closed;
            state.open_generation = None;
        }
        shared.changes.send_replace(state.snapshot);
    }
}

impl Debug for RuntimeMaintenanceIngressGatePermitV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeMaintenanceIngressGatePermitV2(<redacted>)")
    }
}

pub(crate) struct RuntimeMaintenanceIngressGateDrainHandleV2 {
    shared: Arc<RuntimeMaintenanceIngressGateSharedV2>,
    closing_generation: RuntimeMaintenanceGateGenerationV2,
}

impl RuntimeMaintenanceIngressGateDrainHandleV2 {
    pub(crate) fn generation(&self) -> RuntimeMaintenanceGateGenerationV2 {
        self.closing_generation
    }

    pub(crate) async fn wait_closed_until_v2(
        &self,
        deadline: Instant,
    ) -> Result<(), RuntimeMaintenanceIngressGateErrorV2> {
        let mut changes = self.shared.changes.subscribe();
        loop {
            let snapshot = *changes.borrow_and_update();
            if let Some(error) = snapshot.terminal_error {
                return Err(error);
            }
            if snapshot.generation == self.closing_generation
                && snapshot.stage == RuntimeMaintenanceIngressGateStageV2::Closed
                && snapshot.active_permit_count == 0
            {
                return Ok(());
            }
            if snapshot.generation != self.closing_generation {
                return Err(RuntimeMaintenanceIngressGateErrorV2::StaleAuthority);
            }
            if Instant::now() >= deadline {
                return Err(RuntimeMaintenanceIngressGateErrorV2::DrainDeadlineElapsed);
            }
            match tokio::time::timeout_at(TokioInstant::from_std(deadline), changes.changed()).await
            {
                Ok(Ok(())) => {}
                Ok(Err(_)) => {
                    return Err(RuntimeMaintenanceIngressGateErrorV2::ObserverStopped);
                }
                Err(_) => {
                    return Err(RuntimeMaintenanceIngressGateErrorV2::DrainDeadlineElapsed);
                }
            }
        }
    }

    pub(crate) fn into_controller_v2(
        self,
    ) -> Result<
        RuntimeMaintenanceIngressGateControllerV2,
        RuntimeMaintenanceIngressGateTransitionFailureV2<Self>,
    > {
        let transition = {
            let state = lock_state_v2(&self.shared);
            if let Some(error) = state.snapshot.terminal_error {
                Err(error)
            } else if state.snapshot.generation != self.closing_generation {
                Err(RuntimeMaintenanceIngressGateErrorV2::StaleAuthority)
            } else if state.snapshot.stage != RuntimeMaintenanceIngressGateStageV2::Closed
                || state.snapshot.active_permit_count != 0
            {
                Err(RuntimeMaintenanceIngressGateErrorV2::StillDraining)
            } else {
                Ok(())
            }
        };
        if let Err(error) = transition {
            return Err(RuntimeMaintenanceIngressGateTransitionFailureV2 { state: self, error });
        }
        Ok(RuntimeMaintenanceIngressGateControllerV2 {
            shared: self.shared.clone(),
            generation: self.closing_generation,
        })
    }
}

impl Debug for RuntimeMaintenanceIngressGateDrainHandleV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeMaintenanceIngressGateDrainHandleV2(<redacted>)")
    }
}

#[derive(Clone)]
pub(crate) struct RuntimeMaintenanceIngressGateObserverV2 {
    changes: watch::Receiver<RuntimeMaintenanceIngressGateSnapshotV2>,
}

impl RuntimeMaintenanceIngressGateObserverV2 {
    pub(crate) fn snapshot_v2(&self) -> RuntimeMaintenanceIngressGateSnapshotV2 {
        *self.changes.borrow()
    }

    pub(crate) async fn changed_v2(&mut self) -> bool {
        self.changes.changed().await.is_ok()
    }
}

impl Debug for RuntimeMaintenanceIngressGateObserverV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeMaintenanceIngressGateObserverV2(<redacted>)")
    }
}

#[derive(Clone)]
pub(crate) struct RuntimeMaintenanceIngressGateShutdownHandleV2 {
    shared: Arc<RuntimeMaintenanceIngressGateSharedV2>,
}

impl RuntimeMaintenanceIngressGateShutdownHandleV2 {
    pub(crate) fn close_v2(&self) -> RuntimeMaintenanceIngressGateSnapshotV2 {
        let expected = {
            let state = lock_state_v2(&self.shared);
            state.open_generation.unwrap_or(state.snapshot.generation)
        };
        close_shared_v2(&self.shared, expected);
        lock_state_v2(&self.shared).snapshot
    }
}

impl Debug for RuntimeMaintenanceIngressGateShutdownHandleV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeMaintenanceIngressGateShutdownHandleV2(<redacted>)")
    }
}

fn close_shared_v2(
    shared: &Arc<RuntimeMaintenanceIngressGateSharedV2>,
    expected_open_generation: RuntimeMaintenanceGateGenerationV2,
) -> RuntimeMaintenanceGateGenerationV2 {
    let mut state = lock_state_v2(shared);
    if state.snapshot.terminal_error.is_some() {
        return state.snapshot.generation;
    }
    match state.snapshot.stage {
        RuntimeMaintenanceIngressGateStageV2::Closed
        | RuntimeMaintenanceIngressGateStageV2::Closing => state.snapshot.generation,
        RuntimeMaintenanceIngressGateStageV2::Opening
        | RuntimeMaintenanceIngressGateStageV2::Open
            if state.open_generation == Some(expected_open_generation) =>
        {
            let closing_generation = match next_generation_v2(state.snapshot.generation) {
                Ok(generation) => generation,
                Err(error) => {
                    fail_closed_state_v2(&mut state, error);
                    shared.changes.send_replace(state.snapshot);
                    return state.snapshot.generation;
                }
            };
            state.snapshot.generation = closing_generation;
            state.snapshot.stage = RuntimeMaintenanceIngressGateStageV2::Closing;
            if state.snapshot.active_permit_count == 0 {
                state.snapshot.stage = RuntimeMaintenanceIngressGateStageV2::Closed;
                state.open_generation = None;
            }
            shared.changes.send_replace(state.snapshot);
            closing_generation
        }
        RuntimeMaintenanceIngressGateStageV2::Opening
        | RuntimeMaintenanceIngressGateStageV2::Open => state.snapshot.generation,
    }
}

fn lock_state_v2(
    shared: &Arc<RuntimeMaintenanceIngressGateSharedV2>,
) -> std::sync::MutexGuard<'_, RuntimeMaintenanceIngressGateStateV2> {
    match shared.state.lock() {
        Ok(state) => state,
        Err(poisoned) => {
            let mut state = poisoned.into_inner();
            fail_closed_state_v2(&mut state, RuntimeMaintenanceIngressGateErrorV2::Poisoned);
            shared.changes.send_replace(state.snapshot);
            shared.state.clear_poison();
            state
        }
    }
}

fn fail_closed_state_v2(
    state: &mut RuntimeMaintenanceIngressGateStateV2,
    error: RuntimeMaintenanceIngressGateErrorV2,
) {
    state.snapshot.terminal_error.get_or_insert(error);
    if matches!(
        state.snapshot.stage,
        RuntimeMaintenanceIngressGateStageV2::Opening | RuntimeMaintenanceIngressGateStageV2::Open
    ) {
        if let Ok(generation) = next_generation_v2(state.snapshot.generation) {
            state.snapshot.generation = generation;
        }
    }
    state.snapshot.stage = if state.snapshot.active_permit_count == 0 {
        state.open_generation = None;
        RuntimeMaintenanceIngressGateStageV2::Closed
    } else {
        RuntimeMaintenanceIngressGateStageV2::Closing
    };
}

fn next_generation_v2(
    current: RuntimeMaintenanceGateGenerationV2,
) -> Result<RuntimeMaintenanceGateGenerationV2, RuntimeMaintenanceIngressGateErrorV2> {
    current
        .get()
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .and_then(std::num::NonZeroU64::new)
        .and_then(|value| RuntimeMaintenanceGateGenerationV2::new(value).ok())
        .ok_or(RuntimeMaintenanceIngressGateErrorV2::GenerationOverflow)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn gate_starts_closed_and_opens_only_through_linear_authority() {
        let (controller, observer, _shutdown) = RuntimeMaintenanceIngressGateControllerV2::new_v2();
        assert_eq!(
            observer.snapshot_v2().stage(),
            RuntimeMaintenanceIngressGateStageV2::Closed
        );
        let opening = controller.begin_open_v2().unwrap();
        assert_eq!(opening.generation().get(), 2);
        assert_eq!(
            observer.snapshot_v2().stage(),
            RuntimeMaintenanceIngressGateStageV2::Opening
        );
        let open = opening.commit_open_v2().unwrap();
        assert_eq!(open.generation().get(), 2);
        assert_eq!(
            observer.snapshot_v2().stage(),
            RuntimeMaintenanceIngressGateStageV2::Open
        );
        let drain = open.begin_close_v2();
        assert_eq!(drain.generation().get(), 3);
        let controller = drain.into_controller_v2().unwrap();
        let opening = controller.begin_open_v2().unwrap();
        assert_eq!(opening.generation().get(), 4);
    }

    #[tokio::test]
    async fn close_stops_new_permits_and_last_drop_completes_the_drain() {
        let (controller, mut observer, _shutdown) =
            RuntimeMaintenanceIngressGateControllerV2::new_v2();
        let open = controller
            .begin_open_v2()
            .unwrap()
            .commit_open_v2()
            .unwrap();
        assert!(observer.changed_v2().await);
        let first = open.try_acquire_v2().unwrap();
        let second = open.try_acquire_v2().unwrap();
        assert_eq!(first.generation(), open.generation());
        assert_eq!(second.generation(), open.generation());
        assert_eq!(observer.snapshot_v2().active_permit_count(), 2);
        let drain = open.begin_close_v2();
        assert_eq!(
            observer.snapshot_v2().stage(),
            RuntimeMaintenanceIngressGateStageV2::Closing
        );
        drop(first);
        assert_eq!(observer.snapshot_v2().active_permit_count(), 1);
        drop(second);
        drain
            .wait_closed_until_v2(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(
            observer.snapshot_v2().stage(),
            RuntimeMaintenanceIngressGateStageV2::Closed
        );
        assert_eq!(observer.snapshot_v2().active_permit_count(), 0);
    }

    #[tokio::test]
    async fn canceled_waiter_does_not_change_the_counted_gate_state() {
        let (controller, observer, _shutdown) = RuntimeMaintenanceIngressGateControllerV2::new_v2();
        let open = controller
            .begin_open_v2()
            .unwrap()
            .commit_open_v2()
            .unwrap();
        let permit = open.try_acquire_v2().unwrap();
        let drain = open.begin_close_v2();
        let wait = drain.wait_closed_until_v2(Instant::now() + Duration::from_secs(1));
        tokio::pin!(wait);
        tokio::select! {
            result = &mut wait => panic!("drain completed unexpectedly: {result:?}"),
            _ = tokio::task::yield_now() => {}
        }
        drop(wait);
        assert_eq!(
            observer.snapshot_v2().stage(),
            RuntimeMaintenanceIngressGateStageV2::Closing
        );
        assert_eq!(observer.snapshot_v2().active_permit_count(), 1);
        drop(permit);
        assert_eq!(
            observer.snapshot_v2().stage(),
            RuntimeMaintenanceIngressGateStageV2::Closed
        );
    }

    #[test]
    fn shutdown_handle_closes_opening_and_open_states_synchronously() {
        let (controller, observer, shutdown) = RuntimeMaintenanceIngressGateControllerV2::new_v2();
        let opening = controller.begin_open_v2().unwrap();
        let snapshot = shutdown.close_v2();
        assert_eq!(
            snapshot.stage(),
            RuntimeMaintenanceIngressGateStageV2::Closed
        );
        assert_eq!(snapshot.generation().get(), 3);
        let failure = opening.commit_open_v2().unwrap_err();
        assert_eq!(
            failure.error(),
            RuntimeMaintenanceIngressGateErrorV2::StaleAuthority
        );
        let drain = failure.into_state().begin_close_v2();
        assert_eq!(drain.generation().get(), 3);
        assert!(drain.into_controller_v2().is_ok());
        assert_eq!(
            observer.snapshot_v2().stage(),
            RuntimeMaintenanceIngressGateStageV2::Closed
        );
    }

    #[test]
    fn concurrent_open_commit_and_shutdown_finish_closed_once() {
        let (controller, observer, shutdown) = RuntimeMaintenanceIngressGateControllerV2::new_v2();
        let opening = controller.begin_open_v2().unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let opening_barrier = barrier.clone();
        let opening_task = std::thread::spawn(move || {
            opening_barrier.wait();
            opening.commit_open_v2()
        });
        let shutdown_barrier = barrier.clone();
        let shutdown_task = std::thread::spawn(move || {
            shutdown_barrier.wait();
            shutdown.close_v2()
        });
        barrier.wait();
        drop(opening_task.join().unwrap());
        let shutdown_snapshot = shutdown_task.join().unwrap();
        let snapshot = observer.snapshot_v2();
        assert_eq!(
            snapshot.stage(),
            RuntimeMaintenanceIngressGateStageV2::Closed
        );
        assert_eq!(snapshot.generation().get(), 3);
        assert_eq!(snapshot.active_permit_count(), 0);
        assert_eq!(snapshot.terminal_error(), None);
        assert_eq!(
            shutdown_snapshot.stage(),
            RuntimeMaintenanceIngressGateStageV2::Closed
        );
    }

    #[test]
    fn generation_overflow_latches_a_closed_terminal_gate() {
        let generation = RuntimeMaintenanceGateGenerationV2::new(
            std::num::NonZeroU64::new(i64::MAX as u64).unwrap(),
        )
        .unwrap();
        let snapshot = RuntimeMaintenanceIngressGateSnapshotV2 {
            generation,
            stage: RuntimeMaintenanceIngressGateStageV2::Closed,
            active_permit_count: 0,
            terminal_error: None,
        };
        let (changes, observer) = watch::channel(snapshot);
        let shared = Arc::new(RuntimeMaintenanceIngressGateSharedV2 {
            state: Mutex::new(RuntimeMaintenanceIngressGateStateV2 {
                snapshot,
                open_generation: None,
            }),
            changes,
        });
        let controller = RuntimeMaintenanceIngressGateControllerV2 { shared, generation };
        let failure = controller.begin_open_v2().unwrap_err();
        assert_eq!(
            failure.error(),
            RuntimeMaintenanceIngressGateErrorV2::GenerationOverflow
        );
        let snapshot = *observer.borrow();
        assert_eq!(
            snapshot.stage(),
            RuntimeMaintenanceIngressGateStageV2::Closed
        );
        assert_eq!(
            snapshot.terminal_error(),
            Some(RuntimeMaintenanceIngressGateErrorV2::GenerationOverflow)
        );
    }

    #[test]
    fn permit_overflow_closes_ingress_and_latches_the_failure() {
        let (controller, observer, _shutdown) = RuntimeMaintenanceIngressGateControllerV2::new_v2();
        let open = controller
            .begin_open_v2()
            .unwrap()
            .commit_open_v2()
            .unwrap();
        {
            let shared = open.shared.as_ref().unwrap();
            let mut state = lock_state_v2(shared);
            state.snapshot.active_permit_count = i64::MAX as u64;
            shared.changes.send_replace(state.snapshot);
        }
        assert_eq!(
            open.try_acquire_v2().unwrap_err(),
            RuntimeMaintenanceIngressGateErrorV2::PermitCountOverflow
        );
        let snapshot = observer.snapshot_v2();
        assert_eq!(
            snapshot.stage(),
            RuntimeMaintenanceIngressGateStageV2::Closing
        );
        assert_eq!(snapshot.generation().get(), 3);
        assert_eq!(
            snapshot.terminal_error(),
            Some(RuntimeMaintenanceIngressGateErrorV2::PermitCountOverflow)
        );
        assert_eq!(
            open.try_acquire_v2().unwrap_err(),
            RuntimeMaintenanceIngressGateErrorV2::PermitCountOverflow
        );
    }

    #[test]
    fn poisoned_state_closes_before_an_existing_permit_can_drain() {
        let (controller, observer, shutdown) = RuntimeMaintenanceIngressGateControllerV2::new_v2();
        let open = controller
            .begin_open_v2()
            .unwrap()
            .commit_open_v2()
            .unwrap();
        let permit = open.try_acquire_v2().unwrap();
        let shared = open.shared.as_ref().unwrap().clone();
        let poisoning = std::thread::spawn(move || {
            let _state = shared.state.lock().unwrap();
            panic!("poison gate state");
        });
        assert!(poisoning.join().is_err());
        let snapshot = shutdown.close_v2();
        assert_eq!(
            snapshot.stage(),
            RuntimeMaintenanceIngressGateStageV2::Closing
        );
        assert_eq!(
            snapshot.terminal_error(),
            Some(RuntimeMaintenanceIngressGateErrorV2::Poisoned)
        );
        assert_eq!(
            open.try_acquire_v2().unwrap_err(),
            RuntimeMaintenanceIngressGateErrorV2::Poisoned
        );
        drop(permit);
        let snapshot = observer.snapshot_v2();
        assert_eq!(
            snapshot.stage(),
            RuntimeMaintenanceIngressGateStageV2::Closed
        );
        assert_eq!(snapshot.active_permit_count(), 0);
    }

    #[test]
    fn debug_and_error_surfaces_are_finite_and_redacted() {
        let (controller, observer, shutdown) = RuntimeMaintenanceIngressGateControllerV2::new_v2();
        assert_eq!(
            format!("{controller:?}"),
            "RuntimeMaintenanceIngressGateControllerV2(<redacted>)"
        );
        assert_eq!(
            format!("{observer:?}"),
            "RuntimeMaintenanceIngressGateObserverV2(<redacted>)"
        );
        assert_eq!(
            format!("{shutdown:?}"),
            "RuntimeMaintenanceIngressGateShutdownHandleV2(<redacted>)"
        );
        for error in [
            RuntimeMaintenanceIngressGateErrorV2::StaleAuthority,
            RuntimeMaintenanceIngressGateErrorV2::NotClosed,
            RuntimeMaintenanceIngressGateErrorV2::NotOpening,
            RuntimeMaintenanceIngressGateErrorV2::NotOpen,
            RuntimeMaintenanceIngressGateErrorV2::StillDraining,
            RuntimeMaintenanceIngressGateErrorV2::GenerationOverflow,
            RuntimeMaintenanceIngressGateErrorV2::PermitCountOverflow,
            RuntimeMaintenanceIngressGateErrorV2::DrainDeadlineElapsed,
            RuntimeMaintenanceIngressGateErrorV2::ObserverStopped,
            RuntimeMaintenanceIngressGateErrorV2::Poisoned,
        ] {
            assert!(!error.code().is_empty());
        }
    }
}
