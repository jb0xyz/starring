SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
);

LOCK TABLE
    public.runtime_deployments,
    public.runtime_attestations,
    public.runtime_serving_leases,
    public.runtime_writer_fence,
    public.runtime_product_operations_v2,
    public.runtime_drain_intents_v2,
    public.runtime_slot_writer_fences_v2,
    public._sqlx_migrations
IN ACCESS SHARE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    applied_count BIGINT;
    applied_head BIGINT;
    failed_count BIGINT;
    migration_checksum TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    SELECT
        pg_catalog.count(*),
        pg_catalog.max(migration.version),
        pg_catalog.count(*) FILTER (WHERE NOT migration.success)
    INTO applied_count, applied_head, failed_count
    FROM public._sqlx_migrations AS migration;

    SELECT pg_catalog.encode(migration.checksum, 'hex')
    INTO migration_checksum
    FROM public._sqlx_migrations AS migration
    WHERE migration.version = 202608030001
        AND migration.success;

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR applied_count <> 120
        OR applied_head <> 202608030001
        OR failed_count <> 0
        OR migration_checksum
            <> '013761ce2f46111f4d8eead74d521699037eac934b0dab7bfff60eca12451030ff95de2189404e0ddfe7f0f1e3b54fcd'
        OR pg_catalog.to_regprocedure(
            'public.starring_runtime_serving_observe_pending_drain_source_v1(text,bigint,text)'
        ) IS NOT NULL
        OR pg_catalog.to_regprocedure(
            'public.starring_runtime_serving_disconnect_pending_drain_source_if_expired_v1(text,bigint,text,text,text,text,text,text,text,bigint,text,bigint,text,text,text,bigint,bigint,bigint)'
        ) IS NOT NULL
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_serving_schema_manifest_v1()'
                    )
                ),
                'UTF8'
            )),
            'hex'
        ) <> '3a11d73fed6a2bd05e932c27c7e2237d568be66777db14c79a44e84a5816e940'
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_serving_database_readiness_v1()'
                    )
                ),
                'UTF8'
            )),
            'hex'
        ) <> 'e2e2cbbecc245e4c8d96b264d5bf89f1ce01cf4613c86f2d954bbdeeb3d2ad8a'
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_execution_schema_manifest_v1()'
                    )
                ),
                'UTF8'
            )),
            'hex'
        ) <> '99dfc39ef03194161fe67419d87fd2890145980f3147151864ea7552bec36886'
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_source_serving_observation_preflight_drift';
    END IF;
END;
$preflight$;

