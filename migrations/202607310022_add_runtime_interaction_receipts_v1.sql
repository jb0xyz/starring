SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
SET LOCAL search_path = pg_catalog;

LOCK TABLE
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
    WHERE relation.oid = pg_catalog.to_regclass('public.automation_instances')
        AND relation.relkind = 'r'
        AND relation.relpersistence = 'p'
        AND NOT relation.relrowsecurity
        AND NOT relation.relforcerowsecurity;

    IF NOT FOUND
        OR common_owner IS NULL
        OR pg_catalog.to_regrole(current_user) <> common_owner
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
        OR pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_database_readiness_v1()'
        ) IS NULL
        OR pg_catalog.to_regprocedure(
            'public.starring_runtime_interaction_instance_scan_retryable_v2(text,text,text,text,bigint)'
        ) IS NULL
        OR EXISTS (
            SELECT 1
            FROM (
                VALUES
                    ('public.automation_installations'),
                    ('public.runtime_deployments'),
                    ('public.runtime_attestations'),
                    ('public.runtime_serving_leases'),
                    ('public.runtime_gateway_owners'),
                    ('public.automation_ruleset_versions')
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
        RAISE EXCEPTION 'runtime interaction receipt preflight failed'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_class AS relation
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = relation.relnamespace
    WHERE namespace.nspname = 'public'
        AND relation.relname IN (
            'runtime_interaction_receipt_roots_v1',
            'runtime_interaction_receipt_heads_v1',
            'runtime_interaction_receipt_events_v1',
            'runtime_interaction_receipt_token_secrets_v1',
            'runtime_interaction_receipt_heads_recovery_v1_idx',
            'runtime_interaction_receipt_token_expiry_v1_idx'
        );

    IF collision_count <> 0 THEN
        RAISE EXCEPTION 'runtime interaction receipt relation collision exists'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO collision_count
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            'guard_runtime_interaction_receipt_root_v1',
            'guard_runtime_interaction_receipt_head_v1',
            'guard_runtime_interaction_receipt_event_v1',
            'guard_runtime_interaction_receipt_token_v1',
            'starring_runtime_interaction_receipt_schema_manifest_v1',
            'starring_runtime_interaction_receipt_authority_observe_v1',
            'starring_runtime_interaction_receipt_claim_current_v1',
            'starring_runtime_interaction_receipt_claim_v1',
            'starring_runtime_interaction_receipt_plan_bind_v1',
            'starring_runtime_interaction_receipt_acknowledgement_intend_v1',
            'starring_runtime_interaction_receipt_acknowledgement_finish_v1',
            'starring_runtime_interaction_receipt_execution_intend_v1',
            'starring_runtime_interaction_receipt_finish_v1',
            'starring_runtime_interaction_receipt_scan_recoverable_v1',
            'starring_runtime_interaction_receipt_recover_v1',
            'starring_runtime_interaction_receipt_token_expire_v1'
        );

    IF collision_count <> 0 THEN
        RAISE EXCEPTION 'runtime interaction receipt function collision exists'
            USING ERRCODE = '55000';
    END IF;
END;
$preflight$;

CREATE TABLE public.runtime_interaction_receipt_roots_v1 (
    application_id TEXT NOT NULL,
    interaction_id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    installation_id TEXT NOT NULL,
    guild_id TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    actor_user_id TEXT NOT NULL,
    interaction_kind TEXT NOT NULL,
    ruleset_key TEXT NOT NULL,
    target_version BIGINT NOT NULL,
    target_content_hash TEXT NOT NULL,
    binding_revision BIGINT NOT NULL,
    binding_fingerprint TEXT NOT NULL,
    deployment_id TEXT NOT NULL,
    attestation_id TEXT NOT NULL,
    attestation_digest TEXT NOT NULL,
    runtime_generation BIGINT NOT NULL,
    route_controller_fencing_token BIGINT NOT NULL,
    route_incarnation BIGINT NOT NULL,
    origin_process_instance_id TEXT NOT NULL,
    origin_serving_lease_epoch BIGINT NOT NULL,
    origin_serving_revision BIGINT NOT NULL,
    origin_gateway_shard_id TEXT NOT NULL,
    origin_gateway_owner_lease_epoch BIGINT NOT NULL,
    origin_gateway_owner_revision BIGINT NOT NULL,
    runtime_build_revision TEXT NOT NULL,
    route_kind TEXT NOT NULL,
    route_key TEXT NOT NULL,
    instance_id TEXT,
    execution_ruleset_version BIGINT NOT NULL,
    execution_ruleset_content_hash TEXT NOT NULL,
    instance_manifest_digest TEXT,
    request_digest BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT runtime_interaction_receipt_roots_v1_pk PRIMARY KEY (
        application_id,
        interaction_id
    ),
    CONSTRAINT runtime_interaction_receipt_roots_v1_installation_fk FOREIGN KEY (
        tenant_id,
        installation_id
    ) REFERENCES public.automation_installations (
        tenant_id,
        installation_id
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_interaction_receipt_roots_v1_attestation_fk FOREIGN KEY (
        tenant_id,
        installation_id,
        deployment_id,
        attestation_id
    ) REFERENCES public.runtime_attestations (
        tenant_id,
        installation_id,
        deployment_id,
        attestation_id
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_interaction_receipt_roots_v1_target_fk FOREIGN KEY (
        guild_id,
        ruleset_key,
        target_version
    ) REFERENCES public.automation_ruleset_versions (
        guild_id,
        ruleset_key,
        version
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_interaction_receipt_roots_v1_execution_target_fk FOREIGN KEY (
        guild_id,
        ruleset_key,
        execution_ruleset_version
    ) REFERENCES public.automation_ruleset_versions (
        guild_id,
        ruleset_key,
        version
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_interaction_receipt_roots_v1_discord_identity_check CHECK (
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
        AND guild_id ~ '^[1-9][0-9]{0,19}$'
        AND pg_catalog.length(guild_id) <= 20
        AND (
            pg_catalog.length(guild_id) < 20
            OR guild_id <= '18446744073709551615'
        )
        AND channel_id ~ '^[1-9][0-9]{0,19}$'
        AND pg_catalog.length(channel_id) <= 20
        AND (
            pg_catalog.length(channel_id) < 20
            OR channel_id <= '18446744073709551615'
        )
        AND actor_user_id ~ '^[1-9][0-9]{0,19}$'
        AND pg_catalog.length(actor_user_id) <= 20
        AND (
            pg_catalog.length(actor_user_id) < 20
            OR actor_user_id <= '18446744073709551615'
        )
    ),
    CONSTRAINT runtime_interaction_receipt_roots_v1_product_identity_check CHECK (
        tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND deployment_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND attestation_id ~ '^[0-9a-f]{64}$'
        AND attestation_digest ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT runtime_interaction_receipt_roots_v1_target_check CHECK (
        ruleset_key ~ '^[A-Za-z0-9_-]{1,64}$'
        AND target_version BETWEEN 1 AND 4294967295
        AND target_content_hash ~ '^[0-9a-f]{64}$'
        AND binding_revision BETWEEN 1 AND 9223372036854775807
        AND binding_fingerprint ~ '^[0-9a-f]{64}$'
        AND runtime_generation BETWEEN 1 AND 9223372036854775807
        AND route_controller_fencing_token
            BETWEEN 1 AND 9223372036854775807
        AND route_incarnation BETWEEN 1 AND 9223372036854775807
        AND execution_ruleset_version BETWEEN 1 AND 4294967295
        AND execution_ruleset_content_hash ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT runtime_interaction_receipt_roots_v1_fence_check CHECK (
        origin_process_instance_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND origin_serving_lease_epoch BETWEEN 1 AND 9223372036854775807
        AND origin_serving_revision BETWEEN 1 AND 9223372036854775807
        AND origin_gateway_shard_id ~ '^[A-Za-z0-9_.:/-]{1,128}$'
        AND origin_gateway_owner_lease_epoch BETWEEN 1 AND 9223372036854775807
        AND origin_gateway_owner_revision BETWEEN 1 AND 9223372036854775807
        AND runtime_build_revision ~ '^[A-Za-z0-9_.:/-]{1,128}$'
    ),
    CONSTRAINT runtime_interaction_receipt_roots_v1_route_check CHECK (
        interaction_kind IN ('message_component', 'modal_submit')
        AND route_kind IN ('static', 'instance')
        AND pg_catalog.octet_length(route_key) BETWEEN 1 AND 100
        AND route_key = pg_catalog.btrim(route_key)
        AND route_key !~ '[[:cntrl:]]'
        AND (
            (
                route_kind = 'static'
                AND instance_id IS NULL
                AND execution_ruleset_version = target_version
                AND execution_ruleset_content_hash = target_content_hash
                AND instance_manifest_digest IS NULL
            )
            OR (
                route_kind = 'instance'
                AND instance_id ~ '^[A-Za-z0-9_-]{1,32}$'
                AND instance_manifest_digest ~ '^[0-9a-f]{64}$'
            )
        )
    ),
    CONSTRAINT runtime_interaction_receipt_roots_v1_digest_check CHECK (
        pg_catalog.octet_length(request_digest) = 32
    ),
    CONSTRAINT runtime_interaction_receipt_roots_v1_time_check CHECK (
        pg_catalog.isfinite(created_at)
    )
);

CREATE TABLE public.runtime_interaction_receipt_heads_v1 (
    application_id TEXT NOT NULL,
    interaction_id TEXT NOT NULL,
    state TEXT NOT NULL,
    acknowledgement_state TEXT NOT NULL,
    head_revision BIGINT NOT NULL,
    claim_revision BIGINT NOT NULL,
    claim_process_instance_id TEXT NOT NULL,
    claim_gateway_shard_id TEXT NOT NULL,
    claim_gateway_owner_lease_epoch BIGINT NOT NULL,
    claim_gateway_owner_revision BIGINT NOT NULL,
    claim_serving_lease_epoch BIGINT NOT NULL,
    claim_serving_revision BIGINT NOT NULL,
    claim_acquired_at TIMESTAMPTZ NOT NULL,
    claim_expires_at TIMESTAMPTZ NOT NULL,
    action_plan_digest BYTEA,
    acknowledgement_kind TEXT,
    acknowledgement_digest BYTEA,
    acknowledgement_intended_at TIMESTAMPTZ,
    acknowledgement_result TEXT,
    acknowledgement_result_digest BYTEA,
    acknowledged_at TIMESTAMPTZ,
    execution_intended_at TIMESTAMPTZ,
    terminal_outcome_code TEXT,
    terminal_result_digest BYTEA,
    terminal_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT runtime_interaction_receipt_heads_v1_pk PRIMARY KEY (
        application_id,
        interaction_id
    ),
    CONSTRAINT runtime_interaction_receipt_heads_v1_root_fk FOREIGN KEY (
        application_id,
        interaction_id
    ) REFERENCES public.runtime_interaction_receipt_roots_v1 (
        application_id,
        interaction_id
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_interaction_receipt_heads_v1_state_check CHECK (
        state IN (
            'claimed',
            'acknowledging',
            'deferred',
            'prepared',
            'executing',
            'completed',
            'failed',
            'recovery_required'
        )
    ),
    CONSTRAINT runtime_interaction_receipt_heads_v1_revision_check CHECK (
        head_revision BETWEEN 1 AND 9223372036854775807
        AND claim_revision BETWEEN 1 AND 9223372036854775807
        AND claim_revision <= head_revision
    ),
    CONSTRAINT runtime_interaction_receipt_heads_v1_claim_check CHECK (
        claim_process_instance_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND claim_gateway_shard_id ~ '^[A-Za-z0-9_.:/-]{1,128}$'
        AND claim_gateway_owner_lease_epoch BETWEEN 1 AND 9223372036854775807
        AND claim_gateway_owner_revision BETWEEN 1 AND 9223372036854775807
        AND claim_serving_lease_epoch BETWEEN 1 AND 9223372036854775807
        AND claim_serving_revision BETWEEN 1 AND 9223372036854775807
        AND pg_catalog.isfinite(claim_acquired_at)
        AND pg_catalog.isfinite(claim_expires_at)
        AND claim_acquired_at < claim_expires_at
        AND claim_expires_at <= claim_acquired_at + INTERVAL '5 minutes'
    ),
    CONSTRAINT runtime_interaction_receipt_heads_v1_plan_check CHECK (
        action_plan_digest IS NULL
        OR pg_catalog.octet_length(action_plan_digest) = 32
    ),
    CONSTRAINT runtime_interaction_receipt_heads_v1_acknowledgement_check CHECK (
        (
            acknowledgement_kind IS NULL
            AND acknowledgement_digest IS NULL
            AND acknowledgement_intended_at IS NULL
            AND acknowledgement_result IS NULL
            AND acknowledgement_result_digest IS NULL
            AND acknowledged_at IS NULL
        )
        OR (
            acknowledgement_kind IN (
                'defer_ephemeral',
                'respond_ephemeral',
                'respond_message',
                'open_modal',
                'update_message'
            )
            AND pg_catalog.octet_length(acknowledgement_digest) = 32
            AND acknowledgement_intended_at IS NOT NULL
            AND pg_catalog.isfinite(acknowledgement_intended_at)
            AND (
                (
                    acknowledgement_result IS NULL
                    AND acknowledgement_result_digest IS NULL
                    AND acknowledged_at IS NULL
                )
                OR (
                    acknowledgement_result IN (
                        'succeeded',
                        'definitive_failure',
                        'indeterminate'
                    )
                    AND pg_catalog.octet_length(
                        acknowledgement_result_digest
                    ) = 32
                    AND acknowledged_at IS NOT NULL
                    AND pg_catalog.isfinite(acknowledged_at)
                    AND acknowledgement_intended_at <= acknowledged_at
                )
            )
        )
    ),
    CONSTRAINT runtime_interaction_receipt_heads_v1_execution_check CHECK (
        execution_intended_at IS NULL
        OR (
            pg_catalog.isfinite(execution_intended_at)
            AND action_plan_digest IS NOT NULL
            AND claim_acquired_at <= execution_intended_at
        )
    ),
    CONSTRAINT runtime_interaction_receipt_heads_v1_terminal_check CHECK (
        (
            state IN (
                'claimed',
                'acknowledging',
                'deferred',
                'prepared',
                'executing'
            )
            AND terminal_outcome_code IS NULL
            AND terminal_result_digest IS NULL
            AND terminal_at IS NULL
        )
        OR (
            state IN ('completed', 'failed', 'recovery_required')
            AND terminal_outcome_code ~ '^[a-z0-9_]{1,64}$'
            AND pg_catalog.octet_length(terminal_result_digest) = 32
            AND terminal_at IS NOT NULL
            AND pg_catalog.isfinite(terminal_at)
        )
    ),
    CONSTRAINT runtime_interaction_receipt_heads_v1_state_shape_check CHECK (
        acknowledgement_state IN (
            'unacknowledged',
            'attempting',
            'deferred',
            'responded',
            'response_recovery_terminal'
        )
        AND (
            state <> 'claimed'
            OR (
                acknowledgement_state = 'unacknowledged'
                AND acknowledgement_kind IS NULL
            )
        )
        AND (
            state <> 'acknowledging'
            OR (
                acknowledgement_state = 'attempting'
                AND acknowledgement_kind IS NOT NULL
                AND acknowledgement_result IS NULL
            )
        )
        AND (
            state <> 'deferred'
            OR (
                acknowledgement_state = 'deferred'
                AND acknowledgement_kind = 'defer_ephemeral'
                AND acknowledgement_result = 'succeeded'
            )
        )
        AND (
            state <> 'prepared'
            OR (
                action_plan_digest IS NOT NULL
                AND acknowledgement_state IN (
                    'unacknowledged',
                    'deferred',
                    'responded'
                )
            )
        )
        AND (
            state <> 'executing'
            OR (
                action_plan_digest IS NOT NULL
                AND execution_intended_at IS NOT NULL
                AND acknowledgement_state IN (
                    'unacknowledged',
                    'attempting',
                    'deferred',
                    'responded'
                )
            )
        )
        AND (
            state <> 'completed'
            OR (
                action_plan_digest IS NOT NULL
                AND acknowledgement_state IN (
                    'unacknowledged',
                    'deferred',
                    'responded'
                )
            )
        )
        AND (
            acknowledgement_result IS NOT NULL
            OR acknowledgement_state IN ('unacknowledged', 'attempting')
        )
        AND (
            acknowledgement_result IS NULL
            OR acknowledgement_state <> 'attempting'
        )
        AND (
            acknowledgement_result <> 'succeeded'
            OR acknowledgement_state = CASE
                WHEN acknowledgement_kind = 'defer_ephemeral'
                    THEN 'deferred'
                ELSE 'responded'
            END
        )
        AND (
            acknowledgement_result NOT IN (
                'definitive_failure',
                'indeterminate'
            )
            OR acknowledgement_state = 'response_recovery_terminal'
        )
    ),
    CONSTRAINT runtime_interaction_receipt_heads_v1_time_check CHECK (
        pg_catalog.isfinite(updated_at)
        AND claim_acquired_at <= updated_at
        AND (
            acknowledgement_intended_at IS NULL
            OR acknowledgement_intended_at <= updated_at
        )
        AND (acknowledged_at IS NULL OR acknowledged_at <= updated_at)
        AND (
            execution_intended_at IS NULL
            OR execution_intended_at <= updated_at
        )
        AND (terminal_at IS NULL OR terminal_at <= updated_at)
    )
);

CREATE TABLE public.runtime_interaction_receipt_events_v1 (
    application_id TEXT NOT NULL,
    interaction_id TEXT NOT NULL,
    event_revision BIGINT NOT NULL,
    event_kind TEXT NOT NULL,
    from_state TEXT,
    to_state TEXT NOT NULL,
    from_acknowledgement_state TEXT,
    to_acknowledgement_state TEXT NOT NULL,
    claim_revision BIGINT NOT NULL,
    claim_process_instance_id TEXT NOT NULL,
    claim_gateway_shard_id TEXT NOT NULL,
    claim_gateway_owner_lease_epoch BIGINT NOT NULL,
    claim_gateway_owner_revision BIGINT NOT NULL,
    claim_serving_lease_epoch BIGINT NOT NULL,
    claim_serving_revision BIGINT NOT NULL,
    outcome_code TEXT NOT NULL,
    event_digest BYTEA NOT NULL,
    observed_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT runtime_interaction_receipt_events_v1_pk PRIMARY KEY (
        application_id,
        interaction_id,
        event_revision
    ),
    CONSTRAINT runtime_interaction_receipt_events_v1_root_fk FOREIGN KEY (
        application_id,
        interaction_id
    ) REFERENCES public.runtime_interaction_receipt_roots_v1 (
        application_id,
        interaction_id
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_interaction_receipt_events_v1_revision_check CHECK (
        event_revision BETWEEN 1 AND 9223372036854775807
        AND claim_revision BETWEEN 1 AND event_revision
    ),
    CONSTRAINT runtime_interaction_receipt_events_v1_kind_check CHECK (
        event_kind IN (
            'claimed',
            'plan_bound',
            'acknowledgement_intended',
            'acknowledgement_succeeded',
            'acknowledgement_failed',
            'acknowledgement_indeterminate',
            'execution_intended',
            'completed',
            'failed',
            'recovery_required',
            'claim_recovered',
            'claim_recovered_acknowledged',
            'interaction_token_expired'
        )
        AND (
            from_state IS NULL
            OR from_state IN (
                'claimed',
                'acknowledging',
                'deferred',
                'prepared',
                'executing',
                'completed',
                'failed',
                'recovery_required'
            )
        )
        AND to_state IN (
            'claimed',
            'acknowledging',
            'deferred',
            'prepared',
            'executing',
            'completed',
            'failed',
            'recovery_required'
        )
        AND (
            from_acknowledgement_state IS NULL
            OR from_acknowledgement_state IN (
                'unacknowledged',
                'attempting',
                'deferred',
                'responded',
                'response_recovery_terminal'
            )
        )
        AND to_acknowledgement_state IN (
            'unacknowledged',
            'attempting',
            'deferred',
            'responded',
            'response_recovery_terminal'
        )
    ),
    CONSTRAINT runtime_interaction_receipt_events_v1_claim_check CHECK (
        claim_process_instance_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND claim_gateway_shard_id ~ '^[A-Za-z0-9_.:/-]{1,128}$'
        AND claim_gateway_owner_lease_epoch BETWEEN 1 AND 9223372036854775807
        AND claim_gateway_owner_revision BETWEEN 1 AND 9223372036854775807
        AND claim_serving_lease_epoch BETWEEN 1 AND 9223372036854775807
        AND claim_serving_revision BETWEEN 1 AND 9223372036854775807
    ),
    CONSTRAINT runtime_interaction_receipt_events_v1_outcome_check CHECK (
        outcome_code ~ '^[a-z0-9_]{1,64}$'
        AND pg_catalog.octet_length(event_digest) = 32
    ),
    CONSTRAINT runtime_interaction_receipt_events_v1_time_check CHECK (
        pg_catalog.isfinite(observed_at)
    )
);

CREATE TABLE public.runtime_interaction_receipt_token_secrets_v1 (
    application_id TEXT NOT NULL,
    interaction_id TEXT NOT NULL,
    encryption_suite TEXT NOT NULL,
    suite_version SMALLINT NOT NULL,
    key_id TEXT NOT NULL,
    nonce BYTEA NOT NULL,
    ciphertext BYTEA NOT NULL,
    aad_digest BYTEA NOT NULL,
    issued_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT runtime_interaction_receipt_token_secrets_v1_pk PRIMARY KEY (
        application_id,
        interaction_id
    ),
    CONSTRAINT runtime_interaction_receipt_token_secrets_v1_root_fk FOREIGN KEY (
        application_id,
        interaction_id
    ) REFERENCES public.runtime_interaction_receipt_roots_v1 (
        application_id,
        interaction_id
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_interaction_receipt_token_secrets_v1_envelope_check CHECK (
        encryption_suite = 'xchacha20_poly1305'
        AND suite_version = 1
        AND key_id ~ '^[A-Za-z0-9_.:/-]{1,128}$'
        AND pg_catalog.octet_length(nonce) = 24
        AND pg_catalog.octet_length(ciphertext) BETWEEN 17 AND 4112
        AND pg_catalog.octet_length(aad_digest) = 32
    ),
    CONSTRAINT runtime_interaction_receipt_token_secrets_v1_time_check CHECK (
        pg_catalog.isfinite(issued_at)
        AND pg_catalog.isfinite(expires_at)
        AND issued_at < expires_at
        AND expires_at <= issued_at + INTERVAL '15 minutes'
    )
);

CREATE INDEX runtime_interaction_receipt_heads_recovery_v1_idx
ON public.runtime_interaction_receipt_heads_v1 USING btree (
    claim_expires_at,
    application_id COLLATE "C",
    interaction_id COLLATE "C"
)
WHERE state IN (
    'claimed',
    'acknowledging',
    'deferred',
    'prepared',
    'executing'
);

CREATE INDEX runtime_interaction_receipt_token_expiry_v1_idx
ON public.runtime_interaction_receipt_token_secrets_v1 USING btree (
    expires_at,
    application_id COLLATE "C",
    interaction_id COLLATE "C"
);

CREATE FUNCTION public.starring_runtime_interaction_receipt_schema_manifest_v1()
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


CREATE FUNCTION public.guard_runtime_interaction_receipt_root_v1()
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
        MESSAGE = 'runtime_interaction_receipt_root_immutable';
END;
$function$;

DO $manifest_extension$
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
    return_contract := $needle$    RETURN observed_count = 22$needle$;
    return_replacement := $needle$    RETURN public.starring_runtime_interaction_receipt_schema_manifest_v1()
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
        RAISE EXCEPTION 'runtime interaction receipt manifest extension failed'
            USING ERRCODE = '55000';
    END IF;

    function_definition := pg_catalog.replace(
        function_definition,
        return_contract,
        return_replacement
    );
    EXECUTE function_definition;
END;
$manifest_extension$;

DO $readiness_extension$
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
    relation_contract := $needle$        VALUES
            ('public.product_control_plane_identity'),
            ('public.automation_instances'),
            ('public.automation_ruleset_versions')
    ) AS expected(identity)$needle$;
    relation_replacement := $needle$        VALUES
            ('public.product_control_plane_identity'),
            ('public.automation_instances'),
            ('public.automation_ruleset_versions'),
            ('public.runtime_interaction_receipt_roots_v1'),
            ('public.runtime_interaction_receipt_heads_v1'),
            ('public.runtime_interaction_receipt_events_v1'),
            ('public.runtime_interaction_receipt_token_secrets_v1')
    ) AS expected(identity)$needle$;
    capability_contract := $needle$            (
                'public.starring_runtime_interaction_instance_scan_retryable_v2(text,text,text,text,bigint)',
                'expected_after_guild_id text, expected_after_instance_id text, expected_through_guild_id text, expected_through_instance_id text, expected_limit bigint',
                'TABLE(guild_id text, instance_id text, through_guild_id text, through_instance_id text)',
                TRUE,
                256::REAL,
                'plpgsql'
            )$needle$;
    capability_replacement := capability_contract || $extension$,
            (
                'public.starring_runtime_interaction_receipt_authority_observe_v1(text,text,text,text,text,text,bigint,text,bigint,text,bigint,bigint,bigint,text,text,text,text,text)',
                'expected_application_id text, expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_guild_id text, expected_ruleset_key text, expected_target_version bigint, expected_target_content_hash text, expected_binding_revision bigint, expected_binding_fingerprint text, expected_runtime_generation bigint, expected_controller_fencing_token bigint, expected_route_incarnation bigint, expected_process_instance_id text, expected_gateway_shard_id text, expected_runtime_build_revision text, expected_route_kind text, expected_instance_id text',
                'TABLE(tenant_id text, installation_id text, deployment_id text, attestation_id text, attestation_digest text, serving_lease_epoch bigint, serving_revision bigint, gateway_owner_lease_epoch bigint, gateway_owner_revision bigint, route_controller_fencing_token bigint, route_incarnation bigint, runtime_build_revision text, execution_ruleset_version bigint, execution_ruleset_content_hash text, instance_manifest_digest text, observed_database_now timestamp with time zone)',
                TRUE,
                1::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_receipt_claim_v1(text,text,text,text,text,text,text,text,text,text,bigint,text,bigint,text,bigint,bigint,bigint,text,text,text,text,text,text,text,bigint,bigint,bigint,bigint,bigint,text,text,bytea,bigint,text,smallint,text,bytea,bytea,bytea,timestamp with time zone,timestamp with time zone)',
                'expected_application_id text, expected_interaction_id text, expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_guild_id text, expected_channel_id text, expected_actor_user_id text, expected_interaction_kind text, expected_ruleset_key text, expected_target_version bigint, expected_target_content_hash text, expected_binding_revision bigint, expected_binding_fingerprint text, expected_runtime_generation bigint, expected_controller_fencing_token bigint, expected_route_incarnation bigint, expected_process_instance_id text, expected_gateway_shard_id text, expected_runtime_build_revision text, expected_route_kind text, expected_route_key text, expected_instance_id text, expected_attestation_digest text, expected_serving_lease_epoch bigint, expected_serving_revision bigint, expected_gateway_owner_lease_epoch bigint, expected_gateway_owner_revision bigint, expected_execution_ruleset_version bigint, expected_execution_ruleset_content_hash text, expected_instance_manifest_digest text, proposed_request_digest bytea, requested_claim_lease_milliseconds bigint, proposed_token_encryption_suite text, proposed_token_suite_version smallint, proposed_token_key_id text, proposed_token_nonce bytea, proposed_token_ciphertext bytea, proposed_token_aad_digest bytea, proposed_token_issued_at timestamp with time zone, proposed_token_expires_at timestamp with time zone',
                'TABLE(outcome_name text, derived_tenant_id text, derived_installation_id text, derived_deployment_id text, derived_attestation_id text, derived_attestation_digest text, derived_serving_lease_epoch bigint, derived_serving_revision bigint, derived_gateway_owner_lease_epoch bigint, derived_gateway_owner_revision bigint, derived_route_controller_fencing_token bigint, derived_route_incarnation bigint, derived_runtime_build_revision text, derived_execution_ruleset_version bigint, derived_execution_ruleset_content_hash text, derived_instance_manifest_digest text, receipt_state text, resulting_head_revision bigint, resulting_claim_revision bigint, resulting_claim_expires_at timestamp with time zone, resulting_token_issued_at timestamp with time zone, resulting_token_expires_at timestamp with time zone, observed_database_now timestamp with time zone)',
                TRUE,
                1::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_receipt_plan_bind_v1(text,text,bigint,bigint,text,bytea)',
                'expected_application_id text, expected_interaction_id text, expected_head_revision bigint, expected_claim_revision bigint, expected_process_instance_id text, proposed_action_plan_digest bytea',
                'TABLE(outcome_name text, receipt_state text, resulting_head_revision bigint, resulting_claim_revision bigint, resulting_claim_expires_at timestamp with time zone, observed_database_now timestamp with time zone)',
                TRUE,
                1::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_receipt_acknowledgement_intend_v1(text,text,bigint,bigint,text,text,bytea)',
                'expected_application_id text, expected_interaction_id text, expected_head_revision bigint, expected_claim_revision bigint, expected_process_instance_id text, proposed_acknowledgement_kind text, proposed_acknowledgement_digest bytea',
                'TABLE(outcome_name text, receipt_state text, resulting_head_revision bigint, resulting_claim_revision bigint, resulting_claim_expires_at timestamp with time zone, observed_database_now timestamp with time zone)',
                TRUE,
                1::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_receipt_acknowledgement_finish_v1(text,text,bigint,bigint,text,bytea,text,bytea)',
                'expected_application_id text, expected_interaction_id text, expected_head_revision bigint, expected_claim_revision bigint, expected_process_instance_id text, expected_acknowledgement_digest bytea, proposed_acknowledgement_result text, proposed_acknowledgement_result_digest bytea',
                'TABLE(outcome_name text, receipt_state text, resulting_head_revision bigint, resulting_claim_revision bigint, resulting_claim_expires_at timestamp with time zone, observed_database_now timestamp with time zone)',
                TRUE,
                1::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_receipt_execution_intend_v1(text,text,bigint,bigint,text,bytea)',
                'expected_application_id text, expected_interaction_id text, expected_head_revision bigint, expected_claim_revision bigint, expected_process_instance_id text, expected_action_plan_digest bytea',
                'TABLE(outcome_name text, receipt_state text, resulting_head_revision bigint, resulting_claim_revision bigint, resulting_claim_expires_at timestamp with time zone, observed_database_now timestamp with time zone)',
                TRUE,
                1::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_receipt_finish_v1(text,text,bigint,bigint,text,bytea,text,text,bytea)',
                'expected_application_id text, expected_interaction_id text, expected_head_revision bigint, expected_claim_revision bigint, expected_process_instance_id text, expected_action_plan_digest bytea, proposed_terminal_state text, proposed_terminal_outcome_code text, proposed_terminal_result_digest bytea',
                'TABLE(outcome_name text, receipt_state text, resulting_head_revision bigint, resulting_claim_revision bigint, resulting_claim_expires_at timestamp with time zone, observed_database_now timestamp with time zone)',
                TRUE,
                1::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_receipt_scan_recoverable_v1(timestamp with time zone,text,text,timestamp with time zone,text,text,bigint)',
                'expected_after_claim_expires_at timestamp with time zone, expected_after_application_id text, expected_after_interaction_id text, expected_through_claim_expires_at timestamp with time zone, expected_through_application_id text, expected_through_interaction_id text, expected_limit bigint',
                'TABLE(application_id text, interaction_id text, receipt_state text, head_revision bigint, claim_revision bigint, claim_expires_at timestamp with time zone, token_expires_at timestamp with time zone, through_claim_expires_at timestamp with time zone, through_application_id text, through_interaction_id text, observed_database_now timestamp with time zone)',
                TRUE,
                256::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_receipt_recover_v1(text,text,bigint,bigint,text,bigint,bigint,bigint,text,text,text,bytea,bigint)',
                'expected_application_id text, expected_interaction_id text, expected_head_revision bigint, expected_claim_revision bigint, expected_process_instance_id text, expected_runtime_generation bigint, expected_controller_fencing_token bigint, expected_route_incarnation bigint, expected_gateway_shard_id text, expected_runtime_build_revision text, proposed_observation_kind text, proposed_observation_digest bytea, requested_claim_lease_milliseconds bigint',
                'TABLE(outcome_name text, receipt_state text, resulting_head_revision bigint, resulting_claim_revision bigint, resulting_claim_expires_at timestamp with time zone, resulting_gateway_owner_lease_epoch bigint, resulting_gateway_owner_revision bigint, resulting_serving_lease_epoch bigint, resulting_serving_revision bigint, root_tenant_id text, root_installation_id text, root_deployment_id text, root_attestation_digest text, root_guild_id text, root_ruleset_key text, root_target_version bigint, root_target_content_hash text, root_binding_revision bigint, root_binding_fingerprint text, root_runtime_generation bigint, root_process_instance_id text, root_serving_lease_epoch bigint, root_serving_revision bigint, root_gateway_shard_id text, root_gateway_owner_lease_epoch bigint, root_gateway_owner_revision bigint, root_route_controller_fencing_token bigint, root_route_incarnation bigint, root_runtime_build_revision text, root_route_kind text, root_route_key text, root_instance_id text, root_execution_ruleset_version bigint, root_execution_ruleset_content_hash text, root_instance_manifest_digest text, root_request_digest bytea, token_encryption_suite text, token_suite_version smallint, token_key_id text, token_nonce bytea, token_ciphertext bytea, token_aad_digest bytea, token_issued_at timestamp with time zone, token_expires_at timestamp with time zone, observed_database_now timestamp with time zone)',
                TRUE,
                1::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_receipt_token_expire_v1(text,text,bigint,bigint,bytea)',
                'expected_application_id text, expected_interaction_id text, expected_head_revision bigint, expected_claim_revision bigint, proposed_expiry_observation_digest bytea',
                'TABLE(outcome_name text, receipt_state text, resulting_head_revision bigint, resulting_claim_revision bigint, observed_database_now timestamp with time zone)',
                TRUE,
                1::REAL,
                'plpgsql'
            ),
            (
                'public.starring_runtime_interaction_receipt_terminalize_expired_v1(text,text,bigint,bigint,text,text,bytea)',
                'expected_application_id text, expected_interaction_id text, expected_head_revision bigint, expected_claim_revision bigint, expected_process_instance_id text, expected_runtime_build_revision text, proposed_observation_digest bytea',
                'TABLE(outcome_name text, receipt_state text, resulting_head_revision bigint, resulting_claim_revision bigint, resulting_claim_expires_at timestamp with time zone, observed_database_now timestamp with time zone)',
                TRUE,
                1::REAL,
                'plpgsql'
            )$extension$;
    support_contract := $needle$            (
                'public.starring_runtime_interaction_schema_manifest_v1()',
                '',
                'boolean',
                TRUE
            )$needle$;
    support_replacement := support_contract || $extension$,
            (
                'public.guard_runtime_interaction_receipt_root_v1()',
                '',
                'trigger',
                FALSE
            ),
            (
                'public.guard_runtime_interaction_receipt_head_v1()',
                '',
                'trigger',
                FALSE
            ),
            (
                'public.guard_runtime_interaction_receipt_event_v1()',
                '',
                'trigger',
                FALSE
            ),
            (
                'public.guard_runtime_interaction_receipt_token_v1()',
                '',
                'trigger',
                FALSE
            ),
            (
                'public.starring_runtime_interaction_receipt_claim_current_v1(text,text,bigint,text,timestamp with time zone)',
                'expected_application_id text, expected_interaction_id text, expected_claim_revision bigint, expected_process_instance_id text, expected_database_now timestamp with time zone',
                'boolean',
                TRUE
            ),
            (
                'public.starring_runtime_interaction_receipt_schema_manifest_v1()',
                '',
                'boolean',
                TRUE
            )$extension$;
    trigger_contract := $needle$            (
                'public.automation_ruleset_versions',
                'automation_ruleset_versions_reject_truncate',
                'public.reject_ruleset_artifact_mutation()',
                34
            )$needle$;
    trigger_replacement := trigger_contract || $extension$,
            (
                'public.runtime_interaction_receipt_roots_v1',
                'runtime_interaction_receipt_roots_v1_immutable_mutation',
                'public.guard_runtime_interaction_receipt_root_v1()',
                27
            ),
            (
                'public.runtime_interaction_receipt_roots_v1',
                'runtime_interaction_receipt_roots_v1_immutable_truncate',
                'public.guard_runtime_interaction_receipt_root_v1()',
                34
            ),
            (
                'public.runtime_interaction_receipt_heads_v1',
                'runtime_interaction_receipt_heads_v1_guard_mutation',
                'public.guard_runtime_interaction_receipt_head_v1()',
                27
            ),
            (
                'public.runtime_interaction_receipt_heads_v1',
                'runtime_interaction_receipt_heads_v1_guard_truncate',
                'public.guard_runtime_interaction_receipt_head_v1()',
                34
            ),
            (
                'public.runtime_interaction_receipt_events_v1',
                'runtime_interaction_receipt_events_v1_immutable_mutation',
                'public.guard_runtime_interaction_receipt_event_v1()',
                27
            ),
            (
                'public.runtime_interaction_receipt_events_v1',
                'runtime_interaction_receipt_events_v1_immutable_truncate',
                'public.guard_runtime_interaction_receipt_event_v1()',
                34
            ),
            (
                'public.runtime_interaction_receipt_token_secrets_v1',
                'runtime_interaction_receipt_token_secrets_v1_immutable_update',
                'public.guard_runtime_interaction_receipt_token_v1()',
                19
            ),
            (
                'public.runtime_interaction_receipt_token_secrets_v1',
                'runtime_interaction_receipt_token_secrets_v1_immutable_truncate',
                'public.guard_runtime_interaction_receipt_token_v1()',
                34
            )$extension$;
    allowlist_contract := $needle$            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_instance_scan_retryable_v2(text,text,text,text,bigint)'
            )$needle$;
    allowlist_replacement := allowlist_contract || $extension$,
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_receipt_authority_observe_v1(text,text,text,text,text,text,bigint,text,bigint,text,bigint,bigint,bigint,text,text,text,text,text)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_receipt_claim_v1(text,text,text,text,text,text,text,text,text,text,bigint,text,bigint,text,bigint,bigint,bigint,text,text,text,text,text,text,text,bigint,bigint,bigint,bigint,bigint,text,text,bytea,bigint,text,smallint,text,bytea,bytea,bytea,timestamp with time zone,timestamp with time zone)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_receipt_plan_bind_v1(text,text,bigint,bigint,text,bytea)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_receipt_acknowledgement_intend_v1(text,text,bigint,bigint,text,text,bytea)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_receipt_acknowledgement_finish_v1(text,text,bigint,bigint,text,bytea,text,bytea)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_receipt_execution_intend_v1(text,text,bigint,bigint,text,bytea)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_receipt_finish_v1(text,text,bigint,bigint,text,bytea,text,text,bytea)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_receipt_scan_recoverable_v1(timestamp with time zone,text,text,timestamp with time zone,text,text,bigint)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_receipt_recover_v1(text,text,bigint,bigint,text,bigint,bigint,bigint,text,text,text,bytea,bigint)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_receipt_token_expire_v1(text,text,bigint,bigint,bytea)'
            ),
            pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_receipt_terminalize_expired_v1(text,text,bigint,bigint,text,text,bytea)'
            )$extension$;

    IF function_definition IS NULL
        OR pg_catalog.strpos(function_definition, relation_contract) = 0
        OR pg_catalog.strpos(function_definition, capability_contract) = 0
        OR pg_catalog.strpos(function_definition, support_contract) = 0
        OR pg_catalog.strpos(function_definition, trigger_contract) = 0
        OR pg_catalog.strpos(function_definition, allowlist_contract) = 0
    THEN
        RAISE EXCEPTION 'runtime interaction receipt readiness extension failed'
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
$readiness_extension$;

CREATE FUNCTION public.starring_runtime_interaction_receipt_plan_bind_v1(
    expected_application_id TEXT,
    expected_interaction_id TEXT,
    expected_head_revision BIGINT,
    expected_claim_revision BIGINT,
    expected_process_instance_id TEXT,
    proposed_action_plan_digest BYTEA
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
    head_row public.runtime_interaction_receipt_heads_v1%ROWTYPE;
    database_now TIMESTAMPTZ;
BEGIN
    IF expected_application_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_application_id) > 20
        OR expected_interaction_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_interaction_id) > 20
        OR expected_head_revision NOT BETWEEN 1 AND 9223372036854775806
        OR expected_claim_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR pg_catalog.octet_length(proposed_action_plan_digest) <> 32
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_receipt_plan_bind_input_invalid';
    END IF;

    database_now := pg_catalog.clock_timestamp();

    SELECT head.*
    INTO head_row
    FROM public.runtime_interaction_receipt_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_receipt_not_found';
    END IF;

    IF head_row.action_plan_digest IS NOT NULL THEN
        IF head_row.action_plan_digest
            IS DISTINCT FROM proposed_action_plan_digest
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI002',
                MESSAGE = 'runtime_interaction_receipt_plan_corruption';
        END IF;

        IF head_row.head_revision NOT IN (
                expected_head_revision,
                expected_head_revision + 1
            )
            OR head_row.claim_revision <> expected_claim_revision
            OR head_row.claim_process_instance_id
                IS DISTINCT FROM expected_process_instance_id
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI001',
                MESSAGE = 'runtime_interaction_receipt_plan_bind_conflict';
        END IF;

        IF NOT public.starring_runtime_interaction_receipt_claim_current_v1(
            expected_application_id,
            expected_interaction_id,
            expected_claim_revision,
            expected_process_instance_id,
            database_now
        ) THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI004',
                MESSAGE = 'runtime_interaction_receipt_claim_stale';
        END IF;

        outcome_name := 'exact_replay';
        receipt_state := head_row.state;
        resulting_head_revision := head_row.head_revision;
        resulting_claim_revision := head_row.claim_revision;
        resulting_claim_expires_at := head_row.claim_expires_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF head_row.state NOT IN ('claimed', 'deferred')
        OR head_row.head_revision <> expected_head_revision
        OR head_row.claim_revision <> expected_claim_revision
        OR head_row.claim_process_instance_id
            IS DISTINCT FROM expected_process_instance_id
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_receipt_plan_bind_conflict';
    END IF;

    IF NOT public.starring_runtime_interaction_receipt_claim_current_v1(
        expected_application_id,
        expected_interaction_id,
        expected_claim_revision,
        expected_process_instance_id,
        database_now
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_receipt_claim_stale';
    END IF;

    UPDATE public.runtime_interaction_receipt_heads_v1 AS head
    SET state = 'prepared',
        action_plan_digest = proposed_action_plan_digest,
        head_revision = head.head_revision + 1,
        updated_at = database_now
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
        'plan_bound',
        head_row.state,
        'prepared',
        head_row.acknowledgement_state,
        head_row.acknowledgement_state,
        head_row.claim_revision,
        head_row.claim_process_instance_id,
        head_row.claim_gateway_shard_id,
        head_row.claim_gateway_owner_lease_epoch,
        head_row.claim_gateway_owner_revision,
        head_row.claim_serving_lease_epoch,
        head_row.claim_serving_revision,
        'plan_bound',
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.concat_ws(
                '|',
                'starring-runtime-interaction-receipt-event-v1',
                expected_application_id,
                expected_interaction_id,
                (head_row.head_revision + 1)::TEXT,
                'plan_bound',
                head_row.state,
                'prepared',
                head_row.acknowledgement_state,
                head_row.acknowledgement_state,
                head_row.claim_revision::TEXT,
                head_row.claim_process_instance_id,
                head_row.claim_gateway_shard_id,
                head_row.claim_gateway_owner_lease_epoch::TEXT,
                head_row.claim_gateway_owner_revision::TEXT,
                head_row.claim_serving_lease_epoch::TEXT,
                head_row.claim_serving_revision::TEXT,
                'plan_bound'
            ),
            'UTF8'
        )),
        database_now
    );

    outcome_name := 'plan_bound';
    receipt_state := 'prepared';
    resulting_head_revision := head_row.head_revision + 1;
    resulting_claim_revision := head_row.claim_revision;
    resulting_claim_expires_at := head_row.claim_expires_at;
    observed_database_now := database_now;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.guard_runtime_interaction_receipt_event_v1()
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
        MESSAGE = 'runtime_interaction_receipt_event_immutable';
END;
$function$;

CREATE FUNCTION public.guard_runtime_interaction_receipt_token_v1()
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
        MESSAGE = 'runtime_interaction_receipt_token_immutable';
END;
$function$;

CREATE FUNCTION public.guard_runtime_interaction_receipt_head_v1()
RETURNS TRIGGER
LANGUAGE plpgsql
VOLATILE
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    IF TG_OP <> 'UPDATE'
        OR OLD.state IN ('completed', 'failed', 'recovery_required')
        OR ROW(NEW.application_id, NEW.interaction_id) IS DISTINCT FROM
            ROW(OLD.application_id, OLD.interaction_id)
        OR OLD.head_revision = 9223372036854775807
        OR NEW.head_revision IS DISTINCT FROM OLD.head_revision + 1
        OR NEW.claim_revision NOT IN (
            OLD.claim_revision,
            OLD.claim_revision + 1
        )
        OR (
            OLD.claim_revision = 9223372036854775807
            AND NEW.claim_revision <> OLD.claim_revision
        )
        OR (
            OLD.action_plan_digest IS NOT NULL
            AND NEW.action_plan_digest
                IS DISTINCT FROM OLD.action_plan_digest
        )
        OR (
            OLD.acknowledgement_kind IS NOT NULL
            AND ROW(
                NEW.acknowledgement_kind,
                NEW.acknowledgement_digest,
                NEW.acknowledgement_intended_at
            ) IS DISTINCT FROM ROW(
                OLD.acknowledgement_kind,
                OLD.acknowledgement_digest,
                OLD.acknowledgement_intended_at
            )
        )
        OR (
            OLD.acknowledgement_result IS NOT NULL
            AND ROW(
                NEW.acknowledgement_result,
                NEW.acknowledgement_result_digest,
                NEW.acknowledged_at
            ) IS DISTINCT FROM ROW(
                OLD.acknowledgement_result,
                OLD.acknowledgement_result_digest,
                OLD.acknowledged_at
            )
        )
        OR (
            OLD.execution_intended_at IS NOT NULL
            AND NEW.execution_intended_at
                IS DISTINCT FROM OLD.execution_intended_at
        )
        OR NOT (
            NEW.acknowledgement_state = OLD.acknowledgement_state
            OR (
                OLD.acknowledgement_state = 'unacknowledged'
                AND NEW.acknowledgement_state IN (
                    'attempting',
                    'response_recovery_terminal'
                )
            )
            OR (
                OLD.acknowledgement_state = 'attempting'
                AND NEW.acknowledgement_state IN (
                    'deferred',
                    'responded',
                    'response_recovery_terminal'
                )
            )
            OR (
                OLD.acknowledgement_state = 'deferred'
                AND NEW.acknowledgement_state IN (
                    'responded',
                    'response_recovery_terminal'
                )
            )
        )
        OR (
            OLD.terminal_at IS NOT NULL
            AND ROW(
                NEW.terminal_outcome_code,
                NEW.terminal_result_digest,
                NEW.terminal_at
            ) IS DISTINCT FROM ROW(
                OLD.terminal_outcome_code,
                OLD.terminal_result_digest,
                OLD.terminal_at
            )
        )
        OR (
            NEW.claim_revision = OLD.claim_revision
            AND ROW(
                NEW.claim_process_instance_id,
                NEW.claim_gateway_shard_id,
                NEW.claim_gateway_owner_lease_epoch,
                NEW.claim_gateway_owner_revision,
                NEW.claim_serving_lease_epoch,
                NEW.claim_serving_revision,
                NEW.claim_acquired_at,
                NEW.claim_expires_at
            ) IS DISTINCT FROM ROW(
                OLD.claim_process_instance_id,
                OLD.claim_gateway_shard_id,
                OLD.claim_gateway_owner_lease_epoch,
                OLD.claim_gateway_owner_revision,
                OLD.claim_serving_lease_epoch,
                OLD.claim_serving_revision,
                OLD.claim_acquired_at,
                OLD.claim_expires_at
            )
        )
        OR NOT (
            (
                NEW.state = OLD.state
                AND NEW.claim_revision IN (
                    OLD.claim_revision,
                    OLD.claim_revision + 1
                )
            )
            OR (
                OLD.state = 'claimed'
                AND NEW.state IN (
                    'acknowledging',
                    'prepared',
                    'failed',
                    'recovery_required'
                )
            )
            OR (
                OLD.state = 'acknowledging'
                AND NEW.state IN (
                    'deferred',
                    'prepared',
                    'completed',
                    'failed',
                    'recovery_required'
                )
            )
            OR (
                OLD.state = 'deferred'
                AND NEW.state IN (
                    'prepared',
                    'failed',
                    'recovery_required'
                )
            )
            OR (
                OLD.state = 'prepared'
                AND NEW.state IN (
                    'acknowledging',
                    'executing',
                    'completed',
                    'failed',
                    'recovery_required'
                )
            )
            OR (
                OLD.state = 'executing'
                AND NEW.state IN (
                    'completed',
                    'failed',
                    'recovery_required'
                )
            )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_receipt_head_transition_invalid';
    END IF;

    RETURN NEW;
END;
$function$;

CREATE TRIGGER runtime_interaction_receipt_roots_v1_immutable_mutation
BEFORE UPDATE OR DELETE ON public.runtime_interaction_receipt_roots_v1
FOR EACH ROW
EXECUTE FUNCTION public.guard_runtime_interaction_receipt_root_v1();

CREATE TRIGGER runtime_interaction_receipt_roots_v1_immutable_truncate
BEFORE TRUNCATE ON public.runtime_interaction_receipt_roots_v1
FOR EACH STATEMENT
EXECUTE FUNCTION public.guard_runtime_interaction_receipt_root_v1();

CREATE TRIGGER runtime_interaction_receipt_heads_v1_guard_mutation
BEFORE UPDATE OR DELETE ON public.runtime_interaction_receipt_heads_v1
FOR EACH ROW
EXECUTE FUNCTION public.guard_runtime_interaction_receipt_head_v1();

CREATE TRIGGER runtime_interaction_receipt_heads_v1_guard_truncate
BEFORE TRUNCATE ON public.runtime_interaction_receipt_heads_v1
FOR EACH STATEMENT
EXECUTE FUNCTION public.guard_runtime_interaction_receipt_head_v1();

CREATE TRIGGER runtime_interaction_receipt_events_v1_immutable_mutation
BEFORE UPDATE OR DELETE ON public.runtime_interaction_receipt_events_v1
FOR EACH ROW
EXECUTE FUNCTION public.guard_runtime_interaction_receipt_event_v1();

CREATE TRIGGER runtime_interaction_receipt_events_v1_immutable_truncate
BEFORE TRUNCATE ON public.runtime_interaction_receipt_events_v1
FOR EACH STATEMENT
EXECUTE FUNCTION public.guard_runtime_interaction_receipt_event_v1();

CREATE TRIGGER runtime_interaction_receipt_token_secrets_v1_immutable_update
BEFORE UPDATE ON public.runtime_interaction_receipt_token_secrets_v1
FOR EACH ROW
EXECUTE FUNCTION public.guard_runtime_interaction_receipt_token_v1();

CREATE FUNCTION public.starring_runtime_interaction_receipt_authority_observe_v1(
    expected_application_id TEXT,
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_guild_id TEXT,
    expected_ruleset_key TEXT,
    expected_target_version BIGINT,
    expected_target_content_hash TEXT,
    expected_binding_revision BIGINT,
    expected_binding_fingerprint TEXT,
    expected_runtime_generation BIGINT,
    expected_controller_fencing_token BIGINT,
    expected_route_incarnation BIGINT,
    expected_process_instance_id TEXT,
    expected_gateway_shard_id TEXT,
    expected_runtime_build_revision TEXT,
    expected_route_kind TEXT,
    expected_instance_id TEXT
)
RETURNS TABLE(
    tenant_id TEXT,
    installation_id TEXT,
    deployment_id TEXT,
    attestation_id TEXT,
    attestation_digest TEXT,
    serving_lease_epoch BIGINT,
    serving_revision BIGINT,
    gateway_owner_lease_epoch BIGINT,
    gateway_owner_revision BIGINT,
    route_controller_fencing_token BIGINT,
    route_incarnation BIGINT,
    runtime_build_revision TEXT,
    execution_ruleset_version BIGINT,
    execution_ruleset_content_hash TEXT,
    instance_manifest_digest TEXT,
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
    authority_row RECORD;
    execution_row RECORD;
    database_now TIMESTAMPTZ;
BEGIN
    IF expected_application_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_application_id) > 20
        OR (
            pg_catalog.length(expected_application_id) = 20
            AND expected_application_id > '18446744073709551615'
        )
        OR expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_guild_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_guild_id) > 20
        OR (
            pg_catalog.length(expected_guild_id) = 20
            AND expected_guild_id > '18446744073709551615'
        )
        OR expected_ruleset_key !~ '^[A-Za-z0-9_-]{1,64}$'
        OR expected_target_version NOT BETWEEN 1 AND 4294967295
        OR expected_target_content_hash !~ '^[0-9a-f]{64}$'
        OR expected_binding_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_binding_fingerprint !~ '^[0-9a-f]{64}$'
        OR expected_runtime_generation NOT BETWEEN 1 AND 9223372036854775807
        OR expected_controller_fencing_token
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_route_incarnation NOT BETWEEN 1 AND 9223372036854775807
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_gateway_shard_id !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR expected_runtime_build_revision !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR expected_route_kind NOT IN ('static', 'instance')
        OR (
            expected_route_kind = 'static'
            AND expected_instance_id <> ''
        )
        OR (
            expected_route_kind = 'instance'
            AND expected_instance_id !~ '^[A-Za-z0-9_-]{1,32}$'
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_receipt_authority_input_invalid';
    END IF;

    database_now := pg_catalog.clock_timestamp();

    SELECT
        installation.tenant_id,
        installation.installation_id,
        serving.deployment_id,
        serving.attestation_id,
        attestation.attestation_digest,
        serving.lease_epoch AS serving_lease_epoch,
        serving.revision AS serving_revision,
        owner.lease_epoch AS gateway_owner_lease_epoch,
        owner.owner_revision AS gateway_owner_revision,
        attestation.controller_fencing_token,
        attestation.v2_route_incarnation
    INTO authority_row
    FROM public.automation_installations AS installation
    INNER JOIN public.runtime_serving_leases AS serving
        ON serving.tenant_id = installation.tenant_id
        AND serving.installation_id = installation.installation_id
        AND serving.guild_id = installation.discord_guild_id
        AND serving.ruleset_key = installation.ruleset_key
    INNER JOIN public.runtime_deployments AS deployment
        ON deployment.tenant_id = serving.tenant_id
        AND deployment.installation_id = serving.installation_id
        AND deployment.deployment_id = serving.deployment_id
    INNER JOIN public.runtime_attestations AS attestation
        ON attestation.tenant_id = serving.tenant_id
        AND attestation.installation_id = serving.installation_id
        AND attestation.deployment_id = serving.deployment_id
        AND attestation.attestation_id = serving.attestation_id
    INNER JOIN public.runtime_gateway_owners AS owner
        ON owner.gateway_shard_id = expected_gateway_shard_id
    WHERE installation.discord_application_id = expected_application_id
        AND installation.discord_guild_id = expected_guild_id
        AND installation.ruleset_key = expected_ruleset_key
        AND installation.tenant_id = expected_tenant_id
        AND installation.installation_id = expected_installation_id
        AND installation.lifecycle_state = 'active'
        AND serving.deployment_id = expected_deployment_id
        AND serving.process_instance_id = expected_process_instance_id
        AND serving.runtime_generation = expected_runtime_generation
        AND serving.target_version = expected_target_version
        AND serving.target_content_hash = expected_target_content_hash
        AND serving.binding_revision = expected_binding_revision
        AND serving.binding_fingerprint = expected_binding_fingerprint
        AND serving.connected
        AND serving.serving
        AND serving.expires_at > database_now
        AND deployment.phase = 'live'
        AND deployment.live_attestation_id = serving.attestation_id
        AND deployment.guild_id = expected_guild_id
        AND deployment.ruleset_key = expected_ruleset_key
        AND deployment.target_version = expected_target_version
        AND deployment.target_content_hash = expected_target_content_hash
        AND deployment.binding_revision = expected_binding_revision
        AND deployment.binding_fingerprint = expected_binding_fingerprint
        AND deployment.runtime_generation = expected_runtime_generation
        AND attestation.guild_id = expected_guild_id
        AND attestation.ruleset_key = expected_ruleset_key
        AND attestation.target_version = expected_target_version
        AND attestation.target_content_hash = expected_target_content_hash
        AND attestation.binding_revision = expected_binding_revision
        AND attestation.binding_fingerprint = expected_binding_fingerprint
        AND attestation.runtime_generation = expected_runtime_generation
        AND attestation.controller_fencing_token
            = expected_controller_fencing_token
        AND attestation.v2_route_incarnation = expected_route_incarnation
        AND attestation.record_format_version = 2
        AND attestation.process_instance_id = expected_process_instance_id
        AND attestation.runtime_build_revision
            = expected_runtime_build_revision
        AND attestation.gateway_shard_id = expected_gateway_shard_id
        AND owner.process_instance_id = expected_process_instance_id
        AND owner.expected_build_revision = expected_runtime_build_revision
        AND owner.owner_revision IS NOT NULL
        AND owner.expires_at IS NOT NULL
        AND owner.expires_at > database_now
        AND owner.lease_epoch::TEXT = (
            attestation.v2_route_admission
                #>> '{gateway_owner_lease_id,lease_epoch}'
        )
    FOR SHARE OF installation, serving, deployment, attestation, owner;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_receipt_authority_invalid';
    END IF;

    IF expected_route_kind = 'static' THEN
        SELECT artifact.version, artifact.content_hash, NULL::TEXT AS manifest_digest
        INTO execution_row
        FROM public.automation_ruleset_versions AS artifact
        WHERE artifact.guild_id = expected_guild_id
            AND artifact.ruleset_key = expected_ruleset_key
            AND artifact.version = expected_target_version
            AND artifact.content_hash = expected_target_content_hash
        FOR SHARE;

        IF NOT FOUND THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI004',
                MESSAGE = 'runtime_interaction_receipt_static_route_invalid';
        END IF;
    ELSE
        SELECT
            instance.ruleset_version AS version,
            artifact.content_hash,
            pg_catalog.encode(
                pg_catalog.sha256(pg_catalog.convert_to(
                    public.starring_canonical_json_v1(instance.resources),
                    'UTF8'
                )),
                'hex'
            ) AS manifest_digest
        INTO execution_row
        FROM public.automation_instances AS instance
        INNER JOIN public.automation_ruleset_versions AS artifact
            ON artifact.guild_id = instance.guild_id
            AND artifact.ruleset_key = instance.ruleset_key
            AND artifact.version = instance.ruleset_version
        WHERE instance.guild_id = expected_guild_id
            AND instance.instance_id = expected_instance_id
            AND instance.ruleset_key = expected_ruleset_key
            AND instance.status = 'active'
        FOR SHARE OF instance, artifact;

        IF NOT FOUND THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI004',
                MESSAGE = 'runtime_interaction_receipt_instance_route_invalid';
        END IF;
    END IF;

    tenant_id := authority_row.tenant_id;
    installation_id := authority_row.installation_id;
    deployment_id := authority_row.deployment_id;
    attestation_id := authority_row.attestation_id;
    attestation_digest := authority_row.attestation_digest;
    serving_lease_epoch := authority_row.serving_lease_epoch;
    serving_revision := authority_row.serving_revision;
    gateway_owner_lease_epoch := authority_row.gateway_owner_lease_epoch;
    gateway_owner_revision := authority_row.gateway_owner_revision;
    route_controller_fencing_token := authority_row.controller_fencing_token;
    route_incarnation := authority_row.v2_route_incarnation;
    runtime_build_revision := expected_runtime_build_revision;
    execution_ruleset_version := execution_row.version;
    execution_ruleset_content_hash := execution_row.content_hash;
    instance_manifest_digest := CASE
        WHEN expected_route_kind = 'instance' THEN execution_row.manifest_digest
        ELSE NULL
    END;
    observed_database_now := database_now;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_receipt_claim_v1(
    expected_application_id TEXT,
    expected_interaction_id TEXT,
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_deployment_id TEXT,
    expected_guild_id TEXT,
    expected_channel_id TEXT,
    expected_actor_user_id TEXT,
    expected_interaction_kind TEXT,
    expected_ruleset_key TEXT,
    expected_target_version BIGINT,
    expected_target_content_hash TEXT,
    expected_binding_revision BIGINT,
    expected_binding_fingerprint TEXT,
    expected_runtime_generation BIGINT,
    expected_controller_fencing_token BIGINT,
    expected_route_incarnation BIGINT,
    expected_process_instance_id TEXT,
    expected_gateway_shard_id TEXT,
    expected_runtime_build_revision TEXT,
    expected_route_kind TEXT,
    expected_route_key TEXT,
    expected_instance_id TEXT,
    expected_attestation_digest TEXT,
    expected_serving_lease_epoch BIGINT,
    expected_serving_revision BIGINT,
    expected_gateway_owner_lease_epoch BIGINT,
    expected_gateway_owner_revision BIGINT,
    expected_execution_ruleset_version BIGINT,
    expected_execution_ruleset_content_hash TEXT,
    expected_instance_manifest_digest TEXT,
    proposed_request_digest BYTEA,
    requested_claim_lease_milliseconds BIGINT,
    proposed_token_encryption_suite TEXT,
    proposed_token_suite_version SMALLINT,
    proposed_token_key_id TEXT,
    proposed_token_nonce BYTEA,
    proposed_token_ciphertext BYTEA,
    proposed_token_aad_digest BYTEA,
    proposed_token_issued_at TIMESTAMPTZ,
    proposed_token_expires_at TIMESTAMPTZ
)
RETURNS TABLE(
    outcome_name TEXT,
    derived_tenant_id TEXT,
    derived_installation_id TEXT,
    derived_deployment_id TEXT,
    derived_attestation_id TEXT,
    derived_attestation_digest TEXT,
    derived_serving_lease_epoch BIGINT,
    derived_serving_revision BIGINT,
    derived_gateway_owner_lease_epoch BIGINT,
    derived_gateway_owner_revision BIGINT,
    derived_route_controller_fencing_token BIGINT,
    derived_route_incarnation BIGINT,
    derived_runtime_build_revision TEXT,
    derived_execution_ruleset_version BIGINT,
    derived_execution_ruleset_content_hash TEXT,
    derived_instance_manifest_digest TEXT,
    receipt_state TEXT,
    resulting_head_revision BIGINT,
    resulting_claim_revision BIGINT,
    resulting_claim_expires_at TIMESTAMPTZ,
    resulting_token_issued_at TIMESTAMPTZ,
    resulting_token_expires_at TIMESTAMPTZ,
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
    authority_row RECORD;
    root_row public.runtime_interaction_receipt_roots_v1%ROWTYPE;
    head_row public.runtime_interaction_receipt_heads_v1%ROWTYPE;
    database_now TIMESTAMPTZ;
    token_issued TIMESTAMPTZ;
    token_expiry TIMESTAMPTZ;
    authority_available BOOLEAN := FALSE;
BEGIN
    IF expected_application_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_application_id) > 20
        OR (
            pg_catalog.length(expected_application_id) = 20
            AND expected_application_id > '18446744073709551615'
        )
        OR expected_interaction_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_interaction_id) > 20
        OR (
            pg_catalog.length(expected_interaction_id) = 20
            AND expected_interaction_id > '18446744073709551615'
        )
        OR expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_deployment_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_guild_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_guild_id) > 20
        OR (
            pg_catalog.length(expected_guild_id) = 20
            AND expected_guild_id > '18446744073709551615'
        )
        OR expected_channel_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_channel_id) > 20
        OR (
            pg_catalog.length(expected_channel_id) = 20
            AND expected_channel_id > '18446744073709551615'
        )
        OR expected_actor_user_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_actor_user_id) > 20
        OR (
            pg_catalog.length(expected_actor_user_id) = 20
            AND expected_actor_user_id > '18446744073709551615'
        )
        OR expected_interaction_kind NOT IN (
            'message_component',
            'modal_submit'
        )
        OR expected_ruleset_key !~ '^[A-Za-z0-9_-]{1,64}$'
        OR expected_target_version NOT BETWEEN 1 AND 4294967295
        OR expected_target_content_hash !~ '^[0-9a-f]{64}$'
        OR expected_binding_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_binding_fingerprint !~ '^[0-9a-f]{64}$'
        OR expected_runtime_generation NOT BETWEEN 1 AND 9223372036854775807
        OR expected_controller_fencing_token
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_route_incarnation NOT BETWEEN 1 AND 9223372036854775807
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_gateway_shard_id !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR expected_runtime_build_revision !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR expected_route_kind NOT IN ('static', 'instance')
        OR pg_catalog.octet_length(expected_route_key) NOT BETWEEN 1 AND 100
        OR expected_route_key <> pg_catalog.btrim(expected_route_key)
        OR expected_route_key ~ '[[:cntrl:]]'
        OR (
            expected_route_kind = 'static'
            AND expected_instance_id <> ''
        )
        OR (
            expected_route_kind = 'instance'
            AND expected_instance_id !~ '^[A-Za-z0-9_-]{1,32}$'
        )
        OR expected_attestation_digest !~ '^[0-9a-f]{64}$'
        OR expected_serving_lease_epoch NOT BETWEEN 1 AND 9223372036854775807
        OR expected_serving_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_gateway_owner_lease_epoch
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_gateway_owner_revision
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_execution_ruleset_version NOT BETWEEN 1 AND 4294967295
        OR expected_execution_ruleset_content_hash !~ '^[0-9a-f]{64}$'
        OR (
            expected_route_kind = 'static'
            AND (
                expected_execution_ruleset_version <> expected_target_version
                OR expected_execution_ruleset_content_hash
                    <> expected_target_content_hash
                OR expected_instance_manifest_digest <> ''
            )
        )
        OR (
            expected_route_kind = 'instance'
            AND expected_instance_manifest_digest !~ '^[0-9a-f]{64}$'
        )
        OR pg_catalog.octet_length(proposed_request_digest) <> 32
        OR requested_claim_lease_milliseconds NOT BETWEEN 1000 AND 300000
        OR proposed_token_encryption_suite <> 'xchacha20_poly1305'
        OR proposed_token_suite_version <> 1
        OR proposed_token_key_id !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR pg_catalog.octet_length(proposed_token_nonce) <> 24
        OR pg_catalog.octet_length(proposed_token_ciphertext) NOT BETWEEN 17 AND 4112
        OR pg_catalog.octet_length(proposed_token_aad_digest) <> 32
        OR NOT pg_catalog.isfinite(proposed_token_issued_at)
        OR NOT pg_catalog.isfinite(proposed_token_expires_at)
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_receipt_claim_input_invalid';
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

    database_now := pg_catalog.clock_timestamp();

    SELECT root.*
    INTO root_row
    FROM public.runtime_interaction_receipt_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    IF FOUND THEN
        IF root_row.request_digest IS DISTINCT FROM proposed_request_digest
            OR root_row.tenant_id IS DISTINCT FROM expected_tenant_id
            OR root_row.installation_id IS DISTINCT FROM expected_installation_id
            OR root_row.deployment_id IS DISTINCT FROM expected_deployment_id
            OR root_row.attestation_digest
                IS DISTINCT FROM expected_attestation_digest
            OR root_row.guild_id IS DISTINCT FROM expected_guild_id
            OR root_row.channel_id IS DISTINCT FROM expected_channel_id
            OR root_row.actor_user_id IS DISTINCT FROM expected_actor_user_id
            OR root_row.interaction_kind
                IS DISTINCT FROM expected_interaction_kind
            OR root_row.ruleset_key IS DISTINCT FROM expected_ruleset_key
            OR root_row.target_version IS DISTINCT FROM expected_target_version
            OR root_row.target_content_hash
                IS DISTINCT FROM expected_target_content_hash
            OR root_row.binding_revision
                IS DISTINCT FROM expected_binding_revision
            OR root_row.binding_fingerprint
                IS DISTINCT FROM expected_binding_fingerprint
            OR root_row.runtime_generation
                IS DISTINCT FROM expected_runtime_generation
            OR root_row.route_controller_fencing_token
                IS DISTINCT FROM expected_controller_fencing_token
            OR root_row.route_incarnation
                IS DISTINCT FROM expected_route_incarnation
            OR root_row.origin_process_instance_id
                IS DISTINCT FROM expected_process_instance_id
            OR root_row.origin_serving_lease_epoch
                IS DISTINCT FROM expected_serving_lease_epoch
            OR root_row.origin_serving_revision
                > expected_serving_revision
            OR root_row.origin_gateway_shard_id
                IS DISTINCT FROM expected_gateway_shard_id
            OR root_row.origin_gateway_owner_lease_epoch
                IS DISTINCT FROM expected_gateway_owner_lease_epoch
            OR root_row.origin_gateway_owner_revision
                > expected_gateway_owner_revision
            OR root_row.runtime_build_revision
                IS DISTINCT FROM expected_runtime_build_revision
            OR root_row.route_kind IS DISTINCT FROM expected_route_kind
            OR root_row.route_key IS DISTINCT FROM expected_route_key
            OR root_row.instance_id
                IS DISTINCT FROM NULLIF(expected_instance_id, '')
            OR root_row.execution_ruleset_version
                IS DISTINCT FROM expected_execution_ruleset_version
            OR root_row.execution_ruleset_content_hash
                IS DISTINCT FROM expected_execution_ruleset_content_hash
            OR root_row.instance_manifest_digest IS DISTINCT FROM
                NULLIF(expected_instance_manifest_digest, '')
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI002',
                MESSAGE = 'runtime_interaction_receipt_semantic_corruption';
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
                MESSAGE = 'runtime_interaction_receipt_head_missing';
        END IF;

        IF head_row.claim_serving_lease_epoch
                IS DISTINCT FROM expected_serving_lease_epoch
            OR head_row.claim_serving_revision > expected_serving_revision
            OR head_row.claim_gateway_owner_lease_epoch
                IS DISTINCT FROM expected_gateway_owner_lease_epoch
            OR head_row.claim_gateway_owner_revision
                > expected_gateway_owner_revision
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI002',
                MESSAGE = 'runtime_interaction_receipt_claim_authority_regressed';
        END IF;

        SELECT secret.issued_at, secret.expires_at
        INTO token_issued, token_expiry
        FROM public.runtime_interaction_receipt_token_secrets_v1 AS secret
        WHERE secret.application_id = expected_application_id
            AND secret.interaction_id = expected_interaction_id;

        IF head_row.state IN (
            'completed',
            'failed',
            'recovery_required'
        ) THEN
            outcome_name := 'terminal_duplicate';
        ELSIF head_row.claim_expires_at > database_now THEN
            outcome_name := 'in_flight_duplicate';
        ELSIF head_row.state = 'claimed'
            AND head_row.acknowledgement_state = 'unacknowledged'
            AND head_row.action_plan_digest IS NULL
            AND head_row.acknowledgement_kind IS NULL
            AND head_row.execution_intended_at IS NULL
            AND token_expiry > database_now
            AND head_row.head_revision < 9223372036854775807
            AND head_row.claim_revision < 9223372036854775807
        THEN
            BEGIN
                SELECT observed.*
                INTO authority_row
                FROM public.starring_runtime_interaction_receipt_authority_observe_v1(
                    expected_application_id,
                    expected_tenant_id,
                    expected_installation_id,
                    expected_deployment_id,
                    expected_guild_id,
                    expected_ruleset_key,
                    expected_target_version,
                    expected_target_content_hash,
                    expected_binding_revision,
                    expected_binding_fingerprint,
                    expected_runtime_generation,
                    expected_controller_fencing_token,
                    expected_route_incarnation,
                    expected_process_instance_id,
                    expected_gateway_shard_id,
                    expected_runtime_build_revision,
                    expected_route_kind,
                    expected_instance_id
                ) AS observed;
                authority_available := FOUND;
            EXCEPTION
                WHEN SQLSTATE 'RI004' THEN
                    authority_available := FALSE;
            END;

            IF authority_available
                AND authority_row.attestation_id = root_row.attestation_id
                AND authority_row.attestation_digest
                    = root_row.attestation_digest
                AND authority_row.serving_lease_epoch
                    = root_row.origin_serving_lease_epoch
                AND authority_row.serving_revision
                    = expected_serving_revision
                AND authority_row.gateway_owner_lease_epoch
                    = root_row.origin_gateway_owner_lease_epoch
                AND authority_row.gateway_owner_revision
                    = expected_gateway_owner_revision
                AND authority_row.route_controller_fencing_token
                    = root_row.route_controller_fencing_token
                AND authority_row.route_incarnation = root_row.route_incarnation
                AND authority_row.runtime_build_revision
                    = root_row.runtime_build_revision
                AND authority_row.execution_ruleset_version
                    = root_row.execution_ruleset_version
                AND authority_row.execution_ruleset_content_hash
                    = root_row.execution_ruleset_content_hash
                AND authority_row.instance_manifest_digest
                    IS NOT DISTINCT FROM root_row.instance_manifest_digest
            THEN
                UPDATE public.runtime_interaction_receipt_heads_v1 AS head
                SET head_revision = head.head_revision + 1,
                    claim_revision = head.claim_revision + 1,
                    claim_process_instance_id = expected_process_instance_id,
                    claim_gateway_shard_id = expected_gateway_shard_id,
                    claim_gateway_owner_lease_epoch =
                        authority_row.gateway_owner_lease_epoch,
                    claim_gateway_owner_revision =
                        authority_row.gateway_owner_revision,
                    claim_serving_lease_epoch =
                        authority_row.serving_lease_epoch,
                    claim_serving_revision = authority_row.serving_revision,
                    claim_acquired_at = database_now,
                    claim_expires_at = LEAST(
                        database_now
                            + requested_claim_lease_milliseconds
                                * INTERVAL '1 millisecond',
                        token_expiry
                    ),
                    updated_at = database_now
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
                    'claim_recovered',
                    'claimed',
                    'claimed',
                    'unacknowledged',
                    'unacknowledged',
                    head_row.claim_revision + 1,
                    expected_process_instance_id,
                    expected_gateway_shard_id,
                    authority_row.gateway_owner_lease_epoch,
                    authority_row.gateway_owner_revision,
                    authority_row.serving_lease_epoch,
                    authority_row.serving_revision,
                    'pristine_claim_recovered',
                    pg_catalog.sha256(pg_catalog.convert_to(
                        pg_catalog.concat_ws(
                            '|',
                            'starring-runtime-interaction-receipt-event-v1',
                            expected_application_id,
                            expected_interaction_id,
                            (head_row.head_revision + 1)::TEXT,
                            'claim_recovered',
                            'claimed',
                            'claimed',
                            'unacknowledged',
                            'unacknowledged',
                            (head_row.claim_revision + 1)::TEXT,
                            expected_process_instance_id,
                            expected_gateway_shard_id,
                            authority_row.gateway_owner_lease_epoch::TEXT,
                            authority_row.gateway_owner_revision::TEXT,
                            authority_row.serving_lease_epoch::TEXT,
                            authority_row.serving_revision::TEXT,
                            'pristine_claim_recovered'
                        ),
                        'UTF8'
                    )),
                    database_now
                );

                head_row.head_revision := head_row.head_revision + 1;
                head_row.claim_revision := head_row.claim_revision + 1;
                head_row.claim_expires_at := LEAST(
                    database_now
                        + requested_claim_lease_milliseconds
                            * INTERVAL '1 millisecond',
                    token_expiry
                );
                outcome_name := 'pristine_claim_recovered';
            ELSE
                outcome_name := 'recovery_required_duplicate';
            END IF;
        ELSE
            outcome_name := 'recovery_required_duplicate';
        END IF;

        IF outcome_name = 'recovery_required_duplicate'
            AND head_row.state NOT IN (
                'completed',
                'failed',
                'recovery_required'
            )
        THEN
            BEGIN
                SELECT observed.*
                INTO authority_row
                FROM public.starring_runtime_interaction_receipt_authority_observe_v1(
                    expected_application_id,
                    expected_tenant_id,
                    expected_installation_id,
                    expected_deployment_id,
                    expected_guild_id,
                    expected_ruleset_key,
                    expected_target_version,
                    expected_target_content_hash,
                    expected_binding_revision,
                    expected_binding_fingerprint,
                    expected_runtime_generation,
                    expected_controller_fencing_token,
                    expected_route_incarnation,
                    expected_process_instance_id,
                    expected_gateway_shard_id,
                    expected_runtime_build_revision,
                    expected_route_kind,
                    expected_instance_id
                ) AS observed;
                authority_available := FOUND;
            EXCEPTION
                WHEN SQLSTATE 'RI004' THEN
                    authority_available := FALSE;
            END;

            IF NOT authority_available
                OR authority_row.attestation_id
                    IS DISTINCT FROM root_row.attestation_id
                OR authority_row.attestation_digest
                    IS DISTINCT FROM root_row.attestation_digest
                OR authority_row.serving_lease_epoch
                    IS DISTINCT FROM root_row.origin_serving_lease_epoch
                OR authority_row.serving_revision
                    IS DISTINCT FROM expected_serving_revision
                OR authority_row.gateway_owner_lease_epoch
                    IS DISTINCT FROM root_row.origin_gateway_owner_lease_epoch
                OR authority_row.gateway_owner_revision
                    IS DISTINCT FROM expected_gateway_owner_revision
                OR authority_row.route_controller_fencing_token
                    IS DISTINCT FROM root_row.route_controller_fencing_token
                OR authority_row.route_incarnation
                    IS DISTINCT FROM root_row.route_incarnation
                OR authority_row.runtime_build_revision
                    IS DISTINCT FROM root_row.runtime_build_revision
                OR authority_row.execution_ruleset_version
                    IS DISTINCT FROM root_row.execution_ruleset_version
                OR authority_row.execution_ruleset_content_hash
                    IS DISTINCT FROM root_row.execution_ruleset_content_hash
                OR authority_row.instance_manifest_digest
                    IS DISTINCT FROM root_row.instance_manifest_digest
            THEN
                RAISE EXCEPTION USING
                    ERRCODE = 'RI004',
                    MESSAGE = 'runtime_interaction_receipt_recovery_authority_invalid';
            END IF;

            UPDATE public.runtime_interaction_receipt_heads_v1 AS head
            SET state = 'recovery_required',
                head_revision = head.head_revision + 1,
                terminal_outcome_code = 'expired_claim_recovery_required',
                terminal_result_digest = proposed_request_digest,
                terminal_at = database_now,
                updated_at = database_now
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
                'recovery_required',
                head_row.state,
                'recovery_required',
                head_row.acknowledgement_state,
                head_row.acknowledgement_state,
                head_row.claim_revision,
                head_row.claim_process_instance_id,
                head_row.claim_gateway_shard_id,
                head_row.claim_gateway_owner_lease_epoch,
                head_row.claim_gateway_owner_revision,
                head_row.claim_serving_lease_epoch,
                head_row.claim_serving_revision,
                'expired_claim_recovery_required',
                pg_catalog.sha256(pg_catalog.convert_to(
                    pg_catalog.concat_ws(
                        '|',
                        'starring-runtime-interaction-receipt-event-v1',
                        expected_application_id,
                        expected_interaction_id,
                        (head_row.head_revision + 1)::TEXT,
                        'recovery_required',
                        head_row.state,
                        'recovery_required',
                        head_row.acknowledgement_state,
                        head_row.acknowledgement_state,
                        head_row.claim_revision::TEXT,
                        head_row.claim_process_instance_id,
                        head_row.claim_gateway_shard_id,
                        head_row.claim_gateway_owner_lease_epoch::TEXT,
                        head_row.claim_gateway_owner_revision::TEXT,
                        head_row.claim_serving_lease_epoch::TEXT,
                        head_row.claim_serving_revision::TEXT,
                        'expired_claim_recovery_required'
                    ),
                    'UTF8'
                )),
                database_now
            );

            DELETE FROM public.runtime_interaction_receipt_token_secrets_v1
            WHERE application_id = expected_application_id
                AND interaction_id = expected_interaction_id;

            head_row.state := 'recovery_required';
            head_row.head_revision := head_row.head_revision + 1;
        END IF;
        derived_tenant_id := root_row.tenant_id;
        derived_installation_id := root_row.installation_id;
        derived_deployment_id := root_row.deployment_id;
        derived_attestation_id := root_row.attestation_id;
        derived_attestation_digest := root_row.attestation_digest;
        derived_serving_lease_epoch := expected_serving_lease_epoch;
        derived_serving_revision := expected_serving_revision;
        derived_gateway_owner_lease_epoch :=
            expected_gateway_owner_lease_epoch;
        derived_gateway_owner_revision :=
            expected_gateway_owner_revision;
        derived_route_controller_fencing_token :=
            root_row.route_controller_fencing_token;
        derived_route_incarnation := root_row.route_incarnation;
        derived_runtime_build_revision := root_row.runtime_build_revision;
        derived_execution_ruleset_version :=
            root_row.execution_ruleset_version;
        derived_execution_ruleset_content_hash :=
            root_row.execution_ruleset_content_hash;
        derived_instance_manifest_digest := root_row.instance_manifest_digest;
        receipt_state := head_row.state;
        resulting_head_revision := head_row.head_revision;
        resulting_claim_revision := head_row.claim_revision;
        resulting_claim_expires_at := head_row.claim_expires_at;
        resulting_token_issued_at := token_issued;
        resulting_token_expires_at := token_expiry;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF proposed_token_issued_at > database_now + INTERVAL '5 seconds'
        OR proposed_token_expires_at <= database_now
        OR proposed_token_issued_at >= proposed_token_expires_at
        OR proposed_token_expires_at
            > proposed_token_issued_at + INTERVAL '15 minutes'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_receipt_token_expiry_invalid';
    END IF;

    SELECT observed.*
    INTO authority_row
    FROM public.starring_runtime_interaction_receipt_authority_observe_v1(
        expected_application_id,
        expected_tenant_id,
        expected_installation_id,
        expected_deployment_id,
        expected_guild_id,
        expected_ruleset_key,
        expected_target_version,
        expected_target_content_hash,
        expected_binding_revision,
        expected_binding_fingerprint,
        expected_runtime_generation,
        expected_controller_fencing_token,
        expected_route_incarnation,
        expected_process_instance_id,
        expected_gateway_shard_id,
        expected_runtime_build_revision,
        expected_route_kind,
        expected_instance_id
    ) AS observed;

    IF NOT FOUND
        OR authority_row.attestation_digest
            IS DISTINCT FROM expected_attestation_digest
        OR authority_row.serving_lease_epoch
            IS DISTINCT FROM expected_serving_lease_epoch
        OR authority_row.serving_revision
            IS DISTINCT FROM expected_serving_revision
        OR authority_row.gateway_owner_lease_epoch
            IS DISTINCT FROM expected_gateway_owner_lease_epoch
        OR authority_row.gateway_owner_revision
            IS DISTINCT FROM expected_gateway_owner_revision
        OR authority_row.runtime_build_revision
            IS DISTINCT FROM expected_runtime_build_revision
        OR authority_row.execution_ruleset_version
            IS DISTINCT FROM expected_execution_ruleset_version
        OR authority_row.execution_ruleset_content_hash
            IS DISTINCT FROM expected_execution_ruleset_content_hash
        OR authority_row.instance_manifest_digest IS DISTINCT FROM
            NULLIF(expected_instance_manifest_digest, '')
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_receipt_authority_changed';
    END IF;

    INSERT INTO public.runtime_interaction_receipt_roots_v1 (
        application_id,
        interaction_id,
        tenant_id,
        installation_id,
        guild_id,
        channel_id,
        actor_user_id,
        interaction_kind,
        ruleset_key,
        target_version,
        target_content_hash,
        binding_revision,
        binding_fingerprint,
        deployment_id,
        attestation_id,
        attestation_digest,
        runtime_generation,
        route_controller_fencing_token,
        route_incarnation,
        origin_process_instance_id,
        origin_serving_lease_epoch,
        origin_serving_revision,
        origin_gateway_shard_id,
        origin_gateway_owner_lease_epoch,
        origin_gateway_owner_revision,
        runtime_build_revision,
        route_kind,
        route_key,
        instance_id,
        execution_ruleset_version,
        execution_ruleset_content_hash,
        instance_manifest_digest,
        request_digest,
        created_at
    ) VALUES (
        expected_application_id,
        expected_interaction_id,
        authority_row.tenant_id,
        authority_row.installation_id,
        expected_guild_id,
        expected_channel_id,
        expected_actor_user_id,
        expected_interaction_kind,
        expected_ruleset_key,
        expected_target_version,
        expected_target_content_hash,
        expected_binding_revision,
        expected_binding_fingerprint,
        authority_row.deployment_id,
        authority_row.attestation_id,
        authority_row.attestation_digest,
        expected_runtime_generation,
        expected_controller_fencing_token,
        expected_route_incarnation,
        expected_process_instance_id,
        authority_row.serving_lease_epoch,
        authority_row.serving_revision,
        expected_gateway_shard_id,
        authority_row.gateway_owner_lease_epoch,
        authority_row.gateway_owner_revision,
        expected_runtime_build_revision,
        expected_route_kind,
        expected_route_key,
        NULLIF(expected_instance_id, ''),
        authority_row.execution_ruleset_version,
        authority_row.execution_ruleset_content_hash,
        authority_row.instance_manifest_digest,
        proposed_request_digest,
        database_now
    );

    INSERT INTO public.runtime_interaction_receipt_heads_v1 (
        application_id,
        interaction_id,
        state,
        acknowledgement_state,
        head_revision,
        claim_revision,
        claim_process_instance_id,
        claim_gateway_shard_id,
        claim_gateway_owner_lease_epoch,
        claim_gateway_owner_revision,
        claim_serving_lease_epoch,
        claim_serving_revision,
        claim_acquired_at,
        claim_expires_at,
        updated_at
    ) VALUES (
        expected_application_id,
        expected_interaction_id,
        'claimed',
        'unacknowledged',
        1,
        1,
        expected_process_instance_id,
        expected_gateway_shard_id,
        authority_row.gateway_owner_lease_epoch,
        authority_row.gateway_owner_revision,
        authority_row.serving_lease_epoch,
        authority_row.serving_revision,
        database_now,
        LEAST(
            database_now
                + requested_claim_lease_milliseconds
                    * INTERVAL '1 millisecond',
            proposed_token_expires_at
        ),
        database_now
    );

    INSERT INTO public.runtime_interaction_receipt_token_secrets_v1 (
        application_id,
        interaction_id,
        encryption_suite,
        suite_version,
        key_id,
        nonce,
        ciphertext,
        aad_digest,
        issued_at,
        expires_at
    ) VALUES (
        expected_application_id,
        expected_interaction_id,
        proposed_token_encryption_suite,
        proposed_token_suite_version,
        proposed_token_key_id,
        proposed_token_nonce,
        proposed_token_ciphertext,
        proposed_token_aad_digest,
        proposed_token_issued_at,
        proposed_token_expires_at
    );

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
        1,
        'claimed',
        NULL,
        'claimed',
        NULL,
        'unacknowledged',
        1,
        expected_process_instance_id,
        expected_gateway_shard_id,
        authority_row.gateway_owner_lease_epoch,
        authority_row.gateway_owner_revision,
        authority_row.serving_lease_epoch,
        authority_row.serving_revision,
        'claimed_new',
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.concat_ws(
                '|',
                'starring-runtime-interaction-receipt-event-v1',
                expected_application_id,
                expected_interaction_id,
                '1',
                'claimed',
                '',
                'claimed',
                '',
                'unacknowledged',
                '1',
                expected_process_instance_id,
                expected_gateway_shard_id,
                authority_row.gateway_owner_lease_epoch::TEXT,
                authority_row.gateway_owner_revision::TEXT,
                authority_row.serving_lease_epoch::TEXT,
                authority_row.serving_revision::TEXT,
                'claimed_new'
            ),
            'UTF8'
        )),
        database_now
    );

    outcome_name := 'claimed_new';
    derived_tenant_id := authority_row.tenant_id;
    derived_installation_id := authority_row.installation_id;
    derived_deployment_id := authority_row.deployment_id;
    derived_attestation_id := authority_row.attestation_id;
    derived_attestation_digest := authority_row.attestation_digest;
    derived_serving_lease_epoch := authority_row.serving_lease_epoch;
    derived_serving_revision := authority_row.serving_revision;
    derived_gateway_owner_lease_epoch :=
        authority_row.gateway_owner_lease_epoch;
    derived_gateway_owner_revision := authority_row.gateway_owner_revision;
    derived_route_controller_fencing_token :=
        authority_row.route_controller_fencing_token;
    derived_route_incarnation := authority_row.route_incarnation;
    derived_runtime_build_revision := authority_row.runtime_build_revision;
    derived_execution_ruleset_version :=
        authority_row.execution_ruleset_version;
    derived_execution_ruleset_content_hash :=
        authority_row.execution_ruleset_content_hash;
    derived_instance_manifest_digest := authority_row.instance_manifest_digest;
    receipt_state := 'claimed';
    resulting_head_revision := 1;
    resulting_claim_revision := 1;
    resulting_claim_expires_at := LEAST(
        database_now
            + requested_claim_lease_milliseconds * INTERVAL '1 millisecond',
        proposed_token_expires_at
    );
    resulting_token_issued_at := proposed_token_issued_at;
    resulting_token_expires_at := proposed_token_expires_at;
    observed_database_now := database_now;
    RETURN NEXT;
