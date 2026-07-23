use std::num::{NonZeroU32, NonZeroU64};

use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    ActivationAttestationV1, ActivationOutcomeKindV1, ActivationRequestId, BindingRevision,
    CommandGuardV1, ControllerId, DeploymentId, DrainAttestationV1, FencingToken,
    GatewayReadyAttestationV1, GatewayReadyKindV1, InstallationId, LeaseRequestV1, LiveLossKindV1,
    PanelCertificateId, PanelCertificateV1, ProcessInstanceId, PromotionId, RecoverLiveRequestV1,
    RuntimeDeployment, RuntimeDeploymentIdentityV1, RuntimeDeploymentPhaseV1,
    RuntimeDeploymentTargetV1, RuntimeFailureId, RuntimeFailureKindV1, RuntimeFailureV1,
    RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use chrono::{DateTime, Duration, Utc};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;

use super::{
    RuntimePersistedSuspendAttemptRootV2, RuntimeSuspendAttemptOperationBuildErrorV2,
    RuntimeSuspendAttemptOperationFieldV2, RuntimeSuspendAttemptOperationPersistenceErrorV2,
    RuntimeSuspendAttemptOperationV2, RuntimeSuspendAttemptReplayErrorV2,
    RuntimeSuspendAttemptScopeLookupV2,
};
use crate::{
    PanelReportDigestV1, RuntimeAttemptDispositionV2, RuntimeAttestationIdV1, RuntimeBarrierIdV1,
    RuntimeBarrierPauseWitnessV2, RuntimeCanonicalSuspendAttemptV2, RuntimeDeploymentScopeV1,
    RuntimeDrainObligationV2, RuntimeExactLocalRouteIdentityV2, RuntimeExecutionGuardV1,
    RuntimeExecutionReceiptV1, RuntimeGatewayAdmissionSequenceV2, RuntimeLocalRouteEffectV2,
    RuntimePreviousServingLeaseIdentityV1, RuntimeResumeCheckpointV2,
    RuntimeRouteMutationProvenanceV2, RuntimeServingSlotV2, RuntimeSessionActionIdV1,
    RuntimeSuspendAttemptDigestV2, RuntimeSuspendAttemptRequestV2,
    RuntimeSuspendedRouteLifecycleV2, RuntimeSuspensionIdV2, RuntimeSuspensionSourcePhaseV2,
};

const SUSPENSION_ID: &str = "00112233445566778899aabbccddeeff";
const OTHER_SUSPENSION_ID: &str = "ffeeddccbbaa99887766554433221100";

fn at(second: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(second, 0).unwrap()
}

fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn identity(deployment_id: &str) -> RuntimeDeploymentIdentityV1 {
    RuntimeDeploymentIdentityV1 {
        deployment_id: DeploymentId::parse(deployment_id).unwrap(),
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
        convergence_attempt: NonZeroU32::new(5).unwrap(),
        acquired_at,
        expires_at,
    }
}

fn requested_deployment(with_previous: bool) -> (RuntimeDeployment, ControllerId, FencingToken) {
    requested_deployment_for("deployment:1", with_previous)
}

