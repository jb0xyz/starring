#[test]
fn regular_dependencies_stay_pure() {
    let manifest = include_str!("../Cargo.toml");
    let regular = manifest
        .split("[dependencies]")
        .nth(1)
        .unwrap_or("")
        .split("[dev-dependencies]")
        .next()
        .unwrap_or("");
    for forbidden in [
        "sqlx",
        "rusqlite",
        "sqlite",
        "twilight",
        "ai-gateway",
        "ai_gateway",
        "ollama",
        "llm",
        "reqwest",
        "design-harness",
        "automation-runtime =",
        "automation-runtime-convergence-postgres",
        "tokio",
    ] {
        assert!(
            !regular.contains(forbidden),
            "forbidden regular dependency: {forbidden}"
        );
    }
}

#[test]
fn crate_is_library_only() {
    let manifest = include_str!("../Cargo.toml");
    assert!(!manifest.contains("[[bin]]"));
}

#[test]
fn execution_and_serving_ports_stay_independent() {
    let source = include_str!("../src/port.rs");
    let execution = source
        .split("pub trait RuntimeExecutionConvergencePort {")
        .nth(1)
        .and_then(|tail| tail.split("pub trait RuntimeServingLeasePort {").next())
        .unwrap();
    for method in [
        "claim_next_execution",
        "renew_execution",
        "mutate",
        "certify_live",
        "recover_next_stale_live",
        "classify_error",
    ] {
        assert!(execution.contains(method));
    }
    assert!(!execution.contains("heartbeat_serving"));
    assert!(!execution.contains("mark_serving_disconnected"));

    let serving = source
        .split("pub trait RuntimeServingLeasePort {")
        .nth(1)
        .and_then(|tail| {
            tail.split("pub trait RuntimePreviousServingObservationPort:")
                .next()
        })
        .unwrap();
    for method in [
        "heartbeat_serving",
        "mark_serving_disconnected",
        "classify_error",
    ] {
        assert!(serving.contains(method));
    }
    assert!(!serving.contains("claim_next_execution"));
    assert!(!serving.contains("recover_next_stale_live"));
    assert!(source.contains(
        "pub trait RuntimePreviousServingObservationPort: RuntimeExecutionConvergencePort"
    ));
}

#[test]
fn persistence_contract_is_versioned_and_database_independent() {
    let manifest = include_str!("../Cargo.toml");
    let regular = manifest
        .split("[dependencies]")
        .nth(1)
        .unwrap_or("")
        .split("[dev-dependencies]")
        .next()
        .unwrap_or("");
    for forbidden in ["sqlx", "rusqlite", "twilight", "reqwest"] {
        assert!(!regular.contains(forbidden));
    }
    let source = include_str!("../src/persistence.rs");
    for contract in [
        "runtime_desired_target_digest_v1",
        "encode_runtime_live_attestation_record_v1",
        "decode_runtime_live_attestation_record_v1",
        "runtime_live_attestation_digest_v1",
        "RuntimeLiveAttestationRecordV1",
    ] {
        assert!(source.contains(contract));
    }
    assert!(source.contains("impl<'de> Deserialize<'de> for RuntimeDesiredTargetDigestV1"));
}

#[test]
fn v2_evidence_stays_domain_only_and_runtime_independent() {
    for (path, source) in [
        ("v2_gateway.rs", include_str!("../src/v2_gateway.rs")),
        ("v2_evidence.rs", include_str!("../src/v2_evidence.rs")),
        (
            "v2_writer_fence.rs",
            include_str!("../src/v2_writer_fence.rs"),
        ),
        (
            "v2_startup_recovery.rs",
            include_str!("../src/v2_startup_recovery.rs"),
        ),
        (
            "v2_route_provenance.rs",
            include_str!("../src/v2_route_provenance.rs"),
        ),
    ] {
        for forbidden in [
            "Serialize",
            "Deserialize",
            "Default",
            "GatewayReadyLeaseV3",
            "automation_runtime::",
        ] {
            assert!(
                !source.contains(forbidden),
                "forbidden evidence surface in {path}: {forbidden}"
            );
        }
    }
}

#[test]
fn v2_route_mutation_provenance_is_exact_inert_evidence() {
    let source = include_str!("../src/v2_route_provenance.rs");
    for forbidden in [
        "serde",
        "Serialize",
        "Deserialize",
        "Default",
        "Sha256",
        "framed_sha256",
        "canonical_bytes",
        "canonical_json",
        "Authority",
        "Permit",
        "Port",
        "Future",
        "sqlx",
        "rusqlite",
        "tokio",
        "twilight",
        "automation_runtime::",
        "registry_observation_sequence",
        "pub fn validate",
        "pub fn from_parts",
        "pub fn into_parts",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden route provenance surface: {forbidden}"
        );
    }

    let closed = source
        .split("pub struct RuntimeClosedRecoveryRouteWitnessV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    for (field, field_type) in [
        ("recovery_id", "RuntimeRecoveryIdV2"),
        ("originating_emergency_generation", "NonZeroU64"),
        ("recovery_generation", "NonZeroU64"),
        ("recovery_authority_revision", "NonZeroU64"),
        ("gateway_owner_lease_id", "RuntimeGatewayOwnerLeaseIdV1"),
        ("observed_owner_revision", "NonZeroU64"),
        ("owner_expires_at", "DateTime<Utc>"),
        ("process_instance_id", "ProcessInstanceId"),
        ("connection_epoch", "NonZeroU64"),
        ("paused_admission_revision", "NonZeroU64"),
        (
            "connected_event_sequence",
            "RuntimeGatewayAdmissionSequenceV2",
        ),
        ("pause_sequence", "RuntimeGatewayAdmissionSequenceV2"),
    ] {
        assert!(closed.contains(&format!("pub {field}: {field_type},")));
    }
    assert_eq!(closed.matches("    pub ").count(), 12);

    let shutdown = source
        .split("pub struct RuntimeShutdownRouteWitnessV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    for (field, field_type) in [
        ("shutdown_generation", "NonZeroU64"),
        ("gateway_owner_lease_id", "RuntimeGatewayOwnerLeaseIdV1"),
        ("observed_owner_revision", "NonZeroU64"),
        ("owner_expires_at", "DateTime<Utc>"),
        ("process_instance_id", "ProcessInstanceId"),
        ("connection_epoch", "NonZeroU64"),
        ("paused_admission_revision", "NonZeroU64"),
        (
            "connected_event_sequence",
            "RuntimeGatewayAdmissionSequenceV2",
        ),
        ("pause_sequence", "RuntimeGatewayAdmissionSequenceV2"),
    ] {
        assert!(shutdown.contains(&format!("pub {field}: {field_type},")));
    }
    assert_eq!(shutdown.matches("    pub ").count(), 9);

    let provenance = source
        .split("pub enum RuntimeRouteMutationProvenanceV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[cfg(test)]").next())
        .unwrap();
    for member in [
        "Ordinary {",
        "barrier_id: RuntimeBarrierIdV1,",
        "pause: RuntimeBarrierPauseWitnessV2,",
        "ClosedRecovery(RuntimeClosedRecoveryRouteWitnessV2),",
        "Shutdown(RuntimeShutdownRouteWitnessV2),",
    ] {
        assert!(provenance.contains(member), "{member}");
    }
    assert_eq!(
        provenance
            .lines()
            .filter(|line| {
                line.starts_with("    ") && !line.starts_with("        ") && line.trim() != "},"
            })
            .count(),
        3
    );
    assert_eq!(provenance.matches("barrier_id:").count(), 1);
    assert_eq!(provenance.matches("pause:").count(), 1);

    let library = include_str!("../src/lib.rs");
    for exported in [
        "RuntimeClosedRecoveryRouteWitnessV2",
        "RuntimeRouteMutationProvenanceV2",
        "RuntimeShutdownRouteWitnessV2",
    ] {
        assert!(library.contains(exported), "{exported}");
    }
}

