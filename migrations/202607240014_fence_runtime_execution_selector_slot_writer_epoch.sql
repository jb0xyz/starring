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

CREATE TEMPORARY TABLE pg_temp.starring_runtime_execution_selector_epoch_snapshot (
    function_oid OID PRIMARY KEY,
    function_owner OID NOT NULL,
    function_acl ACLITEM[]
) ON COMMIT DROP;

INSERT INTO pg_temp.starring_runtime_execution_selector_epoch_snapshot (
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
            ('public.starring_runtime_execution_claim_next_v1(text,bigint)',
                'd1f07b5cbfb75468f37679567c2512b7f6d0555f31ee5f4f5353ad274e07aadd',
                'plpgsql',
                TRUE, TRUE, 1::REAL, TRUE),
            ('public.starring_runtime_execution_recover_stale_live_v1()',
                '506c532c275fbbe51b1e67d463bf0ddfc71a258e79a6594ab19ea2235c07fc6a',
                'plpgsql',
                TRUE, TRUE, 1::REAL, TRUE),
            ('public.starring_runtime_execution_schema_manifest_v1()',
                '0d0adb92217032ac62b996a0b3e6cb3cdb3ff99a0be983626aa5df4777c78bb7',
                'plpgsql',
                TRUE, FALSE, 0::REAL, TRUE),
            ('public.starring_runtime_execution_database_readiness_v1()',
                'b5362bc1b081789a5b3ac4881fc2ea00c340a013630f7d5c809958ed1c045ec3',
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
    FROM pg_temp.starring_runtime_execution_selector_epoch_snapshot AS snapshot
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
            MESSAGE = 'runtime_execution_selector_slot_writer_epoch_executor_not_quiesced';
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
            FROM pg_temp.starring_runtime_execution_selector_epoch_snapshot
        ) <> 4
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
            MESSAGE = 'runtime_execution_selector_slot_writer_epoch_preflight_drift';
    END IF;
END;
$preflight$;

CREATE OR REPLACE FUNCTION public.starring_runtime_execution_claim_next_v1(expected_controller_id text, requested_lease_milliseconds bigint)
 RETURNS TABLE(outcome_name text, previous_snapshot jsonb, snapshot jsonb, controller_id text, fencing_token bigint, previous_convergence_attempt_no bigint, convergence_attempt_no bigint, acquired_at timestamp with time zone, expires_at timestamp with time zone)
 LANGUAGE plpgsql
 STRICT SECURITY DEFINER ROWS 1
 SET search_path TO 'pg_catalog'
AS $function$
DECLARE
    deployment_row public.runtime_deployments%ROWTYPE;
    previous_snapshot_value JSONB;
    next_snapshot JSONB;
    authority_outcome TEXT;
    mutation_clock TIMESTAMPTZ;
    replay_lookup_clock TIMESTAMPTZ;
    replay_validation_clock TIMESTAMPTZ;
    requested_duration INTERVAL;
    next_revision BIGINT;
    next_fencing_token BIGINT;
    next_attempt BIGINT;
    next_expiry TIMESTAMPTZ;
    candidate_row public.runtime_deployments%ROWTYPE;
    writer_fence_state TEXT;
    slot_writer_epoch BIGINT;
    pending_drain_intent_id TEXT;
    candidate_found BOOLEAN := FALSE;
BEGIN
    PERFORM pg_catalog.set_config('TimeZone', 'UTC', TRUE);
    IF expected_controller_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR requested_lease_milliseconds NOT BETWEEN 1000 AND 600000
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_execution_claim_input_invalid';
    END IF;

    SELECT fence.fence_state
    INTO writer_fence_state
    FROM public.starring_runtime_writer_fence_observe_v1() AS fence;

    IF NOT FOUND
        OR writer_fence_state NOT IN ('open', 'closed')
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_execution_writer_fence_invalid';
    END IF;

    IF writer_fence_state = 'closed' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX005',
            MESSAGE = 'runtime_execution_writer_fenced';
    END IF;

    requested_duration :=
        requested_lease_milliseconds * INTERVAL '1 millisecond';

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            pg_catalog.concat(
                'starring-runtime-execution-controller-v1:',
                expected_controller_id
            ),
            0
        )
    );
    replay_lookup_clock := pg_catalog.clock_timestamp();

    SELECT deployment.*
    INTO candidate_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.controller_id = expected_controller_id
        AND deployment.controller_lease_expires_at
            > replay_lookup_clock
        AND deployment.phase NOT IN ('live', 'superseded', 'cancelled')
    ORDER BY deployment.controller_acquired_at, deployment.deployment_id
    LIMIT 1;

    IF FOUND THEN
        IF candidate_row.guild_id !~ '^[1-9][0-9]{0,19}$'
            OR pg_catalog.length(candidate_row.guild_id) > 20
            OR (
                pg_catalog.length(candidate_row.guild_id) = 20
                AND candidate_row.guild_id COLLATE pg_catalog."C"
                    > '18446744073709551615' COLLATE pg_catalog."C"
            )
            OR candidate_row.ruleset_key !~ '^[A-Za-z0-9_-]{1,64}$'
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_execution_claim_state_invalid';
        END IF;

        BEGIN
            PERFORM pg_catalog.pg_advisory_xact_lock(
                pg_catalog.hashtextextended(
                    pg_catalog.concat(
                        'starring-runtime-serving-slot-v1:',
                        candidate_row.guild_id,
                        ':',
                        candidate_row.ruleset_key
                    ),
                    0
                )
            );

            SELECT slot_fence.writer_epoch
            INTO slot_writer_epoch
            FROM starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(
                candidate_row.guild_id,
                candidate_row.ruleset_key
            ) AS slot_fence;

            SELECT deployment.*
            INTO deployment_row
            FROM public.runtime_deployments AS deployment
            WHERE deployment.tenant_id = candidate_row.tenant_id
                AND deployment.installation_id
                    = candidate_row.installation_id
                AND deployment.deployment_id = candidate_row.deployment_id
                AND deployment.revision = candidate_row.revision
                AND deployment.guild_id = candidate_row.guild_id
                AND deployment.ruleset_key = candidate_row.ruleset_key
                AND deployment IS NOT DISTINCT FROM candidate_row
            FOR UPDATE;

            IF NOT FOUND THEN
                RAISE no_data_found;
            END IF;

            replay_validation_clock := GREATEST(
            pg_catalog.clock_timestamp(),
            replay_lookup_clock
        );
        IF EXISTS (
            SELECT 1
            FROM public.runtime_deployments AS duplicate
            WHERE duplicate.controller_id = expected_controller_id
                AND duplicate.controller_lease_expires_at
                    > replay_validation_clock
                AND duplicate.phase NOT IN ('live', 'superseded', 'cancelled')
                AND duplicate.deployment_id <> deployment_row.deployment_id
        ) THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_execution_claim_controller_ambiguous';
        END IF;

        mutation_clock := replay_validation_clock;
        IF deployment_row.controller_lease_expires_at > mutation_clock THEN
            IF deployment_row.last_controller_id
                    IS DISTINCT FROM expected_controller_id
                OR deployment_row.controller_fencing_token IS NULL
                OR deployment_row.controller_fencing_token
                    IS DISTINCT FROM deployment_row.last_fencing_token
                OR deployment_row.controller_acquired_at IS NULL
                OR deployment_row.controller_lease_expires_at
                    - deployment_row.controller_acquired_at
                    IS DISTINCT FROM requested_duration
                OR deployment_row.convergence_attempt_no
                    NOT BETWEEN 1 AND 4294967295
                OR deployment_row.snapshot #>> '{controller_lease,controller_id}'
                    IS DISTINCT FROM expected_controller_id
                OR deployment_row.snapshot #>> '{controller_lease,fencing_token}'
                    IS DISTINCT FROM deployment_row.controller_fencing_token::TEXT
                OR (deployment_row.snapshot
                        #>> '{controller_lease,acquired_at}')::TIMESTAMPTZ
                    IS DISTINCT FROM deployment_row.controller_acquired_at
                OR (deployment_row.snapshot
                        #>> '{controller_lease,expires_at}')::TIMESTAMPTZ
                    IS DISTINCT FROM deployment_row.controller_lease_expires_at
                OR deployment_row.snapshot ->> 'revision'
                    IS DISTINCT FROM deployment_row.revision::TEXT
                OR deployment_row.snapshot ->> 'last_fencing_token'
                    IS DISTINCT FROM deployment_row.last_fencing_token::TEXT
            THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RX004',
                    MESSAGE = 'runtime_execution_claim_replay_mismatch';
            END IF;

            authority_outcome := public.starring_runtime_lock_current_authority(
                deployment_row.activation_request_id,
                deployment_row.promotion_id,
                deployment_row.tenant_id,
                deployment_row.installation_id,
                deployment_row.installation_authority_revision,
                deployment_row.guild_id,
                deployment_row.ruleset_key,
                deployment_row.target_version,
                deployment_row.target_content_hash,
                deployment_row.binding_revision,
                deployment_row.binding_fingerprint
            );
            IF authority_outcome = 'active_mismatch' THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RX006',
                    MESSAGE = 'runtime_execution_claim_target_superseded';
            ELSIF authority_outcome IS DISTINCT FROM 'exact' THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RX003',
                    MESSAGE = 'runtime_execution_claim_authority_changed';
            END IF;

            replay_validation_clock := GREATEST(
                pg_catalog.clock_timestamp(),
                replay_validation_clock
            );
            IF deployment_row.controller_lease_expires_at
                > replay_validation_clock
            THEN
                outcome_name := 'replayed';
                previous_snapshot := deployment_row.snapshot;
                snapshot := deployment_row.snapshot;
                controller_id := deployment_row.controller_id;
                fencing_token := deployment_row.controller_fencing_token;
                previous_convergence_attempt_no :=
                    deployment_row.convergence_attempt_no - 1;
                convergence_attempt_no :=
                    deployment_row.convergence_attempt_no;
                acquired_at := deployment_row.controller_acquired_at;
                expires_at := deployment_row.controller_lease_expires_at;
                RETURN NEXT;
                RETURN;
            END IF;
        END IF;
        RAISE no_data_found;
        EXCEPTION
            WHEN no_data_found THEN
                candidate_found := FALSE;
        END;
    END IF;

    FOR candidate_row IN
    SELECT deployment.*
    FROM public.runtime_deployments AS deployment
    JOIN public.activation_requests AS activation
        ON activation.id = deployment.activation_request_id
        AND activation.state = 'applied'
        AND activation.authority_kind = 'product_authoring'
        AND activation.link_state_name = 'linked'
        AND activation.promotion_id = deployment.promotion_id
    JOIN public.authoring_promotions AS promotion
        ON promotion.id = deployment.promotion_id
        AND promotion.stage = 'activation_pending'
        AND promotion.tenant_id = deployment.tenant_id
    JOIN public.product_tenants AS tenant
        ON tenant.tenant_id = deployment.tenant_id
        AND tenant.lifecycle_state = 'active'
    JOIN public.automation_installations AS installation
        ON installation.tenant_id = deployment.tenant_id
        AND installation.installation_id = deployment.installation_id
        AND installation.lifecycle_state = 'active'
    JOIN public.automation_installation_authority_versions
        AS historical_authority
        ON historical_authority.tenant_id = installation.tenant_id
        AND historical_authority.installation_id
            = installation.installation_id
        AND historical_authority.revision
            = deployment.installation_authority_revision
        AND historical_authority.binding_revision
            = deployment.binding_revision
        AND historical_authority.binding_fingerprint
            = deployment.binding_fingerprint
    JOIN public.automation_installation_authority_versions
        AS current_authority
        ON current_authority.tenant_id = installation.tenant_id
        AND current_authority.installation_id
            = installation.installation_id
        AND current_authority.revision
            = installation.current_authority_revision
        AND current_authority.binding_revision
            = deployment.binding_revision
        AND current_authority.binding_fingerprint
            = deployment.binding_fingerprint
        AND current_authority.resource_bindings
            IS NOT DISTINCT FROM historical_authority.resource_bindings
    JOIN public.automation_ruleset_activations AS active
        ON active.guild_id = deployment.guild_id
        AND active.ruleset_key = deployment.ruleset_key
        AND active.active_version = deployment.target_version
    JOIN public.automation_ruleset_versions AS version
        ON version.guild_id = active.guild_id
        AND version.ruleset_key = active.ruleset_key
        AND version.version = active.active_version
        AND version.content_hash = deployment.target_content_hash
        AND version.canonical_content_hash = version.content_hash
        AND version.schema_version = 1
    LEFT JOIN public.runtime_slot_writer_fences_v2 AS slot_fence
        ON slot_fence.slot_guild_id = deployment.guild_id
        AND slot_fence.slot_ruleset_key = deployment.ruleset_key
    LEFT JOIN public.runtime_drain_intents_v2 AS pending_drain
        ON pending_drain.slot_guild_id = deployment.guild_id
        AND pending_drain.slot_ruleset_key = deployment.ruleset_key
        AND pending_drain.intent_state = 'pending'
    WHERE deployment.phase NOT IN ('live', 'superseded', 'cancelled')
        AND slot_fence.pending_drain_intent_id IS NULL
        AND pending_drain.drain_intent_id IS NULL
        AND deployment.blocked_at IS NULL
        AND (
            deployment.next_retry_at IS NULL
            OR deployment.next_retry_at <= pg_catalog.clock_timestamp()
        )
        AND (
            deployment.controller_lease_expires_at IS NULL
            OR deployment.controller_lease_expires_at
                <= pg_catalog.clock_timestamp()
        )
        AND promotion.record #>> '{intent,authority,tenant_id}'
            = deployment.tenant_id
        AND promotion.record #>> '{intent,authority,installation_id}'
            = deployment.installation_id
        AND promotion.record #>> '{intent,authority,guild_id}'
            = deployment.guild_id
        AND promotion.record #>> '{intent,authority,ruleset_key}'
            = deployment.ruleset_key
        AND promotion.record #>> '{intent,authority,binding_revision}'
            = deployment.binding_revision::TEXT
        AND promotion.record #>> '{intent,evidence,context_fingerprint}'
            = deployment.binding_fingerprint
        AND promotion.record #>> '{stage,activation,request_id}'
            = deployment.activation_request_id
        AND promotion.record #>> '{stage,activation,target,guild_id}'
            = deployment.guild_id
        AND promotion.record #>> '{stage,activation,target,ruleset_key}'
            = deployment.ruleset_key
        AND promotion.record #>> '{stage,activation,target,version}'
            = deployment.target_version::TEXT
        AND promotion.record #>> '{stage,activation,target,content_hash}'
            = deployment.target_content_hash
    ORDER BY
        COALESCE(deployment.next_retry_at, deployment.requested_at),
        deployment.requested_at,
        deployment.deployment_id
    LIMIT 64
    LOOP
        IF candidate_row.guild_id !~ '^[1-9][0-9]{0,19}$'
            OR pg_catalog.length(candidate_row.guild_id) > 20
            OR (
                pg_catalog.length(candidate_row.guild_id) = 20
                AND candidate_row.guild_id COLLATE pg_catalog."C"
                    > '18446744073709551615' COLLATE pg_catalog."C"
            )
            OR candidate_row.ruleset_key !~ '^[A-Za-z0-9_-]{1,64}$'
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_execution_claim_state_invalid';
        END IF;

        candidate_found := FALSE;
        BEGIN
            IF NOT pg_catalog.pg_try_advisory_xact_lock(
                pg_catalog.hashtextextended(
                    pg_catalog.concat(
                        'starring-runtime-serving-slot-v1:',
                        candidate_row.guild_id,
                        ':',
                        candidate_row.ruleset_key
                    ),
                    0
                )
            )
            THEN
                RAISE no_data_found;
            END IF;

            SELECT
                slot_fence.writer_epoch,
                slot_fence.pending_drain_intent_id
            INTO slot_writer_epoch, pending_drain_intent_id
            FROM starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(
                candidate_row.guild_id,
                candidate_row.ruleset_key
            ) AS slot_fence;

            IF pending_drain_intent_id IS NOT NULL THEN
                RAISE no_data_found;
            END IF;

            SELECT deployment.*
            INTO deployment_row
            FROM public.runtime_deployments AS deployment
            WHERE deployment.tenant_id = candidate_row.tenant_id
                AND deployment.installation_id
                    = candidate_row.installation_id
                AND deployment.deployment_id = candidate_row.deployment_id
                AND deployment.revision = candidate_row.revision
                AND deployment.guild_id = candidate_row.guild_id
                AND deployment.ruleset_key = candidate_row.ruleset_key
                AND deployment IS NOT DISTINCT FROM candidate_row
            FOR UPDATE SKIP LOCKED;

            IF NOT FOUND THEN
                RAISE no_data_found;
            END IF;
            candidate_found := TRUE;
        EXCEPTION
            WHEN no_data_found THEN
                candidate_found := FALSE;
        END;

        IF candidate_found THEN
            EXIT;
        END IF;
    END LOOP;

    IF NOT candidate_found THEN
        RETURN;
    END IF;

    authority_outcome := public.starring_runtime_lock_current_authority(
        deployment_row.activation_request_id,
        deployment_row.promotion_id,
        deployment_row.tenant_id,
        deployment_row.installation_id,
        deployment_row.installation_authority_revision,
        deployment_row.guild_id,
        deployment_row.ruleset_key,
        deployment_row.target_version,
        deployment_row.target_content_hash,
        deployment_row.binding_revision,
        deployment_row.binding_fingerprint
    );

    IF authority_outcome = 'active_mismatch' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX006',
            MESSAGE = 'runtime_execution_claim_target_superseded';
    ELSIF authority_outcome IS DISTINCT FROM 'exact' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_execution_claim_authority_changed';
    END IF;

    IF deployment_row.revision = 9223372036854775807
        OR COALESCE(deployment_row.last_fencing_token, 0)
            = 9223372036854775807
        OR deployment_row.convergence_attempt_no
            NOT BETWEEN 0 AND 4294967294
        OR deployment_row.snapshot ->> 'revision'
            IS DISTINCT FROM deployment_row.revision::TEXT
        OR deployment_row.snapshot ->> 'runtime_generation'
            IS DISTINCT FROM deployment_row.runtime_generation::TEXT
        OR deployment_row.snapshot #>> '{identity,deployment_id}'
            IS DISTINCT FROM deployment_row.deployment_id
        OR deployment_row.snapshot #>> '{identity,tenant_id}'
            IS DISTINCT FROM deployment_row.tenant_id
        OR deployment_row.snapshot #>> '{identity,installation_id}'
            IS DISTINCT FROM deployment_row.installation_id
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_execution_claim_state_invalid';
    END IF;

    mutation_clock := public.starring_runtime_mutation_clock();
    IF deployment_row.next_retry_at IS NOT NULL
        AND deployment_row.next_retry_at > mutation_clock
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX005',
            MESSAGE = 'runtime_execution_claim_retry_not_ready';
    END IF;
    IF deployment_row.phase = 'runtime_pending'
        AND deployment_row.snapshot #>> '{phase,condition,condition}'
            = 'blocked'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_execution_claim_operator_action_required';
    END IF;

    previous_snapshot_value := deployment_row.snapshot;
    next_revision := deployment_row.revision + 1;
    next_fencing_token :=
        COALESCE(deployment_row.last_fencing_token, 0) + 1;
    next_attempt := deployment_row.convergence_attempt_no + 1;
    next_expiry := mutation_clock + requested_duration;

    next_snapshot := pg_catalog.jsonb_set(
        deployment_row.snapshot,
        '{revision}',
        pg_catalog.to_jsonb(next_revision),
        FALSE
    );
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{controller_lease}',
        pg_catalog.jsonb_build_object(
            'controller_id', expected_controller_id,
            'fencing_token', next_fencing_token,
            'acquired_at', mutation_clock,
            'expires_at', next_expiry
        ),
        FALSE
    );
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{last_fencing_token}',
        pg_catalog.to_jsonb(next_fencing_token),
        FALSE
    );

    PERFORM starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(
        deployment_row.guild_id,
        deployment_row.ruleset_key,
        slot_writer_epoch
    );

    UPDATE public.runtime_deployments AS deployment
    SET snapshot = next_snapshot,
        revision = next_revision,
        controller_id = expected_controller_id,
        controller_fencing_token = next_fencing_token,
        controller_acquired_at = mutation_clock,
        controller_lease_expires_at = next_expiry,
        last_fencing_token = next_fencing_token,
        last_controller_id = expected_controller_id,
        convergence_attempt_no = next_attempt,
        updated_at = GREATEST(
            mutation_clock,
            deployment.updated_at + INTERVAL '1 microsecond'
        )
    WHERE deployment.deployment_id = deployment_row.deployment_id
        AND deployment.revision = deployment_row.revision;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_claim_ownership_lost';
    END IF;

    outcome_name := 'applied';
    previous_snapshot := previous_snapshot_value;
    snapshot := next_snapshot;
    controller_id := expected_controller_id;
    fencing_token := next_fencing_token;
    previous_convergence_attempt_no :=
        deployment_row.convergence_attempt_no;
    convergence_attempt_no := next_attempt;
    acquired_at := mutation_clock;
    expires_at := next_expiry;
    RETURN NEXT;
