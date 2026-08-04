SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended(
        'starring-runtime-serving-heartbeat-owner-successors-v1',
        0
    )
);

DO $serving_heartbeat_owner_successors$
DECLARE
    function_identity TEXT :=
        'public.starring_runtime_serving_heartbeat_v2(text,text,text,text,text,text,bigint,bigint,bigint,bigint)';
    function_oid OID;
    function_definition TEXT;
    old_lock_fragment TEXT :=
        E'            MESSAGE = ''runtime_serving_heartbeat_v2_input_invalid'';\n    END IF;\n\n    SELECT attestation.*';
    new_lock_fragment TEXT :=
        E'            MESSAGE = ''runtime_serving_heartbeat_v2_input_invalid'';\n    END IF;\n\n    PERFORM pg_catalog.pg_advisory_xact_lock_shared(\n        pg_catalog.hashtextextended(\n            ''starring-runtime-writer-fence-v1'',\n            0\n        )\n    );\n\n    SELECT attestation.*';
    old_revision_fragment TEXT :=
        E'        OR owner_row.owner_revision::TEXT\n            IS DISTINCT FROM attestation_row.v2_route_admission\n                ->> ''attested_owner_revision''';
    new_revision_fragment TEXT :=
        E'        OR owner_row.owner_revision\n            <= (\n                attestation_row.v2_route_admission\n                    ->> ''attested_owner_revision''\n            )::BIGINT';
    old_projection_fragment TEXT :=
        E'    target_version := lease_record.target_version;\n    target_content_hash := lease_record.target_content_hash;\n    binding_revision := lease_record.binding_revision;\n    binding_fingerprint := lease_record.binding_fingerprint;';
    new_projection_fragment TEXT :=
        E'    target_version := attestation_row.target_version;\n    target_content_hash := attestation_row.target_content_hash;\n    binding_revision := attestation_row.binding_revision;\n    binding_fingerprint := attestation_row.binding_fingerprint;';
    old_definition_digest TEXT :=
        'a11adfbded99484385d25a81bee4cc59e054f98bc1f5792310e56b4eadb8e0fc';
    expected_definition_digest TEXT :=
        '63e717f2c581844342b19353d0119764ae8d661471a554e7fa26b2a329aaab07';
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
                old_lock_fragment,
                ''
            ))
            <> pg_catalog.char_length(old_lock_fragment)
        OR pg_catalog.char_length(function_definition)
            - pg_catalog.char_length(pg_catalog.replace(
                function_definition,
                old_revision_fragment,
                ''
            ))
            <> pg_catalog.char_length(old_revision_fragment)
        OR pg_catalog.char_length(function_definition)
            - pg_catalog.char_length(pg_catalog.replace(
                function_definition,
                old_projection_fragment,
                ''
            ))
            <> pg_catalog.char_length(old_projection_fragment)
        OR pg_catalog.strpos(
            function_definition,
            new_lock_fragment
        ) <> 0
        OR pg_catalog.strpos(
            function_definition,
            new_revision_fragment
        ) <> 0
        OR pg_catalog.strpos(
            function_definition,
            new_projection_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'runtime_serving_heartbeat_owner_successor_precondition_drift';
    END IF;

    function_definition := pg_catalog.replace(
        function_definition,
        old_lock_fragment,
        new_lock_fragment
    );
    function_definition := pg_catalog.replace(
        function_definition,
        old_revision_fragment,
        new_revision_fragment
    );
    function_definition := pg_catalog.replace(
        function_definition,
        old_projection_fragment,
        new_projection_fragment
    );
    EXECUTE function_definition;

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
            old_lock_fragment
        ) <> 0
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(function_oid),
            new_lock_fragment
        ) = 0
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(function_oid),
            old_revision_fragment
        ) <> 0
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(function_oid),
            new_revision_fragment
        ) = 0
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(function_oid),
            old_projection_fragment
        ) <> 0
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(function_oid),
            new_projection_fragment
        ) = 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'runtime_serving_heartbeat_owner_successor_postcondition_drift';
    END IF;
END;
$serving_heartbeat_owner_successors$;

DO $serving_disconnect_projection$
DECLARE
    function_identity TEXT :=
        'public.starring_runtime_serving_disconnect_if_current_v2(text,text,text,text,text,text,bigint,bigint,bigint)';
    function_oid OID;
    function_definition TEXT;
    old_fragment TEXT :=
        E'    target_version := lease_record.target_version;\n    target_content_hash := lease_record.target_content_hash;\n    binding_revision := lease_record.binding_revision;\n    binding_fingerprint := lease_record.binding_fingerprint;';
    new_fragment TEXT :=
        E'    target_version := attestation_row.target_version;\n    target_content_hash := attestation_row.target_content_hash;\n    binding_revision := attestation_row.binding_revision;\n    binding_fingerprint := attestation_row.binding_fingerprint;';
    old_definition_digest TEXT :=
        'd714be0f03a6b8a7b1dd0c276214255fb2c2b7d6841987aba5d63819284d6680';
    expected_definition_digest TEXT :=
        '4620414e2a07bb0d3421c5480cb25339d3c1e53f018b9997cb153aba7b1aa8db';
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
            MESSAGE = 'runtime_serving_disconnect_projection_precondition_drift';
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
            MESSAGE = 'runtime_serving_disconnect_projection_postcondition_drift';
    END IF;
END;
$serving_disconnect_projection$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
