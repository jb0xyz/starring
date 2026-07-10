#[test]
fn core_manifest_does_not_depend_on_ai_gateway() {
    let manifest = include_str!("../Cargo.toml");
    assert!(
        !manifest.contains("ai-gateway"),
        "automation-core must not depend on ai-gateway; event-time AI is forbidden"
    );
    assert!(!manifest.contains("ai_gateway"));
    assert!(!manifest.contains("llm"));
}
