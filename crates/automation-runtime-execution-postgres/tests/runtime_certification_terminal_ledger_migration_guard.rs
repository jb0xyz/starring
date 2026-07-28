const MIGRATION: &str = include_str!(
    "../../../migrations/202607270009_establish_runtime_certification_terminal_ledger_v2.sql"
);
const PREVIOUS_MIGRATION: &str = include_str!(
    "../../../migrations/202607270008_add_owner_fenced_startup_stale_live_execution_v2.sql"
);
const CONTRACT_SOURCE: &str = include_str!("../src/contract.rs");
const DATABASE_SOURCE: &str = include_str!("../src/database.rs");
const SECURITY_SUPPORT_SOURCE: &str = include_str!("postgres_security/support.rs");

const PREVIOUS_MANIFEST_DEFINITION_DIGEST: &str =
    "00824784a0b0276e2ef83b4e4094c274cffb50b9c640af61350a152dc112c835";
const PREVIOUS_READINESS_DEFINITION_DIGEST: &str =
    "c2cba3c5591876238f0ae0248b2c7c205953b6cde2a62705038a42fa9aa2aa81";
const PREVIOUS_OBSERVATION_DEFINITION_DIGEST: &str =
    "1bafd85ec4d2291c6ab7cf213acaed35fe637409a1ed8679881ee8686956df09";
const CURRENT_MANIFEST_CONTENT_DIGEST: &str =
    "51f3694196e13c3b5bd21421ccdaa595291f2832063802df4967f502606bf0b5";
const CURRENT_MANIFEST_DEFINITION_DIGEST: &str =
    "1fa238c260d3bdfa7b0c914a42616c2889c02d92253829c8bafa63a8a255a3f7";
const CURRENT_READINESS_DEFINITION_DIGEST: &str =
    "a5191ef59e5365476860af1150a176049ef00c5b0d6c3f7cfe40e0b5be9d738a";
const CURRENT_OBSERVATION_DEFINITION_DIGEST: &str =
    "7153d2dcf3eaa6a6534368eead9f40c157c63372c879ce99adf173eb3d23f306";
const LATEST_READINESS_DEFINITION_DIGEST: &str =
    "a3674e7c69f24ce212ddf0598d23f448a47f0b6e7766dee20a78399d5b6477e7";

fn dollar_block(tag: &str) -> &'static str {
    MIGRATION
        .split(&format!("DO ${tag}$"))
        .nth(1)
        .unwrap()
        .split(&format!("${tag}$;"))
        .next()
        .unwrap()
}

#[test]
fn migration_is_quiescent_pinned_and_comment_free() {
    let preflight = dollar_block("preflight");
    assert!(MIGRATION.starts_with("SET LOCAL lock_timeout = '5s';"));
    assert!(MIGRATION.ends_with("RESET search_path;\n"));
    assert!(MIGRATION.contains("IN ACCESS EXCLUSIVE MODE;"));
    for required in [
        "executor_role_is_quarantined",
        "executor_membership_count <> 0",
        "other_client_session_count <> 0",
        "prepared_transaction_count <> 0",
        PREVIOUS_MANIFEST_DEFINITION_DIGEST,
        PREVIOUS_READINESS_DEFINITION_DIGEST,
        PREVIOUS_OBSERVATION_DEFINITION_DIGEST,
        "runtime_certification_terminal_ledger_preflight_drift",
    ] {
        assert!(preflight.contains(required), "{required}");
    }
    assert!(!PREVIOUS_MIGRATION.contains("runtime_certification_operation_terminals_v2"));
    for line in MIGRATION.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"));
        assert!(!trimmed.starts_with("//"));
        assert!(!trimmed.starts_with("/*"));
    }
}

