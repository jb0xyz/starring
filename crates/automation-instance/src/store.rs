use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::sync::Mutex;

use discord_model::GuildId;

use crate::id::InstanceId;
use crate::model::{AutomationInstance, InstanceStatus};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstanceStoreError {
    DuplicateInstance,
    NotFound,
    TimedOut,
    Backend(String),
}

pub const MAX_INSTANCE_TEARDOWN_RETRY_BATCH_V1: usize = 256;
pub const MAX_INSTANCE_TEARDOWN_RETRY_SCAN_BATCH_V2: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstanceTeardownRetryKeyV2 {
    guild_id: GuildId,
    instance_id: InstanceId,
}

impl InstanceTeardownRetryKeyV2 {
    pub fn new(guild_id: GuildId, instance_id: InstanceId) -> Option<Self> {
        (guild_id.0 != 0).then_some(Self {
            guild_id,
            instance_id,
        })
    }

    pub fn guild_id(&self) -> GuildId {
        self.guild_id
    }

    pub fn instance_id(&self) -> &InstanceId {
        &self.instance_id
    }

    pub fn cmp_c_v2(&self, other: &Self) -> Ordering {
        self.guild_id
            .to_string()
            .cmp(&other.guild_id.to_string())
            .then_with(|| self.instance_id.as_str().cmp(other.instance_id.as_str()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstanceTeardownRetryScanCursorV2 {
    after: Option<InstanceTeardownRetryKeyV2>,
    through: Option<InstanceTeardownRetryKeyV2>,
}

impl InstanceTeardownRetryScanCursorV2 {
    pub fn initial() -> Self {
        Self {
            after: None,
            through: None,
        }
    }

    pub fn continue_after(
        after: InstanceTeardownRetryKeyV2,
        through: InstanceTeardownRetryKeyV2,
    ) -> Option<Self> {
        (after.cmp_c_v2(&through) == Ordering::Less).then_some(Self {
            after: Some(after),
            through: Some(through),
        })
    }

    pub fn after(&self) -> Option<&InstanceTeardownRetryKeyV2> {
        self.after.as_ref()
    }

    pub fn through(&self) -> Option<&InstanceTeardownRetryKeyV2> {
        self.through.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstanceTeardownRetryScanPageV2 {
    keys: Vec<InstanceTeardownRetryKeyV2>,
    through: Option<InstanceTeardownRetryKeyV2>,
}

impl InstanceTeardownRetryScanPageV2 {
    pub fn new(
        keys: Vec<InstanceTeardownRetryKeyV2>,
        through: Option<InstanceTeardownRetryKeyV2>,
        limit: NonZeroUsize,
    ) -> Option<Self> {
        if limit.get() > MAX_INSTANCE_TEARDOWN_RETRY_SCAN_BATCH_V2
            || keys.len() > limit.get()
            || keys
                .windows(2)
                .any(|pair| pair[0].cmp_c_v2(&pair[1]) != Ordering::Less)
            || keys.iter().any(|key| {
                through
                    .as_ref()
                    .is_none_or(|through| key.cmp_c_v2(through) == Ordering::Greater)
            })
            || (!keys.is_empty() && through.is_none())
        {
            return None;
        }
        Some(Self { keys, through })
    }

    pub fn keys(&self) -> &[InstanceTeardownRetryKeyV2] {
        &self.keys
    }

    pub fn through(&self) -> Option<&InstanceTeardownRetryKeyV2> {
        self.through.as_ref()
    }

    pub fn next_cursor_v2(&self) -> Option<InstanceTeardownRetryScanCursorV2> {
        let last = self.keys.last()?.clone();
        let through = self.through.clone()?;
        InstanceTeardownRetryScanCursorV2::continue_after(last, through)
    }
}

#[allow(async_fn_in_trait)]
pub trait InstanceTeardownRetryScannerV2 {
    async fn scan_retryable_v2(
        &self,
        cursor: &InstanceTeardownRetryScanCursorV2,
        limit: NonZeroUsize,
    ) -> Result<InstanceTeardownRetryScanPageV2, InstanceStoreError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstanceTeardownClaimOutcomeV1 {
    Claimed,
    AlreadyDeleting,
    AlreadyDeleted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstanceTeardownMarkOutcomeV1 {
    MarkedDeleted,
    AlreadyDeleted,
}

#[allow(async_fn_in_trait)]
pub trait InstanceTeardownStoreV1 {
    async fn get_for_teardown_v1(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<Option<AutomationInstance>, InstanceStoreError>;

    async fn claim_deleting_v1(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<InstanceTeardownClaimOutcomeV1, InstanceStoreError>;

    async fn mark_deleted_v1(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<InstanceTeardownMarkOutcomeV1, InstanceStoreError>;

    async fn list_retryable_v1(
        &self,
        guild_id: GuildId,
        limit: NonZeroUsize,
    ) -> Result<Vec<AutomationInstance>, InstanceStoreError>;
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

impl InstanceTeardownStoreV1 for InMemoryInstanceStore {
    async fn get_for_teardown_v1(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<Option<AutomationInstance>, InstanceStoreError> {
        self.get(guild_id, instance_id).await
    }

    async fn claim_deleting_v1(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<InstanceTeardownClaimOutcomeV1, InstanceStoreError> {
        let mut guilds = self.inner.lock().unwrap();
        let instance = guilds
            .get_mut(&guild_id)
            .and_then(|entries| entries.get_mut(instance_id))
            .ok_or(InstanceStoreError::NotFound)?;
        match instance.status {
            InstanceStatus::Active | InstanceStatus::Disabled => {
                instance.status = InstanceStatus::Deleting;
                Ok(InstanceTeardownClaimOutcomeV1::Claimed)
            }
            InstanceStatus::Deleting => Ok(InstanceTeardownClaimOutcomeV1::AlreadyDeleting),
            InstanceStatus::Deleted => Ok(InstanceTeardownClaimOutcomeV1::AlreadyDeleted),
        }
    }

    async fn mark_deleted_v1(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<InstanceTeardownMarkOutcomeV1, InstanceStoreError> {
        let mut guilds = self.inner.lock().unwrap();
        let instance = guilds
            .get_mut(&guild_id)
            .and_then(|entries| entries.get_mut(instance_id))
            .ok_or(InstanceStoreError::NotFound)?;
        match instance.status {
            InstanceStatus::Deleting => {
                instance.status = InstanceStatus::Deleted;
                Ok(InstanceTeardownMarkOutcomeV1::MarkedDeleted)
            }
            InstanceStatus::Deleted => Ok(InstanceTeardownMarkOutcomeV1::AlreadyDeleted),
            InstanceStatus::Active | InstanceStatus::Disabled => Err(InstanceStoreError::NotFound),
        }
    }

    async fn list_retryable_v1(
        &self,
        guild_id: GuildId,
        limit: NonZeroUsize,
    ) -> Result<Vec<AutomationInstance>, InstanceStoreError> {
        if limit.get() > MAX_INSTANCE_TEARDOWN_RETRY_BATCH_V1 {
            return Err(InstanceStoreError::Backend(
                "instance_teardown_retry_batch_invalid".to_string(),
            ));
        }
        let guilds = self.inner.lock().unwrap();
        Ok(guilds
            .get(&guild_id)
            .map(|entries| {
                entries
                    .values()
                    .filter(|instance| instance.status == InstanceStatus::Deleting)
                    .take(limit.get())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }
}

impl InstanceTeardownRetryScannerV2 for InMemoryInstanceStore {
    async fn scan_retryable_v2(
        &self,
        cursor: &InstanceTeardownRetryScanCursorV2,
        limit: NonZeroUsize,
    ) -> Result<InstanceTeardownRetryScanPageV2, InstanceStoreError> {
        if limit.get() > MAX_INSTANCE_TEARDOWN_RETRY_SCAN_BATCH_V2 {
            return Err(InstanceStoreError::Backend(
                "instance_teardown_retry_scan_batch_invalid".to_string(),
            ));
        }
        let guilds = self.inner.lock().unwrap();
        let mut deleting = guilds
            .iter()
            .flat_map(|(guild_id, entries)| {
                entries
                    .values()
                    .filter(|instance| instance.status == InstanceStatus::Deleting)
                    .map(|instance| {
                        InstanceTeardownRetryKeyV2::new(*guild_id, instance.id.clone()).unwrap()
                    })
            })
            .collect::<Vec<_>>();
        deleting.sort_unstable_by(InstanceTeardownRetryKeyV2::cmp_c_v2);
        let through = cursor
            .through()
            .cloned()
            .or_else(|| deleting.last().cloned());
        let keys = deleting
            .into_iter()
            .filter(|key| {
                cursor
                    .after()
                    .is_none_or(|after| key.cmp_c_v2(after) == Ordering::Greater)
                    && through
                        .as_ref()
                        .is_some_and(|through| key.cmp_c_v2(through) != Ordering::Greater)
            })
            .take(limit.get())
            .collect::<Vec<_>>();
        InstanceTeardownRetryScanPageV2::new(keys, through, limit).ok_or_else(|| {
            InstanceStoreError::Backend("instance_teardown_retry_scan_corrupt".to_string())
        })
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
