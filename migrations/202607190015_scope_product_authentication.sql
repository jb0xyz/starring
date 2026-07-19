CREATE FUNCTION public.starring_product_session_read_v1(
    expected_session_digest BYTEA
)
RETURNS TABLE (
    principal_id TEXT,
    discord_user_id TEXT,
    identity_revision BIGINT,
    display_profile JSONB,
    principal_disabled BOOLEAN,
    csrf_digest_length INTEGER,
    oauth_state_digest_length INTEGER,
    csrf_comparison_tag BYTEA,
    last_seen_at TIMESTAMPTZ,
    idle_expires_at TIMESTAMPTZ,
    absolute_expires_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ
)
LANGUAGE sql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
    SELECT authentication_session.principal_id,
        principal.discord_user_id,
        principal.identity_revision,
        principal.display_profile,
        principal.disabled AS principal_disabled,
        pg_catalog.octet_length(authentication_session.csrf_digest)
            AS csrf_digest_length,
        pg_catalog.octet_length(authentication_session.oauth_state_digest)
            AS oauth_state_digest_length,
        NULL::BYTEA AS csrf_comparison_tag,
        authentication_session.last_seen_at,
        authentication_session.idle_expires_at,
        authentication_session.absolute_expires_at,
        authentication_session.revoked_at
    FROM public.product_auth_sessions AS authentication_session
    INNER JOIN public.product_principals AS principal
        ON principal.principal_id = authentication_session.principal_id
    WHERE authentication_session.session_digest = expected_session_digest
        AND pg_catalog.octet_length(expected_session_digest) = 32
    FOR SHARE OF authentication_session, principal;
$function$;

CREATE FUNCTION public.starring_product_session_mutation_read_v1(
    expected_session_digest BYTEA
)
RETURNS TABLE (
    principal_id TEXT,
    discord_user_id TEXT,
    identity_revision BIGINT,
    display_profile JSONB,
    principal_disabled BOOLEAN,
    csrf_digest_length INTEGER,
    oauth_state_digest_length INTEGER,
    csrf_comparison_tag BYTEA,
    last_seen_at TIMESTAMPTZ,
    idle_expires_at TIMESTAMPTZ,
    absolute_expires_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ
)
LANGUAGE sql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
    SELECT authentication_session.principal_id,
        principal.discord_user_id,
        principal.identity_revision,
        principal.display_profile,
        principal.disabled AS principal_disabled,
        pg_catalog.octet_length(authentication_session.csrf_digest)
            AS csrf_digest_length,
        pg_catalog.octet_length(authentication_session.oauth_state_digest)
            AS oauth_state_digest_length,
        pg_catalog.sha256(pg_catalog.byteacat(
            expected_session_digest,
            authentication_session.csrf_digest
        )) AS csrf_comparison_tag,
        authentication_session.last_seen_at,
        authentication_session.idle_expires_at,
        authentication_session.absolute_expires_at,
        authentication_session.revoked_at
    FROM public.product_auth_sessions AS authentication_session
    INNER JOIN public.product_principals AS principal
        ON principal.principal_id = authentication_session.principal_id
    WHERE authentication_session.session_digest = expected_session_digest
        AND pg_catalog.octet_length(expected_session_digest) = 32
    FOR SHARE OF authentication_session, principal;
$function$;

