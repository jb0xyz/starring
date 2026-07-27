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
            ('starring_runtime_private_v2.starring_runtime_pending_drain_initial_state_v2(public.runtime_drain_intents_v2)'),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_root_exact_v2(public.runtime_drain_intents_v2)'),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(public.runtime_drain_intents_v2)'),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_key_text_v2(public.runtime_drain_intents_v2)'),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_provenance_text_v2(text)'),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_claim_text_v2(public.runtime_drain_intents_v2,jsonb)'),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_projection_v2(smallint,bytea,bytea,bytea,bytea)'),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_replay_exact_v2(bytea,bytea)'),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_exact_v2(bytea,smallint,bytea,smallint)'),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_product_root_compound_exact_v2(bytea,text,bigint,bigint,smallint,text,bytea)'),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_initialize_v2()'),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_candidate_v2()'),
            ('public.starring_runtime_startup_recovery_select_pending_drain_v2(text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)'),
            ('public.starring_runtime_startup_recovery_record_pending_drain_none_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint)'),
            ('public.starring_runtime_startup_recovery_execute_pending_drain_v2(text,bigint,bigint,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean,text)')
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
            '63268b8e2e30bbe523a437a5c326daa9ef25b863a866d4f1e67fcf46bc98bd95'
        OR readiness_digest IS DISTINCT FROM
            '7526d7365225da6514fcc589d76c316dd1363c40cad30e12e3f752b4c85e8044'
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
            MESSAGE = 'runtime_startup_pending_drain_preflight_drift';
    END IF;
END;
$preflight$;

ALTER TABLE public.runtime_drain_intents_v2
ADD COLUMN canonical_state_bytes BYTEA,
ADD COLUMN canonical_state_digest TEXT;

ALTER TABLE public.runtime_drain_intents_v2
DROP CONSTRAINT runtime_drain_intents_v2_revision_check,
DROP CONSTRAINT runtime_drain_intents_v2_state_check;

ALTER TABLE public.runtime_drain_intents_v2
ADD CONSTRAINT runtime_drain_intents_v2_revision_check CHECK (
    expected_revision BETWEEN 1 AND 9223372036854775807
    AND intent_revision BETWEEN 1 AND 9223372036854775807
),
ADD CONSTRAINT runtime_drain_intents_v2_state_check CHECK (
    intent_state IN ('pending', 'route_absent_acknowledged')
),
ADD CONSTRAINT runtime_drain_intents_v2_canonical_state_check CHECK (
    (
        canonical_state_bytes IS NULL
        AND canonical_state_digest IS NULL
    )
    OR (
        pg_catalog.octet_length(canonical_state_bytes)
            BETWEEN 1 AND 1048576
        AND canonical_state_digest ~ '^[0-9a-f]{64}$'
        AND canonical_state_digest = pg_catalog.encode(
            pg_catalog.sha256(canonical_state_bytes),
            'hex'
        )
    )
);

DROP INDEX public.runtime_drain_intents_v2_one_pending_per_slot;

CREATE UNIQUE INDEX runtime_drain_intents_v2_one_pending_per_slot
ON public.runtime_drain_intents_v2 (
    slot_guild_id,
    slot_ruleset_key
)
WHERE intent_state IN ('pending', 'route_absent_acknowledged');

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_pending_drain_root_exact_v2(
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
    request_text TEXT;
    request_value JSONB;
    key_value JSONB;
    domain_bytes BYTEA;
    computed_digest TEXT;
BEGIN
    IF pg_catalog.octet_length(drain_row.drain_intent_request_bytes)
            NOT BETWEEN 1 AND 65536
        OR drain_row.drain_intent_digest !~ '^[0-9a-f]{64}$'
    THEN
        RETURN FALSE;
    END IF;

    BEGIN
        request_text := pg_catalog.convert_from(
            drain_row.drain_intent_request_bytes,
            'UTF8'
        );
        request_value := request_text::JSONB;
    EXCEPTION
        WHEN OTHERS THEN
            RETURN FALSE;
    END;

    key_value := request_value -> 'key';
    domain_bytes :=
        pg_catalog.convert_to(
            'starring.runtime.drain_intent.v2',
            'UTF8'
        )
        || pg_catalog.decode('00', 'hex');
    computed_digest := pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.int8send(
                pg_catalog.octet_length(domain_bytes)::BIGINT
            )
            || domain_bytes
            || pg_catalog.int8send(
                pg_catalog.octet_length(
                    drain_row.drain_intent_request_bytes
                )::BIGINT
            )
            || drain_row.drain_intent_request_bytes
        ),
        'hex'
    );

    RETURN computed_digest = drain_row.drain_intent_digest
        AND pg_catalog.left(request_text, 26)
            = '{"format_version":2,"key":'
        AND pg_catalog.right(request_text, 1) = '}'
        AND pg_catalog.jsonb_typeof(request_value) = 'object'
        AND (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(request_value)
        ) = 2
        AND request_value ->> 'format_version' = '2'
        AND pg_catalog.jsonb_typeof(key_value) = 'object'
        AND (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(key_value)
        ) = 8
        AND key_value ->> 'intent_id' = drain_row.drain_intent_id
        AND key_value ->> 'product_operation_id'
            = drain_row.product_operation_id
        AND key_value ->> 'product_mutation_digest'
            = drain_row.product_mutation_digest
        AND key_value #>> '{scope,tenant_id}' = drain_row.tenant_id
        AND key_value #>> '{scope,installation_id}'
            = drain_row.installation_id
        AND key_value #>> '{scope,deployment_id}'
            = drain_row.deployment_id
        AND key_value ->> 'expected_revision'
            = drain_row.expected_revision::TEXT
        AND key_value #>> '{slot,guild_id}' = drain_row.slot_guild_id
        AND key_value #>> '{slot,ruleset_key}'
            = drain_row.slot_ruleset_key
        AND key_value #>> '{expected_target,guild_id}'
            = drain_row.slot_guild_id
        AND key_value #>> '{expected_target,ruleset_key}'
            = drain_row.slot_ruleset_key
        AND key_value ->> 'mutation_kind' IN (
            'apply',
            'supersede',
            'cancel',
            'authority_change',
            'teardown'
        );
END;
$function$;


