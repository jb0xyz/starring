use std::collections::BTreeMap;
use std::fmt::{Debug, Formatter};
use std::num::{NonZeroU64, NonZeroUsize};
use std::time::{Duration, Instant};

use automation_runtime_controller::{
    RuntimeBarrierPauseWitnessV2, RuntimeCertificationOperationIdV2,
    RuntimeCertificationReservationInputV2, RuntimeCertificationReservationScopeLookupV2,
    RuntimeConvergenceSessionV1, RuntimeExecutionReceiptV1,
    RuntimeIngressOpenAcknowledgementLeaseDurationV2, RuntimePanelEvidenceV2,
    RuntimeRouteAdmissionAttestationV2, RuntimeServingRouteAttestationV2, RuntimeServingSlotV2,
};
use automation_runtime_convergence::RuntimeDeploymentPhaseV1;
use automation_runtime_execution_postgres::PostgresPreparedRuntimeCertificationV2;
use automation_runtime_worker::{
    RuntimeCertificationFinalizationOutcomeV2, RuntimeCertificationRecoveryResolutionV2,
    RuntimeCertificationReservationProposalV2, RuntimeCommittedCertificationV2,
    RuntimeIngressOpenAcknowledgementPortV2, RuntimeReservedCertificationV2,
    RuntimeRouteLifecycleV2, RuntimeRouteWitnessV2,
};
use chrono::{DateTime, Utc};
use tokio::task::JoinSet;

use crate::certification_identity::{
    RuntimeBarrierIdGenerationAuthorityV1, RuntimeCertificationOperationIdGenerationAuthorityV2,
};
use crate::closed_recovery::{
    RuntimeClosedRecoveryAdmissionAcknowledgingProcessV2,
    RuntimeClosedRecoveryCertificationFrozenServingOpenProcessV2,
    RuntimeClosedRecoveryEmptyOpenAcknowledgementRefreshV2,
    RuntimeClosedRecoveryIngressAcknowledgementOutcomeV2,
    RuntimeClosedRecoveryServingAcknowledgementEvidenceV2,
    RuntimeClosedRecoveryServingOpenAcknowledgementRefreshV2,
    RuntimeClosedRecoverySupervisedEmptyOpenProcessV2,
    RuntimeClosedRecoverySupervisedServingOpenProcessV2,
};
use crate::gateway::{
    RuntimeDiscordCertificationBarrierBPauseOutcomeV2,
    RuntimeDiscordCertificationBarrierBResumeOutcomeV2,
};
use crate::maintenance_ingress_gate::{
    RuntimeMaintenanceIngressGateOpeningAuthorityV2, RuntimeMaintenanceIngressGateStageV2,
};
use crate::runtime_controller::{
    RuntimeControllerCertificationHandoffRejectionV2, RuntimeControllerCertificationHandoffReplyV2,
    RuntimeControllerCertificationHandoffV2, RuntimeServingControllerSupervisorV2,
};
use crate::serving_heartbeat_monitor::{
    start_runtime_process_serving_heartbeat_monitor_v2, RuntimeServingHeartbeatExternalObserversV2,
    RuntimeServingHeartbeatMonitorConfigV2, RuntimeServingHeartbeatMonitorOutcomeV2,
    RuntimeServingHeartbeatMonitorV2, RuntimeServingHeartbeatTerminalStatusV2,
};

use super::certification_finalizer::RuntimeReservedCertificationFinalizerSlotV2;
use super::observation::{
    collect_recovery_resume_database_evidence_v2, exact_reobserve_ingress_acknowledgement_v2,
    execute_ingress_acknowledgement_v2, finish_production_handoff_transition_v2,
    ingress_acknowledgement_schedule_v2, shutdown_certification_frozen_serving_open_process_v2,
    shutdown_orphaned_admission_process_v2, shutdown_orphaned_empty_open_process_v2,
    shutdown_process_without_lifecycle_v2, shutdown_refreshing_empty_open_process_v2,
    shutdown_refreshing_serving_open_process_v2, shutdown_serving_open_process_v2,
    RuntimeEmptyOpenMonitorV2, RuntimeExactIngressAcknowledgementReobservationV3,
    RuntimeIngressAcknowledgementCleanupV2, RuntimeIngressAcknowledgementScheduleV2,
    RuntimeLifecycleLostRegistryObservationV2,
    RuntimeProcessIngressAcknowledgementExecutionFailureV2, RuntimeProcessProductionHandoffErrorV2,
    RuntimeProcessProductionHandoffFailureV2,
};
use super::serving::stop_runtime_controller_before_cleanup_v2;
use super::serving::RuntimeServingOpenProcessV2;
use super::{complete_certification_finalizer_job_v2, RuntimeProcessFoundationV1};

const INGRESS_ACKNOWLEDGEMENT_LEASE_V2: Duration = Duration::from_secs(10);

pub(super) enum RuntimeServingCertificationHandleOutcomeV2 {
    Completed {
        process: Box<RuntimeServingOpenProcessV2>,
        gateway_monitor: RuntimeEmptyOpenMonitorV2,
        reply: RuntimeControllerCertificationHandoffReplyV2,
    },
    Rejected {
        process: Box<RuntimeServingOpenProcessV2>,
        gateway_monitor: RuntimeEmptyOpenMonitorV2,
        reply: RuntimeControllerCertificationHandoffReplyV2,
    },
    Terminal(RuntimeProcessProductionHandoffErrorV2),
}

