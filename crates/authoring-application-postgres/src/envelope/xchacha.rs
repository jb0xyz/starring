use std::collections::BTreeSet;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;

use chacha20poly1305::aead::{AeadInOut, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use super::{
    validate_key_id, EncryptedSnapshotEnvelopeV1, SnapshotEnvelopeCipher,
    SnapshotEnvelopeCipherError, SnapshotEnvelopeEncryptionPort,
};

pub const XCHACHA20_POLY1305_SNAPSHOT_SUITE_V1: &str = "xchacha20_poly1305";
pub const XCHACHA20_POLY1305_SNAPSHOT_SUITE_VERSION_V1: u16 = 1;
pub const XCHACHA20_POLY1305_SNAPSHOT_NONCE_BYTES_V1: usize = 24;

const MAX_SNAPSHOT_ENVELOPE_KEYS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SnapshotEnvelopeKeyError {
    #[error("snapshot envelope key identifier is invalid")]
    InvalidKeyId,
    #[error("snapshot envelope key material is obviously repetitive")]
    ObviouslyRepetitiveKeyMaterial,
}

pub struct SnapshotEnvelopeKeyV1 {
    key_id: String,
    secret: Zeroizing<[u8; 32]>,
}

impl SnapshotEnvelopeKeyV1 {
    pub fn new(
        key_id: &str,
        secret: Zeroizing<[u8; 32]>,
    ) -> Result<Self, SnapshotEnvelopeKeyError> {
        validate_key_id(key_id).map_err(|_| SnapshotEnvelopeKeyError::InvalidKeyId)?;
        if obvious_repetition(&secret) {
            return Err(SnapshotEnvelopeKeyError::ObviouslyRepetitiveKeyMaterial);
        }
        Ok(Self {
            key_id: key_id.to_string(),
            secret,
        })
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    fn secret(&self) -> &[u8; 32] {
        &self.secret
    }
}

impl Debug for SnapshotEnvelopeKeyV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SnapshotEnvelopeKeyV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SnapshotEnvelopeKeyringError {
    #[error("snapshot envelope keyring exceeds the supported key count")]
    TooManyKeys,
    #[error("snapshot envelope keyring contains a duplicate key identifier")]
    DuplicateKeyId,
    #[error("snapshot envelope keyring aliases key material under multiple identifiers")]
    AliasedKeyMaterial,
}

#[derive(Clone)]
pub struct SnapshotEnvelopeKeyringV1 {
    keys: Arc<[SnapshotEnvelopeKeyV1]>,
}

impl SnapshotEnvelopeKeyringV1 {
    pub fn new(
        active: SnapshotEnvelopeKeyV1,
        retired: impl IntoIterator<Item = SnapshotEnvelopeKeyV1>,
    ) -> Result<Self, SnapshotEnvelopeKeyringError> {
        let keys = std::iter::once(active).chain(retired).collect::<Vec<_>>();
        if keys.len() > MAX_SNAPSHOT_ENVELOPE_KEYS {
            return Err(SnapshotEnvelopeKeyringError::TooManyKeys);
        }
        let unique_ids = keys
            .iter()
            .map(SnapshotEnvelopeKeyV1::key_id)
            .collect::<BTreeSet<_>>();
        if unique_ids.len() != keys.len() {
            return Err(SnapshotEnvelopeKeyringError::DuplicateKeyId);
        }
        let aliased_material = keys.iter().enumerate().any(|(index, candidate)| {
            keys.iter()
                .skip(index + 1)
                .any(|other| bool::from(candidate.secret().ct_eq(other.secret())))
        });
        if aliased_material {
            return Err(SnapshotEnvelopeKeyringError::AliasedKeyMaterial);
        }
        Ok(Self { keys: keys.into() })
    }

    pub fn active_key_id(&self) -> &str {
        self.keys[0].key_id()
    }

    pub fn configured_key_count(&self) -> usize {
        self.keys.len()
    }

    pub fn supports_key_id(&self, key_id: &str) -> bool {
        self.key_for_id(key_id).is_some()
    }

    fn key_for_id(&self, key_id: &str) -> Option<&SnapshotEnvelopeKeyV1> {
        self.keys.iter().find(|key| key.key_id() == key_id)
    }

    fn active(&self) -> &SnapshotEnvelopeKeyV1 {
        &self.keys[0]
    }
}

impl Debug for SnapshotEnvelopeKeyringV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SnapshotEnvelopeKeyringV1(<redacted>)")
    }
}

