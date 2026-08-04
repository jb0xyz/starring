use crate::database_capability::{
    begin_bounded_database_probe, begin_scoped_database_readiness, load_scoped_database_topology,
    verify_scoped_executable_allowlist, verify_scoped_global_user_object_deny,
    verify_scoped_schema_trust, ScopedDatabaseProbeModeV1, ScopedDatabaseTopologyV1,
    ScopedFunctionContractV1, ScopedRelationContractV1,
};
use crate::runtime_convergence_readiness::RUNTIME_ATTEMPT_SCHEMA_CONTRACT_QUERY;
use crate::ProductDecisionReadinessErrorV1;

use super::apply_contract::{
    BEGIN_RUNTIME_DRAIN_ARGUMENTS, BEGIN_RUNTIME_DRAIN_FUNCTION, BEGIN_RUNTIME_DRAIN_RESULT,
    CONSUME_RUNTIME_DRAIN_ARGUMENTS, CONSUME_RUNTIME_DRAIN_FUNCTION, CONSUME_RUNTIME_DRAIN_RESULT,
    DATABASE_IDENTITY_FUNCTION, FINALIZE_ARGUMENTS, FINALIZE_FUNCTION, FINALIZE_RESULT,
    KEYRING_COVERAGE_ARGUMENTS, KEYRING_COVERAGE_FUNCTION, KEYRING_COVERAGE_RESULT, LOCK_ARGUMENTS,
    LOCK_FUNCTION, LOCK_RESULT, TARGET_ARTIFACT_ARGUMENTS, TARGET_ARTIFACT_FUNCTION,
    TARGET_ARTIFACT_RESULT, TOPOLOGY_QUERY,
};
use super::digest::keyring_coverage_identity;
use super::readiness::{map_readiness, readiness_database, verify_approval_support_contract};
use super::store::PostgresProductDecisions;

const FUNCTIONS: [ScopedFunctionContractV1<'static>; 7] = [
    ScopedFunctionContractV1::scalar(DATABASE_IDENTITY_FUNCTION, "text"),
    ScopedFunctionContractV1::set_plpgsql_named(LOCK_FUNCTION, LOCK_RESULT, 1.0, LOCK_ARGUMENTS),
    ScopedFunctionContractV1::set_named(
        TARGET_ARTIFACT_FUNCTION,
        TARGET_ARTIFACT_RESULT,
        1.0,
        TARGET_ARTIFACT_ARGUMENTS,
    ),
    ScopedFunctionContractV1::set_plpgsql_named(
        FINALIZE_FUNCTION,
        FINALIZE_RESULT,
        1.0,
        FINALIZE_ARGUMENTS,
    ),
    ScopedFunctionContractV1::set_plpgsql_named(
        KEYRING_COVERAGE_FUNCTION,
        KEYRING_COVERAGE_RESULT,
        1.0,
        KEYRING_COVERAGE_ARGUMENTS,
    ),
    ScopedFunctionContractV1::set_plpgsql_named(
        BEGIN_RUNTIME_DRAIN_FUNCTION,
        BEGIN_RUNTIME_DRAIN_RESULT,
        1.0,
        BEGIN_RUNTIME_DRAIN_ARGUMENTS,
    ),
    ScopedFunctionContractV1::set_plpgsql_named(
        CONSUME_RUNTIME_DRAIN_FUNCTION,
        CONSUME_RUNTIME_DRAIN_RESULT,
        1.0,
        CONSUME_RUNTIME_DRAIN_ARGUMENTS,
    ),
];
const RELATIONS: [ScopedRelationContractV1<'static>; 25] = [
    ScopedRelationContractV1::ordinary_without_rls("public.product_control_plane_identity"),
    ScopedRelationContractV1::ordinary_without_rls("public.activation_requests"),
    ScopedRelationContractV1::ordinary_without_rls("public.activation_request_approvals"),
    ScopedRelationContractV1::ordinary_without_rls("public.authoring_promotions"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_tenants"),
    ScopedRelationContractV1::ordinary_without_rls("public.automation_installations"),
    ScopedRelationContractV1::ordinary_without_rls(
        "public.automation_installation_authority_versions",
    ),
    ScopedRelationContractV1::ordinary_without_rls("public.product_principals"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_auth_sessions"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_action_receipts"),
    ScopedRelationContractV1::ordinary_without_rls(
        "public.product_action_receipt_idempotency_aliases",
    ),
    ScopedRelationContractV1::ordinary_without_rls("public.product_audit_events"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_action_receipt_audit_evidence"),
    ScopedRelationContractV1::ordinary_without_rls("public.automation_ruleset_activations"),
    ScopedRelationContractV1::ordinary_without_rls("public.automation_ruleset_versions"),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_deployments"),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_product_operations_v2"),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_drain_intents_v2"),
    ScopedRelationContractV1::ordinary_without_rls(
        "public.runtime_product_drain_terminal_actions_v2",
    ),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_certification_operations_v2"),
    ScopedRelationContractV1::ordinary_without_rls(
        "public.runtime_certification_operation_terminals_v2",
    ),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_slot_writer_fences_v2"),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_writer_fence"),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_serving_leases"),
    ScopedRelationContractV1::ordinary_without_rls("public.runtime_attestations"),
];
const PROBE_SESSION_DIGEST: [u8; 32] = [41_u8; 32];
const PROBE_SUBJECT_DIGEST: [u8; 32] = [97_u8; 32];
const LOCK_PROBE_QUERY: &str = "SELECT outcome, exact_replay, requires_commit, \
    resulting_revision, resulting_state, deployment_id, desired_target_digest, \
    locked_projection FROM public.starring_product_apply_lock_v1( \
    'probe_tenant', 'probe_installation', pg_catalog.repeat('0', 64), 1, \
    pg_catalog.repeat('1', 64), 'probe_principal', $1, $2, '1', '1', '1', \
    'invalid', 1, pg_catalog.repeat('2', 64), pg_catalog.repeat('3', 64), \
    TIMESTAMPTZ '2000-01-01T00:00:00Z', TIMESTAMPTZ '2000-01-01T00:00:01Z', \
    '8', TRUE, 'probe_request', pg_catalog.repeat('4', 64), \
    ARRAY[pg_catalog.repeat('4', 64)], ARRAY['probe_key'], \
    ARRAY[pg_catalog.repeat('5', 64)], 'probe_key', pg_catalog.repeat('6', 64), \
    'probe_receipt', 'probe_audit', 'probe_attempt', 'probe_deployment')";
