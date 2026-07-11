use std::collections::BTreeMap;

use automation_ruleset::{RuleSetKey, RuleSetStore};
use desired_state::ResourceKey;
use discord_model::{GuildId, Permissions};
use resource_resolution::ResourceBindingMap;

use crate::gate::check_readiness;
use crate::types::{GuildCapabilities, HydrationError, RuleSetReadinessInput, RuntimeRuleSet};

pub async fn hydrate_active_ruleset(
    store: &impl RuleSetStore,
    guild_id: GuildId,
    key: &RuleSetKey,
    bindings: &ResourceBindingMap,
    guild_capabilities: &GuildCapabilities,
    role_permissions: &BTreeMap<ResourceKey, Permissions>,
) -> Result<RuntimeRuleSet, HydrationError> {
    let artifact = store
        .active(guild_id, key)
        .await
        .map_err(HydrationError::Store)?
        .ok_or(HydrationError::NoActiveRuleSet)?;
    check_readiness(RuleSetReadinessInput {
        artifact: &artifact,
        bindings,
        guild_capabilities,
        role_permissions,
    })
    .map_err(HydrationError::NotReady)
}

#[cfg(test)]
mod tests {
    use super::*;
    use automation_ruleset::{InMemoryRuleSetStore, PublishRuleSetRequest};
    use automation_state::{ActionSpec, InteractionRule, InteractionRuleSet, TriggerSpec};
    use discord_model::UserId;
    use futures::executor::block_on;

    fn def() -> InteractionRuleSet {
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

    fn key() -> RuleSetKey {
        RuleSetKey::parse("studyroom").unwrap()
    }

    #[test]
    fn no_active_is_error() {
        let store = InMemoryRuleSetStore::default();
        let roles = BTreeMap::new();
        let caps = GuildCapabilities {
            base_permissions: Permissions::ADMINISTRATOR,
        };
        let err = block_on(hydrate_active_ruleset(
            &store,
            GuildId(7),
            &key(),
            &ResourceBindingMap::default(),
            &caps,
            &roles,
        ))
        .unwrap_err();
        assert_eq!(err, HydrationError::NoActiveRuleSet);
    }

    #[test]
    fn publish_activate_then_hydrate() {
        let store = InMemoryRuleSetStore::default();
        let published = block_on(store.publish(PublishRuleSetRequest {
            guild_id: GuildId(7),
            ruleset_key: key(),
            definition: def(),
            created_by: UserId(1),
        }))
        .unwrap();
        let version = match published {
            automation_ruleset::PublishOutcome::Created(v) => v.version,
            automation_ruleset::PublishOutcome::Reused(v) => v.version,
        };
        block_on(store.activate(GuildId(7), &key(), version)).unwrap();

        let caps = GuildCapabilities {
            base_permissions: Permissions::ADMINISTRATOR,
        };
        let roles = BTreeMap::new();
        let out = block_on(hydrate_active_ruleset(
            &store,
            GuildId(7),
            &key(),
            &ResourceBindingMap::default(),
            &caps,
            &roles,
        ))
        .unwrap();
        assert_eq!(out.version, version);
    }
}
