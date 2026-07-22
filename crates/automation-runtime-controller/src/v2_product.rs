use automation_runtime_convergence::{DeploymentRevision, RuntimeDeploymentTargetV1};

use crate::{
    RuntimeDeploymentScopeV1, RuntimeProductOperationIdV2, RuntimeProductSemanticRequestDigestV2,
    RuntimeServingSlotV2,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProductMutationKindV2 {
    Apply,
    Supersede,
    Cancel,
    AuthorityChange,
    Teardown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeProductMutationPreimageV2 {
    pub operation_id: RuntimeProductOperationIdV2,
    pub scope: RuntimeDeploymentScopeV1,
    pub expected_revision: DeploymentRevision,
    pub slot: RuntimeServingSlotV2,
    pub expected_target: RuntimeDeploymentTargetV1,
    pub mutation_kind: RuntimeProductMutationKindV2,
    pub product_semantic_request_digest: RuntimeProductSemanticRequestDigestV2,
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

    use super::{RuntimeProductMutationKindV2, RuntimeProductMutationPreimageV2};
    use crate::{
        RuntimeDeploymentScopeV1, RuntimeProductOperationIdV2,
        RuntimeProductSemanticRequestDigestV2, RuntimeServingSlotV2,
    };

    fn scope() -> RuntimeDeploymentScopeV1 {
        RuntimeDeploymentScopeV1 {
            tenant_id: TenantId::parse("tenant:1").unwrap(),
            installation_id: InstallationId::parse("installation:1").unwrap(),
            deployment_id: DeploymentId::parse("deployment:1").unwrap(),
        }
    }

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

    fn preimage(mutation_kind: RuntimeProductMutationKindV2) -> RuntimeProductMutationPreimageV2 {
        let expected_target = target();
        RuntimeProductMutationPreimageV2 {
            operation_id: RuntimeProductOperationIdV2::parse("00112233445566778899aabbccddeeff")
                .unwrap(),
            scope: scope(),
            expected_revision: DeploymentRevision::new(11).unwrap(),
            slot: RuntimeServingSlotV2::from_target(&expected_target),
            expected_target,
            mutation_kind,
            product_semantic_request_digest: RuntimeProductSemanticRequestDigestV2::parse(
                "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            )
            .unwrap(),
        }
    }

    #[test]
    fn product_preimage_carries_the_exact_product_owned_inputs() {
        let preimage = preimage(RuntimeProductMutationKindV2::AuthorityChange);

        assert_eq!(
            preimage.operation_id.as_str(),
            "00112233445566778899aabbccddeeff"
        );
        assert_eq!(preimage.scope, scope());
        assert_eq!(
            preimage.expected_revision,
            DeploymentRevision::new(11).unwrap()
        );
        assert_eq!(preimage.slot, RuntimeServingSlotV2::from_target(&target()));
        assert_eq!(preimage.expected_target, target());
        assert_eq!(
            preimage.mutation_kind,
            RuntimeProductMutationKindV2::AuthorityChange
        );
        assert_eq!(
            preimage.product_semantic_request_digest.as_str(),
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
        );
    }

    #[test]
    fn product_mutation_kind_is_the_closed_five_variant_domain() {
        let variants = [
            RuntimeProductMutationKindV2::Apply,
            RuntimeProductMutationKindV2::Supersede,
            RuntimeProductMutationKindV2::Cancel,
            RuntimeProductMutationKindV2::AuthorityChange,
            RuntimeProductMutationKindV2::Teardown,
        ];

        for (index, variant) in variants.iter().enumerate() {
            assert_eq!(preimage(*variant).mutation_kind, *variant);
            assert!(variants[index + 1..].iter().all(|other| other != variant));
        }
    }
}