const ARTIFACT_PROBE_QUERY: &str = "SELECT pg_catalog.count(*) \
    FROM public.starring_product_apply_target_artifact_v1( \
    'probe_tenant', 'probe_installation', pg_catalog.repeat('0', 64), \
    'probe_principal', $1, '1', '1')";
const FINALIZE_PROBE_QUERY: &str = "SELECT outcome, resulting_revision, resulting_state, \
    exact_replay, guild_id, deployment_id, desired_target_digest \
    FROM public.starring_product_apply_finalize_v1( \
    'probe_tenant', 'probe_installation', pg_catalog.repeat('0', 64), 1, \
    pg_catalog.repeat('1', 64), 'probe_principal', $1, $2, '1', '1', '1', \
    'apply', 1, pg_catalog.repeat('2', 64), pg_catalog.repeat('3', 64), \
    TIMESTAMPTZ '2000-01-01T00:00:00Z', TIMESTAMPTZ '2000-01-01T00:00:01Z', \
    '8', TRUE, 'probe_request', pg_catalog.repeat('4', 64), \
    ARRAY[pg_catalog.repeat('4', 64)], ARRAY['probe_key'], \
    ARRAY[pg_catalog.repeat('5', 64)], 'probe_key', pg_catalog.repeat('6', 64), \
    'probe_receipt', 'probe_audit', 'probe_attempt', 'probe_deployment', \
    '{}'::JSONB, pg_catalog.repeat('7', 64), 'null'::JSONB, '{}'::JSONB, '[]'::JSONB)";
const BEGIN_RUNTIME_DRAIN_PROBE_QUERY: &str = "SELECT outcome, \
    locked_snapshot IS NULL AND observed_at IS NULL \
    AND product_tenant_id IS NULL AND product_installation_id IS NULL \
    AND product_deployment_id IS NULL AND product_expected_revision IS NULL \
    AND product_operation_id IS NULL AND product_expected_target IS NULL \
    AND product_mutation_request_bytes IS NULL AND product_mutation_digest IS NULL \
    AND drain_tenant_id IS NULL AND drain_installation_id IS NULL \
    AND drain_deployment_id IS NULL AND drain_slot_guild_id IS NULL \
    AND drain_slot_ruleset_key IS NULL AND drain_expected_revision IS NULL \
    AND drain_intent_id IS NULL AND drain_intent_request_bytes IS NULL \
    AND drain_intent_digest IS NULL AND intent_revision IS NULL AND intent_state IS NULL \
    AND canonical_state_bytes IS NULL AND canonical_state_digest IS NULL \
    AND writer_epoch_before IS NULL AND writer_epoch_after IS NULL \
    AND pending_drain_intent_id IS NULL AND pending_product_operation_id IS NULL \
    AND pending_tenant_id IS NULL AND pending_installation_id IS NULL \
    AND pending_deployment_id IS NULL AND pending_expected_revision IS NULL \
    AND pending_marked_at IS NULL AS payload_empty \
    FROM public.starring_product_apply_begin_runtime_drain_v2( \
    'probe_tenant', 'probe_installation', pg_catalog.repeat('0', 64), 1, \
    pg_catalog.repeat('1', 64), 'probe_principal', $1, $2, '1', '1', '1', \
    'invalid', 1, pg_catalog.repeat('2', 64), pg_catalog.repeat('3', 64), \
    TIMESTAMPTZ '2000-01-01T00:00:00Z', TIMESTAMPTZ '2000-01-01T00:00:01Z', \
    '8', TRUE, 'probe_request', pg_catalog.repeat('4', 64), \
    ARRAY[pg_catalog.repeat('4', 64)], ARRAY['probe_key'], \
    ARRAY[pg_catalog.repeat('5', 64)], 'probe_key', pg_catalog.repeat('6', 64), \
    'probe_receipt', 'probe_audit', 'probe_attempt', 'probe_deployment', \
    '', '')";
