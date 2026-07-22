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
        ("src/v2_evidence.rs", include_str!("../src/v2_evidence.rs")),
        ("src/v2_gateway.rs", include_str!("../src/v2_gateway.rs")),
        ("src/v2_identity.rs", include_str!("../src/v2_identity.rs")),
        ("src/v2_route.rs", include_str!("../src/v2_route.rs")),
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
