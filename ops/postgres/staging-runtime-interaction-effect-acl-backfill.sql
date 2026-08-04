\set ON_ERROR_STOP on

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

BEGIN;

SET LOCAL starring.expected_staging_database = :'expected_database';
SET LOCAL starring.expected_staging_system_identifier = :'expected_system_identifier';
SET LOCAL starring.runtime_dedicated_cluster_acknowledgement =
    :'runtime_dedicated_cluster_acknowledgement';

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended(
        pg_catalog.format(
            'starring-runtime-interaction-effect-acl-backfill-v1:%s:%s',
            :'expected_database',
            :'expected_system_identifier'
        ),
        0
    )
) AS lock_acquired
\gset effect_acl_backfill_

\unset effect_acl_backfill_lock_acquired

CREATE TEMP TABLE starring_runtime_interaction_effect_acl_manifest (
    function_identity TEXT PRIMARY KEY,
    interaction_executable BOOLEAN NOT NULL
) ON COMMIT DROP;

INSERT INTO pg_temp.starring_runtime_interaction_effect_acl_manifest (
    function_identity,
    interaction_executable
)
VALUES
    ('public.guard_runtime_interaction_effect_event_v1()', FALSE),
    ('public.guard_runtime_interaction_effect_head_v1()', FALSE),
    ('public.guard_runtime_interaction_effect_response_token_delete_v1()', FALSE),
    ('public.guard_runtime_interaction_effect_rollback_v1()', FALSE),
    ('public.guard_runtime_interaction_effect_root_v1()', FALSE),
    ('public.starring_runtime_interaction_effect_compensation_finish_v1(text,text,bigint,bigint,bigint,text,bytea,text,bytea,bigint)', TRUE),
    ('public.starring_runtime_interaction_effect_compensation_intend_v1(text,text,bigint,bigint,text,text,text,bigint,bigint,bigint,bytea,bytea,bigint)', TRUE),
    ('public.starring_runtime_interaction_effect_complete_receipt_v1(text,text,text,bytea,timestamp with time zone)', FALSE),
    ('public.starring_runtime_interaction_effect_finish_v1(text,text,bigint,bigint,text,bytea,bigint,bigint,bytea,text,text)', TRUE),
    ('public.starring_runtime_interaction_effect_intend_v1(text,text,bigint,bigint,text,bytea,bigint,bigint,bytea,bytea,bytea,jsonb,bytea,jsonb,bigint)', TRUE),
    ('public.starring_runtime_interaction_effect_plan_bind_v1(text,text,bigint,bigint,text,bytea,bytea,bytea,jsonb)', TRUE),
    ('public.starring_runtime_interaction_effect_receipt_terminal_sync_v1()', FALSE),
    ('public.starring_runtime_interaction_effect_reconcile_v1(text,text,bigint,bigint,bigint,text,text,text,bigint,bigint,bigint,text,text,bytea,text,bytea,text,bigint)', TRUE),
    ('public.starring_runtime_interaction_effect_recovery_claim_v1(text,text,bigint,bigint,text,text,text,bigint,bigint,bigint,bigint)', TRUE),
    ('public.starring_runtime_interaction_effect_require_rollback_v1(text,text,text,timestamp with time zone)', FALSE),
    ('public.starring_runtime_interaction_effect_resolve_receipt_v1(text,text,bytea,boolean)', FALSE),
    ('public.starring_runtime_interaction_effect_response_tail_claim_v1(text,text,bigint,bigint,text,text,text,bigint,bigint,bigint,bytea,bytea,bytea,bigint)', TRUE),
    ('public.starring_runtime_interaction_effect_response_tail_finalize_v1(text,text,bigint,bigint,text,bigint,bigint,text,text,text,bigint,bigint,bigint,bytea,bytea,text,bytea,bytea,bigint)', TRUE),
    ('public.starring_runtime_interaction_effect_response_tail_scan_v1(timestamp with time zone,text,text,bigint,timestamp with time zone,text,text,bigint,bigint)', TRUE),
    ('public.starring_runtime_interaction_effect_scan_recoverable_v1(timestamp with time zone,text,text,bigint,timestamp with time zone,text,text,bigint,bigint)', TRUE),
    ('public.starring_runtime_interaction_effect_schema_manifest_v1()', FALSE),
    ('public.starring_runtime_interaction_effect_try_complete_rollback_v1(text,text,timestamp with time zone)', FALSE);

