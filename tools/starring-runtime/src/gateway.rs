use std::num::NonZeroU64;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use automation_runtime::{
    shared_gateway_control_channel_with_policy_and_invalidator_v3, GatewayAdmissionPolicyV3,
    GatewayConnectionStateV3, GatewayControlConfigV3, GatewayControlConfigurationErrorV3,
    GatewayControlTransitionErrorV3, GatewayDrainCauseV3, GatewayInvalidationSignalV3,
    GatewayReadyKindV3, GatewaySynchronousInvalidatorV3, SharedGatewayControlV3,
    SharedGatewayRuntimeControlV3,
};
use automation_runtime_controller::{
    RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayReadyAttestationV2, RuntimeGatewayReadyKindV2,
};
use automation_runtime_convergence::ProcessInstanceId;
use automation_runtime_worker::{
    RuntimeAcceptedGatewayOwnerReceiptV1, RuntimeGatewayClosedLifecycleV2,
    RuntimeGatewayClosedSnapshotV2, RuntimeGatewayInvalidationCauseV2,
    RuntimeGatewayOwnerLeasePortV1,
};

use crate::gateway_owner_startup_watchdog::{
    start_runtime_gateway_owner_startup_watchdog_v1, RuntimeGatewayOwnerEmergencyInvalidatorV1,
};
use crate::{
    GatewayResourceConfigV1, RuntimeGatewayOwnerStartupWatchdogConfigV1,
    RuntimeGatewayOwnerStartupWatchdogHandleV1, RuntimeGatewayOwnerStartupWatchdogStartErrorV1,
    RuntimeGatewayOwnerStartupWatchdogStartFailureV1,
};

const SUPPORTED_GATEWAY_SHARD_ID: &str = "shard:0";

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeGatewayBootstrapErrorV1 {
    #[error("runtime gateway command capacity is invalid")]
    CommandCapacity,
    #[error("runtime gateway lifecycle capacity is invalid")]
    LifecycleCapacity,
}

impl RuntimeGatewayBootstrapErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::CommandCapacity => "runtime_gateway_command_capacity",
            Self::LifecycleCapacity => "runtime_gateway_lifecycle_capacity",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeGatewayReadyObservationErrorV1 {
    #[error("runtime gateway connection epoch is stale")]
    StaleConnectionEpoch,
    #[error("runtime gateway admission snapshot is stale")]
    StaleAdmissionSnapshot,
    #[error("runtime gateway control owner is unavailable")]
    ControlOrphaned,
    #[error("runtime gateway is not connected")]
    NotConnected,
    #[error("runtime gateway admission is paused")]
    AdmissionPaused,
    #[error("runtime gateway admission is not paused")]
    AdmissionNotPaused,
    #[error("runtime gateway is draining")]
    Draining,
    #[error("runtime gateway is stopped")]
    Stopped,
    #[error("runtime gateway connection epoch overflowed")]
    ConnectionEpochOverflow,
    #[error("runtime gateway admission revision overflowed")]
    AdmissionRevisionOverflow,
    #[error("runtime gateway admission sequence overflowed")]
    AdmissionSequenceOverflow,
    #[error("runtime gateway lifecycle queue overflowed")]
    LifecycleOverflow,
    #[error("runtime gateway lifecycle observer is unavailable")]
    LifecycleClosed,
    #[error("runtime gateway ready evidence is no longer current")]
    ReadyEvidenceNotCurrent,
    #[error("runtime gateway ready evidence contains a zero sequence")]
    ReadyEvidenceSequenceZero,
    #[error("runtime gateway ready evidence exceeds the persistence domain")]
    ReadyEvidenceOutOfRange,
    #[error("runtime gateway ready evidence lacks an explicit resume")]
    ReadyEvidenceNotExplicitlyResumed,
    #[error("runtime gateway ownership is uncertain")]
    OwnershipUncertain,
}

impl RuntimeGatewayReadyObservationErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::StaleConnectionEpoch => "runtime_gateway_stale_connection_epoch",
            Self::StaleAdmissionSnapshot => "runtime_gateway_stale_admission_snapshot",
            Self::ControlOrphaned => "runtime_gateway_control_orphaned",
            Self::NotConnected => "runtime_gateway_not_connected",
            Self::AdmissionPaused => "runtime_gateway_admission_paused",
            Self::AdmissionNotPaused => "runtime_gateway_admission_not_paused",
            Self::Draining => "runtime_gateway_draining",
            Self::Stopped => "runtime_gateway_stopped",
            Self::ConnectionEpochOverflow => "runtime_gateway_connection_epoch_overflow",
            Self::AdmissionRevisionOverflow => "runtime_gateway_admission_revision_overflow",
            Self::AdmissionSequenceOverflow => "runtime_gateway_admission_sequence_overflow",
            Self::LifecycleOverflow => "runtime_gateway_lifecycle_overflow",
            Self::LifecycleClosed => "runtime_gateway_lifecycle_closed",
            Self::ReadyEvidenceNotCurrent => "runtime_gateway_ready_evidence_not_current",
            Self::ReadyEvidenceSequenceZero => "runtime_gateway_ready_evidence_sequence_zero",
            Self::ReadyEvidenceOutOfRange => "runtime_gateway_ready_evidence_out_of_range",
            Self::ReadyEvidenceNotExplicitlyResumed => {
                "runtime_gateway_ready_evidence_not_explicitly_resumed"
            }
            Self::OwnershipUncertain => "runtime_gateway_ownership_uncertain",
        }
    }
}

struct SharedGatewayControlAdapterV2 {
    process_instance_id: ProcessInstanceId,
    control: SharedGatewayControlV3,
    closed_lifecycle: Arc<Mutex<RuntimeGatewayClosedLifecycleV2>>,
}

struct SharedGatewayRuntimeHalfV3 {
    _inner: SharedGatewayRuntimeControlV3,
}

pub struct RuntimeGatewayBootstrapV1 {
    adapter: SharedGatewayControlAdapterV2,
    _runtime: SharedGatewayRuntimeHalfV3,
    owner_invalidator: Option<RuntimeGatewayOwnerInvalidationBridgeV2>,
    owner_invalidated: Arc<AtomicBool>,
}

struct RuntimeGatewayInvalidationBridgeV2 {
    closed_lifecycle: Arc<Mutex<RuntimeGatewayClosedLifecycleV2>>,
}

struct RuntimeGatewayOwnerInvalidationBridgeV2 {
    closed_lifecycle: Arc<Mutex<RuntimeGatewayClosedLifecycleV2>>,
    invalidated: Arc<AtomicBool>,
}

impl RuntimeGatewayOwnerEmergencyInvalidatorV1 for RuntimeGatewayOwnerInvalidationBridgeV2 {
    fn invalidate_gateway_ownership(&self) {
        invalidate_gateway_owner_state(&self.closed_lifecycle, &self.invalidated);
    }
}

impl GatewaySynchronousInvalidatorV3 for RuntimeGatewayInvalidationBridgeV2 {
    fn invalidate(&self, signal: GatewayInvalidationSignalV3) {
        let mut lifecycle = self
            .closed_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match signal {
            GatewayInvalidationSignalV3::AdmissionPaused => invalidate_closed_lifecycle(
                &mut lifecycle,
                RuntimeGatewayInvalidationCauseV2::CapabilityNotReady,
            ),
            GatewayInvalidationSignalV3::Disconnected(_) => invalidate_closed_lifecycle(
                &mut lifecycle,
                RuntimeGatewayInvalidationCauseV2::TransportDisconnected,
            ),
            GatewayInvalidationSignalV3::Draining(GatewayDrainCauseV3::Commanded)
            | GatewayInvalidationSignalV3::Stopped(_) => shutdown_closed_lifecycle(&mut lifecycle),
            GatewayInvalidationSignalV3::Draining(
                GatewayDrainCauseV3::ControlOrphaned | GatewayDrainCauseV3::LifecycleClosed,
            )
            | GatewayInvalidationSignalV3::ControlOrphaned => invalidate_closed_lifecycle(
                &mut lifecycle,
                RuntimeGatewayInvalidationCauseV2::ControlOrphaned,
            ),
            GatewayInvalidationSignalV3::Draining(
                GatewayDrainCauseV3::ConnectionEpochOverflow
                | GatewayDrainCauseV3::AdmissionRevisionOverflow
                | GatewayDrainCauseV3::AdmissionSequenceOverflow,
            ) => invalidate_closed_lifecycle(
                &mut lifecycle,
                RuntimeGatewayInvalidationCauseV2::ProtocolViolation,
            ),
            GatewayInvalidationSignalV3::Draining(
                GatewayDrainCauseV3::LifecycleOverflow | GatewayDrainCauseV3::RuntimeFailure,
            ) => invalidate_closed_lifecycle(
                &mut lifecycle,
                RuntimeGatewayInvalidationCauseV2::CapabilityNotReady,
            ),
        }
    }
}

