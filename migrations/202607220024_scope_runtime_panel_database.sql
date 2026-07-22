SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

LOCK TABLE
    public.product_control_plane_identity,
    public.runtime_panel_reconciliation_sessions,
    public.ruleset_panel_installations,
    public.strict_panel_operation_journal
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
    collision_count BIGINT;
    unsafe_schema_create_count BIGINT;
    unsafe_default_count BIGINT;
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
            (pg_catalog.to_regclass('public.runtime_panel_reconciliation_sessions')),
            (pg_catalog.to_regclass('public.ruleset_panel_installations')),
            (pg_catalog.to_regclass('public.strict_panel_operation_journal'))
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
        RAISE EXCEPTION 'runtime panel database relations are invalid'
            USING ERRCODE = '55000';
    END IF;

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR NOT pg_catalog.has_schema_privilege(common_owner_name, 'public', 'CREATE')
    THEN
        RAISE EXCEPTION 'runtime panel database migration requires the common owner'
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
        RAISE EXCEPTION 'runtime panel database identity is invalid'
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

    SELECT pg_catalog.count(*)
    INTO unsafe_default_count
    FROM pg_catalog.pg_default_acl AS defaults
    CROSS JOIN LATERAL pg_catalog.aclexplode(defaults.defaclacl) AS privilege
    WHERE defaults.defaclnamespace IN (0, pg_catalog.to_regnamespace('public'))
        AND defaults.defaclrole = common_owner
        AND privilege.grantee <> defaults.defaclrole;

    IF unsafe_schema_create_count <> 0 OR unsafe_default_count <> 0 THEN
        RAISE EXCEPTION 'runtime panel database schema trust is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_runtime_panel_database_readiness_v1',
            'starring_runtime_panel_database_identity_v1'
        );

    IF collision_count <> 0 THEN
        RAISE EXCEPTION 'runtime panel database function already exists'
            USING ERRCODE = '55000';
    END IF;
END;
$preflight$;

CREATE FUNCTION public.starring_runtime_panel_database_identity_v1()
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
        AND identity.database_identity IS NOT NULL
        AND identity.database_identity
            <> '00000000-0000-0000-0000-000000000000'::UUID
        AND identity.created_at IS NOT NULL;
$function$;

CREATE FUNCTION public.starring_runtime_panel_database_readiness_v1()
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
    invalid_capability_count BIGINT;
    unsafe_schema_create_count BIGINT;
    unsafe_default_count BIGINT;
    identity_count BIGINT;
    function_set_count BIGINT;
    role_found BOOLEAN;
    role_row RECORD;
