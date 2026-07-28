use std::fmt::{Debug, Formatter};
use std::num::NonZeroU64;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use tokio::signal::unix::{signal, Signal, SignalKind};
use tokio::sync::watch;

const RUNTIME_SHUTDOWN_WINDOW: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeShutdownCauseV1 {
    Interrupt,
    Terminate,
    Explicit,
    SupervisorFailure,
    GatewayOwnerTerminal,
    DiscordTerminal,
    ReadinessLost,
}

impl RuntimeShutdownCauseV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Interrupt => "interrupt",
            Self::Terminate => "terminate",
            Self::Explicit => "explicit",
            Self::SupervisorFailure => "supervisor_failure",
            Self::GatewayOwnerTerminal => "gateway_owner_terminal",
            Self::DiscordTerminal => "discord_terminal",
            Self::ReadinessLost => "readiness_lost",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RuntimeShutdownObservationV1 {
    cause: RuntimeShutdownCauseV1,
    generation: NonZeroU64,
    received_at: Instant,
    deadline: Instant,
}

impl RuntimeShutdownObservationV1 {
    pub fn cause(self) -> RuntimeShutdownCauseV1 {
        self.cause
    }

    pub fn generation(self) -> NonZeroU64 {
        self.generation
    }

    pub fn received_at(self) -> Instant {
        self.received_at
    }

    pub fn deadline(self) -> Instant {
        self.deadline
    }

    pub fn remaining_at(self, now: Instant) -> Duration {
        self.deadline
            .checked_duration_since(now)
            .unwrap_or(Duration::ZERO)
    }
}

impl Debug for RuntimeShutdownObservationV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeShutdownObservationV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeShutdownTripV1 {
    First(RuntimeShutdownObservationV1),
    Existing(RuntimeShutdownObservationV1),
}

impl RuntimeShutdownTripV1 {
    pub fn observation(self) -> RuntimeShutdownObservationV1 {
        match self {
            Self::First(observation) | Self::Existing(observation) => observation,
        }
    }

    pub const fn first(self) -> bool {
        matches!(self, Self::First(_))
    }
}

struct RuntimeShutdownLatchStateV1 {
    observation: OnceLock<RuntimeShutdownObservationV1>,
    publisher: watch::Sender<Option<RuntimeShutdownObservationV1>>,
    deadline_ceiling: Option<Instant>,
}

impl Debug for RuntimeShutdownLatchStateV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeShutdownLatchStateV1(<redacted>)")
    }
}

#[derive(Clone)]
pub struct RuntimeShutdownTriggerV1 {
    state: Arc<RuntimeShutdownLatchStateV1>,
}

impl RuntimeShutdownTriggerV1 {
    pub fn trip(&self, cause: RuntimeShutdownCauseV1) -> RuntimeShutdownTripV1 {
        self.trip_at(cause, Instant::now())
    }

    fn trip_at(
        &self,
        cause: RuntimeShutdownCauseV1,
        received_at: Instant,
    ) -> RuntimeShutdownTripV1 {
        let deadline = received_at
            .checked_add(RUNTIME_SHUTDOWN_WINDOW)
            .unwrap_or(received_at);
        let deadline = self
            .state
            .deadline_ceiling
            .map_or(deadline, |ceiling| deadline.min(ceiling));
        let candidate = RuntimeShutdownObservationV1 {
            cause,
            generation: NonZeroU64::MIN,
            received_at,
            deadline,
        };
        match self.state.observation.set(candidate) {
            Ok(()) => {
                self.state.publisher.send_replace(Some(candidate));
                RuntimeShutdownTripV1::First(candidate)
            }
            Err(_) => {
                RuntimeShutdownTripV1::Existing(*self.state.observation.get().unwrap_or(&candidate))
            }
        }
    }

    pub fn observed(&self) -> Option<RuntimeShutdownObservationV1> {
        self.state.observation.get().copied()
    }
}

impl Debug for RuntimeShutdownTriggerV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeShutdownTriggerV1(<redacted>)")
    }
}

pub struct RuntimeShutdownSignalLatchV1 {
    state: Arc<RuntimeShutdownLatchStateV1>,
    receiver: watch::Receiver<Option<RuntimeShutdownObservationV1>>,
}