impl Debug for RuntimeServingCertificationHandleOutcomeV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingCertificationHandleOutcomeV2(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeServingCertificationFailureV2 {
    Deadline,
    Finalizer,
    Database,
    Gate,
    Owner,
    Gateway,
    Registry,
    Acknowledgement,
    Protocol,
}

impl RuntimeServingCertificationFailureV2 {
    const fn transition_v2(self) -> RuntimeProcessProductionHandoffFailureV2 {
        match self {
            Self::Deadline => RuntimeProcessProductionHandoffFailureV2::OperationDeadlineElapsed,
            Self::Finalizer => RuntimeProcessProductionHandoffFailureV2::FinalizerTerminal,
            Self::Database => RuntimeProcessProductionHandoffFailureV2::Database,
            Self::Gate => RuntimeProcessProductionHandoffFailureV2::MaintenanceGate,
            Self::Owner => RuntimeProcessProductionHandoffFailureV2::Owner,
            Self::Gateway => RuntimeProcessProductionHandoffFailureV2::Gateway,
            Self::Registry => RuntimeProcessProductionHandoffFailureV2::Registry,
            Self::Acknowledgement | Self::Protocol => {
                RuntimeProcessProductionHandoffFailureV2::ProtocolViolation
            }
        }
    }
}

struct RuntimeServingCertificationAcceptedV2 {
    process: RuntimeServingOpenProcessV2,
    gateway_monitor: RuntimeEmptyOpenMonitorV2,
    staged: crate::registry::RuntimeRegistryReplacementRouteV2,
    session: RuntimeConvergenceSessionV1,
    permit: automation_runtime_worker::RuntimeServingSlotWorkPermitV2,
    evidence: automation_runtime_worker::RuntimeExactTargetEvidenceV2,
    witness: RuntimeRouteWitnessV2,
    finalizer_slot: RuntimeReservedCertificationFinalizerSlotV2,
    operation_id: RuntimeCertificationOperationIdV2,
    barrier_id: automation_runtime_controller::RuntimeBarrierIdV1,
    acceptance_deadline: Instant,
    serving_lease_for: Duration,
}

struct RuntimeServingCertificationSuccessV2 {
    process: RuntimeServingOpenProcessV2,
    gateway_monitor: RuntimeEmptyOpenMonitorV2,
    serving: automation_runtime_controller::RuntimeServingReceiptV2,
}

struct RuntimeServingCertificationProcessCoreV2 {
    controller: RuntimeServingControllerSupervisorV2,
    discord: crate::discord::RuntimeDiscordProcessSupervisorV2,
    foundation: RuntimeProcessFoundationV1,
    readiness: crate::health::RuntimeHealthReadinessPublisherV2,
    ingress_acknowledgement: super::RuntimeProcessIngressAcknowledgementSupervisorV2,
    acknowledgement_safety:
        crate::ingress_acknowledgement_safety::RuntimeIngressAcknowledgementSafetyMonitorV2,
    certification_monitors: RuntimeServingCertificationMonitorSetV2,
    process_generation: NonZeroU64,
}

struct RuntimeServingCertificationNormalProcessV2<G> {
    core: RuntimeServingCertificationProcessCoreV2,
    lifecycle: RuntimeClosedRecoverySupervisedServingOpenProcessV2,
    maintenance_ingress: G,
    gateway_monitor: Option<RuntimeEmptyOpenMonitorV2>,
}

struct RuntimeServingCertificationFrozenProcessV2<G> {
    core: RuntimeServingCertificationProcessCoreV2,
    lifecycle: RuntimeClosedRecoveryCertificationFrozenServingOpenProcessV2,
    maintenance_ingress: G,
    heartbeat: Option<RuntimeServingHeartbeatMonitorV2>,
}

impl RuntimeServingCertificationProcessCoreV2 {
    fn from_process_v2(
        process: RuntimeServingOpenProcessV2,
    ) -> (
        Self,
        RuntimeClosedRecoverySupervisedServingOpenProcessV2,
        crate::maintenance_ingress_gate::RuntimeMaintenanceIngressGateOpenAuthorityV2,
        RuntimeIngressAcknowledgementScheduleV2,
    ) {
        let RuntimeServingOpenProcessV2 {
            controller,
            discord,
            foundation,
            lifecycle,
            maintenance_ingress,
            readiness,
            ingress_acknowledgement,
            acknowledgement_schedule,
            acknowledgement_safety,
            certification_monitors,
            process_generation,
        } = process;
        (
            Self {
                controller,
                discord,
                foundation,
                readiness,
                ingress_acknowledgement,
                acknowledgement_safety,
                certification_monitors,
                process_generation,
            },
            lifecycle,
            maintenance_ingress,
            acknowledgement_schedule,
        )
    }

    async fn stop_monitors_for_cleanup_v2(
        &mut self,
        heartbeat: Option<RuntimeServingHeartbeatMonitorV2>,
    ) {
        let deadline = self
            .foundation
            .shutdown_observer_v1()
            .observed()
            .map(|observation| self.foundation.effective_shutdown_deadline_v1(observation))
            .unwrap_or_else(Instant::now);
        if let Some(heartbeat) = heartbeat {
            let _outcome = heartbeat.stop_until_v2(deadline).await;
        }
        let _outcomes = self
            .certification_monitors
            .stop_all_until_v2(deadline)
            .await;
    }

    async fn cleanup_without_lifecycle_v2<G>(
        mut self,
        maintenance_ingress: G,
        heartbeat: Option<RuntimeServingHeartbeatMonitorV2>,
        transition: RuntimeProcessProductionHandoffFailureV2,
    ) -> RuntimeProcessProductionHandoffErrorV2 {
        self.readiness.remove_readiness_v2();
        drop(maintenance_ingress);
        stop_runtime_controller_before_cleanup_v2(&mut self.controller, &mut self.foundation).await;
        self.stop_monitors_for_cleanup_v2(heartbeat).await;
        let Self {
            controller,
            discord,
            foundation,
            readiness,
            ingress_acknowledgement,
            acknowledgement_safety,
            certification_monitors,
            process_generation,
        } = self;
        drop((controller, readiness, certification_monitors));
        let cleanup = shutdown_process_without_lifecycle_v2(
            foundation,
            discord,
            RuntimeIngressAcknowledgementCleanupV2::new_v2(
                ingress_acknowledgement,
                Some(acknowledgement_safety),
            ),
            RuntimeLifecycleLostRegistryObservationV2::Serving,
            process_generation,
        )
        .await;
        finish_production_handoff_transition_v2(transition, cleanup)
    }

    async fn cleanup_refreshing_v2<G>(
        mut self,
        lifecycle: RuntimeClosedRecoveryServingOpenAcknowledgementRefreshV2,
        maintenance_ingress: G,
        heartbeat: Option<RuntimeServingHeartbeatMonitorV2>,
        transition: RuntimeProcessProductionHandoffFailureV2,
    ) -> RuntimeProcessProductionHandoffErrorV2 {
        self.readiness.remove_readiness_v2();
        stop_runtime_controller_before_cleanup_v2(&mut self.controller, &mut self.foundation).await;
        self.stop_monitors_for_cleanup_v2(heartbeat).await;
        let Self {
            controller,
            discord,
            foundation,
            readiness,
            ingress_acknowledgement,
            acknowledgement_safety,
            certification_monitors,
            process_generation,
        } = self;
        drop((controller, certification_monitors));
        let cleanup = shutdown_refreshing_serving_open_process_v2(
            foundation,
            discord,
            lifecycle,
            RuntimeIngressAcknowledgementCleanupV2::new_v2(
                ingress_acknowledgement,
                Some(acknowledgement_safety),
            ),
            maintenance_ingress,
            readiness,
            process_generation,
        )
        .await;
        finish_production_handoff_transition_v2(transition, cleanup)
    }

    async fn cleanup_admission_v2<G>(
        mut self,
        lifecycle: RuntimeClosedRecoveryAdmissionAcknowledgingProcessV2,
        maintenance_ingress: G,
        heartbeat: Option<RuntimeServingHeartbeatMonitorV2>,
        transition: RuntimeProcessProductionHandoffFailureV2,
    ) -> RuntimeProcessProductionHandoffErrorV2 {
        self.readiness.remove_readiness_v2();
        stop_runtime_controller_before_cleanup_v2(&mut self.controller, &mut self.foundation).await;
        self.stop_monitors_for_cleanup_v2(heartbeat).await;
        let Self {
            controller,
            discord,
            foundation,
            readiness,
            ingress_acknowledgement,
            acknowledgement_safety,
            certification_monitors,
            process_generation,
        } = self;
        drop((controller, certification_monitors));
        let cleanup = shutdown_orphaned_admission_process_v2(
            foundation,
            discord,
            lifecycle,
            RuntimeIngressAcknowledgementCleanupV2::new_v2(
                ingress_acknowledgement,
                Some(acknowledgement_safety),
            ),
            maintenance_ingress,
            readiness,
            process_generation,
        )
        .await;
        finish_production_handoff_transition_v2(transition, cleanup)
    }

    async fn cleanup_empty_v2<G>(
        mut self,
        lifecycle: RuntimeClosedRecoverySupervisedEmptyOpenProcessV2,
        maintenance_ingress: G,
        heartbeat: Option<RuntimeServingHeartbeatMonitorV2>,
        transition: RuntimeProcessProductionHandoffFailureV2,
    ) -> RuntimeProcessProductionHandoffErrorV2 {
        self.readiness.remove_readiness_v2();
        stop_runtime_controller_before_cleanup_v2(&mut self.controller, &mut self.foundation).await;
        self.stop_monitors_for_cleanup_v2(heartbeat).await;
        let Self {
            controller,
            discord,
            foundation,
            readiness,
            ingress_acknowledgement,
            acknowledgement_safety,
            certification_monitors,
            process_generation,
        } = self;
        drop((controller, certification_monitors));
        let cleanup = shutdown_orphaned_empty_open_process_v2(
            foundation,
            discord,
            lifecycle,
            RuntimeIngressAcknowledgementCleanupV2::new_v2(
                ingress_acknowledgement,
                Some(acknowledgement_safety),
            ),
            maintenance_ingress,
            readiness,
            process_generation,
        )
        .await;
        finish_production_handoff_transition_v2(transition, cleanup)
    }

    async fn cleanup_refreshing_empty_v2<G>(
        mut self,
        lifecycle: RuntimeClosedRecoveryEmptyOpenAcknowledgementRefreshV2,
        maintenance_ingress: G,
        heartbeat: Option<RuntimeServingHeartbeatMonitorV2>,
        transition: RuntimeProcessProductionHandoffFailureV2,
    ) -> RuntimeProcessProductionHandoffErrorV2 {
        self.readiness.remove_readiness_v2();
        stop_runtime_controller_before_cleanup_v2(&mut self.controller, &mut self.foundation).await;
        self.stop_monitors_for_cleanup_v2(heartbeat).await;
        let Self {
            controller,
            discord,
            foundation,
            readiness,
            ingress_acknowledgement,
            acknowledgement_safety,
            certification_monitors,
            process_generation,
        } = self;
        drop((controller, certification_monitors));
        let cleanup = shutdown_refreshing_empty_open_process_v2(
            foundation,
            discord,
            lifecycle,
            RuntimeIngressAcknowledgementCleanupV2::new_v2(
                ingress_acknowledgement,
                Some(acknowledgement_safety),
            ),
            maintenance_ingress,
            readiness,
            process_generation,
        )
        .await;
        finish_production_handoff_transition_v2(transition, cleanup)
    }
}

impl<G> RuntimeServingCertificationNormalProcessV2<G> {
    async fn cleanup_v2(
        mut self,
        heartbeat: Option<RuntimeServingHeartbeatMonitorV2>,
        transition: RuntimeProcessProductionHandoffFailureV2,
    ) -> RuntimeProcessProductionHandoffErrorV2 {
        self.core.readiness.remove_readiness_v2();
        if let Some(gateway_monitor) = self.gateway_monitor.take() {
            gateway_monitor.stop_v2().await;
        }
        stop_runtime_controller_before_cleanup_v2(
            &mut self.core.controller,
            &mut self.core.foundation,
        )
        .await;
        self.core.stop_monitors_for_cleanup_v2(heartbeat).await;
        let RuntimeServingCertificationNormalProcessV2 {
            core:
                RuntimeServingCertificationProcessCoreV2 {
                    controller,
                    discord,
                    foundation,
                    readiness,
                    ingress_acknowledgement,
                    acknowledgement_safety,
                    certification_monitors,
                    process_generation,
                },
            lifecycle,
            maintenance_ingress,
            gateway_monitor: _,
        } = self;
        drop((controller, certification_monitors));
        let cleanup = shutdown_serving_open_process_v2(
            foundation,
            discord,
            lifecycle,
            RuntimeIngressAcknowledgementCleanupV2::new_v2(
                ingress_acknowledgement,
                Some(acknowledgement_safety),
            ),
            maintenance_ingress,
            readiness,
            process_generation,
        )
        .await;
        finish_production_handoff_transition_v2(transition, cleanup)
    }
}

impl<G> RuntimeServingCertificationFrozenProcessV2<G> {
    async fn cleanup_v2(
        mut self,
        transition: RuntimeProcessProductionHandoffFailureV2,
    ) -> RuntimeProcessProductionHandoffErrorV2 {
        self.core.readiness.remove_readiness_v2();
        stop_runtime_controller_before_cleanup_v2(
            &mut self.core.controller,
            &mut self.core.foundation,
        )
        .await;
        self.core
            .stop_monitors_for_cleanup_v2(self.heartbeat.take())
            .await;
        let RuntimeServingCertificationFrozenProcessV2 {
            core:
                RuntimeServingCertificationProcessCoreV2 {
                    controller,
                    discord,
                    foundation,
                    readiness,
                    ingress_acknowledgement,
                    acknowledgement_safety,
                    certification_monitors,
                    process_generation,
                },
            lifecycle,
            maintenance_ingress,
            heartbeat: _,
        } = self;
        drop((controller, certification_monitors));
        let cleanup = shutdown_certification_frozen_serving_open_process_v2(
            foundation,
            discord,
            lifecycle,
            RuntimeIngressAcknowledgementCleanupV2::new_v2(
                ingress_acknowledgement,
                Some(acknowledgement_safety),
            ),
            maintenance_ingress,
            readiness,
            process_generation,
        )
        .await;
        finish_production_handoff_transition_v2(transition, cleanup)
    }
}

pub(super) async fn handle_certification_v2(
    process: RuntimeServingOpenProcessV2,
    gateway_monitor: RuntimeEmptyOpenMonitorV2,
    handoff: Box<RuntimeControllerCertificationHandoffV2>,
) -> RuntimeServingCertificationHandleOutcomeV2 {
    let acceptance_deadline =
        match runtime_certification_deadline_v2(handoff.acceptance_deadline_v2()) {
            Some(deadline) => deadline,
            None => {
                return RuntimeServingCertificationHandleOutcomeV2::Rejected {
                    process: Box::new(process),
                    gateway_monitor,
                    reply: handoff.reject_v2(
                        RuntimeControllerCertificationHandoffRejectionV2::DeadlineElapsed,
                    ),
                };
            }
        };
    if handoff.ensure_exact_awaiting_v2().is_err() {
        return RuntimeServingCertificationHandleOutcomeV2::Rejected {
            process: Box::new(process),
            gateway_monitor,
            reply: handoff
                .reject_v2(RuntimeControllerCertificationHandoffRejectionV2::StaleAuthority),
        };
    }
    let slot = RuntimeServingSlotV2::from_target(&handoff.route_witness_v2().identity.target);
    if process
        .certification_monitors
        .ensure_available_v2(&slot)
        .is_err()
    {
        return RuntimeServingCertificationHandleOutcomeV2::Rejected {
            process: Box::new(process),
            gateway_monitor,
            reply: handoff.reject_v2(RuntimeControllerCertificationHandoffRejectionV2::Unavailable),
        };
    }
    let finalizer_slot = match process
        .foundation
        .certification_finalizer_port_v2()
        .and_then(|port| port.reserve_certification_finalizer_slot_v2().ok())
    {
        Some(slot) => slot,
        None => {
            return RuntimeServingCertificationHandleOutcomeV2::Rejected {
                process: Box::new(process),
                gateway_monitor,
                reply: handoff
                    .reject_v2(RuntimeControllerCertificationHandoffRejectionV2::Unavailable),
            };
        }
    };
    let mut operation_identity = RuntimeCertificationOperationIdGenerationAuthorityV2::new();
    let mut barrier_identity = RuntimeBarrierIdGenerationAuthorityV1::new();
    let operation_id = match operation_identity.generate_v2() {
        Ok(operation_id) => operation_id,
        Err(_) => {
            return RuntimeServingCertificationHandleOutcomeV2::Rejected {
                process: Box::new(process),
                gateway_monitor,
                reply: handoff
                    .reject_v2(RuntimeControllerCertificationHandoffRejectionV2::Unavailable),
            };
        }
    };
    let barrier_id = match barrier_identity.generate_v1() {
        Ok(barrier_id) => barrier_id,
        Err(_) => {
            return RuntimeServingCertificationHandleOutcomeV2::Rejected {
                process: Box::new(process),
                gateway_monitor,
                reply: handoff
                    .reject_v2(RuntimeControllerCertificationHandoffRejectionV2::Unavailable),
            };
        }
    };
    let serving_lease_for = handoff.serving_lease_for_v2();
    let (staged, session, permit, evidence, _hydrated, witness) = handoff.accept_v2();
    let accepted = RuntimeServingCertificationAcceptedV2 {
        process,
        gateway_monitor,
        staged,
        session,
        permit,
        evidence,
        witness,
        finalizer_slot,
        operation_id,
        barrier_id,
        acceptance_deadline,
        serving_lease_for,
    };
    match execute_certification_v2(accepted).await {
        Ok(success) => RuntimeServingCertificationHandleOutcomeV2::Completed {
            process: Box::new(success.process),
            gateway_monitor: success.gateway_monitor,
            reply: RuntimeControllerCertificationHandoffReplyV2::accepted_v2(success.serving),
        },
        Err(error) => RuntimeServingCertificationHandleOutcomeV2::Terminal(error),
    }
}

fn runtime_certification_deadline_v2(deadline: DateTime<Utc>) -> Option<Instant> {
    let remaining = deadline.signed_duration_since(Utc::now()).to_std().ok()?;
    if remaining.is_zero() {
        return None;
    }
    Instant::now().checked_add(remaining)
}

enum RuntimeServingCertificationAcknowledgementFailureV2 {
    Normal {
        lifecycle: Box<RuntimeClosedRecoverySupervisedServingOpenProcessV2>,
        opening: RuntimeMaintenanceIngressGateOpeningAuthorityV2,
        transition: RuntimeProcessProductionHandoffFailureV2,
    },
    Refreshing {
        lifecycle: Box<RuntimeClosedRecoveryServingOpenAcknowledgementRefreshV2>,
        opening: RuntimeMaintenanceIngressGateOpeningAuthorityV2,
        transition: RuntimeProcessProductionHandoffFailureV2,
    },
    Admission {
        lifecycle: Box<RuntimeClosedRecoveryAdmissionAcknowledgingProcessV2>,
        opening: RuntimeMaintenanceIngressGateOpeningAuthorityV2,
        transition: RuntimeProcessProductionHandoffFailureV2,
    },
    Empty {
        lifecycle: Box<RuntimeClosedRecoverySupervisedEmptyOpenProcessV2>,
        opening: RuntimeMaintenanceIngressGateOpeningAuthorityV2,
        transition: RuntimeProcessProductionHandoffFailureV2,
    },
    RefreshingEmpty {
        lifecycle: Box<RuntimeClosedRecoveryEmptyOpenAcknowledgementRefreshV2>,
        opening: RuntimeMaintenanceIngressGateOpeningAuthorityV2,
        transition: RuntimeProcessProductionHandoffFailureV2,
    },
    Lost {
        opening: RuntimeMaintenanceIngressGateOpeningAuthorityV2,
        transition: RuntimeProcessProductionHandoffFailureV2,
    },
}

struct RuntimeServingCertificationAcknowledgementInputV2<'a> {
    foundation: &'a mut RuntimeProcessFoundationV1,
    discord: &'a crate::discord::RuntimeDiscordProcessSupervisorV2,
    lifecycle: RuntimeClosedRecoverySupervisedServingOpenProcessV2,
    opening: RuntimeMaintenanceIngressGateOpeningAuthorityV2,
    ingress_acknowledgement: &'a mut super::RuntimeProcessIngressAcknowledgementSupervisorV2,
    acknowledgement_safety:
        &'a crate::ingress_acknowledgement_safety::RuntimeIngressAcknowledgementSafetyMonitorV2,
    owner_receipt: automation_runtime_controller::RuntimeGatewayOwnerLeaseReceiptV1,
    gateway_ready: automation_runtime_controller::RuntimeGatewayReadyAttestationV2,
    deadline: Instant,
}

