LOCK TABLE public.automation_installations,
    public.activation_requests,
    public.automation_ruleset_activations,
    public.runtime_deployments,
    public.automation_ruleset_versions
IN SHARE ROW EXCLUSIVE MODE;

CREATE FUNCTION public.starring_product_ruleset_slot_exact_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_guild_id TEXT,
    expected_ruleset_key TEXT,
    expected_active_version BIGINT
)
RETURNS BOOLEAN
LANGUAGE sql
STABLE
STRICT
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
    SELECT pg_catalog.count(*) = 1
    FROM public.runtime_deployments AS deployment
    INNER JOIN public.activation_requests AS activation
        ON activation.id = deployment.activation_request_id
    INNER JOIN public.automation_ruleset_versions AS version
        ON version.guild_id = deployment.guild_id
        AND version.ruleset_key = deployment.ruleset_key
        AND version.version = deployment.target_version
    WHERE deployment.tenant_id = expected_tenant_id
        AND deployment.installation_id = expected_installation_id
        AND deployment.guild_id = expected_guild_id
        AND deployment.ruleset_key = expected_ruleset_key
        AND deployment.target_version = expected_active_version
        AND deployment.runtime_generation = (
            SELECT pg_catalog.max(candidate.runtime_generation)
            FROM public.runtime_deployments AS candidate
            WHERE candidate.tenant_id = expected_tenant_id
                AND candidate.installation_id = expected_installation_id
                AND candidate.guild_id = expected_guild_id
                AND candidate.ruleset_key = expected_ruleset_key
        )
        AND activation.authority_kind = 'product_authoring'
        AND activation.link_state_name = 'linked'
        AND activation.state = 'applied'
        AND activation.tenant_id = deployment.tenant_id
        AND activation.installation_id = deployment.installation_id
        AND activation.promotion_id = deployment.promotion_id
        AND activation.id = deployment.activation_request_id
        AND activation.guild_id = deployment.guild_id
        AND activation.ruleset_key = deployment.ruleset_key
        AND activation.target_version = deployment.target_version
        AND activation.target_content_hash = deployment.target_content_hash
        AND version.content_hash = deployment.target_content_hash
        AND version.canonical_content_hash = deployment.target_content_hash;
$function$;

REVOKE ALL ON FUNCTION public.starring_product_ruleset_slot_exact_v1(
    TEXT,
    TEXT,
    TEXT,
    TEXT,
    BIGINT
) FROM PUBLIC;

DO $migration$
DECLARE
    invalid_identity TEXT;
BEGIN
    SELECT activation.id
    INTO invalid_identity
    FROM public.activation_requests AS activation
    WHERE activation.authority_kind = 'product_authoring'
        AND activation.state = 'applying'
    ORDER BY activation.id
    LIMIT 1;
    IF FOUND THEN
        RAISE EXCEPTION 'Committed product activation Applying residue requires reconciliation: %',
            invalid_identity
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_activation_applying_residue_absent';
    END IF;

    SELECT activation.id
    INTO invalid_identity
    FROM public.activation_requests AS activation
    INNER JOIN public.automation_installations AS installation
        ON installation.discord_guild_id = activation.guild_id
        AND installation.ruleset_key = activation.ruleset_key
    WHERE activation.authority_kind = 'legacy_manual'
        AND activation.state = 'applying'
    ORDER BY activation.id
    LIMIT 1;
    IF FOUND THEN
        RAISE EXCEPTION 'Product RuleSet slot has an in-flight legacy activation: %',
            invalid_identity
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_ruleset_slot_legacy_apply_absent';
    END IF;

    SELECT installation.installation_id
    INTO invalid_identity
    FROM public.automation_installations AS installation
    INNER JOIN public.automation_ruleset_activations AS active
        ON active.guild_id = installation.discord_guild_id
        AND active.ruleset_key = installation.ruleset_key
    WHERE NOT public.starring_product_ruleset_slot_exact_v1(
        installation.tenant_id,
        installation.installation_id,
        installation.discord_guild_id,
        installation.ruleset_key,
        active.active_version
    )
    ORDER BY installation.installation_id
    LIMIT 1;
    IF FOUND THEN
        RAISE EXCEPTION 'Product RuleSet slot pointer lacks exact latest deployment lineage: %',
            invalid_identity
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_ruleset_slot_pointer_exact';
    END IF;