impl RuntimeShutdownSignalLatchV1 {
    pub fn create() -> Self {
        Self::create_with_deadline_ceiling(None)
    }

    pub fn create_startup_bounded(cleanup_deadline: Instant) -> Self {
        Self::create_with_deadline_ceiling(Some(cleanup_deadline))
    }

    fn create_with_deadline_ceiling(deadline_ceiling: Option<Instant>) -> Self {
        let (publisher, receiver) = watch::channel(None);
        let state = Arc::new(RuntimeShutdownLatchStateV1 {
            observation: OnceLock::new(),
            publisher,
            deadline_ceiling,
        });
        Self { state, receiver }
    }

    pub fn trigger(&self) -> RuntimeShutdownTriggerV1 {
        RuntimeShutdownTriggerV1 {
            state: self.state.clone(),
        }
    }

    pub fn observed(&self) -> Option<RuntimeShutdownObservationV1> {
        self.state.observation.get().copied()
    }

    pub async fn wait(&mut self) -> RuntimeShutdownObservationV1 {
        loop {
            if let Some(observation) = *self.receiver.borrow_and_update() {
                return observation;
            }
            if self.receiver.changed().await.is_err() {
                if let Some(observation) = self.state.observation.get().copied() {
                    return observation;
                }
                std::future::pending::<()>().await;
            }
        }
    }
}

impl Debug for RuntimeShutdownSignalLatchV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeShutdownSignalLatchV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeShutdownSignalErrorV1 {
    #[error("runtime shutdown signal registration failed")]
    Registration,
    #[error("runtime shutdown signal stream closed")]
    StreamClosed,
}

impl RuntimeShutdownSignalErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Registration => "runtime_shutdown_signal_registration",
            Self::StreamClosed => "runtime_shutdown_signal_stream_closed",
        }
    }
}

pub struct RuntimeOsShutdownSignalsV1 {
    interrupt: Option<Signal>,
    terminate: Option<Signal>,
}

impl RuntimeOsShutdownSignalsV1 {
    pub fn register() -> Result<Self, RuntimeShutdownSignalErrorV1> {
        Ok(Self {
            interrupt: Some(
                signal(SignalKind::interrupt())
                    .map_err(|_| RuntimeShutdownSignalErrorV1::Registration)?,
            ),
            terminate: Some(
                signal(SignalKind::terminate())
                    .map_err(|_| RuntimeShutdownSignalErrorV1::Registration)?,
            ),
        })
    }

    pub async fn wait_and_trip(
        &mut self,
        trigger: &RuntimeShutdownTriggerV1,
    ) -> Result<RuntimeShutdownTripV1, RuntimeShutdownSignalErrorV1> {
        loop {
            match (&mut self.interrupt, &mut self.terminate) {
                (Some(interrupt), Some(terminate)) => {
                    tokio::select! {
                        biased;
                        received = terminate.recv() => {
                            if received.is_some() {
                                return Ok(trigger.trip(RuntimeShutdownCauseV1::Terminate));
                            }
                            self.terminate = None;
                        }
                        received = interrupt.recv() => {
                            if received.is_some() {
                                return Ok(trigger.trip(RuntimeShutdownCauseV1::Interrupt));
                            }
                            self.interrupt = None;
                        }
                    }
                }
                (Some(interrupt), None) => {
                    if interrupt.recv().await.is_some() {
                        return Ok(trigger.trip(RuntimeShutdownCauseV1::Interrupt));
                    }
                    self.interrupt = None;
                }
                (None, Some(terminate)) => {
                    if terminate.recv().await.is_some() {
                        return Ok(trigger.trip(RuntimeShutdownCauseV1::Terminate));
                    }
                    self.terminate = None;
                }
                (None, None) => return Err(RuntimeShutdownSignalErrorV1::StreamClosed),
            }
        }
    }
}

