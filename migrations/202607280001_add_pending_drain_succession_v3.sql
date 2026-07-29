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
            ('starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(bytea,smallint,smallint)'),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_succession_projection_v3(bytea,bytea,bytea,bytea)'),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_predecessor_exact_v3(public.runtime_drain_intents_v2,public.runtime_startup_recovery_actions_v2)'),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_successor_exact_v3(public.runtime_drain_intents_v2,public.runtime_drain_intents_v2,public.runtime_deployments,public.runtime_deployments,bytea,text)'),
            ('public.starring_runtime_startup_recovery_select_pending_drain_v3(text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)'),
            ('public.starring_runtime_startup_recovery_pending_drain_succession_v3(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean)')
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
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_roles AS role
            WHERE role.oid = executor_role
                AND role.rolcanlogin
        )
        OR executor_membership_count <> 0
        OR other_client_session_count <> 0
        OR prepared_transaction_count <> 0
        OR collision_count <> 0
        OR manifest_digest IS DISTINCT FROM
            '9de93ea5d565254c47533c7af43959aa873014bee385a2af775fafdcbf8118b9'
        OR readiness_digest IS DISTINCT FROM
            '1c20dcc6c6e01b440d9a5813bad12b109d89a67c5d6815f9fd15551fa3c0f4e5'
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
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_succession_preflight_drift';
    END IF;
