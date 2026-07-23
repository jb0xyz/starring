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
            PathBuf::from("src/closed_recovery.rs"),
            PathBuf::from("src/gateway_lifecycle.rs"),
            PathBuf::from("src/gateway_lifecycle_tests.rs"),
            PathBuf::from("src/gateway_owner.rs"),
            PathBuf::from("src/gateway_owner_watchdog.rs"),
            PathBuf::from("src/lib.rs"),
            PathBuf::from("src/paused_gateway.rs"),
            PathBuf::from("src/product_drain.rs"),
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
        2
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