impl RuntimeGatewayBootstrapV1 {
    pub fn closed_snapshot(&self) -> RuntimeGatewayClosedSnapshotV2 {
        self.adapter
            .closed_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot()
    }

    pub fn observe_current_ready_attestation(
        &self,
    ) -> Result<RuntimeGatewayReadyAttestationV2, RuntimeGatewayReadyObservationErrorV1> {
        self.require_current_gateway_ownership()?;
        let attestation = self.adapter.observe_current_ready_attestation()?;
        self.require_current_gateway_ownership()?;
        Ok(attestation)
    }

    pub fn ready_attestation_is_current(
        &self,
        candidate: &RuntimeGatewayReadyAttestationV2,
    ) -> bool {
        self.observe_current_ready_attestation()
            .is_ok_and(|current| current == *candidate)
    }

    pub fn start_gateway_owner_startup_watchdog_v1<P>(
        &mut self,
        port: P,
        accepted_receipt: RuntimeAcceptedGatewayOwnerReceiptV1,
        request_started_at: Instant,
        response_observed_at: Instant,
        config: RuntimeGatewayOwnerStartupWatchdogConfigV1,
    ) -> Result<
        RuntimeGatewayOwnerStartupWatchdogHandleV1,
        RuntimeGatewayOwnerStartupWatchdogStartFailureV1<P>,
    >
    where
        P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync + 'static,
        P::Error: Send + 'static,
    {
        let lease_id = accepted_receipt.receipt().lease_id.clone();
        let Some(invalidator) = self.owner_invalidator.take() else {
            invalidate_gateway_owner_state(&self.adapter.closed_lifecycle, &self.owner_invalidated);
            return Err(RuntimeGatewayOwnerStartupWatchdogStartFailureV1::new(
                RuntimeGatewayOwnerStartupWatchdogStartErrorV1::AlreadyStarted,
                port,
                lease_id,
            ));
        };
        if accepted_receipt.receipt().lease_id.process_instance_id
            != self.adapter.process_instance_id
        {
            invalidator.invalidate_gateway_ownership();
            return Err(RuntimeGatewayOwnerStartupWatchdogStartFailureV1::new(
                RuntimeGatewayOwnerStartupWatchdogStartErrorV1::ProcessMismatch,
                port,
                lease_id,
            ));
        }
        if accepted_receipt
            .receipt()
            .lease_id
            .gateway_shard_id
            .as_str()
            != SUPPORTED_GATEWAY_SHARD_ID
        {
            invalidator.invalidate_gateway_ownership();
            return Err(RuntimeGatewayOwnerStartupWatchdogStartFailureV1::new(
                RuntimeGatewayOwnerStartupWatchdogStartErrorV1::ShardMismatch,
                port,
                lease_id,
            ));
        }
        start_runtime_gateway_owner_startup_watchdog_v1(
            port,
            invalidator,
            accepted_receipt,
            request_started_at,
            response_observed_at,
            config,
        )
    }

    fn require_current_gateway_ownership(
        &self,
    ) -> Result<(), RuntimeGatewayReadyObservationErrorV1> {
        if self.owner_invalidated.load(Ordering::Acquire) {
            Err(RuntimeGatewayReadyObservationErrorV1::OwnershipUncertain)
        } else {
            Ok(())
        }
    }
}

pub fn compose_runtime_gateway_bootstrap_v1(
    process_instance_id: ProcessInstanceId,
    config: GatewayResourceConfigV1,
) -> Result<RuntimeGatewayBootstrapV1, RuntimeGatewayBootstrapErrorV1> {
    let control_config =
        GatewayControlConfigV3::new(config.command_capacity(), config.lifecycle_capacity())
            .map_err(map_configuration_error)?;
    Ok(compose_with_control_config(
        process_instance_id,
        control_config,
    ))
}

