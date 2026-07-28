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
    public.activation_request_approvals,
    public.automation_ruleset_activations,
    public.automation_ruleset_versions,
    public.product_action_receipts,
    public.product_action_receipt_idempotency_aliases,
    public.product_action_receipt_audit_evidence,
    public.product_audit_events,
    public.runtime_writer_fence,
    public.runtime_slot_writer_fences_v2,
    public.runtime_serving_leases,
    public.runtime_attestations,
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
    apply_executor OID;
    apply_executor_grant_count BIGINT;
    invalid_definition_count BIGINT;
    invalid_relation_count BIGINT;
    terminal_action_count BIGINT;
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
    INTO apply_executor, apply_executor_grant_count
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
    INTO invalid_definition_count
    FROM (
        VALUES
            (
                'public.starring_product_apply_lock_core_unfenced_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)',
                'abb3775e88f9926af64f676d0f94657c8f3c80890aad2b5372116ec886a464f0'
            ),
            (
                'public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)',
                '995457d082714a854257c2020ef854b6d0292302ca755182ac798c0225d1e715'
            ),
            (
                'public.validate_runtime_deployment_projection()',
                '4b35baa82ce44c07564593f677da9050d972ed881e1eb7305fbec77a39f14824'
            ),
            (
                'starring_runtime_private_v2.starring_runtime_product_drain_terminal_action_exact_v2(public.runtime_product_drain_terminal_actions_v2,public.runtime_product_operations_v2,public.runtime_drain_intents_v2)',
                '06f1eaa9f576e21b5f1a4b6c9a0ddfb24695d5e3bd4e482399e9954e5a854ffa'
            ),
            (
                'public.starring_runtime_execution_schema_manifest_v1()',
                '0e40c195026bf46ce6a8e5e70472d108de5deb533d1f072cf056e171c7078fe7'
            ),
            (
                'public.starring_runtime_execution_database_readiness_v1()',
                'a3674e7c69f24ce212ddf0598d23f448a47f0b6e7766dee20a78399d5b6477e7'
            )
    ) AS expected(identity, digest)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid =
            pg_catalog.to_regprocedure(expected.identity)
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(function_row.oid),
                'UTF8'
            )),
            'hex'
        ) IS DISTINCT FROM expected.digest;

    SELECT pg_catalog.count(*)
    INTO invalid_relation_count
    FROM (
        VALUES
            ('public.product_action_receipts'),
            ('public.product_audit_events'),
            ('public.runtime_writer_fence'),
            ('public.runtime_slot_writer_fences_v2'),
            ('public.runtime_serving_leases'),
            ('public.runtime_attestations'),
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

    SELECT pg_catalog.count(*)
    INTO terminal_action_count
    FROM public.runtime_product_drain_terminal_actions_v2;

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR apply_executor_grant_count > 1
        OR (
            apply_executor_grant_count = 1
            AND (
                apply_executor = 0
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
                        AND privilege.grantee = apply_executor
                        AND privilege.grantor = common_owner
                        AND privilege.privilege_type = 'EXECUTE'
                        AND NOT privilege.is_grantable
                )
            )
        )
        OR invalid_definition_count <> 0
        OR invalid_relation_count <> 0
        OR terminal_action_count <> 0
        OR pg_catalog.to_regprocedure(
            'public.starring_product_apply_consume_runtime_drain_v2(text,text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text,bigint,bytea,text,text,text,bigint,text,text,bytea,text,bytea,text,text,bytea)'
        ) IS NOT NULL
        OR pg_catalog.to_regprocedure(
            'starring_runtime_private_v2.starring_product_apply_consume_lock_core_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text)'
        ) IS NOT NULL
        OR pg_catalog.to_regprocedure(
            'starring_runtime_private_v2.starring_product_apply_commit_unfenced_core_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb,timestamp with time zone,boolean)'
        ) IS NOT NULL
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'PA001',
            MESSAGE = 'product_apply_consume_runtime_drain_v2_preflight_drift';
    END IF;
END;
$preflight$;

ALTER TABLE public.runtime_product_drain_terminal_actions_v2
ADD CONSTRAINT runtime_product_drain_terminal_actions_v2_receipt_fk
FOREIGN KEY (product_receipt_id)
REFERENCES public.product_action_receipts(receipt_id)
ON DELETE RESTRICT;

ALTER TABLE public.runtime_product_drain_terminal_actions_v2
ADD CONSTRAINT runtime_product_drain_terminal_actions_v2_audit_fk
FOREIGN KEY (product_audit_event_id)
REFERENCES public.product_audit_events(event_id)
ON DELETE RESTRICT;

ALTER TABLE public.runtime_product_drain_terminal_actions_v2
DROP CONSTRAINT runtime_product_drain_terminal_actions_v2_revision_check,
ADD COLUMN source_result_deployment_snapshot_bytes BYTEA NOT NULL,
ADD COLUMN result_deployment_snapshot_bytes BYTEA,
ADD CONSTRAINT runtime_product_drain_terminal_actions_v2_revision_check
    CHECK (
        source_intent_revision
            BETWEEN 1 AND 9223372036854775806
        AND result_intent_revision =
            source_intent_revision + 1
        AND source_deployment_revision
            BETWEEN 1 AND 9223372036854775806
        AND source_result_deployment_revision =
            source_deployment_revision + 1
        AND (
            (
                terminal_kind = 'consumed'
                AND result_deployment_id IS NOT NULL
                AND result_deployment_id
                    ~ '^[A-Za-z0-9_.:-]{1,128}$'
                AND result_deployment_revision IS NOT NULL
                AND result_deployment_revision = 1
                AND result_deployment_snapshot_digest IS NOT NULL
                AND result_deployment_snapshot_digest
                    ~ '^[0-9a-f]{64}$'
            )
            OR (
                terminal_kind = 'cancelled'
                AND result_deployment_id IS NULL
                AND result_deployment_revision IS NULL
                AND result_deployment_snapshot_digest IS NULL
            )
        )
        AND installation_authority_revision
            BETWEEN 1 AND 9223372036854775807
    ),
ADD CONSTRAINT runtime_product_drain_terminal_actions_v2_snapshot_bytes_check
    CHECK (
        pg_catalog.octet_length(
            source_result_deployment_snapshot_bytes
        ) BETWEEN 32 AND 262144
        AND source_result_deployment_snapshot_digest =
            pg_catalog.encode(
                pg_catalog.sha256(
                    source_result_deployment_snapshot_bytes
                ),
                'hex'
            )
        AND (
            (
                terminal_kind = 'consumed'
                AND result_deployment_snapshot_bytes IS NOT NULL
                AND pg_catalog.octet_length(
                    result_deployment_snapshot_bytes
                ) BETWEEN 32 AND 262144
                AND result_deployment_snapshot_digest =
                    pg_catalog.encode(
                        pg_catalog.sha256(
                            result_deployment_snapshot_bytes
                        ),
                        'hex'
                    )
            )
            OR (
                terminal_kind = 'cancelled'
                AND result_deployment_snapshot_bytes IS NULL
            )
        )
    );