fn requested_deployment_for(
    deployment_id: &str,
    with_previous: bool,
) -> (RuntimeDeployment, ControllerId, FencingToken) {
    let previous = with_previous.then(previous_process);
    let mut deployment = RuntimeDeployment::request(
        identity(deployment_id),
        target(),
        RuntimeGeneration::new(9).unwrap(),
        previous,
        at(1),
    )
    .unwrap();
    let controller_id = controller_id();
    let fencing_token = FencingToken::new(3).unwrap();
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

fn requested_execution_for(deployment_id: &str) -> RuntimeExecutionReceiptV1 {
    let (deployment, controller_id, fencing_token) = requested_deployment_for(deployment_id, false);
    receipt(&deployment, controller_id, fencing_token, at(2), at(100))
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
            automation_runtime_convergence::PreflightAttestationV1 {
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

fn panel_certificate(deployment: &RuntimeDeployment, at_time: DateTime<Utc>) -> PanelCertificateV1 {
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
        reconciled_at: at_time,
    }
}

fn awaiting_execution() -> RuntimeExecutionReceiptV1 {
    let mut execution =
        transition_to_phase(RuntimeSuspensionSourcePhaseV2::ReconcilingPanels, false);
    let mut deployment = RuntimeDeployment::restore(execution.snapshot).unwrap();
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
    execution.snapshot = deployment.snapshot();
    execution
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
    let fencing_token = FencingToken::new(4).unwrap();
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

fn request_for(
    execution: &RuntimeExecutionReceiptV1,
    recorded_at: DateTime<Utc>,
) -> RuntimeSuspendAttemptRequestV2 {
    let source_phase =
        RuntimeSuspensionSourcePhaseV2::from_deployment_phase(&execution.snapshot.phase).unwrap();
    RuntimeSuspendAttemptRequestV2 {
        suspension_id: RuntimeSuspensionIdV2::parse(SUSPENSION_ID).unwrap(),
        action_id: RuntimeSessionActionIdV1::new(non_zero(1)),
        guard: RuntimeExecutionGuardV1 {
            scope: RuntimeDeploymentScopeV1::from_identity(&execution.snapshot.identity),
            expected_revision: execution.snapshot.revision,
            controller_id: execution.controller_id.clone(),
            fencing_token: execution.fencing_token,
            runtime_generation: execution.snapshot.runtime_generation,
            convergence_attempt: execution.convergence_attempt,
        },
        source_phase,
        failure: failure(recorded_at),
        disposition: RuntimeAttemptDispositionV2::Blocked,
        checkpoint: source_phase.required_checkpoint(),
        local_effect: RuntimeLocalRouteEffectV2::None,
        drain_obligation: RuntimeDrainObligationV2::None,
    }
}

fn canonical_for(execution: &RuntimeExecutionReceiptV1) -> RuntimeCanonicalSuspendAttemptV2 {
    RuntimeCanonicalSuspendAttemptV2::new(request_for(execution, evidence_floor(execution)))
        .unwrap()
}

fn operation_for(execution: &RuntimeExecutionReceiptV1) -> RuntimeSuspendAttemptOperationV2 {
    RuntimeSuspendAttemptOperationV2::new(execution, canonical_for(execution)).unwrap()
}

fn build_error(
    execution: &RuntimeExecutionReceiptV1,
    request: RuntimeSuspendAttemptRequestV2,
) -> RuntimeSuspendAttemptOperationBuildErrorV2 {
    RuntimeSuspendAttemptOperationV2::new(
        execution,
        RuntimeCanonicalSuspendAttemptV2::new(request).unwrap(),
    )
    .unwrap_err()
}

fn exact_route(
    execution: &RuntimeExecutionReceiptV1,
    target: RuntimeDeploymentTargetV1,
) -> RuntimeExactLocalRouteIdentityV2 {
    RuntimeExactLocalRouteIdentityV2 {
        identity: RuntimeProcessIdentityV1 {
            target,
            runtime_generation: execution.snapshot.runtime_generation,
            process_instance_id: ProcessInstanceId::parse("process:current").unwrap(),
        },
        controller_fencing_token: execution.fencing_token,
        route_incarnation: non_zero(4),
    }
}

fn previous_lease(process: RuntimeProcessIdentityV1) -> RuntimePreviousServingLeaseIdentityV1 {
    RuntimePreviousServingLeaseIdentityV1 {
        scope: RuntimeDeploymentScopeV1 {
            tenant_id: TenantId::parse("tenant:1").unwrap(),
            installation_id: InstallationId::parse("installation:1").unwrap(),
            deployment_id: DeploymentId::parse("deployment:previous").unwrap(),
        },
        attestation_id: RuntimeAttestationIdV1::parse("d".repeat(64)).unwrap(),
        process,
        lease_epoch: non_zero(7),
        revision: non_zero(8),
    }
}

fn ordinary_provenance() -> RuntimeRouteMutationProvenanceV2 {
    RuntimeRouteMutationProvenanceV2::Ordinary {
        barrier_id: RuntimeBarrierIdV1::parse("ffeeddccbbaa99887766554433221100").unwrap(),
        pause: RuntimeBarrierPauseWitnessV2 {
            coordinator_generation: non_zero(9),
            connection_epoch: non_zero(10),
            paused_admission_revision: non_zero(11),
            pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(12)),
        },
    }
}

fn assert_request_mismatch(
    execution: &RuntimeExecutionReceiptV1,
    request: RuntimeSuspendAttemptRequestV2,
    field: RuntimeSuspendAttemptOperationFieldV2,
) {
    assert_eq!(
        build_error(execution, request),
        RuntimeSuspendAttemptOperationBuildErrorV2::RequestCorrelationMismatch { field }
    );
}

#[test]
fn all_suspendable_phases_bind_their_exact_source_evidence() {
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
        let operation = operation_for(&execution);

        assert_eq!(
            operation.operation_scope().scope(),
            &RuntimeDeploymentScopeV1::from_identity(&execution.snapshot.identity)
        );
        assert_eq!(
            operation.operation_scope().deployment_revision(),
            execution.snapshot.revision
        );
        assert_eq!(
            operation.operation_scope().convergence_attempt(),
            execution.convergence_attempt
        );
        assert_eq!(operation.source_target(), &execution.snapshot.target);
        assert_eq!(operation.source_previous_runtime(), None);
        assert_eq!(operation.source_evidence_at(), evidence_floor(&execution));
        assert_eq!(
            operation.canonical_attempt().request().source_phase,
            source_phase
        );
        assert_eq!(
            operation.canonical_attempt().request().checkpoint,
            source_phase.required_checkpoint()
        );
    }
}

#[test]
fn invalid_and_non_suspendable_executions_are_rejected_before_operation_creation() {
    let mut invalid = transition_to_phase(RuntimeSuspensionSourcePhaseV2::PreflightReady, false);
    invalid.snapshot.preflight = None;
    let canonical = canonical_for(&transition_to_phase(
        RuntimeSuspensionSourcePhaseV2::PreflightReady,
        false,
    ));
    assert_eq!(
        RuntimeSuspendAttemptOperationV2::new(&invalid, canonical),
        Err(RuntimeSuspendAttemptOperationBuildErrorV2::InvalidExecutionReceipt)
    );

    let awaiting = awaiting_execution();
    let scope_error =
        super::RuntimeSuspendAttemptOperationScopeV2::from_suspendable_execution(&awaiting)
            .unwrap_err();
    let lookup_error =
        RuntimeSuspendAttemptScopeLookupV2::from_suspendable_execution(&awaiting).unwrap_err();
    assert_eq!(
        scope_error,
        RuntimeSuspendAttemptOperationBuildErrorV2::SourcePhaseNotSuspendable
    );
    assert_eq!(
        lookup_error,
        RuntimeSuspendAttemptOperationBuildErrorV2::SourcePhaseNotSuspendable
    );
}

#[test]
fn execution_receipt_and_guard_drift_are_rejected() {
    let valid = transition_to_phase(RuntimeSuspensionSourcePhaseV2::Requested, false);
    let canonical = canonical_for(&valid);
    let mut variants = Vec::new();

    let mut value = valid.clone();
    value.controller_id = ControllerId::parse("controller:other").unwrap();
    variants.push(value);

    let mut value = valid.clone();
    value.fencing_token = FencingToken::new(4).unwrap();
    variants.push(value);

    let mut value = valid.clone();
    value.acquired_at = at(3);
    variants.push(value);

    let mut value = valid.clone();
    value.expires_at = at(99);
    variants.push(value);

    let mut value = valid.clone();
    value.expires_at = value.acquired_at;
    variants.push(value);

    let mut value = valid.clone();
    value.snapshot.controller_lease = None;
    variants.push(value);

    let mut value = valid.clone();
    value.snapshot.controller_lease.as_mut().unwrap().expires_at = at(99);
    variants.push(value);

    let mut value = valid;
    value.snapshot.last_fencing_token = Some(FencingToken::new(4).unwrap());
    variants.push(value);

    for execution in variants {
        assert_eq!(
            RuntimeSuspendAttemptOperationV2::new(&execution, canonical.clone()),
            Err(RuntimeSuspendAttemptOperationBuildErrorV2::InvalidExecutionReceipt)
        );
    }
}

#[test]
fn every_guard_field_is_correlated_to_the_execution_receipt() {
    let execution = transition_to_phase(RuntimeSuspensionSourcePhaseV2::Requested, false);
    let base = request_for(&execution, evidence_floor(&execution));

    let mut request = base.clone();
    request.guard.scope.tenant_id = TenantId::parse("tenant:other").unwrap();
    assert_request_mismatch(
        &execution,
        request,
        RuntimeSuspendAttemptOperationFieldV2::Scope,
    );

    let mut request = base.clone();
    request.guard.expected_revision = request.guard.expected_revision.next().unwrap();
    assert_request_mismatch(
        &execution,
        request,
        RuntimeSuspendAttemptOperationFieldV2::DeploymentRevision,
    );

    let mut request = base.clone();
    request.guard.convergence_attempt = NonZeroU32::new(6).unwrap();
    assert_request_mismatch(
        &execution,
        request,
        RuntimeSuspendAttemptOperationFieldV2::ConvergenceAttempt,
    );

    let mut request = base.clone();
    request.guard.controller_id = ControllerId::parse("controller:other").unwrap();
    assert_request_mismatch(
        &execution,
        request,
        RuntimeSuspendAttemptOperationFieldV2::ControllerId,
    );

    let mut request = base.clone();
    request.guard.fencing_token = FencingToken::new(4).unwrap();
    assert_request_mismatch(
        &execution,
        request,
        RuntimeSuspendAttemptOperationFieldV2::FencingToken,
    );

    let mut request = base;
    request.guard.runtime_generation = RuntimeGeneration::new(10).unwrap();
    assert_request_mismatch(
        &execution,
        request,
        RuntimeSuspendAttemptOperationFieldV2::RuntimeGeneration,
    );
}

#[test]
fn source_phase_is_correlated_even_when_its_checkpoint_is_self_consistent() {
    let execution = transition_to_phase(RuntimeSuspensionSourcePhaseV2::Requested, false);
    let mut request = request_for(&execution, evidence_floor(&execution));
    request.source_phase = RuntimeSuspensionSourcePhaseV2::PreflightReady;
    request.checkpoint = RuntimeResumeCheckpointV2::RequestDrain;

    assert_request_mismatch(
        &execution,
        request,
        RuntimeSuspendAttemptOperationFieldV2::SourcePhase,
    );
}

#[test]
fn current_target_and_previous_runtime_drift_are_rejected() {
    let execution = transition_to_phase(RuntimeSuspensionSourcePhaseV2::Requested, false);
    let base = request_for(&execution, evidence_floor(&execution));
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
        let route = exact_route(&execution, target);
        let mut request = base.clone();
        request.local_effect = RuntimeLocalRouteEffectV2::ExactRoute {
            route: route.clone(),
            lifecycle: RuntimeSuspendedRouteLifecycleV2::Staged,
        };
        request.drain_obligation = RuntimeDrainObligationV2::ExactLocalRoute(route);
        assert_request_mismatch(
            &execution,
            request,
            RuntimeSuspendAttemptOperationFieldV2::CurrentTarget,
        );
    }
}

