use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const WORKSPACE: &str = include_str!("../../../Cargo.toml");
const CI_WORKFLOW: &str = include_str!("../../../.github/workflows/ci.yml");

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

fn declaration_attribute_block<'a>(source: &'a str, name: &str) -> &'a str {
    let markers = [format!("struct {name}"), format!("enum {name}")];
    let (declaration, marker) = markers
        .iter()
        .filter_map(|marker| source.find(marker).map(|declaration| (declaration, marker)))
        .min_by_key(|(declaration, _)| *declaration)
        .unwrap();
    let mut start = source[..declaration]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    loop {
        let before = source[..start].trim_end();
        if !before.ends_with(']') {
            break;
        }
        let Some(attribute) = before.rfind("#[") else {
            break;
        };
        start = attribute;
    }
    &source[start..declaration + marker.len()]
}

fn implements_trait(source: &str, name: &str, trait_name: &str) -> bool {
    let marker = format!(" for {name}");
    source.match_indices(&marker).any(|(end, _)| {
        source[..end]
            .rfind("impl ")
            .is_some_and(|start| contains_identifier(&source[start..end], trait_name))
    })
}

fn source_before_test_module(source: &str) -> &str {
    let marker = "#[cfg(test)]\nmod tests {";
    let (production, tests) = source
        .split_once(marker)
        .unwrap_or_else(|| panic!("test module boundary missing"));
    assert!(!tests.contains(marker), "duplicate test module boundary");
    production
}

fn braced_declaration<'a>(source: &'a str, marker: &str) -> &'a str {
    let start = source
        .find(marker)
        .unwrap_or_else(|| panic!("declaration missing: {marker}"));
    let open = source[start..]
        .find('{')
        .map(|offset| start + offset)
        .unwrap_or_else(|| panic!("declaration body missing: {marker}"));
    let close = matching_closing_brace(source, open)
        .unwrap_or_else(|| panic!("declaration body unterminated: {marker}"));
    &source[start..=close]
}

fn matching_closing_brace(source: &str, open: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if *bytes.get(open)? != b'{' {
        return None;
    }
    let mut depth = 1usize;
    let mut index = open + 1;
    while index < bytes.len() {
        if let Some(end) = raw_string_literal_end(bytes, index) {
            index = end;
            continue;
        }
        if bytes[index] == b'b' && bytes.get(index + 1) == Some(&b'"') {
            index = quoted_literal_end(bytes, index + 1)?;
            continue;
        }
        if bytes[index] == b'"' {
            index = quoted_literal_end(bytes, index)?;
            continue;
        }
        if bytes[index] == b'b' && bytes.get(index + 1) == Some(&b'\'') {
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
        match bytes[index] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

fn raw_string_literal_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut prefix = start;
    if bytes.get(prefix) == Some(&b'b') {
        prefix += 1;
    }
    if bytes.get(prefix) != Some(&b'r') {
        return None;
    }
    let mut delimiter = prefix + 1;
    while bytes.get(delimiter) == Some(&b'#') {
        delimiter += 1;
    }
    if bytes.get(delimiter) != Some(&b'"') {
        return None;
    }
    let hashes = delimiter - prefix - 1;
    let mut cursor = delimiter + 1;
    while cursor < bytes.len() {
        if bytes[cursor] == b'"'
            && cursor + hashes < bytes.len()
            && (hashes == 0
                || bytes[cursor + 1..=cursor + hashes]
                    .iter()
                    .all(|value| *value == b'#'))
        {
            return Some(cursor + hashes + 1);
        }
        cursor += 1;
    }
    None
}

fn quoted_literal_end(bytes: &[u8], quote: usize) -> Option<usize> {
    let mut cursor = quote + 1;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor = cursor.checked_add(2)?,
            b'"' => return Some(cursor + 1),
            _ => cursor += 1,
        }
    }
    None
}

fn assert_no_watch_reborrow(source: &str, context: &str) {
    for forbidden in [
        "current_admission_snapshot",
        "issue_ready_lease",
        "require_healthy_paused",
        ".control.",
        ".observer.",
    ] {
        assert!(!source.contains(forbidden), "{context}: {forbidden}");
    }
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
fn authority_type_scanner_crosses_blank_attribute_spacing_and_qualified_impls() {
    let derived = "#[derive(Debug, Clone)]\n\npub struct Guarded { value: usize }";
    assert!(contains_identifier(
        declaration_attribute_block(derived, "Guarded"),
        "Clone"
    ));
    let implemented = concat!(
        "pub struct Guarded { value: usize }\n",
        "impl std::default::Default for Guarded {\n",
        "    fn default() -> Self { Self { value: 0 } }\n",
        "}"
    );
    assert!(implements_trait(implemented, "Guarded", "Default"));
}

#[test]
fn production_source_scanner_crosses_test_only_items() {
    let source = concat!(
        "pub struct Before;\n",
        "#[cfg(test)]\n",
        "struct Fixture;\n",
        "pub struct After;\n",
        "#[cfg(test)]\n",
        "mod tests {}",
    );
    let production = source_before_test_module(source);
    assert!(production.contains("pub struct Before;"));
    assert!(production.contains("pub struct After;"));
    assert!(!production.contains("mod tests"));
}

#[test]
fn startup_observation_fixed_point_cannot_mint_gateway_owner_handoff_authority() {
    let watchdog = include_str!("../src/gateway_owner_startup_watchdog.rs");

    assert!(!watchdog.contains("RuntimeStartupRecoveryObservationFixedPointV2"));
    assert!(!watchdog.contains("plan_runtime_startup_recovery_v2"));
}

#[test]
fn database_readiness_retains_five_exact_receipts_without_serialization() {
    let database = include_str!("../src/database.rs");
    let declaration = database
        .split("pub struct RuntimeDatabaseReadinessV1 {")
        .nth(1)
        .and_then(|source| source.split("}\n\nimpl RuntimeDatabaseReadinessV1").next())
        .unwrap();

    for field in [
        "execution: RuntimeExecutionDatabaseReadinessV1",
        "exact_target: RuntimeExactTargetDatabaseReadinessV1",
        "panel: RuntimePanelDatabaseReadinessV1",
        "serving: RuntimeServingDatabaseReadinessV1",
        "interaction: RuntimeInteractionDatabaseReadinessV1",
        "capability_receipts: RuntimeCapabilityReadinessSetV2",
    ] {
        assert!(declaration.contains(field));
    }
    assert!(!declaration.contains("verified: bool"));
    assert!(database.contains("pub fn exact_capability_receipts(&self)"));
    assert_eq!(
        database.matches("use automation_runtime_worker::{").count(),
        1
    );
    let worker_import = database
        .split("use automation_runtime_worker::{")
        .nth(1)
        .and_then(|source| source.split("};").next())
        .unwrap();
    let imported = worker_import
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .filter(|identifier| !identifier.is_empty())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        imported,
        BTreeSet::from([
            "RuntimeCapabilityReadinessKindV2",
            "RuntimeCapabilityReadinessReceiptV2",
            "RuntimeCapabilityReadinessSetV2",
        ])
    );
    assert!(!database.contains("Serialize"));
    assert!(!database.contains("Deserialize"));
    let refresh = braced_declaration(
        database,
        "pub(crate) struct RuntimeDatabaseReadinessRefreshV2",
    );
    assert!(refresh.contains("readiness: RuntimeDatabaseReadinessV1"));
    let attributes = declaration_attribute_block(database, "RuntimeDatabaseReadinessRefreshV2");
    for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
        assert!(!contains_identifier(attributes, forbidden), "{forbidden}");
        assert!(!implements_trait(
            database,
            "RuntimeDatabaseReadinessRefreshV2",
            forbidden,
        ));
    }
    assert!(database.contains("RuntimeDatabaseReadinessRefreshV2(<redacted>)"));
    let verifier = braced_declaration(
        database,
        "pub(crate) async fn verify_readiness_refresh_until_v2(",
    );
    assert!(verifier.contains("self.verify_readiness_v1()"));
    assert!(verifier.contains("Instant::from_std(operation_cutoff)"));
    assert!(verifier.contains("RuntimeDatabaseCompositionErrorV1::ReadinessTimedOut"));
}

#[test]
fn package_is_registered_once_and_has_only_the_bounded_runtime_slice() {
    assert_eq!(WORKSPACE.matches("\"tools/starring-runtime\"").count(), 1);
    let sources = source_files();
    assert_eq!(
        sources
            .iter()
            .map(|(path, _)| path.to_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "src/build_revision.rs",
            "src/closed_recovery.rs",
            "src/config.rs",
            "src/controller_identity.rs",
            "src/database.rs",
            "src/gateway.rs",
            "src/gateway_owner_startup_watchdog.rs",
            "src/gateway_owner_startup_watchdog_handoff_tests.rs",
            "src/identity_encoding.rs",
            "src/lib.rs",
            "src/main.rs",
            "src/process.rs",
            "src/process_identity.rs",
            "src/process_startup.rs",
            "src/registry.rs",
            "src/secret.rs",
            "src/startup.rs",
            "src/startup_recovery_observation.rs",
            "tests/dependency_guard.rs",
            "tests/gateway_owner_startup_watchdog.rs",
            "tests/process_contract.rs"
        ]
    );
}

#[test]
fn direct_dependencies_are_the_exact_runtime_composition_surface() {
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
            ("automation-runtime".to_string(), None),
            ("automation-runtime-controller".to_string(), None),
            ("automation-runtime-convergence".to_string(), None),
            ("automation-runtime-convergence-postgres".to_string(), None),
            ("automation-runtime-execution-postgres".to_string(), None),
            ("automation-runtime-interaction-postgres".to_string(), None),
            ("automation-runtime-panel-postgres".to_string(), None),
            ("automation-runtime-registry".to_string(), None),
            ("automation-runtime-serving-postgres".to_string(), None),
            ("automation-runtime-worker".to_string(), None),
            ("chrono".to_string(), Some("dev".to_string())),
            ("getrandom".to_string(), None),
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
        if path != Path::new("src/database.rs")
            && path != Path::new("src/gateway.rs")
            && path != Path::new("src/gateway_owner_startup_watchdog.rs")
            && path != Path::new("src/gateway_owner_startup_watchdog_handoff_tests.rs")
            && path != Path::new("src/process_startup.rs")
            && path != Path::new("src/startup_recovery_observation.rs")
        {
            assert!(!contains_identifier(&source, "tokio"), "{}", path.display());
        }
        if path != Path::new("src/process_identity.rs")
            && path != Path::new("src/controller_identity.rs")
        {
            assert!(
                !contains_identifier(&source, "getrandom"),
                "{}",
                path.display()
            );
        }
        if path != Path::new("src/build_revision.rs") {
            assert!(!source.contains("option_env!"), "{}", path.display());
        }
    }
}

#[test]
fn startup_provenance_is_compile_time_canonical_and_nonforgeable() {
    let package_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(!package_root.join("build.rs").exists());
    let build_revision = include_str!("../src/build_revision.rs");
    let production = source_before_test_module(build_revision);
    for required in [
        "option_env!(\"STARRING_RUNTIME_BUILD_REVISION\")",
        "const GIT_REVISION_BYTES: usize = 40;",
        "pub(crate) struct CompiledRuntimeBuildRevisionV1 {",
        "revision: RuntimeBuildRevisionV1,",
        "pub(crate) fn bootstrap_compiled_runtime_build_revision_v1(",
        "pub(crate) fn into_revision(self) -> RuntimeBuildRevisionV1",
        "self.revision",
        "RuntimeBuildRevisionV1::parse(value)",
        "RuntimeBuildRevisionBootstrapErrorV1(<redacted>)",
        "CompiledRuntimeBuildRevisionV1(<redacted>)",
    ] {
        assert!(production.contains(required), "{required}");
    }
    for forbidden in [
        "std::env",
        "env::var",
        "Command::new",
        "std::process",
        "git rev-parse",
        "trim(",
        "to_ascii_lowercase",
        "STARRING_APPROVED_RELEASE_REVISION",
        "pub struct CompiledRuntimeBuildRevisionV1",
        "pub fn bootstrap_compiled_runtime_build_revision_v1",
        "fn revision(&self)",
    ] {
        assert!(!production.contains(forbidden), "{forbidden}");
    }
    let attributes = declaration_attribute_block(production, "CompiledRuntimeBuildRevisionV1");
    for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
        assert!(
            !contains_identifier(attributes, forbidden),
            "CompiledRuntimeBuildRevisionV1: {forbidden}"
        );
        assert!(
            !implements_trait(production, "CompiledRuntimeBuildRevisionV1", forbidden,),
            "CompiledRuntimeBuildRevisionV1: {forbidden}"
        );
    }
    assert!(CI_WORKFLOW.contains(concat!(
        "STARRING_RUNTIME_BUILD_REVISION: ",
        "\"${{ github.sha }}\""
    )));
    assert!(!CI_WORKFLOW.contains("STARRING_APPROVED_RELEASE_REVISION"));
    assert!(CI_WORKFLOW.contains("STARRING_RUNTIME_TEST_REQUIRE_COMPILED_REVISION: \"1\""));
}

