CREATE FUNCTION public.starring_product_authorized_snapshot_read_v1(
    expected_session_id TEXT,
    expected_principal_id TEXT,
    expected_product_session_digest BYTEA,
    expected_tenant_id TEXT,
    expected_installation_id TEXT
)
RETURNS TABLE (
    session_tenant_id TEXT,
    session_installation_id TEXT,
    owner_principal_id TEXT,
    owner_discord_user_id TEXT,
    owner_disabled BOOLEAN,
    actor_session_digest BYTEA,
    current_generation BIGINT,
    session_lifecycle_state TEXT,
    tenant_lifecycle_state TEXT,
    installation_tenant_id TEXT,
    discord_application_id TEXT,
    discord_guild_id TEXT,
    ruleset_key TEXT,
    installation_lifecycle_state TEXT,
    current_authority_revision BIGINT,
    generation BIGINT,
    snapshot_schema_version BIGINT,
    snapshot_ciphertext BYTEA,
    snapshot_nonce BYTEA,
    encryption_key_id TEXT,
    encryption_suite TEXT,
    encryption_suite_version SMALLINT,
    authenticated_metadata_digest TEXT,
    generation_resource_bindings JSONB,
    generation_binding_fingerprint TEXT,
    installation_authority_revision BIGINT,
    generation_stage TEXT,
    candidate_revision BIGINT,
    candidate_hash TEXT,
    harness_contract_revision BIGINT,
    authority_tenant_id TEXT,
    binding_revision BIGINT,
    authority_resource_bindings JSONB,
    authority_binding_fingerprint TEXT,
    policy_revision BIGINT,
    required_approvals INTEGER,
    activation_ttl_seconds BIGINT,
    authority_payload_digest TEXT,
    database_now TIMESTAMPTZ
)
LANGUAGE sql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
    WITH request_clock AS MATERIALIZED (
        SELECT pg_catalog.clock_timestamp() AS database_now
    )
    SELECT authoring_session.tenant_id AS session_tenant_id,
        authoring_session.installation_id AS session_installation_id,
        authoring_session.owner_principal_id,
        principal.discord_user_id AS owner_discord_user_id,
        principal.disabled AS owner_disabled,
        actor_session.session_digest AS actor_session_digest,
        authoring_session.current_generation,
        authoring_session.lifecycle_state AS session_lifecycle_state,
        tenant.lifecycle_state AS tenant_lifecycle_state,
        installation.tenant_id AS installation_tenant_id,
        installation.discord_application_id,
        installation.discord_guild_id,
        installation.ruleset_key,
        installation.lifecycle_state AS installation_lifecycle_state,
        installation.current_authority_revision,
        generation.generation,
        generation.snapshot_schema_version,
        generation.snapshot_ciphertext,
        generation.snapshot_nonce,
        generation.encryption_key_id,
        generation.encryption_suite,
        generation.encryption_suite_version,
        generation.authenticated_metadata_digest,
        generation.resource_bindings AS generation_resource_bindings,
        generation.binding_fingerprint AS generation_binding_fingerprint,
        generation.installation_authority_revision,
        generation.stage AS generation_stage,
        generation.candidate_revision,
        generation.candidate_hash,
        generation.harness_contract_revision,
        authority.tenant_id AS authority_tenant_id,
        authority.binding_revision,
        authority.resource_bindings AS authority_resource_bindings,
        authority.binding_fingerprint AS authority_binding_fingerprint,
        authority.policy_revision,
        authority.required_approvals,
        authority.activation_ttl_seconds,
        authority.authority_payload_digest,
        request_clock.database_now
    FROM public.authoring_sessions AS authoring_session
    INNER JOIN public.product_principals AS principal
        ON principal.principal_id = authoring_session.owner_principal_id
        AND principal.principal_id = expected_principal_id
    INNER JOIN public.product_auth_sessions AS actor_session
        ON actor_session.principal_id = principal.principal_id
        AND actor_session.session_digest = expected_product_session_digest
    CROSS JOIN request_clock
    LEFT JOIN public.product_tenants AS tenant
        ON tenant.tenant_id = authoring_session.tenant_id
    LEFT JOIN public.automation_installations AS installation
        ON installation.tenant_id = authoring_session.tenant_id
        AND installation.installation_id = authoring_session.installation_id
    LEFT JOIN public.authoring_session_generations AS generation
        ON generation.tenant_id = authoring_session.tenant_id
        AND generation.installation_id = authoring_session.installation_id
        AND generation.session_id = authoring_session.session_id
        AND generation.generation = authoring_session.current_generation
    LEFT JOIN public.automation_installation_authority_versions AS authority
        ON authority.tenant_id = generation.tenant_id
        AND authority.installation_id = generation.installation_id
        AND authority.revision = generation.installation_authority_revision
    WHERE authoring_session.session_id = expected_session_id
        AND authoring_session.tenant_id = expected_tenant_id
        AND authoring_session.installation_id = expected_installation_id
        AND expected_session_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND expected_principal_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND expected_tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND expected_installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND pg_catalog.octet_length(expected_product_session_digest) = 32
        AND NOT principal.disabled
        AND pg_catalog.octet_length(actor_session.csrf_digest) = 32
        AND pg_catalog.octet_length(actor_session.oauth_state_digest) = 32
        AND actor_session.revoked_at IS NULL
        AND actor_session.revocation_reason IS NULL
        AND actor_session.authenticated_at = actor_session.created_at
        AND actor_session.created_at <= actor_session.last_seen_at
        AND actor_session.last_seen_at <= request_clock.database_now
        AND actor_session.last_seen_at < actor_session.idle_expires_at
        AND actor_session.idle_expires_at <= actor_session.absolute_expires_at
        AND actor_session.idle_expires_at
            <= actor_session.last_seen_at + INTERVAL '30 minutes'
        AND actor_session.absolute_expires_at
            <= actor_session.authenticated_at + INTERVAL '12 hours'
        AND request_clock.database_now < actor_session.idle_expires_at
        AND request_clock.database_now < actor_session.absolute_expires_at;
