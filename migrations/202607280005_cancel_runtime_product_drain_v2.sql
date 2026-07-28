SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

SELECT pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)
);

LOCK TABLE
    public.product_control_plane_identity,
    public.product_tenants,
    public.product_principals,
    public.product_auth_sessions,
    public.automation_installations,
    public.automation_installation_authority_versions,
    public.authoring_promotions,
    public.activation_requests,
    public.product_action_receipts,
    public.product_action_receipt_idempotency_aliases,
    public.product_action_receipt_audit_evidence,
    public.product_audit_events,
    public.runtime_writer_fence,
    public.runtime_slot_writer_fences_v2,
    public.runtime_serving_leases,
    public.runtime_deployments,
    public.runtime_product_operations_v2,
    public.runtime_drain_intents_v2,
    public.runtime_certification_operations_v2,
    public.runtime_certification_operation_terminals_v2,
    public.runtime_product_drain_terminal_actions_v2
IN ACCESS EXCLUSIVE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    invalid_relation_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    SELECT pg_catalog.count(*)
    INTO invalid_relation_count
    FROM (
        VALUES
            ('public.product_action_receipts'),
            ('public.product_action_receipt_idempotency_aliases'),
            ('public.product_action_receipt_audit_evidence'),
            ('public.product_audit_events'),
            ('public.runtime_writer_fence'),
            ('public.runtime_slot_writer_fences_v2'),
            ('public.runtime_serving_leases'),
            ('public.runtime_deployments'),
            ('public.runtime_product_operations_v2'),
            ('public.runtime_drain_intents_v2'),
            ('public.runtime_certification_operations_v2'),
            ('public.runtime_certification_operation_terminals_v2'),
            ('public.runtime_product_drain_terminal_actions_v2')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = pg_catalog.to_regclass(expected.identity)
    WHERE relation.oid IS NULL
        OR relation.relkind <> 'r'
        OR relation.relowner <> common_owner
        OR relation.relrowsecurity
        OR relation.relforcerowsecurity;

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR invalid_relation_count <> 0
        OR pg_catalog.to_regprocedure(
            'public.starring_product_apply_consume_runtime_drain_v2(text,text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text,bigint,bytea,text,text,text,bigint,text,text,bytea,text,bytea,text,text,bytea)'
        ) IS NULL
        OR pg_catalog.to_regprocedure(
            'public.starring_product_cancel_runtime_drain_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text,text,bigint,text,text,bigint)'
        ) IS NOT NULL
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'PA001',
            MESSAGE =
                'product_cancel_runtime_drain_v2_preflight_drift';
    END IF;
END;
$preflight$;

ALTER TABLE public.runtime_product_drain_terminal_actions_v2
ADD COLUMN source_deployment_snapshot_bytes BYTEA,
ADD COLUMN source_deployment_snapshot_digest TEXT,
ADD COLUMN source_canonical_state_bytes BYTEA,
ADD CONSTRAINT runtime_product_drain_terminal_actions_v2_source_snapshot_check
    CHECK (
        (
            terminal_kind = 'consumed'
            AND source_deployment_snapshot_bytes IS NULL
            AND source_deployment_snapshot_digest IS NULL
            AND source_canonical_state_bytes IS NULL
        )
        OR (
            terminal_kind = 'cancelled'
            AND source_deployment_snapshot_bytes IS NOT NULL
            AND pg_catalog.octet_length(
                source_deployment_snapshot_bytes
            ) BETWEEN 32 AND 262144
            AND source_deployment_snapshot_digest
                ~ '^[0-9a-f]{64}$'
            AND source_deployment_snapshot_digest =
                pg_catalog.encode(
                    pg_catalog.sha256(
                        source_deployment_snapshot_bytes
                    ),
                    'hex'
                )
            AND source_canonical_state_bytes IS NOT NULL
            AND pg_catalog.octet_length(
                source_canonical_state_bytes
            ) BETWEEN 1 AND 1048576
            AND source_canonical_state_digest =
                pg_catalog.encode(
                    pg_catalog.sha256(
                        source_canonical_state_bytes
                    ),
                    'hex'
                )
        )
    );

ALTER TABLE public.product_action_receipts
DROP CONSTRAINT product_action_receipts_approval_key_identity_required,
ADD CONSTRAINT product_action_receipts_approval_key_identity_required CHECK (
    endpoint_domain NOT IN (
        'product_approve_v1',
        'product_apply_v1',
        'product_promote_v1',
        'product_reject_v1',
        'product_cancel_lifecycle_v1'
    ) OR (
        idempotency_digest_key_id IS NOT NULL
        AND idempotency_digest_key_fingerprint IS NOT NULL
    )
);

DO $patch_product_cancellation_evidence$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.assert_product_approval_receipt_alias()'
    );
    previous_fragment :=
        '        ''product_reject_v1''' || E'\n' ||
        '    ) AND NOT EXISTS (';
    next_fragment :=
        '        ''product_reject_v1'',' || E'\n' ||
        '        ''product_cancel_lifecycle_v1''' || E'\n' ||
        '    ) AND NOT EXISTS (';
    IF definition IS NULL
        OR pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'product_cancellation_alias_guard_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.assert_product_approval_receipt_audit()'
    );
    previous_fragment :=
        '        WHEN ''product_reject_v1'' THEN ''promotion.reject''' || E'\n' ||
        '        ELSE NULL';
    next_fragment :=
        '        WHEN ''product_reject_v1'' THEN ''promotion.reject''' || E'\n' ||
        '        WHEN ''product_cancel_lifecycle_v1'' THEN ''promotion.cancel_lifecycle''' || E'\n' ||
        '        ELSE NULL';
    IF definition IS NULL
        OR pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'product_cancellation_audit_guard_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.capture_product_action_receipt_audit_evidence()'
    );
    previous_fragment :=
        '        WHEN ''product_reject_v1'' THEN ''promotion.reject''' || E'\n' ||
        '        ELSE NEW.action';
    next_fragment :=
        '        WHEN ''product_reject_v1'' THEN ''promotion.reject''' || E'\n' ||
        '        WHEN ''product_cancel_lifecycle_v1'' THEN ''promotion.cancel_lifecycle''' || E'\n' ||
        '        ELSE NEW.action';
    IF definition IS NULL
        OR pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'product_cancellation_evidence_capture_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$patch_product_cancellation_evidence$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_product_drain_cancel_root_exact_v2(
    product_row public.runtime_product_operations_v2,
    drain_row public.runtime_drain_intents_v2,
    source_row public.runtime_deployments,
    requested_product_operation_id TEXT,
    requested_drain_intent_id TEXT,
    requested_source_intent_revision BIGINT,
    requested_source_state_digest TEXT
)
RETURNS BOOLEAN
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    product_value JSONB;
    original_semantic_request_digest TEXT;
BEGIN
    product_value := pg_catalog.convert_from(
        product_row.product_mutation_request_bytes,
        'UTF8'
    )::JSONB;
    original_semantic_request_digest :=
        product_value ->> 'product_semantic_request_digest';
    RETURN original_semantic_request_digest
            ~ '^[0-9a-f]{64}$'
        AND starring_runtime_private_v2.starring_runtime_product_drain_consume_root_exact_v2(
            product_row,
            drain_row,
            source_row,
            requested_product_operation_id,
            requested_drain_intent_id,
            requested_source_intent_revision,
            drain_row.canonical_state_bytes,
            requested_source_state_digest,
            original_semantic_request_digest
        );
EXCEPTION
    WHEN OTHERS THEN
        RETURN FALSE;
END;
$function$;

DO $patch_cancellation_execution_manifest$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
    identity TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_schema_manifest_v1()'
    );
    previous_fragment :=
        '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
        '            ''starring_runtime_private_v2.starring_runtime_product_drain_terminal_action_exact_v2(public.runtime_product_drain_terminal_actions_v2,public.runtime_product_operations_v2,public.runtime_drain_intents_v2)''' || E'\n' ||
        '        )';
    next_fragment := previous_fragment;
    FOREACH identity IN ARRAY ARRAY[
        'public.starring_product_cancel_runtime_drain_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text,text,bigint,text,text,bigint)',
        'starring_runtime_private_v2.starring_runtime_product_drain_cancel_root_exact_v2(public.runtime_product_operations_v2,public.runtime_drain_intents_v2,public.runtime_deployments,text,text,bigint,text)',
        'starring_runtime_private_v2.starring_product_lifecycle_cancellation_record_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,timestamp with time zone)',
        'starring_runtime_private_v2.starring_product_lifecycle_cancellation_unkeyed_digest_v2(text,text[])',
        'starring_runtime_private_v2.starring_runtime_product_drain_cancelled_terminal_exact_v2(public.runtime_product_drain_terminal_actions_v2,public.runtime_product_operations_v2,public.runtime_drain_intents_v2)',
        'starring_runtime_private_v2.starring_runtime_product_drain_cancel_source_v2(text,text,bigint,text,text,text,timestamp with time zone)'
    ]
    LOOP
        next_fragment := next_fragment || E'\n' ||
            '        UNION' || E'\n' ||
            '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
            '            ' ||
                pg_catalog.quote_literal(identity) || E'\n' ||
            '        )';
    END LOOP;
    IF definition IS NULL
        OR pg_catalog.strpos(
            definition,
            previous_fragment
        ) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(
                definition,
                previous_fragment,
                ''
            ),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'product_lifecycle_cancellation_manifest_function_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    RETURN observed_count = 901' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''5d70734095987ce4f70a9edddccd345e99a62e2c2090c6c8c11cc662d092d065'';';
    next_fragment :=
        '    RETURN observed_count = 911' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''ae39639ca7f4f2d911e227b8429d1566efdc677dbfd641d8fcf5f24d376baf8b'';';
    IF pg_catalog.strpos(
            definition,
            previous_fragment
        ) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(
                definition,
                previous_fragment,
                ''
            ),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'product_lifecycle_cancellation_manifest_expectation_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$patch_cancellation_execution_manifest$;

