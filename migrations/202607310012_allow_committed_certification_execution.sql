SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
);

LOCK TABLE
    public.runtime_deployments,
    public.runtime_certification_operations_v2,
    public.runtime_certification_operation_terminals_v2
IN ACCESS SHARE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    claim_digest TEXT;
    renew_digest TEXT;
    mutate_digest TEXT;
    manifest_digest TEXT;
    readiness_digest TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_execution_claim_next_v1(text,bigint)'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO claim_digest;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO renew_digest;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO mutate_digest;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_execution_schema_manifest_v1()'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO manifest_digest;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_execution_database_readiness_v1()'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO readiness_digest;

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR claim_digest
            <> '50ed71606e880e95720b2628abd765748700ef78bb429bf2be07f739b2aefd1e'
        OR renew_digest
            <> '7c7b1d1884c79c9040eb6937a997ce3d2540eb390fb3f5c757c8d6dfeda16b0e'
        OR mutate_digest
            <> 'd6972ef0bb0b088480cdfed79da274f183dc3dd61908487d4b8e0339998b2e27'
        OR manifest_digest
            <> 'f6bd51c0de1eff13175d07f8861f71e4f08b2e7395cfc3eaf516cf4b644a4e63'
        OR readiness_digest
            <> '779d97c088a29027589ebdffa9753eb1333a1d9b511cd714211cde6ae8146c4e'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v2()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_committed_certification_execution_preflight_drift';
    END IF;
END;
$preflight$;

DO $patch_execution_writers$
DECLARE
    patch_row RECORD;
    indentation INTEGER;
    indentation_index INTEGER;
    deployment_alias TEXT;
    function_oid OID;
    definition TEXT;
    old_predicate TEXT;
    new_predicate TEXT;
    observed_count BIGINT;
    metadata_before JSONB;
    metadata_after JSONB;
    observed_definition_digest TEXT;
