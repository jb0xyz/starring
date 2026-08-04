SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
);

LOCK TABLE
    public.runtime_deployments,
    public.runtime_product_operations_v2,
    public.runtime_drain_intents_v2,
    public.runtime_slot_writer_fences_v2,
    public.product_auth_sessions,
    public.authoring_promotions,
    public.activation_requests,
    public.automation_ruleset_activations,
    public._sqlx_migrations
IN ACCESS SHARE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    applied_count BIGINT;
    applied_head BIGINT;
    failed_count BIGINT;
    migration_checksum TEXT;
    authority_projection_digest TEXT;
    consume_digest TEXT;
    supersession_digest TEXT;
    execution_manifest_digest TEXT;
    execution_readiness_digest TEXT;
    exact_manifest_digest TEXT;
    exact_readiness_digest TEXT;
    serving_manifest_digest TEXT;
    serving_readiness_digest TEXT;
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
    WHERE migration.version = 202608030002
        AND migration.success;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_product_apply_authority_projection_v1(text,text,text,text,bytea,text,text,text,text,bigint,text,timestamp with time zone,timestamp with time zone,text,boolean,text)'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO authority_projection_digest;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'starring_runtime_private_v2.starring_product_apply_consume_lock_core_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text)'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO consume_digest;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'starring_runtime_private_v2.starring_runtime_product_drain_source_supersession_exact_v2(public.runtime_deployments,jsonb,public.runtime_drain_intents_v2,jsonb,timestamp with time zone)'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO supersession_digest;

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
    INTO execution_manifest_digest;

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
    INTO execution_readiness_digest;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_exact_target_schema_manifest_v1()'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO exact_manifest_digest;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_exact_target_database_readiness_v1()'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO exact_readiness_digest;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_serving_schema_manifest_v1()'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO serving_manifest_digest;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_serving_database_readiness_v1()'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO serving_readiness_digest;

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) IS DISTINCT FROM common_owner
        OR applied_count IS DISTINCT FROM 121
        OR applied_head IS DISTINCT FROM 202608030002
        OR failed_count IS DISTINCT FROM 0
        OR migration_checksum
            IS DISTINCT FROM 'b3862f35f454b96e35e2ec3fa72e6b7a341f49c4a96d927cd9c7ccfbbc9fdcbbadeab96b1128210a49222f0d8fa057aa'
        OR authority_projection_digest
            IS DISTINCT FROM 'dbbd17de881221f5c3c12f26250709045efe388457d8e29b7fd83b751bfca68a'
        OR consume_digest
            IS DISTINCT FROM 'd3f0ee0c510f3f4007b55ab5b0eb66e524758f8f8700518e6649e0baf476e9ea'
        OR supersession_digest
            IS DISTINCT FROM '9c9b255b233d304fee24968e7f71438066d729e24808c1cc222268a4401e4e70'
        OR execution_manifest_digest
            IS DISTINCT FROM '99dfc39ef03194161fe67419d87fd2890145980f3147151864ea7552bec36886'
        OR execution_readiness_digest
            IS DISTINCT FROM '98ed1251e3339ffb452ed12334699e93f43e2ea3cd7d327bc3d2a11fe12b9fb2'
        OR exact_manifest_digest
            IS DISTINCT FROM 'c8e5559234a54c8b4b3be342a98badc0f63d3fb4ae59beea50d105938730ec7d'
        OR exact_readiness_digest
            IS DISTINCT FROM '35903afa3bb9bebe712559a80a503823f4eeedf0d15ebd3d24ce3dbf706b5c14'
        OR serving_manifest_digest
            IS DISTINCT FROM '90ab51452bf5c3ba8074e0bce0f6a643ba374e79497962d0bf2d5aeec062fa96'
        OR serving_readiness_digest
            IS DISTINCT FROM '918e4be248c37e622b1f5b22cb9e252a450b65b295157681c647855d0c0150b9'
        OR public.starring_runtime_execution_schema_manifest_v1()
            IS DISTINCT FROM TRUE
        OR public.starring_runtime_exact_target_schema_manifest_v1()
            IS DISTINCT FROM TRUE
        OR public.starring_runtime_exact_target_schema_manifest_v2()
            IS DISTINCT FROM TRUE
        OR public.starring_runtime_serving_schema_manifest_v1()
            IS DISTINCT FROM TRUE
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'product_apply_consume_commit_contract_preflight_drift';
    END IF;
