SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

LOCK TABLE public.authoring_session_generations IN ACCESS EXCLUSIVE MODE;
LOCK TABLE
    public.product_control_plane_identity,
    public.product_principals,
    public.product_tenants,
    public.automation_installations,
    public.automation_installation_authority_versions,
    public.authoring_sessions
IN SHARE ROW EXCLUSIVE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    relation_count BIGINT;
    ordinary_count BIGINT;
    rls_disabled_count BIGINT;
    owner_count BIGINT;
    identity_count BIGINT;
    unsafe_schema_create_count BIGINT;
    existing_column_count BIGINT;
    existing_constraint_count BIGINT;
    collision_count BIGINT;
    immutable_trigger_count BIGINT;
BEGIN
    SELECT pg_catalog.count(relation.oid),
        pg_catalog.count(relation.oid) FILTER (WHERE relation.relkind = 'r'),
        pg_catalog.count(relation.oid) FILTER (
            WHERE NOT relation.relrowsecurity AND NOT relation.relforcerowsecurity
        ),
        pg_catalog.count(DISTINCT relation.relowner),
        pg_catalog.min(relation.relowner::BIGINT)::OID
    INTO
        relation_count,
        ordinary_count,
        rls_disabled_count,
        owner_count,
        common_owner
    FROM (
        VALUES
            (pg_catalog.to_regclass('public.product_control_plane_identity')),
            (pg_catalog.to_regclass('public.product_principals')),
            (pg_catalog.to_regclass('public.product_tenants')),
            (pg_catalog.to_regclass('public.automation_installations')),
            (pg_catalog.to_regclass(
                'public.automation_installation_authority_versions'
            )),
            (pg_catalog.to_regclass('public.authoring_sessions')),
            (pg_catalog.to_regclass('public.authoring_session_generations'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid;

    IF relation_count <> 7
        OR ordinary_count <> 7
        OR rls_disabled_count <> 7
        OR owner_count <> 1
        OR common_owner IS NULL
    THEN
        RAISE EXCEPTION 'authoring writer relations require one non-RLS owner'
            USING ERRCODE = '55000';
    END IF;

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR NOT pg_catalog.has_schema_privilege(
            common_owner_name,
            'public',
            'CREATE'
        )
    THEN
        RAISE EXCEPTION 'authoring writer migration requires the common owner'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO identity_count
    FROM public.product_control_plane_identity AS identity
    WHERE identity.singleton
        AND identity.database_identity IS NOT NULL
        AND identity.database_identity
            <> '00000000-0000-0000-0000-000000000000'::UUID
        AND identity.created_at IS NOT NULL;
    IF identity_count <> 1 THEN
        RAISE EXCEPTION 'product control plane identity is invalid'
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
        AND privilege.grantee <> namespace.nspowner;
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
        RAISE EXCEPTION 'authoring writer schema is not trusted'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO existing_column_count
    FROM pg_catalog.pg_attribute AS attribute
    WHERE attribute.attrelid = pg_catalog.to_regclass(
            'public.authoring_session_generations'
        )
        AND attribute.attnum > 0
        AND NOT attribute.attisdropped
        AND attribute.attname IN (
            'writer_semantic_request_digest',
            'writer_digest_key_id',
            'writer_digest_key_fingerprint',
            'safe_turn_projection',
            'safe_turn_projection_digest'
        );
    IF existing_column_count <> 0 THEN
        RAISE EXCEPTION 'authoring writer generation column collision'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO existing_constraint_count
    FROM pg_catalog.pg_constraint AS constraint_row
    WHERE constraint_row.conrelid = pg_catalog.to_regclass(
            'public.authoring_session_generations'
        )
        AND constraint_row.conname IN (
            'authoring_generations_writer_metadata_presence_valid',
            'authoring_generations_writer_semantic_digest_valid',
            'authoring_generations_writer_key_identity_valid',
            'authoring_generations_safe_projection_valid',
            'authoring_generations_trusted_stage_valid',
            'authoring_generations_trusted_candidate_valid'
        );
    IF existing_constraint_count <> 0 THEN
        RAISE EXCEPTION 'authoring writer generation constraint collision'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_authoring_session_writer_database_identity_v1',
            'starring_authoring_session_writer_check_v1',
            'starring_authoring_session_writer_load_v1',
            'starring_authoring_session_writer_commit_v1',
            'starring_authoring_session_writer_key_coverage_v1',
            'starring_product_authorized_snapshot_read_v2'
        );
    IF collision_count <> 0 THEN
        RAISE EXCEPTION 'authoring writer function collision'
            USING ERRCODE = '55000';
    END IF;

    IF pg_catalog.to_regprocedure(
        'public.starring_product_authorized_snapshot_read_v1(text,text,bytea,text,text)'
    ) IS NULL
    THEN
        RAISE EXCEPTION 'authorized snapshot reader v1 is unavailable'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO immutable_trigger_count
    FROM pg_catalog.pg_trigger AS trigger_row
    WHERE trigger_row.tgrelid IN (
            pg_catalog.to_regclass('public.authoring_sessions'),
            pg_catalog.to_regclass('public.authoring_session_generations')
        )
        AND NOT trigger_row.tgisinternal
        AND trigger_row.tgenabled = 'O'
        AND trigger_row.tgname IN (
            'authoring_sessions_enforce_transition',
            'authoring_sessions_assert_head_insert',
            'authoring_sessions_assert_head_update',
            'authoring_generations_enforce_sequence',
            'authoring_generations_assert_head',
            'authoring_generations_reject_mutation'
        );
    IF immutable_trigger_count <> 6 THEN
        RAISE EXCEPTION 'authoring generation invariants are unavailable'
            USING ERRCODE = '55000';
    END IF;
END;
$preflight$;

ALTER TABLE public.authoring_session_generations
    ADD COLUMN writer_semantic_request_digest TEXT,
    ADD COLUMN writer_digest_key_id TEXT,
    ADD COLUMN writer_digest_key_fingerprint TEXT,
    ADD COLUMN safe_turn_projection BYTEA,
    ADD COLUMN safe_turn_projection_digest TEXT,
    ADD CONSTRAINT authoring_generations_writer_metadata_presence_valid CHECK (
        (
            writer_semantic_request_digest IS NULL
            AND writer_digest_key_id IS NULL
            AND writer_digest_key_fingerprint IS NULL
            AND safe_turn_projection IS NULL
            AND safe_turn_projection_digest IS NULL
        )
        OR (
            writer_semantic_request_digest IS NOT NULL
            AND writer_digest_key_id IS NOT NULL
            AND writer_digest_key_fingerprint IS NOT NULL
            AND safe_turn_projection IS NOT NULL
            AND safe_turn_projection_digest IS NOT NULL
        )
    ),
    ADD CONSTRAINT authoring_generations_writer_semantic_digest_valid CHECK (
        writer_semantic_request_digest IS NULL
        OR writer_semantic_request_digest ~ '^[0-9a-f]{64}$'
    ),
    ADD CONSTRAINT authoring_generations_writer_key_identity_valid CHECK (
        writer_digest_key_id IS NULL
        OR (
            writer_digest_key_id ~ '^[A-Za-z0-9_.:-]{1,64}$'
            AND writer_digest_key_fingerprint ~ '^[0-9a-f]{64}$'
        )
    ),
    ADD CONSTRAINT authoring_generations_safe_projection_valid CHECK (
        safe_turn_projection IS NULL
        OR (
            pg_catalog.octet_length(safe_turn_projection)
                BETWEEN 1 AND 262144
            AND safe_turn_projection_digest ~ '^[0-9a-f]{64}$'
        )
    ),
    ADD CONSTRAINT authoring_generations_trusted_stage_valid CHECK (
        writer_semantic_request_digest IS NULL
        OR stage IN (
            'needs_input',
            'discussion',
            'capability_gap',
            'preview_ready'
        )
    ),
    ADD CONSTRAINT authoring_generations_trusted_candidate_valid CHECK (
        writer_semantic_request_digest IS NULL
        OR (
            (
                stage = 'preview_ready'
                AND candidate_revision IS NOT NULL
                AND candidate_hash IS NOT NULL
            )
            OR (
                stage <> 'preview_ready'
                AND candidate_revision IS NULL
                AND candidate_hash IS NULL
            )
        )
    );

CREATE FUNCTION public.starring_authoring_session_writer_database_identity_v1()
RETURNS TEXT
LANGUAGE sql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
    SELECT identity.database_identity::TEXT
    FROM public.product_control_plane_identity AS identity
    WHERE identity.singleton;
$function$;

CREATE FUNCTION public.starring_authoring_session_writer_check_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_principal_id TEXT,
    expected_session_id TEXT,
    expected_generation BIGINT,
    writer_request_digest_candidates TEXT[],
    writer_semantic_digest_candidates TEXT[],
    writer_digest_key_id_candidates TEXT[],
    writer_digest_key_fingerprint_candidates TEXT[]
)
RETURNS TABLE (
    outcome_code TEXT,
    current_generation BIGINT,
    matched_generation BIGINT,
    safe_turn_projection BYTEA,
    safe_turn_projection_digest TEXT
)
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
DECLARE
    candidate_count INTEGER;
    candidate_index INTEGER;
    access_is_active BOOLEAN;
    session_row RECORD;
    generation_row RECORD;
BEGIN
    candidate_count := pg_catalog.cardinality(
        writer_request_digest_candidates
    );
    IF expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_principal_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_session_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_generation NOT BETWEEN 0 AND 9223372036854775806
        OR candidate_count NOT BETWEEN 1 AND 8
        OR pg_catalog.array_ndims(writer_request_digest_candidates) <> 1
        OR pg_catalog.array_ndims(writer_semantic_digest_candidates) <> 1
        OR pg_catalog.array_ndims(writer_digest_key_id_candidates) <> 1
        OR pg_catalog.array_ndims(
            writer_digest_key_fingerprint_candidates
        ) <> 1
        OR pg_catalog.array_lower(writer_request_digest_candidates, 1) <> 1
        OR pg_catalog.array_lower(writer_semantic_digest_candidates, 1) <> 1
        OR pg_catalog.array_lower(writer_digest_key_id_candidates, 1) <> 1
        OR pg_catalog.array_lower(
            writer_digest_key_fingerprint_candidates,
            1
        ) <> 1
        OR pg_catalog.cardinality(writer_semantic_digest_candidates)
            <> candidate_count
        OR pg_catalog.cardinality(writer_digest_key_id_candidates)
            <> candidate_count
        OR pg_catalog.cardinality(writer_digest_key_fingerprint_candidates)
            <> candidate_count
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.generate_series(1, candidate_count) AS item(index)
            WHERE writer_request_digest_candidates[item.index] IS NULL
                OR writer_request_digest_candidates[item.index]
                    !~ '^[0-9a-f]{64}$'
                OR writer_semantic_digest_candidates[item.index] IS NULL
                OR writer_semantic_digest_candidates[item.index]
                    !~ '^[0-9a-f]{64}$'
                OR writer_digest_key_id_candidates[item.index] IS NULL
                OR writer_digest_key_id_candidates[item.index]
                    !~ '^[A-Za-z0-9_.:-]{1,64}$'
                OR writer_digest_key_fingerprint_candidates[item.index]
                    IS NULL
                OR writer_digest_key_fingerprint_candidates[item.index]
                    !~ '^[0-9a-f]{64}$'
        )
        OR (
            SELECT pg_catalog.count(DISTINCT candidate.request_digest)
            FROM pg_catalog.unnest(writer_request_digest_candidates)
                AS candidate(request_digest)
        ) <> candidate_count
        OR (
            SELECT pg_catalog.count(DISTINCT candidate.key_id)
            FROM pg_catalog.unnest(writer_digest_key_id_candidates)
                AS candidate(key_id)
        ) <> candidate_count
        OR (
            SELECT pg_catalog.count(DISTINCT candidate.key_fingerprint)
            FROM pg_catalog.unnest(writer_digest_key_fingerprint_candidates)
                AS candidate(key_fingerprint)
        ) <> candidate_count
    THEN
        RAISE EXCEPTION 'authoring writer check input is invalid'
            USING ERRCODE = '22023';
    END IF;

    SELECT EXISTS (
        SELECT 1
        FROM public.product_principals AS principal
        INNER JOIN public.product_tenants AS tenant
            ON tenant.tenant_id = expected_tenant_id
            AND tenant.lifecycle_state = 'active'
        INNER JOIN public.automation_installations AS installation
            ON installation.tenant_id = tenant.tenant_id
            AND installation.installation_id = expected_installation_id
            AND installation.lifecycle_state = 'active'
        INNER JOIN public.automation_installation_authority_versions AS authority
            ON authority.tenant_id = installation.tenant_id
            AND authority.installation_id = installation.installation_id
            AND authority.revision = installation.current_authority_revision
        WHERE principal.principal_id = expected_principal_id
            AND NOT principal.disabled
    )
    INTO access_is_active;

    IF NOT access_is_active THEN
        RETURN QUERY
        SELECT 'invalid_state'::TEXT, NULL::BIGINT, NULL::BIGINT,
            NULL::BYTEA, NULL::TEXT;
        RETURN;
    END IF;

    SELECT
        authoring_session.tenant_id,
        authoring_session.installation_id,
        authoring_session.owner_principal_id,
        authoring_session.current_generation,
        authoring_session.lifecycle_state
    INTO session_row
    FROM public.authoring_sessions AS authoring_session
    WHERE authoring_session.session_id = expected_session_id;

    IF NOT FOUND THEN
        IF expected_generation = 0 THEN
            RETURN QUERY
            SELECT 'proceed'::TEXT, NULL::BIGINT, NULL::BIGINT,
                NULL::BYTEA, NULL::TEXT;
        ELSE
            RETURN QUERY
            SELECT 'generation_conflict'::TEXT, NULL::BIGINT, NULL::BIGINT,
                NULL::BYTEA, NULL::TEXT;
        END IF;
        RETURN;
    END IF;

    IF session_row.tenant_id IS DISTINCT FROM expected_tenant_id
        OR session_row.installation_id IS DISTINCT FROM expected_installation_id
        OR session_row.owner_principal_id IS DISTINCT FROM expected_principal_id
        OR session_row.lifecycle_state IS DISTINCT FROM 'active'
    THEN
        RETURN QUERY
        SELECT 'invalid_state'::TEXT,
            session_row.current_generation::BIGINT,
            NULL::BIGINT,
            NULL::BYTEA,
            NULL::TEXT;
        RETURN;
    END IF;

    SELECT
        generation.generation,
        generation.writer_semantic_request_digest,
        generation.writer_digest_key_id,
        generation.writer_digest_key_fingerprint,
        generation.safe_turn_projection,
        generation.safe_turn_projection_digest,
        pg_catalog.array_position(
            writer_request_digest_candidates,
            generation.writer_request_digest
        ) AS candidate_index
    INTO generation_row
    FROM public.authoring_session_generations AS generation
    WHERE generation.tenant_id = expected_tenant_id
        AND generation.installation_id = expected_installation_id
        AND generation.session_id = expected_session_id
        AND generation.writer_request_digest
            = ANY(writer_request_digest_candidates)
    ORDER BY generation.generation
    LIMIT 1;

    IF FOUND THEN
        candidate_index := generation_row.candidate_index;
        IF candidate_index IS NULL
            OR generation_row.writer_semantic_request_digest IS NULL
            OR generation_row.writer_digest_key_id IS NULL
            OR generation_row.writer_digest_key_fingerprint IS NULL
            OR generation_row.safe_turn_projection IS NULL
            OR generation_row.safe_turn_projection_digest IS NULL
            OR generation_row.writer_digest_key_id
                IS DISTINCT FROM writer_digest_key_id_candidates[candidate_index]
            OR generation_row.writer_digest_key_fingerprint
                IS DISTINCT FROM
                    writer_digest_key_fingerprint_candidates[candidate_index]
        THEN
            RETURN QUERY
            SELECT 'invalid_state'::TEXT,
                session_row.current_generation::BIGINT,
                NULL::BIGINT,
                NULL::BYTEA,
                NULL::TEXT;
            RETURN;
        END IF;

        IF generation_row.writer_semantic_request_digest
            = writer_semantic_digest_candidates[candidate_index]
        THEN
            RETURN QUERY
            SELECT 'exact_replay'::TEXT,
                session_row.current_generation::BIGINT,
                generation_row.generation::BIGINT,
                generation_row.safe_turn_projection::BYTEA,
                generation_row.safe_turn_projection_digest::TEXT;
        ELSE
            RETURN QUERY
            SELECT 'idempotency_conflict'::TEXT,
                session_row.current_generation::BIGINT,
                generation_row.generation::BIGINT,
                NULL::BYTEA,
                NULL::TEXT;
        END IF;
        RETURN;
    END IF;

    IF session_row.current_generation IS DISTINCT FROM expected_generation THEN
        RETURN QUERY
        SELECT 'generation_conflict'::TEXT,
            session_row.current_generation::BIGINT,
            NULL::BIGINT,
            NULL::BYTEA,
            NULL::TEXT;
    ELSE
        RETURN QUERY
        SELECT 'proceed'::TEXT,
            session_row.current_generation::BIGINT,
            NULL::BIGINT,
            NULL::BYTEA,
            NULL::TEXT;
    END IF;
