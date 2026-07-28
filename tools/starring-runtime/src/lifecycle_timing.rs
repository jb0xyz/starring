use std::array;
use std::fmt::{Debug, Formatter};
use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::{RuntimeShutdownCauseV1, RuntimeShutdownObservationV1};

const RUNTIME_LIFECYCLE_TIMING_DURATION_MASK_V2: u64 = (1_u64 << 56) - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub(crate) enum RuntimeLifecycleTimingMetricV2 {
    ShutdownTripToReadinessSeal = 0,
    ShutdownTripToMaintenanceIngressSeal = 1,
    ShutdownTripToGatewayProjection = 2,
    RecoveryResumeClaimToExactReady = 3,
    ExactReadyToDurableAcknowledgementTerminal = 4,
    ShutdownFinalizerJoin = 5,
    ShutdownIngressAcknowledgementJoin = 6,
    ShutdownCapabilityReadinessJoin = 7,
    ShutdownRegistryObservation = 8,
    ShutdownGatewayDrainJoin = 9,
    ShutdownOwnerJoin = 10,
    ShutdownRootSignalJoin = 11,
    ShutdownDatabasePoolsClose = 12,
    ShutdownHealthStop = 13,
    ShutdownTotal = 14,
}

impl RuntimeLifecycleTimingMetricV2 {
    pub(crate) const COUNT: usize = 15;

    const ALL: [Self; Self::COUNT] = [
        Self::ShutdownTripToReadinessSeal,
        Self::ShutdownTripToMaintenanceIngressSeal,
        Self::ShutdownTripToGatewayProjection,
        Self::RecoveryResumeClaimToExactReady,
        Self::ExactReadyToDurableAcknowledgementTerminal,
        Self::ShutdownFinalizerJoin,
        Self::ShutdownIngressAcknowledgementJoin,
        Self::ShutdownCapabilityReadinessJoin,
        Self::ShutdownRegistryObservation,
        Self::ShutdownGatewayDrainJoin,
        Self::ShutdownOwnerJoin,
        Self::ShutdownRootSignalJoin,
        Self::ShutdownDatabasePoolsClose,
        Self::ShutdownHealthStop,
        Self::ShutdownTotal,
    ];

    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::ShutdownTripToReadinessSeal => "shutdown_trip_to_readiness_seal",
            Self::ShutdownTripToMaintenanceIngressSeal => {
                "shutdown_trip_to_maintenance_ingress_seal"
            }
            Self::ShutdownTripToGatewayProjection => "shutdown_trip_to_gateway_projection",
            Self::RecoveryResumeClaimToExactReady => "recovery_resume_claim_to_exact_ready",
            Self::ExactReadyToDurableAcknowledgementTerminal => {
                "exact_ready_to_durable_acknowledgement_terminal"
            }
            Self::ShutdownFinalizerJoin => "shutdown_finalizer_join",
            Self::ShutdownIngressAcknowledgementJoin => "shutdown_ingress_acknowledgement_join",
            Self::ShutdownCapabilityReadinessJoin => "shutdown_capability_readiness_join",
            Self::ShutdownRegistryObservation => "shutdown_registry_observation",
            Self::ShutdownGatewayDrainJoin => "shutdown_gateway_drain_join",
            Self::ShutdownOwnerJoin => "shutdown_owner_join",
            Self::ShutdownRootSignalJoin => "shutdown_root_signal_join",
            Self::ShutdownDatabasePoolsClose => "shutdown_database_pools_close",
            Self::ShutdownHealthStop => "shutdown_health_stop",
            Self::ShutdownTotal => "shutdown_total",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum RuntimeLifecycleTimingOutcomeV2 {
    Completed = 1,
    Rejected = 2,
    FailedClosed = 3,
    DeadlineElapsed = 4,
    Abandoned = 5,
    Skipped = 6,
}

impl RuntimeLifecycleTimingOutcomeV2 {
    const fn code_v2(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Rejected => "rejected",
            Self::FailedClosed => "failed_closed",
            Self::DeadlineElapsed => "deadline_elapsed",
            Self::Abandoned => "abandoned",
            Self::Skipped => "skipped",
        }
    }

