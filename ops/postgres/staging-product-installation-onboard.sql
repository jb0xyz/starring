\set ON_ERROR_STOP on

\if :{?expected_database}
\else
\echo 'expected_database is required'
SELECT 1 / 0;
\endif

\if :{?expected_system_identifier}
\else
\echo 'expected_system_identifier is required'
SELECT 1 / 0;
\endif

\if :{?tenant_id}
\else
\echo 'tenant_id is required'
SELECT 1 / 0;
\endif

\if :{?tenant_display_name}
\else
\echo 'tenant_display_name is required'
SELECT 1 / 0;
\endif

\if :{?installation_id}
\else
\echo 'installation_id is required'
SELECT 1 / 0;
\endif

\if :{?discord_application_id}
\else
\echo 'discord_application_id is required'
SELECT 1 / 0;
\endif

\if :{?discord_guild_id}
\else
\echo 'discord_guild_id is required'
SELECT 1 / 0;
\endif

\if :{?ruleset_key}
\else
\echo 'ruleset_key is required'
SELECT 1 / 0;
\endif

\if :{?created_by_principal_id}
\else
\echo 'created_by_principal_id is required'
SELECT 1 / 0;
\endif

\if :{?created_by_discord_user_id}
\else
\echo 'created_by_discord_user_id is required'
SELECT 1 / 0;
\endif

\if :{?binding_fingerprint}
\else
\echo 'binding_fingerprint is required'
SELECT 1 / 0;
\endif

\if :{?authority_payload_digest}
\else
\echo 'authority_payload_digest is required'
SELECT 1 / 0;
\endif

\if :{?created_by_request_digest}
\else
\echo 'created_by_request_digest is required'
SELECT 1 / 0;
\endif

\if :{?commit_onboarding}
\else
\echo 'commit_onboarding is required'
SELECT 1 / 0;
\endif

SET lock_timeout = '5s';
SET statement_timeout = '60s';
SET idle_in_transaction_session_timeout = '60s';
SET search_path = pg_catalog;
SET starring.expected_staging_database = :'expected_database';
SET starring.expected_staging_system_identifier = :'expected_system_identifier';
SET starring.onboarding_tenant_id = :'tenant_id';
SET starring.onboarding_tenant_display_name = :'tenant_display_name';
SET starring.onboarding_installation_id = :'installation_id';
SET starring.onboarding_discord_application_id = :'discord_application_id';
SET starring.onboarding_discord_guild_id = :'discord_guild_id';
SET starring.onboarding_ruleset_key = :'ruleset_key';
SET starring.onboarding_created_by_principal_id = :'created_by_principal_id';
SET starring.onboarding_created_by_discord_user_id = :'created_by_discord_user_id';
SET starring.onboarding_binding_fingerprint = :'binding_fingerprint';
SET starring.onboarding_authority_payload_digest = :'authority_payload_digest';
SET starring.onboarding_created_by_request_digest = :'created_by_request_digest';
SET starring.onboarding_commit = :'commit_onboarding';

BEGIN TRANSACTION ISOLATION LEVEL SERIALIZABLE;

DO $guard$
DECLARE
    expected_database TEXT := pg_catalog.current_setting(
        'starring.expected_staging_database'
    );
    expected_system_identifier TEXT := pg_catalog.current_setting(
        'starring.expected_staging_system_identifier'
    );
    actual_system_identifier TEXT;
    tenant_id TEXT := pg_catalog.current_setting(
        'starring.onboarding_tenant_id'
    );
    tenant_display_name TEXT := pg_catalog.current_setting(
        'starring.onboarding_tenant_display_name'
    );
    installation_id TEXT := pg_catalog.current_setting(
        'starring.onboarding_installation_id'
    );
    discord_application_id TEXT := pg_catalog.current_setting(
        'starring.onboarding_discord_application_id'
    );
    discord_guild_id TEXT := pg_catalog.current_setting(
        'starring.onboarding_discord_guild_id'
    );
    ruleset_key TEXT := pg_catalog.current_setting(
        'starring.onboarding_ruleset_key'
    );
    created_by_principal_id TEXT := pg_catalog.current_setting(
        'starring.onboarding_created_by_principal_id'
    );
    created_by_discord_user_id TEXT := pg_catalog.current_setting(
        'starring.onboarding_created_by_discord_user_id'
    );
    binding_fingerprint TEXT := pg_catalog.current_setting(
        'starring.onboarding_binding_fingerprint'
    );
    authority_payload_digest TEXT := pg_catalog.current_setting(
        'starring.onboarding_authority_payload_digest'
    );
    created_by_request_digest TEXT := pg_catalog.current_setting(
        'starring.onboarding_created_by_request_digest'
    );
    commit_onboarding TEXT := pg_catalog.current_setting(
        'starring.onboarding_commit'
    );
