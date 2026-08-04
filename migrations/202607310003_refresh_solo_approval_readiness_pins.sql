DO $solo_approval_readiness_pins$
DECLARE
    function_identities TEXT[] := ARRAY[
        'public.starring_runtime_execution_database_readiness_v1()',
        'public.starring_runtime_exact_target_database_readiness_v2()',
        'public.starring_runtime_serving_database_readiness_v1()'
    ];
    old_fragments TEXT[] := ARRAY[
        '644a9c08a9b4a216e45db4a9eae308dfcce726e9f37e8816f8f83049a92cf474',
        'e6b483ea123b1a235652088acd2c4229c24042c21ac407fc8dd4ae97c809489f',
        '7791925b08af642fe3f42d099394e42301086db580dd239b557b73c5640d1811'
    ];
    new_fragments TEXT[] := ARRAY[
        '4e61e3e8a9769f8ef9d1c68bde97cde6be5fb80d54cc5cba1aa5999e34d83bfa',
        'b42afbec22a0531a64708ad8bb3c7f26d73609bd4f0e1e80ec6d0602e98cc966',
        '644b932f77a787089cb71a273be7e56c5e226f06287b5f6a23a0c4d9bbcff762'
    ];
    readiness_digests TEXT[] := ARRAY[
        '1d00ad69e8c2633713f35670b831d274329d24fb3e7410b13d429a19b5fb7c34',
        '2b28b5bac9a444333d1681ccc158243d8a0d010818fa0719374699f8d0275c43',
        '977410de87917e582c6018c0ddcea164045b82a4550fb166ff138e3efa65238d'
    ];
    function_index INTEGER;
    function_oid OID;
    function_definition TEXT;
    observed_digest TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
BEGIN
    IF pg_catalog.array_length(function_identities, 1) <> 3
        OR pg_catalog.array_length(old_fragments, 1) <> 3
        OR pg_catalog.array_length(new_fragments, 1) <> 3
        OR pg_catalog.array_length(readiness_digests, 1) <> 3
    THEN
        RAISE EXCEPTION 'solo approval readiness cardinality failed'
            USING ERRCODE = '55000';
    END IF;

    FOR function_index IN 1..3
    LOOP
        function_oid := pg_catalog.to_regprocedure(
            function_identities[function_index]
        );
        IF function_oid IS NULL THEN
            RAISE EXCEPTION 'solo approval readiness precondition failed'
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
            RAISE EXCEPTION 'solo approval readiness replacement precondition failed'
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
        ),
        pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(function_row.oid),
                'UTF8'
            )),
            'hex'
        )
        INTO metadata_after, observed_digest
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid = function_oid;

        IF metadata_after IS DISTINCT FROM metadata_before
            OR observed_digest <> readiness_digests[function_index]
            OR pg_catalog.strpos(
                pg_catalog.pg_get_functiondef(function_oid),
                old_fragments[function_index]
            ) <> 0
            OR pg_catalog.strpos(
                pg_catalog.pg_get_functiondef(function_oid),
                new_fragments[function_index]
            ) = 0
        THEN
            RAISE EXCEPTION 'solo approval readiness replacement failed'
                USING ERRCODE = '55000';
        END IF;
    END LOOP;
END;
$solo_approval_readiness_pins$;
