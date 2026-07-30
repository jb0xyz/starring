use std::fs;
use std::path::{Path, PathBuf};

fn collect_rust_sources(directory: &Path, sources: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_rust_sources(&path, sources);
        } else if path.extension().and_then(|value| value.to_str()) == Some("rs") {
            sources.push(path);
        }
    }
}

fn contains_identifier(source: &str, expected: &str) -> bool {
    source
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .any(|identifier| identifier == expected)
}

fn implements_trait(source: &str, name: &str, trait_name: &str) -> bool {
    let marker = format!(" for {name}");
    source.match_indices(&marker).any(|(end, _)| {
        source[..end]
            .rfind("impl ")
            .is_some_and(|start| contains_identifier(&source[start..end], trait_name))
    })
}

#[test]
fn worker_dependency_surface_is_pure_library_only_and_closed() {
    let manifest = include_str!("../Cargo.toml");
    let root_manifest = include_str!("../../../Cargo.toml");
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source_root = crate_root.join("src");
    let mut sources = Vec::new();
    collect_rust_sources(&source_root, &mut sources);
    sources.sort();
    let relative_sources = sources
        .iter()
        .map(|path| path.strip_prefix(crate_root).unwrap().to_path_buf())
        .collect::<Vec<_>>();

    assert_eq!(
        root_manifest
            .matches("\"crates/automation-runtime-worker\"")
            .count(),
        1
    );
    assert_eq!(
        relative_sources,
        [
            PathBuf::from("src/capability_readiness.rs"),
            PathBuf::from("src/certification_finalization/tests.rs"),
            PathBuf::from("src/certification_finalization.rs"),
            PathBuf::from("src/certification_reservation.rs"),
            PathBuf::from("src/closed_recovery.rs"),
            PathBuf::from("src/convergence/hydration.rs"),
            PathBuf::from("src/convergence/preflight.rs"),
            PathBuf::from("src/convergence/replacement.rs"),
            PathBuf::from("src/convergence/staging.rs"),
            PathBuf::from("src/convergence/tests.rs"),
            PathBuf::from("src/convergence.rs"),
            PathBuf::from("src/gateway_lifecycle.rs"),
            PathBuf::from("src/gateway_lifecycle_tests.rs"),
            PathBuf::from("src/gateway_owner.rs"),
            PathBuf::from("src/gateway_owner_watchdog.rs"),
            PathBuf::from("src/ingress_acknowledgement.rs"),
            PathBuf::from("src/lib.rs"),
            PathBuf::from("src/paused_gateway.rs"),
            PathBuf::from("src/product_drain.rs"),
            PathBuf::from("src/production_lifecycle/admission.rs"),
            PathBuf::from("src/production_lifecycle/handoff.rs"),
            PathBuf::from("src/production_lifecycle/refresh.rs"),
            PathBuf::from("src/production_lifecycle/serving/route_set.rs"),
            PathBuf::from("src/production_lifecycle/serving/slot_work.rs"),
            PathBuf::from("src/production_lifecycle/serving.rs"),
            PathBuf::from("src/production_lifecycle/shutdown.rs"),
            PathBuf::from("src/production_lifecycle/tests.rs"),
            PathBuf::from("src/production_lifecycle.rs"),
            PathBuf::from("src/recovery.rs"),
            PathBuf::from("src/registry_recovery.rs"),
            PathBuf::from("src/startup_pending_drain/v3/tests.rs"),
            PathBuf::from("src/startup_pending_drain/v3.rs"),
            PathBuf::from("src/startup_pending_drain/v4/mutation.rs"),
            PathBuf::from("src/startup_pending_drain/v4/orchestration.rs"),
            PathBuf::from("src/startup_pending_drain/v4/selection.rs"),
            PathBuf::from("src/startup_pending_drain/v4/terminal.rs"),
            PathBuf::from("src/startup_pending_drain/v4/tests.rs"),
            PathBuf::from("src/startup_pending_drain/v4.rs"),
            PathBuf::from("src/startup_pending_drain.rs"),
            PathBuf::from("src/startup_recovery.rs"),
            PathBuf::from("src/startup_recovery_execution.rs"),
            PathBuf::from("src/startup_recovery_execution_tests.rs"),
            PathBuf::from("src/writer_fence.rs"),
        ]
    );
    assert!(!crate_root.join("build.rs").exists());
    for forbidden in [
        "[[bin]]",
        "[build-dependencies]",
        "build =",
        "crate-type",
        "proc-macro",
        "[dev-dependencies]",
    ] {
        assert!(
            !manifest.contains(forbidden),
            "forbidden worker package surface: {forbidden}"
        );
    }

    let dependencies = manifest
        .split_once("[dependencies]\n")
        .unwrap()
        .1
        .split("\n[")
        .next()
        .unwrap()
        .lines()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(
        dependencies,
        [
            "automation-runtime-controller = { path = \"../automation-runtime-controller\" }",
            "automation-runtime-convergence = { path = \"../automation-runtime-convergence\" }",
            "chrono = \"0.4\"",
            "thiserror.workspace = true",
        ]
    );

    for path in sources {
        let source = fs::read_to_string(&path).unwrap();
        for forbidden in [
            "sqlx",
            "tokio",
            "twilight",
            "serde",
            "serde_json",
            "reqwest",
            "hyper",
            "automation_runtime::",
            "automation_runtime_registry",
            "automation_runtime_execution_postgres",
            "automation_runtime_serving_postgres",
            "ai_gateway",
            "design_harness",
            "std::env",
            "std::fs",
            "std::io",
            "std::net",
            "std::process",
            "std::signal",
            "TcpStream",
            "UdpSocket",
            "async fn",
            "Serialize",
            "Deserialize",
            "Default",
            "unsafe",
            "//",
            "/*",
        ] {
            assert!(
                !source.contains(forbidden),
                "forbidden worker source surface in {}: {forbidden}",
                path.display()
            );
        }
    }
}

#[test]
fn certification_reservation_port_is_pure_checked_and_non_authorizing() {
    let source = include_str!("../src/certification_reservation.rs");
    let port = source
        .split("pub trait RuntimeCertificationReservationPortV2 {")
        .nth(1)
        .and_then(|source| source.split("\n}").next())
        .unwrap();

    for expected in [
        "type Error;",
        "reservation: RuntimeReservedCertificationIntentV2,",
        "Result<RuntimeCertificationIntentReservationOutcomeV2, Self::Error>",
        "lookup: RuntimeCertificationReservationScopeLookupV2,",
        "Result<RuntimeCertificationReservationScopeObservationV2, Self::Error>",
    ] {
        assert!(port.contains(expected), "{expected}");
    }
    for forbidden in [
        "RuntimeCertificationOperationIdV2",
        "RuntimeCanonicalCertificationIntentV2",
        "RuntimeCertificationIntentV2",
        "RuntimeExecutionReceiptV1",
        "authorize",
        "commit",
        "prepare",
        "reset",
        "consume",
        "generate",
        "new_id",
    ] {
        assert!(!port.contains(forbidden), "{forbidden}");
    }
    assert_eq!(port.matches("fn ").count(), 2);
    assert_eq!(port.matches("fn reserve_certification_intent(").count(), 1);
    assert_eq!(
        port.matches("fn observe_certification_reservation_scope(")
            .count(),
        1
    );
}