async fn execute_certification_v2(
    accepted: RuntimeServingCertificationAcceptedV2,
) -> Result<RuntimeServingCertificationSuccessV2, RuntimeProcessProductionHandoffErrorV2> {
    let RuntimeServingCertificationAcceptedV2 {
        process,
        gateway_monitor,
        staged,
        mut session,
        permit,
        evidence,
        witness,
        finalizer_slot,
        operation_id,
        barrier_id,
        acceptance_deadline,
        serving_lease_for,
    } = accepted;
    let (core, lifecycle, maintenance_ingress, acknowledgement_schedule) =
        RuntimeServingCertificationProcessCoreV2::from_process_v2(process);
    let normal = RuntimeServingCertificationNormalProcessV2 {
        core,
        lifecycle,
        maintenance_ingress,
        gateway_monitor: Some(gateway_monitor),
    };
    if Instant::now() >= acceptance_deadline
        || permit.ensure_active().is_err()
        || !matches!(witness.lifecycle, RuntimeRouteLifecycleV2::Staged)
        || witness.active_interactions != 0
    {
        return Err(normal
            .cleanup_v2(
                None,
                RuntimeServingCertificationFailureV2::Protocol.transition_v2(),
            )
            .await);
    }
    normal.core.readiness.remove_readiness_v2();
    let RuntimeServingCertificationNormalProcessV2 {
        core,
        lifecycle,
        maintenance_ingress,
        mut gateway_monitor,
    } = normal;
    let drain = maintenance_ingress.begin_close_v2();
    if drain
        .wait_closed_until_v2(acceptance_deadline)
        .await
        .is_err()
    {
        return Err(RuntimeServingCertificationNormalProcessV2 {
            core,
            lifecycle,
            maintenance_ingress: drain,
            gateway_monitor,
        }
        .cleanup_v2(
            None,
            RuntimeServingCertificationFailureV2::Gate.transition_v2(),
        )
        .await);
    }
    let gate = match drain.into_controller_v2() {
        Ok(gate) => gate,
        Err(failure) => {
            return Err(RuntimeServingCertificationNormalProcessV2 {
                core,
                lifecycle,
                maintenance_ingress: failure.into_state(),
                gateway_monitor,
            }
            .cleanup_v2(
                None,
                RuntimeServingCertificationFailureV2::Gate.transition_v2(),
            )
            .await);
        }
    };
    if let Some(monitor) = gateway_monitor.take() {
        monitor.stop_v2().await;
    }
    let freeze = match lifecycle.prepare_certification_freeze_v2(acceptance_deadline) {
        Ok(freeze) => freeze,
        Err(_) => {
            return Err(core
                .cleanup_without_lifecycle_v2(
                    gate,
                    None,
                    RuntimeServingCertificationFailureV2::Owner.transition_v2(),
                )
                .await);
        }
    };
    let (lifecycle, frozen_owner) = match freeze.freeze_v2().await {
        Ok(frozen) => frozen,
        Err(_) => {
            return Err(core
                .cleanup_without_lifecycle_v2(
                    gate,
                    None,
                    RuntimeServingCertificationFailureV2::Owner.transition_v2(),
                )
                .await);
        }
    };
    let mut frozen = RuntimeServingCertificationFrozenProcessV2 {
        core,
        lifecycle,
        maintenance_ingress: gate,
        heartbeat: None,
    };
    let input = match build_certification_reservation_input_v2(
        &session,
        &evidence,
        &witness,
        frozen_owner.observation_v2().receipt(),
        frozen.core.foundation.build_revision.clone(),
        operation_id,
        serving_lease_for,
    ) {
        Ok(input) => input,
        Err(failure) => {
            return Err(frozen.cleanup_v2(failure.transition_v2()).await);
        }
    };
    let prepared = match reserve_and_prepare_certification_v2(
        &mut session,
        input,
        frozen.core.foundation.databases.execution(),
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(failure) => {
            return Err(frozen.cleanup_v2(failure.transition_v2()).await);
        }
    };
    let commit_deadline = match runtime_certification_deadline_v2(prepared.must_commit_before()) {
        Some(deadline) if Instant::now() < acceptance_deadline => deadline.min(acceptance_deadline),
        _ => {
            return Err(frozen
                .cleanup_v2(RuntimeServingCertificationFailureV2::Deadline.transition_v2())
                .await);
        }
    };
    let barrier_port = match frozen.lifecycle.ordinary_barrier_port_v3() {
        Ok(port) => port,
        Err(_) => {
            return Err(frozen
                .cleanup_v2(RuntimeServingCertificationFailureV2::Gateway.transition_v2())
                .await);
        }
    };
    let paused = match frozen
        .lifecycle
        .pause_certification_barrier_b_v2(&barrier_port, barrier_id.clone(), commit_deadline)
        .await
    {
        RuntimeDiscordCertificationBarrierBPauseOutcomeV2::Applied(paused) => paused,
        RuntimeDiscordCertificationBarrierBPauseOutcomeV2::DefinitelyNotApplied(_) => {
            return Err(frozen
                .cleanup_v2(RuntimeServingCertificationFailureV2::Gateway.transition_v2())
                .await);
        }
        RuntimeDiscordCertificationBarrierBPauseOutcomeV2::Indeterminate(_) => {
            return Err(frozen
                .cleanup_v2(RuntimeServingCertificationFailureV2::Gateway.transition_v2())
                .await);
        }
    };
    let activated = match frozen
        .lifecycle
        .activate_certification_barrier_b_v2(staged, paused)
    {
        Ok(activated) => activated,
        Err(_) => {
            return Err(frozen
                .cleanup_v2(RuntimeServingCertificationFailureV2::Registry.transition_v2())
                .await);
        }
    };
    let resumed = match frozen
        .lifecycle
        .resume_certification_barrier_b_v2(&barrier_port, activated, commit_deadline)
        .await
    {
        RuntimeDiscordCertificationBarrierBResumeOutcomeV2::Applied(resumed) => resumed,
        RuntimeDiscordCertificationBarrierBResumeOutcomeV2::DefinitelyNotApplied { .. }
        | RuntimeDiscordCertificationBarrierBResumeOutcomeV2::Indeterminate(_) => {
            return Err(frozen
                .cleanup_v2(RuntimeServingCertificationFailureV2::Gateway.transition_v2())
                .await);
        }
    };
    let route_admission = match build_route_admission_attestation_v2(
        &resumed,
        frozen_owner.observation_v2().receipt(),
    ) {
        Ok(attestation) => attestation,
        Err(failure) => {
            return Err(frozen.cleanup_v2(failure.transition_v2()).await);
        }
    };
    let completed = match prepared.complete_barrier_b_v2(
        barrier_id,
        resumed.gateway_v2().paused_v2().clone(),
        route_admission,
    ) {
        Ok(completed) => completed,
        Err(_) => {
            return Err(frozen
                .cleanup_v2(RuntimeServingCertificationFailureV2::Protocol.transition_v2())
                .await);
        }
    };
    let gateway_ready = resumed.gateway_v2().ready_v2().clone();
    let (pending, registry_monitor) = match resumed.prepare_serving_monitor_v2() {
        Ok(prepared) => prepared,
        Err(_) => {
            return Err(frozen
                .cleanup_v2(RuntimeServingCertificationFailureV2::Registry.transition_v2())
                .await);
        }
    };
    let registration = completed.authorize_finalization();
    let registered = match finalizer_slot.submit_certification_finalizer_v2(registration) {
        Ok(registered) => registered,
        Err(_) => {
            return Err(frozen
                .cleanup_v2(RuntimeServingCertificationFailureV2::Finalizer.transition_v2())
                .await);
        }
    };
    let committed = match await_committed_certification_v2(
        &mut frozen.core.foundation,
        registered,
        commit_deadline,
    )
    .await
    {
        Ok(committed) => committed,
        Err(failure) => {
            return Err(frozen.cleanup_v2(failure.transition_v2()).await);
        }
    };
    let (canonical, certification_receipt) = committed.into_parts();
    let serving = match session.apply_certification_v2(canonical, certification_receipt) {
        Ok(serving) => serving,
        Err(_) => {
            return Err(frozen
                .cleanup_v2(RuntimeServingCertificationFailureV2::Protocol.transition_v2())
                .await);
        }
    };
    let gateway_loss = frozen
        .lifecycle
        .bind_gateway_ready_invalidation_observer_v2(&gateway_ready);
    let owner_loss = frozen.lifecycle.owner_terminal_observation_v2();
    let observers = match RuntimeServingHeartbeatExternalObserversV2::with_exact_gateway_v2(
        async move {
            let _ = owner_loss.await;
        },
        gateway_loss,
    ) {
        Ok(observers) => observers,
        Err(_) => {
            return Err(frozen
                .cleanup_v2(RuntimeServingCertificationFailureV2::Gateway.transition_v2())
                .await);
        }
    };
    let heartbeat = match start_runtime_process_serving_heartbeat_monitor_v2(
        serving.clone(),
        frozen.core.foundation.databases.serving().clone(),
        registry_monitor,
        frozen.core.foundation.shutdown_trigger_v1(),
        frozen.core.foundation.shutdown_observer_v1(),
        observers,
        RuntimeServingHeartbeatMonitorConfigV2::production_v2(),
    )
    .await
    {
        Ok(heartbeat) => heartbeat.into_monitor_v2(),
        Err(_) => {
            return Err(frozen
                .cleanup_v2(RuntimeServingCertificationFailureV2::Database.transition_v2())
                .await);
        }
    };
    frozen.heartbeat = Some(heartbeat);
    let RuntimeServingCertificationFrozenProcessV2 {
        mut core,
        lifecycle,
        maintenance_ingress,
        heartbeat,
    } = frozen;
    let (lifecycle, successor_owner) =
        match lifecycle.thaw_v2(frozen_owner, acceptance_deadline).await {
            Ok(thawed) => thawed,
            Err(_) => {
                return Err(core
                    .cleanup_without_lifecycle_v2(
                        maintenance_ingress,
                        heartbeat,
                        RuntimeServingCertificationFailureV2::Owner.transition_v2(),
                    )
                    .await);
            }
        };
    let opening = match maintenance_ingress.begin_open_v2() {
        Ok(opening) => opening,
        Err(failure) => {
            return Err(RuntimeServingCertificationNormalProcessV2 {
                core,
                lifecycle,
                maintenance_ingress: failure.into_state(),
                gateway_monitor: None,
            }
            .cleanup_v2(
                heartbeat,
                RuntimeServingCertificationFailureV2::Gate.transition_v2(),
            )
            .await);
        }
    };
    let (lifecycle, opening, schedule, exact_acknowledgement) =
        match publish_preopen_acknowledgement_v2(
            RuntimeServingCertificationAcknowledgementInputV2 {
                foundation: &mut core.foundation,
                discord: &core.discord,
                lifecycle,
                opening,
                ingress_acknowledgement: &mut core.ingress_acknowledgement,
                acknowledgement_safety: &core.acknowledgement_safety,
                owner_receipt: successor_owner.receipt().clone(),
                gateway_ready,
                deadline: acceptance_deadline.min(acknowledgement_schedule.safety_deadline),
            },
        )
        .await
        {
            Ok(acknowledgement) => acknowledgement,
            Err(RuntimeServingCertificationAcknowledgementFailureV2::Normal {
                lifecycle,
                opening,
                transition,
            }) => {
                return Err(RuntimeServingCertificationNormalProcessV2 {
                    core,
                    lifecycle: *lifecycle,
                    maintenance_ingress: opening,
                    gateway_monitor: None,
                }
                .cleanup_v2(heartbeat, transition)
                .await);
            }
            Err(RuntimeServingCertificationAcknowledgementFailureV2::Refreshing {
                lifecycle,
                opening,
                transition,
            }) => {
                return Err(core
                    .cleanup_refreshing_v2(*lifecycle, opening, heartbeat, transition)
                    .await);
            }
            Err(RuntimeServingCertificationAcknowledgementFailureV2::Admission {
                lifecycle,
                opening,
                transition,
            }) => {
                return Err(core
                    .cleanup_admission_v2(*lifecycle, opening, heartbeat, transition)
                    .await);
            }
            Err(RuntimeServingCertificationAcknowledgementFailureV2::Empty {
                lifecycle,
                opening,
                transition,
            }) => {
                return Err(core
                    .cleanup_empty_v2(*lifecycle, opening, heartbeat, transition)
                    .await);
            }
            Err(RuntimeServingCertificationAcknowledgementFailureV2::RefreshingEmpty {
                lifecycle,
                opening,
                transition,
            }) => {
                return Err(core
                    .cleanup_refreshing_empty_v2(*lifecycle, opening, heartbeat, transition)
                    .await);
            }
            Err(RuntimeServingCertificationAcknowledgementFailureV2::Lost {
                opening,
                transition,
            }) => {
                return Err(core
                    .cleanup_without_lifecycle_v2(opening, heartbeat, transition)
                    .await);
            }
        };
    let maintenance_ingress = match opening.commit_open_v2() {
        Ok(open) => open,
        Err(failure) => {
            return Err(RuntimeServingCertificationNormalProcessV2 {
                core,
                lifecycle,
                maintenance_ingress: failure.into_state(),
                gateway_monitor: None,
            }
            .cleanup_v2(
                heartbeat,
                RuntimeServingCertificationFailureV2::Gate.transition_v2(),
            )
            .await);
        }
    };
    let completion = match lifecycle
        .authorize_certification_barrier_b_completion_v2(pending, exact_acknowledgement)
        .await
    {
        Ok(completion) => completion,
        Err(_) => {
            return Err(RuntimeServingCertificationNormalProcessV2 {
                core,
                lifecycle,
                maintenance_ingress,
                gateway_monitor: None,
            }
            .cleanup_v2(
                heartbeat,
                RuntimeServingCertificationFailureV2::Gateway.transition_v2(),
            )
            .await);
        }
    };
    let _completed_barrier = match lifecycle.complete_certification_barrier_b_v2(completion) {
        Ok(completed) => completed,
        Err(_) => {
            return Err(RuntimeServingCertificationNormalProcessV2 {
                core,
                lifecycle,
                maintenance_ingress,
                gateway_monitor: None,
            }
            .cleanup_v2(
                heartbeat,
                RuntimeServingCertificationFailureV2::Gateway.transition_v2(),
            )
            .await);
        }
    };
    let mut lifecycle = lifecycle;
    if lifecycle.complete_slot_work_v2(permit).is_err() {
        return Err(RuntimeServingCertificationNormalProcessV2 {
            core,
            lifecycle,
            maintenance_ingress,
            gateway_monitor: None,
        }
        .cleanup_v2(
            heartbeat,
            RuntimeServingCertificationFailureV2::Protocol.transition_v2(),
        )
        .await);
    }
    let slot = RuntimeServingSlotV2::from_target(&serving.identity.process_identity.target);
    let heartbeat = match heartbeat {
        Some(heartbeat) => heartbeat,
        None => {
            return Err(RuntimeServingCertificationNormalProcessV2 {
                core,
                lifecycle,
                maintenance_ingress,
                gateway_monitor: None,
            }
            .cleanup_v2(
                None,
                RuntimeServingCertificationFailureV2::Protocol.transition_v2(),
            )
            .await);
        }
    };
    if let Err(failure) = core.certification_monitors.insert_v2(slot, heartbeat) {
        let transition = match failure.source_v2() {
            RuntimeServingCertificationMonitorAdmissionErrorV2::DuplicateSlot
            | RuntimeServingCertificationMonitorAdmissionErrorV2::CapacityExhausted => {
                RuntimeServingCertificationFailureV2::Protocol.transition_v2()
            }
        };
        let monitor = failure.into_monitor_v2();
        return Err(RuntimeServingCertificationNormalProcessV2 {
            core,
            lifecycle,
            maintenance_ingress,
            gateway_monitor: None,
        }
        .cleanup_v2(Some(monitor), transition)
        .await);
    }
    let RuntimeServingCertificationProcessCoreV2 {
        controller,
        discord,
        foundation,
        readiness,
        ingress_acknowledgement,
        acknowledgement_safety,
        certification_monitors,
        process_generation,
    } = core;
    let process = RuntimeServingOpenProcessV2 {
        controller,
        discord,
        foundation,
        lifecycle,
        maintenance_ingress,
        readiness,
        ingress_acknowledgement,
        acknowledgement_schedule: schedule,
        acknowledgement_safety,
        certification_monitors,
        process_generation,
    };
    if process.revalidate_v2().await.is_err() {
        return Err(process
            .cleanup_transition_v2(RuntimeServingCertificationFailureV2::Protocol.transition_v2())
            .await);
    }
    let gateway_ready = match process
        .lifecycle
        .observe_exact_current_ready_attestation_v2()
    {
        Ok(ready) => ready,
        Err(_) => {
            return Err(process
                .cleanup_transition_v2(
                    RuntimeServingCertificationFailureV2::Gateway.transition_v2(),
                )
                .await);
        }
    };
    let gateway_monitor = match process.start_gateway_monitor_v2(&gateway_ready) {
        Ok(monitor) => monitor,
        Err(failure) => {
            return Err(process.cleanup_transition_v2(failure).await);
        }
    };
    if !process.readiness.publish_ready_v2() {
        gateway_monitor.stop_v2().await;
        return Err(process
            .cleanup_transition_v2(RuntimeServingCertificationFailureV2::Protocol.transition_v2())
            .await);
    }
    Ok(RuntimeServingCertificationSuccessV2 {
        process,
        gateway_monitor,
        serving,
    })
}

