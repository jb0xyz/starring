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
    manifest_definition_digest TEXT;
    readiness_definition_digest TEXT;
    collision_count BIGINT;
    invalid_acl_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO manifest_definition_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_exact_target_schema_manifest_v1()'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO readiness_definition_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_exact_target_database_readiness_v1()'
    );

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_runtime_exact_target_read_v2',
            'starring_runtime_exact_target_schema_manifest_v2',
            'starring_runtime_exact_target_database_readiness_v2'
        );

    SELECT pg_catalog.count(*)
    INTO invalid_acl_count
    FROM (
        VALUES
            (
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_exact_target_database_readiness_v1()'
                )
            ),
            (
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_exact_target_reader_database_identity_v1()'
                )
            ),
            (
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_exact_target_read_v1(text,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text)'
                )
            )
    ) AS expected(function_oid)
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        (
            SELECT function_row.proacl
            FROM pg_catalog.pg_proc AS function_row
            WHERE function_row.oid = expected.function_oid
        ),
        pg_catalog.acldefault('f', common_owner)
    )) AS privilege
    WHERE expected.function_oid IS NULL
        OR privilege.grantee = 0
        OR privilege.grantor <> common_owner
        OR privilege.privilege_type <> 'EXECUTE'
        OR privilege.is_grantable;

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR collision_count <> 0
        OR invalid_acl_count <> 0
        OR manifest_definition_digest IS DISTINCT FROM
            'b8dad14ddbb78262526673ae75a212ca11b1709ba0ee5a54f5125f55da471af7'
        OR readiness_definition_digest IS DISTINCT FROM
            '35903afa3bb9bebe712559a80a503823f4eeedf0d15ebd3d24ce3dbf706b5c14'
        OR NOT public.starring_runtime_exact_target_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_exact_target_v2_preflight_drift';
    END IF;
END;
$preflight$;

