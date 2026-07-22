use std::collections::BTreeMap;
use std::sync::Mutex;

use discord_model::GuildId;

use crate::id::InstanceId;
use crate::model::{AutomationInstance, InstanceStatus};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstanceStoreError {
    DuplicateInstance,
    NotFound,
    Backend(String),
}

#[allow(async_fn_in_trait)]
pub trait InstanceRouteReaderV1 {
    async fn read_instance_route_v1(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<Option<AutomationInstance>, InstanceStoreError>;
}

#[allow(async_fn_in_trait)]
pub trait InstanceRegistrarV1 {
    async fn register_instance_v1(
        &self,
        instance: AutomationInstance,
    ) -> Result<(), InstanceStoreError>;
}

#[derive(Clone, Copy)]
pub struct LegacyInstanceStoreCapabilitiesV1<'a, T: ?Sized> {
    store: &'a T,
}

impl<'a, T: ?Sized> LegacyInstanceStoreCapabilitiesV1<'a, T> {
    pub fn new(store: &'a T) -> Self {
        Self { store }
    }
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
    async fn transition_to_deleting(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<(), InstanceStoreError>;
    async fn mark_deleted(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<(), InstanceStoreError>;
    async fn list_deleting(
        &self,
        guild_id: GuildId,
    ) -> Result<Vec<AutomationInstance>, InstanceStoreError>;
}

impl<T> InstanceRouteReaderV1 for LegacyInstanceStoreCapabilitiesV1<'_, T>
where
    T: InstanceStore + ?Sized,
{
    async fn read_instance_route_v1(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<Option<AutomationInstance>, InstanceStoreError> {
        self.store.get(guild_id, instance_id).await
    }
}

impl<T> InstanceRegistrarV1 for LegacyInstanceStoreCapabilitiesV1<'_, T>
where
    T: InstanceStore + ?Sized,
{
    async fn register_instance_v1(
        &self,
        instance: AutomationInstance,
    ) -> Result<(), InstanceStoreError> {
        self.store.register(instance).await
    }
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

    async fn transition_to_deleting(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<(), InstanceStoreError> {
        let mut guilds = self.inner.lock().unwrap();
        let instance = guilds
            .get_mut(&guild_id)
            .and_then(|entries| entries.get_mut(instance_id))
            .filter(|instance| instance.status == InstanceStatus::Active)
            .ok_or(InstanceStoreError::NotFound)?;
        instance.status = InstanceStatus::Deleting;
        Ok(())
    }

    async fn mark_deleted(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<(), InstanceStoreError> {
        let mut guilds = self.inner.lock().unwrap();
        let instance = guilds
            .get_mut(&guild_id)
            .and_then(|entries| entries.get_mut(instance_id))
            .filter(|instance| instance.status == InstanceStatus::Deleting)
            .ok_or(InstanceStoreError::NotFound)?;
        instance.status = InstanceStatus::Deleted;
        Ok(())
    }

    async fn list_deleting(
        &self,
        guild_id: GuildId,
    ) -> Result<Vec<AutomationInstance>, InstanceStoreError> {
        let guilds = self.inner.lock().unwrap();
        Ok(guilds
            .get(&guild_id)
            .map(|entries| {
                entries
                    .values()
                    .filter(|instance| instance.status == InstanceStatus::Deleting)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }
}

impl InstanceRouteReaderV1 for InMemoryInstanceStore {
    async fn read_instance_route_v1(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<Option<AutomationInstance>, InstanceStoreError> {
        self.get(guild_id, instance_id).await
    }
}

impl InstanceRegistrarV1 for InMemoryInstanceStore {
    async fn register_instance_v1(
        &self,
        instance: AutomationInstance,
    ) -> Result<(), InstanceStoreError> {
        self.register(instance).await
    }
}

#[cfg(test)]
mod interaction_tests {
    use discord_model::GuildId;
    use futures::executor::block_on;

    use super::*;

    struct NarrowOnlyStore;

    impl InstanceRouteReaderV1 for NarrowOnlyStore {
        async fn read_instance_route_v1(
            &self,
            _guild_id: GuildId,
            _instance_id: &InstanceId,
        ) -> Result<Option<AutomationInstance>, InstanceStoreError> {
            Ok(None)
        }
    }

    impl InstanceRegistrarV1 for NarrowOnlyStore {
        async fn register_instance_v1(
            &self,
            _instance: AutomationInstance,
        ) -> Result<(), InstanceStoreError> {
            Ok(())
        }
    }

    fn accepts_narrow_store(_store: &(impl InstanceRouteReaderV1 + InstanceRegistrarV1)) {}

    #[test]
    fn production_adapter_can_implement_only_narrow_traits() {
        let store = NarrowOnlyStore;
        accepts_narrow_store(&store);
        let id = InstanceId::parse("room_a").unwrap();
        assert_eq!(
            block_on(store.read_instance_route_v1(GuildId(7), &id)).unwrap(),
            None
        );
    }

    #[test]
    fn explicit_legacy_capability_adapter_forwards_a_broad_store() {
        let store = InMemoryInstanceStore::new();
        let adapter = LegacyInstanceStoreCapabilitiesV1::new(&store);
        accepts_narrow_store(&adapter);
        let id = InstanceId::parse("room_a").unwrap();
        assert_eq!(
            block_on(adapter.read_instance_route_v1(GuildId(7), &id)).unwrap(),
            None
        );
    }
}
