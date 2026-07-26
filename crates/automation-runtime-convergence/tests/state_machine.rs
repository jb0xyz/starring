use std::num::NonZeroU32;

use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    ActivationAttestationV1, ActivationOutcomeKindV1, ActivationRequestId, BindingRevision,
    CommandGuardV1, ControllerId, DeploymentId, DrainAttestationV1, FencingToken,
    GatewayReadyAttestationV1, GatewayReadyKindV1, InstallationId, LeaseRequestV1, LiveLossKindV1,
    PanelCertificateId, PanelCertificateV1, PanelIneligibilityV1, PanelReportDigestV1,
    PreflightAttestationV1, ProcessInstanceId, PromotionId, RecoverBlockedRequestV1,
    RecoverLiveRequestV1, RuntimeDeployment, RuntimeDeploymentError, RuntimeDeploymentIdentityV1,
    RuntimeDeploymentPhaseKindV1, RuntimeDeploymentPhaseV1, RuntimeDeploymentTargetV1,
    RuntimeFailureId, RuntimeFailureKindV1, RuntimeFailureV1, RuntimeGeneration,
    RuntimeProcessIdentityV1, SupersedingDeploymentV1, TenantId, TransitionOutcomeV1,
};
use chrono::{DateTime, Duration, TimeZone, Utc};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;

fn at(second: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(1_800_000_000 + second, 0).unwrap()
}

fn target(version: u32, binding_revision: u64, fingerprint: char) -> RuntimeDeploymentTargetV1 {
    RuntimeDeploymentTargetV1 {
        guild_id: GuildId(42),
        ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
        version: RuleSetVersionId::new(version).unwrap(),
        content_hash: RuleSetContentHash::parse_hex(&fingerprint.to_string().repeat(64)).unwrap(),
        binding_revision: BindingRevision::new(binding_revision).unwrap(),
        binding_fingerprint: ResourceBindingFingerprint::parse(&fingerprint.to_string().repeat(64))
            .unwrap(),
    }
}

fn deployment_identity(deployment_id: &str, promotion: char) -> RuntimeDeploymentIdentityV1 {
    RuntimeDeploymentIdentityV1 {
        deployment_id: DeploymentId::parse(deployment_id).unwrap(),
        tenant_id: TenantId::parse("tenant-42").unwrap(),
        installation_id: InstallationId::parse("installation-42").unwrap(),
        promotion_id: PromotionId::parse(promotion.to_string().repeat(64)).unwrap(),
        activation_request_id: ActivationRequestId::parse("activation-2").unwrap(),
    }
}

struct Fixture {
    deployment: RuntimeDeployment,
    previous: RuntimeProcessIdentityV1,
    target: RuntimeDeploymentTargetV1,
    controller: ControllerId,
    token: FencingToken,
}

impl Fixture {
    fn new() -> Self {
        let previous = RuntimeProcessIdentityV1 {
            target: target(1, 1, 'a'),
            runtime_generation: RuntimeGeneration::new(1).unwrap(),
            process_instance_id: ProcessInstanceId::parse("process-old").unwrap(),
        };
        let target = target(2, 2, 'b');
        let mut deployment = RuntimeDeployment::request(
            deployment_identity("deployment-2", 'd'),
            target.clone(),
            RuntimeGeneration::new(2).unwrap(),
            Some(previous.clone()),
            at(0),
        )
        .unwrap();
        let controller = ControllerId::parse("controller-a").unwrap();
        let token = FencingToken::new(10).unwrap();
        deployment
            .acquire_lease(LeaseRequestV1 {
                expected_revision: deployment.revision(),
                controller_id: controller.clone(),
                fencing_token: token,
                now: at(1),
                expires_at: at(3_600),
            })
            .unwrap();
        Self {
            deployment,
            previous,
            target,
            controller,
            token,
        }
    }

    fn guard(&self, second: i64) -> CommandGuardV1 {
        CommandGuardV1 {
            expected_revision: self.deployment.revision(),
            controller_id: self.controller.clone(),
            fencing_token: self.token,
            runtime_generation: RuntimeGeneration::new(2).unwrap(),
            now: at(second),
        }
    }

    fn preflight(&self) -> PreflightAttestationV1 {
        PreflightAttestationV1 {
            target: self.target.clone(),
            runtime_generation: RuntimeGeneration::new(2).unwrap(),
            observed_runtime: Some(self.previous.clone()),
            checked_at: at(10),
        }
    }

