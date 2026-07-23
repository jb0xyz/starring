SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
);

LOCK TABLE
    public.runtime_writer_fence,
    public.runtime_deployments
IN ACCESS EXCLUSIVE MODE;

CREATE TEMPORARY TABLE pg_temp.starring_product_apply_unique_snapshot (
    function_oid OID PRIMARY KEY,
    function_owner OID NOT NULL,
    function_acl ACLITEM[]
) ON COMMIT DROP;

INSERT INTO pg_temp.starring_product_apply_unique_snapshot (
    function_oid,
    function_owner,
    function_acl
)
SELECT
    function_row.oid,
    function_row.proowner,
    function_row.proacl
FROM pg_catalog.pg_proc AS function_row
WHERE function_row.oid = pg_catalog.to_regprocedure(
    'public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)'
);

DO $preflight$
DECLARE
    common_owner OID;
    external_grantee_count BIGINT;
    external_grantee OID;
    invalid_external_acl_count BIGINT;
    invalid_capability_acl_count BIGINT;
    valid_lane_constraint_count BIGINT;
    valid_unresolved_index_count BIGINT;
    invalid_function_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM pg_catalog.pg_proc AS function_row
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)'
        )
        AND (
            function_row.proowner <> common_owner
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
            ) IS DISTINCT FROM
                'c8fd0ff8a91cb0176dfb5ff64e355a79c20f247acd80e97890454c19f73f3765'
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

    SELECT pg_catalog.count(*)
    INTO valid_lane_constraint_count
    FROM pg_catalog.pg_constraint AS constraint_row
    INNER JOIN pg_catalog.pg_class AS relation
        ON relation.oid = constraint_row.conrelid
    INNER JOIN pg_catalog.pg_class AS index_row
        ON index_row.oid = constraint_row.conindid
    INNER JOIN pg_catalog.pg_index AS index_metadata
        ON index_metadata.indexrelid = index_row.oid
    WHERE constraint_row.conname = 'runtime_deployments_lane_generation_unique'
        AND constraint_row.connamespace = pg_catalog.to_regnamespace('public')
        AND relation.oid = pg_catalog.to_regclass('public.runtime_deployments')
        AND relation.relkind = 'r'
        AND relation.relowner = common_owner
        AND NOT relation.relrowsecurity
        AND NOT relation.relforcerowsecurity
        AND constraint_row.contype = 'u'
        AND NOT constraint_row.condeferrable
        AND NOT constraint_row.condeferred
        AND constraint_row.convalidated
        AND index_row.relnamespace = pg_catalog.to_regnamespace('public')
        AND index_row.relname = 'runtime_deployments_lane_generation_unique'
        AND index_row.relkind = 'i'
        AND index_row.relowner = common_owner
        AND index_metadata.indisunique
        AND index_metadata.indisvalid
        AND index_metadata.indisready
        AND index_metadata.indislive
        AND index_metadata.indpred IS NULL
        AND pg_catalog.pg_get_constraintdef(constraint_row.oid)
            = 'UNIQUE (guild_id, ruleset_key, runtime_generation)';

    SELECT pg_catalog.count(*)
    INTO valid_unresolved_index_count
    FROM pg_catalog.pg_class AS index_row
    INNER JOIN pg_catalog.pg_index AS index_metadata
        ON index_metadata.indexrelid = index_row.oid
    WHERE index_row.relnamespace = pg_catalog.to_regnamespace('public')
        AND index_row.relname = 'runtime_deployments_one_unresolved_per_lane'
        AND index_row.relkind = 'i'
        AND index_row.relowner = common_owner
        AND index_metadata.indrelid
            = pg_catalog.to_regclass('public.runtime_deployments')
        AND index_metadata.indisunique
        AND index_metadata.indisvalid
        AND index_metadata.indisready
        AND index_metadata.indislive
        AND NOT index_metadata.indisprimary
        AND NOT index_metadata.indisexclusion
        AND index_metadata.indimmediate
        AND index_metadata.indnkeyatts = 2
        AND index_metadata.indnatts = 2
        AND pg_catalog.pg_get_indexdef(index_row.oid)
            = 'CREATE UNIQUE INDEX runtime_deployments_one_unresolved_per_lane ON public.runtime_deployments USING btree (guild_id, ruleset_key) WHERE (phase <> ALL (ARRAY[''live''::text, ''superseded''::text, ''cancelled''::text]))';

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR invalid_function_count <> 0
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_temp.starring_product_apply_unique_snapshot
        ) <> 1
        OR external_grantee_count > 1
        OR (external_grantee_count = 1 AND external_grantee = 0)
        OR invalid_external_acl_count <> 0
        OR invalid_capability_acl_count <> 0
        OR valid_lane_constraint_count <> 1
        OR valid_unresolved_index_count <> 1
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
            ERRCODE = 'PA001',
            MESSAGE = 'product_apply_unique_normalization_preflight_drift';
    END IF;