CREATE FUNCTION starring_runtime_private_v2.starring_runtime_pending_drain_initial_state_v2(
    drain_row public.runtime_drain_intents_v2
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
    request_text TEXT;
    key_text TEXT;
BEGIN
    IF drain_row.intent_revision <> 1
        OR drain_row.intent_state <> 'pending'
        OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_root_exact_v2(
            drain_row
        )
    THEN
        RETURN NULL;
    END IF;

    request_text := pg_catalog.convert_from(
        drain_row.drain_intent_request_bytes,
        'UTF8'
    );
    key_text := pg_catalog.substr(
        request_text,
        27,
        pg_catalog.length(request_text) - 27
    );

    RETURN pg_catalog.convert_to(
        pg_catalog.concat(
            '{"format_version":2,"root":{"key":',
            key_text,
            ',"drain_intent_digest":',
            pg_catalog.to_json(drain_row.drain_intent_digest)::TEXT,
            '},"intent_revision":1,"state":{"kind":"pending_unclaimed"}}'
        ),
        'UTF8'
    );
END;
$function$;

UPDATE public.runtime_drain_intents_v2 AS drain
SET canonical_state_bytes =
        starring_runtime_private_v2.starring_runtime_pending_drain_initial_state_v2(
            drain
        ),
    canonical_state_digest = pg_catalog.encode(
        pg_catalog.sha256(
            starring_runtime_private_v2.starring_runtime_pending_drain_initial_state_v2(
                drain
            )
        ),
        'hex'
    );

DO $backfill_exact$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM public.runtime_drain_intents_v2 AS drain
        WHERE drain.canonical_state_bytes IS NULL
            OR drain.canonical_state_digest IS NULL
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_pending_drain_backfill_invalid';
    END IF;
END;
$backfill_exact$;

ALTER TABLE public.runtime_drain_intents_v2
ALTER COLUMN canonical_state_bytes SET NOT NULL,
ALTER COLUMN canonical_state_digest SET NOT NULL,
DROP CONSTRAINT runtime_drain_intents_v2_canonical_state_check,
ADD CONSTRAINT runtime_drain_intents_v2_canonical_state_check CHECK (
    pg_catalog.octet_length(canonical_state_bytes)
        BETWEEN 1 AND 1048576
    AND canonical_state_digest ~ '^[0-9a-f]{64}$'
    AND canonical_state_digest = pg_catalog.encode(
        pg_catalog.sha256(canonical_state_bytes),
        'hex'
    )
);

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
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
    state_text TEXT;
    state_value JSONB;
    kind_value TEXT;
BEGIN
    IF NOT starring_runtime_private_v2.starring_runtime_pending_drain_root_exact_v2(
            drain_row
        )
        OR drain_row.canonical_state_bytes IS NULL
        OR drain_row.canonical_state_digest IS NULL
        OR pg_catalog.octet_length(drain_row.canonical_state_bytes)
            NOT BETWEEN 1 AND 1048576
        OR pg_catalog.encode(
            pg_catalog.sha256(drain_row.canonical_state_bytes),
            'hex'
        ) <> drain_row.canonical_state_digest
    THEN
        RETURN FALSE;
    END IF;

    BEGIN
        state_text := pg_catalog.convert_from(
            drain_row.canonical_state_bytes,
            'UTF8'
        );
        state_value := state_text::JSONB;
    EXCEPTION
        WHEN OTHERS THEN
            RETURN FALSE;
    END;

    kind_value := state_value #>> '{state,kind}';
    RETURN pg_catalog.jsonb_typeof(state_value) = 'object'
        AND (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(state_value)
        ) = 4
        AND state_value ->> 'format_version' = '2'
        AND state_value ->> 'intent_revision'
            = drain_row.intent_revision::TEXT
        AND state_value #> '{root,key}'
            = (
                pg_catalog.convert_from(
                    drain_row.drain_intent_request_bytes,
                    'UTF8'
                )::JSONB
            ) -> 'key'
        AND state_value #>> '{root,drain_intent_digest}'
            = drain_row.drain_intent_digest
        AND (
            (
                drain_row.intent_state = 'pending'
                AND kind_value IN (
                    'pending_unclaimed',
                    'pending_claimed',
                    'pending_refenced'
                )
            )
            OR (
                drain_row.intent_state =
                    'route_absent_acknowledged'
                AND kind_value = 'route_absent_acknowledged'
            )
        )
        AND (
            kind_value <> 'pending_unclaimed'
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(
                    state_value -> 'state'
                )
            ) = 1
        )
        AND (
            kind_value NOT IN (
                'pending_claimed',
                'pending_refenced'
            )
            OR (
                (
                    SELECT pg_catalog.count(*)
                    FROM pg_catalog.jsonb_object_keys(
                        state_value -> 'state'
                    )
                ) = 2
                AND state_value
                    #>> '{state,claim,process_instance_id}'
                        ~ '^[A-Za-z0-9_.:-]{1,128}$'
                AND state_value
                    #>> '{state,claim,progress,seal,intent_id}'
                        = drain_row.drain_intent_id
                AND state_value
                    #>> '{state,claim,progress,seal,slot,guild_id}'
                        = drain_row.slot_guild_id
                AND state_value
                    #>> '{state,claim,progress,seal,slot,ruleset_key}'
                        = drain_row.slot_ruleset_key
                AND (
                    (
                        kind_value = 'pending_claimed'
                        AND state_value
                            #>> '{state,claim,progress,kind}'
                                = 'claimed'
                        AND state_value
                            #> '{state,claim,progress,seal,expected_route}'
                                = 'null'::JSONB
                    )
                    OR (
                        kind_value = 'pending_refenced'
                        AND state_value
                            #>> '{state,claim,progress,kind}'
                                = 'refenced'
                        AND pg_catalog.jsonb_typeof(
                            state_value
                                #> '{state,claim,progress,removal_target}'
                        ) = 'object'
                    )
                )
            )
        );
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_pending_drain_projection_v2(
    outcome_tag SMALLINT,
    source_digest_frame BYTEA,
    successor_state_frame BYTEA,
    evidence_frame BYTEA,
    product_root_frame BYTEA
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
BEGIN
    IF outcome_tag NOT BETWEEN 0 AND 2 THEN
        RETURN NULL;
    END IF;
    domain_bytes := pg_catalog.convert_to(
        'starring.runtime.startup_recovery.pending_drain.terminal.v2',
        'UTF8'
    );
    framed_payload :=
        pg_catalog.int8send(
            pg_catalog.octet_length(source_digest_frame)::BIGINT
        )
        || source_digest_frame
        || pg_catalog.int8send(
            pg_catalog.octet_length(successor_state_frame)::BIGINT
        )
        || successor_state_frame
        || pg_catalog.int8send(
            pg_catalog.octet_length(evidence_frame)::BIGINT
        )
        || evidence_frame
        || pg_catalog.int8send(
            pg_catalog.octet_length(product_root_frame)::BIGINT
        )
        || product_root_frame;
    RETURN
        pg_catalog.int8send(
            pg_catalog.octet_length(domain_bytes)::BIGINT
        )
        || domain_bytes
        || pg_catalog.int2send(2::SMALLINT)
        || pg_catalog.int2send(outcome_tag)
        || framed_payload
        || pg_catalog.sha256(framed_payload);
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_pending_drain_replay_exact_v2(
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
    projection_length BIGINT;
    cursor_position BIGINT;
    frame_index INTEGER;
    frame_length BIGINT;
    evidence_value BYTEA;
    payload_start BIGINT;
    payload_end BIGINT;
    payload_value BYTEA;
BEGIN
    domain_bytes := pg_catalog.convert_to(
        'starring.runtime.startup_recovery.pending_drain.terminal.v2',
        'UTF8'
    );
    projection_length := pg_catalog.octet_length(projection_bytes);
    IF projection_length NOT BETWEEN 1 AND 1048576
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
        RETURN FALSE;
    END IF;

    cursor_position :=
        9 + pg_catalog.octet_length(domain_bytes);
    IF pg_catalog.substr(
            projection_bytes,
            cursor_position::INTEGER,
            2
        ) IS DISTINCT FROM pg_catalog.int2send(2::SMALLINT)
    THEN
        RETURN FALSE;
    END IF;
    cursor_position := cursor_position + 4;
    payload_start := cursor_position;

    FOR frame_index IN 1..4 LOOP
        IF cursor_position + 7 > projection_length THEN
            RETURN FALSE;
        END IF;
        frame_length :=
            (
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
            OR cursor_position + frame_length - 1
                > projection_length
        THEN
            RETURN FALSE;
        END IF;
        IF frame_index = 3 THEN
            evidence_value := pg_catalog.substr(
                projection_bytes,
                cursor_position::INTEGER,
                frame_length::INTEGER
            );
        END IF;
        cursor_position := cursor_position + frame_length;
    END LOOP;

    payload_end := cursor_position - 1;
    IF cursor_position + 31 <> projection_length THEN
        RETURN FALSE;
    END IF;
    payload_value := pg_catalog.substr(
        projection_bytes,
        payload_start::INTEGER,
        (payload_end - payload_start + 1)::INTEGER
    );
    RETURN evidence_value IS NOT DISTINCT FROM expected_evidence_frame
        AND pg_catalog.substr(
            projection_bytes,
            cursor_position::INTEGER,
            32
        ) IS NOT DISTINCT FROM pg_catalog.sha256(payload_value);
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_exact_v2(
    projection_bytes BYTEA,
    expected_outcome_tag SMALLINT,
    expected_evidence_frame BYTEA,
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
    evidence_value BYTEA;
    requested_value BYTEA;
    payload_start BIGINT;
    payload_end BIGINT;
    payload_value BYTEA;
BEGIN
    IF expected_outcome_tag NOT BETWEEN 0 AND 2
        OR requested_frame_index NOT BETWEEN 1 AND 4
    THEN
        RETURN NULL;
    END IF;
    domain_bytes := pg_catalog.convert_to(
        'starring.runtime.startup_recovery.pending_drain.terminal.v2',
        'UTF8'
    );
    projection_length := pg_catalog.octet_length(projection_bytes);
    IF projection_length NOT BETWEEN 1 AND 1048576
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

    cursor_position :=
        9 + pg_catalog.octet_length(domain_bytes);
    IF pg_catalog.substr(
            projection_bytes,
            cursor_position::INTEGER,
            2
        ) IS DISTINCT FROM pg_catalog.int2send(2::SMALLINT)
        OR pg_catalog.substr(
            projection_bytes,
            (cursor_position + 2)::INTEGER,
            2
        ) IS DISTINCT FROM pg_catalog.int2send(
            expected_outcome_tag
        )
    THEN
        RETURN NULL;
    END IF;
    cursor_position := cursor_position + 4;
    payload_start := cursor_position;

    FOR frame_index IN 1..4 LOOP
        IF cursor_position + 7 > projection_length THEN
            RETURN NULL;
        END IF;
        frame_length :=
            (
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
            OR cursor_position + frame_length - 1
                > projection_length
        THEN
            RETURN NULL;
        END IF;
        IF frame_index = 3 THEN
            evidence_value := pg_catalog.substr(
                projection_bytes,
                cursor_position::INTEGER,
                frame_length::INTEGER
            );
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
    IF evidence_value IS DISTINCT FROM expected_evidence_frame
        OR pg_catalog.substr(
            projection_bytes,
            cursor_position::INTEGER,
            32
        ) IS DISTINCT FROM pg_catalog.sha256(payload_value)
    THEN
        RETURN NULL;
    END IF;
    RETURN requested_value;
EXCEPTION
    WHEN OTHERS THEN
        RETURN NULL;
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_pending_drain_product_root_compound_exact_v2(
    product_root_frame BYTEA,
    expected_selected_drain_intent_id TEXT,
    expected_source_intent_revision BIGINT,
    expected_claim_action_authority_revision BIGINT,
    expected_stage_tag SMALLINT,
    expected_prior_claim_terminal_digest TEXT,
    expected_seal_bundle BYTEA
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
    cursor_position BIGINT;
    root_length BIGINT;
    frame_length BIGINT;
    token_kind TEXT;
    prior_tag SMALLINT;
    token_kinds TEXT[];
    token_index INTEGER;
    selected_drain_intent_id_value BYTEA;
BEGIN
    IF expected_selected_drain_intent_id
            !~ '^[0-9a-f]{32}$'
        OR expected_source_intent_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_claim_action_authority_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_stage_tag NOT IN (1, 2)
        OR (
            expected_stage_tag = 1
            AND expected_prior_claim_terminal_digest <> ''
        )
        OR (
            expected_stage_tag = 2
            AND expected_prior_claim_terminal_digest
                !~ '^[0-9a-f]{64}$'
        )
        OR pg_catalog.octet_length(expected_seal_bundle)
            NOT BETWEEN 1 AND 4096
    THEN
        RETURN FALSE;
    END IF;
    root_length := pg_catalog.octet_length(product_root_frame);
    IF root_length NOT BETWEEN 1 AND 1048576
        OR pg_catalog.substr(
            product_root_frame,
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
            frame_length :=
                (
                    pg_catalog.get_byte(
                        product_root_frame,
                        cursor_position::INTEGER - 1
                    )::NUMERIC * 72057594037927936
                    + pg_catalog.get_byte(
                        product_root_frame,
                        cursor_position::INTEGER
                    )::NUMERIC * 281474976710656
                    + pg_catalog.get_byte(
                        product_root_frame,
                        cursor_position::INTEGER + 1
                    )::NUMERIC * 1099511627776
                    + pg_catalog.get_byte(
                        product_root_frame,
                        cursor_position::INTEGER + 2
                    )::NUMERIC * 4294967296
                    + pg_catalog.get_byte(
                        product_root_frame,
                        cursor_position::INTEGER + 3
                    )::NUMERIC * 16777216
                    + pg_catalog.get_byte(
                        product_root_frame,
                        cursor_position::INTEGER + 4
                    )::NUMERIC * 65536
                    + pg_catalog.get_byte(
                        product_root_frame,
                        cursor_position::INTEGER + 5
                    )::NUMERIC * 256
                    + pg_catalog.get_byte(
                        product_root_frame,
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
                selected_drain_intent_id_value := pg_catalog.substr(
                    product_root_frame,
                    cursor_position::INTEGER,
                    frame_length::INTEGER
                );
            END IF;
            cursor_position := cursor_position + frame_length;
        END IF;
    END LOOP;

    IF selected_drain_intent_id_value IS DISTINCT FROM
            pg_catalog.convert_to(
                expected_selected_drain_intent_id,
                'UTF8'
            )
        OR cursor_position + 17 > root_length
        OR pg_catalog.substr(
            product_root_frame,
            cursor_position::INTEGER,
            8
        ) IS DISTINCT FROM pg_catalog.int8send(
            expected_source_intent_revision
        )
        OR pg_catalog.substr(
            product_root_frame,
            (cursor_position + 8)::INTEGER,
            8
        ) IS DISTINCT FROM pg_catalog.int8send(
            expected_claim_action_authority_revision
        )
        OR pg_catalog.substr(
            product_root_frame,
            (cursor_position + 16)::INTEGER,
            2
        ) IS DISTINCT FROM pg_catalog.int2send(
            expected_stage_tag
        )
    THEN
        RETURN FALSE;
    END IF;
    cursor_position := cursor_position + 18;
    IF cursor_position + 1 > root_length THEN
        RETURN FALSE;
    END IF;
    prior_tag := CASE
        WHEN pg_catalog.substr(
                product_root_frame,
                cursor_position::INTEGER,
                2
            ) = pg_catalog.int2send(0::SMALLINT)
        THEN 0
        WHEN pg_catalog.substr(
                product_root_frame,
                cursor_position::INTEGER,
                2
            ) = pg_catalog.int2send(1::SMALLINT)
        THEN 1
        ELSE -1
    END;
    cursor_position := cursor_position + 2;
    IF (expected_stage_tag = 1 AND prior_tag <> 0)
        OR (expected_stage_tag = 2 AND prior_tag <> 1)
    THEN
        RETURN FALSE;
    END IF;
    IF prior_tag = 1 THEN
        IF cursor_position + 7 > root_length THEN
            RETURN FALSE;
        END IF;
        frame_length :=
            (
                pg_catalog.get_byte(
                    product_root_frame,
                    cursor_position::INTEGER - 1
                )::NUMERIC * 72057594037927936
                + pg_catalog.get_byte(
                    product_root_frame,
                    cursor_position::INTEGER
                )::NUMERIC * 281474976710656
                + pg_catalog.get_byte(
                    product_root_frame,
                    cursor_position::INTEGER + 1
                )::NUMERIC * 1099511627776
                + pg_catalog.get_byte(
                    product_root_frame,
                    cursor_position::INTEGER + 2
                )::NUMERIC * 4294967296
                + pg_catalog.get_byte(
                    product_root_frame,
                    cursor_position::INTEGER + 3
                )::NUMERIC * 16777216
                + pg_catalog.get_byte(
                    product_root_frame,
                    cursor_position::INTEGER + 4
                )::NUMERIC * 65536
                + pg_catalog.get_byte(
                    product_root_frame,
                    cursor_position::INTEGER + 5
                )::NUMERIC * 256
                + pg_catalog.get_byte(
                    product_root_frame,
                    cursor_position::INTEGER + 6
                )::NUMERIC
            )::BIGINT;
        cursor_position := cursor_position + 8;
        IF frame_length <> 64
            OR cursor_position + frame_length - 1 > root_length
            OR pg_catalog.substr(
                product_root_frame,
                cursor_position::INTEGER,
                frame_length::INTEGER
            ) IS DISTINCT FROM pg_catalog.convert_to(
                expected_prior_claim_terminal_digest,
                'UTF8'
            )
        THEN
            RETURN FALSE;
        END IF;
        cursor_position := cursor_position + frame_length;
    END IF;

    IF cursor_position + 7 > root_length THEN
        RETURN FALSE;
    END IF;
    frame_length :=
        (
            pg_catalog.get_byte(
                product_root_frame,
                cursor_position::INTEGER - 1
            )::NUMERIC * 72057594037927936
            + pg_catalog.get_byte(
                product_root_frame,
                cursor_position::INTEGER
            )::NUMERIC * 281474976710656
            + pg_catalog.get_byte(
                product_root_frame,
                cursor_position::INTEGER + 1
            )::NUMERIC * 1099511627776
            + pg_catalog.get_byte(
                product_root_frame,
                cursor_position::INTEGER + 2
            )::NUMERIC * 4294967296
            + pg_catalog.get_byte(
                product_root_frame,
                cursor_position::INTEGER + 3
            )::NUMERIC * 16777216
            + pg_catalog.get_byte(
                product_root_frame,
                cursor_position::INTEGER + 4
            )::NUMERIC * 65536
            + pg_catalog.get_byte(
                product_root_frame,
                cursor_position::INTEGER + 5
            )::NUMERIC * 256
            + pg_catalog.get_byte(
                product_root_frame,
                cursor_position::INTEGER + 6
            )::NUMERIC
        )::BIGINT;
    cursor_position := cursor_position + 8;
    IF frame_length <> pg_catalog.octet_length(expected_seal_bundle)
        OR cursor_position + frame_length - 1 > root_length
        OR pg_catalog.substr(
            product_root_frame,
            cursor_position::INTEGER,
            frame_length::INTEGER
        ) IS DISTINCT FROM expected_seal_bundle
    THEN
        RETURN FALSE;
    END IF;
    cursor_position := cursor_position + frame_length;

    token_kinds := ARRAY[
        'i', 'f', 'i', 'f', 'f',
        'i', 'f', 'f', 'i'
    ];
    FOREACH token_kind IN ARRAY token_kinds
    LOOP
        IF token_kind = 'i' THEN
            IF cursor_position + 7 > root_length THEN
                RETURN FALSE;
            END IF;
            cursor_position := cursor_position + 8;
        ELSE
            IF cursor_position + 7 > root_length THEN
                RETURN FALSE;
            END IF;
            frame_length :=
                (
                    pg_catalog.get_byte(
                        product_root_frame,
                        cursor_position::INTEGER - 1
                    )::NUMERIC * 72057594037927936
                    + pg_catalog.get_byte(
                        product_root_frame,
                        cursor_position::INTEGER
                    )::NUMERIC * 281474976710656
                    + pg_catalog.get_byte(
                        product_root_frame,
                        cursor_position::INTEGER + 1
                    )::NUMERIC * 1099511627776
                    + pg_catalog.get_byte(
                        product_root_frame,
                        cursor_position::INTEGER + 2
                    )::NUMERIC * 4294967296
                    + pg_catalog.get_byte(
                        product_root_frame,
                        cursor_position::INTEGER + 3
                    )::NUMERIC * 16777216
                    + pg_catalog.get_byte(
                        product_root_frame,
                        cursor_position::INTEGER + 4
                    )::NUMERIC * 65536
                    + pg_catalog.get_byte(
                        product_root_frame,
                        cursor_position::INTEGER + 5
                    )::NUMERIC * 256
                    + pg_catalog.get_byte(
                        product_root_frame,
                        cursor_position::INTEGER + 6
                    )::NUMERIC
                )::BIGINT;
            cursor_position := cursor_position + 8;
            IF frame_length < 0
                OR cursor_position + frame_length - 1 > root_length
            THEN
                RETURN FALSE;
            END IF;
            cursor_position := cursor_position + frame_length;
        END IF;
    END LOOP;
    RETURN cursor_position = root_length + 1;
EXCEPTION
    WHEN OTHERS THEN
        RETURN FALSE;
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_pending_drain_initialize_v2()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    initial_bytes BYTEA;
    gate_stage TEXT;
    gate_product_operation_id TEXT;
    gate_drain_intent_id TEXT;
BEGIN
    IF TG_OP <> 'INSERT' THEN
        RETURN NEW;
    END IF;
    gate_stage := pg_catalog.current_setting(
        'starring.runtime_product_drain_first_apply_stage_v2',
        TRUE
    );
    gate_product_operation_id := pg_catalog.current_setting(
        'starring.runtime_product_drain_first_apply_product_operation_id_v2',
        TRUE
    );
    gate_drain_intent_id := pg_catalog.current_setting(
        'starring.runtime_product_drain_first_apply_drain_intent_id_v2',
        TRUE
    );
    IF NEW.product_operation_id IS NULL
        OR NEW.drain_intent_id IS NULL
        OR NEW.product_operation_id !~ '^[0-9a-f]{32}$'
        OR NEW.drain_intent_id !~ '^[0-9a-f]{32}$'
        OR gate_stage IS DISTINCT FROM 'drain_insert'
        OR gate_product_operation_id
            IS DISTINCT FROM NEW.product_operation_id
        OR gate_drain_intent_id
            IS DISTINCT FROM NEW.drain_intent_id
    THEN
        RETURN NEW;
    END IF;
    IF NEW.canonical_state_bytes IS NULL
        AND NEW.canonical_state_digest IS NULL
    THEN
        initial_bytes :=
            starring_runtime_private_v2.starring_runtime_pending_drain_initial_state_v2(
                NEW
            );
        IF initial_bytes IS NULL THEN
            RAISE EXCEPTION USING
                ERRCODE = '23514',
                MESSAGE = 'runtime_pending_drain_initial_state_invalid';
        END IF;
        NEW.canonical_state_bytes := initial_bytes;
        NEW.canonical_state_digest := pg_catalog.encode(
            pg_catalog.sha256(initial_bytes),
            'hex'
        );
    ELSIF NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
            NEW
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE = 'runtime_pending_drain_initial_state_invalid';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER runtime_drain_intents_v2_00_initialize_canonical_state
BEFORE INSERT ON public.runtime_drain_intents_v2
FOR EACH ROW
EXECUTE FUNCTION starring_runtime_private_v2.starring_runtime_pending_drain_initialize_v2();

CREATE OR REPLACE FUNCTION public.reject_runtime_product_drain_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    gate_stage TEXT;
    gate_product_operation_id TEXT;
    gate_drain_intent_id TEXT;
    gate_source_revision TEXT;
    gate_source_digest TEXT;
    gate_successor_revision TEXT;
    gate_successor_digest TEXT;
BEGIN
    gate_stage := pg_catalog.current_setting(
        'starring.runtime_product_drain_first_apply_stage_v2',
        TRUE
    );
    gate_product_operation_id := pg_catalog.current_setting(
        'starring.runtime_product_drain_first_apply_product_operation_id_v2',
        TRUE
    );
    gate_drain_intent_id := pg_catalog.current_setting(
        'starring.runtime_product_drain_first_apply_drain_intent_id_v2',
        TRUE
    );
    gate_source_revision := pg_catalog.current_setting(
        'starring.runtime_pending_drain_source_revision_v2',
        TRUE
    );
    gate_source_digest := pg_catalog.current_setting(
        'starring.runtime_pending_drain_source_digest_v2',
        TRUE
    );
    gate_successor_revision := pg_catalog.current_setting(
        'starring.runtime_pending_drain_successor_revision_v2',
        TRUE
    );
    gate_successor_digest := pg_catalog.current_setting(
        'starring.runtime_pending_drain_successor_digest_v2',
        TRUE
    );

    IF TG_OP = 'INSERT' THEN
        IF TG_RELID = pg_catalog.to_regclass(
                'public.runtime_product_operations_v2'
            )
        THEN
            IF gate_stage = 'product_insert'
                AND gate_product_operation_id = NEW.product_operation_id
                AND COALESCE(gate_drain_intent_id, '') = ''
            THEN
                PERFORM pg_catalog.set_config(
                    'starring.runtime_product_drain_first_apply_stage_v2',
                    '',
                    TRUE
                );
                RETURN NEW;
            END IF;
        ELSIF TG_RELID = pg_catalog.to_regclass(
                'public.runtime_drain_intents_v2'
            )
        THEN
            IF gate_stage = 'drain_insert'
                AND gate_product_operation_id = NEW.product_operation_id
                AND gate_drain_intent_id = NEW.drain_intent_id
                AND starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
                    NEW
                )
            THEN
                PERFORM pg_catalog.set_config(
                    'starring.runtime_product_drain_first_apply_stage_v2',
                    '',
                    TRUE
                );
                PERFORM pg_catalog.set_config(
                    'starring.runtime_product_drain_first_apply_product_operation_id_v2',
                    '',
                    TRUE
                );
                PERFORM pg_catalog.set_config(
                    'starring.runtime_product_drain_first_apply_drain_intent_id_v2',
                    '',
                    TRUE
                );
                RETURN NEW;
            END IF;
        END IF;
    ELSIF TG_OP = 'UPDATE' THEN
        IF TG_RELID = pg_catalog.to_regclass(
                'public.runtime_drain_intents_v2'
            )
        THEN
            IF gate_stage = 'pending_drain_recovery_update'
                AND gate_drain_intent_id = OLD.drain_intent_id
                AND gate_product_operation_id = OLD.product_operation_id
                AND gate_source_revision = OLD.intent_revision::TEXT
                AND gate_source_digest = OLD.canonical_state_digest
                AND gate_successor_revision = NEW.intent_revision::TEXT
                AND gate_successor_digest = NEW.canonical_state_digest
                AND NEW.intent_revision = OLD.intent_revision + 1
                AND NEW.drain_intent_id = OLD.drain_intent_id
                AND NEW.tenant_id = OLD.tenant_id
                AND NEW.installation_id = OLD.installation_id
                AND NEW.deployment_id = OLD.deployment_id
                AND NEW.slot_guild_id = OLD.slot_guild_id
                AND NEW.slot_ruleset_key = OLD.slot_ruleset_key
                AND NEW.expected_revision = OLD.expected_revision
                AND NEW.product_operation_id = OLD.product_operation_id
                AND NEW.product_mutation_digest
                    = OLD.product_mutation_digest
                AND NEW.drain_intent_request_bytes
                    = OLD.drain_intent_request_bytes
                AND NEW.drain_intent_digest = OLD.drain_intent_digest
                AND starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
                    NEW
                )
            THEN
                PERFORM pg_catalog.set_config(
                    'starring.runtime_product_drain_first_apply_stage_v2',
                    '',
                    TRUE
                );
                PERFORM pg_catalog.set_config(
                    'starring.runtime_product_drain_first_apply_product_operation_id_v2',
                    '',
                    TRUE
                );
                PERFORM pg_catalog.set_config(
                    'starring.runtime_product_drain_first_apply_drain_intent_id_v2',
                    '',
                    TRUE
                );
                PERFORM pg_catalog.set_config(
                    'starring.runtime_pending_drain_source_revision_v2',
                    '',
                    TRUE
                );
                PERFORM pg_catalog.set_config(
                    'starring.runtime_pending_drain_source_digest_v2',
                    '',
                    TRUE
                );
                PERFORM pg_catalog.set_config(
                    'starring.runtime_pending_drain_successor_revision_v2',
                    '',
                    TRUE
                );
                PERFORM pg_catalog.set_config(
                    'starring.runtime_pending_drain_successor_digest_v2',
                    '',
                    TRUE
                );
                RETURN NEW;
            END IF;
        END IF;
    END IF;

    RAISE EXCEPTION USING
        ERRCODE = '23514',
        MESSAGE = 'runtime_product_drain_mutation_rejected';
END;
$function$;

DO $patch_recovery_drain_state_contracts$
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
        WHERE function_row.oid = pg_catalog.to_regprocedure(identity);

        previous_fragment :=
            '        ) = ''CHECK (intent_state = ''''pending''''::text)'';';
        next_fragment :=
            '        ) = ''CHECK (intent_state = ANY (ARRAY[''''pending''''::text, ''''route_absent_acknowledged''''::text]))'';';
        IF definition IS NULL
            OR pg_catalog.strpos(definition, previous_fragment) = 0
            OR pg_catalog.strpos(
                pg_catalog.replace(definition, previous_fragment, ''),
                previous_fragment
            ) <> 0
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_pending_drain_state_constraint_patch_drift';
        END IF;
        definition := pg_catalog.replace(
            definition,
            previous_fragment,
            next_fragment
        );

        previous_fragment :=
            '            WHERE drain.intent_state IS DISTINCT FROM ''pending''';
        next_fragment :=
            '            WHERE drain.intent_state NOT IN (' || E'\n' ||
            '                    ''pending'',' || E'\n' ||
            '                    ''route_absent_acknowledged''' || E'\n' ||
            '                )' || E'\n' ||
            '                OR NOT starring_runtime_private_v2.' ||
            'starring_runtime_pending_drain_state_exact_v2(' || E'\n' ||
            '                    drain' || E'\n' ||
            '                )';
        IF pg_catalog.strpos(definition, previous_fragment) = 0
            OR pg_catalog.strpos(
                pg_catalog.replace(definition, previous_fragment, ''),
                previous_fragment
            ) <> 0
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_pending_drain_state_classifier_patch_drift';
        END IF;
        definition := pg_catalog.replace(
            definition,
            previous_fragment,
            next_fragment
        );

        previous_fragment :=
            '            AND NOT EXISTS (' || E'\n' ||
            '                SELECT 1' || E'\n' ||
            '                FROM public.runtime_drain_intents_v2 AS drain' ||
            E'\n' ||
            '                WHERE drain.slot_guild_id = deployment.guild_id' ||
            E'\n' ||
            '                    AND drain.slot_ruleset_key =' || E'\n' ||
            '                        deployment.ruleset_key' || E'\n' ||
            '                    AND drain.intent_state = ''pending''' ||
            E'\n' ||
            '            )';
        next_fragment :=
            '            AND NOT EXISTS (' || E'\n' ||
            '                SELECT 1' || E'\n' ||
            '                FROM public.runtime_drain_intents_v2 AS drain' ||
            E'\n' ||
            '                WHERE drain.slot_guild_id = deployment.guild_id' ||
            E'\n' ||
            '                    AND drain.slot_ruleset_key =' || E'\n' ||
            '                        deployment.ruleset_key' || E'\n' ||
            '                    AND drain.intent_state IN (' || E'\n' ||
            '                        ''pending'',' || E'\n' ||
            '                        ''route_absent_acknowledged''' || E'\n' ||
            '                    )' || E'\n' ||
            '            )';
        IF pg_catalog.strpos(definition, previous_fragment) = 0
            OR pg_catalog.strpos(
                pg_catalog.replace(definition, previous_fragment, ''),
                previous_fragment
            ) <> 0
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_pending_drain_live_exclusion_patch_drift';
        END IF;
        definition := pg_catalog.replace(
            definition,
            previous_fragment,
            next_fragment
        );
        EXECUTE definition;
    END LOOP;

    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_startup_recovery_observe_v2(text,text,bigint,text,bigint,timestamp with time zone)'
    );

    previous_fragment :=
        '    active_exact_route_count BIGINT;' || E'\n' ||
        '    pending_drain_count BIGINT;';
    next_fragment :=
        '    active_exact_route_count BIGINT;' || E'\n' ||
        '    pending_drain_count BIGINT;' || E'\n' ||
        '    acknowledged_drain_count BIGINT;';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_observation_declaration_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    SELECT pg_catalog.count(*)' || E'\n' ||
        '    INTO pending_drain_count' || E'\n' ||
        '    FROM public.runtime_drain_intents_v2 AS drain' || E'\n' ||
        '    WHERE drain.intent_state = ''pending'';';
    next_fragment :=
        '    SELECT' || E'\n' ||
        '        pg_catalog.count(*) FILTER (' || E'\n' ||
        '            WHERE drain.intent_state = ''pending''' || E'\n' ||
        '        ),' || E'\n' ||
        '        pg_catalog.count(*) FILTER (' || E'\n' ||
        '            WHERE drain.intent_state =' || E'\n' ||
        '                ''route_absent_acknowledged''' || E'\n' ||
        '        )' || E'\n' ||
        '    INTO pending_drain_count, acknowledged_drain_count' || E'\n' ||
        '    FROM public.runtime_drain_intents_v2 AS drain;';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_observation_count_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '        OR pending_drain_count > 4294967295';
    next_fragment :=
        '        OR pending_drain_count > 4294967295' || E'\n' ||
        '        OR acknowledged_drain_count > 4294967295';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_observation_bound_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    acknowledged_product_handoff_count := 0;';
    next_fragment :=
        '    acknowledged_product_handoff_count :=' || E'\n' ||
        '        acknowledged_drain_count;';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_observation_ack_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
    EXECUTE definition;
END;
$patch_recovery_drain_state_contracts$;

DO $patch_slot_fence_readers$
DECLARE
    identity TEXT;
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    previous_fragment := 'drain.intent_state = ''pending''';
    next_fragment :=
        'drain.intent_state IN (''pending'', ''route_absent_acknowledged'')';
    FOREACH identity IN ARRAY ARRAY[
        'starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(text,text)',
        'starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(text,text,bigint)'
    ]
    LOOP
        SELECT pg_catalog.pg_get_functiondef(function_row.oid)
        INTO definition
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid =
            pg_catalog.to_regprocedure(identity);
        IF definition IS NULL
            OR pg_catalog.strpos(definition, previous_fragment) = 0
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_pending_drain_slot_reader_patch_drift';
        END IF;
        EXECUTE pg_catalog.replace(
            definition,
            previous_fragment,
            next_fragment
        );
    END LOOP;
END;
$patch_slot_fence_readers$;

CREATE OR REPLACE FUNCTION public.validate_runtime_slot_writer_fence_symmetry_v2()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    current_drain_count BIGINT;
    current_fence_count BIGINT;
BEGIN
    IF TG_RELID = pg_catalog.to_regclass(
            'public.runtime_slot_writer_fences_v2'
        )
    THEN
        IF TG_OP = 'DELETE' THEN
            SELECT pg_catalog.count(*)
            INTO current_drain_count
            FROM public.runtime_drain_intents_v2 AS drain
            WHERE drain.slot_guild_id = OLD.slot_guild_id
                AND drain.slot_ruleset_key = OLD.slot_ruleset_key
                AND drain.intent_state IN (
                    'pending',
                    'route_absent_acknowledged'
                );
            IF current_drain_count <> 0 THEN
                RAISE EXCEPTION USING
                    ERRCODE = '23514',
                    MESSAGE = 'runtime_slot_writer_fence_symmetry_invalid';
            END IF;
            RETURN NULL;
        END IF;

        IF NEW.pending_drain_intent_id IS NULL THEN
            SELECT pg_catalog.count(*)
            INTO current_drain_count
            FROM public.runtime_drain_intents_v2 AS drain
            WHERE drain.slot_guild_id = NEW.slot_guild_id
                AND drain.slot_ruleset_key = NEW.slot_ruleset_key
                AND drain.intent_state IN (
                    'pending',
                    'route_absent_acknowledged'
                );
            IF current_drain_count <> 0 THEN
                RAISE EXCEPTION USING
                    ERRCODE = '23514',
                    MESSAGE = 'runtime_slot_writer_fence_symmetry_invalid';
            END IF;
            RETURN NULL;
        END IF;

        SELECT pg_catalog.count(*)
        INTO current_drain_count
        FROM public.runtime_drain_intents_v2 AS drain
        WHERE drain.drain_intent_id = NEW.pending_drain_intent_id
            AND drain.product_operation_id
                = NEW.pending_product_operation_id
            AND drain.tenant_id = NEW.pending_tenant_id
            AND drain.installation_id = NEW.pending_installation_id
            AND drain.deployment_id = NEW.pending_deployment_id
            AND drain.slot_guild_id = NEW.slot_guild_id
            AND drain.slot_ruleset_key = NEW.slot_ruleset_key
            AND drain.expected_revision = NEW.pending_expected_revision
            AND drain.intent_state IN (
                'pending',
                'route_absent_acknowledged'
            );
        IF current_drain_count <> 1 THEN
            RAISE EXCEPTION USING
                ERRCODE = '23514',
                MESSAGE = 'runtime_slot_writer_fence_symmetry_invalid';
        END IF;
        RETURN NULL;
    END IF;

    IF TG_OP <> 'DELETE'
        AND NEW.intent_state IN (
            'pending',
            'route_absent_acknowledged'
        )
    THEN
        SELECT pg_catalog.count(*)
        INTO current_fence_count
        FROM public.runtime_slot_writer_fences_v2 AS fence
        WHERE fence.slot_guild_id = NEW.slot_guild_id
            AND fence.slot_ruleset_key = NEW.slot_ruleset_key
            AND fence.pending_drain_intent_id = NEW.drain_intent_id
            AND fence.pending_product_operation_id
                = NEW.product_operation_id
            AND fence.pending_tenant_id = NEW.tenant_id
            AND fence.pending_installation_id = NEW.installation_id
            AND fence.pending_deployment_id = NEW.deployment_id
            AND fence.pending_expected_revision = NEW.expected_revision;
        IF current_fence_count <> 1 THEN
            RAISE EXCEPTION USING
                ERRCODE = '23514',
                MESSAGE = 'runtime_slot_writer_fence_symmetry_invalid';
        END IF;
    END IF;

    IF TG_OP <> 'INSERT'
        AND NOT EXISTS (
            SELECT 1
            FROM public.runtime_drain_intents_v2 AS drain
            WHERE drain.drain_intent_id = OLD.drain_intent_id
                AND drain.intent_state IN (
                    'pending',
                    'route_absent_acknowledged'
                )
        )
    THEN
        SELECT pg_catalog.count(*)
        INTO current_fence_count
        FROM public.runtime_slot_writer_fences_v2 AS fence
        WHERE fence.pending_drain_intent_id = OLD.drain_intent_id;
        IF current_fence_count <> 0 THEN
            RAISE EXCEPTION USING
                ERRCODE = '23514',
                MESSAGE = 'runtime_slot_writer_fence_symmetry_invalid';
        END IF;
    END IF;

    RETURN NULL;
END;
$function$;

DO $patch_deployment_history_guard$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.validate_runtime_convergence_attempt_projection()'
    );

    previous_fragment := '    failure_changed BOOLEAN;';
    next_fragment :=
        previous_fragment || E'\n' ||
        '    pending_drain_history BOOLEAN;';
    IF definition IS NULL
        OR pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(
                definition,
                previous_fragment,
                ''
            ),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_deployment_guard_declaration_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '        IF NEW.last_fencing_token IS DISTINCT FROM OLD.last_fencing_token' || E'\n' ||
        '            AND (' || E'\n' ||
        '                NEW.controller_id IS NULL' || E'\n' ||
        '                OR NEW.last_controller_id IS DISTINCT FROM NEW.controller_id' || E'\n' ||
        '            )' || E'\n' ||
        '        THEN';
    next_fragment :=
        '        pending_drain_history := (' || E'\n' ||
        '            COALESCE(pg_catalog.current_setting(' || E'\n' ||
        '                ''starring.runtime_pending_drain_deployment_action_v2'',' || E'\n' ||
        '                TRUE' || E'\n' ||
        '            ), '''') = ''advance_history''' || E'\n' ||
        '            AND COALESCE(pg_catalog.current_setting(' || E'\n' ||
        '                ''starring.runtime_pending_drain_deployment_id_v2'',' || E'\n' ||
        '                TRUE' || E'\n' ||
        '            ), '''') = OLD.deployment_id' || E'\n' ||
        '            AND COALESCE(pg_catalog.current_setting(' || E'\n' ||
        '                ''starring.runtime_pending_drain_source_fence_v2'',' || E'\n' ||
        '                TRUE' || E'\n' ||
        '            ), '''') = OLD.last_fencing_token::TEXT' || E'\n' ||
        '            AND COALESCE(pg_catalog.current_setting(' || E'\n' ||
        '                ''starring.runtime_pending_drain_successor_fence_v2'',' || E'\n' ||
        '                TRUE' || E'\n' ||
        '            ), '''') = NEW.last_fencing_token::TEXT' || E'\n' ||
        '            AND COALESCE(pg_catalog.current_setting(' || E'\n' ||
        '                ''starring.runtime_pending_drain_successor_controller_v2'',' || E'\n' ||
        '                TRUE' || E'\n' ||
        '            ), '''') = NEW.last_controller_id' || E'\n' ||
        '            AND OLD.controller_id IS NULL' || E'\n' ||
        '            AND NEW.controller_id IS NULL' || E'\n' ||
        '            AND OLD.last_fencing_token BETWEEN 1 AND 9223372036854775806' || E'\n' ||
        '            AND NEW.last_fencing_token = OLD.last_fencing_token + 1' || E'\n' ||
        '            AND NEW.snapshot = pg_catalog.jsonb_set(' || E'\n' ||
        '                OLD.snapshot,' || E'\n' ||
        '                ''{last_fencing_token}'',' || E'\n' ||
        '                pg_catalog.to_jsonb(NEW.last_fencing_token),' || E'\n' ||
        '                FALSE' || E'\n' ||
        '            )' || E'\n' ||
        '            AND pg_catalog.to_jsonb(NEW)' || E'\n' ||
        '                - ARRAY[''snapshot'', ''last_fencing_token'', ''last_controller_id'']' || E'\n' ||
        '                = pg_catalog.to_jsonb(OLD)' || E'\n' ||
        '                - ARRAY[''snapshot'', ''last_fencing_token'', ''last_controller_id'']' || E'\n' ||
        '        );' || E'\n' ||
        E'\n' ||
        '        IF NEW.last_fencing_token IS DISTINCT FROM OLD.last_fencing_token' || E'\n' ||
        '            AND (' || E'\n' ||
        '                NEW.controller_id IS NULL' || E'\n' ||
        '                OR NEW.last_controller_id IS DISTINCT FROM NEW.controller_id' || E'\n' ||
        '            )' || E'\n' ||
        '            AND NOT pending_drain_history' || E'\n' ||
        '        THEN';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_deployment_guard_identity_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '        IF NOT lease_claimed' || E'\n' ||
        '            AND NEW.last_fencing_token IS DISTINCT FROM OLD.last_fencing_token' || E'\n' ||
        '        THEN';
    next_fragment :=
        '        IF NOT lease_claimed' || E'\n' ||
        '            AND NEW.last_fencing_token IS DISTINCT FROM OLD.last_fencing_token' || E'\n' ||
        '            AND NOT pending_drain_history' || E'\n' ||
        '        THEN';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_deployment_guard_history_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$patch_deployment_history_guard$;

DO $patch_deployment_projection_guard$
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

    previous_fragment := '    certification_awaiting_reset BOOLEAN;';
    next_fragment :=
        previous_fragment || E'\n' ||
        '    pending_drain_history BOOLEAN;';
    IF definition IS NULL
        OR pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(
                definition,
                previous_fragment,
                ''
            ),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_deployment_projection_declaration_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    ELSE' || E'\n' ||
        '        certification_awaiting_reset := FALSE;' || E'\n' ||
        '    END IF;' || E'\n' ||
        E'\n' ||
        '    snapshot_phase := NEW.snapshot -> ''phase'' ->> ''phase'';';
    next_fragment :=
        '    ELSE' || E'\n' ||
        '        certification_awaiting_reset := FALSE;' || E'\n' ||
        '    END IF;' || E'\n' ||
        '    pending_drain_history := (' || E'\n' ||
        '        TG_OP = ''UPDATE''' || E'\n' ||
        '        AND COALESCE(pg_catalog.current_setting(' || E'\n' ||
        '            ''starring.runtime_pending_drain_deployment_action_v2'',' || E'\n' ||
        '            TRUE' || E'\n' ||
        '        ), '''') = ''advance_history''' || E'\n' ||
        '        AND COALESCE(pg_catalog.current_setting(' || E'\n' ||
        '            ''starring.runtime_pending_drain_deployment_id_v2'',' || E'\n' ||
        '            TRUE' || E'\n' ||
        '        ), '''') = OLD.deployment_id' || E'\n' ||
        '        AND COALESCE(pg_catalog.current_setting(' || E'\n' ||
        '            ''starring.runtime_pending_drain_source_fence_v2'',' || E'\n' ||
        '            TRUE' || E'\n' ||
        '        ), '''') = OLD.last_fencing_token::TEXT' || E'\n' ||
        '        AND COALESCE(pg_catalog.current_setting(' || E'\n' ||
        '            ''starring.runtime_pending_drain_successor_fence_v2'',' || E'\n' ||
        '            TRUE' || E'\n' ||
        '        ), '''') = NEW.last_fencing_token::TEXT' || E'\n' ||
        '        AND COALESCE(pg_catalog.current_setting(' || E'\n' ||
        '            ''starring.runtime_pending_drain_successor_controller_v2'',' || E'\n' ||
        '            TRUE' || E'\n' ||
        '        ), '''') = NEW.last_controller_id' || E'\n' ||
        '        AND OLD.controller_id IS NULL' || E'\n' ||
        '        AND NEW.controller_id IS NULL' || E'\n' ||
        '        AND OLD.last_fencing_token BETWEEN 1 AND 9223372036854775806' || E'\n' ||
        '        AND NEW.last_fencing_token = OLD.last_fencing_token + 1' || E'\n' ||
        '        AND NEW.snapshot = pg_catalog.jsonb_set(' || E'\n' ||
        '            OLD.snapshot,' || E'\n' ||
        '            ''{last_fencing_token}'',' || E'\n' ||
        '            pg_catalog.to_jsonb(NEW.last_fencing_token),' || E'\n' ||
        '            FALSE' || E'\n' ||
        '        )' || E'\n' ||
        '        AND pg_catalog.to_jsonb(NEW)' || E'\n' ||
        '            - ARRAY[''snapshot'', ''last_fencing_token'', ''last_controller_id'']' || E'\n' ||
        '            = pg_catalog.to_jsonb(OLD)' || E'\n' ||
        '            - ARRAY[''snapshot'', ''last_fencing_token'', ''last_controller_id'']' || E'\n' ||
        '    );' || E'\n' ||
        E'\n' ||
        '    snapshot_phase := NEW.snapshot -> ''phase'' ->> ''phase'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_deployment_projection_gate_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '        IF NEW.revision <> OLD.revision + 1' || E'\n' ||
        '            OR NEW.updated_at <= OLD.updated_at' || E'\n' ||
        '            OR NEW.updated_at < mutation_clock' || E'\n' ||
        '            OR NEW.updated_at > mutation_clock + INTERVAL ''1 microsecond''' || E'\n' ||
        '        THEN';
    next_fragment :=
        '        IF NOT pending_drain_history' || E'\n' ||
        '            AND (' || E'\n' ||
        '                NEW.revision <> OLD.revision + 1' || E'\n' ||
        '                OR NEW.updated_at <= OLD.updated_at' || E'\n' ||
        '                OR NEW.updated_at < mutation_clock' || E'\n' ||
        '                OR NEW.updated_at > mutation_clock + INTERVAL ''1 microsecond''' || E'\n' ||
        '            )' || E'\n' ||
        '        THEN';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_deployment_projection_revision_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$patch_deployment_projection_guard$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_pending_drain_key_text_v2(
    drain_row public.runtime_drain_intents_v2
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
    request_value JSONB;
    key_value JSONB;
    scope_value JSONB;
    slot_value JSONB;
    target_value JSONB;
    expected_revision_text TEXT;
    target_version_text TEXT;
    binding_revision_text TEXT;
BEGIN
    BEGIN
        request_value := pg_catalog.convert_from(
            drain_row.drain_intent_request_bytes,
            'UTF8'
        )::JSONB;
    EXCEPTION
        WHEN OTHERS THEN
            RETURN NULL;
    END;
    key_value := request_value -> 'key';
    scope_value := key_value -> 'scope';
    slot_value := key_value -> 'slot';
    target_value := key_value -> 'expected_target';
    expected_revision_text := key_value ->> 'expected_revision';
    target_version_text := target_value ->> 'version';
    binding_revision_text := target_value ->> 'binding_revision';

    IF pg_catalog.jsonb_typeof(request_value) <> 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(request_value)
        ) <> 2
        OR request_value ->> 'format_version' <> '2'
        OR pg_catalog.jsonb_typeof(
            request_value -> 'format_version'
        ) <> 'number'
        OR pg_catalog.jsonb_typeof(key_value) <> 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(key_value)
        ) <> 8
        OR pg_catalog.jsonb_typeof(key_value -> 'intent_id')
            <> 'string'
        OR key_value ->> 'intent_id' <> drain_row.drain_intent_id
        OR pg_catalog.jsonb_typeof(
            key_value -> 'product_operation_id'
        ) <> 'string'
        OR key_value ->> 'product_operation_id'
            <> drain_row.product_operation_id
        OR pg_catalog.jsonb_typeof(
            key_value -> 'product_mutation_digest'
        ) <> 'string'
        OR key_value ->> 'product_mutation_digest'
            <> drain_row.product_mutation_digest
        OR pg_catalog.jsonb_typeof(scope_value) <> 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(scope_value)
        ) <> 3
        OR pg_catalog.jsonb_typeof(scope_value -> 'tenant_id')
            <> 'string'
        OR scope_value ->> 'tenant_id' <> drain_row.tenant_id
        OR pg_catalog.jsonb_typeof(
            scope_value -> 'installation_id'
        ) <> 'string'
        OR scope_value ->> 'installation_id'
            <> drain_row.installation_id
        OR pg_catalog.jsonb_typeof(scope_value -> 'deployment_id')
            <> 'string'
        OR scope_value ->> 'deployment_id'
            <> drain_row.deployment_id
        OR pg_catalog.jsonb_typeof(key_value -> 'expected_revision')
            <> 'number'
        OR expected_revision_text !~ '^[1-9][0-9]{0,18}$'
        OR expected_revision_text::NUMERIC
            > 9223372036854775807
        OR expected_revision_text <> drain_row.expected_revision::TEXT
        OR pg_catalog.jsonb_typeof(slot_value) <> 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(slot_value)
        ) <> 2
        OR pg_catalog.jsonb_typeof(slot_value -> 'guild_id')
            <> 'string'
        OR slot_value ->> 'guild_id' <> drain_row.slot_guild_id
        OR pg_catalog.jsonb_typeof(slot_value -> 'ruleset_key')
            <> 'string'
        OR slot_value ->> 'ruleset_key'
            <> drain_row.slot_ruleset_key
        OR pg_catalog.jsonb_typeof(target_value) <> 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(target_value)
        ) <> 6
        OR pg_catalog.jsonb_typeof(target_value -> 'guild_id')
            <> 'string'
        OR target_value ->> 'guild_id' <> drain_row.slot_guild_id
        OR pg_catalog.jsonb_typeof(target_value -> 'ruleset_key')
            <> 'string'
        OR target_value ->> 'ruleset_key'
            <> drain_row.slot_ruleset_key
        OR pg_catalog.jsonb_typeof(target_value -> 'version')
            <> 'number'
        OR target_version_text !~ '^[1-9][0-9]{0,9}$'
        OR target_version_text::NUMERIC > 4294967295
        OR pg_catalog.jsonb_typeof(
            target_value -> 'content_hash'
        ) <> 'string'
        OR target_value ->> 'content_hash' !~ '^[0-9a-f]{64}$'
        OR pg_catalog.jsonb_typeof(
            target_value -> 'binding_revision'
        ) <> 'number'
        OR binding_revision_text !~ '^[1-9][0-9]{0,18}$'
        OR binding_revision_text::NUMERIC > 9223372036854775807
        OR pg_catalog.jsonb_typeof(
            target_value -> 'binding_fingerprint'
        ) <> 'string'
        OR target_value ->> 'binding_fingerprint'
            !~ '^[0-9a-f]{64}$'
        OR pg_catalog.jsonb_typeof(key_value -> 'mutation_kind')
            <> 'string'
        OR key_value ->> 'mutation_kind' NOT IN (
            'apply',
            'supersede',
            'cancel',
            'authority_change',
            'teardown'
        )
    THEN
        RETURN NULL;
    END IF;

    RETURN pg_catalog.concat(
        '{"intent_id":',
        pg_catalog.to_json(drain_row.drain_intent_id)::TEXT,
        ',"product_operation_id":',
        pg_catalog.to_json(drain_row.product_operation_id)::TEXT,
        ',"product_mutation_digest":',
        pg_catalog.to_json(drain_row.product_mutation_digest)::TEXT,
        ',"scope":{"tenant_id":',
        pg_catalog.to_json(drain_row.tenant_id)::TEXT,
        ',"installation_id":',
        pg_catalog.to_json(drain_row.installation_id)::TEXT,
        ',"deployment_id":',
        pg_catalog.to_json(drain_row.deployment_id)::TEXT,
        '},"expected_revision":',
        drain_row.expected_revision::TEXT,
        ',"slot":{"guild_id":',
        pg_catalog.to_json(drain_row.slot_guild_id)::TEXT,
        ',"ruleset_key":',
        pg_catalog.to_json(drain_row.slot_ruleset_key)::TEXT,
        '},"expected_target":{"guild_id":',
        pg_catalog.to_json(target_value ->> 'guild_id')::TEXT,
        ',"ruleset_key":',
        pg_catalog.to_json(target_value ->> 'ruleset_key')::TEXT,
        ',"version":',
        target_version_text,
        ',"content_hash":',
        pg_catalog.to_json(target_value ->> 'content_hash')::TEXT,
        ',"binding_revision":',
        binding_revision_text,
        ',"binding_fingerprint":',
        pg_catalog.to_json(
            target_value ->> 'binding_fingerprint'
        )::TEXT,
        '},"mutation_kind":',
        pg_catalog.to_json(key_value ->> 'mutation_kind')::TEXT,
        '}'
    );
EXCEPTION
    WHEN OTHERS THEN
        RETURN NULL;
END;
$function$;

CREATE OR REPLACE FUNCTION starring_runtime_private_v2.starring_runtime_pending_drain_root_exact_v2(
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
    key_text TEXT;
    expected_request BYTEA;
    domain_bytes BYTEA;
BEGIN
    key_text :=
        starring_runtime_private_v2.starring_runtime_pending_drain_key_text_v2(
            drain_row
        );
    IF key_text IS NULL THEN
        RETURN FALSE;
    END IF;
    expected_request := pg_catalog.convert_to(
        pg_catalog.concat(
            '{"format_version":2,"key":',
            key_text,
            '}'
        ),
        'UTF8'
    );
    domain_bytes :=
        pg_catalog.convert_to(
            'starring.runtime.drain_intent.v2',
            'UTF8'
        )
        || pg_catalog.decode('00', 'hex');
    RETURN expected_request = drain_row.drain_intent_request_bytes
        AND drain_row.drain_intent_digest = pg_catalog.encode(
            pg_catalog.sha256(
                pg_catalog.int8send(
                    pg_catalog.octet_length(domain_bytes)::BIGINT
                )
                || domain_bytes
                || pg_catalog.int8send(
                    pg_catalog.octet_length(expected_request)::BIGINT
                )
                || expected_request
            ),
            'hex'
        );
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_pending_drain_provenance_text_v2(
    provenance_text TEXT
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
    value JSONB;
    witness JSONB;
    owner_value JSONB;
    pause_value JSONB;
    kind_value TEXT;
    expected_text TEXT;
    numeric_value TEXT;
BEGIN
    BEGIN
        value := provenance_text::JSONB;
    EXCEPTION
        WHEN OTHERS THEN
            RETURN NULL;
    END;
    kind_value := value ->> 'kind';
    IF kind_value = 'ordinary' THEN
        pause_value := value -> 'pause';
        IF (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(value)
            ) <> 3
            OR value ->> 'barrier_id'
                !~ '^[A-Za-z0-9_.:-]{1,128}$'
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(pause_value)
            ) <> 4
        THEN
            RETURN NULL;
        END IF;
        FOREACH numeric_value IN ARRAY ARRAY[
            pause_value ->> 'coordinator_generation',
            pause_value ->> 'connection_epoch',
            pause_value ->> 'paused_admission_revision',
            pause_value ->> 'pause_sequence'
        ]
        LOOP
            IF numeric_value !~ '^[1-9][0-9]{0,18}$'
                OR numeric_value::NUMERIC
                    > 9223372036854775807
            THEN
                RETURN NULL;
            END IF;
        END LOOP;
        expected_text := pg_catalog.concat(
            '{"kind":"ordinary","barrier_id":',
            pg_catalog.to_json(value ->> 'barrier_id')::TEXT,
            ',"pause":{"coordinator_generation":',
            value #>> '{pause,coordinator_generation}',
            ',"connection_epoch":',
            value #>> '{pause,connection_epoch}',
            ',"paused_admission_revision":',
            value #>> '{pause,paused_admission_revision}',
            ',"pause_sequence":',
            value #>> '{pause,pause_sequence}',
            '}}'
        );
    ELSIF kind_value IN ('closed_recovery', 'shutdown') THEN
        witness := value -> 'witness';
        owner_value := witness -> 'gateway_owner_lease_id';
        IF (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(value)
            ) <> 2
            OR pg_catalog.jsonb_typeof(witness) <> 'object'
            OR pg_catalog.jsonb_typeof(owner_value) <> 'object'
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(owner_value)
            ) <> 4
            OR owner_value ->> 'gateway_shard_id' <> 'shard:0'
            OR owner_value ->> 'process_instance_id'
                !~ '^[A-Za-z0-9_.:-]{1,128}$'
            OR owner_value ->> 'lease_epoch'
                !~ '^[1-9][0-9]{0,18}$'
            OR owner_value ->> 'expected_build_revision'
                !~ '^[A-Za-z0-9_.:/-]{1,128}$'
            OR witness ->> 'process_instance_id'
                !~ '^[A-Za-z0-9_.:-]{1,128}$'
        THEN
            RETURN NULL;
        END IF;
        IF kind_value = 'closed_recovery' THEN
            IF (
                    SELECT pg_catalog.count(*)
                    FROM pg_catalog.jsonb_object_keys(witness)
                ) <> 12
                OR witness ->> 'recovery_id'
                    !~ '^[0-9a-f]{32}$'
            THEN
                RETURN NULL;
            END IF;
            FOREACH numeric_value IN ARRAY ARRAY[
                witness ->> 'originating_emergency_generation',
                witness ->> 'recovery_generation',
                witness ->> 'recovery_authority_revision',
                owner_value ->> 'lease_epoch',
                witness ->> 'observed_owner_revision',
                witness ->> 'connection_epoch',
                witness ->> 'paused_admission_revision',
                witness ->> 'connected_event_sequence',
                witness ->> 'pause_sequence'
            ]
            LOOP
                IF numeric_value !~ '^[1-9][0-9]{0,18}$'
                    OR numeric_value::NUMERIC
                        > 9223372036854775807
                THEN
                    RETURN NULL;
                END IF;
            END LOOP;
            IF witness ->> 'owner_expires_at_unix_microseconds'
                    !~ '^-?(0|[1-9][0-9]{0,18})$'
                OR (
                    witness
                        ->> 'owner_expires_at_unix_microseconds'
                )::NUMERIC NOT BETWEEN
                    -62135596800000000 AND 253402300799999999
                OR owner_value ->> 'process_instance_id'
                    <> witness ->> 'process_instance_id'
            THEN
                RETURN NULL;
            END IF;
            expected_text := pg_catalog.concat(
                '{"kind":"closed_recovery","witness":{"recovery_id":',
                pg_catalog.to_json(witness ->> 'recovery_id')::TEXT,
                ',"originating_emergency_generation":',
                witness ->> 'originating_emergency_generation',
                ',"recovery_generation":',
                witness ->> 'recovery_generation',
                ',"recovery_authority_revision":',
                witness ->> 'recovery_authority_revision',
                ',"gateway_owner_lease_id":{"gateway_shard_id":',
                pg_catalog.to_json(
                    owner_value ->> 'gateway_shard_id'
                )::TEXT,
                ',"process_instance_id":',
                pg_catalog.to_json(
                    owner_value ->> 'process_instance_id'
                )::TEXT,
                ',"lease_epoch":',
                owner_value ->> 'lease_epoch',
                ',"expected_build_revision":',
                pg_catalog.to_json(
                    owner_value ->> 'expected_build_revision'
                )::TEXT,
                '},"observed_owner_revision":',
                witness ->> 'observed_owner_revision',
                ',"owner_expires_at_unix_microseconds":',
                witness ->> 'owner_expires_at_unix_microseconds',
                ',"process_instance_id":',
                pg_catalog.to_json(
                    witness ->> 'process_instance_id'
                )::TEXT,
                ',"connection_epoch":',
                witness ->> 'connection_epoch',
                ',"paused_admission_revision":',
                witness ->> 'paused_admission_revision',
                ',"connected_event_sequence":',
                witness ->> 'connected_event_sequence',
                ',"pause_sequence":',
                witness ->> 'pause_sequence',
                '}}'
            );
        ELSE
            IF (
                    SELECT pg_catalog.count(*)
                    FROM pg_catalog.jsonb_object_keys(witness)
                ) <> 10
            THEN
                RETURN NULL;
            END IF;
            FOREACH numeric_value IN ARRAY ARRAY[
                witness ->> 'shutdown_generation',
                owner_value ->> 'lease_epoch',
                witness ->> 'observed_owner_revision',
                witness ->> 'connection_epoch',
                witness ->> 'paused_admission_revision',
                witness ->> 'connected_event_sequence',
                witness ->> 'pause_sequence'
            ]
            LOOP
                IF numeric_value !~ '^[1-9][0-9]{0,18}$'
                    OR numeric_value::NUMERIC
                        > 9223372036854775807
                THEN
                    RETURN NULL;
                END IF;
            END LOOP;
            IF witness ->> 'owner_expires_at_unix_microseconds'
                    !~ '^-?(0|[1-9][0-9]{0,18})$'
                OR (
                    witness
                        ->> 'owner_expires_at_unix_microseconds'
                )::NUMERIC NOT BETWEEN
                    -62135596800000000 AND 253402300799999999
                OR owner_value ->> 'process_instance_id'
                    <> witness ->> 'process_instance_id'
            THEN
                RETURN NULL;
            END IF;
            expected_text := pg_catalog.concat(
                '{"kind":"shutdown","witness":{"shutdown_generation":',
                witness ->> 'shutdown_generation',
                ',"gateway_owner_lease_id":{"gateway_shard_id":',
                pg_catalog.to_json(
                    owner_value ->> 'gateway_shard_id'
                )::TEXT,
                ',"process_instance_id":',
                pg_catalog.to_json(
                    owner_value ->> 'process_instance_id'
                )::TEXT,
                ',"lease_epoch":',
                owner_value ->> 'lease_epoch',
                ',"expected_build_revision":',
                pg_catalog.to_json(
                    owner_value ->> 'expected_build_revision'
                )::TEXT,
                '},"observed_owner_revision":',
                witness ->> 'observed_owner_revision',
                ',"owner_expires_at_unix_microseconds":',
                witness ->> 'owner_expires_at_unix_microseconds',
                ',"process_instance_id":',
                pg_catalog.to_json(
                    witness ->> 'process_instance_id'
                )::TEXT,
                ',"connection_epoch":',
                witness ->> 'connection_epoch',
                ',"paused_admission_revision":',
                witness ->> 'paused_admission_revision',
                ',"connected_event_sequence":',
                witness ->> 'connected_event_sequence',
                ',"pause_sequence":',
                witness ->> 'pause_sequence',
                '}}'
            );
        END IF;
    ELSE
        RETURN NULL;
    END IF;
    IF expected_text <> provenance_text THEN
        RETURN NULL;
    END IF;
    RETURN expected_text;
EXCEPTION
    WHEN OTHERS THEN
        RETURN NULL;
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_pending_drain_claim_text_v2(
    drain_row public.runtime_drain_intents_v2,
    claim_value JSONB
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
    owner_value JSONB;
    progress_value JSONB;
    seal_value JSONB;
    expected_route_text TEXT;
    old_route_text TEXT;
    removal_target_text TEXT;
    provenance_text TEXT;
    progress_text TEXT;
BEGIN
    owner_value := claim_value -> 'gateway_owner_lease_id';
    progress_value := claim_value -> 'progress';
    seal_value := progress_value -> 'seal';
    IF (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(claim_value)
        ) <> 9
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(owner_value)
        ) <> 4
        OR owner_value ->> 'gateway_shard_id' <> 'shard:0'
        OR owner_value ->> 'process_instance_id'
            !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR owner_value ->> 'lease_epoch'
            !~ '^[1-9][0-9]{0,18}$'
        OR (owner_value ->> 'lease_epoch')::NUMERIC
            > 9223372036854775807
        OR owner_value ->> 'expected_build_revision'
            !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR claim_value ->> 'observed_owner_revision'
            !~ '^[1-9][0-9]{0,18}$'
        OR (claim_value ->> 'observed_owner_revision')::NUMERIC
            > 9223372036854775807
        OR claim_value ->> 'process_instance_id'
            <> owner_value ->> 'process_instance_id'
        OR claim_value ->> 'controller_id'
            !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR claim_value ->> 'controller_fencing_token'
            !~ '^[1-9][0-9]{0,18}$'
        OR (claim_value ->> 'controller_fencing_token')::NUMERIC
            > 9223372036854775807
        OR claim_value ->> 'claim_epoch'
            !~ '^[1-9][0-9]{0,18}$'
        OR (claim_value ->> 'claim_epoch')::NUMERIC
            > 9223372036854775807
        OR claim_value ->> 'claim_revision'
            !~ '^[1-9][0-9]{0,18}$'
        OR (claim_value ->> 'claim_revision')::NUMERIC
            > 9223372036854775807
        OR claim_value ->> 'claim_expires_at_unix_microseconds'
            !~ '^-?(0|[1-9][0-9]{0,18})$'
        OR (
            claim_value ->> 'claim_expires_at_unix_microseconds'
        )::NUMERIC NOT BETWEEN
            -62135596800000000 AND 253402300799999999
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(seal_value)
        ) <> 6
        OR seal_value ->> 'process_instance_id'
            <> claim_value ->> 'process_instance_id'
        OR seal_value #>> '{slot,guild_id}'
            <> drain_row.slot_guild_id
        OR seal_value #>> '{slot,ruleset_key}'
            <> drain_row.slot_ruleset_key
        OR seal_value ->> 'intent_id' <> drain_row.drain_intent_id
        OR seal_value ->> 'seal_generation'
            !~ '^[1-9][0-9]{0,18}$'
        OR (seal_value ->> 'seal_generation')::NUMERIC
            > 9223372036854775807
        OR seal_value ->> 'registry_observation_sequence'
            !~ '^[1-9][0-9]{0,18}$'
        OR (
            seal_value ->> 'registry_observation_sequence'
        )::NUMERIC > 9223372036854775807
    THEN
        RETURN NULL;
    END IF;

    IF seal_value -> 'expected_route' = 'null'::JSONB THEN
        expected_route_text := 'null';
    ELSE
        BEGIN
            expected_route_text := pg_catalog.convert_from(
                starring_runtime_private_v2.starring_runtime_suspended_route_bytes_v2(
                    seal_value -> 'expected_route'
                ),
                'UTF8'
            );
        EXCEPTION
            WHEN OTHERS THEN
                RETURN NULL;
        END;
    END IF;

    IF progress_value ->> 'kind' = 'claimed' THEN
        IF (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(progress_value)
            ) <> 2
        THEN
            RETURN NULL;
        END IF;
        progress_text := pg_catalog.concat(
            '{"kind":"claimed","seal":',
            '{"process_instance_id":',
            pg_catalog.to_json(
                seal_value ->> 'process_instance_id'
            )::TEXT,
            ',"slot":{"guild_id":',
            pg_catalog.to_json(
                seal_value #>> '{slot,guild_id}'
            )::TEXT,
            ',"ruleset_key":',
            pg_catalog.to_json(
                seal_value #>> '{slot,ruleset_key}'
            )::TEXT,
            '},"intent_id":',
            pg_catalog.to_json(seal_value ->> 'intent_id')::TEXT,
            ',"seal_generation":',
            seal_value ->> 'seal_generation',
            ',"expected_route":',
            expected_route_text,
            ',"registry_observation_sequence":',
            seal_value ->> 'registry_observation_sequence',
            '}}'
        );
    ELSIF progress_value ->> 'kind' = 'refenced' THEN
        IF (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(progress_value)
            ) <> 7
        THEN
            RETURN NULL;
        END IF;
        BEGIN
            old_route_text := pg_catalog.convert_from(
                starring_runtime_private_v2.starring_runtime_suspended_route_bytes_v2(
                    progress_value -> 'old_route'
                ),
                'UTF8'
            );
            removal_target_text := pg_catalog.convert_from(
                starring_runtime_private_v2.starring_runtime_suspended_route_bytes_v2(
                    progress_value -> 'removal_target'
                ),
                'UTF8'
            );
        EXCEPTION
            WHEN OTHERS THEN
                RETURN NULL;
        END;
        provenance_text :=
            starring_runtime_private_v2.starring_runtime_pending_drain_provenance_text_v2(
                progress_value ->> 'provenance_json'
            );
        IF provenance_text IS NULL
            OR progress_value ->> 'registry_observation_sequence'
                !~ '^[1-9][0-9]{0,18}$'
            OR progress_value ->> 'refenced_at_unix_microseconds'
                !~ '^-?(0|[1-9][0-9]{0,18})$'
            OR (
                progress_value ->> 'refenced_at_unix_microseconds'
            )::NUMERIC NOT BETWEEN
                -62135596800000000 AND 253402300799999999
            OR seal_value -> 'expected_route'
                IS DISTINCT FROM progress_value -> 'old_route'
            OR progress_value #> '{old_route,identity}'
                IS DISTINCT FROM
                    progress_value #> '{removal_target,identity}'
            OR progress_value #>> '{old_route,route_incarnation}'
                IS DISTINCT FROM
                    progress_value
                        #>> '{removal_target,route_incarnation}'
            OR (
                progress_value
                    #>> '{removal_target,controller_fencing_token}'
            )::NUMERIC <= (
                progress_value
                    #>> '{old_route,controller_fencing_token}'
            )::NUMERIC
            OR (
                claim_value ->> 'controller_fencing_token'
            )::NUMERIC <> (
                progress_value
                    #>> '{removal_target,controller_fencing_token}'
            )::NUMERIC
            OR (
                progress_value
                    ->> 'registry_observation_sequence'
            )::NUMERIC <= (
                seal_value ->> 'registry_observation_sequence'
            )::NUMERIC
        THEN
            RETURN NULL;
        END IF;
        IF (progress_value ->> 'provenance_json')::JSONB
                ->> 'kind' <> 'ordinary'
            AND (progress_value ->> 'provenance_json')::JSONB
                #>> '{witness,process_instance_id}'
                    <> claim_value ->> 'process_instance_id'
        THEN
            RETURN NULL;
        END IF;
        progress_text := pg_catalog.concat(
            '{"kind":"refenced","seal":',
            '{"process_instance_id":',
            pg_catalog.to_json(
                seal_value ->> 'process_instance_id'
            )::TEXT,
            ',"slot":{"guild_id":',
            pg_catalog.to_json(
                seal_value #>> '{slot,guild_id}'
            )::TEXT,
            ',"ruleset_key":',
            pg_catalog.to_json(
                seal_value #>> '{slot,ruleset_key}'
            )::TEXT,
            '},"intent_id":',
            pg_catalog.to_json(seal_value ->> 'intent_id')::TEXT,
            ',"seal_generation":',
            seal_value ->> 'seal_generation',
            ',"expected_route":',
            expected_route_text,
            ',"registry_observation_sequence":',
            seal_value ->> 'registry_observation_sequence',
            '},"provenance_json":',
            pg_catalog.to_json(provenance_text)::TEXT,
            ',"old_route":',
            old_route_text,
            ',"removal_target":',
            removal_target_text,
            ',"registry_observation_sequence":',
            progress_value ->> 'registry_observation_sequence',
            ',"refenced_at_unix_microseconds":',
            progress_value ->> 'refenced_at_unix_microseconds',
            '}'
        );
    ELSE
        RETURN NULL;
    END IF;

    RETURN pg_catalog.concat(
        '{"gateway_owner_lease_id":{"gateway_shard_id":',
        pg_catalog.to_json(
            owner_value ->> 'gateway_shard_id'
        )::TEXT,
        ',"process_instance_id":',
        pg_catalog.to_json(
            owner_value ->> 'process_instance_id'
        )::TEXT,
        ',"lease_epoch":',
        owner_value ->> 'lease_epoch',
        ',"expected_build_revision":',
        pg_catalog.to_json(
            owner_value ->> 'expected_build_revision'
        )::TEXT,
        '},"observed_owner_revision":',
        claim_value ->> 'observed_owner_revision',
        ',"process_instance_id":',
        pg_catalog.to_json(
            claim_value ->> 'process_instance_id'
        )::TEXT,
        ',"controller_id":',
        pg_catalog.to_json(claim_value ->> 'controller_id')::TEXT,
        ',"controller_fencing_token":',
        claim_value ->> 'controller_fencing_token',
        ',"claim_epoch":',
        claim_value ->> 'claim_epoch',
        ',"claim_revision":',
        claim_value ->> 'claim_revision',
        ',"claim_expires_at_unix_microseconds":',
        claim_value ->> 'claim_expires_at_unix_microseconds',
        ',"progress":',
        progress_text,
        '}'
    );