#[test]
fn certification_finalization_is_session_bound_linear_and_lookup_only_after_unknown() {
    let source = include_str!("../src/certification_finalization.rs");
    let finalizer_port = source
        .split("pub trait RuntimeCertificationFinalizerPortV2<P> {")
        .nth(1)
        .and_then(|source| source.split("\n}").next())
        .unwrap();
    let reserved = source
        .split("impl RuntimeReservedCertificationV2 {")
        .nth(1)
        .and_then(|source| source.split("\n}\n\nimpl Debug").next())
        .unwrap();
    let lookup_only = source
        .split("impl<E, R> RuntimeCertificationLookupOnlyRecoveryV2<E, R>")
        .nth(1)
        .and_then(|source| source.split("\n}\n\nimpl<E, R> Debug").next())
        .unwrap();

    assert!(reserved.contains("authority: RuntimeCertificationReservationAuthorityV2,"));
    assert!(reserved.contains("authority.into_reserved_intent()"));
    assert!(!reserved.contains("reservation: RuntimeReservedCertificationIntentV2,"));
    assert!(source.contains(
        "pub fn into_session_outcome(self) -> RuntimeCertificationIntentReservationOutcomeV2"
    ));
    assert!(source.contains(
        "RuntimeCertificationIntentReservationOutcomeV2::Diverged(\n                    RuntimeCertificationDivergenceV2::ReservationMismatch,"
    ));
    assert!(source.contains("struct RuntimeCertificationCommitAuthorityV2 {\n    _private: (),\n}"));
    assert!(!source.contains("from_barrier_completion_v2"));
    assert!(source.contains(
        "pub fn complete_barrier_b_v2(\n        self,\n        barrier_id: RuntimeBarrierIdV1,\n        paused_gateway: RuntimePausedGatewayObservationV2,\n        route_admission: RuntimeRouteAdmissionAttestationV2,"
    ));
    assert!(source.contains("fn canonicalize_barrier_b_completion_v2<P>("));
    for exact_check in [
        "paused_gateway.process_instance_id() != &intent.process_identity.process_instance_id",
        "&route_admission.barrier_id != barrier_id",
        "route_admission.pause.coordinator_generation.get()",
        "route_admission.pause.connection_epoch != paused_gateway.connection_epoch()",
        "route_admission.pause.paused_admission_revision != paused_gateway.admission_revision()",
        "route_admission.pause.pause_sequence != paused_gateway.transition_sequence()",
        "route_admission.gateway.kind != RuntimeGatewayReadyKindV2::Resumed",
        "route_admission.gateway.connected_event_sequence",
        "RuntimeLiveAttestationRecordV2::from_request(request)?",
        ".bind_live_record(record)",
    ] {
        assert!(source.contains(exact_check), "{exact_check}");
    }
    assert!(source.contains(
        "pub struct RuntimeCompletedCertificationBarrierBV2<P> {\n    prepared: P,\n    canonical: RuntimeCanonicalLiveAttestationV2,\n}"
    ));
    assert!(source.contains(
        "pub fn authorize_finalization(self) -> RuntimeCertificationFinalizerRegistrationV2<P>"
    ));
    assert!(source.contains(
        "pub fn into_registration(self) -> RuntimeCertificationFinalizerRegistrationV2<P>"
    ));
    assert_eq!(
        source
            .matches("authority: RuntimeCertificationCommitAuthorityV2 { _private: () },")
            .count(),
        1
    );
    let prepared_impl = source
        .split("impl<P> RuntimePreparedCertificationV2<P>")
        .nth(1)
        .and_then(|source| source.split("\n}\n\nfn canonicalize_barrier_b").next())
        .unwrap();
    assert!(!prepared_impl.contains("authorize_finalization"));
    assert!(source.contains(
        "pub struct RuntimeAuthorizedCertificationRequestV2 {\n    canonical: RuntimeCanonicalLiveAttestationV2,\n    authority: RuntimeCertificationCommitAuthorityV2,\n}"
    ));
    assert!(source.contains(
        "pub struct RuntimeCommittedCertificationV2 {\n    canonical: RuntimeCanonicalLiveAttestationV2,\n    receipt: RuntimeCertificationReceiptV2,\n}"
    ));
    assert!(source.contains("pub fn canonical(&self) -> &RuntimeCanonicalLiveAttestationV2"));
    assert!(source.contains("pub fn into_parts("));
    assert!(source.contains(
        "RuntimeCommittedCertificationV2 {\n            canonical: expected.clone(),\n            receipt,"
    ));
    for name in [
        "RuntimeReservedCertificationV2",
        "RuntimePreparedCertificationV2<P>",
        "RuntimeCompletedCertificationBarrierBV2<P>",
        "RuntimeCertificationBarrierBCompletionFailureV2<P>",
        "RuntimeCertificationCommitAuthorityV2",
        "RuntimeAuthorizedCertificationRequestV2",
        "RuntimeCertificationFinalizerRegistrationV2<P>",
        "RuntimeCertificationFinalizerJobV2<P>",
        "RuntimeCommittedCertificationV2",
        "RuntimeCertificationLookupOnlyRecoveryV2<E, R>",
    ] {
        for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
            assert!(
                !implements_trait(source, name, forbidden),
                "{name}: {forbidden}"
            );
        }
    }
    for expected in [
        "fn accept_certification_finalizer(",
        "registration: RuntimeCertificationFinalizerRegistrationV2<P>,",
        "RuntimeCertificationFinalizerRejectionV2<P, Self::Error>",
    ] {
        assert!(finalizer_port.contains(expected), "{expected}");
    }
    assert!(!finalizer_port.contains("Future"));
    assert!(!finalizer_port.contains("async"));
    for expected in [
        "RuntimeCertificationFinalizationOutcomeV2::Committed",
        "RuntimeCertificationFinalizationOutcomeV2::DefinitelyRolledBack",
        "RuntimeCertificationFinalizationOutcomeV2::Indeterminate",
        "RuntimeCertificationRecoveryResolutionV2::DefinitelyRolledBack",
    ] {
        assert!(source.contains(expected), "{expected}");
    }
    assert!(lookup_only.contains("pub fn lookup(&self)"));
    assert!(lookup_only.contains("pub fn quiesce_and_observe("));
    for forbidden in [
        "commit_live_v2",
        "prepare_live_v2",
        "authorize_finalization",
        "heartbeat",
    ] {
        assert!(!lookup_only.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn product_drain_unknown_recovery_contract_is_owned_and_non_authorizing() {
    let product = include_str!("../src/product_drain.rs");
    let recovery = include_str!("../src/recovery.rs");
    let outcome_prefix = product
        .split("pub struct RuntimeProductDrainRecoveryOutcomeV2<W> {")
        .next()
        .unwrap();
    let outcome_attributes = outcome_prefix.rsplit_once("\n\n").unwrap().1;
    let outcome_fields = product
        .split("pub struct RuntimeProductDrainRecoveryOutcomeV2<W> {")
        .nth(1)
        .and_then(|source| source.split("}\n\npub trait").next())
        .unwrap();
    let port = product
        .split("pub trait RuntimeProductDrainUnknownRecoveryPortV2: Sized {")
        .nth(1)
        .and_then(|source| source.split("\n}").next())
        .unwrap();
    let pending_prefix = recovery
        .split("pub struct RuntimeRecoveryPendingV2<E, R> {")
        .next()
        .unwrap();
    let pending_attributes = pending_prefix.trim();
    let pending_fields = recovery
        .split("pub struct RuntimeRecoveryPendingV2<E, R> {")
        .nth(1)
        .and_then(|source| source.split("\n}").next())
        .unwrap();

    assert!(outcome_attributes.contains("#[must_use]"));
    assert!(pending_attributes.contains("#[must_use]"));
    assert_eq!(
        outcome_fields,
        "\n    pub transaction_ended: W,\n    pub observation: RuntimeProductDrainScopeObservationV2,\n"
    );
    assert_eq!(pending_fields, "\n    pub source: E,\n    pub recovery: R,");
    for source in [outcome_attributes, pending_attributes] {
        for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
            assert!(!source.contains(forbidden));
        }
    }
    for forbidden in ["Clone", "Copy", "Default"] {
        assert!(!implements_trait(
            product,
            "RuntimeProductDrainRecoveryOutcomeV2<W>",
            forbidden
        ));
        assert!(!implements_trait(
            recovery,
            "RuntimeRecoveryPendingV2<E, R>",
            forbidden
        ));
    }
    for expected in [
        "type TransactionEnded;",
        "fn lookup(&self) -> &RuntimeProductDrainScopeLookupV2;",
        "fn quiesce_and_observe(\n        self,\n        timeout: Duration,",
        "RuntimeProductDrainRecoveryOutcomeV2<Self::TransactionEnded>",
        "RuntimeRecoveryPendingV2<Self::Error, Self>",
    ] {
        assert!(port.contains(expected));
    }
    for forbidden in [
        "RuntimeProductOperationIdV2",
        "RuntimeDrainIntentIdV2",
        "RuntimeProductDrainOperationV2",
        "RuntimeProductDrainScopeObservationV2",
        "authorize",
        "apply",
        "insert",
        "mutate",
        "mint",
        "generate",
        "new_id",
    ] {
        assert!(!port.contains(forbidden));
    }
    assert_eq!(port.matches("fn ").count(), 2);
    assert_eq!(port.matches("fn lookup(").count(), 1);
    assert_eq!(port.matches("fn quiesce_and_observe(").count(), 1);
}

#[test]
fn registry_recovery_evidence_is_pure_redacted_and_non_authorizing() {
    let source = include_str!("../src/registry_recovery.rs");
    let declaration = source
        .split("pub struct RuntimeRegistryRecoveryEmptyObservationV2 {")
        .next()
        .unwrap();
    let attributes = declaration.rsplit_once("\n\n").unwrap().1;
    let evidence_fields = source
        .split("pub struct RuntimeRegistryRecoveryEmptyObservationV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl").next())
        .unwrap();

    for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
        assert!(!attributes.contains(forbidden));
    }
    for forbidden in [
        "impl Clone for RuntimeRegistryRecoveryEmptyObservationV2",
        "impl Copy for RuntimeRegistryRecoveryEmptyObservationV2",
        "impl Default for RuntimeRegistryRecoveryEmptyObservationV2",
    ] {
        assert!(!source.contains(forbidden));
    }
    for field in [
        "process_instance_id",
        "observation_sequence",
        "retained_slot_count",
        "retained_empty_tombstone_count",
    ] {
        assert!(evidence_fields.contains(&format!("    {field}:")));
        assert!(!evidence_fields.contains(&format!("    pub {field}:")));
    }
    for expected in [
        "RuntimeRegistryGlobalObservationSequenceV2(<redacted>)",
        "RuntimeRegistryRecoveryObservationInputV2(<redacted>)",
        "RuntimeRegistryRecoveryEmptyObservationV2(<redacted>)",
        "ObservationSequenceOutOfRange",
        "observation.observation_sequence.get() > i64::MAX as u64",
        "pub observation_sequence: RuntimeRegistryGlobalObservationSequenceV2",
        "pub retained_slot_count: u64",
        "pub retained_empty_tombstone_count: u64",
        "pub staged_route_count: u64",
        "pub serving_route_count: u64",
        "pub draining_route_count: u64",
        "pub sealed_slot_count: u64",
        "pub active_interaction_count: u64",
        "pub failed_closed_slot_count: u64",
        "pub registry_failed_closed: bool",
    ] {
        assert!(source.contains(expected));
    }
    for forbidden in [
        "automation_runtime_registry",
        "SlotLifecycleV1",
        "SlotRouteWitnessV1",
        "Arc<",
        "Mutex<",
        "Capability",
        "Authority",
        "Permit",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden registry recovery authority surface: {forbidden}"
        );
    }
}

#[test]
fn paused_gateway_evidence_is_redacted_and_nonserializable() {
    let source = include_str!("../src/paused_gateway.rs");

    for forbidden in ["Serialize", "Deserialize", "Default"] {
        assert!(!source.contains(forbidden));
    }
    assert!(source.contains("RuntimePausedGatewayObservationV2(<redacted>)"));
    for field in [
        "coordinator_generation",
        "process_instance_id",
        "connection_epoch",
        "admission_revision",
        "transition_sequence",
        "connected_event_sequence",
        "last_resume_sequence",
    ] {
        assert!(!source.contains(&format!("pub {field}:")));
    }
}

#[test]
fn readiness_evidence_is_exact_redacted_and_nonserializable() {
    let source = include_str!("../src/capability_readiness.rs");

    for forbidden in ["Serialize", "Deserialize", "Default"] {
        assert!(!source.contains(forbidden));
    }
    assert!(source.contains("RuntimeCapabilityReadinessSetV2(<redacted>)"));
    assert!(source.contains("RuntimeCapabilityReadinessReceiptV2(<redacted>)"));
    assert!(!source.contains("pub database_identity"));
    assert!(!source.contains("pub database_name"));
    assert!(!source.contains("pub executor_role"));
}

#[test]
fn startup_planner_fixed_point_is_narrow_and_non_authorizing() {
    let source = include_str!("../src/startup_recovery.rs");
    let closed = include_str!("../src/closed_recovery.rs");
    let lifecycle = include_str!("../src/gateway_lifecycle.rs");
    let planner_declaration = source
        .split("pub struct RuntimeStartupRecoveryObservationFixedPointV2 {")
        .next()
        .unwrap();
    let planner_attributes = planner_declaration.rsplit_once("\n\n").unwrap().1;
    let proof_declaration = source
        .split("pub struct RuntimeStartupRecoveryFixedPointProofV2 {")
        .next()
        .unwrap();
    let proof_attributes = proof_declaration.rsplit_once("\n\n").unwrap().1;

    for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
        assert!(!planner_attributes.contains(forbidden));
        assert!(!proof_attributes.contains(forbidden));
        assert!(!implements_trait(
            source,
            "RuntimeStartupRecoveryObservationFixedPointV2",
            forbidden
        ));
        assert!(!implements_trait(
            source,
            "RuntimeStartupRecoveryFixedPointProofV2",
            forbidden
        ));
    }
    assert!(source.contains(concat!(
        "pub struct RuntimeStartupRecoveryObservationFixedPointV2 {\n",
        "    acknowledged_product_handoff_count: u32,\n",
        "}"
    )));
    assert_eq!(
        source
            .matches("RuntimeStartupRecoveryObservationFixedPointV2 {")
            .count(),
        3
    );
    let planner = source
        .split("pub fn plan_runtime_startup_recovery_v2(")
        .nth(1)
        .and_then(|source| {
            source
                .split("\n#[derive(Clone, Copy, Debug, PartialEq, Eq)]\nenum RuntimeStartupServingClassificationV2")
                .next()
        })
        .unwrap();
    assert!(planner.contains("RuntimeStartupRecoveryObservationFixedPointV2 {"));
    for forbidden in [
        "RuntimeStartupRecoveryFixedPointProofV2",
        "RuntimeAcceptedStartupRecoveryOutcomeV2",
        "RuntimeClosedRecoveryOperationAuthorityV2",
        "operation_authority",
        "owner_receipt",
        "correlation",
        "successor_authority_revision",
    ] {
        assert!(!planner.contains(forbidden), "{forbidden}");
    }
    assert!(source.contains(concat!(
        "pub struct RuntimeStartupRecoveryFixedPointProofV2 {\n",
        "    operation_authority: RuntimeClosedRecoveryOperationAuthorityV2,\n",
        "    correlation: RuntimeStartupRecoveryObservationCorrelationV2,\n",
        "    successor_authority_revision: RuntimeClosedRecoveryAuthorityRevisionV2,\n",
        "    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,\n",
        "    acknowledged_product_handoff_count: u32,\n",
        "}"
    )));
    assert_eq!(
        source
            .matches("RuntimeStartupRecoveryFixedPointProofV2 {")
            .count(),
        4
    );
    assert!(source.contains("RuntimeStartupRecoveryFixedPointProofV2(<redacted>)"));
    for field in [
        "operation_authority",
        "correlation",
        "successor_authority_revision",
        "owner_receipt",
        "acknowledged_product_handoff_count",
    ] {
        assert!(!source.contains(&format!("pub {field}:")));
    }
    assert!(!closed.contains("RuntimeStartupRecoveryObservationFixedPointV2"));
    assert!(!lifecycle.contains("RuntimeStartupRecoveryObservationFixedPointV2"));
    assert!(!lifecycle.contains("FixedPoint("));
}

