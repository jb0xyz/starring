const MIGRATION: &str = include_str!(
    "../../../migrations/202607270011_add_owner_fenced_startup_suspended_local_execution_v2.sql"
);
const PREVIOUS_MIGRATION: &str = include_str!(
    "../../../migrations/202607270010_add_owner_fenced_startup_reserved_awaiting_execution_v2.sql"
);
const CONTRACT_SOURCE: &str = include_str!("../src/contract.rs");
const DATABASE_SOURCE: &str = include_str!("../src/database.rs");
const SECURITY_SUPPORT_SOURCE: &str = include_str!("postgres_security/support.rs");

const EXECUTOR: &str = "public.starring_runtime_startup_recovery_execute_suspended_local_v2";
const FUNCTION_IDENTITY: &str =
    "public.starring_runtime_startup_recovery_execute_suspended_local_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint)";

fn dollar_block(tag: &str) -> &'static str {
    MIGRATION
        .split(&format!("DO ${tag}$"))
        .nth(1)
        .unwrap()
        .split(&format!("${tag}$;"))
        .next()
        .unwrap()
}

fn executor() -> &'static str {
    MIGRATION
        .split(&format!("CREATE FUNCTION {EXECUTOR}("))
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap()
}

#[test]
fn migration_is_quiescent_collision_safe_and_comment_free() {
    let preflight = dollar_block("preflight");
    assert!(MIGRATION.starts_with("SET LOCAL lock_timeout = '5s';"));
    assert!(MIGRATION.ends_with("RESET search_path;\n"));
    assert!(MIGRATION.contains("IN ACCESS EXCLUSIVE MODE;"));
    for required in [
        "other_client_session_count",
        "prepared_transaction_count",
        "executor_membership_count",
        "starring_runtime_suspended_root_exact_v2",
        "starring_runtime_suspended_quiescent_exact_v2",
        "runtime_startup_suspended_local_preflight_drift",
    ] {
        assert!(preflight.contains(required), "{required}");
    }
    assert!(!PREVIOUS_MIGRATION.contains(FUNCTION_IDENTITY));
    for line in MIGRATION.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"));
        assert!(!trimmed.starts_with("//"));
        assert!(!trimmed.starts_with("/*"));
    }
}

#[test]
fn executor_is_serializable_owner_writer_and_closed_evidence_fenced() {
    let function = executor();
    for required in [
        "transaction_isolation",
        "<> 'serializable'",
        "transaction_read_only",
        "starring-runtime-writer-fence-v1",
        "writer_fence_count",
        "starring-runtime-gateway-owner-v1:",
        "starring-runtime-startup-recovery-action-v2:",
        "FOR UPDATE",
        "runtime_startup_suspended_local_owner_lost",
        "fence.fence_state = 'open'",
        "runtime_startup_suspended_local_state_ambiguous",
        "paused_process_instance_id",
        "paused_last_resume_sequence",
        "registry_retained_empty_tombstone_count",
        "paused_ready_kind NOT IN ('ready', 'resumed')",
        "paused_coordinator_generation\n            IS DISTINCT FROM requested_originating_emergency_generation",
    ] {
        assert!(function.contains(required), "{required}");
    }
    assert!(!function.contains(
        "paused_coordinator_generation\n            IS DISTINCT FROM requested_coordinator_generation"
    ));
    assert!(!function.contains("SKIP LOCKED"));
    let writer = function.find("starring-runtime-writer-fence-v1").unwrap();
    let owner = function.find("starring-runtime-gateway-owner-v1:").unwrap();
    let action = function
        .find("starring-runtime-startup-recovery-action-v2:")
        .unwrap();
    assert!(writer < owner && owner < action);
}

#[test]
fn classifier_rejects_corruption_and_honors_priority_before_no_candidate() {
    let function = executor();
    for required in [
        "higher_live_count",
        "higher_reservation_count",
        "runtime_startup_suspended_local_higher_priority",
        "invalid_ledger_count",
        "invalid_exact_count",
        "starring_runtime_suspended_root_exact_v2",
        "starring_runtime_suspended_quiescent_exact_v2",
        "starring_runtime_suspended_terminal_sidecar_v2",
        "runtime_startup_suspended_local_state_ambiguous",
        "ORDER BY",
        "suspended.suspended_at",
        "suspended.suspension_id COLLATE pg_catalog.\"C\"",
        "same_slot_drain_count",
        "runtime_startup_suspended_local_pending_drain",
    ] {
        assert!(function.contains(required), "{required}");
    }
    let invalid = function.find("IF invalid_ledger_count <> 0").unwrap();
    let candidate = function.find("IF exact_route_count = 0 THEN").unwrap();
    let journal = function[candidate..]
        .find("starring_runtime_startup_recovery_action_record_v2")
        .unwrap()
        + candidate;
    assert!(invalid < candidate && candidate < journal);
}