#[test]
fn exact_local_route_matching_the_full_target_is_accepted() {
    let execution = transition_to_phase(RuntimeSuspensionSourcePhaseV2::Requested, false);
    let route = exact_route(&execution, execution.snapshot.target.clone());
    let mut request = request_for(&execution, evidence_floor(&execution));
    request.local_effect = RuntimeLocalRouteEffectV2::ExactRoute {
        route: route.clone(),
        lifecycle: RuntimeSuspendedRouteLifecycleV2::Draining,
    };
    request.drain_obligation = RuntimeDrainObligationV2::ExactLocalRoute(route);

    assert!(RuntimeSuspendAttemptOperationV2::new(
        &execution,
        RuntimeCanonicalSuspendAttemptV2::new(request).unwrap(),
    )
    .is_ok());
}

#[test]
fn route_absence_correlates_its_slot_and_optional_expected_target() {
    let execution = transition_to_phase(RuntimeSuspensionSourcePhaseV2::Requested, false);
    let base = request_for(&execution, evidence_floor(&execution));

    let mut request = base.clone();
    request.local_effect = RuntimeLocalRouteEffectV2::RouteAbsent {
        slot: RuntimeServingSlotV2::new(GuildId(8), execution.snapshot.target.ruleset_key.clone()),
        expected_route: None,
        provenance: ordinary_provenance(),
        observed_sequence: non_zero(13),
    };
    assert_request_mismatch(
        &execution,
        request,
        RuntimeSuspendAttemptOperationFieldV2::CurrentTarget,
    );

    let mut request = base.clone();
    request.local_effect = RuntimeLocalRouteEffectV2::RouteAbsent {
        slot: RuntimeServingSlotV2::new(
            execution.snapshot.target.guild_id,
            RuleSetKey::parse("other").unwrap(),
        ),
        expected_route: None,
        provenance: ordinary_provenance(),
        observed_sequence: non_zero(13),
    };
    assert_request_mismatch(
        &execution,
        request,
        RuntimeSuspendAttemptOperationFieldV2::CurrentTarget,
    );

    let mut expected_targets = Vec::new();
    let mut value = execution.snapshot.target.clone();
    value.version = RuleSetVersionId::new(2).unwrap();
    expected_targets.push(value);
    let mut value = execution.snapshot.target.clone();
    value.content_hash = RuleSetContentHash::parse_hex(&"c".repeat(64)).unwrap();
    expected_targets.push(value);
    let mut value = execution.snapshot.target.clone();
    value.binding_revision = BindingRevision::new(4).unwrap();
    expected_targets.push(value);
    let mut value = execution.snapshot.target.clone();
    value.binding_fingerprint = ResourceBindingFingerprint::parse(&"d".repeat(64)).unwrap();
    expected_targets.push(value);

    for expected_target in expected_targets {
        let expected_route = exact_route(&execution, expected_target);
        let mut request = base.clone();
        request.local_effect = RuntimeLocalRouteEffectV2::RouteAbsent {
            slot: RuntimeServingSlotV2::from_target(&execution.snapshot.target),
            expected_route: Some(expected_route),
            provenance: ordinary_provenance(),
            observed_sequence: non_zero(13),
        };
        assert_request_mismatch(
            &execution,
            request,
            RuntimeSuspendAttemptOperationFieldV2::CurrentTarget,
        );
    }

    let expected_route = exact_route(&execution, execution.snapshot.target.clone());
    for expected_route in [None, Some(expected_route)] {
        let mut request = base.clone();
        request.local_effect = RuntimeLocalRouteEffectV2::RouteAbsent {
            slot: RuntimeServingSlotV2::from_target(&execution.snapshot.target),
            expected_route,
            provenance: ordinary_provenance(),
            observed_sequence: non_zero(13),
        };
        assert!(RuntimeSuspendAttemptOperationV2::new(
            &execution,
            RuntimeCanonicalSuspendAttemptV2::new(request).unwrap(),
        )
        .is_ok());
    }
}

