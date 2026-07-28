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
fn v2_drain_claim_evidence_stays_checked_and_non_authorizing() {
    let source = include_str!("../src/v2_drain_claim.rs");
    let tests = include_str!("../src/v2_drain_claim/tests.rs");

    for forbidden in [
        "serde",
        "Serialize",
        "Deserialize",
        "Default",
        "sqlx",
        "rusqlite",
        "twilight",
        "automation_runtime_registry",
        "RuntimeAuthorizedDrainClaimV2",
        "RuntimeClosedDrainRecoveryPermitV2",
        "RuntimeShutdownDrainCompletionPermitV2",
        "pub struct RuntimeDrainClaimSealWitnessV2 {\n    pub ",
        "pub struct RuntimeDrainClaimV2 {\n    pub ",
        "pub struct RuntimeRouteAbsentAcknowledgementV2 {\n    pub ",
        "impl Future",
        "async fn",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden V2 drain-claim evidence surface: {forbidden}"
        );
    }

    for declaration in [
        "pub struct RuntimeDrainClaimSealWitnessV2",
        "pub struct RuntimeDrainClaimProgressV2",
        "pub struct RuntimeDrainClaimV2",
        "pub struct RuntimeDrainCertificationResolutionV2",
        "pub struct RuntimeRouteAbsentAcknowledgementV2",
    ] {
        assert!(source.contains(declaration));
    }

    let claim = source
        .split("pub struct RuntimeDrainClaimV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();
    assert!(claim.contains("key: RuntimeDrainIntentKeyV2,"));
    assert!(!claim.contains("pub "));

    for exact_check in [
        "route.slot() != key.slot",
        "route.identity.target != key.expected_target",
        "route.identity.process_instance_id != process_instance_id",
        "seal.expected_route() != Some(&old_route)",
        "old_route.identity != removal_target.identity",
        "old_route.route_incarnation != removal_target.route_incarnation",
        "removal_target.controller_fencing_token <= old_route.controller_fencing_token",
        "registry_observation_sequence <= seal.registry_observation_sequence()",
        "removal_target.controller_fencing_token != controller_fencing_token",
        "provenance_revision != observed_owner_revision",
        "disconnected_revision == expected",
        "route.identity != serving_identity.process_identity",
        "expected_route.as_ref() != claim.progress().removal_target()",
        "registry_observation_sequence <= refence_sequence",
        "&self.key != key",
    ] {
        assert!(
            source.contains(exact_check),
            "missing V2 drain-claim exact check: {exact_check}"
        );
    }

    for test_name in [
        "seal_binds_the_exact_intent_slot_process_and_route",
        "seal_rejects_wrong_route_slot_target_and_process",
        "refenced_progress_changes_only_to_a_strictly_newer_fence",
        "claim_binds_owner_process_fence_and_progress",
        "claim_rejects_foreign_owner_process_and_wrong_claim_fence",
        "claim_and_acknowledgement_reject_foreign_full_key_fields_with_an_empty_route",
        "certification_resolution_binds_operation_scope_target_and_process",
        "all_certification_resolution_variants_have_closed_views",
        "acknowledgement_accepts_refenced_and_initially_absent_claims",
        "acknowledgement_rejects_wrong_route_and_accepts_a_distinct_removal_barrier",
        "persistence_numbers_and_timestamps_reject_noncanonical_values",
    ] {
        assert!(
            tests.contains(&format!("fn {test_name}(")),
            "missing V2 drain-claim behavior test: {test_name}"
        );
    }
}

#[test]
fn v2_drain_intent_state_is_closed_immutable_and_non_authorizing() {
    let source = include_str!("../src/v2_drain_intent_state.rs");
    let tests = include_str!("../src/v2_drain_intent_state/tests.rs");

    for forbidden in [
        "serde",
        "Serialize",
        "Deserialize",
        "Default",
        "sqlx",
        "rusqlite",
        "twilight",
        "Authority",
        "Permit",
        "Port",
        "RuntimeAuthorizedDrainClaimV2",
        "RuntimeClosedDrainRecoveryPermitV2",
        "RuntimeShutdownDrainCompletionPermitV2",
        "pub value:",
        "pub canonical:",
        "pub intent_revision:",
        "pub state:",
        "pub fn new(",
        "pub fn pending(",
        "pub fn consumed(",
        "pub fn cancelled(",
        ".next()",
        "checked_add",
        "SystemTime",
        "Utc::now",
        "impl Future",
        "async fn",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden V2 drain-intent state surface: {forbidden}"
        );
    }

    for declaration in [
        "pub enum RuntimeDrainIntentStateKindV2",
        "pub struct RuntimeDrainIntentStateV2",
        "pub struct RuntimeDrainIntentV2",
        "pub fn from_inserted(",
        "pub fn pending_from_persisted(",
        "pub fn route_absent_acknowledged_from_persisted(",
        "pub fn consumed_from_persisted(",
        "pub fn cancelled_from_persisted(",
        "pub fn freezes_serving_slot(&self) -> bool",
        "pub fn is_runtime_terminal(&self) -> bool",
    ] {
        assert!(source.contains(declaration), "missing {declaration}");
    }

    for exact_check in [
        "RuntimeDrainIntentStateV2::pending(None)",
        "operation.canonical().clone()",
        "root.canonical().clone()",
        "validate_drain_claim_for_key(claim, key)",
        "validate_route_absent_acknowledgement_for_key(acknowledgement, key)",
        "RuntimePersistenceU64V2::from_u64(value)",
        "RuntimeUnixMicrosecondsV2::from_datetime(value)",
    ] {
        assert!(
            source.contains(exact_check),
            "missing V2 drain-intent state check: {exact_check}"
        );
    }

    for test_name in [
        "inserted_state_is_unclaimed_pending_and_retains_the_exact_canonical_roots",
        "persisted_constructors_restore_all_four_closed_state_variants",
        "persisted_pending_and_acknowledged_states_reject_foreign_evidence",
        "revisions_use_the_full_database_integer_range_without_successor_assumptions",
        "terminal_timestamps_are_canonical_without_host_clock_ordering",
        "state_surface_is_cloneable_data_without_wire_or_mutation_authority",
    ] {
        assert!(
            tests.contains(&format!("fn {test_name}(")),
            "missing V2 drain-intent state behavior test: {test_name}"
        );
    }
}

#[test]
fn v2_drain_intent_canonical_state_is_strict_pure_and_closed() {
    let source = include_str!("../src/v2_drain_intent_canonical_state.rs");
    let wire = include_str!("../src/v2_drain_intent_canonical_state/wire.rs");
    let tests = include_str!("../src/v2_drain_intent_canonical_state/tests.rs");

    for forbidden in [
        "sqlx",
        "rusqlite",
        "twilight",
        "std::fs",
        "std::net",
        "SystemTime",
        "Utc::now",
        "impl Future",
        "async fn",
    ] {
        assert!(!source.contains(forbidden), "{forbidden}");
        assert!(!wire.contains(forbidden), "{forbidden}");
    }

    for declaration in [
        "pub struct RuntimeCanonicalDrainIntentStateV2",
        "pub struct RuntimePersistedUnclaimedPendingDrainIntentV2",
        "pub struct RuntimePersistedRouteAbsenceCandidateDrainIntentV2",
        "pub struct RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2",
        "pub struct RuntimeClosedRecoveryEmptyRegistryPendingDrainClaimTransitionV2",
        "pub struct RuntimeClosedRecoveryPendingDrainAcknowledgementTransitionV2",
        "pub struct RuntimeClosedRecoveryPendingDrainSuccessionAcknowledgementInputV2",
        "pub struct RuntimeClosedRecoveryPendingDrainSuccessionAcknowledgementTransitionV2",
        "const DRAIN_INTENT_STATE_MAX_OCTETS: usize = 1_048_576;",
    ] {
        assert!(source.contains(declaration), "{declaration}");
    }

    for exact_check in [
        "RuntimeCanonicalDrainIntentStateV2::from_persisted(",
        "RuntimeDrainAcknowledgementSourceV2::from_route_absence_candidate(",
        "RuntimeDrainIntentReceiptV2::acknowledged(",
        "RuntimeDrainSuccessionAcknowledgementSourceV2::from_expired_route_absent_claimed(",
        "RuntimeDrainIntentReceiptV2::succession_acknowledged(",
        "RuntimeDrainClaimProgressKindV2::Claimed => None",
        "RuntimeDrainClaimProgressKindV2::Refenced => {",
        "claim.progress().removal_target().cloned()",
        "if encode_state(&intent)? != encoded",
        "#[serde(deny_unknown_fields)]",
        "#[serde(tag = \"kind\", deny_unknown_fields)]",
    ] {
        assert!(
            source.contains(exact_check) || wire.contains(exact_check),
            "{exact_check}"
        );
    }

    assert!(!source.contains("pub struct RuntimeClosedRecoveryPendingDrainTransitionV2"));

    for test_name in [
        "all_state_and_pending_progress_variants_roundtrip_exactly",
        "simple_pending_encoding_is_a_fixed_order_golden",
        "decoder_rejects_unknown_noncanonical_and_mismatched_root_state",
        "pending_subtype_and_nested_provenance_corruption_fail_closed",
        "payload_limit_matches_the_one_mebibyte_execution_frame_cap",
        "closed_recovery_requires_two_persisted_cas_transitions",
        "persisted_refenced_candidate_acknowledges_the_exact_removal_target",
        "routed_claimed_state_is_not_a_route_absence_candidate",
        "closed_recovery_builder_rejects_owner_drift_and_revision_overflow",
        "expired_route_absent_claim_succeeds_directly_with_exact_current_evidence",
        "succession_predecessor_classifier_accepts_only_route_absent_claimed",
        "succession_requires_expired_predecessor_distinct_newer_current_owner",
        "succession_rejects_committed_certification_and_invalid_current_provenance",
        "succession_rejects_each_persistence_successor_overflow",
    ] {
        assert!(
            tests.contains(&format!("fn {test_name}(")),
            "missing V2 drain-intent canonical-state test: {test_name}"
        );
    }
}

