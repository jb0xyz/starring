SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
);

LOCK TABLE
    public.runtime_deployments,
    public.runtime_serving_leases,
    public.runtime_product_operations_v2,
    public.runtime_drain_intents_v2,
    public.runtime_slot_writer_fences_v2
IN ACCESS EXCLUSIVE MODE;

DO $preflight$
DECLARE
    core_digest TEXT;
    exact_target_manifest_digest TEXT;
    exact_target_readiness_digest TEXT;
    serving_manifest_digest TEXT;
    serving_readiness_digest TEXT;
    execution_manifest_digest TEXT;
    execution_readiness_digest TEXT;
BEGIN
    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text,bytea,text,bytea,text)'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO core_digest;
    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_exact_target_schema_manifest_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO exact_target_manifest_digest;
    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_exact_target_database_readiness_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO exact_target_readiness_digest;
    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_serving_schema_manifest_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO serving_manifest_digest;
    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_serving_database_readiness_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO serving_readiness_digest;
    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_execution_schema_manifest_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO execution_manifest_digest;
    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_execution_database_readiness_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO execution_readiness_digest;

    IF core_digest
            IS DISTINCT FROM '9668f69cf24d956d4f1f293331c30c81fb46eaea6fcb86e39f05577f02d4c1ac'
        OR public.starring_runtime_interaction_schema_manifest_v1()
            IS DISTINCT FROM TRUE
        OR public.starring_runtime_exact_target_schema_manifest_v1()
            IS DISTINCT FROM TRUE
        OR exact_target_manifest_digest
            IS DISTINCT FROM '5fe0365d0cb4912a01778f3d30a2d649a40e82c5b964ba9e2e7e1901e79eb109'
        OR exact_target_readiness_digest
            IS DISTINCT FROM 'e4bae4b38acc529accd4401af853eb7e96d2a34ad8fb1224b9965166ff40c229'
        OR public.starring_runtime_serving_schema_manifest_v1()
            IS DISTINCT FROM TRUE
        OR serving_manifest_digest
            IS DISTINCT FROM '14a0c119d8fa0b7a85b72509df29156a6c869b5e3f240bc8fffc89fd1a86c4c9'
        OR serving_readiness_digest
            IS DISTINCT FROM '1c0c79c6fbf528f28fb56e91a54b78cd1fe17c70d2bc3e8d7e3dc515d8a7f8f7'
        OR public.starring_runtime_execution_schema_manifest_v1()
            IS DISTINCT FROM TRUE
        OR execution_manifest_digest
            IS DISTINCT FROM '223a7d5a5aba3e418ed310c4cffa8271193af158f12729f74ad85be97123c292'
        OR execution_readiness_digest
            IS DISTINCT FROM '48a10f783603fe02879f2a1cddbecbb39541ac0ca154c77f7b1e0eef8d9f6834'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_product_drain_first_apply_eligibility_preflight_drift';
    END IF;
END;
$preflight$;

