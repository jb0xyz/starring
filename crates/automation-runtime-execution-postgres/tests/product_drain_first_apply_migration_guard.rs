const MIGRATION: &str =
    include_str!("../../../migrations/202607240009_add_product_drain_first_apply_core.sql");
const FIRST_APPLY_IDENTITY: &str = "starring_runtime_private_v2.\
starring_runtime_product_drain_first_apply_core_v2(\
text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text,\
bytea,text,bytea,text)";

fn first_apply_body() -> &'static str {
    MIGRATION
        .split(
            "CREATE FUNCTION \
starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(",
        )
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap()
}

fn trigger_body() -> &'static str {
    MIGRATION
        .split("CREATE OR REPLACE FUNCTION public.reject_runtime_product_drain_mutation()")
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap()
}

#[test]
fn first_apply_migration_is_private_owner_only_atomic_and_comment_free() {
    assert!(!MIGRATION.contains("--"));
    assert!(!MIGRATION.contains("/*"));
    assert!(!MIGRATION.contains("//"));
    assert!(MIGRATION.contains(FIRST_APPLY_IDENTITY));
    for required in [
        "VOLATILE",
        "STRICT",
        "PARALLEL UNSAFE",
        "SECURITY INVOKER",
        "SET search_path = pg_catalog",
        "ROWS 1",
        "REVOKE ALL PRIVILEGES ON FUNCTION",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert!(!MIGRATION.contains(
        "GRANT EXECUTE ON FUNCTION \
starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2"
    ));
    assert!(!MIGRATION.contains("DISABLE TRIGGER"));
    assert!(!MIGRATION.contains("session_replication_role"));
}

#[test]
fn first_apply_rebuilds_and_compares_both_roots_before_writing() {
    let body = first_apply_body();
    let product_build = body
        .find("starring_runtime_product_mutation_bytes_v2")
        .unwrap();
    let product_compare = body
        .find("runtime_product_drain_first_apply_input_invalid")
        .unwrap();
    let drain_build = body.find("starring_runtime_drain_intent_bytes_v2").unwrap();
    let product_insert = body
        .find("INSERT INTO public.runtime_product_operations_v2")
        .unwrap();
    let drain_insert = body
        .find("INSERT INTO public.runtime_drain_intents_v2")
        .unwrap();
    assert!(product_build < product_compare);
    assert!(product_compare < product_insert);
    assert!(drain_build < product_insert);
    assert!(product_insert < drain_insert);
    for required in [
        "starring_runtime_product_mutation_digest_v2",
        "starring_runtime_drain_intent_digest_v2",
        "requested_product_mutation_request_bytes",
        "requested_product_mutation_digest",
        "requested_drain_intent_request_bytes",
        "requested_drain_intent_digest",
        "IS DISTINCT FROM",
    ] {
        assert!(body.contains(required), "{required}");
    }
    for forbidden in [
        "to_json",
        "row_to_json",
        "product_mutation_digest_v2(requested_",
        "drain_intent_digest_v2(requested_",
    ] {
        assert!(!body.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn first_apply_uses_the_global_serializable_lock_order() {
    let body = first_apply_body();
    let isolation = body
        .find("pg_catalog.current_setting('transaction_isolation')")
        .unwrap();
    let writer = body
        .find("pg_catalog.pg_advisory_xact_lock_shared")
        .unwrap();
    let fence = body
        .find("FROM public.runtime_writer_fence AS fence")
        .unwrap();
    let slot = body.find("pg_catalog.pg_advisory_xact_lock(").unwrap();
    let deployment = body
        .find("FROM public.runtime_deployments AS deployment")
        .unwrap();
    let product = body
        .find("FROM public.runtime_product_operations_v2 AS product")
        .unwrap();
    let drain = body
        .find("FROM public.runtime_drain_intents_v2 AS drain")
        .unwrap();
    let product_insert = body
        .find("INSERT INTO public.runtime_product_operations_v2")
        .unwrap();
    assert!(isolation < writer);
    assert!(writer < fence);
    assert!(fence < slot);
    assert!(slot < deployment);
    assert!(deployment < product);
    assert!(product < drain);
    assert!(drain < product_insert);
    assert!(body.contains("<> 'serializable'"));
    assert!(body.contains("FOR UPDATE"));
    assert!(body.contains("'starring-runtime-writer-fence-v1'"));
    assert!(body.contains("'starring-runtime-serving-slot-v1:'"));
    assert!(body.contains("fence_state IS DISTINCT FROM 'open'"));
    assert!(body.contains("deployment_row.phase NOT IN ('awaiting_gateway_ready', 'live')"));
    let complete_pair = body.find("IF product_count = 1 THEN").unwrap();
    let replay = body.find("outcome_name := 'replayed'").unwrap();
    let exact_deployment = body.find("IF deployment_row.revision").unwrap();
    assert!(complete_pair < replay);
    assert!(replay < exact_deployment);
}

#[test]
fn first_apply_gate_is_one_shot_id_bound_and_always_cleared() {
    let body = first_apply_body();
    let trigger = trigger_body();
    for guc in [
        "starring.runtime_product_drain_first_apply_stage_v2",
        "starring.runtime_product_drain_first_apply_product_operation_id_v2",
        "starring.runtime_product_drain_first_apply_drain_intent_id_v2",
    ] {
        assert!(body.contains(guc), "{guc}");
        assert!(trigger.contains(guc), "{guc}");
    }
    for stage in ["product_insert", "drain_insert"] {
        assert!(body.contains(&format!("'{stage}'")), "{stage}");
        assert!(trigger.contains(&format!("'{stage}'")), "{stage}");
    }
    assert_eq!(trigger.matches("TG_OP = 'INSERT'").count(), 1);
    assert!(trigger.contains("'public.runtime_product_operations_v2'"));
    assert!(trigger.contains("'public.runtime_drain_intents_v2'"));
    assert!(trigger.contains("pg_catalog.set_config"));
    assert!(trigger.contains("runtime_product_drain_mutation_rejected"));
    assert!(body.contains("runtime_product_drain_first_apply_gate_precondition_invalid"));
    assert!(body.contains("runtime_product_drain_first_apply_gate_cleanup_invalid"));
    assert!(body.matches("pg_catalog.set_config").count() >= 6);
    assert!(!body.contains("ALTER TABLE"));
}

#[test]
fn first_apply_outcomes_are_closed_and_unique_races_are_explicitly_normalized() {
    let body = first_apply_body();
    for outcome in [
        "inserted",
        "replayed",
        "diverged",
        "identifier_conflict",
        "persistence_corrupt",
    ] {
        assert!(body.contains(&format!("'{outcome}'")), "{outcome}");
    }
    assert!(!body.contains("'adopted'"));
    assert!(body.contains("WHEN unique_violation THEN"));
    assert!(body.contains("GET STACKED DIAGNOSTICS"));
    assert!(body.contains("runtime_product_drain_first_apply_serialization_conflict"));
    assert!(body.contains("ERRCODE = '40001'"));
    for constraint in [
        "runtime_product_operations_v2_pkey",
        "runtime_product_operations_v2_natural_unique",
        "runtime_product_operations_v2_pair_unique",
        "runtime_drain_intents_v2_pkey",
        "runtime_drain_intents_v2_natural_unique",
        "runtime_drain_intents_v2_product_unique",
    ] {
        assert!(body.contains(constraint), "{constraint}");
    }
    assert!(body.contains("RAISE;"));
}

#[test]
fn first_apply_validates_complete_roots_against_locked_database_truth() {
    let body = first_apply_body();
    let start = body.find("stored_product_digest :=").unwrap();
    let end = body
        .find("IF product_row.product_operation_id = requested_operation_id")
        .unwrap();
    let integrity = &body[start..end];
    for required in [
        "deployment_row.guild_id",
        "deployment_row.ruleset_key",
        "deployment_row.target_version",
        "deployment_row.target_content_hash",
        "deployment_row.binding_revision",
        "deployment_row.binding_fingerprint",
        "deployment_row.revision < product_row.expected_revision",
    ] {
        assert!(integrity.contains(required), "{required}");
    }
    assert!(!integrity.contains("requested_target_"));
    let replay = body.find("outcome_name := 'replayed'").unwrap();
    let diverged = body.find("outcome_name := 'diverged'").unwrap();
    assert!(end < replay);
    assert!(replay < diverged);
}

#[test]
fn first_apply_extends_manifest_and_readiness_without_executor_capability_growth() {
    for required in [
        FIRST_APPLY_IDENTITY,
        "invalid_private_function_count",
        "invalid_core_count",
        "runtime_product_drain_first_apply_manifest_function_drift",
        "runtime_product_drain_first_apply_readiness_protected_drift",
        "runtime_product_drain_first_apply_readiness_acl_drift",
        "runtime_product_drain_first_apply_readiness_digest_drift",
        "runtime_product_drain_first_apply_postflight_drift",
        "public.starring_runtime_execution_schema_manifest_v1()",
        "public.starring_runtime_execution_database_readiness_v1()",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert!(!MIGRATION.contains("__MANIFEST"));
    assert!(!MIGRATION.contains("__READINESS"));
    assert!(!MIGRATION.contains("GRANT EXECUTE"));
    assert!(MIGRATION.contains("public.starring_runtime_product_drain_observe_v2"));
    assert!(MIGRATION.contains("public.reject_runtime_product_drain_mutation()"));
}
