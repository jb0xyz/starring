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
    collision_count BIGINT;
    drain_state_constraint_count BIGINT;
    manifest_digest TEXT;
    readiness_digest TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname =
            'starring_runtime_startup_recovery_observe_v2';

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
        OR collision_count <> 0
        OR drain_state_constraint_count <> 1
        OR manifest_digest IS DISTINCT FROM
            '57694b2a5f374fa63882fb52f5bfe506b321968c961ea2cf9de8006fd46a5979'
        OR readiness_digest IS DISTINCT FROM
            '6523d219df9a148c9428ac8f45b9317bcad6b56af44b753f11167fc582ca5875'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_recovery_observation_preflight_drift';
    END IF;
END;
$preflight$;

CREATE FUNCTION public.starring_runtime_startup_recovery_observe_v2(
    expected_gateway_shard_id TEXT,
    expected_process_instance_id TEXT,
    expected_lease_epoch BIGINT,
    expected_runtime_build_revision TEXT,
    expected_owner_revision BIGINT,
    expected_owner_expires_at TIMESTAMPTZ
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
    serving_state_name TEXT,
    serving_count BIGINT,
    serving_earliest_expiry TIMESTAMPTZ,
    serving_retry_after_milliseconds BIGINT,
    recoverable_awaiting_certification_count BIGINT,
    suspended_local_effect_count BIGINT,
    pending_runtime_drain_intent_count BIGINT,
    acknowledged_product_handoff_count BIGINT
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
    owner_found BOOLEAN;
    writer_fence_count BIGINT;
    drain_state_constraint_count BIGINT;
    live_scope_count BIGINT;
    stale_live_count BIGINT;
    foreign_fresh_count BIGINT;
    ambiguous_live_count BIGINT;
    orphan_fresh_count BIGINT;
    earliest_foreign_expiry TIMESTAMPTZ;
    retry_milliseconds NUMERIC;
    reservation_count BIGINT;
    exact_awaiting_reservation_count BIGINT;
    invalid_suspend_attempt_count BIGINT;
    active_exact_route_count BIGINT;
    pending_drain_count BIGINT;
BEGIN
    IF pg_catalog.current_setting('transaction_isolation')
            <> 'serializable'
        OR pg_catalog.current_setting('transaction_read_only') <> 'off'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_recovery_observation_transaction_invalid';
    END IF;

    IF expected_gateway_shard_id IS DISTINCT FROM 'shard:0'
        OR expected_process_instance_id
            !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_lease_epoch
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_runtime_build_revision
            !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR expected_owner_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR NOT pg_catalog.isfinite(expected_owner_expires_at)
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_startup_recovery_observation_input_invalid';
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
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
        public.activation_requests,
        public.authoring_promotions,
        public.product_tenants,
        public.automation_installations,
        public.automation_installation_authority_versions,
        public.automation_ruleset_activations,
        public.automation_ruleset_versions
    IN SHARE MODE;

    database_now := pg_catalog.clock_timestamp();

    SELECT owner.*
    INTO owner_row
    FROM public.runtime_gateway_owners AS owner
    WHERE owner.gateway_shard_id = expected_gateway_shard_id
    FOR UPDATE;
    owner_found := FOUND;

    observed_gateway_shard_id := expected_gateway_shard_id;
    IF owner_found THEN
        observed_process_instance_id := owner_row.process_instance_id;
        observed_lease_epoch := owner_row.lease_epoch;
        observed_runtime_build_revision :=
            owner_row.expected_build_revision;
        observed_owner_revision := owner_row.owner_revision;
        observed_owner_expires_at := owner_row.expires_at;
    END IF;

    IF NOT owner_found
        OR owner_row.process_instance_id
            IS DISTINCT FROM expected_process_instance_id
        OR owner_row.lease_epoch IS DISTINCT FROM expected_lease_epoch
        OR owner_row.expected_build_revision
            IS DISTINCT FROM expected_runtime_build_revision
        OR owner_row.owner_revision
            IS DISTINCT FROM expected_owner_revision
        OR owner_row.expires_at
            IS DISTINCT FROM expected_owner_expires_at
        OR owner_row.expires_at <= database_now
    THEN
        outcome_name := 'not_current';
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT pg_catalog.count(*)
    INTO writer_fence_count
    FROM public.runtime_writer_fence AS fence
    WHERE fence.singleton
        AND fence.fence_state IN ('open', 'closed');

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
        outcome_name := 'ambiguous';
        serving_state_name := 'ambiguous';
        RETURN NEXT;
        RETURN;
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
                    expected_process_instance_id
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
    )
    SELECT
        pg_catalog.count(*),
        pg_catalog.count(*) FILTER (
            WHERE live.is_recoverable_stale
        ),
        pg_catalog.count(*) FILTER (
            WHERE live.is_foreign_fresh
        ),
        pg_catalog.count(*) FILTER (
            WHERE NOT live.is_recoverable_stale
                AND NOT live.is_foreign_fresh
        ),
        pg_catalog.min(live.lease_expires_at) FILTER (
            WHERE live.is_foreign_fresh
        )
    INTO
        live_scope_count,
        stale_live_count,
        foreign_fresh_count,
        ambiguous_live_count,
        earliest_foreign_expiry
    FROM categorized_live AS live;

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
        outcome_name := 'ambiguous';
        serving_state_name := 'ambiguous';
        RETURN NEXT;
        RETURN;
    ELSIF stale_live_count <> 0 THEN
        serving_state_name := 'recoverable_stale';
        serving_count := stale_live_count;
    ELSIF foreign_fresh_count <> 0 THEN
        retry_milliseconds := pg_catalog.floor(
            EXTRACT(
                EPOCH FROM earliest_foreign_expiry - database_now
            ) * 1000
        );
        IF retry_milliseconds < 1 THEN
            outcome_name := 'ambiguous';
            serving_state_name := 'ambiguous';
            serving_count := NULL;
            RETURN NEXT;
            RETURN;
        END IF;
        serving_state_name := 'foreign_fresh';
        serving_count := foreign_fresh_count;
        serving_earliest_expiry := earliest_foreign_expiry;
        serving_retry_after_milliseconds := (
            CASE
                WHEN retry_milliseconds > 1000 THEN 1000
                ELSE retry_milliseconds
            END
        )::BIGINT;
    ELSE
        serving_state_name := 'empty';
        serving_count := 0;
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
        outcome_name := 'ambiguous';
        serving_state_name := 'ambiguous';
        serving_count := NULL;
        serving_earliest_expiry := NULL;
        serving_retry_after_milliseconds := NULL;
        RETURN NEXT;
        RETURN;
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
        outcome_name := 'ambiguous';
        serving_state_name := 'ambiguous';
        serving_count := NULL;
        serving_earliest_expiry := NULL;
        serving_retry_after_milliseconds := NULL;
        RETURN NEXT;
        RETURN;
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
        outcome_name := 'ambiguous';
        serving_state_name := 'ambiguous';
        serving_count := NULL;
        serving_earliest_expiry := NULL;
        serving_retry_after_milliseconds := NULL;
        RETURN NEXT;
        RETURN;
    END IF;

    recoverable_awaiting_certification_count := reservation_count;
    suspended_local_effect_count := active_exact_route_count;
    pending_runtime_drain_intent_count := pending_drain_count;
    acknowledged_product_handoff_count := 0;
    outcome_name := 'observed';
    RETURN NEXT;