END;
$migration$;

CREATE FUNCTION public.assert_no_committed_product_activation_applying()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM public.activation_requests AS activation
        WHERE activation.id = NEW.id
            AND activation.authority_kind = 'product_authoring'
            AND activation.state = 'applying'
    ) THEN
        RAISE EXCEPTION 'Product activation cannot remain Applying at commit'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_activation_applying_residue_absent';
    END IF;
    RETURN NULL;
END;
$function$;

CREATE CONSTRAINT TRIGGER activation_requests_assert_no_product_applying
AFTER INSERT OR UPDATE ON public.activation_requests
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
WHEN (NEW.authority_kind = 'product_authoring' AND NEW.state = 'applying')
EXECUTE FUNCTION public.assert_no_committed_product_activation_applying();

REVOKE ALL ON FUNCTION public.assert_no_committed_product_activation_applying()
FROM PUBLIC;

CREATE FUNCTION public.guard_product_activation_applied_record()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    IF OLD.authority_kind = 'product_authoring'
        AND OLD.state = 'applied'
        AND NEW IS DISTINCT FROM OLD
    THEN
        RAISE EXCEPTION 'Applied product activation record is immutable'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_activation_applied_record_immutable';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER activation_requests_guard_product_applied_record
BEFORE UPDATE ON public.activation_requests
FOR EACH ROW
EXECUTE FUNCTION public.guard_product_activation_applied_record();

REVOKE ALL ON FUNCTION public.guard_product_activation_applied_record()
FROM PUBLIC;

CREATE FUNCTION public.guard_legacy_activation_product_slot()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    IF NEW.authority_kind = 'legacy_manual'
        AND NEW.state = 'applying'
    THEN
        PERFORM pg_catalog.pg_advisory_xact_lock(
            pg_catalog.hashtextextended(
                'starring.ruleset-slot.v1:' || NEW.guild_id || ':' || NEW.ruleset_key,
                0
            )
        );
        IF EXISTS (
            SELECT 1
            FROM public.automation_installations AS installation
            WHERE installation.discord_guild_id = NEW.guild_id
                AND installation.ruleset_key = NEW.ruleset_key
        ) THEN
            RAISE EXCEPTION 'Legacy activation cannot enter Applying for a product-managed RuleSet slot'
                USING ERRCODE = '23514',
                    CONSTRAINT = 'product_ruleset_slot_legacy_apply_forbidden';
        END IF;
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER activation_requests_guard_legacy_product_slot
BEFORE INSERT OR UPDATE ON public.activation_requests
FOR EACH ROW
EXECUTE FUNCTION public.guard_legacy_activation_product_slot();

REVOKE ALL ON FUNCTION public.guard_legacy_activation_product_slot()
FROM PUBLIC;

CREATE FUNCTION public.lock_product_ruleset_slot_takeover()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            'starring.ruleset-slot.v1:' || NEW.discord_guild_id || ':' || NEW.ruleset_key,
            0
        )
    );
    IF EXISTS (
        SELECT 1
        FROM public.activation_requests AS activation
        WHERE activation.guild_id = NEW.discord_guild_id
            AND activation.ruleset_key = NEW.ruleset_key
            AND activation.authority_kind = 'legacy_manual'
            AND activation.state = 'applying'
    ) THEN
        RAISE EXCEPTION 'Product RuleSet slot takeover cannot cross an in-flight legacy activation'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_ruleset_slot_legacy_apply_absent';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER automation_installations_lock_ruleset_slot_takeover
BEFORE INSERT ON public.automation_installations
FOR EACH ROW
EXECUTE FUNCTION public.lock_product_ruleset_slot_takeover();

REVOKE ALL ON FUNCTION public.lock_product_ruleset_slot_takeover()
FROM PUBLIC;

