SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

LOCK TABLE
    public.runtime_writer_fence,
    public.runtime_gateway_owners
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
            'starring_runtime_ingress_open_acknowledgement_observe_v2',
            'starring_runtime_ingress_open_acknowledgement_publish_v2',
            'validate_runtime_ingress_open_acknowledgement_transition_v2',
            'reject_runtime_ingress_open_acknowledgement_mutation_v2'
        );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_execution_schema_manifest_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO manifest_digest;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_execution_database_readiness_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO readiness_digest;

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR pg_catalog.to_regclass(
            'public.runtime_ingress_open_acknowledgements_v2'
        ) IS NOT NULL
        OR collision_count <> 0
        OR manifest_digest IS DISTINCT FROM
            'b7ee8d2a13ae38a88bc1b2558b018e74893e7d90ccd72d96187197a111432e22'
        OR readiness_digest IS DISTINCT FROM
            '3fe2924d130e93d630960be796e3986884fefedddfb91c0dd5b680a41b440cb1'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_ingress_open_acknowledgement_preflight_drift';
    END IF;
END;
$preflight$;

CREATE TABLE public.runtime_ingress_open_acknowledgements_v2 (
    gateway_shard_id TEXT PRIMARY KEY,
    source_acknowledgement_revision BIGINT,
    request_digest BYTEA NOT NULL,
    canonical_request_bytes BYTEA NOT NULL,
    fence_generation BIGINT NOT NULL,
    maintenance_gate_generation BIGINT NOT NULL,
    process_instance_id TEXT NOT NULL,
    owner_lease_epoch BIGINT NOT NULL,
    expected_build_revision TEXT NOT NULL,
    observed_owner_revision BIGINT NOT NULL,
    requested_owner_observed_at TIMESTAMPTZ NOT NULL,
    requested_owner_expires_at TIMESTAMPTZ NOT NULL,
    connection_epoch BIGINT NOT NULL,
    admission_revision BIGINT NOT NULL,
    connected_event_sequence BIGINT NOT NULL,
    resume_sequence BIGINT NOT NULL,
    acknowledgement_revision BIGINT NOT NULL,
    acknowledged_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT runtime_ingress_open_acknowledgements_v2_shard_check CHECK (
        gateway_shard_id = 'shard:0'
    ),
    CONSTRAINT runtime_ingress_open_acknowledgements_v2_source_check CHECK (
        source_acknowledgement_revision IS NULL
        OR source_acknowledgement_revision
            BETWEEN 1 AND 9223372036854775806
    ),
    CONSTRAINT runtime_ingress_open_acknowledgements_v2_digest_check CHECK (
        pg_catalog.octet_length(request_digest) = 32
        AND pg_catalog.octet_length(canonical_request_bytes)
            BETWEEN
                (CASE
                    WHEN source_acknowledgement_revision IS NULL
                        THEN 197
                    ELSE 205
                END)
            AND
                (CASE
                    WHEN source_acknowledgement_revision IS NULL
                        THEN 578
                    ELSE 586
                END)
        AND request_digest =
            pg_catalog.sha256(canonical_request_bytes)
    ),
    CONSTRAINT runtime_ingress_open_acknowledgements_v2_fence_check CHECK (
        fence_generation BETWEEN 1 AND 9223372036854775807
        AND maintenance_gate_generation
            BETWEEN 1 AND 9223372036854775807
    ),
    CONSTRAINT runtime_ingress_open_acknowledgements_v2_owner_check CHECK (
        process_instance_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND owner_lease_epoch BETWEEN 1 AND 9223372036854775807
        AND expected_build_revision ~ '^[A-Za-z0-9_.:/-]{1,128}$'
        AND observed_owner_revision BETWEEN 1 AND 9223372036854775807
        AND pg_catalog.isfinite(requested_owner_observed_at)
        AND (
            EXTRACT(
                EPOCH FROM requested_owner_observed_at
            ) * 1000000
        )::NUMERIC BETWEEN
            -62135596800000000 AND 253402300799999999
        AND pg_catalog.isfinite(requested_owner_expires_at)
        AND (
            EXTRACT(
                EPOCH FROM requested_owner_expires_at
            ) * 1000000
        )::NUMERIC BETWEEN
            -62135596800000000 AND 253402300799999999
        AND requested_owner_observed_at < requested_owner_expires_at
    ),
    CONSTRAINT runtime_ingress_open_acknowledgements_v2_gateway_check CHECK (
        connection_epoch BETWEEN 1 AND 9223372036854775807
        AND admission_revision BETWEEN 1 AND 9223372036854775807
        AND connected_event_sequence
            BETWEEN 1 AND 9223372036854775807
        AND resume_sequence BETWEEN 1 AND 9223372036854775807
        AND resume_sequence > connected_event_sequence
    ),
    CONSTRAINT runtime_ingress_open_acknowledgements_v2_revision_check CHECK (
        acknowledgement_revision BETWEEN 1 AND 9223372036854775807
        AND acknowledgement_revision =
            COALESCE(source_acknowledgement_revision + 1, 1)
    ),
    CONSTRAINT runtime_ingress_open_acknowledgements_v2_interval_check CHECK (
        pg_catalog.isfinite(acknowledged_at)
        AND (
            EXTRACT(EPOCH FROM acknowledged_at) * 1000000
        )::NUMERIC BETWEEN
            -62135596800000000 AND 253402300799999999
        AND pg_catalog.isfinite(expires_at)
        AND (
            EXTRACT(EPOCH FROM expires_at) * 1000000
        )::NUMERIC BETWEEN
            -62135596800000000 AND 253402300799999999
        AND acknowledged_at >= requested_owner_observed_at
        AND acknowledged_at < expires_at
        AND expires_at <= requested_owner_expires_at
        AND expires_at <=
            acknowledged_at + INTERVAL '10 seconds'
    )
);

