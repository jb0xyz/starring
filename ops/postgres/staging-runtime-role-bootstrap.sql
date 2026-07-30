\set ON_ERROR_STOP on

\if :{?runtime_enable}
\else
\echo 'runtime_enable is required'
SELECT 1 / 0;
\endif

\if :{?runtime_execution_role}
\else
\set runtime_execution_role starring_runtime_execution
\endif

\if :{?runtime_exact_target_role}
\else
\set runtime_exact_target_role starring_runtime_exact_target
\endif

\if :{?runtime_panel_role}
\else
\set runtime_panel_role starring_runtime_panel
\endif

\if :{?runtime_serving_role}
\else
\set runtime_serving_role starring_runtime_serving
\endif

\if :{?runtime_interaction_role}
\else
\set runtime_interaction_role starring_runtime_interaction
\endif

\if :{?expected_database}
\else
\echo 'expected_database is required'
SELECT 1 / 0;
\endif

\if :{?expected_system_identifier}
\else
\echo 'expected_system_identifier is required'
SELECT 1 / 0;
\endif

\if :{?runtime_dedicated_cluster_acknowledgement}
\else
\echo 'runtime_dedicated_cluster_acknowledgement is required'
SELECT 1 / 0;
\endif

SET lock_timeout = '5s';
SET statement_timeout = '60s';
SET idle_in_transaction_session_timeout = '60s';
SET search_path = pg_catalog;
SET starring.runtime_enable = :'runtime_enable';
SET starring.expected_staging_database = :'expected_database';
SET starring.expected_staging_system_identifier = :'expected_system_identifier';
SET starring.runtime_dedicated_cluster_acknowledgement =
    :'runtime_dedicated_cluster_acknowledgement';

BEGIN;

CREATE TEMP TABLE starring_runtime_capability_roles (
    capability TEXT PRIMARY KEY,
    role_name NAME NOT NULL UNIQUE,
    CHECK (capability IN (
        'execution',
        'exact_target',
        'panel',
        'serving',
        'interaction'
    ))
) ON COMMIT PRESERVE ROWS;

INSERT INTO pg_temp.starring_runtime_capability_roles (
    capability,
    role_name
)
VALUES
    ('execution', :'runtime_execution_role'),
    ('exact_target', :'runtime_exact_target_role'),
    ('panel', :'runtime_panel_role'),
    ('serving', :'runtime_serving_role'),
    ('interaction', :'runtime_interaction_role');

CREATE TEMP TABLE starring_runtime_capability_functions (
    capability TEXT NOT NULL,
    function_identity TEXT NOT NULL UNIQUE,
    PRIMARY KEY (capability, function_identity),
    FOREIGN KEY (capability)
        REFERENCES pg_temp.starring_runtime_capability_roles (capability)
) ON COMMIT PRESERVE ROWS;

INSERT INTO pg_temp.starring_runtime_capability_functions (
    capability,
    function_identity
)
VALUES
    ('execution', 'public.starring_runtime_execution_database_readiness_v1()'),
    ('execution', 'public.starring_runtime_execution_database_identity_v1()'),
    ('execution', 'public.starring_runtime_execution_claim_next_v1(text,bigint)'),
    ('execution', 'public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)'),
    ('execution', 'public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)'),
    ('execution', 'public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)'),
    ('execution', 'public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)'),
    ('execution', 'public.starring_runtime_execution_recover_stale_live_v1()'),
    ('execution', 'public.starring_runtime_observe_previous_serving_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,jsonb)'),
    ('execution', 'public.starring_runtime_gateway_owner_observe_v1(text)'),
    ('execution', 'public.starring_runtime_gateway_owner_acquire_v1(text,text,text,bigint)'),
    ('execution', 'public.starring_runtime_gateway_owner_renew_v1(text,text,bigint,text,bigint,bigint)'),
    ('execution', 'public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)'),
    ('execution', 'public.starring_runtime_ingress_open_acknowledgement_observe_v2(text)'),
    ('execution', 'public.starring_runtime_ingress_open_acknowledgement_publish_v2(text,bigint,bytea,bytea,bigint,bigint,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,bigint,bigint,bigint,bigint,bigint)'),
    ('execution', 'public.starring_runtime_writer_fence_observe_v1()'),
    ('execution', 'public.starring_runtime_product_drain_observe_v2(text,text,text,bigint,text,text)'),
    ('execution', 'public.starring_runtime_certification_reserve_intent_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint,bytea,text)'),
    ('execution', 'public.starring_runtime_certification_reservation_observe_v2(text,text,text,bigint,bigint)'),
    ('execution', 'public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)'),
    ('execution', 'public.starring_runtime_startup_recovery_execute_stale_live_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)'),
    ('execution', 'public.starring_runtime_startup_recovery_execute_reserved_awaiting_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)'),
    ('execution', 'public.starring_runtime_startup_recovery_execute_suspended_local_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint)'),
    ('execution', 'public.starring_runtime_startup_recovery_select_pending_drain_v2(text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)'),
    ('execution', 'public.starring_runtime_startup_recovery_record_pending_drain_none_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint)'),
    ('execution', 'public.starring_runtime_startup_recovery_execute_pending_drain_v2(text,bigint,bigint,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean,text)'),
    ('execution', 'public.starring_runtime_startup_recovery_select_pending_drain_v3(text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)'),
    ('execution', 'public.starring_runtime_startup_recovery_pending_drain_succession_v3(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean)'),
    ('exact_target', 'public.starring_runtime_exact_target_database_readiness_v2()'),
    ('exact_target', 'public.starring_runtime_exact_target_reader_database_identity_v1()'),
    ('exact_target', 'public.starring_runtime_exact_target_read_v2(text,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text)'),
    ('panel', 'public.starring_runtime_panel_database_identity_v1()'),
    ('panel', 'public.starring_runtime_panel_database_readiness_v1()'),
    ('panel', 'public.starring_runtime_panel_reconciliation_claim_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text)'),
    ('panel', 'public.starring_runtime_panel_reconciliation_check_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint)'),
    ('panel', 'public.starring_runtime_panel_reconciliation_snapshot_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint)'),
    ('panel', 'public.starring_runtime_panel_reconciliation_installation_upsert_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,text,bigint,text,text,text,bigint)'),
    ('panel', 'public.starring_runtime_panel_reconciliation_installation_remove_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,text)'),
    ('panel', 'public.starring_runtime_panel_reconciliation_journal_put_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,smallint,text,text,jsonb)'),
    ('panel', 'public.starring_runtime_panel_reconciliation_journal_remove_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,text)'),
    ('serving', 'public.starring_runtime_serving_database_readiness_v1()'),
    ('serving', 'public.starring_runtime_serving_database_identity_v1()'),
    ('serving', 'public.starring_runtime_serving_heartbeat_v1(text,text,text,text,text,bigint,bigint,bigint,bigint)'),
    ('serving', 'public.starring_runtime_serving_disconnect_v1(text,text,text,text,text,bigint,bigint,bigint)'),
    ('interaction', 'public.starring_runtime_interaction_database_readiness_v1()'),
    ('interaction', 'public.starring_runtime_interaction_database_identity_v1()'),
    ('interaction', 'public.starring_runtime_interaction_route_read_v1(text,text)'),
    ('interaction', 'public.starring_runtime_interaction_pinned_read_v1(text,text)'),
    ('interaction', 'public.starring_runtime_interaction_instance_register_v1(text,text,text,bigint,text,text,jsonb)');

SELECT pg_catalog.pg_advisory_lock(
    pg_catalog.hashtextextended(
        pg_catalog.format(
            'starring-runtime-role-bootstrap-v2:%s:%s',
            :'expected_database',
            :'expected_system_identifier'
        ),
        0
    )
);

DO $guard$
DECLARE
    actual_system_identifier TEXT;
