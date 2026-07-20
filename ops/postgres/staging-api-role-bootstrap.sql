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
        RAISE EXCEPTION 'staging role bootstrap requires a cluster administrator'
            USING ERRCODE = '42501';
    END IF;
END;
$guard$;

CREATE TEMP TABLE starring_api_request_roles (
    role_name NAME PRIMARY KEY
) ON COMMIT PRESERVE ROWS;

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
) ON COMMIT PRESERVE ROWS;

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

DO $roles$
DECLARE
    request_role RECORD;
    database_entry RECORD;
BEGIN
    IF pg_catalog.to_regrole('starring_owner') IS NOT NULL THEN
        ALTER ROLE starring_owner
            NOLOGIN
            NOSUPERUSER
            NOCREATEDB
            NOCREATEROLE
            NOINHERIT
            NOREPLICATION
            NOBYPASSRLS
            CONNECTION LIMIT 0
            VALID UNTIL 'infinity'
            PASSWORD NULL;
        ALTER ROLE starring_owner RESET ALL;
        FOR database_entry IN
            SELECT datname FROM pg_catalog.pg_database ORDER BY datname
        LOOP
            EXECUTE pg_catalog.format(
                'ALTER ROLE starring_owner IN DATABASE %I RESET ALL',
                database_entry.datname
            );
        END LOOP;
    END IF;

    FOR request_role IN
        SELECT role_name
        FROM pg_temp.starring_api_request_roles
        ORDER BY role_name
    LOOP
        IF pg_catalog.to_regrole(request_role.role_name) IS NULL THEN
            EXECUTE pg_catalog.format(
                'CREATE ROLE %I NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 4 VALID UNTIL ''infinity'' PASSWORD NULL',
                request_role.role_name
            );
        ELSE
            EXECUTE pg_catalog.format(
                'ALTER ROLE %I NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 4 VALID UNTIL ''infinity'' PASSWORD NULL',
                request_role.role_name
            );
        END IF;
        EXECUTE pg_catalog.format(
            'ALTER ROLE %I RESET ALL',
            request_role.role_name
        );
        FOR database_entry IN
            SELECT datname FROM pg_catalog.pg_database ORDER BY datname
        LOOP
            EXECUTE pg_catalog.format(
                'ALTER ROLE %I IN DATABASE %I RESET ALL',
                request_role.role_name,
                database_entry.datname
            );
        END LOOP;
    END LOOP;

    IF pg_catalog.to_regrole('starring_api') IS NOT NULL THEN
        ALTER ROLE starring_api
            NOLOGIN
            NOSUPERUSER
            NOCREATEDB
            NOCREATEROLE
            NOINHERIT
            NOREPLICATION
            NOBYPASSRLS
            CONNECTION LIMIT 0
            VALID UNTIL 'infinity'
            PASSWORD NULL;
        ALTER ROLE starring_api RESET ALL;
        FOR database_entry IN
            SELECT datname FROM pg_catalog.pg_database ORDER BY datname
        LOOP
            EXECUTE pg_catalog.format(
                'ALTER ROLE starring_api IN DATABASE %I RESET ALL',
                database_entry.datname
            );
        END LOOP;
    END IF;
END;
$roles$;

DO $memberships$
DECLARE
    membership RECORD;
BEGIN
    FOR membership IN
        WITH managed_roles AS (
            SELECT pg_catalog.to_regrole(role_name::TEXT) AS role_oid
            FROM pg_temp.starring_api_request_roles
            UNION ALL
            SELECT pg_catalog.to_regrole('starring_owner')
            UNION ALL
            SELECT pg_catalog.to_regrole('starring_api')
            WHERE pg_catalog.to_regrole('starring_api') IS NOT NULL
        )
        SELECT DISTINCT
            parent.rolname AS parent_name,
            member.rolname AS member_name,
            grantor.rolname AS grantor_name
        FROM pg_catalog.pg_auth_members AS membership_row
        INNER JOIN pg_catalog.pg_roles AS parent
            ON parent.oid = membership_row.roleid
        INNER JOIN pg_catalog.pg_roles AS member
            ON member.oid = membership_row.member
        INNER JOIN pg_catalog.pg_roles AS grantor
            ON grantor.oid = membership_row.grantor
        WHERE membership_row.roleid IN (
                SELECT role_oid FROM managed_roles
            )
            OR membership_row.member IN (
                SELECT role_oid FROM managed_roles
            )
        ORDER BY parent.rolname, member.rolname, grantor.rolname
    LOOP
        EXECUTE pg_catalog.format(
            'REVOKE %I FROM %I GRANTED BY %I CASCADE',
            membership.parent_name,
            membership.member_name,
            membership.grantor_name
        );
    END LOOP;