CREATE FUNCTION public.assert_product_ruleset_slot_takeover()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    active_version BIGINT;
BEGIN
    IF EXISTS (
        SELECT 1
        FROM public.activation_requests AS activation
        WHERE activation.guild_id = NEW.discord_guild_id
            AND activation.ruleset_key = NEW.ruleset_key
            AND activation.authority_kind = 'legacy_manual'
            AND activation.state = 'applying'
    ) THEN
        RAISE EXCEPTION 'Product RuleSet slot takeover cannot cross an in-flight legacy activation'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_ruleset_slot_legacy_apply_absent';
    END IF;
    SELECT active.active_version
    INTO active_version
    FROM public.automation_ruleset_activations AS active
    WHERE active.guild_id = NEW.discord_guild_id
        AND active.ruleset_key = NEW.ruleset_key;
    IF FOUND AND NOT public.starring_product_ruleset_slot_exact_v1(
        NEW.tenant_id,
        NEW.installation_id,
        NEW.discord_guild_id,
        NEW.ruleset_key,
        active_version
    ) THEN
        RAISE EXCEPTION 'Product RuleSet slot takeover requires exact latest deployment lineage'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_ruleset_slot_pointer_exact';
    END IF;
    RETURN NULL;
END;
$function$;

CREATE CONSTRAINT TRIGGER automation_installations_assert_ruleset_slot_takeover
AFTER INSERT ON public.automation_installations
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION public.assert_product_ruleset_slot_takeover();

REVOKE ALL ON FUNCTION public.assert_product_ruleset_slot_takeover()
FROM PUBLIC;

CREATE FUNCTION public.assert_product_ruleset_slot_pointer()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    installation_row public.automation_installations%ROWTYPE;
    active_version BIGINT;
BEGIN
    IF TG_OP = 'DELETE' THEN
        SELECT installation.*
        INTO installation_row
        FROM public.automation_installations AS installation
        WHERE installation.discord_guild_id = OLD.guild_id
            AND installation.ruleset_key = OLD.ruleset_key;
        IF FOUND THEN
            RAISE EXCEPTION 'Product RuleSet slot pointer cannot be deleted'
                USING ERRCODE = '23514',
                    CONSTRAINT = 'product_ruleset_slot_pointer_delete_forbidden';
        END IF;
        RETURN NULL;
    END IF;

    IF TG_OP = 'UPDATE'
        AND (
            NEW.guild_id IS DISTINCT FROM OLD.guild_id
            OR NEW.ruleset_key IS DISTINCT FROM OLD.ruleset_key
        )
        AND EXISTS (
            SELECT 1
            FROM public.automation_installations AS installation
            WHERE installation.discord_guild_id = OLD.guild_id
                AND installation.ruleset_key = OLD.ruleset_key
        )
    THEN
        RAISE EXCEPTION 'Product RuleSet slot pointer identity is immutable'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_ruleset_slot_pointer_identity_immutable';
    END IF;

    SELECT installation.*
    INTO installation_row
    FROM public.automation_installations AS installation
    WHERE installation.discord_guild_id = NEW.guild_id
        AND installation.ruleset_key = NEW.ruleset_key;
    IF NOT FOUND THEN
        RETURN NULL;
    END IF;
    SELECT active.active_version
    INTO active_version
    FROM public.automation_ruleset_activations AS active
    WHERE active.guild_id = NEW.guild_id
        AND active.ruleset_key = NEW.ruleset_key;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Product RuleSet slot pointer requires exact latest deployment lineage'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_ruleset_slot_pointer_exact';
    END IF;
    IF NOT public.starring_product_ruleset_slot_exact_v1(
        installation_row.tenant_id,
        installation_row.installation_id,
        NEW.guild_id,
        NEW.ruleset_key,
        active_version
    ) THEN
        RAISE EXCEPTION 'Product RuleSet slot pointer requires exact latest deployment lineage'
            USING ERRCODE = '23514',
                CONSTRAINT = 'product_ruleset_slot_pointer_exact';
    END IF;
    RETURN NULL;
END;
$function$;

CREATE CONSTRAINT TRIGGER automation_ruleset_activations_assert_product_slot
AFTER INSERT OR UPDATE OR DELETE ON public.automation_ruleset_activations
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION public.assert_product_ruleset_slot_pointer();

REVOKE ALL ON FUNCTION public.assert_product_ruleset_slot_pointer()
FROM PUBLIC;
