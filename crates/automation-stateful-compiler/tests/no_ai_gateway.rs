#[test]
fn compiler_has_no_ai_gateway_dependency() {
    let manifest = include_str!("../Cargo.toml");
    assert!(!manifest.contains("ai-gateway"));
    assert!(!manifest.contains("reqwest"));
    assert!(!manifest.contains("tokio"));
}
