SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

LOCK TABLE public.automation_instances IN ACCESS EXCLUSIVE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    collision_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.automation_instances')
        AND relation.relkind = 'r'
        AND relation.relpersistence = 'p'
        AND NOT relation.relrowsecurity
        AND NOT relation.relforcerowsecurity;

    IF NOT FOUND
        OR common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_database_readiness_v1()'
        ) IS NULL
        OR pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_instance_list_retryable_v1(text,bigint)'
        ) IS NULL
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
    THEN
        RAISE EXCEPTION 'runtime interaction teardown retry scan preflight failed'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname
            = 'starring_runtime_interaction_instance_scan_retryable_v2';

    IF collision_count <> 0
        OR pg_catalog.to_regclass(
            'public.automation_instances_deleting_retry_scan_v2_idx'
        ) IS NOT NULL
    THEN
        RAISE EXCEPTION 'runtime interaction teardown retry scan collision exists'
            USING ERRCODE = '55000';
    END IF;
END;
$preflight$;

CREATE INDEX automation_instances_deleting_retry_scan_v2_idx
ON public.automation_instances
USING btree (
    guild_id COLLATE "C",
    instance_id COLLATE "C"
)
WHERE status = 'deleting';

CREATE FUNCTION public.starring_runtime_interaction_instance_scan_retryable_v2(
    expected_after_guild_id TEXT,
    expected_after_instance_id TEXT,
    expected_through_guild_id TEXT,
    expected_through_instance_id TEXT,
    expected_limit BIGINT
)
RETURNS TABLE(
    guild_id TEXT,
    instance_id TEXT,
    through_guild_id TEXT,
    through_instance_id TEXT
)
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 256
AS $function$
DECLARE
    cycle_through_guild_id TEXT;
    cycle_through_instance_id TEXT;