#[test]
fn v2_drain_intent_receipts_close_only_structurally_proven_transitions() {
    let source = include_str!("../src/v2_drain_intent_receipt.rs");
    let tests = include_str!("../src/v2_drain_intent_receipt/tests.rs");

    for forbidden in [
        "serde",
        "Serialize",
        "Deserialize",
        "Default",
        "sqlx",
        "rusqlite",
        "twilight",
        "Authority",
        "Permit",
        "Port",
        "RuntimeAuthorizedDrainClaimV2",
        "RuntimeClosedDrainRecoveryPermitV2",
        "RuntimeShutdownDrainCompletionPermitV2",
        "pub outcome:",
        "pub intent:",
        "pub source:",
        "pub fn new(",
        "pub fn from_result(",
        "pub fn claimed(",
        "pub fn claim_initial(",
        "pub fn claim_successor(",
        "pub fn consumed(",
        "pub fn cancelled(",
        ".next()",
        "SystemTime",
        "Instant",
        "Utc::now",
        "next_intent_revision",
        "impl Future",
        "async fn",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden V2 drain-intent receipt surface: {forbidden}"
        );
    }

    for declaration in [
        "pub enum RuntimeDrainIntentMutationOutcomeV2",
        "pub struct RuntimeDrainRefenceSourceV2",
        "pub struct RuntimeDrainAcknowledgementSourceV2",
        "pub struct RuntimeDrainSuccessionAcknowledgementExpectationV2",
        "pub struct RuntimeDrainSuccessionAcknowledgementSourceV2",
        "pub struct RuntimeRouteAbsentDrainIntentSourceV2",
        "pub struct RuntimeDrainIntentReceiptV2",
        "pub fn from_claimed(",
        "pub fn from_route_absence_candidate(",
        "pub fn from_expired_route_absent_claimed(",
        "pub fn from_acknowledged(",
        "pub fn inserted(",
        "pub fn replayed(",
        "pub fn claim_replayed(",
        "pub fn refenced(",
        "pub fn acknowledged(",
        "pub fn succession_acknowledged(",
    ] {
        assert!(source.contains(declaration), "missing {declaration}");
    }

    for exact_check in [
        "operation.canonical() == intent.canonical()",
        "persisted_intent.state().pending_claim().is_some()",
        "persisted_intent != source",
        "source.canonical() == result.canonical()",
        "persisted_intent.intent_revision() <= source.source().intent_revision()",
        "result_claim.claim_revision() <= source_claim.claim_revision()",
        "result_claim.progress().seal() != source_claim.progress().seal()",
        "acknowledgement.claim() != source_claim",
        "source.gateway_owner_lease_id() == result.gateway_owner_lease_id()",
        "source.observed_owner_revision() == result.observed_owner_revision()",
        "source.process_instance_id() == result.process_instance_id()",
        "source.controller_id() == result.controller_id()",
        "source.controller_fencing_token() == result.controller_fencing_token()",
        "source.claim_epoch() == result.claim_epoch()",
        "source.expires_at() == result.expires_at()",
        "current.checked_add(1) == Some(successor)",
    ] {
        assert!(
            source.contains(exact_check),
            "missing V2 drain-intent receipt check: {exact_check}"
        );
    }

    for test_name in [
        "inserted_and_replayed_receipts_bind_the_exact_operation_roots",
        "source_classifiers_accept_only_their_exact_mutable_states",
        "claim_replay_requires_an_exact_unchanged_claimed_aggregate",
        "refence_receipt_allows_only_progress_and_strictly_newer_database_revisions",
        "refence_receipt_rejects_root_state_revision_identity_and_seal_drift",
        "acknowledgement_receipt_accepts_initial_absence_and_durable_refence",
        "acknowledgement_receipt_rejects_root_revision_state_and_claim_drift",
        "transition_receipts_accept_canonical_timestamps_without_host_clock_ordering",
        "succession_receipt_accepts_only_the_exact_atomic_successor",
        "succession_receipt_rejects_root_state_and_intent_revision_drift",
        "succession_receipt_rejects_claim_revision_fence_and_identity_drift",
        "succession_receipt_rejects_seal_provenance_acknowledgement_and_certification_drift",
        "receipt_surface_is_closed_data_without_claim_or_terminal_authority",
    ] {
        assert!(
            tests.contains(&format!("fn {test_name}(")),
            "missing V2 drain-intent receipt behavior test: {test_name}"
        );
    }

    let library = include_str!("../src/lib.rs");
    for exported in [
        "RuntimeDrainAcknowledgementSourceV2",
        "RuntimeDrainIntentMutationOutcomeV2",
        "RuntimeDrainIntentReceiptErrorV2",
        "RuntimeDrainIntentReceiptV2",
        "RuntimeDrainRefenceSourceV2",
        "RuntimeDrainSuccessionAcknowledgementExpectationV2",
        "RuntimeDrainSuccessionAcknowledgementSourceV2",
        "RuntimeRouteAbsentDrainIntentSourceV2",
    ] {
        assert!(library.contains(exported), "missing export {exported}");
    }
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
fn v2_product_drain_operations_bind_natural_scopes_and_byte_exact_replay() {
    let source = include_str!("../src/v2_product_drain_operation.rs");
    let tests = include_str!("../src/v2_product_drain_operation/tests.rs");

    for forbidden in [
        "serde",
        "Serialize",
        "Deserialize",
        "Default",
        "sqlx",
        "rusqlite",
        "twilight",
        "RuntimeExecutionConvergencePort",
        "RuntimeServingLeasePort",
        "RuntimeDrainIntentStateV2",
        "RuntimeDrainClaimV2",
        "pub scope:",
        "pub expected_revision:",
        "pub slot:",
        "pub canonical:",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden V2 Product drain operation surface: {forbidden}"
        );
    }

    for declaration in [
        "pub struct RuntimeProductOperationScopeV2",
        "pub struct RuntimeDrainIntentOperationScopeV2",
        "pub struct RuntimeProductDrainOperationV2",
        "pub struct RuntimePersistedProductDrainRootV2",
        "pub struct RuntimeProductDrainScopeLookupV2",
    ] {
        assert!(source.contains(declaration));
    }
    assert!(source.contains("RuntimeDeployment::restore(snapshot.clone())"));
    assert!(source.contains("RuntimeDeploymentScopeV1::from_identity(&snapshot.identity)"));
    assert!(source.contains("RuntimeServingSlotV2::from_target(&snapshot.target)"));
    assert!(source.contains("product.scope != expected_scope"));
    assert!(source.contains("product.expected_revision != snapshot.revision"));
    assert!(source.contains("product.slot != expected_slot"));
    assert!(source.contains("product.expected_target != snapshot.target"));
    assert!(source.contains("RuntimeCanonicalProductDrainV2::from_persisted("));

    for comparison in [
        "persisted_product_scope != &product.scope",
        "persisted_product_expected_revision != product.expected_revision",
        "persisted_product_operation_id != &product.operation_id",
        "persisted_drain_scope != &drain.scope",
        "persisted_drain_slot != &drain.slot",
        "persisted_drain_expected_revision != drain.expected_revision",
        "persisted_drain_intent_id != &drain.intent_id",
        "persisted_expected_target != &product.expected_target",
        "self.product_operation_scope == *proposed.product_operation_scope()",
        "self.drain_intent_scope == *proposed.drain_intent_scope()",
        "self.product_operation_id() == proposed.product_operation_id()",
        "self.drain_intent_id() == proposed.drain_intent_id()",
        "self.product_mutation_request_bytes()",
        "== proposed.product_mutation_request_bytes()",
        "self.product_mutation_digest() == proposed.product_mutation_digest()",
        "self.drain_intent_request_bytes() == proposed.drain_intent_request_bytes()",
        "self.drain_intent_digest() == proposed.drain_intent_digest()",
    ] {
        assert!(
            source.contains(comparison),
            "missing exact check: {comparison}"
        );
    }

    let lookup = source
        .split("pub struct RuntimeProductDrainScopeLookupV2 {")
        .nth(1)
        .and_then(|source| source.split("fn validate_snapshot(").next())
        .unwrap();
    for forbidden in [
        "RuntimeProductOperationIdV2",
        "RuntimeDrainIntentIdV2",
        "RuntimeProductMutationDigestV2",
        "RuntimeDrainIntentDigestV2",
        "canonical",
    ] {
        assert!(
            !lookup.contains(forbidden),
            "scope-only lookup contains {forbidden}"
        );
    }

    for test_name in [
        "operation_binds_both_exact_natural_scopes_to_the_locked_snapshot",
        "operation_rejects_an_invalid_locked_snapshot",
        "operation_rejects_every_locked_row_root_mismatch",
        "scope_lookup_contains_only_the_two_natural_scopes",
        "persisted_root_reconstructs_both_exact_roots_and_normalized_scopes",
        "persisted_root_rejects_every_normalized_identity_mismatch",
        "persisted_root_rejects_canonical_corruption_in_either_root",
        "byte_exact_replay_accepts_only_the_original_scopes_ids_bytes_and_digests",
    ] {
        assert!(
            tests.contains(&format!("fn {test_name}(")),
            "missing Product drain operation behavior test: {test_name}"
        );
    }
}

