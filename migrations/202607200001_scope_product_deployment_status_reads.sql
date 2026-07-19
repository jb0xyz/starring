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
    function_collision_count BIGINT;
    invalid_function_count BIGINT;
    trigger_mismatch_count BIGINT;
    artifact_support_count BIGINT;
    function_oid OID;
    expected_signature TEXT;
    unexpected_grantee OID;
    unexpected_grantee_name NAME;
    probe_count BIGINT;
    original_search_path TEXT;
    original_quote_all_identifiers TEXT;
    expected_trigger_manifest JSONB := $manifest$
[
  {"relation":"public.automation_ruleset_versions","function":"public.reject_ruleset_artifact_mutation()","definition":"CREATE TRIGGER automation_ruleset_versions_reject_mutation BEFORE DELETE OR UPDATE ON public.automation_ruleset_versions FOR EACH STATEMENT EXECUTE FUNCTION public.reject_ruleset_artifact_mutation()"},
  {"relation":"public.automation_ruleset_versions","function":"public.reject_ruleset_artifact_mutation()","definition":"CREATE TRIGGER automation_ruleset_versions_reject_truncate BEFORE TRUNCATE ON public.automation_ruleset_versions FOR EACH STATEMENT EXECUTE FUNCTION public.reject_ruleset_artifact_mutation()"},
  {"relation":"public.runtime_attestations","function":"public.reject_immutable_product_row()","definition":"CREATE TRIGGER runtime_attestations_reject_mutation BEFORE DELETE OR UPDATE ON public.runtime_attestations FOR EACH ROW EXECUTE FUNCTION public.reject_immutable_product_row()"},
  {"relation":"public.runtime_attestations","function":"public.validate_runtime_attestation_projection()","definition":"CREATE TRIGGER runtime_attestations_validate_projection BEFORE INSERT ON public.runtime_attestations FOR EACH ROW EXECUTE FUNCTION public.validate_runtime_attestation_projection()"},
  {"relation":"public.runtime_deployments","function":"public.guard_runtime_ruleset_artifact_transition()","definition":"CREATE TRIGGER runtime_deployments_guard_ruleset_artifact_transition BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.guard_runtime_ruleset_artifact_transition()"},
  {"relation":"public.runtime_deployments","function":"public.enforce_runtime_deployment_policy_shadow()","definition":"CREATE TRIGGER runtime_deployments_policy_shadow_guard BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.enforce_runtime_deployment_policy_shadow()"},
  {"relation":"public.runtime_deployments","function":"public.reject_runtime_deployment_delete()","definition":"CREATE TRIGGER runtime_deployments_reject_delete BEFORE DELETE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.reject_runtime_deployment_delete()"},
  {"relation":"public.runtime_deployments","function":"public.validate_runtime_deployment_projection()","definition":"CREATE TRIGGER runtime_deployments_validate_projection BEFORE INSERT OR UPDATE ON public.runtime_deployments FOR EACH ROW EXECUTE FUNCTION public.validate_runtime_deployment_projection()"},
  {"relation":"public.runtime_serving_leases","function":"public.reject_runtime_serving_lease_delete()","definition":"CREATE TRIGGER runtime_serving_leases_reject_delete BEFORE DELETE ON public.runtime_serving_leases FOR EACH ROW EXECUTE FUNCTION public.reject_runtime_serving_lease_delete()"},
  {"relation":"public.runtime_serving_leases","function":"public.validate_runtime_serving_lease_transition()","definition":"CREATE TRIGGER runtime_serving_leases_validate_transition BEFORE INSERT OR UPDATE ON public.runtime_serving_leases FOR EACH ROW EXECUTE FUNCTION public.validate_runtime_serving_lease_transition()"}
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
            (pg_catalog.to_regclass('public.product_principals')),
            (pg_catalog.to_regclass('public.product_auth_sessions')),
            (pg_catalog.to_regclass('public.runtime_deployments')),
            (pg_catalog.to_regclass('public.activation_requests')),
            (pg_catalog.to_regclass('public.authoring_promotions')),
            (pg_catalog.to_regclass('public.product_tenants')),
            (pg_catalog.to_regclass('public.automation_installations')),
            (pg_catalog.to_regclass('public.automation_installation_authority_versions')),
            (pg_catalog.to_regclass('public.automation_ruleset_activations')),
            (pg_catalog.to_regclass('public.automation_ruleset_versions')),
            (pg_catalog.to_regclass('public.runtime_attestations')),
            (pg_catalog.to_regclass('public.runtime_serving_leases'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid;
    IF relation_count <> 13
        OR table_count <> 13
        OR rls_disabled_count <> 13
        OR owner_count <> 1
        OR common_owner IS NULL
    THEN
        RAISE EXCEPTION 'product deployment status relations require one non-RLS owner'
            USING ERRCODE = '55000';
    END IF;

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL THEN
        RAISE EXCEPTION 'product deployment status relation owner is unavailable'
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
        RAISE EXCEPTION 'product deployment status schema is not trusted'
            USING ERRCODE = '55000';
    END IF;

    IF pg_catalog.to_regrole(current_user) <> common_owner
        OR NOT pg_catalog.has_schema_privilege(
            common_owner_name,
            'public',
            'CREATE'
        )
    THEN
        RAISE EXCEPTION 'product deployment status migration requires the common owner'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            ('public.validate_runtime_deployment_projection()', TRUE, ARRAY['search_path=pg_catalog']::TEXT[], TRUE),
            ('public.enforce_runtime_deployment_policy_shadow()', TRUE, ARRAY['search_path=pg_catalog']::TEXT[], TRUE),
            ('public.guard_runtime_ruleset_artifact_transition()', TRUE, ARRAY['search_path=pg_catalog']::TEXT[], TRUE),
            ('public.reject_runtime_deployment_delete()', TRUE, ARRAY['search_path=pg_catalog']::TEXT[], TRUE),
            ('public.validate_runtime_attestation_projection()', TRUE, ARRAY['search_path=pg_catalog']::TEXT[], TRUE),
            ('public.reject_immutable_product_row()', FALSE, NULL::TEXT[], FALSE),
            ('public.validate_runtime_serving_lease_transition()', TRUE, ARRAY['search_path=pg_catalog']::TEXT[], TRUE),
            ('public.reject_runtime_serving_lease_delete()', TRUE, ARRAY['search_path=pg_catalog']::TEXT[], TRUE),
            ('public.reject_ruleset_artifact_mutation()', TRUE, ARRAY['search_path=pg_catalog']::TEXT[], TRUE)
    ) AS expected(signature, security_definer, configuration, private_execution)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.signature)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR function_row.proisstrict
        OR function_row.proparallel <> 'u'
        OR function_row.prosecdef <> expected.security_definer
        OR function_row.proretset
        OR function_row.prorows <> 0
        OR function_row.proconfig IS DISTINCT FROM expected.configuration
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM 'plpgsql'
        OR pg_catalog.pg_get_function_identity_arguments(function_row.oid)
            IS DISTINCT FROM ''
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM 'trigger'
        OR (
            expected.private_execution
            AND EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE privilege.grantee <> function_row.proowner
            )
        );
    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION 'product deployment status trigger function contract is invalid'
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
                AND trigger_row.tgconstraint = 0
                AND NOT trigger_row.tgdeferrable
                AND NOT trigger_row.tginitdeferred AS structural_valid,
            pg_catalog.pg_get_triggerdef(trigger_row.oid, FALSE) AS definition
        FROM pg_catalog.pg_trigger AS trigger_row
        WHERE NOT trigger_row.tgisinternal
            AND trigger_row.tgrelid IN (
                SELECT DISTINCT expected.relation_oid
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
        RAISE EXCEPTION 'product deployment status trigger manifest is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            ('public.starring_canonical_json_v1(jsonb)', 'document jsonb'),
            ('public.starring_ruleset_content_hash_v1(bigint,jsonb)', 'schema_version bigint, definition jsonb')
    ) AS expected(signature, identity_arguments)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.signature)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'i'
        OR NOT function_row.proisstrict
        OR function_row.proparallel <> 's'
        OR function_row.prosecdef
        OR function_row.proretset
        OR function_row.prorows <> 0
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM 'plpgsql'
        OR pg_catalog.pg_get_function_identity_arguments(function_row.oid)
            IS DISTINCT FROM expected.identity_arguments
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM 'text'
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> function_row.proowner
        );
    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION 'product deployment status artifact helper contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO function_collision_count
    FROM (
        VALUES
            ('validate_runtime_deployment_projection'),
            ('enforce_runtime_deployment_policy_shadow'),
            ('guard_runtime_ruleset_artifact_transition'),
            ('reject_runtime_deployment_delete'),
            ('validate_runtime_attestation_projection'),
            ('reject_immutable_product_row'),
            ('validate_runtime_serving_lease_transition'),
            ('reject_runtime_serving_lease_delete'),
            ('reject_ruleset_artifact_mutation'),
            ('starring_canonical_json_v1'),
            ('starring_ruleset_content_hash_v1')
    ) AS expected(function_name)
    WHERE (
        SELECT pg_catalog.count(*)
        FROM pg_catalog.pg_proc AS function_row
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = function_row.pronamespace
        WHERE namespace.nspname = 'public'
            AND function_row.proname = expected.function_name
    ) <> 1;
    IF function_collision_count <> 0 THEN
        RAISE EXCEPTION 'product deployment status support function identity is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO artifact_support_count
    FROM pg_catalog.pg_attribute AS attribute
    INNER JOIN pg_catalog.pg_attrdef AS attribute_default
        ON attribute_default.adrelid = attribute.attrelid
        AND attribute_default.adnum = attribute.attnum
    INNER JOIN pg_catalog.pg_constraint AS constraint_row
        ON constraint_row.conrelid = attribute.attrelid
        AND constraint_row.conname = 'arv_content_integrity'
    WHERE attribute.attrelid = pg_catalog.to_regclass(
            'public.automation_ruleset_versions'
        )
        AND attribute.attname = 'canonical_content_hash'
        AND attribute.atttypid = pg_catalog.to_regtype('text')
        AND attribute.attnotnull = FALSE
        AND attribute.attgenerated = 's'
        AND pg_catalog.pg_get_expr(
            attribute_default.adbin,
            attribute_default.adrelid,
            FALSE
        ) = 'public.starring_ruleset_content_hash_v1(schema_version, definition)'
        AND constraint_row.contype = 'c'
        AND constraint_row.convalidated
        AND NOT constraint_row.connoinherit
        AND pg_catalog.pg_get_constraintdef(constraint_row.oid, FALSE)
            = 'CHECK (((canonical_content_hash IS NOT NULL) AND (canonical_content_hash = content_hash)))';
    IF artifact_support_count <> 1 THEN
        RAISE EXCEPTION 'product deployment status artifact support contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO function_collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_product_deployment_status_reader_database_identity_v1',
            'starring_product_deployment_status_read_v1'
        );
    IF function_collision_count <> 0 THEN
        RAISE EXCEPTION 'product deployment status function already exists'
            USING ERRCODE = '55000';
    END IF;

    EXECUTE $definition$
