LOCK TABLE public.automation_installation_authority_versions
IN SHARE ROW EXCLUSIVE MODE;

LOCK TABLE public.activation_requests
IN SHARE ROW EXCLUSIVE MODE;

LOCK TABLE public.activation_request_approvals
IN SHARE ROW EXCLUSIVE MODE;

DO $solo_product_approval_preflight$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM public.automation_installation_authority_versions AS authority
        WHERE authority.required_approvals <> 1
    ) THEN
        RAISE EXCEPTION 'installation authority contains a non-solo approval policy'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM public.activation_requests AS activation
        WHERE activation.authority_kind = 'product_authoring'
            AND activation.required_approvals <> 1
    ) THEN
        RAISE EXCEPTION 'product activation contains a non-solo approval policy'
            USING ERRCODE = '55000';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM public.activation_requests AS activation
        JOIN public.activation_request_approvals AS approval
            ON approval.request_id = activation.id
        WHERE activation.authority_kind = 'product_authoring'
        GROUP BY activation.id
        HAVING pg_catalog.count(*) > 1
    ) THEN
        RAISE EXCEPTION 'product activation contains multiple approvals'
            USING ERRCODE = '55000';
    END IF;
END;
$solo_product_approval_preflight$;

ALTER TABLE public.automation_installation_authority_versions
ADD CONSTRAINT installation_authority_single_approval
CHECK (required_approvals = 1) NOT VALID;

ALTER TABLE public.automation_installation_authority_versions
VALIDATE CONSTRAINT installation_authority_single_approval;

ALTER TABLE public.activation_requests
ADD CONSTRAINT activation_requests_product_single_approval
CHECK (
    authority_kind <> 'product_authoring'
    OR required_approvals = 1
) NOT VALID;

ALTER TABLE public.activation_requests
VALIDATE CONSTRAINT activation_requests_product_single_approval;

DO $solo_product_approval_functions$
DECLARE
    approval_identity TEXT :=
        'public.starring_product_approve_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text)';
    repair_identity TEXT :=
        'public.starring_product_promotion_repair_link_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text,bytea,jsonb,text,text,text[],text[],text[],text,text,text,text)';
    approval_oid OID;
    repair_oid OID;
    approval_gate TEXT := E'    IF activation_row.requester_id = expected_acting_user_id THEN\n        RETURN QUERY SELECT ''self_approval_forbidden'', NULL::BIGINT, NULL::TEXT, FALSE,\n            NULL::TEXT;\n        RETURN;\n    END IF;\n';
    repair_gate TEXT :=
        E'                    OR approval.approver_id = activation_row.requester_id\n';
    function_definition TEXT;
    metadata_before JSONB;
    metadata_after JSONB;
BEGIN
    approval_oid := pg_catalog.to_regprocedure(approval_identity);
    repair_oid := pg_catalog.to_regprocedure(repair_identity);
    IF approval_oid IS NULL OR repair_oid IS NULL THEN
        RAISE EXCEPTION 'solo approval function precondition failed'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.jsonb_build_object(
        'oid', function_row.oid::TEXT,
        'owner', function_row.proowner::TEXT,
        'acl', pg_catalog.to_jsonb(function_row.proacl),
        'volatile', function_row.provolatile,
        'strict', function_row.proisstrict,
        'security_definer', function_row.prosecdef,
        'parallel', function_row.proparallel,
        'rows', function_row.prorows,
        'config', pg_catalog.to_jsonb(function_row.proconfig)
    )
    INTO metadata_before
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = approval_oid;

    function_definition := pg_catalog.pg_get_functiondef(approval_oid);
    IF pg_catalog.char_length(function_definition)
            - pg_catalog.char_length(pg_catalog.replace(
                function_definition,
                approval_gate,
                ''
            ))
        <> pg_catalog.char_length(approval_gate)
    THEN
        RAISE EXCEPTION 'solo approval gate replacement precondition failed'
            USING ERRCODE = '55000';
    END IF;
    EXECUTE pg_catalog.replace(function_definition, approval_gate, '');

    SELECT pg_catalog.jsonb_build_object(
        'oid', function_row.oid::TEXT,
        'owner', function_row.proowner::TEXT,
        'acl', pg_catalog.to_jsonb(function_row.proacl),
        'volatile', function_row.provolatile,
        'strict', function_row.proisstrict,
        'security_definer', function_row.prosecdef,
        'parallel', function_row.proparallel,
        'rows', function_row.prorows,
        'config', pg_catalog.to_jsonb(function_row.proconfig)
    )
    INTO metadata_after
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = approval_oid;
    IF metadata_after IS DISTINCT FROM metadata_before
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(approval_oid),
            'self_approval_forbidden'
        ) <> 0
    THEN
        RAISE EXCEPTION 'solo approval function replacement failed'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.jsonb_build_object(
        'oid', function_row.oid::TEXT,
        'owner', function_row.proowner::TEXT,
        'acl', pg_catalog.to_jsonb(function_row.proacl),
        'volatile', function_row.provolatile,
        'strict', function_row.proisstrict,
        'security_definer', function_row.prosecdef,
        'parallel', function_row.proparallel,
        'rows', function_row.prorows,
        'config', pg_catalog.to_jsonb(function_row.proconfig)
    )
    INTO metadata_before
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = repair_oid;

    function_definition := pg_catalog.pg_get_functiondef(repair_oid);
    IF pg_catalog.char_length(function_definition)
            - pg_catalog.char_length(pg_catalog.replace(
                function_definition,
                repair_gate,
                ''
            ))
        <> pg_catalog.char_length(repair_gate)
    THEN
        RAISE EXCEPTION 'solo approval repair replacement precondition failed'
            USING ERRCODE = '55000';
    END IF;
    EXECUTE pg_catalog.replace(function_definition, repair_gate, '');

    SELECT pg_catalog.jsonb_build_object(
        'oid', function_row.oid::TEXT,
        'owner', function_row.proowner::TEXT,
        'acl', pg_catalog.to_jsonb(function_row.proacl),
        'volatile', function_row.provolatile,
        'strict', function_row.proisstrict,
        'security_definer', function_row.prosecdef,
        'parallel', function_row.proparallel,
        'rows', function_row.prorows,
        'config', pg_catalog.to_jsonb(function_row.proconfig)
    )
    INTO metadata_after
    FROM pg_catalog.pg_proc AS function_row
    WHERE function_row.oid = repair_oid;
    IF metadata_after IS DISTINCT FROM metadata_before
        OR pg_catalog.strpos(
            pg_catalog.pg_get_functiondef(repair_oid),
            'approval.approver_id = activation_row.requester_id'
        ) <> 0
    THEN
        RAISE EXCEPTION 'solo approval repair function replacement failed'
            USING ERRCODE = '55000';
    END IF;
END;
$solo_product_approval_functions$;
