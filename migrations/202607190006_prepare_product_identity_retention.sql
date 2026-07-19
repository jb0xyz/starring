CREATE OR REPLACE FUNCTION public.enforce_product_oauth_flow_transition()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $function$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF pg_catalog.current_setting(
            'starring.product_identity_retention_gate',
            TRUE
        ) IS DISTINCT FROM 'starring.product.identity.retention.v1'
            OR NOT (
                OLD.consumed_at IS NULL
                AND OLD.expires_at
                    <= pg_catalog.clock_timestamp() - INTERVAL '1 hour'
                OR OLD.consumed_at IS NOT NULL
                AND OLD.expires_at
                    <= pg_catalog.clock_timestamp() - INTERVAL '7 days'
            )
            OR EXISTS (
                SELECT 1
                FROM public.product_auth_sessions AS product_session
                WHERE product_session.oauth_state_digest = OLD.state_digest
            )
        THEN
            RAISE EXCEPTION 'product OAuth flows cannot be deleted directly'
                USING ERRCODE = '23514';
        END IF;
        RETURN OLD;
    END IF;
    IF OLD.consumed_at IS NOT NULL THEN
        RAISE EXCEPTION 'consumed product OAuth flows are immutable'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.state_digest IS DISTINCT FROM OLD.state_digest
        OR NEW.browser_nonce_digest IS DISTINCT FROM OLD.browser_nonce_digest
        OR NEW.redirect_uri IS DISTINCT FROM OLD.redirect_uri
        OR NEW.return_path IS DISTINCT FROM OLD.return_path
        OR NEW.created_at IS DISTINCT FROM OLD.created_at
        OR NEW.expires_at IS DISTINCT FROM OLD.expires_at
        OR NEW.consumed_at IS NULL
        OR NEW.terminal_result_code IS NULL
    THEN
        RAISE EXCEPTION 'product OAuth flow updates may only consume an unchanged flow'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE OR REPLACE FUNCTION public.enforce_product_auth_session_transition()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $function$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF pg_catalog.current_setting(
            'starring.product_identity_retention_gate',
            TRUE
        ) IS DISTINCT FROM 'starring.product.identity.retention.v1'
            OR NOT (
                OLD.revoked_at IS NOT NULL
                AND OLD.revoked_at
                    <= pg_catalog.clock_timestamp() - INTERVAL '7 days'
                OR LEAST(OLD.idle_expires_at, OLD.absolute_expires_at)
                    <= pg_catalog.clock_timestamp() - INTERVAL '7 days'
            )
        THEN
            RAISE EXCEPTION 'product authentication sessions cannot be deleted directly'
                USING ERRCODE = '23514';
        END IF;
        RETURN OLD;
    END IF;
    IF OLD.revoked_at IS NOT NULL THEN
        RAISE EXCEPTION 'revoked product authentication sessions are immutable'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.session_digest IS DISTINCT FROM OLD.session_digest
        OR NEW.principal_id IS DISTINCT FROM OLD.principal_id
        OR NEW.csrf_digest IS DISTINCT FROM OLD.csrf_digest
        OR NEW.oauth_state_digest IS DISTINCT FROM OLD.oauth_state_digest
        OR NEW.authenticated_at IS DISTINCT FROM OLD.authenticated_at
        OR NEW.created_at IS DISTINCT FROM OLD.created_at
        OR NEW.absolute_expires_at IS DISTINCT FROM OLD.absolute_expires_at
    THEN
        RAISE EXCEPTION 'product authentication session identity is immutable'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.last_seen_at < OLD.last_seen_at
        OR NEW.last_seen_at > pg_catalog.clock_timestamp()
        OR NEW.idle_expires_at < OLD.idle_expires_at
        OR (
            NEW.revoked_at IS DISTINCT FROM OLD.revoked_at
            AND NEW.revoked_at IS NULL
        )
    THEN
        RAISE EXCEPTION 'product authentication session state cannot move backward'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_auth_sessions_transition_valid';
    END IF;
    IF NEW.last_seen_at IS NOT DISTINCT FROM OLD.last_seen_at
        AND NEW.idle_expires_at IS NOT DISTINCT FROM OLD.idle_expires_at
        AND NEW.revoked_at IS NULL
    THEN
        RAISE EXCEPTION 'product authentication session update made no state transition'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE INDEX product_auth_sessions_terminal_retention_index