    fn drain(&self) -> DrainAttestationV1 {
        DrainAttestationV1 {
            previous_runtime: Some(self.previous.clone()),
            target_runtime_generation: RuntimeGeneration::new(2).unwrap(),
            drained_at: at(20),
        }
    }

    fn activation(&self) -> ActivationAttestationV1 {
        ActivationAttestationV1 {
            activation_request_id: ActivationRequestId::parse("activation-2").unwrap(),
            target: self.target.clone(),
            runtime_generation: RuntimeGeneration::new(2).unwrap(),
            kind: ActivationOutcomeKindV1::Activated,
            activated_at: at(30),
        }
    }

    fn panel(&self) -> PanelCertificateV1 {
        PanelCertificateV1 {
            certificate_id: PanelCertificateId::parse("panels-2").unwrap(),
            report_digest: PanelReportDigestV1::parse("4".repeat(64)).unwrap(),
            target: self.target.clone(),
            runtime_generation: RuntimeGeneration::new(2).unwrap(),
            process_instance_id: ProcessInstanceId::parse("process-new").unwrap(),
            declared_count: 3,
            installed_count: 2,
            unchanged_count: 1,
            skipped_transient_count: 0,
            skipped_unresolved_channel_count: 0,
            failed_count: 0,
            ambiguous_outcome_count: 0,
            stale_message_cleanup_pending_count: 0,
            orphan_message_cleanup_pending_count: 0,
            reposted_old_message_cleanup_pending_count: 0,
            reconciled_at: at(40),
        }
    }

    fn ready(&self) -> GatewayReadyAttestationV1 {
        GatewayReadyAttestationV1 {
            target: self.target.clone(),
            runtime_generation: RuntimeGeneration::new(2).unwrap(),
            process_instance_id: ProcessInstanceId::parse("process-new").unwrap(),
            kind: GatewayReadyKindV1::DiscordReady,
            ready_at: at(50),
        }
    }

    fn advance_to_pending(&mut self) {
        self.deployment
            .accept_preflight(&self.guard(10), self.preflight())
            .unwrap();
        self.deployment.request_drain(&self.guard(11)).unwrap();
        self.deployment
            .accept_drain(&self.guard(20), self.drain())
            .unwrap();
        self.deployment.begin_activation(&self.guard(21)).unwrap();
        self.deployment
            .accept_activation(&self.guard(30), self.activation())
            .unwrap();
    }

    fn advance_to_awaiting_ready(&mut self) {
        self.advance_to_pending();
        self.deployment
            .begin_panel_reconciliation(&self.guard(31))
            .unwrap();
        self.deployment
            .accept_panel_certificate(&self.guard(40), self.panel())
            .unwrap();
    }
}

#[test]
fn exact_evidence_reaches_live_and_roundtrips() {
    let mut fixture = Fixture::new();
    fixture.advance_to_awaiting_ready();
    let outcome = fixture
        .deployment
        .certify_live(&fixture.guard(50), fixture.ready(), at(51))
        .unwrap();
    assert!(matches!(outcome, TransitionOutcomeV1::Applied { .. }));
    assert_eq!(
        fixture.deployment.phase().kind(),
        RuntimeDeploymentPhaseKindV1::Live
    );
    assert!(fixture.deployment.controller_lease().is_none());
    let live = fixture.deployment.live_attestation().unwrap();
    assert_eq!(live.target, fixture.target);
    assert_eq!(live.panel_certificate.declared_count, 3);
    assert_eq!(live.gateway_ready.kind, GatewayReadyKindV1::DiscordReady);
    let json = serde_json::to_string(&fixture.deployment.snapshot()).unwrap();
    let snapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(
        RuntimeDeployment::restore(snapshot).unwrap(),
        fixture.deployment
    );
}

#[test]
fn invalid_phase_skip_is_rejected() {
    let mut fixture = Fixture::new();
    assert_eq!(
        fixture
            .deployment
            .begin_activation(&fixture.guard(2))
            .unwrap_err(),
        RuntimeDeploymentError::InvalidTransition {
            current: RuntimeDeploymentPhaseKindV1::Requested,
            operation: "begin_activation",
        }
    );
}

