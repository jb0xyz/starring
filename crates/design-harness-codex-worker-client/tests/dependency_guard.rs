use std::collections::BTreeSet;

fn dependency_keys(manifest: &str, include_development: bool) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    let mut dependency_map = false;

    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            let section = &line[1..line.len() - 1];
            let regular = section == "dependencies"
                || (section.starts_with("target.") && section.ends_with(".dependencies"));
            let development = section == "dev-dependencies"
                || (section.starts_with("target.") && section.ends_with(".dev-dependencies"));
            dependency_map = regular || (include_development && development);
            if let Some(key) = dependency_table_key(section, include_development) {
                keys.insert(key.to_string());
            }
            continue;
        }
        if !dependency_map || line.is_empty() {
            continue;
        }
        if let Some((key, _)) = line.split_once('=') {
            let key = key.trim().trim_matches(['\'', '"']);
            let key = key.strip_suffix(".workspace").unwrap_or(key);
            if !key.is_empty() {
                keys.insert(key.to_string());
            }
        }
    }

    keys
}

fn dependency_table_key(section: &str, include_development: bool) -> Option<&str> {
    if let Some(key) = section.strip_prefix("dependencies.") {
        return nonempty_key(key);
    }
    if include_development {
        if let Some(key) = section.strip_prefix("dev-dependencies.") {
            return nonempty_key(key);
        }
    }
    if !section.starts_with("target.") {
        return None;
    }
    if let Some(index) = section.find(".dependencies.") {
        return nonempty_key(&section[index + ".dependencies.".len()..]);
    }
    if include_development {
        if let Some(index) = section.find(".dev-dependencies.") {
            return nonempty_key(&section[index + ".dev-dependencies.".len()..]);
        }
    }
    None
}

fn nonempty_key(key: &str) -> Option<&str> {
    let key = key.trim().trim_matches(['\'', '"']);
    (!key.is_empty()).then_some(key)
}

#[test]
fn workspace_registers_one_library_only_edge_crate() {
    let workspace = include_str!("../../../Cargo.toml");
    let member = "\"crates/design-harness-codex-worker-client\"";
    assert_eq!(workspace.matches(member).count(), 1);

    let manifest = include_str!("../Cargo.toml");
    for forbidden in [
        "[[bin]]",
        "[build-dependencies]",
        "build =",
        "crate-type",
        "package =",
        "proc-macro",
    ] {
        assert!(
            !manifest.contains(forbidden),
            "forbidden manifest edge: {forbidden}"
        );
    }
}

#[test]
fn regular_dependencies_exclude_product_and_persistent_edges() {
    let manifest = include_str!("../Cargo.toml");
    let regular_dependencies = dependency_keys(manifest, false);
    assert_eq!(
        regular_dependencies,
        BTreeSet::from([
            "design-harness".to_string(),
            "reqwest".to_string(),
            "serde".to_string(),
            "serde_json".to_string(),
            "sha2".to_string(),
            "zeroize".to_string(),
        ])
    );
    let dependencies = dependency_keys(manifest, true);
    assert_eq!(
        dependencies,
        BTreeSet::from([
            "design-harness".to_string(),
            "reqwest".to_string(),
            "serde".to_string(),
            "serde_json".to_string(),
            "sha2".to_string(),
            "tokio".to_string(),
            "zeroize".to_string(),
        ])
    );
    let forbidden_exact = [
        "sqlx",
        "rusqlite",
        "sqlite",
        "postgres",
        "tokio-postgres",
        "ai-gateway",
        "product-control-http",
        "authoring-promotion",
        "automation-runtime",
        "bot-runtime",
    ];

    for dependency in &dependencies {
        assert!(
            !forbidden_exact.contains(&dependency.as_str()),
            "forbidden regular dependency: {dependency}"
        );
        assert!(
            dependency != "twilight" && !dependency.starts_with("twilight-"),
            "forbidden regular dependency: {dependency}"
        );
        assert!(
            !dependency.starts_with("product-control-")
                && !dependency.starts_with("authoring-promotion-")
                && !dependency.starts_with("automation-runtime-"),
            "forbidden regular dependency: {dependency}"
        );
        assert!(
            !dependency.ends_with("-postgres"),
            "forbidden PostgreSQL adapter dependency: {dependency}"
        );
    }
}

#[test]
fn parser_matches_regular_dependency_keys_without_false_positives() {
    let manifest = r#"
[dependencies]
design-harness = { path = "../design-harness" }
reqwest = "0.12"
serde.workspace = true

[dev-dependencies]
sqlx = "0.8"

[workspace.dependencies]
automation-runtime = "1"
"#;

    assert_eq!(
        dependency_keys(manifest, false),
        BTreeSet::from([
            "design-harness".to_string(),
            "reqwest".to_string(),
            "serde".to_string(),
        ])
    );
    assert!(dependency_keys(manifest, true).contains("sqlx"));
}

#[test]
fn library_source_contains_no_comments() {
    let source = include_str!("../src/lib.rs");
    assert!(!has_rust_comment(source));
}

fn has_rust_comment(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'r' {
            let mut delimiter = index + 1;
            while delimiter < bytes.len() && bytes[delimiter] == b'#' {
                delimiter += 1;
            }
            if delimiter < bytes.len() && bytes[delimiter] == b'"' {
                let hashes = delimiter - index - 1;
                index = delimiter + 1;
                while index < bytes.len() {
                    if bytes[index] == b'"'
                        && index + hashes < bytes.len()
                        && (hashes == 0
                            || bytes[index + 1..=index + hashes]
                                .iter()
                                .all(|value| *value == b'#'))
                    {
                        index += hashes + 1;
                        break;
                    }
                    index += 1;
                }
                continue;
            }
        }
        if bytes[index] == b'"' {
            index += 1;
            while index < bytes.len() {
                match bytes[index] {
                    b'\\' => index = (index + 2).min(bytes.len()),
                    b'"' => {
                        index += 1;
                        break;
                    }
                    _ => index += 1,
                }
            }
            continue;
        }
        if bytes[index] == b'/'
            && index + 1 < bytes.len()
            && matches!(bytes[index + 1], b'/' | b'*')
        {
            return true;
        }
        index += 1;
    }
    false
}
