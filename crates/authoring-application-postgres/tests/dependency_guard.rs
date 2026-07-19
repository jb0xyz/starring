#[test]
fn pure_application_does_not_depend_on_postgres_adapter() {
    let manifest = include_str!("../../authoring-application/Cargo.toml");
    let regular = manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(manifest);
    assert!(!regular.contains("authoring-application-postgres"));
    assert!(!regular.contains("sqlx"));
}

#[test]
fn shared_migrators_watch_the_root_migration_history() {
    for build_script in [
        include_str!("../build.rs"),
        include_str!("../../automation-runtime-convergence-postgres/build.rs"),
    ] {
        assert!(build_script.contains("cargo:rerun-if-changed=../../migrations"));
    }
}

#[test]
fn adapter_uses_only_opaque_discord_identity_and_avoids_transport_or_foreign_decision_stores() {
    let manifest = include_str!("../Cargo.toml");
    let regular = manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(manifest);
    for forbidden in [
        "axum",
        "reqwest",
        "twilight",
        "automation-ruleset-activation-postgres",
        "authoring-promotion-postgres",
    ] {
        assert!(
            !regular.contains(forbidden),
            "forbidden dependency: {forbidden}"
        );
    }
    assert!(regular.contains("authoring-application-discord"));
    let source = include_str!("../src/lib.rs");
    assert!(source.contains("PostgresProductDecisions"));
    assert!(!source.contains("DiscordIdentityV1"));
    assert!(!source.contains("DiscordIdentityError"));
    let identity_model = include_str!("../src/product_identity/model.rs");
    assert!(!identity_model.contains("from_verified_oauth_exchange"));
    let identity_store = concat!(
        include_str!("../src/product_identity/store.rs"),
        include_str!("../src/product_identity/oauth_flow.rs"),
        include_str!("../src/product_identity/session_issue.rs"),
        include_str!("../src/product_identity/session_revoke.rs"),
        include_str!("../src/product_identity/principal.rs"),
        include_str!("../src/product_identity/database.rs"),
    );
    assert!(identity_store.contains("identity: VerifiedDiscordIdentityV1"));
    assert!(identity_store.contains("VerifiedIdentityProjection::from_capability"));
    assert!(!identity_store.contains("pub fn from_capability"));
}

