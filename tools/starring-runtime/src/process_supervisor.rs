use std::fmt::{Debug, Formatter};
use std::net::SocketAddr;
use std::time::Instant;

use tokio::task::JoinHandle;
use tokio::time::{timeout_at, Instant as TokioInstant};

use crate::gateway::RuntimeGatewayShutdownHandleV1;
use crate::health::{
    RuntimeHealthReadinessHandleV1, RuntimeHealthReadinessPublisherV2,
    RuntimeHealthShutdownErrorV1, RuntimeHealthStartErrorV1, RuntimeHealthSupervisorV1,
};
use crate::ingress_acknowledgement_supervisor::{
    RuntimeIngressAcknowledgementShutdownHandleV2, RuntimeIngressAcknowledgementSupervisorExitV2,
    RuntimeIngressAcknowledgementTerminalObserverV2,
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
}

#[derive(Clone)]
pub(crate) struct RuntimeProcessShutdownTriggerV1 {
    trigger: RuntimeShutdownTriggerV1,
    health: RuntimeHealthReadinessHandleV1,
    maintenance_ingress: RuntimeMaintenanceIngressGateShutdownHandleV2,
    ingress_acknowledgement: RuntimeIngressAcknowledgementShutdownHandleV2,
    finalizer: RuntimeMutationFinalizerSealHandleV1,
    gateway: RuntimeGatewayShutdownHandleV1,
}

impl RuntimeProcessShutdownTriggerV1 {
    pub(crate) fn trip(&self, cause: RuntimeShutdownCauseV1) -> RuntimeShutdownTripV1 {
        let trip = self.trigger.trip(cause);
        let deadline = trip.observation().deadline();
        self.health.seal_readiness();
        self.maintenance_ingress.seal_shutdown_v2();
        self.ingress_acknowledgement.seal_until_v2(deadline);
        self.finalizer.seal_intake();
        self.gateway.enter_shutdown();
        trip
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
}

impl RuntimeProcessRootSupervisorV1 {
    pub(crate) async fn start(
        health_bind_addr: SocketAddr,
        finalizer: RuntimeMutationFinalizerSealHandleV1,
        mut finalizer_terminal: RuntimeMutationFinalizerTerminalObserverV1,
        gateway: RuntimeGatewayShutdownHandleV1,
        maintenance_ingress: RuntimeMaintenanceIngressGateShutdownHandleV2,
        ingress_acknowledgement: RuntimeIngressAcknowledgementShutdownHandleV2,
        mut ingress_acknowledgement_terminal: RuntimeIngressAcknowledgementTerminalObserverV2,
    ) -> Result<Self, RuntimeProcessRootSupervisorStartErrorV1> {
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
        let trigger = RuntimeProcessShutdownTriggerV1 {
            trigger: latch.trigger(),
            health: health.readiness_handle(),
            maintenance_ingress,
            ingress_acknowledgement,
            finalizer,
            gateway,
        };
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
            }
        });
        Ok(Self {
            latch,
            trigger,
            signal_task: Some(signal_task),
            health: Some(health),
            readiness_publisher: Some(readiness_publisher),
        })
    }

    pub(crate) fn observer(&self) -> RuntimeShutdownObserverV1 {
        self.latch.observer()
    }

    pub(crate) fn shutdown_trigger(&self) -> RuntimeProcessShutdownTriggerV1 {
        self.trigger.clone()
    }

    pub(crate) fn take_readiness_publisher_v2(
        &mut self,
    ) -> Option<RuntimeHealthReadinessPublisherV2> {
        self.readiness_publisher.take()
    }

    pub(crate) fn trip(&self, cause: RuntimeShutdownCauseV1) -> RuntimeShutdownTripV1 {
        self.trigger.trip(cause)
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
    use std::time::{Duration, Instant};

    use super::*;

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
}
