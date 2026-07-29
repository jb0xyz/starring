use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn rust_sources() -> Vec<PathBuf> {
    let source = root().join("src");
    let mut files = fs::read_dir(source)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .collect::<Vec<_>>();
    files.sort();
    files
}

#[test]
fn authority_operator_has_a_narrow_dependency_surface() {
    let manifest = fs::read_to_string(root().join("Cargo.toml")).unwrap();
    for forbidden in [
        "twilight",
        "design-harness",
        "authoring-application",
        "product-control-http",
        "automation-runtime",
        "reqwest",
        "axum",
    ] {
        assert!(!manifest.contains(forbidden));
    }
    for required in ["resource-resolution", "sqlx", "zeroize", "serde_json"] {
        assert!(manifest.contains(required));
    }
}

#[test]
fn secret_and_database_boundaries_are_fixed_and_redacted() {
    let keychain = fs::read_to_string(root().join("src/keychain.rs")).unwrap();
    for required in [
        "starring.postgres.staging",
        "database.cluster-admin",
        "\"find-generic-password\"",
        "\"-w\"",
        ".env_clear()",
        ".stderr(Stdio::null())",
        "Zeroizing<Vec<u8>>",
    ] {
        assert!(keychain.contains(required));
    }
    let postgres = fs::read_to_string(root().join("src/postgres.rs")).unwrap();
    for required in [
        "resource_binding_fingerprint_v2",
        "installation_authority_payload_digest_v1",
        "installation_authority_request_digest_v1",
        "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE",
        "SET LOCAL ROLE starring_owner",
        "current_authority_revision = 1",
        "current_authority_revision = 2",
        "SERIALIZABLE READ ONLY DEFERRABLE",
    ] {
        assert!(postgres.contains(required));
    }
    for forbidden in [
        "std::env::var(\"DATABASE_URL\")",
        "std::env::var(\"PGPASSWORD\")",
        "println!(\"postgresql://",
        "eprintln!(\"postgresql://",
    ] {
        assert!(!postgres.contains(forbidden));
    }
}

#[test]
fn rust_source_comments_and_unsafe_blocks_are_forbidden() {
    for path in rust_sources() {
        let source = fs::read_to_string(&path).unwrap();
        assert!(!source.lines().any(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("//")
                || trimmed.starts_with("/*")
                || trimmed.starts_with('*')
                || trimmed.ends_with("*/")
        }));
        assert!(!source.contains("unsafe {"));
    }
}
