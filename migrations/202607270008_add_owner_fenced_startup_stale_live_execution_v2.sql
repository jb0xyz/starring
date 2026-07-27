SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
);

LOCK TABLE
    public.runtime_writer_fence,
    public.runtime_gateway_owners,
    public.runtime_deployments,
    public.runtime_serving_leases,
    public.runtime_slot_writer_fences_v2,
    public.runtime_certification_operations_v2,
    public.runtime_suspend_attempt_operations_v2,
    public.runtime_suspended_attempts_v2,
    public.runtime_suspend_attempt_completions_v2,
    public.runtime_product_operations_v2,
    public.runtime_drain_intents_v2,
    public.runtime_startup_recovery_actions_v2,
    public.activation_requests,
    public.authoring_promotions,
    public.product_tenants,
    public.automation_installations,
    public.automation_installation_authority_versions,
    public.automation_ruleset_activations,
    public.automation_ruleset_versions
IN ACCESS EXCLUSIVE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    private_schema_owner OID;
    executor_role OID;
    executor_role_is_quarantined BOOLEAN;
    executor_membership_count BIGINT;
    other_client_session_count BIGINT;
    prepared_transaction_count BIGINT;
    collision_count BIGINT;
    manifest_digest TEXT;
    readiness_digest TEXT;
    action_record_digest TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    SELECT namespace.nspowner
    INTO private_schema_owner
    FROM pg_catalog.pg_namespace AS namespace
    WHERE namespace.nspname = 'starring_runtime_private_v2';

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

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname =
            'starring_runtime_startup_recovery_execute_stale_live_v2';

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

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO action_record_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'starring_runtime_private_v2.starring_runtime_startup_recovery_action_record_v2(text,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,bytea)'
    );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR private_schema_owner IS DISTINCT FROM common_owner
        OR NOT executor_role_is_quarantined
        OR executor_membership_count <> 0
        OR other_client_session_count <> 0
        OR prepared_transaction_count <> 0
        OR collision_count <> 0
        OR EXISTS (
            SELECT 1
            FROM public.runtime_gateway_owners AS owner
            WHERE owner.process_instance_id IS NOT NULL
                AND owner.expires_at > pg_catalog.clock_timestamp()
        )
        OR EXISTS (
            SELECT 1
            FROM public.runtime_serving_leases AS serving
            WHERE serving.connected
                AND serving.serving
                AND serving.expires_at > pg_catalog.clock_timestamp()
        )
        OR manifest_digest IS DISTINCT FROM
            'c76a82cdd88a75259889d4cab4543797ad834d8f2e38f71268bbbc4b0e4cae0f'
        OR readiness_digest IS DISTINCT FROM
            'ee9364b3bb8b17a3a2386c0be06ae2ab12b519c77647a4073e96f45bfb5084a8'
        OR action_record_digest IS DISTINCT FROM
            'bead9e18b19984a20070ee4b739f0fa7aaebb87d07a03913af17dd8b4b5b24b4'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_stale_live_execution_preflight_drift';
    END IF;
END;
$preflight$;

DO $widen_terminal_projection$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'starring_runtime_private_v2.starring_runtime_startup_recovery_action_record_v2(text,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,bytea)'
    );

    previous_fragment := 'NOT BETWEEN 1 AND 131072';
    next_fragment := 'NOT BETWEEN 1 AND 1048576';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_stale_live_execution_action_record_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
    EXECUTE definition;

    ALTER TABLE public.runtime_startup_recovery_actions_v2
        DROP CONSTRAINT
            runtime_startup_recovery_actions_v2_terminal_check;
    ALTER TABLE public.runtime_startup_recovery_actions_v2
        ADD CONSTRAINT
            runtime_startup_recovery_actions_v2_terminal_check CHECK (
                pg_catalog.octet_length(terminal_projection_bytes)
                    BETWEEN 1 AND 1048576
                AND terminal_digest ~ '^[0-9a-f]{64}$'
                AND terminal_digest <> pg_catalog.repeat('0', 64)
                AND terminal_digest =
                    starring_runtime_private_v2.starring_runtime_startup_recovery_terminal_digest_v2(
                        record_format_version,
                        recovery_id,
                        originating_emergency_generation,
                        coordinator_generation,
                        action_authority_revision,
                        selection_authority_revision,
                        recovery_class,
                        gateway_shard_id,
                        owner_process_instance_id,
                        owner_lease_epoch,
                        owner_runtime_build_revision,
                        owner_revision,
                        owner_expires_at,
                        minimum_database_now,
                        recorded_at,
                        terminal_projection_bytes
                    )
            );
END;
$widen_terminal_projection$;

CREATE FUNCTION public.starring_runtime_startup_recovery_execute_stale_live_v2(
    requested_recovery_id TEXT,
    requested_originating_emergency_generation BIGINT,
    requested_coordinator_generation BIGINT,
    requested_action_authority_revision BIGINT,
    requested_selection_authority_revision BIGINT,
    expected_gateway_shard_id TEXT,
    expected_owner_process_instance_id TEXT,
    expected_owner_lease_epoch BIGINT,
    expected_owner_runtime_build_revision TEXT,
    expected_owner_revision BIGINT,
    expected_owner_expires_at TIMESTAMPTZ,
    requested_minimum_database_now TIMESTAMPTZ
)
RETURNS TABLE(
    journal_outcome_name TEXT,
    terminal_outcome_name TEXT,
    recovery_id TEXT,
    originating_emergency_generation BIGINT,
    coordinator_generation BIGINT,
    action_authority_revision BIGINT,
    selection_authority_revision BIGINT,
    recovery_class TEXT,
    observed_gateway_shard_id TEXT,
    observed_process_instance_id TEXT,
    observed_lease_epoch BIGINT,
    observed_runtime_build_revision TEXT,
    observed_owner_revision BIGINT,
    database_now TIMESTAMPTZ,
    observed_owner_expires_at TIMESTAMPTZ,
    minimum_database_now TIMESTAMPTZ,
    recorded_at TIMESTAMPTZ,
    terminal_projection_bytes BYTEA,
    terminal_digest TEXT
)
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
DECLARE
    owner_row public.runtime_gateway_owners%ROWTYPE;
    selection_action_row public.runtime_startup_recovery_actions_v2%ROWTYPE;
    authority_action_row public.runtime_startup_recovery_actions_v2%ROWTYPE;
    existing_action_row public.runtime_startup_recovery_actions_v2%ROWTYPE;
    candidate_row public.runtime_deployments%ROWTYPE;
    deployment_row public.runtime_deployments%ROWTYPE;
    terminal_deployment_row public.runtime_deployments%ROWTYPE;
    slot_fence_row public.runtime_slot_writer_fences_v2%ROWTYPE;
    terminal_slot_fence_row public.runtime_slot_writer_fences_v2%ROWTYPE;
    serving_row public.runtime_serving_leases%ROWTYPE;
    activation_row public.activation_requests%ROWTYPE;
    promotion_row public.authoring_promotions%ROWTYPE;
    action_record RECORD;
    selection_action_found BOOLEAN;
    authority_action_found BOOLEAN;
    serving_found BOOLEAN;
    writer_fence_state TEXT;
    writer_fence_count BIGINT;
    drain_state_constraint_count BIGINT;
    live_scope_count BIGINT;
    stale_live_count BIGINT;
    foreign_fresh_count BIGINT;
    ambiguous_live_count BIGINT;
    orphan_fresh_count BIGINT;
    reservation_count BIGINT;
    exact_awaiting_reservation_count BIGINT;
    invalid_suspend_attempt_count BIGINT;
    active_exact_route_count BIGINT;
    pending_drain_count BIGINT;
    selected_deployment_id TEXT;
    slot_writer_epoch BIGINT;
    successor_slot_writer_epoch BIGINT;
    pending_drain_intent_id TEXT;
    authority_outcome TEXT;
    mutation_clock TIMESTAMPTZ;
    recovery_kind TEXT;
    recovery_kind_tag SMALLINT;
    recovery_evidence TIMESTAMPTZ;
    certified_at TIMESTAMPTZ;
    next_snapshot JSONB;
    recovery_value JSONB;
    next_revision BIGINT;
    domain_bytes BYTEA;
    field_bytes BYTEA;
    no_candidate_projection BYTEA;
    progressed_projection_prefix BYTEA;
