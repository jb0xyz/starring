SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
);

LOCK TABLE public.runtime_deployments IN ACCESS SHARE MODE;

DO $refresh_primary_manifests$
DECLARE
    function_identities TEXT[] := ARRAY[
        'public.starring_runtime_exact_target_schema_manifest_v1()',
        'public.starring_runtime_serving_schema_manifest_v1()'
    ];
    old_fragments TEXT[] := ARRAY[
        'd5f52b36ec0e5002d3330ae242e31e6706cab19405c7541ab8cc4a5244637783',
        '723aff77059617f7c7a2d7c7d95f685f3b546527b3e73f2b05fa280bc3db7bed'
    ];
    new_fragments TEXT[] := ARRAY[
        '4f0faaa39110eabbdfb432ff7437776adb1044911f6e3fe4aad64e529a4fa02a',
        '095b56dd1d761868765c6e21aaf49bdbeed86bc2be95218c191acefdd12a6047'
    ];
    expected_definition_digests TEXT[] := ARRAY[
        'c8e5559234a54c8b4b3be342a98badc0f63d3fb4ae59beea50d105938730ec7d',
        '012e4f8c1dcde470f395360a50e443f80360b868c119b71b612ee20c983801ab'
    ];
    function_index INTEGER;
    function_oid OID;
    definition TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
    observed_definition_digest TEXT;
BEGIN
    IF pg_catalog.to_regrole(current_user) IS DISTINCT FROM (
            SELECT relation.relowner
            FROM pg_catalog.pg_class AS relation
            WHERE relation.oid = pg_catalog.to_regclass(
                'public.runtime_deployments'
            )
        )
        OR pg_catalog.array_length(function_identities, 1) <> 2
        OR pg_catalog.array_length(old_fragments, 1) <> 2
        OR pg_catalog.array_length(new_fragments, 1) <> 2
        OR pg_catalog.array_length(expected_definition_digests, 1) <> 2
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_cross_capability_primary_precondition_drift';
    END IF;

    FOR function_index IN 1..2
    LOOP
        function_oid := pg_catalog.to_regprocedure(
            function_identities[function_index]
        );
        SELECT
            pg_catalog.pg_get_functiondef(function_row.oid),
            pg_catalog.jsonb_build_object(
                'oid', function_row.oid::TEXT,
                'owner', function_row.proowner::TEXT,
                'acl', pg_catalog.to_jsonb(function_row.proacl),
                'language', function_row.prolang::TEXT,
                'kind', function_row.prokind,
                'volatile', function_row.provolatile,
                'strict', function_row.proisstrict,
                'security_definer', function_row.prosecdef,
                'parallel', function_row.proparallel,
                'returns_set', function_row.proretset,
                'rows', function_row.prorows,
                'config', pg_catalog.to_jsonb(function_row.proconfig),
                'leakproof', function_row.proleakproof,
                'argument_defaults', function_row.pronargdefaults,
                'variadic', function_row.provariadic::TEXT,
                'return_type', function_row.prorettype::TEXT
            )
        INTO definition, metadata_before
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid = function_oid;

        IF definition IS NULL
            OR pg_catalog.char_length(definition)
                - pg_catalog.char_length(pg_catalog.replace(
                    definition,
                    old_fragments[function_index],
                    ''
                ))
                <> pg_catalog.char_length(old_fragments[function_index])
            OR pg_catalog.strpos(
                definition,
                new_fragments[function_index]
            ) <> 0
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_cross_capability_primary_definition_drift';
        END IF;

        EXECUTE pg_catalog.replace(
            definition,
            old_fragments[function_index],
            new_fragments[function_index]
        );

        SELECT
            pg_catalog.jsonb_build_object(
                'oid', function_row.oid::TEXT,
                'owner', function_row.proowner::TEXT,
                'acl', pg_catalog.to_jsonb(function_row.proacl),
                'language', function_row.prolang::TEXT,
                'kind', function_row.prokind,
                'volatile', function_row.provolatile,
                'strict', function_row.proisstrict,
                'security_definer', function_row.prosecdef,
                'parallel', function_row.proparallel,
                'returns_set', function_row.proretset,
                'rows', function_row.prorows,
                'config', pg_catalog.to_jsonb(function_row.proconfig),
                'leakproof', function_row.proleakproof,
                'argument_defaults', function_row.pronargdefaults,
                'variadic', function_row.provariadic::TEXT,
                'return_type', function_row.prorettype::TEXT
            ),
            pg_catalog.encode(
                pg_catalog.sha256(pg_catalog.convert_to(
                    pg_catalog.pg_get_functiondef(function_row.oid),
                    'UTF8'
                )),
                'hex'
            )
        INTO metadata_after, observed_definition_digest
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid = function_oid;

        IF metadata_after IS DISTINCT FROM metadata_before
            OR observed_definition_digest
                <> expected_definition_digests[function_index]
            OR pg_catalog.strpos(
                pg_catalog.pg_get_functiondef(function_oid),
                old_fragments[function_index]
            ) <> 0
            OR pg_catalog.strpos(
                pg_catalog.pg_get_functiondef(function_oid),
                new_fragments[function_index]
            ) = 0
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_cross_capability_primary_postcondition_drift';
        END IF;
    END LOOP;