CREATE TEMP TABLE starring_runtime_interaction_effect_role_snapshot
ON COMMIT DROP
AS
SELECT role.oid,
    role.rolname,
    role.rolsuper,
    role.rolinherit,
    role.rolcreaterole,
    role.rolcreatedb,
    role.rolcanlogin,
    role.rolreplication,
    role.rolconnlimit,
    role.rolpassword,
    role.rolvaliduntil,
    role.rolbypassrls
FROM pg_catalog.pg_authid AS role
WHERE role.rolname IN (
    'starring_owner',
    'starring_runtime_interaction'
)
ORDER BY role.rolname;

CREATE TEMP TABLE starring_runtime_interaction_effect_role_setting_snapshot
ON COMMIT DROP
AS
SELECT setting.setdatabase,
    setting.setrole,
    setting.setconfig
FROM pg_catalog.pg_db_role_setting AS setting
WHERE setting.setrole IN (
    pg_catalog.to_regrole('starring_owner'),
    pg_catalog.to_regrole('starring_runtime_interaction')
)
ORDER BY setting.setrole, setting.setdatabase;

DO $preflight$
DECLARE
    actual_system_identifier TEXT;
    common_owner OID := pg_catalog.to_regrole('starring_owner');
    interaction_role OID := pg_catalog.to_regrole(
        'starring_runtime_interaction'
    );
    ledger_count BIGINT;
    ledger_digest TEXT;
