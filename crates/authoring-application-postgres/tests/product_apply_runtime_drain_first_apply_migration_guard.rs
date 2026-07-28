const MIGRATION: &str =
    include_str!("../../../migrations/202607280002_compose_product_apply_runtime_drain_v2.sql");

const FUNCTION_IDENTITY: &str = "public.starring_product_apply_begin_runtime_drain_v2(\
text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,\
timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],\
text[],text,text,text,text,text,text,text,text)";

fn function_body() -> &'static str {
    MIGRATION
        .split("CREATE FUNCTION public.starring_product_apply_begin_runtime_drain_v2(")
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap()
}

fn block(tag: &str) -> &'static str {
    MIGRATION
        .split(&format!("DO ${tag}$"))
        .nth(1)
        .unwrap()
        .split(&format!("${tag}$;"))
        .next()
        .unwrap()
}

#[test]
fn migration_is_atomic_additive_comment_free_and_fail_closed() {
    assert!(!MIGRATION.contains("--"));
    assert!(!MIGRATION.contains("/*"));
    assert!(!MIGRATION.contains("//"));
    assert!(!MIGRATION.contains("DISABLE TRIGGER"));
    assert!(!MIGRATION.contains("session_replication_role"));
    assert!(!MIGRATION.contains("CREATE TABLE"));
    assert!(!MIGRATION.contains("ALTER TABLE"));
    assert!(!MIGRATION.contains("DROP TABLE"));
    assert!(!MIGRATION.contains("CREATE ROLE"));
    assert!(!MIGRATION.contains("ALTER DEFAULT PRIVILEGES"));

    let writer_lock = MIGRATION
        .find("pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)")
        .unwrap();
    let table_lock = MIGRATION.find("LOCK TABLE").unwrap();
    let preflight = MIGRATION.find("DO $preflight$").unwrap();
    let create = MIGRATION
        .find("CREATE FUNCTION public.starring_product_apply_begin_runtime_drain_v2")
        .unwrap();
    let revoke = MIGRATION.find("REVOKE ALL PRIVILEGES ON FUNCTION").unwrap();
    let grant = MIGRATION.find("DO $grant$").unwrap();
    let postflight = MIGRATION.find("DO $postflight$").unwrap();
    assert!(writer_lock < table_lock);
    assert!(table_lock < preflight);
    assert!(preflight < create);
    assert!(create < revoke);
    assert!(revoke < grant);
    assert!(grant < postflight);
    assert!(MIGRATION.contains("IN ACCESS EXCLUSIVE MODE"));
    assert!(MIGRATION
        .trim_end()
        .ends_with("RESET search_path;\nRESET statement_timeout;\nRESET lock_timeout;"));
}

#[test]
fn public_contract_has_exact_authentication_proposals_and_full_projection() {
    assert!(MIGRATION.contains(FUNCTION_IDENTITY));
    let body = function_body();
    let arguments = &body[..body.find("RETURNS TABLE(").unwrap()];
    assert_eq!(arguments.matches("expected_").count(), 18);
    assert!(arguments
        .trim_end()
        .ends_with("proposed_product_operation_id TEXT,\n    proposed_drain_intent_id TEXT\n)"));
    assert!(body.contains(
        "intent_revision BIGINT,\n    intent_state TEXT,\n    canonical_state_bytes BYTEA,\n    \
canonical_state_digest TEXT,\n    writer_epoch_before BIGINT,\n    writer_epoch_after BIGINT"
    ));
    for field in [
        "locked_snapshot JSONB",
        "observed_at TIMESTAMPTZ",
        "product_tenant_id TEXT",
        "product_installation_id TEXT",
        "product_deployment_id TEXT",
        "product_expected_revision BIGINT",
        "product_operation_id TEXT",
        "product_expected_target JSONB",
        "product_mutation_request_bytes BYTEA",
        "product_mutation_digest TEXT",
        "drain_tenant_id TEXT",
        "drain_installation_id TEXT",
        "drain_deployment_id TEXT",
        "drain_slot_guild_id TEXT",
        "drain_slot_ruleset_key TEXT",
        "drain_expected_revision BIGINT",
        "drain_intent_id TEXT",
        "drain_intent_request_bytes BYTEA",
        "drain_intent_digest TEXT",
        "pending_drain_intent_id TEXT",
        "pending_product_operation_id TEXT",
        "pending_tenant_id TEXT",
        "pending_installation_id TEXT",
        "pending_deployment_id TEXT",
        "pending_expected_revision BIGINT",
        "pending_marked_at TIMESTAMPTZ",
    ] {
        assert!(body.contains(field), "{field}");
    }
}

