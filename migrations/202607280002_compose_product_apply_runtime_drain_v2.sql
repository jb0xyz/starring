SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
);

LOCK TABLE
    public.automation_installations,
    public.runtime_deployments,
    public.runtime_serving_leases,
    public.runtime_product_operations_v2,
    public.runtime_drain_intents_v2,
    public.runtime_slot_writer_fences_v2,
    public.runtime_writer_fence
IN ACCESS EXCLUSIVE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    executor_grantee OID;
    executor_grant_count BIGINT;
    invalid_function_count BIGINT;
    invalid_relation_count BIGINT;
    invalid_relation_acl_count BIGINT;
    invalid_private_schema_acl_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    SELECT
        pg_catalog.min(privilege.grantee::BIGINT)::OID,
        pg_catalog.count(*)
    INTO executor_grantee, executor_grant_count
    FROM pg_catalog.pg_proc AS function_row
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'
        )
        AND privilege.grantee <> common_owner;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            (
                'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)',
                '35dff4eac9780b1cea497459ac513c54e5151fc752c290228951fadd6a4c2c2d'::TEXT,
                TRUE,
                'v'::"char",
                'u'::"char",
                ARRAY['search_path=pg_catalog']::TEXT[]
            ),
            (
                'starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text,bytea,text,bytea,text)',
                '534dcc1f973d1b37e9f72e28b01ad6541f2ff4293b1cbc5c3b5893764b7fed6e'::TEXT,
                FALSE,
                'v'::"char",
                'u'::"char",
                ARRAY['search_path=pg_catalog, starring_runtime_private_v2']::TEXT[]
            ),
            (
                'starring_runtime_private_v2.starring_runtime_product_mutation_bytes_v2(text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text)',
                'a7ee4e19bc885037d2ac61203d02d48d30722ce5cb19df9779cd26c40eaa656c'::TEXT,
                FALSE,
                'i'::"char",
                's'::"char",
                ARRAY['search_path=pg_catalog']::TEXT[]
            ),
            (
                'starring_runtime_private_v2.starring_runtime_product_mutation_digest_v2(bytea)',
                'fda0be096bc6dc9d7ab788bd1b303a1bc7aec8fb3f398a9995f27f1681573a23'::TEXT,
                FALSE,
                'i'::"char",
                's'::"char",
                ARRAY['search_path=pg_catalog']::TEXT[]
            ),
            (
                'starring_runtime_private_v2.starring_runtime_drain_intent_bytes_v2(text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text)',
                'bc75651b0d2eb195462390697c880809fd5501e2794e0902aa022a224e928e0d'::TEXT,
                FALSE,
                'i'::"char",
                's'::"char",
                ARRAY['search_path=pg_catalog']::TEXT[]
            ),
            (
                'starring_runtime_private_v2.starring_runtime_drain_intent_digest_v2(bytea)',
                '88e764bec97747486439a1baff108769c2e7a86d324a1a3f92bf91914353f507'::TEXT,
                FALSE,
                'i'::"char",
                's'::"char",
                ARRAY['search_path=pg_catalog']::TEXT[]
            ),
            (
                'starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(text,text)',
                '5952cd4955c7dc7f543618249edbc7a2cbc4bcb494df1f6dacd77b69d88a707d'::TEXT,
                FALSE,
                'v'::"char",
                'u'::"char",
                ARRAY['search_path=pg_catalog']::TEXT[]
            ),
            (
                'starring_runtime_private_v2.starring_runtime_slot_writer_fence_mark_drain_v2(text,text,bigint,text,text,text,text,text,bigint)',
                '77ed38195d939f06a824d3bd7d1fac89643955b2027d0a366d1714eb55e29c99'::TEXT,
                FALSE,
                'v'::"char",
                'u'::"char",
                ARRAY['search_path=pg_catalog']::TEXT[]
            )
    ) AS expected(
        identity,
        definition_digest,
        security_definer,
        volatility,
        parallel_safety,
        configuration
    )
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> expected.volatility
        OR NOT function_row.proisstrict
        OR function_row.proparallel <> expected.parallel_safety
        OR function_row.prosecdef
            IS DISTINCT FROM expected.security_definer
        OR function_row.proconfig
            IS DISTINCT FROM expected.configuration
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM 'plpgsql'
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(function_row.oid),
                'UTF8'
            )),
            'hex'
        ) IS DISTINCT FROM expected.definition_digest;

    SELECT pg_catalog.count(*)
    INTO invalid_relation_count
    FROM (
        VALUES
            ('public.automation_installations'),
            ('public.runtime_deployments'),
            ('public.runtime_serving_leases'),
            ('public.runtime_product_operations_v2'),
            ('public.runtime_drain_intents_v2'),
            ('public.runtime_slot_writer_fences_v2'),
            ('public.runtime_writer_fence')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = pg_catalog.to_regclass(expected.identity)
    WHERE relation.oid IS NULL
        OR relation.relkind <> 'r'
        OR relation.relowner <> common_owner
        OR relation.relrowsecurity
        OR relation.relforcerowsecurity;

    SELECT pg_catalog.count(*)
    INTO invalid_relation_acl_count
    FROM (
        VALUES
            ('public.automation_installations'),
            ('public.runtime_deployments'),
            ('public.runtime_serving_leases'),
            ('public.runtime_product_operations_v2'),
            ('public.runtime_drain_intents_v2'),
            ('public.runtime_slot_writer_fences_v2'),
            ('public.runtime_writer_fence')
    ) AS expected(identity)
    INNER JOIN pg_catalog.pg_class AS relation
        ON relation.oid = pg_catalog.to_regclass(expected.identity)
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        relation.relacl,
        pg_catalog.acldefault('r', relation.relowner)
    )) AS privilege
    WHERE privilege.grantee <> common_owner;

    SELECT pg_catalog.count(*)
    INTO invalid_private_schema_acl_count
    FROM pg_catalog.pg_namespace AS namespace
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        namespace.nspacl,
        pg_catalog.acldefault('n', namespace.nspowner)
    )) AS privilege
    WHERE namespace.oid = pg_catalog.to_regnamespace(
            'starring_runtime_private_v2'
        )
        AND privilege.grantee <> common_owner;

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR executor_grant_count > 1
        OR (
            executor_grant_count = 1
            AND (
                executor_grantee = 0
                OR NOT EXISTS (
                    SELECT 1
                    FROM pg_catalog.pg_proc AS function_row
                    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                        function_row.proacl,
                        pg_catalog.acldefault('f', function_row.proowner)
                    )) AS privilege
                    WHERE function_row.oid = pg_catalog.to_regprocedure(
                            'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'
                        )
                        AND privilege.grantee = executor_grantee
                        AND privilege.grantor = common_owner
                        AND privilege.privilege_type = 'EXECUTE'
                        AND NOT privilege.is_grantable
                )
            )
        )
        OR invalid_function_count <> 0
        OR invalid_relation_count <> 0
        OR invalid_relation_acl_count <> 0
        OR invalid_private_schema_acl_count <> 0
        OR pg_catalog.to_regprocedure(
            'public.starring_product_apply_begin_runtime_drain_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text,text)'
        ) IS NOT NULL
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'PA001',
            MESSAGE =
                'product_apply_begin_runtime_drain_v2_preflight_drift';
    END IF;
