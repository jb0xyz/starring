#[test]
fn no_ai_gateway() {
    assert!(!include_str!("../Cargo.toml").contains("ai-gateway"));
}
