DO $scope$
DECLARE
    relation_count BIGINT;
    table_count BIGINT;
    rls_disabled_count BIGINT;
    owner_count BIGINT;
    common_owner OID;
    common_owner_name NAME;
    identity_count BIGINT;
    unsafe_schema_create_count BIGINT;
    invalid_function_count BIGINT;
    function_collision_count BIGINT;
    trigger_mismatch_count BIGINT;
    function_oid OID;
    function_name NAME;
    expected_signature TEXT;
    unexpected_grantee OID;
    unexpected_grantee_name NAME;
    probe_count BIGINT;
    probe_outcome TEXT;
    original_search_path TEXT;
    original_quote_all_identifiers TEXT;
    external_existing_signatures TEXT[] := ARRAY[
        'public.starring_product_apply_executor_database_identity_v1()',
        'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)',
        'public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)'
    ]::TEXT[];
    internal_helper_signatures TEXT[] := ARRAY[
        'public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)',
        'public.starring_product_apply_authority_projection_v1(text,text,text,text,bytea,text,text,text,text,bigint,text,timestamp with time zone,timestamp with time zone,text,boolean,text)',
        'public.starring_product_ruleset_slot_exact_v1(text,text,text,text,bigint)',
        'public.starring_runtime_desired_target_digest_v1(jsonb,bigint)',
        'public.starring_runtime_lock_current_authority(text,text,text,text,bigint,text,text,bigint,text,bigint,text)',
        'public.starring_runtime_current_mutation_clock()'
    ]::TEXT[];
    strict_trigger_signatures TEXT[] := ARRAY[
        'public.assert_atomic_product_apply_runtime_request()',
        'public.assert_no_committed_product_activation_applying()',
        'public.assert_product_approval_receipt_alias()',
        'public.assert_product_approval_receipt_audit()',
        'public.capture_product_action_receipt_audit_evidence()',
        'public.enforce_activation_approval_payload_binding()',
        'public.enforce_activation_approval_scope()',
        'public.enforce_product_action_receipt_alias_capacity()',
        'public.enforce_product_action_receipt_alias_retention()',
        'public.enforce_product_action_receipt_retention()',
        'public.enforce_product_activation_executor()',
        'public.enforce_product_activation_journal_link()',
        'public.enforce_product_activation_scope()',
        'public.guard_legacy_activation_product_slot()',
        'public.guard_product_activation_applied_record()',
        'public.guard_product_ruleset_artifact_transition()',
        'public.reject_activation_approval_mutation()',
        'public.reject_immutable_product_approval_row()'
    ]::TEXT[];
    nonstrict_trigger_signatures TEXT[] := ARRAY[
        'public.assert_product_ruleset_slot_pointer()',
        'public.enforce_runtime_deployment_policy_shadow()',
        'public.guard_runtime_ruleset_artifact_transition()',
        'public.reject_runtime_deployment_delete()',
        'public.validate_runtime_deployment_projection()'
    ]::TEXT[];
    protected_signatures TEXT[];
    expected_trigger_manifest JSONB := $manifest$