BEGIN
    PERFORM pg_catalog.set_config('TimeZone', 'UTC', TRUE);

    IF pg_catalog.current_setting('transaction_isolation')
            <> 'serializable'
        OR pg_catalog.current_setting('transaction_read_only') <> 'off'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_stale_live_execution_transaction_invalid';
    END IF;

    IF requested_selection_authority_revision
            NOT BETWEEN 1 AND 9223372036854775806
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_startup_stale_live_execution_input_invalid';
    END IF;

    IF requested_recovery_id !~ '^[0-9a-f]{32}$'
        OR requested_originating_emergency_generation
            NOT BETWEEN 1 AND 9223372036854775807
        OR requested_coordinator_generation
            NOT BETWEEN 1 AND 9223372036854775807
        OR requested_action_authority_revision
            NOT BETWEEN 2 AND 9223372036854775807
        OR requested_action_authority_revision
            <> requested_selection_authority_revision + 1
        OR expected_gateway_shard_id IS DISTINCT FROM 'shard:0'
        OR expected_owner_process_instance_id
            !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_owner_lease_epoch
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_owner_runtime_build_revision
            !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR expected_owner_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR NOT pg_catalog.isfinite(expected_owner_expires_at)
        OR NOT pg_catalog.isfinite(requested_minimum_database_now)
        OR requested_minimum_database_now >= expected_owner_expires_at
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_startup_stale_live_execution_input_invalid';
    END IF;

    domain_bytes := pg_catalog.convert_to(
        'starring.runtime.startup_recovery.stale_live.terminal.v2',
        'UTF8'
    );
    no_candidate_projection :=
        pg_catalog.int8send(
            pg_catalog.octet_length(domain_bytes)::BIGINT
        )
        || domain_bytes
        || pg_catalog.int2send(2::SMALLINT)
        || pg_catalog.int2send(0::SMALLINT);
    progressed_projection_prefix :=
        pg_catalog.int8send(
            pg_catalog.octet_length(domain_bytes)::BIGINT
        )
        || domain_bytes
        || pg_catalog.int2send(2::SMALLINT)
        || pg_catalog.int2send(1::SMALLINT);

    SELECT fence.fence_state
    INTO writer_fence_state
    FROM public.starring_runtime_writer_fence_observe_v1() AS fence;

    IF NOT FOUND
        OR writer_fence_state NOT IN ('open', 'closed')
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_stale_live_execution_writer_fence_invalid';
    END IF;
    IF writer_fence_state = 'closed' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX005',
            MESSAGE = 'runtime_startup_stale_live_execution_writer_fenced';
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            pg_catalog.concat(
                'starring-runtime-gateway-owner-v1:',
                expected_gateway_shard_id
            ),
            0
        )
    );
    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            pg_catalog.concat(
                'starring-runtime-startup-recovery-action-v2:',
                requested_recovery_id
            ),
            0
        )
    );

    SELECT owner.*
    INTO owner_row
    FROM public.runtime_gateway_owners AS owner
    WHERE owner.gateway_shard_id = expected_gateway_shard_id
    FOR UPDATE;

    database_now := pg_catalog.clock_timestamp();
    IF NOT FOUND
        OR owner_row.process_instance_id
            IS DISTINCT FROM expected_owner_process_instance_id
        OR owner_row.lease_epoch
            IS DISTINCT FROM expected_owner_lease_epoch
        OR owner_row.expected_build_revision
            IS DISTINCT FROM expected_owner_runtime_build_revision
        OR owner_row.owner_revision
            IS DISTINCT FROM expected_owner_revision
        OR owner_row.expires_at
            IS DISTINCT FROM expected_owner_expires_at
        OR owner_row.expires_at <= database_now
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_startup_stale_live_execution_owner_lost';
    END IF;
    IF database_now < requested_minimum_database_now THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_stale_live_execution_database_clock_regressed';
    END IF;

    SELECT action.*
    INTO selection_action_row
    FROM public.runtime_startup_recovery_actions_v2 AS action
    WHERE action.recovery_id = requested_recovery_id
        AND action.selection_authority_revision
            = requested_selection_authority_revision
    FOR UPDATE;
    selection_action_found := FOUND;

    SELECT action.*
    INTO authority_action_row
    FROM public.runtime_startup_recovery_actions_v2 AS action
    WHERE action.recovery_id = requested_recovery_id
        AND action.action_authority_revision
            = requested_action_authority_revision
    FOR UPDATE;
    authority_action_found := FOUND;

    IF selection_action_found OR authority_action_found THEN
        IF selection_action_found THEN
            existing_action_row := selection_action_row;
        ELSE
            existing_action_row := authority_action_row;
        END IF;

        SELECT record.*
        INTO STRICT action_record
        FROM starring_runtime_private_v2.starring_runtime_startup_recovery_action_record_v2(
            requested_recovery_id,
            requested_originating_emergency_generation,
            requested_coordinator_generation,
            requested_action_authority_revision,
            requested_selection_authority_revision,
            'stale_live',
            expected_gateway_shard_id,
            expected_owner_process_instance_id,
            expected_owner_lease_epoch,
            expected_owner_runtime_build_revision,
            expected_owner_revision,
            expected_owner_expires_at,
            requested_minimum_database_now,
            existing_action_row.terminal_projection_bytes
        ) AS record;

        IF action_record.outcome_name IS DISTINCT FROM 'replayed'
            OR action_record.database_now < database_now
            OR action_record.recorded_at > action_record.database_now
            OR action_record.database_now >= expected_owner_expires_at
            OR action_record.observed_gateway_shard_id
                IS DISTINCT FROM expected_gateway_shard_id
            OR action_record.observed_process_instance_id
                IS DISTINCT FROM expected_owner_process_instance_id
            OR action_record.observed_lease_epoch
                IS DISTINCT FROM expected_owner_lease_epoch
            OR action_record.observed_runtime_build_revision
                IS DISTINCT FROM expected_owner_runtime_build_revision
            OR action_record.observed_owner_revision
                IS DISTINCT FROM expected_owner_revision
            OR action_record.observed_owner_expires_at
                IS DISTINCT FROM expected_owner_expires_at
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_startup_stale_live_execution_replay_invalid';
        END IF;

        IF existing_action_row.terminal_projection_bytes
                IS NOT DISTINCT FROM no_candidate_projection
        THEN
            terminal_outcome_name := 'no_candidate';
        ELSIF pg_catalog.substring(
                existing_action_row.terminal_projection_bytes,
                1,
                pg_catalog.octet_length(progressed_projection_prefix)
            ) IS NOT DISTINCT FROM progressed_projection_prefix
            AND pg_catalog.octet_length(
                existing_action_row.terminal_projection_bytes
            ) > pg_catalog.octet_length(progressed_projection_prefix)
        THEN
            terminal_outcome_name := 'progressed';
        ELSE
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_startup_stale_live_execution_projection_invalid';
        END IF;

        journal_outcome_name := action_record.outcome_name;
        recovery_id := requested_recovery_id;
        originating_emergency_generation :=
            requested_originating_emergency_generation;
        coordinator_generation := requested_coordinator_generation;
        action_authority_revision :=
            requested_action_authority_revision;
        selection_authority_revision :=
            requested_selection_authority_revision;
        recovery_class := 'stale_live';
        observed_gateway_shard_id :=
            action_record.observed_gateway_shard_id;
        observed_process_instance_id :=
            action_record.observed_process_instance_id;
        observed_lease_epoch := action_record.observed_lease_epoch;
        observed_runtime_build_revision :=
            action_record.observed_runtime_build_revision;
        observed_owner_revision := action_record.observed_owner_revision;
        database_now := action_record.database_now;
        observed_owner_expires_at :=
            action_record.observed_owner_expires_at;
        minimum_database_now := requested_minimum_database_now;
        recorded_at := action_record.recorded_at;
        terminal_projection_bytes :=
            existing_action_row.terminal_projection_bytes;
        terminal_digest := action_record.terminal_digest;
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT pg_catalog.count(*)
    INTO writer_fence_count
    FROM public.runtime_writer_fence AS fence
    WHERE fence.singleton
        AND fence.fence_state = 'open';

    SELECT pg_catalog.count(*)
    INTO drain_state_constraint_count
    FROM pg_catalog.pg_constraint AS constraint_row
    WHERE constraint_row.conrelid = pg_catalog.to_regclass(
            'public.runtime_drain_intents_v2'
        )
        AND constraint_row.conname =
            'runtime_drain_intents_v2_state_check'
        AND constraint_row.contype = 'c'
        AND constraint_row.convalidated
        AND pg_catalog.pg_get_constraintdef(
            constraint_row.oid,
            TRUE
        ) = 'CHECK (intent_state = ''pending''::text)';

    IF writer_fence_count <> 1
        OR (
            SELECT pg_catalog.count(*)
            FROM public.runtime_writer_fence
        ) <> 1
        OR drain_state_constraint_count <> 1
        OR EXISTS (
            SELECT 1
            FROM public.runtime_drain_intents_v2 AS drain
            WHERE drain.intent_state IS DISTINCT FROM 'pending'
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_stale_live_execution_state_ambiguous';
    END IF;

    WITH eligible_live AS (
        SELECT
            deployment.*,
            lease.guild_id AS lease_guild_id,
            lease.ruleset_key AS lease_ruleset_key,
            lease.tenant_id AS lease_tenant_id,
            lease.installation_id AS lease_installation_id,
            lease.deployment_id AS lease_deployment_id,
            lease.attestation_id AS lease_attestation_id,
            lease.process_instance_id AS lease_process_instance_id,
            lease.runtime_generation AS lease_runtime_generation,
            lease.target_version AS lease_target_version,
            lease.target_content_hash AS lease_target_content_hash,
            lease.binding_revision AS lease_binding_revision,
            lease.binding_fingerprint AS lease_binding_fingerprint,
            lease.lease_epoch AS lease_epoch,
            lease.revision AS lease_revision,
            lease.connected AS lease_connected,
            lease.serving AS lease_serving,
            lease.acquired_at AS lease_acquired_at,
            lease.last_heartbeat_at AS lease_last_heartbeat_at,
            lease.expires_at AS lease_expires_at,
            slot_fence.writer_epoch AS slot_writer_epoch,
            slot_fence.pending_drain_intent_id
        FROM public.runtime_deployments AS deployment
        LEFT JOIN public.runtime_serving_leases AS lease
            ON lease.guild_id = deployment.guild_id
            AND lease.ruleset_key = deployment.ruleset_key
        LEFT JOIN public.runtime_slot_writer_fences_v2 AS slot_fence
            ON slot_fence.slot_guild_id = deployment.guild_id
            AND slot_fence.slot_ruleset_key = deployment.ruleset_key
        WHERE deployment.phase = 'live'
            AND NOT EXISTS (
                SELECT 1
                FROM public.runtime_drain_intents_v2 AS drain
                WHERE drain.slot_guild_id = deployment.guild_id
                    AND drain.slot_ruleset_key =
                        deployment.ruleset_key
                    AND drain.intent_state = 'pending'
            )
    ), classified_live AS (
        SELECT
            live.*,
            CASE
                WHEN pg_catalog.pg_input_is_valid(
                    live.snapshot #>> '{live,certified_at}',
                    'timestamp with time zone'
                )
                THEN (
                    live.snapshot #>> '{live,certified_at}'
                )::TIMESTAMPTZ
                ELSE NULL
            END AS certified_at,
            (
                live.live_attestation_id IS NOT NULL
                AND live.snapshot #>> '{phase,phase}' = 'live'
                AND live.snapshot ->> 'revision' =
                    live.revision::TEXT
                AND live.snapshot #>> '{live,process_instance_id}' =
                    live.lease_process_instance_id
                AND live.lease_guild_id = live.guild_id
                AND live.lease_ruleset_key = live.ruleset_key
                AND live.lease_tenant_id = live.tenant_id
                AND live.lease_installation_id =
                    live.installation_id
                AND live.lease_deployment_id = live.deployment_id
                AND live.lease_attestation_id =
                    live.live_attestation_id
                AND live.lease_runtime_generation =
                    live.runtime_generation
                AND live.lease_target_version =
                    live.target_version
                AND live.lease_target_content_hash =
                    live.target_content_hash
                AND live.lease_binding_revision =
                    live.binding_revision
                AND live.lease_binding_fingerprint =
                    live.binding_fingerprint
                AND live.lease_connected IS NOT NULL
                AND live.lease_serving =
                    live.lease_connected
                AND live.lease_acquired_at <=
                    live.lease_last_heartbeat_at
                AND live.lease_last_heartbeat_at <=
                    live.lease_expires_at
                AND live.slot_writer_epoch
                    BETWEEN 1 AND 9223372036854775807
                AND live.pending_drain_intent_id IS NULL
                AND pg_catalog.pg_input_is_valid(
                    live.snapshot #>> '{live,certified_at}',
                    'timestamp with time zone'
                )
                AND NOT EXISTS (
                    SELECT 1
                    FROM public.runtime_deployments AS newer
                    WHERE newer.guild_id = live.guild_id
                        AND newer.ruleset_key = live.ruleset_key
                        AND newer.deployment_id <>
                            live.deployment_id
                        AND newer.phase NOT IN (
                            'live',
                            'superseded',
                            'cancelled'
                        )
                )
                AND EXISTS (
                    SELECT 1
                    FROM public.activation_requests AS activation
                    INNER JOIN public.authoring_promotions AS promotion
                        ON promotion.id = live.promotion_id
                        AND promotion.stage = 'activation_pending'
                        AND promotion.tenant_id = live.tenant_id
                    INNER JOIN public.product_tenants AS tenant
                        ON tenant.tenant_id = live.tenant_id
                        AND tenant.lifecycle_state = 'active'
                    INNER JOIN public.automation_installations
                        AS installation
                        ON installation.tenant_id = live.tenant_id
                        AND installation.installation_id =
                            live.installation_id
                        AND installation.lifecycle_state = 'active'
                    INNER JOIN public.automation_installation_authority_versions
                        AS historical_authority
                        ON historical_authority.tenant_id =
                            installation.tenant_id
                        AND historical_authority.installation_id =
                            installation.installation_id
                        AND historical_authority.revision =
                            live.installation_authority_revision
                        AND historical_authority.binding_revision =
                            live.binding_revision
                        AND historical_authority.binding_fingerprint =
                            live.binding_fingerprint
                    INNER JOIN public.automation_installation_authority_versions
                        AS current_authority
                        ON current_authority.tenant_id =
                            installation.tenant_id
                        AND current_authority.installation_id =
                            installation.installation_id
                        AND current_authority.revision =
                            installation.current_authority_revision
                        AND current_authority.binding_revision =
                            live.binding_revision
                        AND current_authority.binding_fingerprint =
                            live.binding_fingerprint
                        AND current_authority.resource_bindings
                            IS NOT DISTINCT FROM
                                historical_authority.resource_bindings
                    INNER JOIN public.automation_ruleset_activations
                        AS active
                        ON active.guild_id = live.guild_id
                        AND active.ruleset_key = live.ruleset_key
                        AND active.active_version =
                            live.target_version
                    INNER JOIN public.automation_ruleset_versions
                        AS version
                        ON version.guild_id = active.guild_id
                        AND version.ruleset_key = active.ruleset_key
                        AND version.version = active.active_version
                        AND version.content_hash =
                            live.target_content_hash
                        AND version.canonical_content_hash =
                            version.content_hash
                        AND version.schema_version = 1
                    WHERE activation.id =
                            live.activation_request_id
                        AND activation.state = 'applied'
                        AND activation.authority_kind =
                            'product_authoring'
                        AND activation.link_state_name = 'linked'
                        AND activation.promotion_id =
                            live.promotion_id
                )
            ) AS shape_is_exact
        FROM eligible_live AS live
    ), categorized_live AS (
        SELECT
            live.*,
            (
                live.shape_is_exact
                AND live.lease_connected
                AND live.lease_serving
                AND live.lease_expires_at > database_now
                AND live.lease_process_instance_id <>
                    expected_owner_process_instance_id
            ) AS is_foreign_fresh,
            (
                live.shape_is_exact
                AND (
                    (
                        NOT live.lease_connected
                        AND NOT live.lease_serving
                        AND live.lease_last_heartbeat_at =
                            live.lease_expires_at
                        AND live.lease_expires_at <= database_now
                        AND live.lease_last_heartbeat_at >=
                            live.certified_at
                    )
                    OR (
                        live.lease_connected
                        AND live.lease_serving
                        AND live.lease_last_heartbeat_at <
                            live.lease_expires_at
                        AND live.lease_expires_at <= database_now
                        AND live.lease_expires_at >=
                            live.certified_at
                    )
                )
            ) AS is_recoverable_stale
        FROM classified_live AS live
    ), live_counts AS (
        SELECT
            pg_catalog.count(*) AS live_scope_count,
            pg_catalog.count(*) FILTER (
                WHERE live.is_recoverable_stale
            ) AS stale_live_count,
            pg_catalog.count(*) FILTER (
                WHERE live.is_foreign_fresh
            ) AS foreign_fresh_count,
            pg_catalog.count(*) FILTER (
                WHERE NOT live.is_recoverable_stale
                    AND NOT live.is_foreign_fresh
            ) AS ambiguous_live_count
        FROM categorized_live AS live
    )
    SELECT
        counts.live_scope_count,
        counts.stale_live_count,
        counts.foreign_fresh_count,
        counts.ambiguous_live_count,
        (
            SELECT live.deployment_id
            FROM categorized_live AS live
            WHERE live.is_recoverable_stale
            ORDER BY
                live.lease_expires_at,
                live.updated_at,
                live.deployment_id COLLATE pg_catalog."C"
            LIMIT 1
        )
    INTO
        live_scope_count,
        stale_live_count,
        foreign_fresh_count,
        ambiguous_live_count,
        selected_deployment_id
    FROM live_counts AS counts;

    SELECT pg_catalog.count(*)
    INTO orphan_fresh_count
    FROM public.runtime_serving_leases AS lease
    WHERE lease.connected
        AND lease.serving
        AND lease.expires_at > database_now
        AND NOT EXISTS (
            SELECT 1
            FROM public.runtime_deployments AS deployment
            WHERE deployment.guild_id = lease.guild_id
                AND deployment.ruleset_key = lease.ruleset_key
                AND deployment.tenant_id = lease.tenant_id
                AND deployment.installation_id =
                    lease.installation_id
                AND deployment.deployment_id =
                    lease.deployment_id
                AND deployment.live_attestation_id =
                    lease.attestation_id
                AND deployment.phase = 'live'
        );

    IF ambiguous_live_count <> 0
        OR orphan_fresh_count <> 0
        OR (
            stale_live_count <> 0
            AND foreign_fresh_count <> 0
        )
        OR live_scope_count <>
            stale_live_count
            + foreign_fresh_count
            + ambiguous_live_count
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_stale_live_execution_state_ambiguous';
    END IF;

    SELECT
        pg_catalog.count(*),
        pg_catalog.count(*) FILTER (
            WHERE deployment.phase = 'awaiting_gateway_ready'
                AND deployment.revision =
                    reservation.deployment_revision
                AND deployment.convergence_attempt_no =
                    reservation.convergence_attempt_no
                AND deployment.snapshot #>> '{phase,phase}' =
                    'awaiting_gateway_ready'
                AND deployment.snapshot ->> 'revision' =
                    reservation.deployment_revision::TEXT
                AND deployment.controller_id IS NOT NULL
                AND deployment.controller_fencing_token IS NOT NULL
                AND deployment.last_controller_id =
                    deployment.controller_id
                AND deployment.last_fencing_token =
                    deployment.controller_fencing_token
        )
    INTO reservation_count, exact_awaiting_reservation_count
    FROM public.runtime_certification_operations_v2 AS reservation
    LEFT JOIN public.runtime_deployments AS deployment
        ON deployment.tenant_id = reservation.tenant_id
        AND deployment.installation_id =
            reservation.installation_id
        AND deployment.deployment_id = reservation.deployment_id;

    IF reservation_count <> exact_awaiting_reservation_count THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_stale_live_execution_state_ambiguous';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_suspend_attempt_count
    FROM public.runtime_suspend_attempt_operations_v2 AS operation
    LEFT JOIN public.runtime_suspended_attempts_v2 AS suspended
        ON suspended.suspension_id = operation.suspension_id
    LEFT JOIN public.runtime_suspend_attempt_completions_v2 AS completion
        ON completion.suspension_id = operation.suspension_id
    WHERE (
            CASE
                WHEN suspended.suspension_id IS NULL THEN 0
                ELSE 1
            END
            +
            CASE
                WHEN completion.suspension_id IS NULL THEN 0
                ELSE 1
            END
        ) <> 1;

    IF invalid_suspend_attempt_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_stale_live_execution_state_ambiguous';
    END IF;

    SELECT pg_catalog.count(*)
    INTO active_exact_route_count
    FROM public.runtime_suspended_attempts_v2 AS suspended
    WHERE suspended.local_effect_kind = 'exact_route';

    SELECT pg_catalog.count(*)
    INTO pending_drain_count
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE drain.intent_state = 'pending';

    IF reservation_count > 4294967295
        OR active_exact_route_count > 4294967295
        OR pending_drain_count > 4294967295
        OR stale_live_count > 4294967295
        OR foreign_fresh_count > 4294967295
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_stale_live_execution_state_ambiguous';
    END IF;

    IF stale_live_count = 0 THEN
        IF selected_deployment_id IS NOT NULL THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_startup_stale_live_execution_selection_invalid';
        END IF;

        SELECT record.*
        INTO STRICT action_record
        FROM starring_runtime_private_v2.starring_runtime_startup_recovery_action_record_v2(
            requested_recovery_id,
            requested_originating_emergency_generation,
            requested_coordinator_generation,
            requested_action_authority_revision,
            requested_selection_authority_revision,
            'stale_live',
            expected_gateway_shard_id,
            expected_owner_process_instance_id,
            expected_owner_lease_epoch,
            expected_owner_runtime_build_revision,
            expected_owner_revision,
            expected_owner_expires_at,
            requested_minimum_database_now,
            no_candidate_projection
        ) AS record;

        IF action_record.outcome_name IS DISTINCT FROM 'applied'
            OR action_record.database_now < database_now
            OR action_record.recorded_at < database_now
            OR action_record.database_now
                IS DISTINCT FROM action_record.recorded_at
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_startup_stale_live_execution_record_invalid';
        END IF;

        journal_outcome_name := action_record.outcome_name;
        terminal_outcome_name := 'no_candidate';
        recovery_id := requested_recovery_id;
        originating_emergency_generation :=
            requested_originating_emergency_generation;
        coordinator_generation := requested_coordinator_generation;
        action_authority_revision :=
            requested_action_authority_revision;
        selection_authority_revision :=
            requested_selection_authority_revision;
        recovery_class := 'stale_live';
        observed_gateway_shard_id :=
            action_record.observed_gateway_shard_id;
        observed_process_instance_id :=
            action_record.observed_process_instance_id;
        observed_lease_epoch := action_record.observed_lease_epoch;
        observed_runtime_build_revision :=
            action_record.observed_runtime_build_revision;
        observed_owner_revision := action_record.observed_owner_revision;
        database_now := action_record.database_now;
        observed_owner_expires_at :=
            action_record.observed_owner_expires_at;
        minimum_database_now := requested_minimum_database_now;
        recorded_at := action_record.recorded_at;
        terminal_projection_bytes := no_candidate_projection;
        terminal_digest := action_record.terminal_digest;
        RETURN NEXT;
        RETURN;
    END IF;

    IF selected_deployment_id IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_stale_live_execution_selection_invalid';
    END IF;

    SELECT deployment.*
    INTO candidate_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.deployment_id = selected_deployment_id;

    IF NOT FOUND
        OR candidate_row.guild_id !~ '^[1-9][0-9]{0,19}$'
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
            MESSAGE = 'runtime_startup_stale_live_execution_selection_invalid';
    END IF;

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

    SELECT
        slot_fence.writer_epoch,
        slot_fence.pending_drain_intent_id
    INTO slot_writer_epoch, pending_drain_intent_id
    FROM starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(
        candidate_row.guild_id,
        candidate_row.ruleset_key
    ) AS slot_fence;

    IF pending_drain_intent_id IS NOT NULL
        OR slot_writer_epoch NOT BETWEEN 1 AND 9223372036854775806
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_stale_live_execution_slot_invalid';
    END IF;

    SELECT fence.*
    INTO slot_fence_row
    FROM public.runtime_slot_writer_fences_v2 AS fence
    WHERE fence.slot_guild_id = candidate_row.guild_id
        AND fence.slot_ruleset_key = candidate_row.ruleset_key
        AND fence.writer_epoch = slot_writer_epoch
    FOR UPDATE;

    IF NOT FOUND
        OR slot_fence_row.pending_drain_intent_id IS NOT NULL
        OR slot_fence_row.pending_product_operation_id IS NOT NULL
        OR slot_fence_row.pending_tenant_id IS NOT NULL
        OR slot_fence_row.pending_installation_id IS NOT NULL
        OR slot_fence_row.pending_deployment_id IS NOT NULL
        OR slot_fence_row.pending_expected_revision IS NOT NULL
        OR slot_fence_row.pending_marked_at IS NOT NULL
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_stale_live_execution_slot_invalid';
    END IF;

    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = candidate_row.tenant_id
        AND deployment.installation_id =
            candidate_row.installation_id
        AND deployment.deployment_id = candidate_row.deployment_id
        AND deployment.revision = candidate_row.revision
        AND deployment.guild_id = candidate_row.guild_id
        AND deployment.ruleset_key = candidate_row.ruleset_key
        AND deployment IS NOT DISTINCT FROM candidate_row
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = '40001',
            MESSAGE = 'runtime_startup_stale_live_execution_selection_changed';
    END IF;

    IF deployment_row.revision NOT BETWEEN 1 AND 9223372036854775806
        OR deployment_row.convergence_attempt_no
            NOT BETWEEN 1 AND 4294967295
        OR deployment_row.phase IS DISTINCT FROM 'live'
        OR deployment_row.live_attestation_id IS NULL
        OR deployment_row.snapshot #>> '{phase,phase}'
            IS DISTINCT FROM 'live'
        OR deployment_row.snapshot ->> 'revision'
            IS DISTINCT FROM deployment_row.revision::TEXT
        OR NOT pg_catalog.pg_input_is_valid(
            deployment_row.snapshot #>> '{live,certified_at}',
            'timestamp with time zone'
        )
        OR EXISTS (
            SELECT 1
            FROM public.runtime_drain_intents_v2 AS drain
            WHERE drain.slot_guild_id = deployment_row.guild_id
                AND drain.slot_ruleset_key =
                    deployment_row.ruleset_key
                AND drain.intent_state = 'pending'
        )
        OR EXISTS (
            SELECT 1
            FROM public.runtime_deployments AS newer
            WHERE newer.guild_id = deployment_row.guild_id
                AND newer.ruleset_key = deployment_row.ruleset_key
                AND newer.deployment_id <> deployment_row.deployment_id
                AND newer.phase NOT IN (
                    'live',
                    'superseded',
                    'cancelled'
                )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = '40001',
            MESSAGE = 'runtime_startup_stale_live_execution_selection_changed';
    END IF;

    SELECT activation.*
    INTO activation_row
    FROM public.activation_requests AS activation
    WHERE activation.id = deployment_row.activation_request_id
    FOR SHARE;

    IF NOT FOUND
        OR activation_row.authority_kind
            IS DISTINCT FROM 'product_authoring'
        OR activation_row.link_state_name IS DISTINCT FROM 'linked'
        OR activation_row.state IS DISTINCT FROM 'applied'
        OR activation_row.promotion_id
            IS DISTINCT FROM deployment_row.promotion_id
        OR activation_row.guild_id
            IS DISTINCT FROM deployment_row.guild_id
        OR activation_row.ruleset_key
            IS DISTINCT FROM deployment_row.ruleset_key
        OR activation_row.target_version
            IS DISTINCT FROM deployment_row.target_version
        OR activation_row.target_content_hash
            IS DISTINCT FROM deployment_row.target_content_hash
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_startup_stale_live_execution_authority_changed';
    END IF;

    SELECT promotion.*
    INTO promotion_row
    FROM public.authoring_promotions AS promotion
    WHERE promotion.id = deployment_row.promotion_id
    FOR SHARE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_startup_stale_live_execution_authority_changed';
    END IF;

    IF pg_catalog.pg_input_is_valid(
            promotion_row.record
                #>> '{intent,authority,binding_revision}',
            'bigint'
        ) IS NOT TRUE
        OR pg_catalog.pg_input_is_valid(
            promotion_row.record
                #>> '{stage,activation,target,version}',
            'bigint'
        ) IS NOT TRUE
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_stale_live_execution_authority_invalid';
    END IF;

    IF promotion_row.stage IS DISTINCT FROM 'activation_pending'
        OR promotion_row.tenant_id
            IS DISTINCT FROM deployment_row.tenant_id
        OR promotion_row.record #>> '{intent,authority,tenant_id}'
            IS DISTINCT FROM deployment_row.tenant_id
        OR promotion_row.record #>> '{intent,authority,installation_id}'
            IS DISTINCT FROM deployment_row.installation_id
        OR promotion_row.record #>> '{intent,authority,guild_id}'
            IS DISTINCT FROM deployment_row.guild_id
        OR promotion_row.record #>> '{intent,authority,ruleset_key}'
            IS DISTINCT FROM deployment_row.ruleset_key
        OR promotion_row.record #>> '{intent,authority,binding_revision}'
            IS DISTINCT FROM deployment_row.binding_revision::TEXT
        OR promotion_row.record #>> '{intent,evidence,context_fingerprint}'
            IS DISTINCT FROM deployment_row.binding_fingerprint
        OR promotion_row.record #>> '{stage,activation,request_id}'
            IS DISTINCT FROM deployment_row.activation_request_id
        OR promotion_row.record #>> '{stage,activation,target,guild_id}'
            IS DISTINCT FROM deployment_row.guild_id
        OR promotion_row.record #>> '{stage,activation,target,ruleset_key}'
            IS DISTINCT FROM deployment_row.ruleset_key
        OR promotion_row.record #>> '{stage,activation,target,version}'
            IS DISTINCT FROM deployment_row.target_version::TEXT
        OR promotion_row.record #>> '{stage,activation,target,content_hash}'
            IS DISTINCT FROM deployment_row.target_content_hash
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_startup_stale_live_execution_authority_changed';
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
            MESSAGE = 'runtime_startup_stale_live_execution_target_superseded';
    ELSIF authority_outcome IS DISTINCT FROM 'exact' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_startup_stale_live_execution_authority_changed';
    END IF;

    SELECT lease.*
    INTO serving_row
    FROM public.runtime_serving_leases AS lease
    WHERE lease.guild_id = deployment_row.guild_id
        AND lease.ruleset_key = deployment_row.ruleset_key
    FOR UPDATE;
    serving_found := FOUND;

    mutation_clock := public.starring_runtime_mutation_clock();
    certified_at := (
        deployment_row.snapshot #>> '{live,certified_at}'
    )::TIMESTAMPTZ;

    IF NOT serving_found
        OR owner_row.expires_at <= mutation_clock
        OR mutation_clock < database_now
        OR mutation_clock < requested_minimum_database_now
        OR serving_row.guild_id
            IS DISTINCT FROM deployment_row.guild_id
        OR serving_row.ruleset_key
            IS DISTINCT FROM deployment_row.ruleset_key
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
        OR serving_row.target_version
            IS DISTINCT FROM deployment_row.target_version
        OR serving_row.target_content_hash
            IS DISTINCT FROM deployment_row.target_content_hash
        OR serving_row.binding_revision
            IS DISTINCT FROM deployment_row.binding_revision
        OR serving_row.binding_fingerprint
            IS DISTINCT FROM deployment_row.binding_fingerprint
        OR serving_row.lease_epoch
            NOT BETWEEN 1 AND 9223372036854775807
        OR serving_row.revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR serving_row.acquired_at > serving_row.last_heartbeat_at
        OR serving_row.last_heartbeat_at > serving_row.expires_at
        OR serving_row.acquired_at > mutation_clock
        OR serving_row.serving IS DISTINCT FROM serving_row.connected
    THEN
        RAISE EXCEPTION USING
            ERRCODE = '40001',
            MESSAGE = 'runtime_startup_stale_live_execution_selection_changed';
    END IF;

    IF NOT serving_row.connected AND NOT serving_row.serving
        AND serving_row.last_heartbeat_at = serving_row.expires_at
        AND serving_row.expires_at <= mutation_clock
        AND serving_row.last_heartbeat_at >= certified_at
    THEN
        recovery_kind := 'serving_disconnected';
        recovery_kind_tag := 1;
        recovery_evidence := serving_row.last_heartbeat_at;
    ELSIF serving_row.connected AND serving_row.serving
        AND serving_row.last_heartbeat_at < serving_row.expires_at
        AND serving_row.expires_at <= mutation_clock
        AND serving_row.expires_at >= certified_at
    THEN
        recovery_kind := 'serving_lease_expired';
        recovery_kind_tag := 2;
        recovery_evidence := serving_row.expires_at;
    ELSE
        RAISE EXCEPTION USING
            ERRCODE = '40001',
            MESSAGE = 'runtime_startup_stale_live_execution_selection_changed';
    END IF;

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

    successor_slot_writer_epoch :=
        starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(
            deployment_row.guild_id,
            deployment_row.ruleset_key,
            slot_writer_epoch
        );
    IF successor_slot_writer_epoch <> slot_writer_epoch + 1 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_stale_live_execution_slot_invalid';
    END IF;

    SELECT fence.*
    INTO terminal_slot_fence_row
    FROM public.runtime_slot_writer_fences_v2 AS fence
    WHERE fence.slot_guild_id = slot_fence_row.slot_guild_id
        AND fence.slot_ruleset_key = slot_fence_row.slot_ruleset_key
    FOR UPDATE;

    IF NOT FOUND
        OR terminal_slot_fence_row.writer_epoch
            IS DISTINCT FROM successor_slot_writer_epoch
        OR terminal_slot_fence_row.slot_guild_id
            IS DISTINCT FROM slot_fence_row.slot_guild_id
        OR terminal_slot_fence_row.slot_ruleset_key
            IS DISTINCT FROM slot_fence_row.slot_ruleset_key
        OR terminal_slot_fence_row.pending_drain_intent_id IS NOT NULL
        OR terminal_slot_fence_row.pending_product_operation_id IS NOT NULL
        OR terminal_slot_fence_row.pending_tenant_id IS NOT NULL
        OR terminal_slot_fence_row.pending_installation_id IS NOT NULL
        OR terminal_slot_fence_row.pending_deployment_id IS NOT NULL
        OR terminal_slot_fence_row.pending_expected_revision IS NOT NULL
        OR terminal_slot_fence_row.pending_marked_at IS NOT NULL
        OR terminal_slot_fence_row.updated_at < slot_fence_row.updated_at
        OR (
            pg_catalog.to_jsonb(terminal_slot_fence_row)
                - ARRAY['writer_epoch', 'updated_at']::TEXT[]
        ) IS DISTINCT FROM (
            pg_catalog.to_jsonb(slot_fence_row)
                - ARRAY['writer_epoch', 'updated_at']::TEXT[]
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_stale_live_execution_slot_invalid';
    END IF;

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
        AND deployment.revision = deployment_row.revision
    RETURNING deployment.* INTO terminal_deployment_row;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = '40001',
            MESSAGE = 'runtime_startup_stale_live_execution_selection_changed';
    END IF;

    IF terminal_deployment_row.revision IS DISTINCT FROM next_revision
        OR terminal_deployment_row.phase
            IS DISTINCT FROM 'runtime_pending'
        OR terminal_deployment_row.live_attestation_id IS NOT NULL
        OR terminal_deployment_row.live_at IS NOT NULL
        OR terminal_deployment_row.snapshot IS DISTINCT FROM next_snapshot
        OR terminal_deployment_row.updated_at IS DISTINCT FROM GREATEST(
            mutation_clock,
            deployment_row.updated_at + INTERVAL '1 microsecond'
        )
        OR (
            pg_catalog.to_jsonb(terminal_deployment_row)
                - ARRAY[
                    'snapshot',
                    'revision',
                    'phase',
                    'live_attestation_id',
                    'live_at',
                    'updated_at'
                ]::TEXT[]
        ) IS DISTINCT FROM (
            pg_catalog.to_jsonb(deployment_row)
                - ARRAY[
                    'snapshot',
                    'revision',
                    'phase',
                    'live_attestation_id',
                    'live_at',
                    'updated_at'
                ]::TEXT[]
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_stale_live_execution_postcondition_invalid';
    END IF;

    terminal_projection_bytes := progressed_projection_prefix;

    field_bytes := pg_catalog.jsonb_send(
        pg_catalog.to_jsonb(deployment_row)
    );
    terminal_projection_bytes := terminal_projection_bytes
        || pg_catalog.int8send(
            pg_catalog.octet_length(field_bytes)::BIGINT
        )
        || field_bytes;
    field_bytes := pg_catalog.jsonb_send(
        pg_catalog.to_jsonb(terminal_deployment_row)
    );
    terminal_projection_bytes := terminal_projection_bytes
        || pg_catalog.int8send(
            pg_catalog.octet_length(field_bytes)::BIGINT
        )
        || field_bytes;
    field_bytes := pg_catalog.jsonb_send(
        pg_catalog.to_jsonb(slot_fence_row)
    );
    terminal_projection_bytes := terminal_projection_bytes
        || pg_catalog.int8send(
            pg_catalog.octet_length(field_bytes)::BIGINT
        )
        || field_bytes;
    field_bytes := pg_catalog.jsonb_send(
        pg_catalog.to_jsonb(terminal_slot_fence_row)
    );
    terminal_projection_bytes := terminal_projection_bytes
        || pg_catalog.int8send(
            pg_catalog.octet_length(field_bytes)::BIGINT
        )
        || field_bytes;
    field_bytes := pg_catalog.jsonb_send(
        pg_catalog.to_jsonb(serving_row)
    );
    terminal_projection_bytes := terminal_projection_bytes
        || pg_catalog.int8send(
            pg_catalog.octet_length(field_bytes)::BIGINT
        )
        || field_bytes
        || pg_catalog.int2send(recovery_kind_tag)
        || pg_catalog.timestamptz_send(recovery_evidence)
        || pg_catalog.timestamptz_send(mutation_clock);

    IF pg_catalog.octet_length(terminal_projection_bytes)
            NOT BETWEEN 1 AND 1048576
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_stale_live_execution_projection_invalid';
    END IF;

    SELECT record.*
    INTO STRICT action_record
    FROM starring_runtime_private_v2.starring_runtime_startup_recovery_action_record_v2(
        requested_recovery_id,
        requested_originating_emergency_generation,
        requested_coordinator_generation,
        requested_action_authority_revision,
        requested_selection_authority_revision,
        'stale_live',
        expected_gateway_shard_id,
        expected_owner_process_instance_id,
        expected_owner_lease_epoch,
        expected_owner_runtime_build_revision,
        expected_owner_revision,
        expected_owner_expires_at,
        requested_minimum_database_now,
        terminal_projection_bytes
    ) AS record;

    IF action_record.outcome_name IS DISTINCT FROM 'applied'
        OR action_record.database_now < mutation_clock
        OR action_record.recorded_at < mutation_clock
        OR action_record.database_now
            IS DISTINCT FROM action_record.recorded_at
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_stale_live_execution_record_invalid';
    END IF;

    journal_outcome_name := action_record.outcome_name;
    terminal_outcome_name := 'progressed';
    recovery_id := requested_recovery_id;
    originating_emergency_generation :=
        requested_originating_emergency_generation;
    coordinator_generation := requested_coordinator_generation;
    action_authority_revision := requested_action_authority_revision;
    selection_authority_revision :=
        requested_selection_authority_revision;
    recovery_class := 'stale_live';
    observed_gateway_shard_id :=
        action_record.observed_gateway_shard_id;
    observed_process_instance_id :=
        action_record.observed_process_instance_id;
    observed_lease_epoch := action_record.observed_lease_epoch;
    observed_runtime_build_revision :=
        action_record.observed_runtime_build_revision;
    observed_owner_revision := action_record.observed_owner_revision;
    database_now := action_record.database_now;
    observed_owner_expires_at :=
        action_record.observed_owner_expires_at;
    minimum_database_now := requested_minimum_database_now;
    recorded_at := action_record.recorded_at;
    terminal_digest := action_record.terminal_digest;
    RETURN NEXT;
END;
$function$;

REVOKE ALL ON FUNCTION
    public.starring_runtime_startup_recovery_execute_stale_live_v2(
        TEXT,
        BIGINT,
        BIGINT,
        BIGINT,
        BIGINT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        BIGINT,
        TIMESTAMPTZ,
        TIMESTAMPTZ
    )
FROM PUBLIC;

DO $execution_acl$
DECLARE
    common_owner OID;
    executor_role OID;
    executor_name NAME;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
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

    IF common_owner IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_stale_live_execution_acl_drift';
    END IF;

    IF executor_role IS NOT NULL THEN
        executor_name := pg_catalog.pg_get_userbyid(executor_role);
        IF executor_name IS NULL THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_startup_stale_live_execution_acl_drift';
        END IF;
        EXECUTE pg_catalog.format(
            'GRANT EXECUTE ON FUNCTION public.starring_runtime_startup_recovery_execute_stale_live_v2(TEXT,BIGINT,BIGINT,BIGINT,BIGINT,TEXT,TEXT,BIGINT,TEXT,BIGINT,TIMESTAMPTZ,TIMESTAMPTZ) TO %I',
            executor_name
        );
    END IF;
END;
$execution_acl$;

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
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_startup_recovery_terminal_digest_v2(smallint,text,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,timestamp with time zone,bytea)''' || E'\n' ||
        '        )';
    next_fragment :=
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_startup_recovery_execute_stale_live_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_startup_recovery_terminal_digest_v2(smallint,text,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,timestamp with time zone,bytea)''' || E'\n' ||
        '        )';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_stale_live_execution_manifest_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        'RETURN observed_count = 768' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''e9fbf54f755c1a5ac234c69eea4252361146b69c032b655270e7306ea929e175'';';
    next_fragment :=
        'RETURN observed_count = 769' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''659a6609f468edabc6135b5b056d58ac1929ea223471155e05325ea0d6da5a87'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_stale_live_execution_manifest_expectation_patch_drift';
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
        '            (' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)'',' || E'\n' ||
        '                ''expected_gateway_shard_id text, expected_process_instance_id text, expected_lease_epoch bigint, expected_runtime_build_revision text, expected_owner_revision bigint, expected_owner_expires_at timestamp with time zone''::TEXT,' || E'\n' ||
        '                ''TABLE(outcome_name text, observed_gateway_shard_id text, observed_process_instance_id text, observed_lease_epoch bigint, observed_runtime_build_revision text, observed_owner_revision bigint, database_now timestamp with time zone, observed_owner_expires_at timestamp with time zone, serving_state_name text, serving_count bigint, serving_earliest_expiry timestamp with time zone, serving_retry_after_milliseconds bigint, recoverable_awaiting_certification_count bigint, suspended_local_effect_count bigint, pending_runtime_drain_intent_count bigint, acknowledged_product_handoff_count bigint)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            )' || E'\n' ||
        '    ) AS expected(';
    next_fragment :=
        '            (' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)'',' || E'\n' ||
        '                ''expected_gateway_shard_id text, expected_process_instance_id text, expected_lease_epoch bigint, expected_runtime_build_revision text, expected_owner_revision bigint, expected_owner_expires_at timestamp with time zone''::TEXT,' || E'\n' ||
        '                ''TABLE(outcome_name text, observed_gateway_shard_id text, observed_process_instance_id text, observed_lease_epoch bigint, observed_runtime_build_revision text, observed_owner_revision bigint, database_now timestamp with time zone, observed_owner_expires_at timestamp with time zone, serving_state_name text, serving_count bigint, serving_earliest_expiry timestamp with time zone, serving_retry_after_milliseconds bigint, recoverable_awaiting_certification_count bigint, suspended_local_effect_count bigint, pending_runtime_drain_intent_count bigint, acknowledged_product_handoff_count bigint)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            ),' || E'\n' ||
        '            (' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_execute_stale_live_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)'',' || E'\n' ||
        '                ''requested_recovery_id text, requested_originating_emergency_generation bigint, requested_coordinator_generation bigint, requested_action_authority_revision bigint, requested_selection_authority_revision bigint, expected_gateway_shard_id text, expected_owner_process_instance_id text, expected_owner_lease_epoch bigint, expected_owner_runtime_build_revision text, expected_owner_revision bigint, expected_owner_expires_at timestamp with time zone, requested_minimum_database_now timestamp with time zone''::TEXT,' || E'\n' ||
        '                ''TABLE(journal_outcome_name text, terminal_outcome_name text, recovery_id text, originating_emergency_generation bigint, coordinator_generation bigint, action_authority_revision bigint, selection_authority_revision bigint, recovery_class text, observed_gateway_shard_id text, observed_process_instance_id text, observed_lease_epoch bigint, observed_runtime_build_revision text, observed_owner_revision bigint, database_now timestamp with time zone, observed_owner_expires_at timestamp with time zone, minimum_database_now timestamp with time zone, recorded_at timestamp with time zone, terminal_projection_bytes bytea, terminal_digest text)''::TEXT,' || E'\n' ||
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
            MESSAGE = 'runtime_startup_stale_live_execution_readiness_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '''c76a82cdd88a75259889d4cab4543797ad834d8f2e38f71268bbbc4b0e4cae0f''::TEXT';
    next_fragment :=
        '''00824784a0b0276e2ef83b4e4094c274cffb50b9c640af61350a152dc112c835''::TEXT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_stale_live_execution_readiness_manifest_digest_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)''' || E'\n' ||
        '            )' || E'\n' ||
        '        )';
    next_fragment :=
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)''' || E'\n' ||
        '            ),' || E'\n' ||
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_execute_stale_live_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)''' || E'\n' ||
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
            MESSAGE = 'runtime_startup_stale_live_execution_readiness_allowlist_patch_drift';
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
    manifest_digest TEXT;
    readiness_digest TEXT;
    action_record_digest TEXT;
    terminal_constraint_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
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
        capability.proconfig,
        pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(capability.oid),
                'UTF8'
            )),
            'hex'
        ) AS definition_digest
    INTO function_row
    FROM pg_catalog.pg_proc AS capability
    WHERE capability.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_startup_recovery_execute_stale_live_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)'
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

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(capability.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO action_record_digest
    FROM pg_catalog.pg_proc AS capability
    WHERE capability.oid = pg_catalog.to_regprocedure(
        'starring_runtime_private_v2.starring_runtime_startup_recovery_action_record_v2(text,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,bytea)'
    );

    SELECT pg_catalog.count(*)
    INTO terminal_constraint_count
    FROM pg_catalog.pg_constraint AS constraint_row
    WHERE constraint_row.conrelid = pg_catalog.to_regclass(
            'public.runtime_startup_recovery_actions_v2'
        )
        AND constraint_row.conname =
            'runtime_startup_recovery_actions_v2_terminal_check'
        AND constraint_row.contype = 'c'
        AND constraint_row.convalidated
        AND pg_catalog.pg_get_constraintdef(
            constraint_row.oid,
            TRUE
        ) = 'CHECK (octet_length(terminal_projection_bytes) >= 1 AND octet_length(terminal_projection_bytes) <= 1048576 AND terminal_digest ~ ''^[0-9a-f]{64}$''::text AND terminal_digest <> repeat(''0''::text, 64) AND terminal_digest = starring_runtime_private_v2.starring_runtime_startup_recovery_terminal_digest_v2(record_format_version, recovery_id, originating_emergency_generation, coordinator_generation, action_authority_revision, selection_authority_revision, recovery_class, gateway_shard_id, owner_process_instance_id, owner_lease_epoch, owner_runtime_build_revision, owner_revision, owner_expires_at, minimum_database_now, recorded_at, terminal_projection_bytes))';

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
        OR function_row.definition_digest IS DISTINCT FROM
            'de30f26d122062ad9da6fc9bd145a7376030fa7e1c9d114db740056e33136a42'
        OR invalid_acl_count <> 0
        OR (
            executor_role IS NOT NULL
            AND NOT pg_catalog.has_function_privilege(
                executor_role,
                function_row.oid,
                'EXECUTE'
            )
        )
        OR manifest_digest IS DISTINCT FROM
            '00824784a0b0276e2ef83b4e4094c274cffb50b9c640af61350a152dc112c835'
        OR readiness_digest IS DISTINCT FROM
            'c2cba3c5591876238f0ae0248b2c7c205953b6cde2a62705038a42fa9aa2aa81'
        OR action_record_digest IS DISTINCT FROM
            '7f3f6f98d37150b86d3d4ff860018053b402afa27eeaee82694a6dbf4f0e301b'
        OR terminal_constraint_count <> 1
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_stale_live_execution_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