fn build_certification_reservation_input_v2(
    session: &RuntimeConvergenceSessionV1,
    evidence: &automation_runtime_worker::RuntimeExactTargetEvidenceV2,
    witness: &RuntimeRouteWitnessV2,
    owner: &automation_runtime_controller::RuntimeGatewayOwnerLeaseReceiptV1,
    build_revision: automation_runtime_controller::RuntimeBuildRevisionV1,
    operation_id: RuntimeCertificationOperationIdV2,
    serving_lease_for: Duration,
) -> Result<RuntimeCertificationReservationInputV2, RuntimeServingCertificationFailureV2> {
    let snapshot = session.snapshot();
    let current_execution = session
        .current_execution_receipt()
        .map_err(|_| RuntimeServingCertificationFailureV2::Protocol)?;
    let panel = snapshot
        .panel_certificate
        .as_ref()
        .ok_or(RuntimeServingCertificationFailureV2::Protocol)?;
    if !matches!(
        snapshot.phase,
        RuntimeDeploymentPhaseV1::AwaitingGatewayReady
    ) || !certification_execution_lineage_matches_v2(evidence.execution(), &current_execution)
        || witness.identity.target != snapshot.target
        || witness.identity.runtime_generation != snapshot.runtime_generation
        || witness.identity.process_instance_id != panel.process_instance_id
        || panel.target != snapshot.target
        || panel.runtime_generation != snapshot.runtime_generation
        || witness.controller_fencing_token != current_execution.fencing_token
        || owner.lease_id.process_instance_id != witness.identity.process_instance_id
        || owner.lease_id.expected_build_revision != build_revision
        || !evidence.binding_pin().matches(
            &automation_runtime_controller::RuntimeDeploymentScopeV1::from_identity(
                &snapshot.identity,
            ),
            &snapshot.target,
        )
        || serving_lease_for.is_zero()
    {
        return Err(RuntimeServingCertificationFailureV2::Protocol);
    }
    Ok(RuntimeCertificationReservationInputV2 {
        operation_id,
        binding_pin: evidence.binding_pin().clone(),
        gateway_owner_lease_id: owner.lease_id.clone(),
        observed_owner_revision: owner.owner_revision,
        runtime_build_revision: build_revision,
        panel: RuntimePanelEvidenceV2 {
            certificate_id: panel.certificate_id.clone(),
            report_digest: panel.report_digest.clone(),
            process_identity: witness.identity.clone(),
            controller_fencing_token: witness.controller_fencing_token,
        },
        serving_lease_for,
    })
}

