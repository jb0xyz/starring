#[test]
fn runtime_dependencies_exclude_live_and_persistent_edges() {
    let manifest = include_str!("../Cargo.toml");
    let runtime = manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(manifest);

    for forbidden in [
        "ai-gateway",
        "twilight",
        "sqlx",
        "rusqlite",
        "sqlite",
        "sled",
        "redb",
        "automation-ruleset",
        "automation-instance-postgres",
        "automation-panel-installation-postgres",
        "automation-ruleset-activation-postgres",
    ] {
        assert!(
            !runtime.contains(forbidden),
            "forbidden dependency: {forbidden}"
        );
    }
}

#[test]
fn manifest_defines_library_only() {
    let manifest = include_str!("../Cargo.toml");
    assert!(!manifest.contains("[[bin]]"));
}