BEGIN
    IF pg_catalog.current_setting('server_version_num')::INTEGER
        NOT BETWEEN 160000 AND 169999
    THEN
        RAISE EXCEPTION 'effect ACL backfill requires PostgreSQL 16'
            USING ERRCODE = '55000';
    END IF;

    IF pg_catalog.current_database() <> 'starring_runtime_staging'
        OR pg_catalog.current_database() IS DISTINCT FROM pg_catalog.current_setting(
            'starring.expected_staging_database'
        )
    THEN
        RAISE EXCEPTION 'effect ACL backfill database acknowledgement is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT control.system_identifier::TEXT
    INTO actual_system_identifier
    FROM pg_catalog.pg_control_system() AS control;

    IF actual_system_identifier IS DISTINCT FROM pg_catalog.current_setting(
            'starring.expected_staging_system_identifier'
        )
        OR pg_catalog.current_setting(
            'starring.runtime_dedicated_cluster_acknowledgement'
        ) IS DISTINCT FROM pg_catalog.format(
            'starring-runtime-dedicated-staging-cluster-v2:%s:starring_runtime_staging:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation',
            actual_system_identifier
        )
    THEN
        RAISE EXCEPTION 'effect ACL backfill cluster acknowledgement is invalid'
            USING ERRCODE = '55000';
    END IF;

    IF current_user <> session_user
        OR current_user <> 'starring_cluster_admin'
        OR NOT EXISTS (
            SELECT 1
            FROM pg_catalog.pg_authid AS role
            WHERE role.rolname = current_user
                AND role.rolsuper
                AND role.rolcanlogin
        )
    THEN
        RAISE EXCEPTION 'effect ACL backfill requires the fixed cluster administrator'
            USING ERRCODE = '42501';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM pg_catalog.pg_stat_activity AS activity
        WHERE activity.pid <> pg_catalog.pg_backend_pid()
            AND activity.backend_type = 'client backend'
    ) OR EXISTS (
        SELECT 1
        FROM pg_catalog.pg_prepared_xacts
    ) THEN
        RAISE EXCEPTION 'effect ACL backfill requires cluster quiescence'
            USING ERRCODE = '55000';
    END IF;

    IF pg_catalog.to_regclass('public._sqlx_migrations') IS NULL THEN
        RAISE EXCEPTION 'effect ACL backfill migration ledger is unavailable'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*),
        pg_catalog.encode(
            pg_catalog.sha256(
                pg_catalog.convert_to(
                    pg_catalog.string_agg(
                        pg_catalog.concat_ws(
                            ':',
                            migration.version::TEXT,
                            CASE WHEN migration.success THEN 'true' ELSE 'false' END,
                            pg_catalog.encode(migration.checksum, 'hex')
                        ),
                        E'\n'
                        ORDER BY migration.version
                    ),
                    'UTF8'
                )
            ),
            'hex'
        )
    INTO ledger_count, ledger_digest
    FROM public._sqlx_migrations AS migration;

    IF ledger_count <> 125
        OR ledger_digest <>
            'ce8f2072d44c9f245972816f4485dda45898400fbc7acb8b9bdd742f9897a7e5'
        OR NOT EXISTS (
            SELECT 1
            FROM public._sqlx_migrations AS migration
            WHERE migration.version = 202608040004
                AND migration.success
                AND migration.checksum = pg_catalog.decode(
                    '2ac0c69bfa9bd5f99c092bdf1d8ac06510bc0c467c8a17cd62a0412f3f409a1128d4afbe5ca2136b77c34eadd91c3056',
                    'hex'
                )
        )
    THEN
        RAISE EXCEPTION 'effect ACL backfill migration ledger is not exact'
            USING ERRCODE = '55000';
    END IF;

    IF (
        SELECT pg_catalog.count(*)
        FROM pg_temp.starring_runtime_interaction_effect_acl_manifest
    ) <> 22
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_temp.starring_runtime_interaction_effect_acl_manifest
            WHERE interaction_executable
        ) <> 11
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_temp.starring_runtime_interaction_effect_role_snapshot
        ) <> 2
    THEN
        RAISE EXCEPTION 'effect ACL backfill fixed manifest is invalid'
            USING ERRCODE = '55000';
    END IF;

    IF common_owner IS NULL
        OR interaction_role IS NULL
        OR NOT EXISTS (
            SELECT 1
            FROM pg_catalog.pg_authid AS role
            WHERE role.oid = common_owner
                AND NOT role.rolsuper
                AND NOT role.rolinherit
                AND NOT role.rolcreaterole
                AND NOT role.rolcreatedb
                AND NOT role.rolcanlogin
                AND NOT role.rolreplication
                AND role.rolconnlimit = 0
                AND role.rolpassword IS NULL
                AND role.rolvaliduntil IS NOT DISTINCT FROM 'infinity'::TIMESTAMPTZ
                AND NOT role.rolbypassrls
        )
        OR NOT EXISTS (
            SELECT 1
            FROM pg_catalog.pg_authid AS role
            WHERE role.oid = interaction_role
                AND NOT role.rolsuper
                AND NOT role.rolinherit
                AND NOT role.rolcreaterole
                AND NOT role.rolcreatedb
                AND role.rolcanlogin
                AND NOT role.rolreplication
                AND role.rolconnlimit = 4
                AND role.rolpassword LIKE 'SCRAM-SHA-256$%'
                AND role.rolvaliduntil IS NOT DISTINCT FROM 'infinity'::TIMESTAMPTZ
                AND NOT role.rolbypassrls
        )
    THEN
        RAISE EXCEPTION 'effect ACL backfill role attributes are invalid'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM pg_catalog.pg_auth_members AS membership
        WHERE membership.roleid IN (common_owner, interaction_role)
            OR membership.member IN (common_owner, interaction_role)
    ) OR EXISTS (
        SELECT 1
        FROM pg_catalog.pg_db_role_setting AS setting
        WHERE setting.setrole IN (common_owner, interaction_role)
    ) OR EXISTS (
        SELECT 1
        FROM pg_catalog.pg_shdepend AS dependency
        WHERE dependency.refclassid = 'pg_catalog.pg_authid'::REGCLASS
            AND dependency.refobjid = interaction_role
            AND dependency.deptype = 'o'
    ) THEN
        RAISE EXCEPTION 'effect ACL backfill role isolation is invalid'
            USING ERRCODE = '55000';
    END IF;

    IF pg_catalog.to_regclass('public.automation_instances') IS NULL
        OR (
            SELECT relation.relowner
            FROM pg_catalog.pg_class AS relation
            WHERE relation.oid = 'public.automation_instances'::REGCLASS
        ) <> common_owner
        OR EXISTS (
            SELECT 1
            FROM pg_temp.starring_runtime_interaction_effect_acl_manifest AS expected
            LEFT JOIN pg_catalog.pg_proc AS function_row
                ON function_row.oid = pg_catalog.to_regprocedure(
                    expected.function_identity
                )
            WHERE function_row.oid IS NULL
                OR function_row.proowner <> common_owner
                OR function_row.prokind <> 'f'
        )
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_proc AS function_row
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = function_row.pronamespace
            WHERE namespace.nspname = 'public'
                AND (
                    function_row.proname LIKE
                        'starring_runtime_interaction_effect_%'
                    OR function_row.proname LIKE
                        'guard_runtime_interaction_effect_%'
                )
        ) <> 22
        OR NOT public.starring_runtime_interaction_effect_schema_manifest_v1()
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
    THEN
        RAISE EXCEPTION 'effect ACL backfill ownership or schema is invalid'
            USING ERRCODE = '55000';
    END IF;
