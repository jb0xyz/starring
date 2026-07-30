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
    stale_live_digest TEXT;
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
                    'public.starring_runtime_startup_recovery_execute_stale_live_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO stale_live_digest;

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
        OR stale_live_digest
            <> '11aeeae9eb23564951a87c947439c0a6f87c5dca1b506a1cb9b5e0f4f9c0c936'
        OR manifest_digest
            <> '2d64e05eaf87f593c181fef92a4131539940fd4e58ac5acfbd33a4c39f8d2f03'
        OR readiness_digest
            <> '7bd23bbaa7cef9cfcb88ac6a273dc6ac82af3e55e5ab71fff5a54b98cd90f81e'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v2()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_terminalized_stale_live_recovery_preflight_drift';
    END IF;
END;
$preflight$;

DO $patch_stale_live$
DECLARE
    function_oid OID;
    definition TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
    observed_definition_digest TEXT;
    old_declaration TEXT := $old$    reservation_count BIGINT;
    exact_awaiting_reservation_count BIGINT;
    invalid_suspend_attempt_count BIGINT;$old$;
    new_declaration TEXT := $new$    reservation_count BIGINT;
    unresolved_reservation_count BIGINT;
    exact_terminal_reservation_count BIGINT;
    invalid_reservation_count BIGINT;
    invalid_suspend_attempt_count BIGINT;$new$;
    old_classifier TEXT := $old$    SELECT
        pg_catalog.count(*),
        pg_catalog.count(*) FILTER (
            WHERE deployment.phase = 'awaiting_gateway_ready'
                AND deployment.revision =
                    reservation.deployment_revision
                AND deployment.convergence_attempt_no =
                    reservation.convergence_attempt_no
                AND deployment.snapshot #>> '{phase,phase}' =
                    'awaiting_gateway_ready'
                AND deployment.snapshot ->> 'revision' =
                    reservation.deployment_revision::TEXT
                AND deployment.controller_id IS NOT NULL
                AND deployment.controller_fencing_token IS NOT NULL
                AND deployment.last_controller_id =
                    deployment.controller_id
                AND deployment.last_fencing_token =
                    deployment.controller_fencing_token
        )
    INTO reservation_count, exact_awaiting_reservation_count
    FROM public.runtime_certification_operations_v2 AS reservation
    LEFT JOIN public.runtime_deployments AS deployment
        ON deployment.tenant_id = reservation.tenant_id
        AND deployment.installation_id =
            reservation.installation_id
        AND deployment.deployment_id = reservation.deployment_id;

    IF reservation_count <> exact_awaiting_reservation_count THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_stale_live_execution_state_ambiguous';
    END IF;$old$;
    new_classifier TEXT := $new$    SELECT
        pg_catalog.count(*),
        pg_catalog.count(*) FILTER (
            WHERE terminal.operation_id IS NULL
                AND deployment.phase = 'awaiting_gateway_ready'
                AND deployment.revision =
                    reservation.deployment_revision
                AND deployment.convergence_attempt_no =
                    reservation.convergence_attempt_no
                AND deployment.snapshot #>> '{phase,phase}' =
                    'awaiting_gateway_ready'
                AND deployment.snapshot ->> 'revision' =
                    reservation.deployment_revision::TEXT
                AND deployment.controller_id IS NOT NULL
                AND deployment.controller_fencing_token IS NOT NULL
                AND deployment.last_controller_id =
                    deployment.controller_id
                AND deployment.last_fencing_token =
                    deployment.controller_fencing_token
        ),
        pg_catalog.count(*) FILTER (
            WHERE terminal.operation_id IS NOT NULL
                AND terminal.intent_fingerprint =
                    reservation.intent_fingerprint
                AND terminal.tenant_id = reservation.tenant_id
                AND terminal.installation_id =
                    reservation.installation_id
                AND terminal.deployment_id =
                    reservation.deployment_id
                AND terminal.deployment_revision =
                    reservation.deployment_revision
                AND terminal.convergence_attempt_no =
                    reservation.convergence_attempt_no
                AND (
                    (
                        terminal.terminal_outcome_name =
                            'awaiting_reset'
                        AND terminal.resulting_phase =
                            'reconciling_panels'
                    )
                    OR (
                        terminal.terminal_outcome_name =
                            'certification_committed'
                        AND terminal.resulting_phase = 'live'
                    )
                )
                AND terminal.resulting_deployment_revision =
                    reservation.deployment_revision + 1
                AND terminal.resulting_convergence_attempt_no =
                    reservation.convergence_attempt_no
                AND deployment.snapshot ->> 'revision' =
                    deployment.revision::TEXT
                AND deployment.snapshot #>> '{phase,phase}' =
                    deployment.phase
                AND (
                    (
                        deployment.revision =
                            terminal.resulting_deployment_revision
                        AND deployment.phase =
                            terminal.resulting_phase
                        AND deployment.convergence_attempt_no =
                            terminal.resulting_convergence_attempt_no
                    )
                    OR (
                        deployment.revision >
                            terminal.resulting_deployment_revision
                        AND deployment.convergence_attempt_no >=
                            terminal.resulting_convergence_attempt_no
                    )
                )
        )
    INTO
        reservation_count,
        unresolved_reservation_count,
        exact_terminal_reservation_count
    FROM public.runtime_certification_operations_v2 AS reservation
    LEFT JOIN public.runtime_certification_operation_terminals_v2
        AS terminal
        ON terminal.operation_id = reservation.operation_id
    LEFT JOIN public.runtime_deployments AS deployment
        ON deployment.tenant_id = reservation.tenant_id
        AND deployment.installation_id =
            reservation.installation_id
        AND deployment.deployment_id = reservation.deployment_id;

    invalid_reservation_count := reservation_count
        - unresolved_reservation_count
        - exact_terminal_reservation_count;
    IF invalid_reservation_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_stale_live_execution_state_ambiguous';
    END IF;$new$;
    old_bound TEXT :=
        '    IF reservation_count > 4294967295';
    new_bound TEXT :=
        '    IF unresolved_reservation_count > 4294967295';
