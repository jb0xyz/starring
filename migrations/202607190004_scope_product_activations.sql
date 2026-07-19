LOCK TABLE public.authoring_promotions,
    public.automation_installations,
    public.activation_requests,
    public.activation_request_approvals,
    public.runtime_deployments
IN SHARE ROW EXCLUSIVE MODE;

ALTER TABLE public.authoring_promotions
ADD COLUMN installation_id TEXT;

UPDATE public.authoring_promotions AS promotion
SET installation_id = promotion.record #>> '{intent,authority,installation_id}'
WHERE promotion.record #>> '{intent,authority,tenant_id}' = promotion.tenant_id;

DO $block$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM public.authoring_promotions AS promotion
        WHERE promotion.installation_id IS NULL
            OR promotion.installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
            OR promotion.record #>> '{intent,authority,tenant_id}'
                IS DISTINCT FROM promotion.tenant_id
            OR promotion.record #>> '{intent,authority,installation_id}'
                IS DISTINCT FROM promotion.installation_id
    ) THEN
        RAISE EXCEPTION 'authoring promotion scope backfill is incomplete'
            USING ERRCODE = '23514';
    END IF;
END;
$block$;

DO $block$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM public.authoring_promotions AS promotion
        LEFT JOIN public.automation_installations AS installation
            ON installation.tenant_id = promotion.tenant_id
            AND installation.installation_id = promotion.installation_id
        WHERE installation.installation_id IS NULL
            OR installation.discord_guild_id
                IS DISTINCT FROM promotion.record #>> '{intent,authority,guild_id}'
            OR installation.ruleset_key
                IS DISTINCT FROM promotion.record #>> '{intent,authority,ruleset_key}'
    ) THEN
        RAISE EXCEPTION 'authoring promotion control-plane provisioning is incomplete'
            USING ERRCODE = '23514';
    END IF;
END;
$block$;

CREATE FUNCTION public.enforce_authoring_promotion_scope()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $function$
DECLARE
    record_tenant_id TEXT;
    record_installation_id TEXT;
BEGIN
    record_tenant_id := NEW.record #>> '{intent,authority,tenant_id}';
    record_installation_id := NEW.record #>> '{intent,authority,installation_id}';
    IF TG_OP = 'UPDATE'
        AND (
            NEW.id IS DISTINCT FROM OLD.id
            OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id
            OR NEW.installation_id IS DISTINCT FROM OLD.installation_id
        )
    THEN
        RAISE EXCEPTION 'authoring promotion scope is immutable'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.installation_id IS NULL THEN
        NEW.installation_id := record_installation_id;
    END IF;
    IF record_tenant_id IS DISTINCT FROM NEW.tenant_id
        OR record_installation_id IS DISTINCT FROM NEW.installation_id
    THEN
        RAISE EXCEPTION 'authoring promotion scalar scope differs from its record'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER authoring_promotions_enforce_scope
BEFORE INSERT OR UPDATE ON public.authoring_promotions
FOR EACH ROW
EXECUTE FUNCTION public.enforce_authoring_promotion_scope();

ALTER TABLE public.authoring_promotions
ALTER COLUMN installation_id SET NOT NULL,
ADD CONSTRAINT authoring_promotions_installation_id_format CHECK (
    installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
),
ADD CONSTRAINT authoring_promotions_installation_fk FOREIGN KEY (
    tenant_id,
    installation_id
) REFERENCES public.automation_installations (
    tenant_id,
    installation_id
) ON DELETE RESTRICT NOT VALID,
ADD CONSTRAINT authoring_promotions_product_scope_unique UNIQUE (
    tenant_id,
    installation_id,
    id
);

ALTER TABLE public.authoring_promotions
VALIDATE CONSTRAINT authoring_promotions_installation_fk;

ALTER TABLE public.activation_requests
ADD COLUMN tenant_id TEXT,
ADD COLUMN installation_id TEXT,
ADD COLUMN product_revision BIGINT;

UPDATE public.activation_requests AS activation
SET tenant_id = promotion.tenant_id,
    installation_id = promotion.installation_id,
    product_revision = 1
FROM public.authoring_promotions AS promotion
WHERE activation.authority_kind = 'product_authoring'
    AND promotion.id = activation.promotion_id
    AND promotion.record #>> '{intent,authority,tenant_id}' = promotion.tenant_id
    AND promotion.record #>> '{intent,authority,installation_id}' = promotion.installation_id
    AND promotion.record #>> '{intent,authority,guild_id}' = activation.guild_id
    AND promotion.record #>> '{intent,authority,ruleset_key}' = activation.ruleset_key;

DO $block$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM public.activation_requests AS activation
        WHERE (
            activation.authority_kind = 'product_authoring'
            AND (
                activation.tenant_id IS NULL
                OR activation.installation_id IS NULL
                OR activation.product_revision IS NULL
            )
        ) OR (
            activation.authority_kind = 'legacy_manual'
            AND (
                activation.tenant_id IS NOT NULL
                OR activation.installation_id IS NOT NULL
                OR activation.product_revision IS NOT NULL
            )
        )
    ) THEN
        RAISE EXCEPTION 'product activation scope backfill is incomplete'
            USING ERRCODE = '23514';
    END IF;
