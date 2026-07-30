const MIGRATION: &str = include_str!(
    "../../../migrations/202607270010_add_owner_fenced_startup_reserved_awaiting_execution_v2.sql"
);
const PREVIOUS_MIGRATION: &str = include_str!(
    "../../../migrations/202607270009_establish_runtime_certification_terminal_ledger_v2.sql"
);
const CONTRACT_SOURCE: &str = include_str!("../src/contract.rs");
const DATABASE_SOURCE: &str = include_str!("../src/database.rs");
const SECURITY_SUPPORT_SOURCE: &str = include_str!("postgres_security/support.rs");

const EXECUTOR: &str = "public.starring_runtime_startup_recovery_execute_reserved_awaiting_v2";
const MIGRATION_READINESS_DIGEST: &str =
    "4e58c914016de080372586cc2efc7e9a5221c8703450d767934389a5c4c07db8";
const CURRENT_READINESS_DIGEST: &str =
    "437eef0962f31be61e9fcb2f6705b2cda14f4d52105ae024ca4bc29b967e001c";
const MANIFEST_DIGEST: &str = "c2de6cf64ce6efbcf22e31f06da774195996060a692c45b48f073ff93fa4d630";

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
fn migration_is_quiescent_pinned_comment_free_and_collision_safe() {
    let preflight = dollar_block("preflight");
    assert!(MIGRATION.starts_with("SET LOCAL lock_timeout = '5s';"));
    assert!(MIGRATION.ends_with("RESET search_path;\n"));
    assert!(MIGRATION.contains("IN ACCESS EXCLUSIVE MODE;"));
    for required in [
        "other_client_session_count",
        "prepared_transaction_count",
        "executor_membership_count",
        "starring_runtime_cert_awaiting_reset_exact_v2",
        "starring_runtime_startup_reserved_projection_exact_v2",
        "runtime_startup_reserved_awaiting_execution_preflight_drift",
    ] {
        assert!(preflight.contains(required), "{required}");
    }
    assert!(!PREVIOUS_MIGRATION.contains(EXECUTOR));
    for line in MIGRATION.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"));
        assert!(!trimmed.starts_with("//"));
        assert!(!trimmed.starts_with("/*"));
    }
    for name in [
        "starring_runtime_startup_recovery_execute_reserved_awaiting_v2",
        "starring_runtime_cert_awaiting_reset_exact_v2",
        "starring_runtime_startup_reserved_projection_exact_v2",
    ] {
        assert!(name.len() <= 63, "{name}");
    }
}

#[test]
fn executor_is_serializable_owner_and_writer_fenced_without_skip_locked() {
    let function = executor();
    for required in [
        "transaction_isolation",
        "<> 'serializable'",
        "transaction_read_only",
        "starring_runtime_writer_fence_observe_v1",
        "starring-runtime-gateway-owner-v1:",
        "starring-runtime-startup-recovery-action-v2:",
        "FOR UPDATE",
        "runtime_startup_reserved_awaiting_execution_owner_lost",
        "runtime_startup_reserved_awaiting_execution_writer_fenced",
    ] {
        assert!(function.contains(required), "{required}");
    }
    assert!(!function.contains("SKIP LOCKED"));
    let writer = function
        .find("starring_runtime_writer_fence_observe_v1")
        .unwrap();
    let owner = function.find("starring-runtime-gateway-owner-v1:").unwrap();
    let action = function
        .find("starring-runtime-startup-recovery-action-v2:")
        .unwrap();
    let slot = function.find("starring-runtime-serving-slot-v1:").unwrap();
    assert!(writer < owner && owner < action && action < slot);
}