EXCEPTION
    WHEN OTHERS THEN
        RETURN NULL;
END;
$function$;

CREATE OR REPLACE FUNCTION starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
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
    key_text TEXT;
    state_text TEXT;
    state_value JSONB;
    state_body JSONB;
    kind_value TEXT;
    claim_text TEXT;
    expected_route_text TEXT;
    provenance_text TEXT;
    acknowledgement_value JSONB;
    certification_value JSONB;
    provenance_value JSONB;
    certification_text TEXT;
    expected_state_text TEXT;
    expected_bytes BYTEA;
    refence_sequence NUMERIC;
    acknowledgement_sequence NUMERIC;
BEGIN
    key_text :=
        starring_runtime_private_v2.starring_runtime_pending_drain_key_text_v2(
            drain_row
        );
    IF key_text IS NULL
        OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_root_exact_v2(
            drain_row
        )
        OR drain_row.canonical_state_bytes IS NULL
        OR drain_row.canonical_state_digest IS NULL
        OR pg_catalog.octet_length(drain_row.canonical_state_bytes)
            NOT BETWEEN 1 AND 1048576
        OR pg_catalog.encode(
            pg_catalog.sha256(drain_row.canonical_state_bytes),
            'hex'
        ) <> drain_row.canonical_state_digest
    THEN
        RETURN FALSE;
    END IF;

    BEGIN
        state_text := pg_catalog.convert_from(
            drain_row.canonical_state_bytes,
            'UTF8'
        );
        state_value := state_text::JSONB;
    EXCEPTION
        WHEN OTHERS THEN
            RETURN FALSE;
    END;
    state_body := state_value -> 'state';
    kind_value := state_body ->> 'kind';
    IF (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(state_value)
        ) <> 4
        OR pg_catalog.jsonb_typeof(
            state_value -> 'format_version'
        ) <> 'number'
        OR state_value ->> 'format_version' <> '2'
        OR pg_catalog.jsonb_typeof(state_value -> 'root')
            <> 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(
                state_value -> 'root'
            )
        ) <> 2
        OR state_value #> '{root,key}'
            <> (
                pg_catalog.convert_from(
                    drain_row.drain_intent_request_bytes,
                    'UTF8'
                )::JSONB
            ) -> 'key'
        OR state_value #>> '{root,drain_intent_digest}'
            <> drain_row.drain_intent_digest
        OR pg_catalog.jsonb_typeof(
            state_value -> 'intent_revision'
        ) <> 'number'
        OR state_value ->> 'intent_revision'
            <> drain_row.intent_revision::TEXT
        OR pg_catalog.jsonb_typeof(state_body) <> 'object'
    THEN
        RETURN FALSE;
    END IF;

    IF kind_value = 'pending_unclaimed' THEN
        IF drain_row.intent_state <> 'pending'
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(state_body)
            ) <> 1
        THEN
            RETURN FALSE;
        END IF;
        expected_state_text := '{"kind":"pending_unclaimed"}';
    ELSIF kind_value IN ('pending_claimed', 'pending_refenced') THEN
        IF drain_row.intent_state <> 'pending'
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(state_body)
            ) <> 2
        THEN
            RETURN FALSE;
        END IF;
        claim_text :=
            starring_runtime_private_v2.starring_runtime_pending_drain_claim_text_v2(
                drain_row,
                state_body -> 'claim'
            );
        IF claim_text IS NULL
            OR (
                kind_value = 'pending_claimed'
                AND state_body #>> '{claim,progress,kind}'
                    <> 'claimed'
            )
            OR (
                kind_value = 'pending_refenced'
                AND state_body #>> '{claim,progress,kind}'
                    <> 'refenced'
            )
        THEN
            RETURN FALSE;
        END IF;
        expected_state_text := pg_catalog.concat(
            '{"kind":',
            pg_catalog.to_json(kind_value)::TEXT,
            ',"claim":',
            claim_text,
            '}'
        );
    ELSIF kind_value = 'route_absent_acknowledged' THEN
        acknowledgement_value :=
            state_body -> 'acknowledgement';
        certification_value :=
            acknowledgement_value -> 'certification';
        IF drain_row.intent_state <>
                'route_absent_acknowledged'
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(state_body)
            ) <> 2
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(
                    acknowledgement_value
                )
            ) <> 6
        THEN
            RETURN FALSE;
        END IF;
        IF certification_value ->> 'kind' =
                'no_operation_reserved'
            AND (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(
                    certification_value
                )
            ) = 1
        THEN
            certification_text :=
                '{"kind":"no_operation_reserved"}';
        ELSIF certification_value ->> 'kind' =
                'no_attestation_for_reserved_operation'
            AND (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(
                    certification_value
                )
            ) = 3
            AND certification_value ->> 'operation_id'
                ~ '^[0-9a-f]{32}$'
            AND certification_value ->> 'intent_fingerprint'
                ~ '^[0-9a-f]{64}$'
        THEN
            certification_text := pg_catalog.concat(
                '{"kind":"no_attestation_for_reserved_operation",',
                '"operation_id":',
                pg_catalog.to_json(
                    certification_value ->> 'operation_id'
                )::TEXT,
                ',"intent_fingerprint":',
                pg_catalog.to_json(
                    certification_value ->> 'intent_fingerprint'
                )::TEXT,
                '}'
            );
        ELSE
            RETURN FALSE;
        END IF;
        claim_text :=
            starring_runtime_private_v2.starring_runtime_pending_drain_claim_text_v2(
                drain_row,
                acknowledgement_value -> 'claim'
            );
        provenance_text :=
            starring_runtime_private_v2.starring_runtime_pending_drain_provenance_text_v2(
                acknowledgement_value ->> 'provenance_json'
            );
        BEGIN
            provenance_value := provenance_text::JSONB;
        EXCEPTION
            WHEN OTHERS THEN
                RETURN FALSE;
        END;
        IF acknowledgement_value -> 'expected_route'
                = 'null'::JSONB
        THEN
            expected_route_text := 'null';
        ELSE
            BEGIN
                expected_route_text := pg_catalog.convert_from(
                    starring_runtime_private_v2.starring_runtime_suspended_route_bytes_v2(
                        acknowledgement_value -> 'expected_route'
                    ),
                    'UTF8'
                );
            EXCEPTION
                WHEN OTHERS THEN
                    RETURN FALSE;
            END;
        END IF;
        IF claim_text IS NULL
            OR provenance_text IS NULL
            OR acknowledgement_value
                    ->> 'registry_observation_sequence'
                    !~ '^[1-9][0-9]{0,18}$'
            OR (
                acknowledgement_value
                    ->> 'registry_observation_sequence'
            )::NUMERIC > 9223372036854775807
            OR acknowledgement_value
                    ->> 'acknowledged_at_unix_microseconds'
                    !~ '^-?(0|[1-9][0-9]{0,18})$'
            OR (
                acknowledgement_value
                    ->> 'acknowledged_at_unix_microseconds'
            )::NUMERIC NOT BETWEEN
                -62135596800000000 AND 253402300799999999
            OR (
                acknowledgement_value
                    #>> '{claim,progress,kind}' = 'claimed'
                AND expected_route_text <> 'null'
            )
            OR (
                acknowledgement_value
                    #>> '{claim,progress,kind}' = 'refenced'
                AND acknowledgement_value -> 'expected_route'
                    <> acknowledgement_value
                        #> '{claim,progress,removal_target}'
            )
            OR (
                provenance_value ->> 'kind' <> 'ordinary'
                AND (
                    provenance_value
                        #>> '{witness,gateway_owner_lease_id,gateway_shard_id}'
                            IS DISTINCT FROM
                                acknowledgement_value
                                    #>>
                                        '{claim,gateway_owner_lease_id,gateway_shard_id}'
                    OR provenance_value
                        #>> '{witness,gateway_owner_lease_id,process_instance_id}'
                            IS DISTINCT FROM
                                acknowledgement_value
                                    #>>
                                        '{claim,gateway_owner_lease_id,process_instance_id}'
                    OR provenance_value
                        #>> '{witness,gateway_owner_lease_id,lease_epoch}'
                            IS DISTINCT FROM
                                acknowledgement_value
                                    #>>
                                        '{claim,gateway_owner_lease_id,lease_epoch}'
                    OR provenance_value
                        #>> '{witness,gateway_owner_lease_id,expected_build_revision}'
                            IS DISTINCT FROM
                                acknowledgement_value
                                    #>>
                                        '{claim,gateway_owner_lease_id,expected_build_revision}'
                    OR provenance_value
                        #>> '{witness,observed_owner_revision}'
                            IS DISTINCT FROM
                                acknowledgement_value
                                    #>> '{claim,observed_owner_revision}'
                    OR provenance_value
                        #>> '{witness,process_instance_id}'
                            IS DISTINCT FROM
                                acknowledgement_value
                                    #>> '{claim,process_instance_id}'
                )
            )
        THEN
            RETURN FALSE;
        END IF;
        IF acknowledgement_value
                #>> '{claim,progress,kind}' = 'refenced'
        THEN
            refence_sequence := (
                acknowledgement_value
                    #>> '{claim,progress,registry_observation_sequence}'
            )::NUMERIC;
            acknowledgement_sequence := (
                acknowledgement_value
                    ->> 'registry_observation_sequence'
            )::NUMERIC;
            IF acknowledgement_sequence <= refence_sequence THEN
                RETURN FALSE;
            END IF;
        END IF;
        expected_state_text := pg_catalog.concat(
            '{"kind":"route_absent_acknowledged",',
            '"acknowledgement":{"claim":',
            claim_text,
            ',"expected_route":',
            expected_route_text,
            ',"provenance_json":',
            pg_catalog.to_json(provenance_text)::TEXT,
            ',"registry_observation_sequence":',
            acknowledgement_value
                ->> 'registry_observation_sequence',
            ',"certification":',
            certification_text,
            ',',
            '"acknowledged_at_unix_microseconds":',
            acknowledgement_value
                ->> 'acknowledged_at_unix_microseconds',
            '}}'
        );
    ELSE
        RETURN FALSE;
    END IF;

    expected_bytes := pg_catalog.convert_to(
        pg_catalog.concat(
            '{"format_version":2,"root":{"key":',
            key_text,
            ',"drain_intent_digest":',
            pg_catalog.to_json(
                drain_row.drain_intent_digest
            )::TEXT,
            '},"intent_revision":',
            drain_row.intent_revision::TEXT,
            ',"state":',
            expected_state_text,
            '}'
        ),
        'UTF8'
    );
    RETURN expected_bytes = drain_row.canonical_state_bytes;
