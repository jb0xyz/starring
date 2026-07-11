use std::collections::BTreeMap;

use automation_ruleset::{
    RuleSetActivation, RuleSetKey, RuleSetStore, RuleSetStoreError, RuleSetVersionId,
};
use desired_state::ResourceKey;
use discord_model::{GuildId, Permissions};
use resource_resolution::ResourceBindingMap;

use crate::gate::check_readiness;
use crate::types::{GuildCapabilities, ReadinessError, RuleSetReadinessInput, RuntimeRuleSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActivationError {
    VersionLookup(RuleSetStoreError),
    VersionNotFound,
    NotReady(ReadinessError),
    Activate(RuleSetStoreError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivationOutcome {
    pub activation: RuleSetActivation,
    pub runtime_ruleset: RuntimeRuleSet,
}

pub async fn activate_if_ready<S>(
    store: &S,
    guild_id: GuildId,
    key: &RuleSetKey,
    version: RuleSetVersionId,
    bindings: &ResourceBindingMap,
    guild_capabilities: &GuildCapabilities,
    role_permissions: &BTreeMap<ResourceKey, Permissions>,
) -> Result<ActivationOutcome, ActivationError>
where
    S: RuleSetStore,
{
    let artifact = store
        .get_version(guild_id, key, version)
        .await
        .map_err(ActivationError::VersionLookup)?
        .ok_or(ActivationError::VersionNotFound)?;
    let runtime_ruleset = check_readiness(RuleSetReadinessInput {
        artifact: &artifact,
        bindings,
        guild_capabilities,
        role_permissions,
    })
    .map_err(ActivationError::NotReady)?;
    let activation = store
        .activate(guild_id, key, version)
        .await
        .map_err(ActivationError::Activate)?;
    Ok(ActivationOutcome {
        activation,
        runtime_ruleset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use automation_ruleset::{
        InMemoryRuleSetStore, PublishOutcome, PublishRuleSetRequest, RuleSetVersion,
    };
    use automation_state::{ActionSpec, InteractionRule, InteractionRuleSet, TriggerSpec};
    use discord_model::UserId;
    use futures::executor::block_on;

    struct SpyStore {
        inner: InMemoryRuleSetStore,
        activate_calls: AtomicUsize,
        fail_activate: bool,
    }

    impl SpyStore {
        fn new(fail_activate: bool) -> Self {
            Self {
                inner: InMemoryRuleSetStore::default(),
                activate_calls: AtomicUsize::new(0),
                fail_activate,
            }
        }

        fn activate_calls(&self) -> usize {
            self.activate_calls.load(Ordering::SeqCst)
        }
    }

    impl RuleSetStore for SpyStore {
        async fn publish(
            &self,
            request: PublishRuleSetRequest,
        ) -> Result<PublishOutcome, RuleSetStoreError> {
            self.inner.publish(request).await
        }

        async fn get_version(
            &self,
            guild_id: GuildId,
            key: &RuleSetKey,
            version: RuleSetVersionId,
        ) -> Result<Option<RuleSetVersion>, RuleSetStoreError> {
            self.inner.get_version(guild_id, key, version).await
        }

        async fn list_versions(
            &self,
            guild_id: GuildId,
            key: &RuleSetKey,
        ) -> Result<Vec<RuleSetVersion>, RuleSetStoreError> {
            self.inner.list_versions(guild_id, key).await
        }

        async fn activate(
            &self,
            guild_id: GuildId,
            key: &RuleSetKey,
            version: RuleSetVersionId,
        ) -> Result<RuleSetActivation, RuleSetStoreError> {
            self.activate_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_activate {
                return Err(RuleSetStoreError::Backend("activate failed".to_string()));
            }
            self.inner.activate(guild_id, key, version).await
        }

        async fn active(
            &self,
            guild_id: GuildId,
            key: &RuleSetKey,
        ) -> Result<Option<RuleSetVersion>, RuleSetStoreError> {
            self.inner.active(guild_id, key).await
        }
    }

    const GUILD: GuildId = GuildId(7);

    fn key() -> RuleSetKey {
        RuleSetKey::parse("studyroom").unwrap()
    }

    fn create_role_rule() -> InteractionRuleSet {
        InteractionRuleSet {
            version: 1,
            panels: vec![],
            modals: vec![],
            rules: vec![InteractionRule {
                key: "r".to_string(),
                trigger: TriggerSpec::InstanceAction {
                    action: "test".to_string(),
                },
                actions: vec![
                    ActionSpec::DeferEphemeral,
                    ActionSpec::CreateRole {
                        key: "role".to_string(),
                        name: "n".to_string(),
                    },
                    ActionSpec::EditResponse {
                        content: "done".to_string(),
                    },
                ],
            }],
        }
    }

    fn no_capability_rule() -> InteractionRuleSet {
        InteractionRuleSet {
            version: 1,
            panels: vec![],
            modals: vec![],
            rules: vec![InteractionRule {
                key: "r".to_string(),
                trigger: TriggerSpec::InstanceAction {
                    action: "test".to_string(),
                },
                actions: vec![
                    ActionSpec::DeferEphemeral,
                    ActionSpec::EditResponse {
                        content: "done".to_string(),
                    },
                ],
            }],
        }
    }

    fn publish(store: &SpyStore, def: InteractionRuleSet) -> RuleSetVersionId {
        let outcome = block_on(store.publish(PublishRuleSetRequest {
            guild_id: GUILD,
            ruleset_key: key(),
            definition: def,
            created_by: UserId(1),
        }))
        .unwrap();
        match outcome {
            PublishOutcome::Created(v) => v.version,
            PublishOutcome::Reused(v) => v.version,
        }
    }

    fn admin() -> GuildCapabilities {
        GuildCapabilities {
            base_permissions: Permissions::ADMINISTRATOR,
        }
    }

    fn no_manage_roles() -> GuildCapabilities {
        GuildCapabilities {
            base_permissions: Permissions::SEND_MESSAGES,
        }
    }

    fn call(
        store: &SpyStore,
        version: RuleSetVersionId,
        caps: &GuildCapabilities,
    ) -> Result<ActivationOutcome, ActivationError> {
        let bindings = ResourceBindingMap::default();
        let roles = BTreeMap::new();
        block_on(activate_if_ready(
            store,
            GUILD,
            &key(),
            version,
            &bindings,
            caps,
            &roles,
        ))
    }

    #[test]
    fn version_not_found_skips_activate() {
        let store = SpyStore::new(false);
        let missing = RuleSetVersionId::new(9).unwrap();
        assert_eq!(
            call(&store, missing, &admin()).unwrap_err(),
            ActivationError::VersionNotFound
        );
        assert_eq!(store.activate_calls(), 0);
    }

    #[test]
    fn not_ready_leaves_active_unchanged() {
        let store = SpyStore::new(false);
        let v1 = publish(&store, no_capability_rule());
        call(&store, v1, &admin()).unwrap();
        let v2 = publish(&store, create_role_rule());
        assert!(matches!(
            call(&store, v2, &no_manage_roles()).unwrap_err(),
            ActivationError::NotReady(_)
        ));
        assert_eq!(store.activate_calls(), 1);
        assert_eq!(
            block_on(store.active(GUILD, &key()))
                .unwrap()
                .unwrap()
                .version,
            v1
        );
    }

    #[test]
    fn ready_activates_once() {
        let store = SpyStore::new(false);
        let v1 = publish(&store, create_role_rule());
        let outcome = call(&store, v1, &admin()).unwrap();
        assert_eq!(outcome.activation.active_version, v1);
        assert_eq!(store.activate_calls(), 1);
    }

    #[test]
    fn already_active_reruns_gate() {
        let store = SpyStore::new(false);
        let v1 = publish(&store, create_role_rule());
        call(&store, v1, &admin()).unwrap();
        call(&store, v1, &admin()).unwrap();
        assert_eq!(store.activate_calls(), 2);
    }

    #[test]
    fn notices_preserved() {
        let store = SpyStore::new(false);
        let v1 = publish(&store, create_role_rule());
        let outcome = call(&store, v1, &admin()).unwrap();
        assert!(!outcome.runtime_ruleset.notices.is_empty());
    }

    #[test]
    fn activate_error_keeps_pointer() {
        let store = SpyStore::new(true);
        let v1 = publish(&store, create_role_rule());
        assert!(matches!(
            call(&store, v1, &admin()).unwrap_err(),
            ActivationError::Activate(_)
        ));
        assert!(block_on(store.active(GUILD, &key())).unwrap().is_none());
    }
}