END;
$preflight$;

DO $create_consume_authority_projection$
DECLARE
    source_identity CONSTANT TEXT :=
        'public.starring_product_apply_authority_projection_v1(text,text,text,text,bytea,text,text,text,text,bigint,text,timestamp with time zone,timestamp with time zone,text,boolean,text)';
    target_identity CONSTANT TEXT :=
        'starring_runtime_private_v2.starring_product_apply_authority_projection_at_v2(text,text,text,text,bytea,text,text,text,text,bigint,text,timestamp with time zone,timestamp with time zone,text,boolean,text,timestamp with time zone)';
    source_oid OID;
    target_oid OID;
    definition TEXT;
    observed_definition_digest TEXT;
    old_identity CONSTANT TEXT :=
        'CREATE OR REPLACE FUNCTION public.starring_product_apply_authority_projection_v1(';
    new_identity CONSTANT TEXT :=
        'CREATE OR REPLACE FUNCTION starring_runtime_private_v2.starring_product_apply_authority_projection_at_v2(';
    old_arguments CONSTANT TEXT :=
        'expected_guild_owner boolean, expected_payload_digest text)';
    new_arguments CONSTANT TEXT :=
        'expected_guild_owner boolean, expected_payload_digest text, requested_authorization_clock timestamp with time zone)';
    old_clock CONSTANT TEXT :=
        '    mutation_clock := pg_catalog.clock_timestamp();';
    new_clock CONSTANT TEXT :=
        $new_clock$    IF NOT pg_catalog.isfinite(requested_authorization_clock)
        OR requested_authorization_clock
            IS DISTINCT FROM pg_catalog.transaction_timestamp()
    THEN
        RETURN pg_catalog.jsonb_build_object(
            'outcome',
            'authorization_stale'
        );
    END IF;
    mutation_clock := requested_authorization_clock;$new_clock$;
