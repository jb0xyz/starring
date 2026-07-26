SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
);

LOCK TABLE
    public.runtime_writer_fence,
    public.runtime_deployments,
    public.runtime_product_operations_v2,
    public.runtime_drain_intents_v2
IN ACCESS EXCLUSIVE MODE;

CREATE TEMPORARY TABLE pg_temp.starring_runtime_first_apply_public_snapshot (
    function_oid OID PRIMARY KEY,
    function_owner OID NOT NULL,
    function_acl ACLITEM[]
) ON COMMIT DROP;

INSERT INTO pg_temp.starring_runtime_first_apply_public_snapshot (
    function_oid,
    function_owner,
    function_acl
)
SELECT
    function_row.oid,
    function_row.proowner,
    function_row.proacl
FROM pg_catalog.pg_proc AS function_row
INNER JOIN pg_catalog.pg_namespace AS namespace
    ON namespace.oid = function_row.pronamespace
WHERE namespace.nspname = 'public';

CREATE TEMPORARY TABLE pg_temp.starring_runtime_first_apply_private_snapshot (
    function_oid OID PRIMARY KEY,
    function_owner OID NOT NULL,
    function_acl ACLITEM[]
) ON COMMIT DROP;

INSERT INTO pg_temp.starring_runtime_first_apply_private_snapshot (
    function_oid,
    function_owner,
    function_acl
)
SELECT
    function_row.oid,
    function_row.proowner,
    function_row.proacl
FROM pg_catalog.pg_proc AS function_row
INNER JOIN pg_catalog.pg_namespace AS namespace
    ON namespace.oid = function_row.pronamespace
WHERE namespace.nspname = 'starring_runtime_private_v2';

CREATE TEMPORARY TABLE pg_temp.starring_runtime_first_apply_capability (
    function_identity TEXT PRIMARY KEY
) ON COMMIT DROP;

INSERT INTO pg_temp.starring_runtime_first_apply_capability (
    function_identity
)
VALUES
    ('public.starring_runtime_execution_database_readiness_v1()'),
    ('public.starring_runtime_execution_database_identity_v1()'),
    ('public.starring_runtime_execution_claim_next_v1(text,bigint)'),
    ('public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)'),
    ('public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)'),
    ('public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)'),
    ('public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)'),
    ('public.starring_runtime_execution_recover_stale_live_v1()'),
    ('public.starring_runtime_observe_previous_serving_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,jsonb)'),
    ('public.starring_runtime_gateway_owner_observe_v1(text)'),
    ('public.starring_runtime_gateway_owner_acquire_v1(text,text,text,bigint)'),
    ('public.starring_runtime_gateway_owner_renew_v1(text,text,bigint,text,bigint,bigint)'),
    ('public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)'),
    ('public.starring_runtime_writer_fence_observe_v1()'),
    ('public.starring_runtime_product_drain_observe_v2(text,text,text,bigint,text,text)');

