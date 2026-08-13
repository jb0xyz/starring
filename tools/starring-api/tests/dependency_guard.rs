use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const WORKSPACE: &str = include_str!("../../../Cargo.toml");

fn collect_source_files(root: &Path, directory: &Path, files: &mut Vec<(PathBuf, String)>) {
    let mut entries = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap())
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let file_type = entry.file_type().unwrap();
        assert!(!file_type.is_symlink(), "{}", entry.path().display());
        if file_type.is_dir() {
            collect_source_files(root, &entry.path(), files);
        } else if entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "rs")
        {
            let relative = entry.path().strip_prefix(root).unwrap().to_path_buf();
            let source = fs::read_to_string(entry.path()).unwrap();
            files.push((relative, source));
        }
    }
}

fn source_files() -> Vec<(PathBuf, String)> {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_source_files(&source_root, &source_root, &mut files);
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

fn source_named<'a>(sources: &'a [(PathBuf, String)], name: &str) -> &'a str {
    sources
        .iter()
        .find(|(path, _)| path == Path::new(name))
        .map(|(_, source)| source.as_str())
        .unwrap()
}

fn package_dependencies() -> Vec<serde_json::Value> {
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
    metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == "starring-api")
        .unwrap()["dependencies"]
        .as_array()
        .unwrap()
        .clone()
}

fn dependency_named<'a>(
    dependencies: &'a [serde_json::Value],
    name: &str,
) -> &'a serde_json::Value {
    dependencies
        .iter()
        .find(|dependency| dependency["name"] == name && dependency["kind"].is_null())
        .unwrap()
}

fn development_dependency_named<'a>(
    dependencies: &'a [serde_json::Value],
    name: &str,
) -> &'a serde_json::Value {
    dependencies
        .iter()
        .find(|dependency| dependency["name"] == name && dependency["kind"] == "dev")
        .unwrap()
}

