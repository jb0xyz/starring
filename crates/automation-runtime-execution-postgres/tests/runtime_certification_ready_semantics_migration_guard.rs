const PREVIOUS_MIGRATION: &str =
    include_str!("../../../migrations/202607300003_finalize_runtime_certification_v2.sql");
const MIGRATION: &str = include_str!(
    "../../../migrations/202607310004_correct_runtime_certification_ready_semantics.sql"
);
const MANIFEST_MIGRATION: &str = include_str!(
    "../../../migrations/202607310005_refresh_runtime_certification_schema_manifest.sql"
);
const READINESS_MIGRATION: &str = include_str!(
    "../../../migrations/202607310006_refresh_runtime_certification_readiness_pin.sql"
);
const EXECUTION_CONTRACT: &str = include_str!("../src/contract.rs");
const EXECUTION_DATABASE: &str = include_str!("../src/database.rs");
const POSTGRES_SECURITY_SUPPORT: &str = include_str!("postgres_security/support.rs");

#[test]
fn previous_certification_migration_remains_immutable() {
    for original in [
        "OR route_record #>> '{gateway,kind}' IS DISTINCT FROM 'resumed'",
        "OR deployment_row.snapshot -> 'gateway_ready' IS NULL",
        "OR deployment_row.snapshot -> 'gateway_ready' = 'null'::JSONB",
        "'gateway_ready', deployment_row.snapshot -> 'gateway_ready'",
        "'discord_resumed',",
    ] {
        assert!(PREVIOUS_MIGRATION.contains(original), "{original}");
    }
}

