SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
);

LOCK TABLE
    public.runtime_deployments,
    public.runtime_drain_intents_v2,
    public.runtime_slot_writer_fences_v2,
    public._sqlx_migrations
IN ACCESS SHARE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    applied_count BIGINT;
    applied_head BIGINT;
    failed_count BIGINT;
    migration_checksum TEXT;
    helper_digest TEXT;
    execution_digest TEXT;
    manifest_digest TEXT;
    readiness_digest TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    SELECT
        pg_catalog.count(*),
        pg_catalog.max(migration.version),
        pg_catalog.count(*) FILTER (WHERE NOT migration.success)
    INTO applied_count, applied_head, failed_count
    FROM public._sqlx_migrations AS migration;

    SELECT pg_catalog.encode(migration.checksum, 'hex')
    INTO migration_checksum
    FROM public._sqlx_migrations AS migration
    WHERE migration.version = 202608020002
        AND migration.success;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'starring_runtime_private_v2.starring_runtime_pending_drain_candidate_v2()'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO helper_digest;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_startup_recovery_execute_pending_drain_v2(text,bigint,bigint,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean,text)'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO execution_digest;

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
        OR applied_count <> 119
        OR applied_head <> 202608020002
        OR failed_count <> 0
        OR migration_checksum
            <> '61ac941862c11f0aaa3cce54a2842ffadf4e5897c39f6796d2c6874e987a9f1e9d4ba6dd3dbc332f569c20d25831d769'
        OR helper_digest
            <> '91d4d64ae0f1b3053ec91f1c1b07164fce08311e26b58718eca672f3fadee909'
        OR execution_digest
            <> '5414574cde39e1c59410e1cac6ccb975a87d16f4807f3ae33b8f28b8157a8e9b'
        OR manifest_digest
            <> '2ee6db433ac8976c754c1566b39eb17950d8c9e1a9e5e56d6d96e45a39342dc7'
        OR readiness_digest
            <> '0e69552f26e09949d44b87c7ae7680432ff2c36a0027230efcf541cc4324cd9f'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v2()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_live_source_selection_preflight_drift';
    END IF;
END;
$preflight$;

DO $patch_priority$
DECLARE
    identity TEXT;
    expected_before_digest TEXT;
    expected_after_digest TEXT;
    function_oid OID;
    definition TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
    observed_definition_digest TEXT;
    old_fragment TEXT :=
        E'    SELECT pg_catalog.count(*)\n    INTO higher_live_count\n    FROM public.runtime_deployments AS deployment\n    WHERE deployment.phase = ''live'';';
    new_fragment TEXT :=
        E'    SELECT pg_catalog.count(*)\n    INTO higher_live_count\n    FROM public.runtime_deployments AS deployment\n    WHERE deployment.phase = ''live''\n        AND NOT EXISTS (\n            SELECT 1\n            FROM public.runtime_drain_intents_v2 AS drain\n            INNER JOIN public.runtime_slot_writer_fences_v2 AS slot\n                ON slot.slot_guild_id = drain.slot_guild_id\n                AND slot.slot_ruleset_key =\n                    drain.slot_ruleset_key\n                AND slot.pending_drain_intent_id =\n                    drain.drain_intent_id\n                AND slot.pending_product_operation_id =\n                    drain.product_operation_id\n                AND slot.pending_tenant_id = drain.tenant_id\n                AND slot.pending_installation_id =\n                    drain.installation_id\n                AND slot.pending_deployment_id =\n                    drain.deployment_id\n                AND slot.pending_expected_revision =\n                    drain.expected_revision\n            WHERE drain.intent_state IN (\n                    ''pending'',\n                    ''route_absent_acknowledged''\n                )\n                AND drain.tenant_id = deployment.tenant_id\n                AND drain.installation_id =\n                    deployment.installation_id\n                AND drain.deployment_id = deployment.deployment_id\n                AND drain.slot_guild_id = deployment.guild_id\n                AND drain.slot_ruleset_key =\n                    deployment.ruleset_key\n        );';
