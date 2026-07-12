CREATE TABLE activation_requests (
    id TEXT PRIMARY KEY,
    guild_id TEXT NOT NULL,
    ruleset_key TEXT NOT NULL,
    target_version BIGINT NOT NULL,
    target_content_hash TEXT NOT NULL,
    requester_id TEXT NOT NULL,
    required_approvals INT NOT NULL,
    state TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    apply_attempt_id TEXT,
    apply_attempt_no BIGINT NOT NULL DEFAULT 0,
    apply_lease_until TIMESTAMPTZ,
    last_apply_error JSONB,
    observed_active_version BIGINT,
    observed_active_hash TEXT,
    applied_at TIMESTAMPTZ,
    applied_by TEXT,
    completion_kind TEXT,
    activation_notices JSONB,
    rejected_at TIMESTAMPTZ,
    rejected_by TEXT,
    rejection_reason TEXT,
    CONSTRAINT activation_requests_id_format CHECK (id ~ '^[A-Za-z0-9_-]{1,64}$'),
    CONSTRAINT activation_requests_target_version_valid CHECK (target_version BETWEEN 1 AND 4294967295),
    CONSTRAINT activation_requests_target_hash_valid CHECK (target_content_hash ~ '^[0-9a-f]{64}$'),
    CONSTRAINT activation_requests_required_approvals_valid CHECK (required_approvals >= 1),
    CONSTRAINT activation_requests_attempt_no_valid CHECK (apply_attempt_no >= 0),
    CONSTRAINT activation_requests_expiry_valid CHECK (expires_at > created_at),
    CONSTRAINT activation_requests_state_valid CHECK (state IN ('pending','approved','applying','applied','rejected','expired')),
    CONSTRAINT activation_requests_attempt_fields_valid CHECK (
        (state = 'applying' AND apply_attempt_id IS NOT NULL AND apply_lease_until IS NOT NULL)
        OR
        (state <> 'applying' AND apply_attempt_id IS NULL AND apply_lease_until IS NULL)
    ),
    CONSTRAINT activation_requests_applied_fields_valid CHECK (
        state <> 'applied'
        OR (applied_at IS NOT NULL AND applied_by IS NOT NULL AND completion_kind IS NOT NULL)
    ),
    CONSTRAINT activation_requests_rejected_fields_valid CHECK (
        state <> 'rejected'
        OR (rejected_at IS NOT NULL AND rejected_by IS NOT NULL)
    ),
    CONSTRAINT activation_requests_completion_kind_valid CHECK (
        completion_kind IS NULL OR completion_kind IN ('activated','already_active','crash_recovered')
    ),
    CONSTRAINT activation_requests_observed_active_valid CHECK (
        (observed_active_version IS NULL AND observed_active_hash IS NULL)
        OR
        (observed_active_version IS NOT NULL AND observed_active_hash IS NOT NULL
         AND observed_active_version BETWEEN 1 AND 4294967295
         AND observed_active_hash ~ '^[0-9a-f]{64}$')
    ),
    CONSTRAINT activation_requests_notices_valid CHECK (
        activation_notices IS NULL OR jsonb_typeof(activation_notices) = 'array'
    ),
    CONSTRAINT activation_requests_error_valid CHECK (
        last_apply_error IS NULL OR jsonb_typeof(last_apply_error) = 'object'
    )
);

CREATE UNIQUE INDEX activation_requests_one_applying_per_ruleset
ON activation_requests (guild_id, ruleset_key)
WHERE state = 'applying';

CREATE TABLE activation_request_approvals (
    request_id TEXT NOT NULL REFERENCES activation_requests(id) ON DELETE CASCADE,
    approver_id TEXT NOT NULL,
    approved_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (request_id, approver_id)
);