const CONSUME_RUNTIME_DRAIN_PROBE_QUERY: &str = "SELECT outcome_name, preparation_ready, \
    exact_replay, requires_commit \
    FROM public.starring_product_apply_consume_runtime_drain_v2(\
        requested_phase => 'invalid', expected_tenant_id => 'probe_tenant', \
        expected_installation_id => 'probe_installation', \
        expected_promotion_id => pg_catalog.repeat('0', 64), \
        expected_product_revision => 1, \
        expected_payload_digest => pg_catalog.repeat('1', 64), \
        expected_principal_id => 'probe_principal', \
        expected_product_session_digest => $1, session_subject_digest => $2, \
        expected_acting_user_id => '1', expected_discord_application_id => '1', \
        expected_guild_id => '1', expected_capability => 'apply', \
        expected_authority_revision => 1, \
        expected_authority_payload_digest => pg_catalog.repeat('2', 64), \
        expected_authority_observation_digest => pg_catalog.repeat('3', 64), \
        expected_authority_observed_at => TIMESTAMPTZ '2000-01-01T00:00:00Z', \
        expected_authority_expires_at => TIMESTAMPTZ '2000-01-01T00:00:01Z', \
        expected_effective_permission_bits => '8', expected_guild_owner => TRUE, \
        product_request_id => 'probe_request', \
        active_idempotency_key_digest => pg_catalog.repeat('4', 64), \
        idempotency_key_digest_candidates => ARRAY[pg_catalog.repeat('4', 64)], \
        idempotency_digest_key_id_candidates => ARRAY['probe_key'], \
        idempotency_digest_key_fingerprint_candidates => \
            ARRAY[pg_catalog.repeat('5', 64)], \
        idempotency_digest_key_id => 'probe_key', \
        semantic_request_digest => pg_catalog.repeat('6', 64), \
        new_receipt_id => 'probe_receipt', new_audit_event_id => 'probe_audit', \
        new_apply_attempt_id => 'probe_attempt', \
        new_deployment_id => 'probe_deployment', \
        expected_drain_intent_id => pg_catalog.repeat('7', 32), \
        expected_source_intent_revision => 1, \
        expected_source_state_bytes => pg_catalog.convert_to('{}', 'UTF8'), \
        expected_source_state_digest => pg_catalog.repeat('8', 64), \
        expected_product_operation_id => pg_catalog.repeat('9', 32), \
        expected_source_deployment_id => 'probe_source', \
        expected_source_deployment_revision => 1, \
        proposed_terminal_action_id => pg_catalog.repeat('a', 64), \
        expected_preparation_token => '', \
        prepared_source_result_snapshot_bytes => ''::BYTEA, \
        prepared_source_result_snapshot_digest => '', \
        prepared_result_deployment_snapshot_bytes => ''::BYTEA, \
        prepared_result_deployment_snapshot_digest => '', \
        prepared_desired_target_digest => '', \
        prepared_activation_notices_bytes => ''::BYTEA)";