END;
$preflight$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(
    projection_bytes BYTEA,
    expected_outcome_tag SMALLINT,
    requested_frame_index SMALLINT
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
    domain_bytes BYTEA;
    projection_length BIGINT;
    cursor_position BIGINT;
    frame_index INTEGER;
    frame_length BIGINT;
    requested_value BYTEA;
    payload_start BIGINT;
    payload_end BIGINT;
    payload_value BYTEA;
BEGIN
    IF expected_outcome_tag NOT BETWEEN 0 AND 3
        OR requested_frame_index NOT BETWEEN 1 AND 4
    THEN
        RETURN NULL;
    END IF;

    domain_bytes := CASE
        WHEN expected_outcome_tag = 3 THEN
            pg_catalog.convert_to(
                'starring.runtime.startup_recovery.pending_drain.succession.terminal.v3',
                'UTF8'
            )
        ELSE
            pg_catalog.convert_to(
                'starring.runtime.startup_recovery.pending_drain.terminal.v2',
                'UTF8'
            )
    END;
    projection_length := pg_catalog.octet_length(projection_bytes);
    IF projection_length NOT BETWEEN 1 AND 131072
        OR pg_catalog.substr(
            projection_bytes,
            1,
            8
        ) IS DISTINCT FROM pg_catalog.int8send(
            pg_catalog.octet_length(domain_bytes)::BIGINT
        )
        OR pg_catalog.substr(
            projection_bytes,
            9,
            pg_catalog.octet_length(domain_bytes)
        ) IS DISTINCT FROM domain_bytes
    THEN
        RETURN NULL;
    END IF;

    cursor_position := 9 + pg_catalog.octet_length(domain_bytes);
    IF pg_catalog.substr(
            projection_bytes,
            cursor_position::INTEGER,
            2
        ) IS DISTINCT FROM pg_catalog.int2send(
            CASE
                WHEN expected_outcome_tag = 3 THEN 3
                ELSE 2
            END::SMALLINT
        )
        OR pg_catalog.substr(
            projection_bytes,
            (cursor_position + 2)::INTEGER,
            2
        ) IS DISTINCT FROM pg_catalog.int2send(expected_outcome_tag)
    THEN
        RETURN NULL;
    END IF;
    cursor_position := cursor_position + 4;
    payload_start := cursor_position;

    FOR frame_index IN 1..4 LOOP
        IF cursor_position + 7 > projection_length THEN
            RETURN NULL;
        END IF;
        frame_length := (
            pg_catalog.get_byte(
                projection_bytes,
                cursor_position::INTEGER - 1
            )::NUMERIC * 72057594037927936
            + pg_catalog.get_byte(
                projection_bytes,
                cursor_position::INTEGER
            )::NUMERIC * 281474976710656
            + pg_catalog.get_byte(
                projection_bytes,
                cursor_position::INTEGER + 1
            )::NUMERIC * 1099511627776
            + pg_catalog.get_byte(
                projection_bytes,
                cursor_position::INTEGER + 2
            )::NUMERIC * 4294967296
            + pg_catalog.get_byte(
                projection_bytes,
                cursor_position::INTEGER + 3
            )::NUMERIC * 16777216
            + pg_catalog.get_byte(
                projection_bytes,
                cursor_position::INTEGER + 4
            )::NUMERIC * 65536
            + pg_catalog.get_byte(
                projection_bytes,
                cursor_position::INTEGER + 5
            )::NUMERIC * 256
            + pg_catalog.get_byte(
                projection_bytes,
                cursor_position::INTEGER + 6
            )::NUMERIC
        )::BIGINT;
        cursor_position := cursor_position + 8;
        IF frame_length < 0
            OR cursor_position + frame_length - 1 > projection_length
        THEN
            RETURN NULL;
        END IF;
        IF frame_index = requested_frame_index THEN
            requested_value := pg_catalog.substr(
                projection_bytes,
                cursor_position::INTEGER,
                frame_length::INTEGER
            );
        END IF;
        cursor_position := cursor_position + frame_length;
    END LOOP;

    payload_end := cursor_position - 1;
    IF cursor_position + 31 <> projection_length THEN
        RETURN NULL;
    END IF;
    payload_value := pg_catalog.substr(
        projection_bytes,
        payload_start::INTEGER,
        (payload_end - payload_start + 1)::INTEGER
    );
    IF pg_catalog.substr(
            projection_bytes,
            cursor_position::INTEGER,
            32
        ) IS DISTINCT FROM pg_catalog.sha256(payload_value)
    THEN
        RETURN NULL;
    END IF;
    RETURN requested_value;
END;
$function$;

CREATE FUNCTION public.starring_runtime_startup_recovery_pending_drain_succession_v3(
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
    registry_retained_empty_tombstone_count BIGINT,
    requested_selected_drain_intent_id TEXT,
    requested_selected_source_intent_revision BIGINT,
    requested_selected_source_state_digest TEXT,
    requested_predecessor_claim_terminal_digest TEXT,
    requested_pre_slot_present BOOLEAN,
    requested_pre_slot_admission_generation BIGINT,
    requested_pre_slot_observation_sequence BIGINT,
    requested_seal_key BYTEA,
    requested_seal_generation BIGINT,
    requested_post_slot_admission_generation BIGINT,
    requested_post_slot_observation_sequence BIGINT,
    requested_post_global_observation_sequence BIGINT,
    requested_post_global_retained_slot_count BIGINT,
    requested_post_global_retained_empty_tombstone_count BIGINT,
    requested_post_global_staged_route_count BIGINT,
    requested_post_global_serving_route_count BIGINT,
    requested_post_global_draining_route_count BIGINT,
    requested_post_global_sealed_slot_count BIGINT,
    requested_post_global_active_interaction_count BIGINT,
    requested_post_global_failed_closed_slot_count BIGINT,
    requested_post_global_registry_failed_closed BOOLEAN
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
    predecessor_action_row public.runtime_startup_recovery_actions_v2%ROWTYPE;
    source_drain_row public.runtime_drain_intents_v2%ROWTYPE;
    candidate_drain_row public.runtime_drain_intents_v2%ROWTYPE;
    successor_drain_row public.runtime_drain_intents_v2%ROWTYPE;
    product_row public.runtime_product_operations_v2%ROWTYPE;
    slot_fence_row public.runtime_slot_writer_fences_v2%ROWTYPE;
    serving_row public.runtime_serving_leases%ROWTYPE;
    reservation_row public.runtime_certification_operations_v2%ROWTYPE;
    certification_terminal_row public.runtime_certification_operation_terminals_v2%ROWTYPE;
    deployment_row public.runtime_deployments%ROWTYPE;
    successor_deployment_row public.runtime_deployments%ROWTYPE;
    action_record RECORD;
    selection_action_found BOOLEAN;
    authority_action_found BOOLEAN;
    writer_fence_count BIGINT;
    invalid_drain_count BIGINT;
    active_pending_count BIGINT;
    candidate_count BIGINT;
    candidate_id TEXT;
    matching_certification_count BIGINT;
    selected_drain_intent_id TEXT;
    state_value JSONB;
    predecessor_frame_value JSONB;
    transition_frame_value JSONB;
    predecessor_frame BYTEA;
    successor_frame BYTEA;
    evidence_frame BYTEA;
    transition_frame BYTEA;
    progressed_projection BYTEA;
    seal_bundle BYTEA;
    last_resume_frame BYTEA;
    ready_kind_tag SMALLINT;
    request_text TEXT;
    product_request_value JSONB;
    drain_request_value JSONB;
    expected_product_bytes BYTEA;
    key_text TEXT;
    certification_text TEXT;
    provenance_text TEXT;
    successor_claim_text TEXT;
    successor_text TEXT;
    successor_bytes BYTEA;
    successor_digest TEXT;
    successor_controller_id TEXT;
    successor_snapshot JSONB;
    successor_revision BIGINT;
    successor_claim_revision BIGINT;
    successor_fencing_token BIGINT;
    predecessor_controller_id TEXT;
    predecessor_recovery_id TEXT;
    predecessor_action_revision BIGINT;
    predecessor_claim_expiry_numeric NUMERIC;
    database_now_numeric NUMERIC;
    owner_expiry_numeric NUMERIC;
    acknowledged_numeric NUMERIC;
    owner_expiry_unix_microseconds BIGINT;
    acknowledged_unix_microseconds BIGINT;
BEGIN
    PERFORM pg_catalog.set_config('TimeZone', 'UTC', TRUE);
    IF pg_catalog.current_setting('transaction_isolation')
            <> 'serializable'
        OR pg_catalog.current_setting('transaction_read_only') <> 'off'
        OR requested_recovery_id !~ '^[0-9a-f]{32}$'
        OR requested_originating_emergency_generation
            NOT BETWEEN 1 AND 9223372036854775806
        OR requested_coordinator_generation
            <> requested_originating_emergency_generation + 1
        OR requested_selection_authority_revision
            NOT BETWEEN 1 AND 9223372036854775806
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
        OR requested_selected_drain_intent_id
            !~ '^[0-9a-f]{32}$'
        OR requested_selected_source_intent_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR requested_selected_source_state_digest
            !~ '^[0-9a-f]{64}$'
        OR requested_predecessor_claim_terminal_digest
            !~ '^[0-9a-f]{64}$'
        OR pg_catalog.octet_length(requested_seal_key) <> 16
        OR pg_catalog.encode(requested_seal_key, 'hex')
            IS DISTINCT FROM requested_selected_drain_intent_id
        OR requested_seal_generation
            NOT BETWEEN 1 AND 9223372036854775807
        OR requested_post_slot_admission_generation
            NOT BETWEEN 1 AND 9223372036854775807
        OR requested_post_slot_observation_sequence
            NOT BETWEEN 1 AND 9223372036854775807
        OR requested_post_global_observation_sequence::NUMERIC
            <> registry_observation_sequence::NUMERIC + 1
        OR requested_post_global_retained_slot_count
            NOT BETWEEN 1 AND 9223372036854775807
        OR requested_post_global_retained_empty_tombstone_count
            NOT BETWEEN 0 AND 9223372036854775807
        OR requested_post_global_staged_route_count <> 0
        OR requested_post_global_serving_route_count <> 0
        OR requested_post_global_draining_route_count <> 0
        OR requested_post_global_sealed_slot_count <> 1
        OR requested_post_global_active_interaction_count <> 0
        OR requested_post_global_failed_closed_slot_count <> 0
        OR requested_post_global_registry_failed_closed
        OR (
            NOT requested_pre_slot_present
            AND (
                requested_pre_slot_admission_generation <> 0
                OR requested_pre_slot_observation_sequence <> 0
                OR requested_seal_generation <> 1
                OR requested_post_slot_admission_generation <> 1
                OR requested_post_slot_observation_sequence <> 1
                OR requested_post_global_retained_slot_count::NUMERIC
                    <> registry_retained_slot_count::NUMERIC + 1
                OR requested_post_global_retained_empty_tombstone_count
                    <> registry_retained_empty_tombstone_count
            )
        )
        OR (
            requested_pre_slot_present
            AND (
                requested_pre_slot_admission_generation
                    NOT BETWEEN 1 AND 9223372036854775806
                OR requested_pre_slot_observation_sequence
                    NOT BETWEEN 1 AND 9223372036854775806
                OR requested_post_slot_admission_generation
                    <> requested_pre_slot_admission_generation + 1
                OR requested_post_slot_observation_sequence
                    <> requested_pre_slot_observation_sequence + 1
                OR registry_retained_empty_tombstone_count < 1
                OR requested_post_global_retained_slot_count
                    <> registry_retained_slot_count
                OR requested_post_global_retained_empty_tombstone_count
                    <> registry_retained_empty_tombstone_count - 1
            )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_pending_drain_succession_input_invalid';
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
    seal_bundle :=
        pg_catalog.int2send(3::SMALLINT)
        || pg_catalog.int2send(
            CASE
                WHEN requested_pre_slot_present THEN 1
                ELSE 0
            END::SMALLINT
        )
        || CASE
            WHEN requested_pre_slot_present
            THEN
                pg_catalog.int8send(
                    requested_pre_slot_admission_generation
                )
                || pg_catalog.int8send(
                    requested_pre_slot_observation_sequence
                )
            ELSE ''::BYTEA
        END
        || requested_seal_key
        || pg_catalog.int8send(requested_seal_generation)
        || pg_catalog.int8send(
            requested_post_slot_admission_generation
        )
        || pg_catalog.int8send(
            requested_post_slot_observation_sequence
        )
        || pg_catalog.int8send(
            requested_post_global_observation_sequence
        )
        || pg_catalog.int8send(
            requested_post_global_retained_slot_count
        )
        || pg_catalog.int8send(
            requested_post_global_retained_empty_tombstone_count
        )
        || pg_catalog.int8send(
            requested_post_global_staged_route_count
        )
        || pg_catalog.int8send(
            requested_post_global_serving_route_count
        )
        || pg_catalog.int8send(
            requested_post_global_draining_route_count
        )
        || pg_catalog.int8send(
            requested_post_global_sealed_slot_count
        )
        || pg_catalog.int8send(
            requested_post_global_active_interaction_count
        )
        || pg_catalog.int8send(
            requested_post_global_failed_closed_slot_count
        )
        || pg_catalog.int2send(
            CASE
                WHEN requested_post_global_registry_failed_closed
                THEN 1
                ELSE 0
            END::SMALLINT
        );
    evidence_frame :=
        pg_catalog.convert_to(requested_recovery_id, 'UTF8')
        || pg_catalog.int8send(
            requested_originating_emergency_generation
        )
        || pg_catalog.int8send(requested_coordinator_generation)
        || pg_catalog.int8send(requested_action_authority_revision)
        || pg_catalog.int8send(
            requested_selection_authority_revision
        )
        || pg_catalog.int8send(
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
        )
        || pg_catalog.int8send(
            pg_catalog.octet_length(seal_bundle)::BIGINT
        )
        || seal_bundle;

    predecessor_frame := pg_catalog.convert_to(
        pg_catalog.jsonb_build_object(
            'drain_intent_id',
            requested_selected_drain_intent_id,
            'source_intent_revision',
            requested_selected_source_intent_revision,
            'source_state_digest',
            requested_selected_source_state_digest,
            'predecessor_claim_terminal_digest',
            requested_predecessor_claim_terminal_digest
        )::TEXT,
        'UTF8'
    );

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
            MESSAGE = 'runtime_pending_drain_succession_owner_lost';
    END IF;
    IF database_now < requested_minimum_database_now THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_pending_drain_succession_clock_regressed';
    END IF;

    SELECT action.*
    INTO selection_action_row
    FROM public.runtime_startup_recovery_actions_v2 AS action
    WHERE action.recovery_id = requested_recovery_id
        AND action.selection_authority_revision =
            requested_selection_authority_revision
    FOR UPDATE;
    selection_action_found := FOUND;
    SELECT action.*
    INTO authority_action_row
    FROM public.runtime_startup_recovery_actions_v2 AS action
    WHERE action.recovery_id = requested_recovery_id
        AND action.action_authority_revision =
            requested_action_authority_revision
    FOR UPDATE;
    authority_action_found := FOUND;

    IF selection_action_found OR authority_action_found THEN
        existing_action_row := CASE
            WHEN selection_action_found THEN selection_action_row
            ELSE authority_action_row
        END;
        predecessor_frame :=
            starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(
                existing_action_row.terminal_projection_bytes,
                3::SMALLINT,
                1::SMALLINT
            );
        successor_frame :=
            starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(
                existing_action_row.terminal_projection_bytes,
                3::SMALLINT,
                2::SMALLINT
            );
        transition_frame :=
            starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(
                existing_action_row.terminal_projection_bytes,
                3::SMALLINT,
                4::SMALLINT
            );
        BEGIN
            predecessor_frame_value :=
                pg_catalog.convert_from(
                    predecessor_frame,
                    'UTF8'
                )::JSONB;
            transition_frame_value :=
                pg_catalog.convert_from(
                    transition_frame,
                    'UTF8'
                )::JSONB;
        EXCEPTION
            WHEN OTHERS THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RX004',
                    MESSAGE = 'runtime_pending_drain_succession_replay_invalid';
        END;
        IF predecessor_frame_value ->> 'drain_intent_id'
                IS DISTINCT FROM requested_selected_drain_intent_id
            OR predecessor_frame_value
                    ->> 'source_intent_revision'
                IS DISTINCT FROM
                    requested_selected_source_intent_revision::TEXT
            OR predecessor_frame_value ->> 'source_state_digest'
                IS DISTINCT FROM
                    requested_selected_source_state_digest
            OR predecessor_frame_value
                    ->> 'predecessor_claim_terminal_digest'
                IS DISTINCT FROM
                    requested_predecessor_claim_terminal_digest
            OR starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(
                    existing_action_row.terminal_projection_bytes,
                    3::SMALLINT,
                    3::SMALLINT
                ) IS DISTINCT FROM evidence_frame
            OR successor_frame IS NULL
            OR transition_frame_value ->> 'successor_state_digest'
                IS DISTINCT FROM pg_catalog.encode(
                    pg_catalog.sha256(successor_frame),
                    'hex'
                )
            OR existing_action_row.minimum_database_now
                IS DISTINCT FROM requested_minimum_database_now
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_pending_drain_succession_replay_invalid';
        END IF;

        SELECT record.*
        INTO STRICT action_record
        FROM starring_runtime_private_v2.starring_runtime_startup_recovery_action_record_v2(
            requested_recovery_id,
            requested_originating_emergency_generation,
            requested_coordinator_generation,
            requested_action_authority_revision,
            requested_selection_authority_revision,
            'pending_runtime_drain_intent',
            expected_gateway_shard_id,
            expected_owner_process_instance_id,
            expected_owner_lease_epoch,
            expected_owner_runtime_build_revision,
            expected_owner_revision,
            expected_owner_expires_at,
            requested_minimum_database_now,
            existing_action_row.terminal_projection_bytes
        ) AS record;
        IF action_record.outcome_name <> 'replayed'
            OR action_record.database_now < database_now
            OR action_record.database_now >= expected_owner_expires_at
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_pending_drain_succession_replay_invalid';
        END IF;

        journal_outcome_name := action_record.outcome_name;
        terminal_outcome_name := 'route_absent_acknowledged';
        recovery_id := requested_recovery_id;
        originating_emergency_generation :=
            requested_originating_emergency_generation;
        coordinator_generation := requested_coordinator_generation;
        action_authority_revision :=
            requested_action_authority_revision;
        selection_authority_revision :=
            requested_selection_authority_revision;
        recovery_class := 'pending_runtime_drain_intent';
        observed_gateway_shard_id :=
            action_record.observed_gateway_shard_id;
        observed_process_instance_id :=
            action_record.observed_process_instance_id;
        observed_lease_epoch := action_record.observed_lease_epoch;
        observed_runtime_build_revision :=
            action_record.observed_runtime_build_revision;
        observed_owner_revision :=
            action_record.observed_owner_revision;
        database_now := action_record.database_now;
        observed_owner_expires_at :=
            action_record.observed_owner_expires_at;
        minimum_database_now :=
            existing_action_row.minimum_database_now;
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
            MESSAGE = 'runtime_pending_drain_succession_state_ambiguous';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_drain_count
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE drain.intent_state IN (
            'pending',
            'route_absent_acknowledged'
        )
        AND NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
            drain
        );
    IF invalid_drain_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_pending_drain_succession_state_ambiguous';
    END IF;

    SELECT candidate.*
    INTO STRICT candidate_count, candidate_id
    FROM starring_runtime_private_v2.starring_runtime_pending_drain_candidate_v2()
        AS candidate;
    active_pending_count := candidate_count;
    selected_drain_intent_id := candidate_id;
    IF candidate_count = 0
        OR candidate_id IS NULL
        OR candidate_id
            IS DISTINCT FROM requested_selected_drain_intent_id
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_pending_drain_succession_selection_changed';
    END IF;

    SELECT drain.*
    INTO candidate_drain_row
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE drain.drain_intent_id = selected_drain_intent_id;
    IF NOT FOUND
        OR candidate_drain_row.intent_state <> 'pending'
        OR candidate_drain_row.intent_revision
            IS DISTINCT FROM
                requested_selected_source_intent_revision
        OR candidate_drain_row.canonical_state_digest
            IS DISTINCT FROM requested_selected_source_state_digest
        OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
            candidate_drain_row
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_pending_drain_succession_candidate_changed';
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            pg_catalog.concat(
                'starring-runtime-serving-slot-v1:',
                candidate_drain_row.slot_guild_id,
                ':',
                candidate_drain_row.slot_ruleset_key
            ),
            0
        )
    );
    SELECT slot.*
    INTO slot_fence_row
    FROM public.runtime_slot_writer_fences_v2 AS slot
    WHERE slot.slot_guild_id =
            candidate_drain_row.slot_guild_id
        AND slot.slot_ruleset_key =
            candidate_drain_row.slot_ruleset_key
    FOR UPDATE;
    IF NOT FOUND
        OR slot_fence_row.writer_epoch
            NOT BETWEEN 1 AND 9223372036854775807
        OR slot_fence_row.pending_drain_intent_id
            IS DISTINCT FROM candidate_drain_row.drain_intent_id
        OR slot_fence_row.pending_product_operation_id
            IS DISTINCT FROM candidate_drain_row.product_operation_id
        OR slot_fence_row.pending_tenant_id
            IS DISTINCT FROM candidate_drain_row.tenant_id
        OR slot_fence_row.pending_installation_id
            IS DISTINCT FROM candidate_drain_row.installation_id
        OR slot_fence_row.pending_deployment_id
            IS DISTINCT FROM candidate_drain_row.deployment_id
        OR slot_fence_row.pending_expected_revision
            IS DISTINCT FROM candidate_drain_row.expected_revision
        OR slot_fence_row.pending_marked_at IS NULL
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_pending_drain_succession_slot_invalid';
    END IF;

    SELECT serving.*
    INTO serving_row
    FROM public.runtime_serving_leases AS serving
    WHERE serving.guild_id = candidate_drain_row.slot_guild_id
        AND serving.ruleset_key =
            candidate_drain_row.slot_ruleset_key
    FOR UPDATE;
    IF FOUND
        AND (
            serving_row.connected
            OR serving_row.serving
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_pending_drain_succession_serving_conflict';
    END IF;

    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = candidate_drain_row.tenant_id
        AND deployment.installation_id =
            candidate_drain_row.installation_id
        AND deployment.deployment_id =
            candidate_drain_row.deployment_id
    FOR UPDATE;
    IF NOT FOUND
        OR deployment_row.revision
            IS DISTINCT FROM candidate_drain_row.expected_revision
        OR deployment_row.guild_id
            IS DISTINCT FROM candidate_drain_row.slot_guild_id
        OR deployment_row.ruleset_key
            IS DISTINCT FROM candidate_drain_row.slot_ruleset_key
        OR deployment_row.controller_id IS NOT NULL
        OR deployment_row.controller_fencing_token IS NOT NULL
        OR deployment_row.controller_acquired_at IS NOT NULL
        OR deployment_row.controller_lease_expires_at IS NOT NULL
        OR deployment_row.snapshot -> 'controller_lease'
            IS DISTINCT FROM 'null'::JSONB
        OR deployment_row.last_fencing_token
            NOT BETWEEN 1 AND 9223372036854775806
        OR deployment_row.last_controller_id IS NULL
        OR deployment_row.snapshot ->> 'last_fencing_token'
            IS DISTINCT FROM deployment_row.last_fencing_token::TEXT
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_pending_drain_succession_deployment_invalid';
    END IF;

    SELECT product.*
    INTO product_row
    FROM public.runtime_product_operations_v2 AS product
    WHERE product.product_operation_id =
            candidate_drain_row.product_operation_id
        AND product.product_mutation_digest =
            candidate_drain_row.product_mutation_digest
        AND product.tenant_id = candidate_drain_row.tenant_id
        AND product.installation_id =
            candidate_drain_row.installation_id
        AND product.deployment_id =
            candidate_drain_row.deployment_id
        AND product.expected_revision =
            candidate_drain_row.expected_revision
        AND product.expected_target_guild_id =
            candidate_drain_row.slot_guild_id
        AND product.expected_target_ruleset_key =
            candidate_drain_row.slot_ruleset_key
    FOR UPDATE;
    BEGIN
        product_request_value := pg_catalog.convert_from(
            product_row.product_mutation_request_bytes,
            'UTF8'
        )::JSONB;
        drain_request_value := pg_catalog.convert_from(
            candidate_drain_row.drain_intent_request_bytes,
            'UTF8'
        )::JSONB;
        expected_product_bytes :=
            starring_runtime_private_v2.starring_runtime_product_mutation_bytes_v2(
                product_row.product_operation_id,
                product_row.tenant_id,
                product_row.installation_id,
                product_row.deployment_id,
                product_row.expected_revision,
                product_row.expected_target_guild_id,
                product_row.expected_target_ruleset_key,
                product_row.expected_target_guild_id,
                product_row.expected_target_ruleset_key,
                product_row.expected_target_version,
                product_row.expected_target_content_hash,
                product_row.expected_target_binding_revision,
                product_row.expected_target_binding_fingerprint,
                product_request_value ->> 'mutation_kind',
                product_request_value
                    ->> 'product_semantic_request_digest'
            );
    EXCEPTION
        WHEN OTHERS THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_pending_drain_succession_product_root_invalid';
    END;
    IF product_row.product_operation_id IS NULL
        OR expected_product_bytes
            IS DISTINCT FROM
                product_row.product_mutation_request_bytes
        OR starring_runtime_private_v2.starring_runtime_product_mutation_digest_v2(
                expected_product_bytes
            ) IS DISTINCT FROM product_row.product_mutation_digest
        OR drain_request_value #>> '{key,intent_id}'
            IS DISTINCT FROM candidate_drain_row.drain_intent_id
        OR drain_request_value #>> '{key,product_operation_id}'
            IS DISTINCT FROM candidate_drain_row.product_operation_id
        OR drain_request_value #>> '{key,product_mutation_digest}'
            IS DISTINCT FROM
                candidate_drain_row.product_mutation_digest
        OR drain_request_value #>> '{key,expected_revision}'
            IS DISTINCT FROM
                candidate_drain_row.expected_revision::TEXT
        OR drain_request_value #>> '{key,expected_target,guild_id}'
            IS DISTINCT FROM product_row.expected_target_guild_id
        OR drain_request_value
                #>> '{key,expected_target,ruleset_key}'
            IS DISTINCT FROM
                product_row.expected_target_ruleset_key
        OR drain_request_value #>> '{key,expected_target,version}'
            IS DISTINCT FROM
                product_row.expected_target_version::TEXT
        OR drain_request_value
                #>> '{key,expected_target,content_hash}'
            IS DISTINCT FROM
                product_row.expected_target_content_hash
        OR drain_request_value
                #>> '{key,expected_target,binding_revision}'
            IS DISTINCT FROM
                product_row.expected_target_binding_revision::TEXT
        OR drain_request_value
                #>> '{key,expected_target,binding_fingerprint}'
            IS DISTINCT FROM
                product_row.expected_target_binding_fingerprint
        OR product_row.expected_target_version
            IS DISTINCT FROM deployment_row.target_version
        OR product_row.expected_target_content_hash
            IS DISTINCT FROM deployment_row.target_content_hash
        OR product_row.expected_target_binding_revision
            IS DISTINCT FROM deployment_row.binding_revision
        OR product_row.expected_target_binding_fingerprint
            IS DISTINCT FROM deployment_row.binding_fingerprint
        OR deployment_row.snapshot #>> '{target,guild_id}'
            IS DISTINCT FROM product_row.expected_target_guild_id
        OR deployment_row.snapshot #>> '{target,ruleset_key}'
            IS DISTINCT FROM
                product_row.expected_target_ruleset_key
        OR deployment_row.snapshot #>> '{target,version}'
            IS DISTINCT FROM
                product_row.expected_target_version::TEXT
        OR deployment_row.snapshot #>> '{target,content_hash}'
            IS DISTINCT FROM
                product_row.expected_target_content_hash
        OR deployment_row.snapshot #>> '{target,binding_revision}'
            IS DISTINCT FROM
                product_row.expected_target_binding_revision::TEXT
        OR deployment_row.snapshot #>> '{target,binding_fingerprint}'
            IS DISTINCT FROM
                product_row.expected_target_binding_fingerprint
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_pending_drain_succession_product_invalid';
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            pg_catalog.concat(
                'starring-runtime-drain-intent-v2:',
                selected_drain_intent_id
            ),
            0
        )
    );
    SELECT drain.*
    INTO source_drain_row
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE drain.drain_intent_id = selected_drain_intent_id
    FOR UPDATE;
    IF NOT FOUND
        OR pg_catalog.to_jsonb(source_drain_row)
            IS DISTINCT FROM pg_catalog.to_jsonb(candidate_drain_row)
        OR source_drain_row.intent_state <> 'pending'
        OR source_drain_row.intent_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
            source_drain_row
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_pending_drain_succession_candidate_changed';
    END IF;

    state_value := pg_catalog.convert_from(
        source_drain_row.canonical_state_bytes,
        'UTF8'
    )::JSONB;
    predecessor_controller_id :=
        state_value #>> '{state,claim,controller_id}';
    IF state_value #>> '{state,kind}' <> 'pending_claimed'
        OR state_value #>> '{state,claim,progress,kind}'
            <> 'claimed'
        OR state_value
                #> '{state,claim,progress,seal,expected_route}'
            IS DISTINCT FROM 'null'::JSONB
        OR predecessor_controller_id
            !~ '^recovery:[0-9a-f]{32}:[1-9][0-9]{0,18}$'
        OR (
            pg_catalog.split_part(
                predecessor_controller_id,
                ':',
                3
            )::NUMERIC
        ) > 9223372036854775807
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_pending_drain_succession_predecessor_invalid';
    END IF;
    predecessor_recovery_id :=
        pg_catalog.split_part(predecessor_controller_id, ':', 2);
    predecessor_action_revision :=
        pg_catalog.split_part(
            predecessor_controller_id,
            ':',
            3
        )::BIGINT;
    predecessor_claim_expiry_numeric := (
        state_value
            #>> '{state,claim,claim_expires_at_unix_microseconds}'
    )::NUMERIC;
    database_now_numeric :=
        EXTRACT(EPOCH FROM database_now) * 1000000;
    IF database_now_numeric <>
            pg_catalog.trunc(database_now_numeric)
        OR database_now_numeric < predecessor_claim_expiry_numeric
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_pending_drain_succession_claim_fresh';
    END IF;

    SELECT pg_catalog.count(*)
    INTO matching_certification_count
    FROM public.runtime_certification_operations_v2 AS reservation
    WHERE reservation.tenant_id = source_drain_row.tenant_id
        AND reservation.installation_id =
            source_drain_row.installation_id
        AND reservation.deployment_id =
            source_drain_row.deployment_id
        AND reservation.deployment_revision =
            source_drain_row.expected_revision;
    IF matching_certification_count > 1 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_pending_drain_succession_certification_ambiguous';
    ELSIF matching_certification_count = 1 THEN
        SELECT reservation.*
        INTO STRICT reservation_row
        FROM public.runtime_certification_operations_v2 AS reservation
        WHERE reservation.tenant_id = source_drain_row.tenant_id
            AND reservation.installation_id =
                source_drain_row.installation_id
            AND reservation.deployment_id =
                source_drain_row.deployment_id
            AND reservation.deployment_revision =
                source_drain_row.expected_revision
        FOR UPDATE;
        SELECT terminal.*
        INTO certification_terminal_row
        FROM public.runtime_certification_operation_terminals_v2 AS terminal
        WHERE terminal.operation_id = reservation_row.operation_id
        FOR UPDATE;
        IF NOT FOUND THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX003',
                MESSAGE = 'runtime_pending_drain_succession_certification_pending';
        ELSIF certification_terminal_row.intent_fingerprint
                IS DISTINCT FROM reservation_row.intent_fingerprint
            OR certification_terminal_row.tenant_id
                IS DISTINCT FROM reservation_row.tenant_id
            OR certification_terminal_row.installation_id
                IS DISTINCT FROM reservation_row.installation_id
            OR certification_terminal_row.deployment_id
                IS DISTINCT FROM reservation_row.deployment_id
            OR certification_terminal_row.deployment_revision
                IS DISTINCT FROM reservation_row.deployment_revision
            OR certification_terminal_row.convergence_attempt_no
                IS DISTINCT FROM reservation_row.convergence_attempt_no
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_pending_drain_succession_certification_invalid';
        ELSIF certification_terminal_row.terminal_outcome_name =
                'awaiting_reset'
        THEN
            certification_text := pg_catalog.concat(
                '{"kind":"no_attestation_for_reserved_operation",',
                '"operation_id":',
                pg_catalog.to_json(
                    reservation_row.operation_id
                )::TEXT,
                ',"intent_fingerprint":',
                pg_catalog.to_json(
                    reservation_row.intent_fingerprint
                )::TEXT,
                '}'
            );
        ELSE
            RAISE EXCEPTION USING
                ERRCODE = 'RX003',
                MESSAGE = 'runtime_pending_drain_succession_certification_committed';
        END IF;
    ELSE
        certification_text :=
            '{"kind":"no_operation_reserved"}';
    END IF;

    SELECT action.*
    INTO predecessor_action_row
    FROM public.runtime_startup_recovery_actions_v2 AS action
    WHERE action.recovery_id = predecessor_recovery_id
        AND action.action_authority_revision =
            predecessor_action_revision
    FOR SHARE;
    IF NOT FOUND
        OR predecessor_action_row.terminal_digest
            IS DISTINCT FROM
                requested_predecessor_claim_terminal_digest
        OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_predecessor_exact_v3(
            source_drain_row,
            predecessor_action_row
        )
        OR state_value
                #>> '{state,claim,gateway_owner_lease_id,gateway_shard_id}'
            IS DISTINCT FROM expected_gateway_shard_id
        OR state_value
                #>> '{state,claim,gateway_owner_lease_id,process_instance_id}'
            IS NOT DISTINCT FROM expected_owner_process_instance_id
        OR (
            state_value
                #>> '{state,claim,gateway_owner_lease_id,lease_epoch}'
        )::BIGINT >= expected_owner_lease_epoch
        OR state_value #>> '{state,claim,controller_id}'
            IS DISTINCT FROM deployment_row.last_controller_id
        OR (
            state_value
                #>> '{state,claim,controller_fencing_token}'
        )::BIGINT <> deployment_row.last_fencing_token
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_pending_drain_succession_predecessor_invalid';
    END IF;

    IF (state_value #>> '{state,claim,claim_revision}')::NUMERIC
            > 9223372036854775806
        OR (state_value
                #>> '{state,claim,controller_fencing_token}')::NUMERIC
            > 9223372036854775806
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_pending_drain_succession_revision_overflow';
    END IF;
    successor_revision := source_drain_row.intent_revision + 1;
    successor_claim_revision :=
        (state_value #>> '{state,claim,claim_revision}')::BIGINT + 1;
    successor_fencing_token :=
        deployment_row.last_fencing_token + 1;
    successor_controller_id := pg_catalog.concat(
        'recovery:',
        requested_recovery_id,
        ':',
        requested_action_authority_revision::TEXT
    );
    owner_expiry_numeric :=
        EXTRACT(EPOCH FROM expected_owner_expires_at) * 1000000;
    acknowledged_numeric :=
        EXTRACT(EPOCH FROM database_now) * 1000000;
    IF owner_expiry_numeric <>
            pg_catalog.trunc(owner_expiry_numeric)
        OR acknowledged_numeric <>
            pg_catalog.trunc(acknowledged_numeric)
        OR owner_expiry_numeric NOT BETWEEN
            -9223372036854775808 AND 9223372036854775807
        OR acknowledged_numeric NOT BETWEEN
            -9223372036854775808 AND 9223372036854775807
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_pending_drain_succession_time_invalid';
    END IF;
    owner_expiry_unix_microseconds :=
        owner_expiry_numeric::BIGINT;
    acknowledged_unix_microseconds :=
        acknowledged_numeric::BIGINT;
    request_text := pg_catalog.convert_from(
        source_drain_row.drain_intent_request_bytes,
        'UTF8'
    );
    key_text := pg_catalog.substr(
        request_text,
        27,
        pg_catalog.length(request_text) - 27
    );
    successor_claim_text := pg_catalog.concat(
        '{"gateway_owner_lease_id":{"gateway_shard_id":',
        pg_catalog.to_json(expected_gateway_shard_id)::TEXT,
        ',"process_instance_id":',
        pg_catalog.to_json(
            expected_owner_process_instance_id
        )::TEXT,
        ',"lease_epoch":',
        expected_owner_lease_epoch::TEXT,
        ',"expected_build_revision":',
        pg_catalog.to_json(
            expected_owner_runtime_build_revision
        )::TEXT,
        '},"observed_owner_revision":',
        expected_owner_revision::TEXT,
        ',"process_instance_id":',
        pg_catalog.to_json(
            expected_owner_process_instance_id
        )::TEXT,
        ',"controller_id":',
        pg_catalog.to_json(successor_controller_id)::TEXT,
        ',"controller_fencing_token":',
        successor_fencing_token::TEXT,
        ',"claim_epoch":',
        requested_coordinator_generation::TEXT,
        ',"claim_revision":',
        successor_claim_revision::TEXT,
        ',"claim_expires_at_unix_microseconds":',
        owner_expiry_unix_microseconds::TEXT,
        ',"progress":{"kind":"claimed","seal":',
        '{"process_instance_id":',
        pg_catalog.to_json(
            expected_owner_process_instance_id
        )::TEXT,
        ',"slot":{"guild_id":',
        pg_catalog.to_json(
            source_drain_row.slot_guild_id
        )::TEXT,
        ',"ruleset_key":',
        pg_catalog.to_json(
            source_drain_row.slot_ruleset_key
        )::TEXT,
        '},"intent_id":',
        pg_catalog.to_json(
            source_drain_row.drain_intent_id
        )::TEXT,
        ',"seal_generation":',
        requested_seal_generation::TEXT,
        ',"expected_route":null,',
        '"registry_observation_sequence":',
        requested_post_slot_observation_sequence::TEXT,
        '}}}'
    );
    provenance_text := pg_catalog.concat(
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
        pg_catalog.to_json(
            expected_owner_process_instance_id
        )::TEXT,
        ',"lease_epoch":',
        expected_owner_lease_epoch::TEXT,
        ',"expected_build_revision":',
        pg_catalog.to_json(
            expected_owner_runtime_build_revision
        )::TEXT,
        '},"observed_owner_revision":',
        expected_owner_revision::TEXT,
        ',"owner_expires_at_unix_microseconds":',
        owner_expiry_unix_microseconds::TEXT,
        ',"process_instance_id":',
        pg_catalog.to_json(
            expected_owner_process_instance_id
        )::TEXT,
        ',"connection_epoch":',
        paused_connection_epoch::TEXT,
        ',"paused_admission_revision":',
        paused_admission_revision::TEXT,
        ',"connected_event_sequence":',
        paused_connected_event_sequence::TEXT,
        ',"pause_sequence":',
        paused_transition_sequence::TEXT,
        '}}'
    );
    successor_text := pg_catalog.concat(
        '{"format_version":2,"root":{"key":',
        key_text,
        ',"drain_intent_digest":',
        pg_catalog.to_json(
            source_drain_row.drain_intent_digest
        )::TEXT,
        '},"intent_revision":',
        successor_revision::TEXT,
        ',"state":{"kind":"route_absent_acknowledged",',
        '"acknowledgement":{"claim":',
        successor_claim_text,
        ',"expected_route":null,',
        '"provenance_json":',
        pg_catalog.to_json(provenance_text)::TEXT,
        ',"registry_observation_sequence":',
        requested_post_global_observation_sequence::TEXT,
        ',"certification":',
        certification_text,
        ',',
        '"acknowledged_at_unix_microseconds":',
        acknowledged_unix_microseconds::TEXT,
        '}}}'
    );
    successor_bytes := pg_catalog.convert_to(
        successor_text,
        'UTF8'
    );
    successor_digest := pg_catalog.encode(
        pg_catalog.sha256(successor_bytes),
        'hex'
    );
    IF pg_catalog.octet_length(successor_bytes)
            NOT BETWEEN 1 AND 1048576
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_pending_drain_succession_successor_oversized';
    END IF;

    successor_snapshot := pg_catalog.jsonb_set(
        deployment_row.snapshot,
        '{last_fencing_token}',
        pg_catalog.to_jsonb(successor_fencing_token),
        FALSE
    );
    PERFORM public.starring_runtime_mutation_clock();
    PERFORM pg_catalog.set_config(
        'starring.runtime_pending_drain_deployment_action_v2',
        'advance_history',
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_pending_drain_deployment_id_v2',
        deployment_row.deployment_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_pending_drain_source_fence_v2',
        deployment_row.last_fencing_token::TEXT,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_pending_drain_successor_fence_v2',
        successor_fencing_token::TEXT,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_pending_drain_successor_controller_v2',
        successor_controller_id,
        TRUE
    );
    UPDATE public.runtime_deployments AS deployment
    SET snapshot = successor_snapshot,
        last_fencing_token = successor_fencing_token,
        last_controller_id = successor_controller_id
    WHERE deployment.deployment_id =
            deployment_row.deployment_id
        AND deployment.revision = deployment_row.revision
        AND deployment.controller_id IS NULL
        AND deployment.last_fencing_token =
            deployment_row.last_fencing_token
        AND deployment.last_controller_id =
            deployment_row.last_controller_id
    RETURNING deployment.* INTO successor_deployment_row;
    IF NOT FOUND
        OR successor_deployment_row.snapshot
            IS DISTINCT FROM successor_snapshot
        OR successor_deployment_row.last_fencing_token
            IS DISTINCT FROM successor_fencing_token
        OR successor_deployment_row.last_controller_id
            IS DISTINCT FROM successor_controller_id
        OR COALESCE(pg_catalog.current_setting(
                'starring.runtime_pending_drain_deployment_action_v2',
                TRUE
            ), '') <> 'advance_history'
        OR COALESCE(pg_catalog.current_setting(
                'starring.runtime_pending_drain_deployment_id_v2',
                TRUE
            ), '') <> deployment_row.deployment_id
        OR COALESCE(pg_catalog.current_setting(
                'starring.runtime_pending_drain_source_fence_v2',
                TRUE
            ), '') <> deployment_row.last_fencing_token::TEXT
        OR COALESCE(pg_catalog.current_setting(
                'starring.runtime_pending_drain_successor_fence_v2',
                TRUE
            ), '') <> successor_fencing_token::TEXT
        OR COALESCE(pg_catalog.current_setting(
                'starring.runtime_pending_drain_successor_controller_v2',
                TRUE
            ), '') <> successor_controller_id
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_pending_drain_succession_fence_invalid';
    END IF;
    PERFORM pg_catalog.set_config(
        'starring.runtime_pending_drain_deployment_action_v2',
        '',
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_pending_drain_deployment_id_v2',
        '',
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_pending_drain_source_fence_v2',
        '',
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_pending_drain_successor_fence_v2',
        '',
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_pending_drain_successor_controller_v2',
        '',
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_mutation_clock',
        '',
        TRUE
    );

    PERFORM pg_catalog.set_config(
        'starring.runtime_product_drain_first_apply_stage_v2',
        'pending_drain_recovery_update',
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_product_drain_first_apply_product_operation_id_v2',
        source_drain_row.product_operation_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_product_drain_first_apply_drain_intent_id_v2',
        source_drain_row.drain_intent_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_pending_drain_source_revision_v2',
        source_drain_row.intent_revision::TEXT,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_pending_drain_source_digest_v2',
        source_drain_row.canonical_state_digest,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_pending_drain_successor_revision_v2',
        successor_revision::TEXT,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_pending_drain_successor_digest_v2',
        successor_digest,
        TRUE
    );
    UPDATE public.runtime_drain_intents_v2 AS drain
    SET intent_revision = successor_revision,
        intent_state = 'route_absent_acknowledged',
        canonical_state_bytes = successor_bytes,
        canonical_state_digest = successor_digest
    WHERE drain.drain_intent_id =
            source_drain_row.drain_intent_id
        AND drain.intent_revision =
            source_drain_row.intent_revision
        AND drain.canonical_state_digest =
            source_drain_row.canonical_state_digest
    RETURNING drain.* INTO successor_drain_row;
    IF NOT FOUND
        OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_successor_exact_v3(
            source_drain_row,
            successor_drain_row,
            deployment_row,
            successor_deployment_row,
            successor_bytes,
            successor_digest
        )
        OR COALESCE(pg_catalog.current_setting(
                'starring.runtime_product_drain_first_apply_stage_v2',
                TRUE
            ), '') <> ''
        OR COALESCE(pg_catalog.current_setting(
                'starring.runtime_product_drain_first_apply_product_operation_id_v2',
                TRUE
            ), '') <> ''
        OR COALESCE(pg_catalog.current_setting(
                'starring.runtime_product_drain_first_apply_drain_intent_id_v2',
                TRUE
            ), '') <> ''
        OR COALESCE(pg_catalog.current_setting(
                'starring.runtime_pending_drain_source_revision_v2',
                TRUE
            ), '') <> ''
        OR COALESCE(pg_catalog.current_setting(
                'starring.runtime_pending_drain_source_digest_v2',
                TRUE
            ), '') <> ''
        OR COALESCE(pg_catalog.current_setting(
                'starring.runtime_pending_drain_successor_revision_v2',
                TRUE
            ), '') <> ''
        OR COALESCE(pg_catalog.current_setting(
                'starring.runtime_pending_drain_successor_digest_v2',
                TRUE
            ), '') <> ''
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_pending_drain_succession_cas_invalid';
    END IF;

    predecessor_frame := pg_catalog.convert_to(
        pg_catalog.jsonb_build_object(
            'drain_intent_id',
            source_drain_row.drain_intent_id,
            'source_intent_revision',
            source_drain_row.intent_revision,
            'source_state_digest',
            source_drain_row.canonical_state_digest,
            'predecessor_claim_terminal_digest',
            predecessor_action_row.terminal_digest,
            'predecessor_gateway_shard_id',
            state_value
                #>> '{state,claim,gateway_owner_lease_id,gateway_shard_id}',
            'predecessor_process_instance_id',
            state_value
                #>> '{state,claim,gateway_owner_lease_id,process_instance_id}',
            'predecessor_lease_epoch',
            (
                state_value
                    #>> '{state,claim,gateway_owner_lease_id,lease_epoch}'
            )::BIGINT,
            'predecessor_runtime_build_revision',
            state_value
                #>> '{state,claim,gateway_owner_lease_id,expected_build_revision}',
            'predecessor_owner_revision',
            (state_value #>> '{state,claim,observed_owner_revision}')::BIGINT,
            'predecessor_controller_id',
            predecessor_controller_id,
            'predecessor_controller_fencing_token',
            (
                state_value
                    #>> '{state,claim,controller_fencing_token}'
            )::BIGINT,
            'predecessor_claim_epoch',
            (state_value #>> '{state,claim,claim_epoch}')::BIGINT,
            'predecessor_claim_revision',
            (state_value #>> '{state,claim,claim_revision}')::BIGINT,
            'predecessor_claim_expires_at_unix_microseconds',
            predecessor_claim_expiry_numeric,
            'predecessor_seal_process_instance_id',
            state_value
                #>> '{state,claim,progress,seal,process_instance_id}',
            'predecessor_seal_generation',
            (
                state_value
                    #>> '{state,claim,progress,seal,seal_generation}'
            )::BIGINT,
            'predecessor_seal_observation_sequence',
            (
                state_value
                    #>> '{state,claim,progress,seal,registry_observation_sequence}'
            )::BIGINT,
            'predecessor_claim_source_digest',
            pg_catalog.convert_from(
                starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(
                    predecessor_action_row.terminal_projection_bytes,
                    1::SMALLINT,
                    1::SMALLINT
                ),
                'UTF8'
            )
        )::TEXT,
        'UTF8'
    );
    transition_frame := pg_catalog.convert_to(
        pg_catalog.jsonb_build_object(
            'tenant_id',
            source_drain_row.tenant_id,
            'installation_id',
            source_drain_row.installation_id,
            'deployment_id',
            source_drain_row.deployment_id,
            'expected_revision',
            source_drain_row.expected_revision,
            'product_operation_id',
            source_drain_row.product_operation_id,
            'product_mutation_digest',
            source_drain_row.product_mutation_digest,
            'product_mutation_request_digest',
            pg_catalog.encode(
                pg_catalog.sha256(
                    product_row.product_mutation_request_bytes
                ),
                'hex'
            ),
            'drain_intent_digest',
            source_drain_row.drain_intent_digest,
            'drain_intent_request_digest',
            pg_catalog.encode(
                pg_catalog.sha256(
                    source_drain_row.drain_intent_request_bytes
                ),
                'hex'
            ),
            'slot_guild_id',
            source_drain_row.slot_guild_id,
            'slot_ruleset_key',
            source_drain_row.slot_ruleset_key,
            'target_version',
            product_row.expected_target_version,
            'target_content_hash',
            product_row.expected_target_content_hash,
            'target_binding_revision',
            product_row.expected_target_binding_revision,
            'target_binding_fingerprint',
            product_row.expected_target_binding_fingerprint,
            'source_fencing_token',
            deployment_row.last_fencing_token,
            'successor_fencing_token',
            successor_fencing_token,
            'successor_controller_id',
            successor_controller_id,
            'successor_claim_revision',
            successor_claim_revision,
            'successor_intent_revision',
            successor_revision,
            'successor_state_digest',
            successor_digest,
            'certification',
            certification_text::JSONB,
            'database_now_unix_microseconds',
            acknowledged_unix_microseconds
        )::TEXT,
        'UTF8'
    );
    progressed_projection :=
        starring_runtime_private_v2.starring_runtime_pending_drain_succession_projection_v3(
            predecessor_frame,
            successor_bytes,
            evidence_frame,
            transition_frame
        );
    IF progressed_projection IS NULL
        OR pg_catalog.octet_length(progressed_projection)
            NOT BETWEEN 1 AND 131072
        OR starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(
                progressed_projection,
                3::SMALLINT,
                1::SMALLINT
            ) IS DISTINCT FROM predecessor_frame
        OR starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(
                progressed_projection,
                3::SMALLINT,
                2::SMALLINT
            ) IS DISTINCT FROM successor_bytes
        OR starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(
                progressed_projection,
                3::SMALLINT,
                3::SMALLINT
            ) IS DISTINCT FROM evidence_frame
        OR starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(
                progressed_projection,
                3::SMALLINT,
                4::SMALLINT
            ) IS DISTINCT FROM transition_frame
        OR pg_catalog.strpos(
                pg_catalog.convert_from(predecessor_frame, 'UTF8'),
                pg_catalog.convert_from(
                    source_drain_row.canonical_state_bytes,
                    'UTF8'
                )
            ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_pending_drain_succession_projection_invalid';
    END IF;

    SELECT record.*
    INTO STRICT action_record
    FROM starring_runtime_private_v2.starring_runtime_startup_recovery_action_record_v2(
        requested_recovery_id,
        requested_originating_emergency_generation,
        requested_coordinator_generation,
        requested_action_authority_revision,
        requested_selection_authority_revision,
        'pending_runtime_drain_intent',
        expected_gateway_shard_id,
        expected_owner_process_instance_id,
        expected_owner_lease_epoch,
        expected_owner_runtime_build_revision,
        expected_owner_revision,
        expected_owner_expires_at,
        requested_minimum_database_now,
        progressed_projection
    ) AS record;
    IF action_record.outcome_name <> 'applied'
        OR action_record.database_now < database_now
        OR action_record.database_now >= expected_owner_expires_at
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_pending_drain_succession_record_invalid';
    END IF;

    journal_outcome_name := action_record.outcome_name;
    terminal_outcome_name := 'route_absent_acknowledged';
    recovery_id := requested_recovery_id;
    originating_emergency_generation :=
        requested_originating_emergency_generation;
    coordinator_generation := requested_coordinator_generation;
    action_authority_revision :=
        requested_action_authority_revision;
    selection_authority_revision :=
        requested_selection_authority_revision;
    recovery_class := 'pending_runtime_drain_intent';
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
    terminal_projection_bytes := progressed_projection;
    terminal_digest := action_record.terminal_digest;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_pending_drain_predecessor_exact_v3(
    drain_row public.runtime_drain_intents_v2,
    action_row public.runtime_startup_recovery_actions_v2
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
    state_value JSONB;
    claim_value JSONB;
    controller_id TEXT;
    controller_recovery_id TEXT;
    controller_action_revision BIGINT;
    claim_expiry_numeric NUMERIC;
    action_expiry_numeric NUMERIC;
    prior_source_digest_frame BYTEA;
    prior_successor_state_frame BYTEA;
    prior_evidence_frame BYTEA;
    prior_product_frame BYTEA;
    cursor_position BIGINT;
    root_length BIGINT;
    frame_length BIGINT;
    token_kind TEXT;
    prior_tag SMALLINT;
    token_kinds TEXT[];
    token_index INTEGER;
    selected_drain_intent_id_value BYTEA;
    prior_seal_bundle BYTEA;
    seal_cursor BIGINT;
    pre_slot_tag SMALLINT;
BEGIN
    IF drain_row.intent_state <> 'pending'
        OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
            drain_row
        )
    THEN
        RETURN FALSE;
    END IF;
    state_value := pg_catalog.convert_from(
        drain_row.canonical_state_bytes,
        'UTF8'
    )::JSONB;
    claim_value := state_value #> '{state,claim}';
    controller_id := claim_value ->> 'controller_id';
    IF state_value #>> '{state,kind}' <> 'pending_claimed'
        OR state_value #>> '{state,claim,progress,kind}' <> 'claimed'
        OR state_value
                #> '{state,claim,progress,seal,expected_route}'
            IS DISTINCT FROM 'null'::JSONB
        OR controller_id
            !~ '^recovery:[0-9a-f]{32}:[1-9][0-9]{0,18}$'
        OR claim_value
                #>> '{gateway_owner_lease_id,gateway_shard_id}'
            <> 'shard:0'
        OR claim_value
                #>> '{gateway_owner_lease_id,process_instance_id}'
            !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR claim_value
                #>> '{gateway_owner_lease_id,expected_build_revision}'
            !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR claim_value ->> 'process_instance_id'
            IS DISTINCT FROM claim_value
                #>> '{gateway_owner_lease_id,process_instance_id}'
        OR claim_value
                #>> '{progress,seal,process_instance_id}'
            IS DISTINCT FROM claim_value ->> 'process_instance_id'
        OR claim_value #>> '{progress,seal,intent_id}'
            IS DISTINCT FROM drain_row.drain_intent_id
        OR claim_value #>> '{progress,seal,slot,guild_id}'
            IS DISTINCT FROM drain_row.slot_guild_id
        OR claim_value #>> '{progress,seal,slot,ruleset_key}'
            IS DISTINCT FROM drain_row.slot_ruleset_key
        OR claim_value #>> '{gateway_owner_lease_id,lease_epoch}'
            !~ '^[1-9][0-9]{0,18}$'
        OR claim_value ->> 'observed_owner_revision'
            !~ '^[1-9][0-9]{0,18}$'
        OR claim_value ->> 'controller_fencing_token'
            !~ '^[1-9][0-9]{0,18}$'
        OR claim_value ->> 'claim_epoch'
            !~ '^[1-9][0-9]{0,18}$'
        OR claim_value ->> 'claim_revision'
            !~ '^[1-9][0-9]{0,18}$'
        OR claim_value ->> 'claim_expires_at_unix_microseconds'
            !~ '^-?[0-9]{1,19}$'
        OR claim_value #>> '{progress,seal,seal_generation}'
            !~ '^[1-9][0-9]{0,18}$'
        OR claim_value
                #>> '{progress,seal,registry_observation_sequence}'
            !~ '^[1-9][0-9]{0,18}$'
    THEN
        RETURN FALSE;
    END IF;

    controller_recovery_id :=
        pg_catalog.split_part(controller_id, ':', 2);
    controller_action_revision :=
        pg_catalog.split_part(controller_id, ':', 3)::BIGINT;
    claim_expiry_numeric :=
        (claim_value ->> 'claim_expires_at_unix_microseconds')::NUMERIC;
    action_expiry_numeric :=
        EXTRACT(EPOCH FROM action_row.owner_expires_at) * 1000000;
    prior_source_digest_frame :=
        starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(
            action_row.terminal_projection_bytes,
            1::SMALLINT,
            1::SMALLINT
        );
    prior_successor_state_frame :=
        starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(
            action_row.terminal_projection_bytes,
            1::SMALLINT,
            2::SMALLINT
        );
    prior_evidence_frame :=
        starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(
            action_row.terminal_projection_bytes,
            1::SMALLINT,
            3::SMALLINT
        );
    prior_product_frame :=
        starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(
            action_row.terminal_projection_bytes,
            1::SMALLINT,
            4::SMALLINT
        );

    root_length := pg_catalog.octet_length(prior_product_frame);
    IF root_length NOT BETWEEN 1 AND 131072
        OR pg_catalog.substr(
            prior_product_frame,
            1,
            2
        ) IS DISTINCT FROM pg_catalog.int2send(2::SMALLINT)
    THEN
        RETURN FALSE;
    END IF;
    cursor_position := 3;
    token_kinds := ARRAY[
        'f', 'f', 'f', 'i', 'f',
        'f', 'f', 'f', 'f', 'f', 'i', 'f',
        'f', 'f', 'i', 'f', 'i', 'f',
        'f', 'f', 'f', 'f'
    ];
    token_index := 0;
    FOREACH token_kind IN ARRAY token_kinds
    LOOP
        token_index := token_index + 1;
        IF token_kind = 'i' THEN
            IF cursor_position + 7 > root_length THEN
                RETURN FALSE;
            END IF;
            cursor_position := cursor_position + 8;
        ELSE
            IF cursor_position + 7 > root_length THEN
                RETURN FALSE;
            END IF;
            frame_length := (
                pg_catalog.get_byte(
                    prior_product_frame,
                    cursor_position::INTEGER - 1
                )::NUMERIC * 72057594037927936
                + pg_catalog.get_byte(
                    prior_product_frame,
                    cursor_position::INTEGER
                )::NUMERIC * 281474976710656
                + pg_catalog.get_byte(
                    prior_product_frame,
                    cursor_position::INTEGER + 1
                )::NUMERIC * 1099511627776
                + pg_catalog.get_byte(
                    prior_product_frame,
                    cursor_position::INTEGER + 2
                )::NUMERIC * 4294967296
                + pg_catalog.get_byte(
                    prior_product_frame,
                    cursor_position::INTEGER + 3
                )::NUMERIC * 16777216
                + pg_catalog.get_byte(
                    prior_product_frame,
                    cursor_position::INTEGER + 4
                )::NUMERIC * 65536
                + pg_catalog.get_byte(
                    prior_product_frame,
                    cursor_position::INTEGER + 5
                )::NUMERIC * 256
                + pg_catalog.get_byte(
                    prior_product_frame,
                    cursor_position::INTEGER + 6
                )::NUMERIC
            )::BIGINT;
            cursor_position := cursor_position + 8;
            IF frame_length < 0
                OR cursor_position + frame_length - 1 > root_length
            THEN
                RETURN FALSE;
            END IF;
            IF token_index = 12 THEN
                selected_drain_intent_id_value :=
                    pg_catalog.substr(
                        prior_product_frame,
                        cursor_position::INTEGER,
                        frame_length::INTEGER
                    );
            END IF;
            cursor_position := cursor_position + frame_length;
        END IF;
    END LOOP;
    IF selected_drain_intent_id_value IS DISTINCT FROM
            pg_catalog.convert_to(
                drain_row.drain_intent_id,
                'UTF8'
            )
        OR cursor_position + 19 > root_length
        OR pg_catalog.substr(
                prior_product_frame,
                cursor_position::INTEGER,
                8
            ) IS DISTINCT FROM pg_catalog.int8send(
                drain_row.intent_revision - 1
            )
        OR pg_catalog.substr(
                prior_product_frame,
                (cursor_position + 8)::INTEGER,
                8
            ) IS DISTINCT FROM pg_catalog.int8send(
                action_row.action_authority_revision
            )
        OR pg_catalog.substr(
                prior_product_frame,
                (cursor_position + 16)::INTEGER,
                2
            ) IS DISTINCT FROM pg_catalog.int2send(1::SMALLINT)
        OR pg_catalog.substr(
                prior_product_frame,
                (cursor_position + 18)::INTEGER,
                2
            ) IS DISTINCT FROM pg_catalog.int2send(0::SMALLINT)
    THEN
        RETURN FALSE;
    END IF;
    cursor_position := cursor_position + 20;
    IF cursor_position + 7 > root_length THEN
        RETURN FALSE;
    END IF;
    frame_length := (
        pg_catalog.get_byte(
            prior_product_frame,
            cursor_position::INTEGER - 1
        )::NUMERIC * 72057594037927936
        + pg_catalog.get_byte(
            prior_product_frame,
            cursor_position::INTEGER
        )::NUMERIC * 281474976710656
        + pg_catalog.get_byte(
            prior_product_frame,
            cursor_position::INTEGER + 1
        )::NUMERIC * 1099511627776
        + pg_catalog.get_byte(
            prior_product_frame,
            cursor_position::INTEGER + 2
        )::NUMERIC * 4294967296
        + pg_catalog.get_byte(
            prior_product_frame,
            cursor_position::INTEGER + 3
        )::NUMERIC * 16777216
        + pg_catalog.get_byte(
            prior_product_frame,
            cursor_position::INTEGER + 4
        )::NUMERIC * 65536
        + pg_catalog.get_byte(
            prior_product_frame,
            cursor_position::INTEGER + 5
        )::NUMERIC * 256
        + pg_catalog.get_byte(
            prior_product_frame,
            cursor_position::INTEGER + 6
        )::NUMERIC
    )::BIGINT;
    cursor_position := cursor_position + 8;
    IF frame_length NOT BETWEEN 1 AND 4096
        OR cursor_position + frame_length - 1 > root_length
    THEN
        RETURN FALSE;
    END IF;
    prior_seal_bundle := pg_catalog.substr(
        prior_product_frame,
        cursor_position::INTEGER,
        frame_length::INTEGER
    );
    IF pg_catalog.substr(
            prior_seal_bundle,
            1,
            2
        ) IS DISTINCT FROM pg_catalog.int2send(2::SMALLINT)
    THEN
        RETURN FALSE;
    END IF;
    pre_slot_tag := CASE
        WHEN pg_catalog.substr(
                prior_seal_bundle,
                3,
                2
            ) = pg_catalog.int2send(0::SMALLINT)
        THEN 0
        WHEN pg_catalog.substr(
                prior_seal_bundle,
                3,
                2
            ) = pg_catalog.int2send(1::SMALLINT)
        THEN 1
        ELSE -1
    END;
    seal_cursor := CASE pre_slot_tag
        WHEN 0 THEN 5
        WHEN 1 THEN 21
        ELSE 0
    END;
    IF seal_cursor = 0
        OR seal_cursor + 39 > frame_length
        OR pg_catalog.substr(
                prior_seal_bundle,
                seal_cursor::INTEGER,
                16
            ) IS DISTINCT FROM pg_catalog.decode(
                drain_row.drain_intent_id,
                'hex'
            )
        OR pg_catalog.substr(
                prior_seal_bundle,
                (seal_cursor + 16)::INTEGER,
                8
            ) IS DISTINCT FROM pg_catalog.int8send(
                (
                    claim_value
                        #>> '{progress,seal,seal_generation}'
                )::BIGINT
            )
        OR pg_catalog.substr(
                prior_seal_bundle,
                (seal_cursor + 32)::INTEGER,
                8
            ) IS DISTINCT FROM pg_catalog.int8send(
                (
                    claim_value
                        #>> '{progress,seal,registry_observation_sequence}'
                )::BIGINT
            )
        OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_product_root_compound_exact_v2(
            prior_product_frame,
            drain_row.drain_intent_id,
            drain_row.intent_revision - 1,
            action_row.action_authority_revision,
            1::SMALLINT,
            '',
            prior_seal_bundle
        )
    THEN
        RETURN FALSE;
    END IF;

    RETURN action_row.record_format_version = 2
        AND action_row.recovery_id = controller_recovery_id
        AND action_row.action_authority_revision =
            controller_action_revision
        AND action_row.selection_authority_revision =
            controller_action_revision - 1
        AND action_row.recovery_class =
            'pending_runtime_drain_intent'
        AND action_row.gateway_shard_id =
            claim_value
                #>> '{gateway_owner_lease_id,gateway_shard_id}'
        AND action_row.owner_process_instance_id =
            claim_value
                #>> '{gateway_owner_lease_id,process_instance_id}'
        AND action_row.owner_lease_epoch =
            (claim_value
                #>> '{gateway_owner_lease_id,lease_epoch}')::BIGINT
        AND action_row.owner_runtime_build_revision =
            claim_value
                #>> '{gateway_owner_lease_id,expected_build_revision}'
        AND action_row.owner_revision =
            (claim_value ->> 'observed_owner_revision')::BIGINT
        AND action_row.coordinator_generation =
            (claim_value ->> 'claim_epoch')::BIGINT
        AND action_row.originating_emergency_generation
            BETWEEN 1 AND 9223372036854775806
        AND action_row.coordinator_generation =
            action_row.originating_emergency_generation + 1
        AND action_row.minimum_database_now <= action_row.recorded_at
        AND action_row.recorded_at < action_row.owner_expires_at
        AND action_expiry_numeric =
            pg_catalog.trunc(action_expiry_numeric)
        AND action_expiry_numeric = claim_expiry_numeric
        AND action_row.terminal_digest =
            starring_runtime_private_v2.starring_runtime_startup_recovery_terminal_digest_v2(
                action_row.record_format_version,
                action_row.recovery_id,
                action_row.originating_emergency_generation,
                action_row.coordinator_generation,
                action_row.action_authority_revision,
                action_row.selection_authority_revision,
                action_row.recovery_class,
                action_row.gateway_shard_id,
                action_row.owner_process_instance_id,
                action_row.owner_lease_epoch,
                action_row.owner_runtime_build_revision,
                action_row.owner_revision,
                action_row.owner_expires_at,
                action_row.minimum_database_now,
                action_row.recorded_at,
                action_row.terminal_projection_bytes
            )
        AND prior_source_digest_frame IS NOT NULL
        AND pg_catalog.octet_length(prior_source_digest_frame) = 64
        AND pg_catalog.convert_from(
                prior_source_digest_frame,
                'UTF8'
            ) ~ '^[0-9a-f]{64}$'
        AND prior_successor_state_frame =
            drain_row.canonical_state_bytes
        AND pg_catalog.encode(
                pg_catalog.sha256(prior_successor_state_frame),
                'hex'
            ) = drain_row.canonical_state_digest
        AND prior_evidence_frame IS NOT NULL
        AND pg_catalog.octet_length(prior_evidence_frame)
            BETWEEN 1 AND 16384
        AND prior_product_frame IS NOT NULL
        AND pg_catalog.octet_length(prior_product_frame)
            BETWEEN 1 AND 131072;
EXCEPTION
    WHEN OTHERS THEN
        RETURN FALSE;
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_pending_drain_successor_exact_v3(
    source_drain public.runtime_drain_intents_v2,
    successor_drain public.runtime_drain_intents_v2,
    source_deployment public.runtime_deployments,
    successor_deployment public.runtime_deployments,
    expected_successor_bytes BYTEA,
    expected_successor_digest TEXT
)
RETURNS BOOLEAN
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
BEGIN
    RETURN source_drain.intent_state = 'pending'
        AND successor_drain.intent_state =
            'route_absent_acknowledged'
        AND source_drain.intent_revision
            BETWEEN 1 AND 9223372036854775806
        AND successor_drain.intent_revision =
            source_drain.intent_revision + 1
        AND successor_drain.canonical_state_bytes =
            expected_successor_bytes
        AND successor_drain.canonical_state_digest =
            expected_successor_digest
        AND expected_successor_digest =
            pg_catalog.encode(
                pg_catalog.sha256(expected_successor_bytes),
                'hex'
            )
        AND starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
            successor_drain
        )
        AND pg_catalog.to_jsonb(successor_drain)
                - ARRAY[
                    'intent_revision',
                    'intent_state',
                    'canonical_state_bytes',
                    'canonical_state_digest'
                ]
            = pg_catalog.to_jsonb(source_drain)
                - ARRAY[
                    'intent_revision',
                    'intent_state',
                    'canonical_state_bytes',
                    'canonical_state_digest'
                ]
        AND source_deployment.controller_id IS NULL
        AND source_deployment.controller_fencing_token IS NULL
        AND source_deployment.controller_acquired_at IS NULL
        AND source_deployment.controller_lease_expires_at IS NULL
        AND source_deployment.last_fencing_token
            BETWEEN 1 AND 9223372036854775806
        AND successor_deployment.last_fencing_token =
            source_deployment.last_fencing_token + 1
        AND successor_deployment.last_controller_id IS NOT NULL
        AND successor_deployment.snapshot =
            pg_catalog.jsonb_set(
                source_deployment.snapshot,
                '{last_fencing_token}',
                pg_catalog.to_jsonb(
                    successor_deployment.last_fencing_token
                ),
                FALSE
            )
        AND successor_deployment.snapshot #>> '{last_fencing_token}'
            = successor_deployment.last_fencing_token::TEXT
        AND pg_catalog.to_jsonb(successor_deployment)
                - ARRAY[
                    'snapshot',
                    'last_fencing_token',
                    'last_controller_id'
                ]
            = pg_catalog.to_jsonb(source_deployment)
                - ARRAY[
                    'snapshot',
                    'last_fencing_token',
                    'last_controller_id'
                ];
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_pending_drain_succession_projection_v3(
    predecessor_frame BYTEA,
    successor_state_frame BYTEA,
    recovery_evidence_frame BYTEA,
    transition_frame BYTEA
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
    domain_bytes BYTEA;
    framed_payload BYTEA;
    projection_bytes BYTEA;
BEGIN
    IF pg_catalog.octet_length(predecessor_frame)
            NOT BETWEEN 1 AND 8192
        OR pg_catalog.octet_length(successor_state_frame)
            NOT BETWEEN 1 AND 1048576
        OR pg_catalog.octet_length(recovery_evidence_frame)
            NOT BETWEEN 1 AND 16384
        OR pg_catalog.octet_length(transition_frame)
            NOT BETWEEN 1 AND 16384
    THEN
        RETURN NULL;
    END IF;
    domain_bytes := pg_catalog.convert_to(
        'starring.runtime.startup_recovery.pending_drain.succession.terminal.v3',
        'UTF8'
    );
    framed_payload :=
        pg_catalog.int8send(
            pg_catalog.octet_length(predecessor_frame)::BIGINT
        )
        || predecessor_frame
        || pg_catalog.int8send(
            pg_catalog.octet_length(successor_state_frame)::BIGINT
        )
        || successor_state_frame
        || pg_catalog.int8send(
            pg_catalog.octet_length(recovery_evidence_frame)::BIGINT
        )
        || recovery_evidence_frame
        || pg_catalog.int8send(
            pg_catalog.octet_length(transition_frame)::BIGINT
        )
        || transition_frame;
    projection_bytes :=
        pg_catalog.int8send(
            pg_catalog.octet_length(domain_bytes)::BIGINT
        )
        || domain_bytes
        || pg_catalog.int2send(3::SMALLINT)
        || pg_catalog.int2send(3::SMALLINT)
        || framed_payload
        || pg_catalog.sha256(framed_payload);
    IF pg_catalog.octet_length(projection_bytes)
            NOT BETWEEN 1 AND 131072
    THEN
        RETURN NULL;
    END IF;
    RETURN projection_bytes;
END;
$function$;

CREATE FUNCTION public.starring_runtime_startup_recovery_select_pending_drain_v3(
    expected_gateway_shard_id TEXT,
    expected_owner_process_instance_id TEXT,
    expected_owner_lease_epoch BIGINT,
    expected_owner_runtime_build_revision TEXT,
    expected_owner_revision BIGINT,
    expected_owner_expires_at TIMESTAMPTZ,
    requested_minimum_database_now TIMESTAMPTZ
)
RETURNS TABLE(
    selection_outcome_name TEXT,
    observed_database_now TIMESTAMPTZ,
    observed_owner_expires_at TIMESTAMPTZ,
    selected_drain_intent_id TEXT,
    selected_source_intent_revision BIGINT,
    selected_source_state_digest TEXT,
    selected_source_state_bytes BYTEA,
    selected_product_operation_id TEXT,
    selected_product_mutation_digest TEXT,
    selected_tenant_id TEXT,
    selected_installation_id TEXT,
    selected_deployment_id TEXT,
    selected_expected_revision BIGINT,
    selected_product_mutation_request_bytes BYTEA,
    selected_drain_intent_request_bytes BYTEA,
    selected_drain_intent_digest TEXT,
    selected_slot_guild_id TEXT,
    selected_slot_ruleset_key TEXT,
    selected_target_version BIGINT,
    selected_target_content_hash TEXT,
    selected_target_binding_revision BIGINT,
    selected_target_binding_fingerprint TEXT,
    predecessor_claim_terminal_digest TEXT,
    predecessor_gateway_shard_id TEXT,
    predecessor_process_instance_id TEXT,
    predecessor_lease_epoch BIGINT,
    predecessor_runtime_build_revision TEXT,
    predecessor_owner_revision BIGINT,
    predecessor_controller_id TEXT,
    predecessor_controller_fencing_token BIGINT,
    predecessor_claim_epoch BIGINT,
    predecessor_claim_revision BIGINT,
    predecessor_claim_expires_at TIMESTAMPTZ,
    predecessor_seal_process_instance_id TEXT,
    predecessor_seal_generation BIGINT,
    predecessor_seal_observation_sequence BIGINT
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
    candidate_row public.runtime_drain_intents_v2%ROWTYPE;
    product_row public.runtime_product_operations_v2%ROWTYPE;
    deployment_row public.runtime_deployments%ROWTYPE;
    predecessor_action_row public.runtime_startup_recovery_actions_v2%ROWTYPE;
    candidate_count BIGINT;
    candidate_id TEXT;
    state_value JSONB;
    state_kind TEXT;
    controller_id TEXT;
    controller_recovery_id TEXT;
    controller_action_revision BIGINT;
    claim_expiry_numeric NUMERIC;
    observed_database_now_numeric NUMERIC;
BEGIN
    PERFORM pg_catalog.set_config('TimeZone', 'UTC', TRUE);
    IF pg_catalog.current_setting('transaction_isolation')
            <> 'serializable'
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
            MESSAGE = 'runtime_pending_drain_v3_selection_input_invalid';
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock_shared(
        pg_catalog.hashtextextended(
            'starring-runtime-writer-fence-v1',
            0
        )
    );
    PERFORM pg_catalog.pg_advisory_xact_lock_shared(
        pg_catalog.hashtextextended(
            pg_catalog.concat(
                'starring-runtime-gateway-owner-v1:',
                expected_gateway_shard_id
            ),
            0
        )
    );

    SELECT owner.*
    INTO owner_row
    FROM public.runtime_gateway_owners AS owner
    WHERE owner.gateway_shard_id = expected_gateway_shard_id;
    observed_database_now := pg_catalog.clock_timestamp();
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
        OR owner_row.expires_at <= observed_database_now
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_pending_drain_v3_selection_owner_lost';
    END IF;
    IF observed_database_now < requested_minimum_database_now THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_pending_drain_v3_selection_clock_regressed';
    END IF;

    SELECT candidate.*
    INTO STRICT candidate_count, candidate_id
    FROM starring_runtime_private_v2.starring_runtime_pending_drain_candidate_v2()
        AS candidate;
    IF candidate_count = 0 THEN
        selection_outcome_name := 'no_candidate';
        observed_owner_expires_at := owner_row.expires_at;
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT drain.*
    INTO candidate_row
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE drain.drain_intent_id = candidate_id;
    IF NOT FOUND
        OR candidate_row.intent_state <> 'pending'
        OR candidate_row.intent_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
            candidate_row
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_pending_drain_v3_selection_changed';
    END IF;

    SELECT product.*
    INTO product_row
    FROM public.runtime_product_operations_v2 AS product
    WHERE product.product_operation_id =
            candidate_row.product_operation_id
        AND product.product_mutation_digest =
            candidate_row.product_mutation_digest
        AND product.tenant_id = candidate_row.tenant_id
        AND product.installation_id =
            candidate_row.installation_id
        AND product.deployment_id =
            candidate_row.deployment_id
        AND product.expected_revision =
            candidate_row.expected_revision
        AND product.expected_target_guild_id =
            candidate_row.slot_guild_id
        AND product.expected_target_ruleset_key =
            candidate_row.slot_ruleset_key;
    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = candidate_row.tenant_id
        AND deployment.installation_id =
            candidate_row.installation_id
        AND deployment.deployment_id =
            candidate_row.deployment_id;
    IF product_row.product_operation_id IS NULL
        OR deployment_row.deployment_id IS NULL
        OR deployment_row.revision
            IS DISTINCT FROM product_row.expected_revision
        OR deployment_row.guild_id
            IS DISTINCT FROM product_row.expected_target_guild_id
        OR deployment_row.ruleset_key
            IS DISTINCT FROM
                product_row.expected_target_ruleset_key
        OR deployment_row.target_version
            IS DISTINCT FROM product_row.expected_target_version
        OR deployment_row.target_content_hash
            IS DISTINCT FROM
                product_row.expected_target_content_hash
        OR deployment_row.binding_revision
            IS DISTINCT FROM
                product_row.expected_target_binding_revision
        OR deployment_row.binding_fingerprint
            IS DISTINCT FROM
                product_row.expected_target_binding_fingerprint
        OR deployment_row.snapshot #>> '{target,guild_id}'
            IS DISTINCT FROM
                product_row.expected_target_guild_id
        OR deployment_row.snapshot #>> '{target,ruleset_key}'
            IS DISTINCT FROM
                product_row.expected_target_ruleset_key
        OR deployment_row.snapshot #>> '{target,version}'
            IS DISTINCT FROM
                product_row.expected_target_version::TEXT
        OR deployment_row.snapshot #>> '{target,content_hash}'
            IS DISTINCT FROM
                product_row.expected_target_content_hash
        OR deployment_row.snapshot #>> '{target,binding_revision}'
            IS DISTINCT FROM
                product_row.expected_target_binding_revision::TEXT
        OR deployment_row.snapshot #>> '{target,binding_fingerprint}'
            IS DISTINCT FROM
                product_row.expected_target_binding_fingerprint
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_pending_drain_v3_selection_target_invalid';
    END IF;

    state_value := pg_catalog.convert_from(
        candidate_row.canonical_state_bytes,
        'UTF8'
    )::JSONB;
    state_kind := state_value #>> '{state,kind}';
    IF state_kind = 'pending_unclaimed' THEN
        selection_outcome_name := 'unclaimed';
    ELSIF state_kind = 'pending_claimed' THEN
        controller_id :=
            state_value #>> '{state,claim,controller_id}';
        IF controller_id
                !~ '^recovery:[0-9a-f]{32}:[1-9][0-9]{0,18}$'
            OR (
                pg_catalog.split_part(
                    controller_id,
                    ':',
                    3
                )::NUMERIC
            ) > 9223372036854775807
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_pending_drain_v3_predecessor_invalid';
        END IF;
        controller_recovery_id :=
            pg_catalog.split_part(controller_id, ':', 2);
        controller_action_revision :=
            pg_catalog.split_part(controller_id, ':', 3)::BIGINT;
        SELECT action.*
        INTO predecessor_action_row
        FROM public.runtime_startup_recovery_actions_v2 AS action
        WHERE action.recovery_id = controller_recovery_id
            AND action.action_authority_revision =
                controller_action_revision;
        IF NOT FOUND
            OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_predecessor_exact_v3(
                candidate_row,
                predecessor_action_row
            )
            OR state_value
                    #>> '{state,claim,gateway_owner_lease_id,gateway_shard_id}'
                IS DISTINCT FROM expected_gateway_shard_id
            OR state_value
                    #>> '{state,claim,gateway_owner_lease_id,process_instance_id}'
                IS NOT DISTINCT FROM
                    expected_owner_process_instance_id
            OR (
                state_value
                    #>> '{state,claim,gateway_owner_lease_id,lease_epoch}'
            )::BIGINT >= expected_owner_lease_epoch
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX003',
                MESSAGE = 'runtime_pending_drain_v3_predecessor_authority_invalid';
        END IF;
        claim_expiry_numeric := (
            state_value
                #>> '{state,claim,claim_expires_at_unix_microseconds}'
        )::NUMERIC;
        observed_database_now_numeric :=
            EXTRACT(EPOCH FROM observed_database_now) * 1000000;
        IF observed_database_now_numeric <>
                pg_catalog.trunc(observed_database_now_numeric)
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_pending_drain_v3_selection_time_invalid';
        END IF;
        selection_outcome_name := CASE
            WHEN observed_database_now_numeric >= claim_expiry_numeric
            THEN 'expired_previous_owner'
            ELSE 'fresh_previous_owner'
        END;
        predecessor_claim_terminal_digest :=
            predecessor_action_row.terminal_digest;
        predecessor_gateway_shard_id :=
            state_value
                #>> '{state,claim,gateway_owner_lease_id,gateway_shard_id}';
        predecessor_process_instance_id :=
            state_value
                #>> '{state,claim,gateway_owner_lease_id,process_instance_id}';
        predecessor_lease_epoch := (
            state_value
                #>> '{state,claim,gateway_owner_lease_id,lease_epoch}'
        )::BIGINT;
        predecessor_runtime_build_revision :=
            state_value
                #>> '{state,claim,gateway_owner_lease_id,expected_build_revision}';
        predecessor_owner_revision := (
            state_value #>> '{state,claim,observed_owner_revision}'
        )::BIGINT;
        predecessor_controller_id := controller_id;
        predecessor_controller_fencing_token := (
            state_value #>> '{state,claim,controller_fencing_token}'
        )::BIGINT;
        predecessor_claim_epoch := (
            state_value #>> '{state,claim,claim_epoch}'
        )::BIGINT;
        predecessor_claim_revision := (
            state_value #>> '{state,claim,claim_revision}'
        )::BIGINT;
        predecessor_claim_expires_at :=
            predecessor_action_row.owner_expires_at;
        predecessor_seal_process_instance_id :=
            state_value
                #>> '{state,claim,progress,seal,process_instance_id}';
        predecessor_seal_generation := (
            state_value
                #>> '{state,claim,progress,seal,seal_generation}'
        )::BIGINT;
        predecessor_seal_observation_sequence := (
            state_value
                #>> '{state,claim,progress,seal,registry_observation_sequence}'
        )::BIGINT;
    ELSE
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_pending_drain_v3_candidate_unsupported';
    END IF;

    selected_drain_intent_id := candidate_row.drain_intent_id;
    selected_source_intent_revision := candidate_row.intent_revision;
    selected_source_state_digest :=
        candidate_row.canonical_state_digest;
    selected_source_state_bytes :=
        candidate_row.canonical_state_bytes;
    selected_product_operation_id :=
        candidate_row.product_operation_id;
    selected_product_mutation_digest :=
        candidate_row.product_mutation_digest;
    selected_tenant_id := candidate_row.tenant_id;
    selected_installation_id := candidate_row.installation_id;
    selected_deployment_id := candidate_row.deployment_id;
    selected_expected_revision := candidate_row.expected_revision;
    selected_product_mutation_request_bytes :=
        product_row.product_mutation_request_bytes;
    selected_drain_intent_request_bytes :=
        candidate_row.drain_intent_request_bytes;
    selected_drain_intent_digest :=
        candidate_row.drain_intent_digest;
    selected_slot_guild_id := candidate_row.slot_guild_id;
    selected_slot_ruleset_key := candidate_row.slot_ruleset_key;
    selected_target_version := product_row.expected_target_version;
    selected_target_content_hash :=
        product_row.expected_target_content_hash;
    selected_target_binding_revision :=
        product_row.expected_target_binding_revision;
    selected_target_binding_fingerprint :=
        product_row.expected_target_binding_fingerprint;
    observed_owner_expires_at := owner_row.expires_at;
    RETURN NEXT;
END;
$function$;

REVOKE ALL ON FUNCTION
    starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(
        BYTEA,
        SMALLINT,
        SMALLINT
    ),
    starring_runtime_private_v2.starring_runtime_pending_drain_succession_projection_v3(
        BYTEA,
        BYTEA,
        BYTEA,
        BYTEA
    ),
    starring_runtime_private_v2.starring_runtime_pending_drain_predecessor_exact_v3(
        public.runtime_drain_intents_v2,
        public.runtime_startup_recovery_actions_v2
    ),
    starring_runtime_private_v2.starring_runtime_pending_drain_successor_exact_v3(
        public.runtime_drain_intents_v2,
        public.runtime_drain_intents_v2,
        public.runtime_deployments,
        public.runtime_deployments,
        BYTEA,
        TEXT
    )
FROM PUBLIC;

REVOKE ALL ON FUNCTION
    public.starring_runtime_startup_recovery_select_pending_drain_v3(
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        BIGINT,
        TIMESTAMPTZ,
        TIMESTAMPTZ
    ),
    public.starring_runtime_startup_recovery_pending_drain_succession_v3(
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
        BIGINT,
        TEXT,
        BIGINT,
        TEXT,
        TEXT,
        BOOLEAN,
        BIGINT,
        BIGINT,
        BYTEA,
        BIGINT,
        BIGINT,
        BIGINT,
        BIGINT,
        BIGINT,
        BIGINT,
        BIGINT,
        BIGINT,
        BIGINT,
        BIGINT,
        BIGINT,
        BIGINT,
        BOOLEAN
    )
FROM PUBLIC;

DO $grant_executor$
DECLARE
    common_owner OID;
    executor_role OID;
    identity TEXT;
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
        FOREACH identity IN ARRAY ARRAY[
            'public.starring_runtime_startup_recovery_select_pending_drain_v3(text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)',
            'public.starring_runtime_startup_recovery_pending_drain_succession_v3(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean)'
        ]
        LOOP
            EXECUTE pg_catalog.format(
                'GRANT EXECUTE ON FUNCTION %s TO %s',
                identity,
                executor_role::REGROLE
            );
        END LOOP;
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
        '            ''starring_runtime_private_v2.starring_runtime_pending_drain_candidate_v2()''' || E'\n' ||
        '        )';
    next_fragment := previous_fragment;
    FOREACH identity IN ARRAY ARRAY[
        'public.starring_runtime_startup_recovery_select_pending_drain_v3(text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)',
        'public.starring_runtime_startup_recovery_pending_drain_succession_v3(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean)',
        'starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(bytea,smallint,smallint)',
        'starring_runtime_private_v2.starring_runtime_pending_drain_succession_projection_v3(bytea,bytea,bytea,bytea)',
        'starring_runtime_private_v2.starring_runtime_pending_drain_predecessor_exact_v3(public.runtime_drain_intents_v2,public.runtime_startup_recovery_actions_v2)',
        'starring_runtime_private_v2.starring_runtime_pending_drain_successor_exact_v3(public.runtime_drain_intents_v2,public.runtime_drain_intents_v2,public.runtime_deployments,public.runtime_deployments,bytea,text)'
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
            MESSAGE = 'runtime_pending_drain_succession_manifest_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        'RETURN observed_count = 828' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''a10c4cc166d3fa07adc4bb800e47f3c0cfb1747b8f6a49fd8e1144d1a11865a3'';';
    next_fragment :=
        'RETURN observed_count = 834' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''90d1ab7064fa288e01b09e81815265d82409ceac50267412ff952f63a6c285a3'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_succession_manifest_expectation_patch_drift';
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
    anchor_fragment TEXT;
    ending_fragment TEXT;
    anchor_position INTEGER;
    ending_position INTEGER;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_database_readiness_v1()'
    );

    anchor_fragment :=
        '                ''public.starring_runtime_startup_recovery_execute_pending_drain_v2(text,bigint,bigint,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean,text)'',';
    ending_fragment :=
        '            )' || E'\n' ||
        '    ) AS expected(';
    anchor_position := pg_catalog.strpos(definition, anchor_fragment);
    IF anchor_position = 0
        OR pg_catalog.strpos(
            pg_catalog.substr(
                definition,
                anchor_position + pg_catalog.length(anchor_fragment)
            ),
            anchor_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_succession_readiness_contract_anchor_drift';
    END IF;
    ending_position := pg_catalog.strpos(
        pg_catalog.substr(definition, anchor_position),
        ending_fragment
    );
    IF ending_position = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_succession_readiness_contract_end_drift';
    END IF;
    ending_position := anchor_position + ending_position - 1;
    next_fragment :=
        ',' || E'\n' ||
        '            (' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_select_pending_drain_v3(text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)'',' || E'\n' ||
        '                ''expected_gateway_shard_id text, expected_owner_process_instance_id text, expected_owner_lease_epoch bigint, expected_owner_runtime_build_revision text, expected_owner_revision bigint, expected_owner_expires_at timestamp with time zone, requested_minimum_database_now timestamp with time zone''::TEXT,' || E'\n' ||
        '                ''TABLE(selection_outcome_name text, observed_database_now timestamp with time zone, observed_owner_expires_at timestamp with time zone, selected_drain_intent_id text, selected_source_intent_revision bigint, selected_source_state_digest text, selected_source_state_bytes bytea, selected_product_operation_id text, selected_product_mutation_digest text, selected_tenant_id text, selected_installation_id text, selected_deployment_id text, selected_expected_revision bigint, selected_product_mutation_request_bytes bytea, selected_drain_intent_request_bytes bytea, selected_drain_intent_digest text, selected_slot_guild_id text, selected_slot_ruleset_key text, selected_target_version bigint, selected_target_content_hash text, selected_target_binding_revision bigint, selected_target_binding_fingerprint text, predecessor_claim_terminal_digest text, predecessor_gateway_shard_id text, predecessor_process_instance_id text, predecessor_lease_epoch bigint, predecessor_runtime_build_revision text, predecessor_owner_revision bigint, predecessor_controller_id text, predecessor_controller_fencing_token bigint, predecessor_claim_epoch bigint, predecessor_claim_revision bigint, predecessor_claim_expires_at timestamp with time zone, predecessor_seal_process_instance_id text, predecessor_seal_generation bigint, predecessor_seal_observation_sequence bigint)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            ),' || E'\n' ||
        '            (' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_pending_drain_succession_v3(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean)'',' || E'\n' ||
        '                ''requested_recovery_id text, requested_originating_emergency_generation bigint, requested_coordinator_generation bigint, requested_action_authority_revision bigint, requested_selection_authority_revision bigint, expected_gateway_shard_id text, expected_owner_process_instance_id text, expected_owner_lease_epoch bigint, expected_owner_runtime_build_revision text, expected_owner_revision bigint, expected_owner_expires_at timestamp with time zone, requested_minimum_database_now timestamp with time zone, paused_process_instance_id text, paused_coordinator_generation bigint, paused_connection_epoch bigint, paused_ready_kind text, paused_admission_revision bigint, paused_transition_sequence bigint, paused_connected_event_sequence bigint, paused_last_resume_sequence bigint, registry_process_instance_id text, registry_observation_sequence bigint, registry_retained_slot_count bigint, registry_retained_empty_tombstone_count bigint, requested_selected_drain_intent_id text, requested_selected_source_intent_revision bigint, requested_selected_source_state_digest text, requested_predecessor_claim_terminal_digest text, requested_pre_slot_present boolean, requested_pre_slot_admission_generation bigint, requested_pre_slot_observation_sequence bigint, requested_seal_key bytea, requested_seal_generation bigint, requested_post_slot_admission_generation bigint, requested_post_slot_observation_sequence bigint, requested_post_global_observation_sequence bigint, requested_post_global_retained_slot_count bigint, requested_post_global_retained_empty_tombstone_count bigint, requested_post_global_staged_route_count bigint, requested_post_global_serving_route_count bigint, requested_post_global_draining_route_count bigint, requested_post_global_sealed_slot_count bigint, requested_post_global_active_interaction_count bigint, requested_post_global_failed_closed_slot_count bigint, requested_post_global_registry_failed_closed boolean''::TEXT,' || E'\n' ||
        '                ''TABLE(journal_outcome_name text, terminal_outcome_name text, recovery_id text, originating_emergency_generation bigint, coordinator_generation bigint, action_authority_revision bigint, selection_authority_revision bigint, recovery_class text, observed_gateway_shard_id text, observed_process_instance_id text, observed_lease_epoch bigint, observed_runtime_build_revision text, observed_owner_revision bigint, database_now timestamp with time zone, observed_owner_expires_at timestamp with time zone, minimum_database_now timestamp with time zone, recorded_at timestamp with time zone, terminal_projection_bytes bytea, terminal_digest text)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            )';
    definition :=
        pg_catalog.substr(
            definition,
            1,
            ending_position + pg_catalog.length('            )') - 1
        ) ||
        next_fragment ||
        pg_catalog.substr(
            definition,
            ending_position + pg_catalog.length('            )')
        );

    previous_fragment :=
        '            (''starring_runtime_private_v2.starring_runtime_pending_drain_candidate_v2()''),';
    next_fragment := previous_fragment || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(bytea,smallint,smallint)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_pending_drain_succession_projection_v3(bytea,bytea,bytea,bytea)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_pending_drain_predecessor_exact_v3(public.runtime_drain_intents_v2,public.runtime_startup_recovery_actions_v2)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_pending_drain_successor_exact_v3(public.runtime_drain_intents_v2,public.runtime_drain_intents_v2,public.runtime_deployments,public.runtime_deployments,bytea,text)''),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_succession_readiness_private_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '''9de93ea5d565254c47533c7af43959aa873014bee385a2af775fafdcbf8118b9''::TEXT';
    next_fragment :=
        '''8f62326b250fba74273b2dbbf33066ef7f1353e9a6f3f464c059b1678bb714d4''::TEXT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_succession_readiness_manifest_digest_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_execute_pending_drain_v2(text,bigint,bigint,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean,text)''' || E'\n' ||
        '            )' || E'\n' ||
        '        )';
    next_fragment :=
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_execute_pending_drain_v2(text,bigint,bigint,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean,text)''' || E'\n' ||
        '            ),' || E'\n' ||
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_select_pending_drain_v3(text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)''' || E'\n' ||
        '            ),' || E'\n' ||
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_pending_drain_succession_v3(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean)''' || E'\n' ||
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
            MESSAGE = 'runtime_pending_drain_succession_readiness_allowlist_patch_drift';
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
    executor_role_is_quarantined BOOLEAN;
    executor_membership_count BIGINT;
    invalid_function_count BIGINT;
    invalid_acl_count BIGINT;
    actual_acl_count BIGINT;
    expected_acl_count BIGINT;
    direct_relation_privilege_count BIGINT;
    direct_column_privilege_count BIGINT;
    invalid_alias_count BIGINT;
    invalid_digest_count BIGINT;
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
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_runtime_startup_recovery_select_pending_drain_v3(text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)',
                'v'::"char",
                'u'::"char",
                TRUE,
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_startup_recovery_pending_drain_succession_v3(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean)',
                'v'::"char",
                'u'::"char",
                TRUE,
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(bytea,smallint,smallint)',
                'i'::"char",
                's'::"char",
                FALSE,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'starring_runtime_private_v2.starring_runtime_pending_drain_succession_projection_v3(bytea,bytea,bytea,bytea)',
                'i'::"char",
                's'::"char",
                FALSE,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'starring_runtime_private_v2.starring_runtime_pending_drain_predecessor_exact_v3(public.runtime_drain_intents_v2,public.runtime_startup_recovery_actions_v2)',
                'i'::"char",
                's'::"char",
                FALSE,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'starring_runtime_private_v2.starring_runtime_pending_drain_successor_exact_v3(public.runtime_drain_intents_v2,public.runtime_drain_intents_v2,public.runtime_deployments,public.runtime_deployments,bytea,text)',
                'i'::"char",
                's'::"char",
                FALSE,
                TRUE,
                FALSE,
                0::REAL
            )
    ) AS expected(
        identity,
        volatility,
        parallel_kind,
        security_definer,
        strict_kind,
        returns_set,
        rows_estimate
    )
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid =
            pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> expected.volatility
        OR function_row.proparallel <> expected.parallel_kind
        OR function_row.prosecdef <> expected.security_definer
        OR function_row.proisstrict <> expected.strict_kind
        OR function_row.proretset <> expected.returns_set
        OR function_row.prorows <> expected.rows_estimate
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
            ('public.starring_runtime_startup_recovery_select_pending_drain_v3(text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)', TRUE),
            ('public.starring_runtime_startup_recovery_pending_drain_succession_v3(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean)', TRUE),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(bytea,smallint,smallint)', FALSE),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_succession_projection_v3(bytea,bytea,bytea,bytea)', FALSE),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_predecessor_exact_v3(public.runtime_drain_intents_v2,public.runtime_startup_recovery_actions_v2)', FALSE),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_successor_exact_v3(public.runtime_drain_intents_v2,public.runtime_drain_intents_v2,public.runtime_deployments,public.runtime_deployments,bytea,text)', FALSE)
    ) AS expected(identity, executor_allowed)
    INNER JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid =
            pg_catalog.to_regprocedure(expected.identity)
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege;
    expected_acl_count := 6 + CASE
        WHEN executor_role IS NULL THEN 0
        ELSE 2
    END;

    SELECT pg_catalog.count(*)
    INTO direct_relation_privilege_count
    FROM pg_catalog.pg_class AS relation
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        relation.relacl,
        ARRAY[]::ACLITEM[]
    )) AS privilege
    WHERE executor_role IS NOT NULL
        AND privilege.grantee = executor_role;

    SELECT pg_catalog.count(*)
    INTO direct_column_privilege_count
    FROM pg_catalog.pg_attribute AS attribute
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        attribute.attacl,
        ARRAY[]::ACLITEM[]
    )) AS privilege
    WHERE executor_role IS NOT NULL
        AND privilege.grantee = executor_role
        AND attribute.attnum > 0
        AND NOT attribute.attisdropped;

    SELECT pg_catalog.count(*)
    INTO invalid_alias_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE (
        (
            namespace.nspname = 'public'
            AND function_row.proname IN (
                'starring_runtime_startup_recovery_select_pending_drain_v3',
                'starring_runtime_startup_recovery_pending_drain_succession_v3'
            )
        )
        OR (
            namespace.nspname = 'starring_runtime_private_v2'
            AND function_row.proname IN (
                'starring_runtime_pending_drain_projection_frame_v3',
                'starring_runtime_pending_drain_succession_projection_v3',
                'starring_runtime_pending_drain_predecessor_exact_v3',
                'starring_runtime_pending_drain_successor_exact_v3'
            )
        )
    )
    AND function_row.oid <> ALL (ARRAY[
        pg_catalog.to_regprocedure(
            'public.starring_runtime_startup_recovery_select_pending_drain_v3(text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)'
        ),
        pg_catalog.to_regprocedure(
            'public.starring_runtime_startup_recovery_pending_drain_succession_v3(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean)'
        ),
        pg_catalog.to_regprocedure(
            'starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(bytea,smallint,smallint)'
        ),
        pg_catalog.to_regprocedure(
            'starring_runtime_private_v2.starring_runtime_pending_drain_succession_projection_v3(bytea,bytea,bytea,bytea)'
        ),
        pg_catalog.to_regprocedure(
            'starring_runtime_private_v2.starring_runtime_pending_drain_predecessor_exact_v3(public.runtime_drain_intents_v2,public.runtime_startup_recovery_actions_v2)'
        ),
        pg_catalog.to_regprocedure(
            'starring_runtime_private_v2.starring_runtime_pending_drain_successor_exact_v3(public.runtime_drain_intents_v2,public.runtime_drain_intents_v2,public.runtime_deployments,public.runtime_deployments,bytea,text)'
        )
    ]);

    SELECT pg_catalog.count(*)
    INTO invalid_digest_count
    FROM (
        VALUES
            ('public.starring_runtime_execution_schema_manifest_v1()', '8f62326b250fba74273b2dbbf33066ef7f1353e9a6f3f464c059b1678bb714d4'),
            ('public.starring_runtime_execution_database_readiness_v1()', 'd73ca3b8f02623884ccf1e77390395a1daeee1d5c3d12274f865740d0798fa06'),
            ('public.starring_runtime_exact_target_schema_manifest_v1()', 'bea5a930a40537f9f06f19a350d1fdba3bf21b222844eb0f442fb506d91a1ebb'),
            ('public.starring_runtime_exact_target_database_readiness_v1()', '5eba72a786aebaa8afdc226d661b45132afc5aa053fab7be6a3b9737fdab0e8c'),
            ('public.starring_runtime_serving_schema_manifest_v1()', 'c679ef7c0722416b514324936a95884d17242e6b67cdb130987e4d4f03a43758'),
            ('public.starring_runtime_serving_database_readiness_v1()', '80e9f1da2a7b48610e95e2540db4c77a3daed2d53b3a2ec18de37c0767ac5380'),
            ('public.starring_runtime_startup_recovery_select_pending_drain_v3(text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)', '67ce81c4a3dcb38936eb52872f5a60cddd16936d5ef7eb7599141a3e86f23975'),
            ('public.starring_runtime_startup_recovery_pending_drain_succession_v3(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean)', 'c6c3642cad780abea816e0f05a183c7fb9af7376e7379a077b4b2343012cae23'),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(bytea,smallint,smallint)', 'cd7223978da9cde002eb693fd276ad79849991c66df25697a55c61d9453d28e2'),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_succession_projection_v3(bytea,bytea,bytea,bytea)', '1196a9589f25a699946dcec4f937516f72b1b31549ca28ebfaeaa001ce4ce189'),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_predecessor_exact_v3(public.runtime_drain_intents_v2,public.runtime_startup_recovery_actions_v2)', '05f495a38a16a2ab0ce057f6e1367fb8510ea95795e14f986b3b806b7b266c8e'),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_successor_exact_v3(public.runtime_drain_intents_v2,public.runtime_drain_intents_v2,public.runtime_deployments,public.runtime_deployments,bytea,text)', '2d384bb36f84ae6e4ae64ccc9ef435692ee7e7013bb81de20f572be7bf41c9ca')
    ) AS expected(identity, digest)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid =
            pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(function_row.oid),
                'UTF8'
            )),
            'hex'
        ) IS DISTINCT FROM expected.digest;

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR NOT executor_role_is_quarantined
        OR executor_membership_count <> 0
        OR invalid_function_count <> 0
        OR invalid_acl_count <> 0
        OR actual_acl_count <> expected_acl_count
        OR direct_relation_privilege_count <> 0
        OR direct_column_privilege_count <> 0
        OR invalid_alias_count <> 0
        OR invalid_digest_count <> 0
        OR NOT public.starring_runtime_exact_target_schema_manifest_v1()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_succession_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
