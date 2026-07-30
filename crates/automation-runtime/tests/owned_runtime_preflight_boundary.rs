fn production_prefix(source: &str) -> &str {
    source.split("#[cfg(test)]").next().unwrap()
}

#[test]
fn owned_preflight_exposes_only_token_and_domain_inputs() {
    let readiness = production_prefix(include_str!("../src/readiness.rs"));
    let public = readiness
        .split("impl OwnedDiscordRuntimePreflightV1")
        .nth(1)
        .unwrap()
        .split("pub fn build_runtime_readiness_context_v1")
        .next()
        .unwrap();

    assert!(public.contains("pub fn new(discord_token: String) -> Self"));
    assert!(public.contains("pub async fn preflight("));
    assert!(public.contains("guild_id: GuildId"));
    assert!(public.contains("artifact: &RuleSetVersion"));
    assert!(public.contains("bindings: &ResourceBindingMap"));
    for forbidden in [
        "pub fn new(http:",
        "pub async fn new(http:",
        "pub fn http(",
        "pub fn client(",
        "pub fn token(",
        "pub http:",
        "pub bot_user_id:",
    ] {
        assert!(!public.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn crate_root_exports_the_owned_boundary_and_domain_error() {
    let root = include_str!("../src/lib.rs");

    assert!(root.contains("OwnedDiscordRuntimePreflightV1"));
    assert!(root.contains("RuntimeDiscordPreflightErrorV1"));
}
