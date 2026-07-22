use automation_runtime_convergence::{DeploymentRevision, RuntimeDeploymentTargetV1};

use crate::{
    RuntimeDeploymentScopeV1, RuntimeDrainIntentIdV2, RuntimeProductMutationDigestV2,
    RuntimeProductMutationKindV2, RuntimeProductOperationIdV2, RuntimeServingSlotV2,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDrainIntentKeyV2 {
    pub intent_id: RuntimeDrainIntentIdV2,
    pub product_operation_id: RuntimeProductOperationIdV2,
    pub product_mutation_digest: RuntimeProductMutationDigestV2,
    pub scope: RuntimeDeploymentScopeV1,
    pub expected_revision: DeploymentRevision,
    pub slot: RuntimeServingSlotV2,
    pub expected_target: RuntimeDeploymentTargetV1,
    pub mutation_kind: RuntimeProductMutationKindV2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDrainIntentPreimageV2 {
    pub key: RuntimeDrainIntentKeyV2,
}

impl RuntimeDrainIntentPreimageV2 {
    pub fn from_key(key: RuntimeDrainIntentKeyV2) -> Self {
        Self { key }
    }
}

#[cfg(test)]
mod tests {
    use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
    use automation_runtime_convergence::{
        BindingRevision, DeploymentId, DeploymentRevision, InstallationId,
        RuntimeDeploymentTargetV1, TenantId,
    };
    use discord_model::GuildId;
    use resource_resolution::ResourceBindingFingerprint;

    use super::{RuntimeDrainIntentKeyV2, RuntimeDrainIntentPreimageV2};
    use crate::{
        RuntimeDeploymentScopeV1, RuntimeDrainIntentIdV2, RuntimeProductMutationDigestV2,
        RuntimeProductMutationKindV2, RuntimeProductOperationIdV2, RuntimeServingSlotV2,
    };

    fn target() -> RuntimeDeploymentTargetV1 {
        RuntimeDeploymentTargetV1 {
            guild_id: GuildId(7),
            ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
            version: RuleSetVersionId::FIRST,
            content_hash: RuleSetContentHash::parse_hex(&"b".repeat(64)).unwrap(),
            binding_revision: BindingRevision::new(3).unwrap(),
            binding_fingerprint: ResourceBindingFingerprint::parse(&"a".repeat(64)).unwrap(),
        }
    }

    fn key() -> RuntimeDrainIntentKeyV2 {
        let expected_target = target();
        RuntimeDrainIntentKeyV2 {
            intent_id: RuntimeDrainIntentIdV2::parse("ffeeddccbbaa99887766554433221100").unwrap(),
            product_operation_id: RuntimeProductOperationIdV2::parse(
                "00112233445566778899aabbccddeeff",
            )
            .unwrap(),
            product_mutation_digest: RuntimeProductMutationDigestV2::parse(
                "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
            )
            .unwrap(),
            scope: RuntimeDeploymentScopeV1 {
                tenant_id: TenantId::parse("tenant:1").unwrap(),
                installation_id: InstallationId::parse("installation:1").unwrap(),
                deployment_id: DeploymentId::parse("deployment:1").unwrap(),
            },
            expected_revision: DeploymentRevision::new(11).unwrap(),
            slot: RuntimeServingSlotV2::from_target(&expected_target),
            expected_target,
            mutation_kind: RuntimeProductMutationKindV2::Teardown,
        }
    }

    #[test]
    fn drain_preimage_contains_only_the_exact_drain_key() {
        let key = key();
        let preimage = RuntimeDrainIntentPreimageV2::from_key(key.clone());

        assert_eq!(preimage.key, key);
    }

    #[test]
    fn drain_key_carries_the_product_correlation_without_recomputing_it() {
        let key = key();

        assert_eq!(key.intent_id.as_str(), "ffeeddccbbaa99887766554433221100");
        assert_eq!(
            key.product_operation_id.as_str(),
            "00112233445566778899aabbccddeeff"
        );
        assert_eq!(
            key.product_mutation_digest.as_str(),
            "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
        );
        assert_eq!(key.expected_revision, DeploymentRevision::new(11).unwrap());
        assert_eq!(key.slot, RuntimeServingSlotV2::from_target(&target()));
        assert_eq!(key.expected_target, target());
        assert_eq!(key.mutation_kind, RuntimeProductMutationKindV2::Teardown);
    }
}
