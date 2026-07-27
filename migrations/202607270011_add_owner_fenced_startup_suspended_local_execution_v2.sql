SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
);

LOCK TABLE
    public.runtime_writer_fence,
    public.runtime_gateway_owners,
    public.runtime_startup_recovery_actions_v2,
    public.runtime_deployments,
    public.runtime_serving_leases,
    public.runtime_slot_writer_fences_v2,
    public.runtime_certification_operations_v2,
    public.runtime_certification_operation_terminals_v2,
    public.runtime_suspend_attempt_operations_v2,
    public.runtime_suspended_attempts_v2,
    public.runtime_suspend_attempt_completions_v2,
    public.runtime_product_operations_v2,
    public.runtime_drain_intents_v2
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
    FROM (
        VALUES
            ('starring_runtime_private_v2.starring_runtime_suspended_route_bytes_v2(jsonb)'),
            ('starring_runtime_private_v2.starring_runtime_suspended_previous_bytes_v2(jsonb)'),
            ('starring_runtime_private_v2.starring_runtime_suspended_root_exact_v2(public.runtime_suspend_attempt_operations_v2,public.runtime_suspended_attempts_v2)'),
            ('starring_runtime_private_v2.starring_runtime_suspended_root_frame_v2(public.runtime_suspend_attempt_operations_v2,public.runtime_deployments)'),
            ('starring_runtime_private_v2.starring_runtime_suspended_sidecar_frame_v2(public.runtime_suspended_attempts_v2)'),
            ('starring_runtime_private_v2.starring_runtime_suspended_projection_exact_v2(bytea,bytea,bytea,bytea,bytea,bytea)'),
            ('starring_runtime_private_v2.starring_runtime_suspended_replay_exact_v2(bytea,bytea)'),
            ('starring_runtime_private_v2.starring_runtime_suspended_terminal_sidecar_v2(bytea,bytea,public.runtime_suspend_attempt_operations_v2,public.runtime_suspended_attempts_v2)'),
            ('starring_runtime_private_v2.starring_runtime_suspended_quiescent_exact_v2(public.runtime_suspend_attempt_operations_v2,public.runtime_suspended_attempts_v2)'),
            ('public.starring_runtime_startup_recovery_execute_suspended_local_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint)')
    ) AS expected(identity)
    WHERE pg_catalog.to_regprocedure(expected.identity) IS NOT NULL;

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
            'c2de6cf64ce6efbcf22e31f06da774195996060a692c45b48f073ff93fa4d630'
        OR readiness_digest IS DISTINCT FROM
            '4e58c914016de080372586cc2efc7e9a5221c8703450d767934389a5c4c07db8'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_suspended_local_preflight_drift';
    END IF;
END;
$preflight$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_suspended_route_bytes_v2(
    route_value JSONB
)
RETURNS BYTEA
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    identity_value JSONB;
    target_value JSONB;
    guild_id TEXT;
    ruleset_key TEXT;
    version_value TEXT;
    content_hash TEXT;
    binding_revision TEXT;
    binding_fingerprint TEXT;
    runtime_generation TEXT;
    process_instance_id TEXT;
    fencing_token TEXT;
    route_incarnation TEXT;
    canonical_text TEXT;
BEGIN
    identity_value := route_value -> 'identity';
    target_value := identity_value -> 'target';
    guild_id := target_value ->> 'guild_id';
    ruleset_key := target_value ->> 'ruleset_key';
    version_value := target_value ->> 'version';
    content_hash := target_value ->> 'content_hash';
    binding_revision := target_value ->> 'binding_revision';
    binding_fingerprint := target_value ->> 'binding_fingerprint';
    runtime_generation := identity_value ->> 'runtime_generation';
    process_instance_id := identity_value ->> 'process_instance_id';
    fencing_token := route_value ->> 'controller_fencing_token';
    route_incarnation := route_value ->> 'route_incarnation';

    IF pg_catalog.jsonb_typeof(route_value) <> 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(route_value)
        ) <> 3
        OR pg_catalog.jsonb_typeof(identity_value) <> 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(identity_value)
        ) <> 3
        OR pg_catalog.jsonb_typeof(target_value) <> 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(target_value)
        ) <> 6
        OR pg_catalog.jsonb_typeof(target_value -> 'guild_id') <> 'string'
        OR guild_id !~ '^[1-9][0-9]{0,19}$'
        OR (
            pg_catalog.length(guild_id) = 20
            AND guild_id COLLATE pg_catalog."C"
                > '18446744073709551615' COLLATE pg_catalog."C"
        )
        OR pg_catalog.jsonb_typeof(target_value -> 'ruleset_key') <> 'string'
        OR ruleset_key !~ '^[A-Za-z0-9_-]{1,64}$'
        OR pg_catalog.jsonb_typeof(target_value -> 'version') <> 'number'
        OR version_value !~ '^[1-9][0-9]{0,9}$'
        OR version_value::NUMERIC > 4294967295
        OR pg_catalog.jsonb_typeof(target_value -> 'content_hash') <> 'string'
        OR content_hash !~ '^[0-9a-f]{64}$'
        OR pg_catalog.jsonb_typeof(
            target_value -> 'binding_revision'
        ) <> 'number'
        OR binding_revision !~ '^[1-9][0-9]{0,18}$'
        OR binding_revision::NUMERIC > 9223372036854775807
        OR pg_catalog.jsonb_typeof(
            target_value -> 'binding_fingerprint'
        ) <> 'string'
        OR binding_fingerprint !~ '^[0-9a-f]{64}$'
        OR pg_catalog.jsonb_typeof(
            identity_value -> 'runtime_generation'
        ) <> 'number'
        OR runtime_generation !~ '^[1-9][0-9]{0,18}$'
        OR runtime_generation::NUMERIC > 9223372036854775807
        OR pg_catalog.jsonb_typeof(
            identity_value -> 'process_instance_id'
        ) <> 'string'
        OR process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR pg_catalog.jsonb_typeof(
            route_value -> 'controller_fencing_token'
        ) <> 'number'
        OR fencing_token !~ '^[1-9][0-9]{0,18}$'
        OR fencing_token::NUMERIC > 9223372036854775807
        OR pg_catalog.jsonb_typeof(
            route_value -> 'route_incarnation'
        ) <> 'number'
        OR route_incarnation !~ '^[1-9][0-9]{0,18}$'
        OR route_incarnation::NUMERIC > 9223372036854775807
    THEN
        RETURN NULL;
    END IF;

    canonical_text := pg_catalog.concat(
        '{"identity":{"target":{"guild_id":',
        pg_catalog.to_json(guild_id)::TEXT,
        ',"ruleset_key":',
        pg_catalog.to_json(ruleset_key)::TEXT,
        ',"version":',
        version_value,
        ',"content_hash":',
        pg_catalog.to_json(content_hash)::TEXT,
        ',"binding_revision":',
        binding_revision,
        ',"binding_fingerprint":',
        pg_catalog.to_json(binding_fingerprint)::TEXT,
        '},"runtime_generation":',
        runtime_generation,
        ',"process_instance_id":',
        pg_catalog.to_json(process_instance_id)::TEXT,
        '},"controller_fencing_token":',
        fencing_token,
        ',"route_incarnation":',
        route_incarnation,
        '}'
    );
    RETURN pg_catalog.convert_to(canonical_text, 'UTF8');
EXCEPTION
    WHEN OTHERS THEN
        RETURN NULL;
END;
$function$;

