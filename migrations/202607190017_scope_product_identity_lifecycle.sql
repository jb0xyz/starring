CREATE TABLE public.product_control_plane_identity (
    singleton BOOLEAN PRIMARY KEY,
    database_identity UUID NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT product_control_plane_identity_singleton CHECK (singleton),
    CONSTRAINT product_control_plane_identity_nonzero CHECK (
        database_identity <> '00000000-0000-0000-0000-000000000000'::UUID
    )
);

INSERT INTO public.product_control_plane_identity (
    singleton,
    database_identity,
    created_at
) VALUES (
    TRUE,
    pg_catalog.gen_random_uuid(),
    pg_catalog.clock_timestamp()
);

CREATE FUNCTION public.starring_product_oauth_database_identity_v1()
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
    WHERE identity.singleton;
$function$;

CREATE FUNCTION public.starring_product_session_issuer_database_identity_v1()
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
    WHERE identity.singleton;
$function$;

CREATE FUNCTION public.starring_product_session_api_database_identity_v1()
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
    WHERE identity.singleton;
$function$;

CREATE FUNCTION public.starring_product_security_revoker_database_identity_v1()
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
    WHERE identity.singleton;
$function$;

CREATE FUNCTION public.starring_product_oauth_flow_create_v1(
    new_state_digest BYTEA,
    new_browser_nonce_digest BYTEA,
    expected_redirect_uri TEXT,
    expected_return_path TEXT,
    flow_lifetime_seconds DOUBLE PRECISION
)
RETURNS TABLE (
    outcome_code TEXT,
    redirect_uri TEXT,
    return_path TEXT,
    expires_at TIMESTAMPTZ,
    database_now TIMESTAMPTZ
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
    request_now TIMESTAMPTZ;
    inserted_expires_at TIMESTAMPTZ;
    conflicting_flow public.product_oauth_flows%ROWTYPE;
    conflicting_flow_count INTEGER := 0;
    exact_flow BOOLEAN := FALSE;
    exact_expires_at TIMESTAMPTZ;
BEGIN
    IF pg_catalog.octet_length(new_state_digest) <> 32
        OR pg_catalog.octet_length(new_browser_nonce_digest) <> 32
        OR new_state_digest = new_browser_nonce_digest
        OR pg_catalog.char_length(expected_redirect_uri) NOT BETWEEN 1 AND 2048
        OR expected_redirect_uri <> pg_catalog.btrim(expected_redirect_uri)
        OR expected_redirect_uri NOT LIKE 'https://%'
        OR expected_redirect_uri ~ '[[:space:][:cntrl:]]'
        OR pg_catalog.strpos(expected_redirect_uri, '#') <> 0
        OR pg_catalog.char_length(expected_return_path) NOT BETWEEN 1 AND 256
        OR expected_return_path !~ '^/[A-Za-z0-9/_-]*$'
        OR pg_catalog.strpos(expected_return_path, '//') <> 0
        OR expected_return_path ~ '(^|/)[.][.](/|$)'
        OR (expected_return_path <> '/'
            AND pg_catalog.right(expected_return_path, 1) = '/')
        OR flow_lifetime_seconds < 1
        OR flow_lifetime_seconds > 600
    THEN
        RETURN QUERY SELECT 'invalid_request'::TEXT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::TIMESTAMPTZ,
            NULL::TIMESTAMPTZ;
        RETURN;
    END IF;

    request_now := pg_catalog.clock_timestamp();
    INSERT INTO public.product_oauth_flows (
        state_digest,
        browser_nonce_digest,
        redirect_uri,
        return_path,
        created_at,
        expires_at
    ) VALUES (
        new_state_digest,
        new_browser_nonce_digest,
        expected_redirect_uri,
        expected_return_path,
        request_now,
        request_now + pg_catalog.make_interval(secs => flow_lifetime_seconds)
    )
    ON CONFLICT DO NOTHING
    RETURNING product_oauth_flows.expires_at
    INTO inserted_expires_at;

    IF inserted_expires_at IS NOT NULL THEN
        RETURN QUERY SELECT 'created'::TEXT,
            expected_redirect_uri,
            expected_return_path,
            inserted_expires_at,
            request_now;
        RETURN;
    END IF;

    FOR conflicting_flow IN
        SELECT oauth_flow.*
        FROM public.product_oauth_flows AS oauth_flow
        WHERE oauth_flow.state_digest = new_state_digest
            OR oauth_flow.browser_nonce_digest = new_browser_nonce_digest
        ORDER BY oauth_flow.state_digest
        FOR UPDATE
    LOOP
        conflicting_flow_count := conflicting_flow_count + 1;
        IF conflicting_flow.state_digest = new_state_digest
            AND conflicting_flow.browser_nonce_digest = new_browser_nonce_digest
            AND conflicting_flow.redirect_uri = expected_redirect_uri
            AND conflicting_flow.return_path = expected_return_path
            AND conflicting_flow.consumed_at IS NULL
            AND conflicting_flow.terminal_result_code IS NULL
            AND conflicting_flow.expires_at
                <= conflicting_flow.created_at + INTERVAL '10 minutes'
        THEN
            exact_flow := TRUE;
            exact_expires_at := conflicting_flow.expires_at;
        END IF;
    END LOOP;

    request_now := pg_catalog.clock_timestamp();
    IF conflicting_flow_count = 1
        AND exact_flow
        AND exact_expires_at > request_now
    THEN
        RETURN QUERY SELECT 'exact_replay'::TEXT,
            expected_redirect_uri,
            expected_return_path,
            exact_expires_at,
            request_now;
    ELSE
        RETURN QUERY SELECT 'digest_conflict'::TEXT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::TIMESTAMPTZ,
            request_now;
    END IF;
END;
$function$;

CREATE FUNCTION public.starring_product_oauth_flow_consume_v1(
    expected_state_digest BYTEA,
    expected_browser_nonce_digest BYTEA,
    expected_redirect_uri TEXT,
    allowed_return_paths TEXT[]
)
RETURNS TABLE (
    outcome_code TEXT,
    redirect_uri TEXT,
    return_path TEXT,
    consumed_at TIMESTAMPTZ
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
    claimed_flow public.product_oauth_flows%ROWTYPE;
    claim_now TIMESTAMPTZ;
    persisted_consumed_at TIMESTAMPTZ;
BEGIN
    IF pg_catalog.octet_length(expected_state_digest) <> 32
        OR pg_catalog.octet_length(expected_browser_nonce_digest) <> 32
        OR expected_state_digest = expected_browser_nonce_digest
        OR pg_catalog.array_ndims(allowed_return_paths) <> 1
        OR pg_catalog.cardinality(allowed_return_paths) NOT BETWEEN 1 AND 64
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.unnest(allowed_return_paths) AS allowed(path)
            WHERE allowed.path IS NULL
                OR pg_catalog.char_length(allowed.path) NOT BETWEEN 1 AND 256
                OR allowed.path !~ '^/[A-Za-z0-9/_-]*$'
                OR pg_catalog.strpos(allowed.path, '//') <> 0
                OR allowed.path ~ '(^|/)[.][.](/|$)'
                OR (allowed.path <> '/' AND pg_catalog.right(allowed.path, 1) = '/')
        )
    THEN
        RETURN QUERY SELECT 'invalid_or_consumed'::TEXT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::TIMESTAMPTZ;
        RETURN;
    END IF;

    SELECT oauth_flow.*
    INTO claimed_flow
    FROM public.product_oauth_flows AS oauth_flow
    WHERE oauth_flow.state_digest = expected_state_digest
        AND oauth_flow.browser_nonce_digest = expected_browser_nonce_digest
        AND oauth_flow.redirect_uri = expected_redirect_uri
        AND oauth_flow.return_path = ANY(allowed_return_paths)
        AND oauth_flow.expires_at
            <= oauth_flow.created_at + INTERVAL '10 minutes'
        AND oauth_flow.consumed_at IS NULL
        AND oauth_flow.terminal_result_code IS NULL
    FOR UPDATE;

    IF NOT FOUND THEN
        RETURN QUERY SELECT 'invalid_or_consumed'::TEXT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::TIMESTAMPTZ;
        RETURN;
    END IF;

    claim_now := pg_catalog.clock_timestamp();
    IF claim_now >= claimed_flow.expires_at THEN
        RETURN QUERY SELECT 'invalid_or_consumed'::TEXT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::TIMESTAMPTZ;
        RETURN;
    END IF;

    UPDATE public.product_oauth_flows AS oauth_flow
    SET consumed_at = claim_now,
        terminal_result_code = 'callback_claimed'
    WHERE oauth_flow.state_digest = expected_state_digest
        AND oauth_flow.consumed_at IS NULL
        AND oauth_flow.terminal_result_code IS NULL
    RETURNING oauth_flow.consumed_at
    INTO persisted_consumed_at;

    IF persisted_consumed_at IS NULL THEN
        RETURN QUERY SELECT 'invariant'::TEXT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::TIMESTAMPTZ;
        RETURN;
    END IF;

    RETURN QUERY SELECT 'claimed'::TEXT,
        claimed_flow.redirect_uri,
        claimed_flow.return_path,
        persisted_consumed_at;
END;
$function$;

CREATE FUNCTION public.starring_product_session_issue_v1(
    expected_oauth_state_digest BYTEA,
    expected_redirect_uri TEXT,
    expected_return_path TEXT,
    expected_consumed_at TIMESTAMPTZ,
    verified_discord_user_id TEXT,
    verified_display_name TEXT,
    new_session_digest BYTEA,
    new_csrf_digest BYTEA,
    idle_lifetime_seconds DOUBLE PRECISION,
    absolute_lifetime_seconds DOUBLE PRECISION
)
RETURNS TABLE (
    outcome_code TEXT,
    principal_id TEXT,
    discord_user_id TEXT,
    identity_revision BIGINT,
    display_profile JSONB,
    idle_expires_at TIMESTAMPTZ,
    absolute_expires_at TIMESTAMPTZ,
    database_now TIMESTAMPTZ
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
    canonical_principal_id TEXT;
    locked_flow public.product_oauth_flows%ROWTYPE;
    existing_session public.product_auth_sessions%ROWTYPE;
    existing_principal public.product_principals%ROWTYPE;
    persisted_principal public.product_principals%ROWTYPE;
    principal_now TIMESTAMPTZ;
    issue_now TIMESTAMPTZ;
    persisted_idle_expires_at TIMESTAMPTZ;
    persisted_absolute_expires_at TIMESTAMPTZ;
    failure_constraint TEXT;
BEGIN
    IF pg_catalog.octet_length(expected_oauth_state_digest) <> 32
        OR pg_catalog.octet_length(new_session_digest) <> 32
        OR pg_catalog.octet_length(new_csrf_digest) <> 32
        OR new_session_digest = new_csrf_digest
        OR new_session_digest = expected_oauth_state_digest
        OR new_csrf_digest = expected_oauth_state_digest
        OR verified_discord_user_id !~ '^[1-9][0-9]{0,19}$'
        OR verified_discord_user_id::NUMERIC > 18446744073709551615
        OR pg_catalog.octet_length(verified_display_name) NOT BETWEEN 1 AND 512
        OR pg_catalog.char_length(verified_display_name) NOT BETWEEN 1 AND 128
        OR verified_display_name <> pg_catalog.btrim(verified_display_name)
        OR verified_display_name ~ '[[:cntrl:]]'
        OR idle_lifetime_seconds <= 0
        OR idle_lifetime_seconds > 1800
        OR absolute_lifetime_seconds < 1
        OR absolute_lifetime_seconds > 43200
        OR idle_lifetime_seconds > absolute_lifetime_seconds
    THEN
        RETURN QUERY SELECT 'invalid_request'::TEXT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::JSONB,
            NULL::TIMESTAMPTZ,
            NULL::TIMESTAMPTZ,
            NULL::TIMESTAMPTZ;
        RETURN;
    END IF;

    canonical_principal_id := 'discord:' || verified_discord_user_id;
    SELECT oauth_flow.*
    INTO locked_flow
    FROM public.product_oauth_flows AS oauth_flow
    WHERE oauth_flow.state_digest = expected_oauth_state_digest
        AND oauth_flow.redirect_uri = expected_redirect_uri
        AND oauth_flow.return_path = expected_return_path
        AND oauth_flow.consumed_at = expected_consumed_at
        AND oauth_flow.terminal_result_code = 'callback_claimed'
        AND oauth_flow.expires_at
            <= oauth_flow.created_at + INTERVAL '10 minutes'
    FOR UPDATE;

    IF NOT FOUND THEN
        RETURN QUERY SELECT 'flow_invalid_or_consumed'::TEXT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::JSONB,
            NULL::TIMESTAMPTZ,
            NULL::TIMESTAMPTZ,
            NULL::TIMESTAMPTZ;
        RETURN;
    END IF;

    issue_now := pg_catalog.clock_timestamp();
    IF locked_flow.consumed_at > issue_now OR issue_now >= locked_flow.expires_at THEN
        RETURN QUERY SELECT 'flow_invalid_or_consumed'::TEXT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::JSONB,
            NULL::TIMESTAMPTZ,
            NULL::TIMESTAMPTZ,
            issue_now;
        RETURN;
    END IF;

    SELECT authentication_session.*
    INTO existing_session
    FROM public.product_auth_sessions AS authentication_session
    WHERE authentication_session.oauth_state_digest = expected_oauth_state_digest
    FOR SHARE;

    IF FOUND THEN
        IF existing_session.session_digest <> new_session_digest
            OR existing_session.csrf_digest <> new_csrf_digest
            OR existing_session.principal_id <> canonical_principal_id
        THEN
            RETURN QUERY SELECT 'flow_invalid_or_consumed'::TEXT,
                NULL::TEXT,
                NULL::TEXT,
                NULL::BIGINT,
                NULL::JSONB,
                NULL::TIMESTAMPTZ,
                NULL::TIMESTAMPTZ,
                issue_now;
            RETURN;
        END IF;

        SELECT principal.*
        INTO existing_principal
        FROM public.product_principals AS principal
        WHERE principal.principal_id = existing_session.principal_id
        FOR SHARE;
        IF NOT FOUND
            OR existing_principal.discord_user_id <> verified_discord_user_id
            OR existing_principal.principal_id <> canonical_principal_id
            OR existing_principal.identity_revision < 1
            OR pg_catalog.jsonb_typeof(existing_principal.display_profile) <> 'object'
            OR pg_catalog.octet_length(existing_principal.display_profile::TEXT) > 16384
            OR existing_session.authenticated_at <> existing_session.created_at
            OR existing_session.created_at <> existing_session.last_seen_at
            OR existing_session.idle_expires_at <= existing_session.last_seen_at
            OR existing_session.idle_expires_at > existing_session.absolute_expires_at
            OR existing_session.idle_expires_at
                > existing_session.last_seen_at + INTERVAL '30 minutes'
            OR existing_session.absolute_expires_at
                > existing_session.authenticated_at + INTERVAL '12 hours'
            OR existing_session.revoked_at IS NOT NULL
            OR existing_session.revocation_reason IS NOT NULL
            OR existing_session.idle_expires_at
                <> existing_session.created_at
                    + pg_catalog.make_interval(secs => idle_lifetime_seconds)
            OR existing_session.absolute_expires_at
                <> existing_session.created_at
                    + pg_catalog.make_interval(secs => absolute_lifetime_seconds)
        THEN
            RETURN QUERY SELECT 'invariant'::TEXT,
                NULL::TEXT,
                NULL::TEXT,
                NULL::BIGINT,
                NULL::JSONB,
                NULL::TIMESTAMPTZ,
                NULL::TIMESTAMPTZ,
                issue_now;
            RETURN;
        END IF;
        IF existing_principal.disabled THEN
            RETURN QUERY SELECT 'principal_disabled'::TEXT,
                NULL::TEXT,
                NULL::TEXT,
                NULL::BIGINT,
                NULL::JSONB,
                NULL::TIMESTAMPTZ,
                NULL::TIMESTAMPTZ,
                issue_now;
            RETURN;
        END IF;
        RETURN QUERY SELECT 'exact_replay'::TEXT,
            existing_principal.principal_id,
            existing_principal.discord_user_id,
            existing_principal.identity_revision,
            existing_principal.display_profile,
            existing_session.idle_expires_at,
            existing_session.absolute_expires_at,
            issue_now;
        RETURN;
    END IF;

    IF EXISTS (
        SELECT 1
        FROM public.product_auth_sessions AS authentication_session
        WHERE authentication_session.session_digest = new_session_digest
            OR authentication_session.csrf_digest = new_csrf_digest
    ) THEN
        RETURN QUERY SELECT 'digest_conflict'::TEXT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::JSONB,
            NULL::TIMESTAMPTZ,
            NULL::TIMESTAMPTZ,
            issue_now;
        RETURN;
    END IF;

    BEGIN
        principal_now := pg_catalog.clock_timestamp();
        INSERT INTO public.product_principals AS principal_record (
            principal_id,
            discord_user_id,
            display_profile,
            last_authenticated_at,
            updated_at
        ) VALUES (
            canonical_principal_id,
            verified_discord_user_id,
            pg_catalog.jsonb_build_object('display_name', verified_display_name),
            principal_now,
            principal_now
        )
        ON CONFLICT ON CONSTRAINT product_principals_discord_user_id_key DO UPDATE SET
            identity_revision = principal_record.identity_revision + 1,
            display_profile = excluded.display_profile,
            last_authenticated_at = GREATEST(
                excluded.last_authenticated_at,
                principal_record.updated_at + INTERVAL '1 microsecond'
            ),
            updated_at = GREATEST(
                excluded.updated_at,
                principal_record.updated_at + INTERVAL '1 microsecond'
            )
        WHERE NOT principal_record.disabled
            AND principal_record.principal_id = canonical_principal_id
        RETURNING principal_record.*
        INTO persisted_principal;

        IF persisted_principal.principal_id IS NULL THEN
            SELECT principal.*
            INTO existing_principal
            FROM public.product_principals AS principal
            WHERE principal.discord_user_id = verified_discord_user_id
            FOR UPDATE;
            IF FOUND AND existing_principal.disabled
                AND existing_principal.principal_id = canonical_principal_id
            THEN
                RETURN QUERY SELECT 'principal_disabled'::TEXT,
                    NULL::TEXT,
                    NULL::TEXT,
                    NULL::BIGINT,
                    NULL::JSONB,
                    NULL::TIMESTAMPTZ,
                    NULL::TIMESTAMPTZ,
                    issue_now;
            ELSE
                RETURN QUERY SELECT 'invariant'::TEXT,
                    NULL::TEXT,
                    NULL::TEXT,
                    NULL::BIGINT,
                    NULL::JSONB,
                    NULL::TIMESTAMPTZ,
                    NULL::TIMESTAMPTZ,
                    issue_now;
            END IF;
            RETURN;
        END IF;

        issue_now := pg_catalog.clock_timestamp();
        IF issue_now >= locked_flow.expires_at THEN
            RAISE EXCEPTION USING
                ERRCODE = 'P1001',
                MESSAGE = 'product OAuth flow expired during session issue';
        END IF;

        INSERT INTO public.product_auth_sessions (
            session_digest,
            principal_id,
            csrf_digest,
            oauth_state_digest,
            authenticated_at,
            created_at,
            last_seen_at,
            idle_expires_at,
            absolute_expires_at
        ) VALUES (
            new_session_digest,
            persisted_principal.principal_id,
            new_csrf_digest,
            expected_oauth_state_digest,
            issue_now,
            issue_now,
            issue_now,
            issue_now + pg_catalog.make_interval(secs => idle_lifetime_seconds),
            issue_now + pg_catalog.make_interval(secs => absolute_lifetime_seconds)
        )
        RETURNING product_auth_sessions.idle_expires_at,
            product_auth_sessions.absolute_expires_at
        INTO persisted_idle_expires_at, persisted_absolute_expires_at;
    EXCEPTION
        WHEN SQLSTATE 'P1001' THEN
            RETURN QUERY SELECT 'flow_invalid_or_consumed'::TEXT,
                NULL::TEXT,
                NULL::TEXT,
                NULL::BIGINT,
                NULL::JSONB,
                NULL::TIMESTAMPTZ,
                NULL::TIMESTAMPTZ,
                issue_now;
            RETURN;
        WHEN unique_violation THEN
            GET STACKED DIAGNOSTICS failure_constraint = CONSTRAINT_NAME;
            IF failure_constraint IN (
                'product_auth_sessions_pkey',
                'product_auth_sessions_csrf_digest_key'
            ) THEN
                RETURN QUERY SELECT 'digest_conflict'::TEXT,
                    NULL::TEXT,
                    NULL::TEXT,
                    NULL::BIGINT,
                    NULL::JSONB,
                    NULL::TIMESTAMPTZ,
                    NULL::TIMESTAMPTZ,
                    issue_now;
            ELSIF failure_constraint = 'product_auth_sessions_oauth_state_unique' THEN
                RETURN QUERY SELECT 'flow_invalid_or_consumed'::TEXT,
                    NULL::TEXT,
                    NULL::TEXT,
                    NULL::BIGINT,
                    NULL::JSONB,
                    NULL::TIMESTAMPTZ,
                    NULL::TIMESTAMPTZ,
                    issue_now;
            ELSE
                RETURN QUERY SELECT 'invariant'::TEXT,
                    NULL::TEXT,
                    NULL::TEXT,
                    NULL::BIGINT,
                    NULL::JSONB,
                    NULL::TIMESTAMPTZ,
                    NULL::TIMESTAMPTZ,
                    issue_now;
            END IF;
            RETURN;
        WHEN foreign_key_violation OR check_violation THEN
            GET STACKED DIAGNOSTICS failure_constraint = CONSTRAINT_NAME;
            IF failure_constraint IN (
                'product_auth_sessions_oauth_state_fk',
                'product_auth_sessions_oauth_binding_valid'
            ) THEN
                RETURN QUERY SELECT 'flow_invalid_or_consumed'::TEXT,
                    NULL::TEXT,
                    NULL::TEXT,
                    NULL::BIGINT,
                    NULL::JSONB,
                    NULL::TIMESTAMPTZ,
                    NULL::TIMESTAMPTZ,
                    issue_now;
            ELSE
                RETURN QUERY SELECT 'invariant'::TEXT,
                    NULL::TEXT,
                    NULL::TEXT,
                    NULL::BIGINT,
                    NULL::JSONB,
                    NULL::TIMESTAMPTZ,
                    NULL::TIMESTAMPTZ,
                    issue_now;
            END IF;
            RETURN;
    END;

    IF persisted_principal.principal_id <> canonical_principal_id
        OR persisted_principal.discord_user_id <> verified_discord_user_id
        OR persisted_principal.identity_revision < 1
        OR persisted_idle_expires_at IS NULL
        OR persisted_absolute_expires_at IS NULL
    THEN
        RETURN QUERY SELECT 'invariant'::TEXT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::JSONB,
            NULL::TIMESTAMPTZ,
            NULL::TIMESTAMPTZ,
            issue_now;
        RETURN;
    END IF;

    RETURN QUERY SELECT 'issued'::TEXT,
        persisted_principal.principal_id,
        persisted_principal.discord_user_id,
        persisted_principal.identity_revision,
        persisted_principal.display_profile,
        persisted_idle_expires_at,
        persisted_absolute_expires_at,
        issue_now;
END;
$function$;

CREATE FUNCTION public.starring_product_session_logout_read_v1(
    expected_session_digest BYTEA
)
RETURNS TABLE (
    csrf_digest_length INTEGER,
    oauth_state_digest_length INTEGER,
    csrf_comparison_tag BYTEA,
    last_seen_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    revocation_reason TEXT
)
LANGUAGE sql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
    SELECT pg_catalog.octet_length(authentication_session.csrf_digest),
        pg_catalog.octet_length(authentication_session.oauth_state_digest),
        pg_catalog.sha256(pg_catalog.byteacat(
            expected_session_digest,
            authentication_session.csrf_digest
        )),
        authentication_session.last_seen_at,
        authentication_session.revoked_at,
        authentication_session.revocation_reason
    FROM public.product_auth_sessions AS authentication_session
    WHERE authentication_session.session_digest = expected_session_digest
        AND pg_catalog.octet_length(expected_session_digest) = 32
    FOR UPDATE;
$function$;

CREATE FUNCTION public.starring_product_session_logout_commit_v1(
    expected_session_digest BYTEA,
    expected_csrf_comparison_tag BYTEA,
    observed_last_seen_at TIMESTAMPTZ
)
RETURNS BIGINT
LANGUAGE sql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
    WITH locked_session AS MATERIALIZED (
        SELECT authentication_session.session_digest
        FROM public.product_auth_sessions AS authentication_session
        WHERE authentication_session.session_digest = expected_session_digest
            AND pg_catalog.octet_length(expected_session_digest) = 32
            AND pg_catalog.octet_length(expected_csrf_comparison_tag) = 32
            AND pg_catalog.octet_length(authentication_session.csrf_digest) = 32
            AND pg_catalog.octet_length(authentication_session.oauth_state_digest) = 32
            AND pg_catalog.sha256(pg_catalog.byteacat(
                expected_session_digest,
                authentication_session.csrf_digest
            )) = expected_csrf_comparison_tag
            AND authentication_session.last_seen_at = observed_last_seen_at
            AND authentication_session.revoked_at IS NULL
        FOR UPDATE
    ), revocation_clock AS MATERIALIZED (
        SELECT pg_catalog.clock_timestamp() AS revoked_at
        FROM locked_session
    ), revoked AS (
        UPDATE public.product_auth_sessions AS authentication_session
        SET revoked_at = GREATEST(
                revocation_clock.revoked_at,
                authentication_session.last_seen_at
            ),
            revocation_reason = 'user_logout'
        FROM locked_session, revocation_clock
        WHERE authentication_session.session_digest = locked_session.session_digest
            AND authentication_session.revoked_at IS NULL
            AND authentication_session.last_seen_at = observed_last_seen_at
        RETURNING 1
    )
    SELECT pg_catalog.count(*) FROM revoked;
$function$;

CREATE FUNCTION public.starring_product_session_security_revoke_v1(
    expected_session_digest BYTEA
)
RETURNS TABLE (outcome_code TEXT)
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
DECLARE
    locked_session public.product_auth_sessions%ROWTYPE;
    revocation_now TIMESTAMPTZ;
    revoked_count BIGINT;
BEGIN
    IF pg_catalog.octet_length(expected_session_digest) <> 32 THEN
        RETURN QUERY SELECT 'invalid_credential'::TEXT;
        RETURN;
    END IF;

    SELECT authentication_session.*
    INTO locked_session
    FROM public.product_auth_sessions AS authentication_session
    WHERE authentication_session.session_digest = expected_session_digest
    FOR UPDATE;

    IF NOT FOUND THEN
        RETURN QUERY SELECT 'invalid_credential'::TEXT;
        RETURN;
    END IF;
    IF locked_session.revoked_at IS NOT NULL THEN
        IF locked_session.revocation_reason = 'security_revocation' THEN
            RETURN QUERY SELECT 'exact_replay'::TEXT;
        ELSE
            RETURN QUERY SELECT 'already_revoked'::TEXT;
        END IF;
        RETURN;
    END IF;
    IF pg_catalog.octet_length(locked_session.oauth_state_digest) <> 32 THEN
        RETURN QUERY SELECT 'invariant'::TEXT;
        RETURN;
    END IF;

    revocation_now := pg_catalog.clock_timestamp();
    UPDATE public.product_auth_sessions AS authentication_session
    SET revoked_at = GREATEST(
            revocation_now,
            authentication_session.last_seen_at
        ),
        revocation_reason = 'security_revocation'
    WHERE authentication_session.session_digest = expected_session_digest
        AND authentication_session.revoked_at IS NULL;
    GET DIAGNOSTICS revoked_count = ROW_COUNT;
    IF revoked_count <> 1 THEN
        RETURN QUERY SELECT 'invariant'::TEXT;
        RETURN;
    END IF;
    RETURN QUERY SELECT 'revoked'::TEXT;
END;
$function$;

REVOKE ALL ON TABLE public.product_control_plane_identity FROM PUBLIC;
REVOKE ALL ON FUNCTION public.starring_product_oauth_database_identity_v1()
FROM PUBLIC;
REVOKE ALL ON FUNCTION public.starring_product_session_issuer_database_identity_v1()
FROM PUBLIC;
REVOKE ALL ON FUNCTION public.starring_product_session_api_database_identity_v1()
FROM PUBLIC;
REVOKE ALL ON FUNCTION public.starring_product_security_revoker_database_identity_v1()
FROM PUBLIC;
REVOKE ALL ON FUNCTION public.starring_product_oauth_flow_create_v1(
    BYTEA,
    BYTEA,
    TEXT,
    TEXT,
    DOUBLE PRECISION
) FROM PUBLIC;
REVOKE ALL ON FUNCTION public.starring_product_oauth_flow_consume_v1(
    BYTEA,
    BYTEA,
    TEXT,
    TEXT[]
) FROM PUBLIC;
REVOKE ALL ON FUNCTION public.starring_product_session_issue_v1(
    BYTEA,
    TEXT,
    TEXT,
    TIMESTAMPTZ,
    TEXT,
    TEXT,
    BYTEA,
    BYTEA,
    DOUBLE PRECISION,
    DOUBLE PRECISION
) FROM PUBLIC;
REVOKE ALL ON FUNCTION public.starring_product_session_logout_read_v1(BYTEA)
FROM PUBLIC;
REVOKE ALL ON FUNCTION public.starring_product_session_logout_commit_v1(
    BYTEA,
    BYTEA,
    TIMESTAMPTZ
) FROM PUBLIC;
REVOKE ALL ON FUNCTION public.starring_product_session_security_revoke_v1(BYTEA)
FROM PUBLIC;
REVOKE ALL ON FUNCTION public.enforce_product_principal_transition()
FROM PUBLIC;
REVOKE ALL ON FUNCTION public.enforce_product_oauth_flow_transition()
FROM PUBLIC;
REVOKE ALL ON FUNCTION public.enforce_product_auth_session_oauth_binding()
FROM PUBLIC;
REVOKE ALL ON FUNCTION public.enforce_product_auth_session_transition()
FROM PUBLIC;
REVOKE ALL ON FUNCTION public.starring_purge_product_identity_v1(INTEGER)
FROM PUBLIC;

DO $ownership$
DECLARE
    relation_count BIGINT;
    table_count BIGINT;
    rls_disabled_count BIGINT;
    owner_count BIGINT;
    common_owner OID;
    common_owner_name NAME;
    expected_relation TEXT;
    relation_oid OID;
    relation_owner OID;
    expected_signature TEXT;
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
            (pg_catalog.to_regclass('public.product_oauth_flows')),
            (pg_catalog.to_regclass('public.product_principals')),
            (pg_catalog.to_regclass('public.product_auth_sessions'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid;

    IF relation_count <> 3
        OR table_count <> 3
        OR rls_disabled_count <> 3
        OR owner_count <> 1
        OR common_owner IS NULL
    THEN
        RAISE EXCEPTION 'product identity relations require one non-RLS owner'
            USING ERRCODE = '55000';
    END IF;

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL THEN
        RAISE EXCEPTION 'product identity relation owner is unavailable'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO relation_count
    FROM public.product_control_plane_identity AS identity
    WHERE identity.singleton
        AND identity.database_identity IS NOT NULL
        AND identity.database_identity
            <> '00000000-0000-0000-0000-000000000000'::UUID
        AND identity.created_at IS NOT NULL;
    IF relation_count <> 1 THEN
        RAISE EXCEPTION 'product control plane identity is invalid'
            USING ERRCODE = '55000';
    END IF;

    FOR expected_relation IN
        SELECT expected.identity
        FROM (
            VALUES
                ('public.product_control_plane_identity')
        ) AS expected(identity)
    LOOP
        relation_oid := pg_catalog.to_regclass(expected_relation);
        SELECT relation.relowner
        INTO relation_owner
        FROM pg_catalog.pg_class AS relation
        WHERE relation.oid = relation_oid
            AND relation.relkind = 'r'
            AND NOT relation.relrowsecurity
            AND NOT relation.relforcerowsecurity;
        IF relation_owner IS NULL THEN
            RAISE EXCEPTION 'product identity relation is invalid'
                USING ERRCODE = '55000';
        END IF;
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON TABLE %s FROM PUBLIC CASCADE',
            expected_relation
        );
        FOR unexpected_grantee IN
            SELECT DISTINCT grant_entry.grantee
            FROM (
                SELECT privilege.grantee, relation.relowner
                FROM pg_catalog.pg_class AS relation
                CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                    relation.relacl,
                    pg_catalog.acldefault('r', relation.relowner)
                )) AS privilege
                WHERE relation.oid = relation_oid
                UNION ALL
                SELECT privilege.grantee, relation.relowner
                FROM pg_catalog.pg_class AS relation
                INNER JOIN pg_catalog.pg_attribute AS attribute
                    ON attribute.attrelid = relation.oid
                    AND attribute.attnum > 0
                    AND NOT attribute.attisdropped
                CROSS JOIN LATERAL pg_catalog.aclexplode(
                    NULLIF(attribute.attacl, '{}'::ACLITEM[])
                ) AS privilege
                WHERE relation.oid = relation_oid
            ) AS grant_entry
            WHERE grant_entry.grantee <> 0
                AND grant_entry.grantee <> grant_entry.relowner
                AND grant_entry.grantee <> common_owner
        LOOP
            unexpected_grantee_name := pg_catalog.pg_get_userbyid(unexpected_grantee);
            IF unexpected_grantee_name IS NULL THEN
                RAISE EXCEPTION 'product identity relation grantee is unavailable'
                    USING ERRCODE = '55000';
            END IF;
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON TABLE %s FROM %I CASCADE',
                expected_relation,
                unexpected_grantee_name
            );
        END LOOP;
        EXECUTE pg_catalog.format(
            'ALTER TABLE %s OWNER TO %I',
            expected_relation,
            common_owner_name
        );
    END LOOP;

    FOR expected_signature IN
        SELECT expected.signature
        FROM (
            VALUES
                ('public.starring_product_session_read_v1(bytea)'),
                ('public.starring_product_session_mutation_read_v1(bytea)'),
                ('public.starring_product_session_touch_v1(bytea,timestamp with time zone,timestamp with time zone,timestamp with time zone,double precision)')
        ) AS expected(signature)
    LOOP
        function_oid := pg_catalog.to_regprocedure(expected_signature);
        IF function_oid IS NULL
            OR (SELECT function_row.proowner FROM pg_catalog.pg_proc AS function_row
                WHERE function_row.oid = function_oid) <> common_owner
        THEN
            RAISE EXCEPTION 'existing authentication function owner is invalid'
                USING ERRCODE = '55000';
        END IF;
    END LOOP;

    FOR expected_signature IN
        SELECT expected.signature
        FROM (
            VALUES
                ('public.starring_product_oauth_database_identity_v1()'),
                ('public.starring_product_session_issuer_database_identity_v1()'),
                ('public.starring_product_session_api_database_identity_v1()'),
                ('public.starring_product_security_revoker_database_identity_v1()'),
                ('public.starring_product_oauth_flow_create_v1(BYTEA, BYTEA, TEXT, TEXT, DOUBLE PRECISION)'),
                ('public.starring_product_oauth_flow_consume_v1(BYTEA, BYTEA, TEXT, TEXT[])'),
                ('public.starring_product_session_issue_v1(BYTEA, TEXT, TEXT, TIMESTAMPTZ, TEXT, TEXT, BYTEA, BYTEA, DOUBLE PRECISION, DOUBLE PRECISION)'),
                ('public.starring_product_session_logout_read_v1(BYTEA)'),
                ('public.starring_product_session_logout_commit_v1(BYTEA, BYTEA, TIMESTAMPTZ)'),
                ('public.starring_product_session_security_revoke_v1(BYTEA)'),
                ('public.enforce_product_principal_transition()'),
                ('public.enforce_product_oauth_flow_transition()'),
                ('public.enforce_product_auth_session_oauth_binding()'),
                ('public.enforce_product_auth_session_transition()'),
                ('public.starring_purge_product_identity_v1(INTEGER)')
        ) AS expected(signature)
    LOOP
        function_oid := pg_catalog.to_regprocedure(expected_signature);
        IF function_oid IS NULL THEN
            RAISE EXCEPTION 'product identity function is unavailable'
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
                RAISE EXCEPTION 'product identity function grantee is unavailable'
                    USING ERRCODE = '55000';
            END IF;
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM %I CASCADE',
                expected_signature,
                unexpected_grantee_name
            );
        END LOOP;
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s OWNER TO %I',
            expected_signature,
            common_owner_name
        );
    END LOOP;
END;
$ownership$;
