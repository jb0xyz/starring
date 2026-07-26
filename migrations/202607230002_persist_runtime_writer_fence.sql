SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

LOCK TABLE
    public.runtime_deployments,
    public.runtime_execution_mutation_markers,
    public.runtime_gateway_owners
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
            'starring_runtime_writer_fence_observe_v1',
            'reject_runtime_writer_fence_mutation'
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
        OR pg_catalog.to_regclass('public.runtime_writer_fence') IS NOT NULL
        OR collision_count <> 0
        OR manifest_digest IS DISTINCT FROM
            '4b7a0b8daf9868d92edfae0cd83e35d805d27b824ef04a8b4eb06a229caeedf0'
        OR readiness_digest IS DISTINCT FROM
            '003baab6fe5443a3bcf6dc6356cd5595434ac68c507a56151a65874397432ff1'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_writer_fence_preflight_drift';
    END IF;
END;
$preflight$;

CREATE TABLE public.runtime_writer_fence (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE,
    fence_state TEXT NOT NULL,
    fence_generation BIGINT NOT NULL,
    cutover_lease_epoch_high_water BIGINT NOT NULL,
    cutover_coordinator_id TEXT,
    cutover_expires_at TIMESTAMPTZ,
    CONSTRAINT runtime_writer_fence_singleton_check CHECK (singleton),
    CONSTRAINT runtime_writer_fence_state_check CHECK (
        fence_state IN ('open', 'closed')
    ),
    CONSTRAINT runtime_writer_fence_generation_check CHECK (
        fence_generation BETWEEN 1 AND 9223372036854775807
    ),
    CONSTRAINT runtime_writer_fence_epoch_check CHECK (
        cutover_lease_epoch_high_water BETWEEN 0 AND 9223372036854775807
    ),
    CONSTRAINT runtime_writer_fence_coordinator_check CHECK (
        cutover_coordinator_id IS NULL
        OR cutover_coordinator_id ~ '^[0-9a-f]{32}$'
    ),
    CONSTRAINT runtime_writer_fence_shape_check CHECK (
        (
            fence_state = 'open'
            AND cutover_coordinator_id IS NULL
            AND cutover_expires_at IS NULL
        )
        OR (
            fence_state = 'closed'
            AND cutover_coordinator_id IS NOT NULL
            AND cutover_lease_epoch_high_water >= 1
            AND cutover_expires_at IS NOT NULL
        )
    )
);

INSERT INTO public.runtime_writer_fence (
    singleton,
    fence_state,
    fence_generation,
    cutover_lease_epoch_high_water,
    cutover_coordinator_id,
    cutover_expires_at
)
VALUES (TRUE, 'open', 1, 0, NULL, NULL);

CREATE FUNCTION public.reject_runtime_writer_fence_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    RAISE EXCEPTION USING
        ERRCODE = '23514',
        MESSAGE = 'runtime_writer_fence_mutation_rejected';
END;
$function$;

CREATE TRIGGER runtime_writer_fence_reject_row_mutation
BEFORE INSERT OR UPDATE OR DELETE ON public.runtime_writer_fence
FOR EACH ROW
EXECUTE FUNCTION public.reject_runtime_writer_fence_mutation();

CREATE TRIGGER runtime_writer_fence_reject_truncate
BEFORE TRUNCATE ON public.runtime_writer_fence
FOR EACH STATEMENT
EXECUTE FUNCTION public.reject_runtime_writer_fence_mutation();

CREATE FUNCTION public.starring_runtime_writer_fence_observe_v1()
RETURNS TABLE(
    fence_state TEXT,
    fence_generation BIGINT,
    cutover_coordinator_id TEXT,
    cutover_lease_epoch BIGINT,
    database_now TIMESTAMPTZ,
    cutover_expires_at TIMESTAMPTZ
)
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
BEGIN
    PERFORM pg_catalog.pg_advisory_xact_lock_shared(
        pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
    );
    database_now := pg_catalog.clock_timestamp();

    SELECT
        fence.fence_state,
        fence.fence_generation,
        CASE
            WHEN fence.fence_state = 'closed'
                THEN fence.cutover_coordinator_id
            ELSE NULL
        END,
        CASE
            WHEN fence.fence_state = 'closed'
                THEN fence.cutover_lease_epoch_high_water
            ELSE NULL
        END,
        fence.cutover_expires_at
    INTO STRICT
        fence_state,
        fence_generation,
        cutover_coordinator_id,
        cutover_lease_epoch,
        cutover_expires_at
    FROM public.runtime_writer_fence AS fence
    WHERE fence.singleton;

    RETURN NEXT;
END;
$function$;