BEGIN
    FOR patch_row IN
        SELECT *
        FROM (
            VALUES
                (
                    'public.starring_runtime_execution_claim_next_v1(text,bigint)',
                    ARRAY[24, 28]::INTEGER[],
                    ARRAY['deployment', 'deployment_row']::TEXT[],
                    2::BIGINT,
                    '50ed71606e880e95720b2628abd765748700ef78bb429bf2be07f739b2aefd1e',
                    '2ffaba44876ebfac5b32e0fdd34d147d26be1d83e312534070ca339df244d28e'
                ),
                (
                    'public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)',
                    ARRAY[20]::INTEGER[],
                    ARRAY['deployment_row']::TEXT[],
                    1::BIGINT,
                    '7c7b1d1884c79c9040eb6937a997ce3d2540eb390fb3f5c757c8d6dfeda16b0e',
                    '4478b214a538ef30df57d34cecbc0afba8052bec72b9c989b4a80b31975e44c2'
                ),
                (
                    'public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)',
                    ARRAY[20]::INTEGER[],
                    ARRAY['deployment_row']::TEXT[],
                    1::BIGINT,
                    'd6972ef0bb0b088480cdfed79da274f183dc3dd61908487d4b8e0339998b2e27',
                    '7d436880cd9ba7b95060ce97f6f36c2789c93a537eff4a7197ac5d71a9294c01'
                )
        ) AS patch(
            identity,
            indentations,
            deployment_aliases,
            expected_occurrences,
            previous_digest,
            expected_digest
        )
    LOOP
        function_oid := pg_catalog.to_regprocedure(patch_row.identity);
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
            ),
            pg_catalog.encode(
                pg_catalog.sha256(pg_catalog.convert_to(
                    pg_catalog.pg_get_functiondef(function_row.oid),
                    'UTF8'
                )),
                'hex'
            )
        INTO definition, metadata_before, observed_definition_digest
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid = function_oid;

        IF definition IS NULL
            OR observed_definition_digest
                IS DISTINCT FROM patch_row.previous_digest
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_committed_certification_execution_definition_drift';
        END IF;

        FOR indentation_index IN
            pg_catalog.array_lower(patch_row.indentations, 1)
            .. pg_catalog.array_upper(patch_row.indentations, 1)
        LOOP
            indentation := patch_row.indentations[indentation_index];
            deployment_alias :=
                patch_row.deployment_aliases[indentation_index];
            old_predicate := pg_catalog.repeat(' ', indentation)
                || 'AND terminal.terminal_outcome_name = ''awaiting_reset'''
                || E'\n'
                || pg_catalog.repeat(' ', indentation)
                || 'AND terminal.resulting_phase = ''reconciling_panels''';
            new_predicate := pg_catalog.repeat(' ', indentation)
                || 'AND ('
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 4)
                || '('
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 8)
                || 'terminal.terminal_outcome_name ='
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 12)
                || '''awaiting_reset'''
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 8)
                || 'AND terminal.resulting_phase ='
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 12)
                || '''reconciling_panels'''
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 4)
                || ')'
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 4)
                || 'OR ('
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 8)
                || 'terminal.terminal_outcome_name ='
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 12)
                || '''certification_committed'''
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 8)
                || 'AND terminal.resulting_phase = ''live'''
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 8)
                || 'AND terminal.resulting_deployment_revision <'
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 12)
                || deployment_alias
                || '.revision'
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 8)
                || 'AND terminal.resulting_convergence_attempt_no <='
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 12)
                || deployment_alias
                || '.convergence_attempt_no'
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 4)
                || ')'
                || E'\n'
                || pg_catalog.repeat(' ', indentation)
                || ')';
            observed_count := (
                pg_catalog.char_length(definition)
                - pg_catalog.char_length(pg_catalog.replace(
                    definition,
                    old_predicate,
                    ''
                ))
            ) / pg_catalog.char_length(old_predicate);
            IF observed_count <> patch_row.expected_occurrences
                OR pg_catalog.strpos(definition, new_predicate) <> 0
            THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RE001',
                    MESSAGE = 'runtime_committed_certification_execution_predicate_drift';
            END IF;
            definition := pg_catalog.replace(
                definition,
                old_predicate,
                new_predicate
            );
        END LOOP;

        EXECUTE definition;

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
            ),
            pg_catalog.encode(
                pg_catalog.sha256(pg_catalog.convert_to(
                    pg_catalog.pg_get_functiondef(function_row.oid),
                    'UTF8'
                )),
                'hex'
            )
        INTO definition, metadata_after, observed_definition_digest
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid = function_oid;

        IF metadata_after IS DISTINCT FROM metadata_before
            OR observed_definition_digest
                IS DISTINCT FROM patch_row.expected_digest
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_committed_certification_execution_postcondition_drift';
        END IF;

        FOR indentation_index IN
            pg_catalog.array_lower(patch_row.indentations, 1)
            .. pg_catalog.array_upper(patch_row.indentations, 1)
        LOOP
            indentation := patch_row.indentations[indentation_index];
            deployment_alias :=
                patch_row.deployment_aliases[indentation_index];
            old_predicate := pg_catalog.repeat(' ', indentation)
                || 'AND terminal.terminal_outcome_name = ''awaiting_reset'''
                || E'\n'
                || pg_catalog.repeat(' ', indentation)
                || 'AND terminal.resulting_phase = ''reconciling_panels''';
            new_predicate := pg_catalog.repeat(' ', indentation)
                || 'AND ('
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 4)
                || '('
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 8)
                || 'terminal.terminal_outcome_name ='
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 12)
                || '''awaiting_reset'''
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 8)
                || 'AND terminal.resulting_phase ='
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 12)
                || '''reconciling_panels'''
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 4)
                || ')'
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 4)
                || 'OR ('
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 8)
                || 'terminal.terminal_outcome_name ='
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 12)
                || '''certification_committed'''
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 8)
                || 'AND terminal.resulting_phase = ''live'''
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 8)
                || 'AND terminal.resulting_deployment_revision <'
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 12)
                || deployment_alias
                || '.revision'
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 8)
                || 'AND terminal.resulting_convergence_attempt_no <='
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 12)
                || deployment_alias
                || '.convergence_attempt_no'
                || E'\n'
                || pg_catalog.repeat(' ', indentation + 4)
                || ')'
                || E'\n'
                || pg_catalog.repeat(' ', indentation)
                || ')';
            observed_count := (
                pg_catalog.char_length(definition)
                - pg_catalog.char_length(pg_catalog.replace(
                    definition,
                    new_predicate,
                    ''
                ))
            ) / pg_catalog.char_length(new_predicate);
            IF pg_catalog.strpos(definition, old_predicate) <> 0
                OR observed_count <> patch_row.expected_occurrences
            THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RE001',
                    MESSAGE = 'runtime_committed_certification_execution_postcondition_drift';
            END IF;
        END LOOP;
    END LOOP;