#[test]
fn stale_revision_generation_and_lease_are_rejected() {
    let mut fixture = Fixture::new();
    let stale = fixture.guard(10);
    fixture
        .deployment
        .accept_preflight(&stale, fixture.preflight())
        .unwrap();
    assert!(matches!(
        fixture.deployment.request_drain(&stale),
        Err(RuntimeDeploymentError::RevisionConflict { .. })
    ));
    let mut stale_generation = fixture.guard(11);
    stale_generation.runtime_generation = RuntimeGeneration::new(1).unwrap();
    assert!(matches!(
        fixture.deployment.request_drain(&stale_generation),
        Err(RuntimeDeploymentError::RuntimeGenerationConflict { .. })
    ));
    let mut stale_lease = fixture.guard(11);
    stale_lease.fencing_token = FencingToken::new(9).unwrap();
    assert!(matches!(
        fixture.deployment.request_drain(&stale_lease),
        Err(RuntimeDeploymentError::FencingTokenConflict { .. })
    ));
}

#[test]
fn expired_lease_requires_a_higher_fencing_token() {
    let mut fixture = Fixture::new();
    let expired = fixture.guard(3_600);
    assert!(matches!(
        fixture
            .deployment
            .accept_preflight(&expired, fixture.preflight()),
        Err(RuntimeDeploymentError::LeaseExpired { .. })
    ));
    let revision = fixture.deployment.revision();
    assert_eq!(
        fixture
            .deployment
            .acquire_lease(LeaseRequestV1 {
                expected_revision: revision,
                controller_id: ControllerId::parse("controller-b").unwrap(),
                fencing_token: fixture.token,
                now: at(3_601),
                expires_at: at(7_200),
            })
            .unwrap_err(),
        RuntimeDeploymentError::FencingTokenNotMonotonic
    );
    fixture
        .deployment
        .acquire_lease(LeaseRequestV1 {
            expected_revision: revision,
            controller_id: ControllerId::parse("controller-b").unwrap(),
            fencing_token: FencingToken::new(11).unwrap(),
            now: at(3_601),
            expires_at: at(7_200),
        })
        .unwrap();
}

#[test]
fn mismatched_target_binding_and_baseline_are_rejected() {
    let mut fixture = Fixture::new();
    let mut wrong_target = fixture.preflight();
    wrong_target.target.binding_revision = BindingRevision::new(99).unwrap();
    assert_eq!(
        fixture
            .deployment
            .accept_preflight(&fixture.guard(10), wrong_target)
            .unwrap_err(),
        RuntimeDeploymentError::TargetMismatch
    );
    let mut wrong_previous = fixture.preflight();
    wrong_previous.observed_runtime = None;
    assert_eq!(
        fixture
            .deployment
            .accept_preflight(&fixture.guard(10), wrong_previous)
            .unwrap_err(),
        RuntimeDeploymentError::PreviousRuntimeMismatch
    );
}

#[test]
fn activation_must_match_the_bound_product_request() {
    let mut fixture = Fixture::new();
    fixture
        .deployment
        .accept_preflight(&fixture.guard(10), fixture.preflight())
        .unwrap();
    fixture
        .deployment
        .request_drain(&fixture.guard(11))
        .unwrap();
    fixture
        .deployment
        .accept_drain(&fixture.guard(20), fixture.drain())
        .unwrap();
    fixture
        .deployment
        .begin_activation(&fixture.guard(21))
        .unwrap();
    let mut activation = fixture.activation();
    activation.activation_request_id = ActivationRequestId::parse("activation-other").unwrap();
    assert_eq!(
        fixture
            .deployment
            .accept_activation(&fixture.guard(30), activation)
            .unwrap_err(),
        RuntimeDeploymentError::ActivationRequestMismatch
    );
}

#[test]
fn panel_skips_fail_closed() {
    let mut fixture = Fixture::new();
    fixture.advance_to_pending();
    fixture
        .deployment
        .begin_panel_reconciliation(&fixture.guard(31))
        .unwrap();
    let mut panel = fixture.panel();
    panel.installed_count = 1;
    panel.skipped_transient_count = 1;
    assert_eq!(
        fixture
            .deployment
            .accept_panel_certificate(&fixture.guard(40), panel)
            .unwrap_err(),
        RuntimeDeploymentError::PanelIneligible(PanelIneligibilityV1::TransientSkipped)
    );
    assert_eq!(
        fixture.deployment.phase().kind(),
        RuntimeDeploymentPhaseKindV1::ReconcilingPanels
    );
}