BEGIN
    SELECT system_identifier::TEXT
    INTO actual_system_identifier
    FROM pg_catalog.pg_control_system();

    IF expected_database IS DISTINCT FROM pg_catalog.current_database()
        OR pg_catalog.current_database()
            !~ '^starring(_[a-z0-9]+)*_staging(_[a-z0-9]+)*$'
        OR expected_system_identifier IS DISTINCT FROM actual_system_identifier
        OR pg_catalog.inet_client_addr()
            IS DISTINCT FROM '127.0.0.1'::PG_CATALOG.INET
        OR pg_catalog.inet_server_addr()
            IS DISTINCT FROM '127.0.0.1'::PG_CATALOG.INET
        OR pg_catalog.inet_server_port() IS DISTINCT FROM 5432
        OR COALESCE((
            SELECT ssl
            FROM pg_catalog.pg_stat_ssl
            WHERE pid = pg_catalog.pg_backend_pid()
        ), TRUE)
        OR current_user IS DISTINCT FROM session_user
        OR NOT EXISTS (
            SELECT 1
            FROM pg_catalog.pg_authid AS role
            WHERE role.rolname = current_user
                AND role.rolsuper
                AND role.rolcanlogin
        )
    THEN
        RAISE EXCEPTION 'staging installation onboarding target is invalid'
            USING ERRCODE = '55000';
    END IF;

    IF tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR created_by_principal_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR ruleset_key !~ '^[A-Za-z0-9_-]{1,64}$'
        OR char_length(tenant_display_name) NOT BETWEEN 1 AND 128
        OR tenant_display_name IS DISTINCT FROM pg_catalog.btrim(
            tenant_display_name
        )
        OR tenant_display_name ~ '[[:cntrl:]]'
        OR discord_application_id !~ '^[1-9][0-9]{0,19}$'
        OR discord_application_id::PG_CATALOG.NUMERIC
            > 18446744073709551615
        OR discord_guild_id !~ '^[1-9][0-9]{0,19}$'
        OR discord_guild_id::PG_CATALOG.NUMERIC > 18446744073709551615
        OR created_by_discord_user_id !~ '^[1-9][0-9]{0,19}$'
        OR created_by_discord_user_id::PG_CATALOG.NUMERIC
            > 18446744073709551615
        OR binding_fingerprint !~ '^[0-9a-f]{64}$'
        OR authority_payload_digest !~ '^[0-9a-f]{64}$'
        OR created_by_request_digest !~ '^[0-9a-f]{64}$'
        OR commit_onboarding NOT IN ('true', 'false')
    THEN
        RAISE EXCEPTION 'staging installation onboarding input is invalid'
            USING ERRCODE = '22023';
    END IF;

    IF pg_catalog.to_regrole('starring_owner') IS NULL
        OR NOT EXISTS (
            SELECT 1
            FROM pg_catalog.pg_authid AS role
            WHERE role.rolname = 'starring_owner'
                AND NOT role.rolcanlogin
                AND NOT role.rolsuper
                AND NOT role.rolcreatedb
                AND NOT role.rolcreaterole
                AND NOT role.rolinherit
                AND NOT role.rolreplication
                AND NOT role.rolbypassrls
                AND role.rolconnlimit = 0
                AND role.rolpassword IS NULL
        )
    THEN
        RAISE EXCEPTION 'staging installation onboarding owner is invalid'
            USING ERRCODE = '55000';
    END IF;
