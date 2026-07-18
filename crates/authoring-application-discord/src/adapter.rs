use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;
use std::time::Duration;

use authoring_application::{
    AuthenticatedActorV1, AuthorizedInstallationScopeV1, AuthorizedInstallationV1, CapabilityV1,
    FreshGuildAuthorityError, FreshGuildAuthorityPort, InstallationSelectorV1,
};
use chrono::{DateTime, TimeDelta, Utc};
use discord_model::{Permissions, RoleId};
use sha2::{Digest, Sha256};

use crate::evidence::FreshDiscordAuthorityEvidenceInputV1;
use crate::{
    DiscordAuthorityClientError, DiscordGuildAuthorityClient, DiscordGuildAuthoritySnapshotV1,
    FreshDiscordAuthorityEvidenceV1, InstallationAuthorityRecordV1,
};

const DIGEST_LENGTH: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DiscordAuthoritySourceError {
    #[error("automation installation was not found")]
    NotFound,
    #[error("automation installation is unavailable")]
    Unavailable,
    #[error("installation authority record is invalid")]
    InvalidRecord,
}

#[allow(async_fn_in_trait)]
pub trait InstallationAuthoritySource {
    async fn load_for_actor(
        &self,
        actor: &AuthenticatedActorV1,
        installation: &InstallationSelectorV1,
    ) -> Result<InstallationAuthorityRecordV1, DiscordAuthoritySourceError>;
}

pub trait AuthorityClock: Clone + Send + Sync + 'static {
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UtcAuthorityClock;

impl AuthorityClock for UtcAuthorityClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthorityConfigError {
    #[error("Discord authority deadline must be positive and at most 30 seconds")]
    InvalidDeadline,
    #[error("write authority lifetime must be positive and at most 5 seconds")]
    InvalidWriteLifetime,
    #[error("read authority lifetime must be positive and at most 30 seconds")]
    InvalidReadLifetime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiscordAuthorityConfigV1 {
    deadline: Duration,
    write_lifetime: TimeDelta,
    read_lifetime: TimeDelta,
}

impl DiscordAuthorityConfigV1 {
    pub fn new(
        deadline: Duration,
        write_lifetime: Duration,
        read_lifetime: Duration,
    ) -> Result<Self, AuthorityConfigError> {
        if deadline.is_zero() || deadline > Duration::from_secs(30) {
            return Err(AuthorityConfigError::InvalidDeadline);
        }
        if write_lifetime.is_zero() || write_lifetime > Duration::from_secs(5) {
            return Err(AuthorityConfigError::InvalidWriteLifetime);
        }
        if read_lifetime.is_zero() || read_lifetime > Duration::from_secs(30) {
            return Err(AuthorityConfigError::InvalidReadLifetime);
        }
        Ok(Self {
            deadline,
            write_lifetime: TimeDelta::from_std(write_lifetime)
                .map_err(|_| AuthorityConfigError::InvalidWriteLifetime)?,
            read_lifetime: TimeDelta::from_std(read_lifetime)
                .map_err(|_| AuthorityConfigError::InvalidReadLifetime)?,
        })
    }

