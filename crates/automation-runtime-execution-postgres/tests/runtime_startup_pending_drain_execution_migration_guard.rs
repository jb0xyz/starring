use std::fs;

const MIGRATION_NAME: &str = "202607270012_add_owner_fenced_startup_pending_drain_execution_v2.sql";
const PREVIOUS_MIGRATION_NAME: &str =
    "202607270011_add_owner_fenced_startup_suspended_local_execution_v2.sql";
const MIGRATION: &str = include_str!(
    "../../../migrations/202607270012_add_owner_fenced_startup_pending_drain_execution_v2.sql"
);
const PREVIOUS_MIGRATION: &str = include_str!(
    "../../../migrations/202607270011_add_owner_fenced_startup_suspended_local_execution_v2.sql"
);
const SECURITY_SUPPORT_SOURCE: &str = include_str!("postgres_security/support.rs");

const SELECTOR: &str = "public.starring_runtime_startup_recovery_select_pending_drain_v2";
const RECORDER: &str = "public.starring_runtime_startup_recovery_record_pending_drain_none_v2";
const EXECUTOR: &str = "public.starring_runtime_startup_recovery_execute_pending_drain_v2";
const SELECTOR_IDENTITY: &str = "public.starring_runtime_startup_recovery_select_pending_drain_v2(text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)";
const RECORDER_IDENTITY: &str = "public.starring_runtime_startup_recovery_record_pending_drain_none_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint)";
const EXECUTOR_IDENTITY: &str = "public.starring_runtime_startup_recovery_execute_pending_drain_v2(text,bigint,bigint,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean,text)";
const MANIFEST_DEFINITION_DIGEST: &str =
    "9de93ea5d565254c47533c7af43959aa873014bee385a2af775fafdcbf8118b9";
const READINESS_DEFINITION_DIGEST: &str =
    "1c20dcc6c6e01b440d9a5813bad12b109d89a67c5d6815f9fd15551fa3c0f4e5";
const LATEST_READINESS_DEFINITION_DIGEST: &str =
    "572d7ffd19d6f2edb5ec84ea6b7bfebd178c7da0568bce61af2f7907cfe72647";

fn dollar_block(tag: &str) -> &'static str {
    MIGRATION
        .split(&format!("DO ${tag}$"))
        .nth(1)
        .unwrap()
        .split(&format!("${tag}$;"))
        .next()
        .unwrap()
}

fn function(name: &str) -> &'static str {
    MIGRATION
        .split(&format!("CREATE FUNCTION {name}("))
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap()
}

#[test]
fn migration_is_ordered_after_011_collision_safe_and_comment_free() {
    let migration_directory = format!("{}/../../migrations", env!("CARGO_MANIFEST_DIR"));
    let mut migrations = fs::read_dir(migration_directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.ends_with(".sql"))
        .collect::<Vec<_>>();
    migrations.sort();
    let previous = migrations
        .iter()
        .position(|name| name == PREVIOUS_MIGRATION_NAME)
        .unwrap();
    let current = migrations
        .iter()
        .position(|name| name == MIGRATION_NAME)
        .unwrap();
    assert_eq!(current, previous + 1);
    assert!(MIGRATION.starts_with("SET LOCAL lock_timeout = '5s';"));
    assert!(MIGRATION.ends_with("RESET search_path;\n"));
    assert!(!PREVIOUS_MIGRATION.contains(SELECTOR_IDENTITY));
    assert!(!PREVIOUS_MIGRATION.contains(RECORDER_IDENTITY));
    assert!(!PREVIOUS_MIGRATION.contains(EXECUTOR_IDENTITY));
    let preflight = dollar_block("preflight");
    for identity in [SELECTOR_IDENTITY, RECORDER_IDENTITY, EXECUTOR_IDENTITY] {
        assert!(preflight.contains(identity), "{identity}");
    }
    for line in MIGRATION.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"));
        assert!(!trimmed.starts_with("//"));
        assert!(!trimmed.starts_with("/*"));
    }
    for name in [
        "starring_runtime_startup_recovery_select_pending_drain_v2",
        "starring_runtime_startup_recovery_record_pending_drain_none_v2",
        "starring_runtime_startup_recovery_execute_pending_drain_v2",
    ] {
        assert!(name.len() <= 63, "{name}");
    }
}

