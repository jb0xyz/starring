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
    public.runtime_certification_operation_terminals_v2,
    public.runtime_attestations,
    public.runtime_startup_recovery_actions_v2,
    public.runtime_drain_intents_v2
IN ACCESS EXCLUSIVE MODE;

DO $preflight$
DECLARE
    common_owner OID;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
        OR pg_catalog.to_regprocedure(
            'public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)'
        ) IS NULL
        OR pg_catalog.to_regprocedure(
            'public.starring_runtime_startup_recovery_execute_reserved_awaiting_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)'
        ) IS NULL
        OR pg_catalog.to_regprocedure(
            'starring_runtime_private_v2.starring_runtime_cert_awaiting_reset_exact_v2(public.runtime_deployments,public.runtime_deployments,timestamp with time zone)'
        ) IS NULL
        OR pg_catalog.to_regprocedure(
            'starring_runtime_private_v2.starring_runtime_unreserved_awaiting_reset_exact_v2(public.runtime_deployments,public.runtime_deployments,timestamp with time zone)'
        ) IS NOT NULL
        OR pg_catalog.to_regprocedure(
            'starring_runtime_private_v2.starring_runtime_startup_unreserved_projection_exact_v2(bytea,timestamp with time zone)'
        ) IS NOT NULL
        OR pg_catalog.to_regprocedure(
            'starring_runtime_private_v2.starring_runtime_startup_unreserved_execute_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)'
        ) IS NOT NULL
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_unreserved_awaiting_preflight_drift';
    END IF;
END;
$preflight$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_unreserved_awaiting_reset_exact_v2(
    previous_deployment public.runtime_deployments,
    proposed_deployment public.runtime_deployments,
    expected_mutation_clock TIMESTAMPTZ
)
RETURNS BOOLEAN
LANGUAGE sql
STABLE
STRICT
PARALLEL UNSAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
SELECT COALESCE((
    pg_catalog.isfinite(expected_mutation_clock)
    AND previous_deployment.phase IS NOT DISTINCT FROM
        'awaiting_gateway_ready'
    AND previous_deployment.revision
        BETWEEN 1 AND 9223372036854775806
    AND previous_deployment.snapshot -> 'phase'
        IS NOT DISTINCT FROM
        '{"phase":"awaiting_gateway_ready"}'::JSONB
    AND previous_deployment.snapshot -> 'revision'
        IS NOT DISTINCT FROM
        pg_catalog.to_jsonb(previous_deployment.revision)
    AND previous_deployment.controller_id IS NOT NULL
    AND previous_deployment.controller_fencing_token IS NOT NULL
    AND previous_deployment.controller_acquired_at IS NOT NULL
    AND previous_deployment.controller_lease_expires_at IS NOT NULL
    AND previous_deployment.last_controller_id
        IS NOT DISTINCT FROM previous_deployment.controller_id
    AND previous_deployment.last_fencing_token
        IS NOT DISTINCT FROM
        previous_deployment.controller_fencing_token
    AND pg_catalog.jsonb_typeof(
        previous_deployment.snapshot -> 'panel_certificate'
    ) IS NOT DISTINCT FROM 'object'
    AND previous_deployment.snapshot -> 'gateway_ready'
        IS NOT DISTINCT FROM 'null'::JSONB
    AND previous_deployment.snapshot -> 'live'
        IS NOT DISTINCT FROM 'null'::JSONB
    AND previous_deployment.live_attestation_id IS NULL
    AND previous_deployment.live_at IS NULL
    AND proposed_deployment.revision IS NOT DISTINCT FROM
        previous_deployment.revision + 1
    AND proposed_deployment.phase IS NOT DISTINCT FROM
        'reconciling_panels'
    AND proposed_deployment.convergence_attempt_no
        IS NOT DISTINCT FROM
        previous_deployment.convergence_attempt_no
    AND proposed_deployment.snapshot -> 'phase'
        IS NOT DISTINCT FROM
        '{"phase":"reconciling_panels"}'::JSONB
    AND proposed_deployment.snapshot -> 'revision'
        IS NOT DISTINCT FROM
        pg_catalog.to_jsonb(proposed_deployment.revision)
    AND proposed_deployment.snapshot -> 'controller_lease'
        IS NOT DISTINCT FROM 'null'::JSONB
    AND proposed_deployment.snapshot -> 'panel_certificate'
        IS NOT DISTINCT FROM 'null'::JSONB
    AND proposed_deployment.snapshot -> 'gateway_ready'
        IS NOT DISTINCT FROM 'null'::JSONB
    AND proposed_deployment.snapshot -> 'live'
        IS NOT DISTINCT FROM 'null'::JSONB
    AND proposed_deployment.controller_id IS NULL
    AND proposed_deployment.controller_fencing_token IS NULL
    AND proposed_deployment.controller_acquired_at IS NULL
    AND proposed_deployment.controller_lease_expires_at IS NULL
    AND proposed_deployment.live_attestation_id IS NULL
    AND proposed_deployment.live_at IS NULL
    AND proposed_deployment.updated_at IS NOT DISTINCT FROM GREATEST(
        expected_mutation_clock,
        previous_deployment.updated_at + INTERVAL '1 microsecond'
    )
    AND pg_catalog.to_jsonb(proposed_deployment) - ARRAY[
        'snapshot',
        'revision',
        'phase',
        'controller_id',
        'controller_fencing_token',
        'controller_acquired_at',
        'controller_lease_expires_at',
        'updated_at'
    ]::TEXT[] IS NOT DISTINCT FROM
        pg_catalog.to_jsonb(previous_deployment) - ARRAY[
            'snapshot',
            'revision',
            'phase',
            'controller_id',
            'controller_fencing_token',
            'controller_acquired_at',
            'controller_lease_expires_at',
            'updated_at'
        ]::TEXT[]
    AND proposed_deployment.snapshot - ARRAY[
        'revision',
        'phase',
        'controller_lease',
        'panel_certificate',
        'gateway_ready',
        'live'
    ]::TEXT[] IS NOT DISTINCT FROM
        previous_deployment.snapshot - ARRAY[
            'revision',
            'phase',
            'controller_lease',
            'panel_certificate',
            'gateway_ready',
            'live'
        ]::TEXT[]
    AND NOT EXISTS (
        SELECT 1
        FROM public.runtime_certification_operations_v2 AS operation
        WHERE operation.tenant_id = previous_deployment.tenant_id
            AND operation.installation_id =
                previous_deployment.installation_id
            AND operation.deployment_id =
                previous_deployment.deployment_id
            AND operation.deployment_revision =
                previous_deployment.revision
            AND operation.convergence_attempt_no =
                previous_deployment.convergence_attempt_no
    )
), FALSE);
$function$;

REVOKE ALL ON FUNCTION
    starring_runtime_private_v2.starring_runtime_unreserved_awaiting_reset_exact_v2(
        public.runtime_deployments,
        public.runtime_deployments,
        TIMESTAMPTZ
    )
FROM PUBLIC;

DO $patch_deployment_validators$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.validate_runtime_deployment_projection()'
    );
    previous_fragment :=
        '        certification_awaiting_reset :=' || E'\n' ||
        '            starring_runtime_private_v2.starring_runtime_cert_awaiting_reset_exact_v2(' || E'\n' ||
        '                OLD,' || E'\n' ||
        '                NEW,' || E'\n' ||
        '                mutation_clock' || E'\n' ||
        '            ) IS TRUE;';
    next_fragment :=
        '        certification_awaiting_reset := (' || E'\n' ||
        '            starring_runtime_private_v2.starring_runtime_cert_awaiting_reset_exact_v2(' || E'\n' ||
        '                OLD,' || E'\n' ||
        '                NEW,' || E'\n' ||
        '                mutation_clock' || E'\n' ||
        '            ) IS TRUE' || E'\n' ||
        '            OR starring_runtime_private_v2.starring_runtime_unreserved_awaiting_reset_exact_v2(' || E'\n' ||
        '                OLD,' || E'\n' ||
        '                NEW,' || E'\n' ||
        '                mutation_clock' || E'\n' ||
        '            ) IS TRUE' || E'\n' ||
        '        );';
    IF definition IS NULL
        OR pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_unreserved_deployment_validator_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.validate_runtime_convergence_attempt_projection()'
    );
    IF definition IS NULL
        OR pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_unreserved_convergence_validator_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$patch_deployment_validators$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_startup_unreserved_projection_exact_v2(
    projection_bytes BYTEA,
    expected_recorded_at TIMESTAMPTZ
)
RETURNS BOOLEAN
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    domain_bytes BYTEA;
    expected_prefix BYTEA;
    cursor_position BIGINT;
    frame_length BIGINT;
    frame_index INTEGER;
    frame_bytes BYTEA;
    source_deployment_frame BYTEA;
    successor_deployment_frame BYTEA;
    source_slot_frame BYTEA;
    successor_slot_frame BYTEA;
    terminal_at_bytes BYTEA;
    source_deployment_json JSONB;
    successor_deployment_json JSONB;
    source_slot_json JSONB;
    successor_slot_json JSONB;
    source_deployment public.runtime_deployments%ROWTYPE;
    successor_deployment public.runtime_deployments%ROWTYPE;
    current_deployment public.runtime_deployments%ROWTYPE;
    source_slot public.runtime_slot_writer_fences_v2%ROWTYPE;
    successor_slot public.runtime_slot_writer_fences_v2%ROWTYPE;
