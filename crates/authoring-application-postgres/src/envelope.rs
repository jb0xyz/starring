use std::fmt::{Debug, Formatter};

use authoring_promotion::{
    AuthoringSessionId, AutomationInstallationId, SessionGeneration, TenantId,
};
use resource_resolution::ResourceBindingFingerprint;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

mod xchacha;

pub use xchacha::{
    SnapshotEnvelopeKeyError, SnapshotEnvelopeKeyV1, SnapshotEnvelopeKeyringError,
    SnapshotEnvelopeKeyringV1, XChaCha20Poly1305SnapshotEnvelopeCipherV1,
    XCHACHA20_POLY1305_SNAPSHOT_NONCE_BYTES_V1, XCHACHA20_POLY1305_SNAPSHOT_SUITE_V1,
    XCHACHA20_POLY1305_SNAPSHOT_SUITE_VERSION_V1,
};

const AUTHENTICATED_DATA_DOMAIN_V1: &[u8] = b"starring.authoring.snapshot_envelope.v1\0";
const MIN_CIPHERTEXT_BYTES: usize = 16;
const MAX_CIPHERTEXT_BYTES: usize = 8 * 1024 * 1024;
const MIN_NONCE_BYTES: usize = 12;
const MAX_NONCE_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SnapshotAuthenticatedDataError {
    #[error("snapshot encryption key identifier is invalid")]
    InvalidKeyId,
    #[error("snapshot encryption suite is invalid")]
    InvalidSuite,
    #[error("snapshot encryption suite version is invalid")]
    InvalidSuiteVersion,
    #[error("snapshot schema version is invalid")]
    InvalidSchemaVersion,
    #[error("snapshot ciphertext length is invalid")]
    InvalidCiphertext,
    #[error("snapshot nonce length is invalid")]
    InvalidNonce,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SnapshotAuthenticatedDataV1 {
    bytes: Vec<u8>,
    digest_hex: String,
}

impl SnapshotAuthenticatedDataV1 {
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn digest_hex(&self) -> &str {
        &self.digest_hex
    }
}

impl Debug for SnapshotAuthenticatedDataV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SnapshotAuthenticatedDataV1")
            .field("digest_hex", &self.digest_hex)
            .field("bytes", &"<redacted>")
            .finish()
    }
}

pub struct SnapshotAuthenticatedDataInputV1<'a> {
    pub tenant_id: &'a TenantId,
    pub installation_id: &'a AutomationInstallationId,
    pub session_id: &'a AuthoringSessionId,
    pub generation: SessionGeneration,
    pub snapshot_schema_version: u32,
    pub binding_fingerprint: &'a ResourceBindingFingerprint,
    pub encryption_key_id: &'a str,
    pub encryption_suite: &'a str,
    pub encryption_suite_version: u16,
}

pub fn build_snapshot_authenticated_data_v1(
    input: SnapshotAuthenticatedDataInputV1<'_>,
) -> Result<SnapshotAuthenticatedDataV1, SnapshotAuthenticatedDataError> {
    validate_header(
        input.encryption_key_id,
        input.encryption_suite,
        input.encryption_suite_version,
    )?;
    if input.snapshot_schema_version == 0 {
        return Err(SnapshotAuthenticatedDataError::InvalidSchemaVersion);
    }
    let mut bytes = Vec::with_capacity(512);
    append_frame(&mut bytes, AUTHENTICATED_DATA_DOMAIN_V1);
    append_field(
        &mut bytes,
        b"tenant_id",
        input.tenant_id.as_str().as_bytes(),
    );
    append_field(
        &mut bytes,
        b"installation_id",
        input.installation_id.as_str().as_bytes(),
    );
    append_field(
        &mut bytes,
        b"session_id",
        input.session_id.as_str().as_bytes(),
    );
    append_field(
        &mut bytes,
        b"generation",
        &input.generation.get().to_be_bytes(),
    );
    append_field(
        &mut bytes,
        b"snapshot_schema_version",
        &input.snapshot_schema_version.to_be_bytes(),
    );
    append_field(
        &mut bytes,
        b"binding_fingerprint",
        input.binding_fingerprint.as_str().as_bytes(),
    );
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
    let digest_hex = lower_hex(Sha256::digest(&bytes).as_slice());
    Ok(SnapshotAuthenticatedDataV1 { bytes, digest_hex })
}