BEGIN
    source_oid := pg_catalog.to_regprocedure(source_identity);
    target_oid := pg_catalog.to_regprocedure(target_identity);

    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = source_oid;

    IF target_oid IS NOT NULL
        OR definition IS NULL
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(definition, 'UTF8')),
            'hex'
        ) IS DISTINCT FROM 'dbbd17de881221f5c3c12f26250709045efe388457d8e29b7fd83b751bfca68a'
        OR pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                old_identity,
                ''
            )) <> pg_catalog.char_length(old_identity)
        OR pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                old_arguments,
                ''
            )) <> pg_catalog.char_length(old_arguments)
        OR pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                old_clock,
                ''
            )) <> pg_catalog.char_length(old_clock)
        OR pg_catalog.strpos(definition, new_identity) <> 0
        OR pg_catalog.strpos(definition, new_arguments) <> 0
        OR pg_catalog.strpos(definition, new_clock) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'product_apply_consume_authority_projection_create_drift';
    END IF;

    definition := pg_catalog.replace(
        definition,
        old_identity,
        new_identity
    );
    definition := pg_catalog.replace(
        definition,
        old_arguments,
        new_arguments
    );
    EXECUTE pg_catalog.replace(definition, old_clock, new_clock);
    EXECUTE 'REVOKE ALL ON FUNCTION ' || target_identity || ' FROM PUBLIC';

    target_oid := pg_catalog.to_regprocedure(target_identity);
    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO observed_definition_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = target_oid;

    IF target_oid IS NULL
        OR observed_definition_digest
            IS DISTINCT FROM 'b1f67099817245ed9d62abacb980f3b4014607b6b3e592bfbe8337d0aee486ab'
        OR NOT EXISTS (
            SELECT 1
            FROM pg_catalog.pg_proc AS function_row
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = function_row.pronamespace
            INNER JOIN pg_catalog.pg_language AS language_row
                ON language_row.oid = function_row.prolang
            WHERE function_row.oid = target_oid
                AND namespace.nspname = 'starring_runtime_private_v2'
                AND namespace.nspowner = function_row.proowner
                AND function_row.proowner = pg_catalog.to_regrole(current_user)
                AND language_row.lanname = 'plpgsql'
                AND function_row.prokind = 'f'
                AND function_row.provolatile = 'v'
                AND function_row.proisstrict
                AND function_row.prosecdef
                AND function_row.proparallel = 'u'
                AND NOT function_row.proretset
                AND NOT function_row.proleakproof
                AND function_row.pronargs = 17
                AND function_row.pronargdefaults = 0
                AND function_row.provariadic = 0
                AND function_row.prorettype = 'jsonb'::REGTYPE
                AND function_row.proconfig =
                    ARRAY['search_path=pg_catalog']::TEXT[]
                AND NOT EXISTS (
                    SELECT 1
                    FROM pg_catalog.aclexplode(COALESCE(
                        function_row.proacl,
                        pg_catalog.acldefault('f', function_row.proowner)
                    )) AS privilege
                    WHERE privilege.grantee <> function_row.proowner
                )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'product_apply_consume_authority_projection_postcondition_drift';
    END IF;
END;
$create_consume_authority_projection$;

DO $patch_consume_clock$
DECLARE
    function_identity CONSTANT TEXT :=
        'starring_runtime_private_v2.starring_product_apply_consume_lock_core_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text)';
    function_oid OID;
    definition TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
    observed_definition_digest TEXT;
    old_authorization_fragment TEXT :=
        '    authorization_clock := pg_catalog.clock_timestamp();';
    new_authorization_fragment TEXT :=
        '    authorization_clock := pg_catalog.transaction_timestamp();';
    old_activation_fragment TEXT :=
        $old_activation$    IF (authority_projection #>> '{activation,expires_at}')::TIMESTAMPTZ
            <= pg_catalog.clock_timestamp()$old_activation$;
    new_activation_fragment TEXT :=
        $new_activation$    IF (authority_projection #>> '{activation,expires_at}')::TIMESTAMPTZ
            <= pg_catalog.transaction_timestamp()$new_activation$;
    old_projection_identity TEXT :=
        'authority_projection := public.starring_product_apply_authority_projection_v1(';
    new_projection_identity TEXT :=
        'authority_projection := starring_runtime_private_v2.starring_product_apply_authority_projection_at_v2(';
    old_projection_tail TEXT :=
        E'        expected_payload_digest\n    );';
    new_projection_tail TEXT :=
        E'        expected_payload_digest,\n        pg_catalog.transaction_timestamp()\n    );';
BEGIN
    function_oid := pg_catalog.to_regprocedure(function_identity);

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
        ) IS DISTINCT FROM 'd3f0ee0c510f3f4007b55ab5b0eb66e524758f8f8700518e6649e0baf476e9ea'
        OR pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                old_authorization_fragment,
                ''
            )) <> pg_catalog.char_length(old_authorization_fragment)
        OR pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                old_activation_fragment,
                ''
            )) <> pg_catalog.char_length(old_activation_fragment)
        OR pg_catalog.strpos(definition, new_authorization_fragment) <> 0
        OR pg_catalog.strpos(definition, new_activation_fragment) <> 0
        OR pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                old_projection_identity,
                ''
            )) <> pg_catalog.char_length(old_projection_identity)
        OR pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                old_projection_tail,
                ''
            )) <> pg_catalog.char_length(old_projection_tail)
        OR pg_catalog.strpos(definition, new_projection_identity) <> 0
        OR pg_catalog.strpos(definition, new_projection_tail) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'product_apply_consume_authorization_clock_patch_drift';
    END IF;

    definition := pg_catalog.replace(
        definition,
        old_authorization_fragment,
        new_authorization_fragment
    );
    definition := pg_catalog.replace(
        definition,
        old_activation_fragment,
        new_activation_fragment
    );
    definition := pg_catalog.replace(
        definition,
        old_projection_identity,
        new_projection_identity
    );
    EXECUTE pg_catalog.replace(
        definition,
        old_projection_tail,
        new_projection_tail
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
        OR observed_definition_digest
            IS DISTINCT FROM 'c05a220d2b9b4d255a27b5826173c9574225a70377508e5bbf5fb788dadcd62c'
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(function_oid),
            old_authorization_fragment
        ) <> 0
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(function_oid),
            old_activation_fragment
        ) <> 0
        OR pg_catalog.char_length(pg_catalog.pg_get_functiondef(function_oid))
            - pg_catalog.char_length(pg_catalog.replace(
                pg_catalog.pg_get_functiondef(function_oid),
                new_authorization_fragment,
                ''
            )) <> pg_catalog.char_length(new_authorization_fragment)
        OR pg_catalog.char_length(pg_catalog.pg_get_functiondef(function_oid))
            - pg_catalog.char_length(pg_catalog.replace(
                pg_catalog.pg_get_functiondef(function_oid),
                new_activation_fragment,
                ''
            )) <> pg_catalog.char_length(new_activation_fragment)
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(function_oid),
            old_projection_identity
        ) <> 0
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(function_oid),
            old_projection_tail
        ) <> 0
        OR pg_catalog.char_length(pg_catalog.pg_get_functiondef(function_oid))
            - pg_catalog.char_length(pg_catalog.replace(
                pg_catalog.pg_get_functiondef(function_oid),
                new_projection_identity,
                ''
            )) <> pg_catalog.char_length(new_projection_identity)
        OR pg_catalog.char_length(pg_catalog.pg_get_functiondef(function_oid))
            - pg_catalog.char_length(pg_catalog.replace(
                pg_catalog.pg_get_functiondef(function_oid),
                new_projection_tail,
                ''
            )) <> pg_catalog.char_length(new_projection_tail)
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'product_apply_consume_authorization_clock_postcondition_drift';
    END IF;