END;
$function$;

CREATE FUNCTION public.starring_product_authorized_snapshot_read_v2(
    expected_session_id TEXT,
    expected_principal_id TEXT,
    expected_product_session_digest BYTEA,
    expected_tenant_id TEXT,
    expected_installation_id TEXT
)
RETURNS TABLE (
    session_tenant_id TEXT,
    session_installation_id TEXT,
    owner_principal_id TEXT,
    owner_discord_user_id TEXT,
    owner_disabled BOOLEAN,
    actor_session_digest BYTEA,
    current_generation BIGINT,
    session_lifecycle_state TEXT,
    tenant_lifecycle_state TEXT,
    installation_tenant_id TEXT,
    discord_application_id TEXT,
    discord_guild_id TEXT,
    ruleset_key TEXT,
    installation_lifecycle_state TEXT,
    current_authority_revision BIGINT,
    generation BIGINT,
    snapshot_schema_version BIGINT,
    snapshot_ciphertext BYTEA,
    snapshot_nonce BYTEA,
    encryption_key_id TEXT,
    encryption_suite TEXT,
    encryption_suite_version SMALLINT,
    authenticated_metadata_digest TEXT,
    generation_resource_bindings JSONB,
    generation_binding_fingerprint TEXT,
    installation_authority_revision BIGINT,
    generation_stage TEXT,
    candidate_revision BIGINT,
    candidate_hash TEXT,
    harness_contract_revision BIGINT,
    authority_tenant_id TEXT,
    binding_revision BIGINT,
    authority_resource_bindings JSONB,
    authority_binding_fingerprint TEXT,
    policy_revision BIGINT,
    required_approvals INTEGER,
    activation_ttl_seconds BIGINT,
    authority_payload_digest TEXT,
    database_now TIMESTAMPTZ,
    writer_request_digest TEXT,
    writer_semantic_request_digest TEXT,
    writer_digest_key_id TEXT,
    writer_digest_key_fingerprint TEXT,
    safe_turn_projection BYTEA,
    safe_turn_projection_digest TEXT
)
LANGUAGE sql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
    WITH request_clock AS MATERIALIZED (
        SELECT pg_catalog.clock_timestamp() AS database_now
    )
    SELECT
        authoring_session.tenant_id AS session_tenant_id,
        authoring_session.installation_id AS session_installation_id,
        authoring_session.owner_principal_id,
        principal.discord_user_id AS owner_discord_user_id,
        principal.disabled AS owner_disabled,
        actor_session.session_digest AS actor_session_digest,
        authoring_session.current_generation,
        authoring_session.lifecycle_state AS session_lifecycle_state,
        tenant.lifecycle_state AS tenant_lifecycle_state,
        installation.tenant_id AS installation_tenant_id,
        installation.discord_application_id,
        installation.discord_guild_id,
        installation.ruleset_key,
        installation.lifecycle_state AS installation_lifecycle_state,
        installation.current_authority_revision,
        generation.generation,
        generation.snapshot_schema_version,
        generation.snapshot_ciphertext,
        generation.snapshot_nonce,
        generation.encryption_key_id,
        generation.encryption_suite,
        generation.encryption_suite_version,
        generation.authenticated_metadata_digest,
        generation.resource_bindings AS generation_resource_bindings,
        generation.binding_fingerprint AS generation_binding_fingerprint,
        generation.installation_authority_revision,
        generation.stage AS generation_stage,
        generation.candidate_revision,
        generation.candidate_hash,
        generation.harness_contract_revision,
        authority.tenant_id AS authority_tenant_id,
        authority.binding_revision,
        authority.resource_bindings AS authority_resource_bindings,
        authority.binding_fingerprint AS authority_binding_fingerprint,
        authority.policy_revision,
        authority.required_approvals,
        authority.activation_ttl_seconds,
        authority.authority_payload_digest,
        request_clock.database_now,
        generation.writer_request_digest,
        generation.writer_semantic_request_digest,
        generation.writer_digest_key_id,
        generation.writer_digest_key_fingerprint,
        generation.safe_turn_projection,
        generation.safe_turn_projection_digest
    FROM public.authoring_sessions AS authoring_session
    INNER JOIN public.product_principals AS principal
        ON principal.principal_id = authoring_session.owner_principal_id
        AND principal.principal_id = expected_principal_id
    INNER JOIN public.product_auth_sessions AS actor_session
        ON actor_session.principal_id = principal.principal_id
        AND actor_session.session_digest = expected_product_session_digest
    CROSS JOIN request_clock
    LEFT JOIN public.product_tenants AS tenant
        ON tenant.tenant_id = authoring_session.tenant_id
    LEFT JOIN public.automation_installations AS installation
        ON installation.tenant_id = authoring_session.tenant_id
        AND installation.installation_id = authoring_session.installation_id
    LEFT JOIN public.authoring_session_generations AS generation
        ON generation.tenant_id = authoring_session.tenant_id
        AND generation.installation_id = authoring_session.installation_id
        AND generation.session_id = authoring_session.session_id
        AND generation.generation = authoring_session.current_generation
    LEFT JOIN public.automation_installation_authority_versions AS authority
        ON authority.tenant_id = generation.tenant_id
        AND authority.installation_id = generation.installation_id
        AND authority.revision = generation.installation_authority_revision
    WHERE authoring_session.session_id = expected_session_id
        AND authoring_session.tenant_id = expected_tenant_id
        AND authoring_session.installation_id = expected_installation_id
        AND expected_session_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND expected_principal_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND expected_tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND expected_installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND pg_catalog.octet_length(expected_product_session_digest) = 32
        AND NOT principal.disabled
        AND pg_catalog.octet_length(actor_session.csrf_digest) = 32
        AND pg_catalog.octet_length(actor_session.oauth_state_digest) = 32
        AND actor_session.revoked_at IS NULL
        AND actor_session.revocation_reason IS NULL
        AND actor_session.authenticated_at = actor_session.created_at
        AND actor_session.created_at <= actor_session.last_seen_at
        AND actor_session.last_seen_at <= request_clock.database_now
        AND actor_session.last_seen_at < actor_session.idle_expires_at
        AND actor_session.idle_expires_at <= actor_session.absolute_expires_at
        AND actor_session.idle_expires_at
            <= actor_session.last_seen_at + INTERVAL '30 minutes'
        AND actor_session.absolute_expires_at
            <= actor_session.authenticated_at + INTERVAL '12 hours'
        AND request_clock.database_now < actor_session.idle_expires_at
        AND request_clock.database_now < actor_session.absolute_expires_at;
