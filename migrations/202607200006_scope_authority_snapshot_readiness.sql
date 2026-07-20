DO $preflight$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    identity_count BIGINT;
    relation_count BIGINT;
    ordinary_count BIGINT;
    rls_disabled_count BIGINT;
    owner_count BIGINT;
    collision_count BIGINT;
    unsafe_schema_create_count BIGINT;
BEGIN
    SELECT pg_catalog.count(relation.oid),
        pg_catalog.count(relation.oid) FILTER (WHERE relation.relkind = 'r'),
        pg_catalog.count(relation.oid) FILTER (
            WHERE NOT relation.relrowsecurity AND NOT relation.relforcerowsecurity
        ),
        pg_catalog.count(DISTINCT relation.relowner),
        pg_catalog.min(relation.relowner::BIGINT)::OID
    INTO relation_count, ordinary_count, rls_disabled_count, owner_count, common_owner
    FROM (
        VALUES
            (pg_catalog.to_regclass('public.product_control_plane_identity')),
            (pg_catalog.to_regclass('public.product_principals')),
            (pg_catalog.to_regclass('public.product_auth_sessions')),
            (pg_catalog.to_regclass('public.product_tenants')),
            (pg_catalog.to_regclass('public.automation_installations')),
            (pg_catalog.to_regclass('public.automation_installation_authority_versions')),
            (pg_catalog.to_regclass('public.authoring_sessions')),
            (pg_catalog.to_regclass('public.authoring_session_generations'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid;

    IF relation_count <> 8
        OR ordinary_count <> 8
        OR rls_disabled_count <> 8
        OR owner_count <> 1
        OR common_owner IS NULL
    THEN
        RAISE EXCEPTION 'authority and snapshot relations require one non-RLS owner'
            USING ERRCODE = '55000';
    END IF;

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR NOT pg_catalog.has_schema_privilege(common_owner_name, 'public', 'CREATE')
    THEN
        RAISE EXCEPTION 'authority and snapshot migration requires the common owner'
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
        RAISE EXCEPTION 'authority and snapshot schema is not trusted'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_product_installation_authority_reader_database_identity_v1',
            'starring_product_authorized_snapshot_reader_database_identity_v1',
            'starring_product_authorized_snapshot_key_coverage_v1'
        );
    IF collision_count <> 0 THEN
        RAISE EXCEPTION 'authority and snapshot readiness function already exists'
            USING ERRCODE = '55000';
    END IF;
END;
$preflight$;

CREATE FUNCTION public.starring_product_installation_authority_reader_database_identity_v1()
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

CREATE FUNCTION public.starring_product_authorized_snapshot_reader_database_identity_v1()
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

CREATE FUNCTION public.starring_product_authorized_snapshot_key_coverage_v1(
    configured_encryption_key_ids TEXT[]
)
RETURNS TABLE(covered BOOLEAN)
LANGUAGE sql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
    WITH input AS MATERIALIZED (
        SELECT configured_encryption_key_ids AS key_ids,
            pg_catalog.cardinality(configured_encryption_key_ids) AS key_count,
            CASE
                WHEN pg_catalog.cardinality(configured_encryption_key_ids)
                    BETWEEN 1 AND 8
                THEN configured_encryption_key_ids
                ELSE ARRAY[]::TEXT[]
            END AS bounded_key_ids
    ), configured AS MATERIALIZED (
        SELECT configured_key.key_id
        FROM input
        CROSS JOIN LATERAL pg_catalog.unnest(input.bounded_key_ids)
            AS configured_key(key_id)
    ), valid_input AS MATERIALIZED (
        SELECT input.key_count BETWEEN 1 AND 8
            AND input.key_count = (
                SELECT pg_catalog.count(DISTINCT configured.key_id)
                FROM configured
            )
            AND NOT EXISTS (
                SELECT 1
                FROM configured
                WHERE configured.key_id IS NULL
                    OR CASE
                        WHEN pg_catalog.octet_length(configured.key_id)
                            BETWEEN 1 AND 128
                        THEN configured.key_id !~ '^[A-Za-z0-9_.:/-]+$'
                        ELSE TRUE
                    END
            ) AS valid
        FROM input
    )
    SELECT CASE
        WHEN valid_input.valid THEN NOT EXISTS (
            SELECT 1
            FROM public.authoring_session_generations AS generation
            WHERE NOT generation.encryption_key_id = ANY(input.bounded_key_ids)
        )
        ELSE FALSE
    END AS covered
    FROM input
    CROSS JOIN valid_input;
$function$;

DO $postflight$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    function_identity TEXT;
    function_oid OID;
    grantee OID;
    grantee_name NAME;
    default_schema_name NAME;
    default_grantee_clause TEXT;
    user_schema_name NAME;
    invalid_function_count BIGINT;
    function_identity_count BIGINT;
    unexpected_routine_identity TEXT;
    coverage_rows BIGINT;
    coverage_value BOOLEAN;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.product_control_plane_identity'
    );

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner IS NULL
        OR common_owner_name IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
    THEN
        RAISE EXCEPTION 'authority and snapshot owner changed during migration'
            USING ERRCODE = '55000';
    END IF;

    EXECUTE pg_catalog.format(
        'ALTER DEFAULT PRIVILEGES FOR ROLE %I REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC',
        common_owner_name
    );

    FOR user_schema_name IN
        SELECT namespace.nspname
        FROM pg_catalog.pg_namespace AS namespace
        WHERE namespace.nspname <> 'information_schema'
            AND pg_catalog.left(namespace.nspname, 3) <> 'pg_'
        ORDER BY namespace.nspname
    LOOP
        EXECUTE pg_catalog.format(
            'ALTER DEFAULT PRIVILEGES FOR ROLE %I IN SCHEMA %I REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC',
            common_owner_name,
            user_schema_name
        );
    END LOOP;

    FOR default_schema_name, grantee IN
        SELECT namespace.nspname, privilege.grantee
        FROM pg_catalog.pg_default_acl AS default_acl
        CROSS JOIN LATERAL pg_catalog.aclexplode(default_acl.defaclacl) AS privilege
        LEFT JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = default_acl.defaclnamespace
        WHERE default_acl.defaclrole = common_owner
            AND default_acl.defaclobjtype = 'f'
            AND privilege.grantee <> common_owner
            AND (
                default_acl.defaclnamespace = 0
                OR (
                    namespace.nspname <> 'information_schema'
                    AND pg_catalog.left(namespace.nspname, 3) <> 'pg_'
                )
            )
        ORDER BY namespace.nspname NULLS FIRST, privilege.grantee
    LOOP
        default_grantee_clause := CASE
            WHEN grantee = 0 THEN 'PUBLIC'
            ELSE pg_catalog.quote_ident(pg_catalog.pg_get_userbyid(grantee))
        END;
        IF default_grantee_clause IS NULL THEN
            RAISE EXCEPTION 'authority and snapshot default grantee is unavailable'
                USING ERRCODE = '55000';
        END IF;
        IF default_schema_name IS NULL THEN
            EXECUTE pg_catalog.format(
                'ALTER DEFAULT PRIVILEGES FOR ROLE %I REVOKE ALL PRIVILEGES ON FUNCTIONS FROM %s',
                common_owner_name,
                default_grantee_clause
            );
        ELSE
            EXECUTE pg_catalog.format(
                'ALTER DEFAULT PRIVILEGES FOR ROLE %I IN SCHEMA %I REVOKE ALL PRIVILEGES ON FUNCTIONS FROM %s',
                common_owner_name,
                default_schema_name,
                default_grantee_clause
            );
        END IF;
    END LOOP;

    FOR function_identity IN
        SELECT expected.identity
        FROM (
            VALUES
                ('public.starring_product_installation_authority_reader_database_identity_v1()'),
                ('public.starring_product_authorized_snapshot_reader_database_identity_v1()'),
                ('public.starring_product_authorized_snapshot_key_coverage_v1(text[])')
        ) AS expected(identity)
    LOOP
        function_oid := pg_catalog.to_regprocedure(function_identity);
        IF function_oid IS NULL THEN
            RAISE EXCEPTION 'authority and snapshot readiness function is unavailable'
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
                RAISE EXCEPTION 'authority and snapshot function grantee is unavailable'
                    USING ERRCODE = '55000';
            END IF;
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM %I CASCADE',
                function_identity,
                grantee_name
            );
        END LOOP;
    END LOOP;

    SELECT pg_catalog.min(pg_catalog.format(
        '%I.%I(%s)',
        namespace.nspname,
        function_row.proname,
        pg_catalog.pg_get_function_identity_arguments(function_row.oid)
    ))
    INTO unexpected_routine_identity
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE namespace.nspname <> 'information_schema'
        AND pg_catalog.left(namespace.nspname, 3) <> 'pg_'
        AND function_row.prokind IN ('f', 'p')
        AND privilege.grantee = 0
        AND privilege.privilege_type = 'EXECUTE';
    IF unexpected_routine_identity IS NOT NULL THEN
        RAISE EXCEPTION 'user routine public execution is not sealed: %',
            unexpected_routine_identity
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM pg_catalog.pg_default_acl AS default_acl
        CROSS JOIN LATERAL pg_catalog.aclexplode(default_acl.defaclacl) AS privilege
        LEFT JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = default_acl.defaclnamespace
        WHERE default_acl.defaclrole = common_owner
            AND default_acl.defaclobjtype = 'f'
            AND (
                default_acl.defaclnamespace = 0
                OR (
                    namespace.nspname <> 'information_schema'
                    AND pg_catalog.left(namespace.nspname, 3) <> 'pg_'
                )
            )
            AND privilege.grantee <> common_owner
            AND privilege.privilege_type = 'EXECUTE'
    ) THEN
        RAISE EXCEPTION 'user routine execution defaults are not sealed'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO function_identity_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_product_installation_authority_reader_database_identity_v1',
            'starring_product_authorized_snapshot_reader_database_identity_v1',
            'starring_product_authorized_snapshot_key_coverage_v1'
        );
    IF function_identity_count <> 3 THEN
        RAISE EXCEPTION 'authority and snapshot readiness function identity is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            ('public.starring_product_installation_authority_reader_database_identity_v1()', ''::TEXT, 'text'::TEXT, FALSE, 0::REAL),
            ('public.starring_product_authorized_snapshot_reader_database_identity_v1()', ''::TEXT, 'text'::TEXT, FALSE, 0::REAL),
            ('public.starring_product_authorized_snapshot_key_coverage_v1(text[])', 'configured_encryption_key_ids text[]'::TEXT, 'TABLE(covered boolean)'::TEXT, TRUE, 1::REAL)
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
        RAISE EXCEPTION 'authority and snapshot readiness function contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*), pg_catalog.bool_and(result.covered)
    INTO coverage_rows, coverage_value
    FROM public.starring_product_authorized_snapshot_key_coverage_v1(
        ARRAY(
            SELECT available.key_id
            FROM (
                SELECT DISTINCT generation.encryption_key_id AS key_id
                FROM public.authoring_session_generations AS generation
                UNION ALL
                SELECT 'migration-probe-v1'::TEXT
                WHERE NOT EXISTS (
                    SELECT 1
                    FROM public.authoring_session_generations AS generation
                )
            ) AS available
            ORDER BY available.key_id
        )
    ) AS result;
    IF coverage_rows <> 1
        OR coverage_value IS DISTINCT FROM TRUE
    THEN
        RAISE EXCEPTION 'authorized snapshot key coverage probe is invalid'
            USING ERRCODE = '55000';
    END IF;
END;
$postflight$;