#[test]
fn startup_identity_entropy_is_exact_independent_and_confined() {
    let encoding = source_before_test_module(include_str!("../src/identity_encoding.rs"));
    let process = source_before_test_module(include_str!("../src/process_identity.rs"));
    let controller = source_before_test_module(include_str!("../src/controller_identity.rs"));
    for required in [
        "pub(crate) const RUNTIME_IDENTITY_ENTROPY_BYTES: usize = 16;",
        "const LOWER_HEX: &[u8; 16] = b\"0123456789abcdef\";",
        "pub(crate) fn encode_runtime_identity_lower_hex_v1(",
        "String::with_capacity(RUNTIME_IDENTITY_ENTROPY_BYTES * 2)",
    ] {
        assert!(encoding.contains(required), "{required}");
    }
    assert!(!encoding.contains("getrandom"));
    for required in [
        "ProcessInstanceId::parse(encode_runtime_identity_lower_hex_v1(bytes))",
        "RuntimeProcessInstanceIdGenerationErrorV1(<redacted>)",
    ] {
        assert!(process.contains(required), "{required}");
    }
    for required in [
        "ControllerId::parse(encode_runtime_identity_lower_hex_v1(bytes))",
        "RuntimeControllerIdGenerationErrorV1(<redacted>)",
    ] {
        assert!(controller.contains(required), "{required}");
    }
    for forbidden in [
        "SystemTime",
        "Instant",
        "process::id",
        "hostname",
        "Uuid",
        "uuid",
        "/dev/urandom",
        "thread_rng",
        "StdRng",
        "getrandom::fill_uninit",
        "getrandom::u32",
        "getrandom::u64",
        "OnceLock",
        "LazyLock",
        "thread_local",
        "Atomic",
        "hash(",
        "xor",
    ] {
        assert!(!process.contains(forbidden), "process: {forbidden}");
        assert!(!controller.contains(forbidden), "controller: {forbidden}");
    }
    let process_wrapper = braced_declaration(
        process,
        "pub(crate) fn generate_runtime_process_instance_id_v1(",
    );
    let process_seam = braced_declaration(
        process,
        "fn generate_runtime_process_instance_id_with_v1<F>(",
    );
    let controller_wrapper = braced_declaration(
        controller,
        "pub(crate) fn generate_runtime_controller_id_v1(",
    );
    let controller_seam =
        braced_declaration(controller, "fn generate_runtime_controller_id_with_v1<F>(");
    for (wrapper, seam) in [
        (process_wrapper, process_seam),
        (controller_wrapper, controller_seam),
    ] {
        assert_eq!(wrapper.matches("getrandom::fill").count(), 1);
        assert_eq!(seam.matches("getrandom::fill").count(), 0);
        assert!(seam.contains("F: FnOnce("));
        assert!(seam.contains("&mut [u8; RUNTIME_IDENTITY_ENTROPY_BYTES]"));
        assert!(seam.contains("let mut bytes = [0_u8; RUNTIME_IDENTITY_ENTROPY_BYTES];"));
        assert_eq!(seam.matches("fill(&mut bytes)?;").count(), 1);
    }
    assert!(!process.contains("ControllerId"));
    assert!(!process.contains("controller_id"));
    assert!(!controller.contains("ProcessInstanceId"));
    assert!(!controller.contains("process_instance_id"));
    assert!(!process.contains("pub fn generate_runtime_process_instance_id_v1"));
    assert!(!controller.contains("pub fn generate_runtime_controller_id_v1"));
}