END;
$guard$;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended(
        pg_catalog.format(
            'starring-product-installation-onboarding-v1:%s:%s:%s',
            pg_catalog.current_database(),
            pg_catalog.current_setting('starring.onboarding_discord_guild_id'),
            pg_catalog.current_setting('starring.onboarding_ruleset_key')
        ),
        0
    )
);

SET LOCAL ROLE starring_owner;
SET LOCAL search_path = pg_catalog, public;
SET CONSTRAINTS ALL DEFERRED;

DO $onboard$
<<onboard>>
DECLARE
    tenant_id TEXT := pg_catalog.current_setting(
        'starring.onboarding_tenant_id'
    );
    tenant_display_name TEXT := pg_catalog.current_setting(
        'starring.onboarding_tenant_display_name'
    );
    installation_id TEXT := pg_catalog.current_setting(
        'starring.onboarding_installation_id'
    );
    discord_application_id TEXT := pg_catalog.current_setting(
        'starring.onboarding_discord_application_id'
    );
    discord_guild_id TEXT := pg_catalog.current_setting(
        'starring.onboarding_discord_guild_id'
    );
    ruleset_key TEXT := pg_catalog.current_setting(
        'starring.onboarding_ruleset_key'
    );
    created_by_principal_id TEXT := pg_catalog.current_setting(
        'starring.onboarding_created_by_principal_id'
    );
    created_by_discord_user_id TEXT := pg_catalog.current_setting(
        'starring.onboarding_created_by_discord_user_id'
    );
    binding_fingerprint TEXT := pg_catalog.current_setting(
        'starring.onboarding_binding_fingerprint'
    );
    authority_payload_digest TEXT := pg_catalog.current_setting(
        'starring.onboarding_authority_payload_digest'
    );
    created_by_request_digest TEXT := pg_catalog.current_setting(
        'starring.onboarding_created_by_request_digest'
    );
    resource_bindings PG_CATALOG.JSONB :=
        '{"channel_bindings":{},"role_bindings":{}}'::PG_CATALOG.JSONB;
    existing_installation public.automation_installations%ROWTYPE;
    existing_tenant public.product_tenants%ROWTYPE;
    existing_authority
        public.automation_installation_authority_versions%ROWTYPE;
