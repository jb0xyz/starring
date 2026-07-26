const MIGRATION: &str =
    include_str!("../../../migrations/202607240001_persist_runtime_product_drain_observation.sql");

const OBSERVE_IDENTITY: &str =
    "public.starring_runtime_product_drain_observe_v2(TEXT,TEXT,TEXT,BIGINT,TEXT,TEXT)";

fn observe_body() -> &'static str {
    MIGRATION
        .split("CREATE FUNCTION public.starring_runtime_product_drain_observe_v2(")
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap()
}

#[test]
fn product_drain_migration_is_comment_free_and_observation_only() {
    assert!(!MIGRATION.contains("--"));
    assert!(!MIGRATION.contains("/*"));
    assert!(!MIGRATION.contains("//"));
    assert!(MIGRATION.contains("CREATE TABLE public.runtime_product_operations_v2"));
    assert!(MIGRATION.contains("CREATE TABLE public.runtime_drain_intents_v2"));
    assert!(MIGRATION.contains(OBSERVE_IDENTITY));
    for forbidden in [
        "starring_runtime_product_drain_insert",
        "starring_runtime_product_drain_create",
        "starring_runtime_product_drain_update",
        "starring_runtime_product_drain_mutate",
        "GRANT SELECT ON TABLE",
        "GRANT INSERT ON TABLE",
        "GRANT UPDATE ON TABLE",
        "GRANT DELETE ON TABLE",
    ] {
        assert!(!MIGRATION.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn product_drain_tables_have_independent_natural_and_id_uniqueness() {
    assert!(MIGRATION.contains("product_operation_id TEXT PRIMARY KEY"));
    assert!(MIGRATION.contains("drain_intent_id TEXT PRIMARY KEY"));
    assert!(MIGRATION.contains("runtime_product_operations_v2_natural_unique UNIQUE (\n        tenant_id,\n        installation_id,\n        deployment_id,\n        expected_revision"));
    assert!(MIGRATION.contains("runtime_drain_intents_v2_natural_unique UNIQUE (\n        tenant_id,\n        installation_id,\n        deployment_id,\n        slot_guild_id,\n        slot_ruleset_key,\n        expected_revision"));
    assert!(MIGRATION.contains(
        "runtime_drain_intents_v2_product_unique UNIQUE (\n        product_operation_id"
    ));
    assert!(MIGRATION.contains("runtime_drain_intents_v2_product_fk FOREIGN KEY"));
}

#[test]
fn product_drain_rows_are_canonical_bounded_and_immutable() {
    for required in [
        "product_operation_id ~ '^[0-9a-f]{32}$'",
        "drain_intent_id ~ '^[0-9a-f]{32}$'",
        "product_mutation_digest ~ '^[0-9a-f]{64}$'",
        "drain_intent_digest ~ '^[0-9a-f]{64}$'",
        "BETWEEN 1 AND 32768",
        "BETWEEN 1 AND 65536",
        "expected_revision BETWEEN 1 AND 9223372036854775807",
        "intent_revision = 1",
        "intent_state = 'pending'",
        "product_mutation_request_bytes BYTEA",
        "drain_intent_request_bytes BYTEA",
        "COLLATE pg_catalog.\"C\"",
        "<= '18446744073709551615' COLLATE pg_catalog.\"C\"",
        "expected_target_version BETWEEN 1 AND 4294967295",
        "expected_target_binding_revision",
        "runtime_product_operations_v2_reject_row_mutation",
        "BEFORE INSERT OR UPDATE OR DELETE ON public.runtime_product_operations_v2",
        "runtime_product_operations_v2_reject_truncate",
        "runtime_drain_intents_v2_reject_row_mutation",
        "BEFORE INSERT OR UPDATE OR DELETE ON public.runtime_drain_intents_v2",
        "runtime_drain_intents_v2_reject_truncate",
        "runtime_product_drain_mutation_rejected",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert!(!MIGRATION.contains("::NUMERIC"));
}

#[test]
fn product_drain_observer_is_id_free_and_returns_closed_outcomes() {
    let header = MIGRATION
        .split("CREATE FUNCTION public.starring_runtime_product_drain_observe_v2(")
        .nth(1)
        .unwrap()
        .split(")\nRETURNS TABLE")
        .next()
        .unwrap();
    assert_eq!(header.matches("expected_").count(), 6);
    for forbidden in [
        "operation_id",
        "intent_id",
        "mutation_digest",
        "intent_digest",
        "request_bytes BYTEA",
    ] {
        assert!(!header.contains(forbidden), "{forbidden}");
    }
    for outcome in [
        "absent",
        "present",
        "ambiguous_product",
        "ambiguous_drain",
        "partial_product",
        "partial_drain",
        "pair_mismatch",
    ] {
        assert!(
            observe_body().contains(&format!("'{outcome}'")),
            "{outcome}"
        );
    }
}

#[test]
fn product_drain_observer_uses_the_global_lock_order_and_counts_before_selecting() {
    let body = observe_body();
    let isolation = body
        .find("pg_catalog.current_setting('transaction_isolation')")
        .unwrap();
    let writer_fence = body
        .find("pg_catalog.pg_advisory_xact_lock_shared")
        .unwrap();
    let writer_fence_read = body
        .find("FROM public.runtime_writer_fence AS fence")
        .unwrap();
    let serving_slot = body.find("pg_catalog.pg_advisory_xact_lock(").unwrap();
    let writer_fence_invalid = body
        .find("runtime_product_drain_observe_writer_fence_invalid")
        .unwrap();
    let writer_fence_closed = body
        .find("runtime_product_drain_observe_writer_fenced")
        .unwrap();
    let deployment = body
        .find("FROM public.runtime_deployments AS deployment")
        .unwrap();
    let deployment_lock = body[deployment..].find("FOR UPDATE").unwrap() + deployment;
    let clock = body
        .find("observed_at := pg_catalog.clock_timestamp();")
        .unwrap();
    let product_count = body
        .find("INTO product_count\n    FROM public.runtime_product_operations_v2")
        .unwrap();
    let drain_count = body
        .find("INTO drain_count\n    FROM public.runtime_drain_intents_v2")
        .unwrap();
    let product_select = body.find("INTO STRICT product_row").unwrap();
    let drain_select = body.find("INTO STRICT drain_row").unwrap();
    assert!(isolation < writer_fence);
    assert!(body.contains("<> 'read committed'"));
    assert!(body.contains("ERRCODE = 'RX004'"));
    assert!(writer_fence < writer_fence_read);
    assert!(writer_fence_read < writer_fence_invalid);
    assert!(writer_fence_invalid < writer_fence_closed);
    assert!(writer_fence_closed < serving_slot);
    assert!(writer_fence_read < serving_slot);
    assert!(serving_slot < deployment);
    assert!(deployment < deployment_lock);
    assert!(deployment_lock < clock);
    assert!(clock < product_count);
    assert!(product_count < drain_count);
    assert!(drain_count < product_select);
    assert!(drain_count < drain_select);
    assert!(body.contains("'starring-runtime-writer-fence-v1'"));
    assert!(body.contains("'starring-runtime-serving-slot-v1:'"));
    assert!(body.contains("ERRCODE = 'RX005'"));
    assert!(body.contains("ERRCODE = 'RX001'"));
    assert!(body.contains("ERRCODE = 'RX002'"));
    assert!(body.contains("writer_fence_state IS DISTINCT FROM 'open'"));
    assert!(body.contains("writer_fence_state IS DISTINCT FROM 'closed'"));
    assert!(body.contains("IF writer_fence_state = 'closed' THEN"));
    assert_eq!(body.matches("= expected_revision").count(), 0);
    assert_eq!(body.matches("$4").count(), 6);
}

#[test]
fn product_drain_executor_surface_is_function_only() {
    assert!(MIGRATION.contains(
        "GRANT EXECUTE ON FUNCTION public.starring_runtime_product_drain_observe_v2(TEXT,TEXT,TEXT,BIGINT,TEXT,TEXT)"
    ));
    assert!(
        MIGRATION.contains("REVOKE ALL ON TABLE public.runtime_product_operations_v2 FROM PUBLIC")
    );
    assert!(MIGRATION.contains("REVOKE ALL ON TABLE public.runtime_drain_intents_v2 FROM PUBLIC"));
    assert!(!MIGRATION
        .contains("GRANT EXECUTE ON FUNCTION public.reject_runtime_product_drain_mutation"));
}

#[test]
fn product_drain_manifest_and_readiness_are_pinned() {
    for placeholder in [
        "__MANIFEST_OBSERVED_COUNT__",
        "__MANIFEST_OBSERVED_DIGEST__",
        "__SCHEMA_MANIFEST_DEFINITION_DIGEST__",
        "__READINESS_DEFINITION_DIGEST__",
    ] {
        assert!(!MIGRATION.contains(placeholder), "{placeholder}");
    }
    for marker in [
        "runtime_product_drain_manifest_relation_patch_drift",
        "runtime_product_drain_manifest_function_patch_drift",
        "runtime_product_drain_readiness_relation_patch_drift",
        "runtime_product_drain_readiness_function_patch_drift",
        "runtime_product_drain_readiness_allowlist_patch_drift",
        "runtime_product_drain_readiness_protected_patch_drift",
        "runtime_product_drain_postflight_drift",
    ] {
        assert!(MIGRATION.contains(marker), "{marker}");
    }
}
