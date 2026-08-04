use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use automation_instance::{InstanceId, InstanceTeardownClaimOutcomeV1, InstanceTeardownStoreV1};
use discord_model::GuildId;

use crate::domain::{
    ExactInstanceTeardownRequestV1, InstanceDeleter, InstanceResource,
    InstanceTeardownRecoveryObservationV1, TeardownError, TeardownOutcome,
};

#[allow(async_fn_in_trait)]
pub trait InstanceTeardownService {
    async fn teardown(
        &self,
        guild_id: GuildId,
        instance_id: InstanceId,
    ) -> Result<TeardownOutcome, TeardownError>;
}

#[allow(async_fn_in_trait)]
pub trait DurableInstanceTeardownServiceV1: InstanceTeardownService {
    async fn teardown_exact_v1(
        &self,
        request: &ExactInstanceTeardownRequestV1,
    ) -> Result<TeardownOutcome, TeardownError>;

    async fn observe_teardown_exact_v1(
        &self,
        request: &ExactInstanceTeardownRequestV1,
    ) -> Result<InstanceTeardownRecoveryObservationV1, TeardownError>;
}

#[derive(Default)]
struct LockEntry {
    held: AtomicBool,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TeardownKey {
    guild_id: GuildId,
    instance_id: InstanceId,
}

struct KeyLockGuard<'a> {
    registry: &'a Mutex<BTreeMap<TeardownKey, Arc<LockEntry>>>,
    key: TeardownKey,
    entry: Arc<LockEntry>,
}

impl Drop for KeyLockGuard<'_> {
    fn drop(&mut self) {
        let mut registry = self.registry.lock().unwrap();
        self.entry.held.store(false, Ordering::Release);
        if Arc::strong_count(&self.entry) == 2
            && registry
                .get(&self.key)
                .is_some_and(|entry| Arc::ptr_eq(entry, &self.entry))
        {
            registry.remove(&self.key);
        }
    }
}

pub struct Teardown<S, D> {
    store: S,
    deleter: D,
    locks: Mutex<BTreeMap<TeardownKey, Arc<LockEntry>>>,
}

impl<S, D> Teardown<S, D> {
    pub fn new(store: S, deleter: D) -> Self {
        Self {
            store,
            deleter,
            locks: Mutex::new(BTreeMap::new()),
        }
    }

    fn try_lock(&self, guild_id: GuildId, instance_id: &InstanceId) -> Option<KeyLockGuard<'_>> {
        let key = TeardownKey {
            guild_id,
            instance_id: instance_id.clone(),
        };
        let entry = {
            let mut locks = self.locks.lock().unwrap();
            locks.entry(key.clone()).or_default().clone()
        };
        if entry
            .held
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return None;
        }
        Some(KeyLockGuard {
            registry: &self.locks,
            key: key.clone(),
            entry,
        })
    }
}