#[test]
fn v2_suspension_request_vocabulary_is_closed_and_inert() {
    let source = include_str!("../src/v2_suspension.rs");
    for forbidden in [
        "serde",
        "Serialize",
        "Deserialize",
        "Default",
        "Sha256",
        "framed_sha256",
        "canonical_bytes",
        "canonical_json",
        "RuntimeSuspendAttemptDigestV2",
        "RuntimeFailureDispositionV1",
        "Authority",
        "Permit",
        "Port",
        "Future",
        "sqlx",
        "rusqlite",
        "tokio",
        "twilight",
        "automation_runtime::",
        "pub fn from_parts",
        "pub fn into_parts",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden suspension request surface: {forbidden}"
        );
    }

    let disposition = source
        .split("pub enum RuntimeAttemptDispositionV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    assert!(disposition.contains("Retryable { retry_not_before: DateTime<Utc> },"));
    assert!(disposition.contains("Blocked,"));
    assert_eq!(
        disposition
            .lines()
            .filter(|line| line.starts_with("    "))
            .count(),
        2
    );

    let checkpoint = source
        .split("pub enum RuntimeResumeCheckpointV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    for variant in [
        "VerifyPreflight,",
        "RequestDrain,",
        "CompleteDrain,",
        "BeginActivation,",
        "ObserveActivation,",
        "BeginPanels,",
        "ReconcilePanels,",
    ] {
        assert!(checkpoint.contains(variant), "{variant}");
    }
    assert_eq!(
        checkpoint
            .lines()
            .filter(|line| line.starts_with("    "))
            .count(),
        7
    );

    let source_phase = source
        .split("pub enum RuntimeSuspensionSourcePhaseV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();
    for variant in [
        "Requested,",
        "PreflightReady,",
        "DrainRequested,",
        "Drained,",
        "ActivationApplying,",
        "RuntimePendingReady,",
        "ReconcilingPanels,",
    ] {
        assert!(source_phase.contains(variant), "{variant}");
    }
    assert_eq!(
        source_phase
            .lines()
            .filter(|line| line.starts_with("    "))
            .count(),
        7
    );
    for mapping in [
        "Self::Requested => RuntimeResumeCheckpointV2::VerifyPreflight",
        "Self::PreflightReady => RuntimeResumeCheckpointV2::RequestDrain",
        "Self::DrainRequested => RuntimeResumeCheckpointV2::CompleteDrain",
        "Self::Drained => RuntimeResumeCheckpointV2::BeginActivation",
        "Self::ActivationApplying => RuntimeResumeCheckpointV2::ObserveActivation",
        "Self::RuntimePendingReady => RuntimeResumeCheckpointV2::BeginPanels",
        "Self::ReconcilingPanels => RuntimeResumeCheckpointV2::ReconcilePanels",
    ] {
        assert!(source.contains(mapping), "{mapping}");
    }
    for rejected in [
        "RuntimeDeploymentPhaseV1::RuntimePending { .. }",
        "RuntimeDeploymentPhaseV1::AwaitingGatewayReady",
        "RuntimeDeploymentPhaseV1::Live",
        "RuntimeDeploymentPhaseV1::Superseded { .. }",
        "RuntimeDeploymentPhaseV1::Cancelled { .. }",
    ] {
        assert!(source.contains(rejected), "{rejected}");
    }
    assert!(source.contains("condition: RuntimePendingConditionV1::Ready"));

    let lifecycle = source
        .split("pub enum RuntimeSuspendedRouteLifecycleV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    assert!(lifecycle.contains("Staged,"));
    assert!(lifecycle.contains("Draining,"));
    assert_eq!(
        lifecycle
            .lines()
            .filter(|line| line.starts_with("    "))
            .count(),
        2
    );

    let obligation = source
        .split("pub enum RuntimeDrainObligationV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    for member in [
        "None,",
        "ExactLocalRoute(RuntimeExactLocalRouteIdentityV2),",
        "PreviousServing(RuntimePreviousServingLeaseIdentityV1),",
        "LocalAndPrevious {",
        "local: RuntimeExactLocalRouteIdentityV2,",
        "previous: RuntimePreviousServingLeaseIdentityV1,",
    ] {
        assert!(obligation.contains(member), "{member}");
    }
    assert_eq!(
        obligation
            .lines()
            .filter(|line| {
                line.starts_with("    ") && !line.starts_with("        ") && line.trim() != "},"
            })
            .count(),
        4
    );

    let local_effect = source
        .split("pub enum RuntimeLocalRouteEffectV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    for member in [
        "None,",
        "ExactRoute {",
        "route: RuntimeExactLocalRouteIdentityV2,",
        "lifecycle: RuntimeSuspendedRouteLifecycleV2,",
        "RouteAbsent {",
        "slot: RuntimeServingSlotV2,",
        "expected_route: Option<RuntimeExactLocalRouteIdentityV2>,",
        "provenance: RuntimeRouteMutationProvenanceV2,",
        "observed_sequence: NonZeroU64,",
    ] {
        assert!(local_effect.contains(member), "{member}");
    }
    assert_eq!(
        local_effect
            .lines()
            .filter(|line| {
                line.starts_with("    ") && !line.starts_with("        ") && line.trim() != "},"
            })
            .count(),
        3
    );

    let request = source
        .split("pub struct RuntimeSuspendAttemptRequestV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[cfg(test)]").next())
        .unwrap();
    for (field, field_type) in [
        ("suspension_id", "RuntimeSuspensionIdV2"),
        ("action_id", "RuntimeSessionActionIdV1"),
        ("guard", "RuntimeExecutionGuardV1"),
        ("source_phase", "RuntimeSuspensionSourcePhaseV2"),
        ("failure", "RuntimeFailureV1"),
        ("disposition", "RuntimeAttemptDispositionV2"),
        ("checkpoint", "RuntimeResumeCheckpointV2"),
        ("local_effect", "RuntimeLocalRouteEffectV2"),
        ("drain_obligation", "RuntimeDrainObligationV2"),
    ] {
        assert!(request.contains(&format!("pub {field}: {field_type},")));
    }
    assert_eq!(request.matches("    pub ").count(), 9);
    assert!(!request.contains("RuntimeDeploymentPhaseV1"));

    let library = include_str!("../src/lib.rs");
    for exported in [
        "RuntimeAttemptDispositionV2",
        "RuntimeDrainObligationV2",
        "RuntimeLocalRouteEffectV2",
        "RuntimeResumeCheckpointV2",
        "RuntimeSuspendAttemptRequestV2",
        "RuntimeSuspendedRouteLifecycleV2",
        "RuntimeSuspensionSourcePhaseV2",
    ] {
        assert!(library.contains(exported), "{exported}");
    }
}

#[test]
fn startup_recovery_observation_dtos_are_exact_and_non_authorizing() {
    let source = include_str!("../src/v2_startup_recovery.rs");

    for expected in [
        "pub struct RuntimeStartupRecoveryObservationCorrelationV2 {",
        "pub recovery_id: RuntimeRecoveryIdV2,",
        "pub originating_emergency_generation: NonZeroU64,",
        "pub coordinator_generation: NonZeroU64,",
        "pub authority_revision: NonZeroU64,",
        "pub struct RuntimeStartupRecoveryObservationRequestV2 {",
        "pub correlation: RuntimeStartupRecoveryObservationCorrelationV2,",
        "pub gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,",
        "pub expected_owner_revision: NonZeroU64,",
        "pub expected_owner_expires_at: DateTime<Utc>,",
        "pub struct RuntimeStartupRecoveryObservationReceiptV2 {",
        "pub owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,",
        "pub state: RuntimeStartupRecoveryStateV2,",
    ] {
        assert!(source.contains(expected), "{expected}");
    }
    for forbidden in [
        "Permit",
        "Authority",
        "Port",
        "Future",
        "Instant",
        "sqlx",
        "tokio",
        "Serialize",
        "Deserialize",
        "Default",
    ] {
        assert!(!source.contains(forbidden), "{forbidden}");
    }
    let library = include_str!("../src/lib.rs");
    for exported in [
        "RuntimeStartupRecoveryObservationCorrelationV2",
        "RuntimeStartupRecoveryObservationReceiptV2",
        "RuntimeStartupRecoveryObservationRequestV2",
    ] {
        assert!(library.contains(exported), "{exported}");
    }
}

#[test]
fn v2_digest_surface_stays_typed_and_nonserializable() {
    let source = include_str!("../src/v2_digest.rs");
    for forbidden in [
        "Serialize",
        "Deserialize",
        "Default",
        "pub fn framed_sha256",
        "pub(crate) fn framed_sha256",
        "impl_computed_runtime_digest_v2!(RuntimeProductSemanticRequestDigestV2)",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden V2 digest surface: {forbidden}"
        );
    }
    for typed_helper in [
        "certification_intent_fingerprint_v2",
        "certification_request_digest_v2",
        "live_attestation_digest_v2",
        "product_mutation_digest_v2",
        "drain_intent_digest_v2",
        "suspend_attempt_digest_v2",
    ] {
        assert!(source.contains(&format!("pub(crate) fn {typed_helper}")));
    }
}

