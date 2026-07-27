const MIGRATION: &str = include_str!(
    "../../../migrations/202607270006_normalize_startup_recovery_owner_projection_v2.sql"
);
const PREVIOUS_MIGRATION: &str =
    include_str!("../../../migrations/202607270005_add_atomic_startup_recovery_observation_v2.sql");
const CONTRACT_SOURCE: &str = include_str!("../src/contract.rs");
const DATABASE_SOURCE: &str = include_str!("../src/database.rs");
const SECURITY_SUPPORT_SOURCE: &str = include_str!("postgres_security/support.rs");

const FUNCTION_IDENTITY: &str =
    "public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)";
const PREVIOUS_OBSERVATION_DIGEST: &str =
    "9e0179b576eec5edf27cf4b9834c3f570073643518fda5a59fba5489c5fb46c6";
const CURRENT_OBSERVATION_DIGEST: &str =
    "1bafd85ec4d2291c6ab7cf213acaed35fe637409a1ed8679881ee8686956df09";
const PREVIOUS_MANIFEST_CONTENT_DIGEST: &str =
    "2b9c978bc17afb7440781c2d5ca50eed37c1ad89986e1f7fe28d2ab5c72fa9b5";
const CURRENT_MANIFEST_CONTENT_DIGEST: &str =
    "0144d12c7fd78a3f7ad75670e255a1cff2c0ba11cf613f10006cfcbc5528dcc9";
const PREVIOUS_MANIFEST_DEFINITION_DIGEST: &str =
    "94177e2025d87f492e988e3e27b8193b0f7157d4ea7fcd6099308534df9073ff";
const CURRENT_MANIFEST_DEFINITION_DIGEST: &str =
    "2e55bd05bb77a1dcc5a4f02efd0b221f2fa085fb92e7da7f97d29408022f0eb3";
const PREVIOUS_READINESS_DIGEST: &str =
    "ae397ea106f18aa71c6cf2427ebf2705638462066e480b6d0f10b9759a8adc5e";
const CURRENT_READINESS_DIGEST: &str =
    "9acd85e2162d4c06593dedae7d2043e53bebc8cd1d70c7aea5aa364cec0cb27f";
const LATEST_READINESS_DIGEST: &str =
    "a5191ef59e5365476860af1150a176049ef00c5b0d6c3f7cfe40e0b5be9d738a";

fn dollar_block(tag: &str) -> &'static str {
    MIGRATION
        .split(&format!("DO ${tag}$"))
        .nth(1)
        .unwrap()
        .split(&format!("${tag}$;"))
        .next()
        .unwrap()
}

#[test]
fn committed_observation_migration_is_superseded_without_rewrite() {
    assert!(PREVIOUS_MIGRATION.contains("IF owner_found THEN"));
    assert!(!PREVIOUS_MIGRATION.contains("AND owner_row.process_instance_id IS NOT NULL\n    THEN"));
    assert!(MIGRATION.contains("DO $patch_observation$"));
    assert!(
        !MIGRATION.contains("CREATE FUNCTION public.starring_runtime_startup_recovery_observe_v2(")
    );
    assert!(!MIGRATION.contains("DROP FUNCTION"));
}

#[test]
fn owner_projection_is_complete_for_active_and_empty_for_tombstone() {
    let patch = dollar_block("patch_observation");
    for required in [
        "IF owner_found THEN",
        "IF owner_found' || E'\\n' ||\n        '        AND owner_row.process_instance_id IS NOT NULL",
        "observed_process_instance_id := owner_row.process_instance_id;",
        "observed_lease_epoch := owner_row.lease_epoch;",
        "observed_runtime_build_revision :=",
        "observed_owner_revision := owner_row.owner_revision;",
        "observed_owner_expires_at := owner_row.expires_at;",
        "runtime_startup_recovery_owner_projection_function_patch_drift",
    ] {
        assert!(patch.contains(required), "{required}");
    }
    assert!(MIGRATION.contains("runtime_gateway_owners_state_check"));
    assert!(MIGRATION.contains(
        "process_instance_id IS NULL AND expected_build_revision IS NULL AND owner_revision IS NULL AND expires_at IS NULL"
    ));
    assert!(MIGRATION.contains(
        "process_instance_id IS NOT NULL AND expected_build_revision IS NOT NULL AND owner_revision IS NOT NULL AND expires_at IS NOT NULL"
    ));
}

#[test]
fn migration_is_serialized_fail_closed_and_comment_free() {
    let preflight = dollar_block("preflight");
    let postflight = dollar_block("postflight");
    assert!(MIGRATION.contains("starring-runtime-writer-fence-v1"));
    assert!(
        MIGRATION.contains("LOCK TABLE public.runtime_gateway_owners IN ACCESS EXCLUSIVE MODE;")
    );
    for required in [
        FUNCTION_IDENTITY,
        PREVIOUS_OBSERVATION_DIGEST,
        CURRENT_OBSERVATION_DIGEST,
        PREVIOUS_MANIFEST_CONTENT_DIGEST,
        CURRENT_MANIFEST_CONTENT_DIGEST,
        PREVIOUS_MANIFEST_DEFINITION_DIGEST,
        CURRENT_MANIFEST_DEFINITION_DIGEST,
        PREVIOUS_READINESS_DIGEST,
        CURRENT_READINESS_DIGEST,
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert!(preflight.contains("runtime_startup_recovery_owner_projection_preflight_drift"));
    assert!(postflight.contains("invalid_acl_count <> 0"));
    assert!(postflight.contains("pg_catalog.has_function_privilege"));
    assert!(postflight.contains("runtime_startup_recovery_owner_projection_postflight_drift"));
    assert!(postflight.contains("public.starring_runtime_execution_schema_manifest_v1()"));
    for line in MIGRATION.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"));
        assert!(!trimmed.starts_with("//"));
        assert!(!trimmed.starts_with("/*"));
    }
    assert!(MIGRATION.ends_with("RESET search_path;\n"));
}

#[test]
fn manifest_readiness_and_rust_pins_advance_together() {
    let manifest = dollar_block("patch_schema_manifest");
    let readiness = dollar_block("patch_readiness");
    assert!(manifest.contains(PREVIOUS_MANIFEST_CONTENT_DIGEST));
    assert!(manifest.contains(CURRENT_MANIFEST_CONTENT_DIGEST));
    assert!(readiness.contains(PREVIOUS_MANIFEST_DEFINITION_DIGEST));
    assert!(readiness.contains(CURRENT_MANIFEST_DEFINITION_DIGEST));
    for source in [CONTRACT_SOURCE, DATABASE_SOURCE, SECURITY_SUPPORT_SOURCE] {
        assert!(source.contains(LATEST_READINESS_DIGEST));
    }
    assert!(!CONTRACT_SOURCE.contains(PREVIOUS_READINESS_DIGEST));
    assert!(!CONTRACT_SOURCE.contains(CURRENT_READINESS_DIGEST));
}