#[test]
fn startup_observation_authority_is_iteration_linear_owner_bound_and_observe_only() {
    let startup = include_str!("../src/startup_recovery.rs");
    let closed = include_str!("../src/closed_recovery.rs");
    let lifecycle = include_str!("../src/gateway_lifecycle.rs");

    for authority in [
        "RuntimeAuthorizedStartupRecoveryIterationV2",
        "RuntimeAuthorizedStartupRecoveryObservationV2",
        "RuntimeCompletedStartupRecoveryObservationV2",
        "RuntimeStartupRecoveryFixedPointProofV2",
    ] {
        let declaration = startup
            .split(&format!("pub struct {authority} {{"))
            .next()
            .unwrap();
        let attributes = declaration.rsplit_once("\n\n").unwrap().1;
        for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
            assert!(!attributes.contains(forbidden));
            assert!(!implements_trait(startup, authority, forbidden));
        }
        let fields = startup
            .split(&format!("pub struct {authority} {{"))
            .nth(1)
            .and_then(|source| source.split("}\n\n").next())
            .unwrap();
        assert!(!fields.contains("pub "));
        assert!(startup.contains(&format!("{authority}(<redacted>)")));
    }
    let accepted_declaration = startup
        .split("pub enum RuntimeAcceptedStartupRecoveryOutcomeV2 {")
        .next()
        .unwrap();
    let accepted_attributes = accepted_declaration.rsplit_once("\n\n").unwrap().1;
    for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
        assert!(!accepted_attributes.contains(forbidden));
        assert!(!implements_trait(
            startup,
            "RuntimeAcceptedStartupRecoveryOutcomeV2",
            forbidden
        ));
    }
    assert!(startup.contains(concat!(
        "pub enum RuntimeAcceptedStartupRecoveryOutcomeV2 {\n",
        "    Continue(RuntimeStartupRecoveryContinuationV2),\n",
        "    FixedPoint(RuntimeStartupRecoveryFixedPointProofV2),\n",
        "}"
    )));
    assert!(startup.contains("RuntimeAcceptedStartupRecoveryOutcomeV2(<redacted>)"));

    for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
        let token_attributes = closed
            .split("pub(crate) struct RuntimeClosedRecoveryOperationAuthorityV2 {")
            .next()
            .unwrap()
            .rsplit_once("\n\n")
            .unwrap()
            .1;
        assert!(!token_attributes.contains(forbidden));
        assert!(!implements_trait(
            closed,
            "RuntimeClosedRecoveryOperationAuthorityV2",
            forbidden
        ));
    }
    assert!(closed.contains(concat!(
        "operation_authority: Option<RuntimeClosedRecoveryOperationAuthorityV2>,\n",
        "    pending_startup_recovery_execution: Option<RuntimePendingStartupRecoveryExecutionV2>,\n",
        "    last_startup_observation_database_now: Option<DateTime<Utc>>,"
    )));
    assert_eq!(
        closed
            .matches("Some(RuntimeClosedRecoveryOperationAuthorityV2 { _private: () })")
            .count(),
        1
    );
    assert_eq!(closed.matches("fn take_operation_authority(").count(), 1);
    assert_eq!(closed.matches("fn restore_operation_authority(").count(), 1);
    assert!(startup.contains(concat!(
        "pub struct RuntimeAuthorizedStartupRecoveryIterationV2 {\n",
        "    request: RuntimeStartupRecoveryObservationRequestV2,\n",
        "    operation_authority: RuntimeClosedRecoveryOperationAuthorityV2,\n",
        "}"
    )));
    assert!(startup.contains(concat!(
        "pub struct RuntimeAuthorizedStartupRecoveryObservationV2 {\n",
        "    request: RuntimeStartupRecoveryObservationRequestV2,\n",
        "    minimum_database_now: DateTime<Utc>,\n",
        "    operation_authority: RuntimeClosedRecoveryOperationAuthorityV2,\n",
        "}"
    )));
    assert!(startup.contains(concat!(
        "pub struct RuntimeCompletedStartupRecoveryObservationV2 {\n",
        "    authorization: RuntimeAuthorizedStartupRecoveryObservationV2,\n",
        "    receipt: RuntimeStartupRecoveryObservationReceiptV2,\n",
        "}"
    )));
    assert!(startup.contains(concat!(
        "pub(crate) struct RuntimeValidatedStartupRecoveryObservationV2 {\n",
        "    operation_authority: RuntimeClosedRecoveryOperationAuthorityV2,\n",
        "    request: RuntimeStartupRecoveryObservationRequestV2,\n",
        "    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,\n",
        "    decision: RuntimeStartupRecoveryDecisionV2,\n",
        "}"
    )));
    assert!(startup.contains(concat!(
        "RuntimeCompletedStartupRecoveryObservationV2 {\n",
        "        authorization,\n",
        "        receipt,\n",
        "    } = completed;\n",
        "    let RuntimeAuthorizedStartupRecoveryObservationV2 {\n",
        "        request,\n",
        "        minimum_database_now,\n",
        "        operation_authority,\n",
        "    } = authorization;"
    )));
    let iteration_authorization = startup
        .split("pub(crate) fn authorize_startup_recovery_iteration_v2(")
        .nth(1)
        .and_then(|source| {
            source
                .split("\npub(crate) fn authorize_startup_recovery_observation_v2(")
                .next()
        })
        .unwrap();
    assert!(iteration_authorization
        .contains("request: startup_recovery_observation_request_v2(permit)"));
    assert!(iteration_authorization.contains("operation_authority,"));
    let authorization = startup
        .split("pub(crate) fn authorize_startup_recovery_observation_v2(")
        .nth(1)
        .and_then(|source| {
            source
                .split("\npub(crate) fn validate_startup_recovery_observation_v2(")
                .next()
        })
        .unwrap();
    let iteration = authorization
        .find("let RuntimeAuthorizedStartupRecoveryIterationV2")
        .unwrap();
    let exact_request = authorization
        .find("request != startup_recovery_observation_request_v2(permit)")
        .unwrap();
    let minimum_database_now = authorization.find("let minimum_database_now").unwrap();
    let authorized = authorization
        .find("Some(RuntimeAuthorizedStartupRecoveryObservationV2")
        .unwrap();
    assert!(
        iteration < exact_request
            && exact_request < minimum_database_now
            && minimum_database_now < authorized
    );
    assert!(!authorization.contains("take_operation_authority"));
    let validation = startup
        .split("pub(crate) fn validate_startup_recovery_observation_v2(")
        .nth(1)
        .and_then(|source| {
            source
                .split("\nfn startup_recovery_observation_request_v2(")
                .next()
        })
        .unwrap();
    let completion = validation
        .find("RuntimeCompletedStartupRecoveryObservationV2")
        .unwrap();
    let authorization = validation
        .find("let RuntimeAuthorizedStartupRecoveryObservationV2")
        .unwrap();
    let decision = validation
        .find("let decision =\n        plan_runtime_startup_recovery_v2")
        .unwrap();
    let validated = validation
        .find("Ok(RuntimeValidatedStartupRecoveryObservationV2")
        .unwrap();
    assert!(completion < authorization && authorization < decision && decision < validated);
    for required in [
        "request != startup_recovery_observation_request_v2(permit)",
        "receipt.correlation != request.correlation",
        "observed_owner.lease_id != request.gateway_owner_lease_id",
        "observed_owner.owner_revision != request.expected_owner_revision",
        "observed_owner.expires_at != request.expected_owner_expires_at",
        "observed_owner.database_now < minimum_database_now",
        "observed_owner.database_lease_duration().is_none()",
        "owner_receipt: receipt.owner_receipt",
    ] {
        assert!(validation.contains(required), "{required}");
    }

    let restore = closed
        .split("pub(crate) fn restore_operation_authority(")
        .nth(1)
        .and_then(|source| {
            source
                .split("\n    pub(crate) fn refresh_readiness(")
                .next()
        })
        .unwrap();
    let revision = restore
        .find("let authority_revision = self.authority_revision.successor()?")
        .unwrap();
    let token = restore
        .find("self.operation_authority = Some(authority)")
        .unwrap();
    let database_now = restore
        .find("self.last_startup_observation_database_now = Some(database_now)")
        .unwrap();
    let publication = restore
        .find("self.authority_revision = authority_revision")
        .unwrap();
    assert!(revision < token && token < database_now && database_now < publication);
    let readiness = closed
        .split("pub(crate) fn refresh_readiness(")
        .nth(1)
        .and_then(|source| {
            source
                .split("\n    pub(crate) fn advance_fixed_point(")
                .next()
        })
        .unwrap();
    let take = readiness
        .find("let authority = self.take_operation_authority()?")
        .unwrap();
    let readiness_revision = readiness
        .find("let authority_revision = self.authority_revision.successor()?")
        .unwrap();
    let readiness_evidence = readiness.find("self.readiness = readiness").unwrap();
    let readiness_publication = readiness
        .find("self.authority_revision = authority_revision")
        .unwrap();
    assert!(
        readiness_revision < take
            && take < readiness_evidence
            && readiness_evidence < readiness_publication
    );
    let fixed_point_advance = closed
        .split("pub(crate) fn advance_fixed_point(")
        .nth(1)
        .and_then(|source| source.split("\n    #[cfg(test)]").next())
        .unwrap();
    let fixed_unavailable = fixed_point_advance
        .find("if self.operation_authority.is_some()")
        .unwrap();
    let fixed_revision = fixed_point_advance
        .find("let authority_revision = self.authority_revision.successor()?")
        .unwrap();
    let fixed_database_now = fixed_point_advance
        .find("self.last_startup_observation_database_now = Some(database_now)")
        .unwrap();
    let fixed_publication = fixed_point_advance
        .find("self.authority_revision = authority_revision")
        .unwrap();
    assert!(
        fixed_unavailable < fixed_revision
            && fixed_revision < fixed_database_now
            && fixed_database_now < fixed_publication
    );

    let acceptance = startup
        .split("pub(crate) fn accept_validated_startup_recovery_observation_v2(")
        .nth(1)
        .and_then(|source| {
            source
                .split("\npub(crate) fn startup_recovery_fixed_point_matches_permit_v2(")
                .next()
        })
        .unwrap();
    assert_eq!(
        acceptance
            .matches("permit.restore_operation_authority(operation_authority, database_now)?")
            .count(),
        1
    );
    assert_eq!(
        acceptance
            .matches("permit.restore_operation_authority_for_recovery(")
            .count(),
        1
    );
    assert_eq!(
        acceptance
            .matches("permit.advance_fixed_point(database_now)?")
            .count(),
        1
    );
    let accepted_parts = acceptance
        .find("let (operation_authority, request, owner_receipt, decision)")
        .unwrap();
    let accepted_database_now = acceptance
        .find("let database_now = owner_receipt.database_now")
        .unwrap();
    let fixed_advance = acceptance
        .find("let authority_revision = permit.advance_fixed_point(database_now)?")
        .unwrap();
    let proof = acceptance
        .find("RuntimeStartupRecoveryFixedPointProofV2 {")
        .unwrap();
    assert!(
        accepted_parts < accepted_database_now
            && accepted_database_now < fixed_advance
            && fixed_advance < proof
    );
    for required in [
        "RuntimeAcceptedStartupRecoveryOutcomeV2::Continue(",
        "RuntimeAcceptedStartupRecoveryOutcomeV2::FixedPoint(",
        "operation_authority,",
        "correlation: request.correlation",
        "successor_authority_revision: authority_revision",
        "owner_receipt,",
        "acknowledged_product_handoff_count: fixed_point",
    ] {
        assert!(acceptance.contains(required), "{required}");
    }
    let fixed_point_matcher = startup
        .split("pub(crate) fn startup_recovery_fixed_point_matches_permit_v2(")
        .nth(1)
        .and_then(|source| {
            source
                .split("\nfn startup_recovery_observation_request_v2(")
                .next()
        })
        .unwrap();
    for required in [
        "!permit.operation_authority_is_available()",
        "proof.correlation.recovery_id == *permit.recovery_id()",
        "proof.correlation.originating_emergency_generation",
        "generation_value(permit.originating_emergency_generation())",
        "proof.correlation.coordinator_generation",
        "generation_value(permit.coordinator_generation())",
        "proof.correlation.authority_revision.get().checked_add(1)",
        "Some(proof.successor_authority_revision.get())",
        "proof.successor_authority_revision == permit.authority_revision()",
        "proof.owner_receipt.lease_id == owner.lease_id",
        "proof.owner_receipt.owner_revision == owner.owner_revision",
        "proof.owner_receipt.expires_at == owner.expires_at",
        "proof.owner_receipt.database_now >= owner.database_now",
        "proof.owner_receipt.database_lease_duration().is_some()",
        "permit.last_startup_observation_database_now() == Some(proof.owner_receipt.database_now)",
    ] {
        assert!(fixed_point_matcher.contains(required), "{required}");
    }

    let port = startup
        .split("pub trait RuntimeStartupRecoveryObservationPortV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    assert_eq!(port.matches("fn observe_startup_recovery(").count(), 1);
    assert!(port.contains("authorization: RuntimeAuthorizedStartupRecoveryObservationV2"));
    assert!(port.contains("operation_cutoff: Instant"));
    for forbidden in [
        "recover_next_stale_live",
        "resume",
        "deploy",
        "activate",
        "mutate",
        "retry",
    ] {
        assert!(!port.contains(forbidden));
    }

    let begin = lifecycle
        .split("pub fn begin_startup_recovery_observation(")
        .nth(1)
        .and_then(|source| {
            source
                .split("\n    pub fn complete_startup_recovery_observation(")
                .next()
        })
        .unwrap();
    let begin_current = begin
        .find("self.validate_recovery_permit(permit)?")
        .unwrap();
    let begin_absent = begin
        .find("permit.operation_authority_is_available()")
        .unwrap();
    let begin_authorization = begin
        .find("authorize_startup_recovery_observation_v2(permit, iteration)")
        .unwrap();
    assert!(begin_current < begin_absent && begin_absent < begin_authorization);
    let complete = lifecycle
        .split("pub fn complete_startup_recovery_observation(")
        .nth(1)
        .and_then(|source| {
            source
                .split("\n    pub fn validate_startup_recovery_fixed_point(")
                .next()
        })
        .unwrap();
    let predecessor = complete
        .find("self.validate_recovery_permit(permit)?")
        .unwrap();
    let unavailable = complete
        .find("permit.operation_authority_is_available()")
        .unwrap();
    let receipt = complete
        .find("validate_startup_recovery_observation_v2(permit, completed)")
        .unwrap();
    let successor = complete
        .find("accept_validated_startup_recovery_observation_v2(permit, validated)")
        .unwrap();
    let publication = complete
        .find("self.snapshot = RuntimeGatewayClosedSnapshotV2::RecoveryPending")
        .unwrap();
    assert!(predecessor < unavailable && unavailable < receipt);
    assert!(receipt < successor && successor < publication);
    let refresh = lifecycle
        .split("pub fn refresh_recovery_readiness(")
        .nth(1)
        .and_then(|source| {
            source
                .split("\n    pub fn begin_startup_recovery_observation(")
                .next()
        })
        .unwrap();
    assert!(refresh.contains("!permit.operation_authority_is_available()"));
    let refresh_successor = refresh.find("permit.refresh_readiness(readiness)").unwrap();
    let refresh_iteration = refresh
        .find("authorize_startup_recovery_iteration_v2(permit, operation_authority)")
        .unwrap();
    let refresh_publication = refresh
        .find("self.snapshot = RuntimeGatewayClosedSnapshotV2::RecoveryPending")
        .unwrap();
    assert!(refresh_successor < refresh_iteration && refresh_iteration < refresh_publication);
    let fixed_point_validation = lifecycle
        .split("pub fn validate_startup_recovery_fixed_point(")
        .nth(1)
        .and_then(|source| source.split("\n    pub fn invalidate(").next())
        .unwrap();
    let fixed_current = fixed_point_validation
        .find("self.validate_recovery_permit(permit)?")
        .unwrap();
    let fixed_match = fixed_point_validation
        .find("startup_recovery_fixed_point_matches_permit_v2(permit, proof)")
        .unwrap();
    assert!(fixed_current < fixed_match);
    assert!(!complete.contains("restore_operation_authority"));
}

#[test]
fn startup_recovery_execution_is_linear_exactly_bound_and_progress_proven() {
    let execution = include_str!("../src/startup_recovery_execution.rs");
    let closed = include_str!("../src/closed_recovery.rs");
    let lifecycle = include_str!("../src/gateway_lifecycle.rs");

    for authority in [
        "RuntimeStartupRecoveryExecutionRequestV2",
        "RuntimeAuthorizedStartupRecoveryExecutionV2",
        "RuntimeStartupRecoveryExecutionReceiptV2",
        "RuntimeCompletedStartupRecoveryExecutionV2",
        "RuntimeAcceptedStartupRecoveryExecutionOutcomeV2",
        "RuntimeStartupRecoveryExecutionTerminalDigestV2",
    ] {
        let declaration = execution
            .split(&format!("pub struct {authority}"))
            .next()
            .unwrap();
        let attributes = declaration.rsplit_once("\n\n").unwrap().1;
        for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
            assert!(!attributes.contains(forbidden), "{authority}: {forbidden}");
            assert!(!implements_trait(execution, authority, forbidden));
        }
    }
    for authority in [
        "RuntimeStartupRecoveryExecutionRequestV2",
        "RuntimeAuthorizedStartupRecoveryExecutionV2",
        "RuntimeCompletedStartupRecoveryExecutionV2",
        "RuntimeAcceptedStartupRecoveryExecutionOutcomeV2",
    ] {
        let fields = execution
            .split(&format!("pub struct {authority} {{"))
            .nth(1)
            .and_then(|source| source.split("}\n\n").next())
            .unwrap();
        assert!(!fields.contains("pub "), "{authority}");
        assert!(execution.contains(&format!("{authority}(<redacted>)")));
    }
    assert!(execution.contains(concat!(
        "pub struct RuntimeStartupRecoveryExecutionReceiptV2 {\n",
        "    pub correlation: RuntimeStartupRecoveryExecutionCorrelationV2,\n",
        "    pub class: RuntimeStartupRecoveryClassV2,\n",
        "    pub owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,\n",
        "    pub outcome: RuntimeStartupRecoveryExecutionReceiptOutcomeV2,\n",
        "}"
    )));
    assert!(execution.contains(concat!(
        "Progressed {\n",
        "        action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,\n",
        "        terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,\n",
        "    }"
    )));
    assert!(!execution.contains("Progressed,"));
    assert!(execution.contains("if value == [0; 32]"));
    assert!(execution.contains("RuntimeStartupRecoveryExecutionDigestErrorV2::Zero"));
    let identity_impl = execution
        .split("impl RuntimeStartupRecoveryExecutionActionIdentityV2 {")
        .nth(1)
        .and_then(|source| source.split("\n}\n\nimpl Debug").next())
        .unwrap();
    assert!(!identity_impl.contains("fn new("));
    for identity in [
        "RuntimeStartupRecoveryExecutionCorrelationV2",
        "RuntimeStartupRecoveryExecutionActionIdentityV2",
    ] {
        let fields = execution
            .split(&format!("pub struct {identity} {{"))
            .nth(1)
            .and_then(|source| source.split("}\n\n").next())
            .unwrap();
        assert!(!fields.contains("pub "), "{identity}");
    }
    assert_eq!(
        execution
            .matches("action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2 {\n",)
            .count(),
        1
    );
    assert!(execution.contains(concat!(
        "pub struct RuntimeAuthorizedStartupRecoveryExecutionV2 {\n",
        "    request: RuntimeStartupRecoveryExecutionRequestV2,\n",
        "    operation_authority: RuntimeClosedRecoveryOperationAuthorityV2,\n",
        "}"
    )));
    assert!(execution.contains(concat!(
        "pub struct RuntimeCompletedStartupRecoveryExecutionV2 {\n",
        "    authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,\n",
        "    receipt: RuntimeStartupRecoveryExecutionReceiptV2,\n",
        "    pending_drain_proof: Option<crate::startup_pending_drain::RuntimePendingDrainExecutionProofV2>,\n",
        "    pending_registry_successor: Option<RuntimeRegistryRecoveryEmptyObservationV2>,\n",
        "}"
    )));
    assert!(execution.contains(concat!(
        "pub(crate) struct RuntimeValidatedStartupRecoveryExecutionV2 {\n",
        "    operation_authority: RuntimeClosedRecoveryOperationAuthorityV2,\n",
        "    request: RuntimeStartupRecoveryExecutionRequestV2,\n",
        "    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,\n",
        "    outcome: RuntimeStartupRecoveryExecutionReceiptOutcomeV2,\n",
        "    pending_drain_proof: Option<crate::startup_pending_drain::RuntimePendingDrainExecutionProofV2>,\n",
        "    pending_registry_successor: Option<RuntimeRegistryRecoveryEmptyObservationV2>,\n",
        "}"
    )));

    for field in [
        "correlation",
        "class",
        "action_identity",
        "gateway_owner_lease_id",
        "expected_owner_revision",
        "expected_owner_expires_at",
        "minimum_database_now",
        "readiness",
        "paused_gateway",
        "registry_process_instance_id",
        "registry_observation_sequence",
        "registry_retained_slot_count",
        "registry_retained_empty_tombstone_count",
    ] {
        assert!(
            execution.contains(&format!("    {field}:")),
            "missing execution request binding: {field}"
        );
    }
    let port = execution
        .split("pub trait RuntimeStartupRecoveryExecutionPortV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\n#[derive").next())
        .unwrap();
    assert_eq!(port.matches("fn execute_startup_recovery(").count(), 1);
    assert!(port.contains("authorization: RuntimeAuthorizedStartupRecoveryExecutionV2"));
    assert!(port.contains("operation_cutoff: Instant"));
    assert!(port.contains("RuntimeCompletedStartupRecoveryExecutionV2"));
    for forbidden in [
        "deploy",
        "activate",
        "admission",
        "Discord",
        "sqlx",
        "twilight",
    ] {
        assert!(!port.contains(forbidden), "{forbidden}");
    }

    let validation = execution
        .split("pub(crate) fn validate_startup_recovery_execution_v2(")
        .nth(1)
        .and_then(|source| {
            source
                .split("\npub(crate) fn accept_validated_startup_recovery_execution_v2(")
                .next()
        })
        .unwrap();
    for required in [
        "validate_execution_request_binding_v2(permit, &request)?",
        "receipt.correlation != request.correlation",
        "receipt.class != request.class",
        "observed_owner.lease_id != request.gateway_owner_lease_id",
        "observed_owner.owner_revision != request.expected_owner_revision",
        "observed_owner.expires_at != request.expected_owner_expires_at",
        "observed_owner.database_now < request.minimum_database_now",
        "observed_owner.database_lease_duration()",
        "action_identity != &request.action_identity",
        "retry_after.is_zero() || *retry_after > available",
    ] {
        assert!(validation.contains(required), "{required}");
    }
    let request_binding = execution
        .split("fn validate_execution_request_binding_v2(")
        .nth(1)
        .and_then(|source| {
            source
                .split("\nfn startup_recovery_execution_request_v2(")
                .next()
        })
        .unwrap();
    for required in [
        "correlation.recovery_id != *permit.recovery_id()",
        "correlation.authority_revision.get() != permit.authority_revision().get()",
        "correlation.selection_authority_revision",
        "pending.selection_correlation().authority_revision",
        "request.class != pending.class()",
        "request.readiness != *permit.readiness()",
        "request.paused_gateway != *permit.paused_gateway()",
        "request.registry_observation_sequence != registry.observation_sequence()",
    ] {
        assert!(request_binding.contains(required), "{required}");
    }

    assert!(closed.contains(concat!(
        "pending_startup_recovery_execution: Option<RuntimePendingStartupRecoveryExecutionV2>,\n",
        "    last_startup_observation_database_now: Option<DateTime<Utc>>,"
    )));
    let recovery_restore = closed
        .split("pub(crate) fn restore_operation_authority_for_recovery(")
        .nth(1)
        .and_then(|source| {
            source
                .split("\n    pub(crate) fn restore_after_startup_recovery_execution(")
                .next()
        })
        .unwrap();
    let successor = recovery_restore
        .find("let authority_revision = self.authority_revision.successor()?")
        .unwrap();
    let authority = recovery_restore
        .find("self.operation_authority = Some(authority)")
        .unwrap();
    let pending = recovery_restore
        .find("self.pending_startup_recovery_execution =")
        .unwrap();
    let publication = recovery_restore
        .find("self.authority_revision = authority_revision")
        .unwrap();
    assert!(successor < authority && authority < pending && pending < publication);
    let execution_restore = closed
        .split("pub(crate) fn restore_after_startup_recovery_execution(")
        .nth(1)
        .and_then(|source| {
            source
                .split("\n    pub(crate) fn refresh_readiness(")
                .next()
        })
        .unwrap();
    let exact_pending = execution_restore
        .find("pending.selection_correlation.authority_revision != selection_authority_revision")
        .unwrap();
    let successor = execution_restore
        .find("let authority_revision = self.authority_revision.successor()?")
        .unwrap();
    let authority = execution_restore
        .find("self.operation_authority = Some(authority)")
        .unwrap();
    let clear = execution_restore
        .find("self.pending_startup_recovery_execution = None")
        .unwrap();
    let publication = execution_restore
        .find("self.authority_revision = authority_revision")
        .unwrap();
    assert!(
        exact_pending < successor
            && successor < authority
            && authority < clear
            && clear < publication
    );

    let begin = lifecycle
        .split("pub fn begin_startup_recovery_execution(")
        .nth(1)
        .and_then(|source| {
            source
                .split("\n    pub fn complete_startup_recovery_execution(")
                .next()
        })
        .unwrap();
    let current = begin
        .find("self.validate_recovery_permit(permit)?")
        .unwrap();
    let available = begin
        .find("!permit.operation_authority_is_available()")
        .unwrap();
    let pending = begin
        .find("permit\n            .pending_startup_recovery_execution()")
        .unwrap();
    let class = begin.find("if class != pending_class").unwrap();
    let authorization = begin
        .find("authorize_startup_recovery_execution_v2(")
        .unwrap();
    assert!(current < available && available < pending && pending < class && class < authorization);
    let complete = lifecycle
        .split("pub fn complete_startup_recovery_execution(")
        .nth(1)
        .and_then(|source| {
            source
                .split("\n    pub fn validate_startup_recovery_fixed_point(")
                .next()
        })
        .unwrap();
    let current = complete
        .find("self.validate_recovery_permit(permit)?")
        .unwrap();
    let unavailable = complete
        .find("permit.operation_authority_is_available()")
        .unwrap();
    let validation = complete
        .find("validate_startup_recovery_execution_v2(permit, completed)")
        .unwrap();
    let acceptance = complete
        .find("accept_validated_startup_recovery_execution_v2(permit, validated)")
        .unwrap();
    let publication = complete
        .find("self.snapshot = RuntimeGatewayClosedSnapshotV2::RecoveryPending")
        .unwrap();
    assert!(
        current < unavailable
            && unavailable < validation
            && validation < acceptance
            && acceptance < publication
    );
}