END;
$memberships$;

DO $revoke_capabilities$
DECLARE
    grantee RECORD;
    namespace_entry RECORD;
    column_grant RECORD;
    database_entry RECORD;
    parameter_entry RECORD;
BEGIN
    CREATE TEMP TABLE starring_api_revoked_grantees (
        grantee_oid OID,
        sql_identity TEXT NOT NULL,
        PRIMARY KEY (sql_identity)
    ) ON COMMIT DROP;

    INSERT INTO pg_temp.starring_api_revoked_grantees (
        grantee_oid,
        sql_identity
    )
    SELECT pg_catalog.to_regrole(role_name::TEXT),
        pg_catalog.quote_ident(role_name::TEXT)
    FROM pg_temp.starring_api_request_roles
    UNION ALL
    SELECT 0::OID, 'PUBLIC'
    UNION ALL
    SELECT pg_catalog.to_regrole('starring_api'),
        pg_catalog.quote_ident('starring_api')
    WHERE pg_catalog.to_regrole('starring_api') IS NOT NULL;

    FOR grantee IN
        SELECT grantee_oid, sql_identity
        FROM pg_temp.starring_api_revoked_grantees
        ORDER BY sql_identity
    LOOP
        FOR parameter_entry IN
            SELECT DISTINCT
                parameter_acl.parname,
                privilege.privilege_type,
                grantor.rolname AS grantor_name
            FROM pg_catalog.pg_parameter_acl AS parameter_acl
            CROSS JOIN LATERAL pg_catalog.aclexplode(
                parameter_acl.paracl
            ) AS privilege
            INNER JOIN pg_catalog.pg_roles AS grantor
                ON grantor.oid = privilege.grantor
            WHERE privilege.grantee = grantee.grantee_oid
                AND privilege.privilege_type IN ('SET', 'ALTER SYSTEM')
            ORDER BY
                parameter_acl.parname,
                privilege.privilege_type,
                grantor.rolname
        LOOP
            BEGIN
                EXECUTE pg_catalog.format(
                    'SET LOCAL ROLE %I',
                    parameter_entry.grantor_name
                );
                EXECUTE pg_catalog.format(
                    'REVOKE %s ON PARAMETER %I FROM %s GRANTED BY %I CASCADE',
                    parameter_entry.privilege_type,
                    parameter_entry.parname,
                    grantee.sql_identity,
                    parameter_entry.grantor_name
                );
                EXECUTE 'RESET ROLE';
            EXCEPTION
                WHEN OTHERS THEN
                    EXECUTE 'RESET ROLE';
                    RAISE;
            END;
        END LOOP;

        FOR database_entry IN
            SELECT datname
            FROM pg_catalog.pg_database
            WHERE datallowconn
            ORDER BY datname
        LOOP
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON DATABASE %I FROM %s',
                database_entry.datname,
                grantee.sql_identity
            );
        END LOOP;

        FOR namespace_entry IN
            SELECT nspname
            FROM pg_catalog.pg_namespace
            WHERE nspname <> 'information_schema'
                AND pg_catalog.left(nspname, 3) <> 'pg_'
            ORDER BY nspname
        LOOP
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON SCHEMA %I FROM %s CASCADE',
                namespace_entry.nspname,
                grantee.sql_identity
            );
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA %I FROM %s CASCADE',
                namespace_entry.nspname,
                grantee.sql_identity
            );
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA %I FROM %s CASCADE',
                namespace_entry.nspname,
                grantee.sql_identity
            );
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON ALL ROUTINES IN SCHEMA %I FROM %s CASCADE',
                namespace_entry.nspname,
                grantee.sql_identity
            );
        END LOOP;

        FOR column_grant IN
            SELECT DISTINCT
                namespace_row.nspname,
                relation.relname,
                attribute.attname,
                privilege.privilege_type
            FROM pg_catalog.pg_attribute AS attribute
            INNER JOIN pg_catalog.pg_class AS relation
                ON relation.oid = attribute.attrelid
            INNER JOIN pg_catalog.pg_namespace AS namespace_row
                ON namespace_row.oid = relation.relnamespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(
                NULLIF(attribute.attacl, '{}'::ACLITEM[])
            ) AS privilege
            WHERE namespace_row.nspname <> 'information_schema'
                AND pg_catalog.left(namespace_row.nspname, 3) <> 'pg_'
                AND relation.relkind IN ('r', 'p', 'v', 'm', 'f')
                AND attribute.attnum > 0
                AND NOT attribute.attisdropped
                AND privilege.grantee = grantee.grantee_oid
                AND privilege.privilege_type IN (
                    'SELECT', 'INSERT', 'UPDATE', 'REFERENCES'
                )
            ORDER BY
                namespace_row.nspname,
                relation.relname,
                attribute.attname,
                privilege.privilege_type
        LOOP
            EXECUTE pg_catalog.format(
                'REVOKE %s (%I) ON TABLE %I.%I FROM %s CASCADE',
                column_grant.privilege_type,
                column_grant.attname,
                column_grant.nspname,
                column_grant.relname,
                grantee.sql_identity
            );
        END LOOP;
    END LOOP;
