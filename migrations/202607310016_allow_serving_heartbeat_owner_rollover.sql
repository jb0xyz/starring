SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended(
        'starring-runtime-serving-heartbeat-owner-rollover-v1',
        0
    )
);

DO $serving_heartbeat_owner_rollover$
DECLARE
    function_identity TEXT :=
        'public.starring_runtime_serving_heartbeat_v2(text,text,text,text,text,text,bigint,bigint,bigint,bigint)';
    function_oid OID;
    function_definition TEXT;
    old_fragment TEXT :=
        E'        OR acknowledgement_row.observed_owner_revision\n            IS DISTINCT FROM owner_row.owner_revision';
    new_fragment TEXT :=
        E'        OR acknowledgement_row.requested_owner_observed_at\n            > pg_catalog.clock_timestamp()\n        OR acknowledgement_row.requested_owner_expires_at\n            <= pg_catalog.clock_timestamp()\n        OR NOT (\n            (\n                acknowledgement_row.observed_owner_revision\n                    = owner_row.owner_revision\n                AND acknowledgement_row.requested_owner_expires_at\n                    = owner_row.expires_at\n            )\n            OR (\n                owner_row.owner_revision\n                    > acknowledgement_row.observed_owner_revision\n                AND owner_row.owner_revision\n                    - acknowledgement_row.observed_owner_revision = 1\n                AND owner_row.expires_at\n                    > acknowledgement_row.requested_owner_expires_at\n            )\n        )';
    old_definition_digest TEXT :=
        '63e717f2c581844342b19353d0119764ae8d661471a554e7fa26b2a329aaab07';
    expected_definition_digest TEXT :=
        '07859d61ceab00eeeaeba860337927b36718d60b2eff468362c3fad57f703327';
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
            MESSAGE = 'runtime_serving_heartbeat_owner_rollover_precondition_drift';
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
            MESSAGE = 'runtime_serving_heartbeat_owner_rollover_postcondition_drift';
    END IF;
END;
$serving_heartbeat_owner_rollover$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
