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
        "authoring-promotion",
        "automation-ruleset-activation",
        "automation-runtime",
        "bot-runtime",
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
fn source_files_contain_no_comments() {
    let sources = [
        ("src/attestation.rs", include_str!("../src/attestation.rs")),
        ("src/error.rs", include_str!("../src/error.rs")),
        ("src/id.rs", include_str!("../src/id.rs")),
        ("src/identity.rs", include_str!("../src/identity.rs")),
        ("src/lib.rs", include_str!("../src/lib.rs")),
        ("src/machine.rs", include_str!("../src/machine.rs")),
        (
            "src/machine/product_drain.rs",
            include_str!("../src/machine/product_drain.rs"),
        ),
        (
            "src/machine/validation.rs",
            include_str!("../src/machine/validation.rs"),
        ),
        ("src/state.rs", include_str!("../src/state.rs")),
        (
            "tests/dependency_guard.rs",
            include_str!("dependency_guard.rs"),
        ),
        (
            "tests/product_drain_source_supersession.rs",
            include_str!("product_drain_source_supersession.rs"),
        ),
        ("tests/state_machine.rs", include_str!("state_machine.rs")),
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

#[test]
fn product_drain_supersession_requires_an_adapter_validated_opaque_proof() {
    let source = include_str!("../src/machine/product_drain.rs");
    for required in [
        "pub struct ProductDrainSourceSupersessionPermitV1",
        "from_adapter_validated_durable_route_absence_acknowledgement",
        "pub fn supersede_product_drain_source",
    ] {
        assert!(source.contains(required), "missing boundary: {required}");
    }
    for forbidden in [
        "pub source:",
        "pub acknowledged_at:",
        "Serialize",
        "Deserialize",
        "impl Clone for ProductDrainSourceSupersessionPermitV1",
        "automation_runtime_controller",
        "RuntimePersistedRouteAbsent",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden proof surface: {forbidden}"
        );
    }
}