#[test]
fn pending_drain_compound_authority_is_linear_and_registry_rollover_gated() {
    let source = include_str!("../src/startup_pending_drain.rs");
    for authority in [
        "RuntimeAuthorizedPendingDrainSelectionV2",
        "RuntimeSelectedPendingDrainCandidateV2",
        "RuntimeSelectedPendingDrainNoCandidateV2",
        "RuntimeAuthorizedPendingDrainClaimV2",
        "RuntimeAuthorizedPendingDrainAcknowledgementV2",
        "RuntimeDurablyAcknowledgedPendingDrainV2",
    ] {
        let declaration = source
            .split(&format!("pub struct {authority} {{"))
            .next()
            .unwrap();
        let attributes = declaration.rsplit_once("\n\n").unwrap().1;
        for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
            assert!(!attributes.contains(forbidden), "{authority}: {forbidden}");
            assert!(!implements_trait(source, authority, forbidden));
        }
        let fields = source
            .split(&format!("pub struct {authority} {{"))
            .nth(1)
            .and_then(|value| value.split("}\n\n").next())
            .unwrap();
        assert!(!fields.contains("pub "), "{authority}");
        assert!(source.contains(&format!("{authority}(<redacted>)")));
    }
    for required in [
        "pub fn accept_selection(\n        self,",
        "pub fn bind_registry_seal(\n        self,",
        "pub fn complete_registry_rollover(\n        self,",
        "pub fn seal_witness(&self) -> &RuntimePendingDrainRegistrySealWitnessV2",
        "validate_registry_rollover_v2(&self.seal, &unseal)?",
        "claim_action_identity != *self.authorization.request().action_identity()",
        "prior_claim_terminal_digest.as_bytes()",
        "RuntimePendingDrainExecutionProofV2::Compound",
    ] {
        assert!(source.contains(required), "{required}");
    }
    let durable = source
        .split("pub struct RuntimeDurablyAcknowledgedPendingDrainV2 {")
        .nth(1)
        .unwrap();
    let seal_access = durable.find("pub fn seal_witness(&self)").unwrap();
    let consume = durable
        .find("pub fn complete_registry_rollover(\n        self,")
        .unwrap();
    let validation = durable
        .find("validate_registry_rollover_v2(&self.seal, &unseal)?")
        .unwrap();
    let completion = durable
        .find("self.authorization.complete_pending_drain(")
        .unwrap();
    assert!(seal_access < consume && consume < validation && validation < completion);

    for (port_name, method, authority, receipt) in [
        (
            "RuntimePendingDrainNoCandidateRecorderPortV2",
            "record_pending_drain_no_candidate",
            "RuntimeSelectedPendingDrainNoCandidateV2",
            "RuntimePendingDrainNoCandidateReceiptV2",
        ),
        (
            "RuntimePendingDrainClaimExecutionPortV2",
            "execute_pending_drain_claim",
            "RuntimeAuthorizedPendingDrainClaimV2",
            "RuntimePendingDrainClaimReceiptV2",
        ),
        (
            "RuntimePendingDrainAcknowledgementExecutionPortV2",
            "execute_pending_drain_acknowledgement",
            "RuntimeAuthorizedPendingDrainAcknowledgementV2",
            "RuntimePendingDrainAcknowledgementReceiptV2",
        ),
    ] {
        let port = source
            .split(&format!("pub trait {port_name} {{"))
            .nth(1)
            .and_then(|value| value.split("\n}\n\n").next())
            .unwrap();
        assert_eq!(port.matches(&format!("fn {method}(")).count(), 1);
        assert!(port.contains(&format!("&{authority}")));
        assert!(port.contains("operation_cutoff: Instant"));
        assert!(port.contains(&format!("Result<{receipt}, Self::Error>")));
        assert!(port.contains("impl Future"));
        assert!(port.contains("+ Send"));
        for forbidden in [
            "RuntimeClosedRecoveryOperationAuthorityV2",
            "RuntimeAuthorizedStartupRecoveryExecutionV2",
            "RuntimeCompletedStartupRecoveryExecutionV2",
            "RuntimeDurablyAcknowledgedPendingDrainV2",
            "deploy",
            "activate",
            "admission",
            "Discord",
            "sqlx",
            "twilight",
        ] {
            assert!(!port.contains(forbidden), "{port_name}: {forbidden}");
        }
    }
}

