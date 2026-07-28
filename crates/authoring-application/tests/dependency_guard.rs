use std::collections::BTreeSet;

fn regular_dependency_keys(manifest: &str) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    let mut dependency_map = false;

    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            let section = &line[1..line.len() - 1];
            dependency_map = section == "dependencies"
                || (section.starts_with("target.") && section.ends_with(".dependencies"));
            if let Some(key) = dependency_table_key(section) {
                keys.insert(key.to_string());
            }
            continue;
        }
        if !dependency_map || line.is_empty() {
            continue;
        }
        if let Some((key, _)) = line.split_once('=') {
            let key = key.trim().trim_matches(['\'', '"']);
            if !key.is_empty() {
                keys.insert(key.to_string());
            }
        }
    }

    keys
}

fn dependency_table_key(section: &str) -> Option<&str> {
    if let Some(key) = section.strip_prefix("dependencies.") {
        return nonempty_key(key);
    }
    if !section.starts_with("target.") {
        return None;
    }
    section
        .find(".dependencies.")
        .and_then(|index| nonempty_key(&section[index + ".dependencies.".len()..]))
}

fn nonempty_key(key: &str) -> Option<&str> {
    let key = key.trim().trim_matches(['\'', '"']);
    (!key.is_empty()).then_some(key)
}

#[test]
fn regular_dependencies_stay_pure() {
    let manifest = include_str!("../Cargo.toml");
    let dependencies = regular_dependency_keys(manifest);
    let forbidden = [
        "sqlx",
        "rusqlite",
        "axum",
        "ai-gateway",
        "automation-runtime",
        "automation-ruleset-activation",
        "automation-ruleset-readiness",
        "approval-manager",
        "policy-engine",
        "preview",
        "postgres",
        "tokio-postgres",
    ];

    for dependency in &dependencies {
        assert!(
            !forbidden.contains(&dependency.as_str()),
            "forbidden regular dependency: {dependency}"
        );
        assert!(
            dependency != "twilight" && !dependency.starts_with("twilight-"),
            "forbidden regular dependency: {dependency}"
        );
        assert!(
            !dependency.ends_with("-postgres"),
            "forbidden PostgreSQL adapter dependency: {dependency}"
        );
    }
}

#[test]
fn parser_matches_dependency_keys_without_substring_false_positives() {
    let manifest = r#"
[dependencies]
automation-ruleset-activation = { path = "../automation-ruleset-activation" }
design-harness = { path = "../design-harness" }

[dev-dependencies]
sqlx = "0.8"

[workspace.dependencies]
ai-gateway = "1"
"#;

    assert_eq!(
        regular_dependency_keys(manifest),
        BTreeSet::from([
            "automation-ruleset-activation".to_string(),
            "design-harness".to_string(),
        ])
    );
}

#[test]
fn authority_inputs_stay_non_deserializable_and_out_of_the_client_command() {
    let source = source();
    assert!(!source.contains("Deserialize"));
    let command = source
        .split("pub struct PromoteOwnedSessionV1")
        .nth(1)
        .unwrap()
        .split('}')
        .next()
        .unwrap();
    for forbidden in [
        "guild_id",
        "installation_id",
        "ruleset_key",
        "requester",
        "binding_revision",
        "policy",
        "artifact",
    ] {
        assert!(
            !command.contains(forbidden),
            "client command leaked {forbidden}"
        );
    }
}

