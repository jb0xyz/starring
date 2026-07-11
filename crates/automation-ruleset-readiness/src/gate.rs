use automation_core::{analyze, validate_bindings, validate_structural, PolicyFinding};
use automation_ruleset::{content_hash, RuleSetVersion, CURRENT_RULESET_SCHEMA_VERSION};
use automation_state::{ActionSpec, InteractionRuleSet};
use discord_model::Permissions;

use crate::types::{PolicySeverity, ReadinessError, RuleSetReadinessInput, RuntimeRuleSet};

pub fn required_capabilities(ruleset: &InteractionRuleSet) -> Permissions {
    let mut required = Permissions::empty();
    for rule in &ruleset.rules {
        for action in &rule.actions {
            match action {
                ActionSpec::CreateRole { .. }
                | ActionSpec::GrantRole { .. }
                | ActionSpec::UpsertOverwrite { .. } => required |= Permissions::MANAGE_ROLES,
                ActionSpec::CreateChannel { .. } => required |= Permissions::MANAGE_CHANNELS,
                _ => {}
            }
        }
    }
    required
}

pub fn policy_severity(finding: &PolicyFinding) -> PolicySeverity {
    match finding {
        PolicyFinding::DynamicResourceCreation { .. }
        | PolicyFinding::CreatedResourceReference { .. }
        | PolicyFinding::EveryoneOverwrite { .. }
        | PolicyFinding::RuntimeMessagePost { .. }
        | PolicyFinding::RuntimeInteractivePanel { .. } => PolicySeverity::Notice,
        _ => PolicySeverity::Blocking,
    }
}

