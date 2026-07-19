use std::fmt::{Debug, Formatter};

use authoring_application::AuthorizedPromotionAccessV1;
use authoring_promotion::{
    derive_promotion_identity_from_secret_v1, AuthoringSessionId, AutomationInstallationId,
    PrincipalId, PromotionId, SessionGeneration, TenantId,
};

use crate::product_action_digest::{
    keyed_digest, product_action_keyring_coverage_identity_v1,
    product_action_session_subject_digest_v1, unkeyed_digest, ProductActionDigestKeyringV1,
};

const ENDPOINT_DOMAIN: &[u8] = b"product_promote_v1";
const IDEMPOTENCY_DOMAIN: &[u8] = b"starring.product.promotion.idempotency.v1";
const SEMANTIC_REQUEST_DOMAIN: &[u8] = b"starring.product.promotion.request.v1";
pub(super) const ADMISSION_DOMAIN: &[u8] = b"starring.product.promotion.admission.v1";
const RECEIPT_ID_DOMAIN: &[u8] = b"starring.product.promotion.receipt.v1";
const AUDIT_EVENT_ID_DOMAIN: &[u8] = b"starring.product.promotion.audit.v1";
const SESSION_SUBJECT_DOMAIN: &[u8] = b"starring.product.session.subject.v1";
const KEY_MATERIAL_FINGERPRINT_DOMAIN: &[u8] =
    b"starring.product.promotion.digest-key-fingerprint.v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(super) enum ProductPromotionDigestErrorV1 {
    #[error("product promotion idempotency secret is invalid")]
    InvalidIdempotencySecret,
    #[error("product promotion identity is invalid")]
    InvalidPromotionIdentity,
}

pub(super) struct ProductPromotionDigestsV1 {
    pub promotion_id: PromotionId,
    pub active_idempotency: String,
    pub idempotency_candidates: Vec<String>,
    pub idempotency_candidate_key_ids: Vec<String>,
    pub idempotency_candidate_key_fingerprints: Vec<String>,
    pub active_key_id: String,
    pub active_key_fingerprint: String,
    pub semantic_request: String,
    pub receipt_id: String,
    pub audit_event_id: String,
    pub session_subject: Vec<u8>,
}

impl Debug for ProductPromotionDigestsV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductPromotionDigestsV1(<redacted>)")
    }
}

pub(super) fn promotion_digests_v1(
    keyring: &ProductActionDigestKeyringV1,
    access: &AuthorizedPromotionAccessV1<
        '_,
        authoring_application_discord::FreshDiscordAuthorityEvidenceV1,
    >,
) -> Result<ProductPromotionDigestsV1, ProductPromotionDigestErrorV1> {
    access.with_product_idempotency_secret(|secret| {
        promotion_digests_from_secret_v1(
            keyring,
            PromotionDigestScopeV1 {
                tenant_id: access.scope().tenant_id(),
                installation_id: access.scope().installation_id(),
                principal_id: access.actor().principal_id(),
                session_id: access.session_id(),
                generation: access.expected_generation(),
                session_fingerprint: access.session_fingerprint().as_bytes(),
            },
            secret,
        )
    })
}

struct PromotionDigestScopeV1<'a> {
    tenant_id: &'a TenantId,
    installation_id: &'a AutomationInstallationId,
    principal_id: &'a PrincipalId,
    session_id: &'a AuthoringSessionId,
    generation: SessionGeneration,
    session_fingerprint: &'a [u8; 32],
}

fn promotion_digests_from_secret_v1(
    keyring: &ProductActionDigestKeyringV1,
    scope: PromotionDigestScopeV1<'_>,
    secret: &[u8],
) -> Result<ProductPromotionDigestsV1, ProductPromotionDigestErrorV1> {
    let secret = std::str::from_utf8(secret)
        .map_err(|_| ProductPromotionDigestErrorV1::InvalidIdempotencySecret)?;
    if secret.is_empty()
        || secret.len() > 128
        || !secret
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        return Err(ProductPromotionDigestErrorV1::InvalidIdempotencySecret);
    }
    let promotion_id =
        derive_promotion_identity_from_secret_v1(scope.tenant_id, scope.principal_id, secret)
            .map_err(|_| ProductPromotionDigestErrorV1::InvalidPromotionIdentity)?
            .promotion_id;
    let idempotency_fields = [
        scope.tenant_id.as_str().as_bytes(),
        scope.installation_id.as_str().as_bytes(),
        scope.principal_id.as_str().as_bytes(),
        ENDPOINT_DOMAIN,
        secret.as_bytes(),
    ];
    let idempotency_candidates = keyring
        .keys()
        .iter()
        .map(|key| keyed_digest(key, IDEMPOTENCY_DOMAIN, &idempotency_fields))
        .collect::<Vec<_>>();
    let active_idempotency = idempotency_candidates[0].clone();
    let keyring_identity =
        product_action_keyring_coverage_identity_v1(keyring, KEY_MATERIAL_FINGERPRINT_DOMAIN);
    let generation = scope.generation.get().to_string();
    let semantic_request = unkeyed_digest(
        SEMANTIC_REQUEST_DOMAIN,
        &[
            scope.tenant_id.as_str().as_bytes(),
            scope.installation_id.as_str().as_bytes(),
            scope.principal_id.as_str().as_bytes(),
            scope.session_id.as_str().as_bytes(),
            generation.as_bytes(),
            promotion_id.as_str().as_bytes(),
        ],
    );
    let identity_fields = [
        scope.tenant_id.as_str().as_bytes(),
        scope.installation_id.as_str().as_bytes(),
        scope.principal_id.as_str().as_bytes(),
        promotion_id.as_str().as_bytes(),
        active_idempotency.as_bytes(),
        semantic_request.as_bytes(),
    ];
    let receipt_id = keyed_digest(keyring.active(), RECEIPT_ID_DOMAIN, &identity_fields);
    let audit_event_id = keyed_digest(keyring.active(), AUDIT_EVENT_ID_DOMAIN, &identity_fields);
    let session_subject = product_action_session_subject_digest_v1(
        SESSION_SUBJECT_DOMAIN,
        scope.tenant_id.as_str().as_bytes(),
        scope.principal_id.as_str().as_bytes(),
        scope.session_fingerprint,
    );
    Ok(ProductPromotionDigestsV1 {
        promotion_id,
        active_idempotency,
        idempotency_candidates,
        idempotency_candidate_key_ids: keyring_identity.key_ids.clone(),
        idempotency_candidate_key_fingerprints: keyring_identity.key_fingerprints.clone(),
        active_key_id: keyring_identity.key_ids[0].clone(),
        active_key_fingerprint: keyring_identity.key_fingerprints[0].clone(),
        semantic_request,
        receipt_id,
        audit_event_id,
        session_subject,
    })
}

