use std::num::{NonZeroU32, NonZeroU64};

use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    BindingRevision, ControllerId, DeploymentId, FencingToken, InstallationId, LeaseRequestV1,
    ProcessInstanceId, RuntimeDeployment, RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1,
    RuntimeFailureId, RuntimeFailureKindV1, RuntimeFailureV1, RuntimeGeneration,
    RuntimeProcessIdentityV1, TenantId,
};
use chrono::{DateTime, Utc};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;

use super::{
    RuntimeSuspendAttemptDrainProgressErrorV2, RuntimeSuspendAttemptDrainProgressV2,
    RuntimeSuspendedAttemptStateErrorV2, RuntimeSuspendedAttemptStateFieldV2,
    RuntimeSuspendedAttemptV2,
};
use crate::{
    GatewayShardIdV1, RuntimeAttemptDispositionV2, RuntimeBarrierIdV1,
    RuntimeBarrierPauseWitnessV2, RuntimeBuildRevisionV1, RuntimeCanonicalSuspendAttemptV2,
    RuntimeCanonicalValueErrorV2, RuntimeClosedRecoveryRouteWitnessV2, RuntimeDeploymentScopeV1,
    RuntimeDrainObligationV2, RuntimeExactLocalRouteIdentityV2, RuntimeExecutionGuardV1,
    RuntimeExecutionReceiptV1, RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeLocalRouteEffectV2, RuntimePersistedSuspendAttemptRootV2,
    RuntimePreviousServingLeaseIdentityV1, RuntimeRecoveryIdV2, RuntimeResumeCheckpointV2,
    RuntimeRouteMutationProvenanceV2, RuntimeServingSlotV2, RuntimeSessionActionIdV1,
    RuntimeShutdownRouteWitnessV2, RuntimeSuspendAttemptCanonicalErrorV2,
    RuntimeSuspendAttemptCorrelationV2, RuntimeSuspendAttemptOperationV2,
    RuntimeSuspendAttemptRequestV2, RuntimeSuspendedRouteLifecycleV2, RuntimeSuspensionIdV2,
    RuntimeSuspensionSourcePhaseV2,
};

const SUSPENSION_ID: &str = "00112233445566778899aabbccddeeff";

fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn at(second: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(second, 0).unwrap()
}

fn at_microseconds(value: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(value).unwrap()
}

fn target() -> RuntimeDeploymentTargetV1 {
    RuntimeDeploymentTargetV1 {
        guild_id: GuildId(7),
        ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
        version: RuleSetVersionId::FIRST,
        content_hash: RuleSetContentHash::parse_hex(&"b".repeat(64)).unwrap(),
        binding_revision: BindingRevision::new(3).unwrap(),
        binding_fingerprint: ResourceBindingFingerprint::parse(&"a".repeat(64)).unwrap(),
    }
}

fn deployment_identity() -> RuntimeDeploymentIdentityV1 {
    RuntimeDeploymentIdentityV1 {
        deployment_id: DeploymentId::parse("deployment:current").unwrap(),
        tenant_id: TenantId::parse("tenant:1").unwrap(),
        installation_id: InstallationId::parse("installation:1").unwrap(),
        promotion_id: automation_runtime_convergence::PromotionId::parse("9".repeat(64)).unwrap(),
        activation_request_id: automation_runtime_convergence::ActivationRequestId::parse(
            "activation:1",
        )
        .unwrap(),
    }
}

fn previous_process() -> RuntimeProcessIdentityV1 {
    RuntimeProcessIdentityV1 {
        target: target(),
        runtime_generation: RuntimeGeneration::new(8).unwrap(),
        process_instance_id: ProcessInstanceId::parse("process:previous").unwrap(),
    }
}

fn execution(with_previous: bool) -> RuntimeExecutionReceiptV1 {
    let mut deployment = RuntimeDeployment::request(
        deployment_identity(),
        target(),
        RuntimeGeneration::new(9).unwrap(),
        with_previous.then(previous_process),
        at(1),
    )
    .unwrap();
    let controller_id = ControllerId::parse("controller:1").unwrap();
    let fencing_token = FencingToken::new(5).unwrap();
    deployment
        .acquire_lease(LeaseRequestV1 {
            expected_revision: deployment.revision(),
            controller_id: controller_id.clone(),
            fencing_token,
            now: at(2),
            expires_at: at(100),
        })
        .unwrap();
    RuntimeExecutionReceiptV1 {
        snapshot: deployment.snapshot(),
        controller_id,
        fencing_token,
        convergence_attempt: NonZeroU32::new(4).unwrap(),
        acquired_at: at(2),
        expires_at: at(100),
    }
}

