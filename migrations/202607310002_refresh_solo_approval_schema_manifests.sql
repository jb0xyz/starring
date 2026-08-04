DO $solo_approval_schema_manifests$
DECLARE
    function_identities TEXT[] := ARRAY[
        'public.starring_runtime_exact_target_schema_manifest_v1()',
        'public.starring_runtime_exact_target_schema_manifest_v2()',
        'public.starring_runtime_execution_schema_manifest_v1()',
        'public.starring_runtime_serving_schema_manifest_v1()'
    ];
    old_fragments TEXT[] := ARRAY[
        E'    RETURN observed_count = 356\n        AND observed_digest\n            = ''0a33a7e7cc2e3e07b7d06e3d8ec6ad48bba473c2a877ea824f6f341ed4d4e7a7'';\n',
        E'            = ''b8dad14ddbb78262526673ae75a212ca11b1709ba0ee5a54f5125f55da471af7''\n',
        E'    RETURN observed_count = 967\n        AND observed_digest\n            = ''3253c6549e25637015c6640748faccd2fac0e3368e84a2b34b7755611a5d208b'';\n',
        E'    RETURN observed_count = 490\n        AND observed_digest\n            = ''66cf1f0613f92e03f3420cc89a700365a6ac238224275fb26107829c13569e36'';\n'
    ];
    new_fragments TEXT[] := ARRAY[
        E'    RETURN observed_count = 358\n        AND observed_digest\n            = ''d5f52b36ec0e5002d3330ae242e31e6706cab19405c7541ab8cc4a5244637783'';\n',
        E'            = ''aee90f2f78d8106e298c8075b0710bca6d47b3b37cc9d2c6598a4f9f769b9f7d''\n',
        E'    RETURN observed_count = 969\n        AND observed_digest\n            = ''462956974d1b225413ead6da18003d29032b267d8d6d86c341d82ba8030a05b6'';\n',
        E'    RETURN observed_count = 492\n        AND observed_digest\n            = ''723aff77059617f7c7a2d7c7d95f685f3b546527b3e73f2b05fa280bc3db7bed'';\n'
    ];
    function_index INTEGER;
    function_oid OID;
    function_definition TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
BEGIN
    IF pg_catalog.array_length(function_identities, 1) <> 4
        OR pg_catalog.array_length(old_fragments, 1) <> 4
        OR pg_catalog.array_length(new_fragments, 1) <> 4
    THEN
        RAISE EXCEPTION 'solo approval schema manifest cardinality failed'
            USING ERRCODE = '55000';
    END IF;

    FOR function_index IN 1..4
    LOOP
        function_oid := pg_catalog.to_regprocedure(
            function_identities[function_index]
        );
        IF function_oid IS NULL THEN
            RAISE EXCEPTION 'solo approval schema manifest precondition failed'
                USING ERRCODE = '55000';
        END IF;

        SELECT pg_catalog.jsonb_build_object(
            'oid', function_row.oid::TEXT,
            'owner', function_row.proowner::TEXT,
            'acl', pg_catalog.to_jsonb(function_row.proacl),
            'volatile', function_row.provolatile,
            'strict', function_row.proisstrict,
            'security_definer', function_row.prosecdef,
            'parallel', function_row.proparallel,
            'rows', function_row.prorows,
            'config', pg_catalog.to_jsonb(function_row.proconfig)
        )
        INTO metadata_before
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid = function_oid;

        function_definition := pg_catalog.pg_get_functiondef(function_oid);
        IF pg_catalog.char_length(function_definition)
                - pg_catalog.char_length(pg_catalog.replace(
                    function_definition,
                    old_fragments[function_index],
                    ''
                ))
            <> pg_catalog.char_length(old_fragments[function_index])
        THEN
            RAISE EXCEPTION 'solo approval schema manifest replacement precondition failed'
                USING ERRCODE = '55000';
        END IF;

        EXECUTE pg_catalog.replace(
            function_definition,
            old_fragments[function_index],
            new_fragments[function_index]
        );

        SELECT pg_catalog.jsonb_build_object(
            'oid', function_row.oid::TEXT,
            'owner', function_row.proowner::TEXT,
            'acl', pg_catalog.to_jsonb(function_row.proacl),
            'volatile', function_row.provolatile,
            'strict', function_row.proisstrict,
            'security_definer', function_row.prosecdef,
            'parallel', function_row.proparallel,
            'rows', function_row.prorows,
            'config', pg_catalog.to_jsonb(function_row.proconfig)
        )
        INTO metadata_after
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid = function_oid;

        IF metadata_after IS DISTINCT FROM metadata_before
            OR pg_catalog.strpos(
                pg_catalog.pg_get_functiondef(function_oid),
                old_fragments[function_index]
            ) <> 0
            OR pg_catalog.strpos(
                pg_catalog.pg_get_functiondef(function_oid),
                new_fragments[function_index]
            ) = 0
        THEN
            RAISE EXCEPTION 'solo approval schema manifest replacement failed'
                USING ERRCODE = '55000';
        END IF;
    END LOOP;

    IF NOT public.starring_runtime_exact_target_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v2()
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
    THEN
        RAISE EXCEPTION 'solo approval schema manifest postflight failed'
            USING ERRCODE = '55000';
    END IF;
END;
$solo_approval_schema_manifests$;