ON public.product_auth_sessions (
    (
        LEAST(
            COALESCE(revoked_at, idle_expires_at),
            idle_expires_at,
            absolute_expires_at
        )
    ),
    session_digest
);

CREATE INDEX product_oauth_flows_consumed_retention_index
ON public.product_oauth_flows (expires_at, state_digest)
WHERE consumed_at IS NOT NULL;

CREATE INDEX product_oauth_flows_unconsumed_retention_index
ON public.product_oauth_flows (expires_at, state_digest)
WHERE consumed_at IS NULL;

CREATE FUNCTION public.starring_purge_product_identity_v1(batch_limit INTEGER)
RETURNS TABLE (
    deleted_sessions INTEGER,
    deleted_oauth_flows INTEGER,
    backlog_remaining BOOLEAN
)
LANGUAGE plpgsql
VOLATILE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    retention_clock TIMESTAMPTZ;
    session_count INTEGER;
    flow_count INTEGER;
    remaining_limit INTEGER;
    backlog BOOLEAN;
BEGIN
    IF batch_limit IS NULL OR batch_limit NOT BETWEEN 1 AND 1000 THEN
        RAISE EXCEPTION 'product identity purge batch limit is invalid'
            USING ERRCODE = '22023';
    END IF;

    retention_clock := pg_catalog.clock_timestamp();
    PERFORM pg_catalog.set_config(
        'starring.product_identity_retention_gate',
        'starring.product.identity.retention.v1',
        TRUE
    );

    WITH candidates AS MATERIALIZED (
        SELECT product_session.ctid AS row_id,
            product_session.session_digest
        FROM public.product_auth_sessions AS product_session
        WHERE LEAST(
            COALESCE(product_session.revoked_at, product_session.idle_expires_at),
            product_session.idle_expires_at,
            product_session.absolute_expires_at
        )
            <= retention_clock - INTERVAL '7 days'
        ORDER BY LEAST(
            COALESCE(product_session.revoked_at, product_session.idle_expires_at),
            product_session.idle_expires_at,
            product_session.absolute_expires_at
        ), product_session.session_digest
        FOR UPDATE OF product_session SKIP LOCKED
        LIMIT batch_limit
    ), deleted AS (
        DELETE FROM public.product_auth_sessions AS product_session
        WHERE product_session.ctid = ANY(
            ARRAY(
                SELECT candidate.row_id
                FROM candidates AS candidate
            )
        )
        RETURNING 1
    )
    SELECT pg_catalog.count(*)::INTEGER
    INTO session_count
    FROM deleted;

    remaining_limit := batch_limit - session_count;
    IF remaining_limit > 0 THEN
        WITH unconsumed_candidates AS MATERIALIZED (
            SELECT oauth_flow.ctid AS row_id,
                oauth_flow.state_digest,
                oauth_flow.expires_at
            FROM public.product_oauth_flows AS oauth_flow
            WHERE oauth_flow.consumed_at IS NULL
                AND oauth_flow.expires_at
                    <= retention_clock - INTERVAL '1 hour'
                AND NOT EXISTS (
                    SELECT 1
                    FROM public.product_auth_sessions AS product_session
                    WHERE product_session.oauth_state_digest = oauth_flow.state_digest
                    OFFSET 0
                )
            ORDER BY oauth_flow.expires_at, oauth_flow.state_digest
            FOR UPDATE OF oauth_flow SKIP LOCKED
            LIMIT remaining_limit
        ), consumed_candidates AS MATERIALIZED (
            SELECT oauth_flow.ctid AS row_id,
                oauth_flow.state_digest,
                oauth_flow.expires_at
            FROM public.product_oauth_flows AS oauth_flow
            WHERE oauth_flow.consumed_at IS NOT NULL
                AND oauth_flow.expires_at
                    <= retention_clock - INTERVAL '7 days'
                AND NOT EXISTS (
                    SELECT 1
                    FROM public.product_auth_sessions AS product_session
                    WHERE product_session.oauth_state_digest = oauth_flow.state_digest
                    OFFSET 0
                )
            ORDER BY oauth_flow.expires_at, oauth_flow.state_digest
            FOR UPDATE OF oauth_flow SKIP LOCKED
            LIMIT remaining_limit
        ), candidates AS MATERIALIZED (
            SELECT bounded.row_id,
                bounded.state_digest,
                bounded.expires_at
            FROM (
                SELECT *
                FROM unconsumed_candidates
                UNION ALL
                SELECT *
                FROM consumed_candidates
            ) AS bounded
            ORDER BY bounded.expires_at, bounded.state_digest
            LIMIT remaining_limit
        ), deleted AS (
            DELETE FROM public.product_oauth_flows AS oauth_flow
            WHERE oauth_flow.ctid = ANY(
                ARRAY(
                    SELECT candidate.row_id
                    FROM candidates AS candidate
                )
            )
            RETURNING 1
        )
        SELECT pg_catalog.count(*)::INTEGER
        INTO flow_count
        FROM deleted;
    ELSE
        flow_count := 0;
    END IF;

    WITH session_backlog AS MATERIALIZED (
        SELECT product_session.session_digest
        FROM public.product_auth_sessions AS product_session
        WHERE LEAST(
            COALESCE(product_session.revoked_at, product_session.idle_expires_at),
            product_session.idle_expires_at,
            product_session.absolute_expires_at
        )
            <= retention_clock - INTERVAL '7 days'
        ORDER BY LEAST(
            COALESCE(product_session.revoked_at, product_session.idle_expires_at),
            product_session.idle_expires_at,
            product_session.absolute_expires_at
        ), product_session.session_digest
        LIMIT 1
    ), unconsumed_flow_backlog AS MATERIALIZED (
        SELECT oauth_flow.state_digest
        FROM public.product_oauth_flows AS oauth_flow
        WHERE oauth_flow.consumed_at IS NULL
            AND oauth_flow.expires_at
                <= retention_clock - INTERVAL '1 hour'
            AND NOT EXISTS (
                SELECT 1
                FROM public.product_auth_sessions AS product_session
                WHERE product_session.oauth_state_digest = oauth_flow.state_digest
                OFFSET 0
            )
        ORDER BY oauth_flow.expires_at, oauth_flow.state_digest
        LIMIT 1
    ), consumed_flow_backlog AS MATERIALIZED (
        SELECT oauth_flow.state_digest
        FROM public.product_oauth_flows AS oauth_flow
        WHERE oauth_flow.consumed_at IS NOT NULL
            AND oauth_flow.expires_at
                <= retention_clock - INTERVAL '7 days'
            AND NOT EXISTS (
                SELECT 1
                FROM public.product_auth_sessions AS product_session
                WHERE product_session.oauth_state_digest = oauth_flow.state_digest
                OFFSET 0
            )
        ORDER BY oauth_flow.expires_at, oauth_flow.state_digest
        LIMIT 1
    )
    SELECT EXISTS (SELECT 1 FROM session_backlog)
        OR EXISTS (SELECT 1 FROM unconsumed_flow_backlog)
        OR EXISTS (SELECT 1 FROM consumed_flow_backlog)
    INTO backlog;

    PERFORM pg_catalog.set_config(
        'starring.product_identity_retention_gate',
        '',
        TRUE
    );
    RETURN QUERY SELECT session_count, flow_count, backlog;
END;
$function$;

REVOKE ALL ON FUNCTION public.starring_purge_product_identity_v1(INTEGER)
FROM PUBLIC;