fn local_route(execution: &RuntimeExecutionReceiptV1) -> RuntimeExactLocalRouteIdentityV2 {
    RuntimeExactLocalRouteIdentityV2 {
        identity: RuntimeProcessIdentityV1 {
            target: execution.snapshot.target.clone(),
            runtime_generation: execution.snapshot.runtime_generation,
            process_instance_id: ProcessInstanceId::parse("process:current").unwrap(),
        },
        controller_fencing_token: execution.fencing_token,
        route_incarnation: non_zero(6),
    }
}

fn previous_lease(execution: &RuntimeExecutionReceiptV1) -> RuntimePreviousServingLeaseIdentityV1 {
    RuntimePreviousServingLeaseIdentityV1 {
        scope: RuntimeDeploymentScopeV1 {
            tenant_id: execution.snapshot.identity.tenant_id.clone(),
            installation_id: execution.snapshot.identity.installation_id.clone(),
            deployment_id: DeploymentId::parse("deployment:previous").unwrap(),
        },
        attestation_id: crate::RuntimeAttestationIdV1::parse("d".repeat(64)).unwrap(),
        process: execution.snapshot.previous_runtime.clone().unwrap(),
        lease_epoch: non_zero(7),
        revision: non_zero(8),
    }
}

fn ordinary_provenance(seed: u128) -> RuntimeRouteMutationProvenanceV2 {
    RuntimeRouteMutationProvenanceV2::Ordinary {
        barrier_id: RuntimeBarrierIdV1::parse(format!("{seed:032x}")).unwrap(),
        pause: RuntimeBarrierPauseWitnessV2 {
            coordinator_generation: non_zero(10),
            connection_epoch: non_zero(11),
            paused_admission_revision: non_zero(12),
            pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(13)),
        },
    }
}

fn gateway_owner_lease_id(process: &str) -> RuntimeGatewayOwnerLeaseIdV1 {
    RuntimeGatewayOwnerLeaseIdV1 {
        gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        process_instance_id: ProcessInstanceId::parse(process).unwrap(),
        lease_epoch: non_zero(20),
        expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
    }
}

fn closed_recovery_provenance() -> RuntimeRouteMutationProvenanceV2 {
    RuntimeRouteMutationProvenanceV2::ClosedRecovery(RuntimeClosedRecoveryRouteWitnessV2 {
        recovery_id: RuntimeRecoveryIdV2::parse("ffeeddccbbaa99887766554433221100").unwrap(),
        originating_emergency_generation: non_zero(21),
        recovery_generation: non_zero(22),
        recovery_authority_revision: non_zero(23),
        gateway_owner_lease_id: gateway_owner_lease_id("process:gateway"),
        observed_owner_revision: non_zero(24),
        owner_expires_at: at(100),
        process_instance_id: ProcessInstanceId::parse("process:gateway").unwrap(),
        connection_epoch: non_zero(25),
        paused_admission_revision: non_zero(26),
        connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(27)),
        pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(28)),
    })
}

fn shutdown_provenance() -> RuntimeRouteMutationProvenanceV2 {
    RuntimeRouteMutationProvenanceV2::Shutdown(RuntimeShutdownRouteWitnessV2 {
        shutdown_generation: non_zero(29),
        gateway_owner_lease_id: gateway_owner_lease_id("process:gateway"),
        observed_owner_revision: non_zero(30),
        owner_expires_at: at(100),
        process_instance_id: ProcessInstanceId::parse("process:gateway").unwrap(),
        connection_epoch: non_zero(31),
        paused_admission_revision: non_zero(32),
        connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(33)),
        pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(34)),
    })
}

fn failure() -> RuntimeFailureV1 {
    RuntimeFailureV1 {
        failure_id: RuntimeFailureId::parse("failure:1").unwrap(),
        kind: RuntimeFailureKindV1::EnvironmentUnavailable,
        code: "dependency_unavailable".to_string(),
        message: "dependency unavailable".to_string(),
        recorded_at: at(3),
    }
}