BEGIN
    SELECT installation.*
    INTO existing_installation
    FROM public.automation_installations AS installation
    WHERE installation.installation_id = onboard.installation_id
    FOR UPDATE;

    IF FOUND THEN
        SELECT tenant.*
        INTO existing_tenant
        FROM public.product_tenants AS tenant
        WHERE tenant.tenant_id = onboard.tenant_id
        FOR UPDATE;

        SELECT authority.*
        INTO existing_authority
        FROM public.automation_installation_authority_versions AS authority
        WHERE authority.tenant_id = onboard.tenant_id
            AND authority.installation_id = onboard.installation_id
            AND authority.revision = 1;

        IF existing_tenant.tenant_id IS NULL
            OR existing_tenant.lifecycle_state <> 'active'
            OR existing_tenant.display_name
                <> onboard.tenant_display_name
            OR existing_tenant.display_metadata
                <> '{"environment":"staging","onboarding":"operator_v1"}'
                    ::PG_CATALOG.JSONB
            OR existing_installation.tenant_id <> onboard.tenant_id
            OR existing_installation.discord_application_id
                <> onboard.discord_application_id
            OR existing_installation.discord_guild_id
                <> onboard.discord_guild_id
            OR existing_installation.ruleset_key <> onboard.ruleset_key
            OR existing_installation.lifecycle_state <> 'active'
            OR existing_installation.current_authority_revision <> 1
            OR existing_authority.installation_id IS NULL
            OR existing_authority.tenant_id <> onboard.tenant_id
            OR existing_authority.binding_revision <> 1
            OR existing_authority.resource_bindings
                <> onboard.resource_bindings
            OR existing_authority.binding_fingerprint
                <> onboard.binding_fingerprint
            OR existing_authority.policy_revision <> 1
            OR existing_authority.required_approvals <> 1
            OR existing_authority.activation_ttl_seconds <> 86400
            OR existing_authority.authority_payload_digest
                <> onboard.authority_payload_digest
            OR existing_authority.created_by_principal_id
                <> onboard.created_by_principal_id
            OR existing_authority.created_by_request_digest
                <> onboard.created_by_request_digest
        THEN
            RAISE EXCEPTION 'staging installation onboarding replay conflicts'
                USING ERRCODE = '23505';
        END IF;

        PERFORM pg_catalog.set_config(
            'starring.onboarding_result',
            'exact_replay',
            TRUE
        );
        RETURN;
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM public.product_principals AS principal
        WHERE principal.principal_id = onboard.created_by_principal_id
            AND principal.discord_user_id
                = onboard.created_by_discord_user_id
            AND NOT principal.disabled
    ) OR NOT EXISTS (
        SELECT 1
        FROM public.product_auth_sessions AS actor_session
        WHERE actor_session.principal_id
                = onboard.created_by_principal_id
            AND actor_session.revoked_at IS NULL
            AND pg_catalog.clock_timestamp()
                < actor_session.idle_expires_at
            AND pg_catalog.clock_timestamp()
                < actor_session.absolute_expires_at
            AND pg_catalog.octet_length(
                actor_session.oauth_state_digest
            ) = 32
    ) THEN
        RAISE EXCEPTION 'staging installation onboarding actor is unavailable'
            USING ERRCODE = '42501';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM public.product_tenants AS tenant
        WHERE tenant.tenant_id = onboard.tenant_id
    ) OR EXISTS (
        SELECT 1
        FROM public.automation_installations AS installation
        WHERE (
            installation.discord_application_id
                = onboard.discord_application_id
            AND installation.discord_guild_id = onboard.discord_guild_id
        ) OR (
            installation.discord_guild_id = onboard.discord_guild_id
            AND installation.ruleset_key = onboard.ruleset_key
        )
    ) THEN
        RAISE EXCEPTION 'staging installation onboarding identity conflicts'
            USING ERRCODE = '23505';
    END IF;

    INSERT INTO public.product_tenants (
        tenant_id,
        lifecycle_state,
        display_name,
        display_metadata
    ) VALUES (
        onboard.tenant_id,
        'active',
        onboard.tenant_display_name,
        '{"environment":"staging","onboarding":"operator_v1"}'
            ::PG_CATALOG.JSONB
    );

    INSERT INTO public.automation_installations (
        installation_id,
        tenant_id,
        discord_application_id,
        discord_guild_id,
        ruleset_key,
        lifecycle_state,
        current_authority_revision
    ) VALUES (
        onboard.installation_id,
        onboard.tenant_id,
        onboard.discord_application_id,
        onboard.discord_guild_id,
        onboard.ruleset_key,
        'active',
        1
    );

    INSERT INTO public.automation_installation_authority_versions (
        installation_id,
        revision,
        tenant_id,
        binding_revision,
        resource_bindings,
        binding_fingerprint,
        policy_revision,
        required_approvals,
        activation_ttl_seconds,
        authority_payload_digest,
        created_by_principal_id,
        created_by_request_digest
    ) VALUES (
        onboard.installation_id,
        1,
        onboard.tenant_id,
        1,
        onboard.resource_bindings,
        onboard.binding_fingerprint,
        1,
        1,
        86400,
        onboard.authority_payload_digest,
        onboard.created_by_principal_id,
        onboard.created_by_request_digest
    );

    PERFORM pg_catalog.set_config(
        'starring.onboarding_result',
        'created',
        TRUE
    );
END;
$onboard$;

SET CONSTRAINTS ALL IMMEDIATE;