#[test]
fn represented_previous_runtime_must_match_its_source_process_exactly() {
    let execution = transition_to_phase(RuntimeSuspensionSourcePhaseV2::Requested, true);
    let source_previous = execution.snapshot.previous_runtime.clone().unwrap();
    let base = request_for(&execution, evidence_floor(&execution));

    let mut request = base.clone();
    request.drain_obligation =
        RuntimeDrainObligationV2::PreviousServing(previous_lease(source_previous.clone()));
    assert!(RuntimeSuspendAttemptOperationV2::new(
        &execution,
        RuntimeCanonicalSuspendAttemptV2::new(request).unwrap(),
    )
    .is_ok());

    let mut variants = Vec::new();
    let mut value = source_previous.clone();
    value.process_instance_id = ProcessInstanceId::parse("process:other").unwrap();
    variants.push(value);
    let mut value = source_previous.clone();
    value.runtime_generation = RuntimeGeneration::new(7).unwrap();
    variants.push(value);
    let mut value = source_previous.clone();
    value.target.version = RuleSetVersionId::new(2).unwrap();
    variants.push(value);
    let mut value = source_previous.clone();
    value.target.content_hash = RuleSetContentHash::parse_hex(&"c".repeat(64)).unwrap();
    variants.push(value);
    let mut value = source_previous.clone();
    value.target.binding_revision = BindingRevision::new(4).unwrap();
    variants.push(value);
    let mut value = source_previous;
    value.target.binding_fingerprint = ResourceBindingFingerprint::parse(&"d".repeat(64)).unwrap();
    variants.push(value);

    for previous in variants {
        let mut request = base.clone();
        request.drain_obligation =
            RuntimeDrainObligationV2::PreviousServing(previous_lease(previous));
        assert_request_mismatch(
            &execution,
            request,
            RuntimeSuspendAttemptOperationFieldV2::PreviousRuntime,
        );
    }

    let without_previous = transition_to_phase(RuntimeSuspensionSourcePhaseV2::Requested, false);
    let mut request = request_for(&without_previous, evidence_floor(&without_previous));
    request.drain_obligation =
        RuntimeDrainObligationV2::PreviousServing(previous_lease(previous_process()));
    assert_request_mismatch(
        &without_previous,
        request,
        RuntimeSuspendAttemptOperationFieldV2::PreviousRuntime,
    );
}