fn operation(
    execution: &RuntimeExecutionReceiptV1,
    local_effect: RuntimeLocalRouteEffectV2,
    drain_obligation: RuntimeDrainObligationV2,
) -> RuntimeSuspendAttemptOperationV2 {
    let request = RuntimeSuspendAttemptRequestV2 {
        suspension_id: RuntimeSuspensionIdV2::parse(SUSPENSION_ID).unwrap(),
        action_id: RuntimeSessionActionIdV1::new(non_zero(9)),
        guard: RuntimeExecutionGuardV1 {
            scope: RuntimeDeploymentScopeV1::from_identity(&execution.snapshot.identity),
            expected_revision: execution.snapshot.revision,
            controller_id: execution.controller_id.clone(),
            fencing_token: execution.fencing_token,
            runtime_generation: execution.snapshot.runtime_generation,
            convergence_attempt: execution.convergence_attempt,
        },
        source_phase: RuntimeSuspensionSourcePhaseV2::Requested,
        failure: failure(),
        disposition: RuntimeAttemptDispositionV2::Blocked,
        checkpoint: RuntimeResumeCheckpointV2::VerifyPreflight,
        local_effect,
        drain_obligation,
    };
    RuntimeSuspendAttemptOperationV2::new(
        execution,
        RuntimeCanonicalSuspendAttemptV2::new(request).unwrap(),
    )
    .unwrap()
}

fn none_operation() -> RuntimeSuspendAttemptOperationV2 {
    operation(
        &execution(false),
        RuntimeLocalRouteEffectV2::None,
        RuntimeDrainObligationV2::None,
    )
}

fn previous_operation() -> RuntimeSuspendAttemptOperationV2 {
    let execution = execution(true);
    let previous = previous_lease(&execution);
    operation(
        &execution,
        RuntimeLocalRouteEffectV2::None,
        RuntimeDrainObligationV2::PreviousServing(previous),
    )
}

fn exact_operation(
    lifecycle: RuntimeSuspendedRouteLifecycleV2,
    with_previous: bool,
) -> RuntimeSuspendAttemptOperationV2 {
    let execution = execution(with_previous);
    let route = local_route(&execution);
    let obligation = if with_previous {
        RuntimeDrainObligationV2::LocalAndPrevious {
            local: route.clone(),
            previous: previous_lease(&execution),
        }
    } else {
        RuntimeDrainObligationV2::ExactLocalRoute(route.clone())
    };
    operation(
        &execution,
        RuntimeLocalRouteEffectV2::ExactRoute { route, lifecycle },
        obligation,
    )
}

fn absent_operation(
    with_expected_route: bool,
    with_previous: bool,
) -> RuntimeSuspendAttemptOperationV2 {
    let execution = execution(with_previous);
    let expected_route = with_expected_route.then(|| local_route(&execution));
    let obligation = if with_previous {
        RuntimeDrainObligationV2::PreviousServing(previous_lease(&execution))
    } else {
        RuntimeDrainObligationV2::None
    };
    operation(
        &execution,
        RuntimeLocalRouteEffectV2::RouteAbsent {
            slot: RuntimeServingSlotV2::from_target(&execution.snapshot.target),
            expected_route,
            provenance: ordinary_provenance(1),
            observed_sequence: non_zero(14),
        },
        obligation,
    )
}

fn initial_operations() -> Vec<RuntimeSuspendAttemptOperationV2> {
    vec![
        none_operation(),
        previous_operation(),
        exact_operation(RuntimeSuspendedRouteLifecycleV2::Staged, false),
        exact_operation(RuntimeSuspendedRouteLifecycleV2::Draining, true),
        absent_operation(false, false),
        absent_operation(true, true),
    ]
}

fn persisted_root(
    operation: &RuntimeSuspendAttemptOperationV2,
) -> RuntimePersistedSuspendAttemptRootV2 {
    RuntimePersistedSuspendAttemptRootV2::from_persisted(
        operation.operation_scope().scope().clone(),
        operation.operation_scope().deployment_revision(),
        operation.operation_scope().convergence_attempt(),
        operation.suspension_id(),
        operation.suspend_attempt_request_bytes(),
        operation.suspend_attempt_digest(),
    )
    .unwrap()
}

fn inserted(
    operation: &RuntimeSuspendAttemptOperationV2,
    sidecar_revision: u64,
) -> RuntimeSuspendedAttemptV2 {
    let request = operation.canonical_attempt().request();
    RuntimeSuspendedAttemptV2::from_inserted(
        operation,
        non_zero(sidecar_revision),
        request.local_effect.clone(),
        request.drain_obligation.clone(),
        at_microseconds(4_000_001),
    )
    .unwrap()
}

fn assert_unreachable(
    root: &RuntimePersistedSuspendAttemptRootV2,
    local_effect: RuntimeLocalRouteEffectV2,
    drain_obligation: RuntimeDrainObligationV2,
) {
    assert_eq!(
        RuntimeSuspendedAttemptV2::from_persisted(
            root,
            non_zero(2),
            local_effect,
            drain_obligation,
            at(4),
        ),
        Err(RuntimeSuspendedAttemptStateErrorV2::UnreachableMutableState)
    );
}