[
  {"relation":"public.activation_request_approvals","function":"public.enforce_activation_approval_payload_binding()","definition":"CREATE TRIGGER activation_request_approvals_enforce_payload_binding BEFORE INSERT OR UPDATE ON public.activation_request_approvals FOR EACH ROW EXECUTE FUNCTION public.enforce_activation_approval_payload_binding()"},
  {"relation":"public.activation_request_approvals","function":"public.enforce_activation_approval_scope()","definition":"CREATE TRIGGER activation_request_approvals_enforce_scope BEFORE INSERT OR UPDATE ON public.activation_request_approvals FOR EACH ROW EXECUTE FUNCTION public.enforce_activation_approval_scope()"},
  {"relation":"public.activation_request_approvals","function":"public.reject_activation_approval_mutation()","definition":"CREATE TRIGGER activation_request_approvals_reject_mutation BEFORE DELETE OR UPDATE ON public.activation_request_approvals FOR EACH ROW EXECUTE FUNCTION public.reject_activation_approval_mutation()"},
  {"relation":"public.activation_requests","function":"public.assert_atomic_product_apply_runtime_request()","definition":"CREATE CONSTRAINT TRIGGER activation_requests_assert_atomic_runtime_request AFTER INSERT OR UPDATE ON public.activation_requests DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.assert_atomic_product_apply_runtime_request()"},
  {"relation":"public.activation_requests","function":"public.assert_no_committed_product_activation_applying()","definition":"CREATE CONSTRAINT TRIGGER activation_requests_assert_no_product_applying AFTER INSERT OR UPDATE ON public.activation_requests DEFERRABLE INITIALLY DEFERRED FOR EACH ROW WHEN (((new.authority_kind = 'product_authoring'::text) AND (new.state = 'applying'::text))) EXECUTE FUNCTION public.assert_no_committed_product_activation_applying()"},
  {"relation":"public.activation_requests","function":"public.enforce_product_activation_executor()","definition":"CREATE TRIGGER activation_requests_enforce_product_executor BEFORE UPDATE ON public.activation_requests FOR EACH ROW EXECUTE FUNCTION public.enforce_product_activation_executor()"},
  {"relation":"public.activation_requests","function":"public.enforce_product_activation_journal_link()","definition":"CREATE TRIGGER activation_requests_enforce_product_journal_link BEFORE INSERT OR UPDATE ON public.activation_requests FOR EACH ROW EXECUTE FUNCTION public.enforce_product_activation_journal_link()"},
  {"relation":"public.activation_requests","function":"public.enforce_product_activation_scope()","definition":"CREATE TRIGGER activation_requests_enforce_product_scope BEFORE INSERT OR UPDATE ON public.activation_requests FOR EACH ROW EXECUTE FUNCTION public.enforce_product_activation_scope()"},
  {"relation":"public.activation_requests","function":"public.guard_legacy_activation_product_slot()","definition":"CREATE TRIGGER activation_requests_guard_legacy_product_slot BEFORE INSERT OR UPDATE ON public.activation_requests FOR EACH ROW EXECUTE FUNCTION public.guard_legacy_activation_product_slot()"},
  {"relation":"public.activation_requests","function":"public.guard_product_activation_applied_record()","definition":"CREATE TRIGGER activation_requests_guard_product_applied_record BEFORE UPDATE ON public.activation_requests FOR EACH ROW EXECUTE FUNCTION public.guard_product_activation_applied_record()"},
  {"relation":"public.activation_requests","function":"public.guard_product_ruleset_artifact_transition()","definition":"CREATE TRIGGER activation_requests_guard_ruleset_artifact_transition BEFORE UPDATE ON public.activation_requests FOR EACH ROW EXECUTE FUNCTION public.guard_product_ruleset_artifact_transition()"},
  {"relation":"public.automation_ruleset_activations","function":"public.assert_product_ruleset_slot_pointer()","definition":"CREATE CONSTRAINT TRIGGER automation_ruleset_activations_assert_product_slot AFTER INSERT OR DELETE OR UPDATE ON public.automation_ruleset_activations DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.assert_product_ruleset_slot_pointer()"},
  {"relation":"public.product_action_receipt_audit_evidence","function":"public.reject_immutable_product_approval_row()","definition":"CREATE TRIGGER product_action_receipt_audit_evidence_reject_mutation BEFORE DELETE OR UPDATE ON public.product_action_receipt_audit_evidence FOR EACH ROW EXECUTE FUNCTION public.reject_immutable_product_approval_row()"},
  {"relation":"public.product_action_receipt_idempotency_aliases","function":"public.enforce_product_action_receipt_alias_capacity()","definition":"CREATE TRIGGER product_action_receipt_idempotency_aliases_enforce_capacity BEFORE INSERT ON public.product_action_receipt_idempotency_aliases FOR EACH ROW EXECUTE FUNCTION public.enforce_product_action_receipt_alias_capacity()"},
  {"relation":"public.product_action_receipt_idempotency_aliases","function":"public.enforce_product_action_receipt_alias_retention()","definition":"CREATE TRIGGER product_action_receipt_idempotency_aliases_reject_mutation BEFORE DELETE OR UPDATE ON public.product_action_receipt_idempotency_aliases FOR EACH ROW EXECUTE FUNCTION public.enforce_product_action_receipt_alias_retention()"},
  {"relation":"public.product_action_receipts","function":"public.assert_product_approval_receipt_alias()","definition":"CREATE CONSTRAINT TRIGGER product_action_receipts_assert_approval_alias AFTER INSERT ON public.product_action_receipts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.assert_product_approval_receipt_alias()"},
  {"relation":"public.product_action_receipts","function":"public.assert_product_approval_receipt_audit()","definition":"CREATE CONSTRAINT TRIGGER product_action_receipts_assert_approval_audit AFTER INSERT ON public.product_action_receipts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.assert_product_approval_receipt_audit()"},
  {"relation":"public.product_action_receipts","function":"public.enforce_product_action_receipt_retention()","definition":"CREATE TRIGGER product_action_receipts_reject_mutation BEFORE DELETE OR UPDATE ON public.product_action_receipts FOR EACH ROW EXECUTE FUNCTION public.enforce_product_action_receipt_retention()"},
  {"relation":"public.product_audit_events","function":"public.capture_product_action_receipt_audit_evidence()","definition":"CREATE TRIGGER product_audit_events_capture_receipt_evidence AFTER INSERT ON public.product_audit_events FOR EACH ROW EXECUTE FUNCTION public.capture_product_action_receipt_audit_evidence()"},
  {"relation":"public.product_audit_events","function":"public.reject_immutable_product_approval_row()","definition":"CREATE TRIGGER product_audit_events_reject_mutation BEFORE DELETE OR UPDATE ON public.product_audit_events FOR EACH ROW EXECUTE FUNCTION public.reject_immutable_product_approval_row()"},
  {"relation":"public.runtime_deployments","function":"public.guard_runtime_ruleset_artifact_transition()","definition":"CREATE TRIGGER runtime_deployments_guard_ruleset_artifact_transition BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.guard_runtime_ruleset_artifact_transition()"},
  {"relation":"public.runtime_deployments","function":"public.enforce_runtime_deployment_policy_shadow()","definition":"CREATE TRIGGER runtime_deployments_policy_shadow_guard BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.enforce_runtime_deployment_policy_shadow()"},
  {"relation":"public.runtime_deployments","function":"public.reject_runtime_deployment_delete()","definition":"CREATE TRIGGER runtime_deployments_reject_delete BEFORE DELETE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.reject_runtime_deployment_delete()"},
  {"relation":"public.runtime_deployments","function":"public.validate_runtime_deployment_projection()","definition":"CREATE TRIGGER runtime_deployments_validate_projection BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.validate_runtime_deployment_projection()"}
]
$manifest$::JSONB;
BEGIN
    original_search_path := pg_catalog.current_setting('search_path');
    original_quote_all_identifiers :=
        pg_catalog.current_setting('quote_all_identifiers');
    PERFORM pg_catalog.set_config('search_path', 'pg_catalog', TRUE);
    PERFORM pg_catalog.set_config('quote_all_identifiers', 'off', TRUE);

    SELECT pg_catalog.count(relation.oid),
        pg_catalog.count(relation.oid) FILTER (WHERE relation.relkind = 'r'),
        pg_catalog.count(relation.oid) FILTER (
            WHERE NOT relation.relrowsecurity AND NOT relation.relforcerowsecurity
        ),
        pg_catalog.count(DISTINCT relation.relowner),
        pg_catalog.min(relation.relowner::BIGINT)::OID
    INTO relation_count, table_count, rls_disabled_count, owner_count, common_owner
    FROM (
        VALUES
            (pg_catalog.to_regclass('public.product_control_plane_identity')),
            (pg_catalog.to_regclass('public.activation_requests')),
            (pg_catalog.to_regclass('public.activation_request_approvals')),
            (pg_catalog.to_regclass('public.authoring_promotions')),
            (pg_catalog.to_regclass('public.product_tenants')),
            (pg_catalog.to_regclass('public.automation_installations')),
            (pg_catalog.to_regclass('public.automation_installation_authority_versions')),
            (pg_catalog.to_regclass('public.product_principals')),
            (pg_catalog.to_regclass('public.product_auth_sessions')),
            (pg_catalog.to_regclass('public.product_action_receipts')),
            (pg_catalog.to_regclass('public.product_action_receipt_idempotency_aliases')),
            (pg_catalog.to_regclass('public.product_audit_events')),
            (pg_catalog.to_regclass('public.product_action_receipt_audit_evidence')),
            (pg_catalog.to_regclass('public.automation_ruleset_activations')),
            (pg_catalog.to_regclass('public.automation_ruleset_versions')),
            (pg_catalog.to_regclass('public.runtime_deployments')),
            (pg_catalog.to_regclass('public.runtime_serving_leases')),
            (pg_catalog.to_regclass('public.runtime_attestations'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid;
    IF relation_count <> 18
        OR table_count <> 18
        OR rls_disabled_count <> 18
        OR owner_count <> 1
        OR common_owner IS NULL
    THEN
        RAISE EXCEPTION 'product apply relations require one non-RLS owner'
            USING ERRCODE = '55000';
    END IF;

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL THEN
        RAISE EXCEPTION 'product apply relation owner is unavailable'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO identity_count
    FROM public.product_control_plane_identity AS identity
    WHERE identity.singleton
        AND identity.database_identity IS NOT NULL
        AND identity.database_identity
            <> '00000000-0000-0000-0000-000000000000'::UUID
        AND identity.created_at IS NOT NULL;
    IF identity_count <> 1 THEN
        RAISE EXCEPTION 'product control plane identity is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO unsafe_schema_create_count
    FROM pg_catalog.pg_namespace AS namespace
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        namespace.nspacl,
        pg_catalog.acldefault('n', namespace.nspowner)
    )) AS privilege
    WHERE namespace.nspname = 'public'
        AND privilege.privilege_type = 'CREATE'
        AND privilege.grantee <> namespace.nspowner;
    IF unsafe_schema_create_count <> 0
        OR NOT EXISTS (
            SELECT 1
            FROM pg_catalog.pg_namespace AS namespace
            WHERE namespace.nspname = 'public'
                AND namespace.nspowner IN (
                    common_owner,
                    pg_catalog.to_regrole('pg_database_owner'),
                    (
                        SELECT database_row.datdba
                        FROM pg_catalog.pg_database AS database_row
                        WHERE database_row.datname = pg_catalog.current_database()
                    )
                )
        )
    THEN
        RAISE EXCEPTION 'product apply schema is not trusted'
            USING ERRCODE = '55000';
    END IF;

    IF pg_catalog.to_regrole(current_user) <> common_owner
        OR NOT pg_catalog.has_schema_privilege(
            common_owner_name,
            'public',
            'CREATE'
        )
    THEN
        RAISE EXCEPTION 'product apply migration requires the common owner'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_product_apply_executor_database_identity_v1()',
                '',
                'text',
                'sql',
                FALSE,
                0::REAL
            ),
            (
                'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)',
                'expected_tenant_id text, expected_installation_id text, expected_promotion_id text, expected_product_revision bigint, expected_payload_digest text, expected_principal_id text, expected_product_session_digest bytea, session_subject_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, expected_authority_revision bigint, expected_authority_payload_digest text, expected_authority_observation_digest text, expected_authority_observed_at timestamp with time zone, expected_authority_expires_at timestamp with time zone, expected_effective_permission_bits text, expected_guild_owner boolean, product_request_id text, active_idempotency_key_digest text, idempotency_key_digest_candidates text[], idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[], idempotency_digest_key_id text, semantic_request_digest text, new_receipt_id text, new_audit_event_id text, new_apply_attempt_id text, new_deployment_id text',
                'TABLE(outcome text, exact_replay boolean, requires_commit boolean, resulting_revision bigint, resulting_state text, deployment_id text, desired_target_digest text, locked_projection jsonb)',
                'plpgsql',
                TRUE,
                1000::REAL
            ),
            (
                'public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)',
                'expected_tenant_id text, expected_installation_id text, expected_promotion_id text, expected_product_revision bigint, expected_payload_digest text, expected_principal_id text, expected_product_session_digest bytea, session_subject_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, expected_authority_revision bigint, expected_authority_payload_digest text, expected_authority_observation_digest text, expected_authority_observed_at timestamp with time zone, expected_authority_expires_at timestamp with time zone, expected_effective_permission_bits text, expected_guild_owner boolean, product_request_id text, active_idempotency_key_digest text, idempotency_key_digest_candidates text[], idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[], idempotency_digest_key_id text, semantic_request_digest text, new_receipt_id text, new_audit_event_id text, new_apply_attempt_id text, new_deployment_id text, locked_projection jsonb, prepared_desired_target_digest text, prepared_previous_runtime jsonb, prepared_snapshot jsonb, prepared_activation_notices jsonb',
                'TABLE(outcome text, resulting_revision bigint, resulting_state text, exact_replay boolean, guild_id text, deployment_id text, desired_target_digest text)',
                'plpgsql',
                TRUE,
                1000::REAL
            )
    ) AS expected(
        signature,
        identity_arguments,
        result_name,
        language_name,
        returns_set,
        rows_estimate
    )
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.signature)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR NOT function_row.proisstrict
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR function_row.proretset <> expected.returns_set
        OR function_row.prorows <> expected.rows_estimate
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM expected.language_name
        OR pg_catalog.pg_get_function_identity_arguments(function_row.oid)
            IS DISTINCT FROM expected.identity_arguments
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM expected.result_name;
    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION 'product apply external function contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)',
                'expected_tenant_id text, expected_installation_id text, expected_promotion_id text, expected_product_revision bigint, expected_payload_digest text, expected_principal_id text, expected_product_session_digest bytea, session_subject_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, expected_authority_revision bigint, expected_authority_payload_digest text, expected_authority_observation_digest text, expected_authority_observed_at timestamp with time zone, expected_authority_expires_at timestamp with time zone, expected_effective_permission_bits text, expected_guild_owner boolean, product_request_id text, active_idempotency_key_digest text, idempotency_key_digest_candidates text[], idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[], idempotency_digest_key_id text, semantic_request_digest text, new_receipt_id text, new_audit_event_id text, new_apply_attempt_id text, new_deployment_id text',
                'TABLE(outcome text, exact_replay boolean, requires_commit boolean, resulting_revision bigint, resulting_state text, deployment_id text, desired_target_digest text, locked_projection jsonb)',
                'plpgsql',
                'v'::"char",
                TRUE,
                TRUE,
                TRUE,
                1000::REAL
            ),
            (
                'public.starring_product_apply_authority_projection_v1(text,text,text,text,bytea,text,text,text,text,bigint,text,timestamp with time zone,timestamp with time zone,text,boolean,text)',
                'expected_tenant_id text, expected_installation_id text, expected_promotion_id text, expected_principal_id text, expected_product_session_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, expected_authority_revision bigint, expected_authority_payload_digest text, expected_authority_observed_at timestamp with time zone, expected_authority_expires_at timestamp with time zone, expected_effective_permission_bits text, expected_guild_owner boolean, expected_payload_digest text',
                'jsonb',
                'plpgsql',
                'v'::"char",
                TRUE,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'public.starring_product_ruleset_slot_exact_v1(text,text,text,text,bigint)',
                'expected_tenant_id text, expected_installation_id text, expected_guild_id text, expected_ruleset_key text, expected_active_version bigint',
                'boolean',
                'sql',
                's'::"char",
                TRUE,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'public.starring_runtime_desired_target_digest_v1(jsonb,bigint)',
                'prepared_snapshot jsonb, installation_authority_revision bigint',
                'text',
                'plpgsql',
                'i'::"char",
                TRUE,
                FALSE,
                FALSE,
                0::REAL
            ),
            (
                'public.starring_runtime_lock_current_authority(text,text,text,text,bigint,text,text,bigint,text,bigint,text)',
                'expected_activation_request_id text, expected_promotion_id text, expected_tenant_id text, expected_installation_id text, expected_installation_authority_revision bigint, expected_guild_id text, expected_ruleset_key text, expected_target_version bigint, expected_target_content_hash text, expected_binding_revision bigint, expected_binding_fingerprint text',
                'text',
                'plpgsql',
                'v'::"char",
                FALSE,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'public.starring_runtime_current_mutation_clock()',
                '',
                'timestamp with time zone',
                'plpgsql',
                'v'::"char",
                FALSE,
                TRUE,
                FALSE,
                0::REAL
            )
    ) AS expected(
        signature,
        identity_arguments,
        result_name,
        language_name,
        volatility,
        strict_input,
        security_definer,
        returns_set,
        rows_estimate
    )
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.signature)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> expected.volatility
        OR function_row.proisstrict <> expected.strict_input
        OR function_row.proparallel <> 'u'
        OR function_row.prosecdef <> expected.security_definer
        OR function_row.proretset <> expected.returns_set
        OR function_row.prorows <> expected.rows_estimate
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM expected.language_name
        OR pg_catalog.pg_get_function_identity_arguments(function_row.oid)
            IS DISTINCT FROM expected.identity_arguments
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM expected.result_name;
    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION 'product apply helper function contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        SELECT signature, TRUE AS strict_input
        FROM pg_catalog.unnest(strict_trigger_signatures) AS strict(signature)
        UNION ALL
        SELECT signature, FALSE AS strict_input
        FROM pg_catalog.unnest(nonstrict_trigger_signatures) AS nonstrict(signature)
    ) AS expected
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.signature)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR function_row.proisstrict <> expected.strict_input
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR function_row.proretset
        OR function_row.prorows <> 0
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM 'plpgsql'
        OR pg_catalog.pg_get_function_identity_arguments(function_row.oid)
            IS DISTINCT FROM ''
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM 'trigger';
    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION 'product apply trigger function contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    WITH expected_triggers AS (
        SELECT pg_catalog.to_regclass(expected.relation) AS relation_oid,
            pg_catalog.to_regprocedure(expected.function) AS function_oid,
            expected.definition
        FROM pg_catalog.jsonb_to_recordset(expected_trigger_manifest)
            AS expected(relation TEXT, function TEXT, definition TEXT)
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
    )
    SELECT pg_catalog.count(*)
    INTO trigger_mismatch_count
    FROM expected_triggers AS expected
    FULL JOIN actual_triggers AS actual
        ON actual.relation_oid = expected.relation_oid
        AND actual.function_oid = expected.function_oid
        AND actual.definition = expected.definition
        AND actual.enabled = 'O'
        AND NOT actual.internal
        AND actual.structural_valid
    WHERE expected.relation_oid IS NULL
        OR expected.function_oid IS NULL
        OR actual.trigger_oid IS NULL;
    IF trigger_mismatch_count <> 0 THEN
        RAISE EXCEPTION 'product apply trigger manifest is invalid'
            USING ERRCODE = '55000';
    END IF;

    protected_signatures := external_existing_signatures
        || internal_helper_signatures
        || strict_trigger_signatures
        || nonstrict_trigger_signatures;
    FOREACH expected_signature IN ARRAY protected_signatures
    LOOP
        function_oid := pg_catalog.to_regprocedure(expected_signature);
        IF function_oid IS NULL THEN
            RAISE EXCEPTION 'product apply protected function is unavailable'
                USING ERRCODE = '55000';
        END IF;
        SELECT function_row.proname
        INTO function_name
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid = function_oid;
        SELECT pg_catalog.count(*)
        INTO function_collision_count
        FROM pg_catalog.pg_proc AS function_row
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = function_row.pronamespace
        WHERE namespace.nspname = 'public'
            AND function_row.proname = function_name;
        IF function_collision_count <> 1 THEN
            RAISE EXCEPTION 'product apply protected function overload is invalid'
                USING ERRCODE = '55000';
        END IF;
    END LOOP;

    SELECT pg_catalog.count(*)
    INTO function_collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_product_apply_target_artifact_v1',
            'starring_product_apply_keyring_coverage_v1'
        );
    IF function_collision_count <> 0 THEN
        RAISE EXCEPTION 'product apply new function already exists'
            USING ERRCODE = '55000';
    END IF;

    EXECUTE $definition$
