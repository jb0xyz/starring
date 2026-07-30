use std::num::{NonZeroU64, NonZeroUsize};
use std::time::{Duration, Instant};

use automation_runtime_controller::RuntimeIngressOpenAcknowledgementLeaseDurationV2;
use automation_runtime_worker::{
    RuntimeIngressOpenAcknowledgementPortV2, RuntimeServingOpenSupervisorConfigV2,
};

use crate::closed_recovery::{
    RuntimeClosedRecoveryIngressAcknowledgementOutcomeV2,
    RuntimeClosedRecoveryIngressAcknowledgementRetainedStateV2,
    RuntimeClosedRecoveryServingAcknowledgementEvidenceV2,
    RuntimeClosedRecoveryServingOpenEvidenceV2,
    RuntimeClosedRecoverySupervisedServingOpenProcessV2,
};
use crate::discord::RuntimeDiscordProcessSupervisorV2;
use crate::ingress_acknowledgement_safety::RuntimeIngressAcknowledgementSafetyMonitorV2;
use crate::maintenance_ingress_gate::RuntimeMaintenanceIngressGateOpenAuthorityV2;
use crate::runtime_controller::{
    RuntimeServingControllerEventV2, RuntimeServingControllerSupervisorV2,
};

use super::observation::{
    collect_recovery_resume_database_evidence_v2, exact_reobserve_ingress_acknowledgement_v2,
    execute_ingress_acknowledgement_v2, finish_production_handoff_transition_v2,
    ingress_acknowledgement_schedule_v2, maintenance_gate_is_open_v2,
    map_empty_open_shutdown_cause_v2, map_production_lifecycle_failure_v2,
    map_worker_production_handoff_failure_v2, production_handoff_shutdown_failure_v2,
    shutdown_orphaned_admission_process_v2, shutdown_orphaned_empty_open_process_v2,
    shutdown_orphaned_ingress_acknowledgement_v2, shutdown_process_without_lifecycle_v2,
    shutdown_refreshing_serving_open_process_v2, shutdown_serving_open_process_v2,
    RuntimeEmptyOpenMonitorV2, RuntimeEmptyOpenProcessV2, RuntimeIngressAcknowledgementCleanupV2,
    RuntimeIngressAcknowledgementScheduleV2, RuntimeLifecycleLostRegistryObservationV2,
    RuntimeProcessIngressAcknowledgementExecutionFailureV2, RuntimeProcessProductionHandoffErrorV2,
    RuntimeProcessProductionHandoffFailureV2,
};
use super::{RuntimeProcessFoundationV1, RuntimeProcessIngressAcknowledgementSupervisorV2};

const SERVING_OPEN_SLOT_WORK_CAPACITY_V2: usize = 1;
const INGRESS_ACKNOWLEDGEMENT_LEASE_V2: Duration = Duration::from_secs(10);

pub(crate) struct RuntimeServingOpenProcessV2 {
    controller: RuntimeServingControllerSupervisorV2,
    discord: RuntimeDiscordProcessSupervisorV2,
    foundation: RuntimeProcessFoundationV1,
    lifecycle: RuntimeClosedRecoverySupervisedServingOpenProcessV2,
    maintenance_ingress: RuntimeMaintenanceIngressGateOpenAuthorityV2,
    readiness: crate::health::RuntimeHealthReadinessPublisherV2,
    ingress_acknowledgement: RuntimeProcessIngressAcknowledgementSupervisorV2,
    acknowledgement_schedule: RuntimeIngressAcknowledgementScheduleV2,
    acknowledgement_safety: RuntimeIngressAcknowledgementSafetyMonitorV2,
    process_generation: NonZeroU64,
}

