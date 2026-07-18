#[test]
fn core_does_not_depend_on_postgres_adapter() {
    let manifest = include_str!("../../authoring-promotion/Cargo.toml");
    let regular = manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(manifest);
    assert!(!regular.contains("authoring-promotion-postgres"));
    assert!(!regular.contains("sqlx"));
}