#[test]
fn an_omitted_previous_obligation_may_preserve_source_previous_runtime() {
    let execution = transition_to_phase(RuntimeSuspensionSourcePhaseV2::Requested, true);
    let operation = operation_for(&execution);

    assert_eq!(
        operation.source_previous_runtime(),
        execution.snapshot.previous_runtime.as_ref()
    );
    assert_eq!(
        operation.canonical_attempt().request().drain_obligation,
        RuntimeDrainObligationV2::None
    );
}

#[test]
fn each_source_phase_accepts_its_evidence_floor_and_rejects_one_microsecond_before_it() {
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
        let floor = evidence_floor(&execution);
        let operation = RuntimeSuspendAttemptOperationV2::new(
            &execution,
            RuntimeCanonicalSuspendAttemptV2::new(request_for(&execution, floor)).unwrap(),
        )
        .unwrap();
        assert_eq!(operation.source_evidence_at(), floor);

        let too_early = request_for(&execution, floor - Duration::microseconds(1));
        assert_request_mismatch(
            &execution,
            too_early,
            RuntimeSuspendAttemptOperationFieldV2::FailureRecordedAt,
        );
    }
}

#[test]
fn recovered_live_history_raises_the_runtime_pending_evidence_floor() {
    let execution = recovered_live_execution();
    assert_eq!(
        execution.snapshot.phase,
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition: automation_runtime_convergence::RuntimePendingConditionV1::Ready,
        }
    );
    assert_eq!(
        execution
            .snapshot
            .last_live_recovery
            .as_ref()
            .unwrap()
            .recovered_at,
        at(13)
    );

    let operation = RuntimeSuspendAttemptOperationV2::new(
        &execution,
        RuntimeCanonicalSuspendAttemptV2::new(request_for(&execution, at(13))).unwrap(),
    )
    .unwrap();
    assert_eq!(operation.source_evidence_at(), at(13));

    assert_request_mismatch(
        &execution,
        request_for(&execution, at(13) - Duration::microseconds(1)),
        RuntimeSuspendAttemptOperationFieldV2::FailureRecordedAt,
    );
}

