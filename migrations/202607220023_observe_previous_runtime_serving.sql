SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

LOCK TABLE public.runtime_deployments, public.runtime_serving_leases
IN ACCESS SHARE MODE;

DO $preflight$
DECLARE
    common_owner OID;
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
            (pg_catalog.to_regclass('public.runtime_deployments')),
            (pg_catalog.to_regclass('public.runtime_serving_leases'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid;

    IF relation_count <> 2
        OR ordinary_count <> 2
        OR owner_count <> 1
        OR common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
    THEN
        RAISE EXCEPTION 'runtime previous serving observation requires the relation owner'
            USING ERRCODE = '55000';
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
        RAISE EXCEPTION 'runtime previous serving observation schema is not trusted'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname = 'starring_runtime_observe_previous_serving_v1';
    IF collision_count <> 0 THEN
        RAISE EXCEPTION 'runtime previous serving observation function already exists'
            USING ERRCODE = '55000';
    END IF;
END;
$preflight$;

CREATE FUNCTION public.starring_runtime_observe_previous_serving_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_deployment_revision BIGINT,
    expected_controller_id TEXT,
    expected_controller_fencing_token BIGINT,
    expected_convergence_attempt_no BIGINT,
    expected_runtime_generation BIGINT,
    expected_guild_id TEXT,
    expected_ruleset_key TEXT,
    expected_target_version BIGINT,
    expected_target_content_hash TEXT,
    expected_binding_revision BIGINT,
    expected_binding_fingerprint TEXT,
    expected_previous_runtime JSONB
)
RETURNS TABLE(
    state_name TEXT,
    observed_at TIMESTAMPTZ,
    lease_tenant_id TEXT,
    lease_installation_id TEXT,
    lease_deployment_id TEXT,
    lease_attestation_id TEXT,
    lease_process_instance_id TEXT,
    lease_runtime_generation BIGINT,
    lease_guild_id TEXT,
    lease_ruleset_key TEXT,
    lease_target_version BIGINT,
    lease_target_content_hash TEXT,
    lease_binding_revision BIGINT,
    lease_binding_fingerprint TEXT,
    lease_epoch BIGINT,
    lease_revision BIGINT,
    lease_connected BOOLEAN,
    lease_serving BOOLEAN,
    lease_acquired_at TIMESTAMPTZ,
    lease_last_heartbeat_at TIMESTAMPTZ,
    lease_expires_at TIMESTAMPTZ
)
LANGUAGE plpgsql
VOLATILE
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
DECLARE
    deployment_row public.runtime_deployments%ROWTYPE;
    serving_row public.runtime_serving_leases%ROWTYPE;
    database_now TIMESTAMPTZ;
    serving_found BOOLEAN;
BEGIN
    IF expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_controller_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_controller_fencing_token NOT BETWEEN 1 AND 9223372036854775807
        OR expected_convergence_attempt_no NOT BETWEEN 1 AND 4294967295
        OR expected_runtime_generation NOT BETWEEN 1 AND 9223372036854775807
        OR NOT (CASE
            WHEN expected_guild_id ~ '^[1-9][0-9]{0,19}$'
                THEN expected_guild_id::NUMERIC <= 18446744073709551615
            ELSE FALSE
        END)
        OR expected_ruleset_key !~ '^[A-Za-z0-9_-]{1,64}$'
        OR expected_target_version NOT BETWEEN 1 AND 4294967295
        OR expected_target_content_hash !~ '^[0-9a-f]{64}$'
        OR expected_binding_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_binding_fingerprint !~ '^[0-9a-f]{64}$'
        OR (
            expected_previous_runtime IS NOT NULL
            AND (
                pg_catalog.jsonb_typeof(expected_previous_runtime) <> 'object'
                OR pg_catalog.octet_length(expected_previous_runtime::TEXT) > 16384
                OR expected_previous_runtime #>> '{target,guild_id}'
                    IS DISTINCT FROM expected_guild_id
                OR expected_previous_runtime #>> '{target,ruleset_key}'
                    IS DISTINCT FROM expected_ruleset_key
                OR NOT (CASE
                    WHEN expected_previous_runtime #>> '{target,version}'
                        ~ '^[1-9][0-9]{0,9}$'
                    THEN (expected_previous_runtime #>> '{target,version}')::NUMERIC
                        <= 4294967295
                    ELSE FALSE
                END)
                OR expected_previous_runtime #>> '{target,content_hash}'
                    !~ '^[0-9a-f]{64}$'
                OR NOT (CASE
                    WHEN expected_previous_runtime #>> '{target,binding_revision}'
                        ~ '^[1-9][0-9]{0,18}$'
                    THEN (expected_previous_runtime #>> '{target,binding_revision}')::NUMERIC
                        <= 9223372036854775807
                    ELSE FALSE
                END)
                OR expected_previous_runtime #>> '{target,binding_fingerprint}'
                    !~ '^[0-9a-f]{64}$'
                OR NOT (CASE
                    WHEN expected_previous_runtime ->> 'runtime_generation'
                        ~ '^[1-9][0-9]{0,18}$'
                    THEN (expected_previous_runtime ->> 'runtime_generation')::NUMERIC
                        < expected_runtime_generation
                    ELSE FALSE
                END)
                OR expected_previous_runtime ->> 'process_instance_id'
                    !~ '^[A-Za-z0-9_.:-]{1,128}$'
            )
        )
    THEN
        RETURN;
    END IF;

    SELECT *
    INTO deployment_row
    FROM public.runtime_deployments
    WHERE tenant_id = expected_tenant_id
        AND installation_id = expected_installation_id
        AND deployment_id = expected_deployment_id
    FOR UPDATE;

    IF NOT FOUND
        OR deployment_row.revision IS DISTINCT FROM expected_deployment_revision
        OR deployment_row.controller_id IS DISTINCT FROM expected_controller_id
        OR deployment_row.controller_fencing_token
            IS DISTINCT FROM expected_controller_fencing_token
        OR deployment_row.convergence_attempt_no
            IS DISTINCT FROM expected_convergence_attempt_no
        OR deployment_row.runtime_generation IS DISTINCT FROM expected_runtime_generation
        OR deployment_row.guild_id IS DISTINCT FROM expected_guild_id
        OR deployment_row.ruleset_key IS DISTINCT FROM expected_ruleset_key
        OR deployment_row.target_version IS DISTINCT FROM expected_target_version
        OR deployment_row.target_content_hash IS DISTINCT FROM expected_target_content_hash
        OR deployment_row.binding_revision IS DISTINCT FROM expected_binding_revision
        OR deployment_row.binding_fingerprint IS DISTINCT FROM expected_binding_fingerprint
        OR deployment_row.previous_runtime IS DISTINCT FROM expected_previous_runtime
        OR deployment_row.snapshot -> 'previous_runtime'
            IS DISTINCT FROM COALESCE(expected_previous_runtime, 'null'::JSONB)
        OR deployment_row.phase <> 'drain_requested'
        OR deployment_row.blocked_at IS NOT NULL
        OR deployment_row.controller_acquired_at IS NULL
        OR deployment_row.controller_lease_expires_at IS NULL
    THEN
        RETURN;
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtextextended(
        pg_catalog.concat(
            'starring-runtime-serving-slot-v1:',
            expected_guild_id,
            ':',
            expected_ruleset_key
        ),
        0
    ));

    SELECT *
    INTO serving_row
    FROM public.runtime_serving_leases
    WHERE guild_id = expected_guild_id
        AND ruleset_key = expected_ruleset_key
    FOR UPDATE;
    serving_found := FOUND;

    database_now := pg_catalog.clock_timestamp();
    IF deployment_row.controller_acquired_at > database_now
        OR deployment_row.controller_lease_expires_at <= database_now
    THEN
        RETURN;
    END IF;

    IF expected_previous_runtime IS NULL THEN
        IF serving_found
            AND serving_row.connected
            AND serving_row.serving
            AND serving_row.expires_at > database_now
        THEN
            RETURN;
        END IF;
        state_name := 'absent';
        observed_at := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF NOT serving_found
        OR serving_row.tenant_id IS DISTINCT FROM expected_tenant_id
        OR serving_row.installation_id IS DISTINCT FROM expected_installation_id
        OR serving_row.deployment_id IS NOT DISTINCT FROM expected_deployment_id
        OR serving_row.guild_id
            IS DISTINCT FROM expected_previous_runtime #>> '{target,guild_id}'
        OR serving_row.ruleset_key
            IS DISTINCT FROM expected_previous_runtime #>> '{target,ruleset_key}'
        OR serving_row.target_version
            IS DISTINCT FROM (expected_previous_runtime #>> '{target,version}')::BIGINT
        OR serving_row.target_content_hash
            IS DISTINCT FROM expected_previous_runtime #>> '{target,content_hash}'
        OR serving_row.binding_revision
            IS DISTINCT FROM (expected_previous_runtime #>> '{target,binding_revision}')::BIGINT
        OR serving_row.binding_fingerprint
            IS DISTINCT FROM expected_previous_runtime #>> '{target,binding_fingerprint}'
        OR serving_row.runtime_generation
            IS DISTINCT FROM (expected_previous_runtime ->> 'runtime_generation')::BIGINT
        OR serving_row.process_instance_id
            IS DISTINCT FROM expected_previous_runtime ->> 'process_instance_id'
        OR serving_row.acquired_at > serving_row.last_heartbeat_at
        OR serving_row.last_heartbeat_at > serving_row.expires_at
        OR serving_row.acquired_at > database_now
        OR serving_row.acquired_at > deployment_row.requested_at
    THEN
        RETURN;
    END IF;

    state_name := CASE
        WHEN NOT serving_row.connected AND NOT serving_row.serving THEN 'disconnected'
        WHEN serving_row.connected AND serving_row.serving
            AND serving_row.expires_at <= database_now THEN 'expired'
        WHEN serving_row.connected AND serving_row.serving
            AND serving_row.expires_at > database_now THEN 'serving'
    END;
    IF state_name IS NULL
        OR (state_name = 'disconnected'
            AND (
                serving_row.last_heartbeat_at IS DISTINCT FROM serving_row.expires_at
                OR serving_row.last_heartbeat_at < deployment_row.requested_at
            ))
        OR (state_name = 'expired'
            AND (
                serving_row.last_heartbeat_at >= serving_row.expires_at
                OR serving_row.expires_at <= deployment_row.requested_at
            ))
        OR (state_name = 'serving'
            AND serving_row.last_heartbeat_at > database_now)
    THEN
        RETURN;
    END IF;

    observed_at := database_now;
    lease_tenant_id := serving_row.tenant_id;
    lease_installation_id := serving_row.installation_id;
    lease_deployment_id := serving_row.deployment_id;
    lease_attestation_id := serving_row.attestation_id;
    lease_process_instance_id := serving_row.process_instance_id;
    lease_runtime_generation := serving_row.runtime_generation;
    lease_guild_id := serving_row.guild_id;
    lease_ruleset_key := serving_row.ruleset_key;
    lease_target_version := serving_row.target_version;
    lease_target_content_hash := serving_row.target_content_hash;
    lease_binding_revision := serving_row.binding_revision;
    lease_binding_fingerprint := serving_row.binding_fingerprint;
    lease_epoch := serving_row.lease_epoch;
    lease_revision := serving_row.revision;
    lease_connected := serving_row.connected;
    lease_serving := serving_row.serving;
    lease_acquired_at := serving_row.acquired_at;
    lease_last_heartbeat_at := serving_row.last_heartbeat_at;
    lease_expires_at := serving_row.expires_at;
    RETURN NEXT;
END;
$function$;

REVOKE ALL PRIVILEGES ON FUNCTION public.starring_runtime_observe_previous_serving_v1(
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
    JSONB
) FROM PUBLIC CASCADE;

DO $postflight$
DECLARE
    function_row pg_catalog.pg_proc%ROWTYPE;
    language_name NAME;
    common_owner OID;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT routine.*
    INTO function_row
    FROM pg_catalog.pg_proc AS routine
    WHERE routine.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_observe_previous_serving_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,jsonb)'
    );
    SELECT language_row.lanname
    INTO language_name
    FROM pg_catalog.pg_language AS language_row
    WHERE language_row.oid = function_row.prolang;

    IF function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR function_row.proisstrict
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR NOT function_row.proretset
        OR function_row.prorows <> 1
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_name IS DISTINCT FROM 'plpgsql'
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
        )
    THEN
        RAISE EXCEPTION 'runtime previous serving observation function contract is invalid'
            USING ERRCODE = '55000';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