EXCEPTION
    WHEN OTHERS THEN
        RETURN FALSE;
END;
$function$;

DO $strict_backfill_exact$
DECLARE
    invalid_drain_count BIGINT;
BEGIN
    SELECT pg_catalog.count(*)
    INTO invalid_drain_count
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
        drain
    );
    IF invalid_drain_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_strict_backfill_invalid';
    END IF;
END
$strict_backfill_exact$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_pending_drain_candidate_v2()
RETURNS TABLE(
    active_pending_count BIGINT,
    selected_drain_intent_id TEXT
)
LANGUAGE plpgsql
VOLATILE
PARALLEL UNSAFE
SECURITY INVOKER
SET search_path = pg_catalog
ROWS 1
AS $function$
DECLARE
    writer_fence_count BIGINT;
    invalid_drain_count BIGINT;
    invalid_acknowledgement_count BIGINT;
    invalid_suspension_count BIGINT;
    invalid_suspension_exact_count BIGINT;
    higher_live_count BIGINT;
    higher_reservation_count BIGINT;
    higher_suspension_count BIGINT;
BEGIN
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
            MESSAGE = 'runtime_startup_pending_drain_state_ambiguous';
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
            MESSAGE = 'runtime_startup_pending_drain_state_ambiguous';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_acknowledgement_count
    FROM public.runtime_drain_intents_v2 AS drain
    CROSS JOIN LATERAL (
        SELECT pg_catalog.convert_from(
            drain.canonical_state_bytes,
            'UTF8'
        )::JSONB AS state_value
    ) AS decoded
    WHERE drain.intent_state = 'route_absent_acknowledged'
        AND (
            (
                decoded.state_value
                    #>> '{state,acknowledgement,certification,kind}'
                        = 'no_operation_reserved'
                AND EXISTS (
                    SELECT 1
                    FROM public.runtime_certification_operations_v2 AS reservation
                    WHERE reservation.tenant_id = drain.tenant_id
                        AND reservation.installation_id =
                            drain.installation_id
                        AND reservation.deployment_id =
                            drain.deployment_id
                        AND reservation.deployment_revision =
                            drain.expected_revision
                )
            )
            OR (
                decoded.state_value
                    #>> '{state,acknowledgement,certification,kind}'
                        =
                            'no_attestation_for_reserved_operation'
                AND NOT EXISTS (
                    SELECT 1
                    FROM public.runtime_certification_operations_v2 AS reservation
                    INNER JOIN public.runtime_certification_operation_terminals_v2 AS terminal
                        ON terminal.operation_id =
                            reservation.operation_id
                    WHERE reservation.operation_id =
                            decoded.state_value
                                #>>
                                    '{state,acknowledgement,certification,operation_id}'
                        AND reservation.intent_fingerprint =
                            decoded.state_value
                                #>>
                                    '{state,acknowledgement,certification,intent_fingerprint}'
                        AND reservation.tenant_id = drain.tenant_id
                        AND reservation.installation_id =
                            drain.installation_id
                        AND reservation.deployment_id =
                            drain.deployment_id
                        AND reservation.deployment_revision =
                            drain.expected_revision
                        AND terminal.terminal_outcome_name =
                            'awaiting_reset'
                        AND terminal.intent_fingerprint =
                            reservation.intent_fingerprint
                )
            )
        );
    IF invalid_acknowledgement_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_pending_drain_acknowledgement_invalid';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_suspension_count
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
    INTO invalid_suspension_exact_count
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
    IF invalid_suspension_count <> 0
        OR invalid_suspension_exact_count <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_pending_drain_higher_state_invalid';
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
    SELECT pg_catalog.count(*)
    INTO higher_suspension_count
    FROM public.runtime_suspended_attempts_v2 AS suspended
    WHERE suspended.local_effect_kind = 'exact_route';
    IF higher_live_count <> 0
        OR higher_reservation_count <> 0
        OR higher_suspension_count <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_startup_pending_drain_higher_priority';
    END IF;

    SELECT
        pg_catalog.count(*),
        (
            SELECT drain.drain_intent_id
            FROM public.runtime_drain_intents_v2 AS drain
            INNER JOIN public.runtime_slot_writer_fences_v2 AS slot
                ON slot.pending_drain_intent_id =
                    drain.drain_intent_id
                AND slot.pending_product_operation_id =
                    drain.product_operation_id
                AND slot.pending_tenant_id = drain.tenant_id
                AND slot.pending_installation_id =
                    drain.installation_id
                AND slot.pending_deployment_id =
                    drain.deployment_id
                AND slot.pending_expected_revision =
                    drain.expected_revision
            WHERE drain.intent_state = 'pending'
            ORDER BY
                slot.pending_marked_at,
                drain.drain_intent_id COLLATE pg_catalog."C"
            LIMIT 1
        )
    INTO active_pending_count, selected_drain_intent_id
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE drain.intent_state = 'pending';

    IF (active_pending_count = 0)
            IS DISTINCT FROM (selected_drain_intent_id IS NULL)
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_pending_drain_selection_invalid';
    END IF;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_startup_recovery_select_pending_drain_v2(
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
    selected_slot_guild_id TEXT,
    selected_slot_ruleset_key TEXT,
    selected_target_version BIGINT,
    selected_target_content_hash TEXT,
    selected_target_binding_revision BIGINT,
    selected_target_binding_fingerprint TEXT
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
    candidate_count BIGINT;
    candidate_id TEXT;
BEGIN
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
            MESSAGE = 'runtime_startup_pending_drain_selection_input_invalid';
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
            MESSAGE = 'runtime_startup_pending_drain_selection_owner_lost';
    END IF;
    IF observed_database_now < requested_minimum_database_now THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_pending_drain_selection_clock_regressed';
    END IF;

    SELECT candidate.*
    INTO STRICT candidate_count, candidate_id
    FROM starring_runtime_private_v2.starring_runtime_pending_drain_candidate_v2()
        AS candidate;
    IF candidate_count = 0 THEN
        selection_outcome_name := 'no_candidate';
    ELSE
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
                MESSAGE = 'runtime_startup_pending_drain_selection_changed';
        END IF;
        selection_outcome_name := 'candidate';
        selected_drain_intent_id := candidate_row.drain_intent_id;
        selected_source_intent_revision :=
            candidate_row.intent_revision;
        selected_source_state_digest :=
            candidate_row.canonical_state_digest;
        selected_slot_guild_id := candidate_row.slot_guild_id;
        selected_slot_ruleset_key :=
            candidate_row.slot_ruleset_key;
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
            OR product_row.expected_target_version
                NOT BETWEEN 1 AND 9223372036854775807
            OR product_row.expected_target_content_hash
                !~ '^[0-9a-f]{64}$'
            OR product_row.expected_target_binding_revision
                NOT BETWEEN 1 AND 9223372036854775807
            OR product_row.expected_target_binding_fingerprint
                !~ '^[0-9a-f]{64}$'
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
                MESSAGE = 'runtime_startup_pending_drain_selection_target_invalid';
        END IF;
        selected_target_version :=
            product_row.expected_target_version;
        selected_target_content_hash :=
            product_row.expected_target_content_hash;
        selected_target_binding_revision :=
            product_row.expected_target_binding_revision;
        selected_target_binding_fingerprint :=
            product_row.expected_target_binding_fingerprint;
    END IF;
    observed_owner_expires_at := owner_row.expires_at;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_startup_recovery_record_pending_drain_none_v2(
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
    action_record RECORD;
    selection_action_found BOOLEAN;
    authority_action_found BOOLEAN;
    candidate_count BIGINT;
    candidate_id TEXT;
    evidence_frame BYTEA;
    no_candidate_projection BYTEA;
    last_resume_frame BYTEA;
    domain_bytes BYTEA;
    ready_kind_tag SMALLINT;
BEGIN
    PERFORM pg_catalog.set_config('TimeZone', 'UTC', TRUE);
    IF pg_catalog.current_setting('transaction_isolation')
            <> 'serializable'
        OR pg_catalog.current_setting('transaction_read_only') <> 'off'
        OR requested_recovery_id !~ '^[0-9a-f]{32}$'
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
            MESSAGE = 'runtime_startup_pending_drain_no_candidate_input_invalid';
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
    no_candidate_projection :=
        starring_runtime_private_v2.starring_runtime_pending_drain_projection_v2(
            0::SMALLINT,
            ''::BYTEA,
            ''::BYTEA,
            evidence_frame,
            ''::BYTEA
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
            MESSAGE = 'runtime_startup_pending_drain_no_candidate_owner_lost';
    END IF;
    IF database_now < requested_minimum_database_now THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_pending_drain_no_candidate_clock_regressed';
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
        IF selection_action_found THEN
            existing_action_row := selection_action_row;
        ELSE
            existing_action_row := authority_action_row;
        END IF;
        domain_bytes := pg_catalog.convert_to(
            'starring.runtime.startup_recovery.pending_drain.terminal.v2',
            'UTF8'
        );
        IF NOT starring_runtime_private_v2.starring_runtime_pending_drain_replay_exact_v2(
                existing_action_row.terminal_projection_bytes,
                evidence_frame
            )
            OR pg_catalog.substr(
                    existing_action_row.terminal_projection_bytes,
                    (
                        11 + pg_catalog.octet_length(domain_bytes)
                    )::INTEGER,
                    2
                ) IS DISTINCT FROM pg_catalog.int2send(0::SMALLINT)
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_startup_pending_drain_no_candidate_replay_invalid';
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
        IF action_record.outcome_name IS DISTINCT FROM 'replayed'
            OR action_record.database_now < database_now
            OR action_record.database_now >= expected_owner_expires_at
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_startup_pending_drain_no_candidate_replay_invalid';
        END IF;
    ELSE
        SELECT candidate.*
        INTO STRICT candidate_count, candidate_id
        FROM starring_runtime_private_v2.starring_runtime_pending_drain_candidate_v2()
            AS candidate;
        IF candidate_count <> 0
            OR candidate_id IS NOT NULL
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX003',
                MESSAGE = 'runtime_startup_pending_drain_no_candidate_changed';
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
            no_candidate_projection
        ) AS record;
        IF action_record.outcome_name IS DISTINCT FROM 'applied'
            OR action_record.database_now < database_now
            OR action_record.database_now >= expected_owner_expires_at
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_startup_pending_drain_no_candidate_record_invalid';
        END IF;
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
    terminal_projection_bytes := CASE action_record.outcome_name
        WHEN 'replayed' THEN existing_action_row.terminal_projection_bytes
        ELSE no_candidate_projection
    END;
    terminal_digest := action_record.terminal_digest;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_startup_recovery_execute_pending_drain_v2(
    requested_recovery_id TEXT,
    requested_originating_emergency_generation BIGINT,
    requested_coordinator_generation BIGINT,
    requested_claim_action_authority_revision BIGINT,
    requested_claim_selection_authority_revision BIGINT,
    requested_ack_action_authority_revision BIGINT,
    requested_ack_selection_authority_revision BIGINT,
    requested_stage TEXT,
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
    requested_post_global_registry_failed_closed BOOLEAN,
    requested_prior_claim_terminal_digest TEXT
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
    prior_claim_action_row public.runtime_startup_recovery_actions_v2%ROWTYPE;
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
    requested_action_authority_revision BIGINT;
    requested_selection_authority_revision BIGINT;
    selection_action_found BOOLEAN;
    authority_action_found BOOLEAN;
    writer_fence_count BIGINT;
    invalid_drain_count BIGINT;
    invalid_acknowledgement_count BIGINT;
    active_pending_count BIGINT;
    matching_certification_count BIGINT;
    higher_live_count BIGINT;
    higher_reservation_count BIGINT;
    higher_suspension_count BIGINT;
    invalid_suspension_count BIGINT;
    invalid_suspension_exact_count BIGINT;
    selected_drain_intent_id TEXT;
    state_text TEXT;
    state_value JSONB;
    state_kind TEXT;
    request_text TEXT;
    product_request_value JSONB;
    drain_request_value JSONB;
    expected_product_bytes BYTEA;
    key_text TEXT;
    claim_marker TEXT;
    claim_start INTEGER;
    claim_text TEXT;
    removal_marker TEXT;
    removal_end_marker TEXT;
    removal_start INTEGER;
    removal_end INTEGER;
    expected_route_text TEXT;
    certification_text TEXT;
    provenance_text TEXT;
    successor_text TEXT;
    successor_bytes BYTEA;
    successor_digest TEXT;
    source_digest_frame BYTEA;
    evidence_frame BYTEA;
    product_root_frame BYTEA;
    prior_product_root_frame BYTEA;
    prior_source_digest_frame BYTEA;
    prior_successor_state_frame BYTEA;
    seal_bundle BYTEA;
    prior_digest_frame BYTEA;
    product_digest_bytes BYTEA;
    drain_digest_bytes BYTEA;
    no_candidate_projection BYTEA;
    progressed_projection BYTEA;
    domain_bytes BYTEA;
    last_resume_frame BYTEA;
    ready_kind_tag SMALLINT;
    stage_tag SMALLINT;
    prior_digest_tag SMALLINT;
    outcome_tag SMALLINT;
    owner_expiry_numeric NUMERIC;
    owner_expiry_unix_microseconds BIGINT;
    acknowledged_numeric NUMERIC;
    acknowledged_unix_microseconds BIGINT;
    deployment_mutation_numeric NUMERIC;
    deployment_mutation_clock TIMESTAMPTZ;
    successor_revision BIGINT;
    stage_source_intent_revision BIGINT;
    successor_fencing_token BIGINT;
    successor_controller_id TEXT;
    successor_snapshot JSONB;
BEGIN
    PERFORM pg_catalog.set_config('TimeZone', 'UTC', TRUE);

    IF pg_catalog.current_setting('transaction_isolation')
            <> 'serializable'
        OR pg_catalog.current_setting('transaction_read_only') <> 'off'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_pending_drain_transaction_invalid';
    END IF;

    stage_tag := CASE requested_stage
        WHEN 'claim' THEN 1
        WHEN 'acknowledge' THEN 2
        ELSE NULL
    END;
    requested_action_authority_revision := CASE stage_tag
        WHEN 1 THEN requested_claim_action_authority_revision
        WHEN 2 THEN requested_ack_action_authority_revision
    END;
    requested_selection_authority_revision := CASE stage_tag
        WHEN 1 THEN requested_claim_selection_authority_revision
        WHEN 2 THEN requested_ack_selection_authority_revision
    END;

    IF requested_recovery_id !~ '^[0-9a-f]{32}$'
        OR requested_originating_emergency_generation
            NOT BETWEEN 1 AND 9223372036854775806
        OR requested_coordinator_generation
            IS DISTINCT FROM
                requested_originating_emergency_generation + 1
        OR stage_tag IS NULL
        OR requested_claim_selection_authority_revision
            NOT BETWEEN 1 AND 9223372036854775805
        OR requested_claim_action_authority_revision
            IS DISTINCT FROM
                requested_claim_selection_authority_revision + 1
        OR requested_ack_selection_authority_revision
            IS DISTINCT FROM
                requested_claim_action_authority_revision
        OR requested_ack_action_authority_revision
            IS DISTINCT FROM
                requested_ack_selection_authority_revision + 1
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
            NOT BETWEEN 1 AND 9223372036854775805
        OR requested_selected_source_state_digest
            !~ '^[0-9a-f]{64}$'
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
                OR requested_post_slot_admission_generation::NUMERIC
                    <> requested_pre_slot_admission_generation::NUMERIC + 1
                OR requested_post_slot_observation_sequence::NUMERIC
                    <> requested_pre_slot_observation_sequence::NUMERIC + 1
                OR registry_retained_empty_tombstone_count < 1
                OR requested_post_global_retained_slot_count
                    <> registry_retained_slot_count
                OR requested_post_global_retained_empty_tombstone_count::NUMERIC
                    <> registry_retained_empty_tombstone_count::NUMERIC - 1
            )
        )
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
            stage_tag = 1
            AND requested_prior_claim_terminal_digest <> ''
        )
        OR (
            stage_tag = 2
            AND requested_prior_claim_terminal_digest
                !~ '^[0-9a-f]{64}$'
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_startup_pending_drain_input_invalid';
    END IF;

    stage_source_intent_revision :=
        requested_selected_source_intent_revision + stage_tag - 1;
    prior_digest_tag := CASE stage_tag
        WHEN 1 THEN 0
        ELSE 1
    END;
    prior_digest_frame := CASE prior_digest_tag
        WHEN 0 THEN ''::BYTEA
        ELSE
            pg_catalog.int8send(64::BIGINT)
            || pg_catalog.convert_to(
                requested_prior_claim_terminal_digest,
                'UTF8'
            )
    END;
    seal_bundle :=
        pg_catalog.int2send(2::SMALLINT)
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
        || pg_catalog.int2send(0::SMALLINT)
        || pg_catalog.int8send(0::BIGINT)
        || pg_catalog.int8send(registry_observation_sequence)
        || pg_catalog.int8send(registry_retained_slot_count)
        || pg_catalog.int8send(
            registry_retained_empty_tombstone_count
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
    no_candidate_projection :=
        starring_runtime_private_v2.starring_runtime_pending_drain_projection_v2(
            0::SMALLINT,
            ''::BYTEA,
            ''::BYTEA,
            evidence_frame,
            ''::BYTEA
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
            MESSAGE = 'runtime_startup_pending_drain_owner_lost';
    END IF;
    IF database_now < requested_minimum_database_now THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_pending_drain_clock_regressed';
    END IF;

    IF stage_tag = 2 THEN
        SELECT action.*
        INTO prior_claim_action_row
        FROM public.runtime_startup_recovery_actions_v2 AS action
        WHERE action.recovery_id = requested_recovery_id
            AND action.action_authority_revision =
                requested_claim_action_authority_revision
        FOR UPDATE;
        IF NOT FOUND
            OR prior_claim_action_row.record_format_version <> 2
            OR prior_claim_action_row.originating_emergency_generation
                IS DISTINCT FROM
                    requested_originating_emergency_generation
            OR prior_claim_action_row.coordinator_generation
                IS DISTINCT FROM requested_coordinator_generation
            OR prior_claim_action_row.selection_authority_revision
                IS DISTINCT FROM
                    requested_claim_selection_authority_revision
            OR prior_claim_action_row.recovery_class
                IS DISTINCT FROM 'pending_runtime_drain_intent'
            OR prior_claim_action_row.gateway_shard_id
                IS DISTINCT FROM expected_gateway_shard_id
            OR prior_claim_action_row.owner_process_instance_id
                IS DISTINCT FROM expected_owner_process_instance_id
            OR prior_claim_action_row.owner_lease_epoch
                IS DISTINCT FROM expected_owner_lease_epoch
            OR prior_claim_action_row.owner_runtime_build_revision
                IS DISTINCT FROM
                    expected_owner_runtime_build_revision
            OR prior_claim_action_row.owner_revision
                IS DISTINCT FROM expected_owner_revision
            OR prior_claim_action_row.owner_expires_at
                IS DISTINCT FROM expected_owner_expires_at
            OR prior_claim_action_row.minimum_database_now
                > requested_minimum_database_now
            OR prior_claim_action_row.recorded_at
                > requested_minimum_database_now
            OR prior_claim_action_row.terminal_digest
                IS DISTINCT FROM
                    requested_prior_claim_terminal_digest
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX003',
                MESSAGE = 'runtime_startup_pending_drain_prior_claim_invalid';
        END IF;
        prior_source_digest_frame :=
            starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_exact_v2(
                prior_claim_action_row.terminal_projection_bytes,
                1::SMALLINT,
                evidence_frame,
                1::SMALLINT
            );
        prior_successor_state_frame :=
            starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_exact_v2(
                prior_claim_action_row.terminal_projection_bytes,
                1::SMALLINT,
                evidence_frame,
                2::SMALLINT
            );
        prior_product_root_frame :=
            starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_exact_v2(
                prior_claim_action_row.terminal_projection_bytes,
                1::SMALLINT,
                evidence_frame,
                4::SMALLINT
            );
        IF prior_source_digest_frame IS DISTINCT FROM
                pg_catalog.convert_to(
                    requested_selected_source_state_digest,
                    'UTF8'
                )
            OR prior_successor_state_frame IS NULL
            OR prior_product_root_frame IS NULL
            OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_product_root_compound_exact_v2(
                prior_product_root_frame,
                requested_selected_drain_intent_id,
                requested_selected_source_intent_revision,
                requested_claim_action_authority_revision,
                1::SMALLINT,
                '',
                seal_bundle
            )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_startup_pending_drain_prior_claim_projection_invalid';
        END IF;
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
        IF existing_action_row.minimum_database_now
                > requested_minimum_database_now
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_startup_pending_drain_replay_clock_regressed';
        END IF;
        prior_source_digest_frame :=
            starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_exact_v2(
                existing_action_row.terminal_projection_bytes,
                stage_tag,
                evidence_frame,
                1::SMALLINT
            );
        prior_product_root_frame :=
            starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_exact_v2(
                existing_action_row.terminal_projection_bytes,
                stage_tag,
                evidence_frame,
                4::SMALLINT
            );
        IF prior_source_digest_frame IS DISTINCT FROM (
            CASE stage_tag
                WHEN 1 THEN
                    pg_catalog.convert_to(
                        requested_selected_source_state_digest,
                        'UTF8'
                    )
                ELSE
                    pg_catalog.convert_to(
                        pg_catalog.encode(
                            pg_catalog.sha256(
                                prior_successor_state_frame
                            ),
                            'hex'
                        ),
                        'UTF8'
                    )
            END
        )
            OR prior_product_root_frame IS NULL
            OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_product_root_compound_exact_v2(
                prior_product_root_frame,
                requested_selected_drain_intent_id,
                stage_source_intent_revision,
                requested_claim_action_authority_revision,
                stage_tag,
                requested_prior_claim_terminal_digest,
                seal_bundle
            )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_startup_pending_drain_replay_invalid';
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
            existing_action_row.minimum_database_now,
            existing_action_row.terminal_projection_bytes
        ) AS record;
        IF action_record.outcome_name IS DISTINCT FROM 'replayed'
            OR action_record.database_now < database_now
            OR action_record.database_now >= expected_owner_expires_at
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_startup_pending_drain_replay_invalid';
        END IF;

        terminal_outcome_name := CASE stage_tag
            WHEN 1 THEN 'claimed'
            ELSE 'route_absent_acknowledged'
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
            MESSAGE = 'runtime_startup_pending_drain_state_ambiguous';
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
            MESSAGE = 'runtime_startup_pending_drain_state_ambiguous';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_acknowledgement_count
    FROM public.runtime_drain_intents_v2 AS drain
    CROSS JOIN LATERAL (
        SELECT pg_catalog.convert_from(
            drain.canonical_state_bytes,
            'UTF8'
        )::JSONB AS state_value
    ) AS decoded
    WHERE drain.intent_state = 'route_absent_acknowledged'
        AND (
            (
                decoded.state_value
                    #>> '{state,acknowledgement,certification,kind}'
                        = 'no_operation_reserved'
                AND EXISTS (
                    SELECT 1
                    FROM public.runtime_certification_operations_v2 AS reservation
                    WHERE reservation.tenant_id = drain.tenant_id
                        AND reservation.installation_id =
                            drain.installation_id
                        AND reservation.deployment_id =
                            drain.deployment_id
                        AND reservation.deployment_revision =
                            drain.expected_revision
                )
            )
            OR (
                decoded.state_value
                    #>> '{state,acknowledgement,certification,kind}'
                        =
                            'no_attestation_for_reserved_operation'
                AND NOT EXISTS (
                    SELECT 1
                    FROM public.runtime_certification_operations_v2 AS reservation
                    INNER JOIN public.runtime_certification_operation_terminals_v2 AS terminal
                        ON terminal.operation_id =
                            reservation.operation_id
                    WHERE reservation.operation_id =
                            decoded.state_value
                                #>>
                                    '{state,acknowledgement,certification,operation_id}'
                        AND reservation.intent_fingerprint =
                            decoded.state_value
                                #>>
                                    '{state,acknowledgement,certification,intent_fingerprint}'
                        AND reservation.tenant_id = drain.tenant_id
                        AND reservation.installation_id =
                            drain.installation_id
                        AND reservation.deployment_id =
                            drain.deployment_id
                        AND reservation.deployment_revision =
                            drain.expected_revision
                        AND terminal.terminal_outcome_name =
                            'awaiting_reset'
                        AND terminal.intent_fingerprint =
                            reservation.intent_fingerprint
                )
            )
        );
    IF invalid_acknowledgement_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_pending_drain_acknowledgement_invalid';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_suspension_count
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
    INTO invalid_suspension_exact_count
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
    IF invalid_suspension_count <> 0
        OR invalid_suspension_exact_count <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_pending_drain_higher_state_invalid';
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
    SELECT pg_catalog.count(*)
    INTO higher_suspension_count
    FROM public.runtime_suspended_attempts_v2 AS suspended
    WHERE suspended.local_effect_kind = 'exact_route';
    IF higher_live_count <> 0
        OR higher_reservation_count <> 0
        OR higher_suspension_count <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_startup_pending_drain_higher_priority';
    END IF;

    SELECT
        pg_catalog.count(*),
        (
            SELECT drain.drain_intent_id
            FROM public.runtime_drain_intents_v2 AS drain
            INNER JOIN public.runtime_slot_writer_fences_v2 AS slot
                ON slot.pending_drain_intent_id =
                    drain.drain_intent_id
                AND slot.pending_product_operation_id =
                    drain.product_operation_id
                AND slot.pending_tenant_id = drain.tenant_id
                AND slot.pending_installation_id =
                    drain.installation_id
                AND slot.pending_deployment_id =
                    drain.deployment_id
                AND slot.pending_expected_revision =
                    drain.expected_revision
            WHERE drain.intent_state = 'pending'
            ORDER BY
                slot.pending_marked_at,
                drain.drain_intent_id COLLATE pg_catalog."C"
            LIMIT 1
        )
    INTO active_pending_count, selected_drain_intent_id
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE drain.intent_state = 'pending';

    IF active_pending_count = 0
        OR selected_drain_intent_id IS NULL
        OR selected_drain_intent_id
            IS DISTINCT FROM requested_selected_drain_intent_id
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_startup_pending_drain_selection_changed';
    END IF;

    SELECT drain.*
    INTO source_drain_row
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE drain.drain_intent_id =
        requested_selected_drain_intent_id;
    IF NOT FOUND
        OR source_drain_row.intent_state <> 'pending'
        OR source_drain_row.intent_revision
            IS DISTINCT FROM stage_source_intent_revision
        OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
            source_drain_row
        )
        OR (
            stage_tag = 1
            AND source_drain_row.canonical_state_digest
                IS DISTINCT FROM
                    requested_selected_source_state_digest
        )
        OR (
            stage_tag = 2
            AND (
                source_drain_row.canonical_state_bytes
                    IS DISTINCT FROM prior_successor_state_frame
                OR source_drain_row.canonical_state_digest
                    IS DISTINCT FROM pg_catalog.encode(
                        pg_catalog.sha256(
                            prior_successor_state_frame
                        ),
                        'hex'
                    )
            )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX001',
            MESSAGE = 'runtime_startup_pending_drain_candidate_changed';
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            pg_catalog.concat(
                'starring-runtime-serving-slot-v1:',
                source_drain_row.slot_guild_id,
                ':',
                source_drain_row.slot_ruleset_key
            ),
            0
        )
    );
    SELECT slot.*
    INTO slot_fence_row
    FROM public.runtime_slot_writer_fences_v2 AS slot
    WHERE slot.slot_guild_id = source_drain_row.slot_guild_id
        AND slot.slot_ruleset_key =
            source_drain_row.slot_ruleset_key
    FOR UPDATE;
    IF NOT FOUND
        OR slot_fence_row.writer_epoch
            NOT BETWEEN 1 AND 9223372036854775807
        OR slot_fence_row.pending_drain_intent_id
            IS DISTINCT FROM source_drain_row.drain_intent_id
        OR slot_fence_row.pending_product_operation_id
            IS DISTINCT FROM source_drain_row.product_operation_id
        OR slot_fence_row.pending_tenant_id
            IS DISTINCT FROM source_drain_row.tenant_id
        OR slot_fence_row.pending_installation_id
            IS DISTINCT FROM source_drain_row.installation_id
        OR slot_fence_row.pending_deployment_id
            IS DISTINCT FROM source_drain_row.deployment_id
        OR slot_fence_row.pending_expected_revision
            IS DISTINCT FROM source_drain_row.expected_revision
        OR slot_fence_row.pending_marked_at IS NULL
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_pending_drain_slot_invalid';
    END IF;

    SELECT serving.*
    INTO serving_row
    FROM public.runtime_serving_leases AS serving
    WHERE serving.guild_id = source_drain_row.slot_guild_id
        AND serving.ruleset_key =
            source_drain_row.slot_ruleset_key
    FOR UPDATE;
    IF FOUND
        AND (
            serving_row.connected
            OR serving_row.serving
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE = 'runtime_startup_pending_drain_serving_conflict';
    END IF;

    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = source_drain_row.tenant_id
        AND deployment.installation_id =
            source_drain_row.installation_id
        AND deployment.deployment_id =
            source_drain_row.deployment_id
    FOR UPDATE;
    IF NOT FOUND
        OR deployment_row.revision
            IS DISTINCT FROM source_drain_row.expected_revision
        OR deployment_row.guild_id
            IS DISTINCT FROM source_drain_row.slot_guild_id
        OR deployment_row.ruleset_key
            IS DISTINCT FROM source_drain_row.slot_ruleset_key
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
            MESSAGE = 'runtime_startup_pending_drain_deployment_invalid';
    END IF;

    SELECT product.*
    INTO product_row
    FROM public.runtime_product_operations_v2 AS product
    WHERE product.product_operation_id =
            source_drain_row.product_operation_id
        AND product.product_mutation_digest =
            source_drain_row.product_mutation_digest
        AND product.tenant_id = source_drain_row.tenant_id
        AND product.installation_id =
            source_drain_row.installation_id
        AND product.deployment_id =
            source_drain_row.deployment_id
        AND product.expected_revision =
            source_drain_row.expected_revision
        AND product.expected_target_guild_id =
            source_drain_row.slot_guild_id
        AND product.expected_target_ruleset_key =
            source_drain_row.slot_ruleset_key
    FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_pending_drain_product_invalid';
    END IF;

    BEGIN
        product_request_value := pg_catalog.convert_from(
            product_row.product_mutation_request_bytes,
            'UTF8'
        )::JSONB;
        drain_request_value := pg_catalog.convert_from(
            source_drain_row.drain_intent_request_bytes,
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
                MESSAGE = 'runtime_startup_pending_drain_product_root_invalid';
    END;

    IF expected_product_bytes
            IS DISTINCT FROM product_row.product_mutation_request_bytes
        OR starring_runtime_private_v2.starring_runtime_product_mutation_digest_v2(
                expected_product_bytes
            ) IS DISTINCT FROM product_row.product_mutation_digest
        OR product_row.expected_target_guild_id
            IS DISTINCT FROM deployment_row.guild_id
        OR product_row.expected_target_ruleset_key
            IS DISTINCT FROM deployment_row.ruleset_key
        OR product_row.expected_target_version
            IS DISTINCT FROM deployment_row.target_version
        OR product_row.expected_target_content_hash
            IS DISTINCT FROM deployment_row.target_content_hash
        OR product_row.expected_target_binding_revision
            IS DISTINCT FROM deployment_row.binding_revision
        OR product_row.expected_target_binding_fingerprint
            IS DISTINCT FROM deployment_row.binding_fingerprint
        OR drain_request_value
                #>> '{key,expected_target,guild_id}'
            IS DISTINCT FROM product_row.expected_target_guild_id
        OR drain_request_value
                #>> '{key,expected_target,ruleset_key}'
            IS DISTINCT FROM product_row.expected_target_ruleset_key
        OR drain_request_value
                #>> '{key,expected_target,version}'
            IS DISTINCT FROM product_row.expected_target_version::TEXT
        OR drain_request_value
                #>> '{key,expected_target,content_hash}'
            IS DISTINCT FROM product_row.expected_target_content_hash
        OR drain_request_value
                #>> '{key,expected_target,binding_revision}'
            IS DISTINCT FROM
                product_row.expected_target_binding_revision::TEXT
        OR drain_request_value
                #>> '{key,expected_target,binding_fingerprint}'
            IS DISTINCT FROM
                product_row.expected_target_binding_fingerprint
        OR deployment_row.snapshot_format_version <> 1
        OR deployment_row.desired_target_digest_version <> 1
        OR pg_catalog.jsonb_typeof(deployment_row.snapshot)
            IS DISTINCT FROM 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(
                deployment_row.snapshot
            )
        ) <> 17
        OR NOT deployment_row.snapshot ?& ARRAY[
            'identity',
            'target',
            'runtime_generation',
            'previous_runtime',
            'requested_at',
            'revision',
            'phase',
            'controller_lease',
            'last_fencing_token',
            'preflight',
            'drain',
            'activation',
            'panel_certificate',
            'gateway_ready',
            'live',
            'last_live_recovery',
            'last_runtime_failure'
        ]
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(
                deployment_row.snapshot -> 'identity'
            )
        ) <> 5
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(
                deployment_row.snapshot -> 'target'
            )
        ) <> 6
        OR deployment_row.snapshot
                #>> '{identity,deployment_id}'
            IS DISTINCT FROM deployment_row.deployment_id
        OR deployment_row.snapshot #>> '{identity,tenant_id}'
            IS DISTINCT FROM deployment_row.tenant_id
        OR deployment_row.snapshot
                #>> '{identity,installation_id}'
            IS DISTINCT FROM deployment_row.installation_id
        OR deployment_row.snapshot #>> '{identity,promotion_id}'
            IS DISTINCT FROM deployment_row.promotion_id
        OR deployment_row.snapshot
                #>> '{identity,activation_request_id}'
            IS DISTINCT FROM deployment_row.activation_request_id
        OR deployment_row.snapshot #>> '{target,guild_id}'
            IS DISTINCT FROM deployment_row.guild_id
        OR deployment_row.snapshot #>> '{target,ruleset_key}'
            IS DISTINCT FROM deployment_row.ruleset_key
        OR deployment_row.snapshot #>> '{target,version}'
            IS DISTINCT FROM deployment_row.target_version::TEXT
        OR deployment_row.snapshot #>> '{target,content_hash}'
            IS DISTINCT FROM deployment_row.target_content_hash
        OR deployment_row.snapshot
                #>> '{target,binding_revision}'
            IS DISTINCT FROM deployment_row.binding_revision::TEXT
        OR deployment_row.snapshot
                #>> '{target,binding_fingerprint}'
            IS DISTINCT FROM deployment_row.binding_fingerprint
        OR deployment_row.snapshot ->> 'runtime_generation'
            IS DISTINCT FROM deployment_row.runtime_generation::TEXT
        OR deployment_row.snapshot -> 'previous_runtime'
            IS DISTINCT FROM COALESCE(
                deployment_row.previous_runtime,
                'null'::JSONB
            )
        OR (
            deployment_row.snapshot ->> 'requested_at'
        )::TIMESTAMPTZ IS DISTINCT FROM deployment_row.requested_at
        OR deployment_row.snapshot ->> 'revision'
            IS DISTINCT FROM deployment_row.revision::TEXT
        OR pg_catalog.jsonb_typeof(
                deployment_row.snapshot -> 'phase'
            ) IS DISTINCT FROM 'object'
        OR deployment_row.snapshot #>> '{phase,phase}'
            IS DISTINCT FROM deployment_row.phase
        OR deployment_row.snapshot -> 'controller_lease'
            IS DISTINCT FROM 'null'::JSONB
        OR deployment_row.snapshot ->> 'last_fencing_token'
            IS DISTINCT FROM deployment_row.last_fencing_token::TEXT
        OR public.starring_runtime_desired_target_digest_v1(
                deployment_row.snapshot,
                deployment_row.installation_authority_revision
            ) IS DISTINCT FROM deployment_row.desired_target_digest
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_pending_drain_product_deployment_root_invalid';
    END IF;

    candidate_drain_row := source_drain_row;
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
            MESSAGE = 'runtime_startup_pending_drain_candidate_changed';
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
            MESSAGE = 'runtime_startup_pending_drain_certification_ambiguous';
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
                MESSAGE = 'runtime_startup_pending_drain_certification_pending';
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
                MESSAGE = 'runtime_startup_pending_drain_certification_invalid';
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
                MESSAGE = 'runtime_startup_pending_drain_committed_certification';
        END IF;
    ELSE
        certification_text :=
            '{"kind":"no_operation_reserved"}';
    END IF;

    state_text := pg_catalog.convert_from(
        source_drain_row.canonical_state_bytes,
        'UTF8'
    );
    state_value := state_text::JSONB;
    state_kind := state_value #>> '{state,kind}';
    successor_revision := source_drain_row.intent_revision + 1;
    request_text := pg_catalog.convert_from(
        source_drain_row.drain_intent_request_bytes,
        'UTF8'
    );
    key_text := pg_catalog.substr(
        request_text,
        27,
        pg_catalog.length(request_text) - 27
    );
    owner_expiry_numeric :=
        EXTRACT(EPOCH FROM expected_owner_expires_at) * 1000000;
    acknowledged_numeric :=
        EXTRACT(EPOCH FROM database_now) * 1000000;
    IF owner_expiry_numeric NOT BETWEEN
            -9223372036854775808 AND 9223372036854775807
        OR owner_expiry_numeric <>
            pg_catalog.trunc(owner_expiry_numeric)
        OR acknowledged_numeric NOT BETWEEN
            -9223372036854775808 AND 9223372036854775807
        OR acknowledged_numeric <>
            pg_catalog.trunc(acknowledged_numeric)
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_pending_drain_time_invalid';
    END IF;
    owner_expiry_unix_microseconds :=
        owner_expiry_numeric::BIGINT;
    acknowledged_unix_microseconds :=
        acknowledged_numeric::BIGINT;

    IF stage_tag = 1
        AND state_kind = 'pending_unclaimed'
    THEN
        deployment_mutation_clock :=
            public.starring_runtime_mutation_clock();
        deployment_mutation_numeric :=
            EXTRACT(EPOCH FROM deployment_mutation_clock) * 1000000;
        IF deployment_mutation_numeric NOT BETWEEN
                -9223372036854775808 AND 9223372036854775807
            OR deployment_mutation_numeric <>
                pg_catalog.trunc(deployment_mutation_numeric)
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_startup_pending_drain_mutation_time_invalid';
        END IF;
        acknowledged_unix_microseconds :=
            deployment_mutation_numeric::BIGINT;
        successor_fencing_token :=
            deployment_row.last_fencing_token + 1;
        successor_controller_id := pg_catalog.concat(
            'recovery:',
            requested_recovery_id,
            ':',
            requested_claim_action_authority_revision::TEXT
        );
        successor_snapshot := pg_catalog.jsonb_set(
            deployment_row.snapshot,
            '{last_fencing_token}',
            pg_catalog.to_jsonb(successor_fencing_token),
            FALSE
        );
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
            OR pg_catalog.to_jsonb(successor_deployment_row)
                - ARRAY[
                    'snapshot',
                    'last_fencing_token',
                    'last_controller_id'
                ]
                IS DISTINCT FROM
                    pg_catalog.to_jsonb(deployment_row)
                    - ARRAY[
                        'snapshot',
                        'last_fencing_token',
                        'last_controller_id'
                    ]
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
                MESSAGE = 'runtime_startup_pending_drain_fence_invalid';
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
        successor_text := pg_catalog.concat(
            '{"format_version":2,"root":{"key":',
            key_text,
            ',"drain_intent_digest":',
            pg_catalog.to_json(
                source_drain_row.drain_intent_digest
            )::TEXT,
            '},"intent_revision":',
            successor_revision::TEXT,
            ',"state":{"kind":"pending_claimed","claim":',
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
            '1',
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
            '}}}}}'
        );
        outcome_tag := 1;
        terminal_outcome_name := 'claimed';
    ELSIF stage_tag = 2
        AND state_kind = 'pending_claimed'
    THEN
        IF state_value
                #>> '{state,claim,gateway_owner_lease_id,gateway_shard_id}'
                IS DISTINCT FROM expected_gateway_shard_id
            OR state_value
                #>> '{state,claim,gateway_owner_lease_id,process_instance_id}'
                IS DISTINCT FROM expected_owner_process_instance_id
            OR state_value
                #>> '{state,claim,gateway_owner_lease_id,lease_epoch}'
                IS DISTINCT FROM expected_owner_lease_epoch::TEXT
            OR state_value
                #>> '{state,claim,gateway_owner_lease_id,expected_build_revision}'
                IS DISTINCT FROM expected_owner_runtime_build_revision
            OR state_value
                #>> '{state,claim,observed_owner_revision}'
                IS DISTINCT FROM expected_owner_revision::TEXT
            OR state_value
                #>> '{state,claim,process_instance_id}'
                IS DISTINCT FROM expected_owner_process_instance_id
            OR state_value
                #>> '{state,claim,claim_expires_at_unix_microseconds}'
                IS DISTINCT FROM
                    owner_expiry_unix_microseconds::TEXT
            OR state_value
                #>> '{state,claim,progress,seal,process_instance_id}'
                IS DISTINCT FROM expected_owner_process_instance_id
            OR state_value
                #>> '{state,claim,progress,seal,intent_id}'
                IS DISTINCT FROM source_drain_row.drain_intent_id
            OR state_value
                #>> '{state,claim,progress,seal,slot,guild_id}'
                IS DISTINCT FROM source_drain_row.slot_guild_id
            OR state_value
                #>> '{state,claim,progress,seal,slot,ruleset_key}'
                IS DISTINCT FROM source_drain_row.slot_ruleset_key
            OR state_value
                #>> '{state,claim,controller_id}'
                IS DISTINCT FROM deployment_row.last_controller_id
            OR state_value
                #>> '{state,claim,controller_fencing_token}'
                IS DISTINCT FROM deployment_row.last_fencing_token::TEXT
            OR state_value
                #>> '{state,claim,controller_id}'
                IS DISTINCT FROM pg_catalog.concat(
                    'recovery:',
                    requested_recovery_id,
                    ':',
                    requested_claim_action_authority_revision::TEXT
                )
            OR state_value #>> '{state,claim,claim_epoch}'
                IS DISTINCT FROM requested_coordinator_generation::TEXT
            OR state_value #>> '{state,claim,claim_revision}'
                IS DISTINCT FROM '1'
            OR state_value
                #>> '{state,claim,progress,seal,seal_generation}'
                IS DISTINCT FROM requested_seal_generation::TEXT
            OR state_value
                #>> '{state,claim,progress,seal,registry_observation_sequence}'
                IS DISTINCT FROM
                    requested_post_slot_observation_sequence::TEXT
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX003',
                MESSAGE = 'runtime_startup_pending_drain_claim_owner_invalid';
        END IF;
        claim_marker := pg_catalog.concat(
            ',"state":{"kind":"',
            state_kind,
            '","claim":'
        );
        claim_start :=
            pg_catalog.strpos(state_text, claim_marker)
            + pg_catalog.length(claim_marker);
        IF claim_start <= pg_catalog.length(claim_marker)
            OR pg_catalog.right(state_text, 2) <> '}}'
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_startup_pending_drain_claim_invalid';
        END IF;
        claim_text := pg_catalog.substr(
            state_text,
            claim_start,
            pg_catalog.length(state_text) - claim_start - 1
        );
        IF state_value
                #> '{state,claim,progress,seal,expected_route}'
                <> 'null'::JSONB
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE = 'runtime_startup_pending_drain_routed_claim';
        END IF;
        expected_route_text := 'null';

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
            claim_text,
            ',"expected_route":',
            expected_route_text,
            ',"provenance_json":',
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
        outcome_tag := 2;
        terminal_outcome_name :=
            'route_absent_acknowledged';
        successor_deployment_row := deployment_row;
    ELSE
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_pending_drain_progress_invalid';
    END IF;

    successor_bytes := pg_catalog.convert_to(
        successor_text,
        'UTF8'
    );
    successor_digest := pg_catalog.encode(
        pg_catalog.sha256(successor_bytes),
        'hex'
    );
    IF pg_catalog.octet_length(successor_bytes)
            NOT BETWEEN 1 AND 900000
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_pending_drain_successor_oversized';
    END IF;

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
        intent_state = CASE outcome_tag
            WHEN 1 THEN 'pending'
            ELSE 'route_absent_acknowledged'
        END,
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
        OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
            successor_drain_row
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
            MESSAGE = 'runtime_startup_pending_drain_cas_invalid';
    END IF;

    product_digest_bytes := pg_catalog.convert_to(
        product_row.product_mutation_digest,
        'UTF8'
    );
    drain_digest_bytes := pg_catalog.convert_to(
        source_drain_row.drain_intent_digest,
        'UTF8'
    );
    product_root_frame :=
        pg_catalog.int2send(2::SMALLINT)
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(product_row.tenant_id, 'UTF8')
            )::BIGINT
        )
        || pg_catalog.convert_to(product_row.tenant_id, 'UTF8')
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(
                    product_row.installation_id,
                    'UTF8'
                )
            )::BIGINT
        )
        || pg_catalog.convert_to(
            product_row.installation_id,
            'UTF8'
        )
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(product_row.deployment_id, 'UTF8')
            )::BIGINT
        )
        || pg_catalog.convert_to(product_row.deployment_id, 'UTF8')
        || pg_catalog.int8send(product_row.expected_revision)
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(
                    product_row.product_operation_id,
                    'UTF8'
                )
            )::BIGINT
        )
        || pg_catalog.convert_to(
            product_row.product_operation_id,
            'UTF8'
        )
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(source_drain_row.tenant_id, 'UTF8')
            )::BIGINT
        )
        || pg_catalog.convert_to(source_drain_row.tenant_id, 'UTF8')
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(
                    source_drain_row.installation_id,
                    'UTF8'
                )
            )::BIGINT
        )
        || pg_catalog.convert_to(
            source_drain_row.installation_id,
            'UTF8'
        )
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(
                    source_drain_row.deployment_id,
                    'UTF8'
                )
            )::BIGINT
        )
        || pg_catalog.convert_to(
            source_drain_row.deployment_id,
            'UTF8'
        )
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(
                    source_drain_row.slot_guild_id,
                    'UTF8'
                )
            )::BIGINT
        )
        || pg_catalog.convert_to(
            source_drain_row.slot_guild_id,
            'UTF8'
        )
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(
                    source_drain_row.slot_ruleset_key,
                    'UTF8'
                )
            )::BIGINT
        )
        || pg_catalog.convert_to(
            source_drain_row.slot_ruleset_key,
            'UTF8'
        )
        || pg_catalog.int8send(source_drain_row.expected_revision)
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(
                    source_drain_row.drain_intent_id,
                    'UTF8'
                )
            )::BIGINT
        )
        || pg_catalog.convert_to(
            source_drain_row.drain_intent_id,
            'UTF8'
        )
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(
                    product_row.expected_target_guild_id,
                    'UTF8'
                )
            )::BIGINT
        )
        || pg_catalog.convert_to(
            product_row.expected_target_guild_id,
            'UTF8'
        )
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(
                    product_row.expected_target_ruleset_key,
                    'UTF8'
                )
            )::BIGINT
        )
        || pg_catalog.convert_to(
            product_row.expected_target_ruleset_key,
            'UTF8'
        )
        || pg_catalog.int8send(
            product_row.expected_target_version
        )
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(
                    product_row.expected_target_content_hash,
                    'UTF8'
                )
            )::BIGINT
        )
        || pg_catalog.convert_to(
            product_row.expected_target_content_hash,
            'UTF8'
        )
        || pg_catalog.int8send(
            product_row.expected_target_binding_revision
        )
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(
                    product_row.expected_target_binding_fingerprint,
                    'UTF8'
                )
            )::BIGINT
        )
        || pg_catalog.convert_to(
            product_row.expected_target_binding_fingerprint,
            'UTF8'
        )
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                product_row.product_mutation_request_bytes
            )::BIGINT
        )
        || product_row.product_mutation_request_bytes
        || pg_catalog.int8send(
            pg_catalog.octet_length(product_digest_bytes)::BIGINT
        )
        || product_digest_bytes
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                source_drain_row.drain_intent_request_bytes
            )::BIGINT
        )
        || source_drain_row.drain_intent_request_bytes
        || pg_catalog.int8send(
            pg_catalog.octet_length(drain_digest_bytes)::BIGINT
        )
        || drain_digest_bytes
        || pg_catalog.int8send(
            source_drain_row.intent_revision
        )
        || pg_catalog.int8send(
            requested_claim_action_authority_revision
        )
        || pg_catalog.int2send(stage_tag)
        || pg_catalog.int2send(prior_digest_tag)
        || prior_digest_frame
        || pg_catalog.int8send(
            pg_catalog.octet_length(seal_bundle)::BIGINT
        )
        || seal_bundle
        || pg_catalog.int8send(deployment_row.revision)
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(deployment_row.phase, 'UTF8')
            )::BIGINT
        )
        || pg_catalog.convert_to(deployment_row.phase, 'UTF8')
        || pg_catalog.int8send(deployment_row.last_fencing_token)
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
        || pg_catalog.int8send(
            successor_deployment_row.last_fencing_token
        )
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(
                    successor_deployment_row.last_controller_id,
                    'UTF8'
                )
            )::BIGINT
        )
        || pg_catalog.convert_to(
            successor_deployment_row.last_controller_id,
            'UTF8'
        )
        || pg_catalog.int8send(
            pg_catalog.octet_length(
                pg_catalog.convert_to(
                    successor_deployment_row.snapshot::TEXT,
                    'UTF8'
                )
            )::BIGINT
        )
        || pg_catalog.convert_to(
            successor_deployment_row.snapshot::TEXT,
            'UTF8'
        )
        || pg_catalog.int8send(
            acknowledged_unix_microseconds
        );
    source_digest_frame := pg_catalog.convert_to(
        source_drain_row.canonical_state_digest,
        'UTF8'
    );
    progressed_projection :=
        starring_runtime_private_v2.starring_runtime_pending_drain_projection_v2(
            outcome_tag,
            source_digest_frame,
            successor_bytes,
            evidence_frame,
            product_root_frame
        );
    IF progressed_projection IS NULL
        OR pg_catalog.octet_length(progressed_projection)
            NOT BETWEEN 1 AND 1048576
        OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_product_root_compound_exact_v2(
            product_root_frame,
            source_drain_row.drain_intent_id,
            source_drain_row.intent_revision,
            requested_claim_action_authority_revision,
            stage_tag,
            requested_prior_claim_terminal_digest,
            seal_bundle
        )
        OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_replay_exact_v2(
            progressed_projection,
            evidence_frame
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_pending_drain_projection_invalid';
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
    IF action_record.outcome_name IS DISTINCT FROM 'applied'
        OR action_record.database_now < database_now
        OR action_record.database_now >= expected_owner_expires_at
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE = 'runtime_startup_pending_drain_record_invalid';
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

REVOKE ALL ON FUNCTION
    starring_runtime_private_v2.starring_runtime_pending_drain_initial_state_v2(
        public.runtime_drain_intents_v2
    ),
    starring_runtime_private_v2.starring_runtime_pending_drain_root_exact_v2(
        public.runtime_drain_intents_v2
    ),
    starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
        public.runtime_drain_intents_v2
    ),
    starring_runtime_private_v2.starring_runtime_pending_drain_key_text_v2(
        public.runtime_drain_intents_v2
    ),
    starring_runtime_private_v2.starring_runtime_pending_drain_provenance_text_v2(
        TEXT
    ),
    starring_runtime_private_v2.starring_runtime_pending_drain_claim_text_v2(
        public.runtime_drain_intents_v2,
        JSONB
    ),
    starring_runtime_private_v2.starring_runtime_pending_drain_projection_v2(
        SMALLINT,
        BYTEA,
        BYTEA,
        BYTEA,
        BYTEA
    ),
    starring_runtime_private_v2.starring_runtime_pending_drain_replay_exact_v2(
        BYTEA,
        BYTEA
    ),
    starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_exact_v2(
        BYTEA,
        SMALLINT,
        BYTEA,
        SMALLINT
    ),
    starring_runtime_private_v2.starring_runtime_pending_drain_product_root_compound_exact_v2(
        BYTEA,
        TEXT,
        BIGINT,
        BIGINT,
        SMALLINT,
        TEXT,
        BYTEA
    ),
    starring_runtime_private_v2.starring_runtime_pending_drain_initialize_v2(),
    starring_runtime_private_v2.starring_runtime_pending_drain_candidate_v2()