const APPLY_SUPPORT_CONTRACT_QUERY: &str = r#"
WITH common_owner AS (
    SELECT relation.relowner AS owner_oid
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.activation_requests')
), expected_trigger_definitions(
    relation_identity,
    function_identity,
    definition
) AS (
    VALUES
        ('public.automation_ruleset_activations',
            'public.assert_product_ruleset_slot_pointer()',
            'CREATE CONSTRAINT TRIGGER automation_ruleset_activations_assert_product_slot AFTER INSERT OR DELETE OR UPDATE ON public.automation_ruleset_activations DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.assert_product_ruleset_slot_pointer()'),
        ('public.runtime_deployments',
            'public.enforce_runtime_deployment_policy_shadow()',
            'CREATE TRIGGER runtime_deployments_policy_shadow_guard BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.enforce_runtime_deployment_policy_shadow()'),
        ('public.runtime_deployments',
            'public.guard_runtime_ruleset_artifact_transition()',
            'CREATE TRIGGER runtime_deployments_guard_ruleset_artifact_transition BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.guard_runtime_ruleset_artifact_transition()'),
        ('public.runtime_deployments',
            'public.reject_runtime_deployment_delete()',
            'CREATE TRIGGER runtime_deployments_reject_delete BEFORE DELETE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.reject_runtime_deployment_delete()'),
        ('public.runtime_deployments',
            'public.validate_runtime_deployment_projection()',
            'CREATE TRIGGER runtime_deployments_validate_projection BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.validate_runtime_deployment_projection()'),
        ('public.runtime_deployments',
            'public.validate_runtime_convergence_attempt_projection()',
            'CREATE TRIGGER runtime_deployments_validate_convergence_attempt BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.validate_runtime_convergence_attempt_projection()')
), expected_triggers AS (
    SELECT pg_catalog.to_regclass(expected.relation_identity) AS relation_oid,
        pg_catalog.to_regprocedure(expected.function_identity) AS function_oid,
        expected.definition
    FROM expected_trigger_definitions AS expected
), actual_triggers AS (
    SELECT trigger_row.oid AS trigger_oid,
        trigger_row.tgrelid AS relation_oid,
        trigger_row.tgfoid AS function_oid,
        trigger_row.tgenabled::TEXT AS enabled,
        trigger_row.tgisinternal AS internal,
        trigger_row.tgparentid = 0
            AND trigger_row.tgconstrrelid = 0
            AND trigger_row.tgconstrindid = 0
            AND pg_catalog.cardinality(trigger_row.tgattr) = 0
            AND trigger_row.tgnargs = 0
            AND pg_catalog.octet_length(trigger_row.tgargs) = 0
            AND trigger_row.tgoldtable IS NULL
            AND trigger_row.tgnewtable IS NULL
            AND (
                (
                    trigger_row.tgconstraint = 0
                    AND NOT trigger_row.tgdeferrable
                    AND NOT trigger_row.tginitdeferred
                    AND constraint_row.oid IS NULL
                ) OR (
                    trigger_row.tgconstraint <> 0
                    AND constraint_row.contype = 't'
                    AND constraint_row.conname = trigger_row.tgname
                    AND constraint_row.conrelid = trigger_row.tgrelid
                    AND constraint_row.condeferrable = trigger_row.tgdeferrable
                    AND constraint_row.condeferred = trigger_row.tginitdeferred
                    AND constraint_row.convalidated
                    AND constraint_row.conparentid = 0
                )
            ) AS structural_valid,
        pg_catalog.pg_get_triggerdef(trigger_row.oid, FALSE) AS definition
    FROM pg_catalog.pg_trigger AS trigger_row
    LEFT JOIN pg_catalog.pg_constraint AS constraint_row
        ON constraint_row.oid = trigger_row.tgconstraint
    WHERE (
        NOT trigger_row.tgisinternal
        AND trigger_row.tgrelid IN (
            SELECT DISTINCT expected.relation_oid
            FROM expected_triggers AS expected
        )
    ) OR trigger_row.tgfoid IN (
        SELECT DISTINCT expected.function_oid
        FROM expected_triggers AS expected
    )
), trigger_manifest AS (
    SELECT (SELECT pg_catalog.count(*) FROM expected_triggers) = 6
        AND (SELECT pg_catalog.count(*) FROM actual_triggers) = 6
        AND NOT EXISTS (
            SELECT 1
            FROM expected_triggers AS expected
            FULL JOIN actual_triggers AS actual
                ON actual.relation_oid = expected.relation_oid
                AND actual.function_oid = expected.function_oid
                AND actual.definition = expected.definition
                AND actual.enabled = 'O'
                AND NOT actual.internal
                AND actual.structural_valid
            WHERE expected.relation_oid IS NULL
                OR actual.trigger_oid IS NULL
        ) AS valid
), expected_routines(
    function_identity,
    language_name,
    volatility,
    strict,
    security_definer,
    returns_set,
    rows_estimate,
    result_name
) AS (
    VALUES
        ('public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)',
            'plpgsql', 'v', TRUE, TRUE, TRUE, 1::REAL,
            'TABLE(outcome text, exact_replay boolean, requires_commit boolean, resulting_revision bigint, resulting_state text, deployment_id text, desired_target_digest text, locked_projection jsonb)'),
        ('public.starring_product_apply_lock_core_unfenced_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)',
            'plpgsql', 'v', TRUE, TRUE, TRUE, 1::REAL,
            'TABLE(outcome text, exact_replay boolean, requires_commit boolean, resulting_revision bigint, resulting_state text, deployment_id text, desired_target_digest text, locked_projection jsonb)'),
        ('public.starring_product_apply_authority_projection_v1(text,text,text,text,bytea,text,text,text,text,bigint,text,timestamp with time zone,timestamp with time zone,text,boolean,text)',
            'plpgsql', 'v', TRUE, TRUE, FALSE, 0::REAL, 'jsonb'),
        ('public.starring_product_ruleset_slot_exact_v1(text,text,text,text,bigint)',
            'sql', 's', TRUE, TRUE, FALSE, 0::REAL, 'boolean'),
        ('public.starring_runtime_lock_current_authority(text,text,text,text,bigint,text,text,bigint,text,bigint,text)',
            'plpgsql', 'v', FALSE, TRUE, FALSE, 0::REAL, 'text'),
        ('public.starring_runtime_current_mutation_clock()',
            'plpgsql', 'v', FALSE, TRUE, FALSE, 0::REAL,
            'timestamp with time zone'),
        ('public.assert_product_ruleset_slot_pointer()',
            'plpgsql', 'v', FALSE, TRUE, FALSE, 0::REAL, 'trigger'),
        ('public.enforce_runtime_deployment_policy_shadow()',
            'plpgsql', 'v', FALSE, TRUE, FALSE, 0::REAL, 'trigger'),
        ('public.guard_runtime_ruleset_artifact_transition()',
            'plpgsql', 'v', FALSE, TRUE, FALSE, 0::REAL, 'trigger'),
        ('public.reject_runtime_deployment_delete()',
            'plpgsql', 'v', FALSE, TRUE, FALSE, 0::REAL, 'trigger'),
        ('public.validate_runtime_deployment_projection()',
            'plpgsql', 'v', FALSE, TRUE, FALSE, 0::REAL, 'trigger'),
        ('public.validate_runtime_convergence_attempt_projection()',
            'plpgsql', 'v', FALSE, TRUE, FALSE, 0::REAL, 'trigger')
), routine_contract AS (
    SELECT pg_catalog.count(*) = 12
        AND pg_catalog.bool_and(COALESCE(
            function_row.oid IS NOT NULL
            AND function_row.proowner = common_owner.owner_oid
            AND function_row.prokind = 'f'
            AND function_row.provolatile = expected.volatility::"char"
            AND function_row.proisstrict = expected.strict
            AND function_row.proparallel = 'u'
            AND function_row.prosecdef = expected.security_definer
            AND NOT function_row.proleakproof
            AND function_row.pronargdefaults = 0
            AND function_row.provariadic = 0
            AND function_row.proretset = expected.returns_set
            AND function_row.prorows = expected.rows_estimate
            AND function_row.proconfig = ARRAY['search_path=pg_catalog']::TEXT[]
            AND language_row.lanname = expected.language_name
            AND pg_catalog.pg_get_function_result(function_row.oid)
                = expected.result_name
            AND NOT EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE privilege.grantee <> function_row.proowner
            ), FALSE)) AS valid
    FROM expected_routines AS expected
    CROSS JOIN common_owner
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.function_identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
), expected_private_routines(
    function_name,
    identity_arguments,
    language_name,
    volatility,
    parallel_safety,
    security_definer,
    returns_set,
    rows_estimate,
    result_name,
    configuration
) AS (
    VALUES
        ('starring_runtime_slot_writer_fence_lock_v2',
            'requested_slot_guild_id text, requested_slot_ruleset_key text',
            'plpgsql', 'v'::"char", 'u'::"char", FALSE, TRUE, 1::REAL,
            'TABLE(writer_epoch bigint, pending_drain_intent_id text, pending_product_operation_id text, pending_tenant_id text, pending_installation_id text, pending_deployment_id text, pending_expected_revision bigint, pending_marked_at timestamp with time zone, observed_at timestamp with time zone)',
            ARRAY['search_path=pg_catalog']::TEXT[]),
        ('starring_runtime_slot_writer_fence_begin_unsafe_v2',
            'requested_slot_guild_id text, requested_slot_ruleset_key text, requested_expected_epoch bigint',
            'plpgsql', 'v'::"char", 'u'::"char", FALSE, FALSE, 0::REAL, 'bigint',
            ARRAY['search_path=pg_catalog']::TEXT[]),
        ('starring_runtime_slot_writer_fence_mark_drain_v2',
            'requested_slot_guild_id text, requested_slot_ruleset_key text, requested_expected_epoch bigint, requested_drain_intent_id text, requested_product_operation_id text, requested_tenant_id text, requested_installation_id text, requested_deployment_id text, requested_expected_revision bigint',
            'plpgsql', 'v'::"char", 'u'::"char", FALSE, FALSE, 0::REAL, 'bigint',
            ARRAY['search_path=pg_catalog']::TEXT[]),
        ('starring_runtime_product_drain_first_apply_core_v2',
            'requested_operation_id text, requested_intent_id text, requested_tenant_id text, requested_installation_id text, requested_deployment_id text, requested_expected_revision bigint, requested_slot_guild_id text, requested_slot_ruleset_key text, requested_target_guild_id text, requested_target_ruleset_key text, requested_target_version bigint, requested_target_content_hash text, requested_target_binding_revision bigint, requested_target_binding_fingerprint text, requested_mutation_kind text, requested_product_semantic_request_digest text, requested_product_mutation_request_bytes bytea, requested_product_mutation_digest text, requested_drain_intent_request_bytes bytea, requested_drain_intent_digest text',
            'plpgsql', 'v'::"char", 'u'::"char", FALSE, TRUE, 1::REAL,
            'TABLE(outcome_name text, locked_snapshot jsonb, observed_at timestamp with time zone, product_tenant_id text, product_installation_id text, product_deployment_id text, product_expected_revision bigint, product_operation_id text, product_expected_target jsonb, product_mutation_request_bytes bytea, product_mutation_digest text, drain_tenant_id text, drain_installation_id text, drain_deployment_id text, drain_slot_guild_id text, drain_slot_ruleset_key text, drain_expected_revision bigint, drain_intent_id text, drain_intent_request_bytes bytea, drain_intent_digest text, intent_revision bigint, intent_state text)',
            ARRAY['search_path=pg_catalog, starring_runtime_private_v2']::TEXT[]),
        ('starring_runtime_product_mutation_bytes_v2',
            'requested_operation_id text, requested_tenant_id text, requested_installation_id text, requested_deployment_id text, requested_expected_revision bigint, requested_slot_guild_id text, requested_slot_ruleset_key text, requested_target_guild_id text, requested_target_ruleset_key text, requested_target_version bigint, requested_target_content_hash text, requested_target_binding_revision bigint, requested_target_binding_fingerprint text, requested_mutation_kind text, requested_product_semantic_request_digest text',
            'plpgsql', 'i'::"char", 's'::"char", FALSE, FALSE, 0::REAL, 'bytea',
            ARRAY['search_path=pg_catalog']::TEXT[]),
        ('starring_runtime_product_mutation_digest_v2',
            'canonical_payload bytea',
            'plpgsql', 'i'::"char", 's'::"char", FALSE, FALSE, 0::REAL, 'text',
            ARRAY['search_path=pg_catalog']::TEXT[]),
        ('starring_runtime_drain_intent_bytes_v2',
            'requested_intent_id text, requested_operation_id text, requested_tenant_id text, requested_installation_id text, requested_deployment_id text, requested_expected_revision bigint, requested_slot_guild_id text, requested_slot_ruleset_key text, requested_target_guild_id text, requested_target_ruleset_key text, requested_target_version bigint, requested_target_content_hash text, requested_target_binding_revision bigint, requested_target_binding_fingerprint text, requested_mutation_kind text, requested_product_semantic_request_digest text',
            'plpgsql', 'i'::"char", 's'::"char", FALSE, FALSE, 0::REAL, 'bytea',
            ARRAY['search_path=pg_catalog']::TEXT[]),
        ('starring_runtime_drain_intent_digest_v2',
            'canonical_payload bytea',
            'plpgsql', 'i'::"char", 's'::"char", FALSE, FALSE, 0::REAL, 'text',
            ARRAY['search_path=pg_catalog']::TEXT[]),
        ('starring_runtime_product_drain_source_supersession_exact_v2',
            'source_row public.runtime_deployments, result_snapshot jsonb, drain_row public.runtime_drain_intents_v2, result_deployment_snapshot jsonb, requested_terminal_time timestamp with time zone',
            'plpgsql', 'i'::"char", 's'::"char", FALSE, FALSE, 0::REAL, 'boolean',
            ARRAY['search_path=pg_catalog']::TEXT[]),
        ('starring_runtime_product_drain_consume_root_exact_v2',
            'product_row public.runtime_product_operations_v2, drain_row public.runtime_drain_intents_v2, source_row public.runtime_deployments, requested_product_operation_id text, requested_drain_intent_id text, requested_source_intent_revision bigint, requested_source_state_bytes bytea, requested_source_state_digest text, requested_semantic_request_digest text',
            'plpgsql', 'i'::"char", 's'::"char", FALSE, FALSE, 0::REAL, 'boolean',
            ARRAY['search_path=pg_catalog']::TEXT[]),
        ('starring_runtime_product_drain_supersede_source_v2',
            'requested_drain_intent_id text, requested_source_deployment_id text, requested_source_deployment_revision bigint, requested_source_result_snapshot_bytes bytea, requested_source_result_snapshot_digest text, requested_result_deployment_snapshot_bytes bytea, requested_result_deployment_snapshot_digest text, requested_terminal_time timestamp with time zone',
            'plpgsql', 'v'::"char", 'u'::"char", FALSE, FALSE, 0::REAL,
            'public.runtime_deployments',
            ARRAY['search_path=pg_catalog']::TEXT[]),
        ('starring_product_apply_authority_projection_at_v2',
            'expected_tenant_id text, expected_installation_id text, expected_promotion_id text, expected_principal_id text, expected_product_session_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, expected_authority_revision bigint, expected_authority_payload_digest text, expected_authority_observed_at timestamp with time zone, expected_authority_expires_at timestamp with time zone, expected_effective_permission_bits text, expected_guild_owner boolean, expected_payload_digest text, requested_authorization_clock timestamp with time zone',
            'plpgsql', 'v'::"char", 'u'::"char", TRUE, FALSE, 0::REAL,
            'jsonb', ARRAY['search_path=pg_catalog']::TEXT[]),
        ('starring_product_apply_consume_lock_core_v2',
            'expected_tenant_id text, expected_installation_id text, expected_promotion_id text, expected_product_revision bigint, expected_payload_digest text, expected_principal_id text, expected_product_session_digest bytea, session_subject_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, expected_authority_revision bigint, expected_authority_payload_digest text, expected_authority_observation_digest text, expected_authority_observed_at timestamp with time zone, expected_authority_expires_at timestamp with time zone, expected_effective_permission_bits text, expected_guild_owner boolean, product_request_id text, active_idempotency_key_digest text, idempotency_key_digest_candidates text[], idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[], idempotency_digest_key_id text, semantic_request_digest text, new_receipt_id text, new_audit_event_id text, new_apply_attempt_id text, new_deployment_id text, expected_source_deployment_id text',
            'plpgsql', 'v'::"char", 'u'::"char", TRUE, TRUE, 1::REAL,
            'TABLE(outcome text, exact_replay boolean, requires_commit boolean, resulting_revision bigint, resulting_state text, deployment_id text, desired_target_digest text, locked_projection jsonb)',
            ARRAY['search_path=pg_catalog']::TEXT[]),
        ('starring_product_apply_commit_unfenced_core_v2',
            'expected_tenant_id text, expected_installation_id text, expected_promotion_id text, expected_product_revision bigint, expected_payload_digest text, expected_principal_id text, expected_product_session_digest bytea, session_subject_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, expected_authority_revision bigint, expected_authority_payload_digest text, expected_authority_observation_digest text, expected_authority_observed_at timestamp with time zone, expected_authority_expires_at timestamp with time zone, expected_effective_permission_bits text, expected_guild_owner boolean, product_request_id text, active_idempotency_key_digest text, idempotency_key_digest_candidates text[], idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[], idempotency_digest_key_id text, semantic_request_digest text, new_receipt_id text, new_audit_event_id text, new_apply_attempt_id text, new_deployment_id text, locked_projection jsonb, prepared_desired_target_digest text, prepared_previous_runtime jsonb, prepared_snapshot jsonb, prepared_activation_notices jsonb, requested_mutation_clock timestamp with time zone, requested_manage_slot_fence boolean',
            'plpgsql', 'v'::"char", 'u'::"char", TRUE, TRUE, 1::REAL,
            'TABLE(outcome text, resulting_revision bigint, resulting_state text, exact_replay boolean, guild_id text, deployment_id text, desired_target_digest text)',
            ARRAY['search_path=pg_catalog']::TEXT[])
), private_routine_contract AS (
    SELECT pg_catalog.count(*) = 14
        AND pg_catalog.bool_and(COALESCE(
            namespace.oid IS NOT NULL
            AND namespace.nspowner = common_owner.owner_oid
            AND function_row.oid IS NOT NULL
            AND function_row.proowner = common_owner.owner_oid
            AND function_row.prokind = 'f'
            AND function_row.provolatile = expected.volatility
            AND function_row.proisstrict
            AND function_row.proparallel = expected.parallel_safety
            AND function_row.prosecdef = expected.security_definer
            AND NOT function_row.proleakproof
            AND function_row.pronargdefaults = 0
            AND function_row.provariadic = 0
            AND function_row.proretset = expected.returns_set
            AND function_row.prorows = expected.rows_estimate
            AND function_row.proconfig = expected.configuration
            AND language_row.lanname = expected.language_name
            AND pg_catalog.pg_get_function_identity_arguments(function_row.oid)
                = expected.identity_arguments
            AND pg_catalog.pg_get_function_result(function_row.oid)
                = expected.result_name
            AND NOT EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE privilege.grantee <> function_row.proowner
            ), FALSE)) AS valid
    FROM expected_private_routines AS expected
    CROSS JOIN common_owner
    LEFT JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.nspname = 'starring_runtime_private_v2'
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.pronamespace = namespace.oid
        AND function_row.proname = expected.function_name
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
)
SELECT trigger_manifest.valid
    AND routine_contract.valid
    AND private_routine_contract.valid
