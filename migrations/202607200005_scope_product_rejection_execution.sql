SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

LOCK TABLE
    public.product_control_plane_identity,
    public.activation_requests,
    public.authoring_promotions,
    public.product_tenants,
    public.automation_installations,
    public.automation_installation_authority_versions,
    public.product_principals,
    public.product_auth_sessions,
    public.product_action_receipts,
    public.product_action_receipt_idempotency_aliases,
    public.product_audit_events,
    public.product_action_receipt_audit_evidence,
    public.activation_request_approvals
IN SHARE ROW EXCLUSIVE MODE;

DO $preflight$
DECLARE
    relation_count BIGINT;
    owner_count BIGINT;
    common_owner OID;
    common_owner_name NAME;
    invalid_function_count BIGINT;
    collision_count BIGINT;
    unsafe_schema_create_count BIGINT;
BEGIN
    SELECT pg_catalog.count(relation.oid),
        pg_catalog.count(DISTINCT relation.relowner),
        pg_catalog.min(relation.relowner::BIGINT)::OID
    INTO relation_count, owner_count, common_owner
    FROM (
        VALUES
            (pg_catalog.to_regclass('public.product_control_plane_identity')),
            (pg_catalog.to_regclass('public.activation_requests')),
            (pg_catalog.to_regclass('public.authoring_promotions')),
            (pg_catalog.to_regclass('public.product_tenants')),
            (pg_catalog.to_regclass('public.automation_installations')),
            (pg_catalog.to_regclass('public.automation_installation_authority_versions')),
            (pg_catalog.to_regclass('public.product_principals')),
            (pg_catalog.to_regclass('public.product_auth_sessions')),
            (pg_catalog.to_regclass('public.product_action_receipts')),
            (pg_catalog.to_regclass('public.product_action_receipt_idempotency_aliases')),
            (pg_catalog.to_regclass('public.product_audit_events')),
            (pg_catalog.to_regclass('public.product_action_receipt_audit_evidence')),
            (pg_catalog.to_regclass('public.activation_request_approvals'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid
        AND relation.relkind = 'r'
        AND relation.relpersistence = 'p'
        AND NOT relation.relrowsecurity
        AND NOT relation.relforcerowsecurity;

    IF relation_count <> 13
        OR owner_count <> 1
        OR common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
    THEN
        RAISE EXCEPTION 'product rejection relations require their common owner'
            USING ERRCODE = '55000';
    END IF;

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL
        OR NOT pg_catalog.has_schema_privilege(common_owner_name, 'public', 'CREATE')
    THEN
        RAISE EXCEPTION 'product rejection relation owner is unavailable'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO unsafe_schema_create_count
    FROM pg_catalog.pg_namespace AS namespace
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        namespace.nspacl,
        pg_catalog.acldefault('n', namespace.nspowner)
    )) AS privilege
    WHERE namespace.nspname = 'public'
        AND privilege.privilege_type = 'CREATE'
        AND privilege.grantee <> namespace.nspowner
        AND privilege.grantee <> pg_catalog.to_regrole('pg_database_owner');

    IF unsafe_schema_create_count <> 0
        OR NOT EXISTS (
            SELECT 1
            FROM pg_catalog.pg_namespace AS namespace
            WHERE namespace.nspname = 'public'
                AND namespace.nspowner IN (
                    common_owner,
                    pg_catalog.to_regrole('pg_database_owner'),
                    (
                        SELECT database_row.datdba
                        FROM pg_catalog.pg_database AS database_row
                        WHERE database_row.datname = pg_catalog.current_database()
                    )
                )
        )
    THEN
        RAISE EXCEPTION 'product rejection schema is not trusted'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            ('public.enforce_product_activation_executor()', TRUE),
            ('public.assert_product_approval_receipt_alias()', TRUE),
            ('public.assert_product_approval_receipt_audit()', TRUE),
            ('public.capture_product_action_receipt_audit_evidence()', TRUE),
            ('public.enforce_product_action_receipt_retention()', TRUE),
            ('public.enforce_product_action_receipt_alias_retention()', TRUE),
            ('public.starring_purge_product_action_receipts_v1(integer)', FALSE)
    ) AS expected(signature, strict_input)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.signature)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR function_row.proisstrict <> expected.strict_input
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR function_row.proconfig IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR language_row.lanname IS DISTINCT FROM 'plpgsql'
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> function_row.proowner
        );

    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION 'product rejection support function contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_product_rejection_executor_database_identity_v1',
            'starring_product_rejection_keyring_coverage_v1',
            'starring_product_reject_v1'
        );

    IF collision_count <> 0
        OR pg_catalog.to_regclass(
            'public.product_action_receipts_rejection_retention_index'
        ) IS NOT NULL
        OR pg_catalog.to_regclass(
            'public.product_action_aliases_rejection_receipt_retention_index'
        ) IS NOT NULL
    THEN
        RAISE EXCEPTION 'product rejection object identity collides with existing state'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM public.activation_requests AS activation
        WHERE activation.state = 'rejected'
            AND (
                activation.rejected_at IS NULL
                OR activation.rejected_by IS NULL
                OR activation.rejection_reason IS NULL
                OR activation.rejected_at < activation.created_at
                OR activation.rejected_at >= activation.expires_at
                OR NOT (CASE
                    WHEN activation.rejected_by ~ '^[1-9][0-9]{0,19}$'
                        THEN activation.rejected_by::NUMERIC <= 18446744073709551615
                    ELSE FALSE
                END)
                OR activation.rejection_reason
                    IS DISTINCT FROM pg_catalog.btrim(activation.rejection_reason)
                OR pg_catalog.char_length(activation.rejection_reason) NOT BETWEEN 1 AND 1000
                OR pg_catalog.octet_length(activation.rejection_reason) > 4000
                OR activation.rejection_reason ~ U&'[\0001-\001F\007F-\009F]'
                OR pg_catalog.left(activation.rejection_reason, 1) = ANY(ARRAY[
                    pg_catalog.chr(9),
                    pg_catalog.chr(10),
                    pg_catalog.chr(11),
                    pg_catalog.chr(12),
                    pg_catalog.chr(13),
                    pg_catalog.chr(32),
                    pg_catalog.chr(133),
                    pg_catalog.chr(160),
                    pg_catalog.chr(5760),
                    pg_catalog.chr(8192),
                    pg_catalog.chr(8193),
                    pg_catalog.chr(8194),
                    pg_catalog.chr(8195),
                    pg_catalog.chr(8196),
                    pg_catalog.chr(8197),
                    pg_catalog.chr(8198),
                    pg_catalog.chr(8199),
                    pg_catalog.chr(8200),
                    pg_catalog.chr(8201),
                    pg_catalog.chr(8202),
                    pg_catalog.chr(8232),
                    pg_catalog.chr(8233),
                    pg_catalog.chr(8239),
                    pg_catalog.chr(8287),
                    pg_catalog.chr(12288)
                ])
                OR pg_catalog.right(activation.rejection_reason, 1) = ANY(ARRAY[
                    pg_catalog.chr(9),
                    pg_catalog.chr(10),
                    pg_catalog.chr(11),
                    pg_catalog.chr(12),
                    pg_catalog.chr(13),
                    pg_catalog.chr(32),
                    pg_catalog.chr(133),
                    pg_catalog.chr(160),
                    pg_catalog.chr(5760),
                    pg_catalog.chr(8192),
                    pg_catalog.chr(8193),
                    pg_catalog.chr(8194),
                    pg_catalog.chr(8195),
                    pg_catalog.chr(8196),
                    pg_catalog.chr(8197),
                    pg_catalog.chr(8198),
                    pg_catalog.chr(8199),
                    pg_catalog.chr(8200),
                    pg_catalog.chr(8201),
                    pg_catalog.chr(8202),
                    pg_catalog.chr(8232),
                    pg_catalog.chr(8233),
                    pg_catalog.chr(8239),
                    pg_catalog.chr(8287),
                    pg_catalog.chr(12288)
                ])
            )
    ) OR EXISTS (
        SELECT 1
        FROM public.activation_requests AS activation
        WHERE activation.state <> 'rejected'
            AND (
                activation.rejected_at IS NOT NULL
                OR activation.rejected_by IS NOT NULL
                OR activation.rejection_reason IS NOT NULL
            )
    ) THEN
        RAISE EXCEPTION 'product rejection migration found invalid rejection state'
            USING ERRCODE = '55000';
    END IF;
