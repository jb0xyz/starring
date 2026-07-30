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
    public.runtime_certification_operation_terminals_v2,
    public.runtime_attestations,
    public.runtime_serving_leases,
    public.runtime_gateway_owners,
    public.runtime_ingress_open_acknowledgements_v2,
    public.automation_installations,
    public.automation_installation_authority_versions,
    public.automation_ruleset_activations,
    public.automation_ruleset_versions
IN ACCESS EXCLUSIVE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    collision_count BIGINT;
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
        AND function_row.proname IN (
            'starring_runtime_certification_prepare_v2',
            'starring_runtime_certification_commit_v2',
            'starring_runtime_certification_observe_v2',
            'starring_runtime_serving_observe_v2',
            'starring_runtime_serving_heartbeat_v2',
            'starring_runtime_serving_disconnect_if_current_v2'
        );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR collision_count <> 0
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_attribute AS attribute
            WHERE attribute.attrelid = pg_catalog.to_regclass(
                    'public.runtime_attestations'
                )
                AND attribute.attname LIKE 'v2_%'
                AND attribute.attnum > 0
                AND NOT attribute.attisdropped
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_v2_preflight_drift';
    END IF;
END;
$preflight$;

ALTER TABLE public.runtime_attestations
    ADD COLUMN v2_operation_id TEXT,
    ADD COLUMN v2_intent_fingerprint TEXT,
    ADD COLUMN v2_request_digest TEXT,
    ADD COLUMN v2_request_bytes BYTEA,
    ADD COLUMN v2_live_attestation_bytes BYTEA,
    ADD COLUMN v2_must_commit_before TIMESTAMPTZ,
    ADD COLUMN v2_route_admission JSONB,
    ADD COLUMN v2_route_incarnation BIGINT,
    ADD COLUMN v2_route_activation_sequence BIGINT,
    ADD COLUMN v2_initial_lease_epoch BIGINT,
    ADD COLUMN v2_initial_serving_revision BIGINT,
    ADD COLUMN v2_prepared_snapshot JSONB,
    ADD COLUMN v2_certified_snapshot JSONB;

ALTER TABLE public.runtime_attestations
    DROP CONSTRAINT runtime_attestations_record_valid;

ALTER TABLE public.runtime_attestations
    ADD CONSTRAINT runtime_attestations_record_valid CHECK (
        record_format_version IN (1, 2)
        AND pg_catalog.jsonb_typeof(record) = 'object'
        AND pg_catalog.octet_length(record::TEXT)
            BETWEEN 32 AND 262144
    ) NOT VALID;

ALTER TABLE public.runtime_attestations
    ADD CONSTRAINT runtime_attestations_v2_shape_valid CHECK (
        (
            record_format_version = 1
            AND v2_operation_id IS NULL
            AND v2_intent_fingerprint IS NULL
            AND v2_request_digest IS NULL
            AND v2_request_bytes IS NULL
            AND v2_live_attestation_bytes IS NULL
            AND v2_must_commit_before IS NULL
            AND v2_route_admission IS NULL
            AND v2_route_incarnation IS NULL
            AND v2_route_activation_sequence IS NULL
            AND v2_initial_lease_epoch IS NULL
            AND v2_initial_serving_revision IS NULL
            AND v2_prepared_snapshot IS NULL
            AND v2_certified_snapshot IS NULL
        )
        OR (
            record_format_version = 2
            AND v2_operation_id ~ '^[0-9a-f]{32}$'
            AND v2_intent_fingerprint ~ '^[0-9a-f]{64}$'
            AND v2_request_digest ~ '^[0-9a-f]{64}$'
            AND pg_catalog.octet_length(v2_request_bytes)
                BETWEEN 1 AND 65536
            AND pg_catalog.octet_length(v2_live_attestation_bytes)
                BETWEEN 1 AND 131072
            AND pg_catalog.isfinite(v2_must_commit_before)
            AND v2_must_commit_before >= certified_at
            AND pg_catalog.jsonb_typeof(v2_route_admission) = 'object'
            AND pg_catalog.octet_length(v2_route_admission::TEXT)
                BETWEEN 32 AND 32768
            AND v2_route_incarnation
                BETWEEN 1 AND 9223372036854775807
            AND v2_route_activation_sequence
                BETWEEN 1 AND 9223372036854775807
            AND v2_initial_lease_epoch
                BETWEEN 1 AND 9223372036854775807
            AND v2_initial_serving_revision
                BETWEEN 1 AND 9223372036854775807
            AND pg_catalog.jsonb_typeof(v2_prepared_snapshot) = 'object'
            AND pg_catalog.octet_length(v2_prepared_snapshot::TEXT)
                BETWEEN 32 AND 262144
            AND pg_catalog.jsonb_typeof(v2_certified_snapshot) = 'object'
            AND pg_catalog.octet_length(v2_certified_snapshot::TEXT)
                BETWEEN 32 AND 262144
            AND v2_request_digest =
                starring_runtime_private_v2.starring_runtime_framed_digest_v2(
                    pg_catalog.convert_to(
                        'starring.runtime.certification_request.v2',
                        'UTF8'
                    ) || pg_catalog.decode('00', 'hex'),
                    v2_request_bytes
                )
            AND attestation_id =
                starring_runtime_private_v2.starring_runtime_framed_digest_v2(
                    pg_catalog.convert_to(
                        'starring.runtime.live_attestation.v2',
                        'UTF8'
                    ) || pg_catalog.decode('00', 'hex'),
                    v2_live_attestation_bytes
                )
            AND v2_live_attestation_bytes =
                pg_catalog.convert_to(
                    '{"format_version":2,"request_digest":"'
                        || v2_request_digest
                        || '","request":',
                    'UTF8'
                )
                || v2_request_bytes
                || pg_catalog.convert_to('}', 'UTF8')
        )
    ) NOT VALID;

ALTER TABLE public.runtime_attestations
    VALIDATE CONSTRAINT runtime_attestations_record_valid;

ALTER TABLE public.runtime_attestations
    VALIDATE CONSTRAINT runtime_attestations_v2_shape_valid;

CREATE UNIQUE INDEX runtime_attestations_v2_operation_unique
ON public.runtime_attestations (v2_operation_id)
WHERE record_format_version = 2;

CREATE UNIQUE INDEX runtime_attestations_v2_request_digest_unique
ON public.runtime_attestations (v2_request_digest)
WHERE record_format_version = 2;

CREATE OR REPLACE FUNCTION public.validate_runtime_attestation_projection()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    deployment_row public.runtime_deployments%ROWTYPE;
    mutation_clock TIMESTAMPTZ;
    decoded_record JSONB;