$function$;

CREATE FUNCTION public.starring_authoring_session_writer_commit_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_principal_id TEXT,
    expected_session_id TEXT,
    expected_generation BIGINT,
    writer_request_digest_candidates TEXT[],
    writer_semantic_digest_candidates TEXT[],
    writer_digest_key_id_candidates TEXT[],
    writer_digest_key_fingerprint_candidates TEXT[],
    new_writer_request_digest TEXT,
    new_writer_semantic_request_digest TEXT,
    new_writer_digest_key_id TEXT,
    new_writer_digest_key_fingerprint TEXT,
    new_snapshot_schema_version BIGINT,
    new_snapshot_ciphertext BYTEA,
    new_snapshot_nonce BYTEA,
    new_encryption_key_id TEXT,
    new_encryption_suite TEXT,
    new_encryption_suite_version SMALLINT,
    new_authenticated_metadata_digest TEXT,
    new_resource_bindings JSONB,
    new_binding_fingerprint TEXT,
    new_installation_authority_revision BIGINT,
    new_installation_authority_payload_digest TEXT,
    new_summary JSONB,
    new_stage TEXT,
    new_candidate_revision BIGINT,
    new_candidate_hash TEXT,
    new_safe_turn_projection BYTEA,
    new_safe_turn_projection_digest TEXT,
    new_harness_contract_revision BIGINT
)
RETURNS TABLE (
    outcome_code TEXT,
    current_generation BIGINT,
    committed_generation BIGINT,
    safe_turn_projection BYTEA,
    safe_turn_projection_digest TEXT
)
LANGUAGE plpgsql
VOLATILE
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog, public
ROWS 1
AS $function$
DECLARE
    candidate_count INTEGER;
    candidate_index INTEGER;
    successor_generation BIGINT;
    access_row RECORD;
    session_row RECORD;
    generation_row RECORD;
