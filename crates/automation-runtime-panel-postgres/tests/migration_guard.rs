#[test]
fn runtime_panel_capabilities_are_private_and_schema_pinned() {
    let migration =
        include_str!("../../../migrations/202607220022_fence_runtime_panel_reconciliation.sql");
    for function in [
        "starring_runtime_panel_execution_lock_v1",
        "starring_runtime_panel_reconciliation_lock_v1",
        "starring_runtime_panel_reconciliation_claim_v1",
        "starring_runtime_panel_reconciliation_check_v1",
        "starring_runtime_panel_reconciliation_snapshot_v1",
        "starring_runtime_panel_reconciliation_installation_upsert_v1",
        "starring_runtime_panel_reconciliation_installation_remove_v1",
        "starring_runtime_panel_reconciliation_journal_put_v1",
        "starring_runtime_panel_reconciliation_journal_remove_v1",
    ] {
        assert!(migration.contains(&format!("CREATE FUNCTION public.{function}(")));
    }
    assert_eq!(
        migration.matches("SECURITY DEFINER").count(),
        migration.matches("SET search_path = pg_catalog").count()
    );
    assert!(migration.contains("REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE"));
    assert!(!migration.contains("CREATE ROLE"));
    assert!(!migration.contains("GRANT "));
}

#[test]
fn runtime_panel_capabilities_preserve_fences_and_cas() {
    let migration =
        include_str!("../../../migrations/202607220022_fence_runtime_panel_reconciliation.sql");
    for state in ["RP001", "RP002", "RP003", "RP004", "RP005"] {
        assert!(migration.contains(&format!("ERRCODE = '{state}'")));
    }
    for invariant in [
        "deployment_row.phase <> 'reconciling_panels'",
        "deployment_row.controller_fencing_token",
        "deployment_row.convergence_attempt_no",
        "deployment_row.runtime_generation",
        "authority_status IS DISTINCT FROM 'exact'",
        "session_row.session_record_revision",
        "requested_session_id TEXT",
        "expected_record_revision = 0",
        "session.next_record_revision + 1",
        "required_lease_headroom_ms NOT BETWEEN 1 AND 30000",
    ] {
        assert!(
            migration.contains(invariant),
            "missing invariant: {invariant}"
        );
    }
    assert!(migration.contains("ROWS 512"));
    assert!(migration.contains("resident_count > 256"));
}

#[test]
fn migration_and_adapter_contract_names_match() {
    let migration =
        include_str!("../../../migrations/202607220022_fence_runtime_panel_reconciliation.sql");
    let contract = include_str!("../src/contract.rs");
    for function in [
        "starring_runtime_panel_reconciliation_claim_v1",
        "starring_runtime_panel_reconciliation_check_v1",
        "starring_runtime_panel_reconciliation_snapshot_v1",
        "starring_runtime_panel_reconciliation_installation_upsert_v1",
        "starring_runtime_panel_reconciliation_installation_remove_v1",
        "starring_runtime_panel_reconciliation_journal_put_v1",
        "starring_runtime_panel_reconciliation_journal_remove_v1",
    ] {
        assert!(migration.contains(function));
        assert!(contract.contains(function));
    }
    let expected_parameters = [17_usize, 19, 18, 25, 20, 23, 20];
    for (query, expected) in contract
        .split("pub(crate) const ")
        .skip(3)
        .zip(expected_parameters)
    {
        let maximum = query
            .split(';')
            .next()
            .unwrap()
            .split('$')
            .skip(1)
            .filter_map(|part| {
                part.chars()
                    .take_while(|character| character.is_ascii_digit())
                    .collect::<String>()
                    .parse::<usize>()
                    .ok()
            })
            .max()
            .unwrap();
        assert_eq!(maximum, expected);
    }
}

#[test]
fn runtime_panel_database_readiness_is_private_and_schema_pinned() {
    let migration =
        include_str!("../../../migrations/202607220024_scope_runtime_panel_database.sql");
    assert!(
        migration.contains("CREATE FUNCTION public.starring_runtime_panel_database_readiness_v1()")
    );
    assert!(
        migration.contains("CREATE FUNCTION public.starring_runtime_panel_database_identity_v1()")
    );
    assert!(migration.contains("SECURITY DEFINER"));
    assert!(migration.contains("SET search_path = pg_catalog"));
    assert!(migration.contains("REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE"));
    assert!(migration.contains("pg_get_function_identity_arguments(function_row.oid) <> ''"));
    assert!(migration.contains(
        "TABLE(database_identity text, database_name text, executor_role text, checked_at timestamp with time zone)"
    ));
    assert!(!migration.contains("CREATE ROLE"));
    assert!(!migration.contains("\nGRANT "));
}

