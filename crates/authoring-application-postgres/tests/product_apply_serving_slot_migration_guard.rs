const MIGRATION: &str =
    include_str!("../../../migrations/202607240005_lock_product_apply_serving_slot.sql");

fn slot_patch() -> &'static str {
    MIGRATION
        .split("next_delegation :=")
        .nth(1)
        .unwrap()
        .split("\n\n    IF pg_catalog.strpos")
        .next()
        .unwrap()
}

#[test]
fn product_apply_serving_slot_migration_is_atomic_and_comment_free() {
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
    assert!(MIGRATION.contains("product_apply_serving_slot_preflight_drift"));
    assert!(MIGRATION.contains("product_apply_serving_slot_postflight_drift"));
}

#[test]
fn product_apply_serving_slot_patch_uses_immutable_identity_and_global_order() {
    let patch = slot_patch();
    let installation = patch
        .find("FROM public.automation_installations AS installation")
        .unwrap();
    let slot = patch.find("starring-runtime-serving-slot-v1:").unwrap();
    let deployment = patch
        .find("FROM public.runtime_deployments AS deployment")
        .unwrap();
    let update = patch.find("FOR UPDATE").unwrap();
    let delegate = patch.find("RETURN QUERY").unwrap();
    assert!(installation < slot);
    assert!(slot < deployment);
    assert!(deployment < update);
    assert!(update < delegate);
    assert!(patch.contains("SELECT installation.discord_guild_id, installation.ruleset_key"));
    assert!(patch.contains("installation.tenant_id = expected_tenant_id"));
    assert!(patch.contains("installation.installation_id = expected_installation_id"));
    assert!(patch.contains("pg_catalog.concat("));
    assert!(patch.contains("serving_slot_guild_id"));
    assert!(patch.contains("serving_slot_ruleset_key"));
    assert!(patch.contains("WHERE deployment.guild_id = serving_slot_guild_id"));
    assert!(patch.contains("deployment.ruleset_key = serving_slot_ruleset_key"));
    assert!(patch.contains("ORDER BY deployment.runtime_generation, deployment.deployment_id"));
    assert!(patch.contains("IF FOUND THEN"));
    assert!(!patch.contains("FOR SHARE"));
    assert!(!patch.contains("expected_guild_id"));
}

#[test]
fn product_apply_serving_slot_postflight_proves_complete_lock_order() {
    for predicate in [
        "writer_position < fence_position",
        "fence_position < share_position",
        "share_position < invalid_position",
        "invalid_position < closed_position",
        "closed_position < installation_position",
        "installation_position < slot_position",
        "slot_position < deployment_position",
        "deployment_position < update_position",
        "update_position < delegate_position",
    ] {
        assert!(MIGRATION.contains(predicate), "{predicate}");
    }
    assert!(MIGRATION.contains("'pg_advisory_xact_lock_shared'"));
    assert!(MIGRATION.contains("'FROM public.runtime_writer_fence AS fence'"));
    assert!(MIGRATION.contains("'FROM public.automation_installations AS installation'"));
    assert!(MIGRATION.contains("'FROM public.runtime_deployments AS deployment'"));
    assert!(MIGRATION.contains("'starring_product_apply_lock_core_unfenced_v1'"));
}

#[test]
fn product_apply_serving_slot_patch_preserves_catalog_authority() {
    for digest in [
        "35dff4eac9780b1cea497459ac513c54e5151fc752c290228951fadd6a4c2c2d",
        "eb864584f7fd1d715c62ce1f1fa38b662deeaadf1b3483155ffac8fffbbff3f0",
        "f930c836ab241c0aa56376199c0518d8cce5a446406b8503eb3f0b90ec314e38",
        "4b01ced1c2b493a04ee4745be6593c10b493ffc06d73cf62f895c9ed46e21c0b",
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
    assert!(MIGRATION.contains("external_grantee = 0"));
    assert!(MIGRATION.contains("function_row.provolatile <> 'v'"));
    assert!(MIGRATION.contains("function_row.proparallel <> 'u'"));
    assert!(MIGRATION.contains("NOT function_row.prosecdef"));
    assert!(MIGRATION.contains("ARRAY['search_path=pg_catalog']::TEXT[]"));
    assert!(MIGRATION.contains("EXECUTE definition;"));
    assert!(MIGRATION.contains("NOT public.starring_runtime_execution_schema_manifest_v1()"));
    assert!(!MIGRATION.contains("__PRODUCT_APPLY_SLOT_WRAPPER_DIGEST__"));
    assert!(!MIGRATION.contains("GRANT "));
    assert!(!MIGRATION.contains("REVOKE "));
}