END;
$revoke_capabilities$;

DO $schema_create$
DECLARE
    unexpected_grantee RECORD;
BEGIN
    FOR unexpected_grantee IN
        SELECT DISTINCT
            CASE
                WHEN privilege.grantee = 0 THEN 'PUBLIC'
                ELSE pg_catalog.quote_ident(
                    pg_catalog.pg_get_userbyid(privilege.grantee)
                )
            END AS sql_identity
        FROM pg_catalog.pg_namespace AS namespace
        CROSS JOIN LATERAL pg_catalog.aclexplode(
            COALESCE(
                namespace.nspacl,
                pg_catalog.acldefault('n', namespace.nspowner)
            )
        ) AS privilege
        WHERE namespace.nspname = 'public'
            AND privilege.privilege_type = 'CREATE'
            AND privilege.grantee <> namespace.nspowner
    LOOP
        EXECUTE pg_catalog.format(
            'REVOKE CREATE ON SCHEMA public FROM %s CASCADE',
            unexpected_grantee.sql_identity
        );
    END LOOP;
END;
$schema_create$;

DO $default_privileges$
DECLARE
    default_owner RECORD;
    default_entry RECORD;
    unexpected_grantee RECORD;
BEGIN
    CREATE TEMP TABLE starring_api_default_owners (
        role_oid OID PRIMARY KEY,
        role_name NAME NOT NULL UNIQUE
    ) ON COMMIT DROP;

    INSERT INTO pg_temp.starring_api_default_owners (role_oid, role_name)
    SELECT pg_catalog.to_regrole(role_name::TEXT), role_name
    FROM pg_temp.starring_api_request_roles
    UNION ALL
    SELECT pg_catalog.to_regrole('starring_owner'), 'starring_owner'
    WHERE pg_catalog.to_regrole('starring_owner') IS NOT NULL
    UNION ALL
    SELECT pg_catalog.to_regrole('starring_api'), 'starring_api'
    WHERE pg_catalog.to_regrole('starring_api') IS NOT NULL;

    FOR default_owner IN
        SELECT role_oid, role_name
        FROM pg_temp.starring_api_default_owners
        ORDER BY role_name
    LOOP
        FOR default_entry IN
            SELECT
                defaults.oid,
                namespace.nspname,
                CASE defaults.defaclobjtype
                    WHEN 'r' THEN 'TABLES'
                    WHEN 'S' THEN 'SEQUENCES'
                    WHEN 'f' THEN 'FUNCTIONS'
                    WHEN 'T' THEN 'TYPES'
                    WHEN 'n' THEN 'SCHEMAS'
                END AS object_group
            FROM pg_catalog.pg_default_acl AS defaults
            LEFT JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = defaults.defaclnamespace
            WHERE defaults.defaclrole = default_owner.role_oid
                AND defaults.defaclobjtype IN ('r', 'S', 'f', 'T', 'n')
            ORDER BY defaults.oid
        LOOP
            FOR unexpected_grantee IN
                SELECT DISTINCT
                    CASE
                        WHEN privilege.grantee = 0 THEN 'PUBLIC'
                        ELSE pg_catalog.quote_ident(
                            pg_catalog.pg_get_userbyid(privilege.grantee)
                        )
                    END AS sql_identity
                FROM pg_catalog.pg_default_acl AS defaults
                CROSS JOIN LATERAL pg_catalog.aclexplode(
                    defaults.defaclacl
                ) AS privilege
                WHERE defaults.oid = default_entry.oid
                    AND (
                        default_owner.role_name <> 'starring_owner'
                        OR privilege.grantee <> default_owner.role_oid
                    )
                ORDER BY sql_identity
            LOOP
                EXECUTE pg_catalog.format(
                    'ALTER DEFAULT PRIVILEGES FOR ROLE %I%s REVOKE ALL PRIVILEGES ON %s FROM %s',
                    default_owner.role_name,
                    CASE
                        WHEN default_entry.nspname IS NULL THEN ''
                        ELSE pg_catalog.format(
                            ' IN SCHEMA %I',
                            default_entry.nspname
                        )
                    END,
                    default_entry.object_group,
                    unexpected_grantee.sql_identity
                );
            END LOOP;

            IF default_owner.role_name <> 'starring_owner'
                AND default_entry.nspname IS NULL
            THEN
                EXECUTE pg_catalog.format(
                    'ALTER DEFAULT PRIVILEGES FOR ROLE %I GRANT ALL PRIVILEGES ON %s TO %I',
                    default_owner.role_name,
                    default_entry.object_group,
                    default_owner.role_name
                );
                IF default_entry.object_group = 'FUNCTIONS' THEN
                    EXECUTE pg_catalog.format(
                        'ALTER DEFAULT PRIVILEGES FOR ROLE %I GRANT EXECUTE ON FUNCTIONS TO PUBLIC',
                        default_owner.role_name
                    );
                ELSIF default_entry.object_group = 'TYPES' THEN
                    EXECUTE pg_catalog.format(
                        'ALTER DEFAULT PRIVILEGES FOR ROLE %I GRANT USAGE ON TYPES TO PUBLIC',
                        default_owner.role_name
                    );
                END IF;
            END IF;
        END LOOP;

        IF default_owner.role_name = 'starring_owner' THEN
            EXECUTE pg_catalog.format(
                'ALTER DEFAULT PRIVILEGES FOR ROLE %I REVOKE ALL PRIVILEGES ON TABLES FROM PUBLIC',
                default_owner.role_name
            );
            EXECUTE pg_catalog.format(
                'ALTER DEFAULT PRIVILEGES FOR ROLE %I REVOKE ALL PRIVILEGES ON SEQUENCES FROM PUBLIC',
                default_owner.role_name
            );
            EXECUTE pg_catalog.format(
                'ALTER DEFAULT PRIVILEGES FOR ROLE %I REVOKE ALL PRIVILEGES ON FUNCTIONS FROM PUBLIC',
                default_owner.role_name
            );
            EXECUTE pg_catalog.format(
                'ALTER DEFAULT PRIVILEGES FOR ROLE %I REVOKE ALL PRIVILEGES ON TYPES FROM PUBLIC',
                default_owner.role_name
            );
            EXECUTE pg_catalog.format(
                'ALTER DEFAULT PRIVILEGES FOR ROLE %I REVOKE ALL PRIVILEGES ON SCHEMAS FROM PUBLIC',
                default_owner.role_name
            );
        END IF;
    END LOOP;
