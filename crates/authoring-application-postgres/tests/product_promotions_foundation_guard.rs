#[test]
fn product_promotion_foundation_keeps_secrets_and_database_authority_separated() {
    let module = include_str!("../src/product_promotions/mod.rs");
    let store = include_str!("../src/product_promotions/store.rs");
    let config = include_str!("../src/product_promotions/config.rs");
    let authorization = include_str!("../src/product_promotions/authorization.rs");
    let digest = include_str!("../src/product_promotions/digest.rs");
    let admission = include_str!("../src/product_promotions/admission.rs");
    assert!(module.contains("mod authorization"));
    assert!(module.contains("mod admission"));
    assert!(store.contains("executor: PgPool"));
    assert!(!store.contains("PromotionService"));
    assert!(config.contains("ProductActionDigestKeyringV1"));
    assert!(config.contains("MAX_TRANSACTION_RETRIES"));
    assert!(authorization.contains("FreshDiscordAuthorityEvidenceV1"));
    assert!(authorization.contains("CapabilityV1::Promote"));
    assert!(authorization.contains("Permissions::ADMINISTRATOR | Permissions::MANAGE_GUILD"));
    assert!(digest.contains("with_product_idempotency_secret"));
    assert!(digest.contains("derive_promotion_identity_from_secret_v1"));
    let production_digest = digest.split("#[cfg(test)]").next().unwrap_or(digest);
    assert!(!production_digest.contains("IdempotencyKey::parse"));
    assert!(!digest.contains(".bind(secret"));
    assert!(admission.contains("serde(deny_unknown_fields)"));
    assert!(admission.contains("ADMISSION_FORMAT_VERSION: u16 = 1"));
    assert!(admission.contains("ConstantTimeEq"));
    assert!(!admission.contains("expected_product_session_digest:"));
}

#[test]
fn product_promotion_foundation_contains_no_direct_sql_or_raw_relation_access() {
    let source = concat!(
        include_str!("../src/product_promotions/store.rs"),
        include_str!("../src/product_promotions/config.rs"),
        include_str!("../src/product_promotions/authorization.rs"),
        include_str!("../src/product_promotions/digest.rs"),
        include_str!("../src/product_promotions/admission.rs"),
    );
    assert!(!source.contains("sqlx::query"));
    assert!(!source.contains("authoring_promotions"));
    assert!(!source.contains("product_action_receipts"));
    assert!(!source.contains("activation_requests"));
}
