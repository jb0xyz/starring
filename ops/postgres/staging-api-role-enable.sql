BEGIN;

SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';
SET LOCAL idle_in_transaction_session_timeout = '60s';
SET LOCAL search_path = pg_catalog;

DO $guard$
DECLARE
    expected_database TEXT;
    expected_system_identifier TEXT;
    actual_system_identifier TEXT;
BEGIN
    expected_database := pg_catalog.current_setting(
        'starring.expected_staging_database',
        TRUE
    );
    IF expected_database IS DISTINCT FROM pg_catalog.current_database()
        OR pg_catalog.current_database()
            !~ '^starring(_[a-z0-9]+)*_staging(_[a-z0-9]+)*$'
    THEN
        RAISE EXCEPTION 'staging database acknowledgement is invalid'
            USING ERRCODE = '55000';
    END IF;

    expected_system_identifier := pg_catalog.current_setting(
        'starring.expected_staging_system_identifier',
        TRUE
    );
    SELECT system_identifier::TEXT
    INTO actual_system_identifier
    FROM pg_catalog.pg_control_system();
    IF expected_system_identifier IS DISTINCT FROM actual_system_identifier THEN
        RAISE EXCEPTION 'staging cluster acknowledgement is invalid'
            USING ERRCODE = '55000';
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_roles AS role
        WHERE role.rolname = current_user
            AND role.rolsuper
    ) THEN
        RAISE EXCEPTION 'staging role enable requires a cluster administrator'
            USING ERRCODE = '42501';
    END IF;
END;
$guard$;

CREATE TEMP TABLE starring_api_request_roles (
    role_name NAME PRIMARY KEY
) ON COMMIT DROP;

INSERT INTO pg_temp.starring_api_request_roles (role_name)
VALUES
    ('starring_identity_oauth'),
    ('starring_identity_issuer'),
    ('starring_identity_session'),
    ('starring_identity_security'),
    ('starring_installation_authority_reader'),
    ('starring_authorized_snapshot_reader'),
    ('starring_promotion_executor'),
    ('starring_decision_reader'),
    ('starring_decision_approval'),
    ('starring_decision_rejection'),
    ('starring_decision_apply'),
    ('starring_deployment_status_reader'),
    ('starring_operational_deployment_status_reader');

CREATE TEMP TABLE starring_api_capability_manifest (
    role_name NAME NOT NULL,
    function_identity TEXT NOT NULL,
    PRIMARY KEY (role_name, function_identity),
    FOREIGN KEY (role_name)
        REFERENCES pg_temp.starring_api_request_roles (role_name)
) ON COMMIT DROP;