FROM trigger_manifest
CROSS JOIN routine_contract
CROSS JOIN private_routine_contract
"#;

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct ApplyLockProbeRow {
    outcome: String,
    exact_replay: bool,
    requires_commit: bool,
    resulting_revision: Option<i64>,
    resulting_state: Option<String>,
    deployment_id: Option<String>,
    desired_target_digest: Option<String>,
    locked_projection: Option<sqlx::types::Json<serde_json::Value>>,
}

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct ApplyFinalizeProbeRow {
    outcome: String,
    resulting_revision: Option<i64>,
    resulting_state: Option<String>,
    exact_replay: bool,
    guild_id: Option<String>,
    deployment_id: Option<String>,
    desired_target_digest: Option<String>,
}

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct ApplyBeginRuntimeDrainProbeRow {
    outcome: String,
    payload_empty: bool,
}

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct ApplyConsumeRuntimeDrainProbeRow {
    outcome_name: String,
    preparation_ready: bool,
    exact_replay: bool,
    requires_commit: bool,
}

impl PostgresProductDecisions {
    pub async fn verify_apply_executor_readiness(
        &self,
    ) -> Result<(), ProductDecisionReadinessErrorV1> {
        self.check_apply_executor_readiness().await.map(drop)
    }

    pub(super) async fn check_apply_executor_readiness(
        &self,
    ) -> Result<ScopedDatabaseTopologyV1, ProductDecisionReadinessErrorV1> {
        let timeout = self.config.statement_timeout();
        let mut metadata = begin_scoped_database_readiness(
            &self.pools.apply_executor,
            &timeout,
            &FUNCTIONS,
            &RELATIONS,
        )
        .await
        .map_err(map_readiness)?;
        verify_scoped_executable_allowlist(&mut metadata, &FUNCTIONS)
            .await
            .map_err(map_readiness)?;
        verify_scoped_global_user_object_deny(&mut metadata, &FUNCTIONS)
            .await
            .map_err(map_readiness)?;
        verify_scoped_schema_trust(&mut metadata, "public", DATABASE_IDENTITY_FUNCTION)
            .await
            .map_err(map_readiness)?;
        verify_scoped_schema_trust(
            &mut metadata,
            "starring_runtime_private_v2",
            DATABASE_IDENTITY_FUNCTION,
        )
        .await
        .map_err(map_readiness)?;
        verify_approval_support_contract(&mut metadata).await?;
        verify_apply_support_contract(&mut metadata).await?;
        let topology = load_scoped_database_topology(&mut metadata, TOPOLOGY_QUERY)
            .await
            .map_err(map_readiness)?;
        if let Err(error) = self.check_apply_keyring_coverage(&mut metadata).await {
            metadata.rollback().await.map_err(readiness_database)?;
            return Err(error);
        }
        metadata.commit().await.map_err(readiness_database)?;
        self.run_apply_probes().await?;
        Ok(topology)
    }

