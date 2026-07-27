SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
);

LOCK TABLE public.runtime_gateway_owners IN ACCESS EXCLUSIVE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    owner_state_constraint_count BIGINT;
    observation_digest TEXT;
    manifest_digest TEXT;
    readiness_digest TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_gateway_owners'
    );

    SELECT pg_catalog.count(*)
    INTO owner_state_constraint_count
    FROM pg_catalog.pg_constraint AS constraint_row
    WHERE constraint_row.conrelid = pg_catalog.to_regclass(
            'public.runtime_gateway_owners'
        )
        AND constraint_row.conname =
            'runtime_gateway_owners_state_check'
        AND constraint_row.contype = 'c'
        AND constraint_row.convalidated
        AND pg_catalog.pg_get_constraintdef(
            constraint_row.oid,
            TRUE
        ) = 'CHECK (process_instance_id IS NULL AND expected_build_revision IS NULL AND owner_revision IS NULL AND expires_at IS NULL OR process_instance_id IS NOT NULL AND expected_build_revision IS NOT NULL AND owner_revision IS NOT NULL AND expires_at IS NOT NULL)';

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO observation_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO manifest_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_schema_manifest_v1()'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO readiness_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_database_readiness_v1()'
    );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR owner_state_constraint_count <> 1
        OR observation_digest IS DISTINCT FROM
            '9e0179b576eec5edf27cf4b9834c3f570073643518fda5a59fba5489c5fb46c6'
        OR manifest_digest IS DISTINCT FROM
            '94177e2025d87f492e988e3e27b8193b0f7157d4ea7fcd6099308534df9073ff'
        OR readiness_digest IS DISTINCT FROM
            'ae397ea106f18aa71c6cf2427ebf2705638462066e480b6d0f10b9759a8adc5e'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_recovery_owner_projection_preflight_drift';
    END IF;
END;
$preflight$;

DO $patch_observation$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)'
    );

    previous_fragment :=
        '    IF owner_found THEN' || E'\n' ||
        '        observed_process_instance_id := owner_row.process_instance_id;' || E'\n' ||
        '        observed_lease_epoch := owner_row.lease_epoch;' || E'\n' ||
        '        observed_runtime_build_revision :=' || E'\n' ||
        '            owner_row.expected_build_revision;' || E'\n' ||
        '        observed_owner_revision := owner_row.owner_revision;' || E'\n' ||
        '        observed_owner_expires_at := owner_row.expires_at;' || E'\n' ||
        '    END IF;';
    next_fragment :=
        '    IF owner_found' || E'\n' ||
        '        AND owner_row.process_instance_id IS NOT NULL' || E'\n' ||
        '    THEN' || E'\n' ||
        '        observed_process_instance_id := owner_row.process_instance_id;' || E'\n' ||
        '        observed_lease_epoch := owner_row.lease_epoch;' || E'\n' ||
        '        observed_runtime_build_revision :=' || E'\n' ||
        '            owner_row.expected_build_revision;' || E'\n' ||
        '        observed_owner_revision := owner_row.owner_revision;' || E'\n' ||
        '        observed_owner_expires_at := owner_row.expires_at;' || E'\n' ||
        '    END IF;';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_recovery_owner_projection_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
    EXECUTE definition;
END;
$patch_observation$;

DO $patch_schema_manifest$
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
        'RETURN observed_count = 734' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''2b9c978bc17afb7440781c2d5ca50eed37c1ad89986e1f7fe28d2ab5c72fa9b5'';';
    next_fragment :=
        'RETURN observed_count = 734' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''0144d12c7fd78a3f7ad75670e255a1cff2c0ba11cf613f10006cfcbc5528dcc9'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_recovery_owner_projection_manifest_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
    EXECUTE definition;
END;
$patch_schema_manifest$;

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
        '''94177e2025d87f492e988e3e27b8193b0f7157d4ea7fcd6099308534df9073ff''::TEXT';
    next_fragment :=
        '''2e55bd05bb77a1dcc5a4f02efd0b221f2fa085fb92e7da7f97d29408022f0eb3''::TEXT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_recovery_owner_projection_readiness_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
    EXECUTE definition;
END;
$patch_readiness$;