#[test]
fn replacement_is_exact_and_preserves_function_metadata() {
    for required in [
        "pg_catalog.to_regprocedure(function_identity)",
        "pg_catalog.pg_get_functiondef(function_oid)",
        "pg_catalog.char_length(function_definition)",
        "pg_catalog.char_length(pg_catalog.replace(",
        "<> pg_catalog.char_length(previous_fragment)",
        "EXECUTE function_definition",
        "'oid', function_row.oid::TEXT",
        "'owner', function_row.proowner::TEXT",
        "'acl', pg_catalog.to_jsonb(function_row.proacl)",
        "'language', function_row.prolang::TEXT",
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
        "metadata_after IS DISTINCT FROM metadata_before",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert_eq!(
        MIGRATION
            .matches("<> pg_catalog.char_length(previous_fragment)")
            .count(),
        7
    );
    assert_eq!(MIGRATION.matches("EXECUTE function_definition").count(), 1);
    assert!(!MIGRATION.contains("CREATE FUNCTION public.starring_runtime_certification_commit_v2"));
}

#[test]
fn replacement_projects_ready_evidence_without_conflating_resume_events() {
    for required in [
        "COALESCE(",
        "route_record #>> ''{gateway,kind}''",
        ") NOT IN (''ready'', ''resumed'')",
        "deployment_row.snapshot -> ''gateway_ready''",
        "IS DISTINCT FROM ''null''::JSONB",
        "gateway_ready_kind := CASE route_record #>> ''{gateway,kind}''",
        "WHEN ''ready'' THEN ''discord_ready''",
        "WHEN ''resumed'' THEN ''discord_resumed''",
        "gateway_ready_value := pg_catalog.jsonb_build_object(",
        "''target'', intent -> ''target''",
        "route_record #>> ''{gateway,process_instance_id}''",
        "''kind'', gateway_ready_kind",
        "''ready_at'', pg_catalog.to_jsonb(database_now)",
        "''gateway_ready'', gateway_ready_value",
        "''{gateway_ready}''",
        "gateway_ready_value",
        "gateway_ready_kind",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert!(MIGRATION.contains("route_record #>> ''{gateway,kind}'' IS DISTINCT FROM ''resumed'''"));
    assert!(MIGRATION.contains(
        "pg_catalog.strpos(\n            function_definition,\n            'route_record #>>"
    ));
}

#[test]
fn migration_scope_is_bounded_and_comment_free() {
    assert!(MIGRATION.contains("SET LOCAL lock_timeout = '5s'"));
    assert!(MIGRATION.contains("SET LOCAL statement_timeout = '30s'"));
    assert!(
        MIGRATION.ends_with("RESET search_path;\nRESET statement_timeout;\nRESET lock_timeout;\n")
    );
    assert!(!MIGRATION.contains("schema_manifest"));
    assert!(!MIGRATION.contains("database_readiness"));
    assert!(!MIGRATION.contains("--"));
    assert!(!MIGRATION.contains("/*"));
    assert!(!MIGRATION.contains("//"));
}

#[test]
fn execution_manifest_refresh_is_exact_and_self_verifying() {
    for required in [
        "public.starring_runtime_execution_schema_manifest_v1()",
        "RETURN observed_count = 969",
        "462956974d1b225413ead6da18003d29032b267d8d6d86c341d82ba8030a05b6",
        "ec41d06fbdfce734b673f6e4e7864e428fb153af992c4f3c395a0eb1cd2106a4",
        "6731f361eb37f170d4cdb91a1c5931101ef6bc2d16c50e1114a452e05b228f7b",
        "pg_catalog.pg_get_functiondef(function_oid)",
        "EXECUTE pg_catalog.replace(",
        "metadata_after IS DISTINCT FROM metadata_before",
        "observed_definition_digest <> expected_definition_digest",
        "OR NOT public.starring_runtime_execution_schema_manifest_v1()",
    ] {
        assert!(MANIFEST_MIGRATION.contains(required), "{required}");
    }
    assert_eq!(
        MANIFEST_MIGRATION
            .matches("RETURN observed_count = 969")
            .count(),
        2
    );
    assert_eq!(
        MANIFEST_MIGRATION
            .matches("EXECUTE pg_catalog.replace(")
            .count(),
        1
    );
}

#[test]
fn execution_readiness_refresh_is_exact_and_pinned_everywhere() {
    let previous_manifest_definition =
        "4e61e3e8a9769f8ef9d1c68bde97cde6be5fb80d54cc5cba1aa5999e34d83bfa";
    let current_manifest_definition =
        "6731f361eb37f170d4cdb91a1c5931101ef6bc2d16c50e1114a452e05b228f7b";
    let previous_readiness_definition =
        "fc2b7bceeb3e9b9fc98335c3c358652e76b3f13edf87bbbb2506b62de3577e0a";
    let current_readiness_definition =
        "437eef0962f31be61e9fcb2f6705b2cda14f4d52105ae024ca4bc29b967e001c";
    for required in [
        "public.starring_runtime_execution_database_readiness_v1()",
        previous_manifest_definition,
        current_manifest_definition,
        previous_readiness_definition,
        "pg_catalog.pg_get_functiondef(function_oid)",
        "EXECUTE pg_catalog.replace(",
        "metadata_after IS DISTINCT FROM metadata_before",
        "observed_definition_digest <> expected_definition_digest",
    ] {
        assert!(READINESS_MIGRATION.contains(required), "{required}");
    }
    assert_eq!(
        READINESS_MIGRATION
            .matches("EXECUTE pg_catalog.replace(")
            .count(),
        1
    );
    for current_source in [
        EXECUTION_CONTRACT,
        EXECUTION_DATABASE,
        POSTGRES_SECURITY_SUPPORT,
    ] {
        assert!(current_source.contains(current_readiness_definition));
    }
}

#[test]
fn contract_refresh_migrations_are_bounded_and_comment_free() {
    for migration in [MANIFEST_MIGRATION, READINESS_MIGRATION] {
        assert!(migration.contains("SET LOCAL lock_timeout = '5s'"));
        assert!(migration.contains("SET LOCAL statement_timeout = '30s'"));
        assert!(migration.contains("'oid', function_row.oid::TEXT"));
        assert!(migration.contains("'owner', function_row.proowner::TEXT"));
        assert!(migration.contains("'acl', pg_catalog.to_jsonb(function_row.proacl)"));
        assert!(migration.contains("'language', function_row.prolang::TEXT"));
        assert!(migration.contains("'return_type', function_row.prorettype::TEXT"));
        assert!(migration
            .ends_with("RESET search_path;\nRESET statement_timeout;\nRESET lock_timeout;\n"));
        assert!(!migration.contains("--"));
        assert!(!migration.contains("/*"));
        assert!(!migration.contains("//"));
    }
}
