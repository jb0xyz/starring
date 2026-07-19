CREATE OR REPLACE FUNCTION public.starring_runtime_lock_current_authority(
    expected_activation_request_id TEXT,
    expected_promotion_id TEXT,
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_installation_authority_revision BIGINT,
    expected_guild_id TEXT,
    expected_ruleset_key TEXT,
    expected_target_version BIGINT,
    expected_target_content_hash TEXT,
    expected_binding_revision BIGINT,
    expected_binding_fingerprint TEXT
)
RETURNS TEXT
LANGUAGE plpgsql
VOLATILE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    activation_row public.activation_requests%ROWTYPE;
    promotion_row public.authoring_promotions%ROWTYPE;
    tenant_row public.product_tenants%ROWTYPE;
    installation_row public.automation_installations%ROWTYPE;
    historical_authority_row public.automation_installation_authority_versions%ROWTYPE;
    current_authority_row public.automation_installation_authority_versions%ROWTYPE;
    active_version BIGINT;
    persisted_content_hash TEXT;
BEGIN
    IF expected_activation_request_id !~ '^[A-Za-z0-9_-]{1,64}$'
        OR expected_promotion_id !~ '^[0-9a-f]{64}$'
        OR expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_authority_revision NOT BETWEEN 1 AND 9223372036854775807
        OR NOT (CASE
            WHEN expected_guild_id ~ '^[1-9][0-9]{0,19}$'
                THEN expected_guild_id::NUMERIC <= 18446744073709551615
            ELSE FALSE
        END)
        OR expected_ruleset_key !~ '^[A-Za-z0-9_-]{1,64}$'
        OR expected_target_version NOT BETWEEN 1 AND 4294967295
        OR expected_target_content_hash !~ '^[0-9a-f]{64}$'
        OR expected_binding_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_binding_fingerprint !~ '^[0-9a-f]{64}$'
    THEN
        RETURN 'scope_mismatch';
    END IF;

    SELECT *
    INTO activation_row
    FROM public.activation_requests
    WHERE id = expected_activation_request_id
    FOR SHARE;
    IF NOT FOUND
        OR activation_row.authority_kind <> 'product_authoring'
        OR activation_row.link_state_name <> 'linked'
        OR activation_row.state <> 'applied'
        OR activation_row.promotion_id IS DISTINCT FROM expected_promotion_id
        OR activation_row.guild_id IS DISTINCT FROM expected_guild_id
        OR activation_row.ruleset_key IS DISTINCT FROM expected_ruleset_key
        OR activation_row.target_version IS DISTINCT FROM expected_target_version
        OR activation_row.target_content_hash IS DISTINCT FROM expected_target_content_hash
    THEN
        RETURN 'scope_mismatch';
    END IF;

    SELECT *
    INTO promotion_row
    FROM public.authoring_promotions
    WHERE id = expected_promotion_id
    FOR SHARE;
    IF NOT FOUND
        OR promotion_row.stage <> 'activation_pending'
        OR promotion_row.tenant_id IS DISTINCT FROM expected_tenant_id
        OR promotion_row.record #>> '{intent,authority,tenant_id}' IS DISTINCT FROM expected_tenant_id
        OR promotion_row.record #>> '{intent,authority,installation_id}' IS DISTINCT FROM expected_installation_id
        OR promotion_row.record #>> '{intent,authority,guild_id}' IS DISTINCT FROM expected_guild_id
        OR promotion_row.record #>> '{intent,authority,ruleset_key}' IS DISTINCT FROM expected_ruleset_key
        OR (promotion_row.record #>> '{intent,authority,binding_revision}')::BIGINT
            IS DISTINCT FROM expected_binding_revision
        OR promotion_row.record #>> '{intent,evidence,context_fingerprint}'
            IS DISTINCT FROM expected_binding_fingerprint
        OR promotion_row.record #>> '{stage,activation,request_id}'
            IS DISTINCT FROM expected_activation_request_id
        OR promotion_row.record #>> '{stage,activation,target,guild_id}'
            IS DISTINCT FROM expected_guild_id
        OR promotion_row.record #>> '{stage,activation,target,ruleset_key}'
            IS DISTINCT FROM expected_ruleset_key
        OR (promotion_row.record #>> '{stage,activation,target,version}')::BIGINT
            IS DISTINCT FROM expected_target_version
        OR promotion_row.record #>> '{stage,activation,target,content_hash}'
            IS DISTINCT FROM expected_target_content_hash
    THEN
        RETURN 'scope_mismatch';
    END IF;

    SELECT *
    INTO tenant_row
    FROM public.product_tenants
    WHERE tenant_id = expected_tenant_id
    FOR SHARE;
    IF NOT FOUND OR tenant_row.lifecycle_state <> 'active' THEN
        RETURN 'lifecycle_inactive';
    END IF;

    SELECT *
    INTO installation_row
    FROM public.automation_installations
    WHERE tenant_id = expected_tenant_id
        AND installation_id = expected_installation_id
    FOR SHARE;
    IF NOT FOUND
        OR installation_row.discord_guild_id IS DISTINCT FROM expected_guild_id
        OR installation_row.ruleset_key IS DISTINCT FROM expected_ruleset_key
    THEN
        RETURN 'scope_mismatch';
    END IF;
    IF installation_row.lifecycle_state <> 'active' THEN
        RETURN 'lifecycle_inactive';
    END IF;

    SELECT *
    INTO historical_authority_row
    FROM public.automation_installation_authority_versions
    WHERE tenant_id = expected_tenant_id
        AND installation_id = expected_installation_id
        AND revision = expected_installation_authority_revision;
    IF NOT FOUND
        OR historical_authority_row.binding_revision IS DISTINCT FROM expected_binding_revision
        OR historical_authority_row.binding_fingerprint IS DISTINCT FROM expected_binding_fingerprint
    THEN
        RETURN 'binding_mismatch';
    END IF;

    SELECT *
    INTO current_authority_row
    FROM public.automation_installation_authority_versions
    WHERE tenant_id = expected_tenant_id
        AND installation_id = expected_installation_id
        AND revision = installation_row.current_authority_revision;
    IF NOT FOUND
        OR current_authority_row.binding_revision IS DISTINCT FROM expected_binding_revision
        OR current_authority_row.binding_fingerprint IS DISTINCT FROM expected_binding_fingerprint
        OR current_authority_row.resource_bindings
            IS DISTINCT FROM historical_authority_row.resource_bindings
    THEN
        RETURN 'binding_mismatch';
    END IF;

    SELECT active.active_version
    INTO active_version
    FROM public.automation_ruleset_activations active
    WHERE active.guild_id = expected_guild_id
        AND active.ruleset_key = expected_ruleset_key
    FOR SHARE;
    IF NOT FOUND OR active_version IS DISTINCT FROM expected_target_version THEN
        RETURN 'active_mismatch';
    END IF;

    SELECT content_hash
    INTO persisted_content_hash
    FROM public.automation_ruleset_versions
    WHERE guild_id = expected_guild_id
        AND ruleset_key = expected_ruleset_key
        AND version = expected_target_version;
    IF NOT FOUND OR persisted_content_hash IS DISTINCT FROM expected_target_content_hash THEN
        RETURN 'active_mismatch';
    END IF;

    RETURN 'exact';
END;
$function$;

REVOKE ALL ON FUNCTION public.starring_runtime_lock_current_authority(
    TEXT,
    TEXT,
    TEXT,
    TEXT,
    BIGINT,
    TEXT,
    TEXT,
    BIGINT,
    TEXT,
    BIGINT,
    TEXT
) FROM PUBLIC;