pub(super) fn encode_lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

#[cfg(test)]
mod tests {
    use authoring_promotion::{derive_promotion_identity_v1, IdempotencyKey};

    use super::*;
    use crate::product_action_digest::ProductActionDigestKeyV1;

    fn key(id: &str, seed: u8) -> ProductActionDigestKeyV1 {
        ProductActionDigestKeyV1::from_bytes(
            id,
            std::array::from_fn(|index| seed.wrapping_add(index as u8)),
        )
        .unwrap()
    }

    fn scope() -> (
        TenantId,
        AutomationInstallationId,
        PrincipalId,
        AuthoringSessionId,
    ) {
        (
            TenantId::parse("tenant-one").unwrap(),
            AutomationInstallationId::parse("installation-one").unwrap(),
            PrincipalId::parse("principal-one").unwrap(),
            AuthoringSessionId::parse("session-one").unwrap(),
        )
    }

    #[test]
    fn borrowed_secret_identity_matches_authoring_domain_identity() {
        let (tenant, _, principal, _) = scope();
        let expected = derive_promotion_identity_v1(
            &tenant,
            &principal,
            &IdempotencyKey::parse("low-entropy-key").unwrap(),
        )
        .unwrap();
        let actual =
            derive_promotion_identity_from_secret_v1(&tenant, &principal, "low-entropy-key")
                .unwrap()
                .promotion_id;
        assert_eq!(actual, expected.promotion_id);
    }

    #[test]
    fn promotion_digest_bundle_is_domain_separated_and_rotation_aware() {
        let keyring =
            ProductActionDigestKeyringV1::new(key("active-v2", 2), [key("retired-v1", 91)])
                .unwrap();
        let (tenant, installation, principal, session) = scope();
        let session_fingerprint = [31_u8; 32];
        let digests = promotion_digests_from_secret_v1(
            &keyring,
            PromotionDigestScopeV1 {
                tenant_id: &tenant,
                installation_id: &installation,
                principal_id: &principal,
                session_id: &session,
                generation: SessionGeneration::new(3).unwrap(),
                session_fingerprint: &session_fingerprint,
            },
            b"low-entropy-key",
        )
        .unwrap();
        assert_eq!(digests.idempotency_candidates.len(), 2);
        assert_eq!(
            digests.idempotency_candidate_key_ids,
            ["active-v2", "retired-v1"]
        );
        assert_eq!(digests.idempotency_candidate_key_fingerprints.len(), 2);
        assert_eq!(digests.active_key_id, "active-v2");
        assert_eq!(digests.active_key_fingerprint.len(), 64);
        assert_eq!(digests.session_subject.len(), 32);
        assert_ne!(digests.active_idempotency, digests.semantic_request);
        assert_ne!(digests.receipt_id, digests.audit_event_id);
        assert_eq!(
            format!("{digests:?}"),
            "ProductPromotionDigestsV1(<redacted>)"
        );
    }

    #[test]
    fn promotion_digest_rejects_invalid_secret_without_persisting_it() {
        let keyring = ProductActionDigestKeyringV1::new(key("active-v1", 4), []).unwrap();
        let (tenant, installation, principal, session) = scope();
        let session_fingerprint = [7_u8; 32];
        let error = promotion_digests_from_secret_v1(
            &keyring,
            PromotionDigestScopeV1 {
                tenant_id: &tenant,
                installation_id: &installation,
                principal_id: &principal,
                session_id: &session,
                generation: SessionGeneration::new(1).unwrap(),
                session_fingerprint: &session_fingerprint,
            },
            b"invalid secret",
        )
        .unwrap_err();
        assert_eq!(
            error,
            ProductPromotionDigestErrorV1::InvalidIdempotencySecret
        );
        assert!(!format!("{error:?}").contains("invalid secret"));
    }
}
