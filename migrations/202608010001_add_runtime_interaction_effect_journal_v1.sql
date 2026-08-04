SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';
SET LOCAL search_path = pg_catalog;

LOCK TABLE
    public.runtime_interaction_receipt_roots_v1,
    public.runtime_interaction_receipt_heads_v1,
    public.runtime_interaction_receipt_events_v1,
    public.runtime_interaction_receipt_token_secrets_v1,
    public.automation_installations,
    public.runtime_deployments,
    public.runtime_attestations,
    public.runtime_serving_leases,
    public.runtime_gateway_owners,
    public.automation_ruleset_versions,
    public.automation_instances
IN ACCESS EXCLUSIVE MODE;

DO $preflight$
DECLARE
    common_owner OID;
    collision_count BIGINT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
            'public.runtime_interaction_receipt_roots_v1'
        )
        AND relation.relkind = 'r'
        AND relation.relpersistence = 'p'
        AND NOT relation.relrowsecurity
        AND NOT relation.relforcerowsecurity;

    IF NOT FOUND
        OR common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR NOT public.starring_runtime_interaction_receipt_schema_manifest_v1()
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
        OR pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_receipt_claim_current_v1(text,text,bigint,text,timestamp with time zone)'
        ) IS NULL
        OR pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_receipt_authority_observe_v1(text,text,text,text,text,text,bigint,text,bigint,text,bigint,bigint,bigint,text,text,text,text,text)'
        ) IS NULL
        OR EXISTS (
            SELECT 1
            FROM (
                VALUES
                    ('public.runtime_interaction_receipt_heads_v1'),
                    ('public.runtime_interaction_receipt_events_v1'),
                    ('public.runtime_interaction_receipt_token_secrets_v1'),
                    ('public.automation_installations'),
                    ('public.runtime_deployments'),
                    ('public.runtime_attestations'),
                    ('public.runtime_serving_leases'),
                    ('public.runtime_gateway_owners'),
                    ('public.automation_ruleset_versions'),
                    ('public.automation_instances')
            ) AS expected(identity)
            LEFT JOIN pg_catalog.pg_class AS relation
                ON relation.oid = pg_catalog.to_regclass(expected.identity)
            WHERE relation.oid IS NULL
                OR relation.relowner <> common_owner
                OR relation.relkind <> 'r'
                OR relation.relpersistence <> 'p'
                OR relation.relrowsecurity
                OR relation.relforcerowsecurity
        )
    THEN
        RAISE EXCEPTION 'runtime interaction effect preflight failed'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_class AS relation
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = relation.relnamespace
    WHERE namespace.nspname = 'public'
        AND relation.relname IN (
            'runtime_interaction_effect_roots_v1',
            'runtime_interaction_effect_rollbacks_v1',
            'runtime_interaction_effect_heads_v1',
            'runtime_interaction_effect_events_v1',
            'runtime_interaction_effect_heads_recovery_v1_idx',
            'runtime_interaction_effect_heads_route_unsafe_v1_idx',
            'runtime_interaction_effect_rollbacks_required_v1_idx',
            'runtime_interaction_effect_heads_correlation_v1_idx',
            'runtime_interaction_receipt_roots_effect_static_route_v1_idx',
            'runtime_interaction_receipt_roots_effect_instance_route_v1_idx'
        );

    IF collision_count <> 0 THEN
        RAISE EXCEPTION 'runtime interaction effect relation collision exists'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'guard_runtime_interaction_effect_root_v1',
            'guard_runtime_interaction_effect_rollback_v1',
            'guard_runtime_interaction_effect_head_v1',
            'guard_runtime_interaction_effect_event_v1',
            'guard_runtime_interaction_effect_response_token_delete_v1',
            'starring_runtime_interaction_effect_receipt_terminal_sync_v1',
            'starring_runtime_interaction_effect_complete_receipt_v1',
            'starring_runtime_interaction_effect_resolve_receipt_v1',
            'starring_runtime_interaction_effect_require_rollback_v1',
            'starring_runtime_interaction_effect_try_complete_rollback_v1',
            'starring_runtime_interaction_effect_schema_manifest_v1',
            'starring_runtime_interaction_effect_plan_bind_v1',
            'starring_runtime_interaction_effect_intend_v1',
            'starring_runtime_interaction_effect_finish_v1',
            'starring_runtime_interaction_effect_scan_recoverable_v1',
            'starring_runtime_interaction_effect_recovery_claim_v1',
            'starring_runtime_interaction_effect_reconcile_v1',
            'starring_runtime_interaction_effect_compensation_intend_v1',
            'starring_runtime_interaction_effect_compensation_finish_v1',
            'starring_runtime_interaction_effect_response_tail_scan_v1',
            'starring_runtime_interaction_effect_response_tail_claim_v1',
            'starring_runtime_interaction_effect_response_tail_finalize_v1'
        );

    IF collision_count <> 0 THEN
        RAISE EXCEPTION 'runtime interaction effect function collision exists'
            USING ERRCODE = '55000';
    END IF;
END;
$preflight$;

CREATE TABLE public.runtime_interaction_effect_roots_v1 (
    application_id TEXT NOT NULL,
    interaction_id TEXT NOT NULL,
    record_format_version SMALLINT NOT NULL,
    action_plan_digest BYTEA NOT NULL,
    preflight_certificate_digest BYTEA NOT NULL,
    snapshot_digest BYTEA NOT NULL,
    action_count SMALLINT NOT NULL,
    certificate_issued_at TIMESTAMPTZ NOT NULL,
    certificate_expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT runtime_interaction_effect_roots_v1_pk PRIMARY KEY (
        application_id,
        interaction_id
    ),
    CONSTRAINT runtime_interaction_effect_roots_v1_receipt_fk FOREIGN KEY (
        application_id,
        interaction_id
    ) REFERENCES public.runtime_interaction_receipt_roots_v1 (
        application_id,
        interaction_id
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_interaction_effect_roots_v1_identity_check CHECK (
        application_id ~ '^[1-9][0-9]{0,19}$'
        AND pg_catalog.length(application_id) <= 20
        AND (
            pg_catalog.length(application_id) < 20
            OR application_id <= '18446744073709551615'
        )
        AND interaction_id ~ '^[1-9][0-9]{0,19}$'
        AND pg_catalog.length(interaction_id) <= 20
        AND (
            pg_catalog.length(interaction_id) < 20
            OR interaction_id <= '18446744073709551615'
        )
    ),
    CONSTRAINT runtime_interaction_effect_roots_v1_plan_check CHECK (
        record_format_version = 1
        AND pg_catalog.octet_length(action_plan_digest) = 32
        AND pg_catalog.octet_length(preflight_certificate_digest) = 32
        AND pg_catalog.octet_length(snapshot_digest) = 32
        AND action_count BETWEEN 0 AND 256
    ),
    CONSTRAINT runtime_interaction_effect_roots_v1_time_check CHECK (
        pg_catalog.isfinite(certificate_issued_at)
        AND pg_catalog.isfinite(certificate_expires_at)
        AND pg_catalog.isfinite(created_at)
        AND certificate_issued_at <= created_at
        AND created_at < certificate_expires_at
        AND certificate_expires_at
            <= certificate_issued_at + INTERVAL '5 minutes'
    )
);

CREATE TABLE public.runtime_interaction_effect_rollbacks_v1 (
    application_id TEXT NOT NULL,
    interaction_id TEXT NOT NULL,
    abort_action_index SMALLINT NOT NULL,
    abort_reason TEXT NOT NULL,
    state TEXT NOT NULL,
    revision BIGINT NOT NULL,
    required_at TIMESTAMPTZ NOT NULL,
    completed_at TIMESTAMPTZ,
    CONSTRAINT runtime_interaction_effect_rollbacks_v1_pk PRIMARY KEY (
        application_id,
        interaction_id
    ),
    CONSTRAINT runtime_interaction_effect_rollbacks_v1_root_fk FOREIGN KEY (
        application_id,
        interaction_id
    ) REFERENCES public.runtime_interaction_effect_roots_v1 (
        application_id,
        interaction_id
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_interaction_effect_rollbacks_v1_identity_check CHECK (
        application_id ~ '^[1-9][0-9]{0,19}$'
        AND pg_catalog.length(application_id) <= 20
        AND (
            pg_catalog.length(application_id) < 20
            OR application_id <= '18446744073709551615'
        )
        AND interaction_id ~ '^[1-9][0-9]{0,19}$'
        AND pg_catalog.length(interaction_id) <= 20
        AND (
            pg_catalog.length(interaction_id) < 20
            OR interaction_id <= '18446744073709551615'
        )
        AND abort_action_index BETWEEN 0 AND 255
    ),
    CONSTRAINT runtime_interaction_effect_rollbacks_v1_state_check CHECK (
        abort_reason IN (
            'definitive_failure',
            'indeterminate',
            'observation_abort',
            'recovery_required',
            'response_failure'
        )
        AND (
            (
                state = 'required'
                AND revision = 1
                AND completed_at IS NULL
            )
            OR (
                state = 'completed'
                AND revision = 2
                AND completed_at IS NOT NULL
                AND required_at <= completed_at
            )
        )
    ),
    CONSTRAINT runtime_interaction_effect_rollbacks_v1_time_check CHECK (
        pg_catalog.isfinite(required_at)
        AND (
            completed_at IS NULL
            OR pg_catalog.isfinite(completed_at)
        )
    )
);

CREATE TABLE public.runtime_interaction_effect_heads_v1 (
    application_id TEXT NOT NULL,
    interaction_id TEXT NOT NULL,
    action_index SMALLINT NOT NULL,
    action_kind TEXT NOT NULL,
    dependency_indices SMALLINT[] NOT NULL,
    planned_identity_digest BYTEA NOT NULL,
    input_digest BYTEA NOT NULL,
    expected_postimage_digest BYTEA NOT NULL,
    planned_recovery_input JSONB NOT NULL,
    planned_preimage_digest BYTEA NOT NULL,
    planned_preimage JSONB NOT NULL,
    resolved_input JSONB,
    resolved_preimage_digest BYTEA,
    resolved_preimage JSONB,
    resolved_effect_identity_digest BYTEA,
    resolved_instance_manifest_digest BYTEA,
    output_kind TEXT NOT NULL,
    correlation_class TEXT NOT NULL,
    correlation_digest BYTEA NOT NULL,
    correlation_marker TEXT,
    state TEXT NOT NULL,
    head_revision BIGINT NOT NULL,
    attempt_count INTEGER NOT NULL,
    observation_attempt_count INTEGER NOT NULL,
    compensation_attempt_count INTEGER NOT NULL,
    compensation_observation_attempt_count INTEGER NOT NULL,
    intent_process_instance_id TEXT,
    intent_receipt_claim_revision BIGINT,
    intent_digest BYTEA,
    intent_at TIMESTAMPTZ,
    result_digest BYTEA,
    output_id TEXT,
    result_at TIMESTAMPTZ,
    success_binding_kind TEXT,
    success_binding_digest BYTEA,
    recovery_claim_revision BIGINT NOT NULL,
    recovery_process_instance_id TEXT,
    recovery_gateway_shard_id TEXT,
    recovery_runtime_build_revision TEXT,
    recovery_acquired_at TIMESTAMPTZ,
    recovery_expires_at TIMESTAMPTZ,
    next_recovery_at TIMESTAMPTZ,
    compensation_intent_digest BYTEA,
    compensation_intent_at TIMESTAMPTZ,
    compensation_result_digest BYTEA,
    compensation_result_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT runtime_interaction_effect_heads_v1_pk PRIMARY KEY (
        application_id,
        interaction_id,
        action_index
    ),
    CONSTRAINT runtime_interaction_effect_heads_v1_root_fk FOREIGN KEY (
        application_id,
        interaction_id
    ) REFERENCES public.runtime_interaction_effect_roots_v1 (
        application_id,
        interaction_id
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_interaction_effect_heads_v1_action_check CHECK (
        action_index BETWEEN 0 AND 255
        AND action_kind IN (
            'create_role',
            'create_channel',
            'grant_role',
            'upsert_overwrite',
            'post_panel',
            'register_instance',
            'teardown_instance',
            'edit_response'
        )
        AND pg_catalog.array_ndims(dependency_indices) = 1
        AND pg_catalog.array_lower(dependency_indices, 1) = 1
        AND pg_catalog.cardinality(dependency_indices) <= 32
        AND pg_catalog.array_position(dependency_indices, NULL) IS NULL
        AND (
            pg_catalog.cardinality(dependency_indices) = 0
            OR (
                0 <= ALL(dependency_indices)
                AND action_index > ALL(dependency_indices)
            )
        )
        AND pg_catalog.octet_length(planned_identity_digest) = 32
        AND pg_catalog.octet_length(input_digest) = 32
        AND pg_catalog.octet_length(expected_postimage_digest) = 32
        AND pg_catalog.jsonb_typeof(planned_recovery_input) = 'object'
        AND pg_catalog.octet_length(planned_recovery_input::TEXT)
            BETWEEN 2 AND 4096
        AND pg_catalog.octet_length(planned_preimage_digest) = 32
        AND pg_catalog.jsonb_typeof(planned_preimage) = 'object'
        AND pg_catalog.octet_length(planned_preimage::TEXT)
            BETWEEN 2 AND 4096
        AND (
            (
                resolved_input IS NULL
                AND resolved_preimage_digest IS NULL
                AND resolved_preimage IS NULL
                AND resolved_effect_identity_digest IS NULL
                AND resolved_instance_manifest_digest IS NULL
            )
            OR (
                pg_catalog.jsonb_typeof(resolved_input) = 'object'
                AND pg_catalog.octet_length(resolved_input::TEXT)
                    BETWEEN 2 AND 4096
                AND pg_catalog.octet_length(resolved_preimage_digest) = 32
                AND pg_catalog.jsonb_typeof(resolved_preimage) = 'object'
                AND pg_catalog.octet_length(resolved_preimage::TEXT)
                    BETWEEN 2 AND 4096
                AND pg_catalog.octet_length(
                    resolved_effect_identity_digest
                ) = 32
                AND (
                    (
                        action_kind = 'register_instance'
                        AND pg_catalog.octet_length(
                            resolved_instance_manifest_digest
                        ) = 32
                    )
                    OR (
                        action_kind <> 'register_instance'
                        AND resolved_instance_manifest_digest IS NULL
                    )
                )
            )
        )
    ),
    CONSTRAINT runtime_interaction_effect_heads_v1_output_check CHECK (
        output_kind IN (
            'created_role',
            'created_channel',
            'role_membership',
            'permission_overwrite',
            'posted_message',
            'instance_state',
            'original_response'
        )
        AND (
            output_id IS NULL
            OR (
                output_kind IN (
                    'created_role',
                    'created_channel',
                    'posted_message'
                )
                AND output_id ~ '^[1-9][0-9]{0,19}$'
                AND pg_catalog.length(output_id) <= 20
                AND (
                    pg_catalog.length(output_id) < 20
                    OR output_id <= '18446744073709551615'
                )
            )
            OR (
                output_kind = 'instance_state'
                AND output_id ~ '^[A-Za-z0-9_-]{1,32}$'
            )
        )
        AND (
            output_kind NOT IN (
                'role_membership',
                'permission_overwrite',
                'original_response'
            )
            OR output_id IS NULL
        )
    ),
    CONSTRAINT runtime_interaction_effect_heads_v1_correlation_check CHECK (
        correlation_class IN (
            'audit_log_reason',
            'message_nonce',
            'internal_idempotency_key',
            'interaction_receipt',
            'unsupported'
        )
        AND pg_catalog.octet_length(correlation_digest) = 32
        AND (
            (
                correlation_class = 'audit_log_reason'
                AND action_kind IN (
                    'create_role',
                    'create_channel',
                    'grant_role',
                    'upsert_overwrite'
                )
                AND correlation_marker IS NOT NULL
                AND correlation_marker ~ '^[0-9a-f]{64}$'
            )
            OR (
                action_kind = 'post_panel'
                AND correlation_class = 'message_nonce'
                AND correlation_marker IS NOT NULL
                AND correlation_marker ~ '^[1-9][0-9]{0,19}$'
                AND (
                    pg_catalog.length(correlation_marker) < 20
                    OR correlation_marker <= '18446744073709551615'
                )
            )
            OR (
                action_kind = 'post_panel'
                AND correlation_class = 'unsupported'
                AND correlation_marker IS NULL
            )
            OR (
                action_kind IN ('register_instance', 'teardown_instance')
                AND correlation_class = 'internal_idempotency_key'
                AND correlation_marker IS NOT NULL
                AND correlation_marker ~ '^[0-9a-f]{64}$'
            )
            OR (
                action_kind = 'edit_response'
                AND correlation_class = 'interaction_receipt'
                AND correlation_marker IS NULL
            )
        )
    ),
    CONSTRAINT runtime_interaction_effect_heads_v1_state_check CHECK (
        state IN (
            'planned',
            'intended',
            'known_succeeded',
            'known_failed',
            'indeterminate',
            'observing',
            'observation_pending',
            'reconciled_succeeded',
            'compensation_intended',
            'compensated',
            'compensation_indeterminate',
            'compensation_observing',
            'compensation_observation_pending',
            'recovery_required'
        )
    ),
    CONSTRAINT runtime_interaction_effect_heads_v1_revision_check CHECK (
        head_revision BETWEEN 1 AND 9223372036854775807
        AND attempt_count BETWEEN 0 AND 64
        AND observation_attempt_count BETWEEN 0 AND 64
        AND compensation_attempt_count BETWEEN 0 AND 64
        AND compensation_observation_attempt_count BETWEEN 0 AND 64
        AND recovery_claim_revision BETWEEN 0 AND 9223372036854775807
    ),
    CONSTRAINT runtime_interaction_effect_heads_v1_intent_check CHECK (
        (
            intent_process_instance_id IS NULL
            AND intent_receipt_claim_revision IS NULL
            AND intent_digest IS NULL
            AND intent_at IS NULL
        )
        OR (
            intent_process_instance_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
            AND intent_receipt_claim_revision
                BETWEEN 1 AND 9223372036854775807
            AND pg_catalog.octet_length(intent_digest) = 32
            AND pg_catalog.isfinite(intent_at)
        )
    ),
    CONSTRAINT runtime_interaction_effect_heads_v1_result_check CHECK (
        (
            result_digest IS NULL
            AND result_at IS NULL
        )
        OR (
            pg_catalog.octet_length(result_digest) = 32
            AND pg_catalog.isfinite(result_at)
        )
    ),
    CONSTRAINT runtime_interaction_effect_heads_v1_success_binding_check CHECK (
        (
            success_binding_kind IS NULL
            AND success_binding_digest IS NULL
        )
        OR (
            success_binding_kind IN ('attempt_result', 'observation')
            AND pg_catalog.octet_length(success_binding_digest) = 32
        )
    ),
    CONSTRAINT runtime_interaction_effect_heads_v1_recovery_check CHECK (
        (
            recovery_process_instance_id IS NULL
            AND recovery_gateway_shard_id IS NULL
            AND recovery_runtime_build_revision IS NULL
            AND recovery_acquired_at IS NULL
            AND recovery_expires_at IS NULL
        )
        OR (
            recovery_claim_revision BETWEEN 1 AND 9223372036854775807
            AND recovery_process_instance_id
                ~ '^[A-Za-z0-9_.:-]{1,128}$'
            AND recovery_gateway_shard_id
                ~ '^[A-Za-z0-9_.:/-]{1,128}$'
            AND recovery_runtime_build_revision
                ~ '^[A-Za-z0-9_.:/-]{1,128}$'
            AND pg_catalog.isfinite(recovery_acquired_at)
            AND pg_catalog.isfinite(recovery_expires_at)
            AND recovery_acquired_at < recovery_expires_at
            AND recovery_expires_at
                <= recovery_acquired_at + INTERVAL '5 minutes'
        )
        AND (
            next_recovery_at IS NULL
            OR pg_catalog.isfinite(next_recovery_at)
        )
    ),
    CONSTRAINT runtime_interaction_effect_heads_v1_compensation_check CHECK (
        (
            compensation_intent_digest IS NULL
            AND compensation_intent_at IS NULL
        )
        OR (
            pg_catalog.octet_length(compensation_intent_digest) = 32
            AND pg_catalog.isfinite(compensation_intent_at)
        )
        AND (
            compensation_result_digest IS NULL
            AND compensation_result_at IS NULL
            OR pg_catalog.octet_length(compensation_result_digest) = 32
                AND pg_catalog.isfinite(compensation_result_at)
        )
    ),
    CONSTRAINT runtime_interaction_effect_heads_v1_state_shape_check CHECK (
        (
            state = 'planned'
            AND attempt_count = 0
            AND observation_attempt_count = 0
            AND compensation_attempt_count = 0
            AND compensation_observation_attempt_count = 0
            AND intent_digest IS NULL
            AND resolved_input IS NULL
            AND resolved_preimage_digest IS NULL
            AND resolved_preimage IS NULL
            AND resolved_effect_identity_digest IS NULL
            AND resolved_instance_manifest_digest IS NULL
            AND result_digest IS NULL
            AND success_binding_kind IS NULL
            AND success_binding_digest IS NULL
            AND output_id IS NULL
            AND recovery_process_instance_id IS NULL
            AND next_recovery_at IS NULL
            AND compensation_intent_digest IS NULL
            AND compensation_result_digest IS NULL
        )
        OR (
            state = 'intended'
            AND attempt_count > 0
            AND intent_digest IS NOT NULL
            AND resolved_input IS NOT NULL
            AND resolved_preimage_digest IS NOT NULL
            AND resolved_preimage IS NOT NULL
            AND resolved_effect_identity_digest IS NOT NULL
            AND result_digest IS NULL
            AND success_binding_kind IS NULL
            AND success_binding_digest IS NULL
            AND recovery_process_instance_id IS NULL
            AND next_recovery_at IS NOT NULL
            AND compensation_intent_digest IS NULL
        )
        OR (
            state IN (
                'known_succeeded',
                'known_failed',
                'reconciled_succeeded'
            )
            AND intent_digest IS NOT NULL
            AND resolved_input IS NOT NULL
            AND resolved_preimage_digest IS NOT NULL
            AND resolved_preimage IS NOT NULL
            AND resolved_effect_identity_digest IS NOT NULL
            AND recovery_process_instance_id IS NULL
            AND next_recovery_at IS NULL
            AND compensation_intent_digest IS NULL
            AND (
                (
                    state = 'known_succeeded'
                    AND result_digest IS NOT NULL
                    AND success_binding_kind = 'attempt_result'
                    AND success_binding_digest = result_digest
                )
                OR (
                    state = 'reconciled_succeeded'
                    AND success_binding_kind = 'observation'
                    AND success_binding_digest IS NOT NULL
                )
                OR (
                    state = 'known_failed'
                    AND result_digest IS NOT NULL
                    AND success_binding_kind IS NULL
                    AND success_binding_digest IS NULL
                )
            )
            AND (
                state = 'known_failed'
                OR (
                    output_kind IN (
                        'created_role',
                        'created_channel',
                        'posted_message',
                        'instance_state'
                    )
                    AND output_id IS NOT NULL
                )
                OR (
                    output_kind IN (
                        'role_membership',
                        'permission_overwrite',
                        'original_response'
                    )
                    AND output_id IS NULL
                )
            )
        )
        OR (
            state IN ('indeterminate', 'observation_pending')
            AND intent_digest IS NOT NULL
            AND resolved_input IS NOT NULL
            AND resolved_preimage_digest IS NOT NULL
            AND resolved_preimage IS NOT NULL
            AND resolved_effect_identity_digest IS NOT NULL
            AND result_digest IS NOT NULL
            AND success_binding_kind IS NULL
            AND success_binding_digest IS NULL
            AND recovery_process_instance_id IS NULL
            AND next_recovery_at IS NOT NULL
            AND compensation_intent_digest IS NULL
        )
        OR (
            state = 'observing'
            AND intent_digest IS NOT NULL
            AND resolved_input IS NOT NULL
            AND resolved_preimage_digest IS NOT NULL
            AND resolved_preimage IS NOT NULL
            AND resolved_effect_identity_digest IS NOT NULL
            AND success_binding_kind IS NULL
            AND success_binding_digest IS NULL
            AND recovery_process_instance_id IS NOT NULL
            AND next_recovery_at = recovery_expires_at
            AND compensation_intent_digest IS NULL
        )
        OR (
            state = 'compensation_intended'
            AND intent_digest IS NOT NULL
            AND resolved_input IS NOT NULL
            AND resolved_preimage_digest IS NOT NULL
            AND resolved_preimage IS NOT NULL
            AND resolved_effect_identity_digest IS NOT NULL
            AND (
                (
                    success_binding_kind = 'attempt_result'
                    AND result_digest IS NOT NULL
                    AND success_binding_digest = result_digest
                )
                OR (
                    success_binding_kind = 'observation'
                    AND success_binding_digest IS NOT NULL
                )
            )
            AND recovery_process_instance_id IS NOT NULL
            AND next_recovery_at = recovery_expires_at
            AND compensation_intent_digest IS NOT NULL
            AND compensation_result_digest IS NULL
        )
        OR (
            state = 'compensated'
            AND intent_digest IS NOT NULL
            AND resolved_input IS NOT NULL
            AND resolved_preimage_digest IS NOT NULL
            AND resolved_preimage IS NOT NULL
            AND resolved_effect_identity_digest IS NOT NULL
            AND (
                (
                    success_binding_kind = 'attempt_result'
                    AND result_digest IS NOT NULL
                    AND success_binding_digest = result_digest
                )
                OR (
                    success_binding_kind = 'observation'
                    AND success_binding_digest IS NOT NULL
                )
            )
            AND recovery_process_instance_id IS NULL
            AND next_recovery_at IS NULL
            AND compensation_intent_digest IS NOT NULL
            AND compensation_result_digest IS NOT NULL
        )
        OR (
            state = 'compensation_indeterminate'
            AND intent_digest IS NOT NULL
            AND resolved_input IS NOT NULL
            AND resolved_preimage_digest IS NOT NULL
            AND resolved_preimage IS NOT NULL
            AND resolved_effect_identity_digest IS NOT NULL
            AND (
                (
                    success_binding_kind = 'attempt_result'
                    AND result_digest IS NOT NULL
                    AND success_binding_digest = result_digest
                )
                OR (
                    success_binding_kind = 'observation'
                    AND success_binding_digest IS NOT NULL
                )
            )
            AND recovery_process_instance_id IS NULL
            AND next_recovery_at IS NOT NULL
            AND compensation_intent_digest IS NOT NULL
            AND compensation_result_digest IS NOT NULL
        )
        OR (
            state = 'compensation_observing'
            AND intent_digest IS NOT NULL
            AND resolved_input IS NOT NULL
            AND resolved_preimage_digest IS NOT NULL
            AND resolved_preimage IS NOT NULL
            AND resolved_effect_identity_digest IS NOT NULL
            AND (
                (
                    success_binding_kind = 'attempt_result'
                    AND result_digest IS NOT NULL
                    AND success_binding_digest = result_digest
                )
                OR (
                    success_binding_kind = 'observation'
                    AND success_binding_digest IS NOT NULL
                )
            )
            AND recovery_process_instance_id IS NOT NULL
            AND next_recovery_at = recovery_expires_at
            AND compensation_intent_digest IS NOT NULL
        )
        OR (
            state = 'compensation_observation_pending'
            AND intent_digest IS NOT NULL
            AND resolved_input IS NOT NULL
            AND resolved_preimage_digest IS NOT NULL
            AND resolved_preimage IS NOT NULL
            AND resolved_effect_identity_digest IS NOT NULL
            AND (
                (
                    success_binding_kind = 'attempt_result'
                    AND result_digest IS NOT NULL
                    AND success_binding_digest = result_digest
                )
                OR (
                    success_binding_kind = 'observation'
                    AND success_binding_digest IS NOT NULL
                )
            )
            AND recovery_process_instance_id IS NULL
            AND next_recovery_at IS NOT NULL
            AND compensation_intent_digest IS NOT NULL
        )
        OR (
            state = 'recovery_required'
            AND intent_digest IS NOT NULL
            AND resolved_input IS NOT NULL
            AND resolved_preimage_digest IS NOT NULL
            AND resolved_preimage IS NOT NULL
            AND resolved_effect_identity_digest IS NOT NULL
            AND (
                result_digest IS NOT NULL
                OR (
                    success_binding_kind = 'observation'
                    AND success_binding_digest IS NOT NULL
                )
            )
            AND recovery_process_instance_id IS NULL
            AND next_recovery_at IS NULL
        )
    ),
    CONSTRAINT runtime_interaction_effect_heads_v1_time_check CHECK (
        pg_catalog.isfinite(updated_at)
        AND (intent_at IS NULL OR intent_at <= updated_at)
        AND (result_at IS NULL OR result_at <= updated_at)
        AND (
            compensation_intent_at IS NULL
            OR compensation_intent_at <= updated_at
        )
        AND (
            compensation_result_at IS NULL
            OR compensation_result_at <= updated_at
        )
    )
);

CREATE TABLE public.runtime_interaction_effect_events_v1 (
    application_id TEXT NOT NULL,
    interaction_id TEXT NOT NULL,
    action_index SMALLINT NOT NULL,
    event_revision BIGINT NOT NULL,
    event_kind TEXT NOT NULL,
    from_state TEXT,
    to_state TEXT NOT NULL,
    receipt_claim_revision BIGINT,
    recovery_claim_revision BIGINT NOT NULL,
    process_instance_id TEXT,
    outcome_code TEXT NOT NULL,
    result_digest BYTEA NOT NULL,
    output_kind TEXT NOT NULL,
    output_id TEXT,
    event_digest BYTEA NOT NULL,
    observed_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT runtime_interaction_effect_events_v1_pk PRIMARY KEY (
        application_id,
        interaction_id,
        action_index,
        event_revision
    ),
    CONSTRAINT runtime_interaction_effect_events_v1_head_fk FOREIGN KEY (
        application_id,
        interaction_id,
        action_index
    ) REFERENCES public.runtime_interaction_effect_heads_v1 (
        application_id,
        interaction_id,
        action_index
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_interaction_effect_events_v1_revision_check CHECK (
        action_index BETWEEN 0 AND 255
        AND event_revision BETWEEN 1 AND 9223372036854775807
        AND (
            receipt_claim_revision IS NULL
            OR receipt_claim_revision BETWEEN 1 AND 9223372036854775807
        )
        AND recovery_claim_revision BETWEEN 0 AND 9223372036854775807
    ),
    CONSTRAINT runtime_interaction_effect_events_v1_state_check CHECK (
        event_kind IN (
            'planned',
            'intended',
            'known_succeeded',
            'known_failed',
            'indeterminate',
            'recovery_claimed',
            'reconciled_success',
            'reconciled_failure',
            'recovery_deferred',
            'recovery_required',
            'compensation_intended',
            'compensated',
            'compensation_indeterminate',
            'compensation_observation_claimed',
            'compensation_observation_deferred'
        )
        AND (
            from_state IS NULL
            OR from_state IN (
                'planned',
                'intended',
                'known_succeeded',
                'known_failed',
                'indeterminate',
                'observing',
                'observation_pending',
                'reconciled_succeeded',
                'recovery_required',
                'compensation_intended',
                'compensated',
                'compensation_indeterminate',
                'compensation_observing',
                'compensation_observation_pending'
            )
        )
        AND to_state IN (
            'planned',
            'intended',
            'known_succeeded',
            'known_failed',
            'indeterminate',
            'observing',
            'observation_pending',
            'reconciled_succeeded',
            'recovery_required',
            'compensation_intended',
            'compensated',
            'compensation_indeterminate',
            'compensation_observing',
            'compensation_observation_pending'
        )
    ),
    CONSTRAINT runtime_interaction_effect_events_v1_process_check CHECK (
        process_instance_id IS NULL
        OR process_instance_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT runtime_interaction_effect_events_v1_result_check CHECK (
        outcome_code ~ '^[a-z0-9_]{1,64}$'
        AND pg_catalog.octet_length(result_digest) = 32
        AND output_kind IN (
            'created_role',
            'created_channel',
            'role_membership',
            'permission_overwrite',
            'posted_message',
            'instance_state',
            'original_response'
        )
        AND (
            output_id IS NULL
            OR (
                output_kind IN (
                    'created_role',
                    'created_channel',
                    'posted_message'
                )
                AND output_id ~ '^[1-9][0-9]{0,19}$'
                AND pg_catalog.length(output_id) <= 20
            )
            OR (
                output_kind = 'instance_state'
                AND output_id ~ '^[A-Za-z0-9_-]{1,32}$'
            )
        )
        AND pg_catalog.octet_length(event_digest) = 32
        AND pg_catalog.isfinite(observed_at)
    )
);

CREATE INDEX runtime_interaction_effect_heads_recovery_v1_idx
ON public.runtime_interaction_effect_heads_v1 USING btree (
    next_recovery_at,
    application_id COLLATE "C",
    interaction_id COLLATE "C",
    action_index
)
WHERE action_kind <> 'edit_response'
AND state IN (
    'intended',
    'indeterminate',
    'observing',
    'observation_pending',
    'compensation_intended',
    'compensation_indeterminate',
    'compensation_observing',
    'compensation_observation_pending'
);

CREATE INDEX runtime_interaction_effect_heads_route_unsafe_v1_idx
ON public.runtime_interaction_effect_heads_v1 USING btree (
    application_id COLLATE "C",
    interaction_id COLLATE "C",
    action_index
)
WHERE action_kind <> 'edit_response'
AND state IN (
    'intended',
    'indeterminate',
    'observing',
    'observation_pending',
    'compensation_intended',
    'compensation_indeterminate',
    'compensation_observing',
    'compensation_observation_pending',
    'recovery_required'
);

CREATE INDEX runtime_interaction_effect_rollbacks_required_v1_idx
ON public.runtime_interaction_effect_rollbacks_v1 USING btree (
    required_at,
    application_id COLLATE "C",
    interaction_id COLLATE "C",
    abort_action_index
)
WHERE state = 'required';

CREATE UNIQUE INDEX runtime_interaction_effect_heads_correlation_v1_idx
ON public.runtime_interaction_effect_heads_v1 USING btree (
    application_id COLLATE "C",
    correlation_class COLLATE "C",
    correlation_marker COLLATE "C"
)
WHERE correlation_marker IS NOT NULL;

CREATE INDEX runtime_interaction_receipt_roots_effect_static_route_v1_idx
ON public.runtime_interaction_receipt_roots_v1 USING btree (
    application_id COLLATE "C",
    guild_id COLLATE "C",
    ruleset_key COLLATE "C",
    route_key COLLATE "C",
    interaction_id COLLATE "C"
)
WHERE route_kind = 'static';

CREATE INDEX runtime_interaction_receipt_roots_effect_instance_route_v1_idx
ON public.runtime_interaction_receipt_roots_v1 USING btree (
    application_id COLLATE "C",
    guild_id COLLATE "C",
    ruleset_key COLLATE "C",
    instance_id COLLATE "C",
    interaction_id COLLATE "C"
)
WHERE route_kind = 'instance';

CREATE FUNCTION public.starring_runtime_interaction_effect_schema_manifest_v1()
RETURNS BOOLEAN
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    RETURN TRUE;
END;
$function$;


DO $receipt_admission_extension$
DECLARE
    function_definition TEXT;
    admission_contract TEXT;
    admission_replacement TEXT;
BEGIN
    function_definition := pg_catalog.pg_get_functiondef(
        pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_receipt_claim_v1(text,text,text,text,text,text,text,text,text,text,bigint,text,bigint,text,bigint,bigint,bigint,text,text,text,text,text,text,text,bigint,bigint,bigint,bigint,bigint,text,text,bytea,bigint,text,smallint,text,bytea,bytea,bytea,timestamp with time zone,timestamp with time zone)'
        )
    );
    admission_contract := $needle$    IF proposed_token_issued_at > database_now + INTERVAL '5 seconds'$needle$;
    admission_replacement := $needle$    IF EXISTS (
        SELECT 1
        FROM (
            SELECT effect.application_id, effect.interaction_id
            FROM public.runtime_interaction_effect_heads_v1 AS effect
            INNER JOIN public.runtime_interaction_receipt_roots_v1
                AS blocked_root
                ON blocked_root.application_id = effect.application_id
                AND blocked_root.interaction_id = effect.interaction_id
            WHERE effect.application_id = expected_application_id
                AND effect.action_kind <> 'edit_response'
                AND effect.state IN (
                    'intended',
                    'indeterminate',
                    'observing',
                    'observation_pending',
                    'compensation_intended',
                    'compensation_indeterminate',
                    'compensation_observing',
                    'compensation_observation_pending',
                    'recovery_required'
                )
                AND blocked_root.guild_id = expected_guild_id
                AND blocked_root.ruleset_key = expected_ruleset_key
                AND blocked_root.interaction_id <> expected_interaction_id
                AND (
                    (
                        expected_route_kind = 'static'
                        AND blocked_root.route_kind = 'static'
                        AND blocked_root.route_key = expected_route_key
                    )
                    OR (
                        expected_route_kind = 'instance'
                        AND blocked_root.route_kind = 'instance'
                        AND blocked_root.instance_id = expected_instance_id
                    )
                )
            UNION ALL
            SELECT effect.application_id, effect.interaction_id
            FROM public.runtime_interaction_effect_rollbacks_v1 AS rollback
            INNER JOIN public.runtime_interaction_effect_heads_v1 AS effect
                ON effect.application_id = rollback.application_id
                AND effect.interaction_id = rollback.interaction_id
                AND effect.action_index <= rollback.abort_action_index
            INNER JOIN public.runtime_interaction_receipt_roots_v1
                AS blocked_root
                ON blocked_root.application_id = effect.application_id
                AND blocked_root.interaction_id = effect.interaction_id
            WHERE rollback.state = 'required'
                AND effect.application_id = expected_application_id
                AND effect.action_kind <> 'edit_response'
                AND effect.state IN (
                    'known_succeeded',
                    'reconciled_succeeded'
                )
                AND blocked_root.guild_id = expected_guild_id
                AND blocked_root.ruleset_key = expected_ruleset_key
                AND blocked_root.interaction_id <> expected_interaction_id
                AND (
                    (
                        expected_route_kind = 'static'
                        AND blocked_root.route_kind = 'static'
                        AND blocked_root.route_key = expected_route_key
                    )
                    OR (
                        expected_route_kind = 'instance'
                        AND blocked_root.route_kind = 'instance'
                        AND blocked_root.instance_id = expected_instance_id
                    )
                )
        ) AS blocked
    )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_effect_route_recovery_blocked';
    END IF;

    IF proposed_token_issued_at > database_now + INTERVAL '5 seconds'$needle$;

    IF function_definition IS NULL
        OR pg_catalog.strpos(function_definition, admission_contract) = 0
        OR pg_catalog.strpos(
            pg_catalog.substr(
                function_definition,
                pg_catalog.strpos(function_definition, admission_contract)
                    + pg_catalog.length(admission_contract)
            ),
            admission_contract
        ) <> 0
    THEN
        RAISE EXCEPTION 'runtime interaction effect admission extension failed'
            USING ERRCODE = '55000';
    END IF;

    function_definition := pg_catalog.replace(
        function_definition,
        admission_contract,
        admission_replacement
    );
    EXECUTE function_definition;
END;
$receipt_admission_extension$;

CREATE FUNCTION public.starring_runtime_interaction_effect_intend_v1(
    expected_application_id TEXT,
    expected_interaction_id TEXT,
    expected_receipt_head_revision BIGINT,
    expected_receipt_claim_revision BIGINT,
    expected_process_instance_id TEXT,
    expected_preflight_certificate_digest BYTEA,
    expected_action_index BIGINT,
    expected_effect_head_revision BIGINT,
    proposed_intent_digest BYTEA,
    proposed_resolved_effect_identity_digest BYTEA,
    proposed_resolved_instance_manifest_digest BYTEA,
    proposed_resolved_input JSONB,
    proposed_resolved_preimage_digest BYTEA,
    proposed_resolved_preimage JSONB,
    requested_recovery_delay_milliseconds BIGINT
)
RETURNS TABLE(
    outcome_name TEXT,
    effect_state TEXT,
    resulting_effect_head_revision BIGINT,
    resulting_recovery_at TIMESTAMPTZ,
    observed_database_now TIMESTAMPTZ
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
    receipt_root public.runtime_interaction_receipt_roots_v1%ROWTYPE;
    receipt_head public.runtime_interaction_receipt_heads_v1%ROWTYPE;
    effect_root public.runtime_interaction_effect_roots_v1%ROWTYPE;
    effect_head public.runtime_interaction_effect_heads_v1%ROWTYPE;
    planned_reference RECORD;
    resolved_reference JSONB;
    expected_reference_slots TEXT[];
    observed_reference_slots TEXT[];
    object_key_count BIGINT;
    matching_count BIGINT;
    normalized_instance_manifest_digest BYTEA;
    database_now TIMESTAMPTZ;
    recovery_at TIMESTAMPTZ;
BEGIN
    IF expected_application_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_application_id) > 20
        OR expected_interaction_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_interaction_id) > 20
        OR expected_receipt_head_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_receipt_claim_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR pg_catalog.octet_length(
            expected_preflight_certificate_digest
        ) <> 32
        OR expected_action_index NOT BETWEEN 0 AND 255
        OR expected_effect_head_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR pg_catalog.octet_length(proposed_intent_digest) <> 32
        OR pg_catalog.octet_length(
            proposed_resolved_effect_identity_digest
        ) <> 32
        OR pg_catalog.octet_length(
            proposed_resolved_instance_manifest_digest
        ) NOT IN (0, 32)
        OR pg_catalog.jsonb_typeof(proposed_resolved_input) <> 'object'
        OR pg_catalog.octet_length(proposed_resolved_input::TEXT)
            NOT BETWEEN 2 AND 4096
        OR pg_catalog.octet_length(
            proposed_resolved_preimage_digest
        ) <> 32
        OR pg_catalog.jsonb_typeof(proposed_resolved_preimage) <> 'object'
        OR pg_catalog.octet_length(proposed_resolved_preimage::TEXT)
            NOT BETWEEN 2 AND 4096
        OR requested_recovery_delay_milliseconds NOT BETWEEN 1000 AND 60000
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_effect_intend_input_invalid';
    END IF;

    database_now := pg_catalog.clock_timestamp();
    normalized_instance_manifest_digest := NULLIF(
        proposed_resolved_instance_manifest_digest,
        ''::BYTEA
    );
    recovery_at := database_now
        + requested_recovery_delay_milliseconds * INTERVAL '1 millisecond';

    SELECT root.*
    INTO receipt_root
    FROM public.runtime_interaction_receipt_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_receipt_not_found';
    END IF;

    SELECT head.*
    INTO receipt_head
    FROM public.runtime_interaction_receipt_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
    FOR SHARE;

    IF NOT FOUND
        OR receipt_head.head_revision <> expected_receipt_head_revision
        OR receipt_head.claim_revision <> expected_receipt_claim_revision
        OR receipt_head.claim_process_instance_id
            IS DISTINCT FROM expected_process_instance_id
        OR receipt_head.state <> 'executing'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_intend_receipt_conflict';
    END IF;

    IF NOT public.starring_runtime_interaction_receipt_claim_current_v1(
        expected_application_id,
        expected_interaction_id,
        expected_receipt_claim_revision,
        expected_process_instance_id,
        database_now
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_effect_receipt_claim_stale';
    END IF;

    SELECT root.*
    INTO effect_root
    FROM public.runtime_interaction_effect_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    IF NOT FOUND
        OR effect_root.preflight_certificate_digest
            IS DISTINCT FROM expected_preflight_certificate_digest
        OR effect_root.action_plan_digest
            IS DISTINCT FROM receipt_head.action_plan_digest
        OR effect_root.certificate_expires_at <= database_now
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_effect_certificate_stale';
    END IF;

    SELECT head.*
    INTO effect_head
    FROM public.runtime_interaction_effect_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
        AND head.action_index = expected_action_index
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_action_not_found';
    END IF;

    IF (
            effect_head.action_kind = 'register_instance'
            AND pg_catalog.octet_length(
                normalized_instance_manifest_digest
            ) IS DISTINCT FROM 32
        )
        OR (
            effect_head.action_kind <> 'register_instance'
            AND normalized_instance_manifest_digest IS NOT NULL
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_effect_resolved_instance_manifest_invalid';
    END IF;

    IF effect_head.state = 'intended'
        AND effect_head.head_revision IN (
            expected_effect_head_revision,
            expected_effect_head_revision + 1
        )
        AND effect_head.intent_process_instance_id
            IS NOT DISTINCT FROM expected_process_instance_id
        AND effect_head.intent_receipt_claim_revision
            IS NOT DISTINCT FROM expected_receipt_claim_revision
        AND effect_head.intent_digest
            IS NOT DISTINCT FROM proposed_intent_digest
        AND effect_head.resolved_effect_identity_digest
            IS NOT DISTINCT FROM proposed_resolved_effect_identity_digest
        AND effect_head.resolved_instance_manifest_digest
            IS NOT DISTINCT FROM normalized_instance_manifest_digest
        AND effect_head.resolved_input
            IS NOT DISTINCT FROM proposed_resolved_input
        AND effect_head.resolved_preimage_digest
            IS NOT DISTINCT FROM proposed_resolved_preimage_digest
        AND effect_head.resolved_preimage
            IS NOT DISTINCT FROM proposed_resolved_preimage
    THEN
        outcome_name := 'exact_replay';
        effect_state := effect_head.state;
        resulting_effect_head_revision := effect_head.head_revision;
        resulting_recovery_at := effect_head.next_recovery_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            'starring-runtime-interaction-effect-route-v1:'
                || pg_catalog.jsonb_build_array(
                    receipt_root.application_id,
                    receipt_root.guild_id,
                    receipt_root.ruleset_key,
                    receipt_root.route_kind,
                    CASE receipt_root.route_kind
                        WHEN 'static' THEN receipt_root.route_key
                        ELSE receipt_root.instance_id
                    END
                )::TEXT,
            0
        )
    );

    IF EXISTS (
        SELECT 1
        FROM (
            SELECT blocked_effect.application_id,
                blocked_effect.interaction_id
            FROM public.runtime_interaction_effect_heads_v1 AS blocked_effect
            INNER JOIN public.runtime_interaction_receipt_roots_v1
                AS blocked_root
                ON blocked_root.application_id = blocked_effect.application_id
                AND blocked_root.interaction_id = blocked_effect.interaction_id
            WHERE blocked_effect.application_id = receipt_root.application_id
                AND blocked_effect.action_kind <> 'edit_response'
                AND blocked_effect.state IN (
                    'intended',
                    'indeterminate',
                    'observing',
                    'observation_pending',
                    'compensation_intended',
                    'compensation_indeterminate',
                    'compensation_observing',
                    'compensation_observation_pending',
                    'recovery_required'
                )
                AND blocked_root.guild_id = receipt_root.guild_id
                AND blocked_root.ruleset_key = receipt_root.ruleset_key
                AND blocked_root.interaction_id <> expected_interaction_id
                AND (
                    (
                        receipt_root.route_kind = 'static'
                        AND blocked_root.route_kind = 'static'
                        AND blocked_root.route_key = receipt_root.route_key
                    )
                    OR (
                        receipt_root.route_kind = 'instance'
                        AND blocked_root.route_kind = 'instance'
                        AND blocked_root.instance_id = receipt_root.instance_id
                    )
                )
            UNION ALL
            SELECT blocked_effect.application_id,
                blocked_effect.interaction_id
            FROM public.runtime_interaction_effect_rollbacks_v1 AS rollback
            INNER JOIN public.runtime_interaction_effect_heads_v1
                AS blocked_effect
                ON blocked_effect.application_id = rollback.application_id
                AND blocked_effect.interaction_id = rollback.interaction_id
                AND blocked_effect.action_index <= rollback.abort_action_index
            INNER JOIN public.runtime_interaction_receipt_roots_v1
                AS blocked_root
                ON blocked_root.application_id = blocked_effect.application_id
                AND blocked_root.interaction_id = blocked_effect.interaction_id
            WHERE rollback.state = 'required'
                AND blocked_effect.application_id = receipt_root.application_id
                AND blocked_effect.action_kind <> 'edit_response'
                AND blocked_effect.state IN (
                    'known_succeeded',
                    'reconciled_succeeded'
                )
                AND blocked_root.guild_id = receipt_root.guild_id
                AND blocked_root.ruleset_key = receipt_root.ruleset_key
                AND blocked_root.interaction_id <> expected_interaction_id
                AND (
                    (
                        receipt_root.route_kind = 'static'
                        AND blocked_root.route_kind = 'static'
                        AND blocked_root.route_key = receipt_root.route_key
                    )
                    OR (
                        receipt_root.route_kind = 'instance'
                        AND blocked_root.route_kind = 'instance'
                        AND blocked_root.instance_id = receipt_root.instance_id
                    )
                )
        ) AS blocked
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_effect_route_recovery_blocked';
    END IF;

    SELECT pg_catalog.count(*)
    INTO object_key_count
    FROM pg_catalog.jsonb_object_keys(proposed_resolved_input);

    IF NOT proposed_resolved_input ? 'references'
        OR pg_catalog.jsonb_typeof(
            proposed_resolved_input->'references'
        ) <> 'array'
        OR proposed_resolved_input - 'references'
            IS DISTINCT FROM effect_head.planned_recovery_input - 'references'
        OR object_key_count <> (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(
                effect_head.planned_recovery_input
            )
        )
        OR (
            effect_head.action_kind = 'register_instance'
            AND (
                object_key_count <> 4
                OR NOT proposed_resolved_input ?& ARRAY[
                    'instance_id',
                    'instance_kind',
                    'manifest_digest'
                ]
                OR COALESCE(
                    proposed_resolved_input->>'instance_id',
                    ''
                ) !~ '^[A-Za-z0-9_-]{1,32}$'
                OR COALESCE(
                    proposed_resolved_input->>'instance_kind',
                    ''
                ) !~ '^[A-Za-z0-9_-]{1,64}$'
                OR COALESCE(
                    proposed_resolved_input->>'manifest_digest',
                    ''
                ) !~ '^[0-9a-f]{64}$'
            )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_effect_resolved_input_invalid';
    END IF;

    SELECT COALESCE(
        pg_catalog.array_agg(
            reference.value->>'slot'
            ORDER BY reference.ordinality
        ),
        ARRAY[]::TEXT[]
    )
    INTO expected_reference_slots
    FROM pg_catalog.jsonb_array_elements(
        effect_head.planned_recovery_input->'references'
    ) WITH ORDINALITY AS reference(value, ordinality);

    SELECT COALESCE(
        pg_catalog.array_agg(
            reference.value->>'slot'
            ORDER BY reference.ordinality
        ),
        ARRAY[]::TEXT[]
    )
    INTO observed_reference_slots
    FROM pg_catalog.jsonb_array_elements(
        proposed_resolved_input->'references'
    ) WITH ORDINALITY AS reference(value, ordinality);

    IF observed_reference_slots IS DISTINCT FROM expected_reference_slots
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_effect_resolved_reference_order_invalid';
    END IF;

    FOR planned_reference IN
        SELECT reference.value
        FROM pg_catalog.jsonb_array_elements(
            effect_head.planned_recovery_input->'references'
        ) AS reference(value)
    LOOP
        SELECT pg_catalog.count(*)
        INTO matching_count
        FROM pg_catalog.jsonb_array_elements(
            proposed_resolved_input->'references'
        ) AS candidate(value)
        WHERE candidate.value->>'slot'
            = planned_reference.value->>'slot';

        IF matching_count <> 1 THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI003',
                MESSAGE = 'runtime_interaction_effect_resolved_reference_invalid';
        END IF;

        SELECT candidate.value
        INTO resolved_reference
        FROM pg_catalog.jsonb_array_elements(
            proposed_resolved_input->'references'
        ) AS candidate(value)
        WHERE candidate.value->>'slot'
            = planned_reference.value->>'slot';

        IF pg_catalog.jsonb_typeof(resolved_reference) <> 'object'
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI003',
                MESSAGE = 'runtime_interaction_effect_resolved_reference_invalid';
        END IF;

        SELECT pg_catalog.count(*)
        INTO object_key_count
        FROM pg_catalog.jsonb_object_keys(resolved_reference);

        IF object_key_count <> 2
            OR NOT resolved_reference ?& ARRAY['slot', 'id']
            OR COALESCE(resolved_reference->>'id', '')
                !~ '^[1-9][0-9]{0,19}$'
            OR pg_catalog.length(resolved_reference->>'id') > 20
            OR (
                pg_catalog.length(resolved_reference->>'id') = 20
                AND resolved_reference->>'id'
                    > '18446744073709551615'
            )
            OR (
                planned_reference.value->>'source' = 'existing'
                AND resolved_reference->>'id'
                    IS DISTINCT FROM planned_reference.value->>'id'
            )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI003',
                MESSAGE = 'runtime_interaction_effect_resolved_reference_invalid';
        END IF;

        IF planned_reference.value->>'source' = 'action_output' THEN
            SELECT pg_catalog.count(*)
            INTO matching_count
            FROM public.runtime_interaction_effect_heads_v1 AS dependency
            WHERE dependency.application_id = expected_application_id
                AND dependency.interaction_id = expected_interaction_id
                AND dependency.action_index = (
                    planned_reference.value->>'action_index'
                )::SMALLINT
                AND dependency.output_kind
                    = planned_reference.value->>'output_kind'
                AND pg_catalog.encode(
                    dependency.planned_identity_digest,
                    'hex'
                ) = planned_reference.value->>'producer_identity_digest'
                AND dependency.output_id = resolved_reference->>'id'
                AND dependency.state IN (
                    'known_succeeded',
                    'reconciled_succeeded'
                );

            IF matching_count <> 1 THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RI001',
                    MESSAGE = 'runtime_interaction_effect_dependency_resolution_conflict';
            END IF;
        END IF;
    END LOOP;

    SELECT pg_catalog.count(*)
    INTO object_key_count
    FROM pg_catalog.jsonb_object_keys(proposed_resolved_preimage);

    IF object_key_count <> (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(effect_head.planned_preimage)
        )
        OR (
            effect_head.planned_preimage->>'kind' = 'none'
            AND proposed_resolved_preimage
                IS DISTINCT FROM effect_head.planned_preimage
        )
        OR (
            effect_head.planned_preimage->>'kind' <> 'none'
            AND (
                proposed_resolved_preimage - 'references'
                    IS DISTINCT FROM
                    effect_head.planned_preimage - 'references'
                OR proposed_resolved_preimage->'references'
                    IS DISTINCT FROM
                    proposed_resolved_input->'references'
            )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_effect_resolved_preimage_invalid';
    END IF;

    IF effect_head.state <> 'planned'
        OR effect_head.head_revision <> expected_effect_head_revision
        OR EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_rollbacks_v1 AS rollback
            WHERE rollback.application_id = expected_application_id
                AND rollback.interaction_id = expected_interaction_id
        )
        OR EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_heads_v1 AS dependency
            WHERE dependency.application_id = expected_application_id
                AND dependency.interaction_id = expected_interaction_id
                AND dependency.action_index
                    = ANY(effect_head.dependency_indices)
                AND dependency.state NOT IN (
                    'known_succeeded',
                    'reconciled_succeeded'
                )
        )
        OR (
            SELECT pg_catalog.count(*)
            FROM public.runtime_interaction_effect_heads_v1 AS dependency
            WHERE dependency.application_id = expected_application_id
                AND dependency.interaction_id = expected_interaction_id
                AND dependency.action_index
                    = ANY(effect_head.dependency_indices)
        ) <> pg_catalog.cardinality(effect_head.dependency_indices)
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_intend_conflict';
    END IF;

    UPDATE public.runtime_interaction_effect_heads_v1 AS head
    SET state = 'intended',
        head_revision = head.head_revision + 1,
        attempt_count = head.attempt_count + 1,
        intent_process_instance_id = expected_process_instance_id,
        intent_receipt_claim_revision = expected_receipt_claim_revision,
        intent_digest = proposed_intent_digest,
        intent_at = database_now,
        resolved_input = proposed_resolved_input,
        resolved_preimage_digest = proposed_resolved_preimage_digest,
        resolved_preimage = proposed_resolved_preimage,
        resolved_effect_identity_digest =
            proposed_resolved_effect_identity_digest,
        resolved_instance_manifest_digest =
            normalized_instance_manifest_digest,
        next_recovery_at = recovery_at,
        updated_at = database_now
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
        AND head.action_index = expected_action_index;

    INSERT INTO public.runtime_interaction_effect_events_v1 (
        application_id,
        interaction_id,
        action_index,
        event_revision,
        event_kind,
        from_state,
        to_state,
        receipt_claim_revision,
        recovery_claim_revision,
        process_instance_id,
        outcome_code,
        result_digest,
        output_kind,
        output_id,
        event_digest,
        observed_at
    ) VALUES (
        expected_application_id,
        expected_interaction_id,
        expected_action_index,
        effect_head.head_revision + 1,
        'intended',
        effect_head.state,
        'intended',
        expected_receipt_claim_revision,
        effect_head.recovery_claim_revision,
        expected_process_instance_id,
        'intended',
        proposed_intent_digest,
        effect_head.output_kind,
        NULL,
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.concat_ws(
                '|',
                'starring-runtime-interaction-effect-event-v1',
                expected_application_id,
                expected_interaction_id,
                expected_action_index::TEXT,
                (effect_head.head_revision + 1)::TEXT,
                'intended',
                effect_head.state,
                'intended',
                expected_receipt_claim_revision::TEXT,
                expected_process_instance_id,
                pg_catalog.encode(proposed_intent_digest, 'hex'),
                pg_catalog.encode(
                    proposed_resolved_effect_identity_digest,
                    'hex'
                ),
                COALESCE(
                    pg_catalog.encode(
                        normalized_instance_manifest_digest,
                        'hex'
                    ),
                    ''
                )
            ),
            'UTF8'
        )),
        database_now
    );

    outcome_name := 'intended';
    effect_state := 'intended';
    resulting_effect_head_revision := effect_head.head_revision + 1;
    resulting_recovery_at := recovery_at;
    observed_database_now := database_now;
    RETURN NEXT;
END;
$function$;

DO $interaction_manifest_extension$
DECLARE
    function_definition TEXT;
    return_contract TEXT;
    return_replacement TEXT;
BEGIN
    function_definition := pg_catalog.pg_get_functiondef(
        pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_schema_manifest_v1()'
        )
    );
    return_contract := $needle$    RETURN public.starring_runtime_interaction_receipt_schema_manifest_v1()
        AND observed_count = 22$needle$;
    return_replacement := $needle$    RETURN public.starring_runtime_interaction_effect_schema_manifest_v1()
        AND public.starring_runtime_interaction_receipt_schema_manifest_v1()
        AND observed_count = 22$needle$;

    IF function_definition IS NULL
        OR pg_catalog.strpos(function_definition, return_contract) = 0
        OR pg_catalog.strpos(
            pg_catalog.substr(
                function_definition,
                pg_catalog.strpos(function_definition, return_contract)
                    + pg_catalog.length(return_contract)
            ),
            return_contract
        ) <> 0
    THEN
        RAISE EXCEPTION 'runtime interaction effect manifest extension failed'
            USING ERRCODE = '55000';
    END IF;

    function_definition := pg_catalog.replace(
        function_definition,
        return_contract,
        return_replacement
    );
    EXECUTE function_definition;
END;
$interaction_manifest_extension$;

CREATE FUNCTION public.starring_runtime_interaction_effect_complete_receipt_v1(
    expected_application_id TEXT,
    expected_interaction_id TEXT,
    proposed_outcome_code TEXT,
    proposed_result_digest BYTEA,
    proposed_observed_at TIMESTAMPTZ
)
RETURNS BIGINT
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    head_row public.runtime_interaction_receipt_heads_v1%ROWTYPE;
    effect_root public.runtime_interaction_effect_roots_v1%ROWTYPE;
BEGIN
    IF expected_application_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_application_id) > 20
        OR expected_interaction_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_interaction_id) > 20
        OR proposed_outcome_code NOT IN (
            'effects_recovered_completed',
            'provisioning_completed_response_unconfirmed',
            'interaction_response_unrecoverable'
        )
        OR pg_catalog.octet_length(proposed_result_digest) <> 32
        OR NOT pg_catalog.isfinite(proposed_observed_at)
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_effect_receipt_completion_input_invalid';
    END IF;

    PERFORM root.application_id
    FROM public.runtime_interaction_receipt_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_receipt_not_found';
    END IF;

    SELECT head.*
    INTO head_row
    FROM public.runtime_interaction_receipt_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_effect_receipt_head_missing';
    END IF;

    SELECT root.*
    INTO effect_root
    FROM public.runtime_interaction_effect_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    IF NOT FOUND
        OR (
            effect_root.action_count > 0
            AND (
                head_row.acknowledgement_kind <> 'defer_ephemeral'
                OR head_row.acknowledgement_state <> 'deferred'
                OR head_row.acknowledgement_result <> 'succeeded'
            )
        )
        OR (
            effect_root.action_count = 0
            AND (
                head_row.acknowledgement_kind NOT IN (
                    'respond_ephemeral',
                    'open_modal'
                )
                OR head_row.acknowledgement_state <> 'responded'
                OR head_row.acknowledgement_result <> 'succeeded'
            )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_effect_acknowledgement_contract_corrupt';
    END IF;

    IF head_row.state = 'completed'
        AND head_row.terminal_outcome_code = proposed_outcome_code
        AND head_row.terminal_result_digest = proposed_result_digest
    THEN
        RETURN head_row.head_revision;
    END IF;

    IF head_row.state <> 'executing'
        OR head_row.action_plan_digest IS NULL
        OR head_row.acknowledgement_state NOT IN (
            'unacknowledged',
            'deferred',
            'responded'
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_receipt_completion_conflict';
    END IF;

    UPDATE public.runtime_interaction_receipt_heads_v1 AS head
    SET state = 'completed',
        head_revision = head.head_revision + 1,
        terminal_outcome_code = proposed_outcome_code,
        terminal_result_digest = proposed_result_digest,
        terminal_at = proposed_observed_at,
        updated_at = proposed_observed_at
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id;

    INSERT INTO public.runtime_interaction_receipt_events_v1 (
        application_id,
        interaction_id,
        event_revision,
        event_kind,
        from_state,
        to_state,
        from_acknowledgement_state,
        to_acknowledgement_state,
        claim_revision,
        claim_process_instance_id,
        claim_gateway_shard_id,
        claim_gateway_owner_lease_epoch,
        claim_gateway_owner_revision,
        claim_serving_lease_epoch,
        claim_serving_revision,
        outcome_code,
        event_digest,
        observed_at
    ) VALUES (
        expected_application_id,
        expected_interaction_id,
        head_row.head_revision + 1,
        'completed',
        head_row.state,
        'completed',
        head_row.acknowledgement_state,
        head_row.acknowledgement_state,
        head_row.claim_revision,
        head_row.claim_process_instance_id,
        head_row.claim_gateway_shard_id,
        head_row.claim_gateway_owner_lease_epoch,
        head_row.claim_gateway_owner_revision,
        head_row.claim_serving_lease_epoch,
        head_row.claim_serving_revision,
        proposed_outcome_code,
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.concat_ws(
                '|',
                'starring-runtime-interaction-receipt-event-v1',
                expected_application_id,
                expected_interaction_id,
                (head_row.head_revision + 1)::TEXT,
                'completed',
                head_row.state,
                'completed',
                head_row.acknowledgement_state,
                head_row.acknowledgement_state,
                head_row.claim_revision::TEXT,
                head_row.claim_process_instance_id,
                head_row.claim_gateway_shard_id,
                head_row.claim_gateway_owner_lease_epoch::TEXT,
                head_row.claim_gateway_owner_revision::TEXT,
                head_row.claim_serving_lease_epoch::TEXT,
                head_row.claim_serving_revision::TEXT,
                proposed_outcome_code
            ),
            'UTF8'
        )),
        proposed_observed_at
    );

    DELETE FROM public.runtime_interaction_receipt_token_secrets_v1
    WHERE application_id = expected_application_id
        AND interaction_id = expected_interaction_id;

    RETURN head_row.head_revision + 1;
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_effect_resolve_receipt_v1(
    expected_application_id TEXT,
    expected_interaction_id TEXT,
    proposed_observation_digest BYTEA,
    response_token_unavailable BOOLEAN
)
RETURNS TABLE(
    outcome_name TEXT,
    receipt_state TEXT,
    resulting_head_revision BIGINT,
    resulting_claim_revision BIGINT,
    resulting_claim_expires_at TIMESTAMPTZ,
    observed_database_now TIMESTAMPTZ
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
    receipt_head public.runtime_interaction_receipt_heads_v1%ROWTYPE;
    effect_root public.runtime_interaction_effect_roots_v1%ROWTYPE;
    response_head public.runtime_interaction_effect_heads_v1%ROWTYPE;
    database_now TIMESTAMPTZ;
    terminal_outcome TEXT;
    response_count BIGINT;
    receipt_revision BIGINT;
BEGIN
    IF expected_application_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_application_id) > 20
        OR expected_interaction_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_interaction_id) > 20
        OR pg_catalog.octet_length(proposed_observation_digest) <> 32
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_effect_receipt_resolution_input_invalid';
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            'starring-runtime-interaction-receipt-v1:'
                || expected_application_id
                || ':'
                || expected_interaction_id,
            0
        )
    );

    PERFORM root.application_id
    FROM public.runtime_interaction_receipt_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    SELECT head.*
    INTO receipt_head
    FROM public.runtime_interaction_receipt_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
    FOR UPDATE;

    database_now := pg_catalog.clock_timestamp();

    IF NOT FOUND OR receipt_head.state <> 'executing' THEN
        outcome_name := 'effect_resolution_unavailable';
        receipt_state := receipt_head.state;
        resulting_head_revision := receipt_head.head_revision;
        resulting_claim_revision := receipt_head.claim_revision;
        resulting_claim_expires_at := receipt_head.claim_expires_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT root.*
    INTO effect_root
    FROM public.runtime_interaction_effect_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    IF NOT FOUND THEN
        outcome_name := 'effect_resolution_unavailable';
        receipt_state := receipt_head.state;
        resulting_head_revision := receipt_head.head_revision;
        resulting_claim_revision := receipt_head.claim_revision;
        resulting_claim_expires_at := receipt_head.claim_expires_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    PERFORM rollback.application_id
    FROM public.runtime_interaction_effect_rollbacks_v1 AS rollback
    WHERE rollback.application_id = expected_application_id
        AND rollback.interaction_id = expected_interaction_id
    FOR UPDATE;

    IF FOUND THEN
        outcome_name := 'effect_resolution_unavailable';
        receipt_state := receipt_head.state;
        resulting_head_revision := receipt_head.head_revision;
        resulting_claim_revision := receipt_head.claim_revision;
        resulting_claim_expires_at := receipt_head.claim_expires_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    PERFORM effect.action_index
    FROM public.runtime_interaction_effect_heads_v1 AS effect
    WHERE effect.application_id = expected_application_id
        AND effect.interaction_id = expected_interaction_id
    ORDER BY effect.action_index
    FOR UPDATE;

    SELECT pg_catalog.count(*)
    INTO response_count
    FROM public.runtime_interaction_effect_heads_v1 AS response
    WHERE response.application_id = expected_application_id
        AND response.interaction_id = expected_interaction_id
        AND response.action_kind = 'edit_response';

    IF effect_root.action_plan_digest
            IS DISTINCT FROM receipt_head.action_plan_digest
        OR effect_root.action_count <> (
            SELECT pg_catalog.count(*)
            FROM public.runtime_interaction_effect_heads_v1 AS effect
            WHERE effect.application_id = expected_application_id
                AND effect.interaction_id = expected_interaction_id
        )
        OR response_count > 1
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_effect_plan_corruption';
    END IF;

    terminal_outcome := CASE
        WHEN effect_root.action_count = 0
            AND receipt_head.acknowledgement_result = 'succeeded'
            THEN 'effects_recovered_completed'
        WHEN effect_root.action_count > 0
            AND NOT EXISTS (
                SELECT 1
                FROM public.runtime_interaction_effect_heads_v1 AS effect
                WHERE effect.application_id = expected_application_id
                    AND effect.interaction_id = expected_interaction_id
                    AND effect.state NOT IN (
                        'known_succeeded',
                        'reconciled_succeeded'
                    )
            )
            THEN 'effects_recovered_completed'
        WHEN NOT EXISTS (
                SELECT 1
                FROM public.runtime_interaction_effect_heads_v1 AS mutable
                WHERE mutable.application_id = expected_application_id
                    AND mutable.interaction_id = expected_interaction_id
                    AND mutable.action_kind <> 'edit_response'
                    AND mutable.state NOT IN (
                        'known_succeeded',
                        'reconciled_succeeded'
                    )
            )
            AND EXISTS (
                SELECT 1
                FROM public.runtime_interaction_effect_heads_v1 AS response
                WHERE response.application_id = expected_application_id
                    AND response.interaction_id = expected_interaction_id
                    AND response.action_kind = 'edit_response'
                    AND response.state IN ('planned', 'known_failed')
            )
            THEN 'provisioning_completed_response_unconfirmed'
        WHEN NOT EXISTS (
                SELECT 1
                FROM public.runtime_interaction_effect_heads_v1 AS mutable
                WHERE mutable.application_id = expected_application_id
                    AND mutable.interaction_id = expected_interaction_id
                    AND mutable.action_kind <> 'edit_response'
                    AND mutable.state NOT IN (
                        'known_succeeded',
                        'reconciled_succeeded'
                    )
            )
            AND EXISTS (
                SELECT 1
                FROM public.runtime_interaction_effect_heads_v1 AS response
                WHERE response.application_id = expected_application_id
                    AND response.interaction_id = expected_interaction_id
                    AND response.action_kind = 'edit_response'
                    AND response.state = 'recovery_required'
            )
            THEN 'interaction_response_unrecoverable'
        ELSE NULL
    END;

    IF terminal_outcome IS NULL
        AND response_token_unavailable
        AND NOT EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_heads_v1 AS mutable
            WHERE mutable.application_id = expected_application_id
                AND mutable.interaction_id = expected_interaction_id
                AND mutable.action_kind <> 'edit_response'
                AND mutable.state NOT IN (
                    'known_succeeded',
                    'reconciled_succeeded'
                )
        )
    THEN
        IF EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_heads_v1 AS response
            WHERE response.application_id = expected_application_id
                AND response.interaction_id = expected_interaction_id
                AND response.action_kind = 'edit_response'
                AND response.state = 'observing'
                AND response.recovery_expires_at > database_now
        ) THEN
            outcome_name := 'effect_recovery_pending';
            receipt_state := receipt_head.state;
            resulting_head_revision := receipt_head.head_revision;
            resulting_claim_revision := receipt_head.claim_revision;
            resulting_claim_expires_at := receipt_head.claim_expires_at;
            observed_database_now := database_now;
            RETURN NEXT;
            RETURN;
        END IF;

        SELECT response.*
        INTO response_head
        FROM public.runtime_interaction_effect_heads_v1 AS response
        WHERE response.application_id = expected_application_id
            AND response.interaction_id = expected_interaction_id
            AND response.action_kind = 'edit_response'
            AND response.state IN (
                'intended',
                'indeterminate',
                'observing',
                'observation_pending'
            );

        IF FOUND THEN
            UPDATE public.runtime_interaction_effect_heads_v1 AS response
            SET state = 'recovery_required',
                head_revision = response.head_revision + 1,
                result_digest = COALESCE(
                    response.result_digest,
                    proposed_observation_digest
                ),
                result_at = COALESCE(response.result_at, database_now),
                recovery_process_instance_id = NULL,
                recovery_gateway_shard_id = NULL,
                recovery_runtime_build_revision = NULL,
                recovery_acquired_at = NULL,
                recovery_expires_at = NULL,
                next_recovery_at = NULL,
                updated_at = database_now
            WHERE response.application_id = expected_application_id
                AND response.interaction_id = expected_interaction_id
                AND response.action_index = response_head.action_index;

            INSERT INTO public.runtime_interaction_effect_events_v1 (
                application_id,
                interaction_id,
                action_index,
                event_revision,
                event_kind,
                from_state,
                to_state,
                receipt_claim_revision,
                recovery_claim_revision,
                process_instance_id,
                outcome_code,
                result_digest,
                output_kind,
                output_id,
                event_digest,
                observed_at
            ) VALUES (
                expected_application_id,
                expected_interaction_id,
                response_head.action_index,
                response_head.head_revision + 1,
                'recovery_required',
                response_head.state,
                'recovery_required',
                NULL,
                response_head.recovery_claim_revision,
                NULL,
                'interaction_response_unrecoverable',
                COALESCE(
                    response_head.result_digest,
                    proposed_observation_digest
                ),
                response_head.output_kind,
                NULL,
                pg_catalog.sha256(pg_catalog.convert_to(
                    pg_catalog.concat_ws(
                        '|',
                        'starring-runtime-interaction-effect-event-v1',
                        expected_application_id,
                        expected_interaction_id,
                        response_head.action_index::TEXT,
                        (response_head.head_revision + 1)::TEXT,
                        'recovery_required',
                        response_head.state,
                        'recovery_required',
                        response_head.recovery_claim_revision::TEXT,
                        'interaction_response_unrecoverable',
                        pg_catalog.encode(
                            COALESCE(
                                response_head.result_digest,
                                proposed_observation_digest
                            ),
                            'hex'
                        )
                    ),
                    'UTF8'
                )),
                database_now
            );

            terminal_outcome := 'interaction_response_unrecoverable';
        END IF;
    END IF;

    IF terminal_outcome IS NOT NULL THEN
        receipt_revision :=
            public.starring_runtime_interaction_effect_complete_receipt_v1(
                expected_application_id,
                expected_interaction_id,
                terminal_outcome,
                proposed_observation_digest,
                database_now
            );
        outcome_name := terminal_outcome;
        receipt_state := 'completed';
        resulting_head_revision := receipt_revision;
        resulting_claim_revision := receipt_head.claim_revision;
        resulting_claim_expires_at := receipt_head.claim_expires_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF NOT EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_heads_v1 AS mutable
            WHERE mutable.application_id = expected_application_id
                AND mutable.interaction_id = expected_interaction_id
                AND mutable.action_kind <> 'edit_response'
                AND mutable.state NOT IN (
                    'known_succeeded',
                    'reconciled_succeeded'
                )
        )
        AND EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_heads_v1 AS response
            WHERE response.application_id = expected_application_id
                AND response.interaction_id = expected_interaction_id
                AND response.action_kind = 'edit_response'
                AND response.state IN (
                    'intended',
                    'indeterminate',
                    'observing',
                    'observation_pending'
                )
        )
    THEN
        outcome_name := 'effect_recovery_pending';
    ELSE
        outcome_name := 'effect_resolution_unavailable';
    END IF;

    receipt_state := receipt_head.state;
    resulting_head_revision := receipt_head.head_revision;
    resulting_claim_revision := receipt_head.claim_revision;
    resulting_claim_expires_at := receipt_head.claim_expires_at;
    observed_database_now := database_now;
    RETURN NEXT;
END;
$function$;

DO $receipt_effect_finish_extension$
DECLARE
    function_definition TEXT;
    lock_contract TEXT;
    lock_replacement TEXT;
    terminal_guard_contract TEXT;
    terminal_guard_replacement TEXT;
    guard_contract TEXT;
    guard_replacement TEXT;
BEGIN
    function_definition := pg_catalog.pg_get_functiondef(
        pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_receipt_finish_v1(text,text,bigint,bigint,text,bytea,text,text,bytea)'
        )
    );
    lock_contract := $needle$    database_now := pg_catalog.clock_timestamp();

    SELECT head.*$needle$;
    lock_replacement := $extension$    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            'starring-runtime-interaction-receipt-v1:'
                || expected_application_id
                || ':'
                || expected_interaction_id,
            0
        )
    );

    database_now := pg_catalog.clock_timestamp();

    SELECT head.*$extension$;
    terminal_guard_contract := $needle$    IF head_row.state IN ('completed', 'failed', 'recovery_required') THEN$needle$;
    terminal_guard_replacement := $extension$    IF head_row.state IN ('failed', 'recovery_required')
        AND head_row.acknowledgement_state = 'response_recovery_terminal'
        AND proposed_terminal_state = 'completed'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_receipt_finish_conflict';
    END IF;

    IF head_row.state IN ('completed', 'failed', 'recovery_required') THEN$extension$;
    guard_contract := $needle$        RETURN NEXT;
        RETURN;
    END IF;

    IF head_row.head_revision <> expected_head_revision$needle$;
    guard_replacement := $extension$        RETURN NEXT;
        RETURN;
    END IF;

    IF proposed_terminal_state = 'completed'
        AND EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_roots_v1 AS effect_root
            WHERE effect_root.application_id = expected_application_id
                AND effect_root.interaction_id = expected_interaction_id
        )
    THEN
        PERFORM effect_root.application_id
        FROM public.runtime_interaction_effect_roots_v1 AS effect_root
        WHERE effect_root.application_id = expected_application_id
            AND effect_root.interaction_id = expected_interaction_id
        FOR KEY SHARE;

        IF EXISTS (
                SELECT 1
                FROM public.runtime_interaction_effect_roots_v1 AS root
                WHERE root.application_id = expected_application_id
                    AND root.interaction_id = expected_interaction_id
                    AND (
                        (
                            root.action_count > 0
                            AND (
                                head_row.acknowledgement_kind
                                    <> 'defer_ephemeral'
                                OR head_row.acknowledgement_state <> 'deferred'
                                OR head_row.acknowledgement_result <> 'succeeded'
                            )
                        )
                        OR (
                            root.action_count = 0
                            AND (
                                head_row.acknowledgement_kind NOT IN (
                                    'respond_ephemeral',
                                    'open_modal'
                                )
                                OR head_row.acknowledgement_state <> 'responded'
                                OR head_row.acknowledgement_result <> 'succeeded'
                            )
                        )
                    )
            )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI001',
                MESSAGE = 'runtime_interaction_receipt_finish_conflict';
        END IF;

        PERFORM rollback.application_id
        FROM public.runtime_interaction_effect_rollbacks_v1 AS rollback
        WHERE rollback.application_id = expected_application_id
            AND rollback.interaction_id = expected_interaction_id
        FOR UPDATE;

        IF FOUND THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI001',
                MESSAGE = 'runtime_interaction_receipt_finish_conflict';
        END IF;

        PERFORM effect.action_index
        FROM public.runtime_interaction_effect_heads_v1 AS effect
        WHERE effect.application_id = expected_application_id
            AND effect.interaction_id = expected_interaction_id
        ORDER BY effect.action_index
        FOR UPDATE;

        IF NOT EXISTS (
                SELECT 1
                FROM public.runtime_interaction_effect_roots_v1 AS effect_root
                WHERE effect_root.application_id = expected_application_id
                    AND effect_root.interaction_id = expected_interaction_id
                    AND effect_root.action_plan_digest
                        = head_row.action_plan_digest
                    AND effect_root.action_count = (
                        SELECT pg_catalog.count(*)
                        FROM public.runtime_interaction_effect_heads_v1
                            AS effect
                        WHERE effect.application_id
                                = expected_application_id
                            AND effect.interaction_id
                                = expected_interaction_id
                    )
            )
            OR (
                SELECT pg_catalog.count(*)
                FROM public.runtime_interaction_effect_heads_v1 AS response
                WHERE response.application_id = expected_application_id
                    AND response.interaction_id = expected_interaction_id
                    AND response.action_kind = 'edit_response'
            ) > 1
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI002',
                MESSAGE = 'runtime_interaction_effect_plan_corruption';
        END IF;

        IF NOT (
            (
                NOT EXISTS (
                    SELECT 1
                    FROM public.runtime_interaction_effect_heads_v1 AS effect
                    WHERE effect.application_id = expected_application_id
                        AND effect.interaction_id = expected_interaction_id
                )
                AND head_row.acknowledgement_result = 'succeeded'
            )
            OR (
                EXISTS (
                    SELECT 1
                    FROM public.runtime_interaction_effect_heads_v1 AS effect
                    WHERE effect.application_id = expected_application_id
                        AND effect.interaction_id = expected_interaction_id
                )
                AND NOT EXISTS (
                    SELECT 1
                    FROM public.runtime_interaction_effect_heads_v1 AS effect
                    WHERE effect.application_id = expected_application_id
                        AND effect.interaction_id = expected_interaction_id
                        AND effect.state NOT IN (
                            'known_succeeded',
                            'reconciled_succeeded'
                        )
                )
            )
            OR (
                proposed_terminal_outcome_code
                    = 'interaction_provisioning_completed_response_unconfirmed'
                AND NOT EXISTS (
                    SELECT 1
                    FROM public.runtime_interaction_effect_heads_v1 AS mutable
                    WHERE mutable.application_id = expected_application_id
                        AND mutable.interaction_id = expected_interaction_id
                        AND mutable.action_kind <> 'edit_response'
                        AND mutable.state NOT IN (
                            'known_succeeded',
                            'reconciled_succeeded'
                        )
                )
                AND EXISTS (
                    SELECT 1
                    FROM public.runtime_interaction_effect_heads_v1 AS response
                    WHERE response.application_id = expected_application_id
                        AND response.interaction_id = expected_interaction_id
                        AND response.action_kind = 'edit_response'
                        AND response.state = 'known_failed'
                )
            )
        ) THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI001',
                MESSAGE = 'runtime_interaction_receipt_finish_conflict';
        END IF;
    END IF;

    IF head_row.head_revision <> expected_head_revision$extension$;

    IF function_definition IS NULL
        OR pg_catalog.strpos(function_definition, lock_contract) = 0
        OR pg_catalog.strpos(
            pg_catalog.substr(
                function_definition,
                pg_catalog.strpos(function_definition, lock_contract)
                    + pg_catalog.length(lock_contract)
            ),
            lock_contract
        ) <> 0
        OR pg_catalog.strpos(
            function_definition,
            terminal_guard_contract
        ) = 0
        OR pg_catalog.strpos(
            pg_catalog.substr(
                function_definition,
                pg_catalog.strpos(
                    function_definition,
                    terminal_guard_contract
                ) + pg_catalog.length(terminal_guard_contract)
            ),
            terminal_guard_contract
        ) <> 0
        OR pg_catalog.strpos(function_definition, guard_contract) = 0
        OR pg_catalog.strpos(
            pg_catalog.substr(
                function_definition,
                pg_catalog.strpos(function_definition, guard_contract)
                    + pg_catalog.length(guard_contract)
            ),
            guard_contract
        ) <> 0
    THEN
        RAISE EXCEPTION 'runtime interaction receipt finish extension failed'
            USING ERRCODE = '55000';
    END IF;

    function_definition := pg_catalog.replace(
        function_definition,
        lock_contract,
        lock_replacement
    );
    function_definition := pg_catalog.replace(
        function_definition,
        terminal_guard_contract,
        terminal_guard_replacement
    );
    function_definition := pg_catalog.replace(
        function_definition,
        guard_contract,
        guard_replacement
    );
    EXECUTE function_definition;
END;
$receipt_effect_finish_extension$;

DO $receipt_effect_terminalization_extension$
DECLARE
    function_definition TEXT;
    terminalization_contract TEXT;
    terminalization_replacement TEXT;
BEGIN
    function_definition := pg_catalog.pg_get_functiondef(
        pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_receipt_terminalize_expired_v1(text,text,bigint,bigint,text,text,bytea)'
        )
    );
    terminalization_contract := $needle$    next_acknowledgement_state := CASE
        WHEN head_row.acknowledgement_state = 'attempting'
            THEN 'response_recovery_terminal'
        ELSE head_row.acknowledgement_state
    END;$needle$;
    terminalization_replacement := $extension$    SELECT
        resolution.outcome_name,
        resolution.receipt_state,
        resolution.resulting_head_revision,
        resolution.resulting_claim_revision,
        resolution.resulting_claim_expires_at,
        resolution.observed_database_now
    INTO
        outcome_name,
        receipt_state,
        resulting_head_revision,
        resulting_claim_revision,
        resulting_claim_expires_at,
        observed_database_now
    FROM public.starring_runtime_interaction_effect_resolve_receipt_v1(
        expected_application_id,
        expected_interaction_id,
        proposed_observation_digest,
        FALSE
    ) AS resolution;

    IF outcome_name <> 'effect_resolution_unavailable' THEN
        RETURN NEXT;
        RETURN;
    END IF;

$extension$ || terminalization_contract;

    IF function_definition IS NULL
        OR pg_catalog.strpos(
            function_definition,
            terminalization_contract
        ) = 0
        OR pg_catalog.strpos(
            pg_catalog.substr(
                function_definition,
                pg_catalog.strpos(
                    function_definition,
                    terminalization_contract
                ) + pg_catalog.length(terminalization_contract)
            ),
            terminalization_contract
        ) <> 0
    THEN
        RAISE EXCEPTION 'runtime interaction receipt terminalization extension failed'
            USING ERRCODE = '55000';
    END IF;

    function_definition := pg_catalog.replace(
        function_definition,
        terminalization_contract,
        terminalization_replacement
    );
    EXECUTE function_definition;
END;
$receipt_effect_terminalization_extension$;

DO $receipt_effect_token_expiry_extension$
DECLARE
    function_definition TEXT;
    expiry_contract TEXT;
    expiry_replacement TEXT;
BEGIN
    function_definition := pg_catalog.pg_get_functiondef(
        pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_receipt_token_expire_v1(text,text,bigint,bigint,bytea)'
        )
    );
    expiry_contract := $needle$    expiry_outcome_code := CASE$needle$;
    expiry_replacement := $extension$    SELECT
        resolution.outcome_name,
        resolution.receipt_state,
        resolution.resulting_head_revision,
        resolution.resulting_claim_revision,
        resolution.observed_database_now
    INTO
        outcome_name,
        receipt_state,
        resulting_head_revision,
        resulting_claim_revision,
        observed_database_now
    FROM public.starring_runtime_interaction_effect_resolve_receipt_v1(
        expected_application_id,
        expected_interaction_id,
        proposed_expiry_observation_digest,
        TRUE
    ) AS resolution;

    IF outcome_name <> 'effect_resolution_unavailable' THEN
        RETURN NEXT;
        RETURN;
    END IF;

$extension$ || expiry_contract;

    IF function_definition IS NULL
        OR pg_catalog.strpos(function_definition, expiry_contract) = 0
        OR pg_catalog.strpos(
            pg_catalog.substr(
                function_definition,
                pg_catalog.strpos(function_definition, expiry_contract)
                    + pg_catalog.length(expiry_contract)
            ),
            expiry_contract
        ) <> 0
    THEN
        RAISE EXCEPTION 'runtime interaction receipt token expiry extension failed'
            USING ERRCODE = '55000';
    END IF;

    function_definition := pg_catalog.replace(
        function_definition,
        expiry_contract,
        expiry_replacement
    );
    EXECUTE function_definition;
END;
$receipt_effect_token_expiry_extension$;

CREATE FUNCTION public.starring_runtime_interaction_effect_require_rollback_v1(
    expected_application_id TEXT,
    expected_interaction_id TEXT,
    proposed_abort_reason TEXT,
    observed_at TIMESTAMPTZ
)
RETURNS BOOLEAN
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    abort_index SMALLINT;
BEGIN
    IF proposed_abort_reason NOT IN (
        'definitive_failure',
        'indeterminate',
        'observation_abort',
        'recovery_required',
        'response_failure'
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_effect_rollback_reason_invalid';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM public.runtime_interaction_effect_heads_v1 AS effect
        WHERE effect.application_id = expected_application_id
            AND effect.interaction_id = expected_interaction_id
            AND effect.action_kind = 'teardown_instance'
            AND effect.state IN (
                'known_succeeded',
                'reconciled_succeeded'
            )
    ) OR (
        proposed_abort_reason <> 'response_failure'
        AND NOT EXISTS (
        SELECT 1
        FROM public.runtime_interaction_effect_heads_v1 AS effect
        WHERE effect.application_id = expected_application_id
            AND effect.interaction_id = expected_interaction_id
            AND effect.action_kind <> 'edit_response'
            AND effect.state NOT IN (
                'known_succeeded',
                'reconciled_succeeded'
            )
        )
    ) THEN
        RETURN FALSE;
    END IF;

    SELECT pg_catalog.max(effect.action_index)
    INTO abort_index
    FROM public.runtime_interaction_effect_heads_v1 AS effect
    WHERE effect.application_id = expected_application_id
        AND effect.interaction_id = expected_interaction_id
        AND effect.action_kind NOT IN (
            'teardown_instance',
            'edit_response'
        )
        AND effect.state <> 'planned';

    IF abort_index IS NULL THEN
        RETURN FALSE;
    END IF;

    INSERT INTO public.runtime_interaction_effect_rollbacks_v1 (
        application_id,
        interaction_id,
        abort_action_index,
        abort_reason,
        state,
        revision,
        required_at,
        completed_at
    ) VALUES (
        expected_application_id,
        expected_interaction_id,
        abort_index,
        proposed_abort_reason,
        'required',
        1,
        observed_at,
        NULL
    ) ON CONFLICT (application_id, interaction_id) DO NOTHING;

    RETURN TRUE;
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_effect_try_complete_rollback_v1(
    expected_application_id TEXT,
    expected_interaction_id TEXT,
    observed_at TIMESTAMPTZ
)
RETURNS BOOLEAN
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    rollback_row public.runtime_interaction_effect_rollbacks_v1%ROWTYPE;
    receipt_state TEXT;
BEGIN
    SELECT rollback.*
    INTO rollback_row
    FROM public.runtime_interaction_effect_rollbacks_v1 AS rollback
    WHERE rollback.application_id = expected_application_id
        AND rollback.interaction_id = expected_interaction_id
    FOR UPDATE;

    IF NOT FOUND THEN
        RETURN TRUE;
    END IF;

    IF rollback_row.state = 'completed' THEN
        RETURN TRUE;
    END IF;

    SELECT head.state
    INTO receipt_state
    FROM public.runtime_interaction_receipt_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
    FOR SHARE;

    IF NOT FOUND
        OR receipt_state NOT IN ('failed', 'recovery_required')
        OR EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_heads_v1 AS effect
            WHERE effect.application_id = expected_application_id
                AND effect.interaction_id = expected_interaction_id
                AND effect.action_kind NOT IN (
                    'teardown_instance',
                    'edit_response'
                )
                AND effect.action_index <= rollback_row.abort_action_index
                AND effect.state NOT IN (
                    'planned',
                    'known_failed',
                    'compensated'
                )
        )
    THEN
        RETURN FALSE;
    END IF;

    UPDATE public.runtime_interaction_effect_rollbacks_v1 AS rollback
    SET state = 'completed',
        revision = 2,
        completed_at = observed_at
    WHERE rollback.application_id = expected_application_id
        AND rollback.interaction_id = expected_interaction_id
        AND rollback.state = 'required'
        AND rollback.revision = 1;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_effect_rollback_completion_conflict';
    END IF;

    DELETE FROM public.runtime_interaction_receipt_token_secrets_v1
    WHERE application_id = expected_application_id
        AND interaction_id = expected_interaction_id;

    RETURN TRUE;
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_effect_finish_v1(
    expected_application_id TEXT,
    expected_interaction_id TEXT,
    expected_receipt_head_revision BIGINT,
    expected_receipt_claim_revision BIGINT,
    expected_process_instance_id TEXT,
    expected_preflight_certificate_digest BYTEA,
    expected_action_index BIGINT,
    expected_effect_head_revision BIGINT,
    proposed_result_digest BYTEA,
    proposed_outcome TEXT,
    proposed_output_id TEXT
)
RETURNS TABLE(
    outcome_name TEXT,
    effect_state TEXT,
    resulting_effect_head_revision BIGINT,
    resulting_recovery_at TIMESTAMPTZ,
    observed_database_now TIMESTAMPTZ
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
    receipt_head public.runtime_interaction_receipt_heads_v1%ROWTYPE;
    effect_root public.runtime_interaction_effect_roots_v1%ROWTYPE;
    effect_head public.runtime_interaction_effect_heads_v1%ROWTYPE;
    database_now TIMESTAMPTZ;
    next_state TEXT;
    next_event_kind TEXT;
    normalized_output_id TEXT;
    recovery_at TIMESTAMPTZ;
BEGIN
    IF expected_application_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_application_id) > 20
        OR expected_interaction_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_interaction_id) > 20
        OR expected_receipt_head_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_receipt_claim_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR pg_catalog.octet_length(
            expected_preflight_certificate_digest
        ) <> 32
        OR expected_action_index NOT BETWEEN 0 AND 255
        OR expected_effect_head_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR pg_catalog.octet_length(proposed_result_digest) <> 32
        OR proposed_outcome NOT IN (
            'succeeded',
            'definitive_failure',
            'indeterminate'
        )
        OR pg_catalog.octet_length(proposed_output_id) > 128
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_effect_finish_input_invalid';
    END IF;

    database_now := pg_catalog.clock_timestamp();
    normalized_output_id := NULLIF(proposed_output_id, '');
    next_state := CASE proposed_outcome
        WHEN 'succeeded' THEN 'known_succeeded'
        WHEN 'definitive_failure' THEN 'known_failed'
        ELSE 'indeterminate'
    END;
    next_event_kind := CASE proposed_outcome
        WHEN 'succeeded' THEN 'known_succeeded'
        WHEN 'definitive_failure' THEN 'known_failed'
        ELSE 'indeterminate'
    END;
    recovery_at := CASE
        WHEN proposed_outcome = 'indeterminate' THEN database_now
        ELSE NULL
    END;

    PERFORM root.application_id
    FROM public.runtime_interaction_receipt_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_receipt_not_found';
    END IF;

    SELECT head.*
    INTO receipt_head
    FROM public.runtime_interaction_receipt_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
    FOR SHARE;

    IF NOT FOUND
        OR receipt_head.head_revision < expected_receipt_head_revision
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_finish_receipt_conflict';
    END IF;

    SELECT root.*
    INTO effect_root
    FROM public.runtime_interaction_effect_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    IF NOT FOUND
        OR effect_root.preflight_certificate_digest
            IS DISTINCT FROM expected_preflight_certificate_digest
        OR effect_root.action_plan_digest
            IS DISTINCT FROM receipt_head.action_plan_digest
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_effect_plan_corruption';
    END IF;

    SELECT head.*
    INTO effect_head
    FROM public.runtime_interaction_effect_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
        AND head.action_index = expected_action_index
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_action_not_found';
    END IF;

    IF effect_head.state = next_state
        AND effect_head.head_revision = expected_effect_head_revision + 1
        AND effect_head.intent_process_instance_id
            IS NOT DISTINCT FROM expected_process_instance_id
        AND effect_head.intent_receipt_claim_revision
            IS NOT DISTINCT FROM expected_receipt_claim_revision
        AND effect_head.result_digest
            IS NOT DISTINCT FROM proposed_result_digest
        AND effect_head.output_id IS NOT DISTINCT FROM normalized_output_id
        AND effect_head.success_binding_kind IS NOT DISTINCT FROM (CASE
            WHEN proposed_outcome = 'succeeded' THEN 'attempt_result'
            ELSE NULL
        END)
        AND effect_head.success_binding_digest IS NOT DISTINCT FROM (CASE
            WHEN proposed_outcome = 'succeeded' THEN proposed_result_digest
            ELSE NULL
        END)
        AND EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_events_v1 AS event
            WHERE event.application_id = expected_application_id
                AND event.interaction_id = expected_interaction_id
                AND event.action_index = expected_action_index
                AND event.event_revision = effect_head.head_revision
                AND event.event_kind = next_event_kind
                AND event.from_state = 'intended'
                AND event.to_state = next_state
                AND event.receipt_claim_revision
                    = expected_receipt_claim_revision
                AND event.recovery_claim_revision
                    = effect_head.recovery_claim_revision
                AND event.process_instance_id = expected_process_instance_id
                AND event.outcome_code = proposed_outcome
                AND event.result_digest = proposed_result_digest
                AND event.output_kind = effect_head.output_kind
                AND event.output_id IS NOT DISTINCT FROM normalized_output_id
                AND event.event_digest = pg_catalog.sha256(
                    pg_catalog.convert_to(
                        pg_catalog.concat_ws(
                            '|',
                            'starring-runtime-interaction-effect-event-v1',
                            expected_application_id,
                            expected_interaction_id,
                            expected_action_index::TEXT,
                            (expected_effect_head_revision + 1)::TEXT,
                            next_event_kind,
                            'intended',
                            next_state,
                            expected_receipt_claim_revision::TEXT,
                            expected_process_instance_id,
                            proposed_outcome,
                            pg_catalog.encode(
                                proposed_result_digest,
                                'hex'
                            ),
                            effect_head.output_kind,
                            COALESCE(normalized_output_id, '')
                        ),
                        'UTF8'
                    )
                )
        )
    THEN
        outcome_name := 'exact_replay';
        effect_state := effect_head.state;
        resulting_effect_head_revision := effect_head.head_revision;
        resulting_recovery_at := effect_head.next_recovery_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF effect_head.state <> 'intended'
        OR effect_head.head_revision <> expected_effect_head_revision
        OR effect_head.intent_process_instance_id
            IS DISTINCT FROM expected_process_instance_id
        OR effect_head.intent_receipt_claim_revision
            IS DISTINCT FROM expected_receipt_claim_revision
        OR (
            proposed_outcome = 'succeeded'
            AND (
                (
                    effect_head.output_kind IN (
                        'role_membership',
                        'permission_overwrite',
                        'original_response'
                    )
                    AND normalized_output_id IS NOT NULL
                )
                OR (
                    effect_head.output_kind IN (
                        'created_role',
                        'created_channel',
                        'posted_message'
                    )
                    AND (
                        normalized_output_id IS NULL
                        OR
                        normalized_output_id
                            !~ '^[1-9][0-9]{0,19}$'
                        OR pg_catalog.length(normalized_output_id) > 20
                        OR (
                            pg_catalog.length(normalized_output_id) = 20
                            AND normalized_output_id
                                > '18446744073709551615'
                        )
                    )
                )
                OR (
                    effect_head.output_kind = 'instance_state'
                    AND (
                        normalized_output_id IS NULL
                        OR normalized_output_id
                            !~ '^[A-Za-z0-9_-]{1,32}$'
                    )
                )
            )
        )
        OR (
            proposed_outcome <> 'succeeded'
            AND normalized_output_id IS NOT NULL
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_finish_conflict';
    END IF;

    UPDATE public.runtime_interaction_effect_heads_v1 AS head
    SET state = next_state,
        head_revision = head.head_revision + 1,
        result_digest = proposed_result_digest,
        output_id = normalized_output_id,
        result_at = database_now,
        success_binding_kind = CASE
            WHEN proposed_outcome = 'succeeded' THEN 'attempt_result'
            ELSE NULL
        END,
        success_binding_digest = CASE
            WHEN proposed_outcome = 'succeeded' THEN proposed_result_digest
            ELSE NULL
        END,
        next_recovery_at = recovery_at,
        updated_at = database_now
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
        AND head.action_index = expected_action_index;

    INSERT INTO public.runtime_interaction_effect_events_v1 (
        application_id,
        interaction_id,
        action_index,
        event_revision,
        event_kind,
        from_state,
        to_state,
        receipt_claim_revision,
        recovery_claim_revision,
        process_instance_id,
        outcome_code,
        result_digest,
        output_kind,
        output_id,
        event_digest,
        observed_at
    ) VALUES (
        expected_application_id,
        expected_interaction_id,
        expected_action_index,
        effect_head.head_revision + 1,
        next_event_kind,
        effect_head.state,
        next_state,
        expected_receipt_claim_revision,
        effect_head.recovery_claim_revision,
        expected_process_instance_id,
        proposed_outcome,
        proposed_result_digest,
        effect_head.output_kind,
        normalized_output_id,
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.concat_ws(
                '|',
                'starring-runtime-interaction-effect-event-v1',
                expected_application_id,
                expected_interaction_id,
                expected_action_index::TEXT,
                (effect_head.head_revision + 1)::TEXT,
                next_event_kind,
                effect_head.state,
                next_state,
                expected_receipt_claim_revision::TEXT,
                expected_process_instance_id,
                proposed_outcome,
                pg_catalog.encode(proposed_result_digest, 'hex'),
                effect_head.output_kind,
                COALESCE(normalized_output_id, '')
            ),
            'UTF8'
        )),
        database_now
    );

    IF proposed_outcome IN ('definitive_failure', 'indeterminate')
        AND effect_head.action_kind <> 'edit_response'
    THEN
        PERFORM public.starring_runtime_interaction_effect_require_rollback_v1(
            expected_application_id,
            expected_interaction_id,
            proposed_outcome,
            database_now
        );
    END IF;

    outcome_name := proposed_outcome;
    effect_state := next_state;
    resulting_effect_head_revision := effect_head.head_revision + 1;
    resulting_recovery_at := recovery_at;
    observed_database_now := database_now;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.guard_runtime_interaction_effect_root_v1()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    RAISE EXCEPTION USING
        ERRCODE = 'RI002',
        MESSAGE = 'runtime_interaction_effect_root_immutable';
END;
$function$;

CREATE FUNCTION public.guard_runtime_interaction_effect_event_v1()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    RAISE EXCEPTION USING
        ERRCODE = 'RI002',
        MESSAGE = 'runtime_interaction_effect_event_immutable';
END;
$function$;

CREATE FUNCTION public.guard_runtime_interaction_effect_rollback_v1()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    IF TG_OP <> 'UPDATE'
        OR ROW(NEW.application_id, NEW.interaction_id)
            IS DISTINCT FROM ROW(OLD.application_id, OLD.interaction_id)
        OR ROW(
            NEW.abort_action_index,
            NEW.abort_reason,
            NEW.required_at
        ) IS DISTINCT FROM ROW(
            OLD.abort_action_index,
            OLD.abort_reason,
            OLD.required_at
        )
        OR OLD.state <> 'required'
        OR OLD.revision <> 1
        OR OLD.completed_at IS NOT NULL
        OR NEW.state <> 'completed'
        OR NEW.revision <> 2
        OR NEW.completed_at IS NULL
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_effect_rollback_transition_invalid';
    END IF;

    RETURN NEW;
END;
$function$;

CREATE FUNCTION public.guard_runtime_interaction_effect_head_v1()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    IF TG_OP <> 'UPDATE'
        OR ROW(NEW.application_id, NEW.interaction_id, NEW.action_index)
            IS DISTINCT FROM
            ROW(OLD.application_id, OLD.interaction_id, OLD.action_index)
        OR ROW(
            NEW.action_kind,
            NEW.dependency_indices,
            NEW.planned_identity_digest,
            NEW.input_digest,
            NEW.expected_postimage_digest,
            NEW.planned_recovery_input,
            NEW.planned_preimage_digest,
            NEW.planned_preimage,
            NEW.output_kind,
            NEW.correlation_class,
            NEW.correlation_digest,
            NEW.correlation_marker
        ) IS DISTINCT FROM ROW(
            OLD.action_kind,
            OLD.dependency_indices,
            OLD.planned_identity_digest,
            OLD.input_digest,
            OLD.expected_postimage_digest,
            OLD.planned_recovery_input,
            OLD.planned_preimage_digest,
            OLD.planned_preimage,
            OLD.output_kind,
            OLD.correlation_class,
            OLD.correlation_digest,
            OLD.correlation_marker
        )
        OR (
            OLD.resolved_input IS NOT NULL
            AND ROW(
                NEW.resolved_input,
                NEW.resolved_preimage_digest,
                NEW.resolved_preimage,
                NEW.resolved_effect_identity_digest,
                NEW.resolved_instance_manifest_digest
            ) IS DISTINCT FROM ROW(
                OLD.resolved_input,
                OLD.resolved_preimage_digest,
                OLD.resolved_preimage,
                OLD.resolved_effect_identity_digest,
                OLD.resolved_instance_manifest_digest
            )
        )
        OR OLD.head_revision = 9223372036854775807
        OR NEW.head_revision IS DISTINCT FROM OLD.head_revision + 1
        OR NEW.attempt_count NOT IN (
            OLD.attempt_count,
            OLD.attempt_count + 1
        )
        OR NEW.observation_attempt_count NOT IN (
            OLD.observation_attempt_count,
            OLD.observation_attempt_count + 1
        )
        OR NEW.compensation_attempt_count NOT IN (
            OLD.compensation_attempt_count,
            OLD.compensation_attempt_count + 1
        )
        OR NEW.compensation_observation_attempt_count NOT IN (
            OLD.compensation_observation_attempt_count,
            OLD.compensation_observation_attempt_count + 1
        )
        OR NEW.recovery_claim_revision NOT IN (
            OLD.recovery_claim_revision,
            OLD.recovery_claim_revision + 1
        )
        OR (
            OLD.recovery_claim_revision = 9223372036854775807
            AND NEW.recovery_claim_revision <> OLD.recovery_claim_revision
        )
        OR (
            OLD.intent_digest IS NOT NULL
            AND ROW(
                NEW.intent_process_instance_id,
                NEW.intent_receipt_claim_revision,
                NEW.intent_digest,
                NEW.intent_at
            ) IS DISTINCT FROM ROW(
                OLD.intent_process_instance_id,
                OLD.intent_receipt_claim_revision,
                OLD.intent_digest,
                OLD.intent_at
            )
        )
        OR (
            OLD.result_digest IS NOT NULL
            AND ROW(NEW.result_digest, NEW.result_at)
                IS DISTINCT FROM
                ROW(OLD.result_digest, OLD.result_at)
        )
        OR (
            OLD.output_id IS NOT NULL
            AND NEW.output_id IS DISTINCT FROM OLD.output_id
        )
        OR (
            OLD.success_binding_digest IS NOT NULL
            AND ROW(
                NEW.success_binding_kind,
                NEW.success_binding_digest
            ) IS DISTINCT FROM ROW(
                OLD.success_binding_kind,
                OLD.success_binding_digest
            )
        )
        OR (
            OLD.compensation_intent_digest IS NOT NULL
            AND ROW(
                NEW.compensation_intent_digest,
                NEW.compensation_intent_at
            ) IS DISTINCT FROM ROW(
                OLD.compensation_intent_digest,
                OLD.compensation_intent_at
            )
        )
        OR (
            OLD.compensation_result_digest IS NOT NULL
            AND ROW(
                NEW.compensation_result_digest,
                NEW.compensation_result_at
            ) IS DISTINCT FROM ROW(
                OLD.compensation_result_digest,
                OLD.compensation_result_at
            )
        )
        OR NOT (
            OLD.state = NEW.state
            OR (OLD.state = 'planned' AND NEW.state = 'intended')
            OR (
                OLD.state = 'intended'
                AND NEW.state IN (
                    'known_succeeded',
                    'known_failed',
                    'indeterminate',
                    'observing'
                )
            )
            OR (
                OLD.state IN (
                    'indeterminate',
                    'observation_pending'
                )
                AND NEW.state = 'observing'
            )
            OR (
                OLD.state IN (
                    'intended',
                    'indeterminate',
                    'observation_pending'
                )
                AND NEW.state = 'recovery_required'
            )
            OR (
                OLD.state = 'observing'
                AND NEW.state IN (
                    'reconciled_succeeded',
                    'known_failed',
                    'observation_pending',
                    'recovery_required'
                )
            )
            OR (
                OLD.state IN ('known_succeeded', 'reconciled_succeeded')
                AND NEW.state = 'compensation_intended'
            )
            OR (
                OLD.state = 'compensation_intended'
                AND NEW.state IN (
                    'compensated',
                    'compensation_indeterminate',
                    'compensation_observing',
                    'recovery_required'
                )
            )
            OR (
                OLD.state IN (
                    'compensation_indeterminate',
                    'compensation_observation_pending'
                )
                AND NEW.state = 'compensation_observing'
            )
            OR (
                OLD.state IN (
                    'compensation_indeterminate',
                    'compensation_observation_pending'
                )
                AND NEW.state = 'recovery_required'
            )
            OR (
                OLD.state = 'compensation_observing'
                AND NEW.state IN (
                    'compensated',
                    'compensation_observation_pending',
                    'recovery_required'
                )
            )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_effect_head_transition_invalid';
    END IF;

    RETURN NEW;
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_effect_receipt_terminal_sync_v1()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    IF NEW.state IN ('failed', 'recovery_required')
        AND OLD.state NOT IN ('failed', 'recovery_required')
    THEN
        PERFORM public.starring_runtime_interaction_effect_require_rollback_v1(
            NEW.application_id,
            NEW.interaction_id,
            CASE
                WHEN NEW.acknowledgement_state
                        = 'response_recovery_terminal'
                    THEN 'response_failure'
                WHEN NEW.state = 'failed' THEN 'definitive_failure'
                ELSE 'recovery_required'
            END,
            NEW.updated_at
        );
        PERFORM public.starring_runtime_interaction_effect_try_complete_rollback_v1(
            NEW.application_id,
            NEW.interaction_id,
            NEW.updated_at
        );
    END IF;

    RETURN NEW;
END;
$function$;

DO $receipt_effect_irreversible_response_extension$
DECLARE
    function_definition TEXT;
    update_contract TEXT;
    update_replacement TEXT;
BEGIN
    function_definition := pg_catalog.pg_get_functiondef(
        pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_receipt_acknowledgement_finish_v1(text,text,bigint,bigint,text,bytea,text,bytea)'
        )
    );
    update_contract := $needle$    UPDATE public.runtime_interaction_receipt_heads_v1 AS head
    SET state = next_state,$needle$;
    update_replacement := $extension$    IF proposed_acknowledgement_result <> 'succeeded'
        AND EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_heads_v1 AS effect
            WHERE effect.application_id = expected_application_id
                AND effect.interaction_id = expected_interaction_id
                AND effect.action_kind = 'teardown_instance'
                AND effect.state IN (
                    'known_succeeded',
                    'reconciled_succeeded'
                )
        )
    THEN
        next_state := 'completed';
        next_outcome_code := 'interaction_response_unrecoverable';
    END IF;

    UPDATE public.runtime_interaction_receipt_heads_v1 AS head
    SET state = next_state,$extension$;
    IF function_definition IS NULL
        OR pg_catalog.strpos(function_definition, update_contract) = 0
        OR pg_catalog.strpos(
            pg_catalog.substr(
                function_definition,
                pg_catalog.strpos(function_definition, update_contract)
                    + pg_catalog.length(update_contract)
            ),
            update_contract
        ) <> 0
    THEN
        RAISE EXCEPTION 'runtime interaction irreversible response extension failed'
            USING ERRCODE = '55000';
    END IF;
    function_definition := pg_catalog.replace(
        function_definition,
        update_contract,
        update_replacement
    );
    EXECUTE function_definition;
END;
$receipt_effect_irreversible_response_extension$;

DO $receipt_effect_deferred_execution_extension$
DECLARE
    function_definition TEXT;
    state_contract TEXT;
    state_replacement TEXT;
    event_value_contract TEXT;
    event_value_replacement TEXT;
    digest_value_contract TEXT;
    digest_value_replacement TEXT;
BEGIN
    function_definition := pg_catalog.pg_get_functiondef(
        pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_receipt_execution_intend_v1(text,text,bigint,bigint,text,bytea)'
        )
    );
    state_contract := $needle$    IF head_row.state <> 'prepared'
$needle$;
    state_replacement := $extension$    IF head_row.state NOT IN ('prepared', 'deferred')
$extension$;
    event_value_contract := $needle$        'prepared',
        'executing',$needle$;
    event_value_replacement := $extension$        head_row.state,
        'executing',$extension$;
    digest_value_contract := $needle$                'prepared',
                'executing',$needle$;
    digest_value_replacement := $extension$                head_row.state,
                'executing',$extension$;
    IF function_definition IS NULL
        OR pg_catalog.strpos(function_definition, state_contract) = 0
        OR pg_catalog.strpos(
            pg_catalog.substr(
                function_definition,
                pg_catalog.strpos(function_definition, state_contract)
                    + pg_catalog.length(state_contract)
            ),
            state_contract
        ) <> 0
        OR pg_catalog.strpos(function_definition, event_value_contract) = 0
        OR pg_catalog.strpos(
            pg_catalog.substr(
                function_definition,
                pg_catalog.strpos(function_definition, event_value_contract)
                    + pg_catalog.length(event_value_contract)
            ),
            event_value_contract
        ) <> 0
        OR pg_catalog.strpos(function_definition, digest_value_contract) = 0
        OR pg_catalog.strpos(
            pg_catalog.substr(
                function_definition,
                pg_catalog.strpos(function_definition, digest_value_contract)
                    + pg_catalog.length(digest_value_contract)
            ),
            digest_value_contract
        ) <> 0
    THEN
        RAISE EXCEPTION 'runtime interaction deferred execution extension failed'
            USING ERRCODE = '55000';
    END IF;
    function_definition := pg_catalog.replace(
        function_definition,
        state_contract,
        state_replacement
    );
    function_definition := pg_catalog.replace(
        function_definition,
        event_value_contract,
        event_value_replacement
    );
    function_definition := pg_catalog.replace(
        function_definition,
        digest_value_contract,
        digest_value_replacement
    );
    EXECUTE function_definition;
END;
$receipt_effect_deferred_execution_extension$;

DO $receipt_effect_deferred_transition_extension$
DECLARE
    function_definition TEXT;
    transition_contract TEXT;
    transition_replacement TEXT;
BEGIN
    function_definition := pg_catalog.pg_get_functiondef(
        pg_catalog.to_regprocedure(
            'public.guard_runtime_interaction_receipt_head_v1()'
        )
    );
    transition_contract := $needle$                OLD.state = 'deferred'
                AND NEW.state IN (
                    'prepared',
                    'failed',
                    'recovery_required'
                )$needle$;
    transition_replacement := $extension$                OLD.state = 'deferred'
                AND NEW.state IN (
                    'prepared',
                    'executing',
                    'failed',
                    'recovery_required'
                )$extension$;
    IF function_definition IS NULL
        OR pg_catalog.strpos(function_definition, transition_contract) = 0
        OR pg_catalog.strpos(
            pg_catalog.substr(
                function_definition,
                pg_catalog.strpos(function_definition, transition_contract)
                    + pg_catalog.length(transition_contract)
            ),
            transition_contract
        ) <> 0
    THEN
        RAISE EXCEPTION 'runtime interaction deferred transition extension failed'
            USING ERRCODE = '55000';
    END IF;
    EXECUTE pg_catalog.replace(
        function_definition,
        transition_contract,
        transition_replacement
    );
END;
$receipt_effect_deferred_transition_extension$;

CREATE FUNCTION public.guard_runtime_interaction_effect_response_token_delete_v1()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    IF OLD.expires_at > pg_catalog.clock_timestamp()
        AND EXISTS (
            SELECT 1
            FROM public.runtime_interaction_receipt_heads_v1 AS receipt
            WHERE receipt.application_id = OLD.application_id
                AND receipt.interaction_id = OLD.interaction_id
                AND receipt.state = 'recovery_required'
        )
        AND EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_heads_v1 AS response
            WHERE response.application_id = OLD.application_id
                AND response.interaction_id = OLD.interaction_id
                AND response.action_kind = 'edit_response'
                AND response.state IN (
                    'planned',
                    'intended',
                    'indeterminate',
                    'observing',
                    'observation_pending'
                )
        )
        AND NOT EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_rollbacks_v1 AS rollback
            WHERE rollback.application_id = OLD.application_id
                AND rollback.interaction_id = OLD.interaction_id
                AND rollback.state = 'required'
        )
        AND NOT EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_heads_v1 AS mutable
            WHERE mutable.application_id = OLD.application_id
                AND mutable.interaction_id = OLD.interaction_id
                AND mutable.action_kind <> 'edit_response'
                AND mutable.state NOT IN (
                    'known_succeeded',
                    'reconciled_succeeded'
                )
        )
    THEN
        RETURN NULL;
    END IF;

    RETURN OLD;
END;
$function$;

CREATE TRIGGER runtime_interaction_effect_roots_v1_immutable_mutation
BEFORE UPDATE OR DELETE ON public.runtime_interaction_effect_roots_v1
FOR EACH ROW
EXECUTE FUNCTION public.guard_runtime_interaction_effect_root_v1();

CREATE TRIGGER runtime_interaction_effect_roots_v1_immutable_truncate
BEFORE TRUNCATE ON public.runtime_interaction_effect_roots_v1
FOR EACH STATEMENT
EXECUTE FUNCTION public.guard_runtime_interaction_effect_root_v1();

CREATE TRIGGER runtime_interaction_effect_rollbacks_v1_guard_mutation
BEFORE UPDATE OR DELETE ON public.runtime_interaction_effect_rollbacks_v1
FOR EACH ROW
EXECUTE FUNCTION public.guard_runtime_interaction_effect_rollback_v1();

CREATE TRIGGER runtime_interaction_effect_rollbacks_v1_guard_truncate
BEFORE TRUNCATE ON public.runtime_interaction_effect_rollbacks_v1
FOR EACH STATEMENT
EXECUTE FUNCTION public.guard_runtime_interaction_effect_rollback_v1();

CREATE TRIGGER runtime_interaction_receipt_heads_v1_effect_terminal_sync
AFTER UPDATE ON public.runtime_interaction_receipt_heads_v1
FOR EACH ROW
EXECUTE FUNCTION public.starring_runtime_interaction_effect_receipt_terminal_sync_v1();

CREATE TRIGGER runtime_interaction_receipt_token_secrets_v1_effect_delete_guard
BEFORE DELETE ON public.runtime_interaction_receipt_token_secrets_v1
FOR EACH ROW
EXECUTE FUNCTION public.guard_runtime_interaction_effect_response_token_delete_v1();

CREATE TRIGGER runtime_interaction_effect_heads_v1_guard_mutation
BEFORE UPDATE OR DELETE ON public.runtime_interaction_effect_heads_v1
FOR EACH ROW
EXECUTE FUNCTION public.guard_runtime_interaction_effect_head_v1();

CREATE TRIGGER runtime_interaction_effect_heads_v1_guard_truncate
BEFORE TRUNCATE ON public.runtime_interaction_effect_heads_v1
FOR EACH STATEMENT
EXECUTE FUNCTION public.guard_runtime_interaction_effect_head_v1();

CREATE TRIGGER runtime_interaction_effect_events_v1_immutable_mutation
BEFORE UPDATE OR DELETE ON public.runtime_interaction_effect_events_v1
FOR EACH ROW
EXECUTE FUNCTION public.guard_runtime_interaction_effect_event_v1();

CREATE TRIGGER runtime_interaction_effect_events_v1_immutable_truncate
BEFORE TRUNCATE ON public.runtime_interaction_effect_events_v1
FOR EACH STATEMENT
EXECUTE FUNCTION public.guard_runtime_interaction_effect_event_v1();

CREATE FUNCTION public.starring_runtime_interaction_effect_response_tail_scan_v1(
    expected_after_recovery_at TIMESTAMPTZ,
    expected_after_application_id TEXT,
    expected_after_interaction_id TEXT,
    expected_after_action_index BIGINT,
    expected_through_recovery_at TIMESTAMPTZ,
    expected_through_application_id TEXT,
    expected_through_interaction_id TEXT,
    expected_through_action_index BIGINT,
    expected_limit BIGINT
)
RETURNS TABLE(
    application_id TEXT,
    interaction_id TEXT,
    action_index SMALLINT,
    effect_state TEXT,
    effect_head_revision BIGINT,
    recovery_claim_revision BIGINT,
    observation_attempt_count INTEGER,
    planned_identity_digest BYTEA,
    input_digest BYTEA,
    expected_postimage_digest BYTEA,
    planned_recovery_input JSONB,
    planned_preimage_digest BYTEA,
    planned_preimage JSONB,
    resolved_input JSONB,
    resolved_preimage_digest BYTEA,
    resolved_preimage JSONB,
    resolved_effect_identity_digest BYTEA,
    intent_digest BYTEA,
    result_digest BYTEA,
    success_binding_kind TEXT,
    success_binding_digest BYTEA,
    correlation_digest BYTEA,
    action_plan_digest BYTEA,
    preflight_certificate_digest BYTEA,
    snapshot_digest BYTEA,
    receipt_state TEXT,
    receipt_head_revision BIGINT,
    receipt_claim_revision BIGINT,
    receipt_claim_expires_at TIMESTAMPTZ,
    token_expires_at TIMESTAMPTZ,
    tenant_id TEXT,
    installation_id TEXT,
    deployment_id TEXT,
    attestation_id TEXT,
    attestation_digest TEXT,
    guild_id TEXT,
    channel_id TEXT,
    actor_user_id TEXT,
    interaction_kind TEXT,
    ruleset_key TEXT,
    target_version BIGINT,
    target_content_hash TEXT,
    binding_revision BIGINT,
    binding_fingerprint TEXT,
    runtime_generation BIGINT,
    route_controller_fencing_token BIGINT,
    route_incarnation BIGINT,
    origin_process_instance_id TEXT,
    origin_serving_lease_epoch BIGINT,
    origin_serving_revision BIGINT,
    origin_gateway_shard_id TEXT,
    origin_gateway_owner_lease_epoch BIGINT,
    origin_gateway_owner_revision BIGINT,
    runtime_build_revision TEXT,
    route_kind TEXT,
    route_key TEXT,
    instance_id TEXT,
    execution_ruleset_version BIGINT,
    execution_ruleset_content_hash TEXT,
    instance_manifest_digest TEXT,
    request_digest BYTEA,
    next_recovery_at TIMESTAMPTZ,
    through_recovery_at TIMESTAMPTZ,
    through_application_id TEXT,
    through_interaction_id TEXT,
    through_action_index SMALLINT,
    observed_database_now TIMESTAMPTZ
)
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 256
AS $function$
DECLARE
    cycle_through_recovery_at TIMESTAMPTZ;
    cycle_through_application_id TEXT;
    cycle_through_interaction_id TEXT;
    cycle_through_action_index SMALLINT;
    database_now TIMESTAMPTZ;
BEGIN
    IF NOT pg_catalog.isfinite(expected_after_recovery_at)
        OR NOT pg_catalog.isfinite(expected_through_recovery_at)
        OR expected_limit NOT BETWEEN 1 AND 256
        OR (
            (expected_after_application_id = '') IS DISTINCT FROM
                (expected_after_interaction_id = '')
        )
        OR (
            (expected_through_application_id = '') IS DISTINCT FROM
                (expected_through_interaction_id = '')
        )
        OR (
            expected_after_application_id = ''
            AND (
                expected_after_recovery_at
                    <> '1970-01-01 00:00:00+00'::TIMESTAMPTZ
                OR expected_after_action_index <> -1
            )
        )
        OR (
            expected_through_application_id = ''
            AND (
                expected_through_recovery_at
                    <> '1970-01-01 00:00:00+00'::TIMESTAMPTZ
                OR expected_through_action_index <> -1
            )
        )
        OR (
            expected_after_application_id <> ''
            AND (
                expected_after_application_id !~ '^[1-9][0-9]{0,19}$'
                OR pg_catalog.length(expected_after_application_id) > 20
                OR expected_after_interaction_id !~ '^[1-9][0-9]{0,19}$'
                OR pg_catalog.length(expected_after_interaction_id) > 20
                OR expected_after_action_index NOT BETWEEN 0 AND 255
            )
        )
        OR (
            expected_through_application_id <> ''
            AND (
                expected_through_application_id !~ '^[1-9][0-9]{0,19}$'
                OR pg_catalog.length(expected_through_application_id) > 20
                OR expected_through_interaction_id !~ '^[1-9][0-9]{0,19}$'
                OR pg_catalog.length(expected_through_interaction_id) > 20
                OR expected_through_action_index NOT BETWEEN 0 AND 255
            )
        )
        OR (
            expected_through_application_id = ''
            AND expected_after_application_id <> ''
        )
        OR (
            expected_after_application_id <> ''
            AND ROW(
                expected_after_recovery_at,
                expected_after_application_id COLLATE "C",
                expected_after_interaction_id COLLATE "C",
                expected_after_action_index
            ) >= ROW(
                expected_through_recovery_at,
                expected_through_application_id COLLATE "C",
                expected_through_interaction_id COLLATE "C",
                expected_through_action_index
            )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_effect_response_tail_scan_input_invalid';
    END IF;

    database_now := pg_catalog.clock_timestamp();

    IF expected_through_application_id = '' THEN
        SELECT candidate.effective_recovery_at,
            candidate.application_id,
            candidate.interaction_id,
            candidate.action_index
        INTO cycle_through_recovery_at,
            cycle_through_application_id,
            cycle_through_interaction_id,
            cycle_through_action_index
        FROM (
            SELECT effect.application_id,
                effect.interaction_id,
                effect.action_index,
                pg_catalog.greatest(
                    COALESCE(effect.next_recovery_at, effect.updated_at),
                    receipt.claim_expires_at
                ) AS effective_recovery_at
            FROM public.runtime_interaction_effect_heads_v1 AS effect
            INNER JOIN public.runtime_interaction_receipt_heads_v1 AS receipt
                ON receipt.application_id = effect.application_id
                AND receipt.interaction_id = effect.interaction_id
            WHERE effect.action_kind = 'edit_response'
                AND effect.correlation_class = 'interaction_receipt'
                AND effect.correlation_marker IS NULL
                AND receipt.state = 'executing'
                AND receipt.claim_expires_at <= database_now
                AND effect.state IN (
                    'planned',
                    'intended',
                    'known_succeeded',
                    'known_failed',
                    'indeterminate',
                    'observing',
                    'observation_pending',
                    'reconciled_succeeded'
                )
                AND (
                    effect.next_recovery_at IS NULL
                    OR effect.next_recovery_at <= database_now
                )
                AND NOT EXISTS (
                    SELECT 1
                    FROM public.runtime_interaction_effect_rollbacks_v1 AS rollback
                    WHERE rollback.application_id = effect.application_id
                        AND rollback.interaction_id = effect.interaction_id
                )
                AND NOT EXISTS (
                    SELECT 1
                    FROM public.runtime_interaction_effect_heads_v1 AS mutable
                    WHERE mutable.application_id = effect.application_id
                        AND mutable.interaction_id = effect.interaction_id
                        AND mutable.action_kind <> 'edit_response'
                        AND mutable.state NOT IN (
                            'known_succeeded',
                            'reconciled_succeeded'
                        )
                )
        ) AS candidate
        WHERE candidate.effective_recovery_at <= database_now
        ORDER BY candidate.effective_recovery_at DESC,
            candidate.application_id COLLATE "C" DESC,
            candidate.interaction_id COLLATE "C" DESC,
            candidate.action_index DESC
        LIMIT 1;

        IF NOT FOUND THEN
            RETURN;
        END IF;
    ELSE
        cycle_through_recovery_at := expected_through_recovery_at;
        cycle_through_application_id := expected_through_application_id;
        cycle_through_interaction_id := expected_through_interaction_id;
        cycle_through_action_index := expected_through_action_index::SMALLINT;
    END IF;

    RETURN QUERY
    WITH eligible AS (
        SELECT effect.*,
            receipt.state AS receipt_state_value,
            receipt.head_revision AS receipt_head_revision_value,
            receipt.claim_revision AS receipt_claim_revision_value,
            receipt.claim_expires_at AS receipt_claim_expires_at_value,
            token.expires_at AS token_expires_at_value,
            pg_catalog.greatest(
                COALESCE(effect.next_recovery_at, effect.updated_at),
                receipt.claim_expires_at
            ) AS effective_recovery_at
        FROM public.runtime_interaction_effect_heads_v1 AS effect
        INNER JOIN public.runtime_interaction_receipt_heads_v1 AS receipt
            ON receipt.application_id = effect.application_id
            AND receipt.interaction_id = effect.interaction_id
        LEFT JOIN public.runtime_interaction_receipt_token_secrets_v1 AS token
            ON token.application_id = effect.application_id
            AND token.interaction_id = effect.interaction_id
        WHERE effect.action_kind = 'edit_response'
            AND effect.correlation_class = 'interaction_receipt'
            AND effect.correlation_marker IS NULL
            AND receipt.state = 'executing'
            AND receipt.claim_expires_at <= database_now
            AND effect.state IN (
                'planned',
                'intended',
                'known_succeeded',
                'known_failed',
                'indeterminate',
                'observing',
                'observation_pending',
                'reconciled_succeeded'
            )
            AND (
                effect.next_recovery_at IS NULL
                OR effect.next_recovery_at <= database_now
            )
            AND NOT EXISTS (
                SELECT 1
                FROM public.runtime_interaction_effect_rollbacks_v1 AS rollback
                WHERE rollback.application_id = effect.application_id
                    AND rollback.interaction_id = effect.interaction_id
            )
            AND NOT EXISTS (
                SELECT 1
                FROM public.runtime_interaction_effect_heads_v1 AS mutable
                WHERE mutable.application_id = effect.application_id
                    AND mutable.interaction_id = effect.interaction_id
                    AND mutable.action_kind <> 'edit_response'
                    AND mutable.state NOT IN (
                        'known_succeeded',
                        'reconciled_succeeded'
                    )
            )
    )
    SELECT effect.application_id,
        effect.interaction_id,
        effect.action_index,
        effect.state,
        effect.head_revision,
        effect.recovery_claim_revision,
        effect.observation_attempt_count,
        effect.planned_identity_digest,
        effect.input_digest,
        effect.expected_postimage_digest,
        effect.planned_recovery_input,
        effect.planned_preimage_digest,
        effect.planned_preimage,
        effect.resolved_input,
        effect.resolved_preimage_digest,
        effect.resolved_preimage,
        effect.resolved_effect_identity_digest,
        effect.intent_digest,
        effect.result_digest,
        effect.success_binding_kind,
        effect.success_binding_digest,
        effect.correlation_digest,
        effect_root.action_plan_digest,
        effect_root.preflight_certificate_digest,
        effect_root.snapshot_digest,
        effect.receipt_state_value,
        effect.receipt_head_revision_value,
        effect.receipt_claim_revision_value,
        effect.receipt_claim_expires_at_value,
        effect.token_expires_at_value,
        receipt_root.tenant_id,
        receipt_root.installation_id,
        receipt_root.deployment_id,
        receipt_root.attestation_id,
        receipt_root.attestation_digest,
        receipt_root.guild_id,
        receipt_root.channel_id,
        receipt_root.actor_user_id,
        receipt_root.interaction_kind,
        receipt_root.ruleset_key,
        receipt_root.target_version,
        receipt_root.target_content_hash,
        receipt_root.binding_revision,
        receipt_root.binding_fingerprint,
        receipt_root.runtime_generation,
        receipt_root.route_controller_fencing_token,
        receipt_root.route_incarnation,
        receipt_root.origin_process_instance_id,
        receipt_root.origin_serving_lease_epoch,
        receipt_root.origin_serving_revision,
        receipt_root.origin_gateway_shard_id,
        receipt_root.origin_gateway_owner_lease_epoch,
        receipt_root.origin_gateway_owner_revision,
        receipt_root.runtime_build_revision,
        receipt_root.route_kind,
        receipt_root.route_key,
        receipt_root.instance_id,
        receipt_root.execution_ruleset_version,
        receipt_root.execution_ruleset_content_hash,
        receipt_root.instance_manifest_digest,
        receipt_root.request_digest,
        effect.effective_recovery_at,
        cycle_through_recovery_at,
        cycle_through_application_id,
        cycle_through_interaction_id,
        cycle_through_action_index,
        database_now
    FROM eligible AS effect
    INNER JOIN public.runtime_interaction_effect_roots_v1 AS effect_root
        ON effect_root.application_id = effect.application_id
        AND effect_root.interaction_id = effect.interaction_id
    INNER JOIN public.runtime_interaction_receipt_roots_v1 AS receipt_root
        ON receipt_root.application_id = effect.application_id
        AND receipt_root.interaction_id = effect.interaction_id
    WHERE effect.effective_recovery_at <= database_now
        AND (
            expected_after_application_id = ''
            OR ROW(
                effect.effective_recovery_at,
                effect.application_id COLLATE "C",
                effect.interaction_id COLLATE "C",
                effect.action_index
            ) > ROW(
                expected_after_recovery_at,
                expected_after_application_id COLLATE "C",
                expected_after_interaction_id COLLATE "C",
                expected_after_action_index
            )
        )
        AND ROW(
            effect.effective_recovery_at,
            effect.application_id COLLATE "C",
            effect.interaction_id COLLATE "C",
            effect.action_index
        ) <= ROW(
            cycle_through_recovery_at,
            cycle_through_application_id COLLATE "C",
            cycle_through_interaction_id COLLATE "C",
            cycle_through_action_index
        )
    ORDER BY effect.effective_recovery_at,
        effect.application_id COLLATE "C",
        effect.interaction_id COLLATE "C",
        effect.action_index
    LIMIT expected_limit;
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_effect_scan_recoverable_v1(
    expected_after_recovery_at TIMESTAMPTZ,
    expected_after_application_id TEXT,
    expected_after_interaction_id TEXT,
    expected_after_action_index BIGINT,
    expected_through_recovery_at TIMESTAMPTZ,
    expected_through_application_id TEXT,
    expected_through_interaction_id TEXT,
    expected_through_action_index BIGINT,
    expected_limit BIGINT
)
RETURNS TABLE(
    application_id TEXT,
    interaction_id TEXT,
    action_index SMALLINT,
    action_kind TEXT,
    effect_state TEXT,
    effect_head_revision BIGINT,
    recovery_claim_revision BIGINT,
    attempt_count INTEGER,
    observation_attempt_count INTEGER,
    compensation_attempt_count INTEGER,
    compensation_observation_attempt_count INTEGER,
    dependency_indices SMALLINT[],
    planned_identity_digest BYTEA,
    input_digest BYTEA,
    expected_postimage_digest BYTEA,
    planned_recovery_input JSONB,
    planned_preimage_digest BYTEA,
    planned_preimage JSONB,
    resolved_input JSONB,
    resolved_preimage_digest BYTEA,
    resolved_preimage JSONB,
    resolved_effect_identity_digest BYTEA,
    resolved_instance_manifest_digest BYTEA,
    output_kind TEXT,
    output_id TEXT,
    correlation_class TEXT,
    correlation_digest BYTEA,
    correlation_marker TEXT,
    intent_digest BYTEA,
    result_digest BYTEA,
    success_binding_kind TEXT,
    success_binding_digest BYTEA,
    compensation_intent_digest BYTEA,
    compensation_result_digest BYTEA,
    next_recovery_at TIMESTAMPTZ,
    action_plan_digest BYTEA,
    preflight_certificate_digest BYTEA,
    snapshot_digest BYTEA,
    certificate_issued_at TIMESTAMPTZ,
    certificate_expires_at TIMESTAMPTZ,
    tenant_id TEXT,
    installation_id TEXT,
    deployment_id TEXT,
    attestation_id TEXT,
    attestation_digest TEXT,
    guild_id TEXT,
    channel_id TEXT,
    actor_user_id TEXT,
    interaction_kind TEXT,
    ruleset_key TEXT,
    target_version BIGINT,
    target_content_hash TEXT,
    binding_revision BIGINT,
    binding_fingerprint TEXT,
    runtime_generation BIGINT,
    route_controller_fencing_token BIGINT,
    route_incarnation BIGINT,
    origin_process_instance_id TEXT,
    origin_serving_lease_epoch BIGINT,
    origin_serving_revision BIGINT,
    origin_gateway_shard_id TEXT,
    origin_gateway_owner_lease_epoch BIGINT,
    origin_gateway_owner_revision BIGINT,
    runtime_build_revision TEXT,
    route_kind TEXT,
    route_key TEXT,
    instance_id TEXT,
    execution_ruleset_version BIGINT,
    execution_ruleset_content_hash TEXT,
    instance_manifest_digest TEXT,
    request_digest BYTEA,
    through_recovery_at TIMESTAMPTZ,
    through_application_id TEXT,
    through_interaction_id TEXT,
    through_action_index SMALLINT,
    observed_database_now TIMESTAMPTZ
)
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
ROWS 256
AS $function$
DECLARE
    cycle_through_recovery_at TIMESTAMPTZ;
    cycle_through_application_id TEXT;
    cycle_through_interaction_id TEXT;
    cycle_through_action_index SMALLINT;
    database_now TIMESTAMPTZ;
BEGIN
    IF NOT pg_catalog.isfinite(expected_after_recovery_at)
        OR NOT pg_catalog.isfinite(expected_through_recovery_at)
        OR expected_limit NOT BETWEEN 1 AND 256
        OR (
            (expected_after_application_id = '') IS DISTINCT FROM
                (expected_after_interaction_id = '')
        )
        OR (
            (expected_through_application_id = '') IS DISTINCT FROM
                (expected_through_interaction_id = '')
        )
        OR (
            expected_after_application_id = ''
            AND (
                expected_after_recovery_at
                    <> '1970-01-01 00:00:00+00'::TIMESTAMPTZ
                OR expected_after_action_index <> -1
            )
        )
        OR (
            expected_through_application_id = ''
            AND (
                expected_through_recovery_at
                    <> '1970-01-01 00:00:00+00'::TIMESTAMPTZ
                OR expected_through_action_index <> -1
            )
        )
        OR (
            expected_after_application_id <> ''
            AND (
                expected_after_application_id !~ '^[1-9][0-9]{0,19}$'
                OR pg_catalog.length(expected_after_application_id) > 20
                OR expected_after_interaction_id !~ '^[1-9][0-9]{0,19}$'
                OR pg_catalog.length(expected_after_interaction_id) > 20
                OR expected_after_action_index NOT BETWEEN 0 AND 255
            )
        )
        OR (
            expected_through_application_id <> ''
            AND (
                expected_through_application_id !~ '^[1-9][0-9]{0,19}$'
                OR pg_catalog.length(expected_through_application_id) > 20
                OR expected_through_interaction_id !~ '^[1-9][0-9]{0,19}$'
                OR pg_catalog.length(expected_through_interaction_id) > 20
                OR expected_through_action_index NOT BETWEEN 0 AND 255
            )
        )
        OR (
            expected_through_application_id = ''
            AND expected_after_application_id <> ''
        )
        OR (
            expected_after_application_id <> ''
            AND ROW(
                expected_after_recovery_at,
                expected_after_application_id COLLATE "C",
                expected_after_interaction_id COLLATE "C",
                expected_after_action_index
            ) >= ROW(
                expected_through_recovery_at,
                expected_through_application_id COLLATE "C",
                expected_through_interaction_id COLLATE "C",
                expected_through_action_index
            )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_effect_recovery_scan_input_invalid';
    END IF;

    database_now := pg_catalog.clock_timestamp();

    IF expected_through_application_id = '' THEN
        WITH eligible AS (
            SELECT head.*,
                head.next_recovery_at AS effective_recovery_at
            FROM public.runtime_interaction_effect_heads_v1 AS head
            INNER JOIN public.runtime_interaction_receipt_heads_v1 AS receipt
                ON receipt.application_id = head.application_id
                AND receipt.interaction_id = head.interaction_id
            WHERE head.action_kind <> 'edit_response'
                AND head.state IN (
                    'intended',
                    'indeterminate',
                    'observing',
                    'observation_pending',
                    'compensation_intended',
                    'compensation_indeterminate',
                    'compensation_observing',
                    'compensation_observation_pending'
                )
                AND head.next_recovery_at <= database_now
                AND (
                    receipt.state IN ('failed', 'recovery_required')
                    OR (
                        receipt.state = 'executing'
                        AND receipt.claim_expires_at <= database_now
                    )
                )
            UNION ALL
            SELECT head.*,
                rollback.required_at
                    + (255 - head.action_index) * INTERVAL '1 microsecond'
                    AS effective_recovery_at
            FROM public.runtime_interaction_effect_rollbacks_v1 AS rollback
            INNER JOIN public.runtime_interaction_effect_heads_v1 AS head
                ON head.application_id = rollback.application_id
                AND head.interaction_id = rollback.interaction_id
                AND head.action_index <= rollback.abort_action_index
            INNER JOIN public.runtime_interaction_receipt_heads_v1 AS receipt
                ON receipt.application_id = head.application_id
                AND receipt.interaction_id = head.interaction_id
            WHERE rollback.state = 'required'
                AND rollback.required_at <= database_now
                AND (
                    receipt.state IN ('failed', 'recovery_required')
                    OR (
                        receipt.state = 'executing'
                        AND receipt.claim_expires_at <= database_now
                    )
                )
                AND head.state IN (
                    'known_succeeded',
                    'reconciled_succeeded'
                )
                AND head.action_kind NOT IN (
                    'teardown_instance',
                    'edit_response'
                )
        )
        SELECT
            head.effective_recovery_at,
            head.application_id,
            head.interaction_id,
            head.action_index
        INTO
            cycle_through_recovery_at,
            cycle_through_application_id,
            cycle_through_interaction_id,
            cycle_through_action_index
        FROM eligible AS head
        WHERE head.effective_recovery_at <= database_now
        ORDER BY
            head.effective_recovery_at DESC,
            head.application_id COLLATE "C" DESC,
            head.interaction_id COLLATE "C" DESC,
            head.action_index DESC
        LIMIT 1;

        IF NOT FOUND THEN
            RETURN;
        END IF;
    ELSE
        cycle_through_recovery_at := expected_through_recovery_at;
        cycle_through_application_id := expected_through_application_id;
        cycle_through_interaction_id := expected_through_interaction_id;
        cycle_through_action_index := expected_through_action_index::SMALLINT;
    END IF;

    RETURN QUERY
    WITH eligible AS (
        SELECT candidate.*,
            candidate.next_recovery_at AS effective_recovery_at
        FROM public.runtime_interaction_effect_heads_v1 AS candidate
        INNER JOIN public.runtime_interaction_receipt_heads_v1 AS receipt
            ON receipt.application_id = candidate.application_id
            AND receipt.interaction_id = candidate.interaction_id
        WHERE candidate.action_kind <> 'edit_response'
            AND candidate.state IN (
                'intended',
                'indeterminate',
                'observing',
                'observation_pending',
                'compensation_intended',
                'compensation_indeterminate',
                'compensation_observing',
                'compensation_observation_pending'
            )
            AND candidate.next_recovery_at <= database_now
            AND (
                receipt.state IN ('failed', 'recovery_required')
                OR (
                    receipt.state = 'executing'
                    AND receipt.claim_expires_at <= database_now
                )
            )
        UNION ALL
        SELECT candidate.*,
            rollback.required_at
                + (255 - candidate.action_index) * INTERVAL '1 microsecond'
                AS effective_recovery_at
        FROM public.runtime_interaction_effect_rollbacks_v1 AS rollback
        INNER JOIN public.runtime_interaction_effect_heads_v1 AS candidate
            ON candidate.application_id = rollback.application_id
            AND candidate.interaction_id = rollback.interaction_id
            AND candidate.action_index <= rollback.abort_action_index
        INNER JOIN public.runtime_interaction_receipt_heads_v1 AS receipt
            ON receipt.application_id = candidate.application_id
            AND receipt.interaction_id = candidate.interaction_id
        WHERE rollback.state = 'required'
            AND rollback.required_at <= database_now
            AND (
                receipt.state IN ('failed', 'recovery_required')
                OR (
                    receipt.state = 'executing'
                    AND receipt.claim_expires_at <= database_now
                )
            )
            AND candidate.state IN (
                'known_succeeded',
                'reconciled_succeeded'
            )
            AND candidate.action_kind NOT IN (
                'teardown_instance',
                'edit_response'
            )
    )
    SELECT
        head.application_id,
        head.interaction_id,
        head.action_index,
        head.action_kind,
        head.state,
        head.head_revision,
        head.recovery_claim_revision,
        head.attempt_count,
        head.observation_attempt_count,
        head.compensation_attempt_count,
        head.compensation_observation_attempt_count,
        head.dependency_indices,
        head.planned_identity_digest,
        head.input_digest,
        head.expected_postimage_digest,
        head.planned_recovery_input,
        head.planned_preimage_digest,
        head.planned_preimage,
        head.resolved_input,
        head.resolved_preimage_digest,
        head.resolved_preimage,
        head.resolved_effect_identity_digest,
        head.resolved_instance_manifest_digest,
        head.output_kind,
        head.output_id,
        head.correlation_class,
        head.correlation_digest,
        head.correlation_marker,
        head.intent_digest,
        head.result_digest,
        head.success_binding_kind,
        head.success_binding_digest,
        head.compensation_intent_digest,
        head.compensation_result_digest,
        head.effective_recovery_at,
        effect_root.action_plan_digest,
        effect_root.preflight_certificate_digest,
        effect_root.snapshot_digest,
        effect_root.certificate_issued_at,
        effect_root.certificate_expires_at,
        receipt_root.tenant_id,
        receipt_root.installation_id,
        receipt_root.deployment_id,
        receipt_root.attestation_id,
        receipt_root.attestation_digest,
        receipt_root.guild_id,
        receipt_root.channel_id,
        receipt_root.actor_user_id,
        receipt_root.interaction_kind,
        receipt_root.ruleset_key,
        receipt_root.target_version,
        receipt_root.target_content_hash,
        receipt_root.binding_revision,
        receipt_root.binding_fingerprint,
        receipt_root.runtime_generation,
        receipt_root.route_controller_fencing_token,
        receipt_root.route_incarnation,
        receipt_root.origin_process_instance_id,
        receipt_root.origin_serving_lease_epoch,
        receipt_root.origin_serving_revision,
        receipt_root.origin_gateway_shard_id,
        receipt_root.origin_gateway_owner_lease_epoch,
        receipt_root.origin_gateway_owner_revision,
        receipt_root.runtime_build_revision,
        receipt_root.route_kind,
        receipt_root.route_key,
        receipt_root.instance_id,
        receipt_root.execution_ruleset_version,
        receipt_root.execution_ruleset_content_hash,
        receipt_root.instance_manifest_digest,
        receipt_root.request_digest,
        cycle_through_recovery_at,
        cycle_through_application_id,
        cycle_through_interaction_id,
        cycle_through_action_index,
        database_now
    FROM eligible AS head
    INNER JOIN public.runtime_interaction_effect_roots_v1 AS effect_root
        ON effect_root.application_id = head.application_id
        AND effect_root.interaction_id = head.interaction_id
    INNER JOIN public.runtime_interaction_receipt_roots_v1 AS receipt_root
        ON receipt_root.application_id = head.application_id
        AND receipt_root.interaction_id = head.interaction_id
    WHERE head.effective_recovery_at <= database_now
        AND (
            expected_after_application_id = ''
            OR ROW(
                head.effective_recovery_at,
                head.application_id COLLATE "C",
                head.interaction_id COLLATE "C",
                head.action_index
            ) > ROW(
                expected_after_recovery_at,
                expected_after_application_id COLLATE "C",
                expected_after_interaction_id COLLATE "C",
                expected_after_action_index
            )
        )
        AND ROW(
            head.effective_recovery_at,
            head.application_id COLLATE "C",
            head.interaction_id COLLATE "C",
            head.action_index
        ) <= ROW(
            cycle_through_recovery_at,
            cycle_through_application_id COLLATE "C",
            cycle_through_interaction_id COLLATE "C",
            cycle_through_action_index
        )
    ORDER BY
        head.effective_recovery_at,
        head.application_id COLLATE "C",
        head.interaction_id COLLATE "C",
        head.action_index
    LIMIT expected_limit;
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_effect_recovery_claim_v1(
    expected_application_id TEXT,
    expected_interaction_id TEXT,
    expected_action_index BIGINT,
    expected_effect_head_revision BIGINT,
    expected_process_instance_id TEXT,
    expected_gateway_shard_id TEXT,
    expected_runtime_build_revision TEXT,
    expected_runtime_generation BIGINT,
    expected_controller_fencing_token BIGINT,
    expected_route_incarnation BIGINT,
    requested_claim_lease_milliseconds BIGINT
)
RETURNS TABLE(
    outcome_name TEXT,
    effect_state TEXT,
    resulting_effect_head_revision BIGINT,
    resulting_recovery_claim_revision BIGINT,
    resulting_recovery_claim_expires_at TIMESTAMPTZ,
    observed_database_now TIMESTAMPTZ
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
    receipt_root public.runtime_interaction_receipt_roots_v1%ROWTYPE;
    receipt_head public.runtime_interaction_receipt_heads_v1%ROWTYPE;
    effect_root public.runtime_interaction_effect_roots_v1%ROWTYPE;
    rollback_row public.runtime_interaction_effect_rollbacks_v1%ROWTYPE;
    effect_head public.runtime_interaction_effect_heads_v1%ROWTYPE;
    database_now TIMESTAMPTZ;
    claim_expiry TIMESTAMPTZ;
    authority_available BOOLEAN;
    observation_state TEXT;
    observation_event_kind TEXT;
    rollback_authorized BOOLEAN := FALSE;
    blocked_digest BYTEA;
    budget_source_state TEXT;
    budget_path TEXT;
    budget_attempt_count INTEGER;
BEGIN
    IF expected_application_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_application_id) > 20
        OR expected_interaction_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_interaction_id) > 20
        OR expected_action_index NOT BETWEEN 0 AND 255
        OR expected_effect_head_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_gateway_shard_id !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR expected_runtime_build_revision !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR expected_runtime_generation NOT BETWEEN 1 AND 9223372036854775807
        OR expected_controller_fencing_token
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_route_incarnation NOT BETWEEN 1 AND 9223372036854775807
        OR requested_claim_lease_milliseconds NOT BETWEEN 1000 AND 300000
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_effect_recovery_claim_input_invalid';
    END IF;

    database_now := pg_catalog.clock_timestamp();
    claim_expiry := database_now
        + requested_claim_lease_milliseconds * INTERVAL '1 millisecond';

    SELECT root.*
    INTO receipt_root
    FROM public.runtime_interaction_receipt_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    IF NOT FOUND
        OR receipt_root.runtime_generation <> expected_runtime_generation
        OR receipt_root.route_controller_fencing_token
            <> expected_controller_fencing_token
        OR receipt_root.route_incarnation <> expected_route_incarnation
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_effect_recovery_authority_stale';
    END IF;

    SELECT head.*
    INTO receipt_head
    FROM public.runtime_interaction_receipt_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
    FOR UPDATE;

    IF NOT FOUND OR receipt_head.state = 'completed' THEN
        RAISE EXCEPTION USING
            ERRCODE = CASE WHEN receipt_head.state = 'completed'
                THEN 'RI001'
                ELSE 'RI002'
            END,
            MESSAGE = CASE WHEN receipt_head.state = 'completed'
                THEN 'runtime_interaction_effect_recovery_claim_conflict'
                ELSE 'runtime_interaction_effect_receipt_head_missing'
            END;
    END IF;

    SELECT EXISTS (
        SELECT 1
        FROM public.starring_runtime_interaction_receipt_authority_observe_v1(
            receipt_root.application_id,
            receipt_root.tenant_id,
            receipt_root.installation_id,
            receipt_root.deployment_id,
            receipt_root.guild_id,
            receipt_root.ruleset_key,
            receipt_root.target_version,
            receipt_root.target_content_hash,
            receipt_root.binding_revision,
            receipt_root.binding_fingerprint,
            receipt_root.runtime_generation,
            receipt_root.route_controller_fencing_token,
            receipt_root.route_incarnation,
            expected_process_instance_id,
            expected_gateway_shard_id,
            expected_runtime_build_revision,
            receipt_root.route_kind,
            COALESCE(receipt_root.instance_id, '')
        ) AS authority
    )
    INTO authority_available;

    IF NOT authority_available THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_effect_recovery_authority_stale';
    END IF;

    SELECT root.*
    INTO effect_root
    FROM public.runtime_interaction_effect_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_plan_not_found';
    END IF;

    SELECT rollback.*
    INTO rollback_row
    FROM public.runtime_interaction_effect_rollbacks_v1 AS rollback
    WHERE rollback.application_id = expected_application_id
        AND rollback.interaction_id = expected_interaction_id
    FOR UPDATE;

    rollback_authorized := FOUND
        AND rollback_row.state = 'required'
        AND expected_action_index <= rollback_row.abort_action_index;

    SELECT head.*
    INTO effect_head
    FROM public.runtime_interaction_effect_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
        AND head.action_index = expected_action_index
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_action_not_found';
    END IF;

    IF (
            effect_head.action_kind = 'edit_response'
            AND (
                receipt_head.state <> 'executing'
                OR receipt_head.claim_expires_at > database_now
            )
        )
        OR (
            effect_head.action_kind <> 'edit_response'
            AND (
                receipt_head.state NOT IN (
                    'executing',
                    'failed',
                    'recovery_required'
                )
                OR (
                    receipt_head.state = 'executing'
                    AND receipt_head.claim_expires_at > database_now
                )
            )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_effect_recovery_claim_active_receipt';
    END IF;

    observation_state := CASE
        WHEN effect_head.state IN (
            'compensation_intended',
            'compensation_indeterminate',
            'compensation_observing',
            'compensation_observation_pending'
        ) THEN 'compensation_observing'
        ELSE 'observing'
    END;
    observation_event_kind := CASE observation_state
        WHEN 'compensation_observing'
            THEN 'compensation_observation_claimed'
        ELSE 'recovery_claimed'
    END;

    IF observation_state = 'compensation_observing'
        AND NOT rollback_authorized
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_compensation_not_authorized';
    END IF;

    budget_source_state := effect_head.state;
    IF effect_head.state = 'recovery_required'
        AND effect_head.head_revision = expected_effect_head_revision + 1
    THEN
        SELECT event.from_state
        INTO budget_source_state
        FROM public.runtime_interaction_effect_events_v1 AS event
        WHERE event.application_id = expected_application_id
            AND event.interaction_id = expected_interaction_id
            AND event.action_index = expected_action_index
            AND event.event_revision = effect_head.head_revision
            AND event.event_kind = 'recovery_required'
            AND event.outcome_code
                = 'recovery_blocked_attempt_budget_exhausted';
    END IF;
    budget_path := CASE
        WHEN budget_source_state LIKE 'compensation_%'
            THEN 'compensation_observation'
        ELSE 'observation'
    END;
    budget_attempt_count := CASE budget_path
        WHEN 'compensation_observation'
            THEN effect_head.compensation_observation_attempt_count
        ELSE effect_head.observation_attempt_count
    END;

    blocked_digest := pg_catalog.sha256(pg_catalog.convert_to(
        pg_catalog.concat_ws(
            '|',
            'starring-runtime-interaction-effect-attempt-budget-block-v1',
            expected_application_id,
            expected_interaction_id,
            expected_action_index::TEXT,
            expected_effect_head_revision::TEXT,
            effect_head.recovery_claim_revision::TEXT,
            expected_process_instance_id,
            expected_gateway_shard_id,
            expected_runtime_build_revision,
            expected_runtime_generation::TEXT,
            expected_controller_fencing_token::TEXT,
            expected_route_incarnation::TEXT,
            budget_source_state,
            budget_path,
            budget_attempt_count::TEXT,
            pg_catalog.encode(
                effect_root.preflight_certificate_digest,
                'hex'
            ),
            'recovery_blocked_attempt_budget_exhausted'
        ),
        'UTF8'
    ));

    IF effect_head.action_kind <> 'edit_response'
        AND effect_head.state = 'recovery_required'
        AND effect_head.head_revision = expected_effect_head_revision + 1
        AND EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_events_v1 AS event
            WHERE event.application_id = expected_application_id
                AND event.interaction_id = expected_interaction_id
                AND event.action_index = expected_action_index
                AND event.event_revision = effect_head.head_revision
                AND event.event_kind = 'recovery_required'
                AND event.from_state = budget_source_state
                AND event.to_state = 'recovery_required'
                AND event.recovery_claim_revision
                    = effect_head.recovery_claim_revision
                AND event.process_instance_id = expected_process_instance_id
                AND event.outcome_code
                    = 'recovery_blocked_attempt_budget_exhausted'
                AND event.result_digest = blocked_digest
                AND event.output_kind = effect_head.output_kind
                AND event.output_id IS NULL
                AND event.event_digest = pg_catalog.sha256(
                    pg_catalog.convert_to(
                        pg_catalog.concat_ws(
                            '|',
                            'starring-runtime-interaction-effect-event-v1',
                            expected_application_id,
                            expected_interaction_id,
                            expected_action_index::TEXT,
                            (expected_effect_head_revision + 1)::TEXT,
                            'recovery_required',
                            event.from_state,
                            'recovery_required',
                            effect_head.recovery_claim_revision::TEXT,
                            expected_process_instance_id,
                            'recovery_blocked_attempt_budget_exhausted',
                            pg_catalog.encode(blocked_digest, 'hex')
                        ),
                        'UTF8'
                    )
                )
                AND (
                    (
                        budget_path = 'observation'
                        AND budget_source_state IN (
                            'intended',
                            'indeterminate',
                            'observing',
                            'observation_pending'
                        )
                        AND effect_head.observation_attempt_count >= 64
                    )
                    OR (
                        budget_path = 'compensation_observation'
                        AND budget_source_state LIKE 'compensation_%'
                        AND effect_head.compensation_observation_attempt_count >= 64
                        AND effect_head.compensation_result_digest IS NOT NULL
                    )
                )
        )
    THEN
        outcome_name := 'exact_replay';
        effect_state := effect_head.state;
        resulting_effect_head_revision := effect_head.head_revision;
        resulting_recovery_claim_revision :=
            effect_head.recovery_claim_revision;
        resulting_recovery_claim_expires_at := database_now;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF effect_head.state = observation_state
        AND effect_head.head_revision IN (
            expected_effect_head_revision,
            expected_effect_head_revision + 1
        )
        AND effect_head.recovery_process_instance_id
            IS NOT DISTINCT FROM expected_process_instance_id
        AND effect_head.recovery_gateway_shard_id
            IS NOT DISTINCT FROM expected_gateway_shard_id
        AND effect_head.recovery_runtime_build_revision
            IS NOT DISTINCT FROM expected_runtime_build_revision
        AND effect_head.recovery_expires_at > database_now
    THEN
        outcome_name := 'exact_replay';
        effect_state := effect_head.state;
        resulting_effect_head_revision := effect_head.head_revision;
        resulting_recovery_claim_revision :=
            effect_head.recovery_claim_revision;
        resulting_recovery_claim_expires_at :=
            effect_head.recovery_expires_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF effect_head.action_kind <> 'edit_response'
        AND effect_head.head_revision = expected_effect_head_revision
        AND effect_head.state IN (
            'intended',
            'indeterminate',
            'observing',
            'observation_pending',
            'compensation_intended',
            'compensation_indeterminate',
            'compensation_observing',
            'compensation_observation_pending'
        )
        AND effect_head.next_recovery_at IS NOT NULL
        AND effect_head.next_recovery_at <= database_now
        AND (
            (
                observation_state = 'observing'
                AND effect_head.observation_attempt_count >= 64
            )
            OR (
                observation_state = 'compensation_observing'
                AND effect_head.compensation_observation_attempt_count >= 64
            )
        )
    THEN
        UPDATE public.runtime_interaction_effect_heads_v1 AS head
        SET state = 'recovery_required',
            head_revision = head.head_revision + 1,
            result_digest = COALESCE(head.result_digest, blocked_digest),
            result_at = COALESCE(head.result_at, database_now),
            compensation_result_digest = CASE
                WHEN observation_state = 'compensation_observing'
                    THEN COALESCE(
                        head.compensation_result_digest,
                        blocked_digest
                    )
                ELSE head.compensation_result_digest
            END,
            compensation_result_at = CASE
                WHEN observation_state = 'compensation_observing'
                    THEN COALESCE(
                        head.compensation_result_at,
                        database_now
                    )
                ELSE head.compensation_result_at
            END,
            recovery_process_instance_id = NULL,
            recovery_gateway_shard_id = NULL,
            recovery_runtime_build_revision = NULL,
            recovery_acquired_at = NULL,
            recovery_expires_at = NULL,
            next_recovery_at = NULL,
            updated_at = database_now
        WHERE head.application_id = expected_application_id
            AND head.interaction_id = expected_interaction_id
            AND head.action_index = expected_action_index;

        INSERT INTO public.runtime_interaction_effect_events_v1 (
            application_id,
            interaction_id,
            action_index,
            event_revision,
            event_kind,
            from_state,
            to_state,
            receipt_claim_revision,
            recovery_claim_revision,
            process_instance_id,
            outcome_code,
            result_digest,
            output_kind,
            output_id,
            event_digest,
            observed_at
        ) VALUES (
            expected_application_id,
            expected_interaction_id,
            expected_action_index,
            effect_head.head_revision + 1,
            'recovery_required',
            effect_head.state,
            'recovery_required',
            NULL,
            effect_head.recovery_claim_revision,
            expected_process_instance_id,
            'recovery_blocked_attempt_budget_exhausted',
            blocked_digest,
            effect_head.output_kind,
            NULL,
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.concat_ws(
                    '|',
                    'starring-runtime-interaction-effect-event-v1',
                    expected_application_id,
                    expected_interaction_id,
                    expected_action_index::TEXT,
                    (effect_head.head_revision + 1)::TEXT,
                    'recovery_required',
                    effect_head.state,
                    'recovery_required',
                    effect_head.recovery_claim_revision::TEXT,
                    expected_process_instance_id,
                    'recovery_blocked_attempt_budget_exhausted',
                    pg_catalog.encode(blocked_digest, 'hex')
                ),
                'UTF8'
            )),
            database_now
        );

        IF observation_state = 'observing' THEN
            PERFORM public.starring_runtime_interaction_effect_require_rollback_v1(
                expected_application_id,
                expected_interaction_id,
                'recovery_required',
                database_now
            );
        END IF;

        outcome_name := 'recovery_blocked_attempt_budget_exhausted';
        effect_state := 'recovery_required';
        resulting_effect_head_revision := effect_head.head_revision + 1;
        resulting_recovery_claim_revision :=
            effect_head.recovery_claim_revision;
        resulting_recovery_claim_expires_at := database_now;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF effect_head.head_revision <> expected_effect_head_revision
        OR effect_head.state NOT IN (
            'intended',
            'indeterminate',
            'observing',
            'observation_pending',
            'compensation_intended',
            'compensation_indeterminate',
            'compensation_observing',
            'compensation_observation_pending'
        )
        OR effect_head.next_recovery_at IS NULL
        OR effect_head.next_recovery_at > database_now
        OR effect_head.recovery_claim_revision = 9223372036854775807
        OR (
            effect_head.action_kind = 'edit_response'
            AND effect_head.observation_attempt_count >= 64
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_recovery_claim_conflict';
    END IF;

    UPDATE public.runtime_interaction_effect_heads_v1 AS head
    SET state = observation_state,
        head_revision = head.head_revision + 1,
        observation_attempt_count = CASE
            WHEN observation_state = 'observing'
                THEN head.observation_attempt_count + 1
            ELSE head.observation_attempt_count
        END,
        compensation_observation_attempt_count = CASE
            WHEN observation_state = 'compensation_observing'
                THEN head.compensation_observation_attempt_count + 1
            ELSE head.compensation_observation_attempt_count
        END,
        recovery_claim_revision = head.recovery_claim_revision + 1,
        recovery_process_instance_id = expected_process_instance_id,
        recovery_gateway_shard_id = expected_gateway_shard_id,
        recovery_runtime_build_revision = expected_runtime_build_revision,
        recovery_acquired_at = database_now,
        recovery_expires_at = claim_expiry,
        next_recovery_at = claim_expiry,
        updated_at = database_now
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
        AND head.action_index = expected_action_index;

    INSERT INTO public.runtime_interaction_effect_events_v1 (
        application_id,
        interaction_id,
        action_index,
        event_revision,
        event_kind,
        from_state,
        to_state,
        receipt_claim_revision,
        recovery_claim_revision,
        process_instance_id,
        outcome_code,
        result_digest,
        output_kind,
        output_id,
        event_digest,
        observed_at
    ) VALUES (
        expected_application_id,
        expected_interaction_id,
        expected_action_index,
        effect_head.head_revision + 1,
        observation_event_kind,
        effect_head.state,
        observation_state,
        NULL,
        effect_head.recovery_claim_revision + 1,
        expected_process_instance_id,
        observation_event_kind,
        COALESCE(effect_head.result_digest, effect_head.input_digest),
        effect_head.output_kind,
        effect_head.output_id,
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.concat_ws(
                '|',
                'starring-runtime-interaction-effect-event-v1',
                expected_application_id,
                expected_interaction_id,
                expected_action_index::TEXT,
                (effect_head.head_revision + 1)::TEXT,
                observation_event_kind,
                effect_head.state,
                observation_state,
                (effect_head.recovery_claim_revision + 1)::TEXT,
                expected_process_instance_id,
                expected_gateway_shard_id,
                expected_runtime_build_revision
            ),
            'UTF8'
        )),
        database_now
    );

    outcome_name := observation_event_kind;
    effect_state := observation_state;
    resulting_effect_head_revision := effect_head.head_revision + 1;
    resulting_recovery_claim_revision :=
        effect_head.recovery_claim_revision + 1;
    resulting_recovery_claim_expires_at := claim_expiry;
    observed_database_now := database_now;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_effect_response_tail_claim_v1(
    expected_application_id TEXT,
    expected_interaction_id TEXT,
    expected_action_index BIGINT,
    expected_effect_head_revision BIGINT,
    expected_process_instance_id TEXT,
    expected_gateway_shard_id TEXT,
    expected_runtime_build_revision TEXT,
    expected_runtime_generation BIGINT,
    expected_controller_fencing_token BIGINT,
    expected_route_incarnation BIGINT,
    expected_preflight_certificate_digest BYTEA,
    expected_postimage_digest BYTEA,
    proposed_unrecoverable_digest BYTEA,
    requested_claim_lease_milliseconds BIGINT
)
RETURNS TABLE(
    outcome_name TEXT,
    effect_state TEXT,
    resulting_effect_head_revision BIGINT,
    resulting_recovery_claim_revision BIGINT,
    resulting_observation_attempt_count INTEGER,
    resulting_recovery_claim_expires_at TIMESTAMPTZ,
    receipt_state TEXT,
    resulting_receipt_head_revision BIGINT,
    token_encryption_suite TEXT,
    token_suite_version SMALLINT,
    token_key_id TEXT,
    token_nonce BYTEA,
    token_ciphertext BYTEA,
    token_aad_digest BYTEA,
    token_issued_at TIMESTAMPTZ,
    token_expires_at TIMESTAMPTZ,
    observed_database_now TIMESTAMPTZ
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
    claim_result RECORD;
    receipt_root public.runtime_interaction_receipt_roots_v1%ROWTYPE;
    effect_root public.runtime_interaction_effect_roots_v1%ROWTYPE;
    effect_head public.runtime_interaction_effect_heads_v1%ROWTYPE;
    receipt_head public.runtime_interaction_receipt_heads_v1%ROWTYPE;
    token_row public.runtime_interaction_receipt_token_secrets_v1%ROWTYPE;
    database_now TIMESTAMPTZ;
    resulting_digest BYTEA;
    budget_digest BYTEA;
    budget_source_state TEXT;
    authority_available BOOLEAN;
    receipt_revision BIGINT;
BEGIN
    IF expected_application_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_application_id) > 20
        OR expected_interaction_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_interaction_id) > 20
        OR expected_action_index NOT BETWEEN 0 AND 255
        OR expected_effect_head_revision
            NOT BETWEEN 1 AND 9223372036854775805
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_gateway_shard_id !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR expected_runtime_build_revision !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR expected_runtime_generation NOT BETWEEN 1 AND 9223372036854775807
        OR expected_controller_fencing_token
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_route_incarnation NOT BETWEEN 1 AND 9223372036854775807
        OR requested_claim_lease_milliseconds NOT BETWEEN 1000 AND 300000
        OR pg_catalog.octet_length(expected_preflight_certificate_digest) <> 32
        OR pg_catalog.octet_length(expected_postimage_digest) <> 32
        OR pg_catalog.octet_length(proposed_unrecoverable_digest) <> 32
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_effect_response_tail_claim_input_invalid';
    END IF;

    database_now := pg_catalog.clock_timestamp();

    SELECT root.*
    INTO receipt_root
    FROM public.runtime_interaction_receipt_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    SELECT head.*
    INTO receipt_head
    FROM public.runtime_interaction_receipt_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
    FOR UPDATE;

    IF FOUND AND receipt_head.state = 'completed' THEN
        SELECT root.*
        INTO effect_root
        FROM public.runtime_interaction_effect_roots_v1 AS root
        WHERE root.application_id = expected_application_id
            AND root.interaction_id = expected_interaction_id
        FOR KEY SHARE;

        SELECT head.*
        INTO effect_head
        FROM public.runtime_interaction_effect_heads_v1 AS head
        WHERE head.application_id = expected_application_id
            AND head.interaction_id = expected_interaction_id
            AND head.action_index = expected_action_index
        FOR UPDATE;

        SELECT event.from_state
        INTO budget_source_state
        FROM public.runtime_interaction_effect_events_v1 AS event
        WHERE event.application_id = expected_application_id
            AND event.interaction_id = expected_interaction_id
            AND event.action_index = expected_action_index
            AND event.event_revision = expected_effect_head_revision + 1
            AND event.event_kind = 'recovery_required'
            AND event.outcome_code
                = 'recovery_blocked_attempt_budget_exhausted';

        budget_digest := pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.concat_ws(
                '|',
                'starring-runtime-interaction-effect-attempt-budget-block-v1',
                expected_application_id,
                expected_interaction_id,
                expected_action_index::TEXT,
                expected_effect_head_revision::TEXT,
                effect_head.recovery_claim_revision::TEXT,
                expected_process_instance_id,
                expected_gateway_shard_id,
                expected_runtime_build_revision,
                expected_runtime_generation::TEXT,
                expected_controller_fencing_token::TEXT,
                expected_route_incarnation::TEXT,
                budget_source_state,
                'response_observation',
                effect_head.observation_attempt_count::TEXT,
                pg_catalog.encode(
                    effect_root.preflight_certificate_digest,
                    'hex'
                ),
                'recovery_blocked_attempt_budget_exhausted'
            ),
            'UTF8'
        ));

        IF receipt_root.application_id IS NULL
            OR receipt_root.runtime_generation <> expected_runtime_generation
            OR receipt_root.route_controller_fencing_token
                <> expected_controller_fencing_token
            OR receipt_root.route_incarnation <> expected_route_incarnation
            OR effect_root.application_id IS NULL
            OR effect_root.preflight_certificate_digest
                IS DISTINCT FROM expected_preflight_certificate_digest
            OR effect_head.application_id IS NULL
            OR effect_head.action_kind <> 'edit_response'
            OR effect_head.output_kind <> 'original_response'
            OR effect_head.correlation_class <> 'interaction_receipt'
            OR effect_head.correlation_marker IS NOT NULL
            OR effect_head.expected_postimage_digest
                IS DISTINCT FROM expected_postimage_digest
            OR effect_head.state <> 'recovery_required'
            OR receipt_head.terminal_outcome_code
                <> 'interaction_response_unrecoverable'
            OR NOT (
                (
                    effect_head.head_revision
                        = expected_effect_head_revision + 2
                    AND receipt_head.terminal_result_digest
                        IS NOT DISTINCT FROM proposed_unrecoverable_digest
                    AND EXISTS (
                        SELECT 1
                        FROM public.runtime_interaction_effect_events_v1 AS event
                        WHERE event.application_id = expected_application_id
                            AND event.interaction_id = expected_interaction_id
                            AND event.action_index = expected_action_index
                            AND event.event_revision
                                = expected_effect_head_revision + 1
                            AND event.event_kind = 'recovery_claimed'
                            AND event.to_state = 'observing'
                            AND event.recovery_claim_revision
                                = effect_head.recovery_claim_revision
                            AND event.process_instance_id
                                = expected_process_instance_id
                            AND event.outcome_code = 'recovery_claimed'
                            AND event.event_digest = pg_catalog.sha256(
                                pg_catalog.convert_to(
                                    pg_catalog.concat_ws(
                                        '|',
                                        'starring-runtime-interaction-effect-event-v1',
                                        expected_application_id,
                                        expected_interaction_id,
                                        expected_action_index::TEXT,
                                        (expected_effect_head_revision + 1)::TEXT,
                                        'recovery_claimed',
                                        event.from_state,
                                        'observing',
                                        effect_head.recovery_claim_revision::TEXT,
                                        expected_process_instance_id,
                                        expected_gateway_shard_id,
                                        expected_runtime_build_revision
                                    ),
                                    'UTF8'
                                )
                            )
                    )
                    AND EXISTS (
                        SELECT 1
                        FROM public.runtime_interaction_effect_events_v1 AS event
                        WHERE event.application_id = expected_application_id
                            AND event.interaction_id = expected_interaction_id
                            AND event.action_index = expected_action_index
                            AND event.event_revision
                                = expected_effect_head_revision + 2
                            AND event.event_kind = 'recovery_required'
                            AND event.from_state = 'observing'
                            AND event.to_state = 'recovery_required'
                            AND event.recovery_claim_revision
                                = effect_head.recovery_claim_revision
                            AND event.process_instance_id
                                = expected_process_instance_id
                            AND event.outcome_code
                                = 'interaction_response_unrecoverable'
                            AND event.result_digest
                                = proposed_unrecoverable_digest
                            AND event.output_kind = effect_head.output_kind
                            AND event.output_id IS NULL
                            AND event.event_digest = pg_catalog.sha256(
                                pg_catalog.convert_to(
                                    pg_catalog.concat_ws(
                                        '|',
                                        'starring-runtime-interaction-effect-event-v1',
                                        expected_application_id,
                                        expected_interaction_id,
                                        expected_action_index::TEXT,
                                        (expected_effect_head_revision + 2)::TEXT,
                                        'recovery_required',
                                        'observing',
                                        'recovery_required',
                                        effect_head.recovery_claim_revision::TEXT,
                                        expected_process_instance_id,
                                        'interaction_response_unrecoverable',
                                        pg_catalog.encode(
                                            proposed_unrecoverable_digest,
                                            'hex'
                                        )
                                    ),
                                    'UTF8'
                                )
                            )
                    )
                )
                OR (
                    effect_head.head_revision
                        = expected_effect_head_revision + 1
                    AND effect_head.observation_attempt_count >= 64
                    AND receipt_head.terminal_result_digest
                        IS NOT DISTINCT FROM budget_digest
                    AND EXISTS (
                        SELECT 1
                        FROM public.runtime_interaction_effect_events_v1 AS event
                        WHERE event.application_id = expected_application_id
                            AND event.interaction_id = expected_interaction_id
                            AND event.action_index = expected_action_index
                            AND event.event_revision
                                = expected_effect_head_revision + 1
                            AND event.event_kind = 'recovery_required'
                            AND event.from_state IN (
                                'intended',
                                'indeterminate',
                                'observing',
                                'observation_pending'
                            )
                            AND event.to_state = 'recovery_required'
                            AND event.recovery_claim_revision
                                = effect_head.recovery_claim_revision
                            AND event.process_instance_id
                                = expected_process_instance_id
                            AND event.outcome_code
                                = 'recovery_blocked_attempt_budget_exhausted'
                            AND event.result_digest
                                = budget_digest
                            AND event.output_kind = effect_head.output_kind
                            AND event.output_id IS NULL
                            AND event.event_digest = pg_catalog.sha256(
                                pg_catalog.convert_to(
                                    pg_catalog.concat_ws(
                                        '|',
                                        'starring-runtime-interaction-effect-event-v1',
                                        expected_application_id,
                                        expected_interaction_id,
                                        expected_action_index::TEXT,
                                        (expected_effect_head_revision + 1)::TEXT,
                                        'recovery_required',
                                        event.from_state,
                                        'recovery_required',
                                        effect_head.recovery_claim_revision::TEXT,
                                        expected_process_instance_id,
                                        'recovery_blocked_attempt_budget_exhausted',
                                        pg_catalog.encode(
                                            budget_digest,
                                            'hex'
                                        )
                                    ),
                                    'UTF8'
                                )
                            )
                    )
                )
            )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI001',
                MESSAGE = 'runtime_interaction_effect_response_tail_claim_conflict';
        END IF;

        outcome_name := 'interaction_response_unrecoverable';
        effect_state := effect_head.state;
        resulting_effect_head_revision := effect_head.head_revision;
        resulting_recovery_claim_revision :=
            effect_head.recovery_claim_revision;
        resulting_observation_attempt_count :=
            effect_head.observation_attempt_count;
        resulting_recovery_claim_expires_at := database_now;
        receipt_state := receipt_head.state;
        resulting_receipt_head_revision := receipt_head.head_revision;
        token_encryption_suite := NULL;
        token_suite_version := NULL;
        token_key_id := NULL;
        token_nonce := NULL;
        token_ciphertext := NULL;
        token_aad_digest := NULL;
        token_issued_at := NULL;
        token_expires_at := NULL;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF receipt_root.application_id IS NULL
        OR receipt_root.runtime_generation <> expected_runtime_generation
        OR receipt_root.route_controller_fencing_token
            <> expected_controller_fencing_token
        OR receipt_root.route_incarnation <> expected_route_incarnation
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_effect_recovery_authority_stale';
    END IF;

    SELECT EXISTS (
        SELECT 1
        FROM public.starring_runtime_interaction_receipt_authority_observe_v1(
            receipt_root.application_id,
            receipt_root.tenant_id,
            receipt_root.installation_id,
            receipt_root.deployment_id,
            receipt_root.guild_id,
            receipt_root.ruleset_key,
            receipt_root.target_version,
            receipt_root.target_content_hash,
            receipt_root.binding_revision,
            receipt_root.binding_fingerprint,
            receipt_root.runtime_generation,
            receipt_root.route_controller_fencing_token,
            receipt_root.route_incarnation,
            expected_process_instance_id,
            expected_gateway_shard_id,
            expected_runtime_build_revision,
            receipt_root.route_kind,
            COALESCE(receipt_root.instance_id, '')
        ) AS authority
    )
    INTO authority_available;

    IF NOT authority_available THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_effect_recovery_authority_stale';
    END IF;

    SELECT root.*
    INTO effect_root
    FROM public.runtime_interaction_effect_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    SELECT head.*
    INTO effect_head
    FROM public.runtime_interaction_effect_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
        AND head.action_index = expected_action_index
    FOR UPDATE;

    budget_source_state := effect_head.state;

    budget_digest := pg_catalog.sha256(pg_catalog.convert_to(
        pg_catalog.concat_ws(
            '|',
            'starring-runtime-interaction-effect-attempt-budget-block-v1',
            expected_application_id,
            expected_interaction_id,
            expected_action_index::TEXT,
            expected_effect_head_revision::TEXT,
            effect_head.recovery_claim_revision::TEXT,
            expected_process_instance_id,
            expected_gateway_shard_id,
            expected_runtime_build_revision,
            expected_runtime_generation::TEXT,
            expected_controller_fencing_token::TEXT,
            expected_route_incarnation::TEXT,
            budget_source_state,
            'response_observation',
            effect_head.observation_attempt_count::TEXT,
            pg_catalog.encode(
                effect_root.preflight_certificate_digest,
                'hex'
            ),
            'recovery_blocked_attempt_budget_exhausted'
        ),
        'UTF8'
    ));

    IF receipt_head.state = 'executing'
        AND receipt_head.claim_expires_at <= database_now
        AND receipt_head.acknowledgement_state IN (
            'unacknowledged',
            'deferred',
            'responded'
        )
        AND effect_root.application_id IS NOT NULL
        AND effect_root.preflight_certificate_digest
            IS NOT DISTINCT FROM expected_preflight_certificate_digest
        AND effect_head.application_id IS NOT NULL
        AND effect_head.action_kind = 'edit_response'
        AND effect_head.output_kind = 'original_response'
        AND effect_head.correlation_class = 'interaction_receipt'
        AND effect_head.correlation_marker IS NULL
        AND effect_head.expected_postimage_digest
            IS NOT DISTINCT FROM expected_postimage_digest
        AND effect_head.head_revision = expected_effect_head_revision
        AND effect_head.state IN (
            'intended',
            'indeterminate',
            'observing',
            'observation_pending'
        )
        AND effect_head.next_recovery_at IS NOT NULL
        AND effect_head.next_recovery_at <= database_now
        AND effect_head.observation_attempt_count >= 64
        AND NOT EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_rollbacks_v1 AS rollback
            WHERE rollback.application_id = expected_application_id
                AND rollback.interaction_id = expected_interaction_id
        )
        AND NOT EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_heads_v1 AS mutable
            WHERE mutable.application_id = expected_application_id
                AND mutable.interaction_id = expected_interaction_id
                AND mutable.action_kind <> 'edit_response'
                AND mutable.state NOT IN (
                    'known_succeeded',
                    'reconciled_succeeded'
                )
        )
    THEN
        UPDATE public.runtime_interaction_effect_heads_v1 AS head
        SET state = 'recovery_required',
            head_revision = head.head_revision + 1,
            result_digest = COALESCE(head.result_digest, budget_digest),
            result_at = COALESCE(head.result_at, database_now),
            recovery_process_instance_id = NULL,
            recovery_gateway_shard_id = NULL,
            recovery_runtime_build_revision = NULL,
            recovery_acquired_at = NULL,
            recovery_expires_at = NULL,
            next_recovery_at = NULL,
            updated_at = database_now
        WHERE head.application_id = expected_application_id
            AND head.interaction_id = expected_interaction_id
            AND head.action_index = expected_action_index;

        INSERT INTO public.runtime_interaction_effect_events_v1 (
            application_id,
            interaction_id,
            action_index,
            event_revision,
            event_kind,
            from_state,
            to_state,
            receipt_claim_revision,
            recovery_claim_revision,
            process_instance_id,
            outcome_code,
            result_digest,
            output_kind,
            output_id,
            event_digest,
            observed_at
        ) VALUES (
            expected_application_id,
            expected_interaction_id,
            expected_action_index,
            effect_head.head_revision + 1,
            'recovery_required',
            effect_head.state,
            'recovery_required',
            NULL,
            effect_head.recovery_claim_revision,
            expected_process_instance_id,
            'recovery_blocked_attempt_budget_exhausted',
            budget_digest,
            effect_head.output_kind,
            NULL,
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.concat_ws(
                    '|',
                    'starring-runtime-interaction-effect-event-v1',
                    expected_application_id,
                    expected_interaction_id,
                    expected_action_index::TEXT,
                    (effect_head.head_revision + 1)::TEXT,
                    'recovery_required',
                    effect_head.state,
                    'recovery_required',
                    effect_head.recovery_claim_revision::TEXT,
                    expected_process_instance_id,
                    'recovery_blocked_attempt_budget_exhausted',
                    pg_catalog.encode(budget_digest, 'hex')
                ),
                'UTF8'
            )),
            database_now
        );

        receipt_revision :=
            public.starring_runtime_interaction_effect_complete_receipt_v1(
                expected_application_id,
                expected_interaction_id,
                'interaction_response_unrecoverable',
                budget_digest,
                database_now
            );

        outcome_name := 'interaction_response_unrecoverable';
        effect_state := 'recovery_required';
        resulting_effect_head_revision := effect_head.head_revision + 1;
        resulting_recovery_claim_revision :=
            effect_head.recovery_claim_revision;
        resulting_observation_attempt_count :=
            effect_head.observation_attempt_count;
        resulting_recovery_claim_expires_at := database_now;
        receipt_state := 'completed';
        resulting_receipt_head_revision := receipt_revision;
        token_encryption_suite := NULL;
        token_suite_version := NULL;
        token_key_id := NULL;
        token_nonce := NULL;
        token_ciphertext := NULL;
        token_aad_digest := NULL;
        token_issued_at := NULL;
        token_expires_at := NULL;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT claim.*
    INTO claim_result
    FROM public.starring_runtime_interaction_effect_recovery_claim_v1(
        expected_application_id,
        expected_interaction_id,
        expected_action_index,
        expected_effect_head_revision,
        expected_process_instance_id,
        expected_gateway_shard_id,
        expected_runtime_build_revision,
        expected_runtime_generation,
        expected_controller_fencing_token,
        expected_route_incarnation,
        requested_claim_lease_milliseconds
    ) AS claim;

    SELECT root.*
    INTO effect_root
    FROM public.runtime_interaction_effect_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    SELECT head.*
    INTO receipt_head
    FROM public.runtime_interaction_receipt_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
    FOR UPDATE;

    SELECT head.*
    INTO effect_head
    FROM public.runtime_interaction_effect_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
        AND head.action_index = expected_action_index
    FOR UPDATE;

    IF effect_root.preflight_certificate_digest
            IS DISTINCT FROM expected_preflight_certificate_digest
        OR receipt_head.state <> 'executing'
        OR receipt_head.acknowledgement_state NOT IN (
            'unacknowledged',
            'deferred',
            'responded'
        )
        OR effect_head.action_kind <> 'edit_response'
        OR effect_head.output_kind <> 'original_response'
        OR effect_head.correlation_class <> 'interaction_receipt'
        OR effect_head.correlation_marker IS NOT NULL
        OR effect_head.expected_postimage_digest
            IS DISTINCT FROM expected_postimage_digest
        OR effect_head.state <> 'observing'
        OR effect_head.head_revision
            <> claim_result.resulting_effect_head_revision
        OR effect_head.recovery_claim_revision
            <> claim_result.resulting_recovery_claim_revision
        OR effect_head.recovery_process_instance_id
            IS DISTINCT FROM expected_process_instance_id
        OR EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_rollbacks_v1 AS rollback
            WHERE rollback.application_id = expected_application_id
                AND rollback.interaction_id = expected_interaction_id
        )
        OR EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_heads_v1 AS mutable
            WHERE mutable.application_id = expected_application_id
                AND mutable.interaction_id = expected_interaction_id
                AND mutable.action_kind <> 'edit_response'
                AND mutable.state NOT IN (
                    'known_succeeded',
                    'reconciled_succeeded'
                )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_response_tail_claim_conflict';
    END IF;

    database_now := pg_catalog.clock_timestamp();

    SELECT token.*
    INTO token_row
    FROM public.runtime_interaction_receipt_token_secrets_v1 AS token
    WHERE token.application_id = expected_application_id
        AND token.interaction_id = expected_interaction_id
    FOR UPDATE;

    IF NOT FOUND
        OR token_row.expires_at
            <= claim_result.resulting_recovery_claim_expires_at
    THEN
        resulting_digest := COALESCE(
            effect_head.result_digest,
            proposed_unrecoverable_digest
        );

        UPDATE public.runtime_interaction_effect_heads_v1 AS head
        SET state = 'recovery_required',
            head_revision = head.head_revision + 1,
            result_digest = resulting_digest,
            result_at = COALESCE(head.result_at, database_now),
            recovery_process_instance_id = NULL,
            recovery_gateway_shard_id = NULL,
            recovery_runtime_build_revision = NULL,
            recovery_acquired_at = NULL,
            recovery_expires_at = NULL,
            next_recovery_at = NULL,
            updated_at = database_now
        WHERE head.application_id = expected_application_id
            AND head.interaction_id = expected_interaction_id
            AND head.action_index = expected_action_index;

        INSERT INTO public.runtime_interaction_effect_events_v1 (
            application_id,
            interaction_id,
            action_index,
            event_revision,
            event_kind,
            from_state,
            to_state,
            receipt_claim_revision,
            recovery_claim_revision,
            process_instance_id,
            outcome_code,
            result_digest,
            output_kind,
            output_id,
            event_digest,
            observed_at
        ) VALUES (
            expected_application_id,
            expected_interaction_id,
            expected_action_index,
            effect_head.head_revision + 1,
            'recovery_required',
            effect_head.state,
            'recovery_required',
            NULL,
            effect_head.recovery_claim_revision,
            expected_process_instance_id,
            'interaction_response_unrecoverable',
            proposed_unrecoverable_digest,
            effect_head.output_kind,
            NULL,
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.concat_ws(
                    '|',
                    'starring-runtime-interaction-effect-event-v1',
                    expected_application_id,
                    expected_interaction_id,
                    expected_action_index::TEXT,
                    (effect_head.head_revision + 1)::TEXT,
                    'recovery_required',
                    effect_head.state,
                    'recovery_required',
                    effect_head.recovery_claim_revision::TEXT,
                    expected_process_instance_id,
                    'interaction_response_unrecoverable',
                    pg_catalog.encode(proposed_unrecoverable_digest, 'hex')
                ),
                'UTF8'
            )),
            database_now
        );

        PERFORM public.starring_runtime_interaction_effect_complete_receipt_v1(
            expected_application_id,
            expected_interaction_id,
            'interaction_response_unrecoverable',
            proposed_unrecoverable_digest,
            database_now
        );

        outcome_name := 'interaction_response_unrecoverable';
        effect_state := 'recovery_required';
        resulting_effect_head_revision := effect_head.head_revision + 1;
        resulting_recovery_claim_revision :=
            effect_head.recovery_claim_revision;
        resulting_observation_attempt_count :=
            effect_head.observation_attempt_count;
        resulting_recovery_claim_expires_at := database_now;
        receipt_state := 'completed';
        resulting_receipt_head_revision := receipt_head.head_revision + 1;
        token_encryption_suite := NULL;
        token_suite_version := NULL;
        token_key_id := NULL;
        token_nonce := NULL;
        token_ciphertext := NULL;
        token_aad_digest := NULL;
        token_issued_at := NULL;
        token_expires_at := NULL;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    outcome_name := CASE claim_result.outcome_name
        WHEN 'exact_replay' THEN 'response_tail_claim_replayed'
        ELSE 'response_tail_claimed'
    END;
    effect_state := effect_head.state;
    resulting_effect_head_revision := effect_head.head_revision;
    resulting_recovery_claim_revision := effect_head.recovery_claim_revision;
    resulting_observation_attempt_count :=
        effect_head.observation_attempt_count;
    resulting_recovery_claim_expires_at := effect_head.recovery_expires_at;
    receipt_state := receipt_head.state;
    resulting_receipt_head_revision := receipt_head.head_revision;
    token_encryption_suite := token_row.encryption_suite;
    token_suite_version := token_row.suite_version;
    token_key_id := token_row.key_id;
    token_nonce := token_row.nonce;
    token_ciphertext := token_row.ciphertext;
    token_aad_digest := token_row.aad_digest;
    token_issued_at := token_row.issued_at;
    token_expires_at := token_row.expires_at;
    observed_database_now := database_now;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_effect_reconcile_v1(
    expected_application_id TEXT,
    expected_interaction_id TEXT,
    expected_action_index BIGINT,
    expected_effect_head_revision BIGINT,
    expected_recovery_claim_revision BIGINT,
    expected_process_instance_id TEXT,
    expected_gateway_shard_id TEXT,
    expected_runtime_build_revision TEXT,
    expected_runtime_generation BIGINT,
    expected_controller_fencing_token BIGINT,
    expected_route_incarnation BIGINT,
    expected_source_effect_state TEXT,
    expected_recovery_path TEXT,
    expected_preflight_certificate_digest BYTEA,
    proposed_observation_outcome TEXT,
    proposed_observation_digest BYTEA,
    proposed_output_id TEXT,
    requested_retry_delay_milliseconds BIGINT
)
RETURNS TABLE(
    outcome_name TEXT,
    effect_state TEXT,
    resulting_effect_head_revision BIGINT,
    resulting_recovery_at TIMESTAMPTZ,
    observed_database_now TIMESTAMPTZ
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
    receipt_root public.runtime_interaction_receipt_roots_v1%ROWTYPE;
    effect_root public.runtime_interaction_effect_roots_v1%ROWTYPE;
    rollback_row public.runtime_interaction_effect_rollbacks_v1%ROWTYPE;
    effect_head public.runtime_interaction_effect_heads_v1%ROWTYPE;
    database_now TIMESTAMPTZ;
    next_state TEXT;
    next_event_kind TEXT;
    expected_observation_state TEXT;
    normalized_output_id TEXT;
    recovery_at TIMESTAMPTZ;
    replay_matches BOOLEAN;
    receipt_state_value TEXT;
    recovery_blocked BOOLEAN;
BEGIN
    IF expected_application_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_application_id) > 20
        OR expected_interaction_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_interaction_id) > 20
        OR expected_action_index NOT BETWEEN 0 AND 255
        OR expected_effect_head_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR expected_recovery_claim_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_gateway_shard_id !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR expected_runtime_build_revision !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR expected_runtime_generation NOT BETWEEN 1 AND 9223372036854775807
        OR expected_controller_fencing_token
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_route_incarnation NOT BETWEEN 1 AND 9223372036854775807
        OR expected_source_effect_state NOT IN (
            'observing',
            'compensation_intended',
            'compensation_observing'
        )
        OR expected_recovery_path NOT IN (
            'observation',
            'compensation',
            'response_tail'
        )
        OR pg_catalog.octet_length(
            expected_preflight_certificate_digest
        ) <> 32
        OR proposed_observation_outcome NOT IN (
            'adopted_success',
            'observed_failure',
            'deferred',
            'conflict',
            'unsupported',
            'compensation_restored',
            'compensation_deferred',
            'compensation_conflict',
            'compensation_unsupported',
            'recovery_blocked_discord_read_rejected',
            'recovery_blocked_response_token_unavailable',
            'recovery_blocked_observation_protocol',
            'recovery_blocked_compensation_conflict',
            'recovery_blocked_compensation_unsupported',
            'recovery_blocked_non_compensable',
            'recovery_blocked_internal_conflict',
            'recovery_blocked_discord_forbidden',
            'recovery_blocked_internal_authority'
        )
        OR pg_catalog.octet_length(proposed_observation_digest) <> 32
        OR pg_catalog.octet_length(proposed_output_id) > 128
        OR requested_retry_delay_milliseconds NOT BETWEEN 1000 AND 60000
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_effect_reconcile_input_invalid';
    END IF;

    database_now := pg_catalog.clock_timestamp();
    normalized_output_id := NULLIF(proposed_output_id, '');
    recovery_blocked := proposed_observation_outcome LIKE
        'recovery_blocked_%';
    next_state := CASE proposed_observation_outcome
        WHEN 'adopted_success' THEN 'reconciled_succeeded'
        WHEN 'observed_failure' THEN 'known_failed'
        WHEN 'deferred' THEN 'observation_pending'
        WHEN 'compensation_restored' THEN 'compensated'
        WHEN 'compensation_deferred'
            THEN 'compensation_observation_pending'
        ELSE 'recovery_required'
    END;
    next_event_kind := CASE proposed_observation_outcome
        WHEN 'adopted_success' THEN 'reconciled_success'
        WHEN 'observed_failure' THEN 'reconciled_failure'
        WHEN 'deferred' THEN 'recovery_deferred'
        WHEN 'compensation_restored' THEN 'compensated'
        WHEN 'compensation_deferred'
            THEN 'compensation_observation_deferred'
        ELSE 'recovery_required'
    END;
    expected_observation_state := CASE
        WHEN recovery_blocked THEN NULL
        WHEN proposed_observation_outcome LIKE 'compensation_%'
            THEN 'compensation_observing'
        ELSE 'observing'
    END;
    recovery_at := CASE
        WHEN next_state IN (
            'observation_pending',
            'compensation_observation_pending'
        )
            THEN database_now
                + requested_retry_delay_milliseconds
                    * INTERVAL '1 millisecond'
        ELSE NULL
    END;

    SELECT root.*
    INTO receipt_root
    FROM public.runtime_interaction_receipt_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_receipt_not_found';
    END IF;

    IF receipt_root.runtime_generation <> expected_runtime_generation
        OR receipt_root.route_controller_fencing_token
            <> expected_controller_fencing_token
        OR receipt_root.route_incarnation <> expected_route_incarnation
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_reconcile_conflict';
    END IF;

    SELECT head.state
    INTO receipt_state_value
    FROM public.runtime_interaction_receipt_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
    FOR SHARE;

    IF NOT FOUND OR receipt_state_value = 'completed' THEN
        RAISE EXCEPTION USING
            ERRCODE = CASE WHEN receipt_state_value = 'completed'
                THEN 'RI001'
                ELSE 'RI002'
            END,
            MESSAGE = CASE WHEN receipt_state_value = 'completed'
                THEN 'runtime_interaction_effect_reconcile_conflict'
                ELSE 'runtime_interaction_effect_receipt_head_missing'
            END;
    END IF;

    SELECT root.*
    INTO effect_root
    FROM public.runtime_interaction_effect_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    IF NOT FOUND
        OR effect_root.preflight_certificate_digest
            IS DISTINCT FROM expected_preflight_certificate_digest
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_effect_plan_corruption';
    END IF;

    IF expected_observation_state = 'compensation_observing' THEN
        SELECT rollback.*
        INTO rollback_row
        FROM public.runtime_interaction_effect_rollbacks_v1 AS rollback
        WHERE rollback.application_id = expected_application_id
            AND rollback.interaction_id = expected_interaction_id
        FOR UPDATE;

        IF NOT FOUND
            OR rollback_row.state <> 'required'
            OR expected_action_index > rollback_row.abort_action_index
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI001',
                MESSAGE = 'runtime_interaction_effect_compensation_not_authorized';
        END IF;
    END IF;

    SELECT head.*
    INTO effect_head
    FROM public.runtime_interaction_effect_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
        AND head.action_index = expected_action_index
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_action_not_found';
    END IF;

    IF (
            expected_source_effect_state = 'observing'
            AND expected_recovery_path NOT IN (
                'observation',
                'response_tail'
            )
        )
        OR (
            expected_source_effect_state IN (
                'compensation_intended',
                'compensation_observing'
            )
            AND expected_recovery_path <> 'compensation'
        )
        OR recovery_blocked
            AND proposed_observation_digest IS DISTINCT FROM
                pg_catalog.sha256(pg_catalog.convert_to(
                    pg_catalog.concat_ws(
                        '|',
                        'starring-runtime-interaction-effect-recovery-block-v1',
                        expected_application_id,
                        expected_interaction_id,
                        expected_action_index::TEXT,
                        expected_effect_head_revision::TEXT,
                        expected_recovery_claim_revision::TEXT,
                        expected_process_instance_id,
                        expected_gateway_shard_id,
                        expected_runtime_build_revision,
                        expected_runtime_generation::TEXT,
                        expected_controller_fencing_token::TEXT,
                        expected_route_incarnation::TEXT,
                        expected_source_effect_state,
                        expected_recovery_path,
                        pg_catalog.encode(
                            expected_preflight_certificate_digest,
                            'hex'
                        ),
                        proposed_observation_outcome
                    ),
                    'UTF8'
                ))
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_reconcile_conflict';
    END IF;

    SELECT EXISTS (
        SELECT 1
        FROM public.runtime_interaction_effect_events_v1 AS event
        WHERE event.application_id = expected_application_id
            AND event.interaction_id = expected_interaction_id
            AND event.action_index = expected_action_index
            AND event.event_revision = effect_head.head_revision
            AND event.event_kind = next_event_kind
            AND event.from_state = expected_source_effect_state
            AND event.to_state = next_state
            AND event.receipt_claim_revision IS NULL
            AND event.recovery_claim_revision
                = expected_recovery_claim_revision
            AND event.process_instance_id = expected_process_instance_id
            AND event.outcome_code = proposed_observation_outcome
            AND event.result_digest = proposed_observation_digest
            AND event.output_kind = effect_head.output_kind
            AND event.output_id IS NOT DISTINCT FROM normalized_output_id
            AND event.event_digest = pg_catalog.sha256(
                pg_catalog.convert_to(
                    pg_catalog.concat_ws(
                        '|',
                        'starring-runtime-interaction-effect-event-v1',
                        expected_application_id,
                        expected_interaction_id,
                        expected_action_index::TEXT,
                        (expected_effect_head_revision + 1)::TEXT,
                        next_event_kind,
                        expected_source_effect_state,
                        next_state,
                        expected_recovery_claim_revision::TEXT,
                        expected_process_instance_id,
                        proposed_observation_outcome,
                        pg_catalog.encode(
                            proposed_observation_digest,
                            'hex'
                        ),
                        effect_head.output_kind,
                        COALESCE(normalized_output_id, '')
                    ),
                    'UTF8'
                )
            )
            AND (
                recovery_blocked
                OR EXISTS (
                    SELECT 1
                    FROM public.runtime_interaction_effect_events_v1
                        AS claim_event
                    WHERE claim_event.application_id
                            = expected_application_id
                        AND claim_event.interaction_id
                            = expected_interaction_id
                        AND claim_event.action_index
                            = expected_action_index
                        AND claim_event.event_revision
                            = expected_effect_head_revision
                        AND claim_event.event_kind = CASE
                            WHEN expected_source_effect_state
                                = 'compensation_observing'
                                THEN 'compensation_observation_claimed'
                            ELSE 'recovery_claimed'
                        END
                        AND claim_event.from_state IN (
                            'intended',
                            'indeterminate',
                            'observing',
                            'observation_pending',
                            'compensation_intended',
                            'compensation_indeterminate',
                            'compensation_observing',
                            'compensation_observation_pending'
                        )
                        AND claim_event.to_state
                            = expected_source_effect_state
                        AND claim_event.receipt_claim_revision IS NULL
                        AND claim_event.recovery_claim_revision
                            = expected_recovery_claim_revision
                        AND claim_event.process_instance_id
                            = expected_process_instance_id
                        AND claim_event.outcome_code
                            = claim_event.event_kind
                        AND claim_event.output_kind
                            = effect_head.output_kind
                        AND claim_event.event_digest = pg_catalog.sha256(
                            pg_catalog.convert_to(
                                pg_catalog.concat_ws(
                                    '|',
                                    'starring-runtime-interaction-effect-event-v1',
                                    expected_application_id,
                                    expected_interaction_id,
                                    expected_action_index::TEXT,
                                    expected_effect_head_revision::TEXT,
                                    claim_event.event_kind,
                                    claim_event.from_state,
                                    expected_source_effect_state,
                                    expected_recovery_claim_revision::TEXT,
                                    expected_process_instance_id,
                                    expected_gateway_shard_id,
                                    expected_runtime_build_revision
                                ),
                                'UTF8'
                            )
                        )
                )
            )
    )
    INTO replay_matches;

    IF effect_head.state = next_state
        AND effect_head.head_revision = expected_effect_head_revision + 1
        AND replay_matches
        AND effect_head.success_binding_kind IS NOT DISTINCT FROM (CASE
            WHEN proposed_observation_outcome = 'adopted_success'
                THEN 'observation'
            ELSE effect_head.success_binding_kind
        END)
        AND (
            proposed_observation_outcome <> 'adopted_success'
            OR effect_head.success_binding_digest
                IS NOT DISTINCT FROM proposed_observation_digest
        )
        AND (
            NOT recovery_blocked
            OR effect_head.result_digest IS NOT NULL
        )
    THEN
        outcome_name := 'exact_replay';
        effect_state := effect_head.state;
        resulting_effect_head_revision := effect_head.head_revision;
        resulting_recovery_at := effect_head.next_recovery_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF recovery_blocked
        AND (
            (
                effect_head.state = 'observing'
                AND (
                    (
                        effect_head.action_kind = 'edit_response'
                        AND proposed_observation_outcome LIKE
                            'recovery_blocked_%'
                    )
                    OR (
                        effect_head.action_kind <> 'edit_response'
                        AND proposed_observation_outcome NOT IN (
                            'recovery_blocked_discord_read_rejected',
                            'recovery_blocked_observation_protocol',
                            'recovery_blocked_internal_conflict',
                            'recovery_blocked_discord_forbidden',
                            'recovery_blocked_internal_authority'
                        )
                    )
                )
            )
            OR (
                effect_head.state IN (
                    'compensation_intended',
                    'compensation_observing'
                )
                AND proposed_observation_outcome NOT IN (
                    'recovery_blocked_discord_read_rejected',
                    'recovery_blocked_observation_protocol',
                    'recovery_blocked_compensation_conflict',
                    'recovery_blocked_compensation_unsupported',
                    'recovery_blocked_non_compensable',
                    'recovery_blocked_internal_conflict',
                    'recovery_blocked_discord_forbidden',
                    'recovery_blocked_internal_authority'
                )
            )
            OR effect_head.state NOT IN (
                'observing',
                'compensation_intended',
                'compensation_observing'
            )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_reconcile_conflict';
    END IF;

    IF recovery_blocked
        AND effect_head.state IN (
            'compensation_intended',
            'compensation_observing'
        )
    THEN
        SELECT rollback.*
        INTO rollback_row
        FROM public.runtime_interaction_effect_rollbacks_v1 AS rollback
        WHERE rollback.application_id = expected_application_id
            AND rollback.interaction_id = expected_interaction_id
        FOR UPDATE;

        IF NOT FOUND
            OR rollback_row.state <> 'required'
            OR expected_action_index > rollback_row.abort_action_index
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI001',
                MESSAGE = 'runtime_interaction_effect_compensation_not_authorized';
        END IF;
    END IF;

    IF (
            recovery_blocked
            AND effect_head.state NOT IN (
                'observing',
                'compensation_intended',
                'compensation_observing'
            )
        )
        OR (
            NOT recovery_blocked
            AND effect_head.state <> expected_observation_state
        )
        OR effect_head.head_revision <> expected_effect_head_revision
        OR effect_head.state <> expected_source_effect_state
        OR (
            effect_head.action_kind = 'edit_response'
            AND expected_recovery_path <> 'response_tail'
        )
        OR (
            effect_head.action_kind <> 'edit_response'
            AND expected_recovery_path = 'response_tail'
        )
        OR effect_head.resolved_effect_identity_digest IS NULL
        OR (
            (
                expected_observation_state = 'compensation_observing'
                OR recovery_blocked
                    AND effect_head.state IN (
                        'compensation_intended',
                        'compensation_observing'
                    )
            )
            AND (
                effect_head.success_binding_kind NOT IN (
                    'attempt_result',
                    'observation'
                )
                OR effect_head.success_binding_digest IS NULL
            )
        )
        OR effect_head.recovery_claim_revision
            <> expected_recovery_claim_revision
        OR effect_head.recovery_process_instance_id
            IS DISTINCT FROM expected_process_instance_id
        OR effect_head.recovery_gateway_shard_id
            IS DISTINCT FROM expected_gateway_shard_id
        OR effect_head.recovery_runtime_build_revision
            IS DISTINCT FROM expected_runtime_build_revision
        OR receipt_root.runtime_generation <> expected_runtime_generation
        OR receipt_root.route_controller_fencing_token
            <> expected_controller_fencing_token
        OR receipt_root.route_incarnation <> expected_route_incarnation
        OR effect_head.recovery_expires_at <= database_now
        OR (
            proposed_observation_outcome = 'adopted_success'
            AND (
                (
                    effect_head.output_kind IN (
                        'role_membership',
                        'permission_overwrite',
                        'original_response'
                    )
                    AND normalized_output_id IS NOT NULL
                )
                OR (
                    effect_head.output_kind IN (
                        'created_role',
                        'created_channel',
                        'posted_message'
                    )
                    AND (
                        normalized_output_id IS NULL
                        OR normalized_output_id
                            !~ '^[1-9][0-9]{0,19}$'
                        OR pg_catalog.length(normalized_output_id) > 20
                        OR (
                            pg_catalog.length(normalized_output_id) = 20
                            AND normalized_output_id
                                > '18446744073709551615'
                        )
                    )
                )
                OR (
                    effect_head.output_kind = 'instance_state'
                    AND (
                        normalized_output_id IS NULL
                        OR normalized_output_id
                            !~ '^[A-Za-z0-9_-]{1,32}$'
                    )
                )
            )
        )
        OR (
            proposed_observation_outcome <> 'adopted_success'
            AND normalized_output_id IS NOT NULL
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_reconcile_conflict';
    END IF;

    UPDATE public.runtime_interaction_effect_heads_v1 AS head
    SET state = next_state,
        head_revision = head.head_revision + 1,
        result_digest = CASE
            WHEN recovery_blocked
                THEN COALESCE(
                    head.result_digest,
                    proposed_observation_digest
                )
            WHEN expected_observation_state = 'observing'
                THEN COALESCE(
                    head.result_digest,
                    proposed_observation_digest
                )
            ELSE head.result_digest
        END,
        output_id = CASE
            WHEN proposed_observation_outcome = 'adopted_success'
                THEN normalized_output_id
            ELSE head.output_id
        END,
        result_at = COALESCE(head.result_at, database_now),
        success_binding_kind = CASE
            WHEN proposed_observation_outcome = 'adopted_success'
                THEN 'observation'
            ELSE head.success_binding_kind
        END,
        success_binding_digest = CASE
            WHEN proposed_observation_outcome = 'adopted_success'
                THEN proposed_observation_digest
            ELSE head.success_binding_digest
        END,
        compensation_result_digest = CASE
            WHEN proposed_observation_outcome = 'compensation_restored'
                THEN COALESCE(
                    head.compensation_result_digest,
                    proposed_observation_digest
                )
            ELSE head.compensation_result_digest
        END,
        compensation_result_at = CASE
            WHEN proposed_observation_outcome = 'compensation_restored'
                THEN COALESCE(head.compensation_result_at, database_now)
            ELSE head.compensation_result_at
        END,
        recovery_process_instance_id = NULL,
        recovery_gateway_shard_id = NULL,
        recovery_runtime_build_revision = NULL,
        recovery_acquired_at = NULL,
        recovery_expires_at = NULL,
        next_recovery_at = recovery_at,
        updated_at = database_now
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
        AND head.action_index = expected_action_index;

    INSERT INTO public.runtime_interaction_effect_events_v1 (
        application_id,
        interaction_id,
        action_index,
        event_revision,
        event_kind,
        from_state,
        to_state,
        receipt_claim_revision,
        recovery_claim_revision,
        process_instance_id,
        outcome_code,
        result_digest,
        output_kind,
        output_id,
        event_digest,
        observed_at
    ) VALUES (
        expected_application_id,
        expected_interaction_id,
        expected_action_index,
        effect_head.head_revision + 1,
        next_event_kind,
        effect_head.state,
        next_state,
        NULL,
        expected_recovery_claim_revision,
        expected_process_instance_id,
        proposed_observation_outcome,
        proposed_observation_digest,
        effect_head.output_kind,
        normalized_output_id,
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.concat_ws(
                '|',
                'starring-runtime-interaction-effect-event-v1',
                expected_application_id,
                expected_interaction_id,
                expected_action_index::TEXT,
                (effect_head.head_revision + 1)::TEXT,
                next_event_kind,
                effect_head.state,
                next_state,
                expected_recovery_claim_revision::TEXT,
                expected_process_instance_id,
                proposed_observation_outcome,
                pg_catalog.encode(proposed_observation_digest, 'hex'),
                effect_head.output_kind,
                COALESCE(normalized_output_id, '')
            ),
            'UTF8'
        )),
        database_now
    );

    IF effect_head.action_kind <> 'edit_response'
        AND (
            (
                expected_observation_state = 'observing'
                AND proposed_observation_outcome IN (
                    'adopted_success',
                    'observed_failure',
                    'conflict',
                    'unsupported'
                )
            )
            OR (
                recovery_blocked
                AND effect_head.state = 'observing'
            )
        )
    THEN
        PERFORM public.starring_runtime_interaction_effect_require_rollback_v1(
            expected_application_id,
            expected_interaction_id,
            CASE
                WHEN recovery_blocked
                    OR proposed_observation_outcome IN (
                    'conflict',
                    'unsupported'
                ) THEN 'recovery_required'
                ELSE 'observation_abort'
            END,
            database_now
        );
    END IF;

    PERFORM public.starring_runtime_interaction_effect_try_complete_rollback_v1(
        expected_application_id,
        expected_interaction_id,
        database_now
    );

    outcome_name := proposed_observation_outcome;
    effect_state := next_state;
    resulting_effect_head_revision := effect_head.head_revision + 1;
    resulting_recovery_at := recovery_at;
    observed_database_now := database_now;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_effect_response_tail_finalize_v1(
    expected_application_id TEXT,
    expected_interaction_id TEXT,
    expected_action_index BIGINT,
    expected_receipt_head_revision BIGINT,
    expected_receipt_state TEXT,
    expected_effect_head_revision BIGINT,
    expected_recovery_claim_revision BIGINT,
    expected_process_instance_id TEXT,
    expected_gateway_shard_id TEXT,
    expected_runtime_build_revision TEXT,
    expected_runtime_generation BIGINT,
    expected_controller_fencing_token BIGINT,
    expected_route_incarnation BIGINT,
    expected_preflight_certificate_digest BYTEA,
    expected_postimage_digest BYTEA,
    proposed_observation_outcome TEXT,
    proposed_observation_digest BYTEA,
    proposed_terminal_result_digest BYTEA,
    requested_retry_delay_milliseconds BIGINT
)
RETURNS TABLE(
    outcome_name TEXT,
    effect_state TEXT,
    resulting_effect_head_revision BIGINT,
    receipt_state TEXT,
    resulting_receipt_head_revision BIGINT,
    resulting_recovery_at TIMESTAMPTZ,
    observed_database_now TIMESTAMPTZ
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
    receipt_root public.runtime_interaction_receipt_roots_v1%ROWTYPE;
    receipt_head public.runtime_interaction_receipt_heads_v1%ROWTYPE;
    effect_root public.runtime_interaction_effect_roots_v1%ROWTYPE;
    effect_head public.runtime_interaction_effect_heads_v1%ROWTYPE;
    reconciliation RECORD;
    database_now TIMESTAMPTZ;
    terminal_outcome TEXT;
    expected_terminal_outcome TEXT;
    expected_effect_state TEXT;
    expected_effect_event_kind TEXT;
    expected_effect_event_outcome TEXT;
    expected_effect_revision BIGINT;
    receipt_revision BIGINT;
    authority_available BOOLEAN;
    resulting_digest BYTEA;
    replay_event_matches BOOLEAN;
    recovery_blocked BOOLEAN;
BEGIN
    IF expected_application_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_application_id) > 20
        OR expected_interaction_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_interaction_id) > 20
        OR expected_action_index NOT BETWEEN 0 AND 255
        OR expected_receipt_head_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR expected_receipt_state <> 'executing'
        OR expected_effect_head_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_recovery_claim_revision
            NOT BETWEEN 0 AND 9223372036854775807
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_gateway_shard_id !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR expected_runtime_build_revision !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR expected_runtime_generation NOT BETWEEN 1 AND 9223372036854775807
        OR expected_controller_fencing_token
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_route_incarnation NOT BETWEEN 1 AND 9223372036854775807
        OR pg_catalog.octet_length(
            expected_preflight_certificate_digest
        ) <> 32
        OR pg_catalog.octet_length(expected_postimage_digest) <> 32
        OR proposed_observation_outcome NOT IN (
            'close_known_state',
            'exact_success',
            'exact_absence',
            'deferred',
            'conflict',
            'unsupported',
            'token_unrecoverable',
            'recovery_blocked_discord_read_rejected',
            'recovery_blocked_response_token_unavailable',
            'recovery_blocked_observation_protocol',
            'recovery_blocked_internal_conflict',
            'recovery_blocked_discord_forbidden',
            'recovery_blocked_internal_authority'
        )
        OR pg_catalog.octet_length(proposed_observation_digest) <> 32
        OR pg_catalog.octet_length(
            proposed_terminal_result_digest
        ) <> 32
        OR requested_retry_delay_milliseconds NOT BETWEEN 1000 AND 60000
        OR (
            proposed_observation_outcome = 'exact_success'
            AND proposed_terminal_result_digest
                IS DISTINCT FROM expected_postimage_digest
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_effect_response_tail_finalize_input_invalid';
    END IF;

    recovery_blocked := proposed_observation_outcome LIKE
        'recovery_blocked_%';

    IF recovery_blocked
        AND (
            proposed_terminal_result_digest
                IS DISTINCT FROM proposed_observation_digest
            OR proposed_observation_digest IS DISTINCT FROM
                pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.concat_ws(
                    '|',
                    'starring-runtime-interaction-response-tail-recovery-block-v1',
                    expected_application_id,
                    expected_interaction_id,
                    expected_action_index::TEXT,
                    expected_receipt_head_revision::TEXT,
                    expected_effect_head_revision::TEXT,
                    expected_recovery_claim_revision::TEXT,
                    expected_process_instance_id,
                    expected_gateway_shard_id,
                    expected_runtime_build_revision,
                    expected_runtime_generation::TEXT,
                    expected_controller_fencing_token::TEXT,
                    expected_route_incarnation::TEXT,
                    pg_catalog.encode(
                        expected_preflight_certificate_digest,
                        'hex'
                    ),
                    proposed_observation_outcome
                ),
                'UTF8'
            ))
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_response_tail_finalize_conflict';
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            'starring-runtime-interaction-receipt-v1:'
                || expected_application_id
                || ':'
                || expected_interaction_id,
            0
        )
    );

    SELECT root.*
    INTO receipt_root
    FROM public.runtime_interaction_receipt_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    IF NOT FOUND
        OR receipt_root.runtime_generation <> expected_runtime_generation
        OR receipt_root.route_controller_fencing_token
            <> expected_controller_fencing_token
        OR receipt_root.route_incarnation <> expected_route_incarnation
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_effect_recovery_authority_stale';
    END IF;

    SELECT head.*
    INTO receipt_head
    FROM public.runtime_interaction_receipt_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_effect_receipt_head_missing';
    END IF;

    IF recovery_blocked THEN
        authority_available := TRUE;
    ELSE
        SELECT EXISTS (
            SELECT 1
            FROM public.starring_runtime_interaction_receipt_authority_observe_v1(
                receipt_root.application_id,
                receipt_root.tenant_id,
                receipt_root.installation_id,
                receipt_root.deployment_id,
                receipt_root.guild_id,
                receipt_root.ruleset_key,
                receipt_root.target_version,
                receipt_root.target_content_hash,
                receipt_root.binding_revision,
                receipt_root.binding_fingerprint,
                receipt_root.runtime_generation,
                receipt_root.route_controller_fencing_token,
                receipt_root.route_incarnation,
                expected_process_instance_id,
                expected_gateway_shard_id,
                expected_runtime_build_revision,
                receipt_root.route_kind,
                COALESCE(receipt_root.instance_id, '')
            ) AS authority
        )
        INTO authority_available;
    END IF;

    IF NOT authority_available THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_effect_recovery_authority_stale';
    END IF;

    SELECT root.*
    INTO effect_root
    FROM public.runtime_interaction_effect_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    IF NOT FOUND
        OR effect_root.preflight_certificate_digest
            IS DISTINCT FROM expected_preflight_certificate_digest
        OR effect_root.action_plan_digest
            IS DISTINCT FROM receipt_head.action_plan_digest
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_effect_plan_corruption';
    END IF;

    PERFORM rollback.application_id
    FROM public.runtime_interaction_effect_rollbacks_v1 AS rollback
    WHERE rollback.application_id = expected_application_id
        AND rollback.interaction_id = expected_interaction_id
    FOR UPDATE;

    IF FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_response_tail_finalize_conflict';
    END IF;

    SELECT head.*
    INTO effect_head
    FROM public.runtime_interaction_effect_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
        AND head.action_index = expected_action_index
    FOR UPDATE;

    IF NOT FOUND
        OR effect_head.action_kind <> 'edit_response'
        OR effect_head.output_kind <> 'original_response'
        OR effect_head.correlation_class <> 'interaction_receipt'
        OR effect_head.correlation_marker IS NOT NULL
        OR effect_head.expected_postimage_digest
            IS DISTINCT FROM expected_postimage_digest
        OR effect_head.recovery_claim_revision
            <> expected_recovery_claim_revision
        OR effect_root.action_count <> (
            SELECT pg_catalog.count(*)
            FROM public.runtime_interaction_effect_heads_v1 AS effect
            WHERE effect.application_id = expected_application_id
                AND effect.interaction_id = expected_interaction_id
        )
        OR EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_heads_v1 AS successor
            WHERE successor.application_id = expected_application_id
                AND successor.interaction_id = expected_interaction_id
                AND successor.action_index > expected_action_index
        )
        OR EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_heads_v1 AS mutable
            WHERE mutable.application_id = expected_application_id
                AND mutable.interaction_id = expected_interaction_id
                AND mutable.action_kind <> 'edit_response'
                AND mutable.state NOT IN (
                    'known_succeeded',
                    'reconciled_succeeded'
                )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_response_tail_finalize_conflict';
    END IF;

    database_now := pg_catalog.clock_timestamp();

    expected_terminal_outcome := CASE proposed_observation_outcome
        WHEN 'close_known_state' THEN CASE effect_head.state
            WHEN 'planned'
                THEN 'provisioning_completed_response_unconfirmed'
            WHEN 'known_succeeded' THEN 'effects_recovered_completed'
            WHEN 'reconciled_succeeded' THEN 'effects_recovered_completed'
            WHEN 'known_failed'
                THEN 'provisioning_completed_response_unconfirmed'
            WHEN 'recovery_required'
                THEN 'interaction_response_unrecoverable'
            ELSE NULL
        END
        WHEN 'exact_success' THEN 'effects_recovered_completed'
        WHEN 'exact_absence'
            THEN 'provisioning_completed_response_unconfirmed'
        WHEN 'conflict' THEN 'interaction_response_unrecoverable'
        WHEN 'unsupported' THEN 'interaction_response_unrecoverable'
        WHEN 'token_unrecoverable'
            THEN 'interaction_response_unrecoverable'
        ELSE NULL
    END;
    expected_effect_state := CASE proposed_observation_outcome
        WHEN 'exact_success' THEN 'reconciled_succeeded'
        WHEN 'exact_absence' THEN 'known_failed'
        WHEN 'conflict' THEN 'recovery_required'
        WHEN 'unsupported' THEN 'recovery_required'
        WHEN 'token_unrecoverable' THEN 'recovery_required'
        ELSE effect_head.state
    END;
    expected_effect_event_outcome := CASE proposed_observation_outcome
        WHEN 'exact_success' THEN 'adopted_success'
        WHEN 'exact_absence' THEN 'observed_failure'
        WHEN 'conflict' THEN 'conflict'
        WHEN 'unsupported' THEN 'unsupported'
        WHEN 'token_unrecoverable'
            THEN 'interaction_response_unrecoverable'
        ELSE NULL
    END;
    expected_effect_event_kind := CASE proposed_observation_outcome
        WHEN 'exact_success' THEN 'reconciled_success'
        WHEN 'exact_absence' THEN 'reconciled_failure'
        WHEN 'conflict' THEN 'recovery_required'
        WHEN 'unsupported' THEN 'recovery_required'
        WHEN 'token_unrecoverable' THEN 'recovery_required'
        ELSE NULL
    END;
    IF recovery_blocked THEN
        expected_terminal_outcome := 'interaction_response_unrecoverable';
        expected_effect_state := 'recovery_required';
        expected_effect_event_kind := 'recovery_required';
        expected_effect_event_outcome := proposed_observation_outcome;
    END IF;
    expected_effect_revision := expected_effect_head_revision
        + CASE
            WHEN expected_effect_event_outcome IS NULL THEN 0
            ELSE 1
        END;
    replay_event_matches := expected_effect_event_outcome IS NULL
        OR EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_events_v1 AS event
            WHERE event.application_id = expected_application_id
                AND event.interaction_id = expected_interaction_id
                AND event.action_index = expected_action_index
                AND event.event_revision = expected_effect_revision
                AND event.event_kind = expected_effect_event_kind
                AND event.from_state = 'observing'
                AND event.to_state = expected_effect_state
                AND event.receipt_claim_revision IS NULL
                AND event.recovery_claim_revision
                    = expected_recovery_claim_revision
                AND event.process_instance_id = expected_process_instance_id
                AND event.outcome_code = expected_effect_event_outcome
                AND event.result_digest
                    IS NOT DISTINCT FROM proposed_observation_digest
                AND event.output_kind = 'original_response'
                AND event.output_id IS NULL
                AND event.event_digest = CASE
                    WHEN proposed_observation_outcome = 'token_unrecoverable'
                        OR recovery_blocked
                    THEN pg_catalog.sha256(pg_catalog.convert_to(
                        pg_catalog.concat_ws(
                            '|',
                            'starring-runtime-interaction-effect-event-v1',
                            expected_application_id,
                            expected_interaction_id,
                            expected_action_index::TEXT,
                            expected_effect_revision::TEXT,
                            expected_effect_event_kind,
                            'observing',
                            expected_effect_state,
                            expected_recovery_claim_revision::TEXT,
                            expected_process_instance_id,
                            expected_effect_event_outcome,
                            pg_catalog.encode(
                                proposed_observation_digest,
                                'hex'
                            )
                        ),
                        'UTF8'
                    ))
                    ELSE pg_catalog.sha256(pg_catalog.convert_to(
                        pg_catalog.concat_ws(
                            '|',
                            'starring-runtime-interaction-effect-event-v1',
                            expected_application_id,
                            expected_interaction_id,
                            expected_action_index::TEXT,
                            expected_effect_revision::TEXT,
                            expected_effect_event_kind,
                            'observing',
                            expected_effect_state,
                            expected_recovery_claim_revision::TEXT,
                            expected_process_instance_id,
                            expected_effect_event_outcome,
                            pg_catalog.encode(
                                proposed_observation_digest,
                                'hex'
                            ),
                            'original_response',
                            ''
                        ),
                        'UTF8'
                    ))
                END
        );

    IF receipt_head.state = 'completed' THEN
        IF receipt_head.head_revision <> expected_receipt_head_revision + 1
            OR expected_terminal_outcome IS NULL
            OR effect_head.state <> expected_effect_state
            OR effect_head.head_revision <> expected_effect_revision
            OR NOT replay_event_matches
            OR (
                proposed_observation_outcome = 'close_known_state'
                AND effect_head.updated_at >= receipt_head.terminal_at
            )
            OR receipt_head.terminal_outcome_code
                <> expected_terminal_outcome
            OR receipt_head.terminal_result_digest
                IS DISTINCT FROM proposed_terminal_result_digest
            OR NOT EXISTS (
                SELECT 1
                FROM public.runtime_interaction_receipt_events_v1 AS event
                WHERE event.application_id = expected_application_id
                    AND event.interaction_id = expected_interaction_id
                    AND event.event_revision = receipt_head.head_revision
                    AND event.event_kind = 'completed'
                    AND event.from_state = 'executing'
                    AND event.to_state = 'completed'
                    AND event.from_acknowledgement_state
                        = receipt_head.acknowledgement_state
                    AND event.to_acknowledgement_state
                        = receipt_head.acknowledgement_state
                    AND event.claim_revision = receipt_head.claim_revision
                    AND event.claim_process_instance_id
                        = receipt_head.claim_process_instance_id
                    AND event.claim_gateway_shard_id
                        = receipt_head.claim_gateway_shard_id
                    AND event.claim_gateway_owner_lease_epoch
                        = receipt_head.claim_gateway_owner_lease_epoch
                    AND event.claim_gateway_owner_revision
                        = receipt_head.claim_gateway_owner_revision
                    AND event.claim_serving_lease_epoch
                        = receipt_head.claim_serving_lease_epoch
                    AND event.claim_serving_revision
                        = receipt_head.claim_serving_revision
                    AND event.outcome_code = expected_terminal_outcome
                    AND event.event_digest = pg_catalog.sha256(
                        pg_catalog.convert_to(
                            pg_catalog.concat_ws(
                                '|',
                                'starring-runtime-interaction-receipt-event-v1',
                                expected_application_id,
                                expected_interaction_id,
                                receipt_head.head_revision::TEXT,
                                'completed',
                                'executing',
                                'completed',
                                receipt_head.acknowledgement_state,
                                receipt_head.acknowledgement_state,
                                receipt_head.claim_revision::TEXT,
                                receipt_head.claim_process_instance_id,
                                receipt_head.claim_gateway_shard_id,
                                receipt_head.claim_gateway_owner_lease_epoch::TEXT,
                                receipt_head.claim_gateway_owner_revision::TEXT,
                                receipt_head.claim_serving_lease_epoch::TEXT,
                                receipt_head.claim_serving_revision::TEXT,
                                expected_terminal_outcome
                            ),
                            'UTF8'
                        )
                    )
                    AND event.observed_at = receipt_head.terminal_at
            )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI001',
                MESSAGE = 'runtime_interaction_effect_response_tail_finalize_conflict';
        END IF;

        outcome_name := 'exact_replay';
        effect_state := effect_head.state;
        resulting_effect_head_revision := effect_head.head_revision;
        receipt_state := receipt_head.state;
        resulting_receipt_head_revision := receipt_head.head_revision;
        resulting_recovery_at := effect_head.next_recovery_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF receipt_head.state <> 'executing'
        OR receipt_head.state <> expected_receipt_state
        OR receipt_head.head_revision <> expected_receipt_head_revision
        OR effect_head.head_revision <> expected_effect_head_revision
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_response_tail_finalize_conflict';
    END IF;

    IF proposed_observation_outcome = 'close_known_state' THEN
        terminal_outcome := CASE effect_head.state
            WHEN 'planned'
                THEN 'provisioning_completed_response_unconfirmed'
            WHEN 'known_succeeded' THEN 'effects_recovered_completed'
            WHEN 'reconciled_succeeded' THEN 'effects_recovered_completed'
            WHEN 'known_failed'
                THEN 'provisioning_completed_response_unconfirmed'
            WHEN 'recovery_required'
                THEN 'interaction_response_unrecoverable'
            ELSE NULL
        END;

        IF terminal_outcome IS NULL THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI001',
                MESSAGE = 'runtime_interaction_effect_response_tail_finalize_conflict';
        END IF;
    ELSIF proposed_observation_outcome = 'token_unrecoverable'
        OR recovery_blocked
    THEN
        IF effect_head.state <> 'observing'
            OR effect_head.recovery_process_instance_id
                IS DISTINCT FROM expected_process_instance_id
            OR effect_head.recovery_gateway_shard_id
                IS DISTINCT FROM expected_gateway_shard_id
            OR effect_head.recovery_runtime_build_revision
                IS DISTINCT FROM expected_runtime_build_revision
            OR effect_head.recovery_expires_at <= database_now
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI001',
                MESSAGE = 'runtime_interaction_effect_response_tail_finalize_conflict';
        END IF;

        resulting_digest := COALESCE(
            effect_head.result_digest,
            proposed_observation_digest
        );

        UPDATE public.runtime_interaction_effect_heads_v1 AS head
        SET state = 'recovery_required',
            head_revision = head.head_revision + 1,
            result_digest = resulting_digest,
            result_at = COALESCE(head.result_at, database_now),
            recovery_process_instance_id = NULL,
            recovery_gateway_shard_id = NULL,
            recovery_runtime_build_revision = NULL,
            recovery_acquired_at = NULL,
            recovery_expires_at = NULL,
            next_recovery_at = NULL,
            updated_at = database_now
        WHERE head.application_id = expected_application_id
            AND head.interaction_id = expected_interaction_id
            AND head.action_index = expected_action_index;

        INSERT INTO public.runtime_interaction_effect_events_v1 (
            application_id,
            interaction_id,
            action_index,
            event_revision,
            event_kind,
            from_state,
            to_state,
            receipt_claim_revision,
            recovery_claim_revision,
            process_instance_id,
            outcome_code,
            result_digest,
            output_kind,
            output_id,
            event_digest,
            observed_at
        ) VALUES (
            expected_application_id,
            expected_interaction_id,
            expected_action_index,
            effect_head.head_revision + 1,
            'recovery_required',
            effect_head.state,
            'recovery_required',
            NULL,
            effect_head.recovery_claim_revision,
            expected_process_instance_id,
            CASE
                WHEN recovery_blocked THEN proposed_observation_outcome
                ELSE 'interaction_response_unrecoverable'
            END,
            proposed_observation_digest,
            effect_head.output_kind,
            NULL,
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.concat_ws(
                    '|',
                    'starring-runtime-interaction-effect-event-v1',
                    expected_application_id,
                    expected_interaction_id,
                    expected_action_index::TEXT,
                    (effect_head.head_revision + 1)::TEXT,
                    'recovery_required',
                    effect_head.state,
                    'recovery_required',
                    effect_head.recovery_claim_revision::TEXT,
                    expected_process_instance_id,
                    CASE
                        WHEN recovery_blocked
                            THEN proposed_observation_outcome
                        ELSE 'interaction_response_unrecoverable'
                    END,
                    pg_catalog.encode(proposed_observation_digest, 'hex')
                ),
                'UTF8'
            )),
            database_now
        );

        effect_head.state := 'recovery_required';
        effect_head.head_revision := effect_head.head_revision + 1;
        terminal_outcome := 'interaction_response_unrecoverable';
    ELSE
        IF effect_head.state <> 'observing'
            OR effect_head.recovery_process_instance_id
                IS DISTINCT FROM expected_process_instance_id
            OR effect_head.recovery_gateway_shard_id
                IS DISTINCT FROM expected_gateway_shard_id
            OR effect_head.recovery_runtime_build_revision
                IS DISTINCT FROM expected_runtime_build_revision
            OR effect_head.recovery_expires_at <= database_now
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI001',
                MESSAGE = 'runtime_interaction_effect_response_tail_finalize_conflict';
        END IF;

        SELECT reconcile.*
        INTO reconciliation
        FROM public.starring_runtime_interaction_effect_reconcile_v1(
            expected_application_id,
            expected_interaction_id,
            expected_action_index,
            expected_effect_head_revision,
            expected_recovery_claim_revision,
            expected_process_instance_id,
            expected_gateway_shard_id,
            expected_runtime_build_revision,
            expected_runtime_generation,
            expected_controller_fencing_token,
            expected_route_incarnation,
            'observing',
            'response_tail',
            expected_preflight_certificate_digest,
            CASE proposed_observation_outcome
                WHEN 'exact_success' THEN 'adopted_success'
                WHEN 'exact_absence' THEN 'observed_failure'
                ELSE proposed_observation_outcome
            END,
            proposed_observation_digest,
            '',
            requested_retry_delay_milliseconds
        ) AS reconcile;

        effect_head.state := reconciliation.effect_state;
        effect_head.head_revision :=
            reconciliation.resulting_effect_head_revision;
        effect_head.next_recovery_at := reconciliation.resulting_recovery_at;

        terminal_outcome := CASE proposed_observation_outcome
            WHEN 'exact_success' THEN 'effects_recovered_completed'
            WHEN 'exact_absence'
                THEN 'provisioning_completed_response_unconfirmed'
            WHEN 'conflict' THEN 'interaction_response_unrecoverable'
            WHEN 'unsupported' THEN 'interaction_response_unrecoverable'
            ELSE NULL
        END;
        IF recovery_blocked THEN
            terminal_outcome := 'interaction_response_unrecoverable';
        END IF;
    END IF;

    IF terminal_outcome IS NULL THEN
        outcome_name := proposed_observation_outcome;
        effect_state := effect_head.state;
        resulting_effect_head_revision := effect_head.head_revision;
        receipt_state := receipt_head.state;
        resulting_receipt_head_revision := receipt_head.head_revision;
        resulting_recovery_at := effect_head.next_recovery_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    receipt_revision :=
        public.starring_runtime_interaction_effect_complete_receipt_v1(
            expected_application_id,
            expected_interaction_id,
            terminal_outcome,
            proposed_terminal_result_digest,
            database_now
        );

    outcome_name := terminal_outcome;
    effect_state := effect_head.state;
    resulting_effect_head_revision := effect_head.head_revision;
    receipt_state := 'completed';
    resulting_receipt_head_revision := receipt_revision;
    resulting_recovery_at := effect_head.next_recovery_at;
    observed_database_now := database_now;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_effect_compensation_intend_v1(
    expected_application_id TEXT,
    expected_interaction_id TEXT,
    expected_action_index BIGINT,
    expected_effect_head_revision BIGINT,
    expected_process_instance_id TEXT,
    expected_gateway_shard_id TEXT,
    expected_runtime_build_revision TEXT,
    expected_runtime_generation BIGINT,
    expected_controller_fencing_token BIGINT,
    expected_route_incarnation BIGINT,
    expected_preflight_certificate_digest BYTEA,
    proposed_compensation_intent_digest BYTEA,
    requested_recovery_delay_milliseconds BIGINT
)
RETURNS TABLE(
    outcome_name TEXT,
    effect_state TEXT,
    resulting_effect_head_revision BIGINT,
    resulting_recovery_claim_revision BIGINT,
    resulting_recovery_at TIMESTAMPTZ,
    observed_database_now TIMESTAMPTZ
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
    receipt_root public.runtime_interaction_receipt_roots_v1%ROWTYPE;
    receipt_head public.runtime_interaction_receipt_heads_v1%ROWTYPE;
    effect_root public.runtime_interaction_effect_roots_v1%ROWTYPE;
    rollback_row public.runtime_interaction_effect_rollbacks_v1%ROWTYPE;
    effect_head public.runtime_interaction_effect_heads_v1%ROWTYPE;
    database_now TIMESTAMPTZ;
    recovery_at TIMESTAMPTZ;
    authority_available BOOLEAN;
    blocked_digest BYTEA;
    budget_source_state TEXT;
BEGIN
    IF expected_application_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_application_id) > 20
        OR expected_interaction_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_interaction_id) > 20
        OR expected_action_index NOT BETWEEN 0 AND 255
        OR expected_effect_head_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_gateway_shard_id !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR expected_runtime_build_revision !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR expected_runtime_generation NOT BETWEEN 1 AND 9223372036854775807
        OR expected_controller_fencing_token
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_route_incarnation NOT BETWEEN 1 AND 9223372036854775807
        OR pg_catalog.octet_length(
            expected_preflight_certificate_digest
        ) <> 32
        OR pg_catalog.octet_length(
            proposed_compensation_intent_digest
        ) <> 32
        OR requested_recovery_delay_milliseconds NOT BETWEEN 1000 AND 60000
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_effect_compensation_intend_input_invalid';
    END IF;

    database_now := pg_catalog.clock_timestamp();
    recovery_at := database_now
        + requested_recovery_delay_milliseconds * INTERVAL '1 millisecond';

    SELECT root.*
    INTO receipt_root
    FROM public.runtime_interaction_receipt_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    IF NOT FOUND
        OR receipt_root.runtime_generation <> expected_runtime_generation
        OR receipt_root.route_controller_fencing_token
            <> expected_controller_fencing_token
        OR receipt_root.route_incarnation <> expected_route_incarnation
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_effect_compensation_authority_stale';
    END IF;

    SELECT head.*
    INTO receipt_head
    FROM public.runtime_interaction_receipt_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_effect_receipt_head_missing';
    END IF;

    IF receipt_head.state NOT IN (
            'executing',
            'failed',
            'recovery_required'
        )
        OR (
            receipt_head.state = 'executing'
            AND receipt_head.claim_expires_at > database_now
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_effect_compensation_active_receipt';
    END IF;

    SELECT EXISTS (
        SELECT 1
        FROM public.starring_runtime_interaction_receipt_authority_observe_v1(
            receipt_root.application_id,
            receipt_root.tenant_id,
            receipt_root.installation_id,
            receipt_root.deployment_id,
            receipt_root.guild_id,
            receipt_root.ruleset_key,
            receipt_root.target_version,
            receipt_root.target_content_hash,
            receipt_root.binding_revision,
            receipt_root.binding_fingerprint,
            receipt_root.runtime_generation,
            receipt_root.route_controller_fencing_token,
            receipt_root.route_incarnation,
            expected_process_instance_id,
            expected_gateway_shard_id,
            expected_runtime_build_revision,
            receipt_root.route_kind,
            COALESCE(receipt_root.instance_id, '')
        ) AS authority
    )
    INTO authority_available;

    IF NOT authority_available THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_effect_compensation_authority_stale';
    END IF;

    SELECT root.*
    INTO effect_root
    FROM public.runtime_interaction_effect_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    IF NOT FOUND
        OR effect_root.preflight_certificate_digest
            IS DISTINCT FROM expected_preflight_certificate_digest
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_effect_plan_corruption';
    END IF;

    SELECT rollback.*
    INTO rollback_row
    FROM public.runtime_interaction_effect_rollbacks_v1 AS rollback
    WHERE rollback.application_id = expected_application_id
        AND rollback.interaction_id = expected_interaction_id
    FOR UPDATE;

    IF NOT FOUND
        OR rollback_row.state <> 'required'
        OR expected_action_index > rollback_row.abort_action_index
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_compensation_not_authorized';
    END IF;

    SELECT head.*
    INTO effect_head
    FROM public.runtime_interaction_effect_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
        AND head.action_index = expected_action_index
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_action_not_found';
    END IF;

    budget_source_state := effect_head.state;
    IF effect_head.state = 'recovery_required'
        AND effect_head.head_revision = expected_effect_head_revision + 1
    THEN
        SELECT event.from_state
        INTO budget_source_state
        FROM public.runtime_interaction_effect_events_v1 AS event
        WHERE event.application_id = expected_application_id
            AND event.interaction_id = expected_interaction_id
            AND event.action_index = expected_action_index
            AND event.event_revision = effect_head.head_revision
            AND event.event_kind = 'recovery_required'
            AND event.outcome_code
                = 'recovery_blocked_attempt_budget_exhausted';
    END IF;

    blocked_digest := pg_catalog.sha256(pg_catalog.convert_to(
        pg_catalog.concat_ws(
            '|',
            'starring-runtime-interaction-effect-attempt-budget-block-v1',
            expected_application_id,
            expected_interaction_id,
            expected_action_index::TEXT,
            expected_effect_head_revision::TEXT,
            effect_head.recovery_claim_revision::TEXT,
            expected_process_instance_id,
            expected_gateway_shard_id,
            expected_runtime_build_revision,
            expected_runtime_generation::TEXT,
            expected_controller_fencing_token::TEXT,
            expected_route_incarnation::TEXT,
            budget_source_state,
            'compensation_intent',
            effect_head.compensation_attempt_count::TEXT,
            pg_catalog.encode(
                effect_root.preflight_certificate_digest,
                'hex'
            ),
            pg_catalog.encode(
                proposed_compensation_intent_digest,
                'hex'
            ),
            'recovery_blocked_attempt_budget_exhausted'
        ),
        'UTF8'
    ));

    IF effect_head.state = 'recovery_required'
        AND effect_head.head_revision = expected_effect_head_revision + 1
        AND effect_head.compensation_attempt_count >= 64
        AND EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_events_v1 AS event
            WHERE event.application_id = expected_application_id
                AND event.interaction_id = expected_interaction_id
                AND event.action_index = expected_action_index
                AND event.event_revision = effect_head.head_revision
                AND event.event_kind = 'recovery_required'
                AND event.from_state = budget_source_state
                AND budget_source_state IN (
                    'known_succeeded',
                    'reconciled_succeeded'
                )
                AND event.to_state = 'recovery_required'
                AND event.recovery_claim_revision
                    = effect_head.recovery_claim_revision
                AND event.process_instance_id = expected_process_instance_id
                AND event.outcome_code
                    = 'recovery_blocked_attempt_budget_exhausted'
                AND event.result_digest = blocked_digest
                AND event.output_kind = effect_head.output_kind
                AND event.output_id IS NULL
                AND event.event_digest = pg_catalog.sha256(
                    pg_catalog.convert_to(
                        pg_catalog.concat_ws(
                            '|',
                            'starring-runtime-interaction-effect-event-v1',
                            expected_application_id,
                            expected_interaction_id,
                            expected_action_index::TEXT,
                            (expected_effect_head_revision + 1)::TEXT,
                            'recovery_required',
                            event.from_state,
                            'recovery_required',
                            effect_head.recovery_claim_revision::TEXT,
                            expected_process_instance_id,
                            'recovery_blocked_attempt_budget_exhausted',
                            pg_catalog.encode(blocked_digest, 'hex')
                        ),
                        'UTF8'
                    )
                )
        )
    THEN
        outcome_name := 'exact_replay';
        effect_state := effect_head.state;
        resulting_effect_head_revision := effect_head.head_revision;
        resulting_recovery_claim_revision :=
            effect_head.recovery_claim_revision;
        resulting_recovery_at := database_now;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF effect_head.state = 'compensation_intended'
        AND effect_head.head_revision IN (
            expected_effect_head_revision,
            expected_effect_head_revision + 1
        )
        AND effect_head.compensation_intent_digest
            IS NOT DISTINCT FROM proposed_compensation_intent_digest
        AND effect_head.recovery_process_instance_id
            IS NOT DISTINCT FROM expected_process_instance_id
        AND effect_head.recovery_gateway_shard_id
            IS NOT DISTINCT FROM expected_gateway_shard_id
        AND effect_head.recovery_runtime_build_revision
            IS NOT DISTINCT FROM expected_runtime_build_revision
        AND effect_head.recovery_expires_at > database_now
    THEN
        outcome_name := 'exact_replay';
        effect_state := effect_head.state;
        resulting_effect_head_revision := effect_head.head_revision;
        resulting_recovery_claim_revision :=
            effect_head.recovery_claim_revision;
        resulting_recovery_at := effect_head.recovery_expires_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF effect_head.state IN (
            'known_succeeded',
            'reconciled_succeeded'
        )
        AND effect_head.head_revision = expected_effect_head_revision
        AND effect_head.action_kind NOT IN (
            'teardown_instance',
            'edit_response'
        )
        AND effect_head.resolved_effect_identity_digest IS NOT NULL
        AND effect_head.success_binding_kind IN (
            'attempt_result',
            'observation'
        )
        AND effect_head.success_binding_digest IS NOT NULL
        AND (
            effect_head.state <> 'known_succeeded'
            OR (
                effect_head.success_binding_kind = 'attempt_result'
                AND effect_head.success_binding_digest
                    IS NOT DISTINCT FROM effect_head.result_digest
            )
        )
        AND effect_head.compensation_attempt_count >= 64
        AND NOT EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_heads_v1 AS dependent
            WHERE dependent.application_id = expected_application_id
                AND dependent.interaction_id = expected_interaction_id
                AND expected_action_index::SMALLINT
                    = ANY(dependent.dependency_indices)
                AND dependent.state NOT IN (
                    'planned',
                    'known_failed',
                    'compensated'
                )
        )
    THEN
        UPDATE public.runtime_interaction_effect_heads_v1 AS head
        SET state = 'recovery_required',
            head_revision = head.head_revision + 1,
            compensation_result_digest = COALESCE(
                head.compensation_result_digest,
                blocked_digest
            ),
            compensation_result_at = COALESCE(
                head.compensation_result_at,
                database_now
            ),
            recovery_process_instance_id = NULL,
            recovery_gateway_shard_id = NULL,
            recovery_runtime_build_revision = NULL,
            recovery_acquired_at = NULL,
            recovery_expires_at = NULL,
            next_recovery_at = NULL,
            updated_at = database_now
        WHERE head.application_id = expected_application_id
            AND head.interaction_id = expected_interaction_id
            AND head.action_index = expected_action_index;

        INSERT INTO public.runtime_interaction_effect_events_v1 (
            application_id,
            interaction_id,
            action_index,
            event_revision,
            event_kind,
            from_state,
            to_state,
            receipt_claim_revision,
            recovery_claim_revision,
            process_instance_id,
            outcome_code,
            result_digest,
            output_kind,
            output_id,
            event_digest,
            observed_at
        ) VALUES (
            expected_application_id,
            expected_interaction_id,
            expected_action_index,
            effect_head.head_revision + 1,
            'recovery_required',
            effect_head.state,
            'recovery_required',
            NULL,
            effect_head.recovery_claim_revision,
            expected_process_instance_id,
            'recovery_blocked_attempt_budget_exhausted',
            blocked_digest,
            effect_head.output_kind,
            NULL,
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.concat_ws(
                    '|',
                    'starring-runtime-interaction-effect-event-v1',
                    expected_application_id,
                    expected_interaction_id,
                    expected_action_index::TEXT,
                    (effect_head.head_revision + 1)::TEXT,
                    'recovery_required',
                    effect_head.state,
                    'recovery_required',
                    effect_head.recovery_claim_revision::TEXT,
                    expected_process_instance_id,
                    'recovery_blocked_attempt_budget_exhausted',
                    pg_catalog.encode(blocked_digest, 'hex')
                ),
                'UTF8'
            )),
            database_now
        );

        outcome_name := 'recovery_blocked_attempt_budget_exhausted';
        effect_state := 'recovery_required';
        resulting_effect_head_revision := effect_head.head_revision + 1;
        resulting_recovery_claim_revision :=
            effect_head.recovery_claim_revision;
        resulting_recovery_at := database_now;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF effect_head.state NOT IN (
            'known_succeeded',
            'reconciled_succeeded'
        )
        OR effect_head.head_revision <> expected_effect_head_revision
        OR effect_head.action_kind IN ('teardown_instance', 'edit_response')
        OR effect_head.resolved_effect_identity_digest IS NULL
        OR effect_head.success_binding_kind NOT IN (
            'attempt_result',
            'observation'
        )
        OR effect_head.success_binding_digest IS NULL
        OR (
            effect_head.state = 'known_succeeded'
            AND (
                effect_head.success_binding_kind <> 'attempt_result'
                OR effect_head.success_binding_digest
                    IS DISTINCT FROM effect_head.result_digest
            )
        )
        OR (
            effect_head.state = 'reconciled_succeeded'
            AND effect_head.success_binding_kind <> 'observation'
        )
        OR effect_head.recovery_claim_revision = 9223372036854775807
        OR EXISTS (
            SELECT 1
            FROM public.runtime_interaction_effect_heads_v1 AS dependent
            WHERE dependent.application_id = expected_application_id
                AND dependent.interaction_id = expected_interaction_id
                AND expected_action_index::SMALLINT
                    = ANY(dependent.dependency_indices)
                AND dependent.state NOT IN (
                    'planned',
                    'known_failed',
                    'compensated'
                )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_compensation_intend_conflict';
    END IF;

    UPDATE public.runtime_interaction_effect_heads_v1 AS head
    SET state = 'compensation_intended',
        head_revision = head.head_revision + 1,
        compensation_attempt_count = head.compensation_attempt_count + 1,
        recovery_claim_revision = head.recovery_claim_revision + 1,
        recovery_process_instance_id = expected_process_instance_id,
        recovery_gateway_shard_id = expected_gateway_shard_id,
        recovery_runtime_build_revision = expected_runtime_build_revision,
        recovery_acquired_at = database_now,
        recovery_expires_at = recovery_at,
        next_recovery_at = recovery_at,
        compensation_intent_digest = proposed_compensation_intent_digest,
        compensation_intent_at = database_now,
        updated_at = database_now
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
        AND head.action_index = expected_action_index;

    INSERT INTO public.runtime_interaction_effect_events_v1 (
        application_id,
        interaction_id,
        action_index,
        event_revision,
        event_kind,
        from_state,
        to_state,
        receipt_claim_revision,
        recovery_claim_revision,
        process_instance_id,
        outcome_code,
        result_digest,
        output_kind,
        output_id,
        event_digest,
        observed_at
    ) VALUES (
        expected_application_id,
        expected_interaction_id,
        expected_action_index,
        effect_head.head_revision + 1,
        'compensation_intended',
        effect_head.state,
        'compensation_intended',
        NULL,
        effect_head.recovery_claim_revision + 1,
        expected_process_instance_id,
        'compensation_intended',
        proposed_compensation_intent_digest,
        effect_head.output_kind,
        effect_head.output_id,
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.concat_ws(
                '|',
                'starring-runtime-interaction-effect-event-v1',
                expected_application_id,
                expected_interaction_id,
                expected_action_index::TEXT,
                (effect_head.head_revision + 1)::TEXT,
                'compensation_intended',
                effect_head.state,
                'compensation_intended',
                (effect_head.recovery_claim_revision + 1)::TEXT,
                expected_process_instance_id,
                pg_catalog.encode(
                    proposed_compensation_intent_digest,
                    'hex'
                )
            ),
            'UTF8'
        )),
        database_now
    );

    outcome_name := 'compensation_intended';
    effect_state := 'compensation_intended';
    resulting_effect_head_revision := effect_head.head_revision + 1;
    resulting_recovery_claim_revision :=
        effect_head.recovery_claim_revision + 1;
    resulting_recovery_at := recovery_at;
    observed_database_now := database_now;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_effect_compensation_finish_v1(
    expected_application_id TEXT,
    expected_interaction_id TEXT,
    expected_action_index BIGINT,
    expected_effect_head_revision BIGINT,
    expected_recovery_claim_revision BIGINT,
    expected_process_instance_id TEXT,
    expected_preflight_certificate_digest BYTEA,
    proposed_compensation_outcome TEXT,
    proposed_compensation_result_digest BYTEA,
    requested_retry_delay_milliseconds BIGINT
)
RETURNS TABLE(
    outcome_name TEXT,
    effect_state TEXT,
    resulting_effect_head_revision BIGINT,
    resulting_recovery_at TIMESTAMPTZ,
    observed_database_now TIMESTAMPTZ
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
    effect_root public.runtime_interaction_effect_roots_v1%ROWTYPE;
    rollback_row public.runtime_interaction_effect_rollbacks_v1%ROWTYPE;
    effect_head public.runtime_interaction_effect_heads_v1%ROWTYPE;
    database_now TIMESTAMPTZ;
    next_state TEXT;
    next_event_kind TEXT;
    recovery_at TIMESTAMPTZ;
    replay_matches BOOLEAN;
BEGIN
    IF expected_application_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_application_id) > 20
        OR expected_interaction_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_interaction_id) > 20
        OR expected_action_index NOT BETWEEN 0 AND 255
        OR expected_effect_head_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR expected_recovery_claim_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR pg_catalog.octet_length(
            expected_preflight_certificate_digest
        ) <> 32
        OR proposed_compensation_outcome NOT IN (
            'compensated',
            'indeterminate',
            'definitive_failure'
        )
        OR pg_catalog.octet_length(
            proposed_compensation_result_digest
        ) <> 32
        OR requested_retry_delay_milliseconds NOT BETWEEN 1000 AND 60000
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_effect_compensation_finish_input_invalid';
    END IF;

    database_now := pg_catalog.clock_timestamp();
    next_state := CASE proposed_compensation_outcome
        WHEN 'compensated' THEN 'compensated'
        WHEN 'indeterminate' THEN 'compensation_indeterminate'
        ELSE 'recovery_required'
    END;
    next_event_kind := CASE proposed_compensation_outcome
        WHEN 'compensated' THEN 'compensated'
        WHEN 'indeterminate' THEN 'compensation_indeterminate'
        ELSE 'recovery_required'
    END;
    recovery_at := CASE
        WHEN proposed_compensation_outcome = 'indeterminate'
            THEN database_now
                + requested_retry_delay_milliseconds
                    * INTERVAL '1 millisecond'
        ELSE NULL
    END;

    PERFORM root.application_id
    FROM public.runtime_interaction_receipt_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_receipt_not_found';
    END IF;

    PERFORM head.application_id
    FROM public.runtime_interaction_receipt_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
    FOR SHARE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_effect_receipt_head_missing';
    END IF;

    SELECT root.*
    INTO effect_root
    FROM public.runtime_interaction_effect_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    IF NOT FOUND
        OR effect_root.preflight_certificate_digest
            IS DISTINCT FROM expected_preflight_certificate_digest
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_effect_plan_corruption';
    END IF;

    SELECT rollback.*
    INTO rollback_row
    FROM public.runtime_interaction_effect_rollbacks_v1 AS rollback
    WHERE rollback.application_id = expected_application_id
        AND rollback.interaction_id = expected_interaction_id
    FOR UPDATE;

    IF NOT FOUND
        OR rollback_row.state <> 'required'
        OR expected_action_index > rollback_row.abort_action_index
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_compensation_not_authorized';
    END IF;

    SELECT head.*
    INTO effect_head
    FROM public.runtime_interaction_effect_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
        AND head.action_index = expected_action_index
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_action_not_found';
    END IF;

    SELECT EXISTS (
        SELECT 1
        FROM public.runtime_interaction_effect_events_v1 AS event
        WHERE event.application_id = expected_application_id
            AND event.interaction_id = expected_interaction_id
            AND event.action_index = expected_action_index
            AND event.event_revision = effect_head.head_revision
            AND event.event_kind = next_event_kind
            AND event.from_state = 'compensation_intended'
            AND event.to_state = next_state
            AND event.receipt_claim_revision IS NULL
            AND event.recovery_claim_revision
                = expected_recovery_claim_revision
            AND event.process_instance_id = expected_process_instance_id
            AND event.outcome_code = proposed_compensation_outcome
            AND event.result_digest
                = proposed_compensation_result_digest
            AND event.output_kind = effect_head.output_kind
            AND event.output_id IS NOT DISTINCT FROM effect_head.output_id
            AND event.event_digest = pg_catalog.sha256(
                pg_catalog.convert_to(
                    pg_catalog.concat_ws(
                        '|',
                        'starring-runtime-interaction-effect-event-v1',
                        expected_application_id,
                        expected_interaction_id,
                        expected_action_index::TEXT,
                        (expected_effect_head_revision + 1)::TEXT,
                        next_event_kind,
                        'compensation_intended',
                        next_state,
                        expected_recovery_claim_revision::TEXT,
                        expected_process_instance_id,
                        proposed_compensation_outcome,
                        pg_catalog.encode(
                            proposed_compensation_result_digest,
                            'hex'
                        )
                    ),
                    'UTF8'
                )
            )
    )
    INTO replay_matches;

    IF effect_head.state = next_state
        AND effect_head.head_revision = expected_effect_head_revision + 1
        AND effect_head.recovery_claim_revision
            = expected_recovery_claim_revision
        AND replay_matches
    THEN
        outcome_name := 'exact_replay';
        effect_state := effect_head.state;
        resulting_effect_head_revision := effect_head.head_revision;
        resulting_recovery_at := effect_head.next_recovery_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF effect_head.state <> 'compensation_intended'
        OR effect_head.head_revision <> expected_effect_head_revision
        OR effect_head.resolved_effect_identity_digest IS NULL
        OR effect_head.success_binding_kind NOT IN (
            'attempt_result',
            'observation'
        )
        OR effect_head.success_binding_digest IS NULL
        OR effect_head.recovery_claim_revision
            <> expected_recovery_claim_revision
        OR effect_head.recovery_process_instance_id
            IS DISTINCT FROM expected_process_instance_id
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_compensation_finish_conflict';
    END IF;

    UPDATE public.runtime_interaction_effect_heads_v1 AS head
    SET state = next_state,
        head_revision = head.head_revision + 1,
        recovery_process_instance_id = NULL,
        recovery_gateway_shard_id = NULL,
        recovery_runtime_build_revision = NULL,
        recovery_acquired_at = NULL,
        recovery_expires_at = NULL,
        next_recovery_at = recovery_at,
        compensation_result_digest =
            proposed_compensation_result_digest,
        compensation_result_at = database_now,
        updated_at = database_now
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
        AND head.action_index = expected_action_index;

    INSERT INTO public.runtime_interaction_effect_events_v1 (
        application_id,
        interaction_id,
        action_index,
        event_revision,
        event_kind,
        from_state,
        to_state,
        receipt_claim_revision,
        recovery_claim_revision,
        process_instance_id,
        outcome_code,
        result_digest,
        output_kind,
        output_id,
        event_digest,
        observed_at
    ) VALUES (
        expected_application_id,
        expected_interaction_id,
        expected_action_index,
        effect_head.head_revision + 1,
        next_event_kind,
        effect_head.state,
        next_state,
        NULL,
        expected_recovery_claim_revision,
        expected_process_instance_id,
        proposed_compensation_outcome,
        proposed_compensation_result_digest,
        effect_head.output_kind,
        effect_head.output_id,
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.concat_ws(
                '|',
                'starring-runtime-interaction-effect-event-v1',
                expected_application_id,
                expected_interaction_id,
                expected_action_index::TEXT,
                (effect_head.head_revision + 1)::TEXT,
                next_event_kind,
                effect_head.state,
                next_state,
                expected_recovery_claim_revision::TEXT,
                expected_process_instance_id,
                proposed_compensation_outcome,
                pg_catalog.encode(
                    proposed_compensation_result_digest,
                    'hex'
                )
            ),
            'UTF8'
        )),
        database_now
    );

    PERFORM public.starring_runtime_interaction_effect_try_complete_rollback_v1(
        expected_application_id,
        expected_interaction_id,
        database_now
    );

    outcome_name := proposed_compensation_outcome;
    effect_state := next_state;
    resulting_effect_head_revision := effect_head.head_revision + 1;
    resulting_recovery_at := recovery_at;
    observed_database_now := database_now;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_effect_plan_bind_v1(
    expected_application_id TEXT,
    expected_interaction_id TEXT,
    expected_receipt_head_revision BIGINT,
    expected_receipt_claim_revision BIGINT,
    expected_process_instance_id TEXT,
    expected_action_plan_digest BYTEA,
    proposed_preflight_certificate_digest BYTEA,
    proposed_snapshot_digest BYTEA,
    proposed_actions JSONB
)
RETURNS TABLE(
    outcome_name TEXT,
    resulting_action_count SMALLINT,
    resulting_certificate_issued_at TIMESTAMPTZ,
    resulting_certificate_expires_at TIMESTAMPTZ,
    observed_database_now TIMESTAMPTZ
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
    receipt_head public.runtime_interaction_receipt_heads_v1%ROWTYPE;
    effect_root public.runtime_interaction_effect_roots_v1%ROWTYPE;
    action_entry RECORD;
    action_document JSONB;
    reference_entry RECORD;
    dependency_values SMALLINT[];
    expected_reference_slots TEXT[];
    observed_reference_slots TEXT[];
    action_number SMALLINT;
    action_name TEXT;
    planned_identity_value BYTEA;
    input_value BYTEA;
    expected_value BYTEA;
    preimage_value BYTEA;
    output_name TEXT;
    correlation_name TEXT;
    correlation_value BYTEA;
    marker_value TEXT;
    planned_recovery_input_value JSONB;
    action_total INTEGER;
    planned_preimage_document JSONB;
    object_key_count BIGINT;
    matching_count BIGINT;
    database_now TIMESTAMPTZ;
BEGIN
    IF expected_application_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_application_id) > 20
        OR expected_interaction_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_interaction_id) > 20
        OR expected_receipt_head_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_receipt_claim_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR pg_catalog.octet_length(expected_action_plan_digest) <> 32
        OR pg_catalog.octet_length(
            proposed_preflight_certificate_digest
        ) <> 32
        OR pg_catalog.octet_length(proposed_snapshot_digest) <> 32
        OR pg_catalog.jsonb_typeof(proposed_actions) <> 'array'
        OR pg_catalog.octet_length(proposed_actions::TEXT) > 2097152
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_effect_plan_bind_input_invalid';
    END IF;

    action_total := pg_catalog.jsonb_array_length(proposed_actions);

    IF action_total NOT BETWEEN 0 AND 256 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_effect_plan_bind_action_count_invalid';
    END IF;

    IF (
        SELECT pg_catalog.count(*)
        FROM pg_catalog.jsonb_array_elements(proposed_actions)
            WITH ORDINALITY AS response(value, ordinality)
        WHERE response.value->>'action_kind' = 'edit_response'
    ) <> (CASE WHEN action_total > 0 THEN 1 ELSE 0 END)
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_array_elements(proposed_actions)
                WITH ORDINALITY AS response(value, ordinality)
            WHERE response.value->>'action_kind' = 'edit_response'
                AND response.ordinality <> action_total
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_effect_response_tail_invalid';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM pg_catalog.jsonb_array_elements(proposed_actions)
            WITH ORDINALITY AS teardown(value, ordinality)
        CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(proposed_actions)
            WITH ORDINALITY AS successor(value, ordinality)
        WHERE teardown.value->>'action_kind' = 'teardown_instance'
            AND successor.ordinality > teardown.ordinality
            AND successor.value->>'action_kind' <> 'edit_response'
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_effect_teardown_commit_boundary_invalid';
    END IF;

    database_now := pg_catalog.clock_timestamp();

    PERFORM root.application_id
    FROM public.runtime_interaction_receipt_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_receipt_not_found';
    END IF;

    SELECT head.*
    INTO receipt_head
    FROM public.runtime_interaction_receipt_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
    FOR SHARE;

    IF NOT FOUND
        OR receipt_head.head_revision < expected_receipt_head_revision
        OR receipt_head.claim_revision <> expected_receipt_claim_revision
        OR receipt_head.claim_process_instance_id
            IS DISTINCT FROM expected_process_instance_id
        OR receipt_head.state NOT IN ('prepared', 'deferred', 'executing')
        OR receipt_head.action_plan_digest
            IS DISTINCT FROM expected_action_plan_digest
        OR (
            action_total > 0
            AND (
                receipt_head.state <> 'deferred'
                OR receipt_head.acknowledgement_kind <> 'defer_ephemeral'
                OR receipt_head.acknowledgement_state <> 'deferred'
                OR receipt_head.acknowledgement_result <> 'succeeded'
            )
        )
        OR (
            action_total = 0
            AND (
                receipt_head.state <> 'prepared'
                OR receipt_head.acknowledgement_kind IS NOT NULL
                OR receipt_head.acknowledgement_state <> 'unacknowledged'
                OR receipt_head.acknowledgement_result IS NOT NULL
            )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_effect_receipt_conflict';
    END IF;

    IF NOT public.starring_runtime_interaction_receipt_claim_current_v1(
        expected_application_id,
        expected_interaction_id,
        expected_receipt_claim_revision,
        expected_process_instance_id,
        database_now
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_effect_receipt_claim_stale';
    END IF;

    FOR action_entry IN
        SELECT element.value, element.ordinality
        FROM pg_catalog.jsonb_array_elements(proposed_actions)
            WITH ORDINALITY AS element(value, ordinality)
        ORDER BY element.ordinality
    LOOP
        action_document := action_entry.value;

        IF pg_catalog.jsonb_typeof(action_document) <> 'object'
            OR NOT action_document ?& ARRAY[
                'action_index',
                'action_kind',
                'dependency_indices',
                'planned_identity_digest',
                'input_digest',
                'expected_postimage_digest',
                'planned_recovery_input',
                'planned_preimage_digest',
                'planned_preimage',
                'output_kind',
                'correlation_class',
                'correlation_digest',
                'correlation_marker'
            ]
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI003',
                MESSAGE = 'runtime_interaction_effect_plan_action_shape_invalid';
        END IF;

        SELECT pg_catalog.count(*)
        INTO object_key_count
        FROM pg_catalog.jsonb_object_keys(action_document);

        IF object_key_count <> 13
            OR COALESCE(action_document->>'action_index', '')
                !~ '^(0|[1-9][0-9]{0,2})$'
            OR pg_catalog.jsonb_typeof(
                action_document->'dependency_indices'
            ) <> 'array'
            OR pg_catalog.jsonb_array_length(
                action_document->'dependency_indices'
            ) > 32
            OR COALESCE(
                action_document->>'planned_identity_digest',
                ''
            ) !~ '^[0-9a-f]{64}$'
            OR COALESCE(action_document->>'input_digest', '')
                !~ '^[0-9a-f]{64}$'
            OR COALESCE(
                action_document->>'expected_postimage_digest',
                ''
            ) !~ '^[0-9a-f]{64}$'
            OR pg_catalog.jsonb_typeof(
                action_document->'planned_recovery_input'
            ) <> 'object'
            OR pg_catalog.octet_length(
                (action_document->'planned_recovery_input')::TEXT
            ) NOT BETWEEN 2 AND 4096
            OR COALESCE(
                action_document->>'planned_preimage_digest',
                ''
            ) !~ '^[0-9a-f]{64}$'
            OR pg_catalog.jsonb_typeof(
                action_document->'planned_preimage'
            ) <> 'object'
            OR pg_catalog.octet_length(
                (action_document->'planned_preimage')::TEXT
            ) NOT BETWEEN 2 AND 4096
            OR pg_catalog.jsonb_typeof(action_document->'correlation_marker')
                NOT IN ('string', 'null')
            OR COALESCE(action_document->>'correlation_digest', '')
                !~ '^[0-9a-f]{64}$'
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI003',
                MESSAGE = 'runtime_interaction_effect_plan_action_value_invalid';
        END IF;

        action_number := (action_document->>'action_index')::SMALLINT;
        action_name := action_document->>'action_kind';
        output_name := action_document->>'output_kind';
        correlation_name := action_document->>'correlation_class';
        correlation_value := pg_catalog.decode(
            action_document->>'correlation_digest',
            'hex'
        );
        marker_value := action_document->>'correlation_marker';
        input_value := pg_catalog.decode(
            action_document->>'input_digest',
            'hex'
        );
        planned_identity_value := pg_catalog.decode(
            action_document->>'planned_identity_digest',
            'hex'
        );
        expected_value := pg_catalog.decode(
            action_document->>'expected_postimage_digest',
            'hex'
        );
        planned_recovery_input_value :=
            action_document->'planned_recovery_input';
        preimage_value := pg_catalog.decode(
            action_document->>'planned_preimage_digest',
            'hex'
        );
        planned_preimage_document := action_document->'planned_preimage';

        IF action_number <> action_entry.ordinality - 1
            OR action_number > 255
            OR action_name NOT IN (
                'create_role',
                'create_channel',
                'grant_role',
                'upsert_overwrite',
                'post_panel',
                'register_instance',
                'teardown_instance',
                'edit_response'
            )
            OR output_name IS DISTINCT FROM (CASE action_name
                WHEN 'create_role' THEN 'created_role'
                WHEN 'create_channel' THEN 'created_channel'
                WHEN 'grant_role' THEN 'role_membership'
                WHEN 'upsert_overwrite' THEN 'permission_overwrite'
                WHEN 'post_panel' THEN 'posted_message'
                WHEN 'register_instance' THEN 'instance_state'
                WHEN 'teardown_instance' THEN 'instance_state'
                ELSE 'original_response'
            END)
            OR (
                action_name IN (
                    'create_role',
                    'create_channel',
                    'grant_role',
                    'upsert_overwrite'
                )
                AND (
                    correlation_name <> 'audit_log_reason'
                    OR COALESCE(marker_value, '') !~ '^[0-9a-f]{64}$'
                )
            )
            OR (
                action_name = 'post_panel'
                AND NOT (
                    (
                        correlation_name = 'message_nonce'
                        AND marker_value ~ '^[1-9][0-9]{0,19}$'
                        AND (
                            pg_catalog.length(marker_value) < 20
                            OR marker_value <= '18446744073709551615'
                        )
                    )
                    OR (
                        correlation_name = 'unsupported'
                        AND marker_value IS NULL
                    )
                )
            )
            OR (
                action_name IN ('register_instance', 'teardown_instance')
                AND (
                    correlation_name <> 'internal_idempotency_key'
                    OR COALESCE(marker_value, '') !~ '^[0-9a-f]{64}$'
                )
            )
            OR (
                action_name = 'edit_response'
                AND (
                    correlation_name <> 'interaction_receipt'
                    OR marker_value IS NOT NULL
                )
            )
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.jsonb_array_elements_text(
                    action_document->'dependency_indices'
                ) AS dependency(value)
                WHERE dependency.value IS NULL
                    OR dependency.value !~ '^(0|[1-9][0-9]{0,2})$'
                    OR dependency.value::INTEGER > 255
                    OR dependency.value::INTEGER >= action_number
            )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI003',
                MESSAGE = 'runtime_interaction_effect_plan_action_contract_invalid';
        END IF;

        SELECT pg_catalog.count(*)
        INTO object_key_count
        FROM pg_catalog.jsonb_object_keys(planned_recovery_input_value);

        expected_reference_slots := CASE action_name
            WHEN 'create_role' THEN ARRAY['guild_id']::TEXT[]
            WHEN 'create_channel' THEN ARRAY['guild_id']::TEXT[]
            WHEN 'grant_role' THEN ARRAY[
                'guild_id',
                'role_id',
                'user_id'
            ]::TEXT[]
            WHEN 'upsert_overwrite' THEN ARRAY[
                'channel_id',
                'guild_id',
                'target_id'
            ]::TEXT[]
            WHEN 'post_panel' THEN ARRAY[
                'channel_id',
                'guild_id'
            ]::TEXT[]
            WHEN 'register_instance' THEN ARRAY['guild_id']::TEXT[]
            WHEN 'teardown_instance' THEN ARRAY['guild_id']::TEXT[]
            ELSE ARRAY[]::TEXT[]
        END;

        IF NOT planned_recovery_input_value ? 'references'
            OR pg_catalog.jsonb_typeof(
                planned_recovery_input_value->'references'
            ) <> 'array'
            OR (
                action_name = 'upsert_overwrite'
                AND (
                    object_key_count <> 4
                    OR NOT planned_recovery_input_value ?& ARRAY[
                        'target_kind',
                        'permission_allow',
                        'permission_deny'
                    ]
                    OR COALESCE(
                        planned_recovery_input_value->>'target_kind',
                        ''
                    ) NOT IN ('role', 'member')
                    OR COALESCE(
                        planned_recovery_input_value->>'permission_allow',
                        ''
                    ) !~ '^(0|[1-9][0-9]{0,19})$'
                    OR pg_catalog.length(
                        planned_recovery_input_value->>'permission_allow'
                    ) > 20
                    OR (
                        pg_catalog.length(
                            planned_recovery_input_value->>'permission_allow'
                        ) = 20
                        AND planned_recovery_input_value->>'permission_allow'
                            > '18446744073709551615'
                    )
                    OR COALESCE(
                        planned_recovery_input_value->>'permission_deny',
                        ''
                    ) !~ '^(0|[1-9][0-9]{0,19})$'
                    OR pg_catalog.length(
                        planned_recovery_input_value->>'permission_deny'
                    ) > 20
                    OR (
                        pg_catalog.length(
                            planned_recovery_input_value->>'permission_deny'
                        ) = 20
                        AND planned_recovery_input_value->>'permission_deny'
                            > '18446744073709551615'
                    )
                )
            )
            OR (
                action_name = 'register_instance'
                AND (
                    object_key_count <> 4
                    OR NOT planned_recovery_input_value ?& ARRAY[
                        'instance_id',
                        'instance_kind',
                        'manifest_digest'
                    ]
                    OR COALESCE(
                        planned_recovery_input_value->>'instance_id',
                        ''
                    ) !~ '^[A-Za-z0-9_-]{1,32}$'
                    OR COALESCE(
                        planned_recovery_input_value->>'instance_kind',
                        ''
                    ) !~ '^[A-Za-z0-9_-]{1,64}$'
                    OR COALESCE(
                        planned_recovery_input_value->>'manifest_digest',
                        ''
                    ) !~ '^[0-9a-f]{64}$'
                )
            )
            OR (
                action_name = 'teardown_instance'
                AND (
                    object_key_count <> 2
                    OR NOT planned_recovery_input_value ? 'instance_id'
                    OR COALESCE(
                        planned_recovery_input_value->>'instance_id',
                        ''
                    ) !~ '^[A-Za-z0-9_-]{1,32}$'
                )
            )
            OR (
                action_name IN ('post_panel', 'edit_response')
                AND (
                    object_key_count <> 2
                    OR NOT planned_recovery_input_value ? 'payload_digest'
                    OR COALESCE(
                        planned_recovery_input_value->>'payload_digest',
                        ''
                    ) !~ '^[0-9a-f]{64}$'
                )
            )
            OR (
                action_name NOT IN (
                    'upsert_overwrite',
                    'register_instance',
                    'teardown_instance',
                    'post_panel',
                    'edit_response'
                )
                AND object_key_count <> 1
            )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI003',
                MESSAGE = 'runtime_interaction_effect_planned_input_invalid';
        END IF;

        SELECT COALESCE(
            pg_catalog.array_agg(
                reference.value->>'slot'
                ORDER BY reference.ordinality
            ),
            ARRAY[]::TEXT[]
        )
        INTO observed_reference_slots
        FROM pg_catalog.jsonb_array_elements(
            planned_recovery_input_value->'references'
        ) WITH ORDINALITY AS reference(value, ordinality);

        IF observed_reference_slots IS DISTINCT FROM expected_reference_slots
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI003',
                MESSAGE = 'runtime_interaction_effect_planned_reference_order_invalid';
        END IF;

        FOR reference_entry IN
            SELECT reference.value
            FROM pg_catalog.jsonb_array_elements(
                planned_recovery_input_value->'references'
            ) AS reference(value)
        LOOP
            IF pg_catalog.jsonb_typeof(reference_entry.value) <> 'object'
            THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RI003',
                    MESSAGE = 'runtime_interaction_effect_planned_reference_invalid';
            END IF;

            SELECT pg_catalog.count(*)
            INTO object_key_count
            FROM pg_catalog.jsonb_object_keys(reference_entry.value);

            IF COALESCE(reference_entry.value->>'slot', '')
                    NOT IN (
                        'guild_id',
                        'channel_id',
                        'role_id',
                        'user_id',
                        'target_id'
                    )
                OR COALESCE(reference_entry.value->>'source', '')
                    NOT IN ('existing', 'action_output')
                OR (
                    reference_entry.value->>'source' = 'existing'
                    AND (
                        object_key_count <> 3
                        OR NOT reference_entry.value ?& ARRAY[
                            'slot',
                            'source',
                            'id'
                        ]
                        OR COALESCE(reference_entry.value->>'id', '')
                            !~ '^[1-9][0-9]{0,19}$'
                        OR pg_catalog.length(
                            reference_entry.value->>'id'
                        ) > 20
                        OR (
                            pg_catalog.length(
                                reference_entry.value->>'id'
                            ) = 20
                            AND reference_entry.value->>'id'
                                > '18446744073709551615'
                        )
                    )
                )
                OR (
                    reference_entry.value->>'source' = 'action_output'
                    AND (
                        object_key_count <> 5
                        OR NOT reference_entry.value ?& ARRAY[
                            'slot',
                            'source',
                            'action_index',
                            'output_kind',
                            'producer_identity_digest'
                        ]
                        OR COALESCE(
                            reference_entry.value->>'action_index',
                            ''
                        ) !~ '^(0|[1-9][0-9]{0,2})$'
                        OR (reference_entry.value->>'action_index')::INTEGER
                            >= action_number
                        OR NOT EXISTS (
                            SELECT 1
                            FROM pg_catalog.jsonb_array_elements_text(
                                action_document->'dependency_indices'
                            ) AS dependency(value)
                            WHERE dependency.value =
                                reference_entry.value->>'action_index'
                        )
                        OR COALESCE(
                            reference_entry.value->>'output_kind',
                            ''
                        ) NOT IN (
                            'created_role',
                            'created_channel',
                            'posted_message',
                            'instance_state'
                        )
                        OR (
                            proposed_actions->(
                                reference_entry.value->>'action_index'
                            )::INTEGER->>'output_kind'
                        ) IS DISTINCT FROM
                            reference_entry.value->>'output_kind'
                        OR COALESCE(
                            reference_entry.value
                                ->>'producer_identity_digest',
                            ''
                        ) !~ '^[0-9a-f]{64}$'
                        OR (
                            proposed_actions->(
                                reference_entry.value->>'action_index'
                            )::INTEGER->>'planned_identity_digest'
                        ) IS DISTINCT FROM
                            reference_entry.value
                                ->>'producer_identity_digest'
                        OR (
                            reference_entry.value->>'slot' = 'role_id'
                            AND reference_entry.value->>'output_kind'
                                <> 'created_role'
                        )
                        OR (
                            reference_entry.value->>'slot' = 'channel_id'
                            AND reference_entry.value->>'output_kind'
                                <> 'created_channel'
                        )
                        OR (
                            reference_entry.value->>'slot' = 'target_id'
                            AND reference_entry.value->>'output_kind'
                                <> 'created_role'
                        )
                        OR reference_entry.value->>'slot' IN (
                            'guild_id',
                            'user_id'
                        )
                    )
                )
            THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RI003',
                    MESSAGE = 'runtime_interaction_effect_planned_reference_invalid';
            END IF;
        END LOOP;

        SELECT pg_catalog.count(*)
        INTO object_key_count
        FROM pg_catalog.jsonb_object_keys(planned_preimage_document);

        IF (
                action_name IN (
                    'create_role',
                    'create_channel',
                    'post_panel',
                    'edit_response'
                )
                AND (
                    object_key_count <> 1
                    OR planned_preimage_document->>'kind' <> 'none'
                )
            )
            OR (
                action_name = 'grant_role'
                AND (
                    object_key_count <> 3
                    OR NOT planned_preimage_document ?& ARRAY[
                        'kind',
                        'references',
                        'present'
                    ]
                    OR planned_preimage_document->>'kind'
                        <> 'role_membership'
                    OR planned_preimage_document->'references'
                        IS DISTINCT FROM
                        planned_recovery_input_value->'references'
                    OR pg_catalog.jsonb_typeof(
                        planned_preimage_document->'present'
                    ) <> 'boolean'
                )
            )
            OR (
                action_name = 'upsert_overwrite'
                AND (
                    NOT planned_preimage_document ?& ARRAY[
                        'kind',
                        'references',
                        'target_kind',
                        'state'
                    ]
                    OR planned_preimage_document->>'kind'
                        <> 'permission_overwrite'
                    OR planned_preimage_document->'references'
                        IS DISTINCT FROM
                        planned_recovery_input_value->'references'
                    OR planned_preimage_document->>'target_kind'
                        IS DISTINCT FROM
                        planned_recovery_input_value->>'target_kind'
                    OR COALESCE(planned_preimage_document->>'state', '')
                        NOT IN ('absent', 'present')
                    OR (
                        planned_preimage_document->>'state' = 'absent'
                        AND object_key_count <> 4
                    )
                    OR (
                        planned_preimage_document->>'state' = 'present'
                        AND (
                            object_key_count <> 6
                            OR NOT planned_preimage_document ?& ARRAY[
                                'permission_allow',
                                'permission_deny'
                            ]
                            OR COALESCE(
                                planned_preimage_document
                                    ->>'permission_allow',
                                ''
                            ) !~ '^(0|[1-9][0-9]{0,19})$'
                            OR pg_catalog.length(
                                planned_preimage_document
                                    ->>'permission_allow'
                            ) > 20
                            OR (
                                pg_catalog.length(
                                    planned_preimage_document
                                        ->>'permission_allow'
                                ) = 20
                                AND planned_preimage_document
                                    ->>'permission_allow'
                                    > '18446744073709551615'
                            )
                            OR COALESCE(
                                planned_preimage_document
                                    ->>'permission_deny',
                                ''
                            ) !~ '^(0|[1-9][0-9]{0,19})$'
                            OR pg_catalog.length(
                                planned_preimage_document
                                    ->>'permission_deny'
                            ) > 20
                            OR (
                                pg_catalog.length(
                                    planned_preimage_document
                                        ->>'permission_deny'
                                ) = 20
                                AND planned_preimage_document
                                    ->>'permission_deny'
                                    > '18446744073709551615'
                            )
                        )
                    )
                )
            )
            OR (
                action_name IN ('register_instance', 'teardown_instance')
                AND (
                    NOT planned_preimage_document ?& ARRAY[
                        'kind',
                        'references',
                        'instance_id',
                        'state'
                    ]
                    OR planned_preimage_document->>'kind'
                        <> 'instance_registration'
                    OR planned_preimage_document->'references'
                        IS DISTINCT FROM
                        planned_recovery_input_value->'references'
                    OR planned_preimage_document->>'instance_id'
                        IS DISTINCT FROM
                        planned_recovery_input_value->>'instance_id'
                    OR COALESCE(planned_preimage_document->>'state', '')
                        NOT IN ('absent', 'present')
                    OR (
                        planned_preimage_document->>'state' = 'absent'
                        AND object_key_count <> 4
                    )
                    OR (
                        planned_preimage_document->>'state' = 'present'
                        AND (
                            object_key_count <> 5
                            OR COALESCE(
                                planned_preimage_document
                                    ->>'manifest_digest',
                                ''
                            ) !~ '^[0-9a-f]{64}$'
                        )
                    )
                )
            )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI003',
                MESSAGE = 'runtime_interaction_effect_planned_preimage_invalid';
        END IF;

        SELECT COALESCE(
            pg_catalog.array_agg(
                dependency.value::SMALLINT
                ORDER BY dependency.ordinality
            ),
            ARRAY[]::SMALLINT[]
        )
        INTO dependency_values
        FROM pg_catalog.jsonb_array_elements_text(
            action_document->'dependency_indices'
        ) WITH ORDINALITY AS dependency(value, ordinality);

        IF dependency_values IS DISTINCT FROM ARRAY(
            SELECT DISTINCT dependency_value
            FROM pg_catalog.unnest(dependency_values) AS dependency_value
            ORDER BY dependency_value
        )
            OR (
                action_number > 0
                AND NOT (action_number - 1 = ANY(dependency_values))
            )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI003',
                MESSAGE = 'runtime_interaction_effect_dependency_order_invalid';
        END IF;

        SELECT pg_catalog.count(*)
        INTO matching_count
        FROM public.runtime_interaction_effect_heads_v1 AS head
        WHERE head.application_id = expected_application_id
            AND head.interaction_id = expected_interaction_id
            AND head.action_index = action_number
            AND head.action_kind = action_name
            AND head.dependency_indices = dependency_values
            AND head.planned_identity_digest = planned_identity_value
            AND head.input_digest = input_value
            AND head.expected_postimage_digest = expected_value
            AND head.planned_recovery_input = planned_recovery_input_value
            AND head.planned_preimage_digest = preimage_value
            AND head.planned_preimage = planned_preimage_document
            AND head.output_kind = output_name
            AND head.correlation_class = correlation_name
            AND head.correlation_digest = correlation_value
            AND head.correlation_marker IS NOT DISTINCT FROM marker_value;

        SELECT root.*
        INTO effect_root
        FROM public.runtime_interaction_effect_roots_v1 AS root
        WHERE root.application_id = expected_application_id
            AND root.interaction_id = expected_interaction_id
        FOR KEY SHARE;

        IF FOUND AND matching_count <> 1 THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI002',
                MESSAGE = 'runtime_interaction_effect_plan_corruption';
        END IF;
    END LOOP;

    SELECT root.*
    INTO effect_root
    FROM public.runtime_interaction_effect_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    IF FOUND THEN
        SELECT pg_catalog.count(*)
        INTO matching_count
        FROM public.runtime_interaction_effect_heads_v1 AS head
        WHERE head.application_id = expected_application_id
            AND head.interaction_id = expected_interaction_id;

        IF effect_root.record_format_version <> 1
            OR effect_root.action_plan_digest
                IS DISTINCT FROM expected_action_plan_digest
            OR effect_root.preflight_certificate_digest
                IS DISTINCT FROM proposed_preflight_certificate_digest
            OR effect_root.snapshot_digest
                IS DISTINCT FROM proposed_snapshot_digest
            OR effect_root.action_count <> action_total
            OR matching_count <> action_total
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI002',
                MESSAGE = 'runtime_interaction_effect_plan_corruption';
        END IF;

        outcome_name := 'exact_replay';
        resulting_action_count := effect_root.action_count;
        resulting_certificate_issued_at := effect_root.certificate_issued_at;
        resulting_certificate_expires_at := effect_root.certificate_expires_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    INSERT INTO public.runtime_interaction_effect_roots_v1 (
        application_id,
        interaction_id,
        record_format_version,
        action_plan_digest,
        preflight_certificate_digest,
        snapshot_digest,
        action_count,
        certificate_issued_at,
        certificate_expires_at,
        created_at
    ) VALUES (
        expected_application_id,
        expected_interaction_id,
        1,
        expected_action_plan_digest,
        proposed_preflight_certificate_digest,
        proposed_snapshot_digest,
        action_total,
        database_now,
        database_now + INTERVAL '5 minutes',
        database_now
    );

    FOR action_entry IN
        SELECT element.value, element.ordinality
        FROM pg_catalog.jsonb_array_elements(proposed_actions)
            WITH ORDINALITY AS element(value, ordinality)
        ORDER BY element.ordinality
    LOOP
        action_document := action_entry.value;
        action_number := (action_document->>'action_index')::SMALLINT;
        action_name := action_document->>'action_kind';
        output_name := action_document->>'output_kind';
        correlation_name := action_document->>'correlation_class';
        correlation_value := pg_catalog.decode(
            action_document->>'correlation_digest',
            'hex'
        );
        marker_value := action_document->>'correlation_marker';
        input_value := pg_catalog.decode(
            action_document->>'input_digest',
            'hex'
        );
        planned_identity_value := pg_catalog.decode(
            action_document->>'planned_identity_digest',
            'hex'
        );
        expected_value := pg_catalog.decode(
            action_document->>'expected_postimage_digest',
            'hex'
        );
        planned_recovery_input_value :=
            action_document->'planned_recovery_input';
        preimage_value := pg_catalog.decode(
            action_document->>'planned_preimage_digest',
            'hex'
        );
        planned_preimage_document := action_document->'planned_preimage';

        SELECT COALESCE(
            pg_catalog.array_agg(
                dependency.value::SMALLINT
                ORDER BY dependency.ordinality
            ),
            ARRAY[]::SMALLINT[]
        )
        INTO dependency_values
        FROM pg_catalog.jsonb_array_elements_text(
            action_document->'dependency_indices'
        ) WITH ORDINALITY AS dependency(value, ordinality);

        INSERT INTO public.runtime_interaction_effect_heads_v1 (
            application_id,
            interaction_id,
            action_index,
            action_kind,
            dependency_indices,
            planned_identity_digest,
            input_digest,
            expected_postimage_digest,
            planned_recovery_input,
            planned_preimage_digest,
            planned_preimage,
            output_kind,
            correlation_class,
            correlation_digest,
            correlation_marker,
            state,
            head_revision,
            attempt_count,
            observation_attempt_count,
            compensation_attempt_count,
            compensation_observation_attempt_count,
            recovery_claim_revision,
            updated_at
        ) VALUES (
            expected_application_id,
            expected_interaction_id,
            action_number,
            action_name,
            dependency_values,
            planned_identity_value,
            input_value,
            expected_value,
            planned_recovery_input_value,
            preimage_value,
            planned_preimage_document,
            output_name,
            correlation_name,
            correlation_value,
            marker_value,
            'planned',
            1,
            0,
            0,
            0,
            0,
            0,
            database_now
        );

        INSERT INTO public.runtime_interaction_effect_events_v1 (
            application_id,
            interaction_id,
            action_index,
            event_revision,
            event_kind,
            from_state,
            to_state,
            receipt_claim_revision,
            recovery_claim_revision,
            process_instance_id,
            outcome_code,
            result_digest,
            output_kind,
            output_id,
            event_digest,
            observed_at
        ) VALUES (
            expected_application_id,
            expected_interaction_id,
            action_number,
            1,
            'planned',
            NULL,
            'planned',
            expected_receipt_claim_revision,
            0,
            expected_process_instance_id,
            'planned',
            input_value,
            output_name,
            NULL,
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.concat_ws(
                    '|',
                    'starring-runtime-interaction-effect-event-v1',
                    expected_application_id,
                    expected_interaction_id,
                    action_number::TEXT,
                    '1',
                    'planned',
                    action_name,
                    pg_catalog.encode(planned_identity_value, 'hex'),
                    pg_catalog.encode(input_value, 'hex'),
                    pg_catalog.encode(expected_value, 'hex'),
                    output_name,
                    correlation_name,
                    pg_catalog.encode(correlation_value, 'hex'),
                    COALESCE(marker_value, '')
                ),
                'UTF8'
            )),
            database_now
        );
    END LOOP;

    outcome_name := 'plan_bound';
    resulting_action_count := action_total::SMALLINT;
    resulting_certificate_issued_at := database_now;
    resulting_certificate_expires_at := database_now + INTERVAL '5 minutes';
    observed_database_now := database_now;
    RETURN NEXT;
END;
$function$;

DO $receipt_effect_advisory_lock_extension$
DECLARE
    function_identity TEXT;
    function_definition TEXT;
    lock_marker TEXT;
    insertion_marker TEXT;
    insertion_offset INTEGER;
BEGIN
    lock_marker := 'starring-runtime-interaction-receipt-v1:';
    insertion_marker := E'    END IF;\n\n';
    FOREACH function_identity IN ARRAY ARRAY[
        'public.starring_runtime_interaction_receipt_claim_v1(text,text,text,text,text,text,text,text,text,text,bigint,text,bigint,text,bigint,bigint,bigint,text,text,text,text,text,text,text,bigint,bigint,bigint,bigint,bigint,text,text,bytea,bigint,text,smallint,text,bytea,bytea,bytea,timestamp with time zone,timestamp with time zone)',
        'public.starring_runtime_interaction_receipt_plan_bind_v1(text,text,bigint,bigint,text,bytea)',
        'public.starring_runtime_interaction_receipt_acknowledgement_intend_v1(text,text,bigint,bigint,text,text,bytea)',
        'public.starring_runtime_interaction_receipt_acknowledgement_finish_v1(text,text,bigint,bigint,text,bytea,text,bytea)',
        'public.starring_runtime_interaction_receipt_execution_intend_v1(text,text,bigint,bigint,text,bytea)',
        'public.starring_runtime_interaction_receipt_finish_v1(text,text,bigint,bigint,text,bytea,text,text,bytea)',
        'public.starring_runtime_interaction_receipt_recover_v1(text,text,bigint,bigint,text,bigint,bigint,bigint,text,text,text,bytea,bigint)',
        'public.starring_runtime_interaction_receipt_token_expire_v1(text,text,bigint,bigint,bytea)',
        'public.starring_runtime_interaction_receipt_terminalize_expired_v1(text,text,bigint,bigint,text,text,bytea)',
        'public.starring_runtime_interaction_effect_plan_bind_v1(text,text,bigint,bigint,text,bytea,bytea,bytea,jsonb)',
        'public.starring_runtime_interaction_effect_intend_v1(text,text,bigint,bigint,text,bytea,bigint,bigint,bytea,bytea,bytea,jsonb,bytea,jsonb,bigint)',
        'public.starring_runtime_interaction_effect_finish_v1(text,text,bigint,bigint,text,bytea,bigint,bigint,bytea,text,text)',
        'public.starring_runtime_interaction_effect_recovery_claim_v1(text,text,bigint,bigint,text,text,text,bigint,bigint,bigint,bigint)',
        'public.starring_runtime_interaction_effect_reconcile_v1(text,text,bigint,bigint,bigint,text,text,text,bigint,bigint,bigint,text,text,bytea,text,bytea,text,bigint)',
        'public.starring_runtime_interaction_effect_compensation_intend_v1(text,text,bigint,bigint,text,text,text,bigint,bigint,bigint,bytea,bytea,bigint)',
        'public.starring_runtime_interaction_effect_compensation_finish_v1(text,text,bigint,bigint,bigint,text,bytea,text,bytea,bigint)',
        'public.starring_runtime_interaction_effect_response_tail_claim_v1(text,text,bigint,bigint,text,text,text,bigint,bigint,bigint,bytea,bytea,bytea,bigint)',
        'public.starring_runtime_interaction_effect_response_tail_finalize_v1(text,text,bigint,bigint,text,bigint,bigint,text,text,text,bigint,bigint,bigint,bytea,bytea,text,bytea,bytea,bigint)'
    ]
    LOOP
        function_definition := pg_catalog.pg_get_functiondef(
            pg_catalog.to_regprocedure(function_identity)
        );
        IF function_definition IS NULL THEN
            RAISE EXCEPTION 'runtime interaction mutation function unavailable'
                USING ERRCODE = '55000';
        END IF;
        IF pg_catalog.strpos(function_definition, lock_marker) <> 0 THEN
            CONTINUE;
        END IF;
        insertion_offset := pg_catalog.strpos(
            function_definition,
            insertion_marker
        );
        IF insertion_offset = 0 THEN
            RAISE EXCEPTION 'runtime interaction mutation lock insertion failed'
                USING ERRCODE = '55000';
        END IF;
        insertion_offset := insertion_offset
            + pg_catalog.length(insertion_marker);
        function_definition := pg_catalog.substr(
            function_definition,
            1,
            insertion_offset - 1
        ) || $lock$    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            'starring-runtime-interaction-receipt-v1:'
                || expected_application_id
                || ':'
                || expected_interaction_id,
            0
        )
    );

$lock$ || pg_catalog.substr(function_definition, insertion_offset);
        EXECUTE function_definition;
    END LOOP;
END;
$receipt_effect_advisory_lock_extension$;

DO $interaction_readiness_extension$
DECLARE
    function_definition TEXT;
    relation_contract TEXT;
    relation_replacement TEXT;
    capability_contract TEXT;
    capability_replacement TEXT;
    support_contract TEXT;
    support_replacement TEXT;
    trigger_contract TEXT;
    trigger_replacement TEXT;
    allowlist_contract TEXT;
    allowlist_replacement TEXT;
BEGIN
    function_definition := pg_catalog.pg_get_functiondef(
        pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_database_readiness_v1()'
        )
    );
    relation_contract := $needle$            ('public.runtime_interaction_receipt_token_secrets_v1')
    ) AS expected(identity)$needle$;
    relation_replacement := $needle$            ('public.runtime_interaction_receipt_token_secrets_v1'),
            ('public.runtime_interaction_effect_roots_v1'),
            ('public.runtime_interaction_effect_rollbacks_v1'),
            ('public.runtime_interaction_effect_heads_v1'),
            ('public.runtime_interaction_effect_events_v1')
    ) AS expected(identity)$needle$;
    capability_contract := $needle$            (
                'public.starring_runtime_interaction_receipt_terminalize_expired_v1(text,text,bigint,bigint,text,text,bytea)',
                'expected_application_id text, expected_interaction_id text, expected_head_revision bigint, expected_claim_revision bigint, expected_process_instance_id text, expected_runtime_build_revision text, proposed_observation_digest bytea',
                'TABLE(outcome_name text, receipt_state text, resulting_head_revision bigint, resulting_claim_revision bigint, resulting_claim_expires_at timestamp with time zone, observed_database_now timestamp with time zone)',
                TRUE,
                1::REAL,
                'plpgsql'
            )$needle$;
    capability_replacement := capability_contract || $extension$,
            (
                'public.starring_runtime_interaction_effect_plan_bind_v1(text,text,bigint,bigint,text,bytea,bytea,bytea,jsonb)',
                'expected_application_id text, expected_interaction_id text, expected_receipt_head_revision bigint, expected_receipt_claim_revision bigint, expected_process_instance_id text, expected_action_plan_digest bytea, proposed_preflight_certificate_digest bytea, proposed_snapshot_digest bytea, proposed_actions jsonb',
                'TABLE(outcome_name text, resulting_action_count smallint, resulting_certificate_issued_at timestamp with time zone, resulting_certificate_expires_at timestamp with time zone, observed_database_now timestamp with time zone)',
                TRUE,
                1::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_effect_intend_v1(text,text,bigint,bigint,text,bytea,bigint,bigint,bytea,bytea,bytea,jsonb,bytea,jsonb,bigint)',
                'expected_application_id text, expected_interaction_id text, expected_receipt_head_revision bigint, expected_receipt_claim_revision bigint, expected_process_instance_id text, expected_preflight_certificate_digest bytea, expected_action_index bigint, expected_effect_head_revision bigint, proposed_intent_digest bytea, proposed_resolved_effect_identity_digest bytea, proposed_resolved_instance_manifest_digest bytea, proposed_resolved_input jsonb, proposed_resolved_preimage_digest bytea, proposed_resolved_preimage jsonb, requested_recovery_delay_milliseconds bigint',
                'TABLE(outcome_name text, effect_state text, resulting_effect_head_revision bigint, resulting_recovery_at timestamp with time zone, observed_database_now timestamp with time zone)',
                TRUE,
                1::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_effect_finish_v1(text,text,bigint,bigint,text,bytea,bigint,bigint,bytea,text,text)',
                'expected_application_id text, expected_interaction_id text, expected_receipt_head_revision bigint, expected_receipt_claim_revision bigint, expected_process_instance_id text, expected_preflight_certificate_digest bytea, expected_action_index bigint, expected_effect_head_revision bigint, proposed_result_digest bytea, proposed_outcome text, proposed_output_id text',
                'TABLE(outcome_name text, effect_state text, resulting_effect_head_revision bigint, resulting_recovery_at timestamp with time zone, observed_database_now timestamp with time zone)',
                TRUE,
                1::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_effect_scan_recoverable_v1(timestamp with time zone,text,text,bigint,timestamp with time zone,text,text,bigint,bigint)',
                'expected_after_recovery_at timestamp with time zone, expected_after_application_id text, expected_after_interaction_id text, expected_after_action_index bigint, expected_through_recovery_at timestamp with time zone, expected_through_application_id text, expected_through_interaction_id text, expected_through_action_index bigint, expected_limit bigint',
                'TABLE(application_id text, interaction_id text, action_index smallint, action_kind text, effect_state text, effect_head_revision bigint, recovery_claim_revision bigint, attempt_count integer, observation_attempt_count integer, compensation_attempt_count integer, compensation_observation_attempt_count integer, dependency_indices smallint[], planned_identity_digest bytea, input_digest bytea, expected_postimage_digest bytea, planned_recovery_input jsonb, planned_preimage_digest bytea, planned_preimage jsonb, resolved_input jsonb, resolved_preimage_digest bytea, resolved_preimage jsonb, resolved_effect_identity_digest bytea, resolved_instance_manifest_digest bytea, output_kind text, output_id text, correlation_class text, correlation_digest bytea, correlation_marker text, intent_digest bytea, result_digest bytea, success_binding_kind text, success_binding_digest bytea, compensation_intent_digest bytea, compensation_result_digest bytea, next_recovery_at timestamp with time zone, action_plan_digest bytea, preflight_certificate_digest bytea, snapshot_digest bytea, certificate_issued_at timestamp with time zone, certificate_expires_at timestamp with time zone, tenant_id text, installation_id text, deployment_id text, attestation_id text, attestation_digest text, guild_id text, channel_id text, actor_user_id text, interaction_kind text, ruleset_key text, target_version bigint, target_content_hash text, binding_revision bigint, binding_fingerprint text, runtime_generation bigint, route_controller_fencing_token bigint, route_incarnation bigint, origin_process_instance_id text, origin_serving_lease_epoch bigint, origin_serving_revision bigint, origin_gateway_shard_id text, origin_gateway_owner_lease_epoch bigint, origin_gateway_owner_revision bigint, runtime_build_revision text, route_kind text, route_key text, instance_id text, execution_ruleset_version bigint, execution_ruleset_content_hash text, instance_manifest_digest text, request_digest bytea, through_recovery_at timestamp with time zone, through_application_id text, through_interaction_id text, through_action_index smallint, observed_database_now timestamp with time zone)',
                TRUE,
                256::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_effect_recovery_claim_v1(text,text,bigint,bigint,text,text,text,bigint,bigint,bigint,bigint)',
                'expected_application_id text, expected_interaction_id text, expected_action_index bigint, expected_effect_head_revision bigint, expected_process_instance_id text, expected_gateway_shard_id text, expected_runtime_build_revision text, expected_runtime_generation bigint, expected_controller_fencing_token bigint, expected_route_incarnation bigint, requested_claim_lease_milliseconds bigint',
                'TABLE(outcome_name text, effect_state text, resulting_effect_head_revision bigint, resulting_recovery_claim_revision bigint, resulting_recovery_claim_expires_at timestamp with time zone, observed_database_now timestamp with time zone)',
                TRUE,
                1::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_effect_reconcile_v1(text,text,bigint,bigint,bigint,text,text,text,bigint,bigint,bigint,text,text,bytea,text,bytea,text,bigint)',
                'expected_application_id text, expected_interaction_id text, expected_action_index bigint, expected_effect_head_revision bigint, expected_recovery_claim_revision bigint, expected_process_instance_id text, expected_gateway_shard_id text, expected_runtime_build_revision text, expected_runtime_generation bigint, expected_controller_fencing_token bigint, expected_route_incarnation bigint, expected_source_effect_state text, expected_recovery_path text, expected_preflight_certificate_digest bytea, proposed_observation_outcome text, proposed_observation_digest bytea, proposed_output_id text, requested_retry_delay_milliseconds bigint',
                'TABLE(outcome_name text, effect_state text, resulting_effect_head_revision bigint, resulting_recovery_at timestamp with time zone, observed_database_now timestamp with time zone)',
                TRUE,
                1::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_effect_compensation_intend_v1(text,text,bigint,bigint,text,text,text,bigint,bigint,bigint,bytea,bytea,bigint)',
                'expected_application_id text, expected_interaction_id text, expected_action_index bigint, expected_effect_head_revision bigint, expected_process_instance_id text, expected_gateway_shard_id text, expected_runtime_build_revision text, expected_runtime_generation bigint, expected_controller_fencing_token bigint, expected_route_incarnation bigint, expected_preflight_certificate_digest bytea, proposed_compensation_intent_digest bytea, requested_recovery_delay_milliseconds bigint',
                'TABLE(outcome_name text, effect_state text, resulting_effect_head_revision bigint, resulting_recovery_claim_revision bigint, resulting_recovery_at timestamp with time zone, observed_database_now timestamp with time zone)',
                TRUE,
                1::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_effect_compensation_finish_v1(text,text,bigint,bigint,bigint,text,bytea,text,bytea,bigint)',
                'expected_application_id text, expected_interaction_id text, expected_action_index bigint, expected_effect_head_revision bigint, expected_recovery_claim_revision bigint, expected_process_instance_id text, expected_preflight_certificate_digest bytea, proposed_compensation_outcome text, proposed_compensation_result_digest bytea, requested_retry_delay_milliseconds bigint',
                'TABLE(outcome_name text, effect_state text, resulting_effect_head_revision bigint, resulting_recovery_at timestamp with time zone, observed_database_now timestamp with time zone)',
                TRUE,
                1::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_effect_response_tail_scan_v1(timestamp with time zone,text,text,bigint,timestamp with time zone,text,text,bigint,bigint)',
                'expected_after_recovery_at timestamp with time zone, expected_after_application_id text, expected_after_interaction_id text, expected_after_action_index bigint, expected_through_recovery_at timestamp with time zone, expected_through_application_id text, expected_through_interaction_id text, expected_through_action_index bigint, expected_limit bigint',
                'TABLE(application_id text, interaction_id text, action_index smallint, effect_state text, effect_head_revision bigint, recovery_claim_revision bigint, observation_attempt_count integer, planned_identity_digest bytea, input_digest bytea, expected_postimage_digest bytea, planned_recovery_input jsonb, planned_preimage_digest bytea, planned_preimage jsonb, resolved_input jsonb, resolved_preimage_digest bytea, resolved_preimage jsonb, resolved_effect_identity_digest bytea, intent_digest bytea, result_digest bytea, success_binding_kind text, success_binding_digest bytea, correlation_digest bytea, action_plan_digest bytea, preflight_certificate_digest bytea, snapshot_digest bytea, receipt_state text, receipt_head_revision bigint, receipt_claim_revision bigint, receipt_claim_expires_at timestamp with time zone, token_expires_at timestamp with time zone, tenant_id text, installation_id text, deployment_id text, attestation_id text, attestation_digest text, guild_id text, channel_id text, actor_user_id text, interaction_kind text, ruleset_key text, target_version bigint, target_content_hash text, binding_revision bigint, binding_fingerprint text, runtime_generation bigint, route_controller_fencing_token bigint, route_incarnation bigint, origin_process_instance_id text, origin_serving_lease_epoch bigint, origin_serving_revision bigint, origin_gateway_shard_id text, origin_gateway_owner_lease_epoch bigint, origin_gateway_owner_revision bigint, runtime_build_revision text, route_kind text, route_key text, instance_id text, execution_ruleset_version bigint, execution_ruleset_content_hash text, instance_manifest_digest text, request_digest bytea, next_recovery_at timestamp with time zone, through_recovery_at timestamp with time zone, through_application_id text, through_interaction_id text, through_action_index smallint, observed_database_now timestamp with time zone)',
                TRUE,
                256::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_effect_response_tail_claim_v1(text,text,bigint,bigint,text,text,text,bigint,bigint,bigint,bytea,bytea,bytea,bigint)',
                'expected_application_id text, expected_interaction_id text, expected_action_index bigint, expected_effect_head_revision bigint, expected_process_instance_id text, expected_gateway_shard_id text, expected_runtime_build_revision text, expected_runtime_generation bigint, expected_controller_fencing_token bigint, expected_route_incarnation bigint, expected_preflight_certificate_digest bytea, expected_postimage_digest bytea, proposed_unrecoverable_digest bytea, requested_claim_lease_milliseconds bigint',
                'TABLE(outcome_name text, effect_state text, resulting_effect_head_revision bigint, resulting_recovery_claim_revision bigint, resulting_observation_attempt_count integer, resulting_recovery_claim_expires_at timestamp with time zone, receipt_state text, resulting_receipt_head_revision bigint, token_encryption_suite text, token_suite_version smallint, token_key_id text, token_nonce bytea, token_ciphertext bytea, token_aad_digest bytea, token_issued_at timestamp with time zone, token_expires_at timestamp with time zone, observed_database_now timestamp with time zone)',
                TRUE,
                1::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_effect_response_tail_finalize_v1(text,text,bigint,bigint,text,bigint,bigint,text,text,text,bigint,bigint,bigint,bytea,bytea,text,bytea,bytea,bigint)',
                'expected_application_id text, expected_interaction_id text, expected_action_index bigint, expected_receipt_head_revision bigint, expected_receipt_state text, expected_effect_head_revision bigint, expected_recovery_claim_revision bigint, expected_process_instance_id text, expected_gateway_shard_id text, expected_runtime_build_revision text, expected_runtime_generation bigint, expected_controller_fencing_token bigint, expected_route_incarnation bigint, expected_preflight_certificate_digest bytea, expected_postimage_digest bytea, proposed_observation_outcome text, proposed_observation_digest bytea, proposed_terminal_result_digest bytea, requested_retry_delay_milliseconds bigint',
                'TABLE(outcome_name text, effect_state text, resulting_effect_head_revision bigint, receipt_state text, resulting_receipt_head_revision bigint, resulting_recovery_at timestamp with time zone, observed_database_now timestamp with time zone)',
                TRUE,
                1::REAL,
                'plpgsql'
            )$extension$;
    support_contract := $needle$            (
                'public.starring_runtime_interaction_receipt_schema_manifest_v1()',
                '',
                'boolean',
                TRUE
            )$needle$;
    support_replacement := support_contract || $extension$,
            (
                'public.guard_runtime_interaction_effect_root_v1()',
                '',
                'trigger',
                FALSE
            ),
            (
                'public.guard_runtime_interaction_effect_head_v1()',
                '',
                'trigger',
                FALSE
            ),
            (
                'public.guard_runtime_interaction_effect_event_v1()',
                '',
                'trigger',
                FALSE
            ),
            (
                'public.guard_runtime_interaction_effect_rollback_v1()',
                '',
                'trigger',
                FALSE
            ),
            (
                'public.guard_runtime_interaction_effect_response_token_delete_v1()',
                '',
                'trigger',
                FALSE
            ),
            (
                'public.starring_runtime_interaction_effect_receipt_terminal_sync_v1()',
                '',
                'trigger',
                FALSE
            ),
            (
                'public.starring_runtime_interaction_effect_complete_receipt_v1(text,text,text,bytea,timestamp with time zone)',
                'expected_application_id text, expected_interaction_id text, proposed_outcome_code text, proposed_result_digest bytea, proposed_observed_at timestamp with time zone',
                'bigint',
                TRUE
            ),
            (
                'public.starring_runtime_interaction_effect_resolve_receipt_v1(text,text,bytea,boolean)',
                'expected_application_id text, expected_interaction_id text, proposed_observation_digest bytea, response_token_unavailable boolean',
                'TABLE(outcome_name text, receipt_state text, resulting_head_revision bigint, resulting_claim_revision bigint, resulting_claim_expires_at timestamp with time zone, observed_database_now timestamp with time zone)',
                TRUE
            ),
            (
                'public.starring_runtime_interaction_effect_require_rollback_v1(text,text,text,timestamp with time zone)',
                'expected_application_id text, expected_interaction_id text, proposed_abort_reason text, observed_at timestamp with time zone',
                'boolean',
                TRUE
            ),
            (
                'public.starring_runtime_interaction_effect_try_complete_rollback_v1(text,text,timestamp with time zone)',
                'expected_application_id text, expected_interaction_id text, observed_at timestamp with time zone',
                'boolean',
                TRUE
            ),
            (
                'public.starring_runtime_interaction_effect_schema_manifest_v1()',
                '',
                'boolean',
                TRUE
            )$extension$;
    trigger_contract := $needle$            (
                'public.runtime_interaction_receipt_token_secrets_v1',
                'runtime_interaction_receipt_token_secrets_v1_immutable_truncate',
                'public.guard_runtime_interaction_receipt_token_v1()',
                34
            )$needle$;
    trigger_replacement := trigger_contract || $extension$,
            (
                'public.runtime_interaction_effect_roots_v1',
                'runtime_interaction_effect_roots_v1_immutable_mutation',
                'public.guard_runtime_interaction_effect_root_v1()',
                27
            ),
            (
                'public.runtime_interaction_effect_roots_v1',
                'runtime_interaction_effect_roots_v1_immutable_truncate',
                'public.guard_runtime_interaction_effect_root_v1()',
                34
            ),
            (
                'public.runtime_interaction_effect_rollbacks_v1',
                'runtime_interaction_effect_rollbacks_v1_guard_mutation',
                'public.guard_runtime_interaction_effect_rollback_v1()',
                27
            ),
            (
                'public.runtime_interaction_effect_rollbacks_v1',
                'runtime_interaction_effect_rollbacks_v1_guard_truncate',
                'public.guard_runtime_interaction_effect_rollback_v1()',
                34
            ),
            (
                'public.runtime_interaction_receipt_heads_v1',
                'runtime_interaction_receipt_heads_v1_effect_terminal_sync',
                'public.starring_runtime_interaction_effect_receipt_terminal_sync_v1()',
                17
            ),
            (
                'public.runtime_interaction_receipt_token_secrets_v1',
                'runtime_interaction_receipt_token_secrets_v1_effect_delete_guar',
                'public.guard_runtime_interaction_effect_response_token_delete_v1()',
                11
            ),
            (
                'public.runtime_interaction_effect_heads_v1',
                'runtime_interaction_effect_heads_v1_guard_mutation',
                'public.guard_runtime_interaction_effect_head_v1()',
                27
            ),
            (
                'public.runtime_interaction_effect_heads_v1',
                'runtime_interaction_effect_heads_v1_guard_truncate',
                'public.guard_runtime_interaction_effect_head_v1()',
                34
            ),
            (
                'public.runtime_interaction_effect_events_v1',
                'runtime_interaction_effect_events_v1_immutable_mutation',
                'public.guard_runtime_interaction_effect_event_v1()',
                27
            ),
            (
                'public.runtime_interaction_effect_events_v1',
                'runtime_interaction_effect_events_v1_immutable_truncate',
                'public.guard_runtime_interaction_effect_event_v1()',
                34
            )$extension$;
    allowlist_contract := $needle$            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_receipt_terminalize_expired_v1(text,text,bigint,bigint,text,text,bytea)'
            )$needle$;
    allowlist_replacement := allowlist_contract || $extension$,
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_effect_plan_bind_v1(text,text,bigint,bigint,text,bytea,bytea,bytea,jsonb)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_effect_intend_v1(text,text,bigint,bigint,text,bytea,bigint,bigint,bytea,bytea,bytea,jsonb,bytea,jsonb,bigint)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_effect_finish_v1(text,text,bigint,bigint,text,bytea,bigint,bigint,bytea,text,text)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_effect_scan_recoverable_v1(timestamp with time zone,text,text,bigint,timestamp with time zone,text,text,bigint,bigint)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_effect_recovery_claim_v1(text,text,bigint,bigint,text,text,text,bigint,bigint,bigint,bigint)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_effect_reconcile_v1(text,text,bigint,bigint,bigint,text,text,text,bigint,bigint,bigint,text,text,bytea,text,bytea,text,bigint)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_effect_compensation_intend_v1(text,text,bigint,bigint,text,text,text,bigint,bigint,bigint,bytea,bytea,bigint)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_effect_compensation_finish_v1(text,text,bigint,bigint,bigint,text,bytea,text,bytea,bigint)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_effect_response_tail_scan_v1(timestamp with time zone,text,text,bigint,timestamp with time zone,text,text,bigint,bigint)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_effect_response_tail_claim_v1(text,text,bigint,bigint,text,text,text,bigint,bigint,bigint,bytea,bytea,bytea,bigint)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_effect_response_tail_finalize_v1(text,text,bigint,bigint,text,bigint,bigint,text,text,text,bigint,bigint,bigint,bytea,bytea,text,bytea,bytea,bigint)'
            )$extension$;

    IF function_definition IS NULL
        OR pg_catalog.strpos(function_definition, relation_contract) = 0
        OR pg_catalog.strpos(function_definition, capability_contract) = 0
        OR pg_catalog.strpos(function_definition, support_contract) = 0
        OR pg_catalog.strpos(function_definition, trigger_contract) = 0
        OR pg_catalog.strpos(function_definition, allowlist_contract) = 0
        OR pg_catalog.strpos(
            pg_catalog.substr(
                function_definition,
                pg_catalog.strpos(function_definition, relation_contract)
                    + pg_catalog.length(relation_contract)
            ),
            relation_contract
        ) <> 0
        OR pg_catalog.strpos(
            pg_catalog.substr(
                function_definition,
                pg_catalog.strpos(function_definition, capability_contract)
                    + pg_catalog.length(capability_contract)
            ),
            capability_contract
        ) <> 0
        OR pg_catalog.strpos(
            pg_catalog.substr(
                function_definition,
                pg_catalog.strpos(function_definition, support_contract)
                    + pg_catalog.length(support_contract)
            ),
            support_contract
        ) <> 0
        OR pg_catalog.strpos(
            pg_catalog.substr(
                function_definition,
                pg_catalog.strpos(function_definition, trigger_contract)
                    + pg_catalog.length(trigger_contract)
            ),
            trigger_contract
        ) <> 0
        OR pg_catalog.strpos(
            pg_catalog.substr(
                function_definition,
                pg_catalog.strpos(function_definition, allowlist_contract)
                    + pg_catalog.length(allowlist_contract)
            ),
            allowlist_contract
        ) <> 0
    THEN
        RAISE EXCEPTION 'runtime interaction effect readiness extension failed'
            USING ERRCODE = '55000';
    END IF;

    function_definition := pg_catalog.replace(
        function_definition,
        relation_contract,
        relation_replacement
    );
    function_definition := pg_catalog.replace(
        function_definition,
        capability_contract,
        capability_replacement
    );
    function_definition := pg_catalog.replace(
        function_definition,
        support_contract,
        support_replacement
    );
    function_definition := pg_catalog.replace(
        function_definition,
        trigger_contract,
        trigger_replacement
    );
    function_definition := pg_catalog.replace(
        function_definition,
        allowlist_contract,
        allowlist_replacement
    );
    EXECUTE function_definition;
END;
$interaction_readiness_extension$;
DO $interaction_readiness_support_set_extension$
DECLARE
    function_definition TEXT;
    set_contract TEXT;
    set_replacement TEXT;
    rows_contract TEXT;
    rows_replacement TEXT;
BEGIN
    function_definition := pg_catalog.pg_get_functiondef(
        pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_database_readiness_v1()'
        )
    );
    set_contract := $needle$        OR function_row.proretset
$needle$;
    set_replacement := $extension$        OR (
            function_row.proretset
            AND expected.identity <>
                'public.starring_runtime_interaction_effect_resolve_receipt_v1(text,text,bytea,boolean)'
        )
        OR (
            NOT function_row.proretset
            AND expected.identity =
                'public.starring_runtime_interaction_effect_resolve_receipt_v1(text,text,bytea,boolean)'
        )
$extension$;
    rows_contract := $needle$        OR function_row.prorows <> 0::REAL
$needle$;
    rows_replacement := $extension$        OR function_row.prorows IS DISTINCT FROM CASE
            WHEN expected.identity =
                'public.starring_runtime_interaction_effect_resolve_receipt_v1(text,text,bytea,boolean)'
                THEN 1::REAL
            ELSE 0::REAL
        END
$extension$;
    IF function_definition IS NULL
        OR pg_catalog.strpos(function_definition, set_contract) = 0
        OR pg_catalog.strpos(
            pg_catalog.substr(
                function_definition,
                pg_catalog.strpos(function_definition, set_contract)
                    + pg_catalog.length(set_contract)
            ),
            set_contract
        ) <> 0
        OR pg_catalog.strpos(function_definition, rows_contract) = 0
        OR pg_catalog.strpos(
            pg_catalog.substr(
                function_definition,
                pg_catalog.strpos(function_definition, rows_contract)
                    + pg_catalog.length(rows_contract)
            ),
            rows_contract
        ) <> 0
    THEN
        RAISE EXCEPTION 'runtime interaction readiness support set extension failed'
            USING ERRCODE = '55000';
    END IF;
    function_definition := pg_catalog.replace(
        function_definition,
        set_contract,
        set_replacement
    );
    function_definition := pg_catalog.replace(
        function_definition,
        rows_contract,
        rows_replacement
    );
    EXECUTE function_definition;
END;
$interaction_readiness_support_set_extension$;
CREATE OR REPLACE FUNCTION public.starring_runtime_interaction_effect_schema_manifest_v1()
RETURNS BOOLEAN
LANGUAGE plpgsql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    observed_count BIGINT;
    observed_digest TEXT;
BEGIN
    WITH manifest(value) AS (
        SELECT pg_catalog.concat_ws(
            '|',
            'relation',
            pg_catalog.format('%I.%I', namespace.nspname, relation.relname),
            relation.relkind::TEXT,
            relation.relpersistence::TEXT,
            relation.relrowsecurity::TEXT,
            relation.relforcerowsecurity::TEXT,
            relation.relispartition::TEXT
        )
        FROM pg_catalog.pg_class AS relation
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = relation.relnamespace
        WHERE namespace.nspname = 'public'
            AND relation.relkind = 'r'
            AND relation.relname LIKE 'runtime_interaction_effect_%'
        UNION ALL
        SELECT pg_catalog.concat_ws(
            '|',
            'attribute',
            pg_catalog.format('%I.%I', namespace.nspname, relation.relname),
            attribute.attnum::TEXT,
            attribute.attname,
            pg_catalog.format_type(attribute.atttypid, attribute.atttypmod),
            attribute.attnotnull::TEXT,
            attribute.attgenerated::TEXT,
            attribute.attidentity::TEXT,
            attribute.attcollation::TEXT,
            COALESCE(pg_catalog.pg_get_expr(
                default_row.adbin,
                default_row.adrelid
            ), '')
        )
        FROM pg_catalog.pg_class AS relation
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = relation.relnamespace
        INNER JOIN pg_catalog.pg_attribute AS attribute
            ON attribute.attrelid = relation.oid
            AND attribute.attnum > 0
            AND NOT attribute.attisdropped
        LEFT JOIN pg_catalog.pg_attrdef AS default_row
            ON default_row.adrelid = relation.oid
            AND default_row.adnum = attribute.attnum
        WHERE namespace.nspname = 'public'
            AND relation.relkind = 'r'
            AND relation.relname LIKE 'runtime_interaction_effect_%'
        UNION ALL
        SELECT pg_catalog.concat_ws(
            '|',
            'constraint',
            pg_catalog.format(
                '%I.%I',
                relation_namespace.nspname,
                relation.relname
            ),
            constraint_row.conname,
            constraint_row.contype::TEXT,
            constraint_row.convalidated::TEXT,
            constraint_row.condeferrable::TEXT,
            constraint_row.condeferred::TEXT,
            constraint_row.connoinherit::TEXT,
            constraint_row.conislocal::TEXT,
            constraint_row.coninhcount::TEXT,
            (constraint_row.conparentid = 0)::TEXT,
            COALESCE(index_row.relname, ''),
            pg_catalog.pg_get_constraintdef(constraint_row.oid, TRUE)
        )
        FROM pg_catalog.pg_constraint AS constraint_row
        INNER JOIN pg_catalog.pg_class AS relation
            ON relation.oid = constraint_row.conrelid
        INNER JOIN pg_catalog.pg_namespace AS relation_namespace
            ON relation_namespace.oid = relation.relnamespace
        LEFT JOIN pg_catalog.pg_class AS index_row
            ON index_row.oid = constraint_row.conindid
        WHERE relation_namespace.nspname = 'public'
            AND relation.relname LIKE 'runtime_interaction_effect_%'
        UNION ALL
        SELECT pg_catalog.concat_ws(
            '|',
            'index',
            pg_catalog.format(
                '%I.%I',
                table_namespace.nspname,
                table_row.relname
            ),
            pg_catalog.format(
                '%I.%I',
                index_namespace.nspname,
                index_row.relname
            ),
            (index_row.relowner = table_row.relowner)::TEXT,
            index_row.relkind::TEXT,
            index_row.relpersistence::TEXT,
            index_row.relispartition::TEXT,
            index_method.amname,
            index_contract.indisprimary::TEXT,
            index_contract.indisunique::TEXT,
            index_contract.indisvalid::TEXT,
            index_contract.indisready::TEXT,
            index_contract.indislive::TEXT,
            index_contract.indimmediate::TEXT,
            index_contract.indisclustered::TEXT,
            index_contract.indisreplident::TEXT,
            index_contract.indnullsnotdistinct::TEXT,
            index_contract.indnkeyatts::TEXT,
            index_contract.indnatts::TEXT,
            index_contract.indkey::TEXT,
            index_contract.indcollation::TEXT,
            index_contract.indclass::TEXT,
            index_contract.indoption::TEXT,
            COALESCE(pg_catalog.pg_get_expr(
                index_contract.indexprs,
                index_contract.indrelid
            ), ''),
            COALESCE(pg_catalog.pg_get_expr(
                index_contract.indpred,
                index_contract.indrelid
            ), ''),
            pg_catalog.pg_get_indexdef(index_row.oid)
        )
        FROM pg_catalog.pg_index AS index_contract
        INNER JOIN pg_catalog.pg_class AS table_row
            ON table_row.oid = index_contract.indrelid
        INNER JOIN pg_catalog.pg_namespace AS table_namespace
            ON table_namespace.oid = table_row.relnamespace
        INNER JOIN pg_catalog.pg_class AS index_row
            ON index_row.oid = index_contract.indexrelid
        INNER JOIN pg_catalog.pg_namespace AS index_namespace
            ON index_namespace.oid = index_row.relnamespace
        INNER JOIN pg_catalog.pg_am AS index_method
            ON index_method.oid = index_row.relam
        WHERE table_namespace.nspname = 'public'
            AND table_row.relname LIKE 'runtime_interaction_effect_%'
        UNION ALL
        SELECT pg_catalog.concat_ws(
            '|',
            'function',
            pg_catalog.format(
                '%I.%I',
                namespace.nspname,
                function_row.proname
            ),
            pg_catalog.pg_get_function_arguments(function_row.oid),
            pg_catalog.pg_get_function_result(function_row.oid),
            function_row.prokind::TEXT,
            function_row.provolatile::TEXT,
            function_row.proisstrict::TEXT,
            function_row.proparallel::TEXT,
            function_row.prosecdef::TEXT,
            function_row.proretset::TEXT,
            function_row.prorows::TEXT,
            function_row.proconfig::TEXT,
            language_row.lanname,
            pg_catalog.pg_get_functiondef(function_row.oid)
        )
        FROM pg_catalog.pg_proc AS function_row
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = function_row.pronamespace
        INNER JOIN pg_catalog.pg_language AS language_row
            ON language_row.oid = function_row.prolang
        WHERE namespace.nspname = 'public'
            AND (
                function_row.proname LIKE
                    'starring_runtime_interaction_effect_%'
                OR function_row.proname LIKE
                    'guard_runtime_interaction_effect_%'
            )
            AND function_row.oid <> pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_effect_schema_manifest_v1()'
            )
        UNION ALL
        SELECT pg_catalog.concat_ws(
            '|',
            'trigger',
            pg_catalog.format(
                '%I.%I',
                namespace.nspname,
                relation.relname
            ),
            trigger_row.tgname,
            trigger_row.tgenabled::TEXT,
            trigger_row.tgtype::TEXT,
            trigger_row.tgnargs::TEXT,
            trigger_row.tgfoid::REGPROCEDURE::TEXT,
            pg_catalog.pg_get_triggerdef(trigger_row.oid, TRUE)
        )
        FROM pg_catalog.pg_trigger AS trigger_row
        INNER JOIN pg_catalog.pg_class AS relation
            ON relation.oid = trigger_row.tgrelid
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = relation.relnamespace
        WHERE namespace.nspname = 'public'
            AND (
                relation.relname LIKE 'runtime_interaction_effect_%'
                OR trigger_row.tgname IN (
                    'runtime_interaction_receipt_heads_v1_effect_terminal_sync',
                    'runtime_interaction_receipt_token_secrets_v1_effect_delete_guar'
                )
            )
            AND NOT trigger_row.tgisinternal
    )
    SELECT pg_catalog.count(*),
        pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.string_agg(value, E'\n' ORDER BY value),
                'UTF8'
            )),
            'hex'
        )
    INTO observed_count, observed_digest
    FROM manifest;

    RETURN observed_count = 154
        AND observed_digest =
            'f293f524ef97b491b6781a795888bf879aa0a82f790be699f31e4cee8c054152';
END;
$function$;
DO $receipt_manifest_refresh$
DECLARE
    function_definition TEXT;
    old_count TEXT;
    new_count TEXT;
    old_digest TEXT;
    new_digest TEXT;
BEGIN
    function_definition := pg_catalog.pg_get_functiondef(
        pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_receipt_schema_manifest_v1()'
        )
    );
    old_count := 'RETURN observed_count = 156';
    new_count := 'RETURN observed_count = 160';
    old_digest := 'bcbf6ee257defdb6c690a1f0d9752f0c84389093c6fc1ae3d9946b7aaecef302';
    new_digest := 'b7a20ed9976d5691ad23c75c61ca99e98b4f6e32e5a6f7ae6e9b36e973ae7ca5';

    IF function_definition IS NULL
        OR pg_catalog.strpos(function_definition, old_count) = 0
        OR pg_catalog.strpos(function_definition, old_digest) = 0
        OR pg_catalog.strpos(
            pg_catalog.substr(
                function_definition,
                pg_catalog.strpos(function_definition, old_count)
                    + pg_catalog.length(old_count)
            ),
            old_count
        ) <> 0
        OR pg_catalog.strpos(
            pg_catalog.substr(
                function_definition,
                pg_catalog.strpos(function_definition, old_digest)
                    + pg_catalog.length(old_digest)
            ),
            old_digest
        ) <> 0
    THEN
        RAISE EXCEPTION 'runtime interaction receipt manifest refresh failed'
            USING ERRCODE = '55000';
    END IF;

    function_definition := pg_catalog.replace(
        function_definition,
        old_count,
        new_count
    );
    function_definition := pg_catalog.replace(
        function_definition,
        old_digest,
        new_digest
    );
    EXECUTE function_definition;
END;
$receipt_manifest_refresh$;

DO $final_privileges$
DECLARE
    common_owner OID;
    object_row RECORD;
    grantee OID;
    grantee_name NAME;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.automation_instances');

    FOR object_row IN
        SELECT relation.oid,
            pg_catalog.format('%I.%I', namespace.nspname, relation.relname)
                AS identity
        FROM pg_catalog.pg_class AS relation
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = relation.relnamespace
        WHERE namespace.nspname = 'public'
            AND relation.relkind = 'r'
            AND relation.relname LIKE 'runtime_interaction_effect_%'
    LOOP
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON TABLE %s FROM PUBLIC CASCADE',
            object_row.identity
        );
        FOR grantee IN
            SELECT DISTINCT privilege.grantee
            FROM pg_catalog.pg_class AS relation
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                relation.relacl,
                pg_catalog.acldefault('r', relation.relowner)
            )) AS privilege
            WHERE relation.oid = object_row.oid
                AND privilege.grantee NOT IN (0, common_owner)
        LOOP
            grantee_name := pg_catalog.pg_get_userbyid(grantee);
            IF grantee_name IS NULL THEN
                RAISE EXCEPTION 'runtime interaction effect grantee is unavailable'
                    USING ERRCODE = '55000';
            END IF;
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON TABLE %s FROM %I CASCADE',
                object_row.identity,
                grantee_name
            );
        END LOOP;
    END LOOP;

    FOR object_row IN
        SELECT function_row.oid,
            function_row.oid::REGPROCEDURE::TEXT AS identity
        FROM pg_catalog.pg_proc AS function_row
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = function_row.pronamespace
        WHERE namespace.nspname = 'public'
            AND (
                function_row.proname LIKE
                    'starring_runtime_interaction_effect_%'
                OR function_row.proname LIKE
                    'guard_runtime_interaction_effect_%'
            )
    LOOP
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE',
            object_row.identity
        );
        FOR grantee IN
            SELECT DISTINCT privilege.grantee
            FROM pg_catalog.pg_proc AS function_row
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE function_row.oid = object_row.oid
                AND privilege.grantee NOT IN (0, common_owner)
        LOOP
            grantee_name := pg_catalog.pg_get_userbyid(grantee);
            IF grantee_name IS NULL THEN
                RAISE EXCEPTION 'runtime interaction effect grantee is unavailable'
                    USING ERRCODE = '55000';
            END IF;
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM %I CASCADE',
                object_row.identity,
                grantee_name
            );
        END LOOP;
    END LOOP;
END;
$final_privileges$;

DO $final_postflight$
DECLARE
    common_owner OID;
    readiness_definition TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.automation_instances');
    readiness_definition := pg_catalog.pg_get_functiondef(
        pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_database_readiness_v1()'
        )
    );

    IF common_owner IS NULL
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_class AS relation
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = relation.relnamespace
            WHERE namespace.nspname = 'public'
                AND relation.relkind = 'r'
                AND relation.relname LIKE 'runtime_interaction_effect_%'
                AND relation.relowner = common_owner
                AND relation.relpersistence = 'p'
                AND NOT relation.relrowsecurity
                AND NOT relation.relforcerowsecurity
                AND NOT EXISTS (
                    SELECT 1
                    FROM pg_catalog.aclexplode(COALESCE(
                        relation.relacl,
                        pg_catalog.acldefault('r', relation.relowner)
                    )) AS privilege
                    WHERE privilege.grantee <> common_owner
                )
        ) <> 4
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_proc AS function_row
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = function_row.pronamespace
            INNER JOIN pg_catalog.pg_language AS language_row
                ON language_row.oid = function_row.prolang
            WHERE namespace.nspname = 'public'
                AND (
                    function_row.proname LIKE
                        'starring_runtime_interaction_effect_%'
                    OR function_row.proname LIKE
                        'guard_runtime_interaction_effect_%'
                )
                AND function_row.proowner = common_owner
                AND function_row.prokind = 'f'
                AND function_row.provolatile = 'v'
                AND function_row.proparallel = 'u'
                AND function_row.prosecdef
                AND function_row.proconfig =
                    ARRAY['search_path=pg_catalog']::TEXT[]
                AND NOT function_row.proleakproof
                AND function_row.pronargdefaults = 0
                AND function_row.provariadic = 0
                AND language_row.lanname = 'plpgsql'
                AND NOT EXISTS (
                    SELECT 1
                    FROM pg_catalog.aclexplode(COALESCE(
                        function_row.proacl,
                        pg_catalog.acldefault('f', function_row.proowner)
                    )) AS privilege
                    WHERE privilege.grantee <> common_owner
                        OR privilege.grantor <> common_owner
                        OR privilege.privilege_type <> 'EXECUTE'
                        OR privilege.is_grantable
                )
        ) <> 22
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_trigger AS trigger_row
            INNER JOIN pg_catalog.pg_class AS relation
                ON relation.oid = trigger_row.tgrelid
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = relation.relnamespace
            WHERE namespace.nspname = 'public'
                AND (
                    relation.relname LIKE 'runtime_interaction_effect_%'
                    OR trigger_row.tgname IN (
                        'runtime_interaction_receipt_heads_v1_effect_terminal_sync',
                        'runtime_interaction_receipt_token_secrets_v1_effect_delete_guar'
                    )
                )
                AND NOT trigger_row.tgisinternal
                AND trigger_row.tgenabled = 'O'
                AND trigger_row.tgnargs = 0
                AND pg_catalog.octet_length(trigger_row.tgargs) = 0
                AND trigger_row.tgconstraint = 0
                AND NOT trigger_row.tgdeferrable
                AND NOT trigger_row.tginitdeferred
        ) <> 10
        OR NOT public.starring_runtime_interaction_effect_schema_manifest_v1()
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
        OR readiness_definition NOT LIKE
            '%starring_runtime_interaction_effect_plan_bind_v1%'
        OR readiness_definition NOT LIKE
            '%starring_runtime_interaction_effect_intend_v1%'
        OR readiness_definition NOT LIKE
            '%starring_runtime_interaction_effect_finish_v1%'
        OR readiness_definition NOT LIKE
            '%starring_runtime_interaction_effect_scan_recoverable_v1%'
        OR readiness_definition NOT LIKE
            '%starring_runtime_interaction_effect_recovery_claim_v1%'
        OR readiness_definition NOT LIKE
            '%starring_runtime_interaction_effect_reconcile_v1%'
        OR readiness_definition NOT LIKE
            '%starring_runtime_interaction_effect_compensation_intend_v1%'
        OR readiness_definition NOT LIKE
            '%starring_runtime_interaction_effect_compensation_finish_v1%'
        OR readiness_definition NOT LIKE
            '%starring_runtime_interaction_effect_response_tail_scan_v1%'
        OR readiness_definition NOT LIKE
            '%starring_runtime_interaction_effect_response_tail_claim_v1%'
        OR readiness_definition NOT LIKE
            '%starring_runtime_interaction_effect_response_tail_finalize_v1%'
    THEN
        RAISE EXCEPTION 'runtime interaction effect migration postflight failed'
            USING ERRCODE = '55000';
    END IF;
END;
$final_postflight$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
