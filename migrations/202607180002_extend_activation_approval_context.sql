ALTER TABLE activation_requests
ADD COLUMN authority_kind TEXT NOT NULL DEFAULT 'legacy_manual',
ADD COLUMN link_state_name TEXT NOT NULL DEFAULT 'not_required',
ADD COLUMN approval_context JSONB NOT NULL DEFAULT '{"authority":"legacy_manual"}'::JSONB,
ADD COLUMN link_state JSONB NOT NULL DEFAULT '{"state":"not_required"}'::JSONB,
ADD COLUMN promotion_id TEXT,
ADD COLUMN promotion_request_digest TEXT,
ADD COLUMN approval_payload_digest TEXT,
ADD COLUMN approval_context_digest TEXT,
ADD COLUMN linked_at TIMESTAMPTZ;

ALTER TABLE activation_request_approvals
ADD COLUMN approval_payload_digest TEXT;

ALTER TABLE activation_requests
ADD CONSTRAINT activation_requests_authority_kind_valid
CHECK (authority_kind IN ('legacy_manual','product_authoring')),
ADD CONSTRAINT activation_requests_link_state_name_valid
CHECK (link_state_name IN ('not_required','unlinked','linked')),
ADD CONSTRAINT activation_requests_approval_context_object
CHECK (jsonb_typeof(approval_context) = 'object'),
ADD CONSTRAINT activation_requests_link_state_object
CHECK (jsonb_typeof(link_state) = 'object'),
ADD CONSTRAINT activation_requests_context_shadow_valid
CHECK (
    ((approval_context ->> 'authority') = authority_kind) IS TRUE
    AND ((link_state ->> 'state') = link_state_name) IS TRUE
),
ADD CONSTRAINT activation_requests_product_context_valid
CHECK (
    (
        authority_kind = 'legacy_manual'
        AND link_state_name = 'not_required'
        AND promotion_id IS NULL
        AND promotion_request_digest IS NULL
        AND approval_payload_digest IS NULL
        AND approval_context_digest IS NULL
        AND linked_at IS NULL
    )
    OR
    (
        authority_kind = 'product_authoring'
        AND link_state_name IN ('unlinked','linked')
        AND promotion_id ~ '^[0-9a-f]{64}$'
        AND promotion_request_digest ~ '^[0-9a-f]{64}$'
        AND approval_payload_digest ~ '^[0-9a-f]{64}$'
        AND approval_context_digest ~ '^[0-9a-f]{64}$'
        AND ((approval_context -> 'context' ->> 'promotion_id') = promotion_id) IS TRUE
        AND ((approval_context -> 'context' ->> 'promotion_request_digest') = promotion_request_digest) IS TRUE
        AND ((approval_context -> 'context' ->> 'approval_payload_digest') = approval_payload_digest) IS TRUE
        AND ((approval_context -> 'context' ->> 'approval_context_digest') = approval_context_digest) IS TRUE
        AND (
            (link_state_name = 'unlinked' AND linked_at IS NULL AND state IN ('pending','expired'))
            OR
            (link_state_name = 'linked' AND linked_at IS NOT NULL)
        )
    )
),
ADD CONSTRAINT activation_requests_link_timestamp_valid
CHECK (linked_at IS NULL OR (linked_at >= created_at AND linked_at < expires_at));

ALTER TABLE activation_request_approvals
ADD CONSTRAINT activation_request_approvals_payload_digest_valid
CHECK (
    approval_payload_digest IS NULL
    OR approval_payload_digest ~ '^[0-9a-f]{64}$'
);

CREATE UNIQUE INDEX activation_requests_one_product_request_per_promotion
ON activation_requests (promotion_id)
WHERE authority_kind = 'product_authoring';

CREATE FUNCTION enforce_activation_approval_payload_binding()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $function$
DECLARE
    parent_authority TEXT;
    parent_link_state TEXT;
    expected_digest TEXT;
BEGIN
    SELECT authority_kind, link_state_name, approval_payload_digest
    INTO parent_authority, parent_link_state, expected_digest
    FROM activation_requests
    WHERE id = NEW.request_id
    FOR KEY SHARE;

    IF parent_authority = 'legacy_manual' AND NEW.approval_payload_digest IS NOT NULL THEN
        RAISE EXCEPTION 'legacy activation approval cannot carry a payload digest'
            USING ERRCODE = '23514';
    END IF;
    IF parent_authority = 'product_authoring'
        AND (parent_link_state <> 'linked' OR NEW.approval_payload_digest IS DISTINCT FROM expected_digest)
    THEN
        RAISE EXCEPTION 'product activation approval payload is not exactly bound'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER activation_request_approvals_enforce_payload_binding
BEFORE INSERT OR UPDATE ON activation_request_approvals
FOR EACH ROW
EXECUTE FUNCTION enforce_activation_approval_payload_binding();