BEGIN
    FOREACH identity IN ARRAY ARRAY[
        'starring_runtime_private_v2.starring_runtime_pending_drain_candidate_v2()',
        'public.starring_runtime_startup_recovery_execute_pending_drain_v2(text,bigint,bigint,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean,text)'
    ]
    LOOP
        IF identity LIKE 'starring_runtime_private_v2.%' THEN
            expected_before_digest :=
                '91d4d64ae0f1b3053ec91f1c1b07164fce08311e26b58718eca672f3fadee909';
            expected_after_digest :=
                '43889d46cada8cb79b0474e1db761f32eac8a68ea6662886db0439e5315cef2a';
        ELSE
            expected_before_digest :=
                '5414574cde39e1c59410e1cac6ccb975a87d16f4807f3ae33b8f28b8157a8e9b';
            expected_after_digest :=
                '6289257130f1327b0f5378b5ad998899de26217ecca62c421a5f3ba8257e38e6';
        END IF;

        function_oid := pg_catalog.to_regprocedure(identity);

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
            'return_type', function_row.prorettype::TEXT,
            'all_argument_types',
                pg_catalog.to_jsonb(function_row.proallargtypes),
            'argument_modes',
                pg_catalog.to_jsonb(function_row.proargmodes),
            'argument_names',
                pg_catalog.to_jsonb(function_row.proargnames)
            )
        INTO definition, metadata_before
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid = function_oid;

        IF definition IS NULL
            OR pg_catalog.encode(
                pg_catalog.sha256(pg_catalog.convert_to(definition, 'UTF8')),
                'hex'
            ) <> expected_before_digest
            OR pg_catalog.char_length(definition)
                - pg_catalog.char_length(pg_catalog.replace(
                    definition,
                    old_fragment,
                    ''
                ))
                <> pg_catalog.char_length(old_fragment)
            OR pg_catalog.strpos(definition, new_fragment) <> 0
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_pending_drain_live_source_selection_patch_drift';
        END IF;

        EXECUTE pg_catalog.replace(
            definition,
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
            'return_type', function_row.prorettype::TEXT,
            'all_argument_types',
                pg_catalog.to_jsonb(function_row.proallargtypes),
            'argument_modes',
                pg_catalog.to_jsonb(function_row.proargmodes),
            'argument_names',
                pg_catalog.to_jsonb(function_row.proargnames)
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
            OR observed_definition_digest <> expected_after_digest
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
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_pending_drain_live_source_selection_patch_postcondition_drift';
        END IF;
    END LOOP;
END;
$patch_priority$;

DO $refresh_manifest$
DECLARE
    function_oid OID;
    definition TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
    observed_definition_digest TEXT;
    old_digest TEXT :=
        '1d85a38b5d30b20a4b15c6adc70af3e08ea66901465ba83b2d2bf8d200ccbfca';
    new_digest TEXT :=
        '4d9eb1fdaa4eac009105ab65b9115e523f52b1128cde4ea3ebcc85f006ea08b9';
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
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(definition, 'UTF8')),
            'hex'
        ) <> '2ee6db433ac8976c754c1566b39eb17950d8c9e1a9e5e56d6d96e45a39342dc7'
        OR pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                old_digest,
                ''
            ))
            <> pg_catalog.char_length(old_digest)
        OR pg_catalog.strpos(definition, new_digest) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_live_source_manifest_drift';
    END IF;

    EXECUTE pg_catalog.replace(
        definition,
        old_digest,
        new_digest
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
            <> '99dfc39ef03194161fe67419d87fd2890145980f3147151864ea7552bec36886'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_live_source_manifest_postcondition_drift';
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
    old_digest TEXT :=
        '2ee6db433ac8976c754c1566b39eb17950d8c9e1a9e5e56d6d96e45a39342dc7';
    new_digest TEXT :=
        '99dfc39ef03194161fe67419d87fd2890145980f3147151864ea7552bec36886';
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
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(definition, 'UTF8')),
            'hex'
        ) <> '0e69552f26e09949d44b87c7ae7680432ff2c36a0027230efcf541cc4324cd9f'
        OR pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                old_digest,
                ''
            ))
            <> pg_catalog.char_length(old_digest)
        OR pg_catalog.strpos(definition, new_digest) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_live_source_readiness_drift';
    END IF;

    EXECUTE pg_catalog.replace(
        definition,
        old_digest,
        new_digest
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
            <> '98ed1251e3339ffb452ed12334699e93f43e2ea3cd7d327bc3d2a11fe12b9fb2'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_live_source_readiness_postcondition_drift';
    END IF;
END;
$refresh_readiness$;

DO $postflight$
DECLARE
    helper_digest TEXT;
    execution_digest TEXT;
    manifest_digest TEXT;
    readiness_digest TEXT;
BEGIN
    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'starring_runtime_private_v2.starring_runtime_pending_drain_candidate_v2()'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO helper_digest;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_startup_recovery_execute_pending_drain_v2(text,bigint,bigint,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean,text)'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO execution_digest;

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

    IF helper_digest
            <> '43889d46cada8cb79b0474e1db761f32eac8a68ea6662886db0439e5315cef2a'
        OR execution_digest
            <> '6289257130f1327b0f5378b5ad998899de26217ecca62c421a5f3ba8257e38e6'
        OR manifest_digest
            <> '99dfc39ef03194161fe67419d87fd2890145980f3147151864ea7552bec36886'
        OR readiness_digest
            <> '98ed1251e3339ffb452ed12334699e93f43e2ea3cd7d327bc3d2a11fe12b9fb2'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v2()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_live_source_selection_postflight_drift';
    END IF;
END;
$postflight$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