DO $patch_core$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text,bytea,text,bytea,text)'
    );

    previous_fragment :=
        '    exception_constraint_name TEXT;' || E'\n' ||
        '    slot_fence_row RECORD;' || E'\n' ||
        'BEGIN';
    next_fragment :=
        '    exception_constraint_name TEXT;' || E'\n' ||
        '    slot_fence_row RECORD;' || E'\n' ||
        '    lane_head_deployment_id TEXT;' || E'\n' ||
        '    unresolved_deployment_count BIGINT;' || E'\n' ||
        '    unresolved_deployment_id TEXT;' || E'\n' ||
        '    serving_row public.runtime_serving_leases%ROWTYPE;' || E'\n' ||
        '    serving_found BOOLEAN;' || E'\n' ||
        '    eligibility_clock TIMESTAMPTZ;' || E'\n' ||
        'BEGIN';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_product_drain_first_apply_eligibility_declaration_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    SELECT deployment.*' || E'\n' ||
        '    INTO deployment_row' || E'\n' ||
        '    FROM public.runtime_deployments AS deployment' || E'\n' ||
        '    WHERE deployment.tenant_id = requested_tenant_id';
    next_fragment :=
        '    PERFORM deployment.deployment_id' || E'\n' ||
        '    FROM public.runtime_deployments AS deployment' || E'\n' ||
        '    WHERE deployment.guild_id = requested_slot_guild_id' || E'\n' ||
        '        AND deployment.ruleset_key = requested_slot_ruleset_key' || E'\n' ||
        '    ORDER BY deployment.runtime_generation, deployment.deployment_id' || E'\n' ||
        '    FOR UPDATE;' || E'\n' ||
        E'\n' ||
        '    SELECT deployment.*' || E'\n' ||
        '    INTO deployment_row' || E'\n' ||
        '    FROM public.runtime_deployments AS deployment' || E'\n' ||
        '    WHERE deployment.tenant_id = requested_tenant_id';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_product_drain_first_apply_eligibility_lane_lock_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    PERFORM 1' || E'\n' ||
        '    FROM public.runtime_product_operations_v2 AS product' || E'\n' ||
        '    WHERE product.product_operation_id = requested_operation_id';
    next_fragment :=
        '    SELECT deployment.deployment_id' || E'\n' ||
        '    INTO lane_head_deployment_id' || E'\n' ||
        '    FROM public.runtime_deployments AS deployment' || E'\n' ||
        '    WHERE deployment.guild_id = requested_slot_guild_id' || E'\n' ||
        '        AND deployment.ruleset_key = requested_slot_ruleset_key' || E'\n' ||
        '        AND deployment.phase NOT IN (''superseded'', ''cancelled'')' || E'\n' ||
        '    ORDER BY deployment.runtime_generation DESC, deployment.deployment_id DESC' || E'\n' ||
        '    LIMIT 1;' || E'\n' ||
        E'\n' ||
        '    SELECT pg_catalog.count(*), pg_catalog.min(deployment.deployment_id)' || E'\n' ||
        '    INTO unresolved_deployment_count, unresolved_deployment_id' || E'\n' ||
        '    FROM public.runtime_deployments AS deployment' || E'\n' ||
        '    WHERE deployment.guild_id = requested_slot_guild_id' || E'\n' ||
        '        AND deployment.ruleset_key = requested_slot_ruleset_key' || E'\n' ||
        '        AND deployment.phase NOT IN (''live'', ''superseded'', ''cancelled'');' || E'\n' ||
        E'\n' ||
        '    SELECT lease.*' || E'\n' ||
        '    INTO serving_row' || E'\n' ||
        '    FROM public.runtime_serving_leases AS lease' || E'\n' ||
        '    WHERE lease.guild_id = requested_slot_guild_id' || E'\n' ||
        '        AND lease.ruleset_key = requested_slot_ruleset_key' || E'\n' ||
        '    FOR UPDATE;' || E'\n' ||
        '    serving_found := FOUND;' || E'\n' ||
        '    eligibility_clock := pg_catalog.clock_timestamp();' || E'\n' ||
        E'\n' ||
        '    IF lane_head_deployment_id IS DISTINCT FROM requested_deployment_id' || E'\n' ||
        '        OR (' || E'\n' ||
        '            deployment_row.phase = ''awaiting_gateway_ready''' || E'\n' ||
        '            AND (' || E'\n' ||
        '                unresolved_deployment_count IS DISTINCT FROM 1' || E'\n' ||
        '                OR unresolved_deployment_id' || E'\n' ||
        '                    IS DISTINCT FROM requested_deployment_id' || E'\n' ||
        '                OR (' || E'\n' ||
        '                    serving_found' || E'\n' ||
        '                    AND serving_row.connected' || E'\n' ||
        '                    AND serving_row.serving' || E'\n' ||
        '                    AND serving_row.expires_at > eligibility_clock' || E'\n' ||
        '                )' || E'\n' ||
        '            )' || E'\n' ||
        '        )' || E'\n' ||
        '        OR (' || E'\n' ||
        '            deployment_row.phase = ''live''' || E'\n' ||
        '            AND (' || E'\n' ||
        '                unresolved_deployment_count IS DISTINCT FROM 0' || E'\n' ||
        '                OR NOT serving_found' || E'\n' ||
        '                OR serving_row.tenant_id' || E'\n' ||
        '                    IS DISTINCT FROM deployment_row.tenant_id' || E'\n' ||
        '                OR serving_row.installation_id' || E'\n' ||
        '                    IS DISTINCT FROM deployment_row.installation_id' || E'\n' ||
        '                OR serving_row.deployment_id' || E'\n' ||
        '                    IS DISTINCT FROM deployment_row.deployment_id' || E'\n' ||
        '                OR serving_row.attestation_id' || E'\n' ||
        '                    IS DISTINCT FROM deployment_row.live_attestation_id' || E'\n' ||
        '                OR serving_row.runtime_generation' || E'\n' ||
        '                    IS DISTINCT FROM deployment_row.runtime_generation' || E'\n' ||
        '                OR serving_row.guild_id' || E'\n' ||
        '                    IS DISTINCT FROM deployment_row.guild_id' || E'\n' ||
        '                OR serving_row.ruleset_key' || E'\n' ||
        '                    IS DISTINCT FROM deployment_row.ruleset_key' || E'\n' ||
        '                OR serving_row.target_version' || E'\n' ||
        '                    IS DISTINCT FROM deployment_row.target_version' || E'\n' ||
        '                OR serving_row.target_content_hash' || E'\n' ||
        '                    IS DISTINCT FROM deployment_row.target_content_hash' || E'\n' ||
        '                OR serving_row.binding_revision' || E'\n' ||
        '                    IS DISTINCT FROM deployment_row.binding_revision' || E'\n' ||
        '                OR serving_row.binding_fingerprint' || E'\n' ||
        '                    IS DISTINCT FROM deployment_row.binding_fingerprint' || E'\n' ||
        '            )' || E'\n' ||
        '        )' || E'\n' ||
        '    THEN' || E'\n' ||
        '        RAISE EXCEPTION USING' || E'\n' ||
        '            ERRCODE = ''RX001'',' || E'\n' ||
        '            MESSAGE =' || E'\n' ||
        '                ''runtime_product_drain_first_apply_deployment_mismatch'';' || E'\n' ||
        '    END IF;' || E'\n' ||
        E'\n' ||
        '    PERFORM 1' || E'\n' ||
        '    FROM public.runtime_product_operations_v2 AS product' || E'\n' ||
        '    WHERE product.product_operation_id = requested_operation_id';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_product_drain_first_apply_eligibility_gate_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    EXECUTE definition;