fn assert_progress_correlation(
    provenance: RuntimeRouteMutationProvenanceV2,
    field: RuntimeSuspendAttemptCorrelationV2,
) {
    let operation = exact_operation(RuntimeSuspendedRouteLifecycleV2::Staged, false);
    let state = inserted(&operation, 1);
    assert_eq!(
        RuntimeSuspendAttemptDrainProgressV2::record_local_absent(state, provenance, non_zero(40),),
        Err(RuntimeSuspendAttemptDrainProgressErrorV2::State(
            RuntimeSuspendedAttemptStateErrorV2::Canonical(
                RuntimeSuspendAttemptCanonicalErrorV2::CorrelationMismatch { field }
            )
        ))
    );
}

#[test]
fn all_six_canonical_roots_insert_and_restore_exactly() {
    let suspended_at = at_microseconds(4_000_001);

    for operation in initial_operations() {
        let request = operation.canonical_attempt().request();
        let state = RuntimeSuspendedAttemptV2::from_inserted(
            &operation,
            non_zero(37),
            request.local_effect.clone(),
            request.drain_obligation.clone(),
            suspended_at,
        )
        .unwrap();
        let root = persisted_root(&operation);
        let restored = RuntimeSuspendedAttemptV2::from_persisted(
            &root,
            state.sidecar_revision(),
            state.local_effect().clone(),
            state.drain_obligation().clone(),
            state.suspended_at(),
        )
        .unwrap();

        assert_eq!(restored, state);
        assert_eq!(state.operation_scope(), operation.operation_scope());
        assert_eq!(state.suspension_id(), operation.suspension_id());
        assert_eq!(state.canonical_attempt(), operation.canonical_attempt());
        assert_eq!(state.source_guard(), &request.guard);
        assert_eq!(state.source_phase(), request.source_phase);
        assert_eq!(state.failure(), &request.failure);
        assert_eq!(state.disposition(), &request.disposition);
        assert_eq!(state.checkpoint(), request.checkpoint);
        assert_eq!(
            state.suspend_attempt_request_bytes(),
            operation.suspend_attempt_request_bytes()
        );
        assert_eq!(state.request_digest(), operation.suspend_attempt_digest());
        assert_eq!(state.sidecar_revision(), non_zero(37));
        assert_eq!(state.local_effect(), &request.local_effect);
        assert_eq!(state.drain_obligation(), &request.drain_obligation);
        assert_eq!(state.suspended_at(), suspended_at);
    }
}

#[test]
fn inserted_state_requires_exact_initial_mutable_values() {
    let operation = exact_operation(RuntimeSuspendedRouteLifecycleV2::Staged, false);
    let route = match &operation.canonical_attempt().request().local_effect {
        RuntimeLocalRouteEffectV2::ExactRoute { route, .. } => route.clone(),
        _ => unreachable!(),
    };
    let reduced_effect = RuntimeLocalRouteEffectV2::RouteAbsent {
        slot: route.slot(),
        expected_route: Some(route),
        provenance: ordinary_provenance(2),
        observed_sequence: non_zero(15),
    };

    assert_eq!(
        RuntimeSuspendedAttemptV2::from_inserted(
            &operation,
            non_zero(1),
            reduced_effect,
            RuntimeDrainObligationV2::None,
            at(4),
        ),
        Err(RuntimeSuspendedAttemptStateErrorV2::InitialStateMismatch)
    );
}

#[test]
fn sidecar_revisions_accept_the_full_database_range_and_reject_overflow() {
    let operation = none_operation();
    let request = operation.canonical_attempt().request();

    for revision in [1, 17, 9_999_991, i64::MAX as u64] {
        let state = RuntimeSuspendedAttemptV2::from_inserted(
            &operation,
            non_zero(revision),
            request.local_effect.clone(),
            request.drain_obligation.clone(),
            at(4),
        )
        .unwrap();
        assert_eq!(state.sidecar_revision().get(), revision);
    }

    assert_eq!(
        RuntimeSuspendedAttemptV2::from_inserted(
            &operation,
            non_zero(i64::MAX as u64 + 1),
            request.local_effect.clone(),
            request.drain_obligation.clone(),
            at(4),
        ),
        Err(RuntimeSuspendedAttemptStateErrorV2::CanonicalValue {
            field: RuntimeSuspendedAttemptStateFieldV2::SidecarRevision,
            reason: RuntimeCanonicalValueErrorV2::PersistenceIntegerOutOfRange,
        })
    );
}

