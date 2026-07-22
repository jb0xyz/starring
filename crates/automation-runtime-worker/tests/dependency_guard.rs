use std::fs;
use std::path::{Path, PathBuf};

fn collect_rust_sources(directory: &Path, sources: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_rust_sources(&path, sources);
        } else if path.extension().and_then(|value| value.to_str()) == Some("rs") {
            sources.push(path);
        }
    }
}

#[test]
fn worker_dependency_surface_is_pure_library_only_and_closed() {
    let manifest = include_str!("../Cargo.toml");
    let root_manifest = include_str!("../../../Cargo.toml");
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source_root = crate_root.join("src");
    let mut sources = Vec::new();
    collect_rust_sources(&source_root, &mut sources);
    sources.sort();
    let relative_sources = sources
        .iter()
        .map(|path| path.strip_prefix(crate_root).unwrap().to_path_buf())
        .collect::<Vec<_>>();

    assert_eq!(
        root_manifest
            .matches("\"crates/automation-runtime-worker\"")
            .count(),
        1
    );
    assert_eq!(
        relative_sources,
        [
            PathBuf::from("src/gateway_lifecycle.rs"),
            PathBuf::from("src/gateway_owner.rs"),
            PathBuf::from("src/lib.rs"),
            PathBuf::from("src/writer_fence.rs"),
        ]
    );
    assert!(!crate_root.join("build.rs").exists());
    for forbidden in [
        "[[bin]]",
        "[build-dependencies]",
        "build =",
        "crate-type",
        "proc-macro",
        "[dev-dependencies]",
    ] {
        assert!(
            !manifest.contains(forbidden),
            "forbidden worker package surface: {forbidden}"
        );
    }

    let dependencies = manifest
        .split_once("[dependencies]\n")
        .unwrap()
        .1
        .split("\n[")
        .next()
        .unwrap()
        .lines()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(
        dependencies,
        [
            "automation-runtime-controller = { path = \"../automation-runtime-controller\" }",
            "automation-runtime-convergence = { path = \"../automation-runtime-convergence\" }",
            "chrono = \"0.4\"",
            "thiserror.workspace = true",
        ]
    );

    for path in sources {
        let source = fs::read_to_string(&path).unwrap();
        for forbidden in [
            "sqlx",
            "tokio",
            "twilight",
            "serde",
            "serde_json",
            "reqwest",
            "hyper",
            "automation_runtime::",
            "automation_runtime_registry",
            "automation_runtime_execution_postgres",
            "automation_runtime_serving_postgres",
            "ai_gateway",
            "design_harness",
            "std::env",
            "std::fs",
            "std::io",
            "std::net",
            "std::process",
            "std::signal",
            "TcpStream",
            "UdpSocket",
            "async fn",
            "Serialize",
            "Deserialize",
            "Default",
            "unsafe",
            "//",
            "/*",
        ] {
            assert!(
                !source.contains(forbidden),
                "forbidden worker source surface in {}: {forbidden}",
                path.display()
            );
        }
    }
}

#[test]
fn worker_coordinator_authority_and_state_surface_stay_exact() {
    let lifecycle = include_str!("../src/gateway_lifecycle.rs");
    let invalidation = lifecycle
        .split("pub enum RuntimeGatewayInvalidationCauseV2 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl From").next())
        .unwrap();

    assert!(lifecycle.contains(concat!(
        "#[derive(Debug, PartialEq, Eq)]\n",
        "pub struct RuntimeGatewayClosedLifecycleV2 {\n",
        "    snapshot: RuntimeGatewayClosedSnapshotV2,\n",
        "}"
    )));
    assert!(!lifecycle.contains("impl Clone for RuntimeGatewayClosedLifecycleV2"));
    assert!(!lifecycle.contains("impl Default for RuntimeGatewayClosedLifecycleV2"));
    assert!(lifecycle.contains("pub enum RuntimeGatewayClosedSnapshotV2"));
    assert!(lifecycle.contains("Emergency {"));
    assert!(lifecycle.contains("Shutdown {"));
    assert!(!lifecycle.contains("Open"));
    assert!(!lifecycle.contains("RecoveryPending"));
    assert!(!lifecycle.contains("AdmissionAcknowledging"));
    assert!(!invalidation.contains("Starting"));
    assert_eq!(invalidation.matches("    ").count(), 5);
}

#[test]
fn worker_writer_fence_surface_is_observe_only() {
    let source = include_str!("../src/writer_fence.rs");

    assert_eq!(source.matches("fn observe_writer_fence(").count(), 1);
    assert_eq!(source.matches("\n    fn ").count(), 1);
    for forbidden in [
        "close_writer_fence",
        "open_writer_fence",
        "renew_writer_fence",
        "acquire_writer_fence",
        "release_writer_fence",
        "Mutation",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden writer fence authority: {forbidden}"
        );
    }
}