END;
$preflight$;

DO $replace$
DECLARE
    definition TEXT;
    insert_marker TEXT;
    clear_marker TEXT;
    insert_fragment TEXT;
    wrapped_fragment TEXT;
    insert_position INTEGER;
    clear_position INTEGER;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)'
    );

    insert_marker := '    INSERT INTO public.runtime_deployments (';
    clear_marker :=
        '    PERFORM pg_catalog.set_config(' || E'\n' ||
        '        ''starring.runtime_mutation_clock'',' || E'\n' ||
        '        '''',' || E'\n' ||
        '        TRUE' || E'\n' ||
        '    );';
    insert_position := pg_catalog.strpos(definition, insert_marker);
    clear_position := pg_catalog.strpos(definition, clear_marker);

    IF insert_position = 0
        OR clear_position <= insert_position
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, insert_marker, ''),
            insert_marker
        ) <> 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, clear_marker, ''),
            clear_marker
        ) <> 0
        OR pg_catalog.strpos(
            definition,
            'runtime_deployments_lane_generation_unique'
        ) <> 0
        OR pg_catalog.strpos(
            definition,
            'runtime_deployments_one_unresolved_per_lane'
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'PA001',
            MESSAGE = 'product_apply_unique_normalization_patch_drift';
    END IF;

    insert_fragment := pg_catalog.substring(
        definition,
        insert_position,
        clear_position - insert_position
    );

    IF pg_catalog.strpos(insert_fragment, 'EXCEPTION') <> 0
        OR pg_catalog.right(insert_fragment, 7) <> '    );' || E'\n'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'PA001',
            MESSAGE = 'product_apply_unique_normalization_fragment_drift';
    END IF;

    wrapped_fragment :=
        '    DECLARE' || E'\n' ||
        '        runtime_unique_schema_name TEXT;' || E'\n' ||
        '        runtime_unique_table_name TEXT;' || E'\n' ||
        '        runtime_unique_constraint_name TEXT;' || E'\n' ||
        '    BEGIN' || E'\n' ||
        '    ' || pg_catalog.replace(
            pg_catalog.rtrim(insert_fragment, E'\n'),
            E'\n',
            E'\n    '
        ) || E'\n' ||
        '    EXCEPTION' || E'\n' ||
        '        WHEN unique_violation THEN' || E'\n' ||
        '            GET STACKED DIAGNOSTICS' || E'\n' ||
        '                runtime_unique_schema_name = SCHEMA_NAME,' || E'\n' ||
        '                runtime_unique_table_name = TABLE_NAME,' || E'\n' ||
        '                runtime_unique_constraint_name = CONSTRAINT_NAME;' || E'\n' ||
        '            IF runtime_unique_schema_name = ''public''' || E'\n' ||
        '                AND runtime_unique_table_name = ''runtime_deployments''' || E'\n' ||
        '                AND runtime_unique_constraint_name IN (' || E'\n' ||
        '                    ''runtime_deployments_lane_generation_unique'',' || E'\n' ||
        '                    ''runtime_deployments_one_unresolved_per_lane''' || E'\n' ||
        '                )' || E'\n' ||
        '            THEN' || E'\n' ||
        '                RAISE EXCEPTION USING' || E'\n' ||
        '                    ERRCODE = ''40001'',' || E'\n' ||
        '                    MESSAGE = ''atomic product apply runtime lane compare-and-swap failed'';' || E'\n' ||
        '            END IF;' || E'\n' ||
        '            RAISE;' || E'\n' ||
        '    END;' || E'\n';

    definition :=
        pg_catalog.substring(definition, 1, insert_position - 1)
        || wrapped_fragment
        || pg_catalog.substring(definition, clear_position);
    EXECUTE definition;