BEGIN
    IF expected_limit NOT BETWEEN 1 AND 256
        OR (
            (expected_after_guild_id = '') IS DISTINCT FROM
                (expected_after_instance_id = '')
        )
        OR (
            (expected_through_guild_id = '') IS DISTINCT FROM
                (expected_through_instance_id = '')
        )
        OR (
            expected_after_guild_id <> ''
            AND (
                expected_after_guild_id !~ '^[1-9][0-9]{0,19}$'
                OR pg_catalog.length(expected_after_guild_id) > 20
                OR (
                    pg_catalog.length(expected_after_guild_id) = 20
                    AND expected_after_guild_id > '18446744073709551615'
                )
                OR expected_after_instance_id !~ '^[A-Za-z0-9_-]{1,32}$'
            )
        )
        OR (
            expected_through_guild_id <> ''
            AND (
                expected_through_guild_id !~ '^[1-9][0-9]{0,19}$'
                OR pg_catalog.length(expected_through_guild_id) > 20
                OR (
                    pg_catalog.length(expected_through_guild_id) = 20
                    AND expected_through_guild_id > '18446744073709551615'
                )
                OR expected_through_instance_id !~ '^[A-Za-z0-9_-]{1,32}$'
            )
        )
        OR (
            expected_through_guild_id = ''
            AND expected_after_guild_id <> ''
        )
        OR (
            expected_after_guild_id <> ''
            AND ROW(
                expected_after_guild_id COLLATE "C",
                expected_after_instance_id COLLATE "C"
            ) >= ROW(
                expected_through_guild_id COLLATE "C",
                expected_through_instance_id COLLATE "C"
            )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_invalid_teardown_retry_scan_input';
    END IF;

    IF expected_through_guild_id = '' THEN
        SELECT instance.guild_id, instance.instance_id
        INTO cycle_through_guild_id, cycle_through_instance_id
        FROM public.automation_instances AS instance
        WHERE instance.status = 'deleting'
        ORDER BY
            instance.guild_id COLLATE "C" DESC,
            instance.instance_id COLLATE "C" DESC
        LIMIT 1;

        IF NOT FOUND THEN
            RETURN;
        END IF;
    ELSE
        cycle_through_guild_id := expected_through_guild_id;
        cycle_through_instance_id := expected_through_instance_id;
    END IF;

    RETURN QUERY
    SELECT
        instance.guild_id,
        instance.instance_id,
        cycle_through_guild_id,
        cycle_through_instance_id
    FROM public.automation_instances AS instance
    WHERE instance.status = 'deleting'
        AND (
            expected_after_guild_id = ''
            OR ROW(
                instance.guild_id COLLATE "C",
                instance.instance_id COLLATE "C"
            ) > ROW(
                expected_after_guild_id COLLATE "C",
                expected_after_instance_id COLLATE "C"
            )
        )
        AND ROW(
            instance.guild_id COLLATE "C",
            instance.instance_id COLLATE "C"
        ) <= ROW(
            cycle_through_guild_id COLLATE "C",
            cycle_through_instance_id COLLATE "C"
        )
    ORDER BY
        instance.guild_id COLLATE "C",
        instance.instance_id COLLATE "C"
    LIMIT expected_limit;
END;
$function$;

DO $manifest_extension$
DECLARE
    function_definition TEXT;
    index_branch TEXT;
    index_branch_replacement TEXT;
    return_contract TEXT;
    return_contract_replacement TEXT;
BEGIN
    function_definition := pg_catalog.pg_get_functiondef(
        pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_schema_manifest_v1()'
        )
    );
    index_branch := $needle$        FROM pg_catalog.pg_index AS index_contract
        INNER JOIN pg_catalog.pg_class AS table_row
            ON table_row.oid = index_contract.indrelid
        INNER JOIN pg_catalog.pg_namespace AS table_namespace
            ON table_namespace.oid = table_row.relnamespace
        INNER JOIN pg_catalog.pg_class AS index_row
            ON index_row.oid = index_contract.indexrelid
        INNER JOIN pg_catalog.pg_namespace AS index_namespace
            ON index_namespace.oid = index_row.relnamespace
        INNER JOIN pg_catalog.pg_am AS index_method
            ON index_method.oid = index_row.relam
        WHERE index_contract.indrelid IN (
            pg_catalog.to_regclass('public.product_control_plane_identity'),
            pg_catalog.to_regclass('public.automation_instances'),
            pg_catalog.to_regclass('public.automation_ruleset_versions')
        )$needle$;
    index_branch_replacement := index_branch
        || E'\n            AND index_row.relname'
        || E'\n                <> ''automation_instances_deleting_retry_scan_v2_idx''';
    return_contract := $needle$    RETURN observed_count = 22
        AND observed_digest
            = '5b4f6fd061991332c8b86244e1a906a07f73b29578336722f7161bd9dac7a61d'$needle$;
    return_contract_replacement := return_contract || $extension$
        AND (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_index AS index_contract
            INNER JOIN pg_catalog.pg_class AS table_row
                ON table_row.oid = index_contract.indrelid
            INNER JOIN pg_catalog.pg_class AS index_row
                ON index_row.oid = index_contract.indexrelid
            INNER JOIN pg_catalog.pg_namespace AS index_namespace
                ON index_namespace.oid = index_row.relnamespace
            INNER JOIN pg_catalog.pg_am AS index_method
                ON index_method.oid = index_row.relam
            WHERE index_row.oid = pg_catalog.to_regclass(
                    'public.automation_instances_deleting_retry_scan_v2_idx'
                )
                AND table_row.oid
                    = pg_catalog.to_regclass('public.automation_instances')
                AND index_namespace.nspname = 'public'
                AND index_row.relname
                    = 'automation_instances_deleting_retry_scan_v2_idx'
                AND index_row.relowner = table_row.relowner
                AND index_row.relkind = 'i'
                AND index_row.relpersistence = 'p'
                AND NOT index_row.relispartition
                AND index_method.amname = 'btree'
                AND NOT index_contract.indisprimary
                AND NOT index_contract.indisunique
                AND index_contract.indisvalid
                AND index_contract.indisready
                AND index_contract.indislive
                AND index_contract.indimmediate
                AND NOT index_contract.indisclustered
                AND NOT index_contract.indisreplident
                AND NOT index_contract.indnullsnotdistinct
                AND index_contract.indnkeyatts = 2
                AND index_contract.indnatts = 2
                AND index_contract.indkey::TEXT = '1 2'
                AND index_contract.indoption::TEXT = '0 0'
                AND index_contract.indexprs IS NULL
                AND pg_catalog.pg_get_expr(
                    index_contract.indpred,
                    index_contract.indrelid
                ) = '(status = ''deleting''::text)'
                AND index_contract.indcollation::TEXT = pg_catalog.format(
                    '%s %s',
                    pg_catalog.to_regcollation('"C"')::OID,
                    pg_catalog.to_regcollation('"C"')::OID
                )
                AND index_contract.indclass::TEXT = pg_catalog.format(
                    '%s %s',
                    (
                        SELECT operator_class.oid
                        FROM pg_catalog.pg_opclass AS operator_class
                        INNER JOIN pg_catalog.pg_am AS operator_method
                            ON operator_method.oid = operator_class.opcmethod
                        WHERE operator_class.opcname = 'text_ops'
                            AND operator_method.amname = 'btree'
                            AND operator_class.opcintype = 'text'::REGTYPE
                            AND operator_class.opcdefault
                    ),
                    (
                        SELECT operator_class.oid
                        FROM pg_catalog.pg_opclass AS operator_class
                        INNER JOIN pg_catalog.pg_am AS operator_method
                            ON operator_method.oid = operator_class.opcmethod
                        WHERE operator_class.opcname = 'text_ops'
                            AND operator_method.amname = 'btree'
                            AND operator_class.opcintype = 'text'::REGTYPE
                            AND operator_class.opcdefault
                    )
                )
        ) = 1$extension$;

    IF function_definition IS NULL
        OR pg_catalog.strpos(function_definition, index_branch) = 0
        OR pg_catalog.strpos(
            pg_catalog.substr(
                function_definition,
                pg_catalog.strpos(function_definition, index_branch)
                    + pg_catalog.length(index_branch)
            ),
            index_branch
        ) <> 0
        OR pg_catalog.strpos(function_definition, return_contract) = 0
    THEN
        RAISE EXCEPTION 'runtime interaction schema manifest extension failed'
            USING ERRCODE = '55000';
    END IF;

    function_definition := pg_catalog.replace(
        function_definition,
        index_branch,
        index_branch_replacement
    );
    function_definition := pg_catalog.replace(
        function_definition,
        return_contract,
        return_contract_replacement
    );
    EXECUTE function_definition;
