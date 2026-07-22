SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

LOCK TABLE
    public.product_control_plane_identity,
    public.runtime_deployments,
    public.runtime_attestations,
    public.runtime_serving_leases,
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
    common_owner_name NAME;
    relation_count BIGINT;
    ordinary_count BIGINT;
    owner_count BIGINT;
    collision_count BIGINT;
    unsafe_schema_create_count BIGINT;
BEGIN
    SELECT pg_catalog.count(relation.oid),
        pg_catalog.count(relation.oid) FILTER (WHERE relation.relkind = 'r'),
        pg_catalog.count(DISTINCT relation.relowner),
        pg_catalog.min(relation.relowner::BIGINT)::OID
    INTO relation_count, ordinary_count, owner_count, common_owner
    FROM (
        VALUES
            (pg_catalog.to_regclass('public.product_control_plane_identity')),
            (pg_catalog.to_regclass('public.runtime_deployments')),
            (pg_catalog.to_regclass('public.runtime_attestations')),
            (pg_catalog.to_regclass('public.runtime_serving_leases')),
            (pg_catalog.to_regclass('public.activation_requests')),
            (pg_catalog.to_regclass('public.authoring_promotions')),
            (pg_catalog.to_regclass('public.product_tenants')),
            (pg_catalog.to_regclass('public.automation_installations')),
            (pg_catalog.to_regclass('public.automation_installation_authority_versions')),
            (pg_catalog.to_regclass('public.automation_ruleset_activations')),
            (pg_catalog.to_regclass('public.automation_ruleset_versions'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid;

    IF relation_count <> 11
        OR ordinary_count <> 11
        OR owner_count <> 1
        OR common_owner IS NULL
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_serving_database_relation_drift';
    END IF;

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR NOT pg_catalog.has_schema_privilege(common_owner_name, 'public', 'CREATE')
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_serving_database_owner_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO unsafe_schema_create_count
    FROM pg_catalog.pg_namespace AS namespace
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        namespace.nspacl,
        pg_catalog.acldefault('n', namespace.nspowner)
    )) AS privilege
    WHERE namespace.nspname = 'public'
        AND privilege.privilege_type = 'CREATE'
        AND privilege.grantee <> namespace.nspowner;

    IF unsafe_schema_create_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_serving_database_schema_authority_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_runtime_serving_schema_manifest_v1',
            'starring_runtime_serving_database_readiness_v1',
            'starring_runtime_serving_database_identity_v1',
            'starring_runtime_serving_heartbeat_v1',
            'starring_runtime_serving_disconnect_v1'
        );

    IF collision_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_serving_database_function_drift';
    END IF;
END;
$preflight$;

CREATE FUNCTION public.starring_runtime_serving_database_identity_v1()
RETURNS TEXT
LANGUAGE sql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
    SELECT identity.database_identity::TEXT
    FROM public.product_control_plane_identity AS identity
    WHERE identity.singleton
        AND identity.database_identity IS NOT NULL
        AND identity.database_identity
            <> '00000000-0000-0000-0000-000000000000'::UUID
        AND identity.created_at IS NOT NULL;
$function$;

