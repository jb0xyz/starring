#[test]
fn core_ruleset_crate_stays_pure() {
    let manifest = include_str!("../../automation-ruleset/Cargo.toml");
    assert!(!manifest.contains("sqlx"));
    assert!(!manifest.contains("automation-ruleset-postgres"));
}