#[test]
fn pending_drain_v3_succession_authority_is_linear_compact_and_outcome_bound() {
    let source = include_str!("../src/startup_pending_drain/v3.rs");
    let parent = include_str!("../src/startup_pending_drain.rs");
    let execution = include_str!("../src/startup_recovery_execution.rs");
    for authority in [
        "RuntimeAuthorizedPendingDrainSelectionV3",
        "RuntimePendingDrainFreshPreviousOwnerSelectionV3",
        "RuntimeSelectedPendingDrainSuccessionV3",
        "RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3",
        "RuntimeDurablyAcknowledgedPendingDrainSuccessionV3",
    ] {
        let declaration = source
            .split(&format!("pub struct {authority} {{"))
            .next()
            .unwrap();
        let attributes = declaration.rsplit_once("\n\n").unwrap().1;
        for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
            assert!(!attributes.contains(forbidden), "{authority}: {forbidden}");
            assert!(!implements_trait(source, authority, forbidden));
        }
        let fields = source
            .split(&format!("pub struct {authority} {{"))
            .nth(1)
            .and_then(|value| value.split("}\n\n").next())
            .unwrap();
        assert!(!fields.contains("pub "), "{authority}");
        assert!(source.contains(&format!("{authority}(<redacted>)")));
    }

    for required in [
        "pub struct RuntimePendingDrainPreviousOwnerClaimedCandidateInputV3",
        "source: RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2",
        "pub struct RuntimePendingDrainPreviousOwnerClaimedCandidateV3",
        "intent_id: RuntimeDrainIntentIdV2",
        "slot: RuntimeServingSlotV2",
        "expected_target: RuntimeDeploymentTargetV1",
        "source_intent_revision: NonZeroU64",
        "source_state_digest: RuntimePendingDrainStateDigestV2",
        "predecessor_claim: RuntimeDrainClaimV2",
        "predecessor_claim_terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2",
        "product_mutation_request_sha256: [u8; 32]",
        "drain_intent_request_sha256: [u8; 32]",
        "RuntimePendingDrainSelectionOutcomeV3::FreshPreviousOwner(candidate)",
        "RuntimePendingDrainSelectionOutcomeV3::ExpiredPreviousOwner(candidate)",
        "Duration::from_secs(1)",
        ".min(remaining_predecessor)",
        ".min(remaining_owner)",
        "validate_registry_rollover_v2(&self.seal, &unseal)?",
        "RuntimePendingDrainExecutionProofV2::Succession",
        "RuntimePendingDrainExecutionProofV2::Deferred",
    ] {
        assert!(source.contains(required), "{required}");
    }
    for forbidden in [
        "source_state_bytes",
        "sqlx",
        "twilight",
        "serde",
        "deploy",
        "activate",
    ] {
        assert!(!source.contains(forbidden), "{forbidden}");
    }
    let compact_candidate = source
        .split("pub struct RuntimePendingDrainPreviousOwnerClaimedCandidateV3 {")
        .nth(1)
        .and_then(|value| value.split("}\n\n").next())
        .unwrap();
    assert!(!compact_candidate.contains("RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2"));
    assert_eq!(
        source
            .matches(".pending_drain_acknowledgement_successor()")
            .count(),
        1
    );
    let unclaimed = source
        .find("RuntimePendingDrainSelectionOutcomeV3::Unclaimed(candidate)")
        .unwrap();
    let acknowledgement = source
        .find(".pending_drain_acknowledgement_successor()")
        .unwrap();
    assert!(unclaimed < acknowledgement);

    for (port_name, method, authority, receipt) in [
        (
            "RuntimePendingDrainSelectionPortV3",
            "select_pending_drain_v3",
            "RuntimeAuthorizedPendingDrainSelectionV3",
            "RuntimePendingDrainSelectionReceiptV3",
        ),
        (
            "RuntimePendingDrainSuccessionAcknowledgementExecutionPortV3",
            "execute_pending_drain_succession_acknowledgement",
            "RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3",
            "RuntimePendingDrainSuccessionAcknowledgementReceiptV3",
        ),
    ] {
        let port = source
            .split(&format!("pub trait {port_name} {{"))
            .nth(1)
            .and_then(|value| value.split("\n}\n\n").next())
            .unwrap();
        assert_eq!(port.matches(&format!("fn {method}(")).count(), 1);
        assert!(port.contains(&format!("&{authority}")));
        assert!(port.contains("operation_cutoff: Instant"));
        assert!(port.contains(&format!("Result<{receipt}, Self::Error>")));
        assert!(port.contains("impl Future"));
        assert!(port.contains("+ Send"));
    }

    assert!(parent.contains("pub(crate) fn matches_outcome("));
    assert!(parent.contains("Self::Deferred(proof)"));
    assert!(parent.contains("Self::Succession(proof)"));
    assert!(execution.contains("|| !proof.matches_outcome(&receipt.outcome)"));
}