CREATE FUNCTION public.starring_runtime_exact_target_read_v2(
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
    desired_target_digest TEXT,
    installation_authority_revision BIGINT,
    installation_authority_payload_digest TEXT,
    installation_authority_policy_revision BIGINT,
    installation_authority_required_approvals INTEGER,
    installation_authority_activation_ttl_seconds BIGINT,
    current_authority_revision BIGINT,
    current_authority_payload_digest TEXT,
    current_authority_policy_revision BIGINT,
    current_authority_required_approvals INTEGER,
    current_authority_activation_ttl_seconds BIGINT,
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
    resource_bindings JSONB,
    database_observed_at TIMESTAMPTZ
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
        deployment.desired_target_digest,
        deployment.installation_authority_revision,
        historical_authority.authority_payload_digest,
        historical_authority.policy_revision,
        historical_authority.required_approvals,
        historical_authority.activation_ttl_seconds,
        installation.current_authority_revision,
        current_authority.authority_payload_digest,
        current_authority.policy_revision,
        current_authority.required_approvals,
        current_authority.activation_ttl_seconds,
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
            WHEN pg_catalog.octet_length(
                historical_authority.resource_bindings::TEXT
            ) <= 262144
            THEN historical_authority.resource_bindings
        END,
        request_clock.database_now
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
    INNER JOIN public.automation_installation_authority_versions
        AS historical_authority
        ON historical_authority.tenant_id = installation.tenant_id
        AND historical_authority.installation_id = installation.installation_id
        AND historical_authority.revision
            = deployment.installation_authority_revision
        AND historical_authority.binding_revision
            = deployment.binding_revision
        AND historical_authority.binding_fingerprint
            = deployment.binding_fingerprint
        AND historical_authority.policy_revision
            = deployment.policy_revision
    INNER JOIN public.automation_installation_authority_versions
        AS current_authority
        ON current_authority.tenant_id = installation.tenant_id
        AND current_authority.installation_id = installation.installation_id
        AND current_authority.revision = installation.current_authority_revision
        AND current_authority.binding_revision = deployment.binding_revision
        AND current_authority.binding_fingerprint
            = deployment.binding_fingerprint
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
        AND deployment.controller_fencing_token
            = expected_controller_fencing_token
        AND deployment.convergence_attempt_no
            = expected_convergence_attempt_no
        AND deployment.runtime_generation = expected_runtime_generation
        AND deployment.guild_id = expected_guild_id
        AND deployment.ruleset_key = expected_ruleset_key
        AND deployment.target_version = expected_target_version
        AND deployment.target_content_hash = expected_target_content_hash
        AND deployment.binding_revision = expected_binding_revision
        AND deployment.binding_fingerprint = expected_binding_fingerprint
        AND deployment.desired_target_digest_version = 1
        AND expected_deployment_revision BETWEEN 1 AND 9223372036854775807
        AND expected_controller_fencing_token
            BETWEEN 1 AND 9223372036854775807
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
        AND promotion.record
                OPERATOR(pg_catalog.#>>) '{intent,authority,tenant_id}'
            = deployment.tenant_id
        AND promotion.record
                OPERATOR(pg_catalog.#>>) '{intent,authority,installation_id}'
            = deployment.installation_id
        AND promotion.record
                OPERATOR(pg_catalog.#>>) '{intent,authority,guild_id}'
            = deployment.guild_id
        AND promotion.record
                OPERATOR(pg_catalog.#>>) '{intent,authority,ruleset_key}'
            = deployment.ruleset_key
        AND promotion.record
                OPERATOR(pg_catalog.#>>) '{intent,authority,binding_revision}'
            = deployment.binding_revision::TEXT
        AND promotion.record
                OPERATOR(pg_catalog.#>>) '{intent,evidence,context_fingerprint}'
            = deployment.binding_fingerprint
        AND promotion.record
                OPERATOR(pg_catalog.#>>) '{stage,activation,request_id}'
            = deployment.activation_request_id
        AND promotion.record
                OPERATOR(pg_catalog.#>>) '{stage,activation,target,guild_id}'
            = deployment.guild_id
        AND promotion.record
                OPERATOR(pg_catalog.#>>) '{stage,activation,target,ruleset_key}'
            = deployment.ruleset_key
        AND promotion.record
                OPERATOR(pg_catalog.#>>) '{stage,activation,target,version}'
            = deployment.target_version::TEXT
        AND promotion.record
                OPERATOR(pg_catalog.#>>) '{stage,activation,target,content_hash}'
            = deployment.target_content_hash
    LIMIT 2;
$function$;

REVOKE ALL ON FUNCTION public.starring_runtime_exact_target_read_v2(
    TEXT,
    TEXT,
    TEXT,
    TEXT,
    TEXT,
    BIGINT,
    TEXT,
    BIGINT,
    BIGINT,
    BIGINT,
    TEXT,
    TEXT,
    BIGINT,
    TEXT,
    BIGINT,
    TEXT
) FROM PUBLIC;

CREATE FUNCTION public.starring_runtime_exact_target_schema_manifest_v2()
RETURNS BOOLEAN
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    common_owner OID;
    legacy_manifest_definition_digest TEXT;
    read_body_digest TEXT;
    invalid_read_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO legacy_manifest_definition_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_exact_target_schema_manifest_v1()'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            function_row.prosrc,
            'UTF8'
        )),
        'hex'
    )
    INTO read_body_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_exact_target_read_v2(text,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text)'
    );

    SELECT pg_catalog.count(*)
    INTO invalid_read_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_runtime_exact_target_read_v2(text,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text)'
        )
        AND (
            function_row.proowner <> common_owner
            OR function_row.prokind <> 'f'
            OR function_row.provolatile <> 'v'
            OR NOT function_row.proisstrict
            OR function_row.proparallel <> 'u'
            OR NOT function_row.prosecdef
            OR NOT function_row.proretset
            OR function_row.prorows <> 1::REAL
            OR function_row.proconfig
                IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
            OR function_row.proleakproof
            OR function_row.pronargdefaults <> 0
            OR function_row.provariadic <> 0
            OR language_row.lanname <> 'sql'
            OR pg_catalog.pg_get_function_identity_arguments(
                function_row.oid
            ) <> 'expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_promotion_id text, expected_activation_request_id text, expected_deployment_revision bigint, expected_controller_id text, expected_controller_fencing_token bigint, expected_convergence_attempt_no bigint, expected_runtime_generation bigint, expected_guild_id text, expected_ruleset_key text, expected_target_version bigint, expected_target_content_hash text, expected_binding_revision bigint, expected_binding_fingerprint text'
            OR pg_catalog.pg_get_function_result(function_row.oid)
                <> 'TABLE(deployment_revision bigint, convergence_attempt_no bigint, desired_target_digest text, installation_authority_revision bigint, installation_authority_payload_digest text, installation_authority_policy_revision bigint, installation_authority_required_approvals integer, installation_authority_activation_ttl_seconds bigint, current_authority_revision bigint, current_authority_payload_digest text, current_authority_policy_revision bigint, current_authority_required_approvals integer, current_authority_activation_ttl_seconds bigint, guild_id text, ruleset_key text, target_version bigint, schema_version bigint, definition jsonb, content_hash text, canonical_content_hash text, created_by text, binding_revision bigint, binding_fingerprint text, resource_bindings jsonb, database_observed_at timestamp with time zone)'
        );

    RETURN common_owner IS NOT NULL
        AND pg_catalog.to_regrole(current_user) = common_owner
        AND legacy_manifest_definition_digest
            = 'b8dad14ddbb78262526673ae75a212ca11b1709ba0ee5a54f5125f55da471af7'
        AND read_body_digest
            = '8b483f4be1500f75d56b969419b888207d6ad1f6ce65028670ebce41593bfa6b'
        AND invalid_read_count = 0
        AND public.starring_runtime_exact_target_schema_manifest_v1();
END;
$function$;

REVOKE ALL ON FUNCTION
    public.starring_runtime_exact_target_schema_manifest_v2()
FROM PUBLIC;

CREATE FUNCTION public.starring_runtime_exact_target_database_readiness_v2()
RETURNS TABLE(
    database_identity TEXT,
    database_name TEXT,
    executor_role TEXT,
    checked_at TIMESTAMPTZ
)
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
DECLARE
    common_owner OID;
    database_owner OID;
    database_oid OID;
    invoker_oid OID;
    invalid_relation_count BIGINT;
    invalid_function_count BIGINT;
    invalid_support_function_count BIGINT;
    identity_count BIGINT;
    unexpected_capability_count BIGINT;
    unsafe_schema_count BIGINT;
    unsafe_default_count BIGINT;
    unsafe_system_count BIGINT;
    role_found BOOLEAN;
    role_row RECORD;
BEGIN
    IF pg_catalog.current_setting('role') <> 'none' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_exact_target_database_role_drift';
    END IF;

    invoker_oid := pg_catalog.to_regrole(session_user);
    SELECT role.rolsuper,
        role.rolinherit,
        role.rolcreaterole,
        role.rolcreatedb,
        role.rolcanlogin,
        role.rolreplication,
        role.rolbypassrls,
        role.rolconnlimit,
        role.rolconfig,
        role.rolname
    INTO role_row
    FROM pg_catalog.pg_roles AS role
    WHERE role.oid = invoker_oid;
    role_found := FOUND;

    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT database_row.oid, database_row.datdba
    INTO database_oid, database_owner
    FROM pg_catalog.pg_database AS database_row
    WHERE database_row.datname = pg_catalog.current_database();

    IF NOT FOUND
        OR NOT role_found
        OR invoker_oid IS NULL
        OR common_owner IS NULL
        OR database_oid IS NULL
        OR database_owner IS NULL
        OR invoker_oid IN (common_owner, database_owner)
        OR role_row.rolsuper
        OR role_row.rolinherit
        OR role_row.rolcreaterole
        OR role_row.rolcreatedb
        OR NOT role_row.rolcanlogin
        OR role_row.rolreplication
        OR role_row.rolbypassrls
        OR role_row.rolconnlimit NOT BETWEEN 1 AND 4
        OR COALESCE(pg_catalog.cardinality(role_row.rolconfig), 0) <> 0
        OR role_row.rolname::TEXT !~ '^[a-z_][a-z0-9_]{0,62}$'
        OR pg_catalog.current_database() !~ '^[a-z_][a-z0-9_]{0,62}$'
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_auth_members AS membership
            WHERE membership.member = invoker_oid
                OR membership.roleid = invoker_oid
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_db_role_setting AS setting
            WHERE (
                    setting.setrole = invoker_oid
                    AND setting.setdatabase IN (0, database_oid)
                )
                OR (
                    setting.setrole = 0
                    AND setting.setdatabase = database_oid
                )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_exact_target_database_role_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_relation_count
    FROM (
        VALUES
            ('public.product_control_plane_identity'),
            ('public.runtime_deployments'),
            ('public.activation_requests'),
            ('public.authoring_promotions'),
            ('public.product_tenants'),
            ('public.automation_installations'),
            ('public.automation_installation_authority_versions'),
            ('public.automation_ruleset_activations'),
            ('public.automation_ruleset_versions')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = pg_catalog.to_regclass(expected.identity)
    WHERE relation.oid IS NULL
        OR relation.relkind <> 'r'
        OR relation.relpersistence <> 'p'
        OR relation.relowner <> common_owner
        OR relation.relrowsecurity
        OR relation.relforcerowsecurity
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                relation.relacl,
                pg_catalog.acldefault('r', relation.relowner)
            )) AS privilege
            WHERE privilege.grantee <> relation.relowner
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_attribute AS attribute
            CROSS JOIN LATERAL pg_catalog.aclexplode(attribute.attacl) AS privilege
            WHERE attribute.attrelid = relation.oid
                AND attribute.attnum > 0
                AND NOT attribute.attisdropped
                AND privilege.grantee <> relation.relowner
        );

    IF invalid_relation_count <> 0
        OR NOT public.starring_runtime_exact_target_schema_manifest_v2()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_exact_target_database_schema_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_runtime_exact_target_database_readiness_v2()',
                ''::TEXT,
                'TABLE(database_identity text, database_name text, executor_role text, checked_at timestamp with time zone)'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_exact_target_reader_database_identity_v1()',
                ''::TEXT,
                'text'::TEXT,
                'sql'::TEXT,
                FALSE,
                0::REAL
            ),
            (
                'public.starring_runtime_exact_target_read_v2(text,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text)',
                'expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_promotion_id text, expected_activation_request_id text, expected_deployment_revision bigint, expected_controller_id text, expected_controller_fencing_token bigint, expected_convergence_attempt_no bigint, expected_runtime_generation bigint, expected_guild_id text, expected_ruleset_key text, expected_target_version bigint, expected_target_content_hash text, expected_binding_revision bigint, expected_binding_fingerprint text'::TEXT,
                'TABLE(deployment_revision bigint, convergence_attempt_no bigint, desired_target_digest text, installation_authority_revision bigint, installation_authority_payload_digest text, installation_authority_policy_revision bigint, installation_authority_required_approvals integer, installation_authority_activation_ttl_seconds bigint, current_authority_revision bigint, current_authority_payload_digest text, current_authority_policy_revision bigint, current_authority_required_approvals integer, current_authority_activation_ttl_seconds bigint, guild_id text, ruleset_key text, target_version bigint, schema_version bigint, definition jsonb, content_hash text, canonical_content_hash text, created_by text, binding_revision bigint, binding_fingerprint text, resource_bindings jsonb, database_observed_at timestamp with time zone)'::TEXT,
                'sql'::TEXT,
                TRUE,
                1::REAL
            )
    ) AS expected(identity, arguments, result, language_name, returns_set, rows_estimate)
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
        OR function_row.proretset IS DISTINCT FROM expected.returns_set
        OR function_row.prorows IS DISTINCT FROM expected.rows_estimate
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM expected.language_name
        OR pg_catalog.pg_get_function_identity_arguments(function_row.oid)
            IS DISTINCT FROM expected.arguments
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM expected.result
        OR NOT pg_catalog.has_function_privilege(
            invoker_oid,
            function_row.oid,
            'EXECUTE'
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee NOT IN (common_owner, invoker_oid)
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
                OR privilege.grantor <> common_owner
        );

    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_exact_target_database_function_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_support_function_count
    FROM (
        VALUES
            (
                'public.starring_runtime_exact_target_schema_manifest_v2()',
                ''::TEXT,
                'boolean'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                'e6b483ea123b1a235652088acd2c4229c24042c21ac407fc8dd4ae97c809489f'::TEXT
            )
    ) AS expected(
        identity,
        arguments,
        result,
        language_name,
        is_strict,
        definition_digest
    )
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR function_row.proisstrict IS DISTINCT FROM expected.is_strict
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR function_row.proretset
        OR function_row.prorows <> 0::REAL
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM expected.language_name
        OR pg_catalog.pg_get_function_identity_arguments(function_row.oid)
            IS DISTINCT FROM expected.arguments
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM expected.result
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(function_row.oid),
                'UTF8'
            )),
            'hex'
        ) IS DISTINCT FROM expected.definition_digest
        OR pg_catalog.has_function_privilege(
            invoker_oid,
            function_row.oid,
            'EXECUTE'
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
                OR privilege.grantor <> common_owner
        );

    IF invalid_support_function_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_exact_target_database_support_function_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO unsafe_schema_count
    FROM pg_catalog.pg_namespace AS namespace
    WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema')
        AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_'
        AND (
            pg_catalog.has_schema_privilege(invoker_oid, namespace.oid, 'CREATE')
            OR (
                namespace.nspname <> 'public'
                AND pg_catalog.has_schema_privilege(invoker_oid, namespace.oid, 'USAGE')
            )
        );

    SELECT pg_catalog.count(*)
    INTO unsafe_default_count
    FROM pg_catalog.pg_default_acl AS defaults
    CROSS JOIN LATERAL pg_catalog.aclexplode(defaults.defaclacl) AS privilege
    WHERE privilege.grantee IN (0, invoker_oid);

    SELECT pg_catalog.count(*)
    INTO unexpected_capability_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE function_row.oid >= 16384
        AND pg_catalog.has_function_privilege(
            invoker_oid,
            function_row.oid,
            'EXECUTE'
        )
        AND function_row.oid NOT IN (
            pg_catalog.to_regprocedure(
                'public.starring_runtime_exact_target_database_readiness_v2()'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_exact_target_reader_database_identity_v1()'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_exact_target_read_v2(text,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text)'
            )
        )
        AND namespace.nspname NOT IN ('pg_catalog', 'information_schema')
        AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_';

    IF unexpected_capability_count <> 0
        OR unsafe_schema_count <> 0
        OR unsafe_default_count <> 0
        OR NOT pg_catalog.has_database_privilege(invoker_oid, database_oid, 'CONNECT')
        OR NOT pg_catalog.has_schema_privilege(invoker_oid, 'public', 'USAGE')
        OR pg_catalog.has_database_privilege(invoker_oid, database_oid, 'CREATE')
        OR pg_catalog.has_database_privilege(invoker_oid, database_oid, 'TEMPORARY')
        OR pg_catalog.has_schema_privilege(invoker_oid, 'public', 'CREATE')
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_database AS database_row
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                database_row.datacl,
                pg_catalog.acldefault('d', database_row.datdba)
            )) AS privilege
            WHERE database_row.oid = database_oid
                AND privilege.grantee IN (0, invoker_oid)
                AND privilege.is_grantable
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_namespace AS namespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                namespace.nspacl,
                pg_catalog.acldefault('n', namespace.nspowner)
            )) AS privilege
            WHERE namespace.nspname = 'public'
                AND privilege.grantee IN (0, invoker_oid)
                AND privilege.is_grantable
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_exact_target_database_capability_drift';
    END IF;

    WITH violations(kind) AS (
        SELECT 'system_namespace'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_namespace AS namespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                namespace.nspacl,
                pg_catalog.acldefault('n', namespace.nspowner)
            )) AS privilege
            WHERE (
                    namespace.nspname = 'information_schema'
                    OR pg_catalog.left(namespace.nspname::TEXT, 3) = 'pg_'
                )
                AND (
                    namespace.nspowner = invoker_oid
                    OR privilege.grantee = invoker_oid
                    OR (
                        privilege.grantee = 0
                        AND (
                            privilege.is_grantable
                            OR (
                                NOT (
                                    namespace.nspname = 'information_schema'
                                    AND privilege.privilege_type = 'USAGE'
                                )
                                AND NOT EXISTS (
                                    SELECT 1
                                    FROM pg_catalog.aclexplode(COALESCE(
                                        (
                                            SELECT initial.initprivs
                                            FROM pg_catalog.pg_init_privs AS initial
                                            WHERE initial.classoid
                                                    = 'pg_catalog.pg_namespace'::REGCLASS
                                                AND initial.objoid = namespace.oid
                                                AND initial.objsubid = 0
                                        ),
                                        pg_catalog.acldefault(
                                            'n',
                                            namespace.nspowner
                                        )
                                    )) AS initial_privilege
                                    WHERE initial_privilege.grantee = 0
                                        AND initial_privilege.privilege_type
                                            = privilege.privilege_type
                                )
                            )
                        )
                    )
                )
        )
        UNION ALL
        SELECT 'system_relation'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_class AS relation
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = relation.relnamespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                relation.relacl,
                pg_catalog.acldefault('r', relation.relowner)
            )) AS privilege
            WHERE (
                    namespace.nspname = 'information_schema'
                    OR pg_catalog.left(namespace.nspname::TEXT, 3) = 'pg_'
                )
                AND relation.relkind IN ('r', 'p', 'v', 'm', 'f')
                AND (
                    relation.relowner = invoker_oid
                    OR privilege.grantee = invoker_oid
                    OR (
                        privilege.grantee = 0
                        AND (
                            privilege.is_grantable
                            OR (
                                NOT (
                                    namespace.nspname = 'information_schema'
                                    AND privilege.privilege_type = 'SELECT'
                                )
                                AND NOT EXISTS (
                                    SELECT 1
                                    FROM pg_catalog.aclexplode(COALESCE(
                                        (
                                            SELECT initial.initprivs
                                            FROM pg_catalog.pg_init_privs AS initial
                                            WHERE initial.classoid
                                                    = 'pg_catalog.pg_class'::REGCLASS
                                                AND initial.objoid = relation.oid
                                                AND initial.objsubid = 0
                                        ),
                                        pg_catalog.acldefault(
                                            'r',
                                            relation.relowner
                                        )
                                    )) AS initial_privilege
                                    WHERE initial_privilege.grantee = 0
                                        AND initial_privilege.privilege_type
                                            = privilege.privilege_type
                                )
                            )
                        )
                    )
                )
        )
        UNION ALL
        SELECT 'system_attribute'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_attribute AS attribute
            INNER JOIN pg_catalog.pg_class AS relation
                ON relation.oid = attribute.attrelid
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = relation.relnamespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(attribute.attacl) AS privilege
            WHERE (
                    namespace.nspname = 'information_schema'
                    OR pg_catalog.left(namespace.nspname::TEXT, 3) = 'pg_'
                )
                AND attribute.attnum > 0
                AND NOT attribute.attisdropped
                AND (
                    privilege.grantee = invoker_oid
                    OR (
                        privilege.grantee = 0
                        AND (
                            privilege.is_grantable
                            OR NOT EXISTS (
                                SELECT 1
                                FROM pg_catalog.aclexplode(COALESCE(
                                    (
                                        SELECT initial.initprivs
                                        FROM pg_catalog.pg_init_privs AS initial
                                        WHERE initial.classoid
                                                = 'pg_catalog.pg_class'::REGCLASS
                                            AND initial.objoid = relation.oid
                                            AND initial.objsubid = attribute.attnum
                                    ),
                                    pg_catalog.acldefault(
                                        'c',
                                        relation.relowner
                                    )
                                )) AS initial_privilege
                                WHERE initial_privilege.grantee = 0
                                    AND initial_privilege.privilege_type
                                        = privilege.privilege_type
                            )
                        )
                    )
                )
        )
        UNION ALL
        SELECT 'system_sequence'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_class AS sequence
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = sequence.relnamespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                sequence.relacl,
                pg_catalog.acldefault('s', sequence.relowner)
            )) AS privilege
            WHERE (
                    namespace.nspname = 'information_schema'
                    OR pg_catalog.left(namespace.nspname::TEXT, 3) = 'pg_'
                )
                AND sequence.relkind = 'S'
                AND (
                    sequence.relowner = invoker_oid
                    OR privilege.grantee = invoker_oid
                    OR (
                        privilege.grantee = 0
                        AND (
                            privilege.is_grantable
                            OR NOT EXISTS (
                                SELECT 1
                                FROM pg_catalog.aclexplode(COALESCE(
                                    (
                                        SELECT initial.initprivs
                                        FROM pg_catalog.pg_init_privs AS initial
                                        WHERE initial.classoid
                                                = 'pg_catalog.pg_class'::REGCLASS
                                            AND initial.objoid = sequence.oid
                                            AND initial.objsubid = 0
                                    ),
                                    pg_catalog.acldefault(
                                        's',
                                        sequence.relowner
                                    )
                                )) AS initial_privilege
                                WHERE initial_privilege.grantee = 0
                                    AND initial_privilege.privilege_type
                                        = privilege.privilege_type
                            )
                        )
                    )
                )
        )
        UNION ALL
        SELECT 'system_function'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_proc AS function_row
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = function_row.pronamespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE (
                    namespace.nspname = 'information_schema'
                    OR pg_catalog.left(namespace.nspname::TEXT, 3) = 'pg_'
                )
                AND (
                    function_row.proowner = invoker_oid
                    OR privilege.grantee = invoker_oid
                    OR (
                        privilege.grantee = 0
                        AND (
                            privilege.is_grantable
                            OR function_row.oid >= 16384
                            OR NOT EXISTS (
                                SELECT 1
                                FROM pg_catalog.aclexplode(COALESCE(
                                    (
                                        SELECT initial.initprivs
                                        FROM pg_catalog.pg_init_privs AS initial
                                        WHERE initial.classoid
                                                = 'pg_catalog.pg_proc'::REGCLASS
                                            AND initial.objoid = function_row.oid
                                            AND initial.objsubid = 0
                                    ),
                                    pg_catalog.acldefault(
                                        'f',
                                        function_row.proowner
                                    )
                                )) AS initial_privilege
                                WHERE initial_privilege.grantee = 0
                                    AND initial_privilege.privilege_type
                                        = privilege.privilege_type
                            )
                        )
                    )
                )
        )
        UNION ALL
        SELECT 'system_type'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_type AS type_row
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = type_row.typnamespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                type_row.typacl,
                pg_catalog.acldefault('T', type_row.typowner)
            )) AS privilege
            WHERE (
                    namespace.nspname = 'information_schema'
                    OR pg_catalog.left(namespace.nspname::TEXT, 3) = 'pg_'
                )
                AND (
                    type_row.typowner = invoker_oid
                    OR privilege.grantee = invoker_oid
                    OR (
                        privilege.grantee = 0
                        AND (
                            privilege.is_grantable
                            OR type_row.oid >= 16384
                            OR NOT EXISTS (
                                SELECT 1
                                FROM pg_catalog.aclexplode(COALESCE(
                                    (
                                        SELECT initial.initprivs
                                        FROM pg_catalog.pg_init_privs AS initial
                                        WHERE initial.classoid
                                                = 'pg_catalog.pg_type'::REGCLASS
                                            AND initial.objoid = type_row.oid
                                            AND initial.objsubid = 0
                                    ),
                                    pg_catalog.acldefault(
                                        'T',
                                        type_row.typowner
                                    )
                                )) AS initial_privilege
                                WHERE initial_privilege.grantee = 0
                                    AND initial_privilege.privilege_type
                                        = privilege.privilege_type
                            )
                        )
                    )
                )
        )
        UNION ALL
        SELECT 'application_relation'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_class AS relation
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = relation.relnamespace
            WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema')
                AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_'
                AND relation.relkind IN ('r', 'p', 'v', 'm', 'f')
                AND (
                    pg_catalog.has_table_privilege(
                        invoker_oid,
                        relation.oid,
                        'SELECT'
                    )
                    OR pg_catalog.has_table_privilege(
                        invoker_oid,
                        relation.oid,
                        'INSERT'
                    )
                    OR pg_catalog.has_table_privilege(
                        invoker_oid,
                        relation.oid,
                        'UPDATE'
                    )
                    OR pg_catalog.has_table_privilege(
                        invoker_oid,
                        relation.oid,
                        'DELETE'
                    )
                    OR pg_catalog.has_table_privilege(
                        invoker_oid,
                        relation.oid,
                        'TRUNCATE'
                    )
                    OR pg_catalog.has_table_privilege(
                        invoker_oid,
                        relation.oid,
                        'REFERENCES'
                    )
                    OR pg_catalog.has_table_privilege(
                        invoker_oid,
                        relation.oid,
                        'TRIGGER'
                    )
                )
        )
        UNION ALL
        SELECT 'application_attribute'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_attribute AS attribute
            INNER JOIN pg_catalog.pg_class AS relation
                ON relation.oid = attribute.attrelid
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = relation.relnamespace
            WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema')
                AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_'
                AND attribute.attnum > 0
                AND NOT attribute.attisdropped
                AND (
                    pg_catalog.has_column_privilege(
                        invoker_oid,
                        relation.oid,
                        attribute.attname,
                        'SELECT'
                    )
                    OR pg_catalog.has_column_privilege(
                        invoker_oid,
                        relation.oid,
                        attribute.attname,
                        'INSERT'
                    )
                    OR pg_catalog.has_column_privilege(
                        invoker_oid,
                        relation.oid,
                        attribute.attname,
                        'UPDATE'
                    )
                    OR pg_catalog.has_column_privilege(
                        invoker_oid,
                        relation.oid,
                        attribute.attname,
                        'REFERENCES'
                    )
                )
        )
        UNION ALL
        SELECT 'application_sequence'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_class AS sequence
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = sequence.relnamespace
            WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema')
                AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_'
                AND sequence.relkind = 'S'
                AND (
                    pg_catalog.has_sequence_privilege(
                        invoker_oid,
                        sequence.oid,
                        'USAGE'
                    )
                    OR pg_catalog.has_sequence_privilege(
                        invoker_oid,
                        sequence.oid,
                        'SELECT'
                    )
                    OR pg_catalog.has_sequence_privilege(
                        invoker_oid,
                        sequence.oid,
                        'UPDATE'
                    )
                )
        )
        UNION ALL
        SELECT 'parameter_acl'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_parameter_acl AS parameter_acl
            CROSS JOIN LATERAL pg_catalog.aclexplode(parameter_acl.paracl) AS privilege
            WHERE privilege.grantee IN (0, invoker_oid)
                AND privilege.privilege_type IN ('SET', 'ALTER SYSTEM')
        )
        UNION ALL
        SELECT 'large_object'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_largeobject_metadata AS large_object
            WHERE large_object.lomowner = invoker_oid
                OR EXISTS (
                    SELECT 1
                    FROM pg_catalog.aclexplode(COALESCE(
                        large_object.lomacl,
                        pg_catalog.acldefault('L', large_object.lomowner)
                    )) AS privilege
                    WHERE privilege.grantee IN (0, invoker_oid)
                        AND (
                            privilege.privilege_type IN ('SELECT', 'UPDATE')
                            OR privilege.is_grantable
                        )
                )
        )
    )
    SELECT pg_catalog.count(*)
    INTO unsafe_system_count
    FROM violations;

    IF unsafe_system_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_exact_target_database_system_capability_drift';
    END IF;

    SELECT pg_catalog.count(*),
        pg_catalog.min(identity.database_identity::TEXT)
    INTO identity_count, database_identity
    FROM public.product_control_plane_identity AS identity
    WHERE identity.singleton
        AND identity.database_identity IS NOT NULL
        AND identity.database_identity
            <> '00000000-0000-0000-0000-000000000000'::UUID
        AND identity.database_identity::TEXT
            ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
        AND identity.created_at IS NOT NULL;

    IF identity_count <> 1 OR database_identity IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_exact_target_database_identity_drift';
    END IF;

    database_name := pg_catalog.current_database()::TEXT;
    executor_role := session_user::TEXT;
    checked_at := pg_catalog.clock_timestamp();
    RETURN NEXT;