DO $patch_terminal_deployment_identity_and_snapshots$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'starring_runtime_private_v2.starring_runtime_product_drain_terminal_projection_v2(text,text,text,text,text,text,text,text,text,bigint,text,bigint,bytea,text,bigint,bigint,text,text,bigint,text,bigint,bigint,text,text,text,bigint,timestamp with time zone)'
    );
    previous_fragment :=
        '                requested_result_deployment_id IS NULL' || E'\n' ||
        '                OR requested_result_deployment_id' || E'\n' ||
        '                    !~ ''^[0-9a-f]{32}$''';
    next_fragment :=
        '                requested_result_deployment_id IS NULL' || E'\n' ||
        '                OR requested_result_deployment_id' || E'\n' ||
        '                    !~ ''^[A-Za-z0-9_.:-]{1,128}$''';
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
                'product_drain_terminal_projection_deployment_identity_drift';
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
        'starring_runtime_private_v2.starring_runtime_product_drain_terminal_action_exact_v2(public.runtime_product_drain_terminal_actions_v2,public.runtime_product_operations_v2,public.runtime_drain_intents_v2)'
    );
    previous_fragment :=
        '    state_terminal_microseconds NUMERIC;' || E'\n' ||
        '    recorded_terminal_microseconds NUMERIC;' || E'\n' ||
        '    expected_projection BYTEA;';
    next_fragment :=
        '    state_terminal_microseconds NUMERIC;' || E'\n' ||
        '    recorded_terminal_microseconds NUMERIC;' || E'\n' ||
        '    expected_projection BYTEA;' || E'\n' ||
        '    source_result_snapshot JSONB;' || E'\n' ||
        '    result_snapshot JSONB;';
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
                'product_drain_terminal_exact_snapshot_declaration_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '        state_value := pg_catalog.convert_from(' || E'\n' ||
        '            drain_row.canonical_state_bytes,' || E'\n' ||
        '            ''UTF8''' || E'\n' ||
        '        )::JSONB;';
    next_fragment :=
        '        state_value := pg_catalog.convert_from(' || E'\n' ||
        '            drain_row.canonical_state_bytes,' || E'\n' ||
        '            ''UTF8''' || E'\n' ||
        '        )::JSONB;' || E'\n' ||
        '        source_result_snapshot := pg_catalog.convert_from(' || E'\n' ||
        '            action_row.source_result_deployment_snapshot_bytes,' || E'\n' ||
        '            ''UTF8''' || E'\n' ||
        '        )::JSONB;' || E'\n' ||
        '        result_snapshot := CASE' || E'\n' ||
        '            WHEN action_row.result_deployment_snapshot_bytes IS NULL' || E'\n' ||
        '            THEN NULL' || E'\n' ||
        '            ELSE pg_catalog.convert_from(' || E'\n' ||
        '                action_row.result_deployment_snapshot_bytes,' || E'\n' ||
        '                ''UTF8''' || E'\n' ||
        '            )::JSONB' || E'\n' ||
        '        END;';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'product_drain_terminal_exact_snapshot_decode_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '        OR action_row.source_result_deployment_snapshot_digest' || E'\n' ||
        '            !~ ''^[0-9a-f]{64}$''';
    next_fragment :=
        '        OR action_row.source_result_deployment_snapshot_digest' || E'\n' ||
        '            !~ ''^[0-9a-f]{64}$''' || E'\n' ||
        '        OR pg_catalog.octet_length(' || E'\n' ||
        '            action_row.source_result_deployment_snapshot_bytes' || E'\n' ||
        '        ) NOT BETWEEN 32 AND 262144' || E'\n' ||
        '        OR action_row.source_result_deployment_snapshot_digest <>' || E'\n' ||
        '            pg_catalog.encode(' || E'\n' ||
        '                pg_catalog.sha256(' || E'\n' ||
        '                    action_row.source_result_deployment_snapshot_bytes' || E'\n' ||
        '                ),' || E'\n' ||
        '                ''hex''' || E'\n' ||
        '            )' || E'\n' ||
        '        OR pg_catalog.jsonb_typeof(source_result_snapshot) <> ''object''' || E'\n' ||
        '        OR source_result_snapshot #>> ''{identity,tenant_id}''' || E'\n' ||
        '            <> product_row.tenant_id' || E'\n' ||
        '        OR source_result_snapshot #>> ''{identity,installation_id}''' || E'\n' ||
        '            <> product_row.installation_id' || E'\n' ||
        '        OR source_result_snapshot #>> ''{identity,deployment_id}''' || E'\n' ||
        '            <> product_row.deployment_id' || E'\n' ||
        '        OR (source_result_snapshot ->> ''revision'')::NUMERIC' || E'\n' ||
        '            IS DISTINCT FROM action_row.source_result_deployment_revision';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'product_drain_terminal_exact_source_snapshot_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '                OR action_row.result_deployment_id IS NULL' || E'\n' ||
        '                OR action_row.result_deployment_id' || E'\n' ||
        '                    !~ ''^[0-9a-f]{32}$''';
    next_fragment :=
        '                OR action_row.result_deployment_id IS NULL' || E'\n' ||
        '                OR action_row.result_deployment_id' || E'\n' ||
        '                    !~ ''^[A-Za-z0-9_.:-]{1,128}$''';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'product_drain_terminal_exact_deployment_identity_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '                OR action_row.result_deployment_snapshot_digest' || E'\n' ||
        '                    !~ ''^[0-9a-f]{64}$''' || E'\n' ||
        '                OR (' || E'\n' ||
        '                    state_value #>> ''{state,resulting_revision}''' || E'\n' ||
        '                )::NUMERIC IS DISTINCT FROM 1';
    next_fragment :=
        '                OR action_row.result_deployment_snapshot_digest' || E'\n' ||
        '                    !~ ''^[0-9a-f]{64}$''' || E'\n' ||
        '                OR pg_catalog.octet_length(' || E'\n' ||
        '                    action_row.result_deployment_snapshot_bytes' || E'\n' ||
        '                ) NOT BETWEEN 32 AND 262144' || E'\n' ||
        '                OR action_row.result_deployment_snapshot_digest <>' || E'\n' ||
        '                    pg_catalog.encode(' || E'\n' ||
        '                        pg_catalog.sha256(' || E'\n' ||
        '                            action_row.result_deployment_snapshot_bytes' || E'\n' ||
        '                        ),' || E'\n' ||
        '                        ''hex''' || E'\n' ||
        '                    )' || E'\n' ||
        '                OR pg_catalog.jsonb_typeof(result_snapshot) <> ''object''' || E'\n' ||
        '                OR result_snapshot #>> ''{identity,tenant_id}''' || E'\n' ||
        '                    <> product_row.tenant_id' || E'\n' ||
        '                OR result_snapshot #>> ''{identity,installation_id}''' || E'\n' ||
        '                    <> product_row.installation_id' || E'\n' ||
        '                OR result_snapshot #>> ''{identity,deployment_id}''' || E'\n' ||
        '                    <> action_row.result_deployment_id' || E'\n' ||
        '                OR (result_snapshot ->> ''revision'')::NUMERIC' || E'\n' ||
        '                    IS DISTINCT FROM action_row.result_deployment_revision' || E'\n' ||
        '                OR (' || E'\n' ||
        '                    state_value #>> ''{state,resulting_revision}''' || E'\n' ||
        '                )::NUMERIC IS DISTINCT FROM 1';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'product_drain_terminal_exact_result_snapshot_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '                OR action_row.result_deployment_snapshot_digest IS NOT NULL' || E'\n' ||
        '            )';
    next_fragment :=
        '                OR action_row.result_deployment_snapshot_digest IS NOT NULL' || E'\n' ||
        '                OR action_row.result_deployment_snapshot_bytes IS NOT NULL' || E'\n' ||
        '            )';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'product_drain_terminal_exact_cancelled_snapshot_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$patch_terminal_deployment_identity_and_snapshots$;

CREATE FUNCTION starring_runtime_private_v2.starring_product_apply_consumed_terminal_replay_exact_v2(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_promotion_id TEXT,
    expected_principal_id TEXT,
    idempotency_key_digest_candidates TEXT[],
    semantic_request_digest TEXT,
    expected_payload_digest TEXT,
    expected_result_deployment_id TEXT,
    expected_resulting_revision BIGINT,
    receipt_row public.product_action_receipts,
    audit_row public.product_audit_events
)
RETURNS TEXT
LANGUAGE plpgsql
STABLE
STRICT
PARALLEL UNSAFE
SECURITY INVOKER
SET search_path = pg_catalog
AS $function$
DECLARE
    correlated_root_count BIGINT;
    correlated_action_count BIGINT;
    action_signal_count BIGINT;
    product_row public.runtime_product_operations_v2%ROWTYPE;
    drain_row public.runtime_drain_intents_v2%ROWTYPE;
    action_row public.runtime_product_drain_terminal_actions_v2%ROWTYPE;
    source_deployment_row public.runtime_deployments%ROWTYPE;
    result_deployment_row public.runtime_deployments%ROWTYPE;
    slot_fence_row public.runtime_slot_writer_fences_v2%ROWTYPE;
BEGIN
    SELECT pg_catalog.count(*)
    INTO correlated_root_count
    FROM public.runtime_product_operations_v2 AS product
    INNER JOIN public.runtime_drain_intents_v2 AS drain
        ON drain.product_operation_id =
            product.product_operation_id
    WHERE product.tenant_id = expected_tenant_id
        AND product.installation_id =
            expected_installation_id
        AND product.product_mutation_request_bytes =
            starring_runtime_private_v2.starring_runtime_product_mutation_bytes_v2(
                product.product_operation_id,
                product.tenant_id,
                product.installation_id,
                product.deployment_id,
                product.expected_revision,
                drain.slot_guild_id,
                drain.slot_ruleset_key,
                product.expected_target_guild_id,
                product.expected_target_ruleset_key,
                product.expected_target_version,
                product.expected_target_content_hash,
                product.expected_target_binding_revision,
                product.expected_target_binding_fingerprint,
                'apply',
                semantic_request_digest
            );

    SELECT pg_catalog.count(*)
    INTO action_signal_count
    FROM public.runtime_product_drain_terminal_actions_v2 AS action
    WHERE action.product_action_idempotency_digest =
            ANY(idempotency_key_digest_candidates)
        OR action.product_action_semantic_request_digest =
            semantic_request_digest;

    IF correlated_root_count = 0 THEN
        RETURN CASE action_signal_count
            WHEN 0 THEN 'not_correlated'
            ELSE 'persistence_corrupt'
        END;
    END IF;
    IF correlated_root_count <> 1 THEN
        RETURN 'persistence_corrupt';
    END IF;

    SELECT product.*
    INTO STRICT product_row
    FROM public.runtime_product_operations_v2 AS product
    INNER JOIN public.runtime_drain_intents_v2 AS drain
        ON drain.product_operation_id =
            product.product_operation_id
    WHERE product.tenant_id = expected_tenant_id
        AND product.installation_id =
            expected_installation_id
        AND product.product_mutation_request_bytes =
            starring_runtime_private_v2.starring_runtime_product_mutation_bytes_v2(
                product.product_operation_id,
                product.tenant_id,
                product.installation_id,
                product.deployment_id,
                product.expected_revision,
                drain.slot_guild_id,
                drain.slot_ruleset_key,
                product.expected_target_guild_id,
                product.expected_target_ruleset_key,
                product.expected_target_version,
                product.expected_target_content_hash,
                product.expected_target_binding_revision,
                product.expected_target_binding_fingerprint,
                'apply',
                semantic_request_digest
            );
    SELECT drain.*
    INTO STRICT drain_row
    FROM public.runtime_drain_intents_v2 AS drain
    WHERE drain.product_operation_id =
            product_row.product_operation_id;

    SELECT pg_catalog.count(*)
    INTO correlated_action_count
    FROM public.runtime_product_drain_terminal_actions_v2 AS action
    WHERE action.drain_intent_id =
            drain_row.drain_intent_id
        OR action.product_action_idempotency_digest =
            ANY(idempotency_key_digest_candidates)
        OR action.product_action_semantic_request_digest =
            semantic_request_digest;
    IF correlated_action_count <> 1 THEN
        RETURN 'persistence_corrupt';
    END IF;

    SELECT action.*
    INTO STRICT action_row
    FROM public.runtime_product_drain_terminal_actions_v2 AS action
    WHERE action.drain_intent_id =
            drain_row.drain_intent_id;
    SELECT deployment.*
    INTO STRICT source_deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.deployment_id =
            product_row.deployment_id;
    SELECT deployment.*
    INTO STRICT result_deployment_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.deployment_id =
            action_row.result_deployment_id;
    SELECT fence.*
    INTO STRICT slot_fence_row
    FROM public.runtime_slot_writer_fences_v2 AS fence
    WHERE fence.slot_guild_id =
            drain_row.slot_guild_id
        AND fence.slot_ruleset_key =
            drain_row.slot_ruleset_key;

    IF action_row.terminal_kind <> 'consumed'
        OR drain_row.intent_state <> 'consumed'
        OR NOT starring_runtime_private_v2.starring_runtime_product_drain_terminal_action_exact_v2(
            action_row,
            product_row,
            drain_row
        )
        OR action_row.product_action_idempotency_digest <>
            ALL(idempotency_key_digest_candidates)
        OR action_row.product_action_semantic_request_digest <>
            semantic_request_digest
        OR action_row.result_deployment_id <>
            expected_result_deployment_id
        OR receipt_row.receipt_id <>
            action_row.product_receipt_id
        OR receipt_row.tenant_id <> expected_tenant_id
        OR receipt_row.installation_id <>
            expected_installation_id
        OR receipt_row.principal_id <> expected_principal_id
        OR receipt_row.endpoint_domain <> 'product_apply_v1'
        OR receipt_row.idempotency_key_digest <>
            action_row.product_action_idempotency_digest
        OR receipt_row.request_digest <>
            semantic_request_digest
        OR receipt_row.target_resource_type <>
            'authoring_promotion'
        OR receipt_row.target_resource_id <>
            expected_promotion_id
        OR receipt_row.resulting_revision IS DISTINCT FROM
            expected_resulting_revision
        OR receipt_row.resulting_state <> 'applied'
        OR receipt_row.result_code <> 'runtime_requested'
        OR receipt_row.http_disposition_class <> 2
        OR receipt_row.completed_at <>
            action_row.terminal_database_time
        OR audit_row.event_id <>
            action_row.product_audit_event_id
        OR audit_row.receipt_id <> receipt_row.receipt_id
        OR audit_row.tenant_id <> expected_tenant_id
        OR audit_row.installation_id <>
            expected_installation_id
        OR audit_row.principal_id <> expected_principal_id
        OR audit_row.action <> 'promotion.apply'
        OR audit_row.payload_digest <>
            expected_payload_digest
        OR audit_row.authority_observation_digest <>
            action_row.authority_observation_digest
        OR audit_row.installation_authority_revision <>
            action_row.installation_authority_revision
        OR audit_row.occurred_at <>
            action_row.terminal_database_time
        OR source_deployment_row.tenant_id <>
            expected_tenant_id
        OR source_deployment_row.installation_id <>
            expected_installation_id
        OR source_deployment_row.guild_id <>
            drain_row.slot_guild_id
        OR source_deployment_row.ruleset_key <>
            drain_row.slot_ruleset_key
        OR source_deployment_row.phase <> 'superseded'
        OR source_deployment_row.revision <
            action_row.source_result_deployment_revision
        OR result_deployment_row.tenant_id <>
            expected_tenant_id
        OR result_deployment_row.installation_id <>
            expected_installation_id
        OR result_deployment_row.guild_id <>
            drain_row.slot_guild_id
        OR result_deployment_row.ruleset_key <>
            drain_row.slot_ruleset_key
        OR result_deployment_row.revision <
            action_row.result_deployment_revision
        OR result_deployment_row.runtime_generation <=
            source_deployment_row.runtime_generation
        OR slot_fence_row.writer_epoch <
            action_row.successor_slot_writer_epoch
    THEN
        RETURN 'persistence_corrupt';
    END IF;
    RETURN 'exact';
EXCEPTION
    WHEN OTHERS THEN
        RETURN 'persistence_corrupt';
END;
$function$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_product_drain_source_supersession_exact_v2(
    source_row public.runtime_deployments,
    result_snapshot JSONB,
    drain_row public.runtime_drain_intents_v2,
    result_deployment_snapshot JSONB,
    requested_terminal_time TIMESTAMPTZ
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
    source_acknowledgement JSONB;
    acknowledged_microseconds NUMERIC;
    terminal_microseconds NUMERIC;
    source_evidence_time TIMESTAMPTZ;
    acknowledged_time TIMESTAMPTZ;
BEGIN
    source_acknowledgement :=
        pg_catalog.convert_from(
            drain_row.canonical_state_bytes,
            'UTF8'
        )::JSONB #> '{state,acknowledgement}';
    acknowledged_microseconds := (
        source_acknowledgement
            ->> 'acknowledged_at_unix_microseconds'
    )::NUMERIC;
    terminal_microseconds :=
        EXTRACT(EPOCH FROM requested_terminal_time) * 1000000;
    acknowledged_time :=
        pg_catalog.to_timestamp(
            acknowledged_microseconds / 1000000
        );

    IF source_row.phase
            NOT IN ('awaiting_gateway_ready', 'live')
        OR source_row.revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR source_row.controller_id IS NOT NULL
        OR source_row.controller_fencing_token IS NOT NULL
        OR source_row.controller_acquired_at IS NOT NULL
        OR source_row.controller_lease_expires_at IS NOT NULL
        OR source_row.tenant_id <> drain_row.tenant_id
        OR source_row.installation_id
            <> drain_row.installation_id
        OR source_row.deployment_id <> drain_row.deployment_id
        OR source_row.guild_id <> drain_row.slot_guild_id
        OR source_row.ruleset_key <> drain_row.slot_ruleset_key
        OR source_row.revision <> drain_row.expected_revision
        OR drain_row.intent_state
            <> 'route_absent_acknowledged'
        OR NOT starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
            drain_row
        )
        OR acknowledged_microseconds NOT BETWEEN
            -62135596800000000 AND 253402300799999999
        OR acknowledged_microseconds <>
            pg_catalog.trunc(acknowledged_microseconds)
        OR terminal_microseconds NOT BETWEEN
            -62135596800000000 AND 253402300799999999
        OR terminal_microseconds <>
            pg_catalog.trunc(terminal_microseconds)
        OR terminal_microseconds < acknowledged_microseconds
        OR requested_terminal_time <= source_row.updated_at
        OR pg_catalog.jsonb_typeof(result_snapshot) <> 'object'
        OR pg_catalog.jsonb_typeof(result_deployment_snapshot)
            <> 'object'
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(result_snapshot)
        ) <> 17
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(
                result_deployment_snapshot
            )
        ) <> 17
        OR NOT result_snapshot ?& ARRAY[
            'identity',
            'target',
            'runtime_generation',
            'previous_runtime',
            'requested_at',
            'revision',
            'phase',
            'controller_lease',
            'last_fencing_token',
            'preflight',
            'drain',
            'activation',
            'panel_certificate',
            'gateway_ready',
            'live',
            'last_live_recovery',
            'last_runtime_failure'
        ]
        OR result_snapshot -> 'revision'
            IS DISTINCT FROM
                pg_catalog.to_jsonb(source_row.revision + 1)
        OR result_snapshot -> 'controller_lease'
            IS DISTINCT FROM 'null'::JSONB
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(
                result_snapshot -> 'phase'
            )
        ) <> 4
        OR result_snapshot #>> '{phase,phase}'
            <> 'superseded'
        OR result_snapshot #>> '{phase,reason}'
            <> 'correlated Product apply'
        OR (
            result_snapshot #>> '{phase,superseded_at}'
        )::TIMESTAMPTZ IS DISTINCT FROM
            requested_terminal_time
        OR result_snapshot #> '{phase,by,identity}'
            IS DISTINCT FROM
                result_deployment_snapshot -> 'identity'
        OR result_snapshot #> '{phase,by,target}'
            IS DISTINCT FROM
                result_deployment_snapshot -> 'target'
        OR result_snapshot #> '{phase,by,runtime_generation}'
            IS DISTINCT FROM
                result_deployment_snapshot -> 'runtime_generation'
        OR result_deployment_snapshot
                #>> '{identity,deployment_id}'
            = source_row.deployment_id
        OR result_deployment_snapshot
                #>> '{identity,tenant_id}'
            <> source_row.tenant_id
        OR result_deployment_snapshot
                #>> '{identity,installation_id}'
            <> source_row.installation_id
        OR result_deployment_snapshot
                #>> '{target,guild_id}'
            <> source_row.guild_id
        OR result_deployment_snapshot
                #>> '{target,ruleset_key}'
            <> source_row.ruleset_key
        OR (
            result_deployment_snapshot ->> 'runtime_generation'
        )::NUMERIC <= source_row.runtime_generation
    THEN
        RETURN FALSE;
    END IF;

    source_evidence_time := CASE source_row.phase
        WHEN 'live' THEN (
            source_row.snapshot #>> '{live,certified_at}'
        )::TIMESTAMPTZ
        ELSE (
            source_row.snapshot
                #>> '{panel_certificate,reconciled_at}'
        )::TIMESTAMPTZ
    END;
    IF source_evidence_time IS NULL
        OR acknowledged_time < source_evidence_time
    THEN
        RETURN FALSE;
    END IF;

    IF source_row.phase = 'awaiting_gateway_ready' THEN
        RETURN result_snapshot - ARRAY[
                'revision',
                'phase',
                'controller_lease'
            ] IS NOT DISTINCT FROM
                source_row.snapshot - ARRAY[
                    'revision',
                    'phase',
                    'controller_lease'
                ];
    END IF;

    RETURN result_snapshot - ARRAY[
            'revision',
            'phase',
            'controller_lease',
            'panel_certificate',
            'gateway_ready',
            'live',
            'last_live_recovery'
        ] IS NOT DISTINCT FROM
            source_row.snapshot - ARRAY[
                'revision',
                'phase',
                'controller_lease',
                'panel_certificate',
                'gateway_ready',
                'live',
                'last_live_recovery'
            ]
        AND result_snapshot -> 'panel_certificate'
            = 'null'::JSONB
        AND result_snapshot -> 'gateway_ready'
            = 'null'::JSONB
        AND result_snapshot -> 'live' = 'null'::JSONB
        AND (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.jsonb_object_keys(
                result_snapshot -> 'last_live_recovery'
            )
        ) = 4
        AND result_snapshot
                #> '{last_live_recovery,prior_live}'
            IS NOT DISTINCT FROM source_row.snapshot -> 'live'
        AND result_snapshot
                #>> '{last_live_recovery,kind}'
            = 'serving_disconnected'
        AND (
            result_snapshot
                #>> '{last_live_recovery,evidence_at}'
        )::TIMESTAMPTZ IS NOT DISTINCT FROM
            acknowledged_time
        AND (
            result_snapshot
                #>> '{last_live_recovery,recovered_at}'
        )::TIMESTAMPTZ IS NOT DISTINCT FROM
            requested_terminal_time;
EXCEPTION
    WHEN OTHERS THEN
        RETURN FALSE;
END;
$function$;


