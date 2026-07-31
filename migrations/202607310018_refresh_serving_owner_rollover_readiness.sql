SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended(
        'starring-runtime-serving-owner-rollover-readiness-v1',
        0
    )
);

DO $serving_owner_rollover_readiness$
DECLARE
    function_identity TEXT :=
        'public.starring_runtime_serving_database_readiness_v1()';
    function_oid OID;
    function_definition TEXT;
    old_fragment TEXT :=
        'ad683aca71f9271021b9d80293ca99fc307473a78f04373f7dfdfc531bf6adcd';
    new_fragment TEXT :=
        '4704438e7b783e5507f7c0f3fdb8629a37d046a8e5faa5e530e65ad7e8580b7a';
    old_definition_digest TEXT :=
        '01bd850ba45c6ee7a2edfbe0bae8cdef79f53c219d05ec90549076ae227a6f71';
    expected_definition_digest TEXT :=
        '691b12d79fc52a9a7b817c584bfcac3a25fa2e9f5f841e5954a535405cb3e191';
    observed_definition_digest TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
BEGIN
    function_oid := pg_catalog.to_regprocedure(function_identity);

    SELECT
        pg_catalog.pg_get_functiondef(function_row.oid),
        pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(function_row.oid),
                'UTF8'
            )),
            'hex'
        ),
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
    INTO
        function_definition,
        observed_definition_digest,
        metadata_before
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = function_oid;

    IF function_definition IS NULL
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR observed_definition_digest <> old_definition_digest
        OR pg_catalog.char_length(function_definition)
            - pg_catalog.char_length(pg_catalog.replace(
                function_definition,
                old_fragment,
                ''
            ))
            <> pg_catalog.char_length(old_fragment)
        OR pg_catalog.strpos(function_definition, new_fragment) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'runtime_serving_owner_rollover_readiness_precondition_drift';
    END IF;

    EXECUTE pg_catalog.replace(
        function_definition,
        old_fragment,
        new_fragment
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
        OR observed_definition_digest <> expected_definition_digest
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(function_oid),
            old_fragment
        ) <> 0
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(function_oid),
            new_fragment
        ) = 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'runtime_serving_owner_rollover_readiness_postcondition_drift';
    END IF;
END;
$serving_owner_rollover_readiness$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
