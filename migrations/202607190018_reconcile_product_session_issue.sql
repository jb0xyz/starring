CREATE OR REPLACE FUNCTION public.starring_product_session_issue_v1(
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
    IF locked_flow.consumed_at > issue_now THEN
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
            OR existing_principal.created_at > existing_principal.last_authenticated_at
            OR existing_principal.last_authenticated_at > existing_principal.updated_at
            OR locked_flow.consumed_at > existing_session.authenticated_at
            OR existing_session.authenticated_at >= locked_flow.expires_at
            OR existing_session.authenticated_at > issue_now
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

    IF issue_now >= locked_flow.expires_at THEN
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
    expected_signature TEXT := 'public.starring_product_session_issue_v1(BYTEA, TEXT, TEXT, TIMESTAMPTZ, TEXT, TEXT, BYTEA, BYTEA, DOUBLE PRECISION, DOUBLE PRECISION)';
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

    function_oid := pg_catalog.to_regprocedure(expected_signature);
    IF function_oid IS NULL THEN
        RAISE EXCEPTION 'product session issue function is unavailable'
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
            RAISE EXCEPTION 'product session issue function grantee is unavailable'
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
    EXECUTE pg_catalog.format(
        'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE',
        expected_signature
    );

    SELECT function_row.oid
    INTO function_oid
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(expected_signature)
        AND function_row.proowner = common_owner
        AND function_row.prolang = (
            SELECT language_row.oid
            FROM pg_catalog.pg_language AS language_row
            WHERE language_row.lanname = 'plpgsql'
        )
        AND function_row.provolatile = 'v'
        AND function_row.proisstrict
        AND function_row.proparallel = 'u'
        AND function_row.prosecdef
        AND function_row.proconfig = ARRAY['search_path=pg_catalog']::TEXT[]
        AND function_row.prorows = 1;
    IF function_oid IS NULL THEN
        RAISE EXCEPTION 'product session issue function contract is invalid'
            USING ERRCODE = '55000';
    END IF;
END;
$ownership$;
