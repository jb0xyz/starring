use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;
use serde::{Deserialize, Serialize};

use crate::{
    ActivationRequestId, BindingRevision, DeploymentId, InstallationId, ProcessInstanceId,
    PromotionId, RuntimeGeneration, TenantId,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeDeploymentIdentityV1 {
    pub deployment_id: DeploymentId,
    pub tenant_id: TenantId,
    pub installation_id: InstallationId,
    pub promotion_id: PromotionId,
    pub activation_request_id: ActivationRequestId,
}

impl RuntimeDeploymentIdentityV1 {
    pub fn same_product_scope(&self, other: &Self) -> bool {
        self.tenant_id == other.tenant_id && self.installation_id == other.installation_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeDeploymentTargetV1 {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub version: RuleSetVersionId,
    pub content_hash: RuleSetContentHash,
    pub binding_revision: BindingRevision,
    pub binding_fingerprint: ResourceBindingFingerprint,
}

impl RuntimeDeploymentTargetV1 {
    pub fn same_slot(&self, other: &Self) -> bool {
        self.guild_id == other.guild_id && self.ruleset_key == other.ruleset_key
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeProcessIdentityV1 {
    pub target: RuntimeDeploymentTargetV1,
    pub runtime_generation: RuntimeGeneration,
    pub process_instance_id: ProcessInstanceId,
}