END;
$manifest_extension$;

DO $readiness_extension$
DECLARE
    function_definition TEXT;
    function_contract TEXT;
    function_contract_replacement TEXT;
    capability_contract TEXT;
    capability_contract_replacement TEXT;
BEGIN
    function_definition := pg_catalog.pg_get_functiondef(
        pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_database_readiness_v1()'
        )
    );
    function_contract := $needle$            (
                'public.starring_runtime_interaction_instance_list_retryable_v1(text,bigint)',
                'expected_guild_id text, expected_limit bigint',
                'TABLE(guild_id text, instance_id text, ruleset_key text, ruleset_version bigint, kind text, created_by text, status text, resources jsonb)',
                TRUE,
                256::REAL,
                'plpgsql'
            )$needle$;
    function_contract_replacement := function_contract || $extension$,
            (
                'public.starring_runtime_interaction_instance_scan_retryable_v2(text,text,text,text,bigint)',
                'expected_after_guild_id text, expected_after_instance_id text, expected_through_guild_id text, expected_through_instance_id text, expected_limit bigint',
                'TABLE(guild_id text, instance_id text, through_guild_id text, through_instance_id text)',
                TRUE,
                256::REAL,
                'plpgsql'
            )$extension$;
    capability_contract := $needle$            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_instance_list_retryable_v1(text,bigint)'
            )$needle$;
    capability_contract_replacement := capability_contract || $extension$,
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_instance_scan_retryable_v2(text,text,text,text,bigint)'
            )$extension$;

    IF function_definition IS NULL
        OR pg_catalog.strpos(function_definition, function_contract) = 0
        OR pg_catalog.strpos(function_definition, capability_contract) = 0
    THEN
        RAISE EXCEPTION 'runtime interaction readiness extension failed'
            USING ERRCODE = '55000';
    END IF;

    function_definition := pg_catalog.replace(
        function_definition,
        function_contract,
        function_contract_replacement
    );
    function_definition := pg_catalog.replace(
        function_definition,
        capability_contract,
        capability_contract_replacement
    );
    EXECUTE function_definition;
