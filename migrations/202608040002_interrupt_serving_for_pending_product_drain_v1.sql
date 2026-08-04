SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended(
        'starring-runtime-serving-pending-product-drain-v1',
        0
    )
);

DO $serving_pending_product_drain$
DECLARE
    function_identity TEXT :=
        'public.starring_runtime_serving_heartbeat_v1(text,text,text,text,text,bigint,bigint,bigint,bigint)';
    function_oid OID;
    function_definition TEXT;
    old_declaration_fragment TEXT :=
        E'    candidate_guild_id TEXT;\n    candidate_ruleset_key TEXT;\nBEGIN\n';
    new_declaration_fragment TEXT :=
        E'    candidate_guild_id TEXT;\n    candidate_ruleset_key TEXT;\n    slot_fence_row public.runtime_slot_writer_fences_v2%ROWTYPE;\n    drain_row public.runtime_drain_intents_v2%ROWTYPE;\n    pending_source RECORD;\nBEGIN\n';
    old_guard_fragment TEXT :=
        E'    END IF;\n\n    SELECT lease.*\n    INTO serving_row\n    FROM public.runtime_serving_leases AS lease\n    WHERE lease.guild_id = deployment_row.guild_id\n        AND lease.ruleset_key = deployment_row.ruleset_key;\n';
    new_guard_fragment TEXT :=
        E'    END IF;\n\n    SELECT fence.*\n    INTO slot_fence_row\n    FROM public.runtime_slot_writer_fences_v2 AS fence\n    WHERE fence.slot_guild_id = deployment_row.guild_id\n        AND fence.slot_ruleset_key = deployment_row.ruleset_key;\n\n    IF NOT FOUND\n        OR slot_fence_row.writer_epoch\n            NOT BETWEEN 1 AND 9223372036854775807\n        OR (\n            slot_fence_row.pending_drain_intent_id IS NULL\n            AND (\n                slot_fence_row.pending_product_operation_id IS NOT NULL\n                OR slot_fence_row.pending_tenant_id IS NOT NULL\n                OR slot_fence_row.pending_installation_id IS NOT NULL\n                OR slot_fence_row.pending_deployment_id IS NOT NULL\n                OR slot_fence_row.pending_expected_revision IS NOT NULL\n                OR slot_fence_row.pending_marked_at IS NOT NULL\n            )\n        )\n    THEN\n        RAISE EXCEPTION USING\n            ERRCODE = ''RS004'',\n            MESSAGE = ''runtime_serving_heartbeat_product_drain_invalid'';\n    END IF;\n\n    IF slot_fence_row.pending_drain_intent_id IS NOT NULL THEN\n        IF slot_fence_row.pending_product_operation_id IS NULL\n            OR slot_fence_row.pending_tenant_id\n                IS DISTINCT FROM expected_tenant_id\n            OR slot_fence_row.pending_installation_id\n                IS DISTINCT FROM expected_installation_id\n            OR slot_fence_row.pending_deployment_id\n                IS DISTINCT FROM expected_deployment_id\n            OR slot_fence_row.pending_expected_revision\n                IS DISTINCT FROM deployment_row.revision\n            OR slot_fence_row.pending_marked_at IS NULL\n            OR NOT pg_catalog.isfinite(slot_fence_row.pending_marked_at)\n        THEN\n            RAISE EXCEPTION USING\n                ERRCODE = ''RS004'',\n                MESSAGE = ''runtime_serving_heartbeat_product_drain_invalid'';\n        END IF;\n\n        SELECT drain.*\n        INTO drain_row\n        FROM public.runtime_drain_intents_v2 AS drain\n        WHERE drain.drain_intent_id\n            = slot_fence_row.pending_drain_intent_id;\n\n        IF NOT FOUND\n            OR drain_row.product_operation_id\n                IS DISTINCT FROM slot_fence_row.pending_product_operation_id\n            OR drain_row.tenant_id IS DISTINCT FROM expected_tenant_id\n            OR drain_row.installation_id\n                IS DISTINCT FROM expected_installation_id\n            OR drain_row.deployment_id\n                IS DISTINCT FROM expected_deployment_id\n            OR drain_row.slot_guild_id\n                IS DISTINCT FROM deployment_row.guild_id\n            OR drain_row.slot_ruleset_key\n                IS DISTINCT FROM deployment_row.ruleset_key\n            OR drain_row.expected_revision\n                IS DISTINCT FROM deployment_row.revision\n            OR drain_row.intent_revision\n                NOT BETWEEN 1 AND 9223372036854775807\n            OR drain_row.canonical_state_digest !~ ''^[0-9a-f]{64}$''\n        THEN\n            RAISE EXCEPTION USING\n                ERRCODE = ''RS004'',\n                MESSAGE = ''runtime_serving_heartbeat_product_drain_invalid'';\n        END IF;\n\n        SELECT source.*\n        INTO pending_source\n        FROM public.starring_runtime_serving_observe_pending_drain_source_v1(\n            drain_row.drain_intent_id,\n            drain_row.intent_revision,\n            drain_row.canonical_state_digest\n        ) AS source;\n\n        IF NOT FOUND\n            OR pending_source.outcome_name IS DISTINCT FROM ''current''\n            OR pending_source.drain_intent_id\n                IS DISTINCT FROM drain_row.drain_intent_id\n            OR pending_source.source_intent_revision\n                IS DISTINCT FROM drain_row.intent_revision\n            OR pending_source.source_state_digest\n                IS DISTINCT FROM drain_row.canonical_state_digest\n            OR pending_source.operation_id !~ ''^[0-9a-f]{32}$''\n            OR pending_source.tenant_id IS DISTINCT FROM expected_tenant_id\n            OR pending_source.installation_id\n                IS DISTINCT FROM expected_installation_id\n            OR pending_source.deployment_id\n                IS DISTINCT FROM expected_deployment_id\n            OR pending_source.attestation_digest\n                IS DISTINCT FROM expected_attestation_id\n            OR pg_catalog.jsonb_typeof(pending_source.process_identity)\n                IS DISTINCT FROM ''object''\n            OR pending_source.process_identity #>> ''{target,guild_id}''\n                IS DISTINCT FROM deployment_row.guild_id\n            OR pending_source.process_identity #>> ''{target,ruleset_key}''\n                IS DISTINCT FROM deployment_row.ruleset_key\n            OR pending_source.process_identity #>> ''{target,version}''\n                IS DISTINCT FROM deployment_row.target_version::TEXT\n            OR pending_source.process_identity #>> ''{target,content_hash}''\n                IS DISTINCT FROM deployment_row.target_content_hash\n            OR pending_source.process_identity\n                    #>> ''{target,binding_revision}''\n                IS DISTINCT FROM deployment_row.binding_revision::TEXT\n            OR pending_source.process_identity\n                    #>> ''{target,binding_fingerprint}''\n                IS DISTINCT FROM deployment_row.binding_fingerprint\n            OR pending_source.process_identity ->> ''runtime_generation''\n                IS DISTINCT FROM expected_runtime_generation::TEXT\n            OR pending_source.process_identity ->> ''process_instance_id''\n                IS DISTINCT FROM expected_process_instance_id\n            OR pending_source.lease_epoch\n                IS DISTINCT FROM expected_lease_epoch\n            OR NOT (\n                pending_source.serving_revision = expected_revision\n                OR (\n                    expected_revision < 9223372036854775807\n                    AND pending_source.serving_revision\n                        = expected_revision + 1\n                )\n            )\n            OR pending_source.connected IS DISTINCT FROM TRUE\n            OR pending_source.serving IS DISTINCT FROM TRUE\n            OR pending_source.observed_at IS NULL\n            OR NOT pg_catalog.isfinite(pending_source.observed_at)\n        THEN\n            RAISE EXCEPTION USING\n                ERRCODE = ''RS004'',\n                MESSAGE = ''runtime_serving_heartbeat_product_drain_invalid'';\n        END IF;\n\n        RAISE EXCEPTION USING\n            ERRCODE = ''RS003'',\n            MESSAGE = ''runtime_serving_heartbeat_product_drain_required'';\n    END IF;\n\n    SELECT lease.*\n    INTO serving_row\n    FROM public.runtime_serving_leases AS lease\n    WHERE lease.guild_id = deployment_row.guild_id\n        AND lease.ruleset_key = deployment_row.ruleset_key;\n';
    old_definition_digest TEXT :=
        '51a3dcd58a44f60320a0bcb5b671fce0eeffa10d08e0a5b8c3880a07ce1802b3';
    expected_definition_digest TEXT :=
        '37e1714c04f1ca66f6b12d571098fff12c31d2bb2aa55355c90c0db417f021de';
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
                old_declaration_fragment,
                ''
            ))
            <> pg_catalog.char_length(old_declaration_fragment)
        OR pg_catalog.char_length(function_definition)
            - pg_catalog.char_length(pg_catalog.replace(
                function_definition,
                old_guard_fragment,
                ''
            ))
            <> pg_catalog.char_length(old_guard_fragment)
        OR pg_catalog.strpos(
            function_definition,
            new_declaration_fragment
        ) <> 0
        OR pg_catalog.strpos(function_definition, new_guard_fragment) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'runtime_serving_pending_product_drain_precondition_drift';
    END IF;

    function_definition := pg_catalog.replace(
        function_definition,
        old_declaration_fragment,
        new_declaration_fragment
    );
    function_definition := pg_catalog.replace(
        function_definition,
        old_guard_fragment,
        new_guard_fragment
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
            old_declaration_fragment
        ) <> 0
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(function_oid),
            new_declaration_fragment
        ) = 0
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(function_oid),
            new_guard_fragment
        ) = 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'runtime_serving_pending_product_drain_postcondition_drift';
    END IF;
END;
$serving_pending_product_drain$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