#[test]
fn panel_certificate_must_match_the_exact_binding() {
    let mut fixture = Fixture::new();
    fixture.advance_to_pending();
    fixture
        .deployment
        .begin_panel_reconciliation(&fixture.guard(31))
        .unwrap();
    let mut panel = fixture.panel();
    panel.target.binding_revision = BindingRevision::new(3).unwrap();
    assert_eq!(
        fixture
            .deployment
            .accept_panel_certificate(&fixture.guard(40), panel)
            .unwrap_err(),
        RuntimeDeploymentError::TargetMismatch
    );
}

#[test]
fn incomplete_panel_certificate_is_rejected() {
    let mut fixture = Fixture::new();
    fixture.advance_to_pending();
    fixture
        .deployment
        .begin_panel_reconciliation(&fixture.guard(31))
        .unwrap();
    let mut panel = fixture.panel();
    panel.unchanged_count = 0;
    assert_eq!(
        fixture
            .deployment
            .accept_panel_certificate(&fixture.guard(40), panel)
            .unwrap_err(),
        RuntimeDeploymentError::PanelIneligible(PanelIneligibilityV1::Incomplete)
    );
}

#[test]
fn ambiguous_and_pending_panel_cleanup_outcomes_fail_closed() {
    let cases = [
        PanelIneligibilityV1::AmbiguousOutcome,
        PanelIneligibilityV1::StaleCleanupPending,
        PanelIneligibilityV1::OrphanCleanupPending,
        PanelIneligibilityV1::RepostedOldMessageCleanupPending,
    ];
    for expected in cases {
        let mut fixture = Fixture::new();
        fixture.advance_to_pending();
        fixture
            .deployment
            .begin_panel_reconciliation(&fixture.guard(31))
            .unwrap();
        let mut panel = fixture.panel();
        match expected {
            PanelIneligibilityV1::AmbiguousOutcome => panel.ambiguous_outcome_count = 1,
            PanelIneligibilityV1::StaleCleanupPending => {
                panel.stale_message_cleanup_pending_count = 1
            }
            PanelIneligibilityV1::OrphanCleanupPending => {
                panel.orphan_message_cleanup_pending_count = 1
            }
            PanelIneligibilityV1::RepostedOldMessageCleanupPending => {
                panel.reposted_old_message_cleanup_pending_count = 1
            }
            _ => unreachable!(),
        }
        assert_eq!(
            fixture
                .deployment
                .accept_panel_certificate(&fixture.guard(40), panel)
                .unwrap_err(),
            RuntimeDeploymentError::PanelIneligible(expected)
        );
    }
}

#[test]
fn mismatched_process_is_rejected_and_resumed_gateway_is_accepted() {
    let mut fixture = Fixture::new();
    fixture.advance_to_awaiting_ready();
    let mut wrong_process = fixture.ready();
    wrong_process.process_instance_id = ProcessInstanceId::parse("other-process").unwrap();
    assert_eq!(
        fixture
            .deployment
            .certify_live(&fixture.guard(50), wrong_process, at(51))
            .unwrap_err(),
        RuntimeDeploymentError::ProcessInstanceMismatch
    );
    let mut resumed = fixture.ready();
    resumed.kind = GatewayReadyKindV1::DiscordResumed;
    fixture
        .deployment
        .certify_live(&fixture.guard(50), resumed, at(51))
        .unwrap();
    assert_eq!(
        fixture.deployment.phase().kind(),
        RuntimeDeploymentPhaseKindV1::Live
    );
}

#[test]
fn exact_replays_do_not_advance_revision() {
    let mut fixture = Fixture::new();
    let preflight = fixture.preflight();
    fixture
        .deployment
        .accept_preflight(&fixture.guard(10), preflight.clone())
        .unwrap();
    let revision = fixture.deployment.revision();
    let replay = fixture
        .deployment
        .accept_preflight(
            &CommandGuardV1 {
                expected_revision: automation_runtime_convergence::DeploymentRevision::FIRST,
                controller_id: fixture.controller.clone(),
                fencing_token: fixture.token,
                runtime_generation: RuntimeGeneration::new(2).unwrap(),
                now: at(10),
            },
            preflight,
        )
        .unwrap();
    assert_eq!(replay, TransitionOutcomeV1::Replayed { revision });
    assert_eq!(fixture.deployment.revision(), revision);
}