#[test]
fn product_decision_adapter_keeps_atomic_security_and_idempotency_boundaries() {
    let approval = include_str!("../src/product_decisions/approve.rs");
    let apply = include_str!("../src/product_decisions/apply.rs");
    let database = include_str!("../src/product_decisions/database.rs");
    let digest = include_str!("../src/product_decisions/digest.rs");
    let config = include_str!("../src/product_decisions/config.rs");
    let query = include_str!("../src/product_decisions/query.rs");
    let store = include_str!("../src/product_decisions/store.rs");
    assert!(store.contains("ProductDecisionDatabasePoolsV1"));
    assert!(store.contains("decision_reader: PgPool"));
    assert!(store.contains("approval_executor: PgPool"));
    assert!(store.contains("apply_executor: PgPool"));
    assert!(query.contains(".decision_reader"));
    assert!(approval.contains(".approval_executor"));
    assert!(store.contains(".approval_executor"));
    assert!(apply.contains(".apply_executor"));
    assert!(approval.contains("public.starring_product_approve_v1"));
    assert!(database.contains("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE"));
    assert!(approval.contains("FreshDiscordAuthorityEvidenceV1"));
    assert!(approval.contains("request.session_fingerprint().as_bytes()"));
    assert!(!approval.contains(".bind(request.command().idempotency_key.as_str())"));
    assert!(digest.contains("Hmac<Sha256>"));
    assert!(digest.contains("IDEMPOTENCY_DOMAIN"));
    assert!(digest.contains("SEMANTIC_REQUEST_DOMAIN"));
    assert!(digest.contains("SESSION_SUBJECT_DOMAIN"));
    assert!(digest.contains("KEY_MATERIAL_FINGERPRINT_DOMAIN"));
    assert!(!digest.contains("request_id().as_str()"));
    assert!(config.contains("ConstantTimeEq"));
    assert!(config.contains("obvious_repetition"));
    let scope_migration =
        include_str!("../../../migrations/202607190004_scope_product_activations.sql");
    assert!(scope_migration.contains("authoring_promotions_product_scope_unique"));
    assert!(scope_migration.contains("activation_requests_product_scope_identity_unique"));
    assert!(scope_migration.contains("activation_requests_product_promotion_scope_fk"));
    assert!(
        scope_migration.contains("authoring promotion control-plane provisioning is incomplete")
    );
    assert!(scope_migration.contains("activation_request_approvals_product_parent_fk"));
    assert!(scope_migration.contains("runtime_deployments_activation_scope_fk"));
    let approval_migration =
        include_str!("../../../migrations/202607190005_guard_product_approval.sql");
    assert!(approval_migration.contains("SECURITY DEFINER"));
    assert!(approval_migration.contains("STRICT\nSECURITY DEFINER"));
    assert!(approval_migration.contains("SET search_path = pg_catalog"));
    assert!(approval_migration.contains("pg_catalog.pg_advisory_xact_lock"));
    assert!(approval_migration.contains("product_approve_v1:key-coverage"));
    assert!(approval_migration.contains("pg_catalog.unnest(idempotency_key_digest_candidates)"));
    assert!(approval_migration.contains("starring.product_approval_gate"));
    assert!(approval_migration.contains("activation approvals are append-only"));
    assert!(approval_migration.contains("FROM PUBLIC;"));
    assert!(approval_migration.contains("public.product_action_receipts"));
    assert!(approval_migration.contains("public.product_action_receipt_idempotency_aliases"));
    assert!(approval_migration.contains("product_action_receipts_assert_approval_alias"));
    assert!(approval_migration.contains("product_action_receipts_assert_approval_audit"));
    assert!(approval_migration.contains("idempotency_keyring_incomplete"));
    assert!(approval_migration.contains("idempotency_digest_key_fingerprint"));
    assert!(approval_migration.contains("public.product_audit_events"));
    assert!(approval_migration.contains("session_subject_digest"));
    assert!(
        approval_migration.contains("DROP CONSTRAINT product_audit_events_session_principal_fk")
    );
    assert!(!approval_migration.contains("idempotency_key TEXT"));
    assert!(approval
        .contains("database_commit(error, \"product approval commit outcome is unavailable\")"));
    assert!(database.contains("fn commit_outcome_is_uncertain"));
    assert!(database.contains("code.as_ref().starts_with(\"08\")"));
    assert!(database.contains("if commit_outcome_is_uncertain(&error)"));
    let approval_scope_migration =
        include_str!("../../../migrations/202607190019_scope_product_approval_execution.sql");
    for required in [
        "starring_product_decision_reader_database_identity_v1",
        "starring_product_approval_executor_database_identity_v1",
        "starring_product_apply_executor_database_identity_v1",
        "product approval relations require one non-RLS owner",
        "function_row.provolatile <> 'v'",
        "function_row.proparallel <> 'u'",
        "function_row.proconfig <> ARRAY['search_path=pg_catalog']::TEXT[]",
        "REVOKE ALL PRIVILEGES ON FUNCTION %s FROM %I CASCADE",
        "ALTER FUNCTION %s OWNER TO %I",
        "RESET ALL",
        "ROWS 1",
        "FROM PUBLIC;",
    ] {
        assert!(
            approval_scope_migration.contains(required),
            "missing product approval scope guard: {required}"
        );
    }
    let binding_identity_migration =
        include_str!("../../../migrations/202607190009_separate_product_binding_identities.sql");
    for required in [
        "CREATE OR REPLACE FUNCTION public.starring_product_approve_v1",
        "#>> '{intent,evidence,context_fingerprint}'",
        "AS historical_authority",
        "product_action_receipt_audit_evidence AS evidence",
        "activation_row.approval_context -> 'context'",
        "pg_catalog.set_config('starring.product_approval_gate', '', TRUE)",
        "STRICT\nSECURITY DEFINER",
        "SET search_path = pg_catalog",
        "FROM PUBLIC",
    ] {
        assert!(
            binding_identity_migration.contains(required),
            "missing binding identity guard: {required}"
        );
    }
    assert!(!binding_identity_migration.contains(
        "authority_row.binding_fingerprint\n            IS DISTINCT FROM activation_row.approval_context #>> '{context,binding,fingerprint}'"
    ));
    let apply_drift_migration =
        include_str!("../../../migrations/202607190010_persist_product_apply_drift.sql");
    for required in [
        "RENAME TO starring_product_apply_lock_core_v1",
        "CREATE FUNCTION public.starring_product_apply_lock_v1",
        "STRICT\nSECURITY DEFINER",
        "SET search_path = pg_catalog",
        "pg_catalog.set_config(\n        'starring.product_approval_context_digest'",
        "'superseded_baseline_drift'",
        "'superseded_binding_drift'",
        "'superseded_policy_drift'",
        "historical_authority_row public.automation_installation_authority_versions%ROWTYPE",
        "installation_row.current_authority_revision\n                    IS DISTINCT FROM expected_authority_revision",
        "public.product_action_receipt_audit_evidence AS evidence",
        "UPDATE public.activation_requests AS activation\n    SET state = 'superseded'",
        "pg_catalog.aclexplode(",
        "pg_catalog.array_agg(grant_row.grantee ORDER BY grant_row.grantee)",
        "ALTER FUNCTION %s OWNER TO %I",
        "GRANT EXECUTE ON FUNCTION %s TO %I%s",
        "REVOKE ALL PRIVILEGES ON FUNCTION %s FROM %I CASCADE",
        "FROM PUBLIC",
    ] {
        assert!(
            apply_drift_migration.contains(required),
            "missing product apply drift guard: {required}"
        );
    }
    assert!(!apply_drift_migration.contains("INSERT INTO public.runtime_deployments"));
    let slot_migration =
        include_str!("../../../migrations/202607190013_own_product_ruleset_slots.sql");
    for required in [
        "product_activation_applying_residue_absent",
        "product_activation_applied_record_immutable",
        "product_ruleset_slot_legacy_apply_absent",
        "product_ruleset_slot_pointer_exact",
        "product_ruleset_slot_pointer_delete_forbidden",
        "product_ruleset_slot_legacy_apply_forbidden",
        "starring_product_ruleset_slot_exact_v1",
        "activation_requests_guard_legacy_product_slot",
        "activation_requests_assert_no_product_applying",
        "activation_requests_guard_product_applied_record",
        "automation_installations_lock_ruleset_slot_takeover",
        "automation_installations_assert_ruleset_slot_takeover",
        "automation_ruleset_activations_assert_product_slot",
        "pg_catalog.pg_advisory_xact_lock",
        "SECURITY DEFINER",
        "SET search_path = pg_catalog",
        "FROM PUBLIC",
    ] {
        assert!(
            slot_migration.contains(required),
            "missing product RuleSet slot guard: {required}"
        );
    }
    assert!(!slot_migration.contains("current_setting("));
    let activation_store =
        include_str!("../../automation-ruleset-activation-postgres/src/store.rs");
    let ruleset_store = include_str!("../../automation-ruleset-postgres/src/lib.rs");
    for source in [slot_migration, activation_store, ruleset_store] {
        assert!(source.contains("starring.ruleset-slot.v1:"));
    }
    assert_eq!(
        activation_store
            .matches("product activation requires authenticated product control")
            .count(),
        1
    );
}