    fn from_code_v2(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Completed),
            2 => Some(Self::Rejected),
            3 => Some(Self::FailedClosed),
            4 => Some(Self::DeadlineElapsed),
            5 => Some(Self::Abandoned),
            6 => Some(Self::Skipped),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeLifecycleShutdownSourceV2 {
    Direct,
    SignalInterrupt,
    SignalTerminate,
    Supervisor,
}

impl RuntimeLifecycleShutdownSourceV2 {
    const fn from_cause_v2(cause: RuntimeShutdownCauseV1) -> Self {
        match cause {
            RuntimeShutdownCauseV1::Explicit => Self::Direct,
            RuntimeShutdownCauseV1::Interrupt => Self::SignalInterrupt,
            RuntimeShutdownCauseV1::Terminate => Self::SignalTerminate,
            RuntimeShutdownCauseV1::SupervisorFailure
            | RuntimeShutdownCauseV1::FinalizerTerminal
            | RuntimeShutdownCauseV1::HealthTerminal
            | RuntimeShutdownCauseV1::IngressAcknowledgementTerminal
            | RuntimeShutdownCauseV1::GatewayOwnerTerminal
            | RuntimeShutdownCauseV1::DiscordTerminal
            | RuntimeShutdownCauseV1::ReadinessLost => Self::Supervisor,
        }
    }

    const fn code_v2(self) -> u8 {
        match self {
            Self::Direct => 1,
            Self::SignalInterrupt => 2,
            Self::SignalTerminate => 3,
            Self::Supervisor => 4,
        }
    }

    const fn label_v2(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::SignalInterrupt => "signal_interrupt",
            Self::SignalTerminate => "signal_terminate",
            Self::Supervisor => "supervisor",
        }
    }

    fn from_code_v2(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Direct),
            2 => Some(Self::SignalInterrupt),
            3 => Some(Self::SignalTerminate),
            4 => Some(Self::Supervisor),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeLifecycleTimingSampleV2 {
    elapsed: Duration,
    outcome: RuntimeLifecycleTimingOutcomeV2,
}

impl RuntimeLifecycleTimingSampleV2 {
    pub(crate) const fn elapsed(self) -> Duration {
        self.elapsed
    }

    pub(crate) const fn outcome(self) -> RuntimeLifecycleTimingOutcomeV2 {
        self.outcome
    }
}

struct RuntimeLifecycleTimingStateV2 {
    samples: [AtomicU64; RuntimeLifecycleTimingMetricV2::COUNT],
    shutdown_source: AtomicU8,
    terminal_emission_count: AtomicU8,
    shutdown_started_at: OnceLock<Instant>,
    recovery_resume_claimed_at: Mutex<Option<Instant>>,
    exact_ready_at: OnceLock<Instant>,
}

#[derive(Clone)]
pub(crate) struct RuntimeLifecycleTimingRecorderV2 {
    state: Arc<RuntimeLifecycleTimingStateV2>,
}

#[derive(Clone)]
pub(crate) struct RuntimeLifecycleTimingObserverV2 {
    state: Arc<RuntimeLifecycleTimingStateV2>,
}

pub(crate) struct RuntimeLifecycleTimingSpanV2 {
    recorder: RuntimeLifecycleTimingRecorderV2,
    metric: RuntimeLifecycleTimingMetricV2,
    started_at: Instant,
    completed: bool,
}

pub(crate) struct RuntimeLifecycleTimingTerminalReporterV2 {
    recorder: RuntimeLifecycleTimingRecorderV2,
    observer: RuntimeLifecycleTimingObserverV2,
    completed: bool,
}

pub(crate) struct RuntimeLifecycleTimingSnapshotV2 {
    samples: [Option<RuntimeLifecycleTimingSampleV2>; RuntimeLifecycleTimingMetricV2::COUNT],
    shutdown_source: Option<RuntimeLifecycleShutdownSourceV2>,
}

impl RuntimeLifecycleTimingRecorderV2 {
    pub(crate) fn create_v2() -> (Self, RuntimeLifecycleTimingObserverV2) {
        let state = Arc::new(RuntimeLifecycleTimingStateV2 {
            samples: array::from_fn(|_| AtomicU64::new(0)),
            shutdown_source: AtomicU8::new(0),
            terminal_emission_count: AtomicU8::new(0),
            shutdown_started_at: OnceLock::new(),
            recovery_resume_claimed_at: Mutex::new(None),
            exact_ready_at: OnceLock::new(),
        });
        (
            Self {
                state: Arc::clone(&state),
            },
            RuntimeLifecycleTimingObserverV2 { state },
        )
    }

