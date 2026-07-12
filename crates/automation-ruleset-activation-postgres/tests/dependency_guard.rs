#[test]
fn core_does_not_depend_on_postgres_adapter() {
    let manifest = include_str!("../../automation-ruleset-activation/Cargo.toml");
    let regular = manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(manifest);
    assert!(!regular.contains("automation-ruleset-activation-postgres"));
    assert!(!regular.contains("sqlx"));
}