#[test]
fn product_identity_retention_keeps_index_bounded_work_shape() {
    let migration =
        include_str!("../../../migrations/202607190006_prepare_product_identity_retention.sql");
    for index in [
        "product_auth_sessions_terminal_retention_index",
        "product_oauth_flows_consumed_retention_index",
        "product_oauth_flows_unconsumed_retention_index",
    ] {
        assert!(migration.contains(index));
    }
    assert!(migration.contains("WITH candidates AS MATERIALIZED"));
    assert!(migration.contains("product_session.ctid AS row_id"));
    assert!(migration.contains("FOR UPDATE OF product_session SKIP LOCKED"));
    assert!(migration.contains("WITH unconsumed_candidates AS MATERIALIZED"));
    assert!(migration.contains("consumed_candidates AS MATERIALIZED"));
    assert_eq!(
        migration
            .matches("FOR UPDATE OF oauth_flow SKIP LOCKED")
            .count(),
        2
    );
    assert_eq!(migration.matches("OFFSET 0").count(), 4);
    assert_eq!(migration.matches("ctid = ANY(").count(), 2);
    assert!(migration.contains("session_backlog AS MATERIALIZED"));
    assert!(migration.contains("unconsumed_flow_backlog AS MATERIALIZED"));
    assert!(migration.contains("consumed_flow_backlog AS MATERIALIZED"));
    assert!(!migration.contains("INNER JOIN bounded_candidates"));
    let adapter = include_str!("../src/product_retention.rs");
    assert!(adapter.contains("MAX_BATCH_LIMIT: u32 = 1000"));
    assert!(adapter.contains("set_config('statement_timeout', $1, TRUE)"));
    assert!(adapter.contains("set_config('lock_timeout', $1, TRUE)"));
    assert!(adapter.contains("transaction.commit().await.map_err(database_commit)?"));
    assert!(adapter.contains("matches!(&error, sqlx::Error::Database(_))"));
}

#[test]
fn product_action_retention_preserves_evidence_and_bounded_work_shape() {
    let migration = include_str!(
        "../../../migrations/202607190007_prepare_product_action_receipt_retention.sql"
    );
    for required in [
        "CREATE TABLE public.product_action_receipt_audit_evidence",
        "product_audit_events_receipt_evidence_fk",
        "product_audit_events_principal_fk",
        "product_action_receipt_audit_evidence_reject_mutation",
        "product_action_receipts_approval_retention_index",
        "product_action_aliases_receipt_retention_index",
        "starring_purge_product_action_receipts_v1",
        "starring_product_approval_keyring_coverage_v1",
        "FOR UPDATE OF receipt SKIP LOCKED",
        "batch_limit NOT BETWEEN 1 AND 1000",
        "existing_alias_count >= 32",
        "SECURITY DEFINER",
        "SET search_path = pg_catalog",
        "FROM PUBLIC",
    ] {
        assert!(migration.contains(required), "missing {required}");
    }
    assert!(migration.contains("DROP CONSTRAINT product_audit_events_receipt_fk"));
    assert!(migration.contains("completed_at + INTERVAL '168 hours'"));
    assert!(migration.contains("replay_guaranteed_until <= retention_clock"));
    assert!(migration
        .contains("REVOKE ALL ON TABLE public.product_action_receipt_audit_evidence\nFROM PUBLIC"));
    let alias_delete = migration
        .find("DELETE FROM public.product_action_receipt_idempotency_aliases")
        .unwrap();
    let receipt_delete = migration
        .find("DELETE FROM public.product_action_receipts")
        .unwrap();
    assert!(alias_delete < receipt_delete);
    assert!(!migration.contains("idempotency_key TEXT"));
    let adapter = include_str!("../src/product_retention.rs");
    assert!(adapter.contains("PostgresProductActionRetention"));
    assert!(adapter.contains("starring_purge_product_action_receipts_v1"));
    assert!(adapter.contains("deleted_receipts.saturating_mul(32)"));
    assert!(adapter.contains("transaction.commit().await.map_err(action_database_commit)?"));
    assert!(adapter.contains("matches!(&error, sqlx::Error::Database(_))"));
}