CREATE FUNCTION public.validate_runtime_ingress_open_acknowledgement_transition_v2()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NEW.source_acknowledgement_revision IS NOT NULL
            OR NEW.acknowledgement_revision <> 1
        THEN
            RAISE EXCEPTION USING
                ERRCODE = '23514',
                MESSAGE =
                    'runtime_ingress_open_acknowledgement_insert_invalid';
        END IF;
        RETURN NEW;
    END IF;

    IF NEW.gateway_shard_id IS DISTINCT FROM OLD.gateway_shard_id
        OR OLD.acknowledgement_revision = 9223372036854775807
        OR NEW.source_acknowledgement_revision
            IS DISTINCT FROM OLD.acknowledgement_revision
        OR NEW.acknowledgement_revision
            IS DISTINCT FROM OLD.acknowledgement_revision + 1
        OR NEW.acknowledged_at < OLD.acknowledged_at
    THEN
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE =
                'runtime_ingress_open_acknowledgement_successor_invalid';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER runtime_ingress_open_acknowledgements_v2_validate_transition
BEFORE INSERT OR UPDATE
ON public.runtime_ingress_open_acknowledgements_v2
FOR EACH ROW
EXECUTE FUNCTION
    public.validate_runtime_ingress_open_acknowledgement_transition_v2();

CREATE FUNCTION public.reject_runtime_ingress_open_acknowledgement_mutation_v2()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    RAISE EXCEPTION USING
        ERRCODE = '23514',
        MESSAGE =
            'runtime_ingress_open_acknowledgement_mutation_rejected';
END;
$function$;

CREATE TRIGGER runtime_ingress_open_acknowledgements_v2_reject_delete
BEFORE DELETE
ON public.runtime_ingress_open_acknowledgements_v2
FOR EACH ROW
EXECUTE FUNCTION
    public.reject_runtime_ingress_open_acknowledgement_mutation_v2();

CREATE TRIGGER runtime_ingress_open_acknowledgements_v2_reject_truncate
BEFORE TRUNCATE
ON public.runtime_ingress_open_acknowledgements_v2
FOR EACH STATEMENT
EXECUTE FUNCTION
    public.reject_runtime_ingress_open_acknowledgement_mutation_v2();