FROM PUBLIC;

REVOKE ALL ON FUNCTION
    public.starring_runtime_startup_recovery_select_pending_drain_v2(
        TEXT,
        TEXT,
        BIGINT,
        TEXT,
        BIGINT,
        TIMESTAMPTZ,
        TIMESTAMPTZ
    ),
    public.starring_runtime_startup_recovery_record_pending_drain_none_v2(
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
    ),
    public.starring_runtime_startup_recovery_execute_pending_drain_v2(
        TEXT,
        BIGINT,
        BIGINT,
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
        BOOLEAN,
        TEXT
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
            'public.starring_runtime_startup_recovery_select_pending_drain_v2(text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)',
            'public.starring_runtime_startup_recovery_record_pending_drain_none_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint)',
            'public.starring_runtime_startup_recovery_execute_pending_drain_v2(text,bigint,bigint,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean,text)'
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
        '            ''starring_runtime_private_v2.starring_runtime_suspended_quiescent_exact_v2(public.runtime_suspend_attempt_operations_v2,public.runtime_suspended_attempts_v2)''' || E'\n' ||
        '        )';
    next_fragment := previous_fragment;
    FOREACH identity IN ARRAY ARRAY[
        'public.starring_runtime_startup_recovery_select_pending_drain_v2(text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)',
        'public.starring_runtime_startup_recovery_record_pending_drain_none_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint)',
        'public.starring_runtime_startup_recovery_execute_pending_drain_v2(text,bigint,bigint,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean,text)',
        'starring_runtime_private_v2.starring_runtime_pending_drain_initial_state_v2(public.runtime_drain_intents_v2)',
        'starring_runtime_private_v2.starring_runtime_pending_drain_root_exact_v2(public.runtime_drain_intents_v2)',
        'starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(public.runtime_drain_intents_v2)',
        'starring_runtime_private_v2.starring_runtime_pending_drain_key_text_v2(public.runtime_drain_intents_v2)',
        'starring_runtime_private_v2.starring_runtime_pending_drain_provenance_text_v2(text)',
        'starring_runtime_private_v2.starring_runtime_pending_drain_claim_text_v2(public.runtime_drain_intents_v2,jsonb)',
        'starring_runtime_private_v2.starring_runtime_pending_drain_projection_v2(smallint,bytea,bytea,bytea,bytea)',
        'starring_runtime_private_v2.starring_runtime_pending_drain_replay_exact_v2(bytea,bytea)',
        'starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_exact_v2(bytea,smallint,bytea,smallint)',
        'starring_runtime_private_v2.starring_runtime_pending_drain_product_root_compound_exact_v2(bytea,text,bigint,bigint,smallint,text,bytea)',
        'starring_runtime_private_v2.starring_runtime_pending_drain_initialize_v2()',
        'starring_runtime_private_v2.starring_runtime_pending_drain_candidate_v2()'
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
            MESSAGE = 'runtime_startup_pending_drain_manifest_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        'RETURN observed_count = 809' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''fbe4a19a7ade16da18b9ce6670e7d1bf7085737d60563286b9176911faafd9dd'';';
    next_fragment :=
        'RETURN observed_count = 828' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''a10c4cc166d3fa07adc4bb800e47f3c0cfb1747b8f6a49fd8e1144d1a11865a3'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_pending_drain_manifest_expectation_patch_drift';
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
        '                ''public.starring_runtime_startup_recovery_execute_suspended_local_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint)'',' || E'\n' ||
        '                ''requested_recovery_id text, requested_originating_emergency_generation bigint, requested_coordinator_generation bigint, requested_action_authority_revision bigint, requested_selection_authority_revision bigint, expected_gateway_shard_id text, expected_owner_process_instance_id text, expected_owner_lease_epoch bigint, expected_owner_runtime_build_revision text, expected_owner_revision bigint, expected_owner_expires_at timestamp with time zone, requested_minimum_database_now timestamp with time zone, paused_process_instance_id text, paused_coordinator_generation bigint, paused_connection_epoch bigint, paused_ready_kind text, paused_admission_revision bigint, paused_transition_sequence bigint, paused_connected_event_sequence bigint, paused_last_resume_sequence bigint, registry_process_instance_id text, registry_observation_sequence bigint, registry_retained_slot_count bigint, registry_retained_empty_tombstone_count bigint''::TEXT,' || E'\n' ||
        '                ''TABLE(journal_outcome_name text, terminal_outcome_name text, recovery_id text, originating_emergency_generation bigint, coordinator_generation bigint, action_authority_revision bigint, selection_authority_revision bigint, recovery_class text, observed_gateway_shard_id text, observed_process_instance_id text, observed_lease_epoch bigint, observed_runtime_build_revision text, observed_owner_revision bigint, database_now timestamp with time zone, observed_owner_expires_at timestamp with time zone, minimum_database_now timestamp with time zone, recorded_at timestamp with time zone, terminal_projection_bytes bytea, terminal_digest text)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            )';
    next_fragment := previous_fragment || ',' || E'\n' ||
        '            (' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_select_pending_drain_v2(text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)'',' || E'\n' ||
        '                ''expected_gateway_shard_id text, expected_owner_process_instance_id text, expected_owner_lease_epoch bigint, expected_owner_runtime_build_revision text, expected_owner_revision bigint, expected_owner_expires_at timestamp with time zone, requested_minimum_database_now timestamp with time zone''::TEXT,' || E'\n' ||
        '                ''TABLE(selection_outcome_name text, observed_database_now timestamp with time zone, observed_owner_expires_at timestamp with time zone, selected_drain_intent_id text, selected_source_intent_revision bigint, selected_source_state_digest text, selected_slot_guild_id text, selected_slot_ruleset_key text, selected_target_version bigint, selected_target_content_hash text, selected_target_binding_revision bigint, selected_target_binding_fingerprint text)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            ),' || E'\n' ||
        '            (' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_record_pending_drain_none_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint)'',' || E'\n' ||
        '                ''requested_recovery_id text, requested_originating_emergency_generation bigint, requested_coordinator_generation bigint, requested_action_authority_revision bigint, requested_selection_authority_revision bigint, expected_gateway_shard_id text, expected_owner_process_instance_id text, expected_owner_lease_epoch bigint, expected_owner_runtime_build_revision text, expected_owner_revision bigint, expected_owner_expires_at timestamp with time zone, requested_minimum_database_now timestamp with time zone, paused_process_instance_id text, paused_coordinator_generation bigint, paused_connection_epoch bigint, paused_ready_kind text, paused_admission_revision bigint, paused_transition_sequence bigint, paused_connected_event_sequence bigint, paused_last_resume_sequence bigint, registry_process_instance_id text, registry_observation_sequence bigint, registry_retained_slot_count bigint, registry_retained_empty_tombstone_count bigint''::TEXT,' || E'\n' ||
        '                ''TABLE(journal_outcome_name text, terminal_outcome_name text, recovery_id text, originating_emergency_generation bigint, coordinator_generation bigint, action_authority_revision bigint, selection_authority_revision bigint, recovery_class text, observed_gateway_shard_id text, observed_process_instance_id text, observed_lease_epoch bigint, observed_runtime_build_revision text, observed_owner_revision bigint, database_now timestamp with time zone, observed_owner_expires_at timestamp with time zone, minimum_database_now timestamp with time zone, recorded_at timestamp with time zone, terminal_projection_bytes bytea, terminal_digest text)''::TEXT,' || E'\n' ||
        '                ''plpgsql''::TEXT,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                TRUE,' || E'\n' ||
        '                1::REAL' || E'\n' ||
        '            ),' || E'\n' ||
        '            (' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_execute_pending_drain_v2(text,bigint,bigint,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean,text)'',' || E'\n' ||
        '                ''requested_recovery_id text, requested_originating_emergency_generation bigint, requested_coordinator_generation bigint, requested_claim_action_authority_revision bigint, requested_claim_selection_authority_revision bigint, requested_ack_action_authority_revision bigint, requested_ack_selection_authority_revision bigint, requested_stage text, expected_gateway_shard_id text, expected_owner_process_instance_id text, expected_owner_lease_epoch bigint, expected_owner_runtime_build_revision text, expected_owner_revision bigint, expected_owner_expires_at timestamp with time zone, requested_minimum_database_now timestamp with time zone, paused_process_instance_id text, paused_coordinator_generation bigint, paused_connection_epoch bigint, paused_ready_kind text, paused_admission_revision bigint, paused_transition_sequence bigint, paused_connected_event_sequence bigint, paused_last_resume_sequence bigint, registry_process_instance_id text, registry_observation_sequence bigint, registry_retained_slot_count bigint, registry_retained_empty_tombstone_count bigint, requested_selected_drain_intent_id text, requested_selected_source_intent_revision bigint, requested_selected_source_state_digest text, requested_pre_slot_present boolean, requested_pre_slot_admission_generation bigint, requested_pre_slot_observation_sequence bigint, requested_seal_key bytea, requested_seal_generation bigint, requested_post_slot_admission_generation bigint, requested_post_slot_observation_sequence bigint, requested_post_global_observation_sequence bigint, requested_post_global_retained_slot_count bigint, requested_post_global_retained_empty_tombstone_count bigint, requested_post_global_staged_route_count bigint, requested_post_global_serving_route_count bigint, requested_post_global_draining_route_count bigint, requested_post_global_sealed_slot_count bigint, requested_post_global_active_interaction_count bigint, requested_post_global_failed_closed_slot_count bigint, requested_post_global_registry_failed_closed boolean, requested_prior_claim_terminal_digest text''::TEXT,' || E'\n' ||
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
            MESSAGE = 'runtime_startup_pending_drain_readiness_function_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            (''starring_runtime_private_v2.starring_runtime_suspended_quiescent_exact_v2(public.runtime_suspend_attempt_operations_v2,public.runtime_suspended_attempts_v2)''),';
    next_fragment := previous_fragment || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_pending_drain_initial_state_v2(public.runtime_drain_intents_v2)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_pending_drain_root_exact_v2(public.runtime_drain_intents_v2)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(public.runtime_drain_intents_v2)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_pending_drain_key_text_v2(public.runtime_drain_intents_v2)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_pending_drain_provenance_text_v2(text)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_pending_drain_claim_text_v2(public.runtime_drain_intents_v2,jsonb)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_pending_drain_projection_v2(smallint,bytea,bytea,bytea,bytea)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_pending_drain_replay_exact_v2(bytea,bytea)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_exact_v2(bytea,smallint,bytea,smallint)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_pending_drain_product_root_compound_exact_v2(bytea,text,bigint,bigint,smallint,text,bytea)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_pending_drain_initialize_v2()''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_pending_drain_candidate_v2()''),';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_pending_drain_readiness_private_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '''63268b8e2e30bbe523a437a5c326daa9ef25b863a866d4f1e67fcf46bc98bd95''::TEXT';
    next_fragment :=
        '''9de93ea5d565254c47533c7af43959aa873014bee385a2af775fafdcbf8118b9''::TEXT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_pending_drain_readiness_manifest_digest_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_execute_suspended_local_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint)''' || E'\n' ||
        '            )' || E'\n' ||
        '        )';
    next_fragment :=
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_execute_suspended_local_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint)''' || E'\n' ||
        '            ),' || E'\n' ||
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_select_pending_drain_v2(text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)''' || E'\n' ||
        '            ),' || E'\n' ||
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_record_pending_drain_none_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint)''' || E'\n' ||
        '            ),' || E'\n' ||
        '            pg_catalog.to_regprocedure(' || E'\n' ||
        '                ''public.starring_runtime_startup_recovery_execute_pending_drain_v2(text,bigint,bigint,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean,text)''' || E'\n' ||
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
            MESSAGE = 'runtime_startup_pending_drain_readiness_allowlist_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
    EXECUTE definition;