#[test]
fn authentication_and_snapshot_transactions_keep_their_security_shape() {
    let authentication = include_str!("../src/authentication.rs");
    assert!(authentication.contains("digest_opaque_session_credential_v1(credential)"));
    assert!(authentication.contains("impl MutationAuthenticationPort for PostgresAuthentication"));
    assert!(authentication
        .contains("load_active_product_session(&self.pool, self.config, credential, Some(csrf))"));
    assert!(authentication
        .contains("SessionValidationError::InvalidCsrf => AuthenticationError::InvalidCsrf"));
    assert!(authentication.contains("public.starring_product_session_read_v1($1)"));
    assert!(authentication.contains("public.starring_product_session_mutation_read_v1($1)"));
    assert!(authentication.contains("public.starring_product_session_touch_v1($1, $2, $3, $4, $5)"));
    assert!(authentication.contains("csrf_comparison_tag"));
    assert!(authentication.contains("SessionProofModeV1::SessionOnly"));
    assert!(authentication.contains("SessionProofModeV1::Mutation"));
    assert!(authentication.contains("touch_active_product_session"));
    assert!(authentication.contains("SELECT pg_catalog.clock_timestamp()"));
    assert!(!authentication.contains("public.product_auth_sessions"));
    assert!(!authentication.contains("public.product_principals"));
    assert!(authentication.contains("pg_catalog.set_config("));
    assert!(!authentication.contains(".bind(credential)"));
    let snapshot = include_str!("../src/snapshot.rs");
    assert!(snapshot.contains("SET TRANSACTION ISOLATION LEVEL READ COMMITTED, READ ONLY"));
    assert!(snapshot.contains("evidence.installation_authority_revision().get()"));
    assert!(snapshot.contains("evidence.installation_authority_digest()"));
    assert!(snapshot.contains("actor.session_fingerprint().as_bytes()"));
    assert!(snapshot.contains("actor.principal_id().as_str()"));
    assert!(snapshot.contains(".bind(scope.tenant_id().as_str())"));
    assert!(snapshot.contains(".bind(scope.installation_id().as_str())"));
    assert!(snapshot
        .contains("public.starring_product_authorized_snapshot_read_v1($1, $2, $3, $4, $5)"));
    for forbidden in [
        "public.authoring_sessions",
        "public.product_principals",
        "public.product_auth_sessions",
        "public.product_tenants",
        "public.automation_installations",
        "public.authoring_session_generations",
        "public.automation_installation_authority_versions",
    ] {
        assert!(!snapshot.contains(forbidden));
    }
    assert!(snapshot.contains("pg_catalog.set_config("));
    assert!(snapshot.contains("lock_timeout"));
    assert!(snapshot.contains("idle_in_transaction_session_timeout"));
    assert!(!snapshot.contains("map_err(|error| session_backend(error.to_string()))"));
    let snapshot_scope_migration =
        include_str!("../../../migrations/202607190016_scope_authorized_snapshot_reads.sql");
    for required in [
        "CREATE FUNCTION public.starring_product_authorized_snapshot_read_v1(",
        "VOLATILE\nSTRICT\nPARALLEL UNSAFE\nSECURITY DEFINER",
        "SET search_path = pg_catalog",
        "WITH request_clock AS MATERIALIZED",
        "pg_catalog.clock_timestamp()",
        "actor_session.session_digest = expected_product_session_digest",
        "actor_session.principal_id = principal.principal_id",
        "pg_catalog.octet_length(actor_session.oauth_state_digest) = 32",
        "actor_session.revoked_at IS NULL",
        "actor_session.authenticated_at = actor_session.created_at",
        "actor_session.last_seen_at <= request_clock.database_now",
        "actor_session.idle_expires_at\n            <= actor_session.last_seen_at + INTERVAL '30 minutes'",
        "actor_session.absolute_expires_at\n            <= actor_session.authenticated_at + INTERVAL '12 hours'",
        "LEFT JOIN public.authoring_session_generations",
        "LEFT JOIN public.automation_installation_authority_versions",
        "relation_count <> 7",
        "table_count <> 7",
        "rls_disabled_count <> 7",
        "owner_count <> 1",
        "pg_catalog.aclexplode(COALESCE(",
        "REVOKE ALL PRIVILEGES ON FUNCTION",
        "ALTER FUNCTION public.starring_product_authorized_snapshot_read_v1",
        ") FROM PUBLIC;",
    ] {
        assert!(
            snapshot_scope_migration.contains(required),
            "missing authorized snapshot scope guard: {required}"
        );
    }
    for forbidden in [
        "actor_session.csrf_digest,",
        "actor_session.oauth_state_digest,",
        "generation.summary",
        "generation.writer_request_digest",
        "authority.created_by_request_digest",
    ] {
        assert!(!snapshot_scope_migration.contains(forbidden));
    }
    let snapshot_readiness = include_str!("../src/snapshot/readiness.rs");
    for required in [
        "FUNCTION_IDENTITY",
        "FUNCTION_RESULT",
        "ScopedFunctionContractV1::set(",
        "RELATIONS: [ScopedRelationContractV1<'static>; 7]",
        "ScopedRelationContractV1::ordinary_without_rls",
        "begin_scoped_database_readiness(",
        "public.starring_product_authorized_snapshot_read_v1(",
        "PROBE_DIGEST: [u8; 31]",
    ] {
        assert!(
            snapshot_readiness.contains(required),
            "missing authorized snapshot readiness guard: {required}"
        );
    }
    let installation_authority = include_str!("../src/installation_authority.rs");
    assert!(installation_authority
        .contains("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY"));
    assert!(installation_authority.contains("pg_catalog.set_config('statement_timeout'"));
    assert!(installation_authority.contains("idle_in_transaction_session_timeout"));
    assert!(installation_authority
        .contains("public.starring_product_installation_authority_read_v1($1, $2, $3)"));
    assert!(installation_authority.contains("oauth_state_digest_length"));
    assert!(installation_authority.contains("fn active_lifecycle("));
    assert!(installation_authority.contains("persisted_session_digest"));
    assert!(installation_authority.contains(".ct_eq(actor.session_fingerprint().as_bytes())"));
    assert!(!installation_authority.contains("MAX("));
    assert!(!installation_authority.contains("created_by_principal_id"));
    assert!(!installation_authority.contains("sqlx::Error::to_string"));
    assert!(!installation_authority.contains("FOR SHARE"));
    assert!(!installation_authority.contains("FOR UPDATE"));
    for forbidden in [
        "public.product_principals",
        "public.product_auth_sessions",
        "public.product_tenants",
        "public.automation_installations",
        "public.automation_installation_authority_versions",
    ] {
        assert!(!installation_authority.contains(forbidden));
    }
    let authority_read_migration =
        include_str!("../../../migrations/202607190014_scope_installation_authority_reads.sql");
    for required in [
        "CREATE FUNCTION public.starring_product_installation_authority_read_v1(",
        "VOLATILE\nSTRICT\nPARALLEL UNSAFE\nSECURITY DEFINER",
        "SET search_path = pg_catalog",
        "pg_catalog.clock_timestamp()",
        "actor_session.session_digest = expected_product_session_digest",
        "principal.disabled AS principal_disabled",
        "pg_catalog.octet_length(actor_session.oauth_state_digest)",
        "actor_session.revoked_at",
        "tenant.lifecycle_state AS tenant_lifecycle_state",
        "installation.lifecycle_state AS installation_lifecycle_state",
        "authority.revision = installation.current_authority_revision",
        "authority.authority_payload_digest",
        "relation_count <> 5",
        "table_count <> 5",
        "owner_count <> 1",
        "pg_catalog.aclexplode(COALESCE(",
        "privilege.grantee <> function_row.proowner",
        "REVOKE ALL PRIVILEGES ON FUNCTION",
        "ALTER FUNCTION public.starring_product_installation_authority_read_v1",
        ") FROM PUBLIC;",
    ] {
        assert!(
            authority_read_migration.contains(required),
            "missing installation authority read guard: {required}"
        );
    }
    assert!(!authority_read_migration.contains("created_by_principal_id"));
    assert!(!authority_read_migration.contains("MAX("));
    let authority_readiness = include_str!("../src/installation_authority/readiness.rs");
    for required in [
        "FUNCTION_IDENTITY",
        "FUNCTION_RESULT",
        "ScopedFunctionContractV1::set(",
        "ScopedRelationContractV1::ordinary(",
        "begin_scoped_database_readiness(",
        "public.starring_product_installation_authority_read_v1($1, $2, $3)",
    ] {
        assert!(
            authority_readiness.contains(required),
            "missing installation authority readiness guard: {required}"
        );
    }
    let database_capability = include_str!("../src/database_capability.rs");
    for required in [
        "pg_catalog.to_regprocedure($1)",
        "pg_catalog.pg_get_function_result(function_row.oid)",
        "function_contract.proconfig = ARRAY['search_path=pg_catalog']::TEXT[]",
        "pg_catalog.aclexplode(COALESCE(",
        "privilege.is_grantable",
        "pg_catalog.has_table_privilege(",
        "pg_catalog.has_any_column_privilege(",
        "pg_catalog.pg_auth_members",
        "membership.roleid = caller_role.oid",
        "membership.roleid = owner_role.oid",
        "current_user = session_user",
        "unexpected_function_grant",
        "caller_role.rolbypassrls",
        "owner_role.rolcanlogin",
        "pg_catalog.has_schema_privilege(",
        "owner_role.rolname, target.schema_oid, 'USAGE'",
        "relation.relrowsecurity",
        "relation.relforcerowsecurity",
    ] {
        assert!(
            database_capability.contains(required),
            "missing scoped database readiness guard: {required}"
        );
    }
    let identity = concat!(
        include_str!("../src/product_identity/store.rs"),
        include_str!("../src/product_identity/oauth_flow.rs"),
        include_str!("../src/product_identity/session_issue.rs"),
        include_str!("../src/product_identity/session_revoke.rs"),
        include_str!("../src/product_identity/principal.rs"),
        include_str!("../src/product_identity/database.rs"),
    );
    assert!(identity.contains("digest_opaque_session_credential_v1(state)"));
    assert!(identity.contains("digest_opaque_session_credential_v1(browser_nonce)"));
    assert!(identity.contains("digest_opaque_session_credential_v1(session.expose_secret())"));
    assert!(identity.contains("digest_opaque_session_credential_v1(csrf.expose_secret())"));
    assert!(!identity.contains(".bind(state)"));
    assert!(!identity.contains(".bind(browser_nonce)"));
    assert!(!identity.contains(".bind(credential)"));
    assert!(!identity.contains(".bind(csrf)"));
    assert!(identity.contains("starring_product_oauth_flow_create_v1"));
    assert!(identity.contains("starring_product_oauth_flow_consume_v1"));
    assert!(identity.contains("starring_product_session_issue_v1"));
    assert!(identity.contains("starring_product_session_logout_read_v1"));
    assert!(identity.contains("starring_product_session_logout_commit_v1"));
    assert!(identity.contains("starring_product_session_security_revoke_v1"));
    assert!(identity.contains("begin_bounded_identity_transaction"));
    assert!(identity.contains("SET TRANSACTION ISOLATION LEVEL READ COMMITTED, READ WRITE"));
    assert!(identity.contains("set_config('statement_timeout'"));
    assert!(identity.contains("set_config('lock_timeout'"));
    assert!(identity.contains("set_config('idle_in_transaction_session_timeout'"));
    assert!(identity.contains("self.pools.oauth_flow_writer"));
    assert!(identity.contains("self.pools.session_issuer"));
    assert!(identity.contains("self.pools.session_api"));
    assert!(identity.contains("self.pools.security_revoker"));
    assert!(!identity.contains("validate_consumed_flow"));
    assert!(!identity.contains("upsert_principal"));
    assert!(!identity.contains("unique_violation"));
    assert!(authentication.contains("persisted_tag.ct_eq(&expected_tag)"));
    assert!(identity.contains("pub async fn current_principal("));
    assert!(identity.contains("credential,\n            None,"));
    assert!(identity.contains("pub async fn verify_csrf("));
    assert!(identity.contains("credential,\n            Some(csrf),"));
    assert!(identity.contains("persisted_tag.ct_eq(&expected_tag)"));
    assert!(identity.contains("ProductLogoutDispositionV1::ExactReplay"));
    let session_issue = include_str!("../src/product_identity/session_issue.rs");
    assert_eq!(
        session_issue
            .matches(".execute_session_issue_attempt(")
            .count(),
        2
    );
    for required in [
        "Err(ProductIdentityError::CommitIndeterminate) => match self",
        "_ => return Err(ProductIdentityError::CommitIndeterminate)",
        "self.commit_issued_session(transaction).await?",
        "if exact_replay {",
        "let _ = transaction.rollback().await",
    ] {
        assert!(
            session_issue.contains(required),
            "missing bounded session issue reconciliation guard: {required}"
        );
    }
    assert!(!session_issue.contains("Err(ProductIdentityError::CommitIndeterminate) => continue"));
    assert!(!identity.contains("public.product_oauth_flows"));
    assert!(!identity.contains("public.product_auth_sessions"));
    assert!(!identity.contains("public.product_principals"));
    assert!(!identity.contains("pg_catalog.clock_timestamp()"));
    assert!(!identity.contains("pg_catalog.make_interval("));
    let identity_scope_migration =
        include_str!("../../../migrations/202607190017_scope_product_identity_lifecycle.sql");
    for required in [
        "CREATE TABLE public.product_control_plane_identity (",
        "pg_catalog.gen_random_uuid()",
        "CREATE FUNCTION public.starring_product_oauth_database_identity_v1()",
        "CREATE FUNCTION public.starring_product_session_issuer_database_identity_v1()",
        "CREATE FUNCTION public.starring_product_session_api_database_identity_v1()",
        "CREATE FUNCTION public.starring_product_security_revoker_database_identity_v1()",
        "CREATE FUNCTION public.starring_product_oauth_flow_create_v1(",
        "CREATE FUNCTION public.starring_product_oauth_flow_consume_v1(",
        "CREATE FUNCTION public.starring_product_session_issue_v1(",
        "CREATE FUNCTION public.starring_product_session_logout_read_v1(",
        "CREATE FUNCTION public.starring_product_session_logout_commit_v1(",
        "CREATE FUNCTION public.starring_product_session_security_revoke_v1(",
        "VOLATILE\nSTRICT\nPARALLEL UNSAFE\nSECURITY DEFINER",
        "SET search_path = pg_catalog",
        "FOR UPDATE",
        "pg_catalog.clock_timestamp()",
        "ON CONFLICT ON CONSTRAINT product_principals_discord_user_id_key",
        "product_auth_sessions_oauth_state_unique",
        "pg_catalog.sha256(pg_catalog.byteacat(",
        "relation_count <> 3",
        "rls_disabled_count <> 3",
        "pg_catalog.aclexplode(COALESCE(",
        "NULLIF(attribute.attacl, '{}'::ACLITEM[])",
        "ALTER TABLE %s OWNER TO %I",
        "starring_purge_product_identity_v1(INTEGER)",
        "ALTER FUNCTION %s OWNER TO %I",
        "FROM PUBLIC;",
    ] {
        assert!(
            identity_scope_migration.contains(required),
            "missing product identity lifecycle guard: {required}"
        );
    }
    let session_issue_reconciliation_migration =
        include_str!("../../../migrations/202607190018_reconcile_product_session_issue.sql");
    for required in [
        "CREATE OR REPLACE FUNCTION public.starring_product_session_issue_v1(",
        "VOLATILE\nSTRICT\nPARALLEL UNSAFE\nSECURITY DEFINER",
        "SET search_path = pg_catalog",
        "locked_flow.consumed_at > existing_session.authenticated_at",
        "existing_session.authenticated_at >= locked_flow.expires_at",
        "existing_session.authenticated_at > issue_now",
        "existing_session.session_digest <> new_session_digest",
        "existing_session.csrf_digest <> new_csrf_digest",
        "existing_session.principal_id <> canonical_principal_id",
        "existing_session.revoked_at IS NOT NULL",
        "existing_session.revocation_reason IS NOT NULL",
        "pg_catalog.make_interval(secs => idle_lifetime_seconds)",
        "pg_catalog.make_interval(secs => absolute_lifetime_seconds)",
        "pg_catalog.aclexplode(COALESCE(",
        "pg_catalog.acldefault('f', function_row.proowner)",
        "ALTER FUNCTION %s OWNER TO %I",
        "REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE",
        "function_row.proconfig = ARRAY['search_path=pg_catalog']::TEXT[]",
    ] {
        assert!(
            session_issue_reconciliation_migration.contains(required),
            "missing product session issue reconciliation guard: {required}"
        );
    }
    let exact_session_lookup = session_issue_reconciliation_migration
        .find("SELECT authentication_session.*")
        .expect("exact session lookup");
    let absent_session_expiry_gate = session_issue_reconciliation_migration
        .find("IF issue_now >= locked_flow.expires_at THEN")
        .expect("absent session expiry gate");
    let exact_replay_outcome = session_issue_reconciliation_migration
        .find("RETURN QUERY SELECT 'exact_replay'::TEXT")
        .expect("exact replay outcome");
    let exact_replay_contract =
        &session_issue_reconciliation_migration[exact_session_lookup..exact_replay_outcome];
    for required in [
        "locked_flow.consumed_at > existing_session.authenticated_at",
        "existing_session.authenticated_at >= locked_flow.expires_at",
        "existing_session.session_digest <> new_session_digest",
        "existing_session.csrf_digest <> new_csrf_digest",
        "existing_session.principal_id <> canonical_principal_id",
    ] {
        assert!(exact_replay_contract.contains(required));
    }
    assert!(exact_session_lookup < exact_replay_outcome);
    assert!(exact_replay_outcome < absent_session_expiry_gate);
    assert!(!session_issue_reconciliation_migration
        .contains("locked_flow.consumed_at > issue_now OR issue_now >= locked_flow.expires_at"));
    let authentication_scope_migration =
        include_str!("../../../migrations/202607190015_scope_product_authentication.sql");
    for required in [
        "CREATE FUNCTION public.starring_product_session_read_v1(",
        "CREATE FUNCTION public.starring_product_session_mutation_read_v1(",
        "CREATE FUNCTION public.starring_product_session_touch_v1(",
        "VOLATILE\nSTRICT\nPARALLEL UNSAFE\nSECURITY DEFINER",
        "SET search_path = pg_catalog",
        "FOR SHARE OF authentication_session, principal",
        "pg_catalog.sha256(pg_catalog.byteacat(",
        "csrf_digest_length",
        "oauth_state_digest_length",
        "touch_interval_seconds >= 1",
        "observed_idle_expires_at - observed_last_seen_at",
        "touch_clock.touched_at >= authentication_session.last_seen_at",
        "relation.relrowsecurity",
        "relation.relforcerowsecurity",
        "pg_catalog.aclexplode(COALESCE(",
        "REVOKE ALL PRIVILEGES ON FUNCTION",
        "ALTER FUNCTION %s OWNER TO %I",
        "FROM PUBLIC;",
    ] {
        assert!(
            authentication_scope_migration.contains(required),
            "missing authentication scope guard: {required}"
        );
    }
    assert!(!authentication_scope_migration.contains("authentication_session.csrf_digest,"));
    let authentication_readiness = include_str!("../src/authentication/readiness.rs");
    for required in [
        "FUNCTIONS: [ScopedFunctionContractV1<'static>; 3]",
        "ScopedRelationContractV1::ordinary_without_rls",
        "begin_scoped_database_readiness(",
        "ScopedDatabaseProbeModeV1::ReadWrite",
        "public.starring_product_session_read_v1($1)",
        "public.starring_product_session_mutation_read_v1($1)",
        "public.starring_product_session_touch_v1(",
        "write_transaction\n            .rollback()",
    ] {
        assert!(
            authentication_readiness.contains(required),
            "missing authentication readiness guard: {required}"
        );
    }
    let identity_readiness = include_str!("../src/product_identity/readiness.rs");
    for required in [
        "verify_oauth_flow_writer_readiness",
        "verify_session_issuer_readiness",
        "verify_session_api_readiness",
        "verify_security_revoker_readiness",
        "const IDENTITY_RELATIONS",
        "starring_product_oauth_database_identity_v1",
        "starring_product_session_issuer_database_identity_v1",
        "starring_product_session_api_database_identity_v1",
        "starring_product_security_revoker_database_identity_v1",
        "load_scoped_database_topology",
        "verify_same_database_distinct_roles",
        "current_database()::TEXT",
        "ScopedFunctionContractV1::set_plpgsql",
        "ScopedRelationContractV1::ordinary_without_rls",
        "begin_scoped_database_readiness(",
        "ScopedDatabaseProbeModeV1::ReadWrite",
        "probe.rollback()",
    ] {
        assert!(
            identity_readiness.contains(required),
            "missing product identity readiness guard: {required}"
        );
    }
    assert_eq!(identity_readiness.matches("&IDENTITY_RELATIONS").count(), 4);
    for required in [
        "canonical_database_identity",
        "load_scoped_database_topology",
        "verify_same_database_distinct_roles",
    ] {
        assert!(
            database_capability.contains(required),
            "missing shared database topology guard: {required}"
        );
    }
    for required in [
        "unexpected_relation_grant",
        "pg_catalog.pg_attribute",
        "NULLIF(attribute.attacl, '{}'::ACLITEM[])",
    ] {
        assert!(
            database_capability.contains(required),
            "missing global relation ACL guard: {required}"
        );
    }
    for (path, source) in [
        ("src/authentication.rs", authentication),
        ("src/installation_authority.rs", installation_authority),
        ("src/snapshot.rs", snapshot),
        ("src/product_identity/store.rs", identity),
    ] {
        for forbidden in [
            "FROM product_",
            "JOIN product_",
            "INTO product_",
            "UPDATE product_",
            "DELETE FROM product_",
            "FROM authoring_",
            "JOIN authoring_",
            "INTO authoring_",
            "UPDATE authoring_",
            "DELETE FROM authoring_",
            "FROM automation_",
            "JOIN automation_",
            "INTO automation_",
            "UPDATE automation_",
            "DELETE FROM automation_",
        ] {
            assert!(
                !source.contains(forbidden),
                "unqualified SQL in {path}: {forbidden}"
            );
        }
        for function in ["clock_timestamp(", "make_interval(", "set_config("] {
            for (index, _) in source.match_indices(function) {
                let qualifier_start = index.checked_sub("pg_catalog.".len()).unwrap();
                assert_eq!(
                    &source[qualifier_start..index],
                    "pg_catalog.",
                    "unqualified SQL function in {path}: {function}"
                );
            }
        }
    }
    let migration =
        include_str!("../../../migrations/202607190003_bind_product_sessions_to_oauth_flows.sql");
    assert!(!migration.contains("ALTER COLUMN oauth_state_digest SET NOT NULL"));
    assert!(migration.contains("revocation_reason = 'oauth_rebinding_required'"));
    assert!(migration.contains("product_auth_sessions_oauth_binding_presence"));
    assert!(migration.contains("product_oauth_flows_lifetime_bounded"));
    assert!(migration.contains(") NOT VALID;"));
    assert!(migration.contains("SET search_path = pg_catalog"));
    assert!(migration.contains("product_auth_sessions_idle_lifetime_bounded"));
    assert!(migration.contains("product_auth_sessions_initial_activity_valid"));
    assert!(migration.contains("NEW.authenticated_at IS DISTINCT FROM NEW.created_at"));
    assert!(migration.contains("NEW.last_seen_at IS DISTINCT FROM NEW.authenticated_at"));
    assert!(migration.contains("NEW.authenticated_at > pg_catalog.clock_timestamp()"));
    assert!(
        migration.contains("NEW.idle_expires_at > NEW.authenticated_at + INTERVAL '30 minutes'")
    );
    assert!(migration.contains("NEW.last_seen_at > pg_catalog.clock_timestamp()"));
    assert!(migration.contains("idle_expires_at <= last_seen_at + INTERVAL '30 minutes'"));
}