CREATE FUNCTION public.starring_runtime_serving_heartbeat_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_attestation_id TEXT,
    expected_process_instance_id TEXT,
    expected_runtime_generation BIGINT,
    expected_lease_epoch BIGINT,
    expected_revision BIGINT,
    requested_lease_milliseconds BIGINT
)
RETURNS TABLE(
    tenant_id TEXT,
    installation_id TEXT,
    deployment_id TEXT,
    guild_id TEXT,
    ruleset_key TEXT,
    attestation_id TEXT,
    process_instance_id TEXT,
    runtime_generation BIGINT,
    lease_epoch BIGINT,
    revision BIGINT,
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
    deployment_row public.runtime_deployments%ROWTYPE;
    serving_row public.runtime_serving_leases%ROWTYPE;
    authority_outcome TEXT;
    canonical_artifact BOOLEAN;
    mutation_clock TIMESTAMPTZ;
    requested_duration INTERVAL;
    next_revision BIGINT;
    next_expiry TIMESTAMPTZ;
BEGIN
    IF expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_attestation_id !~ '^[0-9a-f]{64}$'
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_runtime_generation NOT BETWEEN 1 AND 9223372036854775807
        OR expected_lease_epoch NOT BETWEEN 1 AND 9223372036854775807
        OR expected_revision NOT BETWEEN 1 AND 9223372036854775807
        OR requested_lease_milliseconds NOT BETWEEN 1000 AND 300000
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS002',
            MESSAGE = 'runtime_serving_heartbeat_input_invalid';
    END IF;

    requested_duration := requested_lease_milliseconds * INTERVAL '1 millisecond';

    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = expected_tenant_id
        AND deployment.installation_id = expected_installation_id
        AND deployment.deployment_id = expected_deployment_id
    FOR UPDATE;

    IF NOT FOUND
        OR deployment_row.guild_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(deployment_row.guild_id) > 20
        OR (
            pg_catalog.length(deployment_row.guild_id) = 20
            AND deployment_row.guild_id > '18446744073709551615'
        )
        OR deployment_row.ruleset_key !~ '^[A-Za-z0-9_-]{1,64}$'
        OR deployment_row.target_version NOT BETWEEN 1 AND 4294967295
        OR deployment_row.target_content_hash !~ '^[0-9a-f]{64}$'
        OR deployment_row.binding_revision NOT BETWEEN 1 AND 9223372036854775807
        OR deployment_row.binding_fingerprint !~ '^[0-9a-f]{64}$'
        OR deployment_row.installation_authority_revision
            NOT BETWEEN 1 AND 9223372036854775807
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS001',
            MESSAGE = 'runtime_serving_heartbeat_ownership_lost';
    END IF;

    SELECT lease.*
    INTO serving_row
    FROM public.runtime_serving_leases AS lease
    WHERE lease.guild_id = deployment_row.guild_id
        AND lease.ruleset_key = deployment_row.ruleset_key;

    IF FOUND
        AND expected_revision < 9223372036854775807
        AND serving_row.revision = expected_revision + 1
    THEN
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
        IF NOT FOUND
            OR serving_row.tenant_id IS DISTINCT FROM expected_tenant_id
            OR serving_row.installation_id
                IS DISTINCT FROM expected_installation_id
            OR serving_row.deployment_id IS DISTINCT FROM expected_deployment_id
            OR serving_row.attestation_id IS DISTINCT FROM expected_attestation_id
            OR serving_row.process_instance_id
                IS DISTINCT FROM expected_process_instance_id
            OR serving_row.runtime_generation
                IS DISTINCT FROM expected_runtime_generation
            OR serving_row.lease_epoch IS DISTINCT FROM expected_lease_epoch
            OR serving_row.revision <> expected_revision + 1
            OR serving_row.guild_id IS DISTINCT FROM deployment_row.guild_id
            OR serving_row.ruleset_key IS DISTINCT FROM deployment_row.ruleset_key
            OR serving_row.target_version
                IS DISTINCT FROM deployment_row.target_version
            OR serving_row.target_content_hash
                IS DISTINCT FROM deployment_row.target_content_hash
            OR serving_row.binding_revision
                IS DISTINCT FROM deployment_row.binding_revision
            OR serving_row.binding_fingerprint
                IS DISTINCT FROM deployment_row.binding_fingerprint
            OR NOT serving_row.connected
            OR NOT serving_row.serving
            OR serving_row.acquired_at > serving_row.last_heartbeat_at
            OR serving_row.last_heartbeat_at > serving_row.expires_at
            OR serving_row.expires_at - serving_row.last_heartbeat_at
                IS DISTINCT FROM requested_duration
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RS001',
                MESSAGE = 'runtime_serving_heartbeat_replay_mismatch';
        END IF;

        tenant_id := serving_row.tenant_id;
        installation_id := serving_row.installation_id;
        deployment_id := serving_row.deployment_id;
        guild_id := serving_row.guild_id;
        ruleset_key := serving_row.ruleset_key;
        attestation_id := serving_row.attestation_id;
        process_instance_id := serving_row.process_instance_id;
        runtime_generation := serving_row.runtime_generation;
        lease_epoch := serving_row.lease_epoch;
        revision := serving_row.revision;
        acquired_at := serving_row.acquired_at;
        last_heartbeat_at := serving_row.last_heartbeat_at;
        expires_at := serving_row.expires_at;
        connected := serving_row.connected;
        serving := serving_row.serving;
        RETURN NEXT;
        RETURN;
    END IF;

    IF deployment_row.phase <> 'live'
        OR deployment_row.live_attestation_id
            IS DISTINCT FROM expected_attestation_id
        OR deployment_row.runtime_generation
            IS DISTINCT FROM expected_runtime_generation
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS001',
            MESSAGE = 'runtime_serving_heartbeat_ownership_lost';
    END IF;

    SELECT public.starring_runtime_lock_current_authority(
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
    )
    INTO authority_outcome;

    IF authority_outcome IS DISTINCT FROM 'exact' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS003',
            MESSAGE = 'runtime_serving_heartbeat_authority_changed';
    END IF;

    SELECT version.content_hash = deployment_row.target_content_hash
        AND version.canonical_content_hash = deployment_row.target_content_hash
        AND version.schema_version = 1
    INTO canonical_artifact
    FROM public.automation_ruleset_versions AS version
    WHERE version.guild_id = deployment_row.guild_id
        AND version.ruleset_key = deployment_row.ruleset_key
        AND version.version = deployment_row.target_version
    FOR SHARE;

    IF canonical_artifact IS DISTINCT FROM TRUE THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS004',
            MESSAGE = 'runtime_serving_heartbeat_artifact_invalid';
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

    IF NOT FOUND
        OR serving_row.tenant_id IS DISTINCT FROM expected_tenant_id
        OR serving_row.installation_id IS DISTINCT FROM expected_installation_id
        OR serving_row.deployment_id IS DISTINCT FROM expected_deployment_id
        OR serving_row.attestation_id IS DISTINCT FROM expected_attestation_id
        OR serving_row.process_instance_id
            IS DISTINCT FROM expected_process_instance_id
        OR serving_row.runtime_generation
            IS DISTINCT FROM expected_runtime_generation
        OR serving_row.lease_epoch IS DISTINCT FROM expected_lease_epoch
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS001',
            MESSAGE = 'runtime_serving_heartbeat_identity_mismatch';
    END IF;

    IF serving_row.guild_id IS DISTINCT FROM deployment_row.guild_id
        OR serving_row.ruleset_key IS DISTINCT FROM deployment_row.ruleset_key
        OR serving_row.target_version IS DISTINCT FROM deployment_row.target_version
        OR serving_row.target_content_hash
            IS DISTINCT FROM deployment_row.target_content_hash
        OR serving_row.binding_revision
            IS DISTINCT FROM deployment_row.binding_revision
        OR serving_row.binding_fingerprint
            IS DISTINCT FROM deployment_row.binding_fingerprint
        OR serving_row.acquired_at > serving_row.last_heartbeat_at
        OR serving_row.last_heartbeat_at > serving_row.expires_at
        OR serving_row.serving IS DISTINCT FROM serving_row.connected
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS004',
            MESSAGE = 'runtime_serving_heartbeat_state_invalid';
    END IF;

    IF expected_revision < 9223372036854775807
        AND serving_row.revision = expected_revision + 1
    THEN
        IF NOT serving_row.connected
            OR NOT serving_row.serving
            OR serving_row.expires_at - serving_row.last_heartbeat_at
                IS DISTINCT FROM requested_duration
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RS001',
                MESSAGE = 'runtime_serving_heartbeat_replay_mismatch';
        END IF;
    ELSIF serving_row.revision IS DISTINCT FROM expected_revision THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS001',
            MESSAGE = 'runtime_serving_heartbeat_revision_conflict';
    ELSE
        mutation_clock := public.starring_runtime_mutation_clock();
        IF NOT serving_row.connected
            OR NOT serving_row.serving
            OR serving_row.expires_at <= mutation_clock
            OR expected_revision = 9223372036854775807
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RS001',
                MESSAGE = 'runtime_serving_heartbeat_lease_inactive';
        END IF;
        next_revision := expected_revision + 1;
        next_expiry := mutation_clock + requested_duration;
        IF next_expiry < serving_row.expires_at THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RS001',
                MESSAGE = 'runtime_serving_heartbeat_expiry_regression';
        END IF;
        UPDATE public.runtime_serving_leases AS lease
        SET revision = next_revision,
            connected = TRUE,
            serving = TRUE,
            last_heartbeat_at = mutation_clock,
            expires_at = next_expiry
        WHERE lease.guild_id = deployment_row.guild_id
            AND lease.ruleset_key = deployment_row.ruleset_key
            AND lease.revision = expected_revision
        RETURNING lease.* INTO serving_row;
        IF NOT FOUND THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RS001',
                MESSAGE = 'runtime_serving_heartbeat_revision_conflict';
        END IF;
    END IF;

    tenant_id := serving_row.tenant_id;
    installation_id := serving_row.installation_id;
    deployment_id := serving_row.deployment_id;
    guild_id := serving_row.guild_id;
    ruleset_key := serving_row.ruleset_key;
    attestation_id := serving_row.attestation_id;
    process_instance_id := serving_row.process_instance_id;
    runtime_generation := serving_row.runtime_generation;
    lease_epoch := serving_row.lease_epoch;
    revision := serving_row.revision;
    acquired_at := serving_row.acquired_at;
    last_heartbeat_at := serving_row.last_heartbeat_at;
    expires_at := serving_row.expires_at;
    connected := serving_row.connected;
    serving := serving_row.serving;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_serving_disconnect_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_attestation_id TEXT,
    expected_process_instance_id TEXT,
    expected_runtime_generation BIGINT,
    expected_lease_epoch BIGINT,
    expected_revision BIGINT
)
RETURNS TABLE(
    tenant_id TEXT,
    installation_id TEXT,
    deployment_id TEXT,
    guild_id TEXT,
    ruleset_key TEXT,
    attestation_id TEXT,
    process_instance_id TEXT,
    runtime_generation BIGINT,
    lease_epoch BIGINT,
    revision BIGINT,
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
    deployment_row public.runtime_deployments%ROWTYPE;
    serving_row public.runtime_serving_leases%ROWTYPE;
    mutation_clock TIMESTAMPTZ;
    next_revision BIGINT;
BEGIN
    IF expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_attestation_id !~ '^[0-9a-f]{64}$'
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_runtime_generation NOT BETWEEN 1 AND 9223372036854775807
        OR expected_lease_epoch NOT BETWEEN 1 AND 9223372036854775807
        OR expected_revision NOT BETWEEN 1 AND 9223372036854775807
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS002',
            MESSAGE = 'runtime_serving_disconnect_input_invalid';
    END IF;

    SELECT deployment.*
    INTO deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = expected_tenant_id
        AND deployment.installation_id = expected_installation_id
        AND deployment.deployment_id = expected_deployment_id
    FOR UPDATE;

    IF NOT FOUND
        OR deployment_row.guild_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(deployment_row.guild_id) > 20
        OR (
            pg_catalog.length(deployment_row.guild_id) = 20
            AND deployment_row.guild_id > '18446744073709551615'
        )
        OR deployment_row.ruleset_key !~ '^[A-Za-z0-9_-]{1,64}$'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS001',
            MESSAGE = 'runtime_serving_disconnect_ownership_lost';
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

    IF NOT FOUND
        OR serving_row.tenant_id IS DISTINCT FROM expected_tenant_id
        OR serving_row.installation_id IS DISTINCT FROM expected_installation_id
        OR serving_row.deployment_id IS DISTINCT FROM expected_deployment_id
        OR serving_row.attestation_id IS DISTINCT FROM expected_attestation_id
        OR serving_row.process_instance_id
            IS DISTINCT FROM expected_process_instance_id
        OR serving_row.runtime_generation
            IS DISTINCT FROM expected_runtime_generation
        OR serving_row.lease_epoch IS DISTINCT FROM expected_lease_epoch
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS001',
            MESSAGE = 'runtime_serving_disconnect_identity_mismatch';
    END IF;

    IF serving_row.guild_id IS DISTINCT FROM deployment_row.guild_id
        OR serving_row.ruleset_key IS DISTINCT FROM deployment_row.ruleset_key
        OR serving_row.target_version IS DISTINCT FROM deployment_row.target_version
        OR serving_row.target_content_hash
            IS DISTINCT FROM deployment_row.target_content_hash
        OR serving_row.binding_revision
            IS DISTINCT FROM deployment_row.binding_revision
        OR serving_row.binding_fingerprint
            IS DISTINCT FROM deployment_row.binding_fingerprint
        OR serving_row.acquired_at > serving_row.last_heartbeat_at
        OR serving_row.last_heartbeat_at > serving_row.expires_at
        OR serving_row.serving IS DISTINCT FROM serving_row.connected
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS004',
            MESSAGE = 'runtime_serving_disconnect_state_invalid';
    END IF;

    IF expected_revision < 9223372036854775807
        AND serving_row.revision = expected_revision + 1
    THEN
        IF serving_row.connected
            OR serving_row.serving
            OR serving_row.last_heartbeat_at IS DISTINCT FROM serving_row.expires_at
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RS001',
                MESSAGE = 'runtime_serving_disconnect_replay_mismatch';
        END IF;
    ELSIF serving_row.revision IS DISTINCT FROM expected_revision
        OR NOT serving_row.connected
        OR NOT serving_row.serving
        OR expected_revision = 9223372036854775807
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RS001',
            MESSAGE = 'runtime_serving_disconnect_revision_conflict';
    ELSE
        mutation_clock := public.starring_runtime_mutation_clock();
        next_revision := expected_revision + 1;
        UPDATE public.runtime_serving_leases AS lease
        SET revision = next_revision,
            connected = FALSE,
            serving = FALSE,
            last_heartbeat_at = mutation_clock,
            expires_at = mutation_clock
        WHERE lease.guild_id = deployment_row.guild_id
            AND lease.ruleset_key = deployment_row.ruleset_key
            AND lease.revision = expected_revision
        RETURNING lease.* INTO serving_row;
        IF NOT FOUND THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RS001',
                MESSAGE = 'runtime_serving_disconnect_revision_conflict';
        END IF;
    END IF;

    tenant_id := serving_row.tenant_id;
    installation_id := serving_row.installation_id;
    deployment_id := serving_row.deployment_id;
    guild_id := serving_row.guild_id;
    ruleset_key := serving_row.ruleset_key;
    attestation_id := serving_row.attestation_id;
    process_instance_id := serving_row.process_instance_id;
    runtime_generation := serving_row.runtime_generation;
    lease_epoch := serving_row.lease_epoch;
    revision := serving_row.revision;
    acquired_at := serving_row.acquired_at;
    last_heartbeat_at := serving_row.last_heartbeat_at;
    expires_at := serving_row.expires_at;
    connected := serving_row.connected;
    serving := serving_row.serving;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_serving_schema_manifest_v1()
RETURNS BOOLEAN
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    observed_count BIGINT;
    observed_digest TEXT;
BEGIN
    WITH protected(relation_oid) AS (
        VALUES
            (pg_catalog.to_regclass('public.product_control_plane_identity')),
            (pg_catalog.to_regclass('public.runtime_deployments')),
            (pg_catalog.to_regclass('public.runtime_attestations')),
            (pg_catalog.to_regclass('public.runtime_serving_leases')),
            (pg_catalog.to_regclass('public.activation_requests')),
            (pg_catalog.to_regclass('public.authoring_promotions')),
            (pg_catalog.to_regclass('public.product_tenants')),
            (pg_catalog.to_regclass('public.automation_installations')),
            (pg_catalog.to_regclass('public.automation_installation_authority_versions')),
            (pg_catalog.to_regclass('public.automation_ruleset_activations')),
            (pg_catalog.to_regclass('public.automation_ruleset_versions'))
    ), protected_function(function_oid) AS (
        SELECT pg_catalog.to_regprocedure(
            'public.starring_runtime_serving_database_identity_v1()'
        )
        UNION
        SELECT pg_catalog.to_regprocedure(
            'public.starring_runtime_serving_heartbeat_v1(text,text,text,text,text,bigint,bigint,bigint,bigint)'
        )
        UNION
        SELECT pg_catalog.to_regprocedure(
            'public.starring_runtime_serving_disconnect_v1(text,text,text,text,text,bigint,bigint,bigint)'
        )
        UNION
        SELECT pg_catalog.to_regprocedure(
            'public.starring_runtime_lock_current_authority(text,text,text,text,bigint,text,text,bigint,text,bigint,text)'
        )
        UNION
        SELECT pg_catalog.to_regprocedure(
            'public.starring_runtime_mutation_clock()'
        )
        UNION
        SELECT pg_catalog.to_regprocedure(
            'public.starring_runtime_current_mutation_clock()'
        )
        UNION
        SELECT pg_catalog.to_regprocedure(
            'public.starring_canonical_json_v1(jsonb)'
        )
        UNION
        SELECT pg_catalog.to_regprocedure(
            'public.starring_ruleset_content_hash_v1(bigint,jsonb)'
        )
        UNION
        SELECT trigger_row.tgfoid
        FROM pg_catalog.pg_trigger AS trigger_row
        WHERE trigger_row.tgrelid IN (SELECT relation_oid FROM protected)
            AND NOT trigger_row.tgisinternal
    ), permitted_external_index(index_oid) AS (
        SELECT index_row.oid
        FROM pg_catalog.pg_index AS index_contract
        INNER JOIN pg_catalog.pg_class AS table_row
            ON table_row.oid = index_contract.indrelid
        INNER JOIN pg_catalog.pg_namespace AS table_namespace
            ON table_namespace.oid = table_row.relnamespace
        INNER JOIN pg_catalog.pg_class AS index_row
            ON index_row.oid = index_contract.indexrelid
        INNER JOIN pg_catalog.pg_namespace AS index_namespace
            ON index_namespace.oid = index_row.relnamespace
        INNER JOIN pg_catalog.pg_am AS index_method
            ON index_method.oid = index_row.relam
        WHERE table_namespace.nspname = 'public'
            AND table_row.relname = 'runtime_deployments'
            AND index_namespace.nspname = 'public'
            AND index_row.relname
                = 'runtime_deployments_active_controller_index'
            AND index_row.relowner = table_row.relowner
            AND index_row.relkind = 'i'
            AND index_row.relpersistence = 'p'
            AND NOT index_row.relispartition
            AND index_method.amname = 'btree'
            AND NOT index_contract.indisprimary
            AND NOT index_contract.indisunique
            AND index_contract.indisvalid
            AND index_contract.indisready
            AND index_contract.indislive
            AND index_contract.indimmediate
            AND NOT index_contract.indisclustered
            AND NOT index_contract.indisreplident
            AND NOT index_contract.indnullsnotdistinct
            AND index_contract.indnkeyatts = 4
            AND index_contract.indnatts = 4
            AND index_contract.indexprs IS NULL
            AND pg_catalog.pg_get_expr(
                index_contract.indpred,
                index_contract.indrelid
            ) = '(controller_id IS NOT NULL)'
            AND pg_catalog.pg_get_indexdef(index_row.oid, 1, TRUE)
                = 'controller_id'
            AND pg_catalog.pg_get_indexdef(index_row.oid, 2, TRUE)
                = 'controller_lease_expires_at'
            AND pg_catalog.pg_get_indexdef(index_row.oid, 3, TRUE)
                = 'controller_acquired_at'
            AND pg_catalog.pg_get_indexdef(index_row.oid, 4, TRUE)
                = 'deployment_id'
    ), manifest(value) AS (
        SELECT pg_catalog.concat_ws(
            '|',
            'relation',
            pg_catalog.format('%I.%I', namespace.nspname, relation.relname),
            relation.relkind::TEXT,
            relation.relpersistence::TEXT,
            relation.relispartition::TEXT,
            relation.relrowsecurity::TEXT,
            relation.relforcerowsecurity::TEXT,
            relation.relreplident::TEXT
        )
        FROM pg_catalog.pg_class AS relation
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = relation.relnamespace
        WHERE relation.oid IN (SELECT relation_oid FROM protected)
        UNION ALL
        SELECT pg_catalog.concat_ws(
            '|',
            'inheritance',
            pg_catalog.format(
                '%I.%I',
                child_namespace.nspname,
                child.relname
            ),
            pg_catalog.format(
                '%I.%I',
                parent_namespace.nspname,
                parent.relname
            ),
            inheritance.inhseqno::TEXT,
            inheritance.inhdetachpending::TEXT
        )
        FROM pg_catalog.pg_inherits AS inheritance
        INNER JOIN pg_catalog.pg_class AS child
            ON child.oid = inheritance.inhrelid
        INNER JOIN pg_catalog.pg_namespace AS child_namespace
            ON child_namespace.oid = child.relnamespace
        INNER JOIN pg_catalog.pg_class AS parent
            ON parent.oid = inheritance.inhparent
        INNER JOIN pg_catalog.pg_namespace AS parent_namespace
            ON parent_namespace.oid = parent.relnamespace
        WHERE inheritance.inhrelid IN (SELECT relation_oid FROM protected)
            OR inheritance.inhparent IN (SELECT relation_oid FROM protected)
        UNION ALL
        SELECT pg_catalog.concat_ws(
            '|',
            'attribute',
            pg_catalog.format('%I.%I', namespace.nspname, relation.relname),
            attribute.attnum::TEXT,
            attribute.attname,
            pg_catalog.format_type(attribute.atttypid, attribute.atttypmod),
            attribute.attnotnull::TEXT,
            attribute.attidentity::TEXT,
            attribute.attgenerated::TEXT,
            attribute.attstorage::TEXT,
            attribute.attcompression::TEXT,
            attribute.atthasdef::TEXT,
            COALESCE(
                pg_catalog.pg_get_expr(default_row.adbin, default_row.adrelid),
                ''
            ),
            COALESCE(collation_namespace.nspname, ''),
            COALESCE(collation_row.collname, '')
        )
        FROM pg_catalog.pg_class AS relation
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = relation.relnamespace
        INNER JOIN pg_catalog.pg_attribute AS attribute
            ON attribute.attrelid = relation.oid
            AND attribute.attnum > 0
            AND NOT attribute.attisdropped
        LEFT JOIN pg_catalog.pg_attrdef AS default_row
            ON default_row.adrelid = attribute.attrelid
            AND default_row.adnum = attribute.attnum
        LEFT JOIN pg_catalog.pg_collation AS collation_row
            ON collation_row.oid = attribute.attcollation
        LEFT JOIN pg_catalog.pg_namespace AS collation_namespace
            ON collation_namespace.oid = collation_row.collnamespace
        WHERE relation.oid IN (SELECT relation_oid FROM protected)
        UNION ALL
        SELECT pg_catalog.concat_ws(
            '|',
            'constraint',
            pg_catalog.format('%I.%I', namespace.nspname, relation.relname),
            constraint_row.conname,
            constraint_row.contype::TEXT,
            constraint_row.convalidated::TEXT,
            constraint_row.condeferrable::TEXT,
            constraint_row.condeferred::TEXT,
            constraint_row.connoinherit::TEXT,
            constraint_row.conislocal::TEXT,
            constraint_row.coninhcount::TEXT,
            (constraint_row.conparentid = 0)::TEXT,
            COALESCE(index_row.relname, ''),
            pg_catalog.pg_get_constraintdef(constraint_row.oid, TRUE)
        )
        FROM pg_catalog.pg_constraint AS constraint_row
        INNER JOIN pg_catalog.pg_class AS relation
            ON relation.oid = constraint_row.conrelid
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = relation.relnamespace
        LEFT JOIN pg_catalog.pg_class AS index_row
            ON index_row.oid = constraint_row.conindid
        WHERE constraint_row.conrelid IN (SELECT relation_oid FROM protected)
        UNION ALL
        SELECT pg_catalog.concat_ws(
            '|',
            'index',
            pg_catalog.format(
                '%I.%I',
                table_namespace.nspname,
                table_row.relname
            ),
            pg_catalog.format(
                '%I.%I',
                index_namespace.nspname,
                index_row.relname
            ),
            (index_row.relowner = table_row.relowner)::TEXT,
            index_row.relkind::TEXT,
            index_row.relpersistence::TEXT,
            index_row.relispartition::TEXT,
            index_method.amname,
            index_contract.indisprimary::TEXT,
            index_contract.indisunique::TEXT,
            index_contract.indisvalid::TEXT,
            index_contract.indisready::TEXT,
            index_contract.indislive::TEXT,
            index_contract.indimmediate::TEXT,
            index_contract.indisclustered::TEXT,
            index_contract.indisreplident::TEXT,
            index_contract.indnullsnotdistinct::TEXT,
            index_contract.indnkeyatts::TEXT,
            index_contract.indnatts::TEXT,
            COALESCE(
                pg_catalog.pg_get_expr(
                    index_contract.indexprs,
                    index_contract.indrelid
                ),
                ''
            ),
            COALESCE(
                pg_catalog.pg_get_expr(
                    index_contract.indpred,
                    index_contract.indrelid
                ),
                ''
            ),
            pg_catalog.pg_get_indexdef(index_row.oid)
        )
        FROM pg_catalog.pg_index AS index_contract
        INNER JOIN pg_catalog.pg_class AS table_row
            ON table_row.oid = index_contract.indrelid
        INNER JOIN pg_catalog.pg_namespace AS table_namespace
            ON table_namespace.oid = table_row.relnamespace
        INNER JOIN pg_catalog.pg_class AS index_row
            ON index_row.oid = index_contract.indexrelid
        INNER JOIN pg_catalog.pg_namespace AS index_namespace
            ON index_namespace.oid = index_row.relnamespace
        INNER JOIN pg_catalog.pg_am AS index_method
            ON index_method.oid = index_row.relam
        WHERE index_contract.indrelid IN (SELECT relation_oid FROM protected)
            AND index_contract.indexrelid NOT IN (
                SELECT index_oid FROM permitted_external_index
            )
        UNION ALL
        SELECT pg_catalog.concat_ws(
            '|',
            'trigger',
            pg_catalog.format('%I.%I', namespace.nspname, relation.relname),
            trigger_row.tgname,
            pg_catalog.format(
                '%I.%I(%s)',
                function_namespace.nspname,
                function_row.proname,
                pg_catalog.pg_get_function_identity_arguments(function_row.oid)
            ),
            trigger_row.tgtype::TEXT,
            trigger_row.tgenabled::TEXT,
            trigger_row.tgisinternal::TEXT,
            trigger_row.tgnargs::TEXT,
            pg_catalog.octet_length(trigger_row.tgargs)::TEXT,
            trigger_row.tgattr::TEXT,
            (trigger_row.tgqual IS NULL)::TEXT,
            (trigger_row.tgconstraint = 0)::TEXT,
            trigger_row.tgdeferrable::TEXT,
            trigger_row.tginitdeferred::TEXT,
            COALESCE(trigger_row.tgoldtable, ''),
            COALESCE(trigger_row.tgnewtable, ''),
            pg_catalog.pg_get_triggerdef(trigger_row.oid, TRUE)
        )
        FROM pg_catalog.pg_trigger AS trigger_row
        INNER JOIN pg_catalog.pg_class AS relation
            ON relation.oid = trigger_row.tgrelid
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = relation.relnamespace
        INNER JOIN pg_catalog.pg_proc AS function_row
            ON function_row.oid = trigger_row.tgfoid
        INNER JOIN pg_catalog.pg_namespace AS function_namespace
            ON function_namespace.oid = function_row.pronamespace
        WHERE trigger_row.tgrelid IN (SELECT relation_oid FROM protected)
            AND NOT trigger_row.tgisinternal
        UNION ALL
        SELECT pg_catalog.concat_ws(
            '|',
            'function',
            pg_catalog.format(
                '%I.%I(%s)',
                namespace.nspname,
                function_row.proname,
                pg_catalog.pg_get_function_identity_arguments(function_row.oid)
            ),
            language_row.lanname,
            pg_catalog.pg_get_function_result(function_row.oid),
            pg_catalog.pg_get_functiondef(function_row.oid)
        )
        FROM pg_catalog.pg_proc AS function_row
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = function_row.pronamespace
        INNER JOIN pg_catalog.pg_language AS language_row
            ON language_row.oid = function_row.prolang
        WHERE function_row.oid IN (
            SELECT function_oid
            FROM protected_function
            WHERE function_oid IS NOT NULL
        )
    )
    SELECT pg_catalog.count(*),
        pg_catalog.encode(
            pg_catalog.sha256(
                pg_catalog.convert_to(
                    pg_catalog.string_agg(value, E'\n' ORDER BY value),
                    'UTF8'
                )
            ),
            'hex'
        )
    INTO observed_count, observed_digest
    FROM manifest;

    RETURN observed_count = 451
        AND observed_digest
            = 'a398a8aca610f2082c48c63f3c50c048cb25d6d1024eaa4c0278960c774dbcbf';
END;
$function$;

DO $privileges$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    grantee OID;
    grantee_name NAME;
    column_name NAME;
    relation_identity TEXT;
    function_identity TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');
    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);

    IF common_owner_name IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_serving_database_owner_drift';
    END IF;

    FOREACH relation_identity IN ARRAY ARRAY[
        'public.product_control_plane_identity',
        'public.runtime_deployments',
        'public.runtime_attestations',
        'public.runtime_serving_leases',
        'public.activation_requests',
        'public.authoring_promotions',
        'public.product_tenants',
        'public.automation_installations',
        'public.automation_installation_authority_versions',
        'public.automation_ruleset_activations',
        'public.automation_ruleset_versions'
    ]::TEXT[]
    LOOP
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON TABLE %s FROM PUBLIC CASCADE',
            relation_identity
        );
        FOR grantee IN
            SELECT DISTINCT privilege.grantee
            FROM pg_catalog.pg_class AS relation
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                relation.relacl,
                pg_catalog.acldefault('r', relation.relowner)
            )) AS privilege
            WHERE relation.oid = pg_catalog.to_regclass(relation_identity)
                AND privilege.grantee <> 0
                AND privilege.grantee <> common_owner
        LOOP
            grantee_name := pg_catalog.pg_get_userbyid(grantee);
            IF grantee_name IS NULL THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RE001',
                    MESSAGE = 'runtime_serving_database_relation_grantee_drift';
            END IF;
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON TABLE %s FROM %I CASCADE',
                relation_identity,
                grantee_name
            );
        END LOOP;
        FOR column_name, grantee IN
            SELECT attribute.attname, privilege.grantee
            FROM pg_catalog.pg_class AS relation
            INNER JOIN pg_catalog.pg_attribute AS attribute
                ON attribute.attrelid = relation.oid
            CROSS JOIN LATERAL pg_catalog.aclexplode(attribute.attacl) AS privilege
            WHERE relation.oid = pg_catalog.to_regclass(relation_identity)
                AND attribute.attnum > 0
                AND NOT attribute.attisdropped
                AND privilege.grantee <> 0
                AND privilege.grantee <> common_owner
        LOOP
            grantee_name := pg_catalog.pg_get_userbyid(grantee);
            IF grantee_name IS NULL THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RE001',
                    MESSAGE = 'runtime_serving_database_column_grantee_drift';
            END IF;
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES (%I) ON TABLE %s FROM %I CASCADE',
                column_name,
                relation_identity,
                grantee_name
            );
        END LOOP;
    END LOOP;

    FOREACH function_identity IN ARRAY ARRAY[
        'public.starring_runtime_serving_schema_manifest_v1()',
        'public.starring_runtime_serving_database_identity_v1()',
        'public.starring_runtime_serving_heartbeat_v1(TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,BIGINT,BIGINT,BIGINT)',
        'public.starring_runtime_serving_disconnect_v1(TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,BIGINT,BIGINT)',
        'public.starring_runtime_lock_current_authority(TEXT,TEXT,TEXT,TEXT,BIGINT,TEXT,TEXT,BIGINT,TEXT,BIGINT,TEXT)',
        'public.starring_runtime_mutation_clock()',
        'public.starring_runtime_current_mutation_clock()',
        'public.starring_canonical_json_v1(JSONB)',
        'public.starring_ruleset_content_hash_v1(BIGINT,JSONB)',
        'public.validate_runtime_deployment_projection()',
        'public.enforce_runtime_deployment_policy_shadow()',
        'public.guard_runtime_ruleset_artifact_transition()',
        'public.reject_runtime_deployment_delete()',
        'public.validate_runtime_attestation_projection()',
        'public.reject_immutable_product_row()',
        'public.validate_runtime_serving_lease_transition()',
        'public.reject_runtime_serving_lease_delete()',
        'public.reject_ruleset_artifact_mutation()'
    ]::TEXT[]
    LOOP
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s OWNER TO %I',
            function_identity,
            common_owner_name
        );
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE',
            function_identity
        );
        FOR grantee IN
            SELECT DISTINCT privilege.grantee
            FROM pg_catalog.pg_proc AS function_row
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE function_row.oid = pg_catalog.to_regprocedure(function_identity)
                AND privilege.grantee <> 0
                AND privilege.grantee <> common_owner
        LOOP
            grantee_name := pg_catalog.pg_get_userbyid(grantee);
            IF grantee_name IS NULL THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RE001',
                    MESSAGE = 'runtime_serving_database_function_grantee_drift';
            END IF;
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM %I CASCADE',
                function_identity,
                grantee_name
            );
        END LOOP;
    END LOOP;
