#[test]
fn readiness_is_pure() {
    let manifest = include_str!("../Cargo.toml");
    assert!(!manifest.contains("sqlx"));
    assert!(!manifest.contains("twilight"));
}
