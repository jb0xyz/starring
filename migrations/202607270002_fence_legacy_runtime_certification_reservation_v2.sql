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
    public.runtime_certification_operations_v2,
    public.runtime_execution_mutation_markers,
    public.runtime_attestations,
    public.runtime_serving_leases,
    public.runtime_gateway_owners,
    public.automation_installations,
    public.automation_installation_authority_versions,
    public.automation_ruleset_activations,
    public.automation_ruleset_versions
IN ACCESS EXCLUSIVE MODE;

CREATE TEMPORARY TABLE pg_temp.starring_runtime_legacy_certification_fence_snapshot (
    function_oid OID PRIMARY KEY,
    function_owner OID NOT NULL,
    function_acl ACLITEM[]
) ON COMMIT DROP;

INSERT INTO pg_temp.starring_runtime_legacy_certification_fence_snapshot (
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
        ('public.starring_runtime_execution_claim_next_v1(text,bigint)'),
        ('public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)'),
        ('public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)'),
        ('public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)'),
        ('public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)'),
        ('public.starring_runtime_execution_recover_stale_live_v1()'),
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
            ('public.runtime_certification_operations_v2'),
            ('public.runtime_execution_mutation_markers'),
            ('public.runtime_attestations'),
            ('public.runtime_serving_leases'),
            ('public.runtime_gateway_owners'),
            ('public.automation_installations'),
            ('public.automation_installation_authority_versions'),
            ('public.automation_ruleset_activations'),
            ('public.automation_ruleset_versions')
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
            ('public.starring_runtime_execution_claim_next_v1(text,bigint)',
                '7cb6550864ed68e136e6e6b48c8cce59d179d895e3919a6abca77b7dfc7a4990',
                'plpgsql', TRUE, TRUE, 1::REAL, TRUE),
            ('public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)',
                '3c418773f843f2bf8827464624b4fd3124d8979c4c15b4323a96e76676c11c4e',
                'plpgsql', TRUE, TRUE, 1::REAL, TRUE),
            ('public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)',
                '76d965851a753501722854c0aecc22d51a3eaa92e93d55a299bfd59d5d922559',
                'plpgsql', TRUE, TRUE, 1::REAL, TRUE),
            ('public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)',
                '0b9fdc77ec2d85ea2513d6edf462ddddfe3304c4c1f53bec0432b5e0180e6967',
                'plpgsql', TRUE, TRUE, 1::REAL, TRUE),
            ('public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)',
                '5f0fa2982466bf5ca30250d2334c8314fa0c510ae586f6b750cdd1b662655fe1',
                'plpgsql', TRUE, TRUE, 1::REAL, TRUE),
            ('public.starring_runtime_execution_recover_stale_live_v1()',
                '635aab9493e1fd2ad8a138633d6447f88752589414a27c2bad4e56afdd22f932',
                'plpgsql', TRUE, TRUE, 1::REAL, TRUE),
            ('public.starring_runtime_execution_schema_manifest_v1()',
                '4089395be3df848f9025655ef183b0336ecfefd62861bf735f53c4c26aad2ae7',
                'plpgsql', TRUE, FALSE, 0::REAL, TRUE),
            ('public.starring_runtime_execution_database_readiness_v1()',
                '6962c1c2ffdd862a86aed3c84569ac50307964d59711d0bddc26aadbf68577e2',
                'plpgsql', TRUE, TRUE, 1::REAL, TRUE),
            ('public.starring_runtime_execution_database_identity_v1()',
                '455bf3c81d3b144ab6c3f9da24d8c5c5a7961612d1d87f66da7b44bb0e0f9961',
                'sql', TRUE, FALSE, 0::REAL, TRUE),
            ('public.starring_runtime_writer_fence_observe_v1()',
                'e1d910428439dfa3387988aea6e57d49e3a9a54ba31147f217a19679ed78b5d7',
                'plpgsql', TRUE, TRUE, 1::REAL, TRUE),
            ('starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(text,text)',
                '68708aa143de1daae1247b18f3127e2abdc6d269a14e103d24e5ab6732d23f99',
                'plpgsql', FALSE, TRUE, 1::REAL, TRUE),
            ('starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(text,text,bigint)',
                'da6c88ff80cf366e14f2c12a6204964d708156192a292cc6ad71b959588f07b8',
                'plpgsql', FALSE, FALSE, 0::REAL, TRUE)
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
    FROM pg_temp.starring_runtime_legacy_certification_fence_snapshot AS snapshot
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
                ('public.starring_runtime_execution_claim_next_v1(text,bigint)'),
                ('public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)'),
                ('public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)'),
                ('public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)'),
                ('public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)'),
                ('public.starring_runtime_execution_recover_stale_live_v1()'),
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
            MESSAGE = 'runtime_legacy_certification_fence_executor_not_quiesced';
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
            FROM pg_temp.starring_runtime_legacy_certification_fence_snapshot
        ) <> 8
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
            MESSAGE = 'runtime_legacy_certification_fence_preflight_drift';
    END IF;