END;
$block$;

CREATE FUNCTION public.enforce_product_activation_scope()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $function$
DECLARE
    promotion_tenant_id TEXT;
    promotion_installation_id TEXT;
    promotion_guild_id TEXT;
    promotion_ruleset_key TEXT;
BEGIN
    IF TG_OP = 'UPDATE' THEN
        IF NEW.authority_kind IS DISTINCT FROM OLD.authority_kind
            OR NEW.promotion_id IS DISTINCT FROM OLD.promotion_id
            OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id
            OR NEW.installation_id IS DISTINCT FROM OLD.installation_id
        THEN
            RAISE EXCEPTION 'product activation authority scope is immutable'
                USING ERRCODE = '23514';
        END IF;
        IF NEW.authority_kind = 'product_authoring'
            AND (
                NEW.product_revision IS NULL
                OR NEW.product_revision NOT IN (
                    OLD.product_revision,
                    OLD.product_revision + 1
                )
            )
        THEN
            RAISE EXCEPTION 'product activation revision must stay fixed or advance once'
                USING ERRCODE = '23514';
        END IF;
        IF NEW.authority_kind = 'legacy_manual'
            AND NEW.product_revision IS DISTINCT FROM OLD.product_revision
        THEN
            RAISE EXCEPTION 'legacy activation cannot acquire a product revision'
                USING ERRCODE = '23514';
        END IF;
    END IF;

    IF NEW.authority_kind = 'legacy_manual' THEN
        IF NEW.tenant_id IS NOT NULL
            OR NEW.installation_id IS NOT NULL
            OR NEW.product_revision IS NOT NULL
        THEN
            RAISE EXCEPTION 'legacy activation cannot carry product scope'
                USING ERRCODE = '23514';
        END IF;
        RETURN NEW;
    END IF;

    SELECT promotion.tenant_id,
        promotion.installation_id,
        promotion.record #>> '{intent,authority,guild_id}',
        promotion.record #>> '{intent,authority,ruleset_key}'
    INTO promotion_tenant_id,
        promotion_installation_id,
        promotion_guild_id,
        promotion_ruleset_key
    FROM public.authoring_promotions AS promotion
    WHERE promotion.id = NEW.promotion_id
    FOR SHARE;

    IF NOT FOUND
        OR promotion_tenant_id IS NULL
        OR promotion_installation_id IS NULL
        OR promotion_tenant_id IS DISTINCT FROM NEW.tenant_id
            AND NEW.tenant_id IS NOT NULL
        OR promotion_installation_id IS DISTINCT FROM NEW.installation_id
            AND NEW.installation_id IS NOT NULL
        OR promotion_guild_id IS DISTINCT FROM NEW.guild_id
        OR promotion_ruleset_key IS DISTINCT FROM NEW.ruleset_key
        OR TG_OP = 'INSERT'
            AND NEW.product_revision IS NOT NULL
            AND NEW.product_revision <> 1
    THEN
        RAISE EXCEPTION 'product activation scope does not match its promotion'
            USING ERRCODE = '23514';
    END IF;

    NEW.tenant_id := promotion_tenant_id;
    NEW.installation_id := promotion_installation_id;
    IF TG_OP = 'INSERT' THEN
        NEW.product_revision := 1;
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER activation_requests_enforce_product_scope
BEFORE INSERT OR UPDATE ON public.activation_requests
FOR EACH ROW
EXECUTE FUNCTION public.enforce_product_activation_scope();

ALTER TABLE public.activation_requests
ADD CONSTRAINT activation_requests_product_scope_valid CHECK (
    ((
        authority_kind = 'legacy_manual'
        AND tenant_id IS NULL
        AND installation_id IS NULL
        AND product_revision IS NULL
    )
    OR (
        authority_kind = 'product_authoring'
        AND tenant_id IS NOT NULL
        AND installation_id IS NOT NULL
        AND product_revision IS NOT NULL
        AND tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND product_revision BETWEEN 1 AND 9223372036854775807
    )
) IS TRUE) NOT VALID,
ADD CONSTRAINT activation_requests_product_installation_fk FOREIGN KEY (
    tenant_id,
    installation_id
) REFERENCES public.automation_installations (
    tenant_id,
    installation_id
) ON DELETE RESTRICT NOT VALID,
ADD CONSTRAINT activation_requests_product_promotion_scope_fk FOREIGN KEY (
    tenant_id,
    installation_id,
    promotion_id
) REFERENCES public.authoring_promotions (
    tenant_id,
    installation_id,
    id
) ON DELETE RESTRICT NOT VALID,
ADD CONSTRAINT activation_requests_product_scope_identity_unique UNIQUE (
    tenant_id,
    installation_id,
    id
);