CREATE FUNCTION starring_runtime_private_v2.starring_runtime_product_drain_consume_root_exact_v2(
    product_row public.runtime_product_operations_v2,
    drain_row public.runtime_drain_intents_v2,
    source_row public.runtime_deployments,
    requested_product_operation_id TEXT,
    requested_drain_intent_id TEXT,
    requested_source_intent_revision BIGINT,
    requested_source_state_bytes BYTEA,
    requested_source_state_digest TEXT,
    requested_semantic_request_digest TEXT
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
    expected_product_bytes BYTEA;
    expected_drain_bytes BYTEA;
BEGIN
    expected_product_bytes :=
        starring_runtime_private_v2.starring_runtime_product_mutation_bytes_v2(
            requested_product_operation_id,
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
            requested_semantic_request_digest
        );
    expected_drain_bytes :=
        starring_runtime_private_v2.starring_runtime_drain_intent_bytes_v2(
            requested_drain_intent_id,
            requested_product_operation_id,
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
            requested_semantic_request_digest
        );
    RETURN product_row.product_operation_id =
            requested_product_operation_id
        AND product_row.tenant_id = source_row.tenant_id
        AND product_row.installation_id =
            source_row.installation_id
        AND product_row.deployment_id =
            source_row.deployment_id
        AND product_row.expected_revision = source_row.revision
        AND product_row.expected_target_guild_id =
            source_row.guild_id
        AND product_row.expected_target_ruleset_key =
            source_row.ruleset_key
        AND product_row.expected_target_version =
            source_row.target_version
        AND product_row.expected_target_content_hash =
            source_row.target_content_hash
        AND product_row.expected_target_binding_revision =
            source_row.binding_revision
        AND product_row.expected_target_binding_fingerprint =
            source_row.binding_fingerprint
        AND product_row.product_mutation_request_bytes =
            expected_product_bytes
        AND product_row.product_mutation_digest =
            starring_runtime_private_v2.starring_runtime_product_mutation_digest_v2(
                expected_product_bytes
            )
        AND drain_row.drain_intent_id =
            requested_drain_intent_id
        AND drain_row.product_operation_id =
            requested_product_operation_id
        AND drain_row.product_mutation_digest =
            product_row.product_mutation_digest
        AND drain_row.tenant_id = source_row.tenant_id
        AND drain_row.installation_id =
            source_row.installation_id
        AND drain_row.deployment_id =
            source_row.deployment_id
        AND drain_row.slot_guild_id = source_row.guild_id
        AND drain_row.slot_ruleset_key = source_row.ruleset_key
        AND drain_row.expected_revision = source_row.revision
        AND drain_row.drain_intent_request_bytes =
            expected_drain_bytes
        AND drain_row.drain_intent_digest =
            starring_runtime_private_v2.starring_runtime_drain_intent_digest_v2(
                expected_drain_bytes
            )
        AND drain_row.intent_revision =
            requested_source_intent_revision
        AND drain_row.intent_state =
            'route_absent_acknowledged'
        AND drain_row.canonical_state_bytes =
            requested_source_state_bytes
        AND drain_row.canonical_state_digest =
            requested_source_state_digest
        AND requested_source_state_digest =
            pg_catalog.encode(
                pg_catalog.sha256(requested_source_state_bytes),
                'hex'
            )
        AND starring_runtime_private_v2.starring_runtime_pending_drain_root_exact_v2(
            drain_row
        )
        AND starring_runtime_private_v2.starring_runtime_pending_drain_state_exact_v2(
            drain_row
        );
EXCEPTION
    WHEN OTHERS THEN
        RETURN FALSE;
END;
$function$;

DO $patch_product_drain_source_transition$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.validate_runtime_deployment_projection()'
    );

    previous_fragment :=
        '    pending_drain_history BOOLEAN;' || E'\n' ||
        'BEGIN';
    next_fragment :=
        '    pending_drain_history BOOLEAN;' || E'\n' ||
        '    product_drain_supersession BOOLEAN;' || E'\n' ||
        '    product_drain_row public.runtime_drain_intents_v2%ROWTYPE;' || E'\n' ||
        '    product_result_deployment_snapshot JSONB;' || E'\n' ||
        '    product_drain_gate_present BOOLEAN;' || E'\n' ||
        'BEGIN';
    IF definition IS NULL
        OR pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'product_drain_supersession_trigger_declaration_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    IF authority_outcome <> ''exact'' THEN' || E'\n' ||
        '        RAISE EXCEPTION ''runtime deployment product authority is not current''' || E'\n' ||
        '            USING ERRCODE = ''23514'';' || E'\n' ||
        '    END IF;' || E'\n' ||
        '    mutation_clock := public.starring_runtime_current_mutation_clock();';
    next_fragment :=
        '    mutation_clock := public.starring_runtime_current_mutation_clock();';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'product_drain_supersession_trigger_authority_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    snapshot_phase := NEW.snapshot -> ''phase'' ->> ''phase'';';
    next_fragment :=
        '    product_drain_gate_present := COALESCE(' || E'\n' ||
        '        pg_catalog.current_setting(' || E'\n' ||
        '            ''starring.runtime_product_drain_supersession_stage_v2'',' || E'\n' ||
        '            TRUE' || E'\n' ||
        '        ),' || E'\n' ||
        '        ''''' || E'\n' ||
        '    ) <> '''';' || E'\n' ||
        '    product_drain_supersession := FALSE;' || E'\n' ||
        '    IF product_drain_gate_present AND TG_OP = ''UPDATE'' THEN' || E'\n' ||
        '        BEGIN' || E'\n' ||
        '            SELECT drain.*' || E'\n' ||
        '            INTO STRICT product_drain_row' || E'\n' ||
        '            FROM public.runtime_drain_intents_v2 AS drain' || E'\n' ||
        '            WHERE drain.drain_intent_id = pg_catalog.current_setting(' || E'\n' ||
        '                    ''starring.runtime_product_drain_supersession_drain_intent_id_v2''' || E'\n' ||
        '                )' || E'\n' ||
        '            FOR KEY SHARE;' || E'\n' ||
        '            product_result_deployment_snapshot :=' || E'\n' ||
        '                pg_catalog.current_setting(' || E'\n' ||
        '                    ''starring.runtime_product_drain_supersession_result_snapshot_v2''' || E'\n' ||
        '                )::JSONB;' || E'\n' ||
        '            product_drain_supersession := (' || E'\n' ||
        '                pg_catalog.current_setting(' || E'\n' ||
        '                    ''starring.runtime_product_drain_supersession_stage_v2''' || E'\n' ||
        '                ) = ''source_update''' || E'\n' ||
        '                AND pg_catalog.current_setting(' || E'\n' ||
        '                    ''starring.runtime_product_drain_supersession_source_deployment_id_v2''' || E'\n' ||
        '                ) = OLD.deployment_id' || E'\n' ||
        '                AND pg_catalog.current_setting(' || E'\n' ||
        '                    ''starring.runtime_product_drain_supersession_source_revision_v2''' || E'\n' ||
        '                ) = OLD.revision::TEXT' || E'\n' ||
        '                AND pg_catalog.current_setting(' || E'\n' ||
        '                    ''starring.runtime_product_drain_supersession_result_deployment_id_v2''' || E'\n' ||
        '                ) = product_result_deployment_snapshot' || E'\n' ||
        '                    #>> ''{identity,deployment_id}''' || E'\n' ||
        '                AND pg_catalog.current_setting(' || E'\n' ||
        '                    ''starring.runtime_product_drain_supersession_terminal_microseconds_v2''' || E'\n' ||
        '                ) = (' || E'\n' ||
        '                    EXTRACT(EPOCH FROM mutation_clock) * 1000000' || E'\n' ||
        '                )::BIGINT::TEXT' || E'\n' ||
        '                AND starring_runtime_private_v2.starring_runtime_product_drain_source_supersession_exact_v2(' || E'\n' ||
        '                    OLD,' || E'\n' ||
        '                    NEW.snapshot,' || E'\n' ||
        '                    product_drain_row,' || E'\n' ||
        '                    product_result_deployment_snapshot,' || E'\n' ||
        '                    mutation_clock' || E'\n' ||
        '                )' || E'\n' ||
        '            );' || E'\n' ||
        '        EXCEPTION' || E'\n' ||
        '            WHEN OTHERS THEN' || E'\n' ||
        '                product_drain_supersession := FALSE;' || E'\n' ||
        '        END;' || E'\n' ||
        '        PERFORM pg_catalog.set_config(' || E'\n' ||
        '            ''starring.runtime_product_drain_supersession_stage_v2'',' || E'\n' ||
        '            '''',' || E'\n' ||
        '            TRUE' || E'\n' ||
        '        );' || E'\n' ||
        '        PERFORM pg_catalog.set_config(' || E'\n' ||
        '            ''starring.runtime_product_drain_supersession_drain_intent_id_v2'',' || E'\n' ||
        '            '''',' || E'\n' ||
        '            TRUE' || E'\n' ||
        '        );' || E'\n' ||
        '        PERFORM pg_catalog.set_config(' || E'\n' ||
        '            ''starring.runtime_product_drain_supersession_source_deployment_id_v2'',' || E'\n' ||
        '            '''',' || E'\n' ||
        '            TRUE' || E'\n' ||
        '        );' || E'\n' ||
        '        PERFORM pg_catalog.set_config(' || E'\n' ||
        '            ''starring.runtime_product_drain_supersession_source_revision_v2'',' || E'\n' ||
        '            '''',' || E'\n' ||
        '            TRUE' || E'\n' ||
        '        );' || E'\n' ||
        '        PERFORM pg_catalog.set_config(' || E'\n' ||
        '            ''starring.runtime_product_drain_supersession_result_deployment_id_v2'',' || E'\n' ||
        '            '''',' || E'\n' ||
        '            TRUE' || E'\n' ||
        '        );' || E'\n' ||
        '        PERFORM pg_catalog.set_config(' || E'\n' ||
        '            ''starring.runtime_product_drain_supersession_result_snapshot_v2'',' || E'\n' ||
        '            '''',' || E'\n' ||
        '            TRUE' || E'\n' ||
        '        );' || E'\n' ||
        '        PERFORM pg_catalog.set_config(' || E'\n' ||
        '            ''starring.runtime_product_drain_supersession_terminal_microseconds_v2'',' || E'\n' ||
        '            '''',' || E'\n' ||
        '            TRUE' || E'\n' ||
        '        );' || E'\n' ||
        '    END IF;' || E'\n' ||
        E'\n' ||
        '    IF authority_outcome <> ''exact''' || E'\n' ||
        '        AND NOT product_drain_supersession' || E'\n' ||
        '    THEN' || E'\n' ||
        '        RAISE EXCEPTION ''runtime deployment product authority is not current''' || E'\n' ||
        '            USING ERRCODE = ''23514'';' || E'\n' ||
        '    END IF;' || E'\n' ||
        E'\n' ||
        '    snapshot_phase := NEW.snapshot -> ''phase'' ->> ''phase'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'product_drain_supersession_trigger_gate_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            OR certification_awaiting_reset' || E'\n' ||
        '            OR (OLD.phase = ''live'' AND NEW.phase = ''runtime_pending'')';
    next_fragment :=
        '            OR certification_awaiting_reset' || E'\n' ||
        '            OR product_drain_supersession' || E'\n' ||
        '            OR (OLD.phase = ''live'' AND NEW.phase = ''runtime_pending'')';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'product_drain_supersession_trigger_phase_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
    EXECUTE definition;
END;
$patch_product_drain_source_transition$;

