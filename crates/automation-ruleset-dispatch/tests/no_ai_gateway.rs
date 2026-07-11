#[test]
fn dispatch_crate_runtime_deps_are_pure() {
    let manifest = include_str!("../Cargo.toml");
    let runtime_deps = manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(manifest);
    assert!(!runtime_deps.contains("ai-gateway"));
    assert!(!runtime_deps.contains("twilight"));
    assert!(!runtime_deps.contains("sqlx"));
}