pub fn check_readiness(input: RuleSetReadinessInput) -> Result<RuntimeRuleSet, ReadinessError> {
    let artifact: &RuleSetVersion = input.artifact;
    if artifact.schema_version != CURRENT_RULESET_SCHEMA_VERSION {
        return Err(ReadinessError::UnsupportedSchema(artifact.schema_version));
    }
    validate_structural(&artifact.definition).map_err(ReadinessError::StructurallyInvalid)?;
    let recomputed = content_hash(artifact.schema_version, &artifact.definition)
        .map_err(ReadinessError::HashComputation)?;
    if recomputed != artifact.content_hash {
        return Err(ReadinessError::HashMismatch);
    }
    validate_bindings(&artifact.definition, input.bindings)
        .map_err(ReadinessError::BindingInvalid)?;
    let findings = analyze(&artifact.definition, input.role_permissions);
    let blocking: Vec<PolicyFinding> = findings
        .iter()
        .filter(|f| policy_severity(f) == PolicySeverity::Blocking)
        .cloned()
        .collect();
    if !blocking.is_empty() {
        return Err(ReadinessError::BlockingPolicy(blocking));
    }
    let notices: Vec<PolicyFinding> = findings
        .into_iter()
        .filter(|f| policy_severity(f) == PolicySeverity::Notice)
        .collect();
    let required = required_capabilities(&artifact.definition);
    if !input.guild_capabilities.satisfies(required) {
        let missing = required & !input.guild_capabilities.base_permissions;
        return Err(ReadinessError::MissingCapabilities { missing });
    }
    Ok(RuntimeRuleSet {
        guild_id: artifact.guild_id,
        ruleset_key: artifact.ruleset_key.clone(),
        version: artifact.version,
        definition: artifact.definition.clone(),
        notices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::GuildCapabilities;
    use automation_ruleset::RuleSetKey;
    use automation_ruleset::RuleSetVersionId;
    use automation_ruleset::{content_hash as ch, RuleSetContentHash};
    use automation_state::{
        ActionTarget, ChannelRef, CreatedRef, InteractionRule, OverwriteTargetSpec, RoleRef,
        TriggerSpec,
    };
    use desired_state::ResourceKey;
    use discord_model::{GuildId, RoleId, UserId};
    use resource_resolution::ResourceBindingMap;
    use std::collections::BTreeMap;

    fn ruleset(actions: Vec<ActionSpec>) -> InteractionRuleSet {
        InteractionRuleSet {
            version: 1,
            panels: vec![],
            modals: vec![],
            rules: vec![InteractionRule {
                key: "r".to_string(),
                trigger: TriggerSpec::InstanceAction {
                    action: "test".to_string(),
                },
                actions,
            }],
        }
    }

    fn artifact(def: InteractionRuleSet) -> RuleSetVersion {
        let hash = ch(CURRENT_RULESET_SCHEMA_VERSION, &def).unwrap();
        RuleSetVersion {
            guild_id: GuildId(7),
            ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
            version: RuleSetVersionId::FIRST,
            schema_version: CURRENT_RULESET_SCHEMA_VERSION,
            definition: def,
            content_hash: hash,
            created_by: UserId(1),
        }
    }

    fn admin() -> GuildCapabilities {
        GuildCapabilities {
            base_permissions: Permissions::ADMINISTRATOR,
        }
    }

    fn input<'a>(
        art: &'a RuleSetVersion,
        binds: &'a ResourceBindingMap,
        caps: &'a GuildCapabilities,
        roles: &'a BTreeMap<ResourceKey, Permissions>,
    ) -> RuleSetReadinessInput<'a> {
        RuleSetReadinessInput {
            artifact: art,
            bindings: binds,
            guild_capabilities: caps,
            role_permissions: roles,
        }
    }

    fn create_role() -> ActionSpec {
        ActionSpec::CreateRole {
            key: "role".to_string(),
            name: "n".to_string(),
        }
    }

    #[test]
    fn required_capabilities_maps_actions() {
        let rs = ruleset(vec![
            create_role(),
            ActionSpec::CreateChannel {
                key: "ch".to_string(),
                name: "n".to_string(),
            },
        ]);
        assert_eq!(
            required_capabilities(&rs),
            Permissions::MANAGE_ROLES | Permissions::MANAGE_CHANNELS
        );
        assert!(!required_capabilities(&ruleset(vec![ActionSpec::PostPanel {
            key: "p".to_string(),
            channel: ChannelRef::Created(CreatedRef {
                created: "ch".to_string(),
            }),
            content: "c".to_string(),
            buttons: vec![],
        }]))
        .contains(Permissions::SEND_MESSAGES));
    }

    #[test]
    fn studyroom_shape_passes_all_notice() {
        let def = ruleset(vec![
            create_role(),
            ActionSpec::CreateChannel {
                key: "ch".to_string(),
                name: "n".to_string(),
            },
            ActionSpec::UpsertOverwrite {
                channel: ChannelRef::Created(CreatedRef {
                    created: "ch".to_string(),
                }),
                target: OverwriteTargetSpec::Everyone,
                allow: Permissions::empty(),
                deny: Permissions::VIEW_CHANNEL,
            },
            ActionSpec::GrantRole {
                role: RoleRef::Created(CreatedRef {
                    created: "role".to_string(),
                }),
                target: ActionTarget::Actor,
            },
        ]);
        let art = artifact(def);
        let binds = ResourceBindingMap::default();
        let roles = BTreeMap::new();
        let out = check_readiness(input(&art, &binds, &admin(), &roles)).unwrap();
        assert_eq!(out.version, RuleSetVersionId::FIRST);
        assert!(!out.notices.is_empty());
    }

    #[test]
    fn hash_mismatch_blocked() {
        let mut art = artifact(ruleset(vec![create_role()]));
        art.content_hash = RuleSetContentHash::parse_hex(&"00".repeat(32)).unwrap();
        let binds = ResourceBindingMap::default();
        let roles = BTreeMap::new();
        assert_eq!(
            check_readiness(input(&art, &binds, &admin(), &roles)).unwrap_err(),
            ReadinessError::HashMismatch
        );
    }

    #[test]
    fn privileged_grant_blocked() {
        let art = artifact(ruleset(vec![ActionSpec::GrantRole {
            role: RoleRef::Existing(ResourceKey("admin".to_string())),
            target: ActionTarget::Actor,
        }]));
        let mut roles = BTreeMap::new();
        roles.insert(ResourceKey("admin".to_string()), Permissions::ADMINISTRATOR);
        let mut binds = ResourceBindingMap::default();
        binds
            .role_bindings
            .insert(ResourceKey("admin".to_string()), RoleId(10));
        assert!(matches!(
            check_readiness(input(&art, &binds, &admin(), &roles)).unwrap_err(),
            ReadinessError::BlockingPolicy(_)
        ));
    }

    #[test]
    fn missing_capability_blocked() {
        let art = artifact(ruleset(vec![create_role()]));
        let caps = GuildCapabilities {
            base_permissions: Permissions::SEND_MESSAGES,
        };
        let binds = ResourceBindingMap::default();
        let roles = BTreeMap::new();
        assert!(matches!(
            check_readiness(input(&art, &binds, &caps, &roles)).unwrap_err(),
            ReadinessError::MissingCapabilities { .. }
        ));
    }
}