#[test]
fn deployment_witness_reconstructs_the_full_suspension_observation() {
    let function = executor();
    for required in [
        "deployment_row.convergence_attempt_no",
        "deployment_row.last_controller_id",
        "deployment_row.last_fencing_token",
        "deployment_row.runtime_generation",
        "deployment_row.target_version",
        "deployment_row.target_content_hash",
        "deployment_row.binding_revision",
        "deployment_row.binding_fingerprint",
        "#>> '{phase,condition,condition}' = 'ready'",
        "deployment_row.snapshot -> 'previous_runtime'",
        "failure_recorded_numeric",
        "source_evidence_at",
        "deployment_row.snapshot_format_version",
        "deployment_row.snapshot::TEXT",
        "starring_runtime_suspended_root_frame_v2",
    ] {
        assert!(
            function.contains(required) || MIGRATION.contains(required),
            "{required}"
        );
    }
    assert!(MIGRATION.contains("NOT BETWEEN 1 AND 1048576"));
}

#[test]
fn sidecar_transition_is_one_step_cas_without_deployment_or_slot_epoch_mutation() {
    let function = executor();
    for required in [
        "successor_sidecar_row.sidecar_revision :=",
        "source_sidecar_row.sidecar_revision + 1",
        "successor_sidecar_row.local_effect_kind := 'route_absent'",
        "successor_sidecar_row.drain_obligation_kind := 'none'",
        "successor_sidecar_row.drain_obligation_kind :=",
        "'previous_serving'",
        "UPDATE public.runtime_suspended_attempts_v2 AS suspended",
        "AND suspended.sidecar_revision =",
        "source_sidecar_row.sidecar_revision",
        "RETURNING suspended.*",
        "INTO updated_sidecar_row",
        "IF NOT FOUND",
        "updated_sidecar_row IS DISTINCT FROM successor_sidecar_row",
        "starring_runtime_startup_recovery_action_record_v2",
    ] {
        assert!(function.contains(required), "{required}");
    }
    assert!(!function.contains("UPDATE public.runtime_deployments"));
    assert!(!function.contains("starring_runtime_slot_writer_fence_begin_unsafe_v2"));
    let mutation = function
        .find("UPDATE public.runtime_suspended_attempts_v2 AS suspended")
        .unwrap();
    let journal = function
        .rfind("starring_runtime_startup_recovery_action_record_v2")
        .unwrap();
    assert!(mutation < journal);
}

#[test]
fn projection_replay_and_acl_are_exact_and_fail_closed() {
    for required in [
        "starring_runtime_suspended_projection_exact_v2",
        "starring_runtime_suspended_replay_exact_v2",
        "starring_runtime_suspended_terminal_sidecar_v2",
        "pg_catalog.sha256(",
        "NOT BETWEEN 1 AND 1048576",
        "WHEN OTHERS THEN",
        "RETURN FALSE",
        "REVOKE ALL ON FUNCTION",
        "GRANT EXECUTE ON FUNCTION public.starring_runtime_startup_recovery_execute_suspended_local_v2",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert_eq!(MIGRATION.matches("GRANT EXECUTE ON FUNCTION").count(), 1);
    for forbidden in [
        "GRANT SELECT",
        "GRANT INSERT",
        "GRANT UPDATE",
        "GRANT DELETE",
        "GRANT TRUNCATE",
        "GRANT USAGE ON SCHEMA",
    ] {
        assert!(!MIGRATION.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn manifest_readiness_and_rust_capabilities_advance_together() {
    for source in [MIGRATION, CONTRACT_SOURCE, SECURITY_SUPPORT_SOURCE] {
        assert!(source.contains(FUNCTION_IDENTITY));
    }
    assert!(CONTRACT_SOURCE.contains("OPERATION_CAPABILITY_IDENTITIES_V1: [&str; 22]"));
    assert!(CONTRACT_SOURCE.contains("capabilities.clone().count() != 24"));
    assert!(SECURITY_SUPPORT_SOURCE.contains("const EXECUTOR_FUNCTIONS: [&str; 24]"));
    let readiness = CONTRACT_SOURCE
        .split("RUNTIME_EXECUTION_READINESS_DEFINITION_DIGEST_V1")
        .nth(1)
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    for source in [DATABASE_SOURCE, SECURITY_SUPPORT_SOURCE] {
        let digest = readiness
            .split('"')
            .nth(1)
            .expect("readiness digest must be pinned");
        assert!(source.contains(digest));
    }
}