#[test]
fn authenticated_actor_stays_crate_issued_and_authority_load_stays_atomic() {
    let source = source();
    assert!(!source.contains("from_trusted_edge"));
    assert!(!source.contains("pub fn from_authenticated_session"));
    assert!(!source.contains("pub fn from_authentication_claims"));
    assert!(!source.contains("pub trait OwnedSessionArtifactPort"));
    assert!(!source.contains("pub trait PromotionAuthorityPort"));
    assert!(source.contains("pub trait AuthenticationPort"));
    assert!(source.contains("pub trait MutationAuthenticationPort"));
    assert!(source.contains("pub trait AuthorizedPromotionSnapshotPort"));
    assert!(source.contains("load_atomic_authorized_snapshot"));
    let identity = include_str!("../src/identity.rs");
    let authenticated_session = identity
        .split("struct AuthenticatedSessionV1")
        .nth(1)
        .unwrap()
        .split('}')
        .next()
        .unwrap();
    assert!(!identity.contains("pub struct AuthenticatedSessionV1"));
    assert!(!authenticated_session.contains("tenant"));
    let authentication_claims = identity
        .split("pub struct AuthenticationClaimsV1")
        .nth(1)
        .unwrap()
        .split('}')
        .next()
        .unwrap();
    assert!(!authentication_claims.contains("tenant"));
    assert!(authentication_claims.contains("session_fingerprint"));
    assert!(identity.contains("AuthenticatedSessionFingerprintV1(<redacted>)"));
    assert!(identity.contains("AuthenticationClaimsV1(<redacted>)"));
    let application = [
        include_str!("../src/application.rs"),
        include_str!("../src/application/promotion_flow.rs"),
        include_str!("../src/application/decision_mutation.rs"),
        include_str!("../src/application/lifecycle_cancellation.rs"),
    ]
    .join("\n");
    assert!(application.contains("authenticate_mutation(credential, csrf)"));
    for mutation in [
        "promote_owned_session",
        "approve",
        "reject",
        "apply",
        "cancel_product_lifecycle",
    ] {
        let method = application
            .split(&format!("fn {mutation}"))
            .nth(1)
            .unwrap()
            .split("\n    }")
            .next()
            .unwrap();
        assert!(
            method.contains("csrf"),
            "mutation {mutation} omitted CSRF proof"
        );
    }
}

#[test]
fn product_commands_never_accept_trusted_authority_or_apply_attempt_fields() {
    let source = source();
    for command_name in [
        "ApproveProductPromotionV1",
        "RejectProductPromotionV1",
        "ApplyProductPromotionV1",
        "CancelProductLifecycleMutationV1",
    ] {
        let command = source
            .split(&format!("pub struct {command_name}"))
            .nth(1)
            .unwrap()
            .split('}')
            .next()
            .unwrap();
        for forbidden in [
            "actor",
            "guild_id",
            "requester",
            "policy",
            "target",
            "attempt_id",
        ] {
            assert!(
                !command.contains(forbidden),
                "client command {command_name} leaked {forbidden}"
            );
        }
    }
}

#[test]
fn decision_port_has_only_bound_decisions_and_adapter_derived_apply_identity() {
    let source = include_str!("../src/control.rs");
    for (trait_name, methods) in [
        (
            "ProductDecisionQueryPort",
            &["load_approval_preview", "load_product_status"][..],
        ),
        ("ProductApprovalPort", &["approve_payload_bound"][..]),
        ("ProductRejectionPort", &["reject_payload_bound"][..]),
        ("ProductApplyPort", &["apply_idempotent"][..]),
    ] {
        let port = source
            .split(&format!("pub trait {trait_name}"))
            .nth(1)
            .unwrap()
            .split("\n}")
            .next()
            .unwrap();
        for method in methods {
            assert!(port.contains(method), "{trait_name} omitted {method}");
        }
    }
    let marker = source
        .split("pub trait ProductDecisionPort")
        .nth(1)
        .unwrap()
        .split("\n}")
        .next()
        .unwrap();
    for capability in [
        "ProductDecisionQueryPort",
        "ProductApprovalPort",
        "ProductRejectionPort",
        "ProductApplyPort",
    ] {
        assert!(marker.contains(capability));
    }
    assert!(!marker.contains("async fn"));
    assert!(!source.contains("ApplyAttemptId"));
    assert!(!source.contains("async fn approve("));
    assert!(!source.contains("async fn reject("));
    assert!(!source.contains("async fn apply("));
}