fn compose_with_control_config(
    process_instance_id: ProcessInstanceId,
    config: GatewayControlConfigV3,
) -> RuntimeGatewayBootstrapV1 {
    let closed_lifecycle = Arc::new(Mutex::new(RuntimeGatewayClosedLifecycleV2::starting()));
    let owner_invalidated = Arc::new(AtomicBool::new(false));
    let invalidation = RuntimeGatewayInvalidationBridgeV2 {
        closed_lifecycle: closed_lifecycle.clone(),
    };
    let (control, runtime) = shared_gateway_control_channel_with_policy_and_invalidator_v3(
        config,
        GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect,
        invalidation,
    );
    RuntimeGatewayBootstrapV1 {
        adapter: SharedGatewayControlAdapterV2 {
            process_instance_id,
            control,
            closed_lifecycle: closed_lifecycle.clone(),
        },
        _runtime: SharedGatewayRuntimeHalfV3 { _inner: runtime },
        owner_invalidator: Some(RuntimeGatewayOwnerInvalidationBridgeV2 {
            closed_lifecycle,
            invalidated: owner_invalidated.clone(),
        }),
        owner_invalidated,
    }
}

fn invalidate_gateway_owner_state(
    closed_lifecycle: &Arc<Mutex<RuntimeGatewayClosedLifecycleV2>>,
    invalidated: &AtomicBool,
) {
    if invalidated.swap(true, Ordering::AcqRel) {
        return;
    }
    let mut lifecycle = closed_lifecycle
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    invalidate_closed_lifecycle(
        &mut lifecycle,
        RuntimeGatewayInvalidationCauseV2::OwnershipUncertain,
    );
}

fn invalidate_closed_lifecycle(
    lifecycle: &mut RuntimeGatewayClosedLifecycleV2,
    cause: RuntimeGatewayInvalidationCauseV2,
) {
    let generation = lifecycle.snapshot().generation();
    let _ = lifecycle.invalidate(generation, cause);
}

fn shutdown_closed_lifecycle(lifecycle: &mut RuntimeGatewayClosedLifecycleV2) {
    let generation = lifecycle.snapshot().generation();
    let _ = lifecycle.shutdown(generation);
}

impl SharedGatewayControlAdapterV2 {
    fn observe_current_ready_attestation(
        &self,
    ) -> Result<RuntimeGatewayReadyAttestationV2, RuntimeGatewayReadyObservationErrorV1> {
        let snapshot = self.control.current_admission_snapshot();
        let epoch = match snapshot.connection() {
            GatewayConnectionStateV3::Connected { epoch, .. } => epoch,
            GatewayConnectionStateV3::Starting | GatewayConnectionStateV3::Disconnected { .. } => {
                return Err(RuntimeGatewayReadyObservationErrorV1::NotConnected)
            }
            GatewayConnectionStateV3::Paused { .. } => {
                return Err(RuntimeGatewayReadyObservationErrorV1::AdmissionPaused)
            }
            GatewayConnectionStateV3::Draining { .. } => {
                return Err(RuntimeGatewayReadyObservationErrorV1::Draining)
            }
            GatewayConnectionStateV3::Stopped { .. } => {
                return Err(RuntimeGatewayReadyObservationErrorV1::Stopped)
            }
        };
        let lease = self
            .control
            .issue_ready_lease(epoch)
            .map_err(map_transition_error)?;
        if !self.control.ready_lease_is_current(&lease) {
            return Err(RuntimeGatewayReadyObservationErrorV1::ReadyEvidenceNotCurrent);
        }
        if !lease.was_explicitly_resumed() {
            return Err(RuntimeGatewayReadyObservationErrorV1::ReadyEvidenceNotExplicitlyResumed);
        }
        let connection_epoch = bounded_non_zero(lease.epoch().get())?;
        let admission_revision = bounded_non_zero(lease.admission_revision().get())?;
        let connected_event_sequence = bounded_sequence(lease.connected_event_sequence().get())?;
        let resume_sequence = bounded_sequence(lease.resume_sequence().get())?;
        let kind = match lease.kind() {
            GatewayReadyKindV3::Ready => RuntimeGatewayReadyKindV2::Ready,
            GatewayReadyKindV3::Resumed => RuntimeGatewayReadyKindV2::Resumed,
        };
        Ok(RuntimeGatewayReadyAttestationV2 {
            process_instance_id: self.process_instance_id.clone(),
            connection_epoch,
            kind,
            admission_revision,
            connected_event_sequence,
            resume_sequence,
        })
    }
}