    async fn check_apply_keyring_coverage(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), ProductDecisionReadinessErrorV1> {
        let identity = keyring_coverage_identity(self.config.keyring());
        let outcomes = sqlx::query_scalar::<_, String>(
            "SELECT outcome \
             FROM public.starring_product_apply_keyring_coverage_v1($1, $2)",
        )
        .bind(&identity.key_ids)
        .bind(&identity.key_fingerprints)
        .fetch_all(&mut **transaction)
        .await
        .map_err(readiness_database)?;
        match outcomes.as_slice() {
            [outcome] if outcome == "ok" => Ok(()),
            [outcome] if outcome == "idempotency_keyring_incomplete" => {
                Err(ProductDecisionReadinessErrorV1::IncompleteCoverage)
            }
            _ => Err(ProductDecisionReadinessErrorV1::InvalidResult),
        }
    }

    async fn run_apply_probes(&self) -> Result<(), ProductDecisionReadinessErrorV1> {
        let mut transaction = begin_bounded_database_probe(
            &self.pools.apply_executor,
            &self.config.statement_timeout(),
            ScopedDatabaseProbeModeV1::SerializableReadWrite,
        )
        .await
        .map_err(map_readiness)?;
        let result = run_apply_probes(&mut transaction).await;
        transaction.rollback().await.map_err(readiness_database)?;
        result
    }
}

