SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

LOCK TABLE public.automation_instances IN ACCESS EXCLUSIVE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    collision_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.automation_instances')
        AND relation.relkind = 'r'
        AND relation.relpersistence = 'p'
        AND NOT relation.relrowsecurity
        AND NOT relation.relforcerowsecurity;

    IF NOT FOUND
        OR common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_database_readiness_v1()'
        ) IS NULL
        OR pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_route_read_v1(text,text)'
        ) IS NULL
    THEN
        RAISE EXCEPTION 'runtime interaction teardown migration preflight failed'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_runtime_interaction_instance_get_for_teardown_v1',
            'starring_runtime_interaction_instance_claim_deleting_v1',
            'starring_runtime_interaction_instance_mark_deleted_v1',
            'starring_runtime_interaction_instance_list_retryable_v1'
        );

    IF collision_count <> 0 THEN
        RAISE EXCEPTION 'runtime interaction teardown function collision exists'
            USING ERRCODE = '55000';
    END IF;
END;
$preflight$;

CREATE FUNCTION public.starring_runtime_interaction_instance_get_for_teardown_v1(
    expected_guild_id TEXT,
    expected_instance_id TEXT
)
RETURNS TABLE(
    guild_id TEXT,
    instance_id TEXT,
    ruleset_key TEXT,
    ruleset_version BIGINT,
    kind TEXT,
    created_by TEXT,
    status TEXT,
    resources JSONB
)
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
BEGIN
    RETURN QUERY
    SELECT route.guild_id,
        route.instance_id,
        route.ruleset_key,
        route.ruleset_version,
        route.kind,
        route.created_by,
        route.status,
        route.resources
    FROM public.starring_runtime_interaction_route_read_v1(
        expected_guild_id,
        expected_instance_id
    ) AS route;
END;
$function$;

CREATE OR REPLACE FUNCTION public.starring_runtime_interaction_database_readiness_v1()
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
    identity_count BIGINT;
    invalid_attribute_count BIGINT;
    invalid_function_count BIGINT;
    invalid_relation_count BIGINT;
    invalid_support_function_count BIGINT;
    invalid_trigger_count BIGINT;
    role_found BOOLEAN;
    role_row RECORD;
    unexpected_capability_count BIGINT;
    unsafe_default_count BIGINT;
    unsafe_schema_count BIGINT;