END;
$privileges$;

DO $postflight$
DECLARE
    common_owner OID;
    invalid_relation_count BIGINT;
    invalid_function_count BIGINT;
    invalid_support_acl_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT pg_catalog.count(*)
    INTO invalid_relation_count
    FROM (
        VALUES
            ('public.product_control_plane_identity'),
            ('public.runtime_deployments'),
            ('public.runtime_attestations'),
            ('public.runtime_serving_leases'),
            ('public.activation_requests'),
            ('public.authoring_promotions'),
            ('public.product_tenants'),
            ('public.automation_installations'),
            ('public.automation_installation_authority_versions'),
            ('public.automation_ruleset_activations'),
            ('public.automation_ruleset_versions')
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
            WHERE privilege.grantee <> relation.relowner
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_attribute AS attribute
            CROSS JOIN LATERAL pg_catalog.aclexplode(attribute.attacl) AS privilege
            WHERE attribute.attrelid = relation.oid
                AND attribute.attnum > 0
                AND NOT attribute.attisdropped
                AND privilege.grantee <> relation.relowner
        );

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_runtime_serving_schema_manifest_v1()',
                ''::TEXT,
                'boolean'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'public.starring_runtime_serving_database_identity_v1()',
                ''::TEXT,
                'text'::TEXT,
                'sql'::TEXT,
                TRUE,
                FALSE,
                0::REAL
            ),
            (
                'public.starring_runtime_serving_heartbeat_v1(text,text,text,text,text,bigint,bigint,bigint,bigint)',
                'expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_attestation_id text, expected_process_instance_id text, expected_runtime_generation bigint, expected_lease_epoch bigint, expected_revision bigint, requested_lease_milliseconds bigint'::TEXT,
                'TABLE(tenant_id text, installation_id text, deployment_id text, guild_id text, ruleset_key text, attestation_id text, process_instance_id text, runtime_generation bigint, lease_epoch bigint, revision bigint, acquired_at timestamp with time zone, last_heartbeat_at timestamp with time zone, expires_at timestamp with time zone, connected boolean, serving boolean)'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_serving_disconnect_v1(text,text,text,text,text,bigint,bigint,bigint)',
                'expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_attestation_id text, expected_process_instance_id text, expected_runtime_generation bigint, expected_lease_epoch bigint, expected_revision bigint'::TEXT,
                'TABLE(tenant_id text, installation_id text, deployment_id text, guild_id text, ruleset_key text, attestation_id text, process_instance_id text, runtime_generation bigint, lease_epoch bigint, revision bigint, acquired_at timestamp with time zone, last_heartbeat_at timestamp with time zone, expires_at timestamp with time zone, connected boolean, serving boolean)'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                TRUE,
                1::REAL
            )
    ) AS expected(
        identity,
        arguments,
        result,
        language_name,
        is_strict,
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
        OR function_row.provolatile <> 'v'
        OR function_row.proisstrict IS DISTINCT FROM expected.is_strict
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR function_row.proretset IS DISTINCT FROM expected.returns_set
        OR function_row.prorows IS DISTINCT FROM expected.rows_estimate
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM expected.language_name
        OR pg_catalog.pg_get_function_identity_arguments(function_row.oid)
            IS DISTINCT FROM expected.arguments
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM expected.result
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
                OR privilege.grantor <> common_owner
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
        );

    SELECT pg_catalog.count(*)
    INTO invalid_support_acl_count
    FROM (
        VALUES
            ('public.starring_runtime_lock_current_authority(text,text,text,text,bigint,text,text,bigint,text,bigint,text)'),
            ('public.starring_runtime_mutation_clock()'),
            ('public.starring_runtime_current_mutation_clock()'),
            ('public.starring_canonical_json_v1(jsonb)'),
            ('public.starring_ruleset_content_hash_v1(bigint,jsonb)'),
            ('public.validate_runtime_deployment_projection()'),
            ('public.enforce_runtime_deployment_policy_shadow()'),
            ('public.guard_runtime_ruleset_artifact_transition()'),
            ('public.reject_runtime_deployment_delete()'),
            ('public.validate_runtime_attestation_projection()'),
            ('public.reject_immutable_product_row()'),
            ('public.validate_runtime_serving_lease_transition()'),
            ('public.reject_runtime_serving_lease_delete()'),
            ('public.reject_ruleset_artifact_mutation()')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
                OR privilege.grantor <> common_owner
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
        );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR invalid_relation_count <> 0
        OR invalid_function_count <> 0
        OR invalid_support_acl_count <> 0
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_serving_database_postflight_drift';
    END IF;