END;
$preflight$;

ALTER TABLE public.activation_requests
DROP CONSTRAINT activation_requests_rejected_fields_valid,
ADD CONSTRAINT activation_requests_rejected_fields_valid CHECK (
    (
        state = 'rejected'
        AND rejected_at IS NOT NULL
        AND rejected_by IS NOT NULL
        AND rejection_reason IS NOT NULL
        AND rejected_at >= created_at
        AND rejected_at < expires_at
        AND CASE
            WHEN rejected_by ~ '^[1-9][0-9]{0,19}$'
                THEN rejected_by::NUMERIC <= 18446744073709551615
            ELSE FALSE
        END
        AND rejection_reason = pg_catalog.btrim(rejection_reason)
        AND pg_catalog.char_length(rejection_reason) BETWEEN 1 AND 1000
        AND pg_catalog.octet_length(rejection_reason) <= 4000
        AND rejection_reason !~ U&'[\0001-\001F\007F-\009F]'
        AND pg_catalog.left(rejection_reason, 1) <> ALL(ARRAY[
            pg_catalog.chr(9),
            pg_catalog.chr(10),
            pg_catalog.chr(11),
            pg_catalog.chr(12),
            pg_catalog.chr(13),
            pg_catalog.chr(32),
            pg_catalog.chr(133),
            pg_catalog.chr(160),
            pg_catalog.chr(5760),
            pg_catalog.chr(8192),
            pg_catalog.chr(8193),
            pg_catalog.chr(8194),
            pg_catalog.chr(8195),
            pg_catalog.chr(8196),
            pg_catalog.chr(8197),
            pg_catalog.chr(8198),
            pg_catalog.chr(8199),
            pg_catalog.chr(8200),
            pg_catalog.chr(8201),
            pg_catalog.chr(8202),
            pg_catalog.chr(8232),
            pg_catalog.chr(8233),
            pg_catalog.chr(8239),
            pg_catalog.chr(8287),
            pg_catalog.chr(12288)
        ])
        AND pg_catalog.right(rejection_reason, 1) <> ALL(ARRAY[
            pg_catalog.chr(9),
            pg_catalog.chr(10),
            pg_catalog.chr(11),
            pg_catalog.chr(12),
            pg_catalog.chr(13),
            pg_catalog.chr(32),
            pg_catalog.chr(133),
            pg_catalog.chr(160),
            pg_catalog.chr(5760),
            pg_catalog.chr(8192),
            pg_catalog.chr(8193),
            pg_catalog.chr(8194),
            pg_catalog.chr(8195),
            pg_catalog.chr(8196),
            pg_catalog.chr(8197),
            pg_catalog.chr(8198),
            pg_catalog.chr(8199),
            pg_catalog.chr(8200),
            pg_catalog.chr(8201),
            pg_catalog.chr(8202),
            pg_catalog.chr(8232),
            pg_catalog.chr(8233),
            pg_catalog.chr(8239),
            pg_catalog.chr(8287),
            pg_catalog.chr(12288)
        ])
    ) OR (
        state <> 'rejected'
        AND rejected_at IS NULL
        AND rejected_by IS NULL
        AND rejection_reason IS NULL
    )
);

ALTER TABLE public.product_action_receipts
DROP CONSTRAINT product_action_receipts_approval_key_identity_required,
ADD CONSTRAINT product_action_receipts_approval_key_identity_required CHECK (
    endpoint_domain NOT IN (
        'product_approve_v1',
        'product_apply_v1',
        'product_promote_v1',
        'product_reject_v1'
    ) OR (
        idempotency_digest_key_id IS NOT NULL
        AND idempotency_digest_key_fingerprint IS NOT NULL
    )
);

CREATE INDEX product_action_receipts_rejection_retention_index
ON public.product_action_receipts (completed_at, receipt_id)
WHERE endpoint_domain = 'product_reject_v1';

CREATE INDEX product_action_aliases_rejection_receipt_retention_index
ON public.product_action_receipt_idempotency_aliases (receipt_id)
WHERE endpoint_domain = 'product_reject_v1';

CREATE OR REPLACE FUNCTION public.enforce_product_activation_executor()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
STRICT
SECURITY DEFINER
PARALLEL UNSAFE
SET search_path = pg_catalog
AS $function$
BEGIN
    IF OLD.authority_kind = 'product_authoring'
        AND OLD.state = 'rejected'
    THEN
        RAISE EXCEPTION 'rejected product activation is immutable'
            USING ERRCODE = '23514';
    END IF;

    IF NEW.authority_kind = 'product_authoring'
        AND NEW.state = 'rejected'
        AND OLD.state <> 'rejected'
        AND (
            OLD.authority_kind <> 'product_authoring'
            OR OLD.state <> 'pending'
            OR pg_catalog.current_setting(
                'starring.product_rejection_gate',
                TRUE
            ) IS DISTINCT FROM OLD.approval_context_digest
            OR NEW.product_revision IS DISTINCT FROM OLD.product_revision + 1
            OR NEW.rejected_at IS NULL
            OR NEW.rejected_by IS NULL
            OR NEW.rejection_reason IS NULL
            OR (
                pg_catalog.to_jsonb(NEW)
                    - 'state'
                    - 'product_revision'
                    - 'rejected_at'
                    - 'rejected_by'
                    - 'rejection_reason'
            ) IS DISTINCT FROM (
                pg_catalog.to_jsonb(OLD)
                    - 'state'
                    - 'product_revision'
                    - 'rejected_at'
                    - 'rejected_by'
                    - 'rejection_reason'
            )
        )
    THEN
        RAISE EXCEPTION 'product rejection transition is not authorized'
            USING ERRCODE = '23514';
    END IF;

    IF NEW.authority_kind = 'product_authoring'
        AND NEW.state = 'applying'
        AND (
            OLD.state <> 'applying'
            OR OLD.apply_attempt_id IS DISTINCT FROM NEW.apply_attempt_id
        )
        AND pg_catalog.current_setting(
            'starring.product_approval_context_digest',
            TRUE
        ) IS DISTINCT FROM NEW.approval_context_digest
    THEN
        RAISE EXCEPTION 'product activation executor is not bound to the approval context'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE OR REPLACE FUNCTION public.enforce_product_action_receipt_retention()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
