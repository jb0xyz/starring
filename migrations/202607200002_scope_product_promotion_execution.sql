DO $scope$
DECLARE
    relation_count BIGINT;
    ordinary_count BIGINT;
    rls_disabled_count BIGINT;
    owner_count BIGINT;
    common_owner OID;
    unsafe_schema_create_count BIGINT;
    unsafe_default_count BIGINT;
    collision_count BIGINT;
    required_trigger_count BIGINT;
BEGIN
    SELECT pg_catalog.count(relation.oid),
        pg_catalog.count(relation.oid) FILTER (WHERE relation.relkind = 'r'),
        pg_catalog.count(relation.oid) FILTER (
            WHERE NOT relation.relrowsecurity AND NOT relation.relforcerowsecurity
        ),
        pg_catalog.count(DISTINCT relation.relowner),
        pg_catalog.min(relation.relowner::BIGINT)::OID
    INTO relation_count,
        ordinary_count,
        rls_disabled_count,
        owner_count,
        common_owner
    FROM (
        VALUES
            (pg_catalog.to_regclass('public.product_control_plane_identity')),
            (pg_catalog.to_regclass('public.product_principals')),
            (pg_catalog.to_regclass('public.product_auth_sessions')),
            (pg_catalog.to_regclass('public.product_tenants')),
            (pg_catalog.to_regclass('public.automation_installations')),
            (pg_catalog.to_regclass('public.automation_installation_authority_versions')),
            (pg_catalog.to_regclass('public.authoring_sessions')),
            (pg_catalog.to_regclass('public.authoring_session_generations')),
            (pg_catalog.to_regclass('public.authoring_promotions')),
            (pg_catalog.to_regclass('public.automation_ruleset_heads')),
            (pg_catalog.to_regclass('public.automation_ruleset_versions')),
            (pg_catalog.to_regclass('public.automation_ruleset_activations')),
            (pg_catalog.to_regclass('public.activation_requests')),
            (pg_catalog.to_regclass('public.product_action_receipts')),
            (pg_catalog.to_regclass('public.product_action_receipt_idempotency_aliases')),
            (pg_catalog.to_regclass('public.product_audit_events')),
            (pg_catalog.to_regclass('public.product_action_receipt_audit_evidence'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid;

    IF relation_count <> 17
        OR ordinary_count <> 17
        OR rls_disabled_count <> 17
        OR owner_count <> 1
        OR common_owner IS NULL
    THEN
        RAISE EXCEPTION 'product promotion relation contract is invalid'
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
        AND privilege.grantee <> (
            SELECT role.oid
            FROM pg_catalog.pg_roles AS role
            WHERE role.rolname = 'pg_database_owner'
        );
    IF unsafe_schema_create_count <> 0 THEN
        RAISE EXCEPTION 'product promotion public schema trust is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO unsafe_default_count
    FROM pg_catalog.pg_default_acl AS defaults
    CROSS JOIN LATERAL pg_catalog.aclexplode(defaults.defaclacl) AS privilege
    WHERE defaults.defaclnamespace IN (
        0,
        pg_catalog.to_regnamespace('public')
    )
        AND privilege.grantee <> defaults.defaclrole;
    IF unsafe_default_count <> 0 THEN
        RAISE EXCEPTION 'product promotion default privileges are invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname = ANY(ARRAY[
            'starring_product_promotion_executor_database_identity_v1',
            'starring_product_promotion_replay_v1',
            'starring_product_promotion_prepare_v1',
            'starring_product_promotion_publish_v1',
            'starring_product_promotion_approval_environment_v1',
            'starring_product_promotion_activation_link_v1',
            'starring_product_promotion_repair_link_v1',
            'starring_product_promotion_keyring_coverage_v1',
            'starring_product_promotion_authorize_current_v1',
            'starring_product_promotion_finalize_receipt_v1',
            'enforce_authoring_promotion_product_admission',
            'enforce_authoring_promotion_product_transition'
        ]::NAME[]);
    IF collision_count <> 0 THEN
        RAISE EXCEPTION 'product promotion function name collision is unsafe'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO required_trigger_count
    FROM pg_catalog.pg_trigger AS trigger_row
    INNER JOIN pg_catalog.pg_class AS relation
        ON relation.oid = trigger_row.tgrelid
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = relation.relnamespace
    WHERE namespace.nspname = 'public'
        AND NOT trigger_row.tgisinternal
        AND trigger_row.tgenabled = 'O'
        AND (relation.relname, trigger_row.tgname) IN (
            ('authoring_promotions', 'authoring_promotions_enforce_scope'),
            ('automation_ruleset_versions', 'automation_ruleset_versions_reject_mutation'),
            ('automation_ruleset_versions', 'automation_ruleset_versions_reject_truncate'),
            ('activation_requests', 'activation_requests_enforce_product_journal_link'),
            ('activation_requests', 'activation_requests_enforce_product_scope'),
            ('activation_requests', 'activation_requests_guard_legacy_product_slot'),
            ('activation_requests', 'activation_requests_guard_ruleset_artifact_transition'),
            ('product_action_receipts', 'product_action_receipts_assert_approval_alias'),
            ('product_action_receipts', 'product_action_receipts_assert_approval_audit'),
            ('product_action_receipts', 'product_action_receipts_reject_mutation'),
            ('product_action_receipt_idempotency_aliases', 'product_action_receipt_idempotency_aliases_enforce_capacity'),
            ('product_action_receipt_idempotency_aliases', 'product_action_receipt_idempotency_aliases_reject_mutation'),
            ('product_audit_events', 'product_audit_events_capture_receipt_evidence'),
            ('product_audit_events', 'product_audit_events_reject_mutation'),
            ('product_action_receipt_audit_evidence', 'product_action_receipt_audit_evidence_reject_mutation')
        );
    IF required_trigger_count <> 15 THEN
        RAISE EXCEPTION 'product promotion trigger prerequisite is invalid'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM public.authoring_promotions AS promotion
        WHERE promotion.record_format_version <> 1
            OR promotion.revision NOT BETWEEN 1 AND 9223372036854775807
            OR NOT (
                (promotion.stage = 'prepared' AND promotion.revision = 1)
                OR (promotion.stage = 'published' AND promotion.revision = 2)
                OR (promotion.stage = 'activation_pending' AND promotion.revision = 3)
                OR (promotion.stage = 'expired' AND promotion.revision IN (3, 4))
            )
            OR promotion.record #>> '{stage,state}' IS DISTINCT FROM promotion.stage
            OR (promotion.record #>> '{revision}')::BIGINT
                IS DISTINCT FROM promotion.revision
    ) THEN
        RAISE EXCEPTION 'product promotion legacy journal is invalid'
            USING ERRCODE = '55000';
    END IF;
END;
$scope$;

LOCK TABLE public.authoring_promotions,
    public.product_action_receipts,
    public.product_action_receipt_idempotency_aliases,
    public.product_audit_events,
    public.product_action_receipt_audit_evidence
IN SHARE ROW EXCLUSIVE MODE;

ALTER TABLE public.authoring_promotions
ADD COLUMN product_admission_format_version SMALLINT,
ADD COLUMN product_admission_digest TEXT,
ADD COLUMN product_admission JSONB,
ADD CONSTRAINT authoring_promotions_product_admission_valid CHECK (
    (
        product_admission_format_version IS NULL
        AND product_admission_digest IS NULL
        AND product_admission IS NULL
    ) OR (
        product_admission_format_version = 1
        AND (product_admission_digest ~ '^[0-9a-f]{64}$') IS TRUE
        AND pg_catalog.jsonb_typeof(product_admission) = 'object'
        AND pg_catalog.octet_length(product_admission::TEXT) <= 32768
        AND product_admission ->> 'format_version' = '1'
        AND pg_catalog.jsonb_typeof(product_admission -> 'payload') = 'object'
        AND (
            product_admission ->> 'admitted_at'
            ~ '^[-0-9]{10,}T[0-9:.]+(Z|[+-][0-9:]+)$'
        ) IS TRUE
    )
);

CREATE FUNCTION public.enforce_authoring_promotion_product_admission()
RETURNS TRIGGER
LANGUAGE plpgsql
STRICT
SECURITY DEFINER
PARALLEL UNSAFE
SET search_path = pg_catalog
AS $function$
DECLARE
    payload JSONB;
    admitted_at TIMESTAMPTZ;
    record_created_at TIMESTAMPTZ;
    observed_at TIMESTAMPTZ;
    expires_at TIMESTAMPTZ;
    permission_numeric NUMERIC;
BEGIN
    IF NEW.product_admission_format_version IS NULL
        AND NEW.product_admission_digest IS NULL
        AND NEW.product_admission IS NULL
    THEN
        RETURN NEW;
    END IF;

    IF NEW.product_admission_format_version IS NULL
        OR NEW.product_admission_digest IS NULL
        OR NEW.product_admission IS NULL
    THEN
        RAISE EXCEPTION 'product promotion admission sidecar is incomplete'
            USING ERRCODE = '23514';
    END IF;

    IF NEW.product_admission_format_version <> 1
        OR (NEW.product_admission_digest ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR pg_catalog.jsonb_typeof(NEW.product_admission) IS DISTINCT FROM 'object'
        OR pg_catalog.octet_length(NEW.product_admission::TEXT) > 32768
        OR NEW.product_admission ->> 'format_version' IS DISTINCT FROM '1'
        OR pg_catalog.jsonb_typeof(NEW.product_admission -> 'payload')
            IS DISTINCT FROM 'object'
        OR (
            NEW.product_admission ->> 'admitted_at'
            ~ '^[-0-9]{10,}T[0-9:.]+(Z|[+-][0-9:]+)$'
        ) IS DISTINCT FROM TRUE
    THEN
        RAISE EXCEPTION 'product promotion admission envelope is malformed'
            USING ERRCODE = '23514';
    END IF;

    payload := NEW.product_admission -> 'payload';
    IF (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(NEW.product_admission) AS key(name)
        ) <> 3
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(NEW.product_admission) AS key(name)
            WHERE key.name NOT IN ('format_version', 'payload', 'admitted_at')
        )
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(payload) AS key(name)
        ) <> 31
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(payload) AS key(name)
            WHERE key.name NOT IN (
                'endpoint_domain',
                'product_request_id',
                'tenant_id',
                'installation_id',
                'principal_id',
                'authoring_session_id',
                'generation',
                'candidate_revision',
                'candidate_hash',
                'promotion_id',
                'promotion_request_digest',
                'session_subject_digest',
                'idempotency_key_digest',
                'idempotency_digest_key_id',
                'idempotency_digest_key_fingerprint',
                'semantic_request_digest',
                'receipt_id',
                'audit_event_id',
                'discord_application_id',
                'guild_id',
                'acting_user_id',
                'capability',
                'authority_revision',
                'authority_payload_digest',
                'authority_observation_digest',
                'authority_observed_at',
                'authority_expires_at',
                'effective_permission_bits',
                'guild_owner',
                'binding_fingerprint',
                'policy_revision'
            )
        )
        OR payload ->> 'endpoint_domain' IS DISTINCT FROM 'product_promote_v1'
        OR payload ->> 'tenant_id' IS DISTINCT FROM NEW.tenant_id
        OR payload ->> 'installation_id' IS DISTINCT FROM NEW.installation_id
        OR payload ->> 'principal_id' IS DISTINCT FROM NEW.principal_id
        OR payload ->> 'promotion_id' IS DISTINCT FROM NEW.id
        OR payload ->> 'promotion_request_digest' IS DISTINCT FROM NEW.request_digest
        OR payload ->> 'authoring_session_id'
            IS DISTINCT FROM NEW.record #>> '{intent,authority,session_id}'
        OR payload ->> 'generation'
            IS DISTINCT FROM NEW.record #>> '{intent,authority,session_generation}'
        OR payload ->> 'candidate_revision'
            IS DISTINCT FROM NEW.record #>> '{intent,evidence,candidate_revision}'
        OR payload ->> 'candidate_hash'
            IS DISTINCT FROM NEW.record #>> '{intent,evidence,candidate_ruleset_hash}'
        OR payload ->> 'guild_id'
            IS DISTINCT FROM NEW.record #>> '{intent,authority,guild_id}'
        OR payload ->> 'acting_user_id'
            IS DISTINCT FROM NEW.record #>> '{intent,authority,requester}'
        OR payload ->> 'binding_fingerprint'
            IS DISTINCT FROM NEW.record #>> '{intent,evidence,context_fingerprint}'
        OR payload ->> 'policy_revision'
            IS DISTINCT FROM NEW.record #>> '{intent,authority,policy,revision}'
        OR payload ->> 'capability' IS DISTINCT FROM 'promote'
        OR (payload ->> 'product_request_id' ~ '^[A-Za-z0-9_.:-]{1,128}$')
            IS DISTINCT FROM TRUE
        OR (payload ->> 'session_subject_digest' ~ '^[0-9a-f]{64}$')
            IS DISTINCT FROM TRUE
        OR (payload ->> 'idempotency_key_digest' ~ '^[0-9a-f]{64}$')
            IS DISTINCT FROM TRUE
        OR (payload ->> 'idempotency_digest_key_id' ~ '^[A-Za-z0-9_.:-]{1,64}$')
            IS DISTINCT FROM TRUE
        OR (payload ->> 'idempotency_digest_key_fingerprint' ~ '^[0-9a-f]{64}$')
            IS DISTINCT FROM TRUE
        OR (payload ->> 'semantic_request_digest' ~ '^[0-9a-f]{64}$')
            IS DISTINCT FROM TRUE
        OR (payload ->> 'receipt_id' ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (payload ->> 'audit_event_id' ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (payload ->> 'discord_application_id' ~ '^[1-9][0-9]{0,19}$')
            IS DISTINCT FROM TRUE
        OR (payload ->> 'guild_id' ~ '^[1-9][0-9]{0,19}$') IS DISTINCT FROM TRUE
        OR (payload ->> 'acting_user_id' ~ '^[1-9][0-9]{0,19}$')
            IS DISTINCT FROM TRUE
        OR (payload ->> 'generation' ~ '^[1-9][0-9]{0,18}$') IS DISTINCT FROM TRUE
        OR (payload ->> 'candidate_revision' ~ '^[1-9][0-9]{0,18}$')
            IS DISTINCT FROM TRUE
        OR (payload ->> 'authority_revision' ~ '^[1-9][0-9]{0,18}$')
            IS DISTINCT FROM TRUE
        OR (payload ->> 'policy_revision' ~ '^[1-9][0-9]{0,18}$')
            IS DISTINCT FROM TRUE
        OR (payload ->> 'authority_payload_digest' ~ '^[0-9a-f]{64}$')
            IS DISTINCT FROM TRUE
        OR (payload ->> 'authority_observation_digest' ~ '^[0-9a-f]{64}$')
            IS DISTINCT FROM TRUE
        OR (
            payload ->> 'authority_observed_at'
            ~ '^[-0-9]{10,}T[0-9:.]+(Z|[+-][0-9:]+)$'
        ) IS DISTINCT FROM TRUE
        OR (
            payload ->> 'authority_expires_at'
            ~ '^[-0-9]{10,}T[0-9:.]+(Z|[+-][0-9:]+)$'
        ) IS DISTINCT FROM TRUE
        OR (payload ->> 'effective_permission_bits' ~ '^(0|[1-9][0-9]{0,19})$')
            IS DISTINCT FROM TRUE
        OR pg_catalog.jsonb_typeof(payload -> 'guild_owner') IS DISTINCT FROM 'boolean'
        OR NEW.product_admission ->> 'admitted_at'
            IS DISTINCT FROM NEW.record ->> 'created_at'
    THEN
        RAISE EXCEPTION 'product promotion admission evidence is inconsistent'
            USING ERRCODE = '23514';
    END IF;

    BEGIN
        admitted_at := (NEW.product_admission ->> 'admitted_at')::TIMESTAMPTZ;
        record_created_at := (NEW.record ->> 'created_at')::TIMESTAMPTZ;
        observed_at := (payload ->> 'authority_observed_at')::TIMESTAMPTZ;
        expires_at := (payload ->> 'authority_expires_at')::TIMESTAMPTZ;
        permission_numeric := (payload ->> 'effective_permission_bits')::NUMERIC;
    EXCEPTION
        WHEN invalid_text_representation
            OR numeric_value_out_of_range
            OR datetime_field_overflow
        THEN
            RAISE EXCEPTION 'product promotion admission evidence is malformed'
                USING ERRCODE = '23514';
    END;

    IF admitted_at < record_created_at
        OR observed_at > admitted_at
        OR observed_at >= expires_at
        OR expires_at > observed_at + INTERVAL '5 seconds'
        OR permission_numeric > 18446744073709551615
        OR NOT (
            (payload ->> 'guild_owner')::BOOLEAN
            OR pg_catalog.mod(permission_numeric, 16) >= 8
            OR pg_catalog.mod(permission_numeric, 64) >= 32
        )
    THEN
        RAISE EXCEPTION 'product promotion admission authority interval is invalid'
            USING ERRCODE = '23514';
    END IF;

    RETURN NEW;
END;
$function$;

CREATE FUNCTION public.starring_product_promotion_publish_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_principal_id TEXT,
    expected_product_session_digest BYTEA,
    expected_acting_user_id TEXT,
    expected_discord_application_id TEXT,
    expected_guild_id TEXT,
    expected_capability TEXT,
    observed_current_authority_revision BIGINT,
    observed_current_authority_payload_digest TEXT,
    authority_observation_digest TEXT,
    authority_observed_at TIMESTAMPTZ,
    authority_expires_at TIMESTAMPTZ,
    effective_permission_bits TEXT,
    guild_owner BOOLEAN,
    expected_promotion_id TEXT,
    expected_promotion_revision BIGINT,
    expected_promotion_request_digest TEXT,
    expected_admission_digest TEXT
)
RETURNS TABLE(
    outcome_code TEXT,
    publication_projection JSONB,
    promotion_record JSONB,
    database_now TIMESTAMPTZ
)
LANGUAGE plpgsql
VOLATILE
STRICT
SECURITY DEFINER
PARALLEL UNSAFE
ROWS 1
SET search_path = pg_catalog
AS $function$
BEGIN
    RETURN QUERY SELECT 'persistence_corrupt',
        NULL::JSONB,
        NULL::JSONB,
        pg_catalog.clock_timestamp();
END;
$function$;

CREATE FUNCTION public.starring_product_promotion_approval_environment_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_principal_id TEXT,
    expected_product_session_digest BYTEA,
    expected_acting_user_id TEXT,
    expected_discord_application_id TEXT,
    expected_guild_id TEXT,
    expected_capability TEXT,
    observed_current_authority_revision BIGINT,
    observed_current_authority_payload_digest TEXT,
    authority_observation_digest TEXT,
    authority_observed_at TIMESTAMPTZ,
    authority_expires_at TIMESTAMPTZ,
    effective_permission_bits TEXT,
    guild_owner BOOLEAN,
    expected_promotion_id TEXT,
    expected_promotion_revision BIGINT,
    expected_promotion_request_digest TEXT,
    expected_admission_digest TEXT
)
RETURNS TABLE(
    outcome_code TEXT,
    historical_binding_revision BIGINT,
    historical_resource_bindings JSONB,
    historical_binding_fingerprint TEXT,
    active_version BIGINT,
    active_content_hash TEXT,
    target_artifact_projection JSONB,
    database_now TIMESTAMPTZ
)
LANGUAGE plpgsql
VOLATILE
STRICT
SECURITY DEFINER
PARALLEL UNSAFE
ROWS 1
SET search_path = pg_catalog
AS $function$
BEGIN
    RETURN QUERY SELECT 'persistence_corrupt',
        NULL::BIGINT,
        NULL::JSONB,
        NULL::TEXT,
        NULL::BIGINT,
        NULL::TEXT,
        NULL::JSONB,
        pg_catalog.clock_timestamp();