END;
$patch_consume_clock$;

DO $patch_live_semantics$
DECLARE
    function_identity CONSTANT TEXT :=
        'starring_runtime_private_v2.starring_runtime_product_drain_source_supersession_exact_v2(public.runtime_deployments,jsonb,public.runtime_drain_intents_v2,jsonb,timestamp with time zone)';
    function_oid OID;
    definition TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
    observed_definition_digest TEXT;
    old_fragment TEXT :=
        $old_live$        AND result_snapshot
                #> '{last_live_recovery,prior_live}'
            IS NOT DISTINCT FROM source_row.snapshot -> 'live'$old_live$;
    new_fragment TEXT :=
        $new_live$        AND (
            (((
                result_snapshot
                    #> '{last_live_recovery,prior_live}'
                    #- '{activation,activated_at}'
                ) #- '{panel_certificate,reconciled_at}'
            ) #- '{gateway_ready,ready_at}') - 'certified_at'
        ) IS NOT DISTINCT FROM (
            (((
                source_row.snapshot -> 'live'
                    #- '{activation,activated_at}'
                ) #- '{panel_certificate,reconciled_at}'
            ) #- '{gateway_ready,ready_at}') - 'certified_at'
        )
        AND pg_catalog.jsonb_typeof(
                result_snapshot #>
                    '{last_live_recovery,prior_live,activation,activated_at}'
            ) IS NOT DISTINCT FROM 'string'
        AND pg_catalog.jsonb_typeof(
                source_row.snapshot #> '{live,activation,activated_at}'
            ) IS NOT DISTINCT FROM 'string'
        AND pg_catalog.isfinite((
            result_snapshot #>>
                '{last_live_recovery,prior_live,activation,activated_at}'
        )::TIMESTAMPTZ) IS TRUE
        AND pg_catalog.isfinite((
            source_row.snapshot #>> '{live,activation,activated_at}'
        )::TIMESTAMPTZ) IS TRUE
        AND (
            result_snapshot #>>
                '{last_live_recovery,prior_live,activation,activated_at}'
        )::TIMESTAMPTZ IS NOT DISTINCT FROM (
            source_row.snapshot #>> '{live,activation,activated_at}'
        )::TIMESTAMPTZ
        AND pg_catalog.jsonb_typeof(
                result_snapshot #>
                    '{last_live_recovery,prior_live,panel_certificate,reconciled_at}'
            ) IS NOT DISTINCT FROM 'string'
        AND pg_catalog.jsonb_typeof(
                source_row.snapshot #> '{live,panel_certificate,reconciled_at}'
            ) IS NOT DISTINCT FROM 'string'
        AND pg_catalog.isfinite((
            result_snapshot #>>
                '{last_live_recovery,prior_live,panel_certificate,reconciled_at}'
        )::TIMESTAMPTZ) IS TRUE
        AND pg_catalog.isfinite((
            source_row.snapshot #>> '{live,panel_certificate,reconciled_at}'
        )::TIMESTAMPTZ) IS TRUE
        AND (
            result_snapshot #>>
                '{last_live_recovery,prior_live,panel_certificate,reconciled_at}'
        )::TIMESTAMPTZ IS NOT DISTINCT FROM (
            source_row.snapshot #>> '{live,panel_certificate,reconciled_at}'
        )::TIMESTAMPTZ
        AND pg_catalog.jsonb_typeof(
                result_snapshot #>
                    '{last_live_recovery,prior_live,gateway_ready,ready_at}'
            ) IS NOT DISTINCT FROM 'string'
        AND pg_catalog.jsonb_typeof(
                source_row.snapshot #> '{live,gateway_ready,ready_at}'
            ) IS NOT DISTINCT FROM 'string'
        AND pg_catalog.isfinite((
            result_snapshot #>>
                '{last_live_recovery,prior_live,gateway_ready,ready_at}'
        )::TIMESTAMPTZ) IS TRUE
        AND pg_catalog.isfinite((
            source_row.snapshot #>> '{live,gateway_ready,ready_at}'
        )::TIMESTAMPTZ) IS TRUE
        AND (
            result_snapshot #>>
                '{last_live_recovery,prior_live,gateway_ready,ready_at}'
        )::TIMESTAMPTZ IS NOT DISTINCT FROM (
            source_row.snapshot #>> '{live,gateway_ready,ready_at}'
        )::TIMESTAMPTZ
        AND pg_catalog.jsonb_typeof(
                result_snapshot #>
                    '{last_live_recovery,prior_live,certified_at}'
            ) IS NOT DISTINCT FROM 'string'
        AND pg_catalog.jsonb_typeof(
                source_row.snapshot #> '{live,certified_at}'
            ) IS NOT DISTINCT FROM 'string'
        AND pg_catalog.isfinite((
            result_snapshot #>>
                '{last_live_recovery,prior_live,certified_at}'
        )::TIMESTAMPTZ) IS TRUE
        AND pg_catalog.isfinite((
            source_row.snapshot #>> '{live,certified_at}'
        )::TIMESTAMPTZ) IS TRUE
        AND (
            result_snapshot #>>
                '{last_live_recovery,prior_live,certified_at}'
        )::TIMESTAMPTZ IS NOT DISTINCT FROM (
            source_row.snapshot #>> '{live,certified_at}'
        )::TIMESTAMPTZ$new_live$;
BEGIN
    function_oid := pg_catalog.to_regprocedure(function_identity);

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
        ) IS DISTINCT FROM '9c9b255b233d304fee24968e7f71438066d729e24808c1cc222268a4401e4e70'
        OR pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                old_fragment,
                ''
            )) <> pg_catalog.char_length(old_fragment)
        OR pg_catalog.strpos(definition, new_fragment) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'product_apply_consume_live_semantics_patch_drift';
    END IF;

    EXECUTE pg_catalog.replace(definition, old_fragment, new_fragment);

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
        OR observed_definition_digest
            IS DISTINCT FROM '683eef3f28edca886edca556d2cffc61cc2457f57bbefaec2e6e4b58c54b8b34'
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(function_oid),
            old_fragment
        ) <> 0
        OR pg_catalog.char_length(pg_catalog.pg_get_functiondef(function_oid))
            - pg_catalog.char_length(pg_catalog.replace(
                pg_catalog.pg_get_functiondef(function_oid),
                new_fragment,
                ''
            )) <> pg_catalog.char_length(new_fragment)
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'product_apply_consume_live_semantics_postcondition_drift';
    END IF;