CREATE FUNCTION public.starring_product_apply_target_artifact_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_promotion_id TEXT,
    expected_principal_id TEXT,
    expected_product_session_digest BYTEA,
    expected_acting_discord_user_id TEXT,
    expected_guild_id TEXT
)
RETURNS TABLE(
    schema_version BIGINT,
    definition JSONB,
    content_hash TEXT,
    canonical_content_hash TEXT
)
LANGUAGE sql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
    WITH lifecycle_clock AS MATERIALIZED (
        SELECT pg_catalog.transaction_timestamp() AS database_now
    )
    SELECT version.schema_version,
        CASE
            WHEN pg_catalog.octet_length(version.definition::TEXT) <= 524288
                THEN version.definition
        END AS definition,
        version.content_hash,
        version.canonical_content_hash
    FROM public.activation_requests AS activation
    INNER JOIN public.automation_ruleset_versions AS version
        ON version.guild_id = activation.guild_id
        AND version.ruleset_key = activation.ruleset_key
        AND version.version = activation.target_version
        AND version.content_hash = activation.target_content_hash
    INNER JOIN public.product_tenants AS tenant
        ON tenant.tenant_id = activation.tenant_id
    INNER JOIN public.automation_installations AS installation
        ON installation.tenant_id = activation.tenant_id
        AND installation.installation_id = activation.installation_id
        AND installation.discord_guild_id = activation.guild_id
        AND installation.ruleset_key = activation.ruleset_key
    INNER JOIN public.product_principals AS principal
        ON principal.principal_id = expected_principal_id
        AND principal.discord_user_id = expected_acting_discord_user_id
    INNER JOIN public.product_auth_sessions AS product_session
        ON product_session.principal_id = principal.principal_id
        AND product_session.session_digest = expected_product_session_digest
    CROSS JOIN lifecycle_clock
    WHERE activation.tenant_id = expected_tenant_id
        AND activation.installation_id = expected_installation_id
        AND activation.promotion_id = expected_promotion_id
        AND activation.guild_id = expected_guild_id
        AND activation.authority_kind = 'product_authoring'
        AND tenant.lifecycle_state = 'active'
        AND installation.lifecycle_state = 'active'
        AND NOT principal.disabled
        AND product_session.oauth_state_digest IS NOT NULL
        AND product_session.revoked_at IS NULL
        AND lifecycle_clock.database_now < product_session.idle_expires_at
        AND lifecycle_clock.database_now < product_session.absolute_expires_at
        AND expected_tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND expected_installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND expected_promotion_id ~ '^[0-9a-f]{64}$'
        AND expected_principal_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND pg_catalog.octet_length(expected_product_session_digest) = 32
        AND CASE
            WHEN expected_acting_discord_user_id ~ '^[1-9][0-9]{0,19}$'
                THEN expected_acting_discord_user_id::NUMERIC
                    <= 18446744073709551615
            ELSE FALSE
        END
        AND CASE
            WHEN expected_guild_id ~ '^[1-9][0-9]{0,19}$'
                THEN expected_guild_id::NUMERIC <= 18446744073709551615
            ELSE FALSE
        END
    LIMIT 2
    FOR SHARE OF activation, version;