INSERT INTO pg_temp.starring_api_capability_manifest (
    role_name,
    function_identity
)
VALUES
    ('starring_identity_oauth', 'public.starring_product_oauth_database_identity_v1()'),
    ('starring_identity_oauth', 'public.starring_product_oauth_flow_create_v1(bytea,bytea,text,text,double precision)'),
    ('starring_identity_oauth', 'public.starring_product_oauth_flow_consume_v1(bytea,bytea,text,text[])'),
    ('starring_identity_issuer', 'public.starring_product_session_issuer_database_identity_v1()'),
    ('starring_identity_issuer', 'public.starring_product_session_issue_v1(bytea,text,text,timestamp with time zone,text,text,bytea,bytea,double precision,double precision)'),
    ('starring_identity_session', 'public.starring_product_session_api_database_identity_v1()'),
    ('starring_identity_session', 'public.starring_product_session_read_v1(bytea)'),
    ('starring_identity_session', 'public.starring_product_session_mutation_read_v1(bytea)'),
    ('starring_identity_session', 'public.starring_product_session_touch_v1(bytea,timestamp with time zone,timestamp with time zone,timestamp with time zone,double precision)'),
    ('starring_identity_session', 'public.starring_product_session_logout_read_v1(bytea)'),
    ('starring_identity_session', 'public.starring_product_session_logout_commit_v1(bytea,bytea,timestamp with time zone)'),
    ('starring_identity_security', 'public.starring_product_security_revoker_database_identity_v1()'),
    ('starring_identity_security', 'public.starring_product_session_security_revoke_v1(bytea)'),
    ('starring_installation_authority_reader', 'public.starring_product_installation_authority_reader_database_identity_v1()'),
    ('starring_installation_authority_reader', 'public.starring_product_installation_authority_read_v1(text,text,bytea)'),
    ('starring_authorized_snapshot_reader', 'public.starring_product_authorized_snapshot_reader_database_identity_v1()'),
    ('starring_authorized_snapshot_reader', 'public.starring_product_authorized_snapshot_read_v1(text,text,bytea,text,text)'),
    ('starring_authorized_snapshot_reader', 'public.starring_product_authorized_snapshot_key_coverage_v1(text[])'),
    ('starring_promotion_executor', 'public.starring_product_promotion_executor_database_identity_v1()'),
    ('starring_promotion_executor', 'public.starring_product_promotion_replay_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,bigint,text,text[],text[],text[])'),
    ('starring_promotion_executor', 'public.starring_product_promotion_prepare_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bytea,text,bigint,bigint,text,text,text,text,jsonb,jsonb,text,text,text[],text[],text[],text,text,text,text)'),
    ('starring_promotion_executor', 'public.starring_product_promotion_publish_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text)'),
    ('starring_promotion_executor', 'public.starring_product_promotion_approval_environment_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text)'),
    ('starring_promotion_executor', 'public.starring_product_promotion_activation_link_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text,jsonb)'),
    ('starring_promotion_executor', 'public.starring_product_promotion_repair_link_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text,bytea,jsonb,text,text,text[],text[],text[],text,text,text,text)'),
    ('starring_promotion_executor', 'public.starring_product_promotion_keyring_coverage_v1(text[],text[])'),
    ('starring_decision_reader', 'public.starring_product_decision_reader_database_identity_v1()'),
    ('starring_decision_reader', 'public.starring_product_decision_read_v1(text,text,text,text,text,text,bytea)'),
    ('starring_decision_approval', 'public.starring_product_approval_executor_database_identity_v1()'),
    ('starring_decision_approval', 'public.starring_product_approve_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text)'),
    ('starring_decision_approval', 'public.starring_product_approval_keyring_coverage_v1(text[],text[])'),
    ('starring_decision_rejection', 'public.starring_product_rejection_executor_database_identity_v1()'),
    ('starring_decision_rejection', 'public.starring_product_rejection_keyring_coverage_v1(text[],text[])'),
    ('starring_decision_rejection', 'public.starring_product_reject_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text)'),
    ('starring_decision_apply', 'public.starring_product_apply_executor_database_identity_v1()'),
    ('starring_decision_apply', 'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'),
    ('starring_decision_apply', 'public.starring_product_apply_target_artifact_v1(text,text,text,text,bytea,text,text)'),
    ('starring_decision_apply', 'public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)'),
    ('starring_decision_apply', 'public.starring_product_apply_keyring_coverage_v1(text[],text[])'),
    ('starring_deployment_status_reader', 'public.starring_product_deployment_status_reader_database_identity_v1()'),
    ('starring_deployment_status_reader', 'public.starring_product_deployment_status_read_v1(text,text,text,text,text,text,text,text,bytea)'),
    ('starring_operational_deployment_status_reader', 'public.starring_product_deployment_status_reader_database_identity_v2()'),
    ('starring_operational_deployment_status_reader', 'public.starring_product_deployment_status_read_v2(text,text,text,text,text,text,text,text,bytea)');

DO $preflight$
DECLARE
    owner_oid OID := pg_catalog.to_regrole('starring_owner');
    user_schema_oids OID[];
