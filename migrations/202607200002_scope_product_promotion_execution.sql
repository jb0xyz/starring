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
DECLARE
    access_result RECORD;
    promotion_row public.authoring_promotions%ROWTYPE;
    generation_row public.authoring_session_generations%ROWTYPE;
    historical_authority_row public.automation_installation_authority_versions%ROWTYPE;
    head_row public.automation_ruleset_heads%ROWTYPE;
    version_row public.automation_ruleset_versions%ROWTYPE;
    admission_payload JSONB;
    intent_document JSONB;
    publication_document JSONB;
    publication_record JSONB;
    publication_projection_document JSONB;
    persisted_record JSONB;
    final_clock TIMESTAMPTZ;
    record_created_at TIMESTAMPTZ;
    record_updated_at TIMESTAMPTZ;
    admitted_at TIMESTAMPTZ;
    payload_observed_at TIMESTAMPTZ;
    payload_expires_at TIMESTAMPTZ;
    permission_numeric NUMERIC;
    historical_authority_revision BIGINT;
    historical_generation BIGINT;
    historical_candidate_revision BIGINT;
    registry_schema_version BIGINT;
    published_version BIGINT;
    calculated_candidate_hash TEXT;
    calculated_content_hash TEXT;
    publication_disposition TEXT;
    version_count BIGINT;
    minimum_version BIGINT;
    maximum_version BIGINT;
    inserted_head_count BIGINT;
    updated_promotion_count BIGINT;
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
            access_result.database_now;
        RETURN;
    END IF;

    IF expected_promotion_id !~ '^[0-9a-f]{64}$'
        OR expected_promotion_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_promotion_request_digest !~ '^[0-9a-f]{64}$'
        OR expected_admission_digest !~ '^[0-9a-f]{64}$'
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    SELECT promotion.*
    INTO promotion_row
    FROM public.authoring_promotions AS promotion
    WHERE promotion.id = expected_promotion_id
    FOR UPDATE;
    access_result.database_now := pg_catalog.clock_timestamp();

    IF promotion_row.id IS NULL
        OR promotion_row.tenant_id IS DISTINCT FROM expected_tenant_id
        OR promotion_row.installation_id IS DISTINCT FROM expected_installation_id
        OR promotion_row.principal_id IS DISTINCT FROM expected_principal_id
    THEN
        RETURN QUERY SELECT 'scope_mismatch',
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    intent_document := promotion_row.record -> 'intent';
    IF promotion_row.record_format_version <> 1
        OR promotion_row.request_digest
            IS DISTINCT FROM expected_promotion_request_digest
        OR promotion_row.product_admission_format_version <> 1
        OR promotion_row.product_admission_digest
            IS DISTINCT FROM expected_admission_digest
        OR promotion_row.product_admission IS NULL
        OR pg_catalog.octet_length(promotion_row.record::TEXT) > 8388608
        OR pg_catalog.octet_length(promotion_row.product_admission::TEXT) > 32768
        OR pg_catalog.jsonb_typeof(promotion_row.record) IS DISTINCT FROM 'object'
        OR pg_catalog.jsonb_typeof(intent_document) IS DISTINCT FROM 'object'
        OR pg_catalog.jsonb_typeof(promotion_row.record -> 'stage')
            IS DISTINCT FROM 'object'
        OR promotion_row.record ->> 'id' IS DISTINCT FROM promotion_row.id
        OR promotion_row.record ->> 'revision'
            IS DISTINCT FROM promotion_row.revision::TEXT
        OR promotion_row.record ->> 'request_digest'
            IS DISTINCT FROM promotion_row.request_digest
        OR promotion_row.record #>> '{stage,state}'
            IS DISTINCT FROM promotion_row.stage
        OR promotion_row.record #>> '{intent,authority,tenant_id}'
            IS DISTINCT FROM promotion_row.tenant_id
        OR promotion_row.record #>> '{intent,authority,installation_id}'
            IS DISTINCT FROM promotion_row.installation_id
        OR promotion_row.record #>> '{intent,authority,principal_id}'
            IS DISTINCT FROM promotion_row.principal_id
        OR promotion_row.record #>> '{intent,authority,session_owner_id}'
            IS DISTINCT FROM promotion_row.principal_id
        OR promotion_row.record #>> '{intent,authority,guild_id}'
            IS DISTINCT FROM expected_guild_id
        OR promotion_row.record #>> '{intent,authority,requester}'
            IS DISTINCT FROM expected_acting_user_id
        OR promotion_row.record #>> '{intent,authority,ruleset_key}'
            !~ '^[A-Za-z0-9_-]{1,64}$'
        OR promotion_row.revision < expected_promotion_revision
        OR NOT (
            promotion_row.stage = 'prepared' AND promotion_row.revision = 1
            OR promotion_row.stage = 'published' AND promotion_row.revision = 2
            OR promotion_row.stage = 'activation_pending'
                AND promotion_row.revision = 3
            OR promotion_row.stage = 'expired'
                AND promotion_row.revision IN (3, 4)
        )
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(promotion_row.record) AS key(name)
        ) <> 7
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(promotion_row.record) AS key(name)
            WHERE key.name NOT IN (
                'id', 'revision', 'request_digest', 'intent', 'stage',
                'created_at', 'updated_at'
            )
        )
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(intent_document) AS key(name)
        ) <> 7
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(intent_document) AS key(name)
            WHERE key.name NOT IN (
                'idempotency_scope_digest', 'authority', 'evidence', 'definition',
                'preview', 'registry_schema_version',
                'expected_registry_content_hash'
            )
        )
        OR intent_document ->> 'idempotency_scope_digest'
            IS DISTINCT FROM promotion_row.id
        OR pg_catalog.jsonb_typeof(intent_document -> 'authority')
            IS DISTINCT FROM 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(
                CASE
                    WHEN pg_catalog.jsonb_typeof(intent_document -> 'authority')
                            = 'object'
                        THEN intent_document -> 'authority'
                    ELSE '{}'::JSONB
                END
            ) AS key(name)
        ) <> 11
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(
                CASE
                    WHEN pg_catalog.jsonb_typeof(intent_document -> 'authority')
                            = 'object'
                        THEN intent_document -> 'authority'
                    ELSE '{}'::JSONB
                END
            ) AS key(name)
            WHERE key.name NOT IN (
                'tenant_id', 'principal_id', 'session_owner_id', 'session_id',
                'session_generation', 'guild_id', 'installation_id',
                'ruleset_key', 'requester', 'binding_revision', 'policy'
            )
        )
        OR pg_catalog.jsonb_typeof(intent_document #> '{authority,policy}')
            IS DISTINCT FROM 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(
                CASE
                    WHEN pg_catalog.jsonb_typeof(
                            intent_document #> '{authority,policy}'
                        ) = 'object'
                        THEN intent_document #> '{authority,policy}'
                    ELSE '{}'::JSONB
                END
            ) AS key(name)
        ) <> 3
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(
                CASE
                    WHEN pg_catalog.jsonb_typeof(
                            intent_document #> '{authority,policy}'
                        ) = 'object'
                        THEN intent_document #> '{authority,policy}'
                    ELSE '{}'::JSONB
                END
            ) AS key(name)
            WHERE key.name NOT IN ('revision', 'required_approvals', 'ttl_seconds')
        )
        OR pg_catalog.jsonb_typeof(intent_document -> 'evidence')
            IS DISTINCT FROM 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(
                CASE
                    WHEN pg_catalog.jsonb_typeof(intent_document -> 'evidence')
                            = 'object'
                        THEN intent_document -> 'evidence'
                    ELSE '{}'::JSONB
                END
            ) AS key(name)
        ) <> 25
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(
                CASE
                    WHEN pg_catalog.jsonb_typeof(intent_document -> 'evidence')
                            = 'object'
                        THEN intent_document -> 'evidence'
                    ELSE '{}'::JSONB
                END
            ) AS key(name)
            WHERE key.name NOT IN (
                'artifact_version', 'intent_protocol_version', 'identity_revision',
                'extractor_revision', 'normalizer_revision', 'compiler_revision',
                'simulator_revision', 'recipe_id', 'recipe_version',
                'recipe_descriptor_digest', 'recipe_registry_digest',
                'requested_outcome', 'intent_revision', 'candidate_revision',
                'request_evidence_hash', 'request_evidence_entries',
                'compiler_input_hash', 'semantic_intent_hash', 'compiled_plan_hash',
                'candidate_ruleset_hash', 'candidate_draft_hash',
                'compiled_operations', 'context_fingerprint',
                'external_channel_bindings', 'stage_binding_digest'
            )
        )
        OR pg_catalog.jsonb_typeof(intent_document -> 'definition')
            IS DISTINCT FROM 'object'
        OR pg_catalog.octet_length((intent_document -> 'definition')::TEXT) > 524288
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(
                CASE
                    WHEN pg_catalog.jsonb_typeof(intent_document -> 'definition')
                            = 'object'
                        THEN intent_document -> 'definition'
                    ELSE '{}'::JSONB
                END
            ) AS key(name)
        ) <> 4
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(
                CASE
                    WHEN pg_catalog.jsonb_typeof(intent_document -> 'definition')
                            = 'object'
                        THEN intent_document -> 'definition'
                    ELSE '{}'::JSONB
                END
            ) AS key(name)
            WHERE key.name NOT IN ('version', 'panels', 'modals', 'rules')
        )
        OR intent_document #>> '{definition,version}' IS DISTINCT FROM '1'
        OR pg_catalog.jsonb_typeof(intent_document #> '{definition,panels}')
            IS DISTINCT FROM 'array'
        OR pg_catalog.jsonb_typeof(intent_document #> '{definition,modals}')
            IS DISTINCT FROM 'array'
        OR pg_catalog.jsonb_typeof(intent_document #> '{definition,rules}')
            IS DISTINCT FROM 'array'
        OR pg_catalog.jsonb_typeof(intent_document -> 'preview')
            IS DISTINCT FROM 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(
                CASE
                    WHEN pg_catalog.jsonb_typeof(intent_document -> 'preview')
                            = 'object'
                        THEN intent_document -> 'preview'
                    ELSE '{}'::JSONB
                END
            ) AS key(name)
        ) <> 2
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(
                CASE
                    WHEN pg_catalog.jsonb_typeof(intent_document -> 'preview')
                            = 'object'
                        THEN intent_document -> 'preview'
                    ELSE '{}'::JSONB
                END
            ) AS key(name)
            WHERE key.name NOT IN ('revision', 'summary')
        )
        OR pg_catalog.jsonb_typeof(intent_document #> '{preview,summary}')
            IS DISTINCT FROM 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(
                CASE
                    WHEN pg_catalog.jsonb_typeof(intent_document #> '{preview,summary}')
                            = 'object'
                        THEN intent_document #> '{preview,summary}'
                    ELSE '{}'::JSONB
                END
            ) AS key(name)
        ) <> 5
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(
                CASE
                    WHEN pg_catalog.jsonb_typeof(intent_document #> '{preview,summary}')
                            = 'object'
                        THEN intent_document #> '{preview,summary}'
                    ELSE '{}'::JSONB
                END
            ) AS key(name)
            WHERE key.name NOT IN (
                'panels', 'modals', 'rules', 'actions', 'unresolved_references'
            )
        )
        OR promotion_row.stage = 'prepared'
            AND promotion_row.record -> 'stage'
                IS DISTINCT FROM pg_catalog.jsonb_build_object('state', 'prepared')
        OR promotion_row.stage = 'published' AND (
            (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(
                    promotion_row.record -> 'stage'
                ) AS key(name)
            ) <> 2
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.jsonb_object_keys(
                    promotion_row.record -> 'stage'
                ) AS key(name)
                WHERE key.name NOT IN ('state', 'publication')
            )
        )
        OR promotion_row.stage IN ('activation_pending', 'expired') AND (
            (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(
                    promotion_row.record -> 'stage'
                ) AS key(name)
            ) <> 3
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.jsonb_object_keys(
                    promotion_row.record -> 'stage'
                ) AS key(name)
                WHERE key.name NOT IN ('state', 'publication', 'activation')
            )
        )
        OR (
            promotion_row.record ->> 'created_at'
            ~ '^[-0-9]{10,}T[0-9:.]+(Z|[+-][0-9:]+)$'
        ) IS DISTINCT FROM TRUE
        OR (
            promotion_row.record ->> 'updated_at'
            ~ '^[-0-9]{10,}T[0-9:.]+(Z|[+-][0-9:]+)$'
        ) IS DISTINCT FROM TRUE
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    IF pg_catalog.jsonb_typeof(promotion_row.product_admission)
            IS DISTINCT FROM 'object'
        OR promotion_row.product_admission ->> 'format_version' IS DISTINCT FROM '1'
        OR pg_catalog.jsonb_typeof(promotion_row.product_admission -> 'payload')
            IS DISTINCT FROM 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(
                promotion_row.product_admission
            ) AS key(name)
        ) <> 3
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(
                promotion_row.product_admission
            ) AS key(name)
            WHERE key.name NOT IN ('format_version', 'payload', 'admitted_at')
        )
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    admission_payload := promotion_row.product_admission -> 'payload';
    IF (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(admission_payload) AS key(name)
        ) <> 31
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(admission_payload) AS key(name)
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
        OR admission_payload ->> 'endpoint_domain'
            IS DISTINCT FROM 'product_promote_v1'
        OR admission_payload ->> 'tenant_id' IS DISTINCT FROM expected_tenant_id
        OR admission_payload ->> 'installation_id'
            IS DISTINCT FROM expected_installation_id
        OR admission_payload ->> 'principal_id'
            IS DISTINCT FROM expected_principal_id
        OR admission_payload ->> 'promotion_id'
            IS DISTINCT FROM expected_promotion_id
        OR admission_payload ->> 'promotion_request_digest'
            IS DISTINCT FROM expected_promotion_request_digest
        OR admission_payload ->> 'authoring_session_id'
            IS DISTINCT FROM intent_document #>> '{authority,session_id}'
        OR admission_payload ->> 'generation'
            IS DISTINCT FROM intent_document #>> '{authority,session_generation}'
        OR admission_payload ->> 'candidate_revision'
            IS DISTINCT FROM intent_document #>> '{evidence,candidate_revision}'
        OR admission_payload ->> 'candidate_hash'
            IS DISTINCT FROM intent_document #>> '{evidence,candidate_ruleset_hash}'
        OR admission_payload ->> 'binding_fingerprint'
            IS DISTINCT FROM intent_document #>> '{evidence,context_fingerprint}'
        OR admission_payload ->> 'policy_revision'
            IS DISTINCT FROM intent_document #>> '{authority,policy,revision}'
        OR admission_payload ->> 'discord_application_id'
            IS DISTINCT FROM expected_discord_application_id
        OR admission_payload ->> 'guild_id' IS DISTINCT FROM expected_guild_id
        OR admission_payload ->> 'acting_user_id'
            IS DISTINCT FROM expected_acting_user_id
        OR admission_payload ->> 'capability' IS DISTINCT FROM 'promote'
        OR (admission_payload ->> 'product_request_id'
            ~ '^[A-Za-z0-9_.:-]{1,128}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'session_subject_digest'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'idempotency_key_digest'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'idempotency_digest_key_id'
            ~ '^[A-Za-z0-9_.:-]{1,64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'idempotency_digest_key_fingerprint'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'semantic_request_digest'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'receipt_id'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'audit_event_id'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'authority_revision'
            ~ '^[1-9][0-9]{0,18}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'authority_payload_digest'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'authority_observation_digest'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'effective_permission_bits'
            ~ '^(0|[1-9][0-9]{0,19})$') IS DISTINCT FROM TRUE
        OR pg_catalog.jsonb_typeof(admission_payload -> 'guild_owner')
            IS DISTINCT FROM 'boolean'
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    BEGIN
        record_created_at := (promotion_row.record ->> 'created_at')::TIMESTAMPTZ;
        record_updated_at := (promotion_row.record ->> 'updated_at')::TIMESTAMPTZ;
        admitted_at := (
            promotion_row.product_admission ->> 'admitted_at'
        )::TIMESTAMPTZ;
        payload_observed_at := (
            admission_payload ->> 'authority_observed_at'
        )::TIMESTAMPTZ;
        payload_expires_at := (
            admission_payload ->> 'authority_expires_at'
        )::TIMESTAMPTZ;
        permission_numeric := (
            admission_payload ->> 'effective_permission_bits'
        )::NUMERIC;
        historical_authority_revision := (
            admission_payload ->> 'authority_revision'
        )::BIGINT;
        historical_generation := (
            intent_document #>> '{authority,session_generation}'
        )::BIGINT;
        historical_candidate_revision := (
            intent_document #>> '{evidence,candidate_revision}'
        )::BIGINT;
        registry_schema_version := (
            intent_document ->> 'registry_schema_version'
        )::BIGINT;
    EXCEPTION
        WHEN invalid_text_representation
            OR numeric_value_out_of_range
            OR datetime_field_overflow
        THEN
            RETURN QUERY SELECT 'persistence_corrupt',
                NULL::JSONB,
                NULL::JSONB,
                access_result.database_now;
            RETURN;
    END;

    IF record_created_at > record_updated_at
        OR record_updated_at > access_result.database_now
        OR admitted_at < record_created_at
        OR admitted_at > access_result.database_now
        OR promotion_row.stage IN ('prepared', 'published')
            AND admitted_at IS DISTINCT FROM record_created_at
        OR payload_observed_at > admitted_at
        OR admitted_at >= payload_expires_at
        OR payload_expires_at > payload_observed_at + INTERVAL '5 seconds'
        OR permission_numeric > 18446744073709551615
        OR NOT (
            (admission_payload ->> 'guild_owner')::BOOLEAN
            OR pg_catalog.mod(permission_numeric, 16) >= 8
            OR pg_catalog.mod(permission_numeric, 64) >= 32
        )
        OR historical_authority_revision NOT BETWEEN 1 AND 9223372036854775807
        OR historical_generation NOT BETWEEN 1 AND 9223372036854775807
        OR historical_candidate_revision NOT BETWEEN 1 AND 9223372036854775807
        OR registry_schema_version NOT BETWEEN 1 AND 4294967295
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    SELECT generation.*
    INTO generation_row
    FROM public.authoring_session_generations AS generation
    WHERE generation.tenant_id = expected_tenant_id
        AND generation.installation_id = expected_installation_id
        AND generation.session_id
            = intent_document #>> '{authority,session_id}'
        AND generation.generation = historical_generation;

    SELECT authority.*
    INTO historical_authority_row
    FROM public.automation_installation_authority_versions AS authority
    WHERE authority.tenant_id = expected_tenant_id
        AND authority.installation_id = expected_installation_id
        AND authority.revision = historical_authority_revision;

    calculated_candidate_hash := pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                'starring.intent.candidate_ruleset.v1',
                'UTF8'
            )
            || pg_catalog.decode('00', 'hex')
            || pg_catalog.convert_to(
                public.starring_canonical_json_v1(
                    intent_document -> 'definition'
                ),
                'UTF8'
            )
        ),
        'hex'
    );
    calculated_content_hash := public.starring_ruleset_content_hash_v1(
        registry_schema_version,
        intent_document -> 'definition'
    );

    IF generation_row.session_id IS NULL
        OR generation_row.stage <> 'preview_ready'
        OR generation_row.candidate_revision
            IS DISTINCT FROM historical_candidate_revision
        OR generation_row.candidate_hash
            IS DISTINCT FROM admission_payload ->> 'candidate_hash'
        OR generation_row.binding_fingerprint
            IS DISTINCT FROM admission_payload ->> 'binding_fingerprint'
        OR generation_row.installation_authority_revision
            IS DISTINCT FROM historical_authority_revision
        OR historical_authority_row.installation_id IS NULL
        OR pg_catalog.octet_length(historical_authority_row.resource_bindings::TEXT)
            > 262144
        OR generation_row.resource_bindings
            IS DISTINCT FROM historical_authority_row.resource_bindings
        OR generation_row.binding_fingerprint
            IS DISTINCT FROM historical_authority_row.binding_fingerprint
        OR historical_authority_row.authority_payload_digest
            IS DISTINCT FROM admission_payload ->> 'authority_payload_digest'
        OR historical_authority_row.binding_revision::TEXT
            IS DISTINCT FROM intent_document #>> '{authority,binding_revision}'
        OR historical_authority_row.policy_revision::TEXT
            IS DISTINCT FROM intent_document #>> '{authority,policy,revision}'
        OR historical_authority_row.required_approvals::TEXT
            IS DISTINCT FROM intent_document
                #>> '{authority,policy,required_approvals}'
        OR historical_authority_row.activation_ttl_seconds::TEXT
            IS DISTINCT FROM intent_document #>> '{authority,policy,ttl_seconds}'
        OR calculated_candidate_hash
            IS DISTINCT FROM admission_payload ->> 'candidate_hash'
        OR calculated_content_hash IS NULL
        OR calculated_content_hash
            IS DISTINCT FROM intent_document ->> 'expected_registry_content_hash'
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    IF promotion_row.stage <> 'prepared' THEN
        publication_document := promotion_row.record #> '{stage,publication}';
        IF pg_catalog.jsonb_typeof(publication_document) IS DISTINCT FROM 'object'
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(publication_document) AS key(name)
            ) <> 5
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.jsonb_object_keys(publication_document) AS key(name)
                WHERE key.name NOT IN (
                    'version', 'schema_version', 'content_hash', 'disposition',
                    'registry_created_by'
                )
            )
            OR (publication_document ->> 'version' ~ '^[1-9][0-9]{0,9}$')
                IS DISTINCT FROM TRUE
            OR (publication_document ->> 'schema_version' ~ '^[1-9][0-9]{0,9}$')
                IS DISTINCT FROM TRUE
            OR publication_document ->> 'content_hash'
                IS DISTINCT FROM calculated_content_hash
            OR (
                publication_document ->> 'disposition' IN ('created', 'reused')
            ) IS DISTINCT FROM TRUE
            OR (publication_document ->> 'registry_created_by'
                ~ '^[1-9][0-9]{0,19}$') IS DISTINCT FROM TRUE
        THEN
            RETURN QUERY SELECT 'persistence_corrupt',
                NULL::JSONB,
                NULL::JSONB,
                access_result.database_now;
            RETURN;
        END IF;
        BEGIN
            published_version := (publication_document ->> 'version')::BIGINT;
        EXCEPTION
            WHEN invalid_text_representation OR numeric_value_out_of_range THEN
                RETURN QUERY SELECT 'persistence_corrupt',
                    NULL::JSONB,
                    NULL::JSONB,
                    access_result.database_now;
                RETURN;
        END;
        IF published_version NOT BETWEEN 1 AND 4294967295
            OR publication_document ->> 'schema_version'
                IS DISTINCT FROM registry_schema_version::TEXT
        THEN
            RETURN QUERY SELECT 'persistence_corrupt',
                NULL::JSONB,
                NULL::JSONB,
                access_result.database_now;
            RETURN;
        END IF;
    END IF;

    inserted_head_count := 0;
    IF promotion_row.stage = 'prepared' THEN
        INSERT INTO public.automation_ruleset_heads (
            guild_id,
            ruleset_key,
            next_version
        ) VALUES (
            expected_guild_id,
            intent_document #>> '{authority,ruleset_key}',
            1
        )
        ON CONFLICT (guild_id, ruleset_key) DO NOTHING;
        GET DIAGNOSTICS inserted_head_count = ROW_COUNT;
    END IF;

    SELECT head.*
    INTO head_row
    FROM public.automation_ruleset_heads AS head
    WHERE head.guild_id = expected_guild_id
        AND head.ruleset_key = intent_document #>> '{authority,ruleset_key}'
    FOR UPDATE;

    SELECT pg_catalog.count(*),
        pg_catalog.min(version.version),
        pg_catalog.max(version.version)
    INTO version_count, minimum_version, maximum_version
    FROM public.automation_ruleset_versions AS version
    WHERE version.guild_id = expected_guild_id
        AND version.ruleset_key = intent_document #>> '{authority,ruleset_key}';

    IF head_row.guild_id IS NULL
        OR head_row.next_version NOT BETWEEN 1 AND 4294967296
        OR version_count = 0 AND head_row.next_version <> 1
        OR version_count > 0 AND (
            minimum_version <> 1
            OR maximum_version <> head_row.next_version - 1
            OR version_count <> head_row.next_version - 1
        )
    THEN
        IF inserted_head_count = 1 THEN
            DELETE FROM public.automation_ruleset_heads AS head
            WHERE head.guild_id = expected_guild_id
                AND head.ruleset_key
                    = intent_document #>> '{authority,ruleset_key}'
                AND head.next_version = 1;
        END IF;
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    IF promotion_row.stage = 'prepared' THEN
        SELECT version.*
        INTO version_row
        FROM public.automation_ruleset_versions AS version
        WHERE version.guild_id = expected_guild_id
            AND version.ruleset_key
                = intent_document #>> '{authority,ruleset_key}'
            AND version.content_hash = calculated_content_hash;
        IF version_row.guild_id IS NULL THEN
            IF head_row.next_version > 4294967295
                OR EXISTS (
                    SELECT 1
                    FROM public.automation_ruleset_versions AS version
                    WHERE version.guild_id = expected_guild_id
                        AND version.ruleset_key
                            = intent_document #>> '{authority,ruleset_key}'
                        AND version.version = head_row.next_version
                )
            THEN
                IF inserted_head_count = 1 THEN
                    DELETE FROM public.automation_ruleset_heads AS head
                    WHERE head.guild_id = expected_guild_id
                        AND head.ruleset_key
                            = intent_document #>> '{authority,ruleset_key}'
                        AND head.next_version = 1;
                END IF;
                RETURN QUERY SELECT 'persistence_corrupt',
                    NULL::JSONB,
                    NULL::JSONB,
                    access_result.database_now;
                RETURN;
            END IF;
            published_version := head_row.next_version;
            publication_disposition := 'created';
            version_row.guild_id := expected_guild_id;
            version_row.ruleset_key := intent_document
                #>> '{authority,ruleset_key}';
            version_row.version := published_version;
            version_row.schema_version := registry_schema_version;
            version_row.definition := intent_document -> 'definition';
            version_row.content_hash := calculated_content_hash;
            version_row.created_by := expected_acting_user_id;
        ELSE
            IF version_row.schema_version IS DISTINCT FROM registry_schema_version
                OR version_row.definition
                    IS DISTINCT FROM intent_document -> 'definition'
            OR version_row.content_hash IS DISTINCT FROM calculated_content_hash
            OR version_row.canonical_content_hash
                IS DISTINCT FROM calculated_content_hash
            OR (CASE
                WHEN version_row.created_by ~ '^[1-9][0-9]{0,19}$'
                    THEN version_row.created_by::NUMERIC
                        <= 18446744073709551615
                ELSE FALSE
            END) IS DISTINCT FROM TRUE
        THEN
                RETURN QUERY SELECT 'persistence_corrupt',
                    NULL::JSONB,
                    NULL::JSONB,
                    access_result.database_now;
                RETURN;
            END IF;
            published_version := version_row.version;
            publication_disposition := 'reused';
        END IF;
    ELSE
        SELECT version.*
        INTO version_row
        FROM public.automation_ruleset_versions AS version
        WHERE version.guild_id = expected_guild_id
            AND version.ruleset_key
                = intent_document #>> '{authority,ruleset_key}'
            AND version.version = published_version;
        publication_disposition := publication_document ->> 'disposition';
        IF version_row.guild_id IS NULL
            OR version_row.schema_version IS DISTINCT FROM registry_schema_version
            OR version_row.definition IS DISTINCT FROM intent_document -> 'definition'
            OR version_row.content_hash IS DISTINCT FROM calculated_content_hash
            OR version_row.canonical_content_hash IS DISTINCT FROM calculated_content_hash
            OR version_row.created_by
                IS DISTINCT FROM publication_document ->> 'registry_created_by'
            OR (CASE
                WHEN version_row.created_by ~ '^[1-9][0-9]{0,19}$'
                    THEN version_row.created_by::NUMERIC
                        <= 18446744073709551615
                ELSE FALSE
            END) IS DISTINCT FROM TRUE
        THEN
            RETURN QUERY SELECT 'persistence_corrupt',
                NULL::JSONB,
                NULL::JSONB,
                access_result.database_now;
            RETURN;
        END IF;
    END IF;

    final_clock := pg_catalog.clock_timestamp();
    IF authority_observed_at > final_clock
        OR final_clock >= authority_expires_at
        OR NOT EXISTS (
            SELECT 1
            FROM public.product_principals AS principal
            WHERE principal.principal_id = expected_principal_id
                AND NOT principal.disabled
                AND principal.discord_user_id = expected_acting_user_id
        )
        OR NOT EXISTS (
            SELECT 1
            FROM public.product_auth_sessions AS product_session
            WHERE product_session.session_digest
                    = expected_product_session_digest
                AND product_session.principal_id = expected_principal_id
                AND product_session.oauth_state_digest IS NOT NULL
                AND product_session.revoked_at IS NULL
                AND product_session.revocation_reason IS NULL
                AND final_clock < product_session.idle_expires_at
                AND final_clock < product_session.absolute_expires_at
        )
    THEN
        IF inserted_head_count = 1 THEN
            DELETE FROM public.automation_ruleset_heads AS head
            WHERE head.guild_id = expected_guild_id
                AND head.ruleset_key
                    = intent_document #>> '{authority,ruleset_key}'
                AND head.next_version = 1;
        END IF;
        RETURN QUERY SELECT 'access_denied',
            NULL::JSONB,
            NULL::JSONB,
            final_clock;
        RETURN;
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM public.automation_installations AS installation
        INNER JOIN public.automation_installation_authority_versions AS authority
            ON authority.tenant_id = installation.tenant_id
            AND authority.installation_id = installation.installation_id
            AND authority.revision = installation.current_authority_revision
        WHERE installation.tenant_id = expected_tenant_id
            AND installation.installation_id = expected_installation_id
            AND installation.lifecycle_state = 'active'
            AND installation.discord_application_id
                = expected_discord_application_id
            AND installation.discord_guild_id = expected_guild_id
            AND installation.ruleset_key
                = intent_document #>> '{authority,ruleset_key}'
            AND installation.current_authority_revision
                = observed_current_authority_revision
            AND authority.authority_payload_digest
                = observed_current_authority_payload_digest
    ) THEN
        IF inserted_head_count = 1 THEN
            DELETE FROM public.automation_ruleset_heads AS head
            WHERE head.guild_id = expected_guild_id
                AND head.ruleset_key
                    = intent_document #>> '{authority,ruleset_key}'
                AND head.next_version = 1;
        END IF;
        RETURN QUERY SELECT 'scope_mismatch',
            NULL::JSONB,
            NULL::JSONB,
            final_clock;
        RETURN;
    END IF;

    publication_projection_document := pg_catalog.jsonb_build_object(
        'format_version', 1,
        'disposition', publication_disposition,
        'artifact', pg_catalog.jsonb_build_object(
            'guild_id', version_row.guild_id,
            'ruleset_key', version_row.ruleset_key,
            'version', version_row.version,
            'schema_version', version_row.schema_version,
            'definition', version_row.definition,
            'content_hash', version_row.content_hash,
            'created_by', version_row.created_by
        )
    );
    IF pg_catalog.octet_length(publication_projection_document::TEXT) > 1048576
    THEN
        IF inserted_head_count = 1 THEN
            DELETE FROM public.automation_ruleset_heads AS head
            WHERE head.guild_id = expected_guild_id
                AND head.ruleset_key
                    = intent_document #>> '{authority,ruleset_key}'
                AND head.next_version = 1;
        END IF;
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            final_clock;
        RETURN;
    END IF;

    IF promotion_row.stage = 'prepared' THEN
        publication_record := pg_catalog.jsonb_build_object(
            'version', version_row.version,
            'schema_version', version_row.schema_version,
            'content_hash', version_row.content_hash,
            'disposition', publication_disposition,
            'registry_created_by', version_row.created_by
        );
        persisted_record := promotion_row.record || pg_catalog.jsonb_build_object(
            'revision', 2,
            'stage', pg_catalog.jsonb_build_object(
                'state', 'published',
                'publication', publication_record
            ),
            'updated_at', final_clock
        );
        IF pg_catalog.octet_length(persisted_record::TEXT) > 8388608 THEN
            IF inserted_head_count = 1 THEN
                DELETE FROM public.automation_ruleset_heads AS head
                WHERE head.guild_id = expected_guild_id
                    AND head.ruleset_key
                        = intent_document #>> '{authority,ruleset_key}'
                    AND head.next_version = 1;
            END IF;
            RETURN QUERY SELECT 'persistence_corrupt',
                NULL::JSONB,
                NULL::JSONB,
                final_clock;
            RETURN;
        END IF;

        IF publication_disposition = 'created' THEN
            INSERT INTO public.automation_ruleset_versions (
                guild_id,
                ruleset_key,
                version,
                schema_version,
                definition,
                content_hash,
                created_by
            ) VALUES (
                version_row.guild_id,
                version_row.ruleset_key,
                version_row.version,
                version_row.schema_version,
                version_row.definition,
                version_row.content_hash,
                version_row.created_by
            );
            UPDATE public.automation_ruleset_heads AS head
            SET next_version = head.next_version + 1
            WHERE head.guild_id = version_row.guild_id
                AND head.ruleset_key = version_row.ruleset_key
                AND head.next_version = version_row.version;
            IF NOT FOUND THEN
                RAISE EXCEPTION 'product promotion publication head advance failed'
                    USING ERRCODE = '23514';
            END IF;
        END IF;

        UPDATE public.authoring_promotions AS promotion
        SET revision = 2,
            stage = 'published',
            record = persisted_record
        WHERE promotion.id = promotion_row.id
            AND promotion.revision = 1
            AND promotion.stage = 'prepared';
        GET DIAGNOSTICS updated_promotion_count = ROW_COUNT;
        IF updated_promotion_count <> 1 THEN
            RAISE EXCEPTION 'product promotion publication journal advance failed'
                USING ERRCODE = '23514';
        END IF;

        RETURN QUERY SELECT publication_disposition,
            publication_projection_document,
            persisted_record,
            final_clock;
        RETURN;
    END IF;

    RETURN QUERY SELECT
        CASE promotion_row.stage
            WHEN 'published' THEN 'published_exact'
            ELSE 'final_exact'
        END,
        publication_projection_document,
        promotion_row.record,
        final_clock;
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
    promotion_record JSONB,
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
DECLARE
    access_result RECORD;
    promotion_row public.authoring_promotions%ROWTYPE;
    generation_row public.authoring_session_generations%ROWTYPE;
    historical_authority_row public.automation_installation_authority_versions%ROWTYPE;
    head_row public.automation_ruleset_heads%ROWTYPE;
    target_version_row public.automation_ruleset_versions%ROWTYPE;
    active_version_row public.automation_ruleset_versions%ROWTYPE;
    activation_row public.automation_ruleset_activations%ROWTYPE;
    admission_payload JSONB;
    intent_document JSONB;
    publication_document JSONB;
    target_projection_document JSONB;
    final_clock TIMESTAMPTZ;
    record_created_at TIMESTAMPTZ;
    record_updated_at TIMESTAMPTZ;
    admitted_at TIMESTAMPTZ;
    payload_observed_at TIMESTAMPTZ;
    payload_expires_at TIMESTAMPTZ;
    permission_numeric NUMERIC;
    historical_authority_revision_value BIGINT;
    historical_generation BIGINT;
    historical_candidate_revision BIGINT;
    registry_schema_version BIGINT;
    target_version BIGINT;
    calculated_candidate_hash TEXT;
    calculated_content_hash TEXT;
    version_count BIGINT;
    minimum_version BIGINT;
    maximum_version BIGINT;
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
            NULL::BIGINT,
            NULL::JSONB,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::TEXT,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    IF expected_promotion_id !~ '^[0-9a-f]{64}$'
        OR expected_promotion_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_promotion_request_digest !~ '^[0-9a-f]{64}$'
        OR expected_admission_digest !~ '^[0-9a-f]{64}$'
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::BIGINT,
            NULL::JSONB,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::TEXT,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    SELECT promotion.*
    INTO promotion_row
    FROM public.authoring_promotions AS promotion
    WHERE promotion.id = expected_promotion_id
    FOR SHARE;
    access_result.database_now := pg_catalog.clock_timestamp();

    IF promotion_row.id IS NULL
        OR promotion_row.tenant_id IS DISTINCT FROM expected_tenant_id
        OR promotion_row.installation_id IS DISTINCT FROM expected_installation_id
        OR promotion_row.principal_id IS DISTINCT FROM expected_principal_id
    THEN
        RETURN QUERY SELECT 'scope_mismatch',
            NULL::JSONB,
            NULL::BIGINT,
            NULL::JSONB,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::TEXT,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    intent_document := promotion_row.record -> 'intent';
    publication_document := promotion_row.record #> '{stage,publication}';
    IF promotion_row.record_format_version <> 1
        OR promotion_row.request_digest
            IS DISTINCT FROM expected_promotion_request_digest
        OR promotion_row.product_admission_format_version <> 1
        OR promotion_row.product_admission_digest
            IS DISTINCT FROM expected_admission_digest
        OR promotion_row.product_admission IS NULL
        OR promotion_row.revision < expected_promotion_revision
        OR NOT (
            promotion_row.stage = 'published' AND promotion_row.revision = 2
            OR promotion_row.stage = 'activation_pending'
                AND promotion_row.revision = 3
            OR promotion_row.stage = 'expired'
                AND promotion_row.revision IN (3, 4)
        )
        OR pg_catalog.jsonb_typeof(promotion_row.record) IS DISTINCT FROM 'object'
        OR pg_catalog.octet_length(promotion_row.record::TEXT) > 8388608
        OR pg_catalog.jsonb_typeof(intent_document) IS DISTINCT FROM 'object'
        OR pg_catalog.jsonb_typeof(promotion_row.record -> 'stage')
            IS DISTINCT FROM 'object'
        OR pg_catalog.jsonb_typeof(publication_document) IS DISTINCT FROM 'object'
        OR promotion_row.record ->> 'id' IS DISTINCT FROM promotion_row.id
        OR promotion_row.record ->> 'revision'
            IS DISTINCT FROM promotion_row.revision::TEXT
        OR promotion_row.record ->> 'request_digest'
            IS DISTINCT FROM promotion_row.request_digest
        OR promotion_row.record #>> '{stage,state}'
            IS DISTINCT FROM promotion_row.stage
        OR promotion_row.record #>> '{intent,authority,tenant_id}'
            IS DISTINCT FROM expected_tenant_id
        OR promotion_row.record #>> '{intent,authority,installation_id}'
            IS DISTINCT FROM expected_installation_id
        OR promotion_row.record #>> '{intent,authority,principal_id}'
            IS DISTINCT FROM expected_principal_id
        OR promotion_row.record #>> '{intent,authority,session_owner_id}'
            IS DISTINCT FROM expected_principal_id
        OR promotion_row.record #>> '{intent,authority,guild_id}'
            IS DISTINCT FROM expected_guild_id
        OR promotion_row.record #>> '{intent,authority,requester}'
            IS DISTINCT FROM expected_acting_user_id
        OR promotion_row.record #>> '{intent,authority,ruleset_key}'
            !~ '^[A-Za-z0-9_-]{1,64}$'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(promotion_row.record) AS key(name)
        ) <> 7
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(promotion_row.record) AS key(name)
            WHERE key.name NOT IN (
                'id', 'revision', 'request_digest', 'intent', 'stage',
                'created_at', 'updated_at'
            )
        )
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(intent_document) AS key(name)
        ) <> 7
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(intent_document) AS key(name)
            WHERE key.name NOT IN (
                'idempotency_scope_digest', 'authority', 'evidence', 'definition',
                'preview', 'registry_schema_version',
                'expected_registry_content_hash'
            )
        )
        OR intent_document ->> 'idempotency_scope_digest'
            IS DISTINCT FROM promotion_row.id
        OR pg_catalog.jsonb_typeof(intent_document -> 'authority')
            IS DISTINCT FROM 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(
                CASE
                    WHEN pg_catalog.jsonb_typeof(intent_document -> 'authority')
                            = 'object'
                        THEN intent_document -> 'authority'
                    ELSE '{}'::JSONB
                END
            ) AS key(name)
        ) <> 11
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(
                CASE
                    WHEN pg_catalog.jsonb_typeof(intent_document -> 'authority')
                            = 'object'
                        THEN intent_document -> 'authority'
                    ELSE '{}'::JSONB
                END
            ) AS key(name)
            WHERE key.name NOT IN (
                'tenant_id', 'principal_id', 'session_owner_id', 'session_id',
                'session_generation', 'guild_id', 'installation_id',
                'ruleset_key', 'requester', 'binding_revision', 'policy'
            )
        )
        OR pg_catalog.jsonb_typeof(intent_document #> '{authority,policy}')
            IS DISTINCT FROM 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(
                CASE
                    WHEN pg_catalog.jsonb_typeof(
                            intent_document #> '{authority,policy}'
                        ) = 'object'
                        THEN intent_document #> '{authority,policy}'
                    ELSE '{}'::JSONB
                END
            ) AS key(name)
        ) <> 3
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(
                CASE
                    WHEN pg_catalog.jsonb_typeof(
                            intent_document #> '{authority,policy}'
                        ) = 'object'
                        THEN intent_document #> '{authority,policy}'
                    ELSE '{}'::JSONB
                END
            ) AS key(name)
            WHERE key.name NOT IN ('revision', 'required_approvals', 'ttl_seconds')
        )
        OR pg_catalog.jsonb_typeof(intent_document -> 'evidence')
            IS DISTINCT FROM 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(
                CASE
                    WHEN pg_catalog.jsonb_typeof(intent_document -> 'evidence')
                            = 'object'
                        THEN intent_document -> 'evidence'
                    ELSE '{}'::JSONB
                END
            ) AS key(name)
        ) <> 25
        OR pg_catalog.jsonb_typeof(intent_document -> 'definition')
            IS DISTINCT FROM 'object'
        OR pg_catalog.octet_length((intent_document -> 'definition')::TEXT) > 524288
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(
                CASE
                    WHEN pg_catalog.jsonb_typeof(intent_document -> 'definition')
                            = 'object'
                        THEN intent_document -> 'definition'
                    ELSE '{}'::JSONB
                END
            ) AS key(name)
        ) <> 4
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(
                CASE
                    WHEN pg_catalog.jsonb_typeof(intent_document -> 'definition')
                            = 'object'
                        THEN intent_document -> 'definition'
                    ELSE '{}'::JSONB
                END
            ) AS key(name)
            WHERE key.name NOT IN ('version', 'panels', 'modals', 'rules')
        )
        OR intent_document #>> '{definition,version}' IS DISTINCT FROM '1'
        OR pg_catalog.jsonb_typeof(intent_document #> '{definition,panels}')
            IS DISTINCT FROM 'array'
        OR pg_catalog.jsonb_typeof(intent_document #> '{definition,modals}')
            IS DISTINCT FROM 'array'
        OR pg_catalog.jsonb_typeof(intent_document #> '{definition,rules}')
            IS DISTINCT FROM 'array'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(publication_document) AS key(name)
        ) <> 5
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(publication_document) AS key(name)
            WHERE key.name NOT IN (
                'version', 'schema_version', 'content_hash', 'disposition',
                'registry_created_by'
            )
        )
        OR (publication_document ->> 'version' ~ '^[1-9][0-9]{0,9}$')
            IS DISTINCT FROM TRUE
        OR (publication_document ->> 'schema_version' ~ '^[1-9][0-9]{0,9}$')
            IS DISTINCT FROM TRUE
        OR (publication_document ->> 'content_hash' ~ '^[0-9a-f]{64}$')
            IS DISTINCT FROM TRUE
        OR (
            publication_document ->> 'disposition' IN ('created', 'reused')
        ) IS DISTINCT FROM TRUE
        OR (publication_document ->> 'registry_created_by'
            ~ '^[1-9][0-9]{0,19}$') IS DISTINCT FROM TRUE
        OR promotion_row.stage = 'published' AND (
            (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(
                    promotion_row.record -> 'stage'
                ) AS key(name)
            ) <> 2
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.jsonb_object_keys(
                    promotion_row.record -> 'stage'
                ) AS key(name)
                WHERE key.name NOT IN ('state', 'publication')
            )
        )
        OR promotion_row.stage IN ('activation_pending', 'expired') AND (
            (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(
                    promotion_row.record -> 'stage'
                ) AS key(name)
            ) <> 3
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.jsonb_object_keys(
                    promotion_row.record -> 'stage'
                ) AS key(name)
                WHERE key.name NOT IN ('state', 'publication', 'activation')
            )
        )
        OR (
            promotion_row.record ->> 'created_at'
            ~ '^[-0-9]{10,}T[0-9:.]+(Z|[+-][0-9:]+)$'
        ) IS DISTINCT FROM TRUE
        OR (
            promotion_row.record ->> 'updated_at'
            ~ '^[-0-9]{10,}T[0-9:.]+(Z|[+-][0-9:]+)$'
        ) IS DISTINCT FROM TRUE
        OR pg_catalog.jsonb_typeof(promotion_row.product_admission)
            IS DISTINCT FROM 'object'
        OR pg_catalog.octet_length(promotion_row.product_admission::TEXT) > 32768
        OR promotion_row.product_admission ->> 'format_version' IS DISTINCT FROM '1'
        OR pg_catalog.jsonb_typeof(promotion_row.product_admission -> 'payload')
            IS DISTINCT FROM 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(
                promotion_row.product_admission
            ) AS key(name)
        ) <> 3
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(
                promotion_row.product_admission
            ) AS key(name)
            WHERE key.name NOT IN ('format_version', 'payload', 'admitted_at')
        )
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::BIGINT,
            NULL::JSONB,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::TEXT,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    admission_payload := promotion_row.product_admission -> 'payload';
    IF (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(admission_payload) AS key(name)
        ) <> 31
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(admission_payload) AS key(name)
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
        OR admission_payload ->> 'endpoint_domain'
            IS DISTINCT FROM 'product_promote_v1'
        OR admission_payload ->> 'tenant_id' IS DISTINCT FROM expected_tenant_id
        OR admission_payload ->> 'installation_id'
            IS DISTINCT FROM expected_installation_id
        OR admission_payload ->> 'principal_id'
            IS DISTINCT FROM expected_principal_id
        OR admission_payload ->> 'promotion_id'
            IS DISTINCT FROM expected_promotion_id
        OR admission_payload ->> 'promotion_request_digest'
            IS DISTINCT FROM expected_promotion_request_digest
        OR admission_payload ->> 'authoring_session_id'
            IS DISTINCT FROM intent_document #>> '{authority,session_id}'
        OR admission_payload ->> 'generation'
            IS DISTINCT FROM intent_document #>> '{authority,session_generation}'
        OR admission_payload ->> 'candidate_revision'
            IS DISTINCT FROM intent_document #>> '{evidence,candidate_revision}'
        OR admission_payload ->> 'candidate_hash'
            IS DISTINCT FROM intent_document #>> '{evidence,candidate_ruleset_hash}'
        OR admission_payload ->> 'binding_fingerprint'
            IS DISTINCT FROM intent_document #>> '{evidence,context_fingerprint}'
        OR admission_payload ->> 'policy_revision'
            IS DISTINCT FROM intent_document #>> '{authority,policy,revision}'
        OR admission_payload ->> 'discord_application_id'
            IS DISTINCT FROM expected_discord_application_id
        OR admission_payload ->> 'guild_id' IS DISTINCT FROM expected_guild_id
        OR admission_payload ->> 'acting_user_id'
            IS DISTINCT FROM expected_acting_user_id
        OR admission_payload ->> 'capability' IS DISTINCT FROM 'promote'
        OR (admission_payload ->> 'authority_revision'
            ~ '^[1-9][0-9]{0,18}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'authority_payload_digest'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'authority_observation_digest'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'authority_observed_at'
            ~ '^[-0-9]{10,}T[0-9:.]+(Z|[+-][0-9:]+)$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'authority_expires_at'
            ~ '^[-0-9]{10,}T[0-9:.]+(Z|[+-][0-9:]+)$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'effective_permission_bits'
            ~ '^(0|[1-9][0-9]{0,19})$') IS DISTINCT FROM TRUE
        OR pg_catalog.jsonb_typeof(admission_payload -> 'guild_owner')
            IS DISTINCT FROM 'boolean'
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::BIGINT,
            NULL::JSONB,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::TEXT,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    BEGIN
        record_created_at := (promotion_row.record ->> 'created_at')::TIMESTAMPTZ;
        record_updated_at := (promotion_row.record ->> 'updated_at')::TIMESTAMPTZ;
        admitted_at := (
            promotion_row.product_admission ->> 'admitted_at'
        )::TIMESTAMPTZ;
        payload_observed_at := (
            admission_payload ->> 'authority_observed_at'
        )::TIMESTAMPTZ;
        payload_expires_at := (
            admission_payload ->> 'authority_expires_at'
        )::TIMESTAMPTZ;
        permission_numeric := (
            admission_payload ->> 'effective_permission_bits'
        )::NUMERIC;
        historical_authority_revision_value := (
            admission_payload ->> 'authority_revision'
        )::BIGINT;
        historical_generation := (
            intent_document #>> '{authority,session_generation}'
        )::BIGINT;
        historical_candidate_revision := (
            intent_document #>> '{evidence,candidate_revision}'
        )::BIGINT;
        registry_schema_version := (
            intent_document ->> 'registry_schema_version'
        )::BIGINT;
        target_version := (publication_document ->> 'version')::BIGINT;
    EXCEPTION
        WHEN invalid_text_representation
            OR numeric_value_out_of_range
            OR datetime_field_overflow
        THEN
            RETURN QUERY SELECT 'persistence_corrupt',
                NULL::JSONB,
                NULL::BIGINT,
                NULL::JSONB,
                NULL::TEXT,
                NULL::BIGINT,
                NULL::TEXT,
                NULL::JSONB,
                access_result.database_now;
            RETURN;
    END;

    IF record_created_at > record_updated_at
        OR record_updated_at > access_result.database_now
        OR admitted_at < record_created_at
        OR admitted_at > access_result.database_now
        OR payload_observed_at > admitted_at
        OR admitted_at >= payload_expires_at
        OR payload_expires_at > payload_observed_at + INTERVAL '5 seconds'
        OR permission_numeric > 18446744073709551615
        OR NOT (
            (admission_payload ->> 'guild_owner')::BOOLEAN
            OR pg_catalog.mod(permission_numeric, 16) >= 8
            OR pg_catalog.mod(permission_numeric, 64) >= 32
        )
        OR historical_authority_revision_value
            NOT BETWEEN 1 AND 9223372036854775807
        OR historical_generation NOT BETWEEN 1 AND 9223372036854775807
        OR historical_candidate_revision NOT BETWEEN 1 AND 9223372036854775807
        OR registry_schema_version NOT BETWEEN 1 AND 4294967295
        OR target_version NOT BETWEEN 1 AND 4294967295
        OR publication_document ->> 'schema_version'
            IS DISTINCT FROM registry_schema_version::TEXT
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::BIGINT,
            NULL::JSONB,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::TEXT,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    IF promotion_row.stage IN ('activation_pending', 'expired') THEN
        final_clock := pg_catalog.clock_timestamp();
        IF authority_observed_at > final_clock
            OR final_clock >= authority_expires_at
            OR NOT EXISTS (
                SELECT 1
                FROM public.product_principals AS principal
                WHERE principal.principal_id = expected_principal_id
                    AND NOT principal.disabled
                    AND principal.discord_user_id = expected_acting_user_id
            )
            OR NOT EXISTS (
                SELECT 1
                FROM public.product_auth_sessions AS product_session
                WHERE product_session.session_digest
                        = expected_product_session_digest
                    AND product_session.principal_id = expected_principal_id
                    AND product_session.oauth_state_digest IS NOT NULL
                    AND product_session.revoked_at IS NULL
                    AND product_session.revocation_reason IS NULL
                    AND final_clock < product_session.idle_expires_at
                    AND final_clock < product_session.absolute_expires_at
            )
        THEN
            RETURN QUERY SELECT 'access_denied',
                NULL::JSONB,
                NULL::BIGINT,
                NULL::JSONB,
                NULL::TEXT,
                NULL::BIGINT,
                NULL::TEXT,
                NULL::JSONB,
                final_clock;
            RETURN;
        END IF;
        IF NOT EXISTS (
            SELECT 1
            FROM public.automation_installations AS installation
            INNER JOIN public.automation_installation_authority_versions AS authority
                ON authority.tenant_id = installation.tenant_id
                AND authority.installation_id = installation.installation_id
                AND authority.revision = installation.current_authority_revision
            WHERE installation.tenant_id = expected_tenant_id
                AND installation.installation_id = expected_installation_id
                AND installation.lifecycle_state = 'active'
                AND installation.discord_application_id
                    = expected_discord_application_id
                AND installation.discord_guild_id = expected_guild_id
                AND installation.ruleset_key
                    = intent_document #>> '{authority,ruleset_key}'
                AND installation.current_authority_revision
                    = observed_current_authority_revision
                AND authority.authority_payload_digest
                    = observed_current_authority_payload_digest
        ) THEN
            RETURN QUERY SELECT 'scope_mismatch',
                NULL::JSONB,
                NULL::BIGINT,
                NULL::JSONB,
                NULL::TEXT,
                NULL::BIGINT,
                NULL::TEXT,
                NULL::JSONB,
                final_clock;
            RETURN;
        END IF;
        RETURN QUERY SELECT 'final_replay_required',
            promotion_row.record,
            NULL::BIGINT,
            NULL::JSONB,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::TEXT,
            NULL::JSONB,
            final_clock;
        RETURN;
    END IF;

    SELECT generation.*
    INTO generation_row
    FROM public.authoring_session_generations AS generation
    WHERE generation.tenant_id = expected_tenant_id
        AND generation.installation_id = expected_installation_id
        AND generation.session_id
            = intent_document #>> '{authority,session_id}'
        AND generation.generation = historical_generation;

    SELECT authority.*
    INTO historical_authority_row
    FROM public.automation_installation_authority_versions AS authority
    WHERE authority.tenant_id = expected_tenant_id
        AND authority.installation_id = expected_installation_id
        AND authority.revision = historical_authority_revision_value;

    calculated_candidate_hash := pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                'starring.intent.candidate_ruleset.v1',
                'UTF8'
            )
            || pg_catalog.decode('00', 'hex')
            || pg_catalog.convert_to(
                public.starring_canonical_json_v1(
                    intent_document -> 'definition'
                ),
                'UTF8'
            )
        ),
        'hex'
    );
    calculated_content_hash := public.starring_ruleset_content_hash_v1(
        registry_schema_version,
        intent_document -> 'definition'
    );

    IF generation_row.session_id IS NULL
        OR generation_row.stage <> 'preview_ready'
        OR generation_row.candidate_revision
            IS DISTINCT FROM historical_candidate_revision
        OR generation_row.candidate_hash
            IS DISTINCT FROM admission_payload ->> 'candidate_hash'
        OR generation_row.binding_fingerprint
            IS DISTINCT FROM admission_payload ->> 'binding_fingerprint'
        OR generation_row.installation_authority_revision
            IS DISTINCT FROM historical_authority_revision_value
        OR historical_authority_row.installation_id IS NULL
        OR pg_catalog.octet_length(historical_authority_row.resource_bindings::TEXT)
            > 262144
        OR generation_row.resource_bindings
            IS DISTINCT FROM historical_authority_row.resource_bindings
        OR generation_row.binding_fingerprint
            IS DISTINCT FROM historical_authority_row.binding_fingerprint
        OR historical_authority_row.authority_payload_digest
            IS DISTINCT FROM admission_payload ->> 'authority_payload_digest'
        OR historical_authority_row.binding_revision::TEXT
            IS DISTINCT FROM intent_document #>> '{authority,binding_revision}'
        OR historical_authority_row.policy_revision::TEXT
            IS DISTINCT FROM intent_document #>> '{authority,policy,revision}'
        OR historical_authority_row.required_approvals::TEXT
            IS DISTINCT FROM intent_document
                #>> '{authority,policy,required_approvals}'
        OR historical_authority_row.activation_ttl_seconds::TEXT
            IS DISTINCT FROM intent_document #>> '{authority,policy,ttl_seconds}'
        OR calculated_candidate_hash
            IS DISTINCT FROM admission_payload ->> 'candidate_hash'
        OR calculated_content_hash IS NULL
        OR calculated_content_hash
            IS DISTINCT FROM intent_document ->> 'expected_registry_content_hash'
        OR calculated_content_hash
            IS DISTINCT FROM publication_document ->> 'content_hash'
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::BIGINT,
            NULL::JSONB,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::TEXT,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    SELECT head.*
    INTO head_row
    FROM public.automation_ruleset_heads AS head
    WHERE head.guild_id = expected_guild_id
        AND head.ruleset_key = intent_document #>> '{authority,ruleset_key}'
    FOR SHARE;

    SELECT pg_catalog.count(*),
        pg_catalog.min(version.version),
        pg_catalog.max(version.version)
    INTO version_count, minimum_version, maximum_version
    FROM public.automation_ruleset_versions AS version
    WHERE version.guild_id = expected_guild_id
        AND version.ruleset_key = intent_document #>> '{authority,ruleset_key}';

    SELECT version.*
    INTO target_version_row
    FROM public.automation_ruleset_versions AS version
    WHERE version.guild_id = expected_guild_id
        AND version.ruleset_key = intent_document #>> '{authority,ruleset_key}'
        AND version.version = target_version;

    IF head_row.guild_id IS NULL
        OR head_row.next_version NOT BETWEEN 2 AND 4294967296
        OR version_count = 0
        OR minimum_version <> 1
        OR maximum_version <> head_row.next_version - 1
        OR version_count <> head_row.next_version - 1
        OR target_version_row.guild_id IS NULL
        OR target_version_row.schema_version IS DISTINCT FROM registry_schema_version
        OR target_version_row.definition
            IS DISTINCT FROM intent_document -> 'definition'
        OR target_version_row.content_hash IS DISTINCT FROM calculated_content_hash
        OR target_version_row.canonical_content_hash
            IS DISTINCT FROM calculated_content_hash
        OR target_version_row.created_by
            IS DISTINCT FROM publication_document ->> 'registry_created_by'
        OR (CASE
            WHEN target_version_row.created_by ~ '^[1-9][0-9]{0,19}$'
                THEN target_version_row.created_by::NUMERIC
                    <= 18446744073709551615
            ELSE FALSE
        END) IS DISTINCT FROM TRUE
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::BIGINT,
            NULL::JSONB,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::TEXT,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    SELECT activation.*
    INTO activation_row
    FROM public.automation_ruleset_activations AS activation
    WHERE activation.guild_id = expected_guild_id
        AND activation.ruleset_key
            = intent_document #>> '{authority,ruleset_key}'
    FOR SHARE;

    IF activation_row.guild_id IS NOT NULL THEN
        SELECT version.*
        INTO active_version_row
        FROM public.automation_ruleset_versions AS version
        WHERE version.guild_id = activation_row.guild_id
            AND version.ruleset_key = activation_row.ruleset_key
            AND version.version = activation_row.active_version;
        IF active_version_row.guild_id IS NULL
            OR active_version_row.content_hash IS NULL
            OR active_version_row.canonical_content_hash
                IS DISTINCT FROM active_version_row.content_hash
            OR NOT public.starring_product_ruleset_slot_exact_v1(
                expected_tenant_id,
                expected_installation_id,
                expected_guild_id,
                activation_row.ruleset_key,
                activation_row.active_version
            )
            OR (CASE
                WHEN active_version_row.created_by ~ '^[1-9][0-9]{0,19}$'
                    THEN active_version_row.created_by::NUMERIC
                        <= 18446744073709551615
                ELSE FALSE
            END) IS DISTINCT FROM TRUE
        THEN
            RETURN QUERY SELECT 'persistence_corrupt',
                NULL::JSONB,
                NULL::BIGINT,
                NULL::JSONB,
                NULL::TEXT,
                NULL::BIGINT,
                NULL::TEXT,
                NULL::JSONB,
                access_result.database_now;
            RETURN;
        END IF;
    END IF;

    final_clock := pg_catalog.clock_timestamp();
    IF authority_observed_at > final_clock
        OR final_clock >= authority_expires_at
        OR NOT EXISTS (
            SELECT 1
            FROM public.product_principals AS principal
            WHERE principal.principal_id = expected_principal_id
                AND NOT principal.disabled
                AND principal.discord_user_id = expected_acting_user_id
        )
        OR NOT EXISTS (
            SELECT 1
            FROM public.product_auth_sessions AS product_session
            WHERE product_session.session_digest
                    = expected_product_session_digest
                AND product_session.principal_id = expected_principal_id
                AND product_session.oauth_state_digest IS NOT NULL
                AND product_session.revoked_at IS NULL
                AND product_session.revocation_reason IS NULL
                AND final_clock < product_session.idle_expires_at
                AND final_clock < product_session.absolute_expires_at
        )
    THEN
        RETURN QUERY SELECT 'access_denied',
            NULL::JSONB,
            NULL::BIGINT,
            NULL::JSONB,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::TEXT,
            NULL::JSONB,
            final_clock;
        RETURN;
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM public.automation_installations AS installation
        INNER JOIN public.automation_installation_authority_versions AS authority
            ON authority.tenant_id = installation.tenant_id
            AND authority.installation_id = installation.installation_id
            AND authority.revision = installation.current_authority_revision
        WHERE installation.tenant_id = expected_tenant_id
            AND installation.installation_id = expected_installation_id
            AND installation.lifecycle_state = 'active'
            AND installation.discord_application_id
                = expected_discord_application_id
            AND installation.discord_guild_id = expected_guild_id
            AND installation.ruleset_key
                = intent_document #>> '{authority,ruleset_key}'
            AND installation.current_authority_revision
                = observed_current_authority_revision
            AND authority.authority_payload_digest
                = observed_current_authority_payload_digest
    ) THEN
        RETURN QUERY SELECT 'scope_mismatch',
            NULL::JSONB,
            NULL::BIGINT,
            NULL::JSONB,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::TEXT,
            NULL::JSONB,
            final_clock;
        RETURN;
    END IF;

    target_projection_document := pg_catalog.jsonb_build_object(
        'format_version', 1,
        'artifact', pg_catalog.jsonb_build_object(
            'guild_id', target_version_row.guild_id,
            'ruleset_key', target_version_row.ruleset_key,
            'version', target_version_row.version,
            'schema_version', target_version_row.schema_version,
            'definition', target_version_row.definition,
            'content_hash', target_version_row.content_hash,
            'created_by', target_version_row.created_by
        )
    );
    IF pg_catalog.octet_length(target_projection_document::TEXT) > 1048576 THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::BIGINT,
            NULL::JSONB,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::TEXT,
            NULL::JSONB,
            final_clock;
        RETURN;
    END IF;

    RETURN QUERY SELECT 'resolved',
        promotion_row.record,
        historical_authority_row.binding_revision,
        historical_authority_row.resource_bindings,
        historical_authority_row.binding_fingerprint,
        CASE
            WHEN activation_row.guild_id IS NULL THEN NULL::BIGINT
            ELSE active_version_row.version
        END,
        CASE
            WHEN activation_row.guild_id IS NULL THEN NULL::TEXT
            ELSE active_version_row.content_hash
        END,
        target_projection_document,
        final_clock;
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
DECLARE
    access_result RECORD;
    finalize_result RECORD;
    promotion_row public.authoring_promotions%ROWTYPE;
    authority_row public.automation_installation_authority_versions%ROWTYPE;
    head_row public.automation_ruleset_heads%ROWTYPE;
    target_version_row public.automation_ruleset_versions%ROWTYPE;
    pointer_row public.automation_ruleset_activations%ROWTYPE;
    active_version_row public.automation_ruleset_versions%ROWTYPE;
    activation_row public.activation_requests%ROWTYPE;
    admission_payload JSONB;
    intent_document JSONB;
    publication_document JSONB;
    proposal_document JSONB;
    context_document JSONB;
    expected_bindings JSONB;
    expected_baseline JSONB;
    expected_target JSONB;
    activation_request_document JSONB;
    activation_projection_document JSONB;
    journal_activation_document JSONB;
    persisted_record JSONB;
    receipt_document JSONB;
    audit_document JSONB;
    payload_document JSONB;
    hash_document JSONB;
    digest_input BYTEA;
    field_bytes BYTEA;
    domain_bytes BYTEA;
    calculated_request_id TEXT;
    calculated_policy_digest TEXT;
    calculated_binding_fingerprint TEXT;
    calculated_payload_digest TEXT;
    calculated_context_digest TEXT;
    calculated_content_hash TEXT;
    calculated_active_content_hash TEXT;
    activation_disposition TEXT;
    next_stage TEXT;
    mutation_clock TIMESTAMPTZ;
    created_clock TIMESTAMPTZ;
    expires_clock TIMESTAMPTZ;
    admitted_clock TIMESTAMPTZ;
    observed_clock TIMESTAMPTZ;
    admission_expires_clock TIMESTAMPTZ;
    policy_revision_value BIGINT;
    binding_revision_value BIGINT;
    required_approvals_value BIGINT;
    ttl_seconds_value BIGINT;
    target_version_value BIGINT;
    schema_version_value BIGINT;
    authority_revision_value BIGINT;
    generation_value BIGINT;
    permission_value NUMERIC;
    activation_count BIGINT;
    final_projection_count BIGINT;
    binding_value JSONB;
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
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    IF expected_promotion_id !~ '^[0-9a-f]{64}$'
        OR expected_promotion_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_promotion_request_digest !~ '^[0-9a-f]{64}$'
        OR expected_admission_digest !~ '^[0-9a-f]{64}$'
        OR pg_catalog.jsonb_typeof(activation_proposal) IS DISTINCT FROM 'object'
        OR pg_catalog.octet_length(activation_proposal::TEXT) > 1048576
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(activation_proposal) AS key(name)
        ) <> 2
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(activation_proposal) AS key(name)
            WHERE key.name NOT IN ('format_version', 'proposal')
        )
        OR activation_proposal ->> 'format_version' IS DISTINCT FROM '1'
        OR pg_catalog.jsonb_typeof(activation_proposal -> 'proposal')
            IS DISTINCT FROM 'object'
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;
    proposal_document := activation_proposal -> 'proposal';
    IF (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(proposal_document) AS key(name)
        ) <> 7
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(proposal_document) AS key(name)
            WHERE key.name NOT IN (
                'promotion_id', 'promotion_request_digest', 'expected_revision',
                'request_id', 'target', 'requester', 'approval_context'
            )
        )
        OR proposal_document ->> 'promotion_id'
            IS DISTINCT FROM expected_promotion_id
        OR proposal_document ->> 'promotion_request_digest'
            IS DISTINCT FROM expected_promotion_request_digest
        OR proposal_document ->> 'expected_revision'
            IS DISTINCT FROM expected_promotion_revision::TEXT
        OR (proposal_document ->> 'request_id' ~ '^[A-Za-z0-9_-]{1,64}$')
            IS DISTINCT FROM TRUE
        OR pg_catalog.jsonb_typeof(proposal_document -> 'target')
            IS DISTINCT FROM 'object'
        OR pg_catalog.jsonb_typeof(proposal_document -> 'approval_context')
            IS DISTINCT FROM 'object'
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    SELECT promotion.*
    INTO promotion_row
    FROM public.authoring_promotions AS promotion
    WHERE promotion.id = expected_promotion_id
    FOR UPDATE;
    access_result.database_now := pg_catalog.clock_timestamp();
    IF promotion_row.id IS NULL
        OR promotion_row.tenant_id IS DISTINCT FROM expected_tenant_id
        OR promotion_row.installation_id IS DISTINCT FROM expected_installation_id
        OR promotion_row.principal_id IS DISTINCT FROM expected_principal_id
    THEN
        RETURN QUERY SELECT 'scope_mismatch',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    intent_document := promotion_row.record -> 'intent';
    publication_document := promotion_row.record #> '{stage,publication}';
    IF promotion_row.record_format_version <> 1
        OR promotion_row.request_digest
            IS DISTINCT FROM expected_promotion_request_digest
        OR promotion_row.product_admission_format_version <> 1
        OR promotion_row.product_admission_digest
            IS DISTINCT FROM expected_admission_digest
        OR promotion_row.product_admission IS NULL
        OR pg_catalog.jsonb_typeof(promotion_row.record) IS DISTINCT FROM 'object'
        OR pg_catalog.octet_length(promotion_row.record::TEXT) > 8388608
        OR pg_catalog.jsonb_typeof(intent_document) IS DISTINCT FROM 'object'
        OR pg_catalog.jsonb_typeof(promotion_row.record -> 'stage')
            IS DISTINCT FROM 'object'
        OR pg_catalog.jsonb_typeof(publication_document) IS DISTINCT FROM 'object'
        OR promotion_row.record ->> 'id' IS DISTINCT FROM promotion_row.id
        OR promotion_row.record ->> 'revision'
            IS DISTINCT FROM promotion_row.revision::TEXT
        OR promotion_row.record ->> 'request_digest'
            IS DISTINCT FROM promotion_row.request_digest
        OR promotion_row.record #>> '{stage,state}'
            IS DISTINCT FROM promotion_row.stage
        OR promotion_row.record #>> '{intent,authority,tenant_id}'
            IS DISTINCT FROM expected_tenant_id
        OR promotion_row.record #>> '{intent,authority,installation_id}'
            IS DISTINCT FROM expected_installation_id
        OR promotion_row.record #>> '{intent,authority,principal_id}'
            IS DISTINCT FROM expected_principal_id
        OR promotion_row.record #>> '{intent,authority,session_owner_id}'
            IS DISTINCT FROM expected_principal_id
        OR promotion_row.record #>> '{intent,authority,guild_id}'
            IS DISTINCT FROM expected_guild_id
        OR promotion_row.record #>> '{intent,authority,requester}'
            IS DISTINCT FROM expected_acting_user_id
        OR promotion_row.record #>> '{intent,authority,ruleset_key}'
            !~ '^[A-Za-z0-9_-]{1,64}$'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(promotion_row.record) AS key(name)
        ) <> 7
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(promotion_row.record) AS key(name)
            WHERE key.name NOT IN (
                'id', 'revision', 'request_digest', 'intent', 'stage',
                'created_at', 'updated_at'
            )
        )
        OR pg_catalog.jsonb_typeof(promotion_row.product_admission)
            IS DISTINCT FROM 'object'
        OR pg_catalog.octet_length(promotion_row.product_admission::TEXT) > 32768
        OR promotion_row.product_admission ->> 'format_version' IS DISTINCT FROM '1'
        OR pg_catalog.jsonb_typeof(promotion_row.product_admission -> 'payload')
            IS DISTINCT FROM 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(promotion_row.product_admission) AS key(name)
        ) <> 3
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(promotion_row.product_admission) AS key(name)
            WHERE key.name NOT IN ('format_version', 'payload', 'admitted_at')
        )
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    admission_payload := promotion_row.product_admission -> 'payload';
    IF (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(admission_payload) AS key(name)
        ) <> 31
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(admission_payload) AS key(name)
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
        OR admission_payload ->> 'endpoint_domain'
            IS DISTINCT FROM 'product_promote_v1'
        OR admission_payload ->> 'tenant_id' IS DISTINCT FROM expected_tenant_id
        OR admission_payload ->> 'installation_id'
            IS DISTINCT FROM expected_installation_id
        OR admission_payload ->> 'principal_id'
            IS DISTINCT FROM expected_principal_id
        OR admission_payload ->> 'promotion_id'
            IS DISTINCT FROM expected_promotion_id
        OR admission_payload ->> 'promotion_request_digest'
            IS DISTINCT FROM expected_promotion_request_digest
        OR admission_payload ->> 'discord_application_id'
            IS DISTINCT FROM expected_discord_application_id
        OR admission_payload ->> 'guild_id' IS DISTINCT FROM expected_guild_id
        OR admission_payload ->> 'acting_user_id'
            IS DISTINCT FROM expected_acting_user_id
        OR admission_payload ->> 'capability' IS DISTINCT FROM 'promote'
        OR admission_payload ->> 'authoring_session_id'
            IS DISTINCT FROM intent_document #>> '{authority,session_id}'
        OR admission_payload ->> 'generation'
            IS DISTINCT FROM intent_document #>> '{authority,session_generation}'
        OR admission_payload ->> 'candidate_revision'
            IS DISTINCT FROM intent_document #>> '{evidence,candidate_revision}'
        OR admission_payload ->> 'candidate_hash'
            IS DISTINCT FROM intent_document #>> '{evidence,candidate_ruleset_hash}'
        OR admission_payload ->> 'binding_fingerprint'
            IS DISTINCT FROM intent_document #>> '{evidence,context_fingerprint}'
        OR admission_payload ->> 'policy_revision'
            IS DISTINCT FROM intent_document #>> '{authority,policy,revision}'
        OR (admission_payload ->> 'product_request_id'
            ~ '^[A-Za-z0-9_.:-]{1,128}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'session_subject_digest'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'idempotency_key_digest'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'idempotency_digest_key_id'
            ~ '^[A-Za-z0-9_.:-]{1,64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'idempotency_digest_key_fingerprint'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'semantic_request_digest'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'receipt_id' ~ '^[0-9a-f]{64}$')
            IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'audit_event_id' ~ '^[0-9a-f]{64}$')
            IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'authority_revision'
            ~ '^[1-9][0-9]{0,18}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'authority_payload_digest'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'authority_observation_digest'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'effective_permission_bits'
            ~ '^(0|[1-9][0-9]{0,19})$') IS DISTINCT FROM TRUE
        OR pg_catalog.jsonb_typeof(admission_payload -> 'guild_owner')
            IS DISTINCT FROM 'boolean'
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    BEGIN
        admitted_clock := (
            promotion_row.product_admission ->> 'admitted_at'
        )::TIMESTAMPTZ;
        observed_clock := (
            admission_payload ->> 'authority_observed_at'
        )::TIMESTAMPTZ;
        admission_expires_clock := (
            admission_payload ->> 'authority_expires_at'
        )::TIMESTAMPTZ;
        authority_revision_value := (
            admission_payload ->> 'authority_revision'
        )::BIGINT;
        generation_value := (admission_payload ->> 'generation')::BIGINT;
        permission_value := (
            admission_payload ->> 'effective_permission_bits'
        )::NUMERIC;
    EXCEPTION
        WHEN invalid_text_representation
            OR numeric_value_out_of_range
            OR datetime_field_overflow
        THEN
            RETURN QUERY SELECT 'persistence_corrupt',
                NULL::JSONB,
                NULL::JSONB,
                NULL::TEXT,
                NULL::JSONB,
                NULL::JSONB,
                NULL::JSONB,
                access_result.database_now;
            RETURN;
    END;
    IF admitted_clock > access_result.database_now
        OR observed_clock > admitted_clock
        OR admitted_clock >= admission_expires_clock
        OR admission_expires_clock > observed_clock + INTERVAL '5 seconds'
        OR authority_revision_value NOT BETWEEN 1 AND 9223372036854775807
        OR generation_value NOT BETWEEN 1 AND 9223372036854775807
        OR permission_value > 18446744073709551615
        OR NOT (
            (admission_payload ->> 'guild_owner')::BOOLEAN
            OR pg_catalog.mod(permission_value, 16) >= 8
            OR pg_catalog.mod(permission_value, 64) >= 32
        )
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    IF promotion_row.stage IN ('activation_pending', 'expired') THEN
        SELECT pg_catalog.count(*)
        INTO final_projection_count
        FROM public.product_action_receipts AS receipt
        INNER JOIN public.product_action_receipt_idempotency_aliases AS alias
            ON alias.tenant_id = receipt.tenant_id
            AND alias.installation_id = receipt.installation_id
            AND alias.principal_id = receipt.principal_id
            AND alias.endpoint_domain = receipt.endpoint_domain
            AND alias.idempotency_key_digest = receipt.idempotency_key_digest
            AND alias.idempotency_digest_key_id = receipt.idempotency_digest_key_id
            AND alias.idempotency_digest_key_fingerprint
                = receipt.idempotency_digest_key_fingerprint
            AND alias.receipt_id = receipt.receipt_id
        INNER JOIN public.product_audit_events AS audit
            ON audit.event_id = admission_payload ->> 'audit_event_id'
            AND audit.receipt_id = receipt.receipt_id
            AND audit.tenant_id = receipt.tenant_id
            AND audit.installation_id = receipt.installation_id
            AND audit.principal_id = receipt.principal_id
        INNER JOIN public.product_action_receipt_audit_evidence AS evidence
            ON evidence.event_id = audit.event_id
            AND evidence.receipt_id = receipt.receipt_id
        WHERE receipt.receipt_id = admission_payload ->> 'receipt_id'
            AND receipt.tenant_id = expected_tenant_id
            AND receipt.installation_id = expected_installation_id
            AND receipt.principal_id = expected_principal_id
            AND receipt.endpoint_domain = 'product_promote_v1'
            AND receipt.request_digest
                = admission_payload ->> 'semantic_request_digest'
            AND receipt.target_resource_type = 'authoring_promotion'
            AND receipt.target_resource_id = promotion_row.id
            AND receipt.resulting_revision = 3
            AND (
                promotion_row.stage = 'activation_pending'
                    AND promotion_row.revision = 3
                    AND receipt.resulting_state = 'activation_pending'
                OR promotion_row.stage = 'expired'
                    AND promotion_row.revision = 3
                    AND receipt.resulting_state = 'expired'
                OR promotion_row.stage = 'expired'
                    AND promotion_row.revision = 4
                    AND receipt.resulting_state = 'activation_pending'
            )
            AND receipt.result_code IN ('promotion_created', 'promotion_recovered')
            AND receipt.http_disposition_class = 2
            AND audit.action = 'promotion.promote'
            AND audit.request_id = admission_payload ->> 'product_request_id'
            AND pg_catalog.encode(audit.session_subject_digest, 'hex')
                = admission_payload ->> 'session_subject_digest'
            AND audit.authority_observation_digest
                = admission_payload ->> 'authority_observation_digest'
            AND audit.effective_permission_bits = permission_value
            AND audit.installation_authority_revision = authority_revision_value
            AND audit.expected_generation = generation_value
            AND audit.actual_generation = generation_value
            AND audit.payload_digest = promotion_row.request_digest
            AND audit.binding_fingerprint
                = admission_payload ->> 'binding_fingerprint'
            AND audit.policy_revision::TEXT
                = admission_payload ->> 'policy_revision'
            AND audit.resulting_state = receipt.resulting_state
            AND audit.result_code = receipt.result_code
            AND audit.dependency_latency_classes = '{}'::JSONB
            AND audit.occurred_at = receipt.completed_at
            AND evidence.endpoint_domain = receipt.endpoint_domain
            AND evidence.action = audit.action
            AND evidence.request_digest = receipt.request_digest
            AND evidence.target_resource_type = receipt.target_resource_type
            AND evidence.target_resource_id = receipt.target_resource_id
            AND evidence.resulting_revision = receipt.resulting_revision
            AND evidence.resulting_state = receipt.resulting_state
            AND evidence.result_code = receipt.result_code
            AND evidence.http_disposition_class = receipt.http_disposition_class
            AND evidence.completed_at = receipt.completed_at
            AND evidence.evidence_version = 1
            AND evidence.replay_policy_version = 1
            AND evidence.replay_guaranteed_until
                = receipt.completed_at + INTERVAL '168 hours';
        IF final_projection_count <> 1
            OR promotion_row.revision NOT IN (3, 4)
            OR promotion_row.stage = 'activation_pending' AND promotion_row.revision <> 3
        THEN
            RETURN QUERY SELECT 'persistence_corrupt',
                NULL::JSONB,
                NULL::JSONB,
                NULL::TEXT,
                NULL::JSONB,
                NULL::JSONB,
                NULL::JSONB,
                access_result.database_now;
            RETURN;
        END IF;
        mutation_clock := pg_catalog.clock_timestamp();
        IF authority_observed_at > mutation_clock
            OR mutation_clock >= authority_expires_at
            OR NOT EXISTS (
                SELECT 1
                FROM public.product_principals AS principal
                WHERE principal.principal_id = expected_principal_id
                    AND NOT principal.disabled
                    AND principal.discord_user_id = expected_acting_user_id
            )
            OR NOT EXISTS (
                SELECT 1
                FROM public.product_auth_sessions AS product_session
                WHERE product_session.session_digest = expected_product_session_digest
                    AND product_session.principal_id = expected_principal_id
                    AND product_session.oauth_state_digest IS NOT NULL
                    AND product_session.revoked_at IS NULL
                    AND product_session.revocation_reason IS NULL
                    AND mutation_clock < product_session.idle_expires_at
                    AND mutation_clock < product_session.absolute_expires_at
            )
        THEN
            RETURN QUERY SELECT 'access_denied',
                NULL::JSONB,
                NULL::JSONB,
                NULL::TEXT,
                NULL::JSONB,
                NULL::JSONB,
                NULL::JSONB,
                mutation_clock;
            RETURN;
        END IF;
        IF NOT EXISTS (
            SELECT 1
            FROM public.automation_installations AS installation
            INNER JOIN public.automation_installation_authority_versions AS authority
                ON authority.tenant_id = installation.tenant_id
                AND authority.installation_id = installation.installation_id
                AND authority.revision = installation.current_authority_revision
            WHERE installation.tenant_id = expected_tenant_id
                AND installation.installation_id = expected_installation_id
                AND installation.lifecycle_state = 'active'
                AND installation.discord_application_id
                    = expected_discord_application_id
                AND installation.discord_guild_id = expected_guild_id
                AND installation.ruleset_key
                    = intent_document #>> '{authority,ruleset_key}'
                AND installation.current_authority_revision
                    = observed_current_authority_revision
                AND authority.authority_payload_digest
                    = observed_current_authority_payload_digest
        ) THEN
            RETURN QUERY SELECT 'scope_mismatch',
                NULL::JSONB,
                NULL::JSONB,
                NULL::TEXT,
                NULL::JSONB,
                NULL::JSONB,
                NULL::JSONB,
                mutation_clock;
            RETURN;
        END IF;
        RETURN QUERY SELECT 'final_replay_required',
            promotion_row.record,
            promotion_row.product_admission,
            promotion_row.product_admission_digest,
            NULL::JSONB,
            NULL::JSONB,
            NULL::JSONB,
            mutation_clock;
        RETURN;
    END IF;

    IF promotion_row.stage <> 'published'
        OR promotion_row.revision <> 2
        OR expected_promotion_revision <> 2
        OR admitted_clock IS DISTINCT FROM (
            promotion_row.record ->> 'created_at'
        )::TIMESTAMPTZ
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(promotion_row.record -> 'stage') AS key(name)
        ) <> 2
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(promotion_row.record -> 'stage') AS key(name)
            WHERE key.name NOT IN ('state', 'publication')
        )
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    context_document := proposal_document -> 'approval_context';
    expected_target := pg_catalog.jsonb_build_object(
        'guild_id', expected_guild_id,
        'ruleset_key', intent_document #>> '{authority,ruleset_key}',
        'version', publication_document -> 'version',
        'content_hash', publication_document -> 'content_hash'
    );
    IF (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(proposal_document -> 'target') AS key(name)
        ) <> 4
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(proposal_document -> 'target') AS key(name)
            WHERE key.name NOT IN ('guild_id', 'ruleset_key', 'version', 'content_hash')
        )
        OR proposal_document -> 'target' IS DISTINCT FROM expected_target
        OR proposal_document ->> 'requester' IS DISTINCT FROM expected_acting_user_id
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(context_document) AS key(name)
        ) <> 7
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(context_document) AS key(name)
            WHERE key.name NOT IN (
                'promotion_id', 'promotion_request_digest',
                'approval_payload_digest', 'approval_context_digest',
                'binding', 'baseline', 'policy'
            )
        )
        OR context_document ->> 'promotion_id' IS DISTINCT FROM expected_promotion_id
        OR context_document ->> 'promotion_request_digest'
            IS DISTINCT FROM expected_promotion_request_digest
        OR (context_document ->> 'approval_payload_digest' ~ '^[0-9a-f]{64}$')
            IS DISTINCT FROM TRUE
        OR (context_document ->> 'approval_context_digest' ~ '^[0-9a-f]{64}$')
            IS DISTINCT FROM TRUE
        OR pg_catalog.jsonb_typeof(context_document -> 'binding')
            IS DISTINCT FROM 'object'
        OR pg_catalog.jsonb_typeof(context_document -> 'baseline')
            IS DISTINCT FROM 'object'
        OR pg_catalog.jsonb_typeof(context_document -> 'policy')
            IS DISTINCT FROM 'object'
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    BEGIN
        target_version_value := (publication_document ->> 'version')::BIGINT;
        schema_version_value := (publication_document ->> 'schema_version')::BIGINT;
        binding_revision_value := (
            context_document #>> '{binding,revision}'
        )::BIGINT;
        policy_revision_value := (
            context_document #>> '{policy,revision}'
        )::BIGINT;
        required_approvals_value := (
            context_document #>> '{policy,required_approvals}'
        )::BIGINT;
        ttl_seconds_value := (
            context_document #>> '{policy,ttl_seconds}'
        )::BIGINT;
    EXCEPTION
        WHEN invalid_text_representation OR numeric_value_out_of_range THEN
            RETURN QUERY SELECT 'persistence_corrupt',
                NULL::JSONB,
                NULL::JSONB,
                NULL::TEXT,
                NULL::JSONB,
                NULL::JSONB,
                NULL::JSONB,
                access_result.database_now;
            RETURN;
    END;
    IF target_version_value NOT BETWEEN 1 AND 4294967295
        OR schema_version_value NOT BETWEEN 1 AND 4294967295
        OR binding_revision_value NOT BETWEEN 1 AND 9223372036854775807
        OR policy_revision_value NOT BETWEEN 1 AND 9223372036854775807
        OR required_approvals_value NOT BETWEEN 1 AND 4294967295
        OR ttl_seconds_value NOT BETWEEN 1 AND 9223372036854775807
        OR context_document #>> '{binding,revision}'
            IS DISTINCT FROM intent_document #>> '{authority,binding_revision}'
        OR context_document #>> '{policy,revision}'
            IS DISTINCT FROM intent_document #>> '{authority,policy,revision}'
        OR context_document #>> '{policy,required_approvals}'
            IS DISTINCT FROM intent_document #>> '{authority,policy,required_approvals}'
        OR context_document #>> '{policy,ttl_seconds}'
            IS DISTINCT FROM intent_document #>> '{authority,policy,ttl_seconds}'
        OR context_document #>> '{policy,revision}'
            IS DISTINCT FROM admission_payload ->> 'policy_revision'
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    SELECT authority.*
    INTO authority_row
    FROM public.automation_installation_authority_versions AS authority
    WHERE authority.tenant_id = expected_tenant_id
        AND authority.installation_id = expected_installation_id
        AND authority.revision = authority_revision_value;
    IF authority_row.installation_id IS NULL
        OR authority_row.authority_payload_digest
            IS DISTINCT FROM admission_payload ->> 'authority_payload_digest'
        OR authority_row.binding_revision IS DISTINCT FROM binding_revision_value
        OR authority_row.policy_revision IS DISTINCT FROM policy_revision_value
        OR authority_row.required_approvals::BIGINT
            IS DISTINCT FROM required_approvals_value
        OR authority_row.activation_ttl_seconds
            IS DISTINCT FROM ttl_seconds_value
        OR authority_row.binding_fingerprint
            IS DISTINCT FROM admission_payload ->> 'binding_fingerprint'
        OR pg_catalog.octet_length(authority_row.resource_bindings::TEXT) > 262144
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    SELECT COALESCE(
        pg_catalog.jsonb_agg(
            pg_catalog.jsonb_build_object(
                'kind', 'channel',
                'key', required.key,
                'id', (authority_row.resource_bindings -> 'channel_bindings')
                    -> required.key
            ) ORDER BY required.ordinal
        ),
        '[]'::JSONB
    )
    INTO expected_bindings
    FROM pg_catalog.jsonb_array_elements_text(
        intent_document #> '{evidence,external_channel_bindings}'
    ) WITH ORDINALITY AS required(key, ordinal);
    IF pg_catalog.jsonb_typeof(
            intent_document #> '{evidence,external_channel_bindings}'
        ) IS DISTINCT FROM 'array'
        OR pg_catalog.jsonb_typeof(context_document #> '{binding,required_bindings}')
            IS DISTINCT FROM 'array'
        OR context_document #> '{binding,required_bindings}'
            IS DISTINCT FROM expected_bindings
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_array_elements(expected_bindings)
                AS binding(value)
            WHERE binding.value -> 'id' IS NULL
                OR pg_catalog.jsonb_typeof(binding.value -> 'id') <> 'string'
                OR (binding.value ->> 'key' ~ '^[A-Za-z0-9_-]{1,64}$')
                    IS DISTINCT FROM TRUE
                OR (binding.value ->> 'id' ~ '^[1-9][0-9]{0,19}$')
                    IS DISTINCT FROM TRUE
        )
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    domain_bytes := pg_catalog.convert_to(
        'starring.activation.approval_binding.v1', 'UTF8'
    ) || pg_catalog.decode('00', 'hex');
    digest_input := pg_catalog.int8send(pg_catalog.octet_length(domain_bytes))
        || domain_bytes;
    field_bytes := pg_catalog.convert_to(expected_guild_id, 'UTF8');
    digest_input := digest_input
        || pg_catalog.int8send(pg_catalog.octet_length(field_bytes)) || field_bytes;
    field_bytes := pg_catalog.int8send(binding_revision_value);
    digest_input := digest_input
        || pg_catalog.int8send(pg_catalog.octet_length(field_bytes)) || field_bytes;
    FOR binding_value IN
        SELECT binding.value
        FROM pg_catalog.jsonb_array_elements(expected_bindings) AS binding(value)
    LOOP
        FOR field_bytes IN
            SELECT pg_catalog.convert_to(field.value, 'UTF8')
            FROM pg_catalog.unnest(ARRAY[
                binding_value ->> 'kind',
                binding_value ->> 'key',
                binding_value ->> 'id'
            ]) AS field(value)
        LOOP
            digest_input := digest_input
                || pg_catalog.int8send(pg_catalog.octet_length(field_bytes))
                || field_bytes;
        END LOOP;
    END LOOP;
    calculated_binding_fingerprint := pg_catalog.encode(
        pg_catalog.sha256(digest_input), 'hex'
    );
    IF calculated_binding_fingerprint
        IS DISTINCT FROM context_document #>> '{binding,fingerprint}'
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    SELECT head.*
    INTO head_row
    FROM public.automation_ruleset_heads AS head
    WHERE head.guild_id = expected_guild_id
        AND head.ruleset_key = intent_document #>> '{authority,ruleset_key}'
    FOR SHARE;
    SELECT version.*
    INTO target_version_row
    FROM public.automation_ruleset_versions AS version
    WHERE version.guild_id = expected_guild_id
        AND version.ruleset_key = intent_document #>> '{authority,ruleset_key}'
        AND version.version = target_version_value;
    SELECT activation.*
    INTO pointer_row
    FROM public.automation_ruleset_activations AS activation
    WHERE activation.guild_id = expected_guild_id
        AND activation.ruleset_key = intent_document #>> '{authority,ruleset_key}'
    FOR SHARE;
    IF pointer_row.guild_id IS NOT NULL THEN
        SELECT version.*
        INTO active_version_row
        FROM public.automation_ruleset_versions AS version
        WHERE version.guild_id = pointer_row.guild_id
            AND version.ruleset_key = pointer_row.ruleset_key
            AND version.version = pointer_row.active_version;
        calculated_active_content_hash := public.starring_ruleset_content_hash_v1(
            active_version_row.schema_version,
            active_version_row.definition
        );
    END IF;
    calculated_content_hash := public.starring_ruleset_content_hash_v1(
        schema_version_value,
        intent_document -> 'definition'
    );
    IF head_row.guild_id IS NULL
        OR target_version_row.guild_id IS NULL
        OR target_version_row.schema_version IS DISTINCT FROM schema_version_value
        OR target_version_row.definition IS DISTINCT FROM intent_document -> 'definition'
        OR target_version_row.content_hash IS DISTINCT FROM calculated_content_hash
        OR target_version_row.canonical_content_hash
            IS DISTINCT FROM calculated_content_hash
        OR target_version_row.content_hash
            IS DISTINCT FROM publication_document ->> 'content_hash'
        OR pointer_row.guild_id IS NOT NULL AND (
            active_version_row.guild_id IS NULL
            OR active_version_row.content_hash IS NULL
            OR active_version_row.content_hash
                IS DISTINCT FROM calculated_active_content_hash
            OR active_version_row.canonical_content_hash
                IS DISTINCT FROM calculated_active_content_hash
            OR NOT public.starring_product_ruleset_slot_exact_v1(
                expected_tenant_id,
                expected_installation_id,
                expected_guild_id,
                intent_document #>> '{authority,ruleset_key}',
                pointer_row.active_version
            )
            OR (CASE
                WHEN active_version_row.created_by ~ '^[1-9][0-9]{0,19}$'
                    THEN active_version_row.created_by::NUMERIC
                        <= 18446744073709551615
                ELSE FALSE
            END) IS DISTINCT FROM TRUE
        )
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    expected_baseline := CASE
        WHEN pointer_row.guild_id IS NULL
            THEN pg_catalog.jsonb_build_object('state', 'absent')
        ELSE pg_catalog.jsonb_build_object(
            'state', 'exact',
            'version', active_version_row.version,
            'content_hash', active_version_row.content_hash
        )
    END;
    IF context_document -> 'baseline' IS DISTINCT FROM expected_baseline THEN
        mutation_clock := pg_catalog.clock_timestamp();
        IF authority_observed_at > mutation_clock
            OR mutation_clock >= authority_expires_at
            OR NOT EXISTS (
                SELECT 1
                FROM public.product_principals AS principal
                WHERE principal.principal_id = expected_principal_id
                    AND NOT principal.disabled
                    AND principal.discord_user_id = expected_acting_user_id
            )
            OR NOT EXISTS (
                SELECT 1
                FROM public.product_auth_sessions AS product_session
                WHERE product_session.session_digest = expected_product_session_digest
                    AND product_session.principal_id = expected_principal_id
                    AND product_session.oauth_state_digest IS NOT NULL
                    AND product_session.revoked_at IS NULL
                    AND product_session.revocation_reason IS NULL
                    AND mutation_clock < product_session.idle_expires_at
                    AND mutation_clock < product_session.absolute_expires_at
            )
        THEN
            RETURN QUERY SELECT 'access_denied',
                NULL::JSONB,
                NULL::JSONB,
                NULL::TEXT,
                NULL::JSONB,
                NULL::JSONB,
                NULL::JSONB,
                mutation_clock;
            RETURN;
        END IF;
        IF NOT EXISTS (
            SELECT 1
            FROM public.automation_installations AS installation
            INNER JOIN public.automation_installation_authority_versions AS authority
                ON authority.tenant_id = installation.tenant_id
                AND authority.installation_id = installation.installation_id
                AND authority.revision = installation.current_authority_revision
            WHERE installation.tenant_id = expected_tenant_id
                AND installation.installation_id = expected_installation_id
                AND installation.lifecycle_state = 'active'
                AND installation.discord_application_id
                    = expected_discord_application_id
                AND installation.discord_guild_id = expected_guild_id
                AND installation.ruleset_key
                    = intent_document #>> '{authority,ruleset_key}'
                AND installation.current_authority_revision
                    = observed_current_authority_revision
                AND authority.authority_payload_digest
                    = observed_current_authority_payload_digest
        ) THEN
            RETURN QUERY SELECT 'scope_mismatch',
                NULL::JSONB,
                NULL::JSONB,
                NULL::TEXT,
                NULL::JSONB,
                NULL::JSONB,
                NULL::JSONB,
                mutation_clock;
            RETURN;
        END IF;
        RETURN QUERY SELECT 'approval_environment_changed',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            NULL::JSONB,
            mutation_clock;
        RETURN;
    END IF;

    domain_bytes := pg_catalog.convert_to(
        'starring.activation.approval_policy.v1', 'UTF8'
    ) || pg_catalog.decode('00', 'hex');
    digest_input := pg_catalog.int8send(pg_catalog.octet_length(domain_bytes))
        || domain_bytes;
    FOR field_bytes IN
        SELECT field.value
        FROM pg_catalog.unnest(ARRAY[
            pg_catalog.int8send(policy_revision_value),
            pg_catalog.int4send(required_approvals_value::INTEGER),
            pg_catalog.int8send(ttl_seconds_value)
        ]) AS field(value)
    LOOP
        digest_input := digest_input
            || pg_catalog.int8send(pg_catalog.octet_length(field_bytes))
            || field_bytes;
    END LOOP;
    calculated_policy_digest := pg_catalog.encode(
        pg_catalog.sha256(digest_input), 'hex'
    );
    IF calculated_policy_digest
        IS DISTINCT FROM context_document #>> '{policy,digest}'
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    hash_document := pg_catalog.jsonb_build_object(
        'promotion_id', expected_promotion_id,
        'promotion_request_digest', expected_promotion_request_digest,
        'version', publication_document -> 'version',
        'schema_version', publication_document -> 'schema_version',
        'content_hash', publication_document -> 'content_hash'
    );
    domain_bytes := pg_catalog.convert_to(
        'starring.authoring_promotion.activation_request.v1', 'UTF8'
    ) || pg_catalog.decode('00', 'hex');
    field_bytes := pg_catalog.convert_to(
        public.starring_canonical_json_v1(hash_document), 'UTF8'
    );
    calculated_request_id := pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.int8send(pg_catalog.octet_length(domain_bytes))
            || domain_bytes
            || pg_catalog.int8send(pg_catalog.octet_length(field_bytes))
            || field_bytes
        ),
        'hex'
    );
    IF calculated_request_id IS DISTINCT FROM proposal_document ->> 'request_id' THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    payload_document := pg_catalog.jsonb_build_object(
        'format_version', 1,
        'promotion_id', expected_promotion_id,
        'promotion_request_digest', expected_promotion_request_digest,
        'authority', intent_document -> 'authority',
        'evidence', intent_document -> 'evidence',
        'definition', intent_document -> 'definition',
        'preview', intent_document -> 'preview',
        'publication', publication_document,
        'target', expected_target,
        'binding', context_document -> 'binding',
        'baseline', context_document -> 'baseline',
        'policy', context_document -> 'policy'
    );
    domain_bytes := pg_catalog.convert_to(
        'starring.authoring_promotion.approval_payload.v1', 'UTF8'
    ) || pg_catalog.decode('00', 'hex');
    field_bytes := pg_catalog.convert_to(
        public.starring_canonical_json_v1(payload_document), 'UTF8'
    );
    calculated_payload_digest := pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.int8send(pg_catalog.octet_length(domain_bytes))
            || domain_bytes
            || pg_catalog.int8send(pg_catalog.octet_length(field_bytes))
            || field_bytes
        ),
        'hex'
    );
    IF calculated_payload_digest
        IS DISTINCT FROM context_document ->> 'approval_payload_digest'
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    domain_bytes := pg_catalog.convert_to(
        'starring.activation.approval_context.v1', 'UTF8'
    ) || pg_catalog.decode('00', 'hex');
    digest_input := pg_catalog.int8send(pg_catalog.octet_length(domain_bytes))
        || domain_bytes;
    FOR field_bytes IN
        SELECT field.value
        FROM pg_catalog.unnest(ARRAY[
            pg_catalog.convert_to(proposal_document ->> 'request_id', 'UTF8'),
            pg_catalog.convert_to(expected_guild_id, 'UTF8'),
            pg_catalog.convert_to(intent_document #>> '{authority,ruleset_key}', 'UTF8'),
            pg_catalog.int4send(target_version_value::INTEGER),
            pg_catalog.convert_to(publication_document ->> 'content_hash', 'UTF8'),
            pg_catalog.convert_to(expected_acting_user_id, 'UTF8'),
            pg_catalog.convert_to(expected_promotion_id, 'UTF8'),
            pg_catalog.convert_to(expected_promotion_request_digest, 'UTF8'),
            pg_catalog.convert_to(calculated_payload_digest, 'UTF8'),
            pg_catalog.int8send(binding_revision_value),
            pg_catalog.convert_to(calculated_binding_fingerprint, 'UTF8')
        ]) AS field(value)
    LOOP
        digest_input := digest_input
            || pg_catalog.int8send(pg_catalog.octet_length(field_bytes))
            || field_bytes;
    END LOOP;
    FOR binding_value IN
        SELECT binding.value
        FROM pg_catalog.jsonb_array_elements(expected_bindings) AS binding(value)
    LOOP
        FOR field_bytes IN
            SELECT pg_catalog.convert_to(field.value, 'UTF8')
            FROM pg_catalog.unnest(ARRAY[
                binding_value ->> 'kind',
                binding_value ->> 'key',
                binding_value ->> 'id'
            ]) AS field(value)
        LOOP
            digest_input := digest_input
                || pg_catalog.int8send(pg_catalog.octet_length(field_bytes))
                || field_bytes;
        END LOOP;
    END LOOP;
    IF expected_baseline ->> 'state' = 'absent' THEN
        field_bytes := pg_catalog.convert_to('absent', 'UTF8');
        digest_input := digest_input
            || pg_catalog.int8send(pg_catalog.octet_length(field_bytes))
            || field_bytes;
    ELSE
        field_bytes := pg_catalog.convert_to('exact', 'UTF8');
        digest_input := digest_input
            || pg_catalog.int8send(pg_catalog.octet_length(field_bytes))
            || field_bytes;
        field_bytes := pg_catalog.int4send(active_version_row.version::INTEGER);
        digest_input := digest_input
            || pg_catalog.int8send(pg_catalog.octet_length(field_bytes))
            || field_bytes;
        field_bytes := pg_catalog.convert_to(active_version_row.content_hash, 'UTF8');
        digest_input := digest_input
            || pg_catalog.int8send(pg_catalog.octet_length(field_bytes))
            || field_bytes;
    END IF;
    FOR field_bytes IN
        SELECT field.value
        FROM pg_catalog.unnest(ARRAY[
            pg_catalog.int8send(policy_revision_value),
            pg_catalog.int4send(required_approvals_value::INTEGER),
            pg_catalog.int8send(ttl_seconds_value),
            pg_catalog.convert_to(calculated_policy_digest, 'UTF8')
        ]) AS field(value)
    LOOP
        digest_input := digest_input
            || pg_catalog.int8send(pg_catalog.octet_length(field_bytes))
            || field_bytes;
    END LOOP;
    calculated_context_digest := pg_catalog.encode(
        pg_catalog.sha256(digest_input), 'hex'
    );
    IF calculated_context_digest
        IS DISTINCT FROM context_document ->> 'approval_context_digest'
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    SELECT activation.*
    INTO activation_row
    FROM public.activation_requests AS activation
    WHERE activation.id = proposal_document ->> 'request_id'
    FOR UPDATE;
    mutation_clock := pg_catalog.clock_timestamp();
    IF authority_observed_at > mutation_clock
        OR mutation_clock >= authority_expires_at
        OR NOT EXISTS (
            SELECT 1
            FROM public.product_principals AS principal
            WHERE principal.principal_id = expected_principal_id
                AND NOT principal.disabled
                AND principal.discord_user_id = expected_acting_user_id
        )
        OR NOT EXISTS (
            SELECT 1
            FROM public.product_auth_sessions AS product_session
            WHERE product_session.session_digest = expected_product_session_digest
                AND product_session.principal_id = expected_principal_id
                AND product_session.oauth_state_digest IS NOT NULL
                AND product_session.revoked_at IS NULL
                AND product_session.revocation_reason IS NULL
                AND mutation_clock < product_session.idle_expires_at
                AND mutation_clock < product_session.absolute_expires_at
        )
    THEN
        RETURN QUERY SELECT 'access_denied',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            NULL::JSONB,
            mutation_clock;
        RETURN;
    END IF;
    IF NOT EXISTS (
        SELECT 1
        FROM public.automation_installations AS installation
        INNER JOIN public.automation_installation_authority_versions AS authority
            ON authority.tenant_id = installation.tenant_id
            AND authority.installation_id = installation.installation_id
            AND authority.revision = installation.current_authority_revision
        WHERE installation.tenant_id = expected_tenant_id
            AND installation.installation_id = expected_installation_id
            AND installation.lifecycle_state = 'active'
            AND installation.discord_application_id
                = expected_discord_application_id
            AND installation.discord_guild_id = expected_guild_id
            AND installation.ruleset_key
                = intent_document #>> '{authority,ruleset_key}'
            AND installation.current_authority_revision
                = observed_current_authority_revision
            AND authority.authority_payload_digest
                = observed_current_authority_payload_digest
    ) THEN
        RETURN QUERY SELECT 'scope_mismatch',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            NULL::JSONB,
            mutation_clock;
        RETURN;
    END IF;

    IF activation_row.id IS NULL THEN
        SELECT pg_catalog.count(*)
        INTO activation_count
        FROM public.activation_requests AS activation
        WHERE activation.promotion_id = expected_promotion_id;
        IF activation_count <> 0 THEN
            RETURN QUERY SELECT 'persistence_corrupt',
                NULL::JSONB,
                NULL::JSONB,
                NULL::TEXT,
                NULL::JSONB,
                NULL::JSONB,
                NULL::JSONB,
                mutation_clock;
            RETURN;
        END IF;
        created_clock := mutation_clock;
        expires_clock := mutation_clock + ttl_seconds_value * INTERVAL '1 second';
        INSERT INTO public.activation_requests (
            id, guild_id, ruleset_key, target_version, target_content_hash,
            requester_id, required_approvals, state, created_at, expires_at,
            observed_active_version, observed_active_hash, authority_kind,
            link_state_name, approval_context, link_state, promotion_id,
            promotion_request_digest, approval_payload_digest,
            approval_context_digest
        ) VALUES (
            proposal_document ->> 'request_id',
            expected_guild_id,
            intent_document #>> '{authority,ruleset_key}',
            target_version_value,
            publication_document ->> 'content_hash',
            expected_acting_user_id,
            required_approvals_value::INTEGER,
            'pending',
            created_clock,
            expires_clock,
            CASE WHEN pointer_row.guild_id IS NULL THEN NULL ELSE active_version_row.version END,
            CASE WHEN pointer_row.guild_id IS NULL THEN NULL ELSE active_version_row.content_hash END,
            'product_authoring',
            'unlinked',
            pg_catalog.jsonb_build_object(
                'authority', 'product_authoring',
                'context', context_document
            ),
            pg_catalog.jsonb_build_object('state', 'unlinked'),
            expected_promotion_id,
            expected_promotion_request_digest,
            calculated_payload_digest,
            calculated_context_digest
        );
        activation_disposition := 'created';
        next_stage := 'activation_pending';
    ELSE
        SELECT pg_catalog.count(*)
        INTO activation_count
        FROM public.activation_request_approvals AS approval
        WHERE approval.request_id = activation_row.id;
        IF activation_count <> 0
            OR activation_row.authority_kind <> 'product_authoring'
            OR activation_row.tenant_id IS DISTINCT FROM expected_tenant_id
            OR activation_row.installation_id IS DISTINCT FROM expected_installation_id
            OR activation_row.promotion_id IS DISTINCT FROM expected_promotion_id
            OR activation_row.promotion_request_digest
                IS DISTINCT FROM expected_promotion_request_digest
            OR activation_row.id IS DISTINCT FROM proposal_document ->> 'request_id'
            OR activation_row.guild_id IS DISTINCT FROM expected_guild_id
            OR activation_row.ruleset_key
                IS DISTINCT FROM intent_document #>> '{authority,ruleset_key}'
            OR activation_row.target_version IS DISTINCT FROM target_version_value
            OR activation_row.target_content_hash
                IS DISTINCT FROM publication_document ->> 'content_hash'
            OR activation_row.requester_id IS DISTINCT FROM expected_acting_user_id
            OR activation_row.required_approvals::BIGINT
                IS DISTINCT FROM required_approvals_value
            OR activation_row.approval_context IS DISTINCT FROM
                pg_catalog.jsonb_build_object(
                    'authority', 'product_authoring',
                    'context', context_document
                )
            OR activation_row.approval_payload_digest
                IS DISTINCT FROM calculated_payload_digest
            OR activation_row.approval_context_digest
                IS DISTINCT FROM calculated_context_digest
            OR activation_row.link_state_name <> 'unlinked'
            OR activation_row.link_state IS DISTINCT FROM
                pg_catalog.jsonb_build_object('state', 'unlinked')
            OR activation_row.linked_at IS NOT NULL
            OR activation_row.state NOT IN ('pending', 'expired')
            OR activation_row.apply_attempt_id IS NOT NULL
            OR activation_row.apply_attempt_no <> 0
            OR activation_row.apply_lease_until IS NOT NULL
            OR activation_row.last_apply_error IS NOT NULL
            OR activation_row.applied_at IS NOT NULL
            OR activation_row.applied_by IS NOT NULL
            OR activation_row.completion_kind IS NOT NULL
            OR activation_row.activation_notices IS NOT NULL
            OR activation_row.rejected_at IS NOT NULL
            OR activation_row.rejected_by IS NOT NULL
            OR activation_row.rejection_reason IS NOT NULL
            OR activation_row.termination IS NOT NULL
            OR activation_row.created_at < (
                promotion_row.record ->> 'updated_at'
            )::TIMESTAMPTZ
            OR activation_row.expires_at IS DISTINCT FROM
                activation_row.created_at + ttl_seconds_value * INTERVAL '1 second'
            OR activation_row.observed_active_version IS DISTINCT FROM
                (CASE
                    WHEN pointer_row.guild_id IS NULL THEN NULL::BIGINT
                    ELSE active_version_row.version
                END)
            OR activation_row.observed_active_hash IS DISTINCT FROM
                (CASE
                    WHEN pointer_row.guild_id IS NULL THEN NULL::TEXT
                    ELSE active_version_row.content_hash
                END)
        THEN
            RETURN QUERY SELECT 'persistence_corrupt',
                NULL::JSONB,
                NULL::JSONB,
                NULL::TEXT,
                NULL::JSONB,
                NULL::JSONB,
                NULL::JSONB,
                mutation_clock;
            RETURN;
        END IF;
        created_clock := activation_row.created_at;
        expires_clock := activation_row.expires_at;
        activation_disposition := 'reused';
        IF activation_row.state = 'expired'
            OR activation_row.expires_at <= mutation_clock
        THEN
            next_stage := 'expired';
            IF activation_row.state = 'pending' THEN
                UPDATE public.activation_requests AS activation
                SET state = 'expired'
                WHERE activation.id = activation_row.id
                    AND activation.state = 'pending'
                    AND activation.link_state_name = 'unlinked';
                IF NOT FOUND THEN
                    RAISE EXCEPTION 'product promotion activation expiry failed'
                        USING ERRCODE = '40001';
                END IF;
            END IF;
        ELSE
            next_stage := 'activation_pending';
        END IF;
    END IF;

    journal_activation_document := pg_catalog.jsonb_build_object(
        'request_id', proposal_document ->> 'request_id',
        'target', expected_target,
        'requester', expected_acting_user_id,
        'required_approvals', required_approvals_value,
        'observed_active', CASE
            WHEN pointer_row.guild_id IS NULL THEN NULL::JSONB
            ELSE pg_catalog.jsonb_build_object(
                'version', active_version_row.version,
                'content_hash', active_version_row.content_hash
            )
        END,
        'created_at', created_clock,
        'expires_at', expires_clock,
        'disposition', activation_disposition,
        'request_state_at_journal', CASE
            WHEN next_stage = 'expired' THEN 'expired'
            ELSE 'pending'
        END,
        'approval_context', context_document
    );
    persisted_record := promotion_row.record || pg_catalog.jsonb_build_object(
        'revision', 3,
        'stage', pg_catalog.jsonb_build_object(
            'state', next_stage,
            'publication', publication_document,
            'activation', journal_activation_document
        ),
        'updated_at', mutation_clock
    );
    IF pg_catalog.octet_length(persisted_record::TEXT) > 8388608 THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            NULL::JSONB,
            mutation_clock;
        RETURN;
    END IF;
    UPDATE public.authoring_promotions AS promotion
    SET revision = 3,
        stage = next_stage,
        record = persisted_record
    WHERE promotion.id = promotion_row.id
        AND promotion.revision = 2
        AND promotion.stage = 'published';
    IF NOT FOUND THEN
        RAISE EXCEPTION 'product promotion activation journal advance failed'
            USING ERRCODE = '40001';
    END IF;

    IF next_stage = 'activation_pending' THEN
        UPDATE public.activation_requests AS activation
        SET link_state_name = 'linked',
            link_state = pg_catalog.jsonb_build_object(
                'state', 'linked',
                'linked_at', mutation_clock
            ),
            linked_at = mutation_clock
        WHERE activation.id = proposal_document ->> 'request_id'
            AND activation.state = 'pending'
            AND activation.link_state_name = 'unlinked'
            AND activation.expires_at > mutation_clock;
        IF NOT FOUND THEN
            RAISE EXCEPTION 'product promotion activation link failed'
                USING ERRCODE = '40001';
        END IF;
    END IF;
    SELECT activation.*
    INTO activation_row
    FROM public.activation_requests AS activation
    WHERE activation.id = proposal_document ->> 'request_id';
    activation_request_document := pg_catalog.jsonb_build_object(
        'id', activation_row.id,
        'target', pg_catalog.jsonb_build_object(
            'guild_id', activation_row.guild_id,
            'ruleset_key', activation_row.ruleset_key,
            'version', activation_row.target_version,
            'content_hash', activation_row.target_content_hash
        ),
        'requester', activation_row.requester_id,
        'required_approvals', activation_row.required_approvals,
        'approval_context', activation_row.approval_context,
        'link_state', activation_row.link_state,
        'approvals', '[]'::JSONB,
        'state', activation_row.state,
        'rejection', NULL,
        'apply_attempt_id', NULL,
        'apply_attempt_no', activation_row.apply_attempt_no,
        'apply_lease_until', NULL,
        'last_apply_error', NULL,
        'observed_active', CASE
            WHEN activation_row.observed_active_version IS NULL THEN NULL::JSONB
            ELSE pg_catalog.jsonb_build_object(
                'version', activation_row.observed_active_version,
                'content_hash', activation_row.observed_active_hash
            )
        END,
        'completion', NULL,
        'termination', NULL,
        'created_at', activation_row.created_at,
        'expires_at', activation_row.expires_at
    );
    activation_projection_document := pg_catalog.jsonb_build_object(
        'format_version', 1,
        'disposition', activation_disposition,
        'request', activation_request_document
    );
    receipt_document := pg_catalog.jsonb_build_object(
        'format_version', 1,
        'receipt_id', admission_payload ->> 'receipt_id',
        'tenant_id', expected_tenant_id,
        'installation_id', expected_installation_id,
        'principal_id', expected_principal_id,
        'endpoint_domain', 'product_promote_v1',
        'idempotency_key_digest', admission_payload ->> 'idempotency_key_digest',
        'idempotency_digest_key_id', admission_payload ->> 'idempotency_digest_key_id',
        'idempotency_digest_key_fingerprint',
            admission_payload ->> 'idempotency_digest_key_fingerprint',
        'request_digest', admission_payload ->> 'semantic_request_digest',
        'target_resource_type', 'authoring_promotion',
        'target_resource_id', expected_promotion_id,
        'resulting_revision', 3,
        'resulting_state', next_stage,
        'result_code', 'promotion_created',
        'http_disposition_class', 2,
        'completed_at', mutation_clock
    );
    audit_document := pg_catalog.jsonb_build_object(
        'format_version', 1,
        'event_id', admission_payload ->> 'audit_event_id',
        'receipt_id', admission_payload ->> 'receipt_id',
        'tenant_id', expected_tenant_id,
        'installation_id', expected_installation_id,
        'principal_id', expected_principal_id,
        'session_subject_digest', admission_payload ->> 'session_subject_digest',
        'action', 'promotion.promote',
        'target_resource_type', 'authoring_promotion',
        'target_resource_id', expected_promotion_id,
        'request_id', admission_payload ->> 'product_request_id',
        'authority_observation_digest',
            admission_payload ->> 'authority_observation_digest',
        'effective_permission_bits', admission_payload ->> 'effective_permission_bits',
        'authority_observed_at', admission_payload -> 'authority_observed_at',
        'installation_authority_revision', authority_revision_value,
        'expected_generation', generation_value,
        'actual_generation', generation_value,
        'payload_digest', expected_promotion_request_digest,
        'binding_fingerprint', admission_payload ->> 'binding_fingerprint',
        'policy_revision', policy_revision_value,
        'active_baseline_version', CASE
            WHEN pointer_row.guild_id IS NULL THEN NULL::BIGINT
            ELSE active_version_row.version
        END,
        'active_baseline_hash', CASE
            WHEN pointer_row.guild_id IS NULL THEN NULL::TEXT
            ELSE active_version_row.content_hash
        END,
        'resulting_state', next_stage,
        'result_code', 'promotion_created',
        'dependency_latency_classes', '{}'::JSONB,
        'occurred_at', mutation_clock,
        'endpoint_domain', 'product_promote_v1',
        'request_digest', admission_payload ->> 'semantic_request_digest',
        'resulting_revision', 3,
        'http_disposition_class', 2,
        'completed_at', mutation_clock,
        'evidence_version', 1,
        'replay_policy_version', 1,
        'replay_guaranteed_until', mutation_clock + INTERVAL '168 hours'
    );
    IF pg_catalog.octet_length(activation_projection_document::TEXT) > 1048576
        OR pg_catalog.octet_length(receipt_document::TEXT) > 65536
        OR pg_catalog.octet_length(audit_document::TEXT) > 65536
    THEN
        RAISE EXCEPTION 'product promotion activation projection is too large'
            USING ERRCODE = '23514';
    END IF;
    SELECT *
    INTO finalize_result
    FROM public.starring_product_promotion_finalize_receipt_v1(
        promotion_row.product_admission,
        persisted_record,
        activation_projection_document,
        receipt_document,
        audit_document
    );
    IF finalize_result.outcome_code <> 'created' THEN
        RAISE EXCEPTION 'product promotion receipt finalization failed: %',
            finalize_result.outcome_code
            USING ERRCODE = '23514';
    END IF;
    RETURN QUERY SELECT activation_disposition,
        persisted_record,
        promotion_row.product_admission,
        promotion_row.product_admission_digest,
        activation_projection_document,
        receipt_document,
        audit_document,
        mutation_clock;
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
        ) = NEW.product_admission_digest
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
    FOR SHARE;

    SELECT product_session.*
    INTO product_session_row
    FROM public.product_auth_sessions AS product_session
    WHERE product_session.session_digest = expected_product_session_digest
        AND product_session.principal_id = expected_principal_id
    FOR SHARE;

    IF principal_row.principal_id IS NULL
        OR product_session_row.principal_id IS NULL
        OR principal_row.disabled
        OR principal_row.discord_user_id IS DISTINCT FROM expected_acting_user_id
        OR product_session_row.oauth_state_digest IS NULL
        OR product_session_row.revoked_at IS NOT NULL
        OR product_session_row.revocation_reason IS NOT NULL
    THEN
        RETURN QUERY SELECT 'access_denied', current_clock, NULL::JSONB;
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

    current_clock := pg_catalog.clock_timestamp();
    IF current_clock >= product_session_row.idle_expires_at
        OR current_clock >= product_session_row.absolute_expires_at
    THEN
        RETURN QUERY SELECT 'access_denied', current_clock, NULL::JSONB;
        RETURN;
    END IF;

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
DECLARE
    admission_payload JSONB;
    activation_request JSONB;
    completed_clock TIMESTAMPTZ;
    authority_clock TIMESTAMPTZ;
    replay_until TIMESTAMPTZ;
    resulting_revision_value BIGINT;
    authority_revision_value BIGINT;
    generation_value BIGINT;
    permission_value NUMERIC;
    evidence_count BIGINT;