#[test]
fn v2_product_drain_scope_observation_is_combined_exact_and_non_authorizing() {
    let source = include_str!("../src/v2_product_drain_observation.rs");
    let tests = include_str!("../src/v2_product_drain_observation/tests.rs");

    for forbidden in [
        "serde",
        "Serialize",
        "Deserialize",
        "Default",
        "sqlx",
        "rusqlite",
        "twilight",
        "Port",
        "Authority",
        "Permit",
        "Authorized",
        "RuntimeProductOperationIdV2",
        "RuntimeDrainIntentIdV2",
        "pub root:",
        "pub intent:",
        "pub lookup:",
        "pub locked_snapshot:",
        "pub observed_at:",
        "pub state:",
        "pub fn new(",
        "retry",
        "Retry",
        "transaction",
        "Transaction",
        "SystemTime",
        "Instant",
        "Utc::now",
        "impl Future",
        "async fn",
        "rand",
        "Uuid",
        "CSPRNG",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden V2 Product drain observation surface: {forbidden}"
        );
    }

    for declaration in [
        "pub enum RuntimeProductDrainNaturalScopeV2",
        "pub enum RuntimeProductDrainScopeCorruptionV2",
        "pub enum RuntimeProductDrainScopeObservationKindV2",
        "pub enum RuntimeProductDrainScopeObservationFieldV2",
        "pub enum RuntimeProductDrainScopeObservationErrorV2",
        "pub struct RuntimeObservedProductDrainV2",
        "pub struct RuntimeProductDrainScopeObservationV2",
        "pub fn from_exact_parts(",
        "pub fn require_byte_exact_replay(",
        "pub fn absent(",
        "pub fn present(",
        "pub fn persistence_corrupt(",
        "pub fn into_persisted(",
    ] {
        assert!(source.contains(declaration), "missing {declaration}");
    }

    for exact_check in [
        "root.canonical() != intent.canonical()",
        "RuntimeDeployment::restore(snapshot.clone())",
        "product.scope() != &expected_scope",
        "product.expected_revision() != snapshot.revision",
        "drain.scope() != &expected_scope",
        "drain.slot() != &RuntimeServingSlotV2::from_target(&snapshot.target)",
        "drain.expected_revision() != snapshot.revision",
        "root.product_operation_scope().scope()",
        "!= lookup.product_operation_scope().scope()",
        "root.product_operation_scope().expected_revision()",
        "!= lookup.product_operation_scope().expected_revision()",
        "root.drain_intent_scope().scope() != lookup.drain_intent_scope().scope()",
        "root.drain_intent_scope().slot() != lookup.drain_intent_scope().slot()",
        "root.drain_intent_scope().expected_revision()",
        "!= lookup.drain_intent_scope().expected_revision()",
        "root.canonical().product_preimage().expected_target != snapshot.target",
        "RuntimeUnixMicrosecondsV2::from_datetime(observed_at)",
        "Ambiguous(RuntimeProductDrainNaturalScopeV2)",
        "RuntimeProductDrainScopeObservationKindV2::PersistenceCorrupt",
    ] {
        assert!(
            source.contains(exact_check),
            "missing V2 Product drain observation check: {exact_check}"
        );
    }

    for test_name in [
        "observed_pair_requires_exact_root_and_intent_canonical_identity",
        "absent_observation_binds_the_combined_lookup_snapshot_and_database_time",
        "present_observation_adopts_all_four_persisted_mutable_states",
        "lookup_validation_rejects_invalid_or_different_locked_snapshots",
        "present_observation_rejects_each_reachable_root_snapshot_mismatch",
        "every_physical_corruption_classification_is_closed_and_inert",
        "ambiguous_scope_classification_never_selects_a_first_row",
        "observed_time_is_canonical_without_host_clock_ordering",
        "observed_pair_replay_is_exact_and_does_not_change_the_adopted_identity",
        "combined_observation_surface_has_no_retry_or_persistence_authority",
    ] {
        assert!(
            tests.contains(&format!("fn {test_name}(")),
            "missing V2 Product drain observation behavior test: {test_name}"
        );
    }

    let ambiguous_test = tests
        .split("fn ambiguous_scope_classification_never_selects_a_first_row()")
        .nth(1)
        .and_then(|source| source.split("#[test]").next())
        .unwrap();
    assert!(ambiguous_test.contains("RuntimeProductDrainNaturalScopeV2::ProductOperation"));
    assert!(ambiguous_test.contains("RuntimeProductDrainNaturalScopeV2::DrainIntent"));
    assert!(ambiguous_test.contains("RuntimeProductDrainScopeCorruptionV2::Ambiguous(scope)"));
    assert!(ambiguous_test.contains("observation.persisted().is_none()"));
    assert!(ambiguous_test.contains("observation.into_persisted().is_none()"));

    let library = include_str!("../src/lib.rs");
    for exported in [
        "RuntimeObservedProductDrainV2",
        "RuntimeProductDrainNaturalScopeV2",
        "RuntimeProductDrainScopeCorruptionV2",
        "RuntimeProductDrainScopeObservationErrorV2",
        "RuntimeProductDrainScopeObservationFieldV2",
        "RuntimeProductDrainScopeObservationKindV2",
        "RuntimeProductDrainScopeObservationV2",
    ] {
        assert!(library.contains(exported), "missing export {exported}");
    }
}