#[test]
fn product_lifecycle_cancellation_is_distinct_checked_and_additive() {
    let source = source();
    for required in [
        "CancelLifecycle",
        "ProductDrainSelectorV1",
        "InvalidAcknowledgedStateDigest",
        "IdentityCollision",
        "CancelProductLifecycleMutationV1",
        "AuthorizedCancelProductLifecycleV1",
        "ProductLifecycleCancellationPort",
        "cancel_lifecycle_idempotent",
        "ProductLifecycleCancellationReceiptV1",
        "RuntimeDrainPending(ProductDrainSelectorV1)",
        "LifecycleCancelled(ProductDrainSelectorV1)",
        "CapabilityV1::CancelLifecycle",
        "cancel_product_lifecycle",
    ] {
        assert!(source.contains(required), "{required}");
    }
    let command = source
        .split("pub struct CancelProductLifecycleMutationV1")
        .nth(1)
        .unwrap()
        .split('}')
        .next()
        .unwrap();
    for field in [
        "promotion",
        "expected_payload_digest",
        "expected_revision",
        "drain_selector",
        "idempotency_key",
        "reason",
    ] {
        assert!(command.contains(field), "{field}");
    }
    for forbidden in [
        "actor",
        "guild_id",
        "requester",
        "policy",
        "target",
        "authority",
        "slot_writer_epoch",
    ] {
        assert!(!command.contains(forbidden), "{forbidden}");
    }
    let authorized = source
        .split("pub struct AuthorizedCancelProductLifecycleV1")
        .nth(1)
        .unwrap()
        .split("pub enum ProductLifecycleCancellationReceiptError")
        .next()
        .unwrap();
    assert!(authorized.contains("ProductMutationContextV1"));
    assert!(authorized.contains("pub(crate) fn new"));
    assert!(!authorized.contains("pub fn new"));
    let marker = source
        .split("pub trait ProductDecisionPort")
        .nth(1)
        .unwrap()
        .split("\n}")
        .next()
        .unwrap();
    assert!(!marker.contains("ProductLifecycleCancellationPort"));
}

#[test]
fn product_mutation_context_is_crate_issued_bound_and_redacted() {
    let control = include_str!("../src/control.rs");
    let context = control
        .split("pub struct ProductMutationContextV1")
        .nth(1)
        .unwrap()
        .split("pub struct AuthorizedApprovalPreviewV1")
        .next()
        .unwrap();
    for bound_value in ["request_id", "actor", "scope", "evidence"] {
        assert!(context.contains(bound_value));
    }
    assert!(context.contains("pub(crate) fn new"));
    assert!(!context.contains("pub fn new"));
    assert!(context.contains("ProductMutationContextV1(<redacted>)"));
    assert!(control.contains("ProductRequestIdV1(<redacted>)"));
}

#[test]
fn production_promotion_uses_only_the_authorized_resume_first_boundary() {
    let promotion = include_str!("../src/application/promotion_flow.rs");
    assert!(promotion.contains("P: AuthorizedPromotionSubmissionPort<G::Evidence>"));
    assert!(!promotion.contains("P: PromotionSubmissionPort"));
    let resume = promotion
        .find("find_or_resume_authorized_promotion")
        .unwrap();
    let snapshot = promotion.find("load_atomic_authorized_snapshot").unwrap();
    let submit = promotion.find("submit_authorized_promotion").unwrap();
    assert!(resume < snapshot);
    assert!(snapshot < submit);
    let boundary = include_str!("../src/promotion.rs");
    assert!(boundary.contains("AuthorizedPromotionAccessV1(<redacted>)"));
    assert!(boundary.contains("AuthorizedPromotionSubmissionV1(<redacted>)"));
    assert!(boundary.contains("with_product_idempotency_secret"));
}

fn source() -> String {
    [
        include_str!("../src/lib.rs"),
        include_str!("../src/application.rs"),
        include_str!("../src/application/approval_query.rs"),
        include_str!("../src/application/decision_mutation.rs"),
        include_str!("../src/application/lifecycle_cancellation.rs"),
        include_str!("../src/application/projection_validation.rs"),
        include_str!("../src/application/promotion_flow.rs"),
        include_str!("../src/application/status_query.rs"),
        include_str!("../src/authority.rs"),
        include_str!("../src/control.rs"),
        include_str!("../src/identity.rs"),
        include_str!("../src/lifecycle.rs"),
        include_str!("../src/promotion.rs"),
        include_str!("../src/status.rs"),
        include_str!("../src/status/runtime.rs"),
    ]
    .join("\n")
}