END;
$default_privileges$;

COMMIT;

BEGIN;

SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';
SET LOCAL idle_in_transaction_session_timeout = '60s';
SET LOCAL search_path = pg_catalog;

DO $sessions$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM pg_catalog.pg_prepared_xacts AS prepared
    ) THEN
        RAISE EXCEPTION 'staging database has a prepared transaction'
            USING ERRCODE = '55000';
    END IF;

    PERFORM pg_catalog.pg_terminate_backend(activity.pid, 5000)
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
        );

    IF EXISTS (
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
    ) THEN
        RAISE EXCEPTION 'staging cluster client session drain failed'
            USING ERRCODE = '55000';
    END IF;
END;
$sessions$;

DO $capability_functions$
DECLARE
    manifest_entry RECORD;
    unexpected_grantee RECORD;
    function_oid OID;
    owner_oid OID := pg_catalog.to_regrole('starring_owner');
BEGIN
    IF owner_oid IS NULL
        OR (SELECT pg_catalog.count(*) FROM pg_temp.starring_api_request_roles) <> 13
        OR (SELECT pg_catalog.count(*) FROM pg_temp.starring_api_capability_manifest) <> 43
    THEN
        RAISE EXCEPTION 'staging API capability manifest cardinality is invalid'
            USING ERRCODE = '55000';
    END IF;

    FOR manifest_entry IN
        SELECT role_name, function_identity
        FROM pg_temp.starring_api_capability_manifest
        ORDER BY role_name, function_identity
    LOOP
        function_oid := pg_catalog.to_regprocedure(
            manifest_entry.function_identity
        );
        IF function_oid IS NULL
            OR NOT EXISTS (
                SELECT 1
                FROM pg_catalog.pg_proc AS function_row
                INNER JOIN pg_catalog.pg_namespace AS namespace
                    ON namespace.oid = function_row.pronamespace
                WHERE function_row.oid = function_oid
                    AND namespace.nspname = 'public'
                    AND function_row.prokind = 'f'
                    AND function_row.proowner = owner_oid
            )
        THEN
            RAISE EXCEPTION 'staging API capability function contract is unavailable'
                USING ERRCODE = '55000';
        END IF;

        FOR unexpected_grantee IN
            SELECT DISTINCT
                CASE
                    WHEN privilege.grantee = 0 THEN 'PUBLIC'
                    ELSE pg_catalog.quote_ident(
                        pg_catalog.pg_get_userbyid(privilege.grantee)
                    )
                END AS sql_identity
            FROM pg_catalog.pg_proc AS function_row
            CROSS JOIN LATERAL pg_catalog.aclexplode(
                COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )
            ) AS privilege
            WHERE function_row.oid = function_oid
                AND privilege.privilege_type = 'EXECUTE'
                AND privilege.grantee <> owner_oid
        LOOP
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM %s CASCADE',
                manifest_entry.function_identity,
                unexpected_grantee.sql_identity
            );
        END LOOP;
    END LOOP;
