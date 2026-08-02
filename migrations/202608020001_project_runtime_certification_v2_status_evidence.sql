SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended(
        'starring-product-deployment-status-evidence-v3',
        0
    )
);

LOCK TABLE
    public.runtime_deployments,
    public.runtime_attestations,
    public.runtime_certification_operations_v2
IN ACCESS SHARE MODE;

CREATE TEMP TABLE starring_product_deployment_status_acl_transfer (
    capability_kind TEXT NOT NULL,
    source_function TEXT NOT NULL,
    target_function TEXT NOT NULL,
    identity_function TEXT NOT NULL,
    grantee OID NOT NULL,
    grantor OID NOT NULL,
    privilege_type TEXT NOT NULL,
    is_grantable BOOLEAN NOT NULL
) ON COMMIT DROP;

INSERT INTO pg_temp.starring_product_deployment_status_acl_transfer (
    capability_kind,
    source_function,
    target_function,
    identity_function,
    grantee,
    grantor,
    privilege_type,
    is_grantable
)
SELECT
    expected.capability_kind,
    expected.source_function,
    expected.target_function,
    expected.identity_function,
    privilege.grantee,
    privilege.grantor,
    privilege.privilege_type,
    privilege.is_grantable
FROM (
    VALUES
        (
            'basic',
            'public.starring_product_deployment_status_read_v1(text,text,text,text,text,text,text,text,bytea)',
            'public.starring_product_deployment_status_read_v3(text,text,text,text,text,text,text,text,bytea)',
            'public.starring_product_deployment_status_reader_database_identity_v1()'
        ),
        (
            'operational',
            'public.starring_product_deployment_status_read_v2(text,text,text,text,text,text,text,text,bytea)',
            'public.starring_product_operational_deployment_status_read_v3(text,text,text,text,text,text,text,text,bytea)',
            'public.starring_product_deployment_status_reader_database_identity_v2()'
        )
) AS expected(
    capability_kind,
    source_function,
    target_function,
    identity_function
)
INNER JOIN pg_catalog.pg_proc AS function_row
    ON function_row.oid = pg_catalog.to_regprocedure(
        expected.source_function
    )
CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
    function_row.proacl,
    pg_catalog.acldefault('f', function_row.proowner)
)) AS privilege
WHERE privilege.grantee <> function_row.proowner;

