use std::fmt::{Debug, Formatter};
use std::net::SocketAddr;
use std::time::Instant;

use tokio::task::JoinHandle;
use tokio::time::{timeout_at, Instant as TokioInstant};

use crate::capability_readiness_supervisor::{
    RuntimeCapabilityReadinessActivationErrorV2, RuntimeCapabilityReadinessPreparedV2,
    RuntimeCapabilityReadinessShutdownHandleV2, RuntimeCapabilityReadinessSupervisorExitV2,
    RuntimeCapabilityReadinessSupervisorV2,
};
use crate::database::RuntimeDatabaseReadinessProbeV2;
use crate::gateway::{
    runtime_gateway_shutdown_projection_confirmed_v2, RuntimeGatewayShutdownHandleV1,
};
use crate::health::{
    RuntimeHealthReadinessHandleV1, RuntimeHealthReadinessPublisherV2,
    RuntimeHealthShutdownErrorV1, RuntimeHealthStartErrorV1, RuntimeHealthSupervisorV1,
};
use crate::ingress_acknowledgement_supervisor::{
    RuntimeIngressAcknowledgementShutdownHandleV2, RuntimeIngressAcknowledgementSupervisorExitV2,
    RuntimeIngressAcknowledgementTerminalObserverV2,
};
use crate::lifecycle_timing::{
    RuntimeLifecycleTimingMetricV2, RuntimeLifecycleTimingOutcomeV2,
    RuntimeLifecycleTimingRecorderV2,
};
use crate::maintenance_ingress_gate::RuntimeMaintenanceIngressGateShutdownHandleV2;
use crate::mutation_finalizer::{
    RuntimeMutationFinalizerSealHandleV1, RuntimeMutationFinalizerTerminalObserverV1,
};
use crate::shutdown::RuntimeShutdownObserverV1;
use crate::{
    RuntimeOsShutdownSignalsV1, RuntimeShutdownCauseV1, RuntimeShutdownSignalErrorV1,
    RuntimeShutdownSignalLatchV1, RuntimeShutdownTriggerV1, RuntimeShutdownTripV1,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeProcessRootSupervisorStartErrorV1 {
    #[error("runtime process signal registration failed")]
    Signal(RuntimeShutdownSignalErrorV1),
    #[error("runtime process health listener failed")]
    Health(RuntimeHealthStartErrorV1),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeProcessSignalTaskExitV1 {
    Signal(RuntimeShutdownCauseV1),
    StreamClosed,
    Panicked,
    Commanded,
    Finalizer(crate::RuntimeSupervisorExitV1),
    Health,
    IngressAcknowledgement(RuntimeIngressAcknowledgementSupervisorExitV2),
    CapabilityReadiness(RuntimeCapabilityReadinessSupervisorExitV2),
}

pub(crate) struct RuntimeProcessRootSupervisorControlV2 {
    maintenance_ingress: RuntimeMaintenanceIngressGateShutdownHandleV2,
    ingress_acknowledgement: RuntimeIngressAcknowledgementShutdownHandleV2,
    ingress_acknowledgement_terminal: RuntimeIngressAcknowledgementTerminalObserverV2,
    timing: RuntimeLifecycleTimingRecorderV2,
}

impl RuntimeProcessRootSupervisorControlV2 {
    pub(crate) fn new_v2(
        maintenance_ingress: RuntimeMaintenanceIngressGateShutdownHandleV2,
        ingress_acknowledgement: RuntimeIngressAcknowledgementShutdownHandleV2,
        ingress_acknowledgement_terminal: RuntimeIngressAcknowledgementTerminalObserverV2,
        timing: RuntimeLifecycleTimingRecorderV2,
    ) -> Self {
        Self {
            maintenance_ingress,
            ingress_acknowledgement,
            ingress_acknowledgement_terminal,
            timing,
        }
    }
}

#[derive(Clone)]
pub(crate) struct RuntimeProcessInvalidationTriggerV1 {
    trigger: RuntimeShutdownTriggerV1,
    health: RuntimeHealthReadinessHandleV1,
    maintenance_ingress: RuntimeMaintenanceIngressGateShutdownHandleV2,
    ingress_acknowledgement: RuntimeIngressAcknowledgementShutdownHandleV2,
    capability_readiness: RuntimeCapabilityReadinessShutdownHandleV2,
    finalizer: RuntimeMutationFinalizerSealHandleV1,
    timing: RuntimeLifecycleTimingRecorderV2,
}

impl RuntimeProcessInvalidationTriggerV1 {
    pub(crate) fn trip(&self, cause: RuntimeShutdownCauseV1) -> RuntimeShutdownTripV1 {
        let trip = self.trigger.trip(cause);
        let observation = trip.observation();
        let deadline = trip.observation().deadline();
        self.timing.observe_shutdown_v2(observation);
        self.health.seal_readiness();
        self.timing.record_shutdown_projection_v2(
            RuntimeLifecycleTimingMetricV2::ShutdownTripToReadinessSeal,
            observation,
            RuntimeLifecycleTimingOutcomeV2::Completed,
        );
        let maintenance = self.maintenance_ingress.seal_shutdown_v2();
        self.timing.record_shutdown_projection_v2(
            RuntimeLifecycleTimingMetricV2::ShutdownTripToMaintenanceIngressSeal,
            observation,
            if maintenance.shutdown_sealed() {
                RuntimeLifecycleTimingOutcomeV2::Completed
            } else {
                RuntimeLifecycleTimingOutcomeV2::FailedClosed
            },
        );
        self.ingress_acknowledgement.seal_until_v2(deadline);
        self.capability_readiness.seal_until_v2(deadline);
        self.finalizer.seal_intake();
        trip
    }
}

impl Debug for RuntimeProcessInvalidationTriggerV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessInvalidationTriggerV1(<redacted>)")
    }
}

