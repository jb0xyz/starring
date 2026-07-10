use std::collections::BTreeMap;

use automation_state::{ActionSpec, InteractionRuleSet};
use desired_state::ResourceKey;
use discord_model::Permissions;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyFinding {
    pub rule: String,
    pub role: ResourceKey,
    pub reason: String,
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
            if let ActionSpec::GrantRole { role, .. } = action {
                if let Some(permissions) = roles.get(role) {
                    if permissions.intersects(mask) {
                        findings.push(PolicyFinding {
                            rule: rule.key.clone(),
                            role: role.clone(),
                            reason: "grants a privileged role".to_string(),
                        });
                    }
                }
            }
        }
    }
    findings
}