CREATE FUNCTION public.starring_runtime_startup_recovery_execute_suspended_local_v2(
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
    requested_minimum_database_now TIMESTAMPTZ,
    paused_process_instance_id TEXT,
    paused_coordinator_generation BIGINT,
    paused_connection_epoch BIGINT,
    paused_ready_kind TEXT,
    paused_admission_revision BIGINT,
    paused_transition_sequence BIGINT,
    paused_connected_event_sequence BIGINT,
    paused_last_resume_sequence BIGINT,
    registry_process_instance_id TEXT,
    registry_observation_sequence BIGINT,
    registry_retained_slot_count BIGINT,
    registry_retained_empty_tombstone_count BIGINT
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
    root_row public.runtime_suspend_attempt_operations_v2%ROWTYPE;
    source_sidecar_row public.runtime_suspended_attempts_v2%ROWTYPE;
    successor_sidecar_row public.runtime_suspended_attempts_v2%ROWTYPE;
    updated_sidecar_row public.runtime_suspended_attempts_v2%ROWTYPE;
    deployment_row public.runtime_deployments%ROWTYPE;
    slot_fence_row public.runtime_slot_writer_fences_v2%ROWTYPE;
    action_record RECORD;
    selection_action_found BOOLEAN;
    authority_action_found BOOLEAN;
    writer_fence_count BIGINT;
    invalid_ledger_count BIGINT;
    invalid_exact_count BIGINT;
    exact_route_count BIGINT;
    higher_live_count BIGINT;
    higher_reservation_count BIGINT;
    same_slot_drain_count BIGINT;
    selected_suspension_id TEXT;
    root_value JSONB;
    local_value JSONB;
    drain_value JSONB;
    route_value JSONB;
    previous_value JSONB;
    route_bytes BYTEA;
    previous_bytes BYTEA;
    successor_local_bytes BYTEA;
    successor_drain_bytes BYTEA;
    provenance_frame BYTEA;
    evidence_frame BYTEA;
    root_frame BYTEA;
    source_frame BYTEA;
    successor_frame BYTEA;
    domain_bytes BYTEA;
    no_candidate_projection BYTEA;
    progressed_projection BYTEA;
    owner_expiry_unix_microseconds BIGINT;
    owner_expiry_numeric NUMERIC;
    failure_recorded_numeric NUMERIC;
    ready_kind_tag SMALLINT;
    last_resume_frame BYTEA;
    source_evidence_text TEXT;
    source_evidence_at TIMESTAMPTZ;
    recovered_evidence_text TEXT;
    recovered_evidence_at TIMESTAMPTZ;
BEGIN
    PERFORM pg_catalog.set_config('TimeZone', 'UTC', TRUE);

    IF pg_catalog.current_setting('transaction_isolation')
            <> 'serializable'
        OR pg_catalog.current_setting('transaction_read_only') <> 'off'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_suspended_local_transaction_invalid';
    END IF;

    IF requested_recovery_id !~ '^[0-9a-f]{32}$'
        OR requested_originating_emergency_generation
            NOT BETWEEN 1 AND 9223372036854775806
        OR requested_coordinator_generation
            IS DISTINCT FROM
                requested_originating_emergency_generation + 1
        OR requested_selection_authority_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR requested_action_authority_revision
            IS DISTINCT FROM requested_selection_authority_revision + 1
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
        OR paused_process_instance_id
            IS DISTINCT FROM expected_owner_process_instance_id
        OR paused_coordinator_generation
            IS DISTINCT FROM requested_originating_emergency_generation
        OR paused_connection_epoch
            NOT BETWEEN 1 AND 9223372036854775807
        OR paused_ready_kind NOT IN ('ready', 'resumed')
        OR paused_admission_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR paused_transition_sequence
            NOT BETWEEN 1 AND 9223372036854775807
        OR paused_connected_event_sequence
            NOT BETWEEN 1 AND 9223372036854775807
        OR paused_transition_sequence
            <= paused_connected_event_sequence
        OR paused_last_resume_sequence
            NOT BETWEEN 0 AND 9223372036854775807
        OR (
            paused_last_resume_sequence <> 0
            AND (
                paused_last_resume_sequence
                    <= paused_connected_event_sequence
                OR paused_last_resume_sequence
                    > paused_transition_sequence
            )
        )
        OR registry_process_instance_id
            IS DISTINCT FROM expected_owner_process_instance_id
        OR registry_observation_sequence
            NOT BETWEEN 1 AND 9223372036854775807
        OR registry_retained_slot_count
            NOT BETWEEN 0 AND 9223372036854775807
        OR registry_retained_empty_tombstone_count
            IS DISTINCT FROM registry_retained_slot_count
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_startup_suspended_local_input_invalid';
    END IF;

    ready_kind_tag := CASE paused_ready_kind
        WHEN 'ready' THEN 1
        ELSE 2
    END;
    last_resume_frame := CASE
        WHEN paused_last_resume_sequence = 0
        THEN pg_catalog.int2send(0::SMALLINT)
        ELSE
            pg_catalog.int2send(1::SMALLINT)
            || pg_catalog.int8send(paused_last_resume_sequence)
    END;
    evidence_frame :=
        pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(paused_process_instance_id, 'UTF8')
            )::BIGINT
        )
        || pg_catalog.convert_to(paused_process_instance_id, 'UTF8')
        || pg_catalog.int8send(paused_coordinator_generation)
        || pg_catalog.int8send(paused_connection_epoch)
        || pg_catalog.int2send(ready_kind_tag)
        || pg_catalog.int8send(paused_admission_revision)
        || pg_catalog.int8send(paused_transition_sequence)
        || pg_catalog.int8send(paused_connected_event_sequence)
        || last_resume_frame
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(registry_process_instance_id, 'UTF8')
            )::BIGINT
        )
        || pg_catalog.convert_to(registry_process_instance_id, 'UTF8')
        || pg_catalog.int8send(registry_observation_sequence)
        || pg_catalog.int8send(registry_retained_slot_count)
        || pg_catalog.int8send(
            registry_retained_empty_tombstone_count
        );
    domain_bytes := pg_catalog.convert_to(
        'starring.runtime.startup_recovery.suspended_local_effect.terminal.v2',
        'UTF8'
    );
    no_candidate_projection :=
        pg_catalog.int8send(
            pg_catalog.octet_length(domain_bytes)::BIGINT
        )
        || domain_bytes
        || pg_catalog.int2send(2::SMALLINT)
        || pg_catalog.int2send(0::SMALLINT)
        || pg_catalog.int8send(
            pg_catalog.octet_length(evidence_frame)::BIGINT
        )
        || evidence_frame
        || pg_catalog.sha256(evidence_frame);

    PERFORM pg_catalog.pg_advisory_xact_lock_shared(
        pg_catalog.hashtextextended(
            'starring-runtime-writer-fence-v1',
            0
        )
    );
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
            MESSAGE = 'runtime_startup_suspended_local_owner_lost';
    END IF;
    IF database_now < requested_minimum_database_now THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_suspended_local_clock_regressed';
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
        IF NOT starring_runtime_private_v2.starring_runtime_suspended_replay_exact_v2(
            existing_action_row.terminal_projection_bytes,
            evidence_frame
        ) THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_startup_suspended_local_replay_invalid';
        END IF;

        SELECT record.*
        INTO STRICT action_record
        FROM starring_runtime_private_v2.starring_runtime_startup_recovery_action_record_v2(
            requested_recovery_id,
            requested_originating_emergency_generation,
            requested_coordinator_generation,
            requested_action_authority_revision,
            requested_selection_authority_revision,
            'suspended_local_effect',
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
            OR action_record.database_now >= expected_owner_expires_at
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_startup_suspended_local_replay_invalid';
        END IF;

        terminal_outcome_name := CASE
            WHEN existing_action_row.terminal_projection_bytes
                IS NOT DISTINCT FROM no_candidate_projection
            THEN 'no_candidate'
            ELSE 'progressed'
        END;
        journal_outcome_name := action_record.outcome_name;
        recovery_id := requested_recovery_id;
        originating_emergency_generation :=
            requested_originating_emergency_generation;
        coordinator_generation := requested_coordinator_generation;
        action_authority_revision :=
            requested_action_authority_revision;
        selection_authority_revision :=
            requested_selection_authority_revision;
        recovery_class := 'suspended_local_effect';
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
    IF writer_fence_count <> 1
        OR (
            SELECT pg_catalog.count(*)
            FROM public.runtime_writer_fence
        ) <> 1
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_suspended_local_state_ambiguous';
    END IF;

    SELECT pg_catalog.count(*)
    INTO higher_live_count
    FROM public.runtime_deployments AS deployment
    WHERE deployment.phase = 'live';
    SELECT pg_catalog.count(*)
    INTO higher_reservation_count
    FROM public.runtime_certification_operations_v2 AS reservation
    LEFT JOIN public.runtime_certification_operation_terminals_v2 AS terminal
        ON terminal.operation_id = reservation.operation_id
    WHERE terminal.operation_id IS NULL;
    IF higher_live_count <> 0 OR higher_reservation_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_startup_suspended_local_higher_priority';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_ledger_count
    FROM public.runtime_suspend_attempt_operations_v2 AS root
    LEFT JOIN public.runtime_suspended_attempts_v2 AS suspended
        ON suspended.suspension_id = root.suspension_id
    LEFT JOIN public.runtime_suspend_attempt_completions_v2 AS completion
        ON completion.suspension_id = root.suspension_id
    WHERE (
            CASE WHEN suspended.suspension_id IS NULL THEN 0 ELSE 1 END
            + CASE WHEN completion.suspension_id IS NULL THEN 0 ELSE 1 END
        ) <> 1;
    SELECT pg_catalog.count(*)
    INTO invalid_exact_count
    FROM public.runtime_suspended_attempts_v2 AS suspended
    INNER JOIN public.runtime_suspend_attempt_operations_v2 AS root
        ON root.suspension_id = suspended.suspension_id
    WHERE (
            suspended.local_effect_kind = 'exact_route'
            AND NOT starring_runtime_private_v2.starring_runtime_suspended_root_exact_v2(
                root,
                suspended
            )
        )
        OR (
            suspended.local_effect_kind = 'none'
            AND NOT starring_runtime_private_v2.starring_runtime_suspended_quiescent_exact_v2(
                root,
                suspended
            )
        )
        OR (
            suspended.local_effect_kind = 'route_absent'
            AND NOT EXISTS (
                SELECT 1
                FROM public.runtime_startup_recovery_actions_v2 AS action
                WHERE action.recovery_class =
                        'suspended_local_effect'
                    AND starring_runtime_private_v2.starring_runtime_suspended_terminal_sidecar_v2(
                        action.terminal_projection_bytes,
                        starring_runtime_private_v2.starring_runtime_suspended_sidecar_frame_v2(
                            suspended
                        ),
                        root,
                        suspended
                    )
            )
        );
    IF invalid_ledger_count <> 0 OR invalid_exact_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_suspended_local_state_ambiguous';
    END IF;

    SELECT
        pg_catalog.count(*),
        (
            SELECT suspended.suspension_id
            FROM public.runtime_suspended_attempts_v2 AS suspended
            WHERE suspended.local_effect_kind = 'exact_route'
            ORDER BY
                suspended.suspended_at,
                suspended.suspension_id COLLATE pg_catalog."C"
            LIMIT 1
        )
    INTO exact_route_count, selected_suspension_id
    FROM public.runtime_suspended_attempts_v2 AS suspended
    WHERE suspended.local_effect_kind = 'exact_route';

    IF exact_route_count = 0 THEN
        IF selected_suspension_id IS NOT NULL THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_startup_suspended_local_selection_invalid';
        END IF;
        SELECT record.*
        INTO STRICT action_record
        FROM starring_runtime_private_v2.starring_runtime_startup_recovery_action_record_v2(
            requested_recovery_id,
            requested_originating_emergency_generation,
            requested_coordinator_generation,
            requested_action_authority_revision,
            requested_selection_authority_revision,
            'suspended_local_effect',
            expected_gateway_shard_id,
            expected_owner_process_instance_id,
            expected_owner_lease_epoch,
            expected_owner_runtime_build_revision,
            expected_owner_revision,
            expected_owner_expires_at,
            requested_minimum_database_now,
            no_candidate_projection
        ) AS record;
        IF action_record.outcome_name IS DISTINCT FROM 'applied' THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_startup_suspended_local_record_invalid';
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
        recovery_class := 'suspended_local_effect';
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

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            pg_catalog.concat(
                'starring-runtime-suspend-attempt-v2:',
                selected_suspension_id
            ),
            0
        )
    );
    SELECT root.*
    INTO root_row
    FROM public.runtime_suspend_attempt_operations_v2 AS root
    WHERE root.suspension_id = selected_suspension_id
    FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_suspended_local_candidate_invalid';
    END IF;
    SELECT suspended.*
    INTO source_sidecar_row
    FROM public.runtime_suspended_attempts_v2 AS suspended
    WHERE suspended.suspension_id = selected_suspension_id
    FOR UPDATE;
    IF NOT FOUND
        OR NOT starring_runtime_private_v2.starring_runtime_suspended_root_exact_v2(
            root_row,
            source_sidecar_row
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_suspended_local_candidate_invalid';
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            pg_catalog.concat(
                'starring-runtime-serving-slot-v1:',
                source_sidecar_row.slot_guild_id,
                ':',
                source_sidecar_row.slot_ruleset_key
            ),
            0
        )
    );
    SELECT slot.*
    INTO slot_fence_row
    FROM public.runtime_slot_writer_fences_v2 AS slot
    WHERE slot.slot_guild_id = source_sidecar_row.slot_guild_id
        AND slot.slot_ruleset_key =
            source_sidecar_row.slot_ruleset_key
    FOR UPDATE;
    IF NOT FOUND
        OR slot_fence_row.writer_epoch
            NOT BETWEEN 1 AND 9223372036854775807
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_suspended_local_slot_invalid';
    END IF;
    SELECT pg_catalog.count(*)
    INTO same_slot_drain_count
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE drain.slot_guild_id = source_sidecar_row.slot_guild_id
        AND drain.slot_ruleset_key =
            source_sidecar_row.slot_ruleset_key
        AND drain.intent_state = 'pending';
    IF (
            slot_fence_row.pending_drain_intent_id IS NULL
            AND same_slot_drain_count = 0
        )
    THEN
        NULL;
    ELSIF slot_fence_row.pending_drain_intent_id IS NOT NULL
        AND same_slot_drain_count = 1
        AND EXISTS (
            SELECT 1
            FROM public.runtime_drain_intents_v2 AS drain
            WHERE drain.drain_intent_id =
                    slot_fence_row.pending_drain_intent_id
                AND drain.slot_guild_id =
                    source_sidecar_row.slot_guild_id
                AND drain.slot_ruleset_key =
                    source_sidecar_row.slot_ruleset_key
                AND drain.intent_state = 'pending'
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX007',
            MESSAGE = 'runtime_startup_suspended_local_pending_drain';
    ELSE
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_suspended_local_drain_state_invalid';
    END IF;

    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = root_row.tenant_id
        AND deployment.installation_id = root_row.installation_id
        AND deployment.deployment_id = root_row.deployment_id
    FOR UPDATE;
    root_value := pg_catalog.convert_from(
        root_row.suspend_attempt_request_bytes,
        'UTF8'
    )::JSONB;
    source_evidence_text := CASE root_value ->> 'source_phase'
        WHEN 'requested'
        THEN deployment_row.snapshot ->> 'requested_at'
        WHEN 'preflight_ready'
        THEN deployment_row.snapshot #>> '{preflight,checked_at}'
        WHEN 'drain_requested'
        THEN deployment_row.snapshot #>> '{preflight,checked_at}'
        WHEN 'drained'
        THEN deployment_row.snapshot #>> '{drain,drained_at}'
        WHEN 'activation_applying'
        THEN deployment_row.snapshot #>> '{drain,drained_at}'
        WHEN 'runtime_pending_ready'
        THEN deployment_row.snapshot #>> '{activation,activated_at}'
        WHEN 'reconciling_panels'
        THEN deployment_row.snapshot #>> '{activation,activated_at}'
        ELSE NULL
    END;
    recovered_evidence_text :=
        deployment_row.snapshot #>> '{last_live_recovery,recovered_at}';
    IF NOT FOUND
        OR deployment_row.revision
            IS DISTINCT FROM root_row.deployment_revision
        OR deployment_row.convergence_attempt_no
            IS DISTINCT FROM root_row.convergence_attempt_no
        OR deployment_row.guild_id
            IS DISTINCT FROM source_sidecar_row.slot_guild_id
        OR deployment_row.ruleset_key
            IS DISTINCT FROM source_sidecar_row.slot_ruleset_key
        OR deployment_row.controller_id
            IS DISTINCT FROM root_value #>> '{guard,controller_id}'
        OR deployment_row.controller_fencing_token::TEXT
            IS DISTINCT FROM root_value #>> '{guard,fencing_token}'
        OR deployment_row.last_controller_id
            IS DISTINCT FROM root_value #>> '{guard,controller_id}'
        OR deployment_row.last_fencing_token::TEXT
            IS DISTINCT FROM root_value #>> '{guard,fencing_token}'
        OR deployment_row.runtime_generation::TEXT
            IS DISTINCT FROM root_value #>> '{guard,runtime_generation}'
        OR deployment_row.target_version::TEXT
            IS DISTINCT FROM
                root_value #>>
                    '{local_effect,route,identity,target,version}'
        OR deployment_row.target_content_hash
            IS DISTINCT FROM
                root_value #>>
                    '{local_effect,route,identity,target,content_hash}'
        OR deployment_row.binding_revision::TEXT
            IS DISTINCT FROM
                root_value #>>
                    '{local_effect,route,identity,target,binding_revision}'
        OR deployment_row.binding_fingerprint
            IS DISTINCT FROM
                root_value #>>
                    '{local_effect,route,identity,target,binding_fingerprint}'
        OR deployment_row.snapshot #>> '{target,guild_id}'
            IS DISTINCT FROM deployment_row.guild_id
        OR deployment_row.snapshot #>> '{target,ruleset_key}'
            IS DISTINCT FROM deployment_row.ruleset_key
        OR deployment_row.snapshot #>> '{target,version}'
            IS DISTINCT FROM deployment_row.target_version::TEXT
        OR deployment_row.snapshot #>> '{target,content_hash}'
            IS DISTINCT FROM deployment_row.target_content_hash
        OR deployment_row.snapshot #>> '{target,binding_revision}'
            IS DISTINCT FROM deployment_row.binding_revision::TEXT
        OR deployment_row.snapshot #>> '{target,binding_fingerprint}'
            IS DISTINCT FROM deployment_row.binding_fingerprint
        OR deployment_row.snapshot #>> '{controller_lease,controller_id}'
            IS DISTINCT FROM root_value #>> '{guard,controller_id}'
        OR deployment_row.snapshot #>> '{controller_lease,fencing_token}'
            IS DISTINCT FROM root_value #>> '{guard,fencing_token}'
        OR deployment_row.snapshot ->> 'last_fencing_token'
            IS DISTINCT FROM root_value #>> '{guard,fencing_token}'
        OR NOT (
            (
                root_value ->> 'source_phase' IN (
                    'requested',
                    'preflight_ready',
                    'drain_requested',
                    'drained',
                    'activation_applying',
                    'reconciling_panels'
                )
                AND deployment_row.phase IS NOT DISTINCT FROM
                    root_value ->> 'source_phase'
            )
            OR (
                root_value ->> 'source_phase' =
                    'runtime_pending_ready'
                AND deployment_row.phase = 'runtime_pending'
                AND deployment_row.snapshot
                    #>> '{phase,condition,condition}' = 'ready'
            )
        )
        OR deployment_row.snapshot #>> '{phase,phase}'
            IS DISTINCT FROM deployment_row.phase
        OR deployment_row.snapshot ->> 'revision'
            IS DISTINCT FROM deployment_row.revision::TEXT
        OR deployment_row.snapshot ->> 'runtime_generation'
            IS DISTINCT FROM deployment_row.runtime_generation::TEXT
        OR source_evidence_text IS NULL
        OR NOT pg_catalog.pg_input_is_valid(
            source_evidence_text,
            'timestamp with time zone'
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_suspended_local_deployment_invalid';
    END IF;

    source_evidence_at := source_evidence_text::TIMESTAMPTZ;
    IF recovered_evidence_text IS NOT NULL THEN
        IF NOT pg_catalog.pg_input_is_valid(
            recovered_evidence_text,
            'timestamp with time zone'
        ) THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_startup_suspended_local_deployment_invalid';
        END IF;
        recovered_evidence_at :=
            recovered_evidence_text::TIMESTAMPTZ;
        IF root_value ->> 'source_phase' IN (
                'runtime_pending_ready',
                'reconciling_panels'
            )
        THEN
            source_evidence_at :=
                pg_catalog.greatest(
                    source_evidence_at,
                    recovered_evidence_at
                );
        END IF;
    END IF;
    failure_recorded_numeric :=
        root_value #>> '{failure,recorded_at_unix_microseconds}';
    IF failure_recorded_numeric
            < EXTRACT(EPOCH FROM source_evidence_at) * 1000000
        OR (
            source_sidecar_row.drain_obligation_kind =
                'exact_local_route'
            AND deployment_row.snapshot -> 'previous_runtime'
                IS DISTINCT FROM 'null'::JSONB
        )
        OR (
            source_sidecar_row.drain_obligation_kind =
                'local_and_previous'
            AND deployment_row.snapshot -> 'previous_runtime'
                IS DISTINCT FROM
                    root_value #> '{drain_obligation,previous,process}'
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_suspended_local_observation_invalid';
    END IF;

    owner_expiry_numeric :=
        EXTRACT(EPOCH FROM expected_owner_expires_at) * 1000000;
    IF owner_expiry_numeric NOT BETWEEN
        -9223372036854775808 AND 9223372036854775807
        OR owner_expiry_numeric <> pg_catalog.trunc(owner_expiry_numeric)
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_suspended_local_owner_time_invalid';
    END IF;
    owner_expiry_unix_microseconds := owner_expiry_numeric::BIGINT;
    provenance_frame := pg_catalog.convert_to(
        pg_catalog.concat(
            '{"kind":"closed_recovery","witness":{"recovery_id":',
            pg_catalog.to_json(requested_recovery_id)::TEXT,
            ',"originating_emergency_generation":',
            requested_originating_emergency_generation::TEXT,
            ',"recovery_generation":',
            requested_coordinator_generation::TEXT,
            ',"recovery_authority_revision":',
            requested_action_authority_revision::TEXT,
            ',"gateway_owner_lease_id":{"gateway_shard_id":',
            pg_catalog.to_json(expected_gateway_shard_id)::TEXT,
            ',"process_instance_id":',
            pg_catalog.to_json(expected_owner_process_instance_id)::TEXT,
            ',"lease_epoch":',
            expected_owner_lease_epoch::TEXT,
            ',"expected_build_revision":',
            pg_catalog.to_json(expected_owner_runtime_build_revision)::TEXT,
            '},"observed_owner_revision":',
            expected_owner_revision::TEXT,
            ',"owner_expires_at_unix_microseconds":',
            owner_expiry_unix_microseconds::TEXT,
            ',"process_instance_id":',
            pg_catalog.to_json(paused_process_instance_id)::TEXT,
            ',"connection_epoch":',
            paused_connection_epoch::TEXT,
            ',"paused_admission_revision":',
            paused_admission_revision::TEXT,
            ',"connected_event_sequence":',
            paused_connected_event_sequence::TEXT,
            ',"pause_sequence":',
            paused_transition_sequence::TEXT,
            '}}'
        ),
        'UTF8'
    );
    local_value := root_value -> 'local_effect';
    drain_value := root_value -> 'drain_obligation';
    route_value := local_value -> 'route';
    route_bytes :=
        starring_runtime_private_v2.starring_runtime_suspended_route_bytes_v2(
            route_value
        );
    successor_local_bytes := pg_catalog.convert_to(
        pg_catalog.concat(
            '{"kind":"route_absent","slot":{"guild_id":',
            pg_catalog.to_json(source_sidecar_row.slot_guild_id)::TEXT,
            ',"ruleset_key":',
            pg_catalog.to_json(source_sidecar_row.slot_ruleset_key)::TEXT,
            '},"expected_route":',
            pg_catalog.convert_from(route_bytes, 'UTF8'),
            ',"provenance":',
            pg_catalog.convert_from(provenance_frame, 'UTF8'),
            ',"observed_sequence":',
            registry_observation_sequence::TEXT,
            '}'
        ),
        'UTF8'
    );
    IF source_sidecar_row.drain_obligation_kind =
            'exact_local_route'
    THEN
        successor_drain_bytes :=
            pg_catalog.convert_to('{"kind":"none"}', 'UTF8');
    ELSE
        previous_value := drain_value -> 'previous';
        previous_bytes :=
            starring_runtime_private_v2.starring_runtime_suspended_previous_bytes_v2(
                previous_value
            );
        successor_drain_bytes := pg_catalog.convert_to(
            pg_catalog.concat(
                '{"kind":"previous_serving","previous":',
                pg_catalog.convert_from(previous_bytes, 'UTF8'),
                '}'
            ),
            'UTF8'
        );
    END IF;
    IF route_bytes IS NULL
        OR successor_local_bytes IS NULL
        OR successor_drain_bytes IS NULL
        OR pg_catalog.octet_length(successor_local_bytes)
            NOT BETWEEN 1 AND 131072
        OR pg_catalog.octet_length(successor_drain_bytes)
            NOT BETWEEN 1 AND 131072
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_suspended_local_successor_invalid';
    END IF;

    successor_sidecar_row := source_sidecar_row;
    successor_sidecar_row.sidecar_revision :=
        source_sidecar_row.sidecar_revision + 1;
    successor_sidecar_row.local_effect_kind := 'route_absent';
    successor_sidecar_row.local_effect_bytes :=
        successor_local_bytes;
    IF source_sidecar_row.drain_obligation_kind =
            'exact_local_route'
    THEN
        successor_sidecar_row.drain_obligation_kind := 'none';
    ELSE
        successor_sidecar_row.drain_obligation_kind :=
            'previous_serving';
    END IF;
    successor_sidecar_row.drain_obligation_bytes :=
        successor_drain_bytes;

    root_frame :=
        starring_runtime_private_v2.starring_runtime_suspended_root_frame_v2(
            root_row,
            deployment_row
        );
    source_frame :=
        starring_runtime_private_v2.starring_runtime_suspended_sidecar_frame_v2(
            source_sidecar_row
        );
    successor_frame :=
        starring_runtime_private_v2.starring_runtime_suspended_sidecar_frame_v2(
            successor_sidecar_row
        );
    progressed_projection :=
        pg_catalog.int8send(
            pg_catalog.octet_length(domain_bytes)::BIGINT
        )
        || domain_bytes
        || pg_catalog.int2send(2::SMALLINT)
        || pg_catalog.int2send(1::SMALLINT)
        || pg_catalog.int8send(
            pg_catalog.octet_length(root_frame)::BIGINT
        )
        || root_frame
        || pg_catalog.int8send(
            pg_catalog.octet_length(source_frame)::BIGINT
        )
        || source_frame
        || pg_catalog.int8send(
            pg_catalog.octet_length(successor_frame)::BIGINT
        )
        || successor_frame
        || pg_catalog.int8send(
            pg_catalog.octet_length(provenance_frame)::BIGINT
        )
        || provenance_frame
        || pg_catalog.int8send(
            pg_catalog.octet_length(evidence_frame)::BIGINT
        )
        || evidence_frame
        || pg_catalog.sha256(
            root_frame
            || source_frame
            || successor_frame
            || provenance_frame
            || evidence_frame
        );
    IF pg_catalog.octet_length(progressed_projection)
            NOT BETWEEN 1 AND 1048576
        OR NOT starring_runtime_private_v2.starring_runtime_suspended_projection_exact_v2(
            progressed_projection,
            root_frame,
            source_frame,
            successor_frame,
            provenance_frame,
            evidence_frame
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_suspended_local_projection_invalid';
    END IF;

    PERFORM pg_catalog.set_config(
        'starring.runtime_suspended_local_gate_v2',
        'armed',
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_suspended_local_id_v2',
        source_sidecar_row.suspension_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_suspended_local_source_v2',
        pg_catalog.encode(pg_catalog.sha256(source_frame), 'hex'),
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_suspended_local_successor_v2',
        pg_catalog.encode(pg_catalog.sha256(successor_frame), 'hex'),
        TRUE
    );
    UPDATE public.runtime_suspended_attempts_v2 AS suspended
    SET
        sidecar_revision = successor_sidecar_row.sidecar_revision,
        local_effect_kind = successor_sidecar_row.local_effect_kind,
        local_effect_bytes = successor_sidecar_row.local_effect_bytes,
        drain_obligation_kind =
            successor_sidecar_row.drain_obligation_kind,
        drain_obligation_bytes =
            successor_sidecar_row.drain_obligation_bytes
    WHERE suspended.suspension_id =
            source_sidecar_row.suspension_id
        AND suspended.sidecar_revision =
            source_sidecar_row.sidecar_revision
        AND suspended.local_effect_kind =
            source_sidecar_row.local_effect_kind
        AND suspended.local_effect_bytes =
            source_sidecar_row.local_effect_bytes
        AND suspended.drain_obligation_kind =
            source_sidecar_row.drain_obligation_kind
        AND suspended.drain_obligation_bytes =
            source_sidecar_row.drain_obligation_bytes
    RETURNING suspended.*
    INTO updated_sidecar_row;
    IF NOT FOUND
        OR updated_sidecar_row IS DISTINCT FROM successor_sidecar_row
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_startup_suspended_local_cas_conflict';
    END IF;

    SELECT record.*
    INTO STRICT action_record
    FROM starring_runtime_private_v2.starring_runtime_startup_recovery_action_record_v2(
        requested_recovery_id,
        requested_originating_emergency_generation,
        requested_coordinator_generation,
        requested_action_authority_revision,
        requested_selection_authority_revision,
        'suspended_local_effect',
        expected_gateway_shard_id,
        expected_owner_process_instance_id,
        expected_owner_lease_epoch,
        expected_owner_runtime_build_revision,
        expected_owner_revision,
        expected_owner_expires_at,
        requested_minimum_database_now,
        progressed_projection
    ) AS record;
    IF action_record.outcome_name IS DISTINCT FROM 'applied'
        OR action_record.database_now < database_now
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_suspended_local_record_invalid';
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
    recovery_class := 'suspended_local_effect';
    observed_gateway_shard_id := action_record.observed_gateway_shard_id;
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
    terminal_projection_bytes := progressed_projection;
    terminal_digest := action_record.terminal_digest;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_suspended_previous_bytes_v2(
    previous_value JSONB
)
RETURNS BYTEA
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    scope_value JSONB;
    process_value JSONB;
    route_value JSONB;
    route_bytes BYTEA;
    tenant_id TEXT;
    installation_id TEXT;
    deployment_id TEXT;
    attestation_id TEXT;
    lease_epoch TEXT;
    revision_value TEXT;
    route_text TEXT;
    process_text TEXT;
BEGIN
    scope_value := previous_value -> 'scope';
    process_value := previous_value -> 'process';
    route_value := pg_catalog.jsonb_build_object(
        'identity',
        process_value,
        'controller_fencing_token',
        1,
        'route_incarnation',
        1
    );
    route_bytes :=
        starring_runtime_private_v2.starring_runtime_suspended_route_bytes_v2(
            route_value
        );
    tenant_id := scope_value ->> 'tenant_id';
    installation_id := scope_value ->> 'installation_id';
    deployment_id := scope_value ->> 'deployment_id';
    attestation_id := previous_value ->> 'attestation_id';
    lease_epoch := previous_value ->> 'lease_epoch';
    revision_value := previous_value ->> 'revision';

    IF pg_catalog.jsonb_typeof(previous_value) <> 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(previous_value)
        ) <> 5
        OR pg_catalog.jsonb_typeof(scope_value) <> 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(scope_value)
        ) <> 3
        OR pg_catalog.jsonb_typeof(process_value) <> 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(process_value)
        ) <> 3
        OR route_bytes IS NULL
        OR tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR pg_catalog.jsonb_typeof(
            previous_value -> 'attestation_id'
        ) <> 'string'
        OR attestation_id !~ '^[0-9a-f]{64}$'
        OR pg_catalog.jsonb_typeof(
            previous_value -> 'lease_epoch'
        ) <> 'number'
        OR lease_epoch !~ '^[1-9][0-9]{0,18}$'
        OR lease_epoch::NUMERIC > 9223372036854775807
        OR pg_catalog.jsonb_typeof(previous_value -> 'revision') <> 'number'
        OR revision_value !~ '^[1-9][0-9]{0,18}$'
        OR revision_value::NUMERIC > 9223372036854775807
    THEN
        RETURN NULL;
    END IF;

    route_text := pg_catalog.convert_from(route_bytes, 'UTF8');
    process_text := pg_catalog.substring(
        route_text,
        pg_catalog.length('{"identity":') + 1,
        pg_catalog.length(route_text)
            - pg_catalog.length('{"identity":')
            - pg_catalog.length(
                ',"controller_fencing_token":1,"route_incarnation":1}'
            )
    );
    RETURN pg_catalog.convert_to(
        pg_catalog.concat(
            '{"scope":{"tenant_id":',
            pg_catalog.to_json(tenant_id)::TEXT,
            ',"installation_id":',
            pg_catalog.to_json(installation_id)::TEXT,
            ',"deployment_id":',
            pg_catalog.to_json(deployment_id)::TEXT,
            '},"attestation_id":',
            pg_catalog.to_json(attestation_id)::TEXT,
            ',"process":',
            process_text,
            ',"lease_epoch":',
            lease_epoch,
            ',"revision":',
            revision_value,
            '}'
        ),
        'UTF8'
    );
EXCEPTION
    WHEN OTHERS THEN
        RETURN NULL;
END;
$function$;

REVOKE ALL ON FUNCTION
    starring_runtime_private_v2.starring_runtime_suspended_route_bytes_v2(
        JSONB
    ),
    starring_runtime_private_v2.starring_runtime_suspended_previous_bytes_v2(
        JSONB
    )
FROM PUBLIC;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_suspended_root_frame_v2(
    root_row public.runtime_suspend_attempt_operations_v2,
    deployment_row public.runtime_deployments
)
RETURNS BYTEA
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
    SELECT
        pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(root_row.suspension_id, 'UTF8')
            )::BIGINT
        )
        || pg_catalog.convert_to(root_row.suspension_id, 'UTF8')
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(root_row.tenant_id, 'UTF8')
            )::BIGINT
        )
        || pg_catalog.convert_to(root_row.tenant_id, 'UTF8')
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(root_row.installation_id, 'UTF8')
            )::BIGINT
        )
        || pg_catalog.convert_to(root_row.installation_id, 'UTF8')
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(root_row.deployment_id, 'UTF8')
            )::BIGINT
        )
        || pg_catalog.convert_to(root_row.deployment_id, 'UTF8')
        || pg_catalog.int8send(root_row.deployment_revision)
        || pg_catalog.int8send(root_row.convergence_attempt_no)
        || pg_catalog.decode(root_row.suspend_attempt_digest, 'hex')
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                root_row.suspend_attempt_request_bytes
            )::BIGINT
        )
        || root_row.suspend_attempt_request_bytes
        || pg_catalog.int8send(deployment_row.convergence_attempt_no)
        || CASE
            WHEN deployment_row.last_controller_id IS NULL
            THEN pg_catalog.int2send(0::SMALLINT)
            ELSE
                pg_catalog.int2send(1::SMALLINT)
                || pg_catalog.int8send(
                    pg_catalog.octet_length(
                        pg_catalog.convert_to(
                            deployment_row.last_controller_id,
                            'UTF8'
                        )
                    )::BIGINT
                )
                || pg_catalog.convert_to(
                    deployment_row.last_controller_id,
                    'UTF8'
                )
        END
        || CASE
            WHEN deployment_row.last_fencing_token IS NULL
            THEN pg_catalog.int2send(0::SMALLINT)
            ELSE
                pg_catalog.int2send(1::SMALLINT)
                || pg_catalog.int8send(
                    deployment_row.last_fencing_token
                )
        END
        || pg_catalog.int2send(
            deployment_row.snapshot_format_version::SMALLINT
        )
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(
                    deployment_row.snapshot::TEXT,
                    'UTF8'
                )
            )::BIGINT
        )
        || pg_catalog.convert_to(
            deployment_row.snapshot::TEXT,
            'UTF8'
        )
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_suspended_sidecar_frame_v2(
    sidecar_row public.runtime_suspended_attempts_v2
)
RETURNS BYTEA
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
    SELECT
        pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(sidecar_row.suspension_id, 'UTF8')
            )::BIGINT
        )
        || pg_catalog.convert_to(sidecar_row.suspension_id, 'UTF8')
        || pg_catalog.decode(sidecar_row.suspend_attempt_digest, 'hex')
        || pg_catalog.int8send(sidecar_row.sidecar_revision)
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(sidecar_row.slot_guild_id, 'UTF8')
            )::BIGINT
        )
        || pg_catalog.convert_to(sidecar_row.slot_guild_id, 'UTF8')
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(sidecar_row.slot_ruleset_key, 'UTF8')
            )::BIGINT
        )
        || pg_catalog.convert_to(sidecar_row.slot_ruleset_key, 'UTF8')
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(sidecar_row.local_effect_kind, 'UTF8')
            )::BIGINT
        )
        || pg_catalog.convert_to(sidecar_row.local_effect_kind, 'UTF8')
        || pg_catalog.int8send(
            pg_catalog.octet_length(sidecar_row.local_effect_bytes)::BIGINT
        )
        || sidecar_row.local_effect_bytes
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(
                    sidecar_row.drain_obligation_kind,
                    'UTF8'
                )
            )::BIGINT
        )
        || pg_catalog.convert_to(
            sidecar_row.drain_obligation_kind,
            'UTF8'
        )
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                sidecar_row.drain_obligation_bytes
            )::BIGINT
        )
        || sidecar_row.drain_obligation_bytes
        || pg_catalog.timestamptz_send(sidecar_row.suspended_at)
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_suspended_projection_exact_v2(
    projection_bytes BYTEA,
    expected_root_frame BYTEA,
    expected_source_frame BYTEA,
    expected_successor_frame BYTEA,
    expected_provenance_frame BYTEA,
    expected_evidence_frame BYTEA
)
RETURNS BOOLEAN
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    domain_bytes BYTEA;
    expected_prefix BYTEA;
    expected_scalar BYTEA;
    expected_projection BYTEA;
