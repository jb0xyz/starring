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

    SELECT
        CASE
            WHEN pg_catalog.to_regclass(
                'public.runtime_startup_recovery_actions_v2'
            ) IS NULL
            THEN 0
            ELSE 1
        END
        +
        CASE
            WHEN pg_catalog.to_regprocedure(
                'public.reject_runtime_startup_recovery_action_mutation_v2()'
            ) IS NULL
            THEN 0
            ELSE 1
        END
        +
        CASE
            WHEN pg_catalog.to_regprocedure(
                'starring_runtime_private_v2.starring_runtime_startup_recovery_terminal_digest_v2(smallint,text,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,timestamp with time zone,bytea)'
            ) IS NULL
            THEN 0
            ELSE 1
        END
        +
        CASE
            WHEN pg_catalog.to_regprocedure(
                'starring_runtime_private_v2.starring_runtime_startup_recovery_action_record_v2(text,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,bytea)'
            ) IS NULL
            THEN 0
            ELSE 1
        END
    INTO collision_count;

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
            '2e55bd05bb77a1dcc5a4f02efd0b221f2fa085fb92e7da7f97d29408022f0eb3'
        OR readiness_digest IS DISTINCT FROM
            '9acd85e2162d4c06593dedae7d2043e53bebc8cd1d70c7aea5aa364cec0cb27f'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_recovery_action_journal_preflight_drift';
    END IF;
END;
$preflight$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_startup_recovery_terminal_digest_v2(
    requested_record_format_version SMALLINT,
    requested_recovery_id TEXT,
    requested_originating_emergency_generation BIGINT,
    requested_coordinator_generation BIGINT,
    requested_action_authority_revision BIGINT,
    requested_selection_authority_revision BIGINT,
    requested_recovery_class TEXT,
    requested_gateway_shard_id TEXT,
    requested_owner_process_instance_id TEXT,
    requested_owner_lease_epoch BIGINT,
    requested_owner_runtime_build_revision TEXT,
    requested_owner_revision BIGINT,
    requested_owner_expires_at TIMESTAMPTZ,
    requested_minimum_database_now TIMESTAMPTZ,
    requested_recorded_at TIMESTAMPTZ,
    terminal_projection_bytes BYTEA
)
RETURNS TEXT
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    recovery_id_bytes BYTEA;
    recovery_class_bytes BYTEA;
    gateway_shard_id_bytes BYTEA;
    owner_process_instance_id_bytes BYTEA;
    owner_runtime_build_revision_bytes BYTEA;
    canonical_payload BYTEA;
BEGIN
    recovery_id_bytes := pg_catalog.convert_to(
        requested_recovery_id,
        'UTF8'
    );
    recovery_class_bytes := pg_catalog.convert_to(
        requested_recovery_class,
        'UTF8'
    );
    gateway_shard_id_bytes := pg_catalog.convert_to(
        requested_gateway_shard_id,
        'UTF8'
    );
    owner_process_instance_id_bytes := pg_catalog.convert_to(
        requested_owner_process_instance_id,
        'UTF8'
    );
    owner_runtime_build_revision_bytes := pg_catalog.convert_to(
        requested_owner_runtime_build_revision,
        'UTF8'
    );
    canonical_payload :=
        pg_catalog.int2send(requested_record_format_version)
        || pg_catalog.int8send(
            pg_catalog.octet_length(recovery_id_bytes)::BIGINT
        )
        || recovery_id_bytes
        || pg_catalog.int8send(
            requested_originating_emergency_generation
        )
        || pg_catalog.int8send(requested_coordinator_generation)
        || pg_catalog.int8send(requested_action_authority_revision)
        || pg_catalog.int8send(requested_selection_authority_revision)
        || pg_catalog.int8send(
            pg_catalog.octet_length(recovery_class_bytes)::BIGINT
        )
        || recovery_class_bytes
        || pg_catalog.int8send(
            pg_catalog.octet_length(gateway_shard_id_bytes)::BIGINT
        )
        || gateway_shard_id_bytes
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                owner_process_instance_id_bytes
            )::BIGINT
        )
        || owner_process_instance_id_bytes
        || pg_catalog.int8send(requested_owner_lease_epoch)
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                owner_runtime_build_revision_bytes
            )::BIGINT
        )
        || owner_runtime_build_revision_bytes
        || pg_catalog.int8send(requested_owner_revision)
        || pg_catalog.timestamptz_send(requested_owner_expires_at)
        || pg_catalog.timestamptz_send(requested_minimum_database_now)
        || pg_catalog.timestamptz_send(requested_recorded_at)
        || pg_catalog.int8send(
            pg_catalog.octet_length(terminal_projection_bytes)::BIGINT
        )
        || terminal_projection_bytes;

    RETURN starring_runtime_private_v2.starring_runtime_framed_digest_v2(
        pg_catalog.convert_to(
            'starring.runtime.startup_recovery.action_proof.v2',
            'UTF8'
        ) || pg_catalog.decode('00', 'hex'),
        canonical_payload
    );
END;
$function$;