CREATE FUNCTION public.starring_runtime_serving_observe_pending_drain_source_v1(
    expected_drain_intent_id TEXT,
    expected_source_intent_revision BIGINT,
    expected_source_state_digest TEXT
)
RETURNS TABLE(
    outcome_name TEXT,
    drain_intent_id TEXT,
    source_intent_revision BIGINT,
    source_state_digest TEXT,
    operation_id TEXT,
    tenant_id TEXT,
    installation_id TEXT,
    deployment_id TEXT,
    attestation_digest TEXT,
    process_identity JSONB,
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
    drain_row public.runtime_drain_intents_v2%ROWTYPE;
    product_row public.runtime_product_operations_v2%ROWTYPE;
    fence_row public.runtime_slot_writer_fences_v2%ROWTYPE;
    deployment_row public.runtime_deployments%ROWTYPE;
    attestation_row public.runtime_attestations%ROWTYPE;
    serving_row public.runtime_serving_leases%ROWTYPE;
BEGIN
    IF pg_catalog.current_setting('transaction_isolation')
            <> 'serializable'
        OR pg_catalog.current_setting('transaction_read_only') <> 'off'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_source_serving_observation_transaction_drift';
    END IF;

    IF expected_drain_intent_id !~ '^[0-9a-f]{32}$'
        OR expected_source_intent_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_source_state_digest !~ '^[0-9a-f]{64}$'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS002',
            MESSAGE = 'runtime_pending_drain_source_serving_observation_input_invalid';
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock_shared(
        pg_catalog.hashtextextended(
            'starring-runtime-writer-fence-v1',
            0
        )
    );

    SELECT drain.*
    INTO drain_row
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE drain.drain_intent_id = expected_drain_intent_id;

    IF NOT FOUND
        OR drain_row.intent_state <> 'pending'
        OR drain_row.intent_revision
            IS DISTINCT FROM expected_source_intent_revision
        OR drain_row.canonical_state_digest
            IS DISTINCT FROM expected_source_state_digest
        OR pg_catalog.encode(
            pg_catalog.sha256(drain_row.canonical_state_bytes),
            'hex'
        ) IS DISTINCT FROM expected_source_state_digest
        OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
            drain_row
        )
        OR pg_catalog.convert_from(
            drain_row.canonical_state_bytes,
            'UTF8'
        )::JSONB #>> '{state,kind}'
            IS DISTINCT FROM 'pending_unclaimed'
    THEN
        observed_at := pg_catalog.clock_timestamp();
        outcome_name := 'diverged';
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT product.*
    INTO product_row
    FROM public.runtime_product_operations_v2 AS product
    WHERE product.product_operation_id = drain_row.product_operation_id
        AND product.product_mutation_digest =
            drain_row.product_mutation_digest
        AND product.tenant_id = drain_row.tenant_id
        AND product.installation_id = drain_row.installation_id
        AND product.deployment_id = drain_row.deployment_id
        AND product.expected_revision = drain_row.expected_revision
        AND product.expected_target_guild_id = drain_row.slot_guild_id
        AND product.expected_target_ruleset_key =
            drain_row.slot_ruleset_key;

    SELECT fence.*
    INTO fence_row
    FROM public.runtime_slot_writer_fences_v2 AS fence
    WHERE fence.slot_guild_id = drain_row.slot_guild_id
        AND fence.slot_ruleset_key = drain_row.slot_ruleset_key;

    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = drain_row.tenant_id
        AND deployment.installation_id = drain_row.installation_id
        AND deployment.deployment_id = drain_row.deployment_id;

    IF product_row.product_operation_id IS NULL
        OR fence_row.slot_guild_id IS NULL
        OR deployment_row.deployment_id IS NULL
        OR fence_row.writer_epoch
            NOT BETWEEN 1 AND 9223372036854775807
        OR fence_row.pending_drain_intent_id
            IS DISTINCT FROM drain_row.drain_intent_id
        OR fence_row.pending_product_operation_id
            IS DISTINCT FROM drain_row.product_operation_id
        OR fence_row.pending_tenant_id
            IS DISTINCT FROM drain_row.tenant_id
        OR fence_row.pending_installation_id
            IS DISTINCT FROM drain_row.installation_id
        OR fence_row.pending_deployment_id
            IS DISTINCT FROM drain_row.deployment_id
        OR fence_row.pending_expected_revision
            IS DISTINCT FROM drain_row.expected_revision
        OR fence_row.pending_marked_at IS NULL
        OR NOT pg_catalog.isfinite(fence_row.pending_marked_at)
        OR deployment_row.phase <> 'live'
        OR deployment_row.revision
            IS DISTINCT FROM drain_row.expected_revision
        OR deployment_row.guild_id
            IS DISTINCT FROM product_row.expected_target_guild_id
        OR deployment_row.ruleset_key
            IS DISTINCT FROM product_row.expected_target_ruleset_key
        OR deployment_row.target_version
            IS DISTINCT FROM product_row.expected_target_version
        OR deployment_row.target_content_hash
            IS DISTINCT FROM product_row.expected_target_content_hash
        OR deployment_row.binding_revision
            IS DISTINCT FROM product_row.expected_target_binding_revision
        OR deployment_row.binding_fingerprint
            IS DISTINCT FROM product_row.expected_target_binding_fingerprint
        OR deployment_row.live_attestation_id IS NULL
    THEN
        observed_at := pg_catalog.clock_timestamp();
        outcome_name := 'diverged';
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT attestation.*
    INTO attestation_row
    FROM public.runtime_attestations AS attestation
    WHERE attestation.tenant_id = drain_row.tenant_id
        AND attestation.installation_id = drain_row.installation_id
        AND attestation.deployment_id = drain_row.deployment_id
        AND attestation.attestation_id =
            deployment_row.live_attestation_id;

    IF attestation_row.attestation_id IS NULL
        OR attestation_row.record_format_version <> 2
        OR attestation_row.deployment_revision
            IS DISTINCT FROM deployment_row.revision
        OR attestation_row.guild_id
            IS DISTINCT FROM deployment_row.guild_id
        OR attestation_row.ruleset_key
            IS DISTINCT FROM deployment_row.ruleset_key
        OR attestation_row.target_version
            IS DISTINCT FROM deployment_row.target_version
        OR attestation_row.target_content_hash
            IS DISTINCT FROM deployment_row.target_content_hash
        OR attestation_row.binding_revision
            IS DISTINCT FROM deployment_row.binding_revision
        OR attestation_row.binding_fingerprint
            IS DISTINCT FROM deployment_row.binding_fingerprint
        OR attestation_row.runtime_generation
            IS DISTINCT FROM deployment_row.runtime_generation
        OR attestation_row.v2_operation_id !~ '^[0-9a-f]{32}$'
        OR attestation_row.v2_initial_lease_epoch
            NOT BETWEEN 1 AND 9223372036854775807
    THEN
        observed_at := pg_catalog.clock_timestamp();
        outcome_name := 'diverged';
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT lease.*
    INTO serving_row
    FROM public.runtime_serving_leases AS lease
    WHERE lease.guild_id = drain_row.slot_guild_id
        AND lease.ruleset_key = drain_row.slot_ruleset_key;

    observed_at := pg_catalog.clock_timestamp();
    IF serving_row.guild_id IS NULL THEN
        outcome_name := 'absent';
        drain_intent_id := drain_row.drain_intent_id;
        source_intent_revision := drain_row.intent_revision;
        source_state_digest := drain_row.canonical_state_digest;
        RETURN NEXT;
        RETURN;
    END IF;

    IF serving_row.tenant_id IS DISTINCT FROM drain_row.tenant_id
        OR serving_row.installation_id
            IS DISTINCT FROM drain_row.installation_id
        OR serving_row.deployment_id
            IS DISTINCT FROM drain_row.deployment_id
        OR serving_row.guild_id
            IS DISTINCT FROM drain_row.slot_guild_id
        OR serving_row.ruleset_key
            IS DISTINCT FROM drain_row.slot_ruleset_key
        OR serving_row.attestation_id
            IS DISTINCT FROM attestation_row.attestation_id
        OR serving_row.process_instance_id
            IS DISTINCT FROM attestation_row.process_instance_id
        OR serving_row.runtime_generation
            IS DISTINCT FROM attestation_row.runtime_generation
        OR serving_row.target_version
            IS DISTINCT FROM attestation_row.target_version
        OR serving_row.target_content_hash
            IS DISTINCT FROM attestation_row.target_content_hash
        OR serving_row.binding_revision
            IS DISTINCT FROM attestation_row.binding_revision
        OR serving_row.binding_fingerprint
            IS DISTINCT FROM attestation_row.binding_fingerprint
        OR serving_row.lease_epoch
            IS DISTINCT FROM attestation_row.v2_initial_lease_epoch
        OR serving_row.revision NOT BETWEEN 1 AND 9223372036854775807
        OR serving_row.acquired_at > serving_row.last_heartbeat_at
        OR serving_row.last_heartbeat_at > serving_row.expires_at
        OR serving_row.serving IS DISTINCT FROM serving_row.connected
    THEN
        outcome_name := 'diverged';
        RETURN NEXT;
        RETURN;
    END IF;

    outcome_name := 'current';
    drain_intent_id := drain_row.drain_intent_id;
    source_intent_revision := drain_row.intent_revision;
    source_state_digest := drain_row.canonical_state_digest;
    operation_id := attestation_row.v2_operation_id;
    tenant_id := serving_row.tenant_id;
    installation_id := serving_row.installation_id;
    deployment_id := serving_row.deployment_id;
    attestation_digest := serving_row.attestation_id;
    process_identity := pg_catalog.jsonb_build_object(
        'target',
        pg_catalog.jsonb_build_object(
            'guild_id',
            serving_row.guild_id,
            'ruleset_key',
            serving_row.ruleset_key,
            'version',
            serving_row.target_version,
            'content_hash',
            serving_row.target_content_hash,
            'binding_revision',
            serving_row.binding_revision,
            'binding_fingerprint',
            serving_row.binding_fingerprint
        ),
        'runtime_generation',
        serving_row.runtime_generation,
        'process_instance_id',
        serving_row.process_instance_id
    );
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

