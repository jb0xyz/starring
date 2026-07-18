use std::fmt::{Debug, Formatter};

use authoring_promotion::PrincipalId;

#[derive(Clone, PartialEq, Eq)]
pub struct AuthenticatedSessionFingerprintV1([u8; 32]);

impl AuthenticatedSessionFingerprintV1 {
    pub fn from_sha256_digest(digest: [u8; 32]) -> Self {
        Self(digest)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Debug for AuthenticatedSessionFingerprintV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AuthenticatedSessionFingerprintV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct AuthenticationClaimsV1 {
    principal_id: PrincipalId,
    session_fingerprint: AuthenticatedSessionFingerprintV1,
}

impl AuthenticationClaimsV1 {
    pub fn from_authentication(
        principal_id: PrincipalId,
        session_fingerprint: AuthenticatedSessionFingerprintV1,
    ) -> Self {
        Self {
            principal_id,
            session_fingerprint,
        }
    }
}

impl Debug for AuthenticationClaimsV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AuthenticationClaimsV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
struct AuthenticatedSessionV1 {
    claims: AuthenticationClaimsV1,
}

impl AuthenticatedSessionV1 {
    fn from_claims(claims: AuthenticationClaimsV1) -> Self {
        Self { claims }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct AuthenticatedActorV1 {
    session: AuthenticatedSessionV1,
}

impl Debug for AuthenticatedActorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AuthenticatedActorV1(<redacted>)")
    }
}

impl AuthenticatedActorV1 {
    pub(crate) fn from_authentication_claims(claims: AuthenticationClaimsV1) -> Self {
        Self {
            session: AuthenticatedSessionV1::from_claims(claims),
        }
    }

    pub fn principal_id(&self) -> &PrincipalId {
        &self.session.claims.principal_id
    }

    pub fn session_fingerprint(&self) -> &AuthenticatedSessionFingerprintV1 {
        &self.session.claims.session_fingerprint
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthenticationBackendFailureV1 {
    #[error("authentication backend request timed out")]
    Timeout,
    #[error("authentication backend request can be retried")]
    Retryable,
    #[error("authentication backend is unavailable")]
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthenticationError {
    #[error("authentication credential is invalid")]
    InvalidCredential,
    #[error("authentication credential has expired")]
    Expired,
    #[error("authentication credential was revoked")]
    Revoked,
    #[error(transparent)]
    Backend(#[from] AuthenticationBackendFailureV1),
}

#[allow(async_fn_in_trait)]
pub trait AuthenticationPort {
    type Credential: ?Sized;

    async fn authenticate(
        &self,
        credential: &Self::Credential,
    ) -> Result<AuthenticationClaimsV1, AuthenticationError>;
}
