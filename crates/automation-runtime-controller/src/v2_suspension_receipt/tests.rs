use std::num::{NonZeroU32, NonZeroU64};
use std::time::Duration;

use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    ActivationRequestId, BindingRevision, ControllerId, ControllerLeaseV1, DeploymentId,
    DeploymentRevision, FencingToken, InstallationId, LeaseRequestV1, ProcessInstanceId,
    PromotionId, RuntimeDeployment, RuntimeDeploymentIdentityV1, RuntimeDeploymentSnapshotV1,
    RuntimeDeploymentTargetV1, RuntimeFailureId, RuntimeFailureKindV1, RuntimeFailureV1,
    RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use chrono::{DateTime, TimeDelta, Utc};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;

use super::{
    RuntimeSuspendAttemptMutationOutcomeV2, RuntimeSuspendAttemptReceiptErrorV2,
    RuntimeSuspendAttemptReceiptV2,
};
use crate::{
    RuntimeAttemptDispositionV2, RuntimeAttestationIdV1, RuntimeBarrierIdV1,
    RuntimeBarrierPauseWitnessV2, RuntimeCanonicalSuspendAttemptV2, RuntimeDeploymentScopeV1,
    RuntimeDrainObligationV2, RuntimeExactLocalRouteIdentityV2, RuntimeExecutionGuardV1,
    RuntimeExecutionReceiptV1, RuntimeGatewayAdmissionSequenceV2, RuntimeLocalRouteEffectV2,
    RuntimePersistedSuspendAttemptRootV2, RuntimePreviousServingLeaseIdentityV1,
    RuntimeResumeCheckpointV2, RuntimeResumeSuspendedAttemptV2, RuntimeRouteMutationProvenanceV2,
    RuntimeSessionActionIdV1, RuntimeSuspendAttemptDrainProgressV2,
    RuntimeSuspendAttemptOperationV2, RuntimeSuspendAttemptRequestV2,
    RuntimeSuspendAttemptResumeBasisV2, RuntimeSuspendAttemptResumeGateV2,
    RuntimeSuspendedAttemptObservationV2, RuntimeSuspendedAttemptV2,
    RuntimeSuspendedRouteLifecycleV2, RuntimeSuspensionIdV2, RuntimeSuspensionSourcePhaseV2,
};

const SUSPENSION_ID: &str = "00112233445566778899aabbccddeeff";
const FOREIGN_SUSPENSION_ID: &str = "ffeeddccbbaa99887766554433221100";

#[derive(Clone)]
struct Fixture {
    execution: RuntimeExecutionReceiptV1,
    operation: RuntimeSuspendAttemptOperationV2,
    suspended: RuntimeSuspendedAttemptV2,
}

fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn at(second: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(second, 0).unwrap()
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

fn previous_lease(execution: &RuntimeExecutionReceiptV1) -> RuntimePreviousServingLeaseIdentityV1 {
    RuntimePreviousServingLeaseIdentityV1 {
        scope: RuntimeDeploymentScopeV1 {
            tenant_id: execution.snapshot.identity.tenant_id.clone(),
            installation_id: execution.snapshot.identity.installation_id.clone(),
            deployment_id: DeploymentId::parse("deployment:previous").unwrap(),
        },
        attestation_id: RuntimeAttestationIdV1::parse("d".repeat(64)).unwrap(),
        process: execution.snapshot.previous_runtime.clone().unwrap(),
        lease_epoch: non_zero(7),
        revision: non_zero(8),
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

fn provenance(seed: u128) -> RuntimeRouteMutationProvenanceV2 {
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

fn fixture(
    suspension_id: &str,
    exact_local: bool,
    with_previous: bool,
    deployment_revision: u64,
    convergence_attempt: u32,
    fencing_token: u64,
) -> Fixture {
    fixture_with_disposition(
        suspension_id,
        exact_local,
        with_previous,
        deployment_revision,
        convergence_attempt,
        fencing_token,
        RuntimeAttemptDispositionV2::Blocked,
    )
}

fn fixture_with_disposition(
    suspension_id: &str,
    exact_local: bool,
    with_previous: bool,
    deployment_revision: u64,
    convergence_attempt: u32,
    fencing_token: u64,
    disposition: RuntimeAttemptDispositionV2,
) -> Fixture {
    let mut deployment = RuntimeDeployment::request(
        RuntimeDeploymentIdentityV1 {
            deployment_id: DeploymentId::parse("deployment:current").unwrap(),
            tenant_id: TenantId::parse("tenant:1").unwrap(),
            installation_id: InstallationId::parse("installation:1").unwrap(),
            promotion_id: PromotionId::parse("9".repeat(64)).unwrap(),
            activation_request_id: ActivationRequestId::parse("activation:1").unwrap(),
        },
        target(),
        RuntimeGeneration::new(9).unwrap(),
        with_previous.then(previous_process),
        at(1),
    )
    .unwrap();
    let controller_id = ControllerId::parse("controller:source").unwrap();
    deployment
        .acquire_lease(LeaseRequestV1 {
            expected_revision: deployment.revision(),
            controller_id: controller_id.clone(),
            fencing_token: FencingToken::new(5).unwrap(),
            now: at(2),
            expires_at: at(100),
        })
        .unwrap();
    let fencing_token = FencingToken::new(fencing_token).unwrap();
    let mut snapshot = deployment.snapshot();
    snapshot.revision = DeploymentRevision::new(deployment_revision).unwrap();
    snapshot.last_fencing_token = Some(fencing_token);
    snapshot.controller_lease.as_mut().unwrap().fencing_token = fencing_token;
    let execution = RuntimeExecutionReceiptV1 {
        snapshot,
        controller_id: controller_id.clone(),
        fencing_token,
        convergence_attempt: NonZeroU32::new(convergence_attempt).unwrap(),
        acquired_at: at(2),
        expires_at: at(100),
    };
    let (local_effect, drain_obligation) = if exact_local {
        let route = local_route(&execution);
        let obligation = if with_previous {
            RuntimeDrainObligationV2::LocalAndPrevious {
                local: route.clone(),
                previous: previous_lease(&execution),
            }
        } else {
            RuntimeDrainObligationV2::ExactLocalRoute(route.clone())
        };
        (
            RuntimeLocalRouteEffectV2::ExactRoute {
                route,
                lifecycle: RuntimeSuspendedRouteLifecycleV2::Staged,
            },
            obligation,
        )
    } else {
        (
            RuntimeLocalRouteEffectV2::None,
            if with_previous {
                RuntimeDrainObligationV2::PreviousServing(previous_lease(&execution))
            } else {
                RuntimeDrainObligationV2::None
            },
        )
    };
    let request = RuntimeSuspendAttemptRequestV2 {
        suspension_id: RuntimeSuspensionIdV2::parse(suspension_id).unwrap(),
        action_id: RuntimeSessionActionIdV1::new(non_zero(9)),
        guard: RuntimeExecutionGuardV1 {
            scope: RuntimeDeploymentScopeV1::from_identity(&execution.snapshot.identity),
            expected_revision: execution.snapshot.revision,
            controller_id,
            fencing_token,
            runtime_generation: execution.snapshot.runtime_generation,
            convergence_attempt: execution.convergence_attempt,
        },
        source_phase: RuntimeSuspensionSourcePhaseV2::Requested,
        failure: RuntimeFailureV1 {
            failure_id: RuntimeFailureId::parse("failure:1").unwrap(),
            kind: RuntimeFailureKindV1::EnvironmentUnavailable,
            code: "dependency_unavailable".to_string(),
            message: "dependency unavailable".to_string(),
            recorded_at: at(3),
        },
        disposition,
        checkpoint: RuntimeResumeCheckpointV2::VerifyPreflight,
        local_effect,
        drain_obligation,
    };
    let operation = RuntimeSuspendAttemptOperationV2::new(
        &execution,
        RuntimeCanonicalSuspendAttemptV2::new(request).unwrap(),
    )
    .unwrap();
    let request = operation.canonical_attempt().request();
    let suspended = RuntimeSuspendedAttemptV2::from_inserted(
        &operation,
        non_zero(10),
        request.local_effect.clone(),
        request.drain_obligation.clone(),
        at(4),
    )
    .unwrap();
    Fixture {
        execution,
        operation,
        suspended,
    }
}

fn ordinary_fixture() -> Fixture {
    fixture(SUSPENSION_ID, false, false, 2, 4, 5)
}

fn root(operation: &RuntimeSuspendAttemptOperationV2) -> RuntimePersistedSuspendAttemptRootV2 {
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

fn observation(
    execution: &RuntimeExecutionReceiptV1,
    suspended: RuntimeSuspendedAttemptV2,
    release_lease: bool,
) -> RuntimeSuspendedAttemptObservationV2 {
    let mut snapshot = execution.snapshot.clone();
    if release_lease {
        snapshot.controller_lease = None;
    }
    RuntimeSuspendedAttemptObservationV2::new(snapshot, suspended).unwrap()
}

fn progressed_state(
    operation: &RuntimeSuspendAttemptOperationV2,
    progress: &RuntimeSuspendAttemptDrainProgressV2,
    sidecar_revision: u64,
    suspended_at: DateTime<Utc>,
) -> RuntimeSuspendedAttemptV2 {
    RuntimeSuspendedAttemptV2::from_persisted(
        &root(operation),
        non_zero(sidecar_revision),
        progress.replacement_local_effect().clone(),
        progress.replacement_drain_obligation().clone(),
        suspended_at,
    )
    .unwrap()
}

fn resume(fixture: &Fixture) -> RuntimeResumeSuspendedAttemptV2 {
    resume_with_gate(
        fixture,
        RuntimeSuspendAttemptResumeGateV2::RecoverBlocked {
            expected_failure_id: fixture.suspended.failure().failure_id.clone(),
        },
    )
}

fn resume_with_gate(
    fixture: &Fixture,
    gate: RuntimeSuspendAttemptResumeGateV2,
) -> RuntimeResumeSuspendedAttemptV2 {
    let locally_quiescent = observation(&fixture.execution, fixture.suspended.clone(), true)
        .into_locally_quiescent()
        .unwrap();
    let basis = RuntimeSuspendAttemptResumeBasisV2::new(
        locally_quiescent,
        fixture.execution.convergence_attempt,
        fixture.execution.controller_id.clone(),
        gate,
    )
    .unwrap();
    RuntimeResumeSuspendedAttemptV2::new(
        basis,
        ControllerId::parse("controller:successor").unwrap(),
        Duration::from_secs(90),
    )
    .unwrap()
}

fn successor(resume: &RuntimeResumeSuspendedAttemptV2) -> RuntimeExecutionReceiptV1 {
    let source = resume.basis().snapshot();
    let controller_id = resume.controller_id().clone();
    let fencing_token = resume
        .basis()
        .suspended()
        .source_guard()
        .fencing_token
        .next()
        .unwrap();
    let acquired_at = at(10);
    let expires_at = acquired_at + TimeDelta::seconds(90);
    let mut snapshot = source.clone();
    snapshot.revision = source.revision.next().unwrap();
    snapshot.last_fencing_token = Some(fencing_token);
    snapshot.controller_lease = Some(ControllerLeaseV1 {
        controller_id: controller_id.clone(),
        fencing_token,
        acquired_at,
        expires_at,
    });
    RuntimeExecutionReceiptV1 {
        snapshot,
        controller_id,
        fencing_token,
        convergence_attempt: NonZeroU32::new(
            resume
                .basis()
                .persisted_convergence_attempt()
                .get()
                .checked_add(1)
                .unwrap_or(1),
        )
        .unwrap(),
        acquired_at,
        expires_at,
    }
}

fn assert_present(
    receipt: &RuntimeSuspendAttemptReceiptV2,
    outcome: RuntimeSuspendAttemptMutationOutcomeV2,
    snapshot: &RuntimeDeploymentSnapshotV1,
    suspended: &RuntimeSuspendedAttemptV2,
) {
    assert_eq!(receipt.outcome(), outcome);
    assert_eq!(receipt.snapshot(), snapshot);
    assert_eq!(receipt.suspended(), Some(suspended));
    assert_eq!(receipt.successor_execution(), None);
}

#[test]
fn inserted_receipt_requires_the_exact_initial_root_and_presence_shape() {
    let fixture = ordinary_fixture();
    let observed = observation(&fixture.execution, fixture.suspended.clone(), false);
    let expected_snapshot = observed.snapshot().clone();
    let receipt = RuntimeSuspendAttemptReceiptV2::inserted(&fixture.operation, observed).unwrap();
    assert_present(
        &receipt,
        RuntimeSuspendAttemptMutationOutcomeV2::Inserted,
        &expected_snapshot,
        &fixture.suspended,
    );

    let exact = self::fixture(SUSPENSION_ID, true, false, 2, 4, 5);
    let progress = RuntimeSuspendAttemptDrainProgressV2::record_local_absent(
        exact.suspended.clone(),
        provenance(1),
        non_zero(40),
    )
    .unwrap();
    let progressed = progressed_state(&exact.operation, &progress, 11, at(4));
    let observed = observation(&exact.execution, progressed, false);
    assert_eq!(
        RuntimeSuspendAttemptReceiptV2::inserted(&exact.operation, observed),
        Err(RuntimeSuspendAttemptReceiptErrorV2::InitialStateMismatch)
    );
}

#[test]
fn replay_accepts_initial_and_progressed_states_but_rejects_a_foreign_operation() {
    let fixture = fixture(SUSPENSION_ID, true, false, 2, 4, 5);
    let initial = observation(&fixture.execution, fixture.suspended.clone(), false);
    let initial_snapshot = initial.snapshot().clone();
    let initial_receipt =
        RuntimeSuspendAttemptReceiptV2::replayed(&fixture.operation, initial).unwrap();
    assert_present(
        &initial_receipt,
        RuntimeSuspendAttemptMutationOutcomeV2::Replayed,
        &initial_snapshot,
        &fixture.suspended,
    );

    let progress = RuntimeSuspendAttemptDrainProgressV2::record_local_absent(
        fixture.suspended.clone(),
        provenance(2),
        non_zero(41),
    )
    .unwrap();
    let progressed = progressed_state(&fixture.operation, &progress, 12, at(4));
    let observed = observation(&fixture.execution, progressed.clone(), false);
    let progressed_snapshot = observed.snapshot().clone();
    let progressed_receipt =
        RuntimeSuspendAttemptReceiptV2::replayed(&fixture.operation, observed).unwrap();
    assert_present(
        &progressed_receipt,
        RuntimeSuspendAttemptMutationOutcomeV2::Replayed,
        &progressed_snapshot,
        &progressed,
    );

    let foreign = self::fixture(FOREIGN_SUSPENSION_ID, true, false, 2, 4, 5);
    let observed = observation(&fixture.execution, fixture.suspended, false);
    assert_eq!(
        RuntimeSuspendAttemptReceiptV2::replayed(&foreign.operation, observed),
        Err(RuntimeSuspendAttemptReceiptErrorV2::OperationMismatch)
    );
}

#[test]
fn drain_progress_requires_exact_root_replacement_time_and_successor_revision() {
    let fixture = fixture(SUSPENSION_ID, true, false, 2, 4, 5);
    let progress = RuntimeSuspendAttemptDrainProgressV2::record_local_absent(
        fixture.suspended.clone(),
        provenance(3),
        non_zero(42),
    )
    .unwrap();

    let same_revision = progressed_state(&fixture.operation, &progress, 10, at(4));
    assert_eq!(
        RuntimeSuspendAttemptReceiptV2::drain_progressed(
            &progress,
            observation(&fixture.execution, same_revision, false),
        ),
        Err(RuntimeSuspendAttemptReceiptErrorV2::SidecarRevisionMismatch)
    );

    let advanced = progressed_state(&fixture.operation, &progress, 11, at(4));
    let observed = observation(&fixture.execution, advanced.clone(), false);
    let expected_snapshot = observed.snapshot().clone();
    let receipt = RuntimeSuspendAttemptReceiptV2::drain_progressed(&progress, observed).unwrap();
    assert_present(
        &receipt,
        RuntimeSuspendAttemptMutationOutcomeV2::DrainProgressed,
        &expected_snapshot,
        &advanced,
    );

    let skipped_revision = progressed_state(&fixture.operation, &progress, 12, at(4));
    assert_eq!(
        RuntimeSuspendAttemptReceiptV2::drain_progressed(
            &progress,
            observation(&fixture.execution, skipped_revision, false),
        ),
        Err(RuntimeSuspendAttemptReceiptErrorV2::SidecarRevisionMismatch)
    );

    let changed_time = progressed_state(&fixture.operation, &progress, 12, at(5));
    assert_eq!(
        RuntimeSuspendAttemptReceiptV2::drain_progressed(
            &progress,
            observation(&fixture.execution, changed_time, false),
        ),
        Err(RuntimeSuspendAttemptReceiptErrorV2::DrainProgressMismatch)
    );

    let changed_progress = RuntimeSuspendAttemptDrainProgressV2::record_local_absent(
        fixture.suspended.clone(),
        provenance(4),
        non_zero(43),
    )
    .unwrap();
    let changed_replacement = progressed_state(&fixture.operation, &changed_progress, 12, at(4));
    assert_eq!(
        RuntimeSuspendAttemptReceiptV2::drain_progressed(
            &progress,
            observation(&fixture.execution, changed_replacement, false),
        ),
        Err(RuntimeSuspendAttemptReceiptErrorV2::DrainProgressMismatch)
    );

    let foreign = self::fixture(FOREIGN_SUSPENSION_ID, true, false, 2, 4, 5);
    let foreign_result = progressed_state(&foreign.operation, &progress, 12, at(4));
    assert_eq!(
        RuntimeSuspendAttemptReceiptV2::drain_progressed(
            &progress,
            observation(&foreign.execution, foreign_result, false),
        ),
        Err(RuntimeSuspendAttemptReceiptErrorV2::DrainProgressMismatch)
    );
}

#[test]
fn drain_progress_preserves_the_exact_previous_serving_obligation() {
    let fixture = fixture(SUSPENSION_ID, true, true, 2, 4, 5);
    let progress = RuntimeSuspendAttemptDrainProgressV2::record_local_absent(
        fixture.suspended,
        provenance(5),
        non_zero(44),
    )
    .unwrap();
    assert!(matches!(
        progress.replacement_drain_obligation(),
        RuntimeDrainObligationV2::PreviousServing(_)
    ));
    let progressed = progressed_state(&fixture.operation, &progress, 11, at(4));
    let receipt = RuntimeSuspendAttemptReceiptV2::drain_progressed(
        &progress,
        observation(&fixture.execution, progressed.clone(), false),
    )
    .unwrap();
    assert_eq!(
        receipt.outcome(),
        RuntimeSuspendAttemptMutationOutcomeV2::DrainProgressed
    );
    assert!(matches!(
        receipt.suspended().unwrap().drain_obligation(),
        RuntimeDrainObligationV2::PreviousServing(_)
    ));
    assert_eq!(receipt.suspended(), Some(&progressed));
    assert_eq!(receipt.successor_execution(), None);
}

#[test]
fn resumed_receipt_proves_the_exact_atomic_successor_and_presence_shape() {
    let fixture = fixture(SUSPENSION_ID, false, true, 2, 4, 5);
    let resume = resume(&fixture);
    let successor = successor(&resume);
    let receipt = RuntimeSuspendAttemptReceiptV2::resumed(&resume, successor.clone()).unwrap();
    assert_eq!(
        receipt.outcome(),
        RuntimeSuspendAttemptMutationOutcomeV2::Resumed
    );
    assert_eq!(receipt.snapshot(), &successor.snapshot);
    assert_eq!(receipt.suspended(), None);
    assert_eq!(receipt.successor_execution(), Some(&successor));
    assert_eq!(
        receipt.snapshot().previous_runtime,
        fixture.execution.snapshot.previous_runtime
    );
    assert!(matches!(
        fixture.suspended.drain_obligation(),
        RuntimeDrainObligationV2::PreviousServing(_)
    ));
}

#[test]
fn resumed_receipt_rejects_each_successor_drift() {
    let fixture = ordinary_fixture();
    let resume = resume(&fixture);

    let mut wrong_revision = successor(&resume);
    wrong_revision.snapshot.revision = resume.basis().snapshot().revision;
    assert_eq!(
        RuntimeSuspendAttemptReceiptV2::resumed(&resume, wrong_revision),
        Err(RuntimeSuspendAttemptReceiptErrorV2::SuccessorRevisionMismatch)
    );

    let mut wrong_attempt = successor(&resume);
    wrong_attempt.convergence_attempt = fixture.execution.convergence_attempt;
    assert_eq!(
        RuntimeSuspendAttemptReceiptV2::resumed(&resume, wrong_attempt),
        Err(RuntimeSuspendAttemptReceiptErrorV2::SuccessorAttemptMismatch)
    );

    let mut wrong_controller = successor(&resume);
    let controller = ControllerId::parse("controller:foreign").unwrap();
    wrong_controller.controller_id = controller.clone();
    wrong_controller
        .snapshot
        .controller_lease
        .as_mut()
        .unwrap()
        .controller_id = controller;
    assert_eq!(
        RuntimeSuspendAttemptReceiptV2::resumed(&resume, wrong_controller),
        Err(RuntimeSuspendAttemptReceiptErrorV2::SuccessorControllerMismatch)
    );

    let mut wrong_fence = successor(&resume);
    let fencing_token = wrong_fence.fencing_token.next().unwrap();
    wrong_fence.fencing_token = fencing_token;
    wrong_fence.snapshot.last_fencing_token = Some(fencing_token);
    wrong_fence
        .snapshot
        .controller_lease
        .as_mut()
        .unwrap()
        .fencing_token = fencing_token;
    assert_eq!(
        RuntimeSuspendAttemptReceiptV2::resumed(&resume, wrong_fence),
        Err(RuntimeSuspendAttemptReceiptErrorV2::SuccessorFenceMismatch)
    );

    let mut wrong_duration = successor(&resume);
    wrong_duration.expires_at += TimeDelta::seconds(1);
    wrong_duration
        .snapshot
        .controller_lease
        .as_mut()
        .unwrap()
        .expires_at = wrong_duration.expires_at;
    assert_eq!(
        RuntimeSuspendAttemptReceiptV2::resumed(&resume, wrong_duration),
        Err(RuntimeSuspendAttemptReceiptErrorV2::SuccessorLeaseMismatch)
    );

    let mut wrong_snapshot_lease = successor(&resume);
    wrong_snapshot_lease
        .snapshot
        .controller_lease
        .as_mut()
        .unwrap()
        .acquired_at += TimeDelta::microseconds(1);
    assert_eq!(
        RuntimeSuspendAttemptReceiptV2::resumed(&resume, wrong_snapshot_lease),
        Err(RuntimeSuspendAttemptReceiptErrorV2::SuccessorLeaseMismatch)
    );

    let mut changed_truth = successor(&resume);
    changed_truth.snapshot.target.binding_revision = BindingRevision::new(4).unwrap();
    assert_eq!(
        RuntimeSuspendAttemptReceiptV2::resumed(&resume, changed_truth),
        Err(RuntimeSuspendAttemptReceiptErrorV2::PreservedTruthMismatch)
    );
}

#[test]
fn resumed_receipt_rejects_attempt_and_database_successor_overflow() {
    let attempt_max = fixture(SUSPENSION_ID, false, false, 2, u32::MAX, 5);
    let resume_attempt = resume(&attempt_max);
    assert_eq!(
        RuntimeSuspendAttemptReceiptV2::resumed(&resume_attempt, successor(&resume_attempt),),
        Err(RuntimeSuspendAttemptReceiptErrorV2::SuccessorAttemptMismatch)
    );

    let revision_max = fixture(SUSPENSION_ID, false, false, i64::MAX as u64, 4, 5);
    let resume_revision = resume(&revision_max);
    assert_eq!(
        RuntimeSuspendAttemptReceiptV2::resumed(&resume_revision, successor(&resume_revision),),
        Err(RuntimeSuspendAttemptReceiptErrorV2::SuccessorRevisionMismatch)
    );

    let fence_max = fixture(SUSPENSION_ID, false, false, 2, 4, i64::MAX as u64);
    let resume_fence = resume(&fence_max);
    assert_eq!(
        RuntimeSuspendAttemptReceiptV2::resumed(&resume_fence, successor(&resume_fence)),
        Err(RuntimeSuspendAttemptReceiptErrorV2::SuccessorFenceMismatch)
    );
}

#[test]
fn resumed_receipt_rejects_noncanonical_submicrosecond_times() {
    let fixture = ordinary_fixture();
    let resume = resume(&fixture);
    let mut successor = successor(&resume);
    let acquired_at = DateTime::from_timestamp(10, 1).unwrap();
    let expires_at = acquired_at + TimeDelta::seconds(90);
    successor.acquired_at = acquired_at;
    successor.expires_at = expires_at;
    let lease = successor.snapshot.controller_lease.as_mut().unwrap();
    lease.acquired_at = acquired_at;
    lease.expires_at = expires_at;

    assert_eq!(
        RuntimeSuspendAttemptReceiptV2::resumed(&resume, successor),
        Err(RuntimeSuspendAttemptReceiptErrorV2::InvalidSuccessor)
    );
}

#[test]
fn resumed_receipt_rechecks_the_durable_retry_floor() {
    let retry_not_before = at(20);
    let fixture = fixture_with_disposition(
        SUSPENSION_ID,
        false,
        false,
        2,
        4,
        5,
        RuntimeAttemptDispositionV2::Retryable { retry_not_before },
    );
    let resume = resume_with_gate(
        &fixture,
        RuntimeSuspendAttemptResumeGateV2::Retryable {
            database_observed_at: retry_not_before,
        },
    );
    assert_eq!(
        RuntimeSuspendAttemptReceiptV2::resumed(&resume, successor(&resume)),
        Err(RuntimeSuspendAttemptReceiptErrorV2::SuccessorGateMismatch)
    );

    let mut ready = successor(&resume);
    ready.acquired_at = retry_not_before;
    ready.expires_at = retry_not_before + TimeDelta::seconds(90);
    let lease = ready.snapshot.controller_lease.as_mut().unwrap();
    lease.acquired_at = ready.acquired_at;
    lease.expires_at = ready.expires_at;
    assert!(RuntimeSuspendAttemptReceiptV2::resumed(&resume, ready).is_ok());
}