BEGIN
    IF pg_catalog.current_setting('starring.runtime_enable', TRUE)
        NOT IN ('on', 'off')
    THEN
        RAISE EXCEPTION 'runtime role mode must be on or off'
            USING ERRCODE = '22023';
    END IF;

    IF pg_catalog.current_setting('server_version_num')::INTEGER
        NOT BETWEEN 160000 AND 169999
    THEN
        RAISE EXCEPTION 'runtime staging PostgreSQL major version is unsupported'
            USING ERRCODE = '55000';
    END IF;

    IF pg_catalog.current_database()
            IS DISTINCT FROM pg_catalog.current_setting(
                'starring.expected_staging_database'
            )
        OR pg_catalog.current_database()
            !~ '^starring(_[a-z0-9]+)*_staging(_[a-z0-9]+)*$'
    THEN
        RAISE EXCEPTION 'runtime staging database acknowledgement is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT system_identifier::TEXT
    INTO actual_system_identifier
    FROM pg_catalog.pg_control_system();

    IF actual_system_identifier IS DISTINCT FROM pg_catalog.current_setting(
        'starring.expected_staging_system_identifier'
    ) THEN
        RAISE EXCEPTION 'runtime staging cluster acknowledgement is invalid'
            USING ERRCODE = '55000';
    END IF;

    IF pg_catalog.current_setting(
            'starring.runtime_dedicated_cluster_acknowledgement'
        ) IS DISTINCT FROM pg_catalog.format(
            'starring-runtime-dedicated-staging-cluster-v2:%s:%s:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation',
            actual_system_identifier,
            pg_catalog.current_database()
        )
    THEN
        RAISE EXCEPTION 'runtime dedicated staging cluster acknowledgement is invalid'
            USING ERRCODE = '55000';
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_roles AS role
        WHERE role.rolname = current_user
            AND role.rolsuper
    ) THEN
        RAISE EXCEPTION 'runtime staging role bootstrap requires a cluster administrator'
            USING ERRCODE = '42501';
    END IF;

    IF (
        SELECT pg_catalog.count(*)
        FROM pg_temp.starring_runtime_capability_roles
    ) <> 5
        OR EXISTS (
            SELECT 1
            FROM pg_temp.starring_runtime_capability_roles AS expected
            WHERE expected.role_name::TEXT
                !~ '^[a-z_][a-z0-9_]{0,62}$'
                OR expected.role_name = current_user
                OR expected.role_name
                    = pg_catalog.current_setting('session_authorization')
                OR expected.role_name::TEXT <> CASE expected.capability
                    WHEN 'execution' THEN 'starring_runtime_execution'
                    WHEN 'exact_target' THEN 'starring_runtime_exact_target'
                    WHEN 'panel' THEN 'starring_runtime_panel'
                    WHEN 'serving' THEN 'starring_runtime_serving'
                    WHEN 'interaction' THEN 'starring_runtime_interaction'
                END
        )
    THEN
        RAISE EXCEPTION 'runtime capability role identities are not exact'
            USING ERRCODE = '22023';
    END IF;

    IF (
        SELECT pg_catalog.count(*)
        FROM pg_temp.starring_runtime_capability_functions
    ) <> 49 THEN
        RAISE EXCEPTION 'runtime capability function manifest is invalid'
            USING ERRCODE = '55000';
    END IF;
END;
$guard$;

COMMIT;

BEGIN;

DO $seal$
DECLARE
    enable_roles BOOLEAN;
    role_entry RECORD;
BEGIN
    enable_roles := pg_catalog.current_setting('starring.runtime_enable') = 'on';

    FOR role_entry IN
        SELECT role_name
        FROM pg_temp.starring_runtime_capability_roles
        ORDER BY capability
    LOOP
        IF NOT enable_roles THEN
            IF pg_catalog.to_regrole(role_entry.role_name::TEXT) IS NULL THEN
                EXECUTE pg_catalog.format(
                    'CREATE ROLE %I NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 4 VALID UNTIL ''infinity'' PASSWORD NULL',
                    role_entry.role_name
                );
            ELSE
                EXECUTE pg_catalog.format(
                    'ALTER ROLE %I NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 4 VALID UNTIL ''infinity'' PASSWORD NULL',
                    role_entry.role_name
                );
            END IF;
        ELSIF NOT EXISTS (
            SELECT 1
            FROM pg_catalog.pg_authid AS role
            WHERE role.rolname = role_entry.role_name
                AND NOT role.rolsuper
                AND NOT role.rolcreatedb
                AND NOT role.rolcreaterole
                AND NOT role.rolinherit
                AND NOT role.rolreplication
                AND NOT role.rolbypassrls
                AND NOT role.rolcanlogin
                AND role.rolconnlimit = 4
                AND role.rolvaliduntil
                    IS NOT DISTINCT FROM 'infinity'::TIMESTAMPTZ
                AND role.rolpassword LIKE 'SCRAM-SHA-256$%'
        ) THEN
            RAISE EXCEPTION 'runtime capability role password preflight failed'
                USING ERRCODE = '55000';
        END IF;
    END LOOP;

END;
$seal$;

COMMIT;

BEGIN;

DO $membership_cleanup$
DECLARE
    membership_entry RECORD;
BEGIN
    FOR membership_entry IN
        SELECT
            granted_role.rolname AS granted_role_name,
            member_role.rolname AS member_role_name,
            grantor_role.rolname AS grantor_role_name
        FROM pg_catalog.pg_auth_members AS membership
        INNER JOIN pg_catalog.pg_roles AS granted_role
            ON granted_role.oid = membership.roleid
        INNER JOIN pg_catalog.pg_roles AS member_role
            ON member_role.oid = membership.member
        INNER JOIN pg_catalog.pg_roles AS grantor_role
            ON grantor_role.oid = membership.grantor
        WHERE membership.roleid IN (
                SELECT pg_catalog.to_regrole(role_name::TEXT)
                FROM pg_temp.starring_runtime_capability_roles
            )
            OR membership.member IN (
                SELECT pg_catalog.to_regrole(role_name::TEXT)
                FROM pg_temp.starring_runtime_capability_roles
            )
        ORDER BY
            granted_role.rolname,
            member_role.rolname,
            grantor_role.rolname
    LOOP
        EXECUTE pg_catalog.format(
            'SET LOCAL ROLE %I',
            membership_entry.grantor_role_name
        );
        EXECUTE pg_catalog.format(
            'REVOKE %I FROM %I CASCADE',
            membership_entry.granted_role_name,
            membership_entry.member_role_name
        );
        EXECUTE 'RESET ROLE';
    END LOOP;
END;
$membership_cleanup$;

COMMIT;

BEGIN;

DO $isolation_guard$
DECLARE
    role_entry RECORD;
    role_oid OID;
BEGIN
    IF EXISTS (
        SELECT 1
        FROM pg_catalog.pg_stat_activity AS activity
        WHERE activity.pid <> pg_catalog.pg_backend_pid()
            AND (
                activity.backend_type = 'client backend'
                OR activity.usesysid IN (
                    SELECT pg_catalog.to_regrole(
                        expected.role_name::TEXT
                    )
                    FROM pg_temp.starring_runtime_capability_roles AS expected
                )
            )
    ) OR EXISTS (
        SELECT 1
        FROM pg_catalog.pg_prepared_xacts AS prepared
    ) THEN
        RAISE EXCEPTION 'runtime staging cluster is not quiescent'
            USING ERRCODE = '55000';
    END IF;

    FOR role_entry IN
        SELECT role_name
        FROM pg_temp.starring_runtime_capability_roles
        ORDER BY capability
    LOOP
        role_oid := pg_catalog.to_regrole(role_entry.role_name::TEXT);

        IF role_oid IS NULL THEN
            RAISE EXCEPTION 'runtime capability role is not isolated'
                USING ERRCODE = '55000';
        END IF;

        IF EXISTS (
            SELECT 1
            FROM pg_catalog.pg_auth_members AS membership
            WHERE membership.member = role_oid
                OR membership.roleid = role_oid
        ) OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_shdepend AS dependency
            WHERE dependency.refclassid
                    = 'pg_catalog.pg_authid'::REGCLASS
                AND dependency.refobjid = role_oid
                AND dependency.deptype = 'o'
        ) THEN
            RAISE EXCEPTION 'runtime capability role has external ownership or membership'
                USING ERRCODE = '55000';
        END IF;

    END LOOP;
END;
$isolation_guard$;

DO $quarantine_cleanup$
DECLARE
    enable_roles BOOLEAN;
    role_entry RECORD;
    owner_entry RECORD;
    default_acl_entry RECORD;
    database_entry RECORD;
    schema_entry RECORD;
    relation_entry RECORD;
    function_entry RECORD;
    type_entry RECORD;
    language_entry RECORD;
    foreign_data_wrapper_entry RECORD;
    foreign_server_entry RECORD;
    tablespace_entry RECORD;
    large_object_entry RECORD;
    parameter_entry RECORD;
