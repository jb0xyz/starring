use std::num::{NonZeroU32, NonZeroU64};

use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    ActivationAttestationV1, ActivationOutcomeKindV1, ActivationRequestId, BindingRevision,
    CommandGuardV1, ControllerId, DeploymentId, DeploymentRevision, DrainAttestationV1,
    FencingToken, GatewayReadyAttestationV1, GatewayReadyKindV1, InstallationId, LeaseRequestV1,
    LiveLossKindV1, PanelCertificateId, PanelCertificateV1, PreflightAttestationV1,
    ProcessInstanceId, PromotionId, RecoverLiveRequestV1, RuntimeDeployment,
    RuntimeDeploymentIdentityV1, RuntimeDeploymentPhaseV1, RuntimeDeploymentSnapshotV1,
    RuntimeDeploymentTargetV1, RuntimeFailureId, RuntimeFailureKindV1, RuntimeFailureV1,
    RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use chrono::{DateTime, Duration, Utc};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;

use super::{
    RuntimeSuspendedAttemptObservationErrorV2, RuntimeSuspendedAttemptObservationFieldV2,
    RuntimeSuspendedAttemptObservationKindV2, RuntimeSuspendedAttemptObservationV2,
};
use crate::{
    PanelReportDigestV1, RuntimeAttemptDispositionV2, RuntimeAttestationIdV1, RuntimeBarrierIdV1,
    RuntimeBarrierPauseWitnessV2, RuntimeCanonicalSuspendAttemptV2, RuntimeDeploymentScopeV1,
    RuntimeDrainObligationV2, RuntimeExactLocalRouteIdentityV2, RuntimeExecutionGuardV1,
    RuntimeExecutionReceiptV1, RuntimeGatewayAdmissionSequenceV2, RuntimeLocalRouteEffectV2,
    RuntimePreviousServingLeaseIdentityV1, RuntimeRouteMutationProvenanceV2, RuntimeServingSlotV2,
    RuntimeSessionActionIdV1, RuntimeSuspendAttemptOperationV2, RuntimeSuspendAttemptRequestV2,
    RuntimeSuspendedAttemptV2, RuntimeSuspendedRouteLifecycleV2, RuntimeSuspensionIdV2,
    RuntimeSuspensionSourcePhaseV2,
};

const SUSPENSION_ID: &str = "00112233445566778899aabbccddeeff";

fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn at(second: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(second, 0).unwrap()
}

