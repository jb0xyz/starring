const SOURCE: &str = include_str!("../src/main.rs");

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
    let migration = SOURCE
        .find("automation_ruleset_activation_postgres::MIGRATOR")
        .unwrap();
    let recovery = SOURCE.find("recover_applying(").unwrap();
    let gateway = SOURCE.find("gateway::run(").unwrap();
    assert!(migration < recovery);
    assert!(recovery < gateway);
}