#[test]
fn v2_product_drain_semantic_adoption_is_id_free_closed_and_non_authorizing() {
    let source = include_str!("../src/v2_product_drain_adoption.rs");
    let tests = include_str!("../src/v2_product_drain_adoption/tests.rs");
    let expectation = source
        .split("pub struct RuntimeProductDrainSemanticExpectationV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();

    for forbidden in [
        "RuntimeProductOperationIdV2",
        "RuntimeDrainIntentIdV2",
        "RuntimeProductMutationDigestV2",
        "RuntimeDrainIntentDigestV2",
        "RuntimeCanonicalProductDrainV2",
        "RuntimePersistedProductDrainRootV2",
        "request_bytes",
        "canonical",
    ] {
        assert!(
            !expectation.contains(forbidden),
            "semantic expectation contains {forbidden}"
        );
    }
    for required in [
        "lookup: RuntimeProductDrainScopeLookupV2",
        "expected_target: RuntimeDeploymentTargetV1",
        "mutation_kind: RuntimeProductMutationKindV2",
        "product_semantic_request_digest: RuntimeProductSemanticRequestDigestV2",
    ] {
        assert!(
            expectation.contains(required),
            "semantic expectation is missing {required}"
        );
    }

    for forbidden in [
        "serde",
        "Serialize",
        "Deserialize",
        "Default",
        "sqlx",
        "rusqlite",
        "twilight",
        "Port",
        "Authority",
        "Permit",
        "Authorized",
        "pub expectation:",
        "pub observation:",
        "pub state:",
        "pub fn new(",
        "retry",
        "Retry",
        "transaction",
        "Transaction",
        "SystemTime",
        "Instant",
        "Utc::now",
        "impl Future",
        "async fn",
        "rand",
        "Uuid",
        "CSPRNG",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden V2 Product drain adoption surface: {forbidden}"
        );
    }

    for declaration in [
        "pub struct RuntimeProductDrainSemanticExpectationV2",
        "pub enum RuntimeProductDrainAdoptionKindV2",
        "pub enum RuntimeProductDrainSemanticFieldV2",
        "pub enum RuntimeProductDrainAdoptionDivergenceV2",
        "pub enum RuntimeProductDrainAdoptionErrorV2",
        "pub struct RuntimeProductDrainAdoptionV2",
        "pub fn from_locked_snapshot(",
        "pub fn from_proposed(",
        "pub fn classify_proposed(",
        "pub fn classify_semantic_recovery(",
        "pub fn into_observation(",
    ] {
        assert!(source.contains(declaration), "missing {declaration}");
    }

    for exact_check in [
        "RuntimeProductDrainScopeLookupV2::from_locked_snapshot(snapshot)?",
        "lookup: proposed.scope_lookup()",
        "expected_target: product.expected_target.clone()",
        "mutation_kind: product.mutation_kind",
        "product_semantic_request_digest: product.product_semantic_request_digest.clone()",
        "expectation.lookup() != observation.lookup()",
        "RuntimeProductDrainAdoptionErrorV2::ObservationLookupMismatch",
        "RuntimeProductDrainAdoptionDivergenceV2::PersistenceCorrupt { corruption }",
        "semantic_mismatch(&expectation, persisted)",
        "persisted.require_byte_exact_replay(proposed).is_err()",
        "RuntimeProductDrainAdoptionDivergenceV2::CanonicalMismatch",
        "RuntimeProductDrainAdoptionStateV2::ExactProposedRoot",
        "RuntimeProductDrainAdoptionStateV2::PersistedRoot",
        "&product.expected_target != expectation.expected_target()",
        "product.mutation_kind != expectation.mutation_kind()",
        "&product.product_semantic_request_digest",
        "!= expectation.product_semantic_request_digest()",
    ] {
        assert!(
            source.contains(exact_check),
            "missing V2 Product drain adoption check: {exact_check}"
        );
    }

    for test_name in [
        "semantic_expectation_is_id_free_and_derives_only_locked_product_inputs",
        "exact_proposed_root_classification_preserves_the_complete_observation",
        "same_semantics_with_different_proposed_ids_is_canonical_mismatch",
        "proposed_path_reports_semantic_mismatch_before_canonical_mismatch",
        "semantic_recovery_adopts_persisted_ids_roots_and_all_current_states",
        "semantic_recovery_reports_each_id_independent_mismatch_precisely",
        "classification_rejects_an_observation_from_another_natural_lookup",
        "absent_observation_stays_absent_in_both_classification_paths",
        "every_physical_corruption_reason_is_preserved_as_divergence",
        "semantic_adoption_surface_is_inert_id_free_and_non_authorizing",
    ] {
        assert!(
            tests.contains(&format!("fn {test_name}(")),
            "missing V2 Product drain adoption behavior test: {test_name}"
        );
    }

    let semantic_check = source
        .find("semantic_mismatch(&expectation, persisted)")
        .unwrap();
    let canonical_check = source
        .find("persisted.require_byte_exact_replay(proposed).is_err()")
        .unwrap();
    assert!(semantic_check < canonical_check);

    let library = include_str!("../src/lib.rs");
    for exported in [
        "RuntimeProductDrainAdoptionDivergenceV2",
        "RuntimeProductDrainAdoptionErrorV2",
        "RuntimeProductDrainAdoptionKindV2",
        "RuntimeProductDrainAdoptionV2",
        "RuntimeProductDrainSemanticExpectationV2",
        "RuntimeProductDrainSemanticFieldV2",
    ] {
        assert!(library.contains(exported), "missing export {exported}");
    }
}

