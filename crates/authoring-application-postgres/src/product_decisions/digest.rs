use authoring_application::AuthorizedApproveProductV1;
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use super::config::{ProductDecisionDigestKeyV1, ProductDecisionDigestKeyringV1};

const IDEMPOTENCY_DOMAIN: &[u8] = b"starring.product.approval.idempotency.v1";
const SEMANTIC_REQUEST_DOMAIN: &[u8] = b"starring.product.approval.request.v1";
const RECEIPT_ID_DOMAIN: &[u8] = b"starring.product.approval.receipt.v1";
const AUDIT_EVENT_ID_DOMAIN: &[u8] = b"starring.product.approval.audit.v1";
const SESSION_SUBJECT_DOMAIN: &[u8] = b"starring.product.session.subject.v1";
const KEY_MATERIAL_FINGERPRINT_DOMAIN: &[u8] =
    b"starring.product.approval.digest-key-fingerprint.v1";

pub(crate) struct ApprovalDigests {
    pub active_idempotency: String,
    pub idempotency_candidates: Vec<String>,
    pub idempotency_candidate_key_ids: Vec<String>,
    pub idempotency_candidate_key_fingerprints: Vec<String>,
    pub active_key_id: String,
    pub semantic_request: String,
    pub receipt_id: String,
    pub audit_event_id: String,
    pub session_subject: Vec<u8>,
}

pub(crate) struct KeyringCoverageIdentity {
    pub key_ids: Vec<String>,
    pub key_fingerprints: Vec<String>,
}

pub(crate) fn keyring_coverage_identity(
    keyring: &ProductDecisionDigestKeyringV1,
) -> KeyringCoverageIdentity {
    KeyringCoverageIdentity {
        key_ids: keyring
            .keys()
            .iter()
            .map(|key| key.key_id().to_string())
            .collect(),
        key_fingerprints: keyring
            .keys()
            .iter()
            .map(|key| unkeyed_digest(KEY_MATERIAL_FINGERPRINT_DOMAIN, &[key.secret()]))
            .collect(),
    }
}

pub(crate) fn approval_digests(
    keyring: &ProductDecisionDigestKeyringV1,
    request: &AuthorizedApproveProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
) -> ApprovalDigests {
    let scope = request.scope();
    let command = request.command();
    let expected_revision = command.expected_revision.get().to_string();
    let idempotency_fields = [
        scope.tenant_id().as_str().as_bytes(),
        scope.installation_id().as_str().as_bytes(),
        request.actor().principal_id().as_str().as_bytes(),
        b"product_approve_v1".as_slice(),
        command.idempotency_key.as_str().as_bytes(),
    ];
    let idempotency_candidates = keyring
        .keys()
        .iter()
        .map(|key| keyed_digest(key, IDEMPOTENCY_DOMAIN, &idempotency_fields))
        .collect::<Vec<_>>();
    let active_idempotency = idempotency_candidates[0].clone();
    let keyring_identity = keyring_coverage_identity(keyring);
    let semantic_request = unkeyed_digest(
        SEMANTIC_REQUEST_DOMAIN,
        &[
            scope.tenant_id().as_str().as_bytes(),
            scope.installation_id().as_str().as_bytes(),
            request.actor().principal_id().as_str().as_bytes(),
            command.promotion.promotion_id().as_str().as_bytes(),
            expected_revision.as_bytes(),
            command.expected_payload_digest.as_str().as_bytes(),
        ],
    );
    let identity_fields = [
        scope.tenant_id().as_str().as_bytes(),
        scope.installation_id().as_str().as_bytes(),
        request.actor().principal_id().as_str().as_bytes(),
        active_idempotency.as_bytes(),
        semantic_request.as_bytes(),
    ];
    let receipt_id = keyed_digest(keyring.active(), RECEIPT_ID_DOMAIN, &identity_fields);
    let audit_event_id = keyed_digest(keyring.active(), AUDIT_EVENT_ID_DOMAIN, &identity_fields);
    let session_subject = unkeyed_digest_bytes(
        SESSION_SUBJECT_DOMAIN,
        &[
            scope.tenant_id().as_str().as_bytes(),
            request.actor().principal_id().as_str().as_bytes(),
            request.session_fingerprint().as_bytes().as_slice(),
        ],
    );
    ApprovalDigests {
        active_idempotency,
        idempotency_candidates,
        idempotency_candidate_key_ids: keyring_identity.key_ids,
        idempotency_candidate_key_fingerprints: keyring_identity.key_fingerprints,
        active_key_id: keyring.active().key_id().to_string(),
        semantic_request,
        receipt_id,
        audit_event_id,
        session_subject,
    }
}

fn keyed_digest(key: &ProductDecisionDigestKeyV1, domain: &[u8], fields: &[&[u8]]) -> String {
    lower_hex(&keyed_digest_bytes(key, domain, fields))
}

fn keyed_digest_bytes(
    key: &ProductDecisionDigestKeyV1,
    domain: &[u8],
    fields: &[&[u8]],
) -> Vec<u8> {
    let mut hmac = <Hmac<Sha256> as Mac>::new_from_slice(key.secret())
        .expect("validated product decision digest key has a supported length");
    update_hmac(&mut hmac, domain);
    for field in fields {
        update_hmac(&mut hmac, field);
    }
    hmac.finalize().into_bytes().to_vec()
}