#[test]
fn live_certification_replay_is_idempotent_after_lease_release() {
    let mut fixture = Fixture::new();
    fixture.advance_to_awaiting_ready();
    let ready = fixture.ready();
    fixture
        .deployment
        .certify_live(&fixture.guard(50), ready.clone(), at(51))
        .unwrap();
    let revision = fixture.deployment.revision();
    let replay = fixture
        .deployment
        .certify_live(
            &CommandGuardV1 {
                expected_revision: automation_runtime_convergence::DeploymentRevision::FIRST,
                controller_id: fixture.controller.clone(),
                fencing_token: fixture.token,
                runtime_generation: RuntimeGeneration::new(2).unwrap(),
                now: at(52),
            },
            ready,
            at(51),
        )
        .unwrap();
    assert_eq!(replay, TransitionOutcomeV1::Replayed { revision });
}

#[test]
fn lost_live_evidence_returns_to_pending_and_roundtrips() {
    let mut fixture = Fixture::new();
    fixture.advance_to_awaiting_ready();
    fixture
        .deployment
        .certify_live(&fixture.guard(50), fixture.ready(), at(51))
        .unwrap();
    let expected_revision = fixture.deployment.revision();
    let request = RecoverLiveRequestV1 {
        expected_revision,
        expected_runtime_generation: RuntimeGeneration::new(2).unwrap(),
        expected_process_instance_id: ProcessInstanceId::parse("process-new").unwrap(),
        kind: LiveLossKindV1::ServingLeaseExpired,
        evidence_at: at(60),
        recovered_at: at(61),
    };
    let outcome = fixture.deployment.recover_live(request.clone()).unwrap();
    assert!(matches!(outcome, TransitionOutcomeV1::Applied { .. }));
    assert_eq!(
        fixture.deployment.phase().kind(),
        RuntimeDeploymentPhaseKindV1::RuntimePending
    );
    let snapshot = fixture.deployment.snapshot();
    assert!(snapshot.activation.is_some());
    assert!(snapshot.panel_certificate.is_none());
    assert!(snapshot.gateway_ready.is_none());
    assert!(snapshot.live.is_none());
    let recovery = fixture.deployment.last_live_recovery().unwrap();
    assert_eq!(
        recovery.prior_live.process_instance_id.as_str(),
        "process-new"
    );
    assert_eq!(recovery.kind, LiveLossKindV1::ServingLeaseExpired);
    let revision = fixture.deployment.revision();
    assert_eq!(
        fixture.deployment.recover_live(request).unwrap(),
        TransitionOutcomeV1::Replayed { revision }
    );
    assert_eq!(
        RuntimeDeployment::restore(fixture.deployment.snapshot()).unwrap(),
        fixture.deployment
    );
}

#[test]
fn live_recovery_requires_exact_identity_and_monotonic_database_evidence() {
    let cases = [
        (
            RuntimeGeneration::new(1).unwrap(),
            ProcessInstanceId::parse("process-new").unwrap(),
            at(60),
            at(61),
            RuntimeDeploymentError::RuntimeGenerationConflict {
                expected: RuntimeGeneration::new(2).unwrap(),
                actual: RuntimeGeneration::new(1).unwrap(),
            },
        ),
        (
            RuntimeGeneration::new(2).unwrap(),
            ProcessInstanceId::parse("process-other").unwrap(),
            at(60),
            at(61),
            RuntimeDeploymentError::ProcessInstanceMismatch,
        ),
        (
            RuntimeGeneration::new(2).unwrap(),
            ProcessInstanceId::parse("process-new").unwrap(),
            at(50),
            at(61),
            RuntimeDeploymentError::AttestationTimeRegression,
        ),
        (
            RuntimeGeneration::new(2).unwrap(),
            ProcessInstanceId::parse("process-new").unwrap(),
            at(62),
            at(61),
            RuntimeDeploymentError::AttestationTimeRegression,
        ),
    ];
    for (generation, process, evidence_at, recovered_at, expected) in cases {
        let mut fixture = Fixture::new();
        fixture.advance_to_awaiting_ready();
        fixture
            .deployment
            .certify_live(&fixture.guard(50), fixture.ready(), at(51))
            .unwrap();
        let error = fixture
            .deployment
            .recover_live(RecoverLiveRequestV1 {
                expected_revision: fixture.deployment.revision(),
                expected_runtime_generation: generation,
                expected_process_instance_id: process,
                kind: LiveLossKindV1::ServingDisconnected,
                evidence_at,
                recovered_at,
            })
            .unwrap_err();
        assert_eq!(error, expected);
    }
}