END;
$readiness_extension$;

DO $privileges$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    function_identity TEXT;
    grantee OID;
    grantee_name NAME;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.automation_instances');
    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    function_identity :=
        'public.starring_runtime_interaction_instance_scan_retryable_v2(TEXT,TEXT,TEXT,TEXT,BIGINT)';

    IF common_owner_name IS NULL THEN
        RAISE EXCEPTION 'runtime interaction retry scan owner is unavailable'
            USING ERRCODE = '55000';
    END IF;

    EXECUTE pg_catalog.format(
        'ALTER FUNCTION %s OWNER TO %I',
        function_identity,
        common_owner_name
    );
    EXECUTE pg_catalog.format(
        'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE',
        function_identity
    );
    FOR grantee IN
        SELECT DISTINCT privilege.grantee
        FROM pg_catalog.pg_proc AS function_row
        CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
            function_row.proacl,
            pg_catalog.acldefault('f', function_row.proowner)
        )) AS privilege
        WHERE function_row.oid = pg_catalog.to_regprocedure(function_identity)
            AND privilege.grantee <> 0
            AND privilege.grantee <> common_owner
    LOOP
        grantee_name := pg_catalog.pg_get_userbyid(grantee);
        IF grantee_name IS NULL THEN
            RAISE EXCEPTION 'runtime interaction retry scan grantee is unavailable'
                USING ERRCODE = '55000';
        END IF;
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM %I CASCADE',
            function_identity,
            grantee_name
        );
    END LOOP;
END;
$privileges$;

DO $postflight$
DECLARE
    common_owner OID;
    function_row RECORD;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.automation_instances');

    SELECT function_contract.*, language_row.lanname
    INTO function_row
    FROM pg_catalog.pg_proc AS function_contract
    INNER JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_contract.prolang
    WHERE function_contract.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_interaction_instance_scan_retryable_v2(text,text,text,text,bigint)'
    );

    IF NOT FOUND
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR NOT function_row.proisstrict
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR NOT function_row.proretset
        OR function_row.prorows <> 256::REAL
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR function_row.lanname IS DISTINCT FROM 'plpgsql'
        OR pg_catalog.pg_get_function_arguments(function_row.oid)
            IS DISTINCT FROM
                'expected_after_guild_id text, expected_after_instance_id text, expected_through_guild_id text, expected_through_instance_id text, expected_limit bigint'
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM
                'TABLE(guild_id text, instance_id text, through_guild_id text, through_instance_id text)'
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
                OR privilege.grantor <> common_owner
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
        )
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
        OR pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_database_readiness_v1()'
        )) NOT LIKE
            '%starring_runtime_interaction_instance_scan_retryable_v2%'
        OR pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_schema_manifest_v1()'
        )) NOT LIKE
            '%automation_instances_deleting_retry_scan_v2_idx%'
    THEN
        RAISE EXCEPTION 'runtime interaction teardown retry scan postflight failed'
            USING ERRCODE = '55000';
    END IF;
END;
$postflight$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