CREATE FUNCTION public.starring_runtime_serving_disconnect_pending_drain_source_if_expired_v1(
    expected_drain_intent_id TEXT,
    expected_source_intent_revision BIGINT,
    expected_source_state_digest TEXT,
    expected_operation_id TEXT,
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_guild_id TEXT,
    expected_ruleset_key TEXT,
    expected_target_version BIGINT,
    expected_target_content_hash TEXT,
    expected_binding_revision BIGINT,
    expected_binding_fingerprint TEXT,
    expected_attestation_digest TEXT,
    expected_process_instance_id TEXT,
    expected_runtime_generation BIGINT,
    expected_lease_epoch BIGINT,
    expected_serving_revision BIGINT
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
    drain_row public.runtime_drain_intents_v2%ROWTYPE;
    product_row public.runtime_product_operations_v2%ROWTYPE;
    fence_row public.runtime_slot_writer_fences_v2%ROWTYPE;
    deployment_row public.runtime_deployments%ROWTYPE;
    attestation_row public.runtime_attestations%ROWTYPE;
    serving_row public.runtime_serving_leases%ROWTYPE;
    writer_fence_state TEXT;
    writer_fence_count BIGINT;
    observed_at TIMESTAMPTZ;
    mutation_clock TIMESTAMPTZ;
    next_revision BIGINT;
BEGIN
    IF pg_catalog.current_setting('transaction_isolation')
            <> 'serializable'
        OR pg_catalog.current_setting('transaction_read_only') <> 'off'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_source_serving_disconnect_transaction_drift';
    END IF;

    IF expected_drain_intent_id !~ '^[0-9a-f]{32}$'
        OR expected_source_intent_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_source_state_digest !~ '^[0-9a-f]{64}$'
        OR expected_operation_id !~ '^[0-9a-f]{32}$'
        OR expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_guild_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_guild_id) > 20
        OR (
            pg_catalog.length(expected_guild_id) = 20
            AND expected_guild_id COLLATE pg_catalog."C"
                > '18446744073709551615' COLLATE pg_catalog."C"
        )
        OR expected_ruleset_key !~ '^[A-Za-z0-9_-]{1,64}$'
        OR expected_target_version NOT BETWEEN 1 AND 4294967295
        OR expected_target_content_hash !~ '^[0-9a-f]{64}$'
        OR expected_binding_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_binding_fingerprint !~ '^[0-9a-f]{64}$'
        OR expected_attestation_digest !~ '^[0-9a-f]{64}$'
        OR expected_process_instance_id
            !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_runtime_generation
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_lease_epoch NOT BETWEEN 1 AND 9223372036854775807
        OR expected_serving_revision
            NOT BETWEEN 1 AND 9223372036854775807
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS002',
            MESSAGE = 'runtime_pending_drain_source_serving_disconnect_input_invalid';
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock_shared(
        pg_catalog.hashtextextended(
            'starring-runtime-writer-fence-v1',
            0
        )
    );

    SELECT
        pg_catalog.count(*),
        pg_catalog.min(fence.fence_state)
    INTO writer_fence_count, writer_fence_state
    FROM public.runtime_writer_fence AS fence
    WHERE fence.singleton;

    IF writer_fence_count <> 1
        OR writer_fence_state NOT IN ('open', 'closed')
        OR (
            SELECT pg_catalog.count(*)
            FROM public.runtime_writer_fence
        ) <> 1
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_source_serving_disconnect_writer_fence_drift';
    END IF;

    IF writer_fence_state = 'closed' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS005',
            MESSAGE = 'runtime_pending_drain_source_serving_disconnect_writer_fenced';
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            pg_catalog.concat(
                'starring-runtime-serving-slot-v1:',
                expected_guild_id,
                ':',
                expected_ruleset_key
            ),
            0
        )
    );

    SELECT drain.*
    INTO drain_row
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE drain.drain_intent_id = expected_drain_intent_id;

    IF NOT FOUND
        OR drain_row.intent_state <> 'pending'
        OR drain_row.intent_revision
            IS DISTINCT FROM expected_source_intent_revision
        OR drain_row.canonical_state_digest
            IS DISTINCT FROM expected_source_state_digest
        OR pg_catalog.encode(
            pg_catalog.sha256(drain_row.canonical_state_bytes),
            'hex'
        ) IS DISTINCT FROM expected_source_state_digest
        OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
            drain_row
        )
        OR pg_catalog.convert_from(
            drain_row.canonical_state_bytes,
            'UTF8'
        )::JSONB #>> '{state,kind}'
            IS DISTINCT FROM 'pending_unclaimed'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS001',
            MESSAGE = 'runtime_pending_drain_source_serving_disconnect_source_mismatch';
    END IF;

    SELECT product.*
    INTO product_row
    FROM public.runtime_product_operations_v2 AS product
    WHERE product.product_operation_id = drain_row.product_operation_id
        AND product.product_mutation_digest =
            drain_row.product_mutation_digest
        AND product.tenant_id = drain_row.tenant_id
        AND product.installation_id = drain_row.installation_id
        AND product.deployment_id = drain_row.deployment_id
        AND product.expected_revision = drain_row.expected_revision
        AND product.expected_target_guild_id = drain_row.slot_guild_id
        AND product.expected_target_ruleset_key =
            drain_row.slot_ruleset_key;

    SELECT fence.*
    INTO fence_row
    FROM public.runtime_slot_writer_fences_v2 AS fence
    WHERE fence.slot_guild_id = drain_row.slot_guild_id
        AND fence.slot_ruleset_key = drain_row.slot_ruleset_key;

    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = drain_row.tenant_id
        AND deployment.installation_id = drain_row.installation_id
        AND deployment.deployment_id = drain_row.deployment_id;

    IF product_row.product_operation_id IS NULL
        OR fence_row.slot_guild_id IS NULL
        OR deployment_row.deployment_id IS NULL
        OR drain_row.tenant_id IS DISTINCT FROM expected_tenant_id
        OR drain_row.installation_id
            IS DISTINCT FROM expected_installation_id
        OR drain_row.deployment_id IS DISTINCT FROM expected_deployment_id
        OR drain_row.slot_guild_id IS DISTINCT FROM expected_guild_id
        OR drain_row.slot_ruleset_key IS DISTINCT FROM expected_ruleset_key
        OR product_row.product_operation_id
            IS DISTINCT FROM drain_row.product_operation_id
        OR product_row.expected_target_version
            IS DISTINCT FROM expected_target_version
        OR product_row.expected_target_content_hash
            IS DISTINCT FROM expected_target_content_hash
        OR product_row.expected_target_binding_revision
            IS DISTINCT FROM expected_binding_revision
        OR product_row.expected_target_binding_fingerprint
            IS DISTINCT FROM expected_binding_fingerprint
        OR fence_row.writer_epoch NOT BETWEEN 1 AND 9223372036854775807
        OR fence_row.pending_drain_intent_id
            IS DISTINCT FROM drain_row.drain_intent_id
        OR fence_row.pending_product_operation_id
            IS DISTINCT FROM drain_row.product_operation_id
        OR fence_row.pending_tenant_id
            IS DISTINCT FROM drain_row.tenant_id
        OR fence_row.pending_installation_id
            IS DISTINCT FROM drain_row.installation_id
        OR fence_row.pending_deployment_id
            IS DISTINCT FROM drain_row.deployment_id
        OR fence_row.pending_expected_revision
            IS DISTINCT FROM drain_row.expected_revision
        OR fence_row.pending_marked_at IS NULL
        OR NOT pg_catalog.isfinite(fence_row.pending_marked_at)
        OR deployment_row.phase <> 'live'
        OR deployment_row.revision IS DISTINCT FROM drain_row.expected_revision
        OR deployment_row.guild_id IS DISTINCT FROM expected_guild_id
        OR deployment_row.ruleset_key IS DISTINCT FROM expected_ruleset_key
        OR deployment_row.target_version
            IS DISTINCT FROM expected_target_version
        OR deployment_row.target_content_hash
            IS DISTINCT FROM expected_target_content_hash
        OR deployment_row.binding_revision
            IS DISTINCT FROM expected_binding_revision
        OR deployment_row.binding_fingerprint
            IS DISTINCT FROM expected_binding_fingerprint
        OR deployment_row.live_attestation_id
            IS DISTINCT FROM expected_attestation_digest
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS001',
            MESSAGE = 'runtime_pending_drain_source_serving_disconnect_authority_mismatch';
    END IF;

    SELECT attestation.*
    INTO attestation_row
    FROM public.runtime_attestations AS attestation
    WHERE attestation.tenant_id = expected_tenant_id
        AND attestation.installation_id = expected_installation_id
        AND attestation.deployment_id = expected_deployment_id
        AND attestation.attestation_id = expected_attestation_digest
        AND attestation.v2_operation_id = expected_operation_id
        AND attestation.process_instance_id = expected_process_instance_id
        AND attestation.runtime_generation = expected_runtime_generation
        AND attestation.v2_initial_lease_epoch = expected_lease_epoch;

    IF NOT FOUND
        OR attestation_row.record_format_version <> 2
        OR attestation_row.deployment_revision
            IS DISTINCT FROM deployment_row.revision
        OR attestation_row.guild_id IS DISTINCT FROM expected_guild_id
        OR attestation_row.ruleset_key IS DISTINCT FROM expected_ruleset_key
        OR attestation_row.target_version
            IS DISTINCT FROM expected_target_version
        OR attestation_row.target_content_hash
            IS DISTINCT FROM expected_target_content_hash
        OR attestation_row.binding_revision
            IS DISTINCT FROM expected_binding_revision
        OR attestation_row.binding_fingerprint
            IS DISTINCT FROM expected_binding_fingerprint
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS001',
            MESSAGE = 'runtime_pending_drain_source_serving_disconnect_attestation_mismatch';
    END IF;

    SELECT lease.*
    INTO serving_row
    FROM public.runtime_serving_leases AS lease
    WHERE lease.guild_id = expected_guild_id
        AND lease.ruleset_key = expected_ruleset_key;

    IF NOT FOUND
        OR serving_row.tenant_id IS DISTINCT FROM expected_tenant_id
        OR serving_row.installation_id
            IS DISTINCT FROM expected_installation_id
        OR serving_row.deployment_id IS DISTINCT FROM expected_deployment_id
        OR serving_row.attestation_id
            IS DISTINCT FROM expected_attestation_digest
        OR serving_row.process_instance_id
            IS DISTINCT FROM expected_process_instance_id
        OR serving_row.runtime_generation
            IS DISTINCT FROM expected_runtime_generation
        OR serving_row.lease_epoch IS DISTINCT FROM expected_lease_epoch
        OR serving_row.target_version
            IS DISTINCT FROM expected_target_version
        OR serving_row.target_content_hash
            IS DISTINCT FROM expected_target_content_hash
        OR serving_row.binding_revision
            IS DISTINCT FROM expected_binding_revision
        OR serving_row.binding_fingerprint
            IS DISTINCT FROM expected_binding_fingerprint
        OR serving_row.acquired_at > serving_row.last_heartbeat_at
        OR serving_row.last_heartbeat_at > serving_row.expires_at
        OR serving_row.serving IS DISTINCT FROM serving_row.connected
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS001',
            MESSAGE = 'runtime_pending_drain_source_serving_disconnect_identity_mismatch';
    END IF;

    observed_at := pg_catalog.clock_timestamp();
    IF serving_row.revision = expected_serving_revision THEN
        IF expected_serving_revision = 9223372036854775807
            OR NOT serving_row.connected
            OR NOT serving_row.serving
            OR serving_row.expires_at > observed_at
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RS001',
                MESSAGE = 'runtime_pending_drain_source_serving_disconnect_not_expired';
        END IF;
        mutation_clock := public.starring_runtime_mutation_clock();
        IF serving_row.expires_at > mutation_clock THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RS001',
                MESSAGE = 'runtime_pending_drain_source_serving_disconnect_not_expired';
        END IF;
        next_revision := expected_serving_revision + 1;
        UPDATE public.runtime_serving_leases AS lease
        SET revision = next_revision,
            connected = FALSE,
            serving = FALSE,
            last_heartbeat_at = mutation_clock,
            expires_at = mutation_clock
        WHERE lease.guild_id = expected_guild_id
            AND lease.ruleset_key = expected_ruleset_key
            AND lease.tenant_id = expected_tenant_id
            AND lease.installation_id = expected_installation_id
            AND lease.deployment_id = expected_deployment_id
            AND lease.attestation_id = expected_attestation_digest
            AND lease.process_instance_id = expected_process_instance_id
            AND lease.runtime_generation = expected_runtime_generation
            AND lease.lease_epoch = expected_lease_epoch
            AND lease.target_version = expected_target_version
            AND lease.target_content_hash = expected_target_content_hash
            AND lease.binding_revision = expected_binding_revision
            AND lease.binding_fingerprint = expected_binding_fingerprint
            AND lease.revision = expected_serving_revision
            AND lease.connected
            AND lease.serving
            AND lease.expires_at <= mutation_clock
        RETURNING lease.* INTO serving_row;
        IF NOT FOUND THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RS001',
                MESSAGE = 'runtime_pending_drain_source_serving_disconnect_revision_conflict';
        END IF;
    ELSIF expected_serving_revision < 9223372036854775807
        AND serving_row.revision = expected_serving_revision + 1
    THEN
        IF serving_row.connected
            OR serving_row.serving
            OR serving_row.last_heartbeat_at
                IS DISTINCT FROM serving_row.expires_at
            OR serving_row.expires_at > observed_at
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RS001',
                MESSAGE = 'runtime_pending_drain_source_serving_disconnect_replay_mismatch';
        END IF;
    ELSE
        RAISE EXCEPTION USING
            ERRCODE = 'RS001',
            MESSAGE = 'runtime_pending_drain_source_serving_disconnect_revision_conflict';
    END IF;

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