#[derive(Clone, PartialEq, Eq)]
pub struct EncryptedSnapshotEnvelopeV1 {
    ciphertext: Vec<u8>,
    nonce: Vec<u8>,
    encryption_key_id: String,
    encryption_suite: String,
    encryption_suite_version: u16,
}

impl EncryptedSnapshotEnvelopeV1 {
    pub fn from_persisted_parts(
        ciphertext: Vec<u8>,
        nonce: Vec<u8>,
        encryption_key_id: String,
        encryption_suite: String,
        encryption_suite_version: u16,
    ) -> Result<Self, SnapshotAuthenticatedDataError> {
        validate_header(
            &encryption_key_id,
            &encryption_suite,
            encryption_suite_version,
        )?;
        if !(MIN_CIPHERTEXT_BYTES..=MAX_CIPHERTEXT_BYTES).contains(&ciphertext.len()) {
            return Err(SnapshotAuthenticatedDataError::InvalidCiphertext);
        }
        if !(MIN_NONCE_BYTES..=MAX_NONCE_BYTES).contains(&nonce.len()) {
            return Err(SnapshotAuthenticatedDataError::InvalidNonce);
        }
        Ok(Self {
            ciphertext,
            nonce,
            encryption_key_id,
            encryption_suite,
            encryption_suite_version,
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
}

impl Debug for EncryptedSnapshotEnvelopeV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EncryptedSnapshotEnvelopeV1")
            .field("ciphertext", &"<redacted>")
            .field("nonce", &"<redacted>")
            .field("encryption_key_id", &self.encryption_key_id)
            .field("encryption_suite", &self.encryption_suite)
            .field("encryption_suite_version", &self.encryption_suite_version)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SnapshotEnvelopeCipherError {
    #[error("snapshot encryption key is unavailable")]
    KeyUnavailable,
    #[error("snapshot envelope authentication failed")]
    AuthenticationFailed,
    #[error("snapshot envelope is unsupported")]
    UnsupportedEnvelope,
    #[error("snapshot cipher backend failed")]
    Backend,
}

#[allow(async_fn_in_trait)]
pub trait SnapshotEnvelopeCipher {
    fn configured_encryption_key_ids(&self) -> Option<Vec<&str>> {
        None
    }

    async fn decrypt(
        &self,
        envelope: &EncryptedSnapshotEnvelopeV1,
        authenticated_data: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, SnapshotEnvelopeCipherError>;
}

fn validate_header(
    encryption_key_id: &str,
    encryption_suite: &str,
    encryption_suite_version: u16,
) -> Result<(), SnapshotAuthenticatedDataError> {
    validate_key_id(encryption_key_id)?;
    if encryption_suite.is_empty()
        || encryption_suite.len() > 64
        || !encryption_suite
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
        || !encryption_suite
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(SnapshotAuthenticatedDataError::InvalidSuite);
    }
    if encryption_suite_version == 0 || encryption_suite_version > i16::MAX as u16 {
        return Err(SnapshotAuthenticatedDataError::InvalidSuiteVersion);
    }
    Ok(())
}

fn validate_key_id(encryption_key_id: &str) -> Result<(), SnapshotAuthenticatedDataError> {
    if encryption_key_id.is_empty()
        || encryption_key_id.len() > 128
        || !encryption_key_id.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'/' | b'-')
        })
    {
        return Err(SnapshotAuthenticatedDataError::InvalidKeyId);
    }
    Ok(())
}

fn append_field(output: &mut Vec<u8>, name: &[u8], value: &[u8]) {
    append_frame(output, name);
    append_frame(output, value);
}