END;
$postflight$;

CREATE FUNCTION public.starring_runtime_serving_database_readiness_v1()
RETURNS TABLE(
    database_identity TEXT,
    database_name TEXT,
    executor_role TEXT,
    checked_at TIMESTAMPTZ
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
    common_owner OID;
    database_owner OID;
    database_oid OID;
    invoker_oid OID;
    invalid_relation_count BIGINT;
    invalid_function_count BIGINT;
    invalid_support_function_count BIGINT;
    invalid_protected_function_count BIGINT;
    identity_count BIGINT;
    unexpected_capability_count BIGINT;
    unsafe_schema_count BIGINT;
    unsafe_default_count BIGINT;
    unsafe_system_count BIGINT;
    role_found BOOLEAN;
    role_row RECORD;
BEGIN
    IF pg_catalog.current_setting('role') <> 'none' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_serving_database_role_drift';
    END IF;

    invoker_oid := pg_catalog.to_regrole(session_user);
    SELECT role.rolsuper,
        role.rolinherit,
        role.rolcreaterole,
        role.rolcreatedb,
        role.rolcanlogin,
        role.rolreplication,
        role.rolbypassrls,
        role.rolconnlimit,
        role.rolconfig,
        role.rolname
    INTO role_row
    FROM pg_catalog.pg_roles AS role
    WHERE role.oid = invoker_oid;
    role_found := FOUND;

    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT database_row.oid, database_row.datdba
    INTO database_oid, database_owner
    FROM pg_catalog.pg_database AS database_row
    WHERE database_row.datname = pg_catalog.current_database();

    IF NOT FOUND
        OR NOT role_found
        OR invoker_oid IS NULL
        OR common_owner IS NULL
        OR database_oid IS NULL
        OR database_owner IS NULL
        OR invoker_oid IN (common_owner, database_owner)
        OR role_row.rolsuper
        OR role_row.rolinherit
        OR role_row.rolcreaterole
        OR role_row.rolcreatedb
        OR NOT role_row.rolcanlogin
        OR role_row.rolreplication
        OR role_row.rolbypassrls
        OR role_row.rolconnlimit NOT BETWEEN 1 AND 4
        OR COALESCE(pg_catalog.cardinality(role_row.rolconfig), 0) <> 0
        OR role_row.rolname::TEXT !~ '^[a-z_][a-z0-9_]{0,62}$'
        OR pg_catalog.current_database() !~ '^[a-z_][a-z0-9_]{0,62}$'
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_auth_members AS membership
            WHERE membership.member = invoker_oid
                OR membership.roleid = invoker_oid
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_db_role_setting AS setting
            WHERE (
                    setting.setrole = invoker_oid
                    AND setting.setdatabase IN (0, database_oid)
                )
                OR (
                    setting.setrole = 0
                    AND setting.setdatabase = database_oid
                )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_serving_database_role_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_relation_count
    FROM (
        VALUES
            ('public.product_control_plane_identity'),
            ('public.runtime_deployments'),
            ('public.runtime_attestations'),
            ('public.runtime_serving_leases'),
            ('public.activation_requests'),
            ('public.authoring_promotions'),
            ('public.product_tenants'),
            ('public.automation_installations'),
            ('public.automation_installation_authority_versions'),
            ('public.automation_ruleset_activations'),
            ('public.automation_ruleset_versions')
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
            WHERE privilege.grantee <> relation.relowner
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_attribute AS attribute
            CROSS JOIN LATERAL pg_catalog.aclexplode(attribute.attacl) AS privilege
            WHERE attribute.attrelid = relation.oid
                AND attribute.attnum > 0
                AND NOT attribute.attisdropped
                AND privilege.grantee <> relation.relowner
        );

    IF invalid_relation_count <> 0
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_serving_database_schema_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_runtime_serving_database_readiness_v1()',
                ''::TEXT,
                'TABLE(database_identity text, database_name text, executor_role text, checked_at timestamp with time zone)'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_serving_database_identity_v1()',
                ''::TEXT,
                'text'::TEXT,
                'sql'::TEXT,
                FALSE,
                0::REAL
            ),
            (
                'public.starring_runtime_serving_heartbeat_v1(text,text,text,text,text,bigint,bigint,bigint,bigint)',
                'expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_attestation_id text, expected_process_instance_id text, expected_runtime_generation bigint, expected_lease_epoch bigint, expected_revision bigint, requested_lease_milliseconds bigint'::TEXT,
                'TABLE(tenant_id text, installation_id text, deployment_id text, guild_id text, ruleset_key text, attestation_id text, process_instance_id text, runtime_generation bigint, lease_epoch bigint, revision bigint, acquired_at timestamp with time zone, last_heartbeat_at timestamp with time zone, expires_at timestamp with time zone, connected boolean, serving boolean)'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                1::REAL
            ),
            (
                'public.starring_runtime_serving_disconnect_v1(text,text,text,text,text,bigint,bigint,bigint)',
                'expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_attestation_id text, expected_process_instance_id text, expected_runtime_generation bigint, expected_lease_epoch bigint, expected_revision bigint'::TEXT,
                'TABLE(tenant_id text, installation_id text, deployment_id text, guild_id text, ruleset_key text, attestation_id text, process_instance_id text, runtime_generation bigint, lease_epoch bigint, revision bigint, acquired_at timestamp with time zone, last_heartbeat_at timestamp with time zone, expires_at timestamp with time zone, connected boolean, serving boolean)'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                1::REAL
            )
    ) AS expected(identity, arguments, result, language_name, returns_set, rows_estimate)
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
        OR function_row.proretset IS DISTINCT FROM expected.returns_set
        OR function_row.prorows IS DISTINCT FROM expected.rows_estimate
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM expected.language_name
        OR pg_catalog.pg_get_function_identity_arguments(function_row.oid)
            IS DISTINCT FROM expected.arguments
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM expected.result
        OR NOT pg_catalog.has_function_privilege(
            invoker_oid,
            function_row.oid,
            'EXECUTE'
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee NOT IN (common_owner, invoker_oid)
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
                OR privilege.grantor <> common_owner
        );

    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_serving_database_function_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_support_function_count
    FROM (
        VALUES
            (
                'public.starring_runtime_serving_schema_manifest_v1()',
                ''::TEXT,
                'boolean'::TEXT,
                'plpgsql'::TEXT,
                TRUE,
                'b61a87b523c89b641e66ce8affbd7904c2a1ee0d29c88131a9a3475becc0c8d2'::TEXT
            )
    ) AS expected(
        identity,
        arguments,
        result,
        language_name,
        is_strict,
        definition_digest
    )
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR function_row.proisstrict IS DISTINCT FROM expected.is_strict
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR function_row.proretset
        OR function_row.prorows <> 0::REAL
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM expected.language_name
        OR pg_catalog.pg_get_function_identity_arguments(function_row.oid)
            IS DISTINCT FROM expected.arguments
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM expected.result
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(function_row.oid),
                'UTF8'
            )),
            'hex'
        ) IS DISTINCT FROM expected.definition_digest
        OR pg_catalog.has_function_privilege(
            invoker_oid,
            function_row.oid,
            'EXECUTE'
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
                OR privilege.grantor <> common_owner
        );

    IF invalid_support_function_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_serving_database_support_function_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_protected_function_count
    FROM (
        VALUES
            ('public.starring_runtime_lock_current_authority(text,text,text,text,bigint,text,text,bigint,text,bigint,text)'),
            ('public.starring_runtime_mutation_clock()'),
            ('public.starring_runtime_current_mutation_clock()'),
            ('public.starring_canonical_json_v1(jsonb)'),
            ('public.starring_ruleset_content_hash_v1(bigint,jsonb)'),
            ('public.validate_runtime_deployment_projection()'),
            ('public.enforce_runtime_deployment_policy_shadow()'),
            ('public.guard_runtime_ruleset_artifact_transition()'),
            ('public.reject_runtime_deployment_delete()'),
            ('public.validate_runtime_attestation_projection()'),
            ('public.reject_immutable_product_row()'),
            ('public.validate_runtime_serving_lease_transition()'),
            ('public.reject_runtime_serving_lease_delete()'),
            ('public.reject_ruleset_artifact_mutation()')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR pg_catalog.has_function_privilege(
            invoker_oid,
            function_row.oid,
            'EXECUTE'
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
                OR privilege.grantor <> common_owner
        );

    IF invalid_protected_function_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_serving_database_protected_function_drift';
    END IF;

    SELECT pg_catalog.count(*)
    INTO unsafe_schema_count
    FROM pg_catalog.pg_namespace AS namespace
    WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema')
        AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_'
        AND (
            pg_catalog.has_schema_privilege(invoker_oid, namespace.oid, 'CREATE')
            OR (
                namespace.nspname <> 'public'
                AND pg_catalog.has_schema_privilege(invoker_oid, namespace.oid, 'USAGE')
            )
        );

    SELECT pg_catalog.count(*)
    INTO unsafe_default_count
    FROM pg_catalog.pg_default_acl AS defaults
    CROSS JOIN LATERAL pg_catalog.aclexplode(defaults.defaclacl) AS privilege
    WHERE privilege.grantee IN (0, invoker_oid);

    SELECT pg_catalog.count(*)
    INTO unexpected_capability_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE function_row.oid >= 16384
        AND pg_catalog.has_function_privilege(
            invoker_oid,
            function_row.oid,
            'EXECUTE'
        )
        AND function_row.oid NOT IN (
            pg_catalog.to_regprocedure(
                'public.starring_runtime_serving_database_readiness_v1()'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_serving_database_identity_v1()'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_serving_heartbeat_v1(text,text,text,text,text,bigint,bigint,bigint,bigint)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_serving_disconnect_v1(text,text,text,text,text,bigint,bigint,bigint)'
            )
        )
        AND namespace.nspname NOT IN ('pg_catalog', 'information_schema')
        AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_';

    IF unexpected_capability_count <> 0
        OR unsafe_schema_count <> 0
        OR unsafe_default_count <> 0
        OR NOT pg_catalog.has_database_privilege(invoker_oid, database_oid, 'CONNECT')
        OR NOT pg_catalog.has_schema_privilege(invoker_oid, 'public', 'USAGE')
        OR pg_catalog.has_database_privilege(invoker_oid, database_oid, 'CREATE')
        OR pg_catalog.has_database_privilege(invoker_oid, database_oid, 'TEMPORARY')
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_database AS foreign_database
            WHERE foreign_database.oid <> database_oid
                AND foreign_database.datallowconn
                AND (
                    pg_catalog.has_database_privilege(
                        invoker_oid,
                        foreign_database.oid,
                        'CONNECT'
                    )
                    OR pg_catalog.has_database_privilege(
                        invoker_oid,
                        foreign_database.oid,
                        'CREATE'
                    )
                    OR pg_catalog.has_database_privilege(
                        invoker_oid,
                        foreign_database.oid,
                        'TEMPORARY'
                    )
                )
        )
        OR pg_catalog.has_schema_privilege(invoker_oid, 'public', 'CREATE')
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_database AS database_row
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                database_row.datacl,
                pg_catalog.acldefault('d', database_row.datdba)
            )) AS privilege
            WHERE database_row.oid = database_oid
                AND privilege.grantee IN (0, invoker_oid)
                AND privilege.is_grantable
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.pg_namespace AS namespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                namespace.nspacl,
                pg_catalog.acldefault('n', namespace.nspowner)
            )) AS privilege
            WHERE namespace.nspname = 'public'
                AND privilege.grantee IN (0, invoker_oid)
                AND privilege.is_grantable
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_serving_database_capability_drift';
    END IF;

    WITH violations(kind) AS (
        SELECT 'system_namespace'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_namespace AS namespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                namespace.nspacl,
                pg_catalog.acldefault('n', namespace.nspowner)
            )) AS privilege
            WHERE (
                    namespace.nspname = 'information_schema'
                    OR pg_catalog.left(namespace.nspname::TEXT, 3) = 'pg_'
                )
                AND (
                    namespace.nspowner = invoker_oid
                    OR privilege.grantee = invoker_oid
                    OR (
                        privilege.grantee = 0
                        AND (
                            privilege.is_grantable
                            OR (
                                NOT (
                                    namespace.nspname = 'information_schema'
                                    AND privilege.privilege_type = 'USAGE'
                                )
                                AND NOT EXISTS (
                                    SELECT 1
                                    FROM pg_catalog.aclexplode(COALESCE(
                                        (
                                            SELECT initial.initprivs
                                            FROM pg_catalog.pg_init_privs AS initial
                                            WHERE initial.classoid
                                                    = 'pg_catalog.pg_namespace'::REGCLASS
                                                AND initial.objoid = namespace.oid
                                                AND initial.objsubid = 0
                                        ),
                                        pg_catalog.acldefault(
                                            'n',
                                            namespace.nspowner
                                        )
                                    )) AS initial_privilege
                                    WHERE initial_privilege.grantee = 0
                                        AND initial_privilege.privilege_type
                                            = privilege.privilege_type
                                )
                            )
                        )
                    )
                )
        )
        UNION ALL
        SELECT 'system_relation'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_class AS relation
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = relation.relnamespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                relation.relacl,
                pg_catalog.acldefault('r', relation.relowner)
            )) AS privilege
            WHERE (
                    namespace.nspname = 'information_schema'
                    OR pg_catalog.left(namespace.nspname::TEXT, 3) = 'pg_'
                )
                AND relation.relkind IN ('r', 'p', 'v', 'm', 'f')
                AND (
                    relation.relowner = invoker_oid
                    OR privilege.grantee = invoker_oid
                    OR (
                        privilege.grantee = 0
                        AND (
                            privilege.is_grantable
                            OR (
                                NOT (
                                    namespace.nspname = 'information_schema'
                                    AND privilege.privilege_type = 'SELECT'
                                )
                                AND NOT EXISTS (
                                    SELECT 1
                                    FROM pg_catalog.aclexplode(COALESCE(
                                        (
                                            SELECT initial.initprivs
                                            FROM pg_catalog.pg_init_privs AS initial
                                            WHERE initial.classoid
                                                    = 'pg_catalog.pg_class'::REGCLASS
                                                AND initial.objoid = relation.oid
                                                AND initial.objsubid = 0
                                        ),
                                        pg_catalog.acldefault(
                                            'r',
                                            relation.relowner
                                        )
                                    )) AS initial_privilege
                                    WHERE initial_privilege.grantee = 0
                                        AND initial_privilege.privilege_type
                                            = privilege.privilege_type
                                )
                            )
                        )
                    )
                )
        )
        UNION ALL
        SELECT 'system_attribute'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_attribute AS attribute
            INNER JOIN pg_catalog.pg_class AS relation
                ON relation.oid = attribute.attrelid
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = relation.relnamespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(attribute.attacl) AS privilege
            WHERE (
                    namespace.nspname = 'information_schema'
                    OR pg_catalog.left(namespace.nspname::TEXT, 3) = 'pg_'
                )
                AND attribute.attnum > 0
                AND NOT attribute.attisdropped
                AND (
                    privilege.grantee = invoker_oid
                    OR (
                        privilege.grantee = 0
                        AND (
                            privilege.is_grantable
                            OR NOT EXISTS (
                                SELECT 1
                                FROM pg_catalog.aclexplode(COALESCE(
                                    (
                                        SELECT initial.initprivs
                                        FROM pg_catalog.pg_init_privs AS initial
                                        WHERE initial.classoid
                                                = 'pg_catalog.pg_class'::REGCLASS
                                            AND initial.objoid = relation.oid
                                            AND initial.objsubid = attribute.attnum
                                    ),
                                    pg_catalog.acldefault(
                                        'c',
                                        relation.relowner
                                    )
                                )) AS initial_privilege
                                WHERE initial_privilege.grantee = 0
                                    AND initial_privilege.privilege_type
                                        = privilege.privilege_type
                            )
                        )
                    )
                )
        )
        UNION ALL
        SELECT 'system_sequence'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_class AS sequence
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = sequence.relnamespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                sequence.relacl,
                pg_catalog.acldefault('s', sequence.relowner)
            )) AS privilege
            WHERE (
                    namespace.nspname = 'information_schema'
                    OR pg_catalog.left(namespace.nspname::TEXT, 3) = 'pg_'
                )
                AND sequence.relkind = 'S'
                AND (
                    sequence.relowner = invoker_oid
                    OR privilege.grantee = invoker_oid
                    OR (
                        privilege.grantee = 0
                        AND (
                            privilege.is_grantable
                            OR NOT EXISTS (
                                SELECT 1
                                FROM pg_catalog.aclexplode(COALESCE(
                                    (
                                        SELECT initial.initprivs
                                        FROM pg_catalog.pg_init_privs AS initial
                                        WHERE initial.classoid
                                                = 'pg_catalog.pg_class'::REGCLASS
                                            AND initial.objoid = sequence.oid
                                            AND initial.objsubid = 0
                                    ),
                                    pg_catalog.acldefault(
                                        's',
                                        sequence.relowner
                                    )
                                )) AS initial_privilege
                                WHERE initial_privilege.grantee = 0
                                    AND initial_privilege.privilege_type
                                        = privilege.privilege_type
                            )
                        )
                    )
                )
        )
        UNION ALL
        SELECT 'system_function'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_proc AS function_row
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = function_row.pronamespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE (
                    namespace.nspname = 'information_schema'
                    OR pg_catalog.left(namespace.nspname::TEXT, 3) = 'pg_'
                )
                AND (
                    function_row.proowner = invoker_oid
                    OR privilege.grantee = invoker_oid
                    OR (
                        privilege.grantee = 0
                        AND (
                            privilege.is_grantable
                            OR function_row.oid >= 16384
                            OR NOT EXISTS (
                                SELECT 1
                                FROM pg_catalog.aclexplode(COALESCE(
                                    (
                                        SELECT initial.initprivs
                                        FROM pg_catalog.pg_init_privs AS initial
                                        WHERE initial.classoid
                                                = 'pg_catalog.pg_proc'::REGCLASS
                                            AND initial.objoid = function_row.oid
                                            AND initial.objsubid = 0
                                    ),
                                    pg_catalog.acldefault(
                                        'f',
                                        function_row.proowner
                                    )
                                )) AS initial_privilege
                                WHERE initial_privilege.grantee = 0
                                    AND initial_privilege.privilege_type
                                        = privilege.privilege_type
                            )
                        )
                    )
                )
        )
        UNION ALL
        SELECT 'system_type'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_type AS type_row
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = type_row.typnamespace
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                type_row.typacl,
                pg_catalog.acldefault('T', type_row.typowner)
            )) AS privilege
            WHERE (
                    namespace.nspname = 'information_schema'
                    OR pg_catalog.left(namespace.nspname::TEXT, 3) = 'pg_'
                )
                AND (
                    type_row.typowner = invoker_oid
                    OR privilege.grantee = invoker_oid
                    OR (
                        privilege.grantee = 0
                        AND (
                            privilege.is_grantable
                            OR type_row.oid >= 16384
                            OR NOT EXISTS (
                                SELECT 1
                                FROM pg_catalog.aclexplode(COALESCE(
                                    (
                                        SELECT initial.initprivs
                                        FROM pg_catalog.pg_init_privs AS initial
                                        WHERE initial.classoid
                                                = 'pg_catalog.pg_type'::REGCLASS
                                            AND initial.objoid = type_row.oid
                                            AND initial.objsubid = 0
                                    ),
                                    pg_catalog.acldefault(
                                        'T',
                                        type_row.typowner
                                    )
                                )) AS initial_privilege
                                WHERE initial_privilege.grantee = 0
                                    AND initial_privilege.privilege_type
                                        = privilege.privilege_type
                            )
                        )
                    )
                )
        )
        UNION ALL
        SELECT 'application_relation'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_class AS relation
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = relation.relnamespace
            WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema')
                AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_'
                AND relation.relkind IN ('r', 'p', 'v', 'm', 'f')
                AND (
                    pg_catalog.has_table_privilege(
                        invoker_oid,
                        relation.oid,
                        'SELECT'
                    )
                    OR pg_catalog.has_table_privilege(
                        invoker_oid,
                        relation.oid,
                        'INSERT'
                    )
                    OR pg_catalog.has_table_privilege(
                        invoker_oid,
                        relation.oid,
                        'UPDATE'
                    )
                    OR pg_catalog.has_table_privilege(
                        invoker_oid,
                        relation.oid,
                        'DELETE'
                    )
                    OR pg_catalog.has_table_privilege(
                        invoker_oid,
                        relation.oid,
                        'TRUNCATE'
                    )
                    OR pg_catalog.has_table_privilege(
                        invoker_oid,
                        relation.oid,
                        'REFERENCES'
                    )
                    OR pg_catalog.has_table_privilege(
                        invoker_oid,
                        relation.oid,
                        'TRIGGER'
                    )
                )
        )
        UNION ALL
        SELECT 'application_attribute'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_attribute AS attribute
            INNER JOIN pg_catalog.pg_class AS relation
                ON relation.oid = attribute.attrelid
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = relation.relnamespace
            WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema')
                AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_'
                AND attribute.attnum > 0
                AND NOT attribute.attisdropped
                AND (
                    pg_catalog.has_column_privilege(
                        invoker_oid,
                        relation.oid,
                        attribute.attname,
                        'SELECT'
                    )
                    OR pg_catalog.has_column_privilege(
                        invoker_oid,
                        relation.oid,
                        attribute.attname,
                        'INSERT'
                    )
                    OR pg_catalog.has_column_privilege(
                        invoker_oid,
                        relation.oid,
                        attribute.attname,
                        'UPDATE'
                    )
                    OR pg_catalog.has_column_privilege(
                        invoker_oid,
                        relation.oid,
                        attribute.attname,
                        'REFERENCES'
                    )
                )
        )
        UNION ALL
        SELECT 'application_sequence'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_class AS sequence
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = sequence.relnamespace
            WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema')
                AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_'
                AND sequence.relkind = 'S'
                AND (
                    pg_catalog.has_sequence_privilege(
                        invoker_oid,
                        sequence.oid,
                        'USAGE'
                    )
                    OR pg_catalog.has_sequence_privilege(
                        invoker_oid,
                        sequence.oid,
                        'SELECT'
                    )
                    OR pg_catalog.has_sequence_privilege(
                        invoker_oid,
                        sequence.oid,
                        'UPDATE'
                    )
                )
        )
        UNION ALL
        SELECT 'parameter_acl'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_parameter_acl AS parameter_acl
            CROSS JOIN LATERAL pg_catalog.aclexplode(parameter_acl.paracl) AS privilege
            WHERE privilege.grantee IN (0, invoker_oid)
                AND privilege.privilege_type IN ('SET', 'ALTER SYSTEM')
        )
        UNION ALL
        SELECT 'large_object'
        WHERE EXISTS (
            SELECT 1
            FROM pg_catalog.pg_largeobject_metadata AS large_object
            WHERE large_object.lomowner = invoker_oid
                OR EXISTS (
                    SELECT 1
                    FROM pg_catalog.aclexplode(COALESCE(
                        large_object.lomacl,
                        pg_catalog.acldefault('L', large_object.lomowner)
                    )) AS privilege
                    WHERE privilege.grantee IN (0, invoker_oid)
                        AND (
                            privilege.privilege_type IN ('SELECT', 'UPDATE')
                            OR privilege.is_grantable
                        )
                )
        )
    )
    SELECT pg_catalog.count(*)
    INTO unsafe_system_count
    FROM violations;

    IF unsafe_system_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_serving_database_system_capability_drift';
    END IF;

    SELECT pg_catalog.count(*),
        pg_catalog.min(identity.database_identity::TEXT)
    INTO identity_count, database_identity
    FROM public.product_control_plane_identity AS identity
    WHERE identity.singleton
        AND identity.database_identity IS NOT NULL
        AND identity.database_identity
            <> '00000000-0000-0000-0000-000000000000'::UUID
        AND identity.database_identity::TEXT
            ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
        AND identity.created_at IS NOT NULL;

    IF identity_count <> 1 OR database_identity IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_serving_database_identity_drift';
    END IF;

    database_name := pg_catalog.current_database()::TEXT;
    executor_role := session_user::TEXT;
    checked_at := pg_catalog.clock_timestamp();
    RETURN NEXT;