END;
$function$;

CREATE FUNCTION public.starring_product_promotion_activation_link_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_principal_id TEXT,
    expected_product_session_digest BYTEA,
    expected_acting_user_id TEXT,
    expected_discord_application_id TEXT,
    expected_guild_id TEXT,
    expected_capability TEXT,
    observed_current_authority_revision BIGINT,
    observed_current_authority_payload_digest TEXT,
    authority_observation_digest TEXT,
    authority_observed_at TIMESTAMPTZ,
    authority_expires_at TIMESTAMPTZ,
    effective_permission_bits TEXT,
    guild_owner BOOLEAN,
    expected_promotion_id TEXT,
    expected_promotion_revision BIGINT,
    expected_promotion_request_digest TEXT,
    expected_admission_digest TEXT,
    activation_proposal JSONB
)
RETURNS TABLE(
    outcome_code TEXT,
    promotion_record JSONB,
    admission_evidence JSONB,
    admission_digest TEXT,
    activation_projection JSONB,
    receipt_projection JSONB,
    audit_evidence_projection JSONB,
    database_now TIMESTAMPTZ
)
LANGUAGE plpgsql
VOLATILE
STRICT
SECURITY DEFINER
PARALLEL UNSAFE
ROWS 1
SET search_path = pg_catalog
AS $function$
BEGIN
    RETURN QUERY SELECT 'persistence_corrupt',
        NULL::JSONB,
        NULL::JSONB,
        NULL::TEXT,
        NULL::JSONB,
        NULL::JSONB,
        NULL::JSONB,
        pg_catalog.clock_timestamp();