fn persisted_from(
    operation: &RuntimeSuspendAttemptOperationV2,
) -> Result<RuntimePersistedSuspendAttemptRootV2, RuntimeSuspendAttemptOperationPersistenceErrorV2>
{
    RuntimePersistedSuspendAttemptRootV2::from_persisted(
        operation.operation_scope().scope().clone(),
        operation.operation_scope().deployment_revision(),
        operation.operation_scope().convergence_attempt(),
        operation.suspension_id(),
        operation.suspend_attempt_request_bytes(),
        operation.suspend_attempt_digest(),
    )
}

#[test]
fn persisted_root_requires_exact_normalized_scope_and_identity() {
    let execution = transition_to_phase(RuntimeSuspensionSourcePhaseV2::Requested, false);
    let operation = operation_for(&execution);
    assert!(persisted_from(&operation).is_ok());

    let mut other_scope = operation.operation_scope().scope().clone();
    other_scope.deployment_id = DeploymentId::parse("deployment:other").unwrap();
    assert_eq!(
        RuntimePersistedSuspendAttemptRootV2::from_persisted(
            other_scope,
            operation.operation_scope().deployment_revision(),
            operation.operation_scope().convergence_attempt(),
            operation.suspension_id(),
            operation.suspend_attempt_request_bytes(),
            operation.suspend_attempt_digest(),
        ),
        Err(
            RuntimeSuspendAttemptOperationPersistenceErrorV2::PersistedCorrelationMismatch {
                field: RuntimeSuspendAttemptOperationFieldV2::Scope,
            }
        )
    );

    assert_eq!(
        RuntimePersistedSuspendAttemptRootV2::from_persisted(
            operation.operation_scope().scope().clone(),
            operation
                .operation_scope()
                .deployment_revision()
                .next()
                .unwrap(),
            operation.operation_scope().convergence_attempt(),
            operation.suspension_id(),
            operation.suspend_attempt_request_bytes(),
            operation.suspend_attempt_digest(),
        ),
        Err(
            RuntimeSuspendAttemptOperationPersistenceErrorV2::PersistedCorrelationMismatch {
                field: RuntimeSuspendAttemptOperationFieldV2::DeploymentRevision,
            }
        )
    );

    assert_eq!(
        RuntimePersistedSuspendAttemptRootV2::from_persisted(
            operation.operation_scope().scope().clone(),
            operation.operation_scope().deployment_revision(),
            NonZeroU32::new(6).unwrap(),
            operation.suspension_id(),
            operation.suspend_attempt_request_bytes(),
            operation.suspend_attempt_digest(),
        ),
        Err(
            RuntimeSuspendAttemptOperationPersistenceErrorV2::PersistedCorrelationMismatch {
                field: RuntimeSuspendAttemptOperationFieldV2::ConvergenceAttempt,
            }
        )
    );

    assert_eq!(
        RuntimePersistedSuspendAttemptRootV2::from_persisted(
            operation.operation_scope().scope().clone(),
            operation.operation_scope().deployment_revision(),
            operation.operation_scope().convergence_attempt(),
            &RuntimeSuspensionIdV2::parse(OTHER_SUSPENSION_ID).unwrap(),
            operation.suspend_attempt_request_bytes(),
            operation.suspend_attempt_digest(),
        ),
        Err(
            RuntimeSuspendAttemptOperationPersistenceErrorV2::PersistedCorrelationMismatch {
                field: RuntimeSuspendAttemptOperationFieldV2::SuspensionId,
            }
        )
    );
}

