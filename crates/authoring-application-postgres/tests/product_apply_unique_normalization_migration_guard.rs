const MIGRATION: &str = include_str!(
    "../../../migrations/202607240007_normalize_product_apply_lane_unique_conflicts.sql"
);

fn normalization_patch() -> &'static str {
    MIGRATION
        .split("wrapped_fragment :=")
        .nth(1)
        .unwrap()
        .split("\n\n    definition :=")
        .next()
        .unwrap()
}

#[test]
fn product_apply_unique_normalization_is_atomic_and_comment_free() {
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
    assert!(MIGRATION.contains("product_apply_unique_normalization_preflight_drift"));
    assert!(MIGRATION.contains("product_apply_unique_normalization_patch_drift"));
    assert!(MIGRATION.contains("product_apply_unique_normalization_fragment_drift"));
    assert!(MIGRATION.contains("product_apply_unique_normalization_postflight_drift"));
}

#[test]
fn product_apply_unique_normalization_is_exact_and_preserves_other_violations() {
    let patch = normalization_patch();
    let insert = patch.find("'    ' || pg_catalog.replace(").unwrap();
    let handler = patch.find("WHEN unique_violation THEN").unwrap();
    let diagnostics = patch.find("GET STACKED DIAGNOSTICS").unwrap();
    let schema = patch
        .find("runtime_unique_schema_name = ''public''")
        .unwrap();
    let table = patch
        .find("runtime_unique_table_name = ''runtime_deployments''")
        .unwrap();
    let lane = patch
        .find("''runtime_deployments_lane_generation_unique''")
        .unwrap();
    let unresolved = patch
        .find("''runtime_deployments_one_unresolved_per_lane''")
        .unwrap();
    let serialization = patch.find("ERRCODE = ''40001''").unwrap();
    let reraise = patch.find("'            RAISE;'").unwrap();
    assert!(insert < handler);
    assert!(handler < diagnostics);
    assert!(diagnostics < schema);
    assert!(schema < table);
    assert!(table < lane);
    assert!(lane < unresolved);
    assert!(unresolved < serialization);
    assert!(serialization < reraise);
    assert_eq!(
        MIGRATION
            .matches("INSERT INTO public.runtime_deployments")
            .count(),
        2
    );
    assert!(MIGRATION.contains("insert_marker := '    INSERT INTO public.runtime_deployments ('"));
    assert!(MIGRATION.contains("clear_position - insert_position"));
    assert_eq!(patch.matches("WHEN unique_violation THEN").count(), 1);
    assert_eq!(
        patch
            .matches("runtime_deployments_lane_generation_unique")
            .count(),
        1
    );
    assert_eq!(
        patch
            .matches("runtime_deployments_one_unresolved_per_lane")
            .count(),
        1
    );
    assert!(patch.contains("runtime_unique_constraint_name = CONSTRAINT_NAME"));
    assert!(patch.contains("runtime_unique_schema_name = SCHEMA_NAME"));
    assert!(patch.contains("runtime_unique_table_name = TABLE_NAME"));
    assert!(!patch.contains("product_action_receipts"));
    assert!(!patch.contains("product_audit_events"));
}

#[test]
fn product_apply_unique_normalization_preflights_exact_indexes() {
    assert!(MIGRATION.contains("constraint_row.contype = 'u'"));
    assert!(MIGRATION.contains("NOT constraint_row.condeferrable"));
    assert!(MIGRATION.contains("constraint_row.convalidated"));
    assert!(MIGRATION.contains("index_metadata.indisunique"));
    assert!(MIGRATION.contains("index_metadata.indisvalid"));
    assert!(MIGRATION.contains("index_metadata.indisready"));
    assert!(MIGRATION.contains("index_metadata.indislive"));
    assert!(MIGRATION.contains("index_metadata.indpred IS NULL"));
    assert!(MIGRATION.contains("'UNIQUE (guild_id, ruleset_key, runtime_generation)'"));
    assert!(MIGRATION.contains(
        "CREATE UNIQUE INDEX runtime_deployments_one_unresolved_per_lane ON public.runtime_deployments USING btree (guild_id, ruleset_key)"
    ));
    assert!(MIGRATION.contains("index_metadata.indnkeyatts = 2"));
    assert!(MIGRATION.contains("index_metadata.indnatts = 2"));
    assert!(MIGRATION.contains("valid_lane_constraint_count <> 1"));
    assert!(MIGRATION.contains("valid_unresolved_index_count <> 1"));
}

#[test]
fn product_apply_unique_normalization_preserves_catalog_authority() {
    for digest in [
        "c8fd0ff8a91cb0176dfb5ff64e355a79c20f247acd80e97890454c19f73f3765",
        "ed94feac48946d7067481dd607743295f57f1e13c93231818ebf22d99bc639ac",
    ] {
        assert!(MIGRATION.contains(digest), "{digest}");
    }
    assert!(MIGRATION.contains("function_oid OID PRIMARY KEY"));
    assert!(MIGRATION.contains("function_owner OID NOT NULL"));
    assert!(MIGRATION.contains("function_acl ACLITEM[]"));
    assert!(MIGRATION.contains("snapshot_mismatch_count <> 0"));
    assert!(MIGRATION.contains("invalid_capability_acl_count <> 0"));
    assert!(MIGRATION.contains("external_grantee_count > 1"));
    assert!(MIGRATION.contains("function_row.provolatile <> 'v'"));
    assert!(MIGRATION.contains("function_row.proparallel <> 'u'"));
    assert!(MIGRATION.contains("NOT function_row.prosecdef"));
    assert!(MIGRATION.contains("ARRAY['search_path=pg_catalog']::TEXT[]"));
    assert!(MIGRATION.contains("EXECUTE definition;"));
    assert!(MIGRATION.contains("NOT public.starring_runtime_execution_schema_manifest_v1()"));
    assert!(!MIGRATION.contains("__PRODUCT_APPLY_UNIQUE_NORMALIZATION_DIGEST__"));
    assert!(!MIGRATION.contains("GRANT "));
    assert!(!MIGRATION.contains("REVOKE "));
}