fn append_frame(output: &mut Vec<u8>, value: &[u8]) {
    let length = u64::try_from(value.len()).expect("snapshot metadata field exceeds u64::MAX");
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authenticated_data() -> SnapshotAuthenticatedDataV1 {
        let tenant_id = TenantId::parse("tenant-1").unwrap();
        let installation_id = AutomationInstallationId::parse("installation-1").unwrap();
        let session_id = AuthoringSessionId::parse("session-1").unwrap();
        let binding_fingerprint = ResourceBindingFingerprint::parse(&"a".repeat(64)).unwrap();
        build_snapshot_authenticated_data_v1(SnapshotAuthenticatedDataInputV1 {
            tenant_id: &tenant_id,
            installation_id: &installation_id,
            session_id: &session_id,
            generation: SessionGeneration::new(1).unwrap(),
            snapshot_schema_version: 8,
            binding_fingerprint: &binding_fingerprint,
            encryption_key_id: "keychain:authoring-v1",
            encryption_suite: "xchacha20_poly1305",
            encryption_suite_version: 1,
        })
        .unwrap()
    }

    #[test]
    fn authenticated_data_is_deterministic_and_domain_bound() {
        let first = authenticated_data();
        let second = authenticated_data();
        assert_eq!(first, second);
        assert_eq!(first.digest_hex().len(), 64);
        assert!(first
            .digest_hex()
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
        assert!(format!("{first:?}").contains("<redacted>"));
    }

    #[test]
    fn authenticated_data_changes_for_every_authority_field() {
        #[derive(Clone, Copy)]
        struct Input<'a> {
            tenant: &'a str,
            installation: &'a str,
            session: &'a str,
            generation: u64,
            schema_version: u32,
            fingerprint: char,
            key_id: &'a str,
            suite: &'a str,
            suite_version: u16,
        }

        impl Input<'_> {
            fn digest(self) -> String {
                let tenant_id = TenantId::parse(self.tenant).unwrap();
                let installation_id = AutomationInstallationId::parse(self.installation).unwrap();
                let session_id = AuthoringSessionId::parse(self.session).unwrap();
                let binding_fingerprint =
                    ResourceBindingFingerprint::parse(&self.fingerprint.to_string().repeat(64))
                        .unwrap();
                build_snapshot_authenticated_data_v1(SnapshotAuthenticatedDataInputV1 {
                    tenant_id: &tenant_id,
                    installation_id: &installation_id,
                    session_id: &session_id,
                    generation: SessionGeneration::new(self.generation).unwrap(),
                    snapshot_schema_version: self.schema_version,
                    binding_fingerprint: &binding_fingerprint,
                    encryption_key_id: self.key_id,
                    encryption_suite: self.suite,
                    encryption_suite_version: self.suite_version,
                })
                .unwrap()
                .digest_hex()
                .to_string()
            }
        }

        let baseline = Input {
            tenant: "tenant-1",
            installation: "installation-1",
            session: "session-1",
            generation: 1,
            schema_version: 8,
            fingerprint: 'a',
            key_id: "keychain:authoring-v1",
            suite: "xchacha20_poly1305",
            suite_version: 1,
        };
        let base = baseline.digest();
        let changed = [
            Input {
                tenant: "tenant-2",
                ..baseline
            },
            Input {
                installation: "installation-2",
                ..baseline
            },
            Input {
                session: "session-2",
                ..baseline
            },
            Input {
                generation: 2,
                ..baseline
            },
            Input {
                schema_version: 9,
                ..baseline
            },
            Input {
                fingerprint: 'b',
                ..baseline
            },
            Input {
                key_id: "keychain:authoring-v2",
                ..baseline
            },
            Input {
                suite: "xchacha20_poly1305_alt",
                ..baseline
            },
            Input {
                suite_version: 2,
                ..baseline
            },
        ]
        .map(Input::digest);
        assert!(changed.iter().all(|digest| digest != &base));
    }

    #[test]
    fn envelope_rejects_invalid_header_and_bounds() {
        assert!(EncryptedSnapshotEnvelopeV1::from_persisted_parts(
            vec![0; 15],
            vec![0; 24],
            "key-1".to_string(),
            "xchacha20_poly1305".to_string(),
            1,
        )
        .is_err());
        assert!(EncryptedSnapshotEnvelopeV1::from_persisted_parts(
            vec![0; 16],
            vec![0; 11],
            "key-1".to_string(),
            "xchacha20_poly1305".to_string(),
            1,
        )
        .is_err());
        assert_eq!(
            build_snapshot_authenticated_data_v1(SnapshotAuthenticatedDataInputV1 {
                tenant_id: &TenantId::parse("tenant-1").unwrap(),
                installation_id: &AutomationInstallationId::parse("installation-1").unwrap(),
                session_id: &AuthoringSessionId::parse("session-1").unwrap(),
                generation: SessionGeneration::new(1).unwrap(),
                snapshot_schema_version: 8,
                binding_fingerprint: &ResourceBindingFingerprint::parse(&"a".repeat(64)).unwrap(),
                encryption_key_id: "bad key",
                encryption_suite: "xchacha20_poly1305",
                encryption_suite_version: 1,
            }),
            Err(SnapshotAuthenticatedDataError::InvalidKeyId)
        );
    }
}
