LOCK TABLE public.activation_requests,
    public.runtime_deployments,
    public.automation_ruleset_activations,
    public.product_action_receipts,
    public.product_action_receipt_idempotency_aliases,
    public.product_audit_events,
    public.product_action_receipt_audit_evidence
IN SHARE ROW EXCLUSIVE MODE;

DO $function$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM public.activation_requests AS activation
        WHERE activation.authority_kind = 'product_authoring'
            AND activation.state = 'applied'
            AND NOT EXISTS (
                SELECT 1
                FROM public.runtime_deployments AS deployment
                INNER JOIN public.automation_installation_authority_versions AS authority
                    ON authority.tenant_id = deployment.tenant_id
                    AND authority.installation_id = deployment.installation_id
                    AND authority.revision = deployment.installation_authority_revision
                INNER JOIN public.authoring_promotions AS promotion
                    ON promotion.id = deployment.promotion_id
                    AND promotion.tenant_id = deployment.tenant_id
                    AND promotion.installation_id = deployment.installation_id
                WHERE deployment.tenant_id = activation.tenant_id
                    AND deployment.installation_id = activation.installation_id
                    AND deployment.promotion_id = activation.promotion_id
                    AND deployment.activation_request_id = activation.id
                    AND deployment.guild_id = activation.guild_id
                    AND deployment.ruleset_key = activation.ruleset_key
                    AND deployment.target_version = activation.target_version
                    AND deployment.target_content_hash = activation.target_content_hash
            )
    ) THEN
        RAISE EXCEPTION 'atomic product apply migration found an Applied activation without an exact deployment'
            USING ERRCODE = '23514',
                CONSTRAINT = 'atomic_product_apply_upgrade_deployment_complete';
    END IF;
END;
$function$;

ALTER TABLE public.runtime_deployments
ADD COLUMN policy_revision BIGINT,
ADD COLUMN desired_target_digest_version SMALLINT NOT NULL DEFAULT 1;

ALTER TABLE public.runtime_deployments
DISABLE TRIGGER runtime_deployments_validate_projection;

UPDATE public.runtime_deployments AS deployment
SET policy_revision = authority.policy_revision
FROM public.automation_installation_authority_versions AS authority
WHERE authority.tenant_id = deployment.tenant_id
    AND authority.installation_id = deployment.installation_id
    AND authority.revision = deployment.installation_authority_revision;

ALTER TABLE public.runtime_deployments
ENABLE TRIGGER runtime_deployments_validate_projection;

DO $function$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM public.runtime_deployments AS deployment
        WHERE deployment.policy_revision IS NULL
    ) THEN
        RAISE EXCEPTION 'runtime deployment policy shadow backfill is incomplete'
            USING ERRCODE = '23514',
                CONSTRAINT = 'runtime_deployments_policy_shadow_complete';
    END IF;
END;
$function$;

ALTER TABLE public.runtime_deployments
ALTER COLUMN policy_revision SET NOT NULL,
ADD CONSTRAINT runtime_deployments_policy_revision_valid CHECK (
    policy_revision BETWEEN 1 AND 9223372036854775807
),
ADD CONSTRAINT runtime_deployments_desired_digest_version_valid CHECK (
    desired_target_digest_version = 1
);

CREATE FUNCTION public.starring_runtime_desired_target_digest_v1(
    prepared_snapshot JSONB,
    installation_authority_revision BIGINT
)
RETURNS TEXT
LANGUAGE plpgsql
IMMUTABLE
STRICT
SET search_path = pg_catalog
AS $function$
DECLARE
    material BYTEA := pg_catalog.decode('', 'hex');
    field BYTEA;
    version_value NUMERIC;
    previous_runtime JSONB;
    previous_target JSONB;
