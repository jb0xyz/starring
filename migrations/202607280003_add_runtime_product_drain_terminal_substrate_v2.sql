SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
);

LOCK TABLE
    public.runtime_writer_fence,
    public.runtime_gateway_owners,
    public.runtime_serving_leases,
    public.runtime_deployments,
    public.runtime_slot_writer_fences_v2,
    public.runtime_certification_operations_v2,
    public.runtime_certification_operation_terminals_v2,
    public.runtime_product_operations_v2,
    public.runtime_drain_intents_v2,
    public.product_action_receipt_idempotency_aliases,
    public.product_action_receipts,
    public.product_action_receipt_audit_evidence,
    public.product_audit_events
IN ACCESS EXCLUSIVE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    private_schema_owner OID;
    other_client_session_count BIGINT;
    prepared_transaction_count BIGINT;
    collision_count BIGINT;
    invalid_definition_count BIGINT;
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
            ('public.runtime_product_drain_terminal_actions_v2', FALSE),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_consumed_state_v2(public.runtime_drain_intents_v2,bigint,timestamp with time zone)', TRUE),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_cancelled_state_v2(public.runtime_drain_intents_v2,timestamp with time zone)', TRUE),
            ('starring_runtime_private_v2.starring_runtime_product_drain_terminal_projection_v2(text,text,text,text,text,text,text,text,text,bigint,text,bigint,bytea,text,bigint,bigint,text,text,bigint,text,bigint,bigint,text,text,text,bigint,timestamp with time zone)', TRUE),
            ('starring_runtime_private_v2.starring_runtime_product_drain_terminal_action_exact_v2(public.runtime_product_drain_terminal_actions_v2,public.runtime_product_operations_v2,public.runtime_drain_intents_v2)', TRUE),
            ('starring_runtime_private_v2.starring_runtime_product_drain_terminal_transition_v2(text,bigint,text,text,bigint,timestamp with time zone)', TRUE),
            ('starring_runtime_private_v2.starring_runtime_slot_writer_fence_terminal_release_v2(text,text,bigint,text,text,text,text,text,bigint,bigint,bytea,text,bigint,text,text,timestamp with time zone)', TRUE),
            ('starring_runtime_private_v2.reject_runtime_product_drain_terminal_action_mutation_v2()', TRUE)
    ) AS expected(identity, function_kind)
    WHERE (
            expected.function_kind
            AND pg_catalog.to_regprocedure(expected.identity) IS NOT NULL
        )
        OR (
            NOT expected.function_kind
            AND pg_catalog.to_regclass(expected.identity) IS NOT NULL
        );

    SELECT pg_catalog.count(*)
    INTO invalid_definition_count
    FROM (
        VALUES
            (
                'starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(public.runtime_drain_intents_v2)',
                'bb65753a0060d7eca67071b51c48e5c4d11137d6b52de928efbd5f6795b0f6ab'
            ),
            (
                'public.reject_runtime_product_drain_mutation()',
                '71bae3d64f810dbbe29a670a3d9cedaeb6428a809eb6d8b757e247bdd9c2a046'
            ),
            (
                'public.reject_runtime_slot_writer_fence_mutation_v2()',
                '5276f1b4e0b021d6cf499c725f3a9d95bc1479c541f7087b8bf11cc1656802cd'
            ),
            (
                'public.validate_runtime_slot_writer_fence_symmetry_v2()',
                '3c6901656c8edb5c8d25347d630e6c821963ca86bd0baed5176a7b2a8f34daa8'
            ),
            (
                'starring_runtime_private_v2.starring_runtime_pending_drain_candidate_v2()',
                'bbf036adbe6e83eaf1c3b37887aa6b1c725ee9ceeeec17808291997450a88030'
            ),
            (
                'public.starring_runtime_product_drain_observe_v2(text,text,text,bigint,text,text)',
                'd46670535d533a28d22fe404c3867169068fac20603e40200630b8358431c754'
            ),
            (
                'public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)',
                '2e27557e004d737da10c9e2b9b29db64476726584638a7c19c171fa90caf4a98'
            ),
            (
                'public.starring_runtime_startup_recovery_execute_stale_live_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)',
                'e8ae861c9b542edc7c0ffc7e7550c063c07bf2bc8c17360760181c2e85216b1c'
            ),
            (
                'public.starring_runtime_execution_schema_manifest_v1()',
                '8f62326b250fba74273b2dbbf33066ef7f1353e9a6f3f464c059b1678bb714d4'
            ),
            (
                'public.starring_runtime_execution_database_readiness_v1()',
                'd73ca3b8f02623884ccf1e77390395a1daeee1d5c3d12274f865740d0798fa06'
            )
    ) AS expected(identity, digest)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid =
            pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(function_row.oid),
                'UTF8'
            )),
            'hex'
        ) IS DISTINCT FROM expected.digest;

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR private_schema_owner IS DISTINCT FROM common_owner
        OR other_client_session_count <> 0
        OR prepared_transaction_count <> 0
        OR collision_count <> 0
        OR invalid_definition_count <> 0
        OR pg_catalog.pg_get_constraintdef(
            (
                SELECT constraint_row.oid
                FROM pg_catalog.pg_constraint AS constraint_row
                WHERE constraint_row.conrelid =
                        'public.runtime_drain_intents_v2'::REGCLASS
                    AND constraint_row.conname =
                        'runtime_drain_intents_v2_state_check'
            ),
            TRUE
        ) IS DISTINCT FROM
            'CHECK (intent_state = ANY (ARRAY[''pending''::text, ''route_absent_acknowledged''::text]))'
        OR pg_catalog.pg_get_indexdef(
            pg_catalog.to_regclass(
                'public.runtime_drain_intents_v2_one_pending_per_slot'
            )
        ) IS DISTINCT FROM
            'CREATE UNIQUE INDEX runtime_drain_intents_v2_one_pending_per_slot ON public.runtime_drain_intents_v2 USING btree (slot_guild_id, slot_ruleset_key) WHERE (intent_state = ANY (ARRAY[''pending''::text, ''route_absent_acknowledged''::text]))'
        OR EXISTS (
            SELECT 1
            FROM public.runtime_drain_intents_v2 AS drain
            WHERE drain.intent_state NOT IN (
                'pending',
                'route_absent_acknowledged'
            )
        )
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_terminal_substrate_preflight_drift',
            DETAIL = pg_catalog.format(
                'owner=%s schema_owner=%s sessions=%s prepared=%s collisions=%s definitions=%s',
                common_owner,
                private_schema_owner,
                other_client_session_count,
                prepared_transaction_count,
                collision_count,
                invalid_definition_count
            );
    END IF;
END;
$preflight$;

ALTER TABLE public.runtime_drain_intents_v2
DROP CONSTRAINT runtime_drain_intents_v2_state_check,
ADD CONSTRAINT runtime_drain_intents_v2_state_check CHECK (
    intent_state IN (
        'pending',
        'route_absent_acknowledged',
        'consumed',
        'cancelled'
    )
);

CREATE TABLE public.runtime_product_drain_terminal_actions_v2 (
    terminal_action_id TEXT PRIMARY KEY,
    terminal_kind TEXT NOT NULL,
    drain_intent_id TEXT NOT NULL,
    product_operation_id TEXT NOT NULL,
    product_mutation_digest TEXT NOT NULL,
    drain_intent_digest TEXT NOT NULL,
    product_action_idempotency_digest TEXT NOT NULL,
    product_action_semantic_request_digest TEXT NOT NULL,
    cancellation_reason_digest TEXT,
    source_intent_revision BIGINT NOT NULL,
    source_canonical_state_digest TEXT NOT NULL,
    result_intent_revision BIGINT NOT NULL,
    result_canonical_state_digest TEXT NOT NULL,
    source_deployment_revision BIGINT NOT NULL,
    source_result_deployment_revision BIGINT NOT NULL,
    source_result_deployment_snapshot_digest TEXT NOT NULL,
    result_deployment_id TEXT,
    result_deployment_revision BIGINT,
    result_deployment_snapshot_digest TEXT,
    source_slot_writer_epoch BIGINT NOT NULL,
    successor_slot_writer_epoch BIGINT NOT NULL,
    terminal_database_time TIMESTAMPTZ NOT NULL,
    product_receipt_id TEXT NOT NULL,
    product_audit_event_id TEXT NOT NULL,
    authority_observation_digest TEXT NOT NULL,
    installation_authority_revision BIGINT NOT NULL,
    terminal_projection_bytes BYTEA NOT NULL,
    terminal_projection_digest TEXT NOT NULL,
    CONSTRAINT runtime_product_drain_terminal_actions_v2_drain_unique
        UNIQUE (drain_intent_id),
    CONSTRAINT runtime_product_drain_terminal_actions_v2_action_unique
        UNIQUE (
            terminal_kind,
            product_action_idempotency_digest
        ),
    CONSTRAINT runtime_product_drain_terminal_actions_v2_product_fk
        FOREIGN KEY (product_operation_id)
        REFERENCES public.runtime_product_operations_v2(
            product_operation_id
        )
        ON DELETE RESTRICT,
    CONSTRAINT runtime_product_drain_terminal_actions_v2_drain_fk
        FOREIGN KEY (drain_intent_id)
        REFERENCES public.runtime_drain_intents_v2(
            drain_intent_id
        )
        ON DELETE RESTRICT,
    CONSTRAINT runtime_product_drain_terminal_actions_v2_id_check
        CHECK (
            terminal_action_id ~ '^[0-9a-f]{64}$'
            AND drain_intent_id ~ '^[0-9a-f]{32}$'
            AND product_operation_id ~ '^[0-9a-f]{32}$'
            AND product_receipt_id ~ '^[0-9a-f]{64}$'
            AND product_audit_event_id ~ '^[0-9a-f]{64}$'
        ),
    CONSTRAINT runtime_product_drain_terminal_actions_v2_kind_check
        CHECK (
            (
                terminal_kind = 'consumed'
                AND cancellation_reason_digest IS NULL
            )
            OR (
                terminal_kind = 'cancelled'
                AND cancellation_reason_digest IS NOT NULL
                AND cancellation_reason_digest
                    ~ '^[0-9a-f]{64}$'
            )
        ),
    CONSTRAINT runtime_product_drain_terminal_actions_v2_digest_check
        CHECK (
            product_mutation_digest ~ '^[0-9a-f]{64}$'
            AND drain_intent_digest ~ '^[0-9a-f]{64}$'
            AND product_action_idempotency_digest
                ~ '^[0-9a-f]{64}$'
            AND product_action_semantic_request_digest
                ~ '^[0-9a-f]{64}$'
            AND source_canonical_state_digest
                ~ '^[0-9a-f]{64}$'
            AND result_canonical_state_digest
                ~ '^[0-9a-f]{64}$'
            AND source_result_deployment_snapshot_digest
                ~ '^[0-9a-f]{64}$'
            AND authority_observation_digest
                ~ '^[0-9a-f]{64}$'
        ),
    CONSTRAINT runtime_product_drain_terminal_actions_v2_revision_check
        CHECK (
            source_intent_revision
                BETWEEN 1 AND 9223372036854775806
            AND result_intent_revision =
                source_intent_revision + 1
            AND source_deployment_revision
                BETWEEN 1 AND 9223372036854775806
            AND source_result_deployment_revision =
                source_deployment_revision + 1
            AND (
                (
                    terminal_kind = 'consumed'
                    AND result_deployment_id IS NOT NULL
                    AND result_deployment_id
                        ~ '^[0-9a-f]{32}$'
                    AND result_deployment_revision IS NOT NULL
                    AND result_deployment_revision = 1
                    AND result_deployment_snapshot_digest IS NOT NULL
                    AND result_deployment_snapshot_digest
                        ~ '^[0-9a-f]{64}$'
                )
                OR (
                    terminal_kind = 'cancelled'
                    AND result_deployment_id IS NULL
                    AND result_deployment_revision IS NULL
                    AND result_deployment_snapshot_digest IS NULL
                )
            )
            AND installation_authority_revision
                BETWEEN 1 AND 9223372036854775807
        ),
    CONSTRAINT runtime_product_drain_terminal_actions_v2_epoch_check
        CHECK (
            source_slot_writer_epoch
                BETWEEN 1 AND 9223372036854775806
            AND successor_slot_writer_epoch =
                source_slot_writer_epoch + 1
        ),
    CONSTRAINT runtime_product_drain_terminal_actions_v2_time_check
        CHECK (
            pg_catalog.isfinite(terminal_database_time)
            AND (
                EXTRACT(
                    EPOCH FROM terminal_database_time
                ) * 1000000
            )::NUMERIC BETWEEN
                -62135596800000000 AND 253402300799999999
        ),
    CONSTRAINT runtime_product_drain_terminal_actions_v2_projection_check
        CHECK (
            pg_catalog.octet_length(terminal_projection_bytes)
                BETWEEN 1 AND 2097152
            AND terminal_projection_digest
                ~ '^[0-9a-f]{64}$'
            AND terminal_projection_digest =
                pg_catalog.encode(
                    pg_catalog.sha256(terminal_projection_bytes),
                    'hex'
                )
        )
);