END;
$function$;

CREATE FUNCTION public.starring_product_promotion_repair_link_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_principal_id TEXT,
    expected_product_session_digest BYTEA,
    expected_acting_user_id TEXT,
    expected_discord_application_id TEXT,
    expected_guild_id TEXT,
    expected_capability TEXT,
    observed_current_authority_revision BIGINT,
    observed_current_authority_payload_digest TEXT,
    authority_observation_digest TEXT,
    authority_observed_at TIMESTAMPTZ,
    authority_expires_at TIMESTAMPTZ,
    effective_permission_bits TEXT,
    guild_owner BOOLEAN,
    expected_promotion_id TEXT,
    expected_promotion_request_digest TEXT,
    recovery_product_request_id TEXT,
    recovery_session_subject_digest BYTEA,
    recovery_admission_payload JSONB,
    recovery_admission_digest TEXT,
    active_idempotency_key_digest TEXT,
    idempotency_key_digest_candidates TEXT[],
    idempotency_digest_key_id_candidates TEXT[],
    idempotency_digest_key_fingerprint_candidates TEXT[],
    idempotency_digest_key_id TEXT,
    semantic_request_digest TEXT,
    new_receipt_id TEXT,
    new_audit_event_id TEXT
)
RETURNS TABLE(
    outcome_code TEXT,
    promotion_record JSONB,
    admission_evidence JSONB,
    admission_digest TEXT,
    activation_projection JSONB,
    receipt_projection JSONB,
    audit_evidence_projection JSONB,
    database_now TIMESTAMPTZ
)
LANGUAGE plpgsql
VOLATILE
STRICT
SECURITY DEFINER
PARALLEL UNSAFE
ROWS 1
SET search_path = pg_catalog
AS $function$
BEGIN
    RETURN QUERY SELECT 'persistence_corrupt',
        NULL::JSONB,
        NULL::JSONB,
        NULL::TEXT,
        NULL::JSONB,
        NULL::JSONB,
        NULL::JSONB,
        pg_catalog.clock_timestamp();
END;
$function$;

CREATE TRIGGER authoring_promotions_enforce_product_admission
BEFORE INSERT OR UPDATE ON public.authoring_promotions
FOR EACH ROW
EXECUTE FUNCTION public.enforce_authoring_promotion_product_admission();

CREATE FUNCTION public.enforce_authoring_promotion_product_transition()
RETURNS TRIGGER
LANGUAGE plpgsql
STRICT
SECURITY DEFINER
PARALLEL UNSAFE
SET search_path = pg_catalog
AS $function$
DECLARE
    new_created_at TIMESTAMPTZ;
    new_updated_at TIMESTAMPTZ;
    old_updated_at TIMESTAMPTZ;
    publication JSONB;
    activation JSONB;
    target JSONB;
