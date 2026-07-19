ALTER TABLE public.product_action_receipts
ADD COLUMN idempotency_digest_key_id TEXT,
ADD COLUMN idempotency_digest_key_fingerprint TEXT;

ALTER TABLE public.product_action_receipts
ADD CONSTRAINT product_action_receipts_digest_key_id_valid CHECK (
    idempotency_digest_key_id IS NULL
    OR idempotency_digest_key_id ~ '^[A-Za-z0-9_.:-]{1,64}$'
) NOT VALID,
ADD CONSTRAINT product_action_receipts_digest_key_fingerprint_valid CHECK (
    idempotency_digest_key_fingerprint IS NULL
    OR idempotency_digest_key_fingerprint ~ '^[0-9a-f]{64}$'
) NOT VALID,
ADD CONSTRAINT product_action_receipts_approval_key_identity_required CHECK (
    endpoint_domain <> 'product_approve_v1'
    OR (
        idempotency_digest_key_id IS NOT NULL
        AND idempotency_digest_key_fingerprint IS NOT NULL
    )
) NOT VALID;

ALTER TABLE public.product_action_receipts
VALIDATE CONSTRAINT product_action_receipts_digest_key_id_valid;

ALTER TABLE public.product_action_receipts
VALIDATE CONSTRAINT product_action_receipts_digest_key_fingerprint_valid;

ALTER TABLE public.product_action_receipts
VALIDATE CONSTRAINT product_action_receipts_approval_key_identity_required;

ALTER TABLE public.product_action_receipts
ADD CONSTRAINT product_action_receipts_endpoint_scope_identity_unique UNIQUE (
    tenant_id,
    installation_id,
    principal_id,
    endpoint_domain,
    receipt_id
);

CREATE TABLE public.product_action_receipt_idempotency_aliases (
    tenant_id TEXT NOT NULL,
    installation_id TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    endpoint_domain TEXT NOT NULL,
    idempotency_key_digest TEXT NOT NULL,
    idempotency_digest_key_id TEXT NOT NULL,
    idempotency_digest_key_fingerprint TEXT NOT NULL,
    receipt_id TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT product_action_receipt_idempotency_aliases_primary PRIMARY KEY (
        tenant_id,
        installation_id,
        principal_id,
        endpoint_domain,
        idempotency_key_digest
    ),
    CONSTRAINT product_action_receipt_idempotency_aliases_receipt_fk FOREIGN KEY (
        tenant_id,
        installation_id,
        principal_id,
        endpoint_domain,
        receipt_id
    ) REFERENCES public.product_action_receipts (
        tenant_id,
        installation_id,
        principal_id,
        endpoint_domain,
        receipt_id
    ) ON DELETE RESTRICT,
    CONSTRAINT product_action_receipt_idempotency_aliases_scope_valid CHECK (
        tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND principal_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND endpoint_domain ~ '^[a-z][a-z0-9_.:-]{0,63}$'
        AND idempotency_key_digest ~ '^[0-9a-f]{64}$'
        AND idempotency_digest_key_id ~ '^[A-Za-z0-9_.:-]{1,64}$'
        AND idempotency_digest_key_fingerprint ~ '^[0-9a-f]{64}$'
        AND receipt_id ~ '^[0-9a-f]{64}$'
    )
);

INSERT INTO public.product_action_receipt_idempotency_aliases (
    tenant_id,
    installation_id,
    principal_id,
    endpoint_domain,
    idempotency_key_digest,
    idempotency_digest_key_id,
    idempotency_digest_key_fingerprint,
    receipt_id,
    created_at
)
SELECT receipt.tenant_id,
    receipt.installation_id,
    receipt.principal_id,
    receipt.endpoint_domain,
    receipt.idempotency_key_digest,
    receipt.idempotency_digest_key_id,
    receipt.idempotency_digest_key_fingerprint,
    receipt.receipt_id,
    receipt.completed_at
FROM public.product_action_receipts AS receipt
WHERE receipt.endpoint_domain = 'product_approve_v1';

CREATE INDEX product_action_receipt_idempotency_aliases_key_coverage_index
ON public.product_action_receipt_idempotency_aliases (
    tenant_id,
    installation_id,
    principal_id,
    endpoint_domain,
    receipt_id,
    idempotency_digest_key_id,
    idempotency_digest_key_fingerprint
);

CREATE TRIGGER product_action_receipt_idempotency_aliases_reject_mutation
BEFORE UPDATE OR DELETE ON public.product_action_receipt_idempotency_aliases
FOR EACH ROW
EXECUTE FUNCTION public.reject_immutable_product_row();