CREATE INDEX runtime_product_drain_terminal_actions_v2_semantic_lookup
ON public.runtime_product_drain_terminal_actions_v2 (
    terminal_kind,
    product_action_semantic_request_digest
);

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_pending_drain_consumed_state_v2(
    source_row public.runtime_drain_intents_v2,
    requested_resulting_revision BIGINT,
    requested_terminal_time TIMESTAMPTZ
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
    key_text TEXT;
    terminal_microseconds NUMERIC;
BEGIN
    key_text :=
        starring_runtime_private_v2.starring_runtime_pending_drain_key_text_v2(
            source_row
        );
    terminal_microseconds :=
        EXTRACT(EPOCH FROM requested_terminal_time)
        * 1000000;
    IF source_row.intent_state <> 'route_absent_acknowledged'
        OR source_row.intent_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR requested_resulting_revision <> 1
        OR NOT pg_catalog.isfinite(requested_terminal_time)
        OR terminal_microseconds NOT BETWEEN
            -62135596800000000 AND 253402300799999999
        OR terminal_microseconds <> pg_catalog.trunc(terminal_microseconds)
        OR key_text IS NULL
        OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
            source_row
        )
    THEN
        RETURN NULL;
    END IF;

    RETURN pg_catalog.convert_to(
        pg_catalog.concat(
            '{"format_version":2,"root":{"key":',
            key_text,
            ',"drain_intent_digest":',
            pg_catalog.to_json(source_row.drain_intent_digest)::TEXT,
            '},"intent_revision":',
            (source_row.intent_revision + 1)::TEXT,
            ',"state":{"kind":"consumed","resulting_revision":',
            requested_resulting_revision::TEXT,
            ',"consumed_at_unix_microseconds":',
            terminal_microseconds::BIGINT::TEXT,
            '}}'
        ),
        'UTF8'
    );
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_pending_drain_cancelled_state_v2(
    source_row public.runtime_drain_intents_v2,
    requested_terminal_time TIMESTAMPTZ
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
    key_text TEXT;
    terminal_microseconds NUMERIC;
BEGIN
    key_text :=
        starring_runtime_private_v2.starring_runtime_pending_drain_key_text_v2(
            source_row
        );
    terminal_microseconds :=
        EXTRACT(EPOCH FROM requested_terminal_time)
        * 1000000;
    IF source_row.intent_state <> 'route_absent_acknowledged'
        OR source_row.intent_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR NOT pg_catalog.isfinite(requested_terminal_time)
        OR terminal_microseconds NOT BETWEEN
            -62135596800000000 AND 253402300799999999
        OR terminal_microseconds <> pg_catalog.trunc(terminal_microseconds)
        OR key_text IS NULL
        OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
            source_row
        )
    THEN
        RETURN NULL;
    END IF;

    RETURN pg_catalog.convert_to(
        pg_catalog.concat(
            '{"format_version":2,"root":{"key":',
            key_text,
            ',"drain_intent_digest":',
            pg_catalog.to_json(source_row.drain_intent_digest)::TEXT,
            '},"intent_revision":',
            (source_row.intent_revision + 1)::TEXT,
            ',"state":{"kind":"cancelled",',
            '"cancelled_at_unix_microseconds":',
            terminal_microseconds::BIGINT::TEXT,
            '}}'
        ),
        'UTF8'
    );
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_product_drain_terminal_projection_v2(
    requested_terminal_kind TEXT,
    requested_terminal_action_id TEXT,
    requested_product_action_idempotency_digest TEXT,
    requested_product_action_semantic_request_digest TEXT,
    requested_cancellation_reason_digest TEXT,
    requested_product_operation_id TEXT,
    requested_product_mutation_digest TEXT,
    requested_drain_intent_id TEXT,
    requested_drain_intent_digest TEXT,
    requested_source_intent_revision BIGINT,
    requested_source_canonical_state_digest TEXT,
    requested_result_intent_revision BIGINT,
    requested_result_canonical_state_bytes BYTEA,
    requested_result_canonical_state_digest TEXT,
    requested_source_deployment_revision BIGINT,
    requested_source_result_deployment_revision BIGINT,
    requested_source_result_deployment_snapshot_digest TEXT,
    requested_result_deployment_id TEXT,
    requested_result_deployment_revision BIGINT,
    requested_result_deployment_snapshot_digest TEXT,
    requested_source_slot_writer_epoch BIGINT,
    requested_successor_slot_writer_epoch BIGINT,
    requested_product_receipt_id TEXT,
    requested_product_audit_event_id TEXT,
    requested_authority_observation_digest TEXT,
    requested_installation_authority_revision BIGINT,
    requested_terminal_database_time TIMESTAMPTZ
)
RETURNS BYTEA
LANGUAGE plpgsql
IMMUTABLE
PARALLEL SAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    domain_bytes BYTEA;
    payload_bytes BYTEA;
    projection_bytes BYTEA;
    terminal_microseconds NUMERIC;
    cancellation_reason_bytes BYTEA;
    result_deployment_id_bytes BYTEA;
    result_deployment_snapshot_digest_bytes BYTEA;
BEGIN
    terminal_microseconds :=
        EXTRACT(
            EPOCH FROM requested_terminal_database_time
        ) * 1000000;
    IF requested_terminal_kind IS NULL
        OR requested_terminal_kind NOT IN ('consumed', 'cancelled')
        OR requested_terminal_action_id IS NULL
        OR requested_terminal_action_id
            !~ '^[0-9a-f]{64}$'
        OR requested_product_action_idempotency_digest IS NULL
        OR requested_product_action_idempotency_digest
            !~ '^[0-9a-f]{64}$'
        OR requested_product_action_semantic_request_digest IS NULL
        OR requested_product_action_semantic_request_digest
            !~ '^[0-9a-f]{64}$'
        OR (
            requested_terminal_kind = 'consumed'
            AND requested_cancellation_reason_digest IS NOT NULL
        )
        OR (
            requested_terminal_kind = 'cancelled'
            AND (
                requested_cancellation_reason_digest IS NULL
                OR requested_cancellation_reason_digest
                    !~ '^[0-9a-f]{64}$'
            )
        )
        OR requested_product_operation_id IS NULL
        OR requested_product_operation_id
            !~ '^[0-9a-f]{32}$'
        OR requested_product_mutation_digest IS NULL
        OR requested_product_mutation_digest
            !~ '^[0-9a-f]{64}$'
        OR requested_drain_intent_id IS NULL
        OR requested_drain_intent_id
            !~ '^[0-9a-f]{32}$'
        OR requested_drain_intent_digest IS NULL
        OR requested_drain_intent_digest
            !~ '^[0-9a-f]{64}$'
        OR requested_source_intent_revision IS NULL
        OR requested_source_intent_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR requested_result_intent_revision IS NULL
        OR requested_result_intent_revision
            <> requested_source_intent_revision + 1
        OR requested_source_canonical_state_digest IS NULL
        OR requested_source_canonical_state_digest
            !~ '^[0-9a-f]{64}$'
        OR requested_result_canonical_state_bytes IS NULL
        OR pg_catalog.octet_length(
            requested_result_canonical_state_bytes
        ) NOT BETWEEN 1 AND 1048576
        OR requested_result_canonical_state_digest IS NULL
        OR requested_result_canonical_state_digest
            !~ '^[0-9a-f]{64}$'
        OR requested_result_canonical_state_digest <>
            pg_catalog.encode(
                pg_catalog.sha256(
                    requested_result_canonical_state_bytes
                ),
                'hex'
            )
        OR requested_source_deployment_revision IS NULL
        OR requested_source_deployment_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR requested_source_result_deployment_revision IS NULL
        OR requested_source_result_deployment_revision
            <> requested_source_deployment_revision + 1
        OR requested_source_result_deployment_snapshot_digest IS NULL
        OR requested_source_result_deployment_snapshot_digest
            !~ '^[0-9a-f]{64}$'
        OR (
            requested_terminal_kind = 'consumed'
            AND (
                requested_result_deployment_id IS NULL
                OR requested_result_deployment_id
                    !~ '^[0-9a-f]{32}$'
                OR requested_result_deployment_revision IS NULL
                OR requested_result_deployment_revision <> 1
                OR requested_result_deployment_snapshot_digest IS NULL
                OR requested_result_deployment_snapshot_digest
                    !~ '^[0-9a-f]{64}$'
            )
        )
        OR (
            requested_terminal_kind = 'cancelled'
            AND (
                requested_result_deployment_id IS NOT NULL
                OR requested_result_deployment_revision IS NOT NULL
                OR requested_result_deployment_snapshot_digest IS NOT NULL
            )
        )
        OR requested_source_slot_writer_epoch IS NULL
        OR requested_source_slot_writer_epoch
            NOT BETWEEN 1 AND 9223372036854775806
        OR requested_successor_slot_writer_epoch IS NULL
        OR requested_successor_slot_writer_epoch
            <> requested_source_slot_writer_epoch + 1
        OR requested_product_receipt_id IS NULL
        OR requested_product_receipt_id
            !~ '^[0-9a-f]{64}$'
        OR requested_product_audit_event_id IS NULL
        OR requested_product_audit_event_id
            !~ '^[0-9a-f]{64}$'
        OR requested_authority_observation_digest IS NULL
        OR requested_authority_observation_digest
            !~ '^[0-9a-f]{64}$'
        OR requested_installation_authority_revision IS NULL
        OR requested_installation_authority_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR requested_terminal_database_time IS NULL
        OR NOT pg_catalog.isfinite(
            requested_terminal_database_time
        )
        OR terminal_microseconds NOT BETWEEN
            -62135596800000000 AND 253402300799999999
        OR terminal_microseconds <> pg_catalog.trunc(
            terminal_microseconds
        )
    THEN
        RETURN NULL;
    END IF;

    domain_bytes :=
        pg_catalog.convert_to(
            'starring.runtime.product_drain.terminal.v2',
            'UTF8'
        )
        || pg_catalog.decode('00', 'hex');
    cancellation_reason_bytes := CASE
        WHEN requested_cancellation_reason_digest IS NULL
        THEN ''::BYTEA
        ELSE pg_catalog.convert_to(
            requested_cancellation_reason_digest,
            'UTF8'
        )
    END;
    result_deployment_id_bytes := CASE
        WHEN requested_result_deployment_id IS NULL
        THEN ''::BYTEA
        ELSE pg_catalog.convert_to(
            requested_result_deployment_id,
            'UTF8'
        )
    END;
    result_deployment_snapshot_digest_bytes := CASE
        WHEN requested_result_deployment_snapshot_digest IS NULL
        THEN ''::BYTEA
        ELSE pg_catalog.convert_to(
            requested_result_deployment_snapshot_digest,
            'UTF8'
        )
    END;
    payload_bytes :=
        pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(
                    requested_terminal_kind,
                    'UTF8'
                )
            )::BIGINT
        )
        || pg_catalog.convert_to(requested_terminal_kind, 'UTF8')
        || pg_catalog.int8send(64)
        || pg_catalog.convert_to(requested_terminal_action_id, 'UTF8')
        || pg_catalog.int8send(64)
        || pg_catalog.convert_to(
            requested_product_action_idempotency_digest,
            'UTF8'
        )
        || pg_catalog.int8send(64)
        || pg_catalog.convert_to(
            requested_product_action_semantic_request_digest,
            'UTF8'
        )
        || pg_catalog.int8send(
            pg_catalog.octet_length(cancellation_reason_bytes)::BIGINT
        )
        || cancellation_reason_bytes
        || pg_catalog.int8send(32)
        || pg_catalog.convert_to(requested_product_operation_id, 'UTF8')
        || pg_catalog.int8send(64)
        || pg_catalog.convert_to(
            requested_product_mutation_digest,
            'UTF8'
        )
        || pg_catalog.int8send(32)
        || pg_catalog.convert_to(requested_drain_intent_id, 'UTF8')
        || pg_catalog.int8send(64)
        || pg_catalog.convert_to(
            requested_drain_intent_digest,
            'UTF8'
        )
        || pg_catalog.int8send(8)
        || pg_catalog.int8send(requested_source_intent_revision)
        || pg_catalog.int8send(64)
        || pg_catalog.convert_to(
            requested_source_canonical_state_digest,
            'UTF8'
        )
        || pg_catalog.int8send(8)
        || pg_catalog.int8send(requested_result_intent_revision)
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                requested_result_canonical_state_bytes
            )::BIGINT
        )
        || requested_result_canonical_state_bytes
        || pg_catalog.int8send(64)
        || pg_catalog.convert_to(
            requested_result_canonical_state_digest,
            'UTF8'
        )
        || pg_catalog.int8send(8)
        || pg_catalog.int8send(
            requested_source_deployment_revision
        )
        || pg_catalog.int8send(8)
        || pg_catalog.int8send(
            requested_source_result_deployment_revision
        )
        || pg_catalog.int8send(64)
        || pg_catalog.convert_to(
            requested_source_result_deployment_snapshot_digest,
            'UTF8'
        )
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                result_deployment_id_bytes
            )::BIGINT
        )
        || result_deployment_id_bytes
        || pg_catalog.int8send(8)
        || pg_catalog.int8send(
            COALESCE(requested_result_deployment_revision, 0)
        )
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                result_deployment_snapshot_digest_bytes
            )::BIGINT
        )
        || result_deployment_snapshot_digest_bytes
        || pg_catalog.int8send(8)
        || pg_catalog.int8send(
            requested_source_slot_writer_epoch
        )
        || pg_catalog.int8send(8)
        || pg_catalog.int8send(
            requested_successor_slot_writer_epoch
        )
        || pg_catalog.int8send(64)
        || pg_catalog.convert_to(requested_product_receipt_id, 'UTF8')
        || pg_catalog.int8send(64)
        || pg_catalog.convert_to(
            requested_product_audit_event_id,
            'UTF8'
        )
        || pg_catalog.int8send(64)
        || pg_catalog.convert_to(
            requested_authority_observation_digest,
            'UTF8'
        )
        || pg_catalog.int8send(8)
        || pg_catalog.int8send(
            requested_installation_authority_revision
        )
        || pg_catalog.int8send(8)
        || pg_catalog.int8send(terminal_microseconds::BIGINT);
    projection_bytes :=
        pg_catalog.int8send(
            pg_catalog.octet_length(domain_bytes)::BIGINT
        )
        || domain_bytes
        || pg_catalog.int2send(2::SMALLINT)
        || pg_catalog.int2send(
            CASE requested_terminal_kind
                WHEN 'consumed' THEN 0
                ELSE 1
            END::SMALLINT
        )
        || payload_bytes
        || pg_catalog.sha256(payload_bytes);
    IF pg_catalog.octet_length(projection_bytes)
            NOT BETWEEN 1 AND 2097152
    THEN
        RETURN NULL;
    END IF;
    RETURN projection_bytes;
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_product_drain_terminal_action_exact_v2(
    action_row public.runtime_product_drain_terminal_actions_v2,
    product_row public.runtime_product_operations_v2,
    drain_row public.runtime_drain_intents_v2
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
    state_terminal_microseconds NUMERIC;
    recorded_terminal_microseconds NUMERIC;
    expected_projection BYTEA;
BEGIN
    BEGIN
        state_value := pg_catalog.convert_from(
            drain_row.canonical_state_bytes,
            'UTF8'
        )::JSONB;
    EXCEPTION
        WHEN OTHERS THEN
            RETURN FALSE;
    END;

    state_terminal_microseconds := CASE action_row.terminal_kind
        WHEN 'consumed' THEN (
            state_value #>>
                '{state,consumed_at_unix_microseconds}'
        )::NUMERIC
        WHEN 'cancelled' THEN (
            state_value #>>
                '{state,cancelled_at_unix_microseconds}'
        )::NUMERIC
        ELSE NULL
    END;
    recorded_terminal_microseconds :=
        EXTRACT(
            EPOCH FROM action_row.terminal_database_time
        ) * 1000000;

    IF action_row.product_operation_id
            <> product_row.product_operation_id
        OR action_row.product_operation_id
            <> drain_row.product_operation_id
        OR action_row.drain_intent_id
            <> drain_row.drain_intent_id
        OR action_row.product_mutation_digest
            <> product_row.product_mutation_digest
        OR action_row.product_mutation_digest
            <> drain_row.product_mutation_digest
        OR action_row.drain_intent_digest
            <> drain_row.drain_intent_digest
        OR product_row.tenant_id <> drain_row.tenant_id
        OR product_row.installation_id
            <> drain_row.installation_id
        OR product_row.deployment_id
            <> drain_row.deployment_id
        OR product_row.expected_revision
            <> drain_row.expected_revision
        OR product_row.expected_revision
            <> action_row.source_deployment_revision
        OR action_row.source_result_deployment_revision
            <> action_row.source_deployment_revision + 1
        OR action_row.source_result_deployment_snapshot_digest
            !~ '^[0-9a-f]{64}$'
        OR product_row.expected_target_guild_id
            <> drain_row.slot_guild_id
        OR product_row.expected_target_ruleset_key
            <> drain_row.slot_ruleset_key
        OR drain_row.intent_state <> action_row.terminal_kind
        OR drain_row.intent_revision
            <> action_row.result_intent_revision
        OR drain_row.canonical_state_digest
            <> action_row.result_canonical_state_digest
        OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
            drain_row
        )
        OR state_terminal_microseconds
            IS DISTINCT FROM recorded_terminal_microseconds
        OR (
            action_row.terminal_kind = 'consumed'
            AND (
                action_row.cancellation_reason_digest IS NOT NULL
                OR action_row.result_deployment_id IS NULL
                OR action_row.result_deployment_id
                    !~ '^[0-9a-f]{32}$'
                OR action_row.result_deployment_id =
                    drain_row.deployment_id
                OR action_row.result_deployment_revision
                    IS DISTINCT FROM 1
                OR action_row.result_deployment_snapshot_digest IS NULL
                OR action_row.result_deployment_snapshot_digest
                    !~ '^[0-9a-f]{64}$'
                OR (
                    state_value #>> '{state,resulting_revision}'
                )::NUMERIC IS DISTINCT FROM 1
            )
        )
        OR (
            action_row.terminal_kind = 'cancelled'
            AND (
                action_row.cancellation_reason_digest IS NULL
                OR action_row.cancellation_reason_digest
                    !~ '^[0-9a-f]{64}$'
                OR action_row.result_deployment_id IS NOT NULL
                OR action_row.result_deployment_revision IS NOT NULL
                OR action_row.result_deployment_snapshot_digest IS NOT NULL
            )
        )
    THEN
        RETURN FALSE;
    END IF;

    expected_projection :=
        starring_runtime_private_v2.starring_runtime_product_drain_terminal_projection_v2(
            action_row.terminal_kind,
            action_row.terminal_action_id,
            action_row.product_action_idempotency_digest,
            action_row.product_action_semantic_request_digest,
            action_row.cancellation_reason_digest,
            action_row.product_operation_id,
            action_row.product_mutation_digest,
            action_row.drain_intent_id,
            action_row.drain_intent_digest,
            action_row.source_intent_revision,
            action_row.source_canonical_state_digest,
            action_row.result_intent_revision,
            drain_row.canonical_state_bytes,
            action_row.result_canonical_state_digest,
            action_row.source_deployment_revision,
            action_row.source_result_deployment_revision,
            action_row.source_result_deployment_snapshot_digest,
            action_row.result_deployment_id,
            action_row.result_deployment_revision,
            action_row.result_deployment_snapshot_digest,
            action_row.source_slot_writer_epoch,
            action_row.successor_slot_writer_epoch,
            action_row.product_receipt_id,
            action_row.product_audit_event_id,
            action_row.authority_observation_digest,
            action_row.installation_authority_revision,
            action_row.terminal_database_time
        );
    RETURN expected_projection IS NOT NULL
        AND action_row.terminal_projection_bytes =
            expected_projection
        AND action_row.terminal_projection_digest =
            pg_catalog.encode(
                pg_catalog.sha256(expected_projection),
                'hex'
            );