BEGIN
    mutation_clock := public.starring_runtime_current_mutation_clock();
    SELECT *
    INTO deployment_row
    FROM public.runtime_deployments
    WHERE tenant_id = NEW.tenant_id
        AND installation_id = NEW.installation_id
        AND deployment_id = NEW.deployment_id
    FOR SHARE;

    IF NOT FOUND
        OR deployment_row.promotion_id IS DISTINCT FROM NEW.promotion_id
        OR deployment_row.activation_request_id
            IS DISTINCT FROM NEW.activation_request_id
        OR deployment_row.guild_id IS DISTINCT FROM NEW.guild_id
        OR deployment_row.ruleset_key IS DISTINCT FROM NEW.ruleset_key
        OR deployment_row.target_version IS DISTINCT FROM NEW.target_version
        OR deployment_row.target_content_hash
            IS DISTINCT FROM NEW.target_content_hash
        OR deployment_row.binding_revision
            IS DISTINCT FROM NEW.binding_revision
        OR deployment_row.binding_fingerprint
            IS DISTINCT FROM NEW.binding_fingerprint
        OR deployment_row.runtime_generation
            IS DISTINCT FROM NEW.runtime_generation
        OR deployment_row.phase <> 'awaiting_gateway_ready'
        OR NEW.deployment_revision <> deployment_row.revision + 1
        OR deployment_row.controller_fencing_token
            IS DISTINCT FROM NEW.controller_fencing_token
        OR NEW.certified_at IS DISTINCT FROM mutation_clock
        OR NEW.gateway_ready_at < mutation_clock - INTERVAL '10 minutes'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE =
                'runtime_attestation_fenced_deployment_mismatch';
    END IF;

    IF NEW.record_format_version = 1 THEN
        IF NEW.record #>> '{live,target,guild_id}'
                IS DISTINCT FROM NEW.guild_id
            OR NEW.record #>> '{live,target,ruleset_key}'
                IS DISTINCT FROM NEW.ruleset_key
            OR NEW.record #>> '{live,target,version}'
                IS DISTINCT FROM NEW.target_version::TEXT
            OR NEW.record #>> '{live,target,content_hash}'
                IS DISTINCT FROM NEW.target_content_hash
            OR NEW.record #>> '{live,target,binding_revision}'
                IS DISTINCT FROM NEW.binding_revision::TEXT
            OR NEW.record #>> '{live,target,binding_fingerprint}'
                IS DISTINCT FROM NEW.binding_fingerprint
            OR NEW.record #>> '{live,runtime_generation}'
                IS DISTINCT FROM NEW.runtime_generation::TEXT
            OR NEW.record #>> '{live,process_instance_id}'
                IS DISTINCT FROM NEW.process_instance_id
            OR NEW.record #>> '{live,activation,activation_request_id}'
                IS DISTINCT FROM NEW.activation_request_id
            OR NEW.record #>> '{live,panel_certificate,certificate_id}'
                IS DISTINCT FROM NEW.panel_certificate_id
            OR NEW.record #>> '{live,gateway_ready,kind}'
                IS DISTINCT FROM NEW.gateway_ready_kind
            OR (NEW.record #>> '{live,gateway_ready,ready_at}')
                    ::TIMESTAMPTZ
                IS DISTINCT FROM NEW.gateway_ready_at
            OR (NEW.record #>> '{live,certified_at}')::TIMESTAMPTZ
                IS DISTINCT FROM NEW.certified_at
            OR NEW.record ->> 'runtime_build_revision'
                IS DISTINCT FROM NEW.runtime_build_revision
            OR NEW.record ->> 'panel_report_digest'
                IS DISTINCT FROM NEW.panel_report_digest
            OR NEW.record ->> 'gateway_shard_id'
                IS DISTINCT FROM NEW.gateway_shard_id
            OR NEW.record ->> 'controller_fencing_token'
                IS DISTINCT FROM NEW.controller_fencing_token::TEXT
            OR NEW.record ->> 'deployment_revision'
                IS DISTINCT FROM NEW.deployment_revision::TEXT
        THEN
            RAISE EXCEPTION USING
                ERRCODE = '23514',
                MESSAGE = 'runtime_attestation_v1_shadow_mismatch';
        END IF;
        RETURN NEW;
    END IF;

    IF NEW.record_format_version <> 2 THEN
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE = 'runtime_attestation_format_invalid';
    END IF;

    BEGIN
        decoded_record := pg_catalog.convert_from(
            NEW.v2_live_attestation_bytes,
            'UTF8'
        )::JSONB;
    EXCEPTION
        WHEN OTHERS THEN
            RAISE EXCEPTION USING
                ERRCODE = '23514',
                MESSAGE = 'runtime_attestation_v2_encoding_invalid';
    END;

    IF decoded_record IS DISTINCT FROM NEW.record
        OR NEW.record ->> 'format_version' IS DISTINCT FROM '2'
        OR NEW.record ->> 'request_digest'
            IS DISTINCT FROM NEW.v2_request_digest
        OR NEW.record #>> '{request,format_version}' IS DISTINCT FROM '2'
        OR NEW.record #>> '{request,intent,operation_id}'
            IS DISTINCT FROM NEW.v2_operation_id
        OR NEW.record #>> '{request,intent_fingerprint}'
            IS DISTINCT FROM NEW.v2_intent_fingerprint
        OR NEW.record #>> '{request,intent,guard,scope,tenant_id}'
            IS DISTINCT FROM NEW.tenant_id
        OR NEW.record
                #>> '{request,intent,guard,scope,installation_id}'
            IS DISTINCT FROM NEW.installation_id
        OR NEW.record #>> '{request,intent,guard,scope,deployment_id}'
            IS DISTINCT FROM NEW.deployment_id
        OR NEW.record #>> '{request,intent,guard,expected_revision}'
            IS DISTINCT FROM (NEW.deployment_revision - 1)::TEXT
        OR NEW.record #>> '{request,intent,target,guild_id}'
            IS DISTINCT FROM NEW.guild_id
        OR NEW.record #>> '{request,intent,target,ruleset_key}'
            IS DISTINCT FROM NEW.ruleset_key
        OR NEW.record #>> '{request,intent,target,version}'
            IS DISTINCT FROM NEW.target_version::TEXT
        OR NEW.record #>> '{request,intent,target,content_hash}'
            IS DISTINCT FROM NEW.target_content_hash
        OR NEW.record #>> '{request,intent,target,binding_revision}'
            IS DISTINCT FROM NEW.binding_revision::TEXT
        OR NEW.record #>> '{request,intent,target,binding_fingerprint}'
            IS DISTINCT FROM NEW.binding_fingerprint
        OR NEW.record #>> '{request,intent,guard,runtime_generation}'
            IS DISTINCT FROM NEW.runtime_generation::TEXT
        OR NEW.record #>> '{request,intent,process_identity,process_instance_id}'
            IS DISTINCT FROM NEW.process_instance_id
        OR NEW.record #>> '{request,intent,runtime_build_revision}'
            IS DISTINCT FROM NEW.runtime_build_revision
        OR NEW.record #>> '{request,intent,panel,certificate_id}'
            IS DISTINCT FROM NEW.panel_certificate_id
        OR NEW.record #>> '{request,intent,panel,report_digest}'
            IS DISTINCT FROM NEW.panel_report_digest
        OR NEW.record
                #>> '{request,intent,guard,fencing_token}'
            IS DISTINCT FROM NEW.controller_fencing_token::TEXT
        OR NEW.record
                #>> '{request,intent,gateway_owner_lease_id,gateway_shard_id}'
            IS DISTINCT FROM NEW.gateway_shard_id
        OR NEW.record #> '{request,route_admission}'
            IS DISTINCT FROM NEW.v2_route_admission
        OR NEW.record
                #>> '{request,route_admission,route,route_incarnation}'
            IS DISTINCT FROM NEW.v2_route_incarnation::TEXT
        OR NEW.record
                #>> '{request,route_admission,route,activation_sequence}'
            IS DISTINCT FROM NEW.v2_route_activation_sequence::TEXT
        OR NEW.v2_certified_snapshot #>> '{revision}'
            IS DISTINCT FROM NEW.deployment_revision::TEXT
        OR NEW.v2_certified_snapshot #>> '{phase,phase}'
            IS DISTINCT FROM 'live'
        OR NEW.v2_prepared_snapshot #>> '{revision}'
            IS DISTINCT FROM (NEW.deployment_revision - 1)::TEXT
        OR NEW.v2_prepared_snapshot #>> '{phase,phase}'
            IS DISTINCT FROM 'awaiting_gateway_ready'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE = 'runtime_attestation_v2_shadow_mismatch';
    END IF;
    RETURN NEW;
END;
$function$;


CREATE FUNCTION public.starring_runtime_certification_observe_v2(
    expected_operation_id TEXT,
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_deployment_revision BIGINT,
    expected_convergence_attempt_no BIGINT,
    expected_request_digest TEXT
)
RETURNS TABLE(
    outcome_name TEXT,
    snapshot JSONB,
    convergence_attempt_no BIGINT,
    observed_deployment_revision BIGINT,
    observed_at TIMESTAMPTZ,
    operation_id TEXT,
    intent_fingerprint TEXT,
    certification_intent_bytes BYTEA,
    certification_request_bytes BYTEA,
    request_digest TEXT,
    live_attestation_record_bytes BYTEA,
    attestation_digest TEXT,
    route_admission JSONB,
    tenant_id TEXT,
    installation_id TEXT,
    deployment_id TEXT,
    guild_id TEXT,
    ruleset_key TEXT,
    process_instance_id TEXT,
    runtime_generation BIGINT,
    lease_epoch BIGINT,
    serving_revision BIGINT,
    acquired_at TIMESTAMPTZ,
    last_heartbeat_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    connected BOOLEAN,
    serving BOOLEAN,
    certified_at TIMESTAMPTZ
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
    operation_row public.runtime_certification_operations_v2%ROWTYPE;
    terminal_row public.runtime_certification_operation_terminals_v2%ROWTYPE;
    attestation_row public.runtime_attestations%ROWTYPE;
    terminal_count BIGINT;
    attestation_count BIGINT;
BEGIN
    IF pg_catalog.current_setting('transaction_isolation')
            <> 'read committed'
        OR pg_catalog.current_setting('transaction_read_only') <> 'off'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_certification_observe_v2_transaction_invalid';
    END IF;

    IF expected_operation_id !~ '^[0-9a-f]{32}$'
        OR expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR expected_convergence_attempt_no NOT BETWEEN 1 AND 4294967295
        OR expected_request_digest !~ '^[0-9a-f]{64}$'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_certification_observe_v2_input_invalid';
    END IF;

    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = expected_tenant_id
        AND deployment.installation_id = expected_installation_id
        AND deployment.deployment_id = expected_deployment_id
    FOR SHARE;

    IF deployment_row.deployment_id IS NULL THEN
        observed_at := pg_catalog.clock_timestamp();
        outcome_name := 'diverged';
        RETURN NEXT;
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

    SELECT pg_catalog.count(*)
    INTO terminal_count
    FROM public.runtime_certification_operation_terminals_v2 AS terminal
    WHERE terminal.operation_id = expected_operation_id;

    SELECT pg_catalog.count(*)
    INTO attestation_count
    FROM public.runtime_attestations AS attestation
    WHERE attestation.v2_operation_id = expected_operation_id;

    SELECT operation.*
    INTO operation_row
    FROM public.runtime_certification_operations_v2 AS operation
    WHERE operation.operation_id = expected_operation_id
    FOR KEY SHARE;

    SELECT terminal.*
    INTO terminal_row
    FROM public.runtime_certification_operation_terminals_v2 AS terminal
    WHERE terminal.operation_id = expected_operation_id
    FOR KEY SHARE;

    observed_at := pg_catalog.clock_timestamp();
    IF terminal_count = 1 AND attestation_count = 1 THEN
        SELECT attestation.*
        INTO attestation_row
        FROM public.runtime_attestations AS attestation
        WHERE attestation.v2_operation_id = expected_operation_id
        FOR KEY SHARE;

        IF operation_row.operation_id IS NULL
            OR terminal_row.operation_id IS NULL
            OR terminal_row.terminal_outcome_name
                IS DISTINCT FROM 'certification_committed'
            OR terminal_row.resulting_phase IS DISTINCT FROM 'live'
            OR terminal_row.resulting_deployment_revision
                IS DISTINCT FROM expected_deployment_revision + 1
            OR terminal_row.resulting_convergence_attempt_no
                IS DISTINCT FROM expected_convergence_attempt_no
            OR terminal_row.tenant_id IS DISTINCT FROM expected_tenant_id
            OR terminal_row.installation_id
                IS DISTINCT FROM expected_installation_id
            OR terminal_row.deployment_id
                IS DISTINCT FROM expected_deployment_id
            OR terminal_row.deployment_revision
                IS DISTINCT FROM expected_deployment_revision
            OR terminal_row.terminal_receipt_bytes
                IS DISTINCT FROM attestation_row.v2_live_attestation_bytes
            OR attestation_row.record_format_version <> 2
            OR attestation_row.tenant_id IS DISTINCT FROM expected_tenant_id
            OR attestation_row.installation_id
                IS DISTINCT FROM expected_installation_id
            OR attestation_row.deployment_id
                IS DISTINCT FROM expected_deployment_id
            OR attestation_row.deployment_revision
                IS DISTINCT FROM expected_deployment_revision + 1
            OR attestation_row.convergence_attempt_no
                IS DISTINCT FROM expected_convergence_attempt_no
            OR attestation_row.v2_request_digest
                IS DISTINCT FROM expected_request_digest
            OR attestation_row.v2_intent_fingerprint
                IS DISTINCT FROM operation_row.intent_fingerprint
            OR attestation_row.v2_request_bytes IS NULL
            OR attestation_row.v2_live_attestation_bytes IS NULL
            OR attestation_row.v2_prepared_snapshot IS NULL
            OR attestation_row.v2_certified_snapshot IS NULL
            OR attestation_row.v2_initial_lease_epoch IS NULL
            OR attestation_row.v2_initial_serving_revision IS NULL
        THEN
            outcome_name := 'diverged';
            snapshot := attestation_row.v2_certified_snapshot;
            observed_deployment_revision :=
                attestation_row.deployment_revision;
            RETURN NEXT;
            RETURN;
        END IF;

        outcome_name := 'committed';
        snapshot := attestation_row.v2_certified_snapshot;
        convergence_attempt_no :=
            attestation_row.convergence_attempt_no;
        observed_deployment_revision :=
            attestation_row.deployment_revision;
        operation_id := attestation_row.v2_operation_id;
        intent_fingerprint :=
            attestation_row.v2_intent_fingerprint;
        certification_intent_bytes :=
            operation_row.certification_intent_bytes;
        certification_request_bytes :=
            attestation_row.v2_request_bytes;
        request_digest := attestation_row.v2_request_digest;
        live_attestation_record_bytes :=
            attestation_row.v2_live_attestation_bytes;
        attestation_digest := attestation_row.attestation_id;
        route_admission := attestation_row.v2_route_admission;
        tenant_id := attestation_row.tenant_id;
        installation_id := attestation_row.installation_id;
        deployment_id := attestation_row.deployment_id;
        guild_id := attestation_row.guild_id;
        ruleset_key := attestation_row.ruleset_key;
        process_instance_id := attestation_row.process_instance_id;
        runtime_generation := attestation_row.runtime_generation;
        lease_epoch := attestation_row.v2_initial_lease_epoch;
        serving_revision :=
            attestation_row.v2_initial_serving_revision;
        acquired_at := attestation_row.certified_at;
        last_heartbeat_at := attestation_row.certified_at;
        expires_at := attestation_row.certified_at
            + (
                attestation_row.serving_lease_duration_nanos
                    / 1000000
            ) * INTERVAL '1 millisecond';
        connected := TRUE;
        serving := TRUE;
        certified_at := attestation_row.certified_at;
        RETURN NEXT;
        RETURN;
    END IF;

    IF terminal_count <> 0
        OR attestation_count <> 0
        OR operation_row.operation_id IS NULL
        OR operation_row.tenant_id IS DISTINCT FROM expected_tenant_id
        OR operation_row.installation_id
            IS DISTINCT FROM expected_installation_id
        OR operation_row.deployment_id
            IS DISTINCT FROM expected_deployment_id
        OR operation_row.deployment_revision
            IS DISTINCT FROM expected_deployment_revision
        OR operation_row.convergence_attempt_no
            IS DISTINCT FROM expected_convergence_attempt_no
    THEN
        outcome_name := 'diverged';
        RETURN NEXT;
        RETURN;
    END IF;

    IF deployment_row.revision
            IS DISTINCT FROM expected_deployment_revision
        OR deployment_row.convergence_attempt_no
            IS DISTINCT FROM expected_convergence_attempt_no
        OR deployment_row.phase <> 'awaiting_gateway_ready'
        OR deployment_row.snapshot #>> '{phase,phase}'
            IS DISTINCT FROM 'awaiting_gateway_ready'
        OR deployment_row.snapshot ->> 'revision'
            IS DISTINCT FROM expected_deployment_revision::TEXT
    THEN
        outcome_name := 'diverged';
        snapshot := deployment_row.snapshot;
        convergence_attempt_no :=
            deployment_row.convergence_attempt_no;
        observed_deployment_revision := deployment_row.revision;
        RETURN NEXT;
        RETURN;
    END IF;

    outcome_name := 'not_committed';
    snapshot := deployment_row.snapshot;
    convergence_attempt_no := deployment_row.convergence_attempt_no;
    observed_deployment_revision := deployment_row.revision;
    operation_id := operation_row.operation_id;
    intent_fingerprint := operation_row.intent_fingerprint;
    certification_intent_bytes :=
        operation_row.certification_intent_bytes;
    request_digest := expected_request_digest;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_serving_observe_v2(
    expected_operation_id TEXT,
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_attestation_digest TEXT,
    expected_process_instance_id TEXT,
    expected_runtime_generation BIGINT,
    expected_lease_epoch BIGINT
)
RETURNS TABLE(
    outcome_name TEXT,
    operation_id TEXT,
    tenant_id TEXT,
    installation_id TEXT,
    deployment_id TEXT,
    guild_id TEXT,
    ruleset_key TEXT,
    target_version BIGINT,
    target_content_hash TEXT,
    binding_revision BIGINT,
    binding_fingerprint TEXT,
    attestation_digest TEXT,
    process_instance_id TEXT,
    runtime_generation BIGINT,
    lease_epoch BIGINT,
    serving_revision BIGINT,
    acquired_at TIMESTAMPTZ,
    last_heartbeat_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    connected BOOLEAN,
    serving BOOLEAN,
    observed_at TIMESTAMPTZ
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
    attestation_row public.runtime_attestations%ROWTYPE;
    serving_row public.runtime_serving_leases%ROWTYPE;
BEGIN
    IF expected_operation_id !~ '^[0-9a-f]{32}$'
        OR expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_attestation_digest !~ '^[0-9a-f]{64}$'
        OR expected_process_instance_id
            !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_runtime_generation
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_lease_epoch NOT BETWEEN 1 AND 9223372036854775807
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS002',
            MESSAGE = 'runtime_serving_observe_v2_input_invalid';
    END IF;

    SELECT attestation.*
    INTO attestation_row
    FROM public.runtime_attestations AS attestation
    WHERE attestation.v2_operation_id = expected_operation_id
        AND attestation.attestation_id =
            expected_attestation_digest
        AND attestation.tenant_id = expected_tenant_id
        AND attestation.installation_id = expected_installation_id
        AND attestation.deployment_id = expected_deployment_id
        AND attestation.process_instance_id =
            expected_process_instance_id
        AND attestation.runtime_generation =
            expected_runtime_generation
        AND attestation.v2_initial_lease_epoch =
            expected_lease_epoch
    FOR KEY SHARE;

    observed_at := pg_catalog.clock_timestamp();
    IF attestation_row.attestation_id IS NULL THEN
        outcome_name := 'absent';
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT lease.*
    INTO serving_row
    FROM public.runtime_serving_leases AS lease
    WHERE lease.guild_id = attestation_row.guild_id
        AND lease.ruleset_key = attestation_row.ruleset_key
    FOR SHARE;

    IF serving_row.guild_id IS NULL
        OR serving_row.tenant_id IS DISTINCT FROM expected_tenant_id
        OR serving_row.installation_id
            IS DISTINCT FROM expected_installation_id
        OR serving_row.deployment_id
            IS DISTINCT FROM expected_deployment_id
        OR serving_row.attestation_id
            IS DISTINCT FROM expected_attestation_digest
        OR serving_row.process_instance_id
            IS DISTINCT FROM expected_process_instance_id
        OR serving_row.runtime_generation
            IS DISTINCT FROM expected_runtime_generation
        OR serving_row.lease_epoch IS DISTINCT FROM expected_lease_epoch
        OR serving_row.guild_id IS DISTINCT FROM attestation_row.guild_id
        OR serving_row.ruleset_key
            IS DISTINCT FROM attestation_row.ruleset_key
        OR serving_row.target_version
            IS DISTINCT FROM attestation_row.target_version
        OR serving_row.target_content_hash
            IS DISTINCT FROM attestation_row.target_content_hash
        OR serving_row.binding_revision
            IS DISTINCT FROM attestation_row.binding_revision
        OR serving_row.binding_fingerprint
            IS DISTINCT FROM attestation_row.binding_fingerprint
    THEN
        outcome_name := 'diverged';
        RETURN NEXT;
        RETURN;
    END IF;

    outcome_name := 'current';
    operation_id := expected_operation_id;
    tenant_id := serving_row.tenant_id;
    installation_id := serving_row.installation_id;
    deployment_id := serving_row.deployment_id;
    guild_id := serving_row.guild_id;
    ruleset_key := serving_row.ruleset_key;
    target_version := serving_row.target_version;
    target_content_hash := serving_row.target_content_hash;
    binding_revision := serving_row.binding_revision;
    binding_fingerprint := serving_row.binding_fingerprint;
    attestation_digest := serving_row.attestation_id;
    process_instance_id := serving_row.process_instance_id;
    runtime_generation := serving_row.runtime_generation;
    lease_epoch := serving_row.lease_epoch;
    serving_revision := serving_row.revision;
    acquired_at := serving_row.acquired_at;
    last_heartbeat_at := serving_row.last_heartbeat_at;
    expires_at := serving_row.expires_at;
    connected := serving_row.connected;
    serving := serving_row.serving;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_serving_heartbeat_v2(
    expected_operation_id TEXT,
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_attestation_digest TEXT,
    expected_process_instance_id TEXT,
    expected_runtime_generation BIGINT,
    expected_lease_epoch BIGINT,
    expected_revision BIGINT,
    requested_lease_milliseconds BIGINT
)
RETURNS TABLE(
    operation_id TEXT,
    tenant_id TEXT,
    installation_id TEXT,
    deployment_id TEXT,
    guild_id TEXT,
    ruleset_key TEXT,
    target_version BIGINT,
    target_content_hash TEXT,
    binding_revision BIGINT,
    binding_fingerprint TEXT,
    attestation_digest TEXT,
    process_instance_id TEXT,
    runtime_generation BIGINT,
    lease_epoch BIGINT,
    serving_revision BIGINT,
    acquired_at TIMESTAMPTZ,
    last_heartbeat_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    connected BOOLEAN,
    serving BOOLEAN
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
    attestation_row public.runtime_attestations%ROWTYPE;
    owner_row public.runtime_gateway_owners%ROWTYPE;
    acknowledgement_row
        public.runtime_ingress_open_acknowledgements_v2%ROWTYPE;
    lease_record RECORD;
BEGIN
    IF expected_operation_id !~ '^[0-9a-f]{32}$'
        OR expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_attestation_digest !~ '^[0-9a-f]{64}$'
        OR expected_process_instance_id
            !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_runtime_generation
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_lease_epoch NOT BETWEEN 1 AND 9223372036854775807
        OR expected_revision NOT BETWEEN 1 AND 9223372036854775807
        OR requested_lease_milliseconds NOT BETWEEN 1000 AND 300000
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS002',
            MESSAGE = 'runtime_serving_heartbeat_v2_input_invalid';
    END IF;

    SELECT attestation.*
    INTO attestation_row
    FROM public.runtime_attestations AS attestation
    WHERE attestation.v2_operation_id = expected_operation_id
        AND attestation.attestation_id =
            expected_attestation_digest
        AND attestation.tenant_id = expected_tenant_id
        AND attestation.installation_id = expected_installation_id
        AND attestation.deployment_id = expected_deployment_id
        AND attestation.process_instance_id =
            expected_process_instance_id
        AND attestation.runtime_generation =
            expected_runtime_generation
        AND attestation.v2_initial_lease_epoch =
            expected_lease_epoch
    FOR KEY SHARE;

    IF attestation_row.attestation_id IS NULL
        OR requested_lease_milliseconds * 1000000
            > attestation_row.serving_lease_duration_nanos
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS001',
            MESSAGE = 'runtime_serving_heartbeat_v2_identity_mismatch';
    END IF;

    SELECT owner.*
    INTO owner_row
    FROM public.runtime_gateway_owners AS owner
    WHERE owner.gateway_shard_id =
        attestation_row.v2_route_admission
            #>> '{gateway_owner_lease_id,gateway_shard_id}'
    FOR SHARE;

    SELECT acknowledgement.*
    INTO acknowledgement_row
    FROM public.runtime_ingress_open_acknowledgements_v2
        AS acknowledgement
    WHERE acknowledgement.gateway_shard_id =
        attestation_row.v2_route_admission
            #>> '{gateway_owner_lease_id,gateway_shard_id}'
    FOR SHARE;

    IF owner_row.gateway_shard_id IS NULL
        OR acknowledgement_row.gateway_shard_id IS NULL
        OR owner_row.process_instance_id
            IS DISTINCT FROM expected_process_instance_id
        OR owner_row.lease_epoch::TEXT
            IS DISTINCT FROM attestation_row.v2_route_admission
                #>> '{gateway_owner_lease_id,lease_epoch}'
        OR owner_row.expected_build_revision
            IS DISTINCT FROM attestation_row.v2_route_admission
                #>> '{gateway_owner_lease_id,expected_build_revision}'
        OR owner_row.owner_revision::TEXT
            IS DISTINCT FROM attestation_row.v2_route_admission
                ->> 'attested_owner_revision'
        OR owner_row.expires_at <= pg_catalog.clock_timestamp()
        OR acknowledgement_row.process_instance_id
            IS DISTINCT FROM owner_row.process_instance_id
        OR acknowledgement_row.owner_lease_epoch
            IS DISTINCT FROM owner_row.lease_epoch
        OR acknowledgement_row.expected_build_revision
            IS DISTINCT FROM owner_row.expected_build_revision
        OR acknowledgement_row.observed_owner_revision
            IS DISTINCT FROM owner_row.owner_revision
        OR acknowledgement_row.connection_epoch::TEXT
            IS DISTINCT FROM attestation_row.v2_route_admission
                #>> '{gateway,connection_epoch}'
        OR acknowledgement_row.admission_revision::TEXT
            IS DISTINCT FROM attestation_row.v2_route_admission
                #>> '{gateway,admission_revision}'
        OR acknowledgement_row.connected_event_sequence::TEXT
            IS DISTINCT FROM attestation_row.v2_route_admission
                #>> '{gateway,connected_event_sequence}'
        OR acknowledgement_row.resume_sequence::TEXT
            IS DISTINCT FROM attestation_row.v2_route_admission
                #>> '{gateway,resume_sequence}'
        OR acknowledgement_row.expires_at <= pg_catalog.clock_timestamp()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS001',
            MESSAGE = 'runtime_serving_heartbeat_v2_ingress_mismatch';
    END IF;

    SELECT *
    INTO lease_record
    FROM public.starring_runtime_serving_heartbeat_v1(
        expected_tenant_id,
        expected_installation_id,
        expected_deployment_id,
        expected_attestation_digest,
        expected_process_instance_id,
        expected_runtime_generation,
        expected_lease_epoch,
        expected_revision,
        requested_lease_milliseconds
    );

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS004',
            MESSAGE = 'runtime_serving_heartbeat_v2_result_missing';
    END IF;

    operation_id := expected_operation_id;
    tenant_id := lease_record.tenant_id;
    installation_id := lease_record.installation_id;
    deployment_id := lease_record.deployment_id;
    guild_id := lease_record.guild_id;
    ruleset_key := lease_record.ruleset_key;
    target_version := lease_record.target_version;
    target_content_hash := lease_record.target_content_hash;
    binding_revision := lease_record.binding_revision;
    binding_fingerprint := lease_record.binding_fingerprint;
    attestation_digest := lease_record.attestation_id;
    process_instance_id := lease_record.process_instance_id;
    runtime_generation := lease_record.runtime_generation;
    lease_epoch := lease_record.lease_epoch;
    serving_revision := lease_record.revision;
    acquired_at := lease_record.acquired_at;
    last_heartbeat_at := lease_record.last_heartbeat_at;
    expires_at := lease_record.expires_at;
    connected := lease_record.connected;
    serving := lease_record.serving;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_serving_disconnect_if_current_v2(
    expected_operation_id TEXT,
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_attestation_digest TEXT,
    expected_process_instance_id TEXT,
    expected_runtime_generation BIGINT,
    expected_lease_epoch BIGINT,
    expected_revision BIGINT
)
RETURNS TABLE(
    operation_id TEXT,
    tenant_id TEXT,
    installation_id TEXT,
    deployment_id TEXT,
    guild_id TEXT,
    ruleset_key TEXT,
    target_version BIGINT,
    target_content_hash TEXT,
    binding_revision BIGINT,
    binding_fingerprint TEXT,
    attestation_digest TEXT,
    process_instance_id TEXT,
    runtime_generation BIGINT,
    lease_epoch BIGINT,
    serving_revision BIGINT,
    acquired_at TIMESTAMPTZ,
    last_heartbeat_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    connected BOOLEAN,
    serving BOOLEAN
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
    attestation_row public.runtime_attestations%ROWTYPE;
    lease_record RECORD;
BEGIN
    IF expected_operation_id !~ '^[0-9a-f]{32}$'
        OR expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_attestation_digest !~ '^[0-9a-f]{64}$'
        OR expected_process_instance_id
            !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_runtime_generation
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_lease_epoch NOT BETWEEN 1 AND 9223372036854775807
        OR expected_revision NOT BETWEEN 1 AND 9223372036854775807
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS002',
            MESSAGE = 'runtime_serving_disconnect_v2_input_invalid';
    END IF;

    SELECT attestation.*
    INTO attestation_row
    FROM public.runtime_attestations AS attestation
    WHERE attestation.v2_operation_id = expected_operation_id
        AND attestation.attestation_id =
            expected_attestation_digest
        AND attestation.tenant_id = expected_tenant_id
        AND attestation.installation_id = expected_installation_id
        AND attestation.deployment_id = expected_deployment_id
        AND attestation.process_instance_id =
            expected_process_instance_id
        AND attestation.runtime_generation =
            expected_runtime_generation
        AND attestation.v2_initial_lease_epoch =
            expected_lease_epoch
    FOR KEY SHARE;

    IF attestation_row.attestation_id IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS001',
            MESSAGE = 'runtime_serving_disconnect_v2_identity_mismatch';
    END IF;

    SELECT *
    INTO lease_record
    FROM public.starring_runtime_serving_disconnect_v1(
        expected_tenant_id,
        expected_installation_id,
        expected_deployment_id,
        expected_attestation_digest,
        expected_process_instance_id,
        expected_runtime_generation,
        expected_lease_epoch,
        expected_revision
    );

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS004',
            MESSAGE = 'runtime_serving_disconnect_v2_result_missing';
    END IF;

    operation_id := expected_operation_id;
    tenant_id := lease_record.tenant_id;
    installation_id := lease_record.installation_id;
    deployment_id := lease_record.deployment_id;
    guild_id := lease_record.guild_id;
    ruleset_key := lease_record.ruleset_key;
    target_version := lease_record.target_version;
    target_content_hash := lease_record.target_content_hash;
    binding_revision := lease_record.binding_revision;
    binding_fingerprint := lease_record.binding_fingerprint;
    attestation_digest := lease_record.attestation_id;
    process_instance_id := lease_record.process_instance_id;
    runtime_generation := lease_record.runtime_generation;
    lease_epoch := lease_record.lease_epoch;
    serving_revision := lease_record.revision;
    acquired_at := lease_record.acquired_at;
    last_heartbeat_at := lease_record.last_heartbeat_at;
    expires_at := lease_record.expires_at;
    connected := lease_record.connected;
    serving := lease_record.serving;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_certification_prepare_v2(
    expected_operation_id TEXT,
    expected_intent_fingerprint TEXT,
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_deployment_revision BIGINT,
    expected_controller_id TEXT,
    expected_controller_fencing_token BIGINT,
    expected_runtime_generation BIGINT,
    expected_convergence_attempt_no BIGINT,
    requested_must_commit_before TIMESTAMPTZ
)
RETURNS TABLE(
    outcome_name TEXT,
    locked_snapshot JSONB,
    locked_convergence_attempt_no BIGINT,
    observed_at TIMESTAMPTZ,
    operation_id TEXT,
    certification_intent_bytes BYTEA,
    intent_fingerprint TEXT,
    must_commit_before TIMESTAMPTZ
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
    operation_row public.runtime_certification_operations_v2%ROWTYPE;
    owner_row public.runtime_gateway_owners%ROWTYPE;
    intent JSONB;
    database_now TIMESTAMPTZ;
    authority_outcome TEXT;
BEGIN
    IF pg_catalog.current_setting('transaction_isolation')
            <> 'serializable'
        OR pg_catalog.current_setting('transaction_read_only') <> 'off'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_certification_prepare_v2_transaction_invalid';
    END IF;

    IF expected_operation_id !~ '^[0-9a-f]{32}$'
        OR expected_intent_fingerprint !~ '^[0-9a-f]{64}$'
        OR expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR expected_controller_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_controller_fencing_token
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_runtime_generation
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_convergence_attempt_no NOT BETWEEN 1 AND 4294967295
        OR NOT pg_catalog.isfinite(requested_must_commit_before)
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_certification_prepare_v2_input_invalid';
    END IF;

    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = expected_tenant_id
        AND deployment.installation_id = expected_installation_id
        AND deployment.deployment_id = expected_deployment_id
    FOR UPDATE;

    SELECT operation.*
    INTO operation_row
    FROM public.runtime_certification_operations_v2 AS operation
    WHERE operation.operation_id = expected_operation_id
        AND operation.intent_fingerprint = expected_intent_fingerprint
        AND operation.tenant_id = expected_tenant_id
        AND operation.installation_id = expected_installation_id
        AND operation.deployment_id = expected_deployment_id
        AND operation.deployment_revision = expected_deployment_revision
        AND operation.convergence_attempt_no =
            expected_convergence_attempt_no
    FOR KEY SHARE;

    IF deployment_row.deployment_id IS NULL
        OR operation_row.operation_id IS NULL
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_certification_prepare_v2_ownership_lost';
    END IF;

    BEGIN
        intent := pg_catalog.convert_from(
            operation_row.certification_intent_bytes,
            'UTF8'
        )::JSONB;
    EXCEPTION
        WHEN OTHERS THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_certification_prepare_v2_intent_invalid';
    END;

    database_now := public.starring_runtime_mutation_clock();
    IF requested_must_commit_before <= database_now
        OR requested_must_commit_before
            > database_now + INTERVAL '30 seconds'
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
        OR deployment_row.controller_lease_expires_at
            <= requested_must_commit_before
        OR intent ->> 'format_version' IS DISTINCT FROM '2'
        OR intent ->> 'operation_id'
            IS DISTINCT FROM expected_operation_id
        OR intent #>> '{guard,scope,tenant_id}'
            IS DISTINCT FROM expected_tenant_id
        OR intent #>> '{guard,scope,installation_id}'
            IS DISTINCT FROM expected_installation_id
        OR intent #>> '{guard,scope,deployment_id}'
            IS DISTINCT FROM expected_deployment_id
        OR intent #>> '{guard,expected_revision}'
            IS DISTINCT FROM expected_deployment_revision::TEXT
        OR intent #>> '{guard,controller_id}'
            IS DISTINCT FROM expected_controller_id
        OR intent #>> '{guard,fencing_token}'
            IS DISTINCT FROM expected_controller_fencing_token::TEXT
        OR intent #>> '{guard,runtime_generation}'
            IS DISTINCT FROM expected_runtime_generation::TEXT
        OR intent #>> '{guard,convergence_attempt}'
            IS DISTINCT FROM expected_convergence_attempt_no::TEXT
        OR intent -> 'target'
            IS DISTINCT FROM deployment_row.snapshot -> 'target'
        OR intent #>> '{panel,certificate_id}'
            IS DISTINCT FROM deployment_row.snapshot
                #>> '{panel_certificate,certificate_id}'
        OR intent #>> '{panel,report_digest}'
            IS DISTINCT FROM deployment_row.snapshot
                #>> '{panel_certificate,report_digest}'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_certification_prepare_v2_state_mismatch';
    END IF;

    SELECT owner.*
    INTO owner_row
    FROM public.runtime_gateway_owners AS owner
    WHERE owner.gateway_shard_id =
        intent #>> '{gateway_owner_lease_id,gateway_shard_id}'
    FOR SHARE;

    IF owner_row.gateway_shard_id IS NULL
        OR owner_row.process_instance_id
            IS DISTINCT FROM intent
                #>> '{gateway_owner_lease_id,process_instance_id}'
        OR owner_row.lease_epoch::TEXT
            IS DISTINCT FROM intent
                #>> '{gateway_owner_lease_id,lease_epoch}'
        OR owner_row.expected_build_revision
            IS DISTINCT FROM intent
                #>> '{gateway_owner_lease_id,expected_build_revision}'
        OR owner_row.owner_revision::TEXT
            IS DISTINCT FROM intent ->> 'observed_owner_revision'
        OR owner_row.expires_at <= requested_must_commit_before
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_certification_prepare_v2_owner_mismatch';
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
            MESSAGE = 'runtime_certification_prepare_v2_superseded';
    ELSIF authority_outcome IS DISTINCT FROM 'exact' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_certification_prepare_v2_authority_changed';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM public.runtime_certification_operation_terminals_v2 AS terminal
        WHERE terminal.operation_id = expected_operation_id
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX005',
            MESSAGE = 'runtime_certification_prepare_v2_already_terminal';
    END IF;

    outcome_name := 'prepared';
    locked_snapshot := deployment_row.snapshot;
    locked_convergence_attempt_no := expected_convergence_attempt_no;
    observed_at := database_now;
    operation_id := operation_row.operation_id;
    certification_intent_bytes :=
        operation_row.certification_intent_bytes;
    intent_fingerprint := operation_row.intent_fingerprint;
    must_commit_before := requested_must_commit_before;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_certification_commit_v2(
    expected_operation_id TEXT,
    expected_intent_fingerprint TEXT,
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_deployment_revision BIGINT,
    expected_controller_id TEXT,
    expected_controller_fencing_token BIGINT,
    expected_runtime_generation BIGINT,
    expected_convergence_attempt_no BIGINT,
    proposed_request_bytes BYTEA,
    proposed_request_digest TEXT,
    proposed_live_attestation_bytes BYTEA,
    proposed_live_attestation_digest TEXT
)
RETURNS TABLE(
    outcome_name TEXT,
    previous_snapshot JSONB,
    snapshot JSONB,
    convergence_attempt_no BIGINT,
    operation_id TEXT,
    intent_fingerprint TEXT,
    certification_request_bytes BYTEA,
    request_digest TEXT,
    live_attestation_record_bytes BYTEA,
    attestation_digest TEXT,
    route_admission JSONB,
    tenant_id TEXT,
    installation_id TEXT,
    deployment_id TEXT,
    guild_id TEXT,
    ruleset_key TEXT,
    process_instance_id TEXT,
    runtime_generation BIGINT,
    lease_epoch BIGINT,
    serving_revision BIGINT,
    acquired_at TIMESTAMPTZ,
    last_heartbeat_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    connected BOOLEAN,
    serving BOOLEAN,
    certified_at TIMESTAMPTZ
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
    operation_row public.runtime_certification_operations_v2%ROWTYPE;
    terminal_row public.runtime_certification_operation_terminals_v2%ROWTYPE;
    attestation_row public.runtime_attestations%ROWTYPE;
    serving_row public.runtime_serving_leases%ROWTYPE;
    owner_row public.runtime_gateway_owners%ROWTYPE;
    slot_fence_row public.runtime_slot_writer_fences_v2%ROWTYPE;
    writer_fence_row public.runtime_writer_fence%ROWTYPE;
    intent JSONB;
    request_record JSONB;
    live_record JSONB;
    route_record JSONB;
    next_snapshot JSONB;
    live_value JSONB;
    database_now TIMESTAMPTZ;
    must_commit_microseconds BIGINT;
    must_commit_before_value TIMESTAMPTZ;
    serving_lease_milliseconds BIGINT;
    pause_coordinator_generation BIGINT;
    connected_event_sequence BIGINT;
    pause_sequence BIGINT;
    resume_sequence BIGINT;
    route_incarnation BIGINT;
    route_activation_sequence BIGINT;
    next_lease_epoch BIGINT;
    next_serving_revision BIGINT;
    next_expiry TIMESTAMPTZ;
    authority_outcome TEXT;
    terminal_digest TEXT;
    setting_name TEXT;
BEGIN
    IF pg_catalog.current_setting('transaction_isolation')
            <> 'serializable'
        OR pg_catalog.current_setting('transaction_read_only') <> 'off'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_certification_commit_v2_transaction_invalid';
    END IF;

    IF expected_operation_id !~ '^[0-9a-f]{32}$'
        OR expected_intent_fingerprint !~ '^[0-9a-f]{64}$'
        OR expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR expected_controller_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_controller_fencing_token
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_runtime_generation
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_convergence_attempt_no NOT BETWEEN 1 AND 4294967295
        OR pg_catalog.octet_length(proposed_request_bytes)
            NOT BETWEEN 1 AND 65536
        OR proposed_request_digest !~ '^[0-9a-f]{64}$'
        OR pg_catalog.octet_length(proposed_live_attestation_bytes)
            NOT BETWEEN 1 AND 131072
        OR proposed_live_attestation_digest !~ '^[0-9a-f]{64}$'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_certification_commit_v2_input_invalid';
    END IF;

    IF proposed_request_digest IS DISTINCT FROM
            starring_runtime_private_v2.starring_runtime_framed_digest_v2(
                pg_catalog.convert_to(
                    'starring.runtime.certification_request.v2',
                    'UTF8'
                ) || pg_catalog.decode('00', 'hex'),
                proposed_request_bytes
            )
        OR proposed_live_attestation_digest IS DISTINCT FROM
            starring_runtime_private_v2.starring_runtime_framed_digest_v2(
                pg_catalog.convert_to(
                    'starring.runtime.live_attestation.v2',
                    'UTF8'
                ) || pg_catalog.decode('00', 'hex'),
                proposed_live_attestation_bytes
            )
        OR proposed_live_attestation_bytes IS DISTINCT FROM
            pg_catalog.convert_to(
                '{"format_version":2,"request_digest":"'
                    || proposed_request_digest
                    || '","request":',
                'UTF8'
            )
            || proposed_request_bytes
            || pg_catalog.convert_to('}', 'UTF8')
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_certification_commit_v2_digest_invalid';
    END IF;

    BEGIN
        request_record := pg_catalog.convert_from(
            proposed_request_bytes,
            'UTF8'
        )::JSONB;
        live_record := pg_catalog.convert_from(
            proposed_live_attestation_bytes,
            'UTF8'
        )::JSONB;
        must_commit_microseconds := (
            request_record ->> 'must_commit_before_unix_microseconds'
        )::BIGINT;
        must_commit_before_value :=
            TIMESTAMPTZ 'epoch'
            + must_commit_microseconds * INTERVAL '1 microsecond';
        serving_lease_milliseconds := (
            request_record #>> '{intent,serving_lease_milliseconds}'
        )::BIGINT;
        pause_coordinator_generation := (
            request_record
                #>> '{route_admission,pause,coordinator_generation}'
        )::BIGINT;
        connected_event_sequence := (
            request_record
                #>> '{route_admission,gateway,connected_event_sequence}'
        )::BIGINT;
        pause_sequence := (
            request_record
                #>> '{route_admission,pause,pause_sequence}'
        )::BIGINT;
        resume_sequence := (
            request_record
                #>> '{route_admission,gateway,resume_sequence}'
        )::BIGINT;
        route_incarnation := (
            request_record
                #>> '{route_admission,route,route_incarnation}'
        )::BIGINT;
        route_activation_sequence := (
            request_record
                #>> '{route_admission,route,activation_sequence}'
        )::BIGINT;
    EXCEPTION
        WHEN OTHERS THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX002',
                MESSAGE = 'runtime_certification_commit_v2_encoding_invalid';
    END;

    intent := request_record -> 'intent';
    route_record := request_record -> 'route_admission';
    IF request_record ->> 'format_version' IS DISTINCT FROM '2'
        OR live_record ->> 'format_version' IS DISTINCT FROM '2'
        OR live_record ->> 'request_digest'
            IS DISTINCT FROM proposed_request_digest
        OR live_record -> 'request' IS DISTINCT FROM request_record
        OR pg_catalog.jsonb_typeof(intent) <> 'object'
        OR pg_catalog.jsonb_typeof(route_record) <> 'object'
        OR request_record ->> 'intent_fingerprint'
            IS DISTINCT FROM expected_intent_fingerprint
        OR intent ->> 'operation_id'
            IS DISTINCT FROM expected_operation_id
        OR intent #>> '{guard,scope,tenant_id}'
            IS DISTINCT FROM expected_tenant_id
        OR intent #>> '{guard,scope,installation_id}'
            IS DISTINCT FROM expected_installation_id
        OR intent #>> '{guard,scope,deployment_id}'
            IS DISTINCT FROM expected_deployment_id
        OR intent #>> '{guard,expected_revision}'
            IS DISTINCT FROM expected_deployment_revision::TEXT
        OR intent #>> '{guard,controller_id}'
            IS DISTINCT FROM expected_controller_id
        OR intent #>> '{guard,fencing_token}'
            IS DISTINCT FROM expected_controller_fencing_token::TEXT
        OR intent #>> '{guard,runtime_generation}'
            IS DISTINCT FROM expected_runtime_generation::TEXT
        OR intent #>> '{guard,convergence_attempt}'
            IS DISTINCT FROM expected_convergence_attempt_no::TEXT
        OR serving_lease_milliseconds NOT BETWEEN 1000 AND 300000
        OR NOT pg_catalog.isfinite(must_commit_before_value)
        OR route_record ->> 'barrier_id' !~ '^[0-9a-f]{32}$'
        OR route_record #>> '{gateway,kind}' IS DISTINCT FROM 'resumed'
        OR route_record #>> '{gateway,process_instance_id}'
            IS DISTINCT FROM intent
                #>> '{process_identity,process_instance_id}'
        OR route_record #> '{route,identity}'
            IS DISTINCT FROM intent -> 'process_identity'
        OR route_record #>> '{route,controller_fencing_token}'
            IS DISTINCT FROM expected_controller_fencing_token::TEXT
        OR route_record
                #>> '{gateway_owner_lease_id,gateway_shard_id}'
            IS DISTINCT FROM intent
                #>> '{gateway_owner_lease_id,gateway_shard_id}'
        OR route_record
                #>> '{gateway_owner_lease_id,process_instance_id}'
            IS DISTINCT FROM intent
                #>> '{gateway_owner_lease_id,process_instance_id}'
        OR route_record
                #>> '{gateway_owner_lease_id,lease_epoch}'
            IS DISTINCT FROM intent
                #>> '{gateway_owner_lease_id,lease_epoch}'
        OR route_record
                #>> '{gateway_owner_lease_id,expected_build_revision}'
            IS DISTINCT FROM intent
                #>> '{gateway_owner_lease_id,expected_build_revision}'
        OR route_record ->> 'attested_owner_revision'
            IS DISTINCT FROM intent ->> 'observed_owner_revision'
        OR route_record #>> '{pause,connection_epoch}'
            IS DISTINCT FROM route_record
                #>> '{gateway,connection_epoch}'
        OR route_record #>> '{pause,paused_admission_revision}'
            IS DISTINCT FROM route_record
                #>> '{gateway,admission_revision}'
        OR connected_event_sequence
            NOT BETWEEN 1 AND 9223372036854775807
        OR pause_sequence NOT BETWEEN 1 AND 9223372036854775807
        OR resume_sequence NOT BETWEEN 1 AND 9223372036854775807
        OR connected_event_sequence >= pause_sequence
        OR pause_sequence >= resume_sequence
        OR pause_coordinator_generation
            NOT BETWEEN 1 AND 9223372036854775807
        OR route_incarnation
            NOT BETWEEN 1 AND 9223372036854775807
        OR route_activation_sequence
            NOT BETWEEN 1 AND 9223372036854775807
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_certification_commit_v2_record_invalid';
    END IF;

    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = expected_tenant_id
        AND deployment.installation_id = expected_installation_id
        AND deployment.deployment_id = expected_deployment_id
    FOR UPDATE;

    SELECT operation.*
    INTO operation_row
    FROM public.runtime_certification_operations_v2 AS operation
    WHERE operation.operation_id = expected_operation_id
        AND operation.intent_fingerprint = expected_intent_fingerprint
        AND operation.tenant_id = expected_tenant_id
        AND operation.installation_id = expected_installation_id
        AND operation.deployment_id = expected_deployment_id
        AND operation.deployment_revision = expected_deployment_revision
        AND operation.convergence_attempt_no =
            expected_convergence_attempt_no
    FOR KEY SHARE;

    SELECT terminal.*
    INTO terminal_row
    FROM public.runtime_certification_operation_terminals_v2 AS terminal
    WHERE terminal.operation_id = expected_operation_id
    FOR KEY SHARE;

    IF deployment_row.deployment_id IS NULL
        OR operation_row.operation_id IS NULL
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_certification_commit_v2_ownership_lost';
    END IF;

    IF request_record -> 'intent' IS DISTINCT FROM
            pg_catalog.convert_from(
                operation_row.certification_intent_bytes,
                'UTF8'
            )::JSONB
        OR operation_row.intent_fingerprint
            IS DISTINCT FROM expected_intent_fingerprint
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_certification_commit_v2_reservation_mismatch';
    END IF;

    IF terminal_row.operation_id IS NOT NULL THEN
        SELECT attestation.*
        INTO attestation_row
        FROM public.runtime_attestations AS attestation
        WHERE attestation.v2_operation_id = expected_operation_id
            AND attestation.v2_request_digest =
                proposed_request_digest
            AND attestation.attestation_id =
                proposed_live_attestation_digest
        FOR KEY SHARE;
        IF terminal_row.terminal_outcome_name
                IS DISTINCT FROM 'certification_committed'
            OR attestation_row.attestation_id IS NULL
            OR attestation_row.v2_intent_fingerprint
                IS DISTINCT FROM expected_intent_fingerprint
            OR attestation_row.v2_request_bytes
                IS DISTINCT FROM proposed_request_bytes
            OR attestation_row.v2_live_attestation_bytes
                IS DISTINCT FROM proposed_live_attestation_bytes
            OR terminal_row.terminal_receipt_bytes
                IS DISTINCT FROM proposed_live_attestation_bytes
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE =
                    'runtime_certification_commit_v2_replay_mismatch';
        END IF;

        outcome_name := 'replayed';
        previous_snapshot := attestation_row.v2_prepared_snapshot;
        snapshot := attestation_row.v2_certified_snapshot;
        convergence_attempt_no :=
            attestation_row.convergence_attempt_no;
        operation_id := attestation_row.v2_operation_id;
        intent_fingerprint :=
            attestation_row.v2_intent_fingerprint;
        certification_request_bytes :=
            attestation_row.v2_request_bytes;
        request_digest := attestation_row.v2_request_digest;
        live_attestation_record_bytes :=
            attestation_row.v2_live_attestation_bytes;
        attestation_digest := attestation_row.attestation_id;
        route_admission := attestation_row.v2_route_admission;
        tenant_id := attestation_row.tenant_id;
        installation_id := attestation_row.installation_id;
        deployment_id := attestation_row.deployment_id;
        guild_id := attestation_row.guild_id;
        ruleset_key := attestation_row.ruleset_key;
        process_instance_id := attestation_row.process_instance_id;
        runtime_generation := attestation_row.runtime_generation;
        lease_epoch := attestation_row.v2_initial_lease_epoch;
        serving_revision :=
            attestation_row.v2_initial_serving_revision;
        acquired_at := attestation_row.certified_at;
        last_heartbeat_at := attestation_row.certified_at;
        expires_at := attestation_row.certified_at
            + (
                attestation_row.serving_lease_duration_nanos
                    / 1000000
            ) * INTERVAL '1 millisecond';
        connected := TRUE;
        serving := TRUE;
        certified_at := attestation_row.certified_at;
        RETURN NEXT;
        RETURN;
    END IF;

    database_now := public.starring_runtime_mutation_clock();
    IF database_now > must_commit_before_value
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
        OR deployment_row.controller_lease_expires_at
            <= database_now
        OR intent -> 'target'
            IS DISTINCT FROM deployment_row.snapshot -> 'target'
        OR intent #>> '{panel,certificate_id}'
            IS DISTINCT FROM deployment_row.snapshot
                #>> '{panel_certificate,certificate_id}'
        OR intent #>> '{panel,report_digest}'
            IS DISTINCT FROM deployment_row.snapshot
                #>> '{panel_certificate,report_digest}'
        OR deployment_row.snapshot -> 'gateway_ready' IS NULL
        OR deployment_row.snapshot -> 'gateway_ready' = 'null'::JSONB
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_certification_commit_v2_state_mismatch';
    END IF;

    SELECT fence.*
    INTO writer_fence_row
    FROM public.runtime_writer_fence AS fence
    WHERE fence.singleton
    FOR SHARE;

    SELECT fence.*
    INTO slot_fence_row
    FROM public.runtime_slot_writer_fences_v2 AS fence
    WHERE fence.slot_guild_id = deployment_row.guild_id
        AND fence.slot_ruleset_key = deployment_row.ruleset_key
    FOR UPDATE;

    IF writer_fence_row.fence_state IS DISTINCT FROM 'open'
        OR slot_fence_row.slot_guild_id IS NULL
        OR slot_fence_row.pending_drain_intent_id IS NOT NULL
        OR slot_fence_row.pending_product_operation_id IS NOT NULL
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_certification_commit_v2_writer_fenced';
    END IF;

    SELECT owner.*
    INTO owner_row
    FROM public.runtime_gateway_owners AS owner
    WHERE owner.gateway_shard_id =
        route_record
            #>> '{gateway_owner_lease_id,gateway_shard_id}'
    FOR SHARE;

    IF owner_row.gateway_shard_id IS NULL
        OR owner_row.process_instance_id
            IS DISTINCT FROM route_record
                #>> '{gateway_owner_lease_id,process_instance_id}'
        OR owner_row.lease_epoch::TEXT
            IS DISTINCT FROM route_record
                #>> '{gateway_owner_lease_id,lease_epoch}'
        OR owner_row.expected_build_revision
            IS DISTINCT FROM route_record
                #>> '{gateway_owner_lease_id,expected_build_revision}'
        OR owner_row.owner_revision::TEXT
            IS DISTINCT FROM route_record
                ->> 'attested_owner_revision'
        OR owner_row.expires_at <= database_now
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_certification_commit_v2_owner_mismatch';
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
            MESSAGE = 'runtime_certification_commit_v2_superseded';
    ELSIF authority_outcome IS DISTINCT FROM 'exact' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_certification_commit_v2_authority_changed';
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
    SELECT lease.*
    INTO serving_row
    FROM public.runtime_serving_leases AS lease
    WHERE lease.guild_id = deployment_row.guild_id
        AND lease.ruleset_key = deployment_row.ruleset_key
    FOR UPDATE;

    IF serving_row.guild_id IS NOT NULL
        AND serving_row.expires_at > database_now
        AND (serving_row.connected OR serving_row.serving)
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_certification_commit_v2_serving_conflict';
    END IF;

    IF serving_row.guild_id IS NULL THEN
        next_lease_epoch := 1;
        next_serving_revision := 1;
    ELSE
        IF serving_row.lease_epoch = 9223372036854775807
            OR serving_row.revision = 9223372036854775807
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_certification_commit_v2_serving_overflow';
        END IF;
        next_lease_epoch := serving_row.lease_epoch + 1;
        next_serving_revision := serving_row.revision + 1;
    END IF;

    live_value := pg_catalog.jsonb_build_object(
        'target', deployment_row.snapshot -> 'target',
        'runtime_generation', expected_runtime_generation,
        'process_instance_id',
            intent #>> '{process_identity,process_instance_id}',
        'activation', deployment_row.snapshot -> 'activation',
        'panel_certificate',
            deployment_row.snapshot -> 'panel_certificate',
        'gateway_ready', deployment_row.snapshot -> 'gateway_ready',
        'certified_at', pg_catalog.to_jsonb(database_now)
    );
    next_snapshot := pg_catalog.jsonb_set(
        deployment_row.snapshot,
        '{revision}',
        pg_catalog.to_jsonb(expected_deployment_revision + 1),
        FALSE
    );
    next_snapshot := pg_catalog.jsonb_set(
        next_snapshot,
        '{phase}',
        '{"phase":"live"}'::JSONB,
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
        '{live}',
        live_value,
        FALSE
    );

    INSERT INTO public.runtime_attestations (
        attestation_id,
        attestation_digest,
        deployment_id,
        deployment_revision,
        tenant_id,
        installation_id,
        promotion_id,
        activation_request_id,
        guild_id,
        ruleset_key,
        target_version,
        target_content_hash,
        binding_revision,
        binding_fingerprint,
        runtime_generation,
        controller_fencing_token,
        process_instance_id,
        runtime_build_revision,
        panel_certificate_id,
        panel_report_digest,
        gateway_shard_id,
        gateway_ready_kind,
        gateway_ready_at,
        certified_at,
        record_format_version,
        record,
        created_at,
        convergence_attempt_no,
        serving_lease_duration_nanos,
        v2_operation_id,
        v2_intent_fingerprint,
        v2_request_digest,
        v2_request_bytes,
        v2_live_attestation_bytes,
        v2_must_commit_before,
        v2_route_admission,
        v2_route_incarnation,
        v2_route_activation_sequence,
        v2_initial_lease_epoch,
        v2_initial_serving_revision,
        v2_prepared_snapshot,
        v2_certified_snapshot
    )
    VALUES (
        proposed_live_attestation_digest,
        proposed_live_attestation_digest,
        expected_deployment_id,
        expected_deployment_revision + 1,
        expected_tenant_id,
        expected_installation_id,
        deployment_row.promotion_id,
        deployment_row.activation_request_id,
        deployment_row.guild_id,
        deployment_row.ruleset_key,
        deployment_row.target_version,
        deployment_row.target_content_hash,
        deployment_row.binding_revision,
        deployment_row.binding_fingerprint,
        expected_runtime_generation,
        expected_controller_fencing_token,
        intent #>> '{process_identity,process_instance_id}',
        intent ->> 'runtime_build_revision',
        intent #>> '{panel,certificate_id}',
        intent #>> '{panel,report_digest}',
        intent #>> '{gateway_owner_lease_id,gateway_shard_id}',
        'discord_resumed',
        database_now,
        database_now,
        2,
        live_record,
        database_now,
        expected_convergence_attempt_no,
        serving_lease_milliseconds * 1000000,
        expected_operation_id,
        expected_intent_fingerprint,
        proposed_request_digest,
        proposed_request_bytes,
        proposed_live_attestation_bytes,
        must_commit_before_value,
        route_record,
        route_incarnation,
        route_activation_sequence,
        next_lease_epoch,
        next_serving_revision,
        deployment_row.snapshot,
        next_snapshot
    )
    RETURNING * INTO attestation_row;

    next_expiry := database_now
        + serving_lease_milliseconds * INTERVAL '1 millisecond';
    IF serving_row.guild_id IS NULL THEN
        INSERT INTO public.runtime_serving_leases (
            guild_id,
            ruleset_key,
            tenant_id,
            installation_id,
            deployment_id,
            attestation_id,
            process_instance_id,
            runtime_generation,
            target_version,
            target_content_hash,
            binding_revision,
            binding_fingerprint,
            lease_epoch,
            revision,
            connected,
            serving,
            acquired_at,
            last_heartbeat_at,
            expires_at
        )
        VALUES (
            deployment_row.guild_id,
            deployment_row.ruleset_key,
            expected_tenant_id,
            expected_installation_id,
            expected_deployment_id,
            proposed_live_attestation_digest,
            attestation_row.process_instance_id,
            expected_runtime_generation,
            deployment_row.target_version,
            deployment_row.target_content_hash,
            deployment_row.binding_revision,
            deployment_row.binding_fingerprint,
            next_lease_epoch,
            next_serving_revision,
            TRUE,
            TRUE,
            database_now,
            database_now,
            next_expiry
        )
        RETURNING * INTO serving_row;
    ELSE
        UPDATE public.runtime_serving_leases AS lease
        SET tenant_id = expected_tenant_id,
            installation_id = expected_installation_id,
            deployment_id = expected_deployment_id,
            attestation_id = proposed_live_attestation_digest,
            process_instance_id = attestation_row.process_instance_id,
            runtime_generation = expected_runtime_generation,
            target_version = deployment_row.target_version,
            target_content_hash = deployment_row.target_content_hash,
            binding_revision = deployment_row.binding_revision,
            binding_fingerprint = deployment_row.binding_fingerprint,
            lease_epoch = next_lease_epoch,
            revision = next_serving_revision,
            connected = TRUE,
            serving = TRUE,
            acquired_at = database_now,
            last_heartbeat_at = database_now,
            expires_at = next_expiry
        WHERE lease.guild_id = deployment_row.guild_id
            AND lease.ruleset_key = deployment_row.ruleset_key
        RETURNING * INTO serving_row;
    END IF;

    UPDATE public.runtime_deployments AS deployment
    SET snapshot = next_snapshot,
        revision = expected_deployment_revision + 1,
        phase = 'live',
        controller_id = NULL,
        controller_fencing_token = NULL,
        controller_acquired_at = NULL,
        controller_lease_expires_at = NULL,
        live_attestation_id = proposed_live_attestation_digest,
        live_at = database_now,
        last_controller_id = expected_controller_id,
        updated_at = GREATEST(
            database_now,
            deployment.updated_at + INTERVAL '1 microsecond'
        )
    WHERE deployment.tenant_id = expected_tenant_id
        AND deployment.installation_id = expected_installation_id
        AND deployment.deployment_id = expected_deployment_id
        AND deployment.revision = expected_deployment_revision;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_certification_commit_v2_ownership_lost';
    END IF;

    terminal_digest :=
        starring_runtime_private_v2.starring_runtime_certification_terminal_digest_v2(
            2::SMALLINT,
            expected_operation_id,
            expected_intent_fingerprint,
            expected_tenant_id,
            expected_installation_id,
            expected_deployment_id,
            expected_deployment_revision,
            expected_convergence_attempt_no,
            'certification_committed',
            'live',
            expected_deployment_revision + 1,
            expected_convergence_attempt_no,
            database_now,
            proposed_live_attestation_bytes
        );
    PERFORM pg_catalog.set_config(
        'starring.runtime_certification_terminal_action_v2',
        'insert',
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_certification_terminal_operation_id_v2',
        expected_operation_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_certification_terminal_outcome_v2',
        'certification_committed',
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_certification_terminal_result_phase_v2',
        'live',
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_certification_terminal_result_revision_v2',
        (expected_deployment_revision + 1)::TEXT,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_certification_terminal_result_attempt_v2',
        expected_convergence_attempt_no::TEXT,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_certification_terminal_digest_v2',
        terminal_digest,
        TRUE
    );

    INSERT INTO public.runtime_certification_operation_terminals_v2 (
        record_format_version,
        operation_id,
        intent_fingerprint,
        tenant_id,
        installation_id,
        deployment_id,
        deployment_revision,
        convergence_attempt_no,
        terminal_outcome_name,
        resulting_phase,
        resulting_deployment_revision,
        resulting_convergence_attempt_no,
        terminal_at,
        terminal_receipt_bytes,
        terminal_receipt_digest
    )
    VALUES (
        2,
        expected_operation_id,
        expected_intent_fingerprint,
        expected_tenant_id,
        expected_installation_id,
        expected_deployment_id,
        expected_deployment_revision,
        expected_convergence_attempt_no,
        'certification_committed',
        'live',
        expected_deployment_revision + 1,
        expected_convergence_attempt_no,
        database_now,
        proposed_live_attestation_bytes,
        terminal_digest
    );

    FOREACH setting_name IN ARRAY ARRAY[
        'starring.runtime_certification_terminal_action_v2',
        'starring.runtime_certification_terminal_operation_id_v2',
        'starring.runtime_certification_terminal_outcome_v2',
        'starring.runtime_certification_terminal_result_phase_v2',
        'starring.runtime_certification_terminal_result_revision_v2',
        'starring.runtime_certification_terminal_result_attempt_v2',
        'starring.runtime_certification_terminal_digest_v2'
    ]
    LOOP
        PERFORM pg_catalog.set_config(setting_name, '', TRUE);
    END LOOP;

    outcome_name := 'applied';
    previous_snapshot := deployment_row.snapshot;
    snapshot := next_snapshot;
    convergence_attempt_no := expected_convergence_attempt_no;
    operation_id := expected_operation_id;
    intent_fingerprint := expected_intent_fingerprint;
    certification_request_bytes := proposed_request_bytes;
    request_digest := proposed_request_digest;
    live_attestation_record_bytes :=
        proposed_live_attestation_bytes;
    attestation_digest := proposed_live_attestation_digest;
    route_admission := route_record;
    tenant_id := expected_tenant_id;
    installation_id := expected_installation_id;
    deployment_id := expected_deployment_id;
    guild_id := serving_row.guild_id;
    ruleset_key := serving_row.ruleset_key;
    process_instance_id := serving_row.process_instance_id;
    runtime_generation := serving_row.runtime_generation;
    lease_epoch := serving_row.lease_epoch;
    serving_revision := serving_row.revision;
    acquired_at := serving_row.acquired_at;
    last_heartbeat_at := serving_row.last_heartbeat_at;
    expires_at := serving_row.expires_at;
    connected := serving_row.connected;
    serving := serving_row.serving;
    certified_at := database_now;
    RETURN NEXT;
END;
$function$;

DO $patch_schema_manifests$
DECLARE
    definition TEXT;
    marker TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(
        'public.starring_runtime_execution_schema_manifest_v1()'
            ::REGPROCEDURE
    )
    INTO definition;
    marker := E'    ), manifest(value) AS (';
    IF pg_catalog.strpos(definition, marker) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, marker, ''),
            marker
        ) <> 0
        OR pg_catalog.strpos(
            definition,
            'RETURN observed_count = 948'
        ) = 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_certification_v2_execution_manifest_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        marker,
        E'        UNION\n'
            || E'        SELECT pg_catalog.to_regprocedure(\n'
            || E'            ''public.starring_runtime_certification_prepare_v2(text,text,text,text,text,bigint,text,bigint,bigint,bigint,timestamp with time zone)''\n'
            || E'        )\n'
            || E'        UNION\n'
            || E'        SELECT pg_catalog.to_regprocedure(\n'
            || E'            ''public.starring_runtime_certification_commit_v2(text,text,text,text,text,bigint,text,bigint,bigint,bigint,bytea,text,bytea,text)''\n'
            || E'        )\n'
            || E'        UNION\n'
            || E'        SELECT pg_catalog.to_regprocedure(\n'
            || E'            ''public.starring_runtime_certification_observe_v2(text,text,text,text,bigint,bigint,text)''\n'
            || E'        )\n'
            || marker
    );
    definition := pg_catalog.replace(
        definition,
        E'    RETURN observed_count = 948\n'
            || E'        AND observed_digest\n'
            || E'            = ''bd8e47e52db30d06ac726b2763a20f54b993f1e04c374975a96a510a31919ade'';',
        E'    RETURN observed_count = 967\n'
            || E'        AND observed_digest\n'
            || E'            = ''3253c6549e25637015c6640748faccd2fac0e3368e84a2b34b7755611a5d208b'';'
    );
    EXECUTE definition;

    SELECT pg_catalog.pg_get_functiondef(
        'public.starring_runtime_serving_schema_manifest_v1()'
            ::REGPROCEDURE
    )
    INTO definition;
    marker := E'    ), permitted_external_index(index_oid) AS (';
    IF pg_catalog.strpos(definition, marker) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, marker, ''),
            marker
        ) <> 0
        OR pg_catalog.strpos(
            definition,
            'RETURN observed_count = 471'
        ) = 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_certification_v2_serving_manifest_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        marker,
        E'        UNION\n'
            || E'        SELECT pg_catalog.to_regprocedure(\n'
            || E'            ''public.starring_runtime_serving_observe_v2(text,text,text,text,text,text,bigint,bigint)''\n'
            || E'        )\n'
            || E'        UNION\n'
            || E'        SELECT pg_catalog.to_regprocedure(\n'
            || E'            ''public.starring_runtime_serving_heartbeat_v2(text,text,text,text,text,text,bigint,bigint,bigint,bigint)''\n'
            || E'        )\n'
            || E'        UNION\n'
            || E'        SELECT pg_catalog.to_regprocedure(\n'
            || E'            ''public.starring_runtime_serving_disconnect_if_current_v2(text,text,text,text,text,text,bigint,bigint,bigint)''\n'
            || E'        )\n'
            || marker
    );
    definition := pg_catalog.replace(
        definition,
        E'    RETURN observed_count = 471\n'
            || E'        AND observed_digest\n'
            || E'            = ''ae127076f030fd9d5f38f1fc8403b00ba91503e96bf152624dfd8e968f74012c'';',
        E'    RETURN observed_count = 490\n'
            || E'        AND observed_digest\n'
            || E'            = ''66cf1f0613f92e03f3420cc89a700365a6ac238224275fb26107829c13569e36'';'
    );
    EXECUTE definition;
END;
$patch_schema_manifests$;

DO $patch_readiness$
DECLARE
    definition TEXT;
    contract_marker TEXT;
    allowlist_marker TEXT;
    contract_rows TEXT := '';
    allowlist_rows TEXT := '';
    function_identity TEXT;
    function_arguments TEXT;
    function_result TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(
        'public.starring_runtime_execution_database_readiness_v1()'
            ::REGPROCEDURE
    )
    INTO definition;

    FOREACH function_identity IN ARRAY ARRAY[
        'public.starring_runtime_certification_prepare_v2(text,text,text,text,text,bigint,text,bigint,bigint,bigint,timestamp with time zone)',
        'public.starring_runtime_certification_commit_v2(text,text,text,text,text,bigint,text,bigint,bigint,bigint,bytea,text,bytea,text)',
        'public.starring_runtime_certification_observe_v2(text,text,text,text,bigint,bigint,text)'
    ]::TEXT[]
    LOOP
        SELECT
            pg_catalog.pg_get_function_identity_arguments(
                function_row.oid
            ),
            pg_catalog.pg_get_function_result(function_row.oid)
        INTO function_arguments, function_result
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid =
            pg_catalog.to_regprocedure(function_identity);

        IF function_arguments IS NULL OR function_result IS NULL THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE =
                    'runtime_certification_v2_execution_contract_missing';
        END IF;

        contract_rows := contract_rows || pg_catalog.format(
            E',\n            (\n'
                || E'                %L,\n'
                || E'                %L::TEXT,\n'
                || E'                %L::TEXT,\n'
                || E'                ''plpgsql''::TEXT,\n'
                || E'                TRUE,\n'
                || E'                TRUE,\n'
                || E'                1::REAL\n'
                || E'            )',
            function_identity,
            function_arguments,
            function_result
        );
        allowlist_rows := allowlist_rows || pg_catalog.format(
            E',\n            pg_catalog.to_regprocedure(\n'
                || E'                %L\n'
                || E'            )',
            function_identity
        );
    END LOOP;

    contract_marker :=
        E'    ) AS expected(\n'
        || E'        identity,\n'
        || E'        arguments,\n'
        || E'        result,\n'
        || E'        language_name,\n'
        || E'        is_strict,\n'
        || E'        returns_set,\n'
        || E'        rows_estimate\n'
        || E'    )';
    allowlist_marker :=
        E'        )\n'
        || E'        AND namespace.nspname NOT IN '
        || E'(''pg_catalog'', ''information_schema'')';
    IF pg_catalog.strpos(definition, contract_marker) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, contract_marker, ''),
            contract_marker
        ) <> 0
        OR pg_catalog.strpos(definition, allowlist_marker) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, allowlist_marker, ''),
            allowlist_marker
        ) <> 0
        OR pg_catalog.strpos(
            definition,
            '72ab1200d416d069371db605ffef6f5f6197fc3f9c0fdd241001d43dd9c82434'
        ) = 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_certification_v2_execution_readiness_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        contract_marker,
        contract_rows || E'\n' || contract_marker
    );
    definition := pg_catalog.replace(
        definition,
        allowlist_marker,
        allowlist_rows || E'\n' || allowlist_marker
    );
    definition := pg_catalog.replace(
        definition,
        '72ab1200d416d069371db605ffef6f5f6197fc3f9c0fdd241001d43dd9c82434',
        '644a9c08a9b4a216e45db4a9eae308dfcce726e9f37e8816f8f83049a92cf474'
    );
    EXECUTE definition;

    SELECT pg_catalog.pg_get_functiondef(
        'public.starring_runtime_serving_database_readiness_v1()'
            ::REGPROCEDURE
    )
    INTO definition;
    contract_rows := '';
    allowlist_rows := '';

    FOREACH function_identity IN ARRAY ARRAY[
        'public.starring_runtime_serving_observe_v2(text,text,text,text,text,text,bigint,bigint)',
        'public.starring_runtime_serving_heartbeat_v2(text,text,text,text,text,text,bigint,bigint,bigint,bigint)',
        'public.starring_runtime_serving_disconnect_if_current_v2(text,text,text,text,text,text,bigint,bigint,bigint)'
    ]::TEXT[]
    LOOP
        SELECT
            pg_catalog.pg_get_function_identity_arguments(
                function_row.oid
            ),
            pg_catalog.pg_get_function_result(function_row.oid)
        INTO function_arguments, function_result
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid =
            pg_catalog.to_regprocedure(function_identity);

        IF function_arguments IS NULL OR function_result IS NULL THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE =
                    'runtime_certification_v2_serving_contract_missing';
        END IF;

        contract_rows := contract_rows || pg_catalog.format(
            E',\n            (\n'
                || E'                %L,\n'
                || E'                %L::TEXT,\n'
                || E'                %L::TEXT,\n'
                || E'                ''plpgsql''::TEXT,\n'
                || E'                TRUE,\n'
                || E'                1::REAL\n'
                || E'            )',
            function_identity,
            function_arguments,
            function_result
        );
        allowlist_rows := allowlist_rows || pg_catalog.format(
            E',\n            pg_catalog.to_regprocedure(\n'
                || E'                %L\n'
                || E'            )',
            function_identity
        );
    END LOOP;

    contract_marker :=
        E'    ) AS expected(identity, arguments, result, '
        || E'language_name, returns_set, rows_estimate)';
    IF pg_catalog.strpos(definition, contract_marker) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, contract_marker, ''),
            contract_marker
        ) <> 0
        OR pg_catalog.strpos(definition, allowlist_marker) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, allowlist_marker, ''),
            allowlist_marker
        ) <> 0
        OR pg_catalog.strpos(
            definition,
            'a2362a5fa1b9839e124a290cc1845c4af450e49d2d7d6517c97982d2c4f45546'
        ) = 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_certification_v2_serving_readiness_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        contract_marker,
        contract_rows || E'\n' || contract_marker
    );
    definition := pg_catalog.replace(
        definition,
        allowlist_marker,
        allowlist_rows || E'\n' || allowlist_marker
    );
    definition := pg_catalog.replace(
        definition,
        'a2362a5fa1b9839e124a290cc1845c4af450e49d2d7d6517c97982d2c4f45546',
        '7791925b08af642fe3f42d099394e42301086db580dd239b557b73c5640d1811'
    );
    EXECUTE definition;