#[derive(Clone)]
pub(crate) struct RuntimeProcessShutdownTriggerV1 {
    invalidation: RuntimeProcessInvalidationTriggerV1,
    gateway: RuntimeGatewayShutdownHandleV1,
}

impl RuntimeProcessShutdownTriggerV1 {
    pub(crate) fn trip(&self, cause: RuntimeShutdownCauseV1) -> RuntimeShutdownTripV1 {
        let trip = self.invalidation.trip(cause);
        let projection = self.gateway.enter_shutdown();
        self.invalidation.timing.record_shutdown_projection_v2(
            RuntimeLifecycleTimingMetricV2::ShutdownTripToGatewayProjection,
            trip.observation(),
            if runtime_gateway_shutdown_projection_confirmed_v2(&projection) {
                RuntimeLifecycleTimingOutcomeV2::Completed
            } else {
                RuntimeLifecycleTimingOutcomeV2::FailedClosed
            },
        );
        trip
    }

    pub(crate) fn invalidation_trigger_v1(&self) -> RuntimeProcessInvalidationTriggerV1 {
        self.invalidation.clone()
    }
}

impl Debug for RuntimeProcessShutdownTriggerV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessShutdownTriggerV1(<redacted>)")
    }
}

pub(crate) struct RuntimeProcessRootSupervisorV1 {
    latch: RuntimeShutdownSignalLatchV1,
    trigger: RuntimeProcessShutdownTriggerV1,
    signal_task: Option<JoinHandle<RuntimeProcessSignalTaskExitV1>>,
    health: Option<RuntimeHealthSupervisorV1>,
    readiness_publisher: Option<RuntimeHealthReadinessPublisherV2>,
    capability_readiness: RuntimeCapabilityReadinessSupervisorV2,
}

