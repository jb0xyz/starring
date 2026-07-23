const MIGRATION: &str =
    include_str!("../../../migrations/202607240002_fence_product_apply_writer.sql");
const FENCE_MIGRATION: &str =
    include_str!("../../../migrations/202607230002_persist_runtime_writer_fence.sql");

fn wrapper_body() -> &'static str {
    MIGRATION
        .split("AS $function$")
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap()
}

fn count(haystack: &str, needle: &str) -> usize {
    haystack.match_indices(needle).count()
}

#[test]
fn product_apply_writer_fence_migration_is_additive_and_comment_free() {
    assert!(!MIGRATION.contains("--"));
    assert!(!MIGRATION.contains("/*"));
    assert!(!MIGRATION.contains("//"));
    assert_eq!(
        count(
            MIGRATION,
            ") RENAME TO starring_product_apply_lock_core_unfenced_v1;"
        ),
        1
    );
    assert_eq!(
        count(
            MIGRATION,
            "CREATE FUNCTION public.starring_product_apply_lock_core_v1("
        ),
        1
    );
    assert_eq!(count(MIGRATION, "REVOKE ALL ON FUNCTION"), 2);
    assert!(!MIGRATION.contains("GRANT "));
    assert!(!MIGRATION.contains("CREATE TABLE"));
    assert!(!MIGRATION.contains("runtime_product_operations_v2"));
    assert!(!MIGRATION.contains("runtime_drain_intents_v2"));
    assert!(MIGRATION.contains("public.runtime_deployments\nIN ACCESS EXCLUSIVE MODE;"));
    assert!(MIGRATION.contains("f31f6c2e37558e7d89d3125588acb71186421936ba2cf8762ca4b18462f8a693"));
    assert!(MIGRATION.contains("35dff4eac9780b1cea497459ac513c54e5151fc752c290228951fadd6a4c2c2d"));
    assert!(MIGRATION.contains("9d854a6bcec21072bad8b9f725ea5401125d9842be3a4f5f238cdaaccb863eef"));
    assert!(MIGRATION.contains("4b01ced1c2b493a04ee4745be6593c10b493ffc06d73cf62f895c9ed46e21c0b"));
    assert!(MIGRATION.contains("collision_count <> 0"));
    assert!(MIGRATION.contains("helper_collision_count <> 2"));
    assert!(MIGRATION.contains("active_row.proowner <> common_owner"));
    assert!(MIGRATION.contains("invalid_active_function_count <> 0"));
}

#[test]
fn product_apply_writer_fence_wrapper_fails_closed_before_the_original_core() {
    let body = wrapper_body();
    let writer_lock = body
        .find("pg_catalog.pg_advisory_xact_lock_shared")
        .unwrap();
    let fence_read = body
        .find("FROM public.runtime_writer_fence AS fence")
        .unwrap();
    let fence_invalid = body.find("runtime_writer_fence_invalid").unwrap();
    let fence_closed = body.find("runtime_writer_fenced").unwrap();
    let original_core = body
        .find("public.starring_product_apply_lock_core_unfenced_v1")
        .unwrap();

    assert!(writer_lock < fence_read);
    assert!(fence_read < fence_invalid);
    assert!(fence_invalid < fence_closed);
    assert!(fence_closed < original_core);
    assert!(body.contains("writer_fence_state IS DISTINCT FROM 'open'"));
    assert!(body.contains("writer_fence_state IS DISTINCT FROM 'closed'"));
    assert!(!body.contains("starring-runtime-serving-slot-v1:"));
    assert!(!body.contains("FROM public.automation_installations"));
    assert!(!body.contains("FROM public.runtime_deployments"));
    assert!(!body.contains("FOR UPDATE"));
    assert!(!body.contains("FOR SHARE"));
    for forbidden in ["INSERT INTO", "UPDATE public.", "DELETE FROM", "TRUNCATE"] {
        assert!(!body.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn product_apply_writer_fence_uses_the_canonical_domain_and_exact_acl_shape() {
    let domain = "pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)";
    assert!(wrapper_body().contains(domain));
    assert!(FENCE_MIGRATION.contains(domain));
    assert!(MIGRATION.contains("external_grantee_count > 1"));
    assert!(MIGRATION.contains("external_grantee_count = 1 AND external_grantee = 0"));
    assert!(MIGRATION.contains("invalid_external_acl_count <> 0"));
    assert!(MIGRATION.contains("invalid_capability_acl_count <> 0"));
    assert!(MIGRATION.contains("external_grantee_count <> 1"));
    assert!(MIGRATION.contains("privilege.privilege_type <> 'EXECUTE'"));
    assert!(MIGRATION.contains("privilege.is_grantable"));
    assert!(MIGRATION.contains("privilege.grantor <> common_owner"));
    assert!(MIGRATION.contains("privilege.grantee IS DISTINCT FROM external_grantee"));
    assert!(MIGRATION.contains("public.starring_product_apply_executor_database_identity_v1()"));
    assert!(MIGRATION.contains("public.starring_product_apply_target_artifact_v1"));
    assert!(MIGRATION.contains("public.starring_product_apply_finalize_v1"));
    assert!(MIGRATION.contains("public.starring_product_apply_keyring_coverage_v1"));
    assert!(MIGRATION.contains("WHERE privilege.grantee <> common_owner"));
}

#[test]
fn product_apply_writer_fence_preserves_contract_and_restores_settings() {
    assert!(MIGRATION.contains("VOLATILE\nSTRICT\nPARALLEL UNSAFE\nSECURITY DEFINER"));
    assert!(MIGRATION.contains("SET search_path = pg_catalog\nROWS 1"));
    assert!(MIGRATION.contains("function_row.prorows <> 1::REAL"));
    assert!(MIGRATION.contains("function_row.proparallel <> 'u'"));
    assert!(MIGRATION.contains("function_row.proconfig"));
    assert!(MIGRATION.contains("ARRAY['search_path=pg_catalog']::TEXT[]"));
    assert!(MIGRATION.contains("NOT public.starring_runtime_execution_schema_manifest_v1()"));
    assert!(MIGRATION.contains("RESET statement_timeout;"));
    assert!(MIGRATION.contains("RESET lock_timeout;"));
    assert!(MIGRATION.contains("RESET search_path;"));
}
