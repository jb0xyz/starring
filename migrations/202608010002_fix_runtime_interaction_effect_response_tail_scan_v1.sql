SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended(
        'starring-runtime-interaction-effect-schema-v1',
        0
    )
);

LOCK TABLE
    public.runtime_interaction_receipt_heads_v1,
    public.runtime_interaction_effect_heads_v1,
    public._sqlx_migrations
IN ACCESS SHARE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    interaction_role OID;
    function_oid OID;
    applied_count BIGINT;
    applied_head BIGINT;
    failed_count BIGINT;
    migration_checksum TEXT;
    function_digest TEXT;
    manifest_digest TEXT;
    signature_digest TEXT;
    exact_acl_count BIGINT;
    exact_acl_principal_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.automation_instances'
    );
    interaction_role := pg_catalog.to_regrole(
        'starring_runtime_interaction'
    );
    function_oid := pg_catalog.to_regprocedure(
        'public.starring_runtime_interaction_effect_response_tail_scan_v1(timestamp with time zone,text,text,bigint,timestamp with time zone,text,text,bigint,bigint)'
    );

    SELECT pg_catalog.count(*),
        pg_catalog.max(migration.version),
        pg_catalog.count(*) FILTER (WHERE NOT migration.success)
    INTO applied_count, applied_head, failed_count
    FROM public._sqlx_migrations AS migration;

    SELECT pg_catalog.encode(migration.checksum, 'hex')
    INTO migration_checksum
    FROM public._sqlx_migrations AS migration
    WHERE migration.version = 202608010001
        AND migration.success;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_oid),
            'UTF8'
        )),
        'hex'
    )
    INTO function_digest;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_interaction_effect_schema_manifest_v1()'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO manifest_digest;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_function_identity_arguments(function_oid)
                || E'\n'
                || pg_catalog.pg_get_function_result(function_oid),
            'UTF8'
        )),
        'hex'
    )
    INTO signature_digest;

    SELECT pg_catalog.count(*),
        pg_catalog.count(DISTINCT privilege.grantee)
    INTO exact_acl_count, exact_acl_principal_count
    FROM pg_catalog.pg_proc AS function_row
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE function_row.oid = function_oid
        AND privilege.grantor = common_owner
        AND (
            privilege.grantee = common_owner
            OR privilege.grantee = interaction_role
        )
        AND privilege.privilege_type = 'EXECUTE'
        AND NOT privilege.is_grantable;

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR applied_count <> 116
        OR applied_head <> 202608010001
        OR failed_count <> 0
        OR migration_checksum
            <> '69bc36f83f1e6b575205ca67703639124e41abd9d19b1b701d845ca150dcc4d6202608176b0583c864b4663937bd89e7'
        OR function_oid IS NULL
        OR function_digest
            <> 'c4841de684e511174bb2c3186a7ed94e6a826d127c85e9b73b531c75bf726917'
        OR manifest_digest
            <> '2e4143ab4b3feb364f1dd83d9c085ff7e719f85cd7e2b4f4073840393fcdfdd2'
        OR signature_digest
            <> '948198e83dfad27778d0d9b1e254a3c1319e405d8fb147f5eea37cbb9077f00e'
        OR exact_acl_count
            <> 1 + (interaction_role IS NOT NULL)::INTEGER
        OR exact_acl_principal_count
            <> 1 + (interaction_role IS NOT NULL)::INTEGER
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_proc AS function_row
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE function_row.oid = function_oid
        ) <> 1 + (interaction_role IS NOT NULL)::INTEGER
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_proc AS function_row
            INNER JOIN pg_catalog.pg_language AS language_row
                ON language_row.oid = function_row.prolang
            WHERE function_row.oid = function_oid
                AND (
                    function_row.proowner <> common_owner
                    OR function_row.prokind <> 'f'
                    OR function_row.provolatile <> 'v'
                    OR NOT function_row.proisstrict
                    OR NOT function_row.prosecdef
                    OR function_row.proparallel <> 'u'
                    OR NOT function_row.proretset
                    OR function_row.prorows <> 256
                    OR function_row.proconfig
                        <> ARRAY['search_path=pg_catalog']::TEXT[]
                    OR function_row.proleakproof
                    OR function_row.pronargdefaults <> 0
                    OR function_row.provariadic <> 0
                    OR language_row.lanname <> 'plpgsql'
                )
        )
        OR NOT public.starring_runtime_interaction_effect_schema_manifest_v1()
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_effect_response_tail_scan_fix_preflight_drift';
    END IF;
