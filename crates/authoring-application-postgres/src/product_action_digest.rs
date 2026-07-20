use std::collections::BTreeSet;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

const MAX_DIGEST_KEYS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductActionDigestKeyError {
    #[error("product action digest key ID is invalid")]
    InvalidKeyId,
    #[error("product action digest key material is not production-safe")]
    WeakKeyMaterial,
}

pub struct ProductActionDigestKeyV1 {
    key_id: String,
    secret: Zeroizing<[u8; 32]>,
}

impl ProductActionDigestKeyV1 {
    pub fn from_bytes(key_id: &str, secret: [u8; 32]) -> Result<Self, ProductActionDigestKeyError> {
        let secret = Zeroizing::new(secret);
        if key_id.is_empty()
            || key_id.len() > 64
            || !key_id.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')
            })
        {
            return Err(ProductActionDigestKeyError::InvalidKeyId);
        }
        if obvious_repetition(&secret) {
            return Err(ProductActionDigestKeyError::WeakKeyMaterial);
        }
        Ok(Self {
            key_id: key_id.to_string(),
            secret,
        })
    }

    pub(crate) fn key_id(&self) -> &str {
        &self.key_id
    }

    pub(crate) fn secret(&self) -> &[u8; 32] {
        &self.secret
    }
}

impl Debug for ProductActionDigestKeyV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductDecisionDigestKeyV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductActionDigestKeyringError {
    #[error("product action digest keyring must contain one to eight unique keys")]
    InvalidKeyring,
}

#[derive(Clone)]
pub struct ProductActionDigestKeyringV1 {
    keys: Arc<[ProductActionDigestKeyV1]>,
}

impl ProductActionDigestKeyringV1 {
    pub fn new(
        active: ProductActionDigestKeyV1,
        retired: impl IntoIterator<Item = ProductActionDigestKeyV1>,
    ) -> Result<Self, ProductActionDigestKeyringError> {
        let keys = std::iter::once(active).chain(retired).collect::<Vec<_>>();
        let unique_ids = keys
            .iter()
            .map(ProductActionDigestKeyV1::key_id)
            .collect::<BTreeSet<_>>();
        let duplicate_material = keys.iter().enumerate().any(|(index, candidate)| {
            keys.iter()
                .skip(index + 1)
                .any(|other| bool::from(candidate.secret().ct_eq(other.secret())))
        });
        if keys.is_empty()
            || keys.len() > MAX_DIGEST_KEYS
            || unique_ids.len() != keys.len()
            || duplicate_material
        {
            return Err(ProductActionDigestKeyringError::InvalidKeyring);
        }
        Ok(Self { keys: keys.into() })
    }

    pub(crate) fn active(&self) -> &ProductActionDigestKeyV1 {
        &self.keys[0]
    }

    pub(crate) fn keys(&self) -> &[ProductActionDigestKeyV1] {
        &self.keys
    }
}

impl Debug for ProductActionDigestKeyringV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductDecisionDigestKeyringV1(<redacted>)")
    }
}

pub(crate) struct ProductActionKeyringCoverageIdentityV1 {
    pub key_ids: Vec<String>,
    pub key_fingerprints: Vec<String>,
}

pub(crate) fn product_action_keyring_coverage_identity_v1(
    keyring: &ProductActionDigestKeyringV1,
    key_material_fingerprint_domain: &[u8],
) -> ProductActionKeyringCoverageIdentityV1 {
    ProductActionKeyringCoverageIdentityV1 {
        key_ids: keyring
            .keys()
            .iter()
            .map(|key| key.key_id().to_string())
            .collect(),
        key_fingerprints: keyring
            .keys()
            .iter()
            .map(|key| unkeyed_digest(key_material_fingerprint_domain, &[key.secret()]))
            .collect(),
    }
}

pub(crate) fn product_action_session_subject_digest_v1(
    domain: &[u8],
    tenant_id: &[u8],
    principal_id: &[u8],
    session_fingerprint: &[u8],
) -> Vec<u8> {
    unkeyed_digest_bytes(domain, &[tenant_id, principal_id, session_fingerprint])
}

pub(crate) fn keyed_digest(
    key: &ProductActionDigestKeyV1,
    domain: &[u8],
    fields: &[&[u8]],
) -> String {
    lower_hex(&keyed_digest_bytes(key, domain, fields))
}

pub(crate) fn keyed_digest_bytes(
    key: &ProductActionDigestKeyV1,
    domain: &[u8],
    fields: &[&[u8]],
) -> Vec<u8> {
    let mut hmac = <Hmac<Sha256> as Mac>::new_from_slice(key.secret())
        .expect("validated product action digest key has a supported length");
    update_hmac(&mut hmac, domain);
    for field in fields {
        update_hmac(&mut hmac, field);
    }
    hmac.finalize().into_bytes().to_vec()
}

pub(crate) fn unkeyed_digest(domain: &[u8], fields: &[&[u8]]) -> String {
    unkeyed_digest_owned(domain, fields.iter().copied())
}

pub(crate) fn unkeyed_digest_bytes(domain: &[u8], fields: &[&[u8]]) -> Vec<u8> {
    unkeyed_digest_bytes_owned(domain, fields.iter().copied())
}

fn unkeyed_digest_owned<'a>(domain: &[u8], fields: impl IntoIterator<Item = &'a [u8]>) -> String {
    lower_hex(&unkeyed_digest_bytes_owned(domain, fields))
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
    let length = u64::try_from(value.len()).expect("product action digest input exceeds u64::MAX");
    Mac::update(digest, &length.to_be_bytes());
    Mac::update(digest, value);
}