DO $capability_acl$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    serving_role OID;
    serving_role_count BIGINT;
    serving_role_name NAME;
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
        AND privilege.grantee NOT IN (0, common_owner)
        AND privilege.privilege_type = 'EXECUTE'
        AND NOT privilege.is_grantable;

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    serving_role_name := pg_catalog.pg_get_userbyid(serving_role);
    IF common_owner IS NULL
        OR common_owner_name IS NULL
        OR serving_role_count > 1
        OR (
            serving_role_count = 1
            AND (
                serving_role IS NULL
                OR serving_role_name IS NULL
            )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_source_serving_observation_acl_drift';
    END IF;

    FOREACH function_identity IN ARRAY ARRAY[
        'public.starring_runtime_serving_observe_pending_drain_source_v1(TEXT,BIGINT,TEXT)',
        'public.starring_runtime_serving_disconnect_pending_drain_source_if_expired_v1(TEXT,BIGINT,TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,TEXT,BIGINT,TEXT,TEXT,TEXT,BIGINT,BIGINT,BIGINT)'
    ]::TEXT[]
    LOOP
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s OWNER TO %I',
            function_identity,
            common_owner_name
        );
        EXECUTE pg_catalog.format(
            'REVOKE ALL ON FUNCTION %s FROM PUBLIC',
            function_identity
        );
        IF serving_role_count = 1 THEN
            EXECUTE pg_catalog.format(
                'GRANT EXECUTE ON FUNCTION %s TO %I',
                function_identity,
                serving_role_name
            );
        END IF;
    END LOOP;