BEGIN
    IF pg_catalog.jsonb_typeof(admission_projection) IS DISTINCT FROM 'object'
        OR pg_catalog.jsonb_typeof(promotion_projection) IS DISTINCT FROM 'object'
        OR pg_catalog.jsonb_typeof(activation_projection) IS DISTINCT FROM 'object'
        OR pg_catalog.jsonb_typeof(receipt_projection) IS DISTINCT FROM 'object'
        OR pg_catalog.jsonb_typeof(audit_projection) IS DISTINCT FROM 'object'
        OR pg_catalog.octet_length(admission_projection::TEXT) > 32768
        OR pg_catalog.octet_length(promotion_projection::TEXT) > 8388608
        OR pg_catalog.octet_length(activation_projection::TEXT) > 1048576
        OR pg_catalog.octet_length(receipt_projection::TEXT) > 65536
        OR pg_catalog.octet_length(audit_projection::TEXT) > 65536
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(admission_projection) AS key(name)
        ) <> 3
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(admission_projection) AS key(name)
            WHERE key.name NOT IN ('format_version', 'payload', 'admitted_at')
        )
        OR admission_projection ->> 'format_version' IS DISTINCT FROM '1'
        OR pg_catalog.jsonb_typeof(admission_projection -> 'payload')
            IS DISTINCT FROM 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(promotion_projection) AS key(name)
        ) <> 7
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(promotion_projection) AS key(name)
            WHERE key.name NOT IN (
                'id', 'revision', 'request_digest', 'intent', 'stage',
                'created_at', 'updated_at'
            )
        )
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(activation_projection) AS key(name)
        ) <> 3
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(activation_projection) AS key(name)
            WHERE key.name NOT IN ('format_version', 'disposition', 'request')
        )
        OR activation_projection ->> 'format_version' IS DISTINCT FROM '1'
        OR activation_projection ->> 'disposition' NOT IN ('created', 'reused')
        OR pg_catalog.jsonb_typeof(activation_projection -> 'request')
            IS DISTINCT FROM 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(receipt_projection) AS key(name)
        ) <> 17
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(receipt_projection) AS key(name)
            WHERE key.name NOT IN (
                'format_version', 'receipt_id', 'tenant_id', 'installation_id',
                'principal_id', 'endpoint_domain', 'idempotency_key_digest',
                'idempotency_digest_key_id',
                'idempotency_digest_key_fingerprint', 'request_digest',
                'target_resource_type', 'target_resource_id',
                'resulting_revision', 'resulting_state', 'result_code',
                'http_disposition_class', 'completed_at'
            )
        )
        OR receipt_projection ->> 'format_version' IS DISTINCT FROM '1'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(audit_projection) AS key(name)
        ) <> 34
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(audit_projection) AS key(name)
            WHERE key.name NOT IN (
                'format_version', 'event_id', 'receipt_id', 'tenant_id',
                'installation_id', 'principal_id', 'session_subject_digest',
                'action', 'target_resource_type', 'target_resource_id',
                'request_id', 'authority_observation_digest',
                'effective_permission_bits', 'authority_observed_at',
                'installation_authority_revision', 'expected_generation',
                'actual_generation', 'payload_digest', 'binding_fingerprint',
                'policy_revision', 'active_baseline_version',
                'active_baseline_hash', 'resulting_state', 'result_code',
                'dependency_latency_classes', 'occurred_at', 'endpoint_domain',
                'request_digest', 'resulting_revision',
                'http_disposition_class', 'completed_at', 'evidence_version',
                'replay_policy_version', 'replay_guaranteed_until'
            )
        )
        OR audit_projection ->> 'format_version' IS DISTINCT FROM '1'
    THEN
        RETURN QUERY SELECT 'persistence_corrupt';
        RETURN;
    END IF;

    admission_payload := admission_projection -> 'payload';
    activation_request := activation_projection -> 'request';
    IF (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(admission_payload) AS key(name)
        ) <> 31
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(admission_payload) AS key(name)
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
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(activation_request) AS key(name)
        ) <> 18
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(activation_request) AS key(name)
            WHERE key.name NOT IN (
                'id', 'target', 'requester', 'required_approvals',
                'approval_context', 'link_state', 'approvals', 'state',
                'rejection', 'apply_attempt_id', 'apply_attempt_no',
                'apply_lease_until', 'last_apply_error', 'observed_active',
                'completion', 'termination', 'created_at', 'expires_at'
            )
        )
        OR admission_payload ->> 'endpoint_domain'
            IS DISTINCT FROM 'product_promote_v1'
        OR promotion_projection ->> 'id'
            IS DISTINCT FROM admission_payload ->> 'promotion_id'
        OR promotion_projection ->> 'request_digest'
            IS DISTINCT FROM admission_payload ->> 'promotion_request_digest'
        OR promotion_projection ->> 'revision' IS DISTINCT FROM '3'
        OR promotion_projection #>> '{stage,state}'
            NOT IN ('activation_pending', 'expired')
        OR activation_request ->> 'id'
            IS DISTINCT FROM promotion_projection #>> '{stage,activation,request_id}'
        OR activation_request -> 'target'
            IS DISTINCT FROM promotion_projection #> '{stage,activation,target}'
        OR activation_request ->> 'requester'
            IS DISTINCT FROM promotion_projection #>> '{stage,activation,requester}'
        OR activation_request ->> 'required_approvals'
            IS DISTINCT FROM promotion_projection
                #>> '{stage,activation,required_approvals}'
        OR activation_request -> 'approval_context' #>> '{authority}'
            IS DISTINCT FROM 'product_authoring'
        OR activation_request #> '{approval_context,context}'
            IS DISTINCT FROM promotion_projection
                #> '{stage,activation,approval_context}'
        OR activation_request ->> 'state'
            IS DISTINCT FROM promotion_projection
                #>> '{stage,activation,request_state_at_journal}'
        OR activation_request ->> 'created_at'
            IS DISTINCT FROM promotion_projection #>> '{stage,activation,created_at}'
        OR activation_request ->> 'expires_at'
            IS DISTINCT FROM promotion_projection #>> '{stage,activation,expires_at}'
        OR activation_request -> 'approvals' IS DISTINCT FROM '[]'::JSONB
        OR activation_request -> 'rejection' IS DISTINCT FROM 'null'::JSONB
        OR activation_request -> 'apply_attempt_id' IS DISTINCT FROM 'null'::JSONB
        OR activation_request ->> 'apply_attempt_no' IS DISTINCT FROM '0'
        OR activation_request -> 'apply_lease_until' IS DISTINCT FROM 'null'::JSONB
        OR activation_request -> 'last_apply_error' IS DISTINCT FROM 'null'::JSONB
        OR activation_request -> 'completion' IS DISTINCT FROM 'null'::JSONB
        OR activation_request -> 'termination' IS DISTINCT FROM 'null'::JSONB
        OR admission_payload ->> 'receipt_id'
            IS DISTINCT FROM receipt_projection ->> 'receipt_id'
        OR admission_payload ->> 'audit_event_id'
            IS DISTINCT FROM audit_projection ->> 'event_id'
        OR receipt_projection ->> 'receipt_id'
            IS DISTINCT FROM audit_projection ->> 'receipt_id'
        OR admission_payload ->> 'tenant_id'
            IS DISTINCT FROM receipt_projection ->> 'tenant_id'
        OR admission_payload ->> 'installation_id'
            IS DISTINCT FROM receipt_projection ->> 'installation_id'
        OR admission_payload ->> 'principal_id'
            IS DISTINCT FROM receipt_projection ->> 'principal_id'
        OR receipt_projection ->> 'tenant_id'
            IS DISTINCT FROM audit_projection ->> 'tenant_id'
        OR receipt_projection ->> 'installation_id'
            IS DISTINCT FROM audit_projection ->> 'installation_id'
        OR receipt_projection ->> 'principal_id'
            IS DISTINCT FROM audit_projection ->> 'principal_id'
        OR receipt_projection ->> 'endpoint_domain'
            IS DISTINCT FROM 'product_promote_v1'
        OR receipt_projection ->> 'idempotency_key_digest'
            IS DISTINCT FROM admission_payload ->> 'idempotency_key_digest'
        OR receipt_projection ->> 'idempotency_digest_key_id'
            IS DISTINCT FROM admission_payload ->> 'idempotency_digest_key_id'
        OR receipt_projection ->> 'idempotency_digest_key_fingerprint'
            IS DISTINCT FROM admission_payload
                ->> 'idempotency_digest_key_fingerprint'
        OR receipt_projection ->> 'request_digest'
            IS DISTINCT FROM admission_payload ->> 'semantic_request_digest'
        OR receipt_projection ->> 'target_resource_type'
            IS DISTINCT FROM 'authoring_promotion'
        OR receipt_projection ->> 'target_resource_id'
            IS DISTINCT FROM promotion_projection ->> 'id'
        OR receipt_projection ->> 'resulting_revision' IS DISTINCT FROM '3'
        OR receipt_projection ->> 'resulting_state'
            IS DISTINCT FROM promotion_projection #>> '{stage,state}'
        OR receipt_projection ->> 'result_code'
            IS DISTINCT FROM 'promotion_created'
        OR receipt_projection ->> 'http_disposition_class' IS DISTINCT FROM '2'
        OR audit_projection ->> 'session_subject_digest'
            IS DISTINCT FROM admission_payload ->> 'session_subject_digest'
        OR audit_projection ->> 'action' IS DISTINCT FROM 'promotion.promote'
        OR audit_projection ->> 'target_resource_type'
            IS DISTINCT FROM 'authoring_promotion'
        OR audit_projection ->> 'target_resource_id'
            IS DISTINCT FROM promotion_projection ->> 'id'
        OR audit_projection ->> 'request_id'
            IS DISTINCT FROM admission_payload ->> 'product_request_id'
        OR audit_projection ->> 'authority_observation_digest'
            IS DISTINCT FROM admission_payload ->> 'authority_observation_digest'
        OR audit_projection ->> 'effective_permission_bits'
            IS DISTINCT FROM admission_payload ->> 'effective_permission_bits'
        OR audit_projection ->> 'authority_observed_at'
            IS DISTINCT FROM admission_payload ->> 'authority_observed_at'
        OR audit_projection ->> 'installation_authority_revision'
            IS DISTINCT FROM admission_payload ->> 'authority_revision'
        OR audit_projection ->> 'expected_generation'
            IS DISTINCT FROM admission_payload ->> 'generation'
        OR audit_projection ->> 'actual_generation'
            IS DISTINCT FROM admission_payload ->> 'generation'
        OR audit_projection ->> 'payload_digest'
            IS DISTINCT FROM promotion_projection ->> 'request_digest'
        OR audit_projection ->> 'binding_fingerprint'
            IS DISTINCT FROM admission_payload ->> 'binding_fingerprint'
        OR audit_projection ->> 'policy_revision'
            IS DISTINCT FROM admission_payload ->> 'policy_revision'
        OR activation_request -> 'observed_active' = 'null'::JSONB
            AND (
                audit_projection -> 'active_baseline_version'
                    IS DISTINCT FROM 'null'::JSONB
                OR audit_projection -> 'active_baseline_hash'
                    IS DISTINCT FROM 'null'::JSONB
            )
        OR activation_request -> 'observed_active' <> 'null'::JSONB
            AND (
                audit_projection -> 'active_baseline_version'
                    IS DISTINCT FROM activation_request
                        #> '{observed_active,version}'
                OR audit_projection -> 'active_baseline_hash'
                    IS DISTINCT FROM activation_request
                        #> '{observed_active,content_hash}'
            )
        OR audit_projection ->> 'resulting_state'
            IS DISTINCT FROM receipt_projection ->> 'resulting_state'
        OR audit_projection ->> 'result_code'
            IS DISTINCT FROM receipt_projection ->> 'result_code'
        OR audit_projection -> 'dependency_latency_classes'
            IS DISTINCT FROM '{}'::JSONB
        OR audit_projection ->> 'endpoint_domain'
            IS DISTINCT FROM receipt_projection ->> 'endpoint_domain'
        OR audit_projection ->> 'request_digest'
            IS DISTINCT FROM receipt_projection ->> 'request_digest'
        OR audit_projection -> 'resulting_revision'
            IS DISTINCT FROM receipt_projection -> 'resulting_revision'
        OR audit_projection ->> 'http_disposition_class'
            IS DISTINCT FROM receipt_projection ->> 'http_disposition_class'
        OR audit_projection ->> 'completed_at'
            IS DISTINCT FROM receipt_projection ->> 'completed_at'
        OR audit_projection ->> 'occurred_at'
            IS DISTINCT FROM receipt_projection ->> 'completed_at'
        OR audit_projection ->> 'evidence_version' IS DISTINCT FROM '1'
        OR audit_projection ->> 'replay_policy_version' IS DISTINCT FROM '1'
    THEN
        RETURN QUERY SELECT 'persistence_corrupt';
        RETURN;
    END IF;

    BEGIN
        completed_clock := (receipt_projection ->> 'completed_at')::TIMESTAMPTZ;
        authority_clock := (audit_projection ->> 'authority_observed_at')::TIMESTAMPTZ;
        replay_until := (audit_projection ->> 'replay_guaranteed_until')::TIMESTAMPTZ;
        resulting_revision_value := (
            receipt_projection ->> 'resulting_revision'
        )::BIGINT;
        authority_revision_value := (
            audit_projection ->> 'installation_authority_revision'
        )::BIGINT;
        generation_value := (audit_projection ->> 'expected_generation')::BIGINT;
        permission_value := (
            audit_projection ->> 'effective_permission_bits'
        )::NUMERIC;
    EXCEPTION
        WHEN invalid_text_representation
            OR numeric_value_out_of_range
            OR datetime_field_overflow
        THEN
            RETURN QUERY SELECT 'persistence_corrupt';
            RETURN;
    END;

    IF completed_clock IS DISTINCT FROM (
            promotion_projection ->> 'updated_at'
        )::TIMESTAMPTZ
        OR authority_clock > completed_clock
        OR replay_until IS DISTINCT FROM completed_clock + INTERVAL '168 hours'
        OR resulting_revision_value <> 3
        OR authority_revision_value NOT BETWEEN 1 AND 9223372036854775807
        OR generation_value NOT BETWEEN 1 AND 9223372036854775807
        OR permission_value < 0
        OR permission_value > 18446744073709551615
        OR EXISTS (
            SELECT 1
            FROM public.product_action_receipts AS receipt
            WHERE receipt.receipt_id = receipt_projection ->> 'receipt_id'
                OR receipt.endpoint_domain = 'product_promote_v1'
                    AND receipt.target_resource_type = 'authoring_promotion'
                    AND receipt.target_resource_id = promotion_projection ->> 'id'
        )
        OR EXISTS (
            SELECT 1
            FROM public.product_action_receipt_idempotency_aliases AS alias
            WHERE alias.tenant_id = receipt_projection ->> 'tenant_id'
                AND alias.installation_id = receipt_projection ->> 'installation_id'
                AND alias.principal_id = receipt_projection ->> 'principal_id'
                AND alias.endpoint_domain = 'product_promote_v1'
                AND alias.idempotency_key_digest
                    = receipt_projection ->> 'idempotency_key_digest'
        )
        OR EXISTS (
            SELECT 1
            FROM public.product_audit_events AS audit
            WHERE audit.event_id = audit_projection ->> 'event_id'
                OR audit.receipt_id = receipt_projection ->> 'receipt_id'
                OR audit.tenant_id = receipt_projection ->> 'tenant_id'
                    AND audit.request_id = audit_projection ->> 'request_id'
        )
        OR EXISTS (
            SELECT 1
            FROM public.product_action_receipt_audit_evidence AS evidence
            WHERE evidence.event_id = audit_projection ->> 'event_id'
                OR evidence.receipt_id = receipt_projection ->> 'receipt_id'
        )
    THEN
        RETURN QUERY SELECT 'persistence_corrupt';
        RETURN;
    END IF;

    INSERT INTO public.product_action_receipts (
        receipt_id, tenant_id, installation_id, principal_id, endpoint_domain,
        idempotency_key_digest, idempotency_digest_key_id,
        idempotency_digest_key_fingerprint, request_digest,
        target_resource_type, target_resource_id, resulting_revision,
        resulting_state, result_code, http_disposition_class, completed_at
    ) VALUES (
        receipt_projection ->> 'receipt_id',
        receipt_projection ->> 'tenant_id',
        receipt_projection ->> 'installation_id',
        receipt_projection ->> 'principal_id',
        receipt_projection ->> 'endpoint_domain',
        receipt_projection ->> 'idempotency_key_digest',
        receipt_projection ->> 'idempotency_digest_key_id',
        receipt_projection ->> 'idempotency_digest_key_fingerprint',
        receipt_projection ->> 'request_digest',
        receipt_projection ->> 'target_resource_type',
        receipt_projection ->> 'target_resource_id',
        resulting_revision_value,
        receipt_projection ->> 'resulting_state',
        receipt_projection ->> 'result_code',
        (receipt_projection ->> 'http_disposition_class')::SMALLINT,
        completed_clock
    );

    INSERT INTO public.product_action_receipt_idempotency_aliases (
        tenant_id, installation_id, principal_id, endpoint_domain,
        idempotency_key_digest, idempotency_digest_key_id,
        idempotency_digest_key_fingerprint, receipt_id, created_at
    ) VALUES (
        receipt_projection ->> 'tenant_id',
        receipt_projection ->> 'installation_id',
        receipt_projection ->> 'principal_id',
        receipt_projection ->> 'endpoint_domain',
        receipt_projection ->> 'idempotency_key_digest',
        receipt_projection ->> 'idempotency_digest_key_id',
        receipt_projection ->> 'idempotency_digest_key_fingerprint',
        receipt_projection ->> 'receipt_id',
        completed_clock
    );

    INSERT INTO public.product_audit_events (
        event_id, tenant_id, installation_id, principal_id,
        session_subject_digest, action, target_resource_type,
        target_resource_id, request_id, receipt_id,
        authority_observation_digest, effective_permission_bits,
        authority_observed_at, installation_authority_revision,
        expected_generation, actual_generation, payload_digest,
        binding_fingerprint, policy_revision, active_baseline_version,
        active_baseline_hash, resulting_state, result_code,
        dependency_latency_classes, occurred_at
    ) VALUES (
        audit_projection ->> 'event_id',
        audit_projection ->> 'tenant_id',
        audit_projection ->> 'installation_id',
        audit_projection ->> 'principal_id',
        pg_catalog.decode(audit_projection ->> 'session_subject_digest', 'hex'),
        audit_projection ->> 'action',
        audit_projection ->> 'target_resource_type',
        audit_projection ->> 'target_resource_id',
        audit_projection ->> 'request_id',
        audit_projection ->> 'receipt_id',
        audit_projection ->> 'authority_observation_digest',
        permission_value,
        authority_clock,
        authority_revision_value,
        generation_value,
        generation_value,
        audit_projection ->> 'payload_digest',
        audit_projection ->> 'binding_fingerprint',
        (audit_projection ->> 'policy_revision')::BIGINT,
        (audit_projection ->> 'active_baseline_version')::BIGINT,
        audit_projection ->> 'active_baseline_hash',
        audit_projection ->> 'resulting_state',
        audit_projection ->> 'result_code',
        audit_projection -> 'dependency_latency_classes',
        completed_clock
    );

    SELECT pg_catalog.count(*)
    INTO evidence_count
    FROM public.product_action_receipt_audit_evidence AS evidence
    WHERE evidence.receipt_id = receipt_projection ->> 'receipt_id'
        AND evidence.event_id = audit_projection ->> 'event_id'
        AND evidence.tenant_id = receipt_projection ->> 'tenant_id'
        AND evidence.installation_id = receipt_projection ->> 'installation_id'
        AND evidence.principal_id = receipt_projection ->> 'principal_id'
        AND evidence.endpoint_domain = 'product_promote_v1'
        AND evidence.action = 'promotion.promote'
        AND evidence.request_digest = receipt_projection ->> 'request_digest'
        AND evidence.target_resource_type = 'authoring_promotion'
        AND evidence.target_resource_id = promotion_projection ->> 'id'
        AND evidence.resulting_revision = 3
        AND evidence.resulting_state = receipt_projection ->> 'resulting_state'
        AND evidence.result_code = 'promotion_created'
        AND evidence.http_disposition_class = 2
        AND evidence.completed_at = completed_clock
        AND evidence.evidence_version = 1
        AND evidence.replay_policy_version = 1
        AND evidence.replay_guaranteed_until = replay_until;
    IF evidence_count <> 1 THEN
        RAISE EXCEPTION 'product promotion receipt finalization is incomplete'
            USING ERRCODE = '23514';
    END IF;

    RETURN QUERY SELECT 'created';
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
    promotion_row public.authoring_promotions%ROWTYPE;
    receipt_row public.product_action_receipts%ROWTYPE;
    audit_row public.product_audit_events%ROWTYPE;
    evidence_row public.product_action_receipt_audit_evidence%ROWTYPE;
    activation_row public.activation_requests%ROWTYPE;
    admission_payload JSONB;
    admitted_at TIMESTAMPTZ;
    record_created_at TIMESTAMPTZ;
    record_updated_at TIMESTAMPTZ;
    payload_observed_at TIMESTAMPTZ;
    payload_expires_at TIMESTAMPTZ;
    activation_created_at TIMESTAMPTZ;
    activation_expires_at TIMESTAMPTZ;
    final_clock TIMESTAMPTZ;
    permission_numeric NUMERIC;
    alias_count BIGINT;
    occupied_receipt_count BIGINT;
    target_receipt_count BIGINT;
    activation_count BIGINT;
    audit_count BIGINT;
    evidence_count BIGINT;
    candidate_receipt_id TEXT;
    candidate_target_id TEXT;
    candidate_request_digest TEXT;
    active_baseline_version BIGINT;
    active_baseline_hash TEXT;
    receipt_document JSONB;
    audit_document JSONB;
    input_corrupt BOOLEAN;
    occupancy_corrupt BOOLEAN;