    pub(crate) fn observe_shutdown_v2(&self, observation: RuntimeShutdownObservationV1) {
        let source = RuntimeLifecycleShutdownSourceV2::from_cause_v2(observation.cause());
        let _ = self.state.shutdown_source.compare_exchange(
            0,
            source.code_v2(),
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        let _ = self
            .state
            .shutdown_started_at
            .set(observation.received_at());
    }

    pub(crate) fn record_shutdown_projection_v2(
        &self,
        metric: RuntimeLifecycleTimingMetricV2,
        observation: RuntimeShutdownObservationV1,
        outcome: RuntimeLifecycleTimingOutcomeV2,
    ) {
        self.observe_shutdown_v2(observation);
        self.record_elapsed_v2(metric, observation.received_at(), outcome);
    }

    pub(crate) fn record_recovery_resume_claim_v2(&self) {
        self.record_recovery_resume_claim_at_v2(Instant::now());
    }

    fn record_recovery_resume_claim_at_v2(&self, observed_at: Instant) {
        let mut claimed_at = self
            .state
            .recovery_resume_claimed_at
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *claimed_at = Some(observed_at);
    }

    pub(crate) fn abandon_recovery_resume_claim_v2(&self) {
        let mut claimed_at = self
            .state
            .recovery_resume_claimed_at
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *claimed_at = None;
    }

    pub(crate) fn record_exact_ready_v2(&self) {
        self.record_exact_ready_at_v2(Instant::now());
    }

    fn record_exact_ready_at_v2(&self, observed_at: Instant) {
        if self.state.exact_ready_at.set(observed_at).is_err() {
            return;
        }
        let claimed_at = self
            .state
            .recovery_resume_claimed_at
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(claimed_at) = claimed_at {
            self.record_elapsed_at_v2(
                RuntimeLifecycleTimingMetricV2::RecoveryResumeClaimToExactReady,
                claimed_at,
                observed_at,
                RuntimeLifecycleTimingOutcomeV2::Completed,
            );
        }
    }

    pub(crate) fn record_durable_acknowledgement_terminal_v2(
        &self,
        outcome: RuntimeLifecycleTimingOutcomeV2,
    ) {
        if let Some(ready_at) = self.state.exact_ready_at.get().copied() {
            self.record_elapsed_v2(
                RuntimeLifecycleTimingMetricV2::ExactReadyToDurableAcknowledgementTerminal,
                ready_at,
                outcome,
            );
        }
    }

    pub(crate) fn start_span_v2(
        &self,
        metric: RuntimeLifecycleTimingMetricV2,
    ) -> RuntimeLifecycleTimingSpanV2 {
        RuntimeLifecycleTimingSpanV2 {
            recorder: self.clone(),
            metric,
            started_at: Instant::now(),
            completed: false,
        }
    }

    pub(crate) fn record_skipped_v2(&self, metric: RuntimeLifecycleTimingMetricV2) {
        self.record_duration_v2(
            metric,
            Duration::ZERO,
            RuntimeLifecycleTimingOutcomeV2::Skipped,
        );
    }

    pub(crate) fn finish_shutdown_total_v2(&self, outcome: RuntimeLifecycleTimingOutcomeV2) {
        if let Some(started_at) = self.state.shutdown_started_at.get().copied() {
            self.record_elapsed_v2(
                RuntimeLifecycleTimingMetricV2::ShutdownTotal,
                started_at,
                outcome,
            );
        } else {
            self.record_duration_v2(
                RuntimeLifecycleTimingMetricV2::ShutdownTotal,
                Duration::ZERO,
                outcome,
            );
        }
    }

    fn record_elapsed_v2(
        &self,
        metric: RuntimeLifecycleTimingMetricV2,
        started_at: Instant,
        outcome: RuntimeLifecycleTimingOutcomeV2,
    ) {
        self.record_elapsed_at_v2(metric, started_at, Instant::now(), outcome);
    }

    fn record_elapsed_at_v2(
        &self,
        metric: RuntimeLifecycleTimingMetricV2,
        started_at: Instant,
        completed_at: Instant,
        outcome: RuntimeLifecycleTimingOutcomeV2,
    ) {
        self.record_duration_v2(
            metric,
            completed_at.saturating_duration_since(started_at),
            outcome,
        );
    }

    fn record_duration_v2(
        &self,
        metric: RuntimeLifecycleTimingMetricV2,
        elapsed: Duration,
        outcome: RuntimeLifecycleTimingOutcomeV2,
    ) {
        let elapsed_nanos = elapsed
            .as_nanos()
            .min(RUNTIME_LIFECYCLE_TIMING_DURATION_MASK_V2 as u128)
            as u64;
        let encoded = ((outcome as u64) << 56) | elapsed_nanos;
        let _ = self.state.samples[metric as usize].compare_exchange(
            0,
            encoded,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

impl RuntimeLifecycleTimingSpanV2 {
    pub(crate) fn finish_v2(mut self, outcome: RuntimeLifecycleTimingOutcomeV2) {
        self.recorder
            .record_elapsed_v2(self.metric, self.started_at, outcome);
        self.completed = true;
    }
}

impl Drop for RuntimeLifecycleTimingSpanV2 {
    fn drop(&mut self) {
        if !self.completed {
            self.recorder.record_elapsed_v2(
                self.metric,
                self.started_at,
                RuntimeLifecycleTimingOutcomeV2::Abandoned,
            );
        }
    }
}

impl RuntimeLifecycleTimingObserverV2 {
    pub(crate) fn snapshot_v2(&self) -> RuntimeLifecycleTimingSnapshotV2 {
        RuntimeLifecycleTimingSnapshotV2 {
            samples: array::from_fn(|index| {
                decode_runtime_lifecycle_timing_sample_v2(
                    self.state.samples[index].load(Ordering::Acquire),
                )
            }),
            shutdown_source: RuntimeLifecycleShutdownSourceV2::from_code_v2(
                self.state.shutdown_source.load(Ordering::Acquire),
            ),
        }
    }

    fn emit_terminal_snapshot_v2(&self) {
        if self
            .state
            .terminal_emission_count
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        #[cfg(not(test))]
        {
            let snapshot = self.snapshot_v2();
            let mut stderr = std::io::stderr().lock();
            let _ = snapshot.write_v2(&mut stderr);
        }
    }

    #[cfg(test)]
    pub(crate) fn terminal_emission_count_v2(&self) -> u8 {
        self.state.terminal_emission_count.load(Ordering::Acquire)
    }
}

impl RuntimeLifecycleTimingSnapshotV2 {
    pub(crate) const fn sample_v2(
        &self,
        metric: RuntimeLifecycleTimingMetricV2,
    ) -> Option<RuntimeLifecycleTimingSampleV2> {
        self.samples[metric as usize]
    }

    #[cfg(test)]
    pub(crate) const fn shutdown_source_v2(&self) -> Option<RuntimeLifecycleShutdownSourceV2> {
        self.shutdown_source
    }

    fn write_v2(&self, output: &mut impl Write) -> io::Result<()> {
        write!(
            output,
            "starring_runtime_lifecycle_timing_v2 source={}",
            self.shutdown_source
                .map(RuntimeLifecycleShutdownSourceV2::label_v2)
                .unwrap_or("unobserved")
        )?;
        for metric in RuntimeLifecycleTimingMetricV2::ALL {
            write!(output, " {}=", metric.code())?;
            match self.sample_v2(metric) {
                Some(sample) => write!(
                    output,
                    "{}:{}ns",
                    sample.outcome().code_v2(),
                    sample.elapsed().as_nanos()
                )?,
                None => output.write_all(b"missing")?,
            }
        }
        writeln!(output)
    }

    fn merge_terminal_outcome_v2(
        &self,
        outer: RuntimeLifecycleTimingOutcomeV2,
    ) -> RuntimeLifecycleTimingOutcomeV2 {
        let phase_outcome = RuntimeLifecycleTimingMetricV2::ALL
            .into_iter()
            .filter_map(|metric| self.sample_v2(metric))
            .fold(
                RuntimeLifecycleTimingOutcomeV2::Completed,
                |current, sample| merge_terminal_outcome_v2(current, sample.outcome()),
            );
        merge_terminal_outcome_v2(phase_outcome, outer)
    }
}

fn merge_terminal_outcome_v2(
    current: RuntimeLifecycleTimingOutcomeV2,
    next: RuntimeLifecycleTimingOutcomeV2,
) -> RuntimeLifecycleTimingOutcomeV2 {
    match (current, next) {
        (RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed, _)
        | (_, RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed) => {
            RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed
        }
        (RuntimeLifecycleTimingOutcomeV2::Abandoned, _)
        | (_, RuntimeLifecycleTimingOutcomeV2::Abandoned) => {
            RuntimeLifecycleTimingOutcomeV2::Abandoned
        }
        (
            RuntimeLifecycleTimingOutcomeV2::Rejected
            | RuntimeLifecycleTimingOutcomeV2::FailedClosed,
            _,
        )
        | (
            _,
            RuntimeLifecycleTimingOutcomeV2::Rejected
            | RuntimeLifecycleTimingOutcomeV2::FailedClosed,
        ) => RuntimeLifecycleTimingOutcomeV2::FailedClosed,
        _ => RuntimeLifecycleTimingOutcomeV2::Completed,
    }
}

fn decode_runtime_lifecycle_timing_sample_v2(
    encoded: u64,
) -> Option<RuntimeLifecycleTimingSampleV2> {
    if encoded == 0 {
        return None;
    }
    let outcome = RuntimeLifecycleTimingOutcomeV2::from_code_v2((encoded >> 56) as u8)?;
    let elapsed = Duration::from_nanos(encoded & RUNTIME_LIFECYCLE_TIMING_DURATION_MASK_V2);
    Some(RuntimeLifecycleTimingSampleV2 { elapsed, outcome })
}

impl RuntimeLifecycleTimingTerminalReporterV2 {
    pub(crate) fn new_v2(
        recorder: RuntimeLifecycleTimingRecorderV2,
        observer: RuntimeLifecycleTimingObserverV2,
    ) -> Self {
        Self {
            recorder,
            observer,
            completed: false,
        }
    }

    pub(crate) fn finish_v2(mut self, outer_outcome: RuntimeLifecycleTimingOutcomeV2) {
        let outcome = self
            .observer
            .snapshot_v2()
            .merge_terminal_outcome_v2(outer_outcome);
        self.recorder.finish_shutdown_total_v2(outcome);
        self.observer.emit_terminal_snapshot_v2();
        self.completed = true;
    }

    pub(crate) fn finish_result_v2<T, E>(
        self,
        result: Result<T, E>,
        outcome: RuntimeLifecycleTimingOutcomeV2,
    ) -> Result<T, E> {
        self.finish_v2(outcome);
        result
    }
}

impl Drop for RuntimeLifecycleTimingTerminalReporterV2 {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        self.recorder
            .finish_shutdown_total_v2(RuntimeLifecycleTimingOutcomeV2::Abandoned);
        self.observer.emit_terminal_snapshot_v2();
        self.completed = true;
    }
}

impl Debug for RuntimeLifecycleTimingRecorderV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeLifecycleTimingRecorderV2(<redacted>)")
    }
}

impl Debug for RuntimeLifecycleTimingObserverV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeLifecycleTimingObserverV2(<redacted>)")
    }
}