#[test]
fn capability_is_security_definer_owner_scoped_and_relation_blind() {
    for required in [
        "VOLATILE",
        "STRICT",
        "PARALLEL UNSAFE",
        "SECURITY DEFINER",
        "SET search_path = pg_catalog",
        "ROWS 1",
        "FROM PUBLIC",
        "privilege.privilege_type = 'EXECUTE'",
        "NOT privilege.is_grantable",
        "TO %I",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    for forbidden in [
        "GRANT SELECT",
        "GRANT INSERT",
        "GRANT UPDATE",
        "GRANT DELETE",
        "GRANT ALL",
        "ON TABLE",
        "ON ALL TABLES",
        "starring_runtime_private_v2 TO",
    ] {
        assert!(!MIGRATION.contains(forbidden), "{forbidden}");
    }
    let grant = block("grant");
    assert!(grant.contains("starring_product_apply_lock_v1"));
    assert!(grant.contains("GRANT EXECUTE ON FUNCTION"));
    assert!(grant.contains(FUNCTION_IDENTITY));
    assert!(block("preflight").contains("invalid_relation_acl_count"));
    assert!(block("preflight").contains("invalid_private_schema_acl_count"));
    assert!(block("postflight").contains("invalid_relation_acl_count"));
    assert!(block("postflight").contains("invalid_private_schema_acl_count"));
}

#[test]
fn authenticated_lock_is_exactly_revalidated_before_any_proposal_is_used() {
    let body = function_body();
    let auth = body
        .find("FROM public.starring_product_apply_lock_v1(")
        .unwrap();
    let required = body
        .find("apply_lock_row.outcome IS DISTINCT FROM 'runtime_drain_required'")
        .unwrap();
    let exact_shape = body
        .find("apply_lock_row.exact_replay IS DISTINCT FROM FALSE")
        .unwrap();
    let proposal = body
        .find("proposed_product_operation_id !~ '^[0-9a-f]{32}$'")
        .unwrap();
    let nullable_mode = body
        .find("observation_mode :=\n        proposed_product_operation_id = ''")
        .unwrap();
    let deployment = body
        .find("FROM public.runtime_deployments AS deployment")
        .unwrap();
    let persisted = body
        .find("FROM public.runtime_product_operations_v2 AS product")
        .unwrap();
    let core = body
        .find(
            "FROM \
starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(",
        )
        .unwrap();
    assert!(auth < required);
    assert!(required < exact_shape);
    assert!(exact_shape < nullable_mode);
    assert!(nullable_mode < proposal);
    assert!(proposal < deployment);
    assert!(deployment < persisted);
    assert!(deployment < core);
    for field in [
        "apply_lock_row.requires_commit IS DISTINCT FROM FALSE",
        "apply_lock_row.resulting_revision IS NOT NULL",
        "apply_lock_row.resulting_state IS NOT NULL",
        "apply_lock_row.deployment_id IS NOT NULL",
        "apply_lock_row.desired_target_digest IS NOT NULL",
        "apply_lock_row.locked_projection IS NOT NULL",
    ] {
        assert!(body.contains(field), "{field}");
    }
}

#[test]
fn product_boundary_supplies_random_ids_and_retry_adopts_the_persisted_pair() {
    let body = function_body();
    assert!(!body.contains("starring.runtime.product_drain.apply.operation.v2"));
    assert!(!body.contains("starring.runtime.product_drain.apply.intent.v2"));
    assert!(!body.contains("gen_random"));
    assert!(!body.contains("random()"));
    assert!(!body.contains("uuid_generate"));
    assert!(body.contains("proposed_product_operation_id"));
    assert!(body.contains("proposed_drain_intent_id"));
    assert!(body.contains("natural_product_count = 0 AND natural_drain_count = 0"));
    assert!(body.contains("natural_product_count = 1"));
    assert!(body.contains("natural_drain_count = 1"));
    assert!(body.contains(
        "persisted_drain_product_operation_id\n            IS NOT DISTINCT FROM \
persisted_product_operation_id"
    ));
    assert!(body.contains("selected_product_operation_id := persisted_product_operation_id"));
    assert!(body.contains("selected_drain_intent_id := persisted_drain_intent_id"));
    assert!(body.contains("selected_product_operation_id := proposed_product_operation_id"));
    assert!(body.contains("selected_drain_intent_id := proposed_drain_intent_id"));

    let product_lock = body
        .find("FROM public.runtime_product_operations_v2 AS product")
        .unwrap();
    let drain_lock = body
        .find("FROM public.runtime_drain_intents_v2 AS drain")
        .unwrap();
    let product_builder = body
        .find("starring_runtime_product_mutation_bytes_v2")
        .unwrap();
    let drain_builder = body.find("starring_runtime_drain_intent_bytes_v2").unwrap();
    let core = body
        .find("starring_runtime_product_drain_first_apply_core_v2")
        .unwrap();
    assert!(product_lock < drain_lock);
    assert!(drain_lock < product_builder);
    assert!(product_builder < drain_builder);
    assert!(drain_builder < core);
}

#[test]
fn empty_sentinel_observation_proves_absence_before_product_mints_candidates() {
    let body = function_body();
    let source = body
        .find("FROM public.runtime_deployments AS deployment")
        .unwrap();
    let product = body
        .find("FROM public.runtime_product_operations_v2 AS product")
        .unwrap();
    let drain = body
        .find("FROM public.runtime_drain_intents_v2 AS drain")
        .unwrap();
    let absence = body
        .find("natural_product_count = 0 AND natural_drain_count = 0")
        .unwrap();
    let absent = body
        .find("RETURN QUERY SELECT\n                'absent'")
        .unwrap();
    let candidate_validation = body
        .find("proposed_product_operation_id !~ '^[0-9a-f]{32}$'")
        .unwrap();
    let core = body
        .find("starring_runtime_product_drain_first_apply_core_v2")
        .unwrap();
    assert!(source < product);
    assert!(product < drain);
    assert!(candidate_validation < source);
    assert!(drain < absence);
    assert!(absence < absent);
    assert!(absent < core);
    assert!(body
        .contains("proposed_product_operation_id = ''\n        AND proposed_drain_intent_id = ''"));
    assert!(body.contains(
        "IF NOT observation_mode\n        AND (\n            \
proposed_product_operation_id !~ '^[0-9a-f]{32}$'"
    ));
    assert!(body.contains("slot_fence_before_row.pending_drain_intent_id IS NOT NULL"));
    let absent_end = absent
        + body[absent..]
            .find("            RETURN;\n        END IF;")
            .unwrap();
    let projection = &body[absent..absent_end];
    assert!(projection.contains("source_row.snapshot"));
    assert!(projection.contains("expected_target"));
    assert_eq!(
        projection
            .matches("slot_fence_before_row.writer_epoch")
            .count(),
        2
    );
    assert!(!projection.contains("selected_product_operation_id"));
    assert!(!projection.contains("selected_drain_intent_id"));
}

#[test]
fn lane_head_and_physical_fence_use_the_established_lock_order() {
    let body = function_body();
    let apply = body
        .find("FROM public.starring_product_apply_lock_v1(")
        .unwrap();
    let lane = body
        .find("FROM public.runtime_deployments AS deployment")
        .unwrap();
    let before = body.find("INTO STRICT slot_fence_before_row").unwrap();
    let product = body
        .find("FROM public.runtime_product_operations_v2 AS product")
        .unwrap();
    let drain = body
        .find("FROM public.runtime_drain_intents_v2 AS drain")
        .unwrap();
    let core = body.find("INTO STRICT first_apply_row").unwrap();
    let after = body.find("INTO STRICT slot_fence_after_row").unwrap();
    assert!(apply < lane);
    assert!(lane < before);
    assert!(before < product);
    assert!(product < drain);
    assert!(drain < core);
    assert!(core < after);
    assert!(body.contains("deployment.phase NOT IN ('superseded', 'cancelled')"));
    assert!(body.contains("source_row.phase NOT IN ('awaiting_gateway_ready', 'live')"));
    assert!(body.contains(
        "ORDER BY\n        deployment.runtime_generation DESC,\n        \
deployment.deployment_id DESC"
    ));
    assert!(body.contains("LIMIT 1\n    FOR UPDATE"));
}

#[test]
fn inserted_and_replayed_epochs_have_distinct_exact_contracts() {
    let body = function_body();
    let inserted = body
        .find("first_apply_row.outcome_name = 'inserted'")
        .unwrap();
    let replayed = body
        .find("first_apply_row.outcome_name = 'replayed'")
        .unwrap();
    assert!(inserted < replayed);
    for required in [
        "slot_fence_before_row.pending_drain_intent_id IS NOT NULL",
        "slot_fence_before_row.pending_product_operation_id IS NOT NULL",
        "slot_fence_before_row.writer_epoch\n                    = 9223372036854775807",
        "slot_fence_after_row.writer_epoch\n                    IS DISTINCT FROM \
slot_fence_before_row.writer_epoch + 1",
        "slot_fence_before_row.writer_epoch\n                    IS DISTINCT FROM \
slot_fence_after_row.writer_epoch",
        "slot_fence_before_row.pending_drain_intent_id\n                    IS DISTINCT FROM \
selected_drain_intent_id",
        "slot_fence_before_row.pending_product_operation_id\n                    IS DISTINCT FROM \
selected_product_operation_id",
        "slot_fence_before_row.pending_marked_at\n                    IS DISTINCT FROM \
slot_fence_after_row.pending_marked_at",
    ] {
        assert!(body.contains(required), "{required}");
    }
}

#[test]
fn success_revalidates_and_returns_canonical_root_state_and_fence() {
    let body = function_body();
    let after = body.find("INTO STRICT slot_fence_after_row").unwrap();
    let canonical = body.find("INTO canonical_drain_row").unwrap();
    let validation = body.find("first_apply_row.locked_snapshot").unwrap();
    let result = body.rfind("RETURN QUERY SELECT").unwrap();
    assert!(after < canonical);
    assert!(canonical < validation);
    assert!(validation < result);
    for required in [
        "canonical_drain_row.intent_revision\n            IS DISTINCT FROM \
first_apply_row.intent_revision",
        "canonical_drain_row.intent_state\n            IS DISTINCT FROM \
first_apply_row.intent_state",
        "pg_catalog.sha256(canonical_drain_row.canonical_state_bytes)",
        "canonical_drain_row.canonical_state_digest",
        "slot_fence_after_row.pending_drain_intent_id",
        "slot_fence_after_row.pending_product_operation_id",
        "slot_fence_after_row.pending_tenant_id",
        "slot_fence_after_row.pending_installation_id",
        "slot_fence_after_row.pending_deployment_id",
        "slot_fence_after_row.pending_expected_revision",
        "slot_fence_after_row.pending_marked_at",
    ] {
        assert!(body.contains(required), "{required}");
    }
    let success = &body[result..];
    assert!(success.contains("canonical_drain_row.canonical_state_bytes"));
    assert!(success.contains("canonical_drain_row.canonical_state_digest"));
    assert!(success.contains("slot_fence_before_row.writer_epoch"));
    assert!(success.contains("slot_fence_after_row.writer_epoch"));
}

#[test]
fn preflight_and_postflight_pin_signature_owner_search_path_acl_and_dependencies() {
    let preflight = block("preflight");
    let postflight = block("postflight");
    for required in [
        FUNCTION_IDENTITY,
        "product_apply_begin_runtime_drain_v2_preflight_drift",
        "starring_product_apply_lock_v1",
        "starring_runtime_product_drain_first_apply_core_v2",
        "starring_runtime_product_mutation_bytes_v2",
        "starring_runtime_product_mutation_digest_v2",
        "starring_runtime_drain_intent_bytes_v2",
        "starring_runtime_drain_intent_digest_v2",
        "starring_runtime_slot_writer_fence_lock_v2",
        "starring_runtime_slot_writer_fence_mark_drain_v2",
        "77ed38195d939f06a824d3bd7d1fac89643955b2027d0a366d1714eb55e29c99",
        "starring_runtime_execution_schema_manifest_v1",
    ] {
        assert!(preflight.contains(required), "{required}");
    }
    for required in [
        FUNCTION_IDENTITY,
        "product_apply_begin_runtime_drain_v2_postflight_drift",
        "invalid_dependency_count",
        "function_row.proowner = common_owner",
        "function_row.provolatile = 'v'",
        "function_row.proisstrict",
        "function_row.proparallel = 'u'",
        "function_row.prosecdef",
        "function_row.proretset",
        "function_row.prorows = 1::REAL",
        "ARRAY['search_path=pg_catalog']::TEXT[]",
        "pg_catalog.pg_get_function_arguments",
        "pg_catalog.pg_get_function_result",
        "f62a39f94d315b6f39b0e7d24b6dbd017e35fdcdc50e1e24ae49f1da1aa172b1",
        "starring_runtime_slot_writer_fence_mark_drain_v2",
        "77ed38195d939f06a824d3bd7d1fac89643955b2027d0a366d1714eb55e29c99",
    ] {
        assert!(postflight.contains(required), "{required}");
    }
}
