SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
);

LOCK TABLE
    public.runtime_writer_fence,
    public.runtime_slot_writer_fences_v2,
    public.runtime_drain_intents_v2,
    public.runtime_deployments,
    public.runtime_execution_mutation_markers,
    public.runtime_attestations,
    public.runtime_serving_leases
IN ACCESS EXCLUSIVE MODE;

CREATE TEMPORARY TABLE pg_temp.starring_runtime_execution_slot_epoch_snapshot (
    function_oid OID PRIMARY KEY,
    function_owner OID NOT NULL,
    function_acl ACLITEM[]
) ON COMMIT DROP;

INSERT INTO pg_temp.starring_runtime_execution_slot_epoch_snapshot (
    function_oid,
    function_owner,
    function_acl
)
SELECT
    function_row.oid,
    function_row.proowner,
    function_row.proacl
FROM (
    VALUES
        ('public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)'),
        ('public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)'),
        ('public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)'),
        ('public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)'),
        ('public.starring_runtime_execution_schema_manifest_v1()'),
        ('public.starring_runtime_execution_database_readiness_v1()')
) AS expected(identity)
INNER JOIN pg_catalog.pg_proc AS function_row
    ON function_row.oid = pg_catalog.to_regprocedure(expected.identity);

DO $preflight$
DECLARE
    common_owner OID;
    invalid_relation_count BIGINT;
    invalid_function_count BIGINT;
    invalid_acl_count BIGINT;
    invalid_capability_acl_count BIGINT;
    invalid_owner_only_acl_count BIGINT;
    executor_role OID;
    executor_role_is_quarantined BOOLEAN;
    executor_membership_count BIGINT;
    other_client_session_count BIGINT;
    prepared_transaction_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT pg_catalog.count(*)
    INTO invalid_relation_count
    FROM (
        VALUES
            ('public.runtime_writer_fence'),
            ('public.runtime_slot_writer_fences_v2'),
            ('public.runtime_drain_intents_v2'),
            ('public.runtime_deployments'),
            ('public.runtime_execution_mutation_markers'),
            ('public.runtime_attestations'),
            ('public.runtime_serving_leases')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = pg_catalog.to_regclass(expected.identity)
    WHERE relation.oid IS NULL
        OR relation.relkind <> 'r'
        OR relation.relpersistence <> 'p'
        OR relation.relowner <> common_owner
        OR relation.relrowsecurity
        OR relation.relforcerowsecurity;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            ('public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)',
                '5080cfbe425828c2eb5c54bbe475cba5cf02fa0cecc0aab0a72a4ddb7af5d718',
                'plpgsql',
                TRUE, TRUE, 1::REAL, TRUE),
            ('public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)',
                '7b49bc478d98af25cdf7563a05d3e03ecddbb7fd2ed897a7bcb2f053716fe386',
                'plpgsql',
                TRUE, TRUE, 1::REAL, TRUE),
            ('public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)',
                '84ea51ff6db862974303c191d44e41af00685b3384c732b8dbda1ef7a18df08a',
                'plpgsql',
                TRUE, TRUE, 1::REAL, TRUE),
            ('public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)',
                'ddbda720a96466f784b46de120a199d0b20ac3384a39d70c8829d8747532b105',
                'plpgsql',
                TRUE, TRUE, 1::REAL, TRUE),
            ('public.starring_runtime_execution_schema_manifest_v1()',
                '3a014a2c92d5a7da93867f10d8e5d8f9ca1ac5f49666ad57558d49f46b66b2a0',
                'plpgsql',
                TRUE, FALSE, 0::REAL, TRUE),
            ('public.starring_runtime_execution_database_readiness_v1()',
                '17fdc258083036bc6f6faceee4dbd900f166ce15f711e99ea87e60ae03e3aa31',
                'plpgsql',
                TRUE, TRUE, 1::REAL, TRUE),
            ('public.starring_runtime_execution_database_identity_v1()',
                '455bf3c81d3b144ab6c3f9da24d8c5c5a7961612d1d87f66da7b44bb0e0f9961',
                'sql',
                TRUE, FALSE, 0::REAL, TRUE),
            ('public.starring_runtime_writer_fence_observe_v1()',
                'e1d910428439dfa3387988aea6e57d49e3a9a54ba31147f217a19679ed78b5d7',
                'plpgsql',
                TRUE, TRUE, 1::REAL, TRUE),
            ('starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(text,text)',
                '68708aa143de1daae1247b18f3127e2abdc6d269a14e103d24e5ab6732d23f99',
                'plpgsql',
                FALSE, TRUE, 1::REAL, TRUE),
            ('starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(text,text,bigint)',
                'da6c88ff80cf366e14f2c12a6204964d708156192a292cc6ad71b959588f07b8',
                'plpgsql',
                FALSE, FALSE, 0::REAL, TRUE)
    ) AS expected(
        identity,
        definition_digest,
        language_name,
        security_definer,
        returns_set,
        rows_estimate,
        is_strict
    )
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR function_row.proisstrict <> expected.is_strict
        OR function_row.proparallel <> 'u'
        OR function_row.prosecdef <> expected.security_definer
        OR function_row.proretset <> expected.returns_set
        OR function_row.prorows <> expected.rows_estimate
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM expected.language_name
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(function_row.oid),
                'UTF8'
            )),
            'hex'
        ) IS DISTINCT FROM expected.definition_digest
        OR (
            NOT expected.security_definer
            AND EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE privilege.grantee <> common_owner
            )
        );

    SELECT pg_catalog.count(*)
    INTO invalid_acl_count
    FROM pg_temp.starring_runtime_execution_slot_epoch_snapshot AS snapshot
    INNER JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = snapshot.function_oid
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE privilege.grantee <> common_owner
        AND (
            privilege.grantee = 0
            OR privilege.grantor <> common_owner
            OR privilege.privilege_type <> 'EXECUTE'
            OR privilege.is_grantable
        );

    WITH baseline AS (
        SELECT
            pg_catalog.count(*) FILTER (
                WHERE privilege.grantee <> common_owner
            ) AS external_count,
            COALESCE(pg_catalog.string_agg(
                pg_catalog.concat_ws(
                    ':',
                    privilege.grantee::TEXT,
                    privilege.privilege_type,
                    privilege.is_grantable::TEXT,
                    privilege.grantor::TEXT
                ),
                ',' ORDER BY privilege.grantee, privilege.privilege_type
            ) FILTER (WHERE privilege.grantee <> common_owner), '') AS external_acl,
            pg_catalog.bool_or(
                privilege.grantee <> common_owner
                AND (
                    privilege.grantee = 0
                    OR privilege.privilege_type <> 'EXECUTE'
                    OR privilege.is_grantable
                    OR privilege.grantor <> common_owner
                )
            ) AS invalid
        FROM pg_catalog.pg_proc AS function_row
        LEFT JOIN LATERAL pg_catalog.aclexplode(COALESCE(
            function_row.proacl,
            pg_catalog.acldefault('f', function_row.proowner)
        )) AS privilege ON TRUE
        WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_database_identity_v1()'
        )
    ), observed AS (
        SELECT expected.identity,
            function_row.oid,
            COALESCE(pg_catalog.string_agg(
                pg_catalog.concat_ws(
                    ':',
                    privilege.grantee::TEXT,
                    privilege.privilege_type,
                    privilege.is_grantable::TEXT,
                    privilege.grantor::TEXT
                ),
                ',' ORDER BY privilege.grantee, privilege.privilege_type
            ) FILTER (WHERE privilege.grantee <> common_owner), '') AS external_acl
        FROM (
            VALUES
                ('public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)'),
                ('public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)'),
                ('public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)'),
                ('public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)'),
                ('public.starring_runtime_execution_database_readiness_v1()')
        ) AS expected(identity)
        LEFT JOIN pg_catalog.pg_proc AS function_row
            ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
        LEFT JOIN LATERAL pg_catalog.aclexplode(COALESCE(
            function_row.proacl,
            pg_catalog.acldefault('f', function_row.proowner)
        )) AS privilege ON TRUE
        GROUP BY expected.identity, function_row.oid
    )
    SELECT pg_catalog.count(*)
    INTO invalid_capability_acl_count
    FROM observed
    CROSS JOIN baseline
    WHERE observed.oid IS NULL
        OR baseline.external_count > 1
        OR baseline.invalid
        OR observed.external_acl IS DISTINCT FROM baseline.external_acl;

    SELECT pg_catalog.count(*)
    INTO invalid_owner_only_acl_count
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_schema_manifest_v1()'
        )
        AND EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
        );

    SELECT privilege.grantee
    INTO executor_role
    FROM pg_catalog.pg_proc AS function_row
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_database_identity_v1()'
        )
        AND privilege.grantee <> common_owner
    ORDER BY privilege.grantee
    LIMIT 1;

    SELECT COALESCE(NOT role.rolcanlogin, TRUE)
    INTO executor_role_is_quarantined
    FROM pg_catalog.pg_roles AS role
    WHERE role.oid = executor_role;
    executor_role_is_quarantined := COALESCE(
        executor_role_is_quarantined,
        executor_role IS NULL
    );

    SELECT pg_catalog.count(*)
    INTO executor_membership_count
    FROM pg_catalog.pg_auth_members AS membership
    WHERE membership.roleid = executor_role;

    SELECT pg_catalog.count(*)
    INTO other_client_session_count
    FROM pg_catalog.pg_stat_activity AS activity
    WHERE activity.datid = (
            SELECT database_row.oid
            FROM pg_catalog.pg_database AS database_row
            WHERE database_row.datname = pg_catalog.current_database()
        )
        AND activity.pid <> pg_catalog.pg_backend_pid()
        AND activity.backend_type = 'client backend';

    SELECT pg_catalog.count(*)
    INTO prepared_transaction_count
    FROM pg_catalog.pg_prepared_xacts AS prepared
    WHERE prepared.database = pg_catalog.current_database();

    IF NOT executor_role_is_quarantined
        OR executor_membership_count <> 0
        OR other_client_session_count <> 0
        OR prepared_transaction_count <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_slot_writer_epoch_executor_not_quiesced';
    END IF;

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR invalid_relation_count <> 0
        OR invalid_function_count <> 0
        OR invalid_acl_count <> 0
        OR invalid_capability_acl_count <> 0
        OR invalid_owner_only_acl_count <> 0
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_temp.starring_runtime_execution_slot_epoch_snapshot
        ) <> 6
        OR (SELECT pg_catalog.count(*) FROM public.runtime_writer_fence) <> 1
        OR NOT EXISTS (
            SELECT 1
            FROM public.runtime_writer_fence AS fence
            WHERE fence.singleton
                AND fence.fence_state IN ('open', 'closed')
        )
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v1()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_slot_writer_epoch_preflight_drift';
    END IF;