BEGIN
    enable_roles := pg_catalog.current_setting('starring.runtime_enable') = 'on';

    IF NOT enable_roles THEN
        REVOKE ALL PRIVILEGES ON SCHEMA public FROM PUBLIC CASCADE;

        FOR database_entry IN
            SELECT datname
            FROM pg_catalog.pg_database
            ORDER BY datname
        LOOP
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON DATABASE %I FROM PUBLIC CASCADE',
                database_entry.datname
            );
        END LOOP;

        FOR owner_entry IN
            SELECT role.rolname
            FROM pg_catalog.pg_roles AS role
            WHERE role.rolname <> 'pg_database_owner'
                AND role.oid NOT IN (
                    SELECT pg_catalog.to_regrole(
                        expected.role_name::TEXT
                    )
                    FROM pg_temp.starring_runtime_capability_roles AS expected
                )
                AND pg_catalog.has_schema_privilege(
                    role.oid,
                    'public',
                    'CREATE'
                )
            ORDER BY role.rolname
        LOOP
            EXECUTE pg_catalog.format(
                'ALTER DEFAULT PRIVILEGES FOR ROLE %I REVOKE ALL PRIVILEGES ON TABLES FROM PUBLIC CASCADE',
                owner_entry.rolname
            );
            EXECUTE pg_catalog.format(
                'ALTER DEFAULT PRIVILEGES FOR ROLE %I REVOKE ALL PRIVILEGES ON SEQUENCES FROM PUBLIC CASCADE',
                owner_entry.rolname
            );
            EXECUTE pg_catalog.format(
                'ALTER DEFAULT PRIVILEGES FOR ROLE %I REVOKE ALL PRIVILEGES ON ROUTINES FROM PUBLIC CASCADE',
                owner_entry.rolname
            );
            EXECUTE pg_catalog.format(
                'ALTER DEFAULT PRIVILEGES FOR ROLE %I REVOKE ALL PRIVILEGES ON TYPES FROM PUBLIC CASCADE',
                owner_entry.rolname
            );
            EXECUTE pg_catalog.format(
                'ALTER DEFAULT PRIVILEGES FOR ROLE %I REVOKE ALL PRIVILEGES ON SCHEMAS FROM PUBLIC CASCADE',
                owner_entry.rolname
            );
            EXECUTE pg_catalog.format(
                'ALTER DEFAULT PRIVILEGES FOR ROLE %I IN SCHEMA public REVOKE ALL PRIVILEGES ON TABLES FROM PUBLIC CASCADE',
                owner_entry.rolname
            );
            EXECUTE pg_catalog.format(
                'ALTER DEFAULT PRIVILEGES FOR ROLE %I IN SCHEMA public REVOKE ALL PRIVILEGES ON SEQUENCES FROM PUBLIC CASCADE',
                owner_entry.rolname
            );
            EXECUTE pg_catalog.format(
                'ALTER DEFAULT PRIVILEGES FOR ROLE %I IN SCHEMA public REVOKE ALL PRIVILEGES ON ROUTINES FROM PUBLIC CASCADE',
                owner_entry.rolname
            );
            EXECUTE pg_catalog.format(
                'ALTER DEFAULT PRIVILEGES FOR ROLE %I IN SCHEMA public REVOKE ALL PRIVILEGES ON TYPES FROM PUBLIC CASCADE',
                owner_entry.rolname
            );
        END LOOP;

        FOR default_acl_entry IN
            SELECT DISTINCT
                owner_role.rolname AS owner_name,
                namespace.nspname AS schema_name,
                CASE default_acl.defaclobjtype
                    WHEN 'r' THEN 'TABLES'
                    WHEN 'S' THEN 'SEQUENCES'
                    WHEN 'f' THEN 'ROUTINES'
                    WHEN 'T' THEN 'TYPES'
                    WHEN 'n' THEN 'SCHEMAS'
                END AS object_family,
                pg_catalog.format('%I', grantee_role.rolname)
                    AS grantee_identity
            FROM pg_catalog.pg_default_acl AS default_acl
            INNER JOIN pg_catalog.pg_roles AS owner_role
                ON owner_role.oid = default_acl.defaclrole
            LEFT JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = default_acl.defaclnamespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(
                default_acl.defaclacl
            ) AS privilege
            INNER JOIN pg_catalog.pg_roles AS grantee_role
                ON grantee_role.oid = privilege.grantee
            WHERE default_acl.defaclobjtype IN ('r', 'S', 'f', 'T', 'n')
                AND privilege.grantee <> default_acl.defaclrole
                AND privilege.grantee IN (
                    SELECT pg_catalog.to_regrole(
                        expected.role_name::TEXT
                    )
                    FROM pg_temp.starring_runtime_capability_roles AS expected
                )
            ORDER BY owner_name, schema_name, object_family, grantee_identity
        LOOP
            IF default_acl_entry.schema_name IS NULL THEN
                EXECUTE pg_catalog.format(
                    'ALTER DEFAULT PRIVILEGES FOR ROLE %I REVOKE ALL PRIVILEGES ON %s FROM %s CASCADE',
                    default_acl_entry.owner_name,
                    default_acl_entry.object_family,
                    default_acl_entry.grantee_identity
                );
            ELSE
                EXECUTE pg_catalog.format(
                    'ALTER DEFAULT PRIVILEGES FOR ROLE %I IN SCHEMA %I REVOKE ALL PRIVILEGES ON %s FROM %s CASCADE',
                    default_acl_entry.owner_name,
                    default_acl_entry.schema_name,
                    default_acl_entry.object_family,
                    default_acl_entry.grantee_identity
                );
            END IF;
        END LOOP;

        FOR role_entry IN
            SELECT capability, role_name
            FROM pg_temp.starring_runtime_capability_roles
            ORDER BY capability
        LOOP
            EXECUTE pg_catalog.format(
                'ALTER ROLE %I RESET ALL',
                role_entry.role_name
            );

            FOR database_entry IN
                SELECT database_row.datname
                FROM pg_catalog.pg_db_role_setting AS setting
                INNER JOIN pg_catalog.pg_database AS database_row
                    ON database_row.oid = setting.setdatabase
                WHERE setting.setrole
                    = pg_catalog.to_regrole(role_entry.role_name::TEXT)
                ORDER BY database_row.datname
            LOOP
                EXECUTE pg_catalog.format(
                    'ALTER ROLE %I IN DATABASE %I RESET ALL',
                    role_entry.role_name,
                    database_entry.datname
                );
            END LOOP;

            FOR database_entry IN
                SELECT DISTINCT
                    database_row.datname,
                    grantor_role.rolname AS grantor_name
                FROM pg_catalog.pg_database AS database_row
                CROSS JOIN LATERAL pg_catalog.aclexplode(
                    database_row.datacl
                ) AS privilege
                INNER JOIN pg_catalog.pg_roles AS grantor_role
                    ON grantor_role.oid = privilege.grantor
                WHERE privilege.grantee
                    = pg_catalog.to_regrole(role_entry.role_name::TEXT)
                ORDER BY database_row.datname, grantor_name
            LOOP
                EXECUTE pg_catalog.format(
                    'SET LOCAL ROLE %I',
                    database_entry.grantor_name
                );
                EXECUTE pg_catalog.format(
                    'REVOKE ALL PRIVILEGES ON DATABASE %I FROM %I CASCADE',
                    database_entry.datname,
                    role_entry.role_name
                );
                EXECUTE 'RESET ROLE';
            END LOOP;

            FOR schema_entry IN
                SELECT DISTINCT
                    namespace.nspname,
                    grantor_role.rolname AS grantor_name
                FROM pg_catalog.pg_namespace AS namespace
                CROSS JOIN LATERAL pg_catalog.aclexplode(
                    namespace.nspacl
                ) AS privilege
                INNER JOIN pg_catalog.pg_roles AS grantor_role
                    ON grantor_role.oid = privilege.grantor
                WHERE privilege.grantee
                    = pg_catalog.to_regrole(role_entry.role_name::TEXT)
                ORDER BY namespace.nspname, grantor_name
            LOOP
                EXECUTE pg_catalog.format(
                    'SET LOCAL ROLE %I',
                    schema_entry.grantor_name
                );
                EXECUTE pg_catalog.format(
                    'REVOKE ALL PRIVILEGES ON SCHEMA %I FROM %I CASCADE',
                    schema_entry.nspname,
                    role_entry.role_name
                );
                EXECUTE 'RESET ROLE';
            END LOOP;

            FOR relation_entry IN
                SELECT DISTINCT
                    namespace.nspname,
                    relation.relname,
                    grantor_role.rolname AS grantor_name
                FROM pg_catalog.pg_class AS relation
                INNER JOIN pg_catalog.pg_namespace AS namespace
                    ON namespace.oid = relation.relnamespace
                CROSS JOIN LATERAL pg_catalog.aclexplode(
                    relation.relacl
                ) AS privilege
                INNER JOIN pg_catalog.pg_roles AS grantor_role
                    ON grantor_role.oid = privilege.grantor
                WHERE relation.relkind IN ('r', 'p', 'v', 'm', 'f')
                    AND privilege.grantee
                        = pg_catalog.to_regrole(role_entry.role_name::TEXT)
                ORDER BY
                    namespace.nspname,
                    relation.relname,
                    grantor_name
            LOOP
                EXECUTE pg_catalog.format(
                    'SET LOCAL ROLE %I',
                    relation_entry.grantor_name
                );
                EXECUTE pg_catalog.format(
                    'REVOKE ALL PRIVILEGES ON TABLE %I.%I FROM %I CASCADE',
                    relation_entry.nspname,
                    relation_entry.relname,
                    role_entry.role_name
                );
                EXECUTE 'RESET ROLE';
            END LOOP;

            FOR relation_entry IN
                SELECT DISTINCT
                    namespace.nspname,
                    relation.relname,
                    grantor_role.rolname AS grantor_name
                FROM pg_catalog.pg_class AS relation
                INNER JOIN pg_catalog.pg_namespace AS namespace
                    ON namespace.oid = relation.relnamespace
                CROSS JOIN LATERAL pg_catalog.aclexplode(
                    relation.relacl
                ) AS privilege
                INNER JOIN pg_catalog.pg_roles AS grantor_role
                    ON grantor_role.oid = privilege.grantor
                WHERE relation.relkind = 'S'
                    AND privilege.grantee
                        = pg_catalog.to_regrole(role_entry.role_name::TEXT)
                ORDER BY
                    namespace.nspname,
                    relation.relname,
                    grantor_name
            LOOP
                EXECUTE pg_catalog.format(
                    'SET LOCAL ROLE %I',
                    relation_entry.grantor_name
                );
                EXECUTE pg_catalog.format(
                    'REVOKE ALL PRIVILEGES ON SEQUENCE %I.%I FROM %I CASCADE',
                    relation_entry.nspname,
                    relation_entry.relname,
                    role_entry.role_name
                );
                EXECUTE 'RESET ROLE';
            END LOOP;

            FOR function_entry IN
                SELECT DISTINCT
                    function_row.oid::REGPROCEDURE::TEXT
                        AS function_identity,
                    grantor_role.rolname AS grantor_name
                FROM pg_catalog.pg_proc AS function_row
                CROSS JOIN LATERAL pg_catalog.aclexplode(
                    function_row.proacl
                ) AS privilege
                INNER JOIN pg_catalog.pg_roles AS grantor_role
                    ON grantor_role.oid = privilege.grantor
                WHERE privilege.grantee
                    = pg_catalog.to_regrole(role_entry.role_name::TEXT)
                ORDER BY function_identity, grantor_name
            LOOP
                EXECUTE pg_catalog.format(
                    'SET LOCAL ROLE %I',
                    function_entry.grantor_name
                );
                EXECUTE pg_catalog.format(
                    'REVOKE ALL PRIVILEGES ON ROUTINE %s FROM %I CASCADE',
                    function_entry.function_identity,
                    role_entry.role_name
                );
                EXECUTE 'RESET ROLE';
            END LOOP;

            FOR relation_entry IN
                SELECT
                    namespace.nspname,
                    relation.relname,
                    pg_catalog.string_agg(
                        pg_catalog.format('%I', attribute.attname),
                        ', '
                        ORDER BY attribute.attnum
                    ) AS column_list,
                    grantor_role.rolname AS grantor_name
                FROM pg_catalog.pg_class AS relation
                INNER JOIN pg_catalog.pg_namespace AS namespace
                    ON namespace.oid = relation.relnamespace
                INNER JOIN pg_catalog.pg_attribute AS attribute
                    ON attribute.attrelid = relation.oid
                CROSS JOIN LATERAL pg_catalog.aclexplode(
                    attribute.attacl
                ) AS privilege
                INNER JOIN pg_catalog.pg_roles AS grantor_role
                    ON grantor_role.oid = privilege.grantor
                WHERE relation.relkind IN ('r', 'p', 'v', 'm', 'f')
                    AND attribute.attnum > 0
                    AND NOT attribute.attisdropped
                    AND privilege.grantee
                        = pg_catalog.to_regrole(role_entry.role_name::TEXT)
                GROUP BY
                    namespace.nspname,
                    relation.relname,
                    grantor_role.rolname
                ORDER BY
                    namespace.nspname,
                    relation.relname,
                    grantor_name
            LOOP
                EXECUTE pg_catalog.format(
                    'SET LOCAL ROLE %I',
                    relation_entry.grantor_name
                );
                EXECUTE pg_catalog.format(
                    'REVOKE ALL PRIVILEGES (%s) ON TABLE %I.%I FROM %I CASCADE',
                    relation_entry.column_list,
                    relation_entry.nspname,
                    relation_entry.relname,
                    role_entry.role_name
                );
                EXECUTE 'RESET ROLE';
            END LOOP;

            FOR type_entry IN
                SELECT DISTINCT
                    namespace.nspname,
                    type_row.typname,
                    grantor_role.rolname AS grantor_name
                FROM pg_catalog.pg_type AS type_row
                INNER JOIN pg_catalog.pg_namespace AS namespace
                    ON namespace.oid = type_row.typnamespace
                CROSS JOIN LATERAL pg_catalog.aclexplode(
                    type_row.typacl
                ) AS privilege
                INNER JOIN pg_catalog.pg_roles AS grantor_role
                    ON grantor_role.oid = privilege.grantor
                WHERE privilege.grantee
                    = pg_catalog.to_regrole(role_entry.role_name::TEXT)
                ORDER BY
                    namespace.nspname,
                    type_row.typname,
                    grantor_name
            LOOP
                EXECUTE pg_catalog.format(
                    'SET LOCAL ROLE %I',
                    type_entry.grantor_name
                );
                EXECUTE pg_catalog.format(
                    'REVOKE ALL PRIVILEGES ON TYPE %I.%I FROM %I CASCADE',
                    type_entry.nspname,
                    type_entry.typname,
                    role_entry.role_name
                );
                EXECUTE 'RESET ROLE';
            END LOOP;

            FOR language_entry IN
                SELECT DISTINCT
                    language.lanname,
                    grantor_role.rolname AS grantor_name
                FROM pg_catalog.pg_language AS language
                CROSS JOIN LATERAL pg_catalog.aclexplode(
                    language.lanacl
                ) AS privilege
                INNER JOIN pg_catalog.pg_roles AS grantor_role
                    ON grantor_role.oid = privilege.grantor
                WHERE language.lanpltrusted
                    AND privilege.grantee
                        = pg_catalog.to_regrole(role_entry.role_name::TEXT)
                ORDER BY language.lanname, grantor_name
            LOOP
                EXECUTE pg_catalog.format(
                    'SET LOCAL ROLE %I',
                    language_entry.grantor_name
                );
                EXECUTE pg_catalog.format(
                    'REVOKE ALL PRIVILEGES ON LANGUAGE %I FROM %I CASCADE',
                    language_entry.lanname,
                    role_entry.role_name
                );
                EXECUTE 'RESET ROLE';
            END LOOP;

            FOR foreign_data_wrapper_entry IN
                SELECT DISTINCT
                    wrapper.fdwname,
                    grantor_role.rolname AS grantor_name
                FROM pg_catalog.pg_foreign_data_wrapper AS wrapper
                CROSS JOIN LATERAL pg_catalog.aclexplode(
                    wrapper.fdwacl
                ) AS privilege
                INNER JOIN pg_catalog.pg_roles AS grantor_role
                    ON grantor_role.oid = privilege.grantor
                WHERE privilege.grantee
                    = pg_catalog.to_regrole(role_entry.role_name::TEXT)
                ORDER BY wrapper.fdwname, grantor_name
            LOOP
                EXECUTE pg_catalog.format(
                    'SET LOCAL ROLE %I',
                    foreign_data_wrapper_entry.grantor_name
                );
                EXECUTE pg_catalog.format(
                    'REVOKE ALL PRIVILEGES ON FOREIGN DATA WRAPPER %I FROM %I CASCADE',
                    foreign_data_wrapper_entry.fdwname,
                    role_entry.role_name
                );
                EXECUTE 'RESET ROLE';
            END LOOP;

            FOR foreign_server_entry IN
                SELECT DISTINCT
                    server.srvname,
                    grantor_role.rolname AS grantor_name
                FROM pg_catalog.pg_foreign_server AS server
                CROSS JOIN LATERAL pg_catalog.aclexplode(
                    server.srvacl
                ) AS privilege
                INNER JOIN pg_catalog.pg_roles AS grantor_role
                    ON grantor_role.oid = privilege.grantor
                WHERE privilege.grantee
                    = pg_catalog.to_regrole(role_entry.role_name::TEXT)
                ORDER BY server.srvname, grantor_name
            LOOP
                EXECUTE pg_catalog.format(
                    'SET LOCAL ROLE %I',
                    foreign_server_entry.grantor_name
                );
                EXECUTE pg_catalog.format(
                    'REVOKE ALL PRIVILEGES ON FOREIGN SERVER %I FROM %I CASCADE',
                    foreign_server_entry.srvname,
                    role_entry.role_name
                );
                EXECUTE 'RESET ROLE';
            END LOOP;

            FOR tablespace_entry IN
                SELECT DISTINCT
                    tablespace.spcname,
                    grantor_role.rolname AS grantor_name
                FROM pg_catalog.pg_tablespace AS tablespace
                CROSS JOIN LATERAL pg_catalog.aclexplode(
                    tablespace.spcacl
                ) AS privilege
                INNER JOIN pg_catalog.pg_roles AS grantor_role
                    ON grantor_role.oid = privilege.grantor
                WHERE privilege.grantee
                    = pg_catalog.to_regrole(role_entry.role_name::TEXT)
                ORDER BY tablespace.spcname, grantor_name
            LOOP
                EXECUTE pg_catalog.format(
                    'SET LOCAL ROLE %I',
                    tablespace_entry.grantor_name
                );
                EXECUTE pg_catalog.format(
                    'REVOKE ALL PRIVILEGES ON TABLESPACE %I FROM %I CASCADE',
                    tablespace_entry.spcname,
                    role_entry.role_name
                );
                EXECUTE 'RESET ROLE';
            END LOOP;

            FOR large_object_entry IN
                SELECT DISTINCT
                    large_object.oid,
                    grantor_role.rolname AS grantor_name
                FROM pg_catalog.pg_largeobject_metadata AS large_object
                CROSS JOIN LATERAL pg_catalog.aclexplode(
                    large_object.lomacl
                ) AS privilege
                INNER JOIN pg_catalog.pg_roles AS grantor_role
                    ON grantor_role.oid = privilege.grantor
                WHERE privilege.grantee
                    = pg_catalog.to_regrole(role_entry.role_name::TEXT)
                ORDER BY large_object.oid, grantor_name
            LOOP
                EXECUTE pg_catalog.format(
                    'SET LOCAL ROLE %I',
                    large_object_entry.grantor_name
                );
                EXECUTE pg_catalog.format(
                    'REVOKE ALL PRIVILEGES ON LARGE OBJECT %s FROM %I CASCADE',
                    large_object_entry.oid,
                    role_entry.role_name
                );
                EXECUTE 'RESET ROLE';
            END LOOP;

            FOR parameter_entry IN
                SELECT DISTINCT
                    parameter_acl.parname,
                    grantor_role.rolname AS grantor_name
                FROM pg_catalog.pg_parameter_acl AS parameter_acl
                CROSS JOIN LATERAL pg_catalog.aclexplode(
                    parameter_acl.paracl
                ) AS privilege
                INNER JOIN pg_catalog.pg_roles AS grantor_role
                    ON grantor_role.oid = privilege.grantor
                WHERE privilege.grantee
                    = pg_catalog.to_regrole(role_entry.role_name::TEXT)
                ORDER BY parameter_acl.parname, grantor_name
            LOOP
                EXECUTE pg_catalog.format(
                    'SET LOCAL ROLE %I',
                    parameter_entry.grantor_name
                );
                EXECUTE pg_catalog.format(
                    'REVOKE ALL PRIVILEGES ON PARAMETER %I FROM %I CASCADE',
                    parameter_entry.parname,
                    role_entry.role_name
                );
                EXECUTE 'RESET ROLE';
            END LOOP;
        END LOOP;

        FOR function_entry IN
            SELECT DISTINCT
                function_row.oid::REGPROCEDURE::TEXT
                    AS function_identity,
                CASE
                    WHEN privilege.grantee = 0 THEN 'PUBLIC'
                    ELSE pg_catalog.format('%I', grantee_role.rolname)
                END AS grantee_identity,
                grantor_role.rolname AS grantor_name
            FROM pg_temp.starring_runtime_capability_functions AS expected
            INNER JOIN pg_temp.starring_runtime_capability_roles
                AS expected_role
                ON expected_role.capability = expected.capability
            INNER JOIN pg_catalog.pg_proc AS function_row
                ON function_row.oid = pg_catalog.to_regprocedure(
                    expected.function_identity
                )
            CROSS JOIN LATERAL pg_catalog.aclexplode(
                function_row.proacl
            ) AS privilege
            INNER JOIN pg_catalog.pg_roles AS grantor_role
                ON grantor_role.oid = privilege.grantor
            LEFT JOIN pg_catalog.pg_roles AS grantee_role
                ON grantee_role.oid = privilege.grantee
            WHERE privilege.grantee <> function_row.proowner
                AND privilege.grantee
                    <> pg_catalog.to_regrole(expected_role.role_name::TEXT)
            ORDER BY function_identity, grantee_identity, grantor_name
        LOOP
            EXECUTE pg_catalog.format(
                'SET LOCAL ROLE %I',
                function_entry.grantor_name
            );
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON ROUTINE %s FROM %s CASCADE',
                function_entry.function_identity,
                function_entry.grantee_identity
            );
            EXECUTE 'RESET ROLE';
        END LOOP;
    END IF;
