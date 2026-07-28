\set ON_ERROR_STOP on

\if :{?runtime_enable}
\else
\set runtime_enable off
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
\quit
\endif

\if :{?expected_system_identifier}
\else
\echo 'expected_system_identifier is required'
\quit
\endif

BEGIN;

SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';
SET LOCAL idle_in_transaction_session_timeout = '60s';
SET LOCAL search_path = pg_catalog;
SET LOCAL starring.runtime_enable = :'runtime_enable';
SET LOCAL starring.expected_staging_database = :'expected_database';
SET LOCAL starring.expected_staging_system_identifier = :'expected_system_identifier';

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
) ON COMMIT DROP;

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
) ON COMMIT DROP;

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
    ('exact_target', 'public.starring_runtime_exact_target_database_readiness_v1()'),
    ('exact_target', 'public.starring_runtime_exact_target_reader_database_identity_v1()'),
    ('exact_target', 'public.starring_runtime_exact_target_read_v1(text,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text)'),
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

DO $guard$
DECLARE
    actual_system_identifier TEXT;
    role_entry RECORD;
    role_oid OID;
BEGIN
    IF pg_catalog.current_setting('starring.runtime_enable', TRUE)
        NOT IN ('on', 'off')
    THEN
        RAISE EXCEPTION 'runtime role mode must be on or off'
            USING ERRCODE = '22023';
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
        )
    THEN
        RAISE EXCEPTION 'runtime capability role identities are invalid'
            USING ERRCODE = '22023';
    END IF;

    IF (
        SELECT pg_catalog.count(*)
        FROM pg_temp.starring_runtime_capability_functions
    ) <> 49
        OR EXISTS (
            SELECT 1
            FROM pg_temp.starring_runtime_capability_functions AS expected
            WHERE pg_catalog.to_regprocedure(expected.function_identity) IS NULL
        )
    THEN
        RAISE EXCEPTION 'runtime capability function manifest is unavailable'
            USING ERRCODE = '55000';
    END IF;

    FOR role_entry IN
        SELECT role_name
        FROM pg_temp.starring_runtime_capability_roles
        ORDER BY capability
    LOOP
        role_oid := pg_catalog.to_regrole(role_entry.role_name::TEXT);
        IF role_oid IS NOT NULL
            AND (
                EXISTS (
                    SELECT 1
                    FROM pg_catalog.pg_auth_members AS membership
                    WHERE membership.member = role_oid
                        OR membership.roleid = role_oid
                )
                OR EXISTS (
                    SELECT 1
                    FROM pg_catalog.pg_shdepend AS dependency
                    WHERE dependency.refclassid
                            = 'pg_catalog.pg_authid'::REGCLASS
                        AND dependency.refobjid = role_oid
                        AND dependency.deptype = 'o'
                )
            )
        THEN
            RAISE EXCEPTION 'runtime capability role has external ownership or membership'
                USING ERRCODE = '55000';
        END IF;
    END LOOP;
END;
$guard$;