CREATE FUNCTION starring_runtime_private_v2.starring_runtime_product_drain_supersede_source_v2(
    requested_drain_intent_id TEXT,
    requested_source_deployment_id TEXT,
    requested_source_deployment_revision BIGINT,
    requested_source_result_snapshot_bytes BYTEA,
    requested_source_result_snapshot_digest TEXT,
    requested_result_deployment_snapshot_bytes BYTEA,
    requested_result_deployment_snapshot_digest TEXT,
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
    result_deployment_snapshot JSONB;
    result_deployment_id TEXT;
    terminal_microseconds NUMERIC;
    setting_name TEXT;
BEGIN
    terminal_microseconds :=
        EXTRACT(EPOCH FROM requested_terminal_time) * 1000000;
    IF requested_drain_intent_id !~ '^[0-9a-f]{32}$'
        OR requested_source_deployment_id
            !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR requested_source_deployment_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR pg_catalog.octet_length(
            requested_source_result_snapshot_bytes
        ) NOT BETWEEN 32 AND 262144
        OR requested_source_result_snapshot_digest
            !~ '^[0-9a-f]{64}$'
        OR requested_source_result_snapshot_digest <>
            pg_catalog.encode(
                pg_catalog.sha256(
                    requested_source_result_snapshot_bytes
                ),
                'hex'
            )
        OR pg_catalog.octet_length(
            requested_result_deployment_snapshot_bytes
        ) NOT BETWEEN 32 AND 262144
        OR requested_result_deployment_snapshot_digest
            !~ '^[0-9a-f]{64}$'
        OR requested_result_deployment_snapshot_digest <>
            pg_catalog.encode(
                pg_catalog.sha256(
                    requested_result_deployment_snapshot_bytes
                ),
                'hex'
            )
        OR NOT pg_catalog.isfinite(requested_terminal_time)
        OR terminal_microseconds NOT BETWEEN
            -62135596800000000 AND 253402300799999999
        OR terminal_microseconds <> pg_catalog.trunc(
            terminal_microseconds
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX002',
            MESSAGE = 'runtime_product_drain_supersede_source_input_invalid';
    END IF;

    BEGIN
        result_snapshot := pg_catalog.convert_from(
            requested_source_result_snapshot_bytes,
            'UTF8'
        )::JSONB;
        result_deployment_snapshot := pg_catalog.convert_from(
            requested_result_deployment_snapshot_bytes,
            'UTF8'
        )::JSONB;
        result_deployment_id :=
            result_deployment_snapshot
                #>> '{identity,deployment_id}';
    EXCEPTION
        WHEN OTHERS THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX002',
                MESSAGE =
                    'runtime_product_drain_supersede_source_input_invalid';
    END;

    FOREACH setting_name IN ARRAY ARRAY[
        'starring.runtime_product_drain_supersession_stage_v2',
        'starring.runtime_product_drain_supersession_drain_intent_id_v2',
        'starring.runtime_product_drain_supersession_source_deployment_id_v2',
        'starring.runtime_product_drain_supersession_source_revision_v2',
        'starring.runtime_product_drain_supersession_result_deployment_id_v2',
        'starring.runtime_product_drain_supersession_result_snapshot_v2',
        'starring.runtime_product_drain_supersession_terminal_microseconds_v2'
    ]
    LOOP
        IF COALESCE(
            pg_catalog.current_setting(setting_name, TRUE),
            ''
        ) <> ''
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE =
                    'runtime_product_drain_supersede_source_gate_invalid';
        END IF;
    END LOOP;

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
    WHERE drain.drain_intent_id = requested_drain_intent_id
    FOR KEY SHARE;

    IF source_row.deployment_id IS NULL
        OR drain_row.drain_intent_id IS NULL
        OR NOT starring_runtime_private_v2.starring_runtime_product_drain_source_supersession_exact_v2(
            source_row,
            result_snapshot,
            drain_row,
            result_deployment_snapshot,
            requested_terminal_time
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX003',
            MESSAGE =
                'runtime_product_drain_supersede_source_source_stale';
    END IF;

    PERFORM pg_catalog.set_config(
        'starring.runtime_product_drain_supersession_stage_v2',
        'source_update',
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_product_drain_supersession_drain_intent_id_v2',
        requested_drain_intent_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_product_drain_supersession_source_deployment_id_v2',
        requested_source_deployment_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_product_drain_supersession_source_revision_v2',
        requested_source_deployment_revision::TEXT,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_product_drain_supersession_result_deployment_id_v2',
        result_deployment_id,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_product_drain_supersession_result_snapshot_v2',
        result_deployment_snapshot::TEXT,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_product_drain_supersession_terminal_microseconds_v2',
        terminal_microseconds::BIGINT::TEXT,
        TRUE
    );
    PERFORM pg_catalog.set_config(
        'starring.runtime_mutation_clock',
        requested_terminal_time::TEXT,
        TRUE
    );

    UPDATE public.runtime_deployments AS deployment
    SET snapshot = result_snapshot,
        revision = source_row.revision + 1,
        phase = 'superseded',
        controller_id = NULL,
        controller_fencing_token = NULL,
        controller_acquired_at = NULL,
        controller_lease_expires_at = NULL,
        next_retry_at = NULL,
        last_stable_error_code =
            result_snapshot #>>
                '{last_runtime_failure,failure,code}',
        live_attestation_id = NULL,
        live_at = NULL,
        blocked_at = NULL,
        superseded_at = requested_terminal_time,
        cancelled_at = NULL,
        updated_at = requested_terminal_time
    WHERE deployment.deployment_id =
            requested_source_deployment_id
        AND deployment.revision =
            requested_source_deployment_revision
        AND deployment.phase =
            source_row.phase
    RETURNING deployment.* INTO result_row;

    PERFORM pg_catalog.set_config(
        'starring.runtime_mutation_clock',
        '',
        TRUE
    );

    IF result_row.deployment_id IS NULL
        OR NOT starring_runtime_private_v2.starring_runtime_product_drain_source_supersession_exact_v2(
            source_row,
            result_row.snapshot,
            drain_row,
            result_deployment_snapshot,
            requested_terminal_time
        )
        OR result_row.revision <> source_row.revision + 1
        OR result_row.phase <> 'superseded'
        OR result_row.updated_at <> requested_terminal_time
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE =
                'runtime_product_drain_supersede_source_result_invalid';
    END IF;

    FOREACH setting_name IN ARRAY ARRAY[
        'starring.runtime_product_drain_supersession_stage_v2',
        'starring.runtime_product_drain_supersession_drain_intent_id_v2',
        'starring.runtime_product_drain_supersession_source_deployment_id_v2',
        'starring.runtime_product_drain_supersession_source_revision_v2',
        'starring.runtime_product_drain_supersession_result_deployment_id_v2',
        'starring.runtime_product_drain_supersession_result_snapshot_v2',
        'starring.runtime_product_drain_supersession_terminal_microseconds_v2'
    ]
    LOOP
        IF COALESCE(
            pg_catalog.current_setting(setting_name, TRUE),
            ''
        ) <> ''
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RX004',
                MESSAGE =
                    'runtime_product_drain_supersede_source_gate_invalid';
        END IF;
    END LOOP;
    RETURN result_row;
END;
$function$;

DO $create_consume_apply_cores$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_product_apply_lock_core_unfenced_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'
    );
    previous_fragment :=
        'CREATE OR REPLACE FUNCTION public.starring_product_apply_lock_core_unfenced_v1(expected_tenant_id text, expected_installation_id text, expected_promotion_id text, expected_product_revision bigint, expected_payload_digest text, expected_principal_id text, expected_product_session_digest bytea, session_subject_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, expected_authority_revision bigint, expected_authority_payload_digest text, expected_authority_observation_digest text, expected_authority_observed_at timestamp with time zone, expected_authority_expires_at timestamp with time zone, expected_effective_permission_bits text, expected_guild_owner boolean, product_request_id text, active_idempotency_key_digest text, idempotency_key_digest_candidates text[], idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[], idempotency_digest_key_id text, semantic_request_digest text, new_receipt_id text, new_audit_event_id text, new_apply_attempt_id text, new_deployment_id text)';
    next_fragment :=
        'CREATE FUNCTION starring_runtime_private_v2.starring_product_apply_consume_lock_core_v2(expected_tenant_id text, expected_installation_id text, expected_promotion_id text, expected_product_revision bigint, expected_payload_digest text, expected_principal_id text, expected_product_session_digest bytea, session_subject_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, expected_authority_revision bigint, expected_authority_payload_digest text, expected_authority_observation_digest text, expected_authority_observed_at timestamp with time zone, expected_authority_expires_at timestamp with time zone, expected_effective_permission_bits text, expected_guild_owner boolean, product_request_id text, active_idempotency_key_digest text, idempotency_key_digest_candidates text[], idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[], idempotency_digest_key_id text, semantic_request_digest text, new_receipt_id text, new_audit_event_id text, new_apply_attempt_id text, new_deployment_id text, expected_source_deployment_id text)';
    IF definition IS NULL
        OR pg_catalog.strpos(definition, previous_fragment) <> 1
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'product_apply_consume_lock_core_header_drift';
    END IF;
    definition := next_fragment
        || pg_catalog.substr(
            definition,
            pg_catalog.length(previous_fragment) + 1
        );

    previous_fragment :=
        '    IF unresolved_deployment_id IS NOT NULL THEN' || E'\n' ||
        '        IF unresolved_deployment_phase IN (''awaiting_gateway_ready'', ''live'') THEN' || E'\n' ||
        '            RETURN QUERY SELECT ''runtime_drain_required'', FALSE, FALSE, NULL::BIGINT,' || E'\n' ||
        '                NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;' || E'\n' ||
        '            RETURN;' || E'\n' ||
        '        END IF;' || E'\n' ||
        '        RETURN QUERY SELECT ''runtime_pending_conflict'', FALSE, FALSE, NULL::BIGINT,' || E'\n' ||
        '            NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;' || E'\n' ||
        '        RETURN;' || E'\n' ||
        '    END IF;';
    next_fragment :=
        '    IF unresolved_deployment_id IS NOT NULL' || E'\n' ||
        '        AND (' || E'\n' ||
        '            unresolved_deployment_id <> expected_source_deployment_id' || E'\n' ||
        '            OR unresolved_deployment_phase NOT IN (' || E'\n' ||
        '                ''awaiting_gateway_ready'',' || E'\n' ||
        '                ''live''' || E'\n' ||
        '            )' || E'\n' ||
        '        )' || E'\n' ||
        '    THEN' || E'\n' ||
        '        RETURN QUERY SELECT CASE' || E'\n' ||
        '                WHEN unresolved_deployment_phase IN (' || E'\n' ||
        '                    ''awaiting_gateway_ready'',' || E'\n' ||
        '                    ''live''' || E'\n' ||
        '                ) THEN ''deployment_mismatch''' || E'\n' ||
        '                ELSE ''runtime_pending_conflict''' || E'\n' ||
        '            END,' || E'\n' ||
        '            FALSE, FALSE, NULL::BIGINT, NULL::TEXT, NULL::TEXT,' || E'\n' ||
        '            NULL::TEXT, NULL::JSONB;' || E'\n' ||
        '        RETURN;' || E'\n' ||
        '    END IF;';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'product_apply_consume_lock_core_branch_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
    EXECUTE definition;

    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)'
    );
    previous_fragment :=
        'CREATE OR REPLACE FUNCTION public.starring_product_apply_finalize_v1(expected_tenant_id text, expected_installation_id text, expected_promotion_id text, expected_product_revision bigint, expected_payload_digest text, expected_principal_id text, expected_product_session_digest bytea, session_subject_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, expected_authority_revision bigint, expected_authority_payload_digest text, expected_authority_observation_digest text, expected_authority_observed_at timestamp with time zone, expected_authority_expires_at timestamp with time zone, expected_effective_permission_bits text, expected_guild_owner boolean, product_request_id text, active_idempotency_key_digest text, idempotency_key_digest_candidates text[], idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[], idempotency_digest_key_id text, semantic_request_digest text, new_receipt_id text, new_audit_event_id text, new_apply_attempt_id text, new_deployment_id text, locked_projection jsonb, prepared_desired_target_digest text, prepared_previous_runtime jsonb, prepared_snapshot jsonb, prepared_activation_notices jsonb)';
    next_fragment :=
        'CREATE FUNCTION starring_runtime_private_v2.starring_product_apply_commit_unfenced_core_v2(expected_tenant_id text, expected_installation_id text, expected_promotion_id text, expected_product_revision bigint, expected_payload_digest text, expected_principal_id text, expected_product_session_digest bytea, session_subject_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, expected_authority_revision bigint, expected_authority_payload_digest text, expected_authority_observation_digest text, expected_authority_observed_at timestamp with time zone, expected_authority_expires_at timestamp with time zone, expected_effective_permission_bits text, expected_guild_owner boolean, product_request_id text, active_idempotency_key_digest text, idempotency_key_digest_candidates text[], idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[], idempotency_digest_key_id text, semantic_request_digest text, new_receipt_id text, new_audit_event_id text, new_apply_attempt_id text, new_deployment_id text, locked_projection jsonb, prepared_desired_target_digest text, prepared_previous_runtime jsonb, prepared_snapshot jsonb, prepared_activation_notices jsonb, requested_mutation_clock timestamp with time zone, requested_manage_slot_fence boolean)';
    IF definition IS NULL
        OR pg_catalog.strpos(definition, previous_fragment) <> 1
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'product_apply_commit_core_header_drift';
    END IF;
    definition := next_fragment
        || pg_catalog.substr(
            definition,
            pg_catalog.length(previous_fragment) + 1
        );

    previous_fragment :=
        'BEGIN' || E'\n' ||
        '    IF pg_catalog.current_setting(''transaction_isolation'') <> ''serializable''' || E'\n' ||
        '        OR pg_catalog.current_setting(''transaction_read_only'') <> ''off''' || E'\n' ||
        '        OR pg_catalog.jsonb_typeof(locked_projection) <> ''object''' || E'\n' ||
        '        OR pg_catalog.octet_length(locked_projection::TEXT) > 1048576' || E'\n' ||
        '        OR pg_catalog.current_setting(' || E'\n' ||
        '            ''starring.product_apply_lock_token_v1'',' || E'\n' ||
        '            TRUE' || E'\n' ||
        '        ) IS DISTINCT FROM ''v1:'' || pg_catalog.md5(locked_projection::TEXT)' || E'\n' ||
        '    THEN' || E'\n' ||
        '        RETURN QUERY SELECT ''lock_required'', NULL::BIGINT, NULL::TEXT, FALSE,' || E'\n' ||
        '            NULL::TEXT, NULL::TEXT, NULL::TEXT;' || E'\n' ||
        '        RETURN;' || E'\n' ||
        '    END IF;' || E'\n' ||
        E'\n' ||
        '    SELECT *' || E'\n' ||
        '    INTO lock_row' || E'\n' ||
        '    FROM public.starring_product_apply_lock_v1(' || E'\n' ||
        '        expected_tenant_id,' || E'\n' ||
        '        expected_installation_id,' || E'\n' ||
        '        expected_promotion_id,' || E'\n' ||
        '        expected_product_revision,' || E'\n' ||
        '        expected_payload_digest,' || E'\n' ||
        '        expected_principal_id,' || E'\n' ||
        '        expected_product_session_digest,' || E'\n' ||
        '        session_subject_digest,' || E'\n' ||
        '        expected_acting_user_id,' || E'\n' ||
        '        expected_discord_application_id,' || E'\n' ||
        '        expected_guild_id,' || E'\n' ||
        '        expected_capability,' || E'\n' ||
        '        expected_authority_revision,' || E'\n' ||
        '        expected_authority_payload_digest,' || E'\n' ||
        '        expected_authority_observation_digest,' || E'\n' ||
        '        expected_authority_observed_at,' || E'\n' ||
        '        expected_authority_expires_at,' || E'\n' ||
        '        expected_effective_permission_bits,' || E'\n' ||
        '        expected_guild_owner,' || E'\n' ||
        '        product_request_id,' || E'\n' ||
        '        active_idempotency_key_digest,' || E'\n' ||
        '        idempotency_key_digest_candidates,' || E'\n' ||
        '        idempotency_digest_key_id_candidates,' || E'\n' ||
        '        idempotency_digest_key_fingerprint_candidates,' || E'\n' ||
        '        idempotency_digest_key_id,' || E'\n' ||
        '        semantic_request_digest,' || E'\n' ||
        '        new_receipt_id,' || E'\n' ||
        '        new_audit_event_id,' || E'\n' ||
        '        new_apply_attempt_id,' || E'\n' ||
        '        new_deployment_id' || E'\n' ||
        '    );' || E'\n' ||
        '    IF lock_row.outcome IS DISTINCT FROM ''ready''' || E'\n' ||
        '        OR lock_row.exact_replay' || E'\n' ||
        '        OR lock_row.locked_projection IS DISTINCT FROM locked_projection' || E'\n' ||
        '    THEN' || E'\n' ||
        '        RETURN QUERY SELECT CASE' || E'\n' ||
        '                WHEN lock_row.outcome = ''ready'' THEN ''locked_projection_mismatch''' || E'\n' ||
        '                ELSE COALESCE(lock_row.outcome, ''indeterminate'')' || E'\n' ||
        '            END,' || E'\n' ||
        '            NULL::BIGINT,' || E'\n' ||
        '            NULL::TEXT,' || E'\n' ||
        '            FALSE,' || E'\n' ||
        '            NULL::TEXT,' || E'\n' ||
        '            NULL::TEXT,' || E'\n' ||
        '            NULL::TEXT;' || E'\n' ||
        '        RETURN;' || E'\n' ||
        '    END IF;';
    next_fragment :=
        'BEGIN' || E'\n' ||
        '    IF pg_catalog.current_setting(''transaction_isolation'') <> ''serializable''' || E'\n' ||
        '        OR pg_catalog.current_setting(''transaction_read_only'') <> ''off''' || E'\n' ||
        '        OR requested_manage_slot_fence IS NULL' || E'\n' ||
        '        OR requested_mutation_clock IS NULL' || E'\n' ||
        '        OR NOT (' || E'\n' ||
        '            requested_mutation_clock = ''-infinity''::TIMESTAMPTZ' || E'\n' ||
        '            OR pg_catalog.isfinite(requested_mutation_clock)' || E'\n' ||
        '        )' || E'\n' ||
        '    THEN' || E'\n' ||
        '        RETURN QUERY SELECT ''lock_required'', NULL::BIGINT, NULL::TEXT, FALSE,' || E'\n' ||
        '            NULL::TEXT, NULL::TEXT, NULL::TEXT;' || E'\n' ||
        '        RETURN;' || E'\n' ||
        '    END IF;';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'product_apply_commit_core_lock_block_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    SELECT slot_fence.writer_epoch' || E'\n' ||
        '    INTO slot_writer_epoch' || E'\n' ||
        '    FROM starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(' || E'\n' ||
        '        expected_guild_id,' || E'\n' ||
        '        locked_projection #>> ''{server,target,ruleset_key}''' || E'\n' ||
        '    ) AS slot_fence;' || E'\n' ||
        E'\n' ||
        '    PERFORM starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(' || E'\n' ||
        '        expected_guild_id,' || E'\n' ||
        '        locked_projection #>> ''{server,target,ruleset_key}'',' || E'\n' ||
        '        slot_writer_epoch' || E'\n' ||
        '    );';
    next_fragment :=
        '    IF requested_manage_slot_fence THEN' || E'\n' ||
        '        SELECT slot_fence.writer_epoch' || E'\n' ||
        '        INTO slot_writer_epoch' || E'\n' ||
        '        FROM starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(' || E'\n' ||
        '            expected_guild_id,' || E'\n' ||
        '            locked_projection #>> ''{server,target,ruleset_key}''' || E'\n' ||
        '        ) AS slot_fence;' || E'\n' ||
        E'\n' ||
        '        PERFORM starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(' || E'\n' ||
        '            expected_guild_id,' || E'\n' ||
        '            locked_projection #>> ''{server,target,ruleset_key}'',' || E'\n' ||
        '            slot_writer_epoch' || E'\n' ||
        '        );' || E'\n' ||
        '    END IF;';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'product_apply_commit_core_fence_block_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    mutation_clock := pg_catalog.clock_timestamp();';
    next_fragment :=
        '    mutation_clock := CASE' || E'\n' ||
        '        WHEN requested_mutation_clock = ''-infinity''::TIMESTAMPTZ' || E'\n' ||
        '        THEN pg_catalog.clock_timestamp()' || E'\n' ||
        '        ELSE requested_mutation_clock' || E'\n' ||
        '    END;';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE = 'product_apply_commit_core_clock_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
    EXECUTE definition;
END;
$create_consume_apply_cores$;

