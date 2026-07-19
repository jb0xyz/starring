const SOURCE: &str = include_str!("../src/main.rs");
const MANIFEST: &str = include_str!("../Cargo.toml");

fn has_direct_activation_bypass(source: &str) -> bool {
    source.contains("activate_if_ready(")
        || source.contains(".activate(")
        || source.contains(".activate_guarded(")
}

#[test]
fn production_source_has_no_direct_activation_bypass() {
    assert!(!has_direct_activation_bypass(SOURCE));
    assert!(!SOURCE.contains("Command::Activate"));
    assert!(!SOURCE.contains("force_activate"));
}

#[test]
fn activation_bypass_guard_detects_direct_calls() {
    assert!(has_direct_activation_bypass(
        "activate_if_ready(store, guild, key)"
    ));
    assert!(has_direct_activation_bypass(
        "ruleset_store.activate(guild, key, version)"
    ));
    assert!(has_direct_activation_bypass(
        "ruleset_store.activate_guarded(request)"
    ));
}

#[test]
fn recovery_and_migration_are_wired_before_gateway() {
    let scope_gate = SOURCE
        .rfind("require_legacy_smoke_scope(&database_url)?")
        .unwrap();
    let pool_connect = SOURCE.find("sqlx::PgPool::connect(&database_url)").unwrap();
    let migration = SOURCE
        .find("automation_ruleset_activation_postgres::MIGRATOR")
        .unwrap();
    let slot_guard = SOURCE
        .rfind(
            "reject_product_managed_command(&pool, guild_id, &ruleset_key, &parsed.command).await?",
        )
        .unwrap();
    let command_dispatch = SOURCE.find("match parsed.command").unwrap();
    let recovery = SOURCE.find("recover_applying(").unwrap();
    let gateway = SOURCE.find("gateway::run(").unwrap();
    assert!(scope_gate < pool_connect);
    assert!(migration < slot_guard);
    assert!(slot_guard < command_dispatch);
    assert!(migration < recovery);
    assert!(recovery < gateway);
    assert!(SOURCE.contains("STARRING_ALLOW_INTERACTION_SMOKE=1"));
    assert!(SOURCE.contains("requires the legacy-smoke build feature"));
    assert!(MANIFEST.contains("legacy-smoke = []"));
    assert!(SOURCE.contains("strict Starring test database namespace"));
    assert!(SOURCE.contains("automation_installations"));
    assert!(SOURCE.contains("interaction-smoke cannot access a product-managed RuleSet slot"));
    assert!(SOURCE.contains("activation recovery failed; refusing startup"));
    assert!(SOURCE.contains("activation recovery remained unresolved; refusing startup"));
    assert!(!SOURCE.contains("activation recovery list failed; continuing startup"));
}