impl RuntimeProcessRootSupervisorV1 {
    pub(crate) async fn start(
        health_bind_addr: SocketAddr,
        finalizer: RuntimeMutationFinalizerSealHandleV1,
        mut finalizer_terminal: RuntimeMutationFinalizerTerminalObserverV1,
        gateway: RuntimeGatewayShutdownHandleV1,
        control: RuntimeProcessRootSupervisorControlV2,
    ) -> Result<Self, RuntimeProcessRootSupervisorStartErrorV1> {
        let RuntimeProcessRootSupervisorControlV2 {
            maintenance_ingress,
            ingress_acknowledgement,
            mut ingress_acknowledgement_terminal,
            timing,
        } = control;
        let mut signals = RuntimeOsShutdownSignalsV1::register()
            .map_err(RuntimeProcessRootSupervisorStartErrorV1::Signal)?;
        let mut health = RuntimeHealthSupervisorV1::start(health_bind_addr)
            .await
            .map_err(RuntimeProcessRootSupervisorStartErrorV1::Health)?;
        let readiness_publisher = health
            .take_readiness_publisher_v2()
            .expect("fresh runtime health supervisor");
        let mut health_terminal = health.terminal_observer();
        let latch = create_runtime_process_shutdown_latch_v1();
        let capability_readiness = RuntimeCapabilityReadinessPreparedV2::prepare_v2();
        let trigger = RuntimeProcessShutdownTriggerV1 {
            invalidation: RuntimeProcessInvalidationTriggerV1 {
                trigger: latch.trigger(),
                health: health.readiness_handle(),
                maintenance_ingress,
                ingress_acknowledgement,
                capability_readiness: capability_readiness.shutdown_handle_v2(),
                finalizer,
                timing,
            },
            gateway,
        };
        let capability_readiness = capability_readiness.start_v2(trigger.invalidation_trigger_v1());
        let mut capability_readiness_terminal = capability_readiness.terminal_observer_v2();
        let signal_trigger = trigger.clone();
        let signal_task = tokio::spawn(async move {
            tokio::select! {
                signal = signals.wait_cause() => {
                    match signal {
                        Ok(cause) => {
                            signal_trigger.trip(cause);
                            RuntimeProcessSignalTaskExitV1::Signal(cause)
                        }
                        Err(_) => {
                            signal_trigger.trip(RuntimeShutdownCauseV1::SupervisorFailure);
                            RuntimeProcessSignalTaskExitV1::StreamClosed
                        }
                    }
                }
                exit = finalizer_terminal.wait() => {
                    signal_trigger.trip(RuntimeShutdownCauseV1::FinalizerTerminal);
                    RuntimeProcessSignalTaskExitV1::Finalizer(exit)
                }
                () = health_terminal.wait() => {
                    signal_trigger.trip(RuntimeShutdownCauseV1::HealthTerminal);
                    RuntimeProcessSignalTaskExitV1::Health
                }
                exit = ingress_acknowledgement_terminal.wait_v2() => {
                    signal_trigger.trip(RuntimeShutdownCauseV1::IngressAcknowledgementTerminal);
                    RuntimeProcessSignalTaskExitV1::IngressAcknowledgement(exit)
                }
                exit = capability_readiness_terminal.wait_v2() => {
                    match exit {
                        RuntimeCapabilityReadinessSupervisorExitV2::ReadinessLost => {
                            signal_trigger
                                .invalidation
                                .trip(RuntimeShutdownCauseV1::ReadinessLost);
                        }
                        RuntimeCapabilityReadinessSupervisorExitV2::Commanded => {}
                        RuntimeCapabilityReadinessSupervisorExitV2::ControlClosed
                        | RuntimeCapabilityReadinessSupervisorExitV2::Panicked
                        | RuntimeCapabilityReadinessSupervisorExitV2::DeadlineElapsed => {
                            signal_trigger.trip(RuntimeShutdownCauseV1::SupervisorFailure);
                        }
                    }
                    RuntimeProcessSignalTaskExitV1::CapabilityReadiness(exit)
                }
            }
        });
        Ok(Self {
            latch,
            trigger,
            signal_task: Some(signal_task),
            health: Some(health),
            readiness_publisher: Some(readiness_publisher),
            capability_readiness,
        })
    }

    pub(crate) fn observer(&self) -> RuntimeShutdownObserverV1 {
        self.latch.observer()
    }

    pub(crate) fn shutdown_trigger(&self) -> RuntimeProcessShutdownTriggerV1 {
        self.trigger.clone()
    }

    pub(crate) fn invalidation_trigger(&self) -> RuntimeProcessInvalidationTriggerV1 {
        self.trigger.invalidation_trigger_v1()
    }

    pub(crate) fn take_readiness_publisher_v2(
        &mut self,
    ) -> Option<RuntimeHealthReadinessPublisherV2> {
        self.readiness_publisher.take()
    }