#[test]
fn pending_drain_v4_authority_is_linear_and_ports_require_checked_receipts() {
    let model = include_str!("../src/startup_pending_drain/v4.rs");
    let mutation = include_str!("../src/startup_pending_drain/v4/mutation.rs");
    let orchestration = include_str!("../src/startup_pending_drain/v4/orchestration.rs");
    let selection = include_str!("../src/startup_pending_drain/v4/selection.rs");
    let terminal = include_str!("../src/startup_pending_drain/v4/terminal.rs");
    let source = format!("{model}\n{selection}\n{mutation}\n{terminal}\n{orchestration}");
    assert!(model.lines().count() < 900);
    for required in [
        "RuntimePendingDrainCandidateEvidenceInputV4",
        "RuntimePendingDrainSelectionClassV4",
        "RuntimeAuthorizedPendingDrainSelectionV4",
    ] {
        assert!(selection.contains(required), "{required}");
    }
    for forbidden in [
        "RuntimePendingDrainMutationReceiptV4",
        "RuntimePendingDrainTerminalIdentityV4",
        "RuntimePendingDrainRegistryTransitionPortV4",
    ] {
        assert!(!selection.contains(forbidden), "{forbidden}");
    }
    for required in [
        "RuntimePendingDrainActionIdentityV4",
        "RuntimeRoutedSealedWitnessV4",
        "RuntimeDurableRoutedClaimReceiptV4",
        "RuntimeDurableRefenceReceiptV4",
    ] {
        assert!(mutation.contains(required), "{required}");
    }
    for required in [
        "RuntimePendingDrainTerminalIdentityV4",
        "RuntimePendingDrainUnknownResultV4",
        "RuntimePendingDrainFinalizerRegistrationV4",
    ] {
        assert!(terminal.contains(required), "{required}");
    }
    for required in [
        "RuntimePendingDrainRegistryTransitionPortV4",
        "RuntimeRoutedDrainClaimExecutionPortV4",
        "RuntimePreviousProcessDrainTeardownExecutionPortV4",
    ] {
        assert!(orchestration.contains(required), "{required}");
    }
    for authority in [
        "RuntimeAuthorizedPendingDrainSelectionV4",
        "RuntimeSelectedUnclaimedPendingDrainV4",
        "RuntimeSelectedCurrentRouteAbsentClaimedV4",
        "RuntimeSelectedCurrentRoutedClaimedV4",
        "RuntimeSelectedCurrentRefencedV4",
        "RuntimeReconstructedDurablyRefencedV4",
        "RuntimeAuthorizedRoutedDrainClaimV4",
        "RuntimeAuthorizedDrainRefenceProgressV4",
        "RuntimeAuthorizedSameProcessDrainAcknowledgementV4",
        "RuntimeAuthorizedPreviousProcessDrainTeardownV4",
        "RuntimeDurableRoutedClaimReceiptV4",
        "RuntimeDurableRefenceReceiptV4",
        "RuntimeDurableSameProcessDrainAcknowledgementV4",
        "RuntimeDurablePreviousProcessDrainTeardownV4",
        "RuntimeRoutedDrainRollbackPermitV4",
        "RuntimeRoutedSealedClaimV4",
        "RuntimeDurableRoutedClaimBoundaryV4",
        "RuntimeRoutedDrainRollbackAuthorizationV4",
        "RuntimeRoutedClaimedContinuationV4",
        "RuntimePendingDrainLaneJoinedV4",
        "RuntimePendingDrainServingResolvedV4",
        "RuntimeAuthorizedRegistryRefenceV4",
        "RuntimeLocalRefenceProgressV4",
        "RuntimeDurableRefenceBoundaryV4",
        "RuntimeDurablyRefencedBoundaryV4",
        "RuntimeRouteAbsentAcknowledgementV4",
        "RuntimeDurableSameProcessAcknowledgementBoundaryV4",
        "RuntimePreviousProcessTeardownV4",
        "RuntimeDurablePreviousProcessTeardownBoundaryV4",
    ] {
        let marker = format!("pub struct {authority}");
        let start = source.find(&marker).unwrap();
        let attributes = source[..start].rsplit_once("\n\n").unwrap().1;
        for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
            assert!(!attributes.contains(forbidden), "{authority}: {forbidden}");
            assert!(!implements_trait(&source, authority, forbidden));
        }
        let declaration = &source[start + marker.len()..];
        let fields = declaration
            .split_once('{')
            .and_then(|(_, value)| value.split("\n}\n\n").next())
            .unwrap();
        assert!(!fields.contains("pub "), "{authority}");
        assert!(source.contains(&format!("{authority}(<redacted>)")));
    }

    let registry = orchestration
        .split("pub trait RuntimePendingDrainRegistryTransitionPortV4 {")
        .nth(1)
        .and_then(|value| value.split("\n}\n\n").next())
        .unwrap();
    for required in [
        "RuntimeRoutedSealPortObservationV4",
        "RuntimeRoutedClaimedSealPortObservationV4",
        "RuntimeLocalRefencePortObservationV4",
        "RuntimeDurableRefencePortObservationV4",
        "RuntimeRouteAbsentPortObservationV4",
        "RuntimeEmptySuccessionPortObservationV4",
        "RuntimeAuthorizedRegistryRefenceEvidenceV4",
        "RuntimeSelectedExpiredPreviousOwnerV4",
        "RuntimeDurableRoutedClaimReceiptV4",
        "RuntimeDurableRefenceReceiptV4",
        "RuntimeDurableSameProcessDrainAcknowledgementV4",
        "RuntimeDurablePreviousProcessDrainTeardownV4",
    ] {
        assert!(registry.contains(required), "{required}");
    }
    for forbidden in [
        "RuntimePendingDrainEvidenceDigestV4",
        "[u8; 32]",
        "RuntimeDrainIntentIdV2",
        "ProcessInstanceId",
    ] {
        assert!(!registry.contains(forbidden), "{forbidden}");
    }

    let rollback = orchestration
        .split("pub trait RuntimeRoutedDrainRollbackPortV4 {")
        .nth(1)
        .and_then(|value| value.split("\n}\n\n").next())
        .unwrap();
    assert!(rollback.contains("source: Self::RoutedSealed"));
    assert!(rollback.contains("permit: RuntimeRoutedDrainRollbackPermitV4"));
    for forbidden in [
        "RuntimePendingDrainEvidenceDigestV4",
        "[u8; 32]",
        "RuntimeDrainIntentIdV2",
        "ProcessInstanceId",
    ] {
        assert!(!rollback.contains(forbidden), "{forbidden}");
    }

    for (port_name, authorization) in [
        (
            "RuntimeRoutedDrainClaimExecutionPortV4",
            "RuntimeRoutedSealedClaimV4",
        ),
        (
            "RuntimeDrainRefenceProgressExecutionPortV4",
            "RuntimeLocalRefenceProgressV4",
        ),
        (
            "RuntimeSameProcessDrainAcknowledgementExecutionPortV4",
            "RuntimeRouteAbsentAcknowledgementV4",
        ),
        (
            "RuntimePreviousProcessDrainTeardownExecutionPortV4",
            "RuntimePreviousProcessTeardownV4",
        ),
    ] {
        let port = orchestration
            .split(&format!("pub trait {port_name} {{"))
            .nth(1)
            .and_then(|value| value.split("\n}\n\n").next())
            .unwrap();
        assert!(port.contains("RuntimeRegisteredPendingDrainFinalizerV4<"));
        assert!(port.contains(authorization));
        assert!(port.contains("operation_cutoff: Instant"));
    }

    let root = include_str!("../src/lib.rs");
    for forbidden in [
        "RuntimeRoutedSealedWitnessInputV4",
        "RuntimeRoutedClaimedSealedWitnessInputV4",
        "RuntimeLocallyRefencedSealedWitnessInputV4",
        "RuntimeDurablyRefencedSealedWitnessInputV4",
        "RuntimeRouteAbsentSealedWitnessInputV4",
        "RuntimeEmptySuccessionSealedWitnessInputV4",
        "RuntimePendingDrainMutationReceiptInputV4",
        "RuntimePendingDrainMutationReceiptV4",
        "RuntimeAuthorizedRoutedDrainClaimV4",
        "RuntimeAuthorizedDrainRefenceProgressV4",
        "RuntimeAuthorizedSameProcessDrainAcknowledgementV4",
        "RuntimeAuthorizedPreviousProcessDrainTeardownV4",
        "RuntimeRoutedDrainClaimReceiptV4",
        "RuntimeDrainRefenceProgressReceiptV4",
        "RuntimeSameProcessDrainAcknowledgementReceiptV4",
        "RuntimePreviousProcessDrainTeardownReceiptV4",
    ] {
        assert!(!contains_identifier(root, forbidden), "{forbidden}");
    }
}