END;
$function$;

CREATE TRIGGER runtime_interaction_receipt_token_secrets_v1_immutable_truncate
BEFORE TRUNCATE ON public.runtime_interaction_receipt_token_secrets_v1
FOR EACH STATEMENT
EXECUTE FUNCTION public.guard_runtime_interaction_receipt_token_v1();

CREATE FUNCTION public.starring_runtime_interaction_receipt_claim_current_v1(
    expected_application_id TEXT,
    expected_interaction_id TEXT,
    expected_claim_revision BIGINT,
    expected_process_instance_id TEXT,
    expected_database_now TIMESTAMPTZ
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
    claim_row RECORD;
    route_row RECORD;
    database_now TIMESTAMPTZ;
BEGIN
    IF expected_application_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_application_id) > 20
        OR expected_interaction_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_interaction_id) > 20
        OR expected_claim_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR NOT pg_catalog.isfinite(expected_database_now)
    THEN
        RETURN FALSE;
    END IF;

    SELECT
        root.route_kind,
        root.guild_id,
        root.ruleset_key,
        root.instance_id,
        root.execution_ruleset_version,
        root.execution_ruleset_content_hash,
        root.instance_manifest_digest
    INTO route_row
    FROM public.runtime_interaction_receipt_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    IF NOT FOUND THEN
        RETURN FALSE;
    END IF;

    IF route_row.route_kind = 'static' THEN
        PERFORM artifact.version
        FROM public.automation_ruleset_versions AS artifact
        WHERE artifact.guild_id = route_row.guild_id
            AND artifact.ruleset_key = route_row.ruleset_key
            AND artifact.version = route_row.execution_ruleset_version
            AND artifact.content_hash =
                route_row.execution_ruleset_content_hash
        FOR SHARE OF artifact;
    ELSIF route_row.route_kind = 'instance' THEN
        PERFORM instance.instance_id
        FROM public.automation_instances AS instance
        INNER JOIN public.automation_ruleset_versions AS artifact
            ON artifact.guild_id = instance.guild_id
            AND artifact.ruleset_key = instance.ruleset_key
            AND artifact.version = instance.ruleset_version
        WHERE instance.guild_id = route_row.guild_id
            AND instance.instance_id = route_row.instance_id
            AND instance.ruleset_key = route_row.ruleset_key
            AND instance.status = 'active'
            AND instance.ruleset_version =
                route_row.execution_ruleset_version
            AND artifact.content_hash =
                route_row.execution_ruleset_content_hash
            AND pg_catalog.encode(
                pg_catalog.sha256(pg_catalog.convert_to(
                    public.starring_canonical_json_v1(instance.resources),
                    'UTF8'
                )),
                'hex'
            ) = route_row.instance_manifest_digest
        FOR SHARE OF instance, artifact;
    ELSE
        RETURN FALSE;
    END IF;

    IF NOT FOUND THEN
        RETURN FALSE;
    END IF;

    SELECT
        head.claim_expires_at,
        serving.expires_at AS serving_expires_at,
        owner.expires_at AS owner_expires_at
    INTO claim_row
    FROM public.runtime_interaction_receipt_roots_v1 AS root
    INNER JOIN public.runtime_interaction_receipt_heads_v1 AS head
        ON head.application_id = root.application_id
        AND head.interaction_id = root.interaction_id
    INNER JOIN public.automation_installations AS installation
        ON installation.tenant_id = root.tenant_id
        AND installation.installation_id = root.installation_id
        AND installation.discord_application_id = root.application_id
        AND installation.discord_guild_id = root.guild_id
        AND installation.ruleset_key = root.ruleset_key
    INNER JOIN public.runtime_serving_leases AS serving
        ON serving.tenant_id = root.tenant_id
        AND serving.installation_id = root.installation_id
        AND serving.deployment_id = root.deployment_id
        AND serving.attestation_id = root.attestation_id
        AND serving.guild_id = root.guild_id
        AND serving.ruleset_key = root.ruleset_key
    INNER JOIN public.runtime_deployments AS deployment
        ON deployment.tenant_id = root.tenant_id
        AND deployment.installation_id = root.installation_id
        AND deployment.deployment_id = root.deployment_id
    INNER JOIN public.runtime_attestations AS attestation
        ON attestation.tenant_id = root.tenant_id
        AND attestation.installation_id = root.installation_id
        AND attestation.deployment_id = root.deployment_id
        AND attestation.attestation_id = root.attestation_id
    INNER JOIN public.runtime_gateway_owners AS owner
        ON owner.gateway_shard_id = head.claim_gateway_shard_id
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
        AND head.claim_revision = expected_claim_revision
        AND head.claim_process_instance_id = expected_process_instance_id
        AND head.state IN (
            'claimed',
            'acknowledging',
            'deferred',
            'prepared',
            'executing'
        )
        AND installation.lifecycle_state = 'active'
        AND serving.process_instance_id = expected_process_instance_id
        AND serving.runtime_generation = root.runtime_generation
        AND serving.target_version = root.target_version
        AND serving.target_content_hash = root.target_content_hash
        AND serving.binding_revision = root.binding_revision
        AND serving.binding_fingerprint = root.binding_fingerprint
        AND serving.lease_epoch = head.claim_serving_lease_epoch
        AND serving.revision >= head.claim_serving_revision
        AND serving.connected
        AND serving.serving
        AND deployment.phase = 'live'
        AND deployment.live_attestation_id = root.attestation_id
        AND deployment.guild_id = root.guild_id
        AND deployment.ruleset_key = root.ruleset_key
        AND deployment.target_version = root.target_version
        AND deployment.target_content_hash = root.target_content_hash
        AND deployment.binding_revision = root.binding_revision
        AND deployment.binding_fingerprint = root.binding_fingerprint
        AND deployment.runtime_generation = root.runtime_generation
        AND attestation.guild_id = root.guild_id
        AND attestation.ruleset_key = root.ruleset_key
        AND attestation.target_version = root.target_version
        AND attestation.target_content_hash = root.target_content_hash
        AND attestation.binding_revision = root.binding_revision
        AND attestation.binding_fingerprint = root.binding_fingerprint
        AND attestation.runtime_generation = root.runtime_generation
        AND attestation.attestation_digest = root.attestation_digest
        AND attestation.controller_fencing_token
            = root.route_controller_fencing_token
        AND attestation.v2_route_incarnation = root.route_incarnation
        AND attestation.record_format_version = 2
        AND attestation.process_instance_id = expected_process_instance_id
        AND attestation.runtime_build_revision = root.runtime_build_revision
        AND attestation.gateway_shard_id = head.claim_gateway_shard_id
        AND owner.process_instance_id = expected_process_instance_id
        AND owner.lease_epoch = head.claim_gateway_owner_lease_epoch
        AND owner.owner_revision >= head.claim_gateway_owner_revision
        AND owner.expected_build_revision = root.runtime_build_revision
        AND owner.lease_epoch::TEXT = (
            attestation.v2_route_admission
                #>> '{gateway_owner_lease_id,lease_epoch}'
        )
    FOR SHARE OF
        installation,
        serving,
        deployment,
        attestation,
        owner;

    IF NOT FOUND THEN
        RETURN FALSE;
    END IF;

    database_now := pg_catalog.clock_timestamp();

    RETURN COALESCE(
        claim_row.claim_expires_at > database_now
            AND claim_row.serving_expires_at > database_now
            AND claim_row.owner_expires_at > database_now,
        FALSE
    );
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_receipt_acknowledgement_intend_v1(
    expected_application_id TEXT,
    expected_interaction_id TEXT,
    expected_head_revision BIGINT,
    expected_claim_revision BIGINT,
    expected_process_instance_id TEXT,
    proposed_acknowledgement_kind TEXT,
    proposed_acknowledgement_digest BYTEA
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
    head_row public.runtime_interaction_receipt_heads_v1%ROWTYPE;
    secret_row public.runtime_interaction_receipt_token_secrets_v1%ROWTYPE;
    database_now TIMESTAMPTZ;
    next_state TEXT;
BEGIN
    IF expected_application_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_application_id) > 20
        OR expected_interaction_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_interaction_id) > 20
        OR expected_head_revision NOT BETWEEN 1 AND 9223372036854775806
        OR expected_claim_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR proposed_acknowledgement_kind NOT IN (
            'defer_ephemeral',
            'respond_ephemeral',
            'respond_message',
            'open_modal',
            'update_message'
        )
        OR pg_catalog.octet_length(proposed_acknowledgement_digest) <> 32
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_receipt_ack_intent_input_invalid';
    END IF;

    SELECT head.*
    INTO head_row
    FROM public.runtime_interaction_receipt_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_receipt_not_found';
    END IF;

    SELECT secret.*
    INTO secret_row
    FROM public.runtime_interaction_receipt_token_secrets_v1 AS secret
    WHERE secret.application_id = expected_application_id
        AND secret.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    database_now := pg_catalog.clock_timestamp();

    IF NOT FOUND OR secret_row.expires_at <= database_now THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_receipt_token_expired';
    END IF;

    IF head_row.acknowledgement_kind IS NOT NULL THEN
        IF head_row.acknowledgement_kind
                IS DISTINCT FROM proposed_acknowledgement_kind
            OR head_row.acknowledgement_digest
                IS DISTINCT FROM proposed_acknowledgement_digest
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI002',
                MESSAGE = 'runtime_interaction_receipt_ack_intent_corruption';
        END IF;

        IF head_row.head_revision NOT IN (
                expected_head_revision,
                expected_head_revision + 1
            )
            OR head_row.claim_revision <> expected_claim_revision
            OR head_row.claim_process_instance_id
                IS DISTINCT FROM expected_process_instance_id
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI001',
                MESSAGE = 'runtime_interaction_receipt_ack_intent_conflict';
        END IF;

        IF NOT public.starring_runtime_interaction_receipt_claim_current_v1(
            expected_application_id,
            expected_interaction_id,
            expected_claim_revision,
            expected_process_instance_id,
            database_now
        ) THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI004',
                MESSAGE = 'runtime_interaction_receipt_claim_stale';
        END IF;

        database_now := pg_catalog.clock_timestamp();

        IF head_row.claim_expires_at <= database_now THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI004',
                MESSAGE = 'runtime_interaction_receipt_claim_stale';
        END IF;

        IF secret_row.expires_at <= database_now THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI004',
                MESSAGE = 'runtime_interaction_receipt_token_expired';
        END IF;

        outcome_name := 'exact_replay';
        receipt_state := head_row.state;
        resulting_head_revision := head_row.head_revision;
        resulting_claim_revision := head_row.claim_revision;
        resulting_claim_expires_at := head_row.claim_expires_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF head_row.action_plan_digest IS NULL
        AND proposed_acknowledgement_kind <> 'defer_ephemeral'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_receipt_response_plan_unbound';
    END IF;

    IF head_row.state NOT IN ('claimed', 'prepared', 'executing')
        OR head_row.head_revision <> expected_head_revision
        OR head_row.claim_revision <> expected_claim_revision
        OR head_row.claim_process_instance_id
            IS DISTINCT FROM expected_process_instance_id
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_receipt_ack_intent_conflict';
    END IF;

    IF NOT public.starring_runtime_interaction_receipt_claim_current_v1(
        expected_application_id,
        expected_interaction_id,
        expected_claim_revision,
        expected_process_instance_id,
        database_now
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_receipt_claim_stale';
    END IF;

    database_now := pg_catalog.clock_timestamp();

    IF head_row.claim_expires_at <= database_now THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_receipt_claim_stale';
    END IF;

    IF secret_row.expires_at <= database_now THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_receipt_token_expired';
    END IF;

    next_state := CASE
        WHEN head_row.state = 'executing' THEN 'executing'
        ELSE 'acknowledging'
    END;

    UPDATE public.runtime_interaction_receipt_heads_v1 AS head
    SET state = next_state,
        acknowledgement_state = 'attempting',
        head_revision = head.head_revision + 1,
        acknowledgement_kind = proposed_acknowledgement_kind,
        acknowledgement_digest = proposed_acknowledgement_digest,
        acknowledgement_intended_at = database_now,
        updated_at = database_now
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
        'acknowledgement_intended',
        head_row.state,
        next_state,
        head_row.acknowledgement_state,
        'attempting',
        head_row.claim_revision,
        head_row.claim_process_instance_id,
        head_row.claim_gateway_shard_id,
        head_row.claim_gateway_owner_lease_epoch,
        head_row.claim_gateway_owner_revision,
        head_row.claim_serving_lease_epoch,
        head_row.claim_serving_revision,
        'acknowledgement_intended',
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.concat_ws(
                '|',
                'starring-runtime-interaction-receipt-event-v1',
                expected_application_id,
                expected_interaction_id,
                (head_row.head_revision + 1)::TEXT,
                'acknowledgement_intended',
                head_row.state,
                next_state,
                head_row.acknowledgement_state,
                'attempting',
                head_row.claim_revision::TEXT,
                head_row.claim_process_instance_id,
                head_row.claim_gateway_shard_id,
                head_row.claim_gateway_owner_lease_epoch::TEXT,
                head_row.claim_gateway_owner_revision::TEXT,
                head_row.claim_serving_lease_epoch::TEXT,
                head_row.claim_serving_revision::TEXT,
                'acknowledgement_intended'
            ),
            'UTF8'
        )),
        database_now
    );

    outcome_name := 'acknowledgement_intended';
    receipt_state := next_state;
    resulting_head_revision := head_row.head_revision + 1;
    resulting_claim_revision := head_row.claim_revision;
    resulting_claim_expires_at := head_row.claim_expires_at;
    observed_database_now := database_now;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_receipt_acknowledgement_finish_v1(
    expected_application_id TEXT,
    expected_interaction_id TEXT,
    expected_head_revision BIGINT,
    expected_claim_revision BIGINT,
    expected_process_instance_id TEXT,
    expected_acknowledgement_digest BYTEA,
    proposed_acknowledgement_result TEXT,
    proposed_acknowledgement_result_digest BYTEA
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
    head_row public.runtime_interaction_receipt_heads_v1%ROWTYPE;
    database_now TIMESTAMPTZ;
    next_state TEXT;
    next_acknowledgement_state TEXT;
    next_event_kind TEXT;
    next_outcome_code TEXT;
BEGIN
    IF expected_application_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_application_id) > 20
        OR expected_interaction_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_interaction_id) > 20
        OR expected_head_revision NOT BETWEEN 1 AND 9223372036854775806
        OR expected_claim_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR pg_catalog.octet_length(expected_acknowledgement_digest) <> 32
        OR proposed_acknowledgement_result NOT IN (
            'succeeded',
            'definitive_failure',
            'indeterminate'
        )
        OR pg_catalog.octet_length(
            proposed_acknowledgement_result_digest
        ) <> 32
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_receipt_ack_result_input_invalid';
    END IF;

    database_now := pg_catalog.clock_timestamp();

    SELECT head.*
    INTO head_row
    FROM public.runtime_interaction_receipt_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_receipt_not_found';
    END IF;

    IF head_row.acknowledgement_digest
            IS DISTINCT FROM expected_acknowledgement_digest
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_receipt_ack_result_corruption';
    END IF;

    IF head_row.acknowledgement_result IS NOT NULL THEN
        IF head_row.acknowledgement_result
                IS DISTINCT FROM proposed_acknowledgement_result
            OR head_row.acknowledgement_result_digest
                IS DISTINCT FROM proposed_acknowledgement_result_digest
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI002',
                MESSAGE = 'runtime_interaction_receipt_ack_result_corruption';
        END IF;

        IF head_row.head_revision NOT IN (
                expected_head_revision,
                expected_head_revision + 1
            )
            OR head_row.claim_revision <> expected_claim_revision
            OR head_row.claim_process_instance_id
                IS DISTINCT FROM expected_process_instance_id
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI001',
                MESSAGE = 'runtime_interaction_receipt_ack_result_conflict';
        END IF;

        IF head_row.state NOT IN ('completed', 'failed', 'recovery_required')
            AND NOT public.starring_runtime_interaction_receipt_claim_current_v1(
                expected_application_id,
                expected_interaction_id,
                expected_claim_revision,
                expected_process_instance_id,
                database_now
            )
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI004',
                MESSAGE = 'runtime_interaction_receipt_claim_stale';
        END IF;

        outcome_name := 'exact_replay';
        receipt_state := head_row.state;
        resulting_head_revision := head_row.head_revision;
        resulting_claim_revision := head_row.claim_revision;
        resulting_claim_expires_at := head_row.claim_expires_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF head_row.state NOT IN ('acknowledging', 'executing')
        OR head_row.head_revision <> expected_head_revision
        OR head_row.claim_revision <> expected_claim_revision
        OR head_row.claim_process_instance_id
            IS DISTINCT FROM expected_process_instance_id
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_receipt_ack_result_conflict';
    END IF;

    IF NOT public.starring_runtime_interaction_receipt_claim_current_v1(
        expected_application_id,
        expected_interaction_id,
        expected_claim_revision,
        expected_process_instance_id,
        database_now
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_receipt_claim_stale';
    END IF;

    IF proposed_acknowledgement_result = 'succeeded'
        AND head_row.acknowledgement_kind <> 'defer_ephemeral'
        AND head_row.action_plan_digest IS NULL
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_receipt_response_plan_unbound';
    END IF;

    next_state := CASE proposed_acknowledgement_result
        WHEN 'succeeded' THEN CASE
            WHEN head_row.state = 'executing' THEN 'executing'
            WHEN head_row.acknowledgement_kind = 'defer_ephemeral'
                THEN 'deferred'
            ELSE 'prepared'
        END
        WHEN 'definitive_failure' THEN CASE
            WHEN head_row.state = 'executing' THEN 'recovery_required'
            ELSE 'failed'
        END
        ELSE 'recovery_required'
    END;
    next_event_kind := CASE proposed_acknowledgement_result
        WHEN 'succeeded' THEN 'acknowledgement_succeeded'
        WHEN 'definitive_failure' THEN 'acknowledgement_failed'
        ELSE 'acknowledgement_indeterminate'
    END;
    next_outcome_code := CASE
        WHEN proposed_acknowledgement_result = 'definitive_failure'
            AND head_row.state = 'executing'
            THEN 'acknowledgement_failure_after_execution_intent'
        ELSE CASE proposed_acknowledgement_result
        WHEN 'succeeded' THEN 'acknowledgement_succeeded'
        WHEN 'definitive_failure' THEN 'acknowledgement_definitive_failure'
        ELSE 'acknowledgement_indeterminate'
        END
    END;

    UPDATE public.runtime_interaction_receipt_heads_v1 AS head
    SET state = next_state,
        acknowledgement_state = CASE proposed_acknowledgement_result
            WHEN 'succeeded' THEN CASE
                WHEN head_row.acknowledgement_kind = 'defer_ephemeral'
                    THEN 'deferred'
                ELSE 'responded'
            END
            ELSE 'response_recovery_terminal'
        END,
        head_revision = head.head_revision + 1,
        acknowledgement_result = proposed_acknowledgement_result,
        acknowledgement_result_digest =
            proposed_acknowledgement_result_digest,
        acknowledged_at = database_now,
        terminal_outcome_code = CASE
            WHEN proposed_acknowledgement_result = 'succeeded' THEN NULL
            ELSE next_outcome_code
        END,
        terminal_result_digest = CASE
            WHEN proposed_acknowledgement_result = 'succeeded' THEN NULL
            ELSE proposed_acknowledgement_result_digest
        END,
        terminal_at = CASE
            WHEN proposed_acknowledgement_result = 'succeeded' THEN NULL
            ELSE database_now
        END,
        updated_at = database_now
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
        next_event_kind,
        head_row.state,
        next_state,
        'attempting',
        CASE proposed_acknowledgement_result
            WHEN 'succeeded' THEN CASE
                WHEN head_row.acknowledgement_kind = 'defer_ephemeral'
                    THEN 'deferred'
                ELSE 'responded'
            END
            ELSE 'response_recovery_terminal'
        END,
        head_row.claim_revision,
        head_row.claim_process_instance_id,
        head_row.claim_gateway_shard_id,
        head_row.claim_gateway_owner_lease_epoch,
        head_row.claim_gateway_owner_revision,
        head_row.claim_serving_lease_epoch,
        head_row.claim_serving_revision,
        next_outcome_code,
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.concat_ws(
                '|',
                'starring-runtime-interaction-receipt-event-v1',
                expected_application_id,
                expected_interaction_id,
                (head_row.head_revision + 1)::TEXT,
                next_event_kind,
                head_row.state,
                next_state,
                'attempting',
                CASE proposed_acknowledgement_result
                    WHEN 'succeeded' THEN CASE
                        WHEN head_row.acknowledgement_kind = 'defer_ephemeral'
                            THEN 'deferred'
                        ELSE 'responded'
                    END
                    ELSE 'response_recovery_terminal'
                END,
                head_row.claim_revision::TEXT,
                head_row.claim_process_instance_id,
                head_row.claim_gateway_shard_id,
                head_row.claim_gateway_owner_lease_epoch::TEXT,
                head_row.claim_gateway_owner_revision::TEXT,
                head_row.claim_serving_lease_epoch::TEXT,
                head_row.claim_serving_revision::TEXT,
                next_outcome_code
            ),
            'UTF8'
        )),
        database_now
    );

    IF proposed_acknowledgement_result <> 'succeeded' THEN
        DELETE FROM public.runtime_interaction_receipt_token_secrets_v1
        WHERE application_id = expected_application_id
            AND interaction_id = expected_interaction_id;
    END IF;

    outcome_name := next_outcome_code;
    receipt_state := next_state;
    resulting_head_revision := head_row.head_revision + 1;
    resulting_claim_revision := head_row.claim_revision;
    resulting_claim_expires_at := head_row.claim_expires_at;
    observed_database_now := database_now;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_receipt_execution_intend_v1(
    expected_application_id TEXT,
    expected_interaction_id TEXT,
    expected_head_revision BIGINT,
    expected_claim_revision BIGINT,
    expected_process_instance_id TEXT,
    expected_action_plan_digest BYTEA
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
    head_row public.runtime_interaction_receipt_heads_v1%ROWTYPE;
    database_now TIMESTAMPTZ;
BEGIN
    IF expected_application_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_application_id) > 20
        OR expected_interaction_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_interaction_id) > 20
        OR expected_head_revision NOT BETWEEN 1 AND 9223372036854775806
        OR expected_claim_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR pg_catalog.octet_length(expected_action_plan_digest) <> 32
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_receipt_execution_intent_input_invalid';
    END IF;

    SELECT head.*
    INTO head_row
    FROM public.runtime_interaction_receipt_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_receipt_not_found';
    END IF;

    database_now := pg_catalog.clock_timestamp();

    IF head_row.action_plan_digest
        IS DISTINCT FROM expected_action_plan_digest
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_receipt_plan_corruption';
    END IF;

    IF head_row.execution_intended_at IS NOT NULL THEN
        IF head_row.head_revision NOT IN (
                expected_head_revision,
                expected_head_revision + 1
            )
            OR head_row.claim_revision <> expected_claim_revision
            OR head_row.claim_process_instance_id
                IS DISTINCT FROM expected_process_instance_id
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI001',
                MESSAGE = 'runtime_interaction_receipt_execution_intent_conflict';
        END IF;

        IF NOT public.starring_runtime_interaction_receipt_claim_current_v1(
            expected_application_id,
            expected_interaction_id,
            expected_claim_revision,
            expected_process_instance_id,
            database_now
        ) THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI004',
                MESSAGE = 'runtime_interaction_receipt_claim_stale';
        END IF;

        database_now := pg_catalog.clock_timestamp();

        IF head_row.claim_expires_at <= database_now THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI004',
                MESSAGE = 'runtime_interaction_receipt_claim_stale';
        END IF;

        outcome_name := 'exact_replay';
        receipt_state := head_row.state;
        resulting_head_revision := head_row.head_revision;
        resulting_claim_revision := head_row.claim_revision;
        resulting_claim_expires_at := head_row.claim_expires_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF head_row.state <> 'prepared'
        OR head_row.head_revision <> expected_head_revision
        OR head_row.claim_revision <> expected_claim_revision
        OR head_row.claim_process_instance_id
            IS DISTINCT FROM expected_process_instance_id
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_receipt_execution_intent_conflict';
    END IF;

    IF NOT public.starring_runtime_interaction_receipt_claim_current_v1(
        expected_application_id,
        expected_interaction_id,
        expected_claim_revision,
        expected_process_instance_id,
        database_now
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_receipt_claim_stale';
    END IF;

    database_now := pg_catalog.clock_timestamp();

    IF head_row.claim_expires_at <= database_now THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_receipt_claim_stale';
    END IF;

    UPDATE public.runtime_interaction_receipt_heads_v1 AS head
    SET state = 'executing',
        head_revision = head.head_revision + 1,
        execution_intended_at = database_now,
        updated_at = database_now
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
        'execution_intended',
        'prepared',
        'executing',
        head_row.acknowledgement_state,
        head_row.acknowledgement_state,
        head_row.claim_revision,
        head_row.claim_process_instance_id,
        head_row.claim_gateway_shard_id,
        head_row.claim_gateway_owner_lease_epoch,
        head_row.claim_gateway_owner_revision,
        head_row.claim_serving_lease_epoch,
        head_row.claim_serving_revision,
        'execution_intended',
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.concat_ws(
                '|',
                'starring-runtime-interaction-receipt-event-v1',
                expected_application_id,
                expected_interaction_id,
                (head_row.head_revision + 1)::TEXT,
                'execution_intended',
                'prepared',
                'executing',
                head_row.acknowledgement_state,
                head_row.acknowledgement_state,
                head_row.claim_revision::TEXT,
                head_row.claim_process_instance_id,
                head_row.claim_gateway_shard_id,
                head_row.claim_gateway_owner_lease_epoch::TEXT,
                head_row.claim_gateway_owner_revision::TEXT,
                head_row.claim_serving_lease_epoch::TEXT,
                head_row.claim_serving_revision::TEXT,
                'execution_intended'
            ),
            'UTF8'
        )),
        database_now
    );

    outcome_name := 'execution_intended';
    receipt_state := 'executing';
    resulting_head_revision := head_row.head_revision + 1;
    resulting_claim_revision := head_row.claim_revision;
    resulting_claim_expires_at := head_row.claim_expires_at;
    observed_database_now := database_now;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_receipt_finish_v1(
    expected_application_id TEXT,
    expected_interaction_id TEXT,
    expected_head_revision BIGINT,
    expected_claim_revision BIGINT,
    expected_process_instance_id TEXT,
    expected_action_plan_digest BYTEA,
    proposed_terminal_state TEXT,
    proposed_terminal_outcome_code TEXT,
    proposed_terminal_result_digest BYTEA
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
    head_row public.runtime_interaction_receipt_heads_v1%ROWTYPE;
    database_now TIMESTAMPTZ;
BEGIN
    IF expected_application_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_application_id) > 20
        OR expected_interaction_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_interaction_id) > 20
        OR expected_head_revision NOT BETWEEN 1 AND 9223372036854775806
        OR expected_claim_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR pg_catalog.octet_length(expected_action_plan_digest) NOT IN (0, 32)
        OR proposed_terminal_state NOT IN (
            'completed',
            'failed',
            'recovery_required'
        )
        OR proposed_terminal_outcome_code !~ '^[a-z0-9_]{1,64}$'
        OR proposed_terminal_outcome_code = 'exact_replay'
        OR pg_catalog.octet_length(proposed_terminal_result_digest) <> 32
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_receipt_finish_input_invalid';
    END IF;

    database_now := pg_catalog.clock_timestamp();

    SELECT head.*
    INTO head_row
    FROM public.runtime_interaction_receipt_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_receipt_not_found';
    END IF;

    IF head_row.action_plan_digest IS DISTINCT FROM
        NULLIF(expected_action_plan_digest, ''::BYTEA)
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_receipt_plan_corruption';
    END IF;

    IF head_row.state IN ('completed', 'failed', 'recovery_required') THEN
        IF head_row.state IS DISTINCT FROM proposed_terminal_state
            OR head_row.terminal_outcome_code
                IS DISTINCT FROM proposed_terminal_outcome_code
            OR head_row.terminal_result_digest
                IS DISTINCT FROM proposed_terminal_result_digest
        THEN
            RAISE EXCEPTION USING
                ERRCODE = 'RI002',
                MESSAGE = 'runtime_interaction_receipt_terminal_corruption';
        END IF;

        outcome_name := 'exact_replay';
        receipt_state := head_row.state;
        resulting_head_revision := head_row.head_revision;
        resulting_claim_revision := head_row.claim_revision;
        resulting_claim_expires_at := head_row.claim_expires_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF head_row.head_revision <> expected_head_revision
        OR head_row.claim_revision <> expected_claim_revision
        OR head_row.claim_process_instance_id
            IS DISTINCT FROM expected_process_instance_id
        OR (
            proposed_terminal_state = 'completed'
            AND (
                head_row.state NOT IN ('prepared', 'executing')
                OR head_row.action_plan_digest IS NULL
                OR head_row.acknowledgement_state IN (
                    'attempting',
                    'response_recovery_terminal'
                )
            )
        )
        OR (
            proposed_terminal_state <> 'completed'
            AND head_row.state NOT IN (
                'claimed',
                'deferred',
                'prepared',
                'executing'
            )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_receipt_finish_conflict';
    END IF;

    IF NOT public.starring_runtime_interaction_receipt_claim_current_v1(
        expected_application_id,
        expected_interaction_id,
        expected_claim_revision,
        expected_process_instance_id,
        database_now
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_receipt_claim_stale';
    END IF;

    UPDATE public.runtime_interaction_receipt_heads_v1 AS head
    SET state = proposed_terminal_state,
        head_revision = head.head_revision + 1,
        terminal_outcome_code = proposed_terminal_outcome_code,
        terminal_result_digest = proposed_terminal_result_digest,
        terminal_at = database_now,
        updated_at = database_now
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
        proposed_terminal_state,
        head_row.state,
        proposed_terminal_state,
        head_row.acknowledgement_state,
        head_row.acknowledgement_state,
        head_row.claim_revision,
        head_row.claim_process_instance_id,
        head_row.claim_gateway_shard_id,
        head_row.claim_gateway_owner_lease_epoch,
        head_row.claim_gateway_owner_revision,
        head_row.claim_serving_lease_epoch,
        head_row.claim_serving_revision,
        proposed_terminal_outcome_code,
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.concat_ws(
                '|',
                'starring-runtime-interaction-receipt-event-v1',
                expected_application_id,
                expected_interaction_id,
                (head_row.head_revision + 1)::TEXT,
                proposed_terminal_state,
                head_row.state,
                proposed_terminal_state,
                head_row.acknowledgement_state,
                head_row.acknowledgement_state,
                head_row.claim_revision::TEXT,
                head_row.claim_process_instance_id,
                head_row.claim_gateway_shard_id,
                head_row.claim_gateway_owner_lease_epoch::TEXT,
                head_row.claim_gateway_owner_revision::TEXT,
                head_row.claim_serving_lease_epoch::TEXT,
                head_row.claim_serving_revision::TEXT,
                proposed_terminal_outcome_code
            ),
            'UTF8'
        )),
        database_now
    );

    DELETE FROM public.runtime_interaction_receipt_token_secrets_v1
    WHERE application_id = expected_application_id
        AND interaction_id = expected_interaction_id;

    outcome_name := proposed_terminal_outcome_code;
    receipt_state := proposed_terminal_state;
    resulting_head_revision := head_row.head_revision + 1;
    resulting_claim_revision := head_row.claim_revision;
    resulting_claim_expires_at := head_row.claim_expires_at;
    observed_database_now := database_now;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_receipt_scan_recoverable_v1(
    expected_after_claim_expires_at TIMESTAMPTZ,
    expected_after_application_id TEXT,
    expected_after_interaction_id TEXT,
    expected_through_claim_expires_at TIMESTAMPTZ,
    expected_through_application_id TEXT,
    expected_through_interaction_id TEXT,
    expected_limit BIGINT
)
RETURNS TABLE(
    application_id TEXT,
    interaction_id TEXT,
    receipt_state TEXT,
    head_revision BIGINT,
    claim_revision BIGINT,
    claim_expires_at TIMESTAMPTZ,
    token_expires_at TIMESTAMPTZ,
    through_claim_expires_at TIMESTAMPTZ,
    through_application_id TEXT,
    through_interaction_id TEXT,
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
    cycle_through_claim_expires_at TIMESTAMPTZ;
    cycle_through_application_id TEXT;
    cycle_through_interaction_id TEXT;
    database_now TIMESTAMPTZ;
BEGIN
    IF NOT pg_catalog.isfinite(expected_after_claim_expires_at)
        OR NOT pg_catalog.isfinite(expected_through_claim_expires_at)
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
            expected_after_application_id <> ''
            AND (
                expected_after_application_id !~ '^[1-9][0-9]{0,19}$'
                OR pg_catalog.length(expected_after_application_id) > 20
                OR expected_after_interaction_id !~ '^[1-9][0-9]{0,19}$'
                OR pg_catalog.length(expected_after_interaction_id) > 20
            )
        )
        OR (
            expected_through_application_id <> ''
            AND (
                expected_through_application_id !~ '^[1-9][0-9]{0,19}$'
                OR pg_catalog.length(expected_through_application_id) > 20
                OR expected_through_interaction_id !~ '^[1-9][0-9]{0,19}$'
                OR pg_catalog.length(expected_through_interaction_id) > 20
            )
        )
        OR (
            expected_after_application_id = ''
            AND expected_after_claim_expires_at
                <> '1970-01-01 00:00:00+00'::TIMESTAMPTZ
        )
        OR (
            expected_through_application_id = ''
            AND expected_through_claim_expires_at
                <> '1970-01-01 00:00:00+00'::TIMESTAMPTZ
        )
        OR (
            expected_through_application_id = ''
            AND expected_after_application_id <> ''
        )
        OR (
            expected_after_application_id <> ''
            AND ROW(
                expected_after_claim_expires_at,
                expected_after_application_id COLLATE "C",
                expected_after_interaction_id COLLATE "C"
            ) >= ROW(
                expected_through_claim_expires_at,
                expected_through_application_id COLLATE "C",
                expected_through_interaction_id COLLATE "C"
            )
        )
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_receipt_recovery_scan_input_invalid';
    END IF;

    database_now := pg_catalog.clock_timestamp();

    IF expected_through_application_id = '' THEN
        SELECT
            head.claim_expires_at,
            head.application_id,
            head.interaction_id
        INTO
            cycle_through_claim_expires_at,
            cycle_through_application_id,
            cycle_through_interaction_id
        FROM public.runtime_interaction_receipt_heads_v1 AS head
        WHERE head.state IN (
                'claimed',
                'acknowledging',
                'deferred',
                'prepared',
                'executing'
            )
            AND head.claim_expires_at <= database_now
        ORDER BY
            head.claim_expires_at DESC,
            head.application_id COLLATE "C" DESC,
            head.interaction_id COLLATE "C" DESC
        LIMIT 1;

        IF NOT FOUND THEN
            RETURN;
        END IF;
    ELSE
        cycle_through_claim_expires_at :=
            expected_through_claim_expires_at;
        cycle_through_application_id := expected_through_application_id;
        cycle_through_interaction_id := expected_through_interaction_id;
    END IF;

    RETURN QUERY
    SELECT
        head.application_id,
        head.interaction_id,
        head.state,
        head.head_revision,
        head.claim_revision,
        head.claim_expires_at,
        secret.expires_at,
        cycle_through_claim_expires_at,
        cycle_through_application_id,
        cycle_through_interaction_id,
        database_now
    FROM public.runtime_interaction_receipt_heads_v1 AS head
    LEFT JOIN public.runtime_interaction_receipt_token_secrets_v1 AS secret
        ON secret.application_id = head.application_id
        AND secret.interaction_id = head.interaction_id
    WHERE head.state IN (
            'claimed',
            'acknowledging',
            'deferred',
            'prepared',
            'executing'
        )
        AND head.claim_expires_at <= database_now
        AND (
            expected_after_application_id = ''
            OR ROW(
                head.claim_expires_at,
                head.application_id COLLATE "C",
                head.interaction_id COLLATE "C"
            ) > ROW(
                expected_after_claim_expires_at,
                expected_after_application_id COLLATE "C",
                expected_after_interaction_id COLLATE "C"
            )
        )
        AND ROW(
            head.claim_expires_at,
            head.application_id COLLATE "C",
            head.interaction_id COLLATE "C"
        ) <= ROW(
            cycle_through_claim_expires_at,
            cycle_through_application_id COLLATE "C",
            cycle_through_interaction_id COLLATE "C"
        )
    ORDER BY
        head.claim_expires_at,
        head.application_id COLLATE "C",
        head.interaction_id COLLATE "C"
    LIMIT expected_limit;
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_receipt_recover_v1(
    expected_application_id TEXT,
    expected_interaction_id TEXT,
    expected_head_revision BIGINT,
    expected_claim_revision BIGINT,
    expected_process_instance_id TEXT,
    expected_runtime_generation BIGINT,
    expected_controller_fencing_token BIGINT,
    expected_route_incarnation BIGINT,
    expected_gateway_shard_id TEXT,
    expected_runtime_build_revision TEXT,
    proposed_observation_kind TEXT,
    proposed_observation_digest BYTEA,
    requested_claim_lease_milliseconds BIGINT
)
RETURNS TABLE(
    outcome_name TEXT,
    receipt_state TEXT,
    resulting_head_revision BIGINT,
    resulting_claim_revision BIGINT,
    resulting_claim_expires_at TIMESTAMPTZ,
    resulting_gateway_owner_lease_epoch BIGINT,
    resulting_gateway_owner_revision BIGINT,
    resulting_serving_lease_epoch BIGINT,
    resulting_serving_revision BIGINT,
    root_tenant_id TEXT,
    root_installation_id TEXT,
    root_deployment_id TEXT,
    root_attestation_digest TEXT,
    root_guild_id TEXT,
    root_ruleset_key TEXT,
    root_target_version BIGINT,
    root_target_content_hash TEXT,
    root_binding_revision BIGINT,
    root_binding_fingerprint TEXT,
    root_runtime_generation BIGINT,
    root_process_instance_id TEXT,
    root_serving_lease_epoch BIGINT,
    root_serving_revision BIGINT,
    root_gateway_shard_id TEXT,
    root_gateway_owner_lease_epoch BIGINT,
    root_gateway_owner_revision BIGINT,
    root_route_controller_fencing_token BIGINT,
    root_route_incarnation BIGINT,
    root_runtime_build_revision TEXT,
    root_route_kind TEXT,
    root_route_key TEXT,
    root_instance_id TEXT,
    root_execution_ruleset_version BIGINT,
    root_execution_ruleset_content_hash TEXT,
    root_instance_manifest_digest TEXT,
    root_request_digest BYTEA,
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
    authority_row RECORD;
    owner_row public.runtime_gateway_owners%ROWTYPE;
    root_row public.runtime_interaction_receipt_roots_v1%ROWTYPE;
    head_row public.runtime_interaction_receipt_heads_v1%ROWTYPE;
    secret_row public.runtime_interaction_receipt_token_secrets_v1%ROWTYPE;
    database_now TIMESTAMPTZ;
    next_state TEXT;
    next_acknowledgement_state TEXT;
    next_event_kind TEXT;
    next_outcome_code TEXT;
    new_claim_revision BIGINT;
    new_head_revision BIGINT;
    new_claim_expiry TIMESTAMPTZ;
BEGIN
    IF expected_application_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_application_id) > 20
        OR expected_interaction_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_interaction_id) > 20
        OR expected_head_revision NOT BETWEEN 1 AND 9223372036854775806
        OR expected_claim_revision NOT BETWEEN 1 AND 9223372036854775806
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_runtime_generation NOT BETWEEN 1 AND 9223372036854775807
        OR expected_controller_fencing_token
            NOT BETWEEN 1 AND 9223372036854775807
        OR expected_route_incarnation NOT BETWEEN 1 AND 9223372036854775807
        OR expected_gateway_shard_id !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR expected_runtime_build_revision !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR proposed_observation_kind NOT IN (
            'unacknowledged',
            'acknowledged',
            'mutations_reconciled',
            'response_not_observable'
        )
        OR pg_catalog.octet_length(proposed_observation_digest) <> 32
        OR requested_claim_lease_milliseconds NOT BETWEEN 1000 AND 300000
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_receipt_recovery_input_invalid';
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

    database_now := pg_catalog.clock_timestamp();

    SELECT root.*
    INTO root_row
    FROM public.runtime_interaction_receipt_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_receipt_not_found';
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
            MESSAGE = 'runtime_interaction_receipt_head_missing';
    END IF;

    IF head_row.state IN ('completed', 'failed', 'recovery_required') THEN
        outcome_name := 'terminal_duplicate';
        receipt_state := head_row.state;
        resulting_head_revision := head_row.head_revision;
        resulting_claim_revision := head_row.claim_revision;
        resulting_claim_expires_at := head_row.claim_expires_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF head_row.head_revision <> expected_head_revision
        OR head_row.claim_revision <> expected_claim_revision
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_receipt_recovery_revision_conflict';
    END IF;

    IF head_row.claim_expires_at > database_now THEN
        outcome_name := 'in_flight_duplicate';
        receipt_state := head_row.state;
        resulting_head_revision := head_row.head_revision;
        resulting_claim_revision := head_row.claim_revision;
        resulting_claim_expires_at := head_row.claim_expires_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF head_row.state = 'claimed'
        AND head_row.acknowledgement_state = 'unacknowledged'
        AND head_row.action_plan_digest IS NULL
        AND head_row.acknowledgement_kind IS NULL
        AND head_row.acknowledgement_intended_at IS NULL
        AND head_row.execution_intended_at IS NULL
        AND proposed_observation_kind <> 'unacknowledged'
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_receipt_recovery_observation_conflict';
    END IF;

    IF root_row.origin_process_instance_id
            IS DISTINCT FROM expected_process_instance_id
        OR root_row.runtime_build_revision
            IS DISTINCT FROM expected_runtime_build_revision
    THEN
        outcome_name := 'successor_process_recovery_deferred';
        receipt_state := head_row.state;
        resulting_head_revision := head_row.head_revision;
        resulting_claim_revision := head_row.claim_revision;
        resulting_claim_expires_at := head_row.claim_expires_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF root_row.runtime_generation
            IS DISTINCT FROM expected_runtime_generation
        OR root_row.route_controller_fencing_token
            IS DISTINCT FROM expected_controller_fencing_token
        OR root_row.route_incarnation
            IS DISTINCT FROM expected_route_incarnation
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_receipt_recovery_route_stale';
    END IF;

    SELECT observed.*
    INTO authority_row
    FROM public.starring_runtime_interaction_receipt_authority_observe_v1(
        root_row.application_id,
        root_row.tenant_id,
        root_row.installation_id,
        root_row.deployment_id,
        root_row.guild_id,
        root_row.ruleset_key,
        root_row.target_version,
        root_row.target_content_hash,
        root_row.binding_revision,
        root_row.binding_fingerprint,
        root_row.runtime_generation,
        expected_controller_fencing_token,
        expected_route_incarnation,
        expected_process_instance_id,
        expected_gateway_shard_id,
        expected_runtime_build_revision,
        root_row.route_kind,
        COALESCE(root_row.instance_id, '')
    ) AS observed;

    IF NOT FOUND
        OR authority_row.attestation_id
            IS DISTINCT FROM root_row.attestation_id
        OR authority_row.attestation_digest
            IS DISTINCT FROM root_row.attestation_digest
        OR authority_row.route_controller_fencing_token
            IS DISTINCT FROM root_row.route_controller_fencing_token
        OR authority_row.route_incarnation
            IS DISTINCT FROM root_row.route_incarnation
        OR authority_row.runtime_build_revision
            IS DISTINCT FROM root_row.runtime_build_revision
        OR authority_row.execution_ruleset_version
            IS DISTINCT FROM root_row.execution_ruleset_version
        OR authority_row.execution_ruleset_content_hash
            IS DISTINCT FROM root_row.execution_ruleset_content_hash
        OR authority_row.instance_manifest_digest
            IS DISTINCT FROM root_row.instance_manifest_digest
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_receipt_recovery_authority_invalid';
    END IF;

    SELECT secret.*
    INTO secret_row
    FROM public.runtime_interaction_receipt_token_secrets_v1 AS secret
    WHERE secret.application_id = expected_application_id
        AND secret.interaction_id = expected_interaction_id
    FOR UPDATE;

    IF NOT (
            head_row.state = 'claimed'
            AND head_row.acknowledgement_state = 'unacknowledged'
            AND head_row.action_plan_digest IS NULL
            AND head_row.acknowledgement_kind IS NULL
            AND head_row.acknowledgement_intended_at IS NULL
            AND head_row.execution_intended_at IS NULL
        )
        OR proposed_observation_kind = 'response_not_observable'
        OR secret_row.application_id IS NULL
        OR secret_row.expires_at <= database_now
    THEN
        next_outcome_code := CASE
            WHEN head_row.execution_intended_at IS NOT NULL
                THEN 'expired_claim_recovery_required'
            WHEN proposed_observation_kind = 'response_not_observable'
                THEN 'interaction_response_unrecoverable'
            WHEN secret_row.application_id IS NULL
                OR secret_row.expires_at <= database_now
                THEN 'interaction_token_unavailable'
            ELSE 'expired_claim_recovery_required'
        END;
        next_acknowledgement_state := CASE
            WHEN head_row.acknowledgement_state = 'attempting'
                AND proposed_observation_kind = 'acknowledged'
                THEN CASE
                    WHEN head_row.acknowledgement_kind = 'defer_ephemeral'
                        THEN 'deferred'
                    ELSE 'responded'
                END
            WHEN head_row.acknowledgement_state = 'attempting'
                THEN 'response_recovery_terminal'
            ELSE head_row.acknowledgement_state
        END;

        UPDATE public.runtime_interaction_receipt_heads_v1 AS head
        SET state = 'recovery_required',
            acknowledgement_state = next_acknowledgement_state,
            head_revision = head.head_revision + 1,
            acknowledgement_result = CASE
                WHEN head_row.acknowledgement_state = 'attempting'
                    AND proposed_observation_kind = 'acknowledged'
                    THEN 'succeeded'
                WHEN head_row.acknowledgement_state = 'attempting'
                    THEN 'indeterminate'
                ELSE head.acknowledgement_result
            END,
            acknowledgement_result_digest = CASE
                WHEN head_row.acknowledgement_state = 'attempting'
                    THEN proposed_observation_digest
                ELSE head.acknowledgement_result_digest
            END,
            acknowledged_at = CASE
                WHEN head_row.acknowledgement_state = 'attempting'
                    THEN database_now
                ELSE head.acknowledged_at
            END,
            terminal_outcome_code = next_outcome_code,
            terminal_result_digest = proposed_observation_digest,
            terminal_at = database_now,
            updated_at = database_now
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
            'recovery_required',
            head_row.state,
            'recovery_required',
            head_row.acknowledgement_state,
            next_acknowledgement_state,
            head_row.claim_revision,
            head_row.claim_process_instance_id,
            head_row.claim_gateway_shard_id,
            head_row.claim_gateway_owner_lease_epoch,
            head_row.claim_gateway_owner_revision,
            head_row.claim_serving_lease_epoch,
            head_row.claim_serving_revision,
            next_outcome_code,
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.concat_ws(
                    '|',
                    'starring-runtime-interaction-receipt-event-v1',
                    expected_application_id,
                    expected_interaction_id,
                    (head_row.head_revision + 1)::TEXT,
                    'recovery_required',
                    head_row.state,
                    'recovery_required',
                    head_row.acknowledgement_state,
                    next_acknowledgement_state,
                    head_row.claim_revision::TEXT,
                    head_row.claim_process_instance_id,
                    head_row.claim_gateway_shard_id,
                    head_row.claim_gateway_owner_lease_epoch::TEXT,
                    head_row.claim_gateway_owner_revision::TEXT,
                    head_row.claim_serving_lease_epoch::TEXT,
                    head_row.claim_serving_revision::TEXT,
                    next_outcome_code
                ),
                'UTF8'
            )),
            database_now
        );

        DELETE FROM public.runtime_interaction_receipt_token_secrets_v1
        WHERE application_id = expected_application_id
            AND interaction_id = expected_interaction_id;

        outcome_name := next_outcome_code;
        receipt_state := 'recovery_required';
        resulting_head_revision := head_row.head_revision + 1;
        resulting_claim_revision := head_row.claim_revision;
        resulting_claim_expires_at := head_row.claim_expires_at;
        resulting_gateway_owner_lease_epoch :=
            authority_row.gateway_owner_lease_epoch;
        resulting_gateway_owner_revision :=
            authority_row.gateway_owner_revision;
        resulting_serving_lease_epoch := authority_row.serving_lease_epoch;
        resulting_serving_revision := authority_row.serving_revision;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT
        installation.tenant_id,
        installation.installation_id,
        serving.deployment_id,
        serving.attestation_id,
        serving.lease_epoch AS serving_lease_epoch,
        serving.revision AS serving_revision
    INTO authority_row
    FROM public.automation_installations AS installation
    INNER JOIN public.runtime_serving_leases AS serving
        ON serving.tenant_id = installation.tenant_id
        AND serving.installation_id = installation.installation_id
        AND serving.guild_id = installation.discord_guild_id
        AND serving.ruleset_key = installation.ruleset_key
    INNER JOIN public.runtime_deployments AS deployment
        ON deployment.tenant_id = serving.tenant_id
        AND deployment.installation_id = serving.installation_id
        AND deployment.deployment_id = serving.deployment_id
    INNER JOIN public.runtime_attestations AS attestation
        ON attestation.tenant_id = serving.tenant_id
        AND attestation.installation_id = serving.installation_id
        AND attestation.deployment_id = serving.deployment_id
        AND attestation.attestation_id = serving.attestation_id
    WHERE installation.discord_application_id = root_row.application_id
        AND installation.discord_guild_id = root_row.guild_id
        AND installation.ruleset_key = root_row.ruleset_key
        AND installation.tenant_id = root_row.tenant_id
        AND installation.installation_id = root_row.installation_id
        AND installation.lifecycle_state = 'active'
        AND serving.deployment_id = root_row.deployment_id
        AND serving.attestation_id = root_row.attestation_id
        AND serving.process_instance_id = expected_process_instance_id
        AND serving.runtime_generation = root_row.runtime_generation
        AND serving.target_version = root_row.target_version
        AND serving.target_content_hash = root_row.target_content_hash
        AND serving.binding_revision = root_row.binding_revision
        AND serving.binding_fingerprint = root_row.binding_fingerprint
        AND serving.connected
        AND serving.serving
        AND serving.expires_at > database_now
        AND deployment.phase = 'live'
        AND deployment.live_attestation_id = root_row.attestation_id
        AND deployment.guild_id = root_row.guild_id
        AND deployment.ruleset_key = root_row.ruleset_key
        AND deployment.target_version = root_row.target_version
        AND deployment.target_content_hash = root_row.target_content_hash
        AND deployment.binding_revision = root_row.binding_revision
        AND deployment.binding_fingerprint = root_row.binding_fingerprint
        AND deployment.runtime_generation = root_row.runtime_generation
        AND attestation.guild_id = root_row.guild_id
        AND attestation.ruleset_key = root_row.ruleset_key
        AND attestation.target_version = root_row.target_version
        AND attestation.target_content_hash = root_row.target_content_hash
        AND attestation.binding_revision = root_row.binding_revision
        AND attestation.binding_fingerprint = root_row.binding_fingerprint
        AND attestation.runtime_generation = root_row.runtime_generation
        AND attestation.attestation_digest = root_row.attestation_digest
        AND attestation.controller_fencing_token
            = expected_controller_fencing_token
        AND attestation.v2_route_incarnation = expected_route_incarnation
        AND attestation.record_format_version = 2
        AND attestation.process_instance_id = expected_process_instance_id
        AND attestation.runtime_build_revision
            = expected_runtime_build_revision
        AND attestation.gateway_shard_id = expected_gateway_shard_id
    FOR SHARE OF installation, serving, deployment, attestation;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_receipt_recovery_authority_invalid';
    END IF;

    SELECT owner.*
    INTO owner_row
    FROM public.runtime_gateway_owners AS owner
    WHERE owner.gateway_shard_id = expected_gateway_shard_id
    FOR SHARE;

    IF NOT FOUND
        OR owner_row.process_instance_id
            IS DISTINCT FROM expected_process_instance_id
        OR owner_row.expected_build_revision
            IS DISTINCT FROM expected_runtime_build_revision
        OR owner_row.lease_epoch::TEXT IS DISTINCT FROM
            (
                SELECT attestation.v2_route_admission
                    #>> '{gateway_owner_lease_id,lease_epoch}'
                FROM public.runtime_attestations AS attestation
                WHERE attestation.attestation_id = authority_row.attestation_id
            )
        OR owner_row.expires_at IS NULL
        OR owner_row.expires_at <= database_now
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI004',
            MESSAGE = 'runtime_interaction_receipt_recovery_gateway_invalid';
    END IF;

    new_claim_revision := head_row.claim_revision + 1;
    new_head_revision := head_row.head_revision + 1;
    new_claim_expiry := LEAST(
        database_now
            + requested_claim_lease_milliseconds * INTERVAL '1 millisecond',
        secret_row.expires_at
    );

    IF head_row.acknowledgement_state = 'attempting'
        AND proposed_observation_kind = 'acknowledged'
        AND head_row.acknowledgement_kind <> 'defer_ephemeral'
        AND head_row.action_plan_digest IS NULL
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_receipt_recovery_plan_missing';
    END IF;

    next_state := CASE
        WHEN head_row.acknowledgement_state = 'attempting'
            AND proposed_observation_kind = 'acknowledged'
            THEN CASE
                WHEN head_row.state = 'executing' THEN 'executing'
                WHEN head_row.action_plan_digest IS NOT NULL
                    THEN 'prepared'
                ELSE 'deferred'
            END
        ELSE head_row.state
    END;
    next_acknowledgement_state := CASE
        WHEN head_row.acknowledgement_state = 'attempting'
            AND proposed_observation_kind = 'acknowledged'
            THEN CASE
                WHEN head_row.acknowledgement_kind = 'defer_ephemeral'
                    THEN 'deferred'
                ELSE 'responded'
            END
        ELSE head_row.acknowledgement_state
    END;
    next_event_kind := CASE
        WHEN next_acknowledgement_state
            <> head_row.acknowledgement_state
            THEN 'claim_recovered_acknowledged'
        ELSE 'claim_recovered'
    END;
    next_outcome_code := CASE
        WHEN next_acknowledgement_state
            <> head_row.acknowledgement_state
            THEN 'claim_recovered_acknowledged'
        ELSE 'claim_recovered'
    END;

    UPDATE public.runtime_interaction_receipt_heads_v1 AS head
    SET state = next_state,
        acknowledgement_state = next_acknowledgement_state,
        head_revision = new_head_revision,
        claim_revision = new_claim_revision,
        claim_process_instance_id = expected_process_instance_id,
        claim_gateway_shard_id = expected_gateway_shard_id,
        claim_gateway_owner_lease_epoch = owner_row.lease_epoch,
        claim_gateway_owner_revision = owner_row.owner_revision,
        claim_serving_lease_epoch = authority_row.serving_lease_epoch,
        claim_serving_revision = authority_row.serving_revision,
        claim_acquired_at = database_now,
        claim_expires_at = new_claim_expiry,
        acknowledgement_result = CASE
            WHEN next_acknowledgement_state
                <> head_row.acknowledgement_state THEN 'succeeded'
            ELSE head.acknowledgement_result
        END,
        acknowledgement_result_digest = CASE
            WHEN next_acknowledgement_state
                <> head_row.acknowledgement_state
                THEN proposed_observation_digest
            ELSE head.acknowledgement_result_digest
        END,
        acknowledged_at = CASE
            WHEN next_acknowledgement_state
                <> head_row.acknowledgement_state THEN database_now
            ELSE head.acknowledged_at
        END,
        updated_at = database_now
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
        new_head_revision,
        next_event_kind,
        head_row.state,
        next_state,
        head_row.acknowledgement_state,
        next_acknowledgement_state,
        new_claim_revision,
        expected_process_instance_id,
        expected_gateway_shard_id,
        owner_row.lease_epoch,
        owner_row.owner_revision,
        authority_row.serving_lease_epoch,
        authority_row.serving_revision,
        next_outcome_code,
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.concat_ws(
                '|',
                'starring-runtime-interaction-receipt-event-v1',
                expected_application_id,
                expected_interaction_id,
                new_head_revision::TEXT,
                next_event_kind,
                head_row.state,
                next_state,
                head_row.acknowledgement_state,
                next_acknowledgement_state,
                new_claim_revision::TEXT,
                expected_process_instance_id,
                expected_gateway_shard_id,
                owner_row.lease_epoch::TEXT,
                owner_row.owner_revision::TEXT,
                authority_row.serving_lease_epoch::TEXT,
                authority_row.serving_revision::TEXT,
                next_outcome_code
            ),
            'UTF8'
        )),
        database_now
    );

    outcome_name := next_outcome_code;
    receipt_state := next_state;
    resulting_head_revision := new_head_revision;
    resulting_claim_revision := new_claim_revision;
    resulting_claim_expires_at := new_claim_expiry;
    resulting_gateway_owner_lease_epoch := owner_row.lease_epoch;
    resulting_gateway_owner_revision := owner_row.owner_revision;
    resulting_serving_lease_epoch := authority_row.serving_lease_epoch;
    resulting_serving_revision := authority_row.serving_revision;
    root_tenant_id := root_row.tenant_id;
    root_installation_id := root_row.installation_id;
    root_deployment_id := root_row.deployment_id;
    root_attestation_digest := root_row.attestation_digest;
    root_guild_id := root_row.guild_id;
    root_ruleset_key := root_row.ruleset_key;
    root_target_version := root_row.target_version;
    root_target_content_hash := root_row.target_content_hash;
    root_binding_revision := root_row.binding_revision;
    root_binding_fingerprint := root_row.binding_fingerprint;
    root_runtime_generation := root_row.runtime_generation;
    root_process_instance_id := root_row.origin_process_instance_id;
    root_serving_lease_epoch := root_row.origin_serving_lease_epoch;
    root_serving_revision := root_row.origin_serving_revision;
    root_gateway_shard_id := root_row.origin_gateway_shard_id;
    root_gateway_owner_lease_epoch :=
        root_row.origin_gateway_owner_lease_epoch;
    root_gateway_owner_revision := root_row.origin_gateway_owner_revision;
    root_route_controller_fencing_token :=
        root_row.route_controller_fencing_token;
    root_route_incarnation := root_row.route_incarnation;
    root_runtime_build_revision := root_row.runtime_build_revision;
    root_route_kind := root_row.route_kind;
    root_route_key := root_row.route_key;
    root_instance_id := root_row.instance_id;
    root_execution_ruleset_version := root_row.execution_ruleset_version;
    root_execution_ruleset_content_hash :=
        root_row.execution_ruleset_content_hash;
    root_instance_manifest_digest := root_row.instance_manifest_digest;
    root_request_digest := root_row.request_digest;
    token_encryption_suite := secret_row.encryption_suite;
    token_suite_version := secret_row.suite_version;
    token_key_id := secret_row.key_id;
    token_nonce := secret_row.nonce;
    token_ciphertext := secret_row.ciphertext;
    token_aad_digest := secret_row.aad_digest;
    token_issued_at := secret_row.issued_at;
    token_expires_at := secret_row.expires_at;
    observed_database_now := database_now;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_receipt_terminalize_expired_v1(
    expected_application_id TEXT,
    expected_interaction_id TEXT,
    expected_head_revision BIGINT,
    expected_claim_revision BIGINT,
    expected_process_instance_id TEXT,
    expected_runtime_build_revision TEXT,
    proposed_observation_digest BYTEA
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
    authority_row RECORD;
    root_row public.runtime_interaction_receipt_roots_v1%ROWTYPE;
    head_row public.runtime_interaction_receipt_heads_v1%ROWTYPE;
    database_now TIMESTAMPTZ;
    next_acknowledgement_state TEXT;
    next_terminal_outcome_code TEXT;
    authority_available BOOLEAN := FALSE;
    mutated_count BIGINT;
BEGIN
    IF expected_application_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_application_id) > 20
        OR expected_interaction_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_interaction_id) > 20
        OR expected_head_revision NOT BETWEEN 1 AND 9223372036854775806
        OR expected_claim_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_process_instance_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_runtime_build_revision !~ '^[A-Za-z0-9_.:/-]{1,128}$'
        OR pg_catalog.octet_length(proposed_observation_digest) <> 32
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_receipt_terminalize_expired_input_invalid';
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
    INTO root_row
    FROM public.runtime_interaction_receipt_roots_v1 AS root
    WHERE root.application_id = expected_application_id
        AND root.interaction_id = expected_interaction_id
    FOR KEY SHARE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_receipt_not_found';
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
            MESSAGE = 'runtime_interaction_receipt_head_missing';
    END IF;

    database_now := pg_catalog.clock_timestamp();

    IF head_row.state IN ('completed', 'failed', 'recovery_required') THEN
        outcome_name := 'terminal_receipt_unchanged';
        receipt_state := head_row.state;
        resulting_head_revision := head_row.head_revision;
        resulting_claim_revision := head_row.claim_revision;
        resulting_claim_expires_at := head_row.claim_expires_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF head_row.head_revision <> expected_head_revision
        OR head_row.claim_revision <> expected_claim_revision
    THEN
        outcome_name := 'revision_race';
        receipt_state := head_row.state;
        resulting_head_revision := head_row.head_revision;
        resulting_claim_revision := head_row.claim_revision;
        resulting_claim_expires_at := head_row.claim_expires_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF head_row.claim_expires_at > database_now THEN
        outcome_name := 'claim_renewed';
        receipt_state := head_row.state;
        resulting_head_revision := head_row.head_revision;
        resulting_claim_revision := head_row.claim_revision;
        resulting_claim_expires_at := head_row.claim_expires_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF head_row.state NOT IN (
        'claimed',
        'acknowledging',
        'deferred',
        'prepared',
        'executing'
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI002',
            MESSAGE = 'runtime_interaction_receipt_terminalize_expired_state_corrupt';
    END IF;

    IF root_row.runtime_build_revision
            IS DISTINCT FROM expected_runtime_build_revision
    THEN
        outcome_name := 'route_authority_stale';
        receipt_state := head_row.state;
        resulting_head_revision := head_row.head_revision;
        resulting_claim_revision := head_row.claim_revision;
        resulting_claim_expires_at := head_row.claim_expires_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    BEGIN
        SELECT observed.*
        INTO authority_row
        FROM public.starring_runtime_interaction_receipt_authority_observe_v1(
            root_row.application_id,
            root_row.tenant_id,
            root_row.installation_id,
            root_row.deployment_id,
            root_row.guild_id,
            root_row.ruleset_key,
            root_row.target_version,
            root_row.target_content_hash,
            root_row.binding_revision,
            root_row.binding_fingerprint,
            root_row.runtime_generation,
            root_row.route_controller_fencing_token,
            root_row.route_incarnation,
            expected_process_instance_id,
            root_row.origin_gateway_shard_id,
            expected_runtime_build_revision,
            root_row.route_kind,
            COALESCE(root_row.instance_id, '')
        ) AS observed;
        authority_available := FOUND;
    EXCEPTION
        WHEN SQLSTATE 'RI004' THEN
            authority_available := FALSE;
    END;

    database_now := pg_catalog.clock_timestamp();

    IF head_row.claim_expires_at > database_now THEN
        outcome_name := 'claim_renewed';
        receipt_state := head_row.state;
        resulting_head_revision := head_row.head_revision;
        resulting_claim_revision := head_row.claim_revision;
        resulting_claim_expires_at := head_row.claim_expires_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF NOT authority_available
        OR authority_row.attestation_id
            IS DISTINCT FROM root_row.attestation_id
        OR authority_row.attestation_digest
            IS DISTINCT FROM root_row.attestation_digest
        OR authority_row.route_controller_fencing_token
            IS DISTINCT FROM root_row.route_controller_fencing_token
        OR authority_row.route_incarnation
            IS DISTINCT FROM root_row.route_incarnation
        OR authority_row.runtime_build_revision
            IS DISTINCT FROM expected_runtime_build_revision
        OR authority_row.execution_ruleset_version
            IS DISTINCT FROM root_row.execution_ruleset_version
        OR authority_row.execution_ruleset_content_hash
            IS DISTINCT FROM root_row.execution_ruleset_content_hash
        OR authority_row.instance_manifest_digest
            IS DISTINCT FROM root_row.instance_manifest_digest
    THEN
        outcome_name := 'route_authority_stale';
        receipt_state := head_row.state;
        resulting_head_revision := head_row.head_revision;
        resulting_claim_revision := head_row.claim_revision;
        resulting_claim_expires_at := head_row.claim_expires_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    next_acknowledgement_state := CASE
        WHEN head_row.acknowledgement_state = 'attempting'
            THEN 'response_recovery_terminal'
        ELSE head_row.acknowledgement_state
    END;
    next_terminal_outcome_code := CASE
        WHEN head_row.state = 'claimed'
            THEN 'expired_pristine_claim_abandoned'
        ELSE 'expired_claim_recovery_required'
    END;

    UPDATE public.runtime_interaction_receipt_heads_v1 AS head
    SET state = 'recovery_required',
        acknowledgement_state = next_acknowledgement_state,
        head_revision = head.head_revision + 1,
        acknowledgement_result = CASE
            WHEN head_row.acknowledgement_state = 'attempting'
                THEN 'indeterminate'
            ELSE head.acknowledgement_result
        END,
        acknowledgement_result_digest = CASE
            WHEN head_row.acknowledgement_state = 'attempting'
                THEN proposed_observation_digest
            ELSE head.acknowledgement_result_digest
        END,
        acknowledged_at = CASE
            WHEN head_row.acknowledgement_state = 'attempting'
                THEN database_now
            ELSE head.acknowledged_at
        END,
        terminal_outcome_code = next_terminal_outcome_code,
        terminal_result_digest = proposed_observation_digest,
        terminal_at = database_now,
        updated_at = database_now
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
        AND head.head_revision = expected_head_revision
        AND head.claim_revision = expected_claim_revision
        AND head.claim_expires_at <= database_now
        AND head.state IN (
            'claimed',
            'acknowledging',
            'deferred',
            'prepared',
            'executing'
        );

    GET DIAGNOSTICS mutated_count = ROW_COUNT;

    IF mutated_count <> 1 THEN
        outcome_name := 'revision_race';
        receipt_state := head_row.state;
        resulting_head_revision := head_row.head_revision;
        resulting_claim_revision := head_row.claim_revision;
        resulting_claim_expires_at := head_row.claim_expires_at;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

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
        'recovery_required',
        head_row.state,
        'recovery_required',
        head_row.acknowledgement_state,
        next_acknowledgement_state,
        head_row.claim_revision,
        head_row.claim_process_instance_id,
        head_row.claim_gateway_shard_id,
        head_row.claim_gateway_owner_lease_epoch,
        head_row.claim_gateway_owner_revision,
        head_row.claim_serving_lease_epoch,
        head_row.claim_serving_revision,
        next_terminal_outcome_code,
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.concat_ws(
                '|',
                'starring-runtime-interaction-receipt-event-v1',
                expected_application_id,
                expected_interaction_id,
                (head_row.head_revision + 1)::TEXT,
                'recovery_required',
                head_row.state,
                'recovery_required',
                head_row.acknowledgement_state,
                next_acknowledgement_state,
                head_row.claim_revision::TEXT,
                head_row.claim_process_instance_id,
                head_row.claim_gateway_shard_id,
                head_row.claim_gateway_owner_lease_epoch::TEXT,
                head_row.claim_gateway_owner_revision::TEXT,
                head_row.claim_serving_lease_epoch::TEXT,
                head_row.claim_serving_revision::TEXT,
                next_terminal_outcome_code
            ),
            'UTF8'
        )),
        database_now
    );

    DELETE FROM public.runtime_interaction_receipt_token_secrets_v1
    WHERE application_id = expected_application_id
        AND interaction_id = expected_interaction_id;

    outcome_name := next_terminal_outcome_code;
    receipt_state := 'recovery_required';
    resulting_head_revision := head_row.head_revision + 1;
    resulting_claim_revision := head_row.claim_revision;
    resulting_claim_expires_at := head_row.claim_expires_at;
    observed_database_now := database_now;
    RETURN NEXT;