fn bounded_non_zero(value: u64) -> Result<NonZeroU64, RuntimeGatewayReadyObservationErrorV1> {
    let value = NonZeroU64::new(value)
        .ok_or(RuntimeGatewayReadyObservationErrorV1::ReadyEvidenceSequenceZero)?;
    if value.get() > i64::MAX as u64 {
        return Err(RuntimeGatewayReadyObservationErrorV1::ReadyEvidenceOutOfRange);
    }
    Ok(value)
}

fn bounded_sequence(
    value: u64,
) -> Result<RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayReadyObservationErrorV1> {
    bounded_non_zero(value).map(RuntimeGatewayAdmissionSequenceV2::new)
}

fn map_configuration_error(
    error: GatewayControlConfigurationErrorV3,
) -> RuntimeGatewayBootstrapErrorV1 {
    match error {
        GatewayControlConfigurationErrorV3::CommandCapacity => {
            RuntimeGatewayBootstrapErrorV1::CommandCapacity
        }
        GatewayControlConfigurationErrorV3::LifecycleCapacity => {
            RuntimeGatewayBootstrapErrorV1::LifecycleCapacity
        }
    }
}

fn map_transition_error(
    error: GatewayControlTransitionErrorV3,
) -> RuntimeGatewayReadyObservationErrorV1 {
    match error {
        GatewayControlTransitionErrorV3::StaleConnectionEpoch => {
            RuntimeGatewayReadyObservationErrorV1::StaleConnectionEpoch
        }
        GatewayControlTransitionErrorV3::StaleAdmissionSnapshot => {
            RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot
        }
        GatewayControlTransitionErrorV3::ControlOrphaned => {
            RuntimeGatewayReadyObservationErrorV1::ControlOrphaned
        }
        GatewayControlTransitionErrorV3::NotConnected => {
            RuntimeGatewayReadyObservationErrorV1::NotConnected
        }
        GatewayControlTransitionErrorV3::AdmissionPaused => {
            RuntimeGatewayReadyObservationErrorV1::AdmissionPaused
        }
        GatewayControlTransitionErrorV3::AdmissionNotPaused => {
            RuntimeGatewayReadyObservationErrorV1::AdmissionNotPaused
        }
        GatewayControlTransitionErrorV3::Draining => {
            RuntimeGatewayReadyObservationErrorV1::Draining
        }
        GatewayControlTransitionErrorV3::Stopped => RuntimeGatewayReadyObservationErrorV1::Stopped,
        GatewayControlTransitionErrorV3::ConnectionEpochOverflow => {
            RuntimeGatewayReadyObservationErrorV1::ConnectionEpochOverflow
        }
        GatewayControlTransitionErrorV3::AdmissionRevisionOverflow => {
            RuntimeGatewayReadyObservationErrorV1::AdmissionRevisionOverflow
        }
        GatewayControlTransitionErrorV3::AdmissionSequenceOverflow => {
            RuntimeGatewayReadyObservationErrorV1::AdmissionSequenceOverflow
        }
        GatewayControlTransitionErrorV3::LifecycleOverflow => {
            RuntimeGatewayReadyObservationErrorV1::LifecycleOverflow
        }
        GatewayControlTransitionErrorV3::LifecycleClosed => {
            RuntimeGatewayReadyObservationErrorV1::LifecycleClosed
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;
    use std::sync::{Arc, Mutex};

    use automation_runtime::{
        GatewayCommandAckV3, GatewayControlConfigV3, GatewayControlErrorV3,
        GatewayControlTransitionErrorV3, GatewayDisconnectKindV3, GatewayDrainCauseV3,
        GatewayInvalidationSignalV3, GatewayPauseTokenV3, GatewayReadyKindV3,
        GatewayRuntimeCommandOutcomeV3, GatewaySynchronousInvalidatorV3,
    };
    use automation_runtime_convergence::ProcessInstanceId;
    use automation_runtime_worker::{
        RuntimeGatewayClosedLifecycleV2, RuntimeGatewayClosedSnapshotV2,
        RuntimeGatewayEmergencyCauseV2,
    };

    use crate::gateway_owner_startup_watchdog::RuntimeGatewayOwnerEmergencyInvalidatorV1;

    use super::{
        compose_with_control_config, RuntimeGatewayBootstrapV1, RuntimeGatewayInvalidationBridgeV2,
        RuntimeGatewayReadyObservationErrorV1,
    };

    struct TestPauseTokenV1(GatewayPauseTokenV3);

    fn bootstrap() -> RuntimeGatewayBootstrapV1 {
        compose_with_control_config(
            ProcessInstanceId::parse("runtime-process:1").unwrap(),
            GatewayControlConfigV3::default(),
        )
    }

    fn connect(bootstrap: &mut RuntimeGatewayBootstrapV1, kind: GatewayReadyKindV3) -> u64 {
        bootstrap
            ._runtime
            ._inner
            .mark_connected(kind)
            .unwrap()
            .get()
    }

    async fn pause(bootstrap: &mut RuntimeGatewayBootstrapV1) -> TestPauseTokenV1 {
        let control = &bootstrap.adapter.control;
        let runtime = &mut bootstrap._runtime._inner;
        let (acknowledgement, outcome) =
            tokio::join!(control.pause_admission(), runtime.process_next_command());
        let acknowledgement = acknowledgement.unwrap();
        assert_eq!(
            outcome,
            GatewayRuntimeCommandOutcomeV3::Applied(acknowledgement.clone())
        );
        match acknowledgement {
            GatewayCommandAckV3::Paused { resume_token, .. } => TestPauseTokenV1(resume_token),
            GatewayCommandAckV3::AdmissionResumed { .. } | GatewayCommandAckV3::Draining { .. } => {
                panic!("unexpected gateway acknowledgement")
            }
        }
    }

    async fn resume(
        bootstrap: &mut RuntimeGatewayBootstrapV1,
        token: &TestPauseTokenV1,
    ) -> Result<GatewayCommandAckV3, GatewayControlErrorV3> {
        let control = &bootstrap.adapter.control;
        let runtime = &mut bootstrap._runtime._inner;
        let (acknowledgement, outcome) = tokio::join!(
            control.resume_admission(&token.0),
            runtime.process_next_command()
        );
        match &acknowledgement {
            Ok(acknowledgement) => assert_eq!(
                outcome,
                GatewayRuntimeCommandOutcomeV3::Applied(acknowledgement.clone())
            ),
            Err(GatewayControlErrorV3::Transition(error)) => {
                assert_eq!(outcome, GatewayRuntimeCommandOutcomeV3::Rejected(*error))
            }
            Err(GatewayControlErrorV3::RuntimeStopped)
            | Err(GatewayControlErrorV3::AcknowledgementLost) => {
                panic!("unexpected gateway control failure")
            }
        }
        acknowledgement
    }

    fn disconnect(bootstrap: &mut RuntimeGatewayBootstrapV1) {
        bootstrap
            ._runtime
            ._inner
            .mark_disconnected(GatewayDisconnectKindV3::Reconnect)
            .unwrap();
    }

    #[test]
    fn invalidation_bridge_maps_every_signal_and_shutdown_is_terminal() {
        let closed = Arc::new(Mutex::new(RuntimeGatewayClosedLifecycleV2::starting()));
        let bridge = RuntimeGatewayInvalidationBridgeV2 {
            closed_lifecycle: closed.clone(),
        };
        for (index, (signal, cause)) in [
            (
                GatewayInvalidationSignalV3::AdmissionPaused,
                RuntimeGatewayEmergencyCauseV2::CapabilityNotReady,
            ),
            (
                GatewayInvalidationSignalV3::Disconnected(GatewayDisconnectKindV3::Close),
                RuntimeGatewayEmergencyCauseV2::TransportDisconnected,
            ),
            (
                GatewayInvalidationSignalV3::ControlOrphaned,
                RuntimeGatewayEmergencyCauseV2::ControlOrphaned,
            ),
            (
                GatewayInvalidationSignalV3::Draining(GatewayDrainCauseV3::ControlOrphaned),
                RuntimeGatewayEmergencyCauseV2::ControlOrphaned,
            ),
            (
                GatewayInvalidationSignalV3::Draining(GatewayDrainCauseV3::LifecycleClosed),
                RuntimeGatewayEmergencyCauseV2::ControlOrphaned,
            ),
            (
                GatewayInvalidationSignalV3::Draining(GatewayDrainCauseV3::ConnectionEpochOverflow),
                RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
            ),
            (
                GatewayInvalidationSignalV3::Draining(
                    GatewayDrainCauseV3::AdmissionRevisionOverflow,
                ),
                RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
            ),
            (
                GatewayInvalidationSignalV3::Draining(
                    GatewayDrainCauseV3::AdmissionSequenceOverflow,
                ),
                RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
            ),
            (
                GatewayInvalidationSignalV3::Draining(GatewayDrainCauseV3::LifecycleOverflow),
                RuntimeGatewayEmergencyCauseV2::CapabilityNotReady,
            ),
            (
                GatewayInvalidationSignalV3::Draining(GatewayDrainCauseV3::RuntimeFailure),
                RuntimeGatewayEmergencyCauseV2::CapabilityNotReady,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            bridge.invalidate(signal);
            assert_eq!(
                closed.lock().unwrap().snapshot(),
                RuntimeGatewayClosedSnapshotV2::Emergency {
                    generation:
                        automation_runtime_worker::RuntimeGatewayCoordinatorGenerationV2::new(
                            NonZeroU64::new(index as u64 + 2).unwrap(),
                        ),
                    cause,
                }
            );
        }
        bridge.invalidate(GatewayInvalidationSignalV3::Draining(
            GatewayDrainCauseV3::Commanded,
        ));
        let shutdown = closed.lock().unwrap().snapshot();
        assert!(matches!(
            shutdown,
            RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
        ));
        bridge.invalidate(GatewayInvalidationSignalV3::Stopped(
            GatewayDrainCauseV3::RuntimeFailure,
        ));
        assert_eq!(closed.lock().unwrap().snapshot(), shutdown);

        let stopped = Arc::new(Mutex::new(RuntimeGatewayClosedLifecycleV2::starting()));
        RuntimeGatewayInvalidationBridgeV2 {
            closed_lifecycle: stopped.clone(),
        }
        .invalidate(GatewayInvalidationSignalV3::Stopped(
            GatewayDrainCauseV3::RuntimeFailure,
        ));
        assert!(matches!(
            stopped.lock().unwrap().snapshot(),
            RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
        ));
    }

    #[test]
    fn composition_starts_closed_and_cannot_observe_ready_evidence() {
        let bootstrap = bootstrap();
        assert!(matches!(
            bootstrap.closed_snapshot(),
            RuntimeGatewayClosedSnapshotV2::Emergency {
                cause: RuntimeGatewayEmergencyCauseV2::Starting,
                ..
            }
        ));
        assert_eq!(
            bootstrap.observe_current_ready_attestation(),
            Err(RuntimeGatewayReadyObservationErrorV1::AdmissionPaused)
        );
    }

    #[test]
    fn owner_invalidation_is_one_way_and_blocks_ready_observation() {
        let mut bootstrap = bootstrap();
        let invalidator = bootstrap.owner_invalidator.take().unwrap();
        invalidator.invalidate_gateway_ownership();
        invalidator.invalidate_gateway_ownership();

        assert!(matches!(
            bootstrap.closed_snapshot(),
            RuntimeGatewayClosedSnapshotV2::Emergency {
                generation,
                cause: RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
            } if generation.get() == 2
        ));
        assert_eq!(
            bootstrap.observe_current_ready_attestation(),
            Err(RuntimeGatewayReadyObservationErrorV1::OwnershipUncertain)
        );
    }

    #[test]
    fn connect_remains_paused_until_an_explicit_resume() {
        let mut bootstrap = bootstrap();
        assert_eq!(connect(&mut bootstrap, GatewayReadyKindV3::Ready), 1);
        assert_eq!(
            bootstrap.observe_current_ready_attestation(),
            Err(RuntimeGatewayReadyObservationErrorV1::AdmissionPaused)
        );
    }

    #[tokio::test]
    async fn explicit_resume_maps_exact_ready_evidence_without_opening_worker_state() {
        let mut bootstrap = bootstrap();
        assert_eq!(connect(&mut bootstrap, GatewayReadyKindV3::Ready), 1);
        let token = pause(&mut bootstrap).await;
        assert!(matches!(
            resume(&mut bootstrap, &token).await.unwrap(),
            GatewayCommandAckV3::AdmissionResumed { .. }
        ));
        let observed = bootstrap.observe_current_ready_attestation().unwrap();
        assert_eq!(observed.process_instance_id.as_str(), "runtime-process:1");
        assert_eq!(observed.connection_epoch.get(), 1);
        assert_eq!(observed.admission_revision.get(), 2);
        assert_eq!(observed.connected_event_sequence.get(), 1);
        assert_eq!(observed.resume_sequence.get(), 3);
        assert!(observed.was_explicitly_resumed());
        assert!(bootstrap.ready_attestation_is_current(&observed));
        assert!(matches!(
            bootstrap.closed_snapshot(),
            RuntimeGatewayClosedSnapshotV2::Emergency {
                cause: RuntimeGatewayEmergencyCauseV2::Starting,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn disconnect_invalidates_ready_evidence_and_requires_a_new_resume() {
        let mut bootstrap = bootstrap();
        connect(&mut bootstrap, GatewayReadyKindV3::Ready);
        let token = pause(&mut bootstrap).await;
        resume(&mut bootstrap, &token).await.unwrap();
        let previous = bootstrap.observe_current_ready_attestation().unwrap();
        let previous_revision = previous.admission_revision.get();
        disconnect(&mut bootstrap);
        assert!(!bootstrap.ready_attestation_is_current(&previous));
        assert_eq!(
            bootstrap.observe_current_ready_attestation(),
            Err(RuntimeGatewayReadyObservationErrorV1::AdmissionPaused)
        );
        let closed = bootstrap.closed_snapshot();
        assert_eq!(closed.generation().get(), 2);
        assert!(matches!(
            closed,
            RuntimeGatewayClosedSnapshotV2::Emergency {
                cause: RuntimeGatewayEmergencyCauseV2::TransportDisconnected,
                ..
            }
        ));
        assert_eq!(connect(&mut bootstrap, GatewayReadyKindV3::Ready), 2);
        assert_eq!(
            bootstrap.observe_current_ready_attestation(),
            Err(RuntimeGatewayReadyObservationErrorV1::AdmissionPaused)
        );
        assert!(
            bootstrap
                .adapter
                .control
                .current_admission_snapshot()
                .admission_revision()
                .get()
                > previous_revision
        );
    }

    #[tokio::test]
    async fn resumed_gateway_kind_is_preserved_literally() {
        let mut bootstrap = bootstrap();
        connect(&mut bootstrap, GatewayReadyKindV3::Resumed);
        let token = pause(&mut bootstrap).await;
        resume(&mut bootstrap, &token).await.unwrap();
        assert_eq!(
            bootstrap.observe_current_ready_attestation().unwrap().kind,
            automation_runtime_controller::RuntimeGatewayReadyKindV2::Resumed
        );
    }

    #[tokio::test]
    async fn predecessor_pause_token_cannot_resume_a_successor_connection() {
        let mut bootstrap = bootstrap();
        connect(&mut bootstrap, GatewayReadyKindV3::Ready);
        let predecessor = pause(&mut bootstrap).await;
        resume(&mut bootstrap, &predecessor).await.unwrap();
        disconnect(&mut bootstrap);
        connect(&mut bootstrap, GatewayReadyKindV3::Ready);
        let successor = pause(&mut bootstrap).await;
        assert_eq!(
            resume(&mut bootstrap, &predecessor).await,
            Err(GatewayControlErrorV3::Transition(
                GatewayControlTransitionErrorV3::StaleConnectionEpoch
            ))
        );
        resume(&mut bootstrap, &successor).await.unwrap();
        assert_eq!(
            bootstrap
                .observe_current_ready_attestation()
                .unwrap()
                .connection_epoch
                .get(),
            2
        );
    }

    #[tokio::test]
    async fn dropping_control_owner_forces_the_runtime_half_closed() {
        let RuntimeGatewayBootstrapV1 {
            adapter,
            mut _runtime,
            ..
        } = bootstrap();
        let closed = adapter.closed_lifecycle.clone();
        drop(adapter);
        assert!(matches!(
            closed.lock().unwrap().snapshot(),
            RuntimeGatewayClosedSnapshotV2::Emergency {
                cause: RuntimeGatewayEmergencyCauseV2::ControlOrphaned,
                ..
            }
        ));
        assert_eq!(
            _runtime._inner.process_next_command().await,
            GatewayRuntimeCommandOutcomeV3::ControlOrphaned
        );
        assert!(matches!(
            _runtime._inner.current_connection(),
            automation_runtime::GatewayConnectionStateV3::Draining { .. }
        ));
        drop(_runtime);
        assert!(matches!(
            closed.lock().unwrap().snapshot(),
            RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
        ));
    }
}