CREATE TABLE public.runtime_startup_recovery_actions_v2 (
    record_format_version SMALLINT NOT NULL,
    recovery_id TEXT NOT NULL,
    originating_emergency_generation BIGINT NOT NULL,
    coordinator_generation BIGINT NOT NULL,
    action_authority_revision BIGINT NOT NULL,
    selection_authority_revision BIGINT NOT NULL,
    recovery_class TEXT NOT NULL,
    gateway_shard_id TEXT NOT NULL,
    owner_process_instance_id TEXT NOT NULL,
    owner_lease_epoch BIGINT NOT NULL,
    owner_runtime_build_revision TEXT NOT NULL,
    owner_revision BIGINT NOT NULL,
    owner_expires_at TIMESTAMPTZ NOT NULL,
    minimum_database_now TIMESTAMPTZ NOT NULL,
    terminal_projection_bytes BYTEA NOT NULL,
    terminal_digest TEXT NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT runtime_startup_recovery_actions_v2_primary_key PRIMARY KEY (
        recovery_id,
        selection_authority_revision
    ),
    CONSTRAINT runtime_startup_recovery_actions_v2_action_unique UNIQUE (
        recovery_id,
        action_authority_revision
    ),
    CONSTRAINT runtime_startup_recovery_actions_v2_format_check CHECK (
        record_format_version = 2
    ),
    CONSTRAINT runtime_startup_recovery_actions_v2_identity_check CHECK (
        recovery_id ~ '^[0-9a-f]{32}$'
        AND originating_emergency_generation
            BETWEEN 1 AND 9223372036854775807
        AND coordinator_generation
            BETWEEN 1 AND 9223372036854775807
        AND selection_authority_revision
            BETWEEN 1 AND 9223372036854775806
        AND action_authority_revision::NUMERIC
            = selection_authority_revision::NUMERIC + 1
    ),
    CONSTRAINT runtime_startup_recovery_actions_v2_class_check CHECK (
        recovery_class IN (
            'stale_live',
            'reserved_awaiting_certification',
            'suspended_local_effect',
            'pending_runtime_drain_intent'
        )
    ),
    CONSTRAINT runtime_startup_recovery_actions_v2_owner_check CHECK (
        gateway_shard_id = 'shard:0'
        AND owner_process_instance_id
            ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND owner_lease_epoch BETWEEN 1 AND 9223372036854775807
        AND owner_runtime_build_revision
            ~ '^[A-Za-z0-9_.:/-]{1,128}$'
        AND owner_revision BETWEEN 1 AND 9223372036854775807
    ),
    CONSTRAINT runtime_startup_recovery_actions_v2_time_check CHECK (
        pg_catalog.isfinite(owner_expires_at)
        AND pg_catalog.isfinite(minimum_database_now)
        AND pg_catalog.isfinite(recorded_at)
        AND minimum_database_now <= recorded_at
        AND recorded_at < owner_expires_at
    ),
    CONSTRAINT runtime_startup_recovery_actions_v2_terminal_check CHECK (
        pg_catalog.octet_length(terminal_projection_bytes)
            BETWEEN 1 AND 131072
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
    )
);

CREATE INDEX runtime_startup_recovery_actions_v2_owner_history_index
ON public.runtime_startup_recovery_actions_v2 (
    gateway_shard_id,
    owner_lease_epoch,
    recorded_at,
    recovery_id,
    selection_authority_revision
);

CREATE FUNCTION public.reject_runtime_startup_recovery_action_mutation_v2()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    gate_valid BOOLEAN;
    setting_name TEXT;
