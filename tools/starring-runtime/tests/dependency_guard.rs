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

#[test]
fn paused_production_handoff_is_linear_shutdown_biased_and_non_serving() {
    let observation = include_str!("../src/process/observation.rs");
    let gateway = source_before_test_module(include_str!("../src/gateway.rs"));
    let discord = source_before_test_module(include_str!("../src/discord.rs"));
    let closed_recovery = include_str!("../src/closed_recovery.rs");
    let handoff = braced_declaration(
        observation,
        "pub(crate) async fn into_paused_production_handoff_v2(",
    );

    let finalizer = handoff
        .find("seal_startup_finalizer_for_handoff_v2(handoff_cutoff)")
        .unwrap();
    let worker = handoff.find("into_worker_fixed_point_v2()").unwrap();
    let owner = handoff
        .find("enter_admission_frozen_in_place_v2()")
        .unwrap();
    let frozen = handoff.find("try_into_admission_frozen_v2()").unwrap();
    let discord_ack = handoff
        .find("handoff_to_process_in_place_v2(process_generation)")
        .unwrap();
    let discord_state = handoff.find("into_process_handoff_v2()").unwrap();
    let revalidate = handoff.find("revalidate_paused_v2()").unwrap();
    assert!(
        finalizer < worker
            && worker < owner
            && owner < frozen
            && frozen < discord_ack
            && discord_ack < discord_state
            && discord_state < revalidate
    );
    assert_eq!(
        handoff
            .matches("await_production_handoff_stage_v2(")
            .count(),
        3
    );
    assert_eq!(handoff.matches("&mut shutdown").count(), 3);
    for forbidden in [
        "resume_reserved_runtime_discord_admission_v2",
        "publish_ready",
        "ready_to_serve",
        "execute_admitted_interaction",
        "activate",
        "deploy",
    ] {
        assert!(!contains_identifier(handoff, forbidden), "{forbidden}");
    }

    let final_revalidation = braced_declaration(observation, "pub(crate) fn revalidate_paused_v2(");
    let owner = final_revalidation.find(".revalidate_paused_v2()").unwrap();
    let root_shutdown = final_revalidation
        .find("production_handoff_shutdown_failure_v2(")
        .unwrap();
    assert!(owner < root_shutdown);
    let shutdown_check =
        braced_declaration(observation, "fn production_handoff_shutdown_failure_v2(");
    assert!(shutdown_check.contains("RuntimeProcessProductionHandoffFailureV2::ProcessShutdown"));

    assert!(!gateway.contains("Arc<Mutex<RuntimeGatewayClosedLifecycleV2>>"));
    assert!(!gateway.contains("Arc::try_unwrap"));
    assert!(discord.contains("RuntimeDiscordProcessHandoffStateV2::InFlight"));
    assert!(discord.contains("RuntimeDiscordProcessHandoffV2::Indeterminate"));
    assert!(closed_recovery.contains("operation_cutoff"));
    assert!(closed_recovery.contains("RuntimeClosedRecoveryAdmissionFrozenProcessV2"));
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

fn resolved_package_metadata() -> serde_json::Value {
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--manifest-path",
            concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    serde_json::from_slice(&output.stdout).unwrap()
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
            "RuntimeAuthorizedPendingDrainAcknowledgementV2",
            "RuntimeAuthorizedPendingDrainClaimV2",
            "RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3",
            "RuntimeCapabilityReadinessKindV2",
            "RuntimeCapabilityReadinessReceiptV2",
            "RuntimeCapabilityReadinessSetV2",
            "RuntimePendingDrainAcknowledgementExecutionPortV2",
            "RuntimePendingDrainAcknowledgementReceiptV2",
            "RuntimePendingDrainClaimExecutionPortV2",
            "RuntimePendingDrainClaimReceiptV2",
            "RuntimePendingDrainNoCandidateReceiptV2",
            "RuntimePendingDrainNoCandidateRecorderPortV2",
            "RuntimePendingDrainSuccessionAcknowledgementExecutionPortV3",
            "RuntimePendingDrainSuccessionAcknowledgementReceiptV3",
            "RuntimeSelectedPendingDrainNoCandidateV2",
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
            "src/capability_readiness_supervisor.rs",
            "src/closed_recovery.rs",
            "src/config.rs",
            "src/controller_identity.rs",
            "src/database.rs",
            "src/discord.rs",
            "src/discord_lifecycle.rs",
            "src/gateway.rs",
            "src/gateway_owner_startup.rs",
            "src/gateway_owner_startup_watchdog.rs",
            "src/gateway_owner_startup_watchdog_handoff_tests.rs",
            "src/health.rs",
            "src/identity_encoding.rs",
            "src/ingress_acknowledgement_safety.rs",
            "src/ingress_acknowledgement_supervisor.rs",
            "src/lib.rs",
            "src/lifecycle_timing.rs",
            "src/main.rs",
            "src/maintenance_ingress_gate.rs",
            "src/mutation_finalizer.rs",
            "src/process/closed.rs",
            "src/process/connected.rs",
            "src/process/execution.rs",
            "src/process/observation.rs",
            "src/process/observation_tests.rs",
            "src/process/owner.rs",
            "src/process/pending_drain_finalizer.rs",
            "src/process/readiness.rs",
            "src/process/recovery.rs",
            "src/process/serving.rs",
            "src/process/startup_loop.rs",
            "src/process/startup_loop_tests.rs",
            "src/process.rs",
            "src/process_identity.rs",
            "src/process_startup.rs",
            "src/process_supervisor.rs",
            "src/recovery_identity.rs",
            "src/registry.rs",
            "src/registry_staging_tests.rs",
            "src/registry_succession_tests.rs",
            "src/runtime_controller.rs",
            "src/secret.rs",
            "src/shutdown.rs",
            "src/startup.rs",
            "src/startup_recovery_observation.rs",
            "tests/dependency_guard.rs",
            "tests/gateway_owner_startup_watchdog.rs",
            "tests/mutation_finalizer.rs",
            "tests/process_contract.rs",
            "tests/staging_role_bootstrap_contract.rs"
        ]
    );
}

#[test]
fn direct_dependencies_are_the_exact_runtime_composition_surface() {
    let package_dependencies = package_dependencies();
    let twilight_gateway = package_dependencies
        .iter()
        .find(|dependency| dependency["name"] == "twilight-gateway")
        .unwrap();
    assert_eq!(twilight_gateway["rename"], "paused-discord-gateway");
    assert_eq!(
        twilight_gateway["source"],
        "git+https://github.com/twilight-rs/twilight.git?rev=b4ce13b727e7731b917576ad977300ab6926bb6b"
    );
    assert_eq!(twilight_gateway["uses_default_features"], false);
    assert_eq!(
        twilight_gateway["features"],
        serde_json::json!(["rustls-platform-verifier"])
    );
    let tokio = package_dependencies
        .iter()
        .find(|dependency| dependency["name"] == "tokio")
        .unwrap();
    assert_eq!(tokio["uses_default_features"], false);
    assert_eq!(
        tokio["features"],
        serde_json::json!(["io-util", "macros", "net", "rt", "signal", "sync", "time"])
    );
    let mut dependencies = package_dependencies
        .into_iter()
        .map(|dependency| {
            if dependency["name"] != "twilight-gateway" {
                assert!(dependency["rename"].is_null());
            }
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
            ("chrono".to_string(), None),
            ("getrandom".to_string(), None),
            ("serde_json".to_string(), Some("dev".to_string())),
            ("sqlx".to_string(), None),
            ("thiserror".to_string(), None),
            ("tokio".to_string(), None),
            ("twilight-gateway".to_string(), None),
            ("zeroize".to_string(), None),
        ]
    );
}

#[test]
fn paused_discord_gateway_dependency_is_feature_isolated() {
    let metadata = resolved_package_metadata();
    let package = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| {
            package["name"] == "twilight-gateway"
                && package["version"] == "0.17.1"
                && package["source"]
                    .as_str()
                    .is_some_and(|source| source.starts_with("git+https://github.com/twilight-rs/twilight.git?rev=b4ce13b727e7731b917576ad977300ab6926bb6b#"))
        })
        .unwrap();
    let package_id = package["id"].as_str().unwrap();
    let resolved = metadata["resolve"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["id"] == package_id)
        .unwrap();
    assert_eq!(
        resolved["features"],
        serde_json::json!(["rustls-platform-verifier"])
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
            "twilight_model",
            "TcpListener",
        ] {
            if path == Path::new("src/discord.rs") && forbidden == "twilight_gateway" {
                continue;
            }
            if path == Path::new("src/health.rs") && forbidden == "TcpListener" {
                continue;
            }
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
                if path == Path::new("src/process.rs") && forbidden == "PostgresRuntimeExecutionV1"
                {
                    continue;
                }
                assert!(
                    !contains_identifier(&source, forbidden),
                    "{}: {forbidden}",
                    path.display()
                );
            }
        }
        if path == Path::new("src/process.rs") {
            let finalizer_handoff = braced_declaration(
                &source,
                "pub(super) async fn seal_startup_finalizer_for_handoff_v2(",
            );
            assert_eq!(source.matches("tokio::").count(), 2);
            assert_eq!(finalizer_handoff.matches("tokio::").count(), 2);
            assert_eq!(source.matches("PostgresRuntimeExecutionV1").count(), 1);
            assert!(source.contains(concat!(
                "pub(super) type RuntimeProcessIngressAcknowledgementSupervisorV2 =\n",
                "    RuntimeIngressAcknowledgementSupervisorV2<\n",
                "        automation_runtime_execution_postgres::PostgresRuntimeExecutionV1,\n",
                "        RuntimeProcessIngressAcknowledgementJobV2,\n",
                "    >;"
            )));
        } else if path != Path::new("src/capability_readiness_supervisor.rs")
            && path != Path::new("src/database.rs")
            && path != Path::new("src/gateway.rs")
            && path != Path::new("src/gateway_owner_startup.rs")
            && path != Path::new("src/gateway_owner_startup_watchdog.rs")
            && path != Path::new("src/gateway_owner_startup_watchdog_handoff_tests.rs")
            && path != Path::new("src/health.rs")
            && path != Path::new("src/discord.rs")
            && path != Path::new("src/ingress_acknowledgement_safety.rs")
            && path != Path::new("src/ingress_acknowledgement_supervisor.rs")
            && path != Path::new("src/maintenance_ingress_gate.rs")
            && path != Path::new("src/process/closed.rs")
            && path != Path::new("src/process/connected.rs")
            && path != Path::new("src/process/execution.rs")
            && path != Path::new("src/process/observation.rs")
            && path != Path::new("src/process/observation_tests.rs")
            && path != Path::new("src/process/owner.rs")
            && path != Path::new("src/process/pending_drain_finalizer.rs")
            && path != Path::new("src/process/readiness.rs")
            && path != Path::new("src/process/recovery.rs")
            && path != Path::new("src/process/serving.rs")
            && path != Path::new("src/process/startup_loop.rs")
            && path != Path::new("src/process/startup_loop_tests.rs")
            && path != Path::new("src/process_startup.rs")
            && path != Path::new("src/process_supervisor.rs")
            && path != Path::new("src/runtime_controller.rs")
            && path != Path::new("src/mutation_finalizer.rs")
            && path != Path::new("src/shutdown.rs")
            && path != Path::new("src/startup_recovery_observation.rs")
        {
            assert!(!contains_identifier(&source, "tokio"), "{}", path.display());
        }
        if path != Path::new("src/process_identity.rs")
            && path != Path::new("src/controller_identity.rs")
            && path != Path::new("src/recovery_identity.rs")
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
fn mutation_finalizer_is_bounded_linear_supervised_and_handoff_compatible() {
    let source = include_str!("../src/mutation_finalizer.rs");
    for required in [
        "const RUNTIME_MUTATION_FINALIZER_MAX_CAPACITY: usize = 1_024;",
        "RuntimeMutationFinalizerJobV1<J>",
        "StartupPendingDrain(J)",
        "RuntimeMutationFinalizerGenerationV1",
        "RuntimeMutationFinalizerHandoffStateV1",
        "startup_intake_sealed",
        "startup_jobs_settled",
        "mpsc::channel(capacity)",
        "mpsc::channel(1)",
        "Semaphore::new(capacity)",
        "try_acquire_owned()",
        "self.jobs.try_reserve()",
        "RuntimeMutationFinalizerRegistrationRejectedV1",
        "RuntimeMutationFinalizerWaiterV1(<redacted>)",
        "RuntimeMutationFinalizerSupervisorV1(<redacted>)",
        "RuntimeMutationFinalizerCompletionV1(<redacted>)",
        "RuntimeMutationFinalizerInFlightAbortV1",
        "RuntimeMutationFinalizerInFlightAbortGuardV1",
        "RuntimeMutationFinalizerInFlightStoppedGuardV1",
        "wait_stopped().await",
        "abort_with(RuntimeSupervisorExitV1::DeadlineElapsed)",
        "supervisor_id: NonZeroU64",
        "NEXT_RUNTIME_MUTATION_FINALIZER_SUPERVISOR_ID",
        "task.abort()",
        "actor.abort()",
        "RuntimeSupervisorExitV1::Panicked",
        "RuntimeSupervisorExitV1::Aborted",
    ] {
        assert!(source.contains(required), "{required}");
    }
    for forbidden in [
        "unbounded_channel",
        "spawn_blocking",
        "dyn Fn",
        "BoxFuture",
        "sqlx",
        "PgPool",
        "twilight",
        "RuntimePublicAdmissionPermit",
        "GatewayCommand",
    ] {
        assert!(!contains_identifier(source, forbidden), "{forbidden}");
    }
    for type_name in [
        "RuntimeMutationFinalizerSupervisorV1",
        "RuntimeMutationFinalizerWaiterV1",
        "RuntimeMutationFinalizerCompletionV1",
        "RuntimeMutationFinalizerJobV1",
    ] {
        let attributes = declaration_attribute_block(source, type_name);
        for forbidden in ["Clone", "Copy", "Serialize", "Deserialize", "Default"] {
            assert!(
                !contains_identifier(attributes, forbidden),
                "{type_name}: {forbidden}"
            );
            assert!(
                !source.contains(&format!("{forbidden} for {type_name}")),
                "{type_name}: {forbidden}"
            );
        }
    }
}

