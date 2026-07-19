DO $scope$
DECLARE
    relation_count BIGINT;
    table_count BIGINT;
    rls_disabled_count BIGINT;
    owner_count BIGINT;
    common_owner OID;
    common_owner_name NAME;
    unsafe_schema_create_count BIGINT;
    function_collision_count BIGINT;
    function_oid OID;
    invalid_function_count BIGINT;
    unexpected_grantee OID;
    unexpected_grantee_name NAME;
    expected_signature TEXT;
    probe_count BIGINT;
    original_search_path TEXT;
    original_quote_all_identifiers TEXT;
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
            (pg_catalog.to_regclass('public.authoring_sessions')),
            (pg_catalog.to_regclass('public.authoring_session_generations')),
            (pg_catalog.to_regclass('public.product_principals')),
            (pg_catalog.to_regclass('public.product_auth_sessions')),
            (pg_catalog.to_regclass('public.runtime_deployments'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid;
    IF relation_count <> 12
        OR table_count <> 12
        OR rls_disabled_count <> 12
        OR owner_count <> 1
        OR common_owner IS NULL
    THEN
        RAISE EXCEPTION 'product decision reader relations require one non-RLS owner'
            USING ERRCODE = '55000';
    END IF;

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL THEN
        RAISE EXCEPTION 'product decision reader relation owner is unavailable'
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
        RAISE EXCEPTION 'product decision reader schema is not trusted'
            USING ERRCODE = '55000';
    END IF;

    IF pg_catalog.to_regrole(current_user) <> common_owner
        OR NOT pg_catalog.has_schema_privilege(
            common_owner_name,
            'public',
            'CREATE'
        )
    THEN
        RAISE EXCEPTION 'product decision reader migration requires the common owner'
            USING ERRCODE = '55000';
    END IF;

    function_oid := pg_catalog.to_regprocedure(
        'public.starring_product_decision_reader_database_identity_v1()'
    );
    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM pg_catalog.pg_proc AS function_row
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid = function_oid
        AND (
            function_row.proowner <> common_owner
            OR function_row.prokind <> 'f'
            OR function_row.provolatile <> 'v'
            OR NOT function_row.proisstrict
            OR function_row.proparallel <> 'u'
            OR NOT function_row.prosecdef
            OR function_row.proretset
            OR function_row.prorows <> 0
            OR function_row.proconfig
                IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
            OR function_row.proleakproof
            OR function_row.pronargdefaults <> 0
            OR function_row.provariadic <> 0
            OR language_row.lanname IS DISTINCT FROM 'sql'
            OR pg_catalog.pg_get_function_identity_arguments(function_row.oid)
                IS DISTINCT FROM ''
            OR pg_catalog.pg_get_function_result(function_row.oid)
                IS DISTINCT FROM 'text'
        );
    IF function_oid IS NULL OR invalid_function_count <> 0 THEN
        RAISE EXCEPTION 'product decision reader topology function contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO function_collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname = 'starring_product_decision_read_v1';
    IF function_collision_count <> 0 THEN
        RAISE EXCEPTION 'product decision read function already exists'
            USING ERRCODE = '55000';
    END IF;

    EXECUTE $definition$
CREATE FUNCTION public.starring_product_decision_read_v1(
    expected_promotion_id TEXT,
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_guild_id TEXT,
    expected_principal_id TEXT,
    expected_acting_discord_user_id TEXT,
    expected_product_session_digest BYTEA
)
RETURNS TABLE(
    activation_request_id TEXT,
    activation_tenant_id TEXT,
    activation_installation_id TEXT,
    activation_guild_id TEXT,
    activation_ruleset_key TEXT,
    activation_requester_id TEXT,
    activation_required_approvals INTEGER,
    activation_state TEXT,
    activation_created_at TIMESTAMPTZ,
    activation_expires_at TIMESTAMPTZ,
    activation_promotion_request_digest TEXT,
    activation_approval_payload_digest TEXT,
    activation_approval_context JSONB,
    activation_product_revision BIGINT,
    approval_count BIGINT,
    promotion_tenant_id TEXT,
    promotion_stage TEXT,
    promotion_request_digest TEXT,
    promotion_record JSONB,
    tenant_lifecycle_state TEXT,
    installation_application_id TEXT,
    installation_guild_id TEXT,
    installation_ruleset_key TEXT,
    installation_lifecycle_state TEXT,
    installation_current_authority_revision BIGINT,
    current_authority_payload_digest TEXT,
    promoted_session_owner_principal_id TEXT,
    promoted_session_owner_discord_user_id TEXT,
    promoted_generation_session_id TEXT,
    promoted_generation BIGINT,
    promoted_generation_stage TEXT,
    promoted_generation_candidate_revision BIGINT,
    promoted_generation_candidate_hash TEXT,
    promoted_generation_resource_bindings JSONB,
    promoted_generation_binding_fingerprint TEXT,
    historical_authority_binding_revision BIGINT,
    historical_authority_resource_bindings JSONB,
    historical_authority_resource_context_fingerprint TEXT,
    historical_authority_policy_revision BIGINT,
    historical_authority_required_approvals INTEGER,
    historical_authority_activation_ttl_seconds BIGINT,
    actor_discord_user_id TEXT,
    actor_disabled BOOLEAN,
    actor_session_revoked_at TIMESTAMPTZ,
    actor_session_idle_expires_at TIMESTAMPTZ,
    actor_session_absolute_expires_at TIMESTAMPTZ,
    runtime_deployment_id TEXT,
    runtime_desired_target_digest TEXT,
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
    )
    SELECT
        activation.id AS activation_request_id,
        activation.tenant_id AS activation_tenant_id,
        activation.installation_id AS activation_installation_id,
        activation.guild_id AS activation_guild_id,
        activation.ruleset_key AS activation_ruleset_key,
        activation.requester_id AS activation_requester_id,
        activation.required_approvals AS activation_required_approvals,
        activation.state AS activation_state,
        activation.created_at AS activation_created_at,
        activation.expires_at AS activation_expires_at,
        activation.promotion_request_digest AS activation_promotion_request_digest,
        activation.approval_payload_digest AS activation_approval_payload_digest,
        activation.approval_context AS activation_approval_context,
        activation.product_revision AS activation_product_revision,
        (
            SELECT pg_catalog.count(*)
            FROM public.activation_request_approvals AS approval
            WHERE approval.request_id = activation.id
        ) AS approval_count,
        promotion.tenant_id AS promotion_tenant_id,
        promotion.stage AS promotion_stage,
        promotion.request_digest AS promotion_request_digest,
        promotion.record AS promotion_record,
        tenant.lifecycle_state AS tenant_lifecycle_state,
        installation.discord_application_id AS installation_application_id,
        installation.discord_guild_id AS installation_guild_id,
        installation.ruleset_key AS installation_ruleset_key,
        installation.lifecycle_state AS installation_lifecycle_state,
        installation.current_authority_revision AS installation_current_authority_revision,
        current_authority.authority_payload_digest AS current_authority_payload_digest,
        promoted_session.owner_principal_id AS promoted_session_owner_principal_id,
        promoted_owner.discord_user_id AS promoted_session_owner_discord_user_id,
        promoted_generation.session_id AS promoted_generation_session_id,
        promoted_generation.generation AS promoted_generation,
        promoted_generation.stage AS promoted_generation_stage,
        promoted_generation.candidate_revision AS promoted_generation_candidate_revision,
        promoted_generation.candidate_hash AS promoted_generation_candidate_hash,
        promoted_generation.resource_bindings AS promoted_generation_resource_bindings,
        promoted_generation.binding_fingerprint AS promoted_generation_binding_fingerprint,
        historical_authority.binding_revision AS historical_authority_binding_revision,
        historical_authority.resource_bindings AS historical_authority_resource_bindings,
        historical_authority.binding_fingerprint
            AS historical_authority_resource_context_fingerprint,
        historical_authority.policy_revision AS historical_authority_policy_revision,
        historical_authority.required_approvals AS historical_authority_required_approvals,
        historical_authority.activation_ttl_seconds
            AS historical_authority_activation_ttl_seconds,
        principal.discord_user_id AS actor_discord_user_id,
        principal.disabled AS actor_disabled,
        actor_session.revoked_at AS actor_session_revoked_at,
        actor_session.idle_expires_at AS actor_session_idle_expires_at,
        actor_session.absolute_expires_at AS actor_session_absolute_expires_at,
        deployment.deployment_id AS runtime_deployment_id,
        deployment.desired_target_digest AS runtime_desired_target_digest,
        request_clock.database_now
    FROM public.activation_requests AS activation
    INNER JOIN public.authoring_promotions AS promotion
        ON promotion.id = activation.promotion_id
        AND promotion.tenant_id = activation.tenant_id
        AND promotion.installation_id = activation.installation_id
    INNER JOIN public.product_tenants AS tenant
        ON tenant.tenant_id = activation.tenant_id
    INNER JOIN public.automation_installations AS installation
        ON installation.tenant_id = activation.tenant_id
        AND installation.installation_id = activation.installation_id
    INNER JOIN public.automation_installation_authority_versions AS current_authority
        ON current_authority.tenant_id = installation.tenant_id
        AND current_authority.installation_id = installation.installation_id
        AND current_authority.revision = installation.current_authority_revision
    LEFT JOIN public.authoring_sessions AS promoted_session
        ON promoted_session.tenant_id = promotion.tenant_id
        AND promoted_session.installation_id = promotion.installation_id
        AND promoted_session.session_id = promotion.record #>> '{intent,authority,session_id}'
    LEFT JOIN public.authoring_session_generations AS promoted_generation
        ON promoted_generation.tenant_id = promoted_session.tenant_id
        AND promoted_generation.installation_id = promoted_session.installation_id
        AND promoted_generation.session_id = promoted_session.session_id
        AND promoted_generation.generation::TEXT
            = promotion.record #>> '{intent,authority,session_generation}'
    LEFT JOIN public.product_principals AS promoted_owner
        ON promoted_owner.principal_id = promoted_session.owner_principal_id
    LEFT JOIN public.automation_installation_authority_versions AS historical_authority
        ON historical_authority.tenant_id = promoted_generation.tenant_id
        AND historical_authority.installation_id = promoted_generation.installation_id
        AND historical_authority.revision = promoted_generation.installation_authority_revision
    INNER JOIN public.product_principals AS principal
        ON principal.principal_id = expected_principal_id
        AND principal.discord_user_id = expected_acting_discord_user_id
    INNER JOIN public.product_auth_sessions AS actor_session
        ON actor_session.principal_id = principal.principal_id
        AND actor_session.session_digest = expected_product_session_digest
    CROSS JOIN request_clock
    LEFT JOIN public.runtime_deployments AS deployment
        ON deployment.activation_request_id = activation.id
        AND deployment.tenant_id = activation.tenant_id
        AND deployment.installation_id = activation.installation_id
        AND deployment.promotion_id = activation.promotion_id
    WHERE activation.promotion_id = expected_promotion_id
        AND activation.tenant_id = expected_tenant_id
        AND activation.installation_id = expected_installation_id
        AND activation.guild_id = expected_guild_id
        AND activation.authority_kind = 'product_authoring'
        AND actor_session.oauth_state_digest IS NOT NULL
        AND expected_promotion_id ~ '^[0-9a-f]{64}$'
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
    LIMIT 2;
$function$
$definition$;

    EXECUTE $probe$
        SELECT pg_catalog.count(*)
        FROM public.starring_product_decision_read_v1(
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
        RAISE EXCEPTION 'product decision read function probe is invalid'
            USING ERRCODE = '55000';
    END IF;

    FOR expected_signature IN
        SELECT expected.signature
        FROM (
            VALUES
                ('public.starring_product_decision_reader_database_identity_v1()'),
                ('public.starring_product_decision_read_v1(text,text,text,text,text,text,bytea)')
        ) AS expected(signature)
    LOOP
        function_oid := pg_catalog.to_regprocedure(expected_signature);
        IF function_oid IS NULL THEN
            RAISE EXCEPTION 'product decision reader function is unavailable'
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
                RAISE EXCEPTION 'product decision reader grantee is unavailable'
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
                'public.starring_product_decision_reader_database_identity_v1()',
                '',
                'text',
                FALSE,
                0::REAL
            ),
            (
                'public.starring_product_decision_read_v1(text,text,text,text,text,text,bytea)',
                'expected_promotion_id text, expected_tenant_id text, expected_installation_id text, expected_guild_id text, expected_principal_id text, expected_acting_discord_user_id text, expected_product_session_digest bytea',
                'TABLE(activation_request_id text, activation_tenant_id text, activation_installation_id text, activation_guild_id text, activation_ruleset_key text, activation_requester_id text, activation_required_approvals integer, activation_state text, activation_created_at timestamp with time zone, activation_expires_at timestamp with time zone, activation_promotion_request_digest text, activation_approval_payload_digest text, activation_approval_context jsonb, activation_product_revision bigint, approval_count bigint, promotion_tenant_id text, promotion_stage text, promotion_request_digest text, promotion_record jsonb, tenant_lifecycle_state text, installation_application_id text, installation_guild_id text, installation_ruleset_key text, installation_lifecycle_state text, installation_current_authority_revision bigint, current_authority_payload_digest text, promoted_session_owner_principal_id text, promoted_session_owner_discord_user_id text, promoted_generation_session_id text, promoted_generation bigint, promoted_generation_stage text, promoted_generation_candidate_revision bigint, promoted_generation_candidate_hash text, promoted_generation_resource_bindings jsonb, promoted_generation_binding_fingerprint text, historical_authority_binding_revision bigint, historical_authority_resource_bindings jsonb, historical_authority_resource_context_fingerprint text, historical_authority_policy_revision bigint, historical_authority_required_approvals integer, historical_authority_activation_ttl_seconds bigint, actor_discord_user_id text, actor_disabled boolean, actor_session_revoked_at timestamp with time zone, actor_session_idle_expires_at timestamp with time zone, actor_session_absolute_expires_at timestamp with time zone, runtime_deployment_id text, runtime_desired_target_digest text, database_now timestamp with time zone)',
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
        RAISE EXCEPTION 'product decision reader function contract is invalid'
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
