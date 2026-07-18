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
        "ai-gateway",
        "automation-runtime",
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
    let source = include_str!("../src/lib.rs");
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
    let source = include_str!("../src/lib.rs");
    assert!(!source.contains("from_trusted_edge"));
    assert!(!source.contains("pub fn from_identity"));
    assert!(!source.contains("pub trait OwnedSessionArtifactPort"));
    assert!(!source.contains("pub trait PromotionAuthorityPort"));
    assert!(source.contains("pub trait AuthenticationPort"));
    assert!(source.contains("pub trait AuthorizedPromotionSnapshotPort"));
    assert!(source.contains("load_atomic_authorized_snapshot"));
}