CREATE FUNCTION public.starring_product_deployment_status_reader_database_identity_v1()
RETURNS TEXT
LANGUAGE sql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
    SELECT identity.database_identity::TEXT
    FROM public.product_control_plane_identity AS identity
    WHERE identity.singleton;
$function$
$definition$;

    EXECUTE $definition$
CREATE FUNCTION public.starring_product_deployment_status_read_v1(
    expected_deployment_id TEXT,
    expected_promotion_id TEXT,
    expected_desired_target_digest TEXT,
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_guild_id TEXT,
    expected_principal_id TEXT,
    expected_acting_discord_user_id TEXT,
    expected_product_session_digest BYTEA
)
RETURNS TABLE(
    request_outcome TEXT,
    deployment_projection JSONB,
    activation_projection JSONB,
    promotion_projection JSONB,
    tenant_lifecycle_state TEXT,
    installation_projection JSONB,
    historical_authority_projection JSONB,
    current_authority_projection JSONB,
    active_target_version BIGINT,
    artifact_projection JSONB,
    attestation_projection JSONB,
    serving_projection JSONB,
    database_now TIMESTAMPTZ
)
LANGUAGE sql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
    WITH request_clock AS MATERIALIZED (
        SELECT pg_catalog.statement_timestamp() AS database_now
    ), valid_request AS MATERIALIZED (
        SELECT request_clock.database_now
        FROM request_clock
        WHERE expected_deployment_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
            AND expected_promotion_id ~ '^[0-9a-f]{64}$'
            AND expected_desired_target_digest ~ '^[0-9a-f]{64}$'
            AND expected_tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
            AND expected_installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
            AND expected_principal_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
            AND pg_catalog.octet_length(expected_product_session_digest) = 32
            AND CASE
                WHEN expected_guild_id ~ '^[1-9][0-9]{0,19}$'
                    THEN expected_guild_id::NUMERIC <= 18446744073709551615
                ELSE FALSE
            END
            AND CASE
                WHEN expected_acting_discord_user_id ~ '^[1-9][0-9]{0,19}$'
                    THEN expected_acting_discord_user_id::NUMERIC
                        <= 18446744073709551615
                ELSE FALSE
            END
    ), actor_deployment AS MATERIALIZED (
        SELECT deployment.*,
            valid_request.database_now,
            deployment.promotion_id = expected_promotion_id
                AND deployment.desired_target_digest = expected_desired_target_digest
                AND deployment.guild_id = expected_guild_id AS request_matches
        FROM valid_request
        INNER JOIN public.runtime_deployments AS deployment
            ON deployment.deployment_id = expected_deployment_id
            AND deployment.tenant_id = expected_tenant_id
            AND deployment.installation_id = expected_installation_id
        INNER JOIN public.product_principals AS principal
            ON principal.principal_id = expected_principal_id
            AND principal.discord_user_id = expected_acting_discord_user_id
            AND NOT principal.disabled
        INNER JOIN public.product_auth_sessions AS product_session
            ON product_session.principal_id = principal.principal_id
            AND product_session.session_digest = expected_product_session_digest
            AND pg_catalog.octet_length(product_session.csrf_digest) = 32
            AND pg_catalog.octet_length(product_session.oauth_state_digest) = 32
            AND product_session.revoked_at IS NULL
            AND product_session.revocation_reason IS NULL
            AND product_session.authenticated_at = product_session.created_at
            AND product_session.created_at <= product_session.last_seen_at
            AND product_session.last_seen_at < product_session.idle_expires_at
            AND product_session.idle_expires_at <= product_session.absolute_expires_at
            AND product_session.idle_expires_at
                <= product_session.last_seen_at + INTERVAL '30 minutes'
            AND product_session.absolute_expires_at
                <= product_session.authenticated_at + INTERVAL '12 hours'
            AND product_session.authenticated_at <= valid_request.database_now
            AND product_session.created_at <= valid_request.database_now
            AND product_session.last_seen_at <= valid_request.database_now
            AND valid_request.database_now < product_session.idle_expires_at
            AND valid_request.database_now < product_session.absolute_expires_at
    )
    SELECT
        CASE
            WHEN actor_deployment.request_matches THEN 'exact'::TEXT
            ELSE 'request_mismatch'::TEXT
        END AS request_outcome,
        CASE WHEN actor_deployment.request_matches THEN
            pg_catalog.jsonb_build_object(
                'evidence_format_version', 1,
                'row', pg_catalog.jsonb_build_object(
                    'deployment_id', actor_deployment.deployment_id,
                    'tenant_id', actor_deployment.tenant_id,
                    'installation_id', actor_deployment.installation_id,
                    'promotion_id', actor_deployment.promotion_id,
                    'activation_request_id', actor_deployment.activation_request_id,
                    'installation_authority_revision', actor_deployment.installation_authority_revision,
                    'guild_id', actor_deployment.guild_id,
                    'ruleset_key', actor_deployment.ruleset_key,
                    'target_version', actor_deployment.target_version,
                    'target_content_hash', actor_deployment.target_content_hash,
                    'binding_revision', actor_deployment.binding_revision,
                    'binding_fingerprint', actor_deployment.binding_fingerprint,
                    'desired_target_digest', actor_deployment.desired_target_digest,
                    'runtime_generation', actor_deployment.runtime_generation,
                    'previous_runtime', actor_deployment.previous_runtime,
                    'requested_at', actor_deployment.requested_at,
                    'snapshot_format_version', actor_deployment.snapshot_format_version,
                    'snapshot', actor_deployment.snapshot,
                    'revision', actor_deployment.revision,
                    'phase', actor_deployment.phase,
                    'controller_id', actor_deployment.controller_id,
                    'controller_fencing_token', actor_deployment.controller_fencing_token,
                    'controller_acquired_at', actor_deployment.controller_acquired_at,
                    'controller_lease_expires_at', actor_deployment.controller_lease_expires_at,
                    'last_fencing_token', actor_deployment.last_fencing_token,
                    'next_retry_at', actor_deployment.next_retry_at,
                    'last_stable_error_code', actor_deployment.last_stable_error_code,
                    'live_attestation_id', actor_deployment.live_attestation_id,
                    'live_at', actor_deployment.live_at,
                    'blocked_at', actor_deployment.blocked_at,
                    'superseded_at', actor_deployment.superseded_at,
                    'cancelled_at', actor_deployment.cancelled_at,
                    'created_at', actor_deployment.created_at,
                    'updated_at', actor_deployment.updated_at
                )
            )
        END AS deployment_projection,
        CASE WHEN actor_deployment.request_matches AND activation.id IS NOT NULL THEN
            pg_catalog.jsonb_build_object(
                'evidence_format_version', 1,
                'row', pg_catalog.jsonb_build_object(
                    'id', activation.id,
                    'tenant_id', activation.tenant_id,
                    'installation_id', activation.installation_id,
                    'guild_id', activation.guild_id,
                    'ruleset_key', activation.ruleset_key,
                    'target_version', activation.target_version,
                    'target_content_hash', activation.target_content_hash,
                    'state', activation.state,
                    'authority_kind', activation.authority_kind,
                    'link_state_name', activation.link_state_name,
                    'promotion_id', activation.promotion_id
                )
            )
        END AS activation_projection,
        CASE WHEN actor_deployment.request_matches AND promotion.id IS NOT NULL THEN
            pg_catalog.jsonb_build_object(
                'evidence_format_version', 1,
                'row', pg_catalog.jsonb_build_object(
                    'id', promotion.id,
                    'stage', promotion.stage,
                    'tenant_id', promotion.tenant_id,
                    'installation_id', promotion.installation_id,
                    'record_authority_tenant_id',
                        CASE WHEN pg_catalog.octet_length(
                            promotion.record #>> '{intent,authority,tenant_id}'
                        ) <= 128 THEN
                            promotion.record #>> '{intent,authority,tenant_id}'
                        END,
                    'record_authority_installation_id',
                        CASE WHEN pg_catalog.octet_length(
                            promotion.record #>> '{intent,authority,installation_id}'
                        ) <= 128 THEN
                            promotion.record #>> '{intent,authority,installation_id}'
                        END,
                    'record_authority_guild_id',
                        CASE WHEN pg_catalog.octet_length(
                            promotion.record #>> '{intent,authority,guild_id}'
                        ) <= 20 THEN
                            promotion.record #>> '{intent,authority,guild_id}'
                        END,
                    'record_authority_ruleset_key',
                        CASE WHEN pg_catalog.octet_length(
                            promotion.record #>> '{intent,authority,ruleset_key}'
                        ) <= 64 THEN
                            promotion.record #>> '{intent,authority,ruleset_key}'
                        END,
                    'record_authority_binding_revision',
                        CASE WHEN pg_catalog.octet_length(
                            promotion.record #>> '{intent,authority,binding_revision}'
                        ) <= 19 THEN
                            promotion.record #>> '{intent,authority,binding_revision}'
                        END,
                    'record_context_fingerprint',
                        CASE WHEN pg_catalog.octet_length(
                            promotion.record #>> '{intent,evidence,context_fingerprint}'
                        ) <= 64 THEN
                            promotion.record #>> '{intent,evidence,context_fingerprint}'
                        END,
                    'record_activation_request_id',
                        CASE WHEN pg_catalog.octet_length(
                            promotion.record #>> '{stage,activation,request_id}'
                        ) <= 64 THEN
                            promotion.record #>> '{stage,activation,request_id}'
                        END,
                    'record_activation_guild_id',
                        CASE WHEN pg_catalog.octet_length(
                            promotion.record #>> '{stage,activation,target,guild_id}'
                        ) <= 20 THEN
                            promotion.record #>> '{stage,activation,target,guild_id}'
                        END,
                    'record_activation_ruleset_key',
                        CASE WHEN pg_catalog.octet_length(
                            promotion.record #>> '{stage,activation,target,ruleset_key}'
                        ) <= 64 THEN
                            promotion.record #>> '{stage,activation,target,ruleset_key}'
                        END,
                    'record_activation_target_version',
                        CASE WHEN pg_catalog.octet_length(
                            promotion.record #>> '{stage,activation,target,version}'
                        ) <= 10 THEN
                            promotion.record #>> '{stage,activation,target,version}'
                        END,
                    'record_activation_target_content_hash',
                        CASE WHEN pg_catalog.octet_length(
                            promotion.record #>> '{stage,activation,target,content_hash}'
                        ) <= 64 THEN
                            promotion.record #>> '{stage,activation,target,content_hash}'
                        END
                )
            )
        END AS promotion_projection,
        CASE WHEN actor_deployment.request_matches THEN tenant.lifecycle_state END
            AS tenant_lifecycle_state,
        CASE WHEN actor_deployment.request_matches AND installation.installation_id IS NOT NULL THEN
            pg_catalog.jsonb_build_object(
                'evidence_format_version', 1,
                'row', pg_catalog.jsonb_build_object(
                    'installation_id', installation.installation_id,
                    'tenant_id', installation.tenant_id,
                    'discord_application_id', installation.discord_application_id,
                    'discord_guild_id', installation.discord_guild_id,
                    'ruleset_key', installation.ruleset_key,
                    'lifecycle_state', installation.lifecycle_state,
                    'current_authority_revision', installation.current_authority_revision
                )
            )
        END AS installation_projection,
        CASE WHEN actor_deployment.request_matches AND historical_authority.installation_id IS NOT NULL THEN
            pg_catalog.jsonb_build_object(
                'evidence_format_version', 1,
                'row', pg_catalog.jsonb_build_object(
                    'installation_id', historical_authority.installation_id,
                    'tenant_id', historical_authority.tenant_id,
                    'revision', historical_authority.revision,
                    'binding_revision', historical_authority.binding_revision,
                    'resource_bindings', historical_authority.resource_bindings,
                    'binding_fingerprint', historical_authority.binding_fingerprint
                )
            )
        END AS historical_authority_projection,
        CASE WHEN actor_deployment.request_matches AND current_authority.installation_id IS NOT NULL THEN
            pg_catalog.jsonb_build_object(
                'evidence_format_version', 1,
                'row', pg_catalog.jsonb_build_object(
                    'installation_id', current_authority.installation_id,
                    'tenant_id', current_authority.tenant_id,
                    'revision', current_authority.revision,
                    'binding_revision', current_authority.binding_revision,
                    'resource_bindings', current_authority.resource_bindings,
                    'binding_fingerprint', current_authority.binding_fingerprint,
                    'authority_payload_digest', current_authority.authority_payload_digest
                )
            )
        END AS current_authority_projection,
        CASE WHEN actor_deployment.request_matches THEN active.active_version END
            AS active_target_version,
        CASE WHEN actor_deployment.request_matches AND artifact.guild_id IS NOT NULL THEN
            pg_catalog.jsonb_build_object(
                'evidence_format_version', 1,
                'row', pg_catalog.jsonb_build_object(
                    'schema_version', artifact.schema_version,
                    'definition', artifact.definition,
                    'content_hash', artifact.content_hash,
                    'canonical_content_hash', artifact.canonical_content_hash
                )
            )
        END AS artifact_projection,
        CASE WHEN actor_deployment.request_matches AND attestation.attestation_id IS NOT NULL THEN
            pg_catalog.jsonb_build_object(
                'evidence_format_version', 1,
                'row', pg_catalog.jsonb_build_object(
                    'attestation_id', attestation.attestation_id,
                    'attestation_digest', attestation.attestation_digest,
                    'deployment_id', attestation.deployment_id,
                    'deployment_revision', attestation.deployment_revision,
                    'tenant_id', attestation.tenant_id,
                    'installation_id', attestation.installation_id,
                    'promotion_id', attestation.promotion_id,
                    'activation_request_id', attestation.activation_request_id,
                    'guild_id', attestation.guild_id,
                    'ruleset_key', attestation.ruleset_key,
                    'target_version', attestation.target_version,
                    'target_content_hash', attestation.target_content_hash,
                    'binding_revision', attestation.binding_revision,
                    'binding_fingerprint', attestation.binding_fingerprint,
                    'runtime_generation', attestation.runtime_generation,
                    'controller_fencing_token', attestation.controller_fencing_token,
                    'process_instance_id', attestation.process_instance_id,
                    'runtime_build_revision', attestation.runtime_build_revision,
                    'panel_certificate_id', attestation.panel_certificate_id,
                    'panel_report_digest', attestation.panel_report_digest,
                    'gateway_shard_id', attestation.gateway_shard_id,
                    'gateway_ready_kind', attestation.gateway_ready_kind,
                    'gateway_ready_at', attestation.gateway_ready_at,
                    'certified_at', attestation.certified_at,
                    'record_format_version', attestation.record_format_version,
                    'record', attestation.record,
                    'created_at', attestation.created_at
                )
            )
        END AS attestation_projection,
        CASE WHEN actor_deployment.request_matches AND serving.guild_id IS NOT NULL THEN
            pg_catalog.jsonb_build_object(
                'evidence_format_version', 1,
                'row', pg_catalog.jsonb_build_object(
                    'guild_id', serving.guild_id,
                    'ruleset_key', serving.ruleset_key,
                    'tenant_id', serving.tenant_id,
                    'installation_id', serving.installation_id,
                    'deployment_id', serving.deployment_id,
                    'attestation_id', serving.attestation_id,
                    'process_instance_id', serving.process_instance_id,
                    'runtime_generation', serving.runtime_generation,
                    'target_version', serving.target_version,
                    'target_content_hash', serving.target_content_hash,
                    'binding_revision', serving.binding_revision,
                    'binding_fingerprint', serving.binding_fingerprint,
                    'lease_epoch', serving.lease_epoch,
                    'revision', serving.revision,
                    'connected', serving.connected,
                    'serving', serving.serving,
                    'acquired_at', serving.acquired_at,
                    'last_heartbeat_at', serving.last_heartbeat_at,
                    'expires_at', serving.expires_at
                )
            )
        END AS serving_projection,
        actor_deployment.database_now
    FROM actor_deployment
    LEFT JOIN public.activation_requests AS activation
        ON actor_deployment.request_matches
        AND activation.id = actor_deployment.activation_request_id
    LEFT JOIN public.authoring_promotions AS promotion
        ON actor_deployment.request_matches
        AND promotion.id = actor_deployment.promotion_id
    LEFT JOIN public.product_tenants AS tenant
        ON actor_deployment.request_matches
        AND tenant.tenant_id = actor_deployment.tenant_id
    LEFT JOIN public.automation_installations AS installation
        ON actor_deployment.request_matches
        AND installation.tenant_id = actor_deployment.tenant_id
        AND installation.installation_id = actor_deployment.installation_id
    LEFT JOIN public.automation_installation_authority_versions AS historical_authority
        ON actor_deployment.request_matches
        AND historical_authority.tenant_id = actor_deployment.tenant_id
        AND historical_authority.installation_id = actor_deployment.installation_id
        AND historical_authority.revision
            = actor_deployment.installation_authority_revision
    LEFT JOIN public.automation_installation_authority_versions AS current_authority
        ON actor_deployment.request_matches
        AND current_authority.tenant_id = installation.tenant_id
        AND current_authority.installation_id = installation.installation_id
        AND current_authority.revision = installation.current_authority_revision
    LEFT JOIN public.automation_ruleset_activations AS active
        ON actor_deployment.request_matches
        AND active.guild_id = actor_deployment.guild_id
        AND active.ruleset_key = actor_deployment.ruleset_key
    LEFT JOIN public.automation_ruleset_versions AS artifact
        ON actor_deployment.request_matches
        AND artifact.guild_id = actor_deployment.guild_id
        AND artifact.ruleset_key = actor_deployment.ruleset_key
        AND artifact.version = actor_deployment.target_version
    LEFT JOIN public.runtime_attestations AS attestation
        ON actor_deployment.request_matches
        AND actor_deployment.phase = 'live'
        AND attestation.tenant_id = actor_deployment.tenant_id
        AND attestation.installation_id = actor_deployment.installation_id
        AND attestation.deployment_id = actor_deployment.deployment_id
        AND attestation.attestation_id = actor_deployment.live_attestation_id
    LEFT JOIN public.runtime_serving_leases AS serving
        ON actor_deployment.request_matches
        AND actor_deployment.phase = 'live'
        AND serving.guild_id = actor_deployment.guild_id
        AND serving.ruleset_key = actor_deployment.ruleset_key
    LIMIT 2;