END;
$patch_live_semantics$;

DO $patch_execution_manifest_inventory$
DECLARE
    function_identity CONSTANT TEXT :=
        'public.starring_runtime_execution_schema_manifest_v1()';
    function_oid OID;
    definition TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
    observed_definition_digest TEXT;
    old_identity_fragment TEXT :=
        $old_identity$        UNION
        SELECT pg_catalog.to_regprocedure(
            'starring_runtime_private_v2.starring_product_apply_consume_lock_core_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text)'
        )$old_identity$;
    new_identity_fragment TEXT :=
        $new_identity$        UNION
        SELECT pg_catalog.to_regprocedure(
            'starring_runtime_private_v2.starring_product_apply_authority_projection_at_v2(text,text,text,text,bytea,text,text,text,text,bigint,text,timestamp with time zone,timestamp with time zone,text,boolean,text,timestamp with time zone)'
        )
        UNION
        SELECT pg_catalog.to_regprocedure(
            'starring_runtime_private_v2.starring_product_apply_consume_lock_core_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text)'
        )$new_identity$;
    old_count_fragment CONSTANT TEXT :=
        '    RETURN observed_count = 972';
    new_count_fragment CONSTANT TEXT :=
        '    RETURN observed_count = 973';
BEGIN
    function_oid := pg_catalog.to_regprocedure(function_identity);

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
        ) IS DISTINCT FROM '99dfc39ef03194161fe67419d87fd2890145980f3147151864ea7552bec36886'
        OR pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                old_identity_fragment,
                ''
            )) <> pg_catalog.char_length(old_identity_fragment)
        OR pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                old_count_fragment,
                ''
            )) <> pg_catalog.char_length(old_count_fragment)
        OR pg_catalog.strpos(definition, new_identity_fragment) <> 0
        OR pg_catalog.strpos(definition, new_count_fragment) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'product_apply_consume_execution_inventory_patch_drift';
    END IF;

    definition := pg_catalog.replace(
        definition,
        old_identity_fragment,
        new_identity_fragment
    );
    EXECUTE pg_catalog.replace(
        definition,
        old_count_fragment,
        new_count_fragment
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
        OR observed_definition_digest
            IS DISTINCT FROM 'cfce91b2ff063b6e06e838d897b065d3206b04d170d01c699919f958cdd8332b'
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(function_oid),
            old_count_fragment
        ) <> 0
        OR pg_catalog.char_length(pg_catalog.pg_get_functiondef(function_oid))
            - pg_catalog.char_length(pg_catalog.replace(
                pg_catalog.pg_get_functiondef(function_oid),
                new_identity_fragment,
                ''
            )) <> pg_catalog.char_length(new_identity_fragment)
        OR pg_catalog.char_length(pg_catalog.pg_get_functiondef(function_oid))
            - pg_catalog.char_length(pg_catalog.replace(
                pg_catalog.pg_get_functiondef(function_oid),
                new_count_fragment,
                ''
            )) <> pg_catalog.char_length(new_count_fragment)
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'product_apply_consume_execution_inventory_postcondition_drift';
    END IF;