fn feature_names(dependency: &serde_json::Value) -> Vec<&str> {
    let mut features = dependency["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|feature| feature.as_str().unwrap())
        .collect::<Vec<_>>();
    features.sort_unstable();
    features
}

fn assert_identifiers_absent(path: &Path, source: &str, forbidden: &[&str]) {
    for value in forbidden {
        assert!(
            !contains_identifier(source, value),
            "{}: {value}",
            path.display()
        );
    }
}

fn assert_identifier_prefixes_absent(path: &Path, source: &str, forbidden: &[&str]) {
    for identifier in source
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .filter(|identifier| !identifier.is_empty())
    {
        for prefix in forbidden {
            assert!(
                !identifier.starts_with(prefix),
                "{}: {identifier}",
                path.display()
            );
        }
    }
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

fn contains_identifier(source: &str, expected: &str) -> bool {
    source
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .any(|identifier| identifier == expected)
}

#[test]
fn direct_dependencies_are_closed_to_reviewed_architecture_inputs() {
    let dependencies = package_dependencies();
    for dependency in &dependencies {
        let name = dependency["name"].as_str().unwrap();
        assert!(
            matches!(
                name,
                "async-trait"
                    | "authoring-application"
                    | "authoring-application-discord"
                    | "authoring-application-postgres"
                    | "authoring-promotion"
                    | "automation-ruleset"
                    | "axum"
                    | "base64"
                    | "chrono"
                    | "design-harness-codex-worker-client"
                    | "discord-model"
                    | "hyper"
                    | "hyper-util"
                    | "http-body-util"
                    | "product-control-http"
                    | "serde"
                    | "serde_json"
                    | "sqlx"
                    | "thiserror"
                    | "tokio"
                    | "tower"
                    | "twilight-http"
                    | "url"
                    | "zeroize"
            ),
            "unreviewed direct dependency: {name}"
        );
        assert!(dependency["rename"].is_null(), "dependency alias: {name}");
    }
}

#[test]
fn security_sensitive_dependency_features_are_exact() {
    let dependencies = package_dependencies();
    let axum = dependency_named(&dependencies, "axum");
    assert_eq!(axum["uses_default_features"], false);
    assert_eq!(feature_names(axum), ["http1", "http2", "tokio"]);
    let sqlx = dependency_named(&dependencies, "sqlx");
    assert_eq!(sqlx["uses_default_features"], false);
    assert_eq!(feature_names(sqlx), ["postgres", "runtime-tokio-rustls"]);
    assert_eq!(
        feature_names(dependency_named(&dependencies, "hyper")),
        ["http1", "http2", "server"]
    );
    assert_eq!(
        dependency_named(&dependencies, "hyper")["uses_default_features"],
        false
    );
    assert_eq!(
        feature_names(dependency_named(&dependencies, "hyper-util")),
        ["server-auto", "service", "tokio"]
    );
    assert_eq!(
        dependency_named(&dependencies, "hyper-util")["uses_default_features"],
        false
    );
    assert_eq!(
        feature_names(dependency_named(&dependencies, "tokio")),
        ["macros", "net", "rt-multi-thread", "signal", "sync", "time"]
    );
    assert_eq!(
        dependency_named(&dependencies, "tokio")["uses_default_features"],
        false
    );
    let tower = dependency_named(&dependencies, "tower");
    assert_eq!(tower["uses_default_features"], false);
    assert_eq!(feature_names(tower), ["util"]);
    let test_hyper = development_dependency_named(&dependencies, "hyper");
    assert_eq!(test_hyper["uses_default_features"], false);
    assert_eq!(feature_names(test_hyper), ["client"]);
    let test_tokio = development_dependency_named(&dependencies, "tokio");
    assert_eq!(test_tokio["uses_default_features"], false);
    assert_eq!(feature_names(test_tokio), ["io-util"]);
}

#[test]
fn package_is_registered_once_and_source_modules_are_exactly_classified() {
    let sources = source_files();
    assert_eq!(WORKSPACE.matches("\"tools/starring-api\"").count(), 1);
    for required in ["lib.rs", "input.rs", "error.rs", "projection.rs"] {
        source_named(&sources, required);
    }
    for (path, _) in &sources {
        assert!(matches!(
            path.to_str().unwrap(),
            "lib.rs"
                | "input.rs"
                | "error.rs"
                | "projection.rs"
                | "facade.rs"
                | "secret.rs"
                | "config.rs"
                | "composition.rs"
                | "server.rs"
                | "telemetry.rs"
                | "web.rs"
                | "authoring_admission.rs"
                | "main.rs"
        ));
    }
}

#[test]
fn infrastructure_dependencies_are_confined_to_exact_modules() {
    let sources = source_files();
    for (path, source) in &sources {
        match path.to_str().unwrap() {
            "composition.rs" => assert_identifiers_absent(
                path,
                source,
                &["axum", "hyper", "hyper_util", "tower", "TcpListener"],
            ),
            "server.rs" => assert_identifiers_absent(
                path,
                source,
                &[
                    "sqlx",
                    "twilight_http",
                    "PostgresProductPromotions",
                    "PostgresProductDecisions",
                    "PostgresProductActionDigest",
                    "PostgresProductRuntimeStatus",
                ],
            ),
            "web.rs" => assert_identifiers_absent(
                path,
                source,
                &[
                    "sqlx",
                    "twilight_http",
                    "hyper",
                    "hyper_util",
                    "tower",
                    "TcpListener",
                ],
            ),
            "main.rs" => assert_identifiers_absent(
                path,
                source,
                &[
                    "sqlx",
                    "twilight_http",
                    "axum",
                    "hyper",
                    "hyper_util",
                    "tower",
                    "PgPool",
                    "PgConnectOptions",
                ],
            ),
            _ => assert_identifiers_absent(
                path,
                source,
                &[
                    "sqlx",
                    "twilight_http",
                    "axum",
                    "hyper",
                    "hyper_util",
                    "tower",
                    "TcpListener",
                ],
            ),
        }
    }
}

#[test]
fn raw_operational_adapters_remain_confined_to_facade_and_composition() {
    let sources = source_files();
    for (path, source) in &sources {
        if path != Path::new("composition.rs") && path != Path::new("facade.rs") {
            assert_identifier_prefixes_absent(path, source, &["Postgres", "Twilight"]);
            assert_identifiers_absent(path, source, &["DiscordOAuthClient"]);
        }
    }
}

#[test]
fn codex_worker_client_remains_confined_to_composition() {
    for (path, source) in source_files() {
        if path != Path::new("composition.rs") {
            assert_identifiers_absent(
                &path,
                &source,
                &["design_harness_codex_worker_client", "CodexWorkerClient"],
            );
        }
    }
}

#[test]
fn executable_and_listener_cannot_name_adapter_families() {
    let sources = source_files();
    for path in [Path::new("main.rs"), Path::new("server.rs")] {
        let source = source_named(&sources, path.to_str().unwrap());
        assert_identifiers_absent(
            path,
            source,
            &[
                "authoring_application_discord",
                "authoring_application_postgres",
            ],
        );
        assert_identifier_prefixes_absent(path, source, &["Postgres", "Twilight"]);
    }
}

#[test]
fn executable_is_pinned_to_the_complete_authoring_and_readiness_router() {
    let sources = source_files();
    let main = source_named(&sources, "main.rs");
    for required in [
        "product_control_router_with_full_authoring_v1_and_readiness_gate",
        "ProductApiReadinessGate",
        "initially_unready",
        "serve_verified_loopback_with_runtime_readiness",
    ] {
        assert!(contains_identifier(main, required), "main.rs: {required}");
    }
    for forbidden in [
        "product_control_router",
        "product_control_router_with_readiness_gate",
        "product_control_router_with_operational_v2",
        "product_control_router_with_operational_v2_and_readiness_gate",
        "product_control_router_with_operational_v2_and_lifecycle_v1_and_readiness_gate",
        "serve_verified_loopback",
    ] {
        assert!(
            !contains_identifier(main, forbidden),
            "main.rs: {forbidden}"
        );
    }
}

#[test]
fn every_source_is_comment_free_and_avoids_unsafe_code() {
    for (path, source) in source_files() {
        assert!(!has_rust_comment(&source), "{}", path.display());
        assert!(
            !contains_identifier(&source, "unsafe"),
            "{}",
            path.display()
        );
    }
}

#[test]
fn comment_scanner_distinguishes_literals_from_source_comments() {
    assert!(!has_rust_comment(
        r##"let url = "https://starring.example/path"; let raw = r#"/*data*/"#;"##
    ));
    assert!(!has_rust_comment(
        r##"let raw_bytes = br#"https://starring.example"#;"##
    ));
    assert!(has_rust_comment("let value = 1; // forbidden"));
    assert!(has_rust_comment("let value = /* forbidden */ 1;"));
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
        "MappedLifecycleCancellationCommand",
        "map_lifecycle_cancellation_command",
        "map_authoring_application_error",
        "map_product_application_error",
        "ProductControlPortError::RuntimeDrainRequired",
        "ProductControlPortError::RuntimeDrainPending",
        "ProductControlPortError::LifecycleCancelled",
        "project_promotion",
        "project_lifecycle_cancellation",
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