fn identity() -> RuntimeDeploymentIdentityV1 {
    RuntimeDeploymentIdentityV1 {
        deployment_id: DeploymentId::parse("deployment:current").unwrap(),
        tenant_id: TenantId::parse("tenant:1").unwrap(),
        installation_id: InstallationId::parse("installation:1").unwrap(),
        promotion_id: PromotionId::parse("9".repeat(64)).unwrap(),
        activation_request_id: ActivationRequestId::parse("activation:1").unwrap(),
    }
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

fn previous_process() -> RuntimeProcessIdentityV1 {
    RuntimeProcessIdentityV1 {
        target: target(),
        runtime_generation: RuntimeGeneration::new(8).unwrap(),
        process_instance_id: ProcessInstanceId::parse("process:previous").unwrap(),
    }
}

fn controller_id() -> ControllerId {
    ControllerId::parse("controller:1").unwrap()
}

fn command_guard(
    deployment: &RuntimeDeployment,
    controller_id: &ControllerId,
    fencing_token: FencingToken,
    now: DateTime<Utc>,
) -> CommandGuardV1 {
    CommandGuardV1 {
        expected_revision: deployment.revision(),
        controller_id: controller_id.clone(),
        fencing_token,
        runtime_generation: deployment.runtime_generation(),
        now,
    }
}

fn receipt(
    deployment: &RuntimeDeployment,
    controller_id: ControllerId,
    fencing_token: FencingToken,
    acquired_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
) -> RuntimeExecutionReceiptV1 {
    RuntimeExecutionReceiptV1 {
        snapshot: deployment.snapshot(),
        controller_id,
        fencing_token,
        convergence_attempt: NonZeroU32::new(4).unwrap(),
        acquired_at,
        expires_at,
    }
}

fn requested_deployment(with_previous: bool) -> (RuntimeDeployment, ControllerId, FencingToken) {
    let mut deployment = RuntimeDeployment::request(
        identity(),
        target(),
        RuntimeGeneration::new(9).unwrap(),
        with_previous.then(previous_process),
        at(1),
    )
    .unwrap();
    let controller_id = controller_id();
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
    (deployment, controller_id, fencing_token)
}

fn transition_to_phase(
    source_phase: RuntimeSuspensionSourcePhaseV2,
    with_previous: bool,
) -> RuntimeExecutionReceiptV1 {
    let (mut deployment, controller_id, fencing_token) = requested_deployment(with_previous);
    if source_phase == RuntimeSuspensionSourcePhaseV2::Requested {
        return receipt(&deployment, controller_id, fencing_token, at(2), at(100));
    }
    deployment
        .accept_preflight(
            &command_guard(&deployment, &controller_id, fencing_token, at(3)),
            PreflightAttestationV1 {
                target: deployment.target().clone(),
                runtime_generation: deployment.runtime_generation(),
                observed_runtime: deployment.snapshot().previous_runtime,
                checked_at: at(3),
            },
        )
        .unwrap();
    if source_phase == RuntimeSuspensionSourcePhaseV2::PreflightReady {
        return receipt(&deployment, controller_id, fencing_token, at(2), at(100));
    }
    deployment
        .request_drain(&command_guard(
            &deployment,
            &controller_id,
            fencing_token,
            at(4),
        ))
        .unwrap();
    if source_phase == RuntimeSuspensionSourcePhaseV2::DrainRequested {
        return receipt(&deployment, controller_id, fencing_token, at(2), at(100));
    }
    deployment
        .accept_drain(
            &command_guard(&deployment, &controller_id, fencing_token, at(5)),
            DrainAttestationV1 {
                previous_runtime: deployment.snapshot().previous_runtime,
                target_runtime_generation: deployment.runtime_generation(),
                drained_at: at(5),
            },
        )
        .unwrap();
    if source_phase == RuntimeSuspensionSourcePhaseV2::Drained {
        return receipt(&deployment, controller_id, fencing_token, at(2), at(100));
    }
    deployment
        .begin_activation(&command_guard(
            &deployment,
            &controller_id,
            fencing_token,
            at(6),
        ))
        .unwrap();
    if source_phase == RuntimeSuspensionSourcePhaseV2::ActivationApplying {
        return receipt(&deployment, controller_id, fencing_token, at(2), at(100));
    }
    deployment
        .accept_activation(
            &command_guard(&deployment, &controller_id, fencing_token, at(7)),
            ActivationAttestationV1 {
                activation_request_id: deployment.identity().activation_request_id.clone(),
                target: deployment.target().clone(),
                runtime_generation: deployment.runtime_generation(),
                kind: ActivationOutcomeKindV1::Activated,
                activated_at: at(7),
            },
        )
        .unwrap();
    if source_phase == RuntimeSuspensionSourcePhaseV2::RuntimePendingReady {
        return receipt(&deployment, controller_id, fencing_token, at(2), at(100));
    }
    deployment
        .begin_panel_reconciliation(&command_guard(
            &deployment,
            &controller_id,
            fencing_token,
            at(8),
        ))
        .unwrap();
    receipt(&deployment, controller_id, fencing_token, at(2), at(100))
}

fn panel_certificate(
    deployment: &RuntimeDeployment,
    reconciled_at: DateTime<Utc>,
) -> PanelCertificateV1 {
    PanelCertificateV1 {
        certificate_id: PanelCertificateId::parse("panel:1").unwrap(),
        report_digest: PanelReportDigestV1::parse("c".repeat(64)).unwrap(),
        target: deployment.target().clone(),
        runtime_generation: deployment.runtime_generation(),
        process_instance_id: ProcessInstanceId::parse("process:current").unwrap(),
        declared_count: 0,
        installed_count: 0,
        unchanged_count: 0,
        skipped_transient_count: 0,
        skipped_unresolved_channel_count: 0,
        failed_count: 0,
        ambiguous_outcome_count: 0,
        stale_message_cleanup_pending_count: 0,
        orphan_message_cleanup_pending_count: 0,
        reposted_old_message_cleanup_pending_count: 0,
        reconciled_at,
    }
}

fn recovered_live_execution() -> RuntimeExecutionReceiptV1 {
    let execution = transition_to_phase(RuntimeSuspensionSourcePhaseV2::ReconcilingPanels, false);
    let mut deployment = RuntimeDeployment::restore(execution.snapshot).unwrap();
    let process_instance_id = ProcessInstanceId::parse("process:current").unwrap();
    deployment
        .accept_panel_certificate(
            &command_guard(
                &deployment,
                &execution.controller_id,
                execution.fencing_token,
                at(9),
            ),
            panel_certificate(&deployment, at(9)),
        )
        .unwrap();
    deployment
        .certify_live(
            &command_guard(
                &deployment,
                &execution.controller_id,
                execution.fencing_token,
                at(10),
            ),
            GatewayReadyAttestationV1 {
                target: deployment.target().clone(),
                runtime_generation: deployment.runtime_generation(),
                process_instance_id: process_instance_id.clone(),
                kind: GatewayReadyKindV1::DiscordReady,
                ready_at: at(10),
            },
            at(11),
        )
        .unwrap();
    deployment
        .recover_live(RecoverLiveRequestV1 {
            expected_revision: deployment.revision(),
            expected_runtime_generation: deployment.runtime_generation(),
            expected_process_instance_id: process_instance_id,
            kind: LiveLossKindV1::ServingDisconnected,
            evidence_at: at(12),
            recovered_at: at(13),
        })
        .unwrap();
    let fencing_token = FencingToken::new(6).unwrap();
    deployment
        .acquire_lease(LeaseRequestV1 {
            expected_revision: deployment.revision(),
            controller_id: execution.controller_id.clone(),
            fencing_token,
            now: at(14),
            expires_at: at(100),
        })
        .unwrap();
    receipt(
        &deployment,
        execution.controller_id,
        fencing_token,
        at(14),
        at(100),
    )
}

fn evidence_floor(execution: &RuntimeExecutionReceiptV1) -> DateTime<Utc> {
    match RuntimeSuspensionSourcePhaseV2::from_deployment_phase(&execution.snapshot.phase).unwrap()
    {
        RuntimeSuspensionSourcePhaseV2::Requested => execution.snapshot.requested_at,
        RuntimeSuspensionSourcePhaseV2::PreflightReady
        | RuntimeSuspensionSourcePhaseV2::DrainRequested => {
            execution.snapshot.preflight.as_ref().unwrap().checked_at
        }
        RuntimeSuspensionSourcePhaseV2::Drained
        | RuntimeSuspensionSourcePhaseV2::ActivationApplying => {
            execution.snapshot.drain.as_ref().unwrap().drained_at
        }
        RuntimeSuspensionSourcePhaseV2::RuntimePendingReady
        | RuntimeSuspensionSourcePhaseV2::ReconcilingPanels => {
            let activated_at = execution.snapshot.activation.as_ref().unwrap().activated_at;
            execution
                .snapshot
                .last_live_recovery
                .as_ref()
                .map_or(activated_at, |recovery| {
                    activated_at.max(recovery.recovered_at)
                })
        }
    }
}

fn failure(recorded_at: DateTime<Utc>) -> RuntimeFailureV1 {
    RuntimeFailureV1 {
        failure_id: RuntimeFailureId::parse("failure:1").unwrap(),
        kind: RuntimeFailureKindV1::EnvironmentUnavailable,
        code: "dependency_unavailable".to_string(),
        message: "dependency unavailable".to_string(),
        recorded_at,
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
        route_incarnation: non_zero(7),
    }
}

fn previous_lease(execution: &RuntimeExecutionReceiptV1) -> RuntimePreviousServingLeaseIdentityV1 {
    RuntimePreviousServingLeaseIdentityV1 {
        scope: RuntimeDeploymentScopeV1 {
            tenant_id: execution.snapshot.identity.tenant_id.clone(),
            installation_id: execution.snapshot.identity.installation_id.clone(),
            deployment_id: DeploymentId::parse("deployment:previous").unwrap(),
        },
        attestation_id: RuntimeAttestationIdV1::parse("d".repeat(64)).unwrap(),
        process: execution.snapshot.previous_runtime.clone().unwrap(),
        lease_epoch: non_zero(8),
        revision: non_zero(9),
    }
}

fn ordinary_provenance() -> RuntimeRouteMutationProvenanceV2 {
    RuntimeRouteMutationProvenanceV2::Ordinary {
        barrier_id: RuntimeBarrierIdV1::parse("ffeeddccbbaa99887766554433221100").unwrap(),
        pause: RuntimeBarrierPauseWitnessV2 {
            coordinator_generation: non_zero(10),
            connection_epoch: non_zero(11),
            paused_admission_revision: non_zero(12),
            pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(13)),
        },
    }
}