END;
$capability_acl$;

DO $patch_manifest$
DECLARE
    definition TEXT;
    marker TEXT :=
        E'    ), permitted_external_index(index_oid) AS (';
    previous_result TEXT :=
        E'    RETURN observed_count = 492\n        AND observed_digest\n            = ''11d0780fcc13729aa018acf80b8741c3eb3136f8b68ca42f4b600303389b1eab'';\n';
    next_result TEXT :=
        E'    RETURN observed_count = 494\n        AND observed_digest\n            = ''a4d366aad6c6e320697b90f4e294ca6dfff9cceb1a1935e8d12f0608614eda02'';\n';
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_serving_schema_manifest_v1()'
    );

    IF definition IS NULL
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(definition, 'UTF8')),
            'hex'
        ) <> '3a11d73fed6a2bd05e932c27c7e2237d568be66777db14c79a44e84a5816e940'
        OR pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                marker,
                ''
            )) <> pg_catalog.char_length(marker)
        OR pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                previous_result,
                ''
            )) <> pg_catalog.char_length(previous_result)
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_source_serving_manifest_patch_drift';
    END IF;

    definition := pg_catalog.replace(
        definition,
        marker,
        E'        UNION\n'
            || E'        SELECT pg_catalog.to_regprocedure(\n'
            || E'            ''public.starring_runtime_serving_observe_pending_drain_source_v1(text,bigint,text)''\n'
            || E'        )\n'
            || E'        UNION\n'
            || E'        SELECT pg_catalog.to_regprocedure(\n'
            || E'            ''public.starring_runtime_serving_disconnect_pending_drain_source_if_expired_v1(text,bigint,text,text,text,text,text,text,text,bigint,text,bigint,text,text,text,bigint,bigint,bigint)''\n'
            || E'        )\n'
            || marker
    );
    definition := pg_catalog.replace(
        definition,
        previous_result,
        next_result
    );
    EXECUTE definition;
    IF NOT public.starring_runtime_serving_schema_manifest_v1()
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_serving_schema_manifest_v1()'
                    )
                ),
                'UTF8'
            )),
            'hex'
        ) <> '90ab51452bf5c3ba8074e0bce0f6a643ba374e79497962d0bf2d5aeec062fa96'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_source_serving_manifest_postcondition_drift';
    END IF;