impl RuntimeEmptyOpenProcessV2 {
    pub(crate) async fn enter_serving_open_v2(
        self,
    ) -> Result<RuntimeServingOpenProcessV2, RuntimeProcessProductionHandoffErrorV2> {
        if let Err(transition) = self.revalidate_v2() {
            return Err(self.cleanup_transition_v2(transition).await);
        }
        let config = RuntimeServingOpenSupervisorConfigV2::new(
            NonZeroUsize::new(SERVING_OPEN_SLOT_WORK_CAPACITY_V2)
                .expect("serving slot work capacity must remain nonzero"),
        )
        .expect("serving slot work capacity must remain bounded");
        let database = match collect_recovery_resume_database_evidence_v2(&self.foundation).await {
            Ok(database) => database,
            Err(transition) => return Err(self.cleanup_transition_v2(transition).await),
        };
        let (readiness_evidence, writer_fence_generation) = database.into_parts_v2();
        let gate = self
            .foundation
            .maintenance_ingress_observer_v2()
            .snapshot_v2();
        let finalizer_accepting = self
            .foundation
            .process_finalizer_health_v2()
            .is_some_and(|health| health.is_ready());
        let supervisors_running = finalizer_accepting
            && self.lifecycle.owner_terminal_status_v2().is_none()
            && self.discord.terminal_status_v2().is_none()
            && !self.discord.is_finished_v2()
            && production_handoff_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
                .is_none();
        let maintenance_gate_open =
            maintenance_gate_is_open_v2(gate, self.maintenance_ingress.generation());
        if !maintenance_gate_open || !supervisors_running {
            return Err(self
                .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::ProtocolViolation)
                .await);
        }
        let owner_receipt = match self.lifecycle.observe_current_owner_v2().await {
            Ok(owner) => owner.receipt().clone(),
            Err(_) => {
                return Err(self
                    .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::Owner)
                    .await);
            }
        };
        let predecessor_authorization = self
            .lifecycle
            .authorize_ingress_acknowledgement_predecessor_observation_v2();
        let predecessor_observation = match self
            .foundation
            .databases
            .execution()
            .observe_ingress_open_acknowledgement_predecessor(&predecessor_authorization)
            .await
        {
            Ok(observation) => observation,
            Err(_) => {
                return Err(self
                    .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::Database)
                    .await);
            }
        };
        let ingress_acknowledgement_predecessor =
            match predecessor_authorization.accept(predecessor_observation) {
                Ok(predecessor) => predecessor,
                Err(_) => {
                    return Err(self
                        .cleanup_transition_v2(
                            RuntimeProcessProductionHandoffFailureV2::ProtocolViolation,
                        )
                        .await);
                }
            };
        let serving_evidence = RuntimeClosedRecoveryServingOpenEvidenceV2 {
            owner_receipt,
            readiness: readiness_evidence,
            writer_fence_generation,
            maintenance_gate_generation: gate.generation(),
            maintenance_gate_open,
            finalizer_generation: self.lifecycle.finalizer_generation_v2(),
            finalizer_accepting,
            supervisors_running,
            ingress_acknowledgement_predecessor,
        };
        let Self {
            discord,
            foundation,
            lifecycle,
            maintenance_ingress,
            readiness,
            ingress_acknowledgement,
            acknowledgement_schedule,
            acknowledgement_safety,
            process_generation,
        } = self;
        let prepared = match lifecycle.prepare_serving_open_v2(config, serving_evidence) {
            Ok(prepared) => prepared,
            Err(failure) => {
                let transition = map_worker_production_handoff_failure_v2(failure.error_v2());
                let process = RuntimeEmptyOpenProcessV2 {
                    discord,
                    foundation,
                    lifecycle: failure.into_state_v2(),
                    maintenance_ingress,
                    readiness,
                    ingress_acknowledgement,
                    acknowledgement_schedule,
                    acknowledgement_safety,
                    process_generation,
                };
                return Err(process.cleanup_transition_v2(transition).await);
            }
        };
        let lifecycle = match prepared.commit_v2() {
            Ok(lifecycle) => lifecycle,
            Err(failure) => {
                let transition = map_worker_production_handoff_failure_v2(failure.error_v2());
                let process = RuntimeEmptyOpenProcessV2 {
                    discord,
                    foundation,
                    lifecycle: failure.into_state_v2(),
                    maintenance_ingress,
                    readiness,
                    ingress_acknowledgement,
                    acknowledgement_schedule,
                    acknowledgement_safety,
                    process_generation,
                };
                return Err(process.cleanup_transition_v2(transition).await);
            }
        };
        let controller = RuntimeServingControllerSupervisorV2::start_v2(
            foundation.databases.runtime_controller_v2(),
            foundation
                .secrets
                .discord_bot_token()
                .expose_secret()
                .to_owned(),
            lifecycle.staging_port_v2(),
            foundation.controller_id.clone(),
            foundation.shutdown_trigger_v1(),
        );
        let process = RuntimeServingOpenProcessV2 {
            controller,
            discord,
            foundation,
            lifecycle,
            maintenance_ingress,
            readiness,
            ingress_acknowledgement,
            acknowledgement_schedule,
            acknowledgement_safety,
            process_generation,
        };
        if let Err(transition) = process.revalidate_v2().await {
            Err(process.cleanup_transition_v2(transition).await)
        } else {
            Ok(process)
        }
    }
}