#[test]
fn persisted_root_rejects_digest_and_byte_tampering() {
    let execution = transition_to_phase(RuntimeSuspensionSourcePhaseV2::Requested, false);
    let operation = operation_for(&execution);
    let wrong_digest = RuntimeSuspendAttemptDigestV2::parse("0".repeat(64)).unwrap();

    assert!(matches!(
        RuntimePersistedSuspendAttemptRootV2::from_persisted(
            operation.operation_scope().scope().clone(),
            operation.operation_scope().deployment_revision(),
            operation.operation_scope().convergence_attempt(),
            operation.suspension_id(),
            operation.suspend_attempt_request_bytes(),
            &wrong_digest,
        ),
        Err(RuntimeSuspendAttemptOperationPersistenceErrorV2::Canonical(
            _
        ))
    ));

    let mut tampered = operation.suspend_attempt_request_bytes().to_vec();
    let index = tampered.iter().position(|byte| *byte == b'd').unwrap();
    tampered[index] = b'e';
    assert!(matches!(
        RuntimePersistedSuspendAttemptRootV2::from_persisted(
            operation.operation_scope().scope().clone(),
            operation.operation_scope().deployment_revision(),
            operation.operation_scope().convergence_attempt(),
            operation.suspension_id(),
            &tampered,
            operation.suspend_attempt_digest(),
        ),
        Err(RuntimeSuspendAttemptOperationPersistenceErrorV2::Canonical(
            _
        ))
    ));
}

