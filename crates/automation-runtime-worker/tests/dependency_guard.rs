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
            PathBuf::from("src/gateway_lifecycle.rs"),
            PathBuf::from("src/gateway_owner.rs"),
            PathBuf::from("src/gateway_owner_watchdog.rs"),
            PathBuf::from("src/lib.rs"),
            PathBuf::from("src/paused_gateway.rs"),
            PathBuf::from("src/registry_recovery.rs"),
            PathBuf::from("src/startup_recovery.rs"),
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
fn startup_fixed_point_is_narrow_noncloneable_and_not_serializable() {
    let source = include_str!("../src/startup_recovery.rs");
    let declaration = source
        .split("pub struct RuntimeStartupRecoveryObservationFixedPointV2 {")
        .next()
        .unwrap();
    let attributes = declaration.rsplit_once("\n\n").unwrap().1;

    for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
        assert!(!attributes.contains(forbidden));
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
    assert!(!source.contains("pub acknowledged_product_handoff_count"));
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
    assert!(lifecycle.contains("Shutdown {"));
    assert!(!lifecycle.contains("Open"));
    assert!(!lifecycle.contains("RecoveryPending"));
    assert!(!lifecycle.contains("AdmissionAcknowledging"));
    assert!(!invalidation.contains("Starting"));
    assert_eq!(invalidation.matches("    ").count(), 5);
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