CREATE OR REPLACE FUNCTION public.starring_product_apply_finalize_v1(
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
    locked_projection JSONB,
    prepared_desired_target_digest TEXT,
    prepared_previous_runtime JSONB,
    prepared_snapshot JSONB,
    prepared_activation_notices JSONB
)
RETURNS TABLE(
    outcome TEXT,
    resulting_revision BIGINT,
    resulting_state TEXT,
    exact_replay BOOLEAN,
    guild_id TEXT,
    deployment_id TEXT,
    desired_target_digest TEXT
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
    lock_row RECORD;
BEGIN
    IF pg_catalog.current_setting('transaction_isolation')
            <> 'serializable'
        OR pg_catalog.current_setting('transaction_read_only') <> 'off'
        OR pg_catalog.jsonb_typeof(locked_projection) <> 'object'
        OR pg_catalog.octet_length(locked_projection::TEXT) > 1048576
        OR pg_catalog.current_setting(
            'starring.product_apply_lock_token_v1',
            TRUE
        ) IS DISTINCT FROM
            'v1:' || pg_catalog.md5(locked_projection::TEXT)
    THEN
        RETURN QUERY SELECT
            'lock_required',
            NULL::BIGINT,
            NULL::TEXT,
            FALSE,
            NULL::TEXT,
            NULL::TEXT,
            NULL::TEXT;
        RETURN;
    END IF;

    SELECT apply_lock.*
    INTO lock_row
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
    IF lock_row.outcome IS DISTINCT FROM 'ready'
        OR lock_row.exact_replay
        OR lock_row.locked_projection
            IS DISTINCT FROM locked_projection
    THEN
        RETURN QUERY SELECT
            CASE
                WHEN lock_row.outcome = 'ready'
                    THEN 'locked_projection_mismatch'
                ELSE COALESCE(lock_row.outcome, 'indeterminate')
            END,
            NULL::BIGINT,
            NULL::TEXT,
            FALSE,
            NULL::TEXT,
            NULL::TEXT,
            NULL::TEXT;
        RETURN;
    END IF;

    RETURN QUERY
    SELECT committed.*
    FROM starring_runtime_private_v2.starring_product_apply_commit_unfenced_core_v2(
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
        new_deployment_id,
        locked_projection,
        prepared_desired_target_digest,
        prepared_previous_runtime,
        prepared_snapshot,
        prepared_activation_notices,
        '-infinity'::TIMESTAMPTZ,
        TRUE
    ) AS committed;
END;
$function$;

DO $patch_product_apply_consumed_terminal_replay$
DECLARE
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    SELECT pg_catalog.pg_get_functiondef(function_row.oid)
    INTO definition
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = pg_catalog.to_regprocedure(
        'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'
    );
    previous_fragment :=
        '    postvalidation_outcome TEXT;';
    next_fragment :=
        '    postvalidation_outcome TEXT;' || E'\n' ||
        '    consumed_terminal_replay_outcome TEXT;';
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
                'product_apply_consumed_terminal_replay_declaration_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '            postvalidation_outcome := ''indeterminate'';' || E'\n' ||
        '            RAISE EXCEPTION ''product apply replay evidence changed after core replay''' || E'\n' ||
        '                USING ERRCODE = ''PZ001'';' || E'\n' ||
        '        END IF;' || E'\n' ||
        E'\n' ||
        '        RETURN QUERY SELECT core_row.outcome,';
    next_fragment :=
        '            postvalidation_outcome := ''indeterminate'';' || E'\n' ||
        '            RAISE EXCEPTION ''product apply replay evidence changed after core replay''' || E'\n' ||
        '                USING ERRCODE = ''PZ001'';' || E'\n' ||
        '        END IF;' || E'\n' ||
        E'\n' ||
        '        consumed_terminal_replay_outcome :=' || E'\n' ||
        '            starring_runtime_private_v2.starring_product_apply_consumed_terminal_replay_exact_v2(' || E'\n' ||
        '                expected_tenant_id,' || E'\n' ||
        '                expected_installation_id,' || E'\n' ||
        '                expected_promotion_id,' || E'\n' ||
        '                expected_principal_id,' || E'\n' ||
        '                idempotency_key_digest_candidates,' || E'\n' ||
        '                semantic_request_digest,' || E'\n' ||
        '                expected_payload_digest,' || E'\n' ||
        '                core_row.deployment_id,' || E'\n' ||
        '                core_row.resulting_revision,' || E'\n' ||
        '                receipt_row,' || E'\n' ||
        '                audit_row' || E'\n' ||
        '            );' || E'\n' ||
        '        IF consumed_terminal_replay_outcome IS NULL' || E'\n' ||
        '            OR consumed_terminal_replay_outcome NOT IN (' || E'\n' ||
        '                ''not_correlated'',' || E'\n' ||
        '                ''exact''' || E'\n' ||
        '            )' || E'\n' ||
        '        THEN' || E'\n' ||
        '            postvalidation_outcome := ''persistence_corrupt'';' || E'\n' ||
        '            RAISE EXCEPTION ''product apply consumed terminal replay is corrupt''' || E'\n' ||
        '                USING ERRCODE = ''PZ001'';' || E'\n' ||
        '        END IF;' || E'\n' ||
        E'\n' ||
        '        RETURN QUERY SELECT core_row.outcome,';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'product_apply_consumed_terminal_replay_branch_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$patch_product_apply_consumed_terminal_replay$;

CREATE FUNCTION starring_runtime_private_v2.starring_product_apply_consume_preparation_reservation_v2(
    requested_phase TEXT,
    requested_preparation_token TEXT,
    requested_binding_digest TEXT,
    requested_locked_projection_digest TEXT,
    requested_terminal_database_time TIMESTAMPTZ
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
    reservation_relation OID;
    relation_owner OID;
    relation_namespace OID;
    relation_kind "char";
    relation_persistence "char";
    invalid_column_count BIGINT;
    invalid_acl_count BIGINT;
    reservation_count BIGINT;
    consumed_token TEXT;
    consumed_backend_pid INTEGER;
    consumed_transaction_id BIGINT;
    consumed_binding_digest TEXT;
    consumed_locked_projection_digest TEXT;
    consumed_terminal_database_time TIMESTAMPTZ;
BEGIN
    IF requested_phase NOT IN ('prepare', 'commit')
        OR requested_preparation_token !~ '^v2:[0-9a-f]{64}$'
        OR requested_binding_digest !~ '^[0-9a-f]{64}$'
        OR requested_locked_projection_digest !~ '^[0-9a-f]{64}$'
        OR NOT pg_catalog.isfinite(
            requested_terminal_database_time
        )
    THEN
        RETURN FALSE;
    END IF;

    reservation_relation := pg_catalog.to_regclass(
        'pg_temp.starring_product_apply_consume_preparation_reservations_v2'
    );
    IF reservation_relation IS NULL
        AND requested_phase = 'prepare'
    THEN
        EXECUTE
            'CREATE TEMPORARY TABLE pg_temp.starring_product_apply_consume_preparation_reservations_v2 (
                preparation_token TEXT NOT NULL,
                backend_pid INTEGER NOT NULL,
                transaction_id BIGINT NOT NULL,
                binding_digest TEXT NOT NULL,
                locked_projection_digest TEXT NOT NULL,
                terminal_database_time TIMESTAMPTZ NOT NULL
            ) ON COMMIT DELETE ROWS';
        EXECUTE
            'REVOKE ALL PRIVILEGES ON TABLE pg_temp.starring_product_apply_consume_preparation_reservations_v2 FROM PUBLIC';
        reservation_relation := pg_catalog.to_regclass(
            'pg_temp.starring_product_apply_consume_preparation_reservations_v2'
        );
    END IF;
    IF reservation_relation IS NULL THEN
        RETURN FALSE;
    END IF;

    SELECT
        relation.relowner,
        relation.relnamespace,
        relation.relkind,
        relation.relpersistence
    INTO
        relation_owner,
        relation_namespace,
        relation_kind,
        relation_persistence
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = reservation_relation;

    WITH expected(
        attribute_number,
        attribute_name,
        attribute_type,
        attribute_not_null
    ) AS (
        VALUES
            (1, 'preparation_token', 'text', TRUE),
            (2, 'backend_pid', 'integer', TRUE),
            (3, 'transaction_id', 'bigint', TRUE),
            (4, 'binding_digest', 'text', TRUE),
            (5, 'locked_projection_digest', 'text', TRUE),
            (
                6,
                'terminal_database_time',
                'timestamp with time zone',
                TRUE
            )
    ), observed AS (
        SELECT
            attribute.attnum::INTEGER AS attribute_number,
            attribute.attname::TEXT AS attribute_name,
            pg_catalog.format_type(
                attribute.atttypid,
                attribute.atttypmod
            ) AS attribute_type,
            attribute.attnotnull AS attribute_not_null
        FROM pg_catalog.pg_attribute AS attribute
        WHERE attribute.attrelid = reservation_relation
            AND attribute.attnum > 0
            AND NOT attribute.attisdropped
    )
    SELECT pg_catalog.count(*)
    INTO invalid_column_count
    FROM expected
    FULL JOIN observed USING (attribute_number)
    WHERE expected.attribute_name
            IS DISTINCT FROM observed.attribute_name
        OR expected.attribute_type
            IS DISTINCT FROM observed.attribute_type
        OR expected.attribute_not_null
            IS DISTINCT FROM observed.attribute_not_null;

    SELECT pg_catalog.count(*)
    INTO invalid_acl_count
    FROM pg_catalog.pg_class AS relation
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        relation.relacl,
        pg_catalog.acldefault('r', relation.relowner)
    )) AS privilege
    WHERE relation.oid = reservation_relation
        AND privilege.grantee <> relation.relowner;

    IF relation_owner IS DISTINCT FROM
            pg_catalog.to_regrole(current_user)
        OR relation_namespace IS DISTINCT FROM
            pg_catalog.pg_my_temp_schema()
        OR relation_kind IS DISTINCT FROM 'r'::"char"
        OR relation_persistence IS DISTINCT FROM 't'::"char"
        OR invalid_column_count <> 0
        OR invalid_acl_count <> 0
    THEN
        RETURN FALSE;
    END IF;

    EXECUTE
        'SELECT pg_catalog.count(*) FROM pg_temp.starring_product_apply_consume_preparation_reservations_v2'
    INTO reservation_count;

    IF requested_phase = 'prepare' THEN
        IF reservation_count <> 0 THEN
            RETURN FALSE;
        END IF;
        EXECUTE
            'INSERT INTO pg_temp.starring_product_apply_consume_preparation_reservations_v2 (
                preparation_token,
                backend_pid,
                transaction_id,
                binding_digest,
                locked_projection_digest,
                terminal_database_time
            ) VALUES ($1, $2, $3, $4, $5, $6)'
        USING
            requested_preparation_token,
            pg_catalog.pg_backend_pid(),
            pg_catalog.txid_current(),
            requested_binding_digest,
            requested_locked_projection_digest,
            requested_terminal_database_time;
        RETURN TRUE;
    END IF;

    IF reservation_count <> 1 THEN
        RETURN FALSE;
    END IF;
    EXECUTE
        'DELETE FROM pg_temp.starring_product_apply_consume_preparation_reservations_v2
         WHERE backend_pid = $1
            AND transaction_id = $2
         RETURNING
            preparation_token,
            backend_pid,
            transaction_id,
            binding_digest,
            locked_projection_digest,
            terminal_database_time'
    INTO
        consumed_token,
        consumed_backend_pid,
        consumed_transaction_id,
        consumed_binding_digest,
        consumed_locked_projection_digest,
        consumed_terminal_database_time
    USING
        pg_catalog.pg_backend_pid(),
        pg_catalog.txid_current();

    EXECUTE
        'SELECT pg_catalog.count(*) FROM pg_temp.starring_product_apply_consume_preparation_reservations_v2'
    INTO reservation_count;
    RETURN reservation_count = 0
        AND consumed_token = requested_preparation_token
        AND consumed_backend_pid = pg_catalog.pg_backend_pid()
        AND consumed_transaction_id = pg_catalog.txid_current()
        AND consumed_binding_digest = requested_binding_digest
        AND consumed_locked_projection_digest =
            requested_locked_projection_digest
        AND consumed_terminal_database_time =
            requested_terminal_database_time;
EXCEPTION
    WHEN OTHERS THEN
        RETURN FALSE;
END;
$function$;