END;
$preflight$;

DO $patch_renew$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)'
    );

    previous_fragment :=
        '    next_expiry TIMESTAMPTZ;' || E'\n' ||
        'BEGIN';
    next_fragment :=
        '    next_expiry TIMESTAMPTZ;' || E'\n' ||
        '    writer_fence_state TEXT;' || E'\n' ||
        '    candidate_guild_id TEXT;' || E'\n' ||
        '    candidate_ruleset_key TEXT;' || E'\n' ||
        '    slot_writer_epoch BIGINT;' || E'\n' ||
        'BEGIN';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_slot_writer_epoch_renew_declaration_drift';
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
        '    WHERE deployment.tenant_id = expected_tenant_id' || E'\n' ||
        '        AND deployment.installation_id = expected_installation_id' || E'\n' ||
        '        AND deployment.deployment_id = expected_deployment_id' || E'\n' ||
        '    FOR UPDATE;' || E'\n' ||
        '' || E'\n' ||
        '    IF NOT FOUND THEN' || E'\n' ||
        '        RAISE EXCEPTION USING' || E'\n' ||
        '            ERRCODE = ''RX001'',' || E'\n' ||
        '            MESSAGE = ''runtime_execution_renew_ownership_lost'';' || E'\n' ||
        '    END IF;';
    next_fragment :=
        '    SELECT fence.fence_state' || E'\n' ||
        '    INTO writer_fence_state' || E'\n' ||
        '    FROM public.starring_runtime_writer_fence_observe_v1() AS fence;' || E'\n' ||
        '' || E'\n' ||
        '    IF NOT FOUND' || E'\n' ||
        '        OR writer_fence_state NOT IN (''open'', ''closed'')' || E'\n' ||
        '    THEN' || E'\n' ||
        '        RAISE EXCEPTION USING' || E'\n' ||
        '            ERRCODE = ''RX004'',' || E'\n' ||
        '            MESSAGE = ''runtime_execution_writer_fence_invalid'';' || E'\n' ||
        '    END IF;' || E'\n' ||
        '' || E'\n' ||
        '    IF writer_fence_state = ''closed'' THEN' || E'\n' ||
        '        RAISE EXCEPTION USING' || E'\n' ||
        '            ERRCODE = ''RX005'',' || E'\n' ||
        '            MESSAGE = ''runtime_execution_writer_fenced'';' || E'\n' ||
        '    END IF;' || E'\n' ||
        '' || E'\n' ||
        '    SELECT deployment.guild_id, deployment.ruleset_key' || E'\n' ||
        '    INTO candidate_guild_id, candidate_ruleset_key' || E'\n' ||
        '    FROM public.runtime_deployments AS deployment' || E'\n' ||
        '    WHERE deployment.tenant_id = expected_tenant_id' || E'\n' ||
        '        AND deployment.installation_id = expected_installation_id' || E'\n' ||
        '        AND deployment.deployment_id = expected_deployment_id;' || E'\n' ||
        '' || E'\n' ||
        '    IF NOT FOUND' || E'\n' ||
        '        OR candidate_guild_id !~ ''^[1-9][0-9]{0,19}$''' || E'\n' ||
        '        OR pg_catalog.length(candidate_guild_id) > 20' || E'\n' ||
        '        OR (' || E'\n' ||
        '            pg_catalog.length(candidate_guild_id) = 20' || E'\n' ||
        '            AND candidate_guild_id COLLATE pg_catalog."C"' || E'\n' ||
        '                > ''18446744073709551615'' COLLATE pg_catalog."C"' || E'\n' ||
        '        )' || E'\n' ||
        '        OR candidate_ruleset_key !~ ''^[A-Za-z0-9_-]{1,64}$''' || E'\n' ||
        '    THEN' || E'\n' ||
        '        RAISE EXCEPTION USING' || E'\n' ||
        '            ERRCODE = ''RX001'',' || E'\n' ||
        '            MESSAGE = ''runtime_execution_renew_ownership_lost'';' || E'\n' ||
        '    END IF;' || E'\n' ||
        '' || E'\n' ||
        '    PERFORM pg_catalog.pg_advisory_xact_lock(' || E'\n' ||
        '        pg_catalog.hashtextextended(' || E'\n' ||
        '            pg_catalog.concat(' || E'\n' ||
        '                ''starring-runtime-serving-slot-v1:'',' || E'\n' ||
        '                candidate_guild_id,' || E'\n' ||
        '                '':'',' || E'\n' ||
        '                candidate_ruleset_key' || E'\n' ||
        '            ),' || E'\n' ||
        '            0' || E'\n' ||
        '        )' || E'\n' ||
        '    );' || E'\n' ||
        '' || E'\n' ||
        '    SELECT slot_fence.writer_epoch' || E'\n' ||
        '    INTO slot_writer_epoch' || E'\n' ||
        '    FROM starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(' || E'\n' ||
        '        candidate_guild_id,' || E'\n' ||
        '        candidate_ruleset_key' || E'\n' ||
        '    ) AS slot_fence;' || E'\n' ||
        '' || E'\n' ||
        '    SELECT deployment.*' || E'\n' ||
        '    INTO deployment_row' || E'\n' ||
        '    FROM public.runtime_deployments AS deployment' || E'\n' ||
        '    WHERE deployment.tenant_id = expected_tenant_id' || E'\n' ||
        '        AND deployment.installation_id = expected_installation_id' || E'\n' ||
        '        AND deployment.deployment_id = expected_deployment_id' || E'\n' ||
        '    FOR UPDATE;' || E'\n' ||
        '' || E'\n' ||
        '    IF NOT FOUND' || E'\n' ||
        '        OR deployment_row.guild_id IS DISTINCT FROM candidate_guild_id' || E'\n' ||
        '        OR deployment_row.ruleset_key IS DISTINCT FROM candidate_ruleset_key' || E'\n' ||
        '    THEN' || E'\n' ||
        '        RAISE EXCEPTION USING' || E'\n' ||
        '            ERRCODE = ''RX001'',' || E'\n' ||
        '            MESSAGE = ''runtime_execution_renew_ownership_lost'';' || E'\n' ||
        '    END IF;';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_slot_writer_epoch_renew_lock_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    UPDATE public.runtime_deployments AS deployment';
    next_fragment :=
        '    PERFORM starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(' || E'\n' ||
        '        candidate_guild_id,' || E'\n' ||
        '        candidate_ruleset_key,' || E'\n' ||
        '        slot_writer_epoch' || E'\n' ||
        '    );' || E'\n' ||
        '' || E'\n' ||
        '    UPDATE public.runtime_deployments AS deployment';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_slot_writer_epoch_renew_mutation_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    EXECUTE definition;