fn certification_execution_lineage_matches_v2(
    evidence: &RuntimeExecutionReceiptV1,
    current: &RuntimeExecutionReceiptV1,
) -> bool {
    let Some(revision_delta) = current
        .snapshot
        .revision
        .get()
        .checked_sub(evidence.snapshot.revision.get())
    else {
        return false;
    };
    let Some(fencing_delta) = current
        .fencing_token
        .get()
        .checked_sub(evidence.fencing_token.get())
    else {
        return false;
    };
    let lease_time_matches = if fencing_delta == 0 {
        evidence.acquired_at == current.acquired_at && evidence.expires_at == current.expires_at
    } else {
        evidence.acquired_at <= current.acquired_at && evidence.expires_at < current.expires_at
    };
    revision_delta >= fencing_delta
        && lease_time_matches
        && execution_receipt_lease_matches_v2(evidence)
        && execution_receipt_lease_matches_v2(current)
        && evidence.snapshot.identity == current.snapshot.identity
        && evidence.snapshot.target == current.snapshot.target
        && evidence.snapshot.runtime_generation == current.snapshot.runtime_generation
        && evidence.snapshot.previous_runtime == current.snapshot.previous_runtime
        && evidence.snapshot.requested_at == current.snapshot.requested_at
        && evidence.controller_id == current.controller_id
        && evidence.convergence_attempt == current.convergence_attempt
}

fn execution_receipt_lease_matches_v2(execution: &RuntimeExecutionReceiptV1) -> bool {
    execution
        .snapshot
        .controller_lease
        .as_ref()
        .is_some_and(|lease| {
            lease.controller_id == execution.controller_id
                && lease.fencing_token == execution.fencing_token
                && lease.acquired_at == execution.acquired_at
                && lease.expires_at == execution.expires_at
                && execution.snapshot.last_fencing_token == Some(execution.fencing_token)
        })
}

async fn reserve_and_prepare_certification_v2(
    session: &mut RuntimeConvergenceSessionV1,
    input: RuntimeCertificationReservationInputV2,
    database: &automation_runtime_execution_postgres::PostgresRuntimeExecutionV1,
) -> Result<
    automation_runtime_worker::RuntimePreparedCertificationV2<
        PostgresPreparedRuntimeCertificationV2,
    >,
    RuntimeServingCertificationFailureV2,
> {
    let execution = session
        .current_execution_receipt()
        .map_err(|_| RuntimeServingCertificationFailureV2::Protocol)?;
    let proposed = session
        .begin_certification_reservation_v2(input)
        .map_err(|_| RuntimeServingCertificationFailureV2::Protocol)?;
    let lookup = RuntimeCertificationReservationScopeLookupV2::from_awaiting_execution(&execution)
        .map_err(|_| RuntimeServingCertificationFailureV2::Protocol)?;
    let proposal =
        RuntimeCertificationReservationProposalV2::from_reserved_intent(proposed, lookup)
            .map_err(|_| RuntimeServingCertificationFailureV2::Protocol)?;
    let authority = match proposal.reserve(database).await {
        Ok(checked) => session
            .apply_certification_reservation_v2(checked.into_session_outcome())
            .map_err(|_| RuntimeServingCertificationFailureV2::Protocol)?,
        Err(failure) => {
            let observed = failure
                .proposal
                .observe(database)
                .await
                .map_err(|_| RuntimeServingCertificationFailureV2::Database)?;
            session
                .apply_observed_certification_reservation_v2(observed.into_observation())
                .map_err(|_| RuntimeServingCertificationFailureV2::Protocol)?
        }
    };
    RuntimeReservedCertificationV2::from_reservation_authority(authority)
        .prepare(database)
        .await
        .map_err(|_| RuntimeServingCertificationFailureV2::Database)
}

