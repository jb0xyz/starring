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
    public.runtime_gateway_owners,
    public.automation_installations,
    public.automation_installation_authority_versions,
    public.automation_ruleset_activations,
    public.automation_ruleset_versions
IN ACCESS EXCLUSIVE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    collision_count BIGINT;
    manifest_digest TEXT;
    readiness_digest TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE (
            namespace.nspname = 'public'
            AND function_row.proname IN (
                'reject_runtime_certification_reservation_mutation_v2',
                'starring_runtime_certification_reserve_intent_v2',
                'starring_runtime_certification_reservation_observe_v2'
            )
        )
        OR (
            namespace.nspname = 'starring_runtime_private_v2'
            AND function_row.proname IN (
                'starring_runtime_certification_intent_bytes_v2',
                'starring_runtime_certification_intent_fingerprint_v2'
            )
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

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR pg_catalog.to_regclass(
            'public.runtime_certification_operations_v2'
        ) IS NOT NULL
        OR collision_count <> 0
        OR manifest_digest IS DISTINCT FROM
            '3c97b3b41f45b11ed2b01890c3d708806d802593f71589031cb921dfc5c65fe3'
        OR readiness_digest IS DISTINCT FROM
            'c5a1eb3ae9a229c127a804f6f05298ff9f797604646de202ba1a832012e7bd91'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_reservation_preflight_drift';
    END IF;
END;
$preflight$;

CREATE TABLE public.runtime_certification_operations_v2 (
    operation_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    installation_id TEXT NOT NULL,
    deployment_id TEXT NOT NULL,
    deployment_revision BIGINT NOT NULL,
    convergence_attempt_no BIGINT NOT NULL,
    certification_intent_bytes BYTEA NOT NULL,
    intent_fingerprint TEXT NOT NULL,
    CONSTRAINT runtime_certification_operations_v2_scope_fk FOREIGN KEY (
        tenant_id,
        installation_id,
        deployment_id
    ) REFERENCES public.runtime_deployments (
        tenant_id,
        installation_id,
        deployment_id
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_certification_operations_v2_natural_unique UNIQUE (
        tenant_id,
        installation_id,
        deployment_id,
        deployment_revision,
        convergence_attempt_no
    ),
    CONSTRAINT runtime_certification_operations_v2_child_unique UNIQUE (
        operation_id,
        intent_fingerprint,
        tenant_id,
        installation_id,
        deployment_id,
        deployment_revision,
        convergence_attempt_no
    ),
    CONSTRAINT runtime_certification_operations_v2_id_check CHECK (
        operation_id ~ '^[0-9a-f]{32}$'
    ),
    CONSTRAINT runtime_certification_operations_v2_scope_check CHECK (
        tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND deployment_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT runtime_certification_operations_v2_revision_check CHECK (
        deployment_revision BETWEEN 1 AND 9223372036854775807
        AND convergence_attempt_no BETWEEN 1 AND 4294967295
    ),
    CONSTRAINT runtime_certification_operations_v2_canonical_check CHECK (
        pg_catalog.octet_length(certification_intent_bytes)
            BETWEEN 1 AND 32768
        AND intent_fingerprint ~ '^[0-9a-f]{64}$'
    )
);

CREATE FUNCTION public.reject_runtime_certification_reservation_mutation_v2()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    setting_name TEXT;
BEGIN
    IF TG_OP <> 'INSERT' THEN
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE = 'runtime_certification_reservation_mutation_rejected';
    END IF;

    IF COALESCE(pg_catalog.current_setting(
            'starring.runtime_certification_reservation_action_v2',
            TRUE
        ), '') <> 'insert'
        OR COALESCE(pg_catalog.current_setting(
            'starring.runtime_certification_reservation_operation_id_v2',
            TRUE
        ), '') IS DISTINCT FROM NEW.operation_id
        OR COALESCE(pg_catalog.current_setting(
            'starring.runtime_certification_reservation_tenant_id_v2',
            TRUE
        ), '') IS DISTINCT FROM NEW.tenant_id
        OR COALESCE(pg_catalog.current_setting(
            'starring.runtime_certification_reservation_installation_id_v2',
            TRUE
        ), '') IS DISTINCT FROM NEW.installation_id
        OR COALESCE(pg_catalog.current_setting(
            'starring.runtime_certification_reservation_deployment_id_v2',
            TRUE
        ), '') IS DISTINCT FROM NEW.deployment_id
        OR COALESCE(pg_catalog.current_setting(
            'starring.runtime_certification_reservation_revision_v2',
            TRUE
        ), '') IS DISTINCT FROM NEW.deployment_revision::TEXT
        OR COALESCE(pg_catalog.current_setting(
            'starring.runtime_certification_reservation_attempt_v2',
            TRUE
        ), '') IS DISTINCT FROM NEW.convergence_attempt_no::TEXT
        OR COALESCE(pg_catalog.current_setting(
            'starring.runtime_certification_reservation_fingerprint_v2',
            TRUE
        ), '') IS DISTINCT FROM NEW.intent_fingerprint
    THEN
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE = 'runtime_certification_reservation_mutation_rejected';
    END IF;

    FOREACH setting_name IN ARRAY ARRAY[
        'starring.runtime_certification_reservation_action_v2',
        'starring.runtime_certification_reservation_operation_id_v2',
        'starring.runtime_certification_reservation_tenant_id_v2',
        'starring.runtime_certification_reservation_installation_id_v2',
        'starring.runtime_certification_reservation_deployment_id_v2',
        'starring.runtime_certification_reservation_revision_v2',
        'starring.runtime_certification_reservation_attempt_v2',
        'starring.runtime_certification_reservation_fingerprint_v2'
    ]
    LOOP
        PERFORM pg_catalog.set_config(setting_name, '', TRUE);
    END LOOP;

    RETURN NEW;
END;
$function$;

CREATE FUNCTION public.starring_runtime_certification_reserve_intent_v2(
    requested_action_id BIGINT,
    requested_operation_id TEXT,
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_deployment_revision BIGINT,
    expected_controller_id TEXT,
    expected_controller_fencing_token BIGINT,
    expected_runtime_generation BIGINT,
    expected_convergence_attempt_no BIGINT,
    expected_target_guild_id TEXT,
    expected_target_ruleset_key TEXT,
    expected_target_version BIGINT,
    expected_target_content_hash TEXT,
    expected_target_binding_revision BIGINT,
    expected_target_binding_fingerprint TEXT,
    expected_installation_authority_revision BIGINT,
    expected_process_instance_id TEXT,
    expected_gateway_shard_id TEXT,
    expected_gateway_lease_epoch BIGINT,
    expected_gateway_owner_revision BIGINT,
    expected_runtime_build_revision TEXT,
    expected_panel_certificate_id TEXT,
    expected_panel_report_digest TEXT,
    requested_serving_lease_milliseconds BIGINT,
    proposed_certification_intent_bytes BYTEA,
    proposed_intent_fingerprint TEXT
)
RETURNS TABLE(
    outcome_name TEXT,
    locked_snapshot JSONB,
    locked_convergence_attempt_no BIGINT,
    observed_at TIMESTAMPTZ,
    operation_id TEXT,
    tenant_id TEXT,
    installation_id TEXT,
    deployment_id TEXT,
    deployment_revision BIGINT,
    convergence_attempt_no BIGINT,
    certification_intent_bytes BYTEA,
    intent_fingerprint TEXT
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
    deployment_row public.runtime_deployments%ROWTYPE;
    reservation_row public.runtime_certification_operations_v2%ROWTYPE;
    owner_row public.runtime_gateway_owners%ROWTYPE;
    writer_fence_state TEXT;
    candidate_guild_id TEXT;
    candidate_ruleset_key TEXT;
    slot_writer_epoch BIGINT;
    pending_drain_intent_id TEXT;
    expected_bytes BYTEA;
    expected_fingerprint TEXT;
    authority_outcome TEXT;
    reservation_found BOOLEAN;
    setting_name TEXT;
BEGIN
    IF pg_catalog.current_setting('transaction_isolation')
            <> 'serializable'
        OR pg_catalog.current_setting('transaction_read_only') <> 'off'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_certification_reservation_transaction_invalid';
    END IF;

    expected_bytes :=
        starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2(
            requested_action_id,
            requested_operation_id,
            expected_tenant_id,
            expected_installation_id,
            expected_deployment_id,
            expected_deployment_revision,
            expected_controller_id,
            expected_controller_fencing_token,
            expected_runtime_generation,
            expected_convergence_attempt_no,
            expected_target_guild_id,
            expected_target_ruleset_key,
            expected_target_version,
            expected_target_content_hash,
            expected_target_binding_revision,
            expected_target_binding_fingerprint,
            expected_installation_authority_revision,
            expected_process_instance_id,
            expected_gateway_shard_id,
            expected_gateway_lease_epoch,
            expected_gateway_owner_revision,
            expected_runtime_build_revision,
            expected_panel_certificate_id,
            expected_panel_report_digest,
            requested_serving_lease_milliseconds
        );
    expected_fingerprint :=
        starring_runtime_private_v2.starring_runtime_certification_intent_fingerprint_v2(
            expected_bytes
        );

    IF proposed_certification_intent_bytes IS DISTINCT FROM expected_bytes
        OR proposed_intent_fingerprint IS DISTINCT FROM expected_fingerprint
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_certification_reservation_canonical_input_invalid';
    END IF;

    SELECT fence.fence_state
    INTO writer_fence_state
    FROM public.starring_runtime_writer_fence_observe_v1() AS fence;

    IF NOT FOUND
        OR writer_fence_state NOT IN ('open', 'closed')
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_certification_reservation_writer_fence_invalid';
    END IF;

    IF writer_fence_state = 'closed' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX005',
            MESSAGE = 'runtime_certification_reservation_writer_fenced';
    END IF;

    SELECT deployment.guild_id, deployment.ruleset_key
    INTO candidate_guild_id, candidate_ruleset_key
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = expected_tenant_id
        AND deployment.installation_id = expected_installation_id
        AND deployment.deployment_id = expected_deployment_id;

    IF NOT FOUND
        OR candidate_guild_id IS DISTINCT FROM expected_target_guild_id
        OR candidate_ruleset_key IS DISTINCT FROM expected_target_ruleset_key
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_certification_reservation_ownership_lost';
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            pg_catalog.concat(
                'starring-runtime-serving-slot-v1:',
                candidate_guild_id,
                ':',
                candidate_ruleset_key
            ),
            0
        )
    );

    SELECT
        slot_fence.writer_epoch,
        slot_fence.pending_drain_intent_id
    INTO
        slot_writer_epoch,
        pending_drain_intent_id
    FROM starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(
        candidate_guild_id,
        candidate_ruleset_key
    ) AS slot_fence;

    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = expected_tenant_id
        AND deployment.installation_id = expected_installation_id
        AND deployment.deployment_id = expected_deployment_id
    FOR UPDATE;

    observed_at := pg_catalog.clock_timestamp();
    IF NOT FOUND
        OR deployment_row.guild_id IS DISTINCT FROM candidate_guild_id
        OR deployment_row.ruleset_key IS DISTINCT FROM candidate_ruleset_key
        OR deployment_row.revision
            IS DISTINCT FROM expected_deployment_revision
        OR deployment_row.phase <> 'awaiting_gateway_ready'
        OR deployment_row.controller_id
            IS DISTINCT FROM expected_controller_id
        OR deployment_row.controller_fencing_token
            IS DISTINCT FROM expected_controller_fencing_token
        OR deployment_row.last_controller_id
            IS DISTINCT FROM expected_controller_id
        OR deployment_row.last_fencing_token
            IS DISTINCT FROM expected_controller_fencing_token
        OR deployment_row.runtime_generation
            IS DISTINCT FROM expected_runtime_generation
        OR deployment_row.convergence_attempt_no
            IS DISTINCT FROM expected_convergence_attempt_no
        OR deployment_row.target_version
            IS DISTINCT FROM expected_target_version
        OR deployment_row.target_content_hash
            IS DISTINCT FROM expected_target_content_hash
        OR deployment_row.binding_revision
            IS DISTINCT FROM expected_target_binding_revision
        OR deployment_row.binding_fingerprint
            IS DISTINCT FROM expected_target_binding_fingerprint
        OR deployment_row.installation_authority_revision
            IS DISTINCT FROM expected_installation_authority_revision
        OR deployment_row.snapshot #>> '{phase,phase}'
            IS DISTINCT FROM 'awaiting_gateway_ready'
        OR deployment_row.snapshot ->> 'revision'
            IS DISTINCT FROM expected_deployment_revision::TEXT
        OR deployment_row.snapshot -> 'target' IS DISTINCT FROM
            pg_catalog.jsonb_build_object(
                'guild_id', expected_target_guild_id,
                'ruleset_key', expected_target_ruleset_key,
                'version', expected_target_version,
                'content_hash', expected_target_content_hash,
                'binding_revision', expected_target_binding_revision,
                'binding_fingerprint', expected_target_binding_fingerprint
            )
        OR deployment_row.snapshot #>> '{controller_lease,controller_id}'
            IS DISTINCT FROM expected_controller_id
        OR deployment_row.snapshot #>> '{controller_lease,fencing_token}'
            IS DISTINCT FROM expected_controller_fencing_token::TEXT
        OR (deployment_row.snapshot
                #>> '{controller_lease,acquired_at}')::TIMESTAMPTZ
            IS DISTINCT FROM deployment_row.controller_acquired_at
        OR (deployment_row.snapshot
                #>> '{controller_lease,expires_at}')::TIMESTAMPTZ
            IS DISTINCT FROM deployment_row.controller_lease_expires_at
        OR deployment_row.snapshot #>> '{panel_certificate,certificate_id}'
            IS DISTINCT FROM expected_panel_certificate_id
        OR deployment_row.snapshot #>> '{panel_certificate,report_digest}'
            IS DISTINCT FROM expected_panel_report_digest
        OR deployment_row.snapshot #>> '{panel_certificate,process_instance_id}'
            IS DISTINCT FROM expected_process_instance_id
        OR deployment_row.snapshot #>> '{panel_certificate,runtime_generation}'
            IS DISTINCT FROM expected_runtime_generation::TEXT
        OR deployment_row.snapshot #> '{panel_certificate,target}'
            IS DISTINCT FROM deployment_row.snapshot -> 'target'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_certification_reservation_ownership_lost';
    END IF;

    SELECT reservation.*
    INTO reservation_row
    FROM public.runtime_certification_operations_v2 AS reservation
    WHERE reservation.tenant_id = expected_tenant_id
        AND reservation.installation_id = expected_installation_id
        AND reservation.deployment_id = expected_deployment_id
        AND reservation.deployment_revision
            = expected_deployment_revision
        AND reservation.convergence_attempt_no
            = expected_convergence_attempt_no
    FOR UPDATE;
    reservation_found := FOUND;

    locked_snapshot := deployment_row.snapshot;
    locked_convergence_attempt_no := deployment_row.convergence_attempt_no;

    IF reservation_found THEN
        IF reservation_row.operation_id
                IS DISTINCT FROM requested_operation_id
            OR reservation_row.certification_intent_bytes
                IS DISTINCT FROM proposed_certification_intent_bytes
            OR reservation_row.intent_fingerprint
                IS DISTINCT FROM proposed_intent_fingerprint
        THEN
            outcome_name := 'diverged';
            RETURN NEXT;
            RETURN;
        END IF;

        outcome_name := 'reserved';
        operation_id := reservation_row.operation_id;
        tenant_id := reservation_row.tenant_id;
        installation_id := reservation_row.installation_id;
        deployment_id := reservation_row.deployment_id;
        deployment_revision := reservation_row.deployment_revision;
        convergence_attempt_no := reservation_row.convergence_attempt_no;
        certification_intent_bytes :=
            reservation_row.certification_intent_bytes;
        intent_fingerprint := reservation_row.intent_fingerprint;
        RETURN NEXT;
        RETURN;
    END IF;

    IF pending_drain_intent_id IS NOT NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX007',
            MESSAGE = 'runtime_certification_reservation_product_drain_pending';
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
            MESSAGE = 'runtime_certification_reservation_target_superseded';
    ELSIF authority_outcome IS DISTINCT FROM 'exact' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_certification_reservation_authority_changed';
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

    SELECT owner.*
    INTO owner_row
    FROM public.runtime_gateway_owners AS owner
    WHERE owner.gateway_shard_id = expected_gateway_shard_id
    FOR UPDATE;

    observed_at := GREATEST(observed_at, pg_catalog.clock_timestamp());
    IF NOT FOUND
        OR expected_gateway_shard_id <> 'shard:0'
        OR owner_row.process_instance_id
            IS DISTINCT FROM expected_process_instance_id
        OR owner_row.lease_epoch
            IS DISTINCT FROM expected_gateway_lease_epoch
        OR owner_row.expected_build_revision
            IS DISTINCT FROM expected_runtime_build_revision
        OR owner_row.owner_revision
            IS DISTINCT FROM expected_gateway_owner_revision
        OR owner_row.expires_at IS NULL
        OR owner_row.expires_at <= observed_at
        OR deployment_row.controller_acquired_at IS NULL
        OR deployment_row.controller_acquired_at > observed_at
        OR deployment_row.controller_lease_expires_at IS NULL
        OR deployment_row.controller_lease_expires_at <= observed_at
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_certification_reservation_lease_lost';
    END IF;

    PERFORM 1
    FROM public.runtime_certification_operations_v2 AS reservation
    WHERE reservation.operation_id = requested_operation_id
    FOR KEY SHARE;
    IF FOUND THEN
        outcome_name := 'diverged';
        RETURN NEXT;
        RETURN;
    END IF;

    BEGIN
        PERFORM
            starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(
                candidate_guild_id,
                candidate_ruleset_key,
                slot_writer_epoch
            );

        PERFORM pg_catalog.set_config(
            'starring.runtime_certification_reservation_action_v2',
            'insert',
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_certification_reservation_operation_id_v2',
            requested_operation_id,
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_certification_reservation_tenant_id_v2',
            expected_tenant_id,
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_certification_reservation_installation_id_v2',
            expected_installation_id,
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_certification_reservation_deployment_id_v2',
            expected_deployment_id,
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_certification_reservation_revision_v2',
            expected_deployment_revision::TEXT,
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_certification_reservation_attempt_v2',
            expected_convergence_attempt_no::TEXT,
            TRUE
        );
        PERFORM pg_catalog.set_config(
            'starring.runtime_certification_reservation_fingerprint_v2',
            proposed_intent_fingerprint,
            TRUE
        );

        INSERT INTO public.runtime_certification_operations_v2 (
            operation_id,
            tenant_id,
            installation_id,
            deployment_id,
            deployment_revision,
            convergence_attempt_no,
            certification_intent_bytes,
            intent_fingerprint
        ) VALUES (
            requested_operation_id,
            expected_tenant_id,
            expected_installation_id,
            expected_deployment_id,
            expected_deployment_revision,
            expected_convergence_attempt_no,
            proposed_certification_intent_bytes,
            proposed_intent_fingerprint
        )
        RETURNING * INTO reservation_row;
    EXCEPTION
        WHEN unique_violation THEN
            outcome_name := 'diverged';
            RETURN NEXT;
            RETURN;
        WHEN OTHERS THEN
            FOREACH setting_name IN ARRAY ARRAY[
                'starring.runtime_certification_reservation_action_v2',
                'starring.runtime_certification_reservation_operation_id_v2',
                'starring.runtime_certification_reservation_tenant_id_v2',
                'starring.runtime_certification_reservation_installation_id_v2',
                'starring.runtime_certification_reservation_deployment_id_v2',
                'starring.runtime_certification_reservation_revision_v2',
                'starring.runtime_certification_reservation_attempt_v2',
                'starring.runtime_certification_reservation_fingerprint_v2'
            ]
            LOOP
                PERFORM pg_catalog.set_config(setting_name, '', TRUE);
            END LOOP;
            RAISE;
    END;

    outcome_name := 'reserved';
    operation_id := reservation_row.operation_id;
    tenant_id := reservation_row.tenant_id;
    installation_id := reservation_row.installation_id;
    deployment_id := reservation_row.deployment_id;
    deployment_revision := reservation_row.deployment_revision;
    convergence_attempt_no := reservation_row.convergence_attempt_no;
    certification_intent_bytes := reservation_row.certification_intent_bytes;
    intent_fingerprint := reservation_row.intent_fingerprint;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_certification_reservation_observe_v2(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_deployment_revision BIGINT,
    expected_convergence_attempt_no BIGINT
)
RETURNS TABLE(
    outcome_name TEXT,
    locked_snapshot JSONB,
    locked_convergence_attempt_no BIGINT,
    observed_at TIMESTAMPTZ,
    operation_id TEXT,
    tenant_id TEXT,
    installation_id TEXT,
    deployment_id TEXT,
    deployment_revision BIGINT,
    convergence_attempt_no BIGINT,
    certification_intent_bytes BYTEA,
    intent_fingerprint TEXT
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
    deployment_row public.runtime_deployments%ROWTYPE;
    reservation_row public.runtime_certification_operations_v2%ROWTYPE;
    writer_fence_state TEXT;
    candidate_guild_id TEXT;
    candidate_ruleset_key TEXT;
BEGIN
    IF pg_catalog.current_setting('transaction_isolation')
            <> 'read committed'
        OR pg_catalog.current_setting('transaction_read_only') <> 'off'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_certification_reservation_observe_transaction_invalid';
    END IF;

    IF expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_convergence_attempt_no NOT BETWEEN 1 AND 4294967295
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_certification_reservation_observe_input_invalid';
    END IF;

    SELECT fence.fence_state
    INTO writer_fence_state
    FROM public.starring_runtime_writer_fence_observe_v1() AS fence;

    IF NOT FOUND
        OR writer_fence_state NOT IN ('open', 'closed')
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_certification_reservation_observe_fence_invalid';
    END IF;

    SELECT deployment.guild_id, deployment.ruleset_key
    INTO candidate_guild_id, candidate_ruleset_key
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = expected_tenant_id
        AND deployment.installation_id = expected_installation_id
        AND deployment.deployment_id = expected_deployment_id;

    IF NOT FOUND THEN
        outcome_name := 'diverged';
        observed_at := pg_catalog.clock_timestamp();
        RETURN NEXT;
        RETURN;
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            pg_catalog.concat(
                'starring-runtime-serving-slot-v1:',
                candidate_guild_id,
                ':',
                candidate_ruleset_key
            ),
            0
        )
    );

    PERFORM 1
    FROM starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(
        candidate_guild_id,
        candidate_ruleset_key
    );

    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = expected_tenant_id
        AND deployment.installation_id = expected_installation_id
        AND deployment.deployment_id = expected_deployment_id
    FOR UPDATE;

    observed_at := pg_catalog.clock_timestamp();
    IF NOT FOUND
        OR deployment_row.guild_id IS DISTINCT FROM candidate_guild_id
        OR deployment_row.ruleset_key IS DISTINCT FROM candidate_ruleset_key
        OR deployment_row.revision
            IS DISTINCT FROM expected_deployment_revision
        OR deployment_row.convergence_attempt_no
            IS DISTINCT FROM expected_convergence_attempt_no
        OR deployment_row.phase <> 'awaiting_gateway_ready'
        OR deployment_row.controller_id IS NULL
        OR deployment_row.controller_fencing_token IS NULL
        OR deployment_row.last_controller_id
            IS DISTINCT FROM deployment_row.controller_id
        OR deployment_row.last_fencing_token
            IS DISTINCT FROM deployment_row.controller_fencing_token
        OR deployment_row.snapshot #>> '{phase,phase}'
            IS DISTINCT FROM 'awaiting_gateway_ready'
        OR deployment_row.snapshot ->> 'revision'
            IS DISTINCT FROM expected_deployment_revision::TEXT
        OR deployment_row.snapshot #>> '{controller_lease,controller_id}'
            IS DISTINCT FROM deployment_row.controller_id
        OR deployment_row.snapshot #>> '{controller_lease,fencing_token}'
            IS DISTINCT FROM deployment_row.controller_fencing_token::TEXT
        OR deployment_row.snapshot -> 'panel_certificate' IS NULL
        OR deployment_row.snapshot -> 'panel_certificate' = 'null'::JSONB
    THEN
        outcome_name := 'diverged';
        locked_snapshot := deployment_row.snapshot;
        locked_convergence_attempt_no :=
            deployment_row.convergence_attempt_no;
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT reservation.*
    INTO reservation_row
    FROM public.runtime_certification_operations_v2 AS reservation
    WHERE reservation.tenant_id = expected_tenant_id
        AND reservation.installation_id = expected_installation_id
        AND reservation.deployment_id = expected_deployment_id
        AND reservation.deployment_revision
            = expected_deployment_revision
        AND reservation.convergence_attempt_no
            = expected_convergence_attempt_no
    FOR UPDATE;

    locked_snapshot := deployment_row.snapshot;
    locked_convergence_attempt_no := deployment_row.convergence_attempt_no;
    IF NOT FOUND THEN
        outcome_name := 'absent';
        RETURN NEXT;
        RETURN;
    END IF;

    IF starring_runtime_private_v2.starring_runtime_certification_intent_fingerprint_v2(
        reservation_row.certification_intent_bytes
    ) IS DISTINCT FROM reservation_row.intent_fingerprint
    THEN
        outcome_name := 'diverged';
        RETURN NEXT;
        RETURN;
    END IF;

    outcome_name := 'reserved';
    operation_id := reservation_row.operation_id;
    tenant_id := reservation_row.tenant_id;
    installation_id := reservation_row.installation_id;
    deployment_id := reservation_row.deployment_id;
    deployment_revision := reservation_row.deployment_revision;
    convergence_attempt_no := reservation_row.convergence_attempt_no;
    certification_intent_bytes := reservation_row.certification_intent_bytes;
    intent_fingerprint := reservation_row.intent_fingerprint;
    RETURN NEXT;
END;
$function$;

CREATE TRIGGER runtime_certification_operations_v2_reject_row_mutation
BEFORE INSERT OR UPDATE OR DELETE
ON public.runtime_certification_operations_v2
FOR EACH ROW
EXECUTE FUNCTION public.reject_runtime_certification_reservation_mutation_v2();

CREATE TRIGGER runtime_certification_operations_v2_reject_truncate
BEFORE TRUNCATE
ON public.runtime_certification_operations_v2
FOR EACH STATEMENT
EXECUTE FUNCTION public.reject_runtime_certification_reservation_mutation_v2();

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2(
    requested_action_id BIGINT,
    requested_operation_id TEXT,
    requested_tenant_id TEXT,
    requested_installation_id TEXT,
    requested_deployment_id TEXT,
    requested_expected_revision BIGINT,
    requested_controller_id TEXT,
    requested_fencing_token BIGINT,
    requested_runtime_generation BIGINT,
    requested_convergence_attempt BIGINT,
    requested_target_guild_id TEXT,
    requested_target_ruleset_key TEXT,
    requested_target_version BIGINT,
    requested_target_content_hash TEXT,
    requested_target_binding_revision BIGINT,
    requested_target_binding_fingerprint TEXT,
    requested_installation_authority_revision BIGINT,
    requested_process_instance_id TEXT,
    requested_gateway_shard_id TEXT,
    requested_gateway_lease_epoch BIGINT,
    requested_owner_revision BIGINT,
    requested_runtime_build_revision TEXT,
    requested_panel_certificate_id TEXT,
    requested_panel_report_digest TEXT,
    requested_serving_lease_milliseconds BIGINT
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
    target_bytes BYTEA;
    process_identity_bytes BYTEA;
    canonical_bytes BYTEA;
BEGIN
    IF requested_action_id NOT BETWEEN 1 AND 9223372036854775807
        OR requested_operation_id !~ '^[0-9a-f]{32}$'
        OR requested_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR requested_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR requested_deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR requested_expected_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR requested_controller_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR requested_fencing_token NOT BETWEEN 1 AND 9223372036854775807
        OR requested_runtime_generation
            NOT BETWEEN 1 AND 9223372036854775807
        OR requested_convergence_attempt NOT BETWEEN 1 AND 4294967295
        OR requested_target_guild_id !~ '^[1-9][0-9]{0,19}$'
        OR (
            pg_catalog.octet_length(requested_target_guild_id) = 20
            AND requested_target_guild_id COLLATE pg_catalog."C"
                > '18446744073709551615' COLLATE pg_catalog."C"
        )
        OR requested_target_ruleset_key !~ '^[A-Za-z0-9_-]{1,64}$'
        OR requested_target_version NOT BETWEEN 1 AND 4294967295
        OR requested_target_content_hash !~ '^[0-9a-f]{64}$'
        OR requested_target_binding_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR requested_target_binding_fingerprint !~ '^[0-9a-f]{64}$'
        OR requested_installation_authority_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR requested_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR requested_gateway_shard_id !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR requested_gateway_lease_epoch
            NOT BETWEEN 1 AND 9223372036854775807
        OR requested_owner_revision NOT BETWEEN 1 AND 9223372036854775807
        OR requested_runtime_build_revision !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR requested_panel_certificate_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR requested_panel_report_digest !~ '^[0-9a-f]{64}$'
        OR requested_serving_lease_milliseconds NOT BETWEEN 1000 AND 300000
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_certification_intent_builder_input_invalid';
    END IF;

    target_bytes :=
        pg_catalog.convert_to('{"guild_id":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_target_guild_id
        )
        || pg_catalog.convert_to(',"ruleset_key":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_target_ruleset_key
        )
        || pg_catalog.convert_to(
            pg_catalog.concat(
                ',"version":',
                requested_target_version::TEXT,
                ',"content_hash":'
            ),
            'UTF8'
        )
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_target_content_hash
        )
        || pg_catalog.convert_to(
            pg_catalog.concat(
                ',"binding_revision":',
                requested_target_binding_revision::TEXT,
                ',"binding_fingerprint":'
            ),
            'UTF8'
        )
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_target_binding_fingerprint
        )
        || pg_catalog.convert_to('}', 'UTF8');

    process_identity_bytes :=
        pg_catalog.convert_to('{"target":', 'UTF8')
        || target_bytes
        || pg_catalog.convert_to(
            pg_catalog.concat(
                ',"runtime_generation":',
                requested_runtime_generation::TEXT,
                ',"process_instance_id":'
            ),
            'UTF8'
        )
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_process_instance_id
        )
        || pg_catalog.convert_to('}', 'UTF8');

    canonical_bytes :=
        pg_catalog.convert_to(
            pg_catalog.concat(
                '{"format_version":2,"action_id":',
                requested_action_id::TEXT,
                ',"operation_id":'
            ),
            'UTF8'
        )
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_operation_id
        )
        || pg_catalog.convert_to(',"guard":{"scope":{"tenant_id":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_tenant_id
        )
        || pg_catalog.convert_to(',"installation_id":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_installation_id
        )
        || pg_catalog.convert_to(',"deployment_id":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_deployment_id
        )
        || pg_catalog.convert_to(
            pg_catalog.concat(
                '},"expected_revision":',
                requested_expected_revision::TEXT,
                ',"controller_id":'
            ),
            'UTF8'
        )
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_controller_id
        )
        || pg_catalog.convert_to(
            pg_catalog.concat(
                ',"fencing_token":',
                requested_fencing_token::TEXT,
                ',"runtime_generation":',
                requested_runtime_generation::TEXT,
                ',"convergence_attempt":',
                requested_convergence_attempt::TEXT,
                '},"target":'
            ),
            'UTF8'
        )
        || target_bytes
        || pg_catalog.convert_to(',"binding_pin":{"tenant_id":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_tenant_id
        )
        || pg_catalog.convert_to(',"installation_id":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_installation_id
        )
        || pg_catalog.convert_to(
            pg_catalog.concat(
                ',"installation_authority_revision":',
                requested_installation_authority_revision::TEXT,
                ',"binding_revision":',
                requested_target_binding_revision::TEXT,
                ',"binding_fingerprint":'
            ),
            'UTF8'
        )
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_target_binding_fingerprint
        )
        || pg_catalog.convert_to('},"process_identity":', 'UTF8')
        || process_identity_bytes
        || pg_catalog.convert_to(
            ',"gateway_owner_lease_id":{"gateway_shard_id":',
            'UTF8'
        )
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_gateway_shard_id
        )
        || pg_catalog.convert_to(',"process_instance_id":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_process_instance_id
        )
        || pg_catalog.convert_to(
            pg_catalog.concat(
                ',"lease_epoch":',
                requested_gateway_lease_epoch::TEXT,
                ',"expected_build_revision":'
            ),
            'UTF8'
        )
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_runtime_build_revision
        )
        || pg_catalog.convert_to(
            pg_catalog.concat(
                '},"observed_owner_revision":',
                requested_owner_revision::TEXT,
                ',"runtime_build_revision":'
            ),
            'UTF8'
        )
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_runtime_build_revision
        )
        || pg_catalog.convert_to(',"panel":{"certificate_id":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_panel_certificate_id
        )
        || pg_catalog.convert_to(',"report_digest":', 'UTF8')
        || starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(
            requested_panel_report_digest
        )
        || pg_catalog.convert_to(',"process_identity":', 'UTF8')
        || process_identity_bytes
        || pg_catalog.convert_to(
            pg_catalog.concat(
                ',"controller_fencing_token":',
                requested_fencing_token::TEXT,
                '},"serving_lease_milliseconds":',
                requested_serving_lease_milliseconds::TEXT,
                '}'
            ),
            'UTF8'
        );

    IF pg_catalog.octet_length(canonical_bytes) NOT BETWEEN 1 AND 32768 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_certification_intent_builder_output_invalid';
    END IF;

    RETURN canonical_bytes;
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_certification_intent_fingerprint_v2(
    canonical_payload BYTEA
)
RETURNS TEXT
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
BEGIN
    IF pg_catalog.octet_length(canonical_payload) NOT BETWEEN 1 AND 32768 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_certification_intent_fingerprint_payload_invalid';
    END IF;

    RETURN starring_runtime_private_v2.starring_runtime_framed_digest_v2(
        pg_catalog.convert_to(
            'starring.runtime.certification_intent.v2',
            'UTF8'
        ) || pg_catalog.decode('00', 'hex'),
        canonical_payload
    );