BEGIN
    IF expected_tenant_id IS NULL
        OR expected_installation_id IS NULL
        OR expected_principal_id IS NULL
        OR expected_session_id IS NULL
        OR expected_generation IS NULL
        OR writer_request_digest_candidates IS NULL
        OR writer_semantic_digest_candidates IS NULL
        OR writer_digest_key_id_candidates IS NULL
        OR writer_digest_key_fingerprint_candidates IS NULL
        OR new_writer_request_digest IS NULL
        OR new_writer_semantic_request_digest IS NULL
        OR new_writer_digest_key_id IS NULL
        OR new_writer_digest_key_fingerprint IS NULL
        OR new_snapshot_schema_version IS NULL
        OR new_snapshot_ciphertext IS NULL
        OR new_snapshot_nonce IS NULL
        OR new_encryption_key_id IS NULL
        OR new_encryption_suite IS NULL
        OR new_encryption_suite_version IS NULL
        OR new_authenticated_metadata_digest IS NULL
        OR new_resource_bindings IS NULL
        OR new_binding_fingerprint IS NULL
        OR new_installation_authority_revision IS NULL
        OR new_installation_authority_payload_digest IS NULL
        OR new_summary IS NULL
        OR new_stage IS NULL
        OR new_safe_turn_projection IS NULL
        OR new_safe_turn_projection_digest IS NULL
        OR new_harness_contract_revision IS NULL
    THEN
        RAISE EXCEPTION 'authoring writer commit input is invalid'
            USING ERRCODE = '22023';
    END IF;

    candidate_count := pg_catalog.cardinality(
        writer_request_digest_candidates
    );
    IF expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_principal_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_session_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_generation NOT BETWEEN 0 AND 9223372036854775806
        OR candidate_count NOT BETWEEN 1 AND 8
        OR pg_catalog.array_ndims(writer_request_digest_candidates) <> 1
        OR pg_catalog.array_ndims(writer_semantic_digest_candidates) <> 1
        OR pg_catalog.array_ndims(writer_digest_key_id_candidates) <> 1
        OR pg_catalog.array_ndims(
            writer_digest_key_fingerprint_candidates
        ) <> 1
        OR pg_catalog.array_lower(writer_request_digest_candidates, 1) <> 1
        OR pg_catalog.array_lower(writer_semantic_digest_candidates, 1) <> 1
        OR pg_catalog.array_lower(writer_digest_key_id_candidates, 1) <> 1
        OR pg_catalog.array_lower(
            writer_digest_key_fingerprint_candidates,
            1
        ) <> 1
        OR pg_catalog.cardinality(writer_semantic_digest_candidates)
            <> candidate_count
        OR pg_catalog.cardinality(writer_digest_key_id_candidates)
            <> candidate_count
        OR pg_catalog.cardinality(writer_digest_key_fingerprint_candidates)
            <> candidate_count
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.generate_series(1, candidate_count) AS item(index)
            WHERE writer_request_digest_candidates[item.index] IS NULL
                OR writer_request_digest_candidates[item.index]
                    !~ '^[0-9a-f]{64}$'
                OR writer_semantic_digest_candidates[item.index] IS NULL
                OR writer_semantic_digest_candidates[item.index]
                    !~ '^[0-9a-f]{64}$'
                OR writer_digest_key_id_candidates[item.index] IS NULL
                OR writer_digest_key_id_candidates[item.index]
                    !~ '^[A-Za-z0-9_.:-]{1,64}$'
                OR writer_digest_key_fingerprint_candidates[item.index]
                    IS NULL
                OR writer_digest_key_fingerprint_candidates[item.index]
                    !~ '^[0-9a-f]{64}$'
        )
        OR (
            SELECT pg_catalog.count(DISTINCT candidate.request_digest)
            FROM pg_catalog.unnest(writer_request_digest_candidates)
                AS candidate(request_digest)
        ) <> candidate_count
        OR (
            SELECT pg_catalog.count(DISTINCT candidate.key_id)
            FROM pg_catalog.unnest(writer_digest_key_id_candidates)
                AS candidate(key_id)
        ) <> candidate_count
        OR (
            SELECT pg_catalog.count(DISTINCT candidate.key_fingerprint)
            FROM pg_catalog.unnest(writer_digest_key_fingerprint_candidates)
                AS candidate(key_fingerprint)
        ) <> candidate_count
        OR new_writer_request_digest !~ '^[0-9a-f]{64}$'
        OR new_writer_semantic_request_digest !~ '^[0-9a-f]{64}$'
        OR new_writer_digest_key_id !~ '^[A-Za-z0-9_.:-]{1,64}$'
        OR new_writer_digest_key_fingerprint !~ '^[0-9a-f]{64}$'
        OR new_writer_request_digest
            IS DISTINCT FROM writer_request_digest_candidates[1]
        OR new_writer_semantic_request_digest
            IS DISTINCT FROM writer_semantic_digest_candidates[1]
        OR new_writer_digest_key_id
            IS DISTINCT FROM writer_digest_key_id_candidates[1]
        OR new_writer_digest_key_fingerprint
            IS DISTINCT FROM writer_digest_key_fingerprint_candidates[1]
        OR new_snapshot_schema_version NOT BETWEEN 1 AND 4294967295
        OR pg_catalog.octet_length(new_snapshot_ciphertext)
            NOT BETWEEN 16 AND 8388608
        OR pg_catalog.octet_length(new_snapshot_nonce) <> 24
        OR new_encryption_key_id !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR new_encryption_suite <> 'xchacha20_poly1305'
        OR new_encryption_suite_version <> 1
        OR new_authenticated_metadata_digest !~ '^[0-9a-f]{64}$'
        OR pg_catalog.jsonb_typeof(new_resource_bindings) <> 'object'
        OR pg_catalog.octet_length(new_resource_bindings::TEXT) > 262144
        OR new_binding_fingerprint !~ '^[0-9a-f]{64}$'
        OR new_installation_authority_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR new_installation_authority_payload_digest
            !~ '^[0-9a-f]{64}$'
        OR pg_catalog.jsonb_typeof(new_summary) <> 'object'
        OR pg_catalog.octet_length(new_summary::TEXT) > 32768
        OR new_stage NOT IN (
            'needs_input',
            'discussion',
            'capability_gap',
            'preview_ready'
        )
        OR (
            new_stage = 'preview_ready'
            AND (
                new_candidate_revision IS NULL
                OR new_candidate_revision
                    NOT BETWEEN 1 AND 9223372036854775807
                OR new_candidate_hash IS NULL
                OR new_candidate_hash !~ '^[0-9a-f]{64}$'
            )
        )
        OR (
            new_stage <> 'preview_ready'
            AND (
                new_candidate_revision IS NOT NULL
                OR new_candidate_hash IS NOT NULL
            )
        )
        OR pg_catalog.octet_length(new_safe_turn_projection)
            NOT BETWEEN 1 AND 262144
        OR new_safe_turn_projection_digest !~ '^[0-9a-f]{64}$'
        OR new_harness_contract_revision
            NOT BETWEEN 1 AND 9223372036854775807
    THEN
        RAISE EXCEPTION 'authoring writer commit input is invalid'
            USING ERRCODE = '22023';
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            'starring-authoring-session-writer-v1'
                || pg_catalog.chr(31)
                || expected_session_id,
            0
        )
    );

    SELECT
        installation.current_authority_revision,
        authority.authority_payload_digest,
        authority.resource_bindings,
        authority.binding_fingerprint
    INTO access_row
    FROM public.product_principals AS principal
    INNER JOIN public.product_tenants AS tenant
        ON tenant.tenant_id = expected_tenant_id
        AND tenant.lifecycle_state = 'active'
    INNER JOIN public.automation_installations AS installation
        ON installation.tenant_id = tenant.tenant_id
        AND installation.installation_id = expected_installation_id
        AND installation.lifecycle_state = 'active'
    INNER JOIN public.automation_installation_authority_versions AS authority
        ON authority.tenant_id = installation.tenant_id
        AND authority.installation_id = installation.installation_id
        AND authority.revision = installation.current_authority_revision
    WHERE principal.principal_id = expected_principal_id
        AND NOT principal.disabled
    FOR SHARE OF principal, tenant, installation, authority;

    IF NOT FOUND THEN
        outcome_code := 'invalid_state';
        current_generation := NULL;
        committed_generation := NULL;
        safe_turn_projection := NULL;
        safe_turn_projection_digest := NULL;
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT
        authoring_session.tenant_id,
        authoring_session.installation_id,
        authoring_session.owner_principal_id,
        authoring_session.current_generation,
        authoring_session.lifecycle_state
    INTO session_row
    FROM public.authoring_sessions AS authoring_session
    WHERE authoring_session.session_id = expected_session_id
    FOR UPDATE;

    IF FOUND THEN
        IF session_row.tenant_id IS DISTINCT FROM expected_tenant_id
            OR session_row.installation_id
                IS DISTINCT FROM expected_installation_id
            OR session_row.owner_principal_id
                IS DISTINCT FROM expected_principal_id
            OR session_row.lifecycle_state IS DISTINCT FROM 'active'
        THEN
            outcome_code := 'invalid_state';
            current_generation := session_row.current_generation;
            committed_generation := NULL;
            safe_turn_projection := NULL;
            safe_turn_projection_digest := NULL;
            RETURN NEXT;
            RETURN;
        END IF;

        SELECT
            generation.generation,
            generation.writer_semantic_request_digest,
            generation.writer_digest_key_id,
            generation.writer_digest_key_fingerprint,
            generation.safe_turn_projection,
            generation.safe_turn_projection_digest,
            pg_catalog.array_position(
                writer_request_digest_candidates,
                generation.writer_request_digest
            ) AS candidate_index
        INTO generation_row
        FROM public.authoring_session_generations AS generation
        WHERE generation.tenant_id = expected_tenant_id
            AND generation.installation_id = expected_installation_id
            AND generation.session_id = expected_session_id
            AND generation.writer_request_digest
                = ANY(writer_request_digest_candidates)
        ORDER BY generation.generation
        LIMIT 1;

        IF FOUND THEN
            candidate_index := generation_row.candidate_index;
            IF candidate_index IS NULL
                OR generation_row.writer_semantic_request_digest IS NULL
                OR generation_row.writer_digest_key_id IS NULL
                OR generation_row.writer_digest_key_fingerprint IS NULL
                OR generation_row.safe_turn_projection IS NULL
                OR generation_row.safe_turn_projection_digest IS NULL
                OR generation_row.writer_digest_key_id
                    IS DISTINCT FROM
                        writer_digest_key_id_candidates[candidate_index]
                OR generation_row.writer_digest_key_fingerprint
                    IS DISTINCT FROM
                        writer_digest_key_fingerprint_candidates[candidate_index]
            THEN
                outcome_code := 'invalid_state';
                current_generation := session_row.current_generation;
                committed_generation := NULL;
                safe_turn_projection := NULL;
                safe_turn_projection_digest := NULL;
            ELSIF generation_row.writer_semantic_request_digest
                = writer_semantic_digest_candidates[candidate_index]
            THEN
                outcome_code := 'exact_replay';
                current_generation := session_row.current_generation;
                committed_generation := generation_row.generation;
                safe_turn_projection := generation_row.safe_turn_projection;
                safe_turn_projection_digest :=
                    generation_row.safe_turn_projection_digest;
            ELSE
                outcome_code := 'idempotency_conflict';
                current_generation := session_row.current_generation;
                committed_generation := generation_row.generation;
                safe_turn_projection := NULL;
                safe_turn_projection_digest := NULL;
            END IF;
            RETURN NEXT;
            RETURN;
        END IF;

        IF session_row.current_generation IS DISTINCT FROM expected_generation
        THEN
            outcome_code := 'generation_conflict';
            current_generation := session_row.current_generation;
            committed_generation := NULL;
            safe_turn_projection := NULL;
            safe_turn_projection_digest := NULL;
            RETURN NEXT;
            RETURN;
        END IF;
    ELSIF expected_generation <> 0 THEN
        outcome_code := 'generation_conflict';
        current_generation := NULL;
        committed_generation := NULL;
        safe_turn_projection := NULL;
        safe_turn_projection_digest := NULL;
        RETURN NEXT;
        RETURN;
    END IF;

    IF access_row.current_authority_revision
            IS DISTINCT FROM new_installation_authority_revision
        OR access_row.authority_payload_digest
            IS DISTINCT FROM new_installation_authority_payload_digest
    THEN
        outcome_code := 'authority_conflict';
        current_generation := CASE
            WHEN session_row IS NULL THEN NULL
            ELSE session_row.current_generation
        END;
        committed_generation := NULL;
        safe_turn_projection := NULL;
        safe_turn_projection_digest := NULL;
        RETURN NEXT;
        RETURN;
    END IF;

    IF access_row.resource_bindings IS DISTINCT FROM new_resource_bindings
        OR access_row.binding_fingerprint
            IS DISTINCT FROM new_binding_fingerprint
    THEN
        outcome_code := 'binding_conflict';
        current_generation := CASE
            WHEN session_row IS NULL THEN NULL
            ELSE session_row.current_generation
        END;
        committed_generation := NULL;
        safe_turn_projection := NULL;
        safe_turn_projection_digest := NULL;
        RETURN NEXT;
        RETURN;
    END IF;

    successor_generation := expected_generation + 1;
    IF expected_generation = 0 THEN
        INSERT INTO public.authoring_sessions (
            session_id,
            tenant_id,
            installation_id,
            owner_principal_id,
            current_generation,
            lifecycle_state
        )
        VALUES (
            expected_session_id,
            expected_tenant_id,
            expected_installation_id,
            expected_principal_id,
            successor_generation,
            'active'
        );
    END IF;

    INSERT INTO public.authoring_session_generations (
        session_id,
        generation,
        tenant_id,
        installation_id,
        snapshot_schema_version,
        snapshot_ciphertext,
        snapshot_nonce,
        encryption_key_id,
        encryption_suite,
        encryption_suite_version,
        authenticated_metadata_digest,
        resource_bindings,
        binding_fingerprint,
        installation_authority_revision,
        summary,
        stage,
        candidate_revision,
        candidate_hash,
        writer_request_digest,
        harness_contract_revision,
        writer_semantic_request_digest,
        writer_digest_key_id,
        writer_digest_key_fingerprint,
        safe_turn_projection,
        safe_turn_projection_digest
    )
    VALUES (
        expected_session_id,
        successor_generation,
        expected_tenant_id,
        expected_installation_id,
        new_snapshot_schema_version,
        new_snapshot_ciphertext,
        new_snapshot_nonce,
        new_encryption_key_id,
        new_encryption_suite,
        new_encryption_suite_version,
        new_authenticated_metadata_digest,
        new_resource_bindings,
        new_binding_fingerprint,
        new_installation_authority_revision,
        new_summary,
        new_stage,
        new_candidate_revision,
        new_candidate_hash,
        new_writer_request_digest,
        new_harness_contract_revision,
        new_writer_semantic_request_digest,
        new_writer_digest_key_id,
        new_writer_digest_key_fingerprint,
        new_safe_turn_projection,
        new_safe_turn_projection_digest
    );

    IF expected_generation <> 0 THEN
        UPDATE public.authoring_sessions AS authoring_session
        SET current_generation = successor_generation,
            updated_at = GREATEST(
                pg_catalog.clock_timestamp(),
                authoring_session.updated_at + INTERVAL '1 microsecond'
            )
        WHERE authoring_session.session_id = expected_session_id
            AND authoring_session.tenant_id = expected_tenant_id
            AND authoring_session.installation_id = expected_installation_id
            AND authoring_session.owner_principal_id = expected_principal_id
            AND authoring_session.current_generation = expected_generation
            AND authoring_session.lifecycle_state = 'active';
        IF NOT FOUND THEN
            RAISE EXCEPTION 'authoring writer session head changed while locked'
                USING ERRCODE = '40001';
        END IF;
    END IF;

    SET CONSTRAINTS
        authoring_sessions_assert_head_insert,
        authoring_sessions_assert_head_update,
        authoring_generations_assert_head
        IMMEDIATE;
    SET CONSTRAINTS
        authoring_sessions_assert_head_insert,
        authoring_sessions_assert_head_update,
        authoring_generations_assert_head
        DEFERRED;

    outcome_code := 'committed';
    current_generation := successor_generation;
    committed_generation := successor_generation;
    safe_turn_projection := new_safe_turn_projection;
    safe_turn_projection_digest := new_safe_turn_projection_digest;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_authoring_session_writer_key_coverage_v1(
    configured_encryption_key_ids TEXT[],
    configured_writer_digest_key_ids TEXT[],
    configured_writer_digest_key_fingerprints TEXT[]
)
RETURNS TABLE (covered BOOLEAN)
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
DECLARE
    encryption_key_count INTEGER;
    digest_key_count INTEGER;
    input_is_valid BOOLEAN;