#[test]
fn v2_canonical_values_stay_checked_and_nonserializable() {
    let source = include_str!("../src/v2_canonical_value.rs");
    for forbidden in ["Serialize", "Deserialize", "Default"] {
        assert!(
            !source.contains(forbidden),
            "forbidden V2 canonical value surface: {forbidden}"
        );
    }
    assert!(source.contains("pub(crate) struct RuntimePersistenceU64V2"));
    assert!(!source.contains("pub struct RuntimePersistenceU64V2"));
    assert!(source.contains("pub(crate) struct RuntimeDiscordSnowflakeV2"));
    assert!(!source.contains("pub struct RuntimeDiscordSnowflakeV2"));
    assert!(!include_str!("../src/lib.rs").contains("RuntimeDiscordSnowflakeV2"));
}

#[test]
fn v2_certification_inputs_stay_inert_and_exact() {
    let source = include_str!("../src/v2_certification.rs");
    for forbidden in [
        "serde",
        "Serialize",
        "Deserialize",
        "Default",
        "Sha256",
        "canonical_bytes",
        "canonical_json",
        "RuntimeLiveAttestation",
        "RuntimeCertificationReceipt",
        "RuntimeCertificationPort",
        "sqlx",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden inert V2 certification surface: {forbidden}"
        );
    }

    let intent = source
        .split("pub struct RuntimeCertificationIntentV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    assert!(source.contains(concat!(
        "pub struct RuntimeCertificationIntentV2 {\n",
        "    pub action_id: RuntimeSessionActionIdV1,\n",
        "    pub operation_id: RuntimeCertificationOperationIdV2,\n",
        "    pub guard: RuntimeExecutionGuardV1,\n",
        "    pub target: RuntimeDeploymentTargetV1,\n",
        "    pub binding_pin: RuntimeBindingPinV1,\n",
        "    pub process_identity: RuntimeProcessIdentityV1,\n",
        "    pub gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,\n",
        "    pub observed_owner_revision: NonZeroU64,\n",
        "    pub runtime_build_revision: RuntimeBuildRevisionV1,\n",
        "    pub panel: RuntimePanelEvidenceV2,\n",
        "    pub serving_lease_for: Duration,\n",
        "}"
    )));
    for (field, field_type) in [
        ("action_id", "RuntimeSessionActionIdV1"),
        ("operation_id", "RuntimeCertificationOperationIdV2"),
        ("guard", "RuntimeExecutionGuardV1"),
        ("target", "RuntimeDeploymentTargetV1"),
        ("binding_pin", "RuntimeBindingPinV1"),
        ("process_identity", "RuntimeProcessIdentityV1"),
        ("gateway_owner_lease_id", "RuntimeGatewayOwnerLeaseIdV1"),
        ("observed_owner_revision", "NonZeroU64"),
        ("runtime_build_revision", "RuntimeBuildRevisionV1"),
        ("panel", "RuntimePanelEvidenceV2"),
        ("serving_lease_for", "Duration"),
    ] {
        assert!(intent.contains(&format!("pub {field}: {field_type}")));
    }
    assert_eq!(intent.matches("    pub ").count(), 11);

    let request = source
        .split("pub struct RuntimeCertificationRequestV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[cfg(test)]").next())
        .unwrap();
    assert!(source.contains(concat!(
        "pub struct RuntimeCertificationRequestV2 {\n",
        "    pub intent: RuntimeCertificationIntentV2,\n",
        "    pub intent_fingerprint: RuntimeCertificationIntentFingerprintV2,\n",
        "    pub must_commit_before: DateTime<Utc>,\n",
        "    pub route_admission: RuntimeRouteAdmissionAttestationV2,\n",
        "}"
    )));
    for (field, field_type) in [
        ("intent", "RuntimeCertificationIntentV2"),
        (
            "intent_fingerprint",
            "RuntimeCertificationIntentFingerprintV2",
        ),
        ("must_commit_before", "DateTime<Utc>"),
        ("route_admission", "RuntimeRouteAdmissionAttestationV2"),
    ] {
        assert!(request.contains(&format!("pub {field}: {field_type}")));
    }
    assert_eq!(request.matches("    pub ").count(), 4);
    assert_eq!(source.matches("pub struct RuntimeCertification").count(), 2);

    let library = include_str!("../src/lib.rs");
    assert!(library.contains("RuntimeCertificationIntentV2"));
    assert!(library.contains("RuntimeCertificationRequestV2"));
}

