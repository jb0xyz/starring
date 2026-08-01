use std::collections::BTreeMap;

use automation_runtime_interaction_postgres::MIGRATOR;
use sha2::{Digest, Sha256};

const BACKFILL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ops/postgres/staging-runtime-interaction-effect-acl-backfill.sql"
));
const ROLE_BOOTSTRAP: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ops/postgres/staging-runtime-role-bootstrap.sql"
));
const EFFECT_MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/202608010001_add_runtime_interaction_effect_journal_v1.sql"
));

fn section<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start = source.find(start).unwrap();
    let remaining = &source[start..];
    let end = remaining.find(end).unwrap();
    &remaining[..end]
}

fn position(source: &str, needle: &str) -> usize {
    source.find(needle).unwrap()
}

fn backfill_manifest() -> BTreeMap<String, bool> {
    section(
        BACKFILL,
        "INSERT INTO pg_temp.starring_runtime_interaction_effect_acl_manifest",
        "CREATE TEMP TABLE starring_runtime_interaction_effect_role_snapshot",
    )
    .lines()
    .filter_map(|line| {
        let row = line.trim();
        if !row.starts_with("('public.") {
            return None;
        }
        let row = row.strip_prefix("('").unwrap();
        let (identity, executable) = row.split_once("', ").unwrap();
        let executable = executable
            .strip_suffix("),")
            .or_else(|| executable.strip_suffix(");"))
            .unwrap();
        Some((identity.to_owned(), executable == "TRUE"))
    })
    .collect()
}

fn bootstrap_exported_effects() -> BTreeMap<String, bool> {
    section(
        ROLE_BOOTSTRAP,
        "INSERT INTO pg_temp.starring_runtime_capability_functions",
        "SELECT pg_catalog.pg_advisory_lock",
    )
    .lines()
    .filter_map(|line| {
        let row = line.trim();
        let prefix = "('interaction', '";
        if !row.starts_with(prefix) || !row.contains("public.starring_runtime_interaction_effect_")
        {
            return None;
        }
        let identity = row
            .strip_prefix(prefix)
            .unwrap()
            .strip_suffix("'),")
            .or_else(|| row.strip_prefix(prefix).unwrap().strip_suffix("');"))
            .unwrap();
        Some((identity.to_owned(), true))
    })
    .collect()
}

