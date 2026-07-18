use std::collections::BTreeSet;

fn regular_dependency_keys(manifest: &str) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    let mut dependency_map = false;

    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            let section = &line[1..line.len() - 1];
            dependency_map = section == "dependencies"
                || (section.starts_with("target.") && section.ends_with(".dependencies"));
            if let Some(key) = dependency_table_key(section) {
                keys.insert(key.to_string());
            }
            continue;
        }
        if !dependency_map || line.is_empty() {
            continue;
        }
        if let Some((key, _)) = line.split_once('=') {
            let key = key.trim().trim_matches(['\'', '"']);
            if !key.is_empty() {
                keys.insert(key.to_string());
            }
        }
    }

    keys
}

fn dependency_table_key(section: &str) -> Option<&str> {
    if let Some(key) = section.strip_prefix("dependencies.") {
        return nonempty_key(key);
    }
    if !section.starts_with("target.") {
        return None;
    }
    section
        .find(".dependencies.")
        .and_then(|index| nonempty_key(&section[index + ".dependencies.".len()..]))
}

fn nonempty_key(key: &str) -> Option<&str> {
    let key = key.trim().trim_matches(['\'', '"']);
    (!key.is_empty()).then_some(key)
}

#[test]
fn regular_dependencies_stay_pure() {
    let manifest = include_str!("../Cargo.toml");
    let dependencies = regular_dependency_keys(manifest);
    let forbidden = [
        "sqlx",
        "rusqlite",
        "axum",
        "ai-gateway",
        "automation-runtime",
        "automation-ruleset-activation",
        "automation-ruleset-readiness",
        "approval-manager",
        "policy-engine",
        "preview",
        "postgres",
        "tokio-postgres",
    ];

    for dependency in &dependencies {
        assert!(
            !forbidden.contains(&dependency.as_str()),
            "forbidden regular dependency: {dependency}"
        );
        assert!(
            dependency != "twilight" && !dependency.starts_with("twilight-"),
            "forbidden regular dependency: {dependency}"
        );
        assert!(
            !dependency.ends_with("-postgres"),
            "forbidden PostgreSQL adapter dependency: {dependency}"
        );
    }
}

#[test]
fn parser_matches_dependency_keys_without_substring_false_positives() {
    let manifest = r#"
[dependencies]
automation-ruleset-activation = { path = "../automation-ruleset-activation" }
design-harness = { path = "../design-harness" }

[dev-dependencies]
sqlx = "0.8"

[workspace.dependencies]
ai-gateway = "1"
"#;

    assert_eq!(
        regular_dependency_keys(manifest),
        BTreeSet::from([
            "automation-ruleset-activation".to_string(),
            "design-harness".to_string(),
        ])
    );
}

#[test]
fn authority_inputs_stay_non_deserializable_and_out_of_the_client_command() {
    let source = source();
    assert!(!source.contains("Deserialize"));
    let command = source
        .split("pub struct PromoteOwnedSessionV1")
        .nth(1)
        .unwrap()
        .split('}')
        .next()
        .unwrap();
    for forbidden in [
        "guild_id",
        "installation_id",
        "ruleset_key",
        "requester",
        "binding_revision",
        "policy",
        "artifact",
    ] {
        assert!(
            !command.contains(forbidden),
            "client command leaked {forbidden}"
        );
    }
}

#[test]
fn authenticated_actor_stays_crate_issued_and_authority_load_stays_atomic() {
    let source = source();
    assert!(!source.contains("from_trusted_edge"));
    assert!(!source.contains("pub fn from_identity"));
    assert!(!source.contains("pub trait OwnedSessionArtifactPort"));
    assert!(!source.contains("pub trait PromotionAuthorityPort"));
    assert!(source.contains("pub trait AuthenticationPort"));
    assert!(source.contains("pub trait AuthorizedPromotionSnapshotPort"));
    assert!(source.contains("load_atomic_authorized_snapshot"));
    let identity = include_str!("../src/identity.rs");
    let authenticated_identity = identity
        .split("pub struct AuthenticatedIdentityV1")
        .nth(1)
        .unwrap()
        .split('}')
        .next()
        .unwrap();
    assert!(!authenticated_identity.contains("tenant"));
}

#[test]
fn product_commands_never_accept_trusted_authority_or_apply_attempt_fields() {
    let source = source();
    for command_name in [
        "ApproveProductPromotionV1",
        "RejectProductPromotionV1",
        "ApplyProductPromotionV1",
    ] {
        let command = source
            .split(&format!("pub struct {command_name}"))
            .nth(1)
            .unwrap()
            .split('}')
            .next()
            .unwrap();
        for forbidden in [
            "actor",
            "guild_id",
            "requester",
            "policy",
            "target",
            "attempt_id",
        ] {
            assert!(
                !command.contains(forbidden),
                "client command {command_name} leaked {forbidden}"
            );
        }
    }
}

#[test]
fn decision_port_has_only_bound_decisions_and_adapter_derived_apply_identity() {
    let source = include_str!("../src/control.rs");
    let product_port = source
        .split("pub trait ProductDecisionPort")
        .nth(1)
        .unwrap();
    assert!(product_port.contains("approve_payload_bound"));
    assert!(product_port.contains("reject_payload_bound"));
    assert!(product_port.contains("apply_idempotent"));
    assert!(!product_port.contains("ApplyAttemptId"));
    assert!(!product_port.contains("async fn approve("));
    assert!(!product_port.contains("async fn reject("));
    assert!(!product_port.contains("async fn apply("));
}

fn source() -> String {
    [
        include_str!("../src/lib.rs"),
        include_str!("../src/application.rs"),
        include_str!("../src/authority.rs"),
        include_str!("../src/control.rs"),
        include_str!("../src/identity.rs"),
        include_str!("../src/promotion.rs"),
        include_str!("../src/status.rs"),
    ]
    .join("\n")
}