END;
$preflight$;

DO $patch_claim$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_claim_next_v1(text,bigint)'
    );

    previous_fragment :=
        '    WHERE deployment.controller_id = expected_controller_id' || E'\n' ||
        '        AND deployment.controller_lease_expires_at' || E'\n' ||
        '            > replay_lookup_clock' || E'\n' ||
        '        AND deployment.phase NOT IN (''live'', ''superseded'', ''cancelled'')' || E'\n' ||
        '    ORDER BY deployment.controller_acquired_at, deployment.deployment_id';
    next_fragment :=
        '    WHERE deployment.controller_id = expected_controller_id' || E'\n' ||
        '        AND deployment.controller_lease_expires_at' || E'\n' ||
        '            > replay_lookup_clock' || E'\n' ||
        '        AND deployment.phase NOT IN (''live'', ''superseded'', ''cancelled'')' || E'\n' ||
        '        AND NOT EXISTS (' || E'\n' ||
        '            SELECT 1' || E'\n' ||
        '            FROM public.runtime_certification_operations_v2 AS reservation' || E'\n' ||
        '            WHERE reservation.tenant_id = deployment.tenant_id' || E'\n' ||
        '                AND reservation.installation_id = deployment.installation_id' || E'\n' ||
        '                AND reservation.deployment_id = deployment.deployment_id' || E'\n' ||
        '        )' || E'\n' ||
        '    ORDER BY deployment.controller_acquired_at, deployment.deployment_id';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_legacy_certification_fence_claim_replay_selector_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            FOR UPDATE;' || E'\n\n' ||
        '            IF NOT FOUND THEN' || E'\n' ||
        '                RAISE no_data_found;' || E'\n' ||
        '            END IF;' || E'\n\n' ||
        '            replay_validation_clock := GREATEST(';
    next_fragment :=
        '            FOR UPDATE;' || E'\n\n' ||
        '            IF NOT FOUND THEN' || E'\n' ||
        '                RAISE no_data_found;' || E'\n' ||
        '            END IF;' || E'\n\n' ||
        '            IF EXISTS (' || E'\n' ||
        '                SELECT 1' || E'\n' ||
        '                FROM public.runtime_certification_operations_v2 AS reservation' || E'\n' ||
        '                WHERE reservation.tenant_id = deployment_row.tenant_id' || E'\n' ||
        '                    AND reservation.installation_id = deployment_row.installation_id' || E'\n' ||
        '                    AND reservation.deployment_id = deployment_row.deployment_id' || E'\n' ||
        '            ) THEN' || E'\n' ||
        '                RAISE no_data_found;' || E'\n' ||
        '            END IF;' || E'\n\n' ||
        '            replay_validation_clock := GREATEST(';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_legacy_certification_fence_claim_replay_lock_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '        AND slot_fence.pending_drain_intent_id IS NULL' || E'\n' ||
        '        AND pending_drain.drain_intent_id IS NULL' || E'\n' ||
        '        AND deployment.blocked_at IS NULL';
    next_fragment :=
        '        AND slot_fence.pending_drain_intent_id IS NULL' || E'\n' ||
        '        AND pending_drain.drain_intent_id IS NULL' || E'\n' ||
        '        AND NOT EXISTS (' || E'\n' ||
        '            SELECT 1' || E'\n' ||
        '            FROM public.runtime_certification_operations_v2 AS reservation' || E'\n' ||
        '            WHERE reservation.tenant_id = deployment.tenant_id' || E'\n' ||
        '                AND reservation.installation_id = deployment.installation_id' || E'\n' ||
        '                AND reservation.deployment_id = deployment.deployment_id' || E'\n' ||
        '        )' || E'\n' ||
        '        AND deployment.blocked_at IS NULL';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_legacy_certification_fence_claim_selector_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            FOR UPDATE SKIP LOCKED;' || E'\n\n' ||
        '            IF NOT FOUND THEN' || E'\n' ||
        '                RAISE no_data_found;' || E'\n' ||
        '            END IF;' || E'\n' ||
        '            candidate_found := TRUE;';
    next_fragment :=
        '            FOR UPDATE SKIP LOCKED;' || E'\n\n' ||
        '            IF NOT FOUND THEN' || E'\n' ||
        '                RAISE no_data_found;' || E'\n' ||
        '            END IF;' || E'\n\n' ||
        '            IF EXISTS (' || E'\n' ||
        '                SELECT 1' || E'\n' ||
        '                FROM public.runtime_certification_operations_v2 AS reservation' || E'\n' ||
        '                WHERE reservation.tenant_id = deployment_row.tenant_id' || E'\n' ||
        '                    AND reservation.installation_id = deployment_row.installation_id' || E'\n' ||
        '                    AND reservation.deployment_id = deployment_row.deployment_id' || E'\n' ||
        '            ) THEN' || E'\n' ||
        '                RAISE no_data_found;' || E'\n' ||
        '            END IF;' || E'\n' ||
        '            candidate_found := TRUE;';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_legacy_certification_fence_claim_lock_patch_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$patch_claim$;

