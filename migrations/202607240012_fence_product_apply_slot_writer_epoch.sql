SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
);

LOCK TABLE
    public.runtime_writer_fence,
    public.automation_installations,
    public.runtime_slot_writer_fences_v2,
    public.runtime_drain_intents_v2,
    public.runtime_deployments,
    public.activation_requests
IN ACCESS EXCLUSIVE MODE;

CREATE TEMPORARY TABLE pg_temp.starring_product_apply_slot_epoch_snapshot (
    function_oid OID PRIMARY KEY,
    function_owner OID NOT NULL,
    function_acl ACLITEM[]
) ON COMMIT DROP;

INSERT INTO pg_temp.starring_product_apply_slot_epoch_snapshot (
    function_oid,
    function_owner,
    function_acl
)
SELECT
    function_row.oid,
    function_row.proowner,
    function_row.proacl
FROM (
    VALUES
        ('public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'),
        ('public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)')
) AS expected(identity)
INNER JOIN pg_catalog.pg_proc AS function_row
    ON function_row.oid = pg_catalog.to_regprocedure(expected.identity);

DO $preflight$
DECLARE
    common_owner OID;
    invalid_relation_count BIGINT;
    invalid_function_count BIGINT;
    invalid_capability_acl_count BIGINT;
    external_grantee_count BIGINT;
    external_grantee OID;
    invalid_external_acl_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT pg_catalog.count(*)
    INTO invalid_relation_count
    FROM (
        VALUES
            ('public.runtime_writer_fence'),
            ('public.automation_installations'),
            ('public.runtime_slot_writer_fences_v2'),
            ('public.runtime_drain_intents_v2'),
            ('public.runtime_deployments'),
            ('public.activation_requests')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = pg_catalog.to_regclass(expected.identity)
    WHERE relation.oid IS NULL
        OR relation.relkind <> 'r'
        OR relation.relpersistence <> 'p'
        OR relation.relowner <> common_owner
        OR relation.relrowsecurity
        OR relation.relforcerowsecurity;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)',
                '35dff4eac9780b1cea497459ac513c54e5151fc752c290228951fadd6a4c2c2d'::TEXT,
                TRUE,
                TRUE,
                1::REAL,
                FALSE
            ),
            (
                'public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)',
                'f930c836ab241c0aa56376199c0518d8cce5a446406b8503eb3f0b90ec314e38'::TEXT,
                TRUE,
                TRUE,
                1::REAL,
                TRUE
            ),
            (
                'public.starring_product_apply_lock_core_unfenced_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)',
                'abb3775e88f9926af64f676d0f94657c8f3c80890aad2b5372116ec886a464f0'::TEXT,
                TRUE,
                TRUE,
                1::REAL,
                TRUE
            ),
            (
                'public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)',
                'ed94feac48946d7067481dd607743295f57f1e13c93231818ebf22d99bc639ac'::TEXT,
                TRUE,
                TRUE,
                1::REAL,
                FALSE
            ),
            (
                'starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(text,text)',
                '68708aa143de1daae1247b18f3127e2abdc6d269a14e103d24e5ab6732d23f99'::TEXT,
                FALSE,
                TRUE,
                1::REAL,
                TRUE
            ),
            (
                'starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(text,text,bigint)',
                'da6c88ff80cf366e14f2c12a6204964d708156192a292cc6ad71b959588f07b8'::TEXT,
                FALSE,
                FALSE,
                0::REAL,
                TRUE
            )
    ) AS expected(
        identity,
        definition_digest,
        security_definer,
        returns_set,
        rows_estimate,
        owner_only
    )
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
        OR function_row.prosecdef <> expected.security_definer
        OR function_row.proretset <> expected.returns_set
        OR function_row.prorows <> expected.rows_estimate
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM 'plpgsql'
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(function_row.oid),
                'UTF8'
            )),
            'hex'
        ) IS DISTINCT FROM expected.definition_digest
        OR (
            expected.owner_only
            AND EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE privilege.grantee <> common_owner
            )
        );

    SELECT
        pg_catalog.count(*),
        pg_catalog.min(privilege.grantee::BIGINT)::OID,
        pg_catalog.count(*) FILTER (
            WHERE privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
                OR privilege.grantor <> common_owner
        )
    INTO
        external_grantee_count,
        external_grantee,
        invalid_external_acl_count
    FROM pg_catalog.pg_proc AS function_row
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'
        )
        AND privilege.grantee <> common_owner;

    SELECT pg_catalog.count(*)
    INTO invalid_capability_acl_count
    FROM (
        VALUES
            ('public.starring_product_apply_executor_database_identity_v1()'),
            ('public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'),
            ('public.starring_product_apply_target_artifact_v1(text,text,text,text,bytea,text,text)'),
            ('public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)'),
            ('public.starring_product_apply_keyring_coverage_v1(text[],text[])')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
        ) <> external_grantee_count
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
                AND (
                    external_grantee_count <> 1
                    OR privilege.grantee IS DISTINCT FROM external_grantee
                    OR privilege.grantor <> common_owner
                    OR privilege.privilege_type <> 'EXECUTE'
                    OR privilege.is_grantable
                )
        );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR invalid_relation_count <> 0
        OR invalid_function_count <> 0
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_temp.starring_product_apply_slot_epoch_snapshot
        ) <> 2
        OR external_grantee_count > 1
        OR (external_grantee_count = 1 AND external_grantee = 0)
        OR invalid_external_acl_count <> 0
        OR invalid_capability_acl_count <> 0
        OR (SELECT pg_catalog.count(*) FROM public.runtime_writer_fence) <> 1
        OR NOT EXISTS (
            SELECT 1
            FROM public.runtime_writer_fence AS fence
            WHERE fence.singleton
                AND fence.fence_state IN ('open', 'closed')
        )
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v1()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'PA001',
            MESSAGE = 'product_apply_slot_writer_epoch_preflight_drift';
    END IF;
