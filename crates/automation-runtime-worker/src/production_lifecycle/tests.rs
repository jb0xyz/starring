use std::cell::RefCell;
use std::num::NonZeroU64;

use automation_runtime_controller::{
    GatewayShardIdV1, RuntimeBuildRevisionV1, RuntimeGatewayAdmissionSequenceV2,
    RuntimeGatewayOwnerLeaseIdV1, RuntimeGatewayOwnerLeaseReceiptV1, RuntimeGatewayReadyKindV2,
    RuntimeIngressOpenAcknowledgementInputV2, RuntimeIngressOpenAcknowledgementLeaseDurationV2,
    RuntimeIngressOpenAcknowledgementReceiptInputV2, RuntimeIngressOpenAcknowledgementReceiptV2,
    RuntimeIngressOpenAcknowledgementV2, RuntimeObservedIngressOpenAcknowledgementV2,
    RuntimePublishIngressOpenAcknowledgementInputV2,
    RuntimePublishIngressOpenAcknowledgementOutcomeV2, RuntimePublishIngressOpenAcknowledgementV2,
    RuntimeRecoveryIdV2, RuntimeStartupRecoveryObservationReceiptV2, RuntimeStartupRecoveryStateV2,
    RuntimeStartupServingStateV2, RuntimeWriterFenceGenerationV1,
};
use automation_runtime_convergence::ProcessInstanceId;
use chrono::{DateTime, Utc};

use super::*;
use crate::{
    accept_runtime_registry_recovery_empty_observation_v2,
    classify_ingress_open_acknowledgement_outcome_v2,
    classify_unknown_ingress_open_acknowledgement_v2, RuntimeAcceptedIngressOpenAcknowledgementV2,
    RuntimeAcceptedStartupRecoveryOutcomeV2, RuntimeCapabilityReadinessKindV2,
    RuntimeCapabilityReadinessReceiptV2, RuntimeCapabilityReadinessSetV2,
    RuntimeClosedDrainRecoveryPermitV2, RuntimeClosedRecoveryInputV2,
    RuntimeClosedRecoveryRegistryEvidenceV2, RuntimeGatewayClosedLifecycleV2,
    RuntimeGatewayInvalidationCauseV2, RuntimeIngressOpenAcknowledgementAuthorizationErrorV2,
    RuntimeIngressOpenAcknowledgementProtocolViolationV2,
    RuntimeIngressOpenAcknowledgementResolutionV2, RuntimePausedGatewayObservationV2,
    RuntimePausedGatewaySequenceV2, RuntimeRegistryGlobalObservationSequenceV2,
    RuntimeRegistryRecoveryObservationInputV2, RuntimeStartupRecoveryFixedPointProofV2,
};

fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn at(second: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(second, 0).unwrap()
}

fn sequence(value: u64) -> RuntimeGatewayAdmissionSequenceV2 {
    RuntimeGatewayAdmissionSequenceV2::new(non_zero(value))
}

fn process() -> ProcessInstanceId {
    ProcessInstanceId::parse("runtime-process:1").unwrap()
}

fn readiness(checked_at: i64) -> RuntimeCapabilityReadinessSetV2 {
    let receipt = |kind, role, offset| {
        RuntimeCapabilityReadinessReceiptV2::new(
            kind,
            "01234567-89ab-cdef-8123-456789abcdef",
            "starring",
            role,
            at(checked_at + offset),
        )
        .unwrap()
    };
    RuntimeCapabilityReadinessSetV2::new(
        receipt(RuntimeCapabilityReadinessKindV2::Convergence, "role_a", 0),
        receipt(RuntimeCapabilityReadinessKindV2::ExactTarget, "role_b", 1),
        receipt(RuntimeCapabilityReadinessKindV2::Panel, "role_c", 2),
        receipt(RuntimeCapabilityReadinessKindV2::Serving, "role_d", 3),
        receipt(RuntimeCapabilityReadinessKindV2::Interaction, "role_e", 4),
    )
    .unwrap()
}

fn owner(database_now: i64) -> RuntimeGatewayOwnerLeaseReceiptV1 {
    RuntimeGatewayOwnerLeaseReceiptV1 {
        lease_id: RuntimeGatewayOwnerLeaseIdV1 {
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
            process_instance_id: process(),
            lease_epoch: non_zero(7),
            expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
        },
        owner_revision: non_zero(8),
        database_now: at(database_now),
        expires_at: at(1_000),
    }
}

fn empty_registry(observation_sequence: u64) -> crate::RuntimeRegistryRecoveryEmptyObservationV2 {
    accept_runtime_registry_recovery_empty_observation_v2(
        process(),
        RuntimeRegistryRecoveryObservationInputV2 {
            observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(non_zero(
                observation_sequence,
            )),
            retained_slot_count: 0,
            retained_empty_tombstone_count: 0,
            staged_route_count: 0,
            serving_route_count: 0,
            draining_route_count: 0,
            sealed_slot_count: 0,
            active_interaction_count: 0,
            failed_closed_slot_count: 0,
            registry_failed_closed: false,
        },
    )
    .unwrap()
}