END;
$preflight$;

DO $apply_acl$
DECLARE
    function_entry RECORD;
    grantee_entry RECORD;
BEGIN
    FOR function_entry IN
        SELECT expected.function_identity,
            expected.interaction_executable,
            function_row.oid,
            owner_role.rolname AS owner_name
        FROM pg_temp.starring_runtime_interaction_effect_acl_manifest AS expected
        INNER JOIN pg_catalog.pg_proc AS function_row
            ON function_row.oid = pg_catalog.to_regprocedure(
                expected.function_identity
            )
        INNER JOIN pg_catalog.pg_roles AS owner_role
            ON owner_role.oid = function_row.proowner
        ORDER BY expected.function_identity
    LOOP
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE',
            function_entry.function_identity
        );

        FOR grantee_entry IN
            SELECT DISTINCT privilege.grantee,
                grantee_role.rolname AS grantee_name
            FROM pg_catalog.pg_proc AS function_row
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            LEFT JOIN pg_catalog.pg_roles AS grantee_role
                ON grantee_role.oid = privilege.grantee
            WHERE function_row.oid = function_entry.oid
                AND privilege.grantee <> function_row.proowner
            ORDER BY privilege.grantee
        LOOP
            IF grantee_entry.grantee = 0
                OR grantee_entry.grantee_name IS NULL
            THEN
                RAISE EXCEPTION 'effect ACL backfill grantee is invalid'
                    USING ERRCODE = '55000';
            END IF;
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM %I CASCADE',
                function_entry.function_identity,
                grantee_entry.grantee_name
            );
        END LOOP;

        EXECUTE pg_catalog.format(
            'GRANT EXECUTE ON FUNCTION %s TO %I',
            function_entry.function_identity,
            function_entry.owner_name
        );

        IF function_entry.interaction_executable THEN
            EXECUTE pg_catalog.format(
                'GRANT EXECUTE ON FUNCTION %s TO starring_runtime_interaction',
                function_entry.function_identity
            );
        END IF;
    END LOOP;
END;
$apply_acl$;

DO $postflight$
DECLARE
    common_owner OID := pg_catalog.to_regrole('starring_owner');
    interaction_role OID := pg_catalog.to_regrole(
        'starring_runtime_interaction'
    );