BEGIN
    IF TG_OP <> 'INSERT' THEN
        FOREACH setting_name IN ARRAY ARRAY[
            'starring.runtime_startup_recovery_action_gate_v2',
            'starring.runtime_startup_recovery_action_format_v2',
            'starring.runtime_startup_recovery_action_recovery_id_v2',
            'starring.runtime_startup_recovery_action_origin_generation_v2',
            'starring.runtime_startup_recovery_action_coordinator_generation_v2',
            'starring.runtime_startup_recovery_action_authority_revision_v2',
            'starring.runtime_startup_recovery_action_selection_revision_v2',
            'starring.runtime_startup_recovery_action_class_v2',
            'starring.runtime_startup_recovery_action_gateway_shard_v2',
            'starring.runtime_startup_recovery_action_owner_process_v2',
            'starring.runtime_startup_recovery_action_owner_lease_epoch_v2',
            'starring.runtime_startup_recovery_action_owner_build_v2',
            'starring.runtime_startup_recovery_action_owner_revision_v2',
            'starring.runtime_startup_recovery_action_owner_expires_v2',
            'starring.runtime_startup_recovery_action_minimum_database_now_v2',
            'starring.runtime_startup_recovery_action_terminal_digest_v2',
            'starring.runtime_startup_recovery_action_recorded_at_v2'
        ]
        LOOP
            PERFORM pg_catalog.set_config(setting_name, '', TRUE);
        END LOOP;
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE = 'runtime_startup_recovery_action_mutation_rejected';
    END IF;

    gate_valid :=
        COALESCE(pg_catalog.current_setting(
            'starring.runtime_startup_recovery_action_gate_v2',
            TRUE
        ), '') = 'insert'
        AND COALESCE(pg_catalog.current_setting(
            'starring.runtime_startup_recovery_action_format_v2',
            TRUE
        ), '') IS NOT DISTINCT FROM NEW.record_format_version::TEXT
        AND COALESCE(pg_catalog.current_setting(
            'starring.runtime_startup_recovery_action_recovery_id_v2',
            TRUE
        ), '') IS NOT DISTINCT FROM NEW.recovery_id
        AND COALESCE(pg_catalog.current_setting(
            'starring.runtime_startup_recovery_action_origin_generation_v2',
            TRUE
        ), '') IS NOT DISTINCT FROM
            NEW.originating_emergency_generation::TEXT
        AND COALESCE(pg_catalog.current_setting(
            'starring.runtime_startup_recovery_action_coordinator_generation_v2',
            TRUE
        ), '') IS NOT DISTINCT FROM NEW.coordinator_generation::TEXT
        AND COALESCE(pg_catalog.current_setting(
            'starring.runtime_startup_recovery_action_authority_revision_v2',
            TRUE
        ), '') IS NOT DISTINCT FROM NEW.action_authority_revision::TEXT
        AND COALESCE(pg_catalog.current_setting(
            'starring.runtime_startup_recovery_action_selection_revision_v2',
            TRUE
        ), '') IS NOT DISTINCT FROM
            NEW.selection_authority_revision::TEXT
        AND COALESCE(pg_catalog.current_setting(
            'starring.runtime_startup_recovery_action_class_v2',
            TRUE
        ), '') IS NOT DISTINCT FROM NEW.recovery_class
        AND COALESCE(pg_catalog.current_setting(
            'starring.runtime_startup_recovery_action_gateway_shard_v2',
            TRUE
        ), '') IS NOT DISTINCT FROM NEW.gateway_shard_id
        AND COALESCE(pg_catalog.current_setting(
            'starring.runtime_startup_recovery_action_owner_process_v2',
            TRUE
        ), '') IS NOT DISTINCT FROM NEW.owner_process_instance_id
        AND COALESCE(pg_catalog.current_setting(
            'starring.runtime_startup_recovery_action_owner_lease_epoch_v2',
            TRUE
        ), '') IS NOT DISTINCT FROM NEW.owner_lease_epoch::TEXT
        AND COALESCE(pg_catalog.current_setting(
            'starring.runtime_startup_recovery_action_owner_build_v2',
            TRUE
        ), '') IS NOT DISTINCT FROM NEW.owner_runtime_build_revision
        AND COALESCE(pg_catalog.current_setting(
            'starring.runtime_startup_recovery_action_owner_revision_v2',
            TRUE
        ), '') IS NOT DISTINCT FROM NEW.owner_revision::TEXT
        AND COALESCE(pg_catalog.current_setting(
            'starring.runtime_startup_recovery_action_owner_expires_v2',
            TRUE
        ), '') IS NOT DISTINCT FROM pg_catalog.encode(
            pg_catalog.timestamptz_send(NEW.owner_expires_at),
            'hex'
        )
        AND COALESCE(pg_catalog.current_setting(
            'starring.runtime_startup_recovery_action_minimum_database_now_v2',
            TRUE
        ), '') IS NOT DISTINCT FROM pg_catalog.encode(
            pg_catalog.timestamptz_send(NEW.minimum_database_now),
            'hex'
        )
        AND COALESCE(pg_catalog.current_setting(
            'starring.runtime_startup_recovery_action_terminal_digest_v2',
            TRUE
        ), '') IS NOT DISTINCT FROM NEW.terminal_digest
        AND COALESCE(pg_catalog.current_setting(
            'starring.runtime_startup_recovery_action_recorded_at_v2',
            TRUE
        ), '') IS NOT DISTINCT FROM pg_catalog.encode(
            pg_catalog.timestamptz_send(NEW.recorded_at),
            'hex'
        );

    FOREACH setting_name IN ARRAY ARRAY[
        'starring.runtime_startup_recovery_action_gate_v2',
        'starring.runtime_startup_recovery_action_format_v2',
        'starring.runtime_startup_recovery_action_recovery_id_v2',
        'starring.runtime_startup_recovery_action_origin_generation_v2',
        'starring.runtime_startup_recovery_action_coordinator_generation_v2',
        'starring.runtime_startup_recovery_action_authority_revision_v2',
        'starring.runtime_startup_recovery_action_selection_revision_v2',
        'starring.runtime_startup_recovery_action_class_v2',
        'starring.runtime_startup_recovery_action_gateway_shard_v2',
        'starring.runtime_startup_recovery_action_owner_process_v2',
        'starring.runtime_startup_recovery_action_owner_lease_epoch_v2',
        'starring.runtime_startup_recovery_action_owner_build_v2',
        'starring.runtime_startup_recovery_action_owner_revision_v2',
        'starring.runtime_startup_recovery_action_owner_expires_v2',
        'starring.runtime_startup_recovery_action_minimum_database_now_v2',
        'starring.runtime_startup_recovery_action_terminal_digest_v2',
        'starring.runtime_startup_recovery_action_recorded_at_v2'
    ]
    LOOP
        PERFORM pg_catalog.set_config(setting_name, '', TRUE);
    END LOOP;

    IF NOT gate_valid THEN
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE = 'runtime_startup_recovery_action_mutation_rejected';
    END IF;

    RETURN NEW;
END;
$function$;

CREATE TRIGGER runtime_startup_recovery_actions_v2_reject_row_mutation
BEFORE INSERT OR UPDATE OR DELETE
ON public.runtime_startup_recovery_actions_v2
FOR EACH ROW
EXECUTE FUNCTION public.reject_runtime_startup_recovery_action_mutation_v2();

