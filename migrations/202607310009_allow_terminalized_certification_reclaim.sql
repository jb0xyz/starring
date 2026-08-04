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
            <> 'cc5475b256b6b48f3c4f6d3933461cdcdeff1dbdb974d32d7d735348d8f14eb4'
        OR manifest_digest
            <> 'ee35572e966037477a9070fef87781e901f0ef49e3cb471acebba9c165657676'
        OR readiness_digest
            <> '437eef0962f31be61e9fcb2f6705b2cda14f4d52105ae024ca4bc29b967e001c'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v2()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_terminalized_certification_reclaim_preflight_drift';
    END IF;
END;
$preflight$;

DO $patch_claim$
DECLARE
    function_oid OID;
    definition TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
    observed_definition_digest TEXT;
    old_selector TEXT := $old$        AND NOT EXISTS (
            SELECT 1
            FROM public.runtime_certification_operations_v2 AS reservation
            WHERE reservation.tenant_id = deployment.tenant_id
                AND reservation.installation_id = deployment.installation_id
                AND reservation.deployment_id = deployment.deployment_id
        )$old$;
    new_selector TEXT := $new$        AND NOT EXISTS (
            SELECT 1
            FROM public.runtime_certification_operations_v2 AS reservation
            WHERE reservation.tenant_id = deployment.tenant_id
                AND reservation.installation_id = deployment.installation_id
                AND reservation.deployment_id = deployment.deployment_id
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
                                    = deployment.revision
                                AND terminal.resulting_convergence_attempt_no
                                    = deployment.convergence_attempt_no
                                AND deployment.phase = 'reconciling_panels'
                            )
                            OR (
                                terminal.resulting_deployment_revision
                                    < deployment.revision
                                AND terminal.resulting_convergence_attempt_no
                                    <= deployment.convergence_attempt_no
                            )
                        )
                )
        )$new$;
    old_locked TEXT := $old$            IF EXISTS (
                SELECT 1
                FROM public.runtime_certification_operations_v2 AS reservation
                WHERE reservation.tenant_id = deployment_row.tenant_id
                    AND reservation.installation_id = deployment_row.installation_id
                    AND reservation.deployment_id = deployment_row.deployment_id
            ) THEN
                RAISE no_data_found;
            END IF;$old$;
    new_locked TEXT := $new$            IF EXISTS (
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
                RAISE no_data_found;
            END IF;$new$;
BEGIN
    function_oid := pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_claim_next_v1(text,bigint)'
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
        OR (
            pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                old_selector,
                ''
            ))
        ) / pg_catalog.char_length(old_selector) <> 2
        OR (
            pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                old_locked,
                ''
            ))
        ) / pg_catalog.char_length(old_locked) <> 2
        OR pg_catalog.strpos(definition, new_selector) <> 0
        OR pg_catalog.strpos(definition, new_locked) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_terminalized_certification_reclaim_definition_drift';
    END IF;

    definition := pg_catalog.replace(
        definition,
        old_selector,
        new_selector
    );
    definition := pg_catalog.replace(
        definition,
        old_locked,
        new_locked
    );
    EXECUTE definition;

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
            <> '50ed71606e880e95720b2628abd765748700ef78bb429bf2be07f739b2aefd1e'
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(function_oid),
            old_selector
        ) <> 0
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(function_oid),
            old_locked
        ) <> 0
        OR (
            pg_catalog.char_length(pg_catalog.pg_get_functiondef(function_oid))
            - pg_catalog.char_length(pg_catalog.replace(
                pg_catalog.pg_get_functiondef(function_oid),
                new_selector,
                ''
            ))
        ) / pg_catalog.char_length(new_selector) <> 2
        OR (
            pg_catalog.char_length(pg_catalog.pg_get_functiondef(function_oid))
            - pg_catalog.char_length(pg_catalog.replace(
                pg_catalog.pg_get_functiondef(function_oid),
                new_locked,
                ''
            ))
        ) / pg_catalog.char_length(new_locked) <> 2
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_terminalized_certification_reclaim_postcondition_drift';
    END IF;
END;
$patch_claim$;

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
                'dd7a64d16d27a32dde6f80416e4efc444c69aa59e055ff26f8008a2cdc845a62',
                ''
            ))
            <> 64
        OR pg_catalog.strpos(
            definition,
            '7597ea370c26ac6b5534e1568637e79faad000184e6a862f59acba56276a6a40'
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_terminalized_certification_manifest_drift';
    END IF;

    EXECUTE pg_catalog.replace(
        definition,
        'dd7a64d16d27a32dde6f80416e4efc444c69aa59e055ff26f8008a2cdc845a62',
        '7597ea370c26ac6b5534e1568637e79faad000184e6a862f59acba56276a6a40'
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
            <> '34cde5bd3a13f2132ba29f5324e67c95cf7511ea04aaa1033026289d70267027'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_terminalized_certification_manifest_postcondition_drift';
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
                'ee35572e966037477a9070fef87781e901f0ef49e3cb471acebba9c165657676',
                ''
            ))
            <> 64
        OR pg_catalog.strpos(
            definition,
            '34cde5bd3a13f2132ba29f5324e67c95cf7511ea04aaa1033026289d70267027'
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_terminalized_certification_readiness_drift';
    END IF;

    EXECUTE pg_catalog.replace(
        definition,
        'ee35572e966037477a9070fef87781e901f0ef49e3cb471acebba9c165657676',
        '34cde5bd3a13f2132ba29f5324e67c95cf7511ea04aaa1033026289d70267027'
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
            <> 'd8e46c1204b36b3c909b7e6e88ee768d2ec7e60d05dd4eb99be7d8f064a24714'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_terminalized_certification_readiness_postcondition_drift';
    END IF;
END;
$refresh_readiness$;

DO $postflight$
BEGIN
    IF NOT public.starring_runtime_execution_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v2()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_execution_claim_next_v1(text,bigint)'
                    )
                ),
                'UTF8'
            )),
            'hex'
        ) <> '50ed71606e880e95720b2628abd765748700ef78bb429bf2be07f739b2aefd1e'
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
        ) <> '34cde5bd3a13f2132ba29f5324e67c95cf7511ea04aaa1033026289d70267027'
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
        ) <> 'd8e46c1204b36b3c909b7e6e88ee768d2ec7e60d05dd4eb99be7d8f064a24714'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_terminalized_certification_reclaim_postflight_drift';
    END IF;
END;
$postflight$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