CREATE FUNCTION public.starring_product_apply_consume_runtime_drain_v2(
    requested_phase TEXT,
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
    expected_drain_intent_id TEXT,
    expected_source_intent_revision BIGINT,
    expected_source_state_bytes BYTEA,
    expected_source_state_digest TEXT,
    expected_product_operation_id TEXT,
    expected_source_deployment_id TEXT,
    expected_source_deployment_revision BIGINT,
    proposed_terminal_action_id TEXT,
    expected_preparation_token TEXT,
    prepared_source_result_snapshot_bytes BYTEA,
    prepared_source_result_snapshot_digest TEXT,
    prepared_result_deployment_snapshot_bytes BYTEA,
    prepared_result_deployment_snapshot_digest TEXT,
    prepared_desired_target_digest TEXT,
    prepared_activation_notices_bytes BYTEA
)
RETURNS TABLE(
    outcome_name TEXT,
    preparation_ready BOOLEAN,
    exact_replay BOOLEAN,
    requires_commit BOOLEAN,
    preparation_token TEXT,
    locked_product_projection JSONB,
    source_deployment_snapshot JSONB,
    source_acknowledged_at TIMESTAMPTZ,
    product_operation_id TEXT,
    product_mutation_digest TEXT,
    drain_intent_digest TEXT,
    source_deployment_id TEXT,
    source_deployment_revision BIGINT,
    source_result_deployment_revision BIGINT,
    source_result_deployment_snapshot JSONB,
    source_result_deployment_snapshot_digest TEXT,
    result_deployment_id TEXT,
    result_deployment_revision BIGINT,
    result_deployment_snapshot JSONB,
    result_deployment_snapshot_digest TEXT,
    product_resulting_revision BIGINT,
    product_resulting_state TEXT,
    product_receipt_id TEXT,
    product_audit_event_id TEXT,
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
    optimistic_source_row public.runtime_deployments%ROWTYPE;
    locked_source_row public.runtime_deployments%ROWTYPE;
    root_source_row public.runtime_deployments%ROWTYPE;
    source_result_row public.runtime_deployments%ROWTYPE;
    current_result_row public.runtime_deployments%ROWTYPE;
    product_row public.runtime_product_operations_v2%ROWTYPE;
    drain_row public.runtime_drain_intents_v2%ROWTYPE;
    acknowledged_drain_row public.runtime_drain_intents_v2%ROWTYPE;
    terminal_drain_row public.runtime_drain_intents_v2%ROWTYPE;
    slot_fence_row public.runtime_slot_writer_fences_v2%ROWTYPE;
    serving_row public.runtime_serving_leases%ROWTYPE;
    action_row public.runtime_product_drain_terminal_actions_v2%ROWTYPE;
    conflicting_action_row public.runtime_product_drain_terminal_actions_v2%ROWTYPE;
    receipt_row public.product_action_receipts%ROWTYPE;
    audit_row public.product_audit_events%ROWTYPE;
    apply_lock_row RECORD;
    apply_commit_row RECORD;
    source_state_value JSONB;
    source_certification JSONB;
    prepared_source_result_snapshot JSONB;
    prepared_result_deployment_snapshot JSONB;
    prepared_activation_notices JSONB;
    acknowledged_microseconds NUMERIC;
    acknowledged_time TIMESTAMPTZ;
    terminal_microseconds NUMERIC;
    terminal_time TIMESTAMPTZ;
    computed_preparation_token TEXT;
    preparation_binding_digest TEXT;
    locked_projection_digest TEXT;
    computed_terminal_projection BYTEA;
    computed_terminal_projection_digest TEXT;
    computed_successor_epoch BIGINT;
    certification_operation_count BIGINT;
    certification_terminal_count BIGINT;
    unresolved_count BIGINT;
    action_count BIGINT;
    core_is_replay BOOLEAN;
    requested_is_prepare BOOLEAN;
    preparation_reservation_valid BOOLEAN;
BEGIN
    requested_is_prepare := requested_phase = 'prepare';
    IF pg_catalog.current_setting('transaction_isolation')
            <> 'serializable'
        OR pg_catalog.current_setting('transaction_read_only') <> 'off'
        OR requested_phase NOT IN ('prepare', 'commit')
        OR expected_drain_intent_id !~ '^[0-9a-f]{32}$'
        OR expected_source_intent_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR pg_catalog.octet_length(expected_source_state_bytes)
            NOT BETWEEN 1 AND 1048576
        OR expected_source_state_digest !~ '^[0-9a-f]{64}$'
        OR expected_source_state_digest <>
            pg_catalog.encode(
                pg_catalog.sha256(expected_source_state_bytes),
                'hex'
            )
        OR expected_product_operation_id !~ '^[0-9a-f]{32}$'
        OR expected_source_deployment_id
            !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_source_deployment_revision
            NOT BETWEEN 1 AND 9223372036854775806
        OR proposed_terminal_action_id !~ '^[0-9a-f]{64}$'
        OR expected_capability <> 'apply'
        OR (
            requested_is_prepare
            AND (
                expected_preparation_token <> ''
                OR prepared_source_result_snapshot_bytes <> ''::BYTEA
                OR prepared_source_result_snapshot_digest <> ''
                OR prepared_result_deployment_snapshot_bytes <> ''::BYTEA
                OR prepared_result_deployment_snapshot_digest <> ''
                OR prepared_desired_target_digest <> ''
                OR prepared_activation_notices_bytes <> ''::BYTEA
            )
        )
        OR (
            NOT requested_is_prepare
            AND (
                expected_preparation_token !~ '^v2:[0-9a-f]{64}$'
                OR pg_catalog.octet_length(
                    prepared_source_result_snapshot_bytes
                ) NOT BETWEEN 32 AND 262144
                OR prepared_source_result_snapshot_digest
                    !~ '^[0-9a-f]{64}$'
                OR prepared_source_result_snapshot_digest <>
                    pg_catalog.encode(
                        pg_catalog.sha256(
                            prepared_source_result_snapshot_bytes
                        ),
                        'hex'
                    )
                OR pg_catalog.octet_length(
                    prepared_result_deployment_snapshot_bytes
                ) NOT BETWEEN 32 AND 262144
                OR prepared_result_deployment_snapshot_digest
                    !~ '^[0-9a-f]{64}$'
                OR prepared_result_deployment_snapshot_digest <>
                    pg_catalog.encode(
                        pg_catalog.sha256(
                            prepared_result_deployment_snapshot_bytes
                        ),
                        'hex'
                    )
                OR prepared_desired_target_digest
                    !~ '^[0-9a-f]{64}$'
                OR pg_catalog.octet_length(
                    prepared_activation_notices_bytes
                ) NOT BETWEEN 2 AND 16384
            )
        )
    THEN
        outcome_name := 'invalid_input';
        preparation_ready := FALSE;
        exact_replay := FALSE;
        requires_commit := FALSE;
        RETURN NEXT;
        RETURN;
    END IF;

    BEGIN
        source_state_value := pg_catalog.convert_from(
            expected_source_state_bytes,
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
        IF NOT requested_is_prepare THEN
            prepared_source_result_snapshot :=
                pg_catalog.convert_from(
                    prepared_source_result_snapshot_bytes,
                    'UTF8'
                )::JSONB;
            prepared_result_deployment_snapshot :=
                pg_catalog.convert_from(
                    prepared_result_deployment_snapshot_bytes,
                    'UTF8'
                )::JSONB;
            prepared_activation_notices :=
                pg_catalog.convert_from(
                    prepared_activation_notices_bytes,
                    'UTF8'
                )::JSONB;
        END IF;
    EXCEPTION
        WHEN OTHERS THEN
            outcome_name := 'invalid_input';
            preparation_ready := FALSE;
            exact_replay := FALSE;
            requires_commit := FALSE;
            RETURN NEXT;
            RETURN;
    END;

    IF acknowledged_microseconds NOT BETWEEN
            -62135596800000000 AND 253402300799999999
        OR acknowledged_microseconds <>
            pg_catalog.trunc(acknowledged_microseconds)
        OR pg_catalog.jsonb_typeof(source_certification)
            <> 'object'
        OR (
            NOT requested_is_prepare
            AND (
                pg_catalog.jsonb_typeof(
                    prepared_source_result_snapshot
                ) <> 'object'
                OR pg_catalog.jsonb_typeof(
                    prepared_result_deployment_snapshot
                ) <> 'object'
                OR pg_catalog.jsonb_typeof(
                    prepared_activation_notices
                ) <> 'array'
            )
        )
    THEN
        outcome_name := 'invalid_input';
        preparation_ready := FALSE;
        exact_replay := FALSE;
        requires_commit := FALSE;
        RETURN NEXT;
        RETURN;
    END IF;

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
        preparation_ready := FALSE;
        exact_replay := FALSE;
        requires_commit := FALSE;
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT deployment.*
    INTO optimistic_source_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.deployment_id =
            expected_source_deployment_id;
    IF NOT FOUND
        OR optimistic_source_row.tenant_id <>
            expected_tenant_id
        OR optimistic_source_row.installation_id <>
            expected_installation_id
        OR optimistic_source_row.guild_id <> expected_guild_id
    THEN
        outcome_name := 'scope_mismatch';
        preparation_ready := FALSE;
        exact_replay := FALSE;
        requires_commit := FALSE;
        RETURN NEXT;
        RETURN;
    END IF;

    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended(
            pg_catalog.concat(
                'starring-runtime-serving-slot-v1:',
                optimistic_source_row.guild_id,
                ':',
                optimistic_source_row.ruleset_key
            ),
            0
        )
    );
    PERFORM fence.writer_epoch
    FROM starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(
        optimistic_source_row.guild_id,
        optimistic_source_row.ruleset_key
    ) AS fence;
    IF NOT FOUND THEN
        outcome_name := 'persistence_corrupt';
        preparation_ready := FALSE;
        exact_replay := FALSE;
        requires_commit := FALSE;
        RETURN NEXT;
        RETURN;
    END IF;
    SELECT fence.*
    INTO slot_fence_row
    FROM public.runtime_slot_writer_fences_v2 AS fence
    WHERE fence.slot_guild_id =
            optimistic_source_row.guild_id
        AND fence.slot_ruleset_key =
            optimistic_source_row.ruleset_key
    FOR UPDATE;
    IF NOT FOUND THEN
        outcome_name := 'persistence_corrupt';
        preparation_ready := FALSE;
        exact_replay := FALSE;
        requires_commit := FALSE;
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT serving.*
    INTO serving_row
    FROM public.runtime_serving_leases AS serving
    WHERE serving.guild_id = optimistic_source_row.guild_id
        AND serving.ruleset_key =
            optimistic_source_row.ruleset_key
    FOR UPDATE;

    PERFORM deployment.deployment_id
    FROM public.runtime_deployments AS deployment
    WHERE deployment.guild_id = optimistic_source_row.guild_id
        AND deployment.ruleset_key =
            optimistic_source_row.ruleset_key
    ORDER BY
        deployment.runtime_generation,
        deployment.deployment_id
    FOR UPDATE;

    SELECT deployment.*
    INTO locked_source_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.deployment_id =
            expected_source_deployment_id
    FOR UPDATE;
    IF NOT FOUND
        OR locked_source_row.tenant_id <>
            optimistic_source_row.tenant_id
        OR locked_source_row.installation_id <>
            optimistic_source_row.installation_id
        OR locked_source_row.guild_id <>
            optimistic_source_row.guild_id
        OR locked_source_row.ruleset_key <>
            optimistic_source_row.ruleset_key
    THEN
        outcome_name := 'revision_conflict';
        preparation_ready := FALSE;
        exact_replay := FALSE;
        requires_commit := FALSE;
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT core.*
    INTO apply_lock_row
    FROM starring_runtime_private_v2.starring_product_apply_consume_lock_core_v2(
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
        new_deployment_id,
        expected_source_deployment_id
    ) AS core;
    core_is_replay :=
        apply_lock_row.outcome = 'ok'
        AND apply_lock_row.exact_replay IS TRUE
        AND apply_lock_row.requires_commit IS TRUE;
    IF NOT core_is_replay
        AND (
            apply_lock_row.outcome <> 'ready'
            OR apply_lock_row.exact_replay IS NOT FALSE
            OR apply_lock_row.requires_commit IS NOT FALSE
            OR apply_lock_row.locked_projection IS NULL
        )
    THEN
        outcome_name := CASE apply_lock_row.outcome
            WHEN 'runtime_writer_fenced' THEN 'writer_fenced'
            WHEN 'authorization_stale' THEN 'authorization_stale'
            WHEN 'scope_mismatch' THEN 'scope_mismatch'
            WHEN 'revision_conflict' THEN 'revision_conflict'
            WHEN 'idempotency_conflict' THEN 'idempotency_conflict'
            WHEN 'payload_mismatch' THEN 'idempotency_conflict'
            WHEN 'expired' THEN 'authorization_stale'
            WHEN 'invalid_state' THEN 'revision_conflict'
            WHEN 'baseline_mismatch' THEN 'revision_conflict'
            WHEN 'runtime_pending_conflict' THEN 'revision_conflict'
            WHEN 'runtime_generation_conflict' THEN 'revision_conflict'
            WHEN 'runtime_generation_overflow' THEN 'revision_conflict'
            WHEN 'projection_too_large' THEN 'persistence_corrupt'
            WHEN 'indeterminate' THEN 'indeterminate'
            ELSE 'persistence_corrupt'
        END;
        preparation_ready := FALSE;
        exact_replay := FALSE;
        requires_commit := FALSE;
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
        preparation_ready := FALSE;
        exact_replay := FALSE;
        requires_commit := FALSE;
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
        preparation_ready := FALSE;
        exact_replay := FALSE;
        requires_commit := FALSE;
        RETURN NEXT;
        RETURN;
    END IF;

    acknowledged_drain_row := drain_row;
    acknowledged_drain_row.intent_revision :=
        expected_source_intent_revision;
    acknowledged_drain_row.intent_state :=
        'route_absent_acknowledged';
    acknowledged_drain_row.canonical_state_bytes :=
        expected_source_state_bytes;
    acknowledged_drain_row.canonical_state_digest :=
        expected_source_state_digest;
    root_source_row := locked_source_row;
    root_source_row.revision :=
        expected_source_deployment_revision;

    IF NOT starring_runtime_private_v2.starring_runtime_product_drain_consume_root_exact_v2(
        product_row,
        acknowledged_drain_row,
        root_source_row,
        expected_product_operation_id,
        expected_drain_intent_id,
        expected_source_intent_revision,
        expected_source_state_bytes,
        expected_source_state_digest,
        semantic_request_digest
    )
    THEN
        outcome_name := 'persistence_corrupt';
        preparation_ready := FALSE;
        exact_replay := FALSE;
        requires_commit := FALSE;
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

    SELECT pg_catalog.count(*)
    INTO certification_operation_count
    FROM public.runtime_certification_operations_v2 AS reservation
    WHERE reservation.tenant_id = drain_row.tenant_id
        AND reservation.installation_id =
            drain_row.installation_id
        AND reservation.deployment_id =
            drain_row.deployment_id
        AND reservation.deployment_revision =
            drain_row.expected_revision;
    SELECT pg_catalog.count(*)
    INTO certification_terminal_count
    FROM public.runtime_certification_operations_v2 AS reservation
    INNER JOIN public.runtime_certification_operation_terminals_v2 AS terminal
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
        AND terminal.terminal_outcome_name = 'awaiting_reset';
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
        preparation_ready := FALSE;
        exact_replay := FALSE;
        requires_commit := FALSE;
        RETURN NEXT;
        RETURN;
    END IF;

    PERFORM action.terminal_action_id
    FROM public.runtime_product_drain_terminal_actions_v2 AS action
    WHERE action.terminal_action_id =
            proposed_terminal_action_id
        OR action.drain_intent_id =
            expected_drain_intent_id
        OR (
            action.terminal_kind = 'consumed'
            AND action.product_action_idempotency_digest =
                active_idempotency_key_digest
        )
    ORDER BY action.terminal_action_id
    FOR UPDATE;

    SELECT pg_catalog.count(*)
    INTO action_count
    FROM public.runtime_product_drain_terminal_actions_v2 AS action
    WHERE action.terminal_action_id =
            proposed_terminal_action_id
        OR action.drain_intent_id =
            expected_drain_intent_id
        OR (
            action.terminal_kind = 'consumed'
            AND action.product_action_idempotency_digest =
                active_idempotency_key_digest
        );

    IF action_count > 1 THEN
        outcome_name := 'persistence_corrupt';
        preparation_ready := FALSE;
        exact_replay := FALSE;
        requires_commit := FALSE;
        RETURN NEXT;
        RETURN;
    END IF;

    IF action_count = 1 THEN
        SELECT action.*
        INTO STRICT conflicting_action_row
        FROM public.runtime_product_drain_terminal_actions_v2 AS action
        WHERE action.terminal_action_id =
                proposed_terminal_action_id
            OR action.drain_intent_id =
                expected_drain_intent_id
            OR (
                action.terminal_kind = 'consumed'
                AND action.product_action_idempotency_digest =
                    active_idempotency_key_digest
            );
        action_row := conflicting_action_row;
        IF action_row.terminal_kind = 'cancelled'
            AND action_row.drain_intent_id =
                expected_drain_intent_id
        THEN
            outcome_name := 'cancelled';
            preparation_ready := FALSE;
            exact_replay := FALSE;
            requires_commit := FALSE;
            RETURN NEXT;
            RETURN;
        END IF;
        IF action_row.terminal_kind <> 'consumed'
            OR action_row.terminal_action_id <>
                proposed_terminal_action_id
            OR action_row.drain_intent_id <>
                expected_drain_intent_id
            OR action_row.product_operation_id <>
                expected_product_operation_id
            OR action_row.product_action_idempotency_digest <>
                active_idempotency_key_digest
            OR action_row.product_action_semantic_request_digest <>
                semantic_request_digest
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
                    active_idempotency_key_digest
                    THEN 'idempotency_conflict'
                ELSE 'terminal_conflict'
            END;
            preparation_ready := FALSE;
            exact_replay := FALSE;
            requires_commit := FALSE;
            RETURN NEXT;
            RETURN;
        END IF;

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
        SELECT deployment.*
        INTO current_result_row
        FROM public.runtime_deployments AS deployment
        WHERE deployment.deployment_id =
                action_row.result_deployment_id
        FOR UPDATE;

        IF drain_row.intent_state <> 'consumed'
            OR NOT starring_runtime_private_v2.starring_runtime_product_drain_terminal_action_exact_v2(
                action_row,
                product_row,
                drain_row
            )
            OR receipt_row.receipt_id IS NULL
            OR receipt_row.tenant_id <> expected_tenant_id
            OR receipt_row.installation_id <>
                expected_installation_id
            OR receipt_row.principal_id <> expected_principal_id
            OR receipt_row.endpoint_domain <> 'product_apply_v1'
            OR receipt_row.idempotency_key_digest <>
                active_idempotency_key_digest
            OR receipt_row.request_digest <>
                semantic_request_digest
            OR receipt_row.target_resource_type <>
                'authoring_promotion'
            OR receipt_row.target_resource_id <>
                expected_promotion_id
            OR receipt_row.resulting_revision IS DISTINCT FROM
                expected_product_revision + 2
            OR receipt_row.resulting_state <> 'applied'
            OR receipt_row.result_code <> 'runtime_requested'
            OR receipt_row.completed_at <>
                action_row.terminal_database_time
            OR audit_row.event_id IS NULL
            OR audit_row.receipt_id <> receipt_row.receipt_id
            OR audit_row.tenant_id <> expected_tenant_id
            OR audit_row.installation_id <>
                expected_installation_id
            OR audit_row.principal_id <> expected_principal_id
            OR audit_row.action <> 'promotion.apply'
            OR audit_row.request_id <> product_request_id
            OR audit_row.authority_observation_digest <>
                expected_authority_observation_digest
            OR audit_row.installation_authority_revision <>
                expected_authority_revision
            OR audit_row.occurred_at <>
                action_row.terminal_database_time
            OR locked_source_row.phase <> 'superseded'
            OR locked_source_row.revision <
                action_row.source_result_deployment_revision
            OR current_result_row.deployment_id IS NULL
            OR current_result_row.tenant_id <>
                expected_tenant_id
            OR current_result_row.installation_id <>
                expected_installation_id
            OR current_result_row.guild_id <> expected_guild_id
            OR current_result_row.ruleset_key <>
                locked_source_row.ruleset_key
            OR current_result_row.revision <
                action_row.result_deployment_revision
            OR current_result_row.runtime_generation <=
                locked_source_row.runtime_generation
            OR slot_fence_row.writer_epoch <
                action_row.successor_slot_writer_epoch
        THEN
            outcome_name := 'persistence_corrupt';
            preparation_ready := FALSE;
            exact_replay := FALSE;
            requires_commit := FALSE;
            RETURN NEXT;
            RETURN;
        END IF;

        outcome_name := 'replayed';
        preparation_ready := FALSE;
        exact_replay := TRUE;
        requires_commit := FALSE;
        preparation_token := NULL;
        locked_product_projection := NULL;
        source_deployment_snapshot := NULL;
        source_acknowledged_at := acknowledged_time;
        product_operation_id :=
            action_row.product_operation_id;
        product_mutation_digest :=
            action_row.product_mutation_digest;
        drain_intent_digest :=
            action_row.drain_intent_digest;
        source_deployment_id :=
            expected_source_deployment_id;
        source_deployment_revision :=
            action_row.source_deployment_revision;
        source_result_deployment_revision :=
            action_row.source_result_deployment_revision;
        source_result_deployment_snapshot :=
            pg_catalog.convert_from(
                action_row.source_result_deployment_snapshot_bytes,
                'UTF8'
            )::JSONB;
        source_result_deployment_snapshot_digest :=
            action_row.source_result_deployment_snapshot_digest;
        result_deployment_id :=
            action_row.result_deployment_id;
        result_deployment_revision :=
            action_row.result_deployment_revision;
        result_deployment_snapshot :=
            pg_catalog.convert_from(
                action_row.result_deployment_snapshot_bytes,
                'UTF8'
            )::JSONB;
        result_deployment_snapshot_digest :=
            action_row.result_deployment_snapshot_digest;
        product_resulting_revision :=
            receipt_row.resulting_revision;
        product_resulting_state :=
            receipt_row.resulting_state;
        product_receipt_id := receipt_row.receipt_id;
        product_audit_event_id := audit_row.event_id;
        drain_intent_id := drain_row.drain_intent_id;
        source_intent_revision :=
            action_row.source_intent_revision;
        source_state_bytes :=
            expected_source_state_bytes;
        source_state_digest :=
            action_row.source_canonical_state_digest;
        result_intent_revision :=
            action_row.result_intent_revision;
        result_intent_state := drain_row.intent_state;
        result_state_bytes := drain_row.canonical_state_bytes;
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

    IF core_is_replay
        OR drain_row.intent_state IN ('consumed', 'cancelled')
    THEN
        outcome_name := 'persistence_corrupt';
        preparation_ready := FALSE;
        exact_replay := FALSE;
        requires_commit := FALSE;
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT pg_catalog.count(*)
    INTO unresolved_count
    FROM public.runtime_deployments AS deployment
    WHERE deployment.guild_id = locked_source_row.guild_id
        AND deployment.ruleset_key =
            locked_source_row.ruleset_key
        AND deployment.phase NOT IN ('superseded', 'cancelled')
        AND deployment.deployment_id <>
            locked_source_row.deployment_id;
    IF writer_fence_state <> 'open' THEN
        outcome_name := 'writer_fenced';
        preparation_ready := FALSE;
        exact_replay := FALSE;
        requires_commit := FALSE;
        RETURN NEXT;
        RETURN;
    END IF;
    IF locked_source_row.revision <>
            expected_source_deployment_revision
        OR locked_source_row.phase NOT IN (
            'awaiting_gateway_ready',
            'live'
        )
        OR locked_source_row.controller_id IS NOT NULL
        OR locked_source_row.controller_fencing_token IS NOT NULL
        OR locked_source_row.controller_acquired_at IS NOT NULL
        OR locked_source_row.controller_lease_expires_at IS NOT NULL
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
            expected_source_deployment_id
        OR slot_fence_row.pending_expected_revision <>
            expected_source_deployment_revision
        OR slot_fence_row.writer_epoch
            NOT BETWEEN 1 AND 9223372036854775806
        OR drain_row.intent_state <>
            'route_absent_acknowledged'
        OR drain_row.intent_revision <>
            expected_source_intent_revision
        OR drain_row.canonical_state_bytes <>
            expected_source_state_bytes
        OR drain_row.canonical_state_digest <>
            expected_source_state_digest
    THEN
        outcome_name := 'revision_conflict';
        preparation_ready := FALSE;
        exact_replay := FALSE;
        requires_commit := FALSE;
        RETURN NEXT;
        RETURN;
    END IF;

    terminal_time := pg_catalog.date_trunc(
        'microseconds',
        GREATEST(
            pg_catalog.transaction_timestamp(),
            acknowledged_time,
            locked_source_row.updated_at + INTERVAL '1 microsecond',
            slot_fence_row.updated_at + INTERVAL '1 microsecond',
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
    THEN
        outcome_name := 'persistence_corrupt';
        preparation_ready := FALSE;
        exact_replay := FALSE;
        requires_commit := FALSE;
        RETURN NEXT;
        RETURN;
    END IF;

    locked_projection_digest := pg_catalog.encode(
        pg_catalog.sha256(
            pg_catalog.convert_to(
                apply_lock_row.locked_projection::TEXT,
                'UTF8'
            )
        ),
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
                    pg_catalog.encode(
                        expected_product_session_digest,
                        'hex'
                    ),
                    pg_catalog.encode(
                        session_subject_digest,
                        'hex'
                    ),
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
                    pg_catalog.to_jsonb(
                        idempotency_key_digest_candidates
                    ),
                    pg_catalog.to_jsonb(
                        idempotency_digest_key_id_candidates
                    ),
                    pg_catalog.to_jsonb(
                        idempotency_digest_key_fingerprint_candidates
                    ),
                    idempotency_digest_key_id,
                    semantic_request_digest,
                    new_receipt_id,
                    new_audit_event_id,
                    new_apply_attempt_id,
                    new_deployment_id,
                    expected_drain_intent_id,
                    expected_source_intent_revision,
                    expected_source_state_digest,
                    expected_product_operation_id,
                    expected_source_deployment_id,
                    expected_source_deployment_revision,
                    proposed_terminal_action_id,
                    slot_fence_row.writer_epoch,
                    terminal_microseconds::BIGINT,
                    locked_projection_digest
                )::TEXT,
                'UTF8'
            )
        ),
        'hex'
    );
    computed_preparation_token :=
        'v2:' || preparation_binding_digest;

    IF requested_is_prepare THEN
        IF NOT starring_runtime_private_v2.starring_product_apply_consume_preparation_reservation_v2(
            'prepare',
            computed_preparation_token,
            preparation_binding_digest,
            locked_projection_digest,
            terminal_time
        )
        THEN
            outcome_name := 'indeterminate';
            preparation_ready := FALSE;
            exact_replay := FALSE;
            requires_commit := FALSE;
            RETURN NEXT;
            RETURN;
        END IF;
        outcome_name := 'drain_pending';
        preparation_ready := TRUE;
        exact_replay := FALSE;
        requires_commit := TRUE;
        preparation_token := computed_preparation_token;
        locked_product_projection :=
            apply_lock_row.locked_projection;
        source_deployment_snapshot :=
            locked_source_row.snapshot;
        source_acknowledged_at := acknowledged_time;
        product_operation_id :=
            product_row.product_operation_id;
        product_mutation_digest :=
            product_row.product_mutation_digest;
        drain_intent_digest :=
            drain_row.drain_intent_digest;
        source_deployment_id :=
            locked_source_row.deployment_id;
        source_deployment_revision :=
            locked_source_row.revision;
        product_receipt_id := new_receipt_id;
        product_audit_event_id := new_audit_event_id;
        drain_intent_id := drain_row.drain_intent_id;
        source_intent_revision := drain_row.intent_revision;
        source_state_bytes := drain_row.canonical_state_bytes;
        source_state_digest := drain_row.canonical_state_digest;
        source_slot_epoch := slot_fence_row.writer_epoch;
        terminal_action_id := proposed_terminal_action_id;
        terminal_database_time := terminal_time;
        RETURN NEXT;
        RETURN;
    END IF;

    preparation_reservation_valid :=
        starring_runtime_private_v2.starring_product_apply_consume_preparation_reservation_v2(
            'commit',
            expected_preparation_token,
            preparation_binding_digest,
            locked_projection_digest,
            terminal_time
        );
    IF NOT preparation_reservation_valid
        OR computed_preparation_token IS DISTINCT FROM
            expected_preparation_token
    THEN
        outcome_name := 'indeterminate';
        preparation_ready := FALSE;
        exact_replay := FALSE;
        requires_commit := FALSE;
        RETURN NEXT;
        RETURN;
    END IF;

    BEGIN
        SELECT source.*
        INTO STRICT source_result_row
        FROM starring_runtime_private_v2.starring_runtime_product_drain_supersede_source_v2(
            expected_drain_intent_id,
            expected_source_deployment_id,
            expected_source_deployment_revision,
            prepared_source_result_snapshot_bytes,
            prepared_source_result_snapshot_digest,
            prepared_result_deployment_snapshot_bytes,
            prepared_result_deployment_snapshot_digest,
            terminal_time
        ) AS source;
    EXCEPTION
        WHEN SQLSTATE 'RX002' OR SQLSTATE 'RX003'
            OR SQLSTATE 'RX004' THEN
            outcome_name := 'persistence_corrupt';
            preparation_ready := FALSE;
            exact_replay := FALSE;
            requires_commit := FALSE;
            RETURN NEXT;
            RETURN;
    END;

    SELECT committed.*
    INTO apply_commit_row
    FROM starring_runtime_private_v2.starring_product_apply_commit_unfenced_core_v2(
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
        new_deployment_id,
        apply_lock_row.locked_projection,
        prepared_desired_target_digest,
        apply_lock_row.locked_projection -> 'previous_runtime',
        prepared_result_deployment_snapshot,
        prepared_activation_notices,
        terminal_time,
        FALSE
    ) AS committed;
    IF apply_commit_row.outcome <> 'ok'
        OR apply_commit_row.resulting_revision <>
            expected_product_revision + 2
        OR apply_commit_row.resulting_state <> 'applied'
        OR apply_commit_row.exact_replay
        OR apply_commit_row.guild_id <> expected_guild_id
        OR apply_commit_row.deployment_id <> new_deployment_id
        OR apply_commit_row.desired_target_digest <>
            prepared_desired_target_digest
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE =
                'product_apply_consume_commit_projection_invalid';
    END IF;

    SELECT deployment.*
    INTO current_result_row
    FROM public.runtime_deployments AS deployment
    WHERE deployment.deployment_id = new_deployment_id
    FOR UPDATE;
    IF NOT FOUND
        OR current_result_row.revision <> 1
        OR current_result_row.snapshot <>
            prepared_result_deployment_snapshot
        OR current_result_row.runtime_generation <=
            source_result_row.runtime_generation
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE =
                'product_apply_consume_result_deployment_invalid';
    END IF;

    terminal_drain_row :=
        starring_runtime_private_v2.starring_runtime_product_drain_terminal_transition_v2(
            expected_drain_intent_id,
            expected_source_intent_revision,
            expected_source_state_digest,
            'consumed',
            1,
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
            expected_source_state_bytes,
            expected_source_state_digest,
            terminal_drain_row.intent_revision,
            terminal_drain_row.canonical_state_digest,
            'consumed',
            terminal_time
        );

    computed_terminal_projection :=
        starring_runtime_private_v2.starring_runtime_product_drain_terminal_projection_v2(
            'consumed',
            proposed_terminal_action_id,
            active_idempotency_key_digest,
            semantic_request_digest,
            NULL,
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
            prepared_source_result_snapshot_digest,
            new_deployment_id,
            1,
            prepared_result_deployment_snapshot_digest,
            slot_fence_row.writer_epoch,
            computed_successor_epoch,
            new_receipt_id,
            new_audit_event_id,
            expected_authority_observation_digest,
            expected_authority_revision,
            terminal_time
        );
    IF computed_terminal_projection IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE =
                'product_apply_consume_terminal_projection_invalid';
    END IF;
    computed_terminal_projection_digest :=
        pg_catalog.encode(
            pg_catalog.sha256(computed_terminal_projection),
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
        terminal_projection_digest
    ) VALUES (
        proposed_terminal_action_id,
        'consumed',
        drain_row.drain_intent_id,
        product_row.product_operation_id,
        product_row.product_mutation_digest,
        drain_row.drain_intent_digest,
        active_idempotency_key_digest,
        semantic_request_digest,
        NULL,
        expected_source_intent_revision,
        expected_source_state_digest,
        terminal_drain_row.intent_revision,
        terminal_drain_row.canonical_state_digest,
        expected_source_deployment_revision,
        source_result_row.revision,
        prepared_source_result_snapshot_digest,
        prepared_source_result_snapshot_bytes,
        new_deployment_id,
        1,
        prepared_result_deployment_snapshot_digest,
        prepared_result_deployment_snapshot_bytes,
        slot_fence_row.writer_epoch,
        computed_successor_epoch,
        terminal_time,
        new_receipt_id,
        new_audit_event_id,
        expected_authority_observation_digest,
        expected_authority_revision,
        computed_terminal_projection,
        computed_terminal_projection_digest
    )
    RETURNING * INTO action_row;

    IF NOT starring_runtime_private_v2.starring_runtime_product_drain_terminal_action_exact_v2(
        action_row,
        product_row,
        terminal_drain_row
    )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RX004',
            MESSAGE =
                'product_apply_consume_terminal_action_invalid';
    END IF;

    SET CONSTRAINTS
        public.runtime_drain_intents_v2_assert_slot_writer_fence_symmetry,
        public.runtime_slot_writer_fences_v2_assert_pending_symmetry
    IMMEDIATE;
    SET CONSTRAINTS
        public.runtime_drain_intents_v2_assert_slot_writer_fence_symmetry,
        public.runtime_slot_writer_fences_v2_assert_pending_symmetry
    DEFERRED;

    outcome_name := 'applied';
    preparation_ready := FALSE;
    exact_replay := FALSE;
    requires_commit := FALSE;
    preparation_token := NULL;
    locked_product_projection :=
        apply_lock_row.locked_projection;
    source_deployment_snapshot :=
        locked_source_row.snapshot;
    source_acknowledged_at := acknowledged_time;
    product_operation_id :=
        product_row.product_operation_id;
    product_mutation_digest :=
        product_row.product_mutation_digest;
    drain_intent_digest := drain_row.drain_intent_digest;
    source_deployment_id :=
        locked_source_row.deployment_id;
    source_deployment_revision :=
        locked_source_row.revision;
    source_result_deployment_revision :=
        source_result_row.revision;
    source_result_deployment_snapshot :=
        source_result_row.snapshot;
    source_result_deployment_snapshot_digest :=
        prepared_source_result_snapshot_digest;
    result_deployment_id := current_result_row.deployment_id;
    result_deployment_revision := current_result_row.revision;
    result_deployment_snapshot :=
        current_result_row.snapshot;
    result_deployment_snapshot_digest :=
        prepared_result_deployment_snapshot_digest;
    product_resulting_revision :=
        apply_commit_row.resulting_revision;
    product_resulting_state :=
        apply_commit_row.resulting_state;
    product_receipt_id := new_receipt_id;
    product_audit_event_id := new_audit_event_id;
    drain_intent_id := terminal_drain_row.drain_intent_id;
    source_intent_revision :=
        expected_source_intent_revision;
    source_state_bytes := expected_source_state_bytes;
    source_state_digest := expected_source_state_digest;
    result_intent_revision :=
        terminal_drain_row.intent_revision;
    result_intent_state := terminal_drain_row.intent_state;
    result_state_bytes :=
        terminal_drain_row.canonical_state_bytes;
    result_state_digest :=
        terminal_drain_row.canonical_state_digest;
    source_slot_epoch := slot_fence_row.writer_epoch;
    successor_slot_epoch := computed_successor_epoch;
    terminal_action_id := action_row.terminal_action_id;
    terminal_projection_bytes :=
        action_row.terminal_projection_bytes;
    terminal_projection_digest :=
        action_row.terminal_projection_digest;
    terminal_database_time := terminal_time;
    RETURN NEXT;