CREATE FUNCTION public.starring_product_session_touch_v1(
    expected_session_digest BYTEA,
    observed_last_seen_at TIMESTAMPTZ,
    observed_idle_expires_at TIMESTAMPTZ,
    observed_absolute_expires_at TIMESTAMPTZ,
    touch_interval_seconds DOUBLE PRECISION
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
            AND authentication_session.revoked_at IS NULL
            AND authentication_session.last_seen_at = observed_last_seen_at
            AND authentication_session.idle_expires_at = observed_idle_expires_at
            AND authentication_session.absolute_expires_at = observed_absolute_expires_at
            AND observed_last_seen_at < observed_idle_expires_at
            AND observed_idle_expires_at <= observed_absolute_expires_at
            AND observed_idle_expires_at
                <= observed_last_seen_at + INTERVAL '30 minutes'
            AND touch_interval_seconds >= 1
            AND touch_interval_seconds < pg_catalog.date_part(
                'epoch', observed_idle_expires_at - observed_last_seen_at
            )
        FOR UPDATE
    ), touch_clock AS MATERIALIZED (
        SELECT pg_catalog.clock_timestamp() AS touched_at
        FROM locked_session
    ), touched AS (
        UPDATE public.product_auth_sessions AS authentication_session
        SET last_seen_at = touch_clock.touched_at,
            idle_expires_at = LEAST(
                authentication_session.absolute_expires_at,
                touch_clock.touched_at
                    + (observed_idle_expires_at - observed_last_seen_at)
            )
        FROM locked_session, touch_clock
        WHERE authentication_session.session_digest = locked_session.session_digest
            AND touch_clock.touched_at >= authentication_session.last_seen_at
            AND touch_clock.touched_at < authentication_session.idle_expires_at
            AND touch_clock.touched_at < authentication_session.absolute_expires_at
            AND touch_clock.touched_at - authentication_session.last_seen_at
                >= pg_catalog.make_interval(secs => touch_interval_seconds)
        RETURNING 1
    )
    SELECT pg_catalog.count(*) FROM touched;
$function$;

REVOKE ALL ON FUNCTION public.starring_product_session_read_v1(BYTEA)
FROM PUBLIC;
REVOKE ALL ON FUNCTION public.starring_product_session_mutation_read_v1(BYTEA)
FROM PUBLIC;
REVOKE ALL ON FUNCTION public.starring_product_session_touch_v1(
    BYTEA,
    TIMESTAMPTZ,
    TIMESTAMPTZ,
    TIMESTAMPTZ,
    DOUBLE PRECISION
)
FROM PUBLIC;

DO $ownership$
DECLARE
    relation_count BIGINT;
    table_count BIGINT;
    rls_disabled_count BIGINT;
    owner_count BIGINT;
    common_owner OID;
    common_owner_name NAME;
    function_signature TEXT;
    function_oid OID;
    function_count BIGINT;
    function_owner_count BIGINT;
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
            (pg_catalog.to_regclass('public.product_auth_sessions'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid;

    IF relation_count <> 2
        OR table_count <> 2
        OR rls_disabled_count <> 2
        OR owner_count <> 1
        OR common_owner IS NULL
    THEN
        RAISE EXCEPTION 'authentication relations require one non-RLS owner'
            USING ERRCODE = '55000';
    END IF;

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL THEN
        RAISE EXCEPTION 'authentication relation owner is unavailable'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(function_row.oid),
        pg_catalog.count(DISTINCT function_row.proowner)
    INTO function_count, function_owner_count
    FROM (
        VALUES
            ('public.starring_product_session_read_v1(bytea)'),
            ('public.starring_product_session_mutation_read_v1(bytea)'),
            ('public.starring_product_session_touch_v1(bytea,timestamp with time zone,timestamp with time zone,timestamp with time zone,double precision)')
    ) AS expected(function_signature)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.function_signature);

    IF function_count <> 3 OR function_owner_count <> 1 THEN
        RAISE EXCEPTION 'authentication functions require one creator'
            USING ERRCODE = '55000';
    END IF;

    FOR function_signature IN
        SELECT expected.signature
        FROM (
            VALUES
                ('public.starring_product_session_read_v1(BYTEA)'),
                ('public.starring_product_session_mutation_read_v1(BYTEA)'),
                ('public.starring_product_session_touch_v1(BYTEA, TIMESTAMPTZ, TIMESTAMPTZ, TIMESTAMPTZ, DOUBLE PRECISION)')
        ) AS expected(signature)
    LOOP
        function_oid := pg_catalog.to_regprocedure(function_signature);
        IF function_oid IS NULL THEN
            RAISE EXCEPTION 'authentication function is unavailable'
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
                RAISE EXCEPTION 'authentication function grantee is unavailable'
                    USING ERRCODE = '55000';
            END IF;
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM %I CASCADE',
                function_signature,
                unexpected_grantee_name
            );
        END LOOP;
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s OWNER TO %I',
            function_signature,
            common_owner_name
        );
    END LOOP;
END;
$ownership$;