END;
$quarantine_cleanup$;

DO $function_guard$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM pg_temp.starring_runtime_capability_functions AS expected
        WHERE pg_catalog.to_regprocedure(expected.function_identity) IS NULL
    ) THEN
        RAISE EXCEPTION 'runtime capability function manifest is unavailable'
            USING ERRCODE = '55000';
    END IF;
END;
$function_guard$;

DO $configure$
DECLARE
    role_entry RECORD;
    function_entry RECORD;
BEGIN
    IF pg_catalog.current_setting('starring.runtime_enable') = 'off' THEN
        FOR role_entry IN
            SELECT capability, role_name
            FROM pg_temp.starring_runtime_capability_roles
            ORDER BY capability
        LOOP
            EXECUTE pg_catalog.format(
                'GRANT CONNECT ON DATABASE %I TO %I',
                pg_catalog.current_database(),
                role_entry.role_name
            );
            EXECUTE pg_catalog.format(
                'GRANT USAGE ON SCHEMA public TO %I',
                role_entry.role_name
            );

            FOR function_entry IN
                SELECT
                    expected.function_identity,
                    owner_role.rolname AS owner_name
                FROM pg_temp.starring_runtime_capability_functions
                    AS expected
                INNER JOIN pg_catalog.pg_proc AS function_row
                    ON function_row.oid = pg_catalog.to_regprocedure(
                        expected.function_identity
                    )
                INNER JOIN pg_catalog.pg_roles AS owner_role
                    ON owner_role.oid = function_row.proowner
                WHERE expected.capability = role_entry.capability
                ORDER BY expected.function_identity
            LOOP
                EXECUTE pg_catalog.format(
                    'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE',
                    function_entry.function_identity
                );
                EXECUTE pg_catalog.format(
                    'GRANT EXECUTE ON FUNCTION %s TO %I',
                    function_entry.function_identity,
                    function_entry.owner_name
                );
                EXECUTE pg_catalog.format(
                    'GRANT EXECUTE ON FUNCTION %s TO %I',
                    function_entry.function_identity,
                    role_entry.role_name
                );
            END LOOP;
        END LOOP;
    END IF;
