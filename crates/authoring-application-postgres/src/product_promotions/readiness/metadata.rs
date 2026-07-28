pub(super) const SUPPORT_CONTRACT_QUERY: &str = r#"
WITH common_owner AS (
    SELECT relation.relowner AS owner_oid
    FROM pg_catalog.pg_class AS relation
    WHERE relation.oid = pg_catalog.to_regclass('public.authoring_promotions')
), capability_names(function_name) AS (
    VALUES
        ('starring_product_promotion_executor_database_identity_v1'::NAME),
        ('starring_product_promotion_replay_v1'::NAME),
        ('starring_product_promotion_prepare_v1'::NAME),
        ('starring_product_promotion_publish_v1'::NAME),
        ('starring_product_promotion_approval_environment_v1'::NAME),
        ('starring_product_promotion_activation_link_v1'::NAME),
        ('starring_product_promotion_repair_link_v1'::NAME),
        ('starring_product_promotion_keyring_coverage_v1'::NAME),
        ('starring_product_promotion_authorize_current_v1'::NAME),
        ('starring_product_promotion_finalize_receipt_v1'::NAME)
), capability_overload_contract AS (
    SELECT pg_catalog.count(*) = 10 AS valid
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            SELECT expected.function_name FROM capability_names AS expected
        )
), internal_routines(
    function_identity,
    identity_arguments,
    result_name
) AS (
    VALUES
        ('public.starring_product_promotion_authorize_current_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean)',
            'expected_tenant_id text, expected_installation_id text, expected_principal_id text, expected_product_session_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, observed_current_authority_revision bigint, observed_current_authority_payload_digest text, authority_observation_digest text, authority_observed_at timestamp with time zone, authority_expires_at timestamp with time zone, effective_permission_bits text, guild_owner boolean',
            'TABLE(outcome_code text, database_now timestamp with time zone, current_authority_projection jsonb)'),
        ('public.starring_product_promotion_finalize_receipt_v1(jsonb,jsonb,jsonb,jsonb,jsonb)',
            'admission_projection jsonb, promotion_projection jsonb, activation_projection jsonb, receipt_projection jsonb, audit_projection jsonb',
            'TABLE(outcome_code text)')
), internal_contract AS (
    SELECT pg_catalog.count(*) = 2
        AND pg_catalog.bool_and(COALESCE(
            function_row.oid IS NOT NULL
            AND function_row.proowner = common_owner.owner_oid
            AND function_row.prokind = 'f'
            AND function_row.provolatile = 'v'
            AND function_row.proisstrict
            AND function_row.proparallel = 'u'
            AND function_row.prosecdef
            AND function_row.proretset
            AND function_row.prorows = 1
            AND NOT function_row.proleakproof
            AND function_row.pronargdefaults = 0
            AND function_row.provariadic = 0
            AND function_row.proconfig = ARRAY['search_path=pg_catalog']::TEXT[]
            AND language_row.lanname = 'plpgsql'
            AND pg_catalog.pg_get_function_identity_arguments(function_row.oid)
                = expected.identity_arguments
            AND pg_catalog.pg_get_function_result(function_row.oid)
                = expected.result_name
            AND NOT pg_catalog.has_function_privilege(
                current_user, function_row.oid, 'EXECUTE'
            )
            AND NOT EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE privilege.grantee <> function_row.proowner
            ), FALSE)) AS valid
    FROM internal_routines AS expected
    CROSS JOIN common_owner
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.function_identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
), shared_helpers(
    function_identity,
    identity_arguments,
    result_name,
    language_name,
    volatility,
    parallel_mode,
    security_definer
) AS (
    VALUES
        ('public.starring_canonical_json_v1(jsonb)',
            'document jsonb', 'text', 'plpgsql', 'i'::"char", 's'::"char", FALSE),
        ('public.starring_ruleset_content_hash_v1(bigint,jsonb)',
            'schema_version bigint, definition jsonb', 'text', 'plpgsql',
            'i'::"char", 's'::"char", FALSE),
        ('public.starring_product_ruleset_slot_exact_v1(text,text,text,text,bigint)',
            'expected_tenant_id text, expected_installation_id text, expected_guild_id text, expected_ruleset_key text, expected_active_version bigint',
            'boolean', 'sql', 's'::"char", 'u'::"char", TRUE)
), shared_helper_contract AS (
    SELECT pg_catalog.count(*) = 3
        AND pg_catalog.bool_and(COALESCE(
            function_row.oid IS NOT NULL
            AND function_row.proowner = common_owner.owner_oid
            AND function_row.prokind = 'f'
            AND function_row.provolatile = expected.volatility
            AND function_row.proisstrict
            AND function_row.proparallel = expected.parallel_mode
            AND function_row.prosecdef = expected.security_definer
            AND NOT function_row.proretset
            AND function_row.prorows = 0
            AND NOT function_row.proleakproof
            AND function_row.pronargdefaults = 0
            AND function_row.provariadic = 0
            AND function_row.proconfig = ARRAY['search_path=pg_catalog']::TEXT[]
            AND language_row.lanname = expected.language_name
            AND pg_catalog.pg_get_function_identity_arguments(function_row.oid)
                = expected.identity_arguments
            AND pg_catalog.pg_get_function_result(function_row.oid)
                = expected.result_name
            AND NOT pg_catalog.has_function_privilege(
                current_user, function_row.oid, 'EXECUTE'
            )
            AND NOT EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE privilege.grantee <> function_row.proowner
            ), FALSE)) AS valid
    FROM shared_helpers AS expected
    CROSS JOIN common_owner
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.function_identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
), shared_helper_overload_contract AS (
    SELECT pg_catalog.count(*) = 3 AS valid
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            SELECT expected_function.proname
            FROM shared_helpers AS expected
            INNER JOIN pg_catalog.pg_proc AS expected_function
                ON expected_function.oid
                    = pg_catalog.to_regprocedure(expected.function_identity)
        )
), expected_trigger_definitions(
    relation_identity,
    trigger_name,
    function_identity,
    definition
) AS (
    VALUES
        ('public.authoring_promotions', 'authoring_promotions_enforce_scope',
            'public.enforce_authoring_promotion_scope()',
            'CREATE TRIGGER authoring_promotions_enforce_scope BEFORE INSERT OR UPDATE ON public.authoring_promotions FOR EACH ROW EXECUTE FUNCTION public.enforce_authoring_promotion_scope()'),
        ('public.authoring_promotions', 'authoring_promotions_enforce_product_admission',
            'public.enforce_authoring_promotion_product_admission()',
            'CREATE TRIGGER authoring_promotions_enforce_product_admission BEFORE INSERT OR UPDATE ON public.authoring_promotions FOR EACH ROW EXECUTE FUNCTION public.enforce_authoring_promotion_product_admission()'),
        ('public.authoring_promotions', 'authoring_promotions_enforce_product_transition',
            'public.enforce_authoring_promotion_product_transition()',
            'CREATE TRIGGER authoring_promotions_enforce_product_transition BEFORE INSERT OR UPDATE ON public.authoring_promotions FOR EACH ROW EXECUTE FUNCTION public.enforce_authoring_promotion_product_transition()'),
        ('public.automation_ruleset_versions', 'automation_ruleset_versions_reject_mutation',
            'public.reject_ruleset_artifact_mutation()',
            'CREATE TRIGGER automation_ruleset_versions_reject_mutation BEFORE DELETE OR UPDATE ON public.automation_ruleset_versions FOR EACH STATEMENT EXECUTE FUNCTION public.reject_ruleset_artifact_mutation()'),
        ('public.automation_ruleset_versions', 'automation_ruleset_versions_reject_truncate',
            'public.reject_ruleset_artifact_mutation()',
            'CREATE TRIGGER automation_ruleset_versions_reject_truncate BEFORE TRUNCATE ON public.automation_ruleset_versions FOR EACH STATEMENT EXECUTE FUNCTION public.reject_ruleset_artifact_mutation()'),
        ('public.activation_requests', 'activation_requests_enforce_product_journal_link',
            'public.enforce_product_activation_journal_link()',
            'CREATE TRIGGER activation_requests_enforce_product_journal_link BEFORE INSERT OR UPDATE ON public.activation_requests FOR EACH ROW EXECUTE FUNCTION public.enforce_product_activation_journal_link()'),
        ('public.activation_requests', 'activation_requests_enforce_product_scope',
            'public.enforce_product_activation_scope()',
            'CREATE TRIGGER activation_requests_enforce_product_scope BEFORE INSERT OR UPDATE ON public.activation_requests FOR EACH ROW EXECUTE FUNCTION public.enforce_product_activation_scope()'),
        ('public.activation_requests', 'activation_requests_guard_legacy_product_slot',
            'public.guard_legacy_activation_product_slot()',
            'CREATE TRIGGER activation_requests_guard_legacy_product_slot BEFORE INSERT OR UPDATE ON public.activation_requests FOR EACH ROW EXECUTE FUNCTION public.guard_legacy_activation_product_slot()'),
        ('public.activation_requests', 'activation_requests_guard_ruleset_artifact_transition',
            'public.guard_product_ruleset_artifact_transition()',
            'CREATE TRIGGER activation_requests_guard_ruleset_artifact_transition BEFORE UPDATE ON public.activation_requests FOR EACH ROW EXECUTE FUNCTION public.guard_product_ruleset_artifact_transition()'),
        ('public.activation_request_approvals', 'activation_request_approvals_enforce_payload_binding',
            'public.enforce_activation_approval_payload_binding()',
            'CREATE TRIGGER activation_request_approvals_enforce_payload_binding BEFORE INSERT OR UPDATE ON public.activation_request_approvals FOR EACH ROW EXECUTE FUNCTION public.enforce_activation_approval_payload_binding()'),
        ('public.activation_request_approvals', 'activation_request_approvals_enforce_scope',
            'public.enforce_activation_approval_scope()',
            'CREATE TRIGGER activation_request_approvals_enforce_scope BEFORE INSERT OR UPDATE ON public.activation_request_approvals FOR EACH ROW EXECUTE FUNCTION public.enforce_activation_approval_scope()'),
        ('public.activation_request_approvals', 'activation_request_approvals_reject_mutation',
            'public.reject_activation_approval_mutation()',
            'CREATE TRIGGER activation_request_approvals_reject_mutation BEFORE DELETE OR UPDATE ON public.activation_request_approvals FOR EACH ROW EXECUTE FUNCTION public.reject_activation_approval_mutation()'),
        ('public.product_action_receipts', 'product_action_receipts_assert_approval_alias',
            'public.assert_product_approval_receipt_alias()',
            'CREATE CONSTRAINT TRIGGER product_action_receipts_assert_approval_alias AFTER INSERT ON public.product_action_receipts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.assert_product_approval_receipt_alias()'),
        ('public.product_action_receipts', 'product_action_receipts_assert_approval_audit',
            'public.assert_product_approval_receipt_audit()',
            'CREATE CONSTRAINT TRIGGER product_action_receipts_assert_approval_audit AFTER INSERT ON public.product_action_receipts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.assert_product_approval_receipt_audit()'),
        ('public.product_action_receipts', 'product_action_receipts_reject_mutation',
            'public.enforce_product_action_receipt_retention()',
            'CREATE TRIGGER product_action_receipts_reject_mutation BEFORE DELETE OR UPDATE ON public.product_action_receipts FOR EACH ROW EXECUTE FUNCTION public.enforce_product_action_receipt_retention()'),
        ('public.product_action_receipt_idempotency_aliases', 'product_action_receipt_idempotency_aliases_enforce_capacity',
            'public.enforce_product_action_receipt_alias_capacity()',
            'CREATE TRIGGER product_action_receipt_idempotency_aliases_enforce_capacity BEFORE INSERT ON public.product_action_receipt_idempotency_aliases FOR EACH ROW EXECUTE FUNCTION public.enforce_product_action_receipt_alias_capacity()'),
        ('public.product_action_receipt_idempotency_aliases', 'product_action_receipt_idempotency_aliases_reject_mutation',
            'public.enforce_product_action_receipt_alias_retention()',
            'CREATE TRIGGER product_action_receipt_idempotency_aliases_reject_mutation BEFORE DELETE OR UPDATE ON public.product_action_receipt_idempotency_aliases FOR EACH ROW EXECUTE FUNCTION public.enforce_product_action_receipt_alias_retention()'),
        ('public.product_audit_events', 'product_audit_events_capture_receipt_evidence',
            'public.capture_product_action_receipt_audit_evidence()',
            'CREATE TRIGGER product_audit_events_capture_receipt_evidence AFTER INSERT ON public.product_audit_events FOR EACH ROW EXECUTE FUNCTION public.capture_product_action_receipt_audit_evidence()'),
        ('public.product_audit_events', 'product_audit_events_reject_mutation',
            'public.reject_immutable_product_approval_row()',
            'CREATE TRIGGER product_audit_events_reject_mutation BEFORE DELETE OR UPDATE ON public.product_audit_events FOR EACH ROW EXECUTE FUNCTION public.reject_immutable_product_approval_row()'),
        ('public.product_action_receipt_audit_evidence', 'product_action_receipt_audit_evidence_reject_mutation',
            'public.reject_immutable_product_approval_row()',
            'CREATE TRIGGER product_action_receipt_audit_evidence_reject_mutation BEFORE DELETE OR UPDATE ON public.product_action_receipt_audit_evidence FOR EACH ROW EXECUTE FUNCTION public.reject_immutable_product_approval_row()')
), expected_triggers AS (
    SELECT pg_catalog.to_regclass(expected.relation_identity) AS relation_oid,
        expected.trigger_name,
        pg_catalog.to_regprocedure(expected.function_identity) AS function_oid,
        expected.definition
    FROM expected_trigger_definitions AS expected
), actual_triggers AS (
    SELECT trigger_row.oid AS trigger_oid,
        trigger_row.tgrelid AS relation_oid,
        trigger_row.tgname::TEXT AS trigger_name,
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
    WHERE NOT trigger_row.tgisinternal
        AND (
            trigger_row.tgrelid
                = pg_catalog.to_regclass('public.activation_request_approvals')
            OR
            EXISTS (
                SELECT 1 FROM expected_triggers AS expected
                WHERE expected.relation_oid = trigger_row.tgrelid
                    AND expected.trigger_name = trigger_row.tgname
            ) OR trigger_row.tgfoid IN (
                SELECT expected.function_oid FROM expected_triggers AS expected
            )
        )
), trigger_manifest AS (
    SELECT (SELECT pg_catalog.count(*) FROM expected_triggers) = 20
        AND (SELECT pg_catalog.count(*) FROM actual_triggers) = 20
        AND NOT EXISTS (
            SELECT 1
            FROM expected_triggers AS expected
            FULL JOIN actual_triggers AS actual
                ON actual.relation_oid = expected.relation_oid
                AND actual.trigger_name = expected.trigger_name
                AND actual.function_oid = expected.function_oid
                AND actual.definition = expected.definition
                AND actual.enabled = 'O'
                AND NOT actual.internal
                AND actual.structural_valid
            WHERE expected.relation_oid IS NULL
                OR actual.trigger_oid IS NULL
        ) AS valid
), trigger_helpers(
    function_identity,
    strict,
    security_definer
) AS (
    VALUES
        ('public.enforce_authoring_promotion_scope()', FALSE, FALSE),
        ('public.enforce_authoring_promotion_product_admission()', TRUE, TRUE),
        ('public.enforce_authoring_promotion_product_transition()', TRUE, TRUE),
        ('public.reject_ruleset_artifact_mutation()', FALSE, TRUE),
        ('public.enforce_product_activation_journal_link()', TRUE, TRUE),
        ('public.enforce_product_activation_scope()', TRUE, TRUE),
        ('public.guard_legacy_activation_product_slot()', TRUE, TRUE),
        ('public.guard_product_ruleset_artifact_transition()', TRUE, TRUE),
        ('public.enforce_activation_approval_payload_binding()', TRUE, TRUE),
        ('public.enforce_activation_approval_scope()', TRUE, TRUE),
        ('public.reject_activation_approval_mutation()', TRUE, TRUE),
        ('public.assert_product_approval_receipt_alias()', TRUE, TRUE),
        ('public.assert_product_approval_receipt_audit()', TRUE, TRUE),
        ('public.enforce_product_action_receipt_retention()', TRUE, TRUE),
        ('public.enforce_product_action_receipt_alias_capacity()', TRUE, TRUE),
        ('public.enforce_product_action_receipt_alias_retention()', TRUE, TRUE),
        ('public.capture_product_action_receipt_audit_evidence()', TRUE, TRUE),
        ('public.reject_immutable_product_approval_row()', TRUE, TRUE)
), trigger_helper_contract AS (
    SELECT pg_catalog.count(*) = 18
        AND pg_catalog.bool_and(COALESCE(
            function_row.oid IS NOT NULL
            AND function_row.proowner = common_owner.owner_oid
            AND function_row.prokind = 'f'
            AND function_row.provolatile = 'v'
            AND function_row.proisstrict = expected.strict
            AND function_row.proparallel = 'u'
            AND function_row.prosecdef = expected.security_definer
            AND NOT function_row.proretset
            AND function_row.prorows = 0
            AND NOT function_row.proleakproof
            AND function_row.pronargdefaults = 0
            AND function_row.provariadic = 0
            AND function_row.proconfig = ARRAY['search_path=pg_catalog']::TEXT[]
            AND language_row.lanname = 'plpgsql'
            AND pg_catalog.pg_get_function_identity_arguments(function_row.oid) = ''
            AND pg_catalog.pg_get_function_result(function_row.oid) = 'trigger'
            AND NOT pg_catalog.has_function_privilege(
                current_user, function_row.oid, 'EXECUTE'
            )
            AND NOT EXISTS (
                SELECT 1
                FROM pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                )) AS privilege
                WHERE privilege.grantee <> function_row.proowner
            ), FALSE)) AS valid
    FROM trigger_helpers AS expected
    CROSS JOIN common_owner
    LEFT JOIN pg_catalog.pg_proc AS function_row
        ON function_row.oid = pg_catalog.to_regprocedure(expected.function_identity)
    LEFT JOIN pg_catalog.pg_language AS language_row
        ON language_row.oid = function_row.prolang
), trigger_helper_overload_contract AS (
    SELECT pg_catalog.count(*) = 18 AS valid
    FROM pg_catalog.pg_proc AS function_row
    INNER JOIN pg_catalog.pg_namespace AS namespace
        ON namespace.oid = function_row.pronamespace
    WHERE namespace.nspname = 'public'
        AND function_row.proname IN (
            SELECT expected_function.proname
            FROM trigger_helpers AS expected
            INNER JOIN pg_catalog.pg_proc AS expected_function
                ON expected_function.oid
                    = pg_catalog.to_regprocedure(expected.function_identity)
        )
), expected_constraints(
    relation_identity,
    constraint_name,
    constraint_type,
    no_inherit,
    definition
) AS (
    VALUES
        ('public.authoring_promotions', 'authoring_promotions_product_admission_valid', 'c'::"char", FALSE,
            'CHECK ((((product_admission_format_version IS NULL) AND (product_admission_digest IS NULL) AND (product_admission IS NULL)) OR ((product_admission_format_version = 1) AND ((product_admission_digest ~ ''^[0-9a-f]{64}$''::text) IS TRUE) AND (jsonb_typeof(product_admission) = ''object''::text) AND (octet_length((product_admission)::text) <= 32768) AND ((product_admission ->> ''format_version''::text) = ''1''::text) AND (jsonb_typeof((product_admission -> ''payload''::text)) = ''object''::text) AND (((product_admission ->> ''admitted_at''::text) ~ ''^[-0-9]{10,}T[0-9:.]+(Z|[+-][0-9:]+)$''::text) IS TRUE))))'),
        ('public.authoring_promotions', 'authoring_promotions_stage_revision_valid', 'c'::"char", FALSE,
            'CHECK ((((stage = ''prepared''::text) AND (revision = 1)) OR ((stage = ''published''::text) AND (revision = 2)) OR ((stage = ''activation_pending''::text) AND (revision = 3)) OR ((stage = ''expired''::text) AND (revision = ANY (ARRAY[(3)::bigint, (4)::bigint])))))'),
        ('public.authoring_promotions', 'authoring_promotions_stage_valid', 'c'::"char", FALSE,
            'CHECK ((stage = ANY (ARRAY[''prepared''::text, ''published''::text, ''activation_pending''::text, ''expired''::text])))'),
        ('public.product_action_receipts', 'product_action_receipts_approval_key_identity_required', 'c'::"char", FALSE,
            'CHECK (((endpoint_domain <> ALL (ARRAY[''product_approve_v1''::text, ''product_apply_v1''::text, ''product_promote_v1''::text, ''product_reject_v1''::text, ''product_cancel_lifecycle_v1''::text])) OR ((idempotency_digest_key_id IS NOT NULL) AND (idempotency_digest_key_fingerprint IS NOT NULL))))'),
        ('public.activation_request_approvals', 'activation_request_approvals_pkey', 'p'::"char", TRUE,
            'PRIMARY KEY (request_id, approver_id)'),
        ('public.activation_request_approvals', 'activation_request_approvals_request_id_fkey', 'f'::"char", TRUE,
            'FOREIGN KEY (request_id) REFERENCES public.activation_requests(id) ON DELETE CASCADE'),
        ('public.activation_request_approvals', 'activation_request_approvals_product_parent_fk', 'f'::"char", TRUE,
            'FOREIGN KEY (tenant_id, installation_id, request_id) REFERENCES public.activation_requests(tenant_id, installation_id, id) ON DELETE CASCADE'),
        ('public.activation_request_approvals', 'activation_request_approvals_payload_digest_valid', 'c'::"char", FALSE,
            'CHECK (((approval_payload_digest IS NULL) OR (approval_payload_digest ~ ''^[0-9a-f]{64}$''::text)))'),
        ('public.activation_request_approvals', 'activation_request_approvals_product_scope_valid', 'c'::"char", FALSE,
            'CHECK (((((tenant_id IS NULL) AND (installation_id IS NULL) AND (approval_payload_digest IS NULL)) OR ((tenant_id IS NOT NULL) AND (installation_id IS NOT NULL) AND (approval_payload_digest IS NOT NULL) AND (tenant_id ~ ''^[A-Za-z0-9_.:-]{1,128}$''::text) AND (installation_id ~ ''^[A-Za-z0-9_.:-]{1,128}$''::text) AND (approval_payload_digest ~ ''^[0-9a-f]{64}$''::text))) IS TRUE))')
), constraint_contract AS (
    SELECT pg_catalog.count(*) = 9
        AND pg_catalog.bool_and(COALESCE(
            relation.oid IS NOT NULL
            AND constraint_row.oid IS NOT NULL
            AND constraint_row.contype = expected.constraint_type
            AND constraint_row.convalidated
            AND constraint_row.connoinherit = expected.no_inherit
            AND NOT constraint_row.condeferrable
            AND NOT constraint_row.condeferred
            AND constraint_row.conparentid = 0
            AND pg_catalog.pg_get_constraintdef(constraint_row.oid, FALSE)
                = expected.definition, FALSE)) AS valid
    FROM expected_constraints AS expected
    LEFT JOIN pg_catalog.pg_class AS relation
        ON relation.oid = pg_catalog.to_regclass(expected.relation_identity)
    LEFT JOIN pg_catalog.pg_constraint AS constraint_row
        ON constraint_row.conrelid = relation.oid
        AND constraint_row.conname = expected.constraint_name
), expected_indexes(
    relation_identity,
    index_identity,
    primary_index,
    definition,
    predicate
) AS (
    VALUES
        ('public.authoring_promotions', 'public.authoring_promotions_pkey', TRUE,
            'CREATE UNIQUE INDEX authoring_promotions_pkey ON public.authoring_promotions USING btree (id)', NULL::TEXT),
        ('public.authoring_promotions', 'public.authoring_promotions_product_scope_unique', FALSE,
            'CREATE UNIQUE INDEX authoring_promotions_product_scope_unique ON public.authoring_promotions USING btree (tenant_id, installation_id, id)', NULL::TEXT),
        ('public.automation_ruleset_heads', 'public.automation_ruleset_heads_pkey', TRUE,
            'CREATE UNIQUE INDEX automation_ruleset_heads_pkey ON public.automation_ruleset_heads USING btree (guild_id, ruleset_key)', NULL::TEXT),
        ('public.automation_ruleset_versions', 'public.automation_ruleset_versions_pkey', TRUE,
            'CREATE UNIQUE INDEX automation_ruleset_versions_pkey ON public.automation_ruleset_versions USING btree (guild_id, ruleset_key, version)', NULL::TEXT),
        ('public.automation_ruleset_versions', 'public.arv_hash_unique', FALSE,
            'CREATE UNIQUE INDEX arv_hash_unique ON public.automation_ruleset_versions USING btree (guild_id, ruleset_key, content_hash)', NULL::TEXT),
        ('public.automation_ruleset_activations', 'public.automation_ruleset_activations_pkey', TRUE,
            'CREATE UNIQUE INDEX automation_ruleset_activations_pkey ON public.automation_ruleset_activations USING btree (guild_id, ruleset_key)', NULL::TEXT),
        ('public.activation_requests', 'public.activation_requests_pkey', TRUE,
            'CREATE UNIQUE INDEX activation_requests_pkey ON public.activation_requests USING btree (id)', NULL::TEXT),
        ('public.activation_requests', 'public.activation_requests_product_scope_identity_unique', FALSE,
            'CREATE UNIQUE INDEX activation_requests_product_scope_identity_unique ON public.activation_requests USING btree (tenant_id, installation_id, id)', NULL::TEXT),
        ('public.activation_requests', 'public.activation_requests_one_product_request_per_promotion', FALSE,
            'CREATE UNIQUE INDEX activation_requests_one_product_request_per_promotion ON public.activation_requests USING btree (promotion_id) WHERE (authority_kind = ''product_authoring''::text)',
            '(authority_kind = ''product_authoring''::text)'),
        ('public.activation_request_approvals', 'public.activation_request_approvals_pkey', TRUE,
            'CREATE UNIQUE INDEX activation_request_approvals_pkey ON public.activation_request_approvals USING btree (request_id, approver_id)', NULL::TEXT),
        ('public.product_action_receipts', 'public.product_action_receipts_pkey', TRUE,
            'CREATE UNIQUE INDEX product_action_receipts_pkey ON public.product_action_receipts USING btree (receipt_id)', NULL::TEXT),
        ('public.product_action_receipts', 'public.product_action_receipts_idempotency_unique', FALSE,
            'CREATE UNIQUE INDEX product_action_receipts_idempotency_unique ON public.product_action_receipts USING btree (tenant_id, installation_id, principal_id, endpoint_domain, idempotency_key_digest)', NULL::TEXT),
        ('public.product_action_receipts', 'public.product_action_receipts_endpoint_scope_identity_unique', FALSE,
            'CREATE UNIQUE INDEX product_action_receipts_endpoint_scope_identity_unique ON public.product_action_receipts USING btree (tenant_id, installation_id, principal_id, endpoint_domain, receipt_id)', NULL::TEXT),
        ('public.product_action_receipt_idempotency_aliases', 'public.product_action_receipt_idempotency_aliases_primary', TRUE,
            'CREATE UNIQUE INDEX product_action_receipt_idempotency_aliases_primary ON public.product_action_receipt_idempotency_aliases USING btree (tenant_id, installation_id, principal_id, endpoint_domain, idempotency_key_digest)', NULL::TEXT),
        ('public.product_audit_events', 'public.product_audit_events_pkey', TRUE,
            'CREATE UNIQUE INDEX product_audit_events_pkey ON public.product_audit_events USING btree (event_id)', NULL::TEXT),
        ('public.product_audit_events', 'public.product_audit_events_receipt_unique', FALSE,
            'CREATE UNIQUE INDEX product_audit_events_receipt_unique ON public.product_audit_events USING btree (receipt_id)', NULL::TEXT),
        ('public.product_audit_events', 'public.product_audit_events_request_unique', FALSE,
            'CREATE UNIQUE INDEX product_audit_events_request_unique ON public.product_audit_events USING btree (tenant_id, request_id)', NULL::TEXT),
        ('public.product_action_receipt_audit_evidence', 'public.product_action_receipt_audit_evidence_pkey', TRUE,
            'CREATE UNIQUE INDEX product_action_receipt_audit_evidence_pkey ON public.product_action_receipt_audit_evidence USING btree (receipt_id)', NULL::TEXT),
        ('public.product_action_receipt_audit_evidence', 'public.product_action_receipt_audit_evidence_event_id_key', FALSE,
            'CREATE UNIQUE INDEX product_action_receipt_audit_evidence_event_id_key ON public.product_action_receipt_audit_evidence USING btree (event_id)', NULL::TEXT)
), index_contract AS (
    SELECT pg_catalog.count(*) = 19
        AND pg_catalog.bool_and(COALESCE(
            table_row.oid IS NOT NULL
            AND index_row.oid IS NOT NULL
            AND index_row.relkind = 'i'
            AND index_row.relpersistence = 'p'
            AND index_row.relowner = common_owner.owner_oid
            AND index_metadata.indrelid = table_row.oid
            AND index_metadata.indisunique
            AND index_metadata.indisprimary = expected.primary_index
            AND index_metadata.indisvalid
            AND index_metadata.indisready
            AND index_metadata.indislive
            AND index_metadata.indimmediate
            AND NOT index_metadata.indisclustered
            AND NOT index_metadata.indisreplident
            AND NOT index_metadata.indnullsnotdistinct
            AND index_metadata.indexprs IS NULL
            AND access_method.amname = 'btree'
            AND pg_catalog.pg_get_indexdef(index_metadata.indexrelid, 0, FALSE)
                = expected.definition
            AND pg_catalog.pg_get_expr(
                index_metadata.indpred,
                index_metadata.indrelid,
                FALSE
            ) IS NOT DISTINCT FROM expected.predicate, FALSE)) AS valid
    FROM expected_indexes AS expected
    CROSS JOIN common_owner
    LEFT JOIN pg_catalog.pg_class AS table_row
        ON table_row.oid = pg_catalog.to_regclass(expected.relation_identity)
    LEFT JOIN pg_catalog.pg_class AS index_row
        ON index_row.oid = pg_catalog.to_regclass(expected.index_identity)
    LEFT JOIN pg_catalog.pg_index AS index_metadata
        ON index_metadata.indexrelid = index_row.oid
    LEFT JOIN pg_catalog.pg_am AS access_method
        ON access_method.oid = index_row.relam
), default_privilege_contract AS (
    SELECT NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_default_acl AS defaults
        CROSS JOIN LATERAL pg_catalog.aclexplode(defaults.defaclacl) AS privilege
        WHERE defaults.defaclnamespace IN (
            0,
            pg_catalog.to_regnamespace('public')
        )
            AND privilege.grantee <> defaults.defaclrole
    ) AS valid
)
SELECT COALESCE((SELECT valid FROM capability_overload_contract), FALSE)
    AND COALESCE((SELECT valid FROM internal_contract), FALSE)
    AND COALESCE((SELECT valid FROM shared_helper_contract), FALSE)
    AND COALESCE((SELECT valid FROM shared_helper_overload_contract), FALSE)
    AND COALESCE((SELECT valid FROM trigger_manifest), FALSE)
    AND COALESCE((SELECT valid FROM trigger_helper_contract), FALSE)
    AND COALESCE((SELECT valid FROM trigger_helper_overload_contract), FALSE)
    AND COALESCE((SELECT valid FROM constraint_contract), FALSE)
    AND COALESCE((SELECT valid FROM index_contract), FALSE)
    AND COALESCE((SELECT valid FROM default_privilege_contract), FALSE)
"#;
