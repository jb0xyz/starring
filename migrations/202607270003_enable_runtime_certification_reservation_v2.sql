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

CREATE TEMPORARY TABLE pg_temp.starring_runtime_certification_enable_snapshot (
    function_oid OID PRIMARY KEY,
    function_owner OID NOT NULL,
    function_acl ACLITEM[]
) ON COMMIT DROP;

INSERT INTO pg_temp.starring_runtime_certification_enable_snapshot (
    function_oid,
    function_owner,
    function_acl
)
SELECT
    function_row.oid,
    function_row.proowner,
    function_row.proacl
FROM pg_catalog.pg_proc AS function_row
INNER JOIN pg_catalog.pg_namespace AS namespace
    ON namespace.oid = function_row.pronamespace
WHERE function_row.oid >= 16384
    AND namespace.nspname NOT IN ('pg_catalog', 'information_schema')
    AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_';

DO $preflight$
DECLARE
    common_owner OID;
    executor_role OID;
    executor_role_is_quarantined BOOLEAN;
    invalid_relation_count BIGINT;
    invalid_function_count BIGINT;
    invalid_capability_acl_count BIGINT;
    invalid_owner_only_acl_count BIGINT;
    invalid_trigger_count BIGINT;
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
            (
                'public.starring_runtime_execution_database_identity_v1()',
                '455bf3c81d3b144ab6c3f9da24d8c5c5a7961612d1d87f66da7b44bb0e0f9961'
            ),
            (
                'public.starring_runtime_execution_claim_next_v1(text,bigint)',
                'cc5475b256b6b48f3c4f6d3933461cdcdeff1dbdb974d32d7d735348d8f14eb4'
            ),
            (
                'public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)',
                '00fb1426fd8711b496b35e0658db13a534560ba13191d710c4274cd54461275c'
            ),
            (
                'public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)',
                '9e201e149dac432794bfcfc23b424f59741869fcf9d39765693a21b2451646ce'
            ),
            (
                'public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)',
                '9be2d9b8c329665cea635e8a44144aabe58ed684d3d227eb60ad583f78640269'
            ),
            (
                'public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)',
                '5c1b3c8c50e3a2d3d0f0149bf408fca51069db975573b3375f0f76bc1e5c159c'
            ),
            (
                'public.starring_runtime_execution_recover_stale_live_v1()',
                'b30467f0d866bbcadb82bd6322e5d169aec4c443770c896660b885aa3e3b7457'
            ),
            (
                'public.starring_runtime_execution_schema_manifest_v1()',
                'ff16060ff3ddcb6d71dee07138e411674dd446a792de6cd2e22b400378cf2df4'
            ),
            (
                'public.starring_runtime_execution_database_readiness_v1()',
                'a57602a79ee2aa5ac884dffb56d152bb5721d111e07eac5a5f853952d6db214f'
            ),
            (
                'public.reject_runtime_certification_reservation_mutation_v2()',
                'ff49c4ce2863940ca964444d9046caed23cf7db0cac97163e2ef73d7bd9c207b'
            ),
            (
                'public.starring_runtime_certification_reserve_intent_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint,bytea,text)',
                '4c088bff39108bccd0690c1a8cf395572c7e6f0b4380d4df5460c91398e5038d'
            ),
            (
                'public.starring_runtime_certification_reservation_observe_v2(text,text,text,bigint,bigint)',
                'a6443bcca0fab54523f1570c656da8792dd37a002bca71e0a5d6a53d34ebff39'
            ),
            (
                'starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint)',
                'a98d87b7aba288ca18c44c6c8419ad6092595c8a321da18b7d3d5f005a6a64e9'
            ),
            (
                'starring_runtime_private_v2.starring_runtime_certification_intent_fingerprint_v2(bytea)',
                '5e54a6b0fec4e3d68fb5d12d14fddd7afe13f1279d0d8ea1f0d7681d5037e13b'
            )
    ) AS expected(identity, definition_digest)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(function_row.oid),
                'UTF8'
            )),
            'hex'
        ) IS DISTINCT FROM expected.definition_digest;

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
            COALESCE(pg_catalog.bool_or(
                privilege.grantee <> common_owner
                AND (
                    privilege.grantee = 0
                    OR privilege.privilege_type <> 'EXECUTE'
                    OR privilege.is_grantable
                    OR privilege.grantor <> common_owner
                )
            ), FALSE) AS invalid
        FROM pg_catalog.pg_proc AS function_row
        LEFT JOIN LATERAL pg_catalog.aclexplode(COALESCE(
            function_row.proacl,
            pg_catalog.acldefault('f', function_row.proowner)
        )) AS privilege ON TRUE
        WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_database_identity_v1()'
        )
    ), observed AS (
        SELECT
            expected.identity,
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
                ('public.starring_runtime_execution_database_readiness_v1()'),
                ('public.starring_runtime_execution_database_identity_v1()'),
                ('public.starring_runtime_execution_claim_next_v1(text,bigint)'),
                ('public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)'),
                ('public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)'),
                ('public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)'),
                ('public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)'),
                ('public.starring_runtime_execution_recover_stale_live_v1()'),
                ('public.starring_runtime_observe_previous_serving_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,jsonb)'),
                ('public.starring_runtime_gateway_owner_observe_v1(text)'),
                ('public.starring_runtime_gateway_owner_acquire_v1(text,text,text,bigint)'),
                ('public.starring_runtime_gateway_owner_renew_v1(text,text,bigint,text,bigint,bigint)'),
                ('public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)'),
                ('public.starring_runtime_writer_fence_observe_v1()'),
                ('public.starring_runtime_product_drain_observe_v2(text,text,text,bigint,text,text)')
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
    FROM (
        VALUES
            ('public.reject_runtime_certification_reservation_mutation_v2()'),
            ('public.starring_runtime_certification_reserve_intent_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint,bytea,text)'),
            ('public.starring_runtime_certification_reservation_observe_v2(text,text,text,bigint,bigint)'),
            ('starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint)'),
            ('starring_runtime_private_v2.starring_runtime_certification_intent_fingerprint_v2(bytea)')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
        ) <> 1
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
        );

    SELECT pg_catalog.count(*)
    INTO invalid_owner_only_acl_count
    FROM (
        SELECT invalid_owner_only_acl_count AS invalid_count
        UNION ALL
        SELECT pg_catalog.count(*)
        FROM pg_catalog.pg_class AS relation
        WHERE relation.oid = pg_catalog.to_regclass(
                'public.runtime_certification_operations_v2'
            )
            AND EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    relation.relacl,
                    pg_catalog.acldefault('r', relation.relowner)
                )) AS privilege
                WHERE privilege.grantee <> common_owner
                    OR privilege.grantor <> common_owner
                    OR privilege.privilege_type NOT IN (
                        'INSERT',
                        'SELECT',
                        'UPDATE',
                        'DELETE',
                        'TRUNCATE',
                        'REFERENCES',
                        'TRIGGER'
                    )
                    OR privilege.is_grantable
            )
        UNION ALL
        SELECT pg_catalog.count(*)
        FROM pg_catalog.pg_attribute AS attribute
        CROSS JOIN LATERAL pg_catalog.aclexplode(attribute.attacl) AS privilege
        WHERE attribute.attrelid = pg_catalog.to_regclass(
                'public.runtime_certification_operations_v2'
            )
            AND attribute.attnum > 0
            AND NOT attribute.attisdropped
            AND privilege.grantee <> common_owner
    ) AS invalid
    WHERE invalid.invalid_count <> 0;

    SELECT pg_catalog.count(*)
    INTO invalid_trigger_count
    FROM (
        VALUES
            ('runtime_certification_operations_v2_reject_row_mutation', 31),
            ('runtime_certification_operations_v2_reject_truncate', 34)
    ) AS expected(trigger_name, trigger_type)
    LEFT JOIN pg_catalog.pg_trigger AS trigger_row
        ON trigger_row.tgrelid = pg_catalog.to_regclass(
            'public.runtime_certification_operations_v2'
        )
        AND trigger_row.tgname = expected.trigger_name
    WHERE trigger_row.oid IS NULL
        OR trigger_row.tgisinternal
        OR trigger_row.tgenabled <> 'O'
        OR trigger_row.tgtype <> expected.trigger_type
        OR trigger_row.tgfoid <> pg_catalog.to_regprocedure(
            'public.reject_runtime_certification_reservation_mutation_v2()'
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
    WHERE membership.roleid = executor_role
        OR membership.member = executor_role;

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
            MESSAGE = 'runtime_certification_enable_executor_not_quiesced';
    END IF;

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR invalid_relation_count <> 0
        OR invalid_function_count <> 0
        OR invalid_capability_acl_count <> 0
        OR invalid_owner_only_acl_count <> 0
        OR invalid_trigger_count <> 0
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_temp.starring_runtime_certification_enable_snapshot
        ) = 0
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
            MESSAGE = 'runtime_certification_enable_preflight_drift';
    END IF;