DO $patch_direct_writers$
DECLARE
    patch_row RECORD;
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    FOR patch_row IN
        SELECT *
        FROM (
            VALUES
                (
                    'public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)',
                    'runtime_execution_renew_ownership_lost',
                    '    IF deployment_row.revision = expected_deployment_revision + 1 THEN',
                    'renew'
                ),
                (
                    'public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)',
                    'runtime_execution_mutation_ownership_lost',
                    '    IF deployment_row.revision = expected_deployment_revision + 1',
                    'mutate'
                ),
                (
                    'public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)',
                    'runtime_execution_certify_prepare_ownership_lost',
                    '    authority_outcome := public.starring_runtime_lock_current_authority(',
                    'certify_prepare'
                ),
                (
                    'public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)',
                    'runtime_execution_certify_commit_ownership_lost',
                    '    authority_outcome := public.starring_runtime_lock_current_authority(',
                    'certify_commit'
                )
        ) AS patch(
            identity,
            ownership_message,
            continuation,
            patch_name
        )
    LOOP
        SELECT pg_catalog.pg_get_functiondef(function_row.oid)
        INTO definition
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid = pg_catalog.to_regprocedure(
            patch_row.identity
        );

        previous_fragment :=
            '            MESSAGE = ''' || patch_row.ownership_message ||
                ''';' || E'\n' ||
            '    END IF;' || E'\n\n' ||
            patch_row.continuation;
        next_fragment :=
            '            MESSAGE = ''' || patch_row.ownership_message ||
                ''';' || E'\n' ||
            '    END IF;' || E'\n\n' ||
            '    IF EXISTS (' || E'\n' ||
            '        SELECT 1' || E'\n' ||
            '        FROM public.runtime_certification_operations_v2 AS reservation' || E'\n' ||
            '        WHERE reservation.tenant_id = deployment_row.tenant_id' || E'\n' ||
            '            AND reservation.installation_id = deployment_row.installation_id' || E'\n' ||
            '            AND reservation.deployment_id = deployment_row.deployment_id' || E'\n' ||
            '    ) THEN' || E'\n' ||
            '        RAISE EXCEPTION USING' || E'\n' ||
            '            ERRCODE = ''RX001'',' || E'\n' ||
            '            MESSAGE = ''' || patch_row.ownership_message ||
                ''';' || E'\n' ||
            '    END IF;' || E'\n\n' ||
            patch_row.continuation;
        IF pg_catalog.strpos(definition, previous_fragment) = 0
            OR pg_catalog.strpos(
                pg_catalog.replace(definition, previous_fragment, ''),
                previous_fragment
            ) <> 0
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = pg_catalog.concat(
                    'runtime_legacy_certification_fence_',
                    patch_row.patch_name,
                    '_patch_drift'
                );
        END IF;
        EXECUTE pg_catalog.replace(
            definition,
            previous_fragment,
            next_fragment
        );
    END LOOP;
END;
$patch_direct_writers$;

DO $patch_recover$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_recover_stale_live_v1()'
    );

    previous_fragment :=
        '    WHERE deployment.phase = ''live''' || E'\n' ||
        '        AND slot_fence.pending_drain_intent_id IS NULL' || E'\n' ||
        '        AND pending_drain.drain_intent_id IS NULL';
    next_fragment :=
        '    WHERE deployment.phase = ''live''' || E'\n' ||
        '        AND slot_fence.pending_drain_intent_id IS NULL' || E'\n' ||
        '        AND pending_drain.drain_intent_id IS NULL' || E'\n' ||
        '        AND NOT EXISTS (' || E'\n' ||
        '            SELECT 1' || E'\n' ||
        '            FROM public.runtime_certification_operations_v2 AS reservation' || E'\n' ||
        '            WHERE reservation.tenant_id = deployment.tenant_id' || E'\n' ||
        '                AND reservation.installation_id = deployment.installation_id' || E'\n' ||
        '                AND reservation.deployment_id = deployment.deployment_id' || E'\n' ||
        '        )';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_legacy_certification_fence_recover_selector_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            FOR UPDATE SKIP LOCKED;' || E'\n\n' ||
        '            IF NOT FOUND THEN' || E'\n' ||
        '                RAISE no_data_found;' || E'\n' ||
        '            END IF;' || E'\n' ||
        '            candidate_found := TRUE;';
    next_fragment :=
        '            FOR UPDATE SKIP LOCKED;' || E'\n\n' ||
        '            IF NOT FOUND THEN' || E'\n' ||
        '                RAISE no_data_found;' || E'\n' ||
        '            END IF;' || E'\n\n' ||
        '            IF EXISTS (' || E'\n' ||
        '                SELECT 1' || E'\n' ||
        '                FROM public.runtime_certification_operations_v2 AS reservation' || E'\n' ||
        '                WHERE reservation.tenant_id = deployment_row.tenant_id' || E'\n' ||
        '                    AND reservation.installation_id = deployment_row.installation_id' || E'\n' ||
        '                    AND reservation.deployment_id = deployment_row.deployment_id' || E'\n' ||
        '            ) THEN' || E'\n' ||
        '                RAISE no_data_found;' || E'\n' ||
        '            END IF;' || E'\n' ||
        '            candidate_found := TRUE;';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_legacy_certification_fence_recover_lock_patch_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$patch_recover$;

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
        '    RETURN observed_count = 650' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''f053e9131dcd32f1168ff6201ad57f4f40e3165ab619414a3552b74717bbe2c9'';';
    next_fragment :=
        '    RETURN observed_count = 650' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''65c41a8e67ec225e567403f2f24eba8e31964a51d1a1ce484774cae3db5bd58c'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_legacy_certification_fence_manifest_patch_drift';
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
        '''4089395be3df848f9025655ef183b0336ecfefd62861bf735f53c4c26aad2ae7''::TEXT';
    next_fragment :=
        '''ff16060ff3ddcb6d71dee07138e411674dd446a792de6cd2e22b400378cf2df4''::TEXT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_legacy_certification_fence_readiness_patch_drift';
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
    claim_source TEXT;
    recover_source TEXT;
    direct_source TEXT;
    reserved_guard_count BIGINT;
    writer_position INTEGER;
    controller_position INTEGER;
    slot_position INTEGER;
    physical_position INTEGER;
    deployment_lock_position INTEGER;
    reservation_position INTEGER;
    continuation_position INTEGER;
    direct_identity TEXT;
    continuation_marker TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            ('public.starring_runtime_execution_claim_next_v1(text,bigint)',
                'cc5475b256b6b48f3c4f6d3933461cdcdeff1dbdb974d32d7d735348d8f14eb4',
                TRUE, TRUE, 1::REAL),
            ('public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)',
                '00fb1426fd8711b496b35e0658db13a534560ba13191d710c4274cd54461275c',
                TRUE, TRUE, 1::REAL),
            ('public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)',
                '9e201e149dac432794bfcfc23b424f59741869fcf9d39765693a21b2451646ce',
                TRUE, TRUE, 1::REAL),
            ('public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)',
                '9be2d9b8c329665cea635e8a44144aabe58ed684d3d227eb60ad583f78640269',
                TRUE, TRUE, 1::REAL),
            ('public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)',
                '5c1b3c8c50e3a2d3d0f0149bf408fca51069db975573b3375f0f76bc1e5c159c',
                TRUE, TRUE, 1::REAL),
            ('public.starring_runtime_execution_recover_stale_live_v1()',
                'b30467f0d866bbcadb82bd6322e5d169aec4c443770c896660b885aa3e3b7457',
                TRUE, TRUE, 1::REAL),
            ('public.starring_runtime_execution_schema_manifest_v1()',
                'ff16060ff3ddcb6d71dee07138e411674dd446a792de6cd2e22b400378cf2df4',
                TRUE, FALSE, 0::REAL),
            ('public.starring_runtime_execution_database_readiness_v1()',
                'a57602a79ee2aa5ac884dffb56d152bb5721d111e07eac5a5f853952d6db214f',
                TRUE, TRUE, 1::REAL)
    ) AS contract(
        identity,
        definition_digest,
        security_definer,
        returns_set,
        rows_estimate
    )
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
        OR function_row.prosecdef <> contract.security_definer
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
        ) IS DISTINCT FROM contract.definition_digest;

    SELECT pg_catalog.count(*)
    INTO snapshot_mismatch_count
    FROM pg_temp.starring_runtime_legacy_certification_fence_snapshot AS snapshot
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = snapshot.function_oid
    WHERE function_row.oid IS NULL
        OR function_row.proowner IS DISTINCT FROM snapshot.function_owner
        OR function_row.proacl IS DISTINCT FROM snapshot.function_acl;

    SELECT function_row.prosrc
    INTO claim_source
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_claim_next_v1(text,bigint)'
    );

    SELECT function_row.prosrc
    INTO recover_source
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_recover_stale_live_v1()'
    );

    reserved_guard_count := (
        pg_catalog.length(claim_source)
        - pg_catalog.length(pg_catalog.replace(
            claim_source,
            'FROM public.runtime_certification_operations_v2 AS reservation',
            ''
        ))
    ) / pg_catalog.length(
        'FROM public.runtime_certification_operations_v2 AS reservation'
    );
    writer_position := pg_catalog.strpos(
        claim_source,
        'starring_runtime_writer_fence_observe_v1'
    );
    controller_position := pg_catalog.strpos(
        claim_source,
        'starring-runtime-execution-controller-v1:'
    );
    slot_position := pg_catalog.strpos(
        claim_source,
        'starring-runtime-serving-slot-v1:'
    );
    physical_position := pg_catalog.strpos(
        claim_source,
        'starring_runtime_slot_writer_fence_lock_v2'
    );
    deployment_lock_position := pg_catalog.strpos(
        claim_source,
        'FOR UPDATE;'
    );
    reservation_position := pg_catalog.strpos(
        claim_source,
        'WHERE reservation.tenant_id = deployment_row.tenant_id'
    );
    continuation_position := pg_catalog.strpos(
        claim_source,
        'replay_validation_clock := GREATEST('
    );

    IF reserved_guard_count <> 4
        OR writer_position = 0
        OR controller_position = 0
        OR slot_position = 0
        OR physical_position = 0
        OR deployment_lock_position = 0
        OR reservation_position = 0
        OR continuation_position = 0
        OR NOT (
            writer_position < controller_position
            AND controller_position < slot_position
            AND slot_position < physical_position
            AND physical_position < deployment_lock_position
            AND deployment_lock_position < reservation_position
            AND reservation_position < continuation_position
        )
        OR pg_catalog.strpos(
            claim_source,
            'WHERE reservation.tenant_id = deployment.tenant_id'
        ) = 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_legacy_certification_fence_claim_contract_drift';
    END IF;

    reserved_guard_count := (
        pg_catalog.length(recover_source)
        - pg_catalog.length(pg_catalog.replace(
            recover_source,
            'FROM public.runtime_certification_operations_v2 AS reservation',
            ''
        ))
    ) / pg_catalog.length(
        'FROM public.runtime_certification_operations_v2 AS reservation'
    );
    writer_position := pg_catalog.strpos(
        recover_source,
        'starring_runtime_writer_fence_observe_v1'
    );
    slot_position := pg_catalog.strpos(
        recover_source,
        'starring-runtime-serving-slot-v1:'
    );
    physical_position := pg_catalog.strpos(
        recover_source,
        'starring_runtime_slot_writer_fence_lock_v2'
    );
    deployment_lock_position := pg_catalog.strpos(
        recover_source,
        'FOR UPDATE SKIP LOCKED;'
    );
    reservation_position := pg_catalog.strpos(
        recover_source,
        'WHERE reservation.tenant_id = deployment_row.tenant_id'
    );

    IF reserved_guard_count <> 2
        OR writer_position = 0
        OR slot_position = 0
        OR physical_position = 0
        OR deployment_lock_position = 0
        OR reservation_position = 0
        OR NOT (
            writer_position < slot_position
            AND slot_position < physical_position
            AND physical_position < deployment_lock_position
            AND deployment_lock_position < reservation_position
        )
        OR pg_catalog.strpos(
            recover_source,
            'WHERE reservation.tenant_id = deployment.tenant_id'
        ) = 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_legacy_certification_fence_recover_contract_drift';
    END IF;

    FOR direct_identity, continuation_marker IN
        SELECT *
        FROM (
            VALUES
                (
                    'public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)',
                    'IF deployment_row.revision = expected_deployment_revision + 1 THEN'
                ),
                (
                    'public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)',
                    'IF deployment_row.revision = expected_deployment_revision + 1'
                ),
                (
                    'public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)',
                    'authority_outcome := public.starring_runtime_lock_current_authority('
                ),
                (
                    'public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)',
                    'authority_outcome := public.starring_runtime_lock_current_authority('
                )
        ) AS direct(identity, continuation)
    LOOP
        SELECT function_row.prosrc
        INTO direct_source
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid = pg_catalog.to_regprocedure(direct_identity);

        reserved_guard_count := (
            pg_catalog.length(direct_source)
            - pg_catalog.length(pg_catalog.replace(
                direct_source,
                'FROM public.runtime_certification_operations_v2 AS reservation',
                ''
            ))
        ) / pg_catalog.length(
            'FROM public.runtime_certification_operations_v2 AS reservation'
        );
        writer_position := pg_catalog.strpos(
            direct_source,
            'starring_runtime_writer_fence_observe_v1'
        );
        slot_position := pg_catalog.strpos(
            direct_source,
            'starring-runtime-serving-slot-v1:'
        );
        physical_position := pg_catalog.strpos(
            direct_source,
            'starring_runtime_slot_writer_fence_lock_v2'
        );
        deployment_lock_position := pg_catalog.strpos(
            direct_source,
            'FOR UPDATE;'
        );
        reservation_position := pg_catalog.strpos(
            direct_source,
            'WHERE reservation.tenant_id = deployment_row.tenant_id'
        );
        continuation_position := pg_catalog.strpos(
            direct_source,
            continuation_marker
        );

        IF reserved_guard_count <> 1
            OR writer_position = 0
            OR slot_position = 0
            OR physical_position = 0
            OR deployment_lock_position = 0
            OR reservation_position = 0
            OR continuation_position = 0
            OR NOT (
                writer_position < slot_position
                AND slot_position < physical_position
                AND physical_position < deployment_lock_position
                AND deployment_lock_position < reservation_position
                AND reservation_position < continuation_position
            )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_legacy_certification_fence_direct_contract_drift';
        END IF;
    END LOOP;

    IF common_owner IS NULL
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
            MESSAGE = 'runtime_legacy_certification_fence_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