$function$
$definition$;

    EXECUTE $definition$
CREATE FUNCTION public.starring_product_apply_keyring_coverage_v1(
    idempotency_digest_key_id_candidates TEXT[],
    idempotency_digest_key_fingerprint_candidates TEXT[]
)
RETURNS TABLE(outcome TEXT)
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
BEGIN
    IF pg_catalog.array_ndims(idempotency_digest_key_id_candidates)
            IS DISTINCT FROM 1
        OR pg_catalog.array_lower(idempotency_digest_key_id_candidates, 1)
            IS DISTINCT FROM 1
        OR pg_catalog.cardinality(idempotency_digest_key_id_candidates)
            NOT BETWEEN 1 AND 8
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.unnest(idempotency_digest_key_id_candidates)
                AS candidate(key_id)
            WHERE candidate.key_id !~ '^[A-Za-z0-9_.:-]{1,64}$'
        )
        OR pg_catalog.array_ndims(
            idempotency_digest_key_fingerprint_candidates
        ) IS DISTINCT FROM 1
        OR pg_catalog.array_lower(
            idempotency_digest_key_fingerprint_candidates,
            1
        ) IS DISTINCT FROM 1
        OR pg_catalog.cardinality(
            idempotency_digest_key_fingerprint_candidates
        ) IS DISTINCT FROM pg_catalog.cardinality(
            idempotency_digest_key_id_candidates
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.unnest(
                idempotency_digest_key_fingerprint_candidates
            ) AS candidate(fingerprint)
            WHERE candidate.fingerprint !~ '^[0-9a-f]{64}$'
        )
        OR (
            SELECT pg_catalog.count(DISTINCT candidate.key_id)
            FROM pg_catalog.unnest(idempotency_digest_key_id_candidates)
                AS candidate(key_id)
        ) <> pg_catalog.cardinality(idempotency_digest_key_id_candidates)
        OR (
            SELECT pg_catalog.count(DISTINCT candidate.fingerprint)
            FROM pg_catalog.unnest(
                idempotency_digest_key_fingerprint_candidates
            ) AS candidate(fingerprint)
        ) <> pg_catalog.cardinality(
            idempotency_digest_key_fingerprint_candidates
        )
    THEN
        RETURN QUERY SELECT 'invalid_input';
        RETURN;
    END IF;

    IF EXISTS (
        SELECT 1
        FROM public.product_action_receipts AS receipt
        WHERE receipt.endpoint_domain = 'product_apply_v1'
            AND NOT EXISTS (
                SELECT 1
                FROM public.product_action_receipt_idempotency_aliases AS alias
                WHERE alias.tenant_id = receipt.tenant_id
                    AND alias.installation_id = receipt.installation_id
                    AND alias.principal_id = receipt.principal_id
                    AND alias.endpoint_domain = receipt.endpoint_domain
                    AND alias.receipt_id = receipt.receipt_id
                    AND EXISTS (
                        SELECT 1
                        FROM pg_catalog.generate_subscripts(
                            idempotency_digest_key_id_candidates,
                            1
                        ) AS candidate(ordinal)
                        WHERE idempotency_digest_key_id_candidates[
                                candidate.ordinal
                            ] = alias.idempotency_digest_key_id
                            AND idempotency_digest_key_fingerprint_candidates[
                                candidate.ordinal
                            ] = alias.idempotency_digest_key_fingerprint
                    )
            )
    ) THEN
        RETURN QUERY SELECT 'idempotency_keyring_incomplete';
        RETURN;
    END IF;

    RETURN QUERY SELECT 'ok';
