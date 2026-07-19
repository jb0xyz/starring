DO $scope$
DECLARE
    relation_count BIGINT;
    table_count BIGINT;
    rls_disabled_count BIGINT;
    owner_count BIGINT;
    common_owner OID;
    common_owner_name NAME;
    unsafe_schema_create_count BIGINT;
    trigger_mismatch_count BIGINT;
    expected_signature TEXT;
    function_oid OID;
    unexpected_grantee OID;
    unexpected_grantee_name NAME;
    invalid_function_count BIGINT;
    original_search_path TEXT;
    original_quote_all_identifiers TEXT;
BEGIN
    original_search_path := pg_catalog.current_setting('search_path');
    original_quote_all_identifiers :=
        pg_catalog.current_setting('quote_all_identifiers');
    PERFORM pg_catalog.set_config('search_path', 'pg_catalog', TRUE);
    PERFORM pg_catalog.set_config('quote_all_identifiers', 'off', TRUE);

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
            (pg_catalog.to_regclass('public.product_control_plane_identity')),
            (pg_catalog.to_regclass('public.automation_ruleset_activations')),
            (pg_catalog.to_regclass('public.automation_ruleset_versions')),
            (pg_catalog.to_regclass('public.runtime_deployments'))
    ) AS expected(relation_oid)
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = expected.relation_oid;
    IF relation_count <> 16
        OR table_count <> 16
        OR rls_disabled_count <> 16
        OR owner_count <> 1
        OR common_owner IS NULL
    THEN
        RAISE EXCEPTION 'product approval trigger relations require one non-RLS owner'
            USING ERRCODE = '55000';
    END IF;

    common_owner_name := pg_catalog.pg_get_userbyid(common_owner);
    IF common_owner_name IS NULL THEN
        RAISE EXCEPTION 'product approval trigger relation owner is unavailable'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO unsafe_schema_create_count
    FROM pg_catalog.pg_namespace AS namespace
    CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
        namespace.nspacl,
        pg_catalog.acldefault('n', namespace.nspowner)
    )) AS privilege
    WHERE namespace.nspname = 'public'
        AND privilege.privilege_type = 'CREATE'
        AND privilege.grantee <> namespace.nspowner;
    IF unsafe_schema_create_count <> 0
        OR NOT EXISTS (
            SELECT 1
            FROM pg_catalog.pg_namespace AS namespace
            WHERE namespace.nspname = 'public'
                AND namespace.nspowner IN (
                    common_owner,
                    pg_catalog.to_regrole('pg_database_owner'),
                    (
                        SELECT database_row.datdba
                        FROM pg_catalog.pg_database AS database_row
                        WHERE database_row.datname = pg_catalog.current_database()
                    )
                )
        )
    THEN
        RAISE EXCEPTION 'product approval trigger schema is not trusted'
            USING ERRCODE = '55000';
    END IF;

    WITH expected_trigger_definitions(
        relation_identity,
        function_identity,
        definition
    ) AS (
        VALUES
            ('public.activation_request_approvals',
                'public.enforce_activation_approval_payload_binding()',
                'CREATE TRIGGER activation_request_approvals_enforce_payload_binding BEFORE INSERT OR UPDATE ON public.activation_request_approvals FOR EACH ROW EXECUTE FUNCTION public.enforce_activation_approval_payload_binding()'),
            ('public.activation_request_approvals',
                'public.enforce_activation_approval_scope()',
                'CREATE TRIGGER activation_request_approvals_enforce_scope BEFORE INSERT OR UPDATE ON public.activation_request_approvals FOR EACH ROW EXECUTE FUNCTION public.enforce_activation_approval_scope()'),
            ('public.activation_request_approvals',
                'public.reject_activation_approval_mutation()',
                'CREATE TRIGGER activation_request_approvals_reject_mutation BEFORE DELETE OR UPDATE ON public.activation_request_approvals FOR EACH ROW EXECUTE FUNCTION public.reject_activation_approval_mutation()'),
            ('public.activation_requests',
                'public.assert_atomic_product_apply_runtime_request()',
                'CREATE CONSTRAINT TRIGGER activation_requests_assert_atomic_runtime_request AFTER INSERT OR UPDATE ON public.activation_requests DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.assert_atomic_product_apply_runtime_request()'),
            ('public.activation_requests',
                'public.assert_no_committed_product_activation_applying()',
                'CREATE CONSTRAINT TRIGGER activation_requests_assert_no_product_applying AFTER INSERT OR UPDATE ON public.activation_requests DEFERRABLE INITIALLY DEFERRED FOR EACH ROW WHEN (((new.authority_kind = ''product_authoring''::text) AND (new.state = ''applying''::text))) EXECUTE FUNCTION public.assert_no_committed_product_activation_applying()'),
            ('public.activation_requests',
                'public.enforce_product_activation_executor()',
                'CREATE TRIGGER activation_requests_enforce_product_executor BEFORE UPDATE ON public.activation_requests FOR EACH ROW EXECUTE FUNCTION public.enforce_product_activation_executor()'),
            ('public.activation_requests',
                'public.enforce_product_activation_journal_link()',
                'CREATE TRIGGER activation_requests_enforce_product_journal_link BEFORE INSERT OR UPDATE ON public.activation_requests FOR EACH ROW EXECUTE FUNCTION public.enforce_product_activation_journal_link()'),
            ('public.activation_requests',
                'public.enforce_product_activation_scope()',
                'CREATE TRIGGER activation_requests_enforce_product_scope BEFORE INSERT OR UPDATE ON public.activation_requests FOR EACH ROW EXECUTE FUNCTION public.enforce_product_activation_scope()'),
            ('public.activation_requests',
                'public.guard_legacy_activation_product_slot()',
                'CREATE TRIGGER activation_requests_guard_legacy_product_slot BEFORE INSERT OR UPDATE ON public.activation_requests FOR EACH ROW EXECUTE FUNCTION public.guard_legacy_activation_product_slot()'),
            ('public.activation_requests',
                'public.guard_product_activation_applied_record()',
                'CREATE TRIGGER activation_requests_guard_product_applied_record BEFORE UPDATE ON public.activation_requests FOR EACH ROW EXECUTE FUNCTION public.guard_product_activation_applied_record()'),
            ('public.activation_requests',
                'public.guard_product_ruleset_artifact_transition()',
                'CREATE TRIGGER activation_requests_guard_ruleset_artifact_transition BEFORE UPDATE ON public.activation_requests FOR EACH ROW EXECUTE FUNCTION public.guard_product_ruleset_artifact_transition()'),
            ('public.product_action_receipt_audit_evidence',
                'public.reject_immutable_product_row()',
                'CREATE TRIGGER product_action_receipt_audit_evidence_reject_mutation BEFORE DELETE OR UPDATE ON public.product_action_receipt_audit_evidence FOR EACH ROW EXECUTE FUNCTION public.reject_immutable_product_row()'),
            ('public.product_action_receipt_idempotency_aliases',
                'public.enforce_product_action_receipt_alias_capacity()',
                'CREATE TRIGGER product_action_receipt_idempotency_aliases_enforce_capacity BEFORE INSERT ON public.product_action_receipt_idempotency_aliases FOR EACH ROW EXECUTE FUNCTION public.enforce_product_action_receipt_alias_capacity()'),
            ('public.product_action_receipt_idempotency_aliases',
                'public.enforce_product_action_receipt_alias_retention()',
                'CREATE TRIGGER product_action_receipt_idempotency_aliases_reject_mutation BEFORE DELETE OR UPDATE ON public.product_action_receipt_idempotency_aliases FOR EACH ROW EXECUTE FUNCTION public.enforce_product_action_receipt_alias_retention()'),
            ('public.product_action_receipts',
                'public.assert_product_approval_receipt_alias()',
                'CREATE CONSTRAINT TRIGGER product_action_receipts_assert_approval_alias AFTER INSERT ON public.product_action_receipts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.assert_product_approval_receipt_alias()'),
            ('public.product_action_receipts',
                'public.assert_product_approval_receipt_audit()',
                'CREATE CONSTRAINT TRIGGER product_action_receipts_assert_approval_audit AFTER INSERT ON public.product_action_receipts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.assert_product_approval_receipt_audit()'),
            ('public.product_action_receipts',
                'public.enforce_product_action_receipt_retention()',
                'CREATE TRIGGER product_action_receipts_reject_mutation BEFORE DELETE OR UPDATE ON public.product_action_receipts FOR EACH ROW EXECUTE FUNCTION public.enforce_product_action_receipt_retention()'),
            ('public.product_audit_events',
                'public.capture_product_action_receipt_audit_evidence()',
                'CREATE TRIGGER product_audit_events_capture_receipt_evidence AFTER INSERT ON public.product_audit_events FOR EACH ROW EXECUTE FUNCTION public.capture_product_action_receipt_audit_evidence()'),
            ('public.product_audit_events',
                'public.reject_immutable_product_row()',
                'CREATE TRIGGER product_audit_events_reject_mutation BEFORE DELETE OR UPDATE ON public.product_audit_events FOR EACH ROW EXECUTE FUNCTION public.reject_immutable_product_row()')
    ), expected_triggers AS (
        SELECT pg_catalog.to_regclass(expected.relation_identity) AS relation_oid,
            pg_catalog.to_regprocedure(expected.function_identity) AS function_oid,
            expected.definition
        FROM expected_trigger_definitions AS expected
    ), actual_triggers AS (
        SELECT trigger_row.oid AS trigger_oid,
            trigger_row.tgrelid AS relation_oid,
            trigger_row.tgfoid AS function_oid,
            trigger_row.tgenabled::TEXT AS enabled,
            trigger_row.tgisinternal AS internal,
            trigger_row.tgparentid = 0
                AND trigger_row.tgconstrrelid = 0
                AND trigger_row.tgconstrindid = 0
                AND pg_catalog.cardinality(trigger_row.tgattr) = 0
                AND trigger_row.tgnargs = 0
                AND pg_catalog.octet_length(trigger_row.tgargs) = 0
                AND trigger_row.tgoldtable IS NULL
                AND trigger_row.tgnewtable IS NULL
                AND (
                    (
                        trigger_row.tgconstraint = 0
                        AND NOT trigger_row.tgdeferrable
                        AND NOT trigger_row.tginitdeferred
                        AND constraint_row.oid IS NULL
                    ) OR (
                        trigger_row.tgconstraint <> 0
                        AND constraint_row.contype = 't'
                        AND constraint_row.conname = trigger_row.tgname
                        AND constraint_row.conrelid = trigger_row.tgrelid
                        AND constraint_row.condeferrable = trigger_row.tgdeferrable
                        AND constraint_row.condeferred = trigger_row.tginitdeferred
                        AND constraint_row.convalidated
                        AND constraint_row.conparentid = 0
                    )
                ) AS structural_valid,
            pg_catalog.pg_get_triggerdef(trigger_row.oid, FALSE) AS definition
        FROM pg_catalog.pg_trigger AS trigger_row
        LEFT JOIN pg_catalog.pg_constraint AS constraint_row
            ON constraint_row.oid = trigger_row.tgconstraint
        WHERE (
            NOT trigger_row.tgisinternal
            AND trigger_row.tgrelid IN (
                SELECT DISTINCT expected.relation_oid
                FROM expected_triggers AS expected
            )
        ) OR (
            trigger_row.tgfoid IN (
                SELECT DISTINCT expected.function_oid
                FROM expected_triggers AS expected
            )
            AND trigger_row.tgfoid <> pg_catalog.to_regprocedure(
                'public.reject_immutable_product_row()'
            )
        )
    )
    SELECT pg_catalog.count(*)
    INTO trigger_mismatch_count
    FROM expected_triggers AS expected
    FULL JOIN actual_triggers AS actual
        ON actual.relation_oid = expected.relation_oid
        AND actual.function_oid = expected.function_oid
        AND actual.definition = expected.definition
        AND actual.enabled = 'O'
        AND NOT actual.internal
        AND actual.structural_valid
    WHERE expected.relation_oid IS NULL
        OR actual.trigger_oid IS NULL;
    IF trigger_mismatch_count <> 0 THEN
        RAISE EXCEPTION 'product approval trigger manifest is invalid'
            USING ERRCODE = '55000';
    END IF;

    FOR expected_signature IN
        SELECT expected.signature
        FROM (
            VALUES
                ('public.assert_atomic_product_apply_runtime_request()'),
                ('public.assert_no_committed_product_activation_applying()'),
                ('public.assert_product_approval_receipt_alias()'),
                ('public.assert_product_approval_receipt_audit()'),
                ('public.capture_product_action_receipt_audit_evidence()'),
                ('public.enforce_activation_approval_payload_binding()'),
                ('public.enforce_activation_approval_scope()'),
                ('public.enforce_product_action_receipt_alias_capacity()'),
                ('public.enforce_product_action_receipt_alias_retention()'),
                ('public.enforce_product_action_receipt_retention()'),
                ('public.enforce_product_activation_executor()'),
                ('public.enforce_product_activation_journal_link()'),
                ('public.enforce_product_activation_scope()'),
                ('public.guard_legacy_activation_product_slot()'),
                ('public.guard_product_activation_applied_record()'),
                ('public.guard_product_ruleset_artifact_transition()'),
                ('public.reject_activation_approval_mutation()')
        ) AS expected(signature)
    LOOP
        function_oid := pg_catalog.to_regprocedure(expected_signature);
        IF function_oid IS NULL
            OR (SELECT function_row.proowner
                FROM pg_catalog.pg_proc AS function_row
                WHERE function_row.oid = function_oid) <> common_owner
        THEN
            RAISE EXCEPTION 'product approval trigger function owner is invalid'
                USING ERRCODE = '55000';
        END IF;
    END LOOP;

    function_oid := pg_catalog.to_regprocedure(
        'public.starring_runtime_desired_target_digest_v1(jsonb,bigint)'
    );
    IF function_oid IS NULL
        OR (SELECT function_row.proowner
            FROM pg_catalog.pg_proc AS function_row
            WHERE function_row.oid = function_oid) <> common_owner
    THEN
        RAISE EXCEPTION 'product approval support function owner is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            ('public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'),
            ('public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)'),
            ('public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)')
    ) AS expected(signature)
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.signature)
    WHERE function_row.oid IS NULL
        OR function_row.proowner <> common_owner
        OR function_row.prokind <> 'f'
        OR NOT function_row.prosecdef
        OR function_row.proconfig <> ARRAY['search_path=pg_catalog']::TEXT[];
    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION 'product approval shared apply function contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    EXECUTE $definition$
