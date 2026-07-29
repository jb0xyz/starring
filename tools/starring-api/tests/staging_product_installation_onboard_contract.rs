const ONBOARDING: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ops/postgres/staging-product-installation-onboard.sql"
));

const EMPTY_RESOURCE_BINDINGS: &str = r#"{"channel_bindings":{},"role_bindings":{}}"#;
const EMPTY_RESOURCE_BINDINGS_FINGERPRINT: &str =
    "a44fd4f629a1183147a25a8afb93b026de7e3f92efe737637da222617df0c655";

fn compact(source: &str) -> String {
    source.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn section<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start = source.find(start).unwrap();
    let remaining = &source[start..];
    let end = remaining.find(end).unwrap();
    &remaining[..end]
}

fn position(source: &str, needle: &str) -> usize {
    source.find(needle).unwrap()
}

#[test]
fn onboarding_requires_explicit_inputs_and_an_exact_staging_target() {
    assert!(ONBOARDING
        .trim_start()
        .starts_with("\\set ON_ERROR_STOP on"));

    let required_inputs = [
        "expected_database",
        "expected_system_identifier",
        "tenant_id",
        "tenant_display_name",
        "installation_id",
        "discord_application_id",
        "discord_guild_id",
        "ruleset_key",
        "created_by_principal_id",
        "created_by_discord_user_id",
        "binding_fingerprint",
        "authority_payload_digest",
        "created_by_request_digest",
        "commit_onboarding",
    ];
    for input in required_inputs {
        let preflight = format!(
            "\\if :{{?{input}}}\n\\else\n\\echo '{input} is required'\nSELECT 1 / 0;\n\\endif"
        );
        assert_eq!(
            ONBOARDING.matches(&preflight).count(),
            1,
            "missing exact required-input preflight for {input}"
        );
        assert!(
            ONBOARDING.contains(&format!(":'{input}'")),
            "required input is not consumed: {input}"
        );
    }
    assert_eq!(
        ONBOARDING.matches("SELECT 1 / 0;").count(),
        required_inputs.len()
    );

    for required in [
        "SET search_path = pg_catalog;",
        "BEGIN TRANSACTION ISOLATION LEVEL SERIALIZABLE;",
        "pg_catalog.pg_control_system()",
        "expected_database IS DISTINCT FROM pg_catalog.current_database()",
        "!~ '^starring(_[a-z0-9]+)*_staging(_[a-z0-9]+)*$'",
        "expected_system_identifier IS DISTINCT FROM actual_system_identifier",
        "pg_catalog.inet_client_addr()",
        "'127.0.0.1'::PG_CATALOG.INET",
        "pg_catalog.inet_server_addr()",
        "pg_catalog.inet_server_port() IS DISTINCT FROM 5432",
        "current_user IS DISTINCT FROM session_user",
        "role.rolname = current_user",
        "role.rolsuper",
        "role.rolcanlogin",
        "staging installation onboarding target is invalid",
        "commit_onboarding NOT IN ('true', 'false')",
        "staging installation onboarding input is invalid",
        "pg_catalog.to_regrole('starring_owner') IS NULL",
        "staging installation onboarding owner is invalid",
        "pg_catalog.pg_advisory_xact_lock",
        "SET LOCAL ROLE starring_owner;",
        "SET CONSTRAINTS ALL DEFERRED;",
        "SET CONSTRAINTS ALL IMMEDIATE;",
    ] {
        assert!(
            ONBOARDING.contains(required),
            "missing fail-closed onboarding guard: {required}"
        );
    }
}

#[test]
fn empty_resource_bindings_require_the_exact_canonical_fingerprint() {
    assert_eq!(ONBOARDING.matches(EMPTY_RESOURCE_BINDINGS).count(), 1);
    assert_eq!(
        ONBOARDING
            .matches(EMPTY_RESOURCE_BINDINGS_FINGERPRINT)
            .count(),
        1
    );

    let normalized = compact(ONBOARDING);
    assert!(normalized.contains(&format!(
        "OR binding_fingerprint IS DISTINCT FROM '{EMPTY_RESOURCE_BINDINGS_FINGERPRINT}'"
    )));
    assert!(!normalized.contains("binding_fingerprint !~"));
    assert!(normalized.contains(&format!(
        "resource_bindings PG_CATALOG.JSONB := '{EMPTY_RESOURCE_BINDINGS}'::PG_CATALOG.JSONB"
    )));

    let replay = section(
        &normalized,
        "IF FOUND THEN",
        "PERFORM pg_catalog.set_config( 'starring.onboarding_result', 'exact_replay'",
    );
    for required in [
        "existing_authority.binding_revision <> 1",
        "existing_authority.resource_bindings <> onboard.resource_bindings",
        "existing_authority.binding_fingerprint <> onboard.binding_fingerprint",
        "existing_authority.policy_revision <> 1",
        "existing_authority.required_approvals <> 1",
        "existing_authority.activation_ttl_seconds <> 86400",
        "existing_authority.authority_payload_digest <> onboard.authority_payload_digest",
        "existing_authority.created_by_request_digest <> onboard.created_by_request_digest",
    ] {
        assert!(
            replay.contains(required),
            "exact replay omits canonical authority field: {required}"
        );
    }

    let authority_insert = section(
        &normalized,
        "INSERT INTO public.automation_installation_authority_versions",
        "PERFORM pg_catalog.set_config( 'starring.onboarding_result', 'created'",
    );
    for required in [
        "resource_bindings, binding_fingerprint",
        "onboard.resource_bindings, onboard.binding_fingerprint",
    ] {
        assert!(
            authority_insert.contains(required),
            "authority insert breaks binding/fingerprint coupling: {required}"
        );
    }
}