END;
$refresh_primary_manifests$;

DO $refresh_exact_target_wrapper$
DECLARE
    function_oid OID;
    definition TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
    observed_definition_digest TEXT;
BEGIN
    function_oid := pg_catalog.to_regprocedure(
        'public.starring_runtime_exact_target_schema_manifest_v2()'
    );
    SELECT
        pg_catalog.pg_get_functiondef(function_row.oid),
        pg_catalog.jsonb_build_object(
            'oid', function_row.oid::TEXT,
            'owner', function_row.proowner::TEXT,
            'acl', pg_catalog.to_jsonb(function_row.proacl),
            'language', function_row.prolang::TEXT,
            'kind', function_row.prokind,
            'volatile', function_row.provolatile,
            'strict', function_row.proisstrict,
            'security_definer', function_row.prosecdef,
            'parallel', function_row.proparallel,
            'returns_set', function_row.proretset,
            'rows', function_row.prorows,
            'config', pg_catalog.to_jsonb(function_row.proconfig),
            'leakproof', function_row.proleakproof,
            'argument_defaults', function_row.pronargdefaults,
            'variadic', function_row.provariadic::TEXT,
            'return_type', function_row.prorettype::TEXT
        )
    INTO definition, metadata_before
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = function_oid;

    IF definition IS NULL
        OR pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                'aee90f2f78d8106e298c8075b0710bca6d47b3b37cc9d2c6598a4f9f769b9f7d',
                ''
            ))
            <> 64
        OR pg_catalog.strpos(
            definition,
            'c8e5559234a54c8b4b3be342a98badc0f63d3fb4ae59beea50d105938730ec7d'
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_cross_capability_wrapper_precondition_drift';
    END IF;

    EXECUTE pg_catalog.replace(
        definition,
        'aee90f2f78d8106e298c8075b0710bca6d47b3b37cc9d2c6598a4f9f769b9f7d',
        'c8e5559234a54c8b4b3be342a98badc0f63d3fb4ae59beea50d105938730ec7d'
    );

    SELECT
        pg_catalog.jsonb_build_object(
            'oid', function_row.oid::TEXT,
            'owner', function_row.proowner::TEXT,
            'acl', pg_catalog.to_jsonb(function_row.proacl),
            'language', function_row.prolang::TEXT,
            'kind', function_row.prokind,
            'volatile', function_row.provolatile,
            'strict', function_row.proisstrict,
            'security_definer', function_row.prosecdef,
            'parallel', function_row.proparallel,
            'returns_set', function_row.proretset,
            'rows', function_row.prorows,
            'config', pg_catalog.to_jsonb(function_row.proconfig),
            'leakproof', function_row.proleakproof,
            'argument_defaults', function_row.pronargdefaults,
            'variadic', function_row.provariadic::TEXT,
            'return_type', function_row.prorettype::TEXT
        ),
        pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(function_row.oid),
                'UTF8'
            )),
            'hex'
        )
    INTO metadata_after, observed_definition_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = function_oid;

    IF metadata_after IS DISTINCT FROM metadata_before
        OR observed_definition_digest
            <> '3f6a6a99409f21b6d1af71ecd87f86024b0a4f0c939f1d6dbea558d9991e7612'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_cross_capability_wrapper_postcondition_drift';
    END IF;
END;
$refresh_exact_target_wrapper$;

DO $refresh_readiness$
DECLARE
    function_identities TEXT[] := ARRAY[
        'public.starring_runtime_exact_target_database_readiness_v2()',
        'public.starring_runtime_serving_database_readiness_v1()'
    ];
    old_fragments TEXT[] := ARRAY[
        'b42afbec22a0531a64708ad8bb3c7f26d73609bd4f0e1e80ec6d0602e98cc966',
        '644b932f77a787089cb71a273be7e56c5e226f06287b5f6a23a0c4d9bbcff762'
    ];
    new_fragments TEXT[] := ARRAY[
        '3f6a6a99409f21b6d1af71ecd87f86024b0a4f0c939f1d6dbea558d9991e7612',
        '012e4f8c1dcde470f395360a50e443f80360b868c119b71b612ee20c983801ab'
    ];
    expected_definition_digests TEXT[] := ARRAY[
        '3ada22bd8ca9b0eec6528ec9f6bff320c9bf29d816ee00d24a9cdec592aa359b',
        '16ac5e4726c5ab72da45c1ab67490a50e737197d79a435133fcbd27b56f79a15'
    ];
    function_index INTEGER;
    function_oid OID;
    definition TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
    observed_definition_digest TEXT;
