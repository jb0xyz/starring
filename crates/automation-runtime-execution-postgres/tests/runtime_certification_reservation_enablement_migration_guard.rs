const MIGRATION: &str = include_str!(
    "../../../migrations/202607270003_enable_runtime_certification_reservation_v2.sql"
);
const CONTRACT_SOURCE: &str = include_str!("../src/contract.rs");
const DATABASE_SOURCE: &str = include_str!("../src/database.rs");
const SECURITY_SUPPORT_SOURCE: &str = include_str!("postgres_security/support.rs");
const CURRENT_PIN_GUARDS: [&str; 6] = [
    include_str!("runtime_slot_writer_fence_migration_guard.rs"),
    include_str!("product_drain_first_apply_eligibility_migration_guard.rs"),
    include_str!("runtime_execution_slot_writer_epoch_migration_guard.rs"),
    include_str!("runtime_execution_selector_slot_writer_epoch_migration_guard.rs"),
    include_str!("runtime_certification_reservation_migration_guard.rs"),
    include_str!("runtime_legacy_certification_reservation_fence_migration_guard.rs"),
];
const RESERVE_IDENTITY: &str = "public.starring_runtime_certification_reserve_intent_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint,bytea,text)";
const OBSERVE_IDENTITY: &str =
    "public.starring_runtime_certification_reservation_observe_v2(text,text,text,bigint,bigint)";
const PREVIOUS_READINESS_DIGEST: &str =
    "a57602a79ee2aa5ac884dffb56d152bb5721d111e07eac5a5f853952d6db214f";
const CURRENT_READINESS_DIGEST: &str =
    "c5972296ea84090bae5708fc9efa90cd9f9f848acb156e40680c0ba04fb57b5c";
