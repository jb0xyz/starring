SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

LOCK TABLE
    public.runtime_writer_fence,
    public.runtime_deployments
IN ACCESS EXCLUSIVE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    collision_count BIGINT;
    definition_digest TEXT;
    core_row pg_catalog.pg_proc%ROWTYPE;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT function_row.*
    INTO core_row
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'
    );

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname =
            'starring_product_apply_lock_core_unfenced_v1';

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(core_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO definition_digest;

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR core_row.oid IS NULL
        OR core_row.proowner <> common_owner
        OR core_row.prokind <> 'f'
        OR core_row.provolatile <> 'v'
        OR NOT core_row.proisstrict
        OR core_row.proparallel <> 'u'
        OR NOT core_row.prosecdef
        OR NOT core_row.proretset
        OR core_row.prorows <> 1::REAL
        OR core_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
        OR core_row.proleakproof
        OR core_row.pronargdefaults <> 0
        OR core_row.provariadic <> 0
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                core_row.proacl,
                pg_catalog.acldefault('f', core_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
        )
        OR definition_digest IS DISTINCT FROM
            'f31f6c2e37558e7d89d3125588acb71186421936ba2cf8762ca4b18462f8a693'
        OR collision_count <> 0
        OR (SELECT pg_catalog.count(*) FROM public.runtime_writer_fence) <> 1
        OR NOT EXISTS (
            SELECT 1
            FROM public.runtime_writer_fence AS fence
            WHERE fence.singleton
                AND fence.fence_state IN ('open', 'closed')
        )
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'PA001',
            MESSAGE = 'product_apply_writer_fence_preflight_drift';
    END IF;
END;
$preflight$;

ALTER FUNCTION public.starring_product_apply_lock_core_v1(
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
) RENAME TO starring_product_apply_lock_core_unfenced_v1;

REVOKE ALL ON FUNCTION public.starring_product_apply_lock_core_unfenced_v1(
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

CREATE FUNCTION public.starring_product_apply_lock_core_v1(
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
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 1
AS $function$
DECLARE
    writer_fence_state TEXT;
BEGIN
    PERFORM pg_catalog.pg_advisory_xact_lock_shared(
        pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
    );

    SELECT fence.fence_state
    INTO writer_fence_state
    FROM public.runtime_writer_fence AS fence
    WHERE fence.singleton;

    IF NOT FOUND
        OR (
            writer_fence_state IS DISTINCT FROM 'open'
            AND writer_fence_state IS DISTINCT FROM 'closed'
        )
    THEN
        RETURN QUERY SELECT 'runtime_writer_fence_invalid', FALSE, FALSE,
            NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;

    IF writer_fence_state = 'closed' THEN
        RETURN QUERY SELECT 'runtime_writer_fenced', FALSE, FALSE,
            NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;

    RETURN QUERY
    SELECT core.*
    FROM public.starring_product_apply_lock_core_unfenced_v1(
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
    ) AS core;
END;
$function$;

REVOKE ALL ON FUNCTION public.starring_product_apply_lock_core_v1(
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

DO $postflight$
DECLARE
    common_owner OID;
    active_digest TEXT;
    wrapper_digest TEXT;
    unfenced_digest TEXT;
    wrapper_source TEXT;
    invalid_function_count BIGINT;
    helper_collision_count BIGINT;
    external_grantee_count BIGINT;
    invalid_external_acl_count BIGINT;
    invalid_capability_acl_count BIGINT;
    external_grantee OID;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments');

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO active_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'
    );

    SELECT
        pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(function_row.oid),
                'UTF8'
            )),
            'hex'
        ),
        function_row.prosrc
    INTO wrapper_digest, wrapper_source
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'
    );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(function_row.oid),
            'UTF8'
        )),
        'hex'
    )
    INTO unfenced_digest
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_product_apply_lock_core_unfenced_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'
    );

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            ('public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'),
            ('public.starring_product_apply_lock_core_unfenced_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)')
    ) AS expected(identity)
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
        OR NOT function_row.prosecdef
        OR NOT function_row.proretset
        OR function_row.prorows <> 1::REAL
        OR function_row.proconfig
            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]
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
            WHERE privilege.grantee <> common_owner
        );

    SELECT pg_catalog.count(*)
    INTO helper_collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'starring_product_apply_lock_core_v1',
            'starring_product_apply_lock_core_unfenced_v1'
        );

    SELECT
        pg_catalog.count(*),
        pg_catalog.min(privilege.grantee::BIGINT)::OID,
        pg_catalog.count(*) FILTER (
            WHERE privilege.privilege_type <> 'EXECUTE'
                OR privilege.is_grantable
                OR privilege.grantor <> common_owner
        )
    INTO
        external_grantee_count,
        external_grantee,
        invalid_external_acl_count
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
    INTO invalid_capability_acl_count
    FROM (
        VALUES
            ('public.starring_product_apply_executor_database_identity_v1()'),
            ('public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'),
            ('public.starring_product_apply_target_artifact_v1(text,text,text,text,bytea,text,text)'),
            ('public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)'),
            ('public.starring_product_apply_keyring_coverage_v1(text[],text[])')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
        ) <> external_grantee_count
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> common_owner
                AND (
                    external_grantee_count <> 1
                    OR privilege.grantee IS DISTINCT FROM external_grantee
                    OR privilege.grantor <> common_owner
                    OR privilege.privilege_type <> 'EXECUTE'
                    OR privilege.is_grantable
                )
        );

    IF common_owner IS NULL
        OR invalid_function_count <> 0
        OR helper_collision_count <> 2
        OR external_grantee_count > 1
        OR (external_grantee_count = 1 AND external_grantee = 0)
        OR invalid_external_acl_count <> 0
        OR invalid_capability_acl_count <> 0
        OR active_digest IS DISTINCT FROM
            '35dff4eac9780b1cea497459ac513c54e5151fc752c290228951fadd6a4c2c2d'
        OR wrapper_digest IS DISTINCT FROM
            '9d854a6bcec21072bad8b9f725ea5401125d9842be3a4f5f238cdaaccb863eef'
        OR unfenced_digest IS DISTINCT FROM
            '4b01ced1c2b493a04ee4745be6593c10b493ffc06d73cf62f895c9ed46e21c0b'
        OR pg_catalog.strpos(
            wrapper_source,
            'pg_advisory_xact_lock_shared'
        ) = 0
        OR pg_catalog.strpos(
            wrapper_source,
            'starring_product_apply_lock_core_unfenced_v1'
        ) = 0
        OR pg_catalog.strpos(
            wrapper_source,
            'pg_advisory_xact_lock_shared'
        ) >= pg_catalog.strpos(
            wrapper_source,
            'starring_product_apply_lock_core_unfenced_v1'
        )
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'PA001',
            MESSAGE = 'product_apply_writer_fence_postflight_drift';
    END IF;
END;
$postflight$;

RESET statement_timeout;
RESET lock_timeout;
RESET search_path;