END;
$function$;

DO $readiness_body_postflight$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    grantee OID;
    grantee_name NAME;
    invalid_function_count BIGINT;
    function_identity TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');
    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    function_identity :=
        'public.starring_runtime_serving_database_readiness_v1()';

    IF common_owner_name IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_serving_database_owner_drift';
    END IF;

    EXECUTE pg_catalog.format(
        'ALTER FUNCTION %s OWNER TO %I',
        function_identity,
        common_owner_name
    );
    EXECUTE pg_catalog.format(
        'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE',
        function_identity
    );
    FOR grantee IN
        SELECT DISTINCT privilege.grantee
        FROM pg_catalog.pg_proc AS function_row
        CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
            function_row.proacl,
            pg_catalog.acldefault('f', function_row.proowner)
        )) AS privilege
        WHERE function_row.oid = pg_catalog.to_regprocedure(function_identity)
            AND privilege.grantee <> 0
            AND privilege.grantee <> common_owner
    LOOP
        grantee_name := pg_catalog.pg_get_userbyid(grantee);
        IF grantee_name IS NULL THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RE001',
                MESSAGE = 'runtime_serving_database_function_grantee_drift';
        END IF;
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM %I CASCADE',
            function_identity,
            grantee_name
        );
    END LOOP;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM pg_catalog.pg_proc AS function_row
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_runtime_serving_database_readiness_v1()'
        )
        AND (
            function_row.proowner <> common_owner
            OR function_row.prokind <> 'f'
            OR function_row.provolatile <> 'v'
            OR NOT function_row.proisstrict
            OR function_row.proparallel <> 'u'
            OR NOT function_row.prosecdef
            OR NOT function_row.proretset
            OR function_row.prorows <> 1::REAL
            OR function_row.proconfig
                IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
            OR function_row.proleakproof
            OR function_row.pronargdefaults <> 0
            OR function_row.provariadic <> 0
            OR language_row.lanname IS DISTINCT FROM 'plpgsql'
            OR pg_catalog.pg_get_function_identity_arguments(function_row.oid)
                IS DISTINCT FROM ''
            OR pg_catalog.pg_get_function_result(function_row.oid)
                IS DISTINCT FROM 'TABLE(database_identity text, database_name text, executor_role text, checked_at timestamp with time zone)'
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE privilege.grantee <> common_owner
                    OR privilege.grantor <> common_owner
                    OR privilege.privilege_type <> 'EXECUTE'
                    OR privilege.is_grantable
            )
        );

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR pg_catalog.to_regprocedure(
            'public.starring_runtime_serving_database_readiness_v1()'
        ) IS NULL
        OR invalid_function_count <> 0
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'runtime_serving_database_readiness_postflight_drift';
    END IF;
END;
$readiness_body_postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
