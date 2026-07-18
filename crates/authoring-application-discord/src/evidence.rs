use std::fmt::{Display, Formatter};
use std::num::{NonZeroU64, NonZeroUsize};
use std::time::SystemTime;

use authoring_application::{CapabilityV1, FreshGuildAuthorityEvidence};
use authoring_promotion::{AutomationInstallationId, TenantId};
use chrono::{DateTime, Utc};
use discord_model::{GuildId, Permissions, UserId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DiscordApplicationIdV1(NonZeroU64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DiscordApplicationIdError {
    #[error("Discord application ID must be nonzero")]
    Zero,
}

impl DiscordApplicationIdV1 {
    pub fn new(value: u64) -> Result<Self, DiscordApplicationIdError> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(DiscordApplicationIdError::Zero)
    }

    pub fn get(self) -> u64 {
        self.0.get()
    }
}

impl Display for DiscordApplicationIdV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FreshDiscordAuthorityEvidenceV1 {
    tenant_id: TenantId,
    installation_id: AutomationInstallationId,
    application_id: DiscordApplicationIdV1,
    guild_id: GuildId,
    acting_user_id: UserId,
    capability: CapabilityV1,
    effective_permissions: Permissions,
    owner: bool,
    installation_authority_revision: NonZeroU64,
    installation_authority_digest: String,
    observation_digest: String,
    observed_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    contributing_role_count: NonZeroUsize,
}

pub(crate) struct FreshDiscordAuthorityEvidenceInputV1 {
    pub tenant_id: TenantId,
    pub installation_id: AutomationInstallationId,
    pub application_id: DiscordApplicationIdV1,
    pub guild_id: GuildId,
    pub acting_user_id: UserId,
    pub capability: CapabilityV1,
    pub effective_permissions: Permissions,
    pub owner: bool,
    pub installation_authority_revision: NonZeroU64,
    pub installation_authority_digest: String,
    pub observation_digest: String,
    pub observed_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub contributing_role_count: NonZeroUsize,
}

impl FreshDiscordAuthorityEvidenceV1 {
    pub(crate) fn from_validated(input: FreshDiscordAuthorityEvidenceInputV1) -> Self {
        Self {
            tenant_id: input.tenant_id,
            installation_id: input.installation_id,
            application_id: input.application_id,
            guild_id: input.guild_id,
            acting_user_id: input.acting_user_id,
            capability: input.capability,
            effective_permissions: input.effective_permissions,
            owner: input.owner,
            installation_authority_revision: input.installation_authority_revision,
            installation_authority_digest: input.installation_authority_digest,
            observation_digest: input.observation_digest,
            observed_at: input.observed_at,
            expires_at: input.expires_at,
            contributing_role_count: input.contributing_role_count,
        }
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn installation_id(&self) -> &AutomationInstallationId {
        &self.installation_id
    }

    pub fn application_id(&self) -> DiscordApplicationIdV1 {
        self.application_id
    }

    pub fn guild_id(&self) -> GuildId {
        self.guild_id
    }

    pub fn acting_user_id(&self) -> UserId {
        self.acting_user_id
    }

    pub fn capability(&self) -> CapabilityV1 {
        self.capability
    }

    pub fn effective_permissions(&self) -> Permissions {
        self.effective_permissions
    }

    pub fn owner(&self) -> bool {
        self.owner
    }

    pub fn installation_authority_revision(&self) -> NonZeroU64 {
        self.installation_authority_revision
    }

    pub fn installation_authority_digest(&self) -> &str {
        &self.installation_authority_digest
    }

    pub fn observation_digest(&self) -> &str {
        &self.observation_digest
    }

    pub fn observed_at(&self) -> DateTime<Utc> {
        self.observed_at
    }

    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }

    pub fn contributing_role_count(&self) -> NonZeroUsize {
        self.contributing_role_count
    }

    pub fn is_fresh_at(&self, now: DateTime<Utc>) -> bool {
        now >= self.observed_at && now < self.expires_at
    }
}

impl FreshGuildAuthorityEvidence for FreshDiscordAuthorityEvidenceV1 {
    fn tenant_id(&self) -> &TenantId {
        self.tenant_id()
    }

    fn installation_id(&self) -> &AutomationInstallationId {
        self.installation_id()
    }

    fn discord_application_id(&self) -> NonZeroU64 {
        NonZeroU64::new(self.application_id().get())
            .expect("validated Discord application ID must remain nonzero")
    }

    fn guild_id(&self) -> GuildId {
        self.guild_id()
    }

    fn acting_user_id(&self) -> UserId {
        self.acting_user_id()
    }

    fn capability(&self) -> CapabilityV1 {
        self.capability()
    }

    fn guild_owner(&self) -> bool {
        self.owner()
    }

    fn effective_permissions_bits(&self) -> u64 {
        self.effective_permissions().bits()
    }

    fn installation_authority_revision(&self) -> NonZeroU64 {
        self.installation_authority_revision()
    }

    fn installation_authority_digest(&self) -> &str {
        self.installation_authority_digest()
    }

    fn observation_digest(&self) -> &str {
        self.observation_digest()
    }

    fn observed_at(&self) -> SystemTime {
        self.observed_at().into()
    }

    fn expires_at(&self) -> SystemTime {
        self.expires_at().into()
    }
}
