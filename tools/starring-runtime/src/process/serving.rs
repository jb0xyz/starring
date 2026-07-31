use std::num::{NonZeroU64, NonZeroUsize};
use std::time::{Duration, Instant};

use automation_runtime_controller::RuntimeIngressOpenAcknowledgementLeaseDurationV2;
use automation_runtime_worker::{
    RuntimeIngressOpenAcknowledgementPortV2, RuntimeServingOpenSupervisorConfigV2,
};
use chrono::{DateTime, Utc};

use crate::closed_recovery::{
    RuntimeClosedRecoveryIngressAcknowledgementOutcomeV2,
    RuntimeClosedRecoveryIngressAcknowledgementRetainedStateV2,
    RuntimeClosedRecoveryServingAcknowledgementEvidenceV2,
    RuntimeClosedRecoveryServingOpenEvidenceV2,
    RuntimeClosedRecoverySupervisedServingOpenProcessV2,
};
use crate::discord::RuntimeDiscordProcessSupervisorV2;
use crate::gateway::{
    RuntimeDiscordOrdinaryBarrierFailureV3, RuntimeDiscordOrdinaryBarrierPauseOutcomeV3,
    RuntimeDiscordOrdinaryBarrierReservationV3, RuntimeDiscordOrdinaryBarrierResumeOutcomeV3,
};
use crate::ingress_acknowledgement_safety::RuntimeIngressAcknowledgementSafetyMonitorV2;
use crate::maintenance_ingress_gate::RuntimeMaintenanceIngressGateOpenAuthorityV2;
use crate::runtime_controller::{
    RuntimeControllerBarrierAPauseCommandV2, RuntimeControllerBarrierAPausedV2,
    RuntimeControllerBarrierAResumedV2, RuntimeControllerBarrierBridgeErrorV2,
    RuntimeServingControllerEventV2, RuntimeServingControllerSupervisorV2,
};

use super::observation::{
    collect_recovery_resume_database_evidence_v2, exact_reobserve_ingress_acknowledgement_v2,
    execute_ingress_acknowledgement_v2, finish_production_handoff_transition_v2,
    ingress_acknowledgement_schedule_v2, maintenance_gate_is_open_v2,
    map_empty_open_shutdown_cause_v2, map_production_lifecycle_failure_v2,
    map_worker_production_handoff_failure_v2, production_open_shutdown_failure_v2,
    shutdown_orphaned_admission_process_v2, shutdown_orphaned_empty_open_process_v2,
    shutdown_orphaned_ingress_acknowledgement_v2, shutdown_process_without_lifecycle_v2,
    shutdown_refreshing_serving_open_process_v2, shutdown_serving_open_process_v2,
    RuntimeEmptyOpenMonitorV2, RuntimeEmptyOpenProcessV2,
    RuntimeExactIngressAcknowledgementReobservationV3, RuntimeIngressAcknowledgementCleanupV2,
    RuntimeIngressAcknowledgementScheduleV2, RuntimeLifecycleLostRegistryObservationV2,
    RuntimeProcessIngressAcknowledgementExecutionFailureV2, RuntimeProcessProductionHandoffErrorV2,
    RuntimeProcessProductionHandoffFailureV2,
};
use super::{RuntimeProcessFoundationV1, RuntimeProcessIngressAcknowledgementSupervisorV2};

const SERVING_OPEN_SLOT_WORK_CAPACITY_V2: usize = 1;
const INGRESS_ACKNOWLEDGEMENT_LEASE_V2: Duration = Duration::from_secs(10);

struct RuntimeServingBarrierAPausedStateV3 {
    handle: NonZeroU64,
    command: RuntimeControllerBarrierAPauseCommandV2,
    reservation: RuntimeDiscordOrdinaryBarrierReservationV3,
}

