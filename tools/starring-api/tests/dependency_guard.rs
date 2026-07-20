use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const WORKSPACE: &str = include_str!("../../../Cargo.toml");

fn source_files() -> Vec<(PathBuf, String)> {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut paths = fs::read_dir(source_root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .collect::<Vec<_>>();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let source = fs::read_to_string(&path).unwrap();
            (path, source)
        })
        .collect()
}

fn source_named<'a>(sources: &'a [(PathBuf, String)], name: &str) -> &'a str {
    sources
        .iter()
        .find(|(path, _)| path.file_name().is_some_and(|file_name| file_name == name))
        .map(|(_, source)| source.as_str())
        .unwrap()
}

fn direct_dependency_names() -> Vec<String> {
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
            concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let package = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == "starring-api")
        .unwrap();
    package["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|dependency| dependency["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn edge_mapping_package_has_no_infrastructure_or_raw_store_dependency() {
    let dependencies = direct_dependency_names();
    for forbidden in ["sqlx", "twilight", "axum", "tower", "reqwest"] {
        assert!(!dependencies.iter().any(|dependency| {
            dependency == forbidden || dependency.starts_with(&format!("{forbidden}-"))
        }));
    }
    let sources = source_files();
    for name in ["input.rs", "error.rs", "projection.rs"] {
        let source = source_named(&sources, name);
        for forbidden in [
            "sqlx::",
            "twilight::",
            "twilight_",
            "PostgresProductPromotions",
            "PostgresProductDecisions",
            "PromotionService",
            "PromotionStore ",
            "PendingActivationPort ",
            "RuleSetStore ",
            "ActivationRequestStore",
        ] {
            assert!(!source.contains(forbidden), "{name}: {forbidden}");
        }
    }
}

#[test]
fn package_is_registered_once_and_source_modules_are_classified() {
    let sources = source_files();
    assert_eq!(WORKSPACE.matches("\"tools/starring-api\"").count(), 1);
    for required in ["lib.rs", "input.rs", "error.rs", "projection.rs"] {
        source_named(&sources, required);
    }
    for (path, _) in &sources {
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(matches!(
            name,
            "lib.rs" | "input.rs" | "error.rs" | "projection.rs" | "facade.rs"
        ));
    }
}

#[test]
fn every_source_is_comment_free_and_avoids_unsafe_code() {
    for (path, source) in source_files() {
        assert!(!source.contains("//"), "{}", path.display());
        assert!(!source.contains("/*"), "{}", path.display());
        assert!(!source.contains("*/"), "{}", path.display());
        assert!(!source.contains("unsafe"), "{}", path.display());
    }
}

#[test]
fn edge_contract_keeps_typed_inputs_closed_errors_and_v2_projection() {
    let sources = source_files();
    let input = source_named(&sources, "input.rs");
    let error = source_named(&sources, "error.rs");
    let projection = source_named(&sources, "projection.rs");
    let contract = [input, error, projection].concat();
    for required in [
        "MappedPromoteCommand",
        "MappedApproveCommand",
        "MappedRejectCommand",
        "MappedApplyCommand",
        "map_authoring_application_error",
        "map_product_application_error",
        "project_promotion",
        "project_deployment_operational_v2",
        "system_time_to_utc",
    ] {
        assert!(contract.contains(required));
    }
    assert!(!projection.contains("DateTime::<Utc>::from("));
    assert!(projection.contains("revision: decision.revision().get()"));
    assert!(!projection.contains("revision: promotion.revision()"));
    assert!(projection.contains("DeploymentConvergencePhaseV2::Cancelled"));
    assert!(projection.contains("DeploymentServingFreshnessV2::IdentityMismatch"));
}