CREATE FUNCTION public.assert_product_approval_receipt_alias()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $function$
BEGIN
    IF NEW.endpoint_domain = 'product_approve_v1'
        AND NOT EXISTS (
            SELECT 1
            FROM public.product_action_receipt_idempotency_aliases AS alias
            WHERE alias.tenant_id = NEW.tenant_id
                AND alias.installation_id = NEW.installation_id
                AND alias.principal_id = NEW.principal_id
                AND alias.endpoint_domain = NEW.endpoint_domain
                AND alias.idempotency_key_digest = NEW.idempotency_key_digest
                AND alias.idempotency_digest_key_id = NEW.idempotency_digest_key_id
                AND alias.idempotency_digest_key_fingerprint
                    = NEW.idempotency_digest_key_fingerprint
                AND alias.receipt_id = NEW.receipt_id
        )
    THEN
        RAISE EXCEPTION 'product approval receipt is missing its primary idempotency alias'
            USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END;
$function$;

CREATE CONSTRAINT TRIGGER product_action_receipts_assert_approval_alias
AFTER INSERT ON public.product_action_receipts
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION public.assert_product_approval_receipt_alias();

REVOKE ALL ON FUNCTION public.assert_product_approval_receipt_alias() FROM PUBLIC;

CREATE FUNCTION public.assert_product_approval_receipt_audit()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $function$
BEGIN
    IF NEW.endpoint_domain = 'product_approve_v1'
        AND NOT EXISTS (
            SELECT 1
            FROM public.product_audit_events AS audit
            WHERE audit.tenant_id = NEW.tenant_id
                AND audit.installation_id = NEW.installation_id
                AND audit.principal_id = NEW.principal_id
                AND audit.receipt_id = NEW.receipt_id
                AND audit.action = 'promotion.approve'
                AND audit.target_resource_type = NEW.target_resource_type
                AND audit.target_resource_id = NEW.target_resource_id
                AND audit.resulting_state = NEW.resulting_state
                AND audit.result_code = NEW.result_code
        )
    THEN
        RAISE EXCEPTION 'product approval receipt is missing its audit event'
            USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END;
$function$;

CREATE CONSTRAINT TRIGGER product_action_receipts_assert_approval_audit
AFTER INSERT ON public.product_action_receipts
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION public.assert_product_approval_receipt_audit();

REVOKE ALL ON FUNCTION public.assert_product_approval_receipt_audit() FROM PUBLIC;

ALTER TABLE public.product_audit_events
DROP CONSTRAINT product_audit_events_session_principal_fk;

ALTER TABLE public.product_audit_events
RENAME COLUMN product_session_digest TO session_subject_digest;

ALTER TABLE public.product_audit_events
RENAME CONSTRAINT product_audit_events_session_digest_valid
TO product_audit_events_session_subject_digest_valid;

CREATE OR REPLACE FUNCTION public.enforce_activation_approval_payload_binding()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $function$
DECLARE
    parent_authority TEXT;
    parent_link_state TEXT;
    expected_digest TEXT;
    expected_context_digest TEXT;
    approval_gate TEXT;
BEGIN
    SELECT activation.authority_kind,
        activation.link_state_name,
        activation.approval_payload_digest,
        activation.approval_context_digest
    INTO parent_authority,
        parent_link_state,
        expected_digest,
        expected_context_digest
    FROM public.activation_requests AS activation
    WHERE activation.id = NEW.request_id
    FOR KEY SHARE;

    IF parent_authority = 'legacy_manual' AND NEW.approval_payload_digest IS NOT NULL THEN
        RAISE EXCEPTION 'legacy activation approval cannot carry a payload digest'
            USING ERRCODE = '23514';
    END IF;
    IF parent_authority = 'product_authoring' THEN
        approval_gate := pg_catalog.current_setting(
            'starring.product_approval_gate',
            TRUE
        );
        IF parent_link_state <> 'linked'
            OR NEW.approval_payload_digest IS DISTINCT FROM expected_digest
            OR approval_gate IS DISTINCT FROM expected_context_digest
        THEN
            RAISE EXCEPTION 'product activation approval payload is not exactly authorized'
                USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$function$;

CREATE FUNCTION public.reject_activation_approval_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $function$
BEGIN
    RAISE EXCEPTION 'activation approvals are append-only'
        USING ERRCODE = '23514';
END;
$function$;

