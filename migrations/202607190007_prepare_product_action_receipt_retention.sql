LOCK TABLE public.product_action_receipts,
    public.product_action_receipt_idempotency_aliases,
    public.product_audit_events
IN SHARE ROW EXCLUSIVE MODE;

DO $function$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM public.product_audit_events AS audit
        LEFT JOIN public.product_action_receipts AS receipt
            ON receipt.tenant_id = audit.tenant_id
            AND receipt.installation_id = audit.installation_id
            AND receipt.principal_id = audit.principal_id
            AND receipt.receipt_id = audit.receipt_id
        WHERE receipt.receipt_id IS NULL
            OR receipt.target_resource_type IS DISTINCT FROM audit.target_resource_type
            OR receipt.target_resource_id IS DISTINCT FROM audit.target_resource_id
            OR receipt.resulting_state IS DISTINCT FROM audit.resulting_state
            OR receipt.result_code IS DISTINCT FROM audit.result_code
    ) THEN
        RAISE EXCEPTION 'product action receipt audit history is inconsistent'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_action_receipt_upgrade_audit_consistent';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM public.product_action_receipts AS receipt
        WHERE receipt.endpoint_domain = 'product_approve_v1'
            AND NOT EXISTS (
                SELECT 1
                FROM public.product_audit_events AS audit
                WHERE audit.tenant_id = receipt.tenant_id
                    AND audit.installation_id = receipt.installation_id
                    AND audit.principal_id = receipt.principal_id
                    AND audit.receipt_id = receipt.receipt_id
                    AND audit.action = 'promotion.approve'
                    AND audit.target_resource_type = receipt.target_resource_type
                    AND audit.target_resource_id = receipt.target_resource_id
                    AND audit.resulting_state = receipt.resulting_state
                    AND audit.result_code = receipt.result_code
            )
    ) THEN
        RAISE EXCEPTION 'product approval receipt audit history is incomplete'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_action_receipt_upgrade_approval_audit_complete';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM public.product_action_receipts AS receipt
        WHERE receipt.endpoint_domain = 'product_approve_v1'
            AND NOT EXISTS (
                SELECT 1
                FROM public.product_action_receipt_idempotency_aliases AS alias
                WHERE alias.tenant_id = receipt.tenant_id
                    AND alias.installation_id = receipt.installation_id
                    AND alias.principal_id = receipt.principal_id
                    AND alias.endpoint_domain = receipt.endpoint_domain
                    AND alias.idempotency_key_digest = receipt.idempotency_key_digest
                    AND alias.idempotency_digest_key_id
                        = receipt.idempotency_digest_key_id
                    AND alias.idempotency_digest_key_fingerprint
                        = receipt.idempotency_digest_key_fingerprint
                    AND alias.receipt_id = receipt.receipt_id
            )
    ) THEN
        RAISE EXCEPTION 'product approval receipt primary alias is incomplete'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_action_receipt_upgrade_primary_alias_complete';
    END IF;

    IF EXISTS (
        SELECT alias.receipt_id
        FROM public.product_action_receipt_idempotency_aliases AS alias
        GROUP BY alias.tenant_id,
            alias.installation_id,
            alias.principal_id,
            alias.endpoint_domain,
            alias.receipt_id
        HAVING pg_catalog.count(*) > 32
    ) THEN
        RAISE EXCEPTION 'product action receipt alias capacity is exceeded'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_action_receipt_alias_capacity_valid';
    END IF;
END;
$function$;

