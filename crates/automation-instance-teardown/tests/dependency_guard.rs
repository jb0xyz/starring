#[test]
fn runtime_dependencies_stay_pure() {
    let manifest = include_str!("../Cargo.toml");
    let runtime_deps = manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(manifest);
    assert!(!runtime_deps.contains("sqlx"));
    assert!(!runtime_deps.contains("twilight"));
    assert!(!runtime_deps.contains("automation-runtime"));
}