END;
$patch_execution_manifest_inventory$;

DO $patch_execution_contract$
DECLARE
    function_identity TEXT;
    function_oid OID;
    definition TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
    observed_definition_digest TEXT;
    expected_before_digest TEXT;
    expected_after_digest TEXT;
    old_fragment TEXT;
    new_fragment TEXT;
BEGIN
    FOREACH function_identity IN ARRAY ARRAY[
        'public.starring_runtime_execution_schema_manifest_v1()',
        'public.starring_runtime_execution_database_readiness_v1()',
        'public.starring_runtime_serving_database_readiness_v1()'
    ]
    LOOP
        IF function_identity =
            'public.starring_runtime_execution_schema_manifest_v1()'
        THEN
            expected_before_digest :=
                'cfce91b2ff063b6e06e838d897b065d3206b04d170d01c699919f958cdd8332b';
            expected_after_digest :=
                '3351e2c6a22ce696135c6a9c5d77de9fdb533b86413d6939f0e81af47327c919';
            old_fragment :=
                '4d9eb1fdaa4eac009105ab65b9115e523f52b1128cde4ea3ebcc85f006ea08b9';
            new_fragment :=
                '78f0806f473c2f69a668b86120229c899ec4845cea6d3ef0b57c259726e2a207';
        ELSIF function_identity =
            'public.starring_runtime_execution_database_readiness_v1()'
        THEN
            expected_before_digest :=
                '98ed1251e3339ffb452ed12334699e93f43e2ea3cd7d327bc3d2a11fe12b9fb2';
            expected_after_digest :=
                'b632e1b778ef166f88e6ea206a30bd807b7357e210c29268bc98f39187310faf';
            old_fragment :=
                '99dfc39ef03194161fe67419d87fd2890145980f3147151864ea7552bec36886';
            new_fragment :=
                '3351e2c6a22ce696135c6a9c5d77de9fdb533b86413d6939f0e81af47327c919';
        ELSE
            expected_before_digest :=
                '918e4be248c37e622b1f5b22cb9e252a450b65b295157681c647855d0c0150b9';
            expected_after_digest :=
                '1d7bb5b18129f99ef87b5ad0dfe712b4e6beac33a0461218fedf67fa6990ac3b';
            old_fragment :=
                '99dfc39ef03194161fe67419d87fd2890145980f3147151864ea7552bec36886';
            new_fragment :=
                '3351e2c6a22ce696135c6a9c5d77de9fdb533b86413d6939f0e81af47327c919';
        END IF;

        function_oid := pg_catalog.to_regprocedure(function_identity);

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
            ) IS DISTINCT FROM expected_before_digest
            OR pg_catalog.char_length(definition)
                - pg_catalog.char_length(pg_catalog.replace(
                    definition,
                    old_fragment,
                    ''
                )) <> pg_catalog.char_length(old_fragment)
            OR pg_catalog.strpos(definition, new_fragment) <> 0
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'product_apply_consume_execution_contract_patch_drift';
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
            OR observed_definition_digest IS DISTINCT FROM expected_after_digest
            OR pg_catalog.strpos(
                pg_catalog.pg_get_functiondef(function_oid),
                old_fragment
            ) <> 0
            OR pg_catalog.char_length(
                pg_catalog.pg_get_functiondef(function_oid)
            ) - pg_catalog.char_length(pg_catalog.replace(
                pg_catalog.pg_get_functiondef(function_oid),
                new_fragment,
                ''
            )) <> pg_catalog.char_length(new_fragment)
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'product_apply_consume_execution_contract_postcondition_drift';
        END IF;
    END LOOP;

    IF public.starring_runtime_execution_schema_manifest_v1()
        IS DISTINCT FROM TRUE
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'product_apply_consume_execution_manifest_invalid';
    END IF;