STRICT
SECURITY DEFINER
PARALLEL UNSAFE
SET search_path = pg_catalog
AS $function$
DECLARE
    expected_action TEXT;
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

    expected_action := CASE OLD.endpoint_domain
        WHEN 'product_approve_v1' THEN 'promotion.approve'
        WHEN 'product_apply_v1' THEN 'promotion.apply'
        WHEN 'product_promote_v1' THEN 'promotion.promote'
        WHEN 'product_reject_v1' THEN 'promotion.reject'
        ELSE NULL
    END;
    IF expected_action IS NULL
        OR EXISTS (
            SELECT 1
            FROM public.product_action_receipt_idempotency_aliases AS alias
            WHERE alias.tenant_id = OLD.tenant_id
                AND alias.installation_id = OLD.installation_id
                AND alias.principal_id = OLD.principal_id
                AND alias.endpoint_domain = OLD.endpoint_domain
                AND alias.receipt_id = OLD.receipt_id
        )
        OR (
            OLD.endpoint_domain = 'product_promote_v1'
            AND EXISTS (
                SELECT 1
                FROM public.authoring_promotions AS promotion
                WHERE promotion.tenant_id = OLD.tenant_id
                    AND promotion.installation_id = OLD.installation_id
                    AND promotion.id = OLD.target_resource_id
                    AND promotion.product_admission IS NOT NULL
                    AND promotion.stage IN ('prepared', 'published')
            )
        )
        OR NOT EXISTS (
            SELECT 1
            FROM public.product_action_receipt_audit_evidence AS evidence
            WHERE evidence.receipt_id = OLD.receipt_id
                AND evidence.tenant_id = OLD.tenant_id
                AND evidence.installation_id = OLD.installation_id
                AND evidence.principal_id = OLD.principal_id
                AND evidence.endpoint_domain = OLD.endpoint_domain
                AND evidence.action = expected_action
                AND evidence.request_digest = OLD.request_digest
                AND evidence.target_resource_type = OLD.target_resource_type
                AND evidence.target_resource_id = OLD.target_resource_id
                AND evidence.resulting_revision IS NOT DISTINCT FROM OLD.resulting_revision
                AND evidence.resulting_state = OLD.resulting_state
                AND evidence.result_code = OLD.result_code
                AND evidence.http_disposition_class = OLD.http_disposition_class
                AND evidence.completed_at = OLD.completed_at
                AND evidence.replay_policy_version = 1
                AND evidence.replay_guaranteed_until <= pg_catalog.clock_timestamp()
        )
    THEN
        RAISE EXCEPTION 'product action receipt is not retention eligible'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_action_receipt_retention_eligible';
    END IF;
    RETURN OLD;
END;
$function$;

CREATE OR REPLACE FUNCTION public.enforce_product_action_receipt_alias_retention()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
STRICT
SECURITY DEFINER
PARALLEL UNSAFE
SET search_path = pg_catalog
AS $function$
DECLARE
    expected_action TEXT;
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

    expected_action := CASE OLD.endpoint_domain
        WHEN 'product_approve_v1' THEN 'promotion.approve'
        WHEN 'product_apply_v1' THEN 'promotion.apply'
        WHEN 'product_promote_v1' THEN 'promotion.promote'
        WHEN 'product_reject_v1' THEN 'promotion.reject'
        ELSE NULL
    END;
    IF expected_action IS NULL
        OR (
            OLD.endpoint_domain = 'product_promote_v1'
            AND EXISTS (
                SELECT 1
                FROM public.product_action_receipts AS receipt
                INNER JOIN public.authoring_promotions AS promotion
                    ON promotion.tenant_id = receipt.tenant_id
                    AND promotion.installation_id = receipt.installation_id
                    AND promotion.id = receipt.target_resource_id
                WHERE receipt.tenant_id = OLD.tenant_id
                    AND receipt.installation_id = OLD.installation_id
                    AND receipt.principal_id = OLD.principal_id
                    AND receipt.endpoint_domain = OLD.endpoint_domain
                    AND receipt.receipt_id = OLD.receipt_id
                    AND promotion.product_admission IS NOT NULL
                    AND promotion.stage IN ('prepared', 'published')
            )
        )
        OR NOT EXISTS (
            SELECT 1
            FROM public.product_action_receipts AS receipt
            INNER JOIN public.product_action_receipt_audit_evidence AS evidence
                ON evidence.receipt_id = receipt.receipt_id
                AND evidence.tenant_id = receipt.tenant_id
                AND evidence.installation_id = receipt.installation_id
                AND evidence.principal_id = receipt.principal_id
                AND evidence.endpoint_domain = receipt.endpoint_domain
                AND evidence.action = expected_action
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
                AND evidence.replay_guaranteed_until <= pg_catalog.clock_timestamp()
        )
    THEN
        RAISE EXCEPTION 'product action receipt alias is not retention eligible'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_action_receipt_alias_retention_eligible';
    END IF;
    RETURN OLD;
END;
$function$;

CREATE OR REPLACE FUNCTION public.assert_product_approval_receipt_alias()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
STRICT
SECURITY DEFINER
PARALLEL UNSAFE
SET search_path = pg_catalog
AS $function$
BEGIN
    IF NEW.endpoint_domain IN (
        'product_approve_v1',
        'product_apply_v1',
        'product_promote_v1',
        'product_reject_v1'
    ) AND NOT EXISTS (
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
    ) THEN
        RAISE EXCEPTION 'product approval receipt is missing its primary idempotency alias'
            USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END;
$function$;

CREATE OR REPLACE FUNCTION public.assert_product_approval_receipt_audit()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
STRICT
SECURITY DEFINER
PARALLEL UNSAFE
SET search_path = pg_catalog
AS $function$
DECLARE
    expected_action TEXT;