END;
$preflight$;

DO $patch_response_tail_scan$
DECLARE
    function_oid OID;
    definition TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
    observed_definition_digest TEXT;
    old_fragment TEXT := 'pg_catalog.greatest(';
    new_fragment TEXT := 'GREATEST(';
BEGIN
    function_oid := pg_catalog.to_regprocedure(
        'public.starring_runtime_interaction_effect_response_tail_scan_v1(timestamp with time zone,text,text,bigint,timestamp with time zone,text,text,bigint,bigint)'
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
            'return_type', function_row.prorettype::TEXT,
            'all_argument_types',
                pg_catalog.to_jsonb(function_row.proallargtypes),
            'argument_modes', pg_catalog.to_jsonb(function_row.proargmodes),
            'argument_names', pg_catalog.to_jsonb(function_row.proargnames)
        )
    INTO definition, metadata_before
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = function_oid;

    IF definition IS NULL
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(definition, 'UTF8')),
            'hex'
        ) <> 'c4841de684e511174bb2c3186a7ed94e6a826d127c85e9b73b531c75bf726917'
        OR (
            pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                old_fragment,
                ''
            ))
        ) / pg_catalog.char_length(old_fragment) <> 2
        OR pg_catalog.strpos(definition, new_fragment) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_effect_response_tail_scan_fix_definition_drift';
    END IF;

    EXECUTE pg_catalog.replace(
        definition,
        old_fragment,
        new_fragment
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
            'return_type', function_row.prorettype::TEXT,
            'all_argument_types',
                pg_catalog.to_jsonb(function_row.proallargtypes),
            'argument_modes', pg_catalog.to_jsonb(function_row.proargmodes),
            'argument_names', pg_catalog.to_jsonb(function_row.proargnames)
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
            <> '83f98c884ef9bed3706ace2a9430c401760277fb3cc83ea16646d147c819550b'
        OR pg_catalog.strpos(definition, old_fragment) <> 0
        OR (
            pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                new_fragment,
                ''
            ))
        ) / pg_catalog.char_length(new_fragment) <> 2
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_effect_response_tail_scan_fix_postcondition_drift';
    END IF;
END;
$patch_response_tail_scan$;

DO $refresh_effect_manifest$
DECLARE
    function_oid OID;
    definition TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
    observed_definition_digest TEXT;
    old_digest TEXT :=
        'f293f524ef97b491b6781a795888bf879aa0a82f790be699f31e4cee8c054152';
    new_digest TEXT :=
        '8db2428cc05e5f973639d6f8af244722c40ae9183f3b32202ec45af5e22d5215';
BEGIN
    function_oid := pg_catalog.to_regprocedure(
        'public.starring_runtime_interaction_effect_schema_manifest_v1()'
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
        ) <> '2e4143ab4b3feb364f1dd83d9c085ff7e719f85cd7e2b4f4073840393fcdfdd2'
        OR (
            pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                old_digest,
                ''
            ))
        ) <> 64
        OR pg_catalog.strpos(definition, new_digest) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_effect_response_tail_scan_fix_manifest_drift';
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
            <> '4cb3618c886f231ab75cd9224422131aadba925e9abac870416244a596be2e17'
        OR NOT public.starring_runtime_interaction_effect_schema_manifest_v1()
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_effect_response_tail_scan_fix_manifest_postcondition_drift';
    END IF;
END;
$refresh_effect_manifest$;

DO $postflight$
DECLARE
    function_digest TEXT;
    manifest_digest TEXT;
BEGIN
    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_interaction_effect_response_tail_scan_v1(timestamp with time zone,text,text,bigint,timestamp with time zone,text,text,bigint,bigint)'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO function_digest;
    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_interaction_effect_schema_manifest_v1()'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO manifest_digest;

    IF function_digest
            <> '83f98c884ef9bed3706ace2a9430c401760277fb3cc83ea16646d147c819550b'
        OR manifest_digest
            <> '4cb3618c886f231ab75cd9224422131aadba925e9abac870416244a596be2e17'
        OR NOT public.starring_runtime_interaction_effect_schema_manifest_v1()
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_effect_response_tail_scan_fix_postflight_drift';
    END IF;
END;
$postflight$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