BEGIN
    IF pg_catalog.current_setting('role') <> 'none' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_database_role_drift';
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
        role.rolconfig
    INTO role_row
    FROM pg_catalog.pg_roles AS role
    WHERE role.oid = invoker_oid;
    role_found := FOUND;

    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.automation_instances');

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
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_database_role_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_relation_count
    FROM (
        VALUES
            ('public.product_control_plane_identity'),
            ('public.automation_instances'),
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

    IF invalid_relation_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_database_schema_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_attribute_count
    FROM (
        VALUES
            ('public.product_control_plane_identity', 'singleton', 'boolean', TRUE, ''),
            ('public.product_control_plane_identity', 'database_identity', 'uuid', TRUE, ''),
            ('public.product_control_plane_identity', 'created_at', 'timestamp with time zone', TRUE, ''),
            ('public.automation_instances', 'guild_id', 'text', TRUE, ''),
            ('public.automation_instances', 'instance_id', 'text', TRUE, ''),
            ('public.automation_instances', 'ruleset_key', 'text', TRUE, ''),
            ('public.automation_instances', 'kind', 'text', TRUE, ''),
            ('public.automation_instances', 'created_by', 'text', TRUE, ''),
            ('public.automation_instances', 'status', 'text', TRUE, ''),
            ('public.automation_instances', 'resources', 'jsonb', TRUE, ''),
            ('public.automation_instances', 'ruleset_version', 'bigint', TRUE, ''),
            ('public.automation_ruleset_versions', 'guild_id', 'text', TRUE, ''),
            ('public.automation_ruleset_versions', 'ruleset_key', 'text', TRUE, ''),
            ('public.automation_ruleset_versions', 'version', 'bigint', TRUE, ''),
            ('public.automation_ruleset_versions', 'schema_version', 'bigint', TRUE, ''),
            ('public.automation_ruleset_versions', 'definition', 'jsonb', TRUE, ''),
            ('public.automation_ruleset_versions', 'content_hash', 'text', TRUE, ''),
            ('public.automation_ruleset_versions', 'created_by', 'text', TRUE, ''),
            ('public.automation_ruleset_versions', 'canonical_content_hash', 'text', FALSE, 's')
    ) AS expected(relation_identity, attribute_name, type_name, is_not_null, generated_kind)
    LEFT JOIN pg_catalog.pg_attribute AS attribute
        ON attribute.attrelid = pg_catalog.to_regclass(expected.relation_identity)
        AND attribute.attname = expected.attribute_name
        AND attribute.attnum > 0
        AND NOT attribute.attisdropped
    WHERE attribute.attnum IS NULL
        OR pg_catalog.format_type(attribute.atttypid, attribute.atttypmod)
            IS DISTINCT FROM expected.type_name
        OR attribute.attnotnull IS DISTINCT FROM expected.is_not_null
        OR attribute.attgenerated IS DISTINCT FROM expected.generated_kind;

    IF invalid_attribute_count <> 0
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_attribute AS attribute
            WHERE attribute.attrelid IN (
                    pg_catalog.to_regclass('public.product_control_plane_identity'),
                    pg_catalog.to_regclass('public.automation_instances'),
                    pg_catalog.to_regclass('public.automation_ruleset_versions')
                )
                AND attribute.attnum > 0
                AND NOT attribute.attisdropped
        ) <> 19
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_database_attribute_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_runtime_interaction_database_identity_v1()',
                '',
                'text',
                FALSE,
                0::REAL,
                'sql'
            ),
            (
                'public.starring_runtime_interaction_database_readiness_v1()',
                '',
                'TABLE(database_identity text, database_name text, executor_role text, checked_at timestamp with time zone)',
                TRUE,
                1::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_route_read_v1(text,text)',
                'expected_guild_id text, expected_instance_id text',
                'TABLE(guild_id text, instance_id text, ruleset_key text, ruleset_version bigint, kind text, created_by text, status text, resources jsonb)',
                TRUE,
                1::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_pinned_read_v1(text,text)',
                'expected_guild_id text, expected_instance_id text',
                'TABLE(guild_id text, instance_id text, ruleset_key text, ruleset_version bigint, kind text, created_by text, status text, resources jsonb, artifact_found boolean, artifact_schema_version bigint, artifact_definition jsonb, artifact_content_hash text, artifact_created_by text)',
                TRUE,
                1::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_instance_register_v1(text,text,text,bigint,text,text,jsonb)',
                'expected_guild_id text, expected_instance_id text, expected_ruleset_key text, expected_ruleset_version bigint, expected_kind text, expected_created_by text, expected_resources jsonb',
                'text',
                FALSE,
                0::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_instance_get_for_teardown_v1(text,text)',
                'expected_guild_id text, expected_instance_id text',
                'TABLE(guild_id text, instance_id text, ruleset_key text, ruleset_version bigint, kind text, created_by text, status text, resources jsonb)',
                TRUE,
                1::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_instance_claim_deleting_v1(text,text)',
                'expected_guild_id text, expected_instance_id text',
                'text',
                FALSE,
                0::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_instance_mark_deleted_v1(text,text)',
                'expected_guild_id text, expected_instance_id text',
                'text',
                FALSE,
                0::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_instance_list_retryable_v1(text,bigint)',
                'expected_guild_id text, expected_limit bigint',
                'TABLE(guild_id text, instance_id text, ruleset_key text, ruleset_version bigint, kind text, created_by text, status text, resources jsonb)',
                TRUE,
                256::REAL,
                'plpgsql'
            )
    ) AS expected(identity, arguments, result, returns_set, result_rows, language_name)
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
        OR function_row.prorows IS DISTINCT FROM expected.result_rows
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM expected.language_name
        OR pg_catalog.pg_get_function_arguments(function_row.oid)
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
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_database_function_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_support_function_count
    FROM (
        VALUES
            (
                'public.reject_ruleset_artifact_mutation()',
                '',
                'trigger',
                FALSE
            ),
            (
                'public.guard_runtime_interaction_instance_mutation_v1()',
                '',
                'trigger',
                FALSE
            ),
            (
                'public.starring_runtime_interaction_schema_manifest_v1()',
                '',
                'boolean',
                TRUE
            )
    ) AS expected(identity, arguments, result, is_strict)
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
        OR language_row.lanname IS DISTINCT FROM 'plpgsql'
        OR pg_catalog.pg_get_function_arguments(function_row.oid)
            IS DISTINCT FROM expected.arguments
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM expected.result
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
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_database_immutability_drift';
    END IF;

    IF NOT public.starring_runtime_interaction_schema_manifest_v1() THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_database_constraint_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_trigger_count
    FROM (
        VALUES
            (
                'public.automation_instances',
                'automation_instances_guard_runtime_interaction_mutation',
                'public.guard_runtime_interaction_instance_mutation_v1()',
                27
            ),
            (
                'public.automation_instances',
                'automation_instances_guard_runtime_interaction_truncate',
                'public.guard_runtime_interaction_instance_mutation_v1()',
                34
            ),
            (
                'public.automation_ruleset_versions',
                'automation_ruleset_versions_reject_mutation',
                'public.reject_ruleset_artifact_mutation()',
                26
            ),
            (
                'public.automation_ruleset_versions',
                'automation_ruleset_versions_reject_truncate',
                'public.reject_ruleset_artifact_mutation()',
                34
            )
    ) AS expected(relation_identity, trigger_name, function_identity, trigger_type)
    LEFT JOIN pg_catalog.pg_trigger AS trigger_row
        ON trigger_row.tgrelid = pg_catalog.to_regclass(expected.relation_identity)
        AND trigger_row.tgname = expected.trigger_name
        AND NOT trigger_row.tgisinternal
    WHERE trigger_row.oid IS NULL
        OR trigger_row.tgenabled <> 'O'
        OR trigger_row.tgfoid <> pg_catalog.to_regprocedure(expected.function_identity)
        OR trigger_row.tgtype::INTEGER IS DISTINCT FROM expected.trigger_type
        OR trigger_row.tgnargs <> 0
        OR pg_catalog.octet_length(trigger_row.tgargs) <> 0
        OR pg_catalog.octet_length(trigger_row.tgattr::TEXT) <> 0
        OR trigger_row.tgqual IS NOT NULL
        OR trigger_row.tgconstraint <> 0
        OR trigger_row.tgdeferrable
        OR trigger_row.tginitdeferred
        OR trigger_row.tgoldtable IS NOT NULL
        OR trigger_row.tgnewtable IS NOT NULL;

    IF invalid_trigger_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_database_trigger_drift';
    END IF;

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
                'public.starring_runtime_interaction_database_identity_v1()'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_database_readiness_v1()'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_route_read_v1(text,text)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_pinned_read_v1(text,text)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_instance_register_v1(text,text,text,bigint,text,text,jsonb)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_instance_get_for_teardown_v1(text,text)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_instance_claim_deleting_v1(text,text)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_instance_mark_deleted_v1(text,text)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_instance_list_retryable_v1(text,bigint)'
            )
        )
        AND namespace.nspname NOT IN ('pg_catalog', 'information_schema')
        AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_';

    SELECT pg_catalog.count(*)
    INTO unsafe_schema_count
    FROM pg_catalog.pg_namespace AS namespace
    WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema')
        AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_'
        AND (
            pg_catalog.has_schema_privilege(
                invoker_oid,
                namespace.oid,
                'CREATE'
            )
            OR (
                namespace.nspname <> 'public'
                AND pg_catalog.has_schema_privilege(
                    invoker_oid,
                    namespace.oid,
                    'USAGE'
                )
            )
        );

    SELECT pg_catalog.count(*)
    INTO unsafe_default_count
    FROM pg_catalog.pg_default_acl AS defaults
    CROSS JOIN LATERAL pg_catalog.aclexplode(defaults.defaclacl) AS privilege
    WHERE privilege.grantee IN (0, invoker_oid);

    IF unexpected_capability_count <> 0
        OR unsafe_schema_count <> 0
        OR unsafe_default_count <> 0
        OR NOT pg_catalog.has_database_privilege(
            invoker_oid,
            database_oid,
            'CONNECT'
        )
        OR NOT pg_catalog.has_schema_privilege(
            invoker_oid,
            'public',
            'USAGE'
        )
        OR pg_catalog.has_database_privilege(
            invoker_oid,
            database_oid,
            'CREATE'
        )
        OR pg_catalog.has_database_privilege(
            invoker_oid,
            database_oid,
            'TEMPORARY'
        )
        OR pg_catalog.has_schema_privilege(
            invoker_oid,
            'public',
            'CREATE'
        )
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
        OR EXISTS (
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
        OR EXISTS (
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
        OR EXISTS (
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
        OR EXISTS (
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
        OR EXISTS (
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
        OR EXISTS (
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
        OR EXISTS (
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
        OR EXISTS (
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
        OR EXISTS (
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
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_parameter_acl AS parameter_acl
            CROSS JOIN LATERAL pg_catalog.aclexplode(
                parameter_acl.paracl
            ) AS privilege
            WHERE privilege.grantee IN (0, invoker_oid)
                AND privilege.privilege_type IN ('SET', 'ALTER SYSTEM')
        )
        OR EXISTS (
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
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_database_capability_drift';
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
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_database_identity_drift';
    END IF;

    database_name := pg_catalog.current_database()::TEXT;
    executor_role := session_user::TEXT;
    checked_at := pg_catalog.clock_timestamp();
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_instance_claim_deleting_v1(
    expected_guild_id TEXT,
    expected_instance_id TEXT
)
RETURNS TEXT
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    current_status TEXT;
    updated_count BIGINT;
BEGIN
    IF expected_guild_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_guild_id) > 20
        OR (
            pg_catalog.length(expected_guild_id) = 20
            AND expected_guild_id > '18446744073709551615'
        )
        OR expected_instance_id !~ '^[A-Za-z0-9_-]{1,32}$'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_invalid_teardown_claim_input';
    END IF;

    SELECT instance.status
    INTO current_status
    FROM public.automation_instances AS instance
    WHERE instance.guild_id = expected_guild_id
        AND instance.instance_id = expected_instance_id
    FOR UPDATE;

    IF NOT FOUND THEN
        RETURN 'not_found';
    END IF;

    IF current_status IN ('active', 'disabled') THEN
        UPDATE public.automation_instances AS instance
        SET status = 'deleting'
        WHERE instance.guild_id = expected_guild_id
            AND instance.instance_id = expected_instance_id
            AND instance.status = current_status;
        GET DIAGNOSTICS updated_count = ROW_COUNT;
        IF updated_count <> 1 THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI002',
                MESSAGE = 'runtime_interaction_teardown_claim_state_drift';
        END IF;
        RETURN 'claimed';
    END IF;

    IF current_status = 'deleting' THEN
        RETURN 'already_deleting';
    END IF;

    IF current_status = 'deleted' THEN
        RETURN 'already_deleted';
    END IF;

    RAISE EXCEPTION USING
        ERRCODE = 'RI002',
        MESSAGE = 'runtime_interaction_teardown_claim_status_invalid';
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_instance_mark_deleted_v1(
    expected_guild_id TEXT,
    expected_instance_id TEXT
)
RETURNS TEXT
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    current_status TEXT;
    updated_count BIGINT;
BEGIN
    IF expected_guild_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_guild_id) > 20
        OR (
            pg_catalog.length(expected_guild_id) = 20
            AND expected_guild_id > '18446744073709551615'
        )
        OR expected_instance_id !~ '^[A-Za-z0-9_-]{1,32}$'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_invalid_teardown_mark_input';
    END IF;

    SELECT instance.status
    INTO current_status
    FROM public.automation_instances AS instance
    WHERE instance.guild_id = expected_guild_id
        AND instance.instance_id = expected_instance_id
    FOR UPDATE;

    IF NOT FOUND THEN
        RETURN 'not_found';
    END IF;

    IF current_status = 'deleting' THEN
        UPDATE public.automation_instances AS instance
        SET status = 'deleted'
        WHERE instance.guild_id = expected_guild_id
            AND instance.instance_id = expected_instance_id
            AND instance.status = 'deleting';
        GET DIAGNOSTICS updated_count = ROW_COUNT;
        IF updated_count <> 1 THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI002',
                MESSAGE = 'runtime_interaction_teardown_mark_state_drift';
        END IF;
        RETURN 'marked_deleted';
    END IF;

    IF current_status = 'deleted' THEN
        RETURN 'already_deleted';
    END IF;

    IF current_status IN ('active', 'disabled') THEN
        RETURN 'conflict';
    END IF;

    RAISE EXCEPTION USING
        ERRCODE = 'RI002',
        MESSAGE = 'runtime_interaction_teardown_mark_status_invalid';
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_instance_list_retryable_v1(
    expected_guild_id TEXT,
    expected_limit BIGINT
)
RETURNS TABLE(
    guild_id TEXT,
    instance_id TEXT,
    ruleset_key TEXT,
    ruleset_version BIGINT,
    kind TEXT,
    created_by TEXT,
    status TEXT,
    resources JSONB
)
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 256
AS $function$
BEGIN
    IF expected_guild_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_guild_id) > 20
        OR (
            pg_catalog.length(expected_guild_id) = 20
            AND expected_guild_id > '18446744073709551615'
        )
        OR expected_limit NOT BETWEEN 1 AND 256
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_invalid_teardown_retry_input';
    END IF;

    RETURN QUERY
    SELECT route.guild_id,
        route.instance_id,
        route.ruleset_key,
        route.ruleset_version,
        route.kind,
        route.created_by,
        route.status,
        route.resources
    FROM (
        SELECT instance.instance_id
        FROM public.automation_instances AS instance
        WHERE instance.guild_id = expected_guild_id
            AND instance.status = 'deleting'
        ORDER BY instance.instance_id COLLATE "C"
        LIMIT expected_limit
    ) AS candidate
    CROSS JOIN LATERAL public.starring_runtime_interaction_instance_get_for_teardown_v1(
        expected_guild_id,
        candidate.instance_id
    ) AS route
    WHERE route.status = 'deleting'
    ORDER BY route.instance_id COLLATE "C";
END;
$function$;

DO $privileges$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    function_identity TEXT;
    grantee OID;
    grantee_name NAME;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.automation_instances');
    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);

    IF common_owner_name IS NULL THEN
        RAISE EXCEPTION 'runtime interaction teardown owner is unavailable'
            USING ERRCODE = '55000';
    END IF;

    FOREACH function_identity IN ARRAY ARRAY[
        'public.starring_runtime_interaction_instance_get_for_teardown_v1(TEXT,TEXT)',
        'public.starring_runtime_interaction_instance_claim_deleting_v1(TEXT,TEXT)',
        'public.starring_runtime_interaction_instance_mark_deleted_v1(TEXT,TEXT)',
        'public.starring_runtime_interaction_instance_list_retryable_v1(TEXT,BIGINT)'
    ]::TEXT[]
    LOOP
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
            WHERE function_row.oid = pg_catalog.to_regprocedure(function_identity)
                AND privilege.grantee <> 0
                AND privilege.grantee <> common_owner
        LOOP
            grantee_name := pg_catalog.pg_get_userbyid(grantee);
            IF grantee_name IS NULL THEN
                RAISE EXCEPTION 'runtime interaction teardown grantee is unavailable'
                    USING ERRCODE = '55000';
            END IF;
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM %I CASCADE',
                function_identity,
                grantee_name
            );
        END LOOP;
    END LOOP;
END;
$privileges$;

DO $postflight$
DECLARE
    common_owner OID;
    invalid_function_count BIGINT;
    invalid_relation_acl_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.automation_instances');

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_runtime_interaction_instance_get_for_teardown_v1(text,text)',
                'expected_guild_id text, expected_instance_id text',
                'TABLE(guild_id text, instance_id text, ruleset_key text, ruleset_version bigint, kind text, created_by text, status text, resources jsonb)',
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_interaction_instance_claim_deleting_v1(text,text)',
                'expected_guild_id text, expected_instance_id text',
                'text',
                FALSE,
                0::REAL
            ),
            (
                'public.starring_runtime_interaction_instance_mark_deleted_v1(text,text)',
                'expected_guild_id text, expected_instance_id text',
                'text',
                FALSE,
                0::REAL
            ),
            (
                'public.starring_runtime_interaction_instance_list_retryable_v1(text,bigint)',
                'expected_guild_id text, expected_limit bigint',
                'TABLE(guild_id text, instance_id text, ruleset_key text, ruleset_version bigint, kind text, created_by text, status text, resources jsonb)',
                TRUE,
                256::REAL
            )
    ) AS expected(identity, arguments, result, returns_set, result_rows)
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
        OR function_row.prorows IS DISTINCT FROM expected.result_rows
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM 'plpgsql'
        OR pg_catalog.pg_get_function_arguments(function_row.oid)
            IS DISTINCT FROM expected.arguments
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM expected.result
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
                OR privilege.grantor <> common_owner
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
        );

    SELECT pg_catalog.count(*)
    INTO invalid_relation_acl_count
    FROM (
        VALUES
            ('public.product_control_plane_identity'),
            ('public.automation_instances'),
            ('public.automation_ruleset_versions')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = pg_catalog.to_regclass(expected.identity)
    WHERE relation.oid IS NULL
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

    IF invalid_function_count <> 0
        OR invalid_relation_acl_count <> 0
        OR pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_database_readiness_v1()'
        )) NOT LIKE '%starring_runtime_interaction_instance_get_for_teardown_v1%'
        OR pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_database_readiness_v1()'
        )) NOT LIKE '%starring_runtime_interaction_instance_claim_deleting_v1%'
        OR pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_database_readiness_v1()'
        )) NOT LIKE '%starring_runtime_interaction_instance_mark_deleted_v1%'
        OR pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_database_readiness_v1()'
        )) NOT LIKE '%starring_runtime_interaction_instance_list_retryable_v1%'
    THEN
        RAISE EXCEPTION 'runtime interaction teardown migration postflight failed'
            USING ERRCODE = '55000';
    END IF;
END;
$postflight$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