END;
$patch_execution_writers$;

DO $refresh_manifest$
DECLARE
    function_oid OID;
    definition TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
    observed_definition_digest TEXT;
BEGIN
    function_oid := pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_schema_manifest_v1()'
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
                '053b2cc1576b5a6c6fad441fb5222b60176ad2e4c6581befab383a2d9fb886ee',
                ''
            ))
            <> 64
        OR pg_catalog.strpos(
            definition,
            '1d85a38b5d30b20a4b15c6adc70af3e08ea66901465ba83b2d2bf8d200ccbfca'
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_committed_certification_execution_manifest_drift';
    END IF;

    EXECUTE pg_catalog.replace(
        definition,
        '053b2cc1576b5a6c6fad441fb5222b60176ad2e4c6581befab383a2d9fb886ee',
        '1d85a38b5d30b20a4b15c6adc70af3e08ea66901465ba83b2d2bf8d200ccbfca'
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
            <> '2ee6db433ac8976c754c1566b39eb17950d8c9e1a9e5e56d6d96e45a39342dc7'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_committed_certification_execution_manifest_postcondition_drift';
    END IF;
END;
$refresh_manifest$;

DO $refresh_readiness$
DECLARE
    function_oid OID;
    definition TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
    observed_definition_digest TEXT;
BEGIN
    function_oid := pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_database_readiness_v1()'
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
                'f6bd51c0de1eff13175d07f8861f71e4f08b2e7395cfc3eaf516cf4b644a4e63',
                ''
            ))
            <> 64
        OR pg_catalog.strpos(
            definition,
            '2ee6db433ac8976c754c1566b39eb17950d8c9e1a9e5e56d6d96e45a39342dc7'
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_committed_certification_execution_readiness_drift';
    END IF;

    EXECUTE pg_catalog.replace(
        definition,
        'f6bd51c0de1eff13175d07f8861f71e4f08b2e7395cfc3eaf516cf4b644a4e63',
        '2ee6db433ac8976c754c1566b39eb17950d8c9e1a9e5e56d6d96e45a39342dc7'
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
            <> '0e69552f26e09949d44b87c7ae7680432ff2c36a0027230efcf541cc4324cd9f'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_committed_certification_execution_readiness_postcondition_drift';
    END IF;
END;
$refresh_readiness$;

DO $postflight$
DECLARE
    claim_definition TEXT;
    renew_definition TEXT;
    mutate_definition TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(
        pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_claim_next_v1(text,bigint)'
        )
    )
    INTO claim_definition;
    SELECT pg_catalog.pg_get_functiondef(
        pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)'
        )
    )
    INTO renew_definition;
    SELECT pg_catalog.pg_get_functiondef(
        pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)'
        )
    )
    INTO mutate_definition;

    IF NOT public.starring_runtime_execution_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v2()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
        OR pg_catalog.strpos(
            claim_definition,
            '''certification_committed'''
        ) = 0
        OR pg_catalog.strpos(
            claim_definition,
            'terminal.resulting_phase = ''live'''
        ) = 0
        OR pg_catalog.strpos(
            renew_definition,
            '''certification_committed'''
        ) = 0
        OR pg_catalog.strpos(
            mutate_definition,
            '''certification_committed'''
        ) = 0
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                claim_definition,
                'UTF8'
            )),
            'hex'
        ) <> '2ffaba44876ebfac5b32e0fdd34d147d26be1d83e312534070ca339df244d28e'
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                renew_definition,
                'UTF8'
            )),
            'hex'
        ) <> '4478b214a538ef30df57d34cecbc0afba8052bec72b9c989b4a80b31975e44c2'
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                mutate_definition,
                'UTF8'
            )),
            'hex'
        ) <> '7d436880cd9ba7b95060ce97f6f36c2789c93a537eff4a7197ac5d71a9294c01'
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_execution_schema_manifest_v1()'
                    )
                ),
                'UTF8'
            )),
            'hex'
        ) <> '2ee6db433ac8976c754c1566b39eb17950d8c9e1a9e5e56d6d96e45a39342dc7'
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_execution_database_readiness_v1()'
                    )
                ),
                'UTF8'
            )),
            'hex'
        ) <> '0e69552f26e09949d44b87c7ae7680432ff2c36a0027230efcf541cc4324cd9f'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_committed_certification_execution_postflight_drift';
    END IF;
END;
$postflight$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