END;
$configure$;

DO $postflight$
DECLARE
    enable_roles BOOLEAN;
    database_oid OID;
    role_entry RECORD;
    role_oid OID;
    expected_function_count BIGINT;
    observed_function_count BIGINT;
BEGIN
    enable_roles := pg_catalog.current_setting('starring.runtime_enable') = 'on';

    SELECT database_row.oid
    INTO database_oid
    FROM pg_catalog.pg_database AS database_row
    WHERE database_row.datname = pg_catalog.current_database();

    IF EXISTS (
        WITH actual_public_privileges AS (
            SELECT
                'pg_catalog.pg_namespace'::REGCLASS::OID AS class_oid,
                namespace.oid AS object_oid,
                0::INTEGER AS object_subid,
                namespace.nspowner AS owner_oid,
                'n'::"char" AS object_type,
                namespace.nspname AS namespace_name,
                privilege.grantor,
                privilege.privilege_type,
                privilege.is_grantable
            FROM pg_catalog.pg_namespace AS namespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                namespace.nspacl,
                pg_catalog.acldefault('n', namespace.nspowner)
            )) AS privilege
            WHERE (
                    namespace.nspname = 'information_schema'
                    OR pg_catalog.left(namespace.nspname, 3) = 'pg_'
                )
                AND privilege.grantee = 0

            UNION ALL

            SELECT
                'pg_catalog.pg_class'::REGCLASS::OID,
                relation.oid,
                0::INTEGER,
                relation.relowner,
                CASE
                    WHEN relation.relkind = 'S' THEN 'S'::"char"
                    ELSE 'r'::"char"
                END,
                namespace.nspname,
                privilege.grantor,
                privilege.privilege_type,
                privilege.is_grantable
            FROM pg_catalog.pg_class AS relation
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = relation.relnamespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                relation.relacl,
                pg_catalog.acldefault(
                    CASE
                        WHEN relation.relkind = 'S' THEN 'S'::"char"
                        ELSE 'r'::"char"
                    END,
                    relation.relowner
                )
            )) AS privilege
            WHERE (
                    namespace.nspname = 'information_schema'
                    OR pg_catalog.left(namespace.nspname, 3) = 'pg_'
                )
                AND relation.relkind IN ('r', 'p', 'v', 'm', 'S', 'f')
                AND privilege.grantee = 0

            UNION ALL

            SELECT
                'pg_catalog.pg_class'::REGCLASS::OID,
                relation.oid,
                attribute.attnum::INTEGER,
                relation.relowner,
                'c'::"char",
                namespace.nspname,
                privilege.grantor,
                privilege.privilege_type,
                privilege.is_grantable
            FROM pg_catalog.pg_attribute AS attribute
            INNER JOIN pg_catalog.pg_class AS relation
                ON relation.oid = attribute.attrelid
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = relation.relnamespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                attribute.attacl,
                pg_catalog.acldefault('c', relation.relowner)
            )) AS privilege
            WHERE (
                    namespace.nspname = 'information_schema'
                    OR pg_catalog.left(namespace.nspname, 3) = 'pg_'
                )
                AND attribute.attnum > 0
                AND NOT attribute.attisdropped
                AND privilege.grantee = 0

            UNION ALL

            SELECT
                'pg_catalog.pg_proc'::REGCLASS::OID,
                function_row.oid,
                0::INTEGER,
                function_row.proowner,
                'f'::"char",
                namespace.nspname,
                privilege.grantor,
                privilege.privilege_type,
                privilege.is_grantable
            FROM pg_catalog.pg_proc AS function_row
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = function_row.pronamespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE (
                    namespace.nspname = 'information_schema'
                    OR pg_catalog.left(namespace.nspname, 3) = 'pg_'
                )
                AND privilege.grantee = 0

            UNION ALL

            SELECT
                'pg_catalog.pg_type'::REGCLASS::OID,
                type_row.oid,
                0::INTEGER,
                type_row.typowner,
                'T'::"char",
                namespace.nspname,
                privilege.grantor,
                privilege.privilege_type,
                privilege.is_grantable
            FROM pg_catalog.pg_type AS type_row
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = type_row.typnamespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                type_row.typacl,
                pg_catalog.acldefault('T', type_row.typowner)
            )) AS privilege
            WHERE (
                    namespace.nspname = 'information_schema'
                    OR pg_catalog.left(namespace.nspname, 3) = 'pg_'
                )
                AND privilege.grantee = 0

            UNION ALL

            SELECT
                'pg_catalog.pg_language'::REGCLASS::OID,
                language.oid,
                0::INTEGER,
                language.lanowner,
                'l'::"char",
                NULL::NAME,
                privilege.grantor,
                privilege.privilege_type,
                privilege.is_grantable
            FROM pg_catalog.pg_language AS language
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                language.lanacl,
                pg_catalog.acldefault('l', language.lanowner)
            )) AS privilege
            WHERE privilege.grantee = 0
        )
        SELECT 1
        FROM actual_public_privileges AS actual
        WHERE NOT (
            (
                actual.namespace_name = 'information_schema'
                AND actual.object_oid < 16384
                AND actual.class_oid
                    = 'pg_catalog.pg_namespace'::REGCLASS::OID
                AND actual.object_subid = 0
                AND actual.grantor = actual.owner_oid
                AND actual.privilege_type = 'USAGE'
                AND NOT actual.is_grantable
            )
            OR (
                actual.namespace_name = 'information_schema'
                AND actual.object_oid < 16384
                AND actual.class_oid
                    = 'pg_catalog.pg_class'::REGCLASS::OID
                AND actual.object_subid = 0
                AND actual.grantor = actual.owner_oid
                AND actual.privilege_type = 'SELECT'
                AND NOT actual.is_grantable
                AND EXISTS (
                    SELECT 1
                    FROM pg_catalog.pg_class AS baseline_relation
                    WHERE baseline_relation.oid = actual.object_oid
                        AND baseline_relation.relname NOT IN (
                            '_pg_foreign_data_wrappers',
                            '_pg_foreign_servers',
                            '_pg_foreign_table_columns',
                            '_pg_foreign_tables',
                            '_pg_user_mappings',
                            'sql_parts',
                            'transforms'
                        )
                )
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.pg_init_privs AS initial
                CROSS JOIN LATERAL pg_catalog.aclexplode(
                    initial.initprivs
                ) AS baseline
                WHERE initial.classoid = actual.class_oid
                    AND initial.objoid = actual.object_oid
                    AND initial.objsubid = actual.object_subid
                    AND baseline.grantee = 0
                    AND baseline.grantor = actual.grantor
                    AND baseline.privilege_type
                        = actual.privilege_type
                    AND baseline.is_grantable
                        = actual.is_grantable
            )
            OR (
                (
                    actual.object_oid < 16384
                    OR actual.namespace_name IN (
                        SELECT namespace.nspname
                        FROM pg_catalog.pg_namespace AS namespace
                        WHERE namespace.oid
                                = pg_catalog.pg_my_temp_schema()
                            OR namespace.nspname = pg_catalog.replace(
                                (
                                    SELECT temp_namespace.nspname
                                    FROM pg_catalog.pg_namespace
                                        AS temp_namespace
                                    WHERE temp_namespace.oid
                                        = pg_catalog.pg_my_temp_schema()
                                ),
                                'pg_temp_',
                                'pg_toast_temp_'
                            )
                    )
                )
                AND NOT EXISTS (
                    SELECT 1
                    FROM pg_catalog.pg_init_privs AS initial
                    WHERE initial.classoid = actual.class_oid
                        AND initial.objoid = actual.object_oid
                        AND initial.objsubid = actual.object_subid
                )
                AND EXISTS (
                    SELECT 1
                    FROM pg_catalog.aclexplode(
                        pg_catalog.acldefault(
                            actual.object_type,
                            actual.owner_oid
                        )
                    ) AS baseline
                    WHERE baseline.grantee = 0
                        AND baseline.grantor = actual.grantor
                        AND baseline.privilege_type
                            = actual.privilege_type
                        AND baseline.is_grantable
                            = actual.is_grantable
                )
            )
        )
    ) THEN
        RAISE EXCEPTION 'runtime system PUBLIC privileges are invalid'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM pg_catalog.pg_database AS database_row
        CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
            database_row.datacl,
            pg_catalog.acldefault('d', database_row.datdba)
        )) AS privilege
        WHERE privilege.grantee = 0
    ) OR EXISTS (
        SELECT 1
        FROM pg_catalog.pg_namespace AS namespace
        CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
            namespace.nspacl,
            pg_catalog.acldefault('n', namespace.nspowner)
        )) AS privilege
        WHERE namespace.nspname = 'public'
            AND privilege.grantee = 0
    ) THEN
        RAISE EXCEPTION 'runtime public boundary privileges are invalid'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM pg_temp.starring_runtime_capability_functions AS expected
        INNER JOIN pg_temp.starring_runtime_capability_roles
            AS expected_role
            ON expected_role.capability = expected.capability
        INNER JOIN pg_catalog.pg_proc AS function_row
            ON function_row.oid = pg_catalog.to_regprocedure(
                expected.function_identity
            )
        CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
            function_row.proacl,
            pg_catalog.acldefault('f', function_row.proowner)
        )) AS privilege
        GROUP BY
            function_row.oid,
            function_row.proowner,
            expected_role.role_name
        HAVING pg_catalog.count(*) <> 2
            OR pg_catalog.count(*) FILTER (
                WHERE privilege.grantee = function_row.proowner
                    AND privilege.grantor = function_row.proowner
                    AND privilege.privilege_type = 'EXECUTE'
                    AND NOT privilege.is_grantable
            ) <> 1
            OR pg_catalog.count(*) FILTER (
                WHERE privilege.grantee = pg_catalog.to_regrole(
                        expected_role.role_name::TEXT
                    )
                    AND privilege.grantor = function_row.proowner
                    AND privilege.privilege_type = 'EXECUTE'
                    AND NOT privilege.is_grantable
            ) <> 1
    ) THEN
        RAISE EXCEPTION 'runtime capability function ACL topology is invalid'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM pg_catalog.pg_roles AS owner_role
        WHERE owner_role.rolname <> 'pg_database_owner'
            AND pg_catalog.has_schema_privilege(
                owner_role.oid,
                'public',
                'CREATE'
            )
            AND (
                NOT EXISTS (
                    SELECT 1
                    FROM pg_catalog.pg_default_acl AS default_acl
                    WHERE default_acl.defaclrole = owner_role.oid
                        AND default_acl.defaclnamespace = 0
                        AND default_acl.defaclobjtype = 'f'
                        AND NOT EXISTS (
                            SELECT 1
                            FROM pg_catalog.aclexplode(
                                default_acl.defaclacl
                            ) AS privilege
                            WHERE privilege.grantee = 0
                        )
                )
                OR NOT EXISTS (
                    SELECT 1
                    FROM pg_catalog.pg_default_acl AS default_acl
                    WHERE default_acl.defaclrole = owner_role.oid
                        AND default_acl.defaclnamespace = 0
                        AND default_acl.defaclobjtype = 'T'
                        AND NOT EXISTS (
                            SELECT 1
                            FROM pg_catalog.aclexplode(
                                default_acl.defaclacl
                            ) AS privilege
                            WHERE privilege.grantee = 0
                        )
                )
            )
    ) OR EXISTS (
        SELECT 1
        FROM pg_catalog.pg_default_acl AS default_acl
        INNER JOIN pg_catalog.pg_roles AS owner_role
            ON owner_role.oid = default_acl.defaclrole
        LEFT JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = default_acl.defaclnamespace
        CROSS JOIN LATERAL pg_catalog.aclexplode(
            default_acl.defaclacl
        ) AS privilege
        WHERE (
                privilege.grantee = 0
                AND owner_role.rolname <> 'pg_database_owner'
                AND pg_catalog.has_schema_privilege(
                    owner_role.oid,
                    'public',
                    'CREATE'
                )
                AND (
                    default_acl.defaclnamespace = 0
                    OR namespace.nspname = 'public'
                )
            )
            OR (
                privilege.grantee <> default_acl.defaclrole
                AND privilege.grantee IN (
                    SELECT pg_catalog.to_regrole(
                        expected.role_name::TEXT
                    )
                    FROM pg_temp.starring_runtime_capability_roles AS expected
                )
            )
    ) THEN
        RAISE EXCEPTION 'runtime default privileges are invalid'
            USING ERRCODE = '55000';
    END IF;

    FOR role_entry IN
        SELECT capability, role_name
        FROM pg_temp.starring_runtime_capability_roles
        ORDER BY capability
    LOOP
        role_oid := pg_catalog.to_regrole(role_entry.role_name::TEXT);

        IF role_oid IS NULL
            OR NOT EXISTS (
                SELECT 1
                FROM pg_catalog.pg_authid AS role
                WHERE role.oid = role_oid
                    AND NOT role.rolcanlogin
                    AND NOT role.rolsuper
                    AND NOT role.rolcreatedb
                    AND NOT role.rolcreaterole
                    AND NOT role.rolinherit
                    AND NOT role.rolreplication
                    AND NOT role.rolbypassrls
                    AND role.rolconnlimit = 4
                    AND role.rolvaliduntil
                        IS NOT DISTINCT FROM 'infinity'::TIMESTAMPTZ
                    AND (
                        (
                            enable_roles
                            AND role.rolpassword LIKE 'SCRAM-SHA-256$%'
                        )
                        OR (
                            NOT enable_roles
                            AND role.rolpassword IS NULL
                        )
                    )
            )
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.pg_database AS database_row
                CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                    database_row.datacl,
                    pg_catalog.acldefault('d', database_row.datdba)
                )) AS privilege
                WHERE database_row.oid = database_oid
                    AND privilege.grantee = role_oid
                    AND privilege.privilege_type = 'CONNECT'
                    AND NOT privilege.is_grantable
            ) <> 1
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.pg_database AS database_row
                CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                    database_row.datacl,
                    pg_catalog.acldefault('d', database_row.datdba)
                )) AS privilege
                WHERE database_row.oid = database_oid
                    AND privilege.grantee = role_oid
                    AND (
                        privilege.privilege_type <> 'CONNECT'
                        OR privilege.is_grantable
                    )
            )
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.pg_namespace AS namespace
                CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                    namespace.nspacl,
                    pg_catalog.acldefault('n', namespace.nspowner)
                )) AS privilege
                WHERE namespace.nspname = 'public'
                    AND privilege.grantee = role_oid
                    AND privilege.privilege_type = 'USAGE'
                    AND NOT privilege.is_grantable
            ) <> 1
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.pg_namespace AS namespace
                CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                    namespace.nspacl,
                    pg_catalog.acldefault('n', namespace.nspowner)
                )) AS privilege
                WHERE namespace.nspname = 'public'
                    AND privilege.grantee = role_oid
                    AND (
                        privilege.privilege_type <> 'USAGE'
                        OR privilege.is_grantable
                    )
            )
            OR EXISTS (
                SELECT 1
                FROM pg_temp.starring_runtime_capability_functions AS expected
                WHERE expected.capability = role_entry.capability
                    AND (
                        (
                            SELECT pg_catalog.count(*)
                            FROM pg_catalog.pg_proc AS function_row
                            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                                function_row.proacl,
                                pg_catalog.acldefault(
                                    'f',
                                    function_row.proowner
                                )
                            )) AS privilege
                            WHERE function_row.oid
                                = pg_catalog.to_regprocedure(
                                    expected.function_identity
                                )
                                AND privilege.grantee = role_oid
                                AND privilege.privilege_type = 'EXECUTE'
                        ) <> 1
                        OR EXISTS (
                            SELECT 1
                            FROM pg_catalog.pg_proc AS function_row
                            CROSS JOIN LATERAL pg_catalog.aclexplode(
                                function_row.proacl
                            ) AS privilege
                            WHERE function_row.oid
                                = pg_catalog.to_regprocedure(
                                    expected.function_identity
                                )
                                AND privilege.grantee = role_oid
                                AND (
                                    privilege.privilege_type <> 'EXECUTE'
                                    OR privilege.is_grantable
                                )
                        )
                    )
            )
        THEN
            RAISE EXCEPTION 'runtime capability role attributes are invalid'
                USING ERRCODE = '55000';
        END IF;

        IF EXISTS (
            SELECT 1
            FROM pg_catalog.pg_auth_members AS membership
            WHERE membership.member = role_oid
                OR membership.roleid = role_oid
        ) OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_db_role_setting AS setting
            WHERE setting.setrole IN (0, role_oid)
                AND setting.setdatabase IN (0, database_oid)
        ) THEN
            RAISE EXCEPTION 'runtime capability role settings or membership are invalid'
                USING ERRCODE = '55000';
        END IF;

        IF NOT pg_catalog.has_database_privilege(
                role_oid,
                database_oid,
                'CONNECT'
            )
            OR pg_catalog.has_database_privilege(
                role_oid,
                database_oid,
                'CREATE'
            )
            OR pg_catalog.has_database_privilege(
                role_oid,
                database_oid,
                'TEMPORARY'
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.pg_database AS database_row
                WHERE database_row.oid <> database_oid
                    AND database_row.datallowconn
                    AND (
                        pg_catalog.has_database_privilege(
                            role_oid,
                            database_row.oid,
                            'CONNECT'
                        )
                        OR pg_catalog.has_database_privilege(
                            role_oid,
                            database_row.oid,
                            'CREATE'
                        )
                        OR pg_catalog.has_database_privilege(
                            role_oid,
                            database_row.oid,
                            'TEMPORARY'
                        )
                    )
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.pg_database AS database_row
                CROSS JOIN LATERAL pg_catalog.aclexplode(
                    database_row.datacl
                ) AS privilege
                WHERE database_row.oid <> database_oid
                    AND privilege.grantee = role_oid
            )
        THEN
            RAISE EXCEPTION 'runtime capability database privileges are invalid'
                USING ERRCODE = '55000';
        END IF;

        IF NOT pg_catalog.has_schema_privilege(
                role_oid,
                'public',
                'USAGE'
            )
            OR pg_catalog.has_schema_privilege(
                role_oid,
                'public',
                'CREATE'
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.pg_namespace AS namespace
                WHERE namespace.nspname <> 'public'
                    AND namespace.nspname <> 'information_schema'
                    AND pg_catalog.left(namespace.nspname, 3) <> 'pg_'
                    AND (
                        pg_catalog.has_schema_privilege(
                            role_oid,
                            namespace.oid,
                            'USAGE'
                        )
                        OR pg_catalog.has_schema_privilege(
                            role_oid,
                            namespace.oid,
                            'CREATE'
                        )
                    )
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.pg_namespace AS namespace
                CROSS JOIN LATERAL pg_catalog.aclexplode(
                    namespace.nspacl
                ) AS privilege
                WHERE namespace.nspname <> 'public'
                    AND privilege.grantee = role_oid
            )
        THEN
            RAISE EXCEPTION 'runtime capability schema privileges are invalid'
                USING ERRCODE = '55000';
        END IF;

        IF EXISTS (
            SELECT 1
            FROM pg_catalog.pg_class AS relation
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = relation.relnamespace
            WHERE namespace.nspname <> 'information_schema'
                AND pg_catalog.left(namespace.nspname, 3) <> 'pg_'
                AND relation.relkind IN ('r', 'p', 'v', 'm', 'f')
                AND (
                    pg_catalog.has_table_privilege(
                        role_oid,
                        relation.oid,
                        'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER'
                    )
                    OR pg_catalog.has_any_column_privilege(
                        role_oid,
                        relation.oid,
                        'SELECT,INSERT,UPDATE,REFERENCES'
                    )
                )
        ) OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_sequences AS sequence
            WHERE sequence.schemaname <> 'information_schema'
                AND pg_catalog.left(sequence.schemaname, 3) <> 'pg_'
                AND pg_catalog.has_sequence_privilege(
                    role_oid,
                    pg_catalog.format(
                        '%I.%I',
                        sequence.schemaname,
                        sequence.sequencename
                    ),
                    'USAGE,SELECT,UPDATE'
                )
        ) OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_class AS relation
            CROSS JOIN LATERAL pg_catalog.aclexplode(
                relation.relacl
            ) AS privilege
            WHERE relation.relkind IN ('r', 'p', 'v', 'm', 'f', 'S')
                AND privilege.grantee = role_oid
        ) OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_attribute AS attribute
            CROSS JOIN LATERAL pg_catalog.aclexplode(
                attribute.attacl
            ) AS privilege
            WHERE attribute.attnum > 0
                AND NOT attribute.attisdropped
                AND privilege.grantee = role_oid
        ) THEN
            RAISE EXCEPTION 'runtime capability relation privileges are invalid'
                USING ERRCODE = '55000';
        END IF;

        SELECT pg_catalog.count(*)
        INTO expected_function_count
        FROM pg_temp.starring_runtime_capability_functions AS expected
        WHERE expected.capability = role_entry.capability;

        SELECT pg_catalog.count(*)
        INTO observed_function_count
        FROM pg_catalog.pg_proc AS function_row
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = function_row.pronamespace
        WHERE namespace.nspname <> 'information_schema'
            AND pg_catalog.left(namespace.nspname, 3) <> 'pg_'
            AND pg_catalog.has_function_privilege(
                role_oid,
                function_row.oid,
                'EXECUTE'
            );

        IF observed_function_count <> expected_function_count
            OR EXISTS (
                SELECT 1
                FROM pg_temp.starring_runtime_capability_functions AS expected
                WHERE expected.capability = role_entry.capability
                    AND NOT pg_catalog.has_function_privilege(
                        role_oid,
                        pg_catalog.to_regprocedure(
                            expected.function_identity
                        ),
                        'EXECUTE'
                    )
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.pg_proc AS function_row
                CROSS JOIN LATERAL pg_catalog.aclexplode(
                    function_row.proacl
                ) AS privilege
                WHERE privilege.grantee = role_oid
                    AND NOT EXISTS (
                        SELECT 1
                        FROM pg_temp.starring_runtime_capability_functions
                            AS expected
                        WHERE expected.capability = role_entry.capability
                            AND pg_catalog.to_regprocedure(
                                expected.function_identity
                            ) = function_row.oid
                    )
            )
        THEN
            RAISE EXCEPTION 'runtime capability function privileges are invalid'
                USING ERRCODE = '55000';
        END IF;

        IF EXISTS (
            SELECT 1
            FROM pg_catalog.pg_type AS type_row
            CROSS JOIN LATERAL pg_catalog.aclexplode(
                type_row.typacl
            ) AS privilege
            WHERE privilege.grantee = role_oid
        ) OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_type AS type_row
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = type_row.typnamespace
            LEFT JOIN pg_catalog.pg_class AS type_relation
                ON type_relation.oid = type_row.typrelid
            WHERE namespace.nspname <> 'information_schema'
                AND pg_catalog.left(namespace.nspname, 3) <> 'pg_'
                AND type_row.typisdefined
                AND type_row.typtype <> 'p'
                AND type_row.typelem = 0
                AND (
                    type_row.typrelid = 0
                    OR type_relation.relkind = 'c'
                )
                AND pg_catalog.has_type_privilege(
                    role_oid,
                    type_row.oid,
                    'USAGE'
                )
        ) OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_language AS language
            CROSS JOIN LATERAL pg_catalog.aclexplode(
                language.lanacl
            ) AS privilege
            WHERE privilege.grantee = role_oid
        ) OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_language AS language
            WHERE language.lanname NOT IN (
                    'internal',
                    'c',
                    'sql',
                    'plpgsql'
                )
                AND pg_catalog.has_language_privilege(
                    role_oid,
                    language.oid,
                    'USAGE'
                )
        ) OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_foreign_data_wrapper AS wrapper
            WHERE pg_catalog.has_foreign_data_wrapper_privilege(
                role_oid,
                wrapper.oid,
                'USAGE'
            )
        ) OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_foreign_server AS server
            WHERE pg_catalog.has_server_privilege(
                role_oid,
                server.oid,
                'USAGE'
            )
        ) OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_tablespace AS tablespace
            WHERE pg_catalog.has_tablespace_privilege(
                role_oid,
                tablespace.oid,
                'CREATE'
            )
        ) THEN
            RAISE EXCEPTION 'runtime capability extended object privileges are invalid'
                USING ERRCODE = '55000';
        END IF;

        IF EXISTS (
            SELECT 1
            FROM pg_catalog.pg_parameter_acl AS parameter_acl
            CROSS JOIN LATERAL pg_catalog.aclexplode(
                parameter_acl.paracl
            ) AS privilege
            WHERE privilege.grantee IN (0, role_oid)
                AND privilege.privilege_type IN ('SET', 'ALTER SYSTEM')
        ) OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_largeobject_metadata AS large_object
            WHERE large_object.lomowner = role_oid
                OR EXISTS (
                    SELECT 1
                    FROM pg_catalog.aclexplode(COALESCE(
                        large_object.lomacl,
                        pg_catalog.acldefault(
                            'L',
                            large_object.lomowner
                        )
                    )) AS privilege
                    WHERE privilege.grantee IN (0, role_oid)
                        AND (
                            privilege.privilege_type IN (
                                'SELECT',
                                'UPDATE'
                            )
                            OR privilege.is_grantable
                        )
                )
        ) THEN
            RAISE EXCEPTION 'runtime capability cluster privileges are invalid'
                USING ERRCODE = '55000';
        END IF;
    END LOOP;