BEGIN
    IF pg_catalog.jsonb_typeof(NEW.record) IS DISTINCT FROM 'object'
        OR pg_catalog.octet_length(NEW.record::TEXT) > 8388608
        OR NEW.record ->> 'id' IS DISTINCT FROM NEW.id
        OR NEW.record ->> 'request_digest' IS DISTINCT FROM NEW.request_digest
        OR (NEW.record ->> 'revision' ~ '^[1-9][0-9]{0,18}$')
            IS DISTINCT FROM TRUE
        OR NEW.record ->> 'revision' IS DISTINCT FROM NEW.revision::TEXT
        OR NEW.record #>> '{stage,state}' IS DISTINCT FROM NEW.stage
        OR pg_catalog.jsonb_typeof(NEW.record -> 'stage') IS DISTINCT FROM 'object'
        OR pg_catalog.jsonb_typeof(NEW.record -> 'intent') IS DISTINCT FROM 'object'
        OR NEW.record #>> '{intent,authority,tenant_id}' IS DISTINCT FROM NEW.tenant_id
        OR NEW.record #>> '{intent,authority,installation_id}'
            IS DISTINCT FROM NEW.installation_id
        OR NEW.record #>> '{intent,authority,principal_id}' IS DISTINCT FROM NEW.principal_id
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(CASE
                WHEN pg_catalog.jsonb_typeof(NEW.record) = 'object' THEN NEW.record
                ELSE '{}'::JSONB
            END) AS key(name)
        ) <> 7
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(CASE
                WHEN pg_catalog.jsonb_typeof(NEW.record) = 'object' THEN NEW.record
                ELSE '{}'::JSONB
            END) AS key(name)
            WHERE key.name NOT IN (
                'id',
                'revision',
                'request_digest',
                'intent',
                'stage',
                'created_at',
                'updated_at'
            )
        )
        OR (
            NEW.record ->> 'created_at'
            ~ '^[-0-9]{10,}T[0-9:.]+(Z|[+-][0-9:]+)$'
        ) IS DISTINCT FROM TRUE
        OR (
            NEW.record ->> 'updated_at'
            ~ '^[-0-9]{10,}T[0-9:.]+(Z|[+-][0-9:]+)$'
        ) IS DISTINCT FROM TRUE
    THEN
        RAISE EXCEPTION 'product promotion record projection is malformed'
            USING ERRCODE = '23514';
    END IF;

    BEGIN
        new_created_at := (NEW.record ->> 'created_at')::TIMESTAMPTZ;
        new_updated_at := (NEW.record ->> 'updated_at')::TIMESTAMPTZ;
        IF TG_OP = 'UPDATE' THEN
            old_updated_at := (OLD.record ->> 'updated_at')::TIMESTAMPTZ;
        END IF;
    EXCEPTION
        WHEN invalid_text_representation OR datetime_field_overflow THEN
            RAISE EXCEPTION 'product promotion record timestamps are malformed'
                USING ERRCODE = '23514';
    END;

    IF new_updated_at < new_created_at THEN
        RAISE EXCEPTION 'product promotion record timestamps are invalid'
            USING ERRCODE = '23514';
    END IF;

    IF NEW.stage <> 'prepared' THEN
        publication := NEW.record #> '{stage,publication}';
        IF pg_catalog.jsonb_typeof(publication) IS DISTINCT FROM 'object'
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(CASE
                    WHEN pg_catalog.jsonb_typeof(publication) = 'object' THEN publication
                    ELSE '{}'::JSONB
                END) AS key(name)
            ) <> 5
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.jsonb_object_keys(CASE
                    WHEN pg_catalog.jsonb_typeof(publication) = 'object' THEN publication
                    ELSE '{}'::JSONB
                END) AS key(name)
                WHERE key.name NOT IN (
                    'version',
                    'schema_version',
                    'content_hash',
                    'disposition',
                    'registry_created_by'
                )
            )
            OR (publication ->> 'version' ~ '^[1-9][0-9]{0,9}$')
                IS DISTINCT FROM TRUE
            OR (publication ->> 'schema_version' ~ '^[1-9][0-9]{0,9}$')
                IS DISTINCT FROM TRUE
            OR (publication ->> 'content_hash' ~ '^[0-9a-f]{64}$')
                IS DISTINCT FROM TRUE
            OR (publication ->> 'disposition' IN ('created', 'reused'))
                IS DISTINCT FROM TRUE
            OR (publication ->> 'registry_created_by' ~ '^[1-9][0-9]{0,19}$')
                IS DISTINCT FROM TRUE
        THEN
            RAISE EXCEPTION 'product promotion publication projection is malformed'
                USING ERRCODE = '23514';
        END IF;
    END IF;

    IF NEW.stage IN ('activation_pending', 'expired') THEN
        activation := NEW.record #> '{stage,activation}';
        target := activation -> 'target';
        IF pg_catalog.jsonb_typeof(activation) IS DISTINCT FROM 'object'
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(CASE
                    WHEN pg_catalog.jsonb_typeof(activation) = 'object' THEN activation
                    ELSE '{}'::JSONB
                END) AS key(name)
            ) <> 10
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.jsonb_object_keys(CASE
                    WHEN pg_catalog.jsonb_typeof(activation) = 'object' THEN activation
                    ELSE '{}'::JSONB
                END) AS key(name)
                WHERE key.name NOT IN (
                    'request_id',
                    'target',
                    'requester',
                    'required_approvals',
                    'observed_active',
                    'created_at',
                    'expires_at',
                    'disposition',
                    'request_state_at_journal',
                    'approval_context'
                )
            )
            OR (activation ->> 'request_id' ~ '^[A-Za-z0-9_.:-]{1,128}$')
                IS DISTINCT FROM TRUE
            OR (activation ->> 'requester' ~ '^[1-9][0-9]{0,19}$')
                IS DISTINCT FROM TRUE
            OR (
                activation ->> 'required_approvals'
                ~ '^([1-9]|[1-5][0-9]|6[0-4])$'
            ) IS DISTINCT FROM TRUE
            OR pg_catalog.jsonb_typeof(activation -> 'observed_active')
                NOT IN ('null', 'object')
            OR (
                activation ->> 'created_at'
                ~ '^[-0-9]{10,}T[0-9:.]+(Z|[+-][0-9:]+)$'
            ) IS DISTINCT FROM TRUE
            OR (
                activation ->> 'expires_at'
                ~ '^[-0-9]{10,}T[0-9:.]+(Z|[+-][0-9:]+)$'
            ) IS DISTINCT FROM TRUE
            OR (activation ->> 'disposition' IN ('created', 'reused'))
                IS DISTINCT FROM TRUE
            OR activation ->> 'request_state_at_journal'
                IS DISTINCT FROM (CASE NEW.stage
                    WHEN 'activation_pending' THEN 'pending'
                    WHEN 'expired' THEN 'expired'
                END)
            OR pg_catalog.jsonb_typeof(activation -> 'approval_context')
                IS DISTINCT FROM 'object'
            OR pg_catalog.jsonb_typeof(target) IS DISTINCT FROM 'object'
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(CASE
                    WHEN pg_catalog.jsonb_typeof(target) = 'object' THEN target
                    ELSE '{}'::JSONB
                END) AS key(name)
            ) <> 4
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.jsonb_object_keys(CASE
                    WHEN pg_catalog.jsonb_typeof(target) = 'object' THEN target
                    ELSE '{}'::JSONB
                END) AS key(name)
                WHERE key.name NOT IN ('guild_id', 'ruleset_key', 'version', 'content_hash')
            )
            OR (target ->> 'guild_id' ~ '^[1-9][0-9]{0,19}$') IS DISTINCT FROM TRUE
            OR (target ->> 'ruleset_key' ~ '^[A-Za-z0-9_-]{1,64}$')
                IS DISTINCT FROM TRUE
            OR (target ->> 'version' ~ '^[1-9][0-9]{0,9}$') IS DISTINCT FROM TRUE
            OR (target ->> 'content_hash' ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        THEN
            RAISE EXCEPTION 'product promotion activation projection is malformed'
                USING ERRCODE = '23514';
        END IF;
    END IF;

    IF TG_OP = 'INSERT' THEN
        IF NEW.stage <> 'prepared'
            OR NEW.revision <> 1
            OR NEW.record #>> '{stage,state}' IS DISTINCT FROM 'prepared'
            OR NEW.record -> 'stage' IS DISTINCT FROM pg_catalog.jsonb_build_object(
                'state',
                'prepared'
            )
            OR new_created_at IS DISTINCT FROM new_updated_at
        THEN
            RAISE EXCEPTION 'product promotion insert must be prepared revision one'
                USING ERRCODE = '23514';
        END IF;
        RETURN NEW;
    END IF;

    IF OLD.stage = 'activation_pending'
        AND OLD.revision = 3
        AND NEW.stage = OLD.stage
        AND NEW.revision = OLD.revision
        AND NEW.id IS NOT DISTINCT FROM OLD.id
        AND NEW.record_format_version IS NOT DISTINCT FROM OLD.record_format_version
        AND NEW.request_digest IS NOT DISTINCT FROM OLD.request_digest
        AND NEW.tenant_id IS NOT DISTINCT FROM OLD.tenant_id
        AND NEW.installation_id IS NOT DISTINCT FROM OLD.installation_id
        AND NEW.principal_id IS NOT DISTINCT FROM OLD.principal_id
        AND NEW.record IS NOT DISTINCT FROM OLD.record
        AND OLD.product_admission_format_version IS NULL
        AND OLD.product_admission_digest IS NULL
        AND OLD.product_admission IS NULL
        AND NEW.product_admission_format_version = 1
        AND NEW.product_admission_digest IS NOT NULL
        AND NEW.product_admission IS NOT NULL
        AND pg_catalog.current_setting(
            'starring.product_promotion_legacy_repair_gate',
            TRUE
        ) = 'starring.product.promotion.legacy.repair.v1'
    THEN
        RETURN NEW;
    END IF;

    IF NEW.id IS DISTINCT FROM OLD.id
        OR NEW.record_format_version IS DISTINCT FROM OLD.record_format_version
        OR NEW.request_digest IS DISTINCT FROM OLD.request_digest
        OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id
        OR NEW.installation_id IS DISTINCT FROM OLD.installation_id
        OR NEW.principal_id IS DISTINCT FROM OLD.principal_id
        OR NEW.record -> 'intent' IS DISTINCT FROM OLD.record -> 'intent'
        OR NEW.record ->> 'created_at' IS DISTINCT FROM OLD.record ->> 'created_at'
        OR NEW.product_admission_format_version
            IS DISTINCT FROM OLD.product_admission_format_version
        OR NEW.product_admission_digest IS DISTINCT FROM OLD.product_admission_digest
        OR NEW.product_admission IS DISTINCT FROM OLD.product_admission
        OR new_updated_at < old_updated_at
    THEN
        RAISE EXCEPTION 'product promotion immutable evidence changed'
            USING ERRCODE = '23514';
    END IF;

    IF OLD.stage = 'prepared'
        AND OLD.revision = 1
        AND NEW.stage = 'published'
        AND NEW.revision = 2
    THEN
        IF NEW.record #>> '{stage,state}' IS DISTINCT FROM 'published'
            OR pg_catalog.jsonb_typeof(NEW.record #> '{stage,publication}')
                IS DISTINCT FROM 'object'
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(CASE
                    WHEN pg_catalog.jsonb_typeof(NEW.record -> 'stage') = 'object'
                        THEN NEW.record -> 'stage'
                    ELSE '{}'::JSONB
                END) AS key(name)
            ) <> 2
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.jsonb_object_keys(CASE
                    WHEN pg_catalog.jsonb_typeof(NEW.record -> 'stage') = 'object'
                        THEN NEW.record -> 'stage'
                    ELSE '{}'::JSONB
                END) AS key(name)
                WHERE key.name NOT IN ('state', 'publication')
            )
        THEN
            RAISE EXCEPTION 'product promotion publication transition is malformed'
                USING ERRCODE = '23514';
        END IF;
        RETURN NEW;
    END IF;

    IF OLD.stage = 'published'
        AND OLD.revision = 2
        AND NEW.stage IN ('activation_pending', 'expired')
        AND NEW.revision = 3
    THEN
        IF NEW.record #>> '{stage,state}' IS DISTINCT FROM NEW.stage
            OR NEW.record #> '{stage,publication}'
                IS DISTINCT FROM OLD.record #> '{stage,publication}'
            OR pg_catalog.jsonb_typeof(NEW.record #> '{stage,activation}')
                IS DISTINCT FROM 'object'
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(CASE
                    WHEN pg_catalog.jsonb_typeof(NEW.record -> 'stage') = 'object'
                        THEN NEW.record -> 'stage'
                    ELSE '{}'::JSONB
                END) AS key(name)
            ) <> 3
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.jsonb_object_keys(CASE
                    WHEN pg_catalog.jsonb_typeof(NEW.record -> 'stage') = 'object'
                        THEN NEW.record -> 'stage'
                    ELSE '{}'::JSONB
                END) AS key(name)
                WHERE key.name NOT IN ('state', 'publication', 'activation')
            )
        THEN
            RAISE EXCEPTION 'product promotion activation transition is malformed'
                USING ERRCODE = '23514';
        END IF;
        RETURN NEW;
    END IF;

    IF OLD.stage = 'activation_pending'
        AND OLD.revision = 3
        AND NEW.stage = 'expired'
        AND NEW.revision = 4
    THEN
        IF NEW.record #>> '{stage,state}' IS DISTINCT FROM 'expired'
            OR NEW.record #> '{stage,publication}'
                IS DISTINCT FROM OLD.record #> '{stage,publication}'
            OR (
                NEW.record #> '{stage,activation}'
                #- '{disposition}'
                #- '{request_state_at_journal}'
            ) IS DISTINCT FROM (
                OLD.record #> '{stage,activation}'
                #- '{disposition}'
                #- '{request_state_at_journal}'
            )
            OR NEW.record #>> '{stage,activation,disposition}' IS DISTINCT FROM 'reused'
            OR NEW.record #>> '{stage,activation,request_state_at_journal}'
                IS DISTINCT FROM 'expired'
            OR pg_catalog.jsonb_typeof(NEW.record #> '{stage,activation}')
                IS DISTINCT FROM 'object'
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(CASE
                    WHEN pg_catalog.jsonb_typeof(NEW.record -> 'stage') = 'object'
                        THEN NEW.record -> 'stage'
                    ELSE '{}'::JSONB
                END) AS key(name)
            ) <> 3
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.jsonb_object_keys(CASE
                    WHEN pg_catalog.jsonb_typeof(NEW.record -> 'stage') = 'object'
                        THEN NEW.record -> 'stage'
                    ELSE '{}'::JSONB
                END) AS key(name)
                WHERE key.name NOT IN ('state', 'publication', 'activation')
            )
        THEN
            RAISE EXCEPTION 'product promotion expiry transition is malformed'
                USING ERRCODE = '23514';
        END IF;
        RETURN NEW;
    END IF;

    RAISE EXCEPTION 'product promotion transition is invalid'
        USING ERRCODE = '23514';