fn fixed_point_parts() -> (
    RuntimeGatewayClosedLifecycleV2,
    RuntimeClosedDrainRecoveryPermitV2,
    RuntimeStartupRecoveryFixedPointProofV2,
) {
    let mut lifecycle = RuntimeGatewayClosedLifecycleV2::starting();
    let emergency_generation = lifecycle.snapshot().generation();
    let paused = RuntimePausedGatewayObservationV2::new(
        emergency_generation,
        process(),
        non_zero(2),
        RuntimeGatewayReadyKindV2::Ready,
        non_zero(3),
        RuntimePausedGatewaySequenceV2::new(sequence(5), sequence(4), None).unwrap(),
    );
    let (_, mut permit) = lifecycle
        .begin_recovery(
            emergency_generation,
            RuntimeClosedRecoveryInputV2::new(
                RuntimeRecoveryIdV2::parse("0123456789abcdef0123456789abcdef").unwrap(),
                owner(100),
                readiness(100),
                paused,
                RuntimeClosedRecoveryRegistryEvidenceV2::Empty(empty_registry(6)),
            ),
        )
        .unwrap();
    let iteration = lifecycle
        .refresh_recovery_readiness(&mut permit, readiness(200))
        .unwrap();
    let observation = lifecycle
        .begin_startup_recovery_observation(&mut permit, iteration)
        .unwrap();
    let correlation = observation.request().correlation.clone();
    let completed = observation.complete(RuntimeStartupRecoveryObservationReceiptV2 {
        correlation,
        owner_receipt: owner(210),
        state: RuntimeStartupRecoveryStateV2 {
            serving: RuntimeStartupServingStateV2::Empty,
            recoverable_awaiting_certification_count: 0,
            suspended_local_effect_count: 0,
            pending_runtime_drain_intent_count: 0,
            acknowledged_product_handoff_count: 0,
        },
    });
    let RuntimeAcceptedStartupRecoveryOutcomeV2::FixedPoint(proof) = lifecycle
        .complete_startup_recovery_observation(&mut permit, completed)
        .unwrap()
    else {
        panic!("expected fixed point")
    };
    (lifecycle, permit, proof)
}

