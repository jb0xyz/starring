const MIGRATION: &str = include_str!(
    "../../../migrations/202607270007_establish_runtime_startup_recovery_action_journal_v2.sql"
);
const PREVIOUS_MIGRATION: &str = include_str!(
    "../../../migrations/202607270006_normalize_startup_recovery_owner_projection_v2.sql"
);
const CONTRACT_SOURCE: &str = include_str!("../src/contract.rs");
const DATABASE_SOURCE: &str = include_str!("../src/database.rs");
const SECURITY_SUPPORT_SOURCE: &str = include_str!("postgres_security/support.rs");

const PREVIOUS_MANIFEST_DEFINITION_DIGEST: &str =
    "2e55bd05bb77a1dcc5a4f02efd0b221f2fa085fb92e7da7f97d29408022f0eb3";
const CURRENT_MANIFEST_CONTENT_DIGEST: &str =
    "e9fbf54f755c1a5ac234c69eea4252361146b69c032b655270e7306ea929e175";
const CURRENT_MANIFEST_DEFINITION_DIGEST: &str =
    "c76a82cdd88a75259889d4cab4543797ad834d8f2e38f71268bbbc4b0e4cae0f";
const PREVIOUS_READINESS_DIGEST: &str =
    "9acd85e2162d4c06593dedae7d2043e53bebc8cd1d70c7aea5aa364cec0cb27f";
const CURRENT_READINESS_DIGEST: &str =
    "ee9364b3bb8b17a3a2386c0be06ae2ab12b519c77647a4073e96f45bfb5084a8";
const LATEST_READINESS_DIGEST: &str =
    "059ee21b16b325a4da71dda5d63f75c8aeac4d0e2d9b18cbb3f628d15ea8967d";
const DIGEST_IDENTITY: &str =
    "starring_runtime_private_v2.starring_runtime_startup_recovery_terminal_digest_v2(smallint,text,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,timestamp with time zone,bytea)";
const RECORD_IDENTITY: &str =
    "starring_runtime_private_v2.starring_runtime_startup_recovery_action_record_v2(text,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,bytea)";

fn dollar_block(tag: &str) -> &'static str {
    MIGRATION
        .split(&format!("DO ${tag}$"))
        .nth(1)
        .unwrap()
        .split(&format!("${tag}$;"))
        .next()
        .unwrap()
}

fn function_body(identity: &str) -> &'static str {
    MIGRATION
        .split(&format!("CREATE FUNCTION {identity}"))
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap()
        .split("AS $function$")
        .nth(1)
        .unwrap()
}

#[test]
fn migration_is_additive_serialized_quiescent_and_comment_free() {
    let preflight = dollar_block("preflight");
    assert!(!PREVIOUS_MIGRATION.contains(CURRENT_MANIFEST_DEFINITION_DIGEST));
    assert!(!PREVIOUS_MIGRATION.contains("runtime_startup_recovery_actions_v2"));
    assert!(MIGRATION.starts_with("SET LOCAL lock_timeout = '5s';"));
    assert!(MIGRATION.contains("pg_catalog.pg_advisory_xact_lock("));
    assert!(MIGRATION.contains("'starring-runtime-writer-fence-v1'"));
    assert!(MIGRATION.contains("IN ACCESS EXCLUSIVE MODE;"));
    for required in [
        "executor_role_is_quarantined",
        "executor_membership_count <> 0",
        "other_client_session_count <> 0",
        "prepared_transaction_count <> 0",
        "runtime_startup_recovery_action_journal_preflight_drift",
        PREVIOUS_MANIFEST_DEFINITION_DIGEST,
        PREVIOUS_READINESS_DIGEST,
    ] {
        assert!(preflight.contains(required), "{required}");
    }
    assert!(!MIGRATION.contains("\nGRANT "));
    assert!(!MIGRATION.contains("CREATE OR REPLACE"));
    assert!(!MIGRATION.contains("DROP "));
    for line in MIGRATION.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"));
        assert!(!trimmed.starts_with("//"));
        assert!(!trimmed.starts_with("/*"));
    }
    assert!(MIGRATION.ends_with("RESET search_path;\n"));
}

