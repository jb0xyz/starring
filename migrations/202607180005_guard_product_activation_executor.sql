CREATE FUNCTION enforce_product_activation_executor()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $function$
BEGIN
    IF NEW.authority_kind = 'product_authoring'
        AND NEW.state = 'applying'
        AND (
            OLD.state <> 'applying'
            OR OLD.apply_attempt_id IS DISTINCT FROM NEW.apply_attempt_id
        )
        AND current_setting('starring.product_approval_context_digest', TRUE)
            IS DISTINCT FROM NEW.approval_context_digest
    THEN
        RAISE EXCEPTION 'product activation executor is not bound to the approval context'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER activation_requests_enforce_product_executor
BEFORE UPDATE ON activation_requests
FOR EACH ROW
EXECUTE FUNCTION enforce_product_activation_executor();