BEGIN
    encryption_key_count := pg_catalog.cardinality(
        configured_encryption_key_ids
    );
    digest_key_count := pg_catalog.cardinality(
        configured_writer_digest_key_ids
    );
    input_is_valid :=
        encryption_key_count BETWEEN 1 AND 8
        AND digest_key_count BETWEEN 1 AND 8
        AND pg_catalog.array_ndims(configured_encryption_key_ids) = 1
        AND pg_catalog.array_ndims(configured_writer_digest_key_ids) = 1
        AND pg_catalog.array_ndims(
            configured_writer_digest_key_fingerprints
        ) = 1
        AND pg_catalog.array_lower(configured_encryption_key_ids, 1) = 1
        AND pg_catalog.array_lower(configured_writer_digest_key_ids, 1) = 1
        AND pg_catalog.array_lower(
            configured_writer_digest_key_fingerprints,
            1
        ) = 1
        AND pg_catalog.cardinality(
            configured_writer_digest_key_fingerprints
        ) = digest_key_count
        AND NOT EXISTS (
            SELECT 1
            FROM pg_catalog.unnest(configured_encryption_key_ids)
                AS encryption_key(key_id)
            WHERE encryption_key.key_id IS NULL
                OR encryption_key.key_id
                    !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        )
        AND NOT EXISTS (
            SELECT 1
            FROM pg_catalog.generate_series(1, digest_key_count)
                AS digest_key(index)
            WHERE configured_writer_digest_key_ids[digest_key.index] IS NULL
                OR configured_writer_digest_key_ids[digest_key.index]
                    !~ '^[A-Za-z0-9_.:-]{1,64}$'
                OR configured_writer_digest_key_fingerprints[digest_key.index]
                    IS NULL
                OR configured_writer_digest_key_fingerprints[digest_key.index]
                    !~ '^[0-9a-f]{64}$'
        )
        AND (
            SELECT pg_catalog.count(DISTINCT encryption_key.key_id)
            FROM pg_catalog.unnest(configured_encryption_key_ids)
                AS encryption_key(key_id)
        ) = encryption_key_count
        AND (
            SELECT pg_catalog.count(DISTINCT digest_key.key_id)
            FROM pg_catalog.unnest(configured_writer_digest_key_ids)
                AS digest_key(key_id)
        ) = digest_key_count
        AND (
            SELECT pg_catalog.count(DISTINCT digest_key.key_fingerprint)
            FROM pg_catalog.unnest(configured_writer_digest_key_fingerprints)
                AS digest_key(key_fingerprint)
        ) = digest_key_count;

    covered := COALESCE(input_is_valid, FALSE)
        AND NOT EXISTS (
            SELECT 1
            FROM public.authoring_session_generations AS generation
            WHERE NOT generation.encryption_key_id
                = ANY(configured_encryption_key_ids)
                OR (
                    generation.writer_semantic_request_digest IS NOT NULL
                    AND NOT EXISTS (
                        SELECT 1
                        FROM pg_catalog.generate_series(1, digest_key_count)
                            AS configured(index)
                        WHERE configured_writer_digest_key_ids[configured.index]
                                = generation.writer_digest_key_id
                            AND configured_writer_digest_key_fingerprints[
                                configured.index
                            ] = generation.writer_digest_key_fingerprint
                    )
                )
        );
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_authoring_session_writer_load_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_principal_id TEXT,
    expected_session_id TEXT,
    requested_generation BIGINT
)
RETURNS TABLE (
    outcome_code TEXT,
    head_generation BIGINT,
    snapshot_schema_version BIGINT,
    snapshot_ciphertext BYTEA,
    snapshot_nonce BYTEA,
    encryption_key_id TEXT,
    encryption_suite TEXT,
    encryption_suite_version SMALLINT,
    authenticated_metadata_digest TEXT,
    resource_bindings JSONB,
    binding_fingerprint TEXT,
    installation_authority_revision BIGINT,
    authority_payload_digest TEXT,
    writer_request_digest TEXT,
    writer_semantic_request_digest TEXT,
    writer_digest_key_id TEXT,
    writer_digest_key_fingerprint TEXT,
    safe_turn_projection BYTEA,
    safe_turn_projection_digest TEXT,
    stage TEXT,
    candidate_revision BIGINT,
    candidate_hash TEXT,
    harness_contract_revision BIGINT,
    current_authority_revision BIGINT,
    current_authority_payload_digest TEXT,
    current_resource_bindings JSONB,
    current_binding_fingerprint TEXT
)
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
DECLARE
    access_row RECORD;
    session_row RECORD;
    generation_row RECORD;
    selected_generation BIGINT;