DO $preflight$
DECLARE
    common_owner OID;
    executor_grantee OID;
    external_executor_count BIGINT;
    invalid_capability_acl_count BIGINT;
    invalid_owner_only_acl_count BIGINT;
    invalid_relation_count BIGINT;
    invalid_schema_count BIGINT;
    invalid_private_function_count BIGINT;
    trigger_digest TEXT;
    manifest_digest TEXT;
    readiness_digest TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT
        pg_catalog.min(privilege.grantee::BIGINT)::OID,
        pg_catalog.count(*)
    INTO executor_grantee, external_executor_count
    FROM pg_catalog.pg_proc AS function_row
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_database_identity_v1()'
        )
        AND privilege.grantee <> common_owner;

    SELECT pg_catalog.count(*)
    INTO invalid_capability_acl_count
    FROM pg_temp.starring_runtime_first_apply_capability AS expected
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(
            expected.function_identity
        )
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
        ) <> CASE WHEN executor_grantee IS NULL THEN 1 ELSE 2 END
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee = common_owner
        ) <> 1
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
        ) <> CASE WHEN executor_grantee IS NULL THEN 0 ELSE 1 END
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE (
                    privilege.grantee <> common_owner
                    AND (
                        executor_grantee IS NULL
                        OR privilege.grantee <> executor_grantee
                    )
                )
                OR privilege.grantor <> common_owner
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
        );

    SELECT pg_catalog.count(*)
    INTO invalid_owner_only_acl_count
    FROM (
        VALUES
            ('public.starring_runtime_execution_schema_manifest_v1()'),
            ('public.reject_runtime_product_drain_mutation()')
    ) AS expected(function_identity)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(
            expected.function_identity
        )
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
        ) <> 1
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
    INTO invalid_relation_count
    FROM (
        VALUES
            ('public.runtime_writer_fence'),
            ('public.runtime_deployments'),
            ('public.runtime_product_operations_v2'),
            ('public.runtime_drain_intents_v2')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = pg_catalog.to_regclass(expected.identity)
    WHERE relation.oid IS NULL
        OR relation.relkind <> 'r'
        OR relation.relowner <> common_owner
        OR relation.relrowsecurity
        OR relation.relforcerowsecurity;

    SELECT pg_catalog.count(*)
    INTO invalid_schema_count
    FROM pg_catalog.pg_namespace AS namespace
    WHERE namespace.oid = pg_catalog.to_regnamespace(
            'starring_runtime_private_v2'
        )
        AND (
            namespace.nspowner <> common_owner
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.aclexplode(COALESCE(
                    namespace.nspacl,
                    pg_catalog.acldefault('n', namespace.nspowner)
                )) AS privilege
            ) <> 2
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    namespace.nspacl,
                    pg_catalog.acldefault('n', namespace.nspowner)
                )) AS privilege
                WHERE privilege.grantee <> common_owner
                    OR privilege.grantor <> common_owner
                    OR privilege.privilege_type NOT IN ('USAGE', 'CREATE')
                    OR privilege.is_grantable
            )
        );

    SELECT pg_catalog.count(*)
    INTO invalid_private_function_count
    FROM (
        VALUES
            ('starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(text)'),
            ('starring_runtime_private_v2.starring_runtime_framed_digest_v2(bytea,bytea)'),
            ('starring_runtime_private_v2.starring_runtime_product_mutation_bytes_v2(text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text)'),
            ('starring_runtime_private_v2.starring_runtime_product_mutation_digest_v2(bytea)'),
            ('starring_runtime_private_v2.starring_runtime_drain_intent_bytes_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text)'),
            ('starring_runtime_private_v2.starring_runtime_drain_intent_digest_v2(bytea)')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prosecdef
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
        ) <> 1
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

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO trigger_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.reject_runtime_product_drain_mutation()'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO manifest_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_schema_manifest_v1()'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO readiness_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_database_readiness_v1()'
    );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR external_executor_count > 1
        OR executor_grantee = 0
        OR invalid_capability_acl_count <> 0
        OR invalid_owner_only_acl_count <> 0
        OR invalid_relation_count <> 0
        OR pg_catalog.to_regnamespace(
            'starring_runtime_private_v2'
        ) IS NULL
        OR invalid_schema_count <> 0
        OR invalid_private_function_count <> 0
        OR pg_catalog.to_regprocedure(
            'starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text,bytea,text,bytea,text)'
        ) IS NOT NULL
        OR trigger_digest IS DISTINCT FROM
            '292a961a175397a7a145d78ae8a7211be8091eb68b3a5e73d4d44f514c9ebf8b'
        OR manifest_digest IS DISTINCT FROM
            '27ebe976c214377f71f62cf7d9c90be3009e3c331e395dff7d63c587513be167'
        OR readiness_digest IS DISTINCT FROM
            'c32a430e629c5603de09a15769b664bd533f3d4a86d5b26f514657ad63fc5eec'
        OR (SELECT pg_catalog.count(*) FROM public.runtime_writer_fence) <> 1
        OR NOT EXISTS (
            SELECT 1
            FROM public.runtime_writer_fence AS fence
            WHERE fence.singleton
                AND fence.fence_state IN ('open', 'closed')
        )
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_first_apply_preflight_drift';
    END IF;
END;
$preflight$;

CREATE OR REPLACE FUNCTION public.reject_runtime_product_drain_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    gate_stage TEXT;
    gate_product_operation_id TEXT;
    gate_drain_intent_id TEXT;
BEGIN
    gate_stage := pg_catalog.current_setting(
        'starring.runtime_product_drain_first_apply_stage_v2',
        TRUE
    );
    gate_product_operation_id := pg_catalog.current_setting(
        'starring.runtime_product_drain_first_apply_product_operation_id_v2',
        TRUE
    );
    gate_drain_intent_id := pg_catalog.current_setting(
        'starring.runtime_product_drain_first_apply_drain_intent_id_v2',
        TRUE
    );

    IF TG_OP = 'INSERT' THEN
        IF TG_RELID = pg_catalog.to_regclass(
                'public.runtime_product_operations_v2'
            )
        THEN
            IF gate_stage = 'product_insert'
                AND gate_product_operation_id = NEW.product_operation_id
                AND COALESCE(gate_drain_intent_id, '') = ''
            THEN
                PERFORM pg_catalog.set_config(
                    'starring.runtime_product_drain_first_apply_stage_v2',
                    '',
                    TRUE
                );
                RETURN NEW;
            END IF;
        ELSIF TG_RELID = pg_catalog.to_regclass(
                'public.runtime_drain_intents_v2'
            )
        THEN
            IF gate_stage = 'drain_insert'
                AND gate_product_operation_id = NEW.product_operation_id
                AND gate_drain_intent_id = NEW.drain_intent_id
            THEN
                PERFORM pg_catalog.set_config(
                    'starring.runtime_product_drain_first_apply_stage_v2',
                    '',
                    TRUE
                );
                PERFORM pg_catalog.set_config(
                    'starring.runtime_product_drain_first_apply_product_operation_id_v2',
                    '',
                    TRUE
                );
                PERFORM pg_catalog.set_config(
                    'starring.runtime_product_drain_first_apply_drain_intent_id_v2',
                    '',
                    TRUE
                );
                RETURN NEW;
            END IF;
        END IF;
    END IF;

    RAISE EXCEPTION USING
        ERRCODE = '23514',
        MESSAGE = 'runtime_product_drain_mutation_rejected';
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(
    requested_operation_id TEXT,
    requested_intent_id TEXT,
    requested_tenant_id TEXT,
    requested_installation_id TEXT,
    requested_deployment_id TEXT,
    requested_expected_revision BIGINT,
    requested_slot_guild_id TEXT,
    requested_slot_ruleset_key TEXT,
    requested_target_guild_id TEXT,
    requested_target_ruleset_key TEXT,
    requested_target_version BIGINT,
    requested_target_content_hash TEXT,
    requested_target_binding_revision BIGINT,
    requested_target_binding_fingerprint TEXT,
    requested_mutation_kind TEXT,
    requested_product_semantic_request_digest TEXT,
    requested_product_mutation_request_bytes BYTEA,
    requested_product_mutation_digest TEXT,
    requested_drain_intent_request_bytes BYTEA,
    requested_drain_intent_digest TEXT
)
RETURNS TABLE(
    outcome_name TEXT,
    locked_snapshot JSONB,
    observed_at TIMESTAMPTZ,
    product_tenant_id TEXT,
    product_installation_id TEXT,
    product_deployment_id TEXT,
    product_expected_revision BIGINT,
    product_operation_id TEXT,
    product_expected_target JSONB,
    product_mutation_request_bytes BYTEA,
    product_mutation_digest TEXT,
    drain_tenant_id TEXT,
    drain_installation_id TEXT,
    drain_deployment_id TEXT,
    drain_slot_guild_id TEXT,
    drain_slot_ruleset_key TEXT,
    drain_expected_revision BIGINT,
    drain_intent_id TEXT,
    drain_intent_request_bytes BYTEA,
    drain_intent_digest TEXT,
    intent_revision BIGINT,
    intent_state TEXT
)
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY INVOKER
SET search_path = pg_catalog, starring_runtime_private_v2
ROWS 1
AS $function$
DECLARE
    deployment_row public.runtime_deployments%ROWTYPE;
    product_row public.runtime_product_operations_v2%ROWTYPE;
    drain_row public.runtime_drain_intents_v2%ROWTYPE;
    foreign_product_count BIGINT;
    foreign_drain_count BIGINT;
    product_count BIGINT;
    drain_count BIGINT;
    writer_fence_state TEXT;
    expected_product_bytes BYTEA;
    expected_product_digest TEXT;
    expected_drain_bytes BYTEA;
    expected_drain_digest TEXT;
    stored_product_digest TEXT;
    stored_drain_digest TEXT;
    exception_schema_name TEXT;
    exception_table_name TEXT;
    exception_constraint_name TEXT;