BEGIN
    expected_action := CASE NEW.endpoint_domain
        WHEN 'product_approve_v1' THEN 'promotion.approve'
        WHEN 'product_apply_v1' THEN 'promotion.apply'
        WHEN 'product_promote_v1' THEN 'promotion.promote'
        WHEN 'product_reject_v1' THEN 'promotion.reject'
        ELSE NULL
    END;
    IF expected_action IS NOT NULL
        AND NOT EXISTS (
            SELECT 1
            FROM public.product_audit_events AS audit
            WHERE audit.tenant_id = NEW.tenant_id
                AND audit.installation_id = NEW.installation_id
                AND audit.principal_id = NEW.principal_id
                AND audit.receipt_id = NEW.receipt_id
                AND audit.action = expected_action
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

CREATE OR REPLACE FUNCTION public.capture_product_action_receipt_audit_evidence()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
STRICT
SECURITY DEFINER
PARALLEL UNSAFE
SET search_path = pg_catalog
AS $function$
DECLARE
    receipt_row public.product_action_receipts%ROWTYPE;
    expected_action TEXT;
BEGIN
    SELECT receipt.*
    INTO receipt_row
    FROM public.product_action_receipts AS receipt
    WHERE receipt.tenant_id = NEW.tenant_id
        AND receipt.installation_id = NEW.installation_id
        AND receipt.principal_id = NEW.principal_id
        AND receipt.receipt_id = NEW.receipt_id
    FOR SHARE;

    expected_action := CASE receipt_row.endpoint_domain
        WHEN 'product_approve_v1' THEN 'promotion.approve'
        WHEN 'product_apply_v1' THEN 'promotion.apply'
        WHEN 'product_promote_v1' THEN 'promotion.promote'
        WHEN 'product_reject_v1' THEN 'promotion.reject'
        ELSE NEW.action
    END;
    IF receipt_row.receipt_id IS NULL
        OR receipt_row.target_resource_type IS DISTINCT FROM NEW.target_resource_type
        OR receipt_row.target_resource_id IS DISTINCT FROM NEW.target_resource_id
        OR receipt_row.resulting_state IS DISTINCT FROM NEW.resulting_state
        OR receipt_row.result_code IS DISTINCT FROM NEW.result_code
        OR NEW.action IS DISTINCT FROM expected_action
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

CREATE OR REPLACE FUNCTION public.starring_purge_product_action_receipts_v1(
    batch_limit INTEGER
)
RETURNS TABLE(
    deleted_receipts INTEGER,
    deleted_aliases INTEGER,
    backlog_remaining BOOLEAN
)
LANGUAGE plpgsql
VOLATILE
CALLED ON NULL INPUT
SECURITY DEFINER
PARALLEL UNSAFE
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
        WHERE receipt.endpoint_domain IN (
                'product_approve_v1',
                'product_apply_v1',
                'product_promote_v1',
                'product_reject_v1'
            )
            AND receipt.completed_at <= retention_clock - INTERVAL '168 hours'
            AND (
                receipt.endpoint_domain <> 'product_promote_v1'
                OR NOT EXISTS (
                    SELECT 1
                    FROM public.authoring_promotions AS promotion
                    WHERE promotion.tenant_id = receipt.tenant_id
                        AND promotion.installation_id = receipt.installation_id
                        AND promotion.id = receipt.target_resource_id
                        AND promotion.product_admission IS NOT NULL
                        AND promotion.stage IN ('prepared', 'published')
                )
            )
        ORDER BY receipt.completed_at, receipt.receipt_id
        FOR UPDATE OF receipt SKIP LOCKED
        LIMIT batch_limit
    ) AS candidate;

    IF EXISTS (
        SELECT 1
        FROM public.product_action_receipts AS receipt
        WHERE receipt.receipt_id = ANY(candidate_receipt_ids)
            AND (
                receipt.endpoint_domain NOT IN (
                    'product_approve_v1',
                    'product_apply_v1',
                    'product_promote_v1',
                    'product_reject_v1'
                )
                OR NOT EXISTS (
                    SELECT 1
                    FROM public.product_action_receipt_audit_evidence AS evidence
                    WHERE evidence.receipt_id = receipt.receipt_id
                        AND evidence.tenant_id = receipt.tenant_id
                        AND evidence.installation_id = receipt.installation_id
                        AND evidence.principal_id = receipt.principal_id
                        AND evidence.endpoint_domain = receipt.endpoint_domain
                        AND evidence.action = CASE receipt.endpoint_domain
                            WHEN 'product_approve_v1' THEN 'promotion.approve'
                            WHEN 'product_apply_v1' THEN 'promotion.apply'
                            WHEN 'product_promote_v1' THEN 'promotion.promote'
                            WHEN 'product_reject_v1' THEN 'promotion.reject'
                        END
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
        WHERE alias.endpoint_domain IN (
                'product_approve_v1',
                'product_apply_v1',
                'product_promote_v1',
                'product_reject_v1'
            )
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
    WHERE alias.endpoint_domain IN (
            'product_approve_v1',
            'product_apply_v1',
            'product_promote_v1',
            'product_reject_v1'
        )
        AND alias.receipt_id = ANY(candidate_receipt_ids);
    GET DIAGNOSTICS alias_count = ROW_COUNT;

    DELETE FROM public.product_action_receipts AS receipt
    WHERE receipt.receipt_id = ANY(candidate_receipt_ids)
        AND receipt.endpoint_domain IN (
            'product_approve_v1',
            'product_apply_v1',
            'product_promote_v1',
            'product_reject_v1'
        );
    GET DIAGNOSTICS receipt_count = ROW_COUNT;

    IF receipt_count IS DISTINCT FROM pg_catalog.cardinality(candidate_receipt_ids) THEN
        RAISE EXCEPTION 'product action receipt purge did not delete its locked batch'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_action_receipt_purge_batch_complete';
    END IF;

    SELECT EXISTS (
        SELECT 1
        FROM public.product_action_receipts AS receipt
        WHERE receipt.endpoint_domain IN (
                'product_approve_v1',
                'product_apply_v1',
                'product_promote_v1',
                'product_reject_v1'
            )
            AND receipt.completed_at <= retention_clock - INTERVAL '168 hours'
            AND (
                receipt.endpoint_domain <> 'product_promote_v1'
                OR NOT EXISTS (
                    SELECT 1
                    FROM public.authoring_promotions AS promotion
                    WHERE promotion.tenant_id = receipt.tenant_id
                        AND promotion.installation_id = receipt.installation_id
                        AND promotion.id = receipt.target_resource_id
                        AND promotion.product_admission IS NOT NULL
                        AND promotion.stage IN ('prepared', 'published')
                )
            )
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

CREATE FUNCTION public.starring_product_rejection_executor_database_identity_v1()
RETURNS TEXT
LANGUAGE sql
VOLATILE
STRICT
SECURITY DEFINER
PARALLEL UNSAFE
SET search_path = pg_catalog
AS $function$
    SELECT identity.database_identity::TEXT
    FROM public.product_control_plane_identity AS identity
    WHERE identity.singleton;
$function$;

CREATE FUNCTION public.starring_product_rejection_keyring_coverage_v1(
    idempotency_digest_key_id_candidates TEXT[],
    idempotency_digest_key_fingerprint_candidates TEXT[]
)
RETURNS TABLE(outcome TEXT)
LANGUAGE plpgsql
VOLATILE
STRICT
SECURITY DEFINER
PARALLEL UNSAFE
ROWS 1
SET search_path = pg_catalog
AS $function$
BEGIN
    RETURN QUERY SELECT CASE
        WHEN pg_catalog.array_ndims(idempotency_digest_key_id_candidates)
                IS DISTINCT FROM 1
            OR pg_catalog.array_lower(idempotency_digest_key_id_candidates, 1)
                IS DISTINCT FROM 1
            OR pg_catalog.cardinality(idempotency_digest_key_id_candidates)
                NOT BETWEEN 1 AND 8
            OR pg_catalog.array_ndims(
                idempotency_digest_key_fingerprint_candidates
            ) IS DISTINCT FROM 1
            OR pg_catalog.array_lower(
                idempotency_digest_key_fingerprint_candidates,
                1
            ) IS DISTINCT FROM 1
            OR pg_catalog.cardinality(
                idempotency_digest_key_fingerprint_candidates
            ) IS DISTINCT FROM pg_catalog.cardinality(
                idempotency_digest_key_id_candidates
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.generate_subscripts(
                    idempotency_digest_key_id_candidates,
                    1
                ) AS candidate(ordinal)
                WHERE idempotency_digest_key_id_candidates[candidate.ordinal]
                        !~ '^[A-Za-z0-9_.:-]{1,64}$'
                    OR idempotency_digest_key_fingerprint_candidates[
                        candidate.ordinal
                    ] !~ '^[0-9a-f]{64}$'
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.generate_subscripts(
                    idempotency_digest_key_id_candidates,
                    1
                ) AS left_candidate(ordinal)
                INNER JOIN pg_catalog.generate_subscripts(
                    idempotency_digest_key_id_candidates,
                    1
                ) AS right_candidate(ordinal)
                    ON left_candidate.ordinal < right_candidate.ordinal
                WHERE idempotency_digest_key_id_candidates[left_candidate.ordinal]
                        = idempotency_digest_key_id_candidates[right_candidate.ordinal]
                    OR idempotency_digest_key_fingerprint_candidates[
                        left_candidate.ordinal
                    ] = idempotency_digest_key_fingerprint_candidates[
                        right_candidate.ordinal
                    ]
            )
        THEN 'invalid_input'
        WHEN EXISTS (
            SELECT 1
            FROM public.product_action_receipts AS receipt
            WHERE receipt.endpoint_domain = 'product_reject_v1'
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
                            WHERE idempotency_digest_key_id_candidates[
                                    candidate.ordinal
                                ] = alias.idempotency_digest_key_id
                                AND idempotency_digest_key_fingerprint_candidates[
                                    candidate.ordinal
                                ] = alias.idempotency_digest_key_fingerprint
                        )
                )
        )
        THEN 'idempotency_keyring_incomplete'
        ELSE 'ok'
    END;
END;
$function$;

CREATE FUNCTION public.starring_product_reject_v1(
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
    new_audit_event_id TEXT,
    expected_rejection_reason TEXT
)
RETURNS TABLE(
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
PARALLEL UNSAFE
ROWS 1
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
    next_revision BIGINT;
    active_baseline_version BIGINT;
    active_baseline_hash TEXT;
    candidate_lock_digest TEXT;
BEGIN
    IF pg_catalog.current_setting('transaction_isolation') <> 'serializable'
        OR pg_catalog.current_setting('transaction_read_only') <> 'off'
        OR expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
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
        OR expected_capability <> 'reject'
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
        OR expected_rejection_reason
            IS DISTINCT FROM pg_catalog.btrim(expected_rejection_reason)
        OR pg_catalog.char_length(expected_rejection_reason) NOT BETWEEN 1 AND 1000
        OR pg_catalog.octet_length(expected_rejection_reason) > 4000
        OR expected_rejection_reason ~ U&'[\0001-\001F\007F-\009F]'
        OR pg_catalog.left(expected_rejection_reason, 1) = ANY(ARRAY[
            pg_catalog.chr(9),
            pg_catalog.chr(10),
            pg_catalog.chr(11),
            pg_catalog.chr(12),
            pg_catalog.chr(13),
            pg_catalog.chr(32),
            pg_catalog.chr(133),
            pg_catalog.chr(160),
            pg_catalog.chr(5760),
            pg_catalog.chr(8192),
            pg_catalog.chr(8193),
            pg_catalog.chr(8194),
            pg_catalog.chr(8195),
            pg_catalog.chr(8196),
            pg_catalog.chr(8197),
            pg_catalog.chr(8198),
            pg_catalog.chr(8199),
            pg_catalog.chr(8200),
            pg_catalog.chr(8201),
            pg_catalog.chr(8202),
            pg_catalog.chr(8232),
            pg_catalog.chr(8233),
            pg_catalog.chr(8239),
            pg_catalog.chr(8287),
            pg_catalog.chr(12288)
        ])
        OR pg_catalog.right(expected_rejection_reason, 1) = ANY(ARRAY[
            pg_catalog.chr(9),
            pg_catalog.chr(10),
            pg_catalog.chr(11),
            pg_catalog.chr(12),
            pg_catalog.chr(13),
            pg_catalog.chr(32),
            pg_catalog.chr(133),
            pg_catalog.chr(160),
            pg_catalog.chr(5760),
            pg_catalog.chr(8192),
            pg_catalog.chr(8193),
            pg_catalog.chr(8194),
            pg_catalog.chr(8195),
            pg_catalog.chr(8196),
            pg_catalog.chr(8197),
            pg_catalog.chr(8198),
            pg_catalog.chr(8199),
            pg_catalog.chr(8200),
            pg_catalog.chr(8201),
            pg_catalog.chr(8202),
            pg_catalog.chr(8232),
            pg_catalog.chr(8233),
            pg_catalog.chr(8239),
            pg_catalog.chr(8287),
            pg_catalog.chr(12288)
        ])
    THEN
        RETURN QUERY SELECT 'invalid_input', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
        RETURN;
    END IF;

    IF expected_product_revision = 9223372036854775807 THEN
        RETURN QUERY SELECT 'invalid_state', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
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
                || expected_principal_id || ':product_reject_v1:key-coverage',
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
                    || expected_principal_id || ':product_reject_v1:'
                    || candidate_lock_digest,
                0
            )
        );
    END LOOP;

    SELECT activation.*
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

    SELECT promotion.*
    INTO promotion_row
    FROM public.authoring_promotions AS promotion
    WHERE promotion.id = expected_promotion_id
    FOR SHARE;
    IF NOT FOUND THEN
        RETURN QUERY SELECT 'not_found', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
        RETURN;
    END IF;

    SELECT tenant.*
    INTO tenant_row
    FROM public.product_tenants AS tenant
    WHERE tenant.tenant_id = expected_tenant_id
    FOR SHARE;

    SELECT installation.*
    INTO installation_row
    FROM public.automation_installations AS installation
    WHERE installation.tenant_id = expected_tenant_id
        AND installation.installation_id = expected_installation_id
    FOR SHARE;

    SELECT authority.*
    INTO authority_row
    FROM public.automation_installation_authority_versions AS authority
    WHERE authority.tenant_id = expected_tenant_id
        AND authority.installation_id = expected_installation_id
        AND authority.revision = expected_authority_revision
    FOR SHARE;

    SELECT principal.*
    INTO principal_row
    FROM public.product_principals AS principal
    WHERE principal.principal_id = expected_principal_id
    FOR SHARE;

    SELECT product_session.*
    INTO session_row
    FROM public.product_auth_sessions AS product_session
    WHERE product_session.session_digest = expected_product_session_digest
        AND product_session.principal_id = expected_principal_id
    FOR SHARE;

    mutation_clock := pg_catalog.clock_timestamp();

    IF tenant_row.tenant_id IS NULL
        OR installation_row.installation_id IS NULL
        OR authority_row.installation_id IS NULL
        OR principal_row.principal_id IS NULL
        OR session_row.principal_id IS NULL
        OR tenant_row.lifecycle_state <> 'active'
        OR installation_row.lifecycle_state <> 'active'
        OR installation_row.current_authority_revision
            IS DISTINCT FROM expected_authority_revision
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

    IF authority_row.authority_payload_digest
        IS DISTINCT FROM expected_authority_payload_digest
    THEN
        RETURN QUERY SELECT 'authority_mismatch', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
        RETURN;
    END IF;

    IF installation_row.discord_application_id IS DISTINCT FROM expected_discord_application_id
        OR installation_row.discord_guild_id IS DISTINCT FROM expected_guild_id
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
        OR authority_row.binding_fingerprint
            IS DISTINCT FROM promotion_row.record
                #>> '{intent,evidence,context_fingerprint}'
        OR authority_row.binding_revision::TEXT
            IS DISTINCT FROM promotion_row.record
                #>> '{intent,authority,binding_revision}'
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
        OR activation_row.approval_context -> 'context'
            IS DISTINCT FROM promotion_row.record
                #> '{stage,activation,approval_context}'
    THEN
        RETURN QUERY SELECT 'authority_mismatch', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
        RETURN;
    END IF;

    IF activation_row.approval_payload_digest IS DISTINCT FROM expected_payload_digest THEN
        RETURN QUERY SELECT 'payload_mismatch', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
        RETURN;
    END IF;

    IF EXISTS (
        SELECT 1
        FROM public.product_action_receipts AS receipt
        WHERE receipt.tenant_id = expected_tenant_id
            AND receipt.installation_id = expected_installation_id
            AND receipt.principal_id = expected_principal_id
            AND receipt.endpoint_domain = 'product_reject_v1'
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
        AND alias.endpoint_domain = 'product_reject_v1'
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
                AND alias.endpoint_domain = 'product_reject_v1'
                AND alias.idempotency_key_digest = ANY(idempotency_key_digest_candidates)
            ORDER BY alias.receipt_id
            LIMIT 1
        ) AS matched ON matched.receipt_id = receipt.receipt_id
        WHERE receipt.tenant_id = expected_tenant_id
            AND receipt.installation_id = expected_installation_id
            AND receipt.principal_id = expected_principal_id
            AND receipt.endpoint_domain = 'product_reject_v1'
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
            OR receipt_row.resulting_revision IS DISTINCT FROM expected_product_revision + 1
            OR receipt_row.resulting_state <> 'rejected'
            OR receipt_row.result_code <> 'promotion_rejected'
            OR receipt_row.http_disposition_class <> 2
            OR activation_row.state <> 'rejected'
            OR activation_row.product_revision
                IS DISTINCT FROM receipt_row.resulting_revision
            OR activation_row.rejected_by IS DISTINCT FROM expected_acting_user_id
            OR activation_row.rejection_reason IS DISTINCT FROM expected_rejection_reason
            OR activation_row.rejected_at IS DISTINCT FROM receipt_row.completed_at
            OR NOT EXISTS (
                SELECT 1
                FROM public.product_audit_events AS audit
                INNER JOIN public.product_action_receipt_audit_evidence AS evidence
                    ON evidence.receipt_id = audit.receipt_id
                    AND evidence.event_id = audit.event_id
                    AND evidence.tenant_id = audit.tenant_id
                    AND evidence.installation_id = audit.installation_id
                    AND evidence.principal_id = audit.principal_id
                INNER JOIN public.automation_installation_authority_versions
                    AS historical_authority
                    ON historical_authority.tenant_id = audit.tenant_id
                    AND historical_authority.installation_id = audit.installation_id
                    AND historical_authority.revision
                        = audit.installation_authority_revision
                WHERE audit.receipt_id = receipt_row.receipt_id
                    AND audit.tenant_id = receipt_row.tenant_id
                    AND audit.installation_id = receipt_row.installation_id
                    AND audit.principal_id = receipt_row.principal_id
                    AND audit.action = 'promotion.reject'
                    AND audit.target_resource_type = receipt_row.target_resource_type
                    AND audit.target_resource_id = receipt_row.target_resource_id
                    AND audit.resulting_state = receipt_row.resulting_state
                    AND audit.result_code = receipt_row.result_code
                    AND audit.payload_digest = expected_payload_digest
                    AND audit.binding_fingerprint = historical_authority.binding_fingerprint
                    AND audit.policy_revision = historical_authority.policy_revision
                    AND historical_authority.binding_revision::TEXT
                        = activation_row.approval_context
                            #>> '{context,binding,revision}'
                    AND historical_authority.binding_revision::TEXT
                        = promotion_row.record
                            #>> '{intent,authority,binding_revision}'
                    AND historical_authority.binding_fingerprint
                        = promotion_row.record
                            #>> '{intent,evidence,context_fingerprint}'
                    AND historical_authority.policy_revision::TEXT
                        = activation_row.approval_context
                            #>> '{context,policy,revision}'
                    AND historical_authority.required_approvals::TEXT
                        = activation_row.approval_context
                            #>> '{context,policy,required_approvals}'
                    AND historical_authority.activation_ttl_seconds::TEXT
                        = activation_row.approval_context
                            #>> '{context,policy,ttl_seconds}'
                    AND activation_row.required_approvals
                        = historical_authority.required_approvals
                    AND activation_row.approval_context -> 'context'
                        = promotion_row.record
                            #> '{stage,activation,approval_context}'
                    AND evidence.endpoint_domain = receipt_row.endpoint_domain
                    AND evidence.action = audit.action
                    AND evidence.request_digest = receipt_row.request_digest
                    AND evidence.target_resource_type = receipt_row.target_resource_type
                    AND evidence.target_resource_id = receipt_row.target_resource_id
                    AND evidence.resulting_revision
                        IS NOT DISTINCT FROM receipt_row.resulting_revision
                    AND evidence.resulting_state = receipt_row.resulting_state
                    AND evidence.result_code = receipt_row.result_code
                    AND evidence.http_disposition_class
                        = receipt_row.http_disposition_class
                    AND evidence.completed_at = receipt_row.completed_at
                    AND evidence.evidence_version = 1
                    AND evidence.replay_policy_version = 1
            )
        THEN
            RETURN QUERY SELECT 'indeterminate', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
            RETURN;
        END IF;

        mutation_clock := pg_catalog.clock_timestamp();
        IF mutation_clock >= session_row.idle_expires_at
            OR mutation_clock >= session_row.absolute_expires_at
            OR expected_authority_observed_at > mutation_clock
            OR mutation_clock >= expected_authority_expires_at
        THEN
            RETURN QUERY SELECT 'authorization_stale', NULL::BIGINT, NULL::TEXT, FALSE,
                NULL::TEXT;
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

    mutation_clock := pg_catalog.clock_timestamp();
    IF mutation_clock >= session_row.idle_expires_at
        OR mutation_clock >= session_row.absolute_expires_at
        OR expected_authority_observed_at > mutation_clock
        OR mutation_clock >= expected_authority_expires_at
    THEN
        RETURN QUERY SELECT 'authorization_stale', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
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

    IF activation_row.expires_at <= mutation_clock THEN
        RETURN QUERY SELECT 'expired', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
        RETURN;
    END IF;
    IF activation_row.state <> 'pending' THEN
        RETURN QUERY SELECT 'invalid_state', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;
        RETURN;
    END IF;

    next_revision := activation_row.product_revision + 1;

    PERFORM pg_catalog.set_config(
        'starring.product_rejection_gate',
        activation_row.approval_context_digest,
        TRUE
    );

    UPDATE public.activation_requests AS activation
    SET state = 'rejected',
        product_revision = next_revision,
        rejected_at = mutation_clock,
        rejected_by = expected_acting_user_id,
        rejection_reason = expected_rejection_reason
    WHERE activation.id = activation_row.id
        AND activation.state = 'pending'
        AND activation.product_revision = expected_product_revision;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'product rejection activation compare-and-swap failed'
            USING ERRCODE = '40001';
    END IF;

    PERFORM pg_catalog.set_config('starring.product_rejection_gate', '', TRUE);

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
        'product_reject_v1',
        active_idempotency_key_digest,
        idempotency_digest_key_id,
        idempotency_digest_key_fingerprint_candidates[1],
        semantic_request_digest,
        'authoring_promotion',
        expected_promotion_id,
        next_revision,
        'rejected',
        'promotion_rejected',
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
        'product_reject_v1',
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
        'promotion.reject',
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
        'rejected',
        'promotion_rejected',
        '{}'::JSONB,
        mutation_clock
    );

    RETURN QUERY SELECT 'ok', next_revision, 'rejected', FALSE, activation_row.guild_id;
