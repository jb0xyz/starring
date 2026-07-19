ALTER TABLE public.product_auth_sessions
ADD COLUMN oauth_state_digest BYTEA;

UPDATE public.product_auth_sessions
SET revoked_at = GREATEST(pg_catalog.clock_timestamp(), last_seen_at),
    revocation_reason = 'oauth_rebinding_required'
WHERE oauth_state_digest IS NULL
    AND revoked_at IS NULL;

ALTER TABLE public.product_auth_sessions
ADD CONSTRAINT product_auth_sessions_oauth_state_digest_valid CHECK (
    oauth_state_digest IS NULL
    OR pg_catalog.octet_length(oauth_state_digest) = 32
),
ADD CONSTRAINT product_auth_sessions_oauth_state_digest_distinct CHECK (
    oauth_state_digest <> session_digest
    AND oauth_state_digest <> csrf_digest
),
ADD CONSTRAINT product_auth_sessions_oauth_state_fk FOREIGN KEY (oauth_state_digest)
    REFERENCES public.product_oauth_flows (state_digest)
    ON DELETE RESTRICT,
ADD CONSTRAINT product_auth_sessions_oauth_state_unique UNIQUE (oauth_state_digest),
ADD CONSTRAINT product_auth_sessions_oauth_binding_presence CHECK (
    oauth_state_digest IS NOT NULL
    OR revoked_at IS NOT NULL
),
ADD CONSTRAINT product_auth_sessions_oauth_lifetime_valid CHECK (
    oauth_state_digest IS NULL
    OR (
        authenticated_at = created_at
        AND absolute_expires_at <= authenticated_at + INTERVAL '12 hours'
    )
),
ADD CONSTRAINT product_auth_sessions_idle_lifetime_bounded CHECK (
    oauth_state_digest IS NULL
    OR idle_expires_at <= last_seen_at + INTERVAL '30 minutes'
);

ALTER TABLE public.product_oauth_flows
ADD CONSTRAINT product_oauth_flows_lifetime_bounded CHECK (
    expires_at <= created_at + INTERVAL '10 minutes'
) NOT VALID;

CREATE OR REPLACE FUNCTION public.enforce_product_principal_transition()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $function$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'product principals cannot be deleted directly'
            USING ERRCODE = '23514';
    END IF;
    IF OLD.disabled AND NOT NEW.disabled THEN
        RAISE EXCEPTION 'disabled product principals cannot be re-enabled directly'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.principal_id IS DISTINCT FROM OLD.principal_id
        OR NEW.discord_user_id IS DISTINCT FROM OLD.discord_user_id
        OR NEW.created_at IS DISTINCT FROM OLD.created_at
    THEN
        RAISE EXCEPTION 'product principal identity is immutable'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.identity_revision <> OLD.identity_revision + 1
        OR NEW.updated_at <= OLD.updated_at
        OR NEW.last_authenticated_at < OLD.last_authenticated_at
    THEN
        RAISE EXCEPTION 'product principal revisions and timestamps must advance monotonically'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE OR REPLACE FUNCTION public.enforce_product_oauth_flow_transition()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $function$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'product OAuth flows cannot be deleted directly'
            USING ERRCODE = '23514';
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

CREATE OR REPLACE FUNCTION public.enforce_product_auth_session_oauth_binding()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $function$
BEGIN
    IF NEW.authenticated_at IS DISTINCT FROM NEW.created_at
        OR NEW.last_seen_at IS DISTINCT FROM NEW.authenticated_at
        OR NEW.authenticated_at > pg_catalog.clock_timestamp()
        OR NEW.idle_expires_at > NEW.authenticated_at + INTERVAL '30 minutes'
    THEN
        RAISE EXCEPTION 'product authentication session initial activity is invalid'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_auth_sessions_initial_activity_valid';
    END IF;
    IF NOT EXISTS (
        SELECT 1
        FROM public.product_oauth_flows AS oauth_flow
        WHERE oauth_flow.state_digest = NEW.oauth_state_digest
            AND oauth_flow.consumed_at IS NOT NULL
            AND oauth_flow.terminal_result_code = 'callback_claimed'
            AND oauth_flow.consumed_at <= NEW.authenticated_at
            AND NEW.authenticated_at <= pg_catalog.clock_timestamp()
            AND NEW.authenticated_at < oauth_flow.expires_at
            AND oauth_flow.expires_at <= oauth_flow.created_at + INTERVAL '10 minutes'
            AND pg_catalog.clock_timestamp() < oauth_flow.expires_at
    )
    THEN
        RAISE EXCEPTION 'product authentication session OAuth binding is invalid'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_auth_sessions_oauth_binding_valid';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER product_auth_sessions_enforce_oauth_binding
BEFORE INSERT ON public.product_auth_sessions
FOR EACH ROW
EXECUTE FUNCTION public.enforce_product_auth_session_oauth_binding();

CREATE OR REPLACE FUNCTION public.enforce_product_auth_session_transition()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $function$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'product authentication sessions cannot be deleted directly'
            USING ERRCODE = '23514';
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