END;
$patch_renew$;

DO $patch_mutate$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)'
    );

    previous_fragment :=
        '    candidate_ruleset_key TEXT;' || E'\n' ||
        '    reason_trim_characters CONSTANT TEXT :=';
    next_fragment :=
        '    candidate_ruleset_key TEXT;' || E'\n' ||
        '    slot_writer_epoch BIGINT;' || E'\n' ||
        '    reason_trim_characters CONSTANT TEXT :=';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_slot_writer_epoch_mutate_declaration_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    IF mutation_kind = ''accept_drain'' THEN' || E'\n' ||
        '        SELECT fence.fence_state';
    next_fragment :=
        '    SELECT fence.fence_state';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_slot_writer_epoch_mutate_global_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '        PERFORM pg_catalog.pg_advisory_xact_lock(' || E'\n' ||
        '            pg_catalog.hashtextextended(' || E'\n' ||
        '                pg_catalog.concat(' || E'\n' ||
        '                    ''starring-runtime-serving-slot-v1:'',' || E'\n' ||
        '                    candidate_guild_id,' || E'\n' ||
        '                    '':'',' || E'\n' ||
        '                    candidate_ruleset_key' || E'\n' ||
        '                ),' || E'\n' ||
        '                0' || E'\n' ||
        '            )' || E'\n' ||
        '        );' || E'\n' ||
        '    END IF;' || E'\n' ||
        '' || E'\n' ||
        '    SELECT deployment.*' || E'\n' ||
        '    INTO deployment_row' || E'\n' ||
        '    FROM public.runtime_deployments AS deployment' || E'\n' ||
        '    WHERE deployment.tenant_id = expected_tenant_id' || E'\n' ||
        '        AND deployment.installation_id = expected_installation_id' || E'\n' ||
        '        AND deployment.deployment_id = expected_deployment_id' || E'\n' ||
        '    FOR UPDATE;' || E'\n' ||
        '' || E'\n' ||
        '    IF NOT FOUND' || E'\n' ||
        '        OR (' || E'\n' ||
        '            mutation_kind = ''accept_drain''' || E'\n' ||
        '            AND (' || E'\n' ||
        '                deployment_row.guild_id IS DISTINCT FROM candidate_guild_id' || E'\n' ||
        '                OR deployment_row.ruleset_key' || E'\n' ||
        '                    IS DISTINCT FROM candidate_ruleset_key' || E'\n' ||
        '            )' || E'\n' ||
        '        )' || E'\n' ||
        '    THEN';
    next_fragment :=
        '        PERFORM pg_catalog.pg_advisory_xact_lock(' || E'\n' ||
        '            pg_catalog.hashtextextended(' || E'\n' ||
        '                pg_catalog.concat(' || E'\n' ||
        '                    ''starring-runtime-serving-slot-v1:'',' || E'\n' ||
        '                    candidate_guild_id,' || E'\n' ||
        '                    '':'',' || E'\n' ||
        '                    candidate_ruleset_key' || E'\n' ||
        '                ),' || E'\n' ||
        '                0' || E'\n' ||
        '            )' || E'\n' ||
        '        );' || E'\n' ||
        '' || E'\n' ||
        '    SELECT slot_fence.writer_epoch' || E'\n' ||
        '    INTO slot_writer_epoch' || E'\n' ||
        '    FROM starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(' || E'\n' ||
        '        candidate_guild_id,' || E'\n' ||
        '        candidate_ruleset_key' || E'\n' ||
        '    ) AS slot_fence;' || E'\n' ||
        '' || E'\n' ||
        '    SELECT deployment.*' || E'\n' ||
        '    INTO deployment_row' || E'\n' ||
        '    FROM public.runtime_deployments AS deployment' || E'\n' ||
        '    WHERE deployment.tenant_id = expected_tenant_id' || E'\n' ||
        '        AND deployment.installation_id = expected_installation_id' || E'\n' ||
        '        AND deployment.deployment_id = expected_deployment_id' || E'\n' ||
        '    FOR UPDATE;' || E'\n' ||
        '' || E'\n' ||
        '    IF NOT FOUND' || E'\n' ||
        '        OR deployment_row.guild_id IS DISTINCT FROM candidate_guild_id' || E'\n' ||
        '        OR deployment_row.ruleset_key IS DISTINCT FROM candidate_ruleset_key' || E'\n' ||
        '    THEN';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_slot_writer_epoch_mutate_lock_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    UPDATE public.runtime_deployments AS deployment';
    next_fragment :=
        '    PERFORM starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(' || E'\n' ||
        '        candidate_guild_id,' || E'\n' ||
        '        candidate_ruleset_key,' || E'\n' ||
        '        slot_writer_epoch' || E'\n' ||
        '    );' || E'\n' ||
        '' || E'\n' ||
        '    UPDATE public.runtime_deployments AS deployment';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_slot_writer_epoch_mutate_mutation_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    EXECUTE definition;
