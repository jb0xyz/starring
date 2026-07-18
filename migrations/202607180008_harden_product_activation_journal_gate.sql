UPDATE authoring_promotions
SET record = jsonb_set(
    record #- '{stage,activation,request_state_at_link}',
    '{stage,activation,request_state_at_journal}',
    record #> '{stage,activation,request_state_at_link}',
    TRUE
)
WHERE record #> '{stage,activation,request_state_at_journal}' IS NULL
    AND record #> '{stage,activation,request_state_at_link}' IS NOT NULL;

CREATE OR REPLACE FUNCTION enforce_product_activation_journal_link()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $function$
DECLARE
    journal_stage TEXT;
    journal_request_digest TEXT;
    journal_record JSONB;
    verify_link BOOLEAN := FALSE;
BEGIN
    IF TG_OP = 'UPDATE' THEN
        IF OLD.authority_kind IS DISTINCT FROM NEW.authority_kind THEN
            RAISE EXCEPTION 'activation request authority kind is immutable'
                USING ERRCODE = '23514';
        END IF;

        IF OLD.authority_kind = 'product_authoring' THEN
            IF OLD.id IS DISTINCT FROM NEW.id
                OR OLD.guild_id IS DISTINCT FROM NEW.guild_id
                OR OLD.ruleset_key IS DISTINCT FROM NEW.ruleset_key
                OR OLD.target_version IS DISTINCT FROM NEW.target_version
                OR OLD.target_content_hash IS DISTINCT FROM NEW.target_content_hash
                OR OLD.requester_id IS DISTINCT FROM NEW.requester_id
                OR OLD.required_approvals IS DISTINCT FROM NEW.required_approvals
                OR OLD.created_at IS DISTINCT FROM NEW.created_at
                OR OLD.expires_at IS DISTINCT FROM NEW.expires_at
                OR OLD.observed_active_version IS DISTINCT FROM NEW.observed_active_version
                OR OLD.observed_active_hash IS DISTINCT FROM NEW.observed_active_hash
                OR OLD.approval_context IS DISTINCT FROM NEW.approval_context
                OR OLD.promotion_id IS DISTINCT FROM NEW.promotion_id
                OR OLD.promotion_request_digest IS DISTINCT FROM NEW.promotion_request_digest
                OR OLD.approval_payload_digest IS DISTINCT FROM NEW.approval_payload_digest
                OR OLD.approval_context_digest IS DISTINCT FROM NEW.approval_context_digest
            THEN
                RAISE EXCEPTION 'product activation authority identity is immutable'
                    USING ERRCODE = '23514';
            END IF;

            IF OLD.link_state_name = 'unlinked' AND NEW.link_state_name = 'linked' THEN
                verify_link := TRUE;
            ELSIF OLD.link_state_name IS DISTINCT FROM NEW.link_state_name
                OR OLD.link_state IS DISTINCT FROM NEW.link_state
                OR OLD.linked_at IS DISTINCT FROM NEW.linked_at
            THEN
                RAISE EXCEPTION 'product activation link identity is immutable outside the guarded link transition'
                    USING ERRCODE = '23514';
            END IF;
        END IF;
    ELSIF NEW.authority_kind = 'product_authoring' AND NEW.link_state_name = 'linked' THEN
        verify_link := TRUE;
    END IF;

    IF verify_link THEN
        SELECT stage, request_digest, record
        INTO journal_stage, journal_request_digest, journal_record
        FROM authoring_promotions
        WHERE id = NEW.promotion_id
        FOR SHARE;

        IF NEW.state <> 'pending'
            OR NEW.link_state #>> '{state}' IS DISTINCT FROM 'linked'
            OR (NEW.link_state #>> '{linked_at}')::TIMESTAMPTZ IS DISTINCT FROM NEW.linked_at
            OR NOT FOUND
            OR journal_stage <> 'activation_pending'
            OR journal_request_digest IS DISTINCT FROM NEW.promotion_request_digest
            OR journal_record #>> '{stage,state}' IS DISTINCT FROM 'activation_pending'
            OR journal_record #>> '{stage,activation,request_id}' IS DISTINCT FROM NEW.id
            OR journal_record #>> '{stage,activation,target,guild_id}' IS DISTINCT FROM NEW.guild_id
            OR journal_record #>> '{stage,activation,target,ruleset_key}' IS DISTINCT FROM NEW.ruleset_key
            OR (journal_record #>> '{stage,activation,target,version}')::BIGINT IS DISTINCT FROM NEW.target_version
            OR journal_record #>> '{stage,activation,target,content_hash}' IS DISTINCT FROM NEW.target_content_hash
            OR journal_record #>> '{stage,activation,requester}' IS DISTINCT FROM NEW.requester_id
            OR (journal_record #>> '{stage,activation,required_approvals}')::INTEGER IS DISTINCT FROM NEW.required_approvals
            OR (journal_record #>> '{stage,activation,created_at}')::TIMESTAMPTZ IS DISTINCT FROM NEW.created_at
            OR (journal_record #>> '{stage,activation,expires_at}')::TIMESTAMPTZ IS DISTINCT FROM NEW.expires_at
            OR COALESCE(
                journal_record #>> '{stage,activation,request_state_at_journal}',
                journal_record #>> '{stage,activation,request_state_at_link}'
            ) IS DISTINCT FROM 'pending'
            OR journal_record #> '{stage,activation,approval_context}' IS DISTINCT FROM (NEW.approval_context -> 'context')
            OR journal_record #>> '{stage,activation,approval_context,promotion_id}' IS DISTINCT FROM NEW.promotion_id
            OR journal_record #>> '{stage,activation,approval_context,promotion_request_digest}' IS DISTINCT FROM NEW.promotion_request_digest
            OR journal_record #>> '{stage,activation,approval_context,approval_payload_digest}' IS DISTINCT FROM NEW.approval_payload_digest
            OR journal_record #>> '{stage,activation,approval_context,approval_context_digest}' IS DISTINCT FROM NEW.approval_context_digest
        THEN
            RAISE EXCEPTION 'product activation link is not authorized by an exact activation-pending promotion journal'
                USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$function$;

DROP TRIGGER activation_requests_enforce_product_journal_link ON activation_requests;

CREATE TRIGGER activation_requests_enforce_product_journal_link
BEFORE INSERT OR UPDATE ON activation_requests
FOR EACH ROW
EXECUTE FUNCTION enforce_product_activation_journal_link();