END;
$patch_core$;

DO $patch_manifest$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_schema_manifest_v1()'
    );
    previous_fragment :=
        '    RETURN observed_count = 623' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''ce1e493041abc52b6f4073da976a99b547b32a92d7ff171b64eef791354ff491'';';
    next_fragment :=
        '    RETURN observed_count = 623' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''68588695f6c82923f7830faa333d16533f86b43f3f47bf69756bd7447c1aae91'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_product_drain_first_apply_eligibility_manifest_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$patch_manifest$;

DO $patch_readiness$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_database_readiness_v1()'
    );
    previous_fragment :=
        '''223a7d5a5aba3e418ed310c4cffa8271193af158f12729f74ad85be97123c292''::TEXT';
    next_fragment :=
        '''3a014a2c92d5a7da93867f10d8e5d8f9ca1ac5f49666ad57558d49f46b66b2a0''::TEXT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_product_drain_first_apply_eligibility_readiness_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$patch_readiness$;

DO $postflight$
DECLARE
    core_digest TEXT;
    exact_target_manifest_digest TEXT;
    exact_target_readiness_digest TEXT;
    serving_manifest_digest TEXT;
    serving_readiness_digest TEXT;
    execution_manifest_digest TEXT;
    execution_readiness_digest TEXT;
    core_definition TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO core_definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text,bytea,text,bytea,text)'
    );
    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(core_definition, 'UTF8')),
        'hex'
    )
    INTO core_digest;
    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_exact_target_schema_manifest_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO exact_target_manifest_digest;
    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_exact_target_database_readiness_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO exact_target_readiness_digest;
    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_serving_schema_manifest_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO serving_manifest_digest;
    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_serving_database_readiness_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO serving_readiness_digest;
    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_execution_schema_manifest_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO execution_manifest_digest;
    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_execution_database_readiness_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO execution_readiness_digest;

    IF core_digest
            IS DISTINCT FROM '534dcc1f973d1b37e9f72e28b01ad6541f2ff4293b1cbc5c3b5893764b7fed6e'
        OR pg_catalog.strpos(
            core_definition,
            'ORDER BY deployment.runtime_generation, deployment.deployment_id'
        ) = 0
        OR pg_catalog.strpos(
            core_definition,
            'eligibility_clock := pg_catalog.clock_timestamp()'
        ) = 0
        OR public.starring_runtime_interaction_schema_manifest_v1()
            IS DISTINCT FROM TRUE
        OR public.starring_runtime_exact_target_schema_manifest_v1()
            IS DISTINCT FROM TRUE
        OR exact_target_manifest_digest
            IS DISTINCT FROM '5fe0365d0cb4912a01778f3d30a2d649a40e82c5b964ba9e2e7e1901e79eb109'
        OR exact_target_readiness_digest
            IS DISTINCT FROM 'e4bae4b38acc529accd4401af853eb7e96d2a34ad8fb1224b9965166ff40c229'
        OR public.starring_runtime_serving_schema_manifest_v1()
            IS DISTINCT FROM TRUE
        OR serving_manifest_digest
            IS DISTINCT FROM '14a0c119d8fa0b7a85b72509df29156a6c869b5e3f240bc8fffc89fd1a86c4c9'
        OR serving_readiness_digest
            IS DISTINCT FROM '1c0c79c6fbf528f28fb56e91a54b78cd1fe17c70d2bc3e8d7e3dc515d8a7f8f7'
        OR public.starring_runtime_execution_schema_manifest_v1()
            IS DISTINCT FROM TRUE
        OR execution_manifest_digest
            IS DISTINCT FROM '3a014a2c92d5a7da93867f10d8e5d8f9ca1ac5f49666ad57558d49f46b66b2a0'
        OR execution_readiness_digest
            IS DISTINCT FROM '17fdc258083036bc6f6faceee4dbd900f166ce15f711e99ea87e60ae03e3aa31'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_product_drain_first_apply_eligibility_postflight_drift';
    END IF;
END;
$postflight$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