BEGIN
    FOR function_index IN 1..2
    LOOP
        function_oid := pg_catalog.to_regprocedure(
            function_identities[function_index]
        );
        SELECT
            pg_catalog.pg_get_functiondef(function_row.oid),
            pg_catalog.jsonb_build_object(
                'oid', function_row.oid::TEXT,
                'owner', function_row.proowner::TEXT,
                'acl', pg_catalog.to_jsonb(function_row.proacl),
                'language', function_row.prolang::TEXT,
                'kind', function_row.prokind,
                'volatile', function_row.provolatile,
                'strict', function_row.proisstrict,
                'security_definer', function_row.prosecdef,
                'parallel', function_row.proparallel,
                'returns_set', function_row.proretset,
                'rows', function_row.prorows,
                'config', pg_catalog.to_jsonb(function_row.proconfig),
                'leakproof', function_row.proleakproof,
                'argument_defaults', function_row.pronargdefaults,
                'variadic', function_row.provariadic::TEXT,
                'return_type', function_row.prorettype::TEXT
            )
        INTO definition, metadata_before
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid = function_oid;

        IF definition IS NULL
            OR pg_catalog.char_length(definition)
                - pg_catalog.char_length(pg_catalog.replace(
                    definition,
                    old_fragments[function_index],
                    ''
                ))
                <> pg_catalog.char_length(old_fragments[function_index])
            OR pg_catalog.strpos(
                definition,
                new_fragments[function_index]
            ) <> 0
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_cross_capability_readiness_definition_drift';
        END IF;

        EXECUTE pg_catalog.replace(
            definition,
            old_fragments[function_index],
            new_fragments[function_index]
        );

        SELECT
            pg_catalog.jsonb_build_object(
                'oid', function_row.oid::TEXT,
                'owner', function_row.proowner::TEXT,
                'acl', pg_catalog.to_jsonb(function_row.proacl),
                'language', function_row.prolang::TEXT,
                'kind', function_row.prokind,
                'volatile', function_row.provolatile,
                'strict', function_row.proisstrict,
                'security_definer', function_row.prosecdef,
                'parallel', function_row.proparallel,
                'returns_set', function_row.proretset,
                'rows', function_row.prorows,
                'config', pg_catalog.to_jsonb(function_row.proconfig),
                'leakproof', function_row.proleakproof,
                'argument_defaults', function_row.pronargdefaults,
                'variadic', function_row.provariadic::TEXT,
                'return_type', function_row.prorettype::TEXT
            ),
            pg_catalog.encode(
                pg_catalog.sha256(pg_catalog.convert_to(
                    pg_catalog.pg_get_functiondef(function_row.oid),
                    'UTF8'
                )),
                'hex'
            )
        INTO metadata_after, observed_definition_digest
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid = function_oid;

        IF metadata_after IS DISTINCT FROM metadata_before
            OR observed_definition_digest
                <> expected_definition_digests[function_index]
            OR pg_catalog.strpos(
                pg_catalog.pg_get_functiondef(function_oid),
                old_fragments[function_index]
            ) <> 0
            OR pg_catalog.strpos(
                pg_catalog.pg_get_functiondef(function_oid),
                new_fragments[function_index]
            ) = 0
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_cross_capability_readiness_postcondition_drift';
        END IF;
    END LOOP;
END;
$refresh_readiness$;

DO $postflight$
BEGIN
    IF NOT public.starring_runtime_exact_target_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v2()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_exact_target_database_readiness_v2()'
                    )
                ),
                'UTF8'
            )),
            'hex'
        ) <> '3ada22bd8ca9b0eec6528ec9f6bff320c9bf29d816ee00d24a9cdec592aa359b'
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_serving_database_readiness_v1()'
                    )
                ),
                'UTF8'
            )),
            'hex'
        ) <> '16ac5e4726c5ab72da45c1ab67490a50e737197d79a435133fcbd27b56f79a15'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_cross_capability_readiness_postflight_drift';
    END IF;
END;
$postflight$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