#[test]
fn shutdown_latch_is_single_assignment_bounded_and_non_authorizing() {
    let shutdown = include_str!("../src/shutdown.rs");
    let production = source_before_test_module(shutdown);
    for required in [
        "const RUNTIME_SHUTDOWN_WINDOW: Duration = Duration::from_secs(30);",
        "observation: OnceLock<RuntimeShutdownObservationV1>",
        "pub fn trip(&self, cause: RuntimeShutdownCauseV1)",
        "self.state.observation.set(candidate)",
        "pub fn create_startup_bounded(cleanup_deadline: Instant)",
        "deadline.min(ceiling)",
        "RuntimeShutdownTripV1::First(candidate)",
        "RuntimeShutdownTripV1::Existing(",
        "pub async fn wait(&mut self)",
        "signal(SignalKind::interrupt())",
        "signal(SignalKind::terminate())",
        "RuntimeShutdownObservationV1(<redacted>)",
        "RuntimeShutdownTriggerV1(<redacted>)",
        "RuntimeShutdownSignalLatchV1(<redacted>)",
    ] {
        assert!(production.contains(required), "{required}");
    }
    for type_name in [
        "RuntimeShutdownObservationV1",
        "RuntimeShutdownSignalLatchV1",
        "RuntimeOsShutdownSignalsV1",
    ] {
        let attributes = declaration_attribute_block(production, type_name);
        for forbidden in ["Default", "Serialize", "Deserialize"] {
            assert!(
                !contains_identifier(attributes, forbidden),
                "{type_name}: {forbidden}"
            );
            assert!(
                !implements_trait(production, type_name, forbidden),
                "{type_name}: {forbidden}"
            );
        }
    }
    let latch_attributes = declaration_attribute_block(production, "RuntimeShutdownSignalLatchV1");
    for forbidden in ["Clone", "Copy"] {
        assert!(
            !contains_identifier(latch_attributes, forbidden),
            "RuntimeShutdownSignalLatchV1: {forbidden}"
        );
        assert!(
            !implements_trait(production, "RuntimeShutdownSignalLatchV1", forbidden),
            "RuntimeShutdownSignalLatchV1: {forbidden}"
        );
    }
    assert!(!production.contains("sleep("));
    assert!(!production.contains("SystemTime"));
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
        if path == Path::new("src/gateway.rs")
            || path == Path::new("src/discord.rs")
            || path == Path::new("src/discord_lifecycle.rs")
        {
            continue;
        }
        let source = if path == Path::new("src/process_supervisor.rs") {
            source_before_test_module(source)
        } else {
            source
        };
        if path == Path::new("src/registry_staging_tests.rs")
            || path == Path::new("src/registry_succession_tests.rs")
        {
            continue;
        }
        if path == Path::new("src/gateway_owner_startup_watchdog_handoff_tests.rs") {
            for identifier in source
                .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                .filter(|identifier| !identifier.is_empty())
            {
                let allowed = matches!(
                    identifier,
                    "RuntimeAuthorizedPendingDrainSelectionV3"
                        | "RuntimeAcceptedPendingDrainSelectionV3"
                        | "RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3"
                        | "RuntimePendingDrainFinalizerDispatchFailureV3"
                        | "RuntimePendingDrainFinalizerJobV3"
                        | "RuntimePendingDrainFinalizerPortV3"
                        | "RuntimePendingDrainMutationEnvironmentV3"
                        | "RuntimePendingDrainMutationOutputV3"
                        | "RuntimePendingDrainMutationStageV3"
                        | "RuntimePendingDrainPreviousOwnerClaimedCandidateV3"
                        | "RuntimePendingDrainSelectionOutcomeV3"
                        | "RuntimePendingDrainSelectionReceiptV3"
                        | "RuntimePendingDrainSuccessionAcknowledgementReceiptV3"
                );
                assert!(
                    !identifier.ends_with("V3") || allowed,
                    "{}: {identifier}",
                    path.display()
                );
            }
            continue;
        }
        if path == Path::new("src/gateway_owner_startup.rs")
            || path == Path::new("src/gateway_owner_startup_watchdog.rs")
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
                    "automation_runtime_controller"
                        | "automation_runtime_convergence"
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
            let allowed_paused_connected = path == Path::new("src/process/connected.rs")
                && identifier == "automation_runtime_worker";
            let allowed_recovery_process = path == Path::new("src/process/recovery.rs")
                && identifier == "automation_runtime_worker";
            let allowed_execution_process = path == Path::new("src/process/execution.rs")
                && identifier == "automation_runtime_worker";
            let allowed_pending_drain_finalizer_dependency = path
                == Path::new("src/process/pending_drain_finalizer.rs")
                && identifier == "automation_runtime_worker";
            let allowed_pending_drain_finalizer_v3 = matches!(
                identifier,
                "RuntimeOwnedStartupRecoveryExecutionOutcomeV3"
                    | "RuntimeProcessMutationFinalizerV3"
                    | "RuntimeProcessStartupMutationFinalizerV3"
                    | "RuntimePendingDrainMutationDatabaseV3"
                    | "RuntimePendingDrainFinalizerDispatchFailureV3"
                    | "RuntimePendingDrainFinalizerJobV3"
                    | "RuntimePendingDrainFinalizerPortV3"
                    | "RuntimePendingDrainFinalizerSettledV3"
                    | "RuntimePendingDrainFinalizerSupervisorV3"
                    | "RuntimePendingDrainRegisteredJobV3"
                    | "RuntimePendingDrainMutationEnvironmentV3"
                    | "RuntimePendingDrainMutationOutputV3"
                    | "RuntimePendingDrainMutationStageV3"
                    | "RuntimePendingDrainOwnedStageFailureV3"
                    | "RuntimeProductionPendingDrainFinalizerEnvironmentV3"
                    | "RuntimeStartupRecoveryOwnedStepFutureV3"
                    | "RuntimeStartupRecoveryOwnedStepOutcomeV3"
            ) && matches!(
                path.as_path(),
                path if path == Path::new("src/database.rs")
                    || path == Path::new("src/process.rs")
                    || path == Path::new("src/process/execution.rs")
                    || path == Path::new("src/process/pending_drain_finalizer.rs")
                    || path == Path::new("src/process/startup_loop.rs")
                    || path == Path::new("src/process/startup_loop_tests.rs")
            );
            let allowed_observation_process = (path == Path::new("src/process/observation.rs")
                || path == Path::new("src/process/serving.rs"))
                && matches!(
                    identifier,
                    "automation_runtime_controller" | "automation_runtime_worker"
                );
            let allowed_observation_process_tests = path
                == Path::new("src/process/observation_tests.rs")
                && identifier == "automation_runtime_worker";
            let allowed_process_identity = path == Path::new("src/process_identity.rs")
                && identifier == "automation_runtime_convergence";
            let allowed_controller_identity = path == Path::new("src/controller_identity.rs")
                && identifier == "automation_runtime_convergence";
            let allowed_recovery_identity = path == Path::new("src/recovery_identity.rs")
                && identifier == "automation_runtime_controller";
            let allowed_mutation_finalizer = path == Path::new("src/mutation_finalizer.rs")
                && identifier == "automation_runtime_worker";
            let allowed_maintenance_ingress_gate = path
                == Path::new("src/maintenance_ingress_gate.rs")
                && identifier == "automation_runtime_worker";
            let allowed_ingress_acknowledgement_supervisor = path
                == Path::new("src/ingress_acknowledgement_supervisor.rs")
                && matches!(
                    identifier,
                    "automation_runtime_controller" | "automation_runtime_worker"
                );
            let allowed_runtime_controller = path == Path::new("src/runtime_controller.rs")
                && matches!(
                    identifier,
                    "automation_runtime"
                        | "automation_runtime_controller"
                        | "automation_runtime_convergence"
                        | "automation_runtime_worker"
                );
            let allowed_pending_drain_succession = path
                == Path::new("src/process/pending_drain_finalizer.rs")
                || matches!(
                    (path, identifier),
                    (
                        path,
                        "RuntimeDurablyAcknowledgedPendingDrainSuccessionV3"
                            | "RuntimePendingDrainPreviousOwnerClaimedCandidateV3"
                            | "RuntimeRegistryPendingDrainSuccessionSealBindingV3"
                    ) if path == Path::new("src/registry.rs")
                        || path == Path::new("src/closed_recovery.rs")
                )
                || matches!(
                    (path, identifier),
                    (
                        path,
                        "RuntimeAcceptedPendingDrainSelectionV3"
                            | "RuntimeAuthorizedPendingDrainSelectionV3"
                            | "RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3"
                            | "RuntimePendingDrainSelectionPortV3"
                            | "RuntimePendingDrainSelectionReceiptV3"
                            | "RuntimePendingDrainSuccessionAcknowledgementExecutionPortV3"
                            | "RuntimePendingDrainSuccessionAcknowledgementReceiptV3"
                    ) if path == Path::new("src/database.rs")
                        || path == Path::new("src/process/execution.rs")
                        || path == Path::new("src/process/pending_drain_finalizer.rs")
                );
            assert!(
                (!identifier.ends_with("V3")
                    || allowed_pending_drain_succession
                    || allowed_pending_drain_finalizer_v3)
                    && (allowed_readiness_worker
                        || allowed_registry_adapter
                        || allowed_closed_recovery
                        || allowed_startup_observation
                        || allowed_build_revision
                        || allowed_process_foundation
                        || allowed_paused_connected
                        || allowed_recovery_process
                        || allowed_execution_process
                        || allowed_pending_drain_finalizer_dependency
                        || allowed_observation_process
                        || allowed_observation_process_tests
                        || allowed_process_identity
                        || allowed_controller_identity
                        || allowed_recovery_identity
                        || allowed_mutation_finalizer
                        || allowed_maintenance_ingress_gate
                        || allowed_ingress_acknowledgement_supervisor
                        || allowed_runtime_controller
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
    let process_observation = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/process/observation.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    assert_eq!(
        process_observation
            .matches("automation_runtime_controller")
            .count(),
        7
    );
    assert!(process_observation.contains(concat!(
        "use automation_runtime_controller::{\n",
        "    RuntimeGatewayOwnerLeaseReceiptV1, RuntimeGatewayReadyAttestationV2,\n",
        "    RuntimeIngressOpenAcknowledgementLeaseDurationV2, RuntimeObserveWriterFenceV1,\n",
        "    RuntimeWriterFenceObservationV1,\n",
        "};"
    )));
    assert!(process_observation.contains(
        "writer_fence_generation: automation_runtime_controller::RuntimeWriterFenceGenerationV1,"
    ));
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
        "self.connection_observer.issue_ready_lease(epoch)",
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
        "pub(crate) async fn commit_prepared_owner_in_place_v2(",
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
    assert!(!owner_supervisor.contains("pub(crate) async fn into_production_v1("));
    assert!(!owner_supervisor.contains("RuntimeGatewayOwnerProductionHandoffProofV1"));
    assert!(!owner_supervisor.contains("RuntimeGatewayOwnerProductionSupervisorV1"));
    assert!(!owner_supervisor.contains("RuntimeGatewayOwnerSupervisorCommandV1::Promote"));
    assert!(!owner_supervisor.contains("RuntimeGatewayOwnerSupervisorCommandV1::Prepare"));
    assert!(!owner_supervisor.contains("RuntimeGatewayOwnerSupervisorCommandV1::Commit"));
    for required in [
        "pub(crate) async fn prepare_closed_recovery_v2(\n        mut self,",
        "pub(crate) async fn prepare_closed_recovery_in_place_v2(",
        "pub(crate) fn try_into_prepared_closed_recovery_v2(",
        "pub(crate) async fn abort_and_shutdown_v2(\n        mut self,",
        "pub(crate) async fn abort_and_shutdown_until_v2(",
        "pub(crate) async fn commit_closed_recovery_v2(\n        mut self,",
        "const CLOSED_RECOVERY_COMMAND_CAPACITY: usize = 1;",
        "RuntimeGatewayOwnerClosedRecoveryCommandV2::Prepare",
        "RuntimeGatewayOwnerClosedRecoveryCommandV2::Commit",
        "RuntimeGatewayOwnerSupervisorRoleV1::PreparedClosedRecovery",
        "RuntimeGatewayOwnerSupervisorRoleV1::ClosedRecovery",
        "RuntimeGatewayOwnerSupervisorRoleV1::AdmissionFrozen",
        "RuntimeGatewayOwnerSupervisorCommandV1::EnterAdmissionFrozen",
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
        "    prepared_closed_recovery_observation: Option<RuntimeGatewayOwnerCurrentObservationV1>,\n",
        "}"
    )));
    assert!(!owner_supervisor.contains("prepare_closed_recovery_observation_v2"));
    assert!(owner_supervisor.contains(concat!(
        "#[cfg(test)]\n",
        "#[path = \"gateway_owner_startup_watchdog_handoff_tests.rs\"]\n",
        "mod handoff_tests;"
    )));
    for name in [
        "RuntimeGatewayOwnerSupervisorHandleV1",
        "RuntimeGatewayOwnerStartupWatchdogHandleV1",
        "RuntimeGatewayOwnerPreparedClosedRecoveryV2",
        "RuntimeGatewayOwnerClosedRecoverySupervisorV2",
        "RuntimeGatewayOwnerAdmissionFrozenSupervisorV2",
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
fn maintenance_ingress_gate_is_counted_linear_fail_closed_and_confined() {
    let sources = source_files();
    let gate = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/maintenance_ingress_gate.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    let production = source_before_test_module(gate);
    let library = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/lib.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    assert!(library.contains(concat!(
        "#[cfg_attr(not(test), allow(dead_code))]\n",
        "mod maintenance_ingress_gate;"
    )));
    assert!(!library.contains("pub mod maintenance_ingress_gate"));
    assert!(!library.contains("RuntimeMaintenanceIngressGate"));
    for required in [
        "RuntimeMaintenanceIngressGateStageV2",
        "RuntimeMaintenanceIngressGateSnapshotV2",
        "RuntimeMaintenanceIngressGateControllerV2",
        "RuntimeMaintenanceIngressGateOpeningAuthorityV2",
        "RuntimeMaintenanceIngressGateOpenAuthorityV2",
        "RuntimeMaintenanceIngressGatePermitV2",
        "RuntimeMaintenanceIngressGateDrainHandleV2",
        "RuntimeMaintenanceIngressGateObserverV2",
        "RuntimeMaintenanceIngressGateShutdownHandleV2",
        "shutdown_sealed: bool",
        "terminal_error: Option<RuntimeMaintenanceIngressGateErrorV2>",
        "checked_add(1)",
        ".filter(|value| *value <= i64::MAX as u64)",
        ".filter(|count| *count <= i64::MAX as u64)",
        "fail_closed_state_v2(&mut state, error)",
        "shared.state.clear_poison()",
        "RuntimeMaintenanceIngressGateErrorV2::Poisoned",
        "RuntimeMaintenanceIngressGateStageV2::Opening",
        "RuntimeMaintenanceIngressGateStageV2::Closing",
        "state.snapshot.active_permit_count -= 1;",
        "pub(crate) fn try_acquire_v2(",
        "pub(crate) async fn wait_closed_until_v2(",
        "pub(crate) fn seal_shutdown_v2(&self)",
        "RuntimeMaintenanceIngressGateErrorV2::ShutdownSealed",
    ] {
        assert!(production.contains(required), "{required}");
    }
    for name in [
        "RuntimeMaintenanceIngressGateControllerV2",
        "RuntimeMaintenanceIngressGateOpeningAuthorityV2",
        "RuntimeMaintenanceIngressGateOpenAuthorityV2",
        "RuntimeMaintenanceIngressGatePermitV2",
        "RuntimeMaintenanceIngressGateDrainHandleV2",
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
    for name in [
        "RuntimeMaintenanceIngressGateObserverV2",
        "RuntimeMaintenanceIngressGateShutdownHandleV2",
    ] {
        let attributes = declaration_attribute_block(production, name);
        assert!(contains_identifier(attributes, "Clone"), "{name}");
        for forbidden in ["Copy", "Default", "Serialize", "Deserialize"] {
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
        "RuntimePublicAdmission",
        "RuntimeInteraction",
        "interaction",
        "route",
        "sqlx",
        "twilight",
        "unsafe",
    ] {
        assert!(!contains_identifier(production, forbidden), "{forbidden}");
    }
    let process = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/process.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    let process_observation = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/process/observation.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    let process_supervisor = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/process_supervisor.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    let process_startup = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/process_startup.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    let acknowledgement_supervisor = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/ingress_acknowledgement_supervisor.rs"))
        .map(|(_, source)| source_before_test_module(source))
        .unwrap();
    let capability_readiness_supervisor = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/capability_readiness_supervisor.rs"))
        .map(|(_, source)| source_before_test_module(source))
        .unwrap();
    let acknowledgement_safety = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/ingress_acknowledgement_safety.rs"))
        .map(|(_, source)| source_before_test_module(source))
        .unwrap();
    assert_eq!(
        process
            .matches("RuntimeMaintenanceIngressGateControllerV2")
            .count(),
        4
    );
    assert!(
        process.contains("maintenance_ingress: Option<RuntimeMaintenanceIngressGateControllerV2>,")
    );
    assert!(process.contains("self.maintenance_ingress.take()"));
    assert_eq!(
        process
            .matches("RuntimeMaintenanceIngressGateControllerV2::new_v2()")
            .count(),
        1
    );
    let invalidation_impl = braced_declaration(
        process_supervisor,
        "impl RuntimeProcessInvalidationTriggerV1",
    );
    let invalidation_trip =
        braced_declaration(invalidation_impl, "pub(crate) fn trip(&self, cause:");
    let process_shutdown = invalidation_trip.find("self.trigger.trip(cause)").unwrap();
    let shutdown_deadline = invalidation_trip
        .find("trip.observation().deadline()")
        .unwrap();
    let health_seal = invalidation_trip
        .find("self.health.seal_readiness()")
        .unwrap();
    let ingress_seal = invalidation_trip
        .find("self.maintenance_ingress.seal_shutdown_v2()")
        .unwrap();
    let acknowledgement_seal = invalidation_trip
        .find("self.ingress_acknowledgement.seal_until_v2(deadline)")
        .unwrap();
    let capability_seal = invalidation_trip
        .find("self.capability_readiness.seal_until_v2(deadline)")
        .unwrap();
    let finalizer_seal = invalidation_trip
        .find("self.finalizer.seal_intake()")
        .unwrap();
    assert!(
        process_shutdown < shutdown_deadline
            && shutdown_deadline < health_seal
            && health_seal < ingress_seal
            && ingress_seal < acknowledgement_seal
            && acknowledgement_seal < capability_seal
            && capability_seal < finalizer_seal
    );
    let shutdown_impl =
        braced_declaration(process_supervisor, "impl RuntimeProcessShutdownTriggerV1");
    let shutdown_trip = braced_declaration(shutdown_impl, "pub(crate) fn trip(&self, cause:");
    let invalidation = shutdown_trip.find("self.invalidation.trip(cause)").unwrap();
    let gateway_shutdown = shutdown_trip.find("self.gateway.enter_shutdown()").unwrap();
    assert!(invalidation < gateway_shutdown);
    assert_eq!(process_supervisor.matches(".seal_shutdown_v2()").count(), 1);
    for required in [
        "const RUNTIME_INGRESS_ACKNOWLEDGEMENT_DATA_CAPACITY: usize = 1;",
        "const RUNTIME_INGRESS_ACKNOWLEDGEMENT_CONTROL_CAPACITY: usize = 1;",
        "RuntimeIngressAcknowledgementSupervisorV2",
        "RuntimeIngressAcknowledgementShutdownHandleV2",
        "RuntimeIngressAcknowledgementTerminalObserverV2",
        "RuntimeIngressAcknowledgementAuthorityV2",
        "RuntimeWorkerIngressAcknowledgementJobV2",
        "RuntimeIngressOpenAcknowledgementSingleFlightV2",
        "let (data, data_receiver) = mpsc::channel(RUNTIME_INGRESS_ACKNOWLEDGEMENT_DATA_CAPACITY);",
        "mpsc::channel(RUNTIME_INGRESS_ACKNOWLEDGEMENT_CONTROL_CAPACITY)",
        "self.shared.seal_shutdown(deadline)",
        "state.phase = RuntimeIngressAcknowledgementSupervisorPhaseV2::ShutdownSealed;",
        "tokio::select! {\n            biased;\n            command = control.recv()",
        "command = data.recv()",
        "RuntimeIngressAcknowledgementFailureV2::SecondUncertainty",
    ] {
        assert!(
            acknowledgement_supervisor.contains(required),
            "ingress acknowledgement supervisor: {required}"
        );
    }
    assert_eq!(
        acknowledgement_supervisor
            .matches("port.publish_ingress_open_acknowledgement(attempt)")
            .count(),
        1
    );
    assert_eq!(
        acknowledgement_supervisor
            .matches("RuntimeIngressOpenAcknowledgementSingleFlightV2")
            .count(),
        2
    );
    let actor = braced_declaration(
        acknowledgement_supervisor,
        "async fn run_ingress_acknowledgement_actor_v2<",
    );
    let control = actor.find("command = control.recv()").unwrap();
    let data = actor.find("command = data.recv()").unwrap();
    assert!(control < data);
    for forbidden in [
        "interaction",
        "route",
        "consumer",
        "sqlx",
        "SELECT",
        "INSERT",
        "UPDATE",
        "DELETE",
    ] {
        assert!(
            !contains_identifier(acknowledgement_supervisor, forbidden),
            "ingress acknowledgement supervisor: {forbidden}"
        );
    }
    let empty_open = braced_declaration(process_observation, "impl RuntimeEmptyOpenProcessV2");
    for forbidden in [
        "RuntimeMaintenanceIngressGatePermitV2",
        "try_acquire_v2",
        "interaction",
        "route",
        "consumer",
    ] {
        assert!(
            !contains_identifier(empty_open, forbidden),
            "RuntimeEmptyOpenProcessV2: {forbidden}"
        );
    }
    let enter_empty_open = braced_declaration(
        process_observation,
        "pub(crate) async fn enter_empty_open_v2(",
    );
    let gate_open = enter_empty_open.find("controller.begin_open_v2()").unwrap();
    let gate_commit = enter_empty_open.find("opening.commit_open_v2()").unwrap();
    let predecessor_authorization = enter_empty_open
        .find(".authorize_ingress_acknowledgement_predecessor_observation_v2()")
        .unwrap();
    let predecessor_observation = enter_empty_open
        .find(".observe_ingress_open_acknowledgement_predecessor(&predecessor_authorization)")
        .unwrap();
    let predecessor_accept = enter_empty_open
        .find("predecessor_authorization.accept(predecessor_observation)")
        .unwrap();
    let acknowledgement_authorization = enter_empty_open
        .find(".into_ingress_acknowledgement_authority_v2(")
        .unwrap();
    let initial_acknowledgement_anchor = enter_empty_open
        .find("let initial_acknowledgement_observation_started_at = Instant::now()")
        .unwrap();
    let acknowledgement_lane = enter_empty_open
        .find("execute_ingress_acknowledgement_v2(")
        .unwrap();
    let initial_acknowledgement_schedule = enter_empty_open
        .find(
            "ingress_acknowledgement_schedule_v2(\n            accepted.receipt(),\n            initial_acknowledgement_observation_started_at,",
        )
        .unwrap();
    let acknowledgement_safety_arm = enter_empty_open
        .find("RuntimeIngressAcknowledgementSafetyMonitorV2::start_v2(")
        .unwrap();
    let owner_reobservation = enter_empty_open
        .find(".observe_current_owner_v2()")
        .unwrap();
    let final_acknowledgement_reobservation = enter_empty_open
        .find("exact_reobserve_ingress_acknowledgement_v2(")
        .unwrap();
    let open_transition = enter_empty_open
        .find(".into_empty_open_v2(observation)")
        .unwrap();
    let owner_successor_wait = enter_empty_open
        .find(".wait_for_owner_successor_v2(")
        .unwrap();
    let acknowledgement_refresh = enter_empty_open
        .find(".refresh_acknowledgement_with_owner_v2(")
        .unwrap();
    let capability_activation = enter_empty_open
        .find(".activate_capability_readiness_supervisor_v2(")
        .unwrap();
    let gateway_invalidation_arm = enter_empty_open
        .find(".arm_gateway_invalidation_trigger_v2(")
        .unwrap();
    let final_revalidation = enter_empty_open.rfind(".revalidate_v2()").unwrap();
    let readiness_publish = enter_empty_open
        .find("readiness.publish_ready_v2()")
        .unwrap();
    assert!(
        gate_open < gate_commit
            && gate_commit < predecessor_authorization
            && predecessor_authorization < predecessor_observation
            && predecessor_observation < predecessor_accept
            && predecessor_accept < acknowledgement_authorization
            && acknowledgement_authorization < initial_acknowledgement_anchor
            && initial_acknowledgement_anchor < acknowledgement_lane
            && acknowledgement_lane < initial_acknowledgement_schedule
            && initial_acknowledgement_schedule < acknowledgement_safety_arm
            && acknowledgement_safety_arm < owner_reobservation
            && owner_reobservation < final_acknowledgement_reobservation
            && final_acknowledgement_reobservation < open_transition
            && acknowledgement_safety_arm < owner_successor_wait
            && owner_successor_wait < acknowledgement_refresh
            && acknowledgement_refresh < capability_activation
            && capability_activation < gateway_invalidation_arm
            && gateway_invalidation_arm < final_revalidation
            && final_revalidation < readiness_publish
    );
    assert_eq!(
        enter_empty_open
            .matches("RuntimeIngressAcknowledgementSafetyMonitorV2::start_v2(")
            .count(),
        1
    );
    assert!(!enter_empty_open.contains(".rearm_v2("));
    assert!(!enter_empty_open
        .contains("ingress_acknowledgement_schedule_v2(\n            &final_acknowledgement,"));
    assert!(process_observation
        .contains("acknowledgement_safety: Option<RuntimeIngressAcknowledgementSafetyMonitorV2>,"));
    let admission_shutdown = braced_declaration(
        process_observation,
        "async fn shutdown_admission_acknowledging_process_v2(",
    );
    let admission_monitor_stop = admission_shutdown
        .find("acknowledgement_safety.stop_v2().await")
        .unwrap();
    let admission_lane_shutdown = admission_shutdown
        .find("shutdown_ingress_acknowledgement_supervisor_v2(")
        .unwrap();
    assert!(admission_monitor_stop < admission_lane_shutdown);
    assert!(enter_empty_open.contains(
        "RuntimeIngressOpenAcknowledgementLeaseDurationV2::from_duration(\n            INGRESS_ACKNOWLEDGEMENT_LEASE_V2,"
    ));
    let acknowledgement_lane = braced_declaration(
        process_observation,
        "async fn execute_ingress_acknowledgement_v2(",
    );
    let submit = acknowledgement_lane
        .find("supervisor.try_submit(job, deadline)")
        .unwrap();
    let detach_waiter = acknowledgement_lane.find("waiter.cancel_v2()").unwrap();
    let completion = acknowledgement_lane
        .find("supervisor.recv_completion().await")
        .unwrap();
    assert!(submit < detach_waiter && detach_waiter < completion);
    for required in [
        "RuntimeIngressAcknowledgementExecutionResultV2::Accepted(outcome)",
        "RuntimeIngressAcknowledgementExecutionResultV2::CompletionRejected",
        "RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed",
        "rejection.into_job().into_authority()",
        "job.into_authority()",
    ] {
        assert!(acknowledgement_lane.contains(required), "{required}");
    }
    let refresh = braced_declaration(
        empty_open,
        "async fn refresh_acknowledgement_with_owner_v2(",
    );
    assert!(refresh.contains("refresh.into_ingress_acknowledgement_authority_v2()"));
    assert_eq!(
        refresh
            .matches("execute_ingress_acknowledgement_v2(")
            .count(),
        1
    );
    let final_observation_anchor = refresh
        .find("let final_observation_started_at = Instant::now()")
        .unwrap();
    let final_observation = refresh
        .find("exact_reobserve_ingress_acknowledgement_v2(")
        .unwrap();
    let schedule = refresh
        .find("ingress_acknowledgement_schedule_v2(receipt, final_observation_started_at)")
        .unwrap();
    let safety_rearm = refresh.find(".rearm_v2(schedule.safety_deadline)").unwrap();
    let refresh_revalidation = refresh.rfind(".revalidate_v2()").unwrap();
    assert!(
        final_observation_anchor < final_observation
            && final_observation < schedule
            && schedule < safety_rearm
            && safety_rearm < refresh_revalidation
    );
    assert_eq!(
        enter_empty_open
            .matches("execute_ingress_acknowledgement_v2(")
            .count(),
        1
    );
    assert!(enter_empty_open.contains(
        "lifecycle.into_ingress_acknowledgement_authority_v2(\n            open_generation,"
    ));
    assert!(refresh.contains("maintenance_gate_generation: gate.generation()"));
    for required in [
        "const CAPABILITY_READINESS_CONTROL_CAPACITY_V2: usize = 1;",
        "const CAPABILITY_READINESS_CADENCE_V2: Duration = Duration::from_secs(1);",
        "const CAPABILITY_READINESS_VERIFY_TIMEOUT_V2: Duration = Duration::from_secs(5);",
        "mpsc::channel(CAPABILITY_READINESS_CONTROL_CAPACITY_V2)",
        "tokio::select! {\n        biased;\n        changed = shutdown.changed()",
        "invalidation.invalidate_readiness_v2();",
    ] {
        assert!(
            capability_readiness_supervisor.contains(required),
            "capability readiness supervisor: {required}"
        );
    }
    assert_eq!(
        capability_readiness_supervisor
            .matches("invalidation.invalidate_readiness_v2();")
            .count(),
        3
    );
    for forbidden in [
        "interaction",
        "route",
        "consumer",
        "sqlx",
        "SELECT",
        "INSERT",
        "UPDATE",
        "DELETE",
    ] {
        assert!(
            !contains_identifier(capability_readiness_supervisor, forbidden),
            "capability readiness supervisor: {forbidden}"
        );
    }
    for required in [
        "RuntimeIngressAcknowledgementSafetyMonitorV2",
        "RuntimeIngressAcknowledgementSafetyStageV2::Armed",
        "RuntimeIngressAcknowledgementSafetyStageV2::Expired",
        "deadline <= current.deadline",
        "current.generation.checked_add(1)",
        "tokio::select! {\n                biased;",
        "task.abort();",
        "let _joined = task.await;",
        "invalidate_ingress_acknowledgement_v2();",
    ] {
        assert!(
            acknowledgement_safety.contains(required),
            "acknowledgement safety: {required}"
        );
    }
    for forbidden in [
        "interaction",
        "route",
        "consumer",
        "sqlx",
        "SELECT",
        "INSERT",
        "UPDATE",
        "DELETE",
    ] {
        assert!(
            !contains_identifier(acknowledgement_safety, forbidden),
            "acknowledgement safety: {forbidden}"
        );
    }
    for forbidden in [
        "async fn publish_ingress_acknowledgement_v2",
        ".begin_attempt()",
        ".publish_ingress_open_acknowledgement(attempt)",
    ] {
        assert!(
            !process_observation.contains(forbidden),
            "process-owned acknowledgement lane: {forbidden}"
        );
    }
    let empty_open_shutdown = braced_declaration(
        process_observation,
        "async fn shutdown_empty_open_process_v2(",
    );
    let readiness_remove = empty_open_shutdown
        .find("readiness.remove_readiness_v2()")
        .unwrap();
    let gate_drop = empty_open_shutdown
        .find("drop(maintenance_ingress)")
        .unwrap();
    let root_shutdown = empty_open_shutdown.find(".begin_shutdown_v1(").unwrap();
    assert!(readiness_remove < gate_drop && gate_drop < root_shutdown);
    let startup = braced_declaration(
        process_startup,
        "async fn stage_runtime_process_from_environment_v1(",
    );
    let startup_open = startup.find(".enter_empty_open_v2()").unwrap();
    let startup_serving = startup.find(".enter_serving_open_v2()").unwrap();
    let startup_run = startup.find(".run_until_shutdown_v2()").unwrap();
    let startup_done = startup
        .find("Ok(RuntimeProcessStagingOutcomeV1 { _private: () })")
        .unwrap();
    assert!(
        startup_open < startup_serving
            && startup_serving < startup_run
            && startup_run < startup_done
    );
    assert!(!startup.contains("empty_open\n        .shutdown()"));
    for forbidden in [
        "RuntimeMaintenanceIngressGatePermitV2",
        "try_acquire_v2",
        "interaction",
        "route",
        "consumer",
    ] {
        assert!(
            !contains_identifier(startup, forbidden),
            "EmptyOpen staging: {forbidden}"
        );
    }
    for (path, source) in sources.iter().filter(|(path, _)| path.starts_with("src")) {
        if path == Path::new("src/maintenance_ingress_gate.rs") {
            continue;
        }
        let source = if path == Path::new("src/process_supervisor.rs") {
            source_before_test_module(source)
        } else {
            source
        };
        if path != Path::new("src/process.rs") {
            assert!(
                !contains_identifier(source, "RuntimeMaintenanceIngressGateControllerV2"),
                "{}",
                path.display()
            );
        }
        assert!(
            !contains_identifier(source, "RuntimeMaintenanceIngressGatePermitV2"),
            "{}",
            path.display()
        );
        assert!(
            !contains_identifier(source, "try_acquire_v2"),
            "{}",
            path.display()
        );
    }
}

#[test]
fn paused_discord_connection_is_single_owned_closed_and_bounded() {
    let discord = source_before_test_module(include_str!("../src/discord.rs"));
    let gateway = source_before_test_module(include_str!("../src/gateway.rs"));
    let connected = include_str!("../src/process/connected.rs");
    let startup = source_before_test_module(include_str!("../src/process_startup.rs"));
    let watchdog = include_str!("../src/gateway_owner_startup_watchdog.rs");
    let library = include_str!("../src/lib.rs");

    for required in [
        "Shard::new(ShardId::ONE, token, Intents::empty())",
        "EventTypeFlags::READY",
        "EventTypeFlags::RESUMED",
        "EventTypeFlags::GATEWAY_RECONNECT",
        "EventTypeFlags::GATEWAY_INVALIDATE_SESSION",
        "GatewayCommandAckV3::Paused { .. }",
        "GatewayCommandAckV3::AdmissionResumed { .. }",
        "RuntimeDiscordGatewayExitV1::AdmissionOpened",
        "RuntimeDiscordGatewayTransportStateV1::Connecting",
        "RuntimeDiscordGatewayTransportStateV1::Active",
        "RuntimeDiscordGatewayTransportStateV1::Disconnected",
        "GatewayCommandAckV3::Draining { .. }",
        "control.mark_connected(GatewayReadyKindV3::Ready)",
        "control.mark_connected(GatewayReadyKindV3::Resumed)",
        "control.mark_disconnected(GatewayDisconnectKindV3::Reconnect)",
        "driver.close_until(close_deadline).await",
        "control.mark_stopped()",
        "self.shard.close(CloseFrame::NORMAL)",
        "actor_abort: AbortHandle",
        "control_abort: AbortHandle",
        "join_task: Option<JoinHandle<bool>>",
        "impl Drop for RuntimeDiscordControlTaskV1",
        "let actor_joined = actor_task.await.is_ok();",
        "let _control_result = control_task.await;",
        "let _stopped = stopped_sender.send(true);",
        "self.abort_tasks();",
        "finish_runtime_discord_gateway_without_transport_v1(",
        "finish_runtime_discord_gateway_if_connected_v1(",
        "start_receiver.await.is_ok()",
        "wait_for_lifecycle_drain_v1(",
        "RuntimeDiscordGatewaySupervisorV1(<redacted>)",
    ] {
        assert!(discord.contains(required), "{required}");
    }
    assert_eq!(discord.matches("runtime.spawn(async move").count(), 2);
    for forbidden in [
        "INTERACTION_CREATE",
        "twilight_http",
        "Client",
        "run_shared_gateway_v3",
        "resume_admission",
        "issue_ready_lease",
        "bot_runtime",
        "InteractionCreate",
        "InteractionResponder",
        "transport_started",
        "catch_unwind",
    ] {
        assert!(!discord.contains(forbidden), "discord: {forbidden}");
    }

    for required in [
        "_runtime: Option<SharedGatewayRuntimeHalfV3>",
        "self._runtime.take()",
        "GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect",
        "pub(crate) fn admission_change_watch_v1(",
        "pub(crate) fn begin_discord_drain_v1(",
        "run_runtime_discord_control_v1(",
        "control.next_lifecycle()",
        "prepare_twilight_runtime_discord_gateway_driver_v1(",
        "prepare_discord_gateway_start_v1(operation_cutoff, Some(shutdown))",
        "shutdown: &mut RuntimeShutdownObserverV1,",
        "_observation = shutdown.wait()",
        "supervisor.release_start_v1()",
        "owner_discord_attachment:",
        "abort_handle.abort()",
        "fn gateway_shutdown_watch(&self) -> Option<watch::Receiver<bool>>",
    ] {
        assert!(gateway.contains(required), "{required}");
    }
    assert_eq!(gateway.matches("start_discord_gateway_v1(").count(), 1);
    assert_eq!(
        gateway
            .matches("prepare_twilight_runtime_discord_gateway_driver_v1(")
            .count(),
        1
    );

    for required in [
        "pub(crate) struct RuntimeDiscordStartingProcessV1",
        "pub(crate) struct RuntimePausedConnectedProcessV1",
        "owner_held: RuntimeOwnerHeldProcessV1",
        "discord: RuntimeDiscordGatewaySupervisorV1",
        "paused_gateway: RuntimePausedGatewayObservationV2",
        "pub(crate) async fn wait_for_paused_connected_v1(",
        ".observe_paused_connected_gateway_v2()",
        "Ok(current) if current == first => {",
        "self.require_live_paused_connection_v1()?",
        "pub(crate) fn into_paused_connected_v1(",
        "RuntimeDiscordStartingProcessV1(<redacted>)",
        "RuntimePausedConnectedProcessV1(<redacted>)",
        "shutdown_paused_discord_owner_v1",
        "shutdown_paused_foundation_owner_v1",
        ".begin_shutdown_v1(RuntimeShutdownCauseV1::Explicit)",
        "foundation.observe_shutdown_registry_v1()",
        ".begin_discord_drain_v1()",
        "foundation.finish_shutdown_v1(cleanup_deadline).await",
    ] {
        assert!(connected.contains(required), "{required}");
    }
    let connected_attributes =
        declaration_attribute_block(connected, "RuntimePausedConnectedProcessV1");
    for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
        assert!(
            !contains_identifier(connected_attributes, forbidden),
            "{forbidden}"
        );
        assert!(!implements_trait(
            connected,
            "RuntimePausedConnectedProcessV1",
            forbidden,
        ));
    }
    for marker in [
        "pub(crate) struct RuntimeDiscordStartingProcessV1",
        "pub(crate) struct RuntimePausedConnectedProcessV1",
    ] {
        let declaration = braced_declaration(connected, marker);
        assert!(declaration.find("discord:").unwrap() < declaration.find("owner_held:").unwrap());
    }
    let connected_shutdown = braced_declaration(
        connected,
        "pub(super) async fn shutdown_paused_foundation_owner_v1<",
    );
    let begin = connected_shutdown.find(".begin_shutdown_v1(").unwrap();
    let registry = connected_shutdown
        .find("foundation.observe_shutdown_registry_v1()")
        .unwrap();
    let discord_drain = connected_shutdown
        .find(".begin_discord_drain_v1()")
        .unwrap();
    let discord_shutdown = connected_shutdown.find(".shutdown_until(").unwrap();
    let owner_shutdown = connected_shutdown
        .find("shutdown_owner(owner_cleanup_deadline).await")
        .unwrap();
    let foundation_shutdown = connected_shutdown
        .find("foundation.finish_shutdown_v1(cleanup_deadline).await")
        .unwrap();
    assert!(
        begin < registry
            && registry < discord_drain
            && discord_drain < discord_shutdown
            && discord_shutdown < owner_shutdown
            && owner_shutdown < foundation_shutdown
    );
    let start = braced_declaration(gateway, "pub(crate) async fn start_discord_gateway_v1(");
    let driver = start
        .find("prepare_twilight_runtime_discord_gateway_driver_v1(")
        .unwrap();
    let prepare = start
        .find("prepare_discord_gateway_start_v1(operation_cutoff, Some(shutdown))")
        .unwrap();
    let spawn = start.find("start_runtime_discord_gateway_v1(").unwrap();
    let attach = start.find("attach_discord_supervisor_v1").unwrap();
    let release = start.find("supervisor.release_start_v1()").unwrap();
    assert!(driver < prepare && prepare < spawn && spawn < attach && attach < release);
    let prepare = braced_declaration(gateway, "async fn prepare_discord_gateway_start_v1(");
    let reserve = prepare.find("reserve_discord_supervisor_v1").unwrap();
    let recheck = prepare[reserve..]
        .find("self.owner_invalidated.load(Ordering::Acquire)")
        .map(|offset| reserve + offset)
        .unwrap();
    let runtime_take = prepare.find("self._runtime.take()").unwrap();
    assert!(reserve < recheck && recheck < runtime_take);
    for forbidden in [
        "resume_admission",
        "issue_ready_lease",
        "dispatch",
        "execute",
        "serve",
        "activate",
        "deploy",
        "twilight",
    ] {
        assert!(
            !contains_identifier(connected, forbidden),
            "connected: {forbidden}"
        );
        assert!(
            !contains_identifier(startup, forbidden),
            "startup: {forbidden}"
        );
    }
    assert!(!contains_identifier(connected, "readiness"));
    assert!(!contains_identifier(connected, "recovery"));
    assert!(!library.contains("RuntimeDiscordStartingProcessV1"));
    assert!(!library.contains("RuntimePausedConnectedProcessV1"));

    let watchdog_supervisor = braced_declaration(
        watchdog,
        "async fn run_gateway_owner_startup_watchdog_v1<P>(",
    );
    let invalidation = watchdog_supervisor
        .rfind("guard.invalidate_now();")
        .unwrap();
    let wait = watchdog_supervisor[invalidation..]
        .find("wait_for_emergency_gateway_shutdown_v1(")
        .map(|offset| invalidation + offset)
        .unwrap();
    let release = watchdog_supervisor[wait..]
        .find("release_gateway_owner_v1(&port, lease_id, cleanup_deadline)")
        .map(|offset| wait + offset)
        .unwrap();
    assert!(invalidation < wait && wait < release);
}

#[test]
fn discord_actor_handoff_is_modeful_reserved_bounded_and_non_serving() {
    let discord = source_before_test_module(include_str!("../src/discord.rs"));
    let lifecycle = include_str!("../src/discord_lifecycle.rs");
    let gateway = source_before_test_module(include_str!("../src/gateway.rs"));
    let startup = source_before_test_module(include_str!("../src/process_startup.rs"));

    for required in [
        "RuntimeDiscordActorModeV2",
        "StartupPaused",
        "ProcessSupervised",
        "Draining",
        "RuntimeDiscordAdmissionReservationSnapshotV2",
        "RuntimeDiscordPauseReservationIdentityV2",
        "GatewayAdmissionSnapshotV3",
        "GatewayPauseTokenV3",
    ] {
        assert!(lifecycle.contains(required), "{required}");
    }
    for required in [
        "process_handoff: Option<oneshot::Sender<RuntimeDiscordProcessHandoffCommandV2>>",
        "drain: Option<oneshot::Sender<RuntimeDiscordDrainCommandV2>>",
        "recovery_resume: mpsc::Sender<RuntimeDiscordRecoveryResumeCommandV2>",
        "RuntimeDiscordShutdownOnlySupervisorV2",
        "RuntimeDiscordProcessHandoffV2::Indeterminate",
        "wait_for_runtime_discord_acknowledgement_v2(",
        "startup_operation_cutoff",
        "RuntimeDiscordActorModeV2::ProcessSupervised",
        "RuntimeDiscordActorModeV2::Draining",
        "resume_reserved_runtime_discord_admission_v2(",
        "RuntimeDiscordGatewayExitV1::AdmissionOpened",
    ] {
        assert!(discord.contains(required), "{required}");
    }
    for required in [
        "DISCORD_CONTROL_OPERATION_TIMEOUT",
        "control.pause_admission()",
        "RuntimeDiscordAdmissionReservationSnapshotV2::reserved(",
        "discord_reservation.send_replace(snapshot)",
        "require_discord_pause_reservation_v2",
        "reserved_resume_receiver",
    ] {
        assert!(gateway.contains(required), "{required}");
    }
    assert!(!startup.contains("handoff_to_process_v2"));
    assert!(!discord.contains("execute_admitted_interaction"));
    assert!(!discord.contains("RuntimePublicAdmissionPermit"));
    assert!(!gateway.contains("RuntimePublicAdmissionPermit"));
}

#[test]
fn recovery_pending_process_is_fresh_closed_linear_and_bounded() {
    let recovery = source_before_test_module(include_str!("../src/process/recovery.rs"));
    let identity = source_before_test_module(include_str!("../src/recovery_identity.rs"));
    let closed = include_str!("../src/closed_recovery.rs");
    let startup = source_before_test_module(include_str!("../src/process_startup.rs"));
    let library = include_str!("../src/lib.rs");

    for required in [
        "pub(crate) struct RuntimeRecoveryPendingProcessV2",
        "discord: RuntimeDiscordGatewaySupervisorV1,",
        "foundation: RuntimeProcessFoundationV1,",
        "pending: RuntimeClosedRecoveryPendingPhaseV2,",
        "RuntimeRecoveryPendingProcessV2(<redacted>)",
        "pub(crate) async fn into_recovery_pending_v2(",
        "self.require_current_paused_connection_v1()",
        "generate_runtime_recovery_id_v2()",
        "owner.prepare_closed_recovery_in_place_v2()",
        "owner.try_into_prepared_closed_recovery_v2()",
        "foundation.databases.verify_readiness_v1()",
        "require_prepared_paused_connection_v2(",
        "begin_initial_empty_recovery_retained_v2(",
        "&paused_gateway,",
        ".revalidate_v2()",
        "shutdown_prepared_recovery_v2(",
        "shutdown_pending_recovery_v2(",
        "shutdown_paused_foundation_owner_v1(",
        "RuntimeProcessRecoveryPendingTransitionErrorV2(<redacted>)",
        "RuntimeRecoveryPendingProcessShutdownErrorV2(<redacted>)",
    ] {
        assert!(recovery.contains(required), "{required}");
    }
    let transition = braced_declaration(recovery, "pub(crate) async fn into_recovery_pending_v2(");
    let initial_check = transition
        .find("self.require_current_paused_connection_v1()")
        .unwrap();
    let recovery_id = transition
        .find("generate_runtime_recovery_id_v2()")
        .unwrap();
    let prepare = transition
        .find("owner.prepare_closed_recovery_in_place_v2()")
        .unwrap();
    let prepared = transition
        .find("owner.try_into_prepared_closed_recovery_v2()")
        .unwrap();
    let readiness = transition
        .find("foundation.databases.verify_readiness_v1()")
        .unwrap();
    let exact_recheck = transition
        .find("require_prepared_paused_connection_v2(")
        .unwrap();
    let begin = transition
        .find("begin_initial_empty_recovery_retained_v2(")
        .unwrap();
    let final_recheck = transition[begin..]
        .find(".revalidate_v2()")
        .map(|offset| begin + offset)
        .unwrap();
    assert!(
        initial_check < recovery_id
            && recovery_id < prepare
            && prepare < prepared
            && prepared < readiness
            && readiness < exact_recheck
            && exact_recheck < begin
            && begin < final_recheck
    );
    assert_eq!(
        transition
            .matches("generate_runtime_recovery_id_v2()")
            .count(),
        1
    );
    assert_eq!(
        transition
            .matches("foundation.databases.verify_readiness_v1()")
            .count(),
        1
    );
    assert!(!transition.contains("initial_readiness()"));
    for shutdown_name in [
        "async fn shutdown_prepared_recovery_v2(",
        "async fn shutdown_pending_recovery_v2(",
    ] {
        let shutdown = braced_declaration(recovery, shutdown_name);
        assert!(
            shutdown.contains("shutdown_paused_foundation_owner_v1("),
            "{shutdown_name}"
        );
        assert!(
            shutdown.contains(".abort_and_shutdown_until_v2(deadline)"),
            "{shutdown_name}"
        );
        assert!(!shutdown.contains("foundation.shutdown().await"));
    }
    let state_attributes = declaration_attribute_block(recovery, "RuntimeRecoveryPendingProcessV2");
    for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
        assert!(
            !contains_identifier(state_attributes, forbidden),
            "{forbidden}"
        );
        assert!(!implements_trait(
            recovery,
            "RuntimeRecoveryPendingProcessV2",
            forbidden,
        ));
    }
    for required in [
        "getrandom::fill(bytes)",
        "RuntimeRecoveryIdV2::parse(encode_runtime_identity_lower_hex_v1(bytes))",
        "RuntimeRecoveryIdGenerationErrorV2(<redacted>)",
        "let mut bytes = [0_u8; RUNTIME_IDENTITY_ENTROPY_BYTES];",
        "fill(&mut bytes)?;",
    ] {
        assert!(identity.contains(required), "{required}");
    }
    assert_eq!(identity.matches("getrandom::fill").count(), 1);
    assert_eq!(
        recovery
            .matches("generate_runtime_recovery_id_v2()")
            .count(),
        1
    );
    for forbidden in [
        "SystemTime",
        "process::id",
        "hostname",
        "Uuid",
        "uuid",
        "thread_rng",
        "StdRng",
        "getrandom::u32",
        "getrandom::u64",
        "OnceLock",
        "LazyLock",
        "thread_local",
        "Atomic",
        "hash(",
        "xor",
    ] {
        assert!(!identity.contains(forbidden), "{forbidden}");
    }
    for forbidden in [
        "resume_admission",
        "issue_ready_lease",
        "ready_to_serve",
        "health_ready",
        "serving_lease",
        "heartbeat",
        "dispatch",
        "execute",
        "hydrate",
        "reconcile",
        "activate",
        "deploy",
        "commit_owner_v2",
        "observe_startup_recovery_v2",
    ] {
        assert!(!contains_identifier(recovery, forbidden), "{forbidden}");
        assert!(
            !contains_identifier(startup, forbidden),
            "startup: {forbidden}"
        );
    }
    for required in [
        "pub(crate) fn begin_initial_empty_recovery_retained_v2(",
        "pub(crate) async fn abort_and_shutdown_until_v2(",
        ".abort_and_shutdown_until_v2(cleanup_deadline)",
        "expected_paused_gateway: &RuntimePausedGatewayObservationV2,",
        ".initial_emergency_gateway_section_v2(owner, expected_paused_gateway)",
    ] {
        assert!(closed.contains(required), "{required}");
    }
    assert!(closed.contains(concat!(
        "#[cfg(test)]\n",
        "pub(crate) fn begin_initial_empty_recovery_v2("
    )));
    assert!(!library.contains("RuntimeRecoveryPendingProcessV2"));
    assert!(!library.contains("RuntimeClosedRecoveryPendingPhaseV2"));
    assert!(!library.contains("RuntimeGatewayOwnerPreparedClosedRecoveryV2"));
}

#[test]
fn serving_open_successor_is_linear_bounded_refreshable_and_non_mutating() {
    let sources = source_files();
    let serving = include_str!("../src/process/serving.rs");
    let closed = include_str!("../src/closed_recovery.rs");
    let startup = source_before_test_module(include_str!("../src/process_startup.rs"));
    let transition = braced_declaration(serving, "pub(crate) async fn enter_serving_open_v2(");
    let prepare = transition
        .find(".prepare_serving_open_v2(config, serving_evidence)")
        .unwrap();
    let commit = transition.find("prepared.commit_v2()").unwrap();
    let revalidate = transition.rfind("process.revalidate_v2().await").unwrap();
    assert!(prepare < commit && commit < revalidate);
    assert_eq!(startup.matches(".enter_empty_open_v2()").count(), 1);
    assert_eq!(startup.matches(".enter_serving_open_v2()").count(), 1);
    assert_eq!(startup.matches(".run_until_shutdown_v2()").count(), 1);
    let empty = startup.find(".enter_empty_open_v2()").unwrap();
    let successor = startup.find(".enter_serving_open_v2()").unwrap();
    let run = startup.find(".run_until_shutdown_v2()").unwrap();
    assert!(empty < successor && successor < run);
    for required in [
        "const SERVING_OPEN_SLOT_WORK_CAPACITY_V2: usize = 1;",
        "RuntimeClosedRecoveryServingOpenEvidenceV2",
        "RuntimeClosedRecoveryServingAcknowledgementEvidenceV2",
        ".authorize_acknowledgement_refresh_v2(evidence)",
        "RuntimeClosedRecoveryIngressAcknowledgementOutcomeV2::ServingOpenRefresh",
        "RuntimeClosedRecoveryIngressAcknowledgementRetainedStateV2::ServingOpenRefresh",
        "shutdown_serving_open_process_v2(",
        "shutdown_refreshing_serving_open_process_v2(",
    ] {
        assert!(serving.contains(required), "{required}");
    }
    for required in [
        "RuntimeClosedRecoveryPreparedServingOpenProcessV2",
        "RuntimeClosedRecoverySupervisedServingOpenProcessV2",
        "RuntimeRegistryPreparedServingTransitionV2",
        "RuntimeRegistryServingBindingV2",
        "worker.prepare_serving_open(&port, config)",
        "registry.commit_v2(worker.route_set_epoch())",
        "let route_set = match self.observe_registry_route_set_v2()",
        "RuntimeServingOpenAcknowledgementRefreshInputV2",
        "RuntimeClosedRecoveryIngressAcknowledgementAuthorityV2::ServingOpenRefresh",
    ] {
        assert!(closed.contains(required), "{required}");
    }
    for forbidden in [
        "Shard::new(",
        "run_shared_gateway_v3",
        "begin_paused_discord_connection_v1",
        "enter_empty_open_v2",
        "RuntimeMaintenanceIngressGatePermitV2",
        "try_acquire_v2",
        "INTERACTION_CREATE",
        "hydrate",
        "activate",
        "deploy",
        "sqlx",
        "SELECT",
        "INSERT",
        "UPDATE",
        "DELETE",
    ] {
        assert!(
            !contains_identifier(serving, forbidden),
            "serving: {forbidden}"
        );
    }
    assert_eq!(
        sources
            .iter()
            .filter(|(path, _)| path.starts_with("src"))
            .map(|(_, source)| source.matches("Shard::new(").count())
            .sum::<usize>(),
        1
    );
}

#[test]
fn runtime_controller_is_exact_refreshing_installation_scoped_and_cleanup_ordered() {
    let controller = source_before_test_module(include_str!("../src/runtime_controller.rs"));
    let serving = include_str!("../src/process/serving.rs");

    for required in [
        "const RUNTIME_CONTROLLER_COMMAND_CAPACITY_V2: usize = 1;",
        "claim_next_execution(RuntimeClaimNextExecutionV1",
        "prepare_claim_v2(retained_receipt).await",
        "runtime_discord_preflight_blocking_mutation_v2()",
        "RuntimeConvergenceMutationV1::Cancel",
        "reason: \"runtime_discord_preflight_blocked\".to_owned()",
        "stage_ready.begin_exact_hydration_refresh()",
        "refresh.execute(&self.database)",
        "stage_ready.stage(&stage, Utc::now())",
        "RuntimeControllerDiscordPreflightDispositionV2::DeploymentBlocked",
        "self.cancel_blocked_preflight_v2(preflight_failure_receipt)",
        "RuntimeControllerAttemptV2::RetryRetained(Box::new(",
        "retained_receipt = receipt",
        "runtime_claim_requires_renewal_v2(receipt.expires_at, &self.config, now)",
        "RuntimeHeldRouteOutcomeV2::Retry",
        "held.finish_retry_v2()",
    ] {
        assert!(controller.contains(required), "{required}");
    }
    for forbidden in [
        "RuntimeEmergencyGatewaySection",
        "RuntimeRecoveryPendingGatewaySection",
        "GatewayBarrier",
        "gateway_barrier",
        ".activate(",
        "resume_reserved_runtime_discord_admission",
    ] {
        assert!(!controller.contains(forbidden), "{forbidden}");
    }

    let converge = braced_declaration(controller, "async fn converge_claim_v2(");
    let initial_hydration = converge.find("claimed.begin_hydration()").unwrap();
    let discord_preflight = converge.find("preflight.execute(&self.discord)").unwrap();
    let accept_preflight = converge.find("mutation.execute(&self.database").unwrap();
    let exact_refresh = converge
        .find("stage_ready.begin_exact_hydration_refresh()")
        .unwrap();
    let registry_stage = converge
        .find("stage_ready.stage(&stage, Utc::now())")
        .unwrap();
    assert!(
        initial_hydration < discord_preflight
            && discord_preflight < accept_preflight
            && accept_preflight < exact_refresh
            && exact_refresh < registry_stage
    );

    let held = braced_declaration(controller, "struct RuntimeHeldStagedRouteV2");
    assert!(
        held.find("staged: RuntimeRegistryStagedRouteV2").unwrap()
            < held.find("permit: RuntimeServingSlotWorkPermitV2").unwrap()
    );
    let renewal = braced_declaration(controller, "async fn hold_staged_v2(");
    let database_renewal = renewal.find(".renew_execution(renewal)").unwrap();
    let session_renewal = renewal.find("held.session.apply_renewal(renewal)").unwrap();
    let registry_renewal = renewal
        .find(".advance_authority_v2(held.session.fencing_token())")
        .unwrap();
    assert!(database_renewal < session_renewal && session_renewal < registry_renewal);
    let finish = braced_declaration(controller, "fn finish_v2(");
    assert!(
        finish.find("staged.remove_v2()").unwrap() < finish.find("drop((session, permit").unwrap()
    );

    assert!(!serving.contains("OwnedDiscordRuntimePreflightV1"));
    let shutdown = braced_declaration(serving, "async fn shutdown(mut self)");
    assert!(
        shutdown
            .find("stop_runtime_controller_before_cleanup_v2")
            .unwrap()
            < shutdown.find("shutdown_serving_open_process_v2(").unwrap()
    );
}

#[test]
fn serving_shutdown_observes_valid_route_sets_without_weakening_empty_shutdown() {
    let process = source_before_test_module(include_str!("../src/process.rs"));
    let observation = include_str!("../src/process/observation.rs");
    let serving_process = include_str!("../src/process/serving.rs");
    let empty = braced_declaration(
        process,
        "pub(super) fn observe_shutdown_registry_v1(&mut self)",
    );
    let serving = braced_declaration(
        process,
        "pub(super) fn observe_shutdown_serving_registry_v2(",
    );

    assert!(empty.contains("self.registry.observe_recovery_empty_projection_v2()"));
    assert!(!empty.contains("RuntimeRouteSetObservationV2"));
    assert!(serving.contains("RuntimeRouteSetObservationV2"));
    assert!(serving.contains("accept_shutdown_serving_registry_observation_v2(observation)"));
    assert!(!serving.contains("observe_recovery_empty_projection_v2()"));
    let lost_serving = braced_declaration(
        process,
        "pub(super) fn observe_shutdown_serving_registry_without_lifecycle_v2(&mut self)",
    );
    assert!(lost_serving.contains("self.registry.observe_shutdown_route_set_v2()"));
    assert!(lost_serving.contains("self.observe_shutdown_serving_registry_v2(observation)"));

    for marker in [
        "pub(super) async fn shutdown_serving_open_process_v2(",
        "pub(super) async fn shutdown_refreshing_serving_open_process_v2(",
    ] {
        let shutdown = braced_declaration(observation, marker);
        let sealed = shutdown
            .find("let (lifecycle, registry_observation) = lifecycle.begin_shutdown_v2();")
            .unwrap();
        let first_await = shutdown.find(".await").unwrap();
        let foundation = shutdown
            .find("foundation.observe_shutdown_serving_registry_v2(registry_observation);")
            .unwrap();
        let discord_drain = shutdown
            .find("foundation.gateway.begin_discord_drain_v1()")
            .unwrap();
        assert!(sealed < first_await);
        assert!(sealed < foundation && foundation < discord_drain);
        assert!(!shutdown.contains("lifecycle.observe_registry_route_set_v2()"));
        assert!(!shutdown.contains("foundation.observe_shutdown_registry_v1()"));
    }

    for required in [
        "RuntimeClosedRecoveryShuttingDownServingOpenProcessV2",
        "let worker = match worker.begin_shutdown(generation, RuntimeShutdownCauseV2::Explicit)",
        "let worker = worker.begin_shutdown(RuntimeShutdownCauseV2::Explicit);",
        "let registry_observation = registry.observe_shutdown_route_set_v2();",
    ] {
        assert!(include_str!("../src/closed_recovery.rs").contains(required));
    }

    for marker in [
        "pub(super) async fn shutdown_empty_open_process_v2(",
        "pub(super) async fn shutdown_refreshing_empty_open_process_v2(",
    ] {
        let shutdown = braced_declaration(observation, marker);
        assert!(shutdown.contains("foundation.observe_shutdown_registry_v1()"));
        assert!(!shutdown.contains("observe_shutdown_serving_registry_v2"));
    }

    let lost = braced_declaration(
        observation,
        "pub(super) async fn shutdown_process_without_lifecycle_v2(",
    );
    assert!(lost.contains("RuntimeLifecycleLostRegistryObservationV2::Empty"));
    assert!(lost.contains("foundation.observe_shutdown_registry_v1()"));
    assert!(lost.contains("RuntimeLifecycleLostRegistryObservationV2::Serving"));
    assert!(lost.contains("foundation.observe_shutdown_serving_registry_without_lifecycle_v2()"));
    assert!(serving_process.contains("RuntimeLifecycleLostRegistryObservationV2::Serving,"));
}

#[test]
fn committed_closed_recovery_process_is_linear_retained_and_non_serving() {
    let sources = source_files();
    let process = source_before_test_module(include_str!("../src/process/closed.rs"));
    let closed = include_str!("../src/closed_recovery.rs");
    let owner = include_str!("../src/gateway_owner_startup_watchdog.rs");
    let gateway = source_before_test_module(include_str!("../src/gateway.rs"));
    let startup = source_before_test_module(include_str!("../src/process_startup.rs"));
    let library = include_str!("../src/lib.rs");

    for required in [
        "pub(crate) struct RuntimeClosedRecoveryProcessV2",
        "discord: RuntimeDiscordGatewaySupervisorV1,",
        "foundation: RuntimeProcessFoundationV1,",
        "session: RuntimeClosedRecoverySessionV2,",
        "RuntimeClosedRecoveryProcessV2(<redacted>)",
        "pub(crate) async fn into_closed_recovery_v2(",
        "require_current_recovery_pending_v2(",
        "pending.commit_cutoff_v2()",
        "pending.commit_owner_in_place_v2()",
        "await_closed_recovery_commit_v2(",
        "pending.try_into_committed_session_v2()",
        "let session_validation = session",
        "shutdown_pending_commit_v2(",
        "shutdown_committed_recovery_v2(",
        "RuntimeProcessClosedRecoveryTransitionErrorV2(<redacted>)",
        "RuntimeClosedRecoveryProcessShutdownErrorV2(<redacted>)",
    ] {
        assert!(process.contains(required), "{required}");
    }
    let transition = braced_declaration(process, "pub(crate) async fn into_closed_recovery_v2(");
    let preflight = transition
        .find("require_current_recovery_pending_v2(")
        .unwrap();
    let cutoff = transition.find("pending.commit_cutoff_v2()").unwrap();
    let commit = transition
        .find("pending.commit_owner_in_place_v2()")
        .unwrap();
    let race = transition.find("await_closed_recovery_commit_v2(").unwrap();
    let conversion = transition
        .find("pending.try_into_committed_session_v2()")
        .unwrap();
    let revalidate = transition[conversion..]
        .find(".revalidate_v2()")
        .map(|offset| conversion + offset)
        .unwrap();
    let final_discord = transition[revalidate..]
        .find("discord_transition_failure_v1(&discord)")
        .map(|offset| revalidate + offset)
        .unwrap();
    let final_budget = transition[final_discord..]
        .find("foundation.startup_budget.operation_is_open()")
        .map(|offset| final_discord + offset)
        .unwrap();
    let validation_consumed = transition[final_budget..]
        .find("session_validation.err()")
        .map(|offset| final_budget + offset)
        .unwrap();
    let failed_session_cleanup = transition[validation_consumed..]
        .find("shutdown_committed_recovery_v2(")
        .map(|offset| validation_consumed + offset)
        .unwrap();
    let construct = transition
        .find("Ok(RuntimeClosedRecoveryProcessV2")
        .unwrap();
    assert!(
        preflight < cutoff
            && cutoff < commit
            && commit < race
            && race < conversion
            && conversion < revalidate
            && revalidate < final_discord
            && final_discord < final_budget
            && final_budget < validation_consumed
            && validation_consumed < failed_session_cleanup
            && failed_session_cleanup < construct
    );
    assert_eq!(
        transition
            .matches("pending.commit_owner_in_place_v2()")
            .count(),
        1
    );
    for operation in [
        "discord_transition_failure_v1(&discord)",
        "foundation.startup_budget.operation_is_open()",
        "session_validation.err()",
        "shutdown_committed_recovery_v2(",
    ] {
        assert_eq!(transition.matches(operation).count(), 1, "{operation}");
    }
    assert_eq!(transition.matches(".revalidate_v2()").count(), 1);
    let pending_preflight = braced_declaration(process, "fn require_current_recovery_pending_v2(");
    let initial_budget = pending_preflight
        .find("foundation.startup_budget.operation_is_open()")
        .unwrap();
    let initial_discord = pending_preflight
        .find("discord_transition_failure_v1(discord)")
        .unwrap();
    let pending_validation = pending_preflight
        .find("pending\n        .revalidate_v2()")
        .unwrap();
    let final_discord = pending_preflight
        .rfind("discord_transition_failure_v1(discord)")
        .unwrap();
    let final_budget = pending_preflight
        .rfind("foundation.startup_budget.operation_is_open()")
        .unwrap();
    assert!(
        initial_budget < initial_discord
            && initial_discord < pending_validation
            && pending_validation < final_discord
            && final_discord < final_budget
    );
    assert_eq!(
        pending_preflight
            .matches("foundation.startup_budget.operation_is_open()")
            .count(),
        2
    );
    assert_eq!(
        pending_preflight
            .matches("discord_transition_failure_v1(discord)")
            .count(),
        2
    );
    assert_eq!(pending_preflight.matches(".revalidate_v2()").count(), 1);
    assert_eq!(
        transition
            .matches("pending.try_into_committed_session_v2()")
            .count(),
        1
    );
    let race_helper = braced_declaration(process, "async fn await_closed_recovery_commit_v2<");
    let cutoff_branch = race_helper.find("Instant::now() >= commit_cutoff").unwrap();
    let biased = race_helper.find("biased;").unwrap();
    let shutdown = race_helper.find("observation = &mut shutdown").unwrap();
    let deadline = race_helper.find("sleep_until(").unwrap();
    let discord = race_helper
        .find("transition = &mut discord_terminal")
        .unwrap();
    let commit_result = race_helper.find("result = &mut commit").unwrap();
    assert!(
        cutoff_branch < biased && biased < shutdown && shutdown < deadline && deadline < discord
    );
    assert!(discord < commit_result);
    for shutdown_name in [
        "async fn shutdown_pending_commit_v2(",
        "async fn shutdown_committed_recovery_v2(",
    ] {
        let shutdown = braced_declaration(process, shutdown_name);
        assert!(
            shutdown.contains("shutdown_paused_foundation_owner_v1("),
            "{shutdown_name}"
        );
        assert!(
            shutdown.contains(".abort_and_shutdown_until_v2(deadline)"),
            "{shutdown_name}"
        );
        assert!(!shutdown.contains("foundation.shutdown().await"));
    }
    let state_attributes = declaration_attribute_block(process, "RuntimeClosedRecoveryProcessV2");
    for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
        assert!(
            !contains_identifier(state_attributes, forbidden),
            "{forbidden}"
        );
        assert!(!implements_trait(
            process,
            "RuntimeClosedRecoveryProcessV2",
            forbidden,
        ));
    }
    for required in [
        "committed_closed_recovery_observation:",
        "commit_closed_recovery_in_place_v2(",
        "try_into_committed_closed_recovery_v2(",
        "pub(crate) async fn abort_and_shutdown_until_v2(",
    ] {
        assert!(owner.contains(required), "{required}");
    }
    for required in [
        "pub(crate) fn commit_cutoff_v2(",
        "pub(crate) async fn commit_owner_in_place_v2(",
        "pub(crate) fn try_into_committed_session_v2(",
        "pub(crate) async fn abort_and_shutdown_until_v2(",
    ] {
        assert!(closed.contains(required), "{required}");
    }
    assert!(closed.contains(concat!(
        "#[cfg(test)]\n",
        "    pub(crate) async fn commit_owner_v2("
    )));
    assert!(owner.contains(concat!(
        "#[cfg(test)]\n",
        "    pub(crate) async fn commit_closed_recovery_v2("
    )));
    assert!(gateway.contains("commit_prepared_owner_in_place_v2("));
    assert!(gateway.contains("&mut RuntimeGatewayOwnerPreparedClosedRecoveryV2"));
    for (path, source) in sources.iter().filter(|(path, _)| path.starts_with("src")) {
        for method in ["commit_owner_in_place_v2", "try_into_committed_session_v2"] {
            if contains_identifier(source, method) {
                assert!(
                    path == Path::new("src/closed_recovery.rs")
                        || path == Path::new("src/process/closed.rs")
                        || path
                            == Path::new("src/gateway_owner_startup_watchdog_handoff_tests.rs",),
                    "{}: {method}",
                    path.display()
                );
            }
        }
        for method in [
            "commit_closed_recovery_in_place_v2",
            "try_into_committed_closed_recovery_v2",
        ] {
            if contains_identifier(source, method) {
                assert!(
                    path == Path::new("src/gateway.rs")
                        || path == Path::new("src/closed_recovery.rs")
                        || path == Path::new("src/gateway_owner_startup_watchdog.rs")
                        || path
                            == Path::new("src/gateway_owner_startup_watchdog_handoff_tests.rs",),
                    "{}: {method}",
                    path.display()
                );
            }
        }
        if contains_identifier(source, "commit_prepared_owner_in_place_v2") {
            assert!(
                path == Path::new("src/gateway.rs") || path == Path::new("src/closed_recovery.rs"),
                "{}: commit_prepared_owner_in_place_v2",
                path.display()
            );
        }
    }
    for forbidden in [
        "refresh_iteration_readiness_in_place_v2",
        "observe_startup_recovery_v2",
        "try_into_ready_iteration_v2",
        "begin_startup_recovery_observation_v2",
        "resume_admission",
        "issue_ready_lease",
        "ready_to_serve",
        "health_ready",
        "serving_lease",
        "heartbeat",
        "dispatch",
        "execute",
        "hydrate",
        "reconcile",
        "activate",
        "deploy",
        "into_production",
    ] {
        assert!(!contains_identifier(process, forbidden), "{forbidden}");
    }
    for forbidden in [
        "observe_startup_recovery_v2",
        "begin_startup_recovery_observation_v2",
        "resume_admission",
        "issue_ready_lease",
        "ready_to_serve",
        "health_ready",
        "serving_lease",
        "heartbeat",
        "dispatch",
        "execute",
        "hydrate",
        "reconcile",
        "activate",
        "deploy",
        "into_production",
    ] {
        assert!(
            !contains_identifier(startup, forbidden),
            "startup: {forbidden}"
        );
    }
    let startup_stage = braced_declaration(
        startup,
        "async fn stage_runtime_process_from_environment_v1(",
    );
    let pending = startup_stage.find(".into_recovery_pending_v2()").unwrap();
    let committed = startup_stage.find(".into_closed_recovery_v2()").unwrap();
    let ready = startup_stage
        .find(".into_recovery_iteration_ready_v2()")
        .unwrap();
    let fixed_point = startup_stage
        .find(".into_startup_recovery_fixed_point_v2()")
        .unwrap();
    let paused_production = startup_stage
        .find(".into_paused_production_handoff_v2()")
        .unwrap();
    let process_bound = startup_stage
        .find(".into_process_bound_handoff_v2()")
        .unwrap();
    let recovery_resume = startup_stage.find(".into_recovery_resume_v2()").unwrap();
    let admission = startup_stage.find(".resume_recovery_v2()").unwrap();
    let empty_open = startup_stage.find(".enter_empty_open_v2()").unwrap();
    let serving_open = startup_stage.find(".enter_serving_open_v2()").unwrap();
    let run = startup_stage.find(".run_until_shutdown_v2()").unwrap();
    assert!(
        pending < committed
            && committed < ready
            && ready < fixed_point
            && fixed_point < paused_production
            && paused_production < process_bound
            && process_bound < recovery_resume
            && recovery_resume < admission
            && admission < empty_open
            && empty_open < serving_open
            && serving_open < run
    );
    assert!(!startup_stage.contains("empty_open\n        .shutdown()"));
    assert!(!library.contains("RuntimeClosedRecoveryProcessV2"));
    assert!(!library.contains("RuntimeClosedRecoverySessionV2"));
    assert!(!library.contains("RuntimeGatewayOwnerClosedRecoverySupervisorV2"));
}

#[test]
fn recovery_readiness_process_is_single_use_cancellation_safe_and_non_authorizing() {
    let sources = source_files();
    let process = source_before_test_module(include_str!("../src/process/readiness.rs"));
    let closed_process = source_before_test_module(include_str!("../src/process/closed.rs"));
    let closed = include_str!("../src/closed_recovery.rs");
    let owner_supervisor = include_str!("../src/gateway_owner_startup_watchdog.rs");
    let startup = source_before_test_module(include_str!("../src/process_startup.rs"));
    let library = include_str!("../src/lib.rs");

    for required in [
        "pub enum RuntimeProcessRecoveryReadinessFailureV2",
        "pub enum RuntimeProcessRecoveryReadinessTransitionFailureV2",
        "pub enum RuntimeProcessRecoveryReadinessTransitionErrorV2",
        "RuntimeProcessRecoveryReadinessTransitionErrorV2(<redacted>)",
        "pub(crate) struct RuntimeRecoveryIterationReadyProcessV2",
        "discord: RuntimeDiscordGatewaySupervisorV1,",
        "foundation: RuntimeProcessFoundationV1,",
        "iteration: RuntimeClosedRecoveryReadyIterationV2,",
        "pub(crate) async fn into_recovery_iteration_ready_v2(",
        "require_current_closed_recovery_v2(",
        "session.readiness_cutoff_v2()",
        "session.owner_safety_deadline_v2()",
        "session.owner_terminal_observation_v2()",
        "session.refresh_iteration_readiness_in_place_v2(&foundation.databases)",
        "await_recovery_readiness_refresh_v2(",
        "session.try_into_ready_iteration_v2()",
        "let iteration_validation = iteration",
        "iteration.owner_terminal_status_v2()",
        "iteration.owner_safety_deadline_v2()",
        "shutdown_committed_recovery_v2(",
        "shutdown_ready_recovery_v2(",
        "RuntimeRecoveryIterationReadyProcessV2(<redacted>)",
    ] {
        assert!(process.contains(required), "{required}");
    }
    assert!(process.contains(concat!(
        "pub(crate) struct RuntimeRecoveryIterationReadyProcessV2 {\n",
        "    pub(super) discord: RuntimeDiscordGatewaySupervisorV1,\n",
        "    pub(super) foundation: RuntimeProcessFoundationV1,\n",
        "    pub(super) iteration: RuntimeClosedRecoveryReadyIterationV2,\n",
        "}"
    )));
    assert!(closed_process.contains(concat!(
        "pub(crate) struct RuntimeClosedRecoveryProcessV2 {\n",
        "    pub(super) discord: RuntimeDiscordGatewaySupervisorV1,\n",
        "    pub(super) foundation: RuntimeProcessFoundationV1,\n",
        "    pub(super) session: RuntimeClosedRecoverySessionV2,\n",
        "}"
    )));
    let attributes = declaration_attribute_block(process, "RuntimeRecoveryIterationReadyProcessV2");
    for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
        assert!(!contains_identifier(attributes, forbidden), "{forbidden}");
        assert!(!implements_trait(
            process,
            "RuntimeRecoveryIterationReadyProcessV2",
            forbidden,
        ));
    }

    let transition = braced_declaration(
        process,
        "pub(crate) async fn into_recovery_iteration_ready_v2(",
    );
    let preflight = transition
        .find("require_current_closed_recovery_v2(")
        .unwrap();
    let cutoff = transition.find("session.readiness_cutoff_v2()").unwrap();
    let owner_deadline = transition
        .find("session.owner_safety_deadline_v2()")
        .unwrap();
    let owner_terminal = transition
        .find("session.owner_terminal_observation_v2()")
        .unwrap();
    let refresh = transition
        .find("session.refresh_iteration_readiness_in_place_v2(&foundation.databases)")
        .unwrap();
    let race = transition
        .find("await_recovery_readiness_refresh_v2(")
        .unwrap();
    let failed_cleanup = transition[race..]
        .find("shutdown_committed_recovery_v2(")
        .map(|offset| race + offset)
        .unwrap();
    let conversion = transition
        .find("session.try_into_ready_iteration_v2()")
        .unwrap();
    let retained_conversion_cleanup = transition[conversion..]
        .find("shutdown_committed_recovery_v2(")
        .map(|offset| conversion + offset)
        .unwrap();
    let final_revalidation = transition
        .find("iteration\n            .revalidate_v2()")
        .unwrap();
    let final_discord = transition[final_revalidation..]
        .find("discord_transition_failure_v1(&discord)")
        .map(|offset| final_revalidation + offset)
        .unwrap();
    let final_owner = transition[final_discord..]
        .find("iteration.owner_terminal_status_v2()")
        .map(|offset| final_discord + offset)
        .unwrap();
    let final_budget = transition[final_owner..]
        .find("foundation.startup_budget.operation_is_open()")
        .map(|offset| final_owner + offset)
        .unwrap();
    let ready_cleanup = transition[final_budget..]
        .find("shutdown_ready_recovery_v2(")
        .map(|offset| final_budget + offset)
        .unwrap();
    let construct = transition
        .find("Ok(RuntimeRecoveryIterationReadyProcessV2")
        .unwrap();
    assert!(
        preflight < cutoff
            && cutoff < owner_deadline
            && owner_deadline < owner_terminal
            && owner_terminal < refresh
            && refresh < race
            && race < failed_cleanup
            && failed_cleanup < conversion
            && conversion < retained_conversion_cleanup
            && retained_conversion_cleanup < final_revalidation
            && final_revalidation < final_discord
            && final_discord < final_owner
            && final_owner < final_budget
            && final_budget < ready_cleanup
            && ready_cleanup < construct
    );
    for operation in [
        "session.readiness_cutoff_v2()",
        "session.owner_terminal_observation_v2()",
        "session.refresh_iteration_readiness_in_place_v2(&foundation.databases)",
        "session.try_into_ready_iteration_v2()",
    ] {
        assert_eq!(transition.matches(operation).count(), 1, "{operation}");
    }

    let preflight = braced_declaration(process, "fn require_current_closed_recovery_v2(");
    let initial_budget = preflight
        .find("foundation.startup_budget.operation_is_open()")
        .unwrap();
    let initial_discord = preflight
        .find("discord_transition_failure_v1(discord)")
        .unwrap();
    let initial_owner = preflight
        .find("session.owner_terminal_status_v2()")
        .unwrap();
    let compound = preflight.find("session\n        .revalidate_v2()").unwrap();
    let final_owner = preflight
        .rfind("session.owner_terminal_status_v2()")
        .unwrap();
    let final_discord = preflight
        .rfind("discord_transition_failure_v1(discord)")
        .unwrap();
    let final_budget = preflight
        .rfind("foundation.startup_budget.operation_is_open()")
        .unwrap();
    assert!(
        initial_budget < initial_discord
            && initial_discord < initial_owner
            && initial_owner < compound
            && compound < final_owner
            && final_owner < final_discord
            && final_discord < final_budget
    );
    assert_eq!(
        preflight
            .matches("foundation.startup_budget.operation_is_open()")
            .count(),
        2
    );
    assert_eq!(
        preflight
            .matches("discord_transition_failure_v1(discord)")
            .count(),
        2
    );
    assert_eq!(
        preflight
            .matches("session.owner_terminal_status_v2()")
            .count(),
        2
    );
    assert_eq!(preflight.matches(".revalidate_v2()").count(), 1);

    let race = braced_declaration(process, "async fn await_recovery_readiness_refresh_v2<");
    let elapsed = race.find("Instant::now() >= readiness_cutoff").unwrap();
    let biased = race.find("biased;").unwrap();
    let shutdown = race.find("observation = &mut shutdown").unwrap();
    let deadline = race.find("sleep_until(").unwrap();
    let discord = race.find("transition = &mut discord_terminal").unwrap();
    let owner = race.find("() = &mut owner_terminal").unwrap();
    let refresh = race.find("result = &mut refresh").unwrap();
    assert!(
        elapsed < biased
            && biased < deadline
            && biased < shutdown
            && shutdown < deadline
            && deadline < discord
            && discord < owner
            && owner < refresh
    );
    assert_eq!(race.matches("tokio::select!").count(), 1);

    let cleanup = braced_declaration(process, "async fn shutdown_ready_recovery_v2(");
    assert!(cleanup.contains("shutdown_paused_foundation_owner_v1("));
    assert!(cleanup.contains(".abort_and_shutdown_until_v2(deadline)"));
    assert!(!cleanup.contains("foundation.shutdown().await"));
    let ready_impl = braced_declaration(closed, "impl RuntimeClosedRecoveryReadyIterationV2");
    let ready_abort = braced_declaration(
        ready_impl,
        "pub(crate) async fn abort_and_shutdown_until_v2(",
    );
    let authority_drop = ready_abort
        .find("drop((gateway, registry, operation_cutoff, iteration))")
        .unwrap();
    let owner_abort = ready_abort
        .find("owner.abort_and_shutdown_until_v2(cleanup_deadline).await")
        .unwrap();
    assert!(authority_drop < owner_abort);

    let owner_impl = braced_declaration(
        owner_supervisor,
        "impl RuntimeGatewayOwnerClosedRecoverySupervisorV2",
    );
    let terminal_observation =
        braced_declaration(owner_impl, "pub(crate) fn terminal_observation_v2(");
    let clone = terminal_observation
        .find("let mut terminal = self.inner().terminal.clone()")
        .unwrap();
    let immediate = terminal_observation
        .find("if let Some(exit) = *terminal.borrow()")
        .unwrap();
    let changed = terminal_observation
        .find("terminal.changed().await")
        .unwrap();
    assert!(clone < immediate && immediate < changed);
    assert_eq!(terminal_observation.matches(".await").count(), 1);
    for forbidden in ["spawn", "send", "renew", "acquire", "release", "invalidate"] {
        assert!(
            !contains_identifier(terminal_observation, forbidden),
            "terminal observation: {forbidden}"
        );
    }

    for forbidden in [
        "observe_startup_recovery_v2",
        "observe_startup_recovery_with_v2",
        "observe_startup_recovery",
        "begin_startup_recovery_observation_v2",
        "into_startup_recovery_observation_successor_v2",
        "complete_startup_recovery_observation",
        "validate_startup_recovery_fixed_point_v2",
        "RuntimeStartupRecoveryContinuationV2",
        "RuntimeStartupRecoveryFixedPointProofV2",
        "RuntimeStartupRecoveryObservationPortV2",
        "verify_readiness_v1",
        "initial_readiness",
        "refresh_recovery_readiness",
        "recover_next_stale_live",
        "resume_admission",
        "issue_ready_lease",
        "ready_to_serve",
        "health_ready",
        "serving_lease",
        "heartbeat",
        "dispatch",
        "execute",
        "hydrate",
        "reconcile",
        "activate",
        "deploy",
        "into_production",
        "retry",
    ] {
        assert!(!contains_identifier(process, forbidden), "{forbidden}");
    }
    assert!(!process.contains("loop {"));
    assert!(!process.contains(".resume"));
    assert!(!process.contains("resume("));
    assert!(!process.contains("tokio::spawn"));
    assert!(!process.contains("spawn_blocking"));
    assert_eq!(
        startup
            .matches(".into_recovery_iteration_ready_v2()")
            .count(),
        1
    );
    assert!(
        startup.find(".into_closed_recovery_v2()").unwrap()
            < startup.find(".into_recovery_iteration_ready_v2()").unwrap()
    );
    for forbidden in [
        "observe_startup_recovery_v2",
        "begin_startup_recovery_observation_v2",
        "RuntimeStartupRecoveryObservationPortV2",
    ] {
        assert!(
            !contains_identifier(startup, forbidden),
            "startup: {forbidden}"
        );
    }

    for (path, source) in sources.iter().filter(|(path, _)| path.starts_with("src")) {
        for method in [
            "refresh_iteration_readiness_in_place_v2",
            "try_into_ready_iteration_v2",
        ] {
            if contains_identifier(source, method) {
                assert!(
                    path == Path::new("src/closed_recovery.rs")
                        || path == Path::new("src/process/readiness.rs")
                        || path == Path::new("src/gateway_owner_startup_watchdog_handoff_tests.rs"),
                    "{}: {method}",
                    path.display()
                );
            }
        }
        if contains_identifier(source, "shutdown_committed_recovery_v2") {
            assert!(
                path == Path::new("src/process/closed.rs")
                    || path == Path::new("src/process/readiness.rs"),
                "{}: shutdown_committed_recovery_v2",
                path.display()
            );
        }
    }
    for required in [
        "RuntimeProcessRecoveryReadinessFailureV2",
        "RuntimeProcessRecoveryReadinessTransitionFailureV2",
        "RuntimeProcessRecoveryReadinessTransitionErrorV2",
    ] {
        assert_eq!(library.matches(required).count(), 1, "{required}");
    }
    assert!(!library.contains("RuntimeRecoveryIterationReadyProcessV2"));
    assert!(closed.contains("readiness: RuntimeClosedRecoveryReadinessStateV2,"));
}

