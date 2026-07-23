#[test]
fn regular_dependencies_stay_transport_and_storage_neutral() {
    let manifest = include_str!("../Cargo.toml");
    let regular = manifest
        .split("[dependencies]")
        .nth(1)
        .unwrap_or("")
        .split("[dev-dependencies]")
        .next()
        .unwrap_or("");
    for forbidden in [
        "tokio",
        "twilight",
        "sqlx",
        "rusqlite",
        "sqlite",
        "reqwest",
        "ai-gateway",
        "ai_gateway",
        "ollama",
        "llm",
        "design-harness",
        "serde",
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
        ("src/error.rs", include_str!("../src/error.rs")),
        ("src/identity.rs", include_str!("../src/identity.rs")),
        ("src/lib.rs", include_str!("../src/lib.rs")),
        ("src/registry.rs", include_str!("../src/registry.rs")),
        (
            "src/v2_observation.rs",
            include_str!("../src/v2_observation.rs"),
        ),
        ("src/v2_recovery.rs", include_str!("../src/v2_recovery.rs")),
        ("tests/registry.rs", include_str!("registry.rs")),
    ];
    for (path, source) in sources {
        assert!(!source.contains("//"), "line comment in {path}");
        assert!(!source.contains("/*"), "block comment in {path}");
        assert!(!source.contains("*/"), "block comment terminator in {path}");
    }
}

#[test]
fn v2_admission_does_not_expose_v1_mutation_authority() {
    let source = include_str!("../src/registry.rs");
    let admitted = source
        .split("pub struct AdmittedInteractionV2")
        .nth(1)
        .unwrap_or("")
        .split("pub struct SlotDrainClaimSealV2")
        .next()
        .unwrap_or("");
    assert!(!admitted.contains("AdmittedInteractionV1"));
    assert!(!admitted.contains("SlotMutationTokenV1"));
    assert!(!admitted.contains("ServingSlotSnapshotV1"));
    assert!(!admitted.contains("into_admitted"));
    for authority in ["AdmittedInteractionV2", "SlotDrainClaimSealV2"] {
        let declaration = format!("pub struct {authority}");
        let prefix = source.split(&declaration).next().unwrap_or("").trim_end();
        assert!(!prefix.ends_with(")]"));
        for implemented in ["Clone", "Default", "Serialize", "Deserialize"] {
            assert!(!source.contains(&format!("impl {implemented} for {authority}")));
        }
    }
}

#[test]
fn recovery_observation_is_non_authorizing_typed_and_nonserializable() {
    let recovery = include_str!("../src/v2_recovery.rs");
    let registry = include_str!("../src/registry.rs");
    let observation = recovery
        .split("pub struct RegistryRecoveryObservationV2")
        .nth(1)
        .unwrap_or("")
        .split("impl RegistryRecoveryObservationV2")
        .next()
        .unwrap_or("");
    for forbidden in [
        "pub observation_sequence",
        "pub staged_route_count",
        "pub serving_route_count",
        "pub draining_route_count",
        "pub sealed_slot_count",
        "pub active_interaction_count",
        "Weak<RegistryInner>",
        "SlotMutationTokenV1",
        "SlotDrainClaimSealV2",
    ] {
        assert!(!observation.contains(forbidden), "{forbidden}");
    }
    for forbidden in [
        "Serialize",
        "Deserialize",
        "Default",
        "serde",
        "SlotMutationTokenV1",
        "SlotDrainClaimSealV2",
        "Weak<RegistryInner>",
        "pub const fn new(",
    ] {
        assert!(!recovery.contains(forbidden), "{forbidden}");
    }
    assert!(recovery.contains("RegistryRecoveryObservationV2(<redacted>)"));
    assert!(recovery.contains("pub struct RegistryGlobalObservationSequenceV2"));
    assert!(recovery.contains("pub(crate) const fn new(value: NonZeroU64)"));
    assert!(registry.contains("pub fn recovery_observation_v2("));
    assert!(registry.contains("registry_recovery_observation_v2(&state)"));
    assert!(registry.contains("advance_observation_or_close(observation, slot)"));
}
