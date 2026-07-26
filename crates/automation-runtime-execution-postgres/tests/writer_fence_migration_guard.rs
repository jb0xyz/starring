const MIGRATION: &str =
    include_str!("../../../migrations/202607230002_persist_runtime_writer_fence.sql");

#[test]
fn writer_fence_migration_is_comment_free_and_exactly_scoped() {
    assert!(!MIGRATION.contains("--"));
    assert!(!MIGRATION.contains("/*"));
    assert!(!MIGRATION.contains("//"));
    assert!(MIGRATION.contains("CREATE TABLE public.runtime_writer_fence"));
    assert!(MIGRATION.contains("VALUES (TRUE, 'open', 1, 0, NULL, NULL)"));
    assert!(MIGRATION.contains("cutover_coordinator_id ~ '^[0-9a-f]{32}$'"));
    assert!(MIGRATION.contains("cutover_lease_epoch_high_water >= 1"));
    assert!(MIGRATION.contains("cutover_expires_at IS NOT NULL"));
    assert!(MIGRATION.contains("runtime_writer_fence_preflight_drift"));
    assert!(MIGRATION.contains("runtime_writer_fence_postflight_drift"));
    assert!(MIGRATION.contains("runtime_writer_fence_reject_row_mutation"));
    assert!(MIGRATION.contains("runtime_writer_fence_reject_truncate"));
    assert!(MIGRATION.contains("runtime_writer_fence_mutation_rejected"));
}

#[test]
fn writer_fence_observation_is_locked_and_expiry_does_not_open_it() {
    let body = MIGRATION
        .split("CREATE FUNCTION public.starring_runtime_writer_fence_observe_v1()")
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap();
    let lock = body
        .find("pg_catalog.pg_advisory_xact_lock_shared")
        .unwrap();
    let clock = body
        .find("database_now := pg_catalog.clock_timestamp();")
        .unwrap();
    assert!(lock < clock);
    assert!(body.contains("fence.fence_state = 'closed'"));
    assert!(!body.contains("cutover_expires_at <= database_now"));
    assert!(!body.contains("cutover_expires_at > database_now"));
    assert!(!body.contains("UPDATE "));
    assert!(!body.contains("DELETE "));
}

#[test]
fn writer_fence_executor_surface_is_observation_only() {
    assert!(MIGRATION
        .contains("GRANT EXECUTE ON FUNCTION public.starring_runtime_writer_fence_observe_v1()"));
    assert!(!MIGRATION.contains("GRANT SELECT ON TABLE public.runtime_writer_fence"));
    assert!(!MIGRATION.contains("GRANT UPDATE ON TABLE public.runtime_writer_fence"));
    assert!(!MIGRATION.contains("GRANT DELETE ON TABLE public.runtime_writer_fence"));
    for forbidden in [
        "starring_runtime_writer_fence_close_v1",
        "starring_runtime_writer_fence_open_v1",
        "starring_runtime_writer_fence_renew_v1",
        "starring_runtime_writer_fence_takeover_v1",
    ] {
        assert!(!MIGRATION.contains(forbidden));
    }
}

#[test]
fn writer_fence_manifest_and_readiness_are_pinned() {
    assert!(MIGRATION.contains("RETURN observed_count = 514"));
    assert!(MIGRATION.contains("6f00d0c25506999af7d03eec22ab01513ea4711bbc7dc6eacae2f0f3ce8cd2f5"));
    assert!(MIGRATION.contains("42c6652f5d25634d247821002b619acac6dff1997d6cd1df2ea633d310456061"));
    assert!(MIGRATION.contains("cf84d5a445c591cd11802e9d956c2f03ae7f9c4205134aa1511d4cc40d88fbc3"));
    assert!(MIGRATION.contains("runtime_writer_fence_manifest_relation_patch_drift"));
    assert!(MIGRATION.contains("runtime_writer_fence_manifest_function_patch_drift"));
    assert!(MIGRATION.contains("runtime_writer_fence_readiness_relation_patch_drift"));
    assert!(MIGRATION.contains("runtime_writer_fence_readiness_function_patch_drift"));
    assert!(MIGRATION.contains("runtime_writer_fence_readiness_allowlist_patch_drift"));
    assert!(MIGRATION.contains("runtime_writer_fence_readiness_protected_patch_drift"));
}