END;
$function$;

REVOKE ALL ON FUNCTION
    public.starring_runtime_startup_recovery_observe_v2(
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        BIGINT,
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
            MESSAGE = 'runtime_startup_recovery_observation_execution_acl_drift';
    END IF;

    IF executor_role IS NOT NULL THEN
        executor_name := pg_catalog.pg_get_userbyid(executor_role);
        IF executor_name IS NULL THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_startup_recovery_observation_execution_acl_drift';
        END IF;
        EXECUTE pg_catalog.format(
            'GRANT EXECUTE ON FUNCTION public.starring_runtime_startup_recovery_observe_v2(TEXT,TEXT,BIGINT,TEXT,BIGINT,TIMESTAMPTZ) TO %I',
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
        '            ''public.starring_runtime_certification_reservation_observe_v2(text,text,text,bigint,bigint)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint)''' || E'\n' ||
        '        )';
    next_fragment :=
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_certification_reservation_observe_v2(text,text,text,bigint,bigint)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)''' || E'\n' ||
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
            MESSAGE = 'runtime_startup_recovery_observation_manifest_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        'RETURN observed_count = 733' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''984539f97c292c40c30b262087e312cd423d06c149fb30a4cba6af9596574120'';';
    next_fragment :=
        'RETURN observed_count = 734' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''2b9c978bc17afb7440781c2d5ca50eed37c1ad89986e1f7fe28d2ab5c72fa9b5'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_recovery_observation_manifest_expectation_patch_drift';
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
        '                ''public.starring_runtime_certification_reservation_observe_v2(text,text,text,bigint,bigint)'',' || E'\n' ||
        '                ''expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_deployment_revision bigint, expected_convergence_attempt_no bigint''::TEXT,' || E'\n' ||
        '                ''TABLE(outcome_name text, locked_snapshot jsonb, locked_convergence_attempt_no bigint, observed_at timestamp with time zone, operation_id text, tenant_id text, installation_id text, deployment_id text, deployment_revision bigint, convergence_attempt_no bigint, certification_intent_bytes bytea, intent_fingerprint text)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            )' || E'\n' ||
        '    ) AS expected(';
    next_fragment :=
        '            (' || E'\n' ||
        '                ''public.starring_runtime_certification_reservation_observe_v2(text,text,text,bigint,bigint)'',' || E'\n' ||
        '                ''expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_deployment_revision bigint, expected_convergence_attempt_no bigint''::TEXT,' || E'\n' ||
        '                ''TABLE(outcome_name text, locked_snapshot jsonb, locked_convergence_attempt_no bigint, observed_at timestamp with time zone, operation_id text, tenant_id text, installation_id text, deployment_id text, deployment_revision bigint, convergence_attempt_no bigint, certification_intent_bytes bytea, intent_fingerprint text)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            ),' || E'\n' ||
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
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_recovery_observation_readiness_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '''57694b2a5f374fa63882fb52f5bfe506b321968c961ea2cf9de8006fd46a5979''::TEXT';
    next_fragment :=
        '''94177e2025d87f492e988e3e27b8193b0f7157d4ea7fcd6099308534df9073ff''::TEXT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_recovery_observation_readiness_manifest_digest_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_certification_reservation_observe_v2(text,text,text,bigint,bigint)''' || E'\n' ||
        '            )' || E'\n' ||
        '        )';
    next_fragment :=
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_certification_reservation_observe_v2(text,text,text,bigint,bigint)''' || E'\n' ||
        '            ),' || E'\n' ||
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)''' || E'\n' ||
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
            MESSAGE = 'runtime_startup_recovery_observation_readiness_allowlist_patch_drift';
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
        OR (
            executor_role IS NOT NULL
            AND NOT pg_catalog.has_function_privilege(
                executor_role,
                function_row.oid,
                'EXECUTE'
            )
        )
        OR manifest_digest IS DISTINCT FROM
            '94177e2025d87f492e988e3e27b8193b0f7157d4ea7fcd6099308534df9073ff'
        OR readiness_digest IS DISTINCT FROM
            'ae397ea106f18aa71c6cf2427ebf2705638462066e480b6d0f10b9759a8adc5e'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_recovery_observation_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
