#[test]
fn manifest_does_not_depend_on_ai_gateway() {
    let manifest = include_str!("../Cargo.toml");
    assert!(!manifest.contains("ai-gateway"));
    assert!(!manifest.contains("ai_gateway"));
    assert!(!manifest.contains("llm"));
}
