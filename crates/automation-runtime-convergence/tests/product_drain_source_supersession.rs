use std::num::NonZeroU32;

use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    ActivationAttestationV1, ActivationOutcomeKindV1, ActivationRequestId, BindingRevision,
    CommandGuardV1, ControllerId, DeploymentId, DeploymentRevision, DrainAttestationV1,
    FencingToken, GatewayReadyAttestationV1, GatewayReadyKindV1, InstallationId, LeaseRequestV1,
    LiveLossKindV1, PanelCertificateId, PanelCertificateV1, PanelReportDigestV1,
    PreflightAttestationV1, ProcessInstanceId, ProductDrainSourceCancellationPermitV1,
    ProductDrainSourceSupersessionPermitV1, PromotionId, RuntimeDeployment, RuntimeDeploymentError,
    RuntimeDeploymentIdentityV1, RuntimeDeploymentPhaseKindV1, RuntimeDeploymentPhaseV1,
    RuntimeDeploymentSnapshotV1, RuntimeDeploymentTargetV1, RuntimeFailureId, RuntimeFailureKindV1,
    RuntimeFailureV1, RuntimeGeneration, RuntimeProcessIdentityV1, SupersedingDeploymentV1,
    TenantId, TransitionOutcomeV1,
};
use chrono::{DateTime, TimeZone, Utc};
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