END;
$postflight$;

DO $isolation_postflight$
DECLARE
    role_entry RECORD;
    role_oid OID;
BEGIN
    IF EXISTS (
        SELECT 1
        FROM pg_catalog.pg_stat_activity AS activity
        WHERE activity.pid <> pg_catalog.pg_backend_pid()
            AND (
                activity.backend_type = 'client backend'
                OR activity.usesysid IN (
                    SELECT pg_catalog.to_regrole(
                        expected.role_name::TEXT
                    )
                    FROM pg_temp.starring_runtime_capability_roles AS expected
                )
            )
    ) OR EXISTS (
        SELECT 1
        FROM pg_catalog.pg_prepared_xacts AS prepared
    ) THEN
        RAISE EXCEPTION 'runtime staging cluster lost quiescence'
            USING ERRCODE = '55000';
    END IF;

    FOR role_entry IN
        SELECT role_name
        FROM pg_temp.starring_runtime_capability_roles
        ORDER BY capability
    LOOP
        role_oid := pg_catalog.to_regrole(role_entry.role_name::TEXT);

        IF role_oid IS NULL OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_auth_members AS membership
            WHERE membership.member = role_oid
                OR membership.roleid = role_oid
        ) OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_shdepend AS dependency
            WHERE dependency.refclassid
                    = 'pg_catalog.pg_authid'::REGCLASS
                AND dependency.refobjid = role_oid
                AND dependency.deptype = 'o'
        ) THEN
            RAISE EXCEPTION 'runtime capability role lost isolation'
                USING ERRCODE = '55000';
        END IF;
    END LOOP;