CREATE TRIGGER runtime_startup_recovery_actions_v2_reject_truncate
BEFORE TRUNCATE
ON public.runtime_startup_recovery_actions_v2
FOR EACH STATEMENT
EXECUTE FUNCTION public.reject_runtime_startup_recovery_action_mutation_v2();

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_startup_recovery_action_record_v2(
    requested_recovery_id TEXT,
    requested_originating_emergency_generation BIGINT,
    requested_coordinator_generation BIGINT,
    requested_action_authority_revision BIGINT,
    requested_selection_authority_revision BIGINT,
    requested_recovery_class TEXT,
    expected_gateway_shard_id TEXT,
    expected_owner_process_instance_id TEXT,
    expected_owner_lease_epoch BIGINT,
    expected_owner_runtime_build_revision TEXT,
    expected_owner_revision BIGINT,
    expected_owner_expires_at TIMESTAMPTZ,
    minimum_database_now TIMESTAMPTZ,
    database_terminal_projection_bytes BYTEA
)
RETURNS TABLE(
    outcome_name TEXT,
    observed_gateway_shard_id TEXT,
    observed_process_instance_id TEXT,
    observed_lease_epoch BIGINT,
    observed_runtime_build_revision TEXT,
    observed_owner_revision BIGINT,
    database_now TIMESTAMPTZ,
    observed_owner_expires_at TIMESTAMPTZ,
    recorded_at TIMESTAMPTZ,
    terminal_digest TEXT
)
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY INVOKER
SET search_path = pg_catalog
ROWS 1
AS $function$
DECLARE
    owner_row public.runtime_gateway_owners%ROWTYPE;
    selection_row public.runtime_startup_recovery_actions_v2%ROWTYPE;
    action_row public.runtime_startup_recovery_actions_v2%ROWTYPE;
    existing_row public.runtime_startup_recovery_actions_v2%ROWTYPE;
    latest_row public.runtime_startup_recovery_actions_v2%ROWTYPE;
    inserted_row public.runtime_startup_recovery_actions_v2%ROWTYPE;
    selection_found BOOLEAN;
    action_found BOOLEAN;
    latest_found BOOLEAN;
    derived_terminal_digest TEXT;
    setting_name TEXT;