END;
$patch_readiness$;

DO $patch_cross_runtime_manifests$
DECLARE
    definition TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_exact_target_schema_manifest_v1()'
    );
    IF pg_catalog.strpos(
        definition,
        'RETURN observed_count = 356' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''0cf3922bf0d781286003b91b77410aae1d4e02d210534b5a221032fb151346da'';'
    ) = 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_pending_drain_exact_target_manifest_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        'RETURN observed_count = 356' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''0cf3922bf0d781286003b91b77410aae1d4e02d210534b5a221032fb151346da'';',
        'RETURN observed_count = 356' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''6971d3c87da56aecd5c5615a26e8a2d3f2029e4d3e492f2c253fe73c4f8218f2'';'
    );
    EXECUTE definition;

    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_serving_schema_manifest_v1()'
    );
    IF pg_catalog.strpos(
        definition,
        'RETURN observed_count = 471' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''eb21a116efed629be672561b820c9f525594cdd5a7502f7c60cae00b5f9e051e'';'
    ) = 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_pending_drain_serving_manifest_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        'RETURN observed_count = 471' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''eb21a116efed629be672561b820c9f525594cdd5a7502f7c60cae00b5f9e051e'';',
        'RETURN observed_count = 471' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''877f60fa04f60d99e7f41c11baaec89707722578487bbc932aa20a608dc49b22'';'
    );
    EXECUTE definition;

    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_exact_target_database_readiness_v1()'
    );
    IF pg_catalog.strpos(
        definition,
        '''4633e8a3b8dc31d8ddde8d872969b42bdd25a6d98edaf7f59ec3076f3fa4f728''::TEXT'
    ) = 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_pending_drain_exact_target_readiness_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        '''4633e8a3b8dc31d8ddde8d872969b42bdd25a6d98edaf7f59ec3076f3fa4f728''::TEXT',
        '''bea5a930a40537f9f06f19a350d1fdba3bf21b222844eb0f442fb506d91a1ebb''::TEXT'
    );
    EXECUTE definition;

    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_serving_database_readiness_v1()'
    );
    IF pg_catalog.strpos(
        definition,
        '''2c8957777b2d4a7f1b6050b21e8a5664b5fcff45d4732627bc8e961823a4eada''::TEXT'
    ) = 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_pending_drain_serving_readiness_patch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        '''2c8957777b2d4a7f1b6050b21e8a5664b5fcff45d4732627bc8e961823a4eada''::TEXT',
        '''c679ef7c0722416b514324936a95884d17242e6b67cdb130987e4d4f03a43758''::TEXT'
    );
    EXECUTE definition;