fn build_route_admission_attestation_v2(
    resumed: &crate::gateway::RuntimeDiscordCertificationBarrierBResumedV2,
    owner: &automation_runtime_controller::RuntimeGatewayOwnerLeaseReceiptV1,
) -> Result<RuntimeRouteAdmissionAttestationV2, RuntimeServingCertificationFailureV2> {
    let gateway = resumed.gateway_v2();
    let paused = gateway.paused_v2();
    let activation = resumed.activation_evidence_v2();
    let coordinator_generation = NonZeroU64::new(paused.coordinator_generation().get())
        .ok_or(RuntimeServingCertificationFailureV2::Protocol)?;
    let route = RuntimeServingRouteAttestationV2 {
        identity: activation.identity_v2().clone(),
        controller_fencing_token: activation.fencing_token_v2(),
        route_incarnation: activation.route_incarnation_v2(),
        activation_sequence: activation.activation_sequence_v2(),
    };
    let attestation = RuntimeRouteAdmissionAttestationV2 {
        barrier_id: resumed.barrier_id_v2().clone(),
        pause: RuntimeBarrierPauseWitnessV2 {
            coordinator_generation,
            connection_epoch: paused.connection_epoch(),
            paused_admission_revision: paused.admission_revision(),
            pause_sequence: paused.transition_sequence(),
        },
        gateway: gateway.ready_v2().clone(),
        gateway_owner_lease_id: owner.lease_id.clone(),
        attested_owner_revision: owner.owner_revision,
        route,
    };
    attestation
        .validate()
        .map_err(|_| RuntimeServingCertificationFailureV2::Protocol)?;
    Ok(attestation)
}

async fn await_committed_certification_v2(
    foundation: &mut RuntimeProcessFoundationV1,
    registered: super::RuntimeRegisteredCertificationFinalizerJobV2,
    deadline: Instant,
) -> Result<RuntimeCommittedCertificationV2, RuntimeServingCertificationFailureV2> {
    if Instant::now() >= deadline {
        return Err(RuntimeServingCertificationFailureV2::Deadline);
    }
    let mut shutdown = foundation.shutdown_observer_v1();
    let completion = tokio::select! {
        biased;
        _ = shutdown.wait() => {
            return Err(RuntimeServingCertificationFailureV2::Finalizer);
        }
        completion = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            foundation.next_process_mutation_finalizer_completion_v3(),
        ) => {
            completion
                .map_err(|_| RuntimeServingCertificationFailureV2::Deadline)?
                .ok_or(RuntimeServingCertificationFailureV2::Finalizer)?
        }
    };
    match complete_certification_finalizer_job_v2(registered, completion)
        .map_err(|_| RuntimeServingCertificationFailureV2::Finalizer)?
    {
        RuntimeCertificationFinalizationOutcomeV2::Committed(committed) => Ok(*committed),
        RuntimeCertificationFinalizationOutcomeV2::Indeterminate(recovery) => {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .filter(|remaining| !remaining.is_zero())
                .ok_or(RuntimeServingCertificationFailureV2::Deadline)?;
            match recovery
                .quiesce_and_observe(remaining)
                .await
                .map_err(|_| RuntimeServingCertificationFailureV2::Database)?
            {
                RuntimeCertificationRecoveryResolutionV2::Committed { committed, .. } => {
                    Ok(*committed)
                }
                RuntimeCertificationRecoveryResolutionV2::DefinitelyRolledBack { .. }
                | RuntimeCertificationRecoveryResolutionV2::Diverged { .. }
                | RuntimeCertificationRecoveryResolutionV2::Rejected { .. } => {
                    Err(RuntimeServingCertificationFailureV2::Protocol)
                }
            }
        }
        RuntimeCertificationFinalizationOutcomeV2::DefinitelyRolledBack { .. } => {
            Err(RuntimeServingCertificationFailureV2::Database)
        }
        RuntimeCertificationFinalizationOutcomeV2::RejectedCommitted(_) => {
            Err(RuntimeServingCertificationFailureV2::Protocol)
        }
    }
}

async fn publish_preopen_acknowledgement_v2(
    input: RuntimeServingCertificationAcknowledgementInputV2<'_>,
) -> Result<
    (
        RuntimeClosedRecoverySupervisedServingOpenProcessV2,
        RuntimeMaintenanceIngressGateOpeningAuthorityV2,
        RuntimeIngressAcknowledgementScheduleV2,
        RuntimeExactIngressAcknowledgementReobservationV3,
    ),
    RuntimeServingCertificationAcknowledgementFailureV2,
> {
    let RuntimeServingCertificationAcknowledgementInputV2 {
        foundation,
        discord,
        lifecycle,
        opening,
        ingress_acknowledgement,
        acknowledgement_safety,
        owner_receipt,
        gateway_ready,
        deadline,
    } = input;
    if Instant::now() >= deadline {
        return Err(
            RuntimeServingCertificationAcknowledgementFailureV2::Normal {
                lifecycle: Box::new(lifecycle),
                opening,
                transition: RuntimeServingCertificationFailureV2::Deadline.transition_v2(),
            },
        );
    }
    let gate = foundation.maintenance_ingress_observer_v2().snapshot_v2();
    if gate.stage() != RuntimeMaintenanceIngressGateStageV2::Opening
        || gate.generation() != opening.generation()
        || gate.active_permit_count() != 0
        || gate.shutdown_sealed()
        || gate.terminal_error().is_some()
    {
        return Err(
            RuntimeServingCertificationAcknowledgementFailureV2::Normal {
                lifecycle: Box::new(lifecycle),
                opening,
                transition: RuntimeServingCertificationFailureV2::Gate.transition_v2(),
            },
        );
    }
    let database = match collect_recovery_resume_database_evidence_v2(foundation).await {
        Ok(database) => database,
        Err(_) => {
            return Err(
                RuntimeServingCertificationAcknowledgementFailureV2::Normal {
                    lifecycle: Box::new(lifecycle),
                    opening,
                    transition: RuntimeServingCertificationFailureV2::Database.transition_v2(),
                },
            );
        }
    };
    let (readiness, writer_fence_generation) = database.into_parts_v2();
    let finalizer_accepting = foundation
        .process_finalizer_health_v2()
        .is_some_and(|health| health.is_ready());
    let supervisors_running = finalizer_accepting
        && lifecycle.owner_terminal_status_v2().is_none()
        && discord.terminal_status_v2().is_none()
        && !discord.is_finished_v2()
        && foundation.shutdown_observer_v1().observed().is_none();
    if !supervisors_running {
        return Err(
            RuntimeServingCertificationAcknowledgementFailureV2::Normal {
                lifecycle: Box::new(lifecycle),
                opening,
                transition: RuntimeServingCertificationFailureV2::Protocol.transition_v2(),
            },
        );
    }
    let predecessor_authorization =
        lifecycle.authorize_ingress_acknowledgement_predecessor_observation_v2();
    let final_authorization =
        lifecycle.authorize_ingress_acknowledgement_predecessor_observation_v2();
    let predecessor_observation = match foundation
        .databases
        .execution()
        .observe_ingress_open_acknowledgement_predecessor(&predecessor_authorization)
        .await
    {
        Ok(observation) => observation,
        Err(_) => {
            return Err(
                RuntimeServingCertificationAcknowledgementFailureV2::Normal {
                    lifecycle: Box::new(lifecycle),
                    opening,
                    transition: RuntimeServingCertificationFailureV2::Database.transition_v2(),
                },
            );
        }
    };
    let predecessor = match predecessor_authorization.accept(predecessor_observation) {
        Ok(predecessor) => predecessor,
        Err(_) => {
            return Err(
                RuntimeServingCertificationAcknowledgementFailureV2::Normal {
                    lifecycle: Box::new(lifecycle),
                    opening,
                    transition: RuntimeServingCertificationFailureV2::Protocol.transition_v2(),
                },
            );
        }
    };
    let lease_for = match RuntimeIngressOpenAcknowledgementLeaseDurationV2::from_duration(
        INGRESS_ACKNOWLEDGEMENT_LEASE_V2,
    ) {
        Ok(lease_for) => lease_for,
        Err(_) => {
            return Err(
                RuntimeServingCertificationAcknowledgementFailureV2::Normal {
                    lifecycle: Box::new(lifecycle),
                    opening,
                    transition: RuntimeServingCertificationFailureV2::Protocol.transition_v2(),
                },
            );
        }
    };
    let evidence = RuntimeClosedRecoveryServingAcknowledgementEvidenceV2 {
        owner_receipt,
        readiness,
        gateway_ready,
        writer_fence_generation,
        maintenance_gate_generation: gate.generation(),
        maintenance_gate_open: false,
        maintenance_gate_opening: true,
        finalizer_generation: lifecycle.finalizer_generation_v2(),
        finalizer_accepting,
        supervisors_running,
        predecessor,
        lease_for,
    };
    let refresh = match lifecycle.authorize_resumed_acknowledgement_refresh_v3(evidence) {
        Ok(refresh) => refresh,
        Err(failure) => {
            return Err(
                RuntimeServingCertificationAcknowledgementFailureV2::Normal {
                    lifecycle: Box::new(failure.into_state_v2()),
                    opening,
                    transition: RuntimeServingCertificationFailureV2::Acknowledgement
                        .transition_v2(),
                },
            );
        }
    };
    let authority = refresh.into_ingress_acknowledgement_authority_v2();
    let acknowledgement = execute_ingress_acknowledgement_v2(
        ingress_acknowledgement,
        authority,
        deadline,
        foundation.lifecycle_timing_v2(),
    )
    .await;
    let (lifecycle, accepted_receipt) = match acknowledgement {
        Ok(RuntimeClosedRecoveryIngressAcknowledgementOutcomeV2::ServingOpenRefresh {
            lifecycle,
            accepted_receipt,
        }) => (*lifecycle, *accepted_receipt),
        Ok(RuntimeClosedRecoveryIngressAcknowledgementOutcomeV2::Admission {
            lifecycle, ..
        }) => {
            return Err(
                RuntimeServingCertificationAcknowledgementFailureV2::Admission {
                    lifecycle,
                    opening,
                    transition: RuntimeServingCertificationFailureV2::Acknowledgement
                        .transition_v2(),
                },
            );
        }
        Ok(RuntimeClosedRecoveryIngressAcknowledgementOutcomeV2::EmptyOpenRefresh {
            lifecycle,
            ..
        }) => {
            return Err(RuntimeServingCertificationAcknowledgementFailureV2::Empty {
                lifecycle,
                opening,
                transition: RuntimeServingCertificationFailureV2::Acknowledgement.transition_v2(),
            });
        }
        Err(RuntimeProcessIngressAcknowledgementExecutionFailureV2::Retained {
            authority,
            transition,
        }) => {
            return match (*authority).into_retained_state_v2() {
                crate::closed_recovery::RuntimeClosedRecoveryIngressAcknowledgementRetainedStateV2::ServingOpenRefresh(lifecycle) => {
                    Err(RuntimeServingCertificationAcknowledgementFailureV2::Refreshing {
                        lifecycle,
                        opening,
                        transition,
                    })
                }
                crate::closed_recovery::RuntimeClosedRecoveryIngressAcknowledgementRetainedStateV2::Admission(lifecycle) => {
                    Err(RuntimeServingCertificationAcknowledgementFailureV2::Admission {
                        lifecycle,
                        opening,
                        transition,
                    })
                }
                crate::closed_recovery::RuntimeClosedRecoveryIngressAcknowledgementRetainedStateV2::EmptyOpenRefresh(lifecycle) => {
                    Err(RuntimeServingCertificationAcknowledgementFailureV2::RefreshingEmpty {
                        lifecycle,
                        opening,
                        transition,
                    })
                }
            };
        }
        Err(RuntimeProcessIngressAcknowledgementExecutionFailureV2::AuthorityLost(transition)) => {
            return Err(RuntimeServingCertificationAcknowledgementFailureV2::Lost {
                opening,
                transition,
            });
        }
    };
    let observation_started_at = Instant::now();
    let exact = match exact_reobserve_ingress_acknowledgement_v2(
        foundation.databases.execution(),
        final_authorization,
        &accepted_receipt,
    )
    .await
    {
        Ok(exact) => exact,
        Err(_) => {
            return Err(
                RuntimeServingCertificationAcknowledgementFailureV2::Normal {
                    lifecycle: Box::new(lifecycle),
                    opening,
                    transition: RuntimeServingCertificationFailureV2::Acknowledgement
                        .transition_v2(),
                },
            );
        }
    };
    let schedule =
        match ingress_acknowledgement_schedule_v2(exact.receipt_v3(), observation_started_at) {
            Some(schedule) => schedule,
            None => {
                return Err(
                    RuntimeServingCertificationAcknowledgementFailureV2::Normal {
                        lifecycle: Box::new(lifecycle),
                        opening,
                        transition: RuntimeServingCertificationFailureV2::Deadline.transition_v2(),
                    },
                );
            }
        };
    if !acknowledgement_safety.rearm_v2(schedule.safety_deadline) {
        return Err(
            RuntimeServingCertificationAcknowledgementFailureV2::Normal {
                lifecycle: Box::new(lifecycle),
                opening,
                transition: RuntimeServingCertificationFailureV2::Deadline.transition_v2(),
            },
        );
    }
    Ok((lifecycle, opening, schedule, exact))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(super) enum RuntimeServingCertificationMonitorAdmissionErrorV2 {
    #[error("runtime serving certification monitor already exists for the slot")]
    DuplicateSlot,
    #[error("runtime serving certification monitor capacity is exhausted")]
    CapacityExhausted,
}

pub(super) struct RuntimeServingCertificationMonitorAdmissionFailureV2 {
    source: RuntimeServingCertificationMonitorAdmissionErrorV2,
    monitor: Box<RuntimeServingHeartbeatMonitorV2>,
}

impl RuntimeServingCertificationMonitorAdmissionFailureV2 {
    pub(super) const fn source_v2(&self) -> RuntimeServingCertificationMonitorAdmissionErrorV2 {
        self.source
    }

    pub(super) fn into_monitor_v2(self) -> RuntimeServingHeartbeatMonitorV2 {
        *self.monitor
    }
}

impl Debug for RuntimeServingCertificationMonitorAdmissionFailureV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingCertificationMonitorAdmissionFailureV2(<redacted>)")
    }
}