CREATE FUNCTION public.starring_runtime_ingress_open_acknowledgement_observe_v2(
    expected_gateway_shard_id TEXT
)
RETURNS TABLE(
    outcome_name TEXT,
    gateway_shard_id TEXT,
    source_acknowledgement_revision BIGINT,
    request_digest BYTEA,
    canonical_request_bytes BYTEA,
    fence_generation BIGINT,
    maintenance_gate_generation BIGINT,
    process_instance_id TEXT,
    owner_lease_epoch BIGINT,
    expected_build_revision TEXT,
    observed_owner_revision BIGINT,
    requested_owner_observed_at TIMESTAMPTZ,
    requested_owner_expires_at TIMESTAMPTZ,
    connection_epoch BIGINT,
    admission_revision BIGINT,
    connected_event_sequence BIGINT,
    resume_sequence BIGINT,
    acknowledgement_revision BIGINT,
    acknowledged_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    observed_database_now TIMESTAMPTZ
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
    acknowledgement_row
        public.runtime_ingress_open_acknowledgements_v2%ROWTYPE;
    database_now TIMESTAMPTZ;
BEGIN
    IF expected_gateway_shard_id IS DISTINCT FROM 'shard:0' THEN
        RETURN;
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock_shared(
        pg_catalog.hashtextextended(
            'starring-runtime-ingress-open-acknowledgement-v2:'
                || expected_gateway_shard_id,
            0
        )
    );
    database_now := pg_catalog.clock_timestamp();

    SELECT acknowledgement.*
    INTO acknowledgement_row
    FROM public.runtime_ingress_open_acknowledgements_v2
        AS acknowledgement
    WHERE acknowledgement.gateway_shard_id =
        expected_gateway_shard_id;

    IF NOT FOUND THEN
        RETURN QUERY SELECT
            'missing'::TEXT,
            expected_gateway_shard_id,
            NULL::BIGINT,
            NULL::BYTEA,
            NULL::BYTEA,
            NULL::BIGINT,
            NULL::BIGINT,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::TIMESTAMPTZ,
            NULL::TIMESTAMPTZ,
            NULL::BIGINT,
            NULL::BIGINT,
            NULL::BIGINT,
            NULL::BIGINT,
            NULL::BIGINT,
            NULL::TIMESTAMPTZ,
            NULL::TIMESTAMPTZ,
            database_now;
        RETURN;
    END IF;

    RETURN QUERY SELECT
        'present'::TEXT,
        acknowledgement_row.gateway_shard_id,
        acknowledgement_row.source_acknowledgement_revision,
        acknowledgement_row.request_digest,
        acknowledgement_row.canonical_request_bytes,
        acknowledgement_row.fence_generation,
        acknowledgement_row.maintenance_gate_generation,
        acknowledgement_row.process_instance_id,
        acknowledgement_row.owner_lease_epoch,
        acknowledgement_row.expected_build_revision,
        acknowledgement_row.observed_owner_revision,
        acknowledgement_row.requested_owner_observed_at,
        acknowledgement_row.requested_owner_expires_at,
        acknowledgement_row.connection_epoch,
        acknowledgement_row.admission_revision,
        acknowledgement_row.connected_event_sequence,
        acknowledgement_row.resume_sequence,
        acknowledgement_row.acknowledgement_revision,
        acknowledgement_row.acknowledged_at,
        acknowledgement_row.expires_at,
        database_now;
END;
$function$;

CREATE FUNCTION public.starring_runtime_ingress_open_acknowledgement_publish_v2(
    expected_gateway_shard_id TEXT,
    requested_source_acknowledgement_revision BIGINT,
    proposed_request_digest BYTEA,
    proposed_canonical_request_bytes BYTEA,
    expected_fence_generation BIGINT,
    expected_maintenance_gate_generation BIGINT,
    expected_process_instance_id TEXT,
    expected_owner_lease_epoch BIGINT,
    requested_build_revision TEXT,
    expected_owner_revision BIGINT,
    expected_owner_observed_at TIMESTAMPTZ,
    expected_owner_expires_at TIMESTAMPTZ,
    expected_connection_epoch BIGINT,
    expected_admission_revision BIGINT,
    expected_connected_event_sequence BIGINT,
    expected_resume_sequence BIGINT,
    requested_lease_milliseconds BIGINT
)
RETURNS TABLE(
    outcome_name TEXT,
    gateway_shard_id TEXT,
    source_acknowledgement_revision BIGINT,
    request_digest BYTEA,
    canonical_request_bytes BYTEA,
    fence_generation BIGINT,
    maintenance_gate_generation BIGINT,
    process_instance_id TEXT,
    owner_lease_epoch BIGINT,
    expected_build_revision TEXT,
    observed_owner_revision BIGINT,
    requested_owner_observed_at TIMESTAMPTZ,
    requested_owner_expires_at TIMESTAMPTZ,
    connection_epoch BIGINT,
    admission_revision BIGINT,
    connected_event_sequence BIGINT,
    resume_sequence BIGINT,
    acknowledgement_revision BIGINT,
    acknowledged_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    observed_database_now TIMESTAMPTZ
)
LANGUAGE plpgsql
VOLATILE
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
DECLARE
    writer_fence_row public.runtime_writer_fence%ROWTYPE;
    owner_row public.runtime_gateway_owners%ROWTYPE;
    acknowledgement_row
        public.runtime_ingress_open_acknowledgements_v2%ROWTYPE;
    database_now TIMESTAMPTZ;
    resulting_expiry TIMESTAMPTZ;
    resulting_revision BIGINT;
BEGIN
    IF expected_gateway_shard_id IS DISTINCT FROM 'shard:0'
        OR requested_source_acknowledgement_revision IS NOT NULL
            AND requested_source_acknowledgement_revision
                NOT BETWEEN 1 AND 9223372036854775806
        OR proposed_request_digest IS NULL
        OR pg_catalog.octet_length(proposed_request_digest) <> 32
        OR proposed_canonical_request_bytes IS NULL
        OR pg_catalog.octet_length(proposed_canonical_request_bytes)
            NOT BETWEEN
                (CASE
                    WHEN requested_source_acknowledgement_revision
                        IS NULL
                    THEN 197
                    ELSE 205
                END)
            AND
                (CASE
                    WHEN requested_source_acknowledgement_revision
                        IS NULL
                    THEN 578
                    ELSE 586
                END)
        OR proposed_request_digest IS DISTINCT FROM
            pg_catalog.sha256(proposed_canonical_request_bytes)
        OR expected_fence_generation
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_maintenance_gate_generation
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_process_instance_id IS NULL
        OR expected_process_instance_id
            !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_owner_lease_epoch
            NOT BETWEEN 1 AND 9223372036854775807
        OR requested_build_revision IS NULL
        OR requested_build_revision
            !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR expected_owner_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_owner_observed_at IS NULL
        OR expected_owner_expires_at IS NULL
        OR NOT pg_catalog.isfinite(expected_owner_observed_at)
        OR (
            EXTRACT(EPOCH FROM expected_owner_observed_at)
                * 1000000
        )::NUMERIC NOT BETWEEN
            -62135596800000000 AND 253402300799999999
        OR NOT pg_catalog.isfinite(expected_owner_expires_at)
        OR (
            EXTRACT(EPOCH FROM expected_owner_expires_at)
                * 1000000
        )::NUMERIC NOT BETWEEN
            -62135596800000000 AND 253402300799999999
        OR expected_owner_observed_at >= expected_owner_expires_at
        OR expected_connection_epoch
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_admission_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_connected_event_sequence
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_resume_sequence
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_resume_sequence <= expected_connected_event_sequence
        OR requested_lease_milliseconds NOT BETWEEN 1000 AND 10000
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE =
                'runtime_ingress_open_acknowledgement_request_invalid';
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            'starring-runtime-writer-fence-v1',
            0
        )
    );
    SELECT fence.*
    INTO writer_fence_row
    FROM public.runtime_writer_fence AS fence
    WHERE fence.singleton
    FOR UPDATE;

    IF NOT FOUND
        OR writer_fence_row.fence_state <> 'open'
        OR writer_fence_row.fence_generation
            IS DISTINCT FROM expected_fence_generation
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE =
                'runtime_ingress_open_acknowledgement_writer_fence_stale';
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            'starring-runtime-gateway-owner-v1:'
                || expected_gateway_shard_id,
            0
        )
    );
    SELECT owner.*
    INTO owner_row
    FROM public.runtime_gateway_owners AS owner
    WHERE owner.gateway_shard_id = expected_gateway_shard_id
    FOR UPDATE;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            'starring-runtime-ingress-open-acknowledgement-v2:'
                || expected_gateway_shard_id,
            0
        )
    );
    SELECT acknowledgement.*
    INTO acknowledgement_row
    FROM public.runtime_ingress_open_acknowledgements_v2
        AS acknowledgement
    WHERE acknowledgement.gateway_shard_id =
        expected_gateway_shard_id
    FOR UPDATE;

    database_now := pg_catalog.clock_timestamp();

    IF owner_row.gateway_shard_id IS NULL
        OR owner_row.process_instance_id IS NULL
        OR owner_row.process_instance_id
            IS DISTINCT FROM expected_process_instance_id
        OR owner_row.lease_epoch
            IS DISTINCT FROM expected_owner_lease_epoch
        OR owner_row.expected_build_revision
            IS DISTINCT FROM requested_build_revision
        OR owner_row.owner_revision
            IS DISTINCT FROM expected_owner_revision
        OR owner_row.expires_at
            IS DISTINCT FROM expected_owner_expires_at
        OR expected_owner_observed_at > database_now
        OR owner_row.expires_at <= database_now
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE =
                'runtime_ingress_open_acknowledgement_owner_lost';
    END IF;

    IF acknowledgement_row.gateway_shard_id IS NOT NULL
        AND acknowledgement_row.source_acknowledgement_revision
            IS NOT DISTINCT FROM
                requested_source_acknowledgement_revision
    THEN
        IF acknowledgement_row.request_digest
                IS DISTINCT FROM proposed_request_digest
            OR acknowledgement_row.canonical_request_bytes
                IS DISTINCT FROM proposed_canonical_request_bytes
        THEN
            RETURN QUERY SELECT
                'not_current'::TEXT,
                acknowledgement_row.gateway_shard_id,
                acknowledgement_row.source_acknowledgement_revision,
                acknowledgement_row.request_digest,
                acknowledgement_row.canonical_request_bytes,
                acknowledgement_row.fence_generation,
                acknowledgement_row.maintenance_gate_generation,
                acknowledgement_row.process_instance_id,
                acknowledgement_row.owner_lease_epoch,
                acknowledgement_row.expected_build_revision,
                acknowledgement_row.observed_owner_revision,
                acknowledgement_row.requested_owner_observed_at,
                acknowledgement_row.requested_owner_expires_at,
                acknowledgement_row.connection_epoch,
                acknowledgement_row.admission_revision,
                acknowledgement_row.connected_event_sequence,
                acknowledgement_row.resume_sequence,
                acknowledgement_row.acknowledgement_revision,
                acknowledgement_row.acknowledged_at,
                acknowledgement_row.expires_at,
                database_now;
            RETURN;
        END IF;

        IF acknowledgement_row.fence_generation
                IS DISTINCT FROM expected_fence_generation
            OR acknowledgement_row.maintenance_gate_generation
                IS DISTINCT FROM expected_maintenance_gate_generation
            OR acknowledgement_row.process_instance_id
                IS DISTINCT FROM expected_process_instance_id
            OR acknowledgement_row.owner_lease_epoch
                IS DISTINCT FROM expected_owner_lease_epoch
            OR acknowledgement_row.expected_build_revision
                IS DISTINCT FROM requested_build_revision
            OR acknowledgement_row.observed_owner_revision
                IS DISTINCT FROM expected_owner_revision
            OR acknowledgement_row.requested_owner_observed_at
                IS DISTINCT FROM expected_owner_observed_at
            OR acknowledgement_row.requested_owner_expires_at
                IS DISTINCT FROM expected_owner_expires_at
            OR acknowledgement_row.connection_epoch
                IS DISTINCT FROM expected_connection_epoch
            OR acknowledgement_row.admission_revision
                IS DISTINCT FROM expected_admission_revision
            OR acknowledgement_row.connected_event_sequence
                IS DISTINCT FROM expected_connected_event_sequence
            OR acknowledgement_row.resume_sequence
                IS DISTINCT FROM expected_resume_sequence
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE =
                    'runtime_ingress_open_acknowledgement_replay_corrupt';
        END IF;

        RETURN QUERY SELECT
            CASE
                WHEN database_now >= acknowledgement_row.expires_at
                    THEN 'not_current'::TEXT
                ELSE 'replayed'::TEXT
            END,
            acknowledgement_row.gateway_shard_id,
            acknowledgement_row.source_acknowledgement_revision,
            acknowledgement_row.request_digest,
            acknowledgement_row.canonical_request_bytes,
            acknowledgement_row.fence_generation,
            acknowledgement_row.maintenance_gate_generation,
            acknowledgement_row.process_instance_id,
            acknowledgement_row.owner_lease_epoch,
            acknowledgement_row.expected_build_revision,
            acknowledgement_row.observed_owner_revision,
            acknowledgement_row.requested_owner_observed_at,
            acknowledgement_row.requested_owner_expires_at,
            acknowledgement_row.connection_epoch,
            acknowledgement_row.admission_revision,
            acknowledgement_row.connected_event_sequence,
            acknowledgement_row.resume_sequence,
            acknowledgement_row.acknowledgement_revision,
            acknowledgement_row.acknowledged_at,
            acknowledgement_row.expires_at,
            database_now;
        RETURN;
    END IF;

    IF acknowledgement_row.gateway_shard_id IS NULL THEN
        IF requested_source_acknowledgement_revision IS NOT NULL THEN
            RETURN QUERY SELECT
                'not_current'::TEXT,
                expected_gateway_shard_id,
                NULL::BIGINT,
                NULL::BYTEA,
                NULL::BYTEA,
                NULL::BIGINT,
                NULL::BIGINT,
                NULL::TEXT,
                NULL::BIGINT,
                NULL::TEXT,
                NULL::BIGINT,
                NULL::TIMESTAMPTZ,
                NULL::TIMESTAMPTZ,
                NULL::BIGINT,
                NULL::BIGINT,
                NULL::BIGINT,
                NULL::BIGINT,
                NULL::BIGINT,
                NULL::TIMESTAMPTZ,
                NULL::TIMESTAMPTZ,
                database_now;
            RETURN;
        END IF;
        resulting_revision := 1;
    ELSIF acknowledgement_row.acknowledgement_revision
            IS DISTINCT FROM requested_source_acknowledgement_revision
    THEN
        RETURN QUERY SELECT
            'not_current'::TEXT,
            acknowledgement_row.gateway_shard_id,
            acknowledgement_row.source_acknowledgement_revision,
            acknowledgement_row.request_digest,
            acknowledgement_row.canonical_request_bytes,
            acknowledgement_row.fence_generation,
            acknowledgement_row.maintenance_gate_generation,
            acknowledgement_row.process_instance_id,
            acknowledgement_row.owner_lease_epoch,
            acknowledgement_row.expected_build_revision,
            acknowledgement_row.observed_owner_revision,
            acknowledgement_row.requested_owner_observed_at,
            acknowledgement_row.requested_owner_expires_at,
            acknowledgement_row.connection_epoch,
            acknowledgement_row.admission_revision,
            acknowledgement_row.connected_event_sequence,
            acknowledgement_row.resume_sequence,
            acknowledgement_row.acknowledgement_revision,
            acknowledgement_row.acknowledged_at,
            acknowledgement_row.expires_at,
            database_now;
        RETURN;
    ELSE
        IF acknowledgement_row.acknowledgement_revision =
            9223372036854775807
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE =
                    'runtime_ingress_open_acknowledgement_revision_exhausted';
        END IF;
        resulting_revision :=
            acknowledgement_row.acknowledgement_revision + 1;
    END IF;

    resulting_expiry := LEAST(
        database_now
            + requested_lease_milliseconds
                * INTERVAL '1 millisecond',
        owner_row.expires_at
    );
    IF resulting_expiry <= database_now THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE =
                'runtime_ingress_open_acknowledgement_owner_margin_elapsed';
    END IF;

    IF acknowledgement_row.gateway_shard_id IS NULL THEN
        INSERT INTO
            public.runtime_ingress_open_acknowledgements_v2 (
                gateway_shard_id,
                source_acknowledgement_revision,
                request_digest,
                canonical_request_bytes,
                fence_generation,
                maintenance_gate_generation,
                process_instance_id,
                owner_lease_epoch,
                expected_build_revision,
                observed_owner_revision,
                requested_owner_observed_at,
                requested_owner_expires_at,
                connection_epoch,
                admission_revision,
                connected_event_sequence,
                resume_sequence,
                acknowledgement_revision,
                acknowledged_at,
                expires_at
            )
        VALUES (
            expected_gateway_shard_id,
            requested_source_acknowledgement_revision,
            proposed_request_digest,
            proposed_canonical_request_bytes,
            expected_fence_generation,
            expected_maintenance_gate_generation,
            expected_process_instance_id,
            expected_owner_lease_epoch,
            requested_build_revision,
            expected_owner_revision,
            expected_owner_observed_at,
            expected_owner_expires_at,
            expected_connection_epoch,
            expected_admission_revision,
            expected_connected_event_sequence,
            expected_resume_sequence,
            resulting_revision,
            database_now,
            resulting_expiry
        )
        RETURNING * INTO acknowledgement_row;
    ELSE
        UPDATE
            public.runtime_ingress_open_acknowledgements_v2
                AS acknowledgement
        SET source_acknowledgement_revision =
                requested_source_acknowledgement_revision,
            request_digest = proposed_request_digest,
            canonical_request_bytes =
                proposed_canonical_request_bytes,
            fence_generation = expected_fence_generation,
            maintenance_gate_generation =
                expected_maintenance_gate_generation,
            process_instance_id = expected_process_instance_id,
            owner_lease_epoch = expected_owner_lease_epoch,
            expected_build_revision = requested_build_revision,
            observed_owner_revision = expected_owner_revision,
            requested_owner_observed_at =
                expected_owner_observed_at,
            requested_owner_expires_at = expected_owner_expires_at,
            connection_epoch = expected_connection_epoch,
            admission_revision = expected_admission_revision,
            connected_event_sequence =
                expected_connected_event_sequence,
            resume_sequence = expected_resume_sequence,
            acknowledgement_revision = resulting_revision,
            acknowledged_at = database_now,
            expires_at = resulting_expiry
        WHERE acknowledgement.gateway_shard_id =
                expected_gateway_shard_id
            AND acknowledgement.acknowledgement_revision =
                requested_source_acknowledgement_revision
        RETURNING * INTO acknowledgement_row;
        IF NOT FOUND THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE =
                    'runtime_ingress_open_acknowledgement_cas_lost';
        END IF;
    END IF;

    RETURN QUERY SELECT
        'applied'::TEXT,
        acknowledgement_row.gateway_shard_id,
        acknowledgement_row.source_acknowledgement_revision,
        acknowledgement_row.request_digest,
        acknowledgement_row.canonical_request_bytes,
        acknowledgement_row.fence_generation,
        acknowledgement_row.maintenance_gate_generation,
        acknowledgement_row.process_instance_id,
        acknowledgement_row.owner_lease_epoch,
        acknowledgement_row.expected_build_revision,
        acknowledgement_row.observed_owner_revision,
        acknowledgement_row.requested_owner_observed_at,
        acknowledgement_row.requested_owner_expires_at,
        acknowledgement_row.connection_epoch,
        acknowledgement_row.admission_revision,
        acknowledgement_row.connected_event_sequence,
        acknowledgement_row.resume_sequence,
        acknowledgement_row.acknowledgement_revision,
        acknowledgement_row.acknowledged_at,
        acknowledgement_row.expires_at,
        database_now;
