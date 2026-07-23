use std::num::{NonZeroU32, NonZeroU64};
use std::time::Duration;

use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    ActivationRequestId, BindingRevision, ControllerId, DeploymentId, FencingToken, InstallationId,
    LeaseRequestV1, ProcessInstanceId, PromotionId, RuntimeDeployment, RuntimeDeploymentIdentityV1,
    RuntimeDeploymentTargetV1, RuntimeFailureId, RuntimeFailureKindV1, RuntimeFailureV1,
    RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use chrono::{DateTime, TimeDelta, Utc};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;

use super::{
    RuntimeResumeSuspendedAttemptErrorV2, RuntimeResumeSuspendedAttemptV2,
    RuntimeSuspendAttemptResumeBasisErrorV2, RuntimeSuspendAttemptResumeBasisV2,
    RuntimeSuspendAttemptResumeGateV2,
};
use crate::{
    RuntimeAttemptDispositionV2, RuntimeAttestationIdV1, RuntimeCanonicalSuspendAttemptV2,
    RuntimeDeploymentScopeV1, RuntimeDrainObligationV2, RuntimeExecutionGuardV1,
    RuntimeExecutionReceiptV1, RuntimeLocalRouteEffectV2,
    RuntimeLocallyQuiescentSuspendedAttemptV2, RuntimePreviousServingLeaseIdentityV1,
    RuntimeResumeCheckpointV2, RuntimeSessionActionIdV1, RuntimeSuspendAttemptOperationV2,
    RuntimeSuspendAttemptRequestV2, RuntimeSuspendedAttemptObservationV2,
    RuntimeSuspendedAttemptV2, RuntimeSuspensionIdV2, RuntimeSuspensionSourcePhaseV2,
};

const SUSPENSION_ID: &str = "00112233445566778899aabbccddeeff";

#[derive(Clone)]
struct ResumeFixture {
    locally_quiescent: RuntimeLocallyQuiescentSuspendedAttemptV2,
    convergence_attempt: NonZeroU32,
    last_controller_id: ControllerId,
    failure_id: RuntimeFailureId,
}

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

fn previous_process() -> RuntimeProcessIdentityV1 {
    RuntimeProcessIdentityV1 {
        target: target(),
        runtime_generation: RuntimeGeneration::new(8).unwrap(),
        process_instance_id: ProcessInstanceId::parse("process:previous").unwrap(),
    }
}

fn previous_lease(
    snapshot: &automation_runtime_convergence::RuntimeDeploymentSnapshotV1,
) -> RuntimePreviousServingLeaseIdentityV1 {
    RuntimePreviousServingLeaseIdentityV1 {
        scope: RuntimeDeploymentScopeV1 {
            tenant_id: snapshot.identity.tenant_id.clone(),
            installation_id: snapshot.identity.installation_id.clone(),
            deployment_id: DeploymentId::parse("deployment:previous").unwrap(),
        },
        attestation_id: RuntimeAttestationIdV1::parse("d".repeat(64)).unwrap(),
        process: snapshot.previous_runtime.clone().unwrap(),
        lease_epoch: non_zero(7),
        revision: non_zero(8),
    }
}