BEGIN
    IF pg_catalog.current_setting('transaction_isolation')
            <> 'serializable'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE =
                'runtime_product_drain_first_apply_isolation_invalid';
    END IF;

    IF COALESCE(pg_catalog.current_setting(
            'starring.runtime_product_drain_first_apply_stage_v2',
            TRUE
        ), '') <> ''
        OR COALESCE(pg_catalog.current_setting(
            'starring.runtime_product_drain_first_apply_product_operation_id_v2',
            TRUE
        ), '') <> ''
        OR COALESCE(pg_catalog.current_setting(
            'starring.runtime_product_drain_first_apply_drain_intent_id_v2',
            TRUE
        ), '') <> ''
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE =
                'runtime_product_drain_first_apply_gate_precondition_invalid';
    END IF;

    expected_product_bytes :=
        starring_runtime_private_v2.starring_runtime_product_mutation_bytes_v2(
            requested_operation_id,
            requested_tenant_id,
            requested_installation_id,
            requested_deployment_id,
            requested_expected_revision,
            requested_slot_guild_id,
            requested_slot_ruleset_key,
            requested_target_guild_id,
            requested_target_ruleset_key,
            requested_target_version,
            requested_target_content_hash,
            requested_target_binding_revision,
            requested_target_binding_fingerprint,
            requested_mutation_kind,
            requested_product_semantic_request_digest
        );
    expected_product_digest :=
        starring_runtime_private_v2.starring_runtime_product_mutation_digest_v2(
            expected_product_bytes
        );
    expected_drain_bytes :=
        starring_runtime_private_v2.starring_runtime_drain_intent_bytes_v2(
            requested_intent_id,
            requested_operation_id,
            requested_tenant_id,
            requested_installation_id,
            requested_deployment_id,
            requested_expected_revision,
            requested_slot_guild_id,
            requested_slot_ruleset_key,
            requested_target_guild_id,
            requested_target_ruleset_key,
            requested_target_version,
            requested_target_content_hash,
            requested_target_binding_revision,
            requested_target_binding_fingerprint,
            requested_mutation_kind,
            requested_product_semantic_request_digest
        );
    expected_drain_digest :=
        starring_runtime_private_v2.starring_runtime_drain_intent_digest_v2(
            expected_drain_bytes
        );

    IF requested_product_mutation_request_bytes
            IS DISTINCT FROM expected_product_bytes
        OR requested_product_mutation_digest
            IS DISTINCT FROM expected_product_digest
        OR requested_drain_intent_request_bytes
            IS DISTINCT FROM expected_drain_bytes
        OR requested_drain_intent_digest
            IS DISTINCT FROM expected_drain_digest
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_product_drain_first_apply_input_invalid';
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock_shared(
        pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
    );

    SELECT fence.fence_state
    INTO writer_fence_state
    FROM public.runtime_writer_fence AS fence
    WHERE fence.singleton
    FOR SHARE;

    IF NOT FOUND
        OR (
            writer_fence_state IS DISTINCT FROM 'open'
            AND writer_fence_state IS DISTINCT FROM 'closed'
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE =
                'runtime_product_drain_first_apply_writer_fence_invalid';
    END IF;

    IF writer_fence_state = 'closed' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX005',
            MESSAGE =
                'runtime_product_drain_first_apply_writer_fenced';
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            pg_catalog.concat(
                'starring-runtime-serving-slot-v1:',
                requested_slot_guild_id,
                ':',
                requested_slot_ruleset_key
            ),
            0
        )
    );

    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = requested_tenant_id
        AND deployment.installation_id = requested_installation_id
        AND deployment.deployment_id = requested_deployment_id
        AND deployment.guild_id = requested_slot_guild_id
        AND deployment.ruleset_key = requested_slot_ruleset_key
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE =
                'runtime_product_drain_first_apply_deployment_mismatch';
    END IF;

    locked_snapshot := deployment_row.snapshot;
    observed_at := pg_catalog.clock_timestamp();

    SELECT pg_catalog.count(*)
    INTO product_count
    FROM public.runtime_product_operations_v2 AS product
    WHERE product.tenant_id = requested_tenant_id
        AND product.installation_id = requested_installation_id
        AND product.deployment_id = requested_deployment_id
        AND product.expected_revision = requested_expected_revision;

    IF product_count > 1 THEN
        outcome_name := 'persistence_corrupt';
        RETURN NEXT;
        RETURN;
    END IF;

    IF product_count = 1 THEN
        SELECT product.*
        INTO STRICT product_row
        FROM public.runtime_product_operations_v2 AS product
        WHERE product.tenant_id = requested_tenant_id
            AND product.installation_id = requested_installation_id
            AND product.deployment_id = requested_deployment_id
            AND product.expected_revision = requested_expected_revision
        FOR UPDATE;
    END IF;

    SELECT pg_catalog.count(*)
    INTO drain_count
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE drain.tenant_id = requested_tenant_id
        AND drain.installation_id = requested_installation_id
        AND drain.deployment_id = requested_deployment_id
        AND drain.slot_guild_id = requested_slot_guild_id
        AND drain.slot_ruleset_key = requested_slot_ruleset_key
        AND drain.expected_revision = requested_expected_revision;

    IF drain_count > 1 THEN
        outcome_name := 'persistence_corrupt';
        RETURN NEXT;
        RETURN;
    END IF;

    IF drain_count = 1 THEN
        SELECT drain.*
        INTO STRICT drain_row
        FROM public.runtime_drain_intents_v2 AS drain
        WHERE drain.tenant_id = requested_tenant_id
            AND drain.installation_id = requested_installation_id
            AND drain.deployment_id = requested_deployment_id
            AND drain.slot_guild_id = requested_slot_guild_id
            AND drain.slot_ruleset_key = requested_slot_ruleset_key
            AND drain.expected_revision = requested_expected_revision
        FOR UPDATE;
    END IF;

    IF product_count = 1 THEN
        product_tenant_id := product_row.tenant_id;
        product_installation_id := product_row.installation_id;
        product_deployment_id := product_row.deployment_id;
        product_expected_revision := product_row.expected_revision;
        product_operation_id := product_row.product_operation_id;
        product_expected_target := pg_catalog.jsonb_build_object(
            'guild_id',
            product_row.expected_target_guild_id,
            'ruleset_key',
            product_row.expected_target_ruleset_key,
            'version',
            product_row.expected_target_version,
            'content_hash',
            product_row.expected_target_content_hash,
            'binding_revision',
            product_row.expected_target_binding_revision,
            'binding_fingerprint',
            product_row.expected_target_binding_fingerprint
        );
        product_mutation_request_bytes :=
            product_row.product_mutation_request_bytes;
        product_mutation_digest := product_row.product_mutation_digest;
    END IF;

    IF drain_count = 1 THEN
        drain_tenant_id := drain_row.tenant_id;
        drain_installation_id := drain_row.installation_id;
        drain_deployment_id := drain_row.deployment_id;
        drain_slot_guild_id := drain_row.slot_guild_id;
        drain_slot_ruleset_key := drain_row.slot_ruleset_key;
        drain_expected_revision := drain_row.expected_revision;
        drain_intent_id := drain_row.drain_intent_id;
        drain_intent_request_bytes :=
            drain_row.drain_intent_request_bytes;
        drain_intent_digest := drain_row.drain_intent_digest;
        intent_revision := drain_row.intent_revision;
        intent_state := drain_row.intent_state;
    END IF;

    IF product_count <> drain_count THEN
        outcome_name := 'persistence_corrupt';
        RETURN NEXT;
        RETURN;
    END IF;

    IF product_count = 1 THEN
        stored_product_digest :=
            starring_runtime_private_v2.starring_runtime_product_mutation_digest_v2(
                product_row.product_mutation_request_bytes
            );
        stored_drain_digest :=
            starring_runtime_private_v2.starring_runtime_drain_intent_digest_v2(
                drain_row.drain_intent_request_bytes
            );

        IF stored_product_digest
                IS DISTINCT FROM product_row.product_mutation_digest
            OR stored_drain_digest
                IS DISTINCT FROM drain_row.drain_intent_digest
            OR drain_row.product_operation_id
                IS DISTINCT FROM product_row.product_operation_id
            OR drain_row.product_mutation_digest
                IS DISTINCT FROM product_row.product_mutation_digest
            OR drain_row.tenant_id IS DISTINCT FROM product_row.tenant_id
            OR drain_row.installation_id
                IS DISTINCT FROM product_row.installation_id
            OR drain_row.deployment_id
                IS DISTINCT FROM product_row.deployment_id
            OR drain_row.expected_revision
                IS DISTINCT FROM product_row.expected_revision
            OR drain_row.slot_guild_id
                IS DISTINCT FROM product_row.expected_target_guild_id
            OR drain_row.slot_ruleset_key
                IS DISTINCT FROM product_row.expected_target_ruleset_key
            OR product_row.tenant_id
                IS DISTINCT FROM requested_tenant_id
            OR product_row.installation_id
                IS DISTINCT FROM requested_installation_id
            OR product_row.deployment_id
                IS DISTINCT FROM requested_deployment_id
            OR product_row.expected_revision
                IS DISTINCT FROM requested_expected_revision
            OR product_row.expected_target_guild_id
                IS DISTINCT FROM deployment_row.guild_id
            OR product_row.expected_target_ruleset_key
                IS DISTINCT FROM deployment_row.ruleset_key
            OR product_row.expected_target_version
                IS DISTINCT FROM deployment_row.target_version
            OR product_row.expected_target_content_hash
                IS DISTINCT FROM deployment_row.target_content_hash
            OR product_row.expected_target_binding_revision
                IS DISTINCT FROM deployment_row.binding_revision
            OR product_row.expected_target_binding_fingerprint
                IS DISTINCT FROM deployment_row.binding_fingerprint
            OR deployment_row.revision < product_row.expected_revision
        THEN
            outcome_name := 'persistence_corrupt';
            RETURN NEXT;
            RETURN;
        END IF;

        IF product_row.product_operation_id = requested_operation_id
            AND drain_row.drain_intent_id = requested_intent_id
            AND product_row.product_mutation_request_bytes =
                expected_product_bytes
            AND product_row.product_mutation_digest =
                expected_product_digest
            AND drain_row.drain_intent_request_bytes =
                expected_drain_bytes
            AND drain_row.drain_intent_digest = expected_drain_digest
        THEN
            outcome_name := 'replayed';
            RETURN NEXT;
            RETURN;
        END IF;

        outcome_name := 'diverged';
        RETURN NEXT;
        RETURN;
    END IF;

    IF deployment_row.revision
            IS DISTINCT FROM requested_expected_revision
        OR deployment_row.guild_id
            IS DISTINCT FROM requested_target_guild_id
        OR deployment_row.ruleset_key
            IS DISTINCT FROM requested_target_ruleset_key
        OR deployment_row.target_version
            IS DISTINCT FROM requested_target_version
        OR deployment_row.target_content_hash
            IS DISTINCT FROM requested_target_content_hash
        OR deployment_row.binding_revision
            IS DISTINCT FROM requested_target_binding_revision
        OR deployment_row.binding_fingerprint
            IS DISTINCT FROM requested_target_binding_fingerprint
        OR deployment_row.phase NOT IN ('awaiting_gateway_ready', 'live')
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE =
                'runtime_product_drain_first_apply_deployment_mismatch';
    END IF;

    PERFORM 1
    FROM public.runtime_product_operations_v2 AS product
    WHERE product.product_operation_id = requested_operation_id
    FOR UPDATE;
    foreign_product_count := CASE WHEN FOUND THEN 1 ELSE 0 END;

    PERFORM 1
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE drain.drain_intent_id = requested_intent_id
    FOR UPDATE;
    foreign_drain_count := CASE WHEN FOUND THEN 1 ELSE 0 END;

    IF foreign_product_count > 0 OR foreign_drain_count > 0 THEN
        outcome_name := 'identifier_conflict';
        RETURN NEXT;
        RETURN;
    END IF;

    BEGIN
        PERFORM pg_catalog.set_config(
            'starring.runtime_product_drain_first_apply_product_operation_id_v2',
            requested_operation_id,
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_product_drain_first_apply_drain_intent_id_v2',
            '',
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_product_drain_first_apply_stage_v2',
            'product_insert',
            TRUE
        );

        INSERT INTO public.runtime_product_operations_v2 (
            product_operation_id,
            tenant_id,
            installation_id,
            deployment_id,
            expected_revision,
            expected_target_guild_id,
            expected_target_ruleset_key,
            expected_target_version,
            expected_target_content_hash,
            expected_target_binding_revision,
            expected_target_binding_fingerprint,
            product_mutation_request_bytes,
            product_mutation_digest
        )
        VALUES (
            requested_operation_id,
            requested_tenant_id,
            requested_installation_id,
            requested_deployment_id,
            requested_expected_revision,
            requested_target_guild_id,
            requested_target_ruleset_key,
            requested_target_version,
            requested_target_content_hash,
            requested_target_binding_revision,
            requested_target_binding_fingerprint,
            expected_product_bytes,
            expected_product_digest
        );

        IF COALESCE(pg_catalog.current_setting(
                'starring.runtime_product_drain_first_apply_stage_v2',
                TRUE
            ), '') <> ''
            OR pg_catalog.current_setting(
                'starring.runtime_product_drain_first_apply_product_operation_id_v2',
                TRUE
            ) IS DISTINCT FROM requested_operation_id
            OR COALESCE(pg_catalog.current_setting(
                'starring.runtime_product_drain_first_apply_drain_intent_id_v2',
                TRUE
            ), '') <> ''
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE =
                    'runtime_product_drain_first_apply_gate_consumption_invalid';
        END IF;

        PERFORM pg_catalog.set_config(
            'starring.runtime_product_drain_first_apply_drain_intent_id_v2',
            requested_intent_id,
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_product_drain_first_apply_stage_v2',
            'drain_insert',
            TRUE
        );

        INSERT INTO public.runtime_drain_intents_v2 (
            drain_intent_id,
            tenant_id,
            installation_id,
            deployment_id,
            slot_guild_id,
            slot_ruleset_key,
            expected_revision,
            product_operation_id,
            product_mutation_digest,
            drain_intent_request_bytes,
            drain_intent_digest,
            intent_revision,
            intent_state
        )
        VALUES (
            requested_intent_id,
            requested_tenant_id,
            requested_installation_id,
            requested_deployment_id,
            requested_slot_guild_id,
            requested_slot_ruleset_key,
            requested_expected_revision,
            requested_operation_id,
            expected_product_digest,
            expected_drain_bytes,
            expected_drain_digest,
            1,
            'pending'
        );

        IF COALESCE(pg_catalog.current_setting(
                'starring.runtime_product_drain_first_apply_stage_v2',
                TRUE
            ), '') <> ''
            OR COALESCE(pg_catalog.current_setting(
                'starring.runtime_product_drain_first_apply_product_operation_id_v2',
                TRUE
            ), '') <> ''
            OR COALESCE(pg_catalog.current_setting(
                'starring.runtime_product_drain_first_apply_drain_intent_id_v2',
                TRUE
            ), '') <> ''
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE =
                    'runtime_product_drain_first_apply_gate_consumption_invalid';
        END IF;

        PERFORM pg_catalog.set_config(
            'starring.runtime_product_drain_first_apply_stage_v2',
            '',
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_product_drain_first_apply_product_operation_id_v2',
            '',
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_product_drain_first_apply_drain_intent_id_v2',
            '',
            TRUE
        );
    EXCEPTION
        WHEN unique_violation THEN
            GET STACKED DIAGNOSTICS
                exception_schema_name = SCHEMA_NAME,
                exception_table_name = TABLE_NAME,
                exception_constraint_name = CONSTRAINT_NAME;
            PERFORM pg_catalog.set_config(
                'starring.runtime_product_drain_first_apply_stage_v2',
                '',
                TRUE
            );
            PERFORM pg_catalog.set_config(
                'starring.runtime_product_drain_first_apply_product_operation_id_v2',
                '',
                TRUE
            );
            PERFORM pg_catalog.set_config(
                'starring.runtime_product_drain_first_apply_drain_intent_id_v2',
                '',
                TRUE
            );
            IF COALESCE(pg_catalog.current_setting(
                    'starring.runtime_product_drain_first_apply_stage_v2',
                    TRUE
                ), '') <> ''
                OR COALESCE(pg_catalog.current_setting(
                    'starring.runtime_product_drain_first_apply_product_operation_id_v2',
                    TRUE
                ), '') <> ''
                OR COALESCE(pg_catalog.current_setting(
                    'starring.runtime_product_drain_first_apply_drain_intent_id_v2',
                    TRUE
                ), '') <> ''
            THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RX004',
                    MESSAGE =
                        'runtime_product_drain_first_apply_gate_cleanup_invalid';
            END IF;
            IF exception_schema_name = 'public'
                AND (
                    (
                        exception_table_name =
                            'runtime_product_operations_v2'
                        AND exception_constraint_name IN (
                            'runtime_product_operations_v2_pkey',
                            'runtime_product_operations_v2_natural_unique',
                            'runtime_product_operations_v2_pair_unique'
                        )
                    )
                    OR (
                        exception_table_name =
                            'runtime_drain_intents_v2'
                        AND exception_constraint_name IN (
                            'runtime_drain_intents_v2_pkey',
                            'runtime_drain_intents_v2_natural_unique',
                            'runtime_drain_intents_v2_product_unique'
                        )
                    )
                )
            THEN
                RAISE EXCEPTION USING
                    ERRCODE = '40001',
                    MESSAGE =
                        'runtime_product_drain_first_apply_serialization_conflict';
            END IF;
            RAISE;
        WHEN OTHERS THEN
            PERFORM pg_catalog.set_config(
                'starring.runtime_product_drain_first_apply_stage_v2',
                '',
                TRUE
            );
            PERFORM pg_catalog.set_config(
                'starring.runtime_product_drain_first_apply_product_operation_id_v2',
                '',
                TRUE
            );
            PERFORM pg_catalog.set_config(
                'starring.runtime_product_drain_first_apply_drain_intent_id_v2',
                '',
                TRUE
            );
            IF COALESCE(pg_catalog.current_setting(
                    'starring.runtime_product_drain_first_apply_stage_v2',
                    TRUE
                ), '') <> ''
                OR COALESCE(pg_catalog.current_setting(
                    'starring.runtime_product_drain_first_apply_product_operation_id_v2',
                    TRUE
                ), '') <> ''
                OR COALESCE(pg_catalog.current_setting(
                    'starring.runtime_product_drain_first_apply_drain_intent_id_v2',
                    TRUE
                ), '') <> ''
            THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RX004',
                    MESSAGE =
                        'runtime_product_drain_first_apply_gate_cleanup_invalid';
            END IF;
            RAISE;
    END;

    IF COALESCE(pg_catalog.current_setting(
            'starring.runtime_product_drain_first_apply_stage_v2',
            TRUE
        ), '') <> ''
        OR COALESCE(pg_catalog.current_setting(
            'starring.runtime_product_drain_first_apply_product_operation_id_v2',
            TRUE
        ), '') <> ''
        OR COALESCE(pg_catalog.current_setting(
            'starring.runtime_product_drain_first_apply_drain_intent_id_v2',
            TRUE
        ), '') <> ''
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE =
                'runtime_product_drain_first_apply_gate_cleanup_invalid';
    END IF;

    outcome_name := 'inserted';
    product_tenant_id := requested_tenant_id;
    product_installation_id := requested_installation_id;
    product_deployment_id := requested_deployment_id;
    product_expected_revision := requested_expected_revision;
    product_operation_id := requested_operation_id;
    product_expected_target := pg_catalog.jsonb_build_object(
        'guild_id',
        requested_target_guild_id,
        'ruleset_key',
        requested_target_ruleset_key,
        'version',
        requested_target_version,
        'content_hash',
        requested_target_content_hash,
        'binding_revision',
        requested_target_binding_revision,
        'binding_fingerprint',
        requested_target_binding_fingerprint
    );
    product_mutation_request_bytes := expected_product_bytes;
    product_mutation_digest := expected_product_digest;
    drain_tenant_id := requested_tenant_id;
    drain_installation_id := requested_installation_id;
    drain_deployment_id := requested_deployment_id;
    drain_slot_guild_id := requested_slot_guild_id;
    drain_slot_ruleset_key := requested_slot_ruleset_key;
    drain_expected_revision := requested_expected_revision;
    drain_intent_id := requested_intent_id;
    drain_intent_request_bytes := expected_drain_bytes;
    drain_intent_digest := expected_drain_digest;
    intent_revision := 1;
    intent_state := 'pending';
    RETURN NEXT;
