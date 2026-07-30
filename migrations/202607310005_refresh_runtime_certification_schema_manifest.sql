SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended(
        'starring-runtime-certification-schema-manifest-v1',
        0
    )
);

DO $runtime_certification_schema_manifest$
DECLARE
    function_identity TEXT :=
        'public.starring_runtime_execution_schema_manifest_v1()';
    function_oid OID;
    function_definition TEXT;
    old_fragment TEXT :=
        E'    RETURN observed_count = 969\n        AND observed_digest\n            = ''462956974d1b225413ead6da18003d29032b267d8d6d86c341d82ba8030a05b6'';\n';
    new_fragment TEXT :=
        E'    RETURN observed_count = 969\n        AND observed_digest\n            = ''ec41d06fbdfce734b673f6e4e7864e428fb153af992c4f3c395a0eb1cd2106a4'';\n';
    expected_definition_digest TEXT :=
        '6731f361eb37f170d4cdb91a1c5931101ef6bc2d16c50e1114a452e05b228f7b';
    observed_definition_digest TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
BEGIN
    function_oid := pg_catalog.to_regprocedure(function_identity);
    IF function_oid IS NULL THEN
        RAISE EXCEPTION
            'runtime certification schema manifest precondition failed'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.jsonb_build_object(
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
    INTO metadata_before
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = function_oid;

    function_definition := pg_catalog.pg_get_functiondef(function_oid);
    IF pg_catalog.char_length(function_definition)
            - pg_catalog.char_length(pg_catalog.replace(
                function_definition,
                old_fragment,
                ''
            ))
        <> pg_catalog.char_length(old_fragment)
    THEN
        RAISE EXCEPTION
            'runtime certification schema manifest replacement precondition failed'
            USING ERRCODE = '55000';
    END IF;

    EXECUTE pg_catalog.replace(
        function_definition,
        old_fragment,
        new_fragment
    );

    SELECT pg_catalog.jsonb_build_object(
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
        OR observed_definition_digest <> expected_definition_digest
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(function_oid),
            old_fragment
        ) <> 0
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(function_oid),
            new_fragment
        ) = 0
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION
            'runtime certification schema manifest replacement failed'
            USING ERRCODE = '55000';
    END IF;
END;
$runtime_certification_schema_manifest$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