END;
$patch_mutate$;

DO $patch_certify_prepare$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)'
    );

    previous_fragment :=
        '                candidate_ruleset_key' || E'\n' ||
        '            ),' || E'\n' ||
        '            0' || E'\n' ||
        '        )' || E'\n' ||
        '    );' || E'\n' ||
        '' || E'\n' ||
        '    SELECT deployment.*';
    next_fragment :=
        '                candidate_ruleset_key' || E'\n' ||
        '            ),' || E'\n' ||
        '            0' || E'\n' ||
        '        )' || E'\n' ||
        '    );' || E'\n' ||
        '' || E'\n' ||
        '    PERFORM slot_fence.writer_epoch' || E'\n' ||
        '    FROM starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(' || E'\n' ||
        '        candidate_guild_id,' || E'\n' ||
        '        candidate_ruleset_key' || E'\n' ||
        '    ) AS slot_fence;' || E'\n' ||
        '' || E'\n' ||
        '    SELECT deployment.*';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_slot_writer_epoch_certify_prepare_lock_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    EXECUTE definition;
END;
$patch_certify_prepare$;

DO $patch_certify_commit$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)'
    );

    previous_fragment :=
        '    candidate_ruleset_key TEXT;' || E'\n' ||
        '    existing_attestation public.runtime_attestations%ROWTYPE;';
    next_fragment :=
        '    candidate_ruleset_key TEXT;' || E'\n' ||
        '    slot_writer_epoch BIGINT;' || E'\n' ||
        '    existing_attestation public.runtime_attestations%ROWTYPE;';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_slot_writer_epoch_certify_commit_declaration_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '                candidate_ruleset_key' || E'\n' ||
        '            ),' || E'\n' ||
        '            0' || E'\n' ||
        '        )' || E'\n' ||
        '    );' || E'\n' ||
        '' || E'\n' ||
        '    SELECT deployment.*';
    next_fragment :=
        '                candidate_ruleset_key' || E'\n' ||
        '            ),' || E'\n' ||
        '            0' || E'\n' ||
        '        )' || E'\n' ||
        '    );' || E'\n' ||
        '' || E'\n' ||
        '    SELECT slot_fence.writer_epoch' || E'\n' ||
        '    INTO slot_writer_epoch' || E'\n' ||
        '    FROM starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(' || E'\n' ||
        '        candidate_guild_id,' || E'\n' ||
        '        candidate_ruleset_key' || E'\n' ||
        '    ) AS slot_fence;' || E'\n' ||
        '' || E'\n' ||
        '    SELECT deployment.*';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_slot_writer_epoch_certify_commit_lock_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    INSERT INTO public.runtime_attestations (';
    next_fragment :=
        '    PERFORM starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(' || E'\n' ||
        '        candidate_guild_id,' || E'\n' ||
        '        candidate_ruleset_key,' || E'\n' ||
        '        slot_writer_epoch' || E'\n' ||
        '    );' || E'\n' ||
        '' || E'\n' ||
        '    INSERT INTO public.runtime_attestations (';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_slot_writer_epoch_certify_commit_mutation_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    EXECUTE definition;
END;
$patch_certify_commit$;

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
        '            = ''68588695f6c82923f7830faa333d16533f86b43f3f47bf69756bd7447c1aae91'';';
    next_fragment :=
        '    RETURN observed_count = 623' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''00e12af28c93ce77f62c4e1335aa3de88431bb22096bd85b86038dd555dccd13'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_slot_writer_epoch_manifest_drift';
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
        '''3a014a2c92d5a7da93867f10d8e5d8f9ca1ac5f49666ad57558d49f46b66b2a0''::TEXT';
    next_fragment :=
        '''0d0adb92217032ac62b996a0b3e6cb3cdb3ff99a0be983626aa5df4777c78bb7''::TEXT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_slot_writer_epoch_readiness_drift';
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
    common_owner OID;
    invalid_function_count BIGINT;
    snapshot_mismatch_count BIGINT;
    renew_source TEXT;
    mutate_source TEXT;
    prepare_source TEXT;
    commit_source TEXT;
    source_value TEXT;
    contract_row RECORD;
    global_position INTEGER;
    slot_position INTEGER;
    physical_position INTEGER;
    deployment_position INTEGER;
    replay_position INTEGER;
    begin_position INTEGER;
    mutation_position INTEGER;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            ('public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)',
                '3c418773f843f2bf8827464624b4fd3124d8979c4c15b4323a96e76676c11c4e',
                TRUE, 1::REAL),
            ('public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)',
                '76d965851a753501722854c0aecc22d51a3eaa92e93d55a299bfd59d5d922559',
                TRUE, 1::REAL),
            ('public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)',
                '0b9fdc77ec2d85ea2513d6edf462ddddfe3304c4c1f53bec0432b5e0180e6967',
                TRUE, 1::REAL),
            ('public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)',
                '5f0fa2982466bf5ca30250d2334c8314fa0c510ae586f6b750cdd1b662655fe1',
                TRUE, 1::REAL),
            ('public.starring_runtime_execution_schema_manifest_v1()',
                '0d0adb92217032ac62b996a0b3e6cb3cdb3ff99a0be983626aa5df4777c78bb7',
                FALSE, 0::REAL),
            ('public.starring_runtime_execution_database_readiness_v1()',
                'b5362bc1b081789a5b3ac4881fc2ea00c340a013630f7d5c809958ed1c045ec3',
                TRUE, 1::REAL),
            ('starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(text,text)',
                '68708aa143de1daae1247b18f3127e2abdc6d269a14e103d24e5ab6732d23f99',
                TRUE, 1::REAL),
            ('starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(text,text,bigint)',
                'da6c88ff80cf366e14f2c12a6204964d708156192a292cc6ad71b959588f07b8',
                FALSE, 0::REAL)
    ) AS contract(identity, definition_digest, returns_set, rows_estimate)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(contract.identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR NOT function_row.proisstrict
        OR function_row.proparallel <> 'u'
        OR function_row.prosecdef
            <> (pg_catalog.strpos(contract.identity, 'starring_runtime_private_v2.') = 0)
        OR function_row.proretset <> contract.returns_set
        OR function_row.prorows <> contract.rows_estimate
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM 'plpgsql'
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(function_row.oid),
                'UTF8'
            )),
            'hex'
        ) IS DISTINCT FROM contract.definition_digest
        OR (
            pg_catalog.strpos(contract.identity, 'starring_runtime_private_v2.') <> 0
            AND EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE privilege.grantee <> common_owner
            )
        );

    SELECT pg_catalog.count(*)
    INTO snapshot_mismatch_count
    FROM pg_temp.starring_runtime_execution_slot_epoch_snapshot AS snapshot
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = snapshot.function_oid
    WHERE function_row.oid IS NULL
        OR function_row.proowner IS DISTINCT FROM snapshot.function_owner
        OR function_row.proacl IS DISTINCT FROM snapshot.function_acl;

    SELECT function_row.prosrc
    INTO renew_source
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)'
    );

    SELECT function_row.prosrc
    INTO mutate_source
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)'
    );

    SELECT function_row.prosrc
    INTO prepare_source
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)'
    );

    SELECT function_row.prosrc
    INTO commit_source
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)'
    );

    FOR contract_row IN
        SELECT contract.*
        FROM (
            VALUES
                ('renew', renew_source,
                    'outcome_name := ''replayed''',
                    'UPDATE public.runtime_deployments AS deployment',
                    1::BIGINT),
                ('mutate', mutate_source,
                    'outcome_name := ''replayed''',
                    'UPDATE public.runtime_deployments AS deployment',
                    1::BIGINT),
                ('prepare', prepare_source,
                    'preparation_name := ''replayed''',
                    ''::TEXT,
                    0::BIGINT),
                ('commit', commit_source,
                    'outcome_name := ''replayed''',
                    'INSERT INTO public.runtime_attestations',
                    1::BIGINT)
        ) AS contract(
            function_name,
            function_source,
            replay_fragment,
            mutation_fragment,
            expected_begin_count
        )
    LOOP
        source_value := contract_row.function_source;
        global_position := pg_catalog.strpos(
            source_value,
            'starring_runtime_writer_fence_observe_v1'
        );
        slot_position := pg_catalog.strpos(
            source_value,
            'starring-runtime-serving-slot-v1:'
        );
        physical_position := pg_catalog.strpos(
            source_value,
            'starring_runtime_slot_writer_fence_lock_v2'
        );
        deployment_position := pg_catalog.strpos(
            source_value,
            'SELECT deployment.*'
        );
        replay_position := pg_catalog.strpos(
            source_value,
            contract_row.replay_fragment
        );
        begin_position := pg_catalog.strpos(
            source_value,
            'starring_runtime_slot_writer_fence_begin_unsafe_v2'
        );
        mutation_position := CASE
            WHEN contract_row.mutation_fragment = '' THEN 0
            ELSE pg_catalog.strpos(
                source_value,
                contract_row.mutation_fragment
            )
        END;

        IF global_position = 0
            OR slot_position = 0
            OR physical_position = 0
            OR deployment_position = 0
            OR replay_position = 0
            OR NOT (
                global_position < slot_position
                AND slot_position < physical_position
                AND physical_position < deployment_position
            )
            OR (
                pg_catalog.length(source_value)
                - pg_catalog.length(pg_catalog.replace(
                    source_value,
                    'starring_runtime_slot_writer_fence_lock_v2',
                    ''
                ))
            ) <> pg_catalog.length(
                'starring_runtime_slot_writer_fence_lock_v2'
            )
            OR (
                pg_catalog.length(source_value)
                - pg_catalog.length(pg_catalog.replace(
                    source_value,
                    'starring_runtime_slot_writer_fence_begin_unsafe_v2',
                    ''
                ))
            ) <> contract_row.expected_begin_count * pg_catalog.length(
                'starring_runtime_slot_writer_fence_begin_unsafe_v2'
            )
            OR (
                contract_row.expected_begin_count = 0
                AND begin_position <> 0
            )
            OR (
                contract_row.expected_begin_count = 1
                AND (
                    begin_position = 0
                    OR mutation_position = 0
                    OR replay_position >= begin_position
                    OR begin_position >= mutation_position
                )
            )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = pg_catalog.concat(
                    'runtime_execution_slot_writer_epoch_',
                    contract_row.function_name,
                    '_contract_drift'
                );
        END IF;
    END LOOP;

    IF pg_catalog.strpos(
            mutate_source,
            'starring_runtime_writer_fence_observe_v1'
        ) >= pg_catalog.strpos(
            mutate_source,
            'IF mutation_kind = ''accept_drain'' THEN'
        )
        OR common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR invalid_function_count <> 0
        OR snapshot_mismatch_count <> 0
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v1()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_slot_writer_epoch_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
