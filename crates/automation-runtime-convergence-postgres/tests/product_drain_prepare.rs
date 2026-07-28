use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    ActivationAttestationV1, ActivationOutcomeKindV1, ActivationRequestId, BindingRevision,
    CommandGuardV1, ControllerId, DeploymentId, DeploymentRevision, DrainAttestationV1,
    FencingToken, GatewayReadyAttestationV1, GatewayReadyKindV1, InstallationId, LeaseRequestV1,
    LiveLossKindV1, PanelCertificateId, PanelCertificateV1, PanelReportDigestV1,
    PreflightAttestationV1, ProcessInstanceId, PromotionId, RuntimeDeployment,
    RuntimeDeploymentError, RuntimeDeploymentIdentityV1, RuntimeDeploymentPhaseV1,
    RuntimeDeploymentSnapshotV1, RuntimeDeploymentTargetV1, RuntimeGeneration,
    RuntimeProcessIdentityV1, SupersedingDeploymentV1, TenantId,
};
use automation_runtime_convergence_postgres::{
    prepare_product_drain_source_supersession_v1, RuntimeConvergenceStoreError,
};
use chrono::{DateTime, TimeZone, Utc};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;
use serde_json::Value;
use sha2::{Digest, Sha256};

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

    fn advance_to_awaiting(&mut self) {
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
        self.deployment
            .begin_panel_reconciliation(&self.guard(31))
            .unwrap();
        self.deployment
            .accept_panel_certificate(&self.guard(40), self.panel())
            .unwrap();
    }

    fn advance_to_live(&mut self) {
        self.advance_to_awaiting();
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

fn prepare(
    source: RuntimeDeploymentSnapshotV1,
    successor: SupersedingDeploymentV1,
    reason: &str,
) -> Result<
    automation_runtime_convergence_postgres::PreparedProductDrainSourceSupersessionV1,
    RuntimeConvergenceStoreError,
> {
    let expected_revision = source.revision;
    prepare_product_drain_source_supersession_v1(
        source,
        expected_revision,
        at(60),
        successor,
        reason.to_string(),
        at(61),
    )
}

fn raw_sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn awaiting_source_prepares_exact_superseded_projection_and_bytes() {
    let mut fixture = Fixture::new();
    fixture.advance_to_awaiting();
    let source = fixture.deployment.snapshot();
    let successor = fixture.successor();
    let prepared = prepare(
        source.clone(),
        successor.clone(),
        "correlated Product apply",
    )
    .unwrap();
    assert_eq!(
        prepared.resulting_revision(),
        source.revision.next().unwrap()
    );
    assert_eq!(prepared.snapshot().revision, prepared.resulting_revision());
    assert_eq!(
        prepared.snapshot().phase,
        RuntimeDeploymentPhaseV1::Superseded {
            by: successor,
            reason: "correlated Product apply".to_string(),
            superseded_at: at(61),
        }
    );
    assert_eq!(
        prepared.snapshot_bytes(),
        serde_json::to_vec(prepared.snapshot()).unwrap()
    );
    assert_eq!(
        prepared.snapshot_json(),
        &serde_json::from_slice::<Value>(prepared.snapshot_bytes()).unwrap()
    );
    assert_eq!(
        prepared.snapshot_digest(),
        raw_sha256_hex(prepared.snapshot_bytes())
    );
    assert_eq!(
        RuntimeDeployment::restore(serde_json::from_slice(prepared.snapshot_bytes()).unwrap())
            .unwrap()
            .snapshot(),
        prepared.snapshot().clone()
    );
    assert_eq!(
        format!("{prepared:?}"),
        "PreparedProductDrainSourceSupersessionV1(<opaque>)"
    );
}

#[test]
fn live_source_preserves_prior_live_and_clears_active_evidence() {
    let mut fixture = Fixture::new();
    fixture.advance_to_live();
    let source = fixture.deployment.snapshot();
    let prior_live = source.live.clone().unwrap();
    let prepared = prepare(
        source.clone(),
        fixture.successor(),
        "correlated Product apply",
    )
    .unwrap();
    let result = prepared.snapshot();
    assert_eq!(result.controller_lease, None);
    assert_eq!(result.panel_certificate, None);
    assert_eq!(result.gateway_ready, None);
    assert_eq!(result.live, None);
    assert_eq!(result.preflight, source.preflight);
    assert_eq!(result.drain, source.drain);
    assert_eq!(result.activation, source.activation);
    let recovery = result.last_live_recovery.as_ref().unwrap();
    assert_eq!(recovery.prior_live, prior_live);
    assert_eq!(recovery.kind, LiveLossKindV1::ServingDisconnected);
    assert_eq!(recovery.evidence_at, at(60));
    assert_eq!(recovery.recovered_at, at(61));
}

#[test]
fn snapshot_digest_has_a_stable_golden_and_changes_with_projection() {
    let mut fixture = Fixture::new();
    fixture.advance_to_awaiting();
    let source = fixture.deployment.snapshot();
    let successor = fixture.successor();
    let first = prepare(
        source.clone(),
        successor.clone(),
        "correlated Product apply",
    )
    .unwrap();
    let changed = prepare(source, successor, "different Product apply").unwrap();
    assert_eq!(
        first.snapshot_digest(),
        "cae4194fd51c94881260b61c37472cfabe5efd3a86287a551fa9a3aa741d715e"
    );
    assert_ne!(first.snapshot_bytes(), changed.snapshot_bytes());
    assert_ne!(first.snapshot_digest(), changed.snapshot_digest());
}

#[test]
fn malformed_tampered_or_drifted_source_is_rejected() {
    let mut fixture = Fixture::new();
    fixture.advance_to_awaiting();
    let source = fixture.deployment.snapshot();
    let mut malformed = source.clone();
    malformed.phase = RuntimeDeploymentPhaseV1::Live;
    malformed.controller_lease = None;
    assert!(matches!(
        prepare(malformed, fixture.successor(), "correlated Product apply"),
        Err(RuntimeConvergenceStoreError::Domain(
            RuntimeDeploymentError::InvalidSnapshot
        ))
    ));
    assert!(matches!(
        prepare_product_drain_source_supersession_v1(
            source.clone(),
            DeploymentRevision::FIRST,
            at(60),
            fixture.successor(),
            "correlated Product apply".to_string(),
            at(61),
        ),
        Err(RuntimeConvergenceStoreError::Domain(
            RuntimeDeploymentError::RevisionConflict { .. }
        ))
    ));
    assert!(matches!(
        prepare_product_drain_source_supersession_v1(
            source.clone(),
            source.revision,
            at(39),
            fixture.successor(),
            "correlated Product apply".to_string(),
            at(61),
        ),
        Err(RuntimeConvergenceStoreError::Domain(
            RuntimeDeploymentError::AttestationTimeRegression
        ))
    ));
    assert!(matches!(
        prepare_product_drain_source_supersession_v1(
            source.clone(),
            source.revision,
            at(60),
            fixture.successor(),
            "correlated Product apply".to_string(),
            at(59),
        ),
        Err(RuntimeConvergenceStoreError::Domain(
            RuntimeDeploymentError::AttestationTimeRegression
        ))
    ));
    let mut wrong_scope = fixture.successor();
    wrong_scope.identity.tenant_id = TenantId::parse("tenant-other").unwrap();
    assert!(matches!(
        prepare(source.clone(), wrong_scope, "correlated Product apply"),
        Err(RuntimeConvergenceStoreError::Domain(
            RuntimeDeploymentError::SupersedingDeploymentScopeMismatch
        ))
    ));
    assert!(matches!(
        prepare(source, fixture.successor(), &"x".repeat(1_025)),
        Err(RuntimeConvergenceStoreError::Domain(
            RuntimeDeploymentError::InvalidReason
        ))
    ));
}

#[test]
fn database_timestamps_must_be_canonical_microseconds() {
    let mut fixture = Fixture::new();
    fixture.advance_to_awaiting();
    let source = fixture.deployment.snapshot();
    let submicrosecond = DateTime::from_timestamp(1_800_000_060, 1).unwrap();
    assert!(matches!(
        prepare_product_drain_source_supersession_v1(
            source.clone(),
            source.revision,
            submicrosecond,
            fixture.successor(),
            "correlated Product apply".to_string(),
            at(61),
        ),
        Err(RuntimeConvergenceStoreError::InvalidInput(
            "Product drain acknowledgement database timestamp"
        ))
    ));
    assert!(matches!(
        prepare_product_drain_source_supersession_v1(
            source.clone(),
            source.revision,
            at(60),
            fixture.successor(),
            "correlated Product apply".to_string(),
            DateTime::from_timestamp(1_800_000_061, 1).unwrap(),
        ),
        Err(RuntimeConvergenceStoreError::InvalidInput(
            "Product drain terminal database timestamp"
        ))
    ));
}

#[test]
fn database_revision_and_projection_overflow_are_rejected() {
    let mut fixture = Fixture::new();
    fixture.advance_to_awaiting();
    let mut source = fixture.deployment.snapshot();
    source.revision = DeploymentRevision::new(i64::MAX as u64).unwrap();
    RuntimeDeployment::restore(source.clone()).unwrap();
    assert!(matches!(
        prepare(source, fixture.successor(), "correlated Product apply"),
        Err(RuntimeConvergenceStoreError::InvalidInput(
            "runtime deployment projection"
        ))
    ));
    let source = fixture.deployment.snapshot();
    let mut successor = fixture.successor();
    successor.runtime_generation = RuntimeGeneration::new(u64::MAX).unwrap();
    assert!(matches!(
        prepare(source, successor, "correlated Product apply"),
        Err(RuntimeConvergenceStoreError::InvalidInput(
            "runtime deployment projection"
        ))
    ));
}
