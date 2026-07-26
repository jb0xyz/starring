SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

LOCK TABLE
    public.product_control_plane_identity,
    public.runtime_deployments,
    public.runtime_attestations,
    public.runtime_serving_leases
IN ACCESS SHARE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    relation_count BIGINT;
    ordinary_count BIGINT;
    persistent_count BIGINT;
    rls_disabled_count BIGINT;
    owner_count BIGINT;
    identity_count BIGINT;
    canonical_identity_count BIGINT;
    collision_count BIGINT;
    unsafe_schema_create_count BIGINT;
BEGIN
    SELECT pg_catalog.count(relation.oid),
        pg_catalog.count(relation.oid) FILTER (WHERE relation.relkind = 'r'),
        pg_catalog.count(relation.oid) FILTER (WHERE relation.relpersistence = 'p'),
        pg_catalog.count(relation.oid) FILTER (
            WHERE NOT relation.relrowsecurity AND NOT relation.relforcerowsecurity
        ),
        pg_catalog.count(DISTINCT relation.relowner),
        pg_catalog.min(relation.relowner::BIGINT)::OID
    INTO relation_count, ordinary_count, persistent_count, rls_disabled_count,
        owner_count, common_owner
    FROM (
        VALUES
            (pg_catalog.to_regclass('public.product_control_plane_identity')),
            (pg_catalog.to_regclass('public.runtime_deployments')),
            (pg_catalog.to_regclass('public.runtime_attestations')),
            (pg_catalog.to_regclass('public.runtime_serving_leases'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid;

    IF relation_count <> 4
        OR ordinary_count <> 4
        OR persistent_count <> 4
        OR rls_disabled_count <> 4
        OR owner_count <> 1
        OR common_owner IS NULL
    THEN
        RAISE EXCEPTION 'runtime convergence database relations are invalid'
            USING ERRCODE = '55000';
    END IF;

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR NOT pg_catalog.has_schema_privilege(common_owner_name, 'public', 'CREATE')
    THEN
        RAISE EXCEPTION 'runtime convergence database migration requires the common owner'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*),
        pg_catalog.count(*) FILTER (
            WHERE identity.singleton
                AND identity.database_identity IS NOT NULL
                AND identity.database_identity
                    <> '00000000-0000-0000-0000-000000000000'::UUID
                AND identity.database_identity::TEXT
                    ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
                AND identity.created_at IS NOT NULL
        )
    INTO identity_count, canonical_identity_count
    FROM public.product_control_plane_identity AS identity;

    IF identity_count <> 1 OR canonical_identity_count <> 1 THEN
        RAISE EXCEPTION 'runtime convergence database identity is invalid'
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
        AND privilege.grantee <> namespace.nspowner
        AND privilege.grantee <> pg_catalog.to_regrole('pg_database_owner');

    IF unsafe_schema_create_count <> 0 THEN
        RAISE EXCEPTION 'runtime convergence database schema is not trusted'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname = 'starring_runtime_convergence_database_identity_v1';

    IF collision_count <> 0 THEN
        RAISE EXCEPTION 'runtime convergence database identity function already exists'
            USING ERRCODE = '55000';
    END IF;
END;
$preflight$;

CREATE FUNCTION public.starring_runtime_convergence_database_identity_v1()
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
    WHERE identity.singleton
        AND identity.database_identity
            <> '00000000-0000-0000-0000-000000000000'::UUID
        AND identity.database_identity::TEXT
            ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$';
$function$;

DO $postflight$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    function_oid OID;
    unexpected_grantee OID;
    unexpected_grantee_name NAME;
    invalid_relation_count BIGINT;
    invalid_function_count BIGINT;
    identity_count BIGINT;
    canonical_identity TEXT;
    observed_identity TEXT;
BEGIN
    SELECT pg_catalog.min(relation.relowner::BIGINT)::OID
    INTO common_owner
    FROM (
        VALUES
            (pg_catalog.to_regclass('public.product_control_plane_identity')),
            (pg_catalog.to_regclass('public.runtime_deployments')),
            (pg_catalog.to_regclass('public.runtime_attestations')),
            (pg_catalog.to_regclass('public.runtime_serving_leases'))
    ) AS expected(relation_oid)
    INNER JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid;

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner IS NULL
        OR common_owner_name IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
    THEN
        RAISE EXCEPTION 'runtime convergence database relation owner is unavailable'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_relation_count
    FROM (
        VALUES
            (pg_catalog.to_regclass('public.product_control_plane_identity')),
            (pg_catalog.to_regclass('public.runtime_deployments')),
            (pg_catalog.to_regclass('public.runtime_attestations')),
            (pg_catalog.to_regclass('public.runtime_serving_leases'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid
    WHERE relation.oid IS NULL
        OR relation.relkind <> 'r'
        OR relation.relpersistence <> 'p'
        OR relation.relowner <> common_owner
        OR relation.relrowsecurity
        OR relation.relforcerowsecurity;

    IF invalid_relation_count <> 0 THEN
        RAISE EXCEPTION 'runtime convergence database relation contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    function_oid := pg_catalog.to_regprocedure(
        'public.starring_runtime_convergence_database_identity_v1()'
    );
    IF function_oid IS NULL THEN
        RAISE EXCEPTION 'runtime convergence database identity function is unavailable'
            USING ERRCODE = '55000';
    END IF;

    EXECUTE pg_catalog.format(
        'ALTER FUNCTION public.starring_runtime_convergence_database_identity_v1() OWNER TO %I',
        common_owner_name
    );
    REVOKE ALL PRIVILEGES ON FUNCTION
        public.starring_runtime_convergence_database_identity_v1()
    FROM PUBLIC CASCADE;

    FOR unexpected_grantee IN
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
        unexpected_grantee_name := pg_catalog.pg_get_userbyid(unexpected_grantee);
        IF unexpected_grantee_name IS NULL THEN
            RAISE EXCEPTION 'runtime convergence database function grantee is unavailable'
                USING ERRCODE = '55000';
        END IF;
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON FUNCTION public.starring_runtime_convergence_database_identity_v1() FROM %I CASCADE',
            unexpected_grantee_name
        );
    END LOOP;

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
            OR function_row.prorows <> 0::REAL
            OR function_row.proconfig
                IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
            OR function_row.proleakproof
            OR function_row.pronargs <> 0
            OR function_row.pronargdefaults <> 0
            OR function_row.provariadic <> 0
            OR language_row.lanname IS DISTINCT FROM 'sql'
            OR pg_catalog.pg_get_function_identity_arguments(function_row.oid) <> ''
            OR pg_catalog.pg_get_function_result(function_row.oid) <> 'text'
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE privilege.grantee <> common_owner
            )
        );

    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION 'runtime convergence database identity function contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*),
        pg_catalog.min(identity.database_identity::TEXT)
    INTO identity_count, canonical_identity
    FROM public.product_control_plane_identity AS identity
    WHERE identity.singleton
        AND identity.database_identity
            <> '00000000-0000-0000-0000-000000000000'::UUID
        AND identity.database_identity::TEXT
            ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
        AND identity.created_at IS NOT NULL;

    SELECT public.starring_runtime_convergence_database_identity_v1()
    INTO observed_identity;

    IF identity_count <> 1
        OR canonical_identity IS NULL
        OR observed_identity IS DISTINCT FROM canonical_identity
        OR observed_identity !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
        OR observed_identity = '00000000-0000-0000-0000-000000000000'
    THEN
        RAISE EXCEPTION 'runtime convergence database identity result is invalid'
            USING ERRCODE = '55000';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