END;
$replace$;

DO $postflight$
DECLARE
    common_owner OID;
    external_grantee_count BIGINT;
    external_grantee OID;
    invalid_external_acl_count BIGINT;
    invalid_capability_acl_count BIGINT;
    invalid_function_count BIGINT;
    snapshot_mismatch_count BIGINT;
    finalizer_source TEXT;
    insert_position INTEGER;
    exception_position INTEGER;
    lane_constraint_position INTEGER;
    unresolved_index_position INTEGER;
    reraise_position INTEGER;
    clear_position INTEGER;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM pg_catalog.pg_proc AS function_row
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)'
        )
        AND (
            function_row.proowner <> common_owner
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
            ) IS DISTINCT FROM
                'ed94feac48946d7067481dd607743295f57f1e13c93231818ebf22d99bc639ac'
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

    SELECT pg_catalog.count(*)
    INTO snapshot_mismatch_count
    FROM pg_temp.starring_product_apply_unique_snapshot AS snapshot
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = snapshot.function_oid
    WHERE function_row.oid IS NULL
        OR function_row.proowner IS DISTINCT FROM snapshot.function_owner
        OR function_row.proacl IS DISTINCT FROM snapshot.function_acl;

    SELECT function_row.prosrc
    INTO finalizer_source
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)'
    );

    insert_position := pg_catalog.strpos(
        finalizer_source,
        'INSERT INTO public.runtime_deployments'
    );
    exception_position := pg_catalog.strpos(
        finalizer_source,
        'WHEN unique_violation THEN'
    );
    lane_constraint_position := pg_catalog.strpos(
        finalizer_source,
        'runtime_deployments_lane_generation_unique'
    );
    unresolved_index_position := pg_catalog.strpos(
        finalizer_source,
        'runtime_deployments_one_unresolved_per_lane'
    );
    reraise_position := pg_catalog.strpos(
        finalizer_source,
        '            RAISE;' || E'\n'
    );
    clear_position := pg_catalog.strpos(
        finalizer_source,
        '''starring.runtime_mutation_clock'',' || E'\n' ||
        '        '''''
    );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR invalid_function_count <> 0
        OR external_grantee_count > 1
        OR (external_grantee_count = 1 AND external_grantee = 0)
        OR invalid_external_acl_count <> 0
        OR invalid_capability_acl_count <> 0
        OR snapshot_mismatch_count <> 0
        OR insert_position = 0
        OR exception_position = 0
        OR lane_constraint_position = 0
        OR unresolved_index_position = 0
        OR reraise_position = 0
        OR clear_position = 0
        OR NOT (
            insert_position < exception_position
            AND exception_position < lane_constraint_position
            AND lane_constraint_position < unresolved_index_position
            AND unresolved_index_position < reraise_position
            AND reraise_position < clear_position
        )
        OR (
            pg_catalog.length(finalizer_source)
            - pg_catalog.length(pg_catalog.replace(
                finalizer_source,
                'WHEN unique_violation THEN',
                ''
            ))
        ) <> pg_catalog.length('WHEN unique_violation THEN')
        OR (
            pg_catalog.length(finalizer_source)
            - pg_catalog.length(pg_catalog.replace(
                finalizer_source,
                'runtime_deployments_lane_generation_unique',
                ''
            ))
        ) <> pg_catalog.length('runtime_deployments_lane_generation_unique')
        OR (
            pg_catalog.length(finalizer_source)
            - pg_catalog.length(pg_catalog.replace(
                finalizer_source,
                'runtime_deployments_one_unresolved_per_lane',
                ''
            ))
        ) <> pg_catalog.length('runtime_deployments_one_unresolved_per_lane')
        OR pg_catalog.strpos(
            finalizer_source,
            'runtime_unique_schema_name = ''public'''
        ) = 0
        OR pg_catalog.strpos(
            finalizer_source,
            'runtime_unique_table_name = ''runtime_deployments'''
        ) = 0
        OR pg_catalog.strpos(
            finalizer_source,
            'GET STACKED DIAGNOSTICS'
        ) = 0
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'PA001',
            MESSAGE = 'product_apply_unique_normalization_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
