use std::collections::BTreeMap;
use std::sync::Mutex;

use discord_model::GuildId;

use crate::id::InstanceId;
use crate::model::{AutomationInstance, InstanceStatus};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstanceStoreError {
    DuplicateInstance,
    NotFound,
}

#[allow(async_fn_in_trait)]
pub trait InstanceStore {
    async fn register(&self, instance: AutomationInstance) -> Result<(), InstanceStoreError>;
    async fn get(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<Option<AutomationInstance>, InstanceStoreError>;
    async fn list_by_guild(
        &self,
        guild_id: GuildId,
    ) -> Result<Vec<AutomationInstance>, InstanceStoreError>;
    async fn update_status(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
        status: InstanceStatus,
    ) -> Result<(), InstanceStoreError>;
}

#[derive(Default)]
pub struct InMemoryInstanceStore {
    inner: Mutex<BTreeMap<GuildId, BTreeMap<InstanceId, AutomationInstance>>>,
}

impl InMemoryInstanceStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl InstanceStore for InMemoryInstanceStore {
    async fn register(&self, instance: AutomationInstance) -> Result<(), InstanceStoreError> {
        let mut guilds = self.inner.lock().unwrap();
        let entries = guilds.entry(instance.guild_id).or_default();
        if entries.contains_key(&instance.id) {
            return Err(InstanceStoreError::DuplicateInstance);
        }
        entries.insert(instance.id.clone(), instance);
        Ok(())
    }

    async fn get(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<Option<AutomationInstance>, InstanceStoreError> {
        let guilds = self.inner.lock().unwrap();
        Ok(guilds
            .get(&guild_id)
            .and_then(|entries| entries.get(instance_id))
            .cloned())
    }

    async fn list_by_guild(
        &self,
        guild_id: GuildId,
    ) -> Result<Vec<AutomationInstance>, InstanceStoreError> {
        let guilds = self.inner.lock().unwrap();
        Ok(guilds
            .get(&guild_id)
            .map(|entries| entries.values().cloned().collect())
            .unwrap_or_default())
    }

    async fn update_status(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
        status: InstanceStatus,
    ) -> Result<(), InstanceStoreError> {
        let mut guilds = self.inner.lock().unwrap();
        let instance = guilds
            .get_mut(&guild_id)
            .and_then(|entries| entries.get_mut(instance_id))
            .ok_or(InstanceStoreError::NotFound)?;
        instance.status = status;
        Ok(())
    }
}