CREATE TRIGGER activation_request_approvals_reject_mutation
BEFORE UPDATE OR DELETE ON public.activation_request_approvals
FOR EACH ROW
EXECUTE FUNCTION public.reject_activation_approval_mutation();

REVOKE ALL ON FUNCTION public.enforce_activation_approval_payload_binding() FROM PUBLIC;

REVOKE ALL ON FUNCTION public.reject_activation_approval_mutation() FROM PUBLIC;

CREATE FUNCTION public.starring_product_approve_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_promotion_id TEXT,
    expected_product_revision BIGINT,
    expected_payload_digest TEXT,
    expected_principal_id TEXT,
    expected_product_session_digest BYTEA,
    session_subject_digest BYTEA,
    expected_acting_user_id TEXT,
    expected_discord_application_id TEXT,
    expected_guild_id TEXT,
    expected_capability TEXT,
    expected_authority_revision BIGINT,
    expected_authority_payload_digest TEXT,
    expected_authority_observation_digest TEXT,
    expected_authority_observed_at TIMESTAMPTZ,
    expected_authority_expires_at TIMESTAMPTZ,
    expected_effective_permission_bits TEXT,
    expected_guild_owner BOOLEAN,
    product_request_id TEXT,
    active_idempotency_key_digest TEXT,
    idempotency_key_digest_candidates TEXT[],
    idempotency_digest_key_id_candidates TEXT[],
    idempotency_digest_key_fingerprint_candidates TEXT[],
    idempotency_digest_key_id TEXT,
    semantic_request_digest TEXT,
    new_receipt_id TEXT,
    new_audit_event_id TEXT
)
RETURNS TABLE (
    outcome TEXT,
    resulting_revision BIGINT,
    resulting_state TEXT,
    exact_replay BOOLEAN,
    guild_id TEXT
)
LANGUAGE plpgsql
VOLATILE
STRICT
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    mutation_clock TIMESTAMPTZ;
    activation_row public.activation_requests%ROWTYPE;
    promotion_row public.authoring_promotions%ROWTYPE;
    tenant_row public.product_tenants%ROWTYPE;
    installation_row public.automation_installations%ROWTYPE;
    authority_row public.automation_installation_authority_versions%ROWTYPE;
    principal_row public.product_principals%ROWTYPE;
    session_row public.product_auth_sessions%ROWTYPE;
    receipt_row public.product_action_receipts%ROWTYPE;
    matched_receipt_count BIGINT;
    approval_count BIGINT;
    next_revision BIGINT;
    next_state TEXT;
    result_code TEXT;
    active_baseline_version BIGINT;
    active_baseline_hash TEXT;
    candidate_lock_digest TEXT;