BEGIN
    input_corrupt := expected_promotion_id !~ '^[0-9a-f]{64}$'
        OR expected_session_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_generation NOT BETWEEN 1 AND 9223372036854775807
        OR semantic_request_digest !~ '^[0-9a-f]{64}$'
        OR pg_catalog.array_ndims(idempotency_key_digest_candidates)
            IS DISTINCT FROM 1
        OR pg_catalog.array_ndims(idempotency_digest_key_id_candidates)
            IS DISTINCT FROM 1
        OR pg_catalog.array_ndims(idempotency_digest_key_fingerprint_candidates)
            IS DISTINCT FROM 1
        OR pg_catalog.array_lower(idempotency_key_digest_candidates, 1)
            IS DISTINCT FROM 1
        OR pg_catalog.array_lower(idempotency_digest_key_id_candidates, 1)
            IS DISTINCT FROM 1
        OR pg_catalog.array_lower(idempotency_digest_key_fingerprint_candidates, 1)
            IS DISTINCT FROM 1
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
            FROM pg_catalog.unnest(
                idempotency_key_digest_candidates
            ) AS candidate(digest)
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
        ) <> pg_catalog.cardinality(idempotency_digest_key_fingerprint_candidates);

    alias_count := 0;
    occupied_receipt_count := 0;
    occupancy_corrupt := FALSE;

    IF NOT input_corrupt THEN
        SELECT pg_catalog.count(*),
        pg_catalog.count(DISTINCT occupied.receipt_id)
        INTO alias_count, occupied_receipt_count
        FROM (
        SELECT alias.receipt_id
        FROM public.product_action_receipt_idempotency_aliases AS alias
        WHERE alias.tenant_id = expected_tenant_id
            AND alias.installation_id = expected_installation_id
            AND alias.principal_id = expected_principal_id
            AND alias.endpoint_domain = 'product_promote_v1'
            AND alias.idempotency_key_digest
                = ANY(idempotency_key_digest_candidates)
        UNION ALL
        SELECT receipt.receipt_id
        FROM public.product_action_receipts AS receipt
        WHERE receipt.tenant_id = expected_tenant_id
            AND receipt.installation_id = expected_installation_id
            AND receipt.principal_id = expected_principal_id
            AND receipt.endpoint_domain = 'product_promote_v1'
            AND receipt.idempotency_key_digest
                = ANY(idempotency_key_digest_candidates)
        ) AS occupied(receipt_id);

        occupancy_corrupt := occupied_receipt_count > 1
        OR EXISTS (
            SELECT 1
            FROM public.product_action_receipt_idempotency_aliases AS alias
            WHERE alias.tenant_id = expected_tenant_id
                AND alias.installation_id = expected_installation_id
                AND alias.principal_id = expected_principal_id
                AND alias.endpoint_domain = 'product_promote_v1'
                AND alias.idempotency_key_digest
                    = ANY(idempotency_key_digest_candidates)
                AND NOT EXISTS (
                    SELECT 1
                    FROM public.product_action_receipts AS receipt
                    WHERE receipt.receipt_id = alias.receipt_id
                        AND receipt.tenant_id = alias.tenant_id
                        AND receipt.installation_id = alias.installation_id
                        AND receipt.principal_id = alias.principal_id
                        AND receipt.endpoint_domain = alias.endpoint_domain
                )
        )
        OR EXISTS (
            SELECT 1
            FROM public.product_action_receipt_idempotency_aliases AS alias
            WHERE alias.tenant_id = expected_tenant_id
                AND alias.installation_id = expected_installation_id
                AND alias.principal_id = expected_principal_id
                AND alias.endpoint_domain = 'product_promote_v1'
                AND alias.idempotency_key_digest
                    = ANY(idempotency_key_digest_candidates)
                AND NOT EXISTS (
                    SELECT 1
                    FROM pg_catalog.generate_subscripts(
                        idempotency_key_digest_candidates,
                        1
                    ) AS candidate(ordinal)
                    WHERE idempotency_key_digest_candidates[candidate.ordinal]
                            = alias.idempotency_key_digest
                        AND idempotency_digest_key_id_candidates[candidate.ordinal]
                            = alias.idempotency_digest_key_id
                        AND idempotency_digest_key_fingerprint_candidates[candidate.ordinal]
                            = alias.idempotency_digest_key_fingerprint
                )
        )
        OR EXISTS (
            SELECT 1
            FROM public.product_action_receipts AS receipt
            WHERE receipt.tenant_id = expected_tenant_id
                AND receipt.installation_id = expected_installation_id
                AND receipt.principal_id = expected_principal_id
                AND receipt.endpoint_domain = 'product_promote_v1'
                AND receipt.idempotency_key_digest
                    = ANY(idempotency_key_digest_candidates)
                AND (
                    NOT EXISTS (
                        SELECT 1
                        FROM pg_catalog.generate_subscripts(
                            idempotency_key_digest_candidates,
                            1
                        ) AS candidate(ordinal)
                        WHERE idempotency_key_digest_candidates[candidate.ordinal]
                                = receipt.idempotency_key_digest
                            AND idempotency_digest_key_id_candidates[candidate.ordinal]
                                = receipt.idempotency_digest_key_id
                            AND idempotency_digest_key_fingerprint_candidates[candidate.ordinal]
                                = receipt.idempotency_digest_key_fingerprint
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
        );

        SELECT occupied.receipt_id,
            occupied.target_resource_id,
            occupied.request_digest
        INTO candidate_receipt_id,
            candidate_target_id,
            candidate_request_digest
        FROM public.product_action_receipts AS occupied
        WHERE occupied.tenant_id = expected_tenant_id
            AND occupied.installation_id = expected_installation_id
            AND occupied.principal_id = expected_principal_id
            AND occupied.endpoint_domain = 'product_promote_v1'
            AND occupied.idempotency_key_digest
                = ANY(idempotency_key_digest_candidates)
        LIMIT 1;
    END IF;

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

    IF input_corrupt OR occupancy_corrupt THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    IF candidate_receipt_id IS NOT NULL
        AND (
            candidate_target_id IS DISTINCT FROM expected_promotion_id
            OR candidate_request_digest IS DISTINCT FROM semantic_request_digest
        )
    THEN
        RETURN QUERY SELECT 'idempotency_conflict',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    SELECT promotion.*
    INTO promotion_row
    FROM public.authoring_promotions AS promotion
    WHERE promotion.id = expected_promotion_id
    FOR SHARE;

    final_clock := pg_catalog.clock_timestamp();
    access_result.database_now := final_clock;
    IF final_clock >= authority_expires_at
        OR NOT EXISTS (
            SELECT 1
            FROM public.product_auth_sessions AS product_session
            WHERE product_session.session_digest = expected_product_session_digest
                AND product_session.principal_id = expected_principal_id
                AND product_session.revoked_at IS NULL
                AND product_session.revocation_reason IS NULL
                AND final_clock < product_session.idle_expires_at
                AND final_clock < product_session.absolute_expires_at
        )
    THEN
        RETURN QUERY SELECT 'access_denied',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            final_clock;
        RETURN;
    END IF;

    SELECT pg_catalog.count(*)
    INTO target_receipt_count
    FROM public.product_action_receipts AS receipt
    WHERE receipt.endpoint_domain = 'product_promote_v1'
        AND receipt.target_resource_type = 'authoring_promotion'
        AND receipt.target_resource_id = expected_promotion_id;

    IF promotion_row.id IS NULL THEN
        IF target_receipt_count <> 0
            OR candidate_receipt_id IS NOT NULL
            AND candidate_target_id = expected_promotion_id
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
        IF alias_count <> 0 OR candidate_receipt_id IS NOT NULL THEN
            RETURN QUERY SELECT 'idempotency_conflict',
                NULL::JSONB,
                NULL::JSONB,
                NULL::TEXT,
                NULL::JSONB,
                NULL::JSONB,
                access_result.database_now;
            RETURN;
        END IF;
        RETURN QUERY SELECT 'missing',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    IF promotion_row.tenant_id IS DISTINCT FROM expected_tenant_id
        OR promotion_row.installation_id IS DISTINCT FROM expected_installation_id
        OR promotion_row.principal_id IS DISTINCT FROM expected_principal_id
    THEN
        RETURN QUERY SELECT 'idempotency_conflict',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    IF promotion_row.record_format_version <> 1
        OR promotion_row.revision NOT BETWEEN 1 AND 9223372036854775807
        OR promotion_row.record ->> 'id' IS DISTINCT FROM promotion_row.id
        OR promotion_row.record ->> 'request_digest'
            IS DISTINCT FROM promotion_row.request_digest
        OR promotion_row.record ->> 'revision'
            IS DISTINCT FROM promotion_row.revision::TEXT
        OR promotion_row.record #>> '{stage,state}' IS DISTINCT FROM promotion_row.stage
        OR promotion_row.record #>> '{intent,authority,tenant_id}'
            IS DISTINCT FROM promotion_row.tenant_id
        OR promotion_row.record #>> '{intent,authority,installation_id}'
            IS DISTINCT FROM promotion_row.installation_id
        OR promotion_row.record #>> '{intent,authority,principal_id}'
            IS DISTINCT FROM promotion_row.principal_id
        OR promotion_row.record #>> '{intent,authority,guild_id}'
            IS DISTINCT FROM expected_guild_id
        OR promotion_row.record #>> '{intent,authority,requester}'
            IS DISTINCT FROM expected_acting_user_id
        OR pg_catalog.jsonb_typeof(promotion_row.record) IS DISTINCT FROM 'object'
        OR pg_catalog.jsonb_typeof(promotion_row.record -> 'intent')
            IS DISTINCT FROM 'object'
        OR pg_catalog.jsonb_typeof(promotion_row.record -> 'stage')
            IS DISTINCT FROM 'object'
        OR pg_catalog.octet_length(promotion_row.record::TEXT) > 8388608
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(promotion_row.record) AS key(name)
        ) <> 7
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(promotion_row.record) AS key(name)
            WHERE key.name NOT IN (
                'id', 'revision', 'request_digest', 'intent', 'stage',
                'created_at', 'updated_at'
            )
        )
        OR (
            promotion_row.record ->> 'created_at'
            ~ '^[-0-9]{10,}T[0-9:.]+(Z|[+-][0-9:]+)$'
        ) IS DISTINCT FROM TRUE
        OR (
            promotion_row.record ->> 'updated_at'
            ~ '^[-0-9]{10,}T[0-9:.]+(Z|[+-][0-9:]+)$'
        ) IS DISTINCT FROM TRUE
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

    BEGIN
        record_created_at := (promotion_row.record ->> 'created_at')::TIMESTAMPTZ;
        record_updated_at := (promotion_row.record ->> 'updated_at')::TIMESTAMPTZ;
    EXCEPTION
        WHEN invalid_text_representation OR datetime_field_overflow THEN
            RETURN QUERY SELECT 'persistence_corrupt',
                NULL::JSONB,
                NULL::JSONB,
                NULL::TEXT,
                NULL::JSONB,
                NULL::JSONB,
                access_result.database_now;
            RETURN;
    END;

    IF record_created_at > record_updated_at
        OR record_updated_at > access_result.database_now
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

    IF promotion_row.record #>> '{intent,authority,session_id}'
            IS DISTINCT FROM expected_session_id
        OR promotion_row.record #>> '{intent,authority,session_generation}'
            IS DISTINCT FROM expected_generation::TEXT
    THEN
        RETURN QUERY SELECT 'idempotency_conflict',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    IF promotion_row.product_admission_format_version IS NULL
        AND promotion_row.product_admission_digest IS NULL
        AND promotion_row.product_admission IS NULL
    THEN
        IF promotion_row.stage <> 'activation_pending'
            OR promotion_row.revision <> 3
            OR target_receipt_count <> 0
            OR alias_count <> 0
            OR candidate_receipt_id IS NOT NULL
            OR (
                SELECT pg_catalog.count(*)
                FROM pg_catalog.jsonb_object_keys(
                    promotion_row.record -> 'stage'
                ) AS key(name)
            ) <> 3
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.jsonb_object_keys(
                    promotion_row.record -> 'stage'
                ) AS key(name)
                WHERE key.name NOT IN ('state', 'publication', 'activation')
            )
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

        SELECT pg_catalog.count(*), pg_catalog.min(activation.id)
        INTO activation_count, candidate_receipt_id
        FROM public.activation_requests AS activation
        WHERE activation.promotion_id = expected_promotion_id;
        IF activation_count <> 1 THEN
            RETURN QUERY SELECT 'persistence_corrupt',
                NULL::JSONB,
                NULL::JSONB,
                NULL::TEXT,
                NULL::JSONB,
                NULL::JSONB,
                access_result.database_now;
            RETURN;
        END IF;
        SELECT activation.*
        INTO activation_row
        FROM public.activation_requests AS activation
        WHERE activation.id = candidate_receipt_id
        FOR SHARE;
        BEGIN
            activation_created_at := (
                promotion_row.record #>> '{stage,activation,created_at}'
            )::TIMESTAMPTZ;
            activation_expires_at := (
                promotion_row.record #>> '{stage,activation,expires_at}'
            )::TIMESTAMPTZ;
        EXCEPTION
            WHEN invalid_text_representation OR datetime_field_overflow THEN
                RETURN QUERY SELECT 'persistence_corrupt',
                    NULL::JSONB,
                    NULL::JSONB,
                    NULL::TEXT,
                    NULL::JSONB,
                    NULL::JSONB,
                    access_result.database_now;
                RETURN;
        END;
        IF activation_row.authority_kind <> 'product_authoring'
            OR activation_row.tenant_id IS DISTINCT FROM expected_tenant_id
            OR activation_row.installation_id IS DISTINCT FROM expected_installation_id
            OR activation_row.promotion_id IS DISTINCT FROM expected_promotion_id
            OR activation_row.promotion_request_digest
                IS DISTINCT FROM promotion_row.request_digest
            OR activation_row.guild_id
                IS DISTINCT FROM promotion_row.record
                    #>> '{stage,activation,target,guild_id}'
            OR activation_row.ruleset_key
                IS DISTINCT FROM promotion_row.record
                    #>> '{stage,activation,target,ruleset_key}'
            OR activation_row.target_version::TEXT
                IS DISTINCT FROM promotion_row.record
                    #>> '{stage,activation,target,version}'
            OR activation_row.target_content_hash
                IS DISTINCT FROM promotion_row.record
                    #>> '{stage,activation,target,content_hash}'
            OR activation_row.requester_id
                IS DISTINCT FROM promotion_row.record
                    #>> '{stage,activation,requester}'
            OR activation_row.required_approvals::TEXT
                IS DISTINCT FROM promotion_row.record
                    #>> '{stage,activation,required_approvals}'
            OR activation_row.created_at IS DISTINCT FROM activation_created_at
            OR activation_row.expires_at IS DISTINCT FROM activation_expires_at
            OR promotion_row.record #>> '{stage,activation,request_id}'
                IS DISTINCT FROM activation_row.id
            OR promotion_row.record #>> '{stage,activation,request_state_at_journal}'
                IS DISTINCT FROM 'pending'
            OR promotion_row.record #> '{stage,activation,approval_context}'
                IS DISTINCT FROM activation_row.approval_context -> 'context'
            OR activation_row.link_state_name NOT IN ('unlinked', 'linked')
            OR activation_row.link_state #>> '{state}'
                IS DISTINCT FROM activation_row.link_state_name
            OR activation_row.link_state_name = 'unlinked'
                AND activation_row.linked_at IS NOT NULL
            OR activation_row.link_state_name = 'linked'
                AND activation_row.linked_at IS NULL
            OR NOT EXISTS (
                SELECT 1
                FROM public.automation_ruleset_versions AS version
                WHERE version.guild_id = activation_row.guild_id
                    AND version.ruleset_key = activation_row.ruleset_key
                    AND version.version = activation_row.target_version
                    AND version.content_hash = activation_row.target_content_hash
                    AND version.canonical_content_hash
                        = activation_row.target_content_hash
            )
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
        RETURN QUERY SELECT 'legacy_repair_required',
            promotion_row.record,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    IF promotion_row.product_admission_format_version IS NULL
        OR promotion_row.product_admission_digest IS NULL
        OR promotion_row.product_admission IS NULL
        OR promotion_row.product_admission_format_version <> 1
        OR promotion_row.product_admission_digest !~ '^[0-9a-f]{64}$'
        OR pg_catalog.jsonb_typeof(promotion_row.product_admission)
            IS DISTINCT FROM 'object'
        OR pg_catalog.octet_length(promotion_row.product_admission::TEXT) > 32768
        OR promotion_row.product_admission ->> 'format_version' IS DISTINCT FROM '1'
        OR pg_catalog.jsonb_typeof(promotion_row.product_admission -> 'payload')
            IS DISTINCT FROM 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(
                promotion_row.product_admission
            ) AS key(name)
        ) <> 3
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(
                promotion_row.product_admission
            ) AS key(name)
            WHERE key.name NOT IN ('format_version', 'payload', 'admitted_at')
        )
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

    admission_payload := promotion_row.product_admission -> 'payload';
    IF (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(admission_payload) AS key(name)
        ) <> 31
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(admission_payload) AS key(name)
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
        OR admission_payload ->> 'endpoint_domain'
            IS DISTINCT FROM 'product_promote_v1'
        OR admission_payload ->> 'tenant_id' IS DISTINCT FROM expected_tenant_id
        OR admission_payload ->> 'installation_id'
            IS DISTINCT FROM expected_installation_id
        OR admission_payload ->> 'principal_id'
            IS DISTINCT FROM expected_principal_id
        OR admission_payload ->> 'promotion_id'
            IS DISTINCT FROM expected_promotion_id
        OR admission_payload ->> 'promotion_request_digest'
            IS DISTINCT FROM promotion_row.request_digest
        OR admission_payload ->> 'candidate_revision'
            IS DISTINCT FROM promotion_row.record
                #>> '{intent,evidence,candidate_revision}'
        OR admission_payload ->> 'candidate_hash'
            IS DISTINCT FROM promotion_row.record
                #>> '{intent,evidence,candidate_ruleset_hash}'
        OR admission_payload ->> 'binding_fingerprint'
            IS DISTINCT FROM promotion_row.record
                #>> '{intent,evidence,context_fingerprint}'
        OR admission_payload ->> 'policy_revision'
            IS DISTINCT FROM promotion_row.record
                #>> '{intent,authority,policy,revision}'
        OR admission_payload ->> 'discord_application_id'
            IS DISTINCT FROM expected_discord_application_id
        OR admission_payload ->> 'guild_id' IS DISTINCT FROM expected_guild_id
        OR admission_payload ->> 'acting_user_id'
            IS DISTINCT FROM expected_acting_user_id
        OR admission_payload ->> 'capability' IS DISTINCT FROM 'promote'
        OR (admission_payload ->> 'product_request_id'
            ~ '^[A-Za-z0-9_.:-]{1,128}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'session_subject_digest'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'idempotency_key_digest'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'idempotency_digest_key_id'
            ~ '^[A-Za-z0-9_.:-]{1,64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'idempotency_digest_key_fingerprint'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'semantic_request_digest'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'receipt_id'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'audit_event_id'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'authority_revision'
            ~ '^[1-9][0-9]{0,18}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'authority_payload_digest'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'authority_observation_digest'
            ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        OR (admission_payload ->> 'effective_permission_bits'
            ~ '^(0|[1-9][0-9]{0,19})$') IS DISTINCT FROM TRUE
        OR pg_catalog.jsonb_typeof(admission_payload -> 'guild_owner')
            IS DISTINCT FROM 'boolean'
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

    IF admission_payload ->> 'authoring_session_id'
            IS DISTINCT FROM expected_session_id
        OR admission_payload ->> 'generation'
            IS DISTINCT FROM expected_generation::TEXT
        OR admission_payload ->> 'semantic_request_digest'
            IS DISTINCT FROM semantic_request_digest
        OR NOT EXISTS (
            SELECT 1
            FROM pg_catalog.generate_subscripts(
                idempotency_key_digest_candidates,
                1
            ) AS candidate(ordinal)
            WHERE idempotency_key_digest_candidates[candidate.ordinal]
                    = admission_payload ->> 'idempotency_key_digest'
                AND idempotency_digest_key_id_candidates[candidate.ordinal]
                    = admission_payload ->> 'idempotency_digest_key_id'
                AND idempotency_digest_key_fingerprint_candidates[candidate.ordinal]
                    = admission_payload ->> 'idempotency_digest_key_fingerprint'
        )
    THEN
        RETURN QUERY SELECT 'idempotency_conflict',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    BEGIN
        admitted_at := (
            promotion_row.product_admission ->> 'admitted_at'
        )::TIMESTAMPTZ;
        payload_observed_at := (
            admission_payload ->> 'authority_observed_at'
        )::TIMESTAMPTZ;
        payload_expires_at := (
            admission_payload ->> 'authority_expires_at'
        )::TIMESTAMPTZ;
        permission_numeric := (
            admission_payload ->> 'effective_permission_bits'
        )::NUMERIC;
    EXCEPTION
        WHEN invalid_text_representation
            OR numeric_value_out_of_range
            OR datetime_field_overflow
        THEN
            RETURN QUERY SELECT 'persistence_corrupt',
                NULL::JSONB,
                NULL::JSONB,
                NULL::TEXT,
                NULL::JSONB,
                NULL::JSONB,
                access_result.database_now;
            RETURN;
    END;

    IF admitted_at < record_created_at
        OR admitted_at > access_result.database_now
        OR payload_observed_at > admitted_at
        OR admitted_at >= payload_expires_at
        OR payload_expires_at > payload_observed_at + INTERVAL '5 seconds'
        OR permission_numeric > 18446744073709551615
        OR NOT (
            (admission_payload ->> 'guild_owner')::BOOLEAN
            OR pg_catalog.mod(permission_numeric, 16) >= 8
            OR pg_catalog.mod(permission_numeric, 64) >= 32
        )
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

    IF promotion_row.stage IN ('prepared', 'published') THEN
        IF NOT (
                promotion_row.stage = 'prepared' AND promotion_row.revision = 1
                OR promotion_row.stage = 'published' AND promotion_row.revision = 2
            )
            OR admitted_at IS DISTINCT FROM record_created_at
            OR target_receipt_count <> 0
            OR EXISTS (
                SELECT 1
                FROM public.product_action_receipts AS receipt
                WHERE receipt.receipt_id = admission_payload ->> 'receipt_id'
            )
            OR EXISTS (
                SELECT 1
                FROM public.product_audit_events AS audit
                WHERE audit.event_id = admission_payload ->> 'audit_event_id'
                    OR audit.receipt_id = admission_payload ->> 'receipt_id'
            )
            OR EXISTS (
                SELECT 1
                FROM public.product_action_receipt_audit_evidence AS evidence
                WHERE evidence.event_id = admission_payload ->> 'audit_event_id'
                    OR evidence.receipt_id = admission_payload ->> 'receipt_id'
            )
            OR alias_count <> 0
            OR candidate_receipt_id IS NOT NULL
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
        RETURN QUERY SELECT 'partial_exact',
            promotion_row.record,
            promotion_row.product_admission,
            promotion_row.product_admission_digest,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    IF promotion_row.stage NOT IN ('activation_pending', 'expired')
        OR promotion_row.stage = 'activation_pending' AND promotion_row.revision <> 3
        OR promotion_row.stage = 'expired' AND promotion_row.revision NOT IN (3, 4)
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

    SELECT receipt.*
    INTO receipt_row
    FROM public.product_action_receipts AS receipt
    WHERE receipt.receipt_id = admission_payload ->> 'receipt_id'
    FOR KEY SHARE;
    IF receipt_row.receipt_id IS NULL THEN
        SELECT pg_catalog.count(*)
        INTO evidence_count
        FROM public.product_action_receipt_audit_evidence AS evidence
        WHERE evidence.receipt_id = admission_payload ->> 'receipt_id'
            AND evidence.event_id = admission_payload ->> 'audit_event_id';
        IF evidence_count = 1 THEN
            SELECT evidence.*
            INTO evidence_row
            FROM public.product_action_receipt_audit_evidence AS evidence
            WHERE evidence.receipt_id = admission_payload ->> 'receipt_id'
                AND evidence.event_id = admission_payload ->> 'audit_event_id';
        END IF;
        IF evidence_count = 1
            AND evidence_row.tenant_id IS NOT DISTINCT FROM expected_tenant_id
            AND evidence_row.installation_id
                IS NOT DISTINCT FROM expected_installation_id
            AND evidence_row.principal_id IS NOT DISTINCT FROM expected_principal_id
            AND evidence_row.endpoint_domain = 'product_promote_v1'
            AND evidence_row.action = 'promotion.promote'
            AND evidence_row.request_digest
                IS NOT DISTINCT FROM semantic_request_digest
            AND evidence_row.target_resource_type = 'authoring_promotion'
            AND evidence_row.target_resource_id IS NOT DISTINCT FROM promotion_row.id
            AND (
                promotion_row.stage = 'activation_pending'
                    AND promotion_row.revision = 3
                    AND evidence_row.resulting_revision = 3
                    AND evidence_row.resulting_state = 'activation_pending'
                OR promotion_row.stage = 'expired'
                    AND promotion_row.revision = 3
                    AND evidence_row.resulting_revision = 3
                    AND evidence_row.resulting_state = 'expired'
                OR promotion_row.stage = 'expired'
                    AND promotion_row.revision = 4
                    AND evidence_row.resulting_revision = 3
                    AND evidence_row.resulting_state = 'activation_pending'
            )
            AND evidence_row.result_code IN (
                'promotion_created',
                'promotion_recovered'
            )
            AND evidence_row.http_disposition_class = 2
            AND evidence_row.completed_at >= admitted_at
            AND evidence_row.completed_at <= access_result.database_now
            AND evidence_row.evidence_version = 1
            AND evidence_row.replay_policy_version = 1
            AND evidence_row.replay_guaranteed_until
                = evidence_row.completed_at + INTERVAL '168 hours'
            AND evidence_row.replay_guaranteed_until <= access_result.database_now
        THEN
            RETURN QUERY SELECT 'idempotency_conflict',
                NULL::JSONB,
                NULL::JSONB,
                NULL::TEXT,
                NULL::JSONB,
                NULL::JSONB,
                access_result.database_now;
            RETURN;
        END IF;
    END IF;
    IF receipt_row.receipt_id IS NULL THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    SELECT pg_catalog.count(*)
    INTO audit_count
    FROM public.product_audit_events AS audit
    WHERE audit.event_id = admission_payload ->> 'audit_event_id'
        AND audit.receipt_id = receipt_row.receipt_id;
    SELECT pg_catalog.count(*)
    INTO evidence_count
    FROM public.product_action_receipt_audit_evidence AS evidence
    WHERE evidence.event_id = admission_payload ->> 'audit_event_id'
        AND evidence.receipt_id = receipt_row.receipt_id;
    IF audit_count <> 1 OR evidence_count <> 1 THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;
    SELECT audit.*
    INTO audit_row
    FROM public.product_audit_events AS audit
    WHERE audit.event_id = admission_payload ->> 'audit_event_id';
    SELECT evidence.*
    INTO evidence_row
    FROM public.product_action_receipt_audit_evidence AS evidence
    WHERE evidence.event_id = admission_payload ->> 'audit_event_id';

    IF promotion_row.record #>> '{stage,activation,approval_context,baseline,state}'
        = 'absent'
    THEN
        active_baseline_version := NULL;
        active_baseline_hash := NULL;
    ELSIF promotion_row.record
        #>> '{stage,activation,approval_context,baseline,state}' = 'exact'
    THEN
        BEGIN
            active_baseline_version := (
                promotion_row.record
                    #>> '{stage,activation,approval_context,baseline,version}'
            )::BIGINT;
        EXCEPTION
            WHEN invalid_text_representation OR numeric_value_out_of_range THEN
                RETURN QUERY SELECT 'persistence_corrupt',
                    NULL::JSONB,
                    NULL::JSONB,
                    NULL::TEXT,
                    NULL::JSONB,
                    NULL::JSONB,
                    access_result.database_now;
                RETURN;
        END;
        active_baseline_hash := promotion_row.record
            #>> '{stage,activation,approval_context,baseline,content_hash}';
    ELSE
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            access_result.database_now;
        RETURN;
    END IF;

    IF receipt_row.tenant_id IS DISTINCT FROM expected_tenant_id
        OR receipt_row.installation_id IS DISTINCT FROM expected_installation_id
        OR receipt_row.principal_id IS DISTINCT FROM expected_principal_id
        OR receipt_row.endpoint_domain <> 'product_promote_v1'
        OR receipt_row.idempotency_key_digest
            IS DISTINCT FROM admission_payload ->> 'idempotency_key_digest'
        OR receipt_row.idempotency_digest_key_id
            IS DISTINCT FROM admission_payload ->> 'idempotency_digest_key_id'
        OR receipt_row.idempotency_digest_key_fingerprint
            IS DISTINCT FROM admission_payload
                ->> 'idempotency_digest_key_fingerprint'
        OR receipt_row.request_digest
            IS DISTINCT FROM admission_payload ->> 'semantic_request_digest'
        OR receipt_row.target_resource_type <> 'authoring_promotion'
        OR receipt_row.target_resource_id IS DISTINCT FROM promotion_row.id
        OR NOT (
            promotion_row.stage = 'activation_pending'
                AND promotion_row.revision = 3
                AND receipt_row.resulting_revision = 3
                AND receipt_row.resulting_state = 'activation_pending'
            OR promotion_row.stage = 'expired'
                AND promotion_row.revision = 3
                AND receipt_row.resulting_revision = 3
                AND receipt_row.resulting_state = 'expired'
            OR promotion_row.stage = 'expired'
                AND promotion_row.revision = 4
                AND receipt_row.resulting_revision = 3
                AND receipt_row.resulting_state = 'activation_pending'
        )
        OR receipt_row.result_code NOT IN ('promotion_created', 'promotion_recovered')
        OR receipt_row.http_disposition_class <> 2
        OR receipt_row.completed_at < admitted_at
        OR receipt_row.completed_at > access_result.database_now
        OR receipt_row.result_code = 'promotion_created'
            AND (
                admitted_at IS DISTINCT FROM record_created_at
                OR promotion_row.revision <> 4
                    AND receipt_row.completed_at IS DISTINCT FROM record_updated_at
            )
        OR audit_row.receipt_id IS DISTINCT FROM receipt_row.receipt_id
        OR audit_row.tenant_id IS DISTINCT FROM receipt_row.tenant_id
        OR audit_row.installation_id IS DISTINCT FROM receipt_row.installation_id
        OR audit_row.principal_id IS DISTINCT FROM receipt_row.principal_id
        OR pg_catalog.encode(audit_row.session_subject_digest, 'hex')
            IS DISTINCT FROM admission_payload ->> 'session_subject_digest'
        OR audit_row.action <> 'promotion.promote'
        OR audit_row.target_resource_type IS DISTINCT FROM receipt_row.target_resource_type
        OR audit_row.target_resource_id IS DISTINCT FROM receipt_row.target_resource_id
        OR audit_row.request_id IS DISTINCT FROM admission_payload ->> 'product_request_id'
        OR audit_row.authority_observation_digest
            IS DISTINCT FROM admission_payload ->> 'authority_observation_digest'
        OR audit_row.effective_permission_bits::TEXT
            IS DISTINCT FROM admission_payload ->> 'effective_permission_bits'
        OR audit_row.authority_observed_at IS DISTINCT FROM payload_observed_at
        OR audit_row.installation_authority_revision::TEXT
            IS DISTINCT FROM admission_payload ->> 'authority_revision'
        OR audit_row.expected_generation IS DISTINCT FROM expected_generation
        OR audit_row.actual_generation IS DISTINCT FROM expected_generation
        OR audit_row.payload_digest IS DISTINCT FROM promotion_row.request_digest
        OR audit_row.binding_fingerprint
            IS DISTINCT FROM admission_payload ->> 'binding_fingerprint'
        OR audit_row.policy_revision::TEXT
            IS DISTINCT FROM admission_payload ->> 'policy_revision'
        OR audit_row.active_baseline_version
            IS DISTINCT FROM active_baseline_version
        OR audit_row.active_baseline_hash IS DISTINCT FROM active_baseline_hash
        OR audit_row.resulting_state IS DISTINCT FROM receipt_row.resulting_state
        OR audit_row.result_code IS DISTINCT FROM receipt_row.result_code
        OR audit_row.dependency_latency_classes IS DISTINCT FROM '{}'::JSONB
        OR audit_row.occurred_at IS DISTINCT FROM receipt_row.completed_at
        OR evidence_row.receipt_id IS DISTINCT FROM receipt_row.receipt_id
        OR evidence_row.event_id IS DISTINCT FROM audit_row.event_id
        OR evidence_row.tenant_id IS DISTINCT FROM receipt_row.tenant_id
        OR evidence_row.installation_id IS DISTINCT FROM receipt_row.installation_id
        OR evidence_row.principal_id IS DISTINCT FROM receipt_row.principal_id
        OR evidence_row.endpoint_domain IS DISTINCT FROM receipt_row.endpoint_domain
        OR evidence_row.action IS DISTINCT FROM audit_row.action
        OR evidence_row.request_digest IS DISTINCT FROM receipt_row.request_digest
        OR evidence_row.target_resource_type
            IS DISTINCT FROM receipt_row.target_resource_type
        OR evidence_row.target_resource_id IS DISTINCT FROM receipt_row.target_resource_id
        OR evidence_row.resulting_revision
            IS DISTINCT FROM receipt_row.resulting_revision
        OR evidence_row.resulting_state IS DISTINCT FROM receipt_row.resulting_state
        OR evidence_row.result_code IS DISTINCT FROM receipt_row.result_code
        OR evidence_row.http_disposition_class
            IS DISTINCT FROM receipt_row.http_disposition_class
        OR evidence_row.completed_at IS DISTINCT FROM receipt_row.completed_at
        OR evidence_row.evidence_version <> 1
        OR evidence_row.replay_policy_version <> 1
        OR evidence_row.replay_guaranteed_until
            IS DISTINCT FROM receipt_row.completed_at + INTERVAL '168 hours'
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

    final_clock := pg_catalog.clock_timestamp();
    access_result.database_now := final_clock;
    IF final_clock >= authority_expires_at
        OR NOT EXISTS (
            SELECT 1
            FROM public.product_auth_sessions AS product_session
            WHERE product_session.session_digest = expected_product_session_digest
                AND product_session.principal_id = expected_principal_id
                AND product_session.revoked_at IS NULL
                AND product_session.revocation_reason IS NULL
                AND final_clock < product_session.idle_expires_at
                AND final_clock < product_session.absolute_expires_at
        )
    THEN
        RETURN QUERY SELECT 'access_denied',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            NULL::JSONB,
            NULL::JSONB,
            final_clock;
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
    SELECT expected_tenant_id,
        expected_installation_id,
        expected_principal_id,
        'product_promote_v1',
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

    IF EXISTS (
        SELECT 1
        FROM pg_catalog.generate_subscripts(
            idempotency_key_digest_candidates,
            1
        ) AS candidate(ordinal)
        WHERE NOT EXISTS (
            SELECT 1
            FROM public.product_action_receipt_idempotency_aliases AS alias
            WHERE alias.tenant_id = expected_tenant_id
                AND alias.installation_id = expected_installation_id
                AND alias.principal_id = expected_principal_id
                AND alias.endpoint_domain = 'product_promote_v1'
                AND alias.idempotency_key_digest
                    = idempotency_key_digest_candidates[candidate.ordinal]
                AND alias.idempotency_digest_key_id
                    = idempotency_digest_key_id_candidates[candidate.ordinal]
                AND alias.idempotency_digest_key_fingerprint
                    = idempotency_digest_key_fingerprint_candidates[candidate.ordinal]
                AND alias.receipt_id = receipt_row.receipt_id
        )
    ) THEN
        RAISE EXCEPTION 'product promotion replay alias convergence failed'
            USING ERRCODE = '23514';
    END IF;

    receipt_document := pg_catalog.jsonb_build_object(
        'format_version', 1,
        'receipt_id', receipt_row.receipt_id,
        'tenant_id', receipt_row.tenant_id,
        'installation_id', receipt_row.installation_id,
        'principal_id', receipt_row.principal_id,
        'endpoint_domain', receipt_row.endpoint_domain,
        'idempotency_key_digest', receipt_row.idempotency_key_digest,
        'idempotency_digest_key_id', receipt_row.idempotency_digest_key_id,
        'idempotency_digest_key_fingerprint',
            receipt_row.idempotency_digest_key_fingerprint,
        'request_digest', receipt_row.request_digest,
        'target_resource_type', receipt_row.target_resource_type,
        'target_resource_id', receipt_row.target_resource_id,
        'resulting_revision', receipt_row.resulting_revision,
        'resulting_state', receipt_row.resulting_state,
        'result_code', receipt_row.result_code,
        'http_disposition_class', receipt_row.http_disposition_class,
        'completed_at', receipt_row.completed_at
    );
    audit_document := pg_catalog.jsonb_build_object(
        'format_version', 1,
        'event_id', audit_row.event_id,
        'receipt_id', audit_row.receipt_id,
        'tenant_id', audit_row.tenant_id,
        'installation_id', audit_row.installation_id,
        'principal_id', audit_row.principal_id,
        'session_subject_digest',
            pg_catalog.encode(audit_row.session_subject_digest, 'hex'),
        'action', audit_row.action,
        'target_resource_type', audit_row.target_resource_type,
        'target_resource_id', audit_row.target_resource_id,
        'request_id', audit_row.request_id,
        'authority_observation_digest', audit_row.authority_observation_digest,
        'effective_permission_bits', audit_row.effective_permission_bits::TEXT,
        'authority_observed_at', audit_row.authority_observed_at,
        'installation_authority_revision',
            audit_row.installation_authority_revision,
        'expected_generation', audit_row.expected_generation,
        'actual_generation', audit_row.actual_generation,
        'payload_digest', audit_row.payload_digest,
        'binding_fingerprint', audit_row.binding_fingerprint,
        'policy_revision', audit_row.policy_revision,
        'active_baseline_version', audit_row.active_baseline_version,
        'active_baseline_hash', audit_row.active_baseline_hash,
        'resulting_state', audit_row.resulting_state,
        'result_code', audit_row.result_code,
        'dependency_latency_classes', audit_row.dependency_latency_classes,
        'occurred_at', audit_row.occurred_at,
        'endpoint_domain', evidence_row.endpoint_domain,
        'request_digest', evidence_row.request_digest,
        'resulting_revision', evidence_row.resulting_revision,
        'http_disposition_class', evidence_row.http_disposition_class,
        'completed_at', evidence_row.completed_at,
        'evidence_version', evidence_row.evidence_version,
        'replay_policy_version', evidence_row.replay_policy_version,
        'replay_guaranteed_until', evidence_row.replay_guaranteed_until
    );

    IF (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(receipt_document)
        ) <> 17
        OR pg_catalog.octet_length(receipt_document::TEXT) > 65536
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(audit_document)
        ) <> 34
        OR pg_catalog.octet_length(audit_document::TEXT) > 65536
    THEN
        RAISE EXCEPTION 'product promotion replay projection is malformed'
            USING ERRCODE = '23514';
    END IF;

    RETURN QUERY SELECT 'final_exact',
        promotion_row.record,
        promotion_row.product_admission,
        promotion_row.product_admission_digest,
        receipt_document,
        audit_document,
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
    replay_result RECORD;
    payload_observed_at TIMESTAMPTZ;
    payload_expires_at TIMESTAMPTZ;
    final_clock TIMESTAMPTZ;
    registry_schema_version BIGINT;
    calculated_candidate_hash TEXT;
    calculated_content_hash TEXT;
    prepared_record JSONB;
    admission_document JSONB;
    inserted_count BIGINT;
    session_row public.authoring_sessions%ROWTYPE;
    generation_row public.authoring_session_generations%ROWTYPE;
    installation_row public.automation_installations%ROWTYPE;
    historical_authority_row public.automation_installation_authority_versions%ROWTYPE;
    product_session_row public.product_auth_sessions%ROWTYPE;
    persisted_promotion_row public.authoring_promotions%ROWTYPE;
