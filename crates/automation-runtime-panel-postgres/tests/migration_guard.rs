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
        .skip(1)
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
fn runtime_panel_migration_contains_no_comments() {
    let migration =
        include_str!("../../../migrations/202607220022_fence_runtime_panel_reconciliation.sql");
    for line in migration.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"));
        assert!(!trimmed.starts_with("/*"));
    }
}
