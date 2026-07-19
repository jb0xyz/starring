CREATE FUNCTION public.starring_canonical_json_v1(document JSONB)
RETURNS TEXT
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
SET search_path = pg_catalog
AS $function$
DECLARE
    encoded TEXT;
BEGIN
    CASE pg_catalog.jsonb_typeof(document)
        WHEN 'object' THEN
            SELECT '{' || COALESCE(
                pg_catalog.string_agg(
                    pg_catalog.to_jsonb(entry.key)::TEXT || ':'
                        || public.starring_canonical_json_v1(entry.value),
                    ',' ORDER BY entry.key COLLATE pg_catalog."C"
                ),
                ''
            ) || '}'
            INTO encoded
            FROM pg_catalog.jsonb_each(document) AS entry(key, value);
        WHEN 'array' THEN
            SELECT '[' || COALESCE(
                pg_catalog.string_agg(
                    public.starring_canonical_json_v1(entry.value),
                    ',' ORDER BY entry.ordinality
                ),
                ''
            ) || ']'
            INTO encoded
            FROM pg_catalog.jsonb_array_elements(document)
                WITH ORDINALITY AS entry(value, ordinality);
        WHEN 'string', 'number', 'boolean', 'null' THEN
            encoded := document::TEXT;
        ELSE
            RAISE EXCEPTION 'unsupported RuleSet JSON value'
                USING ERRCODE = '22023';
    END CASE;
    RETURN encoded;
END;
$function$;

CREATE FUNCTION public.starring_ruleset_content_hash_v1(
    schema_version BIGINT,
    definition JSONB
)
RETURNS TEXT
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
SET search_path = pg_catalog
AS $function$
DECLARE
    canonical_definition TEXT;
BEGIN
    IF schema_version NOT BETWEEN 1 AND 4294967295
        OR pg_catalog.jsonb_typeof(definition) <> 'object'
        OR pg_catalog.octet_length(definition::TEXT) > 524288
    THEN
        RETURN NULL;
    END IF;
    canonical_definition := public.starring_canonical_json_v1(definition);
    RETURN pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                '{"definition":' || canonical_definition
                    || ',"schema_version":' || schema_version::TEXT || '}',
                'UTF8'
            )
        ),
        'hex'
    );
END;
$function$;

ALTER TABLE public.automation_ruleset_versions
ADD COLUMN canonical_content_hash TEXT
GENERATED ALWAYS AS (
    public.starring_ruleset_content_hash_v1(schema_version, definition)
) STORED;

ALTER TABLE public.automation_ruleset_versions
ADD CONSTRAINT arv_content_integrity CHECK (
    canonical_content_hash IS NOT NULL
    AND canonical_content_hash = content_hash
) NOT VALID;

ALTER TABLE public.automation_ruleset_versions
VALIDATE CONSTRAINT arv_content_integrity;

DO $block$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM public.activation_requests AS activation
        LEFT JOIN public.automation_ruleset_versions AS version
            ON version.guild_id = activation.guild_id
            AND version.ruleset_key = activation.ruleset_key
            AND version.version = activation.target_version
        WHERE version.guild_id IS NULL
            OR version.content_hash IS DISTINCT FROM activation.target_content_hash
            OR version.canonical_content_hash IS DISTINCT FROM activation.target_content_hash
    ) OR EXISTS (
        SELECT 1
        FROM public.runtime_deployments AS deployment
        LEFT JOIN public.automation_ruleset_versions AS version
            ON version.guild_id = deployment.guild_id
            AND version.ruleset_key = deployment.ruleset_key
            AND version.version = deployment.target_version
        WHERE version.guild_id IS NULL
            OR version.content_hash IS DISTINCT FROM deployment.target_content_hash
            OR version.canonical_content_hash IS DISTINCT FROM deployment.target_content_hash
    ) OR EXISTS (
        SELECT 1
        FROM public.runtime_attestations AS attestation
        LEFT JOIN public.automation_ruleset_versions AS version
            ON version.guild_id = attestation.guild_id
            AND version.ruleset_key = attestation.ruleset_key
            AND version.version = attestation.target_version
        WHERE version.guild_id IS NULL
            OR version.content_hash IS DISTINCT FROM attestation.target_content_hash
            OR version.canonical_content_hash IS DISTINCT FROM attestation.target_content_hash
    ) OR EXISTS (
        SELECT 1
        FROM public.runtime_serving_leases AS serving
        LEFT JOIN public.automation_ruleset_versions AS version
            ON version.guild_id = serving.guild_id
            AND version.ruleset_key = serving.ruleset_key
            AND version.version = serving.target_version
        WHERE version.guild_id IS NULL
            OR version.content_hash IS DISTINCT FROM serving.target_content_hash
            OR version.canonical_content_hash IS DISTINCT FROM serving.target_content_hash
    ) THEN
        RAISE EXCEPTION 'RuleSet shadow target integrity preflight failed'
            USING ERRCODE = '23514',
                CONSTRAINT = 'ruleset_shadow_target_integrity';
    END IF;
