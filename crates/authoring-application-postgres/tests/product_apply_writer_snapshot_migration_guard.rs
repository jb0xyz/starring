const MIGRATION: &str =
    include_str!("../../../migrations/202607240004_snapshot_safe_product_apply_writer.sql");

#[test]
fn product_apply_writer_snapshot_migration_is_atomic_and_comment_free() {
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
    assert!(MIGRATION.contains("public.runtime_deployments\nIN ACCESS EXCLUSIVE MODE;"));
    assert!(MIGRATION.contains("ON COMMIT DROP"));
    assert!(MIGRATION.contains("product_apply_writer_snapshot_preflight_drift"));
    assert!(MIGRATION.contains("product_apply_writer_snapshot_postflight_drift"));
}

#[test]
fn product_apply_writer_snapshot_patch_is_exact_and_snapshot_safe() {
    assert!(MIGRATION.contains("pg_get_functiondef(function_row.oid)"));
    assert!(MIGRATION.contains("WHERE fence.singleton;'"));
    assert!(MIGRATION.contains("'    FOR SHARE;'"));
    assert!(MIGRATION.contains("pg_catalog.replace(\n        definition,"));
    assert!(MIGRATION.contains("EXECUTE definition;"));
    assert!(MIGRATION.contains("wrapper_source"));
    assert!(MIGRATION.contains("writer_position < fence_position"));
    assert!(MIGRATION.contains("fence_position < share_position"));
    assert!(MIGRATION.contains("share_position < invalid_position"));
    assert!(MIGRATION.contains("invalid_position < closed_position"));
    assert!(MIGRATION.contains("closed_position < delegate_position"));
}

#[test]
fn product_apply_writer_snapshot_patch_preserves_catalog_authority() {
    for digest in [
        "35dff4eac9780b1cea497459ac513c54e5151fc752c290228951fadd6a4c2c2d",
        "9d854a6bcec21072bad8b9f725ea5401125d9842be3a4f5f238cdaaccb863eef",
        "eb864584f7fd1d715c62ce1f1fa38b662deeaadf1b3483155ffac8fffbbff3f0",
        "4b01ced1c2b493a04ee4745be6593c10b493ffc06d73cf62f895c9ed46e21c0b",
    ] {
        assert!(MIGRATION.contains(digest), "{digest}");
    }
    assert!(MIGRATION.contains("function_oid OID NOT NULL"));
    assert!(MIGRATION.contains("function_owner OID NOT NULL"));
    assert!(MIGRATION.contains("function_acl ACLITEM[]"));
    assert!(MIGRATION.contains("snapshot_mismatch_count <> 0"));
    assert!(MIGRATION.contains("invalid_capability_acl_count <> 0"));
    assert!(MIGRATION.contains("external_grantee_count > 1"));
    assert!(MIGRATION.contains("external_grantee = 0"));
    assert!(MIGRATION.contains("NOT public.starring_runtime_execution_schema_manifest_v1()"));
    assert!(!MIGRATION.contains("__CORE_DEFINITION_DIGEST__"));
    assert!(!MIGRATION.contains("GRANT "));
    assert!(!MIGRATION.contains("REVOKE "));
}
