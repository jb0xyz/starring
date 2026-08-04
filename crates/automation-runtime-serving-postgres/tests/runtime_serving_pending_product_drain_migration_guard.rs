const HEARTBEAT_MIGRATION: &str = include_str!(
    "../../../migrations/202608040002_interrupt_serving_for_pending_product_drain_v1.sql"
);
const MANIFEST_MIGRATION: &str = include_str!(
    "../../../migrations/202608040003_refresh_serving_pending_product_drain_manifest_v1.sql"
);
const READINESS_MIGRATION: &str = include_str!(
    "../../../migrations/202608040004_refresh_serving_pending_product_drain_readiness_v1.sql"
);

#[test]
fn heartbeat_interrupts_only_for_an_exact_current_slot_drain() {
    for required in [
        "public.runtime_slot_writer_fences_v2 AS fence",
        "fence.slot_guild_id = deployment_row.guild_id",
        "fence.slot_ruleset_key = deployment_row.ruleset_key",
        "slot_fence_row.pending_tenant_id",
        "IS DISTINCT FROM expected_tenant_id",
        "slot_fence_row.pending_installation_id",
        "IS DISTINCT FROM expected_installation_id",
        "slot_fence_row.pending_deployment_id",
        "IS DISTINCT FROM expected_deployment_id",
        "slot_fence_row.pending_expected_revision",
        "IS DISTINCT FROM deployment_row.revision",
        "public.runtime_drain_intents_v2 AS drain",
        "drain.drain_intent_id",
        "slot_fence_row.pending_drain_intent_id",
        "public.starring_runtime_serving_observe_pending_drain_source_v1(",
        "pending_source.outcome_name IS DISTINCT FROM ''current''",
        "pending_source.attestation_digest",
        "IS DISTINCT FROM expected_attestation_id",
        "pending_source.process_identity ->> ''process_instance_id''",
        "IS DISTINCT FROM expected_process_instance_id",
        "pending_source.lease_epoch",
        "IS DISTINCT FROM expected_lease_epoch",
        "pending_source.connected IS DISTINCT FROM TRUE",
        "pending_source.serving IS DISTINCT FROM TRUE",
        "ERRCODE = ''RS003''",
        "runtime_serving_heartbeat_product_drain_required",
        "ERRCODE = ''RS004''",
        "runtime_serving_heartbeat_product_drain_invalid",
    ] {
        assert!(HEARTBEAT_MIGRATION.contains(required), "{required}");
    }

    let guard = HEARTBEAT_MIGRATION
        .split("new_guard_fragment TEXT :=")
        .nth(1)
        .unwrap()
        .split("old_definition_digest TEXT :=")
        .next()
        .unwrap();
    assert!(
        guard.find("runtime_slot_writer_fences_v2").unwrap()
            < guard.find("SELECT lease.*").unwrap()
    );
    assert_eq!(
        guard
            .matches("runtime_slot_writer_fences_v2 AS fence")
            .count(),
        1
    );
    assert_eq!(
        guard.matches("runtime_drain_intents_v2 AS drain").count(),
        1
    );
    assert!(!guard.contains(
        "FROM public.runtime_slot_writer_fences_v2 AS fence\\n        WHERE fence.pending"
    ));
}

#[test]
fn heartbeat_replacement_is_drift_pinned_and_metadata_preserving() {
    for required in [
        "51a3dcd58a44f60320a0bcb5b671fce0eeffa10d08e0a5b8c3880a07ce1802b3",
        "37e1714c04f1ca66f6b12d571098fff12c31d2bb2aa55355c90c0db417f021de",
        "pg_catalog.to_regprocedure(function_identity)",
        "pg_catalog.pg_get_functiondef(function_row.oid)",
        "observed_definition_digest <> old_definition_digest",
        "metadata_after IS DISTINCT FROM metadata_before",
        "observed_definition_digest <> expected_definition_digest",
        "'oid', function_row.oid::TEXT",
        "'owner', function_row.proowner::TEXT",
        "'acl', pg_catalog.to_jsonb(function_row.proacl)",
        "'language', function_row.prolang::TEXT",
        "'kind', function_row.prokind",
        "'volatile', function_row.provolatile",
        "'strict', function_row.proisstrict",
        "'security_definer', function_row.prosecdef",
        "'parallel', function_row.proparallel",
        "'returns_set', function_row.proretset",
        "'rows', function_row.prorows",
        "'config', pg_catalog.to_jsonb(function_row.proconfig)",
        "'leakproof', function_row.proleakproof",
        "'argument_defaults', function_row.pronargdefaults",
        "'variadic', function_row.provariadic::TEXT",
        "'return_type', function_row.prorettype::TEXT",
    ] {
        assert!(HEARTBEAT_MIGRATION.contains(required), "{required}");
    }
    assert_eq!(
        HEARTBEAT_MIGRATION
            .matches("EXECUTE function_definition")
            .count(),
        1
    );
    assert!(!HEARTBEAT_MIGRATION.contains("CREATE FUNCTION public."));
    assert!(!HEARTBEAT_MIGRATION.contains("CREATE OR REPLACE FUNCTION public."));
}

#[test]
fn serving_manifest_and_readiness_are_refreshed() {
    for required in [
        "a4d366aad6c6e320697b90f4e294ca6dfff9cceb1a1935e8d12f0608614eda02",
        "5b233dbcde74bd23f15a54f3af318aebadb5487928c16c2b09c770191108ab03",
        "90ab51452bf5c3ba8074e0bce0f6a643ba374e79497962d0bf2d5aeec062fa96",
        "a18ac0ef4c1275601d9b195ccb601e1982d97ba89fe09a83b8818db3fe126d35",
        "OR NOT public.starring_runtime_serving_schema_manifest_v1()",
    ] {
        assert!(MANIFEST_MIGRATION.contains(required), "{required}");
    }
    for required in [
        "90ab51452bf5c3ba8074e0bce0f6a643ba374e79497962d0bf2d5aeec062fa96",
        "a18ac0ef4c1275601d9b195ccb601e1982d97ba89fe09a83b8818db3fe126d35",
        "1d7bb5b18129f99ef87b5ad0dfe712b4e6beac33a0461218fedf67fa6990ac3b",
        "e598fb40785ccd66ce44ec6c7f85e52fd9e004ab1e05de9c0c03963f06df45f1",
        "OR NOT public.starring_runtime_serving_schema_manifest_v1()",
    ] {
        assert!(READINESS_MIGRATION.contains(required), "{required}");
    }
}

#[test]
fn migrations_are_bounded_and_comment_free() {
    for migration in [HEARTBEAT_MIGRATION, MANIFEST_MIGRATION, READINESS_MIGRATION] {
        assert!(migration.contains("SET LOCAL lock_timeout = '5s'"));
        assert!(migration.contains("SET LOCAL statement_timeout = '30s'"));
        assert!(migration
            .ends_with("RESET search_path;\nRESET statement_timeout;\nRESET lock_timeout;\n"));
        assert!(!migration.contains("--"));
        assert!(!migration.contains("/*"));
        assert!(!migration.contains("//"));
    }
}