$function$;

REVOKE ALL ON FUNCTION public.starring_product_authorized_snapshot_read_v1(
    TEXT,
    TEXT,
    BYTEA,
    TEXT,
    TEXT
) FROM PUBLIC;

DO $ownership$
DECLARE
    relation_count BIGINT;
    table_count BIGINT;
    rls_disabled_count BIGINT;
    owner_count BIGINT;
    common_owner OID;
    common_owner_name NAME;
    function_oid OID;
    unexpected_grantee OID;
    unexpected_grantee_name NAME;
BEGIN
    SELECT pg_catalog.count(relation.oid),
        pg_catalog.count(relation.oid) FILTER (WHERE relation.relkind = 'r'),
        pg_catalog.count(relation.oid) FILTER (
            WHERE NOT relation.relrowsecurity AND NOT relation.relforcerowsecurity
        ),
        pg_catalog.count(DISTINCT relation.relowner),
        pg_catalog.min(relation.relowner::BIGINT)::OID
    INTO relation_count, table_count, rls_disabled_count, owner_count, common_owner
    FROM (
        VALUES
            (pg_catalog.to_regclass('public.product_principals')),
            (pg_catalog.to_regclass('public.product_auth_sessions')),
            (pg_catalog.to_regclass('public.product_tenants')),
            (pg_catalog.to_regclass('public.automation_installations')),
            (pg_catalog.to_regclass('public.authoring_sessions')),
            (pg_catalog.to_regclass('public.authoring_session_generations')),
            (pg_catalog.to_regclass(
                'public.automation_installation_authority_versions'
            ))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid;

    IF relation_count <> 7
        OR table_count <> 7
        OR rls_disabled_count <> 7
        OR owner_count <> 1
        OR common_owner IS NULL
    THEN
        RAISE EXCEPTION 'authorized snapshot relations require one non-RLS owner'
            USING ERRCODE = '55000';
    END IF;

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL THEN
        RAISE EXCEPTION 'authorized snapshot relation owner is unavailable'
            USING ERRCODE = '55000';
    END IF;

    function_oid := pg_catalog.to_regprocedure(
        'public.starring_product_authorized_snapshot_read_v1(text,text,bytea,text,text)'
    );
    IF function_oid IS NULL THEN
        RAISE EXCEPTION 'authorized snapshot function is unavailable'
            USING ERRCODE = '55000';
    END IF;

    FOR unexpected_grantee IN
        SELECT DISTINCT privilege.grantee
        FROM pg_catalog.pg_proc AS function_row
        CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
            function_row.proacl,
            pg_catalog.acldefault('f', function_row.proowner)
        )) AS privilege
        WHERE function_row.oid = function_oid
            AND privilege.grantee <> 0
            AND privilege.grantee <> function_row.proowner
    LOOP
        unexpected_grantee_name := pg_catalog.pg_get_userbyid(unexpected_grantee);
        IF unexpected_grantee_name IS NULL THEN
            RAISE EXCEPTION 'authorized snapshot function grantee is unavailable'
                USING ERRCODE = '55000';
        END IF;
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON FUNCTION public.starring_product_authorized_snapshot_read_v1(TEXT, TEXT, BYTEA, TEXT, TEXT) FROM %I CASCADE',
            unexpected_grantee_name
        );
    END LOOP;

    EXECUTE pg_catalog.format(
        'ALTER FUNCTION public.starring_product_authorized_snapshot_read_v1(TEXT, TEXT, BYTEA, TEXT, TEXT) OWNER TO %I',
        common_owner_name
    );
END;
$ownership$;