END;
$function$;

REVOKE ALL ON TABLE
    public.runtime_ingress_open_acknowledgements_v2
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.validate_runtime_ingress_open_acknowledgement_transition_v2()
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.reject_runtime_ingress_open_acknowledgement_mutation_v2()
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.starring_runtime_ingress_open_acknowledgement_observe_v2(TEXT)
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.starring_runtime_ingress_open_acknowledgement_publish_v2(
        TEXT,
        BIGINT,
        BYTEA,
        BYTEA,
        BIGINT,
        BIGINT,
        TEXT,
        BIGINT,
        TEXT,
        BIGINT,
        TIMESTAMPTZ,
        TIMESTAMPTZ,
        BIGINT,
        BIGINT,
        BIGINT,
        BIGINT,
        BIGINT
    )
FROM PUBLIC;

DO $execution_acl$
DECLARE
    common_owner OID;
    executor_grantee OID;
    grantee_count BIGINT;
    invalid_capability_count BIGINT;
    executor_name NAME;
    function_identity TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    SELECT
        pg_catalog.count(DISTINCT privilege.grantee),
        pg_catalog.min(privilege.grantee::BIGINT)::OID
    INTO grantee_count, executor_grantee
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
            ('public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)')
    ) AS expected(identity)
    INNER JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid =
            pg_catalog.to_regprocedure(expected.identity)
    CROSS JOIN LATERAL pg_catalog.aclexplode(
        COALESCE(
            function_row.proacl,
            pg_catalog.acldefault('f', function_row.proowner)
        )
    ) AS privilege
    WHERE privilege.grantee <> common_owner;

    SELECT pg_catalog.count(*)
    INTO invalid_capability_count
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
            ('public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid =
            pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(
                COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault(
                        'f',
                        function_row.proowner
                    )
                )
            ) AS privilege
            WHERE privilege.grantee <> common_owner
        ) <> CASE
            WHEN executor_grantee IS NULL THEN 0
            ELSE 1
        END
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(
                COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault(
                        'f',
                        function_row.proowner
                    )
                )
            ) AS privilege
            WHERE privilege.grantee <> common_owner
                AND (
                    privilege.grantee IS DISTINCT FROM
                        executor_grantee
                    OR privilege.grantor <> common_owner
                    OR privilege.privilege_type <> 'EXECUTE'
                    OR privilege.is_grantable
                )
        );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR grantee_count > 1
        OR invalid_capability_count <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_ingress_open_acknowledgement_execution_acl_drift';
    END IF;

    IF executor_grantee IS NOT NULL THEN
        executor_name :=
            pg_catalog.pg_get_userbyid(executor_grantee);
        IF executor_name IS NULL THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE =
                    'runtime_ingress_open_acknowledgement_execution_acl_drift';
        END IF;
        FOREACH function_identity IN ARRAY ARRAY[
            'public.starring_runtime_ingress_open_acknowledgement_observe_v2(TEXT)',
            'public.starring_runtime_ingress_open_acknowledgement_publish_v2(TEXT,BIGINT,BYTEA,BYTEA,BIGINT,BIGINT,TEXT,BIGINT,TEXT,BIGINT,TIMESTAMPTZ,TIMESTAMPTZ,BIGINT,BIGINT,BIGINT,BIGINT,BIGINT)'
        ]::TEXT[]
        LOOP
            EXECUTE pg_catalog.format(
                'GRANT EXECUTE ON FUNCTION %s TO %I',
                function_identity,
                executor_name
            );
        END LOOP;
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
        '(pg_catalog.to_regclass(''public.runtime_gateway_owners'')),';
    next_fragment := previous_fragment || E'\n' ||
        '            (pg_catalog.to_regclass(''public.runtime_ingress_open_acknowledgements_v2'')),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_ingress_open_acknowledgement_manifest_relation_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        'SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_writer_fence_observe_v1()''';
    next_fragment :=
        'SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_ingress_open_acknowledgement_observe_v2(text)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_ingress_open_acknowledgement_publish_v2(text,bigint,bytea,bytea,bigint,bigint,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,bigint,bigint,bigint,bigint,bigint)''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.validate_runtime_ingress_open_acknowledgement_transition_v2()''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.reject_runtime_ingress_open_acknowledgement_mutation_v2()''' || E'\n' ||
        '        )' || E'\n' ||
        '        UNION' || E'\n' ||
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''public.starring_runtime_writer_fence_observe_v1()''';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_ingress_open_acknowledgement_manifest_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        'RETURN observed_count = 911' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''ae39639ca7f4f2d911e227b8429d1566efdc677dbfd641d8fcf5f24d376baf8b'';';
    next_fragment :=
        'RETURN observed_count = 948' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''bd8e47e52db30d06ac726b2763a20f54b993f1e04c374975a96a510a31919ade'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_ingress_open_acknowledgement_manifest_expectation_patch_drift';
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
        '(''public.runtime_gateway_owners''),';
    next_fragment := previous_fragment || E'\n' ||
        '            (''public.runtime_ingress_open_acknowledgements_v2''),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_ingress_open_acknowledgement_readiness_relation_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            (' || E'\n' ||
        '                ''public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)'',' || E'\n' ||
        '                ''expected_gateway_shard_id text, expected_process_instance_id text, expected_lease_epoch bigint, requested_build_revision text''::TEXT,' || E'\n' ||
        '                ''TABLE(outcome_name text, gateway_shard_id text, process_instance_id text, lease_epoch bigint, expected_build_revision text, owner_revision bigint, database_now timestamp with time zone, expires_at timestamp with time zone)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            ),' || E'\n' ||
        '            (' || E'\n' ||
        '                ''public.starring_runtime_writer_fence_observe_v1()''';
    next_fragment :=
        '            (' || E'\n' ||
        '                ''public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)'',' || E'\n' ||
        '                ''expected_gateway_shard_id text, expected_process_instance_id text, expected_lease_epoch bigint, requested_build_revision text''::TEXT,' || E'\n' ||
        '                ''TABLE(outcome_name text, gateway_shard_id text, process_instance_id text, lease_epoch bigint, expected_build_revision text, owner_revision bigint, database_now timestamp with time zone, expires_at timestamp with time zone)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            ),' || E'\n' ||
        '            (' || E'\n' ||
        '                ''public.starring_runtime_ingress_open_acknowledgement_observe_v2(text)'',' || E'\n' ||
        '                ''expected_gateway_shard_id text''::TEXT,' || E'\n' ||
        '                ''TABLE(outcome_name text, gateway_shard_id text, source_acknowledgement_revision bigint, request_digest bytea, canonical_request_bytes bytea, fence_generation bigint, maintenance_gate_generation bigint, process_instance_id text, owner_lease_epoch bigint, expected_build_revision text, observed_owner_revision bigint, requested_owner_observed_at timestamp with time zone, requested_owner_expires_at timestamp with time zone, connection_epoch bigint, admission_revision bigint, connected_event_sequence bigint, resume_sequence bigint, acknowledgement_revision bigint, acknowledged_at timestamp with time zone, expires_at timestamp with time zone, observed_database_now timestamp with time zone)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            ),' || E'\n' ||
        '            (' || E'\n' ||
        '                ''public.starring_runtime_ingress_open_acknowledgement_publish_v2(text,bigint,bytea,bytea,bigint,bigint,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,bigint,bigint,bigint,bigint,bigint)'',' || E'\n' ||
        '                ''expected_gateway_shard_id text, requested_source_acknowledgement_revision bigint, proposed_request_digest bytea, proposed_canonical_request_bytes bytea, expected_fence_generation bigint, expected_maintenance_gate_generation bigint, expected_process_instance_id text, expected_owner_lease_epoch bigint, requested_build_revision text, expected_owner_revision bigint, expected_owner_observed_at timestamp with time zone, expected_owner_expires_at timestamp with time zone, expected_connection_epoch bigint, expected_admission_revision bigint, expected_connected_event_sequence bigint, expected_resume_sequence bigint, requested_lease_milliseconds bigint''::TEXT,' || E'\n' ||
        '                ''TABLE(outcome_name text, gateway_shard_id text, source_acknowledgement_revision bigint, request_digest bytea, canonical_request_bytes bytea, fence_generation bigint, maintenance_gate_generation bigint, process_instance_id text, owner_lease_epoch bigint, expected_build_revision text, observed_owner_revision bigint, requested_owner_observed_at timestamp with time zone, requested_owner_expires_at timestamp with time zone, connection_epoch bigint, admission_revision bigint, connected_event_sequence bigint, resume_sequence bigint, acknowledgement_revision bigint, acknowledged_at timestamp with time zone, expires_at timestamp with time zone, observed_database_now timestamp with time zone)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                FALSE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            ),' || E'\n' ||
        '            (' || E'\n' ||
        '                ''public.starring_runtime_writer_fence_observe_v1()''';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_ingress_open_acknowledgement_readiness_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '''b7ee8d2a13ae38a88bc1b2558b018e74893e7d90ccd72d96187197a111432e22''::TEXT';
    next_fragment :=
        '''72ab1200d416d069371db605ffef6f5f6197fc3f9c0fdd241001d43dd9c82434''::TEXT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_ingress_open_acknowledgement_readiness_manifest_digest_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '(''public.reject_runtime_gateway_owner_delete()''),';
    next_fragment := previous_fragment || E'\n' ||
        '            (''public.validate_runtime_ingress_open_acknowledgement_transition_v2()''),' || E'\n' ||
        '            (''public.reject_runtime_ingress_open_acknowledgement_mutation_v2()''),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_ingress_open_acknowledgement_readiness_protected_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)''' || E'\n' ||
        '            ),' || E'\n' ||
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_writer_fence_observe_v1()''';
    next_fragment :=
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)''' || E'\n' ||
        '            ),' || E'\n' ||
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_ingress_open_acknowledgement_observe_v2(text)''' || E'\n' ||
        '            ),' || E'\n' ||
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_ingress_open_acknowledgement_publish_v2(text,bigint,bytea,bytea,bigint,bigint,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,bigint,bigint,bigint,bigint,bigint)''' || E'\n' ||
        '            ),' || E'\n' ||
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_writer_fence_observe_v1()''';
    IF pg_catalog.strpos(definition, previous_fragment) = 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_ingress_open_acknowledgement_readiness_allowlist_patch_drift';
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
    executor_count BIGINT;
    executor_oid OID;
    invalid_executor_count BIGINT;
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
    INTO invalid_relation_count
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_ingress_open_acknowledgements_v2'
    )
        AND (
            relation.relowner <> common_owner
            OR relation.relkind <> 'r'
            OR relation.relpersistence <> 'p'
            OR relation.relrowsecurity
            OR relation.relforcerowsecurity
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(
                    COALESCE(
                        relation.relacl,
                        pg_catalog.acldefault(
                            'r',
                            relation.relowner
                        )
                    )
                ) AS privilege
                WHERE privilege.grantee <>
                    relation.relowner
            )
        );
    IF pg_catalog.to_regclass(
        'public.runtime_ingress_open_acknowledgements_v2'
    ) IS NULL THEN
        invalid_relation_count := invalid_relation_count + 1;
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_runtime_ingress_open_acknowledgement_observe_v2(text)',
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_ingress_open_acknowledgement_publish_v2(text,bigint,bytea,bytea,bigint,bigint,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,bigint,bigint,bigint,bigint,bigint)',
                FALSE,
                1::REAL
            ),
            (
                'public.validate_runtime_ingress_open_acknowledgement_transition_v2()',
                FALSE,
                0::REAL
            ),
            (
                'public.reject_runtime_ingress_open_acknowledgement_mutation_v2()',
                FALSE,
                0::REAL
            )
    ) AS expected(identity, is_strict, rows_estimate)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid =
            pg_catalog.to_regprocedure(expected.identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR function_row.proisstrict IS DISTINCT FROM
            expected.is_strict
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR function_row.prorows IS DISTINCT FROM
            expected.rows_estimate
        OR function_row.proconfig IS DISTINCT FROM
            ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname <> 'plpgsql';

    WITH capability(identity) AS (
        VALUES
            ('public.starring_runtime_ingress_open_acknowledgement_observe_v2(text)'),
            ('public.starring_runtime_ingress_open_acknowledgement_publish_v2(text,bigint,bytea,bytea,bigint,bigint,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,bigint,bigint,bigint,bigint,bigint)')
    ), grants AS (
        SELECT
            privilege.grantee,
            privilege.grantor,
            privilege.privilege_type,
            privilege.is_grantable
        FROM capability AS expected
        INNER JOIN pg_catalog.pg_proc AS function_row
            ON function_row.oid =
                pg_catalog.to_regprocedure(expected.identity)
        CROSS JOIN LATERAL pg_catalog.aclexplode(
            COALESCE(
                function_row.proacl,
                pg_catalog.acldefault(
                    'f',
                    function_row.proowner
                )
            )
        ) AS privilege
        WHERE privilege.grantee <> common_owner
    )
    SELECT
        pg_catalog.count(*) FILTER (
            WHERE grantee = 0
                OR grantor <> common_owner
                OR privilege_type <> 'EXECUTE'
                OR is_grantable
        ),
        pg_catalog.count(DISTINCT grantee),
        pg_catalog.min(grantee::BIGINT)::OID
    INTO invalid_acl_count, executor_count, executor_oid
    FROM grants;

    IF executor_count = 1 THEN
        SELECT pg_catalog.count(*)
        INTO invalid_executor_count
        FROM pg_catalog.pg_roles AS role_row
        WHERE role_row.oid = executor_oid
            AND (
                role_row.rolsuper
                OR role_row.rolinherit
                OR role_row.rolcreaterole
                OR role_row.rolcreatedb
                OR role_row.rolreplication
                OR role_row.rolbypassrls
                OR role_row.oid = common_owner
                OR EXISTS (
                    SELECT 1
                    FROM pg_catalog.pg_auth_members
                        AS membership
                    WHERE membership.member = role_row.oid
                        OR membership.roleid = role_row.oid
                )
            );
    ELSE
        invalid_executor_count := CASE
            WHEN executor_count = 0 THEN 0
            ELSE 1
        END;
    END IF;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_execution_schema_manifest_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO manifest_digest;
    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_execution_database_readiness_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO readiness_digest;

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR invalid_relation_count <> 0
        OR invalid_function_count <> 0
        OR invalid_acl_count <> 0
        OR invalid_executor_count <> 0
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
        OR manifest_digest IS DISTINCT FROM
            '72ab1200d416d069371db605ffef6f5f6197fc3f9c0fdd241001d43dd9c82434'
        OR readiness_digest IS DISTINCT FROM
            '572d7ffd19d6f2edb5ec84ea6b7bfebd178c7da0568bce61af2f7907cfe72647'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_ingress_open_acknowledgement_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