pub(crate) struct RuntimeServingOpenProcessV2 {
    pub(super) controller: RuntimeServingControllerSupervisorV2,
    pub(super) discord: RuntimeDiscordProcessSupervisorV2,
    pub(super) foundation: RuntimeProcessFoundationV1,
    pub(super) lifecycle: RuntimeClosedRecoverySupervisedServingOpenProcessV2,
    pub(super) maintenance_ingress: RuntimeMaintenanceIngressGateOpenAuthorityV2,
    pub(super) readiness: crate::health::RuntimeHealthReadinessPublisherV2,
    pub(super) ingress_acknowledgement: RuntimeProcessIngressAcknowledgementSupervisorV2,
    pub(super) acknowledgement_schedule: RuntimeIngressAcknowledgementScheduleV2,
    pub(super) acknowledgement_safety: RuntimeIngressAcknowledgementSafetyMonitorV2,
    pub(super) certification_monitors:
        super::serving_certification::RuntimeServingCertificationMonitorSetV2,
    pub(super) process_generation: NonZeroU64,
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
        let database = collect_recovery_resume_database_evidence_v2(&self.foundation).await;
        if let Some(transition) =
            production_open_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
        {
            return Err(self.cleanup_transition_v2(transition).await);
        }
        let database = match database {
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
        if let Some(transition) =
            production_open_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
        {
            return Err(self.cleanup_transition_v2(transition).await);
        }
        let supervisors_running = finalizer_accepting
            && self.lifecycle.owner_terminal_status_v2().is_none()
            && self.discord.terminal_status_v2().is_none()
            && !self.discord.is_finished_v2();
        let maintenance_gate_open =
            maintenance_gate_is_open_v2(gate, self.maintenance_ingress.generation());
        if !maintenance_gate_open || !supervisors_running {
            let transition =
                production_open_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
                    .unwrap_or(RuntimeProcessProductionHandoffFailureV2::ProtocolViolation);
            return Err(self.cleanup_transition_v2(transition).await);
        }
        let owner_receipt = self.lifecycle.observe_current_owner_v2().await;
        if let Some(transition) =
            production_open_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
        {
            return Err(self.cleanup_transition_v2(transition).await);
        }
        let owner_receipt = match owner_receipt {
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
        let predecessor_observation = self
            .foundation
            .databases
            .execution()
            .observe_ingress_open_acknowledgement_predecessor(&predecessor_authorization)
            .await;
        if let Some(transition) =
            production_open_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
        {
            return Err(self.cleanup_transition_v2(transition).await);
        }
        let predecessor_observation = match predecessor_observation {
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
            foundation.config.runtime_controller(),
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
            certification_monitors:
                super::serving_certification::RuntimeServingCertificationMonitorSetV2::production_v2(
                    crate::registry::runtime_registry_max_slots_v2(),
                ),
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
    pub(super) async fn revalidate_v2(
        &self,
    ) -> Result<(), RuntimeProcessProductionHandoffFailureV2> {
        if let Some(transition) =
            production_open_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
        {
            return Err(transition);
        }
        if self.discord.terminal_status_v2().is_some() || self.discord.is_finished_v2() {
            return Err(production_open_shutdown_failure_v2(
                &self.foundation.shutdown_observer_v1(),
            )
            .unwrap_or(RuntimeProcessProductionHandoffFailureV2::DiscordIndeterminate));
        }
        if self.lifecycle.process_generation_v2() != self.process_generation {
            return Err(production_open_shutdown_failure_v2(
                &self.foundation.shutdown_observer_v1(),
            )
            .unwrap_or(RuntimeProcessProductionHandoffFailureV2::Owner));
        }
        if !self
            .foundation
            .process_finalizer_health_v2()
            .is_some_and(|health| health.is_ready())
        {
            return Err(production_open_shutdown_failure_v2(
                &self.foundation.shutdown_observer_v1(),
            )
            .unwrap_or(RuntimeProcessProductionHandoffFailureV2::FinalizerTerminal));
        }
        let gate = self
            .foundation
            .maintenance_ingress_observer_v2()
            .snapshot_v2();
        if !maintenance_gate_is_open_v2(gate, self.maintenance_ingress.generation()) {
            return Err(production_open_shutdown_failure_v2(
                &self.foundation.shutdown_observer_v1(),
            )
            .unwrap_or(RuntimeProcessProductionHandoffFailureV2::MaintenanceGate));
        }
        let lifecycle = self.lifecycle.revalidate_v2().await;
        if let Some(transition) =
            production_open_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
        {
            return Err(transition);
        }
        lifecycle.map_err(map_worker_production_handoff_failure_v2)?;
        production_open_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
            .map_or(Ok(()), Err)
    }

    async fn refresh_acknowledgement_v2(
        self,
    ) -> Result<Self, RuntimeProcessProductionHandoffErrorV2> {
        let gateway_ready = self.lifecycle.observe_exact_current_ready_attestation_v2();
        if let Some(transition) =
            production_open_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
        {
            return Err(self.cleanup_transition_v2(transition).await);
        }
        let gateway_ready = match gateway_ready {
            Ok(ready) => ready,
            Err(_) => {
                return Err(self
                    .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::Gateway)
                    .await);
            }
        };
        self.refresh_acknowledgement_with_ready_v3(gateway_ready, true)
            .await
            .map(|(process, _)| process)
    }

    async fn refresh_acknowledgement_with_ready_v3(
        self,
        gateway_ready: automation_runtime_controller::RuntimeGatewayReadyAttestationV2,
        require_normal_revalidation: bool,
    ) -> Result<
        (Self, RuntimeExactIngressAcknowledgementReobservationV3),
        RuntimeProcessProductionHandoffErrorV2,
    > {
        if Instant::now() >= self.acknowledgement_schedule.safety_deadline {
            return Err(self
                .cleanup_transition_v2(
                    RuntimeProcessProductionHandoffFailureV2::OperationDeadlineElapsed,
                )
                .await);
        }
        if require_normal_revalidation {
            if let Err(transition) = self.revalidate_v2().await {
                return Err(self.cleanup_transition_v2(transition).await);
            }
        } else if let Some(transition) =
            production_open_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
        {
            return Err(self.cleanup_transition_v2(transition).await);
        }
        let database = collect_recovery_resume_database_evidence_v2(&self.foundation).await;
        if let Some(transition) =
            production_open_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
        {
            return Err(self.cleanup_transition_v2(transition).await);
        }
        let database = match database {
            Ok(database) => database,
            Err(transition) => return Err(self.cleanup_transition_v2(transition).await),
        };
        let (readiness, writer_fence_generation) = database.into_parts_v2();
        let gate = self
            .foundation
            .maintenance_ingress_observer_v2()
            .snapshot_v2();
        let finalizer_accepting = self
            .foundation
            .process_finalizer_health_v2()
            .is_some_and(|health| health.is_ready());
        if let Some(transition) =
            production_open_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
        {
            return Err(self.cleanup_transition_v2(transition).await);
        }
        let supervisors_running = finalizer_accepting
            && self.lifecycle.owner_terminal_status_v2().is_none()
            && self.discord.terminal_status_v2().is_none()
            && !self.discord.is_finished_v2();
        if !maintenance_gate_is_open_v2(gate, self.maintenance_ingress.generation())
            || !supervisors_running
        {
            let transition =
                production_open_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
                    .unwrap_or(RuntimeProcessProductionHandoffFailureV2::ProtocolViolation);
            return Err(self.cleanup_transition_v2(transition).await);
        }
        let owner_receipt = self.lifecycle.observe_current_owner_v2().await;
        if let Some(transition) =
            production_open_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
        {
            return Err(self.cleanup_transition_v2(transition).await);
        }
        let owner_receipt = match owner_receipt {
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
        let predecessor_observation = self
            .foundation
            .databases
            .execution()
            .observe_ingress_open_acknowledgement_predecessor(&predecessor_authorization)
            .await;
        if let Some(transition) =
            production_open_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
        {
            return Err(self.cleanup_transition_v2(transition).await);
        }
        let predecessor_observation = match predecessor_observation {
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
            maintenance_gate_opening: false,
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
            certification_monitors,
            process_generation,
        } = self;
        let refresh = match if require_normal_revalidation {
            lifecycle.authorize_acknowledgement_refresh_v2(evidence)
        } else {
            lifecycle.authorize_resumed_acknowledgement_refresh_v3(evidence)
        } {
            Ok(refresh) => refresh,
            Err(failure) => {
                let transition =
                    production_open_shutdown_failure_v2(&foundation.shutdown_observer_v1())
                        .unwrap_or_else(|| map_production_lifecycle_failure_v2(failure.error_v2()));
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
                    certification_monitors,
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
        let shutdown_transition =
            production_open_shutdown_failure_v2(&foundation.shutdown_observer_v1());
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
                let transition = shutdown_transition
                    .unwrap_or(RuntimeProcessProductionHandoffFailureV2::ProtocolViolation);
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
                return Err(finish_production_handoff_transition_v2(transition, cleanup));
            }
            Ok(RuntimeClosedRecoveryIngressAcknowledgementOutcomeV2::EmptyOpenRefresh {
                lifecycle,
                ..
            }) => {
                stop_runtime_controller_before_cleanup_v2(&mut controller, &mut foundation).await;
                let transition = shutdown_transition
                    .unwrap_or(RuntimeProcessProductionHandoffFailureV2::ProtocolViolation);
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
                return Err(finish_production_handoff_transition_v2(transition, cleanup));
            }
            Err(RuntimeProcessIngressAcknowledgementExecutionFailureV2::Retained {
                authority,
                transition,
            }) => {
                let retained = (*authority).into_retained_state_v2();
                let transition = shutdown_transition.unwrap_or(transition);
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
                let transition = shutdown_transition.unwrap_or(transition);
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
        if let Some(transition) = shutdown_transition {
            let process = Self {
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
            };
            return Err(process.cleanup_transition_v2(transition).await);
        }
        let final_observation_started_at = Instant::now();
        let final_observation = exact_reobserve_ingress_acknowledgement_v2(
            foundation.databases.execution(),
            final_authorization,
            &accepted_receipt,
        )
        .await;
        let schedule = final_observation.as_ref().ok().and_then(|observation| {
            ingress_acknowledgement_schedule_v2(
                observation.receipt_v3(),
                final_observation_started_at,
            )
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
            certification_monitors,
            process_generation,
        };
        if let Some(transition) =
            production_open_shutdown_failure_v2(&process.foundation.shutdown_observer_v1())
        {
            return Err(process.cleanup_transition_v2(transition).await);
        }
        match (final_observation, schedule) {
            (Ok(receipt), Some(schedule)) => {
                if !process
                    .acknowledgement_safety
                    .rearm_v2(schedule.safety_deadline)
                {
                    Err(process
                        .cleanup_transition_v2(
                            RuntimeProcessProductionHandoffFailureV2::OperationDeadlineElapsed,
                        )
                        .await)
                } else if require_normal_revalidation {
                    if let Err(transition) = process.revalidate_v2().await {
                        Err(process.cleanup_transition_v2(transition).await)
                    } else {
                        Ok((process, receipt))
                    }
                } else {
                    Ok((process, receipt))
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

    pub(super) fn start_gateway_monitor_v2(
        &self,
        gateway_ready: &automation_runtime_controller::RuntimeGatewayReadyAttestationV2,
    ) -> Result<RuntimeEmptyOpenMonitorV2, RuntimeProcessProductionHandoffFailureV2> {
        let gateway_invalidation = self
            .lifecycle
            .bind_gateway_ready_invalidation_observer_v2(gateway_ready);
        if gateway_invalidation.current_invalidation_v2().is_some() {
            return Err(RuntimeProcessProductionHandoffFailureV2::Gateway);
        }
        let trigger = self.foundation.shutdown_trigger_v1();
        let mut shutdown = self.foundation.shutdown_observer_v1();
        let mut discord_terminal = self.discord.observation_v2();
        let owner_terminal = self.lifecycle.owner_terminal_observation_v2();
        Ok(RuntimeEmptyOpenMonitorV2::start(async move {
            tokio::select! {
                biased;
                _ = shutdown.wait() => {}
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
        }))
    }

    pub(crate) async fn run_until_shutdown_v2(
        mut self,
    ) -> Result<(), RuntimeProcessProductionHandoffErrorV2> {
        let gateway_ready = self.lifecycle.observe_exact_current_ready_attestation_v2();
        if let Some(transition) =
            production_open_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
        {
            return Err(self.cleanup_transition_v2(transition).await);
        }
        let gateway_ready = match gateway_ready {
            Ok(gateway_ready) => gateway_ready,
            Err(_) => {
                self.foundation
                    .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::ReadinessLost);
                return Err(self
                    .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::Gateway)
                    .await);
            }
        };
        let monitor = self.start_gateway_monitor_v2(&gateway_ready);
        if let Some(transition) =
            production_open_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
        {
            return Err(self.cleanup_transition_v2(transition).await);
        }
        let mut monitor = Some(match monitor {
            Ok(monitor) => monitor,
            Err(transition) => return Err(self.cleanup_transition_v2(transition).await),
        });
        let mut paused_barrier = None;
        let mut next_barrier_handle = 0u64;
        let mut shutdown = self.foundation.shutdown_observer_v1();
        enum RuntimeServingOpenLoopEventV2 {
            Shutdown(crate::RuntimeShutdownObservationV1),
            Controller(Box<RuntimeServingControllerEventV2>),
            CertificationMonitorTerminal(
                super::serving_certification::RuntimeServingCertificationMonitorTerminalV2,
            ),
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
                    RuntimeServingOpenLoopEventV2::Controller(Box::new(event))
                },
                terminal = self.certification_monitors.next_terminal_v2() => {
                    RuntimeServingOpenLoopEventV2::CertificationMonitorTerminal(terminal)
                },
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(refresh_at)),
                    if paused_barrier.is_none() => {
                    RuntimeServingOpenLoopEventV2::RefreshAcknowledgement
                },
            };
            match selected {
                RuntimeServingOpenLoopEventV2::Shutdown(observation) => break observation,
                RuntimeServingOpenLoopEventV2::CertificationMonitorTerminal(terminal) => {
                    let _ = (
                        terminal.slot_v2(),
                        terminal.status_v2(),
                        terminal.outcomes_v2(),
                    );
                    break self
                        .foundation
                        .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::HealthTerminal);
                }
                RuntimeServingOpenLoopEventV2::Controller(event) => match *event {
                    RuntimeServingControllerEventV2::AcquireSlot { slot, reply } => {
                        let request = self.lifecycle.authorize_slot_work_v2(slot);
                        let permit = self.lifecycle.begin_slot_work_v2(request);
                        let _ = reply.send(permit);
                    }
                    RuntimeServingControllerEventV2::PauseBarrierA { command, reply } => {
                        if paused_barrier.is_some()
                            || Utc::now() < command.started_at
                            || Utc::now() > command.deadline
                        {
                            let _ = reply
                                .send(Err(RuntimeControllerBarrierBridgeErrorV2::StaleAuthority));
                            continue;
                        }
                        if let Some(active) = monitor.take() {
                            active.stop_v2().await;
                        }
                        let port = match self.lifecycle.ordinary_barrier_port_v3() {
                            Ok(port) => port,
                            Err(error) => {
                                let _ = reply.send(Err(map_ordinary_barrier_failure_v3(error)));
                                if let Ok(ready) =
                                    self.lifecycle.observe_exact_current_ready_attestation_v2()
                                {
                                    monitor = self.start_gateway_monitor_v2(&ready).ok();
                                }
                                if monitor.is_none() {
                                    self.foundation.trip_shutdown_v1(
                                        crate::RuntimeShutdownCauseV1::ReadinessLost,
                                    );
                                }
                                continue;
                            }
                        };
                        let generation = self.lifecycle.coordinator_generation_v3();
                        let Some(deadline) = runtime_barrier_deadline_v3(command.deadline) else {
                            let _ = reply
                                .send(Err(RuntimeControllerBarrierBridgeErrorV2::DeadlineElapsed));
                            if let Ok(ready) =
                                self.lifecycle.observe_exact_current_ready_attestation_v2()
                            {
                                monitor = self.start_gateway_monitor_v2(&ready).ok();
                            }
                            if monitor.is_none() {
                                self.foundation
                                    .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::ReadinessLost);
                            }
                            continue;
                        };
                        match port.pause_v3(generation, deadline).await {
                            RuntimeDiscordOrdinaryBarrierPauseOutcomeV3::Applied(reservation) => {
                                let handle_value = next_barrier_handle.checked_add(1);
                                let fields = (
                                    NonZeroU64::new(reservation.coordinator_generation_v3().get()),
                                    NonZeroU64::new(reservation.connection_epoch_v3()),
                                    NonZeroU64::new(reservation.admission_revision_v3()),
                                    NonZeroU64::new(reservation.connected_event_sequence_v3()),
                                    NonZeroU64::new(reservation.pause_sequence_v3()),
                                    handle_value.and_then(NonZeroU64::new),
                                );
                                let (
                                    Some(coordinator_generation),
                                    Some(connection_epoch),
                                    Some(admission_revision),
                                    Some(connected_event_sequence),
                                    Some(pause_sequence),
                                    Some(handle),
                                ) = fields
                                else {
                                    let _ = reply.send(Err(
                                        RuntimeControllerBarrierBridgeErrorV2::Indeterminate,
                                    ));
                                    self.foundation.trip_shutdown_v1(
                                        crate::RuntimeShutdownCauseV1::ReadinessLost,
                                    );
                                    continue;
                                };
                                let paused_at = Utc::now();
                                if paused_at > command.deadline {
                                    let _ = reply.send(Err(
                                        RuntimeControllerBarrierBridgeErrorV2::Indeterminate,
                                    ));
                                    self.foundation.trip_shutdown_v1(
                                        crate::RuntimeShutdownCauseV1::ReadinessLost,
                                    );
                                    continue;
                                }
                                next_barrier_handle = handle.get();
                                paused_barrier = Some(RuntimeServingBarrierAPausedStateV3 {
                                    handle,
                                    command: command.clone(),
                                    reservation,
                                });
                                let _ = reply.send(Ok(
                                    automation_runtime_worker::RuntimeBarrierAPauseObservationV2 {
                                        correlation: command.correlation,
                                        coordinator_generation,
                                        connection_epoch,
                                        admission_revision,
                                        connected_event_sequence,
                                        pause_sequence,
                                        paused_at,
                                        paused: RuntimeControllerBarrierAPausedV2 { handle },
                                    },
                                ));
                            }
                            RuntimeDiscordOrdinaryBarrierPauseOutcomeV3::DefinitelyNotApplied(
                                error,
                            ) => {
                                let _ = reply.send(Err(map_ordinary_barrier_failure_v3(error)));
                                if let Ok(ready) =
                                    self.lifecycle.observe_exact_current_ready_attestation_v2()
                                {
                                    monitor = self.start_gateway_monitor_v2(&ready).ok();
                                }
                                if monitor.is_none() {
                                    self.foundation.trip_shutdown_v1(
                                        crate::RuntimeShutdownCauseV1::ReadinessLost,
                                    );
                                }
                            }
                            RuntimeDiscordOrdinaryBarrierPauseOutcomeV3::Indeterminate(error) => {
                                let _ = reply.send(Err(map_ordinary_barrier_failure_v3(error)));
                                self.foundation
                                    .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::ReadinessLost);
                            }
                        }
                    }
                    RuntimeServingControllerEventV2::ResumeBarrierA { command, reply } => {
                        let matches = paused_barrier.as_ref().is_some_and(|paused| {
                            paused.handle == command.paused.handle
                                && paused.command.correlation == command.correlation
                                && paused.command.deadline == command.deadline
                                && paused.reservation.coordinator_generation_v3().get()
                                    == command.coordinator_generation.get()
                                && paused.reservation.connection_epoch_v3()
                                    == command.connection_epoch.get()
                                && paused.reservation.admission_revision_v3()
                                    == command.pause_admission_revision.get()
                                && paused.reservation.connected_event_sequence_v3()
                                    == command.connected_event_sequence.get()
                                && paused.reservation.pause_sequence_v3()
                                    == command.pause_sequence.get()
                        });
                        if !matches {
                            let _ = reply
                                .send(Err(RuntimeControllerBarrierBridgeErrorV2::StaleAuthority));
                            self.foundation
                                .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::ReadinessLost);
                            continue;
                        }
                        let paused = paused_barrier
                            .take()
                            .expect("validated runtime Barrier A paused state");
                        let port = match self.lifecycle.ordinary_barrier_port_v3() {
                            Ok(port) => port,
                            Err(error) => {
                                let _ = reply.send(Err(map_ordinary_barrier_failure_v3(error)));
                                self.foundation
                                    .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::ReadinessLost);
                                continue;
                            }
                        };
                        let Some(deadline) = runtime_barrier_deadline_v3(command.deadline) else {
                            let _ = reply
                                .send(Err(RuntimeControllerBarrierBridgeErrorV2::DeadlineElapsed));
                            self.foundation
                                .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::ReadinessLost);
                            continue;
                        };
                        let evidence = match port.resume_v3(paused.reservation, deadline).await {
                        RuntimeDiscordOrdinaryBarrierResumeOutcomeV3::Applied(evidence) => evidence,
                        RuntimeDiscordOrdinaryBarrierResumeOutcomeV3::DefinitelyNotApplied {
                            reservation,
                            failure,
                        } => {
                            let exact_reservation =
                                reservation.coordinator_generation_v3().get()
                                    == command.coordinator_generation.get()
                                    && reservation.connection_epoch_v3()
                                        == command.connection_epoch.get()
                                    && reservation.admission_revision_v3()
                                        == command.pause_admission_revision.get()
                                    && reservation.connected_event_sequence_v3()
                                        == command.connected_event_sequence.get()
                                    && reservation.pause_sequence_v3()
                                        == command.pause_sequence.get();
                            let failure = if exact_reservation {
                                map_ordinary_barrier_failure_v3(failure)
                            } else {
                                RuntimeControllerBarrierBridgeErrorV2::StaleAuthority
                            };
                            let _ = reply.send(Err(failure));
                            self.foundation
                                .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::ReadinessLost);
                            continue;
                        }
                        RuntimeDiscordOrdinaryBarrierResumeOutcomeV3::Indeterminate(error) => {
                            let _ = reply.send(Err(map_ordinary_barrier_failure_v3(error)));
                            self.foundation
                                .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::ReadinessLost);
                            continue;
                        }
                    };
                        let resumed_at = Utc::now();
                        let exact = evidence.coordinator_generation_v3().get()
                            == command.coordinator_generation.get()
                            && evidence.connection_epoch_v3() == command.connection_epoch.get()
                            && evidence.admission_revision_v3()
                                == command.pause_admission_revision.get()
                            && evidence.connected_event_sequence_v3()
                                == command.connected_event_sequence.get()
                            && evidence.pause_sequence_v3() == command.pause_sequence.get()
                            && evidence.resume_sequence_v3() > command.pause_sequence.get()
                            && resumed_at >= command.transitioned_at
                            && resumed_at <= command.deadline;
                        if !exact {
                            let _ = reply
                                .send(Err(RuntimeControllerBarrierBridgeErrorV2::Indeterminate));
                            self.foundation
                                .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::ReadinessLost);
                            continue;
                        }
                        let gateway_ready = match self
                            .lifecycle
                            .revalidate_resumed_ordinary_barrier_v3(&evidence)
                            .await
                        {
                            Ok(ready) => ready,
                            Err(_) => {
                                let _ = reply.send(Err(
                                    RuntimeControllerBarrierBridgeErrorV2::Indeterminate,
                                ));
                                self.foundation
                                    .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::ReadinessLost);
                                continue;
                            }
                        };
                        let receipt;
                        match self
                            .refresh_acknowledgement_with_ready_v3(gateway_ready, false)
                            .await
                        {
                            Ok((process, observed_receipt)) => {
                                self = process;
                                receipt = observed_receipt;
                            }
                            Err(error) => {
                                let _ = reply.send(Err(
                                    RuntimeControllerBarrierBridgeErrorV2::Indeterminate,
                                ));
                                return Err(error);
                            }
                        }
                        let completion = match self
                            .lifecycle
                            .authorize_ordinary_barrier_completion_v3(evidence, receipt)
                            .await
                        {
                            Ok(completion) => completion,
                            Err(_) => {
                                let _ = reply.send(Err(
                                    RuntimeControllerBarrierBridgeErrorV2::Indeterminate,
                                ));
                                self.foundation
                                    .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::ReadinessLost);
                                return Err(self
                                    .cleanup_transition_v2(
                                        RuntimeProcessProductionHandoffFailureV2::Gateway,
                                    )
                                    .await);
                            }
                        };
                        let ready = match self.lifecycle.complete_ordinary_barrier_v3(completion) {
                            Ok(ready) => ready,
                            Err(_) => {
                                let _ = reply.send(Err(
                                    RuntimeControllerBarrierBridgeErrorV2::Indeterminate,
                                ));
                                self.foundation
                                    .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::ReadinessLost);
                                return Err(self
                                    .cleanup_transition_v2(
                                        RuntimeProcessProductionHandoffFailureV2::Gateway,
                                    )
                                    .await);
                            }
                        };
                        if let Err(transition) = self.revalidate_v2().await {
                            let _ = reply
                                .send(Err(RuntimeControllerBarrierBridgeErrorV2::Indeterminate));
                            return Err(self.cleanup_transition_v2(transition).await);
                        }
                        monitor = match self.start_gateway_monitor_v2(&ready) {
                            Ok(monitor) => Some(monitor),
                            Err(transition) => {
                                let _ = reply.send(Err(
                                    RuntimeControllerBarrierBridgeErrorV2::Indeterminate,
                                ));
                                return Err(self.cleanup_transition_v2(transition).await);
                            }
                        };
                        let Some(resume_sequence) = NonZeroU64::new(ready.resume_sequence.get())
                        else {
                            unreachable!()
                        };
                        let _ = reply.send(Ok(
                        automation_runtime_worker::RuntimeBarrierAResumeObservationV2 {
                            correlation: command.correlation,
                            coordinator_generation: command.coordinator_generation,
                            connection_epoch: command.connection_epoch,
                            admission_revision: command.pause_admission_revision,
                            connected_event_sequence: command.connected_event_sequence,
                            pause_sequence: command.pause_sequence,
                            resume_sequence,
                            admission:
                                automation_runtime_worker::RuntimeAdmissionDispositionV2::Closed,
                            resumed_at,
                            resumed: RuntimeControllerBarrierAResumedV2 {
                                handle: paused.handle,
                            },
                        },
                    ));
                    }
                    RuntimeServingControllerEventV2::Certification { handoff, reply } => {
                        if paused_barrier.is_some() || monitor.is_none() {
                            let _ = reply.send(handoff.reject_v2(
                                crate::runtime_controller::RuntimeControllerCertificationHandoffRejectionV2::Unavailable,
                            ));
                            continue;
                        }
                        let gateway_monitor =
                            monitor.take().expect("validated serving gateway monitor");
                        match super::serving_certification::handle_certification_v2(
                            self,
                            gateway_monitor,
                            handoff,
                        )
                        .await
                        {
                            super::serving_certification::RuntimeServingCertificationHandleOutcomeV2::Completed {
                                process,
                                gateway_monitor,
                                reply: accepted,
                            } => {
                                self = *process;
                                monitor = Some(gateway_monitor);
                                if reply.send(accepted).is_err() {
                                    self.foundation.trip_shutdown_v1(
                                        crate::RuntimeShutdownCauseV1::SupervisorFailure,
                                    );
                                }
                            }
                            super::serving_certification::RuntimeServingCertificationHandleOutcomeV2::Rejected {
                                process,
                                gateway_monitor,
                                reply: rejected,
                            } => {
                                self = *process;
                                monitor = Some(gateway_monitor);
                                let _ = reply.send(rejected);
                            }
                            super::serving_certification::RuntimeServingCertificationHandleOutcomeV2::Terminal(
                                error,
                            ) => {
                                return Err(error);
                            }
                        }
                    }
                    RuntimeServingControllerEventV2::Terminal => {
                        break self
                            .foundation
                            .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::SupervisorFailure);
                    }
                },
                RuntimeServingOpenLoopEventV2::RefreshAcknowledgement => {
                    match self.refresh_acknowledgement_v2().await {
                        Ok(process) => self = process,
                        Err(error) => {
                            if let Some(active) = monitor.take() {
                                active.stop_v2().await;
                            }
                            return finish_acknowledgement_refresh_error_v2(error);
                        }
                    }
                }
            }
        };
        if let Some(active) = monitor.take() {
            active.stop_v2().await;
        }
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
            mut certification_monitors,
            process_generation,
        } = self;
        let certification_deadline = foundation
            .shutdown_observer_v1()
            .observed()
            .map(|observation| foundation.effective_shutdown_deadline_v1(observation))
            .unwrap_or_else(Instant::now);
        let _certification_outcomes = certification_monitors
            .stop_all_until_v2(certification_deadline)
            .await;
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

    pub(super) async fn cleanup_transition_v2(
        self,
        transition: RuntimeProcessProductionHandoffFailureV2,
    ) -> RuntimeProcessProductionHandoffErrorV2 {
        let cleanup = self.shutdown().await;
        finish_production_handoff_transition_v2(transition, cleanup)
    }
}