BEGIN
    domain_bytes := pg_catalog.convert_to(
        'starring.runtime.startup_recovery.suspended_local_effect.terminal.v2',
        'UTF8'
    );
    expected_prefix :=
        pg_catalog.int8send(
            pg_catalog.octet_length(domain_bytes)::BIGINT
        )
        || domain_bytes
        || pg_catalog.int2send(2::SMALLINT)
        || pg_catalog.int2send(1::SMALLINT);
    expected_scalar := pg_catalog.sha256(
        expected_root_frame
        || expected_source_frame
        || expected_successor_frame
        || expected_provenance_frame
        || expected_evidence_frame
    );
    expected_projection :=
        expected_prefix
        || pg_catalog.int8send(
            pg_catalog.octet_length(expected_root_frame)::BIGINT
        )
        || expected_root_frame
        || pg_catalog.int8send(
            pg_catalog.octet_length(expected_source_frame)::BIGINT
        )
        || expected_source_frame
        || pg_catalog.int8send(
            pg_catalog.octet_length(expected_successor_frame)::BIGINT
        )
        || expected_successor_frame
        || pg_catalog.int8send(
            pg_catalog.octet_length(expected_provenance_frame)::BIGINT
        )
        || expected_provenance_frame
        || pg_catalog.int8send(
            pg_catalog.octet_length(expected_evidence_frame)::BIGINT
        )
        || expected_evidence_frame
        || expected_scalar;
    RETURN projection_bytes IS NOT DISTINCT FROM expected_projection;
