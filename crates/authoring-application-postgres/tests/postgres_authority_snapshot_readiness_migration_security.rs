const MIGRATION: &str =
    include_str!("../../../migrations/202607200006_scope_authority_snapshot_readiness.sql");

#[test]
fn authority_snapshot_readiness_migration_has_closed_security_contracts() {
    assert!(!MIGRATION.contains("--"));
    assert!(!MIGRATION.contains("/*"));
    assert_eq!(MIGRATION.matches("CREATE FUNCTION public.").count(), 3);
    assert!(MIGRATION.starts_with(
        "SET LOCAL lock_timeout = '5s';\nSET LOCAL statement_timeout = '30s';\nSET LOCAL search_path = pg_catalog;\n"
    ));
    assert!(MIGRATION.contains(
        "LOCK TABLE\n    public.product_control_plane_identity,\n    public.product_principals,\n    public.product_auth_sessions,\n    public.product_tenants,\n    public.automation_installations,\n    public.automation_installation_authority_versions,\n    public.authoring_sessions,\n    public.authoring_session_generations\nIN ACCESS SHARE MODE;"
    ));
    for required in [
        "starring_product_installation_authority_reader_database_identity_v1",
        "starring_product_authorized_snapshot_reader_database_identity_v1",
        "starring_product_authorized_snapshot_key_coverage_v1",
        "SECURITY DEFINER",
        "SET search_path = pg_catalog",
        "REVOKE ALL PRIVILEGES ON FUNCTION",
        "REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC",
        "user routine public execution is not sealed",
        "user routine execution defaults are not sealed",
        "function_row.proowner <> common_owner",
        "function_row.provolatile <> 'v'",
        "NOT function_row.proisstrict",
        "function_row.proparallel <> 'u'",
        "function_row.proleakproof",
        "function_row.pronargdefaults <> 0",
        "function_row.provariadic <> 0",
        "language_row.lanname IS DISTINCT FROM 'sql'",
        "configured_encryption_key_ids TEXT[]",
        "RETURNS TABLE(covered BOOLEAN)",
        "pg_catalog.cardinality(configured_encryption_key_ids)",
        "input.key_count BETWEEN 1 AND 8",
        "ELSE ARRAY[]::TEXT[]",
        "pg_catalog.octet_length(configured.key_id)",
        "generation.encryption_key_id",
    ] {
        assert!(
            MIGRATION.contains(required),
            "missing migration guard: {required}"
        );
    }
    for forbidden in [
        "RETURNS TABLE(encryption_key_id",
        "RETURN QUERY SELECT generation.encryption_key_id",
        "CREATE ROLE",
        "GRANT EXECUTE ON FUNCTION",
        "current_setting('",
    ] {
        assert!(
            !MIGRATION.contains(forbidden),
            "migration exposes forbidden surface: {forbidden}"
        );
    }
}