fn suspended(
    execution: &RuntimeExecutionReceiptV1,
    local_effect: RuntimeLocalRouteEffectV2,
    drain_obligation: RuntimeDrainObligationV2,
    failure_recorded_at: DateTime<Utc>,
) -> RuntimeSuspendedAttemptV2 {
    let source_phase =
        RuntimeSuspensionSourcePhaseV2::from_deployment_phase(&execution.snapshot.phase).unwrap();
    let request = RuntimeSuspendAttemptRequestV2 {
        suspension_id: RuntimeSuspensionIdV2::parse(SUSPENSION_ID).unwrap(),
        action_id: RuntimeSessionActionIdV1::new(non_zero(14)),
        guard: RuntimeExecutionGuardV1 {
            scope: RuntimeDeploymentScopeV1::from_identity(&execution.snapshot.identity),
            expected_revision: execution.snapshot.revision,
            controller_id: execution.controller_id.clone(),
            fencing_token: execution.fencing_token,
            runtime_generation: execution.snapshot.runtime_generation,
            convergence_attempt: execution.convergence_attempt,
        },
        source_phase,
        failure: failure(failure_recorded_at),
        disposition: RuntimeAttemptDispositionV2::Blocked,
        checkpoint: source_phase.required_checkpoint(),
        local_effect,
        drain_obligation,
    };
    let operation = RuntimeSuspendAttemptOperationV2::new(
        execution,
        RuntimeCanonicalSuspendAttemptV2::new(request).unwrap(),
    )
    .unwrap();
    let request = operation.canonical_attempt().request();
    RuntimeSuspendedAttemptV2::from_inserted(
        &operation,
        non_zero(15),
        request.local_effect.clone(),
        request.drain_obligation.clone(),
        at(1_000),
    )
    .unwrap()
}

