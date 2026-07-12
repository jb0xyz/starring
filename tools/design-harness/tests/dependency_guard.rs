#[test]
fn runtime_dependencies_are_edge_only() {
    let manifest = include_str!("../Cargo.toml");

    for forbidden in ["twilight", "sqlx", "automation-ruleset", "postgres"] {
        assert!(
            !manifest.contains(forbidden),
            "forbidden dependency: {forbidden}"
        );
    }
}