#[test]
fn public_signatures_and_selector_target_receipt_are_exact() {
    let selector = function(SELECTOR);
    for required in [
        "RETURNS TABLE(",
        "selection_outcome_name TEXT",
        "observed_database_now TIMESTAMPTZ",
        "observed_owner_expires_at TIMESTAMPTZ",
        "selected_drain_intent_id TEXT",
        "selected_source_intent_revision BIGINT",
        "selected_source_state_digest TEXT",
        "selected_slot_guild_id TEXT",
        "selected_slot_ruleset_key TEXT",
        "selected_target_version BIGINT",
        "selected_target_content_hash TEXT",
        "selected_target_binding_revision BIGINT",
        "selected_target_binding_fingerprint TEXT",
        "product_row.expected_target_version",
        "product_row.expected_target_content_hash",
        "product_row.expected_target_binding_revision",
        "product_row.expected_target_binding_fingerprint",
        "deployment_row.snapshot #>> '{target,version}'",
        "deployment_row.snapshot #>> '{target,content_hash}'",
        "deployment_row.snapshot #>> '{target,binding_revision}'",
        "deployment_row.snapshot #>> '{target,binding_fingerprint}'",
        "!~ '^[0-9a-f]{64}$'",
    ] {
        assert!(selector.contains(required), "{required}");
    }
    assert!(selector.contains("selection_outcome_name := 'no_candidate'"));
    assert!(selector.contains("selection_outcome_name := 'candidate'"));
    for identity in [SELECTOR_IDENTITY, RECORDER_IDENTITY, EXECUTOR_IDENTITY] {
        assert!(MIGRATION.contains(identity), "{identity}");
        assert!(SECURITY_SUPPORT_SOURCE.contains(identity), "{identity}");
    }
}

#[test]
fn capabilities_are_serializable_security_definer_and_acl_quarantined() {
    let selector = function(SELECTOR);
    let recorder = function(RECORDER);
    let executor = function(EXECUTOR);
    for body in [selector, recorder, executor] {
        for required in [
            "LANGUAGE plpgsql",
            "VOLATILE",
            "STRICT",
            "PARALLEL UNSAFE",
            "SECURITY DEFINER",
            "SET search_path = pg_catalog",
            "transaction_isolation",
            "<> 'serializable'",
        ] {
            assert!(body.contains(required), "{required}");
        }
        assert!(!body.contains("SKIP LOCKED"));
    }
    assert!(!selector.contains("transaction_read_only') <> 'off'"));
    assert!(recorder.contains("transaction_read_only') <> 'off'"));
    assert!(executor.contains("transaction_read_only') <> 'off'"));
    let grant = dollar_block("grant_executor");
    for identity in [SELECTOR_IDENTITY, RECORDER_IDENTITY, EXECUTOR_IDENTITY] {
        assert!(grant.contains(identity), "{identity}");
    }
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
    assert!(MIGRATION.contains("REVOKE ALL ON FUNCTION"));
    assert!(MIGRATION.contains("invalid_acl_count"));
    assert!(MIGRATION.contains("invalid_alias_count"));
    assert!(MIGRATION.contains("starring_runtime_startup_recovery_record_pending_drain_no_candi"));
}