END;
$capability_functions$;

DO $grant_capabilities$
DECLARE
    request_role RECORD;
    manifest_entry RECORD;
    database_identity TEXT := pg_catalog.quote_ident(
        pg_catalog.current_database()
    );
BEGIN
    EXECUTE pg_catalog.format(
        'GRANT USAGE ON SCHEMA public TO %I',
        'starring_owner'
    );

    FOR request_role IN
        SELECT role_name
        FROM pg_temp.starring_api_request_roles
        ORDER BY role_name
    LOOP
        EXECUTE pg_catalog.format(
            'GRANT CONNECT ON DATABASE %s TO %I',
            database_identity,
            request_role.role_name
        );
        EXECUTE pg_catalog.format(
            'GRANT USAGE ON SCHEMA public TO %I',
            request_role.role_name
        );
    END LOOP;

    FOR manifest_entry IN
        SELECT role_name, function_identity
        FROM pg_temp.starring_api_capability_manifest
        ORDER BY role_name, function_identity
    LOOP
        EXECUTE pg_catalog.format(
            'GRANT EXECUTE ON FUNCTION %s TO %I',
            manifest_entry.function_identity,
            manifest_entry.role_name
        );
    END LOOP;
END;
$grant_capabilities$;

DO $postflight$
DECLARE
    owner_oid OID := pg_catalog.to_regrole('starring_owner');
    user_schema_oids OID[];