EXCEPTION
    WHEN OTHERS THEN
        RETURN FALSE;
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_suspended_replay_exact_v2(
    projection_bytes BYTEA,
    expected_evidence_frame BYTEA
)
RETURNS BOOLEAN
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    domain_bytes BYTEA;
    prefix_bytes BYTEA;
    cursor_position BIGINT;
    projection_length BIGINT;
    frame_length NUMERIC;
    frame_bytes BYTEA;
    root_frame BYTEA;
    source_frame BYTEA;
    successor_frame BYTEA;
    provenance_frame BYTEA;
    evidence_frame BYTEA;
    frame_index INTEGER;
    byte_index INTEGER;
    outcome_tag INTEGER;
    expected_scalar BYTEA;
BEGIN
    projection_length := pg_catalog.octet_length(projection_bytes);
    IF projection_length NOT BETWEEN 1 AND 1048576 THEN
        RETURN FALSE;
    END IF;
    domain_bytes := pg_catalog.convert_to(
        'starring.runtime.startup_recovery.suspended_local_effect.terminal.v2',
        'UTF8'
    );
    prefix_bytes :=
        pg_catalog.int8send(
            pg_catalog.octet_length(domain_bytes)::BIGINT
        )
        || domain_bytes
        || pg_catalog.int2send(2::SMALLINT);
    IF pg_catalog.substring(
            projection_bytes,
            1,
            pg_catalog.octet_length(prefix_bytes)
        ) IS DISTINCT FROM prefix_bytes
        OR projection_length <
            pg_catalog.octet_length(prefix_bytes) + 2 + 8 + 32
    THEN
        RETURN FALSE;
    END IF;
    cursor_position := pg_catalog.octet_length(prefix_bytes) + 1;
    outcome_tag :=
        pg_catalog.get_byte(projection_bytes, cursor_position::INTEGER - 1)
            * 256
        + pg_catalog.get_byte(projection_bytes, cursor_position::INTEGER);
    cursor_position := cursor_position + 2;

    IF outcome_tag = 0 THEN
        frame_length := 0;
        FOR byte_index IN 0..7 LOOP
            frame_length := frame_length * 256
                + pg_catalog.get_byte(
                    projection_bytes,
                    cursor_position::INTEGER - 1 + byte_index
                );
        END LOOP;
        cursor_position := cursor_position + 8;
        IF frame_length < 1
            OR frame_length > 1048576
            OR cursor_position + frame_length + 31
                IS DISTINCT FROM projection_length
        THEN
            RETURN FALSE;
        END IF;
        evidence_frame := pg_catalog.substring(
            projection_bytes,
            cursor_position::INTEGER,
            frame_length::INTEGER
        );
        cursor_position := cursor_position + frame_length::BIGINT;
        RETURN evidence_frame IS NOT DISTINCT FROM expected_evidence_frame
            AND pg_catalog.substring(
                projection_bytes,
                cursor_position::INTEGER,
                32
            ) IS NOT DISTINCT FROM pg_catalog.sha256(evidence_frame);
    END IF;
    IF outcome_tag <> 1 THEN
        RETURN FALSE;
    END IF;

    FOR frame_index IN 1..5 LOOP
        IF cursor_position + 7 > projection_length THEN
            RETURN FALSE;
        END IF;
        frame_length := 0;
        FOR byte_index IN 0..7 LOOP
            frame_length := frame_length * 256
                + pg_catalog.get_byte(
                    projection_bytes,
                    cursor_position::INTEGER - 1 + byte_index
                );
            IF frame_length > 1048576 THEN
                RETURN FALSE;
            END IF;
        END LOOP;
        cursor_position := cursor_position + 8;
        IF frame_length < 1
            OR cursor_position + frame_length - 1 > projection_length
        THEN
            RETURN FALSE;
        END IF;
        frame_bytes := pg_catalog.substring(
            projection_bytes,
            cursor_position::INTEGER,
            frame_length::INTEGER
        );
        cursor_position := cursor_position + frame_length::BIGINT;
        CASE frame_index
            WHEN 1 THEN root_frame := frame_bytes;
            WHEN 2 THEN source_frame := frame_bytes;
            WHEN 3 THEN successor_frame := frame_bytes;
            WHEN 4 THEN provenance_frame := frame_bytes;
            WHEN 5 THEN evidence_frame := frame_bytes;
        END CASE;
    END LOOP;
    IF cursor_position + 31 IS DISTINCT FROM projection_length
        OR evidence_frame IS DISTINCT FROM expected_evidence_frame
    THEN
        RETURN FALSE;
    END IF;
    expected_scalar := pg_catalog.sha256(
        root_frame
        || source_frame
        || successor_frame
        || provenance_frame
        || evidence_frame
    );
    RETURN pg_catalog.substring(
        projection_bytes,
        cursor_position::INTEGER,
        32
    ) IS NOT DISTINCT FROM expected_scalar;
EXCEPTION
    WHEN OTHERS THEN
        RETURN FALSE;
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_suspended_terminal_sidecar_v2(
    projection_bytes BYTEA,
    expected_successor_frame BYTEA,
    root_row public.runtime_suspend_attempt_operations_v2,
    sidecar_row public.runtime_suspended_attempts_v2
)
RETURNS BOOLEAN
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    domain_bytes BYTEA;
    prefix_bytes BYTEA;
    expected_root_prefix BYTEA;
    cursor_position BIGINT;
    projection_length BIGINT;
    frame_length NUMERIC;
    frame_bytes BYTEA;
    root_frame BYTEA;
    source_frame BYTEA;
    successor_frame BYTEA;
    provenance_frame BYTEA;
    evidence_frame BYTEA;
    frame_index INTEGER;
    byte_index INTEGER;