BEGIN
    IF EXISTS (
        SELECT 1
        FROM pg_temp.starring_runtime_interaction_effect_acl_manifest AS expected
        INNER JOIN pg_catalog.pg_proc AS function_row
            ON function_row.oid = pg_catalog.to_regprocedure(
                expected.function_identity
            )
        CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
            function_row.proacl,
            pg_catalog.acldefault('f', function_row.proowner)
        )) AS privilege
        GROUP BY function_row.oid,
            function_row.proowner,
            expected.interaction_executable
        HAVING pg_catalog.count(*) <>
                CASE WHEN expected.interaction_executable THEN 2 ELSE 1 END
            OR pg_catalog.count(*) FILTER (
                WHERE privilege.grantee = common_owner
                    AND privilege.grantor = common_owner
                    AND privilege.privilege_type = 'EXECUTE'
                    AND NOT privilege.is_grantable
            ) <> 1
            OR pg_catalog.count(*) FILTER (
                WHERE privilege.grantee = interaction_role
                    AND privilege.grantor = common_owner
                    AND privilege.privilege_type = 'EXECUTE'
                    AND NOT privilege.is_grantable
            ) <> CASE WHEN expected.interaction_executable THEN 1 ELSE 0 END
    ) OR EXISTS (
        SELECT 1
        FROM pg_temp.starring_runtime_interaction_effect_acl_manifest AS expected
        WHERE pg_catalog.has_function_privilege(
                interaction_role,
                pg_catalog.to_regprocedure(expected.function_identity),
                'EXECUTE'
            ) IS DISTINCT FROM expected.interaction_executable
            OR pg_catalog.has_function_privilege(
                interaction_role,
                pg_catalog.to_regprocedure(expected.function_identity),
                'EXECUTE WITH GRANT OPTION'
            )
    ) THEN
        RAISE EXCEPTION 'effect ACL backfill topology postflight failed'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        (
            SELECT *
            FROM pg_temp.starring_runtime_interaction_effect_role_snapshot
            EXCEPT ALL
            SELECT role.oid,
                role.rolname,
                role.rolsuper,
                role.rolinherit,
                role.rolcreaterole,
                role.rolcreatedb,
                role.rolcanlogin,
                role.rolreplication,
                role.rolconnlimit,
                role.rolpassword,
                role.rolvaliduntil,
                role.rolbypassrls
            FROM pg_catalog.pg_authid AS role
            WHERE role.rolname IN (
                'starring_owner',
                'starring_runtime_interaction'
            )
        )
        UNION ALL
        (
            SELECT role.oid,
                role.rolname,
                role.rolsuper,
                role.rolinherit,
                role.rolcreaterole,
                role.rolcreatedb,
                role.rolcanlogin,
                role.rolreplication,
                role.rolconnlimit,
                role.rolpassword,
                role.rolvaliduntil,
                role.rolbypassrls
            FROM pg_catalog.pg_authid AS role
            WHERE role.rolname IN (
                'starring_owner',
                'starring_runtime_interaction'
            )
            EXCEPT ALL
            SELECT *
            FROM pg_temp.starring_runtime_interaction_effect_role_snapshot
        )
    ) OR EXISTS (
        (
            SELECT *
            FROM pg_temp.starring_runtime_interaction_effect_role_setting_snapshot
            EXCEPT ALL
            SELECT setting.setdatabase,
                setting.setrole,
                setting.setconfig
            FROM pg_catalog.pg_db_role_setting AS setting
            WHERE setting.setrole IN (
                common_owner,
                interaction_role
            )
        )
        UNION ALL
        (
            SELECT setting.setdatabase,
                setting.setrole,
                setting.setconfig
            FROM pg_catalog.pg_db_role_setting AS setting
            WHERE setting.setrole IN (
                common_owner,
                interaction_role
            )
            EXCEPT ALL
            SELECT *
            FROM pg_temp.starring_runtime_interaction_effect_role_setting_snapshot
        )
    ) THEN
        RAISE EXCEPTION 'effect ACL backfill changed role authentication state'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM pg_catalog.pg_stat_activity AS activity
        WHERE activity.pid <> pg_catalog.pg_backend_pid()
            AND activity.backend_type = 'client backend'
    ) OR EXISTS (
        SELECT 1
        FROM pg_catalog.pg_prepared_xacts
    ) THEN
        RAISE EXCEPTION 'effect ACL backfill lost cluster quiescence'
            USING ERRCODE = '55000';
    END IF;
END;
$postflight$;