DO $preflight$
DECLARE
    common_owner OID;
    operation_relation OID := pg_catalog.to_regclass(
        'public.runtime_certification_operations_v2'
    );
    mutation_function OID := pg_catalog.to_regprocedure(
        'public.reject_runtime_certification_reservation_mutation_v2()'
    );
    invalid_column_count BIGINT;
    invalid_trigger_count BIGINT;
    new_function_collision_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    SELECT pg_catalog.count(*)
    INTO new_function_collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_product_deployment_status_read_core_v3',
            'starring_product_deployment_status_read_v3',
            'starring_product_operational_deployment_status_read_v3'
        );

    SELECT pg_catalog.count(*)
    INTO invalid_column_count
    FROM (
        VALUES
            ('runtime_deployments', 'revision', 'bigint', TRUE),
            ('runtime_deployments', 'convergence_attempt_no', 'bigint', TRUE),
            ('runtime_deployments', 'last_failure_attempt_no', 'bigint', FALSE),
            ('runtime_deployments', 'last_controller_id', 'text', FALSE),
            ('runtime_attestations', 'record_format_version', 'smallint', TRUE),
            ('runtime_attestations', 'serving_lease_duration_nanos', 'bigint', TRUE),
            ('runtime_attestations', 'convergence_attempt_no', 'bigint', TRUE),
            ('runtime_attestations', 'v2_operation_id', 'text', FALSE),
            ('runtime_attestations', 'v2_intent_fingerprint', 'text', FALSE),
            ('runtime_attestations', 'v2_request_digest', 'text', FALSE),
            ('runtime_attestations', 'v2_request_bytes', 'bytea', FALSE),
            ('runtime_attestations', 'v2_live_attestation_bytes', 'bytea', FALSE),
            ('runtime_attestations', 'v2_must_commit_before', 'timestamp with time zone', FALSE),
            ('runtime_attestations', 'v2_route_admission', 'jsonb', FALSE),
            ('runtime_attestations', 'v2_route_incarnation', 'bigint', FALSE),
            ('runtime_attestations', 'v2_route_activation_sequence', 'bigint', FALSE),
            ('runtime_attestations', 'v2_initial_lease_epoch', 'bigint', FALSE),
            ('runtime_attestations', 'v2_initial_serving_revision', 'bigint', FALSE),
            ('runtime_attestations', 'v2_prepared_snapshot', 'jsonb', FALSE),
            ('runtime_attestations', 'v2_certified_snapshot', 'jsonb', FALSE),
            ('runtime_certification_operations_v2', 'operation_id', 'text', TRUE),
            ('runtime_certification_operations_v2', 'tenant_id', 'text', TRUE),
            ('runtime_certification_operations_v2', 'installation_id', 'text', TRUE),
            ('runtime_certification_operations_v2', 'deployment_id', 'text', TRUE),
            ('runtime_certification_operations_v2', 'deployment_revision', 'bigint', TRUE),
            ('runtime_certification_operations_v2', 'convergence_attempt_no', 'bigint', TRUE),
            ('runtime_certification_operations_v2', 'certification_intent_bytes', 'bytea', TRUE),
            ('runtime_certification_operations_v2', 'intent_fingerprint', 'text', TRUE)
    ) AS expected(relation_name, column_name, data_type, not_null)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = pg_catalog.to_regclass(
            'public.' || expected.relation_name
        )
    LEFT JOIN pg_catalog.pg_attribute AS attribute
        ON attribute.attrelid = relation.oid
        AND attribute.attname = expected.column_name
        AND attribute.attnum > 0
        AND NOT attribute.attisdropped
    WHERE attribute.attrelid IS NULL
        OR pg_catalog.format_type(
            attribute.atttypid,
            attribute.atttypmod
        ) <> expected.data_type
        OR attribute.attnotnull <> expected.not_null;

    SELECT pg_catalog.count(*)
    INTO invalid_trigger_count
    FROM (
        VALUES
            ('runtime_certification_operations_v2_reject_row_mutation', 31::SMALLINT),
            ('runtime_certification_operations_v2_reject_truncate', 34::SMALLINT)
    ) AS expected(trigger_name, trigger_type)
    LEFT JOIN pg_catalog.pg_trigger AS trigger_row
        ON trigger_row.tgrelid = operation_relation
        AND trigger_row.tgname = expected.trigger_name
    WHERE trigger_row.oid IS NULL
        OR trigger_row.tgfoid <> mutation_function
        OR trigger_row.tgtype <> expected.trigger_type
        OR trigger_row.tgenabled <> 'O'
        OR trigger_row.tgisinternal;

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR new_function_collision_count <> 0
        OR invalid_column_count <> 0
        OR EXISTS (
            SELECT 1
            FROM (
                VALUES
                    ('public.starring_product_deployment_status_reader_database_identity_v1()'),
                    ('public.starring_product_deployment_status_reader_database_identity_v2()'),
                    ('public.starring_product_deployment_status_read_v1(text,text,text,text,text,text,text,text,bytea)'),
                    ('public.starring_product_deployment_status_read_v2(text,text,text,text,text,text,text,text,bytea)')
            ) AS expected(identity)
            LEFT JOIN pg_catalog.pg_proc AS function_row
                ON function_row.oid = pg_catalog.to_regprocedure(
                    expected.identity
                )
            WHERE function_row.oid IS NULL
                OR function_row.proowner <> common_owner
                OR function_row.prokind <> 'f'
        )
        OR EXISTS (
            SELECT 1
            FROM pg_temp.starring_product_deployment_status_acl_transfer
                AS transfer
            WHERE transfer.grantee = 0
                OR transfer.grantor <> common_owner
                OR transfer.privilege_type <> 'EXECUTE'
                OR transfer.is_grantable
                OR NOT EXISTS (
                    SELECT 1
                    FROM pg_catalog.pg_roles AS role
                    WHERE role.oid = transfer.grantee
                )
        )
        OR EXISTS (
            SELECT 1
            FROM pg_temp.starring_product_deployment_status_acl_transfer
                AS transfer
            GROUP BY transfer.capability_kind
            HAVING pg_catalog.count(*) > 1
        )
        OR (
            (
                SELECT pg_catalog.count(*)
                FROM pg_temp.starring_product_deployment_status_acl_transfer
            ) = 2
            AND (
                SELECT pg_catalog.count(DISTINCT transfer.grantee)
                FROM pg_temp.starring_product_deployment_status_acl_transfer
                    AS transfer
            ) <> 2
        )
        OR EXISTS (
            SELECT 1
            FROM pg_temp.starring_product_deployment_status_acl_transfer
                AS transfer
            WHERE NOT EXISTS (
                SELECT 1
                FROM pg_catalog.pg_proc AS function_row
                CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE function_row.oid = pg_catalog.to_regprocedure(
                        transfer.identity_function
                    )
                    AND privilege.grantee = transfer.grantee
                    AND privilege.grantor = common_owner
                    AND privilege.privilege_type = 'EXECUTE'
                    AND NOT privilege.is_grantable
            )
        )
        OR operation_relation IS NULL
        OR mutation_function IS NULL
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_attribute AS attribute
            WHERE attribute.attrelid = operation_relation
                AND attribute.attnum > 0
                AND NOT attribute.attisdropped
        ) <> 8
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_class AS relation
            WHERE relation.oid IN (
                    pg_catalog.to_regclass('public.runtime_deployments'),
                    pg_catalog.to_regclass('public.runtime_attestations'),
                    operation_relation
                )
                AND (
                    relation.relowner <> common_owner
                    OR relation.relkind <> 'r'
                    OR relation.relpersistence <> 'p'
                    OR relation.relrowsecurity
                    OR relation.relforcerowsecurity
                )
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_class AS relation
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                relation.relacl,
                pg_catalog.acldefault('r', relation.relowner)
            )) AS privilege
            WHERE relation.oid = operation_relation
                AND privilege.grantee <> common_owner
        )
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_trigger AS trigger_row
            WHERE trigger_row.tgrelid = operation_relation
                AND NOT trigger_row.tgisinternal
        ) <> 2
        OR invalid_trigger_count <> 0
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_proc AS function_row
            INNER JOIN pg_catalog.pg_language AS language_row
                ON language_row.oid = function_row.prolang
            WHERE function_row.oid = mutation_function
                AND (
                    function_row.proowner <> common_owner
                    OR function_row.prokind <> 'f'
                    OR function_row.prorettype <> 'trigger'::pg_catalog.regtype
                    OR function_row.provolatile <> 'v'
                    OR function_row.proisstrict
                    OR function_row.proparallel <> 'u'
                    OR NOT function_row.prosecdef
                    OR function_row.proconfig
                        IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
                    OR language_row.lanname <> 'plpgsql'
                )
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_proc AS function_row
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE function_row.oid = mutation_function
                AND privilege.privilege_type = 'EXECUTE'
                AND privilege.grantee <> common_owner
        )
        OR NOT EXISTS (
            SELECT 1
            FROM pg_catalog.pg_proc AS function_row
            INNER JOIN pg_catalog.pg_language AS language_row
                ON language_row.oid = function_row.prolang
            WHERE function_row.oid = pg_catalog.to_regprocedure(
                    'public.starring_product_deployment_status_read_core_v2(text,text,text,text,text,text,text,text,bytea)'
                )
                AND function_row.proowner = common_owner
                AND function_row.prokind = 'f'
                AND function_row.provolatile = 'v'
                AND function_row.proisstrict
                AND function_row.proparallel = 'u'
                AND function_row.prosecdef
                AND function_row.proretset
                AND function_row.prorows = 1::REAL
                AND function_row.proconfig
                    IS NOT DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
                AND language_row.lanname = 'sql'
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_v2_status_evidence_preflight_drift';
    END IF;
END;
$preflight$;

CREATE FUNCTION public.starring_product_deployment_status_read_core_v3(
    expected_deployment_id TEXT,
    expected_promotion_id TEXT,
    expected_desired_target_digest TEXT,
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_guild_id TEXT,
    expected_principal_id TEXT,
    expected_acting_discord_user_id TEXT,
    expected_product_session_digest BYTEA
)
RETURNS TABLE(
    request_outcome TEXT,
    deployment_projection JSONB,
    activation_projection JSONB,
    promotion_projection JSONB,
    tenant_lifecycle_state TEXT,
    installation_projection JSONB,
    historical_authority_projection JSONB,
    current_authority_projection JSONB,
    active_target_version BIGINT,
    artifact_projection JSONB,
    attestation_projection JSONB,
    serving_projection JSONB,
    database_now TIMESTAMPTZ,
    deployment_convergence_attempt_no BIGINT,
    deployment_last_failure_attempt_no BIGINT,
    attestation_convergence_attempt_no BIGINT,
    attestation_record_format_version SMALLINT,
    attestation_serving_lease_duration_nanos BIGINT,
    deployment_last_controller_id TEXT,
    v2_evidence_state TEXT,
    v2_operation_id TEXT,
    v2_intent_fingerprint TEXT,
    v2_certification_intent_bytes BYTEA,
    v2_request_digest TEXT,
    v2_request_bytes BYTEA,
    v2_live_attestation_bytes BYTEA,
    v2_must_commit_before TIMESTAMPTZ,
    v2_route_admission JSONB,
    v2_certified_snapshot JSONB
)
LANGUAGE sql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
    WITH base AS MATERIALIZED (
        SELECT status.*
        FROM public.starring_product_deployment_status_read_core_v2(
            expected_deployment_id => expected_deployment_id,
            expected_promotion_id => expected_promotion_id,
            expected_desired_target_digest => expected_desired_target_digest,
            expected_tenant_id => expected_tenant_id,
            expected_installation_id => expected_installation_id,
            expected_guild_id => expected_guild_id,
            expected_principal_id => expected_principal_id,
            expected_acting_discord_user_id => expected_acting_discord_user_id,
            expected_product_session_digest => expected_product_session_digest
        ) AS status
        LIMIT 2
    ), exact_deployment AS MATERIALIZED (
        SELECT deployment.*
        FROM base
        INNER JOIN public.runtime_deployments AS deployment
            ON base.request_outcome = 'exact'
            AND deployment.deployment_id = expected_deployment_id
            AND deployment.promotion_id = expected_promotion_id
            AND deployment.desired_target_digest = expected_desired_target_digest
            AND deployment.tenant_id = expected_tenant_id
            AND deployment.installation_id = expected_installation_id
            AND deployment.guild_id = expected_guild_id
            AND deployment.revision = (
                base.deployment_projection #>> '{row,revision}'
            )::BIGINT
            AND deployment.convergence_attempt_no
                IS NOT DISTINCT FROM base.deployment_convergence_attempt_no
            AND deployment.last_failure_attempt_no
                IS NOT DISTINCT FROM base.deployment_last_failure_attempt_no
        LIMIT 2
    ), exact_attestation AS MATERIALIZED (
        SELECT attestation.*
        FROM base
        INNER JOIN exact_deployment AS deployment ON TRUE
        INNER JOIN public.runtime_attestations AS attestation
            ON base.attestation_projection IS NOT NULL
            AND deployment.phase = 'live'
            AND attestation.attestation_id =
                base.attestation_projection #>> '{row,attestation_id}'
            AND attestation.attestation_id = deployment.live_attestation_id
            AND attestation.tenant_id = deployment.tenant_id
            AND attestation.installation_id = deployment.installation_id
            AND attestation.deployment_id = deployment.deployment_id
            AND attestation.deployment_revision = (
                base.attestation_projection #>> '{row,deployment_revision}'
            )::BIGINT
            AND attestation.convergence_attempt_no
                IS NOT DISTINCT FROM base.attestation_convergence_attempt_no
        LIMIT 2
    ), exact_operation AS MATERIALIZED (
        SELECT operation.*
        FROM exact_deployment AS deployment
        INNER JOIN exact_attestation AS attestation
            ON attestation.record_format_version = 2
        INNER JOIN public.runtime_certification_operations_v2 AS operation
            ON operation.operation_id = attestation.v2_operation_id
            AND operation.intent_fingerprint = attestation.v2_intent_fingerprint
            AND operation.tenant_id = attestation.tenant_id
            AND operation.installation_id = attestation.installation_id
            AND operation.deployment_id = attestation.deployment_id
            AND operation.deployment_revision
                = attestation.deployment_revision - 1
            AND attestation.deployment_revision = deployment.revision
            AND operation.convergence_attempt_no
                = attestation.convergence_attempt_no
            AND attestation.convergence_attempt_no
                = deployment.convergence_attempt_no
        LIMIT 2
    ), classified AS MATERIALIZED (
        SELECT
            base.*,
            deployment.deployment_id AS exact_deployment_id,
            deployment.last_controller_id,
            attestation.attestation_id AS exact_attestation_id,
            attestation.record_format_version,
            attestation.serving_lease_duration_nanos,
            attestation.v2_operation_id AS attestation_v2_operation_id,
            attestation.v2_intent_fingerprint AS attestation_v2_intent_fingerprint,
            attestation.v2_request_digest,
            attestation.v2_request_bytes,
            attestation.v2_live_attestation_bytes,
            attestation.v2_must_commit_before,
            attestation.v2_route_admission,
            attestation.v2_certified_snapshot,
            operation.operation_id AS exact_operation_id,
            operation.intent_fingerprint AS exact_intent_fingerprint,
            operation.certification_intent_bytes,
            CASE
                WHEN base.request_outcome <> 'exact' THEN NULL
                WHEN deployment.deployment_id IS NULL THEN 'invalid'
                WHEN base.attestation_projection IS NULL THEN 'no_attestation'
                WHEN attestation.attestation_id IS NULL THEN 'invalid'
                WHEN attestation.record_format_version = 1
                    AND attestation.v2_operation_id IS NULL
                    AND attestation.v2_intent_fingerprint IS NULL
                    AND attestation.v2_request_digest IS NULL
                    AND attestation.v2_request_bytes IS NULL
                    AND attestation.v2_live_attestation_bytes IS NULL
                    AND attestation.v2_must_commit_before IS NULL
                    AND attestation.v2_route_admission IS NULL
                    AND attestation.v2_route_incarnation IS NULL
                    AND attestation.v2_route_activation_sequence IS NULL
                    AND attestation.v2_initial_lease_epoch IS NULL
                    AND attestation.v2_initial_serving_revision IS NULL
                    AND attestation.v2_prepared_snapshot IS NULL
                    AND attestation.v2_certified_snapshot IS NULL
                    THEN 'v1'
                WHEN attestation.record_format_version = 2
                    AND operation.operation_id IS NOT NULL
                    AND attestation.v2_operation_id IS NOT NULL
                    AND attestation.v2_intent_fingerprint IS NOT NULL
                    AND attestation.v2_request_digest IS NOT NULL
                    AND attestation.v2_request_bytes IS NOT NULL
                    AND attestation.v2_live_attestation_bytes IS NOT NULL
                    AND attestation.v2_must_commit_before IS NOT NULL
                    AND attestation.v2_route_admission IS NOT NULL
                    AND attestation.v2_route_incarnation IS NOT NULL
                    AND attestation.v2_route_activation_sequence IS NOT NULL
                    AND attestation.v2_initial_lease_epoch IS NOT NULL
                    AND attestation.v2_initial_serving_revision IS NOT NULL
                    AND attestation.v2_prepared_snapshot IS NOT NULL
                    AND attestation.v2_certified_snapshot IS NOT NULL
                    THEN 'exact'
                ELSE 'invalid'
            END AS evidence_state
        FROM base
        LEFT JOIN exact_deployment AS deployment ON TRUE
        LEFT JOIN exact_attestation AS attestation ON TRUE
        LEFT JOIN exact_operation AS operation ON TRUE
    )
    SELECT
        classified.request_outcome,
        classified.deployment_projection,
        classified.activation_projection,
        classified.promotion_projection,
        classified.tenant_lifecycle_state,
        classified.installation_projection,
        classified.historical_authority_projection,
        classified.current_authority_projection,
        classified.active_target_version,
        classified.artifact_projection,
        classified.attestation_projection,
        classified.serving_projection,
        classified.database_now,
        classified.deployment_convergence_attempt_no,
        classified.deployment_last_failure_attempt_no,
        classified.attestation_convergence_attempt_no,
        CASE WHEN classified.request_outcome = 'exact' THEN
            classified.record_format_version
        END,
        CASE WHEN classified.request_outcome = 'exact' THEN
            classified.serving_lease_duration_nanos
        END,
        CASE WHEN classified.request_outcome = 'exact' THEN
            classified.last_controller_id
        END,
        classified.evidence_state,
        CASE WHEN classified.evidence_state = 'exact' THEN
            classified.exact_operation_id
        END,
        CASE WHEN classified.evidence_state = 'exact' THEN
            classified.exact_intent_fingerprint
        END,
        CASE WHEN classified.evidence_state = 'exact' THEN
            classified.certification_intent_bytes
        END,
        CASE WHEN classified.evidence_state = 'exact' THEN
            classified.v2_request_digest
        END,
        CASE WHEN classified.evidence_state = 'exact' THEN
            classified.v2_request_bytes
        END,
        CASE WHEN classified.evidence_state = 'exact' THEN
            classified.v2_live_attestation_bytes
        END,
        CASE WHEN classified.evidence_state = 'exact' THEN
            classified.v2_must_commit_before
        END,
        CASE WHEN classified.evidence_state = 'exact' THEN
            classified.v2_route_admission
        END,
        CASE WHEN classified.evidence_state = 'exact' THEN
            classified.v2_certified_snapshot
        END
    FROM classified
    LIMIT 2;
$function$;

CREATE FUNCTION public.starring_product_deployment_status_read_v3(
    expected_deployment_id TEXT,
    expected_promotion_id TEXT,
    expected_desired_target_digest TEXT,
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_guild_id TEXT,
    expected_principal_id TEXT,
    expected_acting_discord_user_id TEXT,
    expected_product_session_digest BYTEA
)
RETURNS TABLE(
    request_outcome TEXT,
    deployment_projection JSONB,
    activation_projection JSONB,
    promotion_projection JSONB,
    tenant_lifecycle_state TEXT,
    installation_projection JSONB,
    historical_authority_projection JSONB,
    current_authority_projection JSONB,
    active_target_version BIGINT,
    artifact_projection JSONB,
    attestation_projection JSONB,
    serving_projection JSONB,
    database_now TIMESTAMPTZ,
    attestation_record_format_version SMALLINT,
    attestation_serving_lease_duration_nanos BIGINT,
    attestation_convergence_attempt_no BIGINT,
    deployment_last_controller_id TEXT,
    v2_evidence_state TEXT,
    v2_operation_id TEXT,
    v2_intent_fingerprint TEXT,
    v2_certification_intent_bytes BYTEA,
    v2_request_digest TEXT,
    v2_request_bytes BYTEA,
    v2_live_attestation_bytes BYTEA,
    v2_must_commit_before TIMESTAMPTZ,
    v2_route_admission JSONB,
    v2_certified_snapshot JSONB
)
LANGUAGE sql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
    SELECT
        status.request_outcome,
        status.deployment_projection,
        status.activation_projection,
        status.promotion_projection,
        status.tenant_lifecycle_state,
        status.installation_projection,
        status.historical_authority_projection,
        status.current_authority_projection,
        status.active_target_version,
        status.artifact_projection,
        status.attestation_projection,
        status.serving_projection,
        status.database_now,
        status.attestation_record_format_version,
        status.attestation_serving_lease_duration_nanos,
        status.attestation_convergence_attempt_no,
        status.deployment_last_controller_id,
        status.v2_evidence_state,
        status.v2_operation_id,
        status.v2_intent_fingerprint,
        status.v2_certification_intent_bytes,
        status.v2_request_digest,
        status.v2_request_bytes,
        status.v2_live_attestation_bytes,
        status.v2_must_commit_before,
        status.v2_route_admission,
        status.v2_certified_snapshot
    FROM public.starring_product_deployment_status_read_core_v3(
        expected_deployment_id => expected_deployment_id,
        expected_promotion_id => expected_promotion_id,
        expected_desired_target_digest => expected_desired_target_digest,
        expected_tenant_id => expected_tenant_id,
        expected_installation_id => expected_installation_id,
        expected_guild_id => expected_guild_id,
        expected_principal_id => expected_principal_id,
        expected_acting_discord_user_id => expected_acting_discord_user_id,
        expected_product_session_digest => expected_product_session_digest
    ) AS status
    LIMIT 2;
$function$;

CREATE FUNCTION public.starring_product_operational_deployment_status_read_v3(
    expected_deployment_id TEXT,
    expected_promotion_id TEXT,
    expected_desired_target_digest TEXT,
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_guild_id TEXT,
    expected_principal_id TEXT,
    expected_acting_discord_user_id TEXT,
    expected_product_session_digest BYTEA
)
RETURNS TABLE(
    request_outcome TEXT,
    deployment_projection JSONB,
    activation_projection JSONB,
    promotion_projection JSONB,
    tenant_lifecycle_state TEXT,
    installation_projection JSONB,
    historical_authority_projection JSONB,
    current_authority_projection JSONB,
    active_target_version BIGINT,
    artifact_projection JSONB,
    attestation_projection JSONB,
    serving_projection JSONB,
    database_now TIMESTAMPTZ,
    deployment_convergence_attempt_no BIGINT,
    deployment_last_failure_attempt_no BIGINT,
    attestation_convergence_attempt_no BIGINT,
    attestation_record_format_version SMALLINT,
    attestation_serving_lease_duration_nanos BIGINT,
    deployment_last_controller_id TEXT,
    v2_evidence_state TEXT,
    v2_operation_id TEXT,
    v2_intent_fingerprint TEXT,
    v2_certification_intent_bytes BYTEA,
    v2_request_digest TEXT,
    v2_request_bytes BYTEA,
    v2_live_attestation_bytes BYTEA,
    v2_must_commit_before TIMESTAMPTZ,
    v2_route_admission JSONB,
    v2_certified_snapshot JSONB
)
LANGUAGE sql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
    SELECT status.*
    FROM public.starring_product_deployment_status_read_core_v3(
        expected_deployment_id => expected_deployment_id,
        expected_promotion_id => expected_promotion_id,
        expected_desired_target_digest => expected_desired_target_digest,
        expected_tenant_id => expected_tenant_id,
        expected_installation_id => expected_installation_id,
        expected_guild_id => expected_guild_id,
        expected_principal_id => expected_principal_id,
        expected_acting_discord_user_id => expected_acting_discord_user_id,
        expected_product_session_digest => expected_product_session_digest
    ) AS status
    LIMIT 2;
$function$;

DO $seal$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    function_identity TEXT;
    grantee OID;
    grantee_name NAME;
    transfer RECORD;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner IS NULL
        OR common_owner_name IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_v2_status_evidence_owner_drift';
    END IF;

    FOR function_identity IN
        SELECT expected.identity
        FROM (
            VALUES
                ('public.starring_product_deployment_status_read_core_v3(text,text,text,text,text,text,text,text,bytea)'),
                ('public.starring_product_deployment_status_read_v3(text,text,text,text,text,text,text,text,bytea)'),
                ('public.starring_product_operational_deployment_status_read_v3(text,text,text,text,text,text,text,text,bytea)')
        ) AS expected(identity)
    LOOP
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s OWNER TO %I',
            function_identity,
            common_owner_name
        );
    END LOOP;

    FOR function_identity IN
        SELECT expected.identity
        FROM (
            VALUES
                ('public.starring_product_deployment_status_read_v1(text,text,text,text,text,text,text,text,bytea)'),
                ('public.starring_product_deployment_status_read_v2(text,text,text,text,text,text,text,text,bytea)'),
                ('public.starring_product_deployment_status_read_core_v3(text,text,text,text,text,text,text,text,bytea)'),
                ('public.starring_product_deployment_status_read_v3(text,text,text,text,text,text,text,text,bytea)'),
                ('public.starring_product_operational_deployment_status_read_v3(text,text,text,text,text,text,text,text,bytea)')
        ) AS expected(identity)
    LOOP
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
            WHERE function_row.oid = pg_catalog.to_regprocedure(
                    function_identity
                )
                AND privilege.grantee <> 0
                AND privilege.grantee <> common_owner
        LOOP
            grantee_name := pg_catalog.pg_get_userbyid(grantee);
            IF grantee_name IS NULL THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RE001',
                    MESSAGE = 'runtime_certification_v2_status_evidence_grantee_drift';
            END IF;
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM %I CASCADE',
                function_identity,
                grantee_name
            );
        END LOOP;
    END LOOP;

    FOR transfer IN
        SELECT acl.target_function, role.rolname
        FROM pg_temp.starring_product_deployment_status_acl_transfer AS acl
        INNER JOIN pg_catalog.pg_roles AS role
            ON role.oid = acl.grantee
        ORDER BY acl.capability_kind
    LOOP
        EXECUTE pg_catalog.format(
            'GRANT EXECUTE ON FUNCTION %s TO %I',
            transfer.target_function,
            transfer.rolname
        );
    END LOOP;