DO $patch_cancellation_execution_readiness$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_runtime_execution_database_readiness_v1()'
    );
    previous_fragment :=
        '            (''starring_runtime_private_v2.starring_runtime_product_drain_terminal_action_exact_v2(public.runtime_product_drain_terminal_actions_v2,public.runtime_product_operations_v2,public.runtime_drain_intents_v2)''),';
    next_fragment := previous_fragment || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_product_drain_cancel_root_exact_v2(public.runtime_product_operations_v2,public.runtime_drain_intents_v2,public.runtime_deployments,text,text,bigint,text)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_product_lifecycle_cancellation_record_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,timestamp with time zone)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_product_lifecycle_cancellation_unkeyed_digest_v2(text,text[])''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_product_drain_cancelled_terminal_exact_v2(public.runtime_product_drain_terminal_actions_v2,public.runtime_product_operations_v2,public.runtime_drain_intents_v2)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_product_drain_cancel_source_v2(text,text,bigint,text,text,text,timestamp with time zone)''),';
    IF definition IS NULL
        OR pg_catalog.strpos(
            definition,
            previous_fragment
        ) = 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'product_lifecycle_cancellation_readiness_private_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '''d65a674f2d4ce2337a6bf8c5d74ad63ff21f0090a70e3bf1049e07dd18bc3abd''::TEXT';
    next_fragment :=
        '''b7ee8d2a13ae38a88bc1b2558b018e74893e7d90ccd72d96187197a111432e22''::TEXT';
    IF pg_catalog.strpos(
            definition,
            previous_fragment
        ) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(
                definition,
                previous_fragment,
                ''
            ),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'product_lifecycle_cancellation_readiness_manifest_digest_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$patch_cancellation_execution_readiness$;

DO $seal_product_lifecycle_cancellation_acl$
DECLARE
    identity TEXT;
BEGIN
    FOREACH identity IN ARRAY ARRAY[
        'public.starring_product_lifecycle_cancellation_executor_database_identity_v1()',
        'public.starring_product_lifecycle_cancellation_keyring_coverage_v1(text[],text[])',
        'public.starring_product_cancel_runtime_drain_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text,text,bigint,text,text,bigint)',
        'starring_runtime_private_v2.starring_runtime_product_drain_cancel_root_exact_v2(public.runtime_product_operations_v2,public.runtime_drain_intents_v2,public.runtime_deployments,text,text,bigint,text)',
        'starring_runtime_private_v2.starring_product_lifecycle_cancellation_record_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,timestamp with time zone)',
        'starring_runtime_private_v2.starring_product_lifecycle_cancellation_unkeyed_digest_v2(text,text[])',
        'starring_runtime_private_v2.starring_runtime_product_drain_cancelled_terminal_exact_v2(public.runtime_product_drain_terminal_actions_v2,public.runtime_product_operations_v2,public.runtime_drain_intents_v2)',
        'starring_runtime_private_v2.starring_runtime_product_drain_cancel_source_v2(text,text,bigint,text,text,text,timestamp with time zone)'
    ]
    LOOP
        IF pg_catalog.to_regprocedure(identity) IS NOT NULL THEN
            EXECUTE pg_catalog.format(
                'REVOKE ALL ON FUNCTION %s FROM PUBLIC',
                identity
            );
        END IF;
    END LOOP;
END;
$seal_product_lifecycle_cancellation_acl$;

CREATE FUNCTION public.starring_product_cancel_runtime_drain_v2(
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
    proposed_terminal_action_id TEXT,
    expected_cancellation_reason TEXT,
    expected_cancellation_reason_digest TEXT,
    expected_drain_intent_id TEXT,
    expected_source_intent_revision BIGINT,
    expected_source_state_digest TEXT,
    expected_product_operation_id TEXT,
    expected_source_deployment_revision BIGINT
)
RETURNS TABLE(
    outcome_name TEXT,
    exact_replay BOOLEAN,
    product_resulting_revision BIGINT,
    product_resulting_state TEXT,
    guild_id TEXT,
    product_receipt_id TEXT,
    product_audit_event_id TEXT,
    cancellation_reason_digest TEXT,
    product_operation_id TEXT,
    source_product_mutation_request_bytes BYTEA,
    product_mutation_digest TEXT,
    source_drain_intent_request_bytes BYTEA,
    drain_intent_digest TEXT,
    source_deployment_id TEXT,
    source_deployment_revision BIGINT,
    source_deployment_snapshot JSONB,
    source_deployment_snapshot_digest TEXT,
    source_result_deployment_revision BIGINT,
    source_result_deployment_snapshot JSONB,
    source_result_deployment_snapshot_digest TEXT,
    drain_intent_id TEXT,
    source_intent_revision BIGINT,
    source_state_bytes BYTEA,
    source_state_digest TEXT,
    result_intent_revision BIGINT,
    result_intent_state TEXT,
    result_state_bytes BYTEA,
    result_state_digest TEXT,
    source_slot_epoch BIGINT,
    successor_slot_epoch BIGINT,
    terminal_action_id TEXT,
    terminal_projection_bytes BYTEA,
    terminal_projection_digest TEXT,
    terminal_database_time TIMESTAMPTZ
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
    optimistic_drain_row public.runtime_drain_intents_v2%ROWTYPE;
    locked_source_row public.runtime_deployments%ROWTYPE;
    root_source_row public.runtime_deployments%ROWTYPE;
    source_result_row public.runtime_deployments%ROWTYPE;
    product_row public.runtime_product_operations_v2%ROWTYPE;
    drain_row public.runtime_drain_intents_v2%ROWTYPE;
    terminal_drain_row public.runtime_drain_intents_v2%ROWTYPE;
    slot_fence_row public.runtime_slot_writer_fences_v2%ROWTYPE;
    serving_row public.runtime_serving_leases%ROWTYPE;
    action_row public.runtime_product_drain_terminal_actions_v2%ROWTYPE;
    receipt_row public.product_action_receipts%ROWTYPE;
    audit_row public.product_audit_events%ROWTYPE;
    cancellation_record_row RECORD;
    source_state_value JSONB;
    source_certification JSONB;
    acknowledged_microseconds NUMERIC;
    acknowledged_time TIMESTAMPTZ;
    terminal_microseconds NUMERIC;
    terminal_time TIMESTAMPTZ;
    computed_semantic_digest TEXT;
    computed_reason_digest TEXT;
    source_snapshot_bytes BYTEA;
    source_snapshot_digest TEXT;
    source_result_snapshot_bytes BYTEA;
    source_result_snapshot_digest TEXT;
    preparation_binding_digest TEXT;
    computed_preparation_token TEXT;
    computed_successor_epoch BIGINT;
    computed_terminal_projection BYTEA;
    computed_terminal_projection_digest TEXT;
    action_count BIGINT;
    certification_operation_count BIGINT;
    certification_terminal_count BIGINT;
    unresolved_count BIGINT;
    mutation_failure_outcome TEXT;
BEGIN
    exact_replay := FALSE;
    computed_semantic_digest :=
        starring_runtime_private_v2.starring_product_lifecycle_cancellation_unkeyed_digest_v2(
            'starring.product.lifecycle-cancellation.request.v1',
            ARRAY[
                expected_tenant_id,
                expected_installation_id,
                expected_principal_id,
                expected_promotion_id,
                expected_product_revision::TEXT,
                expected_payload_digest,
                expected_drain_intent_id,
                expected_source_intent_revision::TEXT,
                expected_source_state_digest,
                expected_product_operation_id,
                expected_source_deployment_revision::TEXT,
                expected_cancellation_reason
            ]
        );
    computed_reason_digest :=
        starring_runtime_private_v2.starring_product_lifecycle_cancellation_unkeyed_digest_v2(
            'starring.product.lifecycle-cancellation.reason.v1',
            ARRAY[
                expected_tenant_id,
                expected_installation_id,
                expected_promotion_id,
                expected_drain_intent_id,
                expected_cancellation_reason
            ]
        );

    IF pg_catalog.current_setting('transaction_isolation')
            <> 'serializable'
        OR pg_catalog.current_setting('transaction_read_only')
            <> 'off'
        OR expected_capability <> 'cancel_lifecycle'
        OR proposed_terminal_action_id
            !~ '^[0-9a-f]{64}$'
        OR expected_drain_intent_id
            !~ '^[0-9a-f]{32}$'
        OR expected_source_intent_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR expected_source_state_digest
            !~ '^[0-9a-f]{64}$'
        OR expected_product_operation_id
            !~ '^[0-9a-f]{32}$'
        OR expected_product_operation_id =
            expected_drain_intent_id
        OR expected_source_deployment_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR expected_cancellation_reason
            IS DISTINCT FROM
                pg_catalog.btrim(expected_cancellation_reason)
        OR pg_catalog.char_length(
            expected_cancellation_reason
        ) NOT BETWEEN 1 AND 1000
        OR pg_catalog.octet_length(
            expected_cancellation_reason
        ) > 4000
        OR expected_cancellation_reason
            ~ U&'[\0001-\001F\007F-\009F]'
        OR computed_reason_digest IS DISTINCT FROM
            expected_cancellation_reason_digest
        OR computed_semantic_digest IS DISTINCT FROM
            semantic_request_digest
    THEN
        outcome_name := 'invalid_input';
        RETURN NEXT;
        RETURN;
    END IF;

    PERFORM activation.id
    FROM public.activation_requests AS activation
    WHERE activation.tenant_id = expected_tenant_id
        AND activation.installation_id =
            expected_installation_id
        AND activation.promotion_id =
            expected_promotion_id
    FOR UPDATE;

    PERFORM pg_catalog.pg_advisory_xact_lock_shared(
        pg_catalog.hashtextextended(
            'starring-runtime-writer-fence-v1',
            0
        )
    );
    SELECT fence.fence_state
    INTO writer_fence_state
    FROM public.runtime_writer_fence AS fence
    WHERE fence.singleton
    FOR SHARE;
    IF NOT FOUND
        OR writer_fence_state NOT IN ('open', 'closed')
        OR (
            SELECT pg_catalog.count(*)
            FROM public.runtime_writer_fence
        ) <> 1
    THEN
        outcome_name := 'persistence_corrupt';
        RETURN NEXT;
        RETURN;
    END IF;
    IF writer_fence_state = 'closed' THEN
        outcome_name := 'writer_fenced';
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT drain.*
    INTO optimistic_drain_row
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE drain.drain_intent_id =
            expected_drain_intent_id;
    IF NOT FOUND THEN
        outcome_name := 'not_found';
        RETURN NEXT;
        RETURN;
    END IF;
    IF optimistic_drain_row.tenant_id <>
            expected_tenant_id
        OR optimistic_drain_row.installation_id <>
            expected_installation_id
        OR optimistic_drain_row.product_operation_id <>
            expected_product_operation_id
    THEN
        outcome_name := 'scope_mismatch';
        RETURN NEXT;
        RETURN;
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            pg_catalog.concat(
                'starring-runtime-serving-slot-v1:',
                optimistic_drain_row.slot_guild_id,
                ':',
                optimistic_drain_row.slot_ruleset_key
            ),
            0
        )
    );
    PERFORM fence.writer_epoch
    FROM starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(
        optimistic_drain_row.slot_guild_id,
        optimistic_drain_row.slot_ruleset_key
    ) AS fence;
    IF NOT FOUND THEN
        outcome_name := 'persistence_corrupt';
        RETURN NEXT;
        RETURN;
    END IF;
    SELECT fence.*
    INTO slot_fence_row
    FROM public.runtime_slot_writer_fences_v2 AS fence
    WHERE fence.slot_guild_id =
            optimistic_drain_row.slot_guild_id
        AND fence.slot_ruleset_key =
            optimistic_drain_row.slot_ruleset_key
    FOR UPDATE;
    IF NOT FOUND THEN
        outcome_name := 'persistence_corrupt';
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT serving.*
    INTO serving_row
    FROM public.runtime_serving_leases AS serving
    WHERE serving.guild_id =
            optimistic_drain_row.slot_guild_id
        AND serving.ruleset_key =
            optimistic_drain_row.slot_ruleset_key
    FOR UPDATE;

    PERFORM deployment.deployment_id
    FROM public.runtime_deployments AS deployment
    WHERE deployment.guild_id =
            optimistic_drain_row.slot_guild_id
        AND deployment.ruleset_key =
            optimistic_drain_row.slot_ruleset_key
    ORDER BY
        deployment.runtime_generation,
        deployment.deployment_id
    FOR UPDATE;

    SELECT deployment.*
    INTO locked_source_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.deployment_id =
            optimistic_drain_row.deployment_id
    FOR UPDATE;
    IF NOT FOUND
        OR locked_source_row.tenant_id <>
            expected_tenant_id
        OR locked_source_row.installation_id <>
            expected_installation_id
        OR locked_source_row.guild_id <>
            optimistic_drain_row.slot_guild_id
        OR locked_source_row.ruleset_key <>
            optimistic_drain_row.slot_ruleset_key
    THEN
        outcome_name := 'scope_mismatch';
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT product.*
    INTO product_row
    FROM public.runtime_product_operations_v2 AS product
    WHERE product.product_operation_id =
            expected_product_operation_id
    FOR KEY SHARE;
    IF NOT FOUND THEN
        outcome_name := 'persistence_corrupt';
        RETURN NEXT;
        RETURN;
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            'starring-runtime-product-drain-v2:'
                || expected_drain_intent_id,
            0
        )
    );
    SELECT drain.*
    INTO drain_row
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE drain.drain_intent_id =
            expected_drain_intent_id
    FOR UPDATE;
    IF NOT FOUND THEN
        outcome_name := 'persistence_corrupt';
        RETURN NEXT;
        RETURN;
    END IF;

    PERFORM reservation.operation_id
    FROM public.runtime_certification_operations_v2 AS reservation
    WHERE reservation.tenant_id = drain_row.tenant_id
        AND reservation.installation_id =
            drain_row.installation_id
        AND reservation.deployment_id =
            drain_row.deployment_id
        AND reservation.deployment_revision =
            drain_row.expected_revision
    ORDER BY reservation.operation_id
    FOR KEY SHARE;
    PERFORM terminal.operation_id
    FROM public.runtime_certification_operation_terminals_v2 AS terminal
    INNER JOIN public.runtime_certification_operations_v2 AS reservation
        ON reservation.operation_id = terminal.operation_id
    WHERE reservation.tenant_id = drain_row.tenant_id
        AND reservation.installation_id =
            drain_row.installation_id
        AND reservation.deployment_id =
            drain_row.deployment_id
        AND reservation.deployment_revision =
            drain_row.expected_revision
    ORDER BY terminal.operation_id
    FOR KEY SHARE OF terminal;

    PERFORM action.terminal_action_id
    FROM public.runtime_product_drain_terminal_actions_v2 AS action
    WHERE action.terminal_action_id =
            proposed_terminal_action_id
        OR action.drain_intent_id =
            expected_drain_intent_id
        OR action.product_action_idempotency_digest =
            ANY(idempotency_key_digest_candidates)
        OR action.product_action_semantic_request_digest =
            semantic_request_digest
    ORDER BY action.terminal_action_id
    FOR UPDATE;

    SELECT pg_catalog.count(*)
    INTO action_count
    FROM public.runtime_product_drain_terminal_actions_v2 AS action
    WHERE action.terminal_action_id =
            proposed_terminal_action_id
        OR action.drain_intent_id =
            expected_drain_intent_id
        OR action.product_action_idempotency_digest =
            ANY(idempotency_key_digest_candidates)
        OR action.product_action_semantic_request_digest =
            semantic_request_digest;

    IF action_count > 1 THEN
        outcome_name := 'persistence_corrupt';
        RETURN NEXT;
        RETURN;
    END IF;

    IF action_count = 1 THEN
        SELECT action.*
        INTO STRICT action_row
        FROM public.runtime_product_drain_terminal_actions_v2
            AS action
        WHERE action.terminal_action_id =
                proposed_terminal_action_id
            OR action.drain_intent_id =
                expected_drain_intent_id
            OR action.product_action_idempotency_digest =
                ANY(idempotency_key_digest_candidates)
            OR action.product_action_semantic_request_digest =
                semantic_request_digest;

        IF action_row.terminal_kind <> 'cancelled'
            OR action_row.terminal_action_id <>
                proposed_terminal_action_id
            OR action_row.drain_intent_id <>
                expected_drain_intent_id
            OR action_row.product_operation_id <>
                expected_product_operation_id
            OR NOT (
                action_row.product_action_idempotency_digest =
                    ANY(idempotency_key_digest_candidates)
            )
            OR action_row.product_action_semantic_request_digest <>
                semantic_request_digest
            OR action_row.cancellation_reason_digest <>
                expected_cancellation_reason_digest
            OR action_row.source_intent_revision <>
                expected_source_intent_revision
            OR action_row.source_canonical_state_digest <>
                expected_source_state_digest
            OR action_row.source_deployment_revision <>
                expected_source_deployment_revision
            OR action_row.product_mutation_digest <>
                product_row.product_mutation_digest
            OR action_row.drain_intent_digest <>
                drain_row.drain_intent_digest
        THEN
            outcome_name := CASE
                WHEN action_row.product_action_idempotency_digest =
                    ANY(idempotency_key_digest_candidates)
                    THEN 'idempotency_conflict'
                ELSE 'terminal_conflict'
            END;
            RETURN NEXT;
            RETURN;
        END IF;

        terminal_drain_row := drain_row;
        root_source_row := locked_source_row;
        BEGIN
            root_source_row.snapshot :=
                pg_catalog.convert_from(
                    action_row.source_deployment_snapshot_bytes,
                    'UTF8'
                )::JSONB;
        EXCEPTION
            WHEN OTHERS THEN
                outcome_name := 'persistence_corrupt';
                RETURN NEXT;
                RETURN;
        END;
        root_source_row.revision :=
            action_row.source_deployment_revision;
        drain_row.intent_revision :=
            action_row.source_intent_revision;
        drain_row.intent_state :=
            'route_absent_acknowledged';
        drain_row.canonical_state_bytes :=
            action_row.source_canonical_state_bytes;
        drain_row.canonical_state_digest :=
            action_row.source_canonical_state_digest;

        IF NOT starring_runtime_private_v2.starring_runtime_product_drain_cancel_root_exact_v2(
                product_row,
                drain_row,
                root_source_row,
                expected_product_operation_id,
                expected_drain_intent_id,
                expected_source_intent_revision,
                expected_source_state_digest
            )
        THEN
            outcome_name := 'persistence_corrupt';
            RETURN NEXT;
            RETURN;
        END IF;
        drain_row := terminal_drain_row;

        SELECT receipt.*
        INTO receipt_row
        FROM public.product_action_receipts AS receipt
        WHERE receipt.receipt_id =
                action_row.product_receipt_id
        FOR KEY SHARE;
        SELECT audit.*
        INTO audit_row
        FROM public.product_audit_events AS audit
        WHERE audit.event_id =
                action_row.product_audit_event_id
        FOR KEY SHARE;

        IF drain_row.intent_state <> 'cancelled'
            OR NOT starring_runtime_private_v2.starring_runtime_product_drain_cancelled_terminal_exact_v2(
                action_row,
                product_row,
                drain_row
            )
            OR receipt_row.receipt_id IS NULL
            OR receipt_row.tenant_id <> expected_tenant_id
            OR receipt_row.installation_id <>
                expected_installation_id
            OR receipt_row.principal_id <>
                expected_principal_id
            OR receipt_row.endpoint_domain <>
                'product_cancel_lifecycle_v1'
            OR receipt_row.request_digest <>
                semantic_request_digest
            OR receipt_row.target_resource_type <>
                'authoring_promotion'
            OR receipt_row.target_resource_id <>
                expected_promotion_id
            OR receipt_row.resulting_revision IS DISTINCT FROM
                expected_product_revision
            OR receipt_row.resulting_state <> 'approved'
            OR receipt_row.result_code <>
                'runtime_drain_cancelled'
            OR receipt_row.http_disposition_class <> 2
            OR receipt_row.completed_at <>
                action_row.terminal_database_time
            OR audit_row.event_id IS NULL
            OR audit_row.receipt_id <>
                receipt_row.receipt_id
            OR audit_row.tenant_id <> expected_tenant_id
            OR audit_row.installation_id <>
                expected_installation_id
            OR audit_row.principal_id <>
                expected_principal_id
            OR audit_row.action <>
                'promotion.cancel_lifecycle'
            OR audit_row.authority_observation_digest <>
                action_row.authority_observation_digest
            OR audit_row.installation_authority_revision <>
                action_row.installation_authority_revision
            OR audit_row.resulting_state <> 'approved'
            OR audit_row.result_code <>
                'runtime_drain_cancelled'
            OR audit_row.occurred_at <>
                action_row.terminal_database_time
            OR locked_source_row.revision <
                action_row.source_result_deployment_revision
            OR slot_fence_row.writer_epoch <
                action_row.successor_slot_writer_epoch
        THEN
            outcome_name := 'persistence_corrupt';
            RETURN NEXT;
            RETURN;
        END IF;

        SELECT cancellation.*
        INTO cancellation_record_row
        FROM starring_runtime_private_v2.starring_product_lifecycle_cancellation_record_v2(
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
            expected_cancellation_reason,
            action_row.terminal_database_time
        ) AS cancellation;

        IF cancellation_record_row.outcome
                IS DISTINCT FROM 'ok'
            OR cancellation_record_row.resulting_revision
                IS DISTINCT FROM expected_product_revision
            OR cancellation_record_row.resulting_state
                IS DISTINCT FROM 'approved'
            OR cancellation_record_row.exact_replay
                IS DISTINCT FROM TRUE
            OR cancellation_record_row.guild_id
                IS DISTINCT FROM expected_guild_id
        THEN
            outcome_name := CASE
                cancellation_record_row.outcome
                WHEN 'invalid_input' THEN 'invalid_input'
                WHEN 'not_found' THEN 'not_found'
                WHEN 'authorization_stale'
                    THEN 'authorization_stale'
                WHEN 'authority_mismatch'
                    THEN 'authority_mismatch'
                WHEN 'scope_mismatch' THEN 'scope_mismatch'
                WHEN 'payload_mismatch'
                    THEN 'payload_mismatch'
                WHEN 'revision_conflict'
                    THEN 'revision_conflict'
                WHEN 'idempotency_conflict'
                    THEN 'idempotency_conflict'
                WHEN 'idempotency_keyring_incomplete'
                    THEN 'idempotency_keyring_incomplete'
                WHEN 'expired' THEN 'authorization_stale'
                WHEN 'invalid_state'
                    THEN 'revision_conflict'
                WHEN 'indeterminate' THEN 'indeterminate'
                ELSE 'persistence_corrupt'
            END;
            RETURN NEXT;
            RETURN;
        END IF;

        outcome_name := 'replayed';
        exact_replay := TRUE;
        product_resulting_revision :=
            receipt_row.resulting_revision;
        product_resulting_state :=
            receipt_row.resulting_state;
        guild_id := expected_guild_id;
        product_receipt_id := receipt_row.receipt_id;
        product_audit_event_id := audit_row.event_id;
        cancellation_reason_digest :=
            action_row.cancellation_reason_digest;
        product_operation_id :=
            action_row.product_operation_id;
        source_product_mutation_request_bytes :=
            product_row.product_mutation_request_bytes;
        product_mutation_digest :=
            action_row.product_mutation_digest;
        source_drain_intent_request_bytes :=
            drain_row.drain_intent_request_bytes;
        drain_intent_digest :=
            action_row.drain_intent_digest;
        source_deployment_id :=
            product_row.deployment_id;
        source_deployment_revision :=
            action_row.source_deployment_revision;
        source_deployment_snapshot :=
            pg_catalog.convert_from(
                action_row.source_deployment_snapshot_bytes,
                'UTF8'
            )::JSONB;
        source_deployment_snapshot_digest :=
            action_row.source_deployment_snapshot_digest;
        source_result_deployment_revision :=
            action_row.source_result_deployment_revision;
        source_result_deployment_snapshot :=
            pg_catalog.convert_from(
                action_row.source_result_deployment_snapshot_bytes,
                'UTF8'
            )::JSONB;
        source_result_deployment_snapshot_digest :=
            action_row.source_result_deployment_snapshot_digest;
        drain_intent_id := action_row.drain_intent_id;
        source_intent_revision :=
            action_row.source_intent_revision;
        source_state_bytes :=
            action_row.source_canonical_state_bytes;
        source_state_digest :=
            action_row.source_canonical_state_digest;
        result_intent_revision :=
            action_row.result_intent_revision;
        result_intent_state := drain_row.intent_state;
        result_state_bytes :=
            drain_row.canonical_state_bytes;
        result_state_digest :=
            drain_row.canonical_state_digest;
        source_slot_epoch :=
            action_row.source_slot_writer_epoch;
        successor_slot_epoch :=
            action_row.successor_slot_writer_epoch;
        terminal_action_id :=
            action_row.terminal_action_id;
        terminal_projection_bytes :=
            action_row.terminal_projection_bytes;
        terminal_projection_digest :=
            action_row.terminal_projection_digest;
        terminal_database_time :=
            action_row.terminal_database_time;
        RETURN NEXT;
        RETURN;
    END IF;

    IF drain_row.intent_state IN ('consumed', 'cancelled') THEN
        outcome_name := 'persistence_corrupt';
        RETURN NEXT;
        RETURN;
    END IF;

    IF drain_row.intent_revision <>
            expected_source_intent_revision
        OR drain_row.canonical_state_digest <>
            expected_source_state_digest
        OR locked_source_row.revision <>
            expected_source_deployment_revision
    THEN
        outcome_name := 'revision_conflict';
        RETURN NEXT;
        RETURN;
    END IF;

    BEGIN
        source_state_value := pg_catalog.convert_from(
            drain_row.canonical_state_bytes,
            'UTF8'
        )::JSONB;
        acknowledged_microseconds := (
            source_state_value
                #>>
                    '{state,acknowledgement,acknowledged_at_unix_microseconds}'
        )::NUMERIC;
        acknowledged_time := pg_catalog.to_timestamp(
            acknowledged_microseconds / 1000000
        );
        source_certification :=
            source_state_value
                #> '{state,acknowledgement,certification}';
    EXCEPTION
        WHEN OTHERS THEN
            outcome_name := 'persistence_corrupt';
            RETURN NEXT;
            RETURN;
    END;

    IF acknowledged_microseconds NOT BETWEEN
            -62135596800000000 AND 253402300799999999
        OR acknowledged_microseconds <>
            pg_catalog.trunc(acknowledged_microseconds)
        OR pg_catalog.jsonb_typeof(source_certification)
            <> 'object'
    THEN
        outcome_name := 'persistence_corrupt';
        RETURN NEXT;
        RETURN;
    END IF;

    root_source_row := locked_source_row;
    IF NOT starring_runtime_private_v2.starring_runtime_product_drain_cancel_root_exact_v2(
            product_row,
            drain_row,
            root_source_row,
            expected_product_operation_id,
            expected_drain_intent_id,
            expected_source_intent_revision,
            expected_source_state_digest
        )
    THEN
        outcome_name := 'persistence_corrupt';
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT pg_catalog.count(*)
    INTO certification_operation_count
    FROM public.runtime_certification_operations_v2
        AS reservation
    WHERE reservation.tenant_id = drain_row.tenant_id
        AND reservation.installation_id =
            drain_row.installation_id
        AND reservation.deployment_id =
            drain_row.deployment_id
        AND reservation.deployment_revision =
            drain_row.expected_revision;
    SELECT pg_catalog.count(*)
    INTO certification_terminal_count
    FROM public.runtime_certification_operations_v2
        AS reservation
    INNER JOIN public.runtime_certification_operation_terminals_v2
        AS terminal
        ON terminal.operation_id = reservation.operation_id
    WHERE reservation.tenant_id = drain_row.tenant_id
        AND reservation.installation_id =
            drain_row.installation_id
        AND reservation.deployment_id =
            drain_row.deployment_id
        AND reservation.deployment_revision =
            drain_row.expected_revision
        AND reservation.operation_id =
            source_certification ->> 'operation_id'
        AND reservation.intent_fingerprint =
            source_certification ->> 'intent_fingerprint'
        AND terminal.intent_fingerprint =
            reservation.intent_fingerprint
        AND terminal.tenant_id = reservation.tenant_id
        AND terminal.installation_id =
            reservation.installation_id
        AND terminal.deployment_id =
            reservation.deployment_id
        AND terminal.deployment_revision =
            reservation.deployment_revision
        AND terminal.convergence_attempt_no =
            reservation.convergence_attempt_no
        AND terminal.terminal_outcome_name =
            'awaiting_reset';

    IF (
            source_certification ->> 'kind' =
                'no_operation_reserved'
            AND (
                (
                    SELECT pg_catalog.count(*)
                    FROM pg_catalog.jsonb_object_keys(
                        source_certification
                    )
                ) <> 1
                OR certification_operation_count <> 0
            )
        )
        OR (
            source_certification ->> 'kind' =
                'no_attestation_for_reserved_operation'
            AND (
                (
                    SELECT pg_catalog.count(*)
                    FROM pg_catalog.jsonb_object_keys(
                        source_certification
                    )
                ) <> 3
                OR certification_operation_count <> 1
                OR certification_terminal_count <> 1
            )
        )
        OR source_certification ->> 'kind' NOT IN (
            'no_operation_reserved',
            'no_attestation_for_reserved_operation'
        )
    THEN
        outcome_name := 'persistence_corrupt';
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT pg_catalog.count(*)
    INTO unresolved_count
    FROM public.runtime_deployments AS deployment
    WHERE deployment.guild_id =
            locked_source_row.guild_id
        AND deployment.ruleset_key =
            locked_source_row.ruleset_key
        AND deployment.phase NOT IN (
            'superseded',
            'cancelled'
        )
        AND deployment.deployment_id <>
            locked_source_row.deployment_id;

    IF locked_source_row.phase NOT IN (
            'awaiting_gateway_ready',
            'live'
        )
        OR locked_source_row.controller_id IS NOT NULL
        OR locked_source_row.controller_fencing_token
            IS NOT NULL
        OR locked_source_row.controller_acquired_at
            IS NOT NULL
        OR locked_source_row.controller_lease_expires_at
            IS NOT NULL
        OR unresolved_count <> 0
        OR (
            serving_row.deployment_id IS NOT NULL
            AND (
                serving_row.deployment_id <>
                    locked_source_row.deployment_id
                OR serving_row.connected
                OR serving_row.serving
            )
        )
        OR slot_fence_row.pending_drain_intent_id <>
            expected_drain_intent_id
        OR slot_fence_row.pending_product_operation_id <>
            expected_product_operation_id
        OR slot_fence_row.pending_tenant_id <>
            expected_tenant_id
        OR slot_fence_row.pending_installation_id <>
            expected_installation_id
        OR slot_fence_row.pending_deployment_id <>
            locked_source_row.deployment_id
        OR slot_fence_row.pending_expected_revision <>
            expected_source_deployment_revision
        OR slot_fence_row.writer_epoch
            NOT BETWEEN 1 AND 9223372036854775806
        OR drain_row.intent_state <>
            'route_absent_acknowledged'
        OR drain_row.canonical_state_bytes IS NULL
    THEN
        outcome_name := 'revision_conflict';
        RETURN NEXT;
        RETURN;
    END IF;

    terminal_time := pg_catalog.date_trunc(
        'microseconds',
        GREATEST(
            pg_catalog.transaction_timestamp(),
            acknowledged_time,
            locked_source_row.updated_at
                + INTERVAL '1 microsecond',
            slot_fence_row.updated_at
                + INTERVAL '1 microsecond',
            slot_fence_row.pending_marked_at
                + INTERVAL '1 microsecond'
        )
    );
    terminal_microseconds :=
        EXTRACT(EPOCH FROM terminal_time) * 1000000;
    IF terminal_microseconds NOT BETWEEN
            -62135596800000000 AND 253402300799999999
        OR terminal_microseconds <>
            pg_catalog.trunc(terminal_microseconds)
        OR terminal_time < acknowledged_time
        OR terminal_time <= locked_source_row.updated_at
        OR terminal_time <= slot_fence_row.updated_at
        OR terminal_time <= slot_fence_row.pending_marked_at
        OR terminal_time >= expected_authority_expires_at
    THEN
        outcome_name := 'authorization_stale';
        RETURN NEXT;
        RETURN;
    END IF;

    source_snapshot_bytes := pg_catalog.convert_to(
        locked_source_row.snapshot::TEXT,
        'UTF8'
    );
    source_snapshot_digest := pg_catalog.encode(
        pg_catalog.sha256(source_snapshot_bytes),
        'hex'
    );
    preparation_binding_digest := pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.jsonb_build_array(
                    expected_tenant_id,
                    expected_installation_id,
                    expected_promotion_id,
                    expected_product_revision,
                    expected_payload_digest,
                    expected_principal_id,
                    semantic_request_digest,
                    expected_cancellation_reason_digest,
                    expected_drain_intent_id,
                    expected_source_intent_revision,
                    expected_source_state_digest,
                    expected_product_operation_id,
                    expected_source_deployment_revision,
                    proposed_terminal_action_id,
                    slot_fence_row.writer_epoch,
                    terminal_microseconds::BIGINT,
                    source_snapshot_digest
                )::TEXT,
                'UTF8'
            )
        ),
        'hex'
    );
    computed_preparation_token :=
        'v2:' || preparation_binding_digest;

    BEGIN
        SELECT cancellation.*
        INTO cancellation_record_row
        FROM starring_runtime_private_v2.starring_product_lifecycle_cancellation_record_v2(
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
            expected_cancellation_reason,
            terminal_time
        ) AS cancellation;

        IF cancellation_record_row.outcome
                IS DISTINCT FROM 'ok'
            OR cancellation_record_row.resulting_revision
                IS DISTINCT FROM expected_product_revision
            OR cancellation_record_row.resulting_state
                IS DISTINCT FROM 'approved'
            OR cancellation_record_row.exact_replay
                IS DISTINCT FROM FALSE
            OR cancellation_record_row.guild_id
                IS DISTINCT FROM expected_guild_id
        THEN
            mutation_failure_outcome := CASE
                cancellation_record_row.outcome
                WHEN 'invalid_input' THEN 'invalid_input'
                WHEN 'not_found' THEN 'not_found'
                WHEN 'authorization_stale'
                    THEN 'authorization_stale'
                WHEN 'authority_mismatch'
                    THEN 'authority_mismatch'
                WHEN 'scope_mismatch' THEN 'scope_mismatch'
                WHEN 'payload_mismatch'
                    THEN 'payload_mismatch'
                WHEN 'revision_conflict'
                    THEN 'revision_conflict'
                WHEN 'idempotency_conflict'
                    THEN 'idempotency_conflict'
                WHEN 'idempotency_keyring_incomplete'
                    THEN 'idempotency_keyring_incomplete'
                WHEN 'expired' THEN 'authorization_stale'
                WHEN 'invalid_state'
                    THEN 'revision_conflict'
                WHEN 'indeterminate' THEN 'indeterminate'
                ELSE 'persistence_corrupt'
            END;
            RAISE EXCEPTION USING
                ERRCODE = 'RX005',
                MESSAGE =
                    'product_lifecycle_cancellation_record_failed';
        END IF;

        SELECT receipt.*
        INTO receipt_row
        FROM public.product_action_receipts AS receipt
        WHERE receipt.receipt_id = new_receipt_id
        FOR KEY SHARE;
        SELECT audit.*
        INTO audit_row
        FROM public.product_audit_events AS audit
        WHERE audit.event_id = new_audit_event_id
        FOR KEY SHARE;
        IF receipt_row.receipt_id IS NULL
            OR receipt_row.endpoint_domain <>
                'product_cancel_lifecycle_v1'
            OR receipt_row.request_digest <>
                semantic_request_digest
            OR receipt_row.resulting_revision IS DISTINCT FROM
                expected_product_revision
            OR receipt_row.resulting_state <> 'approved'
            OR receipt_row.result_code <>
                'runtime_drain_cancelled'
            OR receipt_row.completed_at <> terminal_time
            OR audit_row.event_id IS NULL
            OR audit_row.receipt_id <>
                receipt_row.receipt_id
            OR audit_row.action <>
                'promotion.cancel_lifecycle'
            OR audit_row.resulting_state <> 'approved'
            OR audit_row.result_code <>
                'runtime_drain_cancelled'
            OR audit_row.occurred_at <> terminal_time
        THEN
            mutation_failure_outcome :=
                'persistence_corrupt';
            RAISE EXCEPTION USING
                ERRCODE = 'RX005',
                MESSAGE =
                    'product_lifecycle_cancellation_evidence_invalid';
        END IF;

        IF NOT starring_runtime_private_v2.starring_product_apply_consume_preparation_reservation_v2(
            'prepare',
            computed_preparation_token,
            preparation_binding_digest,
            source_snapshot_digest,
            terminal_time
        )
        THEN
            mutation_failure_outcome := 'indeterminate';
            RAISE EXCEPTION USING
                ERRCODE = 'RX005',
                MESSAGE =
                    'product_lifecycle_cancellation_reservation_failed';
        END IF;

        SELECT source.*
        INTO STRICT source_result_row
        FROM starring_runtime_private_v2.starring_runtime_product_drain_cancel_source_v2(
            expected_drain_intent_id,
            locked_source_row.deployment_id,
            expected_source_deployment_revision,
            computed_preparation_token,
            preparation_binding_digest,
            source_snapshot_digest,
            terminal_time
        ) AS source;

        source_result_snapshot_bytes :=
            pg_catalog.convert_to(
                source_result_row.snapshot::TEXT,
                'UTF8'
            );
        source_result_snapshot_digest :=
            pg_catalog.encode(
                pg_catalog.sha256(
                    source_result_snapshot_bytes
                ),
                'hex'
            );

        terminal_drain_row :=
            starring_runtime_private_v2.starring_runtime_product_drain_terminal_transition_v2(
                expected_drain_intent_id,
                expected_source_intent_revision,
                expected_source_state_digest,
                'cancelled',
                source_result_row.revision,
                terminal_time
            );
        computed_successor_epoch :=
            starring_runtime_private_v2.starring_runtime_slot_writer_fence_terminal_release_v2(
                drain_row.slot_guild_id,
                drain_row.slot_ruleset_key,
                slot_fence_row.writer_epoch,
                drain_row.drain_intent_id,
                drain_row.product_operation_id,
                drain_row.tenant_id,
                drain_row.installation_id,
                drain_row.deployment_id,
                drain_row.expected_revision,
                expected_source_intent_revision,
                drain_row.canonical_state_bytes,
                expected_source_state_digest,
                terminal_drain_row.intent_revision,
                terminal_drain_row.canonical_state_digest,
                'cancelled',
                terminal_time
            );

        computed_terminal_projection :=
            starring_runtime_private_v2.starring_runtime_product_drain_terminal_projection_v2(
                'cancelled',
                proposed_terminal_action_id,
                active_idempotency_key_digest,
                semantic_request_digest,
                expected_cancellation_reason_digest,
                product_row.product_operation_id,
                product_row.product_mutation_digest,
                drain_row.drain_intent_id,
                drain_row.drain_intent_digest,
                expected_source_intent_revision,
                expected_source_state_digest,
                terminal_drain_row.intent_revision,
                terminal_drain_row.canonical_state_bytes,
                terminal_drain_row.canonical_state_digest,
                expected_source_deployment_revision,
                source_result_row.revision,
                source_result_snapshot_digest,
                NULL,
                NULL,
                NULL,
                slot_fence_row.writer_epoch,
                computed_successor_epoch,
                new_receipt_id,
                new_audit_event_id,
                expected_authority_observation_digest,
                expected_authority_revision,
                terminal_time
            );
        IF computed_terminal_projection IS NULL THEN
            mutation_failure_outcome :=
                'persistence_corrupt';
            RAISE EXCEPTION USING
                ERRCODE = 'RX005',
                MESSAGE =
                    'product_lifecycle_cancellation_projection_invalid';
        END IF;
        computed_terminal_projection_digest :=
            pg_catalog.encode(
                pg_catalog.sha256(
                    computed_terminal_projection
                ),
                'hex'
            );

        INSERT INTO public.runtime_product_drain_terminal_actions_v2 (
            terminal_action_id,
            terminal_kind,
            drain_intent_id,
            product_operation_id,
            product_mutation_digest,
            drain_intent_digest,
            product_action_idempotency_digest,
            product_action_semantic_request_digest,
            cancellation_reason_digest,
            source_intent_revision,
            source_canonical_state_digest,
            result_intent_revision,
            result_canonical_state_digest,
            source_deployment_revision,
            source_result_deployment_revision,
            source_result_deployment_snapshot_digest,
            source_result_deployment_snapshot_bytes,
            result_deployment_id,
            result_deployment_revision,
            result_deployment_snapshot_digest,
            result_deployment_snapshot_bytes,
            source_slot_writer_epoch,
            successor_slot_writer_epoch,
            terminal_database_time,
            product_receipt_id,
            product_audit_event_id,
            authority_observation_digest,
            installation_authority_revision,
            terminal_projection_bytes,
            terminal_projection_digest,
            source_deployment_snapshot_bytes,
            source_deployment_snapshot_digest,
            source_canonical_state_bytes
        ) VALUES (
            proposed_terminal_action_id,
            'cancelled',
            drain_row.drain_intent_id,
            product_row.product_operation_id,
            product_row.product_mutation_digest,
            drain_row.drain_intent_digest,
            active_idempotency_key_digest,
            semantic_request_digest,
            expected_cancellation_reason_digest,
            expected_source_intent_revision,
            expected_source_state_digest,
            terminal_drain_row.intent_revision,
            terminal_drain_row.canonical_state_digest,
            expected_source_deployment_revision,
            source_result_row.revision,
            source_result_snapshot_digest,
            source_result_snapshot_bytes,
            NULL,
            NULL,
            NULL,
            NULL,
            slot_fence_row.writer_epoch,
            computed_successor_epoch,
            terminal_time,
            new_receipt_id,
            new_audit_event_id,
            expected_authority_observation_digest,
            expected_authority_revision,
            computed_terminal_projection,
            computed_terminal_projection_digest,
            source_snapshot_bytes,
            source_snapshot_digest,
            drain_row.canonical_state_bytes
        )
        RETURNING * INTO action_row;

        IF NOT starring_runtime_private_v2.starring_runtime_product_drain_cancelled_terminal_exact_v2(
            action_row,
            product_row,
            terminal_drain_row
        )
        THEN
            mutation_failure_outcome :=
                'persistence_corrupt';
            RAISE EXCEPTION USING
                ERRCODE = 'RX005',
                MESSAGE =
                    'product_lifecycle_cancellation_terminal_invalid';
        END IF;

        SET CONSTRAINTS
            public.runtime_drain_intents_v2_assert_slot_writer_fence_symmetry,
            public.runtime_slot_writer_fences_v2_assert_pending_symmetry
        IMMEDIATE;
        SET CONSTRAINTS
            public.runtime_drain_intents_v2_assert_slot_writer_fence_symmetry,
            public.runtime_slot_writer_fences_v2_assert_pending_symmetry
        DEFERRED;
    EXCEPTION
        WHEN SQLSTATE 'RX005' THEN
            outcome_name := COALESCE(
                mutation_failure_outcome,
                'indeterminate'
            );
            exact_replay := FALSE;
            RETURN NEXT;
            RETURN;
        WHEN SQLSTATE 'RX002'
            OR SQLSTATE 'RX003'
            OR SQLSTATE 'RX004'
        THEN
            outcome_name := 'persistence_corrupt';
            exact_replay := FALSE;
            RETURN NEXT;
            RETURN;
    END;

    outcome_name := 'applied';
    exact_replay := FALSE;
    product_resulting_revision :=
        cancellation_record_row.resulting_revision;
    product_resulting_state :=
        cancellation_record_row.resulting_state;
    guild_id := cancellation_record_row.guild_id;
    product_receipt_id := receipt_row.receipt_id;
    product_audit_event_id := audit_row.event_id;
    cancellation_reason_digest :=
        expected_cancellation_reason_digest;
    product_operation_id :=
        product_row.product_operation_id;
    source_product_mutation_request_bytes :=
        product_row.product_mutation_request_bytes;
    product_mutation_digest :=
        product_row.product_mutation_digest;
    source_drain_intent_request_bytes :=
        drain_row.drain_intent_request_bytes;
    drain_intent_digest :=
        drain_row.drain_intent_digest;
    source_deployment_id :=
        locked_source_row.deployment_id;
    source_deployment_revision :=
        locked_source_row.revision;
    source_deployment_snapshot :=
        locked_source_row.snapshot;
    source_deployment_snapshot_digest :=
        source_snapshot_digest;
    source_result_deployment_revision :=
        source_result_row.revision;
    source_result_deployment_snapshot :=
        source_result_row.snapshot;
    source_result_deployment_snapshot_digest :=
        source_result_snapshot_digest;
    drain_intent_id := terminal_drain_row.drain_intent_id;
    source_intent_revision :=
        expected_source_intent_revision;
    source_state_bytes :=
        drain_row.canonical_state_bytes;
    source_state_digest :=
        expected_source_state_digest;
    result_intent_revision :=
        terminal_drain_row.intent_revision;
    result_intent_state :=
        terminal_drain_row.intent_state;
    result_state_bytes :=
        terminal_drain_row.canonical_state_bytes;
    result_state_digest :=
        terminal_drain_row.canonical_state_digest;
    source_slot_epoch :=
        slot_fence_row.writer_epoch;
    successor_slot_epoch :=
        computed_successor_epoch;
    terminal_action_id := action_row.terminal_action_id;
    terminal_projection_bytes :=
        action_row.terminal_projection_bytes;
    terminal_projection_digest :=
        action_row.terminal_projection_digest;
    terminal_database_time := terminal_time;
    RETURN NEXT;
END;
$function$;

DO $create_product_lifecycle_cancellation_record$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_product_reject_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text)'
    );

    previous_fragment :=
        'CREATE OR REPLACE FUNCTION public.starring_product_reject_v1';
    next_fragment :=
        'CREATE FUNCTION starring_runtime_private_v2.starring_product_lifecycle_cancellation_record_v2';
    IF definition IS NULL
        OR pg_catalog.strpos(definition, previous_fragment) <> 1
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'product_cancellation_record_source_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
    definition := pg_catalog.replace(
        definition,
        'expected_rejection_reason text)',
        'expected_cancellation_reason text, requested_terminal_time timestamp with time zone)'
    );
    definition := pg_catalog.replace(
        definition,
        'expected_rejection_reason',
        'expected_cancellation_reason'
    );
    definition := pg_catalog.replace(
        definition,
        'product_reject_v1',
        'product_cancel_lifecycle_v1'
    );
    definition := pg_catalog.replace(
        definition,
        'expected_capability <> ''reject''',
        'expected_capability <> ''cancel_lifecycle'''
    );
    previous_fragment :=
        '        OR expected_cancellation_reason ~ U&''[\0001-\001F\007F-\009F]''';
    next_fragment := previous_fragment || E'\n' ||
        '        OR NOT pg_catalog.isfinite(requested_terminal_time)';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'product_cancellation_record_input_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        E'\n' ||
        '    IF expected_product_revision = 9223372036854775807 THEN' || E'\n' ||
        '        RETURN QUERY SELECT ''invalid_state'', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;' || E'\n' ||
        '        RETURN;' || E'\n' ||
        '    END IF;' || E'\n';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'product_cancellation_record_revision_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        E'\n'
    );

    definition := pg_catalog.replace(
        definition,
        'expected_product_revision + 1',
        'expected_product_revision'
    );
    definition := pg_catalog.replace(
        definition,
        '''rejected''',
        '''approved'''
    );
    definition := pg_catalog.replace(
        definition,
        '''promotion_rejected''',
        '''runtime_drain_cancelled'''
    );
    definition := pg_catalog.replace(
        definition,
        '''promotion.reject''',
        '''promotion.cancel_lifecycle'''
    );
    definition := pg_catalog.replace(
        definition,
        'IF activation_row.state <> ''pending'' THEN',
        'IF activation_row.state <> ''approved'' THEN'
    );

    previous_fragment :=
        '            OR activation_row.rejected_by IS DISTINCT FROM expected_acting_user_id' || E'\n' ||
        '            OR activation_row.rejection_reason IS DISTINCT FROM expected_cancellation_reason' || E'\n' ||
        '            OR activation_row.rejected_at IS DISTINCT FROM receipt_row.completed_at' || E'\n';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'product_cancellation_record_replay_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        ''
    );

    previous_fragment :=
        E'\n' ||
        '    IF activation_row.expires_at <= mutation_clock THEN' || E'\n' ||
        '        RETURN QUERY SELECT ''expired'', NULL::BIGINT, NULL::TEXT, FALSE, NULL::TEXT;' || E'\n' ||
        '        RETURN;' || E'\n' ||
        '    END IF;';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'product_cancellation_record_expiry_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        ''
    );

    previous_fragment :=
        '    next_revision := activation_row.product_revision + 1;' || E'\n' ||
        E'\n' ||
        '    PERFORM pg_catalog.set_config(' || E'\n' ||
        '        ''starring.product_rejection_gate'',' || E'\n' ||
        '        activation_row.approval_context_digest,' || E'\n' ||
        '        TRUE' || E'\n' ||
        '    );' || E'\n' ||
        E'\n' ||
        '    UPDATE public.activation_requests AS activation' || E'\n' ||
        '    SET state = ''approved'',' || E'\n' ||
        '        product_revision = next_revision,' || E'\n' ||
        '        rejected_at = mutation_clock,' || E'\n' ||
        '        rejected_by = expected_acting_user_id,' || E'\n' ||
        '        rejection_reason = expected_cancellation_reason' || E'\n' ||
        '    WHERE activation.id = activation_row.id' || E'\n' ||
        '        AND activation.state = ''pending''' || E'\n' ||
        '        AND activation.product_revision = expected_product_revision;' || E'\n' ||
        '    IF NOT FOUND THEN' || E'\n' ||
        '        RAISE EXCEPTION ''product rejection activation compare-and-swap failed''' || E'\n' ||
        '            USING ERRCODE = ''40001'';' || E'\n' ||
        '    END IF;' || E'\n' ||
        E'\n' ||
        '    PERFORM pg_catalog.set_config(''starring.product_rejection_gate'', '''', TRUE);';
    next_fragment :=
        '    mutation_clock := requested_terminal_time;' || E'\n' ||
        '    next_revision := activation_row.product_revision;';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'product_cancellation_record_mutation_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    IF pg_catalog.strpos(
            definition,
            'activation_row.state <> ''approved'''
        ) = 0
        OR pg_catalog.strpos(
            definition,
            '''product_cancel_lifecycle_v1'''
        ) = 0
        OR pg_catalog.strpos(
            definition,
            '''promotion.cancel_lifecycle'''
        ) = 0
        OR pg_catalog.strpos(
            definition,
            '''runtime_drain_cancelled'''
        ) = 0
        OR pg_catalog.strpos(
            definition,
            'starring.product_rejection_gate'
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'product_cancellation_record_result_drift';
    END IF;
    EXECUTE definition;
END;
$create_product_lifecycle_cancellation_record$;

CREATE FUNCTION public.starring_product_lifecycle_cancellation_executor_database_identity_v1()
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

CREATE FUNCTION public.starring_product_lifecycle_cancellation_keyring_coverage_v1(
    idempotency_digest_key_id_candidates TEXT[],
    idempotency_digest_key_fingerprint_candidates TEXT[]
)
RETURNS TABLE(outcome TEXT)
LANGUAGE plpgsql
VOLATILE
STRICT
SECURITY DEFINER
PARALLEL UNSAFE
ROWS 1
SET search_path = pg_catalog
AS $function$
BEGIN
    RETURN QUERY SELECT CASE
        WHEN pg_catalog.array_ndims(
                idempotency_digest_key_id_candidates
            ) IS DISTINCT FROM 1
            OR pg_catalog.array_lower(
                idempotency_digest_key_id_candidates,
                1
            ) IS DISTINCT FROM 1
            OR pg_catalog.cardinality(
                idempotency_digest_key_id_candidates
            ) NOT BETWEEN 1 AND 8
            OR pg_catalog.array_ndims(
                idempotency_digest_key_fingerprint_candidates
            ) IS DISTINCT FROM 1
            OR pg_catalog.array_lower(
                idempotency_digest_key_fingerprint_candidates,
                1
            ) IS DISTINCT FROM 1
            OR pg_catalog.cardinality(
                idempotency_digest_key_fingerprint_candidates
            ) IS DISTINCT FROM pg_catalog.cardinality(
                idempotency_digest_key_id_candidates
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.generate_subscripts(
                    idempotency_digest_key_id_candidates,
                    1
                ) AS candidate(ordinal)
                WHERE idempotency_digest_key_id_candidates[
                        candidate.ordinal
                    ] !~ '^[A-Za-z0-9_.:-]{1,64}$'
                    OR idempotency_digest_key_fingerprint_candidates[
                        candidate.ordinal
                    ] !~ '^[0-9a-f]{64}$'
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
                    ON left_candidate.ordinal <
                        right_candidate.ordinal
                WHERE idempotency_digest_key_id_candidates[
                        left_candidate.ordinal
                    ] = idempotency_digest_key_id_candidates[
                        right_candidate.ordinal
                    ]
                    OR idempotency_digest_key_fingerprint_candidates[
                        left_candidate.ordinal
                    ] = idempotency_digest_key_fingerprint_candidates[
                        right_candidate.ordinal
                    ]
            )
        THEN 'invalid_input'
        WHEN EXISTS (
            SELECT 1
            FROM public.product_action_receipts AS receipt
            WHERE receipt.endpoint_domain =
                    'product_cancel_lifecycle_v1'
                AND NOT EXISTS (
                    SELECT 1
                    FROM public.product_action_receipt_idempotency_aliases
                        AS alias
                    WHERE alias.tenant_id = receipt.tenant_id
                        AND alias.installation_id =
                            receipt.installation_id
                        AND alias.principal_id =
                            receipt.principal_id
                        AND alias.endpoint_domain =
                            receipt.endpoint_domain
                        AND alias.receipt_id = receipt.receipt_id
                        AND EXISTS (
                            SELECT 1
                            FROM pg_catalog.generate_subscripts(
                                idempotency_digest_key_id_candidates,
                                1
                            ) AS candidate(ordinal)
                            WHERE idempotency_digest_key_id_candidates[
                                    candidate.ordinal
                                ] = alias.idempotency_digest_key_id
                                AND idempotency_digest_key_fingerprint_candidates[
                                    candidate.ordinal
                                ] = alias.idempotency_digest_key_fingerprint
                        )
                )
        )
        THEN 'idempotency_keyring_incomplete'
        ELSE 'ok'
    END;
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_product_lifecycle_cancellation_unkeyed_digest_v2(
    requested_domain TEXT,
    requested_fields TEXT[]
)
RETURNS TEXT
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    payload BYTEA;
    field_value TEXT;
    field_bytes BYTEA;
BEGIN
    IF requested_domain !~
            '^[A-Za-z0-9_.:-]{1,128}$'
        OR pg_catalog.array_ndims(requested_fields)
            IS DISTINCT FROM 1
        OR pg_catalog.array_lower(requested_fields, 1)
            IS DISTINCT FROM 1
        OR pg_catalog.cardinality(requested_fields)
            NOT BETWEEN 1 AND 32
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.unnest(requested_fields)
                AS candidate(value)
            WHERE candidate.value IS NULL
                OR pg_catalog.octet_length(candidate.value) > 8192
        )
    THEN
        RETURN NULL;
    END IF;
    field_bytes := pg_catalog.convert_to(
        requested_domain,
        'UTF8'
    );
    payload :=
        pg_catalog.int8send(
            pg_catalog.octet_length(field_bytes)::BIGINT
        )
        || field_bytes;
    FOREACH field_value IN ARRAY requested_fields
    LOOP
        field_bytes := pg_catalog.convert_to(
            field_value,
            'UTF8'
        );
        payload := payload
            || pg_catalog.int8send(
                pg_catalog.octet_length(field_bytes)::BIGINT
            )
            || field_bytes;
    END LOOP;
    RETURN pg_catalog.encode(
        pg_catalog.sha256(payload),
        'hex'
    );
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_product_drain_cancelled_terminal_exact_v2(
    action_row public.runtime_product_drain_terminal_actions_v2,
    product_row public.runtime_product_operations_v2,
    drain_row public.runtime_drain_intents_v2
)
RETURNS BOOLEAN
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    source_snapshot JSONB;
    result_snapshot JSONB;
    source_drain_row public.runtime_drain_intents_v2%ROWTYPE;