END;
$function$
$definition$;

    FOREACH expected_signature IN ARRAY external_existing_signatures
    LOOP
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s VOLATILE',
            expected_signature
        );
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s STRICT',
            expected_signature
        );
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s PARALLEL UNSAFE',
            expected_signature
        );
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s SECURITY DEFINER',
            expected_signature
        );
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s RESET ALL',
            expected_signature
        );
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s SET search_path = pg_catalog',
            expected_signature
        );
    END LOOP;
    EXECUTE pg_catalog.format(
        'ALTER FUNCTION %s ROWS 1',
        external_existing_signatures[2]
    );
    EXECUTE pg_catalog.format(
        'ALTER FUNCTION %s ROWS 1',
        external_existing_signatures[3]
    );

    expected_signature := internal_helper_signatures[1];
    EXECUTE pg_catalog.format('ALTER FUNCTION %s VOLATILE', expected_signature);
    EXECUTE pg_catalog.format('ALTER FUNCTION %s STRICT', expected_signature);
    EXECUTE pg_catalog.format('ALTER FUNCTION %s PARALLEL UNSAFE', expected_signature);
    EXECUTE pg_catalog.format('ALTER FUNCTION %s SECURITY DEFINER', expected_signature);
    EXECUTE pg_catalog.format('ALTER FUNCTION %s ROWS 1', expected_signature);
    EXECUTE pg_catalog.format('ALTER FUNCTION %s RESET ALL', expected_signature);
    EXECUTE pg_catalog.format(
        'ALTER FUNCTION %s SET search_path = pg_catalog',
        expected_signature
    );

    expected_signature := internal_helper_signatures[2];
    EXECUTE pg_catalog.format('ALTER FUNCTION %s VOLATILE', expected_signature);
    EXECUTE pg_catalog.format('ALTER FUNCTION %s STRICT', expected_signature);
    EXECUTE pg_catalog.format('ALTER FUNCTION %s PARALLEL UNSAFE', expected_signature);
    EXECUTE pg_catalog.format('ALTER FUNCTION %s SECURITY DEFINER', expected_signature);
    EXECUTE pg_catalog.format('ALTER FUNCTION %s RESET ALL', expected_signature);
    EXECUTE pg_catalog.format(
        'ALTER FUNCTION %s SET search_path = pg_catalog',
        expected_signature
    );

    expected_signature := internal_helper_signatures[3];
    EXECUTE pg_catalog.format('ALTER FUNCTION %s STABLE', expected_signature);
    EXECUTE pg_catalog.format('ALTER FUNCTION %s STRICT', expected_signature);
    EXECUTE pg_catalog.format('ALTER FUNCTION %s PARALLEL UNSAFE', expected_signature);
    EXECUTE pg_catalog.format('ALTER FUNCTION %s SECURITY DEFINER', expected_signature);
    EXECUTE pg_catalog.format('ALTER FUNCTION %s RESET ALL', expected_signature);
    EXECUTE pg_catalog.format(
        'ALTER FUNCTION %s SET search_path = pg_catalog',
        expected_signature
    );

    expected_signature := internal_helper_signatures[4];
    EXECUTE pg_catalog.format('ALTER FUNCTION %s IMMUTABLE', expected_signature);
    EXECUTE pg_catalog.format('ALTER FUNCTION %s STRICT', expected_signature);
    EXECUTE pg_catalog.format('ALTER FUNCTION %s PARALLEL UNSAFE', expected_signature);
    EXECUTE pg_catalog.format('ALTER FUNCTION %s SECURITY INVOKER', expected_signature);
    EXECUTE pg_catalog.format('ALTER FUNCTION %s RESET ALL', expected_signature);
    EXECUTE pg_catalog.format(
        'ALTER FUNCTION %s SET search_path = pg_catalog',
        expected_signature
    );

    FOREACH expected_signature IN ARRAY ARRAY[
        internal_helper_signatures[5],
        internal_helper_signatures[6]
    ]::TEXT[]
    LOOP
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s VOLATILE',
            expected_signature
        );
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s CALLED ON NULL INPUT',
            expected_signature
        );
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s PARALLEL UNSAFE',
            expected_signature
        );
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s SECURITY DEFINER',
            expected_signature
        );
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s RESET ALL',
            expected_signature
        );
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s SET search_path = pg_catalog',
            expected_signature
        );
    END LOOP;

    FOREACH expected_signature IN ARRAY strict_trigger_signatures
    LOOP
        EXECUTE pg_catalog.format('ALTER FUNCTION %s VOLATILE', expected_signature);
        EXECUTE pg_catalog.format('ALTER FUNCTION %s STRICT', expected_signature);
        EXECUTE pg_catalog.format('ALTER FUNCTION %s PARALLEL UNSAFE', expected_signature);
        EXECUTE pg_catalog.format('ALTER FUNCTION %s SECURITY DEFINER', expected_signature);
        EXECUTE pg_catalog.format('ALTER FUNCTION %s RESET ALL', expected_signature);
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s SET search_path = pg_catalog',
            expected_signature
        );
    END LOOP;
    FOREACH expected_signature IN ARRAY nonstrict_trigger_signatures
    LOOP
        EXECUTE pg_catalog.format('ALTER FUNCTION %s VOLATILE', expected_signature);
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s CALLED ON NULL INPUT',
            expected_signature
        );
        EXECUTE pg_catalog.format('ALTER FUNCTION %s PARALLEL UNSAFE', expected_signature);
        EXECUTE pg_catalog.format('ALTER FUNCTION %s SECURITY DEFINER', expected_signature);
        EXECUTE pg_catalog.format('ALTER FUNCTION %s RESET ALL', expected_signature);
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s SET search_path = pg_catalog',
            expected_signature
        );
    END LOOP;

    protected_signatures := protected_signatures || ARRAY[
        'public.starring_product_apply_target_artifact_v1(text,text,text,text,bytea,text,text)',
        'public.starring_product_apply_keyring_coverage_v1(text[],text[])'
    ]::TEXT[];
    FOREACH expected_signature IN ARRAY protected_signatures
    LOOP
        function_oid := pg_catalog.to_regprocedure(expected_signature);
        IF function_oid IS NULL THEN
            RAISE EXCEPTION 'product apply protected function is unavailable'
                USING ERRCODE = '55000';
        END IF;
        FOR unexpected_grantee IN
            SELECT DISTINCT privilege.grantee
            FROM pg_catalog.pg_proc AS function_row
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE function_row.oid = function_oid
                AND privilege.grantee <> 0
                AND privilege.grantee <> function_row.proowner
        LOOP
            unexpected_grantee_name :=
                pg_catalog.pg_get_userbyid(unexpected_grantee);
            IF unexpected_grantee_name IS NULL THEN
                RAISE EXCEPTION 'product apply function grantee is unavailable'
                    USING ERRCODE = '55000';
            END IF;
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM %I CASCADE',
                expected_signature,
                unexpected_grantee_name
            );
        END LOOP;
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s OWNER TO %I',
            expected_signature,
            common_owner_name
        );
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE',
            expected_signature
        );
    END LOOP;

    EXECUTE $probe$
        SELECT pg_catalog.count(*)
        FROM public.starring_product_apply_target_artifact_v1(
            '',
            '',
            '',
            '',
            pg_catalog.decode('', 'hex'),
            '',
            ''
        )
    $probe$
    INTO probe_count;
    IF probe_count <> 0 THEN
        RAISE EXCEPTION 'product apply target invalid probe is unsafe'
            USING ERRCODE = '55000';
    END IF;

    EXECUTE $probe$
        SELECT pg_catalog.count(*)
        FROM public.starring_product_apply_target_artifact_v1(
            'probe',
            'probe',
            pg_catalog.repeat('0', 64),
            'probe',
            pg_catalog.decode(pg_catalog.repeat('00', 32), 'hex'),
            '1',
            '1'
        )
    $probe$
    INTO probe_count;
    IF probe_count <> 0 THEN
        RAISE EXCEPTION 'product apply target zero-row probe is unsafe'
            USING ERRCODE = '55000';
    END IF;

    EXECUTE $probe$
        SELECT pg_catalog.count(*), pg_catalog.min(coverage.outcome)
        FROM public.starring_product_apply_keyring_coverage_v1(
            ARRAY[]::TEXT[],
            ARRAY[]::TEXT[]
        ) AS coverage
    $probe$
    INTO probe_count, probe_outcome;
    IF probe_count <> 1 OR probe_outcome IS DISTINCT FROM 'invalid_input' THEN
        RAISE EXCEPTION 'product apply keyring invalid probe is unsafe'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(relation.oid),
        pg_catalog.count(relation.oid) FILTER (WHERE relation.relkind = 'r'),
        pg_catalog.count(relation.oid) FILTER (
            WHERE NOT relation.relrowsecurity AND NOT relation.relforcerowsecurity
        ),
        pg_catalog.count(DISTINCT relation.relowner),
        pg_catalog.min(relation.relowner::BIGINT)::OID
    INTO relation_count, table_count, rls_disabled_count, owner_count, common_owner
    FROM (
        VALUES
            (pg_catalog.to_regclass('public.product_control_plane_identity')),
            (pg_catalog.to_regclass('public.activation_requests')),
            (pg_catalog.to_regclass('public.activation_request_approvals')),
            (pg_catalog.to_regclass('public.authoring_promotions')),
            (pg_catalog.to_regclass('public.product_tenants')),
            (pg_catalog.to_regclass('public.automation_installations')),
            (pg_catalog.to_regclass('public.automation_installation_authority_versions')),
            (pg_catalog.to_regclass('public.product_principals')),
            (pg_catalog.to_regclass('public.product_auth_sessions')),
            (pg_catalog.to_regclass('public.product_action_receipts')),
            (pg_catalog.to_regclass('public.product_action_receipt_idempotency_aliases')),
            (pg_catalog.to_regclass('public.product_audit_events')),
            (pg_catalog.to_regclass('public.product_action_receipt_audit_evidence')),
            (pg_catalog.to_regclass('public.automation_ruleset_activations')),
            (pg_catalog.to_regclass('public.automation_ruleset_versions')),
            (pg_catalog.to_regclass('public.runtime_deployments')),
            (pg_catalog.to_regclass('public.runtime_serving_leases')),
            (pg_catalog.to_regclass('public.runtime_attestations'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid;
    IF relation_count <> 18
        OR table_count <> 18
        OR rls_disabled_count <> 18
        OR owner_count <> 1
        OR common_owner IS NULL
    THEN
        RAISE EXCEPTION 'product apply relation contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_product_apply_executor_database_identity_v1()',
                '',
                'text',
                'sql',
                FALSE,
                0::REAL
            ),
            (
                'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)',
                'expected_tenant_id text, expected_installation_id text, expected_promotion_id text, expected_product_revision bigint, expected_payload_digest text, expected_principal_id text, expected_product_session_digest bytea, session_subject_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, expected_authority_revision bigint, expected_authority_payload_digest text, expected_authority_observation_digest text, expected_authority_observed_at timestamp with time zone, expected_authority_expires_at timestamp with time zone, expected_effective_permission_bits text, expected_guild_owner boolean, product_request_id text, active_idempotency_key_digest text, idempotency_key_digest_candidates text[], idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[], idempotency_digest_key_id text, semantic_request_digest text, new_receipt_id text, new_audit_event_id text, new_apply_attempt_id text, new_deployment_id text',
                'TABLE(outcome text, exact_replay boolean, requires_commit boolean, resulting_revision bigint, resulting_state text, deployment_id text, desired_target_digest text, locked_projection jsonb)',
                'plpgsql',
                TRUE,
                1::REAL
            ),
            (
                'public.starring_product_apply_target_artifact_v1(text,text,text,text,bytea,text,text)',
                'expected_tenant_id text, expected_installation_id text, expected_promotion_id text, expected_principal_id text, expected_product_session_digest bytea, expected_acting_discord_user_id text, expected_guild_id text',
                'TABLE(schema_version bigint, definition jsonb, content_hash text, canonical_content_hash text)',
                'sql',
                TRUE,
                1::REAL
            ),
            (
                'public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)',
                'expected_tenant_id text, expected_installation_id text, expected_promotion_id text, expected_product_revision bigint, expected_payload_digest text, expected_principal_id text, expected_product_session_digest bytea, session_subject_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, expected_authority_revision bigint, expected_authority_payload_digest text, expected_authority_observation_digest text, expected_authority_observed_at timestamp with time zone, expected_authority_expires_at timestamp with time zone, expected_effective_permission_bits text, expected_guild_owner boolean, product_request_id text, active_idempotency_key_digest text, idempotency_key_digest_candidates text[], idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[], idempotency_digest_key_id text, semantic_request_digest text, new_receipt_id text, new_audit_event_id text, new_apply_attempt_id text, new_deployment_id text, locked_projection jsonb, prepared_desired_target_digest text, prepared_previous_runtime jsonb, prepared_snapshot jsonb, prepared_activation_notices jsonb',
                'TABLE(outcome text, resulting_revision bigint, resulting_state text, exact_replay boolean, guild_id text, deployment_id text, desired_target_digest text)',
                'plpgsql',
                TRUE,
                1::REAL
            ),
            (
                'public.starring_product_apply_keyring_coverage_v1(text[],text[])',
                'idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[]',
                'TABLE(outcome text)',
                'plpgsql',
                TRUE,
                1::REAL
            )
    ) AS expected(
        signature,
        identity_arguments,
        result_name,
        language_name,
        returns_set,
        rows_estimate
    )
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.signature)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR NOT function_row.proisstrict
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR function_row.proretset <> expected.returns_set
        OR function_row.prorows <> expected.rows_estimate
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM expected.language_name
        OR pg_catalog.pg_get_function_identity_arguments(function_row.oid)
            IS DISTINCT FROM expected.identity_arguments
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM expected.result_name
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> function_row.proowner
        );
    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION 'product apply external function postcondition is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                internal_helper_signatures[1],
                'TABLE(outcome text, exact_replay boolean, requires_commit boolean, resulting_revision bigint, resulting_state text, deployment_id text, desired_target_digest text, locked_projection jsonb)',
                'plpgsql',
                'v'::"char",
                TRUE,
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                internal_helper_signatures[2],
                'jsonb',
                'plpgsql',
                'v'::"char",
                TRUE,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                internal_helper_signatures[3],
                'boolean',
                'sql',
                's'::"char",
                TRUE,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                internal_helper_signatures[4],
                'text',
                'plpgsql',
                'i'::"char",
                TRUE,
                FALSE,
                FALSE,
                0::REAL
            ),
            (
                internal_helper_signatures[5],
                'text',
                'plpgsql',
                'v'::"char",
                FALSE,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                internal_helper_signatures[6],
                'timestamp with time zone',
                'plpgsql',
                'v'::"char",
                FALSE,
                TRUE,
                FALSE,
                0::REAL
            )
    ) AS expected(
        signature,
        result_name,
        language_name,
        volatility,
        strict_input,
        security_definer,
        returns_set,
        rows_estimate
    )
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.signature)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> expected.volatility
        OR function_row.proisstrict <> expected.strict_input
        OR function_row.proparallel <> 'u'
        OR function_row.prosecdef <> expected.security_definer
        OR function_row.proretset <> expected.returns_set
        OR function_row.prorows <> expected.rows_estimate
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM expected.language_name
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM expected.result_name
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> function_row.proowner
        );
    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION 'product apply helper function postcondition is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        SELECT signature, TRUE AS strict_input
        FROM pg_catalog.unnest(strict_trigger_signatures) AS strict(signature)
        UNION ALL
        SELECT signature, FALSE AS strict_input
        FROM pg_catalog.unnest(nonstrict_trigger_signatures) AS nonstrict(signature)
    ) AS expected
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.signature)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR function_row.proisstrict <> expected.strict_input
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR function_row.proretset
        OR function_row.prorows <> 0
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM 'plpgsql'
        OR pg_catalog.pg_get_function_identity_arguments(function_row.oid)
            IS DISTINCT FROM ''
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM 'trigger'
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> function_row.proowner
        );
    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION 'product apply trigger function postcondition is invalid'
            USING ERRCODE = '55000';
    END IF;

    WITH expected_triggers AS (
        SELECT pg_catalog.to_regclass(expected.relation) AS relation_oid,
            pg_catalog.to_regprocedure(expected.function) AS function_oid,
            expected.definition
        FROM pg_catalog.jsonb_to_recordset(expected_trigger_manifest)
            AS expected(relation TEXT, function TEXT, definition TEXT)
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
    )
    SELECT pg_catalog.count(*)
    INTO trigger_mismatch_count
    FROM expected_triggers AS expected
    FULL JOIN actual_triggers AS actual
        ON actual.relation_oid = expected.relation_oid
        AND actual.function_oid = expected.function_oid
        AND actual.definition = expected.definition
        AND actual.enabled = 'O'
        AND NOT actual.internal
        AND actual.structural_valid
    WHERE expected.relation_oid IS NULL
        OR expected.function_oid IS NULL
        OR actual.trigger_oid IS NULL;
    IF trigger_mismatch_count <> 0 THEN
        RAISE EXCEPTION 'product apply trigger postcondition is invalid'
            USING ERRCODE = '55000';
    END IF;

    FOREACH expected_signature IN ARRAY protected_signatures
    LOOP
        function_oid := pg_catalog.to_regprocedure(expected_signature);
        SELECT function_row.proname
        INTO function_name
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid = function_oid;
        SELECT pg_catalog.count(*)
        INTO function_collision_count
        FROM pg_catalog.pg_proc AS function_row
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = function_row.pronamespace
        WHERE namespace.nspname = 'public'
            AND function_row.proname = function_name;
        IF function_oid IS NULL OR function_collision_count <> 1 THEN
            RAISE EXCEPTION 'product apply function identity postcondition is invalid'
                USING ERRCODE = '55000';
        END IF;
    END LOOP;

    PERFORM pg_catalog.set_config('search_path', original_search_path, TRUE);
    PERFORM pg_catalog.set_config(
        'quote_all_identifiers',
        original_quote_all_identifiers,
        TRUE
    );
END;
$scope$;
