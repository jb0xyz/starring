use std::sync::Arc;

use automation_ruleset::{content_hash, RuleSetVersion};
use automation_runtime_convergence::{
    RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1, RuntimeProcessIdentityV1,
};
use discord_model::GuildId;
use resource_resolution::{resource_binding_fingerprint_v2, ResourceBindingMap};

use crate::ExactServingRouteError;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ServingSlotKeyV1 {
    guild_id: GuildId,
    ruleset_key: automation_ruleset::RuleSetKey,
}

impl ServingSlotKeyV1 {
    pub fn new(guild_id: GuildId, ruleset_key: automation_ruleset::RuleSetKey) -> Self {
        Self {
            guild_id,
            ruleset_key,
        }
    }

    pub fn from_target(target: &RuntimeDeploymentTargetV1) -> Self {
        Self::new(target.guild_id, target.ruleset_key.clone())
    }

    pub fn guild_id(&self) -> GuildId {
        self.guild_id
    }

    pub fn ruleset_key(&self) -> &automation_ruleset::RuleSetKey {
        &self.ruleset_key
    }

    pub fn matches_target(&self, target: &RuntimeDeploymentTargetV1) -> bool {
        self.guild_id == target.guild_id && self.ruleset_key == target.ruleset_key
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExactServingRouteV1 {
    deployment_identity: RuntimeDeploymentIdentityV1,
    process_identity: RuntimeProcessIdentityV1,
    ruleset: Arc<RuleSetVersion>,
    bindings: Arc<ResourceBindingMap>,
}

impl ExactServingRouteV1 {
    pub fn new(
        deployment_identity: RuntimeDeploymentIdentityV1,
        process_identity: RuntimeProcessIdentityV1,
        ruleset: RuleSetVersion,
        bindings: ResourceBindingMap,
    ) -> Result<Self, ExactServingRouteError> {
        let target = &process_identity.target;
        if ruleset.guild_id != target.guild_id || ruleset.ruleset_key != target.ruleset_key {
            return Err(ExactServingRouteError::RuleSetSlotMismatch);
        }
        if ruleset.version != target.version {
            return Err(ExactServingRouteError::RuleSetVersionMismatch);
        }
        if ruleset.content_hash != target.content_hash {
            return Err(ExactServingRouteError::RuleSetContentHashMismatch);
        }
        let actual_content_hash = content_hash(ruleset.schema_version, &ruleset.definition)
            .map_err(|_| ExactServingRouteError::RuleSetDefinitionHashMismatch)?;
        if actual_content_hash != target.content_hash {
            return Err(ExactServingRouteError::RuleSetDefinitionHashMismatch);
        }
        if resource_binding_fingerprint_v2(&bindings) != target.binding_fingerprint {
            return Err(ExactServingRouteError::BindingFingerprintMismatch);
        }
        Ok(Self {
            deployment_identity,
            process_identity,
            ruleset: Arc::new(ruleset),
            bindings: Arc::new(bindings),
        })
    }

    pub fn deployment_identity(&self) -> &RuntimeDeploymentIdentityV1 {
        &self.deployment_identity
    }

    pub fn identity(&self) -> &RuntimeProcessIdentityV1 {
        &self.process_identity
    }

    pub fn process_identity(&self) -> &RuntimeProcessIdentityV1 {
        &self.process_identity
    }

    pub fn ruleset(&self) -> &Arc<RuleSetVersion> {
        &self.ruleset
    }

    pub fn bindings(&self) -> &Arc<ResourceBindingMap> {
        &self.bindings
    }

    pub fn slot_key(&self) -> ServingSlotKeyV1 {
        ServingSlotKeyV1::from_target(&self.process_identity.target)
    }
}