fn checksum_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn migration_ledger_digest() -> String {
    let projection = MIGRATOR
        .iter()
        .map(|migration| {
            format!(
                "{}:true:{}",
                migration.version,
                checksum_hex(migration.checksum.as_ref())
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    checksum_hex(Sha256::digest(projection.as_bytes()).as_ref())
}

#[test]
fn effect_acl_backfill_manifest_matches_the_runtime_capability_boundary() {
    let manifest = backfill_manifest();
    assert_eq!(manifest.len(), 22);
    assert_eq!(manifest.values().filter(|value| **value).count(), 11);
    assert_eq!(
        manifest
            .iter()
            .filter(|(_, executable)| **executable)
            .map(|(identity, _)| (identity.clone(), true))
            .collect::<BTreeMap<_, _>>(),
        bootstrap_exported_effects()
    );

    for identity in manifest.keys() {
        let function_name = identity.split_once('(').unwrap().0;
        assert!(
            EFFECT_MIGRATION.contains(&format!("FUNCTION {function_name}(")),
            "missing effect migration function: {identity}"
        );
    }

    for internal in [
        "public.guard_runtime_interaction_effect_event_v1()",
        "public.guard_runtime_interaction_effect_head_v1()",
        "public.guard_runtime_interaction_effect_response_token_delete_v1()",
        "public.guard_runtime_interaction_effect_rollback_v1()",
        "public.guard_runtime_interaction_effect_root_v1()",
        "public.starring_runtime_interaction_effect_complete_receipt_v1(text,text,text,bytea,timestamp with time zone)",
        "public.starring_runtime_interaction_effect_receipt_terminal_sync_v1()",
        "public.starring_runtime_interaction_effect_require_rollback_v1(text,text,text,timestamp with time zone)",
        "public.starring_runtime_interaction_effect_resolve_receipt_v1(text,text,bytea,boolean)",
        "public.starring_runtime_interaction_effect_schema_manifest_v1()",
        "public.starring_runtime_interaction_effect_try_complete_rollback_v1(text,text,timestamp with time zone)",
    ] {
        assert_eq!(manifest.get(internal), Some(&false));
    }
}

#[test]
fn effect_acl_backfill_pins_target_ledger_identity_and_quiescence() {
    let migrations = MIGRATOR.iter().collect::<Vec<_>>();
    assert_eq!(migrations.len(), 116);
    let latest = migrations.last().unwrap();
    assert_eq!(latest.version, 202_608_010_001);
    let latest_checksum = checksum_hex(latest.checksum.as_ref());
    assert_eq!(
        latest_checksum,
        "69bc36f83f1e6b575205ca67703639124e41abd9d19b1b701d845ca150dcc4d6202608176b0583c864b4663937bd89e7"
    );
    assert_eq!(
        migration_ledger_digest(),
        "33ac781065168bb63b3ac4646f3590cbcb54ee0466654a5112d799c86a30bf44"
    );

    for required in [
        "\\set ON_ERROR_STOP on",
        ":{?expected_database}",
        ":{?expected_system_identifier}",
        ":{?runtime_dedicated_cluster_acknowledgement}",
        "starring_runtime_staging",
        "starring-runtime-dedicated-staging-cluster-v2:%s:starring_runtime_staging:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation",
        "NOT BETWEEN 160000 AND 169999",
        "current_user <> 'starring_cluster_admin'",
        "pg_catalog.pg_advisory_xact_lock",
        "activity.backend_type = 'client backend'",
        "pg_catalog.pg_prepared_xacts",
        "ledger_count <> 116",
        "33ac781065168bb63b3ac4646f3590cbcb54ee0466654a5112d799c86a30bf44",
        "migration.version = 202608010001",
        "69bc36f83f1e6b575205ca67703639124e41abd9d19b1b701d845ca150dcc4d6202608176b0583c864b4663937bd89e7",
    ] {
        assert!(BACKFILL.contains(required), "missing contract: {required}");
    }
}

#[test]
fn effect_acl_backfill_is_credential_preserving_and_acl_only() {
    for required in [
        "role.rolpassword",
        "role.rolvaliduntil",
        "CREATE TEMP TABLE starring_runtime_interaction_effect_role_setting_snapshot",
        "setting.setdatabase",
        "setting.setrole",
        "setting.setconfig",
        "pg_catalog.pg_db_role_setting AS setting",
        "EXCEPT ALL",
        "effect ACL backfill changed role authentication state",
        "REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE",
        "REVOKE ALL PRIVILEGES ON FUNCTION %s FROM %I CASCADE",
        "GRANT EXECUTE ON FUNCTION %s TO %I",
        "GRANT EXECUTE ON FUNCTION %s TO starring_runtime_interaction",
        "EXECUTE WITH GRANT OPTION",
        "SET SESSION AUTHORIZATION starring_runtime_interaction",
        "starring_runtime_interaction_database_readiness_v1()",
        "RESET SESSION AUTHORIZATION",
    ] {
        assert!(BACKFILL.contains(required), "missing contract: {required}");
    }
    assert!(!BACKFILL.contains("role.rolconfig"));
    assert_eq!(
        BACKFILL
            .matches("FROM pg_temp.starring_runtime_interaction_effect_role_setting_snapshot",)
            .count(),
        4
    );
    assert!(
        BACKFILL
            .matches("FROM pg_catalog.pg_db_role_setting AS setting")
            .count()
            >= 6
    );
    let postflight = position(BACKFILL, "DO $postflight$");
    let final_role_proof = position(BACKFILL, "DO $final_role_proof$");
    let drop_snapshots = position(
        BACKFILL,
        "DROP TABLE pg_temp.starring_runtime_interaction_effect_acl_manifest",
    );
    let session_authorization = position(
        BACKFILL,
        "SET SESSION AUTHORIZATION starring_runtime_interaction",
    );
    let readiness = position(
        BACKFILL,
        "FROM public.starring_runtime_interaction_database_readiness_v1() AS readiness",
    );
    let reset_authorization = position(BACKFILL, "RESET SESSION AUTHORIZATION");
    let final_quiescence = position(BACKFILL, "DO $final_quiescence_proof$");
    let commit = position(BACKFILL, "COMMIT;");
    assert!(postflight < final_role_proof);
    assert!(final_role_proof < drop_snapshots);
    assert!(drop_snapshots < session_authorization);
    assert!(session_authorization < readiness);
    assert!(readiness < reset_authorization);
    assert!(reset_authorization < final_quiescence);
    assert!(final_quiescence < commit);

    let drop_section = section(BACKFILL, "DROP TABLE", "SET SESSION AUTHORIZATION");
    for snapshot in [
        "pg_temp.starring_runtime_interaction_effect_acl_manifest",
        "pg_temp.starring_runtime_interaction_effect_role_snapshot",
        "pg_temp.starring_runtime_interaction_effect_role_setting_snapshot",
    ] {
        assert_eq!(drop_section.matches(snapshot).count(), 1);
    }

    let readiness_section = section(
        BACKFILL,
        "SET SESSION AUTHORIZATION starring_runtime_interaction",
        "RESET SESSION AUTHORIZATION",
    );
    for forbidden in [
        "ALTER ROLE",
        "GRANT ",
        "REVOKE ",
        "CREATE ",
        "DROP ",
        "pg_authid",
        "pg_db_role_setting",
    ] {
        assert!(!readiness_section.contains(forbidden));
    }

    let upper = BACKFILL.to_ascii_uppercase();
    for forbidden in [
        "ALTER ROLE",
        "CREATE ROLE",
        "DROP ROLE",
        "ALTER FUNCTION",
        "CREATE FUNCTION",
        "DROP FUNCTION",
        "INSERT INTO PUBLIC.",
        "UPDATE PUBLIC.",
        "DELETE FROM PUBLIC.",
        "TRUNCATE PUBLIC.",
        "PASSWORD '",
        "POSTGRES://",
        "POSTGRESQL://",
        "CFAT_",
        "CFUT_",
    ] {
        assert!(
            !upper.contains(forbidden),
            "forbidden mutation: {forbidden}"
        );
    }
    assert!(!BACKFILL.contains("/*"));
    assert!(!BACKFILL.contains("*/"));
    assert!(!BACKFILL
        .lines()
        .any(|line| line.trim_start().starts_with("--")));
}