END;
$function$;

REVOKE ALL PRIVILEGES ON FUNCTION
    starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(
        TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,TEXT,TEXT,TEXT,TEXT,BIGINT,TEXT,
        BIGINT,TEXT,TEXT,TEXT,BYTEA,TEXT,BYTEA,TEXT
    )
FROM PUBLIC;

DO $patch_manifest$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_schema_manifest_v1()'
    );

    previous_fragment :=
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_drain_intent_digest_v2(bytea)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_observe_previous_serving_v1';
    next_fragment :=
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_drain_intent_digest_v2(bytea)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text,bytea,text,bytea,text)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_observe_previous_serving_v1';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_product_drain_first_apply_manifest_function_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    RETURN observed_count = 581' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''944f87185d6fd290c3b9a2fe2de08ec097c833802292a2ed34c80c811c5ee062'';';
    next_fragment :=
        '    RETURN observed_count = 582' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''cfeaad4271e12f72f20aa57ad8dc92c63a787f260551fee414897a69143b20de'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_product_drain_first_apply_manifest_expectation_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
    EXECUTE definition;
END;
$patch_manifest$;

DO $patch_readiness$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_database_readiness_v1()'
    );

    previous_fragment :=
        '            (''starring_runtime_private_v2.starring_runtime_drain_intent_digest_v2(bytea)''),' || E'\n' ||
        '            (''public.reject_ruleset_artifact_mutation()'')';
    next_fragment :=
        '            (''starring_runtime_private_v2.starring_runtime_drain_intent_digest_v2(bytea)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text,bytea,text,bytea,text)''),' || E'\n' ||
        '            (''public.reject_ruleset_artifact_mutation()'')';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_product_drain_first_apply_readiness_protected_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            (''starring_runtime_private_v2.starring_runtime_drain_intent_digest_v2(bytea)'')' || E'\n' ||
        '    ) AS expected(identity)' || E'\n' ||
        '    LEFT JOIN pg_catalog.pg_proc AS function_row' || E'\n' ||
        '        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)' || E'\n' ||
        '    WHERE function_row.oid IS NULL' || E'\n' ||
        '        OR (' || E'\n' ||
        '            SELECT pg_catalog.count(*)';
    next_fragment :=
        '            (''starring_runtime_private_v2.starring_runtime_drain_intent_digest_v2(bytea)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text,bytea,text,bytea,text)'')' || E'\n' ||
        '    ) AS expected(identity)' || E'\n' ||
        '    LEFT JOIN pg_catalog.pg_proc AS function_row' || E'\n' ||
        '        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)' || E'\n' ||
        '    WHERE function_row.oid IS NULL' || E'\n' ||
        '        OR (' || E'\n' ||
        '            SELECT pg_catalog.count(*)';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_product_drain_first_apply_readiness_acl_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '''27ebe976c214377f71f62cf7d9c90be3009e3c331e395dff7d63c587513be167''::TEXT';
    next_fragment :=
        '''331a95180a75109385566b0b1b0659e247e5619cf02e2f61ee89904a2751856b''::TEXT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_product_drain_first_apply_readiness_digest_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
    EXECUTE definition;