fn update_sha256(digest: &mut Sha256, value: &[u8]) {
    let length = u64::try_from(value.len()).expect("product action digest input exceeds u64::MAX");
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

fn obvious_repetition(secret: &[u8; 32]) -> bool {
    [1_usize, 2, 4, 8, 16]
        .into_iter()
        .any(|period| (period..secret.len()).all(|index| secret[index] == secret[index % period]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(id: &str, byte: u8) -> ProductActionDigestKeyV1 {
        ProductActionDigestKeyV1::from_bytes(
            id,
            std::array::from_fn(|index| byte.wrapping_add(index as u8)),
        )
        .unwrap()
    }

    #[test]
    fn keyring_preserves_active_then_retired_order_without_exposing_secrets() {
        let ring =
            ProductActionDigestKeyringV1::new(key("active-v2", 2), [key("old-v1", 1)]).unwrap();
        assert_eq!(ring.active().key_id(), "active-v2");
        assert_eq!(ring.keys()[1].key_id(), "old-v1");
        assert_eq!(
            format!("{ring:?}"),
            "ProductDecisionDigestKeyringV1(<redacted>)"
        );
        assert_eq!(
            format!("{:?}", ring.active()),
            "ProductDecisionDigestKeyV1(<redacted>)"
        );
    }

    #[test]
    fn key_and_keyring_reject_ambiguous_configuration() {
        assert_eq!(
            ProductActionDigestKeyV1::from_bytes("bad key", [1; 32]).unwrap_err(),
            ProductActionDigestKeyError::InvalidKeyId
        );
        assert_eq!(
            ProductActionDigestKeyringV1::new(key("same", 1), [key("same", 2)]).unwrap_err(),
            ProductActionDigestKeyringError::InvalidKeyring
        );
    }

    #[test]
    fn keyring_rejects_repeated_or_aliased_secret_material() {
        assert_eq!(
            ProductActionDigestKeyV1::from_bytes("zero", [0; 32]).unwrap_err(),
            ProductActionDigestKeyError::WeakKeyMaterial
        );
        assert_eq!(
            ProductActionDigestKeyV1::from_bytes("repeat", [7; 32]).unwrap_err(),
            ProductActionDigestKeyError::WeakKeyMaterial
        );
        let material = std::array::from_fn(|index| index as u8);
        let first = ProductActionDigestKeyV1::from_bytes("first", material).unwrap();
        let second = ProductActionDigestKeyV1::from_bytes("second", material).unwrap();
        assert_eq!(
            ProductActionDigestKeyringV1::new(first, [second]).unwrap_err(),
            ProductActionDigestKeyringError::InvalidKeyring
        );
    }

    #[test]
    fn keyring_rejects_more_than_eight_keys() {
        let active = key("active", 1);
        let retired = (0_u8..8)
            .map(|index| key(&format!("retired-{index}"), index.wrapping_add(20)))
            .collect::<Vec<_>>();
        assert_eq!(
            ProductActionDigestKeyringV1::new(active, retired).unwrap_err(),
            ProductActionDigestKeyringError::InvalidKeyring
        );
    }

    #[test]
    fn framed_digests_are_domain_key_and_field_boundary_separated() {
        let first = key("first", 31);
        let second = key("second", 97);
        let fields = [b"tenant".as_slice(), b"same-low-entropy-key".as_slice()];
        assert_ne!(
            keyed_digest(&first, b"domain-a", &fields),
            keyed_digest(&second, b"domain-a", &fields)
        );
        assert_ne!(
            keyed_digest(&first, b"domain-a", &fields),
            keyed_digest(&first, b"domain-b", &fields)
        );
        assert_ne!(
            unkeyed_digest(b"domain-a", &[b"a", b"bc"]),
            unkeyed_digest(b"domain-a", &[b"ab", b"c"])
        );
    }

    #[test]
    fn shared_session_subject_and_keyring_coverage_are_stable_and_opaque() {
        let keyring =
            ProductActionDigestKeyringV1::new(key("active", 31), [key("retired", 97)]).unwrap();
        let first = product_action_keyring_coverage_identity_v1(&keyring, b"fingerprint-domain");
        let second = product_action_keyring_coverage_identity_v1(&keyring, b"fingerprint-domain");
        assert_eq!(first.key_ids, ["active", "retired"]);
        assert_eq!(first.key_fingerprints, second.key_fingerprints);
        assert!(first
            .key_fingerprints
            .iter()
            .all(|fingerprint| fingerprint.len() == 64));
        let session = [23_u8; 32];
        let subject = product_action_session_subject_digest_v1(
            b"session-subject-domain",
            b"tenant",
            b"principal",
            &session,
        );
        assert_eq!(subject.len(), 32);
        assert_ne!(subject, session);
    }

    #[test]
    fn legacy_product_decision_names_remain_source_compatible_aliases() {
        let legacy_key: crate::ProductDecisionDigestKeyV1 =
            crate::ProductDecisionDigestKeyV1::from_bytes(
                "legacy",
                std::array::from_fn(|index| 53_u8.wrapping_add(index as u8)),
            )
            .unwrap();
        let common_key: ProductActionDigestKeyV1 = legacy_key;
        let legacy_ring: crate::ProductDecisionDigestKeyringV1 =
            crate::ProductDecisionDigestKeyringV1::new(common_key, []).unwrap();
        let common_ring: ProductActionDigestKeyringV1 = legacy_ring;
        assert_eq!(common_ring.active().key_id(), "legacy");
        let legacy_error: crate::ProductDecisionDigestKeyError =
            crate::ProductDecisionDigestKeyV1::from_bytes("bad key", [1; 32]).unwrap_err();
        assert_eq!(legacy_error, ProductActionDigestKeyError::InvalidKeyId);
    }
}