BEGIN
    IF expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_principal_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_session_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR requested_generation NOT BETWEEN 0 AND 9223372036854775807
    THEN
        RAISE EXCEPTION 'authoring writer load input is invalid'
            USING ERRCODE = '22023';
    END IF;

    SELECT
        installation.current_authority_revision,
        authority.authority_payload_digest,
        authority.resource_bindings,
        authority.binding_fingerprint
    INTO access_row
    FROM public.product_principals AS principal
    INNER JOIN public.product_tenants AS tenant
        ON tenant.tenant_id = expected_tenant_id
        AND tenant.lifecycle_state = 'active'
    INNER JOIN public.automation_installations AS installation
        ON installation.tenant_id = tenant.tenant_id
        AND installation.installation_id = expected_installation_id
        AND installation.lifecycle_state = 'active'
    INNER JOIN public.automation_installation_authority_versions AS authority
        ON authority.tenant_id = installation.tenant_id
        AND authority.installation_id = installation.installation_id
        AND authority.revision = installation.current_authority_revision
    WHERE principal.principal_id = expected_principal_id
        AND NOT principal.disabled;

    IF NOT FOUND THEN
        RETURN QUERY
        SELECT
            'not_found'::TEXT,
            NULL::BIGINT,
            NULL::BIGINT,
            NULL::BYTEA,
            NULL::BYTEA,
            NULL::TEXT,
            NULL::TEXT,
            NULL::SMALLINT,
            NULL::TEXT,
            NULL::JSONB,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::BYTEA,
            NULL::TEXT,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::BIGINT,
            NULL::TEXT,
            NULL::JSONB,
            NULL::TEXT;
        RETURN;
    END IF;

    SELECT
        authoring_session.tenant_id,
        authoring_session.installation_id,
        authoring_session.owner_principal_id,
        authoring_session.current_generation,
        authoring_session.lifecycle_state
    INTO session_row
    FROM public.authoring_sessions AS authoring_session
    WHERE authoring_session.session_id = expected_session_id;

    IF NOT FOUND THEN
        IF requested_generation = 0 THEN
            RETURN QUERY
            SELECT
                'empty'::TEXT,
                NULL::BIGINT,
                NULL::BIGINT,
                NULL::BYTEA,
                NULL::BYTEA,
                NULL::TEXT,
                NULL::TEXT,
                NULL::SMALLINT,
                NULL::TEXT,
                NULL::JSONB,
                NULL::TEXT,
                NULL::BIGINT,
                NULL::TEXT,
                NULL::TEXT,
                NULL::TEXT,
                NULL::TEXT,
                NULL::TEXT,
                NULL::BYTEA,
                NULL::TEXT,
                NULL::TEXT,
                NULL::BIGINT,
                NULL::TEXT,
                NULL::BIGINT,
                access_row.current_authority_revision::BIGINT,
                access_row.authority_payload_digest::TEXT,
                access_row.resource_bindings::JSONB,
                access_row.binding_fingerprint::TEXT;
        ELSE
            RETURN QUERY
            SELECT
                'not_found'::TEXT,
                NULL::BIGINT,
                NULL::BIGINT,
                NULL::BYTEA,
                NULL::BYTEA,
                NULL::TEXT,
                NULL::TEXT,
                NULL::SMALLINT,
                NULL::TEXT,
                NULL::JSONB,
                NULL::TEXT,
                NULL::BIGINT,
                NULL::TEXT,
                NULL::TEXT,
                NULL::TEXT,
                NULL::TEXT,
                NULL::TEXT,
                NULL::BYTEA,
                NULL::TEXT,
                NULL::TEXT,
                NULL::BIGINT,
                NULL::TEXT,
                NULL::BIGINT,
                access_row.current_authority_revision::BIGINT,
                access_row.authority_payload_digest::TEXT,
                access_row.resource_bindings::JSONB,
                access_row.binding_fingerprint::TEXT;
        END IF;
        RETURN;
    END IF;

    IF session_row.tenant_id IS DISTINCT FROM expected_tenant_id
        OR session_row.installation_id IS DISTINCT FROM expected_installation_id
        OR session_row.owner_principal_id IS DISTINCT FROM expected_principal_id
        OR session_row.lifecycle_state IS DISTINCT FROM 'active'
    THEN
        RETURN QUERY
        SELECT
            'not_found'::TEXT,
            NULL::BIGINT,
            NULL::BIGINT,
            NULL::BYTEA,
            NULL::BYTEA,
            NULL::TEXT,
            NULL::TEXT,
            NULL::SMALLINT,
            NULL::TEXT,
            NULL::JSONB,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::BYTEA,
            NULL::TEXT,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::TEXT,
            NULL::BIGINT,
            access_row.current_authority_revision::BIGINT,
            access_row.authority_payload_digest::TEXT,
            access_row.resource_bindings::JSONB,
            access_row.binding_fingerprint::TEXT;
        RETURN;
    END IF;

    IF requested_generation > session_row.current_generation THEN
        RETURN QUERY
        SELECT
            'generation_conflict'::TEXT,
            session_row.current_generation::BIGINT,
            NULL::BIGINT,
            NULL::BYTEA,
            NULL::BYTEA,
            NULL::TEXT,
            NULL::TEXT,
            NULL::SMALLINT,
            NULL::TEXT,
            NULL::JSONB,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::BYTEA,
            NULL::TEXT,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::TEXT,
            NULL::BIGINT,
            access_row.current_authority_revision::BIGINT,
            access_row.authority_payload_digest::TEXT,
            access_row.resource_bindings::JSONB,
            access_row.binding_fingerprint::TEXT;
        RETURN;
    END IF;

    selected_generation := CASE
        WHEN requested_generation = 0 THEN session_row.current_generation
        ELSE requested_generation
    END;

    SELECT
        generation.generation,
        generation.snapshot_schema_version,
        generation.snapshot_ciphertext,
        generation.snapshot_nonce,
        generation.encryption_key_id,
        generation.encryption_suite,
        generation.encryption_suite_version,
        generation.authenticated_metadata_digest,
        generation.resource_bindings,
        generation.binding_fingerprint,
        generation.installation_authority_revision,
        historical_authority.authority_payload_digest,
        generation.writer_request_digest,
        generation.writer_semantic_request_digest,
        generation.writer_digest_key_id,
        generation.writer_digest_key_fingerprint,
        generation.safe_turn_projection,
        generation.safe_turn_projection_digest,
        generation.stage,
        generation.candidate_revision,
        generation.candidate_hash,
        generation.harness_contract_revision
    INTO generation_row
    FROM public.authoring_session_generations AS generation
    INNER JOIN public.automation_installation_authority_versions
        AS historical_authority
        ON historical_authority.tenant_id = generation.tenant_id
        AND historical_authority.installation_id = generation.installation_id
        AND historical_authority.revision
            = generation.installation_authority_revision
    WHERE generation.tenant_id = expected_tenant_id
        AND generation.installation_id = expected_installation_id
        AND generation.session_id = expected_session_id
        AND generation.generation = selected_generation;

    IF NOT FOUND THEN
        RETURN QUERY
        SELECT
            'not_found'::TEXT,
            NULL::BIGINT,
            NULL::BIGINT,
            NULL::BYTEA,
            NULL::BYTEA,
            NULL::TEXT,
            NULL::TEXT,
            NULL::SMALLINT,
            NULL::TEXT,
            NULL::JSONB,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::TEXT,
            NULL::BYTEA,
            NULL::TEXT,
            NULL::TEXT,
            NULL::BIGINT,
            NULL::TEXT,
            NULL::BIGINT,
            access_row.current_authority_revision::BIGINT,
            access_row.authority_payload_digest::TEXT,
            access_row.resource_bindings::JSONB,
            access_row.binding_fingerprint::TEXT;
        RETURN;
    END IF;

    RETURN QUERY
    SELECT
        'loaded'::TEXT,
        generation_row.generation::BIGINT,
        generation_row.snapshot_schema_version::BIGINT,
        generation_row.snapshot_ciphertext::BYTEA,
        generation_row.snapshot_nonce::BYTEA,
        generation_row.encryption_key_id::TEXT,
        generation_row.encryption_suite::TEXT,
        generation_row.encryption_suite_version::SMALLINT,
        generation_row.authenticated_metadata_digest::TEXT,
        generation_row.resource_bindings::JSONB,
        generation_row.binding_fingerprint::TEXT,
        generation_row.installation_authority_revision::BIGINT,
        generation_row.authority_payload_digest::TEXT,
        generation_row.writer_request_digest::TEXT,
        generation_row.writer_semantic_request_digest::TEXT,
        generation_row.writer_digest_key_id::TEXT,
        generation_row.writer_digest_key_fingerprint::TEXT,
        generation_row.safe_turn_projection::BYTEA,
        generation_row.safe_turn_projection_digest::TEXT,
        generation_row.stage::TEXT,
        generation_row.candidate_revision::BIGINT,
        generation_row.candidate_hash::TEXT,
        generation_row.harness_contract_revision::BIGINT,
        access_row.current_authority_revision::BIGINT,
        access_row.authority_payload_digest::TEXT,
        access_row.resource_bindings::JSONB,
        access_row.binding_fingerprint::TEXT;