DO $final_role_proof$
BEGIN
    IF EXISTS (
        (
            SELECT *
            FROM pg_temp.starring_runtime_interaction_effect_role_snapshot
            EXCEPT ALL
            SELECT role.oid,
                role.rolname,
                role.rolsuper,
                role.rolinherit,
                role.rolcreaterole,
                role.rolcreatedb,
                role.rolcanlogin,
                role.rolreplication,
                role.rolconnlimit,
                role.rolpassword,
                role.rolvaliduntil,
                role.rolbypassrls
            FROM pg_catalog.pg_authid AS role
            WHERE role.rolname IN (
                'starring_owner',
                'starring_runtime_interaction'
            )
        )
        UNION ALL
        (
            SELECT role.oid,
                role.rolname,
                role.rolsuper,
                role.rolinherit,
                role.rolcreaterole,
                role.rolcreatedb,
                role.rolcanlogin,
                role.rolreplication,
                role.rolconnlimit,
                role.rolpassword,
                role.rolvaliduntil,
                role.rolbypassrls
            FROM pg_catalog.pg_authid AS role
            WHERE role.rolname IN (
                'starring_owner',
                'starring_runtime_interaction'
            )
            EXCEPT ALL
            SELECT *
            FROM pg_temp.starring_runtime_interaction_effect_role_snapshot
        )
    ) OR EXISTS (
        (
            SELECT *
            FROM pg_temp.starring_runtime_interaction_effect_role_setting_snapshot
            EXCEPT ALL
            SELECT setting.setdatabase,
                setting.setrole,
                setting.setconfig
            FROM pg_catalog.pg_db_role_setting AS setting
            WHERE setting.setrole IN (
                pg_catalog.to_regrole('starring_owner'),
                pg_catalog.to_regrole('starring_runtime_interaction')
            )
        )
        UNION ALL
        (
            SELECT setting.setdatabase,
                setting.setrole,
                setting.setconfig
            FROM pg_catalog.pg_db_role_setting AS setting
            WHERE setting.setrole IN (
                pg_catalog.to_regrole('starring_owner'),
                pg_catalog.to_regrole('starring_runtime_interaction')
            )
            EXCEPT ALL
            SELECT *
            FROM pg_temp.starring_runtime_interaction_effect_role_setting_snapshot
        )
    ) OR EXISTS (
        SELECT 1
        FROM pg_catalog.pg_stat_activity AS activity
        WHERE activity.pid <> pg_catalog.pg_backend_pid()
            AND activity.backend_type = 'client backend'
    ) OR EXISTS (
        SELECT 1
        FROM pg_catalog.pg_prepared_xacts
    ) THEN
        RAISE EXCEPTION 'effect ACL backfill final proof failed'
            USING ERRCODE = '55000';
    END IF;
END;
$final_role_proof$;

DROP TABLE pg_temp.starring_runtime_interaction_effect_acl_manifest,
    pg_temp.starring_runtime_interaction_effect_role_snapshot,
    pg_temp.starring_runtime_interaction_effect_role_setting_snapshot;

SET SESSION AUTHORIZATION starring_runtime_interaction;

SELECT 1 / CASE
    WHEN pg_catalog.count(*) = 1
        AND pg_catalog.bool_and(
            readiness.database_identity
                ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
            AND readiness.database_name = 'starring_runtime_staging'
            AND readiness.executor_role = 'starring_runtime_interaction'
            AND readiness.checked_at IS NOT NULL
        )
    THEN 1
    ELSE 0
END AS readiness_proof
FROM public.starring_runtime_interaction_database_readiness_v1() AS readiness
\gset effect_acl_backfill_

RESET SESSION AUTHORIZATION;

\unset effect_acl_backfill_readiness_proof

DO $final_quiescence_proof$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM pg_catalog.pg_stat_activity AS activity
        WHERE activity.pid <> pg_catalog.pg_backend_pid()
            AND activity.backend_type = 'client backend'
    ) OR EXISTS (
        SELECT 1
        FROM pg_catalog.pg_prepared_xacts
    ) THEN
        RAISE EXCEPTION 'effect ACL backfill final quiescence proof failed'
            USING ERRCODE = '55000';
    END IF;
END;
$final_quiescence_proof$;

COMMIT;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
RESET idle_in_transaction_session_timeout;
