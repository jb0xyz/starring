use std::collections::BTreeSet;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;

use chacha20poly1305::aead::{AeadInOut, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use crate::digest::{append_claim_root_v1, append_field};
use crate::{InteractionReceiptClaimRootV1, InteractionTokenAuthenticatedDataDigestV1};

pub const XCHACHA20_POLY1305_INTERACTION_TOKEN_SUITE_V1: &str = "xchacha20_poly1305";
pub const XCHACHA20_POLY1305_INTERACTION_TOKEN_SUITE_VERSION_V1: u16 = 1;
pub const XCHACHA20_POLY1305_INTERACTION_TOKEN_NONCE_BYTES_V1: usize = 24;
pub const MAX_INTERACTION_TOKEN_LIFETIME_MILLISECONDS_V1: u64 = 15 * 60 * 1_000;

const TOKEN_AUTHENTICATED_DATA_DOMAIN_V1: &[u8] =
    b"starring.runtime.interaction.token_envelope.v1\0";
const MAX_INTERACTION_TOKEN_BYTES: usize = 4_096;
const MIN_INTERACTION_TOKEN_CIPHERTEXT_BYTES: usize = 16;
const MAX_INTERACTION_TOKEN_CIPHERTEXT_BYTES: usize = MAX_INTERACTION_TOKEN_BYTES + 16;
const MAX_INTERACTION_TOKEN_ENVELOPE_KEYS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InteractionTokenErrorV1 {
    #[error("interaction token must not be empty")]
    Empty,
    #[error("interaction token exceeds the supported size")]
    TooLarge,
    #[error("interaction token contains an invalid null byte")]
    NullByte,
}

pub struct InteractionTokenV1(Zeroizing<String>);

impl InteractionTokenV1 {
    pub fn new(value: impl Into<String>) -> Result<Self, InteractionTokenErrorV1> {
        let value = Zeroizing::new(value.into());
        if value.is_empty() {
            return Err(InteractionTokenErrorV1::Empty);
        }
        if value.len() > MAX_INTERACTION_TOKEN_BYTES {
            return Err(InteractionTokenErrorV1::TooLarge);
        }
        if value.bytes().any(|byte| byte == 0) {
            return Err(InteractionTokenErrorV1::NullByte);
        }
        Ok(Self(value))
    }

    pub fn expose_secret(&self) -> &str {
        self.0.as_str()
    }
}

impl Debug for InteractionTokenV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("InteractionTokenV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InteractionTokenEnvelopeTimeErrorV1 {
    #[error("interaction token issued-at time must be non-zero")]
    InvalidIssuedAt,
    #[error("interaction token expiry must be after issued-at time")]
    InvalidExpiry,
    #[error("interaction token lifetime exceeds the supported duration")]
    LifetimeTooLong,
    #[error("interaction token is not valid yet")]
    NotYetValid,
    #[error("interaction token has expired")]
    Expired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InteractionTokenEnvelopeTimeV1 {
    issued_at_unix_milliseconds: u64,
    expires_at_unix_milliseconds: u64,
}

impl InteractionTokenEnvelopeTimeV1 {
    pub fn new(
        issued_at_unix_milliseconds: u64,
        expires_at_unix_milliseconds: u64,
    ) -> Result<Self, InteractionTokenEnvelopeTimeErrorV1> {
        if issued_at_unix_milliseconds == 0 {
            return Err(InteractionTokenEnvelopeTimeErrorV1::InvalidIssuedAt);
        }
        let lifetime = expires_at_unix_milliseconds
            .checked_sub(issued_at_unix_milliseconds)
            .ok_or(InteractionTokenEnvelopeTimeErrorV1::InvalidExpiry)?;
        if lifetime == 0 {
            return Err(InteractionTokenEnvelopeTimeErrorV1::InvalidExpiry);
        }
        if lifetime > MAX_INTERACTION_TOKEN_LIFETIME_MILLISECONDS_V1 {
            return Err(InteractionTokenEnvelopeTimeErrorV1::LifetimeTooLong);
        }
        Ok(Self {
            issued_at_unix_milliseconds,
            expires_at_unix_milliseconds,
        })
    }

    pub fn issued_at_unix_milliseconds(self) -> u64 {
        self.issued_at_unix_milliseconds
    }

    pub fn expires_at_unix_milliseconds(self) -> u64 {
        self.expires_at_unix_milliseconds
    }

    pub fn ensure_unexpired(
        self,
        now_unix_milliseconds: u64,
    ) -> Result<(), InteractionTokenEnvelopeTimeErrorV1> {
        if now_unix_milliseconds < self.issued_at_unix_milliseconds {
            return Err(InteractionTokenEnvelopeTimeErrorV1::NotYetValid);
        }
        if now_unix_milliseconds >= self.expires_at_unix_milliseconds {
            return Err(InteractionTokenEnvelopeTimeErrorV1::Expired);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InteractionTokenEnvelopeKeyErrorV1 {
    #[error("interaction token envelope key identifier is invalid")]
    InvalidKeyId,
    #[error("interaction token envelope key material is obviously repetitive")]
    ObviouslyRepetitiveKeyMaterial,
}

pub struct InteractionTokenEnvelopeKeyV1 {
    key_id: String,
    secret: Zeroizing<[u8; 32]>,
}

impl InteractionTokenEnvelopeKeyV1 {
    pub fn new(
        key_id: &str,
        secret: Zeroizing<[u8; 32]>,
    ) -> Result<Self, InteractionTokenEnvelopeKeyErrorV1> {
        validate_key_id(key_id)?;
        if obvious_repetition(&secret) {
            return Err(InteractionTokenEnvelopeKeyErrorV1::ObviouslyRepetitiveKeyMaterial);
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

impl Debug for InteractionTokenEnvelopeKeyV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("InteractionTokenEnvelopeKeyV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InteractionTokenEnvelopeKeyringErrorV1 {
    #[error("interaction token envelope keyring exceeds the supported key count")]
    TooManyKeys,
    #[error("interaction token envelope keyring contains a duplicate key identifier")]
    DuplicateKeyId,
    #[error("interaction token envelope keyring aliases key material under multiple identifiers")]
    AliasedKeyMaterial,
}

#[derive(Clone)]
pub struct InteractionTokenEnvelopeKeyringV1 {
    keys: Arc<[InteractionTokenEnvelopeKeyV1]>,
}

impl InteractionTokenEnvelopeKeyringV1 {
    pub fn new(
        active: InteractionTokenEnvelopeKeyV1,
        retired: impl IntoIterator<Item = InteractionTokenEnvelopeKeyV1>,
    ) -> Result<Self, InteractionTokenEnvelopeKeyringErrorV1> {
        let keys = std::iter::once(active).chain(retired).collect::<Vec<_>>();
        if keys.len() > MAX_INTERACTION_TOKEN_ENVELOPE_KEYS {
            return Err(InteractionTokenEnvelopeKeyringErrorV1::TooManyKeys);
        }
        let unique_ids = keys
            .iter()
            .map(InteractionTokenEnvelopeKeyV1::key_id)
            .collect::<BTreeSet<_>>();
        if unique_ids.len() != keys.len() {
            return Err(InteractionTokenEnvelopeKeyringErrorV1::DuplicateKeyId);
        }
        let aliased_material = keys.iter().enumerate().any(|(index, candidate)| {
            keys.iter()
                .skip(index + 1)
                .any(|other| bool::from(candidate.secret().ct_eq(other.secret())))
        });
        if aliased_material {
            return Err(InteractionTokenEnvelopeKeyringErrorV1::AliasedKeyMaterial);
        }
        Ok(Self { keys: keys.into() })
    }

    pub fn active_key_id(&self) -> &str {
        self.keys[0].key_id()
    }

    pub fn configured_key_ids(&self) -> Vec<&str> {
        self.keys
            .iter()
            .map(InteractionTokenEnvelopeKeyV1::key_id)
            .collect()
    }

    pub fn supports_key_id(&self, key_id: &str) -> bool {
        self.key_for_id(key_id).is_some()
    }

    fn active(&self) -> &InteractionTokenEnvelopeKeyV1 {
        &self.keys[0]
    }

    fn key_for_id(&self, key_id: &str) -> Option<&InteractionTokenEnvelopeKeyV1> {
        self.keys.iter().find(|key| key.key_id() == key_id)
    }
}

impl Debug for InteractionTokenEnvelopeKeyringV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("InteractionTokenEnvelopeKeyringV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InteractionTokenEnvelopeValidationErrorV1 {
    #[error("interaction token envelope key identifier is invalid")]
    InvalidKeyId,
    #[error("interaction token envelope encryption suite is invalid")]
    InvalidSuite,
    #[error("interaction token envelope encryption suite version is invalid")]
    InvalidSuiteVersion,
    #[error("interaction token envelope ciphertext length is invalid")]
    InvalidCiphertext,
    #[error("interaction token envelope nonce length is invalid")]
    InvalidNonce,
    #[error("interaction token envelope authenticated-data digest is invalid")]
    InvalidAuthenticatedDataDigest,
    #[error("interaction token envelope time is invalid")]
    InvalidTime,
}

pub struct EncryptedInteractionTokenV1 {
    ciphertext: Vec<u8>,
    nonce: [u8; XCHACHA20_POLY1305_INTERACTION_TOKEN_NONCE_BYTES_V1],
    encryption_key_id: String,
    encryption_suite: String,
    encryption_suite_version: u16,
    time: InteractionTokenEnvelopeTimeV1,
    authenticated_data_digest: InteractionTokenAuthenticatedDataDigestV1,
}

impl EncryptedInteractionTokenV1 {
    pub fn from_persisted_parts(
        ciphertext: Vec<u8>,
        nonce: Vec<u8>,
        encryption_key_id: String,
        encryption_suite: String,
        encryption_suite_version: u16,
        time: InteractionTokenEnvelopeTimeV1,
        authenticated_data_digest: InteractionTokenAuthenticatedDataDigestV1,
    ) -> Result<Self, InteractionTokenEnvelopeValidationErrorV1> {
        validate_key_id(&encryption_key_id)
            .map_err(|_| InteractionTokenEnvelopeValidationErrorV1::InvalidKeyId)?;
        if encryption_suite != XCHACHA20_POLY1305_INTERACTION_TOKEN_SUITE_V1 {
            return Err(InteractionTokenEnvelopeValidationErrorV1::InvalidSuite);
        }
        if encryption_suite_version != XCHACHA20_POLY1305_INTERACTION_TOKEN_SUITE_VERSION_V1 {
            return Err(InteractionTokenEnvelopeValidationErrorV1::InvalidSuiteVersion);
        }
        if !(MIN_INTERACTION_TOKEN_CIPHERTEXT_BYTES..=MAX_INTERACTION_TOKEN_CIPHERTEXT_BYTES)
            .contains(&ciphertext.len())
        {
            return Err(InteractionTokenEnvelopeValidationErrorV1::InvalidCiphertext);
        }
        let nonce = <[u8; XCHACHA20_POLY1305_INTERACTION_TOKEN_NONCE_BYTES_V1]>::try_from(nonce)
            .map_err(|_| InteractionTokenEnvelopeValidationErrorV1::InvalidNonce)?;
        InteractionTokenEnvelopeTimeV1::new(
            time.issued_at_unix_milliseconds(),
            time.expires_at_unix_milliseconds(),
        )
        .map_err(|_| InteractionTokenEnvelopeValidationErrorV1::InvalidTime)?;
        InteractionTokenAuthenticatedDataDigestV1::parse(
            authenticated_data_digest.as_str().to_string(),
        )
        .map_err(|_| InteractionTokenEnvelopeValidationErrorV1::InvalidAuthenticatedDataDigest)?;
        Ok(Self {
            ciphertext,
            nonce,
            encryption_key_id,
            encryption_suite,
            encryption_suite_version,
            time,
            authenticated_data_digest,
        })
    }

    pub fn ciphertext(&self) -> &[u8] {
        &self.ciphertext
    }

    pub fn nonce(&self) -> &[u8] {
        &self.nonce
    }

    pub fn encryption_key_id(&self) -> &str {
        &self.encryption_key_id
    }

    pub fn encryption_suite(&self) -> &str {
        &self.encryption_suite
    }

    pub fn encryption_suite_version(&self) -> u16 {
        self.encryption_suite_version
    }

    pub fn time(&self) -> InteractionTokenEnvelopeTimeV1 {
        self.time
    }

    pub fn authenticated_data_digest(&self) -> &InteractionTokenAuthenticatedDataDigestV1 {
        &self.authenticated_data_digest
    }
}

impl Debug for EncryptedInteractionTokenV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EncryptedInteractionTokenV1")
            .field("ciphertext", &"<redacted>")
            .field("nonce", &"<redacted>")
            .field("encryption_key_id", &self.encryption_key_id)
            .field("encryption_suite", &self.encryption_suite)
            .field("encryption_suite_version", &self.encryption_suite_version)
            .field("time", &self.time)
            .field("authenticated_data_digest", &self.authenticated_data_digest)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct InteractionTokenAuthenticatedDataV1 {
    bytes: Vec<u8>,
    digest: InteractionTokenAuthenticatedDataDigestV1,
}

impl Debug for InteractionTokenAuthenticatedDataV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InteractionTokenAuthenticatedDataV1")
            .field("bytes", &"<redacted>")
            .field("digest", &self.digest)
            .finish()
    }
}

impl InteractionTokenAuthenticatedDataV1 {
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn digest(&self) -> &InteractionTokenAuthenticatedDataDigestV1 {
        &self.digest
    }
}

pub struct InteractionTokenAuthenticatedDataInputV1<'a> {
    pub claim_root: &'a InteractionReceiptClaimRootV1,
    pub encryption_key_id: &'a str,
    pub encryption_suite: &'a str,
    pub encryption_suite_version: u16,
    pub time: InteractionTokenEnvelopeTimeV1,
}

pub fn build_interaction_token_authenticated_data_v1(
    input: InteractionTokenAuthenticatedDataInputV1<'_>,
) -> Result<InteractionTokenAuthenticatedDataV1, InteractionTokenEnvelopeValidationErrorV1> {
    validate_key_id(input.encryption_key_id)
        .map_err(|_| InteractionTokenEnvelopeValidationErrorV1::InvalidKeyId)?;
    if input.encryption_suite != XCHACHA20_POLY1305_INTERACTION_TOKEN_SUITE_V1 {
        return Err(InteractionTokenEnvelopeValidationErrorV1::InvalidSuite);
    }
    if input.encryption_suite_version != XCHACHA20_POLY1305_INTERACTION_TOKEN_SUITE_VERSION_V1 {
        return Err(InteractionTokenEnvelopeValidationErrorV1::InvalidSuiteVersion);
    }
    InteractionTokenEnvelopeTimeV1::new(
        input.time.issued_at_unix_milliseconds(),
        input.time.expires_at_unix_milliseconds(),
    )
    .map_err(|_| InteractionTokenEnvelopeValidationErrorV1::InvalidTime)?;
    let mut bytes = Vec::with_capacity(2_048);
    append_field(&mut bytes, b"domain", TOKEN_AUTHENTICATED_DATA_DOMAIN_V1);
    append_claim_root_v1(&mut bytes, input.claim_root);
    append_field(
        &mut bytes,
        b"encryption_key_id",
        input.encryption_key_id.as_bytes(),
    );
    append_field(
        &mut bytes,
        b"encryption_suite",
        input.encryption_suite.as_bytes(),
    );
    append_field(
        &mut bytes,
        b"encryption_suite_version",
        &input.encryption_suite_version.to_be_bytes(),
    );
    append_field(
        &mut bytes,
        b"issued_at_unix_milliseconds",
        &input.time.issued_at_unix_milliseconds().to_be_bytes(),
    );
    append_field(
        &mut bytes,
        b"expires_at_unix_milliseconds",
        &input.time.expires_at_unix_milliseconds().to_be_bytes(),
    );
    let digest = InteractionTokenAuthenticatedDataDigestV1::from_sha256(&bytes);
    Ok(InteractionTokenAuthenticatedDataV1 { bytes, digest })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InteractionTokenEnvelopeCipherErrorV1 {
    #[error("interaction token envelope encryption key is unavailable")]
    KeyUnavailable,
    #[error("interaction token envelope authentication failed")]
    AuthenticationFailed,
    #[error("interaction token envelope is unsupported")]
    UnsupportedEnvelope,
    #[error("interaction token envelope is not valid yet")]
    NotYetValid,
    #[error("interaction token envelope has expired")]
    Expired,
    #[error("interaction token cipher backend failed")]
    Backend,
}

#[derive(Clone)]
pub struct XChaCha20Poly1305InteractionTokenCipherV1 {
    keyring: InteractionTokenEnvelopeKeyringV1,
}

impl XChaCha20Poly1305InteractionTokenCipherV1 {
    pub fn new(keyring: InteractionTokenEnvelopeKeyringV1) -> Self {
        Self { keyring }
    }

    pub fn active_encryption_key_id(&self) -> &str {
        self.keyring.active_key_id()
    }

    pub fn configured_encryption_key_ids(&self) -> Vec<&str> {
        self.keyring.configured_key_ids()
    }

    pub fn encrypt(
        &self,
        token: &InteractionTokenV1,
        claim_root: &InteractionReceiptClaimRootV1,
        time: InteractionTokenEnvelopeTimeV1,
    ) -> Result<EncryptedInteractionTokenV1, InteractionTokenEnvelopeCipherErrorV1> {
        validate_time_for_cipher(time, time.issued_at_unix_milliseconds())?;
        let active = self.keyring.active();
        let authenticated_data = build_interaction_token_authenticated_data_v1(
            InteractionTokenAuthenticatedDataInputV1 {
                claim_root,
                encryption_key_id: active.key_id(),
                encryption_suite: XCHACHA20_POLY1305_INTERACTION_TOKEN_SUITE_V1,
                encryption_suite_version: XCHACHA20_POLY1305_INTERACTION_TOKEN_SUITE_VERSION_V1,
                time,
            },
        )
        .map_err(|_| InteractionTokenEnvelopeCipherErrorV1::UnsupportedEnvelope)?;
        let mut nonce_bytes = [0_u8; XCHACHA20_POLY1305_INTERACTION_TOKEN_NONCE_BYTES_V1];
        getrandom::fill(&mut nonce_bytes)
            .map_err(|_| InteractionTokenEnvelopeCipherErrorV1::Backend)?;
        let cipher_key: &Key = active.secret().into();
        let cipher = XChaCha20Poly1305::new(cipher_key);
        let nonce = XNonce::from(nonce_bytes);
        let mut ciphertext = Zeroizing::new(token.expose_secret().as_bytes().to_vec());
        cipher
            .encrypt_in_place(&nonce, authenticated_data.as_bytes(), &mut *ciphertext)
            .map_err(|_| InteractionTokenEnvelopeCipherErrorV1::Backend)?;
        let ciphertext = std::mem::take(&mut *ciphertext);
        EncryptedInteractionTokenV1::from_persisted_parts(
            ciphertext,
            nonce_bytes.to_vec(),
            active.key_id().to_string(),
            XCHACHA20_POLY1305_INTERACTION_TOKEN_SUITE_V1.to_string(),
            XCHACHA20_POLY1305_INTERACTION_TOKEN_SUITE_VERSION_V1,
            time,
            authenticated_data.digest().clone(),
        )
        .map_err(|_| InteractionTokenEnvelopeCipherErrorV1::Backend)
    }

    pub fn decrypt(
        &self,
        envelope: &EncryptedInteractionTokenV1,
        claim_root: &InteractionReceiptClaimRootV1,
        now_unix_milliseconds: u64,
    ) -> Result<InteractionTokenV1, InteractionTokenEnvelopeCipherErrorV1> {
        if envelope.encryption_suite() != XCHACHA20_POLY1305_INTERACTION_TOKEN_SUITE_V1
            || envelope.encryption_suite_version()
                != XCHACHA20_POLY1305_INTERACTION_TOKEN_SUITE_VERSION_V1
            || envelope.nonce().len() != XCHACHA20_POLY1305_INTERACTION_TOKEN_NONCE_BYTES_V1
        {
            return Err(InteractionTokenEnvelopeCipherErrorV1::UnsupportedEnvelope);
        }
        let authenticated_data = build_interaction_token_authenticated_data_v1(
            InteractionTokenAuthenticatedDataInputV1 {
                claim_root,
                encryption_key_id: envelope.encryption_key_id(),
                encryption_suite: envelope.encryption_suite(),
                encryption_suite_version: envelope.encryption_suite_version(),
                time: envelope.time(),
            },
        )
        .map_err(|_| InteractionTokenEnvelopeCipherErrorV1::UnsupportedEnvelope)?;
        if !bool::from(
            authenticated_data
                .digest()
                .as_str()
                .as_bytes()
                .ct_eq(envelope.authenticated_data_digest().as_str().as_bytes()),
        ) {
            return Err(InteractionTokenEnvelopeCipherErrorV1::AuthenticationFailed);
        }
        let key = self
            .keyring
            .key_for_id(envelope.encryption_key_id())
            .ok_or(InteractionTokenEnvelopeCipherErrorV1::KeyUnavailable)?;
        let nonce_bytes =
            <[u8; XCHACHA20_POLY1305_INTERACTION_TOKEN_NONCE_BYTES_V1]>::try_from(envelope.nonce())
                .map_err(|_| InteractionTokenEnvelopeCipherErrorV1::UnsupportedEnvelope)?;
        let cipher_key: &Key = key.secret().into();
        let cipher = XChaCha20Poly1305::new(cipher_key);
        let nonce = XNonce::from(nonce_bytes);
        let mut plaintext = Zeroizing::new(envelope.ciphertext().to_vec());
        cipher
            .decrypt_in_place(&nonce, authenticated_data.as_bytes(), &mut *plaintext)
            .map_err(|_| InteractionTokenEnvelopeCipherErrorV1::AuthenticationFailed)?;
        validate_time_for_cipher(envelope.time(), now_unix_milliseconds)?;
        let token = match String::from_utf8(std::mem::take(&mut *plaintext)) {
            Ok(token) => token,
            Err(error) => {
                drop(Zeroizing::new(error.into_bytes()));
                return Err(InteractionTokenEnvelopeCipherErrorV1::AuthenticationFailed);
            }
        };
        InteractionTokenV1::new(token)
            .map_err(|_| InteractionTokenEnvelopeCipherErrorV1::AuthenticationFailed)
    }
}

impl Debug for XChaCha20Poly1305InteractionTokenCipherV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("XChaCha20Poly1305InteractionTokenCipherV1(<redacted>)")
    }
}

fn validate_key_id(key_id: &str) -> Result<(), InteractionTokenEnvelopeKeyErrorV1> {
    if key_id.is_empty()
        || key_id.len() > 128
        || !key_id.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':' | b'/')
        })
    {
        return Err(InteractionTokenEnvelopeKeyErrorV1::InvalidKeyId);
    }
    Ok(())
}

fn obvious_repetition(secret: &[u8; 32]) -> bool {
    [1_usize, 2, 4, 8, 16]
        .into_iter()
        .any(|period| (period..secret.len()).all(|index| secret[index] == secret[index % period]))
}

fn validate_time_for_cipher(
    time: InteractionTokenEnvelopeTimeV1,
    now_unix_milliseconds: u64,
) -> Result<(), InteractionTokenEnvelopeCipherErrorV1> {
    time.ensure_unexpired(now_unix_milliseconds)
        .map_err(|error| match error {
            InteractionTokenEnvelopeTimeErrorV1::NotYetValid => {
                InteractionTokenEnvelopeCipherErrorV1::NotYetValid
            }
            InteractionTokenEnvelopeTimeErrorV1::Expired => {
                InteractionTokenEnvelopeCipherErrorV1::Expired
            }
            InteractionTokenEnvelopeTimeErrorV1::InvalidIssuedAt
            | InteractionTokenEnvelopeTimeErrorV1::InvalidExpiry
            | InteractionTokenEnvelopeTimeErrorV1::LifetimeTooLong => {
                InteractionTokenEnvelopeCipherErrorV1::UnsupportedEnvelope
            }
        })
}

#[cfg(test)]
mod tests {
    use static_assertions::assert_not_impl_any;

    use super::*;
    use crate::test_support::{
        instance_route, static_route, static_route_with_attestation,
        static_route_with_build_revision, static_route_with_gateway_owner,
        static_route_with_gateway_shard, static_route_with_serving_lease,
    };
    use crate::{
        DiscordApplicationIdV1, DiscordInteractionIdV1, InteractionExpectedRouteV1,
        InteractionReceiptClaimCandidateV1, InteractionReceiptIdentityV1,
        InteractionRequestDigestV1, InteractionRouteBindingV1,
    };

    fn material(seed: u8) -> [u8; 32] {
        std::array::from_fn(|index| seed.wrapping_add((index as u8).wrapping_mul(17)))
    }

    fn key(key_id: &str, seed: u8) -> InteractionTokenEnvelopeKeyV1 {
        InteractionTokenEnvelopeKeyV1::new(key_id, Zeroizing::new(material(seed))).unwrap()
    }

    fn receipt() -> InteractionReceiptIdentityV1 {
        InteractionReceiptIdentityV1::new(
            DiscordApplicationIdV1::new(10).unwrap(),
            DiscordInteractionIdV1::new(20).unwrap(),
        )
    }

    fn request_digest(value: char) -> InteractionRequestDigestV1 {
        InteractionRequestDigestV1::parse(value.to_string().repeat(64)).unwrap()
    }

    fn make_claim_root(
        route: &InteractionRouteBindingV1,
        request: &InteractionRequestDigestV1,
    ) -> InteractionReceiptClaimRootV1 {
        InteractionReceiptClaimCandidateV1::new(
            receipt(),
            InteractionExpectedRouteV1::from_authoritative(route),
            request.clone(),
        )
        .bind_authoritative(route.clone())
        .unwrap()
    }

    fn time() -> InteractionTokenEnvelopeTimeV1 {
        InteractionTokenEnvelopeTimeV1::new(1_000_000, 1_060_000).unwrap()
    }

    fn cipher() -> XChaCha20Poly1305InteractionTokenCipherV1 {
        XChaCha20Poly1305InteractionTokenCipherV1::new(
            InteractionTokenEnvelopeKeyringV1::new(key("active-v2", 41), [key("retired-v1", 19)])
                .unwrap(),
        )
    }

    fn rebuild(
        envelope: &EncryptedInteractionTokenV1,
        ciphertext: Vec<u8>,
        nonce: Vec<u8>,
        key_id: String,
        aad_digest: InteractionTokenAuthenticatedDataDigestV1,
    ) -> EncryptedInteractionTokenV1 {
        EncryptedInteractionTokenV1::from_persisted_parts(
            ciphertext,
            nonce,
            key_id,
            envelope.encryption_suite().to_string(),
            envelope.encryption_suite_version(),
            envelope.time(),
            aad_digest,
        )
        .unwrap()
    }

    #[test]
    fn active_key_roundtrip_uses_fresh_nonces_and_redacts_secrets() {
        let route = static_route(1);
        let request = request_digest('a');
        let claim_root = make_claim_root(&route, &request);
        let token = InteractionTokenV1::new("discord-interaction-token").unwrap();
        let cipher = cipher();
        let first = cipher.encrypt(&token, &claim_root, time()).unwrap();
        let second = cipher.encrypt(&token, &claim_root, time()).unwrap();
        assert_ne!(first.nonce(), second.nonce());
        assert_ne!(first.ciphertext(), second.ciphertext());
        assert_eq!(first.encryption_key_id(), "active-v2");
        assert_eq!(
            cipher
                .decrypt(&first, &claim_root, 1_030_000)
                .unwrap()
                .expose_secret(),
            "discord-interaction-token"
        );
        assert_eq!(format!("{token:?}"), "InteractionTokenV1(<redacted>)");
        assert!(!format!("{first:?}").contains("discord-interaction-token"));
        assert_eq!(
            format!("{:?}", key("debug", 73)),
            "InteractionTokenEnvelopeKeyV1(<redacted>)"
        );
    }

    #[test]
    fn invalid_token_errors_never_render_rejected_secret_input() {
        let secret = format!("sensitive{}", "x".repeat(MAX_INTERACTION_TOKEN_BYTES));
        let error = InteractionTokenV1::new(secret.clone()).unwrap_err();
        assert_eq!(error, InteractionTokenErrorV1::TooLarge);
        assert!(!format!("{error:?}").contains(&secret));
        assert!(!error.to_string().contains(&secret));
    }

    #[test]
    fn retired_key_decrypts_after_rotation() {
        let old_cipher = XChaCha20Poly1305InteractionTokenCipherV1::new(
            InteractionTokenEnvelopeKeyringV1::new(key("retired-v1", 19), []).unwrap(),
        );
        let route = static_route(1);
        let request = request_digest('a');
        let claim_root = make_claim_root(&route, &request);
        let token = InteractionTokenV1::new("retired-token").unwrap();
        let envelope = old_cipher.encrypt(&token, &claim_root, time()).unwrap();
        assert_eq!(
            cipher()
                .decrypt(&envelope, &claim_root, 1_030_000)
                .unwrap()
                .expose_secret(),
            "retired-token"
        );
    }

    #[test]
    fn ciphertext_nonce_key_and_aad_tampering_fail_closed() {
        let route = static_route(1);
        let request = request_digest('a');
        let claim_root = make_claim_root(&route, &request);
        let token = InteractionTokenV1::new("bound-token").unwrap();
        let cipher = cipher();
        let envelope = cipher.encrypt(&token, &claim_root, time()).unwrap();

        let mut ciphertext = envelope.ciphertext().to_vec();
        ciphertext[0] ^= 1;
        let tampered_ciphertext = rebuild(
            &envelope,
            ciphertext,
            envelope.nonce().to_vec(),
            envelope.encryption_key_id().to_string(),
            envelope.authenticated_data_digest().clone(),
        );
        assert!(matches!(
            cipher.decrypt(&tampered_ciphertext, &claim_root, 1_030_000),
            Err(InteractionTokenEnvelopeCipherErrorV1::AuthenticationFailed)
        ));

        let mut nonce = envelope.nonce().to_vec();
        nonce[0] ^= 1;
        let tampered_nonce = rebuild(
            &envelope,
            envelope.ciphertext().to_vec(),
            nonce,
            envelope.encryption_key_id().to_string(),
            envelope.authenticated_data_digest().clone(),
        );
        assert!(matches!(
            cipher.decrypt(&tampered_nonce, &claim_root, 1_030_000),
            Err(InteractionTokenEnvelopeCipherErrorV1::AuthenticationFailed)
        ));

        let wrong_key = rebuild(
            &envelope,
            envelope.ciphertext().to_vec(),
            envelope.nonce().to_vec(),
            "retired-v1".to_string(),
            envelope.authenticated_data_digest().clone(),
        );
        assert!(matches!(
            cipher.decrypt(&wrong_key, &claim_root, 1_030_000),
            Err(InteractionTokenEnvelopeCipherErrorV1::AuthenticationFailed)
        ));

        let changed_route = static_route(2);
        let changed_route_root = make_claim_root(&changed_route, &request);
        assert!(matches!(
            cipher.decrypt(&envelope, &changed_route_root, 1_030_000),
            Err(InteractionTokenEnvelopeCipherErrorV1::AuthenticationFailed)
        ));
        let changed_shard = static_route_with_gateway_shard(1, 2);
        let changed_shard_root = make_claim_root(&changed_shard, &request);
        assert!(matches!(
            cipher.decrypt(&envelope, &changed_shard_root, 1_030_000),
            Err(InteractionTokenEnvelopeCipherErrorV1::AuthenticationFailed)
        ));
        for changed_authority in [
            static_route_with_serving_lease(1, 2),
            static_route_with_gateway_owner(1, 2),
            static_route_with_build_revision(1, 2),
            static_route_with_attestation(1, 'c'),
            instance_route(1),
        ] {
            let changed_authority_root = make_claim_root(&changed_authority, &request);
            assert!(matches!(
                cipher.decrypt(&envelope, &changed_authority_root, 1_030_000),
                Err(InteractionTokenEnvelopeCipherErrorV1::AuthenticationFailed)
            ));
        }
        let changed_request = request_digest('b');
        let changed_request_root = make_claim_root(&route, &changed_request);
        assert!(matches!(
            cipher.decrypt(&envelope, &changed_request_root, 1_030_000),
            Err(InteractionTokenEnvelopeCipherErrorV1::AuthenticationFailed)
        ));

        let changed_digest =
            InteractionTokenAuthenticatedDataDigestV1::parse("b".repeat(64)).unwrap();
        let tampered_aad_digest = rebuild(
            &envelope,
            envelope.ciphertext().to_vec(),
            envelope.nonce().to_vec(),
            envelope.encryption_key_id().to_string(),
            changed_digest,
        );
        assert!(matches!(
            cipher.decrypt(&tampered_aad_digest, &claim_root, 1_030_000),
            Err(InteractionTokenEnvelopeCipherErrorV1::AuthenticationFailed)
        ));
    }

    #[test]
    fn expiry_and_header_validation_fail_closed() {
        let route = static_route(1);
        let request = request_digest('a');
        let claim_root = make_claim_root(&route, &request);
        let token = InteractionTokenV1::new("expiring-token").unwrap();
        let cipher = cipher();
        let envelope = cipher.encrypt(&token, &claim_root, time()).unwrap();
        assert!(matches!(
            cipher.decrypt(&envelope, &claim_root, 1_060_000),
            Err(InteractionTokenEnvelopeCipherErrorV1::Expired)
        ));
        assert!(matches!(
            cipher.decrypt(&envelope, &claim_root, 999_999),
            Err(InteractionTokenEnvelopeCipherErrorV1::NotYetValid)
        ));
        let changed_time = InteractionTokenEnvelopeTimeV1::new(1_000_000, 1_070_000).unwrap();
        let tampered_time = EncryptedInteractionTokenV1::from_persisted_parts(
            envelope.ciphertext().to_vec(),
            envelope.nonce().to_vec(),
            envelope.encryption_key_id().to_string(),
            envelope.encryption_suite().to_string(),
            envelope.encryption_suite_version(),
            changed_time,
            envelope.authenticated_data_digest().clone(),
        )
        .unwrap();
        assert!(matches!(
            cipher.decrypt(&tampered_time, &claim_root, 1_030_000),
            Err(InteractionTokenEnvelopeCipherErrorV1::AuthenticationFailed)
        ));
        let shortened_time = InteractionTokenEnvelopeTimeV1::new(1_000_000, 1_020_000).unwrap();
        let shortened_expiry = EncryptedInteractionTokenV1::from_persisted_parts(
            envelope.ciphertext().to_vec(),
            envelope.nonce().to_vec(),
            envelope.encryption_key_id().to_string(),
            envelope.encryption_suite().to_string(),
            envelope.encryption_suite_version(),
            shortened_time,
            envelope.authenticated_data_digest().clone(),
        )
        .unwrap();
        assert!(matches!(
            cipher.decrypt(&shortened_expiry, &claim_root, 1_030_000),
            Err(InteractionTokenEnvelopeCipherErrorV1::AuthenticationFailed)
        ));
        assert!(matches!(
            EncryptedInteractionTokenV1::from_persisted_parts(
                envelope.ciphertext().to_vec(),
                envelope.nonce().to_vec(),
                envelope.encryption_key_id().to_string(),
                "unsupported".to_string(),
                envelope.encryption_suite_version(),
                envelope.time(),
                envelope.authenticated_data_digest().clone(),
            ),
            Err(InteractionTokenEnvelopeValidationErrorV1::InvalidSuite)
        ));
    }

    #[test]
    fn keyring_rejects_duplicate_identity_and_material() {
        assert!(matches!(
            InteractionTokenEnvelopeKeyV1::new("weak", Zeroizing::new([7; 32])),
            Err(InteractionTokenEnvelopeKeyErrorV1::ObviouslyRepetitiveKeyMaterial)
        ));
        assert!(matches!(
            InteractionTokenEnvelopeKeyringV1::new(key("same", 1), [key("same", 2)]),
            Err(InteractionTokenEnvelopeKeyringErrorV1::DuplicateKeyId)
        ));
        assert!(matches!(
            InteractionTokenEnvelopeKeyringV1::new(key("first", 1), [key("second", 1)]),
            Err(InteractionTokenEnvelopeKeyringErrorV1::AliasedKeyMaterial)
        ));
    }

    assert_not_impl_any!(InteractionTokenV1: Clone, serde::Serialize);
    assert_not_impl_any!(EncryptedInteractionTokenV1: Clone, serde::Serialize);
}