END;
$function$;

REVOKE ALL ON FUNCTION
    public.starring_runtime_exact_target_database_readiness_v2()
FROM PUBLIC;

DO $capability_cutover$
DECLARE
    common_owner OID;
    grantee OID;
    grantee_name NAME;
    mismatch_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    WITH expected(function_oid) AS (
        VALUES
            (
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_exact_target_database_readiness_v1()'
                )
            ),
            (
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_exact_target_reader_database_identity_v1()'
                )
            ),
            (
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_exact_target_read_v1(text,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text)'
                )
            )
    ), granted AS (
        SELECT expected.function_oid,
            privilege.grantee
        FROM expected
        CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
            (
                SELECT function_row.proacl
                FROM pg_catalog.pg_proc AS function_row
                WHERE function_row.oid = expected.function_oid
            ),
            pg_catalog.acldefault('f', common_owner)
        )) AS privilege
        WHERE privilege.grantee <> common_owner
            AND privilege.privilege_type = 'EXECUTE'
    )
    SELECT pg_catalog.count(*)
    INTO mismatch_count
    FROM (
        SELECT granted.grantee
        FROM granted
        GROUP BY granted.grantee
        HAVING pg_catalog.count(DISTINCT function_oid) <> 3
    ) AS mismatch;

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR mismatch_count <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_exact_target_v2_capability_topology_drift';
    END IF;

    FOR grantee IN
        SELECT DISTINCT privilege.grantee
        FROM pg_catalog.pg_proc AS function_row
        CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
            function_row.proacl,
            pg_catalog.acldefault('f', function_row.proowner)
        )) AS privilege
        WHERE function_row.oid = pg_catalog.to_regprocedure(
                'public.starring_runtime_exact_target_read_v1(text,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text)'
            )
            AND privilege.grantee <> common_owner
            AND privilege.privilege_type = 'EXECUTE'
    LOOP
        grantee_name := pg_catalog.pg_get_userbyid(grantee);
        IF grantee_name IS NULL THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_exact_target_v2_capability_grantee_drift';
        END IF;
        EXECUTE pg_catalog.format(
            'GRANT EXECUTE ON FUNCTION public.starring_runtime_exact_target_read_v2(text,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text) TO %I',
            grantee_name
        );
        EXECUTE pg_catalog.format(
            'GRANT EXECUTE ON FUNCTION public.starring_runtime_exact_target_database_readiness_v2() TO %I',
            grantee_name
        );
        EXECUTE pg_catalog.format(
            'REVOKE EXECUTE ON FUNCTION public.starring_runtime_exact_target_read_v1(text,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text) FROM %I',
            grantee_name
        );
        EXECUTE pg_catalog.format(
            'REVOKE EXECUTE ON FUNCTION public.starring_runtime_exact_target_database_readiness_v1() FROM %I',
            grantee_name
        );
    END LOOP;