impl Debug for RuntimeOsShutdownSignalsV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeOsShutdownSignalsV1(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Barrier;
    use std::thread;

    use super::*;

    #[test]
    fn first_cause_and_absolute_deadline_are_immutable() {
        let latch = RuntimeShutdownSignalLatchV1::create();
        let trigger = latch.trigger();
        let first_at = Instant::now();
        let first = trigger.trip_at(RuntimeShutdownCauseV1::DiscordTerminal, first_at);
        let later = trigger.trip_at(
            RuntimeShutdownCauseV1::Interrupt,
            first_at + Duration::from_secs(20),
        );

        assert!(first.first());
        assert!(!later.first());
        assert_eq!(first.observation(), later.observation());
        assert_eq!(
            first.observation().deadline(),
            first_at + RUNTIME_SHUTDOWN_WINDOW
        );
        assert_eq!(latch.observed(), Some(first.observation()));
        assert_eq!(
            first
                .observation()
                .remaining_at(first_at + Duration::from_secs(29)),
            Duration::from_secs(1)
        );
        assert_eq!(
            first
                .observation()
                .remaining_at(first_at + Duration::from_secs(31)),
            Duration::ZERO
        );
    }

    #[test]
    fn startup_cleanup_deadline_can_only_tighten_the_shutdown_window() {
        let received_at = Instant::now();
        let cleanup_deadline = received_at + Duration::from_secs(7);
        let bounded = RuntimeShutdownSignalLatchV1::create_startup_bounded(cleanup_deadline);
        let bounded_observation = bounded
            .trigger()
            .trip_at(RuntimeShutdownCauseV1::SupervisorFailure, received_at);
        let elapsed = RuntimeShutdownSignalLatchV1::create_startup_bounded(
            received_at - Duration::from_secs(1),
        );
        let elapsed_observation = elapsed
            .trigger()
            .trip_at(RuntimeShutdownCauseV1::Terminate, received_at);

        assert_eq!(
            bounded_observation.observation().deadline(),
            cleanup_deadline
        );
        assert_eq!(
            elapsed_observation.observation().remaining_at(received_at),
            Duration::ZERO
        );
    }

    #[test]
    fn concurrent_trip_has_exactly_one_winner_and_one_generation() {
        let trigger = RuntimeShutdownSignalLatchV1::create().trigger();
        let barrier = Arc::new(Barrier::new(9));
        let mut threads = Vec::new();
        for index in 0..8 {
            let trigger = trigger.clone();
            let barrier = barrier.clone();
            threads.push(thread::spawn(move || {
                barrier.wait();
                trigger.trip(if index % 2 == 0 {
                    RuntimeShutdownCauseV1::SupervisorFailure
                } else {
                    RuntimeShutdownCauseV1::ReadinessLost
                })
            }));
        }
        barrier.wait();
        let outcomes = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(outcomes.iter().filter(|outcome| outcome.first()).count(), 1);
        let observation = trigger.observed().unwrap();
        assert_eq!(observation.generation(), NonZeroU64::MIN);
        assert!(outcomes
            .iter()
            .all(|outcome| outcome.observation() == observation));
    }

    #[tokio::test]
    async fn every_subscriber_observes_the_same_latched_event() {
        let mut first_latch = RuntimeShutdownSignalLatchV1::create();
        let trigger = first_latch.trigger();
        let mut second_latch = RuntimeShutdownSignalLatchV1 {
            state: first_latch.state.clone(),
            receiver: first_latch.receiver.clone(),
        };
        let first_wait = first_latch.wait();
        let second_wait = second_latch.wait();
        let trip = trigger.trip(RuntimeShutdownCauseV1::Explicit);
        let (first, second) = tokio::join!(first_wait, second_wait);

        assert_eq!(first, trip.observation());
        assert_eq!(second, trip.observation());
    }

    #[test]
    fn diagnostics_are_finite_and_redacted() {
        let latch = RuntimeShutdownSignalLatchV1::create();
        let trigger = latch.trigger();
        let observation = trigger.trip(RuntimeShutdownCauseV1::GatewayOwnerTerminal);

        assert_eq!(
            RuntimeShutdownCauseV1::GatewayOwnerTerminal.code(),
            "gateway_owner_terminal"
        );
        assert_eq!(
            RuntimeShutdownSignalErrorV1::Registration.code(),
            "runtime_shutdown_signal_registration"
        );
        assert_eq!(
            format!("{:?}", observation.observation()),
            "RuntimeShutdownObservationV1(<redacted>)"
        );
        assert_eq!(
            format!("{trigger:?}"),
            "RuntimeShutdownTriggerV1(<redacted>)"
        );
        assert_eq!(
            format!("{latch:?}"),
            "RuntimeShutdownSignalLatchV1(<redacted>)"
        );
    }
}