#[test]
fn suspended_at_requires_microsecond_precision_without_temporal_ordering() {
    let operation = none_operation();
    let request = operation.canonical_attempt().request();

    for suspended_at in [
        at_microseconds(-62_135_596_800_000_000),
        at_microseconds(-987_654),
        at_microseconds(4_000_001),
        at(10_000),
        at_microseconds(253_402_300_799_999_999),
    ] {
        let state = RuntimeSuspendedAttemptV2::from_inserted(
            &operation,
            non_zero(1),
            request.local_effect.clone(),
            request.drain_obligation.clone(),
            suspended_at,
        )
        .unwrap();
        assert_eq!(state.suspended_at(), suspended_at);
    }

    let sub_microsecond = DateTime::from_timestamp(4, 1).unwrap();
    assert_eq!(
        RuntimeSuspendedAttemptV2::from_inserted(
            &operation,
            non_zero(1),
            request.local_effect.clone(),
            request.drain_obligation.clone(),
            sub_microsecond,
        ),
        Err(RuntimeSuspendedAttemptStateErrorV2::CanonicalValue {
            field: RuntimeSuspendedAttemptStateFieldV2::SuspendedAt,
            reason: RuntimeCanonicalValueErrorV2::TimestampSubMicrosecond,
        })
    );

    let leap_second = DateTime::from_timestamp(59, 1_000_000_000).unwrap();
    assert_eq!(
        RuntimeSuspendedAttemptV2::from_inserted(
            &operation,
            non_zero(1),
            request.local_effect.clone(),
            request.drain_obligation.clone(),
            leap_second,
        ),
        Err(RuntimeSuspendedAttemptStateErrorV2::CanonicalValue {
            field: RuntimeSuspendedAttemptStateFieldV2::SuspendedAt,
            reason: RuntimeCanonicalValueErrorV2::TimestampLeapSecond,
        })
    );

    for out_of_range in [
        at_microseconds(-62_135_596_800_000_001),
        at_microseconds(253_402_300_800_000_000),
    ] {
        assert_eq!(
            RuntimeSuspendedAttemptV2::from_inserted(
                &operation,
                non_zero(1),
                request.local_effect.clone(),
                request.drain_obligation.clone(),
                out_of_range,
            ),
            Err(RuntimeSuspendedAttemptStateErrorV2::CanonicalValue {
                field: RuntimeSuspendedAttemptStateFieldV2::SuspendedAt,
                reason: RuntimeCanonicalValueErrorV2::TimestampOutOfRange,
            })
        );
    }
}

#[test]
fn persisted_exact_routes_allow_only_their_correlated_absence_reductions() {
    let local_only = exact_operation(RuntimeSuspendedRouteLifecycleV2::Staged, false);
    let local_only_root = persisted_root(&local_only);
    let local_route = match &local_only.canonical_attempt().request().local_effect {
        RuntimeLocalRouteEffectV2::ExactRoute { route, .. } => route.clone(),
        _ => unreachable!(),
    };
    let local_absent = RuntimeLocalRouteEffectV2::RouteAbsent {
        slot: local_route.slot(),
        expected_route: Some(local_route.clone()),
        provenance: ordinary_provenance(3),
        observed_sequence: non_zero(16),
    };
    let reduced = RuntimeSuspendedAttemptV2::from_persisted(
        &local_only_root,
        non_zero(8),
        local_absent.clone(),
        RuntimeDrainObligationV2::None,
        at(4),
    )
    .unwrap();
    assert_eq!(reduced.local_effect(), &local_absent);
    assert_eq!(reduced.drain_obligation(), &RuntimeDrainObligationV2::None);

    let with_previous = exact_operation(RuntimeSuspendedRouteLifecycleV2::Draining, true);
    let with_previous_root = persisted_root(&with_previous);
    let (route, previous) = match with_previous.canonical_attempt().request() {
        RuntimeSuspendAttemptRequestV2 {
            local_effect: RuntimeLocalRouteEffectV2::ExactRoute { route, .. },
            drain_obligation: RuntimeDrainObligationV2::LocalAndPrevious { previous, .. },
            ..
        } => (route.clone(), previous.clone()),
        _ => unreachable!(),
    };
    let absent = RuntimeLocalRouteEffectV2::RouteAbsent {
        slot: route.slot(),
        expected_route: Some(route),
        provenance: ordinary_provenance(4),
        observed_sequence: non_zero(17),
    };
    let reduced = RuntimeSuspendedAttemptV2::from_persisted(
        &with_previous_root,
        non_zero(19),
        absent.clone(),
        RuntimeDrainObligationV2::PreviousServing(previous.clone()),
        at(4),
    )
    .unwrap();
    assert_eq!(reduced.local_effect(), &absent);
    assert_eq!(
        reduced.drain_obligation(),
        &RuntimeDrainObligationV2::PreviousServing(previous)
    );
}