fn none_suspended(execution: &RuntimeExecutionReceiptV1) -> RuntimeSuspendedAttemptV2 {
    suspended(
        execution,
        RuntimeLocalRouteEffectV2::None,
        RuntimeDrainObligationV2::None,
        evidence_floor(execution),
    )
}

fn exact_suspended(execution: &RuntimeExecutionReceiptV1) -> RuntimeSuspendedAttemptV2 {
    let route = local_route(execution);
    suspended(
        execution,
        RuntimeLocalRouteEffectV2::ExactRoute {
            route: route.clone(),
            lifecycle: RuntimeSuspendedRouteLifecycleV2::Draining,
        },
        RuntimeDrainObligationV2::ExactLocalRoute(route),
        evidence_floor(execution),
    )
}

fn absent_suspended(
    execution: &RuntimeExecutionReceiptV1,
    with_expected_route: bool,
) -> RuntimeSuspendedAttemptV2 {
    suspended(
        execution,
        RuntimeLocalRouteEffectV2::RouteAbsent {
            slot: RuntimeServingSlotV2::from_target(&execution.snapshot.target),
            expected_route: with_expected_route.then(|| local_route(execution)),
            provenance: ordinary_provenance(),
            observed_sequence: non_zero(16),
        },
        RuntimeDrainObligationV2::None,
        evidence_floor(execution),
    )
}

fn previous_suspended(execution: &RuntimeExecutionReceiptV1) -> RuntimeSuspendedAttemptV2 {
    suspended(
        execution,
        RuntimeLocalRouteEffectV2::None,
        RuntimeDrainObligationV2::PreviousServing(previous_lease(execution)),
        evidence_floor(execution),
    )
}

fn without_lease(mut snapshot: RuntimeDeploymentSnapshotV1) -> RuntimeDeploymentSnapshotV1 {
    snapshot.controller_lease = None;
    snapshot
}

fn assert_mismatch(
    snapshot: RuntimeDeploymentSnapshotV1,
    suspended: &RuntimeSuspendedAttemptV2,
    field: RuntimeSuspendedAttemptObservationFieldV2,
) {
    assert_eq!(
        RuntimeSuspendedAttemptObservationV2::new(snapshot, suspended.clone()),
        Err(RuntimeSuspendedAttemptObservationErrorV2::CorrelationMismatch { field })
    );
}