BEGIN
    SELECT pg_catalog.array_agg(namespace.oid ORDER BY namespace.oid)
    INTO user_schema_oids
    FROM pg_catalog.pg_namespace AS namespace
    WHERE namespace.nspname <> 'information_schema'
        AND pg_catalog.left(namespace.nspname, 3) <> 'pg_';

    IF (SELECT pg_catalog.count(*) FROM pg_temp.starring_api_request_roles) <> 13
        OR (SELECT pg_catalog.count(*) FROM pg_temp.starring_api_capability_manifest) <> 43
        OR EXISTS (
            SELECT 1
            FROM pg_temp.starring_api_capability_manifest AS expected
            INNER JOIN pg_catalog.pg_proc AS function_row
                ON function_row.oid = pg_catalog.to_regprocedure(
                    expected.function_identity
                )
            WHERE function_row.proowner <> owner_oid
                OR function_row.prokind <> 'f'
        )
        OR EXISTS (
            SELECT 1
            FROM pg_temp.starring_api_capability_manifest AS expected
            WHERE pg_catalog.to_regprocedure(expected.function_identity) IS NULL
        )
    THEN
        RAISE EXCEPTION 'staging enable capability manifest is invalid'
            USING ERRCODE = '55000';
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_authid AS role
        WHERE role.oid = owner_oid
            AND NOT role.rolcanlogin
            AND NOT role.rolsuper
            AND NOT role.rolcreatedb
            AND NOT role.rolcreaterole
            AND NOT role.rolinherit
            AND NOT role.rolreplication
            AND NOT role.rolbypassrls
            AND role.rolconnlimit = 0
            AND role.rolvaliduntil = 'infinity'::TIMESTAMP WITH TIME ZONE
            AND role.rolpassword IS NULL
    ) THEN
        RAISE EXCEPTION 'staging owner role enable preflight failed'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM pg_catalog.pg_authid AS role
        INNER JOIN pg_temp.starring_api_request_roles AS expected
            ON expected.role_name = role.rolname
        WHERE role.rolcanlogin
            OR role.rolsuper
            OR role.rolcreatedb
            OR role.rolcreaterole
            OR role.rolinherit
            OR role.rolreplication
            OR role.rolbypassrls
            OR role.rolconnlimit <> 4
            OR role.rolvaliduntil
                IS DISTINCT FROM 'infinity'::TIMESTAMP WITH TIME ZONE
            OR role.rolpassword IS NULL
            OR role.rolpassword NOT LIKE 'SCRAM-SHA-256$%'
    ) OR (
        SELECT pg_catalog.count(*)
        FROM pg_catalog.pg_authid AS role
        INNER JOIN pg_temp.starring_api_request_roles AS expected
            ON expected.role_name = role.rolname
    ) <> 13 THEN
        RAISE EXCEPTION 'staging request role enable attribute preflight failed'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        WITH managed_grantees AS (
            SELECT pg_catalog.to_regrole(role_name::TEXT) AS role_oid
            FROM pg_temp.starring_api_request_roles
            UNION ALL
            SELECT 0::OID
            UNION ALL
            SELECT pg_catalog.to_regrole('starring_api')
            WHERE pg_catalog.to_regrole('starring_api') IS NOT NULL
        )
        SELECT 1
        FROM pg_catalog.pg_parameter_acl AS parameter_acl
        CROSS JOIN LATERAL pg_catalog.aclexplode(
            parameter_acl.paracl
        ) AS privilege
        WHERE privilege.grantee IN (
            SELECT role_oid FROM managed_grantees
        )
    ) THEN
        RAISE EXCEPTION 'staging parameter capability enable preflight failed'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        WITH managed_roles AS (
            SELECT pg_catalog.to_regrole(role_name::TEXT) AS role_oid
            FROM pg_temp.starring_api_request_roles
            UNION ALL
            SELECT owner_oid
            UNION ALL
            SELECT pg_catalog.to_regrole('starring_api')
            WHERE pg_catalog.to_regrole('starring_api') IS NOT NULL
        )
        SELECT 1
        FROM pg_catalog.pg_auth_members AS membership
        WHERE membership.roleid IN (
                SELECT role_oid FROM managed_roles
            )
            OR membership.member IN (
                SELECT role_oid FROM managed_roles
            )
    ) OR EXISTS (
        WITH managed_roles AS (
            SELECT pg_catalog.to_regrole(role_name::TEXT) AS role_oid
            FROM pg_temp.starring_api_request_roles
            UNION ALL
            SELECT owner_oid
            UNION ALL
            SELECT pg_catalog.to_regrole('starring_api')
            WHERE pg_catalog.to_regrole('starring_api') IS NOT NULL
        )
        SELECT 1
        FROM pg_catalog.pg_db_role_setting AS setting
        WHERE setting.setrole IN (
            SELECT role_oid FROM managed_roles
        )
    ) OR EXISTS (
        SELECT 1
        FROM pg_catalog.pg_stat_activity AS activity
        WHERE activity.pid <> pg_catalog.pg_backend_pid()
            AND (
                activity.backend_type = 'client backend'
                OR activity.usename IN (
                    SELECT role_name::TEXT
                    FROM pg_temp.starring_api_request_roles
                    UNION ALL
                    SELECT 'starring_owner'
                    UNION ALL
                    SELECT 'starring_api'
                    WHERE pg_catalog.to_regrole('starring_api') IS NOT NULL
                )
            )
    ) OR EXISTS (
        SELECT 1
        FROM pg_catalog.pg_prepared_xacts AS prepared
    ) THEN
        RAISE EXCEPTION 'staging request role enable isolation preflight failed'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        WITH managed_roles AS (
            SELECT pg_catalog.to_regrole(role_name::TEXT) AS role_oid
            FROM pg_temp.starring_api_request_roles
        )
        SELECT 1
        FROM managed_roles AS managed
        WHERE EXISTS (
                SELECT 1
                FROM pg_catalog.pg_shdepend AS dependency
                WHERE dependency.refclassid = 'pg_catalog.pg_authid'::REGCLASS
                    AND dependency.refobjid = managed.role_oid
                    AND dependency.deptype = 'o'
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.pg_database AS database_row
                WHERE database_row.datname = pg_catalog.current_database()
                    AND database_row.datdba = managed.role_oid
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.pg_namespace AS namespace
                WHERE namespace.oid = ANY(user_schema_oids)
                    AND namespace.nspowner = managed.role_oid
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.pg_class AS relation
                INNER JOIN pg_catalog.pg_namespace AS namespace
                    ON namespace.oid = relation.relnamespace
                WHERE namespace.oid = ANY(user_schema_oids)
                    AND relation.relowner = managed.role_oid
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.pg_proc AS function_row
                INNER JOIN pg_catalog.pg_namespace AS namespace
                    ON namespace.oid = function_row.pronamespace
                WHERE namespace.oid = ANY(user_schema_oids)
                    AND function_row.proowner = managed.role_oid
            )
    ) THEN
        RAISE EXCEPTION 'staging request role enable ownership preflight failed'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM pg_catalog.pg_database AS database_row
        CROSS JOIN LATERAL pg_catalog.aclexplode(
            COALESCE(
                database_row.datacl,
                pg_catalog.acldefault('d', database_row.datdba)
            )
        ) AS privilege
        WHERE database_row.datname = pg_catalog.current_database()
            AND privilege.grantee = 0
    ) OR EXISTS (
        SELECT 1
        FROM pg_catalog.pg_namespace AS namespace
        CROSS JOIN LATERAL pg_catalog.aclexplode(
            COALESCE(
                namespace.nspacl,
                pg_catalog.acldefault('n', namespace.nspowner)
            )
        ) AS privilege
        WHERE namespace.nspname = 'public'
            AND privilege.grantee = 0
    ) THEN
        RAISE EXCEPTION 'staging public database or schema ACL enable preflight failed'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM pg_temp.starring_api_request_roles AS expected
        WHERE NOT pg_catalog.has_database_privilege(
                expected.role_name,
                pg_catalog.current_database(),
                'CONNECT'
            )
            OR pg_catalog.has_database_privilege(
                expected.role_name,
                pg_catalog.current_database(),
                'CREATE'
            )
            OR pg_catalog.has_database_privilege(
                expected.role_name,
                pg_catalog.current_database(),
                'TEMPORARY'
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.pg_database AS database_row
                WHERE database_row.datallowconn
                    AND database_row.datname <> pg_catalog.current_database()
                    AND pg_catalog.has_database_privilege(
                        expected.role_name,
                        database_row.oid,
                        'CONNECT'
                    )
            )
            OR NOT pg_catalog.has_schema_privilege(
                expected.role_name,
                'public',
                'USAGE'
            )
            OR pg_catalog.has_schema_privilege(
                expected.role_name,
                'public',
                'CREATE'
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.pg_namespace AS namespace
                WHERE namespace.oid = ANY(user_schema_oids)
                    AND namespace.nspname <> 'public'
                    AND (
                        pg_catalog.has_schema_privilege(
                            expected.role_name,
                            namespace.oid,
                            'USAGE'
                        )
                        OR pg_catalog.has_schema_privilege(
                            expected.role_name,
                            namespace.oid,
                            'CREATE'
                        )
                    )
            )
    ) THEN
        RAISE EXCEPTION 'staging request role enable database preflight failed'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM pg_catalog.pg_class AS relation
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = relation.relnamespace
        CROSS JOIN pg_temp.starring_api_request_roles AS expected
        WHERE namespace.oid = ANY(user_schema_oids)
            AND relation.relkind IN ('r', 'p', 'v', 'm', 'f', 'S')
            AND (
                (
                    relation.relkind = 'S'
                    AND EXISTS (
                        SELECT 1
                        FROM (
                            VALUES ('USAGE'), ('SELECT'), ('UPDATE')
                        ) AS checked(privilege_name)
                        WHERE pg_catalog.has_sequence_privilege(
                            expected.role_name,
                            relation.oid,
                            checked.privilege_name
                        )
                    )
                )
                OR (
                    relation.relkind <> 'S'
                    AND EXISTS (
                        SELECT 1
                        FROM (
                            VALUES
                                ('SELECT'),
                                ('INSERT'),
                                ('UPDATE'),
                                ('DELETE'),
                                ('TRUNCATE'),
                                ('REFERENCES'),
                                ('TRIGGER')
                        ) AS checked(privilege_name)
                        WHERE pg_catalog.has_table_privilege(
                            expected.role_name,
                            relation.oid,
                            checked.privilege_name
                        )
                    )
                )
                OR (
                    relation.relkind <> 'S'
                    AND EXISTS (
                        SELECT 1
                        FROM (
                            VALUES
                                ('SELECT'),
                                ('INSERT'),
                                ('UPDATE'),
                                ('REFERENCES')
                        ) AS checked(privilege_name)
                        WHERE pg_catalog.has_any_column_privilege(
                            expected.role_name,
                            relation.oid,
                            checked.privilege_name
                        )
                    )
                )
            )
    ) THEN
        RAISE EXCEPTION 'staging request role enable relation preflight failed'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM pg_temp.starring_api_request_roles AS expected_role
        CROSS JOIN pg_catalog.pg_proc AS function_row
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = function_row.pronamespace
        WHERE namespace.oid = ANY(user_schema_oids)
            AND function_row.prokind IN ('f', 'p', 'a', 'w')
            AND pg_catalog.has_function_privilege(
                expected_role.role_name,
                function_row.oid,
                'EXECUTE'
            ) IS DISTINCT FROM EXISTS (
                SELECT 1
                FROM pg_temp.starring_api_capability_manifest AS expected_function
                WHERE expected_function.role_name = expected_role.role_name
                    AND pg_catalog.to_regprocedure(
                        expected_function.function_identity
                    ) = function_row.oid
            )
    ) OR EXISTS (
        SELECT 1
        FROM pg_temp.starring_api_capability_manifest AS expected
        WHERE pg_catalog.has_function_privilege(
            expected.role_name,
            pg_catalog.to_regprocedure(expected.function_identity),
            'EXECUTE WITH GRANT OPTION'
        )
    ) OR EXISTS (
        SELECT 1
        FROM pg_temp.starring_api_capability_manifest AS expected
        INNER JOIN pg_catalog.pg_proc AS function_row
            ON function_row.oid = pg_catalog.to_regprocedure(
                expected.function_identity
            )
        CROSS JOIN LATERAL pg_catalog.aclexplode(
            COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )
        ) AS privilege
        WHERE privilege.privilege_type = 'EXECUTE'
            AND privilege.grantee NOT IN (
                function_row.proowner,
                pg_catalog.to_regrole(expected.role_name::TEXT)
            )
    ) THEN
        RAISE EXCEPTION 'staging request role enable function preflight failed'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        WITH managed_default_owners AS (
            SELECT pg_catalog.to_regrole(role_name::TEXT) AS role_oid
            FROM pg_temp.starring_api_request_roles
            UNION ALL
            SELECT owner_oid
            UNION ALL
            SELECT pg_catalog.to_regrole('starring_api')
            WHERE pg_catalog.to_regrole('starring_api') IS NOT NULL
        )
        SELECT 1
        FROM pg_catalog.pg_default_acl AS defaults
        CROSS JOIN LATERAL pg_catalog.aclexplode(defaults.defaclacl) AS privilege
        WHERE defaults.defaclrole IN (
                SELECT role_oid FROM managed_default_owners
            )
            AND privilege.grantee <> defaults.defaclrole
    ) THEN
        RAISE EXCEPTION 'staging request role enable default privilege preflight failed'
            USING ERRCODE = '55000';
    END IF;