END;
$preflight$;

CREATE FUNCTION public.starring_product_apply_begin_runtime_drain_v2(
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
    proposed_product_operation_id TEXT,
    proposed_drain_intent_id TEXT
)
RETURNS TABLE(
    outcome TEXT,
    locked_snapshot JSONB,
    observed_at TIMESTAMPTZ,
    product_tenant_id TEXT,
    product_installation_id TEXT,
    product_deployment_id TEXT,
    product_expected_revision BIGINT,
    product_operation_id TEXT,
    product_expected_target JSONB,
    product_mutation_request_bytes BYTEA,
    product_mutation_digest TEXT,
    drain_tenant_id TEXT,
    drain_installation_id TEXT,
    drain_deployment_id TEXT,
    drain_slot_guild_id TEXT,
    drain_slot_ruleset_key TEXT,
    drain_expected_revision BIGINT,
    drain_intent_id TEXT,
    drain_intent_request_bytes BYTEA,
    drain_intent_digest TEXT,
    intent_revision BIGINT,
    intent_state TEXT,
    canonical_state_bytes BYTEA,
    canonical_state_digest TEXT,
    writer_epoch_before BIGINT,
    writer_epoch_after BIGINT,
    pending_drain_intent_id TEXT,
    pending_product_operation_id TEXT,
    pending_tenant_id TEXT,
    pending_installation_id TEXT,
    pending_deployment_id TEXT,
    pending_expected_revision BIGINT,
    pending_marked_at TIMESTAMPTZ
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
    apply_lock_row RECORD;
    source_row public.runtime_deployments%ROWTYPE;
    first_apply_row RECORD;
    canonical_drain_row public.runtime_drain_intents_v2%ROWTYPE;
    slot_fence_before_row RECORD;
    slot_fence_after_row RECORD;
    slot_ruleset_key TEXT;
    natural_product_count BIGINT;
    natural_drain_count BIGINT;
    persisted_product_operation_id TEXT;
    persisted_drain_intent_id TEXT;
    persisted_drain_product_operation_id TEXT;
    selected_product_operation_id TEXT;
    selected_drain_intent_id TEXT;
    product_request_bytes BYTEA;
    product_request_digest TEXT;
    drain_request_bytes BYTEA;
    drain_request_digest TEXT;
    expected_target JSONB;
    observation_mode BOOLEAN;
BEGIN
    IF pg_catalog.current_setting('transaction_isolation')
            <> 'serializable'
        OR pg_catalog.current_setting('transaction_read_only') <> 'off'
    THEN
        outcome := 'invalid_input';
        RETURN NEXT;
        RETURN;
    END IF;

    BEGIN
        SELECT apply_lock.*
        INTO STRICT apply_lock_row
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
        ) AS apply_lock;
    EXCEPTION
        WHEN no_data_found OR too_many_rows THEN
            outcome := 'persistence_corrupt';
            RETURN NEXT;
            RETURN;
    END;

    IF apply_lock_row.outcome IS DISTINCT FROM 'runtime_drain_required'
    THEN
        outcome := CASE
            WHEN apply_lock_row.outcome ~ '^[a-z][a-z0-9_]{0,63}$'
                THEN apply_lock_row.outcome
            ELSE 'persistence_corrupt'
        END;
        RETURN NEXT;
        RETURN;
    END IF;

    IF apply_lock_row.exact_replay IS DISTINCT FROM FALSE
        OR apply_lock_row.requires_commit IS DISTINCT FROM FALSE
        OR apply_lock_row.resulting_revision IS NOT NULL
        OR apply_lock_row.resulting_state IS NOT NULL
        OR apply_lock_row.deployment_id IS NOT NULL
        OR apply_lock_row.desired_target_digest IS NOT NULL
        OR apply_lock_row.locked_projection IS NOT NULL
    THEN
        outcome := 'persistence_corrupt';
        RETURN NEXT;
        RETURN;
    END IF;

    observation_mode :=
        proposed_product_operation_id = ''
        AND proposed_drain_intent_id = '';

    IF NOT observation_mode
        AND (
            proposed_product_operation_id !~ '^[0-9a-f]{32}$'
            OR proposed_drain_intent_id !~ '^[0-9a-f]{32}$'
            OR proposed_product_operation_id = proposed_drain_intent_id
        )
    THEN
        outcome := 'invalid_input';
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT installation.ruleset_key
    INTO slot_ruleset_key
    FROM public.automation_installations AS installation
    WHERE installation.tenant_id = expected_tenant_id
        AND installation.installation_id = expected_installation_id
        AND installation.discord_guild_id = expected_guild_id
    FOR SHARE;

    IF NOT FOUND THEN
        outcome := 'deployment_mismatch';
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT deployment.*
    INTO source_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.tenant_id = expected_tenant_id
        AND deployment.installation_id = expected_installation_id
        AND deployment.guild_id = expected_guild_id
        AND deployment.ruleset_key = slot_ruleset_key
        AND deployment.phase NOT IN ('superseded', 'cancelled')
    ORDER BY
        deployment.runtime_generation DESC,
        deployment.deployment_id DESC
    LIMIT 1
    FOR UPDATE;

    IF NOT FOUND
        OR source_row.tenant_id IS DISTINCT FROM expected_tenant_id
        OR source_row.installation_id
            IS DISTINCT FROM expected_installation_id
        OR source_row.phase NOT IN ('awaiting_gateway_ready', 'live')
    THEN
        outcome := 'deployment_mismatch';
        RETURN NEXT;
        RETURN;
    END IF;

    IF source_row.deployment_id = new_deployment_id THEN
        outcome := 'identifier_conflict';
        RETURN NEXT;
        RETURN;
    END IF;

    BEGIN
        SELECT fence.*
        INTO STRICT slot_fence_before_row
        FROM starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(
            source_row.guild_id,
            source_row.ruleset_key
        ) AS fence;
    EXCEPTION
        WHEN no_data_found OR too_many_rows THEN
            outcome := 'persistence_corrupt';
            RETURN NEXT;
            RETURN;
    END;

    expected_target := pg_catalog.jsonb_build_object(
        'guild_id',
        source_row.guild_id,
        'ruleset_key',
        source_row.ruleset_key,
        'version',
        source_row.target_version,
        'content_hash',
        source_row.target_content_hash,
        'binding_revision',
        source_row.binding_revision,
        'binding_fingerprint',
        source_row.binding_fingerprint
    );

    PERFORM product.product_operation_id
    FROM public.runtime_product_operations_v2 AS product
    WHERE product.tenant_id = source_row.tenant_id
        AND product.installation_id = source_row.installation_id
        AND product.deployment_id = source_row.deployment_id
        AND product.expected_revision = source_row.revision
    ORDER BY product.product_operation_id
    FOR UPDATE;

    SELECT
        pg_catalog.count(*),
        pg_catalog.min(product.product_operation_id)
    INTO natural_product_count, persisted_product_operation_id
    FROM public.runtime_product_operations_v2 AS product
    WHERE product.tenant_id = source_row.tenant_id
        AND product.installation_id = source_row.installation_id
        AND product.deployment_id = source_row.deployment_id
        AND product.expected_revision = source_row.revision;

    PERFORM drain.drain_intent_id
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE drain.tenant_id = source_row.tenant_id
        AND drain.installation_id = source_row.installation_id
        AND drain.deployment_id = source_row.deployment_id
        AND drain.slot_guild_id = source_row.guild_id
        AND drain.slot_ruleset_key = source_row.ruleset_key
        AND drain.expected_revision = source_row.revision
    ORDER BY drain.drain_intent_id
    FOR UPDATE;

    SELECT
        pg_catalog.count(*),
        pg_catalog.min(drain.drain_intent_id),
        pg_catalog.min(drain.product_operation_id)
    INTO
        natural_drain_count,
        persisted_drain_intent_id,
        persisted_drain_product_operation_id
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE drain.tenant_id = source_row.tenant_id
        AND drain.installation_id = source_row.installation_id
        AND drain.deployment_id = source_row.deployment_id
        AND drain.slot_guild_id = source_row.guild_id
        AND drain.slot_ruleset_key = source_row.ruleset_key
        AND drain.expected_revision = source_row.revision;

    IF natural_product_count = 0 AND natural_drain_count = 0 THEN
        IF slot_fence_before_row.writer_epoch IS NULL
            OR slot_fence_before_row.writer_epoch
                NOT BETWEEN 1 AND 9223372036854775807
            OR slot_fence_before_row.observed_at IS NULL
            OR NOT pg_catalog.isfinite(slot_fence_before_row.observed_at)
            OR slot_fence_before_row.pending_drain_intent_id IS NOT NULL
            OR slot_fence_before_row.pending_product_operation_id IS NOT NULL
            OR slot_fence_before_row.pending_tenant_id IS NOT NULL
            OR slot_fence_before_row.pending_installation_id IS NOT NULL
            OR slot_fence_before_row.pending_deployment_id IS NOT NULL
            OR slot_fence_before_row.pending_expected_revision IS NOT NULL
            OR slot_fence_before_row.pending_marked_at IS NOT NULL
        THEN
            outcome := 'persistence_corrupt';
            RETURN NEXT;
            RETURN;
        END IF;

        IF observation_mode THEN
            RETURN QUERY SELECT
                'absent',
                source_row.snapshot,
                slot_fence_before_row.observed_at,
                source_row.tenant_id,
                source_row.installation_id,
                source_row.deployment_id,
                source_row.revision,
                NULL::TEXT,
                expected_target,
                NULL::BYTEA,
                NULL::TEXT,
                source_row.tenant_id,
                source_row.installation_id,
                source_row.deployment_id,
                source_row.guild_id,
                source_row.ruleset_key,
                source_row.revision,
                NULL::TEXT,
                NULL::BYTEA,
                NULL::TEXT,
                NULL::BIGINT,
                NULL::TEXT,
                NULL::BYTEA,
                NULL::TEXT,
                slot_fence_before_row.writer_epoch,
                slot_fence_before_row.writer_epoch,
                NULL::TEXT,
                NULL::TEXT,
                NULL::TEXT,
                NULL::TEXT,
                NULL::TEXT,
                NULL::BIGINT,
                NULL::TIMESTAMPTZ;
            RETURN;
        END IF;

        selected_product_operation_id := proposed_product_operation_id;
        selected_drain_intent_id := proposed_drain_intent_id;
    ELSIF natural_product_count = 1
        AND natural_drain_count = 1
        AND persisted_drain_product_operation_id
            IS NOT DISTINCT FROM persisted_product_operation_id
        AND persisted_product_operation_id ~ '^[0-9a-f]{32}$'
        AND persisted_drain_intent_id ~ '^[0-9a-f]{32}$'
        AND persisted_product_operation_id <> persisted_drain_intent_id
    THEN
        selected_product_operation_id := persisted_product_operation_id;
        selected_drain_intent_id := persisted_drain_intent_id;
    ELSE
        outcome := 'persistence_corrupt';
        RETURN NEXT;
        RETURN;
    END IF;

    product_request_bytes :=
        starring_runtime_private_v2.starring_runtime_product_mutation_bytes_v2(
            selected_product_operation_id,
            source_row.tenant_id,
            source_row.installation_id,
            source_row.deployment_id,
            source_row.revision,
            source_row.guild_id,
            source_row.ruleset_key,
            source_row.guild_id,
            source_row.ruleset_key,
            source_row.target_version,
            source_row.target_content_hash,
            source_row.binding_revision,
            source_row.binding_fingerprint,
            'apply',
            semantic_request_digest
        );
    product_request_digest :=
        starring_runtime_private_v2.starring_runtime_product_mutation_digest_v2(
            product_request_bytes
        );
    drain_request_bytes :=
        starring_runtime_private_v2.starring_runtime_drain_intent_bytes_v2(
            selected_drain_intent_id,
            selected_product_operation_id,
            source_row.tenant_id,
            source_row.installation_id,
            source_row.deployment_id,
            source_row.revision,
            source_row.guild_id,
            source_row.ruleset_key,
            source_row.guild_id,
            source_row.ruleset_key,
            source_row.target_version,
            source_row.target_content_hash,
            source_row.binding_revision,
            source_row.binding_fingerprint,
            'apply',
            semantic_request_digest
        );
    drain_request_digest :=
        starring_runtime_private_v2.starring_runtime_drain_intent_digest_v2(
            drain_request_bytes
        );
    BEGIN
        SELECT first_apply.*
        INTO STRICT first_apply_row
        FROM starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(
            selected_product_operation_id,
            selected_drain_intent_id,
            source_row.tenant_id,
            source_row.installation_id,
            source_row.deployment_id,
            source_row.revision,
            source_row.guild_id,
            source_row.ruleset_key,
            source_row.guild_id,
            source_row.ruleset_key,
            source_row.target_version,
            source_row.target_content_hash,
            source_row.binding_revision,
            source_row.binding_fingerprint,
            'apply',
            semantic_request_digest,
            product_request_bytes,
            product_request_digest,
            drain_request_bytes,
            drain_request_digest
        ) AS first_apply;
    EXCEPTION
        WHEN no_data_found OR too_many_rows THEN
            outcome := 'persistence_corrupt';
            RETURN NEXT;
            RETURN;
    END;

    IF first_apply_row.outcome_name
            NOT IN ('inserted', 'replayed')
    THEN
        outcome := CASE
            WHEN first_apply_row.outcome_name IN (
                'persistence_corrupt',
                'diverged',
                'identifier_conflict',
                'slot_conflict'
            )
            THEN first_apply_row.outcome_name
            ELSE 'persistence_corrupt'
        END;
        RETURN NEXT;
        RETURN;
    END IF;

    BEGIN
        SELECT fence.*
        INTO STRICT slot_fence_after_row
        FROM starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(
            source_row.guild_id,
            source_row.ruleset_key
        ) AS fence;
    EXCEPTION
        WHEN no_data_found OR too_many_rows THEN
            outcome := 'persistence_corrupt';
            RETURN NEXT;
            RETURN;
    END;

    SELECT drain.*
    INTO canonical_drain_row
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE drain.drain_intent_id = selected_drain_intent_id
        AND drain.product_operation_id = selected_product_operation_id
        AND drain.tenant_id = source_row.tenant_id
        AND drain.installation_id = source_row.installation_id
        AND drain.deployment_id = source_row.deployment_id
        AND drain.slot_guild_id = source_row.guild_id
        AND drain.slot_ruleset_key = source_row.ruleset_key
        AND drain.expected_revision = source_row.revision
    FOR UPDATE;

    IF NOT FOUND THEN
        outcome := 'persistence_corrupt';
        RETURN NEXT;
        RETURN;
    END IF;

    IF first_apply_row.locked_snapshot
            IS DISTINCT FROM source_row.snapshot
        OR first_apply_row.observed_at IS NULL
        OR NOT pg_catalog.isfinite(first_apply_row.observed_at)
        OR first_apply_row.product_tenant_id
            IS DISTINCT FROM source_row.tenant_id
        OR first_apply_row.product_installation_id
            IS DISTINCT FROM source_row.installation_id
        OR first_apply_row.product_deployment_id
            IS DISTINCT FROM source_row.deployment_id
        OR first_apply_row.product_expected_revision
            IS DISTINCT FROM source_row.revision
        OR first_apply_row.product_operation_id
            IS DISTINCT FROM selected_product_operation_id
        OR first_apply_row.product_expected_target
            IS DISTINCT FROM expected_target
        OR first_apply_row.product_mutation_request_bytes
            IS DISTINCT FROM product_request_bytes
        OR first_apply_row.product_mutation_digest
            IS DISTINCT FROM product_request_digest
        OR first_apply_row.drain_tenant_id
            IS DISTINCT FROM source_row.tenant_id
        OR first_apply_row.drain_installation_id
            IS DISTINCT FROM source_row.installation_id
        OR first_apply_row.drain_deployment_id
            IS DISTINCT FROM source_row.deployment_id
        OR first_apply_row.drain_slot_guild_id
            IS DISTINCT FROM source_row.guild_id
        OR first_apply_row.drain_slot_ruleset_key
            IS DISTINCT FROM source_row.ruleset_key
        OR first_apply_row.drain_expected_revision
            IS DISTINCT FROM source_row.revision
        OR first_apply_row.drain_intent_id
            IS DISTINCT FROM selected_drain_intent_id
        OR first_apply_row.drain_intent_request_bytes
            IS DISTINCT FROM drain_request_bytes
        OR first_apply_row.drain_intent_digest
            IS DISTINCT FROM drain_request_digest
        OR first_apply_row.intent_revision IS NULL
        OR first_apply_row.intent_revision NOT BETWEEN 1 AND 9223372036854775807
        OR first_apply_row.intent_state IS NULL
        OR first_apply_row.intent_state
            NOT IN ('pending', 'route_absent_acknowledged')
        OR canonical_drain_row.intent_revision
            IS DISTINCT FROM first_apply_row.intent_revision
        OR canonical_drain_row.intent_state
            IS DISTINCT FROM first_apply_row.intent_state
        OR canonical_drain_row.canonical_state_bytes IS NULL
        OR canonical_drain_row.canonical_state_digest
            !~ '^[0-9a-f]{64}$'
        OR pg_catalog.encode(
            pg_catalog.sha256(canonical_drain_row.canonical_state_bytes),
            'hex'
        ) IS DISTINCT FROM canonical_drain_row.canonical_state_digest
        OR slot_fence_after_row.writer_epoch IS NULL
        OR slot_fence_after_row.writer_epoch
            NOT BETWEEN 2 AND 9223372036854775807
        OR slot_fence_after_row.pending_drain_intent_id
            IS DISTINCT FROM selected_drain_intent_id
        OR slot_fence_after_row.pending_product_operation_id
            IS DISTINCT FROM selected_product_operation_id
        OR slot_fence_after_row.pending_tenant_id
            IS DISTINCT FROM source_row.tenant_id
        OR slot_fence_after_row.pending_installation_id
            IS DISTINCT FROM source_row.installation_id
        OR slot_fence_after_row.pending_deployment_id
            IS DISTINCT FROM source_row.deployment_id
        OR slot_fence_after_row.pending_expected_revision
            IS DISTINCT FROM source_row.revision
        OR slot_fence_after_row.pending_marked_at IS NULL
        OR NOT pg_catalog.isfinite(
            slot_fence_after_row.pending_marked_at
        )
        OR slot_fence_after_row.observed_at IS NULL
        OR NOT pg_catalog.isfinite(slot_fence_after_row.observed_at)
        OR (
            first_apply_row.outcome_name = 'inserted'
            AND (
                natural_product_count <> 0
                OR natural_drain_count <> 0
                OR first_apply_row.intent_revision IS DISTINCT FROM 1
                OR first_apply_row.intent_state IS DISTINCT FROM 'pending'
                OR slot_fence_before_row.pending_drain_intent_id IS NOT NULL
                OR slot_fence_before_row.pending_product_operation_id IS NOT NULL
                OR slot_fence_before_row.pending_tenant_id IS NOT NULL
                OR slot_fence_before_row.pending_installation_id IS NOT NULL
                OR slot_fence_before_row.pending_deployment_id IS NOT NULL
                OR slot_fence_before_row.pending_expected_revision IS NOT NULL
                OR slot_fence_before_row.pending_marked_at IS NOT NULL
                OR slot_fence_before_row.writer_epoch
                    = 9223372036854775807
                OR slot_fence_after_row.writer_epoch
                    IS DISTINCT FROM slot_fence_before_row.writer_epoch + 1
            )
        )
        OR (
            first_apply_row.outcome_name = 'replayed'
            AND (
                natural_product_count <> 1
                OR natural_drain_count <> 1
                OR slot_fence_before_row.writer_epoch
                    IS DISTINCT FROM slot_fence_after_row.writer_epoch
                OR slot_fence_before_row.pending_drain_intent_id
                    IS DISTINCT FROM selected_drain_intent_id
                OR slot_fence_before_row.pending_product_operation_id
                    IS DISTINCT FROM selected_product_operation_id
                OR slot_fence_before_row.pending_tenant_id
                    IS DISTINCT FROM source_row.tenant_id
                OR slot_fence_before_row.pending_installation_id
                    IS DISTINCT FROM source_row.installation_id
                OR slot_fence_before_row.pending_deployment_id
                    IS DISTINCT FROM source_row.deployment_id
                OR slot_fence_before_row.pending_expected_revision
                    IS DISTINCT FROM source_row.revision
                OR slot_fence_before_row.pending_marked_at
                    IS DISTINCT FROM slot_fence_after_row.pending_marked_at
            )
        )
    THEN
        outcome := 'persistence_corrupt';
        RETURN NEXT;
        RETURN;
    END IF;

    RETURN QUERY SELECT
        first_apply_row.outcome_name,
        first_apply_row.locked_snapshot,
        first_apply_row.observed_at,
        first_apply_row.product_tenant_id,
        first_apply_row.product_installation_id,
        first_apply_row.product_deployment_id,
        first_apply_row.product_expected_revision,
        first_apply_row.product_operation_id,
        first_apply_row.product_expected_target,
        first_apply_row.product_mutation_request_bytes,
        first_apply_row.product_mutation_digest,
        first_apply_row.drain_tenant_id,
        first_apply_row.drain_installation_id,
        first_apply_row.drain_deployment_id,
        first_apply_row.drain_slot_guild_id,
        first_apply_row.drain_slot_ruleset_key,
        first_apply_row.drain_expected_revision,
        first_apply_row.drain_intent_id,
        first_apply_row.drain_intent_request_bytes,
        first_apply_row.drain_intent_digest,
        first_apply_row.intent_revision,
        first_apply_row.intent_state,
        canonical_drain_row.canonical_state_bytes,
        canonical_drain_row.canonical_state_digest,
        slot_fence_before_row.writer_epoch,
        slot_fence_after_row.writer_epoch,
        slot_fence_after_row.pending_drain_intent_id,
        slot_fence_after_row.pending_product_operation_id,
        slot_fence_after_row.pending_tenant_id,
        slot_fence_after_row.pending_installation_id,
        slot_fence_after_row.pending_deployment_id,
        slot_fence_after_row.pending_expected_revision,
        slot_fence_after_row.pending_marked_at;
