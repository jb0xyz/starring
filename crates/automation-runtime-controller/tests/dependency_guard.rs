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