#[test]
fn canonical_state_ddl_trigger_and_manifest_are_pinned() {
    for required in [
        "ADD COLUMN canonical_state_bytes BYTEA",
        "ADD COLUMN canonical_state_digest TEXT",
        "runtime_drain_intents_v2_canonical_state_check",
        "canonical_state_digest = pg_catalog.encode(",
        "pg_catalog.sha256(canonical_state_bytes)",
        "ALTER COLUMN canonical_state_bytes SET NOT NULL",
        "ALTER COLUMN canonical_state_digest SET NOT NULL",
        "CREATE TRIGGER runtime_drain_intents_v2_00_initialize_canonical_state",
        "starring_runtime_pending_drain_initialize_v2",
        "RETURN observed_count = 828",
        "a10c4cc166d3fa07adc4bb800e47f3c0cfb1747b8f6a49fd8e1144d1a11865a3",
        MANIFEST_DEFINITION_DIGEST,
        READINESS_DEFINITION_DIGEST,
        "runtime_startup_pending_drain_execution_postflight_drift",
        "public.starring_runtime_execution_schema_manifest_v1()",
        "public.starring_runtime_exact_target_schema_manifest_v1()",
        "public.starring_runtime_serving_schema_manifest_v1()",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert!(SECURITY_SUPPORT_SOURCE.contains(LATEST_READINESS_DEFINITION_DIGEST));
    let strict_backfill = dollar_block("strict_backfill_exact");
    for required in [
        "FROM public.runtime_drain_intents_v2 AS drain",
        "starring_runtime_pending_drain_state_exact_v2(",
        "ERRCODE = 'RE001'",
        "runtime_pending_drain_strict_backfill_invalid",
    ] {
        assert!(strict_backfill.contains(required), "{required}");
    }
}

#[test]
fn legacy_recovery_and_product_trigger_contracts_are_rebound_exactly() {
    let recovery_patch = dollar_block("patch_recovery_drain_state_contracts");
    for required in [
        "public.starring_runtime_startup_recovery_observe_v2",
        "public.starring_runtime_startup_recovery_execute_stale_live_v2",
        "CHECK (intent_state = ANY (ARRAY[",
        "route_absent_acknowledged",
        "starring_runtime_pending_drain_state_exact_v2(",
        "acknowledged_drain_count",
        "acknowledged_product_handoff_count :=",
        "runtime_pending_drain_live_exclusion_patch_drift",
    ] {
        assert!(recovery_patch.contains(required), "{required}");
    }
    let initializer =
        function("starring_runtime_private_v2.starring_runtime_pending_drain_initialize_v2");
    for required in [
        "gate_stage IS DISTINCT FROM 'drain_insert'",
        "NEW.product_operation_id IS NULL",
        "NEW.drain_intent_id IS NULL",
        "NEW.product_operation_id !~ '^[0-9a-f]{32}$'",
        "NEW.drain_intent_id !~ '^[0-9a-f]{32}$'",
        "IS DISTINCT FROM NEW.product_operation_id",
        "IS DISTINCT FROM NEW.drain_intent_id",
    ] {
        assert!(initializer.contains(required), "{required}");
    }
    let product_trigger = MIGRATION
        .split("CREATE OR REPLACE FUNCTION public.reject_runtime_product_drain_mutation()")
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap();
    assert!(product_trigger
        .contains("ELSIF TG_OP = 'UPDATE' THEN\n        IF TG_RELID = pg_catalog.to_regclass("));
    assert!(product_trigger.contains("IF gate_stage = 'pending_drain_recovery_update'"));
}

#[test]
fn deployment_history_exception_is_exact_and_fail_closed() {
    let convergence_guard = dollar_block("patch_deployment_history_guard");
    let projection_guard = dollar_block("patch_deployment_projection_guard");
    for body in [convergence_guard, projection_guard] {
        for required in [
            "starring.runtime_pending_drain_deployment_action_v2",
            "starring.runtime_pending_drain_deployment_id_v2",
            "starring.runtime_pending_drain_source_fence_v2",
            "starring.runtime_pending_drain_successor_fence_v2",
            "starring.runtime_pending_drain_successor_controller_v2",
            "AND OLD.controller_id IS NULL",
            "AND NEW.controller_id IS NULL",
            "AND OLD.last_fencing_token BETWEEN 1 AND 9223372036854775806",
            "AND NEW.last_fencing_token = OLD.last_fencing_token + 1",
            "AND NEW.snapshot = pg_catalog.jsonb_set(",
            "ARRAY[''snapshot'', ''last_fencing_token'', ''last_controller_id'']",
        ] {
            assert!(body.contains(required), "{required}");
        }
    }
    assert_eq!(
        convergence_guard
            .matches("AND NOT pending_drain_history")
            .count(),
        2
    );
    assert!(projection_guard.contains("IF NOT pending_drain_history"));
}

#[test]
fn compound_claim_ack_revisions_prior_digest_and_seal_bundle_are_exact() {
    let executor = function(EXECUTOR);
    for required in [
        "WHEN 'claim' THEN 1",
        "WHEN 'acknowledge' THEN 2",
        "requested_claim_action_authority_revision",
        "requested_claim_selection_authority_revision",
        "requested_ack_action_authority_revision",
        "requested_ack_selection_authority_revision",
        "requested_claim_selection_authority_revision + 1",
        "requested_ack_selection_authority_revision",
        "requested_ack_selection_authority_revision + 1",
        "stage_source_intent_revision :=",
        "requested_selected_source_intent_revision + stage_tag - 1",
        "requested_prior_claim_terminal_digest",
        "prior_claim_action_row.terminal_digest",
        "prior_claim_action_row.minimum_database_now",
        "prior_claim_action_row.recorded_at",
        "existing_action_row.minimum_database_now",
        "runtime_startup_pending_drain_replay_clock_regressed",
        "pg_catalog.int2send(2::SMALLINT)",
        "requested_pre_slot_present",
        "requested_pre_slot_admission_generation",
        "requested_pre_slot_observation_sequence",
        "requested_seal_key",
        "pg_catalog.encode(requested_seal_key, 'hex')",
        "IS DISTINCT FROM requested_selected_drain_intent_id",
        "requested_seal_generation",
        "requested_post_slot_admission_generation",
        "requested_post_slot_observation_sequence",
        "requested_post_global_observation_sequence",
        "state_kind = 'pending_unclaimed'",
        "state_kind = 'pending_claimed'",
        "'pending_claimed'",
        "'route_absent_acknowledged'",
        "starring_runtime_pending_drain_product_root_compound_exact_v2",
        "starring_runtime_pending_drain_projection_frame_exact_v2",
        "prior_source_digest_frame IS DISTINCT FROM (",
        "pg_catalog.sha256(",
        "prior_successor_state_frame",
    ] {
        assert!(executor.contains(required), "{required}");
    }
    assert!(MIGRATION.contains(
        "starring_runtime_private_v2.starring_runtime_pending_drain_product_root_compound_exact_v2(bytea,text,bigint,bigint,smallint,text,bytea)"
    ));
    for required in [
        "expected_selected_drain_intent_id TEXT",
        "selected_drain_intent_id_value",
        "token_index = 12",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert_eq!(
        executor
            .matches("UPDATE public.runtime_deployments AS deployment")
            .count(),
        1
    );
    assert_eq!(
        executor
            .matches("starring_runtime_startup_recovery_action_record_v2")
            .count(),
        2
    );
}

#[test]
fn lock_order_and_durable_mutations_precede_journal_commit() {
    let executor = function(EXECUTOR);
    let writer = executor.find("starring-runtime-writer-fence-v1").unwrap();
    let owner = executor.find("starring-runtime-gateway-owner-v1:").unwrap();
    let action = executor
        .find("starring-runtime-startup-recovery-action-v2:")
        .unwrap();
    let slot = executor.find("starring-runtime-serving-slot-v1:").unwrap();
    let deployment = executor
        .find("SELECT deployment.*\n    INTO deployment_row")
        .unwrap();
    let product = executor
        .find("SELECT product.*\n    INTO product_row")
        .unwrap();
    let drain = executor.find("starring-runtime-drain-intent-v2:").unwrap();
    assert!(
        writer < owner
            && owner < action
            && action < slot
            && slot < deployment
            && deployment < product
            && product < drain
    );
    let deployment_mutation = executor
        .find("UPDATE public.runtime_deployments AS deployment")
        .unwrap();
    let drain_mutation = executor
        .find("UPDATE public.runtime_drain_intents_v2 AS drain")
        .unwrap();
    let journal = executor
        .rfind("starring_runtime_startup_recovery_action_record_v2")
        .unwrap();
    assert!(deployment_mutation < drain_mutation && drain_mutation < journal);
}