END;
$function$;

REVOKE ALL PRIVILEGES ON FUNCTION
public.starring_product_apply_begin_runtime_drain_v2(
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
    TEXT,
    TEXT
) FROM PUBLIC;

DO $grant$
DECLARE
    common_owner OID;
    executor_role NAME;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    SELECT role.rolname
    INTO executor_role
    FROM pg_catalog.pg_proc AS function_row
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    INNER JOIN pg_catalog.pg_roles AS role
        ON role.oid = privilege.grantee
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'
        )
        AND privilege.grantee <> common_owner;

    IF executor_role IS NOT NULL THEN
        EXECUTE pg_catalog.format(
            'GRANT EXECUTE ON FUNCTION public.starring_product_apply_begin_runtime_drain_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text,text) TO %I',
            executor_role
        );
    END IF;
END;
$grant$;

DO $postflight$
DECLARE
    common_owner OID;
    executor_grantee OID;
    executor_grant_count BIGINT;
    invalid_dependency_count BIGINT;
    invalid_function_acl_count BIGINT;
    invalid_relation_acl_count BIGINT;
    invalid_private_schema_acl_count BIGINT;
    function_definition TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    SELECT
        pg_catalog.min(privilege.grantee::BIGINT)::OID,
        pg_catalog.count(*)
    INTO executor_grantee, executor_grant_count
    FROM pg_catalog.pg_proc AS function_row
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'
        )
        AND privilege.grantee <> common_owner;

    SELECT pg_catalog.count(*)
    INTO invalid_function_acl_count
    FROM pg_catalog.pg_proc AS function_row
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_product_apply_begin_runtime_drain_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text,text)'
        )
        AND (
            privilege.grantor <> common_owner
            OR privilege.privilege_type <> 'EXECUTE'
            OR privilege.is_grantable
            OR (
                privilege.grantee <> common_owner
                AND (
                    executor_grant_count <> 1
                    OR privilege.grantee IS DISTINCT FROM executor_grantee
                )
            )
        );

    SELECT pg_catalog.count(*)
    INTO invalid_relation_acl_count
    FROM (
        VALUES
            ('public.automation_installations'),
            ('public.runtime_deployments'),
            ('public.runtime_serving_leases'),
            ('public.runtime_product_operations_v2'),
            ('public.runtime_drain_intents_v2'),
            ('public.runtime_slot_writer_fences_v2'),
            ('public.runtime_writer_fence')
    ) AS expected(identity)
    INNER JOIN pg_catalog.pg_class AS relation
        ON relation.oid = pg_catalog.to_regclass(expected.identity)
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        relation.relacl,
        pg_catalog.acldefault('r', relation.relowner)
    )) AS privilege
    WHERE privilege.grantee <> common_owner;

    SELECT pg_catalog.count(*)
    INTO invalid_private_schema_acl_count
    FROM pg_catalog.pg_namespace AS namespace
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        namespace.nspacl,
        pg_catalog.acldefault('n', namespace.nspowner)
    )) AS privilege
    WHERE namespace.oid = pg_catalog.to_regnamespace(
            'starring_runtime_private_v2'
        )
        AND privilege.grantee <> common_owner;

    SELECT pg_catalog.count(*)
    INTO invalid_dependency_count
    FROM (
        VALUES (
            'starring_runtime_private_v2.starring_runtime_slot_writer_fence_mark_drain_v2(text,text,bigint,text,text,text,text,text,bigint)',
            '77ed38195d939f06a824d3bd7d1fac89643955b2027d0a366d1714eb55e29c99'::TEXT
        )
    ) AS expected(identity, definition_digest)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR NOT function_row.proisstrict
        OR function_row.proparallel <> 'u'
        OR function_row.prosecdef
        OR function_row.proretset
        OR function_row.prorows <> 0::REAL
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR language_row.lanname IS DISTINCT FROM 'plpgsql'
        OR pg_catalog.pg_get_function_result(function_row.oid)
            IS DISTINCT FROM 'bigint'
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(function_row.oid),
                'UTF8'
            )),
            'hex'
        ) IS DISTINCT FROM expected.definition_digest
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
                OR privilege.grantor <> common_owner
                OR privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
        );

    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO function_definition
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_product_apply_begin_runtime_drain_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text,text)'
        )
        AND function_row.proowner = common_owner
        AND function_row.prokind = 'f'
        AND function_row.provolatile = 'v'
        AND function_row.proisstrict
        AND function_row.proparallel = 'u'
        AND function_row.prosecdef
        AND function_row.proretset
        AND function_row.prorows = 1::REAL
        AND function_row.proconfig
            IS NOT DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        AND NOT function_row.proleakproof
        AND function_row.pronargdefaults = 0
        AND function_row.provariadic = 0
        AND language_row.lanname = 'plpgsql'
        AND pg_catalog.pg_get_function_arguments(function_row.oid)
            IS NOT DISTINCT FROM
                'expected_tenant_id text, expected_installation_id text, expected_promotion_id text, expected_product_revision bigint, expected_payload_digest text, expected_principal_id text, expected_product_session_digest bytea, session_subject_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, expected_authority_revision bigint, expected_authority_payload_digest text, expected_authority_observation_digest text, expected_authority_observed_at timestamp with time zone, expected_authority_expires_at timestamp with time zone, expected_effective_permission_bits text, expected_guild_owner boolean, product_request_id text, active_idempotency_key_digest text, idempotency_key_digest_candidates text[], idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[], idempotency_digest_key_id text, semantic_request_digest text, new_receipt_id text, new_audit_event_id text, new_apply_attempt_id text, new_deployment_id text, proposed_product_operation_id text, proposed_drain_intent_id text'::TEXT
        AND pg_catalog.pg_get_function_result(function_row.oid)
            IS NOT DISTINCT FROM
                'TABLE(outcome text, locked_snapshot jsonb, observed_at timestamp with time zone, product_tenant_id text, product_installation_id text, product_deployment_id text, product_expected_revision bigint, product_operation_id text, product_expected_target jsonb, product_mutation_request_bytes bytea, product_mutation_digest text, drain_tenant_id text, drain_installation_id text, drain_deployment_id text, drain_slot_guild_id text, drain_slot_ruleset_key text, drain_expected_revision bigint, drain_intent_id text, drain_intent_request_bytes bytea, drain_intent_digest text, intent_revision bigint, intent_state text, canonical_state_bytes bytea, canonical_state_digest text, writer_epoch_before bigint, writer_epoch_after bigint, pending_drain_intent_id text, pending_product_operation_id text, pending_tenant_id text, pending_installation_id text, pending_deployment_id text, pending_expected_revision bigint, pending_marked_at timestamp with time zone)'::TEXT;

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR function_definition IS NULL
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                function_definition,
                'UTF8'
            )),
            'hex'
        ) IS DISTINCT FROM
            'f62a39f94d315b6f39b0e7d24b6dbd017e35fdcdc50e1e24ae49f1da1aa172b1'
        OR executor_grant_count > 1
        OR invalid_dependency_count <> 0
        OR invalid_function_acl_count <> 0
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_proc AS function_row
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE function_row.oid = pg_catalog.to_regprocedure(
                    'public.starring_product_apply_begin_runtime_drain_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text,text)'
                )
        ) <> (
            CASE WHEN executor_grant_count = 1 THEN 2 ELSE 1 END
        )
        OR invalid_relation_acl_count <> 0
        OR invalid_private_schema_acl_count <> 0
        OR pg_catalog.strpos(
            function_definition,
            'proposed_product_operation_id'
        ) = 0
        OR pg_catalog.strpos(
            function_definition,
            'proposed_drain_intent_id'
        ) = 0
        OR pg_catalog.strpos(
            function_definition,
            'public.starring_product_apply_lock_v1'
        ) = 0
        OR pg_catalog.strpos(
            function_definition,
            'starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2'
        ) = 0
        OR pg_catalog.strpos(
            function_definition,
            'starring_runtime_private_v2.starring_runtime_product_mutation_bytes_v2'
        ) = 0
        OR pg_catalog.strpos(
            function_definition,
            'starring_runtime_private_v2.starring_runtime_drain_intent_bytes_v2'
        ) = 0
        OR pg_catalog.strpos(
            function_definition,
            'starring.runtime.product_drain.apply.operation.v2'
        ) <> 0
        OR pg_catalog.strpos(
            function_definition,
            'starring.runtime.product_drain.apply.intent.v2'
        ) <> 0
        OR pg_catalog.strpos(
            function_definition,
            'gen_random'
        ) <> 0
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'PA001',
            MESSAGE =
                'product_apply_begin_runtime_drain_v2_postflight_drift';
    END IF;
END;
$postflight$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