BEGIN
    IF NOT starring_runtime_private_v2.starring_runtime_product_drain_terminal_action_exact_v2(
        action_row,
        product_row,
        drain_row
    )
    THEN
        RETURN FALSE;
    END IF;
    IF action_row.terminal_kind = 'consumed' THEN
        RETURN action_row.source_deployment_snapshot_bytes IS NULL
            AND action_row.source_deployment_snapshot_digest IS NULL
            AND action_row.source_canonical_state_bytes IS NULL;
    END IF;
    source_snapshot := pg_catalog.convert_from(
        action_row.source_deployment_snapshot_bytes,
        'UTF8'
    )::JSONB;
    result_snapshot := pg_catalog.convert_from(
        action_row.source_result_deployment_snapshot_bytes,
        'UTF8'
    )::JSONB;
    source_drain_row := drain_row;
    source_drain_row.intent_revision :=
        action_row.source_intent_revision;
    source_drain_row.intent_state :=
        'route_absent_acknowledged';
    source_drain_row.canonical_state_bytes :=
        action_row.source_canonical_state_bytes;
    source_drain_row.canonical_state_digest :=
        action_row.source_canonical_state_digest;
    RETURN action_row.terminal_kind = 'cancelled'
        AND action_row.source_deployment_snapshot_digest =
            pg_catalog.encode(
                pg_catalog.sha256(
                    action_row.source_deployment_snapshot_bytes
                ),
                'hex'
            )
        AND source_snapshot #>> '{identity,tenant_id}' =
            product_row.tenant_id
        AND source_snapshot #>> '{identity,installation_id}' =
            product_row.installation_id
        AND source_snapshot #>> '{identity,deployment_id}' =
            product_row.deployment_id
        AND (source_snapshot ->> 'revision')::NUMERIC
            IS NOT DISTINCT FROM
                action_row.source_deployment_revision
        AND result_snapshot IS NOT DISTINCT FROM
            pg_catalog.jsonb_set(
                source_snapshot,
                '{revision}',
                pg_catalog.to_jsonb(
                    action_row.source_result_deployment_revision
                ),
                FALSE
            )
        AND starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
            source_drain_row
        );