END;
$function$;

CREATE TRIGGER authoring_promotions_enforce_product_transition
BEFORE INSERT OR UPDATE ON public.authoring_promotions
FOR EACH ROW
EXECUTE FUNCTION public.enforce_authoring_promotion_product_transition();

ALTER TABLE public.product_action_receipts
DROP CONSTRAINT product_action_receipts_approval_key_identity_required,
ADD CONSTRAINT product_action_receipts_approval_key_identity_required CHECK (
    endpoint_domain NOT IN (
        'product_approve_v1',
        'product_apply_v1',
        'product_promote_v1'
    ) OR (
        idempotency_digest_key_id IS NOT NULL
        AND idempotency_digest_key_fingerprint IS NOT NULL
    )
);

CREATE INDEX product_action_receipts_promotion_retention_index
ON public.product_action_receipts (completed_at, receipt_id)
WHERE endpoint_domain = 'product_promote_v1';

CREATE INDEX product_action_aliases_promotion_receipt_retention_index
ON public.product_action_receipt_idempotency_aliases (receipt_id)
WHERE endpoint_domain = 'product_promote_v1';

CREATE OR REPLACE FUNCTION public.assert_product_approval_receipt_alias()
RETURNS TRIGGER
LANGUAGE plpgsql
STRICT
SECURITY DEFINER
PARALLEL UNSAFE
SET search_path = pg_catalog
AS $function$
BEGIN
    IF NEW.endpoint_domain IN (
        'product_approve_v1',
        'product_apply_v1',
        'product_promote_v1'
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

CREATE OR REPLACE FUNCTION public.enforce_product_action_receipt_retention()
RETURNS TRIGGER
LANGUAGE plpgsql
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

CREATE OR REPLACE FUNCTION public.starring_purge_product_action_receipts_v1(
    batch_limit INTEGER
)
RETURNS TABLE(
    deleted_receipts INTEGER,
    deleted_aliases INTEGER,
    backlog_remaining BOOLEAN
)
LANGUAGE plpgsql
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
                'product_promote_v1'
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
                    'product_promote_v1'
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
                'product_promote_v1'
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
            'product_promote_v1'
        )
        AND alias.receipt_id = ANY(candidate_receipt_ids);
    GET DIAGNOSTICS alias_count = ROW_COUNT;

    DELETE FROM public.product_action_receipts AS receipt
    WHERE receipt.receipt_id = ANY(candidate_receipt_ids)
        AND receipt.endpoint_domain IN (
            'product_approve_v1',
            'product_apply_v1',
            'product_promote_v1'
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
                'product_promote_v1'
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

CREATE FUNCTION public.starring_product_promotion_authorize_current_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_principal_id TEXT,
    expected_product_session_digest BYTEA,
    expected_acting_user_id TEXT,
    expected_discord_application_id TEXT,
    expected_guild_id TEXT,
    expected_capability TEXT,
    observed_current_authority_revision BIGINT,
    observed_current_authority_payload_digest TEXT,
    authority_observation_digest TEXT,
    authority_observed_at TIMESTAMPTZ,
    authority_expires_at TIMESTAMPTZ,
    effective_permission_bits TEXT,
    guild_owner BOOLEAN
)
RETURNS TABLE(
    outcome_code TEXT,
    database_now TIMESTAMPTZ,
    current_authority_projection JSONB
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
    current_clock TIMESTAMPTZ;
    principal_row public.product_principals%ROWTYPE;
    product_session_row public.product_auth_sessions%ROWTYPE;
    tenant_row public.product_tenants%ROWTYPE;
    installation_row public.automation_installations%ROWTYPE;
    authority_row public.automation_installation_authority_versions%ROWTYPE;
    acting_user_numeric NUMERIC;
    application_numeric NUMERIC;
    guild_numeric NUMERIC;
    permission_numeric NUMERIC;
BEGIN
    current_clock := pg_catalog.clock_timestamp();
    IF expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_principal_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR pg_catalog.octet_length(expected_product_session_digest) <> 32
        OR expected_acting_user_id !~ '^[1-9][0-9]{0,19}$'
        OR expected_discord_application_id !~ '^[1-9][0-9]{0,19}$'
        OR expected_guild_id !~ '^[1-9][0-9]{0,19}$'
        OR expected_capability <> 'promote'
        OR observed_current_authority_revision NOT BETWEEN 1 AND 9223372036854775807
        OR observed_current_authority_payload_digest !~ '^[0-9a-f]{64}$'
        OR authority_observation_digest !~ '^[0-9a-f]{64}$'
        OR authority_observed_at >= authority_expires_at
        OR authority_expires_at > authority_observed_at + INTERVAL '5 seconds'
        OR effective_permission_bits !~ '^(0|[1-9][0-9]{0,19})$'
    THEN
        RETURN QUERY SELECT 'access_denied', current_clock, NULL::JSONB;
        RETURN;
    END IF;

    BEGIN
        acting_user_numeric := expected_acting_user_id::NUMERIC;
        application_numeric := expected_discord_application_id::NUMERIC;
        guild_numeric := expected_guild_id::NUMERIC;
        permission_numeric := effective_permission_bits::NUMERIC;
    EXCEPTION
        WHEN invalid_text_representation OR numeric_value_out_of_range THEN
            RETURN QUERY SELECT 'access_denied', current_clock, NULL::JSONB;
            RETURN;
    END;

    IF acting_user_numeric > 18446744073709551615
        OR application_numeric > 18446744073709551615
        OR guild_numeric > 18446744073709551615
        OR permission_numeric > 18446744073709551615
        OR NOT (
            guild_owner
            OR pg_catalog.mod(permission_numeric, 16) >= 8
            OR pg_catalog.mod(permission_numeric, 64) >= 32
        )
    THEN
        RETURN QUERY SELECT 'access_denied', current_clock, NULL::JSONB;
        RETURN;
    END IF;

    SELECT principal.*
    INTO principal_row
    FROM public.product_principals AS principal
    WHERE principal.principal_id = expected_principal_id
    FOR UPDATE;

    SELECT product_session.*
    INTO product_session_row
    FROM public.product_auth_sessions AS product_session
    WHERE product_session.session_digest = expected_product_session_digest
        AND product_session.principal_id = expected_principal_id
    FOR UPDATE;

    IF principal_row.principal_id IS NULL
        OR product_session_row.principal_id IS NULL
        OR principal_row.disabled
        OR principal_row.discord_user_id IS DISTINCT FROM expected_acting_user_id
        OR product_session_row.oauth_state_digest IS NULL
        OR product_session_row.revoked_at IS NOT NULL
        OR product_session_row.revocation_reason IS NOT NULL
        OR current_clock >= product_session_row.idle_expires_at
        OR current_clock >= product_session_row.absolute_expires_at
    THEN
        RETURN QUERY SELECT 'access_denied', current_clock, NULL::JSONB;
        RETURN;
    END IF;

    SELECT tenant.*
    INTO tenant_row
    FROM public.product_tenants AS tenant
    WHERE tenant.tenant_id = expected_tenant_id
    FOR UPDATE;

    SELECT installation.*
    INTO installation_row
    FROM public.automation_installations AS installation
    WHERE installation.tenant_id = expected_tenant_id
        AND installation.installation_id = expected_installation_id
    FOR UPDATE;

    IF tenant_row.tenant_id IS NULL
        OR installation_row.installation_id IS NULL
        OR tenant_row.lifecycle_state <> 'active'
        OR installation_row.lifecycle_state <> 'active'
    THEN
        RETURN QUERY SELECT 'scope_mismatch', current_clock, NULL::JSONB;
        RETURN;
    END IF;

    SELECT authority.*
    INTO authority_row
    FROM public.automation_installation_authority_versions AS authority
    WHERE authority.tenant_id = expected_tenant_id
        AND authority.installation_id = expected_installation_id
        AND authority.revision = installation_row.current_authority_revision;

    IF installation_row.discord_application_id
            IS DISTINCT FROM expected_discord_application_id
        OR installation_row.discord_guild_id IS DISTINCT FROM expected_guild_id
        OR installation_row.current_authority_revision
            IS DISTINCT FROM observed_current_authority_revision
        OR authority_row.installation_id IS NULL
        OR authority_row.authority_payload_digest
            IS DISTINCT FROM observed_current_authority_payload_digest
        OR authority_observed_at > current_clock
        OR current_clock >= authority_expires_at
    THEN
        RETURN QUERY SELECT 'scope_mismatch', current_clock, NULL::JSONB;
        RETURN;
    END IF;

    RETURN QUERY SELECT
        'authorized',
        current_clock,
        pg_catalog.jsonb_build_object(
            'format_version', 1,
            'tenant_id', expected_tenant_id,
            'installation_id', expected_installation_id,
            'principal_id', expected_principal_id,
            'acting_user_id', expected_acting_user_id,
            'discord_application_id', expected_discord_application_id,
            'guild_id', expected_guild_id,
            'authority_revision', authority_row.revision,
            'authority_payload_digest', authority_row.authority_payload_digest,
            'binding_revision', authority_row.binding_revision,
            'binding_fingerprint', authority_row.binding_fingerprint,
            'policy_revision', authority_row.policy_revision,
            'required_approvals', authority_row.required_approvals,
            'activation_ttl_seconds', authority_row.activation_ttl_seconds,
            'resource_bindings', authority_row.resource_bindings,
            'authority_observation_digest', authority_observation_digest,
            'authority_observed_at', authority_observed_at,
            'authority_expires_at', authority_expires_at,
            'effective_permission_bits', effective_permission_bits,
            'guild_owner', guild_owner
        );
END;
$function$;

CREATE FUNCTION public.starring_product_promotion_finalize_receipt_v1(
    admission_projection JSONB,
    promotion_projection JSONB,
    activation_projection JSONB,
    receipt_projection JSONB,
    audit_projection JSONB
)
RETURNS TABLE(outcome_code TEXT)
LANGUAGE plpgsql
VOLATILE
STRICT
SECURITY DEFINER
PARALLEL UNSAFE
ROWS 1
SET search_path = pg_catalog
AS $function$
BEGIN
    IF pg_catalog.jsonb_typeof(admission_projection) <> 'object'
        OR pg_catalog.jsonb_typeof(promotion_projection) <> 'object'
        OR pg_catalog.jsonb_typeof(activation_projection) <> 'object'
        OR pg_catalog.jsonb_typeof(receipt_projection) <> 'object'
        OR pg_catalog.jsonb_typeof(audit_projection) <> 'object'
    THEN
        RETURN QUERY SELECT 'persistence_corrupt';
        RETURN;
    END IF;
    RETURN QUERY SELECT 'persistence_corrupt';
END;
$function$;

CREATE FUNCTION public.starring_product_promotion_executor_database_identity_v1()
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

CREATE FUNCTION public.starring_product_promotion_keyring_coverage_v1(
    idempotency_digest_key_id_candidates TEXT[],
    idempotency_digest_key_fingerprint_candidates TEXT[]
)
RETURNS TABLE(outcome_code TEXT)
LANGUAGE sql
VOLATILE
STRICT
SECURITY DEFINER
PARALLEL UNSAFE
ROWS 1
SET search_path = pg_catalog
AS $function$
    SELECT CASE
        WHEN pg_catalog.cardinality(idempotency_digest_key_id_candidates)
                IS DISTINCT FROM pg_catalog.cardinality(
                    idempotency_digest_key_fingerprint_candidates
                )
            OR pg_catalog.cardinality(idempotency_digest_key_id_candidates)
                NOT BETWEEN 1 AND 8
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.generate_subscripts(
                    idempotency_digest_key_id_candidates,
                    1
                ) AS candidate(ordinal)
                WHERE idempotency_digest_key_id_candidates[candidate.ordinal]
                        !~ '^[A-Za-z0-9_.:-]{1,64}$'
                    OR idempotency_digest_key_fingerprint_candidates[candidate.ordinal]
                        !~ '^[0-9a-f]{64}$'
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
                    OR idempotency_digest_key_fingerprint_candidates[left_candidate.ordinal]
                        = idempotency_digest_key_fingerprint_candidates[right_candidate.ordinal]
            )
        THEN 'ambiguous_keyring'
        WHEN EXISTS (
            SELECT 1
            FROM public.product_action_receipts AS receipt
            WHERE receipt.endpoint_domain = 'product_promote_v1'
                AND NOT EXISTS (
                    SELECT 1
                    FROM pg_catalog.generate_subscripts(
                        idempotency_digest_key_id_candidates,
                        1
                    ) AS candidate(ordinal)
                    WHERE idempotency_digest_key_id_candidates[candidate.ordinal]
                            = receipt.idempotency_digest_key_id
                        AND idempotency_digest_key_fingerprint_candidates[candidate.ordinal]
                            = receipt.idempotency_digest_key_fingerprint
                )
        ) OR EXISTS (
            SELECT 1
            FROM public.authoring_promotions AS promotion
            WHERE promotion.product_admission IS NOT NULL
                AND promotion.stage IN ('prepared', 'published')
                AND NOT EXISTS (
                    SELECT 1
                    FROM pg_catalog.generate_subscripts(
                        idempotency_digest_key_id_candidates,
                        1
                    ) AS candidate(ordinal)
                    WHERE idempotency_digest_key_id_candidates[candidate.ordinal]
                            = promotion.product_admission
                                #>> '{payload,idempotency_digest_key_id}'
                        AND idempotency_digest_key_fingerprint_candidates[candidate.ordinal]
                            = promotion.product_admission
                                #>> '{payload,idempotency_digest_key_fingerprint}'
                )
        )
        THEN 'missing_key'
        ELSE 'covered'
    END;
