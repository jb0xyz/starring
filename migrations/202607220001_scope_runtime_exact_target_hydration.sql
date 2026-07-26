SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

LOCK TABLE
    public.product_control_plane_identity,
    public.runtime_deployments,
    public.activation_requests,
    public.authoring_promotions,
    public.product_tenants,
    public.automation_installations,
    public.automation_installation_authority_versions,
    public.automation_ruleset_activations,
    public.automation_ruleset_versions
IN ACCESS SHARE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    relation_count BIGINT;
    ordinary_count BIGINT;
    owner_count BIGINT;
    collision_count BIGINT;
    unsafe_schema_create_count BIGINT;
BEGIN
    SELECT pg_catalog.count(relation.oid),
        pg_catalog.count(relation.oid) FILTER (WHERE relation.relkind = 'r'),
        pg_catalog.count(DISTINCT relation.relowner),
        pg_catalog.min(relation.relowner::BIGINT)::OID
    INTO relation_count, ordinary_count, owner_count, common_owner
    FROM (
        VALUES
            (pg_catalog.to_regclass('public.product_control_plane_identity')),
            (pg_catalog.to_regclass('public.runtime_deployments')),
            (pg_catalog.to_regclass('public.activation_requests')),
            (pg_catalog.to_regclass('public.authoring_promotions')),
            (pg_catalog.to_regclass('public.product_tenants')),
            (pg_catalog.to_regclass('public.automation_installations')),
            (pg_catalog.to_regclass('public.automation_installation_authority_versions')),
            (pg_catalog.to_regclass('public.automation_ruleset_activations')),
            (pg_catalog.to_regclass('public.automation_ruleset_versions'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid;

    IF relation_count <> 9
        OR ordinary_count <> 9
        OR owner_count <> 1
        OR common_owner IS NULL
    THEN
        RAISE EXCEPTION 'runtime hydration relations require one ordinary-table owner'
            USING ERRCODE = '55000';
    END IF;

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR NOT pg_catalog.has_schema_privilege(common_owner_name, 'public', 'CREATE')
    THEN
        RAISE EXCEPTION 'runtime hydration migration requires the common owner'
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
    IF unsafe_schema_create_count <> 0 THEN
        RAISE EXCEPTION 'runtime hydration schema is not trusted'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_runtime_exact_target_reader_database_identity_v1',
            'starring_runtime_exact_target_read_v1'
        );
    IF collision_count <> 0 THEN
        RAISE EXCEPTION 'runtime hydration function already exists'
            USING ERRCODE = '55000';
    END IF;
END;
$preflight$;

CREATE FUNCTION public.starring_runtime_exact_target_reader_database_identity_v1()
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
$function$;

CREATE FUNCTION public.starring_runtime_exact_target_read_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_promotion_id TEXT,
    expected_activation_request_id TEXT,
    expected_deployment_revision BIGINT,
    expected_controller_id TEXT,
    expected_controller_fencing_token BIGINT,
    expected_convergence_attempt_no BIGINT,
    expected_runtime_generation BIGINT,
    expected_guild_id TEXT,
    expected_ruleset_key TEXT,
    expected_target_version BIGINT,
    expected_target_content_hash TEXT,
    expected_binding_revision BIGINT,
    expected_binding_fingerprint TEXT
)
RETURNS TABLE(
    deployment_revision BIGINT,
    convergence_attempt_no BIGINT,
    installation_authority_revision BIGINT,
    current_authority_revision BIGINT,
    guild_id TEXT,
    ruleset_key TEXT,
    target_version BIGINT,
    schema_version BIGINT,
    definition JSONB,
    content_hash TEXT,
    canonical_content_hash TEXT,
    created_by TEXT,
    binding_revision BIGINT,
    binding_fingerprint TEXT,
    resource_bindings JSONB
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
    SELECT deployment.revision,
        deployment.convergence_attempt_no,
        deployment.installation_authority_revision,
        installation.current_authority_revision,
        version.guild_id,
        version.ruleset_key,
        version.version,
        version.schema_version,
        CASE
            WHEN pg_catalog.octet_length(version.definition::TEXT) <= 524288
            THEN version.definition
        END,
        version.content_hash,
        version.canonical_content_hash,
        version.created_by,
        historical_authority.binding_revision,
        historical_authority.binding_fingerprint,
        CASE
            WHEN pg_catalog.octet_length(historical_authority.resource_bindings::TEXT) <= 262144
            THEN historical_authority.resource_bindings
        END
    FROM public.runtime_deployments AS deployment
    INNER JOIN public.activation_requests AS activation
        ON activation.id = deployment.activation_request_id
        AND activation.guild_id = deployment.guild_id
        AND activation.ruleset_key = deployment.ruleset_key
        AND activation.target_version = deployment.target_version
        AND activation.target_content_hash = deployment.target_content_hash
        AND activation.state = 'applied'
        AND activation.authority_kind = 'product_authoring'
        AND activation.link_state_name = 'linked'
        AND activation.promotion_id = deployment.promotion_id
    INNER JOIN public.authoring_promotions AS promotion
        ON promotion.id = deployment.promotion_id
        AND promotion.stage = 'activation_pending'
        AND promotion.tenant_id = deployment.tenant_id
    INNER JOIN public.product_tenants AS tenant
        ON tenant.tenant_id = deployment.tenant_id
        AND tenant.lifecycle_state = 'active'
    INNER JOIN public.automation_installations AS installation
        ON installation.tenant_id = deployment.tenant_id
        AND installation.installation_id = deployment.installation_id
        AND installation.discord_guild_id = deployment.guild_id
        AND installation.ruleset_key = deployment.ruleset_key
        AND installation.lifecycle_state = 'active'
    INNER JOIN public.automation_installation_authority_versions AS historical_authority
        ON historical_authority.tenant_id = installation.tenant_id
        AND historical_authority.installation_id = installation.installation_id
        AND historical_authority.revision = deployment.installation_authority_revision
        AND historical_authority.binding_revision = deployment.binding_revision
        AND historical_authority.binding_fingerprint = deployment.binding_fingerprint
    INNER JOIN public.automation_installation_authority_versions AS current_authority
        ON current_authority.tenant_id = installation.tenant_id
        AND current_authority.installation_id = installation.installation_id
        AND current_authority.revision = installation.current_authority_revision
        AND current_authority.binding_revision = deployment.binding_revision
        AND current_authority.binding_fingerprint = deployment.binding_fingerprint
        AND current_authority.resource_bindings
            IS NOT DISTINCT FROM historical_authority.resource_bindings
    INNER JOIN public.automation_ruleset_activations AS active
        ON active.guild_id = deployment.guild_id
        AND active.ruleset_key = deployment.ruleset_key
        AND active.active_version = deployment.target_version
    INNER JOIN public.automation_ruleset_versions AS version
        ON version.guild_id = active.guild_id
        AND version.ruleset_key = active.ruleset_key
        AND version.version = active.active_version
        AND version.content_hash = deployment.target_content_hash
        AND version.canonical_content_hash = version.content_hash
    CROSS JOIN request_clock
    WHERE deployment.tenant_id = expected_tenant_id
        AND deployment.installation_id = expected_installation_id
        AND deployment.deployment_id = expected_deployment_id
        AND deployment.promotion_id = expected_promotion_id
        AND deployment.activation_request_id = expected_activation_request_id
        AND deployment.revision = expected_deployment_revision
        AND deployment.controller_id = expected_controller_id
        AND deployment.controller_fencing_token = expected_controller_fencing_token
        AND deployment.convergence_attempt_no = expected_convergence_attempt_no
        AND deployment.runtime_generation = expected_runtime_generation
        AND deployment.guild_id = expected_guild_id
        AND deployment.ruleset_key = expected_ruleset_key
        AND deployment.target_version = expected_target_version
        AND deployment.target_content_hash = expected_target_content_hash
        AND deployment.binding_revision = expected_binding_revision
        AND deployment.binding_fingerprint = expected_binding_fingerprint
        AND expected_deployment_revision BETWEEN 1 AND 9223372036854775807
        AND expected_controller_fencing_token BETWEEN 1 AND 9223372036854775807
        AND expected_convergence_attempt_no BETWEEN 1 AND 4294967295
        AND expected_runtime_generation BETWEEN 1 AND 9223372036854775807
        AND expected_target_version BETWEEN 1 AND 4294967295
        AND expected_target_content_hash ~ '^[0-9a-f]{64}$'
        AND expected_binding_revision BETWEEN 1 AND 9223372036854775807
        AND expected_binding_fingerprint ~ '^[0-9a-f]{64}$'
        AND deployment.phase IN (
            'requested',
            'preflight_ready',
            'drain_requested',
            'drained',
            'activation_applying',
            'runtime_pending',
            'reconciling_panels',
            'awaiting_gateway_ready'
        )
        AND deployment.blocked_at IS NULL
        AND deployment.controller_acquired_at <= request_clock.database_now
        AND deployment.controller_lease_expires_at > request_clock.database_now
        AND (
            deployment.next_retry_at IS NULL
            OR deployment.next_retry_at <= request_clock.database_now
        )
        AND promotion.record OPERATOR(pg_catalog.#>>) '{intent,authority,tenant_id}'
            = deployment.tenant_id
        AND promotion.record OPERATOR(pg_catalog.#>>) '{intent,authority,installation_id}'
            = deployment.installation_id
        AND promotion.record OPERATOR(pg_catalog.#>>) '{intent,authority,guild_id}'
            = deployment.guild_id
        AND promotion.record OPERATOR(pg_catalog.#>>) '{intent,authority,ruleset_key}'
            = deployment.ruleset_key
        AND promotion.record OPERATOR(pg_catalog.#>>) '{intent,authority,binding_revision}'
            = deployment.binding_revision::TEXT
        AND promotion.record OPERATOR(pg_catalog.#>>) '{intent,evidence,context_fingerprint}'
            = deployment.binding_fingerprint
        AND promotion.record OPERATOR(pg_catalog.#>>) '{stage,activation,request_id}'
            = deployment.activation_request_id
        AND promotion.record OPERATOR(pg_catalog.#>>) '{stage,activation,target,guild_id}'
            = deployment.guild_id
        AND promotion.record OPERATOR(pg_catalog.#>>) '{stage,activation,target,ruleset_key}'
            = deployment.ruleset_key
        AND promotion.record OPERATOR(pg_catalog.#>>) '{stage,activation,target,version}'
            = deployment.target_version::TEXT
        AND promotion.record OPERATOR(pg_catalog.#>>) '{stage,activation,target,content_hash}'
            = deployment.target_content_hash
    LIMIT 2;
$function$;

DO $postflight$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    function_identity TEXT;
    function_oid OID;
    grantee OID;
    grantee_name NAME;
    invalid_function_count BIGINT;
BEGIN
    SELECT pg_catalog.min(relation.relowner::BIGINT)::OID
    INTO common_owner
    FROM (
        VALUES
            (pg_catalog.to_regclass('public.product_control_plane_identity')),
            (pg_catalog.to_regclass('public.runtime_deployments')),
            (pg_catalog.to_regclass('public.activation_requests')),
            (pg_catalog.to_regclass('public.authoring_promotions')),
            (pg_catalog.to_regclass('public.product_tenants')),
            (pg_catalog.to_regclass('public.automation_installations')),
            (pg_catalog.to_regclass('public.automation_installation_authority_versions')),
            (pg_catalog.to_regclass('public.automation_ruleset_activations')),
            (pg_catalog.to_regclass('public.automation_ruleset_versions'))
    ) AS expected(relation_oid)
    INNER JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid;
    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL THEN
        RAISE EXCEPTION 'runtime hydration relation owner is unavailable'
            USING ERRCODE = '55000';
    END IF;

    FOR function_identity IN
        SELECT identity
        FROM (
            VALUES
                ('public.starring_runtime_exact_target_reader_database_identity_v1()'),
                ('public.starring_runtime_exact_target_read_v1(text,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text)')
        ) AS expected(identity)
    LOOP
        function_oid := pg_catalog.to_regprocedure(function_identity);
        IF function_oid IS NULL THEN
            RAISE EXCEPTION 'runtime hydration function is unavailable'
                USING ERRCODE = '55000';
        END IF;
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s OWNER TO %I',
            function_identity,
            common_owner_name
        );
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE',
            function_identity
        );
        FOR grantee IN
            SELECT DISTINCT privilege.grantee
            FROM pg_catalog.pg_proc AS function_row
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE function_row.oid = function_oid
                AND privilege.grantee <> 0
                AND privilege.grantee <> common_owner
        LOOP
            grantee_name := pg_catalog.pg_get_userbyid(grantee);
            IF grantee_name IS NULL THEN
                RAISE EXCEPTION 'runtime hydration function grantee is unavailable'
                    USING ERRCODE = '55000';
            END IF;
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM %I CASCADE',
                function_identity,
                grantee_name
            );
        END LOOP;
    END LOOP;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            ('public.starring_runtime_exact_target_reader_database_identity_v1()', ''::TEXT, 'text'::TEXT, FALSE, 0::REAL),
            ('public.starring_runtime_exact_target_read_v1(text,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text)', 'expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_promotion_id text, expected_activation_request_id text, expected_deployment_revision bigint, expected_controller_id text, expected_controller_fencing_token bigint, expected_convergence_attempt_no bigint, expected_runtime_generation bigint, expected_guild_id text, expected_ruleset_key text, expected_target_version bigint, expected_target_content_hash text, expected_binding_revision bigint, expected_binding_fingerprint text'::TEXT, 'TABLE(deployment_revision bigint, convergence_attempt_no bigint, installation_authority_revision bigint, current_authority_revision bigint, guild_id text, ruleset_key text, target_version bigint, schema_version bigint, definition jsonb, content_hash text, canonical_content_hash text, created_by text, binding_revision bigint, binding_fingerprint text, resource_bindings jsonb)'::TEXT, TRUE, 1::REAL)
    ) AS expected(identity, identity_arguments, result_name, returns_set, rows_estimate)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
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
            WHERE privilege.grantee <> common_owner
        );
    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION 'runtime hydration function contract is invalid'
            USING ERRCODE = '55000';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