#[test]
fn recovery_fences_the_old_process_and_accepts_only_fresh_runtime_evidence() {
    let mut fixture = Fixture::new();
    fixture.advance_to_awaiting_ready();
    fixture
        .deployment
        .certify_live(&fixture.guard(50), fixture.ready(), at(51))
        .unwrap();
    fixture
        .deployment
        .recover_live(RecoverLiveRequestV1 {
            expected_revision: fixture.deployment.revision(),
            expected_runtime_generation: RuntimeGeneration::new(2).unwrap(),
            expected_process_instance_id: ProcessInstanceId::parse("process-new").unwrap(),
            kind: LiveLossKindV1::ServingDisconnected,
            evidence_at: at(60),
            recovered_at: at(61),
        })
        .unwrap();
    let revision = fixture.deployment.revision();
    assert_eq!(
        fixture
            .deployment
            .acquire_lease(LeaseRequestV1 {
                expected_revision: revision,
                controller_id: ControllerId::parse("controller-recovery").unwrap(),
                fencing_token: FencingToken::new(10).unwrap(),
                now: at(62),
                expires_at: at(120),
            })
            .unwrap_err(),
        RuntimeDeploymentError::FencingTokenNotMonotonic
    );
    fixture.controller = ControllerId::parse("controller-recovery").unwrap();
    fixture.token = FencingToken::new(11).unwrap();
    fixture
        .deployment
        .acquire_lease(LeaseRequestV1 {
            expected_revision: revision,
            controller_id: fixture.controller.clone(),
            fencing_token: fixture.token,
            now: at(62),
            expires_at: at(120),
        })
        .unwrap();
    fixture
        .deployment
        .begin_panel_reconciliation(&fixture.guard(63))
        .unwrap();
    let mut stale_panel = fixture.panel();
    stale_panel.reconciled_at = at(64);
    assert_eq!(
        fixture
            .deployment
            .accept_panel_certificate(&fixture.guard(64), stale_panel)
            .unwrap_err(),
        RuntimeDeploymentError::ProcessInstanceMismatch
    );
    let mut recovered_panel = fixture.panel();
    recovered_panel.certificate_id = PanelCertificateId::parse("panels-recovered").unwrap();
    recovered_panel.process_instance_id = ProcessInstanceId::parse("process-recovered").unwrap();
    recovered_panel.reconciled_at = at(64);
    fixture
        .deployment
        .accept_panel_certificate(&fixture.guard(64), recovered_panel)
        .unwrap();
    let mut recovered_ready = fixture.ready();
    recovered_ready.process_instance_id = ProcessInstanceId::parse("process-recovered").unwrap();
    recovered_ready.ready_at = at(65);
    fixture
        .deployment
        .certify_live(&fixture.guard(65), recovered_ready, at(66))
        .unwrap();
    assert_eq!(
        fixture
            .deployment
            .live_attestation()
            .unwrap()
            .process_instance_id
            .as_str(),
        "process-recovered"
    );
}

#[test]
fn corrupted_historical_live_recovery_is_rejected() {
    let mut fixture = Fixture::new();
    fixture.advance_to_awaiting_ready();
    fixture
        .deployment
        .certify_live(&fixture.guard(50), fixture.ready(), at(51))
        .unwrap();
    fixture
        .deployment
        .recover_live(RecoverLiveRequestV1 {
            expected_revision: fixture.deployment.revision(),
            expected_runtime_generation: RuntimeGeneration::new(2).unwrap(),
            expected_process_instance_id: ProcessInstanceId::parse("process-new").unwrap(),
            kind: LiveLossKindV1::ServingLeaseExpired,
            evidence_at: at(60),
            recovered_at: at(61),
        })
        .unwrap();
    let mut snapshot = fixture.deployment.snapshot();
    snapshot
        .last_live_recovery
        .as_mut()
        .unwrap()
        .prior_live
        .panel_certificate
        .failed_count = 1;
    assert_eq!(
        RuntimeDeployment::restore(snapshot).unwrap_err(),
        RuntimeDeploymentError::InvalidSnapshot
    );
}

