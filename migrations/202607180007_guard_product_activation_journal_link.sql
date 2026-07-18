CREATE FUNCTION enforce_product_activation_journal_link()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $function$
DECLARE
    journal_stage TEXT;
    journal_request_digest TEXT;
    journal_record JSONB;
BEGIN
    IF NEW.authority_kind = 'product_authoring'
        AND OLD.link_state_name = 'unlinked'
        AND NEW.link_state_name = 'linked'
    THEN
        SELECT stage, request_digest, record
        INTO journal_stage, journal_request_digest, journal_record
        FROM authoring_promotions
        WHERE id = NEW.promotion_id
        FOR SHARE;

        IF NOT FOUND
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
            OR journal_record #>> '{stage,activation,request_state_at_journal}' IS DISTINCT FROM 'pending'
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

CREATE TRIGGER activation_requests_enforce_product_journal_link
BEFORE UPDATE ON activation_requests
FOR EACH ROW
EXECUTE FUNCTION enforce_product_activation_journal_link();