END;
$patch_readiness$;

DO $capability_acl$
DECLARE
    common_owner OID;
    execution_role OID;
    serving_role OID;
    execution_role_count BIGINT;
    serving_role_count BIGINT;
    role_name NAME;
    function_identity TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    SELECT pg_catalog.count(DISTINCT privilege.grantee),
        pg_catalog.min(privilege.grantee)
    INTO execution_role_count, execution_role
    FROM pg_catalog.pg_proc AS function_row
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_runtime_execution_database_identity_v1()'
        )
        AND privilege.grantee <> common_owner
        AND privilege.privilege_type = 'EXECUTE'
        AND NOT privilege.is_grantable;

    SELECT pg_catalog.count(DISTINCT privilege.grantee),
        pg_catalog.min(privilege.grantee)
    INTO serving_role_count, serving_role
    FROM pg_catalog.pg_proc AS function_row
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_runtime_serving_database_identity_v1()'
        )
        AND privilege.grantee <> common_owner
        AND privilege.privilege_type = 'EXECUTE'
        AND NOT privilege.is_grantable;

    IF common_owner IS NULL
        OR execution_role_count > 1
        OR serving_role_count > 1
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_v2_capability_acl_drift';
    END IF;

    FOREACH function_identity IN ARRAY ARRAY[
        'public.starring_runtime_certification_prepare_v2(TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,TEXT,BIGINT,BIGINT,BIGINT,TIMESTAMPTZ)',
        'public.starring_runtime_certification_commit_v2(TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,TEXT,BIGINT,BIGINT,BIGINT,BYTEA,TEXT,BYTEA,TEXT)',
        'public.starring_runtime_certification_observe_v2(TEXT,TEXT,TEXT,TEXT,BIGINT,BIGINT,TEXT)',
        'public.starring_runtime_serving_observe_v2(TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,BIGINT)',
        'public.starring_runtime_serving_heartbeat_v2(TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,BIGINT,BIGINT,BIGINT)',
        'public.starring_runtime_serving_disconnect_if_current_v2(TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,BIGINT,BIGINT)'
    ]::TEXT[]
    LOOP
        EXECUTE pg_catalog.format(
            'REVOKE ALL ON FUNCTION %s FROM PUBLIC',
            function_identity
        );
    END LOOP;

    IF execution_role IS NOT NULL THEN
        role_name := pg_catalog.pg_get_userbyid(execution_role);
        IF role_name IS NULL THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE =
                    'runtime_certification_v2_execution_role_missing';
        END IF;
        FOREACH function_identity IN ARRAY ARRAY[
            'public.starring_runtime_certification_prepare_v2(TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,TEXT,BIGINT,BIGINT,BIGINT,TIMESTAMPTZ)',
            'public.starring_runtime_certification_commit_v2(TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,TEXT,BIGINT,BIGINT,BIGINT,BYTEA,TEXT,BYTEA,TEXT)',
            'public.starring_runtime_certification_observe_v2(TEXT,TEXT,TEXT,TEXT,BIGINT,BIGINT,TEXT)'
        ]::TEXT[]
        LOOP
            EXECUTE pg_catalog.format(
                'GRANT EXECUTE ON FUNCTION %s TO %I',
                function_identity,
                role_name
            );
        END LOOP;
    END IF;

    IF serving_role IS NOT NULL THEN
        role_name := pg_catalog.pg_get_userbyid(serving_role);
        IF role_name IS NULL THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE =
                    'runtime_certification_v2_serving_role_missing';
        END IF;
        FOREACH function_identity IN ARRAY ARRAY[
            'public.starring_runtime_serving_observe_v2(TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,BIGINT)',
            'public.starring_runtime_serving_heartbeat_v2(TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,BIGINT,BIGINT,BIGINT)',
            'public.starring_runtime_serving_disconnect_if_current_v2(TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,BIGINT,BIGINT)'
        ]::TEXT[]
        LOOP
            EXECUTE pg_catalog.format(
                'GRANT EXECUTE ON FUNCTION %s TO %I',
                function_identity,
                role_name
            );
        END LOOP;
    END IF;