EXCEPTION
    WHEN OTHERS THEN
        RETURN FALSE;
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_product_drain_cancel_source_v2(
    requested_drain_intent_id TEXT,
    requested_source_deployment_id TEXT,
    requested_source_deployment_revision BIGINT,
    requested_preparation_token TEXT,
    requested_binding_digest TEXT,
    requested_locked_projection_digest TEXT,
    requested_terminal_time TIMESTAMPTZ
)
RETURNS public.runtime_deployments
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    source_row public.runtime_deployments%ROWTYPE;
    result_row public.runtime_deployments%ROWTYPE;
    drain_row public.runtime_drain_intents_v2%ROWTYPE;
    result_snapshot JSONB;
BEGIN
    IF NOT starring_runtime_private_v2.starring_product_apply_consume_preparation_reservation_v2(
        'commit',
        requested_preparation_token,
        requested_binding_digest,
        requested_locked_projection_digest,
        requested_terminal_time
    )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE =
                'runtime_product_drain_cancel_reservation_invalid';
    END IF;

    SELECT deployment.*
    INTO source_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.deployment_id =
            requested_source_deployment_id
        AND deployment.revision =
            requested_source_deployment_revision
    FOR UPDATE;

    SELECT drain.*
    INTO drain_row
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE drain.drain_intent_id =
            requested_drain_intent_id
    FOR KEY SHARE;

    result_snapshot := pg_catalog.jsonb_set(
        source_row.snapshot,
        '{revision}',
        pg_catalog.to_jsonb(source_row.revision + 1),
        FALSE
    );

    IF source_row.deployment_id IS NULL
        OR drain_row.drain_intent_id IS NULL
        OR source_row.revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR source_row.phase
            NOT IN ('awaiting_gateway_ready', 'live')
        OR source_row.controller_id IS NOT NULL
        OR source_row.controller_fencing_token IS NOT NULL
        OR source_row.controller_acquired_at IS NOT NULL
        OR source_row.controller_lease_expires_at IS NOT NULL
        OR source_row.tenant_id <> drain_row.tenant_id
        OR source_row.installation_id <>
            drain_row.installation_id
        OR source_row.deployment_id <>
            drain_row.deployment_id
        OR source_row.guild_id <> drain_row.slot_guild_id
        OR source_row.ruleset_key <>
            drain_row.slot_ruleset_key
        OR source_row.revision <> drain_row.expected_revision
        OR drain_row.intent_state <>
            'route_absent_acknowledged'
        OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
            drain_row
        )
        OR requested_terminal_time <= source_row.updated_at
        OR requested_locked_projection_digest <>
            pg_catalog.encode(
                pg_catalog.sha256(
                    pg_catalog.convert_to(
                        source_row.snapshot::TEXT,
                        'UTF8'
                    )
                ),
                'hex'
            )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE =
                'runtime_product_drain_cancel_source_stale';
    END IF;

    PERFORM pg_catalog.set_config(
        'starring.runtime_mutation_clock',
        requested_terminal_time::TEXT,
        TRUE
    );

    UPDATE public.runtime_deployments AS deployment
    SET snapshot = result_snapshot,
        revision = source_row.revision + 1,
        updated_at = requested_terminal_time
    WHERE deployment.deployment_id =
            requested_source_deployment_id
        AND deployment.revision =
            requested_source_deployment_revision
        AND deployment.phase = source_row.phase
    RETURNING deployment.* INTO result_row;

    PERFORM pg_catalog.set_config(
        'starring.runtime_mutation_clock',
        '',
        TRUE
    );

    IF result_row.deployment_id IS NULL
        OR result_row.revision <> source_row.revision + 1
        OR result_row.phase <> source_row.phase
        OR result_row.updated_at <> requested_terminal_time
        OR pg_catalog.to_jsonb(result_row)
            - ARRAY['snapshot', 'revision', 'updated_at']
            IS DISTINCT FROM
                pg_catalog.to_jsonb(source_row)
                - ARRAY['snapshot', 'revision', 'updated_at']
        OR result_row.snapshot IS DISTINCT FROM result_snapshot
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE =
                'runtime_product_drain_cancel_source_result_invalid';
    END IF;
    RETURN result_row;