END;
$function$;

REVOKE ALL ON FUNCTION public.starring_product_rejection_executor_database_identity_v1()
FROM PUBLIC;

REVOKE ALL ON FUNCTION public.starring_product_rejection_keyring_coverage_v1(
    TEXT[],
    TEXT[]
) FROM PUBLIC;

REVOKE ALL ON FUNCTION public.starring_product_reject_v1(
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
    TEXT,
    TEXT
) FROM PUBLIC;

DO $postflight$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    grantee OID;
    grantee_name NAME;
    default_grantee_clause TEXT;
    default_schema_name NAME;
    routine_identity TEXT;
    unexpected_routine_identity TEXT;
    user_schema_name NAME;
    function_identity_count BIGINT;
    invalid_function_count BIGINT;
    invalid_constraint_count BIGINT;
    invalid_index_count BIGINT;
    coverage_outcome TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.activation_requests');

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner IS NULL
        OR common_owner_name IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
    THEN
        RAISE EXCEPTION 'product rejection owner changed during migration'
            USING ERRCODE = '55000';
    END IF;

    EXECUTE pg_catalog.format(
        'ALTER DEFAULT PRIVILEGES FOR ROLE %I REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC',
        common_owner_name
    );

    FOR user_schema_name IN
        SELECT namespace.nspname
        FROM pg_catalog.pg_namespace AS namespace
        WHERE namespace.nspname <> 'information_schema'
            AND pg_catalog.left(namespace.nspname, 3) <> 'pg_'
        ORDER BY namespace.nspname
    LOOP
        EXECUTE pg_catalog.format(
            'ALTER DEFAULT PRIVILEGES FOR ROLE %I IN SCHEMA %I REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC',
            common_owner_name,
            user_schema_name
        );
    END LOOP;

    FOR default_schema_name, grantee IN
        SELECT namespace.nspname, privilege.grantee
        FROM pg_catalog.pg_default_acl AS default_acl
        CROSS JOIN LATERAL pg_catalog.aclexplode(default_acl.defaclacl) AS privilege
        LEFT JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = default_acl.defaclnamespace
        WHERE default_acl.defaclrole = common_owner
            AND default_acl.defaclobjtype = 'f'
            AND privilege.grantee <> common_owner
            AND (
                default_acl.defaclnamespace = 0
                OR (
                    namespace.nspname <> 'information_schema'
                    AND pg_catalog.left(namespace.nspname, 3) <> 'pg_'
                )
            )
        ORDER BY namespace.nspname NULLS FIRST, privilege.grantee
    LOOP
        default_grantee_clause := CASE
            WHEN grantee = 0 THEN 'PUBLIC'
            ELSE pg_catalog.quote_ident(pg_catalog.pg_get_userbyid(grantee))
        END;
        IF default_grantee_clause IS NULL THEN
            RAISE EXCEPTION 'product rejection default grantee is unavailable'
                USING ERRCODE = '55000';
        END IF;
        IF default_schema_name IS NULL THEN
            EXECUTE pg_catalog.format(
                'ALTER DEFAULT PRIVILEGES FOR ROLE %I REVOKE ALL PRIVILEGES ON FUNCTIONS FROM %s',
                common_owner_name,
                default_grantee_clause
            );
        ELSE
            EXECUTE pg_catalog.format(
                'ALTER DEFAULT PRIVILEGES FOR ROLE %I IN SCHEMA %I REVOKE ALL PRIVILEGES ON FUNCTIONS FROM %s',
                common_owner_name,
                default_schema_name,
                default_grantee_clause
            );
        END IF;
    END LOOP;

    FOR routine_identity IN
        SELECT pg_catalog.format(
            '%I.%I(%s)',
            namespace.nspname,
            function_row.proname,
            pg_catalog.pg_get_function_identity_arguments(function_row.oid)
        )
        FROM pg_catalog.pg_proc AS function_row
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = function_row.pronamespace
        WHERE namespace.nspname <> 'information_schema'
            AND pg_catalog.left(namespace.nspname, 3) <> 'pg_'
            AND function_row.prokind IN ('f', 'p')
        ORDER BY namespace.nspname, function_row.proname, function_row.oid
    LOOP
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON ROUTINE %s FROM PUBLIC CASCADE',
            routine_identity
        );
    END LOOP;

    FOR grantee IN
        SELECT DISTINCT privilege.grantee
        FROM pg_catalog.pg_proc AS function_row
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = function_row.pronamespace
        CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
            function_row.proacl,
            pg_catalog.acldefault('f', function_row.proowner)
        )) AS privilege
        WHERE namespace.nspname = 'public'
            AND function_row.proname IN (
                'starring_product_rejection_executor_database_identity_v1',
                'starring_product_rejection_keyring_coverage_v1',
                'starring_product_reject_v1'
            )
            AND privilege.grantee <> 0
            AND privilege.grantee <> common_owner
    LOOP
        grantee_name := pg_catalog.pg_get_userbyid(grantee);
        IF grantee_name IS NULL THEN
            RAISE EXCEPTION 'product rejection grantee is unavailable'
                USING ERRCODE = '55000';
        END IF;
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON FUNCTION public.starring_product_rejection_executor_database_identity_v1() FROM %I CASCADE',
            grantee_name
        );
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON FUNCTION public.starring_product_rejection_keyring_coverage_v1(text[],text[]) FROM %I CASCADE',
            grantee_name
        );
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON FUNCTION public.starring_product_reject_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text) FROM %I CASCADE',
            grantee_name
        );
    END LOOP;

    SELECT pg_catalog.min(pg_catalog.format(
        '%I.%I(%s)',
        namespace.nspname,
        function_row.proname,
        pg_catalog.pg_get_function_identity_arguments(function_row.oid)
    ))
    INTO unexpected_routine_identity
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE namespace.nspname <> 'information_schema'
        AND pg_catalog.left(namespace.nspname, 3) <> 'pg_'
        AND function_row.prokind IN ('f', 'p')
        AND privilege.grantee = 0
        AND privilege.privilege_type = 'EXECUTE';

    IF unexpected_routine_identity IS NOT NULL THEN
        RAISE EXCEPTION 'user routine public execution is not sealed: %',
            unexpected_routine_identity
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM pg_catalog.pg_default_acl AS default_acl
        CROSS JOIN LATERAL pg_catalog.aclexplode(default_acl.defaclacl) AS privilege
        LEFT JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = default_acl.defaclnamespace
        WHERE default_acl.defaclrole = common_owner
            AND default_acl.defaclobjtype = 'f'
            AND (
                default_acl.defaclnamespace = 0
                OR (
                    namespace.nspname <> 'information_schema'
                    AND pg_catalog.left(namespace.nspname, 3) <> 'pg_'
                )
            )
            AND privilege.grantee <> common_owner
            AND privilege.privilege_type = 'EXECUTE'
    ) THEN
        RAISE EXCEPTION 'user routine execution defaults are not sealed'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO function_identity_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_product_rejection_executor_database_identity_v1',
            'starring_product_rejection_keyring_coverage_v1',
            'starring_product_reject_v1'
        );

    IF function_identity_count <> 3 THEN
        RAISE EXCEPTION 'product rejection function identity is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_product_rejection_executor_database_identity_v1()',
                '',
                'text',
                FALSE,
                0::REAL,
                'sql'
            ),
            (
                'public.starring_product_rejection_keyring_coverage_v1(text[],text[])',
                'idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[]',
                'TABLE(outcome text)',
                TRUE,
                1::REAL,
                'plpgsql'
            ),
            (
                'public.starring_product_reject_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text)',
                'expected_tenant_id text, expected_installation_id text, expected_promotion_id text, expected_product_revision bigint, expected_payload_digest text, expected_principal_id text, expected_product_session_digest bytea, session_subject_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, expected_authority_revision bigint, expected_authority_payload_digest text, expected_authority_observation_digest text, expected_authority_observed_at timestamp with time zone, expected_authority_expires_at timestamp with time zone, expected_effective_permission_bits text, expected_guild_owner boolean, product_request_id text, active_idempotency_key_digest text, idempotency_key_digest_candidates text[], idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[], idempotency_digest_key_id text, semantic_request_digest text, new_receipt_id text, new_audit_event_id text, expected_rejection_reason text',
                'TABLE(outcome text, resulting_revision bigint, resulting_state text, exact_replay boolean, guild_id text)',
                TRUE,
                1::REAL,
                'plpgsql'
            )
    ) AS expected(
        signature,
        identity_arguments,
        result_identity,
        returns_set,
        rows_estimate,
        language_name
    )
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.signature)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR NOT function_row.proisstrict
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR function_row.proretset <> expected.returns_set
        OR function_row.prorows <> expected.rows_estimate
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM expected.language_name
        OR pg_catalog.pg_get_function_identity_arguments(function_row.oid)
            IS DISTINCT FROM expected.identity_arguments
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM expected.result_identity
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> function_row.proowner
        );

    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION 'product rejection function contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            ('public.enforce_product_activation_executor()', TRUE),
            ('public.assert_product_approval_receipt_alias()', TRUE),
            ('public.assert_product_approval_receipt_audit()', TRUE),
            ('public.capture_product_action_receipt_audit_evidence()', TRUE),
            ('public.enforce_product_action_receipt_retention()', TRUE),
            ('public.enforce_product_action_receipt_alias_retention()', TRUE),
            ('public.starring_purge_product_action_receipts_v1(integer)', FALSE)
    ) AS expected(signature, strict_input)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.signature)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR function_row.proisstrict <> expected.strict_input
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR function_row.proconfig IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM 'plpgsql'
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> function_row.proowner
        );

    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION 'product rejection support function contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    IF pg_catalog.strpos(
        pg_catalog.pg_get_functiondef(
            pg_catalog.to_regprocedure('public.assert_product_approval_receipt_alias()')
        ),
        'product_reject_v1'
    ) = 0
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure('public.assert_product_approval_receipt_audit()')
            ),
            'promotion.reject'
        ) = 0
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.capture_product_action_receipt_audit_evidence()'
                )
            ),
            'promotion.reject'
        ) = 0
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure('public.enforce_product_action_receipt_retention()')
            ),
            'promotion.reject'
        ) = 0
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.enforce_product_action_receipt_alias_retention()'
                )
            ),
            'promotion.reject'
        ) = 0
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_purge_product_action_receipts_v1(integer)'
                )
            ),
            'promotion.reject'
        ) = 0
    THEN
        RAISE EXCEPTION 'product rejection receipt lifecycle contract is incomplete'
            USING ERRCODE = '55000';
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_trigger AS trigger_row
        WHERE trigger_row.tgrelid = pg_catalog.to_regclass('public.activation_requests')
            AND trigger_row.tgname = 'activation_requests_enforce_product_executor'
            AND NOT trigger_row.tgisinternal
            AND trigger_row.tgenabled = 'O'
            AND trigger_row.tgfoid
                = pg_catalog.to_regprocedure('public.enforce_product_activation_executor()')
            AND pg_catalog.pg_get_triggerdef(trigger_row.oid, FALSE)
                = 'CREATE TRIGGER activation_requests_enforce_product_executor BEFORE UPDATE ON public.activation_requests FOR EACH ROW EXECUTE FUNCTION public.enforce_product_activation_executor()'
    ) OR pg_catalog.strpos(
        pg_catalog.pg_get_functiondef(
            pg_catalog.to_regprocedure('public.enforce_product_activation_executor()')
        ),
        'starring.product_rejection_gate'
    ) = 0
    THEN
        RAISE EXCEPTION 'product rejection transition gate is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_constraint_count
    FROM (
        VALUES
            ('public.activation_requests', 'activation_requests_rejected_fields_valid'),
            (
                'public.product_action_receipts',
                'product_action_receipts_approval_key_identity_required'
            )
    ) AS expected(relation_identity, constraint_name)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = pg_catalog.to_regclass(expected.relation_identity)
    LEFT JOIN pg_catalog.pg_constraint AS constraint_row
        ON constraint_row.conrelid = relation.oid
        AND constraint_row.conname = expected.constraint_name
    WHERE relation.oid IS NULL
        OR relation.relowner <> common_owner
        OR constraint_row.oid IS NULL
        OR constraint_row.contype <> 'c'
        OR NOT constraint_row.convalidated
        OR constraint_row.connoinherit
        OR constraint_row.condeferrable
        OR constraint_row.condeferred
        OR constraint_row.conparentid <> 0;

    IF invalid_constraint_count <> 0
        OR pg_catalog.pg_get_constraintdef((
            SELECT constraint_row.oid
            FROM pg_catalog.pg_constraint AS constraint_row
            WHERE constraint_row.conrelid
                = pg_catalog.to_regclass('public.product_action_receipts')
                AND constraint_row.conname
                    = 'product_action_receipts_approval_key_identity_required'
        ), FALSE) IS DISTINCT FROM
            'CHECK (((endpoint_domain <> ALL (ARRAY[''product_approve_v1''::text, ''product_apply_v1''::text, ''product_promote_v1''::text, ''product_reject_v1''::text])) OR ((idempotency_digest_key_id IS NOT NULL) AND (idempotency_digest_key_fingerprint IS NOT NULL))))'
        OR pg_catalog.strpos(
            pg_catalog.pg_get_constraintdef((
                SELECT constraint_row.oid
                FROM pg_catalog.pg_constraint AS constraint_row
                WHERE constraint_row.conrelid
                    = pg_catalog.to_regclass('public.activation_requests')
                    AND constraint_row.conname = 'activation_requests_rejected_fields_valid'
            ), FALSE),
            'char_length(rejection_reason) >= 1'
        ) = 0
        OR pg_catalog.strpos(
            pg_catalog.pg_get_constraintdef((
                SELECT constraint_row.oid
                FROM pg_catalog.pg_constraint AS constraint_row
                WHERE constraint_row.conrelid
                    = pg_catalog.to_regclass('public.activation_requests')
                    AND constraint_row.conname = 'activation_requests_rejected_fields_valid'
            ), FALSE),
            'octet_length(rejection_reason) <= 4000'
        ) = 0
    THEN
        RAISE EXCEPTION 'product rejection constraint contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_index_count
    FROM (
        VALUES
            (
                'public.product_action_receipts_rejection_retention_index',
                'public.product_action_receipts'
            ),
            (
                'public.product_action_aliases_rejection_receipt_retention_index',
                'public.product_action_receipt_idempotency_aliases'
            )
    ) AS expected(index_identity, relation_identity)
    LEFT JOIN pg_catalog.pg_class AS index_relation
        ON index_relation.oid = pg_catalog.to_regclass(expected.index_identity)
    LEFT JOIN pg_catalog.pg_index AS index_row
        ON index_row.indexrelid = index_relation.oid
    LEFT JOIN pg_catalog.pg_class AS table_relation
        ON table_relation.oid = pg_catalog.to_regclass(expected.relation_identity)
    WHERE index_relation.oid IS NULL
        OR index_relation.relkind <> 'i'
        OR index_relation.relowner <> common_owner
        OR index_row.indrelid IS DISTINCT FROM table_relation.oid
        OR NOT index_row.indisvalid
        OR NOT index_row.indisready
        OR NOT index_row.indislive
        OR index_row.indisunique
        OR index_row.indisexclusion
        OR pg_catalog.pg_get_expr(index_row.indpred, index_row.indrelid)
            IS DISTINCT FROM '(endpoint_domain = ''product_reject_v1''::text)';

    IF invalid_index_count <> 0 THEN
        RAISE EXCEPTION 'product rejection retention index contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    IF public.starring_product_rejection_executor_database_identity_v1()
        IS DISTINCT FROM public.starring_product_approval_executor_database_identity_v1()
    THEN
        RAISE EXCEPTION 'product rejection database identity is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT coverage.outcome
    INTO coverage_outcome
    FROM public.starring_product_rejection_keyring_coverage_v1(
        ARRAY[]::TEXT[],
        ARRAY[]::TEXT[]
    ) AS coverage;

    IF coverage_outcome IS DISTINCT FROM 'invalid_input' THEN
        RAISE EXCEPTION 'product rejection keyring coverage probe is invalid'
            USING ERRCODE = '55000';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
