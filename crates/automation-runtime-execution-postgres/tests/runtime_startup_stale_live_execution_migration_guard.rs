const MIGRATION: &str = include_str!(
    "../../../migrations/202607270008_add_owner_fenced_startup_stale_live_execution_v2.sql"
);
const PREVIOUS_MIGRATION: &str = include_str!(
    "../../../migrations/202607270007_establish_runtime_startup_recovery_action_journal_v2.sql"
);
const OBSERVATION_MIGRATION: &str =
    include_str!("../../../migrations/202607270005_add_atomic_startup_recovery_observation_v2.sql");
const CONTRACT_SOURCE: &str = include_str!("../src/contract.rs");
const DATABASE_SOURCE: &str = include_str!("../src/database.rs");
const SECURITY_SUPPORT_SOURCE: &str = include_str!("postgres_security/support.rs");

const FUNCTION_IDENTITY: &str =
    "public.starring_runtime_startup_recovery_execute_stale_live_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)";
const PREVIOUS_MANIFEST_DEFINITION_DIGEST: &str =
    "c76a82cdd88a75259889d4cab4543797ad834d8f2e38f71268bbbc4b0e4cae0f";
const PREVIOUS_READINESS_DIGEST: &str =
    "ee9364b3bb8b17a3a2386c0be06ae2ab12b519c77647a4073e96f45bfb5084a8";
const PREVIOUS_ACTION_RECORD_DIGEST: &str =
    "bead9e18b19984a20070ee4b739f0fa7aaebb87d07a03913af17dd8b4b5b24b4";
const CURRENT_MANIFEST_CONTENT_DIGEST: &str =
    "659a6609f468edabc6135b5b056d58ac1929ea223471155e05325ea0d6da5a87";
const CURRENT_MANIFEST_DEFINITION_DIGEST: &str =
    "00824784a0b0276e2ef83b4e4094c274cffb50b9c640af61350a152dc112c835";
const CURRENT_READINESS_DIGEST: &str =
    "c2cba3c5591876238f0ae0248b2c7c205953b6cde2a62705038a42fa9aa2aa81";
const LATEST_READINESS_DIGEST: &str =
    "779d97c088a29027589ebdffa9753eb1333a1d9b511cd714211cde6ae8146c4e";
const CURRENT_EXECUTION_DEFINITION_DIGEST: &str =
    "de30f26d122062ad9da6fc9bd145a7376030fa7e1c9d114db740056e33136a42";