async fn verify_apply_support_contract(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), ProductDecisionReadinessErrorV1> {
    let valid = sqlx::query_scalar::<_, bool>(APPLY_SUPPORT_CONTRACT_QUERY)
        .fetch_one(&mut **transaction)
        .await
        .map_err(readiness_database)?;
    let attempt_schema_valid = sqlx::query_scalar::<_, bool>(RUNTIME_ATTEMPT_SCHEMA_CONTRACT_QUERY)
        .fetch_one(&mut **transaction)
        .await
        .map_err(readiness_database)?;
    if !valid || !attempt_schema_valid {
        return Err(ProductDecisionReadinessErrorV1::ContractMismatch);
    }
    Ok(())
}

async fn run_apply_probes(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), ProductDecisionReadinessErrorV1> {
    let lock_rows = sqlx::query_as::<_, ApplyLockProbeRow>(LOCK_PROBE_QUERY)
        .bind(PROBE_SESSION_DIGEST.as_slice())
        .bind(PROBE_SUBJECT_DIGEST.as_slice())
        .fetch_all(&mut **transaction)
        .await
        .map_err(readiness_database)?;
    if !matches!(
        lock_rows.as_slice(),
        [row]
            if lock_probe_row_is_exact(row, "invalid_input")
                || lock_probe_row_is_exact(row, "runtime_writer_fenced")
    ) {
        return Err(ProductDecisionReadinessErrorV1::ContractMismatch);
    }
    let artifact_count = sqlx::query_scalar::<_, i64>(ARTIFACT_PROBE_QUERY)
        .bind(PROBE_SESSION_DIGEST.as_slice())
        .fetch_one(&mut **transaction)
        .await
        .map_err(readiness_database)?;
    if artifact_count != 0 {
        return Err(ProductDecisionReadinessErrorV1::ContractMismatch);
    }
    let finalize_rows = sqlx::query_as::<_, ApplyFinalizeProbeRow>(FINALIZE_PROBE_QUERY)
        .bind(PROBE_SESSION_DIGEST.as_slice())
        .bind(PROBE_SUBJECT_DIGEST.as_slice())
        .fetch_all(&mut **transaction)
        .await
        .map_err(readiness_database)?;
    if finalize_rows
        != [ApplyFinalizeProbeRow {
            outcome: "lock_required".to_string(),
            resulting_revision: None,
            resulting_state: None,
            exact_replay: false,
            guild_id: None,
            deployment_id: None,
            desired_target_digest: None,
        }]
    {
        return Err(ProductDecisionReadinessErrorV1::ContractMismatch);
    }
    let begin_rows =
        sqlx::query_as::<_, ApplyBeginRuntimeDrainProbeRow>(BEGIN_RUNTIME_DRAIN_PROBE_QUERY)
            .bind(PROBE_SESSION_DIGEST.as_slice())
            .bind(PROBE_SUBJECT_DIGEST.as_slice())
            .fetch_all(&mut **transaction)
            .await
            .map_err(readiness_database)?;
    if !matches!(
        begin_rows.as_slice(),
        [ApplyBeginRuntimeDrainProbeRow {
            outcome,
            payload_empty: true,
        }] if outcome == "invalid_input" || outcome == "runtime_writer_fenced"
    ) {
        return Err(ProductDecisionReadinessErrorV1::ContractMismatch);
    }
    let consume_rows =
        sqlx::query_as::<_, ApplyConsumeRuntimeDrainProbeRow>(CONSUME_RUNTIME_DRAIN_PROBE_QUERY)
            .bind(PROBE_SESSION_DIGEST.as_slice())
            .bind(PROBE_SUBJECT_DIGEST.as_slice())
            .fetch_all(&mut **transaction)
            .await
            .map_err(readiness_database)?;
    if consume_rows
        != [ApplyConsumeRuntimeDrainProbeRow {
            outcome_name: "invalid_input".to_string(),
            preparation_ready: false,
            exact_replay: false,
            requires_commit: false,
        }]
    {
        return Err(ProductDecisionReadinessErrorV1::ContractMismatch);
    }
    Ok(())
}

fn lock_probe_row_is_exact(row: &ApplyLockProbeRow, outcome: &str) -> bool {
    row == &ApplyLockProbeRow {
        outcome: outcome.to_string(),
        exact_replay: false,
        requires_commit: false,
        resulting_revision: None,
        resulting_state: None,
        deployment_id: None,
        desired_target_digest: None,
        locked_projection: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_manifest_is_exact_and_nonempty() {
        assert_eq!(FUNCTIONS.len(), 7);
        assert_eq!(RELATIONS.len(), 25);
        assert_eq!(PROBE_SESSION_DIGEST.len(), 32);
        assert_eq!(PROBE_SUBJECT_DIGEST.len(), 32);
    }
}
