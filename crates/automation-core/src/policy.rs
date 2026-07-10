use std::collections::BTreeMap;

use automation_state::{ActionSpec, InteractionRuleSet, RoleRef};
use desired_state::ResourceKey;
use discord_model::Permissions;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyFinding {
    PrivilegedRoleGrant { rule: String, role: ResourceKey },
    DynamicResourceCreation { rule: String, action: DynamicAction },
    CreatedResourceReference { rule: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DynamicAction {
    CreateChannel,
    CreateRole,
}

pub fn privileged_mask() -> Permissions {
    Permissions::ADMINISTRATOR
        | Permissions::MANAGE_GUILD
        | Permissions::MANAGE_ROLES
        | Permissions::MANAGE_CHANNELS
        | Permissions::BAN_MEMBERS
        | Permissions::KICK_MEMBERS
        | Permissions::MODERATE_MEMBERS
}

pub fn analyze(
    ruleset: &InteractionRuleSet,
    roles: &BTreeMap<ResourceKey, Permissions>,
) -> Vec<PolicyFinding> {
    let mask = privileged_mask();
    let mut findings = Vec::new();
    for rule in &ruleset.rules {
        for action in &rule.actions {
            match action {
                ActionSpec::GrantRole { role, .. } => match role {
                    RoleRef::Existing(key) => {
                        if roles.get(key).is_some_and(|perms| perms.intersects(mask)) {
                            findings.push(PolicyFinding::PrivilegedRoleGrant {
                                rule: rule.key.clone(),
                                role: key.clone(),
                            });
                        }
                    }
                    RoleRef::Created { .. } => {
                        findings.push(PolicyFinding::CreatedResourceReference {
                            rule: rule.key.clone(),
                        });
                    }
                },
                ActionSpec::CreateChannel { .. } => {
                    findings.push(PolicyFinding::DynamicResourceCreation {
                        rule: rule.key.clone(),
                        action: DynamicAction::CreateChannel,
                    });
                }
                ActionSpec::CreateRole { .. } => {
                    findings.push(PolicyFinding::DynamicResourceCreation {
                        rule: rule.key.clone(),
                        action: DynamicAction::CreateRole,
                    });
                }
                ActionSpec::RespondEphemeral { .. } | ActionSpec::OpenModal { .. } => {}
            }
        }
    }
    findings
}