impl Debug for RuntimeLifecycleTimingSpanV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeLifecycleTimingSpanV2(<redacted>)")
    }
}

impl Debug for RuntimeLifecycleTimingTerminalReporterV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeLifecycleTimingTerminalReporterV2(<redacted>)")
    }
}

impl Debug for RuntimeLifecycleTimingSnapshotV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeLifecycleTimingSnapshotV2(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_slots_are_first_writer_wins_finite_and_redacted() {
        let (recorder, observer) = RuntimeLifecycleTimingRecorderV2::create_v2();
        let latch = crate::RuntimeShutdownSignalLatchV1::create();
        let observation = latch
            .trigger()
            .trip(crate::RuntimeShutdownCauseV1::Explicit)
            .observation();
        recorder.record_shutdown_projection_v2(
            RuntimeLifecycleTimingMetricV2::ShutdownTripToReadinessSeal,
            observation,
            RuntimeLifecycleTimingOutcomeV2::Completed,
        );
        recorder.record_skipped_v2(RuntimeLifecycleTimingMetricV2::ShutdownTripToReadinessSeal);
        recorder.finish_shutdown_total_v2(RuntimeLifecycleTimingOutcomeV2::Completed);
        let snapshot = observer.snapshot_v2();
        assert_eq!(
            snapshot
                .sample_v2(RuntimeLifecycleTimingMetricV2::ShutdownTripToReadinessSeal)
                .unwrap()
                .outcome(),
            RuntimeLifecycleTimingOutcomeV2::Completed
        );
        assert_eq!(
            snapshot.shutdown_source_v2(),
            Some(RuntimeLifecycleShutdownSourceV2::Direct)
        );
        assert!(snapshot
            .sample_v2(RuntimeLifecycleTimingMetricV2::ShutdownTotal)
            .is_some());
        assert_eq!(
            format!("{recorder:?}"),
            "RuntimeLifecycleTimingRecorderV2(<redacted>)"
        );
        assert_eq!(
            format!("{observer:?}"),
            "RuntimeLifecycleTimingObserverV2(<redacted>)"
        );
    }

    #[test]
    fn resume_and_acknowledgement_slots_correlate_without_identity() {
        let (recorder, observer) = RuntimeLifecycleTimingRecorderV2::create_v2();
        recorder.record_recovery_resume_claim_v2();
        recorder.record_exact_ready_v2();
        recorder
            .record_durable_acknowledgement_terminal_v2(RuntimeLifecycleTimingOutcomeV2::Completed);
        let snapshot = observer.snapshot_v2();
        assert!(snapshot
            .sample_v2(RuntimeLifecycleTimingMetricV2::RecoveryResumeClaimToExactReady)
            .is_some());
        assert_eq!(
            snapshot
                .sample_v2(
                    RuntimeLifecycleTimingMetricV2::ExactReadyToDurableAcknowledgementTerminal
                )
                .unwrap()
                .outcome(),
            RuntimeLifecycleTimingOutcomeV2::Completed
        );
    }

    #[test]
    fn cancelled_resume_claim_is_replaced_by_the_exact_retry_attempt() {
        let (recorder, observer) = RuntimeLifecycleTimingRecorderV2::create_v2();
        let first_claim = Instant::now();
        let retry_claim = first_claim + Duration::from_secs(9);
        let retry_ready = retry_claim + Duration::from_millis(17);
        recorder.record_recovery_resume_claim_at_v2(first_claim);
        recorder.abandon_recovery_resume_claim_v2();
        recorder.record_recovery_resume_claim_at_v2(retry_claim);
        recorder.record_exact_ready_at_v2(retry_ready);
        assert_eq!(
            observer
                .snapshot_v2()
                .sample_v2(RuntimeLifecycleTimingMetricV2::RecoveryResumeClaimToExactReady)
                .unwrap()
                .elapsed(),
            Duration::from_millis(17)
        );
    }

    #[test]
    fn terminal_snapshot_is_single_bounded_redacted_record() {
        let (recorder, observer) = RuntimeLifecycleTimingRecorderV2::create_v2();
        let latch = crate::RuntimeShutdownSignalLatchV1::create();
        let observation = latch
            .trigger()
            .trip(crate::RuntimeShutdownCauseV1::Terminate)
            .observation();
        recorder.observe_shutdown_v2(observation);
        recorder.finish_shutdown_total_v2(RuntimeLifecycleTimingOutcomeV2::Completed);
        let mut output = Vec::new();
        observer.snapshot_v2().write_v2(&mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert_eq!(output.lines().count(), 1);
        assert!(output.starts_with("starring_runtime_lifecycle_timing_v2 source=signal_terminate "));
        assert!(output.contains("shutdown_total=completed:"));
        assert!(!output.contains("controller"));
        assert!(!output.contains("process_instance"));
        assert!(output.len() < 1_500);
    }

    #[test]
    fn unfinished_span_records_abandonment() {
        let (recorder, observer) = RuntimeLifecycleTimingRecorderV2::create_v2();
        drop(recorder.start_span_v2(RuntimeLifecycleTimingMetricV2::ShutdownDatabasePoolsClose));
        assert_eq!(
            observer
                .snapshot_v2()
                .sample_v2(RuntimeLifecycleTimingMetricV2::ShutdownDatabasePoolsClose)
                .unwrap()
                .outcome(),
            RuntimeLifecycleTimingOutcomeV2::Abandoned
        );
    }

    #[test]
    fn outer_failure_controls_terminal_total_and_single_emission() {
        let (recorder, observer) = RuntimeLifecycleTimingRecorderV2::create_v2();
        let reporter = RuntimeLifecycleTimingTerminalReporterV2::new_v2(recorder, observer.clone());
        let result: Result<(), u8> =
            reporter.finish_result_v2(Err(7), RuntimeLifecycleTimingOutcomeV2::FailedClosed);
        assert_eq!(result, Err(7));
        assert_eq!(
            observer
                .snapshot_v2()
                .sample_v2(RuntimeLifecycleTimingMetricV2::ShutdownTotal)
                .unwrap()
                .outcome(),
            RuntimeLifecycleTimingOutcomeV2::FailedClosed
        );
        assert_eq!(observer.terminal_emission_count_v2(), 1);
        observer.emit_terminal_snapshot_v2();
        assert_eq!(observer.terminal_emission_count_v2(), 1);
    }

    #[test]
    fn deadline_phase_preserves_deadline_terminal_total() {
        let (recorder, observer) = RuntimeLifecycleTimingRecorderV2::create_v2();
        recorder.record_duration_v2(
            RuntimeLifecycleTimingMetricV2::ShutdownGatewayDrainJoin,
            Duration::from_millis(3),
            RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed,
        );
        RuntimeLifecycleTimingTerminalReporterV2::new_v2(recorder, observer.clone())
            .finish_v2(RuntimeLifecycleTimingOutcomeV2::Completed);
        assert_eq!(
            observer
                .snapshot_v2()
                .sample_v2(RuntimeLifecycleTimingMetricV2::ShutdownTotal)
                .unwrap()
                .outcome(),
            RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed
        );
    }

    #[test]
    fn failed_phase_prevents_a_completed_terminal_total() {
        let (recorder, observer) = RuntimeLifecycleTimingRecorderV2::create_v2();
        recorder.record_duration_v2(
            RuntimeLifecycleTimingMetricV2::ShutdownOwnerJoin,
            Duration::from_millis(2),
            RuntimeLifecycleTimingOutcomeV2::FailedClosed,
        );
        RuntimeLifecycleTimingTerminalReporterV2::new_v2(recorder, observer.clone())
            .finish_v2(RuntimeLifecycleTimingOutcomeV2::Completed);
        assert_eq!(
            observer
                .snapshot_v2()
                .sample_v2(RuntimeLifecycleTimingMetricV2::ShutdownTotal)
                .unwrap()
                .outcome(),
            RuntimeLifecycleTimingOutcomeV2::FailedClosed
        );
    }

    #[test]
    fn abandoned_terminal_reporter_records_abandonment_and_emits() {
        let (recorder, observer) = RuntimeLifecycleTimingRecorderV2::create_v2();
        drop(RuntimeLifecycleTimingTerminalReporterV2::new_v2(
            recorder,
            observer.clone(),
        ));
        let total = observer
            .snapshot_v2()
            .sample_v2(RuntimeLifecycleTimingMetricV2::ShutdownTotal)
            .unwrap();
        assert_eq!(total.outcome(), RuntimeLifecycleTimingOutcomeV2::Abandoned);
        assert_eq!(total.elapsed(), Duration::ZERO);
        assert_eq!(observer.snapshot_v2().shutdown_source_v2(), None);
        assert_eq!(observer.terminal_emission_count_v2(), 1);
    }
}