EXCEPTION
    WHEN OTHERS THEN
        RETURN FALSE;
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.reject_runtime_product_drain_terminal_action_mutation_v2()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    RAISE EXCEPTION USING
        ERRCODE = '23514',
        MESSAGE = 'runtime_product_drain_terminal_action_mutation_rejected';
END;
$function$;

CREATE TRIGGER runtime_product_drain_terminal_actions_v2_reject_row_mutation
BEFORE UPDATE OR DELETE
ON public.runtime_product_drain_terminal_actions_v2
FOR EACH ROW
EXECUTE FUNCTION starring_runtime_private_v2.reject_runtime_product_drain_terminal_action_mutation_v2();

CREATE TRIGGER runtime_product_drain_terminal_actions_v2_reject_truncate
BEFORE TRUNCATE
ON public.runtime_product_drain_terminal_actions_v2
FOR EACH STATEMENT
EXECUTE FUNCTION starring_runtime_private_v2.reject_runtime_product_drain_terminal_action_mutation_v2();

DO $patch_canonical_validator$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(public.runtime_drain_intents_v2)'
    );

    previous_fragment :=
        '    ELSE' || E'\n' ||
        '        RETURN FALSE;' || E'\n' ||
        '    END IF;' || E'\n' ||
        E'\n' ||
        '    expected_bytes := pg_catalog.convert_to(';
    next_fragment :=
        '    ELSIF kind_value = ''consumed'' THEN' || E'\n' ||
        '        IF drain_row.intent_state <> ''consumed''' || E'\n' ||
        '            OR (' || E'\n' ||
        '                SELECT pg_catalog.count(*)' || E'\n' ||
        '                FROM pg_catalog.jsonb_object_keys(state_body)' || E'\n' ||
        '            ) <> 3' || E'\n' ||
        '            OR state_body ->> ''resulting_revision''' || E'\n' ||
        '                !~ ''^[1-9][0-9]{0,18}$''' || E'\n' ||
        '            OR (' || E'\n' ||
        '                state_body ->> ''resulting_revision''' || E'\n' ||
        '            )::NUMERIC > 9223372036854775807' || E'\n' ||
        '            OR state_body' || E'\n' ||
        '                ->> ''consumed_at_unix_microseconds''' || E'\n' ||
        '                !~ ''^-?(0|[1-9][0-9]{0,18})$''' || E'\n' ||
        '            OR (' || E'\n' ||
        '                state_body' || E'\n' ||
        '                    ->> ''consumed_at_unix_microseconds''' || E'\n' ||
        '            )::NUMERIC NOT BETWEEN' || E'\n' ||
        '                -62135596800000000 AND 253402300799999999' || E'\n' ||
        '        THEN' || E'\n' ||
        '            RETURN FALSE;' || E'\n' ||
        '        END IF;' || E'\n' ||
        '        expected_state_text := pg_catalog.concat(' || E'\n' ||
        '            ''{"kind":"consumed","resulting_revision":'',' || E'\n' ||
        '            state_body ->> ''resulting_revision'',' || E'\n' ||
        '            '',"consumed_at_unix_microseconds":'',' || E'\n' ||
        '            state_body ->> ''consumed_at_unix_microseconds'',' || E'\n' ||
        '            ''}''' || E'\n' ||
        '        );' || E'\n' ||
        '    ELSIF kind_value = ''cancelled'' THEN' || E'\n' ||
        '        IF drain_row.intent_state <> ''cancelled''' || E'\n' ||
        '            OR (' || E'\n' ||
        '                SELECT pg_catalog.count(*)' || E'\n' ||
        '                FROM pg_catalog.jsonb_object_keys(state_body)' || E'\n' ||
        '            ) <> 2' || E'\n' ||
        '            OR state_body' || E'\n' ||
        '                ->> ''cancelled_at_unix_microseconds''' || E'\n' ||
        '                !~ ''^-?(0|[1-9][0-9]{0,18})$''' || E'\n' ||
        '            OR (' || E'\n' ||
        '                state_body' || E'\n' ||
        '                    ->> ''cancelled_at_unix_microseconds''' || E'\n' ||
        '            )::NUMERIC NOT BETWEEN' || E'\n' ||
        '                -62135596800000000 AND 253402300799999999' || E'\n' ||
        '        THEN' || E'\n' ||
        '            RETURN FALSE;' || E'\n' ||
        '        END IF;' || E'\n' ||
        '        expected_state_text := pg_catalog.concat(' || E'\n' ||
        '            ''{"kind":"cancelled",'',' || E'\n' ||
        '            ''"cancelled_at_unix_microseconds":'',' || E'\n' ||
        '            state_body ->> ''cancelled_at_unix_microseconds'',' || E'\n' ||
        '            ''}''' || E'\n' ||
        '        );' || E'\n' ||
        '    ELSE' || E'\n' ||
        '        RETURN FALSE;' || E'\n' ||
        '    END IF;' || E'\n' ||
        E'\n' ||
        '    expected_bytes := pg_catalog.convert_to(';
    IF definition IS NULL
        OR pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_terminal_validator_patch_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$patch_canonical_validator$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_product_drain_terminal_transition_v2(
    requested_drain_intent_id TEXT,
    requested_source_intent_revision BIGINT,
    requested_source_state_digest TEXT,
    requested_terminal_kind TEXT,
    requested_resulting_deployment_revision BIGINT,
    requested_terminal_time TIMESTAMPTZ
)
RETURNS public.runtime_drain_intents_v2
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    source_row public.runtime_drain_intents_v2%ROWTYPE;
    result_row public.runtime_drain_intents_v2%ROWTYPE;
    result_state_bytes BYTEA;
    result_state_digest TEXT;
    terminal_microseconds NUMERIC;
    source_state_value JSONB;
    acknowledged_microseconds NUMERIC;
    setting_name TEXT;