BEGIN
    IF installation_authority_revision NOT BETWEEN 1 AND 9223372036854775807
        OR pg_catalog.jsonb_typeof(prepared_snapshot) IS DISTINCT FROM 'object'
    THEN
        RETURN NULL;
    END IF;
    IF (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(prepared_snapshot)
        ) <> 17
        OR NOT prepared_snapshot ?& ARRAY[
            'identity',
            'target',
            'runtime_generation',
            'previous_runtime',
            'requested_at',
            'revision',
            'phase',
            'controller_lease',
            'last_fencing_token',
            'preflight',
            'drain',
            'activation',
            'panel_certificate',
            'gateway_ready',
            'live',
            'last_live_recovery',
            'last_runtime_failure'
        ]
        OR pg_catalog.jsonb_typeof(prepared_snapshot -> 'identity')
            IS DISTINCT FROM 'object'
        OR pg_catalog.jsonb_typeof(prepared_snapshot -> 'target')
            IS DISTINCT FROM 'object'
        OR pg_catalog.jsonb_typeof(prepared_snapshot -> 'requested_at')
            IS DISTINCT FROM 'string'
        OR pg_catalog.jsonb_typeof(prepared_snapshot -> 'revision')
            IS DISTINCT FROM 'number'
        OR pg_catalog.jsonb_typeof(prepared_snapshot -> 'phase')
            IS DISTINCT FROM 'object'
        OR pg_catalog.jsonb_typeof(prepared_snapshot -> 'controller_lease')
            NOT IN ('null', 'object')
        OR pg_catalog.jsonb_typeof(prepared_snapshot -> 'last_fencing_token')
            NOT IN ('null', 'number')
        OR pg_catalog.jsonb_typeof(prepared_snapshot -> 'preflight')
            NOT IN ('null', 'object')
        OR pg_catalog.jsonb_typeof(prepared_snapshot -> 'drain')
            NOT IN ('null', 'object')
        OR pg_catalog.jsonb_typeof(prepared_snapshot -> 'activation')
            NOT IN ('null', 'object')
        OR pg_catalog.jsonb_typeof(prepared_snapshot -> 'panel_certificate')
            NOT IN ('null', 'object')
        OR pg_catalog.jsonb_typeof(prepared_snapshot -> 'gateway_ready')
            NOT IN ('null', 'object')
        OR pg_catalog.jsonb_typeof(prepared_snapshot -> 'live')
            NOT IN ('null', 'object')
        OR pg_catalog.jsonb_typeof(prepared_snapshot -> 'last_live_recovery')
            NOT IN ('null', 'object')
        OR pg_catalog.jsonb_typeof(prepared_snapshot -> 'last_runtime_failure')
            NOT IN ('null', 'object')
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(prepared_snapshot -> 'identity')
        ) <> 5
        OR NOT ((prepared_snapshot -> 'identity') ?& ARRAY[
            'deployment_id',
            'tenant_id',
            'installation_id',
            'promotion_id',
            'activation_request_id'
        ])
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(prepared_snapshot -> 'target')
        ) <> 6
        OR NOT ((prepared_snapshot -> 'target') ?& ARRAY[
            'guild_id',
            'ruleset_key',
            'version',
            'content_hash',
            'binding_revision',
            'binding_fingerprint'
        ])
        OR pg_catalog.jsonb_typeof(prepared_snapshot #> '{identity,deployment_id}')
            IS DISTINCT FROM 'string'
        OR pg_catalog.jsonb_typeof(prepared_snapshot #> '{identity,tenant_id}')
            IS DISTINCT FROM 'string'
        OR pg_catalog.jsonb_typeof(prepared_snapshot #> '{identity,installation_id}')
            IS DISTINCT FROM 'string'
        OR pg_catalog.jsonb_typeof(prepared_snapshot #> '{identity,promotion_id}')
            IS DISTINCT FROM 'string'
        OR pg_catalog.jsonb_typeof(prepared_snapshot #> '{identity,activation_request_id}')
            IS DISTINCT FROM 'string'
        OR pg_catalog.jsonb_typeof(prepared_snapshot #> '{target,guild_id}')
            IS DISTINCT FROM 'string'
        OR pg_catalog.jsonb_typeof(prepared_snapshot #> '{target,ruleset_key}')
            IS DISTINCT FROM 'string'
        OR pg_catalog.jsonb_typeof(prepared_snapshot #> '{target,version}')
            IS DISTINCT FROM 'number'
        OR pg_catalog.jsonb_typeof(prepared_snapshot #> '{target,content_hash}')
            IS DISTINCT FROM 'string'
        OR pg_catalog.jsonb_typeof(prepared_snapshot #> '{target,binding_revision}')
            IS DISTINCT FROM 'number'
        OR pg_catalog.jsonb_typeof(prepared_snapshot #> '{target,binding_fingerprint}')
            IS DISTINCT FROM 'string'
        OR pg_catalog.jsonb_typeof(prepared_snapshot -> 'runtime_generation')
            IS DISTINCT FROM 'number'
    THEN
        RETURN NULL;
    END IF;

    field := pg_catalog.convert_to(
        'starring.runtime.desired_target.v1',
        'UTF8'
    ) || pg_catalog.decode('00', 'hex');
    material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
        || field;

    field := pg_catalog.convert_to(
        prepared_snapshot #>> '{identity,deployment_id}',
        'UTF8'
    );
    material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
        || field;
    field := pg_catalog.convert_to(
        prepared_snapshot #>> '{identity,tenant_id}',
        'UTF8'
    );
    material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
        || field;
    field := pg_catalog.convert_to(
        prepared_snapshot #>> '{identity,installation_id}',
        'UTF8'
    );
    material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
        || field;
    field := pg_catalog.convert_to(
        prepared_snapshot #>> '{identity,promotion_id}',
        'UTF8'
    );
    material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
        || field;
    field := pg_catalog.convert_to(
        prepared_snapshot #>> '{identity,activation_request_id}',
        'UTF8'
    );
    material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
        || field;

    field := pg_catalog.convert_to(prepared_snapshot #>> '{target,guild_id}', 'UTF8');
    material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
        || field;
    field := pg_catalog.convert_to(prepared_snapshot #>> '{target,ruleset_key}', 'UTF8');
    material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
        || field;
    version_value := (prepared_snapshot #>> '{target,version}')::NUMERIC;
    IF version_value NOT BETWEEN 1 AND 4294967295
        OR pg_catalog.trunc(version_value) IS DISTINCT FROM version_value
    THEN
        RETURN NULL;
    END IF;
    field := pg_catalog.int4send(
        CASE
            WHEN version_value <= 2147483647 THEN version_value::INTEGER
            ELSE (version_value - 4294967296)::INTEGER
        END
    );
    material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
        || field;
    field := pg_catalog.convert_to(
        prepared_snapshot #>> '{target,content_hash}',
        'UTF8'
    );
    material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
        || field;
    field := pg_catalog.int8send(
        (prepared_snapshot #>> '{target,binding_revision}')::BIGINT
    );
    material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
        || field;
    field := pg_catalog.convert_to(
        prepared_snapshot #>> '{target,binding_fingerprint}',
        'UTF8'
    );
    material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
        || field;
    field := pg_catalog.int8send((prepared_snapshot ->> 'runtime_generation')::BIGINT);
    material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
        || field;
    field := pg_catalog.int8send(installation_authority_revision);
    material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
        || field;

    previous_runtime := prepared_snapshot -> 'previous_runtime';
    IF previous_runtime = 'null'::JSONB THEN
        field := pg_catalog.convert_to('absent', 'UTF8');
        material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
            || field;
    ELSE
        IF pg_catalog.jsonb_typeof(previous_runtime) IS DISTINCT FROM 'object'
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(previous_runtime)
            ) <> 3
            OR NOT previous_runtime ?& ARRAY[
                'target',
                'runtime_generation',
                'process_instance_id'
            ]
            OR pg_catalog.jsonb_typeof(previous_runtime -> 'target')
                IS DISTINCT FROM 'object'
            OR pg_catalog.jsonb_typeof(previous_runtime -> 'runtime_generation')
                IS DISTINCT FROM 'number'
            OR pg_catalog.jsonb_typeof(previous_runtime -> 'process_instance_id')
                IS DISTINCT FROM 'string'
        THEN
            RETURN NULL;
        END IF;
        previous_target := previous_runtime -> 'target';
        IF (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(previous_target)
            ) <> 6
            OR NOT previous_target ?& ARRAY[
                'guild_id',
                'ruleset_key',
                'version',
                'content_hash',
                'binding_revision',
                'binding_fingerprint'
            ]
            OR pg_catalog.jsonb_typeof(previous_target -> 'guild_id')
                IS DISTINCT FROM 'string'
            OR pg_catalog.jsonb_typeof(previous_target -> 'ruleset_key')
                IS DISTINCT FROM 'string'
            OR pg_catalog.jsonb_typeof(previous_target -> 'version')
                IS DISTINCT FROM 'number'
            OR pg_catalog.jsonb_typeof(previous_target -> 'content_hash')
                IS DISTINCT FROM 'string'
            OR pg_catalog.jsonb_typeof(previous_target -> 'binding_revision')
                IS DISTINCT FROM 'number'
            OR pg_catalog.jsonb_typeof(previous_target -> 'binding_fingerprint')
                IS DISTINCT FROM 'string'
        THEN
            RETURN NULL;
        END IF;

        field := pg_catalog.convert_to('present', 'UTF8');
        material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
            || field;
        field := pg_catalog.convert_to(previous_target ->> 'guild_id', 'UTF8');
        material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
            || field;
        field := pg_catalog.convert_to(previous_target ->> 'ruleset_key', 'UTF8');
        material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
            || field;
        version_value := (previous_target ->> 'version')::NUMERIC;
        IF version_value NOT BETWEEN 1 AND 4294967295
            OR pg_catalog.trunc(version_value) IS DISTINCT FROM version_value
        THEN
            RETURN NULL;
        END IF;
        field := pg_catalog.int4send(
            CASE
                WHEN version_value <= 2147483647 THEN version_value::INTEGER
                ELSE (version_value - 4294967296)::INTEGER
            END
        );
        material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
            || field;
        field := pg_catalog.convert_to(previous_target ->> 'content_hash', 'UTF8');
        material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
            || field;
        field := pg_catalog.int8send((previous_target ->> 'binding_revision')::BIGINT);
        material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
            || field;
        field := pg_catalog.convert_to(previous_target ->> 'binding_fingerprint', 'UTF8');
        material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
            || field;
        field := pg_catalog.int8send((previous_runtime ->> 'runtime_generation')::BIGINT);
        material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
            || field;
        field := pg_catalog.convert_to(previous_runtime ->> 'process_instance_id', 'UTF8');
        material := material || pg_catalog.int8send(pg_catalog.octet_length(field)::BIGINT)
            || field;
    END IF;

    RETURN pg_catalog.encode(pg_catalog.sha256(material), 'hex');
EXCEPTION
    WHEN OTHERS THEN
        RETURN NULL;
END;
$function$;

REVOKE ALL ON FUNCTION public.starring_runtime_desired_target_digest_v1(
    JSONB,
    BIGINT
) FROM PUBLIC;

DO $function$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM public.activation_requests AS activation
        WHERE activation.authority_kind = 'product_authoring'
            AND activation.state = 'applied'
            AND NOT EXISTS (
                SELECT 1
                FROM public.runtime_deployments AS deployment
                INNER JOIN public.automation_installation_authority_versions AS authority
                    ON authority.tenant_id = deployment.tenant_id
                    AND authority.installation_id = deployment.installation_id
                    AND authority.revision = deployment.installation_authority_revision
                INNER JOIN public.authoring_promotions AS promotion
                    ON promotion.id = deployment.promotion_id
                    AND promotion.tenant_id = deployment.tenant_id
                    AND promotion.installation_id = deployment.installation_id
                WHERE deployment.tenant_id = activation.tenant_id
                    AND deployment.installation_id = activation.installation_id
                    AND deployment.promotion_id = activation.promotion_id
                    AND deployment.activation_request_id = activation.id
                    AND deployment.guild_id = activation.guild_id
                    AND deployment.ruleset_key = activation.ruleset_key
                    AND deployment.target_version = activation.target_version
                    AND deployment.target_content_hash = activation.target_content_hash
                    AND deployment.binding_revision
                        = (activation.approval_context
                            #>> '{context,binding,revision}')::BIGINT
                    AND authority.binding_revision = deployment.binding_revision
                    AND authority.binding_fingerprint = deployment.binding_fingerprint
                    AND authority.policy_revision = deployment.policy_revision
                    AND promotion.record #>> '{intent,evidence,context_fingerprint}'
                        = authority.binding_fingerprint
                    AND promotion.record #> '{stage,activation,approval_context}'
                        = activation.approval_context -> 'context'
                    AND deployment.policy_revision
                        = (activation.approval_context
                            #>> '{context,policy,revision}')::BIGINT
                    AND deployment.desired_target_digest_version = 1
                    AND deployment.desired_target_digest
                        = public.starring_runtime_desired_target_digest_v1(
                            deployment.snapshot,
                            deployment.installation_authority_revision
                        )
            )
    ) THEN
        RAISE EXCEPTION 'atomic product apply migration found an Applied activation without an exact canonical deployment'
            USING ERRCODE = '23514',
                CONSTRAINT = 'atomic_product_apply_upgrade_deployment_complete';
    END IF;
END;
$function$;

CREATE FUNCTION public.enforce_runtime_deployment_policy_shadow()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    authoritative_policy_revision BIGINT;
BEGIN
    SELECT authority.policy_revision
    INTO authoritative_policy_revision
    FROM public.automation_installation_authority_versions AS authority
    WHERE authority.tenant_id = NEW.tenant_id
        AND authority.installation_id = NEW.installation_id
        AND authority.revision = NEW.installation_authority_revision
    FOR KEY SHARE;

    IF authoritative_policy_revision IS NULL THEN
        RAISE EXCEPTION 'runtime deployment policy authority is missing'
            USING ERRCODE = '23514',
                CONSTRAINT = 'runtime_deployments_policy_shadow_exact';
    END IF;
    IF TG_OP = 'INSERT' AND NEW.policy_revision IS NULL THEN
        NEW.policy_revision := authoritative_policy_revision;
    END IF;
    IF NEW.policy_revision IS DISTINCT FROM authoritative_policy_revision
        OR NEW.desired_target_digest_version IS DISTINCT FROM 1
        OR TG_OP = 'UPDATE'
            AND (
                NEW.policy_revision IS DISTINCT FROM OLD.policy_revision
                OR NEW.desired_target_digest_version
                    IS DISTINCT FROM OLD.desired_target_digest_version
            )
    THEN
        RAISE EXCEPTION 'runtime deployment policy shadow is not exact'
            USING ERRCODE = '23514',
                CONSTRAINT = 'runtime_deployments_policy_shadow_exact';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER runtime_deployments_policy_shadow_guard
BEFORE INSERT OR UPDATE ON public.runtime_deployments
FOR EACH ROW
EXECUTE FUNCTION public.enforce_runtime_deployment_policy_shadow();

REVOKE ALL ON FUNCTION public.enforce_runtime_deployment_policy_shadow()
FROM PUBLIC;

ALTER TABLE public.product_action_receipts
DROP CONSTRAINT product_action_receipts_approval_key_identity_required,
ADD CONSTRAINT product_action_receipts_approval_key_identity_required CHECK (
    endpoint_domain NOT IN ('product_approve_v1','product_apply_v1')
    OR (
        idempotency_digest_key_id IS NOT NULL
        AND idempotency_digest_key_fingerprint IS NOT NULL
    )
);

CREATE INDEX product_action_receipts_apply_retention_index
ON public.product_action_receipts (completed_at, receipt_id)
WHERE endpoint_domain = 'product_apply_v1';

CREATE INDEX product_action_aliases_apply_receipt_retention_index
ON public.product_action_receipt_idempotency_aliases (receipt_id)
WHERE endpoint_domain = 'product_apply_v1';

CREATE OR REPLACE FUNCTION public.assert_product_approval_receipt_alias()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $function$
BEGIN
    IF NEW.endpoint_domain IN ('product_approve_v1','product_apply_v1')
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

CREATE OR REPLACE FUNCTION public.assert_product_approval_receipt_audit()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $function$
DECLARE
    expected_action TEXT;
BEGIN
    expected_action := CASE NEW.endpoint_domain
        WHEN 'product_approve_v1' THEN 'promotion.approve'
        WHEN 'product_apply_v1' THEN 'promotion.apply'
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
SECURITY DEFINER
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

REVOKE ALL ON FUNCTION public.capture_product_action_receipt_audit_evidence()
FROM PUBLIC;

CREATE OR REPLACE FUNCTION public.enforce_product_action_receipt_retention()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
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

CREATE OR REPLACE FUNCTION public.enforce_product_action_receipt_alias_retention()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
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
        ELSE NULL
    END;
    IF expected_action IS NULL
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

REVOKE ALL ON FUNCTION public.enforce_product_action_receipt_retention()
FROM PUBLIC;

REVOKE ALL ON FUNCTION public.enforce_product_action_receipt_alias_retention()
FROM PUBLIC;

CREATE OR REPLACE FUNCTION public.starring_purge_product_action_receipts_v1(batch_limit INTEGER)
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
        WHERE receipt.endpoint_domain IN ('product_approve_v1','product_apply_v1')
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
                receipt.endpoint_domain NOT IN ('product_approve_v1','product_apply_v1')
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
        WHERE alias.endpoint_domain IN ('product_approve_v1','product_apply_v1')
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
    WHERE alias.endpoint_domain IN ('product_approve_v1','product_apply_v1')
        AND alias.receipt_id = ANY(candidate_receipt_ids);
    GET DIAGNOSTICS alias_count = ROW_COUNT;

    DELETE FROM public.product_action_receipts AS receipt
    WHERE receipt.receipt_id = ANY(candidate_receipt_ids)
        AND receipt.endpoint_domain IN ('product_approve_v1','product_apply_v1');
    GET DIAGNOSTICS receipt_count = ROW_COUNT;

    IF receipt_count IS DISTINCT FROM pg_catalog.cardinality(candidate_receipt_ids) THEN
        RAISE EXCEPTION 'product action receipt purge did not delete its locked batch'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_action_receipt_purge_batch_complete';
    END IF;

    SELECT EXISTS (
        SELECT 1
        FROM public.product_action_receipts AS receipt
        WHERE receipt.endpoint_domain IN ('product_approve_v1','product_apply_v1')
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

CREATE OR REPLACE FUNCTION public.starring_product_approval_keyring_coverage_v1(
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
        WHERE receipt.endpoint_domain IN ('product_approve_v1','product_apply_v1')
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

CREATE FUNCTION public.starring_product_apply_authority_projection_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_promotion_id TEXT,
    expected_principal_id TEXT,
    expected_product_session_digest BYTEA,
    expected_acting_user_id TEXT,
    expected_discord_application_id TEXT,
    expected_guild_id TEXT,
    expected_capability TEXT,
    expected_authority_revision BIGINT,
    expected_authority_payload_digest TEXT,
    expected_authority_observed_at TIMESTAMPTZ,
    expected_authority_expires_at TIMESTAMPTZ,
    expected_effective_permission_bits TEXT,
    expected_guild_owner BOOLEAN,
    expected_payload_digest TEXT
)
RETURNS JSONB
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
    target_schema_version BIGINT;
    target_definition JSONB;
    target_content_hash TEXT;
    target_created_by TEXT;
    approval_count BIGINT;
BEGIN
    IF expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_promotion_id !~ '^[0-9a-f]{64}$'
        OR expected_principal_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR pg_catalog.octet_length(expected_product_session_digest) <> 32
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
        OR expected_capability <> 'apply'
        OR expected_authority_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_authority_payload_digest !~ '^[0-9a-f]{64}$'
        OR expected_payload_digest !~ '^[0-9a-f]{64}$'
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
    THEN
        RETURN pg_catalog.jsonb_build_object('outcome', 'invalid_input');
    END IF;

    mutation_clock := pg_catalog.clock_timestamp();

    SELECT activation.*
    INTO activation_row
    FROM public.activation_requests AS activation
    WHERE activation.tenant_id = expected_tenant_id
        AND activation.installation_id = expected_installation_id
        AND activation.promotion_id = expected_promotion_id
    FOR UPDATE;
    IF NOT FOUND THEN
        RETURN pg_catalog.jsonb_build_object('outcome', 'not_found');
    END IF;

    SELECT promotion.*
    INTO promotion_row
    FROM public.authoring_promotions AS promotion
    WHERE promotion.tenant_id = expected_tenant_id
        AND promotion.installation_id = expected_installation_id
        AND promotion.id = expected_promotion_id
    FOR SHARE;
    IF NOT FOUND THEN
        RETURN pg_catalog.jsonb_build_object('outcome', 'not_found');
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
        RETURN pg_catalog.jsonb_build_object('outcome', 'authorization_stale');
    END IF;

    IF activation_row.approval_payload_digest
        IS DISTINCT FROM expected_payload_digest
    THEN
        RETURN pg_catalog.jsonb_build_object('outcome', 'payload_mismatch');
    END IF;

    IF installation_row.discord_application_id IS DISTINCT FROM expected_discord_application_id
        OR installation_row.discord_guild_id IS DISTINCT FROM expected_guild_id
        OR activation_row.authority_kind <> 'product_authoring'
        OR activation_row.link_state_name <> 'linked'
        OR activation_row.guild_id IS DISTINCT FROM expected_guild_id
        OR activation_row.ruleset_key IS DISTINCT FROM installation_row.ruleset_key
        OR promotion_row.stage <> 'activation_pending'
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
        RETURN pg_catalog.jsonb_build_object('outcome', 'scope_mismatch');
    END IF;

    IF installation_row.current_authority_revision IS DISTINCT FROM expected_authority_revision
        OR authority_row.authority_payload_digest
            IS DISTINCT FROM expected_authority_payload_digest
        OR authority_row.binding_revision::TEXT
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
        RETURN pg_catalog.jsonb_build_object('outcome', 'authority_mismatch');
    END IF;

    SELECT version.schema_version,
        version.definition,
        version.content_hash,
        version.created_by
    INTO target_schema_version,
        target_definition,
        target_content_hash,
        target_created_by
    FROM public.automation_ruleset_versions AS version
    WHERE version.guild_id = activation_row.guild_id
        AND version.ruleset_key = activation_row.ruleset_key
        AND version.version = activation_row.target_version
    FOR SHARE;
    IF NOT FOUND
        OR target_content_hash IS DISTINCT FROM activation_row.target_content_hash
        OR pg_catalog.octet_length(target_definition::TEXT) > 524288
    THEN
        RETURN pg_catalog.jsonb_build_object('outcome', 'target_mismatch');
    END IF;

    SELECT pg_catalog.count(*)
    INTO approval_count
    FROM public.activation_request_approvals AS approval
    WHERE approval.tenant_id = expected_tenant_id
        AND approval.installation_id = expected_installation_id
        AND approval.request_id = activation_row.id
        AND approval.approval_payload_digest = activation_row.approval_payload_digest;

    RETURN pg_catalog.jsonb_build_object(
        'outcome', 'ok',
        'scope', pg_catalog.jsonb_build_object(
            'tenant_id', expected_tenant_id,
            'installation_id', expected_installation_id,
            'promotion_id', expected_promotion_id,
            'principal_id', expected_principal_id,
            'acting_user_id', expected_acting_user_id,
            'discord_application_id', expected_discord_application_id,
            'guild_id', expected_guild_id
        ),
        'activation', pg_catalog.jsonb_build_object(
            'request_id', activation_row.id,
            'product_revision', activation_row.product_revision,
            'state', activation_row.state,
            'requester_id', activation_row.requester_id,
            'required_approvals', activation_row.required_approvals,
            'approval_count', approval_count,
            'expires_at', activation_row.expires_at,
            'approval_payload_digest', activation_row.approval_payload_digest,
            'approval_context_digest', activation_row.approval_context_digest,
            'approval_context', activation_row.approval_context,
            'observed_active_version', activation_row.observed_active_version,
            'observed_active_hash', activation_row.observed_active_hash
        ),
        'authority', pg_catalog.jsonb_build_object(
            'revision', authority_row.revision,
            'payload_digest', authority_row.authority_payload_digest,
            'binding_revision', authority_row.binding_revision,
            'binding_fingerprint', authority_row.binding_fingerprint,
            'policy_revision', authority_row.policy_revision,
            'required_approvals', authority_row.required_approvals,
            'activation_ttl_seconds', authority_row.activation_ttl_seconds,
            'resource_bindings', authority_row.resource_bindings
        ),
        'target', pg_catalog.jsonb_build_object(
            'guild_id', activation_row.guild_id,
            'ruleset_key', activation_row.ruleset_key,
            'version', activation_row.target_version,
            'content_hash', activation_row.target_content_hash,
            'schema_version', target_schema_version,
            'definition', target_definition,
            'created_by', target_created_by
        )
    );
END;
$function$;

REVOKE ALL ON FUNCTION public.starring_product_apply_authority_projection_v1(
    TEXT,
    TEXT,
    TEXT,
    TEXT,
    BYTEA,
    TEXT,
    TEXT,
    TEXT,
    TEXT,
    BIGINT,
    TEXT,
    TIMESTAMPTZ,
    TIMESTAMPTZ,
    TEXT,
    BOOLEAN,
    TEXT
) FROM PUBLIC;

CREATE FUNCTION public.starring_product_apply_lock_v1(
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
    new_apply_attempt_id TEXT,
    new_deployment_id TEXT
)
RETURNS TABLE (
    outcome TEXT,
    exact_replay BOOLEAN,
    requires_commit BOOLEAN,
    resulting_revision BIGINT,
    resulting_state TEXT,
    deployment_id TEXT,
    desired_target_digest TEXT,
    locked_projection JSONB
)
LANGUAGE plpgsql
VOLATILE
STRICT
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    authority_projection JSONB;
    tenant_row public.product_tenants%ROWTYPE;
    installation_row public.automation_installations%ROWTYPE;
    principal_row public.product_principals%ROWTYPE;
    session_row public.product_auth_sessions%ROWTYPE;
    receipt_row public.product_action_receipts%ROWTYPE;
    matched_receipt_count BIGINT;
    candidate_lock_digest TEXT;
    replay_deployment_id TEXT;
    replay_desired_target_digest TEXT;
    replay_count BIGINT;
    current_active_version BIGINT;
    current_active_hash TEXT;
    target_is_active BOOLEAN;
    unresolved_deployment_id TEXT;
    last_runtime_generation BIGINT;
    next_runtime_generation BIGINT;
    serving_row public.runtime_serving_leases%ROWTYPE;
    previous_runtime JSONB;
    requested_at TIMESTAMPTZ;
    authorization_clock TIMESTAMPTZ;
    replay_payload_digest TEXT;
    projection_token JSONB;
BEGIN
    IF pg_catalog.current_setting('transaction_isolation') <> 'serializable'
        OR pg_catalog.current_setting('transaction_read_only') <> 'off'
        OR expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_promotion_id !~ '^[0-9a-f]{64}$'
        OR expected_payload_digest !~ '^[0-9a-f]{64}$'
        OR expected_principal_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR pg_catalog.octet_length(expected_product_session_digest) <> 32
        OR expected_product_revision NOT BETWEEN 1 AND 9223372036854775807
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
        OR expected_capability <> 'apply'
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
        OR new_apply_attempt_id !~ '^[A-Za-z0-9_-]{1,64}$'
        OR new_deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
    THEN
        RETURN QUERY SELECT 'invalid_input', FALSE, FALSE, NULL::BIGINT, NULL::TEXT,
            NULL::TEXT, NULL::TEXT, NULL::JSONB;
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
        RETURN QUERY SELECT 'invalid_input', FALSE, FALSE, NULL::BIGINT, NULL::TEXT,
            NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            expected_tenant_id || ':' || expected_installation_id || ':'
                || expected_principal_id || ':product_apply_v1:key-coverage',
            0
        )
    );
    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            expected_tenant_id || ':' || expected_installation_id || ':'
                || expected_guild_id || ':product_apply_v1:lane',
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
                    || expected_principal_id || ':product_apply_v1:'
                    || candidate_lock_digest,
                0
            )
        );
    END LOOP;

    authorization_clock := pg_catalog.clock_timestamp();
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

    IF tenant_row.tenant_id IS NULL
        OR installation_row.installation_id IS NULL
        OR principal_row.principal_id IS NULL
        OR session_row.principal_id IS NULL
        OR tenant_row.lifecycle_state <> 'active'
        OR installation_row.lifecycle_state <> 'active'
        OR principal_row.disabled
        OR principal_row.discord_user_id IS DISTINCT FROM expected_acting_user_id
        OR session_row.oauth_state_digest IS NULL
        OR session_row.revoked_at IS NOT NULL
        OR authorization_clock >= session_row.idle_expires_at
        OR authorization_clock >= session_row.absolute_expires_at
        OR expected_authority_observed_at > authorization_clock
        OR authorization_clock >= expected_authority_expires_at
    THEN
        RETURN QUERY SELECT 'authorization_stale', FALSE, FALSE,
            NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;
    IF installation_row.discord_application_id
            IS DISTINCT FROM expected_discord_application_id
        OR installation_row.discord_guild_id IS DISTINCT FROM expected_guild_id
    THEN
        RETURN QUERY SELECT 'scope_mismatch', FALSE, FALSE, NULL::BIGINT,
            NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;

    PERFORM deployment.deployment_id
    FROM public.runtime_deployments AS deployment
    WHERE deployment.guild_id = expected_guild_id
        AND deployment.ruleset_key = installation_row.ruleset_key
    ORDER BY deployment.runtime_generation, deployment.deployment_id
    FOR UPDATE;

    IF EXISTS (
        SELECT 1
        FROM public.product_action_receipts AS receipt
        WHERE receipt.tenant_id = expected_tenant_id
            AND receipt.installation_id = expected_installation_id
            AND receipt.principal_id = expected_principal_id
            AND receipt.endpoint_domain = 'product_apply_v1'
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
        RETURN QUERY SELECT 'idempotency_keyring_incomplete', FALSE, FALSE,
            NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;

    SELECT pg_catalog.count(DISTINCT alias.receipt_id)
    INTO matched_receipt_count
    FROM public.product_action_receipt_idempotency_aliases AS alias
    WHERE alias.tenant_id = expected_tenant_id
        AND alias.installation_id = expected_installation_id
        AND alias.principal_id = expected_principal_id
        AND alias.endpoint_domain = 'product_apply_v1'
        AND alias.idempotency_key_digest = ANY(idempotency_key_digest_candidates);

    IF matched_receipt_count > 1 THEN
        RETURN QUERY SELECT 'indeterminate', FALSE, FALSE, NULL::BIGINT, NULL::TEXT,
            NULL::TEXT, NULL::TEXT, NULL::JSONB;
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
                AND alias.endpoint_domain = 'product_apply_v1'
                AND alias.idempotency_key_digest = ANY(idempotency_key_digest_candidates)
            ORDER BY alias.receipt_id
            LIMIT 1
        ) AS matched ON matched.receipt_id = receipt.receipt_id
        WHERE receipt.tenant_id = expected_tenant_id
            AND receipt.installation_id = expected_installation_id
            AND receipt.principal_id = expected_principal_id
            AND receipt.endpoint_domain = 'product_apply_v1'
        FOR UPDATE OF receipt;

        IF receipt_row.receipt_id IS NULL THEN
            RETURN QUERY SELECT 'indeterminate', FALSE, FALSE, NULL::BIGINT, NULL::TEXT,
                NULL::TEXT, NULL::TEXT, NULL::JSONB;
            RETURN;
        END IF;
        IF receipt_row.request_digest IS DISTINCT FROM semantic_request_digest THEN
            RETURN QUERY SELECT 'idempotency_conflict', FALSE, FALSE, NULL::BIGINT,
                NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
            RETURN;
        END IF;

        SELECT activation.approval_payload_digest
        INTO replay_payload_digest
        FROM public.activation_requests AS activation
        WHERE activation.tenant_id = expected_tenant_id
            AND activation.installation_id = expected_installation_id
            AND activation.promotion_id = expected_promotion_id
        FOR SHARE;
        IF NOT FOUND THEN
            RETURN QUERY SELECT 'indeterminate', FALSE, FALSE, NULL::BIGINT,
                NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
            RETURN;
        END IF;
        IF replay_payload_digest IS DISTINCT FROM expected_payload_digest THEN
            RETURN QUERY SELECT 'payload_mismatch', FALSE, FALSE, NULL::BIGINT,
                NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
            RETURN;
        END IF;

        SELECT pg_catalog.count(*),
            pg_catalog.min(deployment.deployment_id),
            pg_catalog.min(deployment.desired_target_digest)
        INTO replay_count, replay_deployment_id, replay_desired_target_digest
        FROM public.runtime_deployments AS deployment
        INNER JOIN public.activation_requests AS activation
            ON activation.tenant_id = deployment.tenant_id
            AND activation.installation_id = deployment.installation_id
            AND activation.promotion_id = deployment.promotion_id
            AND activation.id = deployment.activation_request_id
        INNER JOIN public.automation_installation_authority_versions AS authority
            ON authority.tenant_id = deployment.tenant_id
            AND authority.installation_id = deployment.installation_id
            AND authority.revision = deployment.installation_authority_revision
        INNER JOIN public.authoring_promotions AS promotion
            ON promotion.id = deployment.promotion_id
            AND promotion.tenant_id = deployment.tenant_id
            AND promotion.installation_id = deployment.installation_id
        WHERE deployment.tenant_id = expected_tenant_id
            AND deployment.installation_id = expected_installation_id
            AND deployment.promotion_id = expected_promotion_id
            AND deployment.guild_id = expected_guild_id
            AND deployment.ruleset_key = installation_row.ruleset_key
            AND activation.authority_kind = 'product_authoring'
            AND activation.link_state_name = 'linked'
            AND activation.state = 'applied'
            AND activation.approval_payload_digest = expected_payload_digest
            AND deployment.guild_id = activation.guild_id
            AND deployment.ruleset_key = activation.ruleset_key
            AND deployment.target_version = activation.target_version
            AND deployment.target_content_hash = activation.target_content_hash
            AND deployment.binding_revision
                = (activation.approval_context
                    #>> '{context,binding,revision}')::BIGINT
            AND deployment.binding_fingerprint = authority.binding_fingerprint
            AND deployment.policy_revision
                = (activation.approval_context
                    #>> '{context,policy,revision}')::BIGINT
            AND authority.binding_revision = deployment.binding_revision
            AND authority.binding_fingerprint = deployment.binding_fingerprint
            AND authority.policy_revision = deployment.policy_revision
            AND promotion.record #>> '{intent,evidence,context_fingerprint}'
                = authority.binding_fingerprint
            AND promotion.record #> '{stage,activation,approval_context}'
                = activation.approval_context -> 'context'
            AND deployment.desired_target_digest_version = 1
            AND deployment.desired_target_digest
                = public.starring_runtime_desired_target_digest_v1(
                    deployment.snapshot,
                    deployment.installation_authority_revision
                );

        IF receipt_row.target_resource_type <> 'authoring_promotion'
            OR receipt_row.target_resource_id IS DISTINCT FROM expected_promotion_id
            OR receipt_row.resulting_revision IS NULL
            OR receipt_row.resulting_state <> 'applied'
            OR receipt_row.result_code <> 'runtime_requested'
            OR replay_count <> 1
            OR replay_deployment_id IS NULL
            OR replay_desired_target_digest !~ '^[0-9a-f]{64}$'
            OR NOT EXISTS (
                SELECT 1
                FROM public.product_audit_events AS audit
                INNER JOIN public.product_action_receipt_audit_evidence AS evidence
                    ON evidence.receipt_id = audit.receipt_id
                    AND evidence.event_id = audit.event_id
                    AND evidence.endpoint_domain = 'product_apply_v1'
                    AND evidence.action = 'promotion.apply'
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
                WHERE audit.receipt_id = receipt_row.receipt_id
                    AND audit.tenant_id = receipt_row.tenant_id
                    AND audit.installation_id = receipt_row.installation_id
                    AND audit.principal_id = receipt_row.principal_id
                    AND audit.action = 'promotion.apply'
                    AND audit.target_resource_type = receipt_row.target_resource_type
                    AND audit.target_resource_id = receipt_row.target_resource_id
                    AND audit.resulting_state = receipt_row.resulting_state
                    AND audit.result_code = receipt_row.result_code
                    AND audit.installation_authority_revision = (
                        SELECT deployment.installation_authority_revision
                        FROM public.runtime_deployments AS deployment
                        WHERE deployment.deployment_id = replay_deployment_id
                    )
                    AND audit.payload_digest = expected_payload_digest
                    AND audit.binding_fingerprint = (
                        SELECT deployment.binding_fingerprint
                        FROM public.runtime_deployments AS deployment
                        WHERE deployment.deployment_id = replay_deployment_id
                    )
                    AND audit.policy_revision = (
                        SELECT deployment.policy_revision
                        FROM public.runtime_deployments AS deployment
                        WHERE deployment.deployment_id = replay_deployment_id
                    )
            )
        THEN
            RETURN QUERY SELECT 'indeterminate', FALSE, FALSE, NULL::BIGINT, NULL::TEXT,
                NULL::TEXT, NULL::TEXT, NULL::JSONB;
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

        RETURN QUERY SELECT 'ok', TRUE, TRUE, receipt_row.resulting_revision,
            receipt_row.resulting_state, replay_deployment_id,
            replay_desired_target_digest, NULL::JSONB;
        RETURN;
    END IF;

    authority_projection := public.starring_product_apply_authority_projection_v1(
        expected_tenant_id,
        expected_installation_id,
        expected_promotion_id,
        expected_principal_id,
        expected_product_session_digest,
        expected_acting_user_id,
        expected_discord_application_id,
        expected_guild_id,
        expected_capability,
        expected_authority_revision,
        expected_authority_payload_digest,
        expected_authority_observed_at,
        expected_authority_expires_at,
        expected_effective_permission_bits,
        expected_guild_owner,
        expected_payload_digest
    );
    IF authority_projection ->> 'outcome' <> 'ok' THEN
        RETURN QUERY SELECT authority_projection ->> 'outcome', FALSE, FALSE,
            NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;

    IF EXISTS (
        SELECT 1
        FROM public.product_audit_events AS audit
        WHERE audit.tenant_id = expected_tenant_id
            AND audit.request_id = product_request_id
    ) OR EXISTS (
        SELECT 1
        FROM public.product_action_receipts AS receipt
        WHERE receipt.receipt_id = new_receipt_id
    ) OR EXISTS (
        SELECT 1
        FROM public.product_audit_events AS audit
        WHERE audit.event_id = new_audit_event_id
    ) OR EXISTS (
        SELECT 1
        FROM public.runtime_deployments AS deployment
        WHERE deployment.deployment_id = new_deployment_id
    ) THEN
        RETURN QUERY SELECT 'idempotency_conflict', FALSE, FALSE, NULL::BIGINT,
            NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;

    IF (authority_projection #>> '{activation,product_revision}')::BIGINT
            IS DISTINCT FROM expected_product_revision
    THEN
        RETURN QUERY SELECT 'revision_conflict', FALSE, FALSE, NULL::BIGINT,
            NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;
    IF authority_projection #>> '{activation,approval_payload_digest}'
            IS DISTINCT FROM expected_payload_digest
    THEN
        RETURN QUERY SELECT 'payload_mismatch', FALSE, FALSE, NULL::BIGINT,
            NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;
    IF (authority_projection #>> '{activation,expires_at}')::TIMESTAMPTZ
            <= pg_catalog.clock_timestamp()
    THEN
        RETURN QUERY SELECT 'expired', FALSE, FALSE, NULL::BIGINT, NULL::TEXT,
            NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;
    IF authority_projection #>> '{activation,state}' <> 'approved'
        OR (authority_projection #>> '{activation,approval_count}')::BIGINT
            < (authority_projection #>> '{activation,required_approvals}')::BIGINT
    THEN
        RETURN QUERY SELECT 'invalid_state', FALSE, FALSE, NULL::BIGINT, NULL::TEXT,
            NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;

    SELECT active.active_version,
        version.content_hash
    INTO current_active_version,
        current_active_hash
    FROM public.automation_ruleset_activations AS active
    INNER JOIN public.automation_ruleset_versions AS version
        ON version.guild_id = active.guild_id
        AND version.ruleset_key = active.ruleset_key
        AND version.version = active.active_version
    WHERE active.guild_id = expected_guild_id
        AND active.ruleset_key = authority_projection #>> '{target,ruleset_key}'
    FOR UPDATE OF active;

    target_is_active := current_active_version
            IS NOT DISTINCT FROM (authority_projection #>> '{target,version}')::BIGINT
        AND current_active_hash
            IS NOT DISTINCT FROM authority_projection #>> '{target,content_hash}';
    IF NOT target_is_active
        AND (
            current_active_version IS DISTINCT FROM
                NULLIF(
                    authority_projection #>> '{activation,observed_active_version}',
                    ''
                )::BIGINT
            OR current_active_hash IS DISTINCT FROM
                NULLIF(
                    authority_projection #>> '{activation,observed_active_hash}',
                    ''
                )
        )
    THEN
        RETURN QUERY SELECT 'baseline_mismatch', FALSE, FALSE, NULL::BIGINT,
            NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;

    SELECT deployment.deployment_id
    INTO unresolved_deployment_id
    FROM public.runtime_deployments AS deployment
    WHERE deployment.guild_id = expected_guild_id
        AND deployment.ruleset_key = authority_projection #>> '{target,ruleset_key}'
        AND deployment.phase NOT IN ('live','superseded','cancelled')
    ORDER BY deployment.runtime_generation DESC, deployment.deployment_id
    LIMIT 1
    FOR UPDATE;
    IF unresolved_deployment_id IS NOT NULL THEN
        RETURN QUERY SELECT 'runtime_pending_conflict', FALSE, FALSE, NULL::BIGINT,
            NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;

    SELECT deployment.runtime_generation
    INTO last_runtime_generation
    FROM public.runtime_deployments AS deployment
    WHERE deployment.guild_id = expected_guild_id
        AND deployment.ruleset_key = authority_projection #>> '{target,ruleset_key}'
    ORDER BY deployment.runtime_generation DESC, deployment.deployment_id
    LIMIT 1
    FOR UPDATE;
    IF last_runtime_generation IS NULL THEN
        next_runtime_generation := 1;
    ELSIF last_runtime_generation = 9223372036854775807 THEN
        RETURN QUERY SELECT 'runtime_generation_overflow', FALSE, FALSE, NULL::BIGINT,
            NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    ELSE
        next_runtime_generation := last_runtime_generation + 1;
    END IF;

    SELECT serving.*
    INTO serving_row
    FROM public.runtime_serving_leases AS serving
    WHERE serving.guild_id = expected_guild_id
        AND serving.ruleset_key = authority_projection #>> '{target,ruleset_key}'
    FOR UPDATE;
    requested_at := pg_catalog.transaction_timestamp();
    IF serving_row.guild_id IS NOT NULL
        AND serving_row.connected
        AND serving_row.serving
        AND serving_row.expires_at > requested_at
    THEN
        previous_runtime := pg_catalog.jsonb_build_object(
            'target', pg_catalog.jsonb_build_object(
                'guild_id', serving_row.guild_id,
                'ruleset_key', serving_row.ruleset_key,
                'version', serving_row.target_version,
                'content_hash', serving_row.target_content_hash,
                'binding_revision', serving_row.binding_revision,
                'binding_fingerprint', serving_row.binding_fingerprint
            ),
            'runtime_generation', serving_row.runtime_generation,
            'process_instance_id', serving_row.process_instance_id
        );
        IF next_runtime_generation <= serving_row.runtime_generation THEN
            RETURN QUERY SELECT 'runtime_generation_conflict', FALSE, FALSE,
                NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
            RETURN;
        END IF;
    ELSE
        previous_runtime := 'null'::JSONB;
    END IF;

    projection_token := pg_catalog.jsonb_build_object(
        'version', 1,
        'requested_at', requested_at,
        'operation', pg_catalog.jsonb_build_object(
            'endpoint_domain', 'product_apply_v1',
            'semantic_request_digest', semantic_request_digest,
            'request_id', product_request_id,
            'receipt_id', new_receipt_id,
            'audit_event_id', new_audit_event_id,
            'apply_attempt_id', new_apply_attempt_id,
            'deployment_id', new_deployment_id,
            'product_session_binding_v1', pg_catalog.md5(
                'product-session-v1:'
                    || pg_catalog.encode(expected_product_session_digest, 'hex')
            ),
            'session_subject_binding_v1', pg_catalog.md5(
                'session-subject-v1:'
                    || pg_catalog.encode(session_subject_digest, 'hex')
            ),
            'active_idempotency_key_digest', active_idempotency_key_digest,
            'idempotency_key_digest_candidates',
                pg_catalog.to_jsonb(idempotency_key_digest_candidates),
            'idempotency_digest_key_id_candidates',
                pg_catalog.to_jsonb(idempotency_digest_key_id_candidates),
            'idempotency_digest_key_fingerprint_candidates',
                pg_catalog.to_jsonb(idempotency_digest_key_fingerprint_candidates),
            'idempotency_digest_key_id', idempotency_digest_key_id,
            'authority_observation_digest', expected_authority_observation_digest,
            'authority_observed_at', expected_authority_observed_at,
            'authority_expires_at', expected_authority_expires_at,
            'effective_permission_bits', expected_effective_permission_bits,
            'guild_owner', expected_guild_owner
        ),
        'server', authority_projection - 'outcome',
        'active', CASE
            WHEN current_active_version IS NULL THEN 'null'::JSONB
            ELSE pg_catalog.jsonb_build_object(
                'version', current_active_version,
                'content_hash', current_active_hash
            )
        END,
        'target_is_active', target_is_active,
        'runtime_generation', next_runtime_generation,
        'previous_runtime', previous_runtime
    );
    IF pg_catalog.octet_length(projection_token::TEXT) > 1048576 THEN
        RETURN QUERY SELECT 'projection_too_large', FALSE, FALSE, NULL::BIGINT,
            NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;

    PERFORM pg_catalog.set_config(
        'starring.product_apply_lock_token_v1',
        'v1:' || pg_catalog.md5(projection_token::TEXT),
        TRUE
    );
    RETURN QUERY SELECT 'ready', FALSE, FALSE, expected_product_revision,
        'approved', new_deployment_id, NULL::TEXT, projection_token;
END;
$function$;

REVOKE ALL ON FUNCTION public.starring_product_apply_lock_v1(
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
    TEXT,
    TEXT
) FROM PUBLIC;

CREATE FUNCTION public.starring_product_apply_finalize_v1(
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
    new_apply_attempt_id TEXT,
    new_deployment_id TEXT,
    locked_projection JSONB,
    prepared_desired_target_digest TEXT,
    prepared_previous_runtime JSONB,
    prepared_snapshot JSONB,
    prepared_activation_notices JSONB
)
RETURNS TABLE (
    outcome TEXT,
    resulting_revision BIGINT,
    resulting_state TEXT,
    exact_replay BOOLEAN,
    guild_id TEXT,
    deployment_id TEXT,
    desired_target_digest TEXT
)
LANGUAGE plpgsql
VOLATILE
STRICT
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    lock_row RECORD;
    requested_at TIMESTAMPTZ;
    prepared_requested_at TIMESTAMPTZ;
    mutation_clock TIMESTAMPTZ;
    next_revision BIGINT;
    pointer_rows BIGINT;
    active_baseline_version BIGINT;
    active_baseline_hash TEXT;
    applied_completion_kind TEXT;
BEGIN
    IF pg_catalog.current_setting('transaction_isolation') <> 'serializable'
        OR pg_catalog.current_setting('transaction_read_only') <> 'off'
        OR pg_catalog.jsonb_typeof(locked_projection) <> 'object'
        OR pg_catalog.octet_length(locked_projection::TEXT) > 1048576
        OR pg_catalog.current_setting(
            'starring.product_apply_lock_token_v1',
            TRUE
        ) IS DISTINCT FROM 'v1:' || pg_catalog.md5(locked_projection::TEXT)
    THEN
        RETURN QUERY SELECT 'lock_required', NULL::BIGINT, NULL::TEXT, FALSE,
            NULL::TEXT, NULL::TEXT, NULL::TEXT;
        RETURN;
    END IF;

    SELECT *
    INTO lock_row
    FROM public.starring_product_apply_lock_v1(
        expected_tenant_id,
        expected_installation_id,
        expected_promotion_id,
        expected_product_revision,
        expected_payload_digest,
        expected_principal_id,
        expected_product_session_digest,
        session_subject_digest,
        expected_acting_user_id,
        expected_discord_application_id,
        expected_guild_id,
        expected_capability,
        expected_authority_revision,
        expected_authority_payload_digest,
        expected_authority_observation_digest,
        expected_authority_observed_at,
        expected_authority_expires_at,
        expected_effective_permission_bits,
        expected_guild_owner,
        product_request_id,
        active_idempotency_key_digest,
        idempotency_key_digest_candidates,
        idempotency_digest_key_id_candidates,
        idempotency_digest_key_fingerprint_candidates,
        idempotency_digest_key_id,
        semantic_request_digest,
        new_receipt_id,
        new_audit_event_id,
        new_apply_attempt_id,
        new_deployment_id
    );
    IF lock_row.outcome IS DISTINCT FROM 'ready'
        OR lock_row.exact_replay
        OR lock_row.locked_projection IS DISTINCT FROM locked_projection
    THEN
        RETURN QUERY SELECT CASE
                WHEN lock_row.outcome = 'ready' THEN 'locked_projection_mismatch'
                ELSE COALESCE(lock_row.outcome, 'indeterminate')
            END,
            NULL::BIGINT,
            NULL::TEXT,
            FALSE,
            NULL::TEXT,
            NULL::TEXT,
            NULL::TEXT;
        RETURN;
    END IF;

    requested_at := (locked_projection ->> 'requested_at')::TIMESTAMPTZ;
    IF requested_at IS DISTINCT FROM pg_catalog.transaction_timestamp()
        OR prepared_desired_target_digest !~ '^[0-9a-f]{64}$'
        OR pg_catalog.jsonb_typeof(prepared_snapshot) IS DISTINCT FROM 'object'
        OR pg_catalog.octet_length(prepared_snapshot::TEXT) NOT BETWEEN 32 AND 262144
        OR pg_catalog.jsonb_typeof(prepared_activation_notices) IS DISTINCT FROM 'array'
        OR pg_catalog.octet_length(prepared_activation_notices::TEXT) > 16384
    THEN
        RETURN QUERY SELECT 'invalid_runtime_projection', NULL::BIGINT, NULL::TEXT,
            FALSE, NULL::TEXT, NULL::TEXT, NULL::TEXT;
        RETURN;
    END IF;

    IF (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(prepared_snapshot)
        ) <> 17
        OR NOT prepared_snapshot ?& ARRAY[
            'identity',
            'target',
            'runtime_generation',
            'previous_runtime',
            'requested_at',
            'revision',
            'phase',
            'controller_lease',
            'last_fencing_token',
            'preflight',
            'drain',
            'activation',
            'panel_certificate',
            'gateway_ready',
            'live',
            'last_live_recovery',
            'last_runtime_failure'
        ]
        OR pg_catalog.jsonb_typeof(prepared_snapshot -> 'identity')
            IS DISTINCT FROM 'object'
        OR pg_catalog.jsonb_typeof(prepared_snapshot -> 'target')
            IS DISTINCT FROM 'object'
    THEN
        RETURN QUERY SELECT 'invalid_runtime_projection', NULL::BIGINT, NULL::TEXT,
            FALSE, NULL::TEXT, NULL::TEXT, NULL::TEXT;
        RETURN;
    END IF;

    IF (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(prepared_snapshot -> 'identity')
        ) <> 5
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(prepared_snapshot -> 'target')
        ) <> 6
        OR prepared_snapshot -> 'identity' IS DISTINCT FROM pg_catalog.jsonb_build_object(
            'deployment_id', new_deployment_id,
            'tenant_id', expected_tenant_id,
            'installation_id', expected_installation_id,
            'promotion_id', expected_promotion_id,
            'activation_request_id',
                locked_projection #>> '{server,activation,request_id}'
        )
        OR prepared_snapshot -> 'target' IS DISTINCT FROM pg_catalog.jsonb_build_object(
            'guild_id', expected_guild_id,
            'ruleset_key', locked_projection #>> '{server,target,ruleset_key}',
            'version', (locked_projection #>> '{server,target,version}')::BIGINT,
            'content_hash', locked_projection #>> '{server,target,content_hash}',
            'binding_revision',
                (locked_projection #>> '{server,authority,binding_revision}')::BIGINT,
            'binding_fingerprint',
                locked_projection #>> '{server,authority,binding_fingerprint}'
        )
        OR prepared_snapshot -> 'runtime_generation' IS DISTINCT FROM pg_catalog.to_jsonb(
            (locked_projection ->> 'runtime_generation')::BIGINT
        )
        OR pg_catalog.jsonb_typeof(prepared_snapshot -> 'requested_at')
            IS DISTINCT FROM 'string'
        OR prepared_snapshot -> 'revision'
            IS DISTINCT FROM pg_catalog.to_jsonb(1::BIGINT)
        OR prepared_snapshot -> 'phase'
            IS DISTINCT FROM '{"phase":"requested"}'::JSONB
        OR prepared_snapshot -> 'previous_runtime'
            IS DISTINCT FROM locked_projection -> 'previous_runtime'
        OR prepared_previous_runtime
            IS DISTINCT FROM locked_projection -> 'previous_runtime'
        OR prepared_snapshot -> 'controller_lease' IS DISTINCT FROM 'null'::JSONB
        OR prepared_snapshot -> 'last_fencing_token' IS DISTINCT FROM 'null'::JSONB
        OR prepared_snapshot -> 'preflight' IS DISTINCT FROM 'null'::JSONB
        OR prepared_snapshot -> 'drain' IS DISTINCT FROM 'null'::JSONB
        OR prepared_snapshot -> 'activation' IS DISTINCT FROM 'null'::JSONB
        OR prepared_snapshot -> 'panel_certificate' IS DISTINCT FROM 'null'::JSONB
        OR prepared_snapshot -> 'gateway_ready' IS DISTINCT FROM 'null'::JSONB
        OR prepared_snapshot -> 'live' IS DISTINCT FROM 'null'::JSONB
        OR prepared_snapshot -> 'last_live_recovery' IS DISTINCT FROM 'null'::JSONB
        OR prepared_snapshot -> 'last_runtime_failure' IS DISTINCT FROM 'null'::JSONB
        OR pg_catalog.jsonb_array_length(prepared_activation_notices) > 128
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_array_elements(prepared_activation_notices) AS notice(value)
            WHERE pg_catalog.jsonb_typeof(notice.value) IS DISTINCT FROM 'string'
                OR pg_catalog.char_length(notice.value #>> '{}') > 1024
        )
    THEN
        RETURN QUERY SELECT 'invalid_runtime_projection', NULL::BIGINT, NULL::TEXT,
            FALSE, NULL::TEXT, NULL::TEXT, NULL::TEXT;
        RETURN;
    END IF;

    BEGIN
        prepared_requested_at := (prepared_snapshot ->> 'requested_at')::TIMESTAMPTZ;
    EXCEPTION
        WHEN invalid_datetime_format OR datetime_field_overflow THEN
            RETURN QUERY SELECT 'invalid_runtime_projection', NULL::BIGINT, NULL::TEXT,
                FALSE, NULL::TEXT, NULL::TEXT, NULL::TEXT;
            RETURN;
    END;
    IF prepared_requested_at IS DISTINCT FROM requested_at
        OR prepared_desired_target_digest IS DISTINCT FROM
            public.starring_runtime_desired_target_digest_v1(
                prepared_snapshot,
                expected_authority_revision
            )
    THEN
        RETURN QUERY SELECT 'invalid_runtime_projection', NULL::BIGINT, NULL::TEXT,
            FALSE, NULL::TEXT, NULL::TEXT, NULL::TEXT;
        RETURN;
    END IF;

    IF (locked_projection #>> '{server,activation,product_revision}')::BIGINT
            = 9223372036854775807
        OR (locked_projection #>> '{server,activation,product_revision}')::BIGINT
            = 9223372036854775806
    THEN
        RETURN QUERY SELECT 'revision_overflow', NULL::BIGINT, NULL::TEXT, FALSE,
            NULL::TEXT, NULL::TEXT, NULL::TEXT;
        RETURN;
    END IF;

    PERFORM pg_catalog.set_config(
        'starring.product_approval_context_digest',
        locked_projection #>> '{server,activation,approval_context_digest}',
        TRUE
    );
    UPDATE public.activation_requests AS activation
    SET state = 'applying',
        apply_attempt_id = new_apply_attempt_id,
        apply_attempt_no = activation.apply_attempt_no + 1,
        apply_lease_until = requested_at + INTERVAL '60 seconds',
        last_apply_error = NULL,
        product_revision = activation.product_revision + 1
    WHERE activation.tenant_id = expected_tenant_id
        AND activation.installation_id = expected_installation_id
        AND activation.id = locked_projection #>> '{server,activation,request_id}'
        AND activation.promotion_id = expected_promotion_id
        AND activation.state = 'approved'
        AND activation.product_revision = expected_product_revision
        AND activation.approval_payload_digest = expected_payload_digest;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'atomic product apply claim compare-and-swap failed'
            USING ERRCODE = '40001';
    END IF;
    PERFORM pg_catalog.set_config(
        'starring.product_approval_context_digest',
        '',
        TRUE
    );

    IF NOT (locked_projection ->> 'target_is_active')::BOOLEAN THEN
        IF locked_projection -> 'active' = 'null'::JSONB THEN
            INSERT INTO public.automation_ruleset_activations (
                guild_id,
                ruleset_key,
                active_version
            ) VALUES (
                expected_guild_id,
                locked_projection #>> '{server,target,ruleset_key}',
                (locked_projection #>> '{server,target,version}')::BIGINT
            )
            ON CONFLICT ON CONSTRAINT automation_ruleset_activations_pkey
            DO NOTHING;
            GET DIAGNOSTICS pointer_rows = ROW_COUNT;
        ELSE
            UPDATE public.automation_ruleset_activations AS active
            SET active_version = (locked_projection #>> '{server,target,version}')::BIGINT
            WHERE active.guild_id = expected_guild_id
                AND active.ruleset_key
                    = locked_projection #>> '{server,target,ruleset_key}'
                AND active.active_version
                    = (locked_projection #>> '{active,version}')::BIGINT;
            GET DIAGNOSTICS pointer_rows = ROW_COUNT;
        END IF;
        IF pointer_rows <> 1 THEN
            RAISE EXCEPTION 'atomic product apply active-pointer compare-and-swap failed'
                USING ERRCODE = '40001';
        END IF;
        applied_completion_kind := 'activated';
    ELSE
        applied_completion_kind := 'already_active';
    END IF;

    mutation_clock := pg_catalog.clock_timestamp();
    next_revision := expected_product_revision + 2;
    UPDATE public.activation_requests AS activation
    SET state = 'applied',
        apply_attempt_id = NULL,
        apply_lease_until = NULL,
        last_apply_error = NULL,
        applied_at = mutation_clock,
        applied_by = expected_acting_user_id,
        completion_kind = applied_completion_kind,
        activation_notices = prepared_activation_notices,
        product_revision = next_revision
    WHERE activation.tenant_id = expected_tenant_id
        AND activation.installation_id = expected_installation_id
        AND activation.id = locked_projection #>> '{server,activation,request_id}'
        AND activation.state = 'applying'
        AND activation.apply_attempt_id = new_apply_attempt_id
        AND activation.product_revision = expected_product_revision + 1;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'atomic product apply completion compare-and-swap failed'
            USING ERRCODE = '40001';
    END IF;

    PERFORM pg_catalog.set_config(
        'starring.runtime_mutation_clock',
        requested_at::TEXT,
        TRUE
    );
    INSERT INTO public.runtime_deployments (
        deployment_id,
        tenant_id,
        installation_id,
        promotion_id,
        activation_request_id,
        installation_authority_revision,
        guild_id,
        ruleset_key,
        target_version,
        target_content_hash,
        binding_revision,
        binding_fingerprint,
        policy_revision,
        desired_target_digest,
        desired_target_digest_version,
        runtime_generation,
        previous_runtime,
        requested_at,
        snapshot_format_version,
        snapshot,
        revision,
        phase,
        controller_id,
        controller_fencing_token,
        controller_acquired_at,
        controller_lease_expires_at,
        last_fencing_token,
        next_retry_at,
        last_stable_error_code,
        live_attestation_id,
        live_at,
        blocked_at,
        superseded_at,
        cancelled_at,
        created_at,
        updated_at
    ) VALUES (
        new_deployment_id,
        expected_tenant_id,
        expected_installation_id,
        expected_promotion_id,
        locked_projection #>> '{server,activation,request_id}',
        expected_authority_revision,
        expected_guild_id,
        locked_projection #>> '{server,target,ruleset_key}',
        (locked_projection #>> '{server,target,version}')::BIGINT,
        locked_projection #>> '{server,target,content_hash}',
        (locked_projection #>> '{server,authority,binding_revision}')::BIGINT,
        locked_projection #>> '{server,authority,binding_fingerprint}',
        (locked_projection #>> '{server,authority,policy_revision}')::BIGINT,
        prepared_desired_target_digest,
        1,
        (locked_projection ->> 'runtime_generation')::BIGINT,
        CASE
            WHEN prepared_previous_runtime = 'null'::JSONB THEN NULL
            ELSE prepared_previous_runtime
        END,
        requested_at,
        1,
        prepared_snapshot,
        1,
        'requested',
        NULL,
        NULL,
        NULL,
        NULL,
        NULL,
        NULL,
        NULL,
        NULL,
        NULL,
        NULL,
        NULL,
        NULL,
        requested_at,
        requested_at
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_mutation_clock',
        '',
        TRUE
    );

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
        'product_apply_v1',
        active_idempotency_key_digest,
        idempotency_digest_key_id,
        idempotency_digest_key_fingerprint_candidates[1],
        semantic_request_digest,
        'authoring_promotion',
        expected_promotion_id,
        next_revision,
        'applied',
        'runtime_requested',
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
        'product_apply_v1',
        idempotency_key_digest_candidates[candidate.ordinal],
        idempotency_digest_key_id_candidates[candidate.ordinal],
        idempotency_digest_key_fingerprint_candidates[candidate.ordinal],
        new_receipt_id,
        mutation_clock
    FROM pg_catalog.generate_subscripts(
        idempotency_key_digest_candidates,
        1
    ) AS candidate(ordinal);

    active_baseline_version := NULLIF(
        locked_projection #>> '{server,activation,observed_active_version}',
        ''
    )::BIGINT;
    active_baseline_hash := NULLIF(
        locked_projection #>> '{server,activation,observed_active_hash}',
        ''
    );
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
        'promotion.apply',
        'authoring_promotion',
        expected_promotion_id,
        product_request_id,
        new_receipt_id,
        expected_authority_observation_digest,
        expected_effective_permission_bits::NUMERIC,
        expected_authority_observed_at,
        expected_authority_revision,
        expected_payload_digest,
        locked_projection #>> '{server,authority,binding_fingerprint}',
        (locked_projection #>> '{server,authority,policy_revision}')::BIGINT,
        active_baseline_version,
        active_baseline_hash,
        'applied',
        'runtime_requested',
        '{}'::JSONB,
        mutation_clock
    );

    PERFORM pg_catalog.set_config(
        'starring.product_apply_lock_token_v1',
        '',
        TRUE
    );
    RETURN QUERY SELECT 'ok', next_revision, 'applied', FALSE,
        expected_guild_id, new_deployment_id, prepared_desired_target_digest;
END;
$function$;

REVOKE ALL ON FUNCTION public.starring_product_apply_finalize_v1(
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
    TEXT,
    TEXT,
    JSONB,
    TEXT,
    JSONB,
    JSONB,
    JSONB
) FROM PUBLIC;

CREATE FUNCTION public.assert_atomic_product_apply_runtime_request()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    exact_deployment_count BIGINT;
BEGIN
    IF NEW.authority_kind = 'product_authoring' AND NEW.state = 'applied' THEN
        IF TG_OP = 'UPDATE' AND OLD.state = 'applied' THEN
            RETURN NULL;
        END IF;
        SELECT pg_catalog.count(*)
        INTO exact_deployment_count
        FROM public.runtime_deployments AS deployment
        INNER JOIN public.automation_installation_authority_versions AS authority
            ON authority.tenant_id = deployment.tenant_id
            AND authority.installation_id = deployment.installation_id
            AND authority.revision = deployment.installation_authority_revision
        INNER JOIN public.authoring_promotions AS promotion
            ON promotion.id = deployment.promotion_id
            AND promotion.tenant_id = deployment.tenant_id
            AND promotion.installation_id = deployment.installation_id
        INNER JOIN public.automation_ruleset_activations AS active
            ON active.guild_id = deployment.guild_id
            AND active.ruleset_key = deployment.ruleset_key
            AND active.active_version = deployment.target_version
        WHERE deployment.tenant_id = NEW.tenant_id
            AND deployment.installation_id = NEW.installation_id
            AND deployment.promotion_id = NEW.promotion_id
            AND deployment.activation_request_id = NEW.id
            AND deployment.guild_id = NEW.guild_id
            AND deployment.ruleset_key = NEW.ruleset_key
            AND deployment.target_version = NEW.target_version
            AND deployment.target_content_hash = NEW.target_content_hash
            AND deployment.binding_revision
                = (NEW.approval_context #>> '{context,binding,revision}')::BIGINT
            AND authority.binding_revision = deployment.binding_revision
            AND authority.binding_fingerprint = deployment.binding_fingerprint
            AND authority.policy_revision = deployment.policy_revision
            AND promotion.record #>> '{intent,evidence,context_fingerprint}'
                = authority.binding_fingerprint
            AND promotion.record #> '{stage,activation,approval_context}'
                = NEW.approval_context -> 'context'
            AND deployment.policy_revision
                = (NEW.approval_context #>> '{context,policy,revision}')::BIGINT
            AND deployment.desired_target_digest_version = 1
            AND deployment.desired_target_digest
                = public.starring_runtime_desired_target_digest_v1(
                    deployment.snapshot,
                    deployment.installation_authority_revision
                );
        IF exact_deployment_count <> 1 THEN
            RAISE EXCEPTION 'Applied product activation requires one exact active runtime deployment'
                USING ERRCODE = '23514',
                    CONSTRAINT = 'atomic_product_apply_runtime_request_exact';
        END IF;
    END IF;
    RETURN NULL;
END;
$function$;

CREATE CONSTRAINT TRIGGER activation_requests_assert_atomic_runtime_request
AFTER INSERT OR UPDATE ON public.activation_requests
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION public.assert_atomic_product_apply_runtime_request();

REVOKE ALL ON FUNCTION public.assert_atomic_product_apply_runtime_request()
FROM PUBLIC;
