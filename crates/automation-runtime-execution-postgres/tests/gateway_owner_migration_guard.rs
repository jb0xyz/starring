const MIGRATION: &str =
    include_str!("../../../migrations/202607230001_persist_runtime_gateway_owner.sql");

const GATEWAY_OWNER_FUNCTIONS: [&str; 4] = [
    "public.starring_runtime_gateway_owner_observe_v1(TEXT)",
    "public.starring_runtime_gateway_owner_acquire_v1(TEXT,TEXT,TEXT,BIGINT)",
    "public.starring_runtime_gateway_owner_renew_v1(TEXT,TEXT,BIGINT,TEXT,BIGINT,BIGINT)",
    "public.starring_runtime_gateway_owner_release_v1(TEXT,TEXT,BIGINT,TEXT)",
];

#[test]
fn gateway_owner_migration_is_comment_free_and_exactly_scoped() {
    assert!(!MIGRATION.contains("--"));
    assert!(!MIGRATION.contains("/*"));
    assert!(!MIGRATION.contains("//"));
    assert!(MIGRATION.contains("CREATE TABLE public.runtime_gateway_owners"));
    assert!(MIGRATION.contains("gateway_shard_id = 'shard:0'"));
    assert!(MIGRATION.contains("requested_lease_milliseconds NOT BETWEEN 1000 AND 300000"));
    assert!(MIGRATION.contains("runtime_gateway_owners_reject_delete"));
    assert!(MIGRATION.contains("runtime_gateway_owners_validate_transition"));
    for function in GATEWAY_OWNER_FUNCTIONS {
        assert!(MIGRATION.contains(function));
    }
}

#[test]
fn gateway_owner_calls_share_one_locked_database_clock_contract() {
    let lock_key = "'starring-runtime-gateway-owner-v1:' || expected_gateway_shard_id";
    assert_eq!(MIGRATION.matches(lock_key).count(), 4);
    assert_eq!(
        MIGRATION
            .matches("database_now := pg_catalog.clock_timestamp();")
            .count(),
        4
    );
    for body in MIGRATION.split("CREATE FUNCTION public.").skip(3).take(4) {
        let lock = body.find("pg_catalog.pg_advisory_xact_lock").unwrap();
        let clock = body
            .find("database_now := pg_catalog.clock_timestamp();")
            .unwrap();
        assert!(lock < clock);
    }
}

#[test]
fn gateway_owner_manifest_and_readiness_are_pinned() {
    assert!(MIGRATION.contains("RETURN observed_count = 495"));
    assert!(MIGRATION.contains("7853a26f4fca9cd45c863c17350d7d02ab31c2dc8c9f16828a039797e9eb9891"));
    assert!(MIGRATION.contains("4b7a0b8daf9868d92edfae0cd83e35d805d27b824ef04a8b4eb06a229caeedf0"));
    assert!(MIGRATION.contains("003baab6fe5443a3bcf6dc6356cd5595434ac68c507a56151a65874397432ff1"));
    assert!(MIGRATION.contains("runtime_gateway_owner_readiness_allowlist_patch_drift"));
    assert!(MIGRATION.contains("runtime_gateway_owner_readiness_protected_patch_drift"));
}
