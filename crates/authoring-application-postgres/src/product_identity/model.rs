use std::fmt::{Debug, Formatter};

use authoring_promotion::PrincipalId;
use chrono::{DateTime, Utc};
use discord_model::UserId;

use crate::{
    ProductDatabaseFailureV1, ProductSecretGeneratorError, ProductSecretV1, ProductSessionDigestV1,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum OAuthFlowError {
    #[error("OAuth flow request is invalid")]
    InvalidRequest,
    #[error("OAuth flow is invalid, expired, or already consumed")]
    InvalidOrConsumed,
    #[error("OAuth flow secret generation failed")]
    SecretGeneration,
    #[error(transparent)]
    Database(#[from] ProductDatabaseFailureV1),
    #[error("OAuth flow persistence invariant failed")]
    Invariant,
    #[error("OAuth flow commit outcome is indeterminate")]
    CommitIndeterminate,
}

impl From<ProductSecretGeneratorError> for OAuthFlowError {
    fn from(_value: ProductSecretGeneratorError) -> Self {
        Self::SecretGeneration
    }
}

pub struct OAuthFlowIssueV1 {
    pub(crate) state: ProductSecretV1,
    pub(crate) browser_nonce: ProductSecretV1,
    pub(crate) redirect_uri: String,
    pub(crate) return_path: String,
    pub(crate) expires_at: DateTime<Utc>,
    pub(crate) max_age_seconds: u32,
}

impl OAuthFlowIssueV1 {
    pub fn state(&self) -> &ProductSecretV1 {
        &self.state
    }

    pub fn browser_nonce(&self) -> &ProductSecretV1 {
        &self.browser_nonce
    }

    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    pub fn return_path(&self) -> &str {
        &self.return_path
    }

    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }

    pub fn max_age_seconds(&self) -> u32 {
        self.max_age_seconds
    }
}

impl Debug for OAuthFlowIssueV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OAuthFlowIssueV1")
            .field("state", &self.state)
            .field("browser_nonce", &self.browser_nonce)
            .field("redirect_uri", &self.redirect_uri)
            .field("return_path", &self.return_path)
            .field("expires_at", &self.expires_at)
            .field("max_age_seconds", &self.max_age_seconds)
            .finish()
    }
}

pub struct ConsumedOAuthFlowV1 {
    pub(crate) state_digest: ProductSessionDigestV1,
    pub(crate) redirect_uri: String,
    pub(crate) return_path: String,
    pub(crate) consumed_at: DateTime<Utc>,
}

impl ConsumedOAuthFlowV1 {
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    pub fn return_path(&self) -> &str {
        &self.return_path
    }

    pub fn consumed_at(&self) -> DateTime<Utc> {
        self.consumed_at
    }
}

impl Debug for ConsumedOAuthFlowV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConsumedOAuthFlowV1")
            .field("state_digest", &self.state_digest)
            .field("redirect_uri", &self.redirect_uri)
            .field("return_path", &self.return_path)
            .field("consumed_at", &self.consumed_at)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductIdentityError {
    #[error("OAuth flow is invalid, expired, or already issued")]
    FlowInvalidOrConsumed,
    #[error("product session credential is invalid")]
    InvalidCredential,
    #[error("product session CSRF proof is invalid")]
    InvalidCsrf,
    #[error("product session has expired")]
    Expired,
    #[error("product session was revoked")]
    Revoked,
    #[error("product principal is disabled")]
    PrincipalDisabled,
    #[error("product session secret generation failed")]
    SecretGeneration,
    #[error(transparent)]
    Database(#[from] ProductDatabaseFailureV1),
    #[error("product identity persistence invariant failed")]
    Invariant,
    #[error("product identity commit outcome is indeterminate")]
    CommitIndeterminate,
}

impl From<ProductSecretGeneratorError> for ProductIdentityError {
    fn from(_value: ProductSecretGeneratorError) -> Self {
        Self::SecretGeneration
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct CurrentProductPrincipalV1 {
    principal_id: PrincipalId,
    session_fingerprint: ProductSessionDigestV1,
    discord_user_id: UserId,
    display_name: String,
    identity_revision: u64,
    absolute_expires_at: DateTime<Utc>,
}

impl CurrentProductPrincipalV1 {
    pub(crate) fn from_authenticated_session(
        principal_id: PrincipalId,
        session_fingerprint: ProductSessionDigestV1,
        discord_user_id: UserId,
        display_name: String,
        identity_revision: u64,
        absolute_expires_at: DateTime<Utc>,
    ) -> Self {
        Self {
            principal_id,
            session_fingerprint,
            discord_user_id,
            display_name,
            identity_revision,
            absolute_expires_at,
        }
    }

    pub fn principal_id(&self) -> &PrincipalId {
        &self.principal_id
    }

    pub fn session_fingerprint(&self) -> &ProductSessionDigestV1 {
        &self.session_fingerprint
    }

    pub fn discord_user_id(&self) -> UserId {
        self.discord_user_id
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn identity_revision(&self) -> u64 {
        self.identity_revision
    }

    pub fn absolute_expires_at(&self) -> DateTime<Utc> {
        self.absolute_expires_at
    }
}

impl Debug for CurrentProductPrincipalV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CurrentProductPrincipalV1")
            .field("principal_id", &"<redacted>")
            .field("session_fingerprint", &self.session_fingerprint)
            .field("discord_user_id", &"<redacted>")
            .field("display_name", &"<redacted>")
            .field("identity_revision", &self.identity_revision)
            .field("absolute_expires_at", &self.absolute_expires_at)
            .finish()
    }
}

pub struct IssuedProductSessionV1 {
    pub(crate) principal: CurrentProductPrincipalV1,
    pub(crate) session: ProductSecretV1,
    pub(crate) csrf: ProductSecretV1,
    pub(crate) return_path: String,
    pub(crate) max_age_seconds: u32,
}

impl IssuedProductSessionV1 {
    pub fn principal(&self) -> &CurrentProductPrincipalV1 {
        &self.principal
    }

    pub fn session(&self) -> &ProductSecretV1 {
        &self.session
    }

    pub fn csrf(&self) -> &ProductSecretV1 {
        &self.csrf
    }

    pub fn return_path(&self) -> &str {
        &self.return_path
    }

    pub fn max_age_seconds(&self) -> u32 {
        self.max_age_seconds
    }
}

impl Debug for IssuedProductSessionV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IssuedProductSessionV1")
            .field("principal", &self.principal)
            .field("session", &self.session)
            .field("csrf", &self.csrf)
            .field("return_path", &self.return_path)
            .field("max_age_seconds", &self.max_age_seconds)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProductSessionRevocationReasonV1 {
    UserLogout,
    SecurityRevocation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProductLogoutDispositionV1 {
    Revoked,
    ExactReplay,
}

impl ProductSessionRevocationReasonV1 {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::UserLogout => "user_logout",
            Self::SecurityRevocation => "security_revocation",
        }
    }
}