END;
$block$;

CREATE FUNCTION public.reject_ruleset_artifact_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    RAISE EXCEPTION 'published RuleSet artifacts are immutable'
        USING ERRCODE = '55000';
END;
$function$;

CREATE TRIGGER automation_ruleset_versions_reject_mutation
BEFORE UPDATE OR DELETE ON public.automation_ruleset_versions
FOR EACH STATEMENT
EXECUTE FUNCTION public.reject_ruleset_artifact_mutation();

CREATE TRIGGER automation_ruleset_versions_reject_truncate
BEFORE TRUNCATE ON public.automation_ruleset_versions
FOR EACH STATEMENT
EXECUTE FUNCTION public.reject_ruleset_artifact_mutation();

CREATE FUNCTION public.guard_product_ruleset_artifact_transition()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    artifact_is_valid BOOLEAN;
BEGIN
    IF NEW.authority_kind = 'product_authoring'
        AND NEW.link_state_name = 'linked'
        AND NEW.state IS DISTINCT FROM OLD.state
        AND NEW.state IN ('applying', 'applied', 'superseded')
    THEN
        SELECT version.canonical_content_hash IS NOT NULL
            AND version.canonical_content_hash = version.content_hash
            AND version.content_hash = NEW.target_content_hash
        INTO artifact_is_valid
        FROM public.automation_ruleset_versions AS version
        WHERE version.guild_id = NEW.guild_id
            AND version.ruleset_key = NEW.ruleset_key
            AND version.version = NEW.target_version
        FOR SHARE;
        IF NOT COALESCE(artifact_is_valid, FALSE) THEN
            RAISE EXCEPTION 'product RuleSet artifact integrity failed'
                USING ERRCODE = 'PZ012';
        END IF;
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER activation_requests_guard_ruleset_artifact_transition
BEFORE UPDATE ON public.activation_requests
FOR EACH ROW
EXECUTE FUNCTION public.guard_product_ruleset_artifact_transition();

CREATE FUNCTION public.guard_runtime_ruleset_artifact_transition()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    artifact_is_valid BOOLEAN;
BEGIN
    SELECT version.canonical_content_hash IS NOT NULL
        AND version.canonical_content_hash = version.content_hash
        AND version.content_hash = NEW.target_content_hash
    INTO artifact_is_valid
    FROM public.automation_ruleset_versions AS version
    WHERE version.guild_id = NEW.guild_id
        AND version.ruleset_key = NEW.ruleset_key
        AND version.version = NEW.target_version
    FOR SHARE;
    IF NOT COALESCE(artifact_is_valid, FALSE) THEN
        RAISE EXCEPTION 'runtime RuleSet artifact integrity failed'
            USING ERRCODE = 'PZ013';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER runtime_deployments_guard_ruleset_artifact_transition
BEFORE INSERT OR UPDATE ON public.runtime_deployments
FOR EACH ROW
EXECUTE FUNCTION public.guard_runtime_ruleset_artifact_transition();

REVOKE ALL ON FUNCTION public.reject_ruleset_artifact_mutation() FROM PUBLIC;
REVOKE ALL ON FUNCTION public.guard_product_ruleset_artifact_transition() FROM PUBLIC;
REVOKE ALL ON FUNCTION public.guard_runtime_ruleset_artifact_transition() FROM PUBLIC;
REVOKE ALL ON FUNCTION public.starring_canonical_json_v1(JSONB) FROM PUBLIC;
REVOKE ALL ON FUNCTION public.starring_ruleset_content_hash_v1(BIGINT, JSONB) FROM PUBLIC;