END;
$seal$;

DO $postflight$
DECLARE
    common_owner OID;
    operation_relation OID := pg_catalog.to_regclass(
        'public.runtime_certification_operations_v2'
    );
    mutation_function OID := pg_catalog.to_regprocedure(
        'public.reject_runtime_certification_reservation_mutation_v2()'
    );
    invalid_function_count BIGINT;
    invalid_trigger_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_product_deployment_status_read_core_v3(text,text,text,text,text,text,text,text,bytea)',
                'TABLE(request_outcome text, deployment_projection jsonb, activation_projection jsonb, promotion_projection jsonb, tenant_lifecycle_state text, installation_projection jsonb, historical_authority_projection jsonb, current_authority_projection jsonb, active_target_version bigint, artifact_projection jsonb, attestation_projection jsonb, serving_projection jsonb, database_now timestamp with time zone, deployment_convergence_attempt_no bigint, deployment_last_failure_attempt_no bigint, attestation_convergence_attempt_no bigint, attestation_record_format_version smallint, attestation_serving_lease_duration_nanos bigint, deployment_last_controller_id text, v2_evidence_state text, v2_operation_id text, v2_intent_fingerprint text, v2_certification_intent_bytes bytea, v2_request_digest text, v2_request_bytes bytea, v2_live_attestation_bytes bytea, v2_must_commit_before timestamp with time zone, v2_route_admission jsonb, v2_certified_snapshot jsonb)'
            ),
            (
                'public.starring_product_deployment_status_read_v3(text,text,text,text,text,text,text,text,bytea)',
                'TABLE(request_outcome text, deployment_projection jsonb, activation_projection jsonb, promotion_projection jsonb, tenant_lifecycle_state text, installation_projection jsonb, historical_authority_projection jsonb, current_authority_projection jsonb, active_target_version bigint, artifact_projection jsonb, attestation_projection jsonb, serving_projection jsonb, database_now timestamp with time zone, attestation_record_format_version smallint, attestation_serving_lease_duration_nanos bigint, attestation_convergence_attempt_no bigint, deployment_last_controller_id text, v2_evidence_state text, v2_operation_id text, v2_intent_fingerprint text, v2_certification_intent_bytes bytea, v2_request_digest text, v2_request_bytes bytea, v2_live_attestation_bytes bytea, v2_must_commit_before timestamp with time zone, v2_route_admission jsonb, v2_certified_snapshot jsonb)'
            ),
            (
                'public.starring_product_operational_deployment_status_read_v3(text,text,text,text,text,text,text,text,bytea)',
                'TABLE(request_outcome text, deployment_projection jsonb, activation_projection jsonb, promotion_projection jsonb, tenant_lifecycle_state text, installation_projection jsonb, historical_authority_projection jsonb, current_authority_projection jsonb, active_target_version bigint, artifact_projection jsonb, attestation_projection jsonb, serving_projection jsonb, database_now timestamp with time zone, deployment_convergence_attempt_no bigint, deployment_last_failure_attempt_no bigint, attestation_convergence_attempt_no bigint, attestation_record_format_version smallint, attestation_serving_lease_duration_nanos bigint, deployment_last_controller_id text, v2_evidence_state text, v2_operation_id text, v2_intent_fingerprint text, v2_certification_intent_bytes bytea, v2_request_digest text, v2_request_bytes bytea, v2_live_attestation_bytes bytea, v2_must_commit_before timestamp with time zone, v2_route_admission jsonb, v2_certified_snapshot jsonb)'
            )
    ) AS expected(identity, result_identity)
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
        OR NOT function_row.proretset
        OR function_row.prorows <> 1::REAL
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.pronargdefaults <> 0
        OR pg_catalog.pg_get_function_identity_arguments(function_row.oid)
            <> 'expected_deployment_id text, expected_promotion_id text, expected_desired_target_digest text, expected_tenant_id text, expected_installation_id text, expected_guild_id text, expected_principal_id text, expected_acting_discord_user_id text, expected_product_session_digest bytea'
        OR pg_catalog.pg_get_function_result(function_row.oid)
            <> expected.result_identity
        OR language_row.lanname <> 'sql';

    SELECT pg_catalog.count(*)
    INTO invalid_trigger_count
    FROM (
        VALUES
            ('runtime_certification_operations_v2_reject_row_mutation', 31::SMALLINT),
            ('runtime_certification_operations_v2_reject_truncate', 34::SMALLINT)
    ) AS expected(trigger_name, trigger_type)
    LEFT JOIN pg_catalog.pg_trigger AS trigger_row
        ON trigger_row.tgrelid = operation_relation
        AND trigger_row.tgname = expected.trigger_name
    WHERE trigger_row.oid IS NULL
        OR trigger_row.tgfoid <> mutation_function
        OR trigger_row.tgtype <> expected.trigger_type
        OR trigger_row.tgenabled <> 'O'
        OR trigger_row.tgisinternal;

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR invalid_function_count <> 0
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_proc AS function_row
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = function_row.pronamespace
            WHERE namespace.nspname = 'public'
                AND function_row.proname IN (
                    'starring_product_deployment_status_read_core_v3',
                    'starring_product_deployment_status_read_v3',
                    'starring_product_operational_deployment_status_read_v3'
                )
        ) <> 3
        OR EXISTS (
            SELECT 1
            FROM (
                VALUES
                    ('public.starring_product_deployment_status_read_v1(text,text,text,text,text,text,text,text,bytea)', NULL::TEXT),
                    ('public.starring_product_deployment_status_read_v2(text,text,text,text,text,text,text,text,bytea)', NULL::TEXT),
                    ('public.starring_product_deployment_status_read_core_v3(text,text,text,text,text,text,text,text,bytea)', NULL::TEXT),
                    ('public.starring_product_deployment_status_read_v3(text,text,text,text,text,text,text,text,bytea)', 'basic'),
                    ('public.starring_product_operational_deployment_status_read_v3(text,text,text,text,text,text,text,text,bytea)', 'operational')
            ) AS expected(identity, capability_kind)
            INNER JOIN pg_catalog.pg_proc AS function_row
                ON function_row.oid = pg_catalog.to_regprocedure(
                    expected.identity
                )
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantor <> common_owner
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
                OR (
                    privilege.grantee <> common_owner
                    AND NOT EXISTS (
                        SELECT 1
                        FROM pg_temp.starring_product_deployment_status_acl_transfer
                            AS transfer
                        WHERE transfer.capability_kind
                                = expected.capability_kind
                            AND transfer.grantee = privilege.grantee
                    )
                )
        )
        OR EXISTS (
            SELECT 1
            FROM pg_temp.starring_product_deployment_status_acl_transfer
                AS transfer
            WHERE NOT EXISTS (
                SELECT 1
                FROM pg_catalog.pg_proc AS function_row
                CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE function_row.oid = pg_catalog.to_regprocedure(
                        transfer.target_function
                    )
                    AND privilege.grantee = transfer.grantee
                    AND privilege.grantor = common_owner
                    AND privilege.privilege_type = 'EXECUTE'
                    AND NOT privilege.is_grantable
            )
        )
        OR operation_relation IS NULL
        OR mutation_function IS NULL
        OR NOT EXISTS (
            SELECT 1
            FROM pg_catalog.pg_class AS relation
            WHERE relation.oid = operation_relation
                AND relation.relowner = common_owner
                AND relation.relkind = 'r'
                AND relation.relpersistence = 'p'
                AND NOT relation.relrowsecurity
                AND NOT relation.relforcerowsecurity
        )
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_attribute AS attribute
            WHERE attribute.attrelid = operation_relation
                AND attribute.attnum > 0
                AND NOT attribute.attisdropped
        ) <> 8
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_class AS relation
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                relation.relacl,
                pg_catalog.acldefault('r', relation.relowner)
            )) AS privilege
            WHERE relation.oid = operation_relation
                AND privilege.grantee <> common_owner
        )
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_trigger AS trigger_row
            WHERE trigger_row.tgrelid = operation_relation
                AND NOT trigger_row.tgisinternal
        ) <> 2
        OR invalid_trigger_count <> 0
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_proc AS function_row
            INNER JOIN pg_catalog.pg_language AS language_row
                ON language_row.oid = function_row.prolang
            WHERE function_row.oid = mutation_function
                AND (
                    function_row.proowner <> common_owner
                    OR function_row.prokind <> 'f'
                    OR function_row.prorettype <> 'trigger'::pg_catalog.regtype
                    OR function_row.provolatile <> 'v'
                    OR function_row.proisstrict
                    OR function_row.proparallel <> 'u'
                    OR NOT function_row.prosecdef
                    OR function_row.proconfig
                        IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
                    OR language_row.lanname <> 'plpgsql'
                )
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_proc AS function_row
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE function_row.oid = mutation_function
                AND privilege.privilege_type = 'EXECUTE'
                AND privilege.grantee <> common_owner
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_v2_status_evidence_postflight_drift';
    END IF;
END;
$postflight$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