#[derive(Clone)]
pub struct XChaCha20Poly1305SnapshotEnvelopeCipherV1 {
    keyring: SnapshotEnvelopeKeyringV1,
}

impl XChaCha20Poly1305SnapshotEnvelopeCipherV1 {
    pub fn new(keyring: SnapshotEnvelopeKeyringV1) -> Self {
        Self { keyring }
    }
}

impl Debug for XChaCha20Poly1305SnapshotEnvelopeCipherV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("XChaCha20Poly1305SnapshotEnvelopeCipherV1(<redacted>)")
    }
}

impl SnapshotEnvelopeCipher for XChaCha20Poly1305SnapshotEnvelopeCipherV1 {
    fn configured_encryption_key_ids(&self) -> Option<Vec<&str>> {
        Some(
            self.keyring
                .keys
                .iter()
                .map(SnapshotEnvelopeKeyV1::key_id)
                .collect(),
        )
    }

    async fn decrypt(
        &self,
        envelope: &EncryptedSnapshotEnvelopeV1,
        authenticated_data: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, SnapshotEnvelopeCipherError> {
        if envelope.encryption_suite() != XCHACHA20_POLY1305_SNAPSHOT_SUITE_V1
            || envelope.encryption_suite_version() != XCHACHA20_POLY1305_SNAPSHOT_SUITE_VERSION_V1
            || envelope.nonce().len() != XCHACHA20_POLY1305_SNAPSHOT_NONCE_BYTES_V1
        {
            return Err(SnapshotEnvelopeCipherError::UnsupportedEnvelope);
        }
        if authenticated_data.is_empty() {
            return Err(SnapshotEnvelopeCipherError::AuthenticationFailed);
        }
        let key = self
            .keyring
            .key_for_id(envelope.encryption_key_id())
            .ok_or(SnapshotEnvelopeCipherError::KeyUnavailable)?;
        let nonce_bytes =
            <[u8; XCHACHA20_POLY1305_SNAPSHOT_NONCE_BYTES_V1]>::try_from(envelope.nonce())
                .map_err(|_| SnapshotEnvelopeCipherError::UnsupportedEnvelope)?;
        let cipher_key: &Key = key.secret().into();
        let cipher = XChaCha20Poly1305::new(cipher_key);
        let nonce = XNonce::from(nonce_bytes);
        let mut plaintext = Zeroizing::new(envelope.ciphertext().to_vec());
        cipher
            .decrypt_in_place(&nonce, authenticated_data, &mut *plaintext)
            .map_err(|_| SnapshotEnvelopeCipherError::AuthenticationFailed)?;
        Ok(plaintext)
    }
}

impl SnapshotEnvelopeEncryptionPort for XChaCha20Poly1305SnapshotEnvelopeCipherV1 {
    fn active_encryption_key_id(&self) -> &str {
        self.keyring.active_key_id()
    }

    fn encryption_suite(&self) -> &str {
        XCHACHA20_POLY1305_SNAPSHOT_SUITE_V1
    }

    fn encryption_suite_version(&self) -> u16 {
        XCHACHA20_POLY1305_SNAPSHOT_SUITE_VERSION_V1
    }