#[test]
fn runtime_panel_database_readiness_fails_closed_on_capability_drift() {
    let migration =
        include_str!("../../../migrations/202607220024_scope_runtime_panel_database.sql");
    for invariant in [
        "pg_catalog.current_setting('role') <> 'none'",
        "role_row.rolinherit",
        "membership.member = invoker_oid",
        "membership.roleid = invoker_oid",
        "role_row.rolconfig",
        "role_row.rolconnlimit NOT BETWEEN 1 AND 4",
        "pg_catalog.pg_db_role_setting",
        "pg_catalog.current_setting('server_version_num')::INTEGER / 10000 <> 16",
        "pg_catalog.has_database_privilege",
        "database_row.datacl",
        "'TEMPORARY'",
        "pg_catalog.has_schema_privilege(invoker_oid, 'public', 'CREATE')",
        "pg_catalog.has_table_privilege(invoker_oid, relation.oid, 'SELECT')",
        "pg_catalog.has_sequence_privilege(invoker_oid, sequence.oid, 'USAGE')",
        "function_row.proconfig IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]",
        "privilege.grantee = 0",
        "privilege.is_grantable",
        "pg_catalog.pg_default_acl",
        "pg_catalog.pg_parameter_acl",
        "pg_catalog.pg_init_privs",
        "'pg_catalog.pg_namespace'::REGCLASS",
        "'pg_catalog.pg_class'::REGCLASS",
        "'pg_catalog.pg_proc'::REGCLASS",
        "'pg_catalog.pg_type'::REGCLASS",
        "pg_catalog.pg_attribute",
        "pg_catalog.pg_type",
        "function_row.oid >= 16384",
        "type_row.oid >= 16384",
        "initial.objsubid = attribute.attnum",
        "sequence.relkind = 'S'",
        "type_row.typacl",
        "pg_catalog.pg_largeobject_metadata",
        "large_object.lomowner = invoker_oid",
        "large_object.lomacl",
        "privilege.privilege_type IN ('SET', 'ALTER SYSTEM')",
        "namespace.nspname <> 'information_schema'",
        "runtime_panel_database_capability_drift",
        "runtime_panel_database_schema_drift",
    ] {
        assert!(
            migration.contains(invariant),
            "missing invariant: {invariant}"
        );
    }
    assert!(!migration.contains(
        "function_row.prosecdef\n                    OR pg_catalog.left(function_row.proname"
    ));
    assert!(!migration.contains("function_row.prokind IN ('f', 'p')"));

    let manifest = migration
        .split_once(") IS DISTINCT FROM ARRAY[")
        .unwrap()
        .1
        .split_once("]::TEXT[]")
        .unwrap()
        .0;
    let names = manifest
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix('\'')
                .and_then(|line| line.split_once('\'').map(|(name, _)| name))
        })
        .collect::<Vec<_>>();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names.len(), 62);
    assert_eq!(names, sorted);
    for private_relation in [
        "_pg_foreign_servers",
        "_pg_user_mappings",
        "sql_parts",
        "transforms",
    ] {
        assert!(!names.contains(&private_relation));
    }
}

#[test]
fn runtime_panel_database_migration_and_adapter_contract_match() {
    let migration =
        include_str!("../../../migrations/202607220024_scope_runtime_panel_database.sql");
    let contract = include_str!("../src/contract.rs");
    assert!(migration.contains("starring_runtime_panel_database_readiness_v1"));
    assert!(contract.contains("starring_runtime_panel_database_readiness_v1"));
    assert!(migration.contains("starring_runtime_panel_database_identity_v1"));
    assert!(contract.contains("starring_runtime_panel_database_identity_v1"));
}

#[test]
fn runtime_panel_migration_contains_no_comments() {
    for migration in [
        include_str!("../../../migrations/202607220022_fence_runtime_panel_reconciliation.sql"),
        include_str!("../../../migrations/202607220024_scope_runtime_panel_database.sql"),
    ] {
        for line in migration.lines() {
            let trimmed = line.trim_start();
            assert!(!trimmed.starts_with("--"));
            assert!(!trimmed.starts_with("/*"));
        }
    }
}