END;
$patch_manifest$;

DO $patch_readiness$
DECLARE
    definition TEXT;
    function_identities TEXT[] := ARRAY[
        'public.starring_runtime_serving_observe_pending_drain_source_v1(text,bigint,text)',
        'public.starring_runtime_serving_disconnect_pending_drain_source_if_expired_v1(text,bigint,text,text,text,text,text,text,text,bigint,text,bigint,text,text,text,bigint,bigint,bigint)'
    ]::TEXT[];
    function_identity TEXT;
    function_arguments TEXT;
    function_result TEXT;
    contract_marker TEXT :=
        E'    ) AS expected(identity, arguments, result, language_name, returns_set, rows_estimate)';
    allowlist_marker TEXT :=
        E'        )\n        AND namespace.nspname NOT IN (''pg_catalog'', ''information_schema'')';
    schema_marker TEXT :=
        E'        OR NOT public.starring_runtime_serving_schema_manifest_v1()\n';
    support_previous TEXT :=
        E'            (\n                ''public.starring_runtime_serving_schema_manifest_v1()'',\n                ''''::TEXT,\n                ''boolean''::TEXT,\n                ''plpgsql''::TEXT,\n                TRUE,\n                ''3a11d73fed6a2bd05e932c27c7e2237d568be66777db14c79a44e84a5816e940''::TEXT\n            )';
    support_next TEXT :=
        E'            (\n                ''public.starring_runtime_serving_schema_manifest_v1()'',\n                ''''::TEXT,\n                ''boolean''::TEXT,\n                ''plpgsql''::TEXT,\n                TRUE,\n                ''90ab51452bf5c3ba8074e0bce0f6a643ba374e79497962d0bf2d5aeec062fa96''::TEXT\n            ),\n            (\n                ''public.starring_runtime_execution_schema_manifest_v1()'',\n                ''''::TEXT,\n                ''boolean''::TEXT,\n                ''plpgsql''::TEXT,\n                TRUE,\n                ''99dfc39ef03194161fe67419d87fd2890145980f3147151864ea7552bec36886''::TEXT\n            )';
    contract_rows TEXT := '';
    allowlist_rows TEXT := '';
BEGIN
    FOREACH function_identity IN ARRAY function_identities
    LOOP
        SELECT
            pg_catalog.pg_get_function_identity_arguments(function_row.oid),
            pg_catalog.pg_get_function_result(function_row.oid)
        INTO function_arguments, function_result
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid = pg_catalog.to_regprocedure(
            function_identity
        );

        IF function_arguments IS NULL OR function_result IS NULL THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_pending_drain_source_serving_readiness_contract_drift';
        END IF;

        contract_rows := contract_rows || pg_catalog.format(
            E',\n            (\n                %L,\n                %L::TEXT,\n                %L::TEXT,\n                ''plpgsql''::TEXT,\n                TRUE,\n                1::REAL\n            )',
            function_identity,
            function_arguments,
            function_result
        );
        allowlist_rows := allowlist_rows || pg_catalog.format(
            E',\n            pg_catalog.to_regprocedure(\n                %L\n            )',
            function_identity
        );
    END LOOP;

    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_serving_database_readiness_v1()'
    );

    IF definition IS NULL
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(definition, 'UTF8')),
            'hex'
        ) <> 'e2e2cbbecc245e4c8d96b264d5bf89f1ce01cf4613c86f2d954bbdeeb3d2ad8a'
        OR pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                contract_marker,
                ''
            )) <> pg_catalog.char_length(contract_marker)
        OR pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                allowlist_marker,
                ''
            )) <> pg_catalog.char_length(allowlist_marker)
        OR pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                schema_marker,
                ''
            )) <> pg_catalog.char_length(schema_marker)
        OR pg_catalog.char_length(definition)
            - pg_catalog.char_length(pg_catalog.replace(
                definition,
                support_previous,
                ''
            )) <> pg_catalog.char_length(support_previous)
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_source_serving_readiness_patch_drift';
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
        schema_marker,
        schema_marker
            || E'        OR NOT public.starring_runtime_execution_schema_manifest_v1()\n'
    );
    definition := pg_catalog.replace(
        definition,
        support_previous,
        support_next
    );
    EXECUTE definition;
END;
$patch_readiness$;

DO $postflight$
DECLARE
    common_owner OID;
    observer_function_oid OID;
    disconnect_function_oid OID;
    manifest_digest TEXT;
    readiness_digest TEXT;
    invalid_function_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    observer_function_oid := pg_catalog.to_regprocedure(
        'public.starring_runtime_serving_observe_pending_drain_source_v1(text,bigint,text)'
    );
    disconnect_function_oid := pg_catalog.to_regprocedure(
        'public.starring_runtime_serving_disconnect_pending_drain_source_if_expired_v1(text,bigint,text,text,text,text,text,text,text,bigint,text,bigint,text,text,text,bigint,bigint,bigint)'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_serving_schema_manifest_v1()'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO manifest_digest;

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_serving_database_readiness_v1()'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO readiness_digest;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_runtime_serving_observe_pending_drain_source_v1(text,bigint,text)',
                'expected_drain_intent_id text, expected_source_intent_revision bigint, expected_source_state_digest text'::TEXT,
                'TABLE(outcome_name text, drain_intent_id text, source_intent_revision bigint, source_state_digest text, operation_id text, tenant_id text, installation_id text, deployment_id text, attestation_digest text, process_identity jsonb, lease_epoch bigint, serving_revision bigint, acquired_at timestamp with time zone, last_heartbeat_at timestamp with time zone, expires_at timestamp with time zone, connected boolean, serving boolean, observed_at timestamp with time zone)'::TEXT
            ),
            (
                'public.starring_runtime_serving_disconnect_pending_drain_source_if_expired_v1(text,bigint,text,text,text,text,text,text,text,bigint,text,bigint,text,text,text,bigint,bigint,bigint)',
                'expected_drain_intent_id text, expected_source_intent_revision bigint, expected_source_state_digest text, expected_operation_id text, expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_guild_id text, expected_ruleset_key text, expected_target_version bigint, expected_target_content_hash text, expected_binding_revision bigint, expected_binding_fingerprint text, expected_attestation_digest text, expected_process_instance_id text, expected_runtime_generation bigint, expected_lease_epoch bigint, expected_serving_revision bigint'::TEXT,
                'TABLE(operation_id text, tenant_id text, installation_id text, deployment_id text, guild_id text, ruleset_key text, target_version bigint, target_content_hash text, binding_revision bigint, binding_fingerprint text, attestation_digest text, process_instance_id text, runtime_generation bigint, lease_epoch bigint, serving_revision bigint, acquired_at timestamp with time zone, last_heartbeat_at timestamp with time zone, expires_at timestamp with time zone, connected boolean, serving boolean)'::TEXT
            )
    ) AS expected(identity, arguments, result)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
            OR function_row.prokind <> 'f'
            OR function_row.provolatile <> 'v'
            OR NOT function_row.proisstrict
            OR function_row.proparallel <> 'u'
            OR NOT function_row.prosecdef
            OR NOT function_row.proretset
            OR function_row.prorows <> 1::REAL
            OR function_row.proconfig IS DISTINCT FROM
                ARRAY[
                    'search_path=pg_catalog'
                ]::TEXT[]
            OR function_row.proleakproof
            OR function_row.pronargdefaults <> 0
            OR function_row.provariadic <> 0
            OR language_row.lanname IS DISTINCT FROM 'plpgsql'
            OR pg_catalog.pg_get_function_identity_arguments(
                function_row.oid
            ) IS DISTINCT FROM expected.arguments
            OR pg_catalog.pg_get_function_result(function_row.oid)
                IS DISTINCT FROM expected.result
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE privilege.grantee = 0
                    OR privilege.grantor <> common_owner
                    OR privilege.privilege_type <> 'EXECUTE'
                    OR privilege.is_grantable
            )
        ;

    IF common_owner IS NULL
        OR observer_function_oid IS NULL
        OR disconnect_function_oid IS NULL
        OR invalid_function_count <> 0
        OR manifest_digest
            <> '90ab51452bf5c3ba8074e0bce0f6a643ba374e79497962d0bf2d5aeec062fa96'
        OR readiness_digest
            <> '918e4be248c37e622b1f5b22cb9e252a450b65b295157681c647855d0c0150b9'
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_pending_drain_source_serving_observation_postflight_drift';
    END IF;
END;
$postflight$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