BEGIN
    IF pg_catalog.current_setting('transaction_isolation')
            <> 'serializable'
        OR pg_catalog.current_setting('transaction_read_only') <> 'off'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_recovery_action_transaction_invalid';
    END IF;

    IF requested_selection_authority_revision
            NOT BETWEEN 1 AND 9223372036854775806
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_startup_recovery_action_input_invalid';
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
        OR requested_recovery_class NOT IN (
            'stale_live',
            'reserved_awaiting_certification',
            'suspended_local_effect',
            'pending_runtime_drain_intent'
        )
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
        OR NOT pg_catalog.isfinite(minimum_database_now)
        OR minimum_database_now >= expected_owner_expires_at
        OR pg_catalog.octet_length(database_terminal_projection_bytes)
            NOT BETWEEN 1 AND 131072
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_startup_recovery_action_input_invalid';
    END IF;

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
            MESSAGE = 'runtime_startup_recovery_action_owner_lost';
    END IF;
    IF database_now < minimum_database_now THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_recovery_action_database_clock_regressed';
    END IF;

    SELECT action.*
    INTO selection_row
    FROM public.runtime_startup_recovery_actions_v2 AS action
    WHERE action.recovery_id = requested_recovery_id
        AND action.selection_authority_revision
            = requested_selection_authority_revision
    FOR UPDATE;
    selection_found := FOUND;

    SELECT action.*
    INTO action_row
    FROM public.runtime_startup_recovery_actions_v2 AS action
    WHERE action.recovery_id = requested_recovery_id
        AND action.action_authority_revision
            = requested_action_authority_revision
    FOR UPDATE;
    action_found := FOUND;

    IF selection_found OR action_found THEN
        IF selection_found
            AND action_found
            AND selection_row.selection_authority_revision
                IS DISTINCT FROM action_row.selection_authority_revision
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX003',
                MESSAGE = 'runtime_startup_recovery_action_identity_conflict';
        END IF;
        IF selection_found THEN
            existing_row := selection_row;
        ELSE
            existing_row := action_row;
        END IF;

        IF existing_row.record_format_version IS DISTINCT FROM 2
            OR existing_row.recovery_id
                IS DISTINCT FROM requested_recovery_id
            OR existing_row.originating_emergency_generation
                IS DISTINCT FROM
                    requested_originating_emergency_generation
            OR existing_row.coordinator_generation
                IS DISTINCT FROM requested_coordinator_generation
            OR existing_row.action_authority_revision
                IS DISTINCT FROM requested_action_authority_revision
            OR existing_row.selection_authority_revision
                IS DISTINCT FROM requested_selection_authority_revision
            OR existing_row.recovery_class
                IS DISTINCT FROM requested_recovery_class
            OR existing_row.gateway_shard_id
                IS DISTINCT FROM expected_gateway_shard_id
            OR existing_row.owner_process_instance_id
                IS DISTINCT FROM expected_owner_process_instance_id
            OR existing_row.owner_lease_epoch
                IS DISTINCT FROM expected_owner_lease_epoch
            OR existing_row.owner_runtime_build_revision
                IS DISTINCT FROM expected_owner_runtime_build_revision
            OR existing_row.owner_revision
                IS DISTINCT FROM expected_owner_revision
            OR existing_row.owner_expires_at
                IS DISTINCT FROM expected_owner_expires_at
            OR existing_row.minimum_database_now
                IS DISTINCT FROM minimum_database_now
            OR existing_row.terminal_projection_bytes
                IS DISTINCT FROM database_terminal_projection_bytes
            OR existing_row.terminal_digest IS DISTINCT FROM
                starring_runtime_private_v2.starring_runtime_startup_recovery_terminal_digest_v2(
                    existing_row.record_format_version,
                    existing_row.recovery_id,
                    existing_row.originating_emergency_generation,
                    existing_row.coordinator_generation,
                    existing_row.action_authority_revision,
                    existing_row.selection_authority_revision,
                    existing_row.recovery_class,
                    existing_row.gateway_shard_id,
                    existing_row.owner_process_instance_id,
                    existing_row.owner_lease_epoch,
                    existing_row.owner_runtime_build_revision,
                    existing_row.owner_revision,
                    existing_row.owner_expires_at,
                    existing_row.minimum_database_now,
                    existing_row.recorded_at,
                    existing_row.terminal_projection_bytes
                )
            OR NOT pg_catalog.isfinite(existing_row.recorded_at)
            OR existing_row.minimum_database_now > existing_row.recorded_at
            OR existing_row.recorded_at >= existing_row.owner_expires_at
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX003',
                MESSAGE = 'runtime_startup_recovery_action_replay_mismatch';
        END IF;
        IF database_now < existing_row.recorded_at THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_startup_recovery_action_database_clock_regressed';
        END IF;

        outcome_name := 'replayed';
        observed_gateway_shard_id := owner_row.gateway_shard_id;
        observed_process_instance_id := owner_row.process_instance_id;
        observed_lease_epoch := owner_row.lease_epoch;
        observed_runtime_build_revision :=
            owner_row.expected_build_revision;
        observed_owner_revision := owner_row.owner_revision;
        observed_owner_expires_at := owner_row.expires_at;
        recorded_at := existing_row.recorded_at;
        terminal_digest := existing_row.terminal_digest;
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT action.*
    INTO latest_row
    FROM public.runtime_startup_recovery_actions_v2 AS action
    WHERE action.recovery_id = requested_recovery_id
    ORDER BY action.selection_authority_revision DESC
    LIMIT 1
    FOR UPDATE;
    latest_found := FOUND;

    IF latest_found
        AND (
            latest_row.originating_emergency_generation
                IS DISTINCT FROM
                    requested_originating_emergency_generation
            OR latest_row.coordinator_generation
                IS DISTINCT FROM requested_coordinator_generation
            OR latest_row.gateway_shard_id
                IS DISTINCT FROM expected_gateway_shard_id
            OR latest_row.owner_process_instance_id
                IS DISTINCT FROM expected_owner_process_instance_id
            OR latest_row.owner_lease_epoch
                IS DISTINCT FROM expected_owner_lease_epoch
            OR latest_row.owner_runtime_build_revision
                IS DISTINCT FROM expected_owner_runtime_build_revision
            OR requested_selection_authority_revision
                <= latest_row.selection_authority_revision
            OR requested_action_authority_revision
                <= latest_row.action_authority_revision
            OR expected_owner_revision < latest_row.owner_revision
            OR (
                expected_owner_revision = latest_row.owner_revision
                AND expected_owner_expires_at
                    IS DISTINCT FROM latest_row.owner_expires_at
            )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_startup_recovery_action_sequence_conflict';
    END IF;
    IF latest_found
        AND (
            database_now < latest_row.recorded_at
            OR minimum_database_now < latest_row.recorded_at
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_recovery_action_database_clock_regressed';
    END IF;

    derived_terminal_digest :=
        starring_runtime_private_v2.starring_runtime_startup_recovery_terminal_digest_v2(
            2::SMALLINT,
            requested_recovery_id,
            requested_originating_emergency_generation,
            requested_coordinator_generation,
            requested_action_authority_revision,
            requested_selection_authority_revision,
            requested_recovery_class,
            expected_gateway_shard_id,
            expected_owner_process_instance_id,
            expected_owner_lease_epoch,
            expected_owner_runtime_build_revision,
            expected_owner_revision,
            expected_owner_expires_at,
            minimum_database_now,
            database_now,
            database_terminal_projection_bytes
        );

    BEGIN
        PERFORM pg_catalog.set_config(
            'starring.runtime_startup_recovery_action_gate_v2',
            'insert',
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_startup_recovery_action_format_v2',
            '2',
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_startup_recovery_action_recovery_id_v2',
            requested_recovery_id,
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_startup_recovery_action_origin_generation_v2',
            requested_originating_emergency_generation::TEXT,
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_startup_recovery_action_coordinator_generation_v2',
            requested_coordinator_generation::TEXT,
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_startup_recovery_action_authority_revision_v2',
            requested_action_authority_revision::TEXT,
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_startup_recovery_action_selection_revision_v2',
            requested_selection_authority_revision::TEXT,
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_startup_recovery_action_class_v2',
            requested_recovery_class,
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_startup_recovery_action_gateway_shard_v2',
            expected_gateway_shard_id,
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_startup_recovery_action_owner_process_v2',
            expected_owner_process_instance_id,
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_startup_recovery_action_owner_lease_epoch_v2',
            expected_owner_lease_epoch::TEXT,
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_startup_recovery_action_owner_build_v2',
            expected_owner_runtime_build_revision,
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_startup_recovery_action_owner_revision_v2',
            expected_owner_revision::TEXT,
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_startup_recovery_action_owner_expires_v2',
            pg_catalog.encode(
                pg_catalog.timestamptz_send(expected_owner_expires_at),
                'hex'
            ),
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_startup_recovery_action_minimum_database_now_v2',
            pg_catalog.encode(
                pg_catalog.timestamptz_send(minimum_database_now),
                'hex'
            ),
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_startup_recovery_action_terminal_digest_v2',
            derived_terminal_digest,
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_startup_recovery_action_recorded_at_v2',
            pg_catalog.encode(
                pg_catalog.timestamptz_send(database_now),
                'hex'
            ),
            TRUE
        );

        INSERT INTO public.runtime_startup_recovery_actions_v2 (
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
            terminal_projection_bytes,
            terminal_digest,
            recorded_at
        ) VALUES (
            2,
            requested_recovery_id,
            requested_originating_emergency_generation,
            requested_coordinator_generation,
            requested_action_authority_revision,
            requested_selection_authority_revision,
            requested_recovery_class,
            expected_gateway_shard_id,
            expected_owner_process_instance_id,
            expected_owner_lease_epoch,
            expected_owner_runtime_build_revision,
            expected_owner_revision,
            expected_owner_expires_at,
            minimum_database_now,
            database_terminal_projection_bytes,
            derived_terminal_digest,
            database_now
        )
        RETURNING * INTO inserted_row;
    EXCEPTION
        WHEN unique_violation THEN
            FOREACH setting_name IN ARRAY ARRAY[
                'starring.runtime_startup_recovery_action_gate_v2',
                'starring.runtime_startup_recovery_action_format_v2',
                'starring.runtime_startup_recovery_action_recovery_id_v2',
                'starring.runtime_startup_recovery_action_origin_generation_v2',
                'starring.runtime_startup_recovery_action_coordinator_generation_v2',
                'starring.runtime_startup_recovery_action_authority_revision_v2',
                'starring.runtime_startup_recovery_action_selection_revision_v2',
                'starring.runtime_startup_recovery_action_class_v2',
                'starring.runtime_startup_recovery_action_gateway_shard_v2',
                'starring.runtime_startup_recovery_action_owner_process_v2',
                'starring.runtime_startup_recovery_action_owner_lease_epoch_v2',
                'starring.runtime_startup_recovery_action_owner_build_v2',
                'starring.runtime_startup_recovery_action_owner_revision_v2',
                'starring.runtime_startup_recovery_action_owner_expires_v2',
                'starring.runtime_startup_recovery_action_minimum_database_now_v2',
                'starring.runtime_startup_recovery_action_terminal_digest_v2',
                'starring.runtime_startup_recovery_action_recorded_at_v2'
            ]
            LOOP
                PERFORM pg_catalog.set_config(setting_name, '', TRUE);
            END LOOP;
            RAISE EXCEPTION USING
                ERRCODE = 'RX003',
                MESSAGE = 'runtime_startup_recovery_action_identity_conflict';
        WHEN OTHERS THEN
            FOREACH setting_name IN ARRAY ARRAY[
                'starring.runtime_startup_recovery_action_gate_v2',
                'starring.runtime_startup_recovery_action_format_v2',
                'starring.runtime_startup_recovery_action_recovery_id_v2',
                'starring.runtime_startup_recovery_action_origin_generation_v2',
                'starring.runtime_startup_recovery_action_coordinator_generation_v2',
                'starring.runtime_startup_recovery_action_authority_revision_v2',
                'starring.runtime_startup_recovery_action_selection_revision_v2',
                'starring.runtime_startup_recovery_action_class_v2',
                'starring.runtime_startup_recovery_action_gateway_shard_v2',
                'starring.runtime_startup_recovery_action_owner_process_v2',
                'starring.runtime_startup_recovery_action_owner_lease_epoch_v2',
                'starring.runtime_startup_recovery_action_owner_build_v2',
                'starring.runtime_startup_recovery_action_owner_revision_v2',
                'starring.runtime_startup_recovery_action_owner_expires_v2',
                'starring.runtime_startup_recovery_action_minimum_database_now_v2',
                'starring.runtime_startup_recovery_action_terminal_digest_v2',
                'starring.runtime_startup_recovery_action_recorded_at_v2'
            ]
            LOOP
                PERFORM pg_catalog.set_config(setting_name, '', TRUE);
            END LOOP;
            RAISE;
    END;

    outcome_name := 'applied';
    observed_gateway_shard_id := owner_row.gateway_shard_id;
    observed_process_instance_id := owner_row.process_instance_id;
    observed_lease_epoch := owner_row.lease_epoch;
    observed_runtime_build_revision := owner_row.expected_build_revision;
    observed_owner_revision := owner_row.owner_revision;
    observed_owner_expires_at := owner_row.expires_at;
    recorded_at := inserted_row.recorded_at;
    terminal_digest := inserted_row.terminal_digest;
    RETURN NEXT;
END;
$function$;

REVOKE ALL ON TABLE
    public.runtime_startup_recovery_actions_v2
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.reject_runtime_startup_recovery_action_mutation_v2()
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    starring_runtime_private_v2.starring_runtime_startup_recovery_terminal_digest_v2(
        SMALLINT,
        TEXT,
        BIGINT,
        BIGINT,
        BIGINT,
        BIGINT,
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        BIGINT,
        TIMESTAMPTZ,
        TIMESTAMPTZ,
        TIMESTAMPTZ,
        BYTEA
    )
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    starring_runtime_private_v2.starring_runtime_startup_recovery_action_record_v2(
        TEXT,
        BIGINT,
        BIGINT,
        BIGINT,
        BIGINT,
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        BIGINT,
        TIMESTAMPTZ,
        TIMESTAMPTZ,
        BYTEA
    )
FROM PUBLIC;

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
        '            (pg_catalog.to_regclass(''public.runtime_suspend_attempt_completions_v2'')),';
    next_fragment := previous_fragment || E'\n' ||
        '            (pg_catalog.to_regclass(''public.runtime_startup_recovery_actions_v2'')),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_recovery_action_journal_manifest_relation_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint)''' || E'\n' ||
        '        )';
    next_fragment :=
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_startup_recovery_terminal_digest_v2(smallint,text,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,timestamp with time zone,bytea)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_startup_recovery_action_record_v2(text,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,bytea)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint)''' || E'\n' ||
        '        )';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_recovery_action_journal_manifest_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        'RETURN observed_count = 734' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''0144d12c7fd78a3f7ad75670e255a1cff2c0ba11cf613f10006cfcbc5528dcc9'';';
    next_fragment :=
        'RETURN observed_count = 768' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''e9fbf54f755c1a5ac234c69eea4252361146b69c032b655270e7306ea929e175'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_recovery_action_journal_manifest_expectation_patch_drift';
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
        '            (''public.runtime_suspend_attempt_completions_v2''),';
    next_fragment := previous_fragment || E'\n' ||
        '            (''public.runtime_startup_recovery_actions_v2''),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_recovery_action_journal_readiness_relation_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            (''public.reject_runtime_suspend_attempt_ledger_mutation_v2()''),' || E'\n' ||
        '            (''public.validate_runtime_suspend_attempt_ledger_v2()''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint)'')';
    next_fragment :=
        '            (''public.reject_runtime_suspend_attempt_ledger_mutation_v2()''),' || E'\n' ||
        '            (''public.validate_runtime_suspend_attempt_ledger_v2()''),' || E'\n' ||
        '            (''public.reject_runtime_startup_recovery_action_mutation_v2()''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_startup_recovery_terminal_digest_v2(smallint,text,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,timestamp with time zone,bytea)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_startup_recovery_action_record_v2(text,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,bytea)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint)'')';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_recovery_action_journal_readiness_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '''2e55bd05bb77a1dcc5a4f02efd0b221f2fa085fb92e7da7f97d29408022f0eb3''::TEXT';
    next_fragment :=
        '''c76a82cdd88a75259889d4cab4543797ad834d8f2e38f71268bbbc4b0e4cae0f''::TEXT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_recovery_action_journal_readiness_manifest_digest_patch_drift';
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
    invalid_relation_count BIGINT;
    invalid_function_count BIGINT;
    invalid_acl_count BIGINT;
    invalid_trigger_count BIGINT;
    invalid_constraint_count BIGINT;
    invalid_index_count BIGINT;
    manifest_digest TEXT;
    readiness_digest TEXT;
    setting_name TEXT;
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

    SELECT pg_catalog.count(*)
    INTO invalid_relation_count
    FROM (
        VALUES ('public.runtime_startup_recovery_actions_v2')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = pg_catalog.to_regclass(expected.identity)
    WHERE relation.oid IS NULL
        OR relation.relkind <> 'r'
        OR relation.relpersistence <> 'p'
        OR relation.relowner <> common_owner
        OR relation.relrowsecurity
        OR relation.relforcerowsecurity
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                relation.relacl,
                pg_catalog.acldefault('r', relation.relowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
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
        );

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.reject_runtime_startup_recovery_action_mutation_v2()',
                'plpgsql',
                'v',
                FALSE,
                'u',
                TRUE,
                FALSE,
                0::REAL,
                'bfd41de11bd4ae9b4b0a1fc98e624266541e6de4c23c802f980a6e0b04ab98f6'
            ),
            (
                'starring_runtime_private_v2.starring_runtime_startup_recovery_terminal_digest_v2(smallint,text,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,timestamp with time zone,bytea)',
                'plpgsql',
                'i',
                TRUE,
                's',
                FALSE,
                FALSE,
                0::REAL,
                'ce512e6b57535d4bc45d7b7c7b056905be5775e418987d2ef79f62b8c05feb41'
            ),
            (
                'starring_runtime_private_v2.starring_runtime_startup_recovery_action_record_v2(text,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,bytea)',
                'plpgsql',
                'v',
                TRUE,
                'u',
                FALSE,
                TRUE,
                1::REAL,
                'bead9e18b19984a20070ee4b739f0fa7aaebb87d07a03913af17dd8b4b5b24b4'
            )
    ) AS expected(
        identity,
        language_name,
        volatility,
        is_strict,
        parallel_safety,
        security_definer,
        returns_set,
        rows_estimate,
        definition_digest
    )
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR language_row.lanname IS DISTINCT FROM expected.language_name
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> expected.volatility
        OR function_row.proisstrict IS DISTINCT FROM expected.is_strict
        OR function_row.proparallel <> expected.parallel_safety
        OR function_row.prosecdef IS DISTINCT FROM expected.security_definer
        OR function_row.proretset IS DISTINCT FROM expected.returns_set
        OR function_row.prorows IS DISTINCT FROM expected.rows_estimate
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(function_row.oid),
                'UTF8'
            )),
            'hex'
        ) IS DISTINCT FROM expected.definition_digest;

    SELECT pg_catalog.count(*)
    INTO invalid_acl_count
    FROM (
        VALUES
            ('public.reject_runtime_startup_recovery_action_mutation_v2()'),
            ('starring_runtime_private_v2.starring_runtime_startup_recovery_terminal_digest_v2(smallint,text,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,timestamp with time zone,bytea)'),
            ('starring_runtime_private_v2.starring_runtime_startup_recovery_action_record_v2(text,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,bytea)')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    LEFT JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege ON TRUE
    WHERE function_row.oid IS NULL
        OR privilege.grantee <> common_owner
        OR privilege.grantor <> common_owner
        OR privilege.privilege_type <> 'EXECUTE'
        OR privilege.is_grantable
        OR (
            executor_role IS NOT NULL
            AND pg_catalog.has_function_privilege(
                executor_role,
                function_row.oid,
                'EXECUTE'
            )
        );

    SELECT pg_catalog.count(*)
    INTO invalid_trigger_count
    FROM (
        VALUES
            (
                'runtime_startup_recovery_actions_v2_reject_row_mutation',
                31
            ),
            (
                'runtime_startup_recovery_actions_v2_reject_truncate',
                34
            )
    ) AS expected(trigger_name, trigger_type)
    LEFT JOIN pg_catalog.pg_trigger AS trigger_row
        ON trigger_row.tgrelid = pg_catalog.to_regclass(
            'public.runtime_startup_recovery_actions_v2'
        )
        AND trigger_row.tgname = expected.trigger_name
    WHERE trigger_row.oid IS NULL
        OR trigger_row.tgisinternal
        OR trigger_row.tgenabled <> 'O'
        OR trigger_row.tgtype <> expected.trigger_type
        OR trigger_row.tgfoid <> pg_catalog.to_regprocedure(
            'public.reject_runtime_startup_recovery_action_mutation_v2()'
        );

    SELECT pg_catalog.count(*)
    INTO invalid_constraint_count
    FROM (
        VALUES
            ('runtime_startup_recovery_actions_v2_primary_key', 'p'),
            ('runtime_startup_recovery_actions_v2_action_unique', 'u'),
            ('runtime_startup_recovery_actions_v2_format_check', 'c'),
            ('runtime_startup_recovery_actions_v2_identity_check', 'c'),
            ('runtime_startup_recovery_actions_v2_class_check', 'c'),
            ('runtime_startup_recovery_actions_v2_owner_check', 'c'),
            ('runtime_startup_recovery_actions_v2_time_check', 'c'),
            ('runtime_startup_recovery_actions_v2_terminal_check', 'c')
    ) AS expected(constraint_name, constraint_type)
    LEFT JOIN pg_catalog.pg_constraint AS constraint_row
        ON constraint_row.conrelid = pg_catalog.to_regclass(
            'public.runtime_startup_recovery_actions_v2'
        )
        AND constraint_row.conname = expected.constraint_name
    WHERE constraint_row.oid IS NULL
        OR constraint_row.contype::TEXT <> expected.constraint_type
        OR NOT constraint_row.convalidated
        OR constraint_row.condeferrable
        OR constraint_row.condeferred;

    SELECT pg_catalog.count(*)
    INTO invalid_index_count
    FROM pg_catalog.pg_index AS index_contract
    INNER JOIN pg_catalog.pg_class AS index_row
        ON index_row.oid = index_contract.indexrelid
    WHERE index_contract.indrelid = pg_catalog.to_regclass(
            'public.runtime_startup_recovery_actions_v2'
        )
        AND index_row.relname =
            'runtime_startup_recovery_actions_v2_owner_history_index'
        AND (
            NOT index_contract.indisvalid
            OR NOT index_contract.indisready
            OR NOT index_contract.indislive
            OR index_contract.indisunique
            OR index_contract.indnkeyatts <> 5
        );
    IF invalid_index_count = 0
        AND NOT EXISTS (
            SELECT 1
            FROM pg_catalog.pg_index AS index_contract
            INNER JOIN pg_catalog.pg_class AS index_row
                ON index_row.oid = index_contract.indexrelid
            WHERE index_contract.indrelid = pg_catalog.to_regclass(
                    'public.runtime_startup_recovery_actions_v2'
                )
                AND index_row.relname =
                    'runtime_startup_recovery_actions_v2_owner_history_index'
        )
    THEN
        invalid_index_count := 1;
    END IF;

    FOREACH setting_name IN ARRAY ARRAY[
        'starring.runtime_startup_recovery_action_gate_v2',
        'starring.runtime_startup_recovery_action_format_v2',
        'starring.runtime_startup_recovery_action_recovery_id_v2',
        'starring.runtime_startup_recovery_action_origin_generation_v2',
        'starring.runtime_startup_recovery_action_coordinator_generation_v2',
        'starring.runtime_startup_recovery_action_authority_revision_v2',
        'starring.runtime_startup_recovery_action_selection_revision_v2',
        'starring.runtime_startup_recovery_action_class_v2',
        'starring.runtime_startup_recovery_action_gateway_shard_v2',
        'starring.runtime_startup_recovery_action_owner_process_v2',
        'starring.runtime_startup_recovery_action_owner_lease_epoch_v2',
        'starring.runtime_startup_recovery_action_owner_build_v2',
        'starring.runtime_startup_recovery_action_owner_revision_v2',
        'starring.runtime_startup_recovery_action_owner_expires_v2',
        'starring.runtime_startup_recovery_action_minimum_database_now_v2',
        'starring.runtime_startup_recovery_action_terminal_digest_v2',
        'starring.runtime_startup_recovery_action_recorded_at_v2'
    ]
    LOOP
        IF COALESCE(pg_catalog.current_setting(setting_name, TRUE), '') <> ''
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_startup_recovery_action_journal_gate_drift';
        END IF;
    END LOOP;

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
        OR invalid_relation_count <> 0
        OR invalid_function_count <> 0
        OR invalid_acl_count <> 0
        OR invalid_trigger_count <> 0
        OR invalid_constraint_count <> 0
        OR invalid_index_count <> 0
        OR (
            SELECT pg_catalog.count(*)
            FROM public.runtime_startup_recovery_actions_v2
        ) <> 0
        OR manifest_digest IS DISTINCT FROM
            'c76a82cdd88a75259889d4cab4543797ad834d8f2e38f71268bbbc4b0e4cae0f'
        OR readiness_digest IS DISTINCT FROM
            'ee9364b3bb8b17a3a2386c0be06ae2ab12b519c77647a4073e96f45bfb5084a8'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_recovery_action_journal_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