#[test]
fn persisted_state_rejects_lifecycle_changes_and_previous_only_removal() {
    for (root_lifecycle, replacement_lifecycle) in [
        (
            RuntimeSuspendedRouteLifecycleV2::Staged,
            RuntimeSuspendedRouteLifecycleV2::Draining,
        ),
        (
            RuntimeSuspendedRouteLifecycleV2::Draining,
            RuntimeSuspendedRouteLifecycleV2::Staged,
        ),
    ] {
        let operation = exact_operation(root_lifecycle, false);
        let root = persisted_root(&operation);
        let route = match &operation.canonical_attempt().request().local_effect {
            RuntimeLocalRouteEffectV2::ExactRoute { route, .. } => route.clone(),
            _ => unreachable!(),
        };
        assert_unreachable(
            &root,
            RuntimeLocalRouteEffectV2::ExactRoute {
                route: route.clone(),
                lifecycle: replacement_lifecycle,
            },
            RuntimeDrainObligationV2::ExactLocalRoute(route),
        );
    }

    let previous_only = previous_operation();
    assert_unreachable(
        &persisted_root(&previous_only),
        RuntimeLocalRouteEffectV2::None,
        RuntimeDrainObligationV2::None,
    );

    let local_and_previous = exact_operation(RuntimeSuspendedRouteLifecycleV2::Draining, true);
    let root = persisted_root(&local_and_previous);
    let route = match &local_and_previous
        .canonical_attempt()
        .request()
        .local_effect
    {
        RuntimeLocalRouteEffectV2::ExactRoute { route, .. } => route.clone(),
        _ => unreachable!(),
    };
    assert_unreachable(
        &root,
        RuntimeLocalRouteEffectV2::ExactRoute {
            route: route.clone(),
            lifecycle: RuntimeSuspendedRouteLifecycleV2::Draining,
        },
        RuntimeDrainObligationV2::ExactLocalRoute(route),
    );
}

#[test]
fn persisted_state_rejects_route_slot_identity_and_previous_corruption() {
    let exact = exact_operation(RuntimeSuspendedRouteLifecycleV2::Staged, false);
    let root = persisted_root(&exact);
    let route = match &exact.canonical_attempt().request().local_effect {
        RuntimeLocalRouteEffectV2::ExactRoute { route, .. } => route.clone(),
        _ => unreachable!(),
    };

    let mut changed_route = route.clone();
    changed_route.identity.process_instance_id = ProcessInstanceId::parse("process:other").unwrap();
    assert_unreachable(
        &root,
        RuntimeLocalRouteEffectV2::ExactRoute {
            route: changed_route.clone(),
            lifecycle: RuntimeSuspendedRouteLifecycleV2::Staged,
        },
        RuntimeDrainObligationV2::ExactLocalRoute(changed_route.clone()),
    );

    assert_unreachable(
        &root,
        RuntimeLocalRouteEffectV2::RouteAbsent {
            slot: RuntimeServingSlotV2::new(
                route.identity.target.guild_id,
                RuleSetKey::parse("other").unwrap(),
            ),
            expected_route: None,
            provenance: ordinary_provenance(5),
            observed_sequence: non_zero(18),
        },
        RuntimeDrainObligationV2::None,
    );

    assert_unreachable(
        &root,
        RuntimeLocalRouteEffectV2::RouteAbsent {
            slot: route.slot(),
            expected_route: None,
            provenance: ordinary_provenance(6),
            observed_sequence: non_zero(19),
        },
        RuntimeDrainObligationV2::None,
    );

    assert_unreachable(
        &root,
        RuntimeLocalRouteEffectV2::RouteAbsent {
            slot: route.slot(),
            expected_route: Some(changed_route),
            provenance: ordinary_provenance(7),
            observed_sequence: non_zero(20),
        },
        RuntimeDrainObligationV2::None,
    );

    let previous = previous_operation();
    let previous_root = persisted_root(&previous);
    let mut changed_previous = match previous
        .canonical_attempt()
        .request()
        .drain_obligation
        .clone()
    {
        RuntimeDrainObligationV2::PreviousServing(previous) => previous,
        _ => unreachable!(),
    };
    changed_previous.process.process_instance_id =
        ProcessInstanceId::parse("process:other").unwrap();
    assert_unreachable(
        &previous_root,
        RuntimeLocalRouteEffectV2::None,
        RuntimeDrainObligationV2::PreviousServing(changed_previous),
    );
}