    fn encrypt(
        &self,
        plaintext: &Zeroizing<Vec<u8>>,
        authenticated_data: &[u8],
    ) -> Result<EncryptedSnapshotEnvelopeV1, SnapshotEnvelopeCipherError> {
        if plaintext.is_empty() || authenticated_data.is_empty() {
            return Err(SnapshotEnvelopeCipherError::AuthenticationFailed);
        }
        let mut nonce_bytes = [0_u8; XCHACHA20_POLY1305_SNAPSHOT_NONCE_BYTES_V1];
        getrandom::fill(&mut nonce_bytes).map_err(|_| SnapshotEnvelopeCipherError::Backend)?;
        let active = self.keyring.active();
        let cipher_key: &Key = active.secret().into();
        let cipher = XChaCha20Poly1305::new(cipher_key);
        let nonce = XNonce::from(nonce_bytes);
        let mut ciphertext = Zeroizing::new(plaintext.as_slice().to_vec());
        cipher
            .encrypt_in_place(&nonce, authenticated_data, &mut *ciphertext)
            .map_err(|_| SnapshotEnvelopeCipherError::Backend)?;
        let ciphertext = std::mem::take(&mut *ciphertext);
        EncryptedSnapshotEnvelopeV1::from_persisted_parts(
            ciphertext,
            nonce_bytes.to_vec(),
            active.key_id().to_string(),
            XCHACHA20_POLY1305_SNAPSHOT_SUITE_V1.to_string(),
            XCHACHA20_POLY1305_SNAPSHOT_SUITE_VERSION_V1,
        )
        .map_err(|_| SnapshotEnvelopeCipherError::Backend)
    }
}

fn obvious_repetition(secret: &[u8; 32]) -> bool {
    [1_usize, 2, 4, 8, 16]
        .into_iter()
        .any(|period| (period..secret.len()).all(|index| secret[index] == secret[index % period]))
}

#[cfg(test)]
mod tests {
    use chacha20poly1305::aead::{Aead, Payload};

    use super::*;

    const AUTHENTICATED_DATA: &[u8] = b"starring.snapshot.test.aad.v1";
    const PLAINTEXT: &[u8] = b"durable authoring snapshot";

    fn material(seed: u8) -> [u8; 32] {
        std::array::from_fn(|index| seed.wrapping_add((index as u8).wrapping_mul(17)))
    }

    fn key(key_id: &str, seed: u8) -> SnapshotEnvelopeKeyV1 {
        SnapshotEnvelopeKeyV1::new(key_id, Zeroizing::new(material(seed))).unwrap()
    }

    fn keyring(
        active_id: &str,
        active_seed: u8,
        retired: impl IntoIterator<Item = SnapshotEnvelopeKeyV1>,
    ) -> SnapshotEnvelopeKeyringV1 {
        SnapshotEnvelopeKeyringV1::new(key(active_id, active_seed), retired).unwrap()
    }