fn identity(
    deployment_id: &str,
    tenant_id: &str,
    installation_id: &str,
    promotion: char,
) -> RuntimeDeploymentIdentityV1 {
    RuntimeDeploymentIdentityV1 {
        deployment_id: DeploymentId::parse(deployment_id).unwrap(),
        tenant_id: TenantId::parse(tenant_id).unwrap(),
        installation_id: InstallationId::parse(installation_id).unwrap(),
        promotion_id: PromotionId::parse(promotion.to_string().repeat(64)).unwrap(),
        activation_request_id: ActivationRequestId::parse(format!("activation-{deployment_id}"))
            .unwrap(),
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
            identity("deployment-2", "tenant-42", "installation-42", 'd'),
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
            activation_request_id: self.deployment.identity().activation_request_id.clone(),
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

    fn advance_to_awaiting_with_failure_history(&mut self) {
        self.advance_to_pending();
        self.deployment
            .begin_panel_reconciliation(&self.guard(31))
            .unwrap();
        self.deployment
            .record_retryable_failure(
                &self.guard(35),
                RuntimeFailureV1 {
                    failure_id: RuntimeFailureId::parse("failure-before-product-drain").unwrap(),
                    kind: RuntimeFailureKindV1::GatewayReadyTimeout,
                    code: "gateway_ready_timeout".to_string(),
                    message: "gateway readiness timed out".to_string(),
                    recorded_at: at(35),
                },
                NonZeroU32::new(1).unwrap(),
                at(36),
            )
            .unwrap();
        self.token = FencingToken::new(11).unwrap();
        self.deployment
            .acquire_lease(LeaseRequestV1 {
                expected_revision: self.deployment.revision(),
                controller_id: self.controller.clone(),
                fencing_token: self.token,
                now: at(36),
                expires_at: at(3_600),
            })
            .unwrap();
        self.deployment
            .resume_runtime_pending(&self.guard(36))
            .unwrap();
        self.deployment
            .begin_panel_reconciliation(&self.guard(37))
            .unwrap();
        self.deployment
            .accept_panel_certificate(&self.guard(40), self.panel())
            .unwrap();
    }

    fn advance_to_live_with_failure_history(&mut self) {
        self.advance_to_awaiting_with_failure_history();
        self.deployment
            .certify_live(&self.guard(50), self.ready(), at(51))
            .unwrap();
    }

    fn successor(&self) -> SupersedingDeploymentV1 {
        SupersedingDeploymentV1 {
            identity: identity("deployment-3", "tenant-42", "installation-42", 'e'),
            target: target(3, 3, 'c'),
            runtime_generation: RuntimeGeneration::new(3).unwrap(),
        }
    }
}

fn permit(
    deployment: &RuntimeDeployment,
    acknowledged_at: DateTime<Utc>,
) -> ProductDrainSourceSupersessionPermitV1 {
    ProductDrainSourceSupersessionPermitV1::from_adapter_validated_durable_route_absence_acknowledgement(
        deployment,
        deployment.revision(),
        acknowledged_at,
    )
    .unwrap()
}

fn assert_common_history_preserved(
    before: &RuntimeDeploymentSnapshotV1,
    after: &RuntimeDeploymentSnapshotV1,
) {
    assert_eq!(after.identity, before.identity);
    assert_eq!(after.target, before.target);
    assert_eq!(after.runtime_generation, before.runtime_generation);
    assert_eq!(after.previous_runtime, before.previous_runtime);
    assert_eq!(after.requested_at, before.requested_at);
    assert_eq!(after.revision, before.revision.next().unwrap());
    assert_eq!(after.last_fencing_token, before.last_fencing_token);
    assert_eq!(after.preflight, before.preflight);
    assert_eq!(after.drain, before.drain);
    assert_eq!(after.activation, before.activation);
    assert_eq!(after.last_runtime_failure, before.last_runtime_failure);
}

#[test]
fn awaiting_gateway_ready_source_is_superseded_once_with_history_preserved() {
    let mut fixture = Fixture::new();
    fixture.advance_to_awaiting_with_failure_history();
    let before = fixture.deployment.snapshot();
    let successor = fixture.successor();
    let outcome = fixture
        .deployment
        .supersede_product_drain_source(
            permit(&fixture.deployment, at(60)),
            successor.clone(),
            "correlated Product apply".to_string(),
            at(61),
        )
        .unwrap();
    assert_eq!(
        outcome,
        TransitionOutcomeV1::Applied {
            revision: before.revision.next().unwrap()
        }
    );
    let after = fixture.deployment.snapshot();
    assert_common_history_preserved(&before, &after);
    assert_eq!(after.controller_lease, None);
    assert_eq!(after.panel_certificate, before.panel_certificate);
    assert_eq!(after.gateway_ready, None);
    assert_eq!(after.live, None);
    assert_eq!(after.last_live_recovery, before.last_live_recovery);
    assert_eq!(
        after.phase,
        RuntimeDeploymentPhaseV1::Superseded {
            by: successor,
            reason: "correlated Product apply".to_string(),
            superseded_at: at(61),
        }
    );
    assert_eq!(
        RuntimeDeployment::restore(after).unwrap(),
        fixture.deployment
    );
}

#[test]
fn live_source_moves_exact_active_evidence_to_recovery_and_clears_it() {
    let mut fixture = Fixture::new();
    fixture.advance_to_live_with_failure_history();
    let before = fixture.deployment.snapshot();
    let prior_live = before.live.clone().unwrap();
    let successor = fixture.successor();
    fixture
        .deployment
        .supersede_product_drain_source(
            permit(&fixture.deployment, at(60)),
            successor.clone(),
            "correlated Product apply".to_string(),
            at(61),
        )
        .unwrap();
    let after = fixture.deployment.snapshot();
    assert_common_history_preserved(&before, &after);
    assert_eq!(after.controller_lease, None);
    assert_eq!(after.panel_certificate, None);
    assert_eq!(after.gateway_ready, None);
    assert_eq!(after.live, None);
    let recovery = after.last_live_recovery.as_ref().unwrap();
    assert_eq!(recovery.prior_live, prior_live);
    assert_eq!(recovery.kind, LiveLossKindV1::ServingDisconnected);
    assert_eq!(recovery.evidence_at, at(60));
    assert_eq!(recovery.recovered_at, at(61));
    assert_eq!(
        after.phase,
        RuntimeDeploymentPhaseV1::Superseded {
            by: successor,
            reason: "correlated Product apply".to_string(),
            superseded_at: at(61),
        }
    );
    assert_eq!(
        RuntimeDeployment::restore(after).unwrap(),
        fixture.deployment
    );
}

#[test]
fn exact_terminal_shape_replays_without_advancing_revision() {
    let mut fixture = Fixture::new();
    fixture.advance_to_live_with_failure_history();
    let first = permit(&fixture.deployment, at(60));
    let replay = permit(&fixture.deployment, at(60));
    let successor = fixture.successor();
    fixture
        .deployment
        .supersede_product_drain_source(
            first,
            successor.clone(),
            "correlated Product apply".to_string(),
            at(61),
        )
        .unwrap();
    let revision = fixture.deployment.revision();
    let snapshot = fixture.deployment.snapshot();
    assert_eq!(
        fixture
            .deployment
            .supersede_product_drain_source(
                replay,
                successor,
                "correlated Product apply".to_string(),
                at(61),
            )
            .unwrap(),
        TransitionOutcomeV1::Replayed { revision }
    );
    assert_eq!(fixture.deployment.snapshot(), snapshot);
}

#[test]
fn permit_accepts_only_exact_eligible_source_and_monotonic_acknowledgement() {
    let fixture = Fixture::new();
    assert_eq!(
        ProductDrainSourceSupersessionPermitV1::from_adapter_validated_durable_route_absence_acknowledgement(
            &fixture.deployment,
            fixture.deployment.revision(),
            at(60),
        )
        .unwrap_err(),
        RuntimeDeploymentError::InvalidTransition {
            current: RuntimeDeploymentPhaseKindV1::Requested,
            operation: "prove_product_drain_route_absence_acknowledgement",
        }
    );
    let mut awaiting = Fixture::new();
    awaiting.advance_to_awaiting_with_failure_history();
    assert!(matches!(
        ProductDrainSourceSupersessionPermitV1::from_adapter_validated_durable_route_absence_acknowledgement(
            &awaiting.deployment,
            DeploymentRevision::FIRST,
            at(60),
        ),
        Err(RuntimeDeploymentError::RevisionConflict { .. })
    ));
    assert_eq!(
        ProductDrainSourceSupersessionPermitV1::from_adapter_validated_durable_route_absence_acknowledgement(
            &awaiting.deployment,
            awaiting.deployment.revision(),
            at(39),
        )
        .unwrap_err(),
        RuntimeDeploymentError::AttestationTimeRegression
    );
    awaiting
        .deployment
        .certify_live(&awaiting.guard(50), awaiting.ready(), at(51))
        .unwrap();
    assert_eq!(
        ProductDrainSourceSupersessionPermitV1::from_adapter_validated_durable_route_absence_acknowledgement(
            &awaiting.deployment,
            awaiting.deployment.revision(),
            at(50),
        )
        .unwrap_err(),
        RuntimeDeploymentError::AttestationTimeRegression
    );
}

#[test]
fn source_change_after_proof_is_rejected_without_mutation() {
    let mut fixture = Fixture::new();
    fixture.advance_to_awaiting_with_failure_history();
    let proof = permit(&fixture.deployment, at(60));
    fixture
        .deployment
        .certify_live(&fixture.guard(50), fixture.ready(), at(51))
        .unwrap();
    let before = fixture.deployment.snapshot();
    assert_eq!(
        fixture
            .deployment
            .supersede_product_drain_source(
                proof,
                fixture.successor(),
                "correlated Product apply".to_string(),
                at(61),
            )
            .unwrap_err(),
        RuntimeDeploymentError::ProductDrainSupersessionSourceMismatch
    );
    assert_eq!(fixture.deployment.snapshot(), before);
}

#[test]
fn successor_scope_generation_identity_reason_and_clock_are_checked() {
    enum Case {
        OldGeneration,
        SameIdentity,
        WrongScope,
        WrongSlot,
        EmptyReason,
        EarlyTerminalClock,
    }
    let cases = [
        Case::OldGeneration,
        Case::SameIdentity,
        Case::WrongScope,
        Case::WrongSlot,
        Case::EmptyReason,
        Case::EarlyTerminalClock,
    ];
    for case in cases {
        let mut fixture = Fixture::new();
        fixture.advance_to_awaiting_with_failure_history();
        let mut successor = fixture.successor();
        let mut reason = "correlated Product apply".to_string();
        let mut superseded_at = at(61);
        let expected = match case {
            Case::OldGeneration => {
                successor.runtime_generation = RuntimeGeneration::new(2).unwrap();
                RuntimeDeploymentError::RuntimeGenerationNotMonotonic
            }
            Case::SameIdentity => {
                successor.identity = fixture.deployment.identity().clone();
                RuntimeDeploymentError::SupersedingDeploymentIdentityConflict
            }
            Case::WrongScope => {
                successor.identity.tenant_id = TenantId::parse("tenant-other").unwrap();
                RuntimeDeploymentError::SupersedingDeploymentScopeMismatch
            }
            Case::WrongSlot => {
                successor.target.guild_id = GuildId(99);
                RuntimeDeploymentError::PreviousRuntimeSlotMismatch
            }
            Case::EmptyReason => {
                reason = " ".to_string();
                RuntimeDeploymentError::InvalidReason
            }
            Case::EarlyTerminalClock => {
                superseded_at = at(59);
                RuntimeDeploymentError::AttestationTimeRegression
            }
        };
        let before = fixture.deployment.snapshot();
        assert_eq!(
            fixture
                .deployment
                .supersede_product_drain_source(
                    permit(&fixture.deployment, at(60)),
                    successor,
                    reason,
                    superseded_at,
                )
                .unwrap_err(),
            expected
        );
        assert_eq!(fixture.deployment.snapshot(), before);
    }
}

#[test]
fn mismatched_terminal_replay_shape_is_rejected_without_mutation() {
    let mut fixture = Fixture::new();
    fixture.advance_to_live_with_failure_history();
    let first = permit(&fixture.deployment, at(60));
    let changed = permit(&fixture.deployment, at(60));
    let successor = fixture.successor();
    fixture
        .deployment
        .supersede_product_drain_source(
            first,
            successor.clone(),
            "correlated Product apply".to_string(),
            at(61),
        )
        .unwrap();
    let before = fixture.deployment.snapshot();
    assert_eq!(
        fixture
            .deployment
            .supersede_product_drain_source(
                changed,
                successor,
                "different Product apply".to_string(),
                at(61),
            )
            .unwrap_err(),
        RuntimeDeploymentError::ProductDrainSupersessionSourceMismatch
    );
    assert_eq!(fixture.deployment.snapshot(), before);
}

#[test]
fn revision_overflow_is_rejected_without_partial_mutation() {
    let mut fixture = Fixture::new();
    fixture.advance_to_awaiting_with_failure_history();
    let mut snapshot = fixture.deployment.snapshot();
    snapshot.revision = DeploymentRevision::new(u64::MAX).unwrap();
    fixture.deployment = RuntimeDeployment::restore(snapshot).unwrap();
    let before = fixture.deployment.snapshot();
    assert_eq!(
        fixture
            .deployment
            .supersede_product_drain_source(
                permit(&fixture.deployment, at(60)),
                fixture.successor(),
                "correlated Product apply".to_string(),
                at(61),
            )
            .unwrap_err(),
        RuntimeDeploymentError::RevisionOverflow
    );
    assert_eq!(fixture.deployment.snapshot(), before);
}

#[test]
fn generic_live_supersession_permissions_remain_closed() {
    let mut fixture = Fixture::new();
    fixture.advance_to_live_with_failure_history();
    assert_eq!(
        fixture
            .deployment
            .supersede(
                &CommandGuardV1 {
                    expected_revision: fixture.deployment.revision(),
                    controller_id: fixture.controller.clone(),
                    fencing_token: fixture.token,
                    runtime_generation: fixture.deployment.runtime_generation(),
                    now: at(60),
                },
                fixture.successor(),
                "generic supersession".to_string(),
                at(61),
            )
            .unwrap_err(),
        RuntimeDeploymentError::LeaseRequired
    );
}

#[test]
fn proof_debug_output_is_opaque() {
    let mut fixture = Fixture::new();
    fixture.advance_to_awaiting_with_failure_history();
    assert_eq!(
        format!("{:?}", permit(&fixture.deployment, at(60))),
        "ProductDrainSourceSupersessionPermitV1(<opaque>)"
    );
}

fn cancellation_permit(
    deployment: &RuntimeDeployment,
    acknowledged_at: DateTime<Utc>,
) -> ProductDrainSourceCancellationPermitV1 {
    ProductDrainSourceCancellationPermitV1::from_adapter_validated_durable_route_absence_acknowledgement(
        deployment,
        deployment.revision(),
        acknowledged_at,
    )
    .unwrap()
}

fn assert_only_revision_changed(
    before: &RuntimeDeploymentSnapshotV1,
    after: &RuntimeDeploymentSnapshotV1,
) {
    let mut expected = before.clone();
    expected.revision = expected.revision.next().unwrap();
    assert_eq!(after, &expected);
}

#[test]
fn awaiting_gateway_ready_product_drain_cancellation_only_advances_revision() {
    let mut fixture = Fixture::new();
    fixture.advance_to_awaiting_with_failure_history();
    let before = fixture.deployment.snapshot();
    assert_eq!(
        fixture
            .deployment
            .cancel_product_drain_source(cancellation_permit(&fixture.deployment, at(60)), at(61))
            .unwrap(),
        TransitionOutcomeV1::Applied {
            revision: before.revision.next().unwrap()
        }
    );
    let after = fixture.deployment.snapshot();
    assert_only_revision_changed(&before, &after);
    assert_eq!(after.phase, RuntimeDeploymentPhaseV1::AwaitingGatewayReady);
    assert_eq!(
        RuntimeDeployment::restore(after).unwrap(),
        fixture.deployment
    );
}

#[test]
fn live_product_drain_cancellation_preserves_all_serving_evidence_and_lease() {
    let mut fixture = Fixture::new();
    fixture.advance_to_live_with_failure_history();
    let before = fixture.deployment.snapshot();
    fixture
        .deployment
        .cancel_product_drain_source(cancellation_permit(&fixture.deployment, at(60)), at(61))
        .unwrap();
    let after = fixture.deployment.snapshot();
    assert_only_revision_changed(&before, &after);
    assert_eq!(after.phase, RuntimeDeploymentPhaseV1::Live);
    assert_eq!(after.controller_lease, before.controller_lease);
    assert_eq!(after.panel_certificate, before.panel_certificate);
    assert_eq!(after.gateway_ready, before.gateway_ready);
    assert_eq!(after.live, before.live);
    assert_eq!(after.last_live_recovery, before.last_live_recovery);
}

#[test]
fn product_drain_cancellation_replay_is_rejected_without_terminal_journal_evidence() {
    let mut fixture = Fixture::new();
    fixture.advance_to_live_with_failure_history();
    let first = cancellation_permit(&fixture.deployment, at(60));
    let same_time_replay = cancellation_permit(&fixture.deployment, at(60));
    let changed_time_replay = cancellation_permit(&fixture.deployment, at(60));
    fixture
        .deployment
        .cancel_product_drain_source(first, at(61))
        .unwrap();
    let snapshot = fixture.deployment.snapshot();
    assert_eq!(
        fixture
            .deployment
            .cancel_product_drain_source(same_time_replay, at(61))
            .unwrap_err(),
        RuntimeDeploymentError::ProductDrainCancellationSourceMismatch
    );
    assert_eq!(fixture.deployment.snapshot(), snapshot);
    assert_eq!(
        fixture
            .deployment
            .cancel_product_drain_source(changed_time_replay, at(62))
            .unwrap_err(),
        RuntimeDeploymentError::ProductDrainCancellationSourceMismatch
    );
    assert_eq!(fixture.deployment.snapshot(), snapshot);
}

#[test]
fn product_drain_cancellation_permit_requires_exact_eligible_source() {
    let fixture = Fixture::new();
    assert_eq!(
        ProductDrainSourceCancellationPermitV1::from_adapter_validated_durable_route_absence_acknowledgement(
            &fixture.deployment,
            fixture.deployment.revision(),
            at(60),
        )
        .unwrap_err(),
        RuntimeDeploymentError::InvalidTransition {
            current: RuntimeDeploymentPhaseKindV1::Requested,
            operation: "prove_product_drain_route_absence_cancellation",
        }
    );
    let mut awaiting = Fixture::new();
    awaiting.advance_to_awaiting_with_failure_history();
    assert!(matches!(
        ProductDrainSourceCancellationPermitV1::from_adapter_validated_durable_route_absence_acknowledgement(
            &awaiting.deployment,
            DeploymentRevision::FIRST,
            at(60),
        ),
        Err(RuntimeDeploymentError::RevisionConflict { .. })
    ));
    assert_eq!(
        ProductDrainSourceCancellationPermitV1::from_adapter_validated_durable_route_absence_acknowledgement(
            &awaiting.deployment,
            awaiting.deployment.revision(),
            at(39),
        )
        .unwrap_err(),
        RuntimeDeploymentError::AttestationTimeRegression
    );
    assert!(
        ProductDrainSourceCancellationPermitV1::from_adapter_validated_durable_route_absence_acknowledgement(
            &awaiting.deployment,
            awaiting.deployment.revision(),
            at(40),
        )
        .is_ok()
    );
    awaiting
        .deployment
        .certify_live(&awaiting.guard(50), awaiting.ready(), at(51))
        .unwrap();
    assert_eq!(
        ProductDrainSourceCancellationPermitV1::from_adapter_validated_durable_route_absence_acknowledgement(
            &awaiting.deployment,
            awaiting.deployment.revision(),
            at(50),
        )
        .unwrap_err(),
        RuntimeDeploymentError::AttestationTimeRegression
    );
    assert!(
        ProductDrainSourceCancellationPermitV1::from_adapter_validated_durable_route_absence_acknowledgement(
            &awaiting.deployment,
            awaiting.deployment.revision(),
            at(51),
        )
        .is_ok()
    );
}

#[test]
fn product_drain_cancellation_rejects_source_drift_without_mutation() {
    let mut fixture = Fixture::new();
    fixture.advance_to_awaiting_with_failure_history();
    let permit = cancellation_permit(&fixture.deployment, at(60));
    fixture
        .deployment
        .certify_live(&fixture.guard(50), fixture.ready(), at(51))
        .unwrap();
    let before = fixture.deployment.snapshot();
    assert_eq!(
        fixture
            .deployment
            .cancel_product_drain_source(permit, at(61))
            .unwrap_err(),
        RuntimeDeploymentError::ProductDrainCancellationSourceMismatch
    );
    assert_eq!(fixture.deployment.snapshot(), before);
}

#[test]
fn product_drain_cancellation_rejects_clock_and_revision_overflow_without_mutation() {
    let mut early = Fixture::new();
    early.advance_to_awaiting_with_failure_history();
    let before = early.deployment.snapshot();
    assert_eq!(
        early
            .deployment
            .cancel_product_drain_source(cancellation_permit(&early.deployment, at(60)), at(59))
            .unwrap_err(),
        RuntimeDeploymentError::AttestationTimeRegression
    );
    assert_eq!(early.deployment.snapshot(), before);

    let mut overflow = Fixture::new();
    overflow.advance_to_awaiting_with_failure_history();
    let mut snapshot = overflow.deployment.snapshot();
    snapshot.revision = DeploymentRevision::new(u64::MAX).unwrap();
    overflow.deployment = RuntimeDeployment::restore(snapshot).unwrap();
    let before = overflow.deployment.snapshot();
    assert_eq!(
        overflow
            .deployment
            .cancel_product_drain_source(cancellation_permit(&overflow.deployment, at(60)), at(61),)
            .unwrap_err(),
        RuntimeDeploymentError::RevisionOverflow
    );
    assert_eq!(overflow.deployment.snapshot(), before);
}

#[test]
fn product_drain_cancellation_permit_debug_output_is_opaque() {
    let mut fixture = Fixture::new();
    fixture.advance_to_awaiting_with_failure_history();
    assert_eq!(
        format!("{:?}", cancellation_permit(&fixture.deployment, at(60))),
        "ProductDrainSourceCancellationPermitV1(<opaque>)"
    );
}
