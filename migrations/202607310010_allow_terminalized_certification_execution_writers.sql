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
        OR renew_digest
            <> '00fb1426fd8711b496b35e0658db13a534560ba13191d710c4274cd54461275c'
        OR mutate_digest
            <> '9e201e149dac432794bfcfc23b424f59741869fcf9d39765693a21b2451646ce'
        OR manifest_digest
            <> '34cde5bd3a13f2132ba29f5324e67c95cf7511ea04aaa1033026289d70267027'
        OR readiness_digest
            <> 'd8e46c1204b36b3c909b7e6e88ee768d2ec7e60d05dd4eb99be7d8f064a24714'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v2()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_terminalized_certification_execution_writer_preflight_drift';
    END IF;
END;
$preflight$;

DO $patch_execution_writers$
DECLARE
    patch_row RECORD;
    function_oid OID;
    definition TEXT;
    old_guard TEXT;
    new_guard TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
    observed_definition_digest TEXT;
BEGIN
    FOR patch_row IN
        SELECT *
        FROM (
            VALUES
                (
                    'public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)',
                    'runtime_execution_renew_ownership_lost',
                    '7c7b1d1884c79c9040eb6937a997ce3d2540eb390fb3f5c757c8d6dfeda16b0e'
                ),
                (
                    'public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)',
                    'runtime_execution_mutation_ownership_lost',
                    'd6972ef0bb0b088480cdfed79da274f183dc3dd61908487d4b8e0339998b2e27'
                )
        ) AS patch(identity, ownership_message, expected_digest)
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
            )
        INTO definition, metadata_before
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid = function_oid;

        old_guard := pg_catalog.replace(
            $old$    IF EXISTS (
        SELECT 1
        FROM public.runtime_certification_operations_v2 AS reservation
        WHERE reservation.tenant_id = deployment_row.tenant_id
            AND reservation.installation_id = deployment_row.installation_id
            AND reservation.deployment_id = deployment_row.deployment_id
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = '__MESSAGE__';
    END IF;$old$,
            '__MESSAGE__',
            patch_row.ownership_message
        );
        new_guard := pg_catalog.replace(
            $new$    IF EXISTS (
        SELECT 1
        FROM public.runtime_certification_operations_v2 AS reservation
        WHERE reservation.tenant_id = deployment_row.tenant_id
            AND reservation.installation_id = deployment_row.installation_id
            AND reservation.deployment_id = deployment_row.deployment_id
            AND NOT EXISTS (
                SELECT 1
                FROM public.runtime_certification_operation_terminals_v2 AS terminal
                WHERE terminal.operation_id = reservation.operation_id
                    AND terminal.intent_fingerprint = reservation.intent_fingerprint
                    AND terminal.tenant_id = reservation.tenant_id
                    AND terminal.installation_id = reservation.installation_id
                    AND terminal.deployment_id = reservation.deployment_id
                    AND terminal.deployment_revision = reservation.deployment_revision
                    AND terminal.convergence_attempt_no = reservation.convergence_attempt_no
                    AND terminal.terminal_outcome_name = 'awaiting_reset'
                    AND terminal.resulting_phase = 'reconciling_panels'
                    AND (
                        (
                            terminal.resulting_deployment_revision
                                = deployment_row.revision
                            AND terminal.resulting_convergence_attempt_no
                                = deployment_row.convergence_attempt_no
                            AND deployment_row.phase = 'reconciling_panels'
                        )
                        OR (
                            terminal.resulting_deployment_revision
                                < deployment_row.revision
                            AND terminal.resulting_convergence_attempt_no
                                <= deployment_row.convergence_attempt_no
                        )
                    )
            )
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = '__MESSAGE__';
    END IF;$new$,
            '__MESSAGE__',
            patch_row.ownership_message
        );

        IF definition IS NULL
            OR (
                pg_catalog.char_length(definition)
                - pg_catalog.char_length(pg_catalog.replace(
                    definition,
                    old_guard,
                    ''
                ))
            ) / pg_catalog.char_length(old_guard) <> 1
            OR pg_catalog.strpos(definition, new_guard) <> 0
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = pg_catalog.concat(
                    'runtime_terminalized_certification_execution_writer_',
                    patch_row.ownership_message,
                    '_definition_drift'
                );
        END IF;

        EXECUTE pg_catalog.replace(definition, old_guard, new_guard);

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
            OR pg_catalog.strpos(definition, old_guard) <> 0
            OR (
                pg_catalog.char_length(definition)
                - pg_catalog.char_length(pg_catalog.replace(
                    definition,
                    new_guard,
                    ''
                ))
            ) / pg_catalog.char_length(new_guard) <> 1
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = pg_catalog.concat(
                    'runtime_terminalized_certification_execution_writer_',
                    patch_row.ownership_message,
                    '_postcondition_drift'
                );
        END IF;
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
                '7597ea370c26ac6b5534e1568637e79faad000184e6a862f59acba56276a6a40',
                ''
            ))
            <> 64
        OR pg_catalog.strpos(
            definition,
            '0235fe476513635ca25c6ec752c26386e1d7d4d317212e2c3ecbeb1f6306f766'
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_terminalized_certification_execution_writer_manifest_drift';
    END IF;

    EXECUTE pg_catalog.replace(
        definition,
        '7597ea370c26ac6b5534e1568637e79faad000184e6a862f59acba56276a6a40',
        '0235fe476513635ca25c6ec752c26386e1d7d4d317212e2c3ecbeb1f6306f766'
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
            <> '2d64e05eaf87f593c181fef92a4131539940fd4e58ac5acfbd33a4c39f8d2f03'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_terminalized_certification_execution_writer_manifest_postcondition_drift';
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
                '34cde5bd3a13f2132ba29f5324e67c95cf7511ea04aaa1033026289d70267027',
                ''
            ))
            <> 64
        OR pg_catalog.strpos(
            definition,
            '2d64e05eaf87f593c181fef92a4131539940fd4e58ac5acfbd33a4c39f8d2f03'
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_terminalized_certification_execution_writer_readiness_drift';
    END IF;

    EXECUTE pg_catalog.replace(
        definition,
        '34cde5bd3a13f2132ba29f5324e67c95cf7511ea04aaa1033026289d70267027',
        '2d64e05eaf87f593c181fef92a4131539940fd4e58ac5acfbd33a4c39f8d2f03'
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
            <> '7bd23bbaa7cef9cfcb88ac6a273dc6ac82af3e55e5ab71fff5a54b98cd90f81e'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_terminalized_certification_execution_writer_readiness_postcondition_drift';
    END IF;
END;
$refresh_readiness$;

DO $postflight$
DECLARE
    patch_row RECORD;
    definition TEXT;
    guard_count BIGINT;
    terminal_count BIGINT;
    writer_position INTEGER;
    slot_position INTEGER;
    physical_position INTEGER;
    deployment_lock_position INTEGER;
    reservation_position INTEGER;
    continuation_position INTEGER;
BEGIN
    FOR patch_row IN
        SELECT *
        FROM (
            VALUES
                (
                    'public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)',
                    '7c7b1d1884c79c9040eb6937a997ce3d2540eb390fb3f5c757c8d6dfeda16b0e',
                    'IF deployment_row.revision = expected_deployment_revision + 1 THEN'
                ),
                (
                    'public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)',
                    'd6972ef0bb0b088480cdfed79da274f183dc3dd61908487d4b8e0339998b2e27',
                    'IF deployment_row.revision = expected_deployment_revision + 1'
                )
        ) AS patch(identity, expected_digest, continuation)
    LOOP
        SELECT pg_catalog.pg_get_functiondef(
            pg_catalog.to_regprocedure(patch_row.identity)
        )
        INTO definition;
        guard_count := (
            pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                'FROM public.runtime_certification_operations_v2 AS reservation',
                ''
            ))
        ) / pg_catalog.char_length(
            'FROM public.runtime_certification_operations_v2 AS reservation'
        );
        terminal_count := (
            pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                'FROM public.runtime_certification_operation_terminals_v2 AS terminal',
                ''
            ))
        ) / pg_catalog.char_length(
            'FROM public.runtime_certification_operation_terminals_v2 AS terminal'
        );
        writer_position := pg_catalog.strpos(
            definition,
            'starring_runtime_writer_fence_observe_v1'
        );
        slot_position := pg_catalog.strpos(
            definition,
            'starring-runtime-serving-slot-v1:'
        );
        physical_position := pg_catalog.strpos(
            definition,
            'starring_runtime_slot_writer_fence_lock_v2'
        );
        deployment_lock_position := pg_catalog.strpos(
            definition,
            'FOR UPDATE;'
        );
        reservation_position := pg_catalog.strpos(
            definition,
            'WHERE reservation.tenant_id = deployment_row.tenant_id'
        );
        continuation_position := pg_catalog.strpos(
            definition,
            patch_row.continuation
        );

        IF pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(definition, 'UTF8')),
            'hex'
        ) IS DISTINCT FROM patch_row.expected_digest
            OR guard_count <> 1
            OR terminal_count <> 1
            OR pg_catalog.strpos(
                definition,
                'terminal.operation_id = reservation.operation_id'
            ) = 0
            OR pg_catalog.strpos(
                definition,
                'terminal.intent_fingerprint = reservation.intent_fingerprint'
            ) = 0
            OR pg_catalog.strpos(
                definition,
                'terminal.deployment_revision = reservation.deployment_revision'
            ) = 0
            OR pg_catalog.strpos(
                definition,
                'terminal.convergence_attempt_no = reservation.convergence_attempt_no'
            ) = 0
            OR pg_catalog.strpos(
                definition,
                'terminal.terminal_outcome_name = ''awaiting_reset'''
            ) = 0
            OR pg_catalog.strpos(
                definition,
                'terminal.resulting_phase = ''reconciling_panels'''
            ) = 0
            OR writer_position = 0
            OR slot_position = 0
            OR physical_position = 0
            OR deployment_lock_position = 0
            OR reservation_position = 0
            OR continuation_position = 0
            OR NOT (
                writer_position < slot_position
                AND slot_position < physical_position
                AND physical_position < deployment_lock_position
                AND deployment_lock_position < reservation_position
                AND reservation_position < continuation_position
            )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_terminalized_certification_execution_writer_contract_drift';
        END IF;
    END LOOP;

    IF NOT public.starring_runtime_execution_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v2()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
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
        ) <> '2d64e05eaf87f593c181fef92a4131539940fd4e58ac5acfbd33a4c39f8d2f03'
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
        ) <> '7bd23bbaa7cef9cfcb88ac6a273dc6ac82af3e55e5ab71fff5a54b98cd90f81e'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_terminalized_certification_execution_writer_postflight_drift';
    END IF;
END;
$postflight$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