BEGIN
    IF pg_catalog.current_setting('role') <> 'none' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_database_role_drift';
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
    WHERE relation.oid = pg_catalog.to_regclass('public.product_control_plane_identity');

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
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_database_role_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_relation_count
    FROM (
        VALUES
            ('public.product_control_plane_identity'),
            ('public.runtime_panel_reconciliation_sessions'),
            ('public.ruleset_panel_installations'),
            ('public.strict_panel_operation_journal')
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
        );

    IF invalid_relation_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_database_schema_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            ('public.starring_runtime_panel_database_identity_v1()', 'text', FALSE, 0::REAL),
            ('public.starring_runtime_panel_database_readiness_v1()', 'TABLE(database_identity text, database_name text, executor_role text, checked_at timestamp with time zone)', TRUE, 1::REAL),
            ('public.starring_runtime_panel_execution_lock_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint)', 'TABLE(checked_at timestamp with time zone, controller_lease_expires_at timestamp with time zone)', TRUE, 1::REAL),
            ('public.starring_runtime_panel_reconciliation_lock_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint)', 'TABLE(checked_at timestamp with time zone, controller_lease_expires_at timestamp with time zone)', TRUE, 1::REAL),
            ('public.starring_runtime_panel_reconciliation_claim_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text)', 'TABLE(session_record_revision bigint, checked_at timestamp with time zone, controller_lease_expires_at timestamp with time zone)', TRUE, 1::REAL),
            ('public.starring_runtime_panel_reconciliation_check_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint)', 'TABLE(checked_at timestamp with time zone, controller_lease_expires_at timestamp with time zone)', TRUE, 1::REAL),
            ('public.starring_runtime_panel_reconciliation_snapshot_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint)', 'TABLE(record_kind text, record_revision bigint, record_format_version smallint, guild_id text, ruleset_key text, panel_key text, installed_version bigint, channel_id text, message_id text, spec_hash text, state_tag text, operation_payload jsonb)', TRUE, 512::REAL),
            ('public.starring_runtime_panel_reconciliation_installation_upsert_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,text,bigint,text,text,text,bigint)', 'bigint', FALSE, 0::REAL),
            ('public.starring_runtime_panel_reconciliation_installation_remove_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,text)', 'bigint', FALSE, 0::REAL),
            ('public.starring_runtime_panel_reconciliation_journal_put_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,smallint,text,text,jsonb)', 'bigint', FALSE, 0::REAL),
            ('public.starring_runtime_panel_reconciliation_journal_remove_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,text)', 'bigint', FALSE, 0::REAL)
    ) AS expected(identity, result_name, returns_set, rows_estimate)
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
        OR function_row.proconfig IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM CASE
            WHEN expected.identity
                = 'public.starring_runtime_panel_database_identity_v1()'
            THEN 'sql'
            ELSE 'plpgsql'
        END
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM expected.result_name;

    SELECT pg_catalog.count(*)
    INTO function_set_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_runtime_panel_database_identity_v1',
            'starring_runtime_panel_database_readiness_v1',
            'starring_runtime_panel_execution_lock_v1',
            'starring_runtime_panel_reconciliation_lock_v1',
            'starring_runtime_panel_reconciliation_claim_v1',
            'starring_runtime_panel_reconciliation_check_v1',
            'starring_runtime_panel_reconciliation_snapshot_v1',
            'starring_runtime_panel_reconciliation_installation_upsert_v1',
            'starring_runtime_panel_reconciliation_installation_remove_v1',
            'starring_runtime_panel_reconciliation_journal_put_v1',
            'starring_runtime_panel_reconciliation_journal_remove_v1'
        );

    IF invalid_function_count <> 0 OR function_set_count <> 11 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_database_schema_drift';
    END IF;

    IF pg_catalog.current_setting('server_version_num')::INTEGER / 10000 <> 16
        OR ARRAY(
            SELECT relation.relname::TEXT
            FROM pg_catalog.pg_class AS relation
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = relation.relnamespace
            WHERE namespace.nspname = 'information_schema'
                AND relation.relkind IN ('r', 'p', 'v', 'm', 'f')
                AND EXISTS (
                    SELECT 1
                    FROM pg_catalog.aclexplode(COALESCE(
                        relation.relacl,
                        pg_catalog.acldefault('r', relation.relowner)
                    )) AS privilege
                    WHERE privilege.grantee = 0
                        AND privilege.privilege_type = 'SELECT'
                )
            ORDER BY relation.relname
        ) IS DISTINCT FROM ARRAY[
                'administrable_role_authorizations',
                'applicable_roles',
                'attributes',
                'character_sets',
                'check_constraint_routine_usage',
                'check_constraints',
                'collation_character_set_applicability',
                'collations',
                'column_column_usage',
                'column_domain_usage',
                'column_options',
                'column_privileges',
                'column_udt_usage',
                'columns',
                'constraint_column_usage',
                'constraint_table_usage',
                'data_type_privileges',
                'domain_constraints',
                'domain_udt_usage',
                'domains',
                'element_types',
                'enabled_roles',
                'foreign_data_wrapper_options',
                'foreign_data_wrappers',
                'foreign_server_options',
                'foreign_servers',
                'foreign_table_options',
                'foreign_tables',
                'information_schema_catalog_name',
                'key_column_usage',
                'parameters',
                'referential_constraints',
                'role_column_grants',
                'role_routine_grants',
                'role_table_grants',
                'role_udt_grants',
                'role_usage_grants',
                'routine_column_usage',
                'routine_privileges',
                'routine_routine_usage',
                'routine_sequence_usage',
                'routine_table_usage',
                'routines',
                'schemata',
                'sequences',
                'sql_features',
                'sql_implementation_info',
                'sql_sizing',
                'table_constraints',
                'table_privileges',
                'tables',
                'triggered_update_columns',
                'triggers',
                'udt_privileges',
                'usage_privileges',
                'user_defined_types',
                'user_mapping_options',
                'user_mappings',
                'view_column_usage',
                'view_routine_usage',
                'view_table_usage',
                'views'
            ]::TEXT[]
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_database_capability_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_capability_count
    FROM (
        VALUES
            ('public.starring_runtime_panel_database_identity_v1()'),
            ('public.starring_runtime_panel_database_readiness_v1()'),
            ('public.starring_runtime_panel_reconciliation_claim_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text)'),
            ('public.starring_runtime_panel_reconciliation_check_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint)'),
            ('public.starring_runtime_panel_reconciliation_snapshot_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint)'),
            ('public.starring_runtime_panel_reconciliation_installation_upsert_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,text,bigint,text,text,text,bigint)'),
            ('public.starring_runtime_panel_reconciliation_installation_remove_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,text)'),
            ('public.starring_runtime_panel_reconciliation_journal_put_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,smallint,text,text,jsonb)'),
            ('public.starring_runtime_panel_reconciliation_journal_remove_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,text)')
    ) AS allowed(identity)
    WHERE NOT pg_catalog.has_function_privilege(
        invoker_oid,
        pg_catalog.to_regprocedure(allowed.identity),
        'EXECUTE'
    );

    IF invalid_capability_count <> 0
        OR pg_catalog.has_function_privilege(
            invoker_oid,
            pg_catalog.to_regprocedure('public.starring_runtime_panel_execution_lock_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint)'),
            'EXECUTE'
        )
        OR pg_catalog.has_function_privilege(
            invoker_oid,
            pg_catalog.to_regprocedure('public.starring_runtime_panel_reconciliation_lock_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint)'),
            'EXECUTE'
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_proc AS function_row
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = function_row.pronamespace
            WHERE namespace.nspname <> 'information_schema'
                AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_'
                AND pg_catalog.has_function_privilege(invoker_oid, function_row.oid, 'EXECUTE')
                AND function_row.oid NOT IN (
                    pg_catalog.to_regprocedure('public.starring_runtime_panel_database_identity_v1()'),
                    pg_catalog.to_regprocedure('public.starring_runtime_panel_database_readiness_v1()'),
                    pg_catalog.to_regprocedure('public.starring_runtime_panel_reconciliation_claim_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text)'),
                    pg_catalog.to_regprocedure('public.starring_runtime_panel_reconciliation_check_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint)'),
                    pg_catalog.to_regprocedure('public.starring_runtime_panel_reconciliation_snapshot_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint)'),
                    pg_catalog.to_regprocedure('public.starring_runtime_panel_reconciliation_installation_upsert_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,text,bigint,text,text,text,bigint)'),
                    pg_catalog.to_regprocedure('public.starring_runtime_panel_reconciliation_installation_remove_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,text)'),
                    pg_catalog.to_regprocedure('public.starring_runtime_panel_reconciliation_journal_put_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,smallint,text,text,jsonb)'),
                    pg_catalog.to_regprocedure('public.starring_runtime_panel_reconciliation_journal_remove_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,text)')
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
            WHERE namespace.nspname = 'public'
                AND function_row.proname IN (
                    'starring_runtime_panel_database_identity_v1',
                    'starring_runtime_panel_database_readiness_v1',
                    'starring_runtime_panel_execution_lock_v1',
                    'starring_runtime_panel_reconciliation_lock_v1',
                    'starring_runtime_panel_reconciliation_claim_v1',
                    'starring_runtime_panel_reconciliation_check_v1',
                    'starring_runtime_panel_reconciliation_snapshot_v1',
                    'starring_runtime_panel_reconciliation_installation_upsert_v1',
                    'starring_runtime_panel_reconciliation_installation_remove_v1',
                    'starring_runtime_panel_reconciliation_journal_put_v1',
                    'starring_runtime_panel_reconciliation_journal_remove_v1'
                )
                AND (
                    privilege.grantee = 0
                    OR privilege.grantee NOT IN (common_owner, invoker_oid)
                    OR (
                        privilege.grantee = invoker_oid
                        AND (
                            privilege.grantor <> common_owner
                            OR privilege.is_grantable
                        )
                    )
                    OR (
                        privilege.grantee = common_owner
                        AND privilege.grantor <> common_owner
                    )
                )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_database_capability_drift';
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

    SELECT pg_catalog.count(*)
    INTO unsafe_default_count
    FROM pg_catalog.pg_default_acl AS defaults
    LEFT JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = defaults.defaclnamespace
    CROSS JOIN LATERAL pg_catalog.aclexplode(defaults.defaclacl) AS privilege
    WHERE (
            defaults.defaclnamespace = 0
            OR (
                namespace.nspname <> 'information_schema'
                AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_'
            )
        )
        AND privilege.grantee <> defaults.defaclrole;

    IF unsafe_schema_create_count <> 0
        OR unsafe_default_count <> 0
        OR NOT pg_catalog.has_database_privilege(
            invoker_oid,
            pg_catalog.current_database(),
            'CONNECT'
        )
        OR pg_catalog.has_database_privilege(
            invoker_oid,
            pg_catalog.current_database(),
            'CREATE'
        )
        OR pg_catalog.has_database_privilege(
            invoker_oid,
            pg_catalog.current_database(),
            'TEMPORARY'
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
        OR NOT pg_catalog.has_schema_privilege(invoker_oid, 'public', 'USAGE')
        OR pg_catalog.has_schema_privilege(invoker_oid, 'public', 'CREATE')
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_namespace AS namespace
            WHERE namespace.nspname <> 'public'
                AND namespace.nspname <> 'information_schema'
                AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_'
                AND (
                    pg_catalog.has_schema_privilege(invoker_oid, namespace.oid, 'USAGE')
                    OR pg_catalog.has_schema_privilege(invoker_oid, namespace.oid, 'CREATE')
                )
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
            WHERE namespace.nspname <> 'information_schema'
                AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_'
                AND relation.relkind IN ('r', 'p', 'v', 'm', 'f')
                AND (
                    pg_catalog.has_table_privilege(invoker_oid, relation.oid, 'SELECT')
                    OR pg_catalog.has_table_privilege(invoker_oid, relation.oid, 'INSERT')
                    OR pg_catalog.has_table_privilege(invoker_oid, relation.oid, 'UPDATE')
                    OR pg_catalog.has_table_privilege(invoker_oid, relation.oid, 'DELETE')
                    OR pg_catalog.has_table_privilege(invoker_oid, relation.oid, 'TRUNCATE')
                    OR pg_catalog.has_table_privilege(invoker_oid, relation.oid, 'REFERENCES')
                    OR pg_catalog.has_table_privilege(invoker_oid, relation.oid, 'TRIGGER')
                    OR pg_catalog.has_any_column_privilege(invoker_oid, relation.oid, 'SELECT')
                    OR pg_catalog.has_any_column_privilege(invoker_oid, relation.oid, 'INSERT')
                    OR pg_catalog.has_any_column_privilege(invoker_oid, relation.oid, 'UPDATE')
                    OR pg_catalog.has_any_column_privilege(invoker_oid, relation.oid, 'REFERENCES')
                )
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_class AS sequence
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = sequence.relnamespace
            WHERE namespace.nspname <> 'information_schema'
                AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_'
                AND sequence.relkind = 'S'
                AND (
                    pg_catalog.has_sequence_privilege(invoker_oid, sequence.oid, 'USAGE')
                    OR pg_catalog.has_sequence_privilege(invoker_oid, sequence.oid, 'SELECT')
                    OR pg_catalog.has_sequence_privilege(invoker_oid, sequence.oid, 'UPDATE')
                )
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_parameter_acl AS parameter_acl
            CROSS JOIN LATERAL pg_catalog.aclexplode(parameter_acl.paracl) AS privilege
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
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_database_capability_drift';
    END IF;

    SELECT pg_catalog.count(*),
        pg_catalog.min(identity.database_identity::TEXT)
    INTO identity_count, database_identity
    FROM public.product_control_plane_identity AS identity
    WHERE identity.singleton
        AND identity.database_identity IS NOT NULL
        AND identity.database_identity
            <> '00000000-0000-0000-0000-000000000000'::UUID
        AND identity.created_at IS NOT NULL;

    IF identity_count <> 1 OR database_identity IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RP004',
            MESSAGE = 'runtime_panel_database_identity_drift';
    END IF;

    database_name := pg_catalog.current_database()::TEXT;
    executor_role := session_user::TEXT;
    checked_at := pg_catalog.clock_timestamp();
    RETURN NEXT;
END;
$function$;

DO $privileges$
DECLARE
    function_identity TEXT;
    function_oid OID;
    common_owner OID;
    common_owner_name NAME;
    grantee OID;
    grantee_name NAME;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.product_control_plane_identity');
    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL THEN
        RAISE EXCEPTION 'runtime panel database function owner is unavailable'
            USING ERRCODE = '55000';
    END IF;
    FOREACH function_identity IN ARRAY ARRAY[
        'public.starring_runtime_panel_database_identity_v1()',
        'public.starring_runtime_panel_database_readiness_v1()'
    ]::TEXT[]
    LOOP
        function_oid := pg_catalog.to_regprocedure(function_identity);
        IF function_oid IS NULL THEN
            RAISE EXCEPTION 'runtime panel database function is unavailable'
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
                RAISE EXCEPTION 'runtime panel database function grantee is unavailable'
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
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.product_control_plane_identity');
    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            ('public.starring_runtime_panel_database_identity_v1()', 'text', FALSE, 0::REAL, 'sql'),
            ('public.starring_runtime_panel_database_readiness_v1()', 'TABLE(database_identity text, database_name text, executor_role text, checked_at timestamp with time zone)', TRUE, 1::REAL, 'plpgsql')
    ) AS expected(identity, result_name, returns_set, rows_estimate, language_name)
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
        OR function_row.proconfig IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM expected.language_name
        OR pg_catalog.pg_get_function_identity_arguments(function_row.oid) <> ''
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
        RAISE EXCEPTION 'runtime panel database function contract is invalid'
            USING ERRCODE = '55000';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