END;
$function$;

CREATE FUNCTION public.starring_runtime_interaction_receipt_token_expire_v1(
    expected_application_id TEXT,
    expected_interaction_id TEXT,
    expected_head_revision BIGINT,
    expected_claim_revision BIGINT,
    proposed_expiry_observation_digest BYTEA
)
RETURNS TABLE(
    outcome_name TEXT,
    receipt_state TEXT,
    resulting_head_revision BIGINT,
    resulting_claim_revision BIGINT,
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
    head_row public.runtime_interaction_receipt_heads_v1%ROWTYPE;
    secret_row public.runtime_interaction_receipt_token_secrets_v1%ROWTYPE;
    secret_found BOOLEAN;
    database_now TIMESTAMPTZ;
    next_acknowledgement_state TEXT;
    expiry_outcome_code TEXT;
BEGIN
    IF expected_application_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_application_id) > 20
        OR expected_interaction_id !~ '^[1-9][0-9]{0,19}$'
        OR pg_catalog.length(expected_interaction_id) > 20
        OR expected_head_revision NOT BETWEEN 1 AND 9223372036854775806
        OR expected_claim_revision NOT BETWEEN 1 AND 9223372036854775807
        OR pg_catalog.octet_length(proposed_expiry_observation_digest) <> 32
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI003',
            MESSAGE = 'runtime_interaction_receipt_token_expire_input_invalid';
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

    SELECT head.*
    INTO head_row
    FROM public.runtime_interaction_receipt_heads_v1 AS head
    WHERE head.application_id = expected_application_id
        AND head.interaction_id = expected_interaction_id
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_receipt_not_found';
    END IF;

    SELECT secret.*
    INTO secret_row
    FROM public.runtime_interaction_receipt_token_secrets_v1 AS secret
    WHERE secret.application_id = expected_application_id
        AND secret.interaction_id = expected_interaction_id
    FOR UPDATE;
    secret_found := FOUND;

    database_now := pg_catalog.clock_timestamp();

    IF head_row.head_revision <> expected_head_revision
        OR head_row.claim_revision <> expected_claim_revision
    THEN
        RAISE EXCEPTION USING
            ERRCODE = 'RI001',
            MESSAGE = 'runtime_interaction_receipt_token_expire_conflict';
    END IF;

    IF NOT secret_found
        AND head_row.state IN ('completed', 'failed', 'recovery_required')
    THEN
        outcome_name := 'token_absent';
        receipt_state := head_row.state;
        resulting_head_revision := head_row.head_revision;
        resulting_claim_revision := head_row.claim_revision;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF secret_found AND secret_row.expires_at > database_now THEN
        outcome_name := 'token_not_expired';
        receipt_state := head_row.state;
        resulting_head_revision := head_row.head_revision;
        resulting_claim_revision := head_row.claim_revision;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    IF head_row.state IN ('completed', 'failed', 'recovery_required') THEN
        DELETE FROM public.runtime_interaction_receipt_token_secrets_v1
        WHERE application_id = expected_application_id
            AND interaction_id = expected_interaction_id;

        outcome_name := 'terminal_token_deleted';
        receipt_state := head_row.state;
        resulting_head_revision := head_row.head_revision;
        resulting_claim_revision := head_row.claim_revision;
        observed_database_now := database_now;
        RETURN NEXT;
        RETURN;
    END IF;

    expiry_outcome_code := CASE
        WHEN secret_found THEN 'interaction_token_expired'
        ELSE 'interaction_token_unavailable'
    END;

    next_acknowledgement_state := CASE
        WHEN head_row.acknowledgement_state = 'attempting'
            THEN 'response_recovery_terminal'
        ELSE head_row.acknowledgement_state
    END;

    UPDATE public.runtime_interaction_receipt_heads_v1 AS head
    SET state = 'recovery_required',
        acknowledgement_state = next_acknowledgement_state,
        head_revision = head.head_revision + 1,
        acknowledgement_result = CASE
            WHEN head_row.acknowledgement_state = 'attempting'
                THEN 'indeterminate'
            ELSE head.acknowledgement_result
        END,
        acknowledgement_result_digest = CASE
            WHEN head_row.acknowledgement_state = 'attempting'
                THEN proposed_expiry_observation_digest
            ELSE head.acknowledgement_result_digest
        END,
        acknowledged_at = CASE
            WHEN head_row.acknowledgement_state = 'attempting'
                THEN database_now
            ELSE head.acknowledged_at
        END,
        terminal_outcome_code = expiry_outcome_code,
        terminal_result_digest = proposed_expiry_observation_digest,
        terminal_at = database_now,
        updated_at = database_now
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
        'interaction_token_expired',
        head_row.state,
        'recovery_required',
        head_row.acknowledgement_state,
        next_acknowledgement_state,
        head_row.claim_revision,
        head_row.claim_process_instance_id,
        head_row.claim_gateway_shard_id,
        head_row.claim_gateway_owner_lease_epoch,
        head_row.claim_gateway_owner_revision,
        head_row.claim_serving_lease_epoch,
        head_row.claim_serving_revision,
        expiry_outcome_code,
        pg_catalog.sha256(pg_catalog.convert_to(
            pg_catalog.concat_ws(
                '|',
                'starring-runtime-interaction-receipt-event-v1',
                expected_application_id,
                expected_interaction_id,
                (head_row.head_revision + 1)::TEXT,
                'interaction_token_expired',
                head_row.state,
                'recovery_required',
                head_row.acknowledgement_state,
                next_acknowledgement_state,
                head_row.claim_revision::TEXT,
                head_row.claim_process_instance_id,
                head_row.claim_gateway_shard_id,
                head_row.claim_gateway_owner_lease_epoch::TEXT,
                head_row.claim_gateway_owner_revision::TEXT,
                head_row.claim_serving_lease_epoch::TEXT,
                head_row.claim_serving_revision::TEXT,
                expiry_outcome_code
            ),
            'UTF8'
        )),
        database_now
    );

    DELETE FROM public.runtime_interaction_receipt_token_secrets_v1
    WHERE application_id = expected_application_id
        AND interaction_id = expected_interaction_id;

    outcome_name := expiry_outcome_code;
    receipt_state := 'recovery_required';
    resulting_head_revision := head_row.head_revision + 1;
    resulting_claim_revision := head_row.claim_revision;
    observed_database_now := database_now;
    RETURN NEXT;