DO $verify$
<<verify>>
DECLARE
    tenant_id TEXT := pg_catalog.current_setting(
        'starring.onboarding_tenant_id'
    );
    installation_id TEXT := pg_catalog.current_setting(
        'starring.onboarding_installation_id'
    );
    discord_guild_id TEXT := pg_catalog.current_setting(
        'starring.onboarding_discord_guild_id'
    );
    ruleset_key TEXT := pg_catalog.current_setting(
        'starring.onboarding_ruleset_key'
    );
    onboarding_result TEXT := pg_catalog.current_setting(
        'starring.onboarding_result'
    );
BEGIN
    IF (
        SELECT pg_catalog.count(*)
        FROM public.product_tenants AS tenant
        WHERE tenant.tenant_id = verify.tenant_id
            AND tenant.lifecycle_state = 'active'
    ) <> 1 OR (
        SELECT pg_catalog.count(*)
        FROM public.automation_installations AS installation
        WHERE installation.tenant_id = verify.tenant_id
            AND installation.installation_id = verify.installation_id
            AND installation.lifecycle_state = 'active'
            AND installation.current_authority_revision = 1
    ) <> 1 OR (
        SELECT pg_catalog.count(*)
        FROM public.automation_installation_authority_versions AS authority
        WHERE authority.tenant_id = verify.tenant_id
            AND authority.installation_id = verify.installation_id
            AND authority.revision = 1
    ) <> 1 OR onboarding_result NOT IN ('created', 'exact_replay') OR (
        onboarding_result = 'created'
        AND (
            SELECT pg_catalog.count(*)
            FROM public.runtime_slot_writer_fences_v2 AS fence
            WHERE fence.slot_guild_id = discord_guild_id
                AND fence.slot_ruleset_key = ruleset_key
                AND fence.writer_epoch = 1
                AND fence.pending_drain_intent_id IS NULL
                AND fence.pending_product_operation_id IS NULL
                AND fence.pending_tenant_id IS NULL
                AND fence.pending_installation_id IS NULL
                AND fence.pending_deployment_id IS NULL
                AND fence.pending_expected_revision IS NULL
                AND fence.pending_marked_at IS NULL
        ) <> 1
    ) OR (
        onboarding_result = 'exact_replay'
        AND (
            SELECT pg_catalog.count(*)
            FROM public.runtime_slot_writer_fences_v2 AS fence
            WHERE fence.slot_guild_id = discord_guild_id
                AND fence.slot_ruleset_key = ruleset_key
                AND fence.writer_epoch BETWEEN 1 AND 9223372036854775807
                AND pg_catalog.isfinite(fence.updated_at)
                AND (
                    (
                        fence.pending_drain_intent_id IS NULL
                        AND fence.pending_product_operation_id IS NULL
                        AND fence.pending_tenant_id IS NULL
                        AND fence.pending_installation_id IS NULL
                        AND fence.pending_deployment_id IS NULL
                        AND fence.pending_expected_revision IS NULL
                        AND fence.pending_marked_at IS NULL
                    ) OR (
                        fence.pending_drain_intent_id IS NOT NULL
                        AND fence.pending_product_operation_id IS NOT NULL
                        AND fence.pending_tenant_id IS NOT NULL
                        AND fence.pending_installation_id IS NOT NULL
                        AND fence.pending_deployment_id IS NOT NULL
                        AND fence.pending_expected_revision IS NOT NULL
                        AND fence.pending_marked_at IS NOT NULL
                    )
                )
        ) <> 1
    ) THEN
        RAISE EXCEPTION 'staging installation onboarding verification failed'
            USING ERRCODE = '55000';
    END IF;
END;
$verify$;

RESET ROLE;

SELECT
    pg_catalog.current_setting('starring.onboarding_result') AS result,
    :'commit_onboarding'::PG_CATALOG.BOOL AS commit_requested,
    1::PG_CATALOG.INT8 AS tenant_rows,
    1::PG_CATALOG.INT8 AS installation_rows,
    1::PG_CATALOG.INT8 AS authority_rows,
    1::PG_CATALOG.INT8 AS writer_fence_rows;

\if :commit_onboarding
COMMIT;
\else
ROLLBACK;
\endif