END;
$function$;

DO $close_consume_capability_acl$
DECLARE
    common_owner OID;
    apply_executor OID;
    apply_executor_count BIGINT;
    function_identity TEXT;
    public_identity CONSTANT TEXT :=
        'public.starring_product_apply_consume_runtime_drain_v2(text,text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text,bigint,bytea,text,text,text,bigint,text,text,bytea,text,bytea,text,text,bytea)';
BEGIN
    FOREACH function_identity IN ARRAY ARRAY[
        public_identity,
        'starring_runtime_private_v2.starring_runtime_product_drain_source_supersession_exact_v2(public.runtime_deployments,jsonb,public.runtime_drain_intents_v2,jsonb,timestamp with time zone)',
        'starring_runtime_private_v2.starring_runtime_product_drain_consume_root_exact_v2(public.runtime_product_operations_v2,public.runtime_drain_intents_v2,public.runtime_deployments,text,text,bigint,bytea,text,text)',
        'starring_runtime_private_v2.starring_runtime_product_drain_supersede_source_v2(text,text,bigint,bytea,text,bytea,text,timestamp with time zone)',
        'starring_runtime_private_v2.starring_product_apply_consumed_terminal_replay_exact_v2(text,text,text,text,text[],text,text,text,bigint,public.product_action_receipts,public.product_audit_events)',
        'starring_runtime_private_v2.starring_product_apply_consume_preparation_reservation_v2(text,text,text,text,timestamp with time zone)',
        'starring_runtime_private_v2.starring_product_apply_consume_lock_core_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text)',
        'starring_runtime_private_v2.starring_product_apply_commit_unfenced_core_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb,timestamp with time zone,boolean)'
    ]
    LOOP
        IF pg_catalog.to_regprocedure(function_identity) IS NULL THEN
            RAISE EXCEPTION USING
                ERRCODE = 'PA001',
                MESSAGE =
                    'product_apply_consume_acl_function_missing';
        END IF;
        EXECUTE pg_catalog.format(
            'REVOKE ALL ON FUNCTION %s FROM PUBLIC',
            function_identity
        );
    END LOOP;

    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );
    SELECT
        pg_catalog.min(privilege.grantee::BIGINT)::OID,
        pg_catalog.count(*)
    INTO apply_executor, apply_executor_count
    FROM pg_catalog.pg_proc AS function_row
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        function_row.proacl,
        pg_catalog.acldefault('f', function_row.proowner)
    )) AS privilege
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'
        )
        AND privilege.grantee <> common_owner
        AND privilege.privilege_type = 'EXECUTE';
    IF apply_executor_count > 1
        OR (
            apply_executor_count = 1
            AND apply_executor = 0
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'PA001',
            MESSAGE =
                'product_apply_consume_executor_topology_invalid';
    END IF;
    IF apply_executor_count = 1 THEN
        EXECUTE pg_catalog.format(
            'GRANT EXECUTE ON FUNCTION %s TO %I',
            public_identity,
            pg_catalog.pg_get_userbyid(apply_executor)
        );
    END IF;
END;
$close_consume_capability_acl$;

DO $patch_consume_shared_readiness$
DECLARE
    identity TEXT;
    definition TEXT;
    previous_fragment TEXT;
    next_fragment TEXT;
BEGIN
    FOREACH identity IN ARRAY ARRAY[
        'public.starring_runtime_exact_target_schema_manifest_v1()',
        'public.starring_runtime_serving_schema_manifest_v1()'
    ]
    LOOP
        SELECT pg_catalog.pg_get_functiondef(function_row.oid)
        INTO definition
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid =
            pg_catalog.to_regprocedure(identity);
        IF identity =
            'public.starring_runtime_exact_target_schema_manifest_v1()'
        THEN
            previous_fragment :=
                '    RETURN observed_count = 356' || E'\n' ||
                '        AND observed_digest' || E'\n' ||
                '            = ''6971d3c87da56aecd5c5615a26e8a2d3f2029e4d3e492f2c253fe73c4f8218f2'';';
            next_fragment :=
                '    RETURN observed_count = 356' || E'\n' ||
                '        AND observed_digest' || E'\n' ||
                '            = ''0a33a7e7cc2e3e07b7d06e3d8ec6ad48bba473c2a877ea824f6f341ed4d4e7a7'';';
        ELSE
            previous_fragment :=
                '    RETURN observed_count = 471' || E'\n' ||
                '        AND observed_digest' || E'\n' ||
                '            = ''877f60fa04f60d99e7f41c11baaec89707722578487bbc932aa20a608dc49b22'';';
            next_fragment :=
                '    RETURN observed_count = 471' || E'\n' ||
                '        AND observed_digest' || E'\n' ||
                '            = ''ae127076f030fd9d5f38f1fc8403b00ba91503e96bf152624dfd8e968f74012c'';';
        END IF;
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
                    'product_apply_consume_shared_manifest_drift';
        END IF;
        EXECUTE pg_catalog.replace(
            definition,
            previous_fragment,
            next_fragment
        );
    END LOOP;

    FOREACH identity IN ARRAY ARRAY[
        'public.starring_runtime_exact_target_database_readiness_v1()',
        'public.starring_runtime_serving_database_readiness_v1()'
    ]
    LOOP
        SELECT pg_catalog.pg_get_functiondef(function_row.oid)
        INTO definition
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid =
            pg_catalog.to_regprocedure(identity);
        IF identity =
            'public.starring_runtime_exact_target_database_readiness_v1()'
        THEN
            previous_fragment :=
                '''bea5a930a40537f9f06f19a350d1fdba3bf21b222844eb0f442fb506d91a1ebb''::TEXT';
            next_fragment :=
                '''b8dad14ddbb78262526673ae75a212ca11b1709ba0ee5a54f5125f55da471af7''::TEXT';
        ELSE
            previous_fragment :=
                '''c679ef7c0722416b514324936a95884d17242e6b67cdb130987e4d4f03a43758''::TEXT';
            next_fragment :=
                '''a2362a5fa1b9839e124a290cc1845c4af450e49d2d7d6517c97982d2c4f45546''::TEXT';
        END IF;
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
                    'product_apply_consume_shared_readiness_drift';
        END IF;
        EXECUTE pg_catalog.replace(
            definition,
            previous_fragment,
            next_fragment
        );
    END LOOP;
END;
$patch_consume_shared_readiness$;

DO $patch_consume_execution_manifest$
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
        'public.starring_product_apply_consume_runtime_drain_v2(text,text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text,bigint,bytea,text,text,text,bigint,text,text,bytea,text,bytea,text,text,bytea)',
        'starring_runtime_private_v2.starring_runtime_product_drain_source_supersession_exact_v2(public.runtime_deployments,jsonb,public.runtime_drain_intents_v2,jsonb,timestamp with time zone)',
        'starring_runtime_private_v2.starring_runtime_product_drain_consume_root_exact_v2(public.runtime_product_operations_v2,public.runtime_drain_intents_v2,public.runtime_deployments,text,text,bigint,bytea,text,text)',
        'starring_runtime_private_v2.starring_runtime_product_drain_supersede_source_v2(text,text,bigint,bytea,text,bytea,text,timestamp with time zone)',
        'starring_runtime_private_v2.starring_product_apply_consumed_terminal_replay_exact_v2(text,text,text,text,text[],text,text,text,bigint,public.product_action_receipts,public.product_audit_events)',
        'starring_runtime_private_v2.starring_product_apply_consume_preparation_reservation_v2(text,text,text,text,timestamp with time zone)',
        'starring_runtime_private_v2.starring_product_apply_consume_lock_core_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text)',
        'starring_runtime_private_v2.starring_product_apply_commit_unfenced_core_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb,timestamp with time zone,boolean)'
    ]
    LOOP
        next_fragment := next_fragment || E'\n' ||
            '        UNION' || E'\n' ||
            '        SELECT pg_catalog.to_regprocedure(' || E'\n' ||
            '            ' || pg_catalog.quote_literal(identity) || E'\n' ||
            '        )';
    END LOOP;
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
                'product_apply_consume_manifest_function_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '    RETURN observed_count = 888' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''aacb4889c005088a91b93ee948502397aa8747275087a4e2a600d2d49a9b8181'';';
    next_fragment :=
        '    RETURN observed_count = 901' || E'\n' ||
        '        AND observed_digest' || E'\n' ||
        '            = ''5d70734095987ce4f70a9edddccd345e99a62e2c2090c6c8c11cc662d092d065'';';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'product_apply_consume_manifest_expectation_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$patch_consume_execution_manifest$;