#[test]
fn gateway_v3_authority_is_confined_and_explicit_resume_is_mandatory() {
    let sources = source_files();
    let gateway = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/gateway.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    for (path, source) in sources.iter().filter(|(path, _)| path.starts_with("src")) {
        if path == Path::new("src/gateway.rs") {
            continue;
        }
        if path == Path::new("src/gateway_owner_startup_watchdog.rs")
            || path == Path::new("src/gateway_owner_startup_watchdog_handoff_tests.rs")
        {
            for identifier in source
                .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                .filter(|identifier| !identifier.is_empty())
            {
                assert!(
                    !identifier.ends_with("V3"),
                    "{}: {identifier}",
                    path.display()
                );
            }
            continue;
        }
        for identifier in source
            .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
            .filter(|identifier| !identifier.is_empty())
        {
            let allowed_readiness_worker =
                path == Path::new("src/database.rs") && identifier == "automation_runtime_worker";
            let allowed_registry_adapter = path == Path::new("src/registry.rs")
                && matches!(
                    identifier,
                    "automation_runtime_convergence"
                        | "automation_runtime_registry"
                        | "automation_runtime_worker"
                );
            let allowed_closed_recovery = path == Path::new("src/closed_recovery.rs")
                && matches!(
                    identifier,
                    "automation_runtime_controller" | "automation_runtime_worker"
                );
            let allowed_startup_observation = path
                == Path::new("src/startup_recovery_observation.rs")
                && identifier == "automation_runtime_worker";
            let allowed_build_revision = path == Path::new("src/build_revision.rs")
                && identifier == "automation_runtime_controller";
            let allowed_process_foundation = path == Path::new("src/process.rs")
                && matches!(
                    identifier,
                    "automation_runtime_controller"
                        | "automation_runtime_convergence"
                        | "automation_runtime_worker"
                );
            let allowed_process_identity = path == Path::new("src/process_identity.rs")
                && identifier == "automation_runtime_convergence";
            let allowed_controller_identity = path == Path::new("src/controller_identity.rs")
                && identifier == "automation_runtime_convergence";
            assert!(
                !identifier.ends_with("V3")
                    && (allowed_readiness_worker
                        || allowed_registry_adapter
                        || allowed_closed_recovery
                        || allowed_startup_observation
                        || allowed_build_revision
                        || allowed_process_foundation
                        || allowed_process_identity
                        || allowed_controller_identity
                        || !matches!(
                            identifier,
                            "automation_runtime"
                                | "automation_runtime_controller"
                                | "automation_runtime_convergence"
                                | "automation_runtime_worker"
                        )),
                "{}: {identifier}",
                path.display()
            );
        }
    }
    for required in [
        "shared_gateway_control_channel_with_policy_and_invalidator_v3",
        "GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect",
        "RuntimeGatewayClosedLifecycleV2::starting()",
        "impl GatewaySynchronousInvalidatorV3 for RuntimeGatewayInvalidationBridgeV2",
        "impl RuntimeGatewayOwnerEmergencyInvalidatorV1 for RuntimeGatewayOwnerInvalidationBridgeV2",
        "pub fn start_gateway_owner_startup_watchdog_v1<P>",
        "const SUPPORTED_GATEWAY_SHARD_ID: &str = \"shard:0\";",
        "RuntimeGatewayOwnerStartupWatchdogStartErrorV1::ProcessMismatch",
        "RuntimeGatewayOwnerStartupWatchdogStartErrorV1::ShardMismatch",
        "pub fn observe_paused_connected_gateway_v2(",
        "require_healthy_paused_control",
        "self.control.issue_ready_lease(epoch)",
        "RuntimePausedGatewayObservationV2::new(",
        "RuntimePausedGatewaySequenceV2::new(",
        "control.admission_snapshot_watch()",
        "pub(crate) struct RuntimeEmergencyGatewaySectionV2<'a>",
        "pub(crate) struct RuntimeRecoveryPendingGatewayBindingV2",
        "pub(crate) struct RuntimeRecoveryPendingGatewaySectionV2<'a>",
        "enum RuntimeGatewayOwnerRecoveryEvidenceV2<'a>",
        "Committed(&'a RuntimeGatewayOwnerClosedRecoverySupervisorV2)",
        "pub(crate) enum RuntimeGatewayRecoveryOwnerCommitErrorV2",
        "pub(crate) fn initial_emergency_gateway_section_v2<'a>(",
        "pub(crate) fn begin_empty_recovery_v2(",
        "_authority: &RuntimeClosedRecoveryTransitionAuthorityV2",
        "registry: RuntimeLockedRegistryEmptyEvidenceV2<'_, '_>",
        "registry.into_observation_v2()",
        "pub(crate) fn into_recovery_pending_binding_v2(",
        "pub(crate) fn pending_section_v2<'a>(",
        "pub(crate) fn committed_pending_section_v2<'a>(",
        "pub(crate) async fn commit_prepared_owner_v2(",
        "impl Drop for RuntimeEmergencyGatewaySectionV2<'_>",
        "impl Drop for RuntimeRecoveryPendingGatewayBindingV2",
        "RuntimeRecoveryPendingGatewayBindingV2(<redacted>)",
    ] {
        assert!(gateway.contains(required), "{required}");
    }
    for forbidden in [
        "shared_gateway_control_channel_v3",
        "shared_gateway_control_channel_with_policy_v3",
        "GatewayAdmissionPolicyV3::ResumeOnConnect",
        "run_shared_gateway_v3",
        "twilight_gateway",
        "twilight_http",
    ] {
        assert!(!gateway.contains(forbidden), "{forbidden}");
    }
    let production = source_before_test_module(gateway);
    let mut remainder = production;
    while let Some((_, public)) = remainder.split_once("pub ") {
        let header_end = public
            .find(['{', ';'])
            .unwrap_or_else(|| panic!("unterminated public declaration"));
        let header = &public[..header_end];
        for identifier in header
            .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
            .filter(|identifier| !identifier.is_empty())
        {
            assert!(
                !identifier.ends_with("V3")
                    && !matches!(
                        identifier,
                        "automation_runtime"
                            | "automation_runtime_controller"
                            | "automation_runtime_convergence"
                            | "automation_runtime_worker"
                    ),
                "public gateway declaration: {identifier}"
            );
        }
        remainder = &public[header_end + 1..];
    }
    let library = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/lib.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    assert!(library.contains("mod gateway;"));
    assert!(!library.contains("pub mod gateway;"));
    assert!(!library.contains("RuntimeGatewayOwnerEmergencyInvalidatorV1"));
    assert!(!library.contains("start_runtime_gateway_owner_startup_watchdog_v1"));
    let owner_supervisor = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/gateway_owner_startup_watchdog.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    assert!(owner_supervisor.contains("pub(crate) trait RuntimeGatewayOwnerEmergencyInvalidatorV1"));
    assert!(
        owner_supervisor.contains("pub(crate) fn start_runtime_gateway_owner_startup_watchdog_v1")
    );
    assert!(!owner_supervisor.contains("pub trait RuntimeGatewayOwnerEmergencyInvalidatorV1"));
    assert!(!owner_supervisor.contains("pub fn start_runtime_gateway_owner_startup_watchdog_v1"));
    assert!(owner_supervisor.contains(concat!(
        "pub struct RuntimeGatewayOwnerCurrentObservationV1 {\n",
        "    receipt: RuntimeGatewayOwnerLeaseReceiptV1,\n",
        "    safety_deadline: Instant,\n",
        "}"
    )));
    assert!(owner_supervisor.contains("shutdown_commands: mpsc::Sender<"));
    assert!(owner_supervisor.contains("supervisor_commands: mpsc::Sender<"));
    assert!(owner_supervisor.contains("closed_recovery_commands: mpsc::Sender<"));
    assert!(owner_supervisor.contains("RuntimeGatewayOwnerObservationCompletionV1::Current"));
    assert!(!owner_supervisor.contains("impl Clone for RuntimeGatewayOwnerStartupWatchdogHandleV1"));
    assert!(owner_supervisor.contains("impl Drop for RuntimeGatewayOwnerSupervisorHandleV1"));
    assert!(!owner_supervisor.contains("impl Drop for RuntimeGatewayOwnerStartupWatchdogHandleV1"));
    assert!(owner_supervisor.contains("pub(crate) async fn into_production_v1("));
    assert!(
        owner_supervisor.contains("pub(crate) struct RuntimeGatewayOwnerProductionHandoffProofV1")
    );
    assert!(
        owner_supervisor.contains("pub(crate) struct RuntimeGatewayOwnerProductionSupervisorV1")
    );
    assert!(owner_supervisor.contains("RuntimeGatewayOwnerSupervisorCommandV1::Promote"));
    assert!(!owner_supervisor.contains("RuntimeGatewayOwnerSupervisorCommandV1::Prepare"));
    assert!(!owner_supervisor.contains("RuntimeGatewayOwnerSupervisorCommandV1::Commit"));
    for required in [
        "pub(crate) async fn prepare_closed_recovery_v2(\n        mut self,",
        "pub(crate) async fn abort_and_shutdown_v2(\n        mut self,",
        "pub(crate) async fn commit_closed_recovery_v2(\n        mut self,",
        "const CLOSED_RECOVERY_COMMAND_CAPACITY: usize = 1;",
        "RuntimeGatewayOwnerClosedRecoveryCommandV2::Prepare",
        "RuntimeGatewayOwnerClosedRecoveryCommandV2::Commit",
        "RuntimeGatewayOwnerSupervisorRoleV1::PreparedClosedRecovery",
        "RuntimeGatewayOwnerSupervisorRoleV1::ClosedRecovery",
        "permit.owner_receipt() != self.observation.receipt()",
        "watchdog.schedule().receipt() != &expected_receipt",
        "RuntimeGatewayOwnerPreparedClosedRecoveryV2(<redacted>)",
        "RuntimeGatewayOwnerClosedRecoverySupervisorV2(<redacted>)",
        "gateway_lifetime: Arc<AtomicBool>",
        "Arc::ptr_eq(&self.gateway_lifetime, expected)",
        "pub(crate) fn is_bound_to_gateway_lifetime_v2(",
    ] {
        assert!(owner_supervisor.contains(required), "{required}");
    }
    assert_eq!(
        owner_supervisor
            .matches("pub(crate) fn is_bound_to_gateway_lifetime_v2(")
            .count(),
        2
    );
    assert_eq!(
        owner_supervisor.matches("runtime.spawn(async move").count(),
        1
    );
    assert!(!owner_supervisor.contains("tokio::spawn"));
    assert!(!owner_supervisor.contains("spawn_local"));
    assert!(owner_supervisor.contains(concat!(
        "pub struct RuntimeGatewayOwnerStartupWatchdogHandleV1 {\n",
        "    inner: Option<RuntimeGatewayOwnerSupervisorHandleV1>,\n",
        "}"
    )));
    assert!(owner_supervisor.contains(concat!(
        "pub(crate) struct RuntimeGatewayOwnerProductionHandoffProofV1 {\n",
        "    _private: (),\n",
        "}"
    )));
    assert!(owner_supervisor.contains(concat!(
        "pub(crate) struct RuntimeGatewayOwnerProductionSupervisorV1 {\n",
        "    inner: Option<RuntimeGatewayOwnerSupervisorHandleV1>,\n",
        "    handoff_observation: RuntimeGatewayOwnerCurrentObservationV1,\n",
        "}"
    )));
    assert_eq!(
        owner_supervisor
            .matches("RuntimeGatewayOwnerProductionHandoffProofV1 {")
            .count(),
        1
    );
    assert!(owner_supervisor.contains(concat!(
        "#[cfg(test)]\n",
        "#[path = \"gateway_owner_startup_watchdog_handoff_tests.rs\"]\n",
        "mod handoff_tests;"
    )));
    for name in [
        "RuntimeGatewayOwnerSupervisorHandleV1",
        "RuntimeGatewayOwnerStartupWatchdogHandleV1",
        "RuntimeGatewayOwnerProductionHandoffProofV1",
        "RuntimeGatewayOwnerProductionSupervisorV1",
        "RuntimeGatewayOwnerPreparedClosedRecoveryV2",
        "RuntimeGatewayOwnerClosedRecoverySupervisorV2",
    ] {
        let attributes = declaration_attribute_block(owner_supervisor, name);
        for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
            assert!(
                !contains_identifier(attributes, forbidden),
                "{name}: {forbidden}"
            );
            assert!(
                !implements_trait(owner_supervisor, name, forbidden),
                "{name}: {forbidden}"
            );
        }
    }
    let owner_evidence =
        braced_declaration(production, "enum RuntimeGatewayOwnerRecoveryEvidenceV2<'a>");
    for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
        assert!(
            !contains_identifier(owner_evidence, forbidden),
            "RuntimeGatewayOwnerRecoveryEvidenceV2: {forbidden}"
        );
        assert!(
            !implements_trait(
                production,
                "RuntimeGatewayOwnerRecoveryEvidenceV2",
                forbidden,
            ),
            "RuntimeGatewayOwnerRecoveryEvidenceV2: {forbidden}"
        );
    }
    for name in [
        "RuntimeEmergencyGatewaySectionV2",
        "RuntimeRecoveryPendingGatewayBindingV2",
        "RuntimeRecoveryPendingGatewaySectionV2",
    ] {
        let attributes = declaration_attribute_block(production, name);
        for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
            assert!(
                !contains_identifier(attributes, forbidden),
                "{name}: {forbidden}"
            );
            assert!(
                !implements_trait(production, name, forbidden),
                "{name}: {forbidden}"
            );
        }
    }
    for forbidden in [
        "pub fn permit",
        "pub fn control",
        "pub fn coordinator",
        "pub fn admission_snapshot",
        "pub fn owner_invalidated",
    ] {
        assert!(!production.contains(forbidden), "{forbidden}");
    }
    let library = include_str!("../src/lib.rs");
    for forbidden in [
        "RuntimeGatewayOwnerProductionHandoffProofV1",
        "RuntimeGatewayOwnerProductionSupervisorV1",
        "RuntimeGatewayOwnerProductionHandoffErrorV1",
        "RuntimeGatewayOwnerPreparedClosedRecoveryV2",
        "RuntimeGatewayOwnerClosedRecoverySupervisorV2",
        "RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2",
        "RuntimeGatewayOwnerClosedRecoveryCommitErrorV2",
        "RuntimeEmergencyGatewaySectionV2",
        "RuntimeRecoveryPendingGatewayBindingV2",
        "RuntimeRecoveryPendingGatewaySectionV2",
        "RuntimeGatewayRecoverySectionErrorV2",
        "RuntimeGatewayRecoveryOwnerCommitErrorV2",
        "RuntimeGatewayOwnerRecoveryEvidenceV2",
    ] {
        assert!(!library.contains(forbidden), "{forbidden}");
    }
    for forbidden in [
        "pub fn watchdog",
        "pub fn schedule",
        "pub fn port",
        "RuntimeGatewayOwnerObservedWatchdogV1",
        "start_runtime_gateway_owner_production",
        "start_gateway_owner_production",
    ] {
        assert!(!owner_supervisor.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn gateway_section_snapshot_guards_never_reborrow_a_live_watch_reference() {
    let gateway = include_str!("../src/gateway.rs");
    let production = source_before_test_module(gateway);
    let emergency = braced_declaration(production, "impl<'a> RuntimeEmergencyGatewaySectionV2<'a>");
    let acquire = braced_declaration(emergency, "fn acquire(");
    let acquire_borrow = acquire
        .find("admission_snapshot.borrow()")
        .unwrap_or_else(|| panic!("initial section watch borrow missing"));
    assert_no_watch_reborrow(&acquire[acquire_borrow..], "initial section after borrow");
    for marker in [
        "fn require_current_v2(&self)",
        "fn require_pending_current_v2(&self)",
    ] {
        assert_no_watch_reborrow(
            braced_declaration(emergency, marker),
            "initial section validation",
        );
    }

    let pending_binding =
        braced_declaration(production, "impl RuntimeRecoveryPendingGatewayBindingV2");
    let pending_section =
        braced_declaration(pending_binding, "fn pending_section_with_owner_v2<'a>(");
    let pending_borrow = pending_section
        .find("admission_snapshot.borrow()")
        .unwrap_or_else(|| panic!("pending section watch borrow missing"));
    assert_no_watch_reborrow(
        &pending_section[pending_borrow..],
        "pending section after borrow",
    );
    let owner_commit = braced_declaration(
        pending_binding,
        "pub(crate) async fn commit_prepared_owner_v2(",
    );
    assert!(owner_commit.contains("commit_cutoff: Instant"));
    let preflight = owner_commit
        .find(".pending_section_v2(&prepared_owner)")
        .unwrap();
    let section_drop = owner_commit.find("drop(section)").unwrap();
    let cutoff_guard = owner_commit
        .find("if Instant::now() >= commit_cutoff")
        .unwrap();
    let select = owner_commit.find("tokio::select!").unwrap();
    let biased = owner_commit.find("biased;").unwrap();
    let timer = owner_commit
        .find("sleep_until(TokioInstant::from_std(commit_cutoff))")
        .unwrap();
    let commit = owner_commit
        .find("prepared_owner.commit_closed_recovery_v2(&self.permit)")
        .unwrap();
    assert!(
        preflight < section_drop
            && section_drop < cutoff_guard
            && cutoff_guard < select
            && select < biased
            && biased < timer
            && timer < commit
    );
    assert_eq!(owner_commit.matches("tokio::select!").count(), 1);
    assert!(!owner_commit.contains(".await"));
    assert_eq!(
        production
            .matches("prepared_owner.commit_closed_recovery_v2(&self.permit)")
            .count(),
        1
    );
    let readiness = braced_declaration(
        pending_binding,
        "pub(crate) fn into_readiness_successor_v2(",
    );
    let readiness_preflight = readiness
        .find(".committed_pending_section_v2(committed_owner)")
        .unwrap();
    let readiness_preflight_drop = readiness.find("drop(section)").unwrap();
    let readiness_transition = readiness
        .find("coordinator.refresh_recovery_readiness(")
        .unwrap();
    let readiness_postflight = readiness
        .rfind(".committed_pending_section_v2(committed_owner)")
        .unwrap();
    assert!(
        readiness_preflight < readiness_preflight_drop
            && readiness_preflight_drop < readiness_transition
            && readiness_transition < readiness_postflight
    );
    assert!(!readiness.contains(".await"));
    let capability_failure = braced_declaration(
        pending_binding,
        "pub(crate) fn invalidate_capability_not_ready_v2(&self)",
    );
    assert!(capability_failure.contains(
        "self.invalidate_if_current_v2(RuntimeGatewayInvalidationCauseV2::CapabilityNotReady)"
    ));

    let pending = braced_declaration(
        production,
        "impl RuntimeRecoveryPendingGatewaySectionV2<'_>",
    );
    let exact_registry = braced_declaration(
        pending,
        "pub(crate) fn validate_empty_registry_projection_v2(",
    );
    let evidence_match = exact_registry
        .find("self.binding.permit.registry_evidence().empty_observation() != observation")
        .unwrap();
    let final_revalidation = exact_registry.find("self.require_current_v2()").unwrap();
    assert!(evidence_match < final_revalidation);
    let pending_current = braced_declaration(pending, "fn require_current_v2(&self)");
    assert_no_watch_reborrow(pending_current, "pending section validation");
    assert_eq!(
        pending_current
            .matches("require_recovery_owner_lifetime_v2(")
            .count(),
        2
    );
    assert_eq!(
        pending_current
            .matches(".validate_recovery_permit(&self.binding.permit)")
            .count(),
        2
    );
}

#[test]
fn closed_recovery_composition_is_private_fixed_order_and_non_authorizing() {
    let sources = source_files();
    let closed = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/closed_recovery.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    let production = closed;
    for required in [
        "pub(crate) fn begin_initial_empty_recovery_v2(",
        "pub(crate) struct RuntimeClosedRecoveryPendingPhaseV2",
        "RuntimeClosedRecoveryPendingPhaseV2(<redacted>)",
        "pub(crate) struct RuntimeClosedRecoverySessionV2",
        "RuntimeClosedRecoverySessionV2(<redacted>)",
        "pub(crate) struct RuntimeClosedRecoveryReadyIterationV2",
        "RuntimeClosedRecoveryReadyIterationV2(<redacted>)",
        "pub(crate) struct RuntimeClosedRecoveryFixedPointV2",
        "RuntimeClosedRecoveryFixedPointV2(<redacted>)",
        "pub(crate) enum RuntimeClosedRecoveryStartupIterationOutcomeV2",
        "RuntimeClosedRecoveryStartupIterationOutcomeV2(<redacted>)",
        "pub(crate) async fn commit_owner_v2(",
        "async fn commit_owner_with_post_commit_v2(",
        "pub(crate) async fn refresh_iteration_readiness_v2(",
        "async fn refresh_iteration_readiness_with_v2<Verify, Verification, PostRefresh>(",
        ".verify_readiness_refresh_until_v2(cutoff)",
        "operation_cutoff: Instant",
        ".operation_cutoff",
        ".min(self.owner.observation().safety_deadline())",
        "Instant::now() >= verification_cutoff",
        ".invalidate_capability_not_ready_v2()",
        ".into_readiness_successor_v2(",
        ".commit_prepared_owner_v2(&authority, owner, commit_cutoff)",
        "post_commit();",
        ".committed_pending_section_v2(owner)",
        "pub(crate) struct RuntimeClosedRecoveryTransitionAuthorityV2",
        "RuntimeClosedRecoveryTransitionAuthorityV2(<redacted>)",
        "let authority = RuntimeClosedRecoveryTransitionAuthorityV2 { _private: () };",
        ".initial_emergency_gateway_section_v2(&owner)",
        ".recovery_observation_guard_v2(&authority, &gateway_section)",
        ".locked_empty_evidence_v2()",
        "readiness: &RuntimeDatabaseReadinessV1",
        ".begin_empty_recovery_v2(",
        "readiness.exact_capability_receipts().clone()",
        ".into_empty_binding_v2()",
        ".into_recovery_pending_binding_v2()",
        ".pending_section_v2(&self.owner)",
        ".revalidate_empty_projection_v2(&section)",
        ".validate_empty_registry_projection_v2(&registry)",
    ] {
        assert!(production.contains(required), "{required}");
    }
    assert!(production.contains(concat!(
        "pub(crate) struct RuntimeClosedRecoveryPendingPhaseV2 {\n",
        "    owner: RuntimeGatewayOwnerPreparedClosedRecoveryV2,\n",
        "    gateway: RuntimeRecoveryPendingGatewayBindingV2,\n",
        "    registry: RuntimeRegistryEmptyRecoveryBindingV2,\n",
        "    operation_cutoff: Instant,\n",
        "}"
    )));
    assert!(production.contains(concat!(
        "pub(crate) struct RuntimeClosedRecoverySessionV2 {\n",
        "    owner: RuntimeGatewayOwnerClosedRecoverySupervisorV2,\n",
        "    gateway: RuntimeRecoveryPendingGatewayBindingV2,\n",
        "    registry: RuntimeRegistryEmptyRecoveryBindingV2,\n",
        "    operation_cutoff: Instant,\n",
        "}"
    )));
    assert!(production.contains(concat!(
        "pub(crate) struct RuntimeClosedRecoveryReadyIterationV2 {\n",
        "    owner: RuntimeGatewayOwnerClosedRecoverySupervisorV2,\n",
        "    gateway: RuntimeRecoveryPendingGatewayBindingV2,\n",
        "    registry: RuntimeRegistryEmptyRecoveryBindingV2,\n",
        "    operation_cutoff: Instant,\n",
        "    iteration: RuntimeAuthorizedStartupRecoveryIterationV2,\n",
        "}"
    )));
    assert!(production.contains(concat!(
        "pub(crate) struct RuntimeClosedRecoveryFixedPointV2 {\n",
        "    owner: RuntimeGatewayOwnerClosedRecoverySupervisorV2,\n",
        "    gateway: RuntimeRecoveryPendingGatewayBindingV2,\n",
        "    registry: RuntimeRegistryEmptyRecoveryBindingV2,\n",
        "    operation_cutoff: Instant,\n",
        "    proof: RuntimeStartupRecoveryFixedPointProofV2,\n",
        "}"
    )));
    assert!(production.contains(concat!(
        "pub(crate) enum RuntimeClosedRecoveryStartupIterationOutcomeV2 {\n",
        "    Continue {\n",
        "        session: RuntimeClosedRecoverySessionV2,\n",
        "        continuation: RuntimeStartupRecoveryContinuationV2,\n",
        "    },\n",
        "    FixedPoint(RuntimeClosedRecoveryFixedPointV2),\n",
        "}"
    )));
    let initial_gateway = production
        .find(".initial_emergency_gateway_section_v2(&owner)")
        .unwrap();
    let initial_registry = production
        .find(".recovery_observation_guard_v2(&authority, &gateway_section)")
        .unwrap();
    let locked_evidence = production.find(".locked_empty_evidence_v2()").unwrap();
    let transition = production.find(".begin_empty_recovery_v2(").unwrap();
    let registry_binding = production.find(".into_empty_binding_v2()").unwrap();
    let gateway_binding = production
        .find(".into_recovery_pending_binding_v2()")
        .unwrap();
    assert!(
        initial_gateway < initial_registry
            && initial_registry < locked_evidence
            && locked_evidence < transition
            && transition < registry_binding
            && registry_binding < gateway_binding
    );
    let final_gateway = production.find(".pending_section_v2(&self.owner)").unwrap();
    let final_registry = production
        .find(".revalidate_empty_projection_v2(&section)")
        .unwrap();
    assert!(final_gateway < final_registry);
    let pending_phase = braced_declaration(production, "impl RuntimeClosedRecoveryPendingPhaseV2");
    let owner_commit =
        braced_declaration(pending_phase, "async fn commit_owner_with_post_commit_v2(");
    let precommit = owner_commit.find("self.revalidate_v2()").unwrap();
    let cutoff = owner_commit
        .find(".min(self.owner.observation().safety_deadline())")
        .unwrap();
    let cutoff_guard = owner_commit
        .find("if Instant::now() >= commit_cutoff")
        .unwrap();
    let commit = owner_commit
        .find(".commit_prepared_owner_v2(&authority, owner, commit_cutoff)")
        .unwrap();
    let hook = owner_commit.find("post_commit();").unwrap();
    let session = owner_commit
        .find("let session = RuntimeClosedRecoverySessionV2")
        .unwrap();
    let postcommit = owner_commit.find("session.revalidate_v2()?").unwrap();
    assert!(
        precommit < cutoff
            && cutoff < cutoff_guard
            && cutoff_guard < commit
            && commit < hook
            && hook < session
            && session < postcommit
    );
    assert_eq!(
        pending_phase
            .matches("commit_owner_with_post_commit_v2(")
            .count(),
        2
    );
    let owner_commit_mapper =
        braced_declaration(production, "fn map_gateway_owner_commit_error_v2(");
    assert!(owner_commit_mapper.contains(concat!(
        "RuntimeGatewayRecoveryOwnerCommitErrorV2::DeadlineElapsed => {\n",
        "            RuntimeClosedRecoveryCommitErrorV2::DeadlineElapsed\n",
        "        }"
    )));
    let committed_session = braced_declaration(production, "impl RuntimeClosedRecoverySessionV2");
    let committed_revalidation = braced_declaration(committed_session, "fn revalidate_v2(&self)");
    assert!(committed_revalidation.contains("revalidate_committed_recovery_v2("));
    let shared_revalidation =
        braced_declaration(production, "fn revalidate_committed_recovery_v2(");
    let committed_deadline = shared_revalidation
        .find("Instant::now() >= operation_cutoff")
        .unwrap();
    let committed_gateway = shared_revalidation
        .find(".committed_pending_section_v2(owner)")
        .unwrap();
    let committed_registry = shared_revalidation
        .find(".revalidate_empty_projection_v2(&section)")
        .unwrap();
    let committed_final = shared_revalidation
        .find(".validate_empty_registry_projection_v2(&observation)")
        .unwrap();
    assert!(
        committed_deadline < committed_gateway
            && committed_gateway < committed_registry
            && committed_registry < committed_final
    );
    let readiness_refresh = braced_declaration(
        committed_session,
        "async fn refresh_iteration_readiness_with_v2<Verify, Verification, PostRefresh>(",
    );
    let refresh_prevalidation = readiness_refresh.find("self.revalidate_v2()").unwrap();
    let refresh_cutoff = readiness_refresh
        .find(".min(self.owner.observation().safety_deadline())")
        .unwrap();
    let refresh_await = readiness_refresh
        .find("verify(verification_cutoff).await")
        .unwrap();
    let refresh_pre_deadline = readiness_refresh
        .find("if Instant::now() >= verification_cutoff")
        .unwrap();
    let refresh_post_deadline = readiness_refresh
        .rfind("if Instant::now() >= verification_cutoff")
        .unwrap();
    let refresh_postvalidation = readiness_refresh[refresh_await + 1..]
        .find("self.revalidate_v2()")
        .map(|offset| offset + refresh_await + 1)
        .unwrap();
    let refresh_successor = readiness_refresh
        .find(".into_readiness_successor_v2(")
        .unwrap();
    let refresh_hook = readiness_refresh.find("post_refresh();").unwrap();
    let refresh_iteration = readiness_refresh
        .find("let iteration = RuntimeClosedRecoveryReadyIterationV2")
        .unwrap();
    let refresh_final = readiness_refresh
        .rfind("iteration\n            .revalidate_v2()")
        .unwrap();
    assert!(
        refresh_prevalidation < refresh_cutoff
            && refresh_cutoff < refresh_pre_deadline
            && refresh_pre_deadline < refresh_await
            && refresh_await < refresh_post_deadline
            && refresh_post_deadline < refresh_postvalidation
            && refresh_postvalidation < refresh_successor
            && refresh_successor < refresh_hook
            && refresh_hook < refresh_iteration
            && refresh_iteration < refresh_final
    );
    assert_eq!(
        readiness_refresh
            .matches("if Instant::now() >= verification_cutoff")
            .count(),
        3
    );
    assert!(!readiness_refresh.contains("initial_readiness"));
    let public_refresh = braced_declaration(
        committed_session,
        "pub(crate) async fn refresh_iteration_readiness_v2(",
    );
    assert!(!public_refresh.contains("operation_cutoff:"));
    let begin = braced_declaration(production, "pub(crate) fn begin_initial_empty_recovery_v2(");
    assert!(!begin.contains(".await"));
    let begin_deadline = begin.find("Instant::now() >= operation_cutoff").unwrap();
    let begin_gateway = begin
        .find(".initial_emergency_gateway_section_v2(&owner)")
        .unwrap();
    assert!(begin_deadline < begin_gateway);
    assert_eq!(owner_commit.matches(".await").count(), 1);
    assert_eq!(readiness_refresh.matches(".await").count(), 1);
    for forbidden in [
        "pub fn permit",
        "pub fn owner",
        "pub fn registry",
        "pub fn gateway",
    ] {
        assert!(!production.contains(forbidden), "{forbidden}");
    }
    for forbidden in [
        "tokio",
        "SharedGatewayControlV3",
        "GatewayAdmissionSnapshotV3",
        "ServingSlotRegistryV1",
        "RegistryRecoveryObservationGuardV2",
        "RegistryEmptyRecoveryCursorV2",
        "RuntimeClosedDrainRecoveryPermitV2",
        "activate",
        "deploy",
    ] {
        assert!(!contains_identifier(production, forbidden), "{forbidden}");
    }
    assert!(!production.contains(".resume"));
    assert!(!production.contains("resume("));
    for name in [
        "RuntimeClosedRecoveryPendingPhaseV2",
        "RuntimeClosedRecoverySessionV2",
        "RuntimeClosedRecoveryReadyIterationV2",
        "RuntimeClosedRecoveryFixedPointV2",
        "RuntimeClosedRecoveryStartupIterationOutcomeV2",
        "RuntimeClosedRecoveryTransitionAuthorityV2",
    ] {
        let attributes = declaration_attribute_block(production, name);
        for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
            assert!(
                !contains_identifier(attributes, forbidden),
                "{name}: {forbidden}"
            );
            assert!(
                !implements_trait(production, name, forbidden),
                "{name}: {forbidden}"
            );
        }
    }
    for (path, source) in sources.iter().filter(|(path, _)| path.starts_with("src")) {
        for method in [
            "recovery_observation_guard_v2",
            "locked_empty_evidence_v2",
            "revalidate_empty_projection_v2",
        ] {
            if contains_identifier(source, method) {
                assert!(
                    path == Path::new("src/closed_recovery.rs")
                        || path == Path::new("src/registry.rs"),
                    "{}: {method}",
                    path.display()
                );
            }
        }
        if contains_identifier(source, "begin_empty_recovery_v2") {
            assert!(
                path == Path::new("src/closed_recovery.rs") || path == Path::new("src/gateway.rs"),
                "{}: begin_empty_recovery_v2",
                path.display()
            );
        }
        if contains_identifier(source, "into_observation_v2") {
            assert!(
                path == Path::new("src/gateway.rs") || path == Path::new("src/registry.rs"),
                "{}: into_observation_v2",
                path.display()
            );
        }
        for method in ["commit_prepared_owner_v2", "committed_pending_section_v2"] {
            if contains_identifier(source, method) {
                assert!(
                    path == Path::new("src/closed_recovery.rs")
                        || path == Path::new("src/gateway.rs"),
                    "{}: {method}",
                    path.display()
                );
            }
        }
        for method in [
            "verify_readiness_refresh_until_v2",
            "into_exact_capability_receipts",
        ] {
            if contains_identifier(source, method) {
                assert!(
                    path == Path::new("src/closed_recovery.rs")
                        || path == Path::new("src/database.rs"),
                    "{}: {method}",
                    path.display()
                );
            }
        }
        for method in [
            "into_readiness_successor_v2",
            "invalidate_capability_not_ready_v2",
        ] {
            if contains_identifier(source, method) {
                assert!(
                    path == Path::new("src/closed_recovery.rs")
                        || path == Path::new("src/gateway.rs")
                        || path == Path::new("src/startup_recovery_observation.rs"),
                    "{}: {method}",
                    path.display()
                );
            }
        }
        for method in [
            "begin_startup_recovery_observation_v2",
            "into_startup_recovery_observation_successor_v2",
        ] {
            if contains_identifier(source, method) {
                assert!(
                    path == Path::new("src/gateway.rs")
                        || path == Path::new("src/startup_recovery_observation.rs"),
                    "{}: {method}",
                    path.display()
                );
            }
        }
    }
    assert!(!production.contains("pub(crate) fn new"));
    assert_eq!(
        production
            .matches("RuntimeClosedRecoveryTransitionAuthorityV2 { _private: () }")
            .count(),
        2
    );
    let library = include_str!("../src/lib.rs");
    assert!(library.contains("mod closed_recovery;"));
    assert!(!library.contains("pub mod closed_recovery;"));
    for forbidden in [
        "begin_initial_empty_recovery_v2",
        "RuntimeClosedRecoveryPendingPhaseV2",
        "RuntimeClosedRecoveryBeginErrorV2",
        "RuntimeClosedRecoverySessionV2",
        "RuntimeClosedRecoveryCommitErrorV2",
        "RuntimeClosedRecoveryReadinessRefreshErrorV2",
        "RuntimeClosedRecoveryReadyIterationV2",
        "RuntimeClosedRecoveryFixedPointV2",
        "RuntimeClosedRecoveryStartupIterationOutcomeV2",
        "RuntimeDatabaseReadinessRefreshV2",
    ] {
        assert!(!library.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn startup_recovery_observation_is_private_linear_deadline_bound_and_non_authorizing() {
    let sources = source_files();
    let observation = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/startup_recovery_observation.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    let closed = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/closed_recovery.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    let gateway = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/gateway.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();

    for required in [
        "pub(crate) enum RuntimeClosedRecoveryStartupObservationErrorV2<E>",
        "pub(crate) async fn observe_startup_recovery_v2<P>(",
        "async fn observe_startup_recovery_with_v2<",
        ".operation_cutoff",
        ".min(self.owner.observation().safety_deadline())",
        ".begin_startup_recovery_observation_v2(&owner, iteration)",
        "biased;",
        "sleep_until(TokioInstant::from_std(observation_cutoff))",
        "result = observe(authorization, observation_cutoff)",
        ".invalidate_capability_not_ready_v2()",
        ".into_startup_recovery_observation_successor_v2(&owner, completed)",
        ".validate_startup_recovery_fixed_point_v2(&owner, &proof)",
        "post_complete();",
        "post_revalidate();",
        "RuntimeClosedRecoveryStartupObservationErrorV2(<redacted>)",
        "RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed",
    ] {
        assert!(observation.contains(required), "{required}");
    }
    let public_method = braced_declaration(
        observation,
        "pub(crate) async fn observe_startup_recovery_v2<P>(",
    );
    let signature = public_method.split("where").next().unwrap();
    assert!(!signature.contains("operation_cutoff"));
    assert!(!signature.contains("Instant"));
    assert_eq!(
        observation
            .matches("observe(authorization, observation_cutoff)")
            .count(),
        1
    );
    assert_eq!(observation.matches("biased;").count(), 1);
    assert_eq!(
        observation
            .matches("Instant::now() >= observation_cutoff")
            .count(),
        10
    );
    let inner = braced_declaration(observation, "async fn observe_startup_recovery_with_v2<");
    let initial_revalidation = inner.find("self.revalidate_v2()").unwrap();
    let cutoff = inner.find("let observation_cutoff").unwrap();
    let begin = inner
        .find(".begin_startup_recovery_observation_v2(&owner, iteration)")
        .unwrap();
    let await_observation = inner
        .find("result = observe(authorization, observation_cutoff)")
        .unwrap();
    let revalidations = inner
        .match_indices("revalidate_committed_recovery_v2(")
        .map(|(position, _)| position)
        .collect::<Vec<_>>();
    assert_eq!(revalidations.len(), 3);
    let successor = inner
        .find(".into_startup_recovery_observation_successor_v2(&owner, completed)")
        .unwrap();
    let post_complete = inner.find("post_complete();").unwrap();
    let post_revalidate = inner.rfind("post_revalidate();").unwrap();
    let outcome = inner.find("match outcome").unwrap();
    let fixed_validation = inner
        .find(".validate_startup_recovery_fixed_point_v2(&owner, &proof)")
        .unwrap();
    let fixed_construction = inner
        .find("let fixed_point = RuntimeClosedRecoveryFixedPointV2")
        .unwrap();
    let fixed_revalidation = inner
        .find("fixed_point\n                    .revalidate_v2()")
        .unwrap();
    assert!(initial_revalidation < cutoff && cutoff < begin && begin < await_observation);
    assert!(
        await_observation < revalidations[0]
            && revalidations[0] < successor
            && successor < post_complete
            && post_complete < revalidations[1]
            && revalidations[1] < post_revalidate
            && post_revalidate < revalidations[2]
            && revalidations[2] < outcome
            && outcome < fixed_validation
            && fixed_validation < fixed_construction
            && fixed_construction < fixed_revalidation
    );
    let production_observation = observation
        .split("#[cfg(test)]\nimpl RuntimeClosedRecoverySessionV2")
        .next()
        .unwrap();
    assert!(production_observation.contains("impl RuntimeClosedRecoveryReadyIterationV2"));
    assert!(!production_observation.contains("impl RuntimeClosedRecoverySessionV2"));
    assert!(!observation.contains("RuntimeStartupRecoveryDecisionV2"));
    assert!(!closed.contains("RuntimeStartupRecoveryDecisionV2"));
    assert!(!observation.contains("(Self, RuntimeStartupRecoveryDecisionV2)"));
    assert!(observation.contains(concat!(
        "RuntimeClosedRecoveryStartupIterationOutcomeV2::Continue {\n",
        "                    session: RuntimeClosedRecoverySessionV2 {"
    )));
    assert!(observation.contains(concat!(
        "RuntimeClosedRecoveryStartupIterationOutcomeV2::FixedPoint(\n",
        "                    fixed_point,"
    )));
    for forbidden in [
        "recover_next_stale_live",
        "activate",
        "deploy",
        "resume",
        "RuntimeExecutionConvergencePort",
        "RuntimeClosedDrainRecoveryPermitV2",
        "sqlx",
        "PgPool",
        "MutexGuard",
        "RegistryRecoveryObservationGuardV2",
    ] {
        assert!(!contains_identifier(observation, forbidden), "{forbidden}");
    }

    let begin_gateway = braced_declaration(
        gateway,
        "pub(crate) fn begin_startup_recovery_observation_v2(",
    );
    let begin_preflight = begin_gateway
        .find("self.committed_pending_section_v2(committed_owner)")
        .unwrap();
    let begin_transition = begin_gateway
        .find("coordinator.begin_startup_recovery_observation(&mut self.permit, iteration)")
        .unwrap();
    let begin_postflight = begin_gateway
        .rfind("self.committed_pending_section_v2(committed_owner)")
        .unwrap();
    assert!(begin_preflight < begin_transition && begin_transition < begin_postflight);
    let complete_gateway = braced_declaration(
        gateway,
        "pub(crate) fn into_startup_recovery_observation_successor_v2(",
    );
    assert!(complete_gateway
        .contains(".complete_startup_recovery_observation(&mut self.permit, completed)"));
    assert!(complete_gateway.contains("self.owner_invalidated.store(true, Ordering::Release)"));
    let fixed_gateway = braced_declaration(
        gateway,
        "pub(crate) fn validate_startup_recovery_fixed_point_v2(",
    );
    let fixed_preflight = fixed_gateway
        .find("self.committed_pending_section_v2(committed_owner)")
        .unwrap();
    let fixed_transition = fixed_gateway
        .find("coordinator.validate_startup_recovery_fixed_point(&self.permit, proof)")
        .unwrap();
    let fixed_postflight = fixed_gateway
        .rfind("self.committed_pending_section_v2(committed_owner)")
        .unwrap();
    assert!(fixed_preflight < fixed_transition && fixed_transition < fixed_postflight);

    let library = include_str!("../src/lib.rs");
    assert!(closed.contains("mod startup_recovery_observation;"));
    assert!(!closed.contains("pub mod startup_recovery_observation;"));
    assert!(!library.contains("mod startup_recovery_observation;"));
    for forbidden in [
        "RuntimeClosedRecoveryStartupObservationErrorV2",
        "observe_startup_recovery_v2",
    ] {
        assert!(!library.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn registry_adapter_is_non_authorizing_fixed_and_confined() {
    let sources = source_files();
    let registry = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/registry.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    let production = registry.split("#[cfg(test)]").next().unwrap();

    for (path, source) in sources.iter().filter(|(path, _)| path.starts_with("src")) {
        if path != Path::new("src/registry.rs") {
            assert!(
                !contains_identifier(source, "automation_runtime_registry"),
                "{}",
                path.display()
            );
            for forbidden in [
                "RegistryRecoveryObservationGuardV2",
                "RegistryEmptyRecoveryCursorV2",
            ] {
                assert!(
                    !contains_identifier(source, forbidden),
                    "{}: {forbidden}",
                    path.display()
                );
            }
        }
    }
    for required in [
        "const REGISTRY_MAX_SLOTS: NonZeroU32 = NonZeroU32::new(4_096).unwrap();",
        "const REGISTRY_MAX_RETIRED_ROUTES_PER_SLOT: NonZeroU32 = NonZeroU32::new(8).unwrap();",
        "max_active_interactions_per_slot",
        "pub fn compose_runtime_registry_bootstrap_v1(",
        "pub fn observe_recovery_empty_projection_v2(",
        "pub(crate) fn recovery_observation_guard_v2(",
        "_authority: &RuntimeClosedRecoveryTransitionAuthorityV2",
        "_section: &RuntimeEmergencyGatewaySectionV2<'_>",
        "fn recovery_observation_guard_unordered_v2(",
        "pub(crate) struct RuntimeRegistryRecoveryGuardV1<'a>",
        "pub(crate) fn locked_empty_evidence_v2<'evidence>(",
        "pub(crate) struct RuntimeLockedRegistryEmptyEvidenceV2<'evidence, 'registry>",
        "pub(crate) fn into_observation_v2(self)",
        "RuntimeLockedRegistryEmptyEvidenceV2(<redacted>)",
        "pub(crate) fn into_empty_binding_v2(",
        "pub(crate) struct RuntimeRegistryEmptyRecoveryBindingV2",
        "pub(crate) fn revalidate_empty_projection_v2(",
        "_section: &RuntimeRecoveryPendingGatewaySectionV2<'_>",
        "fn revalidate_empty_projection_unordered_v2(",
        "RuntimeRegistryRecoveryObservationInputV2 {",
        "observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(",
        "retained_slot_count: observation.retained_slot_count()",
        "retained_empty_tombstone_count: observation.retained_empty_tombstone_count()",
        "staged_route_count: observation.staged_route_count()",
        "serving_route_count: observation.serving_route_count()",
        "draining_route_count: observation.draining_route_count()",
        "sealed_slot_count: observation.sealed_slot_count()",
        "active_interaction_count: observation.active_interaction_count()",
        "failed_closed_slot_count: observation.failed_closed_slot_count()",
        "registry_failed_closed: observation.registry_failed_closed()",
        "RuntimeRegistryBootstrapV1(<redacted>)",
        "RuntimeRegistryEmptyRecoveryBindingV2(<redacted>)",
    ] {
        assert!(production.contains(required), "{required}");
    }
    for forbidden in [
        "ServingSlotRegistryConfigV1::default",
        "pub fn registry",
        "pub fn into_registry",
        "pub fn recovery_observation_guard_v2",
        "pub fn revalidate_empty_recovery_cursor_v2",
        "pub fn into_empty_cursor",
        "pub(crate) fn recovery_observation_guard_unordered_v2",
        "pub(crate) fn revalidate_empty_projection_unordered_v2",
        "Serialize",
        "Deserialize",
    ] {
        assert!(!production.contains(forbidden), "{forbidden}");
    }
    for name in [
        "RuntimeRegistryBootstrapV1",
        "RuntimeRegistryRecoveryGuardV1",
        "RuntimeLockedRegistryEmptyEvidenceV2",
        "RuntimeRegistryEmptyRecoveryBindingV2",
    ] {
        let attributes = declaration_attribute_block(production, name);
        for forbidden in ["Clone", "Copy", "Default"] {
            assert!(
                !contains_identifier(attributes, forbidden),
                "{name}: {forbidden}"
            );
            assert!(
                !implements_trait(production, name, forbidden),
                "{name}: {forbidden}"
            );
        }
    }
    assert!(production.contains(concat!(
        "pub struct RuntimeRegistryBootstrapV1 {\n",
        "    process_instance_id: ProcessInstanceId,\n",
        "    registry: ServingSlotRegistryV1,\n",
        "}"
    )));
    assert!(production.contains("pub(crate) struct RuntimeRegistryRecoveryGuardV1<'a> {"));
    assert!(production.contains(concat!(
        "pub(crate) struct RuntimeLockedRegistryEmptyEvidenceV2<'evidence, 'registry> {\n",
        "    observation: RuntimeRegistryRecoveryEmptyObservationV2,\n",
        "    _guard: &'evidence RuntimeRegistryRecoveryGuardV1<'registry>,\n",
        "}"
    )));
    assert!(production.contains(concat!(
        "pub(crate) struct RuntimeRegistryEmptyRecoveryBindingV2 {\n",
        "    process_instance_id: ProcessInstanceId,\n",
        "    registry: ServingSlotRegistryV1,\n",
        "    cursor: RegistryEmptyRecoveryCursorV2,\n",
        "}"
    )));
    assert_eq!(
        production
            .matches("        Ok(RuntimeRegistryEmptyRecoveryBindingV2 {")
            .count(),
        1
    );
    for forbidden in [
        "pub fn cursor",
        "pub fn registry",
        "pub fn bootstrap",
        "pub fn into_cursor",
        "pub fn into_registry",
    ] {
        assert!(!production.contains(forbidden), "{forbidden}");
    }
    let mut public_surface = production;
    while let Some((_, public)) = public_surface.split_once("pub ") {
        let header_end = public
            .find(['{', ';'])
            .unwrap_or_else(|| panic!("unterminated public registry declaration"));
        let header = &public[..header_end];
        for forbidden in [
            "ServingSlotRegistryV1",
            "RegistryRecoveryObservationGuardV2",
            "RegistryEmptyRecoveryCursorV2",
            "RuntimeRegistryRecoveryGuardV1",
            "RuntimeRegistryEmptyRecoveryBindingV2",
        ] {
            assert!(
                !contains_identifier(header, forbidden),
                "{header}: {forbidden}"
            );
        }
        public_surface = &public[header_end + 1..];
    }
    let library = include_str!("../src/lib.rs");
    for forbidden in [
        "ServingSlotRegistryV1",
        "RegistryRecoveryObservationGuardV2",
        "RegistryEmptyRecoveryCursorV2",
        "RuntimeRegistryRecoveryGuardV1",
        "RuntimeLockedRegistryEmptyEvidenceV2",
        "RuntimeRegistryEmptyRecoveryBindingV2",
    ] {
        assert!(!library.contains(forbidden), "{forbidden}");
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
    let library = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/lib.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    for required in [
        "pub(crate) async fn compose_runtime_database_dependencies_v1(",
        "PgConnectOptions::new_without_pgpass()",
        ".disable_statement_logging()",
        "tokio::join!",
        "biased;",
        "sleep_until(",
        "TokioInstant::from_std(startup_budget.operation_cutoff())",
        "TokioInstant::from_std(startup_budget.cleanup_deadline())",
        "begin_before_operation_cutoff_v1",
        "RuntimeDatabasePoolConnectErrorV1::DeadlineElapsed",
        "RuntimeDatabasePoolConnectErrorV1::CleanupTimedOut",
        "RuntimeDatabaseCompositionErrorV1::StartupCleanupTimedOut",
        "map_database_startup_cleanup_result_v1",
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
        "STARTUP_READINESS_TIMEOUT",
        "RuntimeStartupBudgetV1::begin()",
    ] {
        assert!(!database.contains(forbidden), "{forbidden}");
    }
    assert!(!library.contains("compose_runtime_database_dependencies_v1"));
    let attributes = declaration_attribute_block(database, "RuntimeDatabaseDependenciesV1");
    assert!(!contains_identifier(attributes, "Clone"));
    assert!(!implements_trait(
        database,
        "RuntimeDatabaseDependenciesV1",
        "Clone",
    ));

    let connect = braced_declaration(database, "async fn connect_pool_v1(");
    let guard = connect
        .find("begin_before_operation_cutoff_v1(operation_cutoff")
        .unwrap();
    let future = connect.find("pool.connect_with(options)").unwrap();
    let precheck = connect
        .find("classify_database_connection_deadline_v1(")
        .unwrap();
    let timer = connect.find("tokio::select!").unwrap();
    let biased = connect[timer..].find("biased;").unwrap() + timer;
    let timer_branch = connect[timer..]
        .find("_ = sleep_until(TokioInstant::from_std(acquire_deadline))")
        .unwrap()
        + timer;
    let work_branch = connect[timer..].find("result = connect").unwrap() + timer;
    let postcheck = connect
        .rfind("classify_database_connection_deadline_v1(")
        .unwrap();
    let success = connect.rfind("Ok(pool) => Ok(pool)").unwrap();
    assert!(
        guard < future
            && future < precheck
            && precheck < timer
            && timer < biased
            && biased < timer_branch
            && timer_branch < work_branch
            && work_branch < postcheck
            && postcheck < success
    );

    let composer = braced_declaration(
        database,
        "pub(crate) async fn compose_runtime_database_dependencies_v1(",
    );
    let build = composer
        .find("let build = build_verified_dependencies_v1")
        .unwrap();
    let wait = composer.find("let result = tokio::select!").unwrap();
    let biased = composer[wait..].find("biased;").unwrap() + wait;
    let timer_branch = composer[wait..]
        .find("_ = sleep_until(TokioInstant::from_std(startup_budget.operation_cutoff()))")
        .unwrap()
        + wait;
    let work_branch = composer[wait..].find("result = build").unwrap() + wait;
    let postcheck = composer
        .find("let operation_is_open = startup_budget.operation_is_open()")
        .unwrap();
    let success = composer.find("return Ok(dependencies)").unwrap();
    let cleanup = composer
        .find("map_database_startup_cleanup_result_v1(primary, cleanup)")
        .unwrap();
    assert!(
        build < wait
            && wait < biased
            && biased < timer_branch
            && timer_branch < work_branch
            && work_branch < postcheck
            && postcheck < success
            && success < cleanup
    );

    let hard_cleanup = braced_declaration(database, "async fn close_pool_refs_until(");
    let close = hard_cleanup
        .find("let close = begin_pool_closures(pools)")
        .unwrap();
    let precheck = hard_cleanup
        .find("if TokioInstant::now() >= deadline")
        .unwrap();
    let timer = hard_cleanup.find("tokio::select!").unwrap();
    let biased = hard_cleanup[timer..].find("biased;").unwrap() + timer;
    let timer_branch = hard_cleanup[timer..]
        .find("_ = sleep_until(deadline)")
        .unwrap()
        + timer;
    let work_branch = hard_cleanup[timer..].find("() = &mut close").unwrap() + timer;
    let postcheck = hard_cleanup
        .find("if TokioInstant::now() < deadline")
        .unwrap();
    assert!(
        close < precheck
            && precheck < timer
            && timer < biased
            && biased < timer_branch
            && timer_branch < work_branch
            && work_branch < postcheck
    );
}

#[test]
fn startup_budget_is_single_origin_linear_monotonic_and_partitioned() {
    let sources = source_files();
    let startup = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/startup.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    let production = source_before_test_module(startup);
    let begin = braced_declaration(production, "pub(crate) fn begin()");
    let sync_stage = braced_declaration(
        production,
        "pub(crate) fn run_runtime_startup_sync_stage_v1<T, E, C, S>(",
    );

    for required in [
        "const STARTUP_OPERATION_WINDOW: Duration = Duration::from_secs(35);",
        "const STARTUP_TOTAL_WINDOW: Duration = Duration::from_secs(45);",
        "pub(crate) struct RuntimeStartupBudgetV1 {",
        "operation_cutoff: started_at + STARTUP_OPERATION_WINDOW",
        "cleanup_deadline: started_at + STARTUP_TOTAL_WINDOW",
        "now < self.operation_cutoff",
        "pub(crate) enum RuntimeStartupSyncStageErrorV1<E>",
        "RuntimeStartupSyncStageErrorV1::OperationDeadlineElapsed",
        "RuntimeStartupSyncStageErrorV1::Stage",
        "RuntimeStartupBudgetV1(<redacted>)",
    ] {
        assert!(production.contains(required), "{required}");
    }
    assert_eq!(begin.matches("Instant::now()").count(), 1);
    let checks = sync_stage
        .match_indices("if !operation_is_open()")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let stage = sync_stage.find("let result = stage();").unwrap();
    let result = sync_stage
        .find("result.map_err(RuntimeStartupSyncStageErrorV1::Stage)")
        .unwrap();
    assert_eq!(checks.len(), 2);
    assert!(checks[0] < stage && stage < checks[1] && checks[1] < result);
    assert_eq!(sync_stage.matches("stage()").count(), 1);
    let begin_origins = sources
        .iter()
        .filter(|(path, _)| path.starts_with("src"))
        .flat_map(|(path, source)| {
            let production = source.split("#[cfg(test)]\nmod tests {").next().unwrap();
            production
                .match_indices("RuntimeStartupBudgetV1::begin()")
                .map(move |_| path.as_path())
        })
        .collect::<Vec<_>>();
    assert_eq!(begin_origins, [Path::new("src/process_startup.rs")]);
    assert!(!contains_identifier(production, "SystemTime"));
    assert!(!contains_identifier(production, "tokio"));
    for forbidden in [
        "pub struct RuntimeStartupBudgetV1",
        "pub enum RuntimeStartupSyncStageErrorV1",
        "pub fn run_runtime_startup_sync_stage_v1",
    ] {
        assert!(!production.contains(forbidden), "{forbidden}");
    }
    let attributes = declaration_attribute_block(production, "RuntimeStartupBudgetV1");
    for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
        assert!(!contains_identifier(attributes, forbidden), "{forbidden}");
        assert!(!implements_trait(
            production,
            "RuntimeStartupBudgetV1",
            forbidden,
        ));
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
    let production = source_before_test_module(secret);
    for required in [
        "Command::new(\"/usr/bin/security\")",
        ".env_clear()",
        "KEYCHAIN_TIMEOUT",
        "KEYCHAIN_CLEANUP_WINDOW",
        "KEYCHAIN_CAPTURE_BYTES",
        "child.kill()",
        "child.wait()",
        "Zeroizing<String>",
        "Zeroizing<Vec<u8>>",
        "Vec::with_capacity(KEYCHAIN_CAPTURE_BYTES)",
        "RuntimeDatabaseSecretsByCapabilityV1(<redacted>)",
        "RuntimeDatabasePasswordV1(<redacted>)",
        "RuntimeDiscordBotTokenV1(<redacted>)",
        "RuntimeDatabaseUrlSecretV1(<redacted>)",
        "RuntimeSecretsStartupResolutionErrorV1(<redacted>)",
        "pub(crate) fn resolve_runtime_secrets_until_v1(",
        "startup_budget: &RuntimeStartupBudgetV1",
        "startup_budget.operation_cutoff()",
        "startup_budget.cleanup_deadline()",
        "operation_cutoff: Instant",
        "cleanup_deadline: Instant",
        "run_runtime_startup_sync_stage_v1(",
        "Instant::now() < operation_cutoff",
        "local_deadline.min(operation_cutoff)",
        "fn run_keychain_command_until(",
        "fn capture_keychain_child_until(",
        "fn cancel_keychain_child_until(",
        "struct KeychainReaperDispatchV1",
        "KeychainReaperDispatchV1::start()",
        "starring-runtime-keychain-reaper",
        "capture_bounded_until_cancelled",
        "cancellation.store(true, Ordering::Release)",
        "join_keychain_captures",
        "runtime_secret_keychain_cleanup_timed_out",
    ] {
        assert!(production.contains(required), "{required}");
    }
    for forbidden in [
        "sqlx",
        "PgPool",
        "twilight_http",
        "twilight_gateway",
        "Url",
        "into_zeroizing",
        "spawn_blocking",
        "timeout_at",
    ] {
        assert!(!contains_identifier(production, forbidden), "{forbidden}");
    }
    for forbidden in [
        "pub type RuntimeDatabaseConnectionPartsV1",
        "pub fn into_zeroizing",
        "pub fn resolve_runtime_secrets_until_v1",
        "resolve_runtime_secrets_v1",
        "terminate_and_reap",
    ] {
        assert!(!production.contains(forbidden), "{forbidden}");
    }
    let keychain_runner = braced_declaration(production, "fn run_keychain_command_until(");
    let deadline_checks = keychain_runner
        .match_indices("if now >= operation_deadline")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let cleanup_checks = keychain_runner
        .match_indices("if now >= cleanup_deadline")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let configure = keychain_runner
        .find("configure_keychain_command(&mut command)")
        .unwrap();
    let reaper = keychain_runner
        .find("KeychainReaperDispatchV1::start()")
        .unwrap();
    let spawn = keychain_runner.find(".spawn()").unwrap();
    let capture = keychain_runner
        .find("capture_keychain_child_until(")
        .unwrap();
    assert_eq!(deadline_checks.len(), 2);
    assert_eq!(cleanup_checks.len(), 2);
    assert!(
        deadline_checks[0] < cleanup_checks[0]
            && cleanup_checks[0] < configure
            && configure < reaper
            && reaper < deadline_checks[1]
            && deadline_checks[1] < cleanup_checks[1]
            && cleanup_checks[1] < spawn
            && reaper < spawn
            && spawn < capture
    );
    let keychain_capture = braced_declaration(production, "fn capture_keychain_child_until(");
    let capture_deadline_checks = keychain_capture
        .match_indices("if Instant::now() >= operation_deadline")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let stdout_pipe = keychain_capture.find("child.stdout.take()").unwrap();
    let stdout_thread = keychain_capture
        .find("starring-runtime-keychain-stdout")
        .unwrap();
    let stderr_thread = keychain_capture
        .find("starring-runtime-keychain-stderr")
        .unwrap();
    let status_wait = keychain_capture.find("let status = loop").unwrap();
    assert_eq!(capture_deadline_checks.len(), 6);
    assert!(
        capture_deadline_checks[0] < stdout_pipe
            && stdout_pipe < capture_deadline_checks[1]
            && capture_deadline_checks[1] < stdout_thread
            && stdout_thread < capture_deadline_checks[2]
            && capture_deadline_checks[2] < stderr_thread
            && stderr_thread < capture_deadline_checks[3]
            && capture_deadline_checks[3] < status_wait
    );
    let captures_complete = keychain_capture
        .find("stdout_capture.is_finished() || !stderr_capture.is_finished()")
        .unwrap();
    let operation_postcheck = keychain_capture[captures_complete..]
        .find("if Instant::now() >= operation_deadline")
        .map(|offset| captures_complete + offset)
        .unwrap();
    let join = keychain_capture.find("join_keychain_captures(").unwrap();
    let final_postcheck = keychain_capture[join..]
        .find("if Instant::now() >= operation_deadline")
        .map(|offset| join + offset)
        .unwrap();
    let capture_result = keychain_capture
        .find("let (stdout, stderr) = captures?")
        .unwrap();
    assert!(
        captures_complete < operation_postcheck
            && operation_postcheck < join
            && join < final_postcheck
            && final_postcheck < capture_result
    );
    assert!(!keychain_capture.contains("child.wait()"));
    let cancellation = braced_declaration(production, "fn cancel_keychain_child_until(");
    let cancel = cancellation
        .find("cancellation.store(true, Ordering::Release)")
        .unwrap();
    let kill = cancellation.find("child.kill()").unwrap();
    let observe_child = cancellation.find("child.try_wait()").unwrap();
    let observe_captures = cancellation.find("capture_finished(").unwrap();
    let cleanup_cutoff = cancellation
        .find("if Instant::now() >= cleanup_deadline")
        .unwrap();
    let completed = cancellation
        .find("if child_finished && captures_finished")
        .unwrap();
    let dispatch = cancellation.find("reaper.dispatch(").unwrap();
    assert!(
        cancel < kill
            && kill < observe_child
            && observe_child < observe_captures
            && observe_captures < cleanup_cutoff
            && cleanup_cutoff < completed
            && completed < dispatch
    );
    assert!(!cancellation.contains("child.wait()"));
    assert!(!cancellation.contains(".join()"));
    let reaper_start = braced_declaration(
        production,
        "fn start() -> Result<Self, KeychainSecretReadErrorV1>",
    );
    assert!(reaper_start.contains("starring-runtime-keychain-reaper"));
    assert!(reaper_start.contains("payload.child.wait()"));
    assert!(reaper_start.contains("join_optional_keychain_captures("));
    let reaper_dispatch = braced_declaration(production, "fn dispatch(");
    assert!(reaper_dispatch.contains("self.sender.send(payload)"));
    assert!(reaper_dispatch.contains("std::process::abort()"));
    assert!(
        secret.contains("keychain_cleanup_cutoff_returns_before_late_capture_and_reaper_finishes")
    );
    assert!(production.contains("#[cfg(all(test, unix))]\nfn run_keychain_command("));
    assert!(production.contains("#[cfg(all(test, unix))]\nfn capture_keychain_child("));
    let resolver = braced_declaration(production, "fn resolve_until<");
    assert_eq!(
        resolver
            .matches("run_runtime_startup_sync_stage_v1(")
            .count(),
        2
    );
    for capability in [
        "DatabaseCapabilityV1::Convergence",
        "DatabaseCapabilityV1::ExactTarget",
        "DatabaseCapabilityV1::Panel",
        "DatabaseCapabilityV1::Serving",
        "DatabaseCapabilityV1::Interaction",
    ] {
        assert_eq!(resolver.matches(capability).count(), 1, "{capability}");
    }
}

#[test]
fn process_startup_is_the_single_ordered_bounded_foundation_entry() {
    let sources = source_files();
    let process_startup = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/process_startup.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    let production = source_before_test_module(process_startup);
    let entry = braced_declaration(
        production,
        "pub fn run_runtime_process_foundation_staging_from_environment_v1(",
    );
    let staging = braced_declaration(
        production,
        "async fn stage_runtime_process_foundation_from_environment_v1(",
    );
    let config = staging
        .find("RuntimeConfigV1::from_process_environment")
        .unwrap();
    let secrets = staging.find("resolve_runtime_secrets_until_v1").unwrap();
    let revision = staging
        .find("bootstrap_compiled_runtime_build_revision_v1")
        .unwrap();
    let foundation = staging
        .find("compose_runtime_process_foundation_v1")
        .unwrap();
    let shutdown = staging.find(".shutdown()").unwrap();
    let outcome = staging
        .find("Ok(RuntimeProcessFoundationStagingOutcomeV1")
        .unwrap();
    assert!(
        config < secrets
            && secrets < revision
            && revision < foundation
            && foundation < shutdown
            && shutdown < outcome
    );
    assert_eq!(
        staging
            .matches("run_runtime_startup_sync_stage_v1(")
            .count(),
        3
    );
    for operation in [
        "RuntimeConfigV1::from_process_environment",
        "resolve_runtime_secrets_until_v1",
        "bootstrap_compiled_runtime_build_revision_v1",
        "compose_runtime_process_foundation_v1",
        ".shutdown()",
    ] {
        assert_eq!(staging.matches(operation).count(), 1, "{operation}");
    }
    assert_eq!(
        entry.matches("run_runtime_startup_sync_stage_v1(").count(),
        1
    );
    let entry_body = entry.split_once('{').unwrap().1.trim_start();
    assert!(entry_body.starts_with("let startup_budget = RuntimeStartupBudgetV1::begin();"));
    let budget = entry.find("RuntimeStartupBudgetV1::begin()").unwrap();
    let active_runtime = entry.find("tokio::runtime::Handle::try_current()").unwrap();
    let runtime_stage = entry.find("run_runtime_startup_sync_stage_v1(").unwrap();
    let runtime_builder = entry
        .find("tokio::runtime::Builder::new_current_thread()")
        .unwrap();
    let block_on = entry
        .find("runtime.block_on(stage_runtime_process_foundation_from_environment_v1(")
        .unwrap();
    assert!(
        budget < active_runtime
            && active_runtime < runtime_stage
            && runtime_stage < runtime_builder
            && runtime_builder < block_on
    );
    assert_eq!(
        entry
            .matches("stage_runtime_process_foundation_from_environment_v1(")
            .count(),
        1
    );
    for required in [
        "tokio::runtime::Handle::try_current()",
        "tokio::runtime::Builder::new_current_thread()",
        ".enable_all()",
        ".build()",
        "runtime.block_on(stage_runtime_process_foundation_from_environment_v1(",
        "RuntimeProcessFoundationStagingErrorV1(<redacted>)",
        "RuntimeProcessFoundationStagingOutcomeV1(<redacted>)",
        "pub const fn code(self) -> &'static str",
        "pub const fn context(self) -> Option<&'static str>",
        "pub const fn configuration_class(self) -> bool",
    ] {
        assert!(production.contains(required), "{required}");
    }
    for operation in [
        "tokio::runtime::Handle::try_current()",
        "tokio::runtime::Builder::new_current_thread()",
        ".build()",
        "runtime.block_on(stage_runtime_process_foundation_from_environment_v1(",
    ] {
        assert_eq!(entry.matches(operation).count(), 1, "{operation}");
    }
    for forbidden in [
        "#[tokio::main]",
        "new_multi_thread",
        "tokio::spawn",
        "spawn_blocking",
        "tokio::select!",
        "timeout(",
        "timeout_at",
        "signal",
        "sleep",
        "retry",
        "loop {",
        "compose_runtime_database_dependencies_v1",
        "compose_runtime_registry_bootstrap_v1",
        "compose_runtime_gateway_bootstrap_v1",
        "start_gateway_owner_startup_watchdog_v1",
        "observe_current_ready_attestation",
        "issue_ready_lease",
        "ready_to_serve",
        "health_ready",
        "activate",
        "deploy",
        "twilight",
    ] {
        assert!(!production.contains(forbidden), "{forbidden}");
    }
    let outcome_attributes =
        declaration_attribute_block(production, "RuntimeProcessFoundationStagingOutcomeV1");
    let outcome_declaration = braced_declaration(
        production,
        "pub struct RuntimeProcessFoundationStagingOutcomeV1",
    );
    assert!(outcome_declaration.contains("_private: ()"));
    for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
        assert!(
            !contains_identifier(outcome_attributes, forbidden),
            "{forbidden}"
        );
        assert!(!implements_trait(
            production,
            "RuntimeProcessFoundationStagingOutcomeV1",
            forbidden,
        ));
    }
    let library = include_str!("../src/lib.rs");
    for required in [
        "run_runtime_process_foundation_staging_from_environment_v1",
        "RuntimeProcessFoundationStagingErrorV1",
        "RuntimeProcessFoundationStagingOutcomeV1",
    ] {
        assert_eq!(library.matches(required).count(), 1, "{required}");
    }
    for forbidden in [
        "bootstrap_compiled_runtime_build_revision_v1",
        "CompiledRuntimeBuildRevisionV1",
        "compose_runtime_process_foundation_v1",
        "RuntimeProcessFoundationV1",
        "resolve_runtime_secrets_until_v1",
        "resolve_runtime_secrets_v1",
        "RuntimeStartupBudgetV1",
        "run_runtime_startup_sync_stage_v1",
    ] {
        assert!(!library.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn executable_delegates_once_without_raw_startup_or_runtime_authority() {
    let main = source_before_test_module(include_str!("../src/main.rs"));
    let entry = braced_declaration(main, "fn main() -> ExitCode");
    assert_eq!(
        entry
            .matches("run_runtime_process_foundation_staging_from_environment_v1()")
            .count(),
        1
    );
    assert!(entry.contains("emit_status(status.code(), status.context())"));
    assert!(entry.contains("status.exit_code()"));
    for forbidden in [
        "#[tokio::main]",
        "tokio",
        ".await",
        "RuntimeConfigV1",
        "from_process_environment",
        "resolve_runtime_secrets",
        "RuntimeStartupBudgetV1",
        "bootstrap_compiled_runtime_build_revision_v1",
        "CompiledRuntimeBuildRevisionV1",
        "compose_runtime_process_foundation_v1",
        "RuntimeProcessFoundationV1",
        "compose_runtime_database_dependencies_v1",
        "compose_runtime_registry_bootstrap_v1",
        "compose_runtime_gateway_bootstrap_v1",
        "start_gateway_owner_startup_watchdog_v1",
        "tokio::spawn",
        "spawn_blocking",
        "tokio::select!",
        "signal",
        "sleep",
        "retry",
        "loop {",
        "ready_to_serve",
        "health_ready",
        "twilight",
    ] {
        assert!(!main.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn process_foundation_composes_closed_components_in_order_and_cleans_up_failure() {
    let sources = source_files();
    let process = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/process.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    let production = source_before_test_module(process);
    let composer = braced_declaration(
        production,
        "pub(crate) async fn compose_runtime_process_foundation_v1(",
    );
    let deadline_before_process_identity = composer
        .find("if !startup_budget.operation_is_open()")
        .unwrap();
    let process_identity = composer
        .find("generate_runtime_process_instance_id_v1()")
        .unwrap();
    let deadline_after_process_identity = composer[process_identity..]
        .find("if !startup_budget.operation_is_open()")
        .map(|offset| process_identity + offset)
        .unwrap();
    let process_identity_error = composer
        .find(".map_err(RuntimeProcessFoundationCompositionErrorV1::ProcessInstanceId)")
        .unwrap();
    let controller_identity = composer
        .find("generate_runtime_controller_id_v1()")
        .unwrap();
    let deadline_after_controller_identity = composer[controller_identity..]
        .find("if !startup_budget.operation_is_open()")
        .map(|offset| controller_identity + offset)
        .unwrap();
    let controller_identity_error = composer
        .find(".map_err(RuntimeProcessFoundationCompositionErrorV1::ControllerId)")
        .unwrap();
    let build_revision = composer.find("build_revision.into_revision()").unwrap();
    let databases = composer
        .find("compose_runtime_database_dependencies_v1(&config, &secrets, &startup_budget)")
        .unwrap();
    let closed = composer
        .find("compose_closed_process_components_v1(&process_instance_id, config.gateway())")
        .unwrap();
    let deadline_after_closed_result = composer[closed..]
        .find("if !startup_budget.operation_is_open()")
        .map(|offset| closed + offset)
        .unwrap();
    let closed_result = composer
        .find("let closed_components = match closed_components")
        .unwrap();
    let cleanup = composer
        .find(".close_until(startup_budget.cleanup_deadline())")
        .unwrap();

    assert!(
        deadline_before_process_identity < process_identity
            && process_identity < deadline_after_process_identity
            && deadline_after_process_identity < process_identity_error
            && process_identity_error < controller_identity
            && controller_identity < deadline_after_controller_identity
            && deadline_after_controller_identity < controller_identity_error
            && controller_identity_error < build_revision
            && build_revision < databases
            && databases < closed
            && closed < deadline_after_closed_result
            && deadline_after_closed_result < closed_result
            && closed_result < cleanup
    );
    for required in [
        "pub(crate) struct RuntimeProcessFoundationV1",
        "config: RuntimeConfigV1,",
        "secrets: ResolvedRuntimeSecretsV1,",
        "build_revision: RuntimeBuildRevisionV1,",
        "build_revision: CompiledRuntimeBuildRevisionV1",
        "build_revision.into_revision()",
        "build_revision,",
        "process_instance_id: ProcessInstanceId,",
        "controller_id: ControllerId,",
        "compose_runtime_registry_bootstrap_v1(process_instance_id.clone(), gateway_config)",
        "compose_runtime_gateway_bootstrap_v1(",
        "RuntimeProcessFoundationV1(<redacted>)",
        "RuntimeProcessFoundationCompositionErrorV1(<redacted>)",
        "startup_budget: RuntimeStartupBudgetV1",
        "if !startup_budget.operation_is_open()",
        "cleanup_after_operation_deadline_v1",
        "RuntimeProcessFoundationCompositionErrorV1::OperationDeadlineElapsed",
        "RuntimeProcessFoundationCompositionErrorV1::CleanupAfterOperationDeadline",
        "pub(crate) async fn shutdown(self)",
        "shutdown.close_until(cleanup_deadline).await",
        "finish_runtime_process_foundation_shutdown_v1(",
        "let Self {",
        "drop((",
    ] {
        assert!(production.contains(required), "{required}");
    }
    assert_eq!(
        composer
            .matches("if !startup_budget.operation_is_open()")
            .count(),
        6
    );
    assert_eq!(
        composer
            .matches("generate_runtime_process_instance_id_v1()")
            .count(),
        1
    );
    assert_eq!(
        composer
            .matches("generate_runtime_controller_id_v1()")
            .count(),
        1
    );
    assert_eq!(
        composer.matches("build_revision.into_revision()").count(),
        1
    );
    let signature = production
        .split("pub(crate) async fn compose_runtime_process_foundation_v1(")
        .nth(1)
        .and_then(|source| source.split(") -> Result<").next())
        .unwrap();
    for owned in [
        "config: RuntimeConfigV1",
        "secrets: ResolvedRuntimeSecretsV1",
        "build_revision: CompiledRuntimeBuildRevisionV1",
    ] {
        assert!(signature.contains(owned), "{owned}");
    }
    assert!(!signature.contains("config: &RuntimeConfigV1"));
    assert!(!signature.contains("secrets: &ResolvedRuntimeSecretsV1"));
    assert!(!signature.contains("process_instance_id: ProcessInstanceId"));
    assert!(!signature.contains("controller_id: ControllerId"));
    assert!(!signature.contains("build_revision: RuntimeBuildRevisionV1"));
    assert!(!composer.contains("ControllerId::parse(process_instance_id"));
    assert!(!composer.contains("ControllerId::parse(build_revision"));
    assert!(!composer.contains("ProcessInstanceId::parse(controller_id"));
    for forbidden in [
        "ready_to_serve",
        "health_ready",
        "start_gateway_owner_startup_watchdog_v1",
        "observe_current_ready_attestation()?",
        "Discord",
        "RuntimeStartupBudgetV1::begin()",
        "pub struct RuntimeProcessFoundationV1",
        "pub async fn compose_runtime_process_foundation_v1",
        "pub async fn shutdown(self)",
        "pub fn runtime_build_revision(&self)",
        "pub fn process_instance_id(&self)",
        "pub fn controller_id(&self)",
        "pub fn databases(&self)",
        "pub fn registry(&self)",
        "pub fn gateway(&self)",
    ] {
        assert!(!production.contains(forbidden), "{forbidden}");
    }
    assert!(!composer.contains("drop(secrets)"));
    assert!(!composer.contains("drop(config)"));
    let shutdown = braced_declaration(production, "pub(crate) async fn shutdown(self)");
    let cleanup_deadline = shutdown
        .find("let cleanup_deadline = startup_budget.cleanup_deadline()")
        .unwrap();
    let shutdown_handle = shutdown
        .find("let shutdown = databases.shutdown()")
        .unwrap();
    let closed_components = shutdown
        .find("drop((gateway, registry, databases))")
        .unwrap();
    let close = shutdown
        .find("shutdown.close_until(cleanup_deadline).await")
        .unwrap();
    let release_handle = shutdown.find("drop(shutdown)").unwrap();
    let finish = shutdown
        .find("finish_runtime_process_foundation_shutdown_v1(")
        .unwrap();
    assert!(
        cleanup_deadline < shutdown_handle
            && shutdown_handle < closed_components
            && closed_components < close
            && close < release_handle
            && release_handle < finish
    );
    assert!(shutdown.contains("let cleanup_deadline = startup_budget.cleanup_deadline()"));
    assert!(shutdown.contains("shutdown.close_until(cleanup_deadline).await"));
    assert!(!shutdown.contains("shutdown.close().await"));
    assert!(!composer.contains("shutdown.close().await"));
    let finish_shutdown = braced_declaration(
        production,
        "fn finish_runtime_process_foundation_shutdown_v1<S, R>(",
    );
    let drop_secrets = finish_shutdown.find("drop(secrets)").unwrap();
    let drop_retained = finish_shutdown.find("drop(retained)").unwrap();
    let return_result = finish_shutdown.rfind("result").unwrap();
    assert!(drop_secrets < drop_retained && drop_retained < return_result);
    for required in [
        "shutdown_finish_drops_secrets_only_after_close_returns_on_every_result",
        "[\"pool_close_returned\", \"secrets\", \"retained\"]",
        "assert_shutdown_finish_drop_order(Ok(()))",
        "assert_shutdown_finish_drop_order(Err(RuntimeDatabasePoolShutdownErrorV1::TimedOut))",
    ] {
        assert!(process.contains(required), "{required}");
    }
    for name in [
        "RuntimeProcessFoundationV1",
        "RuntimeClosedProcessComponentsV1",
    ] {
        let attributes = declaration_attribute_block(production, name);
        for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
            assert!(
                !contains_identifier(attributes, forbidden),
                "{name}: {forbidden}"
            );
            assert!(
                !implements_trait(production, name, forbidden),
                "{name}: {forbidden}"
            );
        }
    }
}