END;
$preflight$;

DO $enable$
DECLARE
    request_role RECORD;
BEGIN
    FOR request_role IN
        SELECT role_name
        FROM pg_temp.starring_api_request_roles
        ORDER BY role_name
    LOOP
        EXECUTE pg_catalog.format(
            'ALTER ROLE %I LOGIN',
            request_role.role_name
        );
    END LOOP;
END;
$enable$;

DO $postflight$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM pg_catalog.pg_authid AS role
        INNER JOIN pg_temp.starring_api_request_roles AS expected
            ON expected.role_name = role.rolname
        WHERE NOT role.rolcanlogin
            OR role.rolsuper
            OR role.rolcreatedb
            OR role.rolcreaterole
            OR role.rolinherit
            OR role.rolreplication
            OR role.rolbypassrls
            OR role.rolconnlimit <> 4
            OR role.rolvaliduntil
                IS DISTINCT FROM 'infinity'::TIMESTAMP WITH TIME ZONE
            OR role.rolpassword IS NULL
            OR role.rolpassword NOT LIKE 'SCRAM-SHA-256$%'
    ) OR (
        SELECT pg_catalog.count(*)
        FROM pg_catalog.pg_authid AS role
        INNER JOIN pg_temp.starring_api_request_roles AS expected
            ON expected.role_name = role.rolname
    ) <> 13 THEN
        RAISE EXCEPTION 'staging request role enable postflight failed'
            USING ERRCODE = '55000';
    END IF;
END;
$postflight$;

COMMIT;
