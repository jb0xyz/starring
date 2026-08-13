#[test]
fn regular_dependencies_keep_the_stateful_spec_crate_pure() {
    let manifest = include_str!("../Cargo.toml");
    let regular = manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(manifest);
    for forbidden in ["ai-gateway", "ai_gateway", "llm", "sqlx", "twilight"] {
        assert!(
            !regular.contains(forbidden),
            "forbidden regular dependency: {forbidden}"
        );
    }
}