END;
$capability_acl$;

DO $postflight$
DECLARE
    common_owner OID;
    invalid_function_count BIGINT;
    invalid_index_count BIGINT;
    invalid_constraint_count BIGINT;
    invalid_relation_acl_count BIGINT;
    invalid_readiness_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            ('public.starring_runtime_certification_prepare_v2(text,text,text,text,text,bigint,text,bigint,bigint,bigint,timestamp with time zone)'),
            ('public.starring_runtime_certification_commit_v2(text,text,text,text,text,bigint,text,bigint,bigint,bigint,bytea,text,bytea,text)'),
            ('public.starring_runtime_certification_observe_v2(text,text,text,text,bigint,bigint,text)'),
            ('public.starring_runtime_serving_observe_v2(text,text,text,text,text,text,bigint,bigint)'),
            ('public.starring_runtime_serving_heartbeat_v2(text,text,text,text,text,text,bigint,bigint,bigint,bigint)'),
            ('public.starring_runtime_serving_disconnect_if_current_v2(text,text,text,text,text,text,bigint,bigint,bigint)')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid =
            pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR NOT function_row.proisstrict
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR NOT function_row.proretset
        OR function_row.prorows <> 1
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR pg_catalog.has_function_privilege(
            0,
            function_row.oid,
            'EXECUTE'
        );

    SELECT pg_catalog.count(*)
    INTO invalid_index_count
    FROM (
        VALUES
            ('runtime_attestations_v2_operation_unique'),
            ('runtime_attestations_v2_request_digest_unique')
    ) AS expected(index_name)
    LEFT JOIN pg_catalog.pg_class AS index_row
        ON index_row.relnamespace =
            pg_catalog.to_regnamespace('public')
        AND index_row.relname = expected.index_name
    LEFT JOIN pg_catalog.pg_index AS index_contract
        ON index_contract.indexrelid = index_row.oid
    WHERE index_row.oid IS NULL
        OR index_row.relkind <> 'i'
        OR index_row.relowner <> common_owner
        OR NOT index_contract.indisunique
        OR NOT index_contract.indisvalid
        OR NOT index_contract.indisready
        OR NOT index_contract.indislive;

    SELECT pg_catalog.count(*)
    INTO invalid_constraint_count
    FROM pg_catalog.pg_constraint AS constraint_row
    WHERE constraint_row.conrelid =
            pg_catalog.to_regclass('public.runtime_attestations')
        AND constraint_row.conname IN (
            'runtime_attestations_record_valid',
            'runtime_attestations_v2_shape_valid'
        )
        AND (
            constraint_row.contype <> 'c'
            OR NOT constraint_row.convalidated
        );
    IF invalid_constraint_count <> 0
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_constraint AS constraint_row
            WHERE constraint_row.conrelid =
                    pg_catalog.to_regclass(
                        'public.runtime_attestations'
                    )
                AND constraint_row.conname IN (
                    'runtime_attestations_record_valid',
                    'runtime_attestations_v2_shape_valid'
                )
        ) <> 2
    THEN
        invalid_constraint_count := invalid_constraint_count + 1;
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_relation_acl_count
    FROM (
        VALUES
            ('public.runtime_attestations'),
            ('public.runtime_serving_leases'),
            ('public.runtime_deployments'),
            ('public.runtime_certification_operations_v2'),
            ('public.runtime_certification_operation_terminals_v2')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = pg_catalog.to_regclass(expected.identity)
    WHERE relation.oid IS NULL
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                relation.relacl,
                pg_catalog.acldefault('r', relation.relowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
        );

    SELECT pg_catalog.count(*)
    INTO invalid_readiness_count
    FROM (
        VALUES
            (
                'public.starring_runtime_execution_database_readiness_v1()',
                '2a19a8895be1dcc8596ae9413864dc444827c40255e378c0e06b2e1a359304cf'
            ),
            (
                'public.starring_runtime_serving_database_readiness_v1()',
                'fff86b8fa58c182604cfb3dfdd0146043a995a5b4f07691c360e99447b181f3b'
            )
    ) AS expected(identity, definition_digest)
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
        ) IS DISTINCT FROM expected.definition_digest;

    IF common_owner IS NULL
        OR invalid_function_count <> 0
        OR invalid_index_count <> 0
        OR invalid_constraint_count <> 0
        OR invalid_relation_acl_count <> 0
        OR invalid_readiness_count <> 0
        OR EXISTS (
            SELECT 1
            FROM public.runtime_attestations AS attestation
            WHERE attestation.record_format_version = 1
                AND (
                    attestation.v2_operation_id IS NOT NULL
                    OR attestation.v2_intent_fingerprint IS NOT NULL
                    OR attestation.v2_request_digest IS NOT NULL
                    OR attestation.v2_request_bytes IS NOT NULL
                    OR attestation.v2_live_attestation_bytes IS NOT NULL
                    OR attestation.v2_must_commit_before IS NOT NULL
                    OR attestation.v2_route_admission IS NOT NULL
                    OR attestation.v2_route_incarnation IS NOT NULL
                    OR attestation.v2_route_activation_sequence
                        IS NOT NULL
                    OR attestation.v2_initial_lease_epoch IS NOT NULL
                    OR attestation.v2_initial_serving_revision
                        IS NOT NULL
                    OR attestation.v2_prepared_snapshot IS NOT NULL
                    OR attestation.v2_certified_snapshot IS NOT NULL
                )
        )
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_certification_v2_postflight_drift';
    END IF;
END;
$postflight$;