BEGIN
    IF NOT pg_catalog.isfinite(expected_recorded_at)
        OR pg_catalog.octet_length(projection_bytes)
            NOT BETWEEN 1 AND 1048576
    THEN
        RETURN FALSE;
    END IF;

    domain_bytes := pg_catalog.convert_to(
        'starring.runtime.startup_recovery.reserved_awaiting_certification.terminal.v2',
        'UTF8'
    );
    expected_prefix :=
        pg_catalog.int8send(
            pg_catalog.octet_length(domain_bytes)::BIGINT
        )
        || domain_bytes
        || pg_catalog.int2send(2::SMALLINT)
        || pg_catalog.int2send(2::SMALLINT);
    IF pg_catalog.substring(
            projection_bytes,
            1,
            pg_catalog.octet_length(expected_prefix)
        ) IS DISTINCT FROM expected_prefix
    THEN
        RETURN FALSE;
    END IF;

    cursor_position :=
        pg_catalog.octet_length(expected_prefix)::BIGINT + 1;
    FOR frame_index IN 1..4 LOOP
        IF cursor_position + 7 >
                pg_catalog.octet_length(projection_bytes)
            OR pg_catalog.get_byte(
                projection_bytes,
                (cursor_position - 1)::INTEGER
            ) <> 0
            OR pg_catalog.get_byte(
                projection_bytes,
                cursor_position::INTEGER
            ) <> 0
            OR pg_catalog.get_byte(
                projection_bytes,
                (cursor_position + 1)::INTEGER
            ) <> 0
            OR pg_catalog.get_byte(
                projection_bytes,
                (cursor_position + 2)::INTEGER
            ) <> 0
            OR pg_catalog.get_byte(
                projection_bytes,
                (cursor_position + 3)::INTEGER
            ) <> 0
        THEN
            RETURN FALSE;
        END IF;
        frame_length :=
            pg_catalog.get_byte(
                projection_bytes,
                (cursor_position + 4)::INTEGER
            )::BIGINT * 65536
            + pg_catalog.get_byte(
                projection_bytes,
                (cursor_position + 5)::INTEGER
            )::BIGINT * 256
            + pg_catalog.get_byte(
                projection_bytes,
                (cursor_position + 6)::INTEGER
            )::BIGINT;
        cursor_position := cursor_position + 8;
        IF frame_length NOT BETWEEN 1 AND 1048576
            OR cursor_position + frame_length - 1 >
                pg_catalog.octet_length(projection_bytes)
        THEN
            RETURN FALSE;
        END IF;
        frame_bytes := pg_catalog.substring(
            projection_bytes,
            cursor_position::INTEGER,
            frame_length::INTEGER
        );
        cursor_position := cursor_position + frame_length;
        CASE frame_index
            WHEN 1 THEN
                source_deployment_frame := frame_bytes;
            WHEN 2 THEN
                successor_deployment_frame := frame_bytes;
            WHEN 3 THEN
                source_slot_frame := frame_bytes;
            WHEN 4 THEN
                successor_slot_frame := frame_bytes;
        END CASE;
    END LOOP;
    IF cursor_position + 7
            IS DISTINCT FROM
            pg_catalog.octet_length(projection_bytes)::BIGINT
        OR pg_catalog.get_byte(source_deployment_frame, 0) <> 1
        OR pg_catalog.get_byte(successor_deployment_frame, 0) <> 1
        OR pg_catalog.get_byte(source_slot_frame, 0) <> 1
        OR pg_catalog.get_byte(successor_slot_frame, 0) <> 1
    THEN
        RETURN FALSE;
    END IF;
    terminal_at_bytes := pg_catalog.substring(
        projection_bytes,
        cursor_position::INTEGER,
        8
    );
    cursor_position := cursor_position + 8;

    BEGIN
        source_deployment_json := pg_catalog.convert_from(
            pg_catalog.substring(source_deployment_frame, 2),
            'UTF8'
        )::JSONB;
        successor_deployment_json := pg_catalog.convert_from(
            pg_catalog.substring(successor_deployment_frame, 2),
            'UTF8'
        )::JSONB;
        source_slot_json := pg_catalog.convert_from(
            pg_catalog.substring(source_slot_frame, 2),
            'UTF8'
        )::JSONB;
        successor_slot_json := pg_catalog.convert_from(
            pg_catalog.substring(successor_slot_frame, 2),
            'UTF8'
        )::JSONB;
        SELECT populated.*
        INTO STRICT source_deployment
        FROM pg_catalog.jsonb_populate_record(
            NULL::public.runtime_deployments,
            source_deployment_json
        ) AS populated;
        SELECT populated.*
        INTO STRICT successor_deployment
        FROM pg_catalog.jsonb_populate_record(
            NULL::public.runtime_deployments,
            successor_deployment_json
        ) AS populated;
        SELECT populated.*
        INTO STRICT source_slot
        FROM pg_catalog.jsonb_populate_record(
            NULL::public.runtime_slot_writer_fences_v2,
            source_slot_json
        ) AS populated;
        SELECT populated.*
        INTO STRICT successor_slot
        FROM pg_catalog.jsonb_populate_record(
            NULL::public.runtime_slot_writer_fences_v2,
            successor_slot_json
        ) AS populated;
    EXCEPTION
        WHEN OTHERS THEN
            RETURN FALSE;
    END;

    IF pg_catalog.jsonb_send(source_deployment_json)
            IS DISTINCT FROM source_deployment_frame
        OR pg_catalog.jsonb_send(successor_deployment_json)
            IS DISTINCT FROM successor_deployment_frame
        OR pg_catalog.jsonb_send(source_slot_json)
            IS DISTINCT FROM source_slot_frame
        OR pg_catalog.jsonb_send(successor_slot_json)
            IS DISTINCT FROM successor_slot_frame
        OR pg_catalog.to_jsonb(source_deployment)
            IS DISTINCT FROM source_deployment_json
        OR pg_catalog.to_jsonb(successor_deployment)
            IS DISTINCT FROM successor_deployment_json
        OR pg_catalog.to_jsonb(source_slot)
            IS DISTINCT FROM source_slot_json
        OR pg_catalog.to_jsonb(successor_slot)
            IS DISTINCT FROM successor_slot_json
        OR terminal_at_bytes IS DISTINCT FROM
            pg_catalog.timestamptz_send(successor_deployment.updated_at)
        OR successor_deployment.updated_at > expected_recorded_at
        OR NOT starring_runtime_private_v2.starring_runtime_unreserved_awaiting_reset_exact_v2(
            source_deployment,
            successor_deployment,
            successor_deployment.updated_at
        ) IS TRUE
        OR source_slot.slot_guild_id
            IS DISTINCT FROM source_deployment.guild_id
        OR source_slot.slot_ruleset_key
            IS DISTINCT FROM source_deployment.ruleset_key
        OR successor_slot.slot_guild_id
            IS DISTINCT FROM source_slot.slot_guild_id
        OR successor_slot.slot_ruleset_key
            IS DISTINCT FROM source_slot.slot_ruleset_key
        OR source_slot.writer_epoch
            NOT BETWEEN 1 AND 9223372036854775806
        OR successor_slot.writer_epoch
            IS DISTINCT FROM source_slot.writer_epoch + 1
        OR successor_slot.updated_at < source_slot.updated_at
        OR successor_slot.updated_at < successor_deployment.updated_at
        OR successor_slot.updated_at > expected_recorded_at
        OR source_slot.pending_drain_intent_id IS NOT NULL
        OR source_slot.pending_product_operation_id IS NOT NULL
        OR source_slot.pending_tenant_id IS NOT NULL
        OR source_slot.pending_installation_id IS NOT NULL
        OR source_slot.pending_deployment_id IS NOT NULL
        OR source_slot.pending_expected_revision IS NOT NULL
        OR source_slot.pending_marked_at IS NOT NULL
        OR successor_slot.pending_drain_intent_id IS NOT NULL
        OR successor_slot.pending_product_operation_id IS NOT NULL
        OR successor_slot.pending_tenant_id IS NOT NULL
        OR successor_slot.pending_installation_id IS NOT NULL
        OR successor_slot.pending_deployment_id IS NOT NULL
        OR successor_slot.pending_expected_revision IS NOT NULL
        OR successor_slot.pending_marked_at IS NOT NULL
        OR successor_slot_json - ARRAY[
            'writer_epoch',
            'updated_at'
        ]::TEXT[] IS DISTINCT FROM source_slot_json - ARRAY[
            'writer_epoch',
            'updated_at'
        ]::TEXT[]
        OR EXISTS (
            SELECT 1
            FROM public.runtime_certification_operations_v2 AS operation
            WHERE operation.tenant_id = source_deployment.tenant_id
                AND operation.installation_id =
                    source_deployment.installation_id
                AND operation.deployment_id =
                    source_deployment.deployment_id
                AND operation.deployment_revision =
                    source_deployment.revision
                AND operation.convergence_attempt_no =
                    source_deployment.convergence_attempt_no
        )
    THEN
        RETURN FALSE;
    END IF;

    SELECT deployment.*
    INTO current_deployment
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = source_deployment.tenant_id
        AND deployment.installation_id =
            source_deployment.installation_id
        AND deployment.deployment_id =
            source_deployment.deployment_id;
    RETURN FOUND
        AND current_deployment.snapshot -> 'revision'
            IS NOT DISTINCT FROM
            pg_catalog.to_jsonb(current_deployment.revision)
        AND (
            (
                current_deployment.revision =
                    successor_deployment.revision
                AND current_deployment.phase =
                    successor_deployment.phase
                AND current_deployment.convergence_attempt_no =
                    successor_deployment.convergence_attempt_no
                AND current_deployment.snapshot
                    IS NOT DISTINCT FROM successor_deployment.snapshot
            )
            OR (
                current_deployment.revision >
                    successor_deployment.revision
                AND current_deployment.snapshot #>> '{phase,phase}'
                    IS NOT DISTINCT FROM current_deployment.phase
                AND current_deployment.convergence_attempt_no >=
                    successor_deployment.convergence_attempt_no
            )
        );