$function$
$definition$;

    EXECUTE $probe$
        SELECT pg_catalog.count(*)
        FROM public.starring_product_deployment_status_read_v1(
            '',
            '',
            '',
            '',
            '',
            '',
            '',
            '',
            pg_catalog.decode('', 'hex')
        )
    $probe$
    INTO probe_count;
    IF probe_count <> 0 THEN
        RAISE EXCEPTION 'product deployment status read function probe is invalid'
            USING ERRCODE = '55000';
    END IF;

    FOR expected_signature IN
        SELECT expected.signature
        FROM (
            VALUES
                ('public.starring_product_deployment_status_reader_database_identity_v1()'),
                ('public.starring_product_deployment_status_read_v1(text,text,text,text,text,text,text,text,bytea)')
        ) AS expected(signature)
    LOOP
        function_oid := pg_catalog.to_regprocedure(expected_signature);
        IF function_oid IS NULL THEN
            RAISE EXCEPTION 'product deployment status reader function is unavailable'
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
            unexpected_grantee_name := pg_catalog.pg_get_userbyid(unexpected_grantee);
            IF unexpected_grantee_name IS NULL THEN
                RAISE EXCEPTION 'product deployment status grantee is unavailable'
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

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_product_deployment_status_reader_database_identity_v1()',
                '',
                'text',
                FALSE,
                0::REAL
            ),
            (
                'public.starring_product_deployment_status_read_v1(text,text,text,text,text,text,text,text,bytea)',
                'expected_deployment_id text, expected_promotion_id text, expected_desired_target_digest text, expected_tenant_id text, expected_installation_id text, expected_guild_id text, expected_principal_id text, expected_acting_discord_user_id text, expected_product_session_digest bytea',
                'TABLE(request_outcome text, deployment_projection jsonb, activation_projection jsonb, promotion_projection jsonb, tenant_lifecycle_state text, installation_projection jsonb, historical_authority_projection jsonb, current_authority_projection jsonb, active_target_version bigint, artifact_projection jsonb, attestation_projection jsonb, serving_projection jsonb, database_now timestamp with time zone)',
                TRUE,
                1::REAL
            )
    ) AS expected(signature, identity_arguments, result_name, returns_set, rows_estimate)
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
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proretset <> expected.returns_set
        OR function_row.prorows <> expected.rows_estimate
        OR language_row.lanname IS DISTINCT FROM 'sql'
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
        RAISE EXCEPTION 'product deployment status function contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO function_collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_product_deployment_status_reader_database_identity_v1',
            'starring_product_deployment_status_read_v1'
        );
    IF function_collision_count <> 2 THEN
        RAISE EXCEPTION 'product deployment status function identity is invalid'
            USING ERRCODE = '55000';
    END IF;

    PERFORM pg_catalog.set_config('search_path', original_search_path, TRUE);
    PERFORM pg_catalog.set_config(
        'quote_all_identifiers',
        original_quote_all_identifiers,
        TRUE
    );
END;
$scope$;