CREATE OR REPLACE FUNCTION public.enforce_product_activation_journal_link()
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
        FROM public.authoring_promotions
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
$definition$;

    EXECUTE $definition$
CREATE OR REPLACE FUNCTION public.reject_immutable_product_approval_row()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $function$
BEGIN
    RAISE EXCEPTION 'immutable product records cannot be updated or deleted'
        USING ERRCODE = '23514';
END;
$function$;
$definition$;
    EXECUTE 'DROP TRIGGER product_action_receipt_audit_evidence_reject_mutation ON public.product_action_receipt_audit_evidence';
    EXECUTE $definition$
CREATE TRIGGER product_action_receipt_audit_evidence_reject_mutation
BEFORE DELETE OR UPDATE ON public.product_action_receipt_audit_evidence
FOR EACH ROW
EXECUTE FUNCTION public.reject_immutable_product_approval_row()
$definition$;
    EXECUTE 'DROP TRIGGER product_audit_events_reject_mutation ON public.product_audit_events';
    EXECUTE $definition$
CREATE TRIGGER product_audit_events_reject_mutation
BEFORE DELETE OR UPDATE ON public.product_audit_events
FOR EACH ROW
EXECUTE FUNCTION public.reject_immutable_product_approval_row()
$definition$;

    FOR expected_signature IN
        SELECT expected.signature
        FROM (
            VALUES
                ('public.assert_atomic_product_apply_runtime_request()'),
                ('public.assert_no_committed_product_activation_applying()'),
                ('public.assert_product_approval_receipt_alias()'),
                ('public.assert_product_approval_receipt_audit()'),
                ('public.capture_product_action_receipt_audit_evidence()'),
                ('public.enforce_activation_approval_payload_binding()'),
                ('public.enforce_activation_approval_scope()'),
                ('public.enforce_product_action_receipt_alias_capacity()'),
                ('public.enforce_product_action_receipt_alias_retention()'),
                ('public.enforce_product_action_receipt_retention()'),
                ('public.enforce_product_activation_executor()'),
                ('public.enforce_product_activation_journal_link()'),
                ('public.enforce_product_activation_scope()'),
                ('public.guard_legacy_activation_product_slot()'),
                ('public.guard_product_activation_applied_record()'),
                ('public.guard_product_ruleset_artifact_transition()'),
                ('public.reject_activation_approval_mutation()'),
                ('public.reject_immutable_product_approval_row()')
        ) AS expected(signature)
    LOOP
        EXECUTE pg_catalog.format('ALTER FUNCTION %s VOLATILE', expected_signature);
        EXECUTE pg_catalog.format('ALTER FUNCTION %s STRICT', expected_signature);
        EXECUTE pg_catalog.format('ALTER FUNCTION %s PARALLEL UNSAFE', expected_signature);
        EXECUTE pg_catalog.format('ALTER FUNCTION %s SECURITY DEFINER', expected_signature);
        EXECUTE pg_catalog.format('ALTER FUNCTION %s RESET ALL', expected_signature);
        EXECUTE pg_catalog.format(
            'ALTER FUNCTION %s SET search_path = pg_catalog',
            expected_signature
        );
        function_oid := pg_catalog.to_regprocedure(expected_signature);
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
                RAISE EXCEPTION 'product approval trigger grantee is unavailable'
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

    expected_signature :=
        'public.starring_runtime_desired_target_digest_v1(jsonb,bigint)';
    EXECUTE pg_catalog.format('ALTER FUNCTION %s IMMUTABLE', expected_signature);
    EXECUTE pg_catalog.format('ALTER FUNCTION %s STRICT', expected_signature);
    EXECUTE pg_catalog.format('ALTER FUNCTION %s PARALLEL UNSAFE', expected_signature);
    EXECUTE pg_catalog.format('ALTER FUNCTION %s SECURITY INVOKER', expected_signature);
    EXECUTE pg_catalog.format('ALTER FUNCTION %s RESET ALL', expected_signature);
    EXECUTE pg_catalog.format(
        'ALTER FUNCTION %s SET search_path = pg_catalog',
        expected_signature
    );
    function_oid := pg_catalog.to_regprocedure(expected_signature);
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
            RAISE EXCEPTION 'product approval support grantee is unavailable'
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

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM (
        VALUES
            ('public.assert_atomic_product_apply_runtime_request()'),
            ('public.assert_no_committed_product_activation_applying()'),
            ('public.assert_product_approval_receipt_alias()'),
            ('public.assert_product_approval_receipt_audit()'),
            ('public.capture_product_action_receipt_audit_evidence()'),
            ('public.enforce_activation_approval_payload_binding()'),
            ('public.enforce_activation_approval_scope()'),
            ('public.enforce_product_action_receipt_alias_capacity()'),
            ('public.enforce_product_action_receipt_alias_retention()'),
            ('public.enforce_product_action_receipt_retention()'),
            ('public.enforce_product_activation_executor()'),
            ('public.enforce_product_activation_journal_link()'),
            ('public.enforce_product_activation_scope()'),
            ('public.guard_legacy_activation_product_slot()'),
            ('public.guard_product_activation_applied_record()'),
            ('public.guard_product_ruleset_artifact_transition()'),
            ('public.reject_activation_approval_mutation()'),
            ('public.reject_immutable_product_approval_row()')
    ) AS expected(signature)
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
        OR function_row.proretset
        OR function_row.prorows <> 0
        OR function_row.proconfig <> ARRAY['search_path=pg_catalog']::TEXT[]
        OR language_row.lanname <> 'plpgsql'
        OR pg_catalog.pg_get_function_result(function_row.oid) <> 'trigger'
        OR EXISTS (
            SELECT 1
            FROM pg_catalog.aclexplode(COALESCE(
                function_row.proacl,
                pg_catalog.acldefault('f', function_row.proowner)
            )) AS privilege
            WHERE privilege.grantee <> function_row.proowner
        );
    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION 'product approval trigger function contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO invalid_function_count
    FROM pg_catalog.pg_proc AS function_row
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
    WHERE function_row.oid = pg_catalog.to_regprocedure(
            'public.starring_runtime_desired_target_digest_v1(jsonb,bigint)'
        )
        AND (
            function_row.proowner <> common_owner
            OR function_row.prokind <> 'f'
            OR function_row.provolatile <> 'i'
            OR NOT function_row.proisstrict
            OR function_row.proparallel <> 'u'
            OR function_row.prosecdef
            OR function_row.proretset
            OR function_row.prorows <> 0
            OR function_row.proconfig <> ARRAY['search_path=pg_catalog']::TEXT[]
            OR language_row.lanname <> 'plpgsql'
            OR pg_catalog.pg_get_function_result(function_row.oid) <> 'text'
            OR EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE privilege.grantee <> function_row.proowner
            )
        );
    IF invalid_function_count <> 0 THEN
        RAISE EXCEPTION 'product approval support function contract is invalid'
            USING ERRCODE = '55000';
    END IF;

    SELECT pg_catalog.count(*)
    INTO trigger_mismatch_count
    FROM (
        VALUES
            (
                pg_catalog.to_regclass('public.product_action_receipt_audit_evidence'),
                'CREATE TRIGGER product_action_receipt_audit_evidence_reject_mutation BEFORE DELETE OR UPDATE ON public.product_action_receipt_audit_evidence FOR EACH ROW EXECUTE FUNCTION public.reject_immutable_product_approval_row()'
            ),
            (
                pg_catalog.to_regclass('public.product_audit_events'),
                'CREATE TRIGGER product_audit_events_reject_mutation BEFORE DELETE OR UPDATE ON public.product_audit_events FOR EACH ROW EXECUTE FUNCTION public.reject_immutable_product_approval_row()'
            )
    ) AS expected(relation_oid, definition)
    FULL JOIN (
        SELECT trigger_row.oid AS trigger_oid,
            trigger_row.tgrelid AS relation_oid,
            trigger_row.tgenabled::TEXT AS enabled,
            trigger_row.tgisinternal AS internal,
            trigger_row.tgparentid = 0
                AND trigger_row.tgconstraint = 0
                AND NOT trigger_row.tgdeferrable
                AND NOT trigger_row.tginitdeferred
                AND trigger_row.tgconstrrelid = 0
                AND trigger_row.tgconstrindid = 0
                AND pg_catalog.cardinality(trigger_row.tgattr) = 0
                AND trigger_row.tgnargs = 0
                AND pg_catalog.octet_length(trigger_row.tgargs) = 0
                AND trigger_row.tgoldtable IS NULL
                AND trigger_row.tgnewtable IS NULL AS structural_valid,
            pg_catalog.pg_get_triggerdef(trigger_row.oid, FALSE) AS definition
        FROM pg_catalog.pg_trigger AS trigger_row
        WHERE trigger_row.tgfoid = pg_catalog.to_regprocedure(
            'public.reject_immutable_product_approval_row()'
        )
    ) AS actual
        ON actual.relation_oid = expected.relation_oid
        AND actual.definition = expected.definition
        AND actual.enabled = 'O'
        AND NOT actual.internal
        AND actual.structural_valid
    WHERE expected.relation_oid IS NULL
        OR actual.trigger_oid IS NULL;
    IF trigger_mismatch_count <> 0 THEN
        RAISE EXCEPTION 'product approval immutable trigger contract is invalid'
            USING ERRCODE = '55000';
    END IF;
    PERFORM pg_catalog.set_config('search_path', original_search_path, TRUE);
    PERFORM pg_catalog.set_config(
        'quote_all_identifiers',
        original_quote_all_identifiers,
        TRUE
    );
END;
$scope$;