#[test]
fn v2_certification_outcomes_stay_exact_and_non_authorizing() {
    let source = include_str!("../src/v2_certification_outcome.rs");
    for forbidden in [
        "serde",
        "Serialize",
        "Deserialize",
        "Default",
        "Sha256",
        "canonical_bytes",
        "canonical_json",
        "RuntimeLiveCertificationPortV2",
        "RuntimePreparedLiveCertificationPortV2",
        "RuntimeCertificationCommitAuthorityV2",
        "Box<RuntimeCertificationReceiptV2>",
        "Future",
        "sqlx",
        "tokio",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden V2 certification outcome surface: {forbidden}"
        );
    }

    let serving_identity = source
        .split("pub struct RuntimeServingIdentityV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    for (field, field_type) in [
        ("scope", "RuntimeDeploymentScopeV1"),
        ("operation_id", "RuntimeCertificationOperationIdV2"),
        ("attestation_digest", "RuntimeLiveAttestationDigestV2"),
        ("process_identity", "RuntimeProcessIdentityV1"),
        ("lease_epoch", "NonZeroU64"),
        ("revision", "NonZeroU64"),
    ] {
        assert!(serving_identity.contains(&format!("pub {field}: {field_type}")));
    }
    assert_eq!(serving_identity.matches("    pub ").count(), 6);

    let serving_receipt = source
        .split("pub struct RuntimeServingReceiptV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    for (field, field_type) in [
        ("identity", "RuntimeServingIdentityV2"),
        ("acquired_at", "DateTime<Utc>"),
        ("last_heartbeat_at", "DateTime<Utc>"),
        ("expires_at", "DateTime<Utc>"),
        ("connected", "bool"),
        ("serving", "bool"),
    ] {
        assert!(serving_receipt.contains(&format!("pub {field}: {field_type}")));
    }
    assert_eq!(serving_receipt.matches("    pub ").count(), 6);

    let certification_receipt = source
        .split("pub struct RuntimeCertificationReceiptV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    for (field, field_type) in [
        ("action_id", "RuntimeSessionActionIdV1"),
        ("outcome", "TransitionOutcomeV1"),
        ("snapshot", "RuntimeDeploymentSnapshotV1"),
        ("convergence_attempt", "NonZeroU32"),
        ("operation_id", "RuntimeCertificationOperationIdV2"),
        (
            "intent_fingerprint",
            "RuntimeCertificationIntentFingerprintV2",
        ),
        ("request_digest", "RuntimeCertificationRequestDigestV2"),
        ("attestation_digest", "RuntimeLiveAttestationDigestV2"),
        ("route_admission", "RuntimeRouteAdmissionAttestationV2"),
        ("serving", "RuntimeServingReceiptV2"),
        ("certified_at", "DateTime<Utc>"),
    ] {
        assert!(certification_receipt.contains(&format!("pub {field}: {field_type}")));
    }
    assert_eq!(certification_receipt.matches("    pub ").count(), 11);

    let divergence = source
        .split("pub enum RuntimeCertificationDivergenceV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    for variant in [
        "OwnershipLost,",
        "DeploymentAdvanced {",
        "AuthorityChanged {",
        "Superseded {",
        "Terminal {",
        "ReservationMismatch,",
        "CommittedRequestMismatch,",
        "PersistenceCorrupt,",
    ] {
        assert!(divergence.contains(variant), "{variant}");
    }
    assert_eq!(
        divergence
            .lines()
            .filter(|line| {
                line.starts_with("    ") && !line.starts_with("        ") && line.trim() != "},"
            })
            .count(),
        8
    );
    assert_eq!(
        divergence
            .matches("snapshot: RuntimeDeploymentSnapshotV1")
            .count(),
        4
    );

    let disposition = source
        .split("pub enum RuntimeCertificationRecoveryDispositionV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();
    for variant in [
        "StopOwnership,",
        "DrainAndReplan,",
        "DrainAndStop,",
        "EmergencyHalt,",
    ] {
        assert!(disposition.contains(variant), "{variant}");
    }
    assert_eq!(
        disposition
            .lines()
            .filter(|line| line.starts_with("    ") && !line.starts_with("        "))
            .count(),
        4
    );

    let lookup = source
        .split("pub struct RuntimeCertificationLookupV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    for (field, field_type) in [
        ("scope", "RuntimeDeploymentScopeV1"),
        ("deployment_revision", "DeploymentRevision"),
        ("convergence_attempt", "NonZeroU32"),
        ("operation_id", "RuntimeCertificationOperationIdV2"),
        ("request_digest", "RuntimeCertificationRequestDigestV2"),
    ] {
        assert!(lookup.contains(&format!("pub {field}: {field_type}")));
    }
    assert_eq!(lookup.matches("    pub ").count(), 5);

    let exact_observation = source
        .split("pub enum RuntimeCertificationObservationV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    for member in [
        "NotCommitted {",
        "snapshot: RuntimeDeploymentSnapshotV1,",
        "convergence_attempt: NonZeroU32,",
        "operation_id: RuntimeCertificationOperationIdV2,",
        "request_digest: RuntimeCertificationRequestDigestV2,",
        "observed_deployment_revision: DeploymentRevision,",
        "observed_at: DateTime<Utc>,",
        "Committed(RuntimeCertificationReceiptV2),",
        "Diverged(RuntimeCertificationDivergenceV2),",
    ] {
        assert!(exact_observation.contains(member), "{member}");
    }
    assert_eq!(
        exact_observation
            .lines()
            .filter(|line| {
                line.starts_with("    ") && !line.starts_with("        ") && line.trim() != "},"
            })
            .count(),
        3
    );

    let scope_observation = source
        .split("pub enum AwaitingCertificationScopeObservationV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[cfg(test)]").next())
        .unwrap();
    for member in [
        "Committed(RuntimeCertificationReceiptV2),",
        "NoOperationReserved {",
        "NoAttestationForReservedOperation {",
        "reserved_operation_id: RuntimeCertificationOperationIdV2,",
        "Diverged(RuntimeCertificationDivergenceV2),",
    ] {
        assert!(scope_observation.contains(member), "{member}");
    }
    assert_eq!(
        scope_observation
            .lines()
            .filter(|line| {
                line.starts_with("    ") && !line.starts_with("        ") && line.trim() != "},"
            })
            .count(),
        4
    );
    assert_eq!(
        scope_observation
            .matches("snapshot: RuntimeDeploymentSnapshotV1")
            .count(),
        2
    );
    assert_eq!(
        scope_observation
            .matches("observed_at: DateTime<Utc>")
            .count(),
        2
    );

    let library = include_str!("../src/lib.rs");
    for exported in [
        "AwaitingCertificationScopeObservationV2",
        "RuntimeCertificationDivergenceV2",
        "RuntimeCertificationLookupV2",
        "RuntimeCertificationObservationV2",
        "RuntimeCertificationReceiptV2",
        "RuntimeCertificationRecoveryDispositionV2",
        "RuntimeServingIdentityV2",
        "RuntimeServingReceiptV2",
    ] {
        assert!(library.contains(exported), "{exported}");
    }
}

