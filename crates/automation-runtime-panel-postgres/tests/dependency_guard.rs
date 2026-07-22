#[test]
fn adapter_keeps_discord_and_generic_postgres_edges_out() {
    let manifest = include_str!("../Cargo.toml");
    let regular = manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(manifest);
    for forbidden in [
        "ai-gateway",
        "authoring-application",
        "automation-panel-installation-postgres",
        "automation-runtime =",
        "twilight",
        "axum",
    ] {
        assert!(
            !regular.contains(forbidden),
            "forbidden dependency: {forbidden}"
        );
    }
}

#[test]
fn pure_crates_do_not_depend_on_runtime_panel_postgres() {
    for manifest in [
        include_str!("../../automation-panel-installation/Cargo.toml"),
        include_str!("../../automation-runtime-controller/Cargo.toml"),
        include_str!("../../automation-runtime-convergence/Cargo.toml"),
    ] {
        let regular = manifest
            .split("[dev-dependencies]")
            .next()
            .unwrap_or(manifest);
        assert!(!regular.contains("automation-runtime-panel-postgres"));
        assert!(!regular.contains("sqlx"));
    }
}

#[test]
fn adapter_sources_contain_no_comments() {
    let sources = [
        include_str!("../src/adapter.rs"),
        include_str!("../src/authority.rs"),
        include_str!("../src/contract.rs"),
        include_str!("../src/database.rs"),
        include_str!("../src/error.rs"),
        include_str!("../src/lib.rs"),
        include_str!("../src/reconcile.rs"),
        include_str!("../src/row.rs"),
        include_str!("../src/session.rs"),
        include_str!("../src/store.rs"),
    ];
    for source in sources {
        for line in source.lines() {
            let trimmed = line.trim_start();
            assert!(!trimmed.starts_with("//"));
            assert!(!trimmed.starts_with("/*"));
            assert!(!trimmed.starts_with('*'));
        }
    }
}

#[test]
fn verified_adapter_keeps_the_pool_private() {
    let adapter = include_str!("../src/adapter.rs");
    for required in [
        "PostgresRuntimePanelV1",
        "verify_runtime_panel_database_with_timeouts_v1",
        "PostgresFencedStrictPanelStoreV1::claim_with_timeouts",
        "PostgresFencedStrictPanelStoreV1::claim_with_session_id_and_timeouts",
    ] {
        assert!(adapter.contains(required), "{required}");
    }
    for forbidden in ["pub fn pool", "pub fn connection", "pub fn connect_options"] {
        assert!(!adapter.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn adapter_contract_uses_only_private_capabilities() {
    let contract = include_str!("../src/contract.rs");
    for capability in [
        "starring_runtime_panel_database_readiness_v1",
        "starring_runtime_panel_reconciliation_claim_v1",
        "starring_runtime_panel_reconciliation_check_v1",
        "starring_runtime_panel_reconciliation_snapshot_v1",
        "starring_runtime_panel_reconciliation_installation_upsert_v1",
        "starring_runtime_panel_reconciliation_installation_remove_v1",
        "starring_runtime_panel_reconciliation_journal_put_v1",
        "starring_runtime_panel_reconciliation_journal_remove_v1",
    ] {
        assert!(contract.contains(capability));
    }
    for forbidden in [
        "runtime_deployments",
        "ruleset_panel_installations",
        "strict_panel_operation_journal",
        "INSERT ",
        "UPDATE ",
        "DELETE ",
    ] {
        assert!(!contract.contains(forbidden));
    }
}

#[test]
fn adapter_database_transactions_are_bounded_in_canonical_order() {
    let database = include_str!("../src/database.rs");
    let statement = database
        .find("pg_catalog.set_config('statement_timeout'")
        .unwrap();
    let lock = database
        .find("pg_catalog.set_config('lock_timeout'")
        .unwrap();
    let idle = database
        .find("pg_catalog.set_config('idle_in_transaction_session_timeout'")
        .unwrap();
    let search_path = database
        .find("pg_catalog.set_config('search_path'")
        .unwrap();
    assert!(statement < lock && lock < idle && idle < search_path);
    assert!(database.contains("MAX_RUNTIME_PANEL_DATABASE_TIMEOUT"));
    assert!(database.contains("begin_panel_transaction"));
}
