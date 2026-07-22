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
            files.push((relative, fs::read_to_string(entry.path()).unwrap()));
        }
    }
}

fn source_files() -> Vec<(PathBuf, String)> {
    let package_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for directory in ["src", "tests"] {
        collect_source_files(package_root, &package_root.join(directory), &mut files);
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
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
        .find(|package| package["name"] == "starring-runtime")
        .unwrap()["dependencies"]
        .as_array()
        .unwrap()
        .clone()
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
        if bytes[index] == b'b' && index + 1 < bytes.len() && bytes[index + 1] == b'\'' {
            if let Some(end) = character_literal_end(bytes, index + 1) {
                index = end;
                continue;
            }
        }
        if bytes[index] == b'\'' {
            if let Some(end) = character_literal_end(bytes, index) {
                index = end;
                continue;
            }
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

fn character_literal_end(bytes: &[u8], quote: usize) -> Option<usize> {
    let mut cursor = quote.checked_add(1)?;
    let first = *bytes.get(cursor)?;
    if first == b'\\' {
        cursor = cursor.checked_add(1)?;
        match *bytes.get(cursor)? {
            b'x' => {
                let first_hex = *bytes.get(cursor + 1)?;
                let second_hex = *bytes.get(cursor + 2)?;
                if !first_hex.is_ascii_hexdigit() || !second_hex.is_ascii_hexdigit() {
                    return None;
                }
                cursor += 3;
            }
            b'u' => {
                cursor += 1;
                if *bytes.get(cursor)? != b'{' {
                    return None;
                }
                cursor += 1;
                let digits = cursor;
                while bytes.get(cursor).is_some_and(u8::is_ascii_hexdigit) {
                    cursor += 1;
                }
                if cursor == digits || *bytes.get(cursor)? != b'}' {
                    return None;
                }
                cursor += 1;
            }
            _ => cursor += 1,
        }
    } else {
        cursor += utf8_character_width(first)?;
    }
    if *bytes.get(cursor)? == b'\'' {
        Some(cursor + 1)
    } else {
        None
    }
}

fn utf8_character_width(first: u8) -> Option<usize> {
    match first {
        0x00..=0x7f => Some(1),
        0xc2..=0xdf => Some(2),
        0xe0..=0xef => Some(3),
        0xf0..=0xf4 => Some(4),
        _ => None,
    }
}

fn contains_identifier(source: &str, expected: &str) -> bool {
    source
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .any(|identifier| identifier == expected)
}

#[test]
fn comment_scanner_cannot_be_masked_by_character_literals() {
    for source in [
        r#"let value = '"';"#,
        r#"let value = b'"';"#,
        r#"let value = '\u{2f}';"#,
        r#"let value = b'\x2f';"#,
    ] {
        assert!(!has_rust_comment(source));
    }
    assert!(has_rust_comment(r#"let value = '"'; // hidden"#));
    assert!(has_rust_comment(r#"let value = b'"'; /* hidden */"#));
}

#[test]
fn package_is_registered_once_and_has_only_the_bounded_database_slice() {
    assert_eq!(WORKSPACE.matches("\"tools/starring-runtime\"").count(), 1);
    let sources = source_files();
    assert_eq!(
        sources
            .iter()
            .map(|(path, _)| path.to_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "src/config.rs",
            "src/database.rs",
            "src/lib.rs",
            "src/main.rs",
            "src/secret.rs",
            "tests/dependency_guard.rs",
            "tests/process_contract.rs"
        ]
    );
}

#[test]
fn direct_dependencies_are_the_exact_database_composition_surface() {
    let mut dependencies = package_dependencies()
        .into_iter()
        .map(|dependency| {
            assert!(dependency["rename"].is_null());
            (
                dependency["name"].as_str().unwrap().to_string(),
                dependency["kind"].as_str().map(str::to_string),
            )
        })
        .collect::<Vec<_>>();
    dependencies.sort();
    assert_eq!(
        dependencies,
        [
            ("automation-runtime-convergence-postgres".to_string(), None),
            ("automation-runtime-execution-postgres".to_string(), None),
            ("automation-runtime-interaction-postgres".to_string(), None),
            ("automation-runtime-panel-postgres".to_string(), None),
            ("automation-runtime-serving-postgres".to_string(), None),
            ("serde_json".to_string(), Some("dev".to_string())),
            ("sqlx".to_string(), None),
            ("thiserror".to_string(), None),
            ("tokio".to_string(), None),
            ("zeroize".to_string(), None),
        ]
    );
}

#[test]
fn source_is_comment_free_and_external_composition_is_bounded() {
    for (path, source) in source_files() {
        assert!(!has_rust_comment(&source), "{}", path.display());
        if !path.starts_with("src") {
            continue;
        }
        for forbidden in [
            "ai_gateway",
            "axum",
            "design_harness",
            "dotenv",
            "reqwest",
            "twilight_gateway",
            "twilight_http",
            "TcpListener",
        ] {
            assert!(
                !contains_identifier(&source, forbidden),
                "{}: {forbidden}",
                path.display()
            );
        }
        if path != Path::new("src/database.rs") {
            for forbidden in [
                "sqlx",
                "tokio",
                "PgPool",
                "PostgresRuntimeExecutionV1",
                "PostgresRuntimeExactTargetReader",
                "PostgresRuntimePanelV1",
                "PostgresRuntimeServingLeaseV1",
                "PostgresRuntimeInteractionV1",
            ] {
                assert!(
                    !contains_identifier(&source, forbidden),
                    "{}: {forbidden}",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn database_composition_is_five_pool_function_only_and_fail_closed() {
    let sources = source_files();
    let database = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/database.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    for required in [
        "PgConnectOptions::new_without_pgpass()",
        ".disable_statement_logging()",
        "tokio::join!",
        "timeout_at(startup_deadline",
        "PERIODIC_READINESS_TIMEOUT",
        "verify_expected_database_authority_v1",
        "observe_runtime_execution_database_identity_with_timeouts_v1",
        "PostgresRuntimeExecutionV1::connect_verified",
        "PostgresRuntimeExactTargetReader::connect_verified",
        "PostgresRuntimePanelV1::connect_verified",
        "PostgresRuntimeServingLeaseV1::connect_verified",
        "PostgresRuntimeInteractionV1::connect_verified_with_route_timeout",
        "close_pool_refs_with_deadline",
        "DatabaseCapabilityV1::ALL",
        "BTreeSet",
    ] {
        assert!(database.contains(required), "{required}");
    }
    for forbidden in [
        "sqlx::query",
        "sqlx::query_as",
        "sqlx::query_scalar",
        "PostgresRuntimeConvergenceDatabaseIdentityReader",
        "PostgresRuntimeConvergence",
        "pub fn panel_pool",
        "pub fn database_identity",
        "pub fn database_name",
        "pub fn executor_role",
        "SELECT ",
        "INSERT ",
        "UPDATE ",
        "DELETE ",
    ] {
        assert!(!database.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn secret_resolution_remains_bounded_redacted_and_adapter_free() {
    let sources = source_files();
    let secret = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/secret.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    for required in [
        "Command::new(\"/usr/bin/security\")",
        ".env_clear()",
        "KEYCHAIN_TIMEOUT",
        "KEYCHAIN_CAPTURE_BYTES",
        "terminate_and_reap",
        "child.kill()",
        "child.wait()",
        "Zeroizing<String>",
        "Zeroizing<Vec<u8>>",
        "Vec::with_capacity(KEYCHAIN_CAPTURE_BYTES)",
        "RuntimeDatabaseSecretsByCapabilityV1(<redacted>)",
        "RuntimeDatabasePasswordV1(<redacted>)",
        "RuntimeDiscordBotTokenV1(<redacted>)",
        "RuntimeDatabaseUrlSecretV1(<redacted>)",
    ] {
        assert!(secret.contains(required), "{required}");
    }
    for forbidden in [
        "sqlx",
        "PgPool",
        "twilight_http",
        "twilight_gateway",
        "Url",
        "into_zeroizing",
    ] {
        assert!(!contains_identifier(secret, forbidden), "{forbidden}");
    }
    for forbidden in [
        "pub type RuntimeDatabaseConnectionPartsV1",
        "pub fn into_zeroizing",
    ] {
        assert!(!secret.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn executable_stops_after_secret_resolution_and_cannot_claim_readiness() {
    let sources = source_files();
    let main = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/main.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    assert!(main.contains("RuntimeConfigV1::from_process_environment"));
    assert!(main.contains("resolve_runtime_secrets_v1"));
    assert!(main.contains("runtime_not_composed"));
    assert!(!main.contains("compose_runtime_database_dependencies_v1"));
    for forbidden in ["health_ready", "ready_to_serve", "gateway_connected"] {
        assert!(!main.contains(forbidden));
    }
}