#[test]
fn v2_certification_reservation_derives_and_closes_its_natural_scope() {
    let source = include_str!("../src/v2_certification_operation.rs");
    for forbidden in [
        "serde",
        "Serialize",
        "Deserialize",
        "Default",
        "Sha256",
        "framed_sha256",
        "certification_intent_fingerprint_v2",
        "starring.runtime.",
        "pub fn from_parts",
        "pub fn into_parts",
        "pub fn from_guard",
        "RuntimeCertificationCanonicalRootV2",
        "RuntimeLiveCertificationPortV2",
        "RuntimeCertificationCommitAuthorityV2",
        "Box<RuntimeCertificationDivergenceV2>",
        "Authority",
        "Permit",
        "Future",
        "sqlx",
        "rusqlite",
        "tokio",
        "twilight",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden V2 certification reservation surface: {forbidden}"
        );
    }

    let scope = source
        .split("pub struct RuntimeCertificationOperationScopeV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();
    for field in [
        "scope: RuntimeDeploymentScopeV1,",
        "deployment_revision: DeploymentRevision,",
        "convergence_attempt: NonZeroU32,",
    ] {
        assert!(scope.contains(field), "{field}");
    }
    assert_eq!(scope.matches("    ").count(), 3);
    assert!(!scope.contains("pub "));

    let reserved = source
        .split("pub struct RuntimeReservedCertificationIntentV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();
    assert!(reserved.contains("operation_scope: RuntimeCertificationOperationScopeV2,"));
    assert!(reserved.contains("canonical_intent: RuntimeCanonicalCertificationIntentV2,"));
    assert_eq!(reserved.matches("    ").count(), 2);
    assert!(!reserved.contains("pub "));

    assert!(source.contains("pub fn from_awaiting_execution("));
    assert!(source.contains("validate_execution_receipt(execution)?;"));
    assert!(source.contains("RuntimeDeploymentPhaseV1::AwaitingGatewayReady"));
    assert!(source.contains(concat!(
        "pub fn new(\n",
        "        execution: &RuntimeExecutionReceiptV1,\n",
        "        canonical_intent: RuntimeCanonicalCertificationIntentV2,\n",
        "    ) -> Result<Self, RuntimeCertificationOperationBuildErrorV2> {"
    )));
    assert!(source.contains("validate_intent_against_execution(execution, &canonical_intent)?;"));
    for persisted_input in [
        "persisted_scope: RuntimeDeploymentScopeV1,",
        "persisted_deployment_revision: DeploymentRevision,",
        "persisted_convergence_attempt: NonZeroU32,",
        "persisted_operation_id: &RuntimeCertificationOperationIdV2,",
        "certification_intent_bytes: &[u8],",
        "persisted_fingerprint: &RuntimeCertificationIntentFingerprintV2,",
    ] {
        assert!(source.contains(persisted_input), "{persisted_input}");
    }
    assert!(source.contains("RuntimeCanonicalCertificationIntentV2::from_persisted("));
    assert!(source.contains("persisted_scope != intent.guard.scope"));
    assert!(source.contains("persisted_deployment_revision != intent.guard.expected_revision"));
    assert!(source.contains("persisted_convergence_attempt != intent.guard.convergence_attempt"));
    assert!(source.contains("persisted_operation_id != &intent.operation_id"));
    assert!(source.contains("pub fn require_byte_exact_replay("));
    for replay_field in [
        "self.operation_scope == proposed.operation_scope",
        "self.operation_id() == proposed.operation_id()",
        "self.certification_intent_bytes() == proposed.certification_intent_bytes()",
        "self.intent_fingerprint() == proposed.intent_fingerprint()",
        "Err(RuntimeCertificationDivergenceV2::ReservationMismatch)",
    ] {
        assert!(source.contains(replay_field), "{replay_field}");
    }
    assert!(source.contains(concat!(
        "pub const fn into_divergence(self) -> RuntimeCertificationDivergenceV2 {\n",
        "        RuntimeCertificationDivergenceV2::PersistenceCorrupt"
    )));

    let lookup = source
        .split("pub struct RuntimeCertificationReservationScopeLookupV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();
    assert!(lookup.contains("operation_scope: RuntimeCertificationOperationScopeV2,"));
    assert_eq!(lookup.matches("    ").count(), 1);
    for forbidden in ["operation_id", "fingerprint", "digest"] {
        assert!(!lookup.contains(forbidden), "{forbidden}");
    }

    let observation = source
        .split("pub enum RuntimeCertificationReservationScopeObservationKindV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    for member in [
        "Absent {",
        "lookup: RuntimeCertificationReservationScopeLookupV2,",
        "Reserved {",
        "reservation: RuntimeReservedCertificationIntentV2,",
        "Diverged(RuntimeCertificationDivergenceV2),",
    ] {
        assert!(observation.contains(member), "{member}");
    }
    assert_eq!(
        observation
            .matches("snapshot: RuntimeDeploymentSnapshotV1")
            .count(),
        2
    );
    assert_eq!(
        observation
            .matches("lookup: RuntimeCertificationReservationScopeLookupV2")
            .count(),
        2
    );
    assert_eq!(observation.matches("observed_at: DateTime<Utc>").count(), 2);
    assert_eq!(
        observation
            .lines()
            .filter(|line| {
                line.starts_with("    ") && !line.starts_with("        ") && line.trim() != "},"
            })
            .count(),
        3
    );

    let checked_observation = source
        .split("pub struct RuntimeCertificationReservationScopeObservationV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();
    assert!(checked_observation
        .contains("kind: RuntimeCertificationReservationScopeObservationKindV2,"));
    assert_eq!(checked_observation.matches("    ").count(), 1);
    assert!(!checked_observation.contains("pub "));
    for constructor in ["pub fn absent(", "pub fn reserved(", "pub fn diverged("] {
        assert!(source.contains(constructor), "{constructor}");
    }
    assert_eq!(
        source
            .matches("validate_observation_scope(&snapshot,")
            .count(),
        2
    );
    assert!(source.contains("validate_reservation_against_snapshot(&snapshot, &reservation)?;"));
    assert!(source.contains("lookup.operation_scope() != reservation.operation_scope()"));
    assert!(source.contains("operation_scope.scope.matches(&snapshot.identity)"));
    assert!(source.contains("operation_scope.deployment_revision != snapshot.revision"));

    let outcome = source
        .split("pub enum RuntimeCertificationIntentReservationOutcomeV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nfn").next())
        .unwrap();
    assert!(outcome.contains("Reserved(RuntimeReservedCertificationIntentV2),"));
    assert!(outcome.contains("Diverged(RuntimeCertificationDivergenceV2),"));
    assert_eq!(
        outcome
            .lines()
            .filter(|line| line.starts_with("    ") && !line.starts_with("        "))
            .count(),
        2
    );

    let fields = source
        .split("pub enum RuntimeCertificationOperationFieldV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    for variant in [
        "Scope,",
        "DeploymentRevision,",
        "ConvergenceAttempt,",
        "ControllerId,",
        "FencingToken,",
        "RuntimeGeneration,",
        "Target,",
        "PanelEvidence,",
        "OperationId,",
    ] {
        assert!(fields.contains(variant), "{variant}");
    }
    assert_eq!(
        fields
            .lines()
            .filter(|line| line.starts_with("    ") && !line.starts_with("        "))
            .count(),
        9
    );
    for panel_correlation in [
        "intent.panel.certificate_id == panel.certificate_id",
        "intent.panel.report_digest == panel.report_digest",
        "intent.panel.process_identity.target == panel.target",
        "intent.panel.process_identity.runtime_generation == panel.runtime_generation",
        "intent.panel.process_identity.process_instance_id == panel.process_instance_id",
    ] {
        assert!(source.contains(panel_correlation), "{panel_correlation}");
    }

    let library = include_str!("../src/lib.rs");
    for exported in [
        "RuntimeCertificationIntentReservationOutcomeV2",
        "RuntimeCertificationOperationBuildErrorV2",
        "RuntimeCertificationOperationFieldV2",
        "RuntimeCertificationOperationPersistenceErrorV2",
        "RuntimeCertificationOperationScopeV2",
        "RuntimeCertificationReservationObservationErrorV2",
        "RuntimeCertificationReservationScopeLookupV2",
        "RuntimeCertificationReservationScopeObservationKindV2",
        "RuntimeCertificationReservationScopeObservationV2",
        "RuntimeReservedCertificationIntentV2",
    ] {
        assert!(library.contains(exported), "{exported}");
    }
}

#[test]
fn v2_awaiting_reset_is_checked_closed_and_non_authorizing() {
    let source = include_str!("../src/v2_awaiting_reset.rs");
    for forbidden in [
        "serde",
        "Serialize",
        "Deserialize",
        "Default",
        "Sha256",
        "framed_sha256",
        "canonical_bytes",
        "canonical_json",
        "RuntimeConvergenceMutationV1",
        "Authority",
        "Permit",
        "Port",
        "Future",
        "sqlx",
        "rusqlite",
        "tokio",
        "twilight",
        "drain_intent_absent",
        "pending_drain_count",
        "NoDrainIntentProof",
        "reset_at <",
        "observed_at <",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden Awaiting reset surface: {forbidden}"
        );
    }
    assert!(!source.contains("bool"));

    let basis_kind = source
        .split("pub enum RuntimeAwaitingGatewayReadyResetBasisKindV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    for member in [
        "NoOperationReserved {",
        "NoAttestationForReservedOperation {",
        "snapshot: RuntimeDeploymentSnapshotV1,",
        "reserved_operation_id: RuntimeCertificationOperationIdV2,",
        "observed_at: DateTime<Utc>,",
    ] {
        assert!(basis_kind.contains(member), "{member}");
    }
    assert_eq!(
        basis_kind
            .lines()
            .filter(|line| {
                line.starts_with("    ") && !line.starts_with("        ") && line.trim() != "},"
            })
            .count(),
        2
    );
    assert_eq!(
        basis_kind
            .matches("snapshot: RuntimeDeploymentSnapshotV1")
            .count(),
        2
    );
    assert_eq!(basis_kind.matches("observed_at: DateTime<Utc>").count(), 2);

    let basis = source
        .split("pub struct RuntimeAwaitingGatewayReadyResetBasisV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();
    assert!(basis.contains("kind: RuntimeAwaitingGatewayReadyResetBasisKindV2,"));
    assert_eq!(basis.matches("    ").count(), 1);
    assert!(!basis.contains("pub "));

    let classification = source
        .split("pub enum RuntimeAwaitingGatewayReadyResetClassificationV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();
    for member in [
        "Eligible(RuntimeAwaitingGatewayReadyResetBasisV2),",
        "Committed(RuntimeCertificationReceiptV2),",
        "Diverged(RuntimeCertificationDivergenceV2),",
    ] {
        assert!(classification.contains(member), "{member}");
    }
    assert_eq!(
        classification
            .lines()
            .filter(|line| line.starts_with("    ") && !line.starts_with("        "))
            .count(),
        3
    );
    for source_observation in [
        "AwaitingCertificationScopeObservationV2::Committed(receipt)",
        "AwaitingCertificationScopeObservationV2::NoOperationReserved",
        "AwaitingCertificationScopeObservationV2::NoAttestationForReservedOperation",
        "AwaitingCertificationScopeObservationV2::Diverged(divergence)",
    ] {
        assert!(source.contains(source_observation), "{source_observation}");
    }

    let request = source
        .split("pub struct RuntimeResetAwaitingGatewayReadyV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();
    assert!(request.contains("basis: RuntimeAwaitingGatewayReadyResetBasisV2,"));
    assert_eq!(request.matches("    ").count(), 1);
    assert!(!request.contains("pub "));

    let reservation = source
        .split("pub enum RuntimeCertificationReservationResetReceiptV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    for member in [
        "NotReserved,",
        "Consumed {",
        "operation_id: RuntimeCertificationOperationIdV2,",
        "resulting_revision: DeploymentRevision,",
        "consumed_at: DateTime<Utc>,",
    ] {
        assert!(reservation.contains(member), "{member}");
    }
    assert_eq!(
        reservation
            .lines()
            .filter(|line| {
                line.starts_with("    ") && !line.starts_with("        ") && line.trim() != "},"
            })
            .count(),
        2
    );

    let receipt = source
        .split("pub struct RuntimeAwaitingGatewayReadyResetReceiptV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();
    for field in [
        "outcome: TransitionOutcomeV1,",
        "source_revision: DeploymentRevision,",
        "snapshot: RuntimeDeploymentSnapshotV1,",
        "reservation: RuntimeCertificationReservationResetReceiptV2,",
        "reset_at: DateTime<Utc>,",
    ] {
        assert!(receipt.contains(field), "{field}");
    }
    assert_eq!(receipt.matches("    ").count(), 5);
    assert!(!receipt.contains("pub "));

    let outcome = source
        .split("pub enum RuntimeAwaitingGatewayReadyResetOutcomeV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nfn").next())
        .unwrap();
    for member in [
        "Reset(RuntimeAwaitingGatewayReadyResetReceiptV2),",
        "Committed(RuntimeCertificationReceiptV2),",
        "ProductDrainIntentPresent {",
        "intent_id: RuntimeDrainIntentIdV2",
        "Diverged(RuntimeCertificationDivergenceV2),",
    ] {
        assert!(outcome.contains(member), "{member}");
    }
    assert_eq!(
        outcome
            .lines()
            .filter(|line| {
                line.starts_with("    ") && !line.starts_with("        ") && line.trim() != "},"
            })
            .count(),
        4
    );

    for validation in [
        "RuntimeDeployment::restore(snapshot.clone()).is_err()",
        "RuntimeDeploymentPhaseV1::AwaitingGatewayReady",
        "let fenced_awaiting = snapshot",
        ".is_some_and(|lease|",
        "snapshot.last_fencing_token == Some(lease.fencing_token)",
        "RuntimeDeployment::restore(successor.clone())",
        "source.revision.next()",
        "outcome.revision() != successor.revision",
        "RuntimeDeploymentPhaseV1::ReconcilingPanels",
        "successor.identity != source.identity",
        "successor.target != source.target",
        "successor.runtime_generation != source.runtime_generation",
        "successor.previous_runtime != source.previous_runtime",
        "successor.requested_at != source.requested_at",
        "successor.last_fencing_token != source.last_fencing_token",
        "successor.preflight != source.preflight",
        "successor.drain != source.drain",
        "successor.activation != source.activation",
        "successor.last_live_recovery != source.last_live_recovery",
        "successor.last_runtime_failure != source.last_runtime_failure",
        "successor.panel_certificate.is_some()",
        "successor.gateway_ready.is_some()",
        "successor.live.is_some()",
        "successor.controller_lease.is_some()",
        "operation_id == expected_operation_id",
        "*resulting_revision == successor.revision",
        "*consumed_at == reset_at",
        "RuntimeCertificationDivergenceV2::PersistenceCorrupt",
    ] {
        assert!(source.contains(validation), "{validation}");
    }

    let library = include_str!("../src/lib.rs");
    for exported in [
        "RuntimeAwaitingGatewayReadyResetBasisKindV2",
        "RuntimeAwaitingGatewayReadyResetBasisV2",
        "RuntimeAwaitingGatewayReadyResetClassificationV2",
        "RuntimeAwaitingGatewayReadyResetOutcomeV2",
        "RuntimeAwaitingGatewayReadyResetReceiptErrorV2",
        "RuntimeAwaitingGatewayReadyResetReceiptV2",
        "RuntimeCertificationReservationResetReceiptV2",
        "RuntimeResetAwaitingGatewayReadyV2",
    ] {
        assert!(library.contains(exported), "{exported}");
    }
}

#[test]
fn v2_certification_canonical_surface_stays_closed() {
    let canonical = include_str!("../src/v2_certification_canonical.rs");
    let wire = include_str!("../src/v2_certification_canonical/wire.rs");

    for forbidden in [
        "Serialize",
        "Deserialize",
        "Default",
        "pub intent:",
        "pub bytes:",
        "pub fingerprint:",
        "pub request_digest:",
        "pub request:",
        "pub live_digest:",
        "pub live_record_bytes:",
        "pub fn decode_certification_intent",
        "pub fn encode_certification_intent",
        "pub fn from_parts",
        "pub fn into_parts",
    ] {
        assert!(
            !canonical.contains(forbidden),
            "forbidden V2 certification canonical surface: {forbidden}"
        );
    }
    assert!(canonical.contains("pub struct RuntimeCanonicalCertificationIntentV2"));
    assert!(canonical.contains("intent: RuntimeCertificationIntentV2"));
    assert!(canonical.contains("bytes: Box<[u8]>"));
    assert!(canonical.contains("fingerprint: RuntimeCertificationIntentFingerprintV2"));
    assert!(canonical.contains("pub fn new("));
    assert!(canonical.contains("pub fn from_persisted("));
    assert!(canonical.contains("pub struct RuntimeLiveAttestationRecordV2"));
    assert!(canonical.contains("pub struct RuntimeCanonicalLiveAttestationV2"));
    assert!(canonical.contains("pub fn bind_live_record("));
    assert_eq!(canonical.matches("pub fn from_request(").count(), 1);
    for forbidden in [
        "RuntimeServingIdentityV2",
        "RuntimeServingReceiptV2",
        "RuntimeCertificationReceiptV2",
        "certified_at:",
        "snapshot:",
        "transition:",
        "attestation_digest:",
    ] {
        assert!(
            !canonical.contains(forbidden),
            "forbidden Live preimage field: {forbidden}"
        );
    }

    let record = canonical
        .split("pub struct RuntimeLiveAttestationRecordV2 {")
        .nth(1)
        .and_then(|source| {
            source
                .split("}\n\nimpl RuntimeLiveAttestationRecordV2")
                .next()
        })
        .unwrap();
    assert!(record.contains("request_digest: RuntimeCertificationRequestDigestV2"));
    assert!(record.contains("request: RuntimeCertificationRequestV2"));
    assert_eq!(record.matches("    ").count(), 2);

    let live = canonical
        .split("pub struct RuntimeCanonicalLiveAttestationV2 {")
        .nth(1)
        .and_then(|source| {
            source
                .split("}\n\nimpl RuntimeCanonicalLiveAttestationV2")
                .next()
        })
        .unwrap();
    for field in [
        "reserved_intent",
        "record",
        "request_bytes",
        "live_record_bytes",
        "live_digest",
    ] {
        assert!(live.contains(&format!("    {field}:")));
    }
    assert_eq!(live.matches("    ").count(), 5);

    for forbidden in [
        "pub struct",
        "serde_json::Value",
        "HashMap",
        "BTreeMap",
        "flatten",
        "untagged",
        "rename_all",
        "skip_serializing_if",
        "serde(default",
        "to_vec(intent)",
        "from_slice::<RuntimeCertificationIntentV2>",
    ] {
        assert!(
            !wire.contains(forbidden),
            "forbidden V2 certification intent wire surface: {forbidden}"
        );
    }
    for projection in [
        "struct CertificationIntentWireV2",
        "struct ExecutionGuardWireV2",
        "struct DeploymentScopeWireV2",
        "struct DeploymentTargetWireV2",
        "struct BindingPinWireV2",
        "struct ProcessIdentityWireV2",
        "struct GatewayOwnerLeaseIdWireV2",
        "struct PanelEvidenceWireV2",
        "struct CertificationRequestWireV2",
        "struct RouteAdmissionWireV2",
        "struct BarrierPauseWireV2",
        "struct GatewayReadyWireV2",
        "struct ServingRouteWireV2",
        "struct LiveAttestationRecordWireV2",
    ] {
        assert!(wire.contains(projection));
    }
    assert_eq!(wire.matches("#[serde(deny_unknown_fields)]").count(), 14);

    let intent_wire = wire
        .split("struct CertificationIntentWireV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    let mut previous = 0;
    for field in [
        "format_version",
        "action_id",
        "operation_id",
        "guard",
        "target",
        "binding_pin",
        "process_identity",
        "gateway_owner_lease_id",
        "observed_owner_revision",
        "runtime_build_revision",
        "panel",
        "serving_lease_milliseconds",
    ] {
        let position = intent_wire.find(&format!("    {field}:")).unwrap();
        assert!(position >= previous);
        previous = position;
    }
    assert_eq!(intent_wire.matches("    ").count(), 12);

    let request_wire = wire
        .split("struct CertificationRequestWireV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    let mut previous = 0;
    for field in [
        "format_version",
        "intent",
        "intent_fingerprint",
        "must_commit_before_unix_microseconds",
        "route_admission",
    ] {
        let position = request_wire.find(&format!("    {field}:")).unwrap();
        assert!(position >= previous);
        previous = position;
    }
    assert_eq!(request_wire.matches("    ").count(), 5);

    let live_wire = wire
        .split("struct LiveAttestationRecordWireV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    assert!(live_wire.contains("    format_version:"));
    assert!(live_wire.contains("    request_digest:"));
    assert!(live_wire.contains("    request:"));
    assert_eq!(live_wire.matches("    ").count(), 3);
    assert!(wire.contains(
        "encoded.extend_from_slice(b\"{\\\"format_version\\\":2,\\\"request_digest\\\":\\\"\")"
    ));
    assert!(wire.contains("encoded.extend_from_slice(b\"\\\",\\\"request\\\":\")"));
}

#[test]
fn v2_certification_encoding_reuses_checked_canonical_parts() {
    let canonical = include_str!("../src/v2_certification_canonical.rs");
    let wire = include_str!("../src/v2_certification_canonical/wire.rs");

    assert!(wire.contains("struct CanonicalIntentEncoding"));
    assert!(wire.contains("pub(super) struct CanonicalRequestEncoding"));
    assert!(wire.contains("validate_request(request, &intent.fingerprint)?"));
    assert!(!wire.contains("serde_json::from_slice::<CertificationIntentWireV2>(&intent_bytes)"));
    assert!(!wire.contains("encode_certification_request(record.request())?"));
    assert!(canonical.contains("if record.request.intent != self.intent"));
    assert!(!canonical.contains("record.request.intent.clone()"));
}

#[test]
fn v2_product_and_drain_preimages_stay_inert_and_nonserializable() {
    for (path, source) in [
        ("v2_product.rs", include_str!("../src/v2_product.rs")),
        ("v2_drain.rs", include_str!("../src/v2_drain.rs")),
    ] {
        for forbidden in [
            "serde",
            "Serialize",
            "Deserialize",
            "Default",
            "Sha256",
            "DateTime",
            "canonical_bytes",
        ] {
            assert!(
                !source.contains(forbidden),
                "forbidden inert V2 preimage surface in {path}: {forbidden}"
            );
        }
    }
    let product = include_str!("../src/v2_product.rs");
    assert!(!product.contains("RuntimeProductMutationDigestV2"));
    assert!(!product.contains("RuntimeDrainIntentIdV2"));
    let product_preimage = product
        .split("pub struct RuntimeProductMutationPreimageV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[cfg(test)]").next())
        .unwrap();
    for field in [
        "operation_id",
        "scope",
        "expected_revision",
        "slot",
        "expected_target",
        "mutation_kind",
        "product_semantic_request_digest",
    ] {
        assert!(product_preimage.contains(&format!("pub {field}:")));
    }
    assert_eq!(product_preimage.matches("    pub ").count(), 7);

    let drain = include_str!("../src/v2_drain.rs");
    assert!(!drain.contains("RuntimeDrainIntentDigestV2"));
    assert!(drain.contains("pub fn from_key(key: RuntimeDrainIntentKeyV2) -> Self"));
    let drain_key = drain
        .split("pub struct RuntimeDrainIntentKeyV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    for field in [
        "intent_id",
        "product_operation_id",
        "product_mutation_digest",
        "scope",
        "expected_revision",
        "slot",
        "expected_target",
        "mutation_kind",
    ] {
        assert!(drain_key.contains(&format!("pub {field}:")));
    }
    assert_eq!(drain_key.matches("    pub ").count(), 8);
    let drain_preimage = drain
        .split("pub struct RuntimeDrainIntentPreimageV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();
    assert!(drain_preimage.contains("pub key: RuntimeDrainIntentKeyV2"));
    assert_eq!(drain_preimage.matches("    pub ").count(), 1);
}

#[test]
fn v2_product_drain_canonical_surface_stays_closed_and_purpose_specific() {
    let canonical = include_str!("../src/v2_product_drain_canonical.rs");
    let wire = include_str!("../src/v2_product_drain_canonical/wire.rs");

    for forbidden in [
        "Serialize",
        "Deserialize",
        "Default",
        "pub product_preimage:",
        "pub product_bytes:",
        "pub product_digest:",
        "pub drain_preimage:",
        "pub drain_bytes:",
        "pub drain_digest:",
        "pub fn encode_product_mutation",
        "pub fn decode_product_mutation",
        "pub fn encode_drain_intent",
        "pub fn decode_drain_intent",
    ] {
        assert!(
            !canonical.contains(forbidden),
            "forbidden public canonical aggregate surface: {forbidden}"
        );
    }
    assert!(canonical.contains("pub struct RuntimeCanonicalProductDrainV2"));
    assert!(canonical.contains("product_preimage: RuntimeProductMutationPreimageV2"));
    assert!(canonical.contains("intent_id: RuntimeDrainIntentIdV2"));
    assert!(!canonical.contains(
        "pub fn new(\n        product_preimage: RuntimeProductMutationPreimageV2,\n        drain"
    ));
    assert!(canonical.contains("pub fn from_persisted("));

    for forbidden in [
        "pub struct",
        "serde_json::Value",
        "HashMap",
        "BTreeMap",
        "flatten",
        "untagged",
        "rename_all",
        "skip_serializing_if",
        "serde(default",
        "to_vec(preimage)",
        "from_slice::<RuntimeProductMutationPreimageV2>",
        "from_slice::<RuntimeDrainIntentPreimageV2>",
    ] {
        assert!(
            !wire.contains(forbidden),
            "forbidden V2 Product/drain wire surface: {forbidden}"
        );
    }
    for projection in [
        "struct ProductMutationWireV2",
        "struct DrainIntentWireV2",
        "struct DrainIntentKeyWireV2",
        "struct DeploymentScopeWireV2",
        "struct ServingSlotWireV2",
        "struct DeploymentTargetWireV2",
    ] {
        assert!(wire.contains(projection));
    }
    assert_eq!(wire.matches("#[serde(deny_unknown_fields)]").count(), 6);

    let product_wire = wire
        .split("struct ProductMutationWireV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    for field in [
        "format_version",
        "operation_id",
        "scope",
        "expected_revision",
        "slot",
        "expected_target",
        "mutation_kind",
        "product_semantic_request_digest",
    ] {
        assert!(product_wire.contains(&format!("    {field}:")));
    }
    assert_eq!(product_wire.matches("    ").count(), 8);

    let drain_wire = wire
        .split("struct DrainIntentWireV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    assert!(drain_wire.contains("    format_version:"));
    assert!(drain_wire.contains("    key:"));
    assert_eq!(drain_wire.matches("    ").count(), 2);
}

#[test]
fn source_files_contain_no_comments() {
    let sources = [
        ("src/config.rs", include_str!("../src/config.rs")),
        ("src/dto.rs", include_str!("../src/dto.rs")),
        ("src/failure.rs", include_str!("../src/failure.rs")),
        ("src/lib.rs", include_str!("../src/lib.rs")),
        ("src/planner.rs", include_str!("../src/planner.rs")),
        ("src/port.rs", include_str!("../src/port.rs")),
        ("src/persistence.rs", include_str!("../src/persistence.rs")),
        ("src/retry.rs", include_str!("../src/retry.rs")),
        ("src/session.rs", include_str!("../src/session.rs")),
        ("src/v2_binding.rs", include_str!("../src/v2_binding.rs")),
        (
            "src/v2_canonical_value.rs",
            include_str!("../src/v2_canonical_value.rs"),
        ),
        (
            "src/v2_certification.rs",
            include_str!("../src/v2_certification.rs"),
        ),
        (
            "src/v2_certification_canonical.rs",
            include_str!("../src/v2_certification_canonical.rs"),
        ),
        (
            "src/v2_certification_canonical/wire.rs",
            include_str!("../src/v2_certification_canonical/wire.rs"),
        ),
        (
            "src/v2_certification_canonical/tests.rs",
            include_str!("../src/v2_certification_canonical/tests.rs"),
        ),
        (
            "src/v2_certification_outcome.rs",
            include_str!("../src/v2_certification_outcome.rs"),
        ),
        (
            "src/v2_certification_operation.rs",
            include_str!("../src/v2_certification_operation.rs"),
        ),
        (
            "src/v2_awaiting_reset.rs",
            include_str!("../src/v2_awaiting_reset.rs"),
        ),
        ("src/v2_digest.rs", include_str!("../src/v2_digest.rs")),
        ("src/v2_drain.rs", include_str!("../src/v2_drain.rs")),
        ("src/v2_evidence.rs", include_str!("../src/v2_evidence.rs")),
        ("src/v2_gateway.rs", include_str!("../src/v2_gateway.rs")),
        ("src/v2_identity.rs", include_str!("../src/v2_identity.rs")),
        ("src/v2_product.rs", include_str!("../src/v2_product.rs")),
        (
            "src/v2_product_drain_canonical.rs",
            include_str!("../src/v2_product_drain_canonical.rs"),
        ),
        (
            "src/v2_product_drain_canonical/wire.rs",
            include_str!("../src/v2_product_drain_canonical/wire.rs"),
        ),
        (
            "src/v2_product_drain_canonical/tests.rs",
            include_str!("../src/v2_product_drain_canonical/tests.rs"),
        ),
        ("src/v2_route.rs", include_str!("../src/v2_route.rs")),
        (
            "src/v2_route_provenance.rs",
            include_str!("../src/v2_route_provenance.rs"),
        ),
        (
            "src/v2_startup_recovery.rs",
            include_str!("../src/v2_startup_recovery.rs"),
        ),
        (
            "src/v2_suspension.rs",
            include_str!("../src/v2_suspension.rs"),
        ),
        (
            "src/v2_writer_fence.rs",
            include_str!("../src/v2_writer_fence.rs"),
        ),
        (
            "tests/dependency_guard.rs",
            include_str!("dependency_guard.rs"),
        ),
    ];
    for (path, source) in sources {
        for (index, line) in source.lines().enumerate() {
            let trimmed = line.trim_start();
            assert!(
                !trimmed.starts_with("//")
                    && !trimmed.starts_with("/*")
                    && !trimmed.starts_with('*')
                    && !trimmed.ends_with("*/"),
                "source comment at {path}:{}",
                index + 1
            );
        }
    }
}