END;
$capability_cutover$;

DO $postflight$
DECLARE
    common_owner OID;
    invalid_function_count BIGINT;
    invalid_legacy_acl_count BIGINT;
    manifest_definition_digest TEXT;
    readiness_definition_digest TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_runtime_exact_target_read_v2(text,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text)'
            ),
            (
                'public.starring_runtime_exact_target_schema_manifest_v2()'
            ),
            (
                'public.starring_runtime_exact_target_database_readiness_v2()'
            )
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR NOT function_row.proisstrict
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0;

    SELECT pg_catalog.count(*)
    INTO invalid_legacy_acl_count
    FROM (
        VALUES
            (
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_exact_target_read_v1(text,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text)'
                )
            ),
            (
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_exact_target_database_readiness_v1()'
                )
            )
    ) AS expected(function_oid)
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        (
            SELECT function_row.proacl
            FROM pg_catalog.pg_proc AS function_row
            WHERE function_row.oid = expected.function_oid
        ),
        pg_catalog.acldefault('f', common_owner)
    )) AS privilege
    WHERE privilege.grantee <> common_owner;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO manifest_definition_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_exact_target_schema_manifest_v2()'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO readiness_definition_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_exact_target_database_readiness_v2()'
    );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR invalid_function_count <> 0
        OR invalid_legacy_acl_count <> 0
        OR manifest_definition_digest IS DISTINCT FROM
            'e6b483ea123b1a235652088acd2c4229c24042c21ac407fc8dd4ae97c809489f'
        OR readiness_definition_digest IS DISTINCT FROM
            'f51338cfbfc8d360c90d6ebdbfeebf8c8f5c26165e1d53b837d38cb670733d46'
        OR NOT public.starring_runtime_exact_target_schema_manifest_v2()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_exact_target_v2_postflight_drift';
    END IF;
END;
$postflight$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