DO $configure$
DECLARE
    enable_roles BOOLEAN;
    role_entry RECORD;
    database_entry RECORD;
    schema_entry RECORD;
    function_entry RECORD;
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

        FOR role_entry IN
            SELECT capability, role_name
            FROM pg_temp.starring_runtime_capability_roles
            ORDER BY capability
        LOOP
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

            EXECUTE pg_catalog.format(
                'ALTER ROLE %I RESET ALL',
                role_entry.role_name
            );

            FOR database_entry IN
                SELECT datname
                FROM pg_catalog.pg_database
                ORDER BY datname
            LOOP
                EXECUTE pg_catalog.format(
                    'ALTER ROLE %I IN DATABASE %I RESET ALL',
                    role_entry.role_name,
                    database_entry.datname
                );
                EXECUTE pg_catalog.format(
                    'REVOKE ALL PRIVILEGES ON DATABASE %I FROM %I CASCADE',
                    database_entry.datname,
                    role_entry.role_name
                );
            END LOOP;

            FOR schema_entry IN
                SELECT namespace.nspname
                FROM pg_catalog.pg_namespace AS namespace
                WHERE namespace.nspname <> 'information_schema'
                    AND pg_catalog.left(namespace.nspname, 3) <> 'pg_'
                ORDER BY namespace.nspname
            LOOP
                EXECUTE pg_catalog.format(
                    'REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA %I FROM %I CASCADE',
                    schema_entry.nspname,
                    role_entry.role_name
                );
                EXECUTE pg_catalog.format(
                    'REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA %I FROM %I CASCADE',
                    schema_entry.nspname,
                    role_entry.role_name
                );
                EXECUTE pg_catalog.format(
                    'REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA %I FROM %I CASCADE',
                    schema_entry.nspname,
                    role_entry.role_name
                );
                EXECUTE pg_catalog.format(
                    'REVOKE ALL PRIVILEGES ON SCHEMA %I FROM %I CASCADE',
                    schema_entry.nspname,
                    role_entry.role_name
                );
            END LOOP;

            FOR large_object_entry IN
                SELECT oid
                FROM pg_catalog.pg_largeobject_metadata
                ORDER BY oid
            LOOP
                EXECUTE pg_catalog.format(
                    'REVOKE ALL PRIVILEGES ON LARGE OBJECT %s FROM %I CASCADE',
                    large_object_entry.oid,
                    role_entry.role_name
                );
            END LOOP;

            FOR parameter_entry IN
                SELECT parameter_acl.parname
                FROM pg_catalog.pg_parameter_acl AS parameter_acl
                CROSS JOIN LATERAL pg_catalog.aclexplode(
                    parameter_acl.paracl
                ) AS privilege
                WHERE privilege.grantee
                    = pg_catalog.to_regrole(role_entry.role_name::TEXT)
                ORDER BY parameter_acl.parname
            LOOP
                EXECUTE pg_catalog.format(
                    'REVOKE ALL PRIVILEGES ON PARAMETER %I FROM %I',
                    parameter_entry.parname,
                    role_entry.role_name
                );
            END LOOP;

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
                SELECT function_identity
                FROM pg_temp.starring_runtime_capability_functions
                WHERE capability = role_entry.capability
                ORDER BY function_identity
            LOOP
                EXECUTE pg_catalog.format(
                    'GRANT EXECUTE ON FUNCTION %s TO %I',
                    function_entry.function_identity,
                    role_entry.role_name
                );
            END LOOP;
        END LOOP;
    ELSE
        FOR role_entry IN
            SELECT role_name
            FROM pg_temp.starring_runtime_capability_roles
            ORDER BY capability
        LOOP
            IF NOT EXISTS (
                SELECT 1
                FROM pg_catalog.pg_authid AS role
                WHERE role.rolname = role_entry.role_name
                    AND NOT role.rolsuper
                    AND NOT role.rolcreatedb
                    AND NOT role.rolcreaterole
                    AND NOT role.rolinherit
                    AND NOT role.rolreplication
                    AND NOT role.rolbypassrls
                    AND role.rolconnlimit = 4
                    AND role.rolvaliduntil
                        IS NOT DISTINCT FROM 'infinity'::TIMESTAMPTZ
                    AND role.rolpassword LIKE 'SCRAM-SHA-256$%'
            ) THEN
                RAISE EXCEPTION 'runtime capability role password preflight failed'
                    USING ERRCODE = '55000';
            END IF;

            EXECUTE pg_catalog.format(
                'ALTER ROLE %I LOGIN',
                role_entry.role_name
            );
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
                    AND role.rolcanlogin IS NOT DISTINCT FROM enable_roles
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
        THEN
            RAISE EXCEPTION 'runtime capability function privileges are invalid'
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

COMMIT;