END;
$preflight$;

DO $execution_acl$
DECLARE
    common_owner OID;
    executor_role OID;
    executor_name NAME;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

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

    IF executor_role IS NOT NULL THEN
        executor_name := pg_catalog.pg_get_userbyid(executor_role);
        IF executor_name IS NULL THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_certification_enable_execution_acl_drift';
        END IF;
        EXECUTE pg_catalog.format(
            'GRANT EXECUTE ON FUNCTION public.starring_runtime_certification_reserve_intent_v2(BIGINT,TEXT,TEXT,TEXT,TEXT,BIGINT,TEXT,BIGINT,BIGINT,BIGINT,TEXT,TEXT,BIGINT,TEXT,BIGINT,TEXT,BIGINT,TEXT,TEXT,BIGINT,BIGINT,TEXT,TEXT,TEXT,BIGINT,BYTEA,TEXT) TO %I',
            executor_name
        );
        EXECUTE pg_catalog.format(
            'GRANT EXECUTE ON FUNCTION public.starring_runtime_certification_reservation_observe_v2(TEXT,TEXT,TEXT,BIGINT,BIGINT) TO %I',
            executor_name
        );
    END IF;
END;
$execution_acl$;

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
        '            (' || E'\n' ||
        '                ''public.starring_runtime_product_drain_observe_v2(text,text,text,bigint,text,text)'',' || E'\n' ||
        '                ''expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_revision bigint, expected_slot_guild_id text, expected_slot_ruleset_key text''::TEXT,' || E'\n' ||
        '                ''TABLE(outcome_name text, locked_snapshot jsonb, observed_at timestamp with time zone, product_tenant_id text, product_installation_id text, product_deployment_id text, product_expected_revision bigint, product_operation_id text, product_expected_target jsonb, product_mutation_request_bytes bytea, product_mutation_digest text, drain_tenant_id text, drain_installation_id text, drain_deployment_id text, drain_slot_guild_id text, drain_slot_ruleset_key text, drain_expected_revision bigint, drain_intent_id text, drain_intent_request_bytes bytea, drain_intent_digest text, intent_revision bigint, intent_state text)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            )' || E'\n' ||
        '    ) AS expected(';
    next_fragment :=
        '            (' || E'\n' ||
        '                ''public.starring_runtime_product_drain_observe_v2(text,text,text,bigint,text,text)'',' || E'\n' ||
        '                ''expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_revision bigint, expected_slot_guild_id text, expected_slot_ruleset_key text''::TEXT,' || E'\n' ||
        '                ''TABLE(outcome_name text, locked_snapshot jsonb, observed_at timestamp with time zone, product_tenant_id text, product_installation_id text, product_deployment_id text, product_expected_revision bigint, product_operation_id text, product_expected_target jsonb, product_mutation_request_bytes bytea, product_mutation_digest text, drain_tenant_id text, drain_installation_id text, drain_deployment_id text, drain_slot_guild_id text, drain_slot_ruleset_key text, drain_expected_revision bigint, drain_intent_id text, drain_intent_request_bytes bytea, drain_intent_digest text, intent_revision bigint, intent_state text)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            ),' || E'\n' ||
        '            (' || E'\n' ||
        '                ''public.starring_runtime_certification_reserve_intent_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint,bytea,text)'',' || E'\n' ||
        '                ''requested_action_id bigint, requested_operation_id text, expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_deployment_revision bigint, expected_controller_id text, expected_controller_fencing_token bigint, expected_runtime_generation bigint, expected_convergence_attempt_no bigint, expected_target_guild_id text, expected_target_ruleset_key text, expected_target_version bigint, expected_target_content_hash text, expected_target_binding_revision bigint, expected_target_binding_fingerprint text, expected_installation_authority_revision bigint, expected_process_instance_id text, expected_gateway_shard_id text, expected_gateway_lease_epoch bigint, expected_gateway_owner_revision bigint, expected_runtime_build_revision text, expected_panel_certificate_id text, expected_panel_report_digest text, requested_serving_lease_milliseconds bigint, proposed_certification_intent_bytes bytea, proposed_intent_fingerprint text''::TEXT,' || E'\n' ||
        '                ''TABLE(outcome_name text, locked_snapshot jsonb, locked_convergence_attempt_no bigint, observed_at timestamp with time zone, operation_id text, tenant_id text, installation_id text, deployment_id text, deployment_revision bigint, convergence_attempt_no bigint, certification_intent_bytes bytea, intent_fingerprint text)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            ),' || E'\n' ||
        '            (' || E'\n' ||
        '                ''public.starring_runtime_certification_reservation_observe_v2(text,text,text,bigint,bigint)'',' || E'\n' ||
        '                ''expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_deployment_revision bigint, expected_convergence_attempt_no bigint''::TEXT,' || E'\n' ||
        '                ''TABLE(outcome_name text, locked_snapshot jsonb, locked_convergence_attempt_no bigint, observed_at timestamp with time zone, operation_id text, tenant_id text, installation_id text, deployment_id text, deployment_revision bigint, convergence_attempt_no bigint, certification_intent_bytes bytea, intent_fingerprint text)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            )' || E'\n' ||
        '    ) AS expected(';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_enable_readiness_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            (''public.reject_runtime_certification_reservation_mutation_v2()''),' || E'\n' ||
        '            (''public.starring_runtime_certification_reserve_intent_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint,bytea,text)''),' || E'\n' ||
        '            (''public.starring_runtime_certification_reservation_observe_v2(text,text,text,bigint,bigint)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint)'')';
    next_fragment :=
        '            (''public.reject_runtime_certification_reservation_mutation_v2()''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint)'')';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_enable_readiness_protected_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_product_drain_observe_v2(text,text,text,bigint,text,text)''' || E'\n' ||
        '            )' || E'\n' ||
        '        )';
    next_fragment :=
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_product_drain_observe_v2(text,text,text,bigint,text,text)''' || E'\n' ||
        '            ),' || E'\n' ||
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_certification_reserve_intent_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint,bytea,text)''' || E'\n' ||
        '            ),' || E'\n' ||
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_certification_reservation_observe_v2(text,text,text,bigint,bigint)''' || E'\n' ||
        '            )' || E'\n' ||
        '        )';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_enable_readiness_allowlist_patch_drift';
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
    invalid_function_count BIGINT;
    invalid_target_acl_count BIGINT;
    invalid_private_acl_count BIGINT;
    invalid_relation_acl_count BIGINT;
    invalid_trigger_count BIGINT;
    invalid_capability_acl_count BIGINT;
    snapshot_mismatch_count BIGINT;
    new_function_count BIGINT;
    manifest_digest TEXT;
    readiness_digest TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

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

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_runtime_execution_database_identity_v1()',
                '455bf3c81d3b144ab6c3f9da24d8c5c5a7961612d1d87f66da7b44bb0e0f9961'
            ),
            (
                'public.starring_runtime_execution_claim_next_v1(text,bigint)',
                'cc5475b256b6b48f3c4f6d3933461cdcdeff1dbdb974d32d7d735348d8f14eb4'
            ),
            (
                'public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)',
                '00fb1426fd8711b496b35e0658db13a534560ba13191d710c4274cd54461275c'
            ),
            (
                'public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)',
                '9e201e149dac432794bfcfc23b424f59741869fcf9d39765693a21b2451646ce'
            ),
            (
                'public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)',
                '9be2d9b8c329665cea635e8a44144aabe58ed684d3d227eb60ad583f78640269'
            ),
            (
                'public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)',
                '5c1b3c8c50e3a2d3d0f0149bf408fca51069db975573b3375f0f76bc1e5c159c'
            ),
            (
                'public.starring_runtime_execution_recover_stale_live_v1()',
                'b30467f0d866bbcadb82bd6322e5d169aec4c443770c896660b885aa3e3b7457'
            ),
            (
                'public.starring_runtime_execution_schema_manifest_v1()',
                'ff16060ff3ddcb6d71dee07138e411674dd446a792de6cd2e22b400378cf2df4'
            ),
            (
                'public.starring_runtime_execution_database_readiness_v1()',
                'c5972296ea84090bae5708fc9efa90cd9f9f848acb156e40680c0ba04fb57b5c'
            ),
            (
                'public.reject_runtime_certification_reservation_mutation_v2()',
                'ff49c4ce2863940ca964444d9046caed23cf7db0cac97163e2ef73d7bd9c207b'
            ),
            (
                'public.starring_runtime_certification_reserve_intent_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint,bytea,text)',
                '4c088bff39108bccd0690c1a8cf395572c7e6f0b4380d4df5460c91398e5038d'
            ),
            (
                'public.starring_runtime_certification_reservation_observe_v2(text,text,text,bigint,bigint)',
                'a6443bcca0fab54523f1570c656da8792dd37a002bca71e0a5d6a53d34ebff39'
            ),
            (
                'starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint)',
                'a98d87b7aba288ca18c44c6c8419ad6092595c8a321da18b7d3d5f005a6a64e9'
            ),
            (
                'starring_runtime_private_v2.starring_runtime_certification_intent_fingerprint_v2(bytea)',
                '5e54a6b0fec4e3d68fb5d12d14fddd7afe13f1279d0d8ea1f0d7681d5037e13b'
            )
    ) AS expected(identity, definition_digest)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(function_row.oid),
                'UTF8'
            )),
            'hex'
        ) IS DISTINCT FROM expected.definition_digest;

    SELECT pg_catalog.count(*)
    INTO invalid_target_acl_count
    FROM (
        VALUES
            ('public.starring_runtime_certification_reserve_intent_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint,bytea,text)'),
            ('public.starring_runtime_certification_reservation_observe_v2(text,text,text,bigint,bigint)')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
        ) <> CASE WHEN executor_role IS NULL THEN 1 ELSE 2 END
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee NOT IN (common_owner, executor_role)
                OR privilege.grantor <> common_owner
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
        )
        OR (
            executor_role IS NOT NULL
            AND NOT pg_catalog.has_function_privilege(
                executor_role,
                function_row.oid,
                'EXECUTE'
            )
        );

    SELECT pg_catalog.count(*)
    INTO invalid_private_acl_count
    FROM (
        VALUES
            ('public.reject_runtime_certification_reservation_mutation_v2()'),
            ('starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint)'),
            ('starring_runtime_private_v2.starring_runtime_certification_intent_fingerprint_v2(bytea)')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
        ) <> 1
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
        OR (
            executor_role IS NOT NULL
            AND pg_catalog.has_function_privilege(
                executor_role,
                function_row.oid,
                'EXECUTE'
            )
        );

    SELECT pg_catalog.count(*)
    INTO invalid_relation_acl_count
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
            'public.runtime_certification_operations_v2'
        )
        AND (
            EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    relation.relacl,
                    pg_catalog.acldefault('r', relation.relowner)
                )) AS privilege
                WHERE privilege.grantee <> common_owner
                    OR privilege.grantor <> common_owner
                    OR privilege.privilege_type NOT IN (
                        'INSERT',
                        'SELECT',
                        'UPDATE',
                        'DELETE',
                        'TRUNCATE',
                        'REFERENCES',
                        'TRIGGER'
                    )
                    OR privilege.is_grantable
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.pg_attribute AS attribute
                CROSS JOIN LATERAL pg_catalog.aclexplode(attribute.attacl)
                    AS privilege
                WHERE attribute.attrelid = relation.oid
                    AND attribute.attnum > 0
                    AND NOT attribute.attisdropped
                    AND privilege.grantee <> common_owner
            )
            OR (
                executor_role IS NOT NULL
                AND (
                    pg_catalog.has_table_privilege(
                        executor_role,
                        relation.oid,
                        'SELECT'
                    )
                    OR pg_catalog.has_table_privilege(
                        executor_role,
                        relation.oid,
                        'INSERT'
                    )
                    OR pg_catalog.has_table_privilege(
                        executor_role,
                        relation.oid,
                        'UPDATE'
                    )
                    OR pg_catalog.has_table_privilege(
                        executor_role,
                        relation.oid,
                        'DELETE'
                    )
                    OR pg_catalog.has_table_privilege(
                        executor_role,
                        relation.oid,
                        'TRUNCATE'
                    )
                    OR pg_catalog.has_table_privilege(
                        executor_role,
                        relation.oid,
                        'REFERENCES'
                    )
                    OR pg_catalog.has_table_privilege(
                        executor_role,
                        relation.oid,
                        'TRIGGER'
                    )
                )
            )
        );

    SELECT pg_catalog.count(*)
    INTO invalid_trigger_count
    FROM (
        VALUES
            ('runtime_certification_operations_v2_reject_row_mutation', 31),
            ('runtime_certification_operations_v2_reject_truncate', 34)
    ) AS expected(trigger_name, trigger_type)
    LEFT JOIN pg_catalog.pg_trigger AS trigger_row
        ON trigger_row.tgrelid = pg_catalog.to_regclass(
            'public.runtime_certification_operations_v2'
        )
        AND trigger_row.tgname = expected.trigger_name
    WHERE trigger_row.oid IS NULL
        OR trigger_row.tgisinternal
        OR trigger_row.tgenabled <> 'O'
        OR trigger_row.tgtype <> expected.trigger_type
        OR trigger_row.tgfoid <> pg_catalog.to_regprocedure(
            'public.reject_runtime_certification_reservation_mutation_v2()'
        );

    WITH baseline AS (
        SELECT COALESCE(pg_catalog.string_agg(
            pg_catalog.concat_ws(
                ':',
                privilege.grantee::TEXT,
                privilege.privilege_type,
                privilege.is_grantable::TEXT,
                privilege.grantor::TEXT
            ),
            ',' ORDER BY privilege.grantee, privilege.privilege_type
        ) FILTER (WHERE privilege.grantee <> common_owner), '') AS external_acl
        FROM pg_catalog.pg_proc AS function_row
        LEFT JOIN LATERAL pg_catalog.aclexplode(COALESCE(
            function_row.proacl,
            pg_catalog.acldefault('f', function_row.proowner)
        )) AS privilege ON TRUE
        WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_database_identity_v1()'
        )
    ), observed AS (
        SELECT
            expected.identity,
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
                ('public.starring_runtime_execution_database_readiness_v1()'),
                ('public.starring_runtime_execution_database_identity_v1()'),
                ('public.starring_runtime_execution_claim_next_v1(text,bigint)'),
                ('public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)'),
                ('public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)'),
                ('public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)'),
                ('public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)'),
                ('public.starring_runtime_execution_recover_stale_live_v1()'),
                ('public.starring_runtime_observe_previous_serving_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,jsonb)'),
                ('public.starring_runtime_gateway_owner_observe_v1(text)'),
                ('public.starring_runtime_gateway_owner_acquire_v1(text,text,text,bigint)'),
                ('public.starring_runtime_gateway_owner_renew_v1(text,text,bigint,text,bigint,bigint)'),
                ('public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)'),
                ('public.starring_runtime_writer_fence_observe_v1()'),
                ('public.starring_runtime_product_drain_observe_v2(text,text,text,bigint,text,text)'),
                ('public.starring_runtime_certification_reserve_intent_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint,bytea,text)'),
                ('public.starring_runtime_certification_reservation_observe_v2(text,text,text,bigint,bigint)')
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
        OR observed.external_acl IS DISTINCT FROM baseline.external_acl;

    SELECT pg_catalog.count(*)
    INTO snapshot_mismatch_count
    FROM pg_temp.starring_runtime_certification_enable_snapshot AS snapshot
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = snapshot.function_oid
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> snapshot.function_owner
        OR (
            function_row.oid NOT IN (
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_certification_reserve_intent_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint,bytea,text)'
                ),
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_certification_reservation_observe_v2(text,text,text,bigint,bigint)'
                )
            )
            AND function_row.proacl IS DISTINCT FROM snapshot.function_acl
        );

    SELECT pg_catalog.count(*)
    INTO new_function_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    LEFT JOIN pg_temp.starring_runtime_certification_enable_snapshot AS snapshot
        ON snapshot.function_oid = function_row.oid
    WHERE function_row.oid >= 16384
        AND namespace.nspname NOT IN ('pg_catalog', 'information_schema')
        AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_'
        AND snapshot.function_oid IS NULL;

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
        OR invalid_function_count <> 0
        OR invalid_target_acl_count <> 0
        OR invalid_private_acl_count <> 0
        OR invalid_relation_acl_count <> 0
        OR invalid_trigger_count <> 0
        OR invalid_capability_acl_count <> 0
        OR snapshot_mismatch_count <> 0
        OR new_function_count <> 0
        OR (
            executor_role IS NOT NULL
            AND pg_catalog.has_schema_privilege(
                executor_role,
                'starring_runtime_private_v2',
                'USAGE'
            )
        )
        OR manifest_digest IS DISTINCT FROM
            'ff16060ff3ddcb6d71dee07138e411674dd446a792de6cd2e22b400378cf2df4'
        OR readiness_digest IS DISTINCT FROM
            'c5972296ea84090bae5708fc9efa90cd9f9f848acb156e40680c0ba04fb57b5c'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_enable_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