#[test]
fn every_suspendable_phase_observes_exact_release_and_quiescent_states() {
    for source_phase in [
        RuntimeSuspensionSourcePhaseV2::Requested,
        RuntimeSuspensionSourcePhaseV2::PreflightReady,
        RuntimeSuspensionSourcePhaseV2::DrainRequested,
        RuntimeSuspensionSourcePhaseV2::Drained,
        RuntimeSuspensionSourcePhaseV2::ActivationApplying,
        RuntimeSuspensionSourcePhaseV2::RuntimePendingReady,
        RuntimeSuspensionSourcePhaseV2::ReconcilingPanels,
    ] {
        let execution = transition_to_phase(source_phase, false);
        let suspended = none_suspended(&execution);
        let release = RuntimeSuspendedAttemptObservationV2::new(
            execution.snapshot.clone(),
            suspended.clone(),
        )
        .unwrap();
        assert_eq!(
            release.kind(),
            RuntimeSuspendedAttemptObservationKindV2::ReleasePending
        );
        assert_eq!(release.snapshot(), &execution.snapshot);
        assert_eq!(release.suspended(), &suspended);

        let snapshot = without_lease(execution.snapshot);
        let quiescent =
            RuntimeSuspendedAttemptObservationV2::new(snapshot.clone(), suspended.clone()).unwrap();
        assert_eq!(
            quiescent.kind(),
            RuntimeSuspendedAttemptObservationKindV2::LocallyQuiescent
        );
        assert_eq!(quiescent.snapshot(), &snapshot);
        assert_eq!(quiescent.suspended(), &suspended);
    }
}

#[test]
fn exact_scope_revision_generation_phase_and_last_fence_are_required() {
    let execution = transition_to_phase(RuntimeSuspensionSourcePhaseV2::Requested, false);
    let suspended = none_suspended(&execution);

    for snapshot in {
        let mut values = Vec::new();
        let mut value = execution.snapshot.clone();
        value.identity.tenant_id = TenantId::parse("tenant:other").unwrap();
        values.push(value);
        let mut value = execution.snapshot.clone();
        value.identity.installation_id = InstallationId::parse("installation:other").unwrap();
        values.push(value);
        let mut value = execution.snapshot.clone();
        value.identity.deployment_id = DeploymentId::parse("deployment:other").unwrap();
        values.push(value);
        values
    } {
        assert_mismatch(
            snapshot,
            &suspended,
            RuntimeSuspendedAttemptObservationFieldV2::Scope,
        );
    }

    for revision in [
        DeploymentRevision::new(execution.snapshot.revision.get() - 1).unwrap(),
        execution.snapshot.revision.next().unwrap(),
    ] {
        let mut snapshot = execution.snapshot.clone();
        snapshot.revision = revision;
        assert_mismatch(
            snapshot,
            &suspended,
            RuntimeSuspendedAttemptObservationFieldV2::DeploymentRevision,
        );
    }

    let mut generation = execution.snapshot.clone();
    generation.runtime_generation = RuntimeGeneration::new(10).unwrap();
    assert_mismatch(
        generation,
        &suspended,
        RuntimeSuspendedAttemptObservationFieldV2::RuntimeGeneration,
    );

    let preflight = transition_to_phase(RuntimeSuspensionSourcePhaseV2::PreflightReady, false);
    let preflight_suspended = none_suspended(&preflight);
    let mut phase = preflight.snapshot;
    phase.phase = RuntimeDeploymentPhaseV1::DrainRequested;
    assert_mismatch(
        phase,
        &preflight_suspended,
        RuntimeSuspendedAttemptObservationFieldV2::SourcePhase,
    );

    for last_fencing_token in [None, Some(FencingToken::new(6).unwrap())] {
        let mut snapshot = without_lease(execution.snapshot.clone());
        snapshot.last_fencing_token = last_fencing_token;
        assert_mismatch(
            snapshot,
            &suspended,
            RuntimeSuspendedAttemptObservationFieldV2::LastFencingToken,
        );
    }
}

