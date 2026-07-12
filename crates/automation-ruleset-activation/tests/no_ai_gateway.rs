#[test]
fn manifest_does_not_depend_on_ai_gateway() {
    let manifest = include_str!("../Cargo.toml");
    let regular = manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(manifest);
    assert!(!regular.contains("ai-gateway"));
}