#[test]
fn source_files_contain_no_comments() {
    let sources = [
        (
            "src/authentication.rs",
            include_str!("../src/authentication.rs"),
        ),
        (
            "src/authentication/readiness.rs",
            include_str!("../src/authentication/readiness.rs"),
        ),
        ("src/bindings.rs", include_str!("../src/bindings.rs")),
        ("src/database.rs", include_str!("../src/database.rs")),
        (
            "src/database_capability.rs",
            include_str!("../src/database_capability.rs"),
        ),
        (
            "src/deployment_status.rs",
            include_str!("../src/deployment_status.rs"),
        ),
        ("src/digest.rs", include_str!("../src/digest.rs")),
        ("src/envelope.rs", include_str!("../src/envelope.rs")),
        (
            "src/installation_authority.rs",
            include_str!("../src/installation_authority.rs"),
        ),
        (
            "src/installation_authority/readiness.rs",
            include_str!("../src/installation_authority/readiness.rs"),
        ),
        ("src/lib.rs", include_str!("../src/lib.rs")),
        (
            "src/product_decisions/apply.rs",
            include_str!("../src/product_decisions/apply.rs"),
        ),
        (
            "src/product_decisions/apply_projection.rs",
            include_str!("../src/product_decisions/apply_projection.rs"),
        ),
        (
            "src/product_decisions/apply_sql.rs",
            include_str!("../src/product_decisions/apply_sql.rs"),
        ),
        (
            "src/product_decisions/approve.rs",
            include_str!("../src/product_decisions/approve.rs"),
        ),
        (
            "src/product_decisions/config.rs",
            include_str!("../src/product_decisions/config.rs"),
        ),
        (
            "src/product_decisions/database.rs",
            include_str!("../src/product_decisions/database.rs"),
        ),
        (
            "src/product_decisions/digest.rs",
            include_str!("../src/product_decisions/digest.rs"),
        ),
        (
            "src/product_decisions/mod.rs",
            include_str!("../src/product_decisions/mod.rs"),
        ),
        (
            "src/product_decisions/query.rs",
            include_str!("../src/product_decisions/query.rs"),
        ),
        (
            "src/product_decisions/row.rs",
            include_str!("../src/product_decisions/row.rs"),
        ),
        (
            "src/product_decisions/store.rs",
            include_str!("../src/product_decisions/store.rs"),
        ),
        (
            "src/product_identity/config.rs",
            include_str!("../src/product_identity/config.rs"),
        ),
        (
            "src/product_identity/mod.rs",
            include_str!("../src/product_identity/mod.rs"),
        ),
        (
            "src/product_identity/model.rs",
            include_str!("../src/product_identity/model.rs"),
        ),
        (
            "src/product_identity/store.rs",
            include_str!("../src/product_identity/store.rs"),
        ),
        (
            "src/product_identity/database.rs",
            include_str!("../src/product_identity/database.rs"),
        ),
        (
            "src/product_identity/oauth_flow.rs",
            include_str!("../src/product_identity/oauth_flow.rs"),
        ),
        (
            "src/product_identity/principal.rs",
            include_str!("../src/product_identity/principal.rs"),
        ),
        (
            "src/product_identity/readiness.rs",
            include_str!("../src/product_identity/readiness.rs"),
        ),
        (
            "src/product_identity/session_issue.rs",
            include_str!("../src/product_identity/session_issue.rs"),
        ),
        (
            "src/product_identity/session_revoke.rs",
            include_str!("../src/product_identity/session_revoke.rs"),
        ),
        (
            "src/product_identity/store_tests.rs",
            include_str!("../src/product_identity/store_tests.rs"),
        ),
        (
            "src/product_retention.rs",
            include_str!("../src/product_retention.rs"),
        ),
        ("src/secret.rs", include_str!("../src/secret.rs")),
        ("src/snapshot.rs", include_str!("../src/snapshot.rs")),
        (
            "tests/dependency_guard.rs",
            include_str!("dependency_guard.rs"),
        ),
        (
            "tests/postgres_adapter.rs",
            include_str!("postgres_adapter.rs"),
        ),
        (
            "tests/postgres_product_identity.rs",
            include_str!("postgres_product_identity.rs"),
        ),
        (
            "tests/postgres_product_decisions.rs",
            include_str!("postgres_product_decisions.rs"),
        ),
        (
            "tests/postgres_product_retention.rs",
            include_str!("postgres_product_retention.rs"),
        ),
        (
            "tests/postgres_product_action_retention.rs",
            include_str!("postgres_product_action_retention.rs"),
        ),
        (
            "tests/postgres_product_apply.rs",
            include_str!("postgres_product_apply.rs"),
        ),
        (
            "tests/postgres_product_apply/database_fixture.rs",
            include_str!("postgres_product_apply/database_fixture.rs"),
        ),
        (
            "tests/postgres_product_apply/apply_support.rs",
            include_str!("postgres_product_apply/apply_support.rs"),
        ),
        (
            "tests/postgres_product_apply/apply_semantics.rs",
            include_str!("postgres_product_apply/apply_semantics.rs"),
        ),
        (
            "tests/postgres_product_apply/authority_drift.rs",
            include_str!("postgres_product_apply/authority_drift.rs"),
        ),
        (
            "tests/postgres_product_apply/security_concurrency.rs",
            include_str!("postgres_product_apply/security_concurrency.rs"),
        ),
        (
            "tests/postgres_product_apply/migration_security.rs",
            include_str!("postgres_product_apply/migration_security.rs"),
        ),
        (
            "tests/postgres_product_control_e2e.rs",
            include_str!("postgres_product_control_e2e.rs"),
        ),
        (
            "tests/postgres_product_control_e2e/support.rs",
            include_str!("postgres_product_control_e2e/support.rs"),
        ),
        (
            "tests/postgres_product_control_e2e/authority_history.rs",
            include_str!("postgres_product_control_e2e/authority_history.rs"),
        ),
        (
            "tests/postgres_product_control_e2e/approval_apply_flow.rs",
            include_str!("postgres_product_control_e2e/approval_apply_flow.rs"),
        ),
        (
            "tests/postgres_product_control_e2e/installation_authority.rs",
            include_str!("postgres_product_control_e2e/installation_authority.rs"),
        ),
        (
            "tests/postgres_product_control_e2e/installation_authority_security.rs",
            include_str!("postgres_product_control_e2e/installation_authority_security.rs"),
        ),
        (
            "tests/postgres_product_control_e2e/authentication_security.rs",
            include_str!("postgres_product_control_e2e/authentication_security.rs"),
        ),
        (
            "tests/postgres_product_control_e2e/authentication_migration_security.rs",
            include_str!("postgres_product_control_e2e/authentication_migration_security.rs"),
        ),
        (
            "tests/postgres_product_control_e2e/deployment_status.rs",
            include_str!("postgres_product_control_e2e/deployment_status.rs"),
        ),
    ];
    for (path, source) in sources {
        for (index, line) in source.lines().enumerate() {
            let trimmed = line.trim_start();
            assert!(
                !trimmed.starts_with("//")
                    && !trimmed.starts_with("/*")
                    && !trimmed.ends_with("*/"),
                "source comment at {path}:{}",
                index + 1
            );
        }
    }
}
