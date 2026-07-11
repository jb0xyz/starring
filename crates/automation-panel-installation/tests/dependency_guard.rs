#[test]
fn runtime_dependencies_stay_pure() {
    let manifest = include_str!("../Cargo.toml");
    let runtime_deps = manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(manifest);
    assert!(!runtime_deps.contains("sqlx"));
    assert!(!runtime_deps.contains("twilight"));
    assert!(!runtime_deps.contains("automation-ruleset-readiness"));
    assert!(!runtime_deps.contains("automation-panel-installation-postgres"));
}

#[test]
fn reverse_dependencies_stay_absent() {
    for manifest in [
        include_str!("../../automation-state/Cargo.toml"),
        include_str!("../../automation-ruleset/Cargo.toml"),
        include_str!("../../automation-ruleset-readiness/Cargo.toml"),
    ] {
        assert!(!manifest.contains("automation-panel-installation"));
    }
}