#[test]
fn created_and_exact_replay_have_distinct_writer_fence_contracts() {
    let normalized = compact(ONBOARDING);
    let replay_branch = section(
        &normalized,
        "IF FOUND THEN",
        "IF NOT EXISTS ( SELECT 1 FROM public.product_principals",
    );
    assert!(replay_branch.contains(
        "PERFORM pg_catalog.set_config( 'starring.onboarding_result', 'exact_replay', TRUE )"
    ));
    assert!(replay_branch.trim_end().ends_with("RETURN; END IF;"));
    assert!(replay_branch.contains("staging installation onboarding replay conflicts"));

    let replay_result = position(
        &normalized,
        "PERFORM pg_catalog.set_config( 'starring.onboarding_result', 'exact_replay', TRUE )",
    );
    let actor_gate = position(
        &normalized,
        "IF NOT EXISTS ( SELECT 1 FROM public.product_principals",
    );
    let tenant_insert = position(
        &normalized,
        "INSERT INTO public.product_tenants ( tenant_id",
    );
    let created_result = position(
        &normalized,
        "PERFORM pg_catalog.set_config( 'starring.onboarding_result', 'created', TRUE )",
    );
    assert!(replay_result < actor_gate);
    assert!(actor_gate < tenant_insert);
    assert!(tenant_insert < created_result);

    let actor_creation_gate = section(
        &normalized,
        "IF NOT EXISTS ( SELECT 1 FROM public.product_principals",
        "IF EXISTS ( SELECT 1 FROM public.product_tenants",
    );
    for required in [
        "NOT principal.disabled",
        "FROM public.product_auth_sessions AS actor_session",
        "actor_session.revoked_at IS NULL",
        "actor_session.idle_expires_at",
        "actor_session.absolute_expires_at",
        "pg_catalog.octet_length( actor_session.oauth_state_digest ) = 32",
        "staging installation onboarding actor is unavailable",
    ] {
        assert!(
            actor_creation_gate.contains(required),
            "fresh creation omits actor gate: {required}"
        );
    }

    assert!(!normalized.contains("INSERT INTO public.runtime_slot_writer_fences_v2"));
    let created_fence = section(
        &normalized,
        "onboarding_result = 'created' AND (",
        ") OR ( onboarding_result = 'exact_replay' AND (",
    );
    assert!(created_fence.contains("fence.writer_epoch = 1"));
    assert!(!created_fence.contains("fence.writer_epoch BETWEEN"));
    for field in [
        "pending_drain_intent_id",
        "pending_product_operation_id",
        "pending_tenant_id",
        "pending_installation_id",
        "pending_deployment_id",
        "pending_expected_revision",
        "pending_marked_at",
    ] {
        assert!(
            created_fence.contains(&format!("fence.{field} IS NULL")),
            "created fence permits pending state: {field}"
        );
        assert!(!created_fence.contains(&format!("fence.{field} IS NOT NULL")));
    }

    let replay_fence = section(
        &normalized,
        "onboarding_result = 'exact_replay' AND (",
        ") THEN RAISE EXCEPTION 'staging installation onboarding verification failed'",
    );
    assert!(replay_fence.contains("fence.writer_epoch BETWEEN 1 AND 9223372036854775807"));
    assert!(replay_fence.contains("pg_catalog.isfinite(fence.updated_at)"));
    for field in [
        "pending_drain_intent_id",
        "pending_product_operation_id",
        "pending_tenant_id",
        "pending_installation_id",
        "pending_deployment_id",
        "pending_expected_revision",
        "pending_marked_at",
    ] {
        assert!(
            replay_fence.contains(&format!("fence.{field} IS NULL")),
            "exact replay rejects a clean existing fence: {field}"
        );
        assert!(
            replay_fence.contains(&format!("fence.{field} IS NOT NULL")),
            "exact replay rejects a coherent pending existing fence: {field}"
        );
    }
}

#[test]
fn onboarding_is_transactional_dry_run_capable_and_contains_no_credentials() {
    for required in [
        "SET lock_timeout = '5s';",
        "SET statement_timeout = '60s';",
        "SET idle_in_transaction_session_timeout = '60s';",
        "RESET ROLE;",
        ":'commit_onboarding'::PG_CATALOG.BOOL AS commit_requested",
        "\\if :commit_onboarding\nCOMMIT;\n\\else\nROLLBACK;\n\\endif",
    ] {
        assert!(
            ONBOARDING.contains(required),
            "missing transaction boundary: {required}"
        );
    }
    assert_eq!(
        ONBOARDING
            .lines()
            .filter(|line| line.trim_start().starts_with("\\set "))
            .collect::<Vec<_>>(),
        ["\\set ON_ERROR_STOP on"]
    );

    let lower = ONBOARDING.to_ascii_lowercase();
    for forbidden in [
        "password '",
        "encrypted password",
        "authorization:",
        "bearer ",
        "client_secret",
        "api_token",
        "access_key",
        "secret_access_key",
        "cfat_",
        "cfut_",
        "api.starring.co.kr",
        "discord.com/api",
        "cloudflarestorage.com",
        "://",
        "-----begin age encrypted file-----",
    ] {
        assert!(
            !lower.contains(forbidden),
            "onboarding SQL contains credential or endpoint material: {forbidden}"
        );
    }
}