#[test]
fn persisted_terminal_absence_rejects_proof_replacement() {
    let operation = absent_operation(true, false);
    let root = persisted_root(&operation);
    let request = operation.canonical_attempt().request();
    let (slot, expected_route, observed_sequence) = match &request.local_effect {
        RuntimeLocalRouteEffectV2::RouteAbsent {
            slot,
            expected_route,
            observed_sequence,
            ..
        } => (slot.clone(), expected_route.clone(), *observed_sequence),
        _ => unreachable!(),
    };

    assert_unreachable(
        &root,
        RuntimeLocalRouteEffectV2::RouteAbsent {
            slot: slot.clone(),
            expected_route: expected_route.clone(),
            provenance: ordinary_provenance(99),
            observed_sequence,
        },
        request.drain_obligation.clone(),
    );
    assert_unreachable(
        &root,
        RuntimeLocalRouteEffectV2::RouteAbsent {
            slot,
            expected_route,
            provenance: ordinary_provenance(1),
            observed_sequence: non_zero(observed_sequence.get() + 1),
        },
        request.drain_obligation.clone(),
    );
}

#[test]
fn progress_accepts_all_three_canonical_provenance_families() {
    for provenance in [
        ordinary_provenance(7),
        closed_recovery_provenance(),
        shutdown_provenance(),
    ] {
        let operation = exact_operation(RuntimeSuspendedRouteLifecycleV2::Staged, false);
        let state = inserted(&operation, 1);
        let progress = RuntimeSuspendAttemptDrainProgressV2::record_local_absent(
            state,
            provenance.clone(),
            non_zero(41),
        )
        .unwrap();
        assert!(matches!(
            progress.replacement_local_effect(),
            RuntimeLocalRouteEffectV2::RouteAbsent {
                provenance: actual,
                observed_sequence,
                ..
            } if actual == &provenance && *observed_sequence == non_zero(41)
        ));
    }
}

#[test]
fn progress_rejects_invalid_provenance_self_correlations() {
    let mut invalid_generation = match closed_recovery_provenance() {
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(witness) => witness,
        _ => unreachable!(),
    };
    invalid_generation.recovery_generation = non_zero(23);
    assert_progress_correlation(
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(invalid_generation),
        RuntimeSuspendAttemptCorrelationV2::RouteProvenanceGeneration,
    );

    let mut invalid_closed_process = match closed_recovery_provenance() {
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(witness) => witness,
        _ => unreachable!(),
    };
    invalid_closed_process.process_instance_id = ProcessInstanceId::parse("process:other").unwrap();
    assert_progress_correlation(
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(invalid_closed_process),
        RuntimeSuspendAttemptCorrelationV2::RouteProvenanceProcess,
    );

    let mut invalid_closed_sequence = match closed_recovery_provenance() {
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(witness) => witness,
        _ => unreachable!(),
    };
    invalid_closed_sequence.pause_sequence = invalid_closed_sequence.connected_event_sequence;
    assert_progress_correlation(
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(invalid_closed_sequence),
        RuntimeSuspendAttemptCorrelationV2::RouteProvenanceSequence,
    );

    let mut invalid_shutdown_process = match shutdown_provenance() {
        RuntimeRouteMutationProvenanceV2::Shutdown(witness) => witness,
        _ => unreachable!(),
    };
    invalid_shutdown_process
        .gateway_owner_lease_id
        .process_instance_id = ProcessInstanceId::parse("process:other").unwrap();
    assert_progress_correlation(
        RuntimeRouteMutationProvenanceV2::Shutdown(invalid_shutdown_process),
        RuntimeSuspendAttemptCorrelationV2::RouteProvenanceProcess,
    );

    let mut invalid_shutdown_sequence = match shutdown_provenance() {
        RuntimeRouteMutationProvenanceV2::Shutdown(witness) => witness,
        _ => unreachable!(),
    };
    invalid_shutdown_sequence.pause_sequence = invalid_shutdown_sequence.connected_event_sequence;
    assert_progress_correlation(
        RuntimeRouteMutationProvenanceV2::Shutdown(invalid_shutdown_sequence),
        RuntimeSuspendAttemptCorrelationV2::RouteProvenanceSequence,
    );
}