#[test]
fn retryable_and_blocked_failures_require_explicit_resume() {
    let mut fixture = Fixture::new();
    fixture.advance_to_pending();
    let retryable = RuntimeFailureV1 {
        failure_id: RuntimeFailureId::parse("failure-1").unwrap(),
        kind: RuntimeFailureKindV1::GatewayReadyTimeout,
        code: "gateway_ready_timeout".to_string(),
        message: "gateway did not become ready".to_string(),
        recorded_at: at(35),
    };
    fixture
        .deployment
        .record_retryable_failure(
            &fixture.guard(35),
            retryable,
            NonZeroU32::new(1).unwrap(),
            at(60),
        )
        .unwrap();
    assert!(matches!(
        fixture.deployment.phase(),
        RuntimeDeploymentPhaseV1::RuntimePending { .. }
    ));
    assert!(fixture.deployment.controller_lease().is_none());
    fixture.token = FencingToken::new(11).unwrap();
    fixture
        .deployment
        .acquire_lease(LeaseRequestV1 {
            expected_revision: fixture.deployment.revision(),
            controller_id: fixture.controller.clone(),
            fencing_token: fixture.token,
            now: at(60),
            expires_at: at(3_600),
        })
        .unwrap();
    fixture
        .deployment
        .resume_runtime_pending(&fixture.guard(60))
        .unwrap();
    fixture
        .deployment
        .begin_panel_reconciliation(&fixture.guard(61))
        .unwrap();
    let blocked = RuntimeFailureV1 {
        failure_id: RuntimeFailureId::parse("failure-2").unwrap(),
        kind: RuntimeFailureKindV1::InvariantViolation,
        code: "panel_identity_drift".to_string(),
        message: "panel identity changed".to_string(),
        recorded_at: at(62),
    };
    fixture
        .deployment
        .record_blocked_failure(&fixture.guard(62), blocked)
        .unwrap();
    assert!(matches!(
        fixture.deployment.phase(),
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition: automation_runtime_convergence::RuntimePendingConditionV1::Blocked { .. }
        }
    ));
    assert!(fixture.deployment.controller_lease().is_none());
    let blocked_revision = fixture.deployment.revision();
    let blocked_recovery = RecoverBlockedRequestV1 {
        expected_revision: blocked_revision,
        expected_failure_id: RuntimeFailureId::parse("failure-2").unwrap(),
        controller_id: fixture.controller.clone(),
        fencing_token: FencingToken::new(12).unwrap(),
        now: at(63),
        expires_at: at(3_600),
    };
    assert!(matches!(
        fixture.deployment.acquire_lease(LeaseRequestV1 {
            expected_revision: blocked_revision,
            controller_id: fixture.controller.clone(),
            fencing_token: FencingToken::new(12).unwrap(),
            now: at(63),
            expires_at: at(3_600),
        }),
        Err(RuntimeDeploymentError::InvalidTransition {
            operation: "acquire_lease",
            ..
        })
    ));
    let blocked_retry = RuntimeFailureV1 {
        failure_id: RuntimeFailureId::parse("failure-3").unwrap(),
        kind: RuntimeFailureKindV1::EnvironmentUnavailable,
        code: "environment_unavailable".to_string(),
        message: "environment is unavailable".to_string(),
        recorded_at: at(63),
    };
    assert!(matches!(
        fixture.deployment.record_retryable_failure(
            &fixture.guard(63),
            blocked_retry,
            NonZeroU32::new(2).unwrap(),
            at(70),
        ),
        Err(RuntimeDeploymentError::LeaseRequired)
    ));
    assert!(matches!(
        fixture
            .deployment
            .recover_blocked(blocked_recovery.clone())
            .unwrap(),
        TransitionOutcomeV1::Applied { .. }
    ));
    assert!(matches!(
        fixture.deployment.phase(),
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition: automation_runtime_convergence::RuntimePendingConditionV1::Ready
        }
    ));
    assert!(matches!(
        fixture
            .deployment
            .recover_blocked(blocked_recovery)
            .unwrap(),
        TransitionOutcomeV1::Replayed { .. }
    ));
}

