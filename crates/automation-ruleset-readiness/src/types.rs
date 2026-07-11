use std::collections::BTreeMap;

use automation_core::{PolicyFinding, ValidationError};
use automation_ruleset::{
    RuleSetHashError, RuleSetKey, RuleSetSchemaVersion, RuleSetStoreError, RuleSetVersion,
    RuleSetVersionId,
};
use automation_state::InteractionRuleSet;
use desired_state::ResourceKey;
use discord_model::{GuildId, Permissions, RoleId};
use resource_resolution::ResourceBindingMap;

#[derive(Debug)]
pub struct GuildCapabilities {
    pub base_permissions: Permissions,
}

impl GuildCapabilities {
    pub fn satisfies(&self, required: Permissions) -> bool {
        self.base_permissions.contains(Permissions::ADMINISTRATOR)
            || self.base_permissions.contains(required)
    }
}

pub struct RuleSetReadinessInput<'a> {
    pub artifact: &'a RuleSetVersion,
    pub bindings: &'a ResourceBindingMap,
    pub guild_capabilities: &'a GuildCapabilities,
    pub role_permissions: &'a BTreeMap<ResourceKey, Permissions>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeRuleSet {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub version: RuleSetVersionId,
    pub definition: InteractionRuleSet,
    pub notices: Vec<PolicyFinding>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicySeverity {
    Notice,
    Blocking,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadinessError {
    UnsupportedSchema(RuleSetSchemaVersion),
    StructurallyInvalid(Vec<ValidationError>),
    HashComputation(RuleSetHashError),
    HashMismatch,
    BindingInvalid(Vec<ValidationError>),
    BlockingPolicy(Vec<PolicyFinding>),
    MissingCapabilities { missing: Permissions },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadinessContextError {
    EveryoneRoleMissing,
    BoundRoleMissing { key: ResourceKey, role_id: RoleId },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HydrationError {
    NoActiveRuleSet,
    Store(RuleSetStoreError),
    NotReady(ReadinessError),
}