#[test]
fn gateway_section_snapshot_guards_never_reborrow_a_live_watch_reference() {
    let gateway = include_str!("../src/gateway.rs");
    let production = source_before_test_module(gateway);
    let emergency = braced_declaration(production, "impl<'a> RuntimeEmergencyGatewaySectionV2<'a>");
    let acquire = braced_declaration(emergency, "fn acquire(");
    assert!(acquire.contains("expected_paused_gateway: &RuntimePausedGatewayObservationV2,"));
    assert!(acquire.contains("if paused_gateway != *expected_paused_gateway"));
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
        "pub(crate) async fn commit_prepared_owner_in_place_v2(",
    );
    assert!(owner_commit.contains("commit_cutoff: Instant"));
    let preflight = owner_commit
        .find(".pending_section_v2(prepared_owner)")
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
        .find("prepared_owner.commit_closed_recovery_in_place_v2(self.permit_v2())")
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
            .matches("prepared_owner.commit_closed_recovery_in_place_v2(self.permit_v2())")
            .count(),
        1
    );
    let readiness = braced_declaration(
        pending_binding,
        "pub(crate) fn refresh_readiness_in_place_v2(",
    );
    assert!(readiness.contains("&mut self,"));
    let readiness_preflight = readiness
        .find(".committed_pending_section_v2(committed_owner)")
        .unwrap();
    let readiness_preflight_drop = readiness.find("drop(section)").unwrap();
    let readiness_transition = readiness
        .find(".refresh_recovery_readiness(permit, readiness)")
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
    let permit = exact_registry.find(".permit_v2()").unwrap();
    let evidence = exact_registry.find(".registry_evidence()").unwrap();
    let empty = exact_registry.find(".empty_observation()").unwrap();
    let evidence_match = exact_registry.find("!= observation").unwrap();
    let final_revalidation = exact_registry.find("self.require_current_v2()").unwrap();
    assert!(
        permit < evidence
            && evidence < empty
            && empty < evidence_match
            && evidence_match < final_revalidation
    );
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
            .matches(".validate_recovery_permit(self.binding.permit_v2())")
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
        "pub(crate) fn begin_initial_empty_recovery_retained_v2(",
        "pub(crate) async fn abort_and_shutdown_until_v2(",
        "pub(crate) struct RuntimeClosedRecoveryPendingPhaseV2",
        "RuntimeClosedRecoveryPendingPhaseV2(<redacted>)",
        "pub(crate) struct RuntimeClosedRecoverySessionV2",
        "enum RuntimeClosedRecoverySessionRegistryV2",
        "RuntimeClosedRecoverySessionRegistryV2::Empty",
        "RuntimeClosedRecoverySessionRegistryV2::PendingDrainSealed",
        "RuntimeClosedRecoverySessionRegistryV2::PendingDrainSuccessionSealed",
        "RuntimeClosedRecoverySessionRegistryV2::Failed",
        "pub(crate) fn seal_pending_drain_succession_candidate_v3(",
        "candidate: &RuntimePendingDrainPreviousOwnerClaimedCandidateV3",
        "pub(crate) fn unseal_pending_drain_after_durable_succession_v3(",
        "durable: &RuntimeDurablyAcknowledgedPendingDrainSuccessionV3",
        "fn revalidate_committed_pending_drain_succession_sealed_v3(",
        "RuntimeClosedRecoverySessionV2(<redacted>)",
        "pub(crate) struct RuntimeClosedRecoveryReadyIterationV2",
        "RuntimeClosedRecoveryReadyIterationV2(<redacted>)",
        "pub(crate) struct RuntimeClosedRecoveryFixedPointV2",
        "RuntimeClosedRecoveryFixedPointV2(<redacted>)",
        "pub(crate) enum RuntimeClosedRecoveryStartupIterationOutcomeV2",
        "RuntimeClosedRecoveryStartupIterationOutcomeV2(<redacted>)",
        "pub(crate) async fn commit_owner_v2(",
        "async fn commit_owner_with_post_commit_v2(",
        "pub(crate) fn commit_cutoff_v2(",
        "pub(crate) async fn commit_owner_in_place_v2(",
        "pub(crate) fn try_into_committed_session_v2(",
        "enum RuntimeClosedRecoveryReadinessStateV2",
        "RuntimeClosedRecoveryReadinessStateV2::Available",
        "RuntimeClosedRecoveryReadinessStateV2::Failed",
        "RuntimeClosedRecoveryReadinessStateV2::Ready(iteration)",
        "pub(crate) async fn refresh_iteration_readiness_in_place_v2(",
        "async fn refresh_iteration_readiness_in_place_with_v2<Verify, Verification, PostRefresh>(",
        "pub(crate) fn try_into_ready_iteration_v2(",
        ".verify_readiness_refresh_until_v2(cutoff)",
        "operation_cutoff: Instant",
        ".operation_cutoff",
        ".min(self.owner.observation().safety_deadline())",
        "Instant::now() >= verification_cutoff",
        ".invalidate_capability_not_ready_v2()",
        ".invalidate_protocol_violation_v2()",
        ".refresh_readiness_in_place_v2(",
        ".commit_prepared_owner_in_place_v2(&authority, &mut self.owner, commit_cutoff)",
        "post_commit();",
        ".committed_pending_section_v2(owner)",
        "pub(crate) struct RuntimeClosedRecoveryTransitionAuthorityV2",
        "RuntimeClosedRecoveryTransitionAuthorityV2(<redacted>)",
        "let authority = RuntimeClosedRecoveryTransitionAuthorityV2 { _private: () };",
        ".initial_emergency_gateway_section_v2(owner, expected_paused_gateway)",
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
        "    registry: RuntimeClosedRecoverySessionRegistryV2,\n",
        "    operation_cutoff: Instant,\n",
        "    readiness: RuntimeClosedRecoveryReadinessStateV2,\n",
        "}"
    )));
    assert!(production.contains(concat!(
        "pub(crate) struct RuntimeClosedRecoveryReadyIterationV2 {\n",
        "    owner: RuntimeGatewayOwnerClosedRecoverySupervisorV2,\n",
        "    gateway: RuntimeRecoveryPendingGatewayBindingV2,\n",
        "    registry: RuntimeRegistryEmptyRecoveryBindingV2,\n",
        "    operation_cutoff: Instant,\n",
        "    iteration: Option<RuntimeAuthorizedStartupRecoveryIterationV2>,\n",
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
        .find(".initial_emergency_gateway_section_v2(owner, expected_paused_gateway)")
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
    let owner_commit = braced_declaration(
        pending_phase,
        "pub(crate) async fn commit_owner_in_place_v2(",
    );
    let precommit = owner_commit.find("self.revalidate_v2()").unwrap();
    let cutoff = owner_commit.find("self.commit_cutoff_v2()").unwrap();
    let cutoff_guard = owner_commit
        .find("if Instant::now() >= commit_cutoff")
        .unwrap();
    let commit = owner_commit
        .find(".commit_prepared_owner_in_place_v2(&authority, &mut self.owner, commit_cutoff)")
        .unwrap();
    assert!(precommit < cutoff && cutoff < cutoff_guard && cutoff_guard < commit);
    let wrapper = braced_declaration(pending_phase, "async fn commit_owner_with_post_commit_v2(");
    let wrapper_commit = wrapper
        .find("self.commit_owner_in_place_v2().await?")
        .unwrap();
    let hook = wrapper.find("post_commit();").unwrap();
    let conversion = wrapper.find(".try_into_committed_session_v2()").unwrap();
    let postcommit = wrapper.find("session.revalidate_v2()?").unwrap();
    assert!(wrapper_commit < hook && hook < conversion && conversion < postcommit);
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
    let v2_seal = braced_declaration(
        committed_session,
        "pub(crate) fn seal_pending_drain_candidate_v2(",
    );
    assert!(v2_seal
        .contains("RuntimeClosedRecoverySessionRegistryV2::PendingDrainSealed(Box::new(sealed))"));
    assert!(!v2_seal.contains("PendingDrainSuccessionSealed"));
    let v3_seal = braced_declaration(
        committed_session,
        "pub(crate) fn seal_pending_drain_succession_candidate_v3(",
    );
    assert!(v3_seal.contains(".into_pending_drain_succession_seal_binding_v3(candidate)"));
    assert!(v3_seal.contains(
        "RuntimeClosedRecoverySessionRegistryV2::PendingDrainSuccessionSealed(Box::new(sealed))"
    ));
    assert!(!v3_seal.contains("RuntimeClosedRecoverySessionRegistryV2::PendingDrainSealed("));
    let v2_unseal = braced_declaration(
        committed_session,
        "pub(crate) fn unseal_pending_drain_after_durable_ack_v2(",
    );
    assert!(
        v2_unseal.contains("RuntimeClosedRecoverySessionRegistryV2::PendingDrainSealed(registry)")
    );
    assert!(!v2_unseal.contains("PendingDrainSuccessionSealed"));
    let v3_unseal = braced_declaration(
        committed_session,
        "pub(crate) fn unseal_pending_drain_after_durable_succession_v3(",
    );
    assert!(v3_unseal.contains(
        "RuntimeClosedRecoverySessionRegistryV2::PendingDrainSuccessionSealed(registry)"
    ));
    assert!(v3_unseal.contains(".into_empty_binding_after_durable_succession_v3(durable)"));
    assert!(!v3_unseal.contains("RuntimeClosedRecoverySessionRegistryV2::PendingDrainSealed("));
    let readiness_refresh = braced_declaration(
        committed_session,
        "async fn refresh_iteration_readiness_in_place_with_v2<Verify, Verification, PostRefresh>(",
    );
    let refresh_consume = readiness_refresh.find("std::mem::replace(").unwrap();
    let refresh_failed = readiness_refresh
        .find("RuntimeClosedRecoveryReadinessStateV2::Failed")
        .unwrap();
    let refresh_available = readiness_refresh
        .find("RuntimeClosedRecoveryReadinessStateV2::Available")
        .unwrap();
    let refresh_protocol = readiness_refresh
        .find("self.gateway.invalidate_protocol_violation_v2()")
        .unwrap();
    let refresh_prevalidation = readiness_refresh.find("self.revalidate_v2()").unwrap();
    let refresh_cutoff = readiness_refresh
        .find("self.readiness_cutoff_v2()")
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
        .find(".refresh_readiness_in_place_v2(")
        .unwrap();
    let refresh_hook = readiness_refresh.find("post_refresh();").unwrap();
    let refresh_final_validation = readiness_refresh[refresh_hook..]
        .find("self.revalidate_v2()")
        .map(|offset| refresh_hook + offset)
        .unwrap();
    let refresh_ready = readiness_refresh
        .find("self.readiness = RuntimeClosedRecoveryReadinessStateV2::Ready(iteration)")
        .unwrap();
    assert!(
        refresh_consume < refresh_failed
            && refresh_failed < refresh_available
            && refresh_available < refresh_protocol
            && refresh_protocol < refresh_prevalidation
            && refresh_prevalidation < refresh_cutoff
            && refresh_cutoff < refresh_pre_deadline
            && refresh_pre_deadline < refresh_await
            && refresh_await < refresh_post_deadline
            && refresh_post_deadline < refresh_postvalidation
            && refresh_postvalidation < refresh_successor
            && refresh_successor < refresh_hook
            && refresh_hook < refresh_final_validation
            && refresh_final_validation < refresh_ready
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
        "pub(crate) async fn refresh_iteration_readiness_in_place_v2(",
    );
    assert!(!public_refresh.contains("operation_cutoff:"));
    assert!(public_refresh.contains("&mut self,"));
    let conversion = braced_declaration(
        committed_session,
        "pub(crate) fn try_into_ready_iteration_v2(",
    );
    let ready_match = conversion
        .find("RuntimeClosedRecoveryReadinessStateV2::Ready(iteration)")
        .unwrap();
    let ready_construct = conversion
        .find("Ok(RuntimeClosedRecoveryReadyIterationV2")
        .unwrap();
    let retained_failure = conversion.find("readiness => Err(Box::new(Self").unwrap();
    assert!(ready_match < ready_construct && ready_construct < retained_failure);
    let begin = braced_declaration(
        production,
        "pub(crate) fn begin_initial_empty_recovery_retained_v2(",
    );
    assert!(!begin.contains(".await"));
    let begin_deadline = begin.find("Instant::now() >= operation_cutoff").unwrap();
    let begin_bindings = begin.find("bind_initial_empty_recovery_v2(").unwrap();
    assert!(begin_deadline < begin_bindings);
    let binding = braced_declaration(production, "fn bind_initial_empty_recovery_v2(");
    let begin_gateway = binding
        .find(".initial_emergency_gateway_section_v2(owner, expected_paused_gateway)")
        .unwrap();
    let begin_registry = binding
        .find(".recovery_observation_guard_v2(&authority, &gateway_section)")
        .unwrap();
    assert!(begin_gateway < begin_registry);
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
    let recovery_resume = braced_declaration(
        production,
        "pub(crate) async fn into_admission_acknowledging_v2(",
    );
    assert!(recovery_resume.contains("worker.resume_recovery(&port)"));
    assert_eq!(
        production.matches("worker.resume_recovery(&port)").count(),
        1
    );
    assert!(!production.contains(".resume_admission"));
    assert!(!production.contains(".resume_reserved_admission"));
    assert!(!production.contains(".resume("));
    assert!(!production.contains("resume("));
    let resumed_ready = braced_declaration(
        production,
        "pub(crate) fn observe_exact_resumed_ready_attestation_v2(",
    );
    assert!(
        resumed_ready.contains(".observe_exact_recovery_resume_successor_ready_attestation_v2(")
    );
    assert!(!resumed_ready.contains(".recovery_resume_successor_generation_v2()"));
    let process_observation = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/process/observation.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    let process_resume = braced_declaration(
        process_observation,
        "pub(crate) async fn resume_recovery_v2(",
    );
    let closed_gate = process_resume
        .find("maintenance_gate_is_closed_v2(pre_gate)")
        .unwrap();
    let pre_database = process_resume
        .find("collect_recovery_resume_database_evidence_v2(&self.foundation)")
        .unwrap();
    let gateway_stage_call = process_resume
        .find("execute_recovery_resume_gateway_stage_v2(")
        .unwrap();
    let record_exact_ready = process_resume.find(".record_exact_ready_v2()").unwrap();
    let post_database = process_resume
        .rfind("collect_recovery_resume_database_evidence_v2(&self.foundation)")
        .unwrap();
    let gate_reobservation = process_resume.find("post_gate != pre_gate").unwrap();
    let worker_resume = process_resume
        .find("lifecycle.into_admission_acknowledging_v2(observation)")
        .unwrap();
    assert_eq!(
        process_resume
            .matches("execute_recovery_resume_gateway_stage_v2(")
            .count(),
        1
    );
    assert!(
        closed_gate < pre_database
            && pre_database < gateway_stage_call
            && gateway_stage_call < record_exact_ready
            && record_exact_ready < post_database
            && post_database < gate_reobservation
            && gate_reobservation < worker_resume
    );
    assert!(!process_resume.contains(".resume_reserved_admission_in_place_v2("));
    assert!(!process_resume.contains(".observe_exact_pause_reservation_v2()"));
    let gateway_stage = braced_declaration(
        process_observation,
        "pub(crate) async fn execute_recovery_resume_gateway_stage_v2(",
    );
    let pause = gateway_stage
        .find(".observe_exact_pause_reservation_v2()")
        .unwrap();
    let successor = gateway_stage
        .find(".recovery_resume_successor_generation_v2()")
        .unwrap();
    let owner_receipt = gateway_stage.find(".recovery_resume_permit_v2()").unwrap();
    let resume_deadline = gateway_stage
        .find("let resume_deadline = Instant::now() + resume_for;")
        .unwrap();
    let discord_resume = gateway_stage
        .find(".resume_reserved_admission_in_place_v2(")
        .unwrap();
    let exact_evidence = gateway_stage
        .find("evidence.coordinator_generation_v2() != coordinator_generation")
        .unwrap();
    let successor_evidence = gateway_stage
        .find("lifecycle.coordinator_generation_v2() != expected_gateway_successor")
        .unwrap();
    let ready = gateway_stage
        .find(".observe_exact_resumed_ready_attestation_v2()")
        .unwrap();
    assert!(
        pause < successor
            && successor < owner_receipt
            && owner_receipt < resume_deadline
            && resume_deadline < discord_resume
            && discord_resume < exact_evidence
            && exact_evidence < successor_evidence
            && successor_evidence < ready
    );
    for forbidden in ["begin_open_v2", "commit_open_v2", "try_acquire_v2"] {
        assert!(
            !contains_identifier(process_resume, forbidden),
            "recovery resume: {forbidden}"
        );
    }
    for name in [
        "RuntimeClosedRecoveryPendingPhaseV2",
        "RuntimeClosedRecoverySessionV2",
        "RuntimeClosedRecoveryReadinessStateV2",
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
    let resume_database = braced_declaration(
        process_observation,
        "async fn collect_recovery_resume_database_evidence_v2(",
    );
    for method in [
        "verify_readiness_refresh_until_v2",
        "into_exact_capability_receipts",
    ] {
        assert_eq!(process_observation.matches(method).count(), 1, "{method}");
        assert_eq!(resume_database.matches(method).count(), 1, "{method}");
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
        for method in [
            "commit_prepared_owner_in_place_v2",
            "committed_pending_section_v2",
        ] {
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
                        || path == Path::new("src/database.rs")
                        || path == Path::new("src/process/observation.rs")
                        || path
                            == Path::new("src/gateway_owner_startup_watchdog_handoff_tests.rs",),
                    "{}: {method}",
                    path.display()
                );
            }
        }
        for method in [
            "refresh_readiness_in_place_v2",
            "invalidate_capability_not_ready_v2",
            "invalidate_protocol_violation_v2",
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
            "refresh_iteration_readiness_in_place_v2",
            "try_into_ready_iteration_v2",
        ] {
            if contains_identifier(source, method) {
                assert!(
                    path == Path::new("src/closed_recovery.rs")
                        || path == Path::new("src/process/readiness.rs")
                        || path == Path::new("src/gateway_owner_startup_watchdog_handoff_tests.rs"),
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
fn startup_recovery_observation_process_is_single_use_interruptible_and_fail_closed() {
    let process = include_str!("../src/process/observation.rs");
    let process_tests = include_str!("../src/process/observation_tests.rs");
    let startup = source_before_test_module(include_str!("../src/process_startup.rs"));
    let library = include_str!("../src/lib.rs");

    for required in [
        "pub enum RuntimeProcessStartupRecoveryObservationFailureV2",
        "pub enum RuntimeProcessStartupRecoveryObservationErrorV2",
        "RuntimeProcessStartupRecoveryObservationErrorV2(<redacted>)",
        "pub(crate) enum RuntimeStartupRecoveryObservationProcessOutcomeV2",
        "Continue(RuntimeStartupRecoveryContinueProcessV2)",
        "FixedPoint(RuntimeStartupRecoveryFixedPointProcessV2)",
        "pub(crate) struct RuntimeStartupRecoveryObservedProcessV2",
        "trait RuntimeStartupRecoveryObservationProcessStepV2<P>",
        "async fn observe_startup_recovery_process_step_v2<R, P>(",
        "fn finalize_startup_recovery_process_step_v2<",
        "pub(crate) async fn observe_startup_recovery_once_v2<P>(",
        "P: RuntimeStartupRecoveryObservationPortV2 + Sync",
        "iteration.owner_terminal_observation_v2()",
        "discord.wait_terminal().await.exit()",
        "await_startup_recovery_observation_interrupt_v2(",
        ".observe_startup_recovery_interruptible_in_place_v2(observer, interrupt)",
        "pub(crate) fn into_startup_recovery_observation_outcome_v2(",
        "cleanup_after_startup_recovery_observation_failure_v2(",
        "current_typed_observation_outcome_transition_v2(",
        "shutdown_startup_observation_process_v2(",
        "sequence_startup_observation_cleanup_v2<",
        "finish_observation_transition_v2(",
        "shutdown_paused_foundation_owner_v1,",
        "#[path = \"observation_tests.rs\"]",
    ] {
        assert!(process.contains(required), "{required}");
    }
    for required_test in [
        "generic_process_flow_finalizes_and_shuts_down_both_typed_outcomes",
        "generic_process_failures_retain_cleanup_for_deadline_observer_and_state_change",
        "generic_process_interrupts_drop_the_observer_and_preserve_cleanup",
        "generic_process_future_drop_preserves_resource_and_cleanup_authority",
        "generic_finalize_retains_each_authority_shape_for_ordered_cleanup",
    ] {
        assert!(process_tests.contains(required_test), "{required_test}");
        assert!(!process.contains(required_test), "{required_test}");
    }
    assert_eq!(
        process
            .matches(".observe_startup_recovery_interruptible_in_place_v2(observer, interrupt)")
            .count(),
        1
    );
    let production_transition = braced_declaration(
        process,
        "pub(crate) async fn observe_startup_recovery_once_v2<P>(",
    );
    let signature = production_transition.split("where").next().unwrap();
    assert!(signature.contains("&mut self"));
    assert!(production_transition
        .contains("observe_startup_recovery_process_step_v2(self, observer).await"));
    let transition = braced_declaration(
        process,
        "async fn observe_startup_recovery_process_step_v2<R, P>(",
    );
    let currentness = transition
        .match_indices("resource.current_failure_v2()")
        .map(|(position, _)| position)
        .collect::<Vec<_>>();
    assert_eq!(currentness.len(), 2);
    let observe = transition
        .find("resource.observe_once_v2(observer).await?")
        .unwrap();
    assert!(currentness[0] < observe && observe < currentness[1]);
    let concrete = braced_declaration(
        process,
        "impl<P> RuntimeStartupRecoveryObservationProcessStepV2<P>",
    );
    let owner_terminal = concrete
        .find("iteration.owner_terminal_observation_v2()")
        .unwrap();
    let concrete_observe = concrete
        .find(".observe_startup_recovery_interruptible_in_place_v2(observer, interrupt)")
        .unwrap();
    assert!(owner_terminal < concrete_observe);
    let finalize = braced_declaration(
        process,
        "pub(crate) fn into_startup_recovery_observation_outcome_v2(",
    );
    assert!(!finalize.contains(".await"));
    assert!(finalize.contains("finalize_startup_recovery_process_step_v2("));
    let finalization = braced_declaration(process, "fn finalize_startup_recovery_process_step_v2<");
    let current_resource = finalization.find("current_resource(&resource)").unwrap();
    let finalize_resource = finalization.find("finalize(resource, observed)?").unwrap();
    let current_outcome = finalization.find("current_outcome(&outcome)").unwrap();
    assert!(current_resource < finalize_resource && finalize_resource < current_outcome);
    let cleanup = braced_declaration(process, "async fn sequence_startup_observation_cleanup_v2<");
    let discord_cleanup = cleanup.find("start_discord().await").unwrap();
    let owner_cleanup = cleanup.find("start_owner().await").unwrap();
    let database_cleanup = cleanup.find("start_database().await").unwrap();
    let finish_cleanup = cleanup
        .find("finish(discord, finish_owner_held(owner, database))")
        .unwrap();
    assert!(
        discord_cleanup < owner_cleanup
            && owner_cleanup < database_cleanup
            && database_cleanup < finish_cleanup
    );
    for declaration in [
        "impl RuntimeStartupRecoveryContinueProcessV2",
        "impl RuntimeStartupRecoveryFixedPointProcessV2",
    ] {
        let typed_process = braced_declaration(process, declaration);
        assert!(typed_process.contains("pub(crate) async fn shutdown(self)"));
        assert!(typed_process.contains("shutdown_startup_observation_process_v2("));
    }
    assert!(!startup.contains("observe_startup_recovery_once_v2"));
    assert!(library.contains("RuntimeProcessStartupRecoveryObservationErrorV2"));
    assert!(library.contains("RuntimeProcessStartupRecoveryObservationFailureV2"));
    for private in [
        "RuntimeStartupRecoveryObservationProcessOutcomeV2",
        "RuntimeStartupRecoveryContinueProcessV2",
        "RuntimeStartupRecoveryFixedPointProcessV2",
        "RuntimeStartupRecoveryObservedProcessV2",
        "RuntimeStartupRecoveryObservationFinalizeFailureV2",
    ] {
        assert!(!library.contains(private), "{private}");
    }
    for forbidden in [
        "activate",
        "deploy",
        "open_admission",
        "sqlx",
        "PgPool",
        "PostgresRuntime",
    ] {
        assert!(!contains_identifier(process, forbidden), "{forbidden}");
    }
}

#[test]
fn startup_recovery_loop_is_bounded_reobserving_and_non_authorizing() {
    let process = include_str!("../src/process/startup_loop.rs")
        .split_once("\n#[cfg(test)]")
        .map(|(production, _)| production)
        .unwrap();
    let process_tests = include_str!("../src/process/startup_loop_tests.rs");
    let process_module = include_str!("../src/process.rs");
    let startup = source_before_test_module(include_str!("../src/process_startup.rs"));
    let library = include_str!("../src/lib.rs");

    for required in [
        "pub enum RuntimeProcessStartupRecoveryLoopFailureV2",
        "pub enum RuntimeProcessStartupRecoveryLoopErrorV2",
        "RuntimeProcessStartupRecoveryLoopErrorV2(<redacted>)",
        "trait RuntimeStartupRecoveryLoopReadyStepV2",
        "trait RuntimeStartupRecoveryLoopContinueStepV2",
        "async fn drive_startup_recovery_loop_v2<Ready>(",
        "self.foundation.databases.execution().clone()",
        "self.observe_startup_recovery_once_v2(&observer)",
        "RuntimeStartupRecoveryLoopIterationOutcomeV2::FixedPoint",
        "RuntimeStartupRecoveryContinuationV2::WaitForForeignFresh",
        "wait_for_foreign_fresh_in_place_v2",
        "into_recovery_iteration_ready_v2()",
        "RuntimeStartupRecoveryContinuationV2::Recover",
        "execute_recovery_in_place_v2",
        "cleanup_after_recovery_failure_v2",
        "into_next_ready_after_recovery_v2",
        "sleep_until(TokioInstant::from_std(wait_cutoff))",
        "sleep_until(TokioInstant::from_std(retry_at))",
    ] {
        assert!(process.contains(required), "{required}");
    }
    let driver = braced_declaration(process, "async fn drive_startup_recovery_loop_v2<Ready>(");
    let observe = driver.find("ready.observe_in_place_v2().await").unwrap();
    let finalize = driver
        .find("ready.finalize_observation_v2(observed)")
        .unwrap();
    let fixed_point = driver
        .find("RuntimeStartupRecoveryLoopIterationOutcomeV2::FixedPoint")
        .unwrap();
    let continuation = driver
        .find("RuntimeStartupRecoveryLoopIterationOutcomeV2::Continue")
        .unwrap();
    let wait = driver.find("process.wait_in_place_v2().await").unwrap();
    let readiness = driver
        .find("process.into_next_ready_v2(completion).await")
        .unwrap();
    let recovery = driver
        .find("process.execute_recovery_in_place_v2(class).await")
        .unwrap();
    let recovery_readiness = driver
        .find(".into_next_ready_after_recovery_v2(completion)")
        .unwrap();
    assert!(observe < finalize);
    assert!(finalize < fixed_point);
    assert!(finalize < continuation);
    assert!(continuation < wait && wait < readiness);
    assert!(continuation < recovery && recovery < recovery_readiness);
    let wait_method = braced_declaration(process, "async fn wait_for_foreign_fresh_in_place_v2(");
    let wait_signature = wait_method
        .split_once('{')
        .map(|(signature, _)| signature)
        .unwrap();
    assert!(wait_signature.contains("&mut self"));
    assert!(!wait_signature.contains("\n        self,"));
    assert!(wait_method.contains(".wait_for_bounded_startup_retry_in_place_v2("));
    let bounded_wait = braced_declaration(
        process,
        "async fn wait_for_bounded_startup_retry_in_place_v2(",
    );
    let operation_cutoff = bounded_wait
        .find("self.foundation.startup_budget.operation_cutoff()")
        .unwrap();
    let owner_cutoff = bounded_wait
        .find("self.session.owner_safety_deadline_v2()")
        .unwrap();
    let minimum = bounded_wait
        .find("operation_cutoff.min(owner_safety_deadline)")
        .unwrap();
    let wait_race = bounded_wait
        .find("await_bounded_startup_retry_v2(")
        .unwrap();
    assert!(operation_cutoff < minimum && owner_cutoff < minimum && minimum < wait_race);
    let race = braced_declaration(process, "async fn await_bounded_startup_retry_v2<");
    let deadline = race.find("() = &mut deadline").unwrap();
    let discord = race.find("transition = &mut discord_terminal").unwrap();
    let owner = race.find("() = &mut owner_terminal").unwrap();
    let retry = race.find("() = &mut retry").unwrap();
    assert!(deadline < discord && discord < owner && owner < retry);
    let unavailable = braced_declaration(process, "fn unavailable_recovery_failure_v2(");
    for class in [
        "RuntimeStartupRecoveryClassV2::StaleLive",
        "RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification",
        "RuntimeStartupRecoveryClassV2::SuspendedLocalEffect",
    ] {
        assert_eq!(unavailable.matches(class).count(), 1, "{class}");
    }
    let production_continue = braced_declaration(
        process,
        "impl RuntimeStartupRecoveryLoopContinueStepV2 for RuntimeStartupRecoveryContinueProcessV2",
    );
    let recovery_cleanup =
        braced_declaration(production_continue, "fn cleanup_after_recovery_failure_v2(");
    assert_eq!(
        recovery_cleanup
            .matches("self.session.invalidate_startup_recovery_execution_v2()")
            .count(),
        1
    );
    assert_eq!(recovery_cleanup.matches("self.shutdown().await").count(), 1);
    let wait_cleanup = braced_declaration(production_continue, "fn cleanup_after_wait_failure_v2(");
    assert_eq!(wait_cleanup.matches("self.shutdown().await").count(), 1);
    let production_entry = braced_declaration(
        process,
        "pub(crate) async fn into_startup_recovery_fixed_point_v2(",
    );
    assert_eq!(
        production_entry
            .matches("drive_startup_recovery_loop_v2(self).await")
            .count(),
        1
    );
    for required_test in [
        "production_used_driver_reobserves_after_foreign_fresh_and_stops_at_fixed_point",
        "production_used_driver_refreshes_and_reobserves_after_supported_recovery_execution",
        "production_used_driver_cleans_each_failure_authority_exactly_once",
        "foreign_fresh_wait_has_deterministic_deadline_discord_owner_retry_priority",
        "dropping_a_polled_foreign_fresh_wait_drops_only_the_wait_future",
        "canceling_the_production_used_borrowed_wait_retains_exactly_one_cleanup_authority",
        "canceling_a_borrowed_recovery_retains_exactly_one_cleanup_authority",
        "all_recovery_classes_map_to_distinct_finite_fail_closed_failures",
        "supported_recovery_database_failures_preserve_exact_persistence_codes",
    ] {
        assert!(process_tests.contains(required_test), "{required_test}");
        assert!(!process.contains(required_test), "{required_test}");
    }
    assert_eq!(
        startup
            .matches(".into_startup_recovery_fixed_point_v2()")
            .count(),
        1
    );
    assert!(library.contains("RuntimeProcessStartupRecoveryLoopErrorV2"));
    assert!(library.contains("RuntimeProcessStartupRecoveryLoopFailureV2"));
    assert!(process_module.contains("mod startup_loop;"));
    assert!(process_module.contains("mod execution;"));
    assert!(!library.contains("mod startup_loop;"));
    for forbidden in [
        "resume_admission",
        "open_admission",
        "activate",
        "deploy",
        "ready_to_serve",
        "health_ready",
        "twilight",
        "sqlx",
        "PgPool",
        "PostgresRuntime",
        "LlmClient",
        "gemma",
        "codex",
    ] {
        assert!(!contains_identifier(process, forbidden), "{forbidden}");
    }
}

#[test]
fn supported_startup_recovery_execution_is_interruptible_one_way_and_forces_fresh_observation() {
    let execution = source_before_test_module(include_str!("../src/process/execution.rs"));
    let startup_loop = include_str!("../src/process/startup_loop.rs")
        .split_once("\n#[cfg(test)]")
        .map(|(production, _)| production)
        .unwrap();
    let closed = include_str!("../src/closed_recovery.rs");
    let gateway = source_before_test_module(include_str!("../src/gateway.rs"));
    let finalizer = include_str!("../src/process/pending_drain_finalizer.rs");

    for required in [
        "RuntimeStartupRecoveryExecutionPortV2",
        "RuntimeStartupRecoveryClassV2::StaleLive",
        "RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification",
        "RuntimeStartupRecoveryClassV2::SuspendedLocalEffect",
        "RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent",
        ".begin_startup_recovery_execution_v2(",
        ".execute_startup_recovery(authorization, execution_cutoff)",
        ".complete_startup_recovery_execution_v2(completed)",
        "operation_cutoff.min(owner_safety_deadline)",
        "await_startup_recovery_execution_v2(",
        "tokio::select!",
        "biased;",
        "RuntimeStartupRecoveryExecutionReceiptOutcomeV2::Progressed",
        "RuntimeStartupRecoveryExecutionReceiptOutcomeV2::NoCandidate",
        "RuntimeStartupRecoveryExecutionReceiptOutcomeV2::RetryAfter",
        "invalidate_startup_recovery_execution_v2",
        "prefer_current_startup_recovery_execution_failure_v2",
        "startup_recovery_execution_database_failure_v2",
        "startup_recovery_execution_rejected_v2",
        "startup_recovery_execution_retry_after_unsupported_v2",
    ] {
        assert!(execution.contains(required), "{required}");
    }
    assert!(
        execution
            .matches("current_startup_recovery_execution_transition_v2(self)")
            .count()
            >= 6
    );
    let race = braced_declaration(execution, "async fn await_startup_recovery_execution_v2<");
    let deadline = race.find("() = &mut deadline").unwrap();
    let discord = race.find("transition = &mut discord_terminal").unwrap();
    let owner = race.find("() = &mut owner_terminal").unwrap();
    let database = race.find("result = &mut execution").unwrap();
    assert!(deadline < discord && discord < owner && owner < database);
    let current = braced_declaration(
        execution,
        "fn classify_current_startup_recovery_execution_transition_v2(",
    );
    let cutoff = current
        .find("now >= operation_cutoff.min(owner_safety_deadline)")
        .unwrap();
    let discord = current.find("if let Some(error) = discord").unwrap();
    let owner = current.find("owner_terminal.then_some").unwrap();
    assert!(cutoff < discord && discord < owner);
    let process_execution = braced_declaration(
        execution,
        "pub(super) async fn execute_startup_recovery_in_place_v2(",
    );
    let begin = process_execution
        .find(".begin_startup_recovery_execution_v2(")
        .unwrap();
    let database = process_execution
        .find(".execute_startup_recovery(authorization, execution_cutoff)")
        .unwrap();
    let complete = process_execution
        .find(".complete_startup_recovery_execution_v2(completed)")
        .unwrap();
    assert!(begin < database && database < complete);
    let pending_execution = braced_declaration(
        execution,
        "async fn execute_pending_drain_recovery_owned_v3(",
    );
    let pending_begin = pending_execution
        .find(".begin_startup_recovery_execution_v2(")
        .unwrap();
    let pending_select = pending_execution.find(".select_pending_drain_v3(").unwrap();
    let pending_accept = pending_execution.find(".accept_selection(").unwrap();
    let pending_stage = pending_execution
        .find("execute_owned_pending_drain_stage_v3(")
        .unwrap();
    let pending_gateway_completion = pending_execution
        .rfind(".complete_startup_recovery_execution_v2(completed)")
        .unwrap();
    assert!(
        pending_begin < pending_select
            && pending_select < pending_accept
            && pending_accept < pending_stage
            && pending_stage < pending_gateway_completion
    );
    assert_eq!(
        pending_execution
            .matches(".select_pending_drain_v3(")
            .count(),
        1
    );
    assert_eq!(
        pending_execution
            .matches("execute_owned_pending_drain_stage_v3(")
            .count(),
        4
    );
    let registration =
        braced_declaration(finalizer, "pub(crate) fn register_pending_drain_job_v3<");
    let register = registration.find(".try_register(").unwrap();
    let waiter_drop = registration.find("drop(waiter)").unwrap();
    assert!(register < waiter_drop);
    let completion_helper = braced_declaration(
        finalizer,
        "pub(crate) async fn complete_registered_pending_drain_job_v3<",
    );
    assert!(completion_helper.contains("supervisor.next_completion().await"));
    let combined = braced_declaration(
        finalizer,
        "pub(crate) async fn register_and_complete_pending_drain_job_v3<",
    );
    let register = combined.find("register_pending_drain_job_v3(").unwrap();
    let completion = combined
        .find("complete_registered_pending_drain_job_v3(")
        .unwrap();
    assert!(register < completion);
    let stage_execution = braced_declaration(
        finalizer,
        "async fn execute_pending_drain_mutation_stage_v3<",
    );
    assert_eq!(
        stage_execution
            .matches("pending_drain_requires_exact_finalization_v3(&error)")
            .count(),
        4
    );
    for required in [
        ".record_no_candidate_v3(session, &selection)",
        ".execute_claim_v3(session, &authorization)",
        ".execute_acknowledgement_v3(session, &authorization)",
        ".execute_succession_v3(session, &authorization)",
        ".unseal_pending_drain_after_durable_ack_v2(&durable)",
        ".unseal_pending_drain_after_durable_succession_v3(&durable)",
        ".complete_registry_rollover(unseal)",
    ] {
        assert!(stage_execution.contains(required), "{required}");
    }
    let pending_finalization = braced_declaration(
        finalizer,
        "fn pending_drain_requires_exact_finalization_v3(",
    );
    assert!(pending_finalization.contains("RuntimeExecutionPersistenceErrorV1::Indeterminate"));
    assert!(!pending_finalization.contains(".class()"));
    let production_continue = braced_declaration(
        startup_loop,
        "impl RuntimeStartupRecoveryLoopContinueStepV2 for RuntimeStartupRecoveryContinueProcessV2",
    );
    let loop_recovery = braced_declaration(
        production_continue,
        "async fn into_next_ready_after_recovery_v2(",
    );
    let bounded_retry = loop_recovery
        .find(".wait_for_bounded_startup_retry_in_place_v2(")
        .unwrap();
    let refreshed_recovery = loop_recovery
        .find(".into_recovery_iteration_ready_v2()")
        .unwrap();
    assert!(bounded_retry < refreshed_recovery);
    assert!(loop_recovery.contains("RuntimeClosedRecoveryProcessV2"));
    assert!(loop_recovery.contains(".into_recovery_iteration_ready_v2()"));
    assert!(startup_loop.contains("ready = process"));
    assert!(startup_loop.contains(".into_next_ready_after_recovery_v2(completion)"));
    for required in [
        "pub(crate) fn begin_startup_recovery_execution_v2(",
        "pub(crate) fn complete_startup_recovery_execution_v2(",
        "pub(crate) fn invalidate_startup_recovery_execution_v2(",
    ] {
        assert!(closed.contains(required), "{required}");
    }
    let gateway_begin = braced_declaration(
        gateway,
        "pub(crate) fn begin_startup_recovery_execution_v2(",
    );
    let begin_preflight = gateway_begin
        .find("self.committed_pending_section_v2(committed_owner)")
        .unwrap();
    let begin_transition = gateway_begin
        .find(".begin_startup_recovery_execution(permit, continuation)")
        .unwrap();
    let begin_postflight = gateway_begin
        .rfind("self.committed_pending_section_v2(committed_owner)")
        .unwrap();
    assert!(begin_preflight < begin_transition && begin_transition < begin_postflight);
    let gateway_complete = braced_declaration(
        gateway,
        "pub(crate) fn complete_startup_recovery_execution_v2(",
    );
    let complete_preflight = gateway_complete
        .find("self.committed_pending_section_v2(committed_owner)")
        .unwrap();
    let complete_transition = gateway_complete
        .find(".complete_startup_recovery_execution(permit, completed)")
        .unwrap();
    let complete_postflight = gateway_complete
        .rfind("self.committed_pending_section_v2(committed_owner)")
        .unwrap();
    assert!(complete_preflight < complete_transition && complete_transition < complete_postflight);
    assert!(gateway_complete.contains("owner_invalidated.store(true, Ordering::Release)"));
    assert!(gateway.contains("invalidate_capability_not_ready_v2"));
    for forbidden in [
        "resume_admission",
        "open_admission",
        "activate",
        "deploy",
        "ready_to_serve",
        "health_ready",
        "twilight",
        "sqlx",
        "PgPool",
    ] {
        assert!(!contains_identifier(execution, forbidden), "{forbidden}");
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
        .map(|(_, source)| source_before_test_module(source))
        .unwrap();

    for required in [
        "pub(crate) enum RuntimeClosedRecoveryStartupObservationErrorV2<E>",
        "pub(crate) enum RuntimeClosedRecoveryStartupObservationAttemptErrorV2<E, I>",
        "pub(crate) struct RuntimeClosedRecoveryStartupObservationFailureV2<E, I>",
        "pub(crate) struct RuntimeClosedRecoveryStartupObservationCompletionV2",
        "pub(crate) async fn observe_startup_recovery_v2<P>(",
        "async fn observe_startup_recovery_with_v2<",
        "async fn observe_startup_recovery_interruptible_in_place_with_v2<",
        "pub(crate) fn into_startup_recovery_observation_outcome_v2(",
        "fn finalize_startup_recovery_observation_with_v2<",
        ".operation_cutoff",
        ".min(self.owner.observation().safety_deadline())",
        ".begin_startup_recovery_observation_v2(&self.owner, iteration)",
        "biased;",
        "sleep_until(TokioInstant::from_std(observation_cutoff))",
        "let observed = observe(authorization, observation_cutoff)",
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
    let wrapper = braced_declaration(observation, "async fn observe_startup_recovery_with_v2<");
    assert!(wrapper.contains("observe_startup_recovery_interruptible_in_place_with_v2("));
    assert!(wrapper.contains("finalize_startup_recovery_observation_with_v2("));
    assert!(wrapper.contains("std::future::pending::<std::convert::Infallible>()"));
    let observation_stage = braced_declaration(
        observation,
        "async fn observe_startup_recovery_interruptible_in_place_with_v2<",
    );
    let initial_revalidation = observation_stage.find("self.revalidate_v2()").unwrap();
    let cutoff = observation_stage.find("let observation_cutoff").unwrap();
    let take_iteration = observation_stage.find("self.iteration.take()").unwrap();
    let begin = observation_stage
        .find(".begin_startup_recovery_observation_v2(&self.owner, iteration)")
        .unwrap();
    let await_observation = observation_stage
        .find("let observed = observe(authorization, observation_cutoff)")
        .unwrap();
    let stage_revalidation = observation_stage
        .rfind("revalidate_committed_recovery_v2(")
        .unwrap();
    assert!(
        initial_revalidation < cutoff
            && cutoff < take_iteration
            && take_iteration < begin
            && begin < await_observation
            && await_observation < stage_revalidation
    );
    let finalize = braced_declaration(
        observation,
        "fn finalize_startup_recovery_observation_with_v2<",
    );
    let revalidations = finalize
        .match_indices("revalidate_committed_recovery_v2(")
        .map(|(position, _)| position)
        .collect::<Vec<_>>();
    assert_eq!(revalidations.len(), 3);
    let successor = finalize
        .find(".into_startup_recovery_observation_successor_v2(&owner, completed)")
        .unwrap();
    let post_complete = finalize.find("post_complete();").unwrap();
    let post_revalidate = finalize.rfind("post_revalidate();").unwrap();
    let outcome = finalize.find("match outcome").unwrap();
    let fixed_validation = finalize
        .find(".validate_startup_recovery_fixed_point_v2(&owner, &proof)")
        .unwrap();
    let fixed_construction = finalize
        .find("let fixed_point = RuntimeClosedRecoveryFixedPointV2")
        .unwrap();
    let fixed_revalidation = finalize
        .find("if let Err(error) = fixed_point.revalidate_v2()")
        .unwrap();
    assert!(
        revalidations[0] < successor
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
        .find(".begin_startup_recovery_observation(permit, iteration)")
        .unwrap();
    let begin_postflight = begin_gateway
        .rfind("self.committed_pending_section_v2(committed_owner)")
        .unwrap();
    assert!(begin_preflight < begin_transition && begin_transition < begin_postflight);
    let complete_gateway = braced_declaration(
        gateway,
        "pub(crate) fn into_startup_recovery_observation_successor_v2(",
    );
    assert!(complete_gateway.contains(".complete_startup_recovery_observation(permit, completed)"));
    assert!(complete_gateway.contains("owner_invalidated.store(true, Ordering::Release)"));
    let fixed_gateway = braced_declaration(
        gateway,
        "pub(crate) fn validate_startup_recovery_fixed_point_v2(",
    );
    let fixed_preflight = fixed_gateway
        .find("self.committed_pending_section_v2(committed_owner)")
        .unwrap();
    let fixed_transition = fixed_gateway
        .find(".validate_startup_recovery_fixed_point(self.permit_v2(), proof)")
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
        if path != Path::new("src/registry.rs")
            && path != Path::new("src/registry_staging_tests.rs")
            && path != Path::new("src/registry_succession_tests.rs")
            && path != Path::new("src/runtime_controller.rs")
        {
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
    let runtime_controller = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/runtime_controller.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    assert!(runtime_controller.contains(
        "use automation_runtime_registry::{ExactServingRouteError, ExactServingRouteV1};"
    ));
    for forbidden in [
        "ServingSlotRegistryV1",
        "SlotMutationTokenV1",
        "SlotAdmissionStateV2",
        "RegistryRecoveryObservationGuardV2",
        "RegistryEmptyRecoveryCursorV2",
    ] {
        assert!(
            !contains_identifier(runtime_controller, forbidden),
            "{forbidden}"
        );
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
        "pub(crate) struct RuntimeRegistryPreparedServingTransitionV2",
        "pub(crate) struct RuntimeRegistryServingBindingV2",
        "pub(crate) struct RuntimeRegistryServingTransitionFailureV2",
        "pub(crate) fn prepare_serving_transition_v2(",
        "route_set_epoch: &RuntimeRouteSetEpochV2",
        "pub(crate) fn observe_route_set_v2(",
        "pub(crate) fn commit_v2(",
        "pub(crate) fn cancel_v2(self) -> RuntimeRegistryEmptyRecoveryBindingV2",
        "pub(crate) fn into_binding_v2(self) -> RuntimeRegistryEmptyRecoveryBindingV2",
        "accept_runtime_route_set_observation_v2(",
        "RuntimeRouteSetObservationInputV2 {",
        "RuntimeRegistryPreparedServingTransitionV2(<redacted>)",
        "RuntimeRegistryServingBindingV2(<redacted>)",
        "RuntimeRegistryServingTransitionFailureV2(<redacted>)",
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
        "pub(crate) struct RuntimeRegistryPendingDrainSealBindingV2",
        "pub(crate) fn into_pending_drain_seal_binding_v2(",
        "candidate: &RuntimePendingDrainCandidateV2",
        "candidate.intent_id().canonical_bytes()",
        "pub(crate) fn into_pending_drain_succession_seal_binding_v3(",
        "candidate: &RuntimePendingDrainPreviousOwnerClaimedCandidateV3",
        "fn into_pending_drain_seal_binding_common_v2(",
        "pub(crate) struct RuntimeRegistryPendingDrainSuccessionSealBindingV3",
        "binding: RuntimeRegistryPendingDrainSealBindingV2",
        "RuntimeRegistryPendingDrainSuccessionSealBindingV3(<redacted>)",
        "pub(crate) fn into_empty_binding_after_durable_ack_v2(",
        "durable: &RuntimeDurablyAcknowledgedPendingDrainV2",
        "pub(crate) fn into_empty_binding_after_durable_succession_v3(",
        "durable: &RuntimeDurablyAcknowledgedPendingDrainSuccessionV3",
        "fn into_empty_binding_after_durable_seal_common_v2(",
        "RuntimePendingDrainRegistryUnsealWitnessV2",
        "require_pending_drain_durable_seal_match_v2(&self.witness, durable_seal)",
        "successor_persistence_non_zero_u64_v2(",
        ".unseal_empty_recovery_drain_claim_v2(sealed)",
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
        "RuntimeRegistryPreparedServingTransitionV2",
        "RuntimeRegistryServingBindingV2",
        "RuntimeRegistryServingTransitionFailureV2",
        "RuntimeRegistryPendingDrainSealBindingV2",
        "RuntimeRegistryPendingDrainSuccessionSealBindingV3",
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
    assert!(production.contains(concat!(
        "pub(crate) struct RuntimeRegistryPreparedServingTransitionV2 {\n",
        "    binding: RuntimeRegistryEmptyRecoveryBindingV2,\n",
        "    initial_registry_observation_sequence: RuntimeRegistryGlobalObservationSequenceV2,\n",
        "    initial_retained_slot_count: u64,\n",
        "    initial_retained_empty_tombstone_count: u64,\n",
        "}"
    )));
    assert!(production.contains(concat!(
        "pub(crate) struct RuntimeRegistryServingBindingV2 {\n",
        "    process_instance_id: ProcessInstanceId,\n",
        "    registry: ServingSlotRegistryV1,\n",
        "    initial_registry_observation_sequence: RuntimeRegistryGlobalObservationSequenceV2,\n",
        "    initial_retained_slot_count: u64,\n",
        "    initial_retained_empty_tombstone_count: u64,\n",
        "}"
    )));
    assert!(production.contains(concat!(
        "pub(crate) struct RuntimeRegistryServingTransitionFailureV2 {\n",
        "    binding: RuntimeRegistryEmptyRecoveryBindingV2,\n",
        "    error: RuntimeRegistryRecoveryObservationErrorV1,\n",
        "}"
    )));
    assert_eq!(
        production
            .matches("        Ok(RuntimeRegistryEmptyRecoveryBindingV2 {")
            .count(),
        1
    );
    let v2_unseal = braced_declaration(
        production,
        "pub(crate) fn into_empty_binding_after_durable_ack_v2(",
    );
    assert!(v2_unseal
        .contains("self.into_empty_binding_after_durable_seal_common_v2(durable.seal_witness())"));
    assert!(!v2_unseal.contains("RuntimeDurablyAcknowledgedPendingDrainSuccessionV3"));
    let v3_unseal = braced_declaration(
        production,
        "pub(crate) fn into_empty_binding_after_durable_succession_v3(",
    );
    assert!(v3_unseal
        .contains(".into_empty_binding_after_durable_seal_common_v2(durable.seal_witness())"));
    assert!(!v3_unseal.contains("RuntimeDurablyAcknowledgedPendingDrainV2"));
    let unseal = braced_declaration(
        production,
        "fn into_empty_binding_after_durable_seal_common_v2(",
    );
    let durable_match = unseal
        .find("require_pending_drain_durable_seal_match_v2")
        .unwrap();
    let exact_revalidation = unseal.find("self.revalidate_sealed_v2()?").unwrap();
    let slot_headroom = unseal
        .find("let expected_slot_observation_sequence")
        .unwrap();
    let admission_headroom = unseal.find("let expected_admission_generation").unwrap();
    let registry_headroom = unseal
        .find("let expected_registry_observation_sequence")
        .unwrap();
    let expected_projection = unseal
        .find("accept_runtime_registry_recovery_empty_observation_v2(")
        .unwrap();
    let unseal_witness = unseal
        .find("RuntimePendingDrainRegistryUnsealWitnessV2::new(")
        .unwrap();
    let mutation = unseal
        .find(".unseal_empty_recovery_drain_claim_v2(sealed)")
        .unwrap();
    let post_projection = unseal
        .find("project_empty_observation_v2(&process_instance_id, registry_observation)")
        .unwrap();
    let post_revalidation = unseal
        .find("binding.revalidate_empty_projection_unordered_v2()?")
        .unwrap();
    let actual_unseal_witness = unseal
        .rfind("RuntimePendingDrainRegistryUnsealWitnessV2::new(")
        .unwrap();
    assert!(
        durable_match < exact_revalidation
            && exact_revalidation < slot_headroom
            && slot_headroom < admission_headroom
            && admission_headroom < registry_headroom
            && registry_headroom < expected_projection
            && expected_projection < unseal_witness
            && unseal_witness < mutation
            && mutation < post_projection
            && post_projection < post_revalidation
            && post_revalidation < actual_unseal_witness
    );
    assert_eq!(
        production
            .matches(".seal_empty_recovery_drain_claim_v2(self.cursor, &key, seal_key)")
            .count(),
        1
    );
    assert_eq!(
        production
            .matches(".unseal_empty_recovery_drain_claim_v2(sealed)")
            .count(),
        1
    );
    assert!(!production.contains(".unseal_drain_claim_v2("));
    let serving_binding = braced_declaration(production, "impl RuntimeRegistryServingBindingV2");
    assert_eq!(
        serving_binding
            .matches("pub(crate) fn observe_route_set_v2(")
            .count(),
        1
    );
    for forbidden in [
        "pub(crate) fn install(",
        "pub(crate) fn activate",
        "pub(crate) fn begin_drain",
        "pub(crate) fn remove",
        "pub(crate) fn admit",
        "pub(crate) fn registry",
        "pub(crate) fn into_registry",
        "pub(crate) fn cursor",
    ] {
        assert!(!serving_binding.contains(forbidden), "{forbidden}");
    }
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
            "RuntimeRegistryPreparedServingTransitionV2",
            "RuntimeRegistryServingBindingV2",
            "RuntimeRegistryServingTransitionFailureV2",
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
        "RuntimeRegistryPreparedServingTransitionV2",
        "RuntimeRegistryServingBindingV2",
        "RuntimeRegistryServingTransitionFailureV2",
        "RuntimeRegistryPendingDrainSealBindingV2",
        "RuntimeRegistryPendingDrainSuccessionSealBindingV3",
    ] {
        assert!(!library.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn staged_registry_port_is_process_bound_owned_and_fail_closed() {
    let registry = source_before_test_module(include_str!("../src/registry.rs"));
    let closed_recovery = include_str!("../src/closed_recovery.rs");

    assert!(registry.contains(concat!(
        "pub(crate) struct RuntimeRegistryStagingPortV2 {\n",
        "    process_instance_id: ProcessInstanceId,\n",
        "    registry: ServingSlotRegistryV1,\n",
        "}"
    )));
    assert!(registry.contains(concat!(
        "pub(crate) struct RuntimeRegistryStagedRouteV2 {\n",
        "    registry: ServingSlotRegistryV1,\n",
        "    identity: RuntimeProcessIdentityV1,\n",
        "    token: Option<SlotMutationTokenV1>,\n",
        "    emergency: RuntimeRegistryEmergencyTriggerV2,\n",
        "}"
    )));
    assert!(registry.contains(concat!(
        "pub(crate) struct RuntimeRegistryStagedInstallEvidenceV2 {\n",
        "    route: SlotRouteWitnessV1,\n",
        "    active_interactions: u32,\n",
        "    admission_generation: NonZeroU64,\n",
        "    slot_observation_sequence: NonZeroU64,\n",
        "    registry_observation_sequence: RuntimeRegistryGlobalObservationSequenceV2,\n",
        "}"
    )));
    assert!(registry.contains(concat!(
        "pub(crate) struct RuntimeRegistryStagedInstallV2 {\n",
        "    outcome: RuntimeRegistryStagedInstallOutcomeV2,\n",
        "    evidence: RuntimeRegistryStagedInstallEvidenceV2,\n",
        "    authority: RuntimeRegistryStagedRouteV2,\n",
        "}"
    )));
    assert!(contains_identifier(
        declaration_attribute_block(registry, "RuntimeRegistryStagingPortV2"),
        "Clone"
    ));
    assert!(!contains_identifier(
        declaration_attribute_block(registry, "RuntimeRegistryStagedRouteV2"),
        "Clone"
    ));
    assert!(!implements_trait(
        registry,
        "RuntimeRegistryStagedRouteV2",
        "Clone"
    ));
    assert!(!contains_identifier(
        declaration_attribute_block(registry, "RuntimeRegistryStagedInstallV2"),
        "Clone"
    ));
    assert!(!implements_trait(
        registry,
        "RuntimeRegistryStagedInstallV2",
        "Clone"
    ));
    assert!(!registry.contains("RuntimeRouteWitnessV2"));

    let binding = braced_declaration(registry, "impl RuntimeRegistryServingBindingV2");
    let factory = braced_declaration(
        binding,
        "pub(crate) fn staging_port_v2(&self) -> RuntimeRegistryStagingPortV2",
    );
    assert!(factory.contains("process_instance_id: self.process_instance_id.clone()"));
    assert!(factory.contains("registry: self.registry.clone()"));
    assert!(!binding.contains("install_staged_route_v2"));

    let port = braced_declaration(registry, "impl RuntimeRegistryStagingPortV2");
    let install = braced_declaration(port, "pub(crate) fn install_staged_route_v2(");
    let process_match = install
        .find("route.identity().process_instance_id")
        .unwrap();
    let registry_install = install.find(".install(key, route, fencing_token)").unwrap();
    let lifecycle = install.find("SlotInstallOutcomeV1::Staged").unwrap();
    let authority = install
        .find("let staged = RuntimeRegistryStagedRouteV2")
        .unwrap();
    let witness = install.find("self.registry.route_witness(token)").unwrap();
    let atomic = install
        .find("self.registry.atomic_observation_v2(token.key())")
        .unwrap();
    let global = install
        .find("self.registry.recovery_observation_v2()")
        .unwrap();
    let complete = install.find("complete_staged_install_v2(").unwrap();
    assert!(
        process_match < registry_install
            && registry_install < lifecycle
            && lifecycle < authority
            && authority < witness
            && witness < atomic
            && atomic < global
            && global < complete
    );
    assert!(install.contains(concat!(
        "SlotInstallOutcomeV1::Staged => ",
        "RuntimeRegistryStagedInstallOutcomeV2::Installed"
    )));
    assert!(install.contains(concat!(
        "SlotInstallOutcomeV1::AlreadyStaged => {\n",
        "                RuntimeRegistryStagedInstallOutcomeV2::ExactReplay\n",
        "            }"
    )));
    assert!(install
        .contains(") -> Result<RuntimeRegistryStagedInstallV2, RuntimeRegistryStagingErrorV2>"));
    assert!(install.contains(concat!(
        "SlotInstallOutcomeV1::AlreadyServing | SlotInstallOutcomeV1::AlreadyDraining => {\n",
        "                return Err(RuntimeRegistryStagingErrorV2::UnexpectedLifecycle);\n",
        "            }"
    )));
    let completion = braced_declaration(registry, "fn complete_staged_install_v2(");
    for required in [
        "staged: RuntimeRegistryStagedRouteV2",
        "witness: Result<SlotRouteWitnessV1, ServingSlotRegistryError>",
        "atomic: Result<Option<SlotAtomicObservationV2>, ServingSlotRegistryError>",
        "registry_observation: Result<RegistryRecoveryObservationV2, ServingSlotRegistryError>",
        "validate_staged_evidence_v2(&staged, witness, atomic, registry_observation)?",
        "authority: staged",
    ] {
        assert!(completion.contains(required), "{required}");
    }
    let validation = braced_declaration(registry, "fn validate_staged_evidence_v2(");
    for required in [
        "witness.identity != staged.identity",
        "witness.fencing_token != token.fencing_token()",
        "witness.lifecycle != SlotLifecycleV1::Staged",
        "atomic.route.as_ref() != Some(&witness)",
        "atomic.admission_state != SlotAdmissionStateV2::Staged",
        "atomic.active_interactions != 0",
        "registry_observation.registry_failed_closed()",
        "registry_observation.failed_closed_slot_count() != 0",
        "registry_observation.staged_route_count() == 0",
        "registry_observation.observation_sequence().get() < atomic.observation_sequence.get()",
        "staged.ensure_staged_v2()?",
        "route: witness",
    ] {
        assert!(validation.contains(required), "{required}");
    }
    for forbidden in ["pub(crate) fn registry", "pub(crate) fn into_registry"] {
        assert!(!port.contains(forbidden), "{forbidden}");
    }

    let staged = braced_declaration(registry, "impl RuntimeRegistryStagedRouteV2");
    let advance = braced_declaration(staged, "pub(crate) fn advance_authority_v2(");
    let advance_registry = advance.find(".advance_authority(").unwrap();
    let retain_successor = advance.find("self.token = Some(successor)").unwrap();
    let refresh_evidence = advance.find("self.observe_staged_evidence_v2()").unwrap();
    assert!(advance_registry < retain_successor && retain_successor < refresh_evidence);
    assert!(advance.contains(
        ") -> Result<RuntimeRegistryStagedInstallEvidenceV2, RuntimeRegistryStagingErrorV2>"
    ));
    let explicit = braced_declaration(
        staged,
        "pub(crate) fn remove_v2(mut self) -> Result<(), RuntimeRegistryStagingErrorV2>",
    );
    assert!(explicit.contains("let result = self.remove_inner_v2()"));
    assert!(explicit.contains("if result.is_err()"));
    assert!(explicit.contains("self.emergency.trip_v2()"));
    let cleanup = braced_declaration(
        staged,
        "fn remove_inner_v2(&mut self) -> Result<(), RuntimeRegistryStagingErrorV2>",
    );
    assert!(cleanup.contains("SlotRemovalOutcomeV1::RemovedStaged => Ok(())"));
    assert!(cleanup.contains("SlotRemovalOutcomeV1::RemovedDraining"));
    assert!(cleanup.contains("RuntimeRegistryStagingErrorV2::UnexpectedLifecycle"));
    let drop_impl = braced_declaration(registry, "impl Drop for RuntimeRegistryStagedRouteV2");
    assert!(drop_impl.contains("self.token.is_some() && self.remove_inner_v2().is_err()"));
    assert!(drop_impl.contains("self.emergency.trip_v2()"));

    let lifecycle = braced_declaration(
        closed_recovery,
        "impl RuntimeClosedRecoverySupervisedServingOpenProcessV2",
    );
    let getter = braced_declaration(
        lifecycle,
        "pub(crate) fn staging_port_v2(&self) -> RuntimeRegistryStagingPortV2",
    );
    assert!(getter.contains("self.registry.staging_port_v2()"));
    assert!(!lifecycle.contains("install_staged_route_v2"));
    for forbidden in [
        "ExactServingRouteV1",
        "SlotMutationTokenV1",
        "ServingSlotRegistryV1",
    ] {
        assert!(!getter.contains(forbidden), "{forbidden}");
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
        "const STARTUP_DISCORD_CLEANUP_RESERVE: Duration = Duration::from_secs(7);",
        "const STARTUP_DATABASE_CLEANUP_RESERVE: Duration = Duration::from_secs(2);",
        "pub(crate) struct RuntimeStartupBudgetV1 {",
        "operation_cutoff: started_at + STARTUP_OPERATION_WINDOW",
        "cleanup_deadline: started_at + STARTUP_TOTAL_WINDOW",
        "pub(crate) fn discord_cleanup_deadline(&self) -> Instant",
        "pub(crate) fn owner_cleanup_deadline(&self) -> Instant",
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
fn process_startup_is_the_single_ordered_bounded_recovery_fixed_point_staging_entry() {
    let sources = source_files();
    let process_startup = sources
        .iter()
        .find(|(path, _)| path == Path::new("src/process_startup.rs"))
        .map(|(_, source)| source.as_str())
        .unwrap();
    let production = source_before_test_module(process_startup);
    let entry = braced_declaration(
        production,
        "pub fn run_runtime_process_staging_from_environment_v1(",
    );
    let staging = braced_declaration(
        production,
        "async fn stage_runtime_process_from_environment_v1(",
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
    let owner = staging.find(".into_owner_held_v1()").unwrap();
    let discord_start = staging
        .find(".begin_paused_discord_connection_v1()")
        .unwrap();
    let paused_observation = staging.find(".wait_for_paused_connected_v1()").unwrap();
    let paused_connected = staging.find(".into_paused_connected_v1(").unwrap();
    let recovery_pending = staging.find(".into_recovery_pending_v2()").unwrap();
    let closed_recovery = staging.find(".into_closed_recovery_v2()").unwrap();
    let recovery_iteration_ready = staging.find(".into_recovery_iteration_ready_v2()").unwrap();
    let startup_recovery = staging
        .find(".into_startup_recovery_fixed_point_v2()")
        .unwrap();
    let paused_production = staging
        .find(".into_paused_production_handoff_v2()")
        .unwrap();
    let process_bound = staging.find(".into_process_bound_handoff_v2()").unwrap();
    let recovery_resume = staging.find(".into_recovery_resume_v2()").unwrap();
    let admission = staging.find(".resume_recovery_v2()").unwrap();
    let empty_open = staging.find(".enter_empty_open_v2()").unwrap();
    let serving_open = staging.find(".enter_serving_open_v2()").unwrap();
    let run = staging.find(".run_until_shutdown_v2()").unwrap();
    let outcome = staging.find("Ok(RuntimeProcessStagingOutcomeV1").unwrap();
    assert!(
        config < secrets
            && secrets < revision
            && revision < foundation
            && foundation < owner
            && owner < discord_start
            && discord_start < paused_observation
            && paused_observation < paused_connected
            && paused_connected < recovery_pending
            && recovery_pending < closed_recovery
            && closed_recovery < recovery_iteration_ready
            && recovery_iteration_ready < startup_recovery
            && startup_recovery < paused_production
            && paused_production < process_bound
            && process_bound < recovery_resume
            && recovery_resume < admission
            && admission < empty_open
            && empty_open < serving_open
            && serving_open < run
            && owner < run
            && run < outcome
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
        ".into_owner_held_v1()",
        ".begin_paused_discord_connection_v1()",
        ".wait_for_paused_connected_v1()",
        ".into_paused_connected_v1(",
        ".into_recovery_pending_v2()",
        ".into_closed_recovery_v2()",
        ".into_recovery_iteration_ready_v2()",
        ".into_startup_recovery_fixed_point_v2()",
        ".into_paused_production_handoff_v2()",
        ".into_process_bound_handoff_v2()",
        ".into_recovery_resume_v2()",
        ".resume_recovery_v2()",
        ".enter_empty_open_v2()",
        ".enter_serving_open_v2()",
        ".run_until_shutdown_v2()",
    ] {
        assert_eq!(staging.matches(operation).count(), 1, "{operation}");
    }
    assert!(!staging.contains("empty_open\n        .shutdown()"));
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
        .find("runtime.block_on(stage_runtime_process_from_environment_v1(")
        .unwrap();
    assert!(
        budget < active_runtime
            && active_runtime < runtime_stage
            && runtime_stage < runtime_builder
            && runtime_builder < block_on
    );
    assert_eq!(
        entry
            .matches("stage_runtime_process_from_environment_v1(")
            .count(),
        1
    );
    for required in [
        "tokio::runtime::Handle::try_current()",
        "tokio::runtime::Builder::new_current_thread()",
        ".enable_all()",
        ".build()",
        "runtime.block_on(stage_runtime_process_from_environment_v1(",
        "RuntimeProcessStagingErrorV1(<redacted>)",
        "RuntimeProcessStagingOutcomeV1(<redacted>)",
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
        "runtime.block_on(stage_runtime_process_from_environment_v1(",
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
        "observe_startup_recovery_v2",
        "begin_startup_recovery_observation_v2",
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
        declaration_attribute_block(production, "RuntimeProcessStagingOutcomeV1");
    let outcome_declaration =
        braced_declaration(production, "pub struct RuntimeProcessStagingOutcomeV1");
    assert!(outcome_declaration.contains("_private: ()"));
    for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
        assert!(
            !contains_identifier(outcome_attributes, forbidden),
            "{forbidden}"
        );
        assert!(!implements_trait(
            production,
            "RuntimeProcessStagingOutcomeV1",
            forbidden,
        ));
    }
    let library = include_str!("../src/lib.rs");
    for required in [
        "run_runtime_process_staging_from_environment_v1",
        "RuntimeProcessStagingErrorV1",
        "RuntimeProcessStagingOutcomeV1",
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
    assert!(main.contains("CleanShutdown"));
    assert!(main.contains("runtime_process_clean_shutdown"));
    assert!(main.contains("Self::CleanShutdown => ExitCode::SUCCESS"));
    assert!(main.contains("Self::Failed(error) if error.cleanup_class()"));
    assert_eq!(
        entry
            .matches("run_runtime_process_staging_from_environment_v1()")
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
fn gateway_owner_staging_is_exact_bounded_opaque_and_non_serving() {
    let acquisition = source_before_test_module(include_str!("../src/gateway_owner_startup.rs"));
    let watchdog = include_str!("../src/gateway_owner_startup_watchdog.rs");
    let owner = source_before_test_module(include_str!("../src/process/owner.rs"));
    let gateway = source_before_test_module(include_str!("../src/gateway.rs"));
    let library = include_str!("../src/lib.rs");

    for required in [
        "pub(crate) struct RuntimeAcquiredGatewayOwnerV1",
        "watchdog: RuntimeGatewayOwnerStartupWatchdogHandleV1,",
        "current_observation: RuntimeGatewayOwnerCurrentObservationV1,",
        "let request = RuntimeAcquireGatewayOwnerLeaseV1 {",
        "gateway_shard_id: runtime_gateway_shard_id_v1(),",
        "process_instance_id: process_instance_id.clone(),",
        "expected_build_revision: build_revision.clone(),",
        "lease_for: config.lease_for(),",
        "accept_gateway_owner_acquire_v1(request, outcome)",
        "accept_gateway_owner_observation_v1(&observation_request, observation)",
        "classify_unknown_gateway_owner_acquire_v1(request, observation)",
        ".start_bounded_gateway_owner_startup_watchdog_v1(",
        "cleanup_deadline,",
        ".observe_current_gateway_owner_v1()",
        "release_runtime_gateway_owner_until_v1(",
        "shutdown_until(cleanup_deadline)",
        "pub(crate) async fn prepare_closed_recovery_in_place_v2(",
        "pub(crate) fn try_into_prepared_closed_recovery_v2(",
        "RuntimeGatewayOwnerStartupAcquisitionErrorV1(<redacted>)",
        "RuntimeAcquiredGatewayOwnerV1(<redacted>)",
    ] {
        assert!(acquisition.contains(required), "{required}");
    }
    assert!(!acquisition.contains("prepare_closed_recovery_observation_v2"));
    assert_eq!(acquisition.matches(".acquire_gateway_owner(").count(), 1);
    assert_eq!(
        acquisition.matches("acquire_gateway_owner_once_v1").count(),
        3
    );
    assert_eq!(
        acquisition
            .matches("acquire_gateway_owner_second_v1")
            .count(),
        3
    );
    assert_eq!(
        acquisition
            .matches("resolve_unknown_gateway_owner_acquire_v1")
            .count(),
        3
    );
    assert_eq!(
        acquisition
            .matches(".start_bounded_gateway_owner_startup_watchdog_v1(")
            .count(),
        1
    );
    assert_eq!(
        acquisition
            .matches(".observe_current_gateway_owner_v1()")
            .count(),
        1
    );
    assert!(!acquisition.contains("\"shard:0\""));
    for forbidden in [
        "loop {",
        "while ",
        "tokio::spawn",
        "spawn_blocking",
        "ready_to_serve",
        "readiness",
        "issue_ready_lease",
        "resume",
        "Discord",
        "twilight",
        "activate",
        "deploy",
        "into_production",
        "promote_to_production_v1",
        "start_gateway_owner_production",
        "observe_paused_connected_gateway_v2",
        "TcpListener",
        ".bind(",
        ".listen(",
        ".serve(",
    ] {
        assert!(!acquisition.contains(forbidden), "{forbidden}");
        assert!(!owner.contains(forbidden), "{forbidden}");
    }
    assert!(!owner.contains("prepare_closed_recovery"));
    let acquired_attributes =
        declaration_attribute_block(acquisition, "RuntimeAcquiredGatewayOwnerV1");
    for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
        assert!(
            !contains_identifier(acquired_attributes, forbidden),
            "{forbidden}"
        );
        assert!(!implements_trait(
            acquisition,
            "RuntimeAcquiredGatewayOwnerV1",
            forbidden,
        ));
    }
    assert!(!acquisition.contains("pub struct RuntimeAcquiredGatewayOwnerV1"));

    for required in [
        "const SUPPORTED_GATEWAY_SHARD_ID: &str = \"shard:0\";",
        "pub(crate) fn runtime_gateway_shard_id_v1() -> GatewayShardIdV1",
        "GatewayShardIdV1::parse(SUPPORTED_GATEWAY_SHARD_ID)",
    ] {
        assert!(gateway.contains(required), "{required}");
    }

    for required in [
        "pub(crate) struct RuntimeOwnerHeldProcessV1",
        "foundation: RuntimeProcessFoundationV1,",
        "owner: RuntimeAcquiredGatewayOwnerV1,",
        "impl RuntimeProcessFoundationV1",
        "pub(crate) async fn into_owner_held_v1(",
        "acquire_runtime_gateway_owner_startup_v1(",
        "self.startup_budget.operation_cutoff()",
        "self.startup_budget.owner_cleanup_deadline()",
        "&mut shutdown,",
        "RuntimeOwnerHeldProcessV1(<redacted>)",
        "RuntimeProcessGatewayOwnerTransitionErrorV1(<redacted>)",
        "RuntimeOwnerHeldProcessShutdownErrorV1(<redacted>)",
    ] {
        assert!(owner.contains(required), "{required}");
    }
    assert_eq!(
        owner
            .matches("acquire_runtime_gateway_owner_startup_v1(")
            .count(),
        1
    );
    assert_eq!(owner.matches("owner_cleanup_deadline()").count(), 2);
    assert!(!owner.contains("root_bounded_startup_window_v1"));
    assert!(!owner.contains("startup_budget.cleanup_deadline()"));
    for required in [
        "shutdown: &mut RuntimeShutdownObserverV1,",
        "observation = shutdown.wait()",
        "cleanup_deadline.min(shutdown_deadline)",
    ] {
        assert!(acquisition.contains(required), "{required}");
    }
    let owner_attributes = declaration_attribute_block(owner, "RuntimeOwnerHeldProcessV1");
    for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
        assert!(
            !contains_identifier(owner_attributes, forbidden),
            "{forbidden}"
        );
        assert!(!implements_trait(
            owner,
            "RuntimeOwnerHeldProcessV1",
            forbidden,
        ));
    }
    let shutdown = braced_declaration(
        owner,
        "pub(crate) async fn shutdown(self) -> Result<(), RuntimeOwnerHeldProcessShutdownErrorV1>",
    );
    let finalizer = shutdown.find(".begin_shutdown_v1(").unwrap();
    let registry = shutdown
        .find("foundation.observe_shutdown_registry_v1()")
        .unwrap();
    let owner_release = shutdown
        .find("owner.shutdown_until(owner_cleanup_deadline)")
        .unwrap();
    let foundation_finish = shutdown.find("foundation.finish_shutdown_v1(").unwrap();
    assert!(finalizer < registry && registry < owner_release && owner_release < foundation_finish);
    assert!(!owner.contains("pub struct RuntimeOwnerHeldProcessV1"));
    assert!(!library.contains("RuntimeOwnerHeldProcessV1"));

    for required in [
        "deadline_cap: Option<Instant>,",
        ".map_or(relative, |cap| relative.min(cap))",
        "fn capped_at(mut self, deadline_cap: Instant) -> Self",
        "pub(crate) async fn cleanup_until(",
        "pub(crate) async fn shutdown_until(",
        "pub(crate) async fn release_runtime_gateway_owner_until_v1",
        "RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1::DeadlineElapsed",
        "const STARTUP_ACTOR_TERMINATION_RESERVE:",
        "const STARTUP_TASK_ABORT_RESERVE:",
        "task: Option<tokio::task::JoinHandle<()>>,",
        "let task = runtime.spawn(async move",
        "self.join_task_until(cleanup_deadline)",
        "self.abort_and_join_task().await",
        "cleanup_deadline: Option<Instant>,",
        "self.request_shutdown(Some(actor_cleanup_deadline))",
        "cleanup = cleanup.capped_at(cleanup_deadline);",
        "struct RuntimeGatewayOwnerStartupCleanupCapV1",
        "initial_startup_cleanup_deadline: Option<Instant>,",
        "RuntimeGatewayOwnerStartupCleanupCapV1::new(initial_startup_cleanup_deadline)",
        "startup_cleanup_cap.limit(stop.cleanup_deadline)",
    ] {
        assert!(watchdog.contains(required), "{required}");
    }
    let stop = braced_declaration(watchdog, "impl RuntimeGatewayOwnerStartupWatchdogStopV1");
    assert!(stop.contains("cleanup_deadline: cleanup.deadline()"));
    assert_eq!(
        watchdog
            .matches("startup_cleanup_cap.limit(stop.cleanup_deadline)")
            .count(),
        3
    );
    assert!(!watchdog.contains("promote_to_production_v1"));
    assert!(!watchdog.contains("clear_startup_cleanup_deadline"));
    let watchdog_start = braced_declaration(
        watchdog,
        "pub(crate) fn start_runtime_gateway_owner_startup_watchdog_v1",
    );
    let initialized_cap = watchdog_start
        .find("RuntimeGatewayOwnerStartupCleanupCapV1::new(initial_startup_cleanup_deadline)")
        .unwrap();
    let actor_spawn = watchdog_start
        .find("let task = runtime.spawn(async move")
        .unwrap();
    assert!(initialized_cap < actor_spawn);
    assert!(!watchdog.contains("with_startup_cleanup_deadline"));
    assert!(!watchdog.contains("fn install_startup_cleanup_deadline"));
    assert!(!watchdog.contains("cleanup_timeout"));
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
        "pub(crate) async fn shutdown(",
        "pub(super) async fn begin_shutdown_v1(",
        "pub(super) async fn finish_shutdown_v1(",
        "RuntimeProcessRootSupervisorV1",
        "mutation_finalizer:",
        "shutdown_health_until(cleanup_deadline)",
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
    let shutdown = braced_declaration(production, "pub(crate) async fn shutdown(");
    let begin = shutdown
        .find(".begin_shutdown_v1(RuntimeShutdownCauseV1::Explicit)")
        .unwrap();
    let registry = shutdown
        .find("self.observe_shutdown_registry_v1()")
        .unwrap();
    let finish = shutdown
        .find("self.finish_shutdown_v1(cleanup_deadline)")
        .unwrap();
    assert!(begin < registry && registry < finish);
    let shutdown = braced_declaration(production, "pub(super) async fn finish_shutdown_v1(");
    let finalizer = shutdown
        .find("mutation_finalizer.shutdown_until(cleanup_deadline)")
        .unwrap();
    let signal = shutdown
        .find("root_supervisor.join_signal_until(cleanup_deadline)")
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
    let health = shutdown
        .find(".shutdown_health_until(cleanup_deadline)")
        .unwrap();
    assert!(
        finalizer < signal
            && signal < shutdown_handle
            && shutdown_handle < closed_components
            && closed_components < close
            && close < release_handle
            && release_handle < finish
            && finish < health
    );
    assert!(shutdown.contains("shutdown.close_until(cleanup_deadline).await"));
    assert!(!shutdown.contains("shutdown.close().await"));
    assert!(!composer.contains("shutdown.close().await"));
    let finish_shutdown = braced_declaration(
        production,
        "fn finish_runtime_process_foundation_shutdown_v1<S, R, T>(",
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