END;
$isolation_postflight$;

DO $activate$
DECLARE
    role_entry RECORD;
BEGIN
    IF pg_catalog.current_setting('starring.runtime_enable') = 'on' THEN
        FOR role_entry IN
            SELECT role_name
            FROM pg_temp.starring_runtime_capability_roles
            ORDER BY capability
        LOOP
            EXECUTE pg_catalog.format(
                'ALTER ROLE %I LOGIN',
                role_entry.role_name
            );
        END LOOP;
    END IF;
END;
$activate$;

DO $activation_postflight$
DECLARE
    enable_roles BOOLEAN;
    role_entry RECORD;
BEGIN
    enable_roles := pg_catalog.current_setting('starring.runtime_enable') = 'on';

    FOR role_entry IN
        SELECT role_name
        FROM pg_temp.starring_runtime_capability_roles
        ORDER BY capability
    LOOP
        IF NOT EXISTS (
            SELECT 1
            FROM pg_catalog.pg_roles AS role
            WHERE role.rolname = role_entry.role_name
                AND role.rolcanlogin IS NOT DISTINCT FROM enable_roles
        ) THEN
            RAISE EXCEPTION 'runtime capability role activation is invalid'
                USING ERRCODE = '55000';
        END IF;
    END LOOP;
END;
$activation_postflight$;

COMMIT;

DO $unlock$
BEGIN
    IF NOT pg_catalog.pg_advisory_unlock(
        pg_catalog.hashtextextended(
            pg_catalog.format(
                'starring-runtime-role-bootstrap-v2:%s:%s',
                pg_catalog.current_setting(
                    'starring.expected_staging_database'
                ),
                pg_catalog.current_setting(
                    'starring.expected_staging_system_identifier'
                )
            ),
            0
        )
    ) THEN
        RAISE EXCEPTION 'runtime role bootstrap serialization lock is unavailable'
            USING ERRCODE = '55000';
    END IF;
END;
$unlock$;