#[test]
fn byte_exact_replay_rejects_every_creation_identity_difference() {
    let execution = transition_to_phase(RuntimeSuspensionSourcePhaseV2::Requested, false);
    let operation = operation_for(&execution);
    let persisted = persisted_from(&operation).unwrap();
    let restored_canonical = RuntimeCanonicalSuspendAttemptV2::from_persisted(
        operation.suspend_attempt_request_bytes(),
        operation.suspend_attempt_digest(),
    )
    .unwrap();
    let exact = RuntimeSuspendAttemptOperationV2::new(&execution, restored_canonical).unwrap();
    assert_eq!(persisted.require_byte_exact_replay(&exact), Ok(()));

    let other_scope_execution = requested_execution_for("deployment:other");
    let other_scope = operation_for(&other_scope_execution);
    assert_eq!(
        persisted.require_byte_exact_replay(&other_scope),
        Err(RuntimeSuspendAttemptReplayErrorV2::CreationMismatch)
    );

    let other_revision_execution =
        transition_to_phase(RuntimeSuspensionSourcePhaseV2::PreflightReady, false);
    let other_revision = operation_for(&other_revision_execution);
    assert_eq!(
        persisted.require_byte_exact_replay(&other_revision),
        Err(RuntimeSuspendAttemptReplayErrorV2::CreationMismatch)
    );

    let mut other_attempt_execution = execution.clone();
    other_attempt_execution.convergence_attempt = NonZeroU32::new(6).unwrap();
    let other_attempt = operation_for(&other_attempt_execution);
    assert_eq!(
        persisted.require_byte_exact_replay(&other_attempt),
        Err(RuntimeSuspendAttemptReplayErrorV2::CreationMismatch)
    );

    let mut other_suspension_request = request_for(&execution, evidence_floor(&execution));
    other_suspension_request.suspension_id =
        RuntimeSuspensionIdV2::parse(OTHER_SUSPENSION_ID).unwrap();
    let other_suspension = RuntimeSuspendAttemptOperationV2::new(
        &execution,
        RuntimeCanonicalSuspendAttemptV2::new(other_suspension_request).unwrap(),
    )
    .unwrap();
    assert_eq!(
        persisted.require_byte_exact_replay(&other_suspension),
        Err(RuntimeSuspendAttemptReplayErrorV2::CreationMismatch)
    );

    let mut other_bytes_request = request_for(&execution, evidence_floor(&execution));
    other_bytes_request.action_id = RuntimeSessionActionIdV1::new(non_zero(2));
    let other_bytes = RuntimeSuspendAttemptOperationV2::new(
        &execution,
        RuntimeCanonicalSuspendAttemptV2::new(other_bytes_request).unwrap(),
    )
    .unwrap();
    assert_eq!(
        persisted.require_byte_exact_replay(&other_bytes),
        Err(RuntimeSuspendAttemptReplayErrorV2::CreationMismatch)
    );
}

#[test]
fn scope_lookup_projects_only_the_natural_execution_scope() {
    let execution = transition_to_phase(RuntimeSuspensionSourcePhaseV2::ActivationApplying, true);
    let lookup =
        RuntimeSuspendAttemptScopeLookupV2::from_suspendable_execution(&execution).unwrap();

    assert_eq!(
        lookup.operation_scope().scope(),
        &RuntimeDeploymentScopeV1::from_identity(&execution.snapshot.identity)
    );
    assert_eq!(
        lookup.operation_scope().deployment_revision(),
        execution.snapshot.revision
    );
    assert_eq!(
        lookup.operation_scope().convergence_attempt(),
        execution.convergence_attempt
    );
}