END;
$function$;

CREATE OR REPLACE FUNCTION public.starring_runtime_interaction_receipt_schema_manifest_v1()
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
            AND relation.relname LIKE 'runtime_interaction_receipt_%'
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
            AND relation.relname LIKE 'runtime_interaction_receipt_%'
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
            AND relation.relname LIKE 'runtime_interaction_receipt_%'
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
            AND table_row.relname LIKE 'runtime_interaction_receipt_%'
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
                    'starring_runtime_interaction_receipt_%'
                OR function_row.proname LIKE
                    'guard_runtime_interaction_receipt_%'
            )
            AND function_row.oid <> pg_catalog.to_regprocedure(
                'public.starring_runtime_interaction_receipt_schema_manifest_v1()'
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
            AND relation.relname LIKE 'runtime_interaction_receipt_%'
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

    RETURN observed_count = 156
        AND observed_digest =
            'a2ea200c577ae33a9289803fc380f86cf886a7f4e4f5cc7f7fbc0b66f5ef128a';
END;
$function$;

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
            AND relation.relname LIKE 'runtime_interaction_receipt_%'
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
                RAISE EXCEPTION 'runtime interaction receipt grantee is unavailable'
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
                    'starring_runtime_interaction_receipt_%'
                OR function_row.proname LIKE
                    'guard_runtime_interaction_receipt_%'
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
                RAISE EXCEPTION 'runtime interaction receipt grantee is unavailable'
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
                AND relation.relname LIKE 'runtime_interaction_receipt_%'
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
                        'starring_runtime_interaction_receipt_%'
                    OR function_row.proname LIKE
                        'guard_runtime_interaction_receipt_%'
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
        ) <> 17
        OR (
            SELECT pg_catalog.count(*)
            FROM pg_catalog.pg_trigger AS trigger_row
            INNER JOIN pg_catalog.pg_class AS relation
                ON relation.oid = trigger_row.tgrelid
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = relation.relnamespace
            WHERE namespace.nspname = 'public'
                AND relation.relname LIKE 'runtime_interaction_receipt_%'
                AND NOT trigger_row.tgisinternal
                AND trigger_row.tgenabled = 'O'
                AND trigger_row.tgnargs = 0
                AND pg_catalog.octet_length(trigger_row.tgargs) = 0
                AND trigger_row.tgconstraint = 0
                AND NOT trigger_row.tgdeferrable
                AND NOT trigger_row.tginitdeferred
        ) <> 8
        OR NOT public.starring_runtime_interaction_receipt_schema_manifest_v1()
        OR NOT public.starring_runtime_interaction_schema_manifest_v1()
        OR readiness_definition NOT LIKE
            '%starring_runtime_interaction_receipt_authority_observe_v1%'
        OR readiness_definition NOT LIKE
            '%starring_runtime_interaction_receipt_claim_v1%'
        OR readiness_definition NOT LIKE
            '%starring_runtime_interaction_receipt_recover_v1%'
        OR readiness_definition NOT LIKE
            '%starring_runtime_interaction_receipt_terminalize_expired_v1%'
    THEN
        RAISE EXCEPTION 'runtime interaction receipt migration postflight failed'
            USING ERRCODE = '55000';
    END IF;
END;
$final_postflight$;

RESET search_path;
RESET statement_timeout;
RESET lock_timeout;