#[test]
fn v2_suspension_canonical_surface_stays_closed_and_purpose_specific() {
    let canonical = include_str!("../src/v2_suspension_canonical.rs");
    let wire = include_str!("../src/v2_suspension_canonical/wire.rs");
    let tests = include_str!("../src/v2_suspension_canonical/tests.rs");

    for forbidden in [
        "serde",
        "Serialize",
        "Deserialize",
        "Default",
        "sqlx",
        "rusqlite",
        "twilight",
        "RuntimeExecutionConvergencePort",
        "RuntimeServingLeasePort",
        "RuntimeCertificationCommitAuthority",
        "RuntimeSuspendAttemptAuthority",
        "Permit",
        "Future",
        "pub request:",
        "pub bytes:",
        "pub digest:",
        "pub fn encode",
        "pub fn decode",
        "pub fn from_parts",
        "pub fn into_parts",
    ] {
        assert!(
            !canonical.contains(forbidden),
            "forbidden suspension canonical surface: {forbidden}"
        );
    }

    let aggregate = canonical
        .split("pub struct RuntimeCanonicalSuspendAttemptV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();
    for field in [
        "request: RuntimeSuspendAttemptRequestV2,",
        "bytes: Box<[u8]>,",
        "digest: RuntimeSuspendAttemptDigestV2,",
    ] {
        assert!(aggregate.contains(field), "{field}");
    }
    assert_eq!(aggregate.matches("    ").count(), 3);
    assert!(!aggregate.contains("pub "));

    assert!(canonical.contains("const SUSPEND_ATTEMPT_MAX_OCTETS: usize = 131_072;"));
    assert_eq!(canonical.matches("suspend_attempt_digest_v2(").count(), 2);
    assert!(canonical.contains(concat!(
        "pub fn new(\n",
        "        request: RuntimeSuspendAttemptRequestV2,\n",
        "    ) -> Result<Self, RuntimeSuspendAttemptCanonicalErrorV2> {"
    )));
    assert!(canonical.contains(concat!(
        "pub fn from_persisted(\n",
        "        bytes: &[u8],\n",
        "        persisted_digest: &RuntimeSuspendAttemptDigestV2,\n",
        "    ) -> Result<Self, RuntimeSuspendAttemptCanonicalErrorV2> {"
    )));
    assert!(!canonical.contains(
        "pub fn new(\n        request: RuntimeSuspendAttemptRequestV2,\n        digest:"
    ));
    assert!(
        canonical.contains("if request.source_phase.required_checkpoint() != request.checkpoint")
    );
    assert!(canonical.contains("RuntimeSuspendAttemptCorrelationV2::FailureDispositionTime"));
    assert!(canonical.contains("RuntimeSuspendAttemptCorrelationV2::LocalRouteRuntimeGeneration"));
    assert!(
        canonical.contains("RuntimeSuspendAttemptCorrelationV2::LocalRouteControllerFencingToken")
    );
    assert!(canonical.contains("RuntimeSuspendAttemptCorrelationV2::PreviousServingProductScope"));
    assert!(
        canonical.contains("RuntimeSuspendAttemptCorrelationV2::PreviousServingRuntimeGeneration")
    );
    assert!(canonical.contains("RuntimeSuspendAttemptCorrelationV2::RouteProvenanceProcess"));
    assert!(canonical.contains("RuntimeSuspendAttemptCorrelationV2::RouteProvenanceGeneration"));
    assert!(canonical.contains("RuntimeSuspendAttemptCorrelationV2::RouteProvenanceSequence"));
    assert_eq!(
        canonical
            .matches("RuntimeLocalRouteEffectV2::None,")
            .count(),
        2
    );
    assert_eq!(
        canonical
            .matches("RuntimeLocalRouteEffectV2::ExactRoute")
            .count(),
        2
    );
    assert_eq!(
        canonical
            .matches("RuntimeLocalRouteEffectV2::RouteAbsent")
            .count(),
        2
    );
    assert!(canonical.contains(concat!(
        "_ => {\n",
        "            return Err(correlation(\n",
        "                RuntimeSuspendAttemptCorrelationV2::LocalEffectDrainObligation,\n",
        "            ));\n",
        "        }"
    )));

    for forbidden in [
        "pub struct",
        "pub enum",
        "serde_json::Value",
        "HashMap",
        "BTreeMap",
        "flatten",
        "untagged",
        "rename_all",
        "alias",
        "skip_serializing_if",
        "serde(default",
        "to_vec(request)",
        "from_slice::<RuntimeSuspendAttemptRequestV2>",
    ] {
        assert!(
            !wire.contains(forbidden),
            "forbidden V2 suspension wire surface: {forbidden}"
        );
    }
    for projection in [
        "struct SuspendAttemptWireV2",
        "struct ExecutionGuardWireV2",
        "struct DeploymentScopeWireV2",
        "struct FailureWireV2",
        "enum AttemptDispositionWireV2",
        "struct ExactLocalRouteWireV2",
        "struct ProcessIdentityWireV2",
        "struct DeploymentTargetWireV2",
        "struct ServingSlotWireV2",
        "struct PreviousServingIdentityWireV2",
        "enum DrainObligationWireV2",
        "enum LocalRouteEffectWireV2",
        "enum RouteMutationProvenanceWireV2",
        "struct BarrierPauseWireV2",
        "struct ClosedRecoveryRouteWitnessWireV2",
        "struct ShutdownRouteWitnessWireV2",
        "struct GatewayOwnerLeaseIdWireV2",
    ] {
        assert!(wire.contains(projection), "{projection}");
    }
    assert_eq!(wire.matches("#[serde(deny_unknown_fields)]").count(), 13);
    assert_eq!(
        wire.matches("#[serde(tag = \"kind\", deny_unknown_fields)]")
            .count(),
        4
    );

    let root = wire
        .split("struct SuspendAttemptWireV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    for field in [
        "format_version",
        "suspension_id",
        "action_id",
        "guard",
        "source_phase",
        "failure",
        "disposition",
        "checkpoint",
        "local_effect",
        "drain_obligation",
    ] {
        assert!(root.contains(&format!("    {field}:")), "{field}");
    }
    assert_eq!(root.matches("    ").count(), 10);
    assert!(root.find("format_version").unwrap() < root.find("suspension_id").unwrap());
    assert!(root.find("suspension_id").unwrap() < root.find("action_id").unwrap());
    assert!(root.find("action_id").unwrap() < root.find("guard").unwrap());
    assert!(root.find("guard").unwrap() < root.find("source_phase").unwrap());
    assert!(root.find("source_phase").unwrap() < root.find("failure").unwrap());
    assert!(root.find("failure").unwrap() < root.find("disposition").unwrap());
    assert!(root.find("disposition").unwrap() < root.find("checkpoint").unwrap());
    assert!(root.find("checkpoint").unwrap() < root.find("local_effect").unwrap());
    assert!(root.find("local_effect").unwrap() < root.find("drain_obligation").unwrap());

    for tag in [
        "retryable",
        "blocked",
        "none",
        "exact_local_route",
        "previous_serving",
        "local_and_previous",
        "exact_route",
        "route_absent",
        "ordinary",
        "closed_recovery",
        "shutdown",
    ] {
        assert!(
            wire.contains(&format!("#[serde(rename = \"{tag}\")]")),
            "{tag}"
        );
    }
    assert_eq!(wire.matches("#[serde(rename = \"none\")]").count(), 2);
    assert!(wire.contains("#[serde(deserialize_with = \"deserialize_required_option\")]"));
    assert!(wire.contains("fn deserialize_required_option<'de, D, T>"));
    assert!(wire.contains("ensure_size(encoded)?;\n    let wire = serde_json::from_slice"));
    assert!(wire.contains("if canonical != encoded"));
    assert!(
        wire.contains("return Err(RuntimeSuspendAttemptCanonicalErrorV2::NonCanonicalEncoding);")
    );
    assert!(wire.contains("encoded.len() > SUSPEND_ATTEMPT_MAX_OCTETS"));
    for required in [
        "simple_root_matches_the_exact_byte_and_independent_digest_golden",
        "nested_payload_variants_match_exact_byte_goldens",
        "string_escaping_has_exact_utf8_json_and_digest_goldens",
        "independent_suspend_digest",
        "expected_closed_recovery_provenance_json",
        "expected_shutdown_provenance_json",
    ] {
        assert!(tests.contains(required), "{required}");
    }

    let library = include_str!("../src/lib.rs");
    for exported in [
        "RuntimeCanonicalSuspendAttemptV2",
        "RuntimeSuspendAttemptCanonicalErrorV2",
        "RuntimeSuspendAttemptCanonicalFieldV2",
        "RuntimeSuspendAttemptCorrelationV2",
    ] {
        assert!(library.contains(exported), "{exported}");
    }
}

#[test]
fn v2_suspension_operation_binds_one_exact_execution_without_authority() {
    let source = include_str!("../src/v2_suspension_operation.rs");
    let tests = include_str!("../src/v2_suspension_operation/tests.rs");
    let evidence = include_str!("../src/v2_suspension.rs");

    for forbidden in [
        "serde",
        "Serialize",
        "Deserialize",
        "Default",
        "Sha256",
        "framed_sha256",
        "canonical_bytes",
        "canonical_json",
        "sqlx",
        "rusqlite",
        "tokio",
        "twilight",
        "Authority",
        "Permit",
        "Port",
        "Future",
        "pub fn from_parts",
        "pub fn from_guard",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden suspension operation surface: {forbidden}"
        );
    }

    let scope = source
        .split("pub struct RuntimeSuspendAttemptOperationScopeV2 {")
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

    let operation = source
        .split("pub struct RuntimeSuspendAttemptOperationV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();
    for field in [
        "operation_scope: RuntimeSuspendAttemptOperationScopeV2,",
        "source_target: RuntimeDeploymentTargetV1,",
        "source_previous_runtime: Option<RuntimeProcessIdentityV1>,",
        "source_evidence_at: DateTime<Utc>,",
        "canonical_attempt: RuntimeCanonicalSuspendAttemptV2,",
    ] {
        assert!(operation.contains(field), "{field}");
    }
    assert_eq!(operation.matches("    ").count(), 5);
    assert!(!operation.contains("pub "));

    let persisted = source
        .split("pub struct RuntimePersistedSuspendAttemptRootV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();
    for field in [
        "operation_scope: RuntimeSuspendAttemptOperationScopeV2,",
        "canonical_attempt: RuntimeCanonicalSuspendAttemptV2,",
    ] {
        assert!(persisted.contains(field), "{field}");
    }
    assert_eq!(persisted.matches("    ").count(), 2);
    assert!(!persisted.contains("pub "));

    let lookup = source
        .split("pub struct RuntimeSuspendAttemptScopeLookupV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();
    assert!(lookup.contains("operation_scope: RuntimeSuspendAttemptOperationScopeV2,"));
    assert_eq!(lookup.matches("    ").count(), 1);
    for forbidden in ["suspension_id", "digest", "bytes", "canonical_attempt"] {
        assert!(!lookup.contains(forbidden), "{forbidden}");
    }

    assert!(source.contains(concat!(
        "pub fn new(\n",
        "        execution: &RuntimeExecutionReceiptV1,\n",
        "        canonical_attempt: RuntimeCanonicalSuspendAttemptV2,\n",
        "    ) -> Result<Self, RuntimeSuspendAttemptOperationBuildErrorV2> {"
    )));
    assert!(source.contains(concat!(
        "pub fn from_persisted(\n",
        "        persisted_scope: RuntimeDeploymentScopeV1,\n",
        "        persisted_deployment_revision: DeploymentRevision,\n",
        "        persisted_convergence_attempt: NonZeroU32,\n",
        "        persisted_suspension_id: &RuntimeSuspensionIdV2,"
    )));
    assert!(source.contains("RuntimeDeployment::restore(execution.snapshot.clone())"));
    for receipt_check in [
        "lease.controller_id != execution.controller_id",
        "lease.fencing_token != execution.fencing_token",
        "lease.acquired_at != execution.acquired_at",
        "lease.expires_at != execution.expires_at",
        "execution.snapshot.last_fencing_token != Some(execution.fencing_token)",
        "execution.expires_at <= execution.acquired_at",
    ] {
        assert!(source.contains(receipt_check), "{receipt_check}");
    }

    for phase in [
        "RuntimeSuspensionSourcePhaseV2::Requested",
        "RuntimeSuspensionSourcePhaseV2::PreflightReady",
        "RuntimeSuspensionSourcePhaseV2::DrainRequested",
        "RuntimeSuspensionSourcePhaseV2::Drained",
        "RuntimeSuspensionSourcePhaseV2::ActivationApplying",
        "RuntimeSuspensionSourcePhaseV2::RuntimePendingReady",
        "RuntimeSuspensionSourcePhaseV2::ReconcilingPanels",
    ] {
        assert!(evidence.contains(phase), "{phase}");
    }
    assert!(source.contains("guard.scope != expected_scope"));
    assert!(source.contains("guard.expected_revision != execution.snapshot.revision"));
    assert!(source.contains("guard.convergence_attempt != execution.convergence_attempt"));
    assert!(source.contains("guard.controller_id != execution.controller_id"));
    assert!(source.contains("guard.fencing_token != execution.fencing_token"));
    assert!(source.contains("guard.runtime_generation != execution.snapshot.runtime_generation"));
    assert!(source.contains("suspension_current_target_matches("));
    assert!(source.contains("suspension_previous_runtime_matches("));
    assert!(evidence.contains("route.identity.target == *target"));
    assert!(evidence.contains("previous_runtime == Some(&previous.process)"));
    assert!(evidence.contains("previous.process.target.same_slot(target)"));
    assert!(source.contains("request.failure.recorded_at < evidence_at"));
    assert!(source.contains("suspension_source_evidence_at(&execution.snapshot, expected_phase)"));
    assert!(evidence.contains("snapshot.requested_at"));
    assert!(evidence.contains("preflight.checked_at"));
    assert!(evidence.contains("drain.drained_at"));
    assert!(evidence.contains("activation.activated_at.max(recovery.recovered_at)"));
    assert!(evidence.contains(".last_live_recovery"));

    for persisted_check in [
        "persisted_scope != request.guard.scope",
        "persisted_deployment_revision != request.guard.expected_revision",
        "persisted_convergence_attempt != request.guard.convergence_attempt",
        "persisted_suspension_id != &request.suspension_id",
        "RuntimeCanonicalSuspendAttemptV2::from_persisted",
    ] {
        assert!(source.contains(persisted_check), "{persisted_check}");
    }
    for replay_check in [
        "self.operation_scope == *proposed.operation_scope()",
        "self.suspension_id() == proposed.suspension_id()",
        "self.suspend_attempt_request_bytes() == proposed.suspend_attempt_request_bytes()",
        "self.suspend_attempt_digest() == proposed.suspend_attempt_digest()",
    ] {
        assert!(source.contains(replay_check), "{replay_check}");
    }

    for required_test in [
        "all_suspendable_phases_bind_their_exact_source_evidence",
        "execution_receipt_and_guard_drift_are_rejected",
        "current_target_and_previous_runtime_drift_are_rejected",
        "persisted_root_requires_exact_normalized_scope_and_identity",
        "byte_exact_replay_rejects_every_creation_identity_difference",
    ] {
        assert!(tests.contains(required_test), "{required_test}");
    }

    let library = include_str!("../src/lib.rs");
    for exported in [
        "RuntimePersistedSuspendAttemptRootV2",
        "RuntimeSuspendAttemptOperationScopeV2",
        "RuntimeSuspendAttemptOperationV2",
        "RuntimeSuspendAttemptScopeLookupV2",
    ] {
        assert!(library.contains(exported), "{exported}");
    }
}

#[test]
fn v2_suspension_sidecar_allows_only_exact_local_absence_progress() {
    let source = include_str!("../src/v2_suspension_sidecar.rs");
    let canonical = include_str!("../src/v2_suspension_canonical.rs");
    let wire = include_str!("../src/v2_suspension_canonical/wire.rs");
    let tests = include_str!("../src/v2_suspension_sidecar/tests.rs");

    for forbidden in [
        "serde",
        "Serialize",
        "Deserialize",
        "Default",
        "Sha256",
        "framed_sha256",
        "sqlx",
        "rusqlite",
        "tokio",
        "twilight",
        "Authority",
        "Permit",
        "Port",
        "Future",
        "pub fn new(",
        "pub fn from_parts",
        "pub fn into_parts",
        "pub fn begin_local_drain",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden suspension sidecar surface: {forbidden}"
        );
    }

    let state = source
        .split("pub struct RuntimeSuspendedAttemptV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();
    for field in [
        "operation_scope: RuntimeSuspendAttemptOperationScopeV2,",
        "canonical_attempt: RuntimeCanonicalSuspendAttemptV2,",
        "sidecar_revision: NonZeroU64,",
        "local_effect: RuntimeLocalRouteEffectV2,",
        "drain_obligation: RuntimeDrainObligationV2,",
        "suspended_at: DateTime<Utc>,",
    ] {
        assert!(state.contains(field), "{field}");
    }
    assert_eq!(state.matches("    ").count(), 6);
    assert!(!state.contains("pub "));

    let progress = source
        .split("pub struct RuntimeSuspendAttemptDrainProgressV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();
    assert!(progress.contains("source: RuntimeSuspendedAttemptV2,"));
    assert!(progress.contains("replacement_local_effect: RuntimeLocalRouteEffectV2,"));
    assert!(progress.contains("replacement_drain_obligation: RuntimeDrainObligationV2,"));
    assert_eq!(progress.matches("    ").count(), 3);
    assert!(!progress.contains("pub "));

    assert!(source.contains(concat!(
        "pub fn from_inserted(\n",
        "        operation: &RuntimeSuspendAttemptOperationV2,\n",
        "        sidecar_revision: NonZeroU64,\n",
        "        local_effect: RuntimeLocalRouteEffectV2,\n",
        "        drain_obligation: RuntimeDrainObligationV2,\n",
        "        suspended_at: DateTime<Utc>,"
    )));
    assert!(source.contains(concat!(
        "pub fn from_persisted(\n",
        "        root: &RuntimePersistedSuspendAttemptRootV2,\n",
        "        sidecar_revision: NonZeroU64,\n",
        "        local_effect: RuntimeLocalRouteEffectV2,\n",
        "        drain_obligation: RuntimeDrainObligationV2,\n",
        "        suspended_at: DateTime<Utc>,"
    )));
    assert!(source.contains("local_effect != request.local_effect"));
    assert!(source.contains("drain_obligation != request.drain_obligation"));
    assert!(source.contains("RuntimePersistenceU64V2::from_non_zero(sidecar_revision)"));
    assert!(source.contains("RuntimeUnixMicrosecondsV2::from_datetime(suspended_at)"));
    assert!(source.contains("validate_suspend_attempt_mutable_state("));
    assert!(!source.contains("RuntimeCanonicalSuspendAttemptV2::new(candidate)"));
    assert!(canonical.contains("pub(crate) fn validate_suspend_attempt_mutable_state("));
    assert!(wire.contains("pub(super) fn validate_suspend_attempt_mutable_state("));
    let mutable_validator = wire
        .split("pub(super) fn validate_suspend_attempt_mutable_state(")
        .nth(1)
        .and_then(|source| source.split("}\n\n").next())
        .unwrap();
    assert!(mutable_validator.contains("encode_local_effect(local_effect)?;"));
    assert!(mutable_validator.contains("encode_drain_obligation(drain_obligation)?;"));
    for forbidden in [
        "suspend_attempt_digest_v2",
        "encode_root",
        "SUSPEND_ATTEMPT_MAX_OCTETS",
    ] {
        assert!(!mutable_validator.contains(forbidden), "{forbidden}");
    }

    assert!(source.contains(concat!(
        "pub fn record_local_absent(\n",
        "        source: RuntimeSuspendedAttemptV2,\n",
        "        provenance: RuntimeRouteMutationProvenanceV2,\n",
        "        observed_sequence: NonZeroU64,"
    )));
    assert!(!source.contains("next_sidecar_revision"));
    assert!(source.contains("expected_route: Some(route),"));
    assert!(source.contains("RuntimeDrainObligationV2::ExactLocalRoute(local) if local == &route"));
    assert!(source.contains("RuntimeDrainObligationV2::None"));
    assert!(source.contains("RuntimeDrainObligationV2::LocalAndPrevious { local, previous }"));
    assert!(source.contains("RuntimeDrainObligationV2::PreviousServing(previous.clone())"));
    assert!(source.contains("pub fn expected_sidecar_revision(&self) -> NonZeroU64"));
    assert!(source.contains("pub fn expected_local_effect(&self)"));
    assert!(source.contains("pub fn expected_drain_obligation(&self)"));
    assert!(source.contains("pub fn replacement_local_effect(&self)"));
    assert!(source.contains("pub fn replacement_drain_obligation(&self)"));
    assert!(
        source.contains("current_effect == root_effect && current_obligation == root_obligation")
    );
    assert!(source.contains("*lifecycle == root_lifecycle"));
    assert!(source.contains("expected_route: Some(expected_route)"));
    assert!(source.contains("current_obligation == expected_obligation"));

    let library = include_str!("../src/lib.rs");
    for exported in [
        "RuntimeSuspendAttemptDrainProgressErrorV2",
        "RuntimeSuspendAttemptDrainProgressV2",
        "RuntimeSuspendedAttemptStateErrorV2",
        "RuntimeSuspendedAttemptStateFieldV2",
        "RuntimeSuspendedAttemptV2",
    ] {
        assert!(library.contains(exported), "{exported}");
    }

    for required_test in [
        "all_six_canonical_roots_insert_and_restore_exactly",
        "persisted_exact_routes_allow_only_their_correlated_absence_reductions",
        "persisted_state_rejects_lifecycle_changes_and_previous_only_removal",
        "progress_accepts_all_three_canonical_provenance_families",
        "progress_rejects_invalid_provenance_self_correlations",
        "both_lifecycles_and_both_local_obligations_reduce_exactly_once",
        "terminal_states_cannot_record_another_local_absence",
    ] {
        assert!(tests.contains(required_test), "{required_test}");
    }
}

#[test]
fn v2_suspension_observation_proves_only_checked_local_quiescence() {
    let source = include_str!("../src/v2_suspension_observation.rs");
    let evidence = include_str!("../src/v2_suspension.rs");

    for forbidden in [
        "serde",
        "Serialize",
        "Deserialize",
        "Default",
        "Sha256",
        "framed_sha256",
        "sqlx",
        "rusqlite",
        "tokio",
        "twilight",
        "Authority",
        "Permit",
        "Port",
        "Future",
        "pub fn from_parts",
        "pub fn into_parts",
        "RuntimeQuiescentSuspendedAttemptV2",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden suspension observation surface: {forbidden}"
        );
    }

    let observation = source
        .split("pub struct RuntimeSuspendedAttemptObservationV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();
    for field in [
        "kind: RuntimeSuspendedAttemptObservationKindV2,",
        "snapshot: RuntimeDeploymentSnapshotV1,",
        "suspended: RuntimeSuspendedAttemptV2,",
    ] {
        assert!(observation.contains(field), "{field}");
    }
    assert_eq!(observation.matches("    ").count(), 3);
    assert!(!observation.contains("pub "));

    let locally_quiescent = source
        .split("pub struct RuntimeLocallyQuiescentSuspendedAttemptV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();
    assert!(locally_quiescent.contains("observation: RuntimeSuspendedAttemptObservationV2,"));
    assert_eq!(locally_quiescent.matches("    ").count(), 1);
    assert!(!locally_quiescent.contains("pub "));

    let kind = source
        .split("pub enum RuntimeSuspendedAttemptObservationKindV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n").next())
        .unwrap();
    for variant in ["LocalRoutePresent", "ReleasePending", "LocallyQuiescent"] {
        assert!(kind.contains(variant), "{variant}");
    }
    assert_eq!(kind.matches("    ").count(), 3);

    assert!(source.contains(concat!(
        "pub fn new(\n",
        "        snapshot: RuntimeDeploymentSnapshotV1,\n",
        "        suspended: RuntimeSuspendedAttemptV2,"
    )));
    assert!(source.contains("RuntimeDeployment::restore(snapshot.clone())"));
    assert!(source.contains("operation_scope.scope().matches(&snapshot.identity)"));
    assert!(source.contains("operation_scope.deployment_revision() != snapshot.revision"));
    assert!(source.contains("guard.runtime_generation != snapshot.runtime_generation"));
    assert!(source.contains("RuntimeSuspensionSourcePhaseV2::from_deployment_phase"));
    assert!(source.contains("suspension_current_target_matches("));
    assert!(source.contains("suspension_previous_runtime_matches("));
    assert!(evidence.contains("previous.process.target.same_slot(target)"));
    assert!(evidence.contains("previous_runtime == Some(&previous.process)"));
    assert!(source.contains("suspended.failure().recorded_at"));
    assert!(source.contains("suspension_source_evidence_at(snapshot, suspended.source_phase())"));
    assert!(source.contains("snapshot.last_fencing_token != Some(guard.fencing_token)"));
    assert!(source.contains("lease.controller_id != suspended.source_guard().controller_id"));
    assert!(source.contains("lease.fencing_token != suspended.source_guard().fencing_token"));
    assert!(source.contains("LocalRouteLeaseMissing"));
    assert!(source.contains("RuntimeSuspendedAttemptObservationKindV2::ReleasePending"));
    assert!(source.contains("RuntimeSuspendedAttemptObservationKindV2::LocallyQuiescent"));
    assert!(source.contains("pub fn into_locally_quiescent("));
    assert!(source.contains("impl TryFrom<RuntimeSuspendedAttemptObservationV2>"));
    assert!(!source.contains("convergence_attempt =="));
    assert!(!source.contains("expires_at <="));

    let library = include_str!("../src/lib.rs");
    for exported in [
        "RuntimeLocallyQuiescentSuspendedAttemptV2",
        "RuntimeSuspendedAttemptObservationErrorV2",
        "RuntimeSuspendedAttemptObservationFieldV2",
        "RuntimeSuspendedAttemptObservationKindV2",
        "RuntimeSuspendedAttemptObservationV2",
    ] {
        assert!(library.contains(exported), "{exported}");
    }
}

#[test]
fn v2_suspension_resume_basis_closes_database_correlation_without_authority() {
    let source = include_str!("../src/v2_suspension_resume.rs");

    for forbidden in [
        "serde",
        "Serialize",
        "Deserialize",
        "Default",
        "sqlx",
        "rusqlite",
        "tokio",
        "twilight",
        "Authority",
        "Permit",
        "Port",
        "Future",
        "Utc::now",
        "pub fn from_parts",
        "pub fn into_parts",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden suspension resume surface: {forbidden}"
        );
    }

    let basis = source
        .split("pub struct RuntimeSuspendAttemptResumeBasisV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();
    for field in [
        "locally_quiescent: RuntimeLocallyQuiescentSuspendedAttemptV2,",
        "persisted_convergence_attempt: NonZeroU32,",
        "persisted_last_controller_id: ControllerId,",
        "gate: RuntimeSuspendAttemptResumeGateV2,",
    ] {
        assert!(basis.contains(field), "{field}");
    }
    assert_eq!(basis.matches("    ").count(), 4);
    assert!(!basis.contains("pub "));

    let resume = source
        .split("pub struct RuntimeResumeSuspendedAttemptV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();
    for field in [
        "basis: RuntimeSuspendAttemptResumeBasisV2,",
        "controller_id: ControllerId,",
        "lease_for: RuntimeControllerLeaseDurationV2,",
    ] {
        assert!(resume.contains(field), "{field}");
    }
    assert_eq!(resume.matches("    ").count(), 3);
    assert!(!resume.contains("pub "));

    assert!(source.contains(concat!(
        "pub fn new(\n",
        "        locally_quiescent: RuntimeLocallyQuiescentSuspendedAttemptV2,\n",
        "        persisted_convergence_attempt: NonZeroU32,\n",
        "        persisted_last_controller_id: ControllerId,\n",
        "        gate: RuntimeSuspendAttemptResumeGateV2,"
    )));
    assert!(source.contains("suspended.operation_scope().convergence_attempt()"));
    assert!(
        source.contains("persisted_last_controller_id != suspended.source_guard().controller_id")
    );
    assert!(source.contains("RuntimeUnixMicrosecondsV2::from_datetime(*database_observed_at)"));
    assert!(source.contains("database_observed_at < retry_not_before"));
    assert!(source.contains("expected_failure_id == &suspended.failure().failure_id"));
    assert!(source.contains("!value.subsec_nanos().is_multiple_of(1_000_000)"));
    assert!(source.contains("!(1_000..=600_000).contains(&milliseconds)"));
    assert!(source.contains("RuntimeControllerLeaseDurationV2::new(lease_for)?"));

    let library = include_str!("../src/lib.rs");
    for exported in [
        "RuntimeControllerLeaseDurationV2",
        "RuntimeResumeSuspendedAttemptErrorV2",
        "RuntimeResumeSuspendedAttemptV2",
        "RuntimeSuspendAttemptResumeBasisErrorV2",
        "RuntimeSuspendAttemptResumeBasisV2",
        "RuntimeSuspendAttemptResumeGateV2",
    ] {
        assert!(library.contains(exported), "{exported}");
    }
}

#[test]
fn v2_suspension_receipts_close_presence_and_exact_successors() {
    let source = include_str!("../src/v2_suspension_receipt.rs");

    for forbidden in [
        "serde",
        "Serialize",
        "Deserialize",
        "Default",
        "sqlx",
        "rusqlite",
        "tokio",
        "twilight",
        "Authority",
        "Permit",
        "Port",
        "Future",
        "Utc::now",
        "pub fn new(",
        "pub fn from_parts",
        "pub fn into_parts",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden suspension receipt surface: {forbidden}"
        );
    }

    let outcome = source
        .split("pub enum RuntimeSuspendAttemptMutationOutcomeV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n").next())
        .unwrap();
    for variant in ["Inserted", "Replayed", "DrainProgressed", "Resumed"] {
        assert!(outcome.contains(variant), "{variant}");
    }
    assert_eq!(outcome.matches("    ").count(), 4);

    let receipt = source
        .split("pub struct RuntimeSuspendAttemptReceiptV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();
    for field in [
        "outcome: RuntimeSuspendAttemptMutationOutcomeV2,",
        "snapshot: RuntimeDeploymentSnapshotV1,",
        "suspended: Option<RuntimeSuspendedAttemptV2>,",
        "successor_execution: Option<RuntimeExecutionReceiptV1>,",
    ] {
        assert!(receipt.contains(field), "{field}");
    }
    assert_eq!(receipt.matches("    ").count(), 4);
    assert!(!receipt.contains("pub "));

    for constructor in [
        "pub fn inserted(",
        "pub fn replayed(",
        "pub fn drain_progressed(",
        "pub fn resumed(",
    ] {
        assert!(source.contains(constructor), "{constructor}");
    }
    assert!(source.contains("suspended: Some(observation.suspended().clone())"));
    assert!(source.contains("successor_execution: None"));
    assert!(source.contains("suspended: None"));
    assert!(source.contains("successor_execution: Some(successor_execution)"));

    assert!(source.contains("suspended.operation_scope() != operation.operation_scope()"));
    assert!(source.contains("suspended.canonical_attempt() != operation.canonical_attempt()"));
    assert!(source.contains("suspended.local_effect() != &request.local_effect"));
    assert!(source.contains("suspended.drain_obligation() != &request.drain_obligation"));
    assert!(source.contains("source\n        .sidecar_revision()"));
    assert!(source.contains(".checked_add(1)"));
    assert!(source.contains("RuntimePersistenceU64V2::from_non_zero(expected_revision)"));
    assert!(source.contains("result.sidecar_revision() != expected_revision"));
    assert!(source.contains("result.local_effect() != progress.replacement_local_effect()"));
    assert!(source.contains("result.drain_obligation() != progress.replacement_drain_obligation()"));
    assert!(source.contains("result.suspended_at() != source.suspended_at()"));

    assert!(source.contains("RuntimeDeployment::restore(snapshot.clone())"));
    assert!(source.contains("RuntimeUnixMicrosecondsV2::from_datetime(successor.acquired_at)"));
    assert!(source.contains("source\n        .revision\n        .next()"));
    assert!(source.contains("persisted_convergence_attempt()"));
    assert!(source.contains(".checked_add(1)"));
    assert!(source.contains(".fencing_token\n        .next()"));
    assert!(source.contains("RuntimePersistenceU64V2::from_u64(expected_revision.get())"));
    assert!(source.contains("RuntimePersistenceU64V2::from_u64(expected_fence.get())"));
    assert!(source.contains("&successor.controller_id != resume.controller_id()"));
    assert!(source.contains("snapshot.last_fencing_token != Some(expected_fence)"));
    assert!(source.contains("lease.controller_id != successor.controller_id"));
    assert!(source.contains("lease.fencing_token != successor.fencing_token"));
    assert!(source.contains("lease.acquired_at != successor.acquired_at"));
    assert!(source.contains("lease.expires_at != successor.expires_at"));
    assert!(source.contains("Some(resume.lease_for())"));
    assert!(source.contains("successor.acquired_at < *retry_not_before"));
    for preserved in [
        "source.identity == successor.identity",
        "source.target == successor.target",
        "source.runtime_generation == successor.runtime_generation",
        "source.previous_runtime == successor.previous_runtime",
        "source.requested_at == successor.requested_at",
        "source.phase == successor.phase",
        "source.preflight == successor.preflight",
        "source.drain == successor.drain",
        "source.activation == successor.activation",
        "source.panel_certificate == successor.panel_certificate",
        "source.gateway_ready == successor.gateway_ready",
        "source.live == successor.live",
        "source.last_live_recovery == successor.last_live_recovery",
        "source.last_runtime_failure == successor.last_runtime_failure",
    ] {
        assert!(source.contains(preserved), "{preserved}");
    }

    let library = include_str!("../src/lib.rs");
    for exported in [
        "RuntimeSuspendAttemptMutationOutcomeV2",
        "RuntimeSuspendAttemptReceiptErrorV2",
        "RuntimeSuspendAttemptReceiptV2",
    ] {
        assert!(library.contains(exported), "{exported}");
    }
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
        (
            "src/v2_drain_claim.rs",
            include_str!("../src/v2_drain_claim.rs"),
        ),
        (
            "src/v2_drain_claim/tests.rs",
            include_str!("../src/v2_drain_claim/tests.rs"),
        ),
        (
            "src/v2_drain_intent_state.rs",
            include_str!("../src/v2_drain_intent_state.rs"),
        ),
        (
            "src/v2_drain_intent_state/tests.rs",
            include_str!("../src/v2_drain_intent_state/tests.rs"),
        ),
        (
            "src/v2_drain_intent_canonical_state.rs",
            include_str!("../src/v2_drain_intent_canonical_state.rs"),
        ),
        (
            "src/v2_drain_intent_canonical_state/wire.rs",
            include_str!("../src/v2_drain_intent_canonical_state/wire.rs"),
        ),
        (
            "src/v2_drain_intent_canonical_state/tests.rs",
            include_str!("../src/v2_drain_intent_canonical_state/tests.rs"),
        ),
        (
            "src/v2_drain_intent_receipt.rs",
            include_str!("../src/v2_drain_intent_receipt.rs"),
        ),
        (
            "src/v2_drain_intent_receipt/tests.rs",
            include_str!("../src/v2_drain_intent_receipt/tests.rs"),
        ),
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
        (
            "src/v2_product_drain_operation.rs",
            include_str!("../src/v2_product_drain_operation.rs"),
        ),
        (
            "src/v2_product_drain_operation/tests.rs",
            include_str!("../src/v2_product_drain_operation/tests.rs"),
        ),
        (
            "src/v2_product_drain_observation.rs",
            include_str!("../src/v2_product_drain_observation.rs"),
        ),
        (
            "src/v2_product_drain_observation/tests.rs",
            include_str!("../src/v2_product_drain_observation/tests.rs"),
        ),
        (
            "src/v2_product_drain_adoption.rs",
            include_str!("../src/v2_product_drain_adoption.rs"),
        ),
        (
            "src/v2_product_drain_adoption/tests.rs",
            include_str!("../src/v2_product_drain_adoption/tests.rs"),
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
            "src/v2_suspension_canonical.rs",
            include_str!("../src/v2_suspension_canonical.rs"),
        ),
        (
            "src/v2_suspension_canonical/wire.rs",
            include_str!("../src/v2_suspension_canonical/wire.rs"),
        ),
        (
            "src/v2_suspension_canonical/tests.rs",
            include_str!("../src/v2_suspension_canonical/tests.rs"),
        ),
        (
            "src/v2_suspension_operation.rs",
            include_str!("../src/v2_suspension_operation.rs"),
        ),
        (
            "src/v2_suspension_operation/tests.rs",
            include_str!("../src/v2_suspension_operation/tests.rs"),
        ),
        (
            "src/v2_suspension_observation.rs",
            include_str!("../src/v2_suspension_observation.rs"),
        ),
        (
            "src/v2_suspension_observation/tests.rs",
            include_str!("../src/v2_suspension_observation/tests.rs"),
        ),
        (
            "src/v2_suspension_resume.rs",
            include_str!("../src/v2_suspension_resume.rs"),
        ),
        (
            "src/v2_suspension_resume/tests.rs",
            include_str!("../src/v2_suspension_resume/tests.rs"),
        ),
        (
            "src/v2_suspension_receipt.rs",
            include_str!("../src/v2_suspension_receipt.rs"),
        ),
        (
            "src/v2_suspension_receipt/tests.rs",
            include_str!("../src/v2_suspension_receipt/tests.rs"),
        ),
        (
            "src/v2_suspension_sidecar.rs",
            include_str!("../src/v2_suspension_sidecar.rs"),
        ),
        (
            "src/v2_suspension_sidecar/tests.rs",
            include_str!("../src/v2_suspension_sidecar/tests.rs"),
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
