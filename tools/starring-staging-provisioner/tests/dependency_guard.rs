use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_source_files(&source_root, &source_root, &mut files);
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
        .find(|package| package["name"] == "starring-staging-provisioner")
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

#[test]
fn direct_dependencies_are_closed_to_the_staging_boundary() {
    let dependencies = package_dependencies();
    for dependency in dependencies {
        let name = dependency["name"].as_str().unwrap();
        assert!(
            matches!(
                name,
                "base64"
                    | "getrandom"
                    | "hmac"
                    | "serde"
                    | "serde_json"
                    | "sha2"
                    | "sqlx"
                    | "subtle"
                    | "thiserror"
                    | "tokio"
                    | "zeroize"
            ),
            "unreviewed direct dependency: {name}"
        );
        assert!(dependency["rename"].is_null());
    }
}

#[test]
fn source_keeps_fixed_process_and_secret_transport_boundaries() {
    let sources = source_files();
    for (path, source) in &sources {
        assert!(!has_rust_comment(source), "{}", path.display());
        for forbidden in [
            "PGPASSWORD",
            "DATABASE_URL",
            "password_file",
            "password-file",
            "psql",
            "Command::new(\"expect\")",
        ] {
            assert!(
                !source.contains(forbidden),
                "{}: {forbidden}",
                path.display()
            );
        }
    }
    let keychain = sources
        .iter()
        .find(|(path, _)| path == Path::new("keychain.rs"))
        .map(|(_, source)| source)
        .unwrap();
    assert_eq!(keychain.matches("Command::new(SECURITY_PATH)").count(), 3);
    assert!(keychain.contains("const SECURITY_PATH: &str = \"/usr/bin/security\""));
    assert!(keychain.contains("add-generic-password"));
    assert!(keychain.contains("\"find-generic-password\""));
    assert!(keychain.contains("\"delete-generic-password\""));
    assert!(keychain.contains(".arg(\"-i\")"));
    assert!(keychain.contains("input.extend_from_slice(b\" -X \")"));
    assert!(keychain.contains("input.extend_from_slice(b\"-U \")"));
    assert!(keychain.contains(".stdin(Stdio::piped())"));
    assert!(keychain.contains(".stdout(Stdio::null())"));
    assert!(keychain.contains(".stderr(Stdio::null())"));
}

#[test]
fn workspace_and_fixed_target_membership_are_present_once() {
    let workspace = include_str!("../../../Cargo.toml");
    assert_eq!(
        workspace
            .matches("\"tools/starring-staging-provisioner\"")
            .count(),
        1
    );
    let identity = include_str!("../src/identity.rs");
    assert_eq!(
        identity
            .matches("pub const APPLICATION_DATABASE_IDENTITIES")
            .count(),
        1
    );
    assert!(identity
        .contains("pub const PEER_SOCKET_DIRECTORY: &str = \"/private/tmp/starring-bootstrap\""));
}

#[test]
fn incremental_writer_mode_is_explicit_and_runbooked_once() {
    let main = include_str!("../src/main.rs");
    let incremental = include_str!("../src/incremental_writer.rs");
    let readme = include_str!("../README.md");
    let runbook = include_str!(
        "../../../docs/superpowers/runbooks/2026-07-29-macos-starring-integrated-staging-cutover.md"
    );
    assert_eq!(main.matches("\"--provision-authoring-writer\"").count(), 1);
    assert_eq!(incremental.matches("CREATE ROLE ").count(), 3);
    assert_eq!(incremental.matches("starring-api.staging").count(), 1);
    assert!(readme.contains("authoring_writer=exact_replay"));
    assert!(main.contains("snapshot_reader=v2_only"));
    assert!(runbook.contains("authoring-writer-created.txt"));
    assert!(runbook.contains("authoring-writer-replay.txt"));
    assert!(runbook.contains("Do not rerun the\none-shot provisioner"));
}