DO $postflight$
DECLARE
    common_owner OID;
    executor_role OID;
    function_row RECORD;
    invalid_acl_count BIGINT;
    owner_state_constraint_count BIGINT;
    observation_digest TEXT;
    manifest_digest TEXT;
    readiness_digest TEXT;
    observation_definition TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_gateway_owners'
    );

    SELECT privilege.grantee
    INTO executor_role
    FROM pg_catalog.pg_proc AS capability
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        capability.proacl,
        pg_catalog.acldefault('f', capability.proowner)
    )) AS privilege
    WHERE capability.oid = pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_database_identity_v1()'
        )
        AND privilege.grantee <> common_owner
    ORDER BY privilege.grantee
    LIMIT 1;

    SELECT
        capability.oid,
        capability.proowner,
        capability.prokind,
        capability.provolatile,
        capability.proisstrict,
        capability.proparallel,
        capability.prosecdef,
        capability.proretset,
        capability.prorows,
        capability.proleakproof,
        capability.pronargdefaults,
        capability.provariadic,
        capability.proconfig
    INTO function_row
    FROM pg_catalog.pg_proc AS capability
    WHERE capability.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)'
    );

    SELECT pg_catalog.count(*)
    INTO invalid_acl_count
    FROM pg_catalog.aclexplode(COALESCE(
        (
            SELECT capability.proacl
            FROM pg_catalog.pg_proc AS capability
            WHERE capability.oid = function_row.oid
        ),
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE privilege.grantee NOT IN (common_owner, executor_role)
        OR privilege.grantor <> common_owner
        OR privilege.privilege_type <> 'EXECUTE'
        OR privilege.is_grantable;

    SELECT pg_catalog.count(*)
    INTO owner_state_constraint_count
    FROM pg_catalog.pg_constraint AS constraint_row
    WHERE constraint_row.conrelid = pg_catalog.to_regclass(
            'public.runtime_gateway_owners'
        )
        AND constraint_row.conname =
            'runtime_gateway_owners_state_check'
        AND constraint_row.contype = 'c'
        AND constraint_row.convalidated
        AND pg_catalog.pg_get_constraintdef(
            constraint_row.oid,
            TRUE
        ) = 'CHECK (process_instance_id IS NULL AND expected_build_revision IS NULL AND owner_revision IS NULL AND expires_at IS NULL OR process_instance_id IS NOT NULL AND expected_build_revision IS NOT NULL AND owner_revision IS NOT NULL AND expires_at IS NOT NULL)';

    SELECT
        pg_catalog.pg_get_functiondef(capability.oid),
        pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(capability.oid),
                'UTF8'
            )),
            'hex'
        )
    INTO observation_definition, observation_digest
    FROM pg_catalog.pg_proc AS capability
    WHERE capability.oid = function_row.oid;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(capability.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO manifest_digest
    FROM pg_catalog.pg_proc AS capability
    WHERE capability.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_schema_manifest_v1()'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(capability.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO readiness_digest
    FROM pg_catalog.pg_proc AS capability
    WHERE capability.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_database_readiness_v1()'
    );

    IF common_owner IS NULL
        OR function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR NOT function_row.proisstrict
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR NOT function_row.proretset
        OR function_row.prorows <> 1::REAL
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR invalid_acl_count <> 0
        OR owner_state_constraint_count <> 1
        OR (
            executor_role IS NOT NULL
            AND NOT pg_catalog.has_function_privilege(
                executor_role,
                function_row.oid,
                'EXECUTE'
            )
        )
        OR observation_definition NOT LIKE
            '%IF owner_found%AND owner_row.process_instance_id IS NOT NULL%THEN%'
        OR observation_digest IS DISTINCT FROM
            '1bafd85ec4d2291c6ab7cf213acaed35fe637409a1ed8679881ee8686956df09'
        OR manifest_digest IS DISTINCT FROM
            '2e55bd05bb77a1dcc5a4f02efd0b221f2fa085fb92e7da7f97d29408022f0eb3'
        OR readiness_digest IS DISTINCT FROM
            '9acd85e2162d4c06593dedae7d2043e53bebc8cd1d70c7aea5aa364cec0cb27f'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_recovery_owner_projection_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