pub(super) struct RuntimeServingCertificationMonitorTerminalV2 {
    slot: Option<RuntimeServingSlotV2>,
    status: RuntimeServingHeartbeatTerminalStatusV2,
    outcomes: Vec<RuntimeServingHeartbeatMonitorOutcomeV2>,
}

impl RuntimeServingCertificationMonitorTerminalV2 {
    pub(super) fn slot_v2(&self) -> Option<&RuntimeServingSlotV2> {
        self.slot.as_ref()
    }

    pub(super) const fn status_v2(&self) -> RuntimeServingHeartbeatTerminalStatusV2 {
        self.status
    }

    pub(super) fn outcomes_v2(&self) -> &[RuntimeServingHeartbeatMonitorOutcomeV2] {
        &self.outcomes
    }
}

impl Debug for RuntimeServingCertificationMonitorTerminalV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingCertificationMonitorTerminalV2(<redacted>)")
    }
}

pub(super) struct RuntimeServingCertificationMonitorSetV2 {
    capacity: NonZeroUsize,
    monitors: BTreeMap<RuntimeServingSlotV2, RuntimeServingHeartbeatMonitorV2>,
    terminal_observers: JoinSet<(
        RuntimeServingSlotV2,
        RuntimeServingHeartbeatTerminalStatusV2,
    )>,
}

impl RuntimeServingCertificationMonitorSetV2 {
    pub(super) fn production_v2(capacity: NonZeroUsize) -> Self {
        Self::with_capacity_v2(capacity)
    }

    fn with_capacity_v2(capacity: NonZeroUsize) -> Self {
        Self {
            capacity,
            monitors: BTreeMap::new(),
            terminal_observers: JoinSet::new(),
        }
    }

    pub(super) fn ensure_available_v2(
        &self,
        slot: &RuntimeServingSlotV2,
    ) -> Result<(), RuntimeServingCertificationMonitorAdmissionErrorV2> {
        classify_monitor_admission_v2(
            self.monitors.contains_key(slot),
            self.monitors.len(),
            self.capacity,
        )
    }

    pub(super) fn insert_v2(
        &mut self,
        slot: RuntimeServingSlotV2,
        monitor: RuntimeServingHeartbeatMonitorV2,
    ) -> Result<(), RuntimeServingCertificationMonitorAdmissionFailureV2> {
        if let Err(source) = self.ensure_available_v2(&slot) {
            return Err(RuntimeServingCertificationMonitorAdmissionFailureV2 {
                source,
                monitor: Box::new(monitor),
            });
        }
        let mut terminal = monitor.terminal_observer_v2();
        let observed_slot = slot.clone();
        self.terminal_observers.spawn(async move {
            let status = terminal.wait_v2().await;
            (observed_slot, status)
        });
        let previous = self.monitors.insert(slot, monitor);
        debug_assert!(previous.is_none());
        Ok(())
    }

    pub(super) async fn next_terminal_v2(
        &mut self,
    ) -> RuntimeServingCertificationMonitorTerminalV2 {
        loop {
            if self.monitors.is_empty() {
                std::future::pending::<()>().await;
                unreachable!();
            }
            let Some(joined) = self.terminal_observers.join_next().await else {
                let outcomes = self.stop_all_until_v2(Instant::now()).await;
                return RuntimeServingCertificationMonitorTerminalV2 {
                    slot: None,
                    status: RuntimeServingHeartbeatTerminalStatusV2::ActorPanicked,
                    outcomes,
                };
            };
            let (slot, status) = match joined {
                Ok(terminal) => terminal,
                Err(_) => {
                    let outcomes = self.stop_all_until_v2(Instant::now()).await;
                    return RuntimeServingCertificationMonitorTerminalV2 {
                        slot: None,
                        status: RuntimeServingHeartbeatTerminalStatusV2::ActorPanicked,
                        outcomes,
                    };
                }
            };
            let Some(monitor) = self.monitors.remove(&slot) else {
                continue;
            };
            let outcome = monitor.wait_v2().await;
            return RuntimeServingCertificationMonitorTerminalV2 {
                slot: Some(slot),
                status,
                outcomes: vec![outcome],
            };
        }
    }

    pub(super) async fn stop_all_until_v2(
        &mut self,
        deadline: Instant,
    ) -> Vec<RuntimeServingHeartbeatMonitorOutcomeV2> {
        let monitors = std::mem::take(&mut self.monitors);
        self.terminal_observers.abort_all();
        let mut outcomes = Vec::with_capacity(monitors.len());
        for (_, monitor) in monitors {
            outcomes.push(monitor.stop_until_v2(deadline).await);
        }
        outcomes
    }
}

impl Debug for RuntimeServingCertificationMonitorSetV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingCertificationMonitorSetV2(<redacted>)")
    }
}

