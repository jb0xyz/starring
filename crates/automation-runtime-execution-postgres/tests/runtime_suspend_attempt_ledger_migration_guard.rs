const MIGRATION: &str = include_str!(
    "../../../migrations/202607270004_establish_runtime_suspend_attempt_ledger_v2.sql"
);
const CONTRACT_SOURCE: &str = include_str!("../src/contract.rs");
const DATABASE_SOURCE: &str = include_str!("../src/database.rs");
const SECURITY_SUPPORT_SOURCE: &str = include_str!("postgres_security/support.rs");

const PREVIOUS_MANIFEST_DIGEST: &str =
    "ff16060ff3ddcb6d71dee07138e411674dd446a792de6cd2e22b400378cf2df4";
const CURRENT_MANIFEST_DIGEST: &str =
    "57694b2a5f374fa63882fb52f5bfe506b321968c961ea2cf9de8006fd46a5979";
const PREVIOUS_READINESS_DIGEST: &str =
    "c5972296ea84090bae5708fc9efa90cd9f9f848acb156e40680c0ba04fb57b5c";
const CURRENT_READINESS_DIGEST: &str =
    "6523d219df9a148c9428ac8f45b9317bcad6b56af44b753f11167fc582ca5875";
const LATEST_READINESS_DIGEST: &str =
    "779d97c088a29027589ebdffa9753eb1333a1d9b511cd714211cde6ae8146c4e";

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
fn migration_is_atomic_quiescent_zero_cutover_and_comment_free() {
    let global = MIGRATION.find("pg_advisory_xact_lock(").unwrap();
    let tables = MIGRATION.find("LOCK TABLE").unwrap();
    let preflight = MIGRATION.find("DO $preflight$").unwrap();
    let roots = MIGRATION
        .find("CREATE TABLE public.runtime_suspend_attempt_operations_v2")
        .unwrap();
    let manifest = MIGRATION.find("DO $patch_schema_manifest$").unwrap();
    let readiness = MIGRATION.find("DO $patch_readiness$").unwrap();
    let postflight = MIGRATION.find("DO $postflight$").unwrap();
    assert!(global < tables);
    assert!(tables < preflight);
    assert!(preflight < roots);
    assert!(roots < manifest);
    assert!(manifest < readiness);
    assert!(readiness < postflight);
    for required in [
        "SET LOCAL lock_timeout = '5s';",
        "SET LOCAL statement_timeout = '30s';",
        "IN ACCESS EXCLUSIVE MODE;",
        "NOT role.rolcanlogin",
        "FROM pg_catalog.pg_auth_members AS membership",
        "FROM pg_catalog.pg_stat_activity AS activity",
        "activity.backend_type = 'client backend'",
        "FROM pg_catalog.pg_prepared_xacts AS prepared",
        "owner.expires_at > pg_catalog.clock_timestamp()",
        "serving.expires_at > pg_catalog.clock_timestamp()",
        PREVIOUS_MANIFEST_DIGEST,
        PREVIOUS_READINESS_DIGEST,
        "RESET statement_timeout;",
        "RESET lock_timeout;",
        "RESET search_path;",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert!(!MIGRATION.contains("--"));
    assert!(!MIGRATION.contains("/*"));
    assert!(!MIGRATION.contains("//"));
}

#[test]
fn ledger_separates_immutable_roots_active_state_and_terminal_evidence() {
    for required in [
        "CREATE TABLE public.runtime_suspend_attempt_operations_v2",
        "suspend_attempt_request_bytes BYTEA NOT NULL",
        "suspend_attempt_digest TEXT NOT NULL",
        "runtime_suspend_attempt_operations_v2_natural_unique UNIQUE",
        "runtime_suspend_attempt_operations_v2_child_unique UNIQUE",
        "CREATE TABLE public.runtime_suspended_attempts_v2",
        "sidecar_revision BIGINT NOT NULL",
        "local_effect_kind TEXT NOT NULL",
        "local_effect_bytes BYTEA NOT NULL",
        "drain_obligation_kind TEXT NOT NULL",
        "drain_obligation_bytes BYTEA NOT NULL",
        "runtime_suspended_attempts_v2_slot_unique UNIQUE",
        "CREATE TABLE public.runtime_suspend_attempt_completions_v2",
        "resulting_deployment_revision = deployment_revision + 1",
        "resulting_convergence_attempt_no = convergence_attempt_no + 1",
        "successor_controller_fencing_token BIGINT NOT NULL",
        "successor_expires_at > successor_acquired_at",
        "ON DELETE RESTRICT",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert_eq!(
        MIGRATION
            .matches("CREATE TABLE public.runtime_suspend")
            .count(),
        3
    );
    assert_eq!(
        MIGRATION
            .matches("CREATE TABLE public.runtime_suspended_attempts_v2")
            .count(),
        1
    );
}

#[test]
fn every_table_is_fail_closed_and_cross_table_consistency_is_deferred() {
    for required in [
        "CREATE FUNCTION public.reject_runtime_suspend_attempt_ledger_mutation_v2()",
        "MESSAGE = 'runtime_suspend_attempt_ledger_mutation_rejected'",
        "CREATE FUNCTION public.validate_runtime_suspend_attempt_ledger_v2()",
        "active_count + completion_count <> 1",
        "MESSAGE = 'runtime_suspend_attempt_ledger_consistency_invalid'",
        "DEFERRABLE INITIALLY DEFERRED",
        "BEFORE INSERT OR UPDATE OR DELETE",
        "BEFORE TRUNCATE",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert_eq!(
        MIGRATION
            .matches("BEFORE INSERT OR UPDATE OR DELETE")
            .count(),
        3
    );
    assert_eq!(MIGRATION.matches("BEFORE TRUNCATE").count(), 3);
    assert_eq!(
        MIGRATION
            .matches("CREATE CONSTRAINT TRIGGER runtime_suspend")
            .count(),
        3
    );
    assert!(!MIGRATION.contains("GRANT "));
}

#[test]
fn relations_and_trigger_functions_are_owner_only_with_no_executor_surface() {
    let postflight = dollar_block("postflight");
    for identity in [
        "public.runtime_suspend_attempt_operations_v2",
        "public.runtime_suspended_attempts_v2",
        "public.runtime_suspend_attempt_completions_v2",
        "public.reject_runtime_suspend_attempt_ledger_mutation_v2()",
        "public.validate_runtime_suspend_attempt_ledger_v2()",
    ] {
        assert!(MIGRATION.contains(identity), "{identity}");
        assert!(postflight.contains(identity), "{identity}");
    }
    for required in [
        "privilege.grantee <> common_owner",
        "pg_catalog.has_table_privilege",
        "pg_catalog.has_function_privilege",
        "'SELECT'",
        "'INSERT'",
        "'UPDATE'",
        "'DELETE'",
        "'TRUNCATE'",
        "'REFERENCES'",
        "'TRIGGER'",
    ] {
        assert!(postflight.contains(required), "{required}");
    }
}

#[test]
fn manifest_readiness_and_runtime_pins_advance_exactly_once() {
    let manifest = dollar_block("patch_schema_manifest");
    let readiness = dollar_block("patch_readiness");
    for required in [
        "RETURN observed_count = 733",
        "984539f97c292c40c30b262087e312cd423d06c149fb30a4cba6af9596574120",
        CURRENT_MANIFEST_DIGEST,
        CURRENT_READINESS_DIGEST,
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert!(manifest.contains("runtime_suspend_attempt_ledger_manifest_relation_patch_drift"));
    assert!(manifest.contains("runtime_suspend_attempt_ledger_manifest_expectation_patch_drift"));
    assert!(readiness.contains("runtime_suspend_attempt_ledger_readiness_relation_patch_drift"));
    assert!(readiness.contains("runtime_suspend_attempt_ledger_readiness_function_patch_drift"));
    assert!(
        readiness.contains("runtime_suspend_attempt_ledger_readiness_manifest_digest_patch_drift")
    );
    for source in [CONTRACT_SOURCE, DATABASE_SOURCE, SECURITY_SUPPORT_SOURCE] {
        assert!(source.contains(LATEST_READINESS_DIGEST));
    }
}