EXCEPTION
    WHEN OTHERS THEN
        PERFORM pg_catalog.set_config(
            'starring.runtime_mutation_clock',
            '',
            TRUE
        );
        RAISE;
END;
$function$;

DO $seal_product_lifecycle_cancellation_acl_final$
DECLARE
    identity TEXT;
BEGIN
    FOREACH identity IN ARRAY ARRAY[
        'public.starring_product_lifecycle_cancellation_executor_database_identity_v1()',
        'public.starring_product_lifecycle_cancellation_keyring_coverage_v1(text[],text[])',
        'public.starring_product_cancel_runtime_drain_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text,text,bigint,text,text,bigint)',
        'starring_runtime_private_v2.starring_runtime_product_drain_cancel_root_exact_v2(public.runtime_product_operations_v2,public.runtime_drain_intents_v2,public.runtime_deployments,text,text,bigint,text)',
        'starring_runtime_private_v2.starring_product_lifecycle_cancellation_record_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,timestamp with time zone)',
        'starring_runtime_private_v2.starring_product_lifecycle_cancellation_unkeyed_digest_v2(text,text[])',
        'starring_runtime_private_v2.starring_runtime_product_drain_cancelled_terminal_exact_v2(public.runtime_product_drain_terminal_actions_v2,public.runtime_product_operations_v2,public.runtime_drain_intents_v2)',
        'starring_runtime_private_v2.starring_runtime_product_drain_cancel_source_v2(text,text,bigint,text,text,text,timestamp with time zone)'
    ]
    LOOP
        IF pg_catalog.to_regprocedure(identity) IS NULL THEN
            RAISE EXCEPTION USING
                ERRCODE = 'PA001',
                MESSAGE =
                    'product_lifecycle_cancellation_acl_function_missing';
        END IF;
        EXECUTE pg_catalog.format(
            'REVOKE ALL ON FUNCTION %s FROM PUBLIC',
            identity
        );
    END LOOP;