END;
$function$;

CREATE OR REPLACE FUNCTION public.starring_runtime_execution_recover_stale_live_v1()
 RETURNS TABLE(outcome_name text, observed_snapshot jsonb, deployment_snapshot jsonb, convergence_attempt_no bigint, loss_kind text, evidence_at timestamp with time zone, recovered_at timestamp with time zone)
 LANGUAGE plpgsql
 STRICT SECURITY DEFINER ROWS 1
 SET search_path TO 'pg_catalog'
AS $function$
DECLARE
    deployment_row public.runtime_deployments%ROWTYPE;
    serving_row public.runtime_serving_leases%ROWTYPE;
    previous_snapshot JSONB;
    next_snapshot JSONB;
    recovery_value JSONB;
    authority_outcome TEXT;
    mutation_clock TIMESTAMPTZ;
    recovery_kind TEXT;
    recovery_evidence TIMESTAMPTZ;
    next_revision BIGINT;
    serving_found BOOLEAN;
    writer_fence_state TEXT;
    candidate_row public.runtime_deployments%ROWTYPE;
    candidate_found BOOLEAN := FALSE;
    slot_writer_epoch BIGINT;
    pending_drain_intent_id TEXT;
BEGIN
    PERFORM pg_catalog.set_config('TimeZone', 'UTC', TRUE);

    SELECT fence.fence_state
    INTO writer_fence_state
    FROM public.starring_runtime_writer_fence_observe_v1() AS fence;

    IF NOT FOUND
        OR writer_fence_state NOT IN ('open', 'closed')
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_execution_writer_fence_invalid';
    END IF;

    IF writer_fence_state = 'closed' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX005',
            MESSAGE = 'runtime_execution_writer_fenced';
    END IF;

    FOR candidate_row IN
    SELECT deployment.*
    FROM public.runtime_deployments AS deployment
    JOIN public.runtime_serving_leases AS serving_lease
        ON serving_lease.guild_id = deployment.guild_id
        AND serving_lease.ruleset_key = deployment.ruleset_key
        AND serving_lease.tenant_id = deployment.tenant_id
        AND serving_lease.installation_id = deployment.installation_id
        AND serving_lease.deployment_id = deployment.deployment_id
        AND serving_lease.attestation_id = deployment.live_attestation_id
    JOIN public.activation_requests AS activation
        ON activation.id = deployment.activation_request_id
        AND activation.state = 'applied'
        AND activation.authority_kind = 'product_authoring'
        AND activation.link_state_name = 'linked'
        AND activation.promotion_id = deployment.promotion_id
    JOIN public.authoring_promotions AS promotion
        ON promotion.id = deployment.promotion_id
        AND promotion.stage = 'activation_pending'
        AND promotion.tenant_id = deployment.tenant_id
    JOIN public.product_tenants AS tenant
        ON tenant.tenant_id = deployment.tenant_id
        AND tenant.lifecycle_state = 'active'
    JOIN public.automation_installations AS installation
        ON installation.tenant_id = deployment.tenant_id
        AND installation.installation_id = deployment.installation_id
        AND installation.lifecycle_state = 'active'
    JOIN public.automation_installation_authority_versions
        AS historical_authority
        ON historical_authority.tenant_id = installation.tenant_id
        AND historical_authority.installation_id
            = installation.installation_id
        AND historical_authority.revision
            = deployment.installation_authority_revision
        AND historical_authority.binding_revision
            = deployment.binding_revision
        AND historical_authority.binding_fingerprint
            = deployment.binding_fingerprint
    JOIN public.automation_installation_authority_versions
        AS current_authority
        ON current_authority.tenant_id = installation.tenant_id
        AND current_authority.installation_id
            = installation.installation_id
        AND current_authority.revision
            = installation.current_authority_revision
        AND current_authority.binding_revision
            = deployment.binding_revision
        AND current_authority.binding_fingerprint
            = deployment.binding_fingerprint
        AND current_authority.resource_bindings
            IS NOT DISTINCT FROM historical_authority.resource_bindings
    JOIN public.automation_ruleset_activations AS active
        ON active.guild_id = deployment.guild_id
        AND active.ruleset_key = deployment.ruleset_key
        AND active.active_version = deployment.target_version
    JOIN public.automation_ruleset_versions AS version
        ON version.guild_id = active.guild_id
        AND version.ruleset_key = active.ruleset_key
        AND version.version = active.active_version
        AND version.content_hash = deployment.target_content_hash
        AND version.canonical_content_hash = version.content_hash
        AND version.schema_version = 1
    LEFT JOIN public.runtime_slot_writer_fences_v2 AS slot_fence
        ON slot_fence.slot_guild_id = deployment.guild_id
        AND slot_fence.slot_ruleset_key = deployment.ruleset_key
    LEFT JOIN public.runtime_drain_intents_v2 AS pending_drain
        ON pending_drain.slot_guild_id = deployment.guild_id
        AND pending_drain.slot_ruleset_key = deployment.ruleset_key
        AND pending_drain.intent_state = 'pending'
    WHERE deployment.phase = 'live'
        AND slot_fence.pending_drain_intent_id IS NULL
        AND pending_drain.drain_intent_id IS NULL
        AND deployment.revision < 9223372036854775807
        AND deployment.convergence_attempt_no BETWEEN 1 AND 4294967295
        AND serving_lease.process_instance_id
            = deployment.snapshot #>> '{live,process_instance_id}'
        AND serving_lease.runtime_generation
            = deployment.runtime_generation
        AND (
            NOT serving_lease.connected
            OR NOT serving_lease.serving
            OR serving_lease.expires_at <= pg_catalog.clock_timestamp()
        )
        AND promotion.record #>> '{intent,authority,tenant_id}'
            = deployment.tenant_id
        AND promotion.record #>> '{intent,authority,installation_id}'
            = deployment.installation_id
        AND promotion.record #>> '{intent,authority,guild_id}'
            = deployment.guild_id
        AND promotion.record #>> '{intent,authority,ruleset_key}'
            = deployment.ruleset_key
        AND promotion.record #>> '{intent,authority,binding_revision}'
            = deployment.binding_revision::TEXT
        AND promotion.record #>> '{intent,evidence,context_fingerprint}'
            = deployment.binding_fingerprint
        AND NOT EXISTS (
            SELECT 1
            FROM public.runtime_deployments AS newer
            WHERE newer.guild_id = deployment.guild_id
                AND newer.ruleset_key = deployment.ruleset_key
                AND newer.deployment_id <> deployment.deployment_id
                AND newer.phase NOT IN ('live', 'superseded', 'cancelled')
        )
    ORDER BY
        serving_lease.expires_at,
        deployment.updated_at,
        deployment.deployment_id
    LIMIT 64
    LOOP
        IF candidate_row.guild_id !~ '^[1-9][0-9]{0,19}$'
            OR pg_catalog.length(candidate_row.guild_id) > 20
            OR (
                pg_catalog.length(candidate_row.guild_id) = 20
                AND candidate_row.guild_id COLLATE pg_catalog."C"
                    > '18446744073709551615' COLLATE pg_catalog."C"
            )
            OR candidate_row.ruleset_key !~ '^[A-Za-z0-9_-]{1,64}$'
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_execution_recover_state_invalid';
        END IF;

        BEGIN
            IF NOT pg_catalog.pg_try_advisory_xact_lock(
                pg_catalog.hashtextextended(
                    pg_catalog.concat(
                        'starring-runtime-serving-slot-v1:',
                        candidate_row.guild_id,
                        ':',
                        candidate_row.ruleset_key
                    ),
                    0
                )
            )
            THEN
                RAISE no_data_found;
            END IF;

            SELECT
                slot_fence.writer_epoch,
                slot_fence.pending_drain_intent_id
            INTO slot_writer_epoch, pending_drain_intent_id
            FROM starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(
                candidate_row.guild_id,
                candidate_row.ruleset_key
            ) AS slot_fence;

            IF pending_drain_intent_id IS NOT NULL THEN
                RAISE no_data_found;
            END IF;

            SELECT deployment.*
            INTO deployment_row
            FROM public.runtime_deployments AS deployment
            WHERE deployment.tenant_id = candidate_row.tenant_id
                AND deployment.installation_id
                    = candidate_row.installation_id
                AND deployment.deployment_id = candidate_row.deployment_id
                AND deployment.revision = candidate_row.revision
                AND deployment.guild_id = candidate_row.guild_id
                AND deployment.ruleset_key = candidate_row.ruleset_key
                AND deployment IS NOT DISTINCT FROM candidate_row
            FOR UPDATE SKIP LOCKED;

            IF NOT FOUND THEN
                RAISE no_data_found;
            END IF;
            candidate_found := TRUE;
        EXCEPTION
            WHEN no_data_found THEN
                candidate_found := FALSE;
        END;

        IF candidate_found THEN
            EXIT;
        END IF;
    END LOOP;

    IF NOT candidate_found THEN
        RETURN;
    END IF;

    authority_outcome := public.starring_runtime_lock_current_authority(
        deployment_row.activation_request_id,
        deployment_row.promotion_id,
        deployment_row.tenant_id,
        deployment_row.installation_id,
        deployment_row.installation_authority_revision,
        deployment_row.guild_id,
        deployment_row.ruleset_key,
        deployment_row.target_version,
        deployment_row.target_content_hash,
        deployment_row.binding_revision,
        deployment_row.binding_fingerprint
    );
    IF authority_outcome = 'active_mismatch' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX006',
            MESSAGE = 'runtime_execution_recover_target_superseded';
    ELSIF authority_outcome IS DISTINCT FROM 'exact' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_execution_recover_authority_changed';
    END IF;

    SELECT lease.*
    INTO serving_row
    FROM public.runtime_serving_leases AS lease
    WHERE lease.guild_id = deployment_row.guild_id
        AND lease.ruleset_key = deployment_row.ruleset_key
    FOR UPDATE;
    serving_found := FOUND;

    mutation_clock := public.starring_runtime_mutation_clock();
    IF NOT serving_found
        OR serving_row.tenant_id
            IS DISTINCT FROM deployment_row.tenant_id
        OR serving_row.installation_id
            IS DISTINCT FROM deployment_row.installation_id
        OR serving_row.deployment_id
            IS DISTINCT FROM deployment_row.deployment_id
        OR serving_row.attestation_id
            IS DISTINCT FROM deployment_row.live_attestation_id
        OR serving_row.process_instance_id
            IS DISTINCT FROM deployment_row.snapshot
                #>> '{live,process_instance_id}'
        OR serving_row.runtime_generation
            IS DISTINCT FROM deployment_row.runtime_generation
        OR serving_row.guild_id IS DISTINCT FROM deployment_row.guild_id
        OR serving_row.ruleset_key
            IS DISTINCT FROM deployment_row.ruleset_key
        OR serving_row.target_version
            IS DISTINCT FROM deployment_row.target_version
        OR serving_row.target_content_hash
            IS DISTINCT FROM deployment_row.target_content_hash
        OR serving_row.binding_revision
            IS DISTINCT FROM deployment_row.binding_revision
        OR serving_row.binding_fingerprint
            IS DISTINCT FROM deployment_row.binding_fingerprint
        OR serving_row.acquired_at > serving_row.last_heartbeat_at
        OR serving_row.last_heartbeat_at > serving_row.expires_at
        OR serving_row.acquired_at > mutation_clock
        OR serving_row.serving IS DISTINCT FROM serving_row.connected
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_execution_recover_state_invalid';
    END IF;

    IF NOT serving_row.connected AND NOT serving_row.serving
        AND serving_row.last_heartbeat_at = serving_row.expires_at
        AND serving_row.expires_at <= mutation_clock
    THEN
        recovery_kind := 'serving_disconnected';
        recovery_evidence := serving_row.last_heartbeat_at;
    ELSIF serving_row.connected AND serving_row.serving
        AND serving_row.last_heartbeat_at < serving_row.expires_at
        AND serving_row.expires_at <= mutation_clock
    THEN
        recovery_kind := 'serving_lease_expired';
        recovery_evidence := serving_row.expires_at;
    ELSE
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_recover_serving_active';
    END IF;

    IF recovery_evidence
            < (deployment_row.snapshot
                #>> '{live,certified_at}')::TIMESTAMPTZ
        OR EXISTS (
            SELECT 1
            FROM public.runtime_deployments AS newer
            WHERE newer.guild_id = deployment_row.guild_id
                AND newer.ruleset_key = deployment_row.ruleset_key
                AND newer.deployment_id <> deployment_row.deployment_id
                AND newer.phase NOT IN ('live', 'superseded', 'cancelled')
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_recover_ownership_lost';
    END IF;

    previous_snapshot := deployment_row.snapshot;
    next_revision := deployment_row.revision + 1;
    recovery_value := pg_catalog.jsonb_build_object(
        'prior_live', deployment_row.snapshot -> 'live',
        'kind', recovery_kind,
        'evidence_at', recovery_evidence,
        'recovered_at', mutation_clock
    );
    next_snapshot := pg_catalog.jsonb_set(
        deployment_row.snapshot,
        '{revision}',
        pg_catalog.to_jsonb(next_revision),
        FALSE
    );
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{phase}',
        '{"phase":"runtime_pending","condition":{"condition":"ready"}}'::JSONB,
        FALSE
    );
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{panel_certificate}',
        'null'::JSONB,
        FALSE
    );
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{gateway_ready}',
        'null'::JSONB,
        FALSE
    );
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{live}',
        'null'::JSONB,
        FALSE
    );
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{last_live_recovery}',
        recovery_value,
        FALSE
    );

    PERFORM starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(
        deployment_row.guild_id,
        deployment_row.ruleset_key,
        slot_writer_epoch
    );

    UPDATE public.runtime_deployments AS deployment
    SET snapshot = next_snapshot,
        revision = next_revision,
        phase = 'runtime_pending',
        live_attestation_id = NULL,
        live_at = NULL,
        updated_at = GREATEST(
            mutation_clock,
            deployment.updated_at + INTERVAL '1 microsecond'
        )
    WHERE deployment.tenant_id = deployment_row.tenant_id
        AND deployment.installation_id = deployment_row.installation_id
        AND deployment.deployment_id = deployment_row.deployment_id
        AND deployment.revision = deployment_row.revision;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_execution_recover_ownership_lost';
    END IF;

    outcome_name := 'applied';
    observed_snapshot := previous_snapshot;
    deployment_snapshot := next_snapshot;
    convergence_attempt_no := deployment_row.convergence_attempt_no;
    loss_kind := recovery_kind;
    evidence_at := recovery_evidence;
    recovered_at := mutation_clock;
    RETURN NEXT;