END;
$patch_execution_contract$;

DO $postflight$
DECLARE
    authority_projection_digest TEXT;
    consume_authority_projection_digest TEXT;
    consume_authority_projection_valid BOOLEAN;
    consume_digest TEXT;
    supersession_digest TEXT;
    execution_manifest_digest TEXT;
    execution_readiness_digest TEXT;
    exact_manifest_digest TEXT;
    exact_readiness_digest TEXT;
    serving_manifest_digest TEXT;
    serving_readiness_digest TEXT;
BEGIN
    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_product_apply_authority_projection_v1(text,text,text,text,bytea,text,text,text,text,bigint,text,timestamp with time zone,timestamp with time zone,text,boolean,text)'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO authority_projection_digest;

    SELECT
        pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(function_row.oid),
                'UTF8'
            )),
            'hex'
        ),
        namespace.nspowner = function_row.proowner
            AND function_row.proowner = pg_catalog.to_regrole(current_user)
            AND language_row.lanname = 'plpgsql'
            AND function_row.prokind = 'f'
            AND function_row.provolatile = 'v'
            AND function_row.proisstrict
            AND function_row.prosecdef
            AND function_row.proparallel = 'u'
            AND NOT function_row.proretset
            AND NOT function_row.proleakproof
            AND function_row.pronargs = 17
            AND function_row.pronargdefaults = 0
            AND function_row.provariadic = 0
            AND function_row.prorettype = 'jsonb'::REGTYPE
            AND function_row.proconfig =
                ARRAY['search_path=pg_catalog']::TEXT[]
            AND NOT EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE privilege.grantee <> function_row.proowner
            )
    INTO consume_authority_projection_digest,
        consume_authority_projection_valid
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    INNER JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'starring_runtime_private_v2.starring_product_apply_authority_projection_at_v2(text,text,text,text,bytea,text,text,text,text,bigint,text,timestamp with time zone,timestamp with time zone,text,boolean,text,timestamp with time zone)'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'starring_runtime_private_v2.starring_product_apply_consume_lock_core_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text)'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO consume_digest;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'starring_runtime_private_v2.starring_runtime_product_drain_source_supersession_exact_v2(public.runtime_deployments,jsonb,public.runtime_drain_intents_v2,jsonb,timestamp with time zone)'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO supersession_digest;

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
    INTO execution_manifest_digest;

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
    INTO execution_readiness_digest;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_exact_target_schema_manifest_v1()'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO exact_manifest_digest;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_exact_target_database_readiness_v1()'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO exact_readiness_digest;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_serving_schema_manifest_v1()'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO serving_manifest_digest;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_serving_database_readiness_v1()'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO serving_readiness_digest;

    IF authority_projection_digest
            IS DISTINCT FROM 'dbbd17de881221f5c3c12f26250709045efe388457d8e29b7fd83b751bfca68a'
        OR consume_authority_projection_digest
            IS DISTINCT FROM 'b1f67099817245ed9d62abacb980f3b4014607b6b3e592bfbe8337d0aee486ab'
        OR consume_authority_projection_valid IS DISTINCT FROM TRUE
        OR consume_digest
            IS DISTINCT FROM 'c05a220d2b9b4d255a27b5826173c9574225a70377508e5bbf5fb788dadcd62c'
        OR supersession_digest
            IS DISTINCT FROM '683eef3f28edca886edca556d2cffc61cc2457f57bbefaec2e6e4b58c54b8b34'
        OR execution_manifest_digest
            IS DISTINCT FROM '3351e2c6a22ce696135c6a9c5d77de9fdb533b86413d6939f0e81af47327c919'
        OR execution_readiness_digest
            IS DISTINCT FROM 'b632e1b778ef166f88e6ea206a30bd807b7357e210c29268bc98f39187310faf'
        OR exact_manifest_digest
            IS DISTINCT FROM 'c8e5559234a54c8b4b3be342a98badc0f63d3fb4ae59beea50d105938730ec7d'
        OR exact_readiness_digest
            IS DISTINCT FROM '35903afa3bb9bebe712559a80a503823f4eeedf0d15ebd3d24ce3dbf706b5c14'
        OR serving_manifest_digest
            IS DISTINCT FROM '90ab51452bf5c3ba8074e0bce0f6a643ba374e79497962d0bf2d5aeec062fa96'
        OR serving_readiness_digest
            IS DISTINCT FROM '1d7bb5b18129f99ef87b5ad0dfe712b4e6beac33a0461218fedf67fa6990ac3b'
        OR public.starring_runtime_execution_schema_manifest_v1()
            IS DISTINCT FROM TRUE
        OR public.starring_runtime_exact_target_schema_manifest_v1()
            IS DISTINCT FROM TRUE
        OR public.starring_runtime_exact_target_schema_manifest_v2()
            IS DISTINCT FROM TRUE
        OR public.starring_runtime_serving_schema_manifest_v1()
            IS DISTINCT FROM TRUE
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'product_apply_consume_commit_contract_postflight_drift';
    END IF;
END;
$postflight$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