END;
$patch_readiness$;

DO $postflight$
DECLARE
    common_owner OID;
    executor_grantee OID;
    external_executor_count BIGINT;
    invalid_capability_acl_count BIGINT;
    invalid_owner_only_acl_count BIGINT;
    invalid_private_function_count BIGINT;
    invalid_core_count BIGINT;
    public_snapshot_mismatch_count BIGINT;
    private_snapshot_mismatch_count BIGINT;
    trigger_digest TEXT;
    manifest_digest TEXT;
    readiness_digest TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT
        pg_catalog.min(privilege.grantee::BIGINT)::OID,
        pg_catalog.count(*)
    INTO executor_grantee, external_executor_count
    FROM pg_catalog.pg_proc AS function_row
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_database_identity_v1()'
        )
        AND privilege.grantee <> common_owner;

    SELECT pg_catalog.count(*)
    INTO invalid_capability_acl_count
    FROM pg_temp.starring_runtime_first_apply_capability AS expected
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(
            expected.function_identity
        )
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
        ) <> CASE WHEN executor_grantee IS NULL THEN 1 ELSE 2 END
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee = common_owner
        ) <> 1
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
        ) <> CASE WHEN executor_grantee IS NULL THEN 0 ELSE 1 END
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE (
                    privilege.grantee <> common_owner
                    AND (
                        executor_grantee IS NULL
                        OR privilege.grantee <> executor_grantee
                    )
                )
                OR privilege.grantor <> common_owner
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
        );

    SELECT pg_catalog.count(*)
    INTO invalid_owner_only_acl_count
    FROM (
        VALUES
            ('public.starring_runtime_execution_schema_manifest_v1()'),
            ('public.reject_runtime_product_drain_mutation()')
    ) AS expected(function_identity)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(
            expected.function_identity
        )
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
        ) <> 1
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
    INTO invalid_private_function_count
    FROM pg_temp.starring_runtime_first_apply_private_snapshot AS snapshot
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = snapshot.function_oid
    WHERE function_row.oid IS NULL
        OR function_row.proowner IS DISTINCT FROM snapshot.function_owner
        OR function_row.proacl IS DISTINCT FROM snapshot.function_acl;

    SELECT pg_catalog.count(*)
    INTO invalid_core_count
    FROM pg_catalog.pg_proc AS function_row
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text,bytea,text,bytea,text)'
        )
        AND (
            function_row.proowner <> common_owner
            OR function_row.prokind <> 'f'
            OR function_row.provolatile <> 'v'
            OR NOT function_row.proisstrict
            OR function_row.proparallel <> 'u'
            OR function_row.prosecdef
            OR NOT function_row.proretset
            OR function_row.prorows <> 1::REAL
            OR function_row.proleakproof
            OR function_row.pronargdefaults <> 0
            OR function_row.provariadic <> 0
            OR function_row.proconfig IS DISTINCT FROM ARRAY[
                'search_path=pg_catalog, starring_runtime_private_v2'
            ]::TEXT[]
            OR language_row.lanname IS DISTINCT FROM 'plpgsql'
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
            ) <> 1
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
            )
        );

    SELECT pg_catalog.count(*)
    INTO public_snapshot_mismatch_count
    FROM (
        SELECT
            snapshot.function_oid,
            snapshot.function_owner,
            snapshot.function_acl,
            function_row.oid AS observed_oid,
            function_row.proowner AS observed_owner,
            function_row.proacl AS observed_acl
        FROM pg_temp.starring_runtime_first_apply_public_snapshot AS snapshot
        FULL OUTER JOIN (
            SELECT function_row.oid, function_row.proowner, function_row.proacl
            FROM pg_catalog.pg_proc AS function_row
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = function_row.pronamespace
            WHERE namespace.nspname = 'public'
        ) AS function_row
            ON function_row.oid = snapshot.function_oid
    ) AS comparison
    WHERE comparison.function_oid IS NULL
        OR comparison.observed_oid IS NULL
        OR comparison.function_owner
            IS DISTINCT FROM comparison.observed_owner
        OR comparison.function_acl IS DISTINCT FROM comparison.observed_acl;

    SELECT pg_catalog.count(*)
    INTO private_snapshot_mismatch_count
    FROM (
        SELECT
            snapshot.function_oid,
            snapshot.function_owner,
            snapshot.function_acl,
            function_row.oid AS observed_oid,
            function_row.proowner AS observed_owner,
            function_row.proacl AS observed_acl
        FROM pg_temp.starring_runtime_first_apply_private_snapshot AS snapshot
        FULL OUTER JOIN (
            SELECT function_row.oid, function_row.proowner, function_row.proacl
            FROM pg_catalog.pg_proc AS function_row
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = function_row.pronamespace
            WHERE namespace.nspname = 'starring_runtime_private_v2'
                AND function_row.oid <> pg_catalog.to_regprocedure(
                    'starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text,bytea,text,bytea,text)'
                )
        ) AS function_row
            ON function_row.oid = snapshot.function_oid
    ) AS comparison
    WHERE comparison.function_oid IS NULL
        OR comparison.observed_oid IS NULL
        OR comparison.function_owner
            IS DISTINCT FROM comparison.observed_owner
        OR comparison.function_acl IS DISTINCT FROM comparison.observed_acl;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO trigger_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.reject_runtime_product_drain_mutation()'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO manifest_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_schema_manifest_v1()'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO readiness_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_database_readiness_v1()'
    );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR external_executor_count > 1
        OR executor_grantee = 0
        OR invalid_capability_acl_count <> 0
        OR invalid_owner_only_acl_count <> 0
        OR invalid_private_function_count <> 0
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_temp.starring_runtime_first_apply_private_snapshot
        ) <> 6
        OR pg_catalog.to_regprocedure(
            'starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text,bytea,text,bytea,text)'
        ) IS NULL
        OR invalid_core_count <> 0
        OR public_snapshot_mismatch_count <> 0
        OR private_snapshot_mismatch_count <> 0
        OR trigger_digest IS DISTINCT FROM
            '46eb448ad443abd551a67aba47f77c6dbfe331e5b473c0bda984dc0614d4c38a'
        OR manifest_digest IS DISTINCT FROM
            '331a95180a75109385566b0b1b0659e247e5619cf02e2f61ee89904a2751856b'
        OR readiness_digest IS DISTINCT FROM
            '3e2d46d692daf8bd9cff68f00459f00f6b8bf314378a663727b94493d7e45279'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_first_apply_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