CREATE TABLE public.product_action_receipt_audit_evidence (
    receipt_id TEXT PRIMARY KEY,
    event_id TEXT NOT NULL UNIQUE,
    tenant_id TEXT NOT NULL,
    installation_id TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    endpoint_domain TEXT NOT NULL,
    action TEXT NOT NULL,
    request_digest TEXT NOT NULL,
    target_resource_type TEXT NOT NULL,
    target_resource_id TEXT NOT NULL,
    resulting_revision BIGINT,
    resulting_state TEXT NOT NULL,
    result_code TEXT NOT NULL,
    http_disposition_class SMALLINT NOT NULL,
    completed_at TIMESTAMPTZ NOT NULL,
    evidence_version SMALLINT NOT NULL,
    replay_policy_version SMALLINT NOT NULL,
    replay_guaranteed_until TIMESTAMPTZ NOT NULL,
    CONSTRAINT product_action_receipt_audit_evidence_event_fk FOREIGN KEY (event_id)
        REFERENCES public.product_audit_events (event_id)
        ON DELETE RESTRICT
        DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT product_action_receipt_audit_evidence_scope_valid CHECK (
        receipt_id ~ '^[0-9a-f]{64}$'
        AND event_id ~ '^[0-9a-f]{64}$'
        AND tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND principal_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND endpoint_domain ~ '^[a-z][a-z0-9_.:-]{0,63}$'
        AND action ~ '^[a-z][a-z0-9_.:-]{0,63}$'
        AND request_digest ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT product_action_receipt_audit_evidence_target_valid CHECK (
        target_resource_type ~ '^[a-z][a-z0-9_]{0,63}$'
        AND target_resource_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT product_action_receipt_audit_evidence_result_valid CHECK (
        resulting_revision IS NULL
        OR resulting_revision BETWEEN 1 AND 9223372036854775807
    ),
    CONSTRAINT product_action_receipt_audit_evidence_disposition_valid CHECK (
        resulting_state ~ '^[a-z][a-z0-9_]{0,63}$'
        AND result_code ~ '^[a-z][a-z0-9_.:-]{0,63}$'
        AND http_disposition_class IN (2, 4)
    ),
    CONSTRAINT product_action_receipt_audit_evidence_replay_valid CHECK (
        evidence_version = 1
        AND replay_policy_version = 1
        AND replay_guaranteed_until = completed_at + INTERVAL '168 hours'
    )
);

INSERT INTO public.product_action_receipt_audit_evidence (
    receipt_id,
    event_id,
    tenant_id,
    installation_id,
    principal_id,
    endpoint_domain,
    action,
    request_digest,
    target_resource_type,
    target_resource_id,
    resulting_revision,
    resulting_state,
    result_code,
    http_disposition_class,
    completed_at,
    evidence_version,
    replay_policy_version,
    replay_guaranteed_until
)
SELECT receipt.receipt_id,
    audit.event_id,
    receipt.tenant_id,
    receipt.installation_id,
    receipt.principal_id,
    receipt.endpoint_domain,
    audit.action,
    receipt.request_digest,
    receipt.target_resource_type,
    receipt.target_resource_id,
    receipt.resulting_revision,
    receipt.resulting_state,
    receipt.result_code,
    receipt.http_disposition_class,
    receipt.completed_at,
    1,
    1,
    receipt.completed_at + INTERVAL '168 hours'
FROM public.product_audit_events AS audit
INNER JOIN public.product_action_receipts AS receipt
    ON receipt.tenant_id = audit.tenant_id
    AND receipt.installation_id = audit.installation_id
    AND receipt.principal_id = audit.principal_id
    AND receipt.receipt_id = audit.receipt_id
    AND receipt.target_resource_type = audit.target_resource_type
    AND receipt.target_resource_id = audit.target_resource_id
    AND receipt.resulting_state = audit.resulting_state
    AND receipt.result_code = audit.result_code;

DO $function$
BEGIN
    IF (
        SELECT pg_catalog.count(*)
        FROM public.product_action_receipt_audit_evidence
    ) IS DISTINCT FROM (
        SELECT pg_catalog.count(*)
        FROM public.product_audit_events
    ) THEN
        RAISE EXCEPTION 'product action receipt evidence backfill is incomplete'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_action_receipt_evidence_backfill_complete';
    END IF;
END;
$function$;

CREATE FUNCTION public.capture_product_action_receipt_audit_evidence()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    receipt_row public.product_action_receipts%ROWTYPE;
BEGIN
    SELECT receipt.*
    INTO receipt_row
    FROM public.product_action_receipts AS receipt
    WHERE receipt.tenant_id = NEW.tenant_id
        AND receipt.installation_id = NEW.installation_id
        AND receipt.principal_id = NEW.principal_id
        AND receipt.receipt_id = NEW.receipt_id
    FOR SHARE;

    IF receipt_row.receipt_id IS NULL
        OR receipt_row.target_resource_type IS DISTINCT FROM NEW.target_resource_type
        OR receipt_row.target_resource_id IS DISTINCT FROM NEW.target_resource_id
        OR receipt_row.resulting_state IS DISTINCT FROM NEW.resulting_state
        OR receipt_row.result_code IS DISTINCT FROM NEW.result_code
        OR (
            receipt_row.endpoint_domain = 'product_approve_v1'
            AND NEW.action <> 'promotion.approve'
        )
    THEN
        RAISE EXCEPTION 'product action receipt audit evidence is inconsistent'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_action_receipt_audit_evidence_consistent';
    END IF;

    INSERT INTO public.product_action_receipt_audit_evidence (
        receipt_id,
        event_id,
        tenant_id,
        installation_id,
        principal_id,
        endpoint_domain,
        action,
        request_digest,
        target_resource_type,
        target_resource_id,
        resulting_revision,
        resulting_state,
        result_code,
        http_disposition_class,
        completed_at,
        evidence_version,
        replay_policy_version,
        replay_guaranteed_until
    ) VALUES (
        receipt_row.receipt_id,
        NEW.event_id,
        receipt_row.tenant_id,
        receipt_row.installation_id,
        receipt_row.principal_id,
        receipt_row.endpoint_domain,
        NEW.action,
        receipt_row.request_digest,
        receipt_row.target_resource_type,
        receipt_row.target_resource_id,
        receipt_row.resulting_revision,
        receipt_row.resulting_state,
        receipt_row.result_code,
        receipt_row.http_disposition_class,
        receipt_row.completed_at,
        1,
        1,
        receipt_row.completed_at + INTERVAL '168 hours'
    );
    RETURN NULL;
END;
$function$;

CREATE TRIGGER product_audit_events_capture_receipt_evidence
AFTER INSERT ON public.product_audit_events
FOR EACH ROW
EXECUTE FUNCTION public.capture_product_action_receipt_audit_evidence();

REVOKE ALL ON FUNCTION public.capture_product_action_receipt_audit_evidence()
FROM PUBLIC;

ALTER TABLE public.product_audit_events
ADD CONSTRAINT product_audit_events_receipt_evidence_fk FOREIGN KEY (receipt_id)
    REFERENCES public.product_action_receipt_audit_evidence (receipt_id)
    ON DELETE RESTRICT
    DEFERRABLE INITIALLY DEFERRED
    NOT VALID;

ALTER TABLE public.product_audit_events
VALIDATE CONSTRAINT product_audit_events_receipt_evidence_fk;

ALTER TABLE public.product_audit_events
ADD CONSTRAINT product_audit_events_principal_fk FOREIGN KEY (principal_id)
    REFERENCES public.product_principals (principal_id)
    ON DELETE RESTRICT
    NOT VALID;

ALTER TABLE public.product_audit_events
VALIDATE CONSTRAINT product_audit_events_principal_fk;

ALTER TABLE public.product_audit_events
DROP CONSTRAINT product_audit_events_receipt_fk;

CREATE TRIGGER product_action_receipt_audit_evidence_reject_mutation
BEFORE UPDATE OR DELETE ON public.product_action_receipt_audit_evidence
FOR EACH ROW
EXECUTE FUNCTION public.reject_immutable_product_row();

CREATE FUNCTION public.enforce_product_action_receipt_alias_capacity()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    existing_alias_count BIGINT;
BEGIN
    PERFORM 1
    FROM public.product_action_receipts AS receipt
    WHERE receipt.tenant_id = NEW.tenant_id
        AND receipt.installation_id = NEW.installation_id
        AND receipt.principal_id = NEW.principal_id
        AND receipt.endpoint_domain = NEW.endpoint_domain
        AND receipt.receipt_id = NEW.receipt_id
    FOR UPDATE;

    IF NOT FOUND OR EXISTS (
        SELECT 1
        FROM public.product_action_receipt_idempotency_aliases AS alias
        WHERE alias.tenant_id = NEW.tenant_id
            AND alias.installation_id = NEW.installation_id
            AND alias.principal_id = NEW.principal_id
            AND alias.endpoint_domain = NEW.endpoint_domain
            AND alias.idempotency_key_digest = NEW.idempotency_key_digest
    ) THEN
        RETURN NEW;
    END IF;

    SELECT pg_catalog.count(*)
    INTO existing_alias_count
    FROM public.product_action_receipt_idempotency_aliases AS alias
    WHERE alias.tenant_id = NEW.tenant_id
        AND alias.installation_id = NEW.installation_id
        AND alias.principal_id = NEW.principal_id
        AND alias.endpoint_domain = NEW.endpoint_domain
        AND alias.receipt_id = NEW.receipt_id;

    IF existing_alias_count >= 32 THEN
        RAISE EXCEPTION 'product action receipt alias capacity is exceeded'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_action_receipt_alias_capacity_valid';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER product_action_receipt_idempotency_aliases_enforce_capacity
BEFORE INSERT ON public.product_action_receipt_idempotency_aliases
FOR EACH ROW
EXECUTE FUNCTION public.enforce_product_action_receipt_alias_capacity();

REVOKE ALL ON FUNCTION public.enforce_product_action_receipt_alias_capacity()
FROM PUBLIC;

CREATE FUNCTION public.enforce_product_action_receipt_retention()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    IF TG_OP = 'UPDATE'
        OR pg_catalog.current_setting(
            'starring.product_action_receipt_retention_gate',
            TRUE
        ) IS DISTINCT FROM 'starring.product.action.receipt.retention.v1'
    THEN
        RAISE EXCEPTION 'immutable product records cannot be updated or deleted'
            USING ERRCODE = '23514';
    END IF;

    IF OLD.endpoint_domain <> 'product_approve_v1'
        OR EXISTS (
            SELECT 1
            FROM public.product_action_receipt_idempotency_aliases AS alias
            WHERE alias.tenant_id = OLD.tenant_id
                AND alias.installation_id = OLD.installation_id
                AND alias.principal_id = OLD.principal_id
                AND alias.endpoint_domain = OLD.endpoint_domain
                AND alias.receipt_id = OLD.receipt_id
        )
        OR NOT EXISTS (
            SELECT 1
            FROM public.product_action_receipt_audit_evidence AS evidence
            WHERE evidence.receipt_id = OLD.receipt_id
                AND evidence.tenant_id = OLD.tenant_id
                AND evidence.installation_id = OLD.installation_id
                AND evidence.principal_id = OLD.principal_id
                AND evidence.endpoint_domain = OLD.endpoint_domain
                AND evidence.action = 'promotion.approve'
                AND evidence.request_digest = OLD.request_digest
                AND evidence.target_resource_type = OLD.target_resource_type
                AND evidence.target_resource_id = OLD.target_resource_id
                AND evidence.resulting_revision IS NOT DISTINCT FROM OLD.resulting_revision
                AND evidence.resulting_state = OLD.resulting_state
                AND evidence.result_code = OLD.result_code
                AND evidence.http_disposition_class = OLD.http_disposition_class
                AND evidence.completed_at = OLD.completed_at
                AND evidence.replay_policy_version = 1
                AND evidence.replay_guaranteed_until
                    <= pg_catalog.clock_timestamp()
        )
    THEN
        RAISE EXCEPTION 'product action receipt is not retention eligible'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_action_receipt_retention_eligible';
    END IF;
    RETURN OLD;
END;
$function$;

CREATE FUNCTION public.enforce_product_action_receipt_alias_retention()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    IF TG_OP = 'UPDATE'
        OR pg_catalog.current_setting(
            'starring.product_action_receipt_retention_gate',
            TRUE
        ) IS DISTINCT FROM 'starring.product.action.receipt.retention.v1'
    THEN
        RAISE EXCEPTION 'immutable product records cannot be updated or deleted'
            USING ERRCODE = '23514';
    END IF;

    IF OLD.endpoint_domain <> 'product_approve_v1'
        OR NOT EXISTS (
            SELECT 1
            FROM public.product_action_receipts AS receipt
            INNER JOIN public.product_action_receipt_audit_evidence AS evidence
                ON evidence.receipt_id = receipt.receipt_id
                AND evidence.tenant_id = receipt.tenant_id
                AND evidence.installation_id = receipt.installation_id
                AND evidence.principal_id = receipt.principal_id
                AND evidence.endpoint_domain = receipt.endpoint_domain
                AND evidence.action = 'promotion.approve'
                AND evidence.request_digest = receipt.request_digest
                AND evidence.target_resource_type = receipt.target_resource_type
                AND evidence.target_resource_id = receipt.target_resource_id
                AND evidence.resulting_revision
                    IS NOT DISTINCT FROM receipt.resulting_revision
                AND evidence.resulting_state = receipt.resulting_state
                AND evidence.result_code = receipt.result_code
                AND evidence.http_disposition_class = receipt.http_disposition_class
                AND evidence.completed_at = receipt.completed_at
            WHERE receipt.tenant_id = OLD.tenant_id
                AND receipt.installation_id = OLD.installation_id
                AND receipt.principal_id = OLD.principal_id
                AND receipt.endpoint_domain = OLD.endpoint_domain
                AND receipt.receipt_id = OLD.receipt_id
                AND evidence.replay_policy_version = 1
                AND evidence.replay_guaranteed_until
                    <= pg_catalog.clock_timestamp()
        )
    THEN
        RAISE EXCEPTION 'product action receipt alias is not retention eligible'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_action_receipt_alias_retention_eligible';
    END IF;
    RETURN OLD;
END;
$function$;

DROP TRIGGER product_action_receipts_reject_mutation
ON public.product_action_receipts;

CREATE TRIGGER product_action_receipts_reject_mutation
BEFORE UPDATE OR DELETE ON public.product_action_receipts
FOR EACH ROW
EXECUTE FUNCTION public.enforce_product_action_receipt_retention();

DROP TRIGGER product_action_receipt_idempotency_aliases_reject_mutation
ON public.product_action_receipt_idempotency_aliases;

CREATE TRIGGER product_action_receipt_idempotency_aliases_reject_mutation
BEFORE UPDATE OR DELETE ON public.product_action_receipt_idempotency_aliases
FOR EACH ROW
EXECUTE FUNCTION public.enforce_product_action_receipt_alias_retention();

REVOKE ALL ON FUNCTION public.enforce_product_action_receipt_retention()
FROM PUBLIC;

REVOKE ALL ON FUNCTION public.enforce_product_action_receipt_alias_retention()
FROM PUBLIC;

CREATE INDEX product_action_receipts_approval_retention_index
ON public.product_action_receipts (completed_at, receipt_id)
WHERE endpoint_domain = 'product_approve_v1';

CREATE INDEX product_action_aliases_receipt_retention_index
ON public.product_action_receipt_idempotency_aliases (receipt_id)
WHERE endpoint_domain = 'product_approve_v1';

CREATE FUNCTION public.starring_purge_product_action_receipts_v1(batch_limit INTEGER)
RETURNS TABLE (
    deleted_receipts INTEGER,
    deleted_aliases INTEGER,
    backlog_remaining BOOLEAN
)
LANGUAGE plpgsql
VOLATILE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    retention_clock TIMESTAMPTZ;
    candidate_receipt_ids TEXT[];
    receipt_count INTEGER;
    alias_count INTEGER;
    backlog BOOLEAN;
BEGIN
    IF batch_limit IS NULL OR batch_limit NOT BETWEEN 1 AND 1000 THEN
        RAISE EXCEPTION 'product action receipt purge batch limit is invalid'
            USING ERRCODE = '22023',
                CONSTRAINT = 'product_action_receipt_purge_batch_limit_valid';
    END IF;

    retention_clock := pg_catalog.clock_timestamp();

    SELECT COALESCE(
        pg_catalog.array_agg(candidate.receipt_id),
        ARRAY[]::TEXT[]
    )
    INTO candidate_receipt_ids
    FROM (
        SELECT receipt.receipt_id
        FROM public.product_action_receipts AS receipt
        WHERE receipt.endpoint_domain = 'product_approve_v1'
            AND receipt.completed_at <= retention_clock - INTERVAL '168 hours'
        ORDER BY receipt.completed_at, receipt.receipt_id
        FOR UPDATE OF receipt SKIP LOCKED
        LIMIT batch_limit
    ) AS candidate;

    IF EXISTS (
        SELECT 1
        FROM public.product_action_receipts AS receipt
        WHERE receipt.receipt_id = ANY(candidate_receipt_ids)
            AND (
                receipt.endpoint_domain <> 'product_approve_v1'
                OR NOT EXISTS (
                    SELECT 1
                    FROM public.product_action_receipt_audit_evidence AS evidence
                    WHERE evidence.receipt_id = receipt.receipt_id
                        AND evidence.tenant_id = receipt.tenant_id
                        AND evidence.installation_id = receipt.installation_id
                        AND evidence.principal_id = receipt.principal_id
                        AND evidence.endpoint_domain = receipt.endpoint_domain
                        AND evidence.action = 'promotion.approve'
                        AND evidence.request_digest = receipt.request_digest
                        AND evidence.target_resource_type = receipt.target_resource_type
                        AND evidence.target_resource_id = receipt.target_resource_id
                        AND evidence.resulting_revision
                            IS NOT DISTINCT FROM receipt.resulting_revision
                        AND evidence.resulting_state = receipt.resulting_state
                        AND evidence.result_code = receipt.result_code
                        AND evidence.http_disposition_class = receipt.http_disposition_class
                        AND evidence.completed_at = receipt.completed_at
                        AND evidence.replay_policy_version = 1
                        AND evidence.replay_guaranteed_until <= retention_clock
                )
                OR NOT EXISTS (
                    SELECT 1
                    FROM public.product_action_receipt_idempotency_aliases AS alias
                    WHERE alias.tenant_id = receipt.tenant_id
                        AND alias.installation_id = receipt.installation_id
                        AND alias.principal_id = receipt.principal_id
                        AND alias.endpoint_domain = receipt.endpoint_domain
                        AND alias.idempotency_key_digest
                            = receipt.idempotency_key_digest
                        AND alias.idempotency_digest_key_id
                            = receipt.idempotency_digest_key_id
                        AND alias.idempotency_digest_key_fingerprint
                            = receipt.idempotency_digest_key_fingerprint
                        AND alias.receipt_id = receipt.receipt_id
                )
            )
    ) THEN
        RAISE EXCEPTION 'product action receipt retention evidence is incomplete'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_action_receipt_retention_evidence_complete';
    END IF;

    IF EXISTS (
        SELECT alias.receipt_id
        FROM public.product_action_receipt_idempotency_aliases AS alias
        WHERE alias.endpoint_domain = 'product_approve_v1'
            AND alias.receipt_id = ANY(candidate_receipt_ids)
        GROUP BY alias.tenant_id,
            alias.installation_id,
            alias.principal_id,
            alias.endpoint_domain,
            alias.receipt_id
        HAVING pg_catalog.count(*) > 32
    ) THEN
        RAISE EXCEPTION 'product action receipt alias capacity is exceeded'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_action_receipt_alias_capacity_valid';
    END IF;

    PERFORM pg_catalog.set_config(
        'starring.product_action_receipt_retention_gate',
        'starring.product.action.receipt.retention.v1',
        TRUE
    );

    DELETE FROM public.product_action_receipt_idempotency_aliases AS alias
    WHERE alias.endpoint_domain = 'product_approve_v1'
        AND alias.receipt_id = ANY(candidate_receipt_ids);
    GET DIAGNOSTICS alias_count = ROW_COUNT;

    DELETE FROM public.product_action_receipts AS receipt
    WHERE receipt.receipt_id = ANY(candidate_receipt_ids)
        AND receipt.endpoint_domain = 'product_approve_v1';
    GET DIAGNOSTICS receipt_count = ROW_COUNT;

    IF receipt_count IS DISTINCT FROM pg_catalog.cardinality(candidate_receipt_ids) THEN
        RAISE EXCEPTION 'product action receipt purge did not delete its locked batch'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_action_receipt_purge_batch_complete';
    END IF;

    SELECT EXISTS (
        SELECT 1
        FROM public.product_action_receipts AS receipt
        WHERE receipt.endpoint_domain = 'product_approve_v1'
            AND receipt.completed_at <= retention_clock - INTERVAL '168 hours'
        ORDER BY receipt.completed_at, receipt.receipt_id
        LIMIT 1
    )
    INTO backlog;

    PERFORM pg_catalog.set_config(
        'starring.product_action_receipt_retention_gate',
        '',
        TRUE
    );
    RETURN QUERY SELECT receipt_count, alias_count, backlog;
EXCEPTION
    WHEN OTHERS THEN
        PERFORM pg_catalog.set_config(
            'starring.product_action_receipt_retention_gate',
            '',
            TRUE
        );
        RAISE;
END;
$function$;

REVOKE ALL ON FUNCTION public.starring_purge_product_action_receipts_v1(INTEGER)
FROM PUBLIC;

CREATE FUNCTION public.starring_product_approval_keyring_coverage_v1(
    idempotency_digest_key_id_candidates TEXT[],
    idempotency_digest_key_fingerprint_candidates TEXT[]
)
RETURNS TABLE (outcome TEXT)
LANGUAGE plpgsql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    IF pg_catalog.array_ndims(idempotency_digest_key_id_candidates) IS DISTINCT FROM 1
        OR pg_catalog.array_lower(idempotency_digest_key_id_candidates, 1)
            IS DISTINCT FROM 1
        OR pg_catalog.cardinality(idempotency_digest_key_id_candidates) NOT BETWEEN 1 AND 8
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.unnest(idempotency_digest_key_id_candidates)
                AS candidate(key_id)
            WHERE candidate.key_id !~ '^[A-Za-z0-9_.:-]{1,64}$'
        )
        OR pg_catalog.array_ndims(idempotency_digest_key_fingerprint_candidates)
            IS DISTINCT FROM 1
        OR pg_catalog.array_lower(idempotency_digest_key_fingerprint_candidates, 1)
            IS DISTINCT FROM 1
        OR pg_catalog.cardinality(idempotency_digest_key_fingerprint_candidates)
            IS DISTINCT FROM pg_catalog.cardinality(idempotency_digest_key_id_candidates)
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.unnest(idempotency_digest_key_fingerprint_candidates)
                AS candidate(fingerprint)
            WHERE candidate.fingerprint !~ '^[0-9a-f]{64}$'
        )
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
        RETURN QUERY SELECT 'invalid_input';
        RETURN;
    END IF;

    IF EXISTS (
        SELECT 1
        FROM public.product_action_receipts AS receipt
        WHERE receipt.endpoint_domain = 'product_approve_v1'
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
        RETURN QUERY SELECT 'idempotency_keyring_incomplete';
        RETURN;
    END IF;

    RETURN QUERY SELECT 'ok';
END;
$function$;

REVOKE ALL ON FUNCTION public.starring_product_approval_keyring_coverage_v1(
    TEXT[],
    TEXT[]
) FROM PUBLIC;

REVOKE ALL ON TABLE public.product_action_receipt_audit_evidence
FROM PUBLIC;