impl<S, D> Teardown<S, D>
where
    S: InstanceTeardownStoreV1,
    D: InstanceDeleter,
{
    async fn teardown_locked(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
        exact: Option<&ExactInstanceTeardownRequestV1>,
    ) -> Result<TeardownOutcome, TeardownError> {
        let initial = self
            .store
            .get_for_teardown_v1(guild_id, instance_id)
            .await
            .map_err(TeardownError::Lookup)?
            .ok_or(TeardownError::InstanceNotFound)?;
        if exact.is_some_and(|request| !request.matches_instance_v1(&initial)) {
            return Err(TeardownError::ManifestDrift);
        }
        let claim = self
            .store
            .claim_deleting_v1(guild_id, instance_id)
            .await
            .map_err(TeardownError::Store)?;
        let instance = self
            .store
            .get_for_teardown_v1(guild_id, instance_id)
            .await
            .map_err(TeardownError::Lookup)?
            .ok_or(TeardownError::InstanceNotFound)?;
        if exact.is_some_and(|request| !request.matches_instance_v1(&instance)) {
            return Err(TeardownError::ManifestDrift);
        }
        let first_owner = match claim {
            InstanceTeardownClaimOutcomeV1::Claimed => true,
            InstanceTeardownClaimOutcomeV1::AlreadyDeleting => false,
            InstanceTeardownClaimOutcomeV1::AlreadyDeleted => {
                return Ok(TeardownOutcome::AlreadyDeleted);
            }
        };

        for (alias, message) in &instance.resources.messages {
            let resource = InstanceResource::Message {
                alias: alias.clone(),
                channel: message.channel,
                id: message.id,
            };
            self.deleter
                .delete_message(guild_id, message.channel, message.id)
                .await
                .map_err(|source| TeardownError::DeleteFailed { resource, source })?;
        }
        for (alias, id) in &instance.resources.channels {
            let resource = InstanceResource::Channel {
                alias: alias.clone(),
                id: *id,
            };
            self.deleter
                .delete_channel(guild_id, *id)
                .await
                .map_err(|source| TeardownError::DeleteFailed { resource, source })?;
        }
        for (alias, id) in &instance.resources.roles {
            let resource = InstanceResource::Role {
                alias: alias.clone(),
                id: *id,
            };
            self.deleter
                .delete_role(guild_id, *id)
                .await
                .map_err(|source| TeardownError::DeleteFailed { resource, source })?;
        }

        self.store
            .mark_deleted_v1(guild_id, instance_id)
            .await
            .map_err(TeardownError::Store)?;
        Ok(if first_owner {
            TeardownOutcome::Completed
        } else {
            TeardownOutcome::ResumedAndCompleted
        })
    }
}

impl<S, D> InstanceTeardownService for Teardown<S, D>
where
    S: InstanceTeardownStoreV1,
    D: InstanceDeleter,
{
    async fn teardown(
        &self,
        guild_id: GuildId,
        instance_id: InstanceId,
    ) -> Result<TeardownOutcome, TeardownError> {
        let Some(_guard) = self.try_lock(guild_id, &instance_id) else {
            return Ok(TeardownOutcome::InProgress);
        };
        self.teardown_locked(guild_id, &instance_id, None).await
    }
}

impl<S, D> DurableInstanceTeardownServiceV1 for Teardown<S, D>
where
    S: InstanceTeardownStoreV1,
    D: InstanceDeleter,
{
    async fn teardown_exact_v1(
        &self,
        request: &ExactInstanceTeardownRequestV1,
    ) -> Result<TeardownOutcome, TeardownError> {
        let Some(_guard) = self.try_lock(request.guild_id(), request.instance_id()) else {
            return Ok(TeardownOutcome::InProgress);
        };
        self.teardown_locked(request.guild_id(), request.instance_id(), Some(request))
            .await
    }

    async fn observe_teardown_exact_v1(
        &self,
        request: &ExactInstanceTeardownRequestV1,
    ) -> Result<InstanceTeardownRecoveryObservationV1, TeardownError> {
        let instance = self
            .store
            .get_for_teardown_v1(request.guild_id(), request.instance_id())
            .await
            .map_err(TeardownError::Lookup)?
            .ok_or(TeardownError::InstanceNotFound)?;
        if !request.matches_instance_v1(&instance) {
            return Err(TeardownError::ManifestDrift);
        }
        Ok(match instance.status {
            automation_instance::InstanceStatus::Active
            | automation_instance::InstanceStatus::Disabled => {
                InstanceTeardownRecoveryObservationV1::ProvenNotStarted
            }
            automation_instance::InstanceStatus::Deleting => {
                InstanceTeardownRecoveryObservationV1::DurableRetryPending
            }
            automation_instance::InstanceStatus::Deleted => {
                InstanceTeardownRecoveryObservationV1::ProvenSucceeded
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use automation_instance::InstanceId;
    use discord_model::GuildId;

    use super::Teardown;

    #[test]
    fn released_lock_is_removed_from_registry() {
        let service = Teardown::new((), ());
        let id = InstanceId::parse("room_001").unwrap();
        {
            let _guard = service.try_lock(GuildId(7), &id).unwrap();
            assert_eq!(service.locks.lock().unwrap().len(), 1);
        }
        assert!(service.locks.lock().unwrap().is_empty());
    }
}