BEGIN
    terminal_microseconds :=
        EXTRACT(EPOCH FROM requested_terminal_time)
        * 1000000;
    IF requested_drain_intent_id !~ '^[0-9a-f]{32}$'
        OR requested_source_intent_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR requested_source_state_digest
            !~ '^[0-9a-f]{64}$'
        OR requested_terminal_kind
            NOT IN ('consumed', 'cancelled')
        OR requested_resulting_deployment_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR NOT pg_catalog.isfinite(requested_terminal_time)
        OR terminal_microseconds NOT BETWEEN
            -62135596800000000 AND 253402300799999999
        OR terminal_microseconds <> pg_catalog.trunc(
            terminal_microseconds
        )
        OR (
            requested_terminal_kind = 'consumed'
            AND requested_resulting_deployment_revision <> 1
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_product_drain_terminal_transition_input_invalid';
    END IF;

    FOREACH setting_name IN ARRAY ARRAY[
        'starring.runtime_product_drain_first_apply_stage_v2',
        'starring.runtime_product_drain_first_apply_product_operation_id_v2',
        'starring.runtime_product_drain_first_apply_drain_intent_id_v2',
        'starring.runtime_pending_drain_source_revision_v2',
        'starring.runtime_pending_drain_source_digest_v2',
        'starring.runtime_pending_drain_successor_revision_v2',
        'starring.runtime_pending_drain_successor_digest_v2',
        'starring.runtime_product_drain_terminal_kind_v2',
        'starring.runtime_product_drain_terminal_microseconds_v2'
    ]
    LOOP
        IF COALESCE(
            pg_catalog.current_setting(setting_name, TRUE),
            ''
        ) <> ''
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_product_drain_terminal_transition_gate_invalid';
        END IF;
    END LOOP;

    SELECT drain.*
    INTO source_row
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE drain.drain_intent_id = requested_drain_intent_id
    FOR UPDATE;

    IF NOT FOUND
        OR source_row.intent_revision
            <> requested_source_intent_revision
        OR source_row.canonical_state_digest
            <> requested_source_state_digest
        OR source_row.intent_state
            <> 'route_absent_acknowledged'
        OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
            source_row
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_product_drain_terminal_transition_source_stale';
    END IF;

    IF source_row.expected_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR (
            requested_terminal_kind = 'cancelled'
            AND requested_resulting_deployment_revision
                <> source_row.expected_revision + 1
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE =
                'runtime_product_drain_terminal_transition_revision_invalid';
    END IF;

    BEGIN
        source_state_value := pg_catalog.convert_from(
            source_row.canonical_state_bytes,
            'UTF8'
        )::JSONB;
    EXCEPTION
        WHEN OTHERS THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE =
                    'runtime_product_drain_terminal_transition_causal_clock_invalid';
    END;
    IF (
            source_state_value
                #>> '{state,acknowledgement,acknowledged_at_unix_microseconds}'
        ) IS NULL
        OR source_state_value
                #>> '{state,acknowledgement,acknowledged_at_unix_microseconds}'
                !~ '^-?(0|[1-9][0-9]{0,18})$'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE =
                'runtime_product_drain_terminal_transition_causal_clock_invalid';
    END IF;
    acknowledged_microseconds := (
        source_state_value
            #>> '{state,acknowledgement,acknowledged_at_unix_microseconds}'
    )::NUMERIC;
    IF acknowledged_microseconds NOT BETWEEN
            -62135596800000000 AND 253402300799999999
        OR terminal_microseconds < acknowledged_microseconds
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE =
                'runtime_product_drain_terminal_transition_causal_clock_invalid';
    END IF;

    result_state_bytes := CASE requested_terminal_kind
        WHEN 'consumed' THEN
            starring_runtime_private_v2.starring_runtime_pending_drain_consumed_state_v2(
                source_row,
                requested_resulting_deployment_revision,
                requested_terminal_time
            )
        ELSE
            starring_runtime_private_v2.starring_runtime_pending_drain_cancelled_state_v2(
                source_row,
                requested_terminal_time
            )
    END;
    IF result_state_bytes IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_product_drain_terminal_transition_state_invalid';
    END IF;
    result_state_digest := pg_catalog.encode(
        pg_catalog.sha256(result_state_bytes),
        'hex'
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_product_drain_first_apply_stage_v2',
        'terminal_update',
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_product_drain_first_apply_product_operation_id_v2',
        source_row.product_operation_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_product_drain_first_apply_drain_intent_id_v2',
        source_row.drain_intent_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_pending_drain_source_revision_v2',
        source_row.intent_revision::TEXT,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_pending_drain_source_digest_v2',
        source_row.canonical_state_digest,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_pending_drain_successor_revision_v2',
        (source_row.intent_revision + 1)::TEXT,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_pending_drain_successor_digest_v2',
        result_state_digest,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_product_drain_terminal_kind_v2',
        requested_terminal_kind,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_product_drain_terminal_microseconds_v2',
        terminal_microseconds::BIGINT::TEXT,
        TRUE
    );

    UPDATE public.runtime_drain_intents_v2 AS drain
    SET intent_revision = source_row.intent_revision + 1,
        intent_state = requested_terminal_kind,
        canonical_state_bytes = result_state_bytes,
        canonical_state_digest = result_state_digest
    WHERE drain.drain_intent_id = source_row.drain_intent_id
        AND drain.product_operation_id =
            source_row.product_operation_id
        AND drain.intent_revision = source_row.intent_revision
        AND drain.canonical_state_digest =
            source_row.canonical_state_digest
        AND drain.intent_state = 'route_absent_acknowledged'
    RETURNING drain.* INTO result_row;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_product_drain_terminal_transition_source_stale';
    END IF;

    FOREACH setting_name IN ARRAY ARRAY[
        'starring.runtime_product_drain_first_apply_stage_v2',
        'starring.runtime_product_drain_first_apply_product_operation_id_v2',
        'starring.runtime_product_drain_first_apply_drain_intent_id_v2',
        'starring.runtime_pending_drain_source_revision_v2',
        'starring.runtime_pending_drain_source_digest_v2',
        'starring.runtime_pending_drain_successor_revision_v2',
        'starring.runtime_pending_drain_successor_digest_v2',
        'starring.runtime_product_drain_terminal_kind_v2',
        'starring.runtime_product_drain_terminal_microseconds_v2'
    ]
    LOOP
        IF COALESCE(
            pg_catalog.current_setting(setting_name, TRUE),
            ''
        ) <> ''
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_product_drain_terminal_transition_gate_invalid';
        END IF;
    END LOOP;

    IF NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
        result_row
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_product_drain_terminal_transition_result_invalid';
    END IF;
    RETURN result_row;
END;
$function$;

DO $patch_drain_mutation_guard$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.reject_runtime_product_drain_mutation()'
    );

    previous_fragment :=
        '    gate_successor_digest TEXT;' || E'\n' ||
        'BEGIN';
    next_fragment :=
        '    gate_successor_digest TEXT;' || E'\n' ||
        '    gate_terminal_kind TEXT;' || E'\n' ||
        '    gate_terminal_microseconds TEXT;' || E'\n' ||
        '    terminal_state_value JSONB;' || E'\n' ||
        'BEGIN';
    IF definition IS NULL
        OR pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_terminal_guard_declaration_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    gate_successor_digest := pg_catalog.current_setting(' || E'\n' ||
        '        ''starring.runtime_pending_drain_successor_digest_v2'',' || E'\n' ||
        '        TRUE' || E'\n' ||
        '    );';
    next_fragment := previous_fragment || E'\n' ||
        '    gate_terminal_kind := pg_catalog.current_setting(' || E'\n' ||
        '        ''starring.runtime_product_drain_terminal_kind_v2'',' || E'\n' ||
        '        TRUE' || E'\n' ||
        '    );' || E'\n' ||
        '    gate_terminal_microseconds := pg_catalog.current_setting(' || E'\n' ||
        '        ''starring.runtime_product_drain_terminal_microseconds_v2'',' || E'\n' ||
        '        TRUE' || E'\n' ||
        '    );';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_terminal_guard_setting_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '        THEN' || E'\n' ||
        '            IF gate_stage = ''pending_drain_recovery_update''';
    next_fragment :=
        '        THEN' || E'\n' ||
        '            IF gate_stage = ''terminal_update''' || E'\n' ||
        '                AND gate_terminal_kind IN (' || E'\n' ||
        '                    ''consumed'',' || E'\n' ||
        '                    ''cancelled''' || E'\n' ||
        '                )' || E'\n' ||
        '                AND gate_drain_intent_id = OLD.drain_intent_id' || E'\n' ||
        '                AND gate_product_operation_id =' || E'\n' ||
        '                    OLD.product_operation_id' || E'\n' ||
        '                AND gate_source_revision =' || E'\n' ||
        '                    OLD.intent_revision::TEXT' || E'\n' ||
        '                AND gate_source_digest =' || E'\n' ||
        '                    OLD.canonical_state_digest' || E'\n' ||
        '                AND gate_successor_revision =' || E'\n' ||
        '                    NEW.intent_revision::TEXT' || E'\n' ||
        '                AND gate_successor_digest =' || E'\n' ||
        '                    NEW.canonical_state_digest' || E'\n' ||
        '                AND OLD.intent_state =' || E'\n' ||
        '                    ''route_absent_acknowledged''' || E'\n' ||
        '                AND starring_runtime_private_v2.' || E'\n' ||
        'starring_runtime_pending_drain_state_exact_v2(OLD)' || E'\n' ||
        '                AND NEW.intent_revision =' || E'\n' ||
        '                    OLD.intent_revision + 1' || E'\n' ||
        '                AND NEW.intent_state = gate_terminal_kind' || E'\n' ||
        '                AND NEW.drain_intent_id = OLD.drain_intent_id' || E'\n' ||
        '                AND NEW.tenant_id = OLD.tenant_id' || E'\n' ||
        '                AND NEW.installation_id = OLD.installation_id' || E'\n' ||
        '                AND NEW.deployment_id = OLD.deployment_id' || E'\n' ||
        '                AND NEW.slot_guild_id = OLD.slot_guild_id' || E'\n' ||
        '                AND NEW.slot_ruleset_key = OLD.slot_ruleset_key' || E'\n' ||
        '                AND NEW.expected_revision = OLD.expected_revision' || E'\n' ||
        '                AND NEW.product_operation_id =' || E'\n' ||
        '                    OLD.product_operation_id' || E'\n' ||
        '                AND NEW.product_mutation_digest =' || E'\n' ||
        '                    OLD.product_mutation_digest' || E'\n' ||
        '                AND NEW.drain_intent_request_bytes =' || E'\n' ||
        '                    OLD.drain_intent_request_bytes' || E'\n' ||
        '                AND NEW.drain_intent_digest =' || E'\n' ||
        '                    OLD.drain_intent_digest' || E'\n' ||
        '                AND starring_runtime_private_v2.' || E'\n' ||
        'starring_runtime_pending_drain_state_exact_v2(NEW)' || E'\n' ||
        '            THEN' || E'\n' ||
        '                terminal_state_value := pg_catalog.convert_from(' || E'\n' ||
        '                    NEW.canonical_state_bytes,' || E'\n' ||
        '                    ''UTF8''' || E'\n' ||
        '                )::JSONB;' || E'\n' ||
        '                IF gate_terminal_microseconds IS DISTINCT FROM (' || E'\n' ||
        '                    CASE gate_terminal_kind' || E'\n' ||
        '                        WHEN ''consumed'' THEN terminal_state_value' || E'\n' ||
        '                            #>> ''{state,consumed_at_unix_microseconds}''' || E'\n' ||
        '                        ELSE terminal_state_value' || E'\n' ||
        '                            #>> ''{state,cancelled_at_unix_microseconds}''' || E'\n' ||
        '                    END' || E'\n' ||
        '                )' || E'\n' ||
        '                THEN' || E'\n' ||
        '                    RAISE EXCEPTION USING' || E'\n' ||
        '                        ERRCODE = ''23514'',' || E'\n' ||
        '                        MESSAGE =' || E'\n' ||
        '                            ''runtime_product_drain_mutation_rejected'';' || E'\n' ||
        '                END IF;' || E'\n' ||
        '                PERFORM pg_catalog.set_config(' || E'\n' ||
        '                    setting_name,' || E'\n' ||
        '                    '''',' || E'\n' ||
        '                    TRUE' || E'\n' ||
        '                )' || E'\n' ||
        '                FROM pg_catalog.unnest(ARRAY[' || E'\n' ||
        '                    ''starring.runtime_product_drain_first_apply_stage_v2'',' || E'\n' ||
        '                    ''starring.runtime_product_drain_first_apply_product_operation_id_v2'',' || E'\n' ||
        '                    ''starring.runtime_product_drain_first_apply_drain_intent_id_v2'',' || E'\n' ||
        '                    ''starring.runtime_pending_drain_source_revision_v2'',' || E'\n' ||
        '                    ''starring.runtime_pending_drain_source_digest_v2'',' || E'\n' ||
        '                    ''starring.runtime_pending_drain_successor_revision_v2'',' || E'\n' ||
        '                    ''starring.runtime_pending_drain_successor_digest_v2'',' || E'\n' ||
        '                    ''starring.runtime_product_drain_terminal_kind_v2'',' || E'\n' ||
        '                    ''starring.runtime_product_drain_terminal_microseconds_v2''' || E'\n' ||
        '                ]) AS settings(setting_name);' || E'\n' ||
        '                RETURN NEW;' || E'\n' ||
        '            END IF;' || E'\n' ||
        '            IF gate_stage = ''pending_drain_recovery_update''';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_terminal_guard_branch_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$patch_drain_mutation_guard$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_slot_writer_fence_terminal_release_v2(
    requested_slot_guild_id TEXT,
    requested_slot_ruleset_key TEXT,
    requested_source_epoch BIGINT,
    requested_drain_intent_id TEXT,
    requested_product_operation_id TEXT,
    requested_tenant_id TEXT,
    requested_installation_id TEXT,
    requested_deployment_id TEXT,
    requested_expected_revision BIGINT,
    requested_source_intent_revision BIGINT,
    requested_source_state_bytes BYTEA,
    requested_source_state_digest TEXT,
    requested_result_intent_revision BIGINT,
    requested_result_state_digest TEXT,
    requested_terminal_kind TEXT,
    requested_terminal_time TIMESTAMPTZ
)
RETURNS BIGINT
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    fence_row public.runtime_slot_writer_fences_v2%ROWTYPE;
    terminal_row public.runtime_drain_intents_v2%ROWTYPE;
    source_row public.runtime_drain_intents_v2%ROWTYPE;
    source_state_value JSONB;
    terminal_state_value JSONB;
    terminal_microseconds NUMERIC;
    acknowledged_microseconds NUMERIC;
    successor_epoch BIGINT;
    setting_name TEXT;
BEGIN
    terminal_microseconds :=
        EXTRACT(EPOCH FROM requested_terminal_time)
        * 1000000;
    IF requested_slot_guild_id !~ '^[1-9][0-9]{0,19}$'
        OR (
            pg_catalog.length(requested_slot_guild_id) = 20
            AND requested_slot_guild_id
                COLLATE pg_catalog."C" >
                '18446744073709551615'
                COLLATE pg_catalog."C"
        )
        OR requested_slot_ruleset_key
            !~ '^[A-Za-z0-9_-]{1,64}$'
        OR requested_source_epoch
            NOT BETWEEN 1 AND 9223372036854775806
        OR requested_drain_intent_id !~ '^[0-9a-f]{32}$'
        OR requested_product_operation_id !~ '^[0-9a-f]{32}$'
        OR requested_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR requested_installation_id
            !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR requested_deployment_id
            !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR requested_expected_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR requested_source_intent_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR pg_catalog.octet_length(requested_source_state_bytes)
            NOT BETWEEN 1 AND 1048576
        OR requested_source_state_digest !~ '^[0-9a-f]{64}$'
        OR requested_source_state_digest <>
            pg_catalog.encode(
                pg_catalog.sha256(requested_source_state_bytes),
                'hex'
            )
        OR requested_result_intent_revision
            <> requested_source_intent_revision + 1
        OR requested_result_state_digest !~ '^[0-9a-f]{64}$'
        OR requested_terminal_kind
            NOT IN ('consumed', 'cancelled')
        OR NOT pg_catalog.isfinite(requested_terminal_time)
        OR terminal_microseconds NOT BETWEEN
            -62135596800000000 AND 253402300799999999
        OR terminal_microseconds <> pg_catalog.trunc(
            terminal_microseconds
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_slot_writer_fence_terminal_release_input_invalid';
    END IF;

    FOREACH setting_name IN ARRAY ARRAY[
        'starring.runtime_slot_writer_fence_action_v2',
        'starring.runtime_slot_writer_fence_slot_guild_id_v2',
        'starring.runtime_slot_writer_fence_slot_ruleset_key_v2',
        'starring.runtime_slot_writer_fence_expected_epoch_v2',
        'starring.runtime_slot_writer_fence_drain_intent_id_v2',
        'starring.runtime_slot_writer_fence_product_operation_id_v2',
        'starring.runtime_slot_writer_fence_tenant_id_v2',
        'starring.runtime_slot_writer_fence_installation_id_v2',
        'starring.runtime_slot_writer_fence_deployment_id_v2',
        'starring.runtime_slot_writer_fence_expected_revision_v2',
        'starring.runtime_slot_writer_fence_marked_at_v2',
        'starring.runtime_slot_writer_fence_source_intent_revision_v2',
        'starring.runtime_slot_writer_fence_source_state_digest_v2',
        'starring.runtime_slot_writer_fence_result_intent_revision_v2',
        'starring.runtime_slot_writer_fence_result_state_digest_v2',
        'starring.runtime_slot_writer_fence_terminal_kind_v2',
        'starring.runtime_slot_writer_fence_terminal_microseconds_v2',
        'starring.runtime_slot_writer_fence_successor_epoch_v2'
    ]
    LOOP
        IF COALESCE(
            pg_catalog.current_setting(setting_name, TRUE),
            ''
        ) <> ''
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_slot_writer_fence_terminal_release_gate_invalid';
        END IF;
    END LOOP;

    SELECT drain.*
    INTO terminal_row
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE drain.drain_intent_id = requested_drain_intent_id
        AND drain.product_operation_id =
            requested_product_operation_id
        AND drain.tenant_id = requested_tenant_id
        AND drain.installation_id =
            requested_installation_id
        AND drain.deployment_id = requested_deployment_id
        AND drain.slot_guild_id = requested_slot_guild_id
        AND drain.slot_ruleset_key = requested_slot_ruleset_key
        AND drain.expected_revision = requested_expected_revision
        AND drain.intent_revision =
            requested_result_intent_revision
        AND drain.canonical_state_digest =
            requested_result_state_digest
        AND drain.intent_state = requested_terminal_kind
    FOR UPDATE;

    IF NOT FOUND
        OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
            terminal_row
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_slot_writer_fence_terminal_release_source_stale';
    END IF;

    source_row := terminal_row;
    source_row.intent_revision :=
        requested_source_intent_revision;
    source_row.intent_state := 'route_absent_acknowledged';
    source_row.canonical_state_bytes :=
        requested_source_state_bytes;
    source_row.canonical_state_digest :=
        requested_source_state_digest;
    IF NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
        source_row
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_slot_writer_fence_terminal_release_source_invalid';
    END IF;

    BEGIN
        source_state_value := pg_catalog.convert_from(
            requested_source_state_bytes,
            'UTF8'
        )::JSONB;
    EXCEPTION
        WHEN OTHERS THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE =
                    'runtime_slot_writer_fence_terminal_release_causal_clock_invalid';
    END;
    IF (
            source_state_value
                #>> '{state,acknowledgement,acknowledged_at_unix_microseconds}'
        ) IS NULL
        OR source_state_value
                #>> '{state,acknowledgement,acknowledged_at_unix_microseconds}'
                !~ '^-?(0|[1-9][0-9]{0,18})$'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE =
                'runtime_slot_writer_fence_terminal_release_causal_clock_invalid';
    END IF;
    acknowledged_microseconds := (
        source_state_value
            #>> '{state,acknowledgement,acknowledged_at_unix_microseconds}'
    )::NUMERIC;
    IF acknowledged_microseconds NOT BETWEEN
            -62135596800000000 AND 253402300799999999
        OR terminal_microseconds < acknowledged_microseconds
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE =
                'runtime_slot_writer_fence_terminal_release_causal_clock_invalid';
    END IF;

    terminal_state_value := pg_catalog.convert_from(
        terminal_row.canonical_state_bytes,
        'UTF8'
    )::JSONB;
    IF terminal_microseconds::BIGINT::TEXT IS DISTINCT FROM (
        CASE requested_terminal_kind
            WHEN 'consumed' THEN terminal_state_value
                #>> '{state,consumed_at_unix_microseconds}'
            ELSE terminal_state_value
                #>> '{state,cancelled_at_unix_microseconds}'
        END
    )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_slot_writer_fence_terminal_release_result_invalid';
    END IF;

    SELECT fence.*
    INTO fence_row
    FROM public.runtime_slot_writer_fences_v2 AS fence
    WHERE fence.slot_guild_id = requested_slot_guild_id
        AND fence.slot_ruleset_key = requested_slot_ruleset_key
    FOR UPDATE;

    IF NOT FOUND
        OR fence_row.writer_epoch <> requested_source_epoch
        OR fence_row.pending_drain_intent_id
            <> requested_drain_intent_id
        OR fence_row.pending_product_operation_id
            <> requested_product_operation_id
        OR fence_row.pending_tenant_id <> requested_tenant_id
        OR fence_row.pending_installation_id
            <> requested_installation_id
        OR fence_row.pending_deployment_id
            <> requested_deployment_id
        OR fence_row.pending_expected_revision
            <> requested_expected_revision
        OR fence_row.pending_marked_at IS NULL
        OR requested_terminal_time < fence_row.updated_at
        OR requested_terminal_time < fence_row.pending_marked_at
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_slot_writer_fence_terminal_release_source_stale';
    END IF;
    successor_epoch := requested_source_epoch + 1;

    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_action_v2',
        'terminal_release',
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_slot_guild_id_v2',
        requested_slot_guild_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_slot_ruleset_key_v2',
        requested_slot_ruleset_key,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_expected_epoch_v2',
        requested_source_epoch::TEXT,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_drain_intent_id_v2',
        requested_drain_intent_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_product_operation_id_v2',
        requested_product_operation_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_tenant_id_v2',
        requested_tenant_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_installation_id_v2',
        requested_installation_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_deployment_id_v2',
        requested_deployment_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_expected_revision_v2',
        requested_expected_revision::TEXT,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_marked_at_v2',
        fence_row.pending_marked_at::TEXT,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_source_intent_revision_v2',
        requested_source_intent_revision::TEXT,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_source_state_digest_v2',
        requested_source_state_digest,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_result_intent_revision_v2',
        requested_result_intent_revision::TEXT,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_result_state_digest_v2',
        requested_result_state_digest,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_terminal_kind_v2',
        requested_terminal_kind,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_terminal_microseconds_v2',
        terminal_microseconds::BIGINT::TEXT,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_slot_writer_fence_successor_epoch_v2',
        successor_epoch::TEXT,
        TRUE
    );

    UPDATE public.runtime_slot_writer_fences_v2 AS fence
    SET writer_epoch = successor_epoch,
        pending_drain_intent_id = NULL,
        pending_product_operation_id = NULL,
        pending_tenant_id = NULL,
        pending_installation_id = NULL,
        pending_deployment_id = NULL,
        pending_expected_revision = NULL,
        pending_marked_at = NULL,
        updated_at = requested_terminal_time
    WHERE fence.slot_guild_id = requested_slot_guild_id
        AND fence.slot_ruleset_key = requested_slot_ruleset_key
        AND fence.writer_epoch = requested_source_epoch
        AND fence.pending_drain_intent_id =
            requested_drain_intent_id
        AND fence.pending_product_operation_id =
            requested_product_operation_id
    RETURNING fence.writer_epoch INTO successor_epoch;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_slot_writer_fence_terminal_release_source_stale';
    END IF;

    FOREACH setting_name IN ARRAY ARRAY[
        'starring.runtime_slot_writer_fence_action_v2',
        'starring.runtime_slot_writer_fence_slot_guild_id_v2',
        'starring.runtime_slot_writer_fence_slot_ruleset_key_v2',
        'starring.runtime_slot_writer_fence_expected_epoch_v2',
        'starring.runtime_slot_writer_fence_drain_intent_id_v2',
        'starring.runtime_slot_writer_fence_product_operation_id_v2',
        'starring.runtime_slot_writer_fence_tenant_id_v2',
        'starring.runtime_slot_writer_fence_installation_id_v2',
        'starring.runtime_slot_writer_fence_deployment_id_v2',
        'starring.runtime_slot_writer_fence_expected_revision_v2',
        'starring.runtime_slot_writer_fence_marked_at_v2',
        'starring.runtime_slot_writer_fence_source_intent_revision_v2',
        'starring.runtime_slot_writer_fence_source_state_digest_v2',
        'starring.runtime_slot_writer_fence_result_intent_revision_v2',
        'starring.runtime_slot_writer_fence_result_state_digest_v2',
        'starring.runtime_slot_writer_fence_terminal_kind_v2',
        'starring.runtime_slot_writer_fence_terminal_microseconds_v2',
        'starring.runtime_slot_writer_fence_successor_epoch_v2'
    ]
    LOOP
        IF COALESCE(
            pg_catalog.current_setting(setting_name, TRUE),
            ''
        ) <> ''
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_slot_writer_fence_terminal_release_gate_invalid';
        END IF;
    END LOOP;
    RETURN successor_epoch;
END;
$function$;

DO $patch_slot_mutation_guard$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.reject_runtime_slot_writer_fence_mutation_v2()'
    );

    previous_fragment :=
        '    gate_marked_at TEXT;' || E'\n' ||
        '    setting_name TEXT;';
    next_fragment :=
        '    gate_marked_at TEXT;' || E'\n' ||
        '    gate_source_intent_revision TEXT;' || E'\n' ||
        '    gate_source_state_digest TEXT;' || E'\n' ||
        '    gate_result_intent_revision TEXT;' || E'\n' ||
        '    gate_result_state_digest TEXT;' || E'\n' ||
        '    gate_terminal_kind TEXT;' || E'\n' ||
        '    gate_terminal_microseconds TEXT;' || E'\n' ||
        '    gate_successor_epoch TEXT;' || E'\n' ||
        '    terminal_drain_count BIGINT;' || E'\n' ||
        '    setting_name TEXT;';
    IF definition IS NULL
        OR pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_slot_terminal_release_declaration_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    gate_marked_at := pg_catalog.current_setting(' || E'\n' ||
        '        ''starring.runtime_slot_writer_fence_marked_at_v2'',' || E'\n' ||
        '        TRUE' || E'\n' ||
        '    );';
    next_fragment := previous_fragment || E'\n' ||
        '    gate_source_intent_revision := pg_catalog.current_setting(' || E'\n' ||
        '        ''starring.runtime_slot_writer_fence_source_intent_revision_v2'',' || E'\n' ||
        '        TRUE' || E'\n' ||
        '    );' || E'\n' ||
        '    gate_source_state_digest := pg_catalog.current_setting(' || E'\n' ||
        '        ''starring.runtime_slot_writer_fence_source_state_digest_v2'',' || E'\n' ||
        '        TRUE' || E'\n' ||
        '    );' || E'\n' ||
        '    gate_result_intent_revision := pg_catalog.current_setting(' || E'\n' ||
        '        ''starring.runtime_slot_writer_fence_result_intent_revision_v2'',' || E'\n' ||
        '        TRUE' || E'\n' ||
        '    );' || E'\n' ||
        '    gate_result_state_digest := pg_catalog.current_setting(' || E'\n' ||
        '        ''starring.runtime_slot_writer_fence_result_state_digest_v2'',' || E'\n' ||
        '        TRUE' || E'\n' ||
        '    );' || E'\n' ||
        '    gate_terminal_kind := pg_catalog.current_setting(' || E'\n' ||
        '        ''starring.runtime_slot_writer_fence_terminal_kind_v2'',' || E'\n' ||
        '        TRUE' || E'\n' ||
        '    );' || E'\n' ||
        '    gate_terminal_microseconds := pg_catalog.current_setting(' || E'\n' ||
        '        ''starring.runtime_slot_writer_fence_terminal_microseconds_v2'',' || E'\n' ||
        '        TRUE' || E'\n' ||
        '    );' || E'\n' ||
        '    gate_successor_epoch := pg_catalog.current_setting(' || E'\n' ||
        '        ''starring.runtime_slot_writer_fence_successor_epoch_v2'',' || E'\n' ||
        '        TRUE' || E'\n' ||
        '    );';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_slot_terminal_release_setting_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    RAISE EXCEPTION USING' || E'\n' ||
        '        ERRCODE = ''23514'',' || E'\n' ||
        '        MESSAGE = ''runtime_slot_writer_fence_mutation_rejected'';';
    next_fragment :=
        '    IF TG_OP = ''UPDATE''' || E'\n' ||
        '        AND gate_action = ''terminal_release''' || E'\n' ||
        '        AND gate_slot_guild_id = OLD.slot_guild_id' || E'\n' ||
        '        AND gate_slot_ruleset_key = OLD.slot_ruleset_key' || E'\n' ||
        '        AND gate_expected_epoch = OLD.writer_epoch::TEXT' || E'\n' ||
        '        AND gate_drain_intent_id = OLD.pending_drain_intent_id' || E'\n' ||
        '        AND gate_product_operation_id =' || E'\n' ||
        '            OLD.pending_product_operation_id' || E'\n' ||
        '        AND gate_tenant_id = OLD.pending_tenant_id' || E'\n' ||
        '        AND gate_installation_id = OLD.pending_installation_id' || E'\n' ||
        '        AND gate_deployment_id = OLD.pending_deployment_id' || E'\n' ||
        '        AND gate_expected_revision =' || E'\n' ||
        '            OLD.pending_expected_revision::TEXT' || E'\n' ||
        '        AND gate_marked_at = OLD.pending_marked_at::TEXT' || E'\n' ||
        '        AND gate_source_intent_revision' || E'\n' ||
        '            ~ ''^[1-9][0-9]{0,18}$''' || E'\n' ||
        '        AND gate_source_state_digest ~ ''^[0-9a-f]{64}$''' || E'\n' ||
        '        AND gate_result_intent_revision' || E'\n' ||
        '            ~ ''^[1-9][0-9]{0,18}$''' || E'\n' ||
        '        AND gate_result_state_digest ~ ''^[0-9a-f]{64}$''' || E'\n' ||
        '        AND gate_terminal_kind IN (''consumed'', ''cancelled'')' || E'\n' ||
        '        AND gate_terminal_microseconds' || E'\n' ||
        '            ~ ''^-?(0|[1-9][0-9]{0,18})$''' || E'\n' ||
        '        AND gate_successor_epoch = NEW.writer_epoch::TEXT' || E'\n' ||
        '        AND NEW.slot_guild_id = OLD.slot_guild_id' || E'\n' ||
        '        AND NEW.slot_ruleset_key = OLD.slot_ruleset_key' || E'\n' ||
        '        AND OLD.writer_epoch BETWEEN 1 AND 9223372036854775806' || E'\n' ||
        '        AND NEW.writer_epoch = OLD.writer_epoch + 1' || E'\n' ||
        '        AND NEW.pending_drain_intent_id IS NULL' || E'\n' ||
        '        AND NEW.pending_product_operation_id IS NULL' || E'\n' ||
        '        AND NEW.pending_tenant_id IS NULL' || E'\n' ||
        '        AND NEW.pending_installation_id IS NULL' || E'\n' ||
        '        AND NEW.pending_deployment_id IS NULL' || E'\n' ||
        '        AND NEW.pending_expected_revision IS NULL' || E'\n' ||
        '        AND NEW.pending_marked_at IS NULL' || E'\n' ||
        '        AND NEW.updated_at >= OLD.updated_at' || E'\n' ||
        '        AND (' || E'\n' ||
        '            EXTRACT(EPOCH FROM NEW.updated_at) * 1000000' || E'\n' ||
        '        )::BIGINT::TEXT = gate_terminal_microseconds' || E'\n' ||
        '    THEN' || E'\n' ||
        '        SELECT pg_catalog.count(*)' || E'\n' ||
        '        INTO terminal_drain_count' || E'\n' ||
        '        FROM public.runtime_drain_intents_v2 AS drain' || E'\n' ||
        '        WHERE drain.drain_intent_id = gate_drain_intent_id' || E'\n' ||
        '            AND drain.product_operation_id =' || E'\n' ||
        '                gate_product_operation_id' || E'\n' ||
        '            AND drain.tenant_id = gate_tenant_id' || E'\n' ||
        '            AND drain.installation_id = gate_installation_id' || E'\n' ||
        '            AND drain.deployment_id = gate_deployment_id' || E'\n' ||
        '            AND drain.slot_guild_id = gate_slot_guild_id' || E'\n' ||
        '            AND drain.slot_ruleset_key = gate_slot_ruleset_key' || E'\n' ||
        '            AND drain.expected_revision::TEXT =' || E'\n' ||
        '                gate_expected_revision' || E'\n' ||
        '            AND drain.intent_revision::TEXT =' || E'\n' ||
        '                gate_result_intent_revision' || E'\n' ||
        '            AND drain.canonical_state_digest =' || E'\n' ||
        '                gate_result_state_digest' || E'\n' ||
        '            AND drain.intent_state = gate_terminal_kind' || E'\n' ||
        '            AND starring_runtime_private_v2.' || E'\n' ||
        'starring_runtime_pending_drain_state_exact_v2(drain)' || E'\n' ||
        '            AND CASE gate_terminal_kind' || E'\n' ||
        '                WHEN ''consumed'' THEN (' || E'\n' ||
        '                    pg_catalog.convert_from(' || E'\n' ||
        '                        drain.canonical_state_bytes,' || E'\n' ||
        '                        ''UTF8''' || E'\n' ||
        '                    )::JSONB' || E'\n' ||
        '                ) #>> ''{state,consumed_at_unix_microseconds}''' || E'\n' ||
        '                ELSE (' || E'\n' ||
        '                    pg_catalog.convert_from(' || E'\n' ||
        '                        drain.canonical_state_bytes,' || E'\n' ||
        '                        ''UTF8''' || E'\n' ||
        '                    )::JSONB' || E'\n' ||
        '                ) #>> ''{state,cancelled_at_unix_microseconds}''' || E'\n' ||
        '            END = gate_terminal_microseconds;' || E'\n' ||
        '        IF terminal_drain_count = 1 THEN' || E'\n' ||
        '            FOREACH setting_name IN ARRAY ARRAY[' || E'\n' ||
        '                ''starring.runtime_slot_writer_fence_action_v2'',' || E'\n' ||
        '                ''starring.runtime_slot_writer_fence_slot_guild_id_v2'',' || E'\n' ||
        '                ''starring.runtime_slot_writer_fence_slot_ruleset_key_v2'',' || E'\n' ||
        '                ''starring.runtime_slot_writer_fence_expected_epoch_v2'',' || E'\n' ||
        '                ''starring.runtime_slot_writer_fence_drain_intent_id_v2'',' || E'\n' ||
        '                ''starring.runtime_slot_writer_fence_product_operation_id_v2'',' || E'\n' ||
        '                ''starring.runtime_slot_writer_fence_tenant_id_v2'',' || E'\n' ||
        '                ''starring.runtime_slot_writer_fence_installation_id_v2'',' || E'\n' ||
        '                ''starring.runtime_slot_writer_fence_deployment_id_v2'',' || E'\n' ||
        '                ''starring.runtime_slot_writer_fence_expected_revision_v2'',' || E'\n' ||
        '                ''starring.runtime_slot_writer_fence_marked_at_v2'',' || E'\n' ||
        '                ''starring.runtime_slot_writer_fence_source_intent_revision_v2'',' || E'\n' ||
        '                ''starring.runtime_slot_writer_fence_source_state_digest_v2'',' || E'\n' ||
        '                ''starring.runtime_slot_writer_fence_result_intent_revision_v2'',' || E'\n' ||
        '                ''starring.runtime_slot_writer_fence_result_state_digest_v2'',' || E'\n' ||
        '                ''starring.runtime_slot_writer_fence_terminal_kind_v2'',' || E'\n' ||
        '                ''starring.runtime_slot_writer_fence_terminal_microseconds_v2'',' || E'\n' ||
        '                ''starring.runtime_slot_writer_fence_successor_epoch_v2''' || E'\n' ||
        '            ]' || E'\n' ||
        '            LOOP' || E'\n' ||
        '                PERFORM pg_catalog.set_config(setting_name, '''', TRUE);' || E'\n' ||
        '            END LOOP;' || E'\n' ||
        '            RETURN NEW;' || E'\n' ||
        '        END IF;' || E'\n' ||
        '    END IF;' || E'\n' ||
        E'\n' ||
        previous_fragment;
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_slot_terminal_release_branch_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$patch_slot_mutation_guard$;

DO $patch_slot_symmetry$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.validate_runtime_slot_writer_fence_symmetry_v2()'
    );

    previous_fragment :=
        '    IF TG_OP <> ''INSERT''' || E'\n' ||
        '        AND NOT EXISTS (';
    next_fragment :=
        '    IF TG_OP <> ''DELETE''' || E'\n' ||
        '        AND NEW.intent_state IN (''consumed'', ''cancelled'')' || E'\n' ||
        '    THEN' || E'\n' ||
        '        IF NOT starring_runtime_private_v2.' || E'\n' ||
        'starring_runtime_pending_drain_state_exact_v2(NEW)' || E'\n' ||
        '            OR EXISTS (' || E'\n' ||
        '                SELECT 1' || E'\n' ||
        '                FROM public.runtime_slot_writer_fences_v2 AS fence' || E'\n' ||
        '                WHERE fence.pending_drain_intent_id =' || E'\n' ||
        '                    NEW.drain_intent_id' || E'\n' ||
        '            )' || E'\n' ||
        '            OR (' || E'\n' ||
        '                SELECT pg_catalog.count(*)' || E'\n' ||
        '                FROM public.runtime_product_drain_terminal_actions_v2 AS action' || E'\n' ||
        '                INNER JOIN public.runtime_product_operations_v2 AS product' || E'\n' ||
        '                    ON product.product_operation_id =' || E'\n' ||
        '                        action.product_operation_id' || E'\n' ||
        '                WHERE action.drain_intent_id = NEW.drain_intent_id' || E'\n' ||
        '                    AND action.product_operation_id =' || E'\n' ||
        '                        NEW.product_operation_id' || E'\n' ||
        '                    AND starring_runtime_private_v2.' || E'\n' ||
        'starring_runtime_product_drain_terminal_action_exact_v2(' || E'\n' ||
        '                        action,' || E'\n' ||
        '                        product,' || E'\n' ||
        '                        NEW' || E'\n' ||
        '                    )' || E'\n' ||
        '            ) <> 1' || E'\n' ||
        '        THEN' || E'\n' ||
        '            RAISE EXCEPTION USING' || E'\n' ||
        '                ERRCODE = ''23514'',' || E'\n' ||
        '                MESSAGE = ''runtime_slot_writer_fence_symmetry_invalid'';' || E'\n' ||
        '        END IF;' || E'\n' ||
        '    END IF;' || E'\n' ||
        E'\n' ||
        previous_fragment;
    IF definition IS NULL
        OR pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_terminal_symmetry_patch_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$patch_slot_symmetry$;

DO $patch_startup_terminal_readers$
DECLARE
    identity TEXT;
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    FOREACH identity IN ARRAY ARRAY[
        'public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)',
        'public.starring_runtime_startup_recovery_execute_stale_live_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)'
    ]
    LOOP
        SELECT pg_catalog.pg_get_functiondef(function_row.oid)
        INTO definition
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid =
            pg_catalog.to_regprocedure(identity);

        previous_fragment :=
            '        ) = ''CHECK (intent_state = ANY (ARRAY[''''pending''''::text, ''''route_absent_acknowledged''''::text]))'';';
        next_fragment :=
            '        ) = ''CHECK (intent_state = ANY (ARRAY[''''pending''''::text, ''''route_absent_acknowledged''''::text, ''''consumed''''::text, ''''cancelled''''::text]))'';';
        IF definition IS NULL
            OR pg_catalog.strpos(definition, previous_fragment) = 0
            OR pg_catalog.strpos(
                pg_catalog.replace(definition, previous_fragment, ''),
                previous_fragment
            ) <> 0
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_product_drain_terminal_reader_constraint_drift';
        END IF;
        definition := pg_catalog.replace(
            definition,
            previous_fragment,
            next_fragment
        );

        previous_fragment :=
            '            WHERE drain.intent_state NOT IN (' || E'\n' ||
            '                    ''pending'',' || E'\n' ||
            '                    ''route_absent_acknowledged''' || E'\n' ||
            '                )' || E'\n' ||
            '                OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(' || E'\n' ||
            '                    drain' || E'\n' ||
            '                )';
        next_fragment :=
            '            WHERE drain.intent_state NOT IN (' || E'\n' ||
            '                    ''pending'',' || E'\n' ||
            '                    ''route_absent_acknowledged'',' || E'\n' ||
            '                    ''consumed'',' || E'\n' ||
            '                    ''cancelled''' || E'\n' ||
            '                )' || E'\n' ||
            '                OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(' || E'\n' ||
            '                    drain' || E'\n' ||
            '                )' || E'\n' ||
            '                OR (' || E'\n' ||
            '                    drain.intent_state IN (' || E'\n' ||
            '                        ''consumed'',' || E'\n' ||
            '                        ''cancelled''' || E'\n' ||
            '                    )' || E'\n' ||
            '                    AND NOT EXISTS (' || E'\n' ||
            '                        SELECT 1' || E'\n' ||
            '                        FROM public.runtime_product_drain_terminal_actions_v2 AS action' || E'\n' ||
            '                        INNER JOIN public.runtime_product_operations_v2 AS product' || E'\n' ||
            '                            ON product.product_operation_id =' || E'\n' ||
            '                                action.product_operation_id' || E'\n' ||
            '                        WHERE action.drain_intent_id =' || E'\n' ||
            '                                drain.drain_intent_id' || E'\n' ||
            '                            AND starring_runtime_private_v2.' || E'\n' ||
            'starring_runtime_product_drain_terminal_action_exact_v2(' || E'\n' ||
            '                                action,' || E'\n' ||
            '                                product,' || E'\n' ||
            '                                drain' || E'\n' ||
            '                            )' || E'\n' ||
            '                    )' || E'\n' ||
            '                )';
        IF pg_catalog.strpos(definition, previous_fragment) = 0
            OR pg_catalog.strpos(
                pg_catalog.replace(definition, previous_fragment, ''),
                previous_fragment
            ) <> 0
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_product_drain_terminal_reader_classifier_drift';
        END IF;
        EXECUTE pg_catalog.replace(
            definition,
            previous_fragment,
            next_fragment
        );
    END LOOP;
END;
$patch_startup_terminal_readers$;

DO $patch_pending_candidate_reader$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'starring_runtime_private_v2.starring_runtime_pending_drain_candidate_v2()'
    );

    previous_fragment :=
        '    FROM public.runtime_drain_intents_v2 AS drain' || E'\n' ||
        '    WHERE drain.intent_state IN (' || E'\n' ||
        '            ''pending'',' || E'\n' ||
        '            ''route_absent_acknowledged''' || E'\n' ||
        '        )' || E'\n' ||
        '        AND NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(' || E'\n' ||
        '            drain' || E'\n' ||
        '        );';
    next_fragment :=
        '    FROM public.runtime_drain_intents_v2 AS drain' || E'\n' ||
        '    WHERE NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(' || E'\n' ||
        '            drain' || E'\n' ||
        '        )' || E'\n' ||
        '        OR (' || E'\n' ||
        '            drain.intent_state IN (''consumed'', ''cancelled'')' || E'\n' ||
        '            AND NOT EXISTS (' || E'\n' ||
        '                SELECT 1' || E'\n' ||
        '                FROM public.runtime_product_drain_terminal_actions_v2 AS action' || E'\n' ||
        '                INNER JOIN public.runtime_product_operations_v2 AS product' || E'\n' ||
        '                    ON product.product_operation_id =' || E'\n' ||
        '                        action.product_operation_id' || E'\n' ||
        '                WHERE action.drain_intent_id = drain.drain_intent_id' || E'\n' ||
        '                    AND starring_runtime_private_v2.' || E'\n' ||
        'starring_runtime_product_drain_terminal_action_exact_v2(' || E'\n' ||
        '                        action,' || E'\n' ||
        '                        product,' || E'\n' ||
        '                        drain' || E'\n' ||
        '                    )' || E'\n' ||
        '            )' || E'\n' ||
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
            MESSAGE = 'runtime_product_drain_terminal_candidate_patch_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$patch_pending_candidate_reader$;

DO $patch_product_observation_reader$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_product_drain_observe_v2(text,text,text,bigint,text,text)'
    );

    previous_fragment :=
        '        drain_tenant_id := drain_row.tenant_id;';
    next_fragment :=
        '        IF NOT starring_runtime_private_v2.' || E'\n' ||
        'starring_runtime_pending_drain_state_exact_v2(drain_row)' || E'\n' ||
        '            OR (' || E'\n' ||
        '                drain_row.intent_state IN (' || E'\n' ||
        '                    ''consumed'',' || E'\n' ||
        '                    ''cancelled''' || E'\n' ||
        '                )' || E'\n' ||
        '                AND NOT EXISTS (' || E'\n' ||
        '                    SELECT 1' || E'\n' ||
        '                    FROM public.runtime_product_drain_terminal_actions_v2 AS action' || E'\n' ||
        '                    INNER JOIN public.runtime_product_operations_v2 AS product' || E'\n' ||
        '                        ON product.product_operation_id =' || E'\n' ||
        '                            action.product_operation_id' || E'\n' ||
        '                    WHERE action.drain_intent_id =' || E'\n' ||
        '                            drain_row.drain_intent_id' || E'\n' ||
        '                        AND starring_runtime_private_v2.' || E'\n' ||
        'starring_runtime_product_drain_terminal_action_exact_v2(' || E'\n' ||
        '                            action,' || E'\n' ||
        '                            product,' || E'\n' ||
        '                            drain_row' || E'\n' ||
        '                        )' || E'\n' ||
        '                )' || E'\n' ||
        '            )' || E'\n' ||
        '        THEN' || E'\n' ||
        '            outcome_name := ''persistence_corrupt'';' || E'\n' ||
        '            RETURN NEXT;' || E'\n' ||
        '            RETURN;' || E'\n' ||
        '        END IF;' || E'\n' ||
        E'\n' ||
        previous_fragment;
    IF definition IS NULL
        OR pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_terminal_observation_patch_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$patch_product_observation_reader$;