    fn evidence_lifetime(self, capability: CapabilityV1) -> TimeDelta {
        match capability {
            CapabilityV1::Read => self.read_lifetime,
            CapabilityV1::Promote
            | CapabilityV1::Approve
            | CapabilityV1::Reject
            | CapabilityV1::Apply => self.write_lifetime,
        }
    }
}

impl Default for DiscordAuthorityConfigV1 {
    fn default() -> Self {
        Self::new(
            Duration::from_secs(5),
            Duration::from_secs(5),
            Duration::from_secs(30),
        )
        .expect("default Discord authority configuration must be valid")
    }
}

pub struct DiscordGuildAuthorityAdapter<S, C, K = UtcAuthorityClock> {
    source: S,
    client: C,
    clock: K,
    config: DiscordAuthorityConfigV1,
}

impl<S, C> DiscordGuildAuthorityAdapter<S, C, UtcAuthorityClock> {
    pub fn new(source: S, client: C, config: DiscordAuthorityConfigV1) -> Self {
        Self {
            source,
            client,
            clock: UtcAuthorityClock,
            config,
        }
    }
}

impl<S, C, K> DiscordGuildAuthorityAdapter<S, C, K> {
    pub fn with_clock(source: S, client: C, clock: K, config: DiscordAuthorityConfigV1) -> Self {
        Self {
            source,
            client,
            clock,
            config,
        }
    }
}

impl<S, C, K> FreshGuildAuthorityPort for DiscordGuildAuthorityAdapter<S, C, K>
where
    S: InstallationAuthoritySource + Sync,
    C: DiscordGuildAuthorityClient + Sync,
    K: AuthorityClock,
{
    type Evidence = FreshDiscordAuthorityEvidenceV1;

    async fn authorize_installation(
        &self,
        actor: &AuthenticatedActorV1,
        installation: &InstallationSelectorV1,
        capability: CapabilityV1,
    ) -> Result<AuthorizedInstallationV1<Self::Evidence>, FreshGuildAuthorityError> {
        let record = self
            .source
            .load_for_actor(actor, installation)
            .await
            .map_err(map_source_error)?;
        validate_record(installation, &record, self.client.application_id())?;
        let snapshot = tokio::time::timeout(
            self.config.deadline,
            self.client
                .fetch_authority_snapshot(record.guild_id, record.acting_user_id),
        )
        .await
        .map_err(|_| FreshGuildAuthorityError::Stale)?
        .map_err(map_client_error)?;
        let evaluated = evaluate_snapshot(&record, snapshot)?;
        if !evaluated.owner
            && !evaluated
                .effective_permissions
                .intersects(Permissions::ADMINISTRATOR | Permissions::MANAGE_GUILD)
        {
            return Err(FreshGuildAuthorityError::Forbidden);
        }
        let observed_at = self.clock.now();
        let expires_at = observed_at
            .checked_add_signed(self.config.evidence_lifetime(capability))
            .ok_or_else(|| FreshGuildAuthorityError::Backend("authority_time_overflow".into()))?;
        let observation_digest =
            observation_digest(&record, capability, &evaluated, observed_at, expires_at);
        let scope = AuthorizedInstallationScopeV1::from_fresh_authority(
            record.tenant_id.clone(),
            record.installation_id.clone(),
            record.guild_id,
            record.acting_user_id,
        );
        let evidence =
            FreshDiscordAuthorityEvidenceV1::from_validated(FreshDiscordAuthorityEvidenceInputV1 {
                tenant_id: record.tenant_id,
                installation_id: record.installation_id,
                application_id: record.application_id,
                guild_id: record.guild_id,
                acting_user_id: record.acting_user_id,
                capability,
                effective_permissions: evaluated.effective_permissions,
                owner: evaluated.owner,
                installation_authority_revision: record.authority_revision,
                installation_authority_digest: record.authority_digest,
                observation_digest,
                observed_at,
                expires_at,
                contributing_role_count: evaluated.contributing_role_count,
            });
        Ok(AuthorizedInstallationV1::from_fresh_authority(
            scope, evidence,
        ))
    }
}

fn map_source_error(error: DiscordAuthoritySourceError) -> FreshGuildAuthorityError {
    match error {
        DiscordAuthoritySourceError::NotFound => FreshGuildAuthorityError::InstallationNotFound,
        DiscordAuthoritySourceError::Unavailable => {
            FreshGuildAuthorityError::Backend("installation_authority_unavailable".into())
        }
        DiscordAuthoritySourceError::InvalidRecord => {
            FreshGuildAuthorityError::Backend("installation_authority_invalid".into())
        }
    }
}

fn map_client_error(error: DiscordAuthorityClientError) -> FreshGuildAuthorityError {
    match error {
        DiscordAuthorityClientError::Timeout => FreshGuildAuthorityError::Stale,
        DiscordAuthorityClientError::Inaccessible => FreshGuildAuthorityError::Forbidden,
        DiscordAuthorityClientError::Unavailable => {
            FreshGuildAuthorityError::Backend("discord_authority_unavailable".into())
        }
        DiscordAuthorityClientError::InvalidResponse => {
            FreshGuildAuthorityError::Backend("discord_authority_invalid_response".into())
        }
    }
}

fn validate_record(
    installation: &InstallationSelectorV1,
    record: &InstallationAuthorityRecordV1,
    client_application_id: crate::DiscordApplicationIdV1,
) -> Result<(), FreshGuildAuthorityError> {
    if installation.installation_id() != &record.installation_id {
        return Err(FreshGuildAuthorityError::ScopeMismatch);
    }
    if record.application_id != client_application_id {
        return Err(FreshGuildAuthorityError::ScopeMismatch);
    }
    if record.authority_digest.len() != DIGEST_LENGTH
        || !record
            .authority_digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(FreshGuildAuthorityError::Backend(
            "installation_authority_invalid".into(),
        ));
    }
    Ok(())
}