const CURRENT_ACTION_RECORD_DIGEST: &str =
    "7f3f6f98d37150b86d3d4ff860018053b402afa27eeaee82694a6dbf4f0e301b";

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
        .split("CREATE FUNCTION public.starring_runtime_startup_recovery_execute_stale_live_v2(")
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
fn migration_is_serialized_quiescent_pinned_and_comment_free() {
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
        PREVIOUS_READINESS_DIGEST,
        PREVIOUS_ACTION_RECORD_DIGEST,
        "runtime_startup_stale_live_execution_preflight_drift",
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
fn public_contract_is_exact_executor_only_and_serializable() {
    let body = function_body();
    let result = MIGRATION
        .split("RETURNS TABLE(")
        .nth(1)
        .unwrap()
        .split(")\nLANGUAGE")
        .next()
        .unwrap();
    for field in [
        "journal_outcome_name TEXT",
        "terminal_outcome_name TEXT",
        "recovery_id TEXT",
        "originating_emergency_generation BIGINT",
        "coordinator_generation BIGINT",
        "action_authority_revision BIGINT",
        "selection_authority_revision BIGINT",
        "recovery_class TEXT",
        "observed_gateway_shard_id TEXT",
        "observed_process_instance_id TEXT",
        "observed_lease_epoch BIGINT",
        "observed_runtime_build_revision TEXT",
        "observed_owner_revision BIGINT",
        "database_now TIMESTAMPTZ",
        "observed_owner_expires_at TIMESTAMPTZ",
        "minimum_database_now TIMESTAMPTZ",
        "recorded_at TIMESTAMPTZ",
        "terminal_projection_bytes BYTEA",
        "terminal_digest TEXT",
    ] {
        assert!(result.contains(field), "{field}");
    }
    for required in [
        "VOLATILE",
        "STRICT",
        "PARALLEL UNSAFE",
        "SECURITY DEFINER",
        "SET search_path = pg_catalog",
        "ROWS 1",
        "transaction_isolation",
        "<> 'serializable'",
        "transaction_read_only",
        "runtime_startup_stale_live_execution_transaction_invalid",
        "REVOKE ALL ON FUNCTION",
        "GRANT EXECUTE ON FUNCTION public.starring_runtime_startup_recovery_execute_stale_live_v2",
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
    assert!(body.contains("expected_gateway_shard_id IS DISTINCT FROM 'shard:0'"));
}

#[test]
fn journal_replay_authenticates_before_terminal_projection_parsing() {
    let body = function_body();
    let replay_branch = body
        .split("IF selection_action_found OR authority_action_found THEN")
        .nth(1)
        .unwrap()
        .split("SELECT pg_catalog.count(*)\n    INTO writer_fence_count")
        .next()
        .unwrap();
    let authenticate = replay_branch
        .find("starring_runtime_startup_recovery_action_record_v2(")
        .unwrap();
    let parse = replay_branch
        .find("existing_action_row.terminal_projection_bytes\n                IS NOT DISTINCT FROM no_candidate_projection")
        .unwrap();
    assert!(authenticate < parse);
    for required in [
        "action_record.outcome_name IS DISTINCT FROM 'replayed'",
        "terminal_outcome_name := 'no_candidate';",
        "terminal_outcome_name := 'progressed';",
        "runtime_startup_stale_live_execution_projection_invalid",
    ] {
        assert!(replay_branch.contains(required), "{required}");
    }
}

#[test]
fn classifier_inventory_and_deterministic_earliest_selection_are_preserved() {
    let body = function_body();
    for required in [
        "runtime_drain_intents_v2_state_check",
        "live_scope_count",
        "stale_live_count",
        "foreign_fresh_count",
        "ambiguous_live_count",
        "orphan_fresh_count",
        "reservation_count",
        "exact_awaiting_reservation_count",
        "invalid_suspend_attempt_count",
        "active_exact_route_count",
        "pending_drain_count",
        "stale_live_count <> 0\n            AND foreign_fresh_count <> 0",
        "orphan_fresh_count <> 0",
        "reservation_count <> exact_awaiting_reservation_count",
    ] {
        assert!(body.contains(required), "{required}");
        assert!(OBSERVATION_MIGRATION.contains(required), "{required}");
    }
    for required in [
        "live.lease_expires_at,\n                live.updated_at,\n                live.deployment_id COLLATE pg_catalog.\"C\"",
        "LIMIT 1",
        "runtime_startup_stale_live_execution_state_ambiguous",
    ] {
        assert!(body.contains(required), "{required}");
    }
    for forbidden in [
        "SKIP LOCKED",
        "pg_try_advisory",
        "LIMIT 64",
        "starring_runtime_startup_recovery_execute_stale_live_v1",
    ] {
        assert!(!body.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn lock_order_and_authority_cast_safety_are_explicit() {
    let body = function_body();
    let writer = body
        .find("starring_runtime_writer_fence_observe_v1()")
        .unwrap();
    let owner_advisory = body.find("'starring-runtime-gateway-owner-v1:'").unwrap();
    let recovery_advisory = body
        .find("'starring-runtime-startup-recovery-action-v2:'")
        .unwrap();
    let owner_row = body.find("FROM public.runtime_gateway_owners").unwrap();
    let slot_advisory = body.find("'starring-runtime-serving-slot-v1:'").unwrap();
    let slot_lock = body
        .find("starring_runtime_slot_writer_fence_lock_v2(")
        .unwrap();
    let deployment = body.rfind("FROM public.runtime_deployments").unwrap();
    let activation = body.rfind("FROM public.activation_requests").unwrap();
    let promotion = body.rfind("FROM public.authoring_promotions").unwrap();
    let authority = body
        .find("starring_runtime_lock_current_authority(")
        .unwrap();
    let serving = body.rfind("FROM public.runtime_serving_leases").unwrap();
    let slot_mutation = body
        .find("starring_runtime_slot_writer_fence_begin_unsafe_v2(")
        .unwrap();
    let deployment_mutation = body.find("UPDATE public.runtime_deployments").unwrap();
    let journal = body
        .rfind("starring_runtime_startup_recovery_action_record_v2(")
        .unwrap();
    assert!(writer < owner_advisory);
    assert!(owner_advisory < recovery_advisory);
    assert!(recovery_advisory < owner_row);
    assert!(owner_row < slot_advisory);
    assert!(slot_advisory < slot_lock);
    assert!(slot_lock < deployment);
    assert!(deployment < activation);
    assert!(activation < promotion);
    assert!(promotion < authority);
    assert!(authority < serving);
    assert!(serving < slot_mutation);
    assert!(slot_mutation < deployment_mutation);
    assert!(deployment_mutation < journal);
    for required in [
        "FOR SHARE;",
        "pg_catalog.pg_input_is_valid(",
        "'{intent,authority,binding_revision}'",
        "'{stage,activation,target,version}'",
        ") IS NOT TRUE",
        "runtime_startup_stale_live_execution_authority_invalid",
    ] {
        assert!(body.contains(required), "{required}");
    }
}

#[test]
fn mutation_projection_and_clock_proof_bind_actual_pre_and_post_rows() {
    let body = function_body();
    for required in [
        "mutation_clock := public.starring_runtime_mutation_clock();",
        "OR mutation_clock < database_now",
        "OR mutation_clock < requested_minimum_database_now",
        "RETURNING deployment.* INTO terminal_deployment_row",
        "terminal_deployment_row.snapshot IS DISTINCT FROM next_snapshot",
        "terminal_deployment_row.updated_at IS DISTINCT FROM GREATEST(",
        "pg_catalog.to_jsonb(deployment_row)",
        "pg_catalog.to_jsonb(terminal_deployment_row)",
        "pg_catalog.to_jsonb(slot_fence_row)",
        "pg_catalog.to_jsonb(terminal_slot_fence_row)",
        "pg_catalog.to_jsonb(serving_row)",
        "pg_catalog.int2send(recovery_kind_tag)",
        "pg_catalog.timestamptz_send(recovery_evidence)",
        "pg_catalog.timestamptz_send(mutation_clock)",
        "action_record.database_now < mutation_clock",
        "action_record.recorded_at < mutation_clock",
        "action_record.database_now\n            IS DISTINCT FROM action_record.recorded_at",
    ] {
        assert!(body.contains(required), "{required}");
    }
    assert!(!body.contains("pg_catalog.jsonb_send(previous_snapshot)"));
    assert!(!body.contains("pg_catalog.jsonb_send(next_snapshot)"));
    assert!(
        body.rfind("UPDATE public.runtime_deployments").unwrap()
            < body
                .rfind("starring_runtime_startup_recovery_action_record_v2(")
                .unwrap()
    );
}

#[test]
fn one_mebibyte_bound_is_widened_at_all_three_enforcement_points() {
    let widen = dollar_block("widen_terminal_projection");
    let body = function_body();
    assert!(widen.contains("NOT BETWEEN 1 AND 131072"));
    assert!(widen.contains("NOT BETWEEN 1 AND 1048576"));
    assert!(widen.contains("runtime_startup_recovery_actions_v2_terminal_check CHECK ("));
    assert!(widen.contains("BETWEEN 1 AND 1048576"));
    assert!(body.contains("NOT BETWEEN 1 AND 1048576"));
    assert_eq!(MIGRATION.matches("131072").count(), 1);
}

#[test]
fn no_candidate_is_journaled_and_progress_is_atomic() {
    let body = function_body();
    let no_candidate = body
        .split("IF stale_live_count = 0 THEN")
        .nth(1)
        .unwrap()
        .split("IF selected_deployment_id IS NULL THEN")
        .next()
        .unwrap();
    assert!(no_candidate.contains("starring_runtime_startup_recovery_action_record_v2("));
    assert!(no_candidate.contains("no_candidate_projection"));
    assert!(no_candidate.contains("action_record.outcome_name IS DISTINCT FROM 'applied'"));
    let slot_mutation = body
        .find("starring_runtime_slot_writer_fence_begin_unsafe_v2(")
        .unwrap();
    let deployment_mutation = body.find("UPDATE public.runtime_deployments").unwrap();
    let journal = body
        .rfind("starring_runtime_startup_recovery_action_record_v2(")
        .unwrap();
    assert!(slot_mutation < deployment_mutation);
    assert!(deployment_mutation < journal);
}

#[test]
fn manifest_readiness_acl_and_rust_pins_advance_together() {
    let manifest = dollar_block("patch_schema_manifest");
    let readiness = dollar_block("patch_readiness");
    let postflight = dollar_block("postflight");
    for source in [MIGRATION, CONTRACT_SOURCE, SECURITY_SUPPORT_SOURCE] {
        assert!(source.contains(FUNCTION_IDENTITY));
    }
    for digest in [
        CURRENT_MANIFEST_CONTENT_DIGEST,
        CURRENT_MANIFEST_DEFINITION_DIGEST,
        CURRENT_READINESS_DIGEST,
        CURRENT_EXECUTION_DEFINITION_DIGEST,
        CURRENT_ACTION_RECORD_DIGEST,
    ] {
        assert!(MIGRATION.contains(digest), "{digest}");
    }
    assert!(manifest.contains("RETURN observed_count = 769"));
    assert!(
        readiness.contains("runtime_startup_stale_live_execution_readiness_allowlist_patch_drift")
    );
    assert!(postflight.contains("pg_catalog.has_function_privilege"));
    assert!(CONTRACT_SOURCE.contains("OPERATION_CAPABILITY_IDENTITIES_V1: [&str; 29]"));
    assert!(CONTRACT_SOURCE.contains("capabilities.clone().count() != 31"));
    assert!(SECURITY_SUPPORT_SOURCE.contains("const EXECUTOR_FUNCTIONS: [&str; 31]"));
    for source in [CONTRACT_SOURCE, DATABASE_SOURCE, SECURITY_SUPPORT_SOURCE] {
        assert!(source.contains(LATEST_READINESS_DIGEST));
    }
    assert!(!CONTRACT_SOURCE.contains(PREVIOUS_READINESS_DIGEST));
    assert!(!DATABASE_SOURCE.contains(PREVIOUS_READINESS_DIGEST));
    assert!(!SECURITY_SUPPORT_SOURCE.contains(PREVIOUS_READINESS_DIGEST));
}