$function$;

CREATE FUNCTION public.starring_product_promotion_replay_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_principal_id TEXT,
    expected_product_session_digest BYTEA,
    expected_acting_user_id TEXT,
    expected_discord_application_id TEXT,
    expected_guild_id TEXT,
    expected_capability TEXT,
    observed_current_authority_revision BIGINT,
    observed_current_authority_payload_digest TEXT,
    authority_observation_digest TEXT,
    authority_observed_at TIMESTAMPTZ,
    authority_expires_at TIMESTAMPTZ,
    effective_permission_bits TEXT,
    guild_owner BOOLEAN,
    expected_promotion_id TEXT,
    expected_session_id TEXT,
    expected_generation BIGINT,
    semantic_request_digest TEXT,
    idempotency_key_digest_candidates TEXT[],
    idempotency_digest_key_id_candidates TEXT[],
    idempotency_digest_key_fingerprint_candidates TEXT[]
)
RETURNS TABLE(
    outcome_code TEXT,
    promotion_record JSONB,
    admission_evidence JSONB,
    admission_digest TEXT,
    receipt_projection JSONB,
    audit_evidence_projection JSONB,
    database_now TIMESTAMPTZ
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
    access_result RECORD;
BEGIN
    SELECT *
    INTO access_result
    FROM public.starring_product_promotion_authorize_current_v1(
        expected_tenant_id,
        expected_installation_id,
        expected_principal_id,
        expected_product_session_digest,
        expected_acting_user_id,
        expected_discord_application_id,
        expected_guild_id,
        expected_capability,
        observed_current_authority_revision,
        observed_current_authority_payload_digest,
        authority_observation_digest,
        authority_observed_at,
        authority_expires_at,
        effective_permission_bits,
        guild_owner
    );
    IF access_result.outcome_code <> 'authorized' THEN
        RETURN QUERY SELECT access_result.outcome_code,
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;
    IF expected_promotion_id !~ '^[0-9a-f]{64}$'
        OR expected_session_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_generation NOT BETWEEN 1 AND 9223372036854775807
        OR semantic_request_digest !~ '^[0-9a-f]{64}$'
        OR pg_catalog.cardinality(idempotency_key_digest_candidates)
            IS DISTINCT FROM pg_catalog.cardinality(idempotency_digest_key_id_candidates)
        OR pg_catalog.cardinality(idempotency_key_digest_candidates)
            IS DISTINCT FROM pg_catalog.cardinality(
                idempotency_digest_key_fingerprint_candidates
            )
        OR pg_catalog.cardinality(idempotency_key_digest_candidates) NOT BETWEEN 1 AND 8
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;
    RETURN QUERY SELECT 'persistence_corrupt',
        NULL::JSONB,
        NULL::JSONB,
        NULL::TEXT,
        NULL::JSONB,
        NULL::JSONB,
        access_result.database_now;
END;
$function$;

CREATE FUNCTION public.starring_product_promotion_prepare_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_principal_id TEXT,
    expected_product_session_digest BYTEA,
    expected_acting_user_id TEXT,
    expected_discord_application_id TEXT,
    expected_guild_id TEXT,
    expected_capability TEXT,
    observed_current_authority_revision BIGINT,
    observed_current_authority_payload_digest TEXT,
    authority_observation_digest TEXT,
    authority_observed_at TIMESTAMPTZ,
    authority_expires_at TIMESTAMPTZ,
    effective_permission_bits TEXT,
    guild_owner BOOLEAN,
    product_request_id TEXT,
    session_subject_digest BYTEA,
    expected_session_id TEXT,
    expected_generation BIGINT,
    expected_candidate_revision BIGINT,
    expected_candidate_hash TEXT,
    expected_binding_fingerprint TEXT,
    expected_promotion_id TEXT,
    expected_promotion_request_digest TEXT,
    prepared_promotion_intent JSONB,
    product_admission_payload JSONB,
    product_admission_digest TEXT,
    active_idempotency_key_digest TEXT,
    idempotency_key_digest_candidates TEXT[],
    idempotency_digest_key_id_candidates TEXT[],
    idempotency_digest_key_fingerprint_candidates TEXT[],
    idempotency_digest_key_id TEXT,
    semantic_request_digest TEXT,
    new_receipt_id TEXT,
    new_audit_event_id TEXT
)
RETURNS TABLE(
    outcome_code TEXT,
    promotion_record JSONB,
    admission_evidence JSONB,
    admission_digest TEXT,
    database_now TIMESTAMPTZ
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
    access_result RECORD;
    payload_observed_at TIMESTAMPTZ;
    payload_expires_at TIMESTAMPTZ;
    session_row public.authoring_sessions%ROWTYPE;
    generation_row public.authoring_session_generations%ROWTYPE;
BEGIN
    SELECT *
    INTO access_result
    FROM public.starring_product_promotion_authorize_current_v1(
        expected_tenant_id,
        expected_installation_id,
        expected_principal_id,
        expected_product_session_digest,
        expected_acting_user_id,
        expected_discord_application_id,
        expected_guild_id,
        expected_capability,
        observed_current_authority_revision,
        observed_current_authority_payload_digest,
        authority_observation_digest,
        authority_observed_at,
        authority_expires_at,
        effective_permission_bits,
        guild_owner
    );
    IF access_result.outcome_code <> 'authorized' THEN
        RETURN QUERY SELECT access_result.outcome_code,
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            access_result.database_now;
        RETURN;
    END IF;
    IF product_request_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR pg_catalog.octet_length(session_subject_digest) <> 32
        OR expected_session_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_generation NOT BETWEEN 1 AND 9223372036854775807
        OR expected_candidate_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_candidate_hash !~ '^[0-9a-f]{64}$'
        OR expected_binding_fingerprint !~ '^[0-9a-f]{64}$'
        OR expected_promotion_id !~ '^[0-9a-f]{64}$'
        OR expected_promotion_request_digest !~ '^[0-9a-f]{64}$'
        OR pg_catalog.jsonb_typeof(prepared_promotion_intent) <> 'object'
        OR pg_catalog.octet_length(prepared_promotion_intent::TEXT) > 8388608
        OR pg_catalog.jsonb_typeof(product_admission_payload) <> 'object'
        OR pg_catalog.octet_length(product_admission_payload::TEXT) > 32768
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(CASE
                WHEN pg_catalog.jsonb_typeof(product_admission_payload) = 'object'
                    THEN product_admission_payload
                ELSE '{}'::JSONB
            END) AS key(name)
        ) <> 31
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(CASE
                WHEN pg_catalog.jsonb_typeof(product_admission_payload) = 'object'
                    THEN product_admission_payload
                ELSE '{}'::JSONB
            END) AS key(name)
            WHERE key.name NOT IN (
                'endpoint_domain', 'product_request_id', 'tenant_id',
                'installation_id', 'principal_id', 'authoring_session_id',
                'generation', 'candidate_revision', 'candidate_hash',
                'promotion_id', 'promotion_request_digest', 'session_subject_digest',
                'idempotency_key_digest', 'idempotency_digest_key_id',
                'idempotency_digest_key_fingerprint', 'semantic_request_digest',
                'receipt_id', 'audit_event_id', 'discord_application_id', 'guild_id',
                'acting_user_id', 'capability', 'authority_revision',
                'authority_payload_digest', 'authority_observation_digest',
                'authority_observed_at', 'authority_expires_at',
                'effective_permission_bits', 'guild_owner', 'binding_fingerprint',
                'policy_revision'
            )
        )
        OR product_admission_digest !~ '^[0-9a-f]{64}$'
        OR product_admission_payload ->> 'endpoint_domain'
            IS DISTINCT FROM 'product_promote_v1'
        OR product_admission_payload ->> 'product_request_id'
            IS DISTINCT FROM product_request_id
        OR product_admission_payload ->> 'tenant_id'
            IS DISTINCT FROM expected_tenant_id
        OR product_admission_payload ->> 'installation_id'
            IS DISTINCT FROM expected_installation_id
        OR product_admission_payload ->> 'principal_id'
            IS DISTINCT FROM expected_principal_id
        OR product_admission_payload ->> 'authoring_session_id'
            IS DISTINCT FROM expected_session_id
        OR product_admission_payload ->> 'generation'
            IS DISTINCT FROM expected_generation::TEXT
        OR product_admission_payload ->> 'candidate_revision'
            IS DISTINCT FROM expected_candidate_revision::TEXT
        OR product_admission_payload ->> 'candidate_hash'
            IS DISTINCT FROM expected_candidate_hash
        OR product_admission_payload ->> 'binding_fingerprint'
            IS DISTINCT FROM expected_binding_fingerprint
        OR product_admission_payload ->> 'promotion_id'
            IS DISTINCT FROM expected_promotion_id
        OR product_admission_payload ->> 'promotion_request_digest'
            IS DISTINCT FROM expected_promotion_request_digest
        OR product_admission_payload ->> 'session_subject_digest'
            IS DISTINCT FROM pg_catalog.encode(session_subject_digest, 'hex')
        OR product_admission_payload ->> 'idempotency_key_digest'
            IS DISTINCT FROM active_idempotency_key_digest
        OR product_admission_payload ->> 'idempotency_digest_key_id'
            IS DISTINCT FROM idempotency_digest_key_id
        OR product_admission_payload ->> 'semantic_request_digest'
            IS DISTINCT FROM semantic_request_digest
        OR product_admission_payload ->> 'receipt_id' IS DISTINCT FROM new_receipt_id
        OR product_admission_payload ->> 'audit_event_id'
            IS DISTINCT FROM new_audit_event_id
        OR product_admission_payload ->> 'discord_application_id'
            IS DISTINCT FROM expected_discord_application_id
        OR product_admission_payload ->> 'guild_id' IS DISTINCT FROM expected_guild_id
        OR product_admission_payload ->> 'acting_user_id'
            IS DISTINCT FROM expected_acting_user_id
        OR product_admission_payload ->> 'capability' IS DISTINCT FROM expected_capability
        OR product_admission_payload ->> 'authority_revision'
            IS DISTINCT FROM observed_current_authority_revision::TEXT
        OR product_admission_payload ->> 'authority_payload_digest'
            IS DISTINCT FROM observed_current_authority_payload_digest
        OR product_admission_payload ->> 'authority_observation_digest'
            IS DISTINCT FROM authority_observation_digest
        OR product_admission_payload ->> 'effective_permission_bits'
            IS DISTINCT FROM effective_permission_bits
        OR product_admission_payload -> 'guild_owner'
            IS DISTINCT FROM pg_catalog.to_jsonb(guild_owner)
        OR product_admission_payload ->> 'policy_revision'
            IS DISTINCT FROM access_result.current_authority_projection
                ->> 'policy_revision'
        OR product_admission_payload ->> 'binding_fingerprint'
            IS DISTINCT FROM access_result.current_authority_projection
                ->> 'binding_fingerprint'
        OR prepared_promotion_intent #>> '{authority,tenant_id}'
            IS DISTINCT FROM expected_tenant_id
        OR prepared_promotion_intent #>> '{authority,installation_id}'
            IS DISTINCT FROM expected_installation_id
        OR prepared_promotion_intent #>> '{authority,principal_id}'
            IS DISTINCT FROM expected_principal_id
        OR prepared_promotion_intent #>> '{authority,session_id}'
            IS DISTINCT FROM expected_session_id
        OR prepared_promotion_intent #>> '{authority,session_generation}'
            IS DISTINCT FROM expected_generation::TEXT
        OR prepared_promotion_intent #>> '{authority,guild_id}'
            IS DISTINCT FROM expected_guild_id
        OR prepared_promotion_intent #>> '{authority,requester}'
            IS DISTINCT FROM expected_acting_user_id
        OR prepared_promotion_intent #>> '{evidence,candidate_revision}'
            IS DISTINCT FROM expected_candidate_revision::TEXT
        OR prepared_promotion_intent #>> '{evidence,candidate_ruleset_hash}'
            IS DISTINCT FROM expected_candidate_hash
        OR prepared_promotion_intent #>> '{evidence,context_fingerprint}'
            IS DISTINCT FROM expected_binding_fingerprint
        OR active_idempotency_key_digest !~ '^[0-9a-f]{64}$'
        OR idempotency_digest_key_id !~ '^[A-Za-z0-9_.:-]{1,64}$'
        OR semantic_request_digest !~ '^[0-9a-f]{64}$'
        OR new_receipt_id !~ '^[0-9a-f]{64}$'
        OR new_audit_event_id !~ '^[0-9a-f]{64}$'
        OR pg_catalog.cardinality(idempotency_key_digest_candidates)
            IS DISTINCT FROM pg_catalog.cardinality(idempotency_digest_key_id_candidates)
        OR pg_catalog.cardinality(idempotency_key_digest_candidates)
            IS DISTINCT FROM pg_catalog.cardinality(
                idempotency_digest_key_fingerprint_candidates
            )
        OR pg_catalog.cardinality(idempotency_key_digest_candidates) NOT BETWEEN 1 AND 8
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.generate_subscripts(
                idempotency_key_digest_candidates,
                1
            ) AS candidate(ordinal)
            WHERE (idempotency_key_digest_candidates[candidate.ordinal]
                    ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
                OR (idempotency_digest_key_id_candidates[candidate.ordinal]
                    ~ '^[A-Za-z0-9_.:-]{1,64}$') IS DISTINCT FROM TRUE
                OR (idempotency_digest_key_fingerprint_candidates[candidate.ordinal]
                    ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        )
        OR (
            SELECT pg_catalog.count(DISTINCT candidate.digest)
            FROM pg_catalog.unnest(idempotency_key_digest_candidates) AS candidate(digest)
        ) <> pg_catalog.cardinality(idempotency_key_digest_candidates)
        OR (
            SELECT pg_catalog.count(DISTINCT candidate.key_id)
            FROM pg_catalog.unnest(
                idempotency_digest_key_id_candidates
            ) AS candidate(key_id)
        ) <> pg_catalog.cardinality(idempotency_digest_key_id_candidates)
        OR (
            SELECT pg_catalog.count(DISTINCT candidate.fingerprint)
            FROM pg_catalog.unnest(
                idempotency_digest_key_fingerprint_candidates
            ) AS candidate(fingerprint)
        ) <> pg_catalog.cardinality(idempotency_digest_key_fingerprint_candidates)
        OR NOT EXISTS (
            SELECT 1
            FROM pg_catalog.generate_subscripts(
                idempotency_key_digest_candidates,
                1
            ) AS candidate(ordinal)
            WHERE idempotency_key_digest_candidates[candidate.ordinal]
                    = active_idempotency_key_digest
                AND idempotency_digest_key_id_candidates[candidate.ordinal]
                    = idempotency_digest_key_id
                AND idempotency_digest_key_fingerprint_candidates[candidate.ordinal]
                    = product_admission_payload
                        ->> 'idempotency_digest_key_fingerprint'
        )
        OR (
            product_admission_payload ->> 'authority_observed_at'
            ~ '^[-0-9]{10,}T[0-9:.]+(Z|[+-][0-9:]+)$'
        ) IS DISTINCT FROM TRUE
        OR (
            product_admission_payload ->> 'authority_expires_at'
            ~ '^[-0-9]{10,}T[0-9:.]+(Z|[+-][0-9:]+)$'
        ) IS DISTINCT FROM TRUE
    THEN
        RETURN QUERY SELECT 'invalid_candidate',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            access_result.database_now;
        RETURN;
    END IF;

    BEGIN
        payload_observed_at := (
            product_admission_payload ->> 'authority_observed_at'
        )::TIMESTAMPTZ;
        payload_expires_at := (
            product_admission_payload ->> 'authority_expires_at'
        )::TIMESTAMPTZ;
    EXCEPTION
        WHEN invalid_text_representation OR datetime_field_overflow THEN
            RETURN QUERY SELECT 'invalid_candidate',
                NULL::JSONB,
                NULL::JSONB,
                NULL::TEXT,
                access_result.database_now;
            RETURN;
    END;

    IF payload_observed_at IS DISTINCT FROM authority_observed_at
        OR payload_expires_at IS DISTINCT FROM authority_expires_at
    THEN
        RETURN QUERY SELECT 'invalid_candidate',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            access_result.database_now;
        RETURN;
    END IF;

    SELECT session.*
    INTO session_row
    FROM public.authoring_sessions AS session
    WHERE session.tenant_id = expected_tenant_id
        AND session.installation_id = expected_installation_id
        AND session.session_id = expected_session_id
    FOR UPDATE;

    SELECT generation.*
    INTO generation_row
    FROM public.authoring_session_generations AS generation
    WHERE generation.tenant_id = expected_tenant_id
        AND generation.installation_id = expected_installation_id
        AND generation.session_id = expected_session_id
        AND generation.generation = expected_generation;

    IF session_row.session_id IS NULL
        OR session_row.owner_principal_id IS DISTINCT FROM expected_principal_id
        OR session_row.lifecycle_state <> 'active'
        OR session_row.current_generation IS DISTINCT FROM expected_generation
        OR generation_row.session_id IS NULL
        OR generation_row.stage <> 'preview_ready'
        OR generation_row.candidate_revision IS DISTINCT FROM expected_candidate_revision
        OR generation_row.candidate_hash IS DISTINCT FROM expected_candidate_hash
        OR generation_row.binding_fingerprint IS DISTINCT FROM expected_binding_fingerprint
        OR generation_row.installation_authority_revision
            IS DISTINCT FROM observed_current_authority_revision
    THEN
        RETURN QUERY SELECT 'generation_mismatch',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            access_result.database_now;
        RETURN;
    END IF;
    RETURN QUERY SELECT 'persistence_corrupt',
        NULL::JSONB,
        NULL::JSONB,
        NULL::TEXT,
        access_result.database_now;
END;
$function$;

DO $scope$
DECLARE
    common_owner_name NAME;
    expected_signature TEXT;
    function_oid OID;
    unexpected_grantee OID;
    unexpected_grantee_name NAME;
    protected_signatures TEXT[] := ARRAY[
        'public.enforce_authoring_promotion_product_admission()',
        'public.enforce_authoring_promotion_product_transition()',
        'public.starring_product_promotion_executor_database_identity_v1()',
        'public.starring_product_promotion_replay_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,bigint,text,text[],text[],text[])',
        'public.starring_product_promotion_prepare_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bytea,text,bigint,bigint,text,text,text,text,jsonb,jsonb,text,text,text[],text[],text[],text,text,text,text)',
        'public.starring_product_promotion_publish_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text)',
        'public.starring_product_promotion_approval_environment_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text)',
        'public.starring_product_promotion_activation_link_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text,jsonb)',
        'public.starring_product_promotion_repair_link_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text,bytea,jsonb,text,text,text[],text[],text[],text,text,text,text)',
        'public.starring_product_promotion_keyring_coverage_v1(text[],text[])',
        'public.starring_product_promotion_authorize_current_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean)',
        'public.starring_product_promotion_finalize_receipt_v1(jsonb,jsonb,jsonb,jsonb,jsonb)'
    ]::TEXT[];
BEGIN
    SELECT pg_catalog.pg_get_userbyid(relation.relowner)
    INTO common_owner_name
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.authoring_promotions');
    IF common_owner_name IS NULL THEN
        RAISE EXCEPTION 'product promotion common owner is unavailable'
            USING ERRCODE = '55000';
    END IF;

    FOREACH expected_signature IN ARRAY protected_signatures
    LOOP
        function_oid := pg_catalog.to_regprocedure(expected_signature);
        IF function_oid IS NULL THEN
            RAISE EXCEPTION 'product promotion protected function is unavailable'
                USING ERRCODE = '55000';
        END IF;
        FOR unexpected_grantee IN
            SELECT DISTINCT privilege.grantee
            FROM pg_catalog.pg_proc AS function_row
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE function_row.oid = function_oid
                AND privilege.grantee <> 0
                AND privilege.grantee <> function_row.proowner
        LOOP
            unexpected_grantee_name := pg_catalog.pg_get_userbyid(unexpected_grantee);
            IF unexpected_grantee_name IS NULL THEN
                RAISE EXCEPTION 'product promotion function grantee is unavailable'
                    USING ERRCODE = '55000';
            END IF;
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM %I CASCADE',
                expected_signature,
                unexpected_grantee_name
            );
        END LOOP;
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE',
            expected_signature
        );
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s OWNER TO %I',
            expected_signature,
            common_owner_name
        );
    END LOOP;
END;
$scope$;