END;
$seal_product_lifecycle_cancellation_acl_final$;

DO $postflight$
DECLARE
    common_owner OID;
    invalid_public_count BIGINT;
    invalid_private_count BIGINT;
    invalid_public_acl_count BIGINT;
    cancellation_grantee_count BIGINT;
    cancellation_grantee OID;
    invalid_executor_count BIGINT;
    invalid_constraint_count BIGINT;
    invalid_terminal_count BIGINT;
    execution_manifest_digest TEXT;
    execution_readiness_digest TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );

    SELECT pg_catalog.count(*)
    INTO invalid_public_count
    FROM (
        VALUES
            (
                'public.starring_product_lifecycle_cancellation_executor_database_identity_v1()',
                FALSE,
                0::REAL,
                'text'
            ),
            (
                'public.starring_product_lifecycle_cancellation_keyring_coverage_v1(text[],text[])',
                TRUE,
                1::REAL,
                'TABLE(outcome text)'
            ),
            (
                'public.starring_product_cancel_runtime_drain_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text,text,bigint,text,text,bigint)',
                TRUE,
                1::REAL,
                'TABLE(outcome_name text, exact_replay boolean, product_resulting_revision bigint, product_resulting_state text, guild_id text, product_receipt_id text, product_audit_event_id text, cancellation_reason_digest text, product_operation_id text, source_product_mutation_request_bytes bytea, product_mutation_digest text, source_drain_intent_request_bytes bytea, drain_intent_digest text, source_deployment_id text, source_deployment_revision bigint, source_deployment_snapshot jsonb, source_deployment_snapshot_digest text, source_result_deployment_revision bigint, source_result_deployment_snapshot jsonb, source_result_deployment_snapshot_digest text, drain_intent_id text, source_intent_revision bigint, source_state_bytes bytea, source_state_digest text, result_intent_revision bigint, result_intent_state text, result_state_bytes bytea, result_state_digest text, source_slot_epoch bigint, successor_slot_epoch bigint, terminal_action_id text, terminal_projection_bytes bytea, terminal_projection_digest text, terminal_database_time timestamp with time zone)'
            )
    ) AS expected(
        identity,
        returns_set,
        rows_estimate,
        result_identity
    )
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid =
            pg_catalog.to_regprocedure(expected.identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR NOT function_row.proisstrict
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR function_row.proretset <>
            expected.returns_set
        OR function_row.prorows <>
            expected.rows_estimate
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR function_row.proconfig IS DISTINCT FROM
            ARRAY['search_path=pg_catalog']::TEXT[]
        OR language_row.lanname NOT IN ('sql', 'plpgsql')
        OR pg_catalog.pg_get_function_result(
            function_row.oid
        ) IS DISTINCT FROM expected.result_identity;

    SELECT pg_catalog.count(*)
    INTO invalid_private_count
    FROM (
        VALUES
            ('starring_runtime_private_v2.starring_runtime_product_drain_cancel_root_exact_v2(public.runtime_product_operations_v2,public.runtime_drain_intents_v2,public.runtime_deployments,text,text,bigint,text)'),
            ('starring_runtime_private_v2.starring_product_lifecycle_cancellation_record_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,timestamp with time zone)'),
            ('starring_runtime_private_v2.starring_product_lifecycle_cancellation_unkeyed_digest_v2(text,text[])'),
            ('starring_runtime_private_v2.starring_runtime_product_drain_cancelled_terminal_exact_v2(public.runtime_product_drain_terminal_actions_v2,public.runtime_product_operations_v2,public.runtime_drain_intents_v2)'),
            ('starring_runtime_private_v2.starring_runtime_product_drain_cancel_source_v2(text,text,bigint,text,text,text,timestamp with time zone)')
    ) AS expected(identity)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid =
            pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR NOT function_row.proisstrict
        OR function_row.proleakproof
        OR function_row.pronargdefaults <> 0
        OR function_row.provariadic <> 0
        OR function_row.proconfig IS DISTINCT FROM
            ARRAY['search_path=pg_catalog']::TEXT[]
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault(
                    'f',
                    function_row.proowner
                )
            )) AS privilege
            WHERE privilege.grantee <>
                function_row.proowner
        );

    WITH cancellation_functions(identity) AS (
        VALUES
            ('public.starring_product_lifecycle_cancellation_executor_database_identity_v1()'),
            ('public.starring_product_lifecycle_cancellation_keyring_coverage_v1(text[],text[])'),
            ('public.starring_product_cancel_runtime_drain_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text,text,bigint,text,text,bigint)')
    ), grants AS (
        SELECT
            expected.identity,
            privilege.grantee,
            privilege.grantor,
            privilege.privilege_type,
            privilege.is_grantable
        FROM cancellation_functions AS expected
        INNER JOIN pg_catalog.pg_proc AS function_row
            ON function_row.oid =
                pg_catalog.to_regprocedure(
                    expected.identity
                )
        CROSS JOIN LATERAL pg_catalog.aclexplode(
            COALESCE(
                function_row.proacl,
                pg_catalog.acldefault(
                    'f',
                    function_row.proowner
                )
            )
        ) AS privilege
        WHERE privilege.grantee <> common_owner
    )
    SELECT
        pg_catalog.count(*) FILTER (
            WHERE grantee = 0
                OR grantor <> common_owner
                OR privilege_type <> 'EXECUTE'
                OR is_grantable
        ),
        pg_catalog.count(DISTINCT grantee),
        pg_catalog.min(grantee::BIGINT)::OID
    INTO
        invalid_public_acl_count,
        cancellation_grantee_count,
        cancellation_grantee
    FROM grants;

    IF cancellation_grantee_count = 1 THEN
        SELECT pg_catalog.count(*)
        INTO invalid_executor_count
        FROM pg_catalog.pg_roles AS role_row
        WHERE role_row.oid = cancellation_grantee
            AND (
                role_row.rolsuper
                OR role_row.rolinherit
                OR role_row.rolcreaterole
                OR role_row.rolcreatedb
                OR role_row.rolreplication
                OR role_row.rolbypassrls
                OR role_row.oid = common_owner
                OR EXISTS (
                    SELECT 1
                    FROM pg_catalog.pg_auth_members
                        AS membership
                    WHERE membership.member =
                            role_row.oid
                        OR membership.roleid =
                            role_row.oid
                )
            );
    ELSE
        invalid_executor_count := CASE
            WHEN cancellation_grantee_count = 0 THEN 0
            ELSE 1
        END;
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_constraint_count
    FROM pg_catalog.pg_constraint AS constraint_row
    WHERE constraint_row.conrelid =
            pg_catalog.to_regclass(
                'public.runtime_product_drain_terminal_actions_v2'
            )
        AND constraint_row.conname =
            'runtime_product_drain_terminal_actions_v2_source_snapshot_check'
        AND (
            constraint_row.contype <> 'c'
            OR NOT constraint_row.convalidated
            OR constraint_row.condeferrable
            OR constraint_row.condeferred
            OR constraint_row.conparentid <> 0
        );
    IF invalid_constraint_count = 0
        AND NOT EXISTS (
            SELECT 1
            FROM pg_catalog.pg_constraint AS constraint_row
            WHERE constraint_row.conrelid =
                    pg_catalog.to_regclass(
                        'public.runtime_product_drain_terminal_actions_v2'
                    )
                AND constraint_row.conname =
                    'runtime_product_drain_terminal_actions_v2_source_snapshot_check'
        )
    THEN
        invalid_constraint_count := 1;
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_terminal_count
    FROM public.runtime_product_drain_terminal_actions_v2
        AS action
    WHERE (
            action.terminal_kind = 'consumed'
            AND (
                action.source_deployment_snapshot_bytes
                    IS NOT NULL
                OR action.source_deployment_snapshot_digest
                    IS NOT NULL
                OR action.source_canonical_state_bytes
                    IS NOT NULL
            )
        )
        OR (
            action.terminal_kind = 'cancelled'
            AND NOT starring_runtime_private_v2.starring_runtime_product_drain_cancelled_terminal_exact_v2(
                action,
                (
                    SELECT product
                    FROM public.runtime_product_operations_v2
                        AS product
                    WHERE product.product_operation_id =
                        action.product_operation_id
                ),
                (
                    SELECT drain
                    FROM public.runtime_drain_intents_v2
                        AS drain
                    WHERE drain.drain_intent_id =
                        action.drain_intent_id
                )
            )
        );

    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_execution_schema_manifest_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO execution_manifest_digest;
    SELECT pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_execution_database_readiness_v1()'
                    )
                ),
                'UTF8'
            )
        ),
        'hex'
    )
    INTO execution_readiness_digest;

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <>
            common_owner
        OR invalid_public_count <> 0
        OR invalid_private_count <> 0
        OR invalid_public_acl_count <> 0
        OR invalid_executor_count <> 0
        OR invalid_constraint_count <> 0
        OR invalid_terminal_count <> 0
        OR NOT public.starring_runtime_execution_schema_manifest_v1()
        OR NOT public.starring_runtime_exact_target_schema_manifest_v1()
        OR NOT public.starring_runtime_serving_schema_manifest_v1()
        OR execution_manifest_digest <>
            'b7ee8d2a13ae38a88bc1b2558b018e74893e7d90ccd72d96187197a111432e22'
        OR execution_readiness_digest <>
            '3fe2924d130e93d630960be796e3986884fefedddfb91c0dd5b680a41b440cb1'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'PA001',
            MESSAGE =
                'product_lifecycle_cancellation_postflight_drift',
            DETAIL = pg_catalog.format(
                'public=%s private=%s public_acl=%s executor=%s constraint=%s terminal=%s manifest=%s readiness=%s',
                invalid_public_count,
                invalid_private_count,
                invalid_public_acl_count,
                invalid_executor_count,
                invalid_constraint_count,
                invalid_terminal_count,
                execution_manifest_digest,
                execution_readiness_digest
            );
    END IF;
END;
$postflight$;

RESET search_path;
