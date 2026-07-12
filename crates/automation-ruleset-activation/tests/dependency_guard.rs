#[test]
fn regular_dependencies_stay_pure() {
    let manifest = include_str!("../Cargo.toml");
    let regular = manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(manifest);
    for forbidden in [
        "sqlx",
        "twilight",
        "approval-manager",
        "policy-engine",
        "preview",
        "automation-ruleset-dispatch",
    ] {
        assert!(!regular.contains(forbidden));
    }
}