END;
$preflight$;

DO $patch_wrapper$
DECLARE
    definition TEXT;
    previous_declaration TEXT;
    next_declaration TEXT;
    previous_physical_lock TEXT;
    next_physical_lock TEXT;
    previous_delegation_start TEXT;
    next_delegation_start TEXT;
    previous_delegation_end TEXT;
    next_delegation_end TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'
    );

    previous_declaration :=
        '    serving_slot_ruleset_key TEXT;' || E'\n' ||
        'BEGIN';
    next_declaration :=
        '    serving_slot_ruleset_key TEXT;' || E'\n' ||
        '    slot_pending_drain_intent_id TEXT;' || E'\n' ||
        '    core_row RECORD;' || E'\n' ||
        'BEGIN';
    previous_physical_lock :=
        '        PERFORM deployment.deployment_id' || E'\n' ||
        '        FROM public.runtime_deployments AS deployment';
    next_physical_lock :=
        '        SELECT slot_fence.pending_drain_intent_id' || E'\n' ||
        '        INTO slot_pending_drain_intent_id' || E'\n' ||
        '        FROM starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(' || E'\n' ||
        '            serving_slot_guild_id,' || E'\n' ||
        '            serving_slot_ruleset_key' || E'\n' ||
        '        ) AS slot_fence;' || E'\n' ||
        '' || E'\n' ||
        '        PERFORM deployment.deployment_id' || E'\n' ||
        '        FROM public.runtime_deployments AS deployment';
    previous_delegation_start :=
        '    RETURN QUERY' || E'\n' ||
        '    SELECT core.*' || E'\n' ||
        '    FROM public.starring_product_apply_lock_core_unfenced_v1(';
    next_delegation_start :=
        '    SELECT *' || E'\n' ||
        '    INTO core_row' || E'\n' ||
        '    FROM public.starring_product_apply_lock_core_unfenced_v1(';
    previous_delegation_end :=
        '    ) AS core;' || E'\n' ||
        'END;';
    next_delegation_end :=
        '    ) AS core;' || E'\n' ||
        '' || E'\n' ||
        '    IF core_row.outcome = ''ready''' || E'\n' ||
        '        AND slot_pending_drain_intent_id IS NOT NULL' || E'\n' ||
        '    THEN' || E'\n' ||
        '        RETURN QUERY SELECT ''runtime_drain_required'', FALSE, FALSE,' || E'\n' ||
        '            NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;' || E'\n' ||
        '        RETURN;' || E'\n' ||
        '    END IF;' || E'\n' ||
        '' || E'\n' ||
        '    RETURN QUERY SELECT' || E'\n' ||
        '        core_row.outcome,' || E'\n' ||
        '        core_row.exact_replay,' || E'\n' ||
        '        core_row.requires_commit,' || E'\n' ||
        '        core_row.resulting_revision,' || E'\n' ||
        '        core_row.resulting_state,' || E'\n' ||
        '        core_row.deployment_id,' || E'\n' ||
        '        core_row.desired_target_digest,' || E'\n' ||
        '        core_row.locked_projection;' || E'\n' ||
        'END;';

    IF pg_catalog.strpos(definition, previous_declaration) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_declaration, ''),
            previous_declaration
        ) <> 0
        OR pg_catalog.strpos(definition, previous_physical_lock) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_physical_lock, ''),
            previous_physical_lock
        ) <> 0
        OR pg_catalog.strpos(definition, previous_delegation_start) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_delegation_start, ''),
            previous_delegation_start
        ) <> 0
        OR pg_catalog.strpos(definition, previous_delegation_end) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_delegation_end, ''),
            previous_delegation_end
        ) <> 0
        OR pg_catalog.strpos(
            definition,
            'starring_runtime_slot_writer_fence_lock_v2'
        ) <> 0
        OR pg_catalog.strpos(
            definition,
            'starring_runtime_slot_writer_fence_begin_unsafe_v2'
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'PA001',
            MESSAGE = 'product_apply_slot_writer_epoch_wrapper_patch_drift';
    END IF;

    definition := pg_catalog.replace(
        definition,
        previous_declaration,
        next_declaration
    );
    definition := pg_catalog.replace(
        definition,
        previous_physical_lock,
        next_physical_lock
    );
    definition := pg_catalog.replace(
        definition,
        previous_delegation_start,
        next_delegation_start
    );
    definition := pg_catalog.replace(
        definition,
        previous_delegation_end,
        next_delegation_end
    );
    EXECUTE definition;
END;
$patch_wrapper$;

DO $patch_finalize$
DECLARE
    definition TEXT;
    previous_declaration TEXT;
    next_declaration TEXT;
    previous_mutation TEXT;
    next_mutation TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)'
    );

    previous_declaration :=
        '    applied_completion_kind TEXT;' || E'\n' ||
        'BEGIN';
    next_declaration :=
        '    applied_completion_kind TEXT;' || E'\n' ||
        '    slot_writer_epoch BIGINT;' || E'\n' ||
        'BEGIN';
    previous_mutation :=
        '    PERFORM pg_catalog.set_config(' || E'\n' ||
        '        ''starring.product_approval_context_digest'',' || E'\n' ||
        '        locked_projection #>> ''{server,activation,approval_context_digest}'',' || E'\n' ||
        '        TRUE' || E'\n' ||
        '    );';
    next_mutation :=
        '    SELECT slot_fence.writer_epoch' || E'\n' ||
        '    INTO slot_writer_epoch' || E'\n' ||
        '    FROM starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(' || E'\n' ||
        '        expected_guild_id,' || E'\n' ||
        '        locked_projection #>> ''{server,target,ruleset_key}''' || E'\n' ||
        '    ) AS slot_fence;' || E'\n' ||
        '' || E'\n' ||
        '    PERFORM starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(' || E'\n' ||
        '        expected_guild_id,' || E'\n' ||
        '        locked_projection #>> ''{server,target,ruleset_key}'',' || E'\n' ||
        '        slot_writer_epoch' || E'\n' ||
        '    );' || E'\n' ||
        '' || E'\n' ||
        '    PERFORM pg_catalog.set_config(' || E'\n' ||
        '        ''starring.product_approval_context_digest'',' || E'\n' ||
        '        locked_projection #>> ''{server,activation,approval_context_digest}'',' || E'\n' ||
        '        TRUE' || E'\n' ||
        '    );';

    IF pg_catalog.strpos(definition, previous_declaration) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_declaration, ''),
            previous_declaration
        ) <> 0
        OR pg_catalog.strpos(definition, previous_mutation) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_mutation, ''),
            previous_mutation
        ) <> 0
        OR pg_catalog.strpos(
            definition,
            'starring_runtime_slot_writer_fence_lock_v2'
        ) <> 0
        OR pg_catalog.strpos(
            definition,
            'starring_runtime_slot_writer_fence_begin_unsafe_v2'
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'PA001',
            MESSAGE = 'product_apply_slot_writer_epoch_finalize_patch_drift';
    END IF;

    definition := pg_catalog.replace(
        definition,
        previous_declaration,
        next_declaration
    );
    definition := pg_catalog.replace(
        definition,
        previous_mutation,
        next_mutation
    );
    EXECUTE definition;
END;
$patch_finalize$;

DO $postflight$
DECLARE
    common_owner OID;
    invalid_function_count BIGINT;
    snapshot_mismatch_count BIGINT;
    wrapper_source TEXT;
    finalizer_source TEXT;
    wrapper_global_position INTEGER;
    wrapper_global_row_position INTEGER;
    wrapper_global_share_position INTEGER;
    wrapper_slot_position INTEGER;
    wrapper_physical_position INTEGER;
    wrapper_deployment_position INTEGER;
    wrapper_delegate_position INTEGER;
    wrapper_pending_position INTEGER;
    wrapper_ready_position INTEGER;
    wrapper_drain_required_position INTEGER;
    wrapper_passthrough_position INTEGER;
    finalizer_lock_position INTEGER;
    finalizer_replay_position INTEGER;
    finalizer_projection_position INTEGER;
    finalizer_physical_position INTEGER;
    finalizer_begin_position INTEGER;
    finalizer_activation_position INTEGER;
    finalizer_runtime_position INTEGER;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)',
                'cfaa7fbfcd2655dc3f0ac9f2b79833ca4ef8fe68fa10553ae9a1fee1d925e732'::TEXT
            ),
            (
                'public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)',
                '995457d082714a854257c2020ef854b6d0292302ca755182ac798c0225d1e715'::TEXT
            )
    ) AS expected(identity, definition_digest)
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
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM 'plpgsql'
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(function_row.oid),
                'UTF8'
            )),
            'hex'
        ) IS DISTINCT FROM expected.definition_digest;

    SELECT pg_catalog.count(*)
    INTO snapshot_mismatch_count
    FROM pg_temp.starring_product_apply_slot_epoch_snapshot AS snapshot
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = snapshot.function_oid
    WHERE function_row.oid IS NULL
        OR function_row.proowner IS DISTINCT FROM snapshot.function_owner
        OR function_row.proacl IS DISTINCT FROM snapshot.function_acl;

    SELECT function_row.prosrc
    INTO wrapper_source
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'
    );

    SELECT function_row.prosrc
    INTO finalizer_source
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)'
    );

    wrapper_global_position := pg_catalog.strpos(
        wrapper_source,
        'pg_advisory_xact_lock_shared'
    );
    wrapper_global_row_position := pg_catalog.strpos(
        wrapper_source,
        'FROM public.runtime_writer_fence AS fence'
    );
    wrapper_global_share_position := pg_catalog.strpos(
        wrapper_source,
        'FOR SHARE'
    );
    wrapper_slot_position := pg_catalog.strpos(
        wrapper_source,
        'starring-runtime-serving-slot-v1:'
    );
    wrapper_physical_position := pg_catalog.strpos(
        wrapper_source,
        'starring_runtime_slot_writer_fence_lock_v2'
    );
    wrapper_deployment_position := pg_catalog.strpos(
        wrapper_source,
        'FROM public.runtime_deployments AS deployment'
    );
    wrapper_delegate_position := pg_catalog.strpos(
        wrapper_source,
        'starring_product_apply_lock_core_unfenced_v1'
    );
    wrapper_pending_position := pg_catalog.strpos(
        wrapper_source,
        'slot_pending_drain_intent_id IS NOT NULL'
    );
    wrapper_ready_position := pg_catalog.strpos(
        wrapper_source,
        'core_row.outcome = ''ready'''
    );
    wrapper_drain_required_position := pg_catalog.strpos(
        wrapper_source,
        'runtime_drain_required'
    );
    wrapper_passthrough_position := pg_catalog.strpos(
        wrapper_source,
        'core_row.exact_replay'
    );
    finalizer_lock_position := pg_catalog.strpos(
        finalizer_source,
        'FROM public.starring_product_apply_lock_v1'
    );
    finalizer_replay_position := pg_catalog.strpos(
        finalizer_source,
        'lock_row.exact_replay'
    );
    finalizer_projection_position := pg_catalog.strpos(
        finalizer_source,
        'prepared_desired_target_digest IS DISTINCT FROM'
    );
    finalizer_physical_position := pg_catalog.strpos(
        finalizer_source,
        'starring_runtime_slot_writer_fence_lock_v2'
    );
    finalizer_begin_position := pg_catalog.strpos(
        finalizer_source,
        'starring_runtime_slot_writer_fence_begin_unsafe_v2'
    );
    finalizer_activation_position := pg_catalog.strpos(
        finalizer_source,
        'UPDATE public.activation_requests AS activation'
    );
    finalizer_runtime_position := pg_catalog.strpos(
        finalizer_source,
        'INSERT INTO public.runtime_deployments'
    );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR invalid_function_count <> 0
        OR snapshot_mismatch_count <> 0
        OR wrapper_global_position = 0
        OR wrapper_global_row_position = 0
        OR wrapper_global_share_position = 0
        OR wrapper_slot_position = 0
        OR wrapper_physical_position = 0
        OR wrapper_deployment_position = 0
        OR wrapper_delegate_position = 0
        OR wrapper_pending_position = 0
        OR wrapper_ready_position = 0
        OR wrapper_drain_required_position = 0
        OR wrapper_passthrough_position = 0
        OR finalizer_lock_position = 0
        OR finalizer_replay_position = 0
        OR finalizer_projection_position = 0
        OR finalizer_physical_position = 0
        OR finalizer_begin_position = 0
        OR finalizer_activation_position = 0
        OR finalizer_runtime_position = 0
        OR NOT (
            wrapper_global_position < wrapper_global_row_position
            AND wrapper_global_row_position < wrapper_global_share_position
            AND wrapper_global_share_position < wrapper_slot_position
            AND wrapper_slot_position < wrapper_physical_position
            AND wrapper_physical_position < wrapper_deployment_position
            AND wrapper_deployment_position < wrapper_delegate_position
            AND wrapper_delegate_position < wrapper_ready_position
            AND wrapper_ready_position < wrapper_pending_position
            AND wrapper_pending_position < wrapper_drain_required_position
            AND wrapper_drain_required_position < wrapper_passthrough_position
            AND finalizer_lock_position < finalizer_replay_position
            AND finalizer_replay_position < finalizer_projection_position
            AND finalizer_projection_position < finalizer_physical_position
            AND finalizer_physical_position < finalizer_begin_position
            AND finalizer_begin_position < finalizer_activation_position
            AND finalizer_activation_position < finalizer_runtime_position
        )
        OR (
            pg_catalog.length(wrapper_source)
            - pg_catalog.length(pg_catalog.replace(
                wrapper_source,
                'starring_runtime_slot_writer_fence_lock_v2',
                ''
            ))
        ) <> pg_catalog.length(
            'starring_runtime_slot_writer_fence_lock_v2'
        )
        OR pg_catalog.strpos(
            wrapper_source,
            'starring_runtime_slot_writer_fence_begin_unsafe_v2'
        ) <> 0
        OR (
            pg_catalog.length(finalizer_source)
            - pg_catalog.length(pg_catalog.replace(
                finalizer_source,
                'starring_runtime_slot_writer_fence_lock_v2',
                ''
            ))
        ) <> pg_catalog.length(
            'starring_runtime_slot_writer_fence_lock_v2'
        )
        OR (
            pg_catalog.length(finalizer_source)
            - pg_catalog.length(pg_catalog.replace(
                finalizer_source,
                'starring_runtime_slot_writer_fence_begin_unsafe_v2',
                ''
            ))
        ) <> pg_catalog.length(
            'starring_runtime_slot_writer_fence_begin_unsafe_v2'
        )
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v1()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'PA001',
            MESSAGE = 'product_apply_slot_writer_epoch_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