BEGIN
    IF root_row.suspension_id
            IS DISTINCT FROM sidecar_row.suspension_id
        OR root_row.suspend_attempt_digest
            IS DISTINCT FROM sidecar_row.suspend_attempt_digest
        OR root_row.tenant_id IS DISTINCT FROM sidecar_row.tenant_id
        OR root_row.installation_id
            IS DISTINCT FROM sidecar_row.installation_id
        OR root_row.deployment_id
            IS DISTINCT FROM sidecar_row.deployment_id
        OR root_row.deployment_revision
            IS DISTINCT FROM sidecar_row.deployment_revision
        OR root_row.convergence_attempt_no
            IS DISTINCT FROM sidecar_row.convergence_attempt_no
        OR pg_catalog.encode(
            pg_catalog.sha256(
                pg_catalog.int8send(36::BIGINT)
                || pg_catalog.convert_to(
                    'starring.runtime.suspend_attempt.v2',
                    'UTF8'
                )
                || pg_catalog.decode('00', 'hex')
                || pg_catalog.int8send(
                    pg_catalog.octet_length(
                        root_row.suspend_attempt_request_bytes
                    )::BIGINT
                )
                || root_row.suspend_attempt_request_bytes
            ),
            'hex'
        ) IS DISTINCT FROM root_row.suspend_attempt_digest
    THEN
        RETURN FALSE;
    END IF;
    expected_root_prefix :=
        pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(root_row.suspension_id, 'UTF8')
            )::BIGINT
        )
        || pg_catalog.convert_to(root_row.suspension_id, 'UTF8')
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(root_row.tenant_id, 'UTF8')
            )::BIGINT
        )
        || pg_catalog.convert_to(root_row.tenant_id, 'UTF8')
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(root_row.installation_id, 'UTF8')
            )::BIGINT
        )
        || pg_catalog.convert_to(root_row.installation_id, 'UTF8')
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(root_row.deployment_id, 'UTF8')
            )::BIGINT
        )
        || pg_catalog.convert_to(root_row.deployment_id, 'UTF8')
        || pg_catalog.int8send(root_row.deployment_revision)
        || pg_catalog.int8send(root_row.convergence_attempt_no)
        || pg_catalog.decode(root_row.suspend_attempt_digest, 'hex')
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                root_row.suspend_attempt_request_bytes
            )::BIGINT
        )
        || root_row.suspend_attempt_request_bytes;
    projection_length := pg_catalog.octet_length(projection_bytes);
    domain_bytes := pg_catalog.convert_to(
        'starring.runtime.startup_recovery.suspended_local_effect.terminal.v2',
        'UTF8'
    );
    prefix_bytes :=
        pg_catalog.int8send(
            pg_catalog.octet_length(domain_bytes)::BIGINT
        )
        || domain_bytes
        || pg_catalog.int2send(2::SMALLINT)
        || pg_catalog.int2send(1::SMALLINT);
    IF projection_length NOT BETWEEN 1 AND 1048576
        OR pg_catalog.substring(
            projection_bytes,
            1,
            pg_catalog.octet_length(prefix_bytes)
        ) IS DISTINCT FROM prefix_bytes
    THEN
        RETURN FALSE;
    END IF;
    cursor_position := pg_catalog.octet_length(prefix_bytes) + 1;
    FOR frame_index IN 1..5 LOOP
        IF cursor_position + 7 > projection_length THEN
            RETURN FALSE;
        END IF;
        frame_length := 0;
        FOR byte_index IN 0..7 LOOP
            frame_length := frame_length * 256
                + pg_catalog.get_byte(
                    projection_bytes,
                    cursor_position::INTEGER - 1 + byte_index
                );
            IF frame_length > 1048576 THEN
                RETURN FALSE;
            END IF;
        END LOOP;
        cursor_position := cursor_position + 8;
        IF frame_length < 1
            OR cursor_position + frame_length - 1 > projection_length
        THEN
            RETURN FALSE;
        END IF;
        frame_bytes := pg_catalog.substring(
            projection_bytes,
            cursor_position::INTEGER,
            frame_length::INTEGER
        );
        cursor_position := cursor_position + frame_length::BIGINT;
        CASE frame_index
            WHEN 1 THEN root_frame := frame_bytes;
            WHEN 2 THEN source_frame := frame_bytes;
            WHEN 3 THEN successor_frame := frame_bytes;
            WHEN 4 THEN provenance_frame := frame_bytes;
            WHEN 5 THEN evidence_frame := frame_bytes;
        END CASE;
    END LOOP;
    RETURN cursor_position + 31 = projection_length
        AND pg_catalog.substring(
            root_frame,
            1,
            pg_catalog.octet_length(expected_root_prefix)
        ) IS NOT DISTINCT FROM expected_root_prefix
        AND successor_frame IS NOT DISTINCT FROM
            expected_successor_frame
        AND pg_catalog.substring(
            projection_bytes,
            cursor_position::INTEGER,
            32
        ) IS NOT DISTINCT FROM pg_catalog.sha256(
            root_frame
            || source_frame
            || successor_frame
            || provenance_frame
            || evidence_frame
        )
        AND pg_catalog.substring(
            provenance_frame,
            1,
            pg_catalog.octet_length(
                pg_catalog.convert_to(
                    '{"kind":"closed_recovery","witness":',
                    'UTF8'
                )
            )
        ) IS NOT DISTINCT FROM pg_catalog.convert_to(
            '{"kind":"closed_recovery","witness":',
            'UTF8'
        );
EXCEPTION
    WHEN OTHERS THEN
        RETURN FALSE;
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_suspended_quiescent_exact_v2(
    root_row public.runtime_suspend_attempt_operations_v2,
    sidecar_row public.runtime_suspended_attempts_v2
)
RETURNS BOOLEAN
LANGUAGE plpgsql
STABLE
STRICT
PARALLEL SAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    root_value JSONB;
    drain_value JSONB;
    previous_bytes BYTEA;
    expected_drain_bytes BYTEA;
    expected_digest TEXT;
BEGIN
    expected_digest := pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.int8send(36::BIGINT)
            || pg_catalog.convert_to(
                'starring.runtime.suspend_attempt.v2',
                'UTF8'
            )
            || pg_catalog.decode('00', 'hex')
            || pg_catalog.int8send(
                pg_catalog.octet_length(
                    root_row.suspend_attempt_request_bytes
                )::BIGINT
            )
            || root_row.suspend_attempt_request_bytes
        ),
        'hex'
    );
    IF expected_digest IS DISTINCT FROM root_row.suspend_attempt_digest
        OR root_row.suspension_id
            IS DISTINCT FROM sidecar_row.suspension_id
        OR root_row.suspend_attempt_digest
            IS DISTINCT FROM sidecar_row.suspend_attempt_digest
        OR root_row.tenant_id IS DISTINCT FROM sidecar_row.tenant_id
        OR root_row.installation_id
            IS DISTINCT FROM sidecar_row.installation_id
        OR root_row.deployment_id
            IS DISTINCT FROM sidecar_row.deployment_id
        OR root_row.deployment_revision
            IS DISTINCT FROM sidecar_row.deployment_revision
        OR root_row.convergence_attempt_no
            IS DISTINCT FROM sidecar_row.convergence_attempt_no
        OR sidecar_row.local_effect_kind <> 'none'
        OR sidecar_row.local_effect_bytes
            IS DISTINCT FROM pg_catalog.convert_to(
                '{"kind":"none"}',
                'UTF8'
            )
    THEN
        RETURN FALSE;
    END IF;
    root_value := pg_catalog.convert_from(
        root_row.suspend_attempt_request_bytes,
        'UTF8'
    )::JSONB;
    drain_value := root_value -> 'drain_obligation';
    IF root_value -> 'local_effect'
            IS DISTINCT FROM '{"kind":"none"}'::JSONB
        OR sidecar_row.drain_obligation_kind NOT IN (
            'none',
            'previous_serving'
        )
    THEN
        RETURN FALSE;
    END IF;
    IF sidecar_row.drain_obligation_kind = 'none' THEN
        expected_drain_bytes :=
            pg_catalog.convert_to('{"kind":"none"}', 'UTF8');
    ELSE
        previous_bytes :=
            starring_runtime_private_v2.starring_runtime_suspended_previous_bytes_v2(
                drain_value -> 'previous'
            );
        IF drain_value ->> 'kind' <> 'previous_serving'
            OR previous_bytes IS NULL
        THEN
            RETURN FALSE;
        END IF;
        expected_drain_bytes := pg_catalog.convert_to(
            pg_catalog.concat(
                '{"kind":"previous_serving","previous":',
                pg_catalog.convert_from(previous_bytes, 'UTF8'),
                '}'
            ),
            'UTF8'
        );
    END IF;
    RETURN sidecar_row.drain_obligation_bytes
            IS NOT DISTINCT FROM expected_drain_bytes
        AND drain_value IS NOT DISTINCT FROM
            pg_catalog.convert_from(expected_drain_bytes, 'UTF8')::JSONB
        AND starring_runtime_private_v2.starring_runtime_suspended_root_exact_v2(
            root_row,
            sidecar_row
        );
EXCEPTION
    WHEN OTHERS THEN
        RETURN FALSE;
END;
$function$;

REVOKE ALL ON FUNCTION
    starring_runtime_private_v2.starring_runtime_suspended_root_frame_v2(
        public.runtime_suspend_attempt_operations_v2,
        public.runtime_deployments
    ),
    starring_runtime_private_v2.starring_runtime_suspended_sidecar_frame_v2(
        public.runtime_suspended_attempts_v2
    ),
    starring_runtime_private_v2.starring_runtime_suspended_projection_exact_v2(
        BYTEA,
        BYTEA,
        BYTEA,
        BYTEA,
        BYTEA,
        BYTEA
    ),
    starring_runtime_private_v2.starring_runtime_suspended_replay_exact_v2(
        BYTEA,
        BYTEA
    ),
    starring_runtime_private_v2.starring_runtime_suspended_terminal_sidecar_v2(
        BYTEA,
        BYTEA,
        public.runtime_suspend_attempt_operations_v2,
        public.runtime_suspended_attempts_v2
    ),
    starring_runtime_private_v2.starring_runtime_suspended_quiescent_exact_v2(
        public.runtime_suspend_attempt_operations_v2,
        public.runtime_suspended_attempts_v2
    )
FROM PUBLIC;

CREATE OR REPLACE FUNCTION public.reject_runtime_suspend_attempt_ledger_mutation_v2()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
PARALLEL UNSAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    source_frame BYTEA;
    successor_frame BYTEA;
    gate_valid BOOLEAN;
    setting_name TEXT;
BEGIN
    gate_valid := FALSE;
    IF TG_TABLE_SCHEMA = 'public'
        AND TG_TABLE_NAME = 'runtime_suspended_attempts_v2'
        AND TG_OP = 'UPDATE'
    THEN
        source_frame :=
            starring_runtime_private_v2.starring_runtime_suspended_sidecar_frame_v2(
                OLD
            );
        successor_frame :=
            starring_runtime_private_v2.starring_runtime_suspended_sidecar_frame_v2(
                NEW
            );
        gate_valid :=
            COALESCE(pg_catalog.current_setting(
                'starring.runtime_suspended_local_gate_v2',
                TRUE
            ), '') = 'armed'
            AND COALESCE(pg_catalog.current_setting(
                'starring.runtime_suspended_local_id_v2',
                TRUE
            ), '') IS NOT DISTINCT FROM OLD.suspension_id
            AND COALESCE(pg_catalog.current_setting(
                'starring.runtime_suspended_local_source_v2',
                TRUE
            ), '') IS NOT DISTINCT FROM pg_catalog.encode(
                pg_catalog.sha256(source_frame),
                'hex'
            )
            AND COALESCE(pg_catalog.current_setting(
                'starring.runtime_suspended_local_successor_v2',
                TRUE
            ), '') IS NOT DISTINCT FROM pg_catalog.encode(
                pg_catalog.sha256(successor_frame),
                'hex'
            )
            AND NEW.suspension_id IS NOT DISTINCT FROM OLD.suspension_id
            AND NEW.suspend_attempt_digest
                IS NOT DISTINCT FROM OLD.suspend_attempt_digest
            AND NEW.tenant_id IS NOT DISTINCT FROM OLD.tenant_id
            AND NEW.installation_id
                IS NOT DISTINCT FROM OLD.installation_id
            AND NEW.deployment_id IS NOT DISTINCT FROM OLD.deployment_id
            AND NEW.deployment_revision
                IS NOT DISTINCT FROM OLD.deployment_revision
            AND NEW.convergence_attempt_no
                IS NOT DISTINCT FROM OLD.convergence_attempt_no
            AND OLD.sidecar_revision < 9223372036854775807
            AND NEW.sidecar_revision = OLD.sidecar_revision + 1
            AND NEW.slot_guild_id IS NOT DISTINCT FROM OLD.slot_guild_id
            AND NEW.slot_ruleset_key
                IS NOT DISTINCT FROM OLD.slot_ruleset_key
            AND OLD.local_effect_kind = 'exact_route'
            AND NEW.local_effect_kind = 'route_absent'
            AND (
                (
                    OLD.drain_obligation_kind = 'exact_local_route'
                    AND NEW.drain_obligation_kind = 'none'
                )
                OR (
                    OLD.drain_obligation_kind = 'local_and_previous'
                    AND NEW.drain_obligation_kind = 'previous_serving'
                )
            )
            AND NEW.suspended_at IS NOT DISTINCT FROM OLD.suspended_at;
    END IF;

    FOREACH setting_name IN ARRAY ARRAY[
        'starring.runtime_suspended_local_gate_v2',
        'starring.runtime_suspended_local_id_v2',
        'starring.runtime_suspended_local_source_v2',
        'starring.runtime_suspended_local_successor_v2'
    ]
    LOOP
        PERFORM pg_catalog.set_config(setting_name, '', TRUE);
    END LOOP;

    IF NOT gate_valid THEN
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE = 'runtime_suspend_attempt_ledger_mutation_rejected';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_suspended_root_exact_v2(
    root_row public.runtime_suspend_attempt_operations_v2,
    sidecar_row public.runtime_suspended_attempts_v2
)
RETURNS BOOLEAN
LANGUAGE plpgsql
STABLE
STRICT
PARALLEL SAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    root_value JSONB;
    guard_value JSONB;
    scope_value JSONB;
    failure_value JSONB;
    disposition_value JSONB;
    local_value JSONB;
    drain_value JSONB;
    route_value JSONB;
    obligation_route_value JSONB;
    previous_value JSONB;
    route_bytes BYTEA;
    obligation_route_bytes BYTEA;
    previous_bytes BYTEA;
    local_bytes BYTEA;
    drain_bytes BYTEA;
    canonical_root BYTEA;
    action_id TEXT;
    expected_revision TEXT;
    controller_id TEXT;
    fencing_token TEXT;
    runtime_generation TEXT;
    convergence_attempt TEXT;
    source_phase TEXT;
    expected_checkpoint TEXT;
    failure_id TEXT;
    failure_kind TEXT;
    failure_code TEXT;
    failure_message TEXT;
    failure_recorded_at TEXT;
    disposition_kind TEXT;
    retry_not_before TEXT;
    checkpoint_value TEXT;
    lifecycle_value TEXT;
    route_guild_id TEXT;
    route_ruleset_key TEXT;
    route_runtime_generation TEXT;
    route_fencing_token TEXT;
    previous_tenant_id TEXT;
    previous_installation_id TEXT;
    previous_deployment_id TEXT;
    previous_guild_id TEXT;
    previous_ruleset_key TEXT;
    previous_runtime_generation TEXT;
    disposition_bytes BYTEA;