END;
$patch_cross_runtime_manifests$;

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
    invalid_column_count BIGINT;
    invalid_alias_count BIGINT;
    manifest_digest TEXT;
    readiness_digest TEXT;
    selector_digest TEXT;
    recorder_digest TEXT;
    executor_digest TEXT;
    product_trigger_digest TEXT;
    slot_validator_digest TEXT;
    deployment_validator_digest TEXT;
    convergence_validator_digest TEXT;
    exact_target_manifest_digest TEXT;
    exact_target_readiness_digest TEXT;
    serving_manifest_digest TEXT;
    serving_readiness_digest TEXT;
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
                'public.starring_runtime_startup_recovery_select_pending_drain_v2(text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)',
                'v'::"char",
                'u'::"char",
                TRUE,
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_startup_recovery_record_pending_drain_none_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint)',
                'v'::"char",
                'u'::"char",
                TRUE,
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_startup_recovery_execute_pending_drain_v2(text,bigint,bigint,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean,text)',
                'v'::"char",
                'u'::"char",
                TRUE,
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'starring_runtime_private_v2.starring_runtime_pending_drain_initial_state_v2(public.runtime_drain_intents_v2)',
                'i'::"char",
                's'::"char",
                FALSE,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'starring_runtime_private_v2.starring_runtime_pending_drain_root_exact_v2(public.runtime_drain_intents_v2)',
                'i'::"char",
                's'::"char",
                FALSE,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(public.runtime_drain_intents_v2)',
                'i'::"char",
                's'::"char",
                FALSE,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'starring_runtime_private_v2.starring_runtime_pending_drain_key_text_v2(public.runtime_drain_intents_v2)',
                'i'::"char",
                's'::"char",
                FALSE,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'starring_runtime_private_v2.starring_runtime_pending_drain_provenance_text_v2(text)',
                'i'::"char",
                's'::"char",
                FALSE,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'starring_runtime_private_v2.starring_runtime_pending_drain_claim_text_v2(public.runtime_drain_intents_v2,jsonb)',
                'i'::"char",
                's'::"char",
                FALSE,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'starring_runtime_private_v2.starring_runtime_pending_drain_projection_v2(smallint,bytea,bytea,bytea,bytea)',
                'i'::"char",
                's'::"char",
                FALSE,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'starring_runtime_private_v2.starring_runtime_pending_drain_replay_exact_v2(bytea,bytea)',
                'i'::"char",
                's'::"char",
                FALSE,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_exact_v2(bytea,smallint,bytea,smallint)',
                'i'::"char",
                's'::"char",
                FALSE,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'starring_runtime_private_v2.starring_runtime_pending_drain_product_root_compound_exact_v2(bytea,text,bigint,bigint,smallint,text,bytea)',
                'i'::"char",
                's'::"char",
                FALSE,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'starring_runtime_private_v2.starring_runtime_pending_drain_initialize_v2()',
                'v'::"char",
                'u'::"char",
                TRUE,
                FALSE,
                FALSE,
                0::REAL
            ),
            (
                'starring_runtime_private_v2.starring_runtime_pending_drain_candidate_v2()',
                'v'::"char",
                'u'::"char",
                FALSE,
                FALSE,
                TRUE,
                1::REAL
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
            ('public.starring_runtime_startup_recovery_select_pending_drain_v2(text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)', TRUE),
            ('public.starring_runtime_startup_recovery_record_pending_drain_none_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint)', TRUE),
            ('public.starring_runtime_startup_recovery_execute_pending_drain_v2(text,bigint,bigint,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean,text)', TRUE),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_initial_state_v2(public.runtime_drain_intents_v2)', FALSE),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_root_exact_v2(public.runtime_drain_intents_v2)', FALSE),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(public.runtime_drain_intents_v2)', FALSE),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_key_text_v2(public.runtime_drain_intents_v2)', FALSE),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_provenance_text_v2(text)', FALSE),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_claim_text_v2(public.runtime_drain_intents_v2,jsonb)', FALSE),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_projection_v2(smallint,bytea,bytea,bytea,bytea)', FALSE),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_replay_exact_v2(bytea,bytea)', FALSE),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_exact_v2(bytea,smallint,bytea,smallint)', FALSE),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_product_root_compound_exact_v2(bytea,text,bigint,bigint,smallint,text,bytea)', FALSE),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_initialize_v2()', FALSE),
            ('starring_runtime_private_v2.starring_runtime_pending_drain_candidate_v2()', FALSE)
    ) AS expected(identity, executor_allowed)
    INNER JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid =
            pg_catalog.to_regprocedure(expected.identity)
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege;
    expected_acl_count := 15 + CASE
        WHEN executor_role IS NULL THEN 0
        ELSE 3
    END;

    SELECT pg_catalog.count(*)
    INTO invalid_column_count
    FROM (
        VALUES
            ('canonical_state_bytes', 'bytea'),
            ('canonical_state_digest', 'text')
    ) AS expected(column_name, data_type)
    LEFT JOIN pg_catalog.pg_attribute AS attribute
        ON attribute.attrelid = pg_catalog.to_regclass(
            'public.runtime_drain_intents_v2'
        )
        AND attribute.attname = expected.column_name
        AND attribute.attnum > 0
        AND NOT attribute.attisdropped
    WHERE attribute.attnum IS NULL
        OR NOT attribute.attnotnull
        OR pg_catalog.format_type(
            attribute.atttypid,
            attribute.atttypmod
        ) <> expected.data_type;

    SELECT pg_catalog.count(*)
    INTO invalid_alias_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname =
            'starring_runtime_startup_recovery_record_pending_drain_no_candi';

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
    INTO selector_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_startup_recovery_select_pending_drain_v2(text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO recorder_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_startup_recovery_record_pending_drain_none_v2(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint)'
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
        'public.starring_runtime_startup_recovery_execute_pending_drain_v2(text,bigint,bigint,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean,text)'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO product_trigger_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.reject_runtime_product_drain_mutation()'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO slot_validator_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.validate_runtime_slot_writer_fence_symmetry_v2()'
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

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR NOT executor_role_is_quarantined
        OR executor_membership_count <> 0
        OR invalid_function_count <> 0
        OR invalid_acl_count <> 0
        OR actual_acl_count <> expected_acl_count
        OR invalid_column_count <> 0
        OR invalid_alias_count <> 0
        OR manifest_digest IS DISTINCT FROM
            '9de93ea5d565254c47533c7af43959aa873014bee385a2af775fafdcbf8118b9'
        OR readiness_digest IS DISTINCT FROM
            '1c20dcc6c6e01b440d9a5813bad12b109d89a67c5d6815f9fd15551fa3c0f4e5'
        OR selector_digest IS DISTINCT FROM
            '480ac807bc917924090375c050705d2dcf51d5234dc9144d1d6b97be4590323d'
        OR recorder_digest IS DISTINCT FROM
            'c0293f15ad24fb56e8a6b4a1a692db8b414f21c3a364999fbae26f1340a13181'
        OR executor_digest IS DISTINCT FROM
            '5414574cde39e1c59410e1cac6ccb975a87d16f4807f3ae33b8f28b8157a8e9b'
        OR product_trigger_digest IS DISTINCT FROM
            '71bae3d64f810dbbe29a670a3d9cedaeb6428a809eb6d8b757e247bdd9c2a046'
        OR slot_validator_digest IS DISTINCT FROM
            '3c6901656c8edb5c8d25347d630e6c821963ca86bd0baed5176a7b2a8f34daa8'
        OR deployment_validator_digest IS DISTINCT FROM
            '4b35baa82ce44c07564593f677da9050d972ed881e1eb7305fbec77a39f14824'
        OR convergence_validator_digest IS DISTINCT FROM
            '6b0ac1c07359f4f2ae1408fe2c5920f5d63df09907e3b5d6108f818fa2a8a685'
        OR exact_target_manifest_digest IS DISTINCT FROM
            'bea5a930a40537f9f06f19a350d1fdba3bf21b222844eb0f442fb506d91a1ebb'
        OR exact_target_readiness_digest IS DISTINCT FROM
            '5eba72a786aebaa8afdc226d661b45132afc5aa053fab7be6a3b9737fdab0e8c'
        OR serving_manifest_digest IS DISTINCT FROM
            'c679ef7c0722416b514324936a95884d17242e6b67cdb130987e4d4f03a43758'
        OR serving_readiness_digest IS DISTINCT FROM
            '80e9f1da2a7b48610e95e2540db4c77a3daed2d53b3a2ec18de37c0767ac5380'
        OR NOT public.starring_runtime_exact_target_schema_manifest_v1()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_startup_pending_drain_execution_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
