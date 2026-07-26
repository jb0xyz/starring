const MIGRATION: &str =
    include_str!("../../../migrations/202607240006_classify_product_apply_drain_required.sql");

fn drain_patch() -> &'static str {
    MIGRATION
        .split("next_fragment :=")
        .nth(1)
        .unwrap()
        .split("\n\n    IF pg_catalog.strpos")
        .next()
        .unwrap()
}

#[test]
fn product_apply_drain_required_migration_is_atomic_and_comment_free() {
    assert!(!MIGRATION.contains("--"));
    assert!(!MIGRATION.contains("/*"));
    assert!(!MIGRATION.contains("//"));
    let writer_barrier = MIGRATION.find("pg_advisory_xact_lock(").unwrap();
    let table_barrier = MIGRATION.find("LOCK TABLE").unwrap();
    let preflight = MIGRATION.find("DO $preflight$").unwrap();
    let replacement = MIGRATION.find("DO $replace$").unwrap();
    let postflight = MIGRATION.find("DO $postflight$").unwrap();
    assert!(writer_barrier < table_barrier);
    assert!(table_barrier < preflight);
    assert!(preflight < replacement);
    assert!(replacement < postflight);
    assert!(MIGRATION.contains("public.runtime_writer_fence,"));
    assert!(MIGRATION.contains("public.automation_installations,"));
    assert!(MIGRATION.contains("public.runtime_deployments\nIN ACCESS EXCLUSIVE MODE;"));
    assert!(MIGRATION.contains("ON COMMIT DROP"));
    assert!(MIGRATION.contains("product_apply_drain_required_preflight_drift"));
    assert!(MIGRATION.contains("product_apply_drain_required_patch_drift"));
    assert!(MIGRATION.contains("product_apply_drain_required_postflight_drift"));
}

#[test]
fn product_apply_drain_required_is_closed_and_phase_specific() {
    let patch = drain_patch();
    assert!(patch.contains("unresolved_deployment_phase IN (''awaiting_gateway_ready'', ''live'')"));
    assert!(patch.contains("deployment.phase NOT IN (''superseded'', ''cancelled'')"));
    assert!(patch.contains("ORDER BY deployment.runtime_generation DESC, deployment.deployment_id"));
    assert!(patch.contains("LIMIT 1"));
    assert!(patch
        .contains("RETURN QUERY SELECT ''runtime_drain_required'', FALSE, FALSE, NULL::BIGINT,"));
    assert!(patch.contains("NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;"));
    assert!(patch.contains("SELECT deployment.deployment_id, deployment.phase"));
    assert!(patch.contains("INTO unresolved_deployment_id, unresolved_deployment_phase"));
    assert!(!patch.contains("requested''"));
    assert!(!patch.contains("runtime_pending''"));
    assert!(!patch.contains("reconciling_panels''"));
}

#[test]
fn product_apply_drain_required_preserves_error_precedence() {
    for predicate in [
        "baseline_position < drain_position",
        "drain_position < pending_position",
        "pending_position < generation_position",
    ] {
        assert!(MIGRATION.contains(predicate), "{predicate}");
    }
    assert!(MIGRATION.contains("'baseline_mismatch'"));
    assert!(MIGRATION.contains("'runtime_drain_required'"));
    assert!(MIGRATION.contains("'runtime_pending_conflict'"));
    assert!(MIGRATION.contains("'runtime_generation_overflow'"));
    assert!(MIGRATION.contains("pg_catalog.length(core_source)"));
    assert!(MIGRATION.contains("pg_catalog.length('runtime_drain_required')"));
}

#[test]
fn product_apply_drain_required_preserves_catalog_authority() {
    for digest in [
        "35dff4eac9780b1cea497459ac513c54e5151fc752c290228951fadd6a4c2c2d",
        "f930c836ab241c0aa56376199c0518d8cce5a446406b8503eb3f0b90ec314e38",
        "4b01ced1c2b493a04ee4745be6593c10b493ffc06d73cf62f895c9ed46e21c0b",
        "abb3775e88f9926af64f676d0f94657c8f3c80890aad2b5372116ec886a464f0",
    ] {
        assert!(MIGRATION.contains(digest), "{digest}");
    }
    assert!(MIGRATION.contains("function_oid OID NOT NULL"));
    assert!(MIGRATION.contains("function_owner OID NOT NULL"));
    assert!(MIGRATION.contains("function_acl ACLITEM[]"));
    assert!(MIGRATION.contains("snapshot_mismatch_count <> 0"));
    assert!(MIGRATION.contains("invalid_relation_count <> 0"));
    assert!(MIGRATION.contains("invalid_capability_acl_count <> 0"));
    assert!(MIGRATION.contains("external_grantee_count > 1"));
    assert!(MIGRATION.contains("function_row.provolatile <> 'v'"));
    assert!(MIGRATION.contains("function_row.proparallel <> 'u'"));
    assert!(MIGRATION.contains("NOT function_row.prosecdef"));
    assert!(MIGRATION.contains("ARRAY['search_path=pg_catalog']::TEXT[]"));
    assert!(MIGRATION.contains("EXECUTE definition;"));
    assert!(MIGRATION.contains("NOT public.starring_runtime_execution_schema_manifest_v1()"));
    assert!(!MIGRATION.contains("GRANT "));
    assert!(!MIGRATION.contains("REVOKE "));
}