END;
$function$;

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
        '            = ''00e12af28c93ce77f62c4e1335aa3de88431bb22096bd85b86038dd555dccd13'';';
    next_fragment :=
        '    RETURN observed_count = 623' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''e9af146803f79bf195250ac230a9c39d7eef4f29349ac08a9d1c3914187fd3f2'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_selector_slot_writer_epoch_manifest_drift';
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
        '''0d0adb92217032ac62b996a0b3e6cb3cdb3ff99a0be983626aa5df4777c78bb7''::TEXT';
    next_fragment :=
        '''3c97b3b41f45b11ed2b01890c3d708806d802593f71589031cb921dfc5c65fe3''::TEXT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_selector_slot_writer_epoch_readiness_drift';
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
    global_position INTEGER;
    controller_position INTEGER;
    slot_position INTEGER;
    physical_position INTEGER;
    deployment_lock_position INTEGER;
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
            ('public.starring_runtime_execution_claim_next_v1(text,bigint)',
                '7cb6550864ed68e136e6e6b48c8cce59d179d895e3919a6abca77b7dfc7a4990',
                TRUE, TRUE, 1::REAL),
            ('public.starring_runtime_execution_recover_stale_live_v1()',
                '635aab9493e1fd2ad8a138633d6447f88752589414a27c2bad4e56afdd22f932',
                TRUE, TRUE, 1::REAL),
            ('public.starring_runtime_execution_schema_manifest_v1()',
                '3c97b3b41f45b11ed2b01890c3d708806d802593f71589031cb921dfc5c65fe3',
                TRUE, FALSE, 0::REAL),
            ('public.starring_runtime_execution_database_readiness_v1()',
                'c5a1eb3ae9a229c127a804f6f05298ff9f797604646de202ba1a832012e7bd91',
                TRUE, TRUE, 1::REAL),
            ('starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(text,text)',
                '68708aa143de1daae1247b18f3127e2abdc6d269a14e103d24e5ab6732d23f99',
                FALSE, TRUE, 1::REAL),
            ('starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(text,text,bigint)',
                'da6c88ff80cf366e14f2c12a6204964d708156192a292cc6ad71b959588f07b8',
                FALSE, FALSE, 0::REAL)
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
        ) IS DISTINCT FROM contract.definition_digest
        OR (
            NOT contract.security_definer
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
    FROM pg_temp.starring_runtime_execution_selector_epoch_snapshot AS snapshot
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

    global_position := pg_catalog.strpos(
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
    replay_position := pg_catalog.strpos(
        claim_source,
        'outcome_name := ''replayed'''
    );
    begin_position := pg_catalog.strpos(
        claim_source,
        'starring_runtime_slot_writer_fence_begin_unsafe_v2'
    );
    mutation_position := pg_catalog.strpos(
        claim_source,
        'UPDATE public.runtime_deployments AS deployment'
    );

    IF global_position = 0
        OR controller_position = 0
        OR slot_position = 0
        OR physical_position = 0
        OR deployment_lock_position = 0
        OR replay_position = 0
        OR begin_position = 0
        OR mutation_position = 0
        OR NOT (
            global_position < controller_position
            AND controller_position < slot_position
            AND slot_position < physical_position
            AND physical_position < deployment_lock_position
            AND replay_position < begin_position
            AND begin_position < mutation_position
        )
        OR (
            pg_catalog.length(claim_source)
            - pg_catalog.length(pg_catalog.replace(
                claim_source,
                'starring_runtime_slot_writer_fence_lock_v2',
                ''
            ))
        ) <> 2 * pg_catalog.length(
            'starring_runtime_slot_writer_fence_lock_v2'
        )
        OR (
            pg_catalog.length(claim_source)
            - pg_catalog.length(pg_catalog.replace(
                claim_source,
                'starring_runtime_slot_writer_fence_begin_unsafe_v2',
                ''
            ))
        ) <> pg_catalog.length(
            'starring_runtime_slot_writer_fence_begin_unsafe_v2'
        )
        OR (
            pg_catalog.length(claim_source)
            - pg_catalog.length(pg_catalog.replace(
                claim_source,
                'WHEN no_data_found THEN',
                ''
            ))
        ) <> 2 * pg_catalog.length('WHEN no_data_found THEN')
        OR (
            pg_catalog.length(claim_source)
            - pg_catalog.length(pg_catalog.replace(
                claim_source,
                'LIMIT 64',
                ''
            ))
        ) <> pg_catalog.length('LIMIT 64')
        OR pg_catalog.strpos(
            claim_source,
            'LEFT JOIN public.runtime_drain_intents_v2 AS pending_drain'
        ) = 0
        OR pg_catalog.strpos(
            claim_source,
            'AND slot_fence.pending_drain_intent_id IS NULL'
        ) = 0
        OR pg_catalog.strpos(
            claim_source,
            'AND pending_drain.drain_intent_id IS NULL'
        ) = 0
        OR pg_catalog.strpos(
            claim_source,
            'WHERE deployment.tenant_id = candidate_row.tenant_id'
        ) = 0
        OR pg_catalog.strpos(
            claim_source,
            'AND deployment IS NOT DISTINCT FROM candidate_row'
        ) = 0
        OR pg_catalog.strpos(
            claim_source,
            'IF pending_drain_intent_id IS NOT NULL THEN'
        ) < replay_position
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_selector_slot_writer_epoch_claim_contract_drift';
    END IF;

    global_position := pg_catalog.strpos(
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
    begin_position := pg_catalog.strpos(
        recover_source,
        'starring_runtime_slot_writer_fence_begin_unsafe_v2'
    );
    mutation_position := pg_catalog.strpos(
        recover_source,
        'UPDATE public.runtime_deployments AS deployment'
    );

    IF global_position = 0
        OR slot_position = 0
        OR physical_position = 0
        OR deployment_lock_position = 0
        OR begin_position = 0
        OR mutation_position = 0
        OR NOT (
            global_position < slot_position
            AND slot_position < physical_position
            AND physical_position < deployment_lock_position
            AND begin_position < mutation_position
        )
        OR (
            pg_catalog.length(recover_source)
            - pg_catalog.length(pg_catalog.replace(
                recover_source,
                'starring_runtime_slot_writer_fence_lock_v2',
                ''
            ))
        ) <> pg_catalog.length(
            'starring_runtime_slot_writer_fence_lock_v2'
        )
        OR (
            pg_catalog.length(recover_source)
            - pg_catalog.length(pg_catalog.replace(
                recover_source,
                'starring_runtime_slot_writer_fence_begin_unsafe_v2',
                ''
            ))
        ) <> pg_catalog.length(
            'starring_runtime_slot_writer_fence_begin_unsafe_v2'
        )
        OR (
            pg_catalog.length(recover_source)
            - pg_catalog.length(pg_catalog.replace(
                recover_source,
                'WHEN no_data_found THEN',
                ''
            ))
        ) <> pg_catalog.length('WHEN no_data_found THEN')
        OR (
            pg_catalog.length(recover_source)
            - pg_catalog.length(pg_catalog.replace(
                recover_source,
                'LIMIT 64',
                ''
            ))
        ) <> pg_catalog.length('LIMIT 64')
        OR pg_catalog.strpos(
            recover_source,
            'LEFT JOIN public.runtime_drain_intents_v2 AS pending_drain'
        ) = 0
        OR pg_catalog.strpos(
            recover_source,
            'AND slot_fence.pending_drain_intent_id IS NULL'
        ) = 0
        OR pg_catalog.strpos(
            recover_source,
            'AND pending_drain.drain_intent_id IS NULL'
        ) = 0
        OR pg_catalog.strpos(
            recover_source,
            'WHERE deployment.tenant_id = candidate_row.tenant_id'
        ) = 0
        OR pg_catalog.strpos(
            recover_source,
            'AND deployment IS NOT DISTINCT FROM candidate_row'
        ) = 0
        OR pg_catalog.strpos(
            recover_source,
            'PERFORM pg_catalog.pg_advisory_xact_lock('
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_execution_selector_slot_writer_epoch_recover_contract_drift';
    END IF;

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
            MESSAGE = 'runtime_execution_selector_slot_writer_epoch_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
