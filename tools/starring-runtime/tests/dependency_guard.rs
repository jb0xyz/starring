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
fn package_is_registered_once_and_has_only_the_bounded_first_slice() {
    assert_eq!(WORKSPACE.matches("\"tools/starring-runtime\"").count(), 1);
    let sources = source_files();
    assert_eq!(
        sources
            .iter()
            .map(|(path, _)| path.to_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "src/config.rs",
            "src/lib.rs",
            "src/main.rs",
            "tests/dependency_guard.rs",
            "tests/process_contract.rs"
        ]
    );
}

#[test]
fn direct_dependencies_exclude_runtime_adapters_ai_and_environment_loaders() {
    let dependencies = package_dependencies();
    assert_eq!(dependencies.len(), 1);
    let dependency = &dependencies[0];
    assert_eq!(dependency["name"], "serde_json");
    assert_eq!(dependency["kind"], "dev");
    assert!(dependency["rename"].is_null());
}

#[test]
fn source_is_comment_free_and_cannot_compose_external_systems() {
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
            "sqlx",
            "tokio",
            "twilight_gateway",
            "twilight_http",
            "PgPool",
            "TcpListener",
        ] {
            assert!(
                !contains_identifier(&source, forbidden),
                "{}: {forbidden}",
                path.display()
            );
        }
    }
}

#[test]
fn executable_stops_after_configuration_and_cannot_claim_readiness() {
    let sources = source_files();
    let main = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/main.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    assert!(main.contains("RuntimeConfigV1::from_process_environment"));
    assert!(main.contains("runtime_not_composed"));
    for forbidden in ["health_ready", "ready_to_serve", "gateway_connected"] {
        assert!(!main.contains(forbidden));
    }
}