#[test]
fn worker_coordinator_authority_and_state_surface_stay_exact() {
    let lifecycle = include_str!("../src/gateway_lifecycle.rs");
    let invalidation = lifecycle
        .split("pub enum RuntimeGatewayInvalidationCauseV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl From").next())
        .unwrap();

    assert!(lifecycle.contains(concat!(
        "#[derive(Debug, PartialEq, Eq)]\n",
        "pub struct RuntimeGatewayClosedLifecycleV2 {\n",
        "    snapshot: RuntimeGatewayClosedSnapshotV2,\n",
        "}"
    )));
    assert!(!lifecycle.contains("impl Clone for RuntimeGatewayClosedLifecycleV2"));
    assert!(!lifecycle.contains("impl Default for RuntimeGatewayClosedLifecycleV2"));
    assert!(lifecycle.contains("pub enum RuntimeGatewayClosedSnapshotV2"));
    assert!(lifecycle.contains("Emergency {"));
    assert!(lifecycle.contains("RecoveryPending {"));
    assert!(lifecycle.contains("Shutdown {"));
    assert!(!lifecycle.contains("Open"));
    assert!(!lifecycle.contains("AdmissionAcknowledging"));
    assert!(!invalidation.contains("Starting"));
    assert_eq!(invalidation.matches("    ").count(), 5);
    let refresh = lifecycle
        .split("pub fn refresh_recovery_readiness(")
        .nth(1)
        .and_then(|source| source.split("\n    pub fn invalidate(").next())
        .unwrap();
    let current = refresh
        .find("self.validate_recovery_permit(permit)?")
        .unwrap();
    let authority = refresh
        .find("permit.readiness().has_same_authority_as(&readiness)")
        .unwrap();
    let freshness = refresh
        .find("readiness.has_strictly_newer_checks_than(permit.readiness())")
        .unwrap();
    let successor = refresh.find("permit.refresh_readiness(readiness)").unwrap();
    let publication = refresh
        .find("self.snapshot = RuntimeGatewayClosedSnapshotV2::RecoveryPending")
        .unwrap();
    assert!(current < authority && authority < freshness && freshness < successor);
    assert!(successor < publication);
    assert!(!refresh.contains("async"));
}

#[test]
fn closed_recovery_authority_is_narrow_noncloneable_and_state_only() {
    let source = include_str!("../src/closed_recovery.rs");
    let permit_declaration = source
        .split("pub struct RuntimeClosedDrainRecoveryPermitV2 {")
        .next()
        .unwrap();
    let permit_attributes = permit_declaration.rsplit_once("\n\n").unwrap().1;
    let input_declaration = source
        .split("pub struct RuntimeClosedRecoveryInputV2 {")
        .next()
        .unwrap();
    let input_attributes = input_declaration.rsplit_once("\n\n").unwrap().1;
    let registry_declaration = source
        .split("pub enum RuntimeClosedRecoveryRegistryEvidenceV2 {")
        .next()
        .unwrap();
    let registry_attributes = registry_declaration.rsplit_once("\n\n").unwrap().1;
    let permit_fields = source
        .split("pub struct RuntimeClosedDrainRecoveryPermitV2 {")
        .nth(1)
        .and_then(|source| {
            source
                .split("}\n\nimpl RuntimeClosedDrainRecoveryPermitV2")
                .next()
        })
        .unwrap();
    let permit_impl = source
        .split("impl RuntimeClosedDrainRecoveryPermitV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl Debug").next())
        .unwrap();

    for attributes in [permit_attributes, input_attributes, registry_attributes] {
        for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
            assert!(!attributes.contains(forbidden));
        }
    }
    for authority in [
        "RuntimeClosedDrainRecoveryPermitV2",
        "RuntimeClosedRecoveryInputV2",
        "RuntimeClosedRecoveryRegistryEvidenceV2",
    ] {
        for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
            assert!(!implements_trait(source, authority, forbidden));
        }
    }
    assert!(!permit_fields.contains("pub "));
    assert_eq!(permit_impl.matches("pub(crate) fn new(").count(), 1);
    assert_eq!(
        permit_impl
            .matches("pub(crate) fn refresh_readiness(")
            .count(),
        1
    );
    assert!(!permit_impl.contains("pub fn refresh_readiness("));
    for forbidden in ["pub fn new(", "pub fn from_", "pub(crate) fn from_"] {
        assert!(!permit_impl.contains(forbidden));
    }
    for forbidden in [
        "pub fn new(value: NonZeroU64)",
        "advance_authority",
        "resume",
        "std::future",
        "async",
        "RuntimeGatewayControl",
        "automation_runtime_registry",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden closed recovery authority surface: {forbidden}"
        );
    }
    for expected in [
        "pub const FIRST: Self = Self(NonZeroU64::MIN);",
        "RuntimeClosedRecoveryInputV2(<redacted>)",
        "RuntimeClosedRecoveryRegistryEvidenceV2(<redacted>)",
        "pub enum RuntimeClosedRecoveryRegistryEvidenceV2 {\n    Empty(",
        "RuntimeClosedDrainRecoveryPermitV2(<redacted>)",
        "originating_emergency_generation",
        "coordinator_generation",
        "recovery_id",
        "authority_revision",
        "owner_receipt",
        "readiness",
        "paused_gateway",
        "registry_evidence",
    ] {
        assert!(source.contains(expected));
    }
}

#[test]
fn worker_writer_fence_surface_is_observe_only() {
    let source = include_str!("../src/writer_fence.rs");

    assert_eq!(source.matches("fn observe_writer_fence(").count(), 1);
    assert_eq!(source.matches("\n    fn ").count(), 1);
    for forbidden in [
        "close_writer_fence",
        "open_writer_fence",
        "renew_writer_fence",
        "acquire_writer_fence",
        "release_writer_fence",
        "Mutation",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden writer fence authority: {forbidden}"
        );
    }
}

#[test]
fn worker_gateway_owner_watchdog_state_is_nonclone_and_monotonic() {
    let source = include_str!("../src/gateway_owner_watchdog.rs");
    let owner_source = include_str!("../src/gateway_owner.rs");

    for authority in [
        "RuntimeGatewayOwnerRenewalScheduleV1",
        "RuntimeGatewayOwnerWatchdogV1",
        "RuntimeGatewayOwnerObservationInFlightV1",
        "RuntimeGatewayOwnerRenewalInFlightV1",
        "RuntimeGatewayOwnerUnknownRenewalV1",
    ] {
        assert!(source.contains(&format!(
            "#[derive(Debug, PartialEq, Eq)]\npub struct {authority}"
        )));
    }
    assert!(!source.contains("SystemTime"));
    assert!(!source.contains("Utc::now"));
    assert!(!source.contains("reconcile_observation"));
    assert!(!source.contains("RuntimeGatewayOwnerObservedWatchdogV1"));
    assert!(!source.contains("pub fn from_receipt"));
    assert!(source.contains("pub fn from_accepted_receipt"));
    assert!(source.contains(".checked_add(lease_duration)"));
    assert!(source.contains("response_observed_at >= safety_deadline"));
    assert!(owner_source.contains(concat!(
        "#[derive(Debug, PartialEq, Eq)]\n",
        "pub struct RuntimeAcceptedGatewayOwnerReceiptV1 {\n",
        "    receipt: RuntimeGatewayOwnerLeaseReceiptV1,\n",
        "}"
    )));
    assert!(!owner_source.contains("impl Clone for RuntimeAcceptedGatewayOwnerReceiptV1"));
}

#[test]
fn production_lifecycle_suffix_is_linear_pure_and_has_no_customer_ingress() {
    let model = include_str!("../src/production_lifecycle.rs");
    let handoff = include_str!("../src/production_lifecycle/handoff.rs");
    let admission = include_str!("../src/production_lifecycle/admission.rs");
    let refresh = include_str!("../src/production_lifecycle/refresh.rs");
    let serving = include_str!("../src/production_lifecycle/serving.rs");
    let serving_route_set = include_str!("../src/production_lifecycle/serving/route_set.rs");
    let serving_slot_work = include_str!("../src/production_lifecycle/serving/slot_work.rs");
    let shutdown = include_str!("../src/production_lifecycle/shutdown.rs");
    let source = format!(
        "{model}\n{handoff}\n{admission}\n{refresh}\n{serving}\n{serving_route_set}\n{serving_slot_work}\n{shutdown}"
    );
    let root = include_str!("../src/lib.rs");
    let closed = include_str!("../src/gateway_lifecycle.rs");

    assert!(model.lines().count() < 400);
    assert!(handoff.lines().count() < 500);
    assert!(admission.lines().count() < 700);
    assert!(refresh.lines().count() < 400);
    assert!(serving.lines().count() < 925);
    assert!(serving_route_set.lines().count() < 200);
    assert!(serving_slot_work.lines().count() < 350);
    assert!(shutdown.lines().count() < 650);

    for authority in [
        "RuntimeStartupRecoveryFixedPointProcessV2",
        "RuntimeProductionFixedPointAcceptanceFailureV2",
        "RuntimeProductionTransitionFailureV2",
        "RuntimeProductionHandoffProcessV2",
        "RuntimeRecoveryResumePermitV2",
        "RuntimeAdmissionAcknowledgingProcessV2",
        "RuntimeEmptyOpenEpochV2",
        "RuntimeEmptyOpenProcessV2",
        "RuntimeRouteSetEpochV2",
        "RuntimeServingOpenPreparedV2",
        "RuntimeServingOpenEpochV2",
        "RuntimeServingOpenProcessV2",
        "RuntimeServingSlotWorkRequestV2",
        "RuntimeServingSlotWorkPermitV2",
        "RuntimeProductionEmergencyProcessV2",
        "RuntimeShuttingDownProcessV2",
    ] {
        let marker = format!("pub struct {authority}");
        let start = source.find(&marker).unwrap();
        let attributes = source[..start].rsplit("\n\n").next().unwrap();
        for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
            assert!(
                !attributes.contains(forbidden),
                "{authority} derives {forbidden}"
            );
            assert!(
                !source.contains(&format!("{forbidden} for {authority}")),
                "{authority} implements {forbidden}"
            );
        }
        let fields = source[start..]
            .split_once('{')
            .unwrap()
            .1
            .split_once("\n}")
            .unwrap()
            .0;
        assert!(
            fields.lines().all(|line| !line.starts_with("    pub ")),
            "{authority} exposes authority fields"
        );
    }

    for transition in [
        "pub fn begin_production_handoff<P>(\n        self,",
        "pub fn resume_recovery<P>(\n        self,",
        "pub fn observe_open_production<P>(\n        self,",
        "pub fn prepare_serving_open<P>(\n        self,",
        "pub fn commit(self) -> RuntimeServingOpenProcessV2",
        "pub fn cancel(self) -> RuntimeEmptyOpenProcessV2",
        "pub fn authorize_slot_work(\n        &self,",
        "pub fn begin_slot_work(\n        &mut self,",
        "pub fn invalidate_production(\n        self,",
        "pub fn begin_shutdown(\n        self,",
    ] {
        assert!(source.contains(transition), "{transition}");
    }

    for port in [
        "RuntimeProductionHandoffObservationPortV2",
        "RuntimeRecoveryResumePortV2",
        "RuntimeOpenProductionObservationPortV2",
        "RuntimeServingOpenObservationPortV2",
    ] {
        let body = source
            .split(&format!("pub trait {port} {{"))
            .nth(1)
            .and_then(|value| value.split("\n}\n").next())
            .unwrap();
        assert_eq!(body.matches("\n    fn ").count(), 1, "{port}");
        assert!(!body.contains("RuntimeGatewayClosedLifecycleV2"), "{port}");
        assert!(
            !body.contains("RuntimeClosedDrainRecoveryPermitV2"),
            "{port}"
        );
    }

    for required in [
        "into_production_fixed_point",
        "RuntimeStartupRecoveryFixedPointProofV2",
        "RuntimeProductionLifecycleStageV2::AdmissionAcknowledging",
        "RuntimeProductionLifecycleStageV2::OpenProduction",
        "RuntimeProductionLifecycleStageV2::Shutdown",
        "RuntimeIngressOpenAcknowledgementObservationV2",
        "RuntimeRouteSetEpochV2",
        "RuntimeServingSlotWorkSupervisorV2",
        "RuntimeProductionInvalidationOutcomeV2",
        "RuntimeShutdownCauseV2::GenerationOverflow",
    ] {
        assert!(source.contains(required), "{required}");
    }

    for forbidden in [
        "RuntimePublicAdmissionPermit",
        "AdmittedInteraction",
        "execute_interaction",
        "interaction_consumer",
        "GatewayPauseToken",
        "SharedGatewayControl",
        "sqlx",
        "twilight",
        "tokio",
        "serde",
        "async fn",
    ] {
        assert!(!source.contains(forbidden), "{forbidden}");
    }

    for exported in [
        "RuntimeStartupRecoveryFixedPointProcessV2",
        "RuntimeProductionHandoffProcessV2",
        "RuntimeRecoveryResumePermitV2",
        "RuntimeAdmissionAcknowledgingProcessV2",
        "RuntimeEmptyOpenProcessV2",
        "RuntimeRouteSetEpochV2",
        "RuntimeServingOpenPreparedV2",
        "RuntimeServingOpenProcessV2",
        "RuntimeServingSlotWorkRequestV2",
        "RuntimeServingSlotWorkPermitV2",
        "RuntimeProductionEmergencyProcessV2",
        "RuntimeShuttingDownProcessV2",
    ] {
        assert!(contains_identifier(root, exported), "{exported}");
    }
    assert!(!contains_identifier(root, "RuntimePublicAdmissionPermitV2"));
    assert!(!closed.contains("AdmissionAcknowledging"));
    assert!(!closed.contains("OpenProduction"));
}