pub(super) async fn stop_runtime_controller_before_cleanup_v2(
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

fn runtime_barrier_deadline_v3(deadline: DateTime<Utc>) -> Option<Instant> {
    let remaining = deadline.signed_duration_since(Utc::now()).to_std().ok()?;
    if remaining.is_zero() {
        return None;
    }
    Instant::now().checked_add(remaining)
}

fn map_ordinary_barrier_failure_v3(
    error: RuntimeDiscordOrdinaryBarrierFailureV3,
) -> RuntimeControllerBarrierBridgeErrorV2 {
    match error {
        RuntimeDiscordOrdinaryBarrierFailureV3::CommandUnavailable => {
            RuntimeControllerBarrierBridgeErrorV2::Unavailable
        }
        RuntimeDiscordOrdinaryBarrierFailureV3::DeadlineElapsed => {
            RuntimeControllerBarrierBridgeErrorV2::DeadlineElapsed
        }
        RuntimeDiscordOrdinaryBarrierFailureV3::StaleAuthority => {
            RuntimeControllerBarrierBridgeErrorV2::StaleAuthority
        }
        RuntimeDiscordOrdinaryBarrierFailureV3::Indeterminate => {
            RuntimeControllerBarrierBridgeErrorV2::Indeterminate
        }
    }
}

fn finish_acknowledgement_refresh_error_v2(
    error: RuntimeProcessProductionHandoffErrorV2,
) -> Result<(), RuntimeProcessProductionHandoffErrorV2> {
    match error {
        RuntimeProcessProductionHandoffErrorV2::Transition(
            RuntimeProcessProductionHandoffFailureV2::ProcessShutdown,
        ) => Ok(()),
        error => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acknowledgement_refresh_treats_only_cleaned_process_shutdown_as_success() {
        let clean = finish_acknowledgement_refresh_error_v2(
            RuntimeProcessProductionHandoffErrorV2::Transition(
                RuntimeProcessProductionHandoffFailureV2::ProcessShutdown,
            ),
        );
        assert!(clean.is_ok());

        let violation = finish_acknowledgement_refresh_error_v2(
            RuntimeProcessProductionHandoffErrorV2::Transition(
                RuntimeProcessProductionHandoffFailureV2::ProtocolViolation,
            ),
        );
        assert!(matches!(
            violation,
            Err(RuntimeProcessProductionHandoffErrorV2::Transition(
                RuntimeProcessProductionHandoffFailureV2::ProtocolViolation
            ))
        ));
    }
}
