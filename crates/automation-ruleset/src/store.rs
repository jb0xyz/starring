use std::collections::BTreeMap;
use std::sync::Mutex;

use automation_core::validate_structural;
use automation_core::ValidationError;
use automation_state::InteractionRuleSet;
use discord_model::{GuildId, UserId};

use crate::hash::{RuleSetHasher, Sha256RuleSetHasher};
use crate::key::RuleSetKey;
use crate::model::{RuleSetActivation, RuleSetVersion};
use crate::version::RuleSetVersionId;
use crate::version::CURRENT_RULESET_SCHEMA_VERSION;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishRuleSetRequest {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub definition: InteractionRuleSet,
    pub created_by: UserId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublishOutcome {
    Created(RuleSetVersion),
    Reused(RuleSetVersion),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleSetStoreError {
    InvalidDefinition(Vec<ValidationError>),
    VersionNotFound,
    VersionOverflow,
    HashCollision,
    Canonicalization(String),
    Backend(String),
}

#[allow(async_fn_in_trait)]
pub trait RuleSetStore {
    async fn publish(
        &self,
        request: PublishRuleSetRequest,
    ) -> Result<PublishOutcome, RuleSetStoreError>;

    async fn get_version(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
        version: RuleSetVersionId,
    ) -> Result<Option<RuleSetVersion>, RuleSetStoreError>;

    async fn list_versions(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
    ) -> Result<Vec<RuleSetVersion>, RuleSetStoreError>;

    async fn activate(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
        version: RuleSetVersionId,
    ) -> Result<RuleSetActivation, RuleSetStoreError>;

    async fn active(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
    ) -> Result<Option<RuleSetVersion>, RuleSetStoreError>;
}

#[derive(Default)]
struct GuildRuleSet {
    versions: BTreeMap<RuleSetVersionId, RuleSetVersion>,
    active: Option<RuleSetVersionId>,
}

pub struct InMemoryRuleSetStore<H: RuleSetHasher = Sha256RuleSetHasher> {
    hasher: H,
    inner: Mutex<BTreeMap<(GuildId, RuleSetKey), GuildRuleSet>>,
}

impl Default for InMemoryRuleSetStore<Sha256RuleSetHasher> {
    fn default() -> Self {
        Self::new(Sha256RuleSetHasher)
    }
}

impl<H: RuleSetHasher> InMemoryRuleSetStore<H> {
    pub fn new(hasher: H) -> Self {
        Self {
            hasher,
            inner: Mutex::new(BTreeMap::new()),
        }
    }
}

impl<H: RuleSetHasher> RuleSetStore for InMemoryRuleSetStore<H> {
    async fn publish(
        &self,
        request: PublishRuleSetRequest,
    ) -> Result<PublishOutcome, RuleSetStoreError> {
        validate_structural(&request.definition).map_err(RuleSetStoreError::InvalidDefinition)?;
        let schema_version = CURRENT_RULESET_SCHEMA_VERSION;
        let content_hash = self
            .hasher
            .hash(schema_version, &request.definition)
            .map_err(|e| match e {
                crate::hash::RuleSetHashError::Serialization(m) => {
                    RuleSetStoreError::Canonicalization(m)
                }
            })?;
        let mut guilds = self.inner.lock().unwrap();
        let entry = guilds
            .entry((request.guild_id, request.ruleset_key.clone()))
            .or_default();
        for existing in entry.versions.values() {
            if existing.content_hash == content_hash {
                if existing.definition == request.definition {
                    return Ok(PublishOutcome::Reused(existing.clone()));
                }
                return Err(RuleSetStoreError::HashCollision);
            }
        }
        let version = match entry.versions.keys().next_back() {
            Some(max) => max.next().map_err(|_| RuleSetStoreError::VersionOverflow)?,
            None => RuleSetVersionId::FIRST,
        };
        let record = RuleSetVersion {
            guild_id: request.guild_id,
            ruleset_key: request.ruleset_key,
            version,
            schema_version,
            definition: request.definition,
            content_hash,
            created_by: request.created_by,
        };
        entry.versions.insert(version, record.clone());
        Ok(PublishOutcome::Created(record))
    }

    async fn get_version(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
        version: RuleSetVersionId,
    ) -> Result<Option<RuleSetVersion>, RuleSetStoreError> {
        let guilds = self.inner.lock().unwrap();
        Ok(guilds
            .get(&(guild_id, key.clone()))
            .and_then(|entry| entry.versions.get(&version))
            .cloned())
    }

    async fn list_versions(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
    ) -> Result<Vec<RuleSetVersion>, RuleSetStoreError> {
        let guilds = self.inner.lock().unwrap();
        Ok(guilds
            .get(&(guild_id, key.clone()))
            .map(|entry| entry.versions.values().cloned().collect())
            .unwrap_or_default())
    }

    async fn activate(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
        version: RuleSetVersionId,
    ) -> Result<RuleSetActivation, RuleSetStoreError> {
        let mut guilds = self.inner.lock().unwrap();
        let entry = guilds
            .get_mut(&(guild_id, key.clone()))
            .filter(|entry| entry.versions.contains_key(&version))
            .ok_or(RuleSetStoreError::VersionNotFound)?;
        entry.active = Some(version);
        Ok(RuleSetActivation {
            guild_id,
            ruleset_key: key.clone(),
            active_version: version,
        })
    }

    async fn active(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
    ) -> Result<Option<RuleSetVersion>, RuleSetStoreError> {
        let guilds = self.inner.lock().unwrap();
        Ok(guilds.get(&(guild_id, key.clone())).and_then(|entry| {
            entry
                .active
                .and_then(|version| entry.versions.get(&version))
                .cloned()
        }))
    }
}
