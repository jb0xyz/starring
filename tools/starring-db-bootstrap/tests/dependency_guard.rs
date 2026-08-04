use std::fs;
use std::path::{Path, PathBuf};

fn source_files() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .collect::<Vec<_>>();
    files.sort();
    files
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
fn direct_dependencies_remain_narrow() {
    let manifest =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    for required in [
        "automation-instance-postgres",
        "libc",
        "sqlx",
        "thiserror",
        "tokio",
        "zeroize",
    ] {
        assert!(manifest.contains(required));
    }
    for forbidden in [
        "keyring",
        "reqwest",
        "axum",
        "twilight",
        "design-harness",
        "authoring-application",
    ] {
        assert!(!manifest.contains(forbidden));
    }
}

#[test]
fn keychain_process_and_secret_transport_are_fixed() {
    let keychain =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/keychain.rs")).unwrap();
    let production = keychain.split("#[cfg(test)]").next().unwrap();
    for required in [
        "const SECURITY_PATH: &str = \"/usr/bin/security\"",
        "starring.postgres.staging",
        "database.cluster-admin",
        "\"find-generic-password\"",
        "\"-w\"",
        ".env_clear()",
        ".stdin(Stdio::null())",
        ".stdout(Stdio::piped())",
        ".stderr(Stdio::null())",
        "Zeroizing<String>",
        "Zeroizing<Vec<u8>>",
        "COMMAND_TIMEOUT",
        "MAX_CAPTURE_BYTES",
    ] {
        assert!(production.contains(required), "{required}");
    }
    for forbidden in [
        ".env(",
        "PGPASSWORD",
        "DATABASE_URL",
        "println!",
        "eprintln!",
        "Command::new(\"sh\")",
        "Command::new(\"expect\")",
    ] {
        assert!(!production.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn command_modes_are_explicit_and_source_has_no_comments() {
    let main =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs")).unwrap();
    assert!(main.contains("\"--keychain-admin\""));
    assert!(main.contains("\"--peer-bootstrap\""));
    assert!(main.contains("read_interactive_admin_url()"));
    assert!(main.contains("read_admin_url_from_keychain()"));
    for path in source_files() {
        let source = fs::read_to_string(&path).unwrap();
        assert!(!has_rust_comment(&source), "{}", path.display());
    }
}