BEGIN
    IF pg_catalog.encode(
            pg_catalog.sha256(
                pg_catalog.int8send(36::BIGINT)
                || pg_catalog.convert_to(
                    'starring.runtime.suspend_attempt.v2',
                    'UTF8'
                )
                || pg_catalog.decode('00', 'hex')
                || pg_catalog.int8send(
                    pg_catalog.octet_length(
                        root_row.suspend_attempt_request_bytes
                    )::BIGINT
                )
                || root_row.suspend_attempt_request_bytes
            ),
            'hex'
        ) IS DISTINCT FROM root_row.suspend_attempt_digest
        OR root_row.suspension_id
            IS DISTINCT FROM sidecar_row.suspension_id
        OR root_row.suspend_attempt_digest
            IS DISTINCT FROM sidecar_row.suspend_attempt_digest
        OR root_row.tenant_id IS DISTINCT FROM sidecar_row.tenant_id
        OR root_row.installation_id
            IS DISTINCT FROM sidecar_row.installation_id
        OR root_row.deployment_id
            IS DISTINCT FROM sidecar_row.deployment_id
        OR root_row.deployment_revision
            IS DISTINCT FROM sidecar_row.deployment_revision
        OR root_row.convergence_attempt_no
            IS DISTINCT FROM sidecar_row.convergence_attempt_no
        OR sidecar_row.local_effect_kind NOT IN ('exact_route', 'none')
        OR (
            sidecar_row.local_effect_kind = 'exact_route'
            AND sidecar_row.drain_obligation_kind NOT IN (
                'exact_local_route',
                'local_and_previous'
            )
        )
        OR (
            sidecar_row.local_effect_kind = 'none'
            AND sidecar_row.drain_obligation_kind NOT IN (
                'none',
                'previous_serving'
            )
        )
        OR sidecar_row.sidecar_revision >= 9223372036854775807
        OR EXTRACT(EPOCH FROM sidecar_row.suspended_at) * 1000000
            NOT BETWEEN
                -62135596800000000 AND 253402300799999999
        OR EXTRACT(EPOCH FROM sidecar_row.suspended_at) * 1000000
            <> pg_catalog.trunc(
                EXTRACT(EPOCH FROM sidecar_row.suspended_at)
                    * 1000000
            )
    THEN
        RETURN FALSE;
    END IF;

    root_value := pg_catalog.convert_from(
        root_row.suspend_attempt_request_bytes,
        'UTF8'
    )::JSONB;
    guard_value := root_value -> 'guard';
    scope_value := guard_value -> 'scope';
    failure_value := root_value -> 'failure';
    disposition_value := root_value -> 'disposition';
    local_value := root_value -> 'local_effect';
    drain_value := root_value -> 'drain_obligation';
    route_value := local_value -> 'route';
    route_bytes :=
        starring_runtime_private_v2.starring_runtime_suspended_route_bytes_v2(
            route_value
        );

    action_id := root_value ->> 'action_id';
    expected_revision := guard_value ->> 'expected_revision';
    controller_id := guard_value ->> 'controller_id';
    fencing_token := guard_value ->> 'fencing_token';
    runtime_generation := guard_value ->> 'runtime_generation';
    convergence_attempt := guard_value ->> 'convergence_attempt';
    source_phase := root_value ->> 'source_phase';
    failure_id := failure_value ->> 'failure_id';
    failure_kind := failure_value ->> 'kind';
    failure_code := failure_value ->> 'code';
    failure_message := failure_value ->> 'message';
    failure_recorded_at :=
        failure_value ->> 'recorded_at_unix_microseconds';
    disposition_kind := disposition_value ->> 'kind';
    retry_not_before :=
        disposition_value ->> 'retry_not_before_unix_microseconds';
    checkpoint_value := root_value ->> 'checkpoint';
    lifecycle_value := local_value ->> 'lifecycle';
    route_guild_id := route_value #>> '{identity,target,guild_id}';
    route_ruleset_key :=
        route_value #>> '{identity,target,ruleset_key}';
    route_runtime_generation :=
        route_value #>> '{identity,runtime_generation}';
    route_fencing_token :=
        route_value ->> 'controller_fencing_token';

    expected_checkpoint := CASE source_phase
        WHEN 'requested' THEN 'verify_preflight'
        WHEN 'preflight_ready' THEN 'request_drain'
        WHEN 'drain_requested' THEN 'complete_drain'
        WHEN 'drained' THEN 'begin_activation'
        WHEN 'activation_applying' THEN 'observe_activation'
        WHEN 'runtime_pending_ready' THEN 'begin_panels'
        WHEN 'reconciling_panels' THEN 'reconcile_panels'
        ELSE NULL
    END;

    IF pg_catalog.jsonb_typeof(root_value) <> 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(root_value)
        ) <> 10
        OR root_value ->> 'format_version' IS DISTINCT FROM '2'
        OR pg_catalog.jsonb_typeof(
            root_value -> 'format_version'
        ) <> 'number'
        OR pg_catalog.jsonb_typeof(
            root_value -> 'suspension_id'
        ) <> 'string'
        OR root_value ->> 'suspension_id'
            IS DISTINCT FROM root_row.suspension_id
        OR pg_catalog.jsonb_typeof(root_value -> 'action_id') <> 'number'
        OR action_id !~ '^[1-9][0-9]{0,18}$'
        OR action_id::NUMERIC > 9223372036854775807
        OR pg_catalog.jsonb_typeof(guard_value) <> 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(guard_value)
        ) <> 6
        OR pg_catalog.jsonb_typeof(scope_value) <> 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(scope_value)
        ) <> 3
        OR scope_value ->> 'tenant_id'
            IS DISTINCT FROM root_row.tenant_id
        OR scope_value ->> 'installation_id'
            IS DISTINCT FROM root_row.installation_id
        OR scope_value ->> 'deployment_id'
            IS DISTINCT FROM root_row.deployment_id
        OR pg_catalog.jsonb_typeof(
            scope_value -> 'tenant_id'
        ) <> 'string'
        OR pg_catalog.jsonb_typeof(
            scope_value -> 'installation_id'
        ) <> 'string'
        OR pg_catalog.jsonb_typeof(
            scope_value -> 'deployment_id'
        ) <> 'string'
        OR pg_catalog.jsonb_typeof(
            guard_value -> 'expected_revision'
        ) <> 'number'
        OR expected_revision IS DISTINCT FROM
            root_row.deployment_revision::TEXT
        OR pg_catalog.jsonb_typeof(
            guard_value -> 'controller_id'
        ) <> 'string'
        OR controller_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR pg_catalog.jsonb_typeof(
            guard_value -> 'fencing_token'
        ) <> 'number'
        OR fencing_token !~ '^[1-9][0-9]{0,18}$'
        OR fencing_token::NUMERIC > 9223372036854775807
        OR pg_catalog.jsonb_typeof(
            guard_value -> 'runtime_generation'
        ) <> 'number'
        OR runtime_generation !~ '^[1-9][0-9]{0,18}$'
        OR runtime_generation::NUMERIC > 9223372036854775807
        OR pg_catalog.jsonb_typeof(
            guard_value -> 'convergence_attempt'
        ) <> 'number'
        OR convergence_attempt IS DISTINCT FROM
            root_row.convergence_attempt_no::TEXT
        OR pg_catalog.jsonb_typeof(
            root_value -> 'source_phase'
        ) <> 'string'
        OR expected_checkpoint IS NULL
        OR pg_catalog.jsonb_typeof(failure_value) <> 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(failure_value)
        ) <> 5
        OR pg_catalog.jsonb_typeof(
            failure_value -> 'failure_id'
        ) <> 'string'
        OR failure_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR pg_catalog.jsonb_typeof(failure_value -> 'kind') <> 'string'
        OR failure_kind NOT IN (
            'environment_unavailable',
            'activation_not_observable',
            'panel_reconciliation',
            'gateway_start',
            'gateway_ready_timeout',
            'invariant_violation'
        )
        OR pg_catalog.jsonb_typeof(failure_value -> 'code') <> 'string'
        OR failure_code !~ '^[a-z0-9_]{1,64}$'
        OR pg_catalog.jsonb_typeof(
            failure_value -> 'message'
        ) <> 'string'
        OR pg_catalog.btrim(failure_message) = ''
        OR pg_catalog.octet_length(
            pg_catalog.convert_to(failure_message, 'UTF8')
        ) > 1024
        OR pg_catalog.jsonb_typeof(
            failure_value -> 'recorded_at_unix_microseconds'
        ) <> 'number'
        OR failure_recorded_at !~ '^-?(0|[1-9][0-9]{0,18})$'
        OR failure_recorded_at::NUMERIC NOT BETWEEN
            -62135596800000000 AND 253402300799999999
        OR pg_catalog.jsonb_typeof(disposition_value) <> 'object'
        OR disposition_kind NOT IN ('retryable', 'blocked')
        OR (
            disposition_kind = 'blocked'
            AND (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(disposition_value)
            ) <> 1
        )
        OR (
            disposition_kind = 'retryable'
            AND (
                (
                    SELECT pg_catalog.count(*)
                    FROM pg_catalog.jsonb_object_keys(disposition_value)
                ) <> 2
                OR pg_catalog.jsonb_typeof(
                    disposition_value
                        -> 'retry_not_before_unix_microseconds'
                ) <> 'number'
                OR retry_not_before !~ '^-?(0|[1-9][0-9]{0,18})$'
                OR retry_not_before::NUMERIC NOT BETWEEN
                    -62135596800000000 AND 253402300799999999
                OR retry_not_before::NUMERIC
                    < failure_recorded_at::NUMERIC
            )
        )
        OR pg_catalog.jsonb_typeof(
            root_value -> 'checkpoint'
        ) <> 'string'
        OR checkpoint_value IS DISTINCT FROM expected_checkpoint
    THEN
        RETURN FALSE;
    END IF;

    IF sidecar_row.local_effect_kind = 'exact_route' THEN
        IF pg_catalog.jsonb_typeof(local_value) <> 'object'
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(local_value)
            ) <> 3
            OR local_value ->> 'kind' IS DISTINCT FROM 'exact_route'
            OR pg_catalog.jsonb_typeof(local_value -> 'kind') <> 'string'
            OR route_bytes IS NULL
            OR pg_catalog.jsonb_typeof(
                local_value -> 'lifecycle'
            ) <> 'string'
            OR lifecycle_value NOT IN ('staged', 'draining')
            OR route_guild_id IS DISTINCT FROM sidecar_row.slot_guild_id
            OR route_ruleset_key
                IS DISTINCT FROM sidecar_row.slot_ruleset_key
            OR route_runtime_generation IS DISTINCT FROM runtime_generation
            OR route_fencing_token IS DISTINCT FROM fencing_token
        THEN
            RETURN FALSE;
        END IF;
        local_bytes := pg_catalog.convert_to(
            pg_catalog.concat(
                '{"kind":"exact_route","route":',
                pg_catalog.convert_from(route_bytes, 'UTF8'),
                ',"lifecycle":',
                pg_catalog.to_json(lifecycle_value)::TEXT,
                '}'
            ),
            'UTF8'
        );
    ELSE
        IF pg_catalog.jsonb_typeof(local_value) <> 'object'
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(local_value)
            ) <> 1
            OR local_value ->> 'kind' IS DISTINCT FROM 'none'
            OR pg_catalog.jsonb_typeof(local_value -> 'kind') <> 'string'
        THEN
            RETURN FALSE;
        END IF;
        local_bytes := pg_catalog.convert_to('{"kind":"none"}', 'UTF8');
    END IF;
    IF local_bytes IS DISTINCT FROM sidecar_row.local_effect_bytes
        OR local_value IS DISTINCT FROM
            pg_catalog.convert_from(local_bytes, 'UTF8')::JSONB
    THEN
        RETURN FALSE;
    END IF;

    IF sidecar_row.drain_obligation_kind = 'exact_local_route' THEN
        IF pg_catalog.jsonb_typeof(drain_value) <> 'object'
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(drain_value)
            ) <> 2
            OR drain_value ->> 'kind'
                IS DISTINCT FROM 'exact_local_route'
        THEN
            RETURN FALSE;
        END IF;
        obligation_route_value := drain_value -> 'route';
        obligation_route_bytes :=
            starring_runtime_private_v2.starring_runtime_suspended_route_bytes_v2(
                obligation_route_value
            );
        IF obligation_route_bytes IS DISTINCT FROM route_bytes THEN
            RETURN FALSE;
        END IF;
        drain_bytes := pg_catalog.convert_to(
            pg_catalog.concat(
                '{"kind":"exact_local_route","route":',
                pg_catalog.convert_from(route_bytes, 'UTF8'),
                '}'
            ),
            'UTF8'
        );
    ELSIF sidecar_row.drain_obligation_kind = 'local_and_previous' THEN
        IF pg_catalog.jsonb_typeof(drain_value) <> 'object'
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(drain_value)
            ) <> 3
            OR drain_value ->> 'kind'
                IS DISTINCT FROM 'local_and_previous'
        THEN
            RETURN FALSE;
        END IF;
        obligation_route_value := drain_value -> 'local';
        obligation_route_bytes :=
            starring_runtime_private_v2.starring_runtime_suspended_route_bytes_v2(
                obligation_route_value
            );
        previous_value := drain_value -> 'previous';
        previous_bytes :=
            starring_runtime_private_v2.starring_runtime_suspended_previous_bytes_v2(
                previous_value
            );
        previous_tenant_id :=
            previous_value #>> '{scope,tenant_id}';
        previous_installation_id :=
            previous_value #>> '{scope,installation_id}';
        previous_deployment_id :=
            previous_value #>> '{scope,deployment_id}';
        previous_guild_id :=
            previous_value #>> '{process,target,guild_id}';
        previous_ruleset_key :=
            previous_value #>> '{process,target,ruleset_key}';
        previous_runtime_generation :=
            previous_value #>> '{process,runtime_generation}';
        IF obligation_route_bytes IS DISTINCT FROM route_bytes
            OR previous_bytes IS NULL
            OR previous_tenant_id IS DISTINCT FROM root_row.tenant_id
            OR previous_installation_id
                IS DISTINCT FROM root_row.installation_id
            OR previous_deployment_id = root_row.deployment_id
            OR previous_guild_id
                IS DISTINCT FROM sidecar_row.slot_guild_id
            OR previous_ruleset_key
                IS DISTINCT FROM sidecar_row.slot_ruleset_key
            OR previous_runtime_generation::NUMERIC
                >= runtime_generation::NUMERIC
        THEN
            RETURN FALSE;
        END IF;
        drain_bytes := pg_catalog.convert_to(
            pg_catalog.concat(
                '{"kind":"local_and_previous","local":',
                pg_catalog.convert_from(route_bytes, 'UTF8'),
                ',"previous":',
                pg_catalog.convert_from(previous_bytes, 'UTF8'),
                '}'
            ),
            'UTF8'
        );
    ELSIF sidecar_row.drain_obligation_kind = 'none' THEN
        IF pg_catalog.jsonb_typeof(drain_value) <> 'object'
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(drain_value)
            ) <> 1
            OR drain_value ->> 'kind' IS DISTINCT FROM 'none'
            OR pg_catalog.jsonb_typeof(drain_value -> 'kind') <> 'string'
        THEN
            RETURN FALSE;
        END IF;
        drain_bytes := pg_catalog.convert_to('{"kind":"none"}', 'UTF8');
    ELSE
        IF pg_catalog.jsonb_typeof(drain_value) <> 'object'
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(drain_value)
            ) <> 2
            OR drain_value ->> 'kind'
                IS DISTINCT FROM 'previous_serving'
            OR pg_catalog.jsonb_typeof(
                drain_value -> 'kind'
            ) <> 'string'
        THEN
            RETURN FALSE;
        END IF;
        previous_value := drain_value -> 'previous';
        previous_bytes :=
            starring_runtime_private_v2.starring_runtime_suspended_previous_bytes_v2(
                previous_value
            );
        previous_tenant_id :=
            previous_value #>> '{scope,tenant_id}';
        previous_installation_id :=
            previous_value #>> '{scope,installation_id}';
        previous_deployment_id :=
            previous_value #>> '{scope,deployment_id}';
        previous_guild_id :=
            previous_value #>> '{process,target,guild_id}';
        previous_ruleset_key :=
            previous_value #>> '{process,target,ruleset_key}';
        previous_runtime_generation :=
            previous_value #>> '{process,runtime_generation}';
        IF previous_bytes IS NULL
            OR previous_tenant_id IS DISTINCT FROM root_row.tenant_id
            OR previous_installation_id
                IS DISTINCT FROM root_row.installation_id
            OR previous_deployment_id = root_row.deployment_id
            OR previous_guild_id
                IS DISTINCT FROM sidecar_row.slot_guild_id
            OR previous_ruleset_key
                IS DISTINCT FROM sidecar_row.slot_ruleset_key
            OR previous_runtime_generation::NUMERIC
                >= runtime_generation::NUMERIC
        THEN
            RETURN FALSE;
        END IF;
        drain_bytes := pg_catalog.convert_to(
            pg_catalog.concat(
                '{"kind":"previous_serving","previous":',
                pg_catalog.convert_from(previous_bytes, 'UTF8'),
                '}'
            ),
            'UTF8'
        );
    END IF;

    IF drain_bytes IS DISTINCT FROM sidecar_row.drain_obligation_bytes
        OR drain_value IS DISTINCT FROM
            pg_catalog.convert_from(drain_bytes, 'UTF8')::JSONB
    THEN
        RETURN FALSE;
    END IF;

    disposition_bytes := CASE disposition_kind
        WHEN 'blocked' THEN
            pg_catalog.convert_to('{"kind":"blocked"}', 'UTF8')
        ELSE
            pg_catalog.convert_to(
                pg_catalog.concat(
                    '{"kind":"retryable","retry_not_before_unix_microseconds":',
                    retry_not_before,
                    '}'
                ),
                'UTF8'
            )
    END;
    canonical_root := pg_catalog.convert_to(
        pg_catalog.concat(
            '{"format_version":2,"suspension_id":',
            pg_catalog.to_json(root_row.suspension_id)::TEXT,
            ',"action_id":',
            action_id,
            ',"guard":{"scope":{"tenant_id":',
            pg_catalog.to_json(root_row.tenant_id)::TEXT,
            ',"installation_id":',
            pg_catalog.to_json(root_row.installation_id)::TEXT,
            ',"deployment_id":',
            pg_catalog.to_json(root_row.deployment_id)::TEXT,
            '},"expected_revision":',
            expected_revision,
            ',"controller_id":',
            pg_catalog.to_json(controller_id)::TEXT,
            ',"fencing_token":',
            fencing_token,
            ',"runtime_generation":',
            runtime_generation,
            ',"convergence_attempt":',
            convergence_attempt,
            '},"source_phase":',
            pg_catalog.to_json(source_phase)::TEXT,
            ',"failure":{"failure_id":',
            pg_catalog.to_json(failure_id)::TEXT,
            ',"kind":',
            pg_catalog.to_json(failure_kind)::TEXT,
            ',"code":',
            pg_catalog.to_json(failure_code)::TEXT,
            ',"message":',
            pg_catalog.to_json(failure_message)::TEXT,
            ',"recorded_at_unix_microseconds":',
            failure_recorded_at,
            '},"disposition":',
            pg_catalog.convert_from(disposition_bytes, 'UTF8'),
            ',"checkpoint":',
            pg_catalog.to_json(checkpoint_value)::TEXT,
            ',"local_effect":',
            pg_catalog.convert_from(local_bytes, 'UTF8'),
            ',"drain_obligation":',
            pg_catalog.convert_from(drain_bytes, 'UTF8'),
            '}'
        ),
        'UTF8'
    );
    RETURN canonical_root IS NOT DISTINCT FROM
        root_row.suspend_attempt_request_bytes;