#[test]
fn both_lifecycles_and_both_local_obligations_reduce_exactly_once() {
    for lifecycle in [
        RuntimeSuspendedRouteLifecycleV2::Staged,
        RuntimeSuspendedRouteLifecycleV2::Draining,
    ] {
        for with_previous in [false, true] {
            let operation = exact_operation(lifecycle, with_previous);
            let root = persisted_root(&operation);
            let state = inserted(&operation, 30);
            let suspended_at = state.suspended_at();
            let route = match state.local_effect() {
                RuntimeLocalRouteEffectV2::ExactRoute {
                    route,
                    lifecycle: actual_lifecycle,
                } => {
                    assert_eq!(*actual_lifecycle, lifecycle);
                    route.clone()
                }
                _ => unreachable!(),
            };
            let expected_obligation = match state.drain_obligation() {
                RuntimeDrainObligationV2::ExactLocalRoute(local) => {
                    assert_eq!(local, &route);
                    RuntimeDrainObligationV2::None
                }
                RuntimeDrainObligationV2::LocalAndPrevious { local, previous } => {
                    assert_eq!(local, &route);
                    RuntimeDrainObligationV2::PreviousServing(previous.clone())
                }
                _ => unreachable!(),
            };
            let provenance = ordinary_provenance(10);
            let observed_sequence = non_zero(23);
            let progress = RuntimeSuspendAttemptDrainProgressV2::record_local_absent(
                state.clone(),
                provenance.clone(),
                observed_sequence,
            )
            .unwrap();
            let replacement_effect = RuntimeLocalRouteEffectV2::RouteAbsent {
                slot: route.slot(),
                expected_route: Some(route),
                provenance,
                observed_sequence,
            };

            assert_eq!(progress.source(), &state);
            assert_eq!(progress.expected_sidecar_revision(), non_zero(30));
            assert_eq!(progress.expected_local_effect(), state.local_effect());
            assert_eq!(
                progress.expected_drain_obligation(),
                state.drain_obligation()
            );
            assert_eq!(progress.replacement_local_effect(), &replacement_effect);
            assert_eq!(
                progress.replacement_drain_obligation(),
                &expected_obligation
            );
            assert_eq!(progress.source().suspended_at(), suspended_at);
            assert_eq!(
                progress.source().operation_scope(),
                operation.operation_scope()
            );
            assert_eq!(
                progress.source().canonical_attempt(),
                operation.canonical_attempt()
            );
            assert_eq!(
                progress.source().suspend_attempt_request_bytes(),
                operation.suspend_attempt_request_bytes()
            );
            assert_eq!(
                progress.source().request_digest(),
                operation.suspend_attempt_digest()
            );

            let restored = RuntimeSuspendedAttemptV2::from_persisted(
                &root,
                non_zero(31),
                progress.replacement_local_effect().clone(),
                progress.replacement_drain_obligation().clone(),
                progress.source().suspended_at(),
            )
            .unwrap();
            assert_eq!(restored.sidecar_revision(), non_zero(31));
            assert_eq!(restored.local_effect(), progress.replacement_local_effect());
            assert_eq!(
                restored.drain_obligation(),
                progress.replacement_drain_obligation()
            );
        }
    }
}

#[test]
fn terminal_states_cannot_record_another_local_absence() {
    let none = inserted(&none_operation(), 1);
    assert_eq!(
        RuntimeSuspendAttemptDrainProgressV2::record_local_absent(
            none,
            ordinary_provenance(11),
            non_zero(24),
        ),
        Err(RuntimeSuspendAttemptDrainProgressErrorV2::NoExactLocalRoute)
    );

    let absent = inserted(&absent_operation(false, false), 1);
    assert_eq!(
        RuntimeSuspendAttemptDrainProgressV2::record_local_absent(
            absent,
            ordinary_provenance(12),
            non_zero(25),
        ),
        Err(RuntimeSuspendAttemptDrainProgressErrorV2::NoExactLocalRoute)
    );

    let operation = exact_operation(RuntimeSuspendedRouteLifecycleV2::Draining, false);
    let root = persisted_root(&operation);
    let exact = inserted(&operation, 1);
    let progressed = RuntimeSuspendAttemptDrainProgressV2::record_local_absent(
        exact,
        ordinary_provenance(13),
        non_zero(26),
    )
    .unwrap();
    assert_eq!(
        RuntimeSuspendAttemptDrainProgressV2::record_local_absent(
            RuntimeSuspendedAttemptV2::from_persisted(
                &root,
                non_zero(2),
                progressed.replacement_local_effect().clone(),
                progressed.replacement_drain_obligation().clone(),
                progressed.source().suspended_at(),
            )
            .unwrap(),
            ordinary_provenance(14),
            non_zero(27),
        ),
        Err(RuntimeSuspendAttemptDrainProgressErrorV2::NoExactLocalRoute)
    );
}