REVOKE ALL ON TABLE public.runtime_writer_fence FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.reject_runtime_writer_fence_mutation()
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.starring_runtime_writer_fence_observe_v1()
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
            MESSAGE = 'runtime_writer_fence_execution_acl_drift';
    END IF;

    IF executor_grantee IS NOT NULL THEN
        executor_name := pg_catalog.pg_get_userbyid(executor_grantee);
        IF executor_name IS NULL THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_writer_fence_execution_acl_drift';
        END IF;
        EXECUTE pg_catalog.format(
            'GRANT EXECUTE ON FUNCTION public.starring_runtime_writer_fence_observe_v1() TO %I',
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
        '(pg_catalog.to_regclass(''public.runtime_gateway_owners'')),';
    next_fragment := previous_fragment || E'\n' ||
        '            (pg_catalog.to_regclass(''public.runtime_writer_fence'')),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_writer_fence_manifest_relation_patch_drift';
    END IF;
    definition := pg_catalog.replace(definition, previous_fragment, next_fragment);

    previous_fragment :=
        'SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_observe_previous_serving_v1';
    next_fragment :=
        'SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_writer_fence_observe_v1()''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_observe_previous_serving_v1';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_writer_fence_manifest_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(definition, previous_fragment, next_fragment);

    previous_fragment :=
        'RETURN observed_count = 495' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''7853a26f4fca9cd45c863c17350d7d02ab31c2dc8c9f16828a039797e9eb9891'';';
    next_fragment :=
        'RETURN observed_count = 514' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''6f00d0c25506999af7d03eec22ab01513ea4711bbc7dc6eacae2f0f3ce8cd2f5'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_writer_fence_manifest_expectation_patch_drift';
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
        '(''public.runtime_gateway_owners''),';
    next_fragment := previous_fragment || E'\n' ||
        '            (''public.runtime_writer_fence''),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_writer_fence_readiness_relation_patch_drift';
    END IF;
    definition := pg_catalog.replace(definition, previous_fragment, next_fragment);

    previous_fragment :=
        '(''public.reject_runtime_gateway_owner_delete()''),' || E'\n' ||
        '            (''public.reject_ruleset_artifact_mutation()'')';
    next_fragment :=
        '(''public.reject_runtime_gateway_owner_delete()''),' || E'\n' ||
        '            (''public.reject_runtime_writer_fence_mutation()''),' || E'\n' ||
        '            (''public.reject_ruleset_artifact_mutation()'')';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_writer_fence_readiness_protected_patch_drift';
    END IF;
    definition := pg_catalog.replace(definition, previous_fragment, next_fragment);

    previous_fragment :=
        '                ''public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)'',' || E'\n' ||
        '                ''expected_gateway_shard_id text, expected_process_instance_id text, expected_lease_epoch bigint, requested_build_revision text''::TEXT,' || E'\n' ||
        '                ''TABLE(outcome_name text, gateway_shard_id text, process_instance_id text, lease_epoch bigint, expected_build_revision text, owner_revision bigint, database_now timestamp with time zone, expires_at timestamp with time zone)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            )' || E'\n' ||
        '    ) AS expected(';
    next_fragment :=
        '                ''public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)'',' || E'\n' ||
        '                ''expected_gateway_shard_id text, expected_process_instance_id text, expected_lease_epoch bigint, requested_build_revision text''::TEXT,' || E'\n' ||
        '                ''TABLE(outcome_name text, gateway_shard_id text, process_instance_id text, lease_epoch bigint, expected_build_revision text, owner_revision bigint, database_now timestamp with time zone, expires_at timestamp with time zone)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            ),' || E'\n' ||
        '            (' || E'\n' ||
        '                ''public.starring_runtime_writer_fence_observe_v1()'',' || E'\n' ||
        '                ''''::TEXT,' || E'\n' ||
        '                ''TABLE(fence_state text, fence_generation bigint, cutover_coordinator_id text, cutover_lease_epoch bigint, database_now timestamp with time zone, cutover_expires_at timestamp with time zone)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            )' || E'\n' ||
        '    ) AS expected(';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_writer_fence_readiness_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(definition, previous_fragment, next_fragment);

    previous_fragment :=
        '''4b7a0b8daf9868d92edfae0cd83e35d805d27b824ef04a8b4eb06a229caeedf0''::TEXT';
    next_fragment :=
        '''42c6652f5d25634d247821002b619acac6dff1997d6cd1df2ea633d310456061''::TEXT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_writer_fence_readiness_manifest_digest_patch_drift';
    END IF;
    definition := pg_catalog.replace(definition, previous_fragment, next_fragment);

    previous_fragment :=
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)''' || E'\n' ||
        '            )' || E'\n' ||
        '        )';
    next_fragment :=
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)''' || E'\n' ||
        '            ),' || E'\n' ||
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_writer_fence_observe_v1()''' || E'\n' ||
        '            )' || E'\n' ||
        '        )';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_writer_fence_readiness_allowlist_patch_drift';
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
    initial_row_count BIGINT;
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
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_writer_fence')
        AND (
            relation.relkind <> 'r'
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
            )
        );

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_runtime_writer_fence_observe_v1()',
                TRUE,
                TRUE,
                1::REAL,
                TRUE
            ),
            (
                'public.reject_runtime_writer_fence_mutation()',
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
    INTO initial_row_count
    FROM public.runtime_writer_fence AS fence
    WHERE fence.singleton
        AND fence.fence_state = 'open'
        AND fence.fence_generation = 1
        AND fence.cutover_lease_epoch_high_water = 0
        AND fence.cutover_coordinator_id IS NULL
        AND fence.cutover_expires_at IS NULL;

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
        OR pg_catalog.to_regclass('public.runtime_writer_fence') IS NULL
        OR invalid_relation_count <> 0
        OR invalid_function_count <> 0
        OR initial_row_count <> 1
        OR (SELECT pg_catalog.count(*) FROM public.runtime_writer_fence) <> 1
        OR manifest_digest IS DISTINCT FROM
            '42c6652f5d25634d247821002b619acac6dff1997d6cd1df2ea633d310456061'
        OR readiness_digest IS DISTINCT FROM
            'cf84d5a445c591cd11802e9d956c2f03ae7f9c4205134aa1511d4cc40d88fbc3'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_writer_fence_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