#[test]
fn an_exact_local_route_requires_the_full_target_and_source_lease() {
    let execution = transition_to_phase(RuntimeSuspensionSourcePhaseV2::Requested, false);
    let suspended = exact_suspended(&execution);
    let observation =
        RuntimeSuspendedAttemptObservationV2::new(execution.snapshot.clone(), suspended.clone())
            .unwrap();
    assert_eq!(
        observation.kind(),
        RuntimeSuspendedAttemptObservationKindV2::LocalRoutePresent
    );

    let mut targets = Vec::new();
    let mut value = execution.snapshot.target.clone();
    value.guild_id = GuildId(8);
    targets.push(value);
    let mut value = execution.snapshot.target.clone();
    value.ruleset_key = RuleSetKey::parse("other").unwrap();
    targets.push(value);
    let mut value = execution.snapshot.target.clone();
    value.version = RuleSetVersionId::new(2).unwrap();
    targets.push(value);
    let mut value = execution.snapshot.target.clone();
    value.content_hash = RuleSetContentHash::parse_hex(&"c".repeat(64)).unwrap();
    targets.push(value);
    let mut value = execution.snapshot.target.clone();
    value.binding_revision = BindingRevision::new(4).unwrap();
    targets.push(value);
    let mut value = execution.snapshot.target.clone();
    value.binding_fingerprint = ResourceBindingFingerprint::parse(&"d".repeat(64)).unwrap();
    targets.push(value);

    for target in targets {
        let mut snapshot = execution.snapshot.clone();
        snapshot.target = target;
        assert_mismatch(
            snapshot,
            &suspended,
            RuntimeSuspendedAttemptObservationFieldV2::CurrentTarget,
        );
    }

    assert_eq!(
        RuntimeSuspendedAttemptObservationV2::new(without_lease(execution.snapshot), suspended,),
        Err(RuntimeSuspendedAttemptObservationErrorV2::LocalRouteLeaseMissing)
    );
}

#[test]
fn route_absence_correlates_optional_identity_and_exact_slot() {
    let execution = transition_to_phase(RuntimeSuspensionSourcePhaseV2::Requested, false);
    let expected = absent_suspended(&execution, true);
    assert_eq!(
        RuntimeSuspendedAttemptObservationV2::new(execution.snapshot.clone(), expected.clone(),)
            .unwrap()
            .kind(),
        RuntimeSuspendedAttemptObservationKindV2::ReleasePending
    );

    for mutate in [0_u8, 1_u8, 2_u8, 3_u8] {
        let mut snapshot = execution.snapshot.clone();
        match mutate {
            0 => snapshot.target.version = RuleSetVersionId::new(2).unwrap(),
            1 => {
                snapshot.target.content_hash =
                    RuleSetContentHash::parse_hex(&"c".repeat(64)).unwrap()
            }
            2 => snapshot.target.binding_revision = BindingRevision::new(4).unwrap(),
            _ => {
                snapshot.target.binding_fingerprint =
                    ResourceBindingFingerprint::parse(&"d".repeat(64)).unwrap()
            }
        }
        assert_mismatch(
            snapshot,
            &expected,
            RuntimeSuspendedAttemptObservationFieldV2::CurrentTarget,
        );
    }

    let no_expected = absent_suspended(&execution, false);
    assert!(RuntimeSuspendedAttemptObservationV2::new(
        execution.snapshot.clone(),
        no_expected.clone(),
    )
    .is_ok());

    for target in {
        let mut values = Vec::new();
        let mut value = execution.snapshot.target.clone();
        value.guild_id = GuildId(8);
        values.push(value);
        let mut value = execution.snapshot.target.clone();
        value.ruleset_key = RuleSetKey::parse("other").unwrap();
        values.push(value);
        values
    } {
        let mut snapshot = execution.snapshot.clone();
        snapshot.target = target;
        assert_mismatch(
            snapshot,
            &no_expected,
            RuntimeSuspendedAttemptObservationFieldV2::CurrentTarget,
        );
    }
}

#[test]
fn represented_previous_runtime_is_exact_and_must_share_the_current_slot() {
    let execution = transition_to_phase(RuntimeSuspensionSourcePhaseV2::Requested, true);
    let suspended = previous_suspended(&execution);
    assert!(RuntimeSuspendedAttemptObservationV2::new(
        execution.snapshot.clone(),
        suspended.clone(),
    )
    .is_ok());

    let mut process = execution.snapshot.clone();
    process
        .previous_runtime
        .as_mut()
        .unwrap()
        .process_instance_id = ProcessInstanceId::parse("process:other").unwrap();
    assert_mismatch(
        process,
        &suspended,
        RuntimeSuspendedAttemptObservationFieldV2::PreviousRuntime,
    );

    let mut generation = execution.snapshot.clone();
    generation
        .previous_runtime
        .as_mut()
        .unwrap()
        .runtime_generation = RuntimeGeneration::new(7).unwrap();
    assert_mismatch(
        generation,
        &suspended,
        RuntimeSuspendedAttemptObservationFieldV2::PreviousRuntime,
    );

    let mut wrong_slot = execution.snapshot.clone();
    wrong_slot.target.guild_id = GuildId(8);
    wrong_slot
        .previous_runtime
        .as_mut()
        .unwrap()
        .target
        .guild_id = GuildId(8);
    assert_mismatch(
        wrong_slot,
        &suspended,
        RuntimeSuspendedAttemptObservationFieldV2::PreviousRuntime,
    );
}