    pub(crate) fn trip(&self, cause: RuntimeShutdownCauseV1) -> RuntimeShutdownTripV1 {
        self.trigger.trip(cause)
    }

    pub(crate) async fn activate_capability_readiness_until_v2(
        &mut self,
        probe: RuntimeDatabaseReadinessProbeV2,
        deadline: Instant,
    ) -> Result<(), RuntimeCapabilityReadinessActivationErrorV2> {
        self.capability_readiness
            .activate_until_v2(probe, deadline)
            .await
    }

    pub(crate) async fn shutdown_capability_readiness_until_v2(
        &mut self,
        deadline: Instant,
    ) -> RuntimeCapabilityReadinessSupervisorExitV2 {
        self.capability_readiness.shutdown_until_v2(deadline).await
    }

    pub(crate) async fn join_signal_until(
        &mut self,
        deadline: Instant,
    ) -> RuntimeProcessSignalTaskExitV1 {
        let Some(mut task) = self.signal_task.take() else {
            return RuntimeProcessSignalTaskExitV1::Panicked;
        };
        if task.is_finished() && Instant::now() < deadline {
            return match timeout_at(TokioInstant::from_std(deadline), &mut task).await {
                Ok(Ok(exit)) => exit,
                _ => RuntimeProcessSignalTaskExitV1::Panicked,
            };
        }
        task.abort();
        let _ = task.await;
        RuntimeProcessSignalTaskExitV1::Commanded
    }

    pub(crate) async fn shutdown_health_until(
        mut self,
        deadline: Instant,
    ) -> Result<(), RuntimeHealthShutdownErrorV1> {
        match self.health.take() {
            Some(health) => health.shutdown_until(deadline).await,
            None => Err(RuntimeHealthShutdownErrorV1::TaskStopped),
        }
    }
}

pub(crate) fn create_runtime_process_shutdown_latch_v1() -> RuntimeShutdownSignalLatchV1 {
    RuntimeShutdownSignalLatchV1::create()
}

impl Drop for RuntimeProcessRootSupervisorV1 {
    fn drop(&mut self) {
        self.trigger.trip(RuntimeShutdownCauseV1::SupervisorFailure);
        if let Some(task) = self.signal_task.take() {
            task.abort();
        }
    }
}

impl Debug for RuntimeProcessRootSupervisorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessRootSupervisorV1(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;
    use std::time::{Duration, Instant};

    use automation_runtime_convergence::ProcessInstanceId;
    use automation_runtime_worker::{
        RuntimeGatewayClosedSnapshotV2, RuntimeMutationFinalizerGenerationV1,
    };

    use super::*;
    use crate::ingress_acknowledgement_supervisor::{
        RuntimeIngressAcknowledgementExecutionContextV2,
        RuntimeIngressAcknowledgementExecutionResultV2, RuntimeIngressAcknowledgementLaneJobV2,
        RuntimeIngressAcknowledgementSupervisorConfigV2,
        RuntimeIngressAcknowledgementSupervisorPhaseV2, RuntimeIngressAcknowledgementSupervisorV2,
    };
    use crate::lifecycle_timing::{
        RuntimeLifecycleShutdownSourceV2, RuntimeLifecycleTimingMetricV2,
        RuntimeLifecycleTimingOutcomeV2, RuntimeLifecycleTimingRecorderV2,
    };
    use crate::maintenance_ingress_gate::{
        RuntimeMaintenanceIngressGateControllerV2, RuntimeMaintenanceIngressGateStageV2,
    };
    use crate::mutation_finalizer::{
        RuntimeMutationFinalizerConfigV1, RuntimeMutationFinalizerJobV1,
        RuntimeMutationFinalizerPortV1, RuntimeMutationFinalizerSupervisorV1,
    };
    use crate::{compose_runtime_gateway_bootstrap_v1, GatewayResourceConfigV1};

    #[derive(Clone, Copy)]
    struct ComposedAcknowledgementJobV2;

    impl RuntimeIngressAcknowledgementLaneJobV2<()> for ComposedAcknowledgementJobV2 {
        type Output = ();
        type CompletionError = ();

        async fn execute(
            self,
            _port: &(),
            _context: RuntimeIngressAcknowledgementExecutionContextV2,
        ) -> RuntimeIngressAcknowledgementExecutionResultV2<Self, Self::Output, ()> {
            RuntimeIngressAcknowledgementExecutionResultV2::Accepted(())
        }
    }

    #[derive(Clone, Copy)]
    struct ComposedFinalizerPortV1;

    impl RuntimeMutationFinalizerPortV1 for ComposedFinalizerPortV1 {
        type Job = ();
        type Output = ();
        type Error = ();

        async fn execute(
            &self,
            _job: RuntimeMutationFinalizerJobV1<Self::Job>,
        ) -> Result<Self::Output, Self::Error> {
            Ok(())
        }
    }

    async fn direct_trip_projection_sample_v2() -> [Duration; 3] {
        let process_instance_id =
            ProcessInstanceId::parse("runtime-process:direct-trip-projection").unwrap();
        let gateway = compose_runtime_gateway_bootstrap_v1(
            process_instance_id,
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let (maintenance, maintenance_observer, maintenance_shutdown) =
            RuntimeMaintenanceIngressGateControllerV2::new_v2();
        let maintenance = maintenance
            .begin_open_v2()
            .unwrap()
            .commit_open_v2()
            .unwrap();
        let acknowledgement =
            RuntimeIngressAcknowledgementSupervisorV2::<(), ComposedAcknowledgementJobV2>::start(
                (),
                RuntimeIngressAcknowledgementSupervisorConfigV2::new(Duration::from_millis(1))
                    .unwrap(),
            );
        let finalizer = RuntimeMutationFinalizerSupervisorV1::start(
            RuntimeMutationFinalizerConfigV1::new(1).unwrap(),
            RuntimeMutationFinalizerGenerationV1::new(NonZeroU64::MIN).unwrap(),
            ComposedFinalizerPortV1,
        )
        .unwrap();
        let (timing, timing_observer) = RuntimeLifecycleTimingRecorderV2::create_v2();
        let mut root = RuntimeProcessRootSupervisorV1::start(
            "127.0.0.1:0".parse().unwrap(),
            finalizer.seal_handle(),
            finalizer.terminal_observer(),
            gateway.shutdown_handle_v1(),
            RuntimeProcessRootSupervisorControlV2::new_v2(
                maintenance_shutdown,
                acknowledgement.shutdown_handle_v2(),
                acknowledgement.terminal_observer_v2(),
                timing,
            ),
        )
        .await
        .unwrap();
        let readiness = root.take_readiness_publisher_v2().unwrap();
        let readiness_state = root.health.as_ref().unwrap().readiness_handle();
        assert!(readiness.publish_ready_v2());
        root.shutdown_trigger()
            .trip(RuntimeShutdownCauseV1::Explicit);
        assert!(readiness_state.is_sealed());
        assert!(!readiness_state.is_ready());
        assert!(maintenance_observer.snapshot_v2().shutdown_sealed());
        assert!(matches!(
            gateway.closed_snapshot(),
            RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
        ));
        let snapshot = timing_observer.snapshot_v2();
        assert_eq!(
            snapshot.shutdown_source_v2(),
            Some(RuntimeLifecycleShutdownSourceV2::Direct)
        );
        let durations = [
            RuntimeLifecycleTimingMetricV2::ShutdownTripToReadinessSeal,
            RuntimeLifecycleTimingMetricV2::ShutdownTripToMaintenanceIngressSeal,
            RuntimeLifecycleTimingMetricV2::ShutdownTripToGatewayProjection,
        ]
        .map(|metric| {
            let sample = snapshot.sample_v2(metric).unwrap();
            assert_eq!(sample.outcome(), RuntimeLifecycleTimingOutcomeV2::Completed);
            sample.elapsed()
        });
        drop(maintenance);
        let deadline = Instant::now() + Duration::from_secs(2);
        assert_eq!(
            root.shutdown_capability_readiness_until_v2(deadline).await,
            RuntimeCapabilityReadinessSupervisorExitV2::Commanded
        );
        let _signal_exit = root.join_signal_until(deadline).await;
        root.shutdown_health_until(deadline).await.unwrap();
        assert_eq!(
            acknowledgement.shutdown_until(deadline).await.exit(),
            RuntimeIngressAcknowledgementSupervisorExitV2::Commanded
        );
        assert_eq!(
            finalizer.shutdown_until(deadline).await.exit(),
            crate::RuntimeSupervisorExitV1::Commanded
        );
        durations
    }

    fn projection_percentiles_v2(mut values: Vec<Duration>) -> [Duration; 4] {
        values.sort_unstable();
        let nearest_rank = |percent: usize| {
            let rank = (values.len() * percent).div_ceil(100);
            values[rank.saturating_sub(1)]
        };
        [
            nearest_rank(50),
            nearest_rank(95),
            nearest_rank(99),
            *values.last().unwrap(),
        ]
    }

    fn require_release_profile_v2() {
        #[cfg(debug_assertions)]
        panic!("release profile required");
    }

    #[tokio::test]
    #[ignore = "diagnostic in-process gateway shutdown projection"]
    async fn runtime_direct_trip_projection_release_diagnostic_v2() {
        require_release_profile_v2();
        for _ in 0..20 {
            let _ = direct_trip_projection_sample_v2().await;
        }
        let mut readiness = Vec::with_capacity(200);
        let mut maintenance = Vec::with_capacity(200);
        let mut gateway = Vec::with_capacity(200);
        for _ in 0..200 {
            let [readiness_elapsed, maintenance_elapsed, gateway_elapsed] =
                direct_trip_projection_sample_v2().await;
            readiness.push(readiness_elapsed);
            maintenance.push(maintenance_elapsed);
            gateway.push(gateway_elapsed);
        }
        let readiness = projection_percentiles_v2(readiness);
        let maintenance = projection_percentiles_v2(maintenance);
        let gateway = projection_percentiles_v2(gateway);
        for (name, values) in [
            ("readiness_seal", readiness),
            ("maintenance_ingress_seal", maintenance),
            ("gateway_shutdown_projection", gateway),
        ] {
            println!(
                "runtime_lifecycle_diagnostic_cohort scope=in_process_gateway_shutdown_projection load=idle unmeasured=os_signal_delivery,discord_hard_pause,capacity_50_percent,capacity_90_percent metric={name} warmup=20 samples=200 p50={}ns p95={}ns p99={}ns max={}ns",
                values[0].as_nanos(),
                values[1].as_nanos(),
                values[2].as_nanos(),
                values[3].as_nanos()
            );
        }
        assert!(readiness[2] <= Duration::from_millis(50));
        assert!(maintenance[2] <= Duration::from_millis(50));
        assert!(gateway[2] <= Duration::from_millis(250));
    }

    #[test]
    fn process_latch_uses_a_fresh_shutdown_window() {
        let latch = create_runtime_process_shutdown_latch_v1();
        let now = Instant::now();
        let observation = latch
            .trigger()
            .trip(RuntimeShutdownCauseV1::Explicit)
            .observation();

        assert!(observation.deadline() >= now + Duration::from_secs(29));
        assert!(observation.deadline() <= Instant::now() + Duration::from_secs(30));
    }

    #[tokio::test]
    async fn composed_invalidation_seals_every_admission_surface_before_gateway_shutdown() {
        let process_instance_id =
            ProcessInstanceId::parse("runtime-process:composed-invalidation").unwrap();
        let gateway = compose_runtime_gateway_bootstrap_v1(
            process_instance_id,
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let (maintenance, maintenance_observer, maintenance_shutdown) =
            RuntimeMaintenanceIngressGateControllerV2::new_v2();
        let maintenance = maintenance
            .begin_open_v2()
            .unwrap()
            .commit_open_v2()
            .unwrap();
        assert_eq!(
            maintenance_observer.snapshot_v2().stage(),
            RuntimeMaintenanceIngressGateStageV2::Open
        );
        let acknowledgement =
            RuntimeIngressAcknowledgementSupervisorV2::<(), ComposedAcknowledgementJobV2>::start(
                (),
                RuntimeIngressAcknowledgementSupervisorConfigV2::new(Duration::from_millis(1))
                    .unwrap(),
            );
        let finalizer = RuntimeMutationFinalizerSupervisorV1::start(
            RuntimeMutationFinalizerConfigV1::new(1).unwrap(),
            RuntimeMutationFinalizerGenerationV1::new(NonZeroU64::MIN).unwrap(),
            ComposedFinalizerPortV1,
        )
        .unwrap();
        let (timing, timing_observer) = RuntimeLifecycleTimingRecorderV2::create_v2();
        let mut root = RuntimeProcessRootSupervisorV1::start(
            "127.0.0.1:0".parse().unwrap(),
            finalizer.seal_handle(),
            finalizer.terminal_observer(),
            gateway.shutdown_handle_v1(),
            RuntimeProcessRootSupervisorControlV2::new_v2(
                maintenance_shutdown,
                acknowledgement.shutdown_handle_v2(),
                acknowledgement.terminal_observer_v2(),
                timing,
            ),
        )
        .await
        .unwrap();
        let readiness = root.take_readiness_publisher_v2().unwrap();
        let readiness_state = root.health.as_ref().unwrap().readiness_handle();
        assert!(readiness.publish_ready_v2());
        assert!(readiness_state.is_ready());
        assert!(!readiness_state.is_sealed());

        let invalidation = root.invalidation_trigger();
        let trip = invalidation.trip(RuntimeShutdownCauseV1::ReadinessLost);

        assert_eq!(
            trip.observation().cause(),
            RuntimeShutdownCauseV1::ReadinessLost
        );
        assert!(!readiness_state.is_ready());
        assert!(readiness_state.is_sealed());
        assert!(!readiness.publish_ready_v2());
        let maintenance_snapshot = maintenance_observer.snapshot_v2();
        assert!(maintenance_snapshot.shutdown_sealed());
        assert_ne!(
            maintenance_snapshot.stage(),
            RuntimeMaintenanceIngressGateStageV2::Open
        );
        assert_eq!(
            acknowledgement.snapshot().phase(),
            RuntimeIngressAcknowledgementSupervisorPhaseV2::ShutdownSealed
        );
        assert!(invalidation.capability_readiness.is_sealed_v2());
        let finalizer_snapshot = finalizer.snapshot();
        assert!(finalizer_snapshot.shutdown_sealed());
        assert!(!finalizer_snapshot.intake_open());
        assert!(!matches!(
            gateway.closed_snapshot(),
            RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
        ));

        root.shutdown_trigger()
            .trip(RuntimeShutdownCauseV1::Explicit);

        assert!(matches!(
            gateway.closed_snapshot(),
            RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
        ));
        let timing = timing_observer.snapshot_v2();
        assert_eq!(
            timing.shutdown_source_v2(),
            Some(RuntimeLifecycleShutdownSourceV2::Supervisor)
        );
        for metric in [
            RuntimeLifecycleTimingMetricV2::ShutdownTripToReadinessSeal,
            RuntimeLifecycleTimingMetricV2::ShutdownTripToMaintenanceIngressSeal,
            RuntimeLifecycleTimingMetricV2::ShutdownTripToGatewayProjection,
        ] {
            assert_eq!(
                timing.sample_v2(metric).unwrap().outcome(),
                RuntimeLifecycleTimingOutcomeV2::Completed
            );
        }

        drop(maintenance);
        let deadline = Instant::now() + Duration::from_secs(2);
        let capability_exit = root.shutdown_capability_readiness_until_v2(deadline).await;
        assert_eq!(
            capability_exit,
            RuntimeCapabilityReadinessSupervisorExitV2::Commanded
        );
        let _signal_exit = root.join_signal_until(deadline).await;
        root.shutdown_health_until(deadline).await.unwrap();
        let acknowledgement_report = acknowledgement.shutdown_until(deadline).await;
        assert_eq!(
            acknowledgement_report.exit(),
            RuntimeIngressAcknowledgementSupervisorExitV2::Commanded
        );
        let finalizer_report = finalizer.shutdown_until(deadline).await;
        assert_eq!(
            finalizer_report.exit(),
            crate::RuntimeSupervisorExitV1::Commanded
        );
    }
}
