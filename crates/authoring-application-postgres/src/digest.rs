use std::fmt::{Debug, Formatter};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const OPAQUE_SESSION_SECRET_BYTES: usize = 32;
const OPAQUE_SESSION_CREDENTIAL_BYTES: usize = 43;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum OpaqueSessionCredentialError {
    #[error("opaque product session credential is malformed")]
    Malformed,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProductSessionDigestV1([u8; 32]);

impl ProductSessionDigestV1 {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn into_bytes(self) -> [u8; 32] {
        self.0
    }
}

impl Debug for ProductSessionDigestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductSessionDigestV1(<redacted>)")
    }
}

pub fn digest_opaque_session_credential_v1(
    credential: &str,
) -> Result<ProductSessionDigestV1, OpaqueSessionCredentialError> {
    if credential.len() != OPAQUE_SESSION_CREDENTIAL_BYTES
        || !credential
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(OpaqueSessionCredentialError::Malformed);
    }
    let decoded = URL_SAFE_NO_PAD
        .decode(credential)
        .map_err(|_| OpaqueSessionCredentialError::Malformed)?;
    if decoded.len() != OPAQUE_SESSION_SECRET_BYTES {
        return Err(OpaqueSessionCredentialError::Malformed);
    }
    let decoded = Zeroizing::new(decoded);
    let digest: [u8; 32] = Sha256::digest(decoded.as_slice()).into();
    Ok(ProductSessionDigestV1(digest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_digest_decodes_the_canonical_secret() {
        let credential = URL_SAFE_NO_PAD.encode([7_u8; 32]);
        let digest = digest_opaque_session_credential_v1(&credential).unwrap();
        assert_eq!(
            digest.as_bytes(),
            &<[u8; 32]>::from(Sha256::digest([7_u8; 32]))
        );
        assert_eq!(format!("{digest:?}"), "ProductSessionDigestV1(<redacted>)");
    }

    #[test]
    fn credential_digest_rejects_noncanonical_and_unbounded_input() {
        for credential in [
            "",
            "A",
            &"A".repeat(42),
            &"A".repeat(44),
            "///////////////////////////////////////////",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        ] {
            assert_eq!(
                digest_opaque_session_credential_v1(credential),
                Err(OpaqueSessionCredentialError::Malformed)
            );
        }
    }
}
