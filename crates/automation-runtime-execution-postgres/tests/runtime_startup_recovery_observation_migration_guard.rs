const MIGRATION: &str =
    include_str!("../../../migrations/202607270005_add_atomic_startup_recovery_observation_v2.sql");
const CONTRACT_SOURCE: &str = include_str!("../src/contract.rs");
const DATABASE_SOURCE: &str = include_str!("../src/database.rs");
const SECURITY_SUPPORT_SOURCE: &str = include_str!("postgres_security/support.rs");
const SUSPENSION_GUARD: &str = include_str!("runtime_suspend_attempt_ledger_migration_guard.rs");
const OWNER_PROJECTION_GUARD: &str =
    include_str!("runtime_startup_recovery_owner_projection_migration_guard.rs");

const FUNCTION_IDENTITY: &str =
    "public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)";
const PREVIOUS_MANIFEST_DIGEST: &str =
    "57694b2a5f374fa63882fb52f5bfe506b321968c961ea2cf9de8006fd46a5979";
const CURRENT_MANIFEST_DIGEST: &str =
    "94177e2025d87f492e988e3e27b8193b0f7157d4ea7fcd6099308534df9073ff";
const PREVIOUS_READINESS_DIGEST: &str =
    "6523d219df9a148c9428ac8f45b9317bcad6b56af44b753f11167fc582ca5875";
const CURRENT_READINESS_DIGEST: &str =
    "ae397ea106f18aa71c6cf2427ebf2705638462066e480b6d0f10b9759a8adc5e";
const LATEST_READINESS_DIGEST: &str =
    "c2cba3c5591876238f0ae0248b2c7c205953b6cde2a62705038a42fa9aa2aa81";

fn dollar_block(tag: &str) -> &'static str {
    MIGRATION
        .split(&format!("DO ${tag}$"))
        .nth(1)
        .unwrap()
        .split(&format!("${tag}$;"))
        .next()
        .unwrap()
}

fn function_body() -> &'static str {
    MIGRATION
        .split("CREATE FUNCTION public.starring_runtime_startup_recovery_observe_v2(")
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
fn observation_is_serializable_locked_single_clock_and_comment_free() {
    let body = function_body();
    let global = body.find("'starring-runtime-writer-fence-v1'").unwrap();
    let owner = body.find("'starring-runtime-gateway-owner-v1:'").unwrap();
    let tables = body.find("LOCK TABLE").unwrap();
    let clock = body
        .find("database_now := pg_catalog.clock_timestamp();")
        .unwrap();
    let owner_row = body.find("FROM public.runtime_gateway_owners").unwrap();
    assert!(global < owner);
    assert!(owner < tables);
    assert!(tables < clock);
    assert!(clock < owner_row);
    assert_eq!(body.matches("pg_catalog.clock_timestamp()").count(), 1);
    for required in [
        "transaction_isolation",
        "<> 'serializable'",
        "transaction_read_only",
        "IN SHARE MODE;",
        "FOR UPDATE;",
        "runtime_startup_recovery_observation_transaction_invalid",
        "runtime_startup_recovery_observation_input_invalid",
    ] {
        assert!(body.contains(required), "{required}");
    }
    for line in MIGRATION.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"));
        assert!(!trimmed.starts_with("//"));
        assert!(!trimmed.starts_with("/*"));
    }
    assert!(MIGRATION.ends_with("RESET search_path;\n"));
}

#[test]
fn exact_owner_tuple_and_outcome_shapes_are_explicit() {
    let body = function_body();
    for required in [
        "owner_row.process_instance_id\n            IS DISTINCT FROM expected_process_instance_id",
        "owner_row.lease_epoch IS DISTINCT FROM expected_lease_epoch",
        "owner_row.expected_build_revision\n            IS DISTINCT FROM expected_runtime_build_revision",
        "owner_row.owner_revision\n            IS DISTINCT FROM expected_owner_revision",
        "owner_row.expires_at\n            IS DISTINCT FROM expected_owner_expires_at",
        "owner_row.expires_at <= database_now",
        "outcome_name := 'not_current';",
        "outcome_name := 'ambiguous';",
        "outcome_name := 'observed';",
        "serving_state_name := 'ambiguous';",
    ] {
        assert!(body.contains(required), "{required}");
    }
    let result = MIGRATION
        .split("RETURNS TABLE(")
        .nth(1)
        .unwrap()
        .split(")\nLANGUAGE")
        .next()
        .unwrap();
    for field in [
        "observed_gateway_shard_id TEXT",
        "observed_process_instance_id TEXT",
        "observed_lease_epoch BIGINT",
        "observed_runtime_build_revision TEXT",
        "observed_owner_revision BIGINT",
        "database_now TIMESTAMPTZ",
        "observed_owner_expires_at TIMESTAMPTZ",
        "serving_state_name TEXT",
        "serving_count BIGINT",
        "serving_earliest_expiry TIMESTAMPTZ",
        "serving_retry_after_milliseconds BIGINT",
        "recoverable_awaiting_certification_count BIGINT",
        "suspended_local_effect_count BIGINT",
        "pending_runtime_drain_intent_count BIGINT",
        "acknowledged_product_handoff_count BIGINT",
    ] {
        assert!(result.contains(field), "{field}");
    }
}