#[test]
fn serving_open_authority_is_linear_epoch_fenced_and_keyed() {
    let serving_root = include_str!("../src/production_lifecycle/serving.rs");
    let serving_route_set = include_str!("../src/production_lifecycle/serving/route_set.rs");
    let serving_slot_work = include_str!("../src/production_lifecycle/serving/slot_work.rs");
    let serving = format!("{serving_root}\n{serving_route_set}\n{serving_slot_work}");
    let shutdown = include_str!("../src/production_lifecycle/shutdown.rs");
    let lifecycle = include_str!("../src/production_lifecycle.rs");
    let root = include_str!("../src/lib.rs");

    let prepared = serving_root
        .split("pub struct RuntimeServingOpenPreparedV2 {")
        .nth(1)
        .and_then(|source| source.split("\n}\n").next())
        .unwrap();
    assert!(prepared.contains("state: Box<RuntimeEmptyOpenProcessV2>,"));
    assert!(prepared.contains("route_set_epoch: RuntimeRouteSetEpochV2,"));

    let process = serving_root
        .split("pub struct RuntimeServingOpenProcessV2 {")
        .nth(1)
        .and_then(|source| source.split("\n}\n").next())
        .unwrap();
    assert!(!process.contains("RuntimeEmptyOpenProcessV2"));
    assert!(process.contains("_admission: RuntimeAdmissionAcknowledgingProcessV2,"));
    assert!(process.contains("epoch: RuntimeServingOpenEpochV2,"));
    assert!(process.contains("supervisor: RuntimeServingSlotWorkSupervisorV2,"));

    let route_epoch = serving_root
        .split("pub struct RuntimeRouteSetEpochV2 {")
        .nth(1)
        .and_then(|source| source.split("impl Debug for RuntimeRouteSetEpochV2").next())
        .unwrap();
    assert!(!route_epoch.contains("pub fn new("));
    assert!(route_epoch.contains(
        "initial_registry_observation_sequence: RuntimeRegistryGlobalObservationSequenceV2,"
    ));
    assert!(route_epoch.contains("initial_retained_slot_count: u64,"));
    assert!(route_epoch.contains("initial_retained_empty_tombstone_count: u64,"));

    for required in [
        "let RuntimeEmptyOpenProcessV2 { _admission, epoch } = *state;",
        "registry_empty: _,",
        "RuntimeProductionLifecycleStageV2::OpenProduction",
        "observed.route_set.observation_sequence() != request.registry_observation_sequence",
        "observed.route_set.retained_slot_count()",
        "request.route_set_epoch.initial_retained_slot_count",
        "observed.route_set.retained_empty_tombstone_count()",
        "request\n                .route_set_epoch\n                .initial_retained_empty_tombstone_count",
        "observed.gateway_owner.database_now < previous_owner.database_now",
        "observed.writer_fence_open",
        "observed.maintenance_gate_open",
        "observed.finalizer_accepting",
        "ingress_acknowledgement_predecessor",
        "if !observed.supervisors_running",
        "BTreeMap<RuntimeServingSlotV2, NonZeroU64>",
        "state.active.contains_key(&slot)",
        "state.active.len() >= state.max_in_flight.get()",
        "impl Drop for RuntimeServingSlotWorkPermitV2",
        "state.active.get(&identity.slot)",
        "Arc::downgrade(&self.state)",
        "RuntimeServingSlotWorkErrorV2::StaleRouteSetEpoch",
        "RuntimeServingOpenAcknowledgementRefreshV2",
        "route_set_sequence < previous_route_set_sequence",
        "route_set_sequence == previous_route_set_sequence",
        "input.route_set != epoch.route_set",
    ] {
        assert!(serving.contains(required), "{required}");
    }

    for forbidden in [
        "successor_generation(",
        "RuntimeProductionLifecycleStageV2::Serving",
        "pub route_set_epoch:",
        "pub supervisor:",
        "async fn",
        "tokio",
        "sqlx",
        "twilight",
        "serde",
    ] {
        assert!(!serving.contains(forbidden), "{forbidden}");
    }

    for authority in [
        "RuntimeRouteSetEpochV2",
        "RuntimeServingOpenPreparedV2",
        "RuntimeServingOpenEpochV2",
        "RuntimeServingOpenProcessV2",
        "RuntimeServingSlotWorkRequestV2",
        "RuntimeServingSlotWorkPermitV2",
        "RuntimeServingOpenAcknowledgementRefreshV2",
    ] {
        assert!(
            serving.contains(&format!("{authority}(<redacted>)")),
            "{authority}"
        );
        for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
            assert!(
                !implements_trait(&serving, authority, forbidden),
                "{authority} implements {forbidden}"
            );
        }
        assert!(contains_identifier(lifecycle, authority), "{authority}");
        assert!(contains_identifier(root, authority), "{authority}");
    }

    assert!(shutdown.contains("RuntimeProductionEmergencySourceV2::ServingOpen"));
    assert!(shutdown.contains("RuntimeProductionTerminalSourceV2::ServingOpen"));
    assert!(shutdown.contains("impl RuntimeServingOpenProcessV2"));
}

#[test]
fn ingress_acknowledgement_authority_is_linear_replayable_and_controller_composed() {
    let source = include_str!("../src/ingress_acknowledgement.rs");
    let admission = include_str!("../src/production_lifecycle/admission.rs");
    let refresh = include_str!("../src/production_lifecycle/refresh.rs");
    let lifecycle = include_str!("../src/production_lifecycle.rs");
    let root = include_str!("../src/lib.rs");

    let authority = source
        .split("pub struct RuntimeAuthorizedIngressOpenAcknowledgementV2 {")
        .next()
        .unwrap()
        .rsplit_once("\n\n")
        .unwrap()
        .1;
    for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
        assert!(!authority.contains(forbidden), "{forbidden}");
        assert!(!source.contains(&format!(
            "impl {forbidden} for RuntimeAuthorizedIngressOpenAcknowledgementV2"
        )));
    }
    assert!(source.contains("authorization: &RuntimeAuthorizedIngressOpenAcknowledgementV2,"));
    assert_eq!(
        source
            .matches("authorization: &RuntimeAuthorizedIngressOpenAcknowledgementV2,")
            .count(),
        3
    );
    let accepted = source
        .split("pub struct RuntimeAcceptedIngressOpenAcknowledgementV2 {")
        .nth(1)
        .unwrap()
        .split("impl Debug for RuntimeAcceptedIngressOpenAcknowledgementV2")
        .next()
        .unwrap();
    assert!(accepted.contains("request: RuntimePublishIngressOpenAcknowledgementV2,"));
    assert!(accepted.contains("receipt: RuntimeIngressOpenAcknowledgementReceiptV2,"));
    assert!(
        accepted.contains("pub fn request(&self) -> &RuntimePublishIngressOpenAcknowledgementV2")
    );
    for required in [
        "pub trait RuntimeIngressOpenAcknowledgementPortV2",
        "fn publish_ingress_open_acknowledgement<'a>(",
        "fn observe_ingress_open_acknowledgement<'a>(",
        "fn observe_ingress_open_acknowledgement_predecessor(",
        "DefinitelyNotApplied",
        "OutcomeUnknown",
        "RuntimeIngressOpenAcknowledgementObservationErrorClassV2",
        "RuntimeIngressOpenAcknowledgementResolutionV2",
        "ReplaySameRequest",
        "ReplayBudgetExhausted",
        "RuntimeIngressOpenAcknowledgementSingleFlightV2",
        "RuntimeIngressOpenAcknowledgementAttemptV2",
        "RuntimeIngressOpenAcknowledgementPredecessorObservationAuthorizationV2",
        "Divergent",
        "ProtocolViolation",
        "RuntimeIngressOpenAcknowledgementMutationErrorV2::DefinitelyNotApplied(<redacted>)",
        "RuntimeAuthorizedIngressOpenAcknowledgementV2(<redacted>)",
    ] {
        assert!(source.contains(required), "{required}");
    }
    for forbidden in [
        "sqlx",
        "rusqlite",
        "twilight",
        "tokio",
        "Serialize",
        "Deserialize",
        "async fn",
        "Utc::now",
        "SystemTime",
    ] {
        assert!(!source.contains(forbidden), "{forbidden}");
    }
    assert!(admission.contains("pub fn authorize_ingress_open_acknowledgement("));
    assert!(admission
        .contains("pub fn authorize_ingress_open_acknowledgement_predecessor_observation("));
    assert!(
        refresh.contains("pub fn authorize_ingress_open_acknowledgement_refresh(\n        self,")
    );
    assert!(refresh.contains("RuntimeEmptyOpenAcknowledgementRefreshV2"));
    assert!(admission.contains(
        "RuntimeIngressOpenAcknowledgementV2 as RuntimeDurableIngressOpenAcknowledgementV2"
    ));
    let open_observation = admission
        .split("pub struct RuntimeIngressOpenAcknowledgementObservationV2 {")
        .nth(1)
        .unwrap()
        .split("impl Debug for RuntimeIngressOpenAcknowledgementObservationV2")
        .next()
        .unwrap();
    assert!(open_observation.contains("accepted: RuntimeAcceptedIngressOpenAcknowledgementV2,"));
    assert!(open_observation.contains(
        "pub fn from_accepted(accepted: RuntimeAcceptedIngressOpenAcknowledgementV2) -> Self"
    ));
    assert!(!open_observation.contains("RuntimeDurableIngressOpenAcknowledgementV2,"));
    assert!(!open_observation.contains("pub fn new("));
    assert!(!source.contains("pub fn into_acknowledgement("));
    assert!(!admission.contains("RuntimeIngressOpenAcknowledgementObservationInputV2"));
    assert!(!lifecycle.contains("RuntimeIngressOpenAcknowledgementObservationInputV2"));
    assert!(!root.contains("RuntimeIngressOpenAcknowledgementObservationInputV2"));
    for exported in [
        "RuntimeAuthorizedIngressOpenAcknowledgementV2",
        "RuntimeIngressOpenAcknowledgementPortV2",
        "RuntimeIngressOpenAcknowledgementMutationErrorV2",
        "RuntimeIngressOpenAcknowledgementResolutionV2",
        "RuntimeIngressOpenAcknowledgementSingleFlightV2",
        "RuntimeIngressOpenAcknowledgementPredecessorV2",
        "RuntimeEmptyOpenAcknowledgementRefreshV2",
    ] {
        assert!(contains_identifier(root, exported), "{exported}");
    }
}