#[test]
fn terminal_first_transition_is_exact_and_atomic() {
    let function = executor();
    for required in [
        "'reserved_awaiting_certification'",
        "'awaiting_reset'",
        "'reconciling_panels'",
        "starring_runtime_cert_awaiting_reset_exact_v2",
        "starring_runtime_startup_reserved_projection_exact_v2",
        "source_deployment_frame",
        "successor_deployment_frame",
        "source_slot_frame",
        "successor_slot_frame",
        "reservation_frame",
        "pg_catalog.sha256(source_deployment_frame)",
        "pg_catalog.sha256(reservation_frame)",
        "NOT BETWEEN 1 AND 1048576",
    ] {
        assert!(function.contains(required), "{required}");
    }
    let epoch = function
        .find("starring_runtime_slot_writer_fence_begin_unsafe_v2")
        .unwrap();
    let terminal = function
        .find("INSERT INTO public.runtime_certification_operation_terminals_v2")
        .unwrap();
    let deployment = function
        .find("UPDATE public.runtime_deployments AS deployment")
        .unwrap();
    let action = function
        .rfind("starring_runtime_startup_recovery_action_record_v2")
        .unwrap();
    assert!(epoch < terminal && terminal < deployment && deployment < action);
}

#[test]
fn observer_and_executor_share_fail_closed_canonical_classification() {
    let observer = dollar_block("patch_observation");
    let function = executor();
    for required in [
        "executable_unresolved_reservation_count",
        "slot_fence.pending_drain_intent_id IS NULL",
        "pending_drain.intent_state = ''pending''",
        "deployment.snapshot -> ''phase'' =",
        "pg_catalog.to_jsonb(",
        "invalid_reservation_count := reservation_count",
    ] {
        assert!(observer.contains(required), "{required}");
    }
    for required in [
        "unresolved_reservation_count <> 0",
        "executable_reservation_count = 0",
        "runtime_startup_reserved_awaiting_execution_product_drain_pending",
        "deployment.snapshot -> 'phase'",
        "pg_catalog.to_jsonb(deployment_row.revision)",
    ] {
        assert!(function.contains(required), "{required}");
    }
}

#[test]
fn replay_parser_is_byte_exact_and_fail_closed() {
    let helper = MIGRATION
        .split(
            "CREATE FUNCTION starring_runtime_private_v2.starring_runtime_startup_reserved_projection_exact_v2(",
        )
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap();
    for required in [
        "FOR frame_index IN 1..5 LOOP",
        "frame_length > 1048576",
        "pg_catalog.get_byte",
        "pg_catalog.jsonb_send(source_deployment_json)",
        "pg_catalog.to_jsonb(source_deployment)",
        "source_slot.writer_epoch",
        "NOT BETWEEN 1 AND 9223372036854775806",
        "successor_slot.updated_at < expected_terminal.terminal_at",
        "expected_terminal.terminal_receipt_bytes",
        "pg_catalog.substring(",
        "IS NOT DISTINCT FROM expected_terminal_scalar",
        "WHEN OTHERS THEN",
        "RETURN FALSE",
    ] {
        assert!(helper.contains(required), "{required}");
    }
}

#[test]
fn manifest_readiness_acl_and_rust_pins_advance_together() {
    for required in [
        "RETURN observed_count = 799",
        "52022b152b1189e01928d8cc14dc229d1ed094a5da7837711a06cf3077b0ea41",
        MANIFEST_DIGEST,
        MIGRATION_READINESS_DIGEST,
        "runtime_startup_reserved_awaiting_execution_postflight_drift",
        "starring_runtime_execution_database_readiness_v1",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert!(CONTRACT_SOURCE.contains("OPERATION_CAPABILITY_IDENTITIES_V1: [&str; 29]"));
    assert!(CONTRACT_SOURCE.contains(EXECUTOR));
    assert!(CONTRACT_SOURCE.contains("capabilities.clone().count() != 31"));
    assert!(SECURITY_SUPPORT_SOURCE.contains("const EXECUTOR_FUNCTIONS: [&str; 31]"));
    for source in [CONTRACT_SOURCE, DATABASE_SOURCE, SECURITY_SUPPORT_SOURCE] {
        assert!(source.contains(CURRENT_READINESS_DIGEST));
    }
}