BEGIN
    SELECT *
    INTO replay_result
    FROM public.starring_product_promotion_replay_v1(
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
        guild_owner,
        expected_promotion_id,
        expected_session_id,
        expected_generation,
        semantic_request_digest,
        idempotency_key_digest_candidates,
        idempotency_digest_key_id_candidates,
        idempotency_digest_key_fingerprint_candidates
    );
    IF replay_result.outcome_code <> 'missing' THEN
        IF replay_result.outcome_code IN ('partial_exact', 'final_exact') THEN
            RETURN QUERY SELECT replay_result.outcome_code,
                replay_result.promotion_record,
                replay_result.admission_evidence,
                replay_result.admission_digest,
                replay_result.database_now;
        ELSIF replay_result.outcome_code = 'legacy_repair_required' THEN
            RETURN QUERY SELECT 'idempotency_conflict',
                NULL::JSONB,
                NULL::JSONB,
                NULL::TEXT,
                replay_result.database_now;
        ELSE
            RETURN QUERY SELECT replay_result.outcome_code,
                NULL::JSONB,
                NULL::JSONB,
                NULL::TEXT,
                replay_result.database_now;
        END IF;
        RETURN;
    END IF;

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
        OR session_subject_digest = expected_product_session_digest
        OR expected_session_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_generation NOT BETWEEN 1 AND 9223372036854775807
        OR expected_candidate_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_candidate_hash !~ '^[0-9a-f]{64}$'
        OR expected_binding_fingerprint !~ '^[0-9a-f]{64}$'
        OR expected_promotion_id !~ '^[0-9a-f]{64}$'
        OR expected_promotion_request_digest !~ '^[0-9a-f]{64}$'
        OR pg_catalog.jsonb_typeof(prepared_promotion_intent) <> 'object'
        OR pg_catalog.octet_length(prepared_promotion_intent::TEXT) > 8388608
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(CASE
                WHEN pg_catalog.jsonb_typeof(prepared_promotion_intent) = 'object'
                    THEN prepared_promotion_intent
                ELSE '{}'::JSONB
            END) AS key(name)
        ) <> 7
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(CASE
                WHEN pg_catalog.jsonb_typeof(prepared_promotion_intent) = 'object'
                    THEN prepared_promotion_intent
                ELSE '{}'::JSONB
            END) AS key(name)
            WHERE key.name NOT IN (
                'idempotency_scope_digest', 'authority', 'evidence', 'definition',
                'preview', 'registry_schema_version',
                'expected_registry_content_hash'
            )
        )
        OR prepared_promotion_intent ->> 'idempotency_scope_digest'
            IS DISTINCT FROM expected_promotion_id
        OR pg_catalog.jsonb_typeof(prepared_promotion_intent -> 'authority')
            IS DISTINCT FROM 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(CASE
                WHEN pg_catalog.jsonb_typeof(prepared_promotion_intent -> 'authority')
                        = 'object'
                    THEN prepared_promotion_intent -> 'authority'
                ELSE '{}'::JSONB
            END) AS key(name)
        ) <> 11
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(CASE
                WHEN pg_catalog.jsonb_typeof(prepared_promotion_intent -> 'authority')
                        = 'object'
                    THEN prepared_promotion_intent -> 'authority'
                ELSE '{}'::JSONB
            END) AS key(name)
            WHERE key.name NOT IN (
                'tenant_id', 'principal_id', 'session_owner_id', 'session_id',
                'session_generation', 'guild_id', 'installation_id',
                'ruleset_key', 'requester', 'binding_revision', 'policy'
            )
        )
        OR pg_catalog.jsonb_typeof(prepared_promotion_intent #> '{authority,policy}')
            IS DISTINCT FROM 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(CASE
                WHEN pg_catalog.jsonb_typeof(
                        prepared_promotion_intent #> '{authority,policy}'
                    ) = 'object'
                    THEN prepared_promotion_intent #> '{authority,policy}'
                ELSE '{}'::JSONB
            END) AS key(name)
        ) <> 3
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(CASE
                WHEN pg_catalog.jsonb_typeof(
                        prepared_promotion_intent #> '{authority,policy}'
                    ) = 'object'
                    THEN prepared_promotion_intent #> '{authority,policy}'
                ELSE '{}'::JSONB
            END) AS key(name)
            WHERE key.name NOT IN ('revision', 'required_approvals', 'ttl_seconds')
        )
        OR pg_catalog.jsonb_typeof(prepared_promotion_intent -> 'evidence')
            IS DISTINCT FROM 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(CASE
                WHEN pg_catalog.jsonb_typeof(prepared_promotion_intent -> 'evidence')
                        = 'object'
                    THEN prepared_promotion_intent -> 'evidence'
                ELSE '{}'::JSONB
            END) AS key(name)
        ) <> 25
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(CASE
                WHEN pg_catalog.jsonb_typeof(prepared_promotion_intent -> 'evidence')
                        = 'object'
                    THEN prepared_promotion_intent -> 'evidence'
                ELSE '{}'::JSONB
            END) AS key(name)
            WHERE key.name NOT IN (
                'artifact_version', 'intent_protocol_version', 'identity_revision',
                'extractor_revision', 'normalizer_revision', 'compiler_revision',
                'simulator_revision', 'recipe_id', 'recipe_version',
                'recipe_descriptor_digest', 'recipe_registry_digest',
                'requested_outcome', 'intent_revision', 'candidate_revision',
                'request_evidence_hash', 'request_evidence_entries',
                'compiler_input_hash', 'semantic_intent_hash', 'compiled_plan_hash',
                'candidate_ruleset_hash', 'candidate_draft_hash',
                'compiled_operations', 'context_fingerprint',
                'external_channel_bindings', 'stage_binding_digest'
            )
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.unnest(ARRAY[
                'artifact_version', 'intent_protocol_version', 'identity_revision',
                'extractor_revision', 'normalizer_revision', 'compiler_revision',
                'simulator_revision', 'recipe_version', 'intent_revision',
                'candidate_revision', 'request_evidence_entries',
                'compiled_operations'
            ]) AS field(name)
            WHERE pg_catalog.jsonb_typeof(
                prepared_promotion_intent -> 'evidence' -> field.name
            ) IS DISTINCT FROM 'number'
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.unnest(ARRAY[
                'recipe_id', 'recipe_descriptor_digest', 'recipe_registry_digest',
                'requested_outcome', 'request_evidence_hash', 'compiler_input_hash',
                'semantic_intent_hash', 'compiled_plan_hash',
                'candidate_ruleset_hash', 'candidate_draft_hash',
                'context_fingerprint', 'stage_binding_digest'
            ]) AS field(name)
            WHERE pg_catalog.jsonb_typeof(
                prepared_promotion_intent -> 'evidence' -> field.name
            ) IS DISTINCT FROM 'string'
        )
        OR prepared_promotion_intent #>> '{evidence,artifact_version}'
            IS DISTINCT FROM '1'
        OR (CASE
            WHEN (
                prepared_promotion_intent #>> '{evidence,intent_protocol_version}'
            ) ~ '^[1-9][0-9]{0,4}$'
                THEN (prepared_promotion_intent
                    #>> '{evidence,intent_protocol_version}')::NUMERIC <= 65535
            ELSE FALSE
        END) IS DISTINCT FROM TRUE
        OR (CASE
            WHEN (
                prepared_promotion_intent #>> '{evidence,identity_revision}'
            ) ~ '^[1-9][0-9]{0,4}$'
                THEN (prepared_promotion_intent
                    #>> '{evidence,identity_revision}')::NUMERIC <= 65535
            ELSE FALSE
        END) IS DISTINCT FROM TRUE
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.unnest(ARRAY[
                prepared_promotion_intent #>> '{evidence,extractor_revision}',
                prepared_promotion_intent #>> '{evidence,normalizer_revision}',
                prepared_promotion_intent #>> '{evidence,compiler_revision}',
                prepared_promotion_intent #>> '{evidence,simulator_revision}',
                prepared_promotion_intent #>> '{evidence,recipe_version}'
            ]) AS revision(value)
            WHERE (CASE
                WHEN revision.value ~ '^[1-9][0-9]{0,9}$'
                    THEN revision.value::NUMERIC <= 4294967295
                ELSE FALSE
            END) IS DISTINCT FROM TRUE
        )
        OR prepared_promotion_intent #>> '{evidence,recipe_id}'
            !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.unnest(ARRAY[
                prepared_promotion_intent #>> '{evidence,recipe_descriptor_digest}',
                prepared_promotion_intent #>> '{evidence,recipe_registry_digest}',
                prepared_promotion_intent #>> '{evidence,request_evidence_hash}',
                prepared_promotion_intent #>> '{evidence,compiler_input_hash}',
                prepared_promotion_intent #>> '{evidence,semantic_intent_hash}',
                prepared_promotion_intent #>> '{evidence,compiled_plan_hash}',
                prepared_promotion_intent #>> '{evidence,candidate_ruleset_hash}',
                prepared_promotion_intent #>> '{evidence,candidate_draft_hash}',
                prepared_promotion_intent #>> '{evidence,context_fingerprint}',
                prepared_promotion_intent #>> '{evidence,stage_binding_digest}'
            ]) AS digest(value)
            WHERE (digest.value ~ '^[0-9a-f]{64}$') IS DISTINCT FROM TRUE
        )
        OR prepared_promotion_intent #>> '{evidence,requested_outcome}'
            IS DISTINCT FROM 'validated_preview'
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.unnest(ARRAY[
                prepared_promotion_intent #>> '{evidence,intent_revision}',
                prepared_promotion_intent #>> '{evidence,candidate_revision}',
                prepared_promotion_intent #>> '{evidence,request_evidence_entries}',
                prepared_promotion_intent #>> '{evidence,compiled_operations}'
            ]) AS counter(value)
            WHERE (CASE
                WHEN counter.value ~ '^[1-9][0-9]{0,19}$'
                    THEN counter.value::NUMERIC <= 18446744073709551615
                ELSE FALSE
            END) IS DISTINCT FROM TRUE
        )
        OR pg_catalog.jsonb_typeof(
            prepared_promotion_intent #> '{evidence,external_channel_bindings}'
        ) IS DISTINCT FROM 'array'
        OR pg_catalog.jsonb_array_length(CASE
            WHEN pg_catalog.jsonb_typeof(
                    prepared_promotion_intent #> '{evidence,external_channel_bindings}'
                ) = 'array'
                THEN prepared_promotion_intent
                    #> '{evidence,external_channel_bindings}'
            ELSE '[]'::JSONB
        END) = 0
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_array_elements(CASE
                WHEN pg_catalog.jsonb_typeof(
                        prepared_promotion_intent
                            #> '{evidence,external_channel_bindings}'
                    ) = 'array'
                    THEN prepared_promotion_intent
                        #> '{evidence,external_channel_bindings}'
                ELSE '[]'::JSONB
            END) AS binding(value)
            WHERE pg_catalog.jsonb_typeof(binding.value) <> 'string'
                OR (binding.value #>> '{}') !~ '^[A-Za-z0-9_.:-]{1,128}$'
        )
        OR (
            SELECT pg_catalog.count(*)
                <> pg_catalog.count(DISTINCT binding.value #>> '{}')
            FROM pg_catalog.jsonb_array_elements(CASE
                WHEN pg_catalog.jsonb_typeof(
                        prepared_promotion_intent
                            #> '{evidence,external_channel_bindings}'
                    ) = 'array'
                    THEN prepared_promotion_intent
                        #> '{evidence,external_channel_bindings}'
                ELSE '[]'::JSONB
            END) AS binding(value)
        )
        OR (
            SELECT pg_catalog.array_agg(binding.value #>> '{}' ORDER BY binding.ordinal)
            FROM pg_catalog.jsonb_array_elements(CASE
                WHEN pg_catalog.jsonb_typeof(
                        prepared_promotion_intent
                            #> '{evidence,external_channel_bindings}'
                    ) = 'array'
                    THEN prepared_promotion_intent
                        #> '{evidence,external_channel_bindings}'
                ELSE '[]'::JSONB
            END) WITH ORDINALITY AS binding(value, ordinal)
        ) IS DISTINCT FROM (
            SELECT pg_catalog.array_agg(binding.value #>> '{}' ORDER BY binding.value #>> '{}')
            FROM pg_catalog.jsonb_array_elements(CASE
                WHEN pg_catalog.jsonb_typeof(
                        prepared_promotion_intent
                            #> '{evidence,external_channel_bindings}'
                    ) = 'array'
                    THEN prepared_promotion_intent
                        #> '{evidence,external_channel_bindings}'
                ELSE '[]'::JSONB
            END) AS binding(value)
        )
        OR pg_catalog.jsonb_typeof(prepared_promotion_intent -> 'definition')
            IS DISTINCT FROM 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(CASE
                WHEN pg_catalog.jsonb_typeof(prepared_promotion_intent -> 'definition')
                        = 'object'
                    THEN prepared_promotion_intent -> 'definition'
                ELSE '{}'::JSONB
            END) AS key(name)
        ) <> 4
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(CASE
                WHEN pg_catalog.jsonb_typeof(prepared_promotion_intent -> 'definition')
                        = 'object'
                    THEN prepared_promotion_intent -> 'definition'
                ELSE '{}'::JSONB
            END) AS key(name)
            WHERE key.name NOT IN ('version', 'panels', 'modals', 'rules')
        )
        OR pg_catalog.jsonb_typeof(prepared_promotion_intent #> '{definition,version}')
            IS DISTINCT FROM 'number'
        OR prepared_promotion_intent #>> '{definition,version}' IS DISTINCT FROM '1'
        OR pg_catalog.jsonb_typeof(prepared_promotion_intent #> '{definition,panels}')
            IS DISTINCT FROM 'array'
        OR pg_catalog.jsonb_typeof(prepared_promotion_intent #> '{definition,modals}')
            IS DISTINCT FROM 'array'
        OR pg_catalog.jsonb_typeof(prepared_promotion_intent #> '{definition,rules}')
            IS DISTINCT FROM 'array'
        OR pg_catalog.octet_length(
            (prepared_promotion_intent -> 'definition')::TEXT
        ) > 524288
        OR pg_catalog.jsonb_typeof(prepared_promotion_intent -> 'preview')
            IS DISTINCT FROM 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(CASE
                WHEN pg_catalog.jsonb_typeof(prepared_promotion_intent -> 'preview')
                        = 'object'
                    THEN prepared_promotion_intent -> 'preview'
                ELSE '{}'::JSONB
            END) AS key(name)
        ) <> 2
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(CASE
                WHEN pg_catalog.jsonb_typeof(prepared_promotion_intent -> 'preview')
                        = 'object'
                    THEN prepared_promotion_intent -> 'preview'
                ELSE '{}'::JSONB
            END) AS key(name)
            WHERE key.name NOT IN ('revision', 'summary')
        )
        OR pg_catalog.jsonb_typeof(prepared_promotion_intent #> '{preview,revision}')
            IS DISTINCT FROM 'number'
        OR pg_catalog.jsonb_typeof(prepared_promotion_intent #> '{preview,summary}')
            IS DISTINCT FROM 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(CASE
                WHEN pg_catalog.jsonb_typeof(
                        prepared_promotion_intent #> '{preview,summary}'
                    ) = 'object'
                    THEN prepared_promotion_intent #> '{preview,summary}'
                ELSE '{}'::JSONB
            END) AS key(name)
        ) <> 5
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_object_keys(CASE
                WHEN pg_catalog.jsonb_typeof(
                        prepared_promotion_intent #> '{preview,summary}'
                    ) = 'object'
                    THEN prepared_promotion_intent #> '{preview,summary}'
                ELSE '{}'::JSONB
            END) AS key(name)
            WHERE key.name NOT IN (
                'panels', 'modals', 'rules', 'actions', 'unresolved_references'
            )
        )
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.unnest(ARRAY[
                'panels', 'modals', 'rules', 'actions'
            ]) AS field(name)
            WHERE pg_catalog.jsonb_typeof(
                prepared_promotion_intent #> '{preview,summary}' -> field.name
            ) IS DISTINCT FROM 'number'
        )
        OR prepared_promotion_intent #>> '{preview,revision}'
            IS DISTINCT FROM expected_candidate_revision::TEXT
        OR pg_catalog.jsonb_typeof(
            prepared_promotion_intent #> '{preview,summary,unresolved_references}'
        ) IS DISTINCT FROM 'array'
        OR pg_catalog.jsonb_array_length(CASE
            WHEN pg_catalog.jsonb_typeof(
                    prepared_promotion_intent
                        #> '{preview,summary,unresolved_references}'
                ) = 'array'
                THEN prepared_promotion_intent
                    #> '{preview,summary,unresolved_references}'
            ELSE '[]'::JSONB
        END) <> 0
        OR prepared_promotion_intent #>> '{preview,summary,panels}' IS DISTINCT FROM
            pg_catalog.jsonb_array_length(CASE
                WHEN pg_catalog.jsonb_typeof(
                        prepared_promotion_intent #> '{definition,panels}'
                    ) = 'array'
                    THEN prepared_promotion_intent #> '{definition,panels}'
                ELSE '[]'::JSONB
            END)::TEXT
        OR prepared_promotion_intent #>> '{preview,summary,modals}' IS DISTINCT FROM
            pg_catalog.jsonb_array_length(CASE
                WHEN pg_catalog.jsonb_typeof(
                        prepared_promotion_intent #> '{definition,modals}'
                    ) = 'array'
                    THEN prepared_promotion_intent #> '{definition,modals}'
                ELSE '[]'::JSONB
            END)::TEXT
        OR prepared_promotion_intent #>> '{preview,summary,rules}' IS DISTINCT FROM
            pg_catalog.jsonb_array_length(CASE
                WHEN pg_catalog.jsonb_typeof(
                        prepared_promotion_intent #> '{definition,rules}'
                    ) = 'array'
                    THEN prepared_promotion_intent #> '{definition,rules}'
                ELSE '[]'::JSONB
            END)::TEXT
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_array_elements(CASE
                WHEN pg_catalog.jsonb_typeof(
                        prepared_promotion_intent #> '{definition,rules}'
                    ) = 'array'
                    THEN prepared_promotion_intent #> '{definition,rules}'
                ELSE '[]'::JSONB
            END) AS rule(value)
            WHERE pg_catalog.jsonb_typeof(rule.value) <> 'object'
                OR pg_catalog.jsonb_typeof(rule.value -> 'actions') <> 'array'
        )
        OR prepared_promotion_intent #>> '{preview,summary,actions}' IS DISTINCT FROM (
            SELECT COALESCE(
                pg_catalog.sum(pg_catalog.jsonb_array_length(CASE
                    WHEN pg_catalog.jsonb_typeof(rule.value -> 'actions') = 'array'
                        THEN rule.value -> 'actions'
                    ELSE '[]'::JSONB
                END)),
                0
            )::TEXT
            FROM pg_catalog.jsonb_array_elements(CASE
                WHEN pg_catalog.jsonb_typeof(
                        prepared_promotion_intent #> '{definition,rules}'
                    ) = 'array'
                    THEN prepared_promotion_intent #> '{definition,rules}'
                ELSE '[]'::JSONB
            END) AS rule(value)
        )
        OR prepared_promotion_intent ->> 'expected_registry_content_hash'
            !~ '^[0-9a-f]{64}$'
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
        OR product_admission_payload ->> 'idempotency_digest_key_fingerprint'
            !~ '^[0-9a-f]{64}$'
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
        OR prepared_promotion_intent #>> '{authority,session_owner_id}'
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
        OR idempotency_key_digest_candidates[1]
            IS DISTINCT FROM active_idempotency_key_digest
        OR idempotency_digest_key_id_candidates[1]
            IS DISTINCT FROM idempotency_digest_key_id
        OR idempotency_digest_key_fingerprint_candidates[1]
            IS DISTINCT FROM product_admission_payload
                ->> 'idempotency_digest_key_fingerprint'
        OR semantic_request_digest !~ '^[0-9a-f]{64}$'
        OR new_receipt_id !~ '^[0-9a-f]{64}$'
        OR new_audit_event_id !~ '^[0-9a-f]{64}$'
        OR pg_catalog.array_ndims(idempotency_key_digest_candidates)
            IS DISTINCT FROM 1
        OR pg_catalog.array_ndims(idempotency_digest_key_id_candidates)
            IS DISTINCT FROM 1
        OR pg_catalog.array_ndims(idempotency_digest_key_fingerprint_candidates)
            IS DISTINCT FROM 1
        OR pg_catalog.array_lower(idempotency_key_digest_candidates, 1)
            IS DISTINCT FROM 1
        OR pg_catalog.array_lower(idempotency_digest_key_id_candidates, 1)
            IS DISTINCT FROM 1
        OR pg_catalog.array_lower(idempotency_digest_key_fingerprint_candidates, 1)
            IS DISTINCT FROM 1
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

    SELECT installation.*
    INTO installation_row
    FROM public.automation_installations AS installation
    WHERE installation.tenant_id = expected_tenant_id
        AND installation.installation_id = expected_installation_id;

    SELECT authority.*
    INTO historical_authority_row
    FROM public.automation_installation_authority_versions AS authority
    WHERE authority.tenant_id = expected_tenant_id
        AND authority.installation_id = expected_installation_id
        AND authority.revision = generation_row.installation_authority_revision;

    IF installation_row.installation_id IS NULL
        OR historical_authority_row.installation_id IS NULL
        OR generation_row.resource_bindings
            IS DISTINCT FROM historical_authority_row.resource_bindings
        OR generation_row.binding_fingerprint
            IS DISTINCT FROM historical_authority_row.binding_fingerprint
    THEN
        RETURN QUERY SELECT 'persistence_corrupt',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            access_result.database_now;
        RETURN;
    END IF;

    IF installation_row.lifecycle_state <> 'active'
        OR installation_row.discord_application_id
            IS DISTINCT FROM expected_discord_application_id
        OR installation_row.discord_guild_id IS DISTINCT FROM expected_guild_id
        OR installation_row.current_authority_revision
            IS DISTINCT FROM observed_current_authority_revision
        OR historical_authority_row.revision
            IS DISTINCT FROM observed_current_authority_revision
        OR historical_authority_row.authority_payload_digest
            IS DISTINCT FROM observed_current_authority_payload_digest
    THEN
        RETURN QUERY SELECT 'scope_mismatch',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            access_result.database_now;
        RETURN;
    END IF;

    IF prepared_promotion_intent #>> '{authority,ruleset_key}'
            IS DISTINCT FROM installation_row.ruleset_key
        OR prepared_promotion_intent #>> '{authority,binding_revision}'
            IS DISTINCT FROM historical_authority_row.binding_revision::TEXT
        OR prepared_promotion_intent #>> '{authority,policy,revision}'
            IS DISTINCT FROM historical_authority_row.policy_revision::TEXT
        OR prepared_promotion_intent #>> '{authority,policy,required_approvals}'
            IS DISTINCT FROM historical_authority_row.required_approvals::TEXT
        OR prepared_promotion_intent #>> '{authority,policy,ttl_seconds}'
            IS DISTINCT FROM historical_authority_row.activation_ttl_seconds::TEXT
    THEN
        RETURN QUERY SELECT 'invalid_candidate',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            access_result.database_now;
        RETURN;
    END IF;

    BEGIN
        registry_schema_version := (
            prepared_promotion_intent ->> 'registry_schema_version'
        )::BIGINT;
    EXCEPTION
        WHEN invalid_text_representation OR numeric_value_out_of_range THEN
            RETURN QUERY SELECT 'invalid_candidate',
                NULL::JSONB,
                NULL::JSONB,
                NULL::TEXT,
                access_result.database_now;
            RETURN;
    END;

    calculated_candidate_hash := pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                'starring.intent.candidate_ruleset.v1',
                'UTF8'
            )
            || pg_catalog.decode('00', 'hex')
            || pg_catalog.convert_to(
                public.starring_canonical_json_v1(
                    prepared_promotion_intent -> 'definition'
                ),
                'UTF8'
            )
        ),
        'hex'
    );
    calculated_content_hash := public.starring_ruleset_content_hash_v1(
        registry_schema_version,
        prepared_promotion_intent -> 'definition'
    );
    IF registry_schema_version NOT BETWEEN 1 AND 4294967295
        OR calculated_candidate_hash IS DISTINCT FROM expected_candidate_hash
        OR calculated_content_hash IS NULL
        OR calculated_content_hash IS DISTINCT FROM
            prepared_promotion_intent ->> 'expected_registry_content_hash'
    THEN
        RETURN QUERY SELECT 'invalid_candidate',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            access_result.database_now;
        RETURN;
    END IF;

    final_clock := pg_catalog.clock_timestamp();

    SELECT product_session.*
    INTO product_session_row
    FROM public.product_auth_sessions AS product_session
    WHERE product_session.session_digest = expected_product_session_digest
        AND product_session.principal_id = expected_principal_id
    FOR SHARE;

    SELECT installation.*
    INTO installation_row
    FROM public.automation_installations AS installation
    WHERE installation.tenant_id = expected_tenant_id
        AND installation.installation_id = expected_installation_id
    FOR SHARE;

    IF product_session_row.principal_id IS NULL
        OR product_session_row.oauth_state_digest IS NULL
        OR product_session_row.revoked_at IS NOT NULL
        OR product_session_row.revocation_reason IS NOT NULL
        OR final_clock >= product_session_row.idle_expires_at
        OR final_clock >= product_session_row.absolute_expires_at
        OR payload_observed_at > final_clock
        OR final_clock >= payload_expires_at
        OR authority_observed_at > final_clock
        OR final_clock >= authority_expires_at
    THEN
        RETURN QUERY SELECT 'access_denied',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            final_clock;
        RETURN;
    END IF;

    IF installation_row.installation_id IS NULL
        OR installation_row.lifecycle_state <> 'active'
        OR installation_row.discord_application_id
            IS DISTINCT FROM expected_discord_application_id
        OR installation_row.discord_guild_id IS DISTINCT FROM expected_guild_id
        OR installation_row.current_authority_revision
            IS DISTINCT FROM observed_current_authority_revision
        OR historical_authority_row.authority_payload_digest
            IS DISTINCT FROM observed_current_authority_payload_digest
    THEN
        RETURN QUERY SELECT 'scope_mismatch',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            final_clock;
        RETURN;
    END IF;

    prepared_record := pg_catalog.jsonb_build_object(
        'id', expected_promotion_id,
        'revision', 1,
        'request_digest', expected_promotion_request_digest,
        'intent', prepared_promotion_intent,
        'stage', pg_catalog.jsonb_build_object('state', 'prepared'),
        'created_at', final_clock,
        'updated_at', final_clock
    );
    admission_document := pg_catalog.jsonb_build_object(
        'format_version', 1,
        'payload', product_admission_payload,
        'admitted_at', final_clock
    );

    IF pg_catalog.octet_length(prepared_record::TEXT) > 8388608
        OR pg_catalog.octet_length(admission_document::TEXT) > 32768
    THEN
        RETURN QUERY SELECT 'invalid_candidate',
            NULL::JSONB,
            NULL::JSONB,
            NULL::TEXT,
            final_clock;
        RETURN;
    END IF;

    INSERT INTO public.authoring_promotions (
        id,
        record_format_version,
        revision,
        stage,
        request_digest,
        tenant_id,
        installation_id,
        principal_id,
        record,
        product_admission_format_version,
        product_admission_digest,
        product_admission
    ) VALUES (
        expected_promotion_id,
        1,
        1,
        'prepared',
        expected_promotion_request_digest,
        expected_tenant_id,
        expected_installation_id,
        expected_principal_id,
        prepared_record,
        1,
        product_admission_digest,
        admission_document
    )
    ON CONFLICT (id) DO NOTHING;
    GET DIAGNOSTICS inserted_count = ROW_COUNT;

    IF inserted_count = 0 THEN
        RAISE EXCEPTION 'product promotion concurrent identity did not converge'
            USING ERRCODE = '40001';
    END IF;

    SELECT promotion.*
    INTO persisted_promotion_row
    FROM public.authoring_promotions AS promotion
    WHERE promotion.id = expected_promotion_id
    FOR SHARE;

    IF persisted_promotion_row.id IS NULL
        OR persisted_promotion_row.record_format_version IS DISTINCT FROM 1
        OR persisted_promotion_row.revision IS DISTINCT FROM 1
        OR persisted_promotion_row.stage IS DISTINCT FROM 'prepared'
        OR persisted_promotion_row.request_digest
            IS DISTINCT FROM expected_promotion_request_digest
        OR persisted_promotion_row.tenant_id IS DISTINCT FROM expected_tenant_id
        OR persisted_promotion_row.installation_id
            IS DISTINCT FROM expected_installation_id
        OR persisted_promotion_row.principal_id IS DISTINCT FROM expected_principal_id
        OR persisted_promotion_row.record IS DISTINCT FROM prepared_record
        OR persisted_promotion_row.product_admission_format_version IS DISTINCT FROM 1
        OR persisted_promotion_row.product_admission_digest
            IS DISTINCT FROM product_admission_digest
        OR persisted_promotion_row.product_admission IS DISTINCT FROM admission_document
        OR (persisted_promotion_row.record ->> 'created_at')::TIMESTAMPTZ
            IS DISTINCT FROM final_clock
        OR (persisted_promotion_row.record ->> 'updated_at')::TIMESTAMPTZ
            IS DISTINCT FROM final_clock
        OR (persisted_promotion_row.product_admission ->> 'admitted_at')::TIMESTAMPTZ
            IS DISTINCT FROM final_clock
        OR persisted_promotion_row.product_admission ->> 'admitted_at'
            IS DISTINCT FROM persisted_promotion_row.record ->> 'created_at'
    THEN
        RAISE EXCEPTION 'product promotion prepared write verification failed'
            USING ERRCODE = '23514';
    END IF;

    RETURN QUERY SELECT 'created',
        persisted_promotion_row.record,
        persisted_promotion_row.product_admission,
        persisted_promotion_row.product_admission_digest,
        final_clock;
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
        'public.enforce_authoring_promotion_scope()',
        'public.enforce_authoring_promotion_product_admission()',
        'public.enforce_authoring_promotion_product_transition()',
        'public.reject_ruleset_artifact_mutation()',
        'public.enforce_product_activation_journal_link()',
        'public.enforce_product_activation_scope()',
        'public.guard_legacy_activation_product_slot()',
        'public.guard_product_ruleset_artifact_transition()',
        'public.assert_product_approval_receipt_alias()',
        'public.assert_product_approval_receipt_audit()',
        'public.enforce_product_action_receipt_retention()',
        'public.enforce_product_action_receipt_alias_capacity()',
        'public.enforce_product_action_receipt_alias_retention()',
        'public.capture_product_action_receipt_audit_evidence()',
        'public.reject_immutable_product_approval_row()',
        'public.starring_canonical_json_v1(jsonb)',
        'public.starring_ruleset_content_hash_v1(bigint,jsonb)',
        'public.starring_product_ruleset_slot_exact_v1(text,text,text,text,bigint)',
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