#[test]
fn serving_projection_is_truthful_bounded_and_never_merges_mixed_classes() {
    let body = function_body();
    for required in [
        "deployment.phase = 'live'",
        "drain.intent_state = 'pending'",
        "live.lease_process_instance_id <>\n                    expected_process_instance_id",
        "live.lease_expires_at > database_now",
        "live.lease_expires_at <= database_now",
        "live.lease_last_heartbeat_at >=\n                            live.certified_at",
        "stale_live_count <> 0\n            AND foreign_fresh_count <> 0",
        "orphan_fresh_count <> 0",
        "serving_state_name := 'recoverable_stale';",
        "serving_state_name := 'foreign_fresh';",
        "serving_state_name := 'empty';",
        "retry_milliseconds < 1",
        "WHEN retry_milliseconds > 1000 THEN 1000",
        "ELSE retry_milliseconds",
        "stale_live_count > 4294967295",
        "foreign_fresh_count > 4294967295",
    ] {
        assert!(body.contains(required), "{required}");
    }
}

#[test]
fn current_reservation_and_drain_limits_fail_closed_without_fake_counts() {
    let body = function_body();
    for required in [
        "runtime_drain_intents_v2_state_check",
        "CHECK (intent_state = ''pending''::text)",
        "drain.intent_state IS DISTINCT FROM 'pending'",
        "reservation_count <> exact_awaiting_reservation_count",
        "deployment.phase = 'awaiting_gateway_ready'",
        "deployment.revision =\n                    reservation.deployment_revision",
        "deployment.convergence_attempt_no =\n                    reservation.convergence_attempt_no",
        "invalid_suspend_attempt_count",
        "LEFT JOIN public.runtime_suspended_attempts_v2 AS suspended",
        "suspended.local_effect_kind = 'exact_route'",
        "LEFT JOIN public.runtime_suspend_attempt_completions_v2 AS completion",
        "completion.suspension_id = operation.suspension_id",
        "pending_runtime_drain_intent_count := pending_drain_count;",
        "acknowledged_product_handoff_count := 0;",
    ] {
        assert!(body.contains(required), "{required}");
    }
    assert!(
        body.find("reservation_count <> exact_awaiting_reservation_count")
            .unwrap()
            < body
                .find("acknowledged_product_handoff_count := 0;")
                .unwrap()
    );
}

#[test]
fn future_certification_terminal_evidence_must_replace_nonawaiting_fail_closed_contract() {
    let body = function_body();
    let fail_closed = body
        .find("reservation_count <> exact_awaiting_reservation_count")
        .unwrap();
    let projected = body
        .find("recoverable_awaiting_certification_count := reservation_count;")
        .unwrap();
    assert!(fail_closed < projected);
    assert!(!MIGRATION.contains("runtime_certification_operation_completions"));
}

#[test]
fn only_observation_execute_is_exposed_and_readiness_pins_move_once() {
    let acl = dollar_block("execution_acl");
    let manifest = dollar_block("patch_schema_manifest");
    let readiness = dollar_block("patch_readiness");
    let postflight = dollar_block("postflight");
    assert_eq!(MIGRATION.matches("GRANT EXECUTE ON FUNCTION").count(), 1);
    assert!(acl
        .contains("GRANT EXECUTE ON FUNCTION public.starring_runtime_startup_recovery_observe_v2"));
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
    for required in [
        FUNCTION_IDENTITY,
        "RETURN observed_count = 734",
        "2b9c978bc17afb7440781c2d5ca50eed37c1ad89986e1f7fe28d2ab5c72fa9b5",
        PREVIOUS_MANIFEST_DIGEST,
        CURRENT_MANIFEST_DIGEST,
        PREVIOUS_READINESS_DIGEST,
        CURRENT_READINESS_DIGEST,
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert!(manifest.contains("runtime_startup_recovery_observation_manifest_function_patch_drift"));
    assert!(
        readiness.contains("runtime_startup_recovery_observation_readiness_function_patch_drift")
    );
    assert!(
        readiness.contains("runtime_startup_recovery_observation_readiness_allowlist_patch_drift")
    );
    assert!(postflight.contains("pg_catalog.has_function_privilege"));
    for source in [CONTRACT_SOURCE, DATABASE_SOURCE, SECURITY_SUPPORT_SOURCE] {
        assert!(source.contains(LATEST_READINESS_DIGEST));
    }
    assert!(CONTRACT_SOURCE.contains(FUNCTION_IDENTITY));
    assert!(SECURITY_SUPPORT_SOURCE.contains(FUNCTION_IDENTITY));
    assert!(SUSPENSION_GUARD.contains(PREVIOUS_READINESS_DIGEST));
    assert!(OWNER_PROJECTION_GUARD.contains(CURRENT_READINESS_DIGEST));
    assert!(OWNER_PROJECTION_GUARD.contains(LATEST_READINESS_DIGEST));
}
