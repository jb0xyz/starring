const MIGRATION: &str =
    include_str!("../../../migrations/202607240008_add_product_runtime_canonical_builders.sql");

const PRIVATE_SCHEMA: &str = "starring_runtime_private_v2";

const HELPERS: [&str; 6] = [
    "starring_runtime_json_string_bytes_v2",
    "starring_runtime_framed_digest_v2",
    "starring_runtime_product_mutation_bytes_v2",
    "starring_runtime_product_mutation_digest_v2",
    "starring_runtime_drain_intent_bytes_v2",
    "starring_runtime_drain_intent_digest_v2",
];

#[test]
fn canonical_builder_migration_is_atomic_private_and_comment_free() {
    assert!(!MIGRATION.contains("--"));
    assert!(!MIGRATION.contains("/*"));
    assert!(!MIGRATION.contains("//"));
    assert!(!MIGRATION.contains("__MANIFEST"));
    assert!(!MIGRATION.contains("__READINESS"));
    assert!(MIGRATION.contains("SET LOCAL search_path = pg_catalog;"));
    assert!(MIGRATION.contains("CREATE SCHEMA starring_runtime_private_v2"));
    assert!(MIGRATION
        .contains("REVOKE ALL PRIVILEGES ON SCHEMA starring_runtime_private_v2 FROM PUBLIC;"));
    assert!(MIGRATION.contains("DO $preflight$"));
    assert!(MIGRATION.contains("DO $patch_manifest$"));
    assert!(MIGRATION.contains("DO $patch_readiness$"));
    assert!(MIGRATION.contains("DO $postflight$"));
    assert!(MIGRATION.contains("runtime_canonical_builders_preflight_drift"));
    assert!(MIGRATION.contains("runtime_canonical_builders_postflight_drift"));
}

#[test]
fn canonical_builders_are_fixed_owner_only_pure_helpers() {
    for helper in HELPERS {
        assert!(
            MIGRATION.contains(&format!("{PRIVATE_SCHEMA}.{helper}")),
            "{helper}"
        );
    }
    assert_eq!(MIGRATION.matches("CREATE FUNCTION").count(), HELPERS.len());
    assert_eq!(MIGRATION.matches("IMMUTABLE").count(), HELPERS.len());
    assert_eq!(MIGRATION.matches("STRICT").count(), HELPERS.len());
    assert_eq!(MIGRATION.matches("PARALLEL SAFE").count(), HELPERS.len());
    assert_eq!(MIGRATION.matches("SECURITY INVOKER").count(), HELPERS.len());
    assert_eq!(
        MIGRATION.matches("SET search_path = pg_catalog").count(),
        HELPERS.len()
    );
    assert!(MIGRATION.contains("REVOKE ALL PRIVILEGES ON FUNCTION"));
    assert!(!MIGRATION.contains("SECURITY DEFINER\nSET search_path = pg_catalog"));
    assert!(!MIGRATION.contains("CREATE TABLE public."));
    assert!(!MIGRATION.contains("ALTER TABLE"));
    assert!(!MIGRATION.contains("CREATE TRIGGER"));
}

#[test]
fn byte_builders_use_one_escape_primitive_and_no_database_json_renderer() {
    assert_eq!(
        MIGRATION
            .matches(
                "CREATE FUNCTION starring_runtime_private_v2.starring_runtime_json_string_bytes_v2"
            )
            .count(),
        1
    );
    assert!(MIGRATION.contains("pg_catalog.decode('5c22', 'hex')"));
    assert!(MIGRATION.contains("pg_catalog.decode('5c5c', 'hex')"));
    assert!(MIGRATION.contains("E'\\\\u00'"));
    for forbidden in [
        "to_json(",
        "to_jsonb(",
        "json_build",
        "jsonb_build",
        "::JSON",
        "::JSONB",
        "starring_canonical_json_v1",
    ] {
        assert!(!MIGRATION.contains(forbidden), "{forbidden}");
    }
    assert!(MIGRATION
        .contains("starring_runtime_private_v2.starring_runtime_product_mutation_bytes_v2("));
    assert!(MIGRATION
        .contains("starring_runtime_private_v2.starring_runtime_product_mutation_digest_v2("));
    assert!(MIGRATION.contains("product_digest :="));
}

#[test]
fn canonical_builders_pin_ranges_tags_slot_and_domains() {
    for tag in [
        "'apply'",
        "'supersede'",
        "'cancel'",
        "'authority_change'",
        "'teardown'",
    ] {
        assert!(MIGRATION.contains(tag), "{tag}");
    }
    for contract in [
        "requested_expected_revision NOT BETWEEN 1 AND 9223372036854775807",
        "requested_target_version NOT BETWEEN 1 AND 4294967295",
        "requested_target_binding_revision",
        "requested_slot_guild_id <> requested_target_guild_id",
        "requested_slot_ruleset_key <> requested_target_ruleset_key",
        "requested_product_semantic_request_digest !~ '^[0-9a-f]{64}$'",
        "pg_catalog.octet_length(canonical_payload) NOT BETWEEN 1 AND 32768",
        "pg_catalog.octet_length(canonical_payload) NOT BETWEEN 1 AND 65536",
        "starring.runtime.product_mutation.v2",
        "starring.runtime.drain_intent.v2",
        "pg_catalog.int8send(",
        "pg_catalog.decode('00', 'hex')",
    ] {
        assert!(MIGRATION.contains(contract), "{contract}");
    }
}

#[test]
fn manifest_readiness_and_acl_contracts_are_extended_without_grants() {
    for digest in [
        "58520c287c446b96198d624c9e10c4dc7e1ba8371cc721f6192a903685197476",
        "c819437ec90f4f64ebd8a3722979e2ea817e87bdc370eef1e5c196e163551188",
        "944f87185d6fd290c3b9a2fe2de08ec097c833802292a2ed34c80c811c5ee062",
        "27ebe976c214377f71f62cf7d9c90be3009e3c331e395dff7d63c587513be167",
        "c32a430e629c5603de09a15769b664bd533f3d4a86d5b26f514657ad63fc5eec",
    ] {
        assert!(MIGRATION.contains(digest), "{digest}");
    }
    assert!(MIGRATION.contains("protected_schema(namespace_oid)"));
    assert!(MIGRATION.contains("invalid_private_helper_acl_count"));
    assert!(MIGRATION.contains("invalid_private_schema_count"));
    assert!(MIGRATION.contains("invalid_capability_acl_count"));
    assert!(MIGRATION.contains("invalid_manifest_acl_count"));
    assert!(MIGRATION.contains("public_snapshot_mismatch_count"));
    assert!(MIGRATION.contains("external_executor_count > 1"));
    assert!(MIGRATION.contains("privilege.grantee <> common_owner"));
    assert!(MIGRATION.contains("privilege.privilege_type <> 'EXECUTE'"));
    assert!(!MIGRATION.contains("GRANT EXECUTE"));
}