END;
$function$;

REVOKE ALL ON TABLE
    public.runtime_certification_operations_v2
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.reject_runtime_certification_reservation_mutation_v2()
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.starring_runtime_certification_reserve_intent_v2(
        BIGINT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        BIGINT,
        BIGINT,
        BIGINT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        BIGINT,
        TEXT,
        BIGINT,
        TEXT,
        TEXT,
        BIGINT,
        BIGINT,
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        BYTEA,
        TEXT
    )
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.starring_runtime_certification_reservation_observe_v2(
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        BIGINT
    )
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2(
        BIGINT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        BIGINT,
        BIGINT,
        BIGINT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        BIGINT,
        TEXT,
        BIGINT,
        TEXT,
        TEXT,
        BIGINT,
        BIGINT,
        TEXT,
        TEXT,
        TEXT,
        BIGINT
    )
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    starring_runtime_private_v2.starring_runtime_certification_intent_fingerprint_v2(
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
        '(pg_catalog.to_regclass(''public.runtime_drain_intents_v2'')),';
    next_fragment := previous_fragment || E'\n' ||
        '            (pg_catalog.to_regclass(''public.runtime_certification_operations_v2'')),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_reservation_manifest_relation_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        'SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_product_drain_observe_v2(text,text,text,bigint,text,text)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(text)''';
    next_fragment :=
        'SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_product_drain_observe_v2(text,text,text,bigint,text,text)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.reject_runtime_certification_reservation_mutation_v2()''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_certification_reserve_intent_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint,bytea,text)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_certification_reservation_observe_v2(text,text,text,bigint,bigint)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_certification_intent_fingerprint_v2(bytea)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(text)''';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_reservation_manifest_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        'RETURN observed_count = 623' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''e9af146803f79bf195250ac230a9c39d7eef4f29349ac08a9d1c3914187fd3f2'';';
    next_fragment :=
        'RETURN observed_count = 650' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''f053e9131dcd32f1168ff6201ad57f4f40e3165ab619414a3552b74717bbe2c9'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_reservation_manifest_expectation_patch_drift';
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
        '(''public.runtime_drain_intents_v2''),';
    next_fragment := previous_fragment || E'\n' ||
        '            (''public.runtime_certification_operations_v2''),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_reservation_readiness_relation_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '(''public.reject_runtime_product_drain_mutation()''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(text)'')';
    next_fragment :=
        '(''public.reject_runtime_product_drain_mutation()''),' || E'\n' ||
        '            (''public.reject_runtime_certification_reservation_mutation_v2()''),' || E'\n' ||
        '            (''public.starring_runtime_certification_reserve_intent_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint,bytea,text)''),' || E'\n' ||
        '            (''public.starring_runtime_certification_reservation_observe_v2(text,text,text,bigint,bigint)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_certification_intent_fingerprint_v2(bytea)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(text)'')';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_reservation_readiness_protected_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '(''starring_runtime_private_v2.starring_runtime_framed_digest_v2(bytea,bytea)''),';
    next_fragment := previous_fragment || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_certification_intent_fingerprint_v2(bytea)''),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_reservation_readiness_private_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '''3c97b3b41f45b11ed2b01890c3d708806d802593f71589031cb921dfc5c65fe3''::TEXT';
    next_fragment :=
        '''4089395be3df848f9025655ef183b0336ecfefd62861bf735f53c4c26aad2ae7''::TEXT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_reservation_readiness_manifest_digest_patch_drift';
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
    invalid_relation_count BIGINT;
    invalid_function_count BIGINT;
    invalid_acl_count BIGINT;
    invalid_trigger_count BIGINT;
    setting_name TEXT;
    golden_bytes BYTEA;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT pg_catalog.count(*)
    INTO invalid_relation_count
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
            'public.runtime_certification_operations_v2'
        )
        AND (
            relation.relkind <> 'r'
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
        );

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.reject_runtime_certification_reservation_mutation_v2()',
                'ff49c4ce2863940ca964444d9046caed23cf7db0cac97163e2ef73d7bd9c207b',
                'plpgsql',
                'v',
                FALSE,
                'u',
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'public.starring_runtime_certification_reserve_intent_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint,bytea,text)',
                '4c088bff39108bccd0690c1a8cf395572c7e6f0b4380d4df5460c91398e5038d',
                'plpgsql',
                'v',
                TRUE,
                'u',
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_certification_reservation_observe_v2(text,text,text,bigint,bigint)',
                'a6443bcca0fab54523f1570c656da8792dd37a002bca71e0a5d6a53d34ebff39',
                'plpgsql',
                'v',
                TRUE,
                'u',
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2(bigint,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,text,text,bigint,bigint,text,text,text,bigint)',
                'a98d87b7aba288ca18c44c6c8419ad6092595c8a321da18b7d3d5f005a6a64e9',
                'plpgsql',
                'i',
                TRUE,
                's',
                FALSE,
                FALSE,
                0::REAL
            ),
            (
                'starring_runtime_private_v2.starring_runtime_certification_intent_fingerprint_v2(bytea)',
                '5e54a6b0fec4e3d68fb5d12d14fddd7afe13f1279d0d8ea1f0d7681d5037e13b',
                'plpgsql',
                'i',
                TRUE,
                's',
                FALSE,
                FALSE,
                0::REAL
            ),
            (
                'public.starring_runtime_execution_schema_manifest_v1()',
                '4089395be3df848f9025655ef183b0336ecfefd62861bf735f53c4c26aad2ae7',
                'plpgsql',
                'v',
                TRUE,
                'u',
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'public.starring_runtime_execution_database_readiness_v1()',
                '6962c1c2ffdd862a86aed3c84569ac50307964d59711d0bddc26aadbf68577e2',
                'plpgsql',
                'v',
                TRUE,
                'u',
                TRUE,
                TRUE,
                1::REAL
            )
    ) AS expected(
        identity,
        definition_digest,
        language_name,
        volatility,
        is_strict,
        parallel_safety,
        security_definer,
        returns_set,
        rows_estimate
    )
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> expected.volatility
        OR function_row.proisstrict IS DISTINCT FROM expected.is_strict
        OR function_row.proparallel <> expected.parallel_safety
        OR function_row.prosecdef IS DISTINCT FROM expected.security_definer
        OR function_row.proretset IS DISTINCT FROM expected.returns_set
        OR function_row.prorows IS DISTINCT FROM expected.rows_estimate
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
        ) IS DISTINCT FROM expected.definition_digest;

    SELECT pg_catalog.count(*)
    INTO invalid_acl_count
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
    LEFT JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege ON TRUE
    WHERE function_row.oid IS NULL
        OR privilege.grantee <> common_owner
        OR privilege.grantor <> common_owner
        OR privilege.privilege_type <> 'EXECUTE'
        OR privilege.is_grantable;

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

    FOREACH setting_name IN ARRAY ARRAY[
        'starring.runtime_certification_reservation_action_v2',
        'starring.runtime_certification_reservation_operation_id_v2',
        'starring.runtime_certification_reservation_tenant_id_v2',
        'starring.runtime_certification_reservation_installation_id_v2',
        'starring.runtime_certification_reservation_deployment_id_v2',
        'starring.runtime_certification_reservation_revision_v2',
        'starring.runtime_certification_reservation_attempt_v2',
        'starring.runtime_certification_reservation_fingerprint_v2'
    ]
    LOOP
        IF COALESCE(pg_catalog.current_setting(setting_name, TRUE), '') <> ''
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_certification_reservation_gate_drift';
        END IF;
    END LOOP;

    golden_bytes :=
        starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2(
            1,
            '00112233445566778899aabbccddeeff',
            'tenant:1',
            'installation:1',
            'deployment:1',
            2,
            'controller:1',
            3,
            4,
            5,
            '7',
            'studyroom',
            1,
            pg_catalog.repeat('b', 64),
            3,
            pg_catalog.repeat('a', 64),
            6,
            'process:1',
            'shard:0',
            5,
            7,
            'build:1',
            'panel:1',
            pg_catalog.repeat('c', 64),
            30000
        );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR pg_catalog.to_regclass(
            'public.runtime_certification_operations_v2'
        ) IS NULL
        OR invalid_relation_count <> 0
        OR invalid_function_count <> 0
        OR invalid_acl_count <> 0
        OR invalid_trigger_count <> 0
        OR (SELECT pg_catalog.count(*)
            FROM public.runtime_certification_operations_v2) <> 0
        OR pg_catalog.octet_length(golden_bytes) <> 1844
        OR starring_runtime_private_v2.starring_runtime_certification_intent_fingerprint_v2(
            golden_bytes
        ) IS DISTINCT FROM
            '686ccbc5e00269f5b373bd5eec398e3b845e17d938cce2b4ae3e1ef19923b99d'
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_reservation_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