DO $patch_consume_execution_readiness$
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
        '            (''starring_runtime_private_v2.starring_runtime_product_drain_source_supersession_exact_v2(public.runtime_deployments,jsonb,public.runtime_drain_intents_v2,jsonb,timestamp with time zone)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_product_drain_consume_root_exact_v2(public.runtime_product_operations_v2,public.runtime_drain_intents_v2,public.runtime_deployments,text,text,bigint,bytea,text,text)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_runtime_product_drain_supersede_source_v2(text,text,bigint,bytea,text,bytea,text,timestamp with time zone)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_product_apply_consumed_terminal_replay_exact_v2(text,text,text,text,text[],text,text,text,bigint,public.product_action_receipts,public.product_audit_events)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_product_apply_consume_preparation_reservation_v2(text,text,text,text,timestamp with time zone)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_product_apply_consume_lock_core_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text)''),' || E'\n' ||
        '            (''starring_runtime_private_v2.starring_product_apply_commit_unfenced_core_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb,timestamp with time zone,boolean)''),';
    IF definition IS NULL
        OR pg_catalog.strpos(definition, previous_fragment) = 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'product_apply_consume_readiness_private_drift';
    END IF;
    definition := pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );

    previous_fragment :=
        '''0e40c195026bf46ce6a8e5e70472d108de5deb533d1f072cf056e171c7078fe7''::TEXT';
    next_fragment :=
        '''d65a674f2d4ce2337a6bf8c5d74ad63ff21f0090a70e3bf1049e07dd18bc3abd''::TEXT';
    IF pg_catalog.strpos(definition, previous_fragment) = 0
        OR pg_catalog.strpos(
            pg_catalog.replace(definition, previous_fragment, ''),
            previous_fragment
        ) <> 0
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RE001',
            MESSAGE =
                'product_apply_consume_readiness_manifest_digest_drift';
    END IF;
    EXECUTE pg_catalog.replace(
        definition,
        previous_fragment,
        next_fragment
    );
END;
$patch_consume_execution_readiness$;

DO $postflight$
DECLARE
    common_owner OID;
    public_function OID;
    invalid_public_count BIGINT;
    invalid_private_count BIGINT;
    invalid_acl_count BIGINT;
    invalid_constraint_count BIGINT;
    terminal_action_count BIGINT;
    manifest_valid BOOLEAN;
    exact_manifest_valid BOOLEAN;
    serving_manifest_valid BOOLEAN;
    manifest_definition_digest TEXT;
    exact_manifest_definition_digest TEXT;
    serving_manifest_definition_digest TEXT;
    readiness_definition_digest TEXT;
    exact_readiness_definition_digest TEXT;
    serving_readiness_definition_digest TEXT;
BEGIN
    SELECT relation.relowner
    INTO common_owner
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass(
        'public.runtime_deployments'
    );
    public_function := pg_catalog.to_regprocedure(
        'public.starring_product_apply_consume_runtime_drain_v2(text,text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text,bigint,bytea,text,text,text,bigint,text,text,bytea,text,bytea,text,text,bytea)'
    );

    SELECT pg_catalog.count(*)
    INTO invalid_public_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid = public_function
        AND (
            function_row.proowner <> common_owner
            OR function_row.prokind <> 'f'
            OR function_row.provolatile <> 'v'
            OR NOT function_row.proisstrict
            OR function_row.proparallel <> 'u'
            OR NOT function_row.prosecdef
            OR function_row.proleakproof
            OR function_row.pronargdefaults <> 0
            OR function_row.provariadic <> 0
            OR NOT function_row.proretset
            OR function_row.prorows <> 1
            OR function_row.proconfig
                <> ARRAY['search_path=pg_catalog']::TEXT[]
            OR language_row.lanname <> 'plpgsql'
        );
    IF public_function IS NULL THEN
        invalid_public_count := invalid_public_count + 1;
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_private_count
    FROM (
        VALUES
            ('starring_runtime_private_v2.starring_runtime_product_drain_source_supersession_exact_v2(public.runtime_deployments,jsonb,public.runtime_drain_intents_v2,jsonb,timestamp with time zone)'),
            ('starring_runtime_private_v2.starring_runtime_product_drain_consume_root_exact_v2(public.runtime_product_operations_v2,public.runtime_drain_intents_v2,public.runtime_deployments,text,text,bigint,bytea,text,text)'),
            ('starring_runtime_private_v2.starring_runtime_product_drain_supersede_source_v2(text,text,bigint,bytea,text,bytea,text,timestamp with time zone)'),
            ('starring_runtime_private_v2.starring_product_apply_consumed_terminal_replay_exact_v2(text,text,text,text,text[],text,text,text,bigint,public.product_action_receipts,public.product_audit_events)'),
            ('starring_runtime_private_v2.starring_product_apply_consume_preparation_reservation_v2(text,text,text,text,timestamp with time zone)'),
            ('starring_runtime_private_v2.starring_product_apply_consume_lock_core_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text)'),
            ('starring_runtime_private_v2.starring_product_apply_commit_unfenced_core_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb,timestamp with time zone,boolean)')
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
        OR function_row.proconfig
            <> ARRAY['search_path=pg_catalog']::TEXT[]
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault(
                    'f',
                    function_row.proowner
                )
            )) AS privilege
            WHERE privilege.grantee <> function_row.proowner
        );

    WITH lock_grants AS (
        SELECT privilege.grantee,
            privilege.grantor,
            privilege.privilege_type,
            privilege.is_grantable
        FROM pg_catalog.pg_proc AS function_row
        CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
            function_row.proacl,
            pg_catalog.acldefault('f', function_row.proowner)
        )) AS privilege
        WHERE function_row.oid = pg_catalog.to_regprocedure(
                'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'
            )
            AND privilege.grantee <> common_owner
    ), consume_grants AS (
        SELECT privilege.grantee,
            privilege.grantor,
            privilege.privilege_type,
            privilege.is_grantable
        FROM pg_catalog.pg_proc AS function_row
        CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
            function_row.proacl,
            pg_catalog.acldefault('f', function_row.proowner)
        )) AS privilege
        WHERE function_row.oid = public_function
            AND privilege.grantee <> common_owner
    ), difference AS (
        (SELECT * FROM lock_grants EXCEPT SELECT * FROM consume_grants)
        UNION ALL
        (SELECT * FROM consume_grants EXCEPT SELECT * FROM lock_grants)
    )
    SELECT pg_catalog.count(*)
    INTO invalid_acl_count
    FROM difference;

    SELECT pg_catalog.count(*)
    INTO invalid_constraint_count
    FROM (
        VALUES
            (
                'runtime_product_drain_terminal_actions_v2_receipt_fk',
                'FOREIGN KEY (product_receipt_id) REFERENCES public.product_action_receipts(receipt_id) ON DELETE RESTRICT'
            ),
            (
                'runtime_product_drain_terminal_actions_v2_audit_fk',
                'FOREIGN KEY (product_audit_event_id) REFERENCES public.product_audit_events(event_id) ON DELETE RESTRICT'
            )
    ) AS expected(constraint_name, definition)
    LEFT JOIN pg_catalog.pg_constraint AS constraint_row
        ON constraint_row.conrelid = pg_catalog.to_regclass(
            'public.runtime_product_drain_terminal_actions_v2'
        )
        AND constraint_row.conname =
            expected.constraint_name
    WHERE constraint_row.oid IS NULL
        OR constraint_row.contype <> 'f'
        OR NOT constraint_row.convalidated
        OR constraint_row.condeferrable
        OR constraint_row.condeferred
        OR constraint_row.conparentid <> 0
        OR pg_catalog.pg_get_constraintdef(
            constraint_row.oid,
            TRUE
        ) <> expected.definition;

    SELECT pg_catalog.count(*)
    INTO terminal_action_count
    FROM public.runtime_product_drain_terminal_actions_v2;
    SELECT public.starring_runtime_execution_schema_manifest_v1()
    INTO manifest_valid;
    SELECT
        public.starring_runtime_exact_target_schema_manifest_v1(),
        public.starring_runtime_serving_schema_manifest_v1()
    INTO exact_manifest_valid, serving_manifest_valid;
    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_execution_schema_manifest_v1()'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO manifest_definition_digest;
    SELECT
        pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_exact_target_schema_manifest_v1()'
                    )
                ),
                'UTF8'
            )),
            'hex'
        ),
        pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_serving_schema_manifest_v1()'
                    )
                ),
                'UTF8'
            )),
            'hex'
        )
    INTO exact_manifest_definition_digest,
        serving_manifest_definition_digest;
    SELECT pg_catalog.encode(
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_execution_database_readiness_v1()'
                )
            ),
            'UTF8'
        )),
        'hex'
    )
    INTO readiness_definition_digest;
    SELECT
        pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_exact_target_database_readiness_v1()'
                    )
                ),
                'UTF8'
            )),
            'hex'
        ),
        pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.pg_get_functiondef(
                    pg_catalog.to_regprocedure(
                        'public.starring_runtime_serving_database_readiness_v1()'
                    )
                ),
                'UTF8'
            )),
            'hex'
        )
    INTO exact_readiness_definition_digest,
        serving_readiness_definition_digest;

    IF common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR invalid_public_count <> 0
        OR invalid_private_count <> 0
        OR invalid_acl_count <> 0
        OR invalid_constraint_count <> 0
        OR terminal_action_count <> 0
        OR NOT manifest_valid
        OR NOT exact_manifest_valid
        OR NOT serving_manifest_valid
        OR manifest_definition_digest <>
            'd65a674f2d4ce2337a6bf8c5d74ad63ff21f0090a70e3bf1049e07dd18bc3abd'
        OR exact_manifest_definition_digest <>
            'b8dad14ddbb78262526673ae75a212ca11b1709ba0ee5a54f5125f55da471af7'
        OR serving_manifest_definition_digest <>
            'a2362a5fa1b9839e124a290cc1845c4af450e49d2d7d6517c97982d2c4f45546'
        OR readiness_definition_digest <>
            '059ee21b16b325a4da71dda5d63f75c8aeac4d0e2d9b18cbb3f628d15ea8967d'
        OR exact_readiness_definition_digest <>
            '35903afa3bb9bebe712559a80a503823f4eeedf0d15ebd3d24ce3dbf706b5c14'
        OR serving_readiness_definition_digest <>
            '8263d7ebddcd4f4c45b5b129ee061e85f92d596847d28448e0bb29dec6c8588d'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'PA001',
            MESSAGE =
                'product_apply_consume_runtime_drain_v2_postflight_drift',
            DETAIL = pg_catalog.format(
                'public=%s private=%s acl=%s constraints=%s terminal=%s manifest=%s exact_manifest=%s serving_manifest=%s manifest_digest=%s exact_manifest_digest=%s serving_manifest_digest=%s readiness_digest=%s exact_readiness_digest=%s serving_readiness_digest=%s',
                invalid_public_count,
                invalid_private_count,
                invalid_acl_count,
                invalid_constraint_count,
                terminal_action_count,
                manifest_valid,
                exact_manifest_valid,
                serving_manifest_valid,
                manifest_definition_digest,
                exact_manifest_definition_digest,
                serving_manifest_definition_digest,
                readiness_definition_digest,
                exact_readiness_definition_digest,
                serving_readiness_definition_digest
            );
    END IF;
END;
$postflight$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