BEGIN
    SELECT pg_catalog.array_agg(namespace.oid ORDER BY namespace.oid)
    INTO user_schema_oids
    FROM pg_catalog.pg_namespace AS namespace
    WHERE namespace.nspname <> 'information_schema'
        AND pg_catalog.left(namespace.nspname, 3) <> 'pg_';

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
            AND pg_catalog.has_schema_privilege(
                role.oid,
                'public',
                'USAGE'
            )
    ) THEN
        RAISE EXCEPTION 'starring owner role postflight failed'
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
            OR role.rolpassword IS NOT NULL
    ) OR (
        SELECT pg_catalog.count(*)
        FROM pg_catalog.pg_authid AS role
        INNER JOIN pg_temp.starring_api_request_roles AS expected
            ON expected.role_name = role.rolname
    ) <> 13 THEN
        RAISE EXCEPTION 'staging request role attribute postflight failed'
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
        RAISE EXCEPTION 'staging parameter capability postflight failed'
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
    ) THEN
        RAISE EXCEPTION 'staging role membership postflight failed'
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
        FROM pg_catalog.pg_db_role_setting AS setting
        WHERE setting.setrole IN (
                SELECT role_oid FROM managed_roles
            )
    ) THEN
        RAISE EXCEPTION 'staging role setting postflight failed'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
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
        RAISE EXCEPTION 'staging cluster activity postflight failed'
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
        RAISE EXCEPTION 'staging request role object ownership postflight failed'
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
        RAISE EXCEPTION 'staging public database or schema ACL postflight failed'
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
        RAISE EXCEPTION 'staging database or schema capability postflight failed'
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
        RAISE EXCEPTION 'staging relation or sequence capability postflight failed'
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
    ) THEN
        RAISE EXCEPTION 'staging function capability postflight failed'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
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
        RAISE EXCEPTION 'staging function ACL postflight failed'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM pg_catalog.pg_namespace AS namespace
        CROSS JOIN LATERAL pg_catalog.aclexplode(
            COALESCE(
                namespace.nspacl,
                pg_catalog.acldefault('n', namespace.nspowner)
            )
        ) AS privilege
        WHERE namespace.nspname = 'public'
            AND privilege.privilege_type = 'CREATE'
            AND privilege.grantee <> namespace.nspowner
    ) THEN
        RAISE EXCEPTION 'staging public schema trust postflight failed'
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
        RAISE EXCEPTION 'staging default privilege postflight failed'
            USING ERRCODE = '55000';
    END IF;

    IF pg_catalog.to_regrole('starring_api') IS NOT NULL
        AND (
            EXISTS (
                SELECT 1
                FROM pg_catalog.pg_shdepend AS dependency
                WHERE dependency.refclassid = 'pg_catalog.pg_authid'::REGCLASS
                    AND dependency.refobjid
                        = pg_catalog.to_regrole('starring_api')
                    AND dependency.deptype = 'o'
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.pg_database AS database_row
                WHERE database_row.datname = pg_catalog.current_database()
                    AND database_row.datdba
                        = pg_catalog.to_regrole('starring_api')
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.pg_namespace AS namespace
                WHERE namespace.oid = ANY(user_schema_oids)
                    AND namespace.nspowner
                        = pg_catalog.to_regrole('starring_api')
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.pg_class AS relation
                INNER JOIN pg_catalog.pg_namespace AS namespace
                    ON namespace.oid = relation.relnamespace
                WHERE namespace.oid = ANY(user_schema_oids)
                    AND relation.relowner
                        = pg_catalog.to_regrole('starring_api')
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.pg_proc AS function_row
                INNER JOIN pg_catalog.pg_namespace AS namespace
                    ON namespace.oid = function_row.pronamespace
                WHERE namespace.oid = ANY(user_schema_oids)
                    AND function_row.proowner
                        = pg_catalog.to_regrole('starring_api')
            )
            OR pg_catalog.has_database_privilege(
                'starring_api',
                pg_catalog.current_database(),
                'CONNECT'
            )
            OR pg_catalog.has_database_privilege(
                'starring_api',
                pg_catalog.current_database(),
                'CREATE'
            )
            OR pg_catalog.has_database_privilege(
                'starring_api',
                pg_catalog.current_database(),
                'TEMPORARY'
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.pg_database AS database_row
                WHERE database_row.datallowconn
                    AND pg_catalog.has_database_privilege(
                        'starring_api',
                        database_row.oid,
                        'CONNECT'
                    )
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.pg_namespace AS namespace
                WHERE namespace.oid = ANY(user_schema_oids)
                    AND (
                        pg_catalog.has_schema_privilege(
                            'starring_api',
                            namespace.oid,
                            'USAGE'
                        )
                        OR pg_catalog.has_schema_privilege(
                            'starring_api',
                            namespace.oid,
                            'CREATE'
                        )
                    )
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.pg_class AS relation
                INNER JOIN pg_catalog.pg_namespace AS namespace
                    ON namespace.oid = relation.relnamespace
                WHERE namespace.oid = ANY(user_schema_oids)
                    AND relation.relkind IN ('r', 'p', 'v', 'm', 'f', 'S')
                    AND (
                        (
                            relation.relkind = 'S'
                            AND EXISTS (
                                SELECT 1
                                FROM (
                                    VALUES
                                        ('USAGE'),
                                        ('SELECT'),
                                        ('UPDATE')
                                ) AS checked(privilege_name)
                                WHERE pg_catalog.has_sequence_privilege(
                                    'starring_api',
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
                                    'starring_api',
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
                                    'starring_api',
                                    relation.oid,
                                    checked.privilege_name
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
                WHERE namespace.oid = ANY(user_schema_oids)
                    AND function_row.prokind IN ('f', 'p', 'a', 'w')
                    AND pg_catalog.has_function_privilege(
                        'starring_api',
                        function_row.oid,
                        'EXECUTE'
                    )
            )
        )
    THEN
        RAISE EXCEPTION 'legacy staging API role postflight failed'
            USING ERRCODE = '55000';
    END IF;
END;
$postflight$;

DROP TABLE pg_temp.starring_api_capability_manifest;
DROP TABLE pg_temp.starring_api_request_roles;

COMMIT;
