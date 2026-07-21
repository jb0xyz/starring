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
        include_str!("../src/authority.rs"),
        include_str!("../src/contract.rs"),
        include_str!("../src/error.rs"),
        include_str!("../src/lib.rs"),
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
fn adapter_contract_uses_only_private_capabilities() {
    let contract = include_str!("../src/contract.rs");
    for capability in [
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