EXCEPTION
    WHEN OTHERS THEN
        RETURN FALSE;
END;
$function$;

REVOKE ALL ON FUNCTION
    starring_runtime_private_v2.starring_runtime_startup_unreserved_projection_exact_v2(
        BYTEA,
        TIMESTAMPTZ
    )
FROM PUBLIC;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_startup_unreserved_execute_v2(
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
    observed_gateway_shard_id TEXT,
    observed_process_instance_id TEXT,
    observed_lease_epoch BIGINT,
    observed_runtime_build_revision TEXT,
    observed_owner_revision BIGINT,
    database_now TIMESTAMPTZ,
    observed_owner_expires_at TIMESTAMPTZ,
    recorded_at TIMESTAMPTZ,
    terminal_projection_bytes BYTEA,
    terminal_digest TEXT
)
LANGUAGE plpgsql
STRICT
SECURITY INVOKER
ROWS 1
SET search_path = pg_catalog
AS $function$
DECLARE
    owner_row public.runtime_gateway_owners%ROWTYPE;
    deployment_row public.runtime_deployments%ROWTYPE;
    terminal_deployment_row public.runtime_deployments%ROWTYPE;
    slot_fence_row public.runtime_slot_writer_fences_v2%ROWTYPE;
    terminal_slot_fence_row public.runtime_slot_writer_fences_v2%ROWTYPE;
    observation_row RECORD;
    action_record RECORD;
    slot_writer_epoch BIGINT;
    successor_slot_writer_epoch BIGINT;
    authority_outcome TEXT;
    mutation_clock TIMESTAMPTZ;
    next_revision BIGINT;
    next_snapshot JSONB;
    proposed_deployment JSONB;
    domain_bytes BYTEA;
    field_bytes BYTEA;
    projection_prefix BYTEA;
    source_deployment_frame BYTEA;
    successor_deployment_frame BYTEA;
    source_slot_frame BYTEA;
    successor_slot_frame BYTEA;