REVOKE ALL PRIVILEGES
ON TABLE public.runtime_product_drain_terminal_actions_v2
FROM PUBLIC;

REVOKE ALL PRIVILEGES ON FUNCTION
    starring_runtime_private_v2.starring_runtime_pending_drain_consumed_state_v2(
        public.runtime_drain_intents_v2,
        BIGINT,
        TIMESTAMPTZ
    ),
    starring_runtime_private_v2.starring_runtime_pending_drain_cancelled_state_v2(
        public.runtime_drain_intents_v2,
        TIMESTAMPTZ
    ),
    starring_runtime_private_v2.starring_runtime_product_drain_terminal_projection_v2(
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        BIGINT,
        BYTEA,
        TEXT,
        BIGINT,
        BIGINT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        BIGINT,
        BIGINT,
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        TIMESTAMPTZ
    ),
    starring_runtime_private_v2.starring_runtime_product_drain_terminal_action_exact_v2(
        public.runtime_product_drain_terminal_actions_v2,
        public.runtime_product_operations_v2,
        public.runtime_drain_intents_v2
    ),
    starring_runtime_private_v2.starring_runtime_product_drain_terminal_transition_v2(
        TEXT,
        BIGINT,
        TEXT,
        TEXT,
        BIGINT,
        TIMESTAMPTZ
    ),
    starring_runtime_private_v2.starring_runtime_slot_writer_fence_terminal_release_v2(
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        BIGINT,
        BYTEA,
        TEXT,
        BIGINT,
        TEXT,
        TEXT,
        TIMESTAMPTZ
    ),
    starring_runtime_private_v2.reject_runtime_product_drain_terminal_action_mutation_v2()