END;
$function$;

REVOKE ALL ON FUNCTION
    public.starring_authoring_session_writer_database_identity_v1()
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.starring_authoring_session_writer_check_v1(
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT[],
        TEXT[],
        TEXT[],
        TEXT[]
    )
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.starring_authoring_session_writer_load_v1(
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        BIGINT
    )
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.starring_authoring_session_writer_commit_v1(
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        TEXT[],
        TEXT[],
        TEXT[],
        TEXT[],
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        BIGINT,
        BYTEA,
        BYTEA,
        TEXT,
        TEXT,
        SMALLINT,
        TEXT,
        JSONB,
        TEXT,
        BIGINT,
        TEXT,
        JSONB,
        TEXT,
        BIGINT,
        TEXT,
        BYTEA,
        TEXT,
        BIGINT
    )
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.starring_authoring_session_writer_key_coverage_v1(
        TEXT[],
        TEXT[],
        TEXT[]
    )
FROM PUBLIC;
REVOKE ALL ON FUNCTION
    public.starring_product_authorized_snapshot_read_v2(
        TEXT,
        TEXT,
        BYTEA,
        TEXT,
        TEXT
    )
FROM PUBLIC;

DO $postflight$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    function_identity TEXT;
    function_oid OID;
    unexpected_grantee OID;
    unexpected_grantee_name NAME;
    function_count BIGINT;
    invalid_function_count BIGINT;
    invalid_acl_count BIGINT;
    invalid_column_count BIGINT;
    validated_constraint_count BIGINT;
    partial_metadata_count BIGINT;
    invalid_result_contract_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.authoring_session_generations'
    );
    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner IS NULL
        OR common_owner_name IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
    THEN
        RAISE EXCEPTION 'authoring writer owner changed during migration'
            USING ERRCODE = '55000';
    END IF;

    FOR function_identity IN
        SELECT expected.identity
        FROM (
            VALUES
                (
                    'public.starring_authoring_session_writer_database_identity_v1()'
                ),
                (
                    'public.starring_authoring_session_writer_check_v1(text,text,text,text,bigint,text[],text[],text[],text[])'
                ),
                (
                    'public.starring_authoring_session_writer_load_v1(text,text,text,text,bigint)'
                ),
                (
                    'public.starring_authoring_session_writer_commit_v1(text,text,text,text,bigint,text[],text[],text[],text[],text,text,text,text,bigint,bytea,bytea,text,text,smallint,text,jsonb,text,bigint,text,jsonb,text,bigint,text,bytea,text,bigint)'
                ),
                (
                    'public.starring_authoring_session_writer_key_coverage_v1(text[],text[],text[])'
                ),
                (
                    'public.starring_product_authorized_snapshot_read_v2(text,text,bytea,text,text)'
                )
        ) AS expected(identity)
    LOOP
        function_oid := pg_catalog.to_regprocedure(function_identity);
        IF function_oid IS NULL THEN
            RAISE EXCEPTION 'authoring writer function is unavailable'
                USING ERRCODE = '55000';
        END IF;
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s OWNER TO %I',
            function_identity,
            common_owner_name
        );
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE',
            function_identity
        );
        FOR unexpected_grantee IN
            SELECT DISTINCT privilege.grantee
            FROM pg_catalog.pg_proc AS function_row
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE function_row.oid = function_oid
                AND privilege.grantee <> 0
                AND privilege.grantee <> common_owner
        LOOP
            unexpected_grantee_name := pg_catalog.pg_get_userbyid(
                unexpected_grantee
            );
            IF unexpected_grantee_name IS NULL THEN
                RAISE EXCEPTION 'authoring writer grantee is unavailable'
                    USING ERRCODE = '55000';
            END IF;
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM %I CASCADE',
                function_identity,
                unexpected_grantee_name
            );
        END LOOP;
    END LOOP;

    SELECT pg_catalog.count(*)
    INTO function_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_authoring_session_writer_database_identity_v1',
            'starring_authoring_session_writer_check_v1',
            'starring_authoring_session_writer_load_v1',
            'starring_authoring_session_writer_commit_v1',
            'starring_authoring_session_writer_key_coverage_v1',
            'starring_product_authorized_snapshot_read_v2'
        );
    IF function_count <> 6 THEN
        RAISE EXCEPTION 'authoring writer function set is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_authoring_session_writer_database_identity_v1',
            'starring_authoring_session_writer_check_v1',
            'starring_authoring_session_writer_load_v1',
            'starring_authoring_session_writer_commit_v1',
            'starring_authoring_session_writer_key_coverage_v1',
            'starring_product_authorized_snapshot_read_v2'
        )
        AND (
            function_row.proowner <> common_owner
            OR NOT function_row.prosecdef
            OR function_row.provolatile <> 'v'
            OR function_row.proparallel <> 'u'
            OR function_row.proconfig IS DISTINCT FROM CASE
                WHEN function_row.proname
                    = 'starring_authoring_session_writer_commit_v1'
                THEN ARRAY['search_path=pg_catalog, public']::TEXT[]
                ELSE ARRAY['search_path=pg_catalog']::TEXT[]
            END
            OR function_row.proretset
                IS DISTINCT FROM (
                    function_row.proname
                        <> 'starring_authoring_session_writer_database_identity_v1'
                )
            OR function_row.proisstrict
                IS DISTINCT FROM (
                    function_row.proname
                        <> 'starring_authoring_session_writer_commit_v1'
                )
        );
    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION 'authoring writer function metadata is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_acl_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_authoring_session_writer_database_identity_v1',
            'starring_authoring_session_writer_check_v1',
            'starring_authoring_session_writer_load_v1',
            'starring_authoring_session_writer_commit_v1',
            'starring_authoring_session_writer_key_coverage_v1',
            'starring_product_authorized_snapshot_read_v2'
        )
        AND (
            privilege.grantee <> common_owner
            OR privilege.grantor <> common_owner
            OR privilege.privilege_type <> 'EXECUTE'
            OR privilege.is_grantable
        );
    IF invalid_acl_count <> 0 THEN
        RAISE EXCEPTION 'authoring writer function ACL is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_column_count
    FROM (
        VALUES
            ('writer_semantic_request_digest', 'text'),
            ('writer_digest_key_id', 'text'),
            ('writer_digest_key_fingerprint', 'text'),
            ('safe_turn_projection', 'bytea'),
            ('safe_turn_projection_digest', 'text')
    ) AS expected(column_name, type_name)
    LEFT JOIN pg_catalog.pg_attribute AS attribute
        ON attribute.attrelid = pg_catalog.to_regclass(
            'public.authoring_session_generations'
        )
        AND attribute.attname = expected.column_name
        AND attribute.attnum > 0
        AND NOT attribute.attisdropped
    WHERE attribute.attnum IS NULL
        OR pg_catalog.format_type(attribute.atttypid, attribute.atttypmod)
            IS DISTINCT FROM expected.type_name
        OR attribute.attnotnull
        OR attribute.atthasdef;
    IF invalid_column_count <> 0 THEN
        RAISE EXCEPTION 'authoring writer generation columns are invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO validated_constraint_count
    FROM pg_catalog.pg_constraint AS constraint_row
    WHERE constraint_row.conrelid = pg_catalog.to_regclass(
            'public.authoring_session_generations'
        )
        AND constraint_row.contype = 'c'
        AND constraint_row.convalidated
        AND constraint_row.conname IN (
            'authoring_generations_writer_metadata_presence_valid',
            'authoring_generations_writer_semantic_digest_valid',
            'authoring_generations_writer_key_identity_valid',
            'authoring_generations_safe_projection_valid',
            'authoring_generations_trusted_stage_valid',
            'authoring_generations_trusted_candidate_valid'
        );
    IF validated_constraint_count <> 6 THEN
        RAISE EXCEPTION 'authoring writer generation constraints are invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO partial_metadata_count
    FROM public.authoring_session_generations AS generation
    WHERE (
        generation.writer_semantic_request_digest IS NOT NULL
    )::INTEGER
        + (generation.writer_digest_key_id IS NOT NULL)::INTEGER
        + (generation.writer_digest_key_fingerprint IS NOT NULL)::INTEGER
        + (generation.safe_turn_projection IS NOT NULL)::INTEGER
        + (generation.safe_turn_projection_digest IS NOT NULL)::INTEGER
        NOT IN (0, 5);
    IF partial_metadata_count <> 0 THEN
        RAISE EXCEPTION 'authoring writer metadata is partially populated'
            USING ERRCODE = '55000';
    END IF;

    WITH expected(identity, output_names, output_types) AS (
        VALUES
            (
                'public.starring_authoring_session_writer_check_v1(text,text,text,text,bigint,text[],text[],text[],text[])',
                ARRAY[
                    'outcome_code',
                    'current_generation',
                    'matched_generation',
                    'safe_turn_projection',
                    'safe_turn_projection_digest'
                ]::TEXT[],
                ARRAY['text','bigint','bigint','bytea','text']::TEXT[]
            ),
            (
                'public.starring_authoring_session_writer_load_v1(text,text,text,text,bigint)',
                ARRAY[
                    'outcome_code',
                    'head_generation',
                    'snapshot_schema_version',
                    'snapshot_ciphertext',
                    'snapshot_nonce',
                    'encryption_key_id',
                    'encryption_suite',
                    'encryption_suite_version',
                    'authenticated_metadata_digest',
                    'resource_bindings',
                    'binding_fingerprint',
                    'installation_authority_revision',
                    'authority_payload_digest',
                    'writer_request_digest',
                    'writer_semantic_request_digest',
                    'writer_digest_key_id',
                    'writer_digest_key_fingerprint',
                    'safe_turn_projection',
                    'safe_turn_projection_digest',
                    'stage',
                    'candidate_revision',
                    'candidate_hash',
                    'harness_contract_revision',
                    'current_authority_revision',
                    'current_authority_payload_digest',
                    'current_resource_bindings',
                    'current_binding_fingerprint'
                ]::TEXT[],
                ARRAY[
                    'text','bigint','bigint','bytea','bytea','text','text',
                    'smallint','text','jsonb','text','bigint','text','text',
                    'text','text','text','bytea','text','text','bigint',
                    'text','bigint','bigint','text','jsonb','text'
                ]::TEXT[]
            ),
            (
                'public.starring_authoring_session_writer_commit_v1(text,text,text,text,bigint,text[],text[],text[],text[],text,text,text,text,bigint,bytea,bytea,text,text,smallint,text,jsonb,text,bigint,text,jsonb,text,bigint,text,bytea,text,bigint)',
                ARRAY[
                    'outcome_code',
                    'current_generation',
                    'committed_generation',
                    'safe_turn_projection',
                    'safe_turn_projection_digest'
                ]::TEXT[],
                ARRAY['text','bigint','bigint','bytea','text']::TEXT[]
            ),
            (
                'public.starring_authoring_session_writer_key_coverage_v1(text[],text[],text[])',
                ARRAY['covered']::TEXT[],
                ARRAY['boolean']::TEXT[]
            ),
            (
                'public.starring_product_authorized_snapshot_read_v2(text,text,bytea,text,text)',
                ARRAY[
                    'session_tenant_id',
                    'session_installation_id',
                    'owner_principal_id',
                    'owner_discord_user_id',
                    'owner_disabled',
                    'actor_session_digest',
                    'current_generation',
                    'session_lifecycle_state',
                    'tenant_lifecycle_state',
                    'installation_tenant_id',
                    'discord_application_id',
                    'discord_guild_id',
                    'ruleset_key',
                    'installation_lifecycle_state',
                    'current_authority_revision',
                    'generation',
                    'snapshot_schema_version',
                    'snapshot_ciphertext',
                    'snapshot_nonce',
                    'encryption_key_id',
                    'encryption_suite',
                    'encryption_suite_version',
                    'authenticated_metadata_digest',
                    'generation_resource_bindings',
                    'generation_binding_fingerprint',
                    'installation_authority_revision',
                    'generation_stage',
                    'candidate_revision',
                    'candidate_hash',
                    'harness_contract_revision',
                    'authority_tenant_id',
                    'binding_revision',
                    'authority_resource_bindings',
                    'authority_binding_fingerprint',
                    'policy_revision',
                    'required_approvals',
                    'activation_ttl_seconds',
                    'authority_payload_digest',
                    'database_now',
                    'writer_request_digest',
                    'writer_semantic_request_digest',
                    'writer_digest_key_id',
                    'writer_digest_key_fingerprint',
                    'safe_turn_projection',
                    'safe_turn_projection_digest'
                ]::TEXT[],
                ARRAY[
                    'text','text','text','text','boolean','bytea','bigint',
                    'text','text','text','text','text','text','text','bigint',
                    'bigint','bigint','bytea','bytea','text','text','smallint',
                    'text','jsonb','text','bigint','text','bigint','text',
                    'bigint','text','bigint','jsonb','text','bigint','integer',
                    'bigint','text','timestamp with time zone','text','text',
                    'text','text','bytea','text'
                ]::TEXT[]
            )
    ), observed AS (
        SELECT
            expected.identity,
            expected.output_names,
            expected.output_types,
            (
                SELECT pg_catalog.array_agg(
                    argument_name.argument_name
                    ORDER BY argument_name.ordinality
                )
                FROM pg_catalog.unnest(function_row.proargnames)
                    WITH ORDINALITY AS argument_name(
                        argument_name,
                        ordinality
                    )
                INNER JOIN pg_catalog.unnest(function_row.proargmodes)
                    WITH ORDINALITY AS argument_mode(
                        argument_mode,
                        ordinality
                    )
                    ON argument_mode.ordinality = argument_name.ordinality
                WHERE argument_mode.argument_mode = 't'
            ) AS observed_names,
            (
                SELECT pg_catalog.array_agg(
                    pg_catalog.format_type(argument_type.argument_type, NULL)
                    ORDER BY argument_type.ordinality
                )
                FROM pg_catalog.unnest(function_row.proallargtypes)
                    WITH ORDINALITY AS argument_type(
                        argument_type,
                        ordinality
                    )
                INNER JOIN pg_catalog.unnest(function_row.proargmodes)
                    WITH ORDINALITY AS argument_mode(
                        argument_mode,
                        ordinality
                    )
                    ON argument_mode.ordinality = argument_type.ordinality
                WHERE argument_mode.argument_mode = 't'
            ) AS observed_types
        FROM expected
        LEFT JOIN pg_catalog.pg_proc AS function_row
            ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    )
    SELECT pg_catalog.count(*)
    INTO invalid_result_contract_count
    FROM observed
    WHERE observed_names IS DISTINCT FROM output_names
        OR observed_types IS DISTINCT FROM output_types;
    IF invalid_result_contract_count <> 0 THEN
        RAISE EXCEPTION 'authoring writer result contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    IF pg_catalog.to_regprocedure(
        'public.starring_product_authorized_snapshot_read_v1(text,text,bytea,text,text)'
    ) IS NULL
    THEN
        RAISE EXCEPTION 'authorized snapshot reader v1 changed during migration'
            USING ERRCODE = '55000';
    END IF;
END;
$postflight$;
