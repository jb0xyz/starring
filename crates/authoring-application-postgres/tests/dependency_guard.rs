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
    let identity_store = include_str!("../src/product_identity/store.rs");
    assert!(identity_store.contains("identity: VerifiedDiscordIdentityV1"));
    assert!(identity_store.contains("VerifiedIdentityProjection::from_capability"));
    assert!(!identity_store.contains("pub fn from_capability"));
}

#[test]
fn product_decision_adapter_keeps_atomic_security_and_idempotency_boundaries() {
    let store = include_str!("../src/product_decisions/store.rs");
    let digest = include_str!("../src/product_decisions/digest.rs");
    let config = include_str!("../src/product_decisions/config.rs");
    assert!(store.contains("public.starring_product_approve_v1"));
    assert!(store.contains("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE"));
    assert!(store.contains("FreshDiscordAuthorityEvidenceV1"));
    assert!(store.contains("request.session_fingerprint().as_bytes()"));
    assert!(!store.contains(".bind(request.command().idempotency_key.as_str())"));
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
    let decision_store = include_str!("../src/product_decisions/store.rs");
    assert!(decision_store.contains("transaction.commit().await.map_err(database_commit)?"));
    assert!(decision_store.contains("matches!(&error, sqlx::Error::Database(_))"));
    let activation_store =
        include_str!("../../automation-ruleset-activation-postgres/src/store.rs");
    assert_eq!(
        activation_store
            .matches("product activation requires authenticated product control")
            .count(),
        2
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
    assert!(authentication.contains("FOR SHARE OF authentication_session, principal"));
    assert!(authentication.contains("touch_active_product_session"));
    assert!(authentication.contains("SELECT pg_catalog.clock_timestamp()"));
    assert!(authentication.contains("public.product_auth_sessions"));
    assert!(authentication.contains("public.product_principals"));
    assert!(authentication.contains("pg_catalog.set_config("));
    assert!(authentication.contains("pg_catalog.make_interval("));
    assert!(!authentication.contains(".bind(credential)"));
    let snapshot = include_str!("../src/snapshot.rs");
    assert!(snapshot.contains("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY"));
    assert!(snapshot.contains("CURRENT_TIMESTAMP AS database_now"));
    assert!(snapshot.contains("evidence.installation_authority_revision().get()"));
    assert!(snapshot.contains("evidence.installation_authority_digest()"));
    assert!(snapshot.contains("actor.session_fingerprint().as_bytes()"));
    assert!(snapshot.contains("actor_session.session_digest = $2"));
    assert!(snapshot.contains("authoring_session.tenant_id = $3"));
    assert!(snapshot.contains("authoring_session.installation_id = $4"));
    assert!(snapshot.contains(".bind(scope.tenant_id().as_str())"));
    assert!(snapshot.contains(".bind(scope.installation_id().as_str())"));
    assert!(snapshot.contains("public.authoring_sessions"));
    assert!(snapshot.contains("public.product_principals"));
    assert!(snapshot.contains("public.product_auth_sessions"));
    assert!(snapshot.contains("public.product_tenants"));
    assert!(snapshot.contains("public.automation_installations"));
    assert!(snapshot.contains("public.authoring_session_generations"));
    assert!(snapshot.contains("public.automation_installation_authority_versions"));
    assert!(snapshot.contains("pg_catalog.set_config("));
    assert!(!snapshot.contains("map_err(|error| session_backend(error.to_string()))"));
    let identity = include_str!("../src/product_identity/store.rs")
        .split("#[cfg(test)]")
        .next()
        .unwrap();
    assert!(identity.contains("digest_opaque_session_credential_v1(state)"));
    assert!(identity.contains("digest_opaque_session_credential_v1(browser_nonce)"));
    assert!(identity.contains("digest_opaque_session_credential_v1(session.expose_secret())"));
    assert!(identity.contains("digest_opaque_session_credential_v1(csrf.expose_secret())"));
    assert!(!identity.contains(".bind(state)"));
    assert!(!identity.contains(".bind(browser_nonce)"));
    assert!(!identity.contains(".bind(credential)"));
    assert!(!identity.contains(".bind(csrf)"));
    assert!(identity.contains("oauth_state_digest"));
    assert!(identity.contains("validate_consumed_flow"));
    assert!(authentication.contains("persisted_csrf.ct_eq(expected)"));
    assert!(identity.contains("persisted.ct_eq(expected.as_bytes())"));
    assert!(identity.contains("ProductLogoutDispositionV1::ExactReplay"));
    assert!(identity.contains("inserted.idle_expires_at.min(inserted.absolute_expires_at)"));
    assert!(identity.contains("public.product_oauth_flows"));
    assert!(identity.contains("public.product_auth_sessions"));
    assert!(identity.contains("public.product_principals"));
    assert!(identity.contains("pg_catalog.clock_timestamp()"));
    assert!(identity.contains("pg_catalog.make_interval("));
    assert!(identity.contains("pg_catalog.set_config("));
    for (path, source) in [
        ("src/authentication.rs", authentication),
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
        ("src/bindings.rs", include_str!("../src/bindings.rs")),
        ("src/database.rs", include_str!("../src/database.rs")),
        ("src/digest.rs", include_str!("../src/digest.rs")),
        ("src/envelope.rs", include_str!("../src/envelope.rs")),
        ("src/lib.rs", include_str!("../src/lib.rs")),
        (
            "src/product_decisions/config.rs",
            include_str!("../src/product_decisions/config.rs"),
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
            "tests/postgres_product_control_e2e.rs",
            include_str!("postgres_product_control_e2e.rs"),
        ),
    ];
    for (path, source) in sources {
        for (index, line) in source.lines().enumerate() {
            let trimmed = line.trim_start();
            assert!(
                !trimmed.starts_with("//")
                    && !trimmed.starts_with("/*")
                    && !trimmed.starts_with('*')
                    && !trimmed.ends_with("*/"),
                "source comment at {path}:{}",
                index + 1
            );
        }
    }
}
