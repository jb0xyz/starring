use std::fs;
use std::path::Path;

fn collect_sources(path: &Path, output: &mut String) {
    for entry in fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            collect_sources(&path, output);
        } else if path.extension().and_then(|value| value.to_str()) == Some("rs") {
            output.push_str(&fs::read_to_string(path).unwrap());
        }
    }
}

#[test]
fn http_edge_depends_on_the_facade_and_not_raw_stores() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(manifest.contains("authoring-application"));
    for forbidden in [
        "authoring-promotion-postgres",
        "automation-ruleset-activation-postgres",
        "automation-ruleset-postgres",
        "sqlx",
        "twilight",
    ] {
        assert!(
            !manifest.contains(forbidden),
            "forbidden dependency {forbidden}"
        );
    }
    assert!(manifest.contains("zeroize = \"1\""));
    assert!(manifest.contains("subtle = \"2\""));
    let mut sources = String::new();
    collect_sources(&root.join("src"), &mut sources);
    for forbidden in [
        "PromotionStore",
        "ActivationRequestStore",
        "PostgresPromotionStore",
        "PostgresActivationRequestStore",
    ] {
        assert!(!sources.contains(forbidden), "forbidden symbol {forbidden}");
    }
}

#[test]
fn transport_secrets_are_zeroizing_and_queries_are_not_traced() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let secrets = fs::read_to_string(root.join("src/secret.rs")).unwrap();
    let facade = fs::read_to_string(root.join("src/facade.rs")).unwrap();
    let router = fs::read_to_string(root.join("src/router.rs")).unwrap();
    let boundary = fs::read_to_string(root.join("src/router/boundary.rs")).unwrap();
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(secrets.contains("Zeroizing<String>"));
    assert!(secrets.contains("ConstantTimeEq"));
    assert!(!secrets.contains("pub struct OAuthCode(String)"));
    assert!(secrets.contains("pub struct IdempotencyKey(Zeroizing<String>)"));
    assert!(router.contains("query.map(Zeroizing::new)"));
    assert!(router.contains("return malformed_oauth_callback(&request_id)"));
    assert!(router.contains("append_cookie(&mut response, clear_cookie(OAUTH_COOKIE))"));
    assert!(boundary.contains("Result<Zeroizing<String>, CookieReadError>"));
    assert!(boundary.contains(") -> Result<IdempotencyKey, Response>"));
    assert!(!facade.contains("authorization_url"));
    assert!(!facade.contains("use url::Url"));
    assert!(facade.contains("pub struct DiscordAuthorizationRequest"));
    assert!(facade.contains("pub authorization_request: DiscordAuthorizationRequest"));
    assert!(facade.contains("pub idempotency_key: crate::IdempotencyKey"));
    assert!(!facade.contains("pub idempotency_key: String"));
    assert!(boundary.contains(") -> Option<Zeroizing<String>>"));
    assert!(boundary.contains("let mut location = Zeroizing::new(String::with_capacity("));
    assert!(boundary.contains("https://discord.com/oauth2/authorize?client_id="));
    assert!(boundary.contains("&response_type=code&scope=identify&state="));
    assert_eq!(boundary.matches("request.uri().path()").count(), 2);
    assert!(!boundary.contains("request.uri().query()"));
    assert!(!boundary.contains("request.uri().to_string()"));
    assert!(!manifest.contains("tower-http"));
    assert!(!manifest.contains("tracing"));
}

#[test]
fn source_contains_no_comments() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut sources = String::new();
    collect_sources(&root.join("src"), &mut sources);
    collect_sources(&root.join("tests"), &mut sources);
    assert!(!sources
        .lines()
        .any(|line| line.trim_start().starts_with("//")));
}