fn fixture(disposition: RuntimeAttemptDispositionV2, with_previous: bool) -> ResumeFixture {
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
    let convergence_attempt = NonZeroU32::new(4).unwrap();
    let execution = RuntimeExecutionReceiptV1 {
        snapshot: deployment.snapshot(),
        controller_id: controller_id.clone(),
        fencing_token,
        convergence_attempt,
        acquired_at: at(2),
        expires_at: at(100),
    };
    let failure_id = RuntimeFailureId::parse("failure:1").unwrap();
    let drain_obligation = if with_previous {
        RuntimeDrainObligationV2::PreviousServing(previous_lease(&execution.snapshot))
    } else {
        RuntimeDrainObligationV2::None
    };
    let request = RuntimeSuspendAttemptRequestV2 {
        suspension_id: RuntimeSuspensionIdV2::parse(SUSPENSION_ID).unwrap(),
        action_id: RuntimeSessionActionIdV1::new(non_zero(9)),
        guard: RuntimeExecutionGuardV1 {
            scope: RuntimeDeploymentScopeV1::from_identity(&execution.snapshot.identity),
            expected_revision: execution.snapshot.revision,
            controller_id: controller_id.clone(),
            fencing_token,
            runtime_generation: execution.snapshot.runtime_generation,
            convergence_attempt,
        },
        source_phase: RuntimeSuspensionSourcePhaseV2::Requested,
        failure: RuntimeFailureV1 {
            failure_id: failure_id.clone(),
            kind: RuntimeFailureKindV1::EnvironmentUnavailable,
            code: "dependency_unavailable".to_string(),
            message: "dependency unavailable".to_string(),
            recorded_at: at(3),
        },
        disposition,
        checkpoint: RuntimeResumeCheckpointV2::VerifyPreflight,
        local_effect: RuntimeLocalRouteEffectV2::None,
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
    let mut snapshot = execution.snapshot;
    snapshot.controller_lease = None;
    let locally_quiescent = RuntimeSuspendedAttemptObservationV2::new(snapshot, suspended)
        .unwrap()
        .into_locally_quiescent()
        .unwrap();
    ResumeFixture {
        locally_quiescent,
        convergence_attempt,
        last_controller_id: controller_id,
        failure_id,
    }
}

fn basis(
    fixture: &ResumeFixture,
    gate: RuntimeSuspendAttemptResumeGateV2,
) -> Result<RuntimeSuspendAttemptResumeBasisV2, RuntimeSuspendAttemptResumeBasisErrorV2> {
    RuntimeSuspendAttemptResumeBasisV2::new(
        fixture.locally_quiescent.clone(),
        fixture.convergence_attempt,
        fixture.last_controller_id.clone(),
        gate,
    )
}

#[test]
fn retry_readiness_uses_the_exact_database_time_floor_without_a_host_clock() {
    let retry_not_before = at_microseconds(20_000_000);
    let fixture = fixture(
        RuntimeAttemptDispositionV2::Retryable { retry_not_before },
        false,
    );

    assert_eq!(
        basis(
            &fixture,
            RuntimeSuspendAttemptResumeGateV2::Retryable {
                database_observed_at: retry_not_before - TimeDelta::microseconds(1),
            },
        ),
        Err(RuntimeSuspendAttemptResumeBasisErrorV2::RetryNotReady)
    );

    for database_observed_at in [
        retry_not_before,
        retry_not_before + TimeDelta::microseconds(1),
    ] {
        let gate = RuntimeSuspendAttemptResumeGateV2::Retryable {
            database_observed_at,
        };
        let resume_basis = basis(&fixture, gate.clone()).unwrap();
        assert_eq!(resume_basis.gate(), &gate);
        assert_eq!(
            resume_basis.persisted_convergence_attempt(),
            fixture.convergence_attempt
        );
        assert_eq!(
            resume_basis.persisted_last_controller_id(),
            &fixture.last_controller_id
        );
    }

    let earlier_retry = at(3);
    let rollback_fixture = self::fixture(
        RuntimeAttemptDispositionV2::Retryable {
            retry_not_before: earlier_retry,
        },
        false,
    );
    assert!(basis(
        &rollback_fixture,
        RuntimeSuspendAttemptResumeGateV2::Retryable {
            database_observed_at: earlier_retry,
        },
    )
    .is_ok());
}

#[test]
fn retry_database_observation_must_be_canonical_microsecond_time() {
    let retry_not_before = at(20);
    let fixture = fixture(
        RuntimeAttemptDispositionV2::Retryable { retry_not_before },
        false,
    );
    let database_observed_at = DateTime::from_timestamp(20, 1).unwrap();

    assert_eq!(
        basis(
            &fixture,
            RuntimeSuspendAttemptResumeGateV2::Retryable {
                database_observed_at,
            },
        ),
        Err(RuntimeSuspendAttemptResumeBasisErrorV2::InvalidDatabaseObservation)
    );
}

#[test]
fn blocked_resume_requires_the_exact_failure_id_and_matching_disposition() {
    let blocked = fixture(RuntimeAttemptDispositionV2::Blocked, false);
    let exact_gate = RuntimeSuspendAttemptResumeGateV2::RecoverBlocked {
        expected_failure_id: blocked.failure_id.clone(),
    };
    let resume_basis = basis(&blocked, exact_gate.clone()).unwrap();
    assert_eq!(resume_basis.gate(), &exact_gate);

    assert_eq!(
        basis(
            &blocked,
            RuntimeSuspendAttemptResumeGateV2::RecoverBlocked {
                expected_failure_id: RuntimeFailureId::parse("failure:other").unwrap(),
            },
        ),
        Err(RuntimeSuspendAttemptResumeBasisErrorV2::FailureIdMismatch)
    );
    assert_eq!(
        basis(
            &blocked,
            RuntimeSuspendAttemptResumeGateV2::Retryable {
                database_observed_at: at(20),
            },
        ),
        Err(RuntimeSuspendAttemptResumeBasisErrorV2::DispositionMismatch)
    );

    let retryable = fixture(
        RuntimeAttemptDispositionV2::Retryable {
            retry_not_before: at(20),
        },
        false,
    );
    assert_eq!(
        basis(
            &retryable,
            RuntimeSuspendAttemptResumeGateV2::RecoverBlocked {
                expected_failure_id: retryable.failure_id.clone(),
            },
        ),
        Err(RuntimeSuspendAttemptResumeBasisErrorV2::DispositionMismatch)
    );
}

#[test]
fn durable_attempt_and_last_controller_must_match_the_suspension_source() {
    let fixture = fixture(RuntimeAttemptDispositionV2::Blocked, false);
    let gate = RuntimeSuspendAttemptResumeGateV2::RecoverBlocked {
        expected_failure_id: fixture.failure_id.clone(),
    };

    assert_eq!(
        RuntimeSuspendAttemptResumeBasisV2::new(
            fixture.locally_quiescent.clone(),
            NonZeroU32::new(fixture.convergence_attempt.get() + 1).unwrap(),
            fixture.last_controller_id.clone(),
            gate.clone(),
        ),
        Err(RuntimeSuspendAttemptResumeBasisErrorV2::ConvergenceAttemptMismatch)
    );
    assert_eq!(
        RuntimeSuspendAttemptResumeBasisV2::new(
            fixture.locally_quiescent.clone(),
            fixture.convergence_attempt,
            ControllerId::parse("controller:other").unwrap(),
            gate,
        ),
        Err(RuntimeSuspendAttemptResumeBasisErrorV2::LastControllerMismatch)
    );
}

#[test]
fn local_quiescence_retains_an_exact_previous_serving_obligation() {
    let fixture = fixture(RuntimeAttemptDispositionV2::Blocked, true);
    assert!(fixture
        .locally_quiescent
        .snapshot()
        .controller_lease
        .is_none());
    assert!(fixture
        .locally_quiescent
        .snapshot()
        .previous_runtime
        .is_some());
    assert!(matches!(
        fixture.locally_quiescent.suspended().drain_obligation(),
        RuntimeDrainObligationV2::PreviousServing(_)
    ));

    let resume_basis = basis(
        &fixture,
        RuntimeSuspendAttemptResumeGateV2::RecoverBlocked {
            expected_failure_id: fixture.failure_id.clone(),
        },
    )
    .unwrap();
    assert_eq!(resume_basis.locally_quiescent(), &fixture.locally_quiescent);
    assert_eq!(
        resume_basis.suspended(),
        fixture.locally_quiescent.suspended()
    );
    assert_eq!(
        resume_basis.snapshot(),
        fixture.locally_quiescent.snapshot()
    );
    assert!(matches!(
        resume_basis.suspended().drain_obligation(),
        RuntimeDrainObligationV2::PreviousServing(_)
    ));
}

#[test]
fn resume_intent_rejects_zero_lease_and_preserves_valid_inputs() {
    let fixture = fixture(RuntimeAttemptDispositionV2::Blocked, false);
    let resume_basis = basis(
        &fixture,
        RuntimeSuspendAttemptResumeGateV2::RecoverBlocked {
            expected_failure_id: fixture.failure_id.clone(),
        },
    )
    .unwrap();
    let controller_id = ControllerId::parse("controller:successor").unwrap();

    for invalid in [
        Duration::ZERO,
        Duration::from_millis(999),
        Duration::from_secs(1) + Duration::from_nanos(1),
        Duration::from_millis(600_001),
    ] {
        assert_eq!(
            RuntimeResumeSuspendedAttemptV2::new(
                resume_basis.clone(),
                controller_id.clone(),
                invalid,
            ),
            Err(RuntimeResumeSuspendedAttemptErrorV2::InvalidLeaseDuration)
        );
    }

    for lease_for in [
        Duration::from_secs(1),
        Duration::from_secs(90),
        Duration::from_secs(600),
    ] {
        let resume = RuntimeResumeSuspendedAttemptV2::new(
            resume_basis.clone(),
            controller_id.clone(),
            lease_for,
        )
        .unwrap();
        assert_eq!(resume.basis(), &resume_basis);
        assert_eq!(resume.controller_id(), &controller_id);
        assert_eq!(resume.lease_for(), lease_for);
        assert_eq!(resume.lease_duration().get(), lease_for);
        assert_eq!(
            resume.lease_duration().milliseconds(),
            lease_for.as_millis() as u64
        );
    }
}
