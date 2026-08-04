fn production_prefix(source: &str) -> &str {
    source.split("#[cfg(test)]").next().unwrap()
}

#[test]
fn production_role_creation_explicitly_requests_empty_permissions() {
    let source = production_prefix(include_str!("../src/mutation.rs"));
    let create_role = source
        .split("async fn create_role(")
        .nth(1)
        .unwrap()
        .split("async fn create_channel(")
        .next()
        .unwrap();

    assert_eq!(
        create_role
            .matches(".permissions(TwilightPermissions::empty())")
            .count(),
        1
    );
    assert_eq!(create_role.matches(".permissions(").count(), 1);
}

#[test]
fn role_permission_boundary_adds_no_dependency() {
    let manifest = include_str!("../Cargo.toml");

    assert!(manifest.contains("twilight-model = \"0.17\""));
    for dependency in ["reqwest", "hyper", "wiremock", "mockito"] {
        assert!(!manifest.contains(dependency), "{dependency}");
    }
}