#[test]
fn an_unrepresented_previous_runtime_may_remain_or_be_omitted() {
    let execution = transition_to_phase(RuntimeSuspensionSourcePhaseV2::Requested, true);
    let suspended = none_suspended(&execution);

    assert!(RuntimeSuspendedAttemptObservationV2::new(
        execution.snapshot.clone(),
        suspended.clone(),
    )
    .is_ok());

    let mut omitted = execution.snapshot;
    omitted.previous_runtime = None;
    assert!(RuntimeSuspendedAttemptObservationV2::new(omitted, suspended).is_ok());
}

#[test]
fn lease_identity_selects_local_present_release_pending_or_rejection() {
    let execution = transition_to_phase(RuntimeSuspensionSourcePhaseV2::Requested, false);
    let exact = exact_suspended(&execution);
    let none = none_suspended(&execution);

    assert_eq!(
        RuntimeSuspendedAttemptObservationV2::new(execution.snapshot.clone(), exact.clone(),)
            .unwrap()
            .kind(),
        RuntimeSuspendedAttemptObservationKindV2::LocalRoutePresent
    );
    assert_eq!(
        RuntimeSuspendedAttemptObservationV2::new(execution.snapshot.clone(), none.clone(),)
            .unwrap()
            .kind(),
        RuntimeSuspendedAttemptObservationKindV2::ReleasePending
    );

    let mut foreign_controller = execution.snapshot.clone();
    foreign_controller
        .controller_lease
        .as_mut()
        .unwrap()
        .controller_id = ControllerId::parse("controller:other").unwrap();
    for suspended in [&exact, &none] {
        assert_mismatch(
            foreign_controller.clone(),
            suspended,
            RuntimeSuspendedAttemptObservationFieldV2::ControllerId,
        );
    }

    let mut foreign_fence = execution.snapshot.clone();
    foreign_fence
        .controller_lease
        .as_mut()
        .unwrap()
        .fencing_token = FencingToken::new(6).unwrap();
    foreign_fence.last_fencing_token = Some(FencingToken::new(6).unwrap());
    for suspended in [&exact, &none] {
        assert_mismatch(
            foreign_fence.clone(),
            suspended,
            RuntimeSuspendedAttemptObservationFieldV2::FencingToken,
        );
    }
}

#[test]
fn no_lease_is_locally_quiescent_for_none_and_previous_obligations() {
    let without_previous = transition_to_phase(RuntimeSuspensionSourcePhaseV2::Requested, false);
    let none = none_suspended(&without_previous);
    assert_eq!(
        RuntimeSuspendedAttemptObservationV2::new(without_lease(without_previous.snapshot), none,)
            .unwrap()
            .kind(),
        RuntimeSuspendedAttemptObservationKindV2::LocallyQuiescent
    );

    let with_previous = transition_to_phase(RuntimeSuspensionSourcePhaseV2::Requested, true);
    let previous = previous_suspended(&with_previous);
    assert_eq!(
        RuntimeSuspendedAttemptObservationV2::new(without_lease(with_previous.snapshot), previous,)
            .unwrap()
            .kind(),
        RuntimeSuspendedAttemptObservationKindV2::LocallyQuiescent
    );
}

#[test]
fn invalid_snapshot_and_lease_windows_are_rejected_before_correlation() {
    let execution = transition_to_phase(RuntimeSuspensionSourcePhaseV2::Requested, false);
    let suspended = none_suspended(&execution);

    let mut invalid_phase = execution.snapshot.clone();
    invalid_phase.phase = RuntimeDeploymentPhaseV1::PreflightReady;
    assert_eq!(
        RuntimeSuspendedAttemptObservationV2::new(invalid_phase, suspended.clone()),
        Err(RuntimeSuspendedAttemptObservationErrorV2::InvalidSnapshot)
    );

    let mut zero_window = execution.snapshot.clone();
    let lease = zero_window.controller_lease.as_mut().unwrap();
    lease.expires_at = lease.acquired_at;
    assert_eq!(
        RuntimeSuspendedAttemptObservationV2::new(zero_window, suspended.clone()),
        Err(RuntimeSuspendedAttemptObservationErrorV2::InvalidSnapshot)
    );

    let mut before_request = execution.snapshot;
    before_request
        .controller_lease
        .as_mut()
        .unwrap()
        .acquired_at = at(0);
    assert_eq!(
        RuntimeSuspendedAttemptObservationV2::new(before_request, suspended),
        Err(RuntimeSuspendedAttemptObservationErrorV2::InvalidSnapshot)
    );
}