    fn encrypted_envelope(
        key_id: &str,
        secret: [u8; 32],
        nonce: [u8; 24],
        authenticated_data: &[u8],
        plaintext: &[u8],
    ) -> EncryptedSnapshotEnvelopeV1 {
        let cipher = XChaCha20Poly1305::new(&Key::from(secret));
        let ciphertext = cipher
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: plaintext,
                    aad: authenticated_data,
                },
            )
            .unwrap();
        EncryptedSnapshotEnvelopeV1::from_persisted_parts(
            ciphertext,
            nonce.to_vec(),
            key_id.to_string(),
            XCHACHA20_POLY1305_SNAPSHOT_SUITE_V1.to_string(),
            XCHACHA20_POLY1305_SNAPSHOT_SUITE_VERSION_V1,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn active_and_retired_keys_decrypt_without_exposing_secrets() {
        let active_envelope = encrypted_envelope(
            "active-v2",
            material(41),
            [3; 24],
            AUTHENTICATED_DATA,
            PLAINTEXT,
        );
        let retired_envelope = encrypted_envelope(
            "retired-v1",
            material(19),
            [5; 24],
            AUTHENTICATED_DATA,
            PLAINTEXT,
        );
        let ring = keyring("active-v2", 41, [key("retired-v1", 19)]);
        assert_eq!(ring.active_key_id(), "active-v2");
        assert_eq!(ring.configured_key_count(), 2);
        assert!(ring.supports_key_id("retired-v1"));
        assert!(!ring.supports_key_id("missing"));
        assert_eq!(format!("{ring:?}"), "SnapshotEnvelopeKeyringV1(<redacted>)");
        let cipher = XChaCha20Poly1305SnapshotEnvelopeCipherV1::new(ring);
        assert_eq!(
            cipher.configured_encryption_key_ids(),
            Some(vec!["active-v2", "retired-v1"])
        );
        assert_eq!(
            format!("{cipher:?}"),
            "XChaCha20Poly1305SnapshotEnvelopeCipherV1(<redacted>)"
        );
        assert_eq!(
            cipher
                .decrypt(&active_envelope, AUTHENTICATED_DATA)
                .await
                .unwrap()
                .as_slice(),
            PLAINTEXT
        );
        assert_eq!(
            cipher
                .decrypt(&retired_envelope, AUTHENTICATED_DATA)
                .await
                .unwrap()
                .as_slice(),
            PLAINTEXT
        );
        assert_eq!(
            format!("{:?}", key("debug-key", 83)),
            "SnapshotEnvelopeKeyV1(<redacted>)"
        );
    }

    #[tokio::test]
    async fn encryption_uses_the_active_key_and_a_fresh_twenty_four_byte_nonce() {
        let cipher = XChaCha20Poly1305SnapshotEnvelopeCipherV1::new(keyring(
            "active-v2",
            41,
            [key("retired-v1", 19)],
        ));
        let plaintext = Zeroizing::new(PLAINTEXT.to_vec());
        let first = cipher.encrypt(&plaintext, AUTHENTICATED_DATA).unwrap();
        let second = cipher.encrypt(&plaintext, AUTHENTICATED_DATA).unwrap();
        assert_eq!(first.encryption_key_id(), "active-v2");
        assert_eq!(second.encryption_key_id(), "active-v2");
        assert_eq!(
            first.nonce().len(),
            XCHACHA20_POLY1305_SNAPSHOT_NONCE_BYTES_V1
        );
        assert_ne!(first.nonce(), second.nonce());
        assert_ne!(first.ciphertext(), second.ciphertext());
        assert_eq!(
            cipher
                .decrypt(&first, AUTHENTICATED_DATA)
                .await
                .unwrap()
                .as_slice(),
            PLAINTEXT
        );
        assert_eq!(
            cipher
                .decrypt(&second, AUTHENTICATED_DATA)
                .await
                .unwrap()
                .as_slice(),
            PLAINTEXT
        );
    }

    #[tokio::test]
    async fn missing_key_and_wrong_key_fail_closed() {
        let envelope = encrypted_envelope(
            "expected-key",
            material(31),
            [7; 24],
            AUTHENTICATED_DATA,
            PLAINTEXT,
        );
        let missing = XChaCha20Poly1305SnapshotEnvelopeCipherV1::new(keyring("other-key", 31, []));
        assert_eq!(
            missing.decrypt(&envelope, AUTHENTICATED_DATA).await,
            Err(SnapshotEnvelopeCipherError::KeyUnavailable)
        );
        let wrong = XChaCha20Poly1305SnapshotEnvelopeCipherV1::new(keyring("expected-key", 97, []));
        assert_eq!(
            wrong.decrypt(&envelope, AUTHENTICATED_DATA).await,
            Err(SnapshotEnvelopeCipherError::AuthenticationFailed)
        );
    }

    #[tokio::test]
    async fn unsupported_suite_version_and_nonce_lengths_are_rejected() {
        let cipher = XChaCha20Poly1305SnapshotEnvelopeCipherV1::new(keyring("key-v1", 13, []));
        for (suite, version, nonce) in [
            ("aes256_gcm", 1, vec![1; 24]),
            (XCHACHA20_POLY1305_SNAPSHOT_SUITE_V1, 2, vec![1; 24]),
            (XCHACHA20_POLY1305_SNAPSHOT_SUITE_V1, 1, vec![1; 23]),
            (XCHACHA20_POLY1305_SNAPSHOT_SUITE_V1, 1, vec![1; 25]),
        ] {
            let envelope = EncryptedSnapshotEnvelopeV1::from_persisted_parts(
                vec![1; 16],
                nonce,
                "key-v1".to_string(),
                suite.to_string(),
                version,
            )
            .unwrap();
            assert_eq!(
                cipher.decrypt(&envelope, AUTHENTICATED_DATA).await,
                Err(SnapshotEnvelopeCipherError::UnsupportedEnvelope)
            );
        }
    }

    #[tokio::test]
    async fn ciphertext_tag_nonce_and_authenticated_data_tampering_are_rejected() {
        let secret = material(71);
        let nonce = [11; 24];
        let envelope = encrypted_envelope("key-v1", secret, nonce, AUTHENTICATED_DATA, PLAINTEXT);
        let cipher = XChaCha20Poly1305SnapshotEnvelopeCipherV1::new(keyring("key-v1", 71, []));
        let mut ciphertext_tampered = envelope.ciphertext().to_vec();
        ciphertext_tampered[0] ^= 1;
        let ciphertext_tampered = EncryptedSnapshotEnvelopeV1::from_persisted_parts(
            ciphertext_tampered,
            nonce.to_vec(),
            "key-v1".to_string(),
            XCHACHA20_POLY1305_SNAPSHOT_SUITE_V1.to_string(),
            1,
        )
        .unwrap();
        let mut tag_tampered = envelope.ciphertext().to_vec();
        let tag_index = tag_tampered.len() - 1;
        tag_tampered[tag_index] ^= 1;
        let tag_tampered = EncryptedSnapshotEnvelopeV1::from_persisted_parts(
            tag_tampered,
            nonce.to_vec(),
            "key-v1".to_string(),
            XCHACHA20_POLY1305_SNAPSHOT_SUITE_V1.to_string(),
            1,
        )
        .unwrap();
        let mut altered_nonce = nonce;
        altered_nonce[0] ^= 1;
        let nonce_tampered = EncryptedSnapshotEnvelopeV1::from_persisted_parts(
            envelope.ciphertext().to_vec(),
            altered_nonce.to_vec(),
            "key-v1".to_string(),
            XCHACHA20_POLY1305_SNAPSHOT_SUITE_V1.to_string(),
            1,
        )
        .unwrap();
        for candidate in [&ciphertext_tampered, &tag_tampered, &nonce_tampered] {
            assert_eq!(
                cipher.decrypt(candidate, AUTHENTICATED_DATA).await,
                Err(SnapshotEnvelopeCipherError::AuthenticationFailed)
            );
        }
        assert_eq!(
            cipher.decrypt(&envelope, b"altered-aad").await,
            Err(SnapshotEnvelopeCipherError::AuthenticationFailed)
        );
        assert_eq!(
            cipher.decrypt(&envelope, b"").await,
            Err(SnapshotEnvelopeCipherError::AuthenticationFailed)
        );
    }

    #[tokio::test]
    async fn authenticated_key_identifier_cannot_be_renamed() {
        let secret = material(107);
        let original_aad = b"key-id:key-a";
        let renamed_aad = b"key-id:key-b";
        let encrypted = encrypted_envelope("key-b", secret, [17; 24], original_aad, PLAINTEXT);
        let cipher = XChaCha20Poly1305SnapshotEnvelopeCipherV1::new(keyring("key-b", 107, []));
        assert_eq!(
            cipher.decrypt(&encrypted, renamed_aad).await,
            Err(SnapshotEnvelopeCipherError::AuthenticationFailed)
        );
    }

    #[test]
    fn keys_and_keyrings_reject_ambiguous_or_weak_configuration() {
        assert_eq!(
            SnapshotEnvelopeKeyV1::new("bad key", Zeroizing::new(material(1))).unwrap_err(),
            SnapshotEnvelopeKeyError::InvalidKeyId
        );
        for secret in [
            [0; 32],
            [7; 32],
            std::array::from_fn(|index| (index % 8) as u8),
        ] {
            assert_eq!(
                SnapshotEnvelopeKeyV1::new("weak", Zeroizing::new(secret)).unwrap_err(),
                SnapshotEnvelopeKeyError::ObviouslyRepetitiveKeyMaterial
            );
        }
        assert_eq!(
            SnapshotEnvelopeKeyringV1::new(key("same", 1), [key("same", 2)]).unwrap_err(),
            SnapshotEnvelopeKeyringError::DuplicateKeyId
        );
        let aliased = material(29);
        assert_eq!(
            SnapshotEnvelopeKeyringV1::new(
                SnapshotEnvelopeKeyV1::new("first", Zeroizing::new(aliased)).unwrap(),
                [SnapshotEnvelopeKeyV1::new("second", Zeroizing::new(aliased)).unwrap()],
            )
            .unwrap_err(),
            SnapshotEnvelopeKeyringError::AliasedKeyMaterial
        );
        let retired = (0_u8..8)
            .map(|index| key(&format!("retired-{index}"), index.wrapping_add(80)))
            .collect::<Vec<_>>();
        assert_eq!(
            SnapshotEnvelopeKeyringV1::new(key("active", 3), retired).unwrap_err(),
            SnapshotEnvelopeKeyringError::TooManyKeys
        );
    }
}