struct EvaluatedAuthority {
    effective_permissions: Permissions,
    owner: bool,
    contributing_role_count: NonZeroUsize,
    canonical_roles: Vec<(RoleId, Permissions)>,
}

fn evaluate_snapshot(
    record: &InstallationAuthorityRecordV1,
    snapshot: DiscordGuildAuthoritySnapshotV1,
) -> Result<EvaluatedAuthority, FreshGuildAuthorityError> {
    if snapshot.guild_id != record.guild_id || snapshot.member_user_id != record.acting_user_id {
        return Err(FreshGuildAuthorityError::ScopeMismatch);
    }
    if snapshot.member_is_bot || snapshot.member_is_system || snapshot.member_pending {
        return Err(FreshGuildAuthorityError::Forbidden);
    }
    let mut roles = BTreeMap::new();
    for role in snapshot.roles {
        if roles.insert(role.role_id, role.permissions).is_some() {
            return Err(FreshGuildAuthorityError::Backend(
                "discord_authority_duplicate_role".into(),
            ));
        }
    }
    let everyone_role = RoleId(record.guild_id.0);
    let everyone = roles.get(&everyone_role).copied().ok_or_else(|| {
        FreshGuildAuthorityError::Backend("discord_authority_missing_everyone_role".into())
    })?;
    let member_role_count = snapshot.member_role_ids.len();
    let member_role_ids = snapshot
        .member_role_ids
        .into_iter()
        .collect::<BTreeSet<_>>();
    if member_role_count != member_role_ids.len() {
        return Err(FreshGuildAuthorityError::Backend(
            "discord_authority_duplicate_member_role".into(),
        ));
    }
    let mut effective = everyone;
    let mut canonical_roles = vec![(everyone_role, everyone)];
    for role_id in member_role_ids {
        if role_id == everyone_role {
            return Err(FreshGuildAuthorityError::Backend(
                "discord_authority_invalid_member_roles".into(),
            ));
        }
        let permissions = roles.get(&role_id).copied().ok_or_else(|| {
            FreshGuildAuthorityError::Backend("discord_authority_missing_member_role".into())
        })?;
        effective |= permissions;
        canonical_roles.push((role_id, permissions));
    }
    let contributing_role_count = NonZeroUsize::new(canonical_roles.len())
        .ok_or_else(|| FreshGuildAuthorityError::Backend("discord_authority_empty_roles".into()))?;
    Ok(EvaluatedAuthority {
        effective_permissions: effective,
        owner: snapshot.owner_id == record.acting_user_id,
        contributing_role_count,
        canonical_roles,
    })
}

fn observation_digest(
    record: &InstallationAuthorityRecordV1,
    capability: CapabilityV1,
    evaluated: &EvaluatedAuthority,
    observed_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
) -> String {
    let mut hasher = Sha256::new();
    update_field(&mut hasher, b"starring.discord-authority.v1");
    update_field(&mut hasher, record.tenant_id.as_str().as_bytes());
    update_field(&mut hasher, record.installation_id.as_str().as_bytes());
    update_field(
        &mut hasher,
        record.application_id.get().to_string().as_bytes(),
    );
    update_field(&mut hasher, record.guild_id.0.to_string().as_bytes());
    update_field(&mut hasher, record.acting_user_id.0.to_string().as_bytes());
    update_field(&mut hasher, capability_name(capability));
    update_field(
        &mut hasher,
        evaluated
            .effective_permissions
            .bits()
            .to_string()
            .as_bytes(),
    );
    update_field(
        &mut hasher,
        if evaluated.owner { b"owner" } else { b"member" },
    );
    update_field(
        &mut hasher,
        record.authority_revision.get().to_string().as_bytes(),
    );
    update_field(&mut hasher, record.authority_digest.as_bytes());
    for (role_id, permissions) in &evaluated.canonical_roles {
        update_field(&mut hasher, role_id.0.to_string().as_bytes());
        update_field(&mut hasher, permissions.bits().to_string().as_bytes());
    }
    update_field(
        &mut hasher,
        observed_at.timestamp_millis().to_string().as_bytes(),
    );
    update_field(
        &mut hasher,
        expires_at.timestamp_millis().to_string().as_bytes(),
    );
    encode_lower_hex(hasher.finalize().as_slice())
}

fn update_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn capability_name(capability: CapabilityV1) -> &'static [u8] {
    match capability {
        CapabilityV1::Promote => b"promote",
        CapabilityV1::Read => b"read",
        CapabilityV1::Approve => b"approve",
        CapabilityV1::Reject => b"reject",
        CapabilityV1::Apply => b"apply",
    }
}

fn encode_lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}
