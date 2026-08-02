\set ON_ERROR_STOP on

\if :{?expected_database}
\else
\echo 'expected_database is required'
\quit 3
\endif

\if :{?expected_system_identifier}
\else
\echo 'expected_system_identifier is required'
\quit 3
\endif

\if :{?runtime_dedicated_cluster_acknowledgement}
\else
\echo 'runtime_dedicated_cluster_acknowledgement is required'
\quit 3
\endif

BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY;

SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';
SET LOCAL idle_in_transaction_session_timeout = '60s';
SET LOCAL search_path = pg_catalog;

SELECT (
    pg_catalog.current_setting('server_version_num')::INTEGER
        BETWEEN 160000 AND 169999
    AND pg_catalog.current_database() = 'starring_runtime_staging'
    AND pg_catalog.current_database() = :'expected_database'
    AND current_user = session_user
    AND current_user = 'starring_cluster_admin'
    AND EXISTS (
        SELECT 1
        FROM pg_catalog.pg_authid AS role
        WHERE role.rolname = current_user
            AND role.rolsuper
            AND role.rolcanlogin
    )
    AND (
        SELECT control.system_identifier::TEXT
        FROM pg_catalog.pg_control_system() AS control
    ) = :'expected_system_identifier'
    AND :'runtime_dedicated_cluster_acknowledgement' = pg_catalog.format(
        'starring-runtime-dedicated-staging-cluster-v2:%s:starring_runtime_staging:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation',
        (
            SELECT control.system_identifier::TEXT
            FROM pg_catalog.pg_control_system() AS control
        )
    )
) AS effect_inspection_target_valid
\gset

\if :effect_inspection_target_valid
\else
\echo 'runtime interaction effect inspection target validation failed'
\quit 3
\endif

SELECT (
    pg_catalog.to_regclass('public._sqlx_migrations') IS NOT NULL
    AND pg_catalog.to_regclass(
        'public.runtime_interaction_effect_heads_v1'
    ) IS NOT NULL
    AND pg_catalog.to_regclass(
        'public.runtime_interaction_effect_events_v1'
    ) IS NOT NULL
    AND pg_catalog.to_regprocedure(
        'public.starring_runtime_interaction_effect_schema_manifest_v1()'
    ) IS NOT NULL
) AS effect_inspection_schema_available
\gset

\if :effect_inspection_schema_available
\else
\echo 'runtime interaction effect inspection schema is unavailable'
\quit 3
\endif

SELECT (
    ledger.observed_count = 119
    AND ledger.observed_digest =
        '0cc4481ac9cdd2bb54b6d3e48253fd96faa7773633995cc4c777c84c3b386b88'
    AND EXISTS (
        SELECT 1
        FROM public._sqlx_migrations AS migration
        WHERE migration.version = 202608020002
            AND migration.success
            AND migration.checksum = pg_catalog.decode(
                '61ac941862c11f0aaa3cce54a2842ffadf4e5897c39f6796d2c6874e987a9f1e9d4ba6dd3dbc332f569c20d25831d769',
                'hex'
            )
    )
) AS effect_inspection_ledger_valid
FROM (
    SELECT pg_catalog.count(*) AS observed_count,
        pg_catalog.encode(
            pg_catalog.sha256(
                pg_catalog.convert_to(
                    pg_catalog.string_agg(
                        pg_catalog.concat_ws(
                            ':',
                            migration.version::TEXT,
                            CASE
                                WHEN migration.success THEN 'true'
                                ELSE 'false'
                            END,
                            pg_catalog.encode(migration.checksum, 'hex')
                        ),
                        E'\n'
                        ORDER BY migration.version
                    ),
                    'UTF8'
                )
            ),
            'hex'
        ) AS observed_digest
    FROM public._sqlx_migrations AS migration
) AS ledger
\gset

\if :effect_inspection_ledger_valid
\else
\echo 'runtime interaction effect inspection migration ledger validation failed'
\quit 3
\endif

SELECT public.starring_runtime_interaction_effect_schema_manifest_v1()
    AS effect_inspection_schema_valid
\gset

\if :effect_inspection_schema_valid
\else
\echo 'runtime interaction effect inspection schema validation failed'
\quit 3
\endif

WITH blocked_heads AS (
    SELECT head.application_id,
        head.interaction_id,
        head.action_index,
        head.action_kind,
        head.head_revision
    FROM public.runtime_interaction_effect_heads_v1 AS head
    WHERE head.state = 'recovery_required'
), terminal_events AS (
    SELECT head.action_kind,
        event.event_kind,
        event.to_state,
        event.outcome_code,
        event.observed_at
    FROM blocked_heads AS head
    LEFT JOIN public.runtime_interaction_effect_events_v1 AS event
        ON event.application_id = head.application_id
        AND event.interaction_id = head.interaction_id
        AND event.action_index = head.action_index
        AND event.event_revision = head.head_revision
)
SELECT NOT EXISTS (
    SELECT 1
    FROM terminal_events AS terminal
    WHERE terminal.event_kind IS DISTINCT FROM 'recovery_required'
        OR terminal.to_state IS DISTINCT FROM 'recovery_required'
        OR terminal.outcome_code IS NULL
        OR terminal.observed_at IS NULL
        OR terminal.action_kind NOT IN (
            'create_role',
            'create_channel',
            'grant_role',
            'upsert_overwrite',
            'post_panel',
            'register_instance',
            'teardown_instance',
            'edit_response'
        )
        OR terminal.outcome_code NOT IN (
            'recovery_blocked_discord_read_rejected',
            'recovery_blocked_response_token_unavailable',
            'recovery_blocked_observation_protocol',
            'recovery_blocked_compensation_conflict',
            'recovery_blocked_compensation_unsupported',
            'recovery_blocked_non_compensable',
            'recovery_blocked_internal_conflict',
            'recovery_blocked_discord_forbidden',
            'recovery_blocked_internal_authority',
            'recovery_blocked_attempt_budget_exhausted'
        )
) AS effect_inspection_projection_valid
\gset

\if :effect_inspection_projection_valid
\else
\echo 'runtime interaction effect inspection projection validation failed'
\quit 3
\endif

WITH blocked_heads AS (
    SELECT head.application_id,
        head.interaction_id,
        head.action_index,
        head.action_kind,
        head.head_revision
    FROM public.runtime_interaction_effect_heads_v1 AS head
    WHERE head.state = 'recovery_required'
), terminal_events AS (
    SELECT head.action_kind,
        event.outcome_code,
        event.observed_at
    FROM blocked_heads AS head
    INNER JOIN public.runtime_interaction_effect_events_v1 AS event
        ON event.application_id = head.application_id
        AND event.interaction_id = head.interaction_id
        AND event.action_index = head.action_index
        AND event.event_revision = head.head_revision
)
SELECT terminal.outcome_code AS recovery_block_code,
    terminal.action_kind,
    pg_catalog.count(*) AS blocked_effect_count,
    pg_catalog.min(terminal.observed_at) AS oldest_blocked_at,
    pg_catalog.max(terminal.observed_at) AS newest_blocked_at
FROM terminal_events AS terminal
GROUP BY terminal.outcome_code, terminal.action_kind
ORDER BY terminal.outcome_code COLLATE "C", terminal.action_kind COLLATE "C";

COMMIT;
