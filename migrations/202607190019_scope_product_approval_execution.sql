DO $preflight$
DECLARE
    relation_count BIGINT;
    table_count BIGINT;
    rls_disabled_count BIGINT;
    owner_count BIGINT;
    common_owner OID;
    common_owner_name NAME;
    identity_count BIGINT;
    expected_signature TEXT;
    function_oid OID;
BEGIN
    SELECT pg_catalog.count(relation.oid),
        pg_catalog.count(relation.oid) FILTER (WHERE relation.relkind = 'r'),
        pg_catalog.count(relation.oid) FILTER (
            WHERE NOT relation.relrowsecurity AND NOT relation.relforcerowsecurity
        ),
        pg_catalog.count(DISTINCT relation.relowner),
        pg_catalog.min(relation.relowner::BIGINT)::OID
    INTO relation_count, table_count, rls_disabled_count, owner_count, common_owner
    FROM (
        VALUES
            (pg_catalog.to_regclass('public.activation_requests')),
            (pg_catalog.to_regclass('public.authoring_promotions')),
            (pg_catalog.to_regclass('public.product_tenants')),
            (pg_catalog.to_regclass('public.automation_installations')),
            (pg_catalog.to_regclass('public.automation_installation_authority_versions')),
            (pg_catalog.to_regclass('public.product_principals')),
            (pg_catalog.to_regclass('public.product_auth_sessions')),
            (pg_catalog.to_regclass('public.product_action_receipts')),
            (pg_catalog.to_regclass('public.product_action_receipt_idempotency_aliases')),
            (pg_catalog.to_regclass('public.product_audit_events')),
            (pg_catalog.to_regclass('public.product_action_receipt_audit_evidence')),
            (pg_catalog.to_regclass('public.activation_request_approvals')),
            (pg_catalog.to_regclass('public.product_control_plane_identity'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid;

    IF relation_count <> 13
        OR table_count <> 13
        OR rls_disabled_count <> 13
        OR owner_count <> 1
        OR common_owner IS NULL
    THEN
        RAISE EXCEPTION 'product approval relations require one non-RLS owner'
            USING ERRCODE = '55000';
    END IF;

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL THEN
        RAISE EXCEPTION 'product approval relation owner is unavailable'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO identity_count
    FROM public.product_control_plane_identity AS identity
    WHERE identity.singleton
        AND identity.database_identity IS NOT NULL
        AND identity.database_identity
            <> '00000000-0000-0000-0000-000000000000'::UUID
        AND identity.created_at IS NOT NULL;
    IF identity_count <> 1 THEN
        RAISE EXCEPTION 'product control plane identity is invalid'
            USING ERRCODE = '55000';
    END IF;

    FOR expected_signature IN
        SELECT expected.signature
        FROM (
            VALUES
                ('public.starring_product_approve_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text)'),
                ('public.starring_product_approval_keyring_coverage_v1(text[],text[])')
        ) AS expected(signature)
    LOOP
        function_oid := pg_catalog.to_regprocedure(expected_signature);
        IF function_oid IS NULL
            OR (SELECT function_row.proowner
                FROM pg_catalog.pg_proc AS function_row
                WHERE function_row.oid = function_oid) <> common_owner
        THEN
            RAISE EXCEPTION 'product approval function owner is invalid'
                USING ERRCODE = '55000';
        END IF;
    END LOOP;
END;
$preflight$;

CREATE FUNCTION public.starring_product_decision_reader_database_identity_v1()
RETURNS TEXT
LANGUAGE sql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
    SELECT identity.database_identity::TEXT
    FROM public.product_control_plane_identity AS identity
    WHERE identity.singleton;
$function$;

CREATE FUNCTION public.starring_product_approval_executor_database_identity_v1()
RETURNS TEXT
LANGUAGE sql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
    SELECT identity.database_identity::TEXT
    FROM public.product_control_plane_identity AS identity
    WHERE identity.singleton;
$function$;

CREATE FUNCTION public.starring_product_apply_executor_database_identity_v1()
RETURNS TEXT
LANGUAGE sql
VOLATILE
STRICT
PARALLEL UNSAFE
SECURITY DEFINER
SET search_path = pg_catalog
AS $function$
    SELECT identity.database_identity::TEXT
    FROM public.product_control_plane_identity AS identity
    WHERE identity.singleton;
$function$;

ALTER FUNCTION public.starring_product_approve_v1(
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
    TEXT
) VOLATILE;
ALTER FUNCTION public.starring_product_approve_v1(
    TEXT, TEXT, TEXT, BIGINT, TEXT, TEXT, BYTEA, BYTEA, TEXT, TEXT, TEXT,
    TEXT, BIGINT, TEXT, TEXT, TIMESTAMPTZ, TIMESTAMPTZ, TEXT, BOOLEAN,
    TEXT, TEXT, TEXT[], TEXT[], TEXT[], TEXT, TEXT, TEXT, TEXT
) STRICT;
ALTER FUNCTION public.starring_product_approve_v1(
    TEXT, TEXT, TEXT, BIGINT, TEXT, TEXT, BYTEA, BYTEA, TEXT, TEXT, TEXT,
    TEXT, BIGINT, TEXT, TEXT, TIMESTAMPTZ, TIMESTAMPTZ, TEXT, BOOLEAN,
    TEXT, TEXT, TEXT[], TEXT[], TEXT[], TEXT, TEXT, TEXT, TEXT
) PARALLEL UNSAFE;
ALTER FUNCTION public.starring_product_approve_v1(
    TEXT, TEXT, TEXT, BIGINT, TEXT, TEXT, BYTEA, BYTEA, TEXT, TEXT, TEXT,
    TEXT, BIGINT, TEXT, TEXT, TIMESTAMPTZ, TIMESTAMPTZ, TEXT, BOOLEAN,
    TEXT, TEXT, TEXT[], TEXT[], TEXT[], TEXT, TEXT, TEXT, TEXT
) SECURITY DEFINER;
ALTER FUNCTION public.starring_product_approve_v1(
    TEXT, TEXT, TEXT, BIGINT, TEXT, TEXT, BYTEA, BYTEA, TEXT, TEXT, TEXT,
    TEXT, BIGINT, TEXT, TEXT, TIMESTAMPTZ, TIMESTAMPTZ, TEXT, BOOLEAN,
    TEXT, TEXT, TEXT[], TEXT[], TEXT[], TEXT, TEXT, TEXT, TEXT
) ROWS 1;
ALTER FUNCTION public.starring_product_approve_v1(
    TEXT, TEXT, TEXT, BIGINT, TEXT, TEXT, BYTEA, BYTEA, TEXT, TEXT, TEXT,
    TEXT, BIGINT, TEXT, TEXT, TIMESTAMPTZ, TIMESTAMPTZ, TEXT, BOOLEAN,
    TEXT, TEXT, TEXT[], TEXT[], TEXT[], TEXT, TEXT, TEXT, TEXT
) RESET ALL;
ALTER FUNCTION public.starring_product_approve_v1(
    TEXT, TEXT, TEXT, BIGINT, TEXT, TEXT, BYTEA, BYTEA, TEXT, TEXT, TEXT,
    TEXT, BIGINT, TEXT, TEXT, TIMESTAMPTZ, TIMESTAMPTZ, TEXT, BOOLEAN,
    TEXT, TEXT, TEXT[], TEXT[], TEXT[], TEXT, TEXT, TEXT, TEXT
) SET search_path = pg_catalog;

ALTER FUNCTION public.starring_product_approval_keyring_coverage_v1(TEXT[], TEXT[])
VOLATILE;
ALTER FUNCTION public.starring_product_approval_keyring_coverage_v1(TEXT[], TEXT[])
STRICT;
ALTER FUNCTION public.starring_product_approval_keyring_coverage_v1(TEXT[], TEXT[])
PARALLEL UNSAFE;
ALTER FUNCTION public.starring_product_approval_keyring_coverage_v1(TEXT[], TEXT[])
SECURITY DEFINER;
ALTER FUNCTION public.starring_product_approval_keyring_coverage_v1(TEXT[], TEXT[])
ROWS 1;
ALTER FUNCTION public.starring_product_approval_keyring_coverage_v1(TEXT[], TEXT[])
RESET ALL;
ALTER FUNCTION public.starring_product_approval_keyring_coverage_v1(TEXT[], TEXT[])
SET search_path = pg_catalog;

REVOKE ALL ON FUNCTION public.starring_product_decision_reader_database_identity_v1()
FROM PUBLIC;
REVOKE ALL ON FUNCTION public.starring_product_approval_executor_database_identity_v1()
FROM PUBLIC;
REVOKE ALL ON FUNCTION public.starring_product_apply_executor_database_identity_v1()
FROM PUBLIC;
REVOKE ALL ON FUNCTION public.starring_product_approve_v1(
    TEXT, TEXT, TEXT, BIGINT, TEXT, TEXT, BYTEA, BYTEA, TEXT, TEXT, TEXT,
    TEXT, BIGINT, TEXT, TEXT, TIMESTAMPTZ, TIMESTAMPTZ, TEXT, BOOLEAN,
    TEXT, TEXT, TEXT[], TEXT[], TEXT[], TEXT, TEXT, TEXT, TEXT
) FROM PUBLIC;
REVOKE ALL ON FUNCTION public.starring_product_approval_keyring_coverage_v1(
    TEXT[], TEXT[]
) FROM PUBLIC;

DO $ownership$
DECLARE
    common_owner OID;
    common_owner_name NAME;
    expected_signature TEXT;
    function_oid OID;
    unexpected_grantee OID;
    unexpected_grantee_name NAME;
BEGIN
    SELECT pg_catalog.min(relation.relowner::BIGINT)::OID
    INTO common_owner
    FROM (
        VALUES
            (pg_catalog.to_regclass('public.activation_requests')),
            (pg_catalog.to_regclass('public.authoring_promotions')),
            (pg_catalog.to_regclass('public.product_tenants')),
            (pg_catalog.to_regclass('public.automation_installations')),
            (pg_catalog.to_regclass('public.automation_installation_authority_versions')),
            (pg_catalog.to_regclass('public.product_principals')),
            (pg_catalog.to_regclass('public.product_auth_sessions')),
            (pg_catalog.to_regclass('public.product_action_receipts')),
            (pg_catalog.to_regclass('public.product_action_receipt_idempotency_aliases')),
            (pg_catalog.to_regclass('public.product_audit_events')),
            (pg_catalog.to_regclass('public.product_action_receipt_audit_evidence')),
            (pg_catalog.to_regclass('public.activation_request_approvals')),
            (pg_catalog.to_regclass('public.product_control_plane_identity'))
    ) AS expected(relation_oid)
    INNER JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid;
    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner IS NULL OR common_owner_name IS NULL THEN
        RAISE EXCEPTION 'product approval relation owner is unavailable'
            USING ERRCODE = '55000';
    END IF;

    FOR expected_signature IN
        SELECT expected.signature
        FROM (
            VALUES
                ('public.starring_product_decision_reader_database_identity_v1()'),
                ('public.starring_product_approval_executor_database_identity_v1()'),
                ('public.starring_product_apply_executor_database_identity_v1()'),
                ('public.starring_product_approve_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text)'),
                ('public.starring_product_approval_keyring_coverage_v1(text[],text[])')
        ) AS expected(signature)
    LOOP
        function_oid := pg_catalog.to_regprocedure(expected_signature);
        IF function_oid IS NULL THEN
            RAISE EXCEPTION 'product approval function is unavailable'
                USING ERRCODE = '55000';
        END IF;
        FOR unexpected_grantee IN
            SELECT DISTINCT privilege.grantee
            FROM pg_catalog.pg_proc AS function_row
            CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE function_row.oid = function_oid
                AND privilege.grantee <> 0
                AND privilege.grantee <> function_row.proowner
        LOOP
            unexpected_grantee_name := pg_catalog.pg_get_userbyid(unexpected_grantee);
            IF unexpected_grantee_name IS NULL THEN
                RAISE EXCEPTION 'product approval function grantee is unavailable'
                    USING ERRCODE = '55000';
            END IF;
            EXECUTE pg_catalog.format(
                'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM %I CASCADE',
                expected_signature,
                unexpected_grantee_name
            );
        END LOOP;
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s OWNER TO %I',
            expected_signature,
            common_owner_name
        );
        EXECUTE pg_catalog.format(
            'REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE',
            expected_signature
        );
    END LOOP;
END;
$ownership$;

DO $verification$
DECLARE
    relation_count BIGINT;
    table_count BIGINT;
    rls_disabled_count BIGINT;
    owner_count BIGINT;
    common_owner OID;
    invalid_function_count BIGINT;
BEGIN
    SELECT pg_catalog.count(relation.oid),
        pg_catalog.count(relation.oid) FILTER (WHERE relation.relkind = 'r'),
        pg_catalog.count(relation.oid) FILTER (
            WHERE NOT relation.relrowsecurity AND NOT relation.relforcerowsecurity
        ),
        pg_catalog.count(DISTINCT relation.relowner),
        pg_catalog.min(relation.relowner::BIGINT)::OID
    INTO relation_count, table_count, rls_disabled_count, owner_count, common_owner
    FROM (
        VALUES
            (pg_catalog.to_regclass('public.activation_requests')),
            (pg_catalog.to_regclass('public.authoring_promotions')),
            (pg_catalog.to_regclass('public.product_tenants')),
            (pg_catalog.to_regclass('public.automation_installations')),
            (pg_catalog.to_regclass('public.automation_installation_authority_versions')),
            (pg_catalog.to_regclass('public.product_principals')),
            (pg_catalog.to_regclass('public.product_auth_sessions')),
            (pg_catalog.to_regclass('public.product_action_receipts')),
            (pg_catalog.to_regclass('public.product_action_receipt_idempotency_aliases')),
            (pg_catalog.to_regclass('public.product_audit_events')),
            (pg_catalog.to_regclass('public.product_action_receipt_audit_evidence')),
            (pg_catalog.to_regclass('public.activation_request_approvals')),
            (pg_catalog.to_regclass('public.product_control_plane_identity'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid;
    IF relation_count <> 13
        OR table_count <> 13
        OR rls_disabled_count <> 13
        OR owner_count <> 1
        OR common_owner IS NULL
    THEN
        RAISE EXCEPTION 'product approval relation contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            ('public.starring_product_decision_reader_database_identity_v1()',
                'sql', 'text', FALSE, 0::REAL),
            ('public.starring_product_approval_executor_database_identity_v1()',
                'sql', 'text', FALSE, 0::REAL),
            ('public.starring_product_apply_executor_database_identity_v1()',
                'sql', 'text', FALSE, 0::REAL),
            ('public.starring_product_approve_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text)',
                'plpgsql',
                'TABLE(outcome text, resulting_revision bigint, resulting_state text, exact_replay boolean, guild_id text)',
                TRUE, 1::REAL),
            ('public.starring_product_approval_keyring_coverage_v1(text[],text[])',
                'plpgsql', 'TABLE(outcome text)', TRUE, 1::REAL)
    ) AS expected(signature, language_name, result_name, returns_set, rows_estimate)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.signature)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR function_row.provolatile <> 'v'
        OR NOT function_row.proisstrict
        OR function_row.proparallel <> 'u'
        OR NOT function_row.prosecdef
        OR function_row.proconfig <> ARRAY['search_path=pg_catalog']::TEXT[]
        OR function_row.proretset <> expected.returns_set
        OR function_row.prorows <> expected.rows_estimate
        OR language_row.lanname <> expected.language_name
        OR pg_catalog.pg_get_function_result(function_row.oid) <> expected.result_name
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> function_row.proowner
        );
    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION 'product approval function contract is invalid'
            USING ERRCODE = '55000';
    END IF;
END;
$verification$;
