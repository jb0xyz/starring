use std::collections::BTreeSet;
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

fn declaration_attribute_block<'a>(source: &'a str, name: &str) -> &'a str {
    let marker = format!("struct {name}");
    let declaration = source.find(&marker).unwrap();
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
            "src/config.rs",
            "src/database.rs",
            "src/gateway.rs",
            "src/gateway_owner_startup_watchdog.rs",
            "src/gateway_owner_startup_watchdog_handoff_tests.rs",
            "src/lib.rs",
            "src/main.rs",
            "src/secret.rs",
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
            ("automation-runtime-serving-postgres".to_string(), None),
            ("automation-runtime-worker".to_string(), None),
            ("chrono".to_string(), Some("dev".to_string())),
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
        {
            assert!(!contains_identifier(&source, "tokio"), "{}", path.display());
        }
    }
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
            assert!(
                !identifier.ends_with("V3")
                    && (allowed_readiness_worker
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
    let production = gateway.split("#[cfg(test)]").next().unwrap();
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
    ] {
        let attributes = declaration_attribute_block(owner_supervisor, name);
        for forbidden in ["Clone", "Copy", "Default"] {
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
    let library = include_str!("../src/lib.rs");
    for forbidden in [
        "RuntimeGatewayOwnerProductionHandoffProofV1",
        "RuntimeGatewayOwnerProductionSupervisorV1",
        "RuntimeGatewayOwnerProductionHandoffErrorV1",
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
    for forbidden in [
        "health_ready",
        "ready_to_serve",
        "gateway_connected",
        "compose_runtime_gateway_bootstrap_v1",
        "RuntimeGatewayBootstrapV1",
    ] {
        assert!(!main.contains(forbidden));
    }
}
