SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

LOCK TABLE
    public.runtime_deployments,
    public.runtime_execution_mutation_markers,
    public.runtime_gateway_owners,
    public.runtime_writer_fence
IN ACCESS EXCLUSIVE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    collision_count BIGINT;
    manifest_digest TEXT;
    readiness_digest TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_runtime_product_drain_observe_v2',
            'reject_runtime_product_drain_mutation'
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
        OR pg_catalog.to_regclass('public.runtime_product_operations_v2') IS NOT NULL
        OR pg_catalog.to_regclass('public.runtime_drain_intents_v2') IS NOT NULL
        OR collision_count <> 0
        OR manifest_digest IS DISTINCT FROM
            '42c6652f5d25634d247821002b619acac6dff1997d6cd1df2ea633d310456061'
        OR readiness_digest IS DISTINCT FROM
            'cf84d5a445c591cd11802e9d956c2f03ae7f9c4205134aa1511d4cc40d88fbc3'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_preflight_drift';
    END IF;
END;
$preflight$;

CREATE TABLE public.runtime_product_operations_v2 (
    product_operation_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    installation_id TEXT NOT NULL,
    deployment_id TEXT NOT NULL,
    expected_revision BIGINT NOT NULL,
    expected_target_guild_id TEXT NOT NULL,
    expected_target_ruleset_key TEXT NOT NULL,
    expected_target_version BIGINT NOT NULL,
    expected_target_content_hash TEXT NOT NULL,
    expected_target_binding_revision BIGINT NOT NULL,
    expected_target_binding_fingerprint TEXT NOT NULL,
    product_mutation_request_bytes BYTEA NOT NULL,
    product_mutation_digest TEXT NOT NULL,
    CONSTRAINT runtime_product_operations_v2_scope_fk FOREIGN KEY (
        tenant_id,
        installation_id,
        deployment_id
    ) REFERENCES public.runtime_deployments (
        tenant_id,
        installation_id,
        deployment_id
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_product_operations_v2_natural_unique UNIQUE (
        tenant_id,
        installation_id,
        deployment_id,
        expected_revision
    ),
    CONSTRAINT runtime_product_operations_v2_pair_unique UNIQUE (
        product_operation_id,
        product_mutation_digest,
        tenant_id,
        installation_id,
        deployment_id,
        expected_revision,
        expected_target_guild_id,
        expected_target_ruleset_key
    ),
    CONSTRAINT runtime_product_operations_v2_id_check CHECK (
        product_operation_id ~ '^[0-9a-f]{32}$'
    ),
    CONSTRAINT runtime_product_operations_v2_scope_check CHECK (
        tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND deployment_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT runtime_product_operations_v2_revision_check CHECK (
        expected_revision BETWEEN 1 AND 9223372036854775807
    ),
    CONSTRAINT runtime_product_operations_v2_target_check CHECK (
        expected_target_guild_id ~ '^[1-9][0-9]{0,19}$'
        AND (
            pg_catalog.length(expected_target_guild_id) < 20
            OR expected_target_guild_id COLLATE pg_catalog."C"
                <= '18446744073709551615' COLLATE pg_catalog."C"
        )
        AND expected_target_ruleset_key ~ '^[A-Za-z0-9_-]{1,64}$'
        AND expected_target_version BETWEEN 1 AND 4294967295
        AND expected_target_content_hash ~ '^[0-9a-f]{64}$'
        AND expected_target_binding_revision
            BETWEEN 1 AND 9223372036854775807
        AND expected_target_binding_fingerprint ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT runtime_product_operations_v2_canonical_check CHECK (
        pg_catalog.octet_length(product_mutation_request_bytes)
            BETWEEN 1 AND 32768
        AND product_mutation_digest ~ '^[0-9a-f]{64}$'
    )
);

CREATE TABLE public.runtime_drain_intents_v2 (
    drain_intent_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    installation_id TEXT NOT NULL,
    deployment_id TEXT NOT NULL,
    slot_guild_id TEXT NOT NULL,
    slot_ruleset_key TEXT NOT NULL,
    expected_revision BIGINT NOT NULL,
    product_operation_id TEXT NOT NULL,
    product_mutation_digest TEXT NOT NULL,
    drain_intent_request_bytes BYTEA NOT NULL,
    drain_intent_digest TEXT NOT NULL,
    intent_revision BIGINT NOT NULL,
    intent_state TEXT NOT NULL,
    CONSTRAINT runtime_drain_intents_v2_scope_fk FOREIGN KEY (
        tenant_id,
        installation_id,
        deployment_id
    ) REFERENCES public.runtime_deployments (
        tenant_id,
        installation_id,
        deployment_id
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_drain_intents_v2_product_fk FOREIGN KEY (
        product_operation_id,
        product_mutation_digest,
        tenant_id,
        installation_id,
        deployment_id,
        expected_revision,
        slot_guild_id,
        slot_ruleset_key
    ) REFERENCES public.runtime_product_operations_v2 (
        product_operation_id,
        product_mutation_digest,
        tenant_id,
        installation_id,
        deployment_id,
        expected_revision,
        expected_target_guild_id,
        expected_target_ruleset_key
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_drain_intents_v2_natural_unique UNIQUE (
        tenant_id,
        installation_id,
        deployment_id,
        slot_guild_id,
        slot_ruleset_key,
        expected_revision
    ),
    CONSTRAINT runtime_drain_intents_v2_product_unique UNIQUE (
        product_operation_id
    ),
    CONSTRAINT runtime_drain_intents_v2_id_check CHECK (
        drain_intent_id ~ '^[0-9a-f]{32}$'
        AND product_operation_id ~ '^[0-9a-f]{32}$'
    ),
    CONSTRAINT runtime_drain_intents_v2_scope_check CHECK (
        tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND deployment_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT runtime_drain_intents_v2_slot_check CHECK (
        slot_guild_id ~ '^[1-9][0-9]{0,19}$'
        AND (
            pg_catalog.length(slot_guild_id) < 20
            OR slot_guild_id COLLATE pg_catalog."C"
                <= '18446744073709551615' COLLATE pg_catalog."C"
        )
        AND slot_ruleset_key ~ '^[A-Za-z0-9_-]{1,64}$'
    ),
    CONSTRAINT runtime_drain_intents_v2_revision_check CHECK (
        expected_revision BETWEEN 1 AND 9223372036854775807
        AND intent_revision = 1
    ),
    CONSTRAINT runtime_drain_intents_v2_canonical_check CHECK (
        product_mutation_digest ~ '^[0-9a-f]{64}$'
        AND pg_catalog.octet_length(drain_intent_request_bytes)
            BETWEEN 1 AND 65536
        AND drain_intent_digest ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT runtime_drain_intents_v2_state_check CHECK (
        intent_state = 'pending'
    )
);

CREATE FUNCTION public.reject_runtime_product_drain_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    RAISE EXCEPTION USING
        ERRCODE = '23514',
        MESSAGE = 'runtime_product_drain_mutation_rejected';
END;
$function$;

CREATE TRIGGER runtime_product_operations_v2_reject_row_mutation
BEFORE INSERT OR UPDATE OR DELETE ON public.runtime_product_operations_v2
FOR EACH ROW
EXECUTE FUNCTION public.reject_runtime_product_drain_mutation();

CREATE TRIGGER runtime_product_operations_v2_reject_truncate
BEFORE TRUNCATE ON public.runtime_product_operations_v2
FOR EACH STATEMENT
EXECUTE FUNCTION public.reject_runtime_product_drain_mutation();

CREATE TRIGGER runtime_drain_intents_v2_reject_row_mutation
BEFORE INSERT OR UPDATE OR DELETE ON public.runtime_drain_intents_v2
FOR EACH ROW
EXECUTE FUNCTION public.reject_runtime_product_drain_mutation();

CREATE TRIGGER runtime_drain_intents_v2_reject_truncate
BEFORE TRUNCATE ON public.runtime_drain_intents_v2
FOR EACH STATEMENT
EXECUTE FUNCTION public.reject_runtime_product_drain_mutation();

CREATE FUNCTION public.starring_runtime_product_drain_observe_v2(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_revision BIGINT,
    expected_slot_guild_id TEXT,
    expected_slot_ruleset_key TEXT
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
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
DECLARE
    deployment_row public.runtime_deployments%ROWTYPE;
    product_row public.runtime_product_operations_v2%ROWTYPE;
    drain_row public.runtime_drain_intents_v2%ROWTYPE;
    writer_fence_state TEXT;
    product_count BIGINT;
    drain_count BIGINT;
BEGIN
    IF pg_catalog.current_setting('transaction_isolation')
            <> 'read committed'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_product_drain_observe_isolation_invalid';
    END IF;

    IF expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR $4 NOT BETWEEN 1 AND 9223372036854775807
        OR expected_slot_guild_id !~ '^[1-9][0-9]{0,19}$'
        OR (
            pg_catalog.length(expected_slot_guild_id) = 20
            AND expected_slot_guild_id COLLATE pg_catalog."C"
                > '18446744073709551615' COLLATE pg_catalog."C"
        )
        OR expected_slot_ruleset_key !~ '^[A-Za-z0-9_-]{1,64}$'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_product_drain_observe_input_invalid';
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock_shared(
        pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
    );

    SELECT fence.fence_state
    INTO writer_fence_state
    FROM public.runtime_writer_fence AS fence
    WHERE fence.singleton;

    IF NOT FOUND
        OR (
            writer_fence_state IS DISTINCT FROM 'open'
            AND writer_fence_state IS DISTINCT FROM 'closed'
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_product_drain_observe_writer_fence_invalid';
    END IF;

    IF writer_fence_state = 'closed' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX005',
            MESSAGE = 'runtime_product_drain_observe_writer_fenced';
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            pg_catalog.concat(
                'starring-runtime-serving-slot-v1:',
                expected_slot_guild_id,
                ':',
                expected_slot_ruleset_key
            ),
            0
        )
    );

    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = expected_tenant_id
        AND deployment.installation_id = expected_installation_id
        AND deployment.deployment_id = expected_deployment_id
        AND deployment.revision = $4
        AND deployment.guild_id = expected_slot_guild_id
        AND deployment.ruleset_key = expected_slot_ruleset_key
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_product_drain_observe_deployment_mismatch';
    END IF;

    locked_snapshot := deployment_row.snapshot;
    observed_at := pg_catalog.clock_timestamp();

    SELECT pg_catalog.count(*)
    INTO product_count
    FROM public.runtime_product_operations_v2 AS product
    WHERE product.tenant_id = expected_tenant_id
        AND product.installation_id = expected_installation_id
        AND product.deployment_id = expected_deployment_id
        AND product.expected_revision = $4;

    SELECT pg_catalog.count(*)
    INTO drain_count
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE drain.tenant_id = expected_tenant_id
        AND drain.installation_id = expected_installation_id
        AND drain.deployment_id = expected_deployment_id
        AND drain.slot_guild_id = expected_slot_guild_id
        AND drain.slot_ruleset_key = expected_slot_ruleset_key
        AND drain.expected_revision = $4;

    IF product_count > 1 THEN
        outcome_name := 'ambiguous_product';
        RETURN NEXT;
        RETURN;
    END IF;

    IF drain_count > 1 THEN
        outcome_name := 'ambiguous_drain';
        RETURN NEXT;
        RETURN;
    END IF;

    IF product_count = 1 THEN
        SELECT product.*
        INTO STRICT product_row
        FROM public.runtime_product_operations_v2 AS product
        WHERE product.tenant_id = expected_tenant_id
            AND product.installation_id = expected_installation_id
            AND product.deployment_id = expected_deployment_id
            AND product.expected_revision = $4
        FOR UPDATE;

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
        SELECT drain.*
        INTO STRICT drain_row
        FROM public.runtime_drain_intents_v2 AS drain
        WHERE drain.tenant_id = expected_tenant_id
            AND drain.installation_id = expected_installation_id
            AND drain.deployment_id = expected_deployment_id
            AND drain.slot_guild_id = expected_slot_guild_id
            AND drain.slot_ruleset_key = expected_slot_ruleset_key
            AND drain.expected_revision = $4
        FOR UPDATE;

        drain_tenant_id := drain_row.tenant_id;
        drain_installation_id := drain_row.installation_id;
        drain_deployment_id := drain_row.deployment_id;
        drain_slot_guild_id := drain_row.slot_guild_id;
        drain_slot_ruleset_key := drain_row.slot_ruleset_key;
        drain_expected_revision := drain_row.expected_revision;
        drain_intent_id := drain_row.drain_intent_id;
        drain_intent_request_bytes := drain_row.drain_intent_request_bytes;
        drain_intent_digest := drain_row.drain_intent_digest;
        intent_revision := drain_row.intent_revision;
        intent_state := drain_row.intent_state;
    END IF;

    IF product_count = 0 AND drain_count = 0 THEN
        outcome_name := 'absent';
    ELSIF product_count = 1 AND drain_count = 0 THEN
        outcome_name := 'partial_product';
    ELSIF product_count = 0 AND drain_count = 1 THEN
        outcome_name := 'partial_drain';
    ELSIF drain_row.product_operation_id
            IS DISTINCT FROM product_row.product_operation_id
        OR drain_row.product_mutation_digest
            IS DISTINCT FROM product_row.product_mutation_digest
        OR drain_row.tenant_id IS DISTINCT FROM product_row.tenant_id
        OR drain_row.installation_id
            IS DISTINCT FROM product_row.installation_id
        OR drain_row.deployment_id IS DISTINCT FROM product_row.deployment_id
        OR drain_row.expected_revision
            IS DISTINCT FROM product_row.expected_revision
        OR drain_row.slot_guild_id
            IS DISTINCT FROM product_row.expected_target_guild_id
        OR drain_row.slot_ruleset_key
            IS DISTINCT FROM product_row.expected_target_ruleset_key
    THEN
        outcome_name := 'pair_mismatch';
    ELSE
        outcome_name := 'present';
    END IF;

    RETURN NEXT;
END;
$function$;

REVOKE ALL ON TABLE public.runtime_product_operations_v2 FROM PUBLIC;
REVOKE ALL ON TABLE public.runtime_drain_intents_v2 FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.reject_runtime_product_drain_mutation()
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.starring_runtime_product_drain_observe_v2(TEXT,TEXT,TEXT,BIGINT,TEXT,TEXT)
FROM PUBLIC;

DO $execution_acl$
DECLARE
    common_owner OID;
    executor_grantee OID;
    invalid_capability_count BIGINT;
    executor_name NAME;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT pg_catalog.min(privilege.grantee::BIGINT)::OID
    INTO executor_grantee
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
    INTO invalid_capability_count
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_database_identity_v1()'
        )
        AND EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
                AND (
                    privilege.grantee IS DISTINCT FROM executor_grantee
                    OR privilege.grantor <> common_owner
                    OR privilege.privilege_type <> 'EXECUTE'
                    OR privilege.is_grantable
                )
        );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR invalid_capability_count <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_execution_acl_drift';
    END IF;

    IF executor_grantee IS NOT NULL THEN
        executor_name := pg_catalog.pg_get_userbyid(executor_grantee);
        IF executor_name IS NULL THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_product_drain_execution_acl_drift';
        END IF;
        EXECUTE pg_catalog.format(
            'GRANT EXECUTE ON FUNCTION public.starring_runtime_product_drain_observe_v2(TEXT,TEXT,TEXT,BIGINT,TEXT,TEXT) TO %I',
            executor_name
        );
    END IF;
END;
$execution_acl$;

DO $patch_schema_manifest$
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
        '(pg_catalog.to_regclass(''public.runtime_writer_fence'')),';
    next_fragment := previous_fragment || E'\n' ||
        '            (pg_catalog.to_regclass(''public.runtime_product_operations_v2'')),' || E'\n' ||
        '            (pg_catalog.to_regclass(''public.runtime_drain_intents_v2'')),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_manifest_relation_patch_drift';
    END IF;
    definition := pg_catalog.replace(definition, previous_fragment, next_fragment);

    previous_fragment :=
        'SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_writer_fence_observe_v1()''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_observe_previous_serving_v1';
    next_fragment :=
        'SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_writer_fence_observe_v1()''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_product_drain_observe_v2(text,text,text,bigint,text,text)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_observe_previous_serving_v1';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_manifest_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(definition, previous_fragment, next_fragment);

    previous_fragment :=
        'RETURN observed_count = 514' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''6f00d0c25506999af7d03eec22ab01513ea4711bbc7dc6eacae2f0f3ce8cd2f5'';';
    next_fragment :=
        'RETURN observed_count = 574' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''996ddaf475f4c184cc4d7f2c576d8543c7cd84ae13d614d51723103b3d43bdbe'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_manifest_expectation_patch_drift';
    END IF;
    definition := pg_catalog.replace(definition, previous_fragment, next_fragment);
    EXECUTE definition;
END;
$patch_schema_manifest$;

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
        '(''public.runtime_writer_fence''),';
    next_fragment := previous_fragment || E'\n' ||
        '            (''public.runtime_product_operations_v2''),' || E'\n' ||
        '            (''public.runtime_drain_intents_v2''),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_readiness_relation_patch_drift';
    END IF;
    definition := pg_catalog.replace(definition, previous_fragment, next_fragment);

    previous_fragment :=
        '(''public.reject_runtime_writer_fence_mutation()''),' || E'\n' ||
        '            (''public.reject_ruleset_artifact_mutation()'')';
    next_fragment :=
        '(''public.reject_runtime_writer_fence_mutation()''),' || E'\n' ||
        '            (''public.reject_runtime_product_drain_mutation()''),' || E'\n' ||
        '            (''public.reject_ruleset_artifact_mutation()'')';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_readiness_protected_patch_drift';
    END IF;
    definition := pg_catalog.replace(definition, previous_fragment, next_fragment);

    previous_fragment :=
        '                ''public.starring_runtime_writer_fence_observe_v1()'',' || E'\n' ||
        '                ''''::TEXT,' || E'\n' ||
        '                ''TABLE(fence_state text, fence_generation bigint, cutover_coordinator_id text, cutover_lease_epoch bigint, database_now timestamp with time zone, cutover_expires_at timestamp with time zone)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            )' || E'\n' ||
        '    ) AS expected(';
    next_fragment :=
        '                ''public.starring_runtime_writer_fence_observe_v1()'',' || E'\n' ||
        '                ''''::TEXT,' || E'\n' ||
        '                ''TABLE(fence_state text, fence_generation bigint, cutover_coordinator_id text, cutover_lease_epoch bigint, database_now timestamp with time zone, cutover_expires_at timestamp with time zone)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            ),' || E'\n' ||
        '            (' || E'\n' ||
        '                ''public.starring_runtime_product_drain_observe_v2(text,text,text,bigint,text,text)'',' || E'\n' ||
        '                ''expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_revision bigint, expected_slot_guild_id text, expected_slot_ruleset_key text''::TEXT,' || E'\n' ||
        '                ''TABLE(outcome_name text, locked_snapshot jsonb, observed_at timestamp with time zone, product_tenant_id text, product_installation_id text, product_deployment_id text, product_expected_revision bigint, product_operation_id text, product_expected_target jsonb, product_mutation_request_bytes bytea, product_mutation_digest text, drain_tenant_id text, drain_installation_id text, drain_deployment_id text, drain_slot_guild_id text, drain_slot_ruleset_key text, drain_expected_revision bigint, drain_intent_id text, drain_intent_request_bytes bytea, drain_intent_digest text, intent_revision bigint, intent_state text)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            )' || E'\n' ||
        '    ) AS expected(';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_readiness_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(definition, previous_fragment, next_fragment);

    previous_fragment :=
        '''42c6652f5d25634d247821002b619acac6dff1997d6cd1df2ea633d310456061''::TEXT';
    next_fragment :=
        '''6cd7af34a993bdfe7a6b86e303d88b4b98bfd5170b34d0a1f15cb81a4aee3a2e''::TEXT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_readiness_manifest_digest_patch_drift';
    END IF;
    definition := pg_catalog.replace(definition, previous_fragment, next_fragment);

    previous_fragment :=
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_writer_fence_observe_v1()''' || E'\n' ||
        '            )' || E'\n' ||
        '        )';
    next_fragment :=
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_writer_fence_observe_v1()''' || E'\n' ||
        '            ),' || E'\n' ||
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_product_drain_observe_v2(text,text,text,bigint,text,text)''' || E'\n' ||
        '            )' || E'\n' ||
        '        )';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_readiness_allowlist_patch_drift';
    END IF;
    definition := pg_catalog.replace(definition, previous_fragment, next_fragment);
    EXECUTE definition;
END;
$patch_readiness$;

DO $postflight$
DECLARE
    common_owner OID;
    executor_grantee OID;
    invalid_relation_count BIGINT;
    invalid_function_count BIGINT;
    invalid_trigger_count BIGINT;
    manifest_digest TEXT;
    readiness_digest TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT pg_catalog.min(privilege.grantee::BIGINT)::OID
    INTO executor_grantee
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
    INTO invalid_relation_count
    FROM (
        VALUES
            (pg_catalog.to_regclass('public.runtime_product_operations_v2')),
            (pg_catalog.to_regclass('public.runtime_drain_intents_v2'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid
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
            WHERE privilege.grantee <> common_owner
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_attribute AS attribute
            CROSS JOIN LATERAL pg_catalog.aclexplode(attribute.attacl)
                AS privilege
            WHERE attribute.attrelid = relation.oid
                AND attribute.attnum > 0
                AND NOT attribute.attisdropped
                AND privilege.grantee <> common_owner
        );

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_runtime_product_drain_observe_v2(text,text,text,bigint,text,text)',
                TRUE,
                TRUE,
                1::REAL,
                TRUE
            ),
            (
                'public.reject_runtime_product_drain_mutation()',
                FALSE,
                FALSE,
                0::REAL,
                FALSE
            )
    ) AS expected(
        identity,
        is_strict,
        returns_set,
        rows_estimate,
        executor_capability
    )
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
        OR function_row.proretset IS DISTINCT FROM expected.returns_set
        OR function_row.prorows IS DISTINCT FROM expected.rows_estimate
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM 'plpgsql'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
        ) <> CASE
            WHEN expected.executor_capability AND executor_grantee IS NOT NULL
                THEN 1
            ELSE 0
        END
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee NOT IN (common_owner, executor_grantee)
                OR privilege.grantor <> common_owner
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
        );

    SELECT pg_catalog.count(*)
    INTO invalid_trigger_count
    FROM (
        VALUES
            (
                pg_catalog.to_regclass('public.runtime_product_operations_v2'),
                'runtime_product_operations_v2_reject_row_mutation'
            ),
            (
                pg_catalog.to_regclass('public.runtime_product_operations_v2'),
                'runtime_product_operations_v2_reject_truncate'
            ),
            (
                pg_catalog.to_regclass('public.runtime_drain_intents_v2'),
                'runtime_drain_intents_v2_reject_row_mutation'
            ),
            (
                pg_catalog.to_regclass('public.runtime_drain_intents_v2'),
                'runtime_drain_intents_v2_reject_truncate'
            )
    ) AS expected(relation_oid, trigger_name)
    LEFT JOIN pg_catalog.pg_trigger AS trigger_row
        ON trigger_row.tgrelid = expected.relation_oid
        AND trigger_row.tgname = expected.trigger_name
    WHERE trigger_row.oid IS NULL
        OR trigger_row.tgisinternal
        OR trigger_row.tgenabled <> 'O'
        OR trigger_row.tgfoid IS DISTINCT FROM pg_catalog.to_regprocedure(
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
        OR pg_catalog.to_regclass(
            'public.runtime_product_operations_v2'
        ) IS NULL
        OR pg_catalog.to_regclass('public.runtime_drain_intents_v2') IS NULL
        OR invalid_relation_count <> 0
        OR invalid_function_count <> 0
        OR invalid_trigger_count <> 0
        OR (
            SELECT pg_catalog.count(*)
            FROM public.runtime_product_operations_v2
        ) <> 0
        OR (
            SELECT pg_catalog.count(*)
            FROM public.runtime_drain_intents_v2
        ) <> 0
        OR manifest_digest IS DISTINCT FROM
            '6cd7af34a993bdfe7a6b86e303d88b4b98bfd5170b34d0a1f15cb81a4aee3a2e'
        OR readiness_digest IS DISTINCT FROM
            '746a7e3ac38bf588665063c9b3d79df4fc18e278e2a4edf4d1e3ea97e723827d'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