fn unkeyed_digest(domain: &[u8], fields: &[&[u8]]) -> String {
    unkeyed_digest_owned(domain, fields.iter().copied())
}

fn unkeyed_digest_owned<'a>(domain: &[u8], fields: impl IntoIterator<Item = &'a [u8]>) -> String {
    lower_hex(&unkeyed_digest_bytes_owned(domain, fields))
}

fn unkeyed_digest_bytes(domain: &[u8], fields: &[&[u8]]) -> Vec<u8> {
    unkeyed_digest_bytes_owned(domain, fields.iter().copied())
}

fn unkeyed_digest_bytes_owned<'a>(
    domain: &[u8],
    fields: impl IntoIterator<Item = &'a [u8]>,
) -> Vec<u8> {
    let mut hasher = Sha256::new();
    update_sha256(&mut hasher, domain);
    for field in fields {
        update_sha256(&mut hasher, field);
    }
    hasher.finalize().to_vec()
}

fn update_hmac(digest: &mut Hmac<Sha256>, value: &[u8]) {
    let length =
        u64::try_from(value.len()).expect("product decision digest input exceeds u64::MAX");
    Mac::update(digest, &length.to_be_bytes());
    Mac::update(digest, value);
}

fn update_sha256(digest: &mut Sha256, value: &[u8]) {
    let length =
        u64::try_from(value.len()).expect("product decision digest input exceeds u64::MAX");
    Digest::update(digest, length.to_be_bytes());
    Digest::update(digest, value);
}

fn lower_hex(bytes: &[u8]) -> String {
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
    use super::*;

    #[test]
    fn hmac_is_domain_separated_and_key_bound() {
        let first =
            ProductDecisionDigestKeyV1::from_bytes("v1", std::array::from_fn(|index| index as u8))
                .unwrap();
        let second = ProductDecisionDigestKeyV1::from_bytes(
            "v2",
            std::array::from_fn(|index| 255_u8.wrapping_sub(index as u8)),
        )
        .unwrap();
        let fields = [b"tenant".as_slice(), b"same-low-entropy-key".as_slice()];
        assert_ne!(
            keyed_digest(&first, IDEMPOTENCY_DOMAIN, &fields),
            keyed_digest(&second, IDEMPOTENCY_DOMAIN, &fields)
        );
        assert_ne!(
            keyed_digest(&first, IDEMPOTENCY_DOMAIN, &fields),
            keyed_digest(&first, RECEIPT_ID_DOMAIN, &fields)
        );
        assert_ne!(
            keyed_digest(&first, IDEMPOTENCY_DOMAIN, &fields),
            keyed_digest(&first, SESSION_SUBJECT_DOMAIN, &fields)
        );
    }

    #[test]
    fn length_framing_distinguishes_ambiguous_field_boundaries() {
        assert_ne!(
            unkeyed_digest(SEMANTIC_REQUEST_DOMAIN, &[b"a", b"bc"]),
            unkeyed_digest(SEMANTIC_REQUEST_DOMAIN, &[b"ab", b"c"])
        );
    }

    #[test]
    fn session_subject_is_stable_opaque_and_domain_separated() {
        let session = [23_u8; 32];
        let fields = [
            b"tenant".as_slice(),
            b"principal".as_slice(),
            session.as_slice(),
        ];
        let first = unkeyed_digest_bytes(SESSION_SUBJECT_DOMAIN, &fields);
        let second = unkeyed_digest_bytes(SESSION_SUBJECT_DOMAIN, &fields);
        assert_eq!(first, second);
        assert_eq!(first.len(), 32);
        assert_ne!(first, session);
        assert_ne!(
            first,
            unkeyed_digest_bytes(SEMANTIC_REQUEST_DOMAIN, &fields)
        );
    }

    #[test]
    fn key_material_fingerprint_changes_when_an_identifier_is_reused() {
        let first = ProductDecisionDigestKeyV1::from_bytes(
            "reused",
            std::array::from_fn(|index| 17_u8.wrapping_add(index as u8)),
        )
        .unwrap();
        let second = ProductDecisionDigestKeyV1::from_bytes(
            "reused",
            std::array::from_fn(|index| 113_u8.wrapping_add(index as u8)),
        )
        .unwrap();
        assert_ne!(
            unkeyed_digest(KEY_MATERIAL_FINGERPRINT_DOMAIN, &[first.secret()]),
            unkeyed_digest(KEY_MATERIAL_FINGERPRINT_DOMAIN, &[second.secret()])
        );
    }

    #[test]
    fn keyring_coverage_identity_preserves_secret_order_without_secret_material() {
        let first = ProductDecisionDigestKeyV1::from_bytes(
            "first",
            std::array::from_fn(|index| 31_u8.wrapping_add(index as u8)),
        )
        .unwrap();
        let second = ProductDecisionDigestKeyV1::from_bytes(
            "second",
            std::array::from_fn(|index| 97_u8.wrapping_add(index as u8)),
        )
        .unwrap();
        let keyring = ProductDecisionDigestKeyringV1::new(first, [second]).unwrap();
        let identity = keyring_coverage_identity(&keyring);
        assert_eq!(identity.key_ids, ["first", "second"]);
        assert_eq!(identity.key_fingerprints.len(), 2);
        assert!(identity
            .key_fingerprints
            .iter()
            .all(|fingerprint| fingerprint.len() == 64));
        assert!(!format!("{:?}", keyring).contains(&identity.key_fingerprints[0]));
    }
}
