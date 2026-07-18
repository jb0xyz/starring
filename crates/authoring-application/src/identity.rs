use authoring_promotion::PrincipalId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthenticatedIdentityV1 {
    principal_id: PrincipalId,
}

impl AuthenticatedIdentityV1 {
    pub fn from_authentication(principal_id: PrincipalId) -> Self {
        Self { principal_id }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthenticatedActorV1 {
    identity: AuthenticatedIdentityV1,
}

impl AuthenticatedActorV1 {
    pub(crate) fn from_identity(identity: AuthenticatedIdentityV1) -> Self {
        Self { identity }
    }

    pub fn principal_id(&self) -> &PrincipalId {
        &self.identity.principal_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthenticationError {
    #[error("authentication credential is invalid")]
    InvalidCredential,
    #[error("authentication credential has expired")]
    Expired,
    #[error("authentication credential was revoked")]
    Revoked,
    #[error("authentication backend failed: {0}")]
    Backend(String),
}

#[allow(async_fn_in_trait)]
pub trait AuthenticationPort {
    type Credential: ?Sized;

    async fn authenticate(
        &self,
        credential: &Self::Credential,
    ) -> Result<AuthenticatedIdentityV1, AuthenticationError>;
}