#[test]
fn journal_relation_is_append_only_and_exactly_keyed() {
    for required in [
        "CREATE TABLE public.runtime_startup_recovery_actions_v2",
        "record_format_version SMALLINT NOT NULL",
        "recovery_id TEXT NOT NULL",
        "originating_emergency_generation BIGINT NOT NULL",
        "coordinator_generation BIGINT NOT NULL",
        "action_authority_revision BIGINT NOT NULL",
        "selection_authority_revision BIGINT NOT NULL",
        "terminal_projection_bytes BYTEA NOT NULL",
        "terminal_digest TEXT NOT NULL",
        "recorded_at TIMESTAMPTZ NOT NULL",
        "PRIMARY KEY (\n        recovery_id,\n        selection_authority_revision",
        "UNIQUE (\n        recovery_id,\n        action_authority_revision",
        "action_authority_revision::NUMERIC\n            = selection_authority_revision::NUMERIC + 1",
        "pg_catalog.isfinite(owner_expires_at)",
        "pg_catalog.isfinite(minimum_database_now)",
        "pg_catalog.isfinite(recorded_at)",
        "minimum_database_now <= recorded_at",
        "recorded_at < owner_expires_at",
        "runtime_startup_recovery_actions_v2_owner_history_index",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    for recovery_class in [
        "stale_live",
        "reserved_awaiting_certification",
        "suspended_local_effect",
        "pending_runtime_drain_intent",
    ] {
        assert!(MIGRATION.contains(recovery_class), "{recovery_class}");
    }
    assert!(MIGRATION.contains("TG_OP <> 'INSERT'"));
    assert!(MIGRATION.contains("BEFORE INSERT OR UPDATE OR DELETE"));
    assert!(MIGRATION.contains("BEFORE TRUNCATE"));
    assert!(MIGRATION.contains("runtime_startup_recovery_action_mutation_rejected"));
}

#[test]
fn digest_binds_the_complete_persisted_action_proof() {
    let body = function_body(
        "starring_runtime_private_v2.starring_runtime_startup_recovery_terminal_digest_v2(",
    );
    for required in [
        "pg_catalog.int2send(requested_record_format_version)",
        "pg_catalog.octet_length(recovery_id_bytes)",
        "requested_originating_emergency_generation",
        "requested_coordinator_generation",
        "requested_action_authority_revision",
        "requested_selection_authority_revision",
        "pg_catalog.octet_length(recovery_class_bytes)",
        "pg_catalog.octet_length(gateway_shard_id_bytes)",
        "pg_catalog.octet_length(\n                owner_process_instance_id_bytes",
        "requested_owner_lease_epoch",
        "owner_runtime_build_revision_bytes",
        "requested_owner_revision",
        "pg_catalog.timestamptz_send(requested_owner_expires_at)",
        "pg_catalog.timestamptz_send(requested_minimum_database_now)",
        "pg_catalog.timestamptz_send(requested_recorded_at)",
        "pg_catalog.octet_length(terminal_projection_bytes)",
        "'starring.runtime.startup_recovery.action_proof.v2'",
        "pg_catalog.decode('00', 'hex')",
    ] {
        assert!(body.contains(required), "{required}");
    }
    let terminal_check = MIGRATION
        .split("CONSTRAINT runtime_startup_recovery_actions_v2_terminal_check CHECK (")
        .nth(1)
        .unwrap()
        .split("\n    )\n);")
        .next()
        .unwrap();
    for required in [
        "record_format_version",
        "recovery_id",
        "originating_emergency_generation",
        "coordinator_generation",
        "action_authority_revision",
        "selection_authority_revision",
        "recovery_class",
        "gateway_shard_id",
        "owner_process_instance_id",
        "owner_lease_epoch",
        "owner_runtime_build_revision",
        "owner_revision",
        "owner_expires_at",
        "minimum_database_now",
        "recorded_at",
        "terminal_projection_bytes",
    ] {
        assert!(terminal_check.contains(required), "{required}");
    }
    assert!(MIGRATION.contains(DIGEST_IDENTITY));
    assert!(MIGRATION.contains("ce512e6b57535d4bc45d7b7c7b056905be5775e418987d2ef79f62b8c05feb41"));
}

#[test]
fn private_record_helper_is_serializable_exact_and_lock_ordered() {
    let body = function_body(
        "starring_runtime_private_v2.starring_runtime_startup_recovery_action_record_v2(",
    );
    let writer = body
        .find("pg_catalog.pg_advisory_xact_lock_shared(")
        .unwrap();
    let owner = body.find("'starring-runtime-gateway-owner-v1:'").unwrap();
    let recovery = body
        .find("'starring-runtime-startup-recovery-action-v2:'")
        .unwrap();
    let owner_row = body.find("FROM public.runtime_gateway_owners").unwrap();
    let journal_row = body
        .find("FROM public.runtime_startup_recovery_actions_v2")
        .unwrap();
    assert!(writer < owner);
    assert!(owner < recovery);
    assert!(recovery < owner_row);
    assert!(owner_row < journal_row);
    for required in [
        "transaction_isolation",
        "<> 'serializable'",
        "transaction_read_only",
        "runtime_startup_recovery_action_transaction_invalid",
        "runtime_startup_recovery_action_input_invalid",
        "requested_selection_authority_revision\n            NOT BETWEEN 1 AND 9223372036854775806",
        "requested_action_authority_revision\n            NOT BETWEEN 2 AND 9223372036854775807",
        "runtime_startup_recovery_action_owner_lost",
        "runtime_startup_recovery_action_replay_mismatch",
        "runtime_startup_recovery_action_identity_conflict",
        "runtime_startup_recovery_action_sequence_conflict",
        "IF database_now < existing_row.recorded_at THEN",
        "database_now < latest_row.recorded_at",
        "minimum_database_now < latest_row.recorded_at",
        "expected_owner_revision = latest_row.owner_revision",
        "IS DISTINCT FROM latest_row.owner_expires_at",
        "outcome_name := 'replayed';",
        "outcome_name := 'applied';",
    ] {
        assert!(body.contains(required), "{required}");
    }
    assert_eq!(body.matches("pg_catalog.clock_timestamp()").count(), 1);
    assert!(MIGRATION.contains(RECORD_IDENTITY));
    assert!(MIGRATION.contains("bead9e18b19984a20070ee4b739f0fa7aaebb87d07a03913af17dd8b4b5b24b4"));
}

#[test]
fn guc_gate_is_integrity_only_and_every_path_clears_it() {
    let trigger = function_body("public.reject_runtime_startup_recovery_action_mutation_v2(");
    let record = function_body(
        "starring_runtime_private_v2.starring_runtime_startup_recovery_action_record_v2(",
    );
    let settings = [
        "starring.runtime_startup_recovery_action_gate_v2",
        "starring.runtime_startup_recovery_action_format_v2",
        "starring.runtime_startup_recovery_action_recovery_id_v2",
        "starring.runtime_startup_recovery_action_origin_generation_v2",
        "starring.runtime_startup_recovery_action_coordinator_generation_v2",
        "starring.runtime_startup_recovery_action_authority_revision_v2",
        "starring.runtime_startup_recovery_action_selection_revision_v2",
        "starring.runtime_startup_recovery_action_class_v2",
        "starring.runtime_startup_recovery_action_gateway_shard_v2",
        "starring.runtime_startup_recovery_action_owner_process_v2",
        "starring.runtime_startup_recovery_action_owner_lease_epoch_v2",
        "starring.runtime_startup_recovery_action_owner_build_v2",
        "starring.runtime_startup_recovery_action_owner_revision_v2",
        "starring.runtime_startup_recovery_action_owner_expires_v2",
        "starring.runtime_startup_recovery_action_minimum_database_now_v2",
        "starring.runtime_startup_recovery_action_terminal_digest_v2",
        "starring.runtime_startup_recovery_action_recorded_at_v2",
    ];
    for setting in settings {
        assert!(trigger.contains(setting), "{setting}");
        assert!(record.contains(setting), "{setting}");
    }
    assert!(trigger.contains("PERFORM pg_catalog.set_config(setting_name, '', TRUE);"));
    assert!(record.contains("PERFORM pg_catalog.set_config(setting_name, '', TRUE);"));
    assert!(MIGRATION.contains("REVOKE ALL ON TABLE"));
    assert!(MIGRATION.contains("REVOKE ALL ON FUNCTION"));
    assert!(!MIGRATION.contains("TO starring_runtime_execution"));
}

#[test]
fn manifest_readiness_acl_and_rust_pins_advance_together() {
    let manifest = dollar_block("patch_schema_manifest");
    let readiness = dollar_block("patch_readiness");
    let postflight = dollar_block("postflight");
    for required in [
        "RETURN observed_count = 768",
        CURRENT_MANIFEST_CONTENT_DIGEST,
        CURRENT_MANIFEST_DEFINITION_DIGEST,
        CURRENT_READINESS_DIGEST,
        DIGEST_IDENTITY,
        RECORD_IDENTITY,
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert!(manifest.contains("runtime_startup_recovery_actions_v2"));
    assert!(readiness.contains("reject_runtime_startup_recovery_action_mutation_v2"));
    assert!(postflight.contains("invalid_relation_count <> 0"));
    assert!(postflight.contains("invalid_function_count <> 0"));
    assert!(postflight.contains("invalid_acl_count <> 0"));
    assert!(postflight.contains("invalid_trigger_count <> 0"));
    assert!(postflight.contains("invalid_constraint_count <> 0"));
    assert!(postflight.contains("invalid_index_count <> 0"));
    assert!(postflight.contains("runtime_startup_recovery_action_journal_gate_drift"));
    for source in [CONTRACT_SOURCE, DATABASE_SOURCE, SECURITY_SUPPORT_SOURCE] {
        assert!(source.contains(LATEST_READINESS_DIGEST));
    }
    assert!(!CONTRACT_SOURCE.contains(PREVIOUS_READINESS_DIGEST));
}