BEGIN
    IF expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_promotion_id !~ '^[0-9a-f]{64}$'
        OR expected_product_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_payload_digest !~ '^[0-9a-f]{64}$'
        OR expected_principal_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR pg_catalog.octet_length(expected_product_session_digest) <> 32
        OR pg_catalog.octet_length(session_subject_digest) <> 32
        OR session_subject_digest = expected_product_session_digest
        OR NOT (CASE
            WHEN expected_acting_user_id ~ '^[1-9][0-9]{0,19}$'
                THEN expected_acting_user_id::NUMERIC <= 18446744073709551615
            ELSE FALSE
        END)
        OR NOT (CASE
            WHEN expected_discord_application_id ~ '^[1-9][0-9]{0,19}$'
                THEN expected_discord_application_id::NUMERIC <= 18446744073709551615
            ELSE FALSE
        END)
        OR NOT (CASE
            WHEN expected_guild_id ~ '^[1-9][0-9]{0,19}$'
                THEN expected_guild_id::NUMERIC <= 18446744073709551615
            ELSE FALSE
        END)
        OR expected_capability <> 'approve'
        OR expected_authority_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_authority_payload_digest !~ '^[0-9a-f]{64}$'
        OR expected_authority_observation_digest !~ '^[0-9a-f]{64}$'
        OR expected_authority_observed_at >= expected_authority_expires_at
        OR expected_authority_expires_at
            > expected_authority_observed_at + INTERVAL '5 seconds'
        OR NOT (CASE
            WHEN expected_effective_permission_bits ~ '^(0|[1-9][0-9]{0,19})$'
                THEN expected_effective_permission_bits::NUMERIC
                    <= 18446744073709551615
            ELSE FALSE
        END)
        OR NOT (
            expected_guild_owner
            OR CASE
                WHEN expected_effective_permission_bits ~ '^(0|[1-9][0-9]{0,19})$'
                THEN pg_catalog.mod(expected_effective_permission_bits::NUMERIC, 16) >= 8
                    OR pg_catalog.mod(expected_effective_permission_bits::NUMERIC, 64) >= 32
                ELSE FALSE
            END
        )
        OR product_request_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR active_idempotency_key_digest !~ '^[0-9a-f]{64}$'
        OR pg_catalog.array_ndims(idempotency_key_digest_candidates) IS DISTINCT FROM 1
        OR pg_catalog.array_lower(idempotency_key_digest_candidates, 1) IS DISTINCT FROM 1
        OR pg_catalog.cardinality(idempotency_key_digest_candidates) NOT BETWEEN 1 AND 8
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.unnest(idempotency_key_digest_candidates) AS candidate(digest)
            WHERE candidate.digest !~ '^[0-9a-f]{64}$'
        )
        OR pg_catalog.array_ndims(idempotency_digest_key_id_candidates) IS DISTINCT FROM 1
        OR pg_catalog.array_lower(idempotency_digest_key_id_candidates, 1) IS DISTINCT FROM 1
        OR pg_catalog.cardinality(idempotency_digest_key_id_candidates)
            IS DISTINCT FROM pg_catalog.cardinality(idempotency_key_digest_candidates)
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.unnest(idempotency_digest_key_id_candidates) AS candidate(key_id)
            WHERE candidate.key_id !~ '^[A-Za-z0-9_.:-]{1,64}$'
        )
        OR pg_catalog.array_ndims(idempotency_digest_key_fingerprint_candidates)
            IS DISTINCT FROM 1
        OR pg_catalog.array_lower(idempotency_digest_key_fingerprint_candidates, 1)
            IS DISTINCT FROM 1
        OR pg_catalog.cardinality(idempotency_digest_key_fingerprint_candidates)
            IS DISTINCT FROM pg_catalog.cardinality(idempotency_key_digest_candidates)
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.unnest(idempotency_digest_key_fingerprint_candidates)
                AS candidate(fingerprint)
            WHERE candidate.fingerprint !~ '^[0-9a-f]{64}$'
        )
        OR idempotency_digest_key_id !~ '^[A-Za-z0-9_.:-]{1,64}$'
        OR idempotency_key_digest_candidates[1]
            IS DISTINCT FROM active_idempotency_key_digest
        OR idempotency_digest_key_id_candidates[1]
            IS DISTINCT FROM idempotency_digest_key_id
        OR semantic_request_digest !~ '^[0-9a-f]{64}$'
        OR new_receipt_id !~ '^[0-9a-f]{64}$'
        OR new_audit_event_id !~ '^[0-9a-f]{64}$'
    THEN
        RETURN QUERY SELECT 'invalid_input', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
        RETURN;
    END IF;

    IF (
        SELECT pg_catalog.count(DISTINCT candidate.digest)
        FROM pg_catalog.unnest(idempotency_key_digest_candidates) AS candidate(digest)
    ) <> pg_catalog.cardinality(idempotency_key_digest_candidates)
        OR (
            SELECT pg_catalog.count(DISTINCT candidate.key_id)
            FROM pg_catalog.unnest(idempotency_digest_key_id_candidates)
                AS candidate(key_id)
        ) <> pg_catalog.cardinality(idempotency_digest_key_id_candidates)
        OR (
            SELECT pg_catalog.count(DISTINCT candidate.fingerprint)
            FROM pg_catalog.unnest(idempotency_digest_key_fingerprint_candidates)
                AS candidate(fingerprint)
        ) <> pg_catalog.cardinality(idempotency_digest_key_fingerprint_candidates)
    THEN
        RETURN QUERY SELECT 'invalid_input', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
        RETURN;
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            expected_tenant_id || ':' || expected_installation_id || ':'
                || expected_principal_id || ':product_approve_v1:key-coverage',
            0
        )
    );

    FOR candidate_lock_digest IN
        SELECT candidate.digest
        FROM pg_catalog.unnest(idempotency_key_digest_candidates) AS candidate(digest)
        ORDER BY candidate.digest
    LOOP
        PERFORM pg_catalog.pg_advisory_xact_lock(
            pg_catalog.hashtextextended(
                expected_tenant_id || ':' || expected_installation_id || ':'
                    || expected_principal_id || ':product_approve_v1:'
                    || candidate_lock_digest,
                0
            )
        );
    END LOOP;
    mutation_clock := pg_catalog.clock_timestamp();

    SELECT *
    INTO activation_row
    FROM public.activation_requests AS activation
    WHERE activation.tenant_id = expected_tenant_id
        AND activation.installation_id = expected_installation_id
        AND activation.promotion_id = expected_promotion_id
    FOR UPDATE;
    IF NOT FOUND THEN
        RETURN QUERY SELECT 'not_found', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
        RETURN;
    END IF;

    SELECT *
    INTO promotion_row
    FROM public.authoring_promotions AS promotion
    WHERE promotion.id = expected_promotion_id
    FOR SHARE;
    IF NOT FOUND THEN
        RETURN QUERY SELECT 'not_found', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
        RETURN;
    END IF;

    SELECT *
    INTO tenant_row
    FROM public.product_tenants AS tenant
    WHERE tenant.tenant_id = expected_tenant_id
    FOR SHARE;

    SELECT *
    INTO installation_row
    FROM public.automation_installations AS installation
    WHERE installation.tenant_id = expected_tenant_id
        AND installation.installation_id = expected_installation_id
    FOR SHARE;

    SELECT *
    INTO authority_row
    FROM public.automation_installation_authority_versions AS authority
    WHERE authority.tenant_id = expected_tenant_id
        AND authority.installation_id = expected_installation_id
        AND authority.revision = expected_authority_revision
    FOR SHARE;

    SELECT *
    INTO principal_row
    FROM public.product_principals AS principal
    WHERE principal.principal_id = expected_principal_id
    FOR SHARE;

    SELECT *
    INTO session_row
    FROM public.product_auth_sessions AS product_session
    WHERE product_session.session_digest = expected_product_session_digest
        AND product_session.principal_id = expected_principal_id
    FOR SHARE;

    IF tenant_row.tenant_id IS NULL
        OR installation_row.installation_id IS NULL
        OR authority_row.installation_id IS NULL
        OR principal_row.principal_id IS NULL
        OR session_row.principal_id IS NULL
        OR tenant_row.lifecycle_state <> 'active'
        OR installation_row.lifecycle_state <> 'active'
        OR principal_row.disabled
        OR principal_row.discord_user_id IS DISTINCT FROM expected_acting_user_id
        OR session_row.oauth_state_digest IS NULL
        OR session_row.revoked_at IS NOT NULL
        OR mutation_clock >= session_row.idle_expires_at
        OR mutation_clock >= session_row.absolute_expires_at
        OR expected_authority_observed_at > mutation_clock
        OR mutation_clock >= expected_authority_expires_at
    THEN
        RETURN QUERY SELECT 'authorization_stale', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
        RETURN;
    END IF;

    IF installation_row.discord_application_id IS DISTINCT FROM expected_discord_application_id
        OR installation_row.discord_guild_id IS DISTINCT FROM expected_guild_id
        OR installation_row.current_authority_revision IS DISTINCT FROM expected_authority_revision
        OR activation_row.authority_kind <> 'product_authoring'
        OR activation_row.link_state_name <> 'linked'
        OR activation_row.guild_id IS DISTINCT FROM expected_guild_id
        OR activation_row.ruleset_key IS DISTINCT FROM installation_row.ruleset_key
        OR promotion_row.tenant_id IS DISTINCT FROM expected_tenant_id
        OR promotion_row.installation_id IS DISTINCT FROM expected_installation_id
        OR promotion_row.record #>> '{intent,authority,tenant_id}'
            IS DISTINCT FROM expected_tenant_id
        OR promotion_row.record #>> '{intent,authority,installation_id}'
            IS DISTINCT FROM expected_installation_id
        OR promotion_row.record #>> '{intent,authority,guild_id}'
            IS DISTINCT FROM expected_guild_id
        OR promotion_row.record #>> '{intent,authority,ruleset_key}'
            IS DISTINCT FROM activation_row.ruleset_key
        OR promotion_row.record #>> '{stage,activation,request_id}'
            IS DISTINCT FROM activation_row.id
        OR promotion_row.request_digest IS DISTINCT FROM activation_row.promotion_request_digest
    THEN
        RETURN QUERY SELECT 'scope_mismatch', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
        RETURN;
    END IF;

    IF authority_row.binding_revision::TEXT
            IS DISTINCT FROM activation_row.approval_context #>> '{context,binding,revision}'
        OR authority_row.authority_payload_digest
            IS DISTINCT FROM expected_authority_payload_digest
        OR authority_row.binding_fingerprint
            IS DISTINCT FROM activation_row.approval_context #>> '{context,binding,fingerprint}'
        OR authority_row.policy_revision::TEXT
            IS DISTINCT FROM activation_row.approval_context #>> '{context,policy,revision}'
        OR authority_row.required_approvals::TEXT
            IS DISTINCT FROM activation_row.approval_context
                #>> '{context,policy,required_approvals}'
        OR authority_row.activation_ttl_seconds::TEXT
            IS DISTINCT FROM activation_row.approval_context
                #>> '{context,policy,ttl_seconds}'
        OR activation_row.required_approvals IS DISTINCT FROM authority_row.required_approvals
        OR activation_row.approval_payload_digest
            IS DISTINCT FROM promotion_row.record
                #>> '{stage,activation,approval_context,approval_payload_digest}'
        OR activation_row.approval_context #>> '{context,policy,digest}'
            IS DISTINCT FROM promotion_row.record
                #>> '{stage,activation,approval_context,policy,digest}'
    THEN
        RETURN QUERY SELECT 'authority_mismatch', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
        RETURN;
    END IF;

    IF EXISTS (
        SELECT 1
        FROM public.product_action_receipts AS receipt
        WHERE receipt.tenant_id = expected_tenant_id
            AND receipt.installation_id = expected_installation_id
            AND receipt.principal_id = expected_principal_id
            AND receipt.endpoint_domain = 'product_approve_v1'
            AND NOT EXISTS (
                SELECT 1
                FROM public.product_action_receipt_idempotency_aliases AS alias
                WHERE alias.tenant_id = receipt.tenant_id
                    AND alias.installation_id = receipt.installation_id
                    AND alias.principal_id = receipt.principal_id
                    AND alias.endpoint_domain = receipt.endpoint_domain
                    AND alias.receipt_id = receipt.receipt_id
                    AND EXISTS (
                        SELECT 1
                        FROM pg_catalog.generate_subscripts(
                            idempotency_digest_key_id_candidates,
                            1
                        ) AS candidate(ordinal)
                        WHERE idempotency_digest_key_id_candidates[candidate.ordinal]
                                = alias.idempotency_digest_key_id
                            AND idempotency_digest_key_fingerprint_candidates[
                                candidate.ordinal
                            ] = alias.idempotency_digest_key_fingerprint
                    )
            )
    ) THEN
        RETURN QUERY SELECT 'idempotency_keyring_incomplete', NULL::BIGINT, NULL::TEXT,
            FALSE, NULL::TEXT;
        RETURN;
    END IF;

    SELECT pg_catalog.count(DISTINCT alias.receipt_id)
    INTO matched_receipt_count
    FROM public.product_action_receipt_idempotency_aliases AS alias
    WHERE alias.tenant_id = expected_tenant_id
        AND alias.installation_id = expected_installation_id
        AND alias.principal_id = expected_principal_id
        AND alias.endpoint_domain = 'product_approve_v1'
        AND alias.idempotency_key_digest = ANY(idempotency_key_digest_candidates);

    IF matched_receipt_count > 1 THEN
        RETURN QUERY SELECT 'indeterminate', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
        RETURN;
    END IF;

    IF matched_receipt_count = 1 THEN
        SELECT receipt.*
        INTO receipt_row
        FROM public.product_action_receipts AS receipt
        INNER JOIN (
            SELECT DISTINCT alias.receipt_id
            FROM public.product_action_receipt_idempotency_aliases AS alias
            WHERE alias.tenant_id = expected_tenant_id
                AND alias.installation_id = expected_installation_id
                AND alias.principal_id = expected_principal_id
                AND alias.endpoint_domain = 'product_approve_v1'
                AND alias.idempotency_key_digest = ANY(idempotency_key_digest_candidates)
            ORDER BY alias.receipt_id
            LIMIT 1
        ) AS matched ON matched.receipt_id = receipt.receipt_id
        WHERE receipt.tenant_id = expected_tenant_id
            AND receipt.installation_id = expected_installation_id
            AND receipt.principal_id = expected_principal_id
            AND receipt.endpoint_domain = 'product_approve_v1'
        FOR UPDATE OF receipt;

        IF receipt_row.receipt_id IS NULL THEN
            RETURN QUERY SELECT 'indeterminate', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
            RETURN;
        END IF;
        IF receipt_row.request_digest IS DISTINCT FROM semantic_request_digest THEN
            RETURN QUERY SELECT 'idempotency_conflict', NULL::BIGINT, NULL::TEXT, FALSE,
                NULL::TEXT;
            RETURN;
        END IF;
        IF receipt_row.target_resource_type <> 'authoring_promotion'
            OR receipt_row.target_resource_id IS DISTINCT FROM expected_promotion_id
            OR receipt_row.resulting_revision IS NULL
            OR receipt_row.resulting_state NOT IN ('pending','approved')
            OR receipt_row.result_code NOT IN ('approval_recorded','approval_quorum_reached')
            OR NOT EXISTS (
                SELECT 1
                FROM public.product_audit_events AS audit
                WHERE audit.receipt_id = receipt_row.receipt_id
                    AND audit.tenant_id = receipt_row.tenant_id
                    AND audit.installation_id = receipt_row.installation_id
                    AND audit.principal_id = receipt_row.principal_id
                    AND audit.action = 'promotion.approve'
                    AND audit.target_resource_id = receipt_row.target_resource_id
                    AND audit.resulting_state = receipt_row.resulting_state
                    AND audit.result_code = receipt_row.result_code
            )
        THEN
            RETURN QUERY SELECT 'indeterminate', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
            RETURN;
        END IF;
        INSERT INTO public.product_action_receipt_idempotency_aliases (
            tenant_id,
            installation_id,
            principal_id,
            endpoint_domain,
            idempotency_key_digest,
            idempotency_digest_key_id,
            idempotency_digest_key_fingerprint,
            receipt_id,
            created_at
        )
        SELECT receipt_row.tenant_id,
            receipt_row.installation_id,
            receipt_row.principal_id,
            receipt_row.endpoint_domain,
            idempotency_key_digest_candidates[candidate.ordinal],
            idempotency_digest_key_id_candidates[candidate.ordinal],
            idempotency_digest_key_fingerprint_candidates[candidate.ordinal],
            receipt_row.receipt_id,
            receipt_row.completed_at
        FROM pg_catalog.generate_subscripts(
            idempotency_key_digest_candidates,
            1
        ) AS candidate(ordinal)
        ON CONFLICT (
            tenant_id,
            installation_id,
            principal_id,
            endpoint_domain,
            idempotency_key_digest
        ) DO NOTHING;
        RETURN QUERY SELECT 'ok', receipt_row.resulting_revision,
            receipt_row.resulting_state, TRUE, activation_row.guild_id;
        RETURN;
    END IF;

    IF EXISTS (
        SELECT 1
        FROM public.product_audit_events AS audit
        WHERE audit.tenant_id = expected_tenant_id
            AND audit.request_id = product_request_id
    ) THEN
        RETURN QUERY SELECT 'idempotency_conflict', NULL::BIGINT, NULL::TEXT, FALSE,
            NULL::TEXT;
        RETURN;
    END IF;

    IF promotion_row.stage <> 'activation_pending'
        OR activation_row.product_revision IS DISTINCT FROM expected_product_revision
    THEN
        IF activation_row.product_revision IS DISTINCT FROM expected_product_revision THEN
            RETURN QUERY SELECT 'revision_conflict', NULL::BIGINT, NULL::TEXT, FALSE,
                NULL::TEXT;
        ELSE
            RETURN QUERY SELECT 'invalid_state', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
        END IF;
        RETURN;
    END IF;

    IF activation_row.approval_payload_digest IS DISTINCT FROM expected_payload_digest THEN
        RETURN QUERY SELECT 'payload_mismatch', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
        RETURN;
    END IF;
    IF activation_row.expires_at <= mutation_clock THEN
        RETURN QUERY SELECT 'expired', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
        RETURN;
    END IF;
    IF activation_row.state <> 'pending' THEN
        RETURN QUERY SELECT 'invalid_state', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
        RETURN;
    END IF;
    IF activation_row.requester_id = expected_acting_user_id THEN
        RETURN QUERY SELECT 'self_approval_forbidden', NULL::BIGINT, NULL::TEXT, FALSE,
            NULL::TEXT;
        RETURN;
    END IF;
    IF EXISTS (
        SELECT 1
        FROM public.activation_request_approvals AS approval
        WHERE approval.request_id = activation_row.id
            AND approval.approver_id = expected_acting_user_id
    ) THEN
        RETURN QUERY SELECT 'duplicate_decision', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
        RETURN;
    END IF;

    PERFORM pg_catalog.set_config(
        'starring.product_approval_gate',
        activation_row.approval_context_digest,
        TRUE
    );

    INSERT INTO public.activation_request_approvals (
        request_id,
        tenant_id,
        installation_id,
        approver_id,
        approved_at,
        approval_payload_digest
    ) VALUES (
        activation_row.id,
        expected_tenant_id,
        expected_installation_id,
        expected_acting_user_id,
        mutation_clock,
        expected_payload_digest
    );

    SELECT pg_catalog.count(*)
    INTO approval_count
    FROM public.activation_request_approvals AS approval
    WHERE approval.request_id = activation_row.id;

    IF approval_count >= activation_row.required_approvals THEN
        next_state := 'approved';
        result_code := 'approval_quorum_reached';
    ELSE
        next_state := 'pending';
        result_code := 'approval_recorded';
    END IF;
    next_revision := activation_row.product_revision + 1;

    UPDATE public.activation_requests AS activation
    SET state = next_state,
        product_revision = next_revision
    WHERE activation.id = activation_row.id
        AND activation.state = 'pending'
        AND activation.product_revision = expected_product_revision;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'product approval activation compare-and-swap failed'
            USING ERRCODE = '40001';
    END IF;

    INSERT INTO public.product_action_receipts (
        receipt_id,
        tenant_id,
        installation_id,
        principal_id,
        endpoint_domain,
        idempotency_key_digest,
        idempotency_digest_key_id,
        idempotency_digest_key_fingerprint,
        request_digest,
        target_resource_type,
        target_resource_id,
        resulting_revision,
        resulting_state,
        result_code,
        http_disposition_class,
        completed_at
    ) VALUES (
        new_receipt_id,
        expected_tenant_id,
        expected_installation_id,
        expected_principal_id,
        'product_approve_v1',
        active_idempotency_key_digest,
        idempotency_digest_key_id,
        idempotency_digest_key_fingerprint_candidates[1],
        semantic_request_digest,
        'authoring_promotion',
        expected_promotion_id,
        next_revision,
        next_state,
        result_code,
        2,
        mutation_clock
    );

    INSERT INTO public.product_action_receipt_idempotency_aliases (
        tenant_id,
        installation_id,
        principal_id,
        endpoint_domain,
        idempotency_key_digest,
        idempotency_digest_key_id,
        idempotency_digest_key_fingerprint,
        receipt_id,
        created_at
    )
    SELECT expected_tenant_id,
        expected_installation_id,
        expected_principal_id,
        'product_approve_v1',
        idempotency_key_digest_candidates[candidate.ordinal],
        idempotency_digest_key_id_candidates[candidate.ordinal],
        idempotency_digest_key_fingerprint_candidates[candidate.ordinal],
        new_receipt_id,
        mutation_clock
    FROM pg_catalog.generate_subscripts(
        idempotency_key_digest_candidates,
        1
    ) AS candidate(ordinal);

    IF activation_row.observed_active_version IS NOT NULL THEN
        active_baseline_version := activation_row.observed_active_version;
        active_baseline_hash := activation_row.observed_active_hash;
    END IF;

    INSERT INTO public.product_audit_events (
        event_id,
        tenant_id,
        installation_id,
        principal_id,
        session_subject_digest,
        action,
        target_resource_type,
        target_resource_id,
        request_id,
        receipt_id,
        authority_observation_digest,
        effective_permission_bits,
        authority_observed_at,
        installation_authority_revision,
        payload_digest,
        binding_fingerprint,
        policy_revision,
        active_baseline_version,
        active_baseline_hash,
        resulting_state,
        result_code,
        dependency_latency_classes,
        occurred_at
    ) VALUES (
        new_audit_event_id,
        expected_tenant_id,
        expected_installation_id,
        expected_principal_id,
        session_subject_digest,
        'promotion.approve',
        'authoring_promotion',
        expected_promotion_id,
        product_request_id,
        new_receipt_id,
        expected_authority_observation_digest,
        expected_effective_permission_bits::NUMERIC,
        expected_authority_observed_at,
        expected_authority_revision,
        expected_payload_digest,
        authority_row.binding_fingerprint,
        authority_row.policy_revision,
        active_baseline_version,
        active_baseline_hash,
        next_state,
        result_code,
        '{}'::JSONB,
        mutation_clock
    );

    RETURN QUERY SELECT 'ok', next_revision, next_state, FALSE, activation_row.guild_id;
END;
$function$;

REVOKE ALL ON FUNCTION public.starring_product_approve_v1(
    TEXT,
    TEXT,
    TEXT,
    BIGINT,
    TEXT,
    TEXT,
    BYTEA,
    BYTEA,
    TEXT,
    TEXT,
    TEXT,
    TEXT,
    BIGINT,
    TEXT,
    TEXT,
    TIMESTAMPTZ,
    TIMESTAMPTZ,
    TEXT,
    BOOLEAN,
    TEXT,
    TEXT,
    TEXT[],
    TEXT[],
    TEXT[],
    TEXT,
    TEXT,
    TEXT,
    TEXT
) FROM PUBLIC;