fn fixed_point() -> RuntimeStartupRecoveryFixedPointProcessV2 {
    let (lifecycle, permit, proof) = fixed_point_parts();
    lifecycle
        .into_production_fixed_point(permit, proof)
        .unwrap()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TestPortError {
    Unavailable,
}

struct HandoffPort {
    fail: bool,
    settled: bool,
}

impl RuntimeProductionHandoffObservationPortV2 for HandoffPort {
    type Error = TestPortError;

    fn observe_production_handoff(
        &self,
        request: &RuntimeProductionHandoffRequestV2,
    ) -> Result<RuntimeProductionHandoffObservationV2, Self::Error> {
        if self.fail {
            return Err(TestPortError::Unavailable);
        }
        Ok(RuntimeProductionHandoffObservationV2::new(
            RuntimeProductionHandoffObservationInputV2 {
                coordinator_generation: request.coordinator_generation(),
                recovery_id: request.recovery_id().clone(),
                recovery_authority_revision: request.recovery_authority_revision(),
                owner_lease_id: request.owner_lease_id().clone(),
                owner_revision: request.owner_revision(),
                owner_expires_at: request.owner_expires_at(),
                process_instance_id: request.process_instance_id().clone(),
                connection_epoch: request.connection_epoch(),
                paused_admission_revision: request.paused_admission_revision(),
                connected_event_sequence: request.connected_event_sequence(),
                pause_sequence: request.pause_sequence(),
                registry_observation_sequence: request.registry_observation_sequence(),
                finalizer_generation: RuntimeMutationFinalizerGenerationV1::new(non_zero(9))
                    .unwrap(),
                startup_intake_sealed: true,
                startup_jobs_settled: self.settled,
                supervisors_started: true,
            },
        ))
    }
}

fn handoff() -> RuntimeProductionHandoffProcessV2 {
    fixed_point()
        .begin_production_handoff(&HandoffPort {
            fail: false,
            settled: true,
        })
        .unwrap()
}

#[derive(Clone, Copy)]
enum ResumeDrift {
    None,
    Port,
    AdmissionRevision,
    ConnectionEpoch,
    NotExplicitlyResumed,
    WriterFenceClosed,
}

struct ResumePort {
    drift: ResumeDrift,
}

impl RuntimeRecoveryResumePortV2 for ResumePort {
    type Error = TestPortError;

    fn resume_or_observe_recovery(
        &self,
        permit: &RuntimeRecoveryResumePermitV2,
    ) -> Result<RuntimeRecoveryResumeObservationV2, Self::Error> {
        if matches!(self.drift, ResumeDrift::Port) {
            return Err(TestPortError::Unavailable);
        }
        let paused_admission_revision = if matches!(self.drift, ResumeDrift::AdmissionRevision) {
            non_zero(permit.paused_admission_revision().get() + 1)
        } else {
            permit.paused_admission_revision()
        };
        let connection_epoch = if matches!(self.drift, ResumeDrift::ConnectionEpoch) {
            non_zero(permit.connection_epoch().get() + 1)
        } else {
            permit.connection_epoch()
        };
        let resume_sequence = if matches!(self.drift, ResumeDrift::NotExplicitlyResumed) {
            permit.connected_event_sequence()
        } else {
            sequence(6)
        };
        Ok(RuntimeRecoveryResumeObservationV2::new(
            RuntimeRecoveryResumeObservationInputV2 {
                coordinator_generation: permit.coordinator_generation(),
                recovery_id: permit.recovery_id().clone(),
                recovery_authority_revision: permit.recovery_authority_revision(),
                process_instance_id: permit.process_instance_id().clone(),
                connection_epoch: permit.connection_epoch(),
                paused_admission_revision,
                connected_event_sequence: permit.connected_event_sequence(),
                pause_sequence: permit.pause_sequence(),
                owner_receipt: owner(250),
                readiness: readiness(200),
                registry_observation_sequence: permit.registry_observation_sequence(),
                finalizer_generation: permit.finalizer_generation(),
                writer_fence_generation: RuntimeWriterFenceGenerationV1::new(non_zero(7)),
                writer_fence_open: !matches!(self.drift, ResumeDrift::WriterFenceClosed),
                maintenance_gate_generation: RuntimeMaintenanceGateGenerationV2::new(non_zero(11))
                    .unwrap(),
                maintenance_gate_closed: true,
                gateway_ready: automation_runtime_controller::RuntimeGatewayReadyAttestationV2 {
                    process_instance_id: permit.process_instance_id().clone(),
                    connection_epoch,
                    kind: permit.ready_kind(),
                    admission_revision: permit.paused_admission_revision(),
                    connected_event_sequence: permit.connected_event_sequence(),
                    resume_sequence,
                },
            },
        ))
    }
}

fn admission() -> RuntimeAdmissionAcknowledgingProcessV2 {
    handoff()
        .resume_recovery(&ResumePort {
            drift: ResumeDrift::None,
        })
        .unwrap()
}

#[derive(Clone, Copy)]
enum OpenDrift {
    None,
    Port,
    Reconnect,
    GateNotAdvanced,
    ExpiredAcknowledgement,
    SupervisorStopped,
}

struct OpenPort {
    drift: OpenDrift,
    acknowledgement: RefCell<Option<RuntimeAcceptedIngressOpenAcknowledgementV2>>,
}

impl RuntimeOpenProductionObservationPortV2 for OpenPort {
    type Error = TestPortError;

    fn observe_open_production(
        &self,
        request: &RuntimeOpenProductionRequestV2,
    ) -> Result<RuntimeOpenProductionObservationV2, Self::Error> {
        if matches!(self.drift, OpenDrift::Port) {
            return Err(TestPortError::Unavailable);
        }
        let mut ready = request.gateway_ready().clone();
        if matches!(self.drift, OpenDrift::Reconnect) {
            ready.connection_epoch = non_zero(ready.connection_epoch.get() + 1);
        }
        let observed_database_second = if matches!(self.drift, OpenDrift::ExpiredAcknowledgement) {
            256
        } else {
            252
        };
        let observed_database_now = at(observed_database_second);
        let open_gate_generation = if matches!(self.drift, OpenDrift::GateNotAdvanced) {
            request.closed_maintenance_gate_generation()
        } else {
            RuntimeMaintenanceGateGenerationV2::new(non_zero(
                request.closed_maintenance_gate_generation().get() + 1,
            ))
            .unwrap()
        };
        let acknowledgement = RuntimeIngressOpenAcknowledgementObservationV2::from_accepted(
            self.acknowledgement
                .borrow_mut()
                .take()
                .expect("test open acknowledgement must be consumed exactly once"),
        );
        let mut current_owner = owner(observed_database_second);
        current_owner.expires_at = request.owner_expires_at();
        Ok(RuntimeOpenProductionObservationV2::new(
            RuntimeOpenProductionObservationInputV2 {
                coordinator_generation: request.coordinator_generation(),
                writer_fence_generation: request.writer_fence_generation(),
                writer_fence_open: true,
                maintenance_gate_generation: open_gate_generation,
                maintenance_gate_open: true,
                owner_receipt: current_owner,
                readiness: readiness(200),
                gateway_ready: ready,
                registry_empty: empty_registry(request.registry_observation_sequence().get()),
                finalizer_generation: request.finalizer_generation(),
                finalizer_accepting: true,
                supervisors_running: !matches!(self.drift, OpenDrift::SupervisorStopped),
                observed_database_now,
                ingress_acknowledgement: acknowledgement,
            },
        ))
    }
}

fn open() -> RuntimeEmptyOpenProcessV2 {
    let state = admission();
    let port = open_port(&state, OpenDrift::None);
    state.observe_open_production(&port).unwrap()
}

fn acknowledgement_for_authorization(
    authorization: &crate::RuntimeAuthorizedIngressOpenAcknowledgementV2,
    revision: u64,
) -> RuntimeIngressOpenAcknowledgementV2 {
    acknowledgement_for_authorization_at(authorization, revision, 251, 256)
}

fn acknowledgement_for_authorization_at(
    authorization: &crate::RuntimeAuthorizedIngressOpenAcknowledgementV2,
    revision: u64,
    acknowledged_at: i64,
    expires_at: i64,
) -> RuntimeIngressOpenAcknowledgementV2 {
    let request = authorization.request();
    RuntimeIngressOpenAcknowledgementV2::new(RuntimeIngressOpenAcknowledgementInputV2 {
        fence_generation: request.fence_generation(),
        maintenance_gate_generation: request.maintenance_gate_generation(),
        gateway_owner_lease_id: request.owner_receipt().lease_id.clone(),
        observed_owner_revision: request.owner_receipt().owner_revision,
        process_instance_id: request.gateway_ready().process_instance_id.clone(),
        connection_epoch: request.gateway_ready().connection_epoch,
        admission_revision: request.gateway_ready().admission_revision,
        connected_event_sequence: request.gateway_ready().connected_event_sequence,
        resume_sequence: request.gateway_ready().resume_sequence,
        acknowledgement_revision: non_zero(revision),
        acknowledged_at: at(acknowledged_at),
        expires_at: at(expires_at),
    })
    .unwrap()
}

fn receipt_for_authorization(
    authorization: &crate::RuntimeAuthorizedIngressOpenAcknowledgementV2,
    revision: u64,
) -> RuntimeIngressOpenAcknowledgementReceiptV2 {
    RuntimeIngressOpenAcknowledgementReceiptV2::new(
        RuntimeIngressOpenAcknowledgementReceiptInputV2 {
            source_acknowledgement_revision: authorization
                .request()
                .source_acknowledgement_revision(),
            request_digest: authorization.request().request_digest(),
            acknowledgement: acknowledgement_for_authorization(authorization, revision),
            observed_database_now: at(252),
        },
    )
    .unwrap()
}

fn accepted_for_authorization(
    authorization: &crate::RuntimeAuthorizedIngressOpenAcknowledgementV2,
    revision: u64,
) -> RuntimeAcceptedIngressOpenAcknowledgementV2 {
    let resolution = classify_ingress_open_acknowledgement_outcome_v2(
        authorization,
        RuntimePublishIngressOpenAcknowledgementOutcomeV2::Applied(receipt_for_authorization(
            authorization,
            revision,
        )),
    );
    let RuntimeIngressOpenAcknowledgementResolutionV2::AppliedExact(accepted) = resolution else {
        panic!("test acknowledgement must be accepted")
    };
    accepted
}

fn open_port(state: &RuntimeAdmissionAcknowledgingProcessV2, drift: OpenDrift) -> OpenPort {
    let lease = RuntimeIngressOpenAcknowledgementLeaseDurationV2::from_milliseconds(5_000).unwrap();
    let open_gate = RuntimeMaintenanceGateGenerationV2::new(non_zero(
        state.closed_maintenance_gate_generation().get() + 1,
    ))
    .unwrap();
    let initial = state
        .authorize_ingress_open_acknowledgement(open_gate, None, lease)
        .unwrap();
    let predecessor = acknowledgement_for_authorization(&initial, 1);
    let renewal = state
        .authorize_ingress_open_acknowledgement(open_gate, Some(&predecessor), lease)
        .unwrap();
    OpenPort {
        drift,
        acknowledgement: RefCell::new(Some(accepted_for_authorization(&renewal, 2))),
    }
}

#[test]
fn admission_acknowledgement_authority_requires_the_exact_open_gate_and_predecessor() {
    let state = admission();
    let lease = RuntimeIngressOpenAcknowledgementLeaseDurationV2::from_milliseconds(5_000).unwrap();
    let wrong_gate = RuntimeMaintenanceGateGenerationV2::new(non_zero(11)).unwrap();
    assert!(matches!(
        state.authorize_ingress_open_acknowledgement(wrong_gate, None, lease),
        Err(RuntimeIngressOpenAcknowledgementAuthorizationErrorV2::OpenGateMismatch)
    ));

    let open_gate = RuntimeMaintenanceGateGenerationV2::new(non_zero(12)).unwrap();
    let initial = state
        .authorize_ingress_open_acknowledgement(open_gate, None, lease)
        .unwrap();
    let predecessor = acknowledgement_for_authorization(&initial, 1);
    let renewal = state
        .authorize_ingress_open_acknowledgement(open_gate, Some(&predecessor), lease)
        .unwrap();

    assert_eq!(
        renewal.request().source_acknowledgement_revision(),
        Some(non_zero(1))
    );
    assert_eq!(
        format!("{renewal:?}"),
        "RuntimeAuthorizedIngressOpenAcknowledgementV2(<redacted>)"
    );

    let mut mismatched_input = RuntimeIngressOpenAcknowledgementInputV2 {
        fence_generation: renewal.request().fence_generation(),
        maintenance_gate_generation: renewal.request().maintenance_gate_generation(),
        gateway_owner_lease_id: renewal.request().owner_receipt().lease_id.clone(),
        observed_owner_revision: renewal.request().owner_receipt().owner_revision,
        process_instance_id: renewal
            .request()
            .gateway_ready()
            .process_instance_id
            .clone(),
        connection_epoch: renewal.request().gateway_ready().connection_epoch,
        admission_revision: renewal.request().gateway_ready().admission_revision,
        connected_event_sequence: renewal.request().gateway_ready().connected_event_sequence,
        resume_sequence: renewal.request().gateway_ready().resume_sequence,
        acknowledgement_revision: non_zero(1),
        acknowledged_at: at(251),
        expires_at: at(256),
    };
    mismatched_input.admission_revision = non_zero(
        mismatched_input
            .admission_revision
            .get()
            .checked_add(1)
            .unwrap(),
    );
    let mismatched = RuntimeIngressOpenAcknowledgementV2::new(mismatched_input).unwrap();
    assert!(matches!(
        state.authorize_ingress_open_acknowledgement(open_gate, Some(&mismatched), lease),
        Err(RuntimeIngressOpenAcknowledgementAuthorizationErrorV2::PreviousAcknowledgementMismatch)
    ));
}

#[test]
fn acknowledgement_outcome_accepts_exact_receipts_and_unknown_recovery_is_bounded() {
    let state = admission();
    let lease = RuntimeIngressOpenAcknowledgementLeaseDurationV2::from_milliseconds(5_000).unwrap();
    let open_gate = RuntimeMaintenanceGateGenerationV2::new(non_zero(12)).unwrap();
    let initial = state
        .authorize_ingress_open_acknowledgement(open_gate, None, lease)
        .unwrap();
    let receipt = receipt_for_authorization(&initial, 1);

    assert!(matches!(
        classify_ingress_open_acknowledgement_outcome_v2(
            &initial,
            RuntimePublishIngressOpenAcknowledgementOutcomeV2::Applied(receipt.clone()),
        ),
        RuntimeIngressOpenAcknowledgementResolutionV2::AppliedExact(_)
    ));
    assert!(matches!(
        classify_unknown_ingress_open_acknowledgement_v2(
            &initial,
            RuntimeObservedIngressOpenAcknowledgementV2::present(receipt),
        ),
        RuntimeIngressOpenAcknowledgementResolutionV2::AdoptExact(_)
    ));
    assert!(matches!(
        classify_unknown_ingress_open_acknowledgement_v2(
            &initial,
            RuntimeObservedIngressOpenAcknowledgementV2::missing(
                initial
                    .request()
                    .owner_receipt()
                    .lease_id
                    .gateway_shard_id
                    .clone(),
                at(252),
            )
            .unwrap(),
        ),
        RuntimeIngressOpenAcknowledgementResolutionV2::ReplaySameRequest
    ));

    let predecessor = acknowledgement_for_authorization(&initial, 1);
    let renewal = state
        .authorize_ingress_open_acknowledgement(open_gate, Some(&predecessor), lease)
        .unwrap();
    assert!(matches!(
        classify_unknown_ingress_open_acknowledgement_v2(
            &renewal,
            RuntimeObservedIngressOpenAcknowledgementV2::present(receipt_for_authorization(
                &initial, 1,
            )),
        ),
        RuntimeIngressOpenAcknowledgementResolutionV2::ReplaySameRequest
    ));
    assert!(matches!(
        classify_unknown_ingress_open_acknowledgement_v2(
            &renewal,
            RuntimeObservedIngressOpenAcknowledgementV2::missing(
                renewal
                    .request()
                    .owner_receipt()
                    .lease_id
                    .gateway_shard_id
                    .clone(),
                at(252),
            )
            .unwrap(),
        ),
        RuntimeIngressOpenAcknowledgementResolutionV2::Stale
    ));

    let divergent = RuntimeIngressOpenAcknowledgementReceiptV2::new(
        RuntimeIngressOpenAcknowledgementReceiptInputV2 {
            source_acknowledgement_revision: Some(non_zero(1)),
            request_digest:
                automation_runtime_controller::RuntimeIngressOpenAcknowledgementRequestDigestV2::from_bytes(
                    [9; 32],
                ),
            acknowledgement: acknowledgement_for_authorization(&renewal, 2),
            observed_database_now: at(252),
        },
    )
    .unwrap();
    assert!(matches!(
        classify_unknown_ingress_open_acknowledgement_v2(
            &renewal,
            RuntimeObservedIngressOpenAcknowledgementV2::present(divergent),
        ),
        RuntimeIngressOpenAcknowledgementResolutionV2::Divergent
    ));

    let expired = RuntimeIngressOpenAcknowledgementReceiptV2::new(
        RuntimeIngressOpenAcknowledgementReceiptInputV2 {
            source_acknowledgement_revision: None,
            request_digest: initial.request().request_digest(),
            acknowledgement: acknowledgement_for_authorization(&initial, 1),
            observed_database_now: at(256),
        },
    )
    .unwrap();
    assert!(matches!(
        classify_unknown_ingress_open_acknowledgement_v2(
            &initial,
            RuntimeObservedIngressOpenAcknowledgementV2::present(expired),
        ),
        RuntimeIngressOpenAcknowledgementResolutionV2::Stale
    ));
}

#[test]
fn acknowledgement_classifier_blocks_raw_timing_bypasses_before_open_capability() {
    let state = admission();
    let lease = RuntimeIngressOpenAcknowledgementLeaseDurationV2::from_milliseconds(5_000).unwrap();
    let open_gate = RuntimeMaintenanceGateGenerationV2::new(non_zero(12)).unwrap();
    let authorization = state
        .authorize_ingress_open_acknowledgement(open_gate, None, lease)
        .unwrap();

    for (acknowledged_at, expires_at, expected) in [
        (
            249,
            254,
            RuntimeIngressOpenAcknowledgementProtocolViolationV2::AcknowledgementBeforeOwnerObservation,
        ),
        (
            251,
            257,
            RuntimeIngressOpenAcknowledgementProtocolViolationV2::AcknowledgementLeaseMismatch,
        ),
    ] {
        let receipt = RuntimeIngressOpenAcknowledgementReceiptV2::new(
            RuntimeIngressOpenAcknowledgementReceiptInputV2 {
                source_acknowledgement_revision: None,
                request_digest: authorization.request().request_digest(),
                acknowledgement: acknowledgement_for_authorization_at(
                    &authorization,
                    1,
                    acknowledged_at,
                    expires_at,
                ),
                observed_database_now: at(252),
            },
        )
        .unwrap();
        assert!(matches!(
            classify_ingress_open_acknowledgement_outcome_v2(
                &authorization,
                RuntimePublishIngressOpenAcknowledgementOutcomeV2::Applied(receipt),
            ),
            RuntimeIngressOpenAcknowledgementResolutionV2::ProtocolViolation(error)
                if error == expected
        ));
    }
}

#[test]
fn open_rejects_an_accepted_acknowledgement_from_a_different_gateway_snapshot() {
    let state = admission();
    let mut gateway_ready = state.gateway_ready().clone();
    gateway_ready.kind = RuntimeGatewayReadyKindV2::Resumed;
    let authorization = crate::RuntimeAuthorizedIngressOpenAcknowledgementV2::from_request(
        RuntimePublishIngressOpenAcknowledgementV2::new(
            RuntimePublishIngressOpenAcknowledgementInputV2 {
                source_acknowledgement_revision: None,
                fence_generation: state.writer_fence_generation(),
                maintenance_gate_generation: non_zero(
                    state.closed_maintenance_gate_generation().get() + 1,
                ),
                owner_receipt: owner(250),
                gateway_ready,
                lease_for: RuntimeIngressOpenAcknowledgementLeaseDurationV2::from_milliseconds(
                    5_000,
                )
                .unwrap(),
            },
        )
        .unwrap(),
    );
    let port = OpenPort {
        drift: OpenDrift::None,
        acknowledgement: RefCell::new(Some(accepted_for_authorization(&authorization, 1))),
    };

    let failure = state.observe_open_production(&port).unwrap_err();
    assert_eq!(
        failure.contract_error(),
        Some(RuntimeProductionLifecycleErrorV2::IngressAcknowledgementMismatch)
    );
}

#[test]
fn fixed_point_to_open_production_to_shutdown_is_linear_and_exact() {
    let fixed_point = fixed_point();
    assert_eq!(
        fixed_point.stage(),
        RuntimeProductionLifecycleStageV2::FixedPoint
    );
    assert_eq!(fixed_point.coordinator_generation().get(), 2);
    assert_eq!(fixed_point.acknowledged_product_handoff_count(), 0);

    let handoff = fixed_point
        .begin_production_handoff(&HandoffPort {
            fail: false,
            settled: true,
        })
        .unwrap();
    assert_eq!(
        handoff.stage(),
        RuntimeProductionLifecycleStageV2::ProductionHandoff
    );
    assert_eq!(handoff.finalizer_generation().get(), 9);

    let admission = handoff
        .resume_recovery(&ResumePort {
            drift: ResumeDrift::None,
        })
        .unwrap();
    assert_eq!(
        admission.stage(),
        RuntimeProductionLifecycleStageV2::AdmissionAcknowledging
    );
    assert_eq!(admission.coordinator_generation().get(), 3);
    assert!(admission.gateway_ready().was_explicitly_resumed());

    let port = open_port(&admission, OpenDrift::None);
    let open = admission.observe_open_production(&port).unwrap();
    assert_eq!(
        open.stage(),
        RuntimeProductionLifecycleStageV2::OpenProduction
    );
    assert_eq!(
        open.epoch().registry_empty().observation_sequence().get(),
        6
    );
    assert_eq!(
        open.epoch()
            .ingress_acknowledgement()
            .acknowledgement_revision()
            .get(),
        2
    );
    assert_eq!(
        open.epoch()
            .ingress_acknowledgement()
            .maintenance_gate_generation()
            .get(),
        12
    );

    let generation = open.coordinator_generation();
    let shutdown = open
        .begin_shutdown(generation, RuntimeShutdownCauseV2::Explicit)
        .unwrap();
    assert_eq!(
        shutdown.stage(),
        RuntimeProductionLifecycleStageV2::Shutdown
    );
    assert_eq!(
        shutdown.source_stage(),
        RuntimeProductionLifecycleStageV2::OpenProduction
    );
    assert_eq!(shutdown.coordinator_generation().get(), 4);
    assert_eq!(shutdown.cause(), RuntimeShutdownCauseV2::Explicit);
}

#[test]
fn fixed_point_and_port_failures_return_all_unconsumed_authority() {
    let (mut lifecycle, permit, proof) = fixed_point_parts();
    let generation = permit.coordinator_generation();
    lifecycle
        .invalidate(
            generation,
            RuntimeGatewayInvalidationCauseV2::ProtocolViolation,
        )
        .unwrap();
    let failure = lifecycle
        .into_production_fixed_point(permit, proof)
        .unwrap_err();
    assert_eq!(
        failure.error(),
        RuntimeProductionLifecycleErrorV2::FixedPoint(
            RuntimeGatewayClosedTransitionErrorV2::StaleRecoveryPermit
        )
    );
    let _ = failure.into_parts();

    let failure = fixed_point()
        .begin_production_handoff(&HandoffPort {
            fail: true,
            settled: true,
        })
        .unwrap_err();
    assert_eq!(failure.port_error(), Some(&TestPortError::Unavailable));
    let fixed_point = failure.into_state();
    assert_eq!(
        fixed_point.stage(),
        RuntimeProductionLifecycleStageV2::FixedPoint
    );

    let failure = fixed_point
        .begin_production_handoff(&HandoffPort {
            fail: false,
            settled: false,
        })
        .unwrap_err();
    assert_eq!(
        failure.contract_error(),
        Some(RuntimeProductionLifecycleErrorV2::StartupJobsUnsettled)
    );
    assert_eq!(
        failure.into_state().stage(),
        RuntimeProductionLifecycleStageV2::FixedPoint
    );
}

#[test]
fn stale_pause_token_and_reconnect_evidence_never_enter_admission() {
    let failure = handoff()
        .resume_recovery(&ResumePort {
            drift: ResumeDrift::Port,
        })
        .unwrap_err();
    assert_eq!(failure.port_error(), Some(&TestPortError::Unavailable));
    let handoff_state = failure.into_state();

    let failure = handoff_state
        .resume_recovery(&ResumePort {
            drift: ResumeDrift::AdmissionRevision,
        })
        .unwrap_err();
    assert_eq!(
        failure.contract_error(),
        Some(RuntimeProductionLifecycleErrorV2::StaleAdmissionRevision)
    );
    let handoff_state = failure.into_state();

    let failure = handoff_state
        .resume_recovery(&ResumePort {
            drift: ResumeDrift::ConnectionEpoch,
        })
        .unwrap_err();
    assert_eq!(
        failure.contract_error(),
        Some(RuntimeProductionLifecycleErrorV2::StaleConnectionEpoch)
    );
    let handoff_state = failure.into_state();

    let failure = handoff_state
        .resume_recovery(&ResumePort {
            drift: ResumeDrift::NotExplicitlyResumed,
        })
        .unwrap_err();
    assert_eq!(
        failure.contract_error(),
        Some(RuntimeProductionLifecycleErrorV2::ExplicitResumeMissing)
    );
    assert_eq!(
        failure.into_state().stage(),
        RuntimeProductionLifecycleStageV2::ProductionHandoff
    );

    let failure = handoff()
        .resume_recovery(&ResumePort {
            drift: ResumeDrift::WriterFenceClosed,
        })
        .unwrap_err();
    assert_eq!(
        failure.contract_error(),
        Some(RuntimeProductionLifecycleErrorV2::WriterFenceMismatch)
    );
}

#[test]
fn open_requires_current_acknowledgement_supervision_and_same_connection() {
    let state = admission();
    let port = open_port(&state, OpenDrift::Reconnect);
    let failure = state.observe_open_production(&port).unwrap_err();
    assert_eq!(
        failure.contract_error(),
        Some(RuntimeProductionLifecycleErrorV2::StaleConnectionEpoch)
    );
    assert_eq!(
        failure.into_state().stage(),
        RuntimeProductionLifecycleStageV2::AdmissionAcknowledging
    );

    let state = admission();
    let port = open_port(&state, OpenDrift::ExpiredAcknowledgement);
    let failure = state.observe_open_production(&port).unwrap_err();
    assert_eq!(
        failure.contract_error(),
        Some(RuntimeProductionLifecycleErrorV2::IngressAcknowledgementNotCurrent)
    );

    let state = admission();
    let port = open_port(&state, OpenDrift::GateNotAdvanced);
    let failure = state.observe_open_production(&port).unwrap_err();
    assert_eq!(
        failure.contract_error(),
        Some(RuntimeProductionLifecycleErrorV2::MaintenanceGateMismatch)
    );

    let state = admission();
    let port = open_port(&state, OpenDrift::SupervisorStopped);
    let failure = state.observe_open_production(&port).unwrap_err();
    assert_eq!(
        failure.contract_error(),
        Some(RuntimeProductionLifecycleErrorV2::SupervisorsNotReady)
    );

    let state = admission();
    let port = open_port(&state, OpenDrift::Port);
    let failure = state.observe_open_production(&port).unwrap_err();
    assert_eq!(failure.port_error(), Some(&TestPortError::Unavailable));
}

#[test]
fn reconnect_consumes_open_into_emergency_before_any_new_resume() {
    let open = open();
    let generation = open.coordinator_generation();
    let outcome = open
        .invalidate_production(
            generation,
            RuntimeGatewayInvalidationCauseV2::TransportDisconnected,
        )
        .unwrap();
    let RuntimeProductionInvalidationOutcomeV2::Emergency(emergency) = outcome else {
        panic!("expected emergency")
    };
    assert_eq!(
        emergency.source_stage(),
        RuntimeProductionLifecycleStageV2::OpenProduction
    );
    assert_eq!(
        emergency.cause(),
        RuntimeGatewayInvalidationCauseV2::TransportDisconnected
    );
    assert_eq!(emergency.coordinator_generation().get(), 4);
    let emergency_generation = emergency.coordinator_generation();
    let shutdown = emergency
        .begin_shutdown(
            emergency_generation,
            RuntimeShutdownCauseV2::TransportDisconnected,
        )
        .unwrap();
    assert_eq!(shutdown.coordinator_generation().get(), 5);
    assert_eq!(
        shutdown.source_stage(),
        RuntimeProductionLifecycleStageV2::Emergency
    );
}

#[test]
fn stale_shutdown_generation_returns_open_authority_and_current_generation_closes() {
    let open = open();
    let failure = open
        .begin_shutdown(
            RuntimeGatewayCoordinatorGenerationV2::FIRST,
            RuntimeShutdownCauseV2::SignalTerm,
        )
        .unwrap_err();
    assert_eq!(
        failure.contract_error(),
        Some(RuntimeProductionLifecycleErrorV2::StaleGeneration)
    );
    let open = failure.into_state();
    let generation = open.coordinator_generation();
    let shutdown = open
        .begin_shutdown(generation, RuntimeShutdownCauseV2::SignalTerm)
        .unwrap();
    assert_eq!(shutdown.cause(), RuntimeShutdownCauseV2::SignalTerm);
}

#[test]
fn generation_and_persistence_overflow_remain_paused_or_shutdown() {
    let outside_persistence = non_zero(i64::MAX as u64 + 1);
    assert_eq!(
        RuntimeMutationFinalizerGenerationV1::new(outside_persistence),
        Err(RuntimeProductionLifecycleErrorV2::SequenceOutOfRange)
    );
    assert_eq!(
        RuntimeMaintenanceGateGenerationV2::new(outside_persistence),
        Err(RuntimeProductionLifecycleErrorV2::SequenceOutOfRange)
    );

    let mut handoff = handoff();
    handoff.replace_coordinator_generation_for_test(RuntimeGatewayCoordinatorGenerationV2::new(
        non_zero(i64::MAX as u64),
    ));
    let failure = handoff
        .resume_recovery(&ResumePort {
            drift: ResumeDrift::None,
        })
        .unwrap_err();
    assert_eq!(
        failure.contract_error(),
        Some(RuntimeProductionLifecycleErrorV2::GenerationOverflow)
    );
    assert_eq!(
        failure.into_state().stage(),
        RuntimeProductionLifecycleStageV2::ProductionHandoff
    );
}

#[test]
fn authority_debug_surfaces_are_redacted() {
    assert_eq!(
        format!("{:?}", fixed_point()),
        "RuntimeStartupRecoveryFixedPointProcessV2(<redacted>)"
    );
    assert_eq!(
        format!("{:?}", handoff().recovery_resume_permit()),
        "RuntimeRecoveryResumePermitV2(<redacted>)"
    );
    assert_eq!(
        format!("{:?}", admission()),
        "RuntimeAdmissionAcknowledgingProcessV2(<redacted>)"
    );
    assert_eq!(
        format!("{:?}", open()),
        "RuntimeEmptyOpenProcessV2(<redacted>)"
    );
}