#[test]
fn old_runtime_generation_and_old_superseder_are_rejected() {
    let previous = RuntimeProcessIdentityV1 {
        target: target(2, 2, 'b'),
        runtime_generation: RuntimeGeneration::new(5).unwrap(),
        process_instance_id: ProcessInstanceId::parse("process-5").unwrap(),
    };
    assert_eq!(
        RuntimeDeployment::request(
            deployment_identity("deployment-5", 'e'),
            target(3, 3, 'c'),
            RuntimeGeneration::new(5).unwrap(),
            Some(previous),
            at(0),
        )
        .unwrap_err(),
        RuntimeDeploymentError::RuntimeGenerationNotMonotonic
    );
    let mut fixture = Fixture::new();
    let by = SupersedingDeploymentV1 {
        identity: deployment_identity("deployment-old", 'f'),
        target: target(3, 3, 'c'),
        runtime_generation: RuntimeGeneration::new(1).unwrap(),
    };
    assert_eq!(
        fixture
            .deployment
            .supersede(&fixture.guard(2), by, "new target".to_string(), at(2))
            .unwrap_err(),
        RuntimeDeploymentError::RuntimeGenerationNotMonotonic
    );
}

#[test]
fn corrupted_live_snapshot_is_rejected() {
    let fixture = Fixture::new();
    let mut snapshot = fixture.deployment.snapshot();
    snapshot.phase = RuntimeDeploymentPhaseV1::Live;
    snapshot.controller_lease = None;
    assert_eq!(
        RuntimeDeployment::restore(snapshot).unwrap_err(),
        RuntimeDeploymentError::InvalidSnapshot
    );
}

#[test]
fn cancellation_is_rejected_after_drain_is_accepted() {
    let mut fixture = Fixture::new();
    fixture
        .deployment
        .accept_preflight(&fixture.guard(10), fixture.preflight())
        .unwrap();
    fixture
        .deployment
        .request_drain(&fixture.guard(11))
        .unwrap();
    fixture
        .deployment
        .accept_drain(&fixture.guard(20), fixture.drain())
        .unwrap();
    assert_eq!(
        fixture
            .deployment
            .cancel(&fixture.guard(21), "too late".to_string(), at(21))
            .unwrap_err(),
        RuntimeDeploymentError::InvalidTransition {
            current: RuntimeDeploymentPhaseKindV1::Drained,
            operation: "cancel",
        }
    );
}

#[test]
fn promotion_identity_requires_lowercase_sha256_shape() {
    assert!(PromotionId::parse("a".repeat(64)).is_ok());
    assert!(PromotionId::parse("A".repeat(64)).is_err());
    assert!(PromotionId::parse("a".repeat(63)).is_err());
}

#[test]
fn active_lease_cannot_be_stolen() {
    let mut fixture = Fixture::new();
    let result = fixture.deployment.acquire_lease(LeaseRequestV1 {
        expected_revision: fixture.deployment.revision(),
        controller_id: ControllerId::parse("controller-b").unwrap(),
        fencing_token: FencingToken::new(11).unwrap(),
        now: at(2),
        expires_at: at(4_000),
    });
    assert!(matches!(
        result,
        Err(RuntimeDeploymentError::LeaseHeld { .. })
    ));
}

#[test]
fn lease_owner_can_renew_with_a_higher_fencing_token() {
    let mut fixture = Fixture::new();
    let outcome = fixture
        .deployment
        .acquire_lease(LeaseRequestV1 {
            expected_revision: fixture.deployment.revision(),
            controller_id: fixture.controller.clone(),
            fencing_token: FencingToken::new(11).unwrap(),
            now: at(2),
            expires_at: at(7_200),
        })
        .unwrap();
    assert!(matches!(outcome, TransitionOutcomeV1::Applied { .. }));
    let lease = fixture.deployment.controller_lease().unwrap();
    assert_eq!(lease.fencing_token, FencingToken::new(11).unwrap());
    assert_eq!(lease.expires_at, at(7_200));
}

#[test]
fn ready_attestation_cannot_predate_panel_certificate() {
    let mut fixture = Fixture::new();
    fixture.advance_to_awaiting_ready();
    let mut ready = fixture.ready();
    ready.ready_at = at(39);
    assert_eq!(
        fixture
            .deployment
            .certify_live(&fixture.guard(50), ready, at(51))
            .unwrap_err(),
        RuntimeDeploymentError::AttestationTimeRegression
    );
}

#[test]
fn lease_duration_uses_strict_expiry() {
    let fixture = Fixture::new();
    let lease = fixture.deployment.controller_lease().unwrap();
    assert_eq!(
        lease.expires_at - lease.acquired_at,
        Duration::seconds(3_599)
    );
}