fn classify_monitor_admission_v2(
    duplicate: bool,
    current: usize,
    capacity: NonZeroUsize,
) -> Result<(), RuntimeServingCertificationMonitorAdmissionErrorV2> {
    if duplicate {
        Err(RuntimeServingCertificationMonitorAdmissionErrorV2::DuplicateSlot)
    } else if current >= capacity.get() {
        Err(RuntimeServingCertificationMonitorAdmissionErrorV2::CapacityExhausted)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use automation_runtime_convergence::{
        BindingRevision, CommandGuardV1, ControllerId, FencingToken, LeaseRequestV1,
        PreflightAttestationV1, PromotionId, RuntimeDeployment, RuntimeDeploymentIdentityV1,
        RuntimeDeploymentTargetV1, RuntimeGeneration,
    };
    use serde_json::json;

    use super::*;

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(second, 0).unwrap()
    }

    fn lineage_execution_v2() -> RuntimeExecutionReceiptV1 {
        let identity: RuntimeDeploymentIdentityV1 = serde_json::from_value(json!({
            "deployment_id": "deployment:lineage",
            "tenant_id": "tenant:lineage",
            "installation_id": "installation:lineage",
            "promotion_id": "a".repeat(64),
            "activation_request_id": "activation:lineage"
        }))
        .unwrap();
        let target: RuntimeDeploymentTargetV1 = serde_json::from_value(json!({
            "guild_id": "7",
            "ruleset_key": "lineage",
            "version": 1,
            "content_hash": "b".repeat(64),
            "binding_revision": 3,
            "binding_fingerprint": "c".repeat(64)
        }))
        .unwrap();
        let controller_id = ControllerId::parse("controller:lineage").unwrap();
        let fencing_token = FencingToken::new(5).unwrap();
        let mut deployment = RuntimeDeployment::request(
            identity,
            target,
            RuntimeGeneration::new(4).unwrap(),
            None,
            at(1),
        )
        .unwrap();
        deployment
            .acquire_lease(LeaseRequestV1 {
                expected_revision: deployment.revision(),
                controller_id,
                fencing_token,
                now: at(10),
                expires_at: at(30),
            })
            .unwrap();
        execution_from_deployment_v2(&deployment)
    }

    fn execution_from_deployment_v2(deployment: &RuntimeDeployment) -> RuntimeExecutionReceiptV1 {
        let snapshot = deployment.snapshot();
        let lease = snapshot.controller_lease.as_ref().unwrap();
        RuntimeExecutionReceiptV1 {
            controller_id: lease.controller_id.clone(),
            fencing_token: lease.fencing_token,
            convergence_attempt: NonZeroU32::new(6).unwrap(),
            acquired_at: lease.acquired_at,
            expires_at: lease.expires_at,
            snapshot,
        }
    }

    #[test]
    fn certification_lineage_accepts_valid_mutation_and_renewal_progress() {
        let evidence = lineage_execution_v2();
        let mut mutation = RuntimeDeployment::restore(evidence.snapshot.clone()).unwrap();
        mutation
            .accept_preflight(
                &CommandGuardV1 {
                    expected_revision: mutation.revision(),
                    controller_id: evidence.controller_id.clone(),
                    fencing_token: evidence.fencing_token,
                    runtime_generation: evidence.snapshot.runtime_generation,
                    now: at(11),
                },
                PreflightAttestationV1 {
                    target: evidence.snapshot.target.clone(),
                    runtime_generation: evidence.snapshot.runtime_generation,
                    observed_runtime: None,
                    checked_at: at(11),
                },
            )
            .unwrap();
        let mutation = execution_from_deployment_v2(&mutation);
        assert!(certification_execution_lineage_matches_v2(
            &evidence, &mutation
        ));

        let mut renewed = RuntimeDeployment::restore(evidence.snapshot.clone()).unwrap();
        renewed
            .acquire_lease(LeaseRequestV1 {
                expected_revision: renewed.revision(),
                controller_id: evidence.controller_id.clone(),
                fencing_token: evidence.fencing_token.next().unwrap(),
                now: at(11),
                expires_at: at(40),
            })
            .unwrap();
        renewed
            .accept_preflight(
                &CommandGuardV1 {
                    expected_revision: renewed.revision(),
                    controller_id: evidence.controller_id.clone(),
                    fencing_token: evidence.fencing_token.next().unwrap(),
                    runtime_generation: evidence.snapshot.runtime_generation,
                    now: at(12),
                },
                PreflightAttestationV1 {
                    target: evidence.snapshot.target.clone(),
                    runtime_generation: evidence.snapshot.runtime_generation,
                    observed_runtime: None,
                    checked_at: at(12),
                },
            )
            .unwrap();
        let renewed = execution_from_deployment_v2(&renewed);
        assert!(certification_execution_lineage_matches_v2(
            &evidence, &renewed
        ));
        assert_ne!(evidence.snapshot, renewed.snapshot);
    }

    #[test]
    fn certification_lineage_rejects_identity_authority_and_time_drift() {
        let evidence = lineage_execution_v2();

        let mut promotion = evidence.clone();
        promotion.snapshot.identity.promotion_id = PromotionId::parse("d".repeat(64)).unwrap();
        assert!(!certification_execution_lineage_matches_v2(
            &evidence, &promotion
        ));

        let mut target = evidence.clone();
        target.snapshot.target.binding_revision = BindingRevision::new(4).unwrap();
        assert!(!certification_execution_lineage_matches_v2(
            &evidence, &target
        ));

        let mut controller = evidence.clone();
        controller.controller_id = ControllerId::parse("controller:other").unwrap();
        assert!(!certification_execution_lineage_matches_v2(
            &evidence,
            &controller
        ));

        let mut fence = evidence.clone();
        fence.fencing_token = FencingToken::new(4).unwrap();
        fence
            .snapshot
            .controller_lease
            .as_mut()
            .unwrap()
            .fencing_token = fence.fencing_token;
        fence.snapshot.last_fencing_token = Some(fence.fencing_token);
        assert!(!certification_execution_lineage_matches_v2(
            &evidence, &fence
        ));

        let mut attempt = evidence.clone();
        attempt.convergence_attempt = NonZeroU32::new(7).unwrap();
        assert!(!certification_execution_lineage_matches_v2(
            &evidence, &attempt
        ));

        let mut revision = evidence.clone();
        revision.snapshot.revision = revision.snapshot.revision.next().unwrap();
        assert!(!certification_execution_lineage_matches_v2(
            &revision, &evidence
        ));

        let mut acquired = evidence.clone();
        acquired.acquired_at = at(9);
        acquired
            .snapshot
            .controller_lease
            .as_mut()
            .unwrap()
            .acquired_at = acquired.acquired_at;
        assert!(!certification_execution_lineage_matches_v2(
            &evidence, &acquired
        ));

        let mut expiry = evidence.clone();
        expiry.expires_at = at(29);
        expiry
            .snapshot
            .controller_lease
            .as_mut()
            .unwrap()
            .expires_at = expiry.expires_at;
        assert!(!certification_execution_lineage_matches_v2(
            &evidence, &expiry
        ));

        let mut same_revision_new_fence = evidence.clone();
        same_revision_new_fence.fencing_token =
            same_revision_new_fence.fencing_token.next().unwrap();
        same_revision_new_fence.expires_at = at(40);
        same_revision_new_fence
            .snapshot
            .controller_lease
            .as_mut()
            .unwrap()
            .fencing_token = same_revision_new_fence.fencing_token;
        same_revision_new_fence
            .snapshot
            .controller_lease
            .as_mut()
            .unwrap()
            .expires_at = same_revision_new_fence.expires_at;
        same_revision_new_fence.snapshot.last_fencing_token =
            Some(same_revision_new_fence.fencing_token);
        assert!(!certification_execution_lineage_matches_v2(
            &evidence,
            &same_revision_new_fence
        ));

        let mut same_fence_new_expiry = evidence.clone();
        same_fence_new_expiry.expires_at = at(40);
        same_fence_new_expiry
            .snapshot
            .controller_lease
            .as_mut()
            .unwrap()
            .expires_at = same_fence_new_expiry.expires_at;
        assert!(!certification_execution_lineage_matches_v2(
            &evidence,
            &same_fence_new_expiry
        ));

        let mut fence_outpaces_revision = evidence.clone();
        fence_outpaces_revision.snapshot.revision =
            fence_outpaces_revision.snapshot.revision.next().unwrap();
        fence_outpaces_revision.fencing_token =
            FencingToken::new(evidence.fencing_token.get() + 2).unwrap();
        fence_outpaces_revision.expires_at = at(40);
        fence_outpaces_revision
            .snapshot
            .controller_lease
            .as_mut()
            .unwrap()
            .fencing_token = fence_outpaces_revision.fencing_token;
        fence_outpaces_revision
            .snapshot
            .controller_lease
            .as_mut()
            .unwrap()
            .expires_at = fence_outpaces_revision.expires_at;
        fence_outpaces_revision.snapshot.last_fencing_token =
            Some(fence_outpaces_revision.fencing_token);
        assert!(!certification_execution_lineage_matches_v2(
            &evidence,
            &fence_outpaces_revision
        ));
    }

    #[test]
    fn monitor_set_rejects_duplicates_and_capacity_without_replacement() {
        let capacity = NonZeroUsize::MIN;
        assert_eq!(classify_monitor_admission_v2(false, 0, capacity), Ok(()));
        assert_eq!(
            classify_monitor_admission_v2(true, 0, capacity),
            Err(RuntimeServingCertificationMonitorAdmissionErrorV2::DuplicateSlot)
        );
        assert_eq!(
            classify_monitor_admission_v2(false, 1, capacity),
            Err(RuntimeServingCertificationMonitorAdmissionErrorV2::CapacityExhausted)
        );
    }

    #[test]
    fn production_monitor_capacity_is_finite_and_nonzero() {
        let set = RuntimeServingCertificationMonitorSetV2::production_v2(NonZeroUsize::MIN);
        assert_eq!(set.capacity.get(), 1);
        assert!(set.monitors.is_empty());
    }

    #[tokio::test]
    async fn empty_monitor_set_never_emits_a_terminal_event() {
        let mut set = RuntimeServingCertificationMonitorSetV2::production_v2(NonZeroUsize::MIN);
        let observed =
            tokio::time::timeout(Duration::from_millis(10), set.next_terminal_v2()).await;
        assert!(observed.is_err());
    }
}