impl RuntimeServingOpenProcessV2 {
    async fn revalidate_v2(&self) -> Result<(), RuntimeProcessProductionHandoffFailureV2> {
        if self.discord.terminal_status_v2().is_some() || self.discord.is_finished_v2() {
            return Err(RuntimeProcessProductionHandoffFailureV2::DiscordIndeterminate);
        }
        if self.lifecycle.process_generation_v2() != self.process_generation {
            return Err(RuntimeProcessProductionHandoffFailureV2::Owner);
        }
        if !self
            .foundation
            .process_finalizer_health_v2()
            .is_some_and(|health| health.is_ready())
        {
            return Err(RuntimeProcessProductionHandoffFailureV2::FinalizerTerminal);
        }
        let gate = self
            .foundation
            .maintenance_ingress_observer_v2()
            .snapshot_v2();
        if !maintenance_gate_is_open_v2(gate, self.maintenance_ingress.generation()) {
            return Err(RuntimeProcessProductionHandoffFailureV2::MaintenanceGate);
        }
        self.lifecycle
            .revalidate_v2()
            .await
            .map_err(map_worker_production_handoff_failure_v2)?;
        production_handoff_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
            .map_or(Ok(()), Err)
    }

    async fn refresh_acknowledgement_v2(
        self,
    ) -> Result<Self, RuntimeProcessProductionHandoffErrorV2> {
        if Instant::now() >= self.acknowledgement_schedule.safety_deadline {
            return Err(self
                .cleanup_transition_v2(
                    RuntimeProcessProductionHandoffFailureV2::OperationDeadlineElapsed,
                )
                .await);
        }
        if let Err(transition) = self.revalidate_v2().await {
            return Err(self.cleanup_transition_v2(transition).await);
        }
        let database = match collect_recovery_resume_database_evidence_v2(&self.foundation).await {
            Ok(database) => database,
            Err(transition) => return Err(self.cleanup_transition_v2(transition).await),
        };
        let (readiness, writer_fence_generation) = database.into_parts_v2();
        let gateway_ready = match self.lifecycle.observe_exact_current_ready_attestation_v2() {
            Ok(ready) => ready,
            Err(_) => {
                return Err(self
                    .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::Gateway)
                    .await);
            }
        };
        let gate = self
            .foundation
            .maintenance_ingress_observer_v2()
            .snapshot_v2();
        let finalizer_accepting = self
            .foundation
            .process_finalizer_health_v2()
            .is_some_and(|health| health.is_ready());
        let supervisors_running = finalizer_accepting
            && self.lifecycle.owner_terminal_status_v2().is_none()
            && self.discord.terminal_status_v2().is_none()
            && !self.discord.is_finished_v2()
            && production_handoff_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
                .is_none();
        if !maintenance_gate_is_open_v2(gate, self.maintenance_ingress.generation())
            || !supervisors_running
        {
            return Err(self
                .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::ProtocolViolation)
                .await);
        }
        let owner_receipt = match self.lifecycle.observe_current_owner_v2().await {
            Ok(owner) => owner.receipt().clone(),
            Err(_) => {
                return Err(self
                    .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::Owner)
                    .await);
            }
        };
        let predecessor_authorization = self
            .lifecycle
            .authorize_ingress_acknowledgement_predecessor_observation_v2();
        let final_authorization = self
            .lifecycle
            .authorize_ingress_acknowledgement_predecessor_observation_v2();
        let predecessor_observation = match self
            .foundation
            .databases
            .execution()
            .observe_ingress_open_acknowledgement_predecessor(&predecessor_authorization)
            .await
        {
            Ok(observation) => observation,
            Err(_) => {
                return Err(self
                    .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::Database)
                    .await);
            }
        };
        let predecessor = match predecessor_authorization.accept(predecessor_observation) {
            Ok(predecessor) => predecessor,
            Err(_) => {
                return Err(self
                    .cleanup_transition_v2(
                        RuntimeProcessProductionHandoffFailureV2::ProtocolViolation,
                    )
                    .await);
            }
        };
        let lease_for = RuntimeIngressOpenAcknowledgementLeaseDurationV2::from_duration(
            INGRESS_ACKNOWLEDGEMENT_LEASE_V2,
        )
        .expect("bounded ingress acknowledgement lease");
        let evidence = RuntimeClosedRecoveryServingAcknowledgementEvidenceV2 {
            owner_receipt,
            readiness,
            gateway_ready,
            writer_fence_generation,
            maintenance_gate_generation: gate.generation(),
            maintenance_gate_open: true,
            finalizer_generation: self.lifecycle.finalizer_generation_v2(),
            finalizer_accepting,
            supervisors_running,
            predecessor,
            lease_for,
        };
        let Self {
            mut controller,
            discord,
            mut foundation,
            lifecycle,
            maintenance_ingress,
            readiness,
            mut ingress_acknowledgement,
            acknowledgement_schedule,
            acknowledgement_safety,
            process_generation,
        } = self;
        let refresh = match lifecycle.authorize_acknowledgement_refresh_v2(evidence) {
            Ok(refresh) => refresh,
            Err(failure) => {
                let transition = map_production_lifecycle_failure_v2(failure.error_v2());
                let process = Self {
                    controller,
                    discord,
                    foundation,
                    lifecycle: failure.into_state_v2(),
                    maintenance_ingress,
                    readiness,
                    ingress_acknowledgement,
                    acknowledgement_schedule,
                    acknowledgement_safety,
                    process_generation,
                };
                return Err(process.cleanup_transition_v2(transition).await);
            }
        };
        let authority = refresh.into_ingress_acknowledgement_authority_v2();
        let acknowledgement = execute_ingress_acknowledgement_v2(
            &mut ingress_acknowledgement,
            authority,
            acknowledgement_schedule.safety_deadline,
            foundation.lifecycle_timing_v2(),
        )
        .await;
        let (lifecycle, accepted_receipt) = match acknowledgement {
            Ok(RuntimeClosedRecoveryIngressAcknowledgementOutcomeV2::ServingOpenRefresh {
                lifecycle,
                accepted_receipt,
            }) => (*lifecycle, *accepted_receipt),
            Ok(RuntimeClosedRecoveryIngressAcknowledgementOutcomeV2::Admission {
                lifecycle,
                ..
            }) => {
                stop_runtime_controller_before_cleanup_v2(&mut controller, &mut foundation).await;
                let cleanup = shutdown_orphaned_admission_process_v2(
                    foundation,
                    discord,
                    *lifecycle,
                    RuntimeIngressAcknowledgementCleanupV2::new_v2(
                        ingress_acknowledgement,
                        Some(acknowledgement_safety),
                    ),
                    maintenance_ingress,
                    readiness,
                    process_generation,
                )
                .await;
                return Err(finish_production_handoff_transition_v2(
                    RuntimeProcessProductionHandoffFailureV2::ProtocolViolation,
                    cleanup,
                ));
            }
            Ok(RuntimeClosedRecoveryIngressAcknowledgementOutcomeV2::EmptyOpenRefresh {
                lifecycle,
                ..
            }) => {
                stop_runtime_controller_before_cleanup_v2(&mut controller, &mut foundation).await;
                let cleanup = shutdown_orphaned_empty_open_process_v2(
                    foundation,
                    discord,
                    *lifecycle,
                    RuntimeIngressAcknowledgementCleanupV2::new_v2(
                        ingress_acknowledgement,
                        Some(acknowledgement_safety),
                    ),
                    maintenance_ingress,
                    readiness,
                    process_generation,
                )
                .await;
                return Err(finish_production_handoff_transition_v2(
                    RuntimeProcessProductionHandoffFailureV2::ProtocolViolation,
                    cleanup,
                ));
            }
            Err(RuntimeProcessIngressAcknowledgementExecutionFailureV2::Retained {
                authority,
                transition,
            }) => {
                let retained = (*authority).into_retained_state_v2();
                let RuntimeClosedRecoveryIngressAcknowledgementRetainedStateV2::ServingOpenRefresh(
                    refresh,
                ) = retained
                else {
                    stop_runtime_controller_before_cleanup_v2(&mut controller, &mut foundation)
                        .await;
                    let cleanup = shutdown_orphaned_ingress_acknowledgement_v2(
                        foundation,
                        discord,
                        retained,
                        RuntimeIngressAcknowledgementCleanupV2::new_v2(
                            ingress_acknowledgement,
                            Some(acknowledgement_safety),
                        ),
                        maintenance_ingress,
                        readiness,
                        process_generation,
                    )
                    .await;
                    return Err(finish_production_handoff_transition_v2(transition, cleanup));
                };
                stop_runtime_controller_before_cleanup_v2(&mut controller, &mut foundation).await;
                let cleanup = shutdown_refreshing_serving_open_process_v2(
                    foundation,
                    discord,
                    *refresh,
                    RuntimeIngressAcknowledgementCleanupV2::new_v2(
                        ingress_acknowledgement,
                        Some(acknowledgement_safety),
                    ),
                    maintenance_ingress,
                    readiness,
                    process_generation,
                )
                .await;
                return Err(finish_production_handoff_transition_v2(transition, cleanup));
            }
            Err(RuntimeProcessIngressAcknowledgementExecutionFailureV2::AuthorityLost(
                transition,
            )) => {
                stop_runtime_controller_before_cleanup_v2(&mut controller, &mut foundation).await;
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
                return Err(finish_production_handoff_transition_v2(transition, cleanup));
            }
        };
        let final_observation_started_at = Instant::now();
        let final_observation = exact_reobserve_ingress_acknowledgement_v2(
            foundation.databases.execution(),
            final_authorization,
            &accepted_receipt,
        )
        .await;
        let schedule = final_observation.as_ref().ok().and_then(|receipt| {
            ingress_acknowledgement_schedule_v2(receipt, final_observation_started_at)
        });
        let process = Self {
            controller,
            discord,
            foundation,
            lifecycle,
            maintenance_ingress,
            readiness,
            ingress_acknowledgement,
            acknowledgement_schedule: schedule.unwrap_or(acknowledgement_schedule),
            acknowledgement_safety,
            process_generation,
        };
        match (final_observation, schedule) {
            (Ok(_), Some(schedule)) => {
                if !process
                    .acknowledgement_safety
                    .rearm_v2(schedule.safety_deadline)
                {
                    Err(process
                        .cleanup_transition_v2(
                            RuntimeProcessProductionHandoffFailureV2::OperationDeadlineElapsed,
                        )
                        .await)
                } else if let Err(transition) = process.revalidate_v2().await {
                    Err(process.cleanup_transition_v2(transition).await)
                } else {
                    Ok(process)
                }
            }
            (Err(transition), _) => Err(process.cleanup_transition_v2(transition).await),
            (Ok(_), None) => Err(process
                .cleanup_transition_v2(
                    RuntimeProcessProductionHandoffFailureV2::OperationDeadlineElapsed,
                )
                .await),
        }
    }

    pub(crate) async fn run_until_shutdown_v2(
        mut self,
    ) -> Result<(), RuntimeProcessProductionHandoffErrorV2> {
        let gateway_ready = match self.lifecycle.observe_exact_current_ready_attestation_v2() {
            Ok(gateway_ready) => gateway_ready,
            Err(_) => {
                self.foundation
                    .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::ReadinessLost);
                return Err(self
                    .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::Gateway)
                    .await);
            }
        };
        let gateway_invalidation = self
            .lifecycle
            .bind_gateway_ready_invalidation_observer_v2(&gateway_ready);
        if gateway_invalidation.current_invalidation_v2().is_some() {
            self.foundation
                .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::ReadinessLost);
            return Err(self
                .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::Gateway)
                .await);
        }
        let trigger = self.foundation.shutdown_trigger_v1();
        let mut shutdown_for_monitor = self.foundation.shutdown_observer_v1();
        let mut discord_terminal = self.discord.observation_v2();
        let owner_terminal = self.lifecycle.owner_terminal_observation_v2();
        let monitor = RuntimeEmptyOpenMonitorV2::start(async move {
            tokio::select! {
                biased;
                _ = shutdown_for_monitor.wait() => {}
                _ = gateway_invalidation.wait_v2() => {
                    trigger.trip(crate::RuntimeShutdownCauseV1::ReadinessLost);
                }
                _ = discord_terminal.wait_terminal() => {
                    trigger.trip(crate::RuntimeShutdownCauseV1::DiscordTerminal);
                }
                _ = owner_terminal => {
                    trigger.trip(crate::RuntimeShutdownCauseV1::GatewayOwnerTerminal);
                }
            }
        });
        let mut shutdown = self.foundation.shutdown_observer_v1();
        enum RuntimeServingOpenLoopEventV2 {
            Shutdown(crate::RuntimeShutdownObservationV1),
            Controller(RuntimeServingControllerEventV2),
            RefreshAcknowledgement,
        }
        let observation = loop {
            if let Some(observation) = shutdown.observed() {
                break observation;
            }
            if Instant::now() >= self.acknowledgement_schedule.safety_deadline {
                break self
                    .foundation
                    .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::ReadinessLost);
            }
            let refresh_at = self.acknowledgement_schedule.refresh_at;
            let selected = tokio::select! {
                biased;
                observation = shutdown.wait() => {
                    RuntimeServingOpenLoopEventV2::Shutdown(observation)
                },
                event = self.controller.next_event_v2() => {
                    RuntimeServingOpenLoopEventV2::Controller(event)
                },
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(refresh_at)) => {
                    RuntimeServingOpenLoopEventV2::RefreshAcknowledgement
                },
            };
            match selected {
                RuntimeServingOpenLoopEventV2::Shutdown(observation) => break observation,
                RuntimeServingOpenLoopEventV2::Controller(
                    RuntimeServingControllerEventV2::AcquireSlot { slot, reply },
                ) => {
                    let request = self.lifecycle.authorize_slot_work_v2(slot);
                    let permit = self.lifecycle.begin_slot_work_v2(request);
                    let _ = reply.send(permit);
                }
                RuntimeServingOpenLoopEventV2::Controller(
                    RuntimeServingControllerEventV2::Terminal,
                ) => {
                    break self
                        .foundation
                        .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::SupervisorFailure);
                }
                RuntimeServingOpenLoopEventV2::RefreshAcknowledgement => {
                    match self.refresh_acknowledgement_v2().await {
                        Ok(process) => self = process,
                        Err(error) => {
                            monitor.stop_v2().await;
                            return Err(error);
                        }
                    }
                }
            }
        };
        monitor.stop_v2().await;
        let cleanup = self.shutdown().await;
        match observation.cause() {
            crate::RuntimeShutdownCauseV1::Interrupt
            | crate::RuntimeShutdownCauseV1::Terminate
            | crate::RuntimeShutdownCauseV1::Explicit => cleanup.map_err(|cleanup| {
                finish_production_handoff_transition_v2(
                    RuntimeProcessProductionHandoffFailureV2::ProcessShutdown,
                    Err(cleanup),
                )
            }),
            cause => Err(finish_production_handoff_transition_v2(
                map_empty_open_shutdown_cause_v2(cause),
                cleanup,
            )),
        }
    }

    async fn shutdown(mut self) -> Result<(), crate::RuntimeClosedRecoveryProcessCleanupFailureV2> {
        let _revalidation = self.revalidate_v2().await;
        stop_runtime_controller_before_cleanup_v2(&mut self.controller, &mut self.foundation).await;
        let Self {
            controller,
            discord,
            foundation,
            lifecycle,
            maintenance_ingress,
            readiness,
            ingress_acknowledgement,
            acknowledgement_schedule: _,
            acknowledgement_safety,
            process_generation,
        } = self;
        drop(controller);
        shutdown_serving_open_process_v2(
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
        .await
    }

    async fn cleanup_transition_v2(
        self,
        transition: RuntimeProcessProductionHandoffFailureV2,
    ) -> RuntimeProcessProductionHandoffErrorV2 {
        let cleanup = self.shutdown().await;
        finish_production_handoff_transition_v2(transition, cleanup)
    }
}

async fn stop_runtime_controller_before_cleanup_v2(
    controller: &mut RuntimeServingControllerSupervisorV2,
    foundation: &mut RuntimeProcessFoundationV1,
) {
    let observation = foundation.trip_shutdown_v1(crate::RuntimeShutdownCauseV1::Explicit);
    let deadline = foundation.effective_shutdown_deadline_v1(observation);
    if controller.shutdown_until_v2(deadline).await.is_err() {
        foundation.record_runtime_controller_shutdown_failure_v2();
        foundation.trip_shutdown_v1(crate::RuntimeShutdownCauseV1::SupervisorFailure);
    }
}