BEGIN
    function_oid := pg_catalog.to_regprocedure(
        'public.starring_runtime_startup_recovery_execute_stale_live_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)'
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
                old_declaration,
                ''
            ))
        ) / pg_catalog.char_length(old_declaration) <> 1
        OR (
            pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                old_classifier,
                ''
            ))
        ) / pg_catalog.char_length(old_classifier) <> 1
        OR (
            pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                old_bound,
                ''
            ))
        ) / pg_catalog.char_length(old_bound) <> 1
        OR pg_catalog.strpos(definition, new_declaration) <> 0
        OR pg_catalog.strpos(definition, new_classifier) <> 0
        OR pg_catalog.strpos(definition, new_bound) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_terminalized_stale_live_recovery_definition_drift';
    END IF;

    definition := pg_catalog.replace(
        definition,
        old_declaration,
        new_declaration
    );
    definition := pg_catalog.replace(
        definition,
        old_classifier,
        new_classifier
    );
    definition := pg_catalog.replace(
        definition,
        old_bound,
        new_bound
    );
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
            <> '31ec76b9dbbde23f3caa66e2435ddac8a64755729e14385a3baf96dd8060c8fd'
        OR pg_catalog.strpos(definition, old_declaration) <> 0
        OR pg_catalog.strpos(definition, old_classifier) <> 0
        OR pg_catalog.strpos(definition, old_bound) <> 0
        OR (
            pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                new_declaration,
                ''
            ))
        ) / pg_catalog.char_length(new_declaration) <> 1
        OR (
            pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                new_classifier,
                ''
            ))
        ) / pg_catalog.char_length(new_classifier) <> 1
        OR (
            pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                new_bound,
                ''
            ))
        ) / pg_catalog.char_length(new_bound) <> 1
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_terminalized_stale_live_recovery_postcondition_drift';
    END IF;
END;
$patch_stale_live$;

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
                '0235fe476513635ca25c6ec752c26386e1d7d4d317212e2c3ecbeb1f6306f766',
                ''
            ))
            <> 64
        OR pg_catalog.strpos(
            definition,
            '053b2cc1576b5a6c6fad441fb5222b60176ad2e4c6581befab383a2d9fb886ee'
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_terminalized_stale_live_recovery_manifest_drift';
    END IF;

    EXECUTE pg_catalog.replace(
        definition,
        '0235fe476513635ca25c6ec752c26386e1d7d4d317212e2c3ecbeb1f6306f766',
        '053b2cc1576b5a6c6fad441fb5222b60176ad2e4c6581befab383a2d9fb886ee'
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
            <> 'f6bd51c0de1eff13175d07f8861f71e4f08b2e7395cfc3eaf516cf4b644a4e63'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_terminalized_stale_live_recovery_manifest_postcondition_drift';
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
                '2d64e05eaf87f593c181fef92a4131539940fd4e58ac5acfbd33a4c39f8d2f03',
                ''
            ))
            <> 64
        OR pg_catalog.strpos(
            definition,
            'f6bd51c0de1eff13175d07f8861f71e4f08b2e7395cfc3eaf516cf4b644a4e63'
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_terminalized_stale_live_recovery_readiness_drift';
    END IF;

    EXECUTE pg_catalog.replace(
        definition,
        '2d64e05eaf87f593c181fef92a4131539940fd4e58ac5acfbd33a4c39f8d2f03',
        'f6bd51c0de1eff13175d07f8861f71e4f08b2e7395cfc3eaf516cf4b644a4e63'
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
            <> '779d97c088a29027589ebdffa9753eb1333a1d9b511cd714211cde6ae8146c4e'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_terminalized_stale_live_recovery_readiness_postcondition_drift';
    END IF;
END;
$refresh_readiness$;

DO $postflight$
DECLARE
    definition TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(
        pg_catalog.to_regprocedure(
            'public.starring_runtime_startup_recovery_execute_stale_live_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)'
        )
    )
    INTO definition;

    IF NOT public.starring_runtime_execution_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v2()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
        OR pg_catalog.strpos(
            definition,
            'LEFT JOIN public.runtime_certification_operation_terminals_v2'
        ) = 0
        OR pg_catalog.strpos(
            definition,
            'terminal.operation_id = reservation.operation_id'
        ) = 0
        OR pg_catalog.strpos(
            definition,
            'invalid_reservation_count := reservation_count'
        ) = 0
        OR pg_catalog.strpos(
            definition,
            'IF unresolved_reservation_count > 4294967295'
        ) = 0
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                definition,
                'UTF8'
            )),
            'hex'
        ) <> '31ec76b9dbbde23f3caa66e2435ddac8a64755729e14385a3baf96dd8060c8fd'
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
        ) <> 'f6bd51c0de1eff13175d07f8861f71e4f08b2e7395cfc3eaf516cf4b644a4e63'
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
        ) <> '779d97c088a29027589ebdffa9753eb1333a1d9b511cd714211cde6ae8146c4e'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_terminalized_stale_live_recovery_postflight_drift';
    END IF;
END;
$postflight$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