EXCEPTION
    WHEN OTHERS THEN
        RETURN FALSE;
END;
$function$;

REVOKE ALL ON FUNCTION
    starring_runtime_private_v2.starring_runtime_suspended_root_exact_v2(
        public.runtime_suspend_attempt_operations_v2,
        public.runtime_suspended_attempts_v2
    )
FROM PUBLIC;

REVOKE ALL ON FUNCTION
    public.starring_runtime_startup_recovery_execute_suspended_local_v2(
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
        TIMESTAMPTZ,
        TEXT,
        BIGINT,
        BIGINT,
        TEXT,
        BIGINT,
        BIGINT,
        BIGINT,
        BIGINT,
        TEXT,
        BIGINT,
        BIGINT,
        BIGINT
    )
FROM PUBLIC;

DO $grant_executor$
DECLARE
    common_owner OID;
    executor_role OID;
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

    IF executor_role IS NOT NULL THEN
        EXECUTE pg_catalog.format(
            'GRANT EXECUTE ON FUNCTION public.starring_runtime_startup_recovery_execute_suspended_local_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint) TO %s',
            executor_role::REGROLE
        );
    END IF;
END;
$grant_executor$;

DO $patch_schema_manifest$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
    identity TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_schema_manifest_v1()'
    );

    previous_fragment :=
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_startup_reserved_projection_exact_v2(bytea,text,bigint,bigint,bigint,bigint,timestamp with time zone,public.runtime_certification_operation_terminals_v2)''' || E'\n' ||
        '        )';
    next_fragment := previous_fragment;
    FOREACH identity IN ARRAY ARRAY[
        'public.starring_runtime_startup_recovery_execute_suspended_local_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint)',
        'starring_runtime_private_v2.starring_runtime_suspended_route_bytes_v2(jsonb)',
        'starring_runtime_private_v2.starring_runtime_suspended_previous_bytes_v2(jsonb)',
        'starring_runtime_private_v2.starring_runtime_suspended_root_exact_v2(public.runtime_suspend_attempt_operations_v2,public.runtime_suspended_attempts_v2)',
        'starring_runtime_private_v2.starring_runtime_suspended_root_frame_v2(public.runtime_suspend_attempt_operations_v2,public.runtime_deployments)',
        'starring_runtime_private_v2.starring_runtime_suspended_sidecar_frame_v2(public.runtime_suspended_attempts_v2)',
        'starring_runtime_private_v2.starring_runtime_suspended_projection_exact_v2(bytea,bytea,bytea,bytea,bytea,bytea)',
        'starring_runtime_private_v2.starring_runtime_suspended_replay_exact_v2(bytea,bytea)',
        'starring_runtime_private_v2.starring_runtime_suspended_terminal_sidecar_v2(bytea,bytea,public.runtime_suspend_attempt_operations_v2,public.runtime_suspended_attempts_v2)',
        'starring_runtime_private_v2.starring_runtime_suspended_quiescent_exact_v2(public.runtime_suspend_attempt_operations_v2,public.runtime_suspended_attempts_v2)'
    ]
    LOOP
        next_fragment := next_fragment || E'\n' ||
            '        UNION' || E'\n' ||
            '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
            '            ' || pg_catalog.quote_literal(identity) || E'\n' ||
            '        )';
    END LOOP;
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_suspended_manifest_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        'RETURN observed_count = 799' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''52022b152b1189e01928d8cc14dc229d1ed094a5da7837711a06cf3077b0ea41'';';
    next_fragment :=
        'RETURN observed_count = 809' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''fbe4a19a7ade16da18b9ce6670e7d1bf7085737d60563286b9176911faafd9dd'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_suspended_manifest_expectation_patch_drift';
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
        '                ''public.starring_runtime_startup_recovery_execute_reserved_awaiting_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)'',' || E'\n' ||
        '                ''requested_recovery_id text, requested_originating_emergency_generation bigint, requested_coordinator_generation bigint, requested_action_authority_revision bigint, requested_selection_authority_revision bigint, expected_gateway_shard_id text, expected_owner_process_instance_id text, expected_owner_lease_epoch bigint, expected_owner_runtime_build_revision text, expected_owner_revision bigint, expected_owner_expires_at timestamp with time zone, requested_minimum_database_now timestamp with time zone''::TEXT,' || E'\n' ||
        '                ''TABLE(journal_outcome_name text, terminal_outcome_name text, recovery_id text, originating_emergency_generation bigint, coordinator_generation bigint, action_authority_revision bigint, selection_authority_revision bigint, recovery_class text, observed_gateway_shard_id text, observed_process_instance_id text, observed_lease_epoch bigint, observed_runtime_build_revision text, observed_owner_revision bigint, database_now timestamp with time zone, observed_owner_expires_at timestamp with time zone, minimum_database_now timestamp with time zone, recorded_at timestamp with time zone, terminal_projection_bytes bytea, terminal_digest text)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            )';
    next_fragment := previous_fragment || ',' || E'\n' ||
        '            (' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_execute_suspended_local_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint)'',' || E'\n' ||
        '                ''requested_recovery_id text, requested_originating_emergency_generation bigint, requested_coordinator_generation bigint, requested_action_authority_revision bigint, requested_selection_authority_revision bigint, expected_gateway_shard_id text, expected_owner_process_instance_id text, expected_owner_lease_epoch bigint, expected_owner_runtime_build_revision text, expected_owner_revision bigint, expected_owner_expires_at timestamp with time zone, requested_minimum_database_now timestamp with time zone, paused_process_instance_id text, paused_coordinator_generation bigint, paused_connection_epoch bigint, paused_ready_kind text, paused_admission_revision bigint, paused_transition_sequence bigint, paused_connected_event_sequence bigint, paused_last_resume_sequence bigint, registry_process_instance_id text, registry_observation_sequence bigint, registry_retained_slot_count bigint, registry_retained_empty_tombstone_count bigint''::TEXT,' || E'\n' ||
        '                ''TABLE(journal_outcome_name text, terminal_outcome_name text, recovery_id text, originating_emergency_generation bigint, coordinator_generation bigint, action_authority_revision bigint, selection_authority_revision bigint, recovery_class text, observed_gateway_shard_id text, observed_process_instance_id text, observed_lease_epoch bigint, observed_runtime_build_revision text, observed_owner_revision bigint, database_now timestamp with time zone, observed_owner_expires_at timestamp with time zone, minimum_database_now timestamp with time zone, recorded_at timestamp with time zone, terminal_projection_bytes bytea, terminal_digest text)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            )';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_suspended_readiness_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            (''starring_runtime_private_v2.starring_runtime_startup_reserved_projection_exact_v2(bytea,text,bigint,bigint,bigint,bigint,timestamp with time zone,public.runtime_certification_operation_terminals_v2)''),';
    next_fragment := previous_fragment || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_suspended_route_bytes_v2(jsonb)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_suspended_previous_bytes_v2(jsonb)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_suspended_root_exact_v2(public.runtime_suspend_attempt_operations_v2,public.runtime_suspended_attempts_v2)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_suspended_root_frame_v2(public.runtime_suspend_attempt_operations_v2,public.runtime_deployments)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_suspended_sidecar_frame_v2(public.runtime_suspended_attempts_v2)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_suspended_projection_exact_v2(bytea,bytea,bytea,bytea,bytea,bytea)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_suspended_replay_exact_v2(bytea,bytea)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_suspended_terminal_sidecar_v2(bytea,bytea,public.runtime_suspend_attempt_operations_v2,public.runtime_suspended_attempts_v2)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_suspended_quiescent_exact_v2(public.runtime_suspend_attempt_operations_v2,public.runtime_suspended_attempts_v2)''),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_suspended_readiness_private_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '''c2de6cf64ce6efbcf22e31f06da774195996060a692c45b48f073ff93fa4d630''::TEXT';
    next_fragment :=
        '''63268b8e2e30bbe523a437a5c326daa9ef25b863a866d4f1e67fcf46bc98bd95''::TEXT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_suspended_readiness_manifest_digest_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_execute_reserved_awaiting_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)''' || E'\n' ||
        '            )' || E'\n' ||
        '        )';
    next_fragment :=
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_execute_reserved_awaiting_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)''' || E'\n' ||
        '            ),' || E'\n' ||
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_execute_suspended_local_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint)''' || E'\n' ||
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
            MESSAGE = 'runtime_startup_suspended_readiness_allowlist_patch_drift';
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
    invalid_acl_count BIGINT;
    actual_acl_count BIGINT;
    expected_acl_count BIGINT;
    executor_identity_acl_count BIGINT;
    invalid_executor_identity_acl_count BIGINT;
    exact_target_manifest_digest TEXT;
    exact_target_readiness_digest TEXT;
    serving_manifest_digest TEXT;
    serving_readiness_digest TEXT;
    manifest_digest TEXT;
    readiness_digest TEXT;
    observation_digest TEXT;
    executor_digest TEXT;
    suspension_trigger_digest TEXT;
    deployment_validator_digest TEXT;
    convergence_validator_digest TEXT;
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
        pg_catalog.count(*) FILTER (
            WHERE privilege.grantee <> common_owner
        ),
        pg_catalog.count(*) FILTER (
            WHERE privilege.grantee <> common_owner
                AND (
                    privilege.grantor <> common_owner
                    OR privilege.privilege_type <> 'EXECUTE'
                    OR privilege.is_grantable
                    OR privilege.grantee IS DISTINCT FROM executor_role
                )
        )
    INTO
        executor_identity_acl_count,
        invalid_executor_identity_acl_count
    FROM pg_catalog.pg_proc AS capability
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        capability.proacl,
        pg_catalog.acldefault('f', capability.proowner)
    )) AS privilege
    WHERE capability.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_database_identity_v1()'
    );

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_runtime_startup_recovery_execute_suspended_local_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint)',
                'v'::"char",
                'u'::"char",
                TRUE,
                TRUE
            ),
            (
                'starring_runtime_private_v2.starring_runtime_suspended_route_bytes_v2(jsonb)',
                'i'::"char",
                's'::"char",
                FALSE,
                FALSE
            ),
            (
                'starring_runtime_private_v2.starring_runtime_suspended_previous_bytes_v2(jsonb)',
                'i'::"char",
                's'::"char",
                FALSE,
                FALSE
            ),
            (
                'starring_runtime_private_v2.starring_runtime_suspended_root_exact_v2(public.runtime_suspend_attempt_operations_v2,public.runtime_suspended_attempts_v2)',
                's'::"char",
                's'::"char",
                FALSE,
                FALSE
            ),
            (
                'starring_runtime_private_v2.starring_runtime_suspended_root_frame_v2(public.runtime_suspend_attempt_operations_v2,public.runtime_deployments)',
                'i'::"char",
                's'::"char",
                FALSE,
                FALSE
            ),
            (
                'starring_runtime_private_v2.starring_runtime_suspended_sidecar_frame_v2(public.runtime_suspended_attempts_v2)',
                'i'::"char",
                's'::"char",
                FALSE,
                FALSE
            ),
            (
                'starring_runtime_private_v2.starring_runtime_suspended_projection_exact_v2(bytea,bytea,bytea,bytea,bytea,bytea)',
                'i'::"char",
                's'::"char",
                FALSE,
                FALSE
            ),
            (
                'starring_runtime_private_v2.starring_runtime_suspended_replay_exact_v2(bytea,bytea)',
                'i'::"char",
                's'::"char",
                FALSE,
                FALSE
            ),
            (
                'starring_runtime_private_v2.starring_runtime_suspended_terminal_sidecar_v2(bytea,bytea,public.runtime_suspend_attempt_operations_v2,public.runtime_suspended_attempts_v2)',
                'i'::"char",
                's'::"char",
                FALSE,
                FALSE
            ),
            (
                'starring_runtime_private_v2.starring_runtime_suspended_quiescent_exact_v2(public.runtime_suspend_attempt_operations_v2,public.runtime_suspended_attempts_v2)',
                's'::"char",
                's'::"char",
                FALSE,
                FALSE
            )
    ) AS expected(
        identity,
        volatility,
        parallel_kind,
        security_definer,
        returns_set
    )
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid =
            pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> expected.volatility
        OR NOT function_row.proisstrict
        OR function_row.proparallel <> expected.parallel_kind
        OR function_row.prosecdef <> expected.security_definer
        OR function_row.proretset <> expected.returns_set
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[];

    SELECT
        pg_catalog.count(*) FILTER (
            WHERE privilege.grantor <> common_owner
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
                OR (
                    privilege.grantee <> common_owner
                    AND NOT (
                        expected.executor_allowed
                        AND executor_role IS NOT NULL
                        AND privilege.grantee = executor_role
                    )
                )
        ),
        pg_catalog.count(*)
    INTO invalid_acl_count, actual_acl_count
    FROM (
        VALUES
            (
                'public.starring_runtime_startup_recovery_execute_suspended_local_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint)',
                TRUE
            ),
            (
                'starring_runtime_private_v2.starring_runtime_suspended_route_bytes_v2(jsonb)',
                FALSE
            ),
            (
                'starring_runtime_private_v2.starring_runtime_suspended_previous_bytes_v2(jsonb)',
                FALSE
            ),
            (
                'starring_runtime_private_v2.starring_runtime_suspended_root_exact_v2(public.runtime_suspend_attempt_operations_v2,public.runtime_suspended_attempts_v2)',
                FALSE
            ),
            (
                'starring_runtime_private_v2.starring_runtime_suspended_root_frame_v2(public.runtime_suspend_attempt_operations_v2,public.runtime_deployments)',
                FALSE
            ),
            (
                'starring_runtime_private_v2.starring_runtime_suspended_sidecar_frame_v2(public.runtime_suspended_attempts_v2)',
                FALSE
            ),
            (
                'starring_runtime_private_v2.starring_runtime_suspended_projection_exact_v2(bytea,bytea,bytea,bytea,bytea,bytea)',
                FALSE
            ),
            (
                'starring_runtime_private_v2.starring_runtime_suspended_replay_exact_v2(bytea,bytea)',
                FALSE
            ),
            (
                'starring_runtime_private_v2.starring_runtime_suspended_terminal_sidecar_v2(bytea,bytea,public.runtime_suspend_attempt_operations_v2,public.runtime_suspended_attempts_v2)',
                FALSE
            ),
            (
                'starring_runtime_private_v2.starring_runtime_suspended_quiescent_exact_v2(public.runtime_suspend_attempt_operations_v2,public.runtime_suspended_attempts_v2)',
                FALSE
            )
    ) AS expected(identity, executor_allowed)
    INNER JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid =
            pg_catalog.to_regprocedure(expected.identity)
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege;
    expected_acl_count := 10 + CASE
        WHEN executor_role IS NULL THEN 0
        ELSE 1
    END;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO exact_target_manifest_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_exact_target_schema_manifest_v1()'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO exact_target_readiness_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_exact_target_database_readiness_v1()'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO serving_manifest_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_serving_schema_manifest_v1()'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO serving_readiness_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_serving_database_readiness_v1()'
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
    INTO executor_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_startup_recovery_execute_suspended_local_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint)'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO suspension_trigger_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.reject_runtime_suspend_attempt_ledger_mutation_v2()'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO deployment_validator_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.validate_runtime_deployment_projection()'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO convergence_validator_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.validate_runtime_convergence_attempt_projection()'
    );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR invalid_function_count <> 0
        OR invalid_acl_count <> 0
        OR actual_acl_count <> expected_acl_count
        OR executor_identity_acl_count <> (
            CASE
                WHEN executor_role IS NULL THEN 0
                ELSE 1
            END
        )
        OR invalid_executor_identity_acl_count <> 0
        OR manifest_digest IS DISTINCT FROM
            '63268b8e2e30bbe523a437a5c326daa9ef25b863a866d4f1e67fcf46bc98bd95'
        OR readiness_digest IS DISTINCT FROM
            '7526d7365225da6514fcc589d76c316dd1363c40cad30e12e3f752b4c85e8044'
        OR observation_digest IS DISTINCT FROM
            'bd2844074b41d4c8723c44ce482be8aa943b2354904de42bf9260dce0376aab1'
        OR executor_digest IS DISTINCT FROM
            '8722480a7845c98d290eaf0292d2073cedea2b69807d958201c3618f5d3c7aa1'
        OR suspension_trigger_digest IS DISTINCT FROM
            'f78e63ec1d4529840e0bae7b114c95ce128fab02bf3b7d9220740f75e25a5f23'
        OR deployment_validator_digest IS DISTINCT FROM
            '6b9d750c7ddee0d4e4c2034efaaaedfa8e45dca96f1e9d709360731cabd012c4'
        OR convergence_validator_digest IS DISTINCT FROM
            'a8be65eb734c5b1fa50edadecbe4547ea96f594800b984d36d327cff246ff252'
        OR exact_target_manifest_digest IS DISTINCT FROM
            '4633e8a3b8dc31d8ddde8d872969b42bdd25a6d98edaf7f59ec3076f3fa4f728'
        OR exact_target_readiness_digest IS DISTINCT FROM
            '5110e2d4b5846a64550e2ed55c219a0d010d3ed944a986eca2e927ce345276ad'
        OR serving_manifest_digest IS DISTINCT FROM
            '2c8957777b2d4a7f1b6050b21e8a5664b5fcff45d4732627bc8e961823a4eada'
        OR serving_readiness_digest IS DISTINCT FROM
            'aa6b4f686ff76b627a7850e6ea110cb5ecf4c2e20c1a252b56b9b91bfee1cf27'
        OR NOT public.starring_runtime_exact_target_schema_manifest_v1()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_suspended_local_execution_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
