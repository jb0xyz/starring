const MIGRATION: &str =
    include_str!("../../../migrations/202608030001_fix_pending_drain_live_source_selection_v2.sql");
const CONTRACT_SOURCE: &str = include_str!("../src/contract.rs");
const DATABASE_SOURCE: &str = include_str!("../src/database.rs");
const SECURITY_SUPPORT_SOURCE: &str = include_str!("postgres_security/support.rs");

fn dollar_block(tag: &str) -> &'static str {
    MIGRATION
        .split(&format!("DO ${tag}$"))
        .nth(1)
        .unwrap()
        .split(&format!("${tag}$;"))
        .next()
        .unwrap()
}

#[test]
fn pending_drain_live_source_selection_is_bounded_forward_only_and_comment_free() {
    assert!(MIGRATION.starts_with("SET LOCAL lock_timeout = '5s';"));
    assert!(MIGRATION.ends_with("RESET lock_timeout;\n"));
    assert!(MIGRATION.contains("starring-runtime-writer-fence-v1"));
    assert!(MIGRATION.contains("IN ACCESS SHARE MODE;"));
    for line in MIGRATION.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"));
        assert!(!trimmed.starts_with("//"));
        assert!(!trimmed.starts_with("/*"));
    }
    for forbidden in [
        "GRANT ",
        "REVOKE ",
        "DROP ",
        "DELETE FROM",
        "UPDATE public.",
        "INSERT INTO public.",
        "CREATE FUNCTION public.",
    ] {
        assert!(!MIGRATION.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn pending_drain_live_source_selection_patches_both_priority_checks_exactly() {
    let patch = dollar_block("patch_priority");
    for required in [
        "starring_runtime_private_v2.starring_runtime_pending_drain_candidate_v2()",
        "public.starring_runtime_startup_recovery_execute_pending_drain_v2(text,bigint",
        "WHERE deployment.phase = ''live''",
        "INNER JOIN public.runtime_slot_writer_fences_v2 AS slot",
        "slot.pending_drain_intent_id =",
        "drain.drain_intent_id",
        "slot.pending_product_operation_id =",
        "drain.product_operation_id",
        "slot.pending_tenant_id = drain.tenant_id",
        "slot.pending_installation_id =",
        "drain.installation_id",
        "slot.pending_deployment_id =",
        "drain.deployment_id",
        "slot.pending_expected_revision =",
        "drain.expected_revision",
        "drain.tenant_id = deployment.tenant_id",
        "drain.installation_id =",
        "deployment.installation_id",
        "drain.deployment_id = deployment.deployment_id",
        "drain.slot_guild_id = deployment.guild_id",
        "drain.slot_ruleset_key =",
        "deployment.ruleset_key",
        "''route_absent_acknowledged''",
        "metadata_after IS DISTINCT FROM metadata_before",
        "observed_definition_digest <> expected_after_digest",
    ] {
        assert!(patch.contains(required), "{required}");
    }
    assert_eq!(patch.matches("FOREACH identity").count(), 1);
    assert_eq!(patch.matches("expected_before_digest :=").count(), 2);
    assert_eq!(patch.matches("expected_after_digest :=").count(), 2);
    assert!(!patch.contains("drain.expected_revision = deployment.revision"));
}

#[test]
fn pending_drain_live_source_selection_digest_chain_is_exact_and_current() {
    for required in [
        "91d4d64ae0f1b3053ec91f1c1b07164fce08311e26b58718eca672f3fadee909",
        "43889d46cada8cb79b0474e1db761f32eac8a68ea6662886db0439e5315cef2a",
        "5414574cde39e1c59410e1cac6ccb975a87d16f4807f3ae33b8f28b8157a8e9b",
        "6289257130f1327b0f5378b5ad998899de26217ecca62c421a5f3ba8257e38e6",
        "1d85a38b5d30b20a4b15c6adc70af3e08ea66901465ba83b2d2bf8d200ccbfca",
        "4d9eb1fdaa4eac009105ab65b9115e523f52b1128cde4ea3ebcc85f006ea08b9",
        "2ee6db433ac8976c754c1566b39eb17950d8c9e1a9e5e56d6d96e45a39342dc7",
        "99dfc39ef03194161fe67419d87fd2890145980f3147151864ea7552bec36886",
        "0e69552f26e09949d44b87c7ae7680432ff2c36a0027230efcf541cc4324cd9f",
        "98ed1251e3339ffb452ed12334699e93f43e2ea3cd7d327bc3d2a11fe12b9fb2",
        "applied_count <> 119",
        "applied_head <> 202608020002",
        "61ac941862c11f0aaa3cce54a2842ffadf4e5897c39f6796d2c6874e987a9f1e9d4ba6dd3dbc332f569c20d25831d769",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    for source in [CONTRACT_SOURCE, DATABASE_SOURCE, SECURITY_SUPPORT_SOURCE] {
        assert!(source.contains("98ed1251e3339ffb452ed12334699e93f43e2ea3cd7d327bc3d2a11fe12b9fb2"));
    }
}

#[test]
fn pending_drain_live_source_selection_preserves_function_metadata_and_manifests() {
    let patch = dollar_block("patch_priority");
    for required in [
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
        "'all_argument_types'",
        "'argument_modes'",
        "'argument_names'",
    ] {
        assert!(patch.contains(required), "{required}");
    }
    let postflight = dollar_block("postflight");
    for required in [
        "public.starring_runtime_execution_schema_manifest_v1()",
        "public.starring_runtime_execution_database_readiness_v1()",
        "public.starring_runtime_exact_target_schema_manifest_v1()",
        "public.starring_runtime_exact_target_schema_manifest_v2()",
        "public.starring_runtime_serving_schema_manifest_v1()",
        "public.starring_runtime_interaction_schema_manifest_v1()",
        "43889d46cada8cb79b0474e1db761f32eac8a68ea6662886db0439e5315cef2a",
        "6289257130f1327b0f5378b5ad998899de26217ecca62c421a5f3ba8257e38e6",
        "99dfc39ef03194161fe67419d87fd2890145980f3147151864ea7552bec36886",
        "98ed1251e3339ffb452ed12334699e93f43e2ea3cd7d327bc3d2a11fe12b9fb2",
    ] {
        assert!(postflight.contains(required), "{required}");
    }
}