FROM PUBLIC;

DO $patch_execution_manifest$
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
        '            (pg_catalog.to_regclass(''public.runtime_drain_intents_v2'')),';
    next_fragment := previous_fragment || E'\n' ||
        '            (pg_catalog.to_regclass(''public.runtime_product_drain_terminal_actions_v2'')),';
    IF definition IS NULL
        OR pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_terminal_manifest_relation_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(public.runtime_drain_intents_v2)''' || E'\n' ||
        '        )';
    next_fragment := previous_fragment;
    FOREACH identity IN ARRAY ARRAY[
        'starring_runtime_private_v2.starring_runtime_pending_drain_consumed_state_v2(public.runtime_drain_intents_v2,bigint,timestamp with time zone)',
        'starring_runtime_private_v2.starring_runtime_pending_drain_cancelled_state_v2(public.runtime_drain_intents_v2,timestamp with time zone)',
        'starring_runtime_private_v2.starring_runtime_product_drain_terminal_projection_v2(text,text,text,text,text,text,text,text,text,bigint,text,bigint,bytea,text,bigint,bigint,text,text,bigint,text,bigint,bigint,text,text,text,bigint,timestamp with time zone)',
        'starring_runtime_private_v2.starring_runtime_product_drain_terminal_action_exact_v2(public.runtime_product_drain_terminal_actions_v2,public.runtime_product_operations_v2,public.runtime_drain_intents_v2)',
        'starring_runtime_private_v2.starring_runtime_product_drain_terminal_transition_v2(text,bigint,text,text,bigint,timestamp with time zone)',
        'starring_runtime_private_v2.starring_runtime_slot_writer_fence_terminal_release_v2(text,text,bigint,text,text,text,text,text,bigint,bigint,bytea,text,bigint,text,text,timestamp with time zone)'
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
            MESSAGE = 'runtime_product_drain_terminal_manifest_function_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    RETURN observed_count = 834' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''90d1ab7064fa288e01b09e81815265d82409ceac50267412ff952f63a6c285a3'';';
    next_fragment :=
        '    RETURN observed_count = 888' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''aacb4889c005088a91b93ee948502397aa8747275087a4e2a600d2d49a9b8181'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_terminal_manifest_expectation_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$patch_execution_manifest$;

DO $patch_execution_readiness$
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
        '            (''public.runtime_drain_intents_v2''),';
    next_fragment := previous_fragment || E'\n' ||
        '            (''public.runtime_product_drain_terminal_actions_v2''),';
    IF definition IS NULL
        OR pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_terminal_readiness_relation_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            (''starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(public.runtime_drain_intents_v2)''),';
    next_fragment := previous_fragment || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_pending_drain_consumed_state_v2(public.runtime_drain_intents_v2,bigint,timestamp with time zone)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_pending_drain_cancelled_state_v2(public.runtime_drain_intents_v2,timestamp with time zone)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_product_drain_terminal_projection_v2(text,text,text,text,text,text,text,text,text,bigint,text,bigint,bytea,text,bigint,bigint,text,text,bigint,text,bigint,bigint,text,text,text,bigint,timestamp with time zone)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_product_drain_terminal_action_exact_v2(public.runtime_product_drain_terminal_actions_v2,public.runtime_product_operations_v2,public.runtime_drain_intents_v2)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_product_drain_terminal_transition_v2(text,bigint,text,text,bigint,timestamp with time zone)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_slot_writer_fence_terminal_release_v2(text,text,bigint,text,text,text,text,text,bigint,bigint,bytea,text,bigint,text,text,timestamp with time zone)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.reject_runtime_product_drain_terminal_action_mutation_v2()''),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_terminal_readiness_protected_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            (''starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text,bytea,text,bytea,text)''),';
    next_fragment := previous_fragment || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_pending_drain_consumed_state_v2(public.runtime_drain_intents_v2,bigint,timestamp with time zone)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_pending_drain_cancelled_state_v2(public.runtime_drain_intents_v2,timestamp with time zone)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_product_drain_terminal_projection_v2(text,text,text,text,text,text,text,text,text,bigint,text,bigint,bytea,text,bigint,bigint,text,text,bigint,text,bigint,bigint,text,text,text,bigint,timestamp with time zone)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_product_drain_terminal_action_exact_v2(public.runtime_product_drain_terminal_actions_v2,public.runtime_product_operations_v2,public.runtime_drain_intents_v2)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_product_drain_terminal_transition_v2(text,bigint,text,text,bigint,timestamp with time zone)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_slot_writer_fence_terminal_release_v2(text,text,bigint,text,text,text,text,text,bigint,bigint,bytea,text,bigint,text,text,timestamp with time zone)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.reject_runtime_product_drain_terminal_action_mutation_v2()''),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_terminal_readiness_private_acl_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '''8f62326b250fba74273b2dbbf33066ef7f1353e9a6f3f464c059b1678bb714d4''::TEXT';
    next_fragment :=
        '''0e40c195026bf46ce6a8e5e70472d108de5deb533d1f072cf056e171c7078fe7''::TEXT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_product_drain_terminal_readiness_manifest_digest_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$patch_execution_readiness$;

DO $postflight$
DECLARE
    common_owner OID;
    private_schema_owner OID;
    invalid_definition_count BIGINT;
    invalid_function_acl_count BIGINT;
    invalid_relation_acl_count BIGINT;
    invalid_column_acl_count BIGINT;
    invalid_trigger_count BIGINT;
    invalid_index_count BIGINT;
    invalid_constraint_count BIGINT;
    invalid_public_capability_count BIGINT;
    journal_column_contract TEXT;
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

    SELECT pg_catalog.count(*)
    INTO invalid_definition_count
    FROM (
        VALUES
            (
                'starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(public.runtime_drain_intents_v2)',
                '322925d4cd07411adc51058f3a6bd21d975d96989c1e2085aa405bf7134bc0e7'
            ),
            (
                'starring_runtime_private_v2.starring_runtime_pending_drain_consumed_state_v2(public.runtime_drain_intents_v2,bigint,timestamp with time zone)',
                'a8476e5ee24cf70f37a5459909a291529628d24c8a6b2c49f5d0953209619d7a'
            ),
            (
                'starring_runtime_private_v2.starring_runtime_pending_drain_cancelled_state_v2(public.runtime_drain_intents_v2,timestamp with time zone)',
                'fd1dd0458dab4365d25556446425209ea963426d31c23b4693b2d8f49708479f'
            ),
            (
                'starring_runtime_private_v2.starring_runtime_product_drain_terminal_projection_v2(text,text,text,text,text,text,text,text,text,bigint,text,bigint,bytea,text,bigint,bigint,text,text,bigint,text,bigint,bigint,text,text,text,bigint,timestamp with time zone)',
                '8a325da2df438e87470a5a831ac67e82f9b5329660bc3f211bbeac0453feeed3'
            ),
            (
                'starring_runtime_private_v2.starring_runtime_product_drain_terminal_action_exact_v2(public.runtime_product_drain_terminal_actions_v2,public.runtime_product_operations_v2,public.runtime_drain_intents_v2)',
                '06f1eaa9f576e21b5f1a4b6c9a0ddfb24695d5e3bd4e482399e9954e5a854ffa'
            ),
            (
                'starring_runtime_private_v2.starring_runtime_product_drain_terminal_transition_v2(text,bigint,text,text,bigint,timestamp with time zone)',
                '632c89ccf3227ca401c2b46089f8f05e1bd157b249c41a6042705e5efe20d91c'
            ),
            (
                'starring_runtime_private_v2.starring_runtime_slot_writer_fence_terminal_release_v2(text,text,bigint,text,text,text,text,text,bigint,bigint,bytea,text,bigint,text,text,timestamp with time zone)',
                '150373deba2cc1fec96626ecfb71fd5050ea96bf565b467d48b709f597855132'
            ),
            (
                'starring_runtime_private_v2.reject_runtime_product_drain_terminal_action_mutation_v2()',
                'aeb22915c3c90d961965c806a502e047957b1d165f33462791b86163778772e2'
            ),
            (
                'public.reject_runtime_product_drain_mutation()',
                '31d7a8e9ef374ba3d0d43c2d9fa5380862769ad21f72f7b1ac91804cf210100e'
            ),
            (
                'public.reject_runtime_slot_writer_fence_mutation_v2()',
                '1ecb1519a59a8a7ef85e9cabb34e67ae72926287efef41e2a270fdc7fd9aadf5'
            ),
            (
                'public.validate_runtime_slot_writer_fence_symmetry_v2()',
                'f109bcc5f6d86f920d99c6bf6debfd2864d9c158ed67af6f6e54c66e398b2ba1'
            ),
            (
                'starring_runtime_private_v2.starring_runtime_pending_drain_candidate_v2()',
                '91d4d64ae0f1b3053ec91f1c1b07164fce08311e26b58718eca672f3fadee909'
            ),
            (
                'public.starring_runtime_product_drain_observe_v2(text,text,text,bigint,text,text)',
                '3dbdfd3f1af8a577246baf57cfd4a62bc01b5bc0b49e137b325658c98f8d23a9'
            ),
            (
                'public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)',
                'ca46333d2a3ed1168a3c3c6dab063e61009f6a2ecba48b2d62a55fe33c0ac0e4'
            ),
            (
                'public.starring_runtime_startup_recovery_execute_stale_live_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)',
                '11aeeae9eb23564951a87c947439c0a6f87c5dca1b506a1cb9b5e0f4f9c0c936'
            ),
            (
                'public.starring_runtime_execution_schema_manifest_v1()',
                '0e40c195026bf46ce6a8e5e70472d108de5deb533d1f072cf056e171c7078fe7'
            ),
            (
                'public.starring_runtime_execution_database_readiness_v1()',
                'a3674e7c69f24ce212ddf0598d23f448a47f0b6e7766dee20a78399d5b6477e7'
            )
    ) AS expected(identity, digest)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid =
            pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(function_row.oid),
                'UTF8'
            )),
            'hex'
        ) IS DISTINCT FROM expected.digest;

    SELECT pg_catalog.count(*)
    INTO invalid_function_acl_count
    FROM (
        VALUES
            ('starring_runtime_private_v2.starring_runtime_pending_drain_consumed_state_v2(public.runtime_drain_intents_v2,bigint,timestamp with time zone)'),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_cancelled_state_v2(public.runtime_drain_intents_v2,timestamp with time zone)'),
            ('starring_runtime_private_v2.starring_runtime_product_drain_terminal_projection_v2(text,text,text,text,text,text,text,text,text,bigint,text,bigint,bytea,text,bigint,bigint,text,text,bigint,text,bigint,bigint,text,text,text,bigint,timestamp with time zone)'),
            ('starring_runtime_private_v2.starring_runtime_product_drain_terminal_action_exact_v2(public.runtime_product_drain_terminal_actions_v2,public.runtime_product_operations_v2,public.runtime_drain_intents_v2)'),
            ('starring_runtime_private_v2.starring_runtime_product_drain_terminal_transition_v2(text,bigint,text,text,bigint,timestamp with time zone)'),
            ('starring_runtime_private_v2.starring_runtime_slot_writer_fence_terminal_release_v2(text,text,bigint,text,text,text,text,text,bigint,bigint,bytea,text,bigint,text,text,timestamp with time zone)'),
            ('starring_runtime_private_v2.reject_runtime_product_drain_terminal_action_mutation_v2()')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid =
            pg_catalog.to_regprocedure(expected.identity)
    LEFT JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
        ON TRUE
    WHERE function_row.oid IS NULL
        OR privilege.grantee <> common_owner
        OR privilege.grantor <> common_owner
        OR privilege.privilege_type <> 'EXECUTE'
        OR privilege.is_grantable;

    SELECT pg_catalog.count(*)
    INTO invalid_relation_acl_count
    FROM pg_catalog.pg_class AS relation
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        relation.relacl,
        pg_catalog.acldefault('r', relation.relowner)
    )) AS privilege
    WHERE relation.oid = pg_catalog.to_regclass(
            'public.runtime_product_drain_terminal_actions_v2'
        )
        AND (
            privilege.grantee <> common_owner
            OR privilege.grantor <> common_owner
            OR privilege.is_grantable
        );

    SELECT pg_catalog.count(*)
    INTO invalid_column_acl_count
    FROM pg_catalog.pg_attribute AS attribute
    CROSS JOIN LATERAL pg_catalog.aclexplode(attribute.attacl) AS privilege
    WHERE attribute.attrelid = pg_catalog.to_regclass(
            'public.runtime_product_drain_terminal_actions_v2'
        )
        AND privilege.grantee <> common_owner;

    SELECT pg_catalog.count(*)
    INTO invalid_trigger_count
    FROM (
        VALUES
            (
                'runtime_product_drain_terminal_actions_v2_reject_row_mutation',
                27::SMALLINT
            ),
            (
                'runtime_product_drain_terminal_actions_v2_reject_truncate',
                34::SMALLINT
            )
    ) AS expected(name, trigger_type)
    LEFT JOIN pg_catalog.pg_trigger AS trigger_row
        ON trigger_row.tgrelid = pg_catalog.to_regclass(
                'public.runtime_product_drain_terminal_actions_v2'
            )
            AND trigger_row.tgname = expected.name
    WHERE trigger_row.oid IS NULL
        OR trigger_row.tgisinternal
        OR trigger_row.tgenabled <> 'O'
        OR trigger_row.tgtype <> expected.trigger_type
        OR trigger_row.tgfoid <> pg_catalog.to_regprocedure(
            'starring_runtime_private_v2.reject_runtime_product_drain_terminal_action_mutation_v2()'
        );

    SELECT pg_catalog.count(*)
    INTO invalid_index_count
    FROM (
        VALUES
            (
                'runtime_product_drain_terminal_actions_v2_pkey',
                TRUE,
                TRUE
            ),
            (
                'runtime_product_drain_terminal_actions_v2_drain_unique',
                TRUE,
                FALSE
            ),
            (
                'runtime_product_drain_terminal_actions_v2_action_unique',
                TRUE,
                FALSE
            ),
            (
                'runtime_product_drain_terminal_actions_v2_semantic_lookup',
                FALSE,
                FALSE
            )
    ) AS expected(name, unique_index, primary_index)
    LEFT JOIN pg_catalog.pg_class AS index_relation
        ON index_relation.relnamespace =
                pg_catalog.to_regnamespace('public')
            AND index_relation.relname = expected.name
    LEFT JOIN pg_catalog.pg_index AS index_row
        ON index_row.indexrelid = index_relation.oid
    WHERE index_row.indexrelid IS NULL
        OR index_row.indrelid <> pg_catalog.to_regclass(
            'public.runtime_product_drain_terminal_actions_v2'
        )
        OR index_row.indisunique <> expected.unique_index
        OR index_row.indisprimary <> expected.primary_index
        OR NOT index_row.indisvalid
        OR NOT index_row.indisready;

    SELECT pg_catalog.count(*)
    INTO invalid_constraint_count
    FROM (
        VALUES
            ('runtime_product_drain_terminal_actions_v2_pkey'),
            ('runtime_product_drain_terminal_actions_v2_action_unique'),
            ('runtime_product_drain_terminal_actions_v2_drain_unique'),
            ('runtime_product_drain_terminal_actions_v2_product_fk'),
            ('runtime_product_drain_terminal_actions_v2_drain_fk'),
            ('runtime_product_drain_terminal_actions_v2_id_check'),
            ('runtime_product_drain_terminal_actions_v2_kind_check'),
            ('runtime_product_drain_terminal_actions_v2_digest_check'),
            ('runtime_product_drain_terminal_actions_v2_revision_check'),
            ('runtime_product_drain_terminal_actions_v2_epoch_check'),
            ('runtime_product_drain_terminal_actions_v2_time_check'),
            ('runtime_product_drain_terminal_actions_v2_projection_check')
    ) AS expected(name)
    LEFT JOIN pg_catalog.pg_constraint AS constraint_row
        ON constraint_row.conrelid = pg_catalog.to_regclass(
                'public.runtime_product_drain_terminal_actions_v2'
            )
            AND constraint_row.conname = expected.name
    WHERE constraint_row.oid IS NULL
        OR NOT constraint_row.convalidated;

    SELECT pg_catalog.count(*)
    INTO invalid_public_capability_count
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.pronamespace = pg_catalog.to_regnamespace('public')
        AND function_row.proname IN (
            'starring_product_apply_consume_runtime_drain_v2',
            'starring_product_cancel_runtime_drain_v2'
        );

    SELECT pg_catalog.string_agg(
        attribute.attname || ':' ||
            pg_catalog.format_type(
                attribute.atttypid,
                attribute.atttypmod
            ) || ':' ||
            attribute.attnotnull::TEXT,
        ',' ORDER BY attribute.attnum
    )
    INTO journal_column_contract
    FROM pg_catalog.pg_attribute AS attribute
    WHERE attribute.attrelid = pg_catalog.to_regclass(
            'public.runtime_product_drain_terminal_actions_v2'
        )
        AND attribute.attnum > 0
        AND NOT attribute.attisdropped;

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR private_schema_owner IS DISTINCT FROM common_owner
        OR invalid_definition_count <> 0
        OR invalid_function_acl_count <> 0
        OR invalid_relation_acl_count <> 0
        OR invalid_column_acl_count <> 0
        OR invalid_trigger_count <> 0
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_trigger AS trigger_row
            WHERE trigger_row.tgrelid = pg_catalog.to_regclass(
                    'public.runtime_product_drain_terminal_actions_v2'
                )
                AND NOT trigger_row.tgisinternal
        ) <> 2
        OR invalid_index_count <> 0
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_index AS index_row
            WHERE index_row.indrelid = pg_catalog.to_regclass(
                    'public.runtime_product_drain_terminal_actions_v2'
                )
        ) <> 4
        OR invalid_constraint_count <> 0
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_constraint AS constraint_row
            WHERE constraint_row.conrelid = pg_catalog.to_regclass(
                    'public.runtime_product_drain_terminal_actions_v2'
                )
        ) <> 12
        OR invalid_public_capability_count <> 0
        OR journal_column_contract IS DISTINCT FROM
            'terminal_action_id:text:true,terminal_kind:text:true,drain_intent_id:text:true,product_operation_id:text:true,product_mutation_digest:text:true,drain_intent_digest:text:true,product_action_idempotency_digest:text:true,product_action_semantic_request_digest:text:true,cancellation_reason_digest:text:false,source_intent_revision:bigint:true,source_canonical_state_digest:text:true,result_intent_revision:bigint:true,result_canonical_state_digest:text:true,source_deployment_revision:bigint:true,source_result_deployment_revision:bigint:true,source_result_deployment_snapshot_digest:text:true,result_deployment_id:text:false,result_deployment_revision:bigint:false,result_deployment_snapshot_digest:text:false,source_slot_writer_epoch:bigint:true,successor_slot_writer_epoch:bigint:true,terminal_database_time:timestamp with time zone:true,product_receipt_id:text:true,product_audit_event_id:text:true,authority_observation_digest:text:true,installation_authority_revision:bigint:true,terminal_projection_bytes:bytea:true,terminal_projection_digest:text:true'
        OR EXISTS (
            SELECT 1
            FROM public.runtime_product_drain_terminal_actions_v2
        )
        OR (
            SELECT relation.relkind
            FROM pg_catalog.pg_class AS relation
            WHERE relation.oid = pg_catalog.to_regclass(
                'public.runtime_product_drain_terminal_actions_v2'
            )
        ) IS DISTINCT FROM 'r'
        OR (
            SELECT relation.relpersistence
            FROM pg_catalog.pg_class AS relation
            WHERE relation.oid = pg_catalog.to_regclass(
                'public.runtime_product_drain_terminal_actions_v2'
            )
        ) IS DISTINCT FROM 'p'
        OR (
            SELECT relation.relowner
            FROM pg_catalog.pg_class AS relation
            WHERE relation.oid = pg_catalog.to_regclass(
                'public.runtime_product_drain_terminal_actions_v2'
            )
        ) IS DISTINCT FROM common_owner
        OR (
            SELECT relation.relrowsecurity OR relation.relforcerowsecurity
            FROM pg_catalog.pg_class AS relation
            WHERE relation.oid = pg_catalog.to_regclass(
                'public.runtime_product_drain_terminal_actions_v2'
            )
        )
        OR pg_catalog.pg_get_constraintdef(
            (
                SELECT constraint_row.oid
                FROM pg_catalog.pg_constraint AS constraint_row
                WHERE constraint_row.conrelid =
                        'public.runtime_drain_intents_v2'::REGCLASS
                    AND constraint_row.conname =
                        'runtime_drain_intents_v2_state_check'
            ),
            TRUE
        ) IS DISTINCT FROM
            'CHECK (intent_state = ANY (ARRAY[''pending''::text, ''route_absent_acknowledged''::text, ''consumed''::text, ''cancelled''::text]))'
        OR pg_catalog.pg_get_indexdef(
            pg_catalog.to_regclass(
                'public.runtime_drain_intents_v2_one_pending_per_slot'
            )
        ) IS DISTINCT FROM
            'CREATE UNIQUE INDEX runtime_drain_intents_v2_one_pending_per_slot ON public.runtime_drain_intents_v2 USING btree (slot_guild_id, slot_ruleset_key) WHERE (intent_state = ANY (ARRAY[''pending''::text, ''route_absent_acknowledged''::text]))'
        OR NOT public.starring_runtime_exact_target_schema_manifest_v1()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'runtime_product_drain_terminal_substrate_postflight_drift',
            DETAIL = pg_catalog.format(
                'owner=%s schema_owner=%s definitions=%s function_acl=%s relation_acl=%s column_acl=%s triggers=%s indexes=%s constraints=%s public_capabilities=%s',
                common_owner,
                private_schema_owner,
                invalid_definition_count,
                invalid_function_acl_count,
                invalid_relation_acl_count,
                invalid_column_acl_count,
                invalid_trigger_count,
                invalid_index_count,
                invalid_constraint_count,
                invalid_public_capability_count
            );
    END IF;
END;
$postflight$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