ALTER TABLE public.activation_requests
VALIDATE CONSTRAINT activation_requests_product_scope_valid;

ALTER TABLE public.activation_requests
VALIDATE CONSTRAINT activation_requests_product_installation_fk;

ALTER TABLE public.activation_requests
VALIDATE CONSTRAINT activation_requests_product_promotion_scope_fk;

ALTER TABLE public.activation_request_approvals
ADD COLUMN tenant_id TEXT,
ADD COLUMN installation_id TEXT;

UPDATE public.activation_request_approvals AS approval
SET tenant_id = activation.tenant_id,
    installation_id = activation.installation_id
FROM public.activation_requests AS activation
WHERE activation.id = approval.request_id
    AND activation.authority_kind = 'product_authoring';

CREATE FUNCTION public.enforce_activation_approval_scope()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $function$
DECLARE
    parent_authority_kind TEXT;
    parent_tenant_id TEXT;
    parent_installation_id TEXT;
BEGIN
    SELECT activation.authority_kind,
        activation.tenant_id,
        activation.installation_id
    INTO parent_authority_kind,
        parent_tenant_id,
        parent_installation_id
    FROM public.activation_requests AS activation
    WHERE activation.id = NEW.request_id
    FOR KEY SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'activation approval parent is missing'
            USING ERRCODE = '23503';
    END IF;
    IF TG_OP = 'UPDATE'
        AND (
            NEW.request_id IS DISTINCT FROM OLD.request_id
            OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id
            OR NEW.installation_id IS DISTINCT FROM OLD.installation_id
        )
    THEN
        RAISE EXCEPTION 'activation approval scope is immutable'
            USING ERRCODE = '23514';
    END IF;
    IF parent_authority_kind = 'legacy_manual' THEN
        IF NEW.tenant_id IS NOT NULL OR NEW.installation_id IS NOT NULL THEN
            RAISE EXCEPTION 'legacy activation approval cannot carry product scope'
                USING ERRCODE = '23514';
        END IF;
    ELSE
        IF NEW.tenant_id IS NOT NULL AND NEW.tenant_id IS DISTINCT FROM parent_tenant_id
            OR NEW.installation_id IS NOT NULL
                AND NEW.installation_id IS DISTINCT FROM parent_installation_id
        THEN
            RAISE EXCEPTION 'activation approval scope differs from its parent'
                USING ERRCODE = '23514';
        END IF;
        NEW.tenant_id := parent_tenant_id;
        NEW.installation_id := parent_installation_id;
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER activation_request_approvals_enforce_scope
BEFORE INSERT OR UPDATE ON public.activation_request_approvals
FOR EACH ROW
EXECUTE FUNCTION public.enforce_activation_approval_scope();

ALTER TABLE public.activation_request_approvals
ADD CONSTRAINT activation_request_approvals_product_scope_valid CHECK (
    ((
        tenant_id IS NULL
        AND installation_id IS NULL
        AND approval_payload_digest IS NULL
    )
    OR (
        tenant_id IS NOT NULL
        AND installation_id IS NOT NULL
        AND approval_payload_digest IS NOT NULL
        AND tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND approval_payload_digest ~ '^[0-9a-f]{64}$'
    )
) IS TRUE) NOT VALID,
ADD CONSTRAINT activation_request_approvals_product_parent_fk FOREIGN KEY (
    tenant_id,
    installation_id,
    request_id
) REFERENCES public.activation_requests (
    tenant_id,
    installation_id,
    id
) ON DELETE CASCADE NOT VALID;

ALTER TABLE public.activation_request_approvals
VALIDATE CONSTRAINT activation_request_approvals_product_scope_valid;

ALTER TABLE public.activation_request_approvals
VALIDATE CONSTRAINT activation_request_approvals_product_parent_fk;

ALTER TABLE public.runtime_deployments
ADD CONSTRAINT runtime_deployments_activation_scope_fk FOREIGN KEY (
    tenant_id,
    installation_id,
    activation_request_id
) REFERENCES public.activation_requests (
    tenant_id,
    installation_id,
    id
) ON DELETE RESTRICT NOT VALID,
ADD CONSTRAINT runtime_deployments_promotion_scope_fk FOREIGN KEY (
    tenant_id,
    installation_id,
    promotion_id
) REFERENCES public.authoring_promotions (
    tenant_id,
    installation_id,
    id
) ON DELETE RESTRICT NOT VALID;

ALTER TABLE public.runtime_deployments
VALIDATE CONSTRAINT runtime_deployments_activation_scope_fk;

ALTER TABLE public.runtime_deployments
VALIDATE CONSTRAINT runtime_deployments_promotion_scope_fk;

CREATE INDEX activation_requests_product_scope_index
ON public.activation_requests (
    tenant_id,
    installation_id,
    promotion_id,
    product_revision
)
WHERE authority_kind = 'product_authoring';
