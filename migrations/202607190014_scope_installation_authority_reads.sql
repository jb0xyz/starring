CREATE FUNCTION public.starring_product_installation_authority_read_v1(
    expected_installation_id TEXT,
    expected_principal_id TEXT,
    expected_product_session_digest BYTEA
)
RETURNS TABLE (
    principal_id TEXT,
    acting_user_id TEXT,
    principal_disabled BOOLEAN,
    session_digest BYTEA,
    session_principal_id TEXT,
    oauth_state_digest_length INTEGER,
    last_seen_at TIMESTAMPTZ,
    idle_expires_at TIMESTAMPTZ,
    absolute_expires_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    installation_tenant_id TEXT,
    installation_id TEXT,
    tenant_id TEXT,
    tenant_lifecycle_state TEXT,
    installation_lifecycle_state TEXT,
    discord_application_id TEXT,
    discord_guild_id TEXT,
    current_authority_revision BIGINT,
    authority_tenant_id TEXT,
    authority_installation_id TEXT,
    authority_revision BIGINT,
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
    SELECT principal.principal_id,
        principal.discord_user_id AS acting_user_id,
        principal.disabled AS principal_disabled,
        actor_session.session_digest,
        actor_session.principal_id AS session_principal_id,
        pg_catalog.octet_length(actor_session.oauth_state_digest)
            AS oauth_state_digest_length,
        actor_session.last_seen_at,
        actor_session.idle_expires_at,
        actor_session.absolute_expires_at,
        actor_session.revoked_at,
        installation.tenant_id AS installation_tenant_id,
        installation.installation_id,
        tenant.tenant_id,
        tenant.lifecycle_state AS tenant_lifecycle_state,
        installation.lifecycle_state AS installation_lifecycle_state,
        installation.discord_application_id,
        installation.discord_guild_id,
        installation.current_authority_revision,
        authority.tenant_id AS authority_tenant_id,
        authority.installation_id AS authority_installation_id,
        authority.revision AS authority_revision,
        authority.authority_payload_digest,
        request_clock.database_now
    FROM public.automation_installations AS installation
    LEFT JOIN public.product_tenants AS tenant
        ON tenant.tenant_id = installation.tenant_id
    INNER JOIN public.product_principals AS principal
        ON principal.principal_id = expected_principal_id
    INNER JOIN public.product_auth_sessions AS actor_session
        ON actor_session.principal_id = principal.principal_id
        AND actor_session.session_digest = expected_product_session_digest
    LEFT JOIN public.automation_installation_authority_versions AS authority
        ON authority.tenant_id = installation.tenant_id
        AND authority.installation_id = installation.installation_id
        AND authority.revision = installation.current_authority_revision
    CROSS JOIN request_clock
    WHERE installation.installation_id = expected_installation_id
        AND expected_installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND expected_principal_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND pg_catalog.octet_length(expected_product_session_digest) = 32;
$function$;

REVOKE ALL ON FUNCTION public.starring_product_installation_authority_read_v1(
    TEXT,
    TEXT,
    BYTEA
) FROM PUBLIC;

DO $ownership$
DECLARE
    relation_count BIGINT;
    table_count BIGINT;
    owner_count BIGINT;
    common_owner OID;
    common_owner_name NAME;
    unexpected_grantee OID;
    unexpected_grantee_name NAME;
BEGIN
    SELECT pg_catalog.count(relation.oid),
        pg_catalog.count(relation.oid) FILTER (WHERE relation.relkind = 'r'),
        pg_catalog.count(DISTINCT relation.relowner),
        pg_catalog.min(relation.relowner::BIGINT)::OID
    INTO relation_count, table_count, owner_count, common_owner
    FROM (
        VALUES
            (pg_catalog.to_regclass('public.product_principals')),
            (pg_catalog.to_regclass('public.product_auth_sessions')),
            (pg_catalog.to_regclass('public.product_tenants')),
            (pg_catalog.to_regclass('public.automation_installations')),
            (pg_catalog.to_regclass(
                'public.automation_installation_authority_versions'
            ))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid;

    IF relation_count <> 5
        OR table_count <> 5
        OR owner_count <> 1
        OR common_owner IS NULL
    THEN
        RAISE EXCEPTION 'installation authority relations require one owner'
            USING ERRCODE = '55000';
    END IF;

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL THEN
        RAISE EXCEPTION 'installation authority relation owner is unavailable'
            USING ERRCODE = '55000';
    END IF;

    FOR unexpected_grantee IN
        SELECT DISTINCT privilege.grantee
        FROM pg_catalog.pg_proc AS function_row
        CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
            function_row.proacl,
            pg_catalog.acldefault('f', function_row.proowner)
        )) AS privilege
        WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_product_installation_authority_read_v1(text,text,bytea)'
        )
            AND privilege.grantee <> 0
            AND privilege.grantee <> function_row.proowner
    LOOP
        unexpected_grantee_name := pg_catalog.pg_get_userbyid(unexpected_grantee);
        IF unexpected_grantee_name IS NULL THEN
            RAISE EXCEPTION 'installation authority function grantee is unavailable'
                USING ERRCODE = '55000';
        END IF;
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON FUNCTION public.starring_product_installation_authority_read_v1(TEXT, TEXT, BYTEA) FROM %I CASCADE',
            unexpected_grantee_name
        );
    END LOOP;

    EXECUTE pg_catalog.format(
        'ALTER FUNCTION public.starring_product_installation_authority_read_v1(TEXT, TEXT, BYTEA) OWNER TO %I',
        common_owner_name
    );
END;
$ownership$;