BEGIN
    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    INNER JOIN public.runtime_slot_writer_fences_v2 AS slot_fence
        ON slot_fence.slot_guild_id = deployment.guild_id
        AND slot_fence.slot_ruleset_key = deployment.ruleset_key
    WHERE deployment.phase = 'awaiting_gateway_ready'
        AND deployment.revision BETWEEN 1 AND 9223372036854775806
        AND deployment.snapshot -> 'phase' =
            '{"phase":"awaiting_gateway_ready"}'::JSONB
        AND deployment.snapshot -> 'revision' =
            pg_catalog.to_jsonb(deployment.revision)
        AND deployment.controller_id IS NOT NULL
        AND deployment.controller_fencing_token IS NOT NULL
        AND deployment.controller_acquired_at IS NOT NULL
        AND deployment.controller_lease_expires_at IS NOT NULL
        AND deployment.last_controller_id = deployment.controller_id
        AND deployment.last_fencing_token =
            deployment.controller_fencing_token
        AND pg_catalog.jsonb_typeof(
            deployment.snapshot -> 'panel_certificate'
        ) = 'object'
        AND deployment.snapshot -> 'gateway_ready' = 'null'::JSONB
        AND deployment.snapshot -> 'live' = 'null'::JSONB
        AND deployment.live_attestation_id IS NULL
        AND deployment.live_at IS NULL
        AND slot_fence.writer_epoch
            BETWEEN 1 AND 9223372036854775806
        AND slot_fence.pending_drain_intent_id IS NULL
        AND slot_fence.pending_product_operation_id IS NULL
        AND slot_fence.pending_tenant_id IS NULL
        AND slot_fence.pending_installation_id IS NULL
        AND slot_fence.pending_deployment_id IS NULL
        AND slot_fence.pending_expected_revision IS NULL
        AND slot_fence.pending_marked_at IS NULL
        AND NOT EXISTS (
            SELECT 1
            FROM public.runtime_drain_intents_v2 AS drain
            WHERE drain.slot_guild_id = deployment.guild_id
                AND drain.slot_ruleset_key = deployment.ruleset_key
                AND drain.intent_state IN (
                    'pending',
                    'route_absent_acknowledged'
                )
        )
        AND NOT EXISTS (
            SELECT 1
            FROM public.runtime_certification_operations_v2 AS operation
            WHERE operation.tenant_id = deployment.tenant_id
                AND operation.installation_id =
                    deployment.installation_id
                AND operation.deployment_id =
                    deployment.deployment_id
                AND operation.deployment_revision =
                    deployment.revision
                AND operation.convergence_attempt_no =
                    deployment.convergence_attempt_no
        )
    ORDER BY
        deployment.requested_at,
        deployment.deployment_id COLLATE pg_catalog."C",
        deployment.revision,
        deployment.convergence_attempt_no
    LIMIT 1;
    IF NOT FOUND THEN
        RETURN;
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            pg_catalog.concat(
                'starring-runtime-serving-slot-v1:',
                deployment_row.guild_id,
                ':',
                deployment_row.ruleset_key
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
        OR database_now < requested_minimum_database_now
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_startup_unreserved_awaiting_owner_lost';
    END IF;

    SELECT fence.*
    INTO slot_fence_row
    FROM public.runtime_slot_writer_fences_v2 AS fence
    WHERE fence.slot_guild_id = deployment_row.guild_id
        AND fence.slot_ruleset_key = deployment_row.ruleset_key
    FOR UPDATE;
    IF NOT FOUND
        OR slot_fence_row.writer_epoch
            NOT BETWEEN 1 AND 9223372036854775806
    THEN
        RAISE EXCEPTION USING
            ERRCODE = '40001',
            MESSAGE = 'runtime_startup_unreserved_awaiting_selection_changed';
    END IF;
    IF slot_fence_row.pending_drain_intent_id IS NULL
        AND slot_fence_row.pending_product_operation_id IS NULL
        AND slot_fence_row.pending_tenant_id IS NULL
        AND slot_fence_row.pending_installation_id IS NULL
        AND slot_fence_row.pending_deployment_id IS NULL
        AND slot_fence_row.pending_expected_revision IS NULL
        AND slot_fence_row.pending_marked_at IS NULL
    THEN
        IF EXISTS (
            SELECT 1
            FROM public.runtime_drain_intents_v2 AS drain
            WHERE drain.slot_guild_id = deployment_row.guild_id
                AND drain.slot_ruleset_key = deployment_row.ruleset_key
                AND drain.intent_state IN (
                    'pending',
                    'route_absent_acknowledged'
                )
            FOR SHARE
        )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_startup_unreserved_awaiting_drain_state_invalid';
        END IF;
    ELSIF slot_fence_row.pending_drain_intent_id IS NOT NULL
        AND slot_fence_row.pending_product_operation_id IS NOT NULL
        AND slot_fence_row.pending_tenant_id IS NOT NULL
        AND slot_fence_row.pending_installation_id IS NOT NULL
        AND slot_fence_row.pending_deployment_id IS NOT NULL
        AND slot_fence_row.pending_expected_revision IS NOT NULL
        AND slot_fence_row.pending_marked_at IS NOT NULL
        AND EXISTS (
            SELECT 1
            FROM public.runtime_drain_intents_v2 AS drain
            WHERE drain.drain_intent_id =
                    slot_fence_row.pending_drain_intent_id
                AND drain.product_operation_id =
                    slot_fence_row.pending_product_operation_id
                AND drain.tenant_id =
                    slot_fence_row.pending_tenant_id
                AND drain.installation_id =
                    slot_fence_row.pending_installation_id
                AND drain.deployment_id =
                    slot_fence_row.pending_deployment_id
                AND drain.expected_revision =
                    slot_fence_row.pending_expected_revision
                AND drain.slot_guild_id = deployment_row.guild_id
                AND drain.slot_ruleset_key = deployment_row.ruleset_key
                AND drain.intent_state IN (
                    'pending',
                    'route_absent_acknowledged'
                )
            FOR SHARE
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX007',
            MESSAGE = 'runtime_startup_unreserved_awaiting_product_drain_pending';
    ELSE
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_unreserved_awaiting_drain_state_invalid';
    END IF;
    slot_writer_epoch := slot_fence_row.writer_epoch;

    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = deployment_row.tenant_id
        AND deployment.installation_id =
            deployment_row.installation_id
        AND deployment.deployment_id = deployment_row.deployment_id
    FOR UPDATE;
    IF NOT FOUND
        OR deployment_row.phase <> 'awaiting_gateway_ready'
        OR deployment_row.revision NOT BETWEEN 1 AND 9223372036854775806
        OR deployment_row.snapshot -> 'phase'
            IS DISTINCT FROM
            '{"phase":"awaiting_gateway_ready"}'::JSONB
        OR deployment_row.snapshot -> 'revision'
            IS DISTINCT FROM
            pg_catalog.to_jsonb(deployment_row.revision)
        OR deployment_row.controller_id IS NULL
        OR deployment_row.controller_fencing_token IS NULL
        OR deployment_row.controller_acquired_at IS NULL
        OR deployment_row.controller_lease_expires_at IS NULL
        OR deployment_row.last_controller_id
            IS DISTINCT FROM deployment_row.controller_id
        OR deployment_row.last_fencing_token
            IS DISTINCT FROM deployment_row.controller_fencing_token
        OR pg_catalog.jsonb_typeof(
            deployment_row.snapshot -> 'panel_certificate'
        ) <> 'object'
        OR deployment_row.snapshot -> 'gateway_ready'
            IS DISTINCT FROM 'null'::JSONB
        OR deployment_row.snapshot -> 'live'
            IS DISTINCT FROM 'null'::JSONB
        OR deployment_row.live_attestation_id IS NOT NULL
        OR deployment_row.live_at IS NOT NULL
        OR EXISTS (
            SELECT 1
            FROM public.runtime_certification_operations_v2 AS operation
            WHERE operation.tenant_id = deployment_row.tenant_id
                AND operation.installation_id =
                    deployment_row.installation_id
                AND operation.deployment_id =
                    deployment_row.deployment_id
                AND operation.deployment_revision =
                    deployment_row.revision
                AND operation.convergence_attempt_no =
                    deployment_row.convergence_attempt_no
            FOR SHARE
        )
        OR EXISTS (
            SELECT 1
            FROM public.runtime_drain_intents_v2 AS drain
            WHERE drain.slot_guild_id = deployment_row.guild_id
                AND drain.slot_ruleset_key =
                    deployment_row.ruleset_key
                AND drain.intent_state IN (
                    'pending',
                    'route_absent_acknowledged'
                )
            FOR SHARE
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = '40001',
            MESSAGE = 'runtime_startup_unreserved_awaiting_selection_changed';
    END IF;

    SELECT observation.*
    INTO STRICT observation_row
    FROM public.starring_runtime_startup_recovery_observe_v2(
        expected_gateway_shard_id,
        expected_owner_process_instance_id,
        expected_owner_lease_epoch,
        expected_owner_runtime_build_revision,
        expected_owner_revision,
        expected_owner_expires_at
    ) AS observation;
    IF observation_row.outcome_name = 'not_current' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_startup_unreserved_awaiting_owner_lost';
    ELSIF observation_row.outcome_name IS DISTINCT FROM 'observed'
        OR observation_row.serving_state_name = 'ambiguous'
        OR observation_row.recoverable_awaiting_certification_count < 1
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_unreserved_awaiting_state_ambiguous';
    ELSIF observation_row.serving_state_name = 'recoverable_stale' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_startup_unreserved_awaiting_higher_priority_pending';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM public.runtime_attestations AS attestation
        WHERE attestation.tenant_id = deployment_row.tenant_id
            AND attestation.installation_id =
                deployment_row.installation_id
            AND attestation.deployment_id =
                deployment_row.deployment_id
            AND attestation.deployment_revision =
                deployment_row.revision + 1
        FOR SHARE
    ) OR EXISTS (
        SELECT 1
        FROM public.runtime_serving_leases AS serving
        WHERE serving.tenant_id = deployment_row.tenant_id
            AND serving.installation_id =
                deployment_row.installation_id
            AND serving.deployment_id =
                deployment_row.deployment_id
        FOR SHARE
    )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_unreserved_awaiting_attestation_invalid';
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
            MESSAGE = 'runtime_startup_unreserved_awaiting_target_superseded';
    ELSIF authority_outcome IS DISTINCT FROM 'exact' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_startup_unreserved_awaiting_authority_changed';
    END IF;

    mutation_clock := public.starring_runtime_mutation_clock();
    IF mutation_clock < database_now
        OR mutation_clock < requested_minimum_database_now
        OR mutation_clock >= expected_owner_expires_at
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_startup_unreserved_awaiting_owner_lost';
    END IF;

    successor_slot_writer_epoch :=
        starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(
            deployment_row.guild_id,
            deployment_row.ruleset_key,
            slot_writer_epoch
        );
    IF successor_slot_writer_epoch <> slot_writer_epoch + 1 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_unreserved_awaiting_slot_invalid';
    END IF;

    SELECT fence.*
    INTO terminal_slot_fence_row
    FROM public.runtime_slot_writer_fences_v2 AS fence
    WHERE fence.slot_guild_id = deployment_row.guild_id
        AND fence.slot_ruleset_key = deployment_row.ruleset_key
    FOR UPDATE;
    IF NOT FOUND
        OR terminal_slot_fence_row.writer_epoch
            IS DISTINCT FROM successor_slot_writer_epoch
        OR terminal_slot_fence_row.pending_drain_intent_id IS NOT NULL
        OR terminal_slot_fence_row.pending_product_operation_id IS NOT NULL
        OR terminal_slot_fence_row.pending_tenant_id IS NOT NULL
        OR terminal_slot_fence_row.pending_installation_id IS NOT NULL
        OR terminal_slot_fence_row.pending_deployment_id IS NOT NULL
        OR terminal_slot_fence_row.pending_expected_revision IS NOT NULL
        OR terminal_slot_fence_row.pending_marked_at IS NOT NULL
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_unreserved_awaiting_slot_invalid';
    END IF;

    next_revision := deployment_row.revision + 1;
    next_snapshot := pg_catalog.jsonb_set(
        deployment_row.snapshot,
        '{revision}',
        pg_catalog.to_jsonb(next_revision),
        FALSE
    );
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{phase}',
        '{"phase":"reconciling_panels"}'::JSONB,
        FALSE
    );
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{controller_lease}',
        'null'::JSONB,
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
    proposed_deployment := pg_catalog.to_jsonb(deployment_row)
        || pg_catalog.jsonb_build_object(
            'snapshot', next_snapshot,
            'revision', next_revision,
            'phase', 'reconciling_panels',
            'controller_id', NULL::TEXT,
            'controller_fencing_token', NULL::BIGINT,
            'controller_acquired_at', NULL::TIMESTAMPTZ,
            'controller_lease_expires_at', NULL::TIMESTAMPTZ,
            'updated_at', GREATEST(
                mutation_clock,
                deployment_row.updated_at + INTERVAL '1 microsecond'
            )
        );
    source_deployment_frame :=
        pg_catalog.jsonb_send(pg_catalog.to_jsonb(deployment_row));
    successor_deployment_frame :=
        pg_catalog.jsonb_send(proposed_deployment);
    source_slot_frame :=
        pg_catalog.jsonb_send(pg_catalog.to_jsonb(slot_fence_row));
    successor_slot_frame :=
        pg_catalog.jsonb_send(
            pg_catalog.to_jsonb(terminal_slot_fence_row)
        );

    UPDATE public.runtime_deployments AS deployment
    SET snapshot = next_snapshot,
        revision = next_revision,
        phase = 'reconciling_panels',
        controller_id = NULL,
        controller_fencing_token = NULL,
        controller_acquired_at = NULL,
        controller_lease_expires_at = NULL,
        updated_at = GREATEST(
            mutation_clock,
            deployment.updated_at + INTERVAL '1 microsecond'
        )
    WHERE deployment.tenant_id = deployment_row.tenant_id
        AND deployment.installation_id =
            deployment_row.installation_id
        AND deployment.deployment_id = deployment_row.deployment_id
        AND deployment.revision = deployment_row.revision
    RETURNING deployment.* INTO terminal_deployment_row;
    IF NOT FOUND
        OR pg_catalog.to_jsonb(terminal_deployment_row)
            IS DISTINCT FROM proposed_deployment
        OR NOT starring_runtime_private_v2.starring_runtime_unreserved_awaiting_reset_exact_v2(
            deployment_row,
            terminal_deployment_row,
            mutation_clock
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_unreserved_awaiting_transition_invalid';
    END IF;

    domain_bytes := pg_catalog.convert_to(
        'starring.runtime.startup_recovery.reserved_awaiting_certification.terminal.v2',
        'UTF8'
    );
    projection_prefix :=
        pg_catalog.int8send(
            pg_catalog.octet_length(domain_bytes)::BIGINT
        )
        || domain_bytes
        || pg_catalog.int2send(2::SMALLINT)
        || pg_catalog.int2send(2::SMALLINT);
    terminal_projection_bytes := projection_prefix;
    FOREACH field_bytes IN ARRAY ARRAY[
        source_deployment_frame,
        successor_deployment_frame,
        source_slot_frame,
        successor_slot_frame
    ]
    LOOP
        terminal_projection_bytes := terminal_projection_bytes
            || pg_catalog.int8send(
                pg_catalog.octet_length(field_bytes)::BIGINT
            )
            || field_bytes;
    END LOOP;
    terminal_projection_bytes := terminal_projection_bytes
        || pg_catalog.timestamptz_send(
            terminal_deployment_row.updated_at
        );
    SELECT record.*
    INTO STRICT action_record
    FROM starring_runtime_private_v2.starring_runtime_startup_recovery_action_record_v2(
        requested_recovery_id,
        requested_originating_emergency_generation,
        requested_coordinator_generation,
        requested_action_authority_revision,
        requested_selection_authority_revision,
        'reserved_awaiting_certification',
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
        OR NOT starring_runtime_private_v2.starring_runtime_startup_unreserved_projection_exact_v2(
            terminal_projection_bytes,
            action_record.recorded_at
        ) IS TRUE
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_unreserved_awaiting_record_invalid';
    END IF;

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
    recorded_at := action_record.recorded_at;
    terminal_digest := action_record.terminal_digest;
    RETURN NEXT;
END;
$function$;

REVOKE ALL ON FUNCTION
    starring_runtime_private_v2.starring_runtime_startup_unreserved_execute_v2(
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
        '    executable_unresolved_reservation_count BIGINT;' || E'\n' ||
        '    exact_terminal_reservation_count BIGINT;';
    next_fragment :=
        '    executable_unresolved_reservation_count BIGINT;' || E'\n' ||
        '    exact_terminal_reservation_count BIGINT;' || E'\n' ||
        '    unreserved_awaiting_count BIGINT;' || E'\n' ||
        '    executable_unreserved_awaiting_count BIGINT;' || E'\n' ||
        '    blocked_unreserved_awaiting_count BIGINT;';
    IF definition IS NULL
        OR pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_unreserved_observation_declaration_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    IF executable_unresolved_reservation_count > 4294967295';
    next_fragment :=
        '    SELECT' || E'\n' ||
        '        pg_catalog.count(*),' || E'\n' ||
        '        pg_catalog.count(*) FILTER (' || E'\n' ||
        '            WHERE slot_fence.writer_epoch' || E'\n' ||
        '                    BETWEEN 1 AND 9223372036854775806' || E'\n' ||
        '                AND slot_fence.pending_drain_intent_id IS NULL' || E'\n' ||
        '                AND slot_fence.pending_product_operation_id IS NULL' || E'\n' ||
        '                AND slot_fence.pending_tenant_id IS NULL' || E'\n' ||
        '                AND slot_fence.pending_installation_id IS NULL' || E'\n' ||
        '                AND slot_fence.pending_deployment_id IS NULL' || E'\n' ||
        '                AND slot_fence.pending_expected_revision IS NULL' || E'\n' ||
        '                AND slot_fence.pending_marked_at IS NULL' || E'\n' ||
        '                AND NOT EXISTS (' || E'\n' ||
        '                    SELECT 1' || E'\n' ||
        '                    FROM public.runtime_drain_intents_v2 AS drain' || E'\n' ||
        '                    WHERE drain.slot_guild_id = deployment.guild_id' || E'\n' ||
        '                        AND drain.slot_ruleset_key =' || E'\n' ||
        '                            deployment.ruleset_key' || E'\n' ||
        '                        AND drain.intent_state IN (' || E'\n' ||
        '                            ''pending'',' || E'\n' ||
        '                            ''route_absent_acknowledged''' || E'\n' ||
        '                        )' || E'\n' ||
        '                )' || E'\n' ||
        '        ),' || E'\n' ||
        '        pg_catalog.count(*) FILTER (' || E'\n' ||
        '            WHERE slot_fence.writer_epoch' || E'\n' ||
        '                    BETWEEN 1 AND 9223372036854775806' || E'\n' ||
        '                AND slot_fence.pending_drain_intent_id IS NOT NULL' || E'\n' ||
        '                AND slot_fence.pending_product_operation_id IS NOT NULL' || E'\n' ||
        '                AND slot_fence.pending_tenant_id IS NOT NULL' || E'\n' ||
        '                AND slot_fence.pending_installation_id IS NOT NULL' || E'\n' ||
        '                AND slot_fence.pending_deployment_id IS NOT NULL' || E'\n' ||
        '                AND slot_fence.pending_expected_revision IS NOT NULL' || E'\n' ||
        '                AND slot_fence.pending_marked_at IS NOT NULL' || E'\n' ||
        '                AND EXISTS (' || E'\n' ||
        '                    SELECT 1' || E'\n' ||
        '                    FROM public.runtime_drain_intents_v2 AS drain' || E'\n' ||
        '                    WHERE drain.drain_intent_id =' || E'\n' ||
        '                            slot_fence.pending_drain_intent_id' || E'\n' ||
        '                        AND drain.product_operation_id =' || E'\n' ||
        '                            slot_fence.pending_product_operation_id' || E'\n' ||
        '                        AND drain.tenant_id =' || E'\n' ||
        '                            slot_fence.pending_tenant_id' || E'\n' ||
        '                        AND drain.installation_id =' || E'\n' ||
        '                            slot_fence.pending_installation_id' || E'\n' ||
        '                        AND drain.deployment_id =' || E'\n' ||
        '                            slot_fence.pending_deployment_id' || E'\n' ||
        '                        AND drain.expected_revision =' || E'\n' ||
        '                            slot_fence.pending_expected_revision' || E'\n' ||
        '                        AND drain.slot_guild_id = deployment.guild_id' || E'\n' ||
        '                        AND drain.slot_ruleset_key =' || E'\n' ||
        '                            deployment.ruleset_key' || E'\n' ||
        '                        AND drain.intent_state IN (' || E'\n' ||
        '                            ''pending'',' || E'\n' ||
        '                            ''route_absent_acknowledged''' || E'\n' ||
        '                        )' || E'\n' ||
        '                )' || E'\n' ||
        '        )' || E'\n' ||
        '    INTO' || E'\n' ||
        '        unreserved_awaiting_count,' || E'\n' ||
        '        executable_unreserved_awaiting_count,' || E'\n' ||
        '        blocked_unreserved_awaiting_count' || E'\n' ||
        '    FROM public.runtime_deployments AS deployment' || E'\n' ||
        '    LEFT JOIN public.runtime_slot_writer_fences_v2 AS slot_fence' || E'\n' ||
        '        ON slot_fence.slot_guild_id = deployment.guild_id' || E'\n' ||
        '        AND slot_fence.slot_ruleset_key = deployment.ruleset_key' || E'\n' ||
        '    WHERE deployment.phase = ''awaiting_gateway_ready''' || E'\n' ||
        '        AND deployment.revision BETWEEN 1 AND 9223372036854775806' || E'\n' ||
        '        AND deployment.snapshot -> ''phase'' =' || E'\n' ||
        '            ''{"phase":"awaiting_gateway_ready"}''::JSONB' || E'\n' ||
        '        AND deployment.snapshot -> ''revision'' =' || E'\n' ||
        '            pg_catalog.to_jsonb(deployment.revision)' || E'\n' ||
        '        AND deployment.controller_id IS NOT NULL' || E'\n' ||
        '        AND deployment.controller_fencing_token IS NOT NULL' || E'\n' ||
        '        AND deployment.controller_acquired_at IS NOT NULL' || E'\n' ||
        '        AND deployment.controller_lease_expires_at IS NOT NULL' || E'\n' ||
        '        AND deployment.last_controller_id = deployment.controller_id' || E'\n' ||
        '        AND deployment.last_fencing_token =' || E'\n' ||
        '            deployment.controller_fencing_token' || E'\n' ||
        '        AND pg_catalog.jsonb_typeof(' || E'\n' ||
        '            deployment.snapshot -> ''panel_certificate''' || E'\n' ||
        '        ) = ''object''' || E'\n' ||
        '        AND deployment.snapshot -> ''gateway_ready'' = ''null''::JSONB' || E'\n' ||
        '        AND deployment.snapshot -> ''live'' = ''null''::JSONB' || E'\n' ||
        '        AND deployment.live_attestation_id IS NULL' || E'\n' ||
        '        AND deployment.live_at IS NULL' || E'\n' ||
        '        AND NOT EXISTS (' || E'\n' ||
        '            SELECT 1' || E'\n' ||
        '            FROM public.runtime_certification_operations_v2 AS operation' || E'\n' ||
        '            WHERE operation.tenant_id = deployment.tenant_id' || E'\n' ||
        '                AND operation.installation_id =' || E'\n' ||
        '                    deployment.installation_id' || E'\n' ||
        '                AND operation.deployment_id =' || E'\n' ||
        '                    deployment.deployment_id' || E'\n' ||
        '                AND operation.deployment_revision = deployment.revision' || E'\n' ||
        '                AND operation.convergence_attempt_no =' || E'\n' ||
        '                    deployment.convergence_attempt_no' || E'\n' ||
        '        );' || E'\n' ||
        E'\n' ||
        '    IF unreserved_awaiting_count' || E'\n' ||
        '            <> executable_unreserved_awaiting_count' || E'\n' ||
        '                + blocked_unreserved_awaiting_count' || E'\n' ||
        '    THEN' || E'\n' ||
        '        outcome_name := ''ambiguous'';' || E'\n' ||
        '        serving_state_name := ''ambiguous'';' || E'\n' ||
        '        serving_count := NULL;' || E'\n' ||
        '        serving_earliest_expiry := NULL;' || E'\n' ||
        '        serving_retry_after_milliseconds := NULL;' || E'\n' ||
        '        RETURN NEXT;' || E'\n' ||
        '        RETURN;' || E'\n' ||
        '    END IF;' || E'\n' ||
        E'\n' ||
        '    IF executable_unresolved_reservation_count' || E'\n' ||
        '            + executable_unreserved_awaiting_count > 4294967295';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_unreserved_observation_query_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    recoverable_awaiting_certification_count :=' || E'\n' ||
        '        executable_unresolved_reservation_count;';
    next_fragment :=
        '    recoverable_awaiting_certification_count :=' || E'\n' ||
        '        executable_unresolved_reservation_count' || E'\n' ||
        '        + executable_unreserved_awaiting_count;';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_unreserved_observation_projection_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$patch_observation$;

DO $patch_execution$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_startup_recovery_execute_reserved_awaiting_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)'
    );

    previous_fragment :=
        '    observation_row RECORD;' || E'\n' ||
        '    selection_action_found BOOLEAN;';
    next_fragment :=
        '    observation_row RECORD;' || E'\n' ||
        '    unreserved_recovery_row RECORD;' || E'\n' ||
        '    selection_action_found BOOLEAN;';
    IF definition IS NULL
        OR pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_unreserved_execution_declaration_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    progressed_projection_prefix BYTEA;' || E'\n' ||
        'BEGIN';
    next_fragment :=
        '    progressed_projection_prefix BYTEA;' || E'\n' ||
        '    unreserved_progressed_projection_prefix BYTEA;' || E'\n' ||
        'BEGIN';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_unreserved_execution_prefix_declaration_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    progressed_projection_prefix :=' || E'\n' ||
        '        pg_catalog.int8send(' || E'\n' ||
        '            pg_catalog.octet_length(domain_bytes)::BIGINT' || E'\n' ||
        '        )' || E'\n' ||
        '        || domain_bytes' || E'\n' ||
        '        || pg_catalog.int2send(2::SMALLINT)' || E'\n' ||
        '        || pg_catalog.int2send(1::SMALLINT);';
    next_fragment := previous_fragment || E'\n' ||
        '    unreserved_progressed_projection_prefix :=' || E'\n' ||
        '        pg_catalog.int8send(' || E'\n' ||
        '            pg_catalog.octet_length(domain_bytes)::BIGINT' || E'\n' ||
        '        )' || E'\n' ||
        '        || domain_bytes' || E'\n' ||
        '        || pg_catalog.int2send(2::SMALLINT)' || E'\n' ||
        '        || pg_catalog.int2send(2::SMALLINT);';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_unreserved_execution_prefix_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '        ELSIF pg_catalog.substring(' || E'\n' ||
        '                existing_action_row.terminal_projection_bytes,' || E'\n' ||
        '                1,' || E'\n' ||
        '                pg_catalog.octet_length(progressed_projection_prefix)' || E'\n' ||
        '            ) IS NOT DISTINCT FROM progressed_projection_prefix';
    next_fragment :=
        '        ELSIF pg_catalog.substring(' || E'\n' ||
        '                existing_action_row.terminal_projection_bytes,' || E'\n' ||
        '                1,' || E'\n' ||
        '                pg_catalog.octet_length(' || E'\n' ||
        '                    unreserved_progressed_projection_prefix' || E'\n' ||
        '                )' || E'\n' ||
        '            ) IS NOT DISTINCT FROM' || E'\n' ||
        '                unreserved_progressed_projection_prefix' || E'\n' ||
        '            AND starring_runtime_private_v2.starring_runtime_startup_unreserved_projection_exact_v2(' || E'\n' ||
        '                existing_action_row.terminal_projection_bytes,' || E'\n' ||
        '                action_record.recorded_at' || E'\n' ||
        '            ) IS TRUE' || E'\n' ||
        '        THEN' || E'\n' ||
        '            terminal_outcome_name := ''progressed'';' || E'\n' ||
        '        ELSIF pg_catalog.substring(' || E'\n' ||
        '                existing_action_row.terminal_projection_bytes,' || E'\n' ||
        '                1,' || E'\n' ||
        '                pg_catalog.octet_length(progressed_projection_prefix)' || E'\n' ||
        '            ) IS NOT DISTINCT FROM progressed_projection_prefix';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_unreserved_execution_replay_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    WITH inventory AS (' || E'\n' ||
        '        SELECT';
    next_fragment :=
        '    SELECT recovery.*' || E'\n' ||
        '    INTO unreserved_recovery_row' || E'\n' ||
        '    FROM starring_runtime_private_v2.starring_runtime_startup_unreserved_execute_v2(' || E'\n' ||
        '        requested_recovery_id,' || E'\n' ||
        '        requested_originating_emergency_generation,' || E'\n' ||
        '        requested_coordinator_generation,' || E'\n' ||
        '        requested_action_authority_revision,' || E'\n' ||
        '        requested_selection_authority_revision,' || E'\n' ||
        '        expected_gateway_shard_id,' || E'\n' ||
        '        expected_owner_process_instance_id,' || E'\n' ||
        '        expected_owner_lease_epoch,' || E'\n' ||
        '        expected_owner_runtime_build_revision,' || E'\n' ||
        '        expected_owner_revision,' || E'\n' ||
        '        expected_owner_expires_at,' || E'\n' ||
        '        requested_minimum_database_now' || E'\n' ||
        '    ) AS recovery;' || E'\n' ||
        '    IF FOUND THEN' || E'\n' ||
        '        journal_outcome_name := ''applied'';' || E'\n' ||
        '        terminal_outcome_name := ''progressed'';' || E'\n' ||
        '        recovery_id := requested_recovery_id;' || E'\n' ||
        '        originating_emergency_generation :=' || E'\n' ||
        '            requested_originating_emergency_generation;' || E'\n' ||
        '        coordinator_generation := requested_coordinator_generation;' || E'\n' ||
        '        action_authority_revision :=' || E'\n' ||
        '            requested_action_authority_revision;' || E'\n' ||
        '        selection_authority_revision :=' || E'\n' ||
        '            requested_selection_authority_revision;' || E'\n' ||
        '        recovery_class := ''reserved_awaiting_certification'';' || E'\n' ||
        '        observed_gateway_shard_id :=' || E'\n' ||
        '            unreserved_recovery_row.observed_gateway_shard_id;' || E'\n' ||
        '        observed_process_instance_id :=' || E'\n' ||
        '            unreserved_recovery_row.observed_process_instance_id;' || E'\n' ||
        '        observed_lease_epoch :=' || E'\n' ||
        '            unreserved_recovery_row.observed_lease_epoch;' || E'\n' ||
        '        observed_runtime_build_revision :=' || E'\n' ||
        '            unreserved_recovery_row.observed_runtime_build_revision;' || E'\n' ||
        '        observed_owner_revision :=' || E'\n' ||
        '            unreserved_recovery_row.observed_owner_revision;' || E'\n' ||
        '        database_now := unreserved_recovery_row.database_now;' || E'\n' ||
        '        observed_owner_expires_at :=' || E'\n' ||
        '            unreserved_recovery_row.observed_owner_expires_at;' || E'\n' ||
        '        minimum_database_now := requested_minimum_database_now;' || E'\n' ||
        '        recorded_at := unreserved_recovery_row.recorded_at;' || E'\n' ||
        '        terminal_projection_bytes :=' || E'\n' ||
        '            unreserved_recovery_row.terminal_projection_bytes;' || E'\n' ||
        '        terminal_digest := unreserved_recovery_row.terminal_digest;' || E'\n' ||
        '        RETURN NEXT;' || E'\n' ||
        '        RETURN;' || E'\n' ||
        '    END IF;' || E'\n' ||
        E'\n' ||
        '    WITH inventory AS (' || E'\n' ||
        '        SELECT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_unreserved_execution_dispatch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    IF executable_reservation_count = 0 THEN' || E'\n' ||
        '        SELECT observation.*';
    next_fragment :=
        '    IF executable_reservation_count = 0 THEN' || E'\n' ||
        '        IF EXISTS (' || E'\n' ||
        '            SELECT 1' || E'\n' ||
        '            FROM public.runtime_deployments AS deployment' || E'\n' ||
        '            INNER JOIN public.runtime_slot_writer_fences_v2 AS slot_fence' || E'\n' ||
        '                ON slot_fence.slot_guild_id = deployment.guild_id' || E'\n' ||
        '                AND slot_fence.slot_ruleset_key = deployment.ruleset_key' || E'\n' ||
        '            WHERE deployment.phase = ''awaiting_gateway_ready''' || E'\n' ||
        '                AND deployment.revision BETWEEN 1 AND 9223372036854775806' || E'\n' ||
        '                AND deployment.snapshot -> ''phase'' =' || E'\n' ||
        '                    ''{"phase":"awaiting_gateway_ready"}''::JSONB' || E'\n' ||
        '                AND deployment.snapshot -> ''revision'' =' || E'\n' ||
        '                    pg_catalog.to_jsonb(deployment.revision)' || E'\n' ||
        '                AND deployment.controller_id IS NOT NULL' || E'\n' ||
        '                AND deployment.controller_fencing_token IS NOT NULL' || E'\n' ||
        '                AND deployment.controller_acquired_at IS NOT NULL' || E'\n' ||
        '                AND deployment.controller_lease_expires_at IS NOT NULL' || E'\n' ||
        '                AND deployment.last_controller_id = deployment.controller_id' || E'\n' ||
        '                AND deployment.last_fencing_token =' || E'\n' ||
        '                    deployment.controller_fencing_token' || E'\n' ||
        '                AND pg_catalog.jsonb_typeof(' || E'\n' ||
        '                    deployment.snapshot -> ''panel_certificate''' || E'\n' ||
        '                ) = ''object''' || E'\n' ||
        '                AND deployment.snapshot -> ''gateway_ready'' = ''null''::JSONB' || E'\n' ||
        '                AND deployment.snapshot -> ''live'' = ''null''::JSONB' || E'\n' ||
        '                AND deployment.live_attestation_id IS NULL' || E'\n' ||
        '                AND deployment.live_at IS NULL' || E'\n' ||
        '                AND slot_fence.writer_epoch' || E'\n' ||
        '                    BETWEEN 1 AND 9223372036854775806' || E'\n' ||
        '                AND slot_fence.pending_drain_intent_id IS NOT NULL' || E'\n' ||
        '                AND slot_fence.pending_product_operation_id IS NOT NULL' || E'\n' ||
        '                AND slot_fence.pending_tenant_id IS NOT NULL' || E'\n' ||
        '                AND slot_fence.pending_installation_id IS NOT NULL' || E'\n' ||
        '                AND slot_fence.pending_deployment_id IS NOT NULL' || E'\n' ||
        '                AND slot_fence.pending_expected_revision IS NOT NULL' || E'\n' ||
        '                AND slot_fence.pending_marked_at IS NOT NULL' || E'\n' ||
        '                AND EXISTS (' || E'\n' ||
        '                    SELECT 1' || E'\n' ||
        '                    FROM public.runtime_drain_intents_v2 AS drain' || E'\n' ||
        '                    WHERE drain.drain_intent_id =' || E'\n' ||
        '                            slot_fence.pending_drain_intent_id' || E'\n' ||
        '                        AND drain.product_operation_id =' || E'\n' ||
        '                            slot_fence.pending_product_operation_id' || E'\n' ||
        '                        AND drain.tenant_id = slot_fence.pending_tenant_id' || E'\n' ||
        '                        AND drain.installation_id =' || E'\n' ||
        '                            slot_fence.pending_installation_id' || E'\n' ||
        '                        AND drain.deployment_id =' || E'\n' ||
        '                            slot_fence.pending_deployment_id' || E'\n' ||
        '                        AND drain.expected_revision =' || E'\n' ||
        '                            slot_fence.pending_expected_revision' || E'\n' ||
        '                        AND drain.slot_guild_id = deployment.guild_id' || E'\n' ||
        '                        AND drain.slot_ruleset_key =' || E'\n' ||
        '                            deployment.ruleset_key' || E'\n' ||
        '                        AND drain.intent_state IN (' || E'\n' ||
        '                            ''pending'',' || E'\n' ||
        '                            ''route_absent_acknowledged''' || E'\n' ||
        '                        )' || E'\n' ||
        '                )' || E'\n' ||
        '                AND NOT EXISTS (' || E'\n' ||
        '                    SELECT 1' || E'\n' ||
        '                    FROM public.runtime_certification_operations_v2 AS operation' || E'\n' ||
        '                    WHERE operation.tenant_id = deployment.tenant_id' || E'\n' ||
        '                        AND operation.installation_id =' || E'\n' ||
        '                            deployment.installation_id' || E'\n' ||
        '                        AND operation.deployment_id =' || E'\n' ||
        '                            deployment.deployment_id' || E'\n' ||
        '                        AND operation.deployment_revision = deployment.revision' || E'\n' ||
        '                        AND operation.convergence_attempt_no =' || E'\n' ||
        '                            deployment.convergence_attempt_no' || E'\n' ||
        '                )' || E'\n' ||
        '        )' || E'\n' ||
        '        THEN' || E'\n' ||
        '            RAISE EXCEPTION USING' || E'\n' ||
        '                ERRCODE = ''RX007'',' || E'\n' ||
        '                MESSAGE = ''runtime_startup_unreserved_awaiting_product_drain_pending'';' || E'\n' ||
        '        END IF;' || E'\n' ||
        E'\n' ||
        '        SELECT observation.*';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_unreserved_execution_blocked_dispatch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    EXECUTE definition;
END;
$patch_execution$;

DO $extend_security_manifests$
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
        '            ''starring_runtime_private_v2.starring_runtime_cert_awaiting_reset_exact_v2(public.runtime_deployments,public.runtime_deployments,timestamp with time zone)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_startup_reserved_projection_exact_v2(bytea,text,bigint,bigint,bigint,bigint,timestamp with time zone,public.runtime_certification_operation_terminals_v2)''';
    next_fragment :=
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_cert_awaiting_reset_exact_v2(public.runtime_deployments,public.runtime_deployments,timestamp with time zone)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_unreserved_awaiting_reset_exact_v2(public.runtime_deployments,public.runtime_deployments,timestamp with time zone)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_startup_unreserved_projection_exact_v2(bytea,timestamp with time zone)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_startup_unreserved_execute_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_startup_reserved_projection_exact_v2(bytea,text,bigint,bigint,bigint,bigint,timestamp with time zone,public.runtime_certification_operation_terminals_v2)''';
    IF definition IS NULL
        OR pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_unreserved_manifest_function_set_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
    previous_fragment := '    RETURN observed_count = 969';
    next_fragment := '    RETURN observed_count = 972';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_unreserved_manifest_count_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_database_readiness_v1()'
    );
    previous_fragment :=
        '            (''starring_runtime_private_v2.starring_runtime_slot_writer_fence_installation_insert_v2()'')';
    next_fragment :=
        '            (''starring_runtime_private_v2.starring_runtime_slot_writer_fence_installation_insert_v2()''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_unreserved_awaiting_reset_exact_v2(public.runtime_deployments,public.runtime_deployments,timestamp with time zone)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_startup_unreserved_projection_exact_v2(bytea,timestamp with time zone)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_startup_unreserved_execute_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)'')';
    IF definition IS NULL
        OR pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_unreserved_readiness_helper_set_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$extend_security_manifests$;

DO $refresh_manifest$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
BEGIN
    SELECT
        pg_catalog.pg_get_functiondef(function_row.oid),
        pg_catalog.jsonb_build_object(
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
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_schema_manifest_v1()'
    );
    previous_fragment :=
        'ec41d06fbdfce734b673f6e4e7864e428fb153af992c4f3c395a0eb1cd2106a4';
    next_fragment :=
        'dd7a64d16d27a32dde6f80416e4efc444c69aa59e055ff26f8008a2cdc845a62';
    IF definition IS NULL
        OR pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                previous_fragment,
                ''
            ))
            <> pg_catalog.char_length(previous_fragment)
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_unreserved_manifest_precondition_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    SELECT pg_catalog.jsonb_build_object(
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
    INTO metadata_after
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_schema_manifest_v1()'
    );
    IF metadata_after IS DISTINCT FROM metadata_before
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_unreserved_manifest_postcondition_drift';
    END IF;
END;
$refresh_manifest$;

DO $refresh_readiness$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
BEGIN
    SELECT
        pg_catalog.pg_get_functiondef(function_row.oid),
        pg_catalog.jsonb_build_object(
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
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_database_readiness_v1()'
    );
    previous_fragment :=
        '6731f361eb37f170d4cdb91a1c5931101ef6bc2d16c50e1114a452e05b228f7b';
    next_fragment :=
        'ee35572e966037477a9070fef87781e901f0ef49e3cb471acebba9c165657676';
    IF definition IS NULL
        OR pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                previous_fragment,
                ''
            ))
            <> pg_catalog.char_length(previous_fragment)
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_unreserved_readiness_precondition_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    SELECT pg_catalog.jsonb_build_object(
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
    INTO metadata_after
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_database_readiness_v1()'
    );
    IF metadata_after IS DISTINCT FROM metadata_before
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_execution_database_readiness_v1()'
                )
            ),
            next_fragment
        ) = 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_unreserved_readiness_postcondition_drift';
    END IF;
END;
$refresh_readiness$;

DO $postflight$
DECLARE
    common_owner OID;
    invalid_helper_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    SELECT pg_catalog.count(*)
    INTO invalid_helper_count
    FROM (
        VALUES
            (
                'starring_runtime_private_v2.starring_runtime_unreserved_awaiting_reset_exact_v2(public.runtime_deployments,public.runtime_deployments,timestamp with time zone)',
                's'::"char",
                TRUE,
                FALSE,
                'u'::"char"
            ),
            (
                'starring_runtime_private_v2.starring_runtime_startup_unreserved_projection_exact_v2(bytea,timestamp with time zone)',
                'v'::"char",
                TRUE,
                FALSE,
                'u'::"char"
            ),
            (
                'starring_runtime_private_v2.starring_runtime_startup_unreserved_execute_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)',
                'v'::"char",
                TRUE,
                FALSE,
                'u'::"char"
            )
    ) AS expected(
        identity,
        volatility,
        strict,
        security_definer,
        parallel_safety
    )
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid =
            pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.provolatile <> expected.volatility
        OR function_row.proisstrict <> expected.strict
        OR function_row.prosecdef <> expected.security_definer
        OR function_row.proparallel <> expected.parallel_safety
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
        );

    IF invalid_helper_count <> 0
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_execution_database_readiness_v1()'
                    )
                ),
                'UTF8'
            )),
            'hex'
        ) <> '437eef0962f31be61e9fcb2f6705b2cda14f4d52105ae024ca4bc29b967e001c'
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)'
                )
            ),
            'executable_unreserved_awaiting_count'
        ) = 0
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_startup_recovery_execute_reserved_awaiting_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)'
                )
            ),
            'starring_runtime_startup_unreserved_execute_v2'
        ) = 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_unreserved_awaiting_postflight_drift';
    END IF;
END;
$postflight$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