#[test]
fn observation_uses_durable_lease_evidence_without_a_host_clock_rule() {
    let execution = transition_to_phase(RuntimeSuspensionSourcePhaseV2::Requested, false);
    let suspended = none_suspended(&execution);
    assert!(execution.expires_at < suspended.suspended_at());

    let observation =
        RuntimeSuspendedAttemptObservationV2::new(execution.snapshot, suspended).unwrap();
    assert_eq!(
        observation.kind(),
        RuntimeSuspendedAttemptObservationKindV2::ReleasePending
    );
}

#[test]
fn persisted_failure_time_is_correlated_to_the_observed_phase_floor() {
    let execution = transition_to_phase(RuntimeSuspensionSourcePhaseV2::PreflightReady, false);
    let floor = evidence_floor(&execution);

    for recorded_at in [floor, floor + Duration::microseconds(1)] {
        let suspended = suspended(
            &execution,
            RuntimeLocalRouteEffectV2::None,
            RuntimeDrainObligationV2::None,
            recorded_at,
        );
        assert!(
            RuntimeSuspendedAttemptObservationV2::new(execution.snapshot.clone(), suspended,)
                .is_ok()
        );
    }

    let suspended = none_suspended(&execution);
    let mut later_evidence = execution.snapshot;
    later_evidence.preflight.as_mut().unwrap().checked_at = floor + Duration::microseconds(1);
    assert_mismatch(
        later_evidence,
        &suspended,
        RuntimeSuspendedAttemptObservationFieldV2::FailureRecordedAt,
    );
}

#[test]
fn live_recovery_history_raises_the_observed_runtime_pending_floor() {
    let ordinary = transition_to_phase(RuntimeSuspensionSourcePhaseV2::RuntimePendingReady, false);
    let stale = none_suspended(&ordinary);
    assert_eq!(stale.failure().recorded_at, at(7));

    let recovered = recovered_live_execution();
    assert_eq!(
        recovered
            .snapshot
            .last_live_recovery
            .as_ref()
            .unwrap()
            .recovered_at,
        at(13)
    );
    let exact = none_suspended(&recovered);
    assert_eq!(exact.failure().recorded_at, at(13));
    assert!(RuntimeSuspendedAttemptObservationV2::new(recovered.snapshot.clone(), exact,).is_ok());

    let mut recovered_snapshot = recovered.snapshot;
    recovered_snapshot.revision = ordinary.snapshot.revision;
    recovered_snapshot.controller_lease = ordinary.snapshot.controller_lease.clone();
    recovered_snapshot.last_fencing_token = ordinary.snapshot.last_fencing_token;
    assert_mismatch(
        recovered_snapshot,
        &stale,
        RuntimeSuspendedAttemptObservationFieldV2::FailureRecordedAt,
    );
}

#[test]
fn only_locally_quiescent_observations_convert_to_the_wrapper() {
    let execution = transition_to_phase(RuntimeSuspensionSourcePhaseV2::Requested, false);
    let exact = exact_suspended(&execution);
    let none = none_suspended(&execution);

    let local =
        RuntimeSuspendedAttemptObservationV2::new(execution.snapshot.clone(), exact).unwrap();
    assert_eq!(
        local.into_locally_quiescent(),
        Err(RuntimeSuspendedAttemptObservationErrorV2::NotQuiescent)
    );

    let release =
        RuntimeSuspendedAttemptObservationV2::new(execution.snapshot.clone(), none.clone())
            .unwrap();
    assert_eq!(
        release.into_locally_quiescent(),
        Err(RuntimeSuspendedAttemptObservationErrorV2::NotQuiescent)
    );

    let snapshot = without_lease(execution.snapshot);
    let observation =
        RuntimeSuspendedAttemptObservationV2::new(snapshot.clone(), none.clone()).unwrap();
    let quiescent = observation.clone().into_locally_quiescent().unwrap();
    assert_eq!(quiescent.observation(), &observation);
    assert_eq!(quiescent.snapshot(), &snapshot);
    assert_eq!(quiescent.suspended(), &none);
}
