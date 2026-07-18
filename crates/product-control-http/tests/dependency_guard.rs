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
fn source_contains_no_comments() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut sources = String::new();
    collect_sources(&root.join("src"), &mut sources);
    assert!(!sources
        .lines()
        .any(|line| line.trim_start().starts_with("//")));
}
