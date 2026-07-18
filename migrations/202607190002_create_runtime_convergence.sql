CREATE TABLE public.runtime_deployments (
    deployment_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    installation_id TEXT NOT NULL,
    promotion_id TEXT NOT NULL,
    activation_request_id TEXT NOT NULL,
    installation_authority_revision BIGINT NOT NULL,
    guild_id TEXT NOT NULL,
    ruleset_key TEXT NOT NULL,
    target_version BIGINT NOT NULL,
    target_content_hash TEXT NOT NULL,
    binding_revision BIGINT NOT NULL,
    binding_fingerprint TEXT NOT NULL,
    desired_target_digest TEXT NOT NULL,
    runtime_generation BIGINT NOT NULL,
    previous_runtime JSONB,
    requested_at TIMESTAMPTZ NOT NULL,
    snapshot_format_version SMALLINT NOT NULL,
    snapshot JSONB NOT NULL,
    revision BIGINT NOT NULL,
    phase TEXT NOT NULL,
    controller_id TEXT,
    controller_fencing_token BIGINT,
    controller_acquired_at TIMESTAMPTZ,
    controller_lease_expires_at TIMESTAMPTZ,
    last_fencing_token BIGINT,
    next_retry_at TIMESTAMPTZ,
    last_stable_error_code TEXT,
    live_attestation_id TEXT,
    live_at TIMESTAMPTZ,
    blocked_at TIMESTAMPTZ,
    superseded_at TIMESTAMPTZ,
    cancelled_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT runtime_deployments_installation_fk FOREIGN KEY (
        tenant_id,
        installation_id
    ) REFERENCES public.automation_installations (tenant_id, installation_id)
        ON DELETE RESTRICT,
    CONSTRAINT runtime_deployments_authority_fk FOREIGN KEY (
        tenant_id,
        installation_id,
        installation_authority_revision
    ) REFERENCES public.automation_installation_authority_versions (
        tenant_id,
        installation_id,
        revision
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_deployments_promotion_fk FOREIGN KEY (promotion_id)
        REFERENCES public.authoring_promotions (id)
        ON DELETE RESTRICT,
    CONSTRAINT runtime_deployments_activation_fk FOREIGN KEY (activation_request_id)
        REFERENCES public.activation_requests (id)
        ON DELETE RESTRICT,
    CONSTRAINT runtime_deployments_target_fk FOREIGN KEY (
        guild_id,
        ruleset_key,
        target_version
    ) REFERENCES public.automation_ruleset_versions (
        guild_id,
        ruleset_key,
        version
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_deployments_scope_identity_unique UNIQUE (
        tenant_id,
        installation_id,
        deployment_id
    ),
    CONSTRAINT runtime_deployments_activation_unique UNIQUE (activation_request_id),
    CONSTRAINT runtime_deployments_desired_target_unique UNIQUE (
        tenant_id,
        installation_id,
        desired_target_digest
    ),
    CONSTRAINT runtime_deployments_lane_generation_unique UNIQUE (
        guild_id,
        ruleset_key,
        runtime_generation
    ),
    CONSTRAINT runtime_deployments_id_format CHECK (
        deployment_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND activation_request_id ~ '^[A-Za-z0-9_-]{1,64}$'
    ),
    CONSTRAINT runtime_deployments_promotion_id_format CHECK (
        promotion_id ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT runtime_deployments_target_valid CHECK (
        CASE
            WHEN guild_id ~ '^[1-9][0-9]{0,19}$'
                THEN guild_id::NUMERIC <= 18446744073709551615
            ELSE FALSE
        END
        AND ruleset_key ~ '^[A-Za-z0-9_-]{1,64}$'
        AND target_version BETWEEN 1 AND 4294967295
        AND target_content_hash ~ '^[0-9a-f]{64}$'
        AND binding_revision BETWEEN 1 AND 9223372036854775807
        AND binding_fingerprint ~ '^[0-9a-f]{64}$'
        AND desired_target_digest ~ '^[0-9a-f]{64}$'
        AND installation_authority_revision BETWEEN 1 AND 9223372036854775807
        AND runtime_generation BETWEEN 1 AND 9223372036854775807
    ),
    CONSTRAINT runtime_deployments_previous_runtime_valid CHECK (
        previous_runtime IS NULL
        OR (
            jsonb_typeof(previous_runtime) = 'object'
            AND octet_length(previous_runtime::TEXT) <= 16384
        )
    ),
    CONSTRAINT runtime_deployments_snapshot_valid CHECK (
        snapshot_format_version = 1
        AND jsonb_typeof(snapshot) = 'object'
        AND octet_length(snapshot::TEXT) BETWEEN 32 AND 262144
    ),
    CONSTRAINT runtime_deployments_revision_valid CHECK (
        revision BETWEEN 1 AND 9223372036854775807
        AND (last_fencing_token IS NULL OR last_fencing_token BETWEEN 1 AND 9223372036854775807)
    ),
    CONSTRAINT runtime_deployments_phase_valid CHECK (
        phase IN (
            'requested',
            'preflight_ready',
            'drain_requested',
            'drained',
            'activation_applying',
            'runtime_pending',
            'reconciling_panels',
            'awaiting_gateway_ready',
            'live',
            'superseded',
            'cancelled'
        )
    ),
    CONSTRAINT runtime_deployments_controller_lease_valid CHECK (
        (
            controller_id IS NULL
            AND controller_fencing_token IS NULL
            AND controller_acquired_at IS NULL
            AND controller_lease_expires_at IS NULL
        )
        OR (
            controller_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
            AND controller_fencing_token BETWEEN 1 AND 9223372036854775807
            AND controller_acquired_at >= requested_at
            AND controller_lease_expires_at > controller_acquired_at
            AND last_fencing_token = controller_fencing_token
            AND phase NOT IN ('live','superseded','cancelled')
        )
    ),
    CONSTRAINT runtime_deployments_retry_valid CHECK (
        next_retry_at IS NULL OR next_retry_at >= requested_at
    ),
    CONSTRAINT runtime_deployments_error_valid CHECK (
        last_stable_error_code IS NULL
        OR last_stable_error_code ~ '^[a-z0-9_]{1,64}$'
    ),
    CONSTRAINT runtime_deployments_terminal_projection_valid CHECK (
        (
            (phase = 'live' AND live_attestation_id IS NOT NULL AND live_at IS NOT NULL)
            OR (phase <> 'live' AND live_attestation_id IS NULL AND live_at IS NULL)
        )
        AND (phase = 'superseded') = (superseded_at IS NOT NULL)
        AND (phase = 'cancelled') = (cancelled_at IS NOT NULL)
        AND (phase <> 'live' OR blocked_at IS NULL)
        AND (phase NOT IN ('live','superseded','cancelled') OR controller_id IS NULL)
    ),
    CONSTRAINT runtime_deployments_timestamps_valid CHECK (
        requested_at >= created_at
        AND created_at <= updated_at
        AND (live_at IS NULL OR live_at >= requested_at)
        AND (blocked_at IS NULL OR blocked_at >= requested_at)
        AND (superseded_at IS NULL OR superseded_at >= requested_at)
        AND (cancelled_at IS NULL OR cancelled_at >= requested_at)
    )
);

CREATE UNIQUE INDEX runtime_deployments_one_unresolved_per_lane
ON public.runtime_deployments (guild_id, ruleset_key)
WHERE phase NOT IN ('live','superseded','cancelled');

CREATE INDEX runtime_deployments_claimable_index
ON public.runtime_deployments (
    COALESCE(next_retry_at, requested_at),
    requested_at,
    deployment_id
)
WHERE phase NOT IN ('live','superseded','cancelled')
    AND blocked_at IS NULL;

CREATE INDEX runtime_deployments_scope_status_index
ON public.runtime_deployments (
    tenant_id,
    installation_id,
    updated_at DESC,
    deployment_id
);

CREATE TABLE public.runtime_attestations (
    attestation_id TEXT PRIMARY KEY,
    attestation_digest TEXT NOT NULL UNIQUE,
    deployment_id TEXT NOT NULL,
    deployment_revision BIGINT NOT NULL,
    tenant_id TEXT NOT NULL,
    installation_id TEXT NOT NULL,
    promotion_id TEXT NOT NULL,
    activation_request_id TEXT NOT NULL,
    guild_id TEXT NOT NULL,
    ruleset_key TEXT NOT NULL,
    target_version BIGINT NOT NULL,
    target_content_hash TEXT NOT NULL,
    binding_revision BIGINT NOT NULL,
    binding_fingerprint TEXT NOT NULL,
    runtime_generation BIGINT NOT NULL,
    controller_fencing_token BIGINT NOT NULL,
    process_instance_id TEXT NOT NULL,
    runtime_build_revision TEXT NOT NULL,
    panel_certificate_id TEXT NOT NULL,
    panel_report_digest TEXT NOT NULL,
    gateway_shard_id TEXT NOT NULL,
    gateway_ready_kind TEXT NOT NULL,
    gateway_ready_at TIMESTAMPTZ NOT NULL,
    certified_at TIMESTAMPTZ NOT NULL,
    record_format_version SMALLINT NOT NULL,
    record JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT runtime_attestations_deployment_fk FOREIGN KEY (
        tenant_id,
        installation_id,
        deployment_id
    ) REFERENCES public.runtime_deployments (
        tenant_id,
        installation_id,
        deployment_id
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_attestations_target_fk FOREIGN KEY (
        guild_id,
        ruleset_key,
        target_version
    ) REFERENCES public.automation_ruleset_versions (
        guild_id,
        ruleset_key,
        version
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_attestations_scope_identity_unique UNIQUE (
        tenant_id,
        installation_id,
        deployment_id,
        attestation_id
    ),
    CONSTRAINT runtime_attestations_deployment_revision_unique UNIQUE (
        deployment_id,
        deployment_revision
    ),
    CONSTRAINT runtime_attestations_id_format CHECK (
        attestation_id ~ '^[0-9a-f]{64}$'
        AND attestation_digest = attestation_id
        AND deployment_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND promotion_id ~ '^[0-9a-f]{64}$'
        AND activation_request_id ~ '^[A-Za-z0-9_-]{1,64}$'
    ),
    CONSTRAINT runtime_attestations_target_valid CHECK (
        CASE
            WHEN guild_id ~ '^[1-9][0-9]{0,19}$'
                THEN guild_id::NUMERIC <= 18446744073709551615
            ELSE FALSE
        END
        AND ruleset_key ~ '^[A-Za-z0-9_-]{1,64}$'
        AND target_version BETWEEN 1 AND 4294967295
        AND target_content_hash ~ '^[0-9a-f]{64}$'
        AND binding_revision BETWEEN 1 AND 9223372036854775807
        AND binding_fingerprint ~ '^[0-9a-f]{64}$'
        AND runtime_generation BETWEEN 1 AND 9223372036854775807
        AND deployment_revision BETWEEN 1 AND 9223372036854775807
        AND controller_fencing_token BETWEEN 1 AND 9223372036854775807
    ),
    CONSTRAINT runtime_attestations_runtime_identity_valid CHECK (
        process_instance_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND runtime_build_revision ~ '^[A-Za-z0-9_.:/-]{1,128}$'
        AND panel_certificate_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND panel_report_digest ~ '^[0-9a-f]{64}$'
        AND gateway_shard_id ~ '^[A-Za-z0-9_.:/-]{1,128}$'
        AND gateway_ready_kind IN ('discord_ready','discord_resumed')
    ),
    CONSTRAINT runtime_attestations_record_valid CHECK (
        record_format_version = 1
        AND jsonb_typeof(record) = 'object'
        AND octet_length(record::TEXT) BETWEEN 32 AND 262144
    ),
    CONSTRAINT runtime_attestations_timestamps_valid CHECK (
        gateway_ready_at <= certified_at
        AND certified_at = created_at
    )
);

ALTER TABLE public.runtime_deployments
    ADD CONSTRAINT runtime_deployments_live_attestation_fk FOREIGN KEY (
        tenant_id,
        installation_id,
        deployment_id,
        live_attestation_id
    ) REFERENCES public.runtime_attestations (
        tenant_id,
        installation_id,
        deployment_id,
        attestation_id
    ) ON DELETE RESTRICT
    DEFERRABLE INITIALLY DEFERRED;

CREATE INDEX runtime_attestations_scope_time_index
ON public.runtime_attestations (
    tenant_id,
    installation_id,
    created_at DESC,
    attestation_id
);

CREATE TABLE public.runtime_serving_leases (
    guild_id TEXT NOT NULL,
    ruleset_key TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    installation_id TEXT NOT NULL,
    deployment_id TEXT NOT NULL,
    attestation_id TEXT NOT NULL,
    process_instance_id TEXT NOT NULL,
    runtime_generation BIGINT NOT NULL,
    target_version BIGINT NOT NULL,
    target_content_hash TEXT NOT NULL,
    binding_revision BIGINT NOT NULL,
    binding_fingerprint TEXT NOT NULL,
    lease_epoch BIGINT NOT NULL,
    revision BIGINT NOT NULL,
    connected BOOLEAN NOT NULL,
    serving BOOLEAN NOT NULL,
    acquired_at TIMESTAMPTZ NOT NULL,
    last_heartbeat_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (guild_id, ruleset_key),
    CONSTRAINT runtime_serving_leases_attestation_fk FOREIGN KEY (
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
    CONSTRAINT runtime_serving_leases_target_fk FOREIGN KEY (
        guild_id,
        ruleset_key,
        target_version
    ) REFERENCES public.automation_ruleset_versions (
        guild_id,
        ruleset_key,
        version
    ) ON DELETE RESTRICT,
    CONSTRAINT runtime_serving_leases_scope_identity_unique UNIQUE (
        tenant_id,
        installation_id,
        deployment_id,
        attestation_id,
        lease_epoch
    ),
    CONSTRAINT runtime_serving_leases_identity_valid CHECK (
        CASE
            WHEN guild_id ~ '^[1-9][0-9]{0,19}$'
                THEN guild_id::NUMERIC <= 18446744073709551615
            ELSE FALSE
        END
        AND ruleset_key ~ '^[A-Za-z0-9_-]{1,64}$'
        AND tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND deployment_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND attestation_id ~ '^[0-9a-f]{64}$'
        AND process_instance_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT runtime_serving_leases_target_valid CHECK (
        runtime_generation BETWEEN 1 AND 9223372036854775807
        AND target_version BETWEEN 1 AND 4294967295
        AND target_content_hash ~ '^[0-9a-f]{64}$'
        AND binding_revision BETWEEN 1 AND 9223372036854775807
        AND binding_fingerprint ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT runtime_serving_leases_revision_valid CHECK (
        lease_epoch BETWEEN 1 AND 9223372036854775807
        AND revision BETWEEN 1 AND 9223372036854775807
    ),
    CONSTRAINT runtime_serving_leases_state_valid CHECK (
        serving = connected
    ),
    CONSTRAINT runtime_serving_leases_timestamps_valid CHECK (
        acquired_at <= last_heartbeat_at
        AND last_heartbeat_at <= expires_at
    )
);

CREATE INDEX runtime_serving_leases_scope_status_index
ON public.runtime_serving_leases (
    tenant_id,
    installation_id,
    expires_at,
    deployment_id
);

CREATE FUNCTION public.starring_runtime_lock_current_authority(
    expected_activation_request_id TEXT,
    expected_promotion_id TEXT,
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_installation_authority_revision BIGINT,
    expected_guild_id TEXT,
    expected_ruleset_key TEXT,
    expected_target_version BIGINT,
    expected_target_content_hash TEXT,
    expected_binding_revision BIGINT,
    expected_binding_fingerprint TEXT
)
RETURNS TEXT
LANGUAGE plpgsql
VOLATILE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    activation_row public.activation_requests%ROWTYPE;
    promotion_row public.authoring_promotions%ROWTYPE;
    tenant_row public.product_tenants%ROWTYPE;
    installation_row public.automation_installations%ROWTYPE;
    authority_row public.automation_installation_authority_versions%ROWTYPE;
    active_version BIGINT;
    persisted_content_hash TEXT;
BEGIN
    IF expected_activation_request_id !~ '^[A-Za-z0-9_-]{1,64}$'
        OR expected_promotion_id !~ '^[0-9a-f]{64}$'
        OR expected_tenant_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
        OR expected_installation_authority_revision NOT BETWEEN 1 AND 9223372036854775807
        OR NOT (CASE
            WHEN expected_guild_id ~ '^[1-9][0-9]{0,19}$'
                THEN expected_guild_id::NUMERIC <= 18446744073709551615
            ELSE FALSE
        END)
        OR expected_ruleset_key !~ '^[A-Za-z0-9_-]{1,64}$'
        OR expected_target_version NOT BETWEEN 1 AND 4294967295
        OR expected_target_content_hash !~ '^[0-9a-f]{64}$'
        OR expected_binding_revision NOT BETWEEN 1 AND 9223372036854775807
        OR expected_binding_fingerprint !~ '^[0-9a-f]{64}$'
    THEN
        RETURN 'scope_mismatch';
    END IF;

    SELECT *
    INTO activation_row
    FROM public.activation_requests
    WHERE id = expected_activation_request_id
    FOR SHARE;
    IF NOT FOUND
        OR activation_row.authority_kind <> 'product_authoring'
        OR activation_row.link_state_name <> 'linked'
        OR activation_row.state <> 'applied'
        OR activation_row.promotion_id IS DISTINCT FROM expected_promotion_id
        OR activation_row.guild_id IS DISTINCT FROM expected_guild_id
        OR activation_row.ruleset_key IS DISTINCT FROM expected_ruleset_key
        OR activation_row.target_version IS DISTINCT FROM expected_target_version
        OR activation_row.target_content_hash IS DISTINCT FROM expected_target_content_hash
    THEN
        RETURN 'scope_mismatch';
    END IF;

    SELECT *
    INTO promotion_row
    FROM public.authoring_promotions
    WHERE id = expected_promotion_id
    FOR SHARE;
    IF NOT FOUND
        OR promotion_row.stage <> 'activation_pending'
        OR promotion_row.tenant_id IS DISTINCT FROM expected_tenant_id
        OR promotion_row.record #>> '{intent,authority,tenant_id}' IS DISTINCT FROM expected_tenant_id
        OR promotion_row.record #>> '{intent,authority,installation_id}' IS DISTINCT FROM expected_installation_id
        OR promotion_row.record #>> '{intent,authority,guild_id}' IS DISTINCT FROM expected_guild_id
        OR promotion_row.record #>> '{intent,authority,ruleset_key}' IS DISTINCT FROM expected_ruleset_key
        OR (promotion_row.record #>> '{intent,authority,binding_revision}')::BIGINT
            IS DISTINCT FROM expected_binding_revision
        OR promotion_row.record #>> '{intent,evidence,context_fingerprint}'
            IS DISTINCT FROM expected_binding_fingerprint
        OR promotion_row.record #>> '{stage,activation,request_id}'
            IS DISTINCT FROM expected_activation_request_id
        OR promotion_row.record #>> '{stage,activation,target,guild_id}'
            IS DISTINCT FROM expected_guild_id
        OR promotion_row.record #>> '{stage,activation,target,ruleset_key}'
            IS DISTINCT FROM expected_ruleset_key
        OR (promotion_row.record #>> '{stage,activation,target,version}')::BIGINT
            IS DISTINCT FROM expected_target_version
        OR promotion_row.record #>> '{stage,activation,target,content_hash}'
            IS DISTINCT FROM expected_target_content_hash
    THEN
        RETURN 'scope_mismatch';
    END IF;

    SELECT *
    INTO tenant_row
    FROM public.product_tenants
    WHERE tenant_id = expected_tenant_id
    FOR SHARE;
    IF NOT FOUND OR tenant_row.lifecycle_state <> 'active' THEN
        RETURN 'lifecycle_inactive';
    END IF;

    SELECT *
    INTO installation_row
    FROM public.automation_installations
    WHERE tenant_id = expected_tenant_id
        AND installation_id = expected_installation_id
    FOR SHARE;
    IF NOT FOUND
        OR installation_row.discord_guild_id IS DISTINCT FROM expected_guild_id
        OR installation_row.ruleset_key IS DISTINCT FROM expected_ruleset_key
    THEN
        RETURN 'scope_mismatch';
    END IF;
    IF installation_row.lifecycle_state <> 'active' THEN
        RETURN 'lifecycle_inactive';
    END IF;
    IF installation_row.current_authority_revision
        IS DISTINCT FROM expected_installation_authority_revision
    THEN
        RETURN 'binding_mismatch';
    END IF;

    SELECT *
    INTO authority_row
    FROM public.automation_installation_authority_versions
    WHERE tenant_id = expected_tenant_id
        AND installation_id = expected_installation_id
        AND revision = expected_installation_authority_revision;
    IF NOT FOUND
        OR authority_row.binding_revision IS DISTINCT FROM expected_binding_revision
        OR authority_row.binding_fingerprint IS DISTINCT FROM expected_binding_fingerprint
    THEN
        RETURN 'binding_mismatch';
    END IF;

    SELECT active.active_version
    INTO active_version
    FROM public.automation_ruleset_activations active
    WHERE active.guild_id = expected_guild_id
        AND active.ruleset_key = expected_ruleset_key
    FOR SHARE;
    IF NOT FOUND OR active_version IS DISTINCT FROM expected_target_version THEN
        RETURN 'active_mismatch';
    END IF;

    SELECT content_hash
    INTO persisted_content_hash
    FROM public.automation_ruleset_versions
    WHERE guild_id = expected_guild_id
        AND ruleset_key = expected_ruleset_key
        AND version = expected_target_version;
    IF NOT FOUND OR persisted_content_hash IS DISTINCT FROM expected_target_content_hash THEN
        RETURN 'active_mismatch';
    END IF;

    RETURN 'exact';
END;
$function$;

REVOKE ALL ON FUNCTION public.starring_runtime_lock_current_authority(
    TEXT,
    TEXT,
    TEXT,
    TEXT,
    BIGINT,
    TEXT,
    TEXT,
    BIGINT,
    TEXT,
    BIGINT,
    TEXT
) FROM PUBLIC;

CREATE FUNCTION public.starring_runtime_mutation_clock()
RETURNS TIMESTAMPTZ
LANGUAGE plpgsql
VOLATILE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    mutation_clock TIMESTAMPTZ;
BEGIN
    mutation_clock := clock_timestamp();
    PERFORM set_config('starring.runtime_mutation_clock', mutation_clock::TEXT, TRUE);
    RETURN mutation_clock;
END;
$function$;

REVOKE ALL ON FUNCTION public.starring_runtime_mutation_clock() FROM PUBLIC;

CREATE FUNCTION public.starring_runtime_current_mutation_clock()
RETURNS TIMESTAMPTZ
LANGUAGE plpgsql
VOLATILE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    configured_clock TEXT;
    mutation_clock TIMESTAMPTZ;
    wall_clock TIMESTAMPTZ;
    maximum_age INTERVAL;
BEGIN
    configured_clock := current_setting('starring.runtime_mutation_clock', TRUE);
    IF configured_clock IS NULL OR configured_clock = '' THEN
        RAISE EXCEPTION 'runtime mutation clock is required'
            USING ERRCODE = '55000';
    END IF;
    mutation_clock := configured_clock::TIMESTAMPTZ;
    wall_clock := clock_timestamp();
    maximum_age := LEAST(
        current_setting('statement_timeout')::INTERVAL,
        INTERVAL '30 seconds'
    );
    IF maximum_age <= INTERVAL '0 seconds'
        OR mutation_clock > wall_clock
        OR mutation_clock < wall_clock - maximum_age
    THEN
        RAISE EXCEPTION 'runtime mutation clock is stale'
            USING ERRCODE = '55000';
    END IF;
    RETURN mutation_clock;
END;
$function$;

REVOKE ALL ON FUNCTION public.starring_runtime_current_mutation_clock() FROM PUBLIC;

CREATE FUNCTION public.validate_runtime_deployment_projection()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    authority_outcome TEXT;
    snapshot_phase TEXT;
    mutation_clock TIMESTAMPTZ;
    attestation_row public.runtime_attestations%ROWTYPE;
    serving_row public.runtime_serving_leases%ROWTYPE;
    stable_failure_message TEXT;
BEGIN
    authority_outcome := public.starring_runtime_lock_current_authority(
        NEW.activation_request_id,
        NEW.promotion_id,
        NEW.tenant_id,
        NEW.installation_id,
        NEW.installation_authority_revision,
        NEW.guild_id,
        NEW.ruleset_key,
        NEW.target_version,
        NEW.target_content_hash,
        NEW.binding_revision,
        NEW.binding_fingerprint
    );
    IF authority_outcome <> 'exact' THEN
        RAISE EXCEPTION 'runtime deployment product authority is not current'
            USING ERRCODE = '23514';
    END IF;
    mutation_clock := public.starring_runtime_current_mutation_clock();

    snapshot_phase := NEW.snapshot -> 'phase' ->> 'phase';
    IF NEW.snapshot #>> '{identity,deployment_id}' IS DISTINCT FROM NEW.deployment_id
        OR NEW.snapshot #>> '{identity,tenant_id}' IS DISTINCT FROM NEW.tenant_id
        OR NEW.snapshot #>> '{identity,installation_id}' IS DISTINCT FROM NEW.installation_id
        OR NEW.snapshot #>> '{identity,promotion_id}' IS DISTINCT FROM NEW.promotion_id
        OR NEW.snapshot #>> '{identity,activation_request_id}' IS DISTINCT FROM NEW.activation_request_id
        OR NEW.snapshot #>> '{target,guild_id}' IS DISTINCT FROM NEW.guild_id
        OR NEW.snapshot #>> '{target,ruleset_key}' IS DISTINCT FROM NEW.ruleset_key
        OR NEW.snapshot #>> '{target,version}' IS DISTINCT FROM NEW.target_version::TEXT
        OR NEW.snapshot #>> '{target,content_hash}' IS DISTINCT FROM NEW.target_content_hash
        OR NEW.snapshot #>> '{target,binding_revision}' IS DISTINCT FROM NEW.binding_revision::TEXT
        OR NEW.snapshot #>> '{target,binding_fingerprint}' IS DISTINCT FROM NEW.binding_fingerprint
        OR NEW.snapshot ->> 'runtime_generation' IS DISTINCT FROM NEW.runtime_generation::TEXT
        OR NEW.snapshot ->> 'revision' IS DISTINCT FROM NEW.revision::TEXT
        OR snapshot_phase IS DISTINCT FROM NEW.phase
        OR NEW.snapshot -> 'previous_runtime' IS DISTINCT FROM COALESCE(NEW.previous_runtime, 'null'::JSONB)
        OR NEW.snapshot #>> '{last_runtime_failure,failure,code}'
            IS DISTINCT FROM NEW.last_stable_error_code
        OR (
            TG_OP = 'INSERT'
            AND (
                NEW.requested_at IS DISTINCT FROM mutation_clock
                OR NEW.created_at IS DISTINCT FROM mutation_clock
                OR NEW.updated_at IS DISTINCT FROM mutation_clock
            )
        )
    THEN
        RAISE EXCEPTION 'runtime deployment shadow columns differ from its core snapshot'
            USING ERRCODE = '23514';
    END IF;

    IF NEW.snapshot -> 'controller_lease' = 'null'::JSONB THEN
        IF NEW.controller_id IS NOT NULL
            OR NEW.controller_fencing_token IS NOT NULL
            OR NEW.controller_acquired_at IS NOT NULL
            OR NEW.controller_lease_expires_at IS NOT NULL
        THEN
            RAISE EXCEPTION 'runtime deployment controller shadow differs from its core snapshot'
                USING ERRCODE = '23514';
        END IF;
    ELSIF NEW.snapshot #>> '{controller_lease,controller_id}' IS DISTINCT FROM NEW.controller_id
        OR NEW.snapshot #>> '{controller_lease,fencing_token}' IS DISTINCT FROM NEW.controller_fencing_token::TEXT
        OR (NEW.snapshot #>> '{controller_lease,acquired_at}')::TIMESTAMPTZ
            IS DISTINCT FROM NEW.controller_acquired_at
        OR (NEW.snapshot #>> '{controller_lease,expires_at}')::TIMESTAMPTZ
            IS DISTINCT FROM NEW.controller_lease_expires_at
    THEN
        RAISE EXCEPTION 'runtime deployment controller shadow differs from its core snapshot'
            USING ERRCODE = '23514';
    END IF;

    IF NEW.controller_lease_expires_at IS NOT NULL
        AND (
            NEW.controller_lease_expires_at <= mutation_clock
            OR NEW.controller_lease_expires_at > mutation_clock + INTERVAL '10 minutes'
        )
    THEN
        RAISE EXCEPTION 'runtime controller lease exceeds its bounded lifetime'
            USING ERRCODE = '23514';
    END IF;

    IF NEW.snapshot #>> '{phase,condition,condition}' = 'retryable' THEN
        IF (NEW.snapshot #>> '{phase,condition,retry_not_before}')::TIMESTAMPTZ
                IS DISTINCT FROM NEW.next_retry_at
            OR NEW.next_retry_at > mutation_clock + INTERVAL '1 day'
            OR NEW.blocked_at IS NOT NULL
        THEN
            RAISE EXCEPTION 'runtime retry projection differs from its core snapshot'
                USING ERRCODE = '23514';
        END IF;
    ELSIF NEW.snapshot #>> '{phase,condition,condition}' = 'blocked' THEN
        IF NEW.next_retry_at IS NOT NULL
            OR (NEW.snapshot #>> '{phase,condition,failure,recorded_at}')::TIMESTAMPTZ
                IS DISTINCT FROM NEW.blocked_at
        THEN
            RAISE EXCEPTION 'runtime blocked projection differs from its core snapshot'
                USING ERRCODE = '23514';
        END IF;
    ELSIF NEW.next_retry_at IS NOT NULL OR NEW.blocked_at IS NOT NULL THEN
        RAISE EXCEPTION 'runtime pending projection differs from its core snapshot'
            USING ERRCODE = '23514';
    END IF;

    IF NEW.phase = 'live' AND (NEW.snapshot #>> '{live,certified_at}')::TIMESTAMPTZ
            IS DISTINCT FROM NEW.live_at
    THEN
        RAISE EXCEPTION 'runtime Live time differs from its core snapshot'
            USING ERRCODE = '23514';
    END IF;

    IF NEW.phase = 'superseded'
        AND (NEW.snapshot #>> '{phase,superseded_at}')::TIMESTAMPTZ
            IS DISTINCT FROM NEW.superseded_at
    THEN
        RAISE EXCEPTION 'runtime superseded time differs from its core snapshot'
            USING ERRCODE = '23514';
    END IF;

    IF NEW.phase = 'cancelled'
        AND (NEW.snapshot #>> '{phase,cancelled_at}')::TIMESTAMPTZ
            IS DISTINCT FROM NEW.cancelled_at
    THEN
        RAISE EXCEPTION 'runtime cancelled time differs from its core snapshot'
            USING ERRCODE = '23514';
    END IF;

    IF NEW.snapshot -> 'last_runtime_failure' <> 'null'::JSONB THEN
        stable_failure_message := CASE NEW.snapshot #>> '{last_runtime_failure,failure,kind}'
            WHEN 'environment_unavailable' THEN 'runtime environment unavailable'
            WHEN 'activation_not_observable' THEN 'activation not observable'
            WHEN 'panel_reconciliation' THEN 'panel reconciliation failed'
            WHEN 'gateway_start' THEN 'gateway start failed'
            WHEN 'gateway_ready_timeout' THEN 'gateway Ready timed out'
            WHEN 'invariant_violation' THEN 'runtime invariant rejected'
            ELSE NULL
        END;
        IF stable_failure_message IS NULL
            OR NEW.snapshot #>> '{last_runtime_failure,failure,message}'
                IS DISTINCT FROM stable_failure_message
        THEN
            RAISE EXCEPTION 'runtime failure projection must expose stable evidence only'
                USING ERRCODE = '23514';
        END IF;
    END IF;

    IF TG_OP = 'UPDATE' THEN
        IF NEW.deployment_id IS DISTINCT FROM OLD.deployment_id
            OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id
            OR NEW.installation_id IS DISTINCT FROM OLD.installation_id
            OR NEW.promotion_id IS DISTINCT FROM OLD.promotion_id
            OR NEW.activation_request_id IS DISTINCT FROM OLD.activation_request_id
            OR NEW.installation_authority_revision IS DISTINCT FROM OLD.installation_authority_revision
            OR NEW.guild_id IS DISTINCT FROM OLD.guild_id
            OR NEW.ruleset_key IS DISTINCT FROM OLD.ruleset_key
            OR NEW.target_version IS DISTINCT FROM OLD.target_version
            OR NEW.target_content_hash IS DISTINCT FROM OLD.target_content_hash
            OR NEW.binding_revision IS DISTINCT FROM OLD.binding_revision
            OR NEW.binding_fingerprint IS DISTINCT FROM OLD.binding_fingerprint
            OR NEW.desired_target_digest IS DISTINCT FROM OLD.desired_target_digest
            OR NEW.runtime_generation IS DISTINCT FROM OLD.runtime_generation
            OR NEW.previous_runtime IS DISTINCT FROM OLD.previous_runtime
            OR NEW.requested_at IS DISTINCT FROM OLD.requested_at
            OR NEW.created_at IS DISTINCT FROM OLD.created_at
            OR NEW.snapshot_format_version IS DISTINCT FROM OLD.snapshot_format_version
        THEN
            RAISE EXCEPTION 'runtime deployment immutable identity cannot change'
                USING ERRCODE = '23514';
        END IF;
        IF NEW.revision <> OLD.revision + 1
            OR NEW.updated_at <= OLD.updated_at
            OR NEW.updated_at < mutation_clock
            OR NEW.updated_at > mutation_clock + INTERVAL '1 microsecond'
        THEN
            RAISE EXCEPTION 'runtime deployment revision and update time must advance once'
                USING ERRCODE = '23514';
        END IF;
        IF OLD.phase IN ('superseded','cancelled') THEN
            RAISE EXCEPTION 'terminal runtime deployment is immutable'
                USING ERRCODE = '23514';
        END IF;
        IF NOT (
            NEW.phase = OLD.phase
            OR (OLD.phase = 'requested' AND NEW.phase IN ('preflight_ready','cancelled','superseded'))
            OR (OLD.phase = 'preflight_ready' AND NEW.phase IN ('drain_requested','cancelled','superseded'))
            OR (OLD.phase = 'drain_requested' AND NEW.phase IN ('drained','cancelled','superseded'))
            OR (OLD.phase = 'drained' AND NEW.phase IN ('activation_applying','superseded'))
            OR (OLD.phase = 'activation_applying' AND NEW.phase IN ('runtime_pending','superseded'))
            OR (OLD.phase = 'runtime_pending' AND NEW.phase IN ('reconciling_panels','superseded'))
            OR (OLD.phase = 'reconciling_panels' AND NEW.phase IN ('runtime_pending','awaiting_gateway_ready','superseded'))
            OR (OLD.phase = 'awaiting_gateway_ready' AND NEW.phase IN ('runtime_pending','live','superseded'))
            OR (OLD.phase = 'live' AND NEW.phase = 'runtime_pending')
        ) THEN
            RAISE EXCEPTION 'runtime deployment phase transition is invalid'
                USING ERRCODE = '23514';
        END IF;
        IF OLD.phase = 'live' AND NEW.phase = 'runtime_pending' THEN
            SELECT *
            INTO attestation_row
            FROM public.runtime_attestations
            WHERE tenant_id = OLD.tenant_id
                AND installation_id = OLD.installation_id
                AND deployment_id = OLD.deployment_id
                AND attestation_id = OLD.live_attestation_id
            FOR KEY SHARE;

            SELECT *
            INTO serving_row
            FROM public.runtime_serving_leases
            WHERE guild_id = OLD.guild_id
                AND ruleset_key = OLD.ruleset_key
            FOR SHARE;

            IF NEW.snapshot -> 'preflight' IS DISTINCT FROM OLD.snapshot -> 'preflight'
                OR NEW.snapshot -> 'drain' IS DISTINCT FROM OLD.snapshot -> 'drain'
                OR NEW.snapshot -> 'activation' IS DISTINCT FROM OLD.snapshot -> 'activation'
                OR NEW.snapshot -> 'last_runtime_failure' IS DISTINCT FROM OLD.snapshot -> 'last_runtime_failure'
                OR NEW.snapshot -> 'panel_certificate' IS DISTINCT FROM 'null'::JSONB
                OR NEW.snapshot -> 'gateway_ready' IS DISTINCT FROM 'null'::JSONB
                OR NEW.snapshot -> 'live' IS DISTINCT FROM 'null'::JSONB
                OR NEW.snapshot #>> '{phase,condition,condition}' IS DISTINCT FROM 'ready'
                OR NEW.snapshot #> '{last_live_recovery,prior_live}' IS DISTINCT FROM OLD.snapshot -> 'live'
                OR (
                    NEW.snapshot #>> '{last_live_recovery,kind}'
                    IN ('serving_lease_expired','serving_disconnected')
                ) IS DISTINCT FROM TRUE
                OR (NEW.snapshot #>> '{last_live_recovery,recovered_at}')::TIMESTAMPTZ
                    IS DISTINCT FROM mutation_clock
                OR (NEW.snapshot #>> '{last_live_recovery,evidence_at}')::TIMESTAMPTZ
                    > mutation_clock
                OR (NEW.snapshot #>> '{last_live_recovery,evidence_at}')::TIMESTAMPTZ
                    < (OLD.snapshot #>> '{live,certified_at}')::TIMESTAMPTZ
                OR NEW.last_fencing_token IS DISTINCT FROM OLD.last_fencing_token
                OR attestation_row.attestation_id IS NULL
                OR serving_row.deployment_id IS NULL
                OR attestation_row.process_instance_id
                    IS DISTINCT FROM OLD.snapshot #>> '{live,process_instance_id}'
                OR attestation_row.runtime_generation IS DISTINCT FROM OLD.runtime_generation
                OR serving_row.tenant_id IS DISTINCT FROM OLD.tenant_id
                OR serving_row.installation_id IS DISTINCT FROM OLD.installation_id
                OR serving_row.deployment_id IS DISTINCT FROM OLD.deployment_id
                OR serving_row.attestation_id IS DISTINCT FROM OLD.live_attestation_id
                OR serving_row.process_instance_id
                    IS DISTINCT FROM OLD.snapshot #>> '{live,process_instance_id}'
                OR serving_row.runtime_generation IS DISTINCT FROM OLD.runtime_generation
                OR (
                    NEW.snapshot #>> '{last_live_recovery,kind}' = 'serving_disconnected'
                    AND (
                        (serving_row.connected AND serving_row.serving)
                        OR (NEW.snapshot #>> '{last_live_recovery,evidence_at}')::TIMESTAMPTZ
                            IS DISTINCT FROM serving_row.last_heartbeat_at
                    )
                )
                OR (
                    NEW.snapshot #>> '{last_live_recovery,kind}' = 'serving_lease_expired'
                    AND (
                        NOT serving_row.connected
                        OR NOT serving_row.serving
                        OR serving_row.expires_at > mutation_clock
                        OR (NEW.snapshot #>> '{last_live_recovery,evidence_at}')::TIMESTAMPTZ
                            IS DISTINCT FROM serving_row.expires_at
                    )
                )
            THEN
                RAISE EXCEPTION 'Live recovery must preserve activation and bind exact stale process evidence'
                    USING ERRCODE = '23514';
            END IF;
        END IF;
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER runtime_deployments_validate_projection
BEFORE INSERT OR UPDATE ON public.runtime_deployments
FOR EACH ROW
EXECUTE FUNCTION public.validate_runtime_deployment_projection();

CREATE FUNCTION public.reject_runtime_deployment_delete()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    RAISE EXCEPTION 'runtime deployments cannot be deleted directly'
        USING ERRCODE = '23514';
END;
$function$;

CREATE TRIGGER runtime_deployments_reject_delete
BEFORE DELETE ON public.runtime_deployments
FOR EACH ROW
EXECUTE FUNCTION public.reject_runtime_deployment_delete();

CREATE FUNCTION public.validate_runtime_attestation_projection()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    deployment_row public.runtime_deployments%ROWTYPE;
    mutation_clock TIMESTAMPTZ;
BEGIN
    mutation_clock := public.starring_runtime_current_mutation_clock();
    SELECT *
    INTO deployment_row
    FROM public.runtime_deployments
    WHERE tenant_id = NEW.tenant_id
        AND installation_id = NEW.installation_id
        AND deployment_id = NEW.deployment_id
    FOR SHARE;

    IF NOT FOUND
        OR deployment_row.promotion_id IS DISTINCT FROM NEW.promotion_id
        OR deployment_row.activation_request_id IS DISTINCT FROM NEW.activation_request_id
        OR deployment_row.guild_id IS DISTINCT FROM NEW.guild_id
        OR deployment_row.ruleset_key IS DISTINCT FROM NEW.ruleset_key
        OR deployment_row.target_version IS DISTINCT FROM NEW.target_version
        OR deployment_row.target_content_hash IS DISTINCT FROM NEW.target_content_hash
        OR deployment_row.binding_revision IS DISTINCT FROM NEW.binding_revision
        OR deployment_row.binding_fingerprint IS DISTINCT FROM NEW.binding_fingerprint
        OR deployment_row.runtime_generation IS DISTINCT FROM NEW.runtime_generation
        OR deployment_row.phase <> 'awaiting_gateway_ready'
        OR NEW.deployment_revision <> deployment_row.revision + 1
        OR deployment_row.controller_fencing_token IS DISTINCT FROM NEW.controller_fencing_token
        OR NEW.certified_at IS DISTINCT FROM mutation_clock
        OR NEW.gateway_ready_at < mutation_clock - INTERVAL '10 minutes'
    THEN
        RAISE EXCEPTION 'runtime attestation differs from its fenced deployment target'
            USING ERRCODE = '23514';
    END IF;

    IF NEW.record #>> '{live,target,guild_id}' IS DISTINCT FROM NEW.guild_id
        OR NEW.record #>> '{live,target,ruleset_key}' IS DISTINCT FROM NEW.ruleset_key
        OR NEW.record #>> '{live,target,version}' IS DISTINCT FROM NEW.target_version::TEXT
        OR NEW.record #>> '{live,target,content_hash}' IS DISTINCT FROM NEW.target_content_hash
        OR NEW.record #>> '{live,target,binding_revision}' IS DISTINCT FROM NEW.binding_revision::TEXT
        OR NEW.record #>> '{live,target,binding_fingerprint}' IS DISTINCT FROM NEW.binding_fingerprint
        OR NEW.record #>> '{live,runtime_generation}' IS DISTINCT FROM NEW.runtime_generation::TEXT
        OR NEW.record #>> '{live,process_instance_id}' IS DISTINCT FROM NEW.process_instance_id
        OR NEW.record #>> '{live,activation,activation_request_id}'
            IS DISTINCT FROM NEW.activation_request_id
        OR NEW.record #>> '{live,panel_certificate,certificate_id}'
            IS DISTINCT FROM NEW.panel_certificate_id
        OR NEW.record #>> '{live,gateway_ready,kind}' IS DISTINCT FROM NEW.gateway_ready_kind
        OR (NEW.record #>> '{live,gateway_ready,ready_at}')::TIMESTAMPTZ
            IS DISTINCT FROM NEW.gateway_ready_at
        OR (NEW.record #>> '{live,certified_at}')::TIMESTAMPTZ
            IS DISTINCT FROM NEW.certified_at
        OR NEW.record ->> 'runtime_build_revision' IS DISTINCT FROM NEW.runtime_build_revision
        OR NEW.record ->> 'panel_report_digest' IS DISTINCT FROM NEW.panel_report_digest
        OR NEW.record ->> 'gateway_shard_id' IS DISTINCT FROM NEW.gateway_shard_id
        OR NEW.record ->> 'controller_fencing_token'
            IS DISTINCT FROM NEW.controller_fencing_token::TEXT
        OR NEW.record ->> 'deployment_revision' IS DISTINCT FROM NEW.deployment_revision::TEXT
    THEN
        RAISE EXCEPTION 'runtime attestation shadow columns differ from its record'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER runtime_attestations_validate_projection
BEFORE INSERT ON public.runtime_attestations
FOR EACH ROW
EXECUTE FUNCTION public.validate_runtime_attestation_projection();

CREATE TRIGGER runtime_attestations_reject_mutation
BEFORE UPDATE OR DELETE ON public.runtime_attestations
FOR EACH ROW
EXECUTE FUNCTION public.reject_immutable_product_row();

CREATE FUNCTION public.validate_runtime_serving_lease_transition()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
DECLARE
    attestation_row public.runtime_attestations%ROWTYPE;
    mutation_clock TIMESTAMPTZ;
BEGIN
    mutation_clock := public.starring_runtime_current_mutation_clock();
    SELECT *
    INTO attestation_row
    FROM public.runtime_attestations
    WHERE tenant_id = NEW.tenant_id
        AND installation_id = NEW.installation_id
        AND deployment_id = NEW.deployment_id
        AND attestation_id = NEW.attestation_id
    FOR KEY SHARE;

    IF NOT FOUND
        OR attestation_row.guild_id IS DISTINCT FROM NEW.guild_id
        OR attestation_row.ruleset_key IS DISTINCT FROM NEW.ruleset_key
        OR attestation_row.target_version IS DISTINCT FROM NEW.target_version
        OR attestation_row.target_content_hash IS DISTINCT FROM NEW.target_content_hash
        OR attestation_row.binding_revision IS DISTINCT FROM NEW.binding_revision
        OR attestation_row.binding_fingerprint IS DISTINCT FROM NEW.binding_fingerprint
        OR attestation_row.runtime_generation IS DISTINCT FROM NEW.runtime_generation
        OR attestation_row.process_instance_id IS DISTINCT FROM NEW.process_instance_id
    THEN
        RAISE EXCEPTION 'runtime serving lease differs from its immutable attestation'
            USING ERRCODE = '23514';
    END IF;

    IF TG_OP = 'INSERT' THEN
        IF NEW.lease_epoch <> 1
            OR NEW.revision <> 1
            OR NEW.acquired_at IS DISTINCT FROM mutation_clock
            OR NEW.last_heartbeat_at IS DISTINCT FROM mutation_clock
            OR NEW.expires_at <= mutation_clock
            OR NEW.expires_at > mutation_clock + INTERVAL '5 minutes'
            OR NOT NEW.connected
            OR NOT NEW.serving
        THEN
            RAISE EXCEPTION 'initial runtime serving lease must be a fresh serving epoch'
                USING ERRCODE = '23514';
        END IF;
    ELSIF TG_OP = 'UPDATE' THEN
        IF NEW.revision <> OLD.revision + 1 THEN
            RAISE EXCEPTION 'runtime serving lease revision must advance once'
                USING ERRCODE = '23514';
        END IF;
        IF NEW.lease_epoch = OLD.lease_epoch THEN
            IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id
                OR NEW.installation_id IS DISTINCT FROM OLD.installation_id
                OR NEW.deployment_id IS DISTINCT FROM OLD.deployment_id
                OR NEW.attestation_id IS DISTINCT FROM OLD.attestation_id
                OR NEW.process_instance_id IS DISTINCT FROM OLD.process_instance_id
                OR NEW.runtime_generation IS DISTINCT FROM OLD.runtime_generation
                OR NEW.target_version IS DISTINCT FROM OLD.target_version
                OR NEW.target_content_hash IS DISTINCT FROM OLD.target_content_hash
                OR NEW.binding_revision IS DISTINCT FROM OLD.binding_revision
                OR NEW.binding_fingerprint IS DISTINCT FROM OLD.binding_fingerprint
                OR NEW.acquired_at IS DISTINCT FROM OLD.acquired_at
                OR NEW.last_heartbeat_at IS DISTINCT FROM mutation_clock
                OR (NOT OLD.connected AND NEW.connected)
                OR (NOT OLD.serving AND NEW.serving)
                OR (
                    NEW.connected
                    AND (
                        NEW.expires_at <= mutation_clock
                        OR NEW.expires_at > mutation_clock + INTERVAL '5 minutes'
                        OR NEW.expires_at < OLD.expires_at
                    )
                )
                OR (
                    NOT NEW.connected
                    AND NEW.expires_at IS DISTINCT FROM mutation_clock
                )
            THEN
                RAISE EXCEPTION 'runtime serving heartbeat cannot change its fenced identity'
                    USING ERRCODE = '23514';
            END IF;
        ELSIF NEW.lease_epoch = OLD.lease_epoch + 1 THEN
            IF NEW.acquired_at IS DISTINCT FROM mutation_clock
                OR NEW.last_heartbeat_at IS DISTINCT FROM mutation_clock
                OR NEW.expires_at <= mutation_clock
                OR NEW.expires_at > mutation_clock + INTERVAL '5 minutes'
                OR NOT NEW.connected
                OR NOT NEW.serving
            THEN
                RAISE EXCEPTION 'replacement runtime serving lease must start a fresh serving epoch'
                    USING ERRCODE = '23514';
            END IF;
            IF OLD.expires_at > mutation_clock AND (OLD.connected OR OLD.serving) THEN
                RAISE EXCEPTION 'connected runtime serving lease cannot be replaced before expiry'
                    USING ERRCODE = '55006';
            END IF;
        ELSE
            RAISE EXCEPTION 'runtime serving lease epoch must remain or advance once'
                USING ERRCODE = '23514';
        END IF;
        IF NEW.last_heartbeat_at < OLD.last_heartbeat_at THEN
            RAISE EXCEPTION 'runtime serving heartbeat cannot move backward'
                USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER runtime_serving_leases_validate_transition
BEFORE INSERT OR UPDATE ON public.runtime_serving_leases
FOR EACH ROW
EXECUTE FUNCTION public.validate_runtime_serving_lease_transition();

CREATE FUNCTION public.reject_runtime_serving_lease_delete()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
BEGIN
    RAISE EXCEPTION 'runtime serving leases cannot be deleted directly'
        USING ERRCODE = '23514';
END;
$function$;

CREATE TRIGGER runtime_serving_leases_reject_delete
BEFORE DELETE ON public.runtime_serving_leases
FOR EACH ROW
EXECUTE FUNCTION public.reject_runtime_serving_lease_delete();

REVOKE ALL ON TABLE public.runtime_deployments FROM PUBLIC;
REVOKE ALL ON TABLE public.runtime_attestations FROM PUBLIC;
REVOKE ALL ON TABLE public.runtime_serving_leases FROM PUBLIC;
REVOKE ALL ON FUNCTION public.validate_runtime_deployment_projection() FROM PUBLIC;
REVOKE ALL ON FUNCTION public.reject_runtime_deployment_delete() FROM PUBLIC;
REVOKE ALL ON FUNCTION public.validate_runtime_attestation_projection() FROM PUBLIC;
REVOKE ALL ON FUNCTION public.validate_runtime_serving_lease_transition() FROM PUBLIC;
REVOKE ALL ON FUNCTION public.reject_runtime_serving_lease_delete() FROM PUBLIC;