const LATEST_READINESS_DIGEST: &str =
    "de739460f2c86c2016cbc91aa47a625fbced903cc93722de80a33c93c7b54932";

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
fn migration_is_atomic_quiesced_and_object_preserving() {
    let barrier = MIGRATION.find("pg_advisory_xact_lock(").unwrap();
    let table_lock = MIGRATION.find("LOCK TABLE").unwrap();
    let snapshot = MIGRATION
        .find("starring_runtime_certification_enable_snapshot")
        .unwrap();
    let preflight = MIGRATION.find("DO $preflight$").unwrap();
    let acl = MIGRATION.find("DO $execution_acl$").unwrap();
    let readiness = MIGRATION.find("DO $patch_readiness$").unwrap();
    let postflight = MIGRATION.find("DO $postflight$").unwrap();
    assert!(barrier < table_lock);
    assert!(table_lock < snapshot);
    assert!(snapshot < preflight);
    assert!(preflight < acl);
    assert!(acl < readiness);
    assert!(readiness < postflight);

    for required in [
        "SET LOCAL lock_timeout = '5s';",
        "SET LOCAL statement_timeout = '30s';",
        "IN ACCESS EXCLUSIVE MODE;",
        "function_oid OID PRIMARY KEY",
        "function_owner OID NOT NULL",
        "function_acl ACLITEM[]",
        "WHERE function_row.oid >= 16384",
        "NOT role.rolcanlogin",
        "FROM pg_catalog.pg_auth_members AS membership",
        "membership.roleid = executor_role",
        "OR membership.member = executor_role",
        "FROM pg_catalog.pg_stat_activity AS activity",
        "activity.backend_type = 'client backend'",
        "FROM pg_catalog.pg_prepared_xacts AS prepared",
        "runtime_certification_enable_executor_not_quiesced",
        "snapshot_mismatch_count <> 0",
        "new_function_count <> 0",
        "RESET statement_timeout;",
        "RESET lock_timeout;",
        "RESET search_path;",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    for forbidden in [
        "--",
        "/*",
        "//",
        "CREATE FUNCTION ",
        "CREATE OR REPLACE FUNCTION ",
        "CREATE TABLE ",
        "ALTER FUNCTION ",
        "ALTER DEFAULT PRIVILEGES",
    ] {
        assert!(!MIGRATION.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn enablement_grants_exactly_two_existing_functions_to_the_executor() {
    let acl = dollar_block("execution_acl");
    assert_eq!(acl.matches("GRANT EXECUTE ON FUNCTION").count(), 2);
    for required in [
        "public.starring_runtime_certification_reserve_intent_v2(BIGINT,TEXT,TEXT,TEXT,TEXT,BIGINT,TEXT,BIGINT,BIGINT,BIGINT,TEXT,TEXT,BIGINT,TEXT,BIGINT,TEXT,BIGINT,TEXT,TEXT,BIGINT,BIGINT,TEXT,TEXT,TEXT,BIGINT,BYTEA,TEXT)",
        "public.starring_runtime_certification_reservation_observe_v2(TEXT,TEXT,TEXT,BIGINT,BIGINT)",
        "pg_catalog.pg_get_userbyid(executor_role)",
        "TO %I",
    ] {
        assert!(acl.contains(required), "{required}");
    }
    for forbidden in [
        "PUBLIC",
        "runtime_certification_operations_v2",
        "starring_runtime_private_v2",
        "reject_runtime_certification_reservation_mutation_v2",
    ] {
        assert!(!acl.contains(forbidden), "{forbidden}");
    }
    assert_eq!(MIGRATION.matches("GRANT EXECUTE ON FUNCTION").count(), 2);
    assert!(!MIGRATION.contains("GRANT SELECT"));
    assert!(!MIGRATION.contains("GRANT INSERT"));
    assert!(!MIGRATION.contains("GRANT UPDATE"));
    assert!(!MIGRATION.contains("GRANT DELETE"));

    let postflight = dollar_block("postflight");
    for required in [
        "invalid_target_acl_count",
        "CASE WHEN executor_role IS NULL THEN 1 ELSE 2 END",
        "privilege.grantee NOT IN (common_owner, executor_role)",
        "pg_catalog.has_function_privilege",
        "invalid_capability_acl_count",
        "observed.external_acl IS DISTINCT FROM baseline.external_acl",
        RESERVE_IDENTITY,
        OBSERVE_IDENTITY,
    ] {
        assert!(postflight.contains(required), "{required}");
    }
}

#[test]
fn raw_table_trigger_and_private_builder_surfaces_remain_owner_only() {
    let preflight = dollar_block("preflight");
    let postflight = dollar_block("postflight");
    for required in [
        "public.reject_runtime_certification_reservation_mutation_v2()",
        "starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2",
        "starring_runtime_private_v2.starring_runtime_certification_intent_fingerprint_v2",
        "public.runtime_certification_operations_v2",
        "runtime_certification_operations_v2_reject_row_mutation",
        "runtime_certification_operations_v2_reject_truncate",
    ] {
        assert!(preflight.contains(required), "{required}");
        assert!(postflight.contains(required), "{required}");
    }
    for required in [
        "invalid_private_acl_count",
        "privilege.grantee <> common_owner",
        "invalid_relation_acl_count",
        "pg_catalog.has_table_privilege",
        "'SELECT'",
        "'INSERT'",
        "'UPDATE'",
        "'DELETE'",
        "'TRUNCATE'",
        "'REFERENCES'",
        "'TRIGGER'",
        "pg_catalog.has_schema_privilege",
        "'starring_runtime_private_v2'",
        "'USAGE'",
    ] {
        assert!(postflight.contains(required), "{required}");
    }
}

#[test]
fn readiness_moves_the_two_functions_from_protected_to_capability_allowlists() {
    let readiness = dollar_block("patch_readiness");
    for required in [
        RESERVE_IDENTITY,
        OBSERVE_IDENTITY,
        "runtime_certification_enable_readiness_function_patch_drift",
        "runtime_certification_enable_readiness_protected_patch_drift",
        "runtime_certification_enable_readiness_allowlist_patch_drift",
        "TABLE(outcome_name text, locked_snapshot jsonb, locked_convergence_attempt_no bigint",
    ] {
        assert!(readiness.contains(required), "{required}");
    }
    let postflight = dollar_block("postflight");
    for required in [
        "ff16060ff3ddcb6d71dee07138e411674dd446a792de6cd2e22b400378cf2df4",
        CURRENT_READINESS_DIGEST,
        "runtime_certification_enable_postflight_drift",
    ] {
        assert!(postflight.contains(required), "{required}");
    }
    assert!(dollar_block("preflight").contains(PREVIOUS_READINESS_DIGEST));
    assert!(!MIGRATION.contains("0000000000000000000000000000000000000000000000000000000000000000"));
}

#[test]
fn rust_contract_and_security_fixture_expose_the_latest_capabilities() {
    let operations = CONTRACT_SOURCE
        .split("OPERATION_CAPABILITY_IDENTITIES_V1")
        .nth(1)
        .unwrap()
        .split("];")
        .next()
        .unwrap();
    assert!(CONTRACT_SOURCE.contains("OPERATION_CAPABILITY_IDENTITIES_V1: [&str; 22]"));
    assert!(CONTRACT_SOURCE.contains("capabilities.clone().count() != 24"));
    for identity in [RESERVE_IDENTITY, OBSERVE_IDENTITY] {
        assert_eq!(operations.matches(identity).count(), 1, "{identity}");
    }
    for forbidden in [
        "runtime_certification_operations_v2",
        "reject_runtime_certification_reservation_mutation_v2",
        "starring_runtime_private_v2",
    ] {
        assert!(!operations.contains(forbidden), "{forbidden}");
    }

    let executor_functions = SECURITY_SUPPORT_SOURCE
        .split("const EXECUTOR_FUNCTIONS")
        .nth(1)
        .unwrap()
        .split("];")
        .next()
        .unwrap();
    assert!(SECURITY_SUPPORT_SOURCE.contains("const EXECUTOR_FUNCTIONS: [&str; 24]"));
    for identity in [RESERVE_IDENTITY, OBSERVE_IDENTITY] {
        assert_eq!(
            executor_functions.matches(identity).count(),
            1,
            "{identity}"
        );
    }
}

#[test]
fn current_readiness_pin_is_propagated_without_rewriting_history() {
    for source in [CONTRACT_SOURCE, DATABASE_SOURCE, SECURITY_SUPPORT_SOURCE] {
        assert!(source.contains(LATEST_READINESS_DIGEST));
        assert!(!source.contains(PREVIOUS_READINESS_DIGEST));
    }
    for guard in CURRENT_PIN_GUARDS {
        assert!(guard.contains(LATEST_READINESS_DIGEST));
    }
    assert!(MIGRATION.contains(PREVIOUS_READINESS_DIGEST));
    assert!(MIGRATION.contains(CURRENT_READINESS_DIGEST));
}