#[test]
fn immutable_terminal_rows_bind_exact_root_result_and_receipt() {
    for required in [
        "CREATE TABLE public.runtime_certification_operation_terminals_v2",
        "record_format_version SMALLINT NOT NULL",
        "operation_id TEXT PRIMARY KEY",
        "terminal_outcome_name TEXT NOT NULL",
        "resulting_phase TEXT NOT NULL",
        "resulting_deployment_revision BIGINT NOT NULL",
        "resulting_convergence_attempt_no BIGINT NOT NULL",
        "terminal_receipt_bytes BYTEA NOT NULL",
        "terminal_receipt_digest TEXT NOT NULL",
        "FROM public.runtime_certification_operations_v2 AS operation",
        "runtime_certification_terminal_root_invalid",
        "terminal_outcome_name = 'awaiting_reset'",
        "resulting_phase = 'reconciling_panels'",
        "terminal_outcome_name = 'certification_committed'",
        "resulting_phase = 'live'",
        "resulting_deployment_revision =",
        "deployment_revision + 1",
        "resulting_convergence_attempt_no =",
        "convergence_attempt_no",
        "starring_runtime_private_v2.starring_runtime_certification_terminal_digest_v2",
        "starring.runtime.certification.terminal.v2",
        "pg_catalog.timestamptz_send(requested_terminal_at)",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert!(MIGRATION.contains("BETWEEN 1 AND 1048576"));
    assert!(MIGRATION.contains("terminal_receipt_digest <> pg_catalog.repeat('0', 64)"));
}

#[test]
fn mutation_gate_is_insert_only_scrubs_state_and_has_exact_short_trigger_names() {
    let function = MIGRATION
        .split("CREATE FUNCTION public.reject_runtime_certification_terminal_mutation_v2()")
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap();
    let non_insert = function.find("IF TG_OP <> 'INSERT' THEN").unwrap();
    let new_reference = function.find("NEW.operation_id").unwrap();
    assert!(non_insert < new_reference);
    assert_eq!(
        function
            .matches("runtime_certification_terminal_mutation_rejected")
            .count(),
        2
    );
    for setting in [
        "starring.runtime_certification_terminal_action_v2",
        "starring.runtime_certification_terminal_operation_id_v2",
        "starring.runtime_certification_terminal_outcome_v2",
        "starring.runtime_certification_terminal_result_phase_v2",
        "starring.runtime_certification_terminal_result_revision_v2",
        "starring.runtime_certification_terminal_result_attempt_v2",
        "starring.runtime_certification_terminal_digest_v2",
    ] {
        assert_eq!(function.matches(setting).count(), 3, "{setting}");
    }
    for trigger in [
        "runtime_certification_terminals_v2_reject_row",
        "runtime_certification_terminals_v2_reject_truncate",
    ] {
        assert_eq!(MIGRATION.matches(trigger).count(), 2, "{trigger}");
        assert!(trigger.len() <= 63);
    }
    for forbidden in [
        "GRANT SELECT",
        "GRANT INSERT",
        "GRANT UPDATE",
        "GRANT DELETE",
        "GRANT TRUNCATE",
        "GRANT EXECUTE",
    ] {
        assert!(!MIGRATION.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn observer_is_atomic_and_partitions_every_root_exactly_once() {
    let patch = dollar_block("patch_observation");
    for required in [
        "public.runtime_certification_operation_terminals_v2",
        "terminal.operation_id IS NULL",
        "deployment.phase = ''awaiting_gateway_ready''",
        "terminal.operation_id IS NOT NULL",
        "deployment.revision =",
        "terminal.resulting_deployment_revision",
        "deployment.revision >",
        "terminal.resulting_deployment_revision",
        "deployment.convergence_attempt_no >=",
        "terminal.resulting_convergence_attempt_no",
        "deployment.snapshot ->> ''revision'' =",
        "deployment.revision::TEXT",
        "deployment.snapshot #>> ''{phase,phase}'' =",
        "deployment.phase",
        "invalid_reservation_count := reservation_count",
        "- unresolved_reservation_count",
        "- exact_terminal_reservation_count",
        "IF invalid_reservation_count <> 0 THEN",
        "recoverable_awaiting_certification_count :=",
        "unresolved_reservation_count",
    ] {
        assert!(patch.contains(required), "{required}");
    }
    assert!(patch.contains("'        public.runtime_certification_operation_terminals_v2,'"));
    assert!(!patch.contains("SKIP LOCKED"));
}

#[test]
fn manifest_readiness_and_rust_pins_advance_once() {
    for required in [
        "RETURN observed_count = 796",
        CURRENT_MANIFEST_CONTENT_DIGEST,
        CURRENT_MANIFEST_DEFINITION_DIGEST,
        CURRENT_READINESS_DEFINITION_DIGEST,
        CURRENT_OBSERVATION_DEFINITION_DIGEST,
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    let postflight = dollar_block("postflight");
    for required in [
        "invalid_relation_count",
        "invalid_function_count",
        "invalid_trigger_count",
        "runtime_certification_terminal_ledger_postflight_drift",
    ] {
        assert!(postflight.contains(required), "{required}");
    }
    for source in [CONTRACT_SOURCE, DATABASE_SOURCE, SECURITY_SUPPORT_SOURCE] {
        assert!(
            source.contains(LATEST_READINESS_DEFINITION_DIGEST),
            "latest readiness pin"
        );
    }
}
