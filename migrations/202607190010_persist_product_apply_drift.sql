ALTER FUNCTION public.starring_product_apply_lock_v1(
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
) RENAME TO starring_product_apply_lock_core_v1;

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

CREATE FUNCTION public.starring_product_apply_lock_v1(
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
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    core_row RECORD;
    activation_row public.activation_requests%ROWTYPE;
    promotion_row public.authoring_promotions%ROWTYPE;
    installation_row public.automation_installations%ROWTYPE;
    authority_row public.automation_installation_authority_versions%ROWTYPE;
    historical_authority_row public.automation_installation_authority_versions%ROWTYPE;
    session_row public.product_auth_sessions%ROWTYPE;
    receipt_row public.product_action_receipts%ROWTYPE;
    audit_row public.product_audit_events%ROWTYPE;
    matched_terminal_count BIGINT;
    approval_count BIGINT;
    current_active_version BIGINT;
    current_active_hash TEXT;
    target_schema_version BIGINT;
    target_content_hash TEXT;
    target_is_active BOOLEAN;
    baseline_drift BOOLEAN;
    binding_drift BOOLEAN;
    policy_drift BOOLEAN;
    mutation_clock TIMESTAMPTZ;
    next_revision BIGINT;
    result_code TEXT;
    reason_record JSONB;
    termination_record JSONB;
    active_baseline_version BIGINT;
    active_baseline_hash TEXT;
    postvalidation_outcome TEXT;
BEGIN
    BEGIN
        SELECT *
        INTO core_row
        FROM public.starring_product_apply_lock_core_v1(
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
        );

    IF core_row.outcome NOT IN (
        'authority_mismatch',
        'baseline_mismatch',
        'indeterminate'
    ) AND NOT (
        core_row.outcome = 'ok'
        AND core_row.exact_replay
    ) THEN
        RETURN QUERY SELECT core_row.outcome,
            core_row.exact_replay,
            core_row.requires_commit,
            core_row.resulting_revision,
            core_row.resulting_state,
            core_row.deployment_id,
            core_row.desired_target_digest,
            core_row.locked_projection;
        RETURN;
    END IF;

    IF core_row.outcome = 'ok' AND core_row.exact_replay THEN
        SELECT installation.*
        INTO installation_row
        FROM public.automation_installations AS installation
        WHERE installation.tenant_id = expected_tenant_id
            AND installation.installation_id = expected_installation_id
        FOR SHARE;

        SELECT authority.*
        INTO authority_row
        FROM public.automation_installation_authority_versions AS authority
        WHERE authority.tenant_id = expected_tenant_id
            AND authority.installation_id = expected_installation_id
            AND authority.revision = installation_row.current_authority_revision
        FOR SHARE;

        SELECT product_session.*
        INTO session_row
        FROM public.product_auth_sessions AS product_session
        WHERE product_session.session_digest = expected_product_session_digest
            AND product_session.principal_id = expected_principal_id
        FOR SHARE;

        SELECT activation.*
        INTO activation_row
        FROM public.activation_requests AS activation
        WHERE activation.tenant_id = expected_tenant_id
            AND activation.installation_id = expected_installation_id
            AND activation.promotion_id = expected_promotion_id
        FOR SHARE;

        SELECT receipt.*
        INTO receipt_row
        FROM public.product_action_receipts AS receipt
        INNER JOIN (
            SELECT DISTINCT alias.receipt_id
            FROM public.product_action_receipt_idempotency_aliases AS alias
            WHERE alias.tenant_id = expected_tenant_id
                AND alias.installation_id = expected_installation_id
                AND alias.principal_id = expected_principal_id
                AND alias.endpoint_domain = 'product_apply_v1'
                AND alias.idempotency_key_digest = ANY(idempotency_key_digest_candidates)
            ORDER BY alias.receipt_id
            LIMIT 1
        ) AS matched ON matched.receipt_id = receipt.receipt_id
        WHERE receipt.tenant_id = expected_tenant_id
            AND receipt.installation_id = expected_installation_id
            AND receipt.principal_id = expected_principal_id
            AND receipt.endpoint_domain = 'product_apply_v1'
        FOR SHARE OF receipt;

        SELECT audit.*
        INTO audit_row
        FROM public.product_audit_events AS audit
        INNER JOIN public.product_action_receipt_audit_evidence AS evidence
            ON evidence.receipt_id = audit.receipt_id
            AND evidence.event_id = audit.event_id
            AND evidence.tenant_id = receipt_row.tenant_id
            AND evidence.installation_id = receipt_row.installation_id
            AND evidence.principal_id = receipt_row.principal_id
            AND evidence.endpoint_domain = receipt_row.endpoint_domain
            AND evidence.action = 'promotion.apply'
            AND evidence.request_digest = receipt_row.request_digest
            AND evidence.target_resource_type = receipt_row.target_resource_type
            AND evidence.target_resource_id = receipt_row.target_resource_id
            AND evidence.resulting_revision
                IS NOT DISTINCT FROM receipt_row.resulting_revision
            AND evidence.resulting_state = receipt_row.resulting_state
            AND evidence.result_code = receipt_row.result_code
            AND evidence.http_disposition_class = receipt_row.http_disposition_class
            AND evidence.completed_at = receipt_row.completed_at
            AND evidence.evidence_version = 1
            AND evidence.replay_policy_version = 1
        WHERE audit.receipt_id = receipt_row.receipt_id
            AND audit.tenant_id = receipt_row.tenant_id
            AND audit.installation_id = receipt_row.installation_id
            AND audit.principal_id = receipt_row.principal_id
            AND audit.action = 'promotion.apply'
            AND audit.target_resource_type = receipt_row.target_resource_type
            AND audit.target_resource_id = receipt_row.target_resource_id
            AND audit.resulting_state = receipt_row.resulting_state
            AND audit.result_code = receipt_row.result_code;

        mutation_clock := pg_catalog.clock_timestamp();
        IF session_row.principal_id IS NULL
            OR session_row.oauth_state_digest IS NULL
            OR session_row.revoked_at IS NOT NULL
            OR mutation_clock >= session_row.idle_expires_at
            OR mutation_clock >= session_row.absolute_expires_at
            OR expected_authority_observed_at > mutation_clock
            OR mutation_clock >= expected_authority_expires_at
        THEN
            postvalidation_outcome := 'authorization_stale';
            RAISE EXCEPTION 'product apply replay authorization expired after core replay'
                USING ERRCODE = 'PZ001';
        END IF;

        IF installation_row.installation_id IS NULL
            OR authority_row.installation_id IS NULL
            OR installation_row.current_authority_revision
                IS DISTINCT FROM expected_authority_revision
            OR authority_row.authority_payload_digest
                IS DISTINCT FROM expected_authority_payload_digest
        THEN
            postvalidation_outcome := 'authority_mismatch';
            RAISE EXCEPTION 'product apply replay authority changed after core replay'
                USING ERRCODE = 'PZ001';
        END IF;

        IF activation_row.id IS NULL
            OR receipt_row.receipt_id IS NULL
            OR audit_row.event_id IS NULL
            OR activation_row.state <> 'applied'
            OR activation_row.product_revision
                IS DISTINCT FROM receipt_row.resulting_revision
            OR activation_row.approval_payload_digest
                IS DISTINCT FROM expected_payload_digest
            OR activation_row.created_at > receipt_row.completed_at
            OR receipt_row.completed_at >= activation_row.expires_at
            OR receipt_row.resulting_revision
                IS DISTINCT FROM core_row.resulting_revision
            OR receipt_row.resulting_state <> 'applied'
            OR core_row.resulting_state <> 'applied'
            OR receipt_row.result_code <> 'runtime_requested'
            OR receipt_row.http_disposition_class <> 2
            OR receipt_row.request_digest IS DISTINCT FROM semantic_request_digest
            OR receipt_row.target_resource_type <> 'authoring_promotion'
            OR receipt_row.target_resource_id IS DISTINCT FROM expected_promotion_id
            OR audit_row.receipt_id IS DISTINCT FROM receipt_row.receipt_id
            OR audit_row.resulting_state IS DISTINCT FROM receipt_row.resulting_state
            OR audit_row.result_code IS DISTINCT FROM receipt_row.result_code
            OR audit_row.occurred_at IS DISTINCT FROM receipt_row.completed_at
            OR audit_row.payload_digest IS DISTINCT FROM expected_payload_digest
        THEN
            postvalidation_outcome := 'indeterminate';
            RAISE EXCEPTION 'product apply replay evidence changed after core replay'
                USING ERRCODE = 'PZ001';
        END IF;

        RETURN QUERY SELECT core_row.outcome,
            core_row.exact_replay,
            core_row.requires_commit,
            core_row.resulting_revision,
            core_row.resulting_state,
            core_row.deployment_id,
            core_row.desired_target_digest,
            core_row.locked_projection;
        RETURN;
    END IF;

    EXCEPTION
        WHEN SQLSTATE 'PZ001' THEN
            RETURN QUERY SELECT postvalidation_outcome, FALSE, FALSE,
                NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
            RETURN;
    END;

    IF core_row.outcome = 'indeterminate' THEN
        SELECT pg_catalog.count(DISTINCT alias.receipt_id)
        INTO matched_terminal_count
        FROM public.product_action_receipt_idempotency_aliases AS alias
        WHERE alias.tenant_id = expected_tenant_id
            AND alias.installation_id = expected_installation_id
            AND alias.principal_id = expected_principal_id
            AND alias.endpoint_domain = 'product_apply_v1'
            AND alias.idempotency_key_digest = ANY(idempotency_key_digest_candidates);

        IF matched_terminal_count = 1 THEN
            SELECT receipt.*
            INTO receipt_row
            FROM public.product_action_receipts AS receipt
            INNER JOIN (
                SELECT DISTINCT alias.receipt_id
                FROM public.product_action_receipt_idempotency_aliases AS alias
                WHERE alias.tenant_id = expected_tenant_id
                    AND alias.installation_id = expected_installation_id
                    AND alias.principal_id = expected_principal_id
                    AND alias.endpoint_domain = 'product_apply_v1'
                    AND alias.idempotency_key_digest
                        = ANY(idempotency_key_digest_candidates)
                ORDER BY alias.receipt_id
                LIMIT 1
            ) AS matched ON matched.receipt_id = receipt.receipt_id
            WHERE receipt.tenant_id = expected_tenant_id
                AND receipt.installation_id = expected_installation_id
                AND receipt.principal_id = expected_principal_id
                AND receipt.endpoint_domain = 'product_apply_v1'
            FOR UPDATE OF receipt;
        END IF;

        IF receipt_row.receipt_id IS NOT NULL
            AND receipt_row.request_digest = semantic_request_digest
            AND receipt_row.target_resource_type = 'authoring_promotion'
            AND receipt_row.target_resource_id = expected_promotion_id
            AND receipt_row.resulting_revision IS NOT NULL
            AND receipt_row.resulting_state = 'superseded'
            AND receipt_row.result_code IN (
                'superseded_baseline_drift',
                'superseded_binding_drift',
                'superseded_policy_drift'
            )
            AND receipt_row.http_disposition_class = 4
        THEN
            SELECT installation.*
            INTO installation_row
            FROM public.automation_installations AS installation
            WHERE installation.tenant_id = expected_tenant_id
                AND installation.installation_id = expected_installation_id
            FOR SHARE;

            SELECT authority.*
            INTO authority_row
            FROM public.automation_installation_authority_versions AS authority
            WHERE authority.tenant_id = expected_tenant_id
                AND authority.installation_id = expected_installation_id
                AND authority.revision = installation_row.current_authority_revision
            FOR SHARE;

            SELECT product_session.*
            INTO session_row
            FROM public.product_auth_sessions AS product_session
            WHERE product_session.session_digest = expected_product_session_digest
                AND product_session.principal_id = expected_principal_id
            FOR SHARE;

            SELECT activation.*
            INTO activation_row
            FROM public.activation_requests AS activation
            WHERE activation.tenant_id = expected_tenant_id
                AND activation.installation_id = expected_installation_id
                AND activation.promotion_id = expected_promotion_id
            FOR SHARE;

            SELECT audit.*
            INTO audit_row
            FROM public.product_audit_events AS audit
            INNER JOIN public.product_action_receipt_audit_evidence AS evidence
                ON evidence.receipt_id = audit.receipt_id
                AND evidence.event_id = audit.event_id
                AND evidence.tenant_id = receipt_row.tenant_id
                AND evidence.installation_id = receipt_row.installation_id
                AND evidence.principal_id = receipt_row.principal_id
                AND evidence.endpoint_domain = receipt_row.endpoint_domain
                AND evidence.action = 'promotion.apply'
                AND evidence.request_digest = receipt_row.request_digest
                AND evidence.target_resource_type = receipt_row.target_resource_type
                AND evidence.target_resource_id = receipt_row.target_resource_id
                AND evidence.resulting_revision
                    IS NOT DISTINCT FROM receipt_row.resulting_revision
                AND evidence.resulting_state = receipt_row.resulting_state
                AND evidence.result_code = receipt_row.result_code
                AND evidence.http_disposition_class = receipt_row.http_disposition_class
                AND evidence.completed_at = receipt_row.completed_at
                AND evidence.evidence_version = 1
                AND evidence.replay_policy_version = 1
            WHERE audit.receipt_id = receipt_row.receipt_id
                AND audit.tenant_id = receipt_row.tenant_id
                AND audit.installation_id = receipt_row.installation_id
                AND audit.principal_id = receipt_row.principal_id
                AND audit.action = 'promotion.apply'
                AND audit.target_resource_type = receipt_row.target_resource_type
                AND audit.target_resource_id = receipt_row.target_resource_id
                AND audit.resulting_state = receipt_row.resulting_state
                AND audit.result_code = receipt_row.result_code;

            SELECT authority.*
            INTO historical_authority_row
            FROM public.automation_installation_authority_versions AS authority
            WHERE authority.tenant_id = expected_tenant_id
                AND authority.installation_id = expected_installation_id
                AND authority.revision = audit_row.installation_authority_revision
            FOR SHARE;

            mutation_clock := pg_catalog.clock_timestamp();
            IF session_row.principal_id IS NULL
                OR session_row.oauth_state_digest IS NULL
                OR session_row.revoked_at IS NOT NULL
                OR mutation_clock >= session_row.idle_expires_at
                OR mutation_clock >= session_row.absolute_expires_at
                OR expected_authority_observed_at > mutation_clock
                OR mutation_clock >= expected_authority_expires_at
            THEN
                RETURN QUERY SELECT 'authorization_stale', FALSE, FALSE,
                    NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
                RETURN;
            END IF;

            IF installation_row.installation_id IS NULL
                OR authority_row.installation_id IS NULL
                OR installation_row.current_authority_revision
                    IS DISTINCT FROM expected_authority_revision
                OR authority_row.authority_payload_digest
                    IS DISTINCT FROM expected_authority_payload_digest
            THEN
                RETURN QUERY SELECT 'authority_mismatch', FALSE, FALSE,
                    NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
                RETURN;
            END IF;

            IF activation_row.id IS NOT NULL
                AND audit_row.event_id IS NOT NULL
                AND historical_authority_row.installation_id IS NOT NULL
                AND session_row.principal_id IS NOT NULL
                AND session_row.oauth_state_digest IS NOT NULL
                AND session_row.revoked_at IS NULL
                AND mutation_clock < session_row.idle_expires_at
                AND mutation_clock < session_row.absolute_expires_at
                AND expected_authority_observed_at <= mutation_clock
                AND mutation_clock < expected_authority_expires_at
                AND installation_row.current_authority_revision
                    = expected_authority_revision
                AND authority_row.authority_payload_digest
                    = expected_authority_payload_digest
                AND activation_row.state = 'superseded'
                AND activation_row.product_revision = receipt_row.resulting_revision
                AND activation_row.approval_payload_digest = expected_payload_digest
                AND activation_row.created_at <= receipt_row.completed_at
                AND receipt_row.completed_at < activation_row.expires_at
                AND audit_row.installation_authority_revision
                    = historical_authority_row.revision
                AND audit_row.payload_digest = expected_payload_digest
                AND audit_row.binding_fingerprint
                    = historical_authority_row.binding_fingerprint
                AND audit_row.policy_revision = historical_authority_row.policy_revision
                AND receipt_row.completed_at = audit_row.occurred_at
                AND NOT EXISTS (
                    SELECT 1
                    FROM public.runtime_deployments AS deployment
                    WHERE deployment.tenant_id = expected_tenant_id
                        AND deployment.installation_id = expected_installation_id
                        AND deployment.activation_request_id = activation_row.id
                )
                AND activation_row.termination = pg_catalog.jsonb_build_object(
                    'kind', 'superseded',
                    'at', receipt_row.completed_at,
                    'reason',
                    CASE receipt_row.result_code
                        WHEN 'superseded_baseline_drift' THEN
                            pg_catalog.jsonb_build_object(
                                'reason', 'active_baseline_drift',
                                'expected', CASE
                                    WHEN activation_row.observed_active_version IS NULL THEN
                                        pg_catalog.jsonb_build_object('state', 'absent')
                                    ELSE pg_catalog.jsonb_build_object(
                                        'state', 'exact',
                                        'version', activation_row.observed_active_version,
                                        'content_hash', activation_row.observed_active_hash
                                    )
                                END,
                                'observed', CASE
                                    WHEN audit_row.active_baseline_version IS NULL THEN
                                        pg_catalog.jsonb_build_object('state', 'absent')
                                    ELSE pg_catalog.jsonb_build_object(
                                        'state', 'exact',
                                        'version', audit_row.active_baseline_version,
                                        'content_hash', audit_row.active_baseline_hash
                                    )
                                END
                            )
                        WHEN 'superseded_binding_drift' THEN
                            pg_catalog.jsonb_build_object(
                                'reason', 'binding_drift',
                                'expected_revision', (
                                    activation_row.approval_context
                                        #>> '{context,binding,revision}'
                                )::BIGINT,
                                'observed_revision',
                                    historical_authority_row.binding_revision,
                                'expected_fingerprint',
                                    activation_row.approval_context
                                        #>> '{context,binding,fingerprint}',
                                'observed_fingerprint', 'null'::JSONB
                            )
                        WHEN 'superseded_policy_drift' THEN
                            pg_catalog.jsonb_build_object(
                                'reason', 'policy_drift',
                                'expected_revision', (
                                    activation_row.approval_context
                                        #>> '{context,policy,revision}'
                                )::BIGINT,
                                'observed_revision',
                                    historical_authority_row.policy_revision,
                                'expected_required_approvals', (
                                    activation_row.approval_context
                                        #>> '{context,policy,required_approvals}'
                                )::INTEGER,
                                'observed_required_approvals',
                                    historical_authority_row.required_approvals,
                                'expected_ttl_seconds', (
                                    activation_row.approval_context
                                        #>> '{context,policy,ttl_seconds}'
                                )::BIGINT,
                                'observed_ttl_seconds',
                                    historical_authority_row.activation_ttl_seconds
                            )
                    END
                )
                AND (
                    audit_row.active_baseline_version IS NULL
                ) = (
                    audit_row.active_baseline_hash IS NULL
                )
            THEN
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
                SELECT receipt_row.tenant_id,
                    receipt_row.installation_id,
                    receipt_row.principal_id,
                    receipt_row.endpoint_domain,
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

                RETURN QUERY SELECT 'superseded', TRUE, TRUE,
                    receipt_row.resulting_revision,
                    receipt_row.resulting_state,
                    NULL::TEXT,
                    NULL::TEXT,
                    NULL::JSONB;
                RETURN;
            END IF;
        END IF;

        RETURN QUERY SELECT core_row.outcome,
            core_row.exact_replay,
            core_row.requires_commit,
            core_row.resulting_revision,
            core_row.resulting_state,
            core_row.deployment_id,
            core_row.desired_target_digest,
            core_row.locked_projection;
        RETURN;
    END IF;

    mutation_clock := pg_catalog.clock_timestamp();

    SELECT activation.*
    INTO activation_row
    FROM public.activation_requests AS activation
    WHERE activation.tenant_id = expected_tenant_id
        AND activation.installation_id = expected_installation_id
        AND activation.promotion_id = expected_promotion_id
    FOR UPDATE;

    SELECT promotion.*
    INTO promotion_row
    FROM public.authoring_promotions AS promotion
    WHERE promotion.tenant_id = expected_tenant_id
        AND promotion.installation_id = expected_installation_id
        AND promotion.id = expected_promotion_id
    FOR SHARE;

    SELECT installation.*
    INTO installation_row
    FROM public.automation_installations AS installation
    WHERE installation.tenant_id = expected_tenant_id
        AND installation.installation_id = expected_installation_id
    FOR SHARE;

    SELECT authority.*
    INTO authority_row
    FROM public.automation_installation_authority_versions AS authority
    WHERE authority.tenant_id = expected_tenant_id
        AND authority.installation_id = expected_installation_id
        AND authority.revision = installation_row.current_authority_revision
    FOR SHARE;

    SELECT product_session.*
    INTO session_row
    FROM public.product_auth_sessions AS product_session
    WHERE product_session.session_digest = expected_product_session_digest
        AND product_session.principal_id = expected_principal_id
    FOR SHARE;

    SELECT historical_authority.*
    INTO historical_authority_row
    FROM public.automation_installation_authority_versions AS historical_authority
    WHERE historical_authority.tenant_id = expected_tenant_id
        AND historical_authority.installation_id = expected_installation_id
        AND historical_authority.binding_revision::TEXT
            = activation_row.approval_context #>> '{context,binding,revision}'
        AND historical_authority.binding_fingerprint
            = promotion_row.record #>> '{intent,evidence,context_fingerprint}'
        AND historical_authority.policy_revision::TEXT
            = activation_row.approval_context #>> '{context,policy,revision}'
        AND historical_authority.required_approvals::TEXT
            = activation_row.approval_context
                #>> '{context,policy,required_approvals}'
        AND historical_authority.activation_ttl_seconds::TEXT
            = activation_row.approval_context #>> '{context,policy,ttl_seconds}'
    ORDER BY historical_authority.revision
    LIMIT 1
    FOR SHARE;

    SELECT pg_catalog.count(*)
    INTO approval_count
    FROM public.activation_request_approvals AS approval
    WHERE approval.tenant_id = expected_tenant_id
        AND approval.installation_id = expected_installation_id
        AND approval.request_id = activation_row.id
        AND approval.approval_payload_digest = activation_row.approval_payload_digest;

    IF activation_row.id IS NULL
        OR promotion_row.id IS NULL
        OR installation_row.installation_id IS NULL
        OR authority_row.installation_id IS NULL
        OR historical_authority_row.installation_id IS NULL
        OR session_row.principal_id IS NULL
    THEN
        RETURN QUERY SELECT core_row.outcome,
            core_row.exact_replay,
            core_row.requires_commit,
            core_row.resulting_revision,
            core_row.resulting_state,
            core_row.deployment_id,
            core_row.desired_target_digest,
            core_row.locked_projection;
        RETURN;
    END IF;

    IF installation_row.lifecycle_state <> 'active'
        OR installation_row.discord_application_id
            IS DISTINCT FROM expected_discord_application_id
        OR installation_row.discord_guild_id IS DISTINCT FROM expected_guild_id
        OR session_row.oauth_state_digest IS NULL
        OR session_row.revoked_at IS NOT NULL
        OR mutation_clock >= session_row.idle_expires_at
        OR mutation_clock >= session_row.absolute_expires_at
        OR expected_authority_observed_at > mutation_clock
        OR mutation_clock >= expected_authority_expires_at
    THEN
        RETURN QUERY SELECT 'authorization_stale', FALSE, FALSE,
            NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;

    IF installation_row.current_authority_revision
            IS DISTINCT FROM expected_authority_revision
        OR authority_row.authority_payload_digest
            IS DISTINCT FROM expected_authority_payload_digest
    THEN
        RETURN QUERY SELECT core_row.outcome,
            core_row.exact_replay,
            core_row.requires_commit,
            core_row.resulting_revision,
            core_row.resulting_state,
            core_row.deployment_id,
            core_row.desired_target_digest,
            core_row.locked_projection;
        RETURN;
    END IF;

    IF activation_row.product_revision IS DISTINCT FROM expected_product_revision THEN
        RETURN QUERY SELECT 'revision_conflict', FALSE, FALSE,
            NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;

    IF activation_row.approval_payload_digest IS DISTINCT FROM expected_payload_digest THEN
        RETURN QUERY SELECT 'payload_mismatch', FALSE, FALSE,
            NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;

    IF activation_row.expires_at <= mutation_clock THEN
        RETURN QUERY SELECT 'expired', FALSE, FALSE,
            NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;

    IF activation_row.state <> 'approved'
        OR approval_count < activation_row.required_approvals
    THEN
        RETURN QUERY SELECT 'invalid_state', FALSE, FALSE,
            NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;

    IF activation_row.authority_kind <> 'product_authoring'
        OR activation_row.link_state_name <> 'linked'
        OR activation_row.guild_id IS DISTINCT FROM expected_guild_id
        OR activation_row.ruleset_key IS DISTINCT FROM installation_row.ruleset_key
        OR promotion_row.stage <> 'activation_pending'
        OR promotion_row.request_digest
            IS DISTINCT FROM activation_row.promotion_request_digest
        OR promotion_row.record #>> '{intent,authority,tenant_id}'
            IS DISTINCT FROM expected_tenant_id
        OR promotion_row.record #>> '{intent,authority,installation_id}'
            IS DISTINCT FROM expected_installation_id
        OR promotion_row.record #>> '{intent,authority,guild_id}'
            IS DISTINCT FROM expected_guild_id
        OR promotion_row.record #>> '{intent,authority,ruleset_key}'
            IS DISTINCT FROM activation_row.ruleset_key
        OR promotion_row.record #>> '{intent,authority,binding_revision}'
            IS DISTINCT FROM activation_row.approval_context
                #>> '{context,binding,revision}'
        OR promotion_row.record #>> '{stage,activation,request_id}'
            IS DISTINCT FROM activation_row.id
        OR promotion_row.record #>> '{stage,activation,target,guild_id}'
            IS DISTINCT FROM activation_row.guild_id
        OR promotion_row.record #>> '{stage,activation,target,ruleset_key}'
            IS DISTINCT FROM activation_row.ruleset_key
        OR promotion_row.record #>> '{stage,activation,target,version}'
            IS DISTINCT FROM activation_row.target_version::TEXT
        OR promotion_row.record #>> '{stage,activation,target,content_hash}'
            IS DISTINCT FROM activation_row.target_content_hash
        OR promotion_row.record #>> '{stage,activation,requester}'
            IS DISTINCT FROM activation_row.requester_id
        OR promotion_row.record #>> '{stage,activation,required_approvals}'
            IS DISTINCT FROM activation_row.required_approvals::TEXT
        OR promotion_row.record #> '{stage,activation,approval_context}'
            IS DISTINCT FROM activation_row.approval_context -> 'context'
        OR activation_row.approval_context #>> '{context,binding,fingerprint}'
            !~ '^[0-9a-f]{64}$'
        OR activation_row.approval_context #>> '{context,policy,digest}'
            !~ '^[0-9a-f]{64}$'
        OR historical_authority_row.binding_revision::TEXT
            IS DISTINCT FROM activation_row.approval_context
                #>> '{context,binding,revision}'
        OR historical_authority_row.binding_fingerprint
            IS DISTINCT FROM promotion_row.record
                #>> '{intent,evidence,context_fingerprint}'
        OR historical_authority_row.policy_revision::TEXT
            IS DISTINCT FROM activation_row.approval_context
                #>> '{context,policy,revision}'
        OR historical_authority_row.required_approvals::TEXT
            IS DISTINCT FROM activation_row.approval_context
                #>> '{context,policy,required_approvals}'
        OR historical_authority_row.activation_ttl_seconds::TEXT
            IS DISTINCT FROM activation_row.approval_context
                #>> '{context,policy,ttl_seconds}'
    THEN
        RETURN QUERY SELECT core_row.outcome,
            core_row.exact_replay,
            core_row.requires_commit,
            core_row.resulting_revision,
            core_row.resulting_state,
            core_row.deployment_id,
            core_row.desired_target_digest,
            core_row.locked_projection;
        RETURN;
    END IF;

    IF EXISTS (
        SELECT 1
        FROM public.product_audit_events AS audit
        WHERE audit.tenant_id = expected_tenant_id
            AND audit.request_id = product_request_id
    ) OR EXISTS (
        SELECT 1
        FROM public.product_action_receipts AS receipt
        WHERE receipt.receipt_id = new_receipt_id
    ) OR EXISTS (
        SELECT 1
        FROM public.product_audit_events AS audit
        WHERE audit.event_id = new_audit_event_id
    ) OR EXISTS (
        SELECT 1
        FROM public.runtime_deployments AS deployment
        WHERE deployment.deployment_id = new_deployment_id
    ) THEN
        RETURN QUERY SELECT 'idempotency_conflict', FALSE, FALSE,
            NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;

    SELECT version.schema_version,
        version.content_hash
    INTO target_schema_version,
        target_content_hash
    FROM public.automation_ruleset_versions AS version
    WHERE version.guild_id = activation_row.guild_id
        AND version.ruleset_key = activation_row.ruleset_key
        AND version.version = activation_row.target_version
    FOR SHARE;

    IF target_schema_version IS NULL
        OR target_content_hash IS DISTINCT FROM activation_row.target_content_hash
    THEN
        RETURN QUERY SELECT 'target_mismatch', FALSE, FALSE,
            NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;

    SELECT active.active_version,
        version.content_hash
    INTO current_active_version,
        current_active_hash
    FROM public.automation_ruleset_activations AS active
    INNER JOIN public.automation_ruleset_versions AS version
        ON version.guild_id = active.guild_id
        AND version.ruleset_key = active.ruleset_key
        AND version.version = active.active_version
    WHERE active.guild_id = expected_guild_id
        AND active.ruleset_key = activation_row.ruleset_key
    FOR UPDATE OF active;

    target_is_active := current_active_version
            IS NOT DISTINCT FROM activation_row.target_version
        AND current_active_hash IS NOT DISTINCT FROM activation_row.target_content_hash;
    baseline_drift := NOT target_is_active AND (
        current_active_version IS DISTINCT FROM activation_row.observed_active_version
        OR current_active_hash IS DISTINCT FROM activation_row.observed_active_hash
    );
    binding_drift := authority_row.binding_revision::TEXT
            IS DISTINCT FROM activation_row.approval_context
                #>> '{context,binding,revision}'
        OR authority_row.binding_fingerprint
            IS DISTINCT FROM historical_authority_row.binding_fingerprint
        OR authority_row.resource_bindings
            IS DISTINCT FROM historical_authority_row.resource_bindings;
    policy_drift := authority_row.policy_revision::TEXT
            IS DISTINCT FROM activation_row.approval_context
                #>> '{context,policy,revision}'
        OR authority_row.required_approvals::TEXT
            IS DISTINCT FROM activation_row.approval_context
                #>> '{context,policy,required_approvals}'
        OR authority_row.activation_ttl_seconds::TEXT
            IS DISTINCT FROM activation_row.approval_context
                #>> '{context,policy,ttl_seconds}';

    IF NOT baseline_drift AND NOT binding_drift AND NOT policy_drift THEN
        RETURN QUERY SELECT core_row.outcome,
            core_row.exact_replay,
            core_row.requires_commit,
            core_row.resulting_revision,
            core_row.resulting_state,
            core_row.deployment_id,
            core_row.desired_target_digest,
            core_row.locked_projection;
        RETURN;
    END IF;

    IF expected_product_revision > 9223372036854775805 THEN
        RETURN QUERY SELECT 'revision_overflow', FALSE, FALSE,
            NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;

    IF baseline_drift THEN
        result_code := 'superseded_baseline_drift';
        reason_record := pg_catalog.jsonb_build_object(
            'reason', 'active_baseline_drift',
            'expected', CASE
                WHEN activation_row.observed_active_version IS NULL THEN
                    pg_catalog.jsonb_build_object('state', 'absent')
                ELSE pg_catalog.jsonb_build_object(
                    'state', 'exact',
                    'version', activation_row.observed_active_version,
                    'content_hash', activation_row.observed_active_hash
                )
            END,
            'observed', CASE
                WHEN current_active_version IS NULL THEN
                    pg_catalog.jsonb_build_object('state', 'absent')
                ELSE pg_catalog.jsonb_build_object(
                    'state', 'exact',
                    'version', current_active_version,
                    'content_hash', current_active_hash
                )
            END
        );
    ELSIF binding_drift THEN
        result_code := 'superseded_binding_drift';
        reason_record := pg_catalog.jsonb_build_object(
            'reason', 'binding_drift',
            'expected_revision', (
                activation_row.approval_context
                    #>> '{context,binding,revision}'
            )::BIGINT,
            'observed_revision', authority_row.binding_revision,
            'expected_fingerprint', activation_row.approval_context
                #>> '{context,binding,fingerprint}',
            'observed_fingerprint', 'null'::JSONB
        );
    ELSE
        result_code := 'superseded_policy_drift';
        reason_record := pg_catalog.jsonb_build_object(
            'reason', 'policy_drift',
            'expected_revision', (
                activation_row.approval_context
                    #>> '{context,policy,revision}'
            )::BIGINT,
            'observed_revision', authority_row.policy_revision,
            'expected_required_approvals', (
                activation_row.approval_context
                    #>> '{context,policy,required_approvals}'
            )::INTEGER,
            'observed_required_approvals', authority_row.required_approvals,
            'expected_ttl_seconds', (
                activation_row.approval_context
                    #>> '{context,policy,ttl_seconds}'
            )::BIGINT,
            'observed_ttl_seconds', authority_row.activation_ttl_seconds
        );
    END IF;

    IF EXISTS (
        SELECT 1
        FROM public.runtime_deployments AS deployment
        WHERE deployment.tenant_id = expected_tenant_id
            AND deployment.installation_id = expected_installation_id
            AND deployment.activation_request_id = activation_row.id
    ) THEN
        RETURN QUERY SELECT 'indeterminate', FALSE, FALSE,
            NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;

    mutation_clock := pg_catalog.clock_timestamp();
    IF session_row.oauth_state_digest IS NULL
        OR session_row.revoked_at IS NOT NULL
        OR mutation_clock >= session_row.idle_expires_at
        OR mutation_clock >= session_row.absolute_expires_at
        OR expected_authority_observed_at > mutation_clock
        OR mutation_clock >= expected_authority_expires_at
    THEN
        RETURN QUERY SELECT 'authorization_stale', FALSE, FALSE,
            NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;

    IF installation_row.current_authority_revision
            IS DISTINCT FROM expected_authority_revision
        OR authority_row.authority_payload_digest
            IS DISTINCT FROM expected_authority_payload_digest
    THEN
        RETURN QUERY SELECT 'authority_mismatch', FALSE, FALSE,
            NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;

    IF activation_row.expires_at <= mutation_clock THEN
        RETURN QUERY SELECT 'expired', FALSE, FALSE,
            NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB;
        RETURN;
    END IF;

    termination_record := pg_catalog.jsonb_build_object(
        'kind', 'superseded',
        'at', mutation_clock,
        'reason', reason_record
    );
    next_revision := expected_product_revision + 2;

    PERFORM pg_catalog.set_config(
        'starring.product_approval_context_digest',
        activation_row.approval_context_digest,
        TRUE
    );
    UPDATE public.activation_requests AS activation
    SET state = 'applying',
        apply_attempt_id = new_apply_attempt_id,
        apply_attempt_no = activation.apply_attempt_no + 1,
        apply_lease_until = mutation_clock + INTERVAL '60 seconds',
        last_apply_error = NULL,
        product_revision = activation.product_revision + 1
    WHERE activation.tenant_id = expected_tenant_id
        AND activation.installation_id = expected_installation_id
        AND activation.id = activation_row.id
        AND activation.promotion_id = expected_promotion_id
        AND activation.state = 'approved'
        AND activation.product_revision = expected_product_revision
        AND activation.approval_payload_digest = expected_payload_digest;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'atomic product apply supersession claim compare-and-swap failed'
            USING ERRCODE = '40001';
    END IF;
    PERFORM pg_catalog.set_config(
        'starring.product_approval_context_digest',
        '',
        TRUE
    );

    UPDATE public.activation_requests AS activation
    SET state = 'superseded',
        apply_attempt_id = NULL,
        apply_lease_until = NULL,
        last_apply_error = NULL,
        termination = termination_record,
        product_revision = activation.product_revision + 1
    WHERE activation.tenant_id = expected_tenant_id
        AND activation.installation_id = expected_installation_id
        AND activation.id = activation_row.id
        AND activation.promotion_id = expected_promotion_id
        AND activation.state = 'applying'
        AND activation.apply_attempt_id = new_apply_attempt_id
        AND activation.product_revision = expected_product_revision + 1;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'atomic product apply supersession compare-and-swap failed'
            USING ERRCODE = '40001';
    END IF;

    INSERT INTO public.product_action_receipts (
        receipt_id,
        tenant_id,
        installation_id,
        principal_id,
        endpoint_domain,
        idempotency_key_digest,
        idempotency_digest_key_id,
        idempotency_digest_key_fingerprint,
        request_digest,
        target_resource_type,
        target_resource_id,
        resulting_revision,
        resulting_state,
        result_code,
        http_disposition_class,
        completed_at
    ) VALUES (
        new_receipt_id,
        expected_tenant_id,
        expected_installation_id,
        expected_principal_id,
        'product_apply_v1',
        active_idempotency_key_digest,
        idempotency_digest_key_id,
        idempotency_digest_key_fingerprint_candidates[1],
        semantic_request_digest,
        'authoring_promotion',
        expected_promotion_id,
        next_revision,
        'superseded',
        result_code,
        4,
        mutation_clock
    );

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
        'product_apply_v1',
        idempotency_key_digest_candidates[candidate.ordinal],
        idempotency_digest_key_id_candidates[candidate.ordinal],
        idempotency_digest_key_fingerprint_candidates[candidate.ordinal],
        new_receipt_id,
        mutation_clock
    FROM pg_catalog.generate_subscripts(
        idempotency_key_digest_candidates,
        1
    ) AS candidate(ordinal);

    active_baseline_version := current_active_version;
    active_baseline_hash := current_active_hash;
    INSERT INTO public.product_audit_events (
        event_id,
        tenant_id,
        installation_id,
        principal_id,
        session_subject_digest,
        action,
        target_resource_type,
        target_resource_id,
        request_id,
        receipt_id,
        authority_observation_digest,
        effective_permission_bits,
        authority_observed_at,
        installation_authority_revision,
        payload_digest,
        binding_fingerprint,
        policy_revision,
        active_baseline_version,
        active_baseline_hash,
        resulting_state,
        result_code,
        dependency_latency_classes,
        occurred_at
    ) VALUES (
        new_audit_event_id,
        expected_tenant_id,
        expected_installation_id,
        expected_principal_id,
        session_subject_digest,
        'promotion.apply',
        'authoring_promotion',
        expected_promotion_id,
        product_request_id,
        new_receipt_id,
        expected_authority_observation_digest,
        expected_effective_permission_bits::NUMERIC,
        expected_authority_observed_at,
        authority_row.revision,
        expected_payload_digest,
        authority_row.binding_fingerprint,
        authority_row.policy_revision,
        active_baseline_version,
        active_baseline_hash,
        'superseded',
        result_code,
        '{}'::JSONB,
        mutation_clock
    );

    RETURN QUERY SELECT 'superseded', FALSE, TRUE,
        next_revision,
        'superseded',
        NULL::TEXT,
        NULL::TEXT,
        NULL::JSONB;
END;
$function$;

REVOKE ALL ON FUNCTION public.starring_product_apply_lock_v1(
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

DO $privilege_transfer$
DECLARE
    grantee_name TEXT;
    privilege_grantees OID[];
    privilege_grantable BOOLEAN[];
    privilege_index INTEGER;
    function_owner OID;
    finalizer_owner OID;
    function_owner_name TEXT;
    core_identity CONSTANT TEXT :=
        'public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)';
    wrapper_identity CONSTANT TEXT :=
        'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)';
    finalizer_identity CONSTANT TEXT :=
        'public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)';
BEGIN
    SELECT core.proowner,
        finalizer.proowner
    INTO function_owner,
        finalizer_owner
    FROM pg_catalog.pg_proc AS core
    CROSS JOIN pg_catalog.pg_proc AS finalizer
    WHERE core.oid = pg_catalog.to_regprocedure(core_identity)
        AND finalizer.oid = pg_catalog.to_regprocedure(finalizer_identity);

    IF function_owner IS NULL
        OR finalizer_owner IS NULL
        OR function_owner <> finalizer_owner
    THEN
        RAISE EXCEPTION 'product apply lock and finalizer owners are inconsistent'
            USING ERRCODE = '55000';
    END IF;

    function_owner_name := pg_catalog.pg_get_userbyid(function_owner);
    IF function_owner_name IS NULL THEN
        RAISE EXCEPTION 'product apply function owner is unavailable'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.array_agg(grant_row.grantee ORDER BY grant_row.grantee),
        pg_catalog.array_agg(grant_row.is_grantable ORDER BY grant_row.grantee)
    INTO privilege_grantees,
        privilege_grantable
    FROM (
        SELECT privilege.grantee,
            pg_catalog.bool_or(privilege.is_grantable) AS is_grantable
        FROM pg_catalog.pg_proc AS function_row
        CROSS JOIN LATERAL pg_catalog.aclexplode(
            COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )
        ) AS privilege
        WHERE function_row.oid = pg_catalog.to_regprocedure(core_identity)
            AND privilege.privilege_type = 'EXECUTE'
            AND privilege.grantee <> 0
            AND privilege.grantee <> function_row.proowner
        GROUP BY privilege.grantee
    ) AS grant_row;

    EXECUTE pg_catalog.format(
        'ALTER FUNCTION %s OWNER TO %I',
        wrapper_identity,
        function_owner_name
    );

    IF pg_catalog.cardinality(privilege_grantees) > 0 THEN
        FOR privilege_index IN 1..pg_catalog.cardinality(privilege_grantees)
        LOOP
            grantee_name := pg_catalog.pg_get_userbyid(
                privilege_grantees[privilege_index]
            );
            IF grantee_name IS NULL THEN
                RAISE EXCEPTION 'product apply lock privilege grantee is unavailable'
                    USING ERRCODE = '55000';
            END IF;
            EXECUTE pg_catalog.format(
                'GRANT EXECUTE ON FUNCTION %s TO %I%s',
                wrapper_identity,
                grantee_name,
                CASE
                    WHEN privilege_grantable[privilege_index] THEN
                        ' WITH GRANT OPTION'
                    ELSE ''
                END
            );
        END LOOP;

        FOR privilege_index IN 1..pg_catalog.cardinality(privilege_grantees)
        LOOP
            grantee_name := pg_catalog.pg_get_userbyid(
                privilege_grantees[privilege_index]
            );
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM %I CASCADE',
                core_identity,
                grantee_name
            );
        END LOOP;
    END IF;
END;
$privilege_transfer$;
