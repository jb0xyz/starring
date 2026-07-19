CREATE TABLE product_principals (
    principal_id TEXT PRIMARY KEY,
    discord_user_id TEXT NOT NULL UNIQUE,
    disabled BOOLEAN NOT NULL DEFAULT FALSE,
    identity_revision BIGINT NOT NULL DEFAULT 1,
    display_profile JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_authenticated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT product_principals_id_format CHECK (
        principal_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT product_principals_discord_id_format CHECK (
        CASE
            WHEN discord_user_id ~ '^[1-9][0-9]{0,19}$'
                THEN discord_user_id::NUMERIC <= 18446744073709551615
            ELSE FALSE
        END
    ),
    CONSTRAINT product_principals_revision_valid CHECK (
        identity_revision BETWEEN 1 AND 9223372036854775807
    ),
    CONSTRAINT product_principals_profile_valid CHECK (
        jsonb_typeof(display_profile) = 'object'
        AND octet_length(display_profile::TEXT) <= 16384
    ),
    CONSTRAINT product_principals_timestamps_valid CHECK (
        created_at <= last_authenticated_at
        AND last_authenticated_at <= updated_at
    )
);

CREATE TABLE product_oauth_flows (
    state_digest BYTEA PRIMARY KEY,
    browser_nonce_digest BYTEA NOT NULL UNIQUE,
    redirect_uri TEXT NOT NULL,
    return_path TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    terminal_result_code TEXT,
    CONSTRAINT product_oauth_flows_state_digest_valid CHECK (
        octet_length(state_digest) = 32
    ),
    CONSTRAINT product_oauth_flows_nonce_digest_valid CHECK (
        octet_length(browser_nonce_digest) = 32
    ),
    CONSTRAINT product_oauth_flows_distinct_digests CHECK (
        state_digest <> browser_nonce_digest
    ),
    CONSTRAINT product_oauth_flows_redirect_uri_valid CHECK (
        char_length(redirect_uri) BETWEEN 1 AND 2048
        AND redirect_uri = btrim(redirect_uri)
        AND redirect_uri LIKE 'https://%'
        AND redirect_uri !~ '[[:space:][:cntrl:]]'
        AND position('#' IN redirect_uri) = 0
    ),
    CONSTRAINT product_oauth_flows_return_path_valid CHECK (
        char_length(return_path) BETWEEN 1 AND 256
        AND return_path ~ '^/[A-Za-z0-9/_-]*$'
        AND position('//' IN return_path) = 0
        AND return_path !~ '(^|/)[.][.](/|$)'
    ),
    CONSTRAINT product_oauth_flows_expiry_valid CHECK (
        expires_at > created_at
    ),
    CONSTRAINT product_oauth_flows_terminal_valid CHECK (
        (
            consumed_at IS NULL
            AND terminal_result_code IS NULL
        )
        OR (
            consumed_at IS NOT NULL
            AND consumed_at >= created_at
            AND consumed_at <= expires_at
            AND terminal_result_code ~ '^[a-z][a-z0-9_.:-]{0,63}$'
        )
    )
);

CREATE TABLE product_auth_sessions (
    session_digest BYTEA PRIMARY KEY,
    principal_id TEXT NOT NULL,
    csrf_digest BYTEA NOT NULL UNIQUE,
    authenticated_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    idle_expires_at TIMESTAMPTZ NOT NULL,
    absolute_expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    revocation_reason TEXT,
    CONSTRAINT product_auth_sessions_principal_fk FOREIGN KEY (principal_id)
        REFERENCES product_principals (principal_id)
        ON DELETE RESTRICT,
    CONSTRAINT product_auth_sessions_identity_unique UNIQUE (
        session_digest,
        principal_id
    ),
    CONSTRAINT product_auth_sessions_session_digest_valid CHECK (
        octet_length(session_digest) = 32
    ),
    CONSTRAINT product_auth_sessions_csrf_digest_valid CHECK (
        octet_length(csrf_digest) = 32
    ),
    CONSTRAINT product_auth_sessions_distinct_digests CHECK (
        session_digest <> csrf_digest
    ),
    CONSTRAINT product_auth_sessions_principal_id_format CHECK (
        principal_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT product_auth_sessions_timestamps_valid CHECK (
        authenticated_at <= created_at
        AND created_at <= last_seen_at
        AND last_seen_at < idle_expires_at
        AND idle_expires_at <= absolute_expires_at
        AND absolute_expires_at > created_at
        AND (revoked_at IS NULL OR revoked_at >= last_seen_at)
    ),
    CONSTRAINT product_auth_sessions_revocation_valid CHECK (
        (revoked_at IS NULL AND revocation_reason IS NULL)
        OR (
            revoked_at IS NOT NULL
            AND revocation_reason ~ '^[a-z][a-z0-9_.:-]{0,63}$'
        )
    )
);

CREATE TABLE product_tenants (
    tenant_id TEXT PRIMARY KEY,
    lifecycle_state TEXT NOT NULL,
    display_name TEXT NOT NULL,
    display_metadata JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT product_tenants_id_format CHECK (
        tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT product_tenants_state_valid CHECK (
        lifecycle_state IN ('provisioning','active','suspended','disabled')
    ),
    CONSTRAINT product_tenants_display_name_valid CHECK (
        char_length(display_name) BETWEEN 1 AND 128
        AND display_name = btrim(display_name)
        AND display_name !~ '[[:cntrl:]]'
    ),
    CONSTRAINT product_tenants_metadata_valid CHECK (
        jsonb_typeof(display_metadata) = 'object'
        AND octet_length(display_metadata::TEXT) <= 16384
    ),
    CONSTRAINT product_tenants_timestamps_valid CHECK (
        created_at <= updated_at
    )
);

CREATE TABLE automation_installations (
    installation_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    discord_application_id TEXT NOT NULL,
    discord_guild_id TEXT NOT NULL,
    ruleset_key TEXT NOT NULL,
    lifecycle_state TEXT NOT NULL,
    current_authority_revision BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT automation_installations_tenant_fk FOREIGN KEY (tenant_id)
        REFERENCES product_tenants (tenant_id)
        ON DELETE RESTRICT,
    CONSTRAINT automation_installations_scope_unique UNIQUE (
        tenant_id,
        installation_id
    ),
    CONSTRAINT automation_installations_application_guild_unique UNIQUE (
        discord_application_id,
        discord_guild_id
    ),
    CONSTRAINT automation_installations_ruleset_identity_unique UNIQUE (
        discord_guild_id,
        ruleset_key
    ),
    CONSTRAINT automation_installations_id_format CHECK (
        installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT automation_installations_tenant_id_format CHECK (
        tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT automation_installations_application_id_format CHECK (
        CASE
            WHEN discord_application_id ~ '^[1-9][0-9]{0,19}$'
                THEN discord_application_id::NUMERIC <= 18446744073709551615
            ELSE FALSE
        END
    ),
    CONSTRAINT automation_installations_guild_id_format CHECK (
        CASE
            WHEN discord_guild_id ~ '^[1-9][0-9]{0,19}$'
                THEN discord_guild_id::NUMERIC <= 18446744073709551615
            ELSE FALSE
        END
    ),
    CONSTRAINT automation_installations_ruleset_key_format CHECK (
        ruleset_key ~ '^[A-Za-z0-9_-]{1,64}$'
    ),
    CONSTRAINT automation_installations_state_valid CHECK (
        lifecycle_state IN ('provisioning','active','suspended','disabled')
    ),
    CONSTRAINT automation_installations_authority_revision_valid CHECK (
        current_authority_revision BETWEEN 1 AND 9223372036854775807
    ),
    CONSTRAINT automation_installations_timestamps_valid CHECK (
        created_at <= updated_at
    )
);

CREATE TABLE automation_installation_authority_versions (
    installation_id TEXT NOT NULL,
    revision BIGINT NOT NULL,
    tenant_id TEXT NOT NULL,
    binding_revision BIGINT NOT NULL,
    resource_bindings JSONB NOT NULL,
    binding_fingerprint TEXT NOT NULL,
    policy_revision BIGINT NOT NULL,
    required_approvals INTEGER NOT NULL,
    activation_ttl_seconds BIGINT NOT NULL,
    authority_payload_digest TEXT NOT NULL,
    created_by_principal_id TEXT NOT NULL,
    created_by_request_digest TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (installation_id, revision),
    CONSTRAINT installation_authority_installation_fk FOREIGN KEY (
        tenant_id,
        installation_id
    ) REFERENCES automation_installations (tenant_id, installation_id)
        ON DELETE RESTRICT,
    CONSTRAINT installation_authority_principal_fk FOREIGN KEY (
        created_by_principal_id
    ) REFERENCES product_principals (principal_id)
        ON DELETE RESTRICT,
    CONSTRAINT installation_authority_scope_unique UNIQUE (
        tenant_id,
        installation_id,
        revision
    ),
    CONSTRAINT installation_authority_payload_unique UNIQUE (
        tenant_id,
        installation_id,
        authority_payload_digest
    ),
    CONSTRAINT installation_authority_request_unique UNIQUE (
        tenant_id,
        installation_id,
        created_by_request_digest
    ),
    CONSTRAINT installation_authority_installation_id_format CHECK (
        installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT installation_authority_tenant_id_format CHECK (
        tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT installation_authority_revision_valid CHECK (
        revision BETWEEN 1 AND 9223372036854775807
        AND binding_revision BETWEEN 1 AND 9223372036854775807
        AND policy_revision BETWEEN 1 AND 9223372036854775807
    ),
    CONSTRAINT installation_authority_bindings_valid CHECK (
        jsonb_typeof(resource_bindings) = 'object'
        AND octet_length(resource_bindings::TEXT) <= 262144
    ),
    CONSTRAINT installation_authority_binding_fingerprint_valid CHECK (
        binding_fingerprint ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT installation_authority_policy_valid CHECK (
        required_approvals BETWEEN 1 AND 64
        AND activation_ttl_seconds BETWEEN 1 AND 31536000
    ),
    CONSTRAINT installation_authority_payload_digest_valid CHECK (
        authority_payload_digest ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT installation_authority_principal_id_format CHECK (
        created_by_principal_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT installation_authority_request_digest_valid CHECK (
        created_by_request_digest ~ '^[0-9a-f]{64}$'
    )
);

ALTER TABLE automation_installations
    ADD CONSTRAINT automation_installations_authority_head_fk FOREIGN KEY (
        tenant_id,
        installation_id,
        current_authority_revision
    ) REFERENCES automation_installation_authority_versions (
        tenant_id,
        installation_id,
        revision
    ) ON DELETE RESTRICT
    DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE authoring_sessions (
    session_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    installation_id TEXT NOT NULL,
    owner_principal_id TEXT NOT NULL,
    current_generation BIGINT NOT NULL,
    lifecycle_state TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT authoring_sessions_installation_fk FOREIGN KEY (
        tenant_id,
        installation_id
    ) REFERENCES automation_installations (tenant_id, installation_id)
        ON DELETE RESTRICT,
    CONSTRAINT authoring_sessions_owner_fk FOREIGN KEY (owner_principal_id)
        REFERENCES product_principals (principal_id)
        ON DELETE RESTRICT,
    CONSTRAINT authoring_sessions_scope_unique UNIQUE (
        tenant_id,
        installation_id,
        session_id
    ),
    CONSTRAINT authoring_sessions_id_format CHECK (
        session_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT authoring_sessions_tenant_id_format CHECK (
        tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT authoring_sessions_installation_id_format CHECK (
        installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT authoring_sessions_owner_id_format CHECK (
        owner_principal_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT authoring_sessions_generation_valid CHECK (
        current_generation BETWEEN 1 AND 9223372036854775807
    ),
    CONSTRAINT authoring_sessions_state_valid CHECK (
        lifecycle_state IN ('active','closed','archived')
    ),
    CONSTRAINT authoring_sessions_timestamps_valid CHECK (
        created_at <= updated_at
    )
);

CREATE TABLE authoring_session_generations (
    session_id TEXT NOT NULL,
    generation BIGINT NOT NULL,
    tenant_id TEXT NOT NULL,
    installation_id TEXT NOT NULL,
    snapshot_schema_version BIGINT NOT NULL,
    snapshot_ciphertext BYTEA NOT NULL,
    snapshot_nonce BYTEA NOT NULL,
    encryption_key_id TEXT NOT NULL,
    encryption_suite TEXT NOT NULL,
    encryption_suite_version SMALLINT NOT NULL,
    authenticated_metadata_digest TEXT NOT NULL,
    resource_bindings JSONB NOT NULL,
    binding_fingerprint TEXT NOT NULL,
    installation_authority_revision BIGINT NOT NULL,
    summary JSONB NOT NULL,
    stage TEXT NOT NULL,
    candidate_revision BIGINT,
    candidate_hash TEXT,
    writer_request_digest TEXT NOT NULL,
    harness_contract_revision BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (session_id, generation),
    CONSTRAINT authoring_generations_session_fk FOREIGN KEY (
        tenant_id,
        installation_id,
        session_id
    ) REFERENCES authoring_sessions (tenant_id, installation_id, session_id)
        ON DELETE RESTRICT,
    CONSTRAINT authoring_generations_authority_fk FOREIGN KEY (
        tenant_id,
        installation_id,
        installation_authority_revision
    ) REFERENCES automation_installation_authority_versions (
        tenant_id,
        installation_id,
        revision
    ) ON DELETE RESTRICT,
    CONSTRAINT authoring_generations_scope_unique UNIQUE (
        tenant_id,
        installation_id,
        session_id,
        generation
    ),
    CONSTRAINT authoring_generations_writer_request_unique UNIQUE (
        tenant_id,
        installation_id,
        session_id,
        writer_request_digest
    ),
    CONSTRAINT authoring_generations_session_id_format CHECK (
        session_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT authoring_generations_tenant_id_format CHECK (
        tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT authoring_generations_installation_id_format CHECK (
        installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT authoring_generations_revisions_valid CHECK (
        generation BETWEEN 1 AND 9223372036854775807
        AND snapshot_schema_version BETWEEN 1 AND 4294967295
        AND installation_authority_revision BETWEEN 1 AND 9223372036854775807
        AND harness_contract_revision BETWEEN 1 AND 9223372036854775807
    ),
    CONSTRAINT authoring_generations_ciphertext_valid CHECK (
        octet_length(snapshot_ciphertext) BETWEEN 16 AND 8388608
    ),
    CONSTRAINT authoring_generations_nonce_valid CHECK (
        octet_length(snapshot_nonce) BETWEEN 12 AND 32
    ),
    CONSTRAINT authoring_generations_key_id_valid CHECK (
        encryption_key_id ~ '^[A-Za-z0-9_.:/-]{1,128}$'
    ),
    CONSTRAINT authoring_generations_suite_valid CHECK (
        encryption_suite ~ '^[a-z][a-z0-9_]{0,63}$'
        AND encryption_suite_version BETWEEN 1 AND 32767
    ),
    CONSTRAINT authoring_generations_metadata_digest_valid CHECK (
        authenticated_metadata_digest ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT authoring_generations_bindings_valid CHECK (
        jsonb_typeof(resource_bindings) = 'object'
        AND octet_length(resource_bindings::TEXT) <= 262144
    ),
    CONSTRAINT authoring_generations_binding_fingerprint_valid CHECK (
        binding_fingerprint ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT authoring_generations_summary_valid CHECK (
        jsonb_typeof(summary) = 'object'
        AND octet_length(summary::TEXT) <= 32768
    ),
    CONSTRAINT authoring_generations_stage_valid CHECK (
        stage ~ '^[a-z][a-z0-9_]{0,63}$'
    ),
    CONSTRAINT authoring_generations_candidate_valid CHECK (
        (
            candidate_revision IS NULL
            AND candidate_hash IS NULL
        )
        OR (
            candidate_revision BETWEEN 1 AND 9223372036854775807
            AND candidate_hash ~ '^[0-9a-f]{64}$'
        )
    ),
    CONSTRAINT authoring_generations_preview_projection_valid CHECK (
        stage <> 'preview_ready'
        OR candidate_revision IS NOT NULL
    ),
    CONSTRAINT authoring_generations_writer_digest_valid CHECK (
        writer_request_digest ~ '^[0-9a-f]{64}$'
    )
);

ALTER TABLE authoring_sessions
    ADD CONSTRAINT authoring_sessions_generation_head_fk FOREIGN KEY (
        tenant_id,
        installation_id,
        session_id,
        current_generation
    ) REFERENCES authoring_session_generations (
        tenant_id,
        installation_id,
        session_id,
        generation
    ) ON DELETE RESTRICT
    DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE product_action_receipts (
    receipt_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    installation_id TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    endpoint_domain TEXT NOT NULL,
    idempotency_key_digest TEXT NOT NULL,
    request_digest TEXT NOT NULL,
    target_resource_type TEXT NOT NULL,
    target_resource_id TEXT NOT NULL,
    resulting_revision BIGINT,
    resulting_state TEXT NOT NULL,
    result_code TEXT NOT NULL,
    http_disposition_class SMALLINT NOT NULL,
    completed_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT product_action_receipts_installation_fk FOREIGN KEY (
        tenant_id,
        installation_id
    ) REFERENCES automation_installations (tenant_id, installation_id)
        ON DELETE RESTRICT,
    CONSTRAINT product_action_receipts_principal_fk FOREIGN KEY (principal_id)
        REFERENCES product_principals (principal_id)
        ON DELETE RESTRICT,
    CONSTRAINT product_action_receipts_scope_identity_unique UNIQUE (
        tenant_id,
        installation_id,
        principal_id,
        receipt_id
    ),
    CONSTRAINT product_action_receipts_idempotency_unique UNIQUE (
        tenant_id,
        installation_id,
        principal_id,
        endpoint_domain,
        idempotency_key_digest
    ),
    CONSTRAINT product_action_receipts_id_format CHECK (
        receipt_id ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT product_action_receipts_tenant_id_format CHECK (
        tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT product_action_receipts_installation_id_format CHECK (
        installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT product_action_receipts_principal_id_format CHECK (
        principal_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT product_action_receipts_endpoint_valid CHECK (
        endpoint_domain ~ '^[a-z][a-z0-9_.:-]{0,63}$'
    ),
    CONSTRAINT product_action_receipts_digests_valid CHECK (
        idempotency_key_digest ~ '^[0-9a-f]{64}$'
        AND request_digest ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT product_action_receipts_target_valid CHECK (
        target_resource_type ~ '^[a-z][a-z0-9_]{0,63}$'
        AND target_resource_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT product_action_receipts_result_valid CHECK (
        (resulting_revision IS NULL OR resulting_revision BETWEEN 1 AND 9223372036854775807)
        AND resulting_state ~ '^[a-z][a-z0-9_]{0,63}$'
        AND result_code ~ '^[a-z][a-z0-9_.:-]{0,63}$'
        AND http_disposition_class IN (2, 4)
    )
);

CREATE TABLE product_audit_events (
    event_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    installation_id TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    product_session_digest BYTEA NOT NULL,
    action TEXT NOT NULL,
    target_resource_type TEXT NOT NULL,
    target_resource_id TEXT NOT NULL,
    request_id TEXT NOT NULL,
    receipt_id TEXT NOT NULL,
    authority_observation_digest TEXT NOT NULL,
    effective_permission_bits NUMERIC(20, 0) NOT NULL,
    authority_observed_at TIMESTAMPTZ NOT NULL,
    installation_authority_revision BIGINT NOT NULL,
    expected_generation BIGINT,
    actual_generation BIGINT,
    payload_digest TEXT,
    binding_fingerprint TEXT,
    policy_revision BIGINT,
    active_baseline_version BIGINT,
    active_baseline_hash TEXT,
    resulting_state TEXT NOT NULL,
    result_code TEXT NOT NULL,
    dependency_latency_classes JSONB NOT NULL DEFAULT '{}'::JSONB,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT product_audit_events_installation_fk FOREIGN KEY (
        tenant_id,
        installation_id
    ) REFERENCES automation_installations (tenant_id, installation_id)
        ON DELETE RESTRICT,
    CONSTRAINT product_audit_events_session_principal_fk FOREIGN KEY (
        product_session_digest,
        principal_id
    ) REFERENCES product_auth_sessions (session_digest, principal_id)
        ON DELETE RESTRICT,
    CONSTRAINT product_audit_events_receipt_fk FOREIGN KEY (
        tenant_id,
        installation_id,
        principal_id,
        receipt_id
    ) REFERENCES product_action_receipts (
        tenant_id,
        installation_id,
        principal_id,
        receipt_id
    ) ON DELETE RESTRICT,
    CONSTRAINT product_audit_events_authority_fk FOREIGN KEY (
        tenant_id,
        installation_id,
        installation_authority_revision
    ) REFERENCES automation_installation_authority_versions (
        tenant_id,
        installation_id,
        revision
    ) ON DELETE RESTRICT,
    CONSTRAINT product_audit_events_receipt_unique UNIQUE (receipt_id),
    CONSTRAINT product_audit_events_request_unique UNIQUE (tenant_id, request_id),
    CONSTRAINT product_audit_events_id_format CHECK (
        event_id ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT product_audit_events_tenant_id_format CHECK (
        tenant_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT product_audit_events_installation_id_format CHECK (
        installation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT product_audit_events_principal_id_format CHECK (
        principal_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT product_audit_events_session_digest_valid CHECK (
        octet_length(product_session_digest) = 32
    ),
    CONSTRAINT product_audit_events_action_valid CHECK (
        action ~ '^[a-z][a-z0-9_.:-]{0,63}$'
    ),
    CONSTRAINT product_audit_events_target_valid CHECK (
        target_resource_type ~ '^[a-z][a-z0-9_]{0,63}$'
        AND target_resource_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT product_audit_events_request_id_format CHECK (
        request_id ~ '^[A-Za-z0-9_.:-]{1,128}$'
    ),
    CONSTRAINT product_audit_events_receipt_id_format CHECK (
        receipt_id ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT product_audit_events_authority_valid CHECK (
        authority_observation_digest ~ '^[0-9a-f]{64}$'
        AND effective_permission_bits >= 0
        AND effective_permission_bits <= 18446744073709551615
        AND installation_authority_revision BETWEEN 1 AND 9223372036854775807
        AND authority_observed_at <= occurred_at
    ),
    CONSTRAINT product_audit_events_generation_valid CHECK (
        (
            expected_generation IS NULL
            AND actual_generation IS NULL
        )
        OR (
            expected_generation BETWEEN 1 AND 9223372036854775807
            AND actual_generation BETWEEN 1 AND 9223372036854775807
        )
    ),
    CONSTRAINT product_audit_events_payload_digest_valid CHECK (
        payload_digest IS NULL OR payload_digest ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT product_audit_events_binding_fingerprint_valid CHECK (
        binding_fingerprint IS NULL OR binding_fingerprint ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT product_audit_events_policy_revision_valid CHECK (
        policy_revision IS NULL OR policy_revision BETWEEN 1 AND 9223372036854775807
    ),
    CONSTRAINT product_audit_events_active_baseline_valid CHECK (
        (
            active_baseline_version IS NULL
            AND active_baseline_hash IS NULL
        )
        OR (
            active_baseline_version BETWEEN 1 AND 4294967295
            AND active_baseline_hash ~ '^[0-9a-f]{64}$'
        )
    ),
    CONSTRAINT product_audit_events_result_valid CHECK (
        resulting_state ~ '^[a-z][a-z0-9_]{0,63}$'
        AND result_code ~ '^[a-z][a-z0-9_.:-]{0,63}$'
    ),
    CONSTRAINT product_audit_events_latency_valid CHECK (
        jsonb_typeof(dependency_latency_classes) = 'object'
        AND octet_length(dependency_latency_classes::TEXT) <= 4096
    )
);

CREATE INDEX product_principals_active_auth_index
ON product_principals (last_authenticated_at DESC, principal_id)
WHERE disabled = FALSE;

CREATE INDEX product_oauth_flows_unconsumed_expiry_index
ON product_oauth_flows (expires_at)
WHERE consumed_at IS NULL;

CREATE INDEX product_auth_sessions_principal_active_index
ON product_auth_sessions (principal_id, idle_expires_at, absolute_expires_at)
WHERE revoked_at IS NULL;

CREATE INDEX product_auth_sessions_expiry_index
ON product_auth_sessions (LEAST(idle_expires_at, absolute_expires_at))
WHERE revoked_at IS NULL;

CREATE INDEX product_tenants_lifecycle_index
ON product_tenants (lifecycle_state, tenant_id);

CREATE INDEX automation_installations_tenant_lifecycle_index
ON automation_installations (tenant_id, lifecycle_state, installation_id);

CREATE INDEX installation_authority_created_index
ON automation_installation_authority_versions (
    tenant_id,
    installation_id,
    created_at DESC
);

CREATE INDEX authoring_sessions_owner_lifecycle_index
ON authoring_sessions (
    tenant_id,
    installation_id,
    owner_principal_id,
    lifecycle_state,
    updated_at DESC
);

CREATE INDEX authoring_generations_created_index
ON authoring_session_generations (
    tenant_id,
    installation_id,
    session_id,
    created_at DESC
);

CREATE INDEX authoring_generations_preview_index
ON authoring_session_generations (
    tenant_id,
    installation_id,
    stage,
    created_at DESC
)
WHERE stage = 'preview_ready';

CREATE INDEX product_action_receipts_target_index
ON product_action_receipts (
    tenant_id,
    installation_id,
    target_resource_type,
    target_resource_id,
    completed_at DESC
);

CREATE INDEX product_action_receipts_principal_time_index
ON product_action_receipts (
    tenant_id,
    installation_id,
    principal_id,
    completed_at DESC
);

CREATE INDEX product_audit_events_target_time_index
ON product_audit_events (
    tenant_id,
    installation_id,
    target_resource_type,
    target_resource_id,
    occurred_at DESC
);

CREATE INDEX product_audit_events_principal_time_index
ON product_audit_events (
    tenant_id,
    installation_id,
    principal_id,
    occurred_at DESC
);

CREATE FUNCTION reject_immutable_product_row()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $function$
BEGIN
    RAISE EXCEPTION 'immutable product records cannot be updated or deleted'
        USING ERRCODE = '23514';
END;
$function$;

CREATE FUNCTION enforce_product_principal_transition()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $function$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'product principals cannot be deleted'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.principal_id IS DISTINCT FROM OLD.principal_id
        OR NEW.discord_user_id IS DISTINCT FROM OLD.discord_user_id
        OR NEW.created_at IS DISTINCT FROM OLD.created_at
    THEN
        RAISE EXCEPTION 'product principal identity is immutable'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.identity_revision <> OLD.identity_revision + 1
        OR NEW.updated_at <= OLD.updated_at
        OR NEW.last_authenticated_at < OLD.last_authenticated_at
    THEN
        RAISE EXCEPTION 'product principal revisions and timestamps must advance monotonically'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER product_principals_enforce_transition
BEFORE UPDATE OR DELETE ON product_principals
FOR EACH ROW
EXECUTE FUNCTION enforce_product_principal_transition();

CREATE FUNCTION enforce_product_oauth_flow_transition()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $function$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'product OAuth flows cannot be deleted directly'
            USING ERRCODE = '23514';
    END IF;
    IF OLD.consumed_at IS NOT NULL THEN
        RAISE EXCEPTION 'consumed product OAuth flows are immutable'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.state_digest IS DISTINCT FROM OLD.state_digest
        OR NEW.browser_nonce_digest IS DISTINCT FROM OLD.browser_nonce_digest
        OR NEW.redirect_uri IS DISTINCT FROM OLD.redirect_uri
        OR NEW.return_path IS DISTINCT FROM OLD.return_path
        OR NEW.created_at IS DISTINCT FROM OLD.created_at
        OR NEW.expires_at IS DISTINCT FROM OLD.expires_at
        OR NEW.consumed_at IS NULL
        OR NEW.terminal_result_code IS NULL
    THEN
        RAISE EXCEPTION 'product OAuth flow updates may only consume an unchanged flow'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER product_oauth_flows_enforce_transition
BEFORE UPDATE OR DELETE ON product_oauth_flows
FOR EACH ROW
EXECUTE FUNCTION enforce_product_oauth_flow_transition();

CREATE FUNCTION enforce_product_auth_session_transition()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $function$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'product authentication sessions cannot be deleted directly'
            USING ERRCODE = '23514';
    END IF;
    IF OLD.revoked_at IS NOT NULL THEN
        RAISE EXCEPTION 'revoked product authentication sessions are immutable'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.session_digest IS DISTINCT FROM OLD.session_digest
        OR NEW.principal_id IS DISTINCT FROM OLD.principal_id
        OR NEW.csrf_digest IS DISTINCT FROM OLD.csrf_digest
        OR NEW.authenticated_at IS DISTINCT FROM OLD.authenticated_at
        OR NEW.created_at IS DISTINCT FROM OLD.created_at
        OR NEW.absolute_expires_at IS DISTINCT FROM OLD.absolute_expires_at
    THEN
        RAISE EXCEPTION 'product authentication session identity is immutable'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.last_seen_at < OLD.last_seen_at
        OR NEW.idle_expires_at < OLD.idle_expires_at
        OR NEW.revoked_at IS DISTINCT FROM OLD.revoked_at
            AND NEW.revoked_at IS NULL
    THEN
        RAISE EXCEPTION 'product authentication session state cannot move backward'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.last_seen_at IS NOT DISTINCT FROM OLD.last_seen_at
        AND NEW.idle_expires_at IS NOT DISTINCT FROM OLD.idle_expires_at
        AND NEW.revoked_at IS NULL
    THEN
        RAISE EXCEPTION 'product authentication session update made no state transition'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER product_auth_sessions_enforce_transition
BEFORE UPDATE OR DELETE ON product_auth_sessions
FOR EACH ROW
EXECUTE FUNCTION enforce_product_auth_session_transition();

CREATE FUNCTION enforce_product_tenant_transition()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $function$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'product tenants cannot be deleted directly'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id
        OR NEW.created_at IS DISTINCT FROM OLD.created_at
    THEN
        RAISE EXCEPTION 'product tenant identity is immutable'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.lifecycle_state IS DISTINCT FROM OLD.lifecycle_state
        AND NOT (
            (OLD.lifecycle_state = 'provisioning' AND NEW.lifecycle_state IN ('active','suspended','disabled'))
            OR (OLD.lifecycle_state = 'active' AND NEW.lifecycle_state IN ('suspended','disabled'))
            OR (OLD.lifecycle_state = 'suspended' AND NEW.lifecycle_state IN ('active','disabled'))
        )
    THEN
        RAISE EXCEPTION 'product tenant lifecycle transition is invalid'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.updated_at <= OLD.updated_at THEN
        RAISE EXCEPTION 'product tenant updated timestamp must advance'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER product_tenants_enforce_transition
BEFORE UPDATE OR DELETE ON product_tenants
FOR EACH ROW
EXECUTE FUNCTION enforce_product_tenant_transition();

CREATE FUNCTION enforce_automation_installation_transition()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $function$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'automation installations cannot be deleted directly'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.installation_id IS DISTINCT FROM OLD.installation_id
        OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id
        OR NEW.discord_application_id IS DISTINCT FROM OLD.discord_application_id
        OR NEW.discord_guild_id IS DISTINCT FROM OLD.discord_guild_id
        OR NEW.ruleset_key IS DISTINCT FROM OLD.ruleset_key
        OR NEW.created_at IS DISTINCT FROM OLD.created_at
    THEN
        RAISE EXCEPTION 'automation installation identity is immutable'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.current_authority_revision IS DISTINCT FROM OLD.current_authority_revision
        AND NEW.current_authority_revision <> OLD.current_authority_revision + 1
    THEN
        RAISE EXCEPTION 'automation installation authority head must advance by one revision'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.lifecycle_state IS DISTINCT FROM OLD.lifecycle_state
        AND NOT (
            (OLD.lifecycle_state = 'provisioning' AND NEW.lifecycle_state IN ('active','suspended','disabled'))
            OR (OLD.lifecycle_state = 'active' AND NEW.lifecycle_state IN ('suspended','disabled'))
            OR (OLD.lifecycle_state = 'suspended' AND NEW.lifecycle_state IN ('active','disabled'))
        )
    THEN
        RAISE EXCEPTION 'automation installation lifecycle transition is invalid'
            USING ERRCODE = '23514';
    END IF;
    IF OLD.lifecycle_state = 'disabled'
        OR NEW.updated_at <= OLD.updated_at
    THEN
        RAISE EXCEPTION 'disabled automation installations are immutable and timestamps must advance'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER automation_installations_enforce_transition
BEFORE UPDATE OR DELETE ON automation_installations
FOR EACH ROW
EXECUTE FUNCTION enforce_automation_installation_transition();

CREATE FUNCTION enforce_installation_authority_sequence()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $function$
DECLARE
    installation_head BIGINT;
    installation_state TEXT;
    installation_created_at TIMESTAMPTZ;
    latest_revision BIGINT;
BEGIN
    SELECT current_authority_revision, lifecycle_state, created_at
    INTO installation_head, installation_state, installation_created_at
    FROM automation_installations
    WHERE tenant_id = NEW.tenant_id
        AND installation_id = NEW.installation_id
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION 'automation installation authority parent is missing'
            USING ERRCODE = '23503';
    END IF;
    IF installation_state = 'disabled' OR NEW.created_at < installation_created_at THEN
        RAISE EXCEPTION 'automation installation authority creation context is invalid'
            USING ERRCODE = '23514';
    END IF;

    SELECT MAX(revision)
    INTO latest_revision
    FROM automation_installation_authority_versions
    WHERE tenant_id = NEW.tenant_id
        AND installation_id = NEW.installation_id;

    IF latest_revision IS NULL THEN
        IF NEW.revision <> 1 OR installation_head <> 1 THEN
            RAISE EXCEPTION 'initial automation installation authority revision must be one'
                USING ERRCODE = '23514';
        END IF;
    ELSIF NEW.revision <> latest_revision + 1
        OR installation_head NOT IN (latest_revision, NEW.revision)
    THEN
        RAISE EXCEPTION 'automation installation authority revisions must be contiguous'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER installation_authority_enforce_sequence
BEFORE INSERT ON automation_installation_authority_versions
FOR EACH ROW
EXECUTE FUNCTION enforce_installation_authority_sequence();

CREATE FUNCTION assert_installation_authority_head()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $function$
DECLARE
    expected_head BIGINT;
    actual_head BIGINT;
BEGIN
    SELECT current_authority_revision
    INTO expected_head
    FROM automation_installations
    WHERE tenant_id = NEW.tenant_id
        AND installation_id = NEW.installation_id;

    SELECT MAX(revision)
    INTO actual_head
    FROM automation_installation_authority_versions
    WHERE tenant_id = NEW.tenant_id
        AND installation_id = NEW.installation_id;

    IF expected_head IS NULL OR actual_head IS NULL OR expected_head <> actual_head THEN
        RAISE EXCEPTION 'automation installation authority head is inconsistent'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE CONSTRAINT TRIGGER automation_installations_assert_head_insert
AFTER INSERT ON automation_installations
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION assert_installation_authority_head();

CREATE CONSTRAINT TRIGGER automation_installations_assert_head_update
AFTER UPDATE OF current_authority_revision ON automation_installations
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION assert_installation_authority_head();

CREATE CONSTRAINT TRIGGER installation_authority_assert_head
AFTER INSERT ON automation_installation_authority_versions
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION assert_installation_authority_head();

CREATE TRIGGER installation_authority_reject_mutation
BEFORE UPDATE OR DELETE ON automation_installation_authority_versions
FOR EACH ROW
EXECUTE FUNCTION reject_immutable_product_row();

CREATE FUNCTION enforce_authoring_session_transition()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $function$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'authoring sessions cannot be deleted directly'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.session_id IS DISTINCT FROM OLD.session_id
        OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id
        OR NEW.installation_id IS DISTINCT FROM OLD.installation_id
        OR NEW.owner_principal_id IS DISTINCT FROM OLD.owner_principal_id
        OR NEW.created_at IS DISTINCT FROM OLD.created_at
    THEN
        RAISE EXCEPTION 'authoring session ownership and scope are immutable'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.current_generation IS DISTINCT FROM OLD.current_generation
        AND (
            OLD.lifecycle_state <> 'active'
            OR NEW.current_generation <> OLD.current_generation + 1
        )
    THEN
        RAISE EXCEPTION 'active authoring session generation head must advance by one'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.lifecycle_state IS DISTINCT FROM OLD.lifecycle_state
        AND NOT (
            (OLD.lifecycle_state = 'active' AND NEW.lifecycle_state IN ('closed','archived'))
            OR (OLD.lifecycle_state = 'closed' AND NEW.lifecycle_state IN ('active','archived'))
        )
    THEN
        RAISE EXCEPTION 'authoring session lifecycle transition is invalid'
            USING ERRCODE = '23514';
    END IF;
    IF OLD.lifecycle_state = 'archived' OR NEW.updated_at <= OLD.updated_at THEN
        RAISE EXCEPTION 'archived authoring sessions are immutable and timestamps must advance'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER authoring_sessions_enforce_transition
BEFORE UPDATE OR DELETE ON authoring_sessions
FOR EACH ROW
EXECUTE FUNCTION enforce_authoring_session_transition();

CREATE FUNCTION enforce_authoring_generation_sequence()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $function$
DECLARE
    session_head BIGINT;
    session_state TEXT;
    session_created_at TIMESTAMPTZ;
    latest_generation BIGINT;
BEGIN
    SELECT current_generation, lifecycle_state, created_at
    INTO session_head, session_state, session_created_at
    FROM authoring_sessions
    WHERE tenant_id = NEW.tenant_id
        AND installation_id = NEW.installation_id
        AND session_id = NEW.session_id
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION 'authoring session generation parent is missing'
            USING ERRCODE = '23503';
    END IF;
    IF session_state <> 'active' OR NEW.created_at < session_created_at THEN
        RAISE EXCEPTION 'authoring session generation creation context is invalid'
            USING ERRCODE = '23514';
    END IF;

    SELECT MAX(generation)
    INTO latest_generation
    FROM authoring_session_generations
    WHERE tenant_id = NEW.tenant_id
        AND installation_id = NEW.installation_id
        AND session_id = NEW.session_id;

    IF latest_generation IS NULL THEN
        IF NEW.generation <> 1 OR session_head <> 1 THEN
            RAISE EXCEPTION 'initial authoring session generation must be one'
                USING ERRCODE = '23514';
        END IF;
    ELSIF NEW.generation <> latest_generation + 1
        OR session_head NOT IN (latest_generation, NEW.generation)
    THEN
        RAISE EXCEPTION 'authoring session generations must be contiguous'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER authoring_generations_enforce_sequence
BEFORE INSERT ON authoring_session_generations
FOR EACH ROW
EXECUTE FUNCTION enforce_authoring_generation_sequence();

CREATE FUNCTION assert_authoring_session_generation_head()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $function$
DECLARE
    expected_head BIGINT;
    actual_head BIGINT;
BEGIN
    SELECT current_generation
    INTO expected_head
    FROM authoring_sessions
    WHERE tenant_id = NEW.tenant_id
        AND installation_id = NEW.installation_id
        AND session_id = NEW.session_id;

    SELECT MAX(generation)
    INTO actual_head
    FROM authoring_session_generations
    WHERE tenant_id = NEW.tenant_id
        AND installation_id = NEW.installation_id
        AND session_id = NEW.session_id;

    IF expected_head IS NULL OR actual_head IS NULL OR expected_head <> actual_head THEN
        RAISE EXCEPTION 'authoring session generation head is inconsistent'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE CONSTRAINT TRIGGER authoring_sessions_assert_head_insert
AFTER INSERT ON authoring_sessions
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION assert_authoring_session_generation_head();

CREATE CONSTRAINT TRIGGER authoring_sessions_assert_head_update
AFTER UPDATE OF current_generation ON authoring_sessions
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION assert_authoring_session_generation_head();

CREATE CONSTRAINT TRIGGER authoring_generations_assert_head
AFTER INSERT ON authoring_session_generations
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION assert_authoring_session_generation_head();

CREATE TRIGGER authoring_generations_reject_mutation
BEFORE UPDATE OR DELETE ON authoring_session_generations
FOR EACH ROW
EXECUTE FUNCTION reject_immutable_product_row();

CREATE TRIGGER product_action_receipts_reject_mutation
BEFORE UPDATE OR DELETE ON product_action_receipts
FOR EACH ROW
EXECUTE FUNCTION reject_immutable_product_row();

CREATE TRIGGER product_audit_events_reject_mutation
BEFORE UPDATE OR DELETE ON product_audit_events
FOR EACH ROW
EXECUTE FUNCTION reject_immutable_product_row();
