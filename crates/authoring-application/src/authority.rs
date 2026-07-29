use std::num::NonZeroU64;
use std::time::SystemTime;

use authoring_promotion::{AutomationInstallationId, TenantId};
use discord_model::{GuildId, UserId};

use crate::AuthenticatedActorV1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallationSelectorV1 {
    installation_id: AutomationInstallationId,
}

impl InstallationSelectorV1 {
    pub fn new(installation_id: AutomationInstallationId) -> Self {
        Self { installation_id }
    }

    pub fn installation_id(&self) -> &AutomationInstallationId {
        &self.installation_id
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityV1 {
    Promote,
    Read,
    Approve,
    Reject,
    Apply,
    CancelLifecycle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizedInstallationScopeV1 {
    tenant_id: TenantId,
    installation_id: AutomationInstallationId,
    guild_id: GuildId,
    acting_user_id: UserId,
}

impl AuthorizedInstallationScopeV1 {
    pub fn from_fresh_authority(
        tenant_id: TenantId,
        installation_id: AutomationInstallationId,
        guild_id: GuildId,
        acting_user_id: UserId,
    ) -> Self {
        Self {
            tenant_id,
            installation_id,
            guild_id,
            acting_user_id,
        }
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn installation_id(&self) -> &AutomationInstallationId {
        &self.installation_id
    }

    pub fn guild_id(&self) -> GuildId {
        self.guild_id
    }

    pub fn acting_user_id(&self) -> UserId {
        self.acting_user_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizedInstallationV1<E> {
    scope: AuthorizedInstallationScopeV1,
    evidence: E,
}

impl<E> AuthorizedInstallationV1<E> {
    pub fn from_fresh_authority(scope: AuthorizedInstallationScopeV1, evidence: E) -> Self {
        Self { scope, evidence }
    }

    pub fn scope(&self) -> &AuthorizedInstallationScopeV1 {
        &self.scope
    }

    pub fn evidence(&self) -> &E {
        &self.evidence
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FreshGuildAuthorityError {
    #[error("automation installation was not found")]
    InstallationNotFound,
    #[error("authenticated principal is not authorized for this capability")]
    Forbidden,
    #[error("guild authority could not be confirmed while it was fresh")]
    Stale,
    #[error("guild authority returned a mismatched installation scope")]
    ScopeMismatch,
    #[error("guild authority backend failed: {0}")]
    Backend(String),
}

pub(crate) fn validate_authorized_scope(
    installation: &InstallationSelectorV1,
    scope: &AuthorizedInstallationScopeV1,
) -> Result<(), FreshGuildAuthorityError> {
    if installation.installation_id() != scope.installation_id() {
        return Err(FreshGuildAuthorityError::ScopeMismatch);
    }
    Ok(())
}

#[allow(async_fn_in_trait)]
pub trait FreshGuildAuthorityPort {
    type Evidence;

    async fn authorize_installation(
        &self,
        actor: &AuthenticatedActorV1,
        installation: &InstallationSelectorV1,
        capability: CapabilityV1,
    ) -> Result<AuthorizedInstallationV1<Self::Evidence>, FreshGuildAuthorityError>;
}

pub trait FreshGuildAuthorityEvidence {
    fn tenant_id(&self) -> &TenantId;
    fn installation_id(&self) -> &AutomationInstallationId;
    fn discord_application_id(&self) -> NonZeroU64;
    fn guild_id(&self) -> GuildId;
    fn acting_user_id(&self) -> UserId;
    fn capability(&self) -> CapabilityV1;
    fn guild_owner(&self) -> bool;
    fn effective_permissions_bits(&self) -> u64;
    fn installation_authority_revision(&self) -> NonZeroU64;
    fn installation_authority_digest(&self) -> &str;
    fn observation_digest(&self) -> &str;
    fn observed_at(&self) -> SystemTime;
    fn expires_at(&self) -> SystemTime;
}
