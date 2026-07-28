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
            "src/registry/v4_drain.rs",
            include_str!("../src/registry/v4_drain.rs"),
        ),
        (
            "src/registry/v4_drain/worker_port.rs",
            include_str!("../src/registry/v4_drain/worker_port.rs"),
        ),
        (
            "src/registry/v4_drain/state.rs",
            include_str!("../src/registry/v4_drain/state.rs"),
        ),
        (
            "src/v2_observation.rs",
            include_str!("../src/v2_observation.rs"),
        ),
        ("src/v2_recovery.rs", include_str!("../src/v2_recovery.rs")),
        ("tests/registry.rs", include_str!("registry.rs")),
        ("tests/v4_drain.rs", include_str!("v4_drain.rs")),
    ];
    for (path, source) in sources {
        assert!(!source.contains("//"), "line comment in {path}");
        assert!(!source.contains("/*"), "block comment in {path}");
        assert!(!source.contains("*/"), "block comment terminator in {path}");
    }
}

#[test]
fn v4_drain_capabilities_are_linear_nonserializable_and_nonconstructible() {
    let source = concat!(
        include_str!("../src/registry/v4_drain.rs"),
        include_str!("../src/registry/v4_drain/state.rs"),
        include_str!("../src/registry/v4_drain/worker_port.rs")
    );
    let public = include_str!("../src/lib.rs");
    let states = [
        "RoutedObservedV4",
        "RoutedSealedV4",
        "RoutedSealedObservationV4",
        "RoutedClaimedSealedV4",
        "LocallyRefencedSealedV4",
        "DurablyRefencedSealedV4",
        "DrainingRefencedSealedV4",
        "DrainingRefencedObservationV4",
        "RouteAbsentSealedV4",
        "EmptySuccessionSealedV4",
        "AcknowledgedEmptyV4",
    ];
    for state in states {
        let declaration = format!("pub struct {state}");
        let prefix = source.split(&declaration).next().unwrap_or("").trim_end();
        assert!(!prefix.ends_with(")]"), "{state}");
        let implementation = source
            .split(&format!("impl {state} {{"))
            .nth(1)
            .unwrap_or("")
            .split("\n}")
            .next()
            .unwrap_or("");
        for constructor in [
            "pub fn new(",
            "pub const fn new(",
            "from_raw",
            "from_parts",
            "into_raw",
            "into_parts",
        ] {
            assert!(
                !implementation.contains(constructor),
                "{state}: {constructor}"
            );
        }
        for implemented in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
            assert!(
                !source.contains(&format!("impl {implemented} for {state}")),
                "{state}: {implemented}"
            );
        }
        assert!(public.contains(state), "{state}");
    }
    for forbidden in [
        "serde",
        "Serialize",
        "Deserialize",
        "async fn",
        ".await",
        "tokio",
        "twilight",
        "sqlx",
        "rusqlite",
        "from_raw_parts",
    ] {
        assert!(!source.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn v4_drain_transitions_consume_sources_and_use_dedicated_methods() {
    let source = concat!(
        include_str!("../src/registry/v4_drain.rs"),
        include_str!("../src/registry/v4_drain/worker_port.rs")
    );
    for signature in [
        "source: RoutedObservedV4,",
        "source: RoutedSealedV4,",
        "source: RoutedClaimedSealedV4,",
        "source: LocallyRefencedSealedV4,",
        "source: DurablyRefencedSealedV4,",
        "source: DrainingRefencedSealedV4,",
        "observation: DrainingRefencedObservationV4,",
        "source: RouteAbsentSealedV4,",
        "source: EmptySuccessionSealedV4,",
    ] {
        assert!(source.contains(signature), "{signature}");
    }
    for method in [
        "pub fn observe_routed_v4(",
        "pub fn observe_routed_sealed_v4(",
        "pub fn observe_draining_refenced_v4(",
        "pub fn seal_empty_succession_v4(",
        "pub fn checkpoint_locally_refenced_seal_v4(",
        "pub fn resume_locally_refenced_sealed_v4(",
        "pub fn resume_durably_refenced_sealed_v4(",
        "pub fn checkpoint_route_absent_seal_v4(",
        "pub fn resume_route_absent_sealed_v4(",
        "fn seal_routed(",
        "fn recover_routed_claimed(",
        "fn bind_claim(",
        "fn refence<J: Send, S: Send, C: Send>(",
        "fn bind_refence(",
        "fn recover_durable_refence(",
        "fn begin_drain(",
        "fn remove(",
        "fn recover_route_absent(",
        "fn seal_empty_succession(",
        "fn consume_acknowledgement(",
        "fn consume_succession_acknowledgement(",
        "fn rollback_routed_seal_v4(",
    ] {
        assert!(source.contains(method), "{method}");
    }
    for forbidden in [
        "pub fn seal_routed_v4(",
        "pub fn bind_routed_claim_v4(",
        "pub fn refence_routed_claim_v4(",
        "pub fn bind_durable_refence_v4(",
        "pub fn remove_draining_refenced_v4(",
        "pub fn consume_route_absent_acknowledgement_v4(",
        "pub fn consume_empty_succession_acknowledgement_v4(",
        "pub fn mutate_sealed",
        "pub fn remove_sealed",
        "pub fn unseal_v4",
        "pub fn escape",
        "pub fn resume_routed_sealed_v4",
    ] {
        assert!(!source.contains(forbidden), "{forbidden}");
    }
    assert!(source.contains("source: LocallyRefencedSealedV4,"));
    assert!(source.contains("source: RouteAbsentSealedV4,"));
    assert!(source.contains("claim_receipt_digest: RegistryDurableReceiptDigestV4,"));
    assert!(source.contains("refence_receipt_digest: RegistryDurableReceiptDigestV4,"));
    assert_eq!(source.matches("pub fn resume_").count(), 3);
    assert!(source.contains("type RoutedSealed = RoutedSealedV4;"));
    assert!(source.contains("RuntimeRoutedDrainRollbackPermitV4,"));
}

#[test]
fn ordinary_registry_mutations_remain_closed_under_a_seal() {
    let source = include_str!("../src/registry.rs");
    for method in [
        "pub fn install(",
        "pub fn activate_with_sequence_v2(",
        "pub fn begin_drain_with_authority(",
        "pub fn remove(",
        "pub fn remove_with_authority(",
        "pub fn advance_authority(",
    ] {
        let body = source
            .split(method)
            .nth(1)
            .unwrap_or("")
            .split("\n    pub fn ")
            .next()
            .unwrap_or("");
        assert!(body.contains("ensure_slot_unsealed(slot)?;"), "{method}");
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

#[test]
fn empty_recovery_cursor_is_instance_bound_noncloneable_and_nonauthorizing() {
    let registry = include_str!("../src/registry.rs");
    let error = include_str!("../src/error.rs");
    let public = include_str!("../src/lib.rs");
    let specification = include_str!(
        "../../../docs/superpowers/specs/2026-07-22-production-runtime-worker-composition-design.md"
    );
    let cursor = registry
        .split("pub struct RegistryEmptyRecoveryCursorV2")
        .nth(1)
        .unwrap_or("")
        .split("impl fmt::Debug for RegistryEmptyRecoveryCursorV2")
        .next()
        .unwrap_or("");
    assert!(cursor.contains("registry: Weak<RegistryInner>"));
    assert!(cursor.contains("expected_sequence: RegistryGlobalObservationSequenceV2"));
    assert!(!cursor.contains("pub registry"));
    assert!(!cursor.contains("pub expected_sequence"));

    let guard = registry
        .split("pub struct RegistryRecoveryObservationGuardV2")
        .nth(1)
        .unwrap_or("")
        .split("impl RegistryRecoveryObservationGuardV2")
        .next()
        .unwrap_or("");
    assert!(guard.contains("_state: MutexGuard<'a, RegistryState>"));
    assert!(guard.contains("registry: Weak<RegistryInner>"));
    assert!(!guard.contains("pub _state"));
    assert!(!guard.contains("pub registry"));

    for authority in [
        "RegistryEmptyRecoveryCursorV2",
        "RegistryRecoveryObservationGuardV2",
    ] {
        let declaration = format!("pub struct {authority}");
        let prefix = registry.split(&declaration).next().unwrap_or("").trim_end();
        assert!(!prefix.ends_with(")]"));
        for implemented in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
            assert!(!registry.contains(&format!("impl {implemented} for {authority}")));
        }
    }

    assert!(!registry.contains("impl RegistryEmptyRecoveryCursorV2"));
    assert!(registry.contains("RegistryEmptyRecoveryCursorV2(<redacted>)"));
    assert!(registry.contains("pub fn recovery_observation_guard_v2("));
    assert!(registry.contains("pub fn revalidate_empty_recovery_cursor_v2("));
    assert!(registry.contains("Weak::ptr_eq(&cursor.registry"));
    assert!(registry.contains("observation.observation_sequence() != cursor.expected_sequence"));
    for forbidden in [
        "cursor.seal",
        "cursor.refence",
        "cursor.remove",
        "cursor.install",
        "cursor.activate",
        "cursor.admit",
    ] {
        assert!(!registry.contains(forbidden), "{forbidden}");
    }
    for (source, forbidden) in [
        (registry, "RegistryRecoveryCursorV2"),
        (registry, "revalidate_recovery_cursor_v2"),
        (error, "StaleRegistryRecoveryCursor"),
        (public, "RegistryRecoveryCursorV2"),
    ] {
        assert!(!source.contains(forbidden), "{forbidden}");
    }
    assert!(specification.contains("exclusively the startup-empty fast path"));
    assert!(specification.contains("classified non-empty branch requires"));
    assert!(specification.contains("separate exact per-slot instance-bound witnesses"));
    assert!(specification.contains("empty cursor cannot replace those witnesses"));
    assert!(specification.contains("only when selecting the startup-empty branch"));
}
