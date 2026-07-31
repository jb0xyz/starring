use std::num::NonZeroU64;

use automation_instance::InstanceId;
use automation_ruleset::{RuleSetContentHash, RuleSetVersionId};
use automation_runtime_convergence::{
    DeploymentId, FencingToken, InstallationId, RuntimeDeploymentIdentityV1,
    RuntimeProcessIdentityV1, TenantId,
};

use crate::{
    InteractionActionPlanDigestV1, InteractionInstanceManifestDigestV1, InteractionRequestDigestV1,
    InteractionRouteAttestationDigestV1,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DiscordInteractionIdentityErrorV1 {
    #[error("Discord interaction identity must be non-zero")]
    Zero,
}

macro_rules! define_discord_identity {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(NonZeroU64);

        impl $name {
            pub fn new(value: u64) -> Result<Self, DiscordInteractionIdentityErrorV1> {
                NonZeroU64::new(value)
                    .map(Self)
                    .ok_or(DiscordInteractionIdentityErrorV1::Zero)
            }

            pub fn get(self) -> u64 {
                self.0.get()
            }
        }
    };
}

define_discord_identity!(DiscordApplicationIdV1);
define_discord_identity!(DiscordInteractionIdV1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InteractionReceiptIdentityV1 {
    application_id: DiscordApplicationIdV1,
    interaction_id: DiscordInteractionIdV1,
}

impl InteractionReceiptIdentityV1 {
    pub fn new(
        application_id: DiscordApplicationIdV1,
        interaction_id: DiscordInteractionIdV1,
    ) -> Self {
        Self {
            application_id,
            interaction_id,
        }
    }

    pub fn application_id(self) -> DiscordApplicationIdV1 {
        self.application_id
    }

    pub fn interaction_id(self) -> DiscordInteractionIdV1 {
        self.interaction_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionProductScopeV1 {
    tenant_id: TenantId,
    installation_id: InstallationId,
    deployment_id: DeploymentId,
}

impl InteractionProductScopeV1 {
    pub fn new(
        tenant_id: TenantId,
        installation_id: InstallationId,
        deployment_id: DeploymentId,
    ) -> Self {
        Self {
            tenant_id,
            installation_id,
            deployment_id,
        }
    }

    pub fn from_deployment_identity(identity: &RuntimeDeploymentIdentityV1) -> Self {
        Self {
            tenant_id: identity.tenant_id.clone(),
            installation_id: identity.installation_id.clone(),
            deployment_id: identity.deployment_id.clone(),
        }
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn installation_id(&self) -> &InstallationId {
        &self.installation_id
    }

    pub fn deployment_id(&self) -> &DeploymentId {
        &self.deployment_id
    }
}

macro_rules! define_nonzero_revision {
    ($name:ident, $variant:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(NonZeroU64);

        impl $name {
            pub fn new(value: u64) -> Result<Self, InteractionRouteBindingErrorV1> {
                NonZeroU64::new(value)
                    .map(Self)
                    .ok_or(InteractionRouteBindingErrorV1::$variant)
            }

            pub fn get(self) -> u64 {
                self.0.get()
            }
        }
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InteractionRouteBindingErrorV1 {
    #[error("interaction serving lease epoch must be non-zero")]
    ServingLeaseEpoch,
    #[error("interaction serving lease revision must be non-zero")]
    ServingLeaseRevision,
    #[error("interaction route guild identity must be non-zero")]
    GuildIdentity,
    #[error("interaction route incarnation must be non-zero")]
    RouteIncarnation,
    #[error("interaction gateway shard identity is invalid")]
    GatewayShardIdentity,
    #[error("interaction gateway owner lease epoch must be non-zero")]
    GatewayOwnerLeaseEpoch,
    #[error("interaction gateway owner revision must be non-zero")]
    GatewayOwnerRevision,
    #[error("interaction runtime build revision is invalid")]
    RuntimeBuildRevision,
}

define_nonzero_revision!(InteractionServingLeaseEpochV1, ServingLeaseEpoch);
define_nonzero_revision!(InteractionServingLeaseRevisionV1, ServingLeaseRevision);
define_nonzero_revision!(InteractionRouteIncarnationV1, RouteIncarnation);
define_nonzero_revision!(InteractionGatewayOwnerLeaseEpochV1, GatewayOwnerLeaseEpoch);
define_nonzero_revision!(InteractionGatewayOwnerRevisionV1, GatewayOwnerRevision);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InteractionGatewayShardIdentityV1(String);

impl InteractionGatewayShardIdentityV1 {
    pub fn parse(value: impl Into<String>) -> Result<Self, InteractionRouteBindingErrorV1> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':' | b'/')
            })
        {
            return Err(InteractionRouteBindingErrorV1::GatewayShardIdentity);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InteractionRuntimeBuildRevisionV1(String);

impl InteractionRuntimeBuildRevisionV1 {
    pub fn parse(value: impl Into<String>) -> Result<Self, InteractionRouteBindingErrorV1> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':' | b'/')
            })
        {
            return Err(InteractionRouteBindingErrorV1::RuntimeBuildRevision);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionGatewayOwnerIdentityV1 {
    shard_identity: InteractionGatewayShardIdentityV1,
    lease_epoch: InteractionGatewayOwnerLeaseEpochV1,
    owner_revision: InteractionGatewayOwnerRevisionV1,
    runtime_build_revision: InteractionRuntimeBuildRevisionV1,
}

impl InteractionGatewayOwnerIdentityV1 {
    pub fn new(
        shard_identity: InteractionGatewayShardIdentityV1,
        lease_epoch: InteractionGatewayOwnerLeaseEpochV1,
        owner_revision: InteractionGatewayOwnerRevisionV1,
        runtime_build_revision: InteractionRuntimeBuildRevisionV1,
    ) -> Self {
        Self {
            shard_identity,
            lease_epoch,
            owner_revision,
            runtime_build_revision,
        }
    }

    pub fn shard_identity(&self) -> &InteractionGatewayShardIdentityV1 {
        &self.shard_identity
    }

    pub fn lease_epoch(&self) -> InteractionGatewayOwnerLeaseEpochV1 {
        self.lease_epoch
    }

    pub fn owner_revision(&self) -> InteractionGatewayOwnerRevisionV1 {
        self.owner_revision
    }

    pub fn runtime_build_revision(&self) -> &InteractionRuntimeBuildRevisionV1 {
        &self.runtime_build_revision
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionServingRouteIdentityV1 {
    attestation_digest: InteractionRouteAttestationDigestV1,
    lease_epoch: InteractionServingLeaseEpochV1,
    lease_revision: InteractionServingLeaseRevisionV1,
    gateway_owner_identity: InteractionGatewayOwnerIdentityV1,
    route_fencing_token: FencingToken,
    route_incarnation: InteractionRouteIncarnationV1,
}

impl InteractionServingRouteIdentityV1 {
    pub fn new(
        attestation_digest: InteractionRouteAttestationDigestV1,
        lease_epoch: InteractionServingLeaseEpochV1,
        lease_revision: InteractionServingLeaseRevisionV1,
        gateway_owner_identity: InteractionGatewayOwnerIdentityV1,
        route_fencing_token: FencingToken,
        route_incarnation: InteractionRouteIncarnationV1,
    ) -> Self {
        Self {
            attestation_digest,
            lease_epoch,
            lease_revision,
            gateway_owner_identity,
            route_fencing_token,
            route_incarnation,
        }
    }

    pub fn attestation_digest(&self) -> &InteractionRouteAttestationDigestV1 {
        &self.attestation_digest
    }

    pub fn lease_epoch(&self) -> InteractionServingLeaseEpochV1 {
        self.lease_epoch
    }

    pub fn lease_revision(&self) -> InteractionServingLeaseRevisionV1 {
        self.lease_revision
    }

    pub fn gateway_shard_identity(&self) -> &InteractionGatewayShardIdentityV1 {
        self.gateway_owner_identity.shard_identity()
    }

    pub fn gateway_owner_lease_epoch(&self) -> InteractionGatewayOwnerLeaseEpochV1 {
        self.gateway_owner_identity.lease_epoch()
    }

    pub fn gateway_owner_revision(&self) -> InteractionGatewayOwnerRevisionV1 {
        self.gateway_owner_identity.owner_revision()
    }

    pub fn runtime_build_revision(&self) -> &InteractionRuntimeBuildRevisionV1 {
        self.gateway_owner_identity.runtime_build_revision()
    }

    pub fn gateway_owner_identity(&self) -> &InteractionGatewayOwnerIdentityV1 {
        &self.gateway_owner_identity
    }

    pub fn route_fencing_token(&self) -> FencingToken {
        self.route_fencing_token
    }

    pub fn route_incarnation(&self) -> InteractionRouteIncarnationV1 {
        self.route_incarnation
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractionExecutionRouteV1 {
    Static {
        ruleset_version: RuleSetVersionId,
        ruleset_content_hash: RuleSetContentHash,
    },
    Instance {
        instance_id: InstanceId,
        pinned_ruleset_version: RuleSetVersionId,
        pinned_ruleset_content_hash: RuleSetContentHash,
        resource_manifest_digest: InteractionInstanceManifestDigestV1,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionRouteBindingV1 {
    scope: InteractionProductScopeV1,
    process_identity: RuntimeProcessIdentityV1,
    serving_identity: InteractionServingRouteIdentityV1,
    execution_route: InteractionExecutionRouteV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionExpectedRouteV1 {
    scope: InteractionProductScopeV1,
    process_identity: RuntimeProcessIdentityV1,
    gateway_shard_identity: InteractionGatewayShardIdentityV1,
    runtime_build_revision: InteractionRuntimeBuildRevisionV1,
    route_fencing_token: FencingToken,
    route_incarnation: InteractionRouteIncarnationV1,
}

impl InteractionExpectedRouteV1 {
    pub fn new(
        scope: InteractionProductScopeV1,
        process_identity: RuntimeProcessIdentityV1,
        gateway_shard_identity: InteractionGatewayShardIdentityV1,
        runtime_build_revision: InteractionRuntimeBuildRevisionV1,
        route_fencing_token: FencingToken,
        route_incarnation: InteractionRouteIncarnationV1,
    ) -> Result<Self, InteractionRouteBindingErrorV1> {
        validate_process_identity(&process_identity)?;
        Ok(Self {
            scope,
            process_identity,
            gateway_shard_identity,
            runtime_build_revision,
            route_fencing_token,
            route_incarnation,
        })
    }

    pub fn from_authoritative(route: &InteractionRouteBindingV1) -> Self {
        Self {
            scope: route.scope.clone(),
            process_identity: route.process_identity.clone(),
            gateway_shard_identity: route
                .serving_identity
                .gateway_owner_identity
                .shard_identity
                .clone(),
            runtime_build_revision: route
                .serving_identity
                .gateway_owner_identity
                .runtime_build_revision
                .clone(),
            route_fencing_token: route.serving_identity.route_fencing_token,
            route_incarnation: route.serving_identity.route_incarnation,
        }
    }

    pub fn scope(&self) -> &InteractionProductScopeV1 {
        &self.scope
    }

    pub fn process_identity(&self) -> &RuntimeProcessIdentityV1 {
        &self.process_identity
    }

    pub fn gateway_shard_identity(&self) -> &InteractionGatewayShardIdentityV1 {
        &self.gateway_shard_identity
    }

    pub fn runtime_build_revision(&self) -> &InteractionRuntimeBuildRevisionV1 {
        &self.runtime_build_revision
    }

    pub fn route_fencing_token(&self) -> FencingToken {
        self.route_fencing_token
    }

    pub fn route_incarnation(&self) -> InteractionRouteIncarnationV1 {
        self.route_incarnation
    }
}

impl InteractionRouteBindingV1 {
    pub fn new_static(
        scope: InteractionProductScopeV1,
        process_identity: RuntimeProcessIdentityV1,
        serving_identity: InteractionServingRouteIdentityV1,
    ) -> Result<Self, InteractionRouteBindingErrorV1> {
        validate_process_identity(&process_identity)?;
        let execution_route = InteractionExecutionRouteV1::Static {
            ruleset_version: process_identity.target.version,
            ruleset_content_hash: process_identity.target.content_hash,
        };
        Ok(Self {
            scope,
            process_identity,
            serving_identity,
            execution_route,
        })
    }

    pub fn new_instance(
        scope: InteractionProductScopeV1,
        process_identity: RuntimeProcessIdentityV1,
        serving_identity: InteractionServingRouteIdentityV1,
        instance_id: InstanceId,
        pinned_ruleset_version: RuleSetVersionId,
        pinned_ruleset_content_hash: RuleSetContentHash,
        resource_manifest_digest: InteractionInstanceManifestDigestV1,
    ) -> Result<Self, InteractionRouteBindingErrorV1> {
        validate_process_identity(&process_identity)?;
        Ok(Self {
            scope,
            process_identity,
            serving_identity,
            execution_route: InteractionExecutionRouteV1::Instance {
                instance_id,
                pinned_ruleset_version,
                pinned_ruleset_content_hash,
                resource_manifest_digest,
            },
        })
    }

    pub fn scope(&self) -> &InteractionProductScopeV1 {
        &self.scope
    }

    pub fn process_identity(&self) -> &RuntimeProcessIdentityV1 {
        &self.process_identity
    }

    pub fn serving_identity(&self) -> &InteractionServingRouteIdentityV1 {
        &self.serving_identity
    }

    pub fn execution_route(&self) -> &InteractionExecutionRouteV1 {
        &self.execution_route
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionReceiptContractV1 {
    claim_root: InteractionReceiptClaimRootV1,
    action_plan_digest: InteractionActionPlanDigestV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionReceiptClaimCandidateV1 {
    identity: InteractionReceiptIdentityV1,
    expected_route: InteractionExpectedRouteV1,
    request_digest: InteractionRequestDigestV1,
}

impl InteractionReceiptClaimCandidateV1 {
    pub fn new(
        identity: InteractionReceiptIdentityV1,
        expected_route: InteractionExpectedRouteV1,
        request_digest: InteractionRequestDigestV1,
    ) -> Self {
        Self {
            identity,
            expected_route,
            request_digest,
        }
    }

    pub fn identity(&self) -> InteractionReceiptIdentityV1 {
        self.identity
    }

    pub fn expected_route(&self) -> &InteractionExpectedRouteV1 {
        &self.expected_route
    }

    pub fn request_digest(&self) -> &InteractionRequestDigestV1 {
        &self.request_digest
    }

    pub fn bind_authoritative(
        self,
        route: InteractionRouteBindingV1,
    ) -> Result<InteractionReceiptClaimRootV1, InteractionReceiptBindingErrorV1> {
        if self.expected_route.scope != route.scope
            || self.expected_route.process_identity != route.process_identity
            || self.expected_route.gateway_shard_identity
                != route.serving_identity.gateway_owner_identity.shard_identity
            || self.expected_route.runtime_build_revision
                != route
                    .serving_identity
                    .gateway_owner_identity
                    .runtime_build_revision
            || self.expected_route.route_fencing_token != route.serving_identity.route_fencing_token
            || self.expected_route.route_incarnation != route.serving_identity.route_incarnation
        {
            return Err(InteractionReceiptBindingErrorV1::ExpectedRouteMismatch);
        }
        Ok(InteractionReceiptClaimRootV1 {
            identity: self.identity,
            route,
            request_digest: self.request_digest,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionReceiptClaimRootV1 {
    identity: InteractionReceiptIdentityV1,
    route: InteractionRouteBindingV1,
    request_digest: InteractionRequestDigestV1,
}

impl InteractionReceiptClaimRootV1 {
    pub fn identity(&self) -> InteractionReceiptIdentityV1 {
        self.identity
    }

    pub fn route(&self) -> &InteractionRouteBindingV1 {
        &self.route
    }

    pub fn request_digest(&self) -> &InteractionRequestDigestV1 {
        &self.request_digest
    }

    pub fn bind_action_plan(
        self,
        action_plan_digest: InteractionActionPlanDigestV1,
    ) -> InteractionReceiptContractV1 {
        InteractionReceiptContractV1 {
            claim_root: self,
            action_plan_digest,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InteractionReceiptBindingErrorV1 {
    #[error("authoritative interaction route does not match the claimed expected route")]
    ExpectedRouteMismatch,
}

impl InteractionReceiptContractV1 {
    pub fn claim_root(&self) -> &InteractionReceiptClaimRootV1 {
        &self.claim_root
    }

    pub fn identity(&self) -> InteractionReceiptIdentityV1 {
        self.claim_root.identity()
    }

    pub fn route(&self) -> &InteractionRouteBindingV1 {
        self.claim_root.route()
    }

    pub fn request_digest(&self) -> &InteractionRequestDigestV1 {
        self.claim_root.request_digest()
    }

    pub fn action_plan_digest(&self) -> &InteractionActionPlanDigestV1 {
        &self.action_plan_digest
    }
}

fn validate_process_identity(
    process_identity: &RuntimeProcessIdentityV1,
) -> Result<(), InteractionRouteBindingErrorV1> {
    if process_identity.target.guild_id.0 == 0 {
        return Err(InteractionRouteBindingErrorV1::GuildIdentity);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        static_route, static_route_with_build_revision, static_route_with_gateway_owner,
    };

    fn receipt_identity() -> InteractionReceiptIdentityV1 {
        InteractionReceiptIdentityV1::new(
            DiscordApplicationIdV1::new(1).unwrap(),
            DiscordInteractionIdV1::new(2).unwrap(),
        )
    }

    #[test]
    fn candidate_binds_authoritative_root_before_action_plan_contract() {
        let route = static_route(1);
        let expected_route = InteractionExpectedRouteV1::from_authoritative(&route);
        let candidate = InteractionReceiptClaimCandidateV1::new(
            receipt_identity(),
            expected_route,
            InteractionRequestDigestV1::parse("a".repeat(64)).unwrap(),
        );
        let root = candidate.bind_authoritative(route.clone()).unwrap();
        assert_eq!(root.identity(), receipt_identity());
        assert_eq!(root.route(), &route);
        let contract =
            root.bind_action_plan(InteractionActionPlanDigestV1::parse("b".repeat(64)).unwrap());
        assert_eq!(contract.identity(), receipt_identity());
        assert_eq!(contract.route(), &route);
        assert_eq!(contract.request_digest().as_str(), "a".repeat(64));
    }

    #[test]
    fn authoritative_route_mismatch_fails_closed() {
        let expected = static_route(1);
        let candidate = InteractionReceiptClaimCandidateV1::new(
            receipt_identity(),
            InteractionExpectedRouteV1::from_authoritative(&expected),
            InteractionRequestDigestV1::parse("a".repeat(64)).unwrap(),
        );
        assert_eq!(
            candidate.bind_authoritative(static_route(2)),
            Err(InteractionReceiptBindingErrorV1::ExpectedRouteMismatch)
        );

        let expected = static_route(1);
        let candidate = InteractionReceiptClaimCandidateV1::new(
            receipt_identity(),
            InteractionExpectedRouteV1::from_authoritative(&expected),
            InteractionRequestDigestV1::parse("a".repeat(64)).unwrap(),
        );
        let mut mismatched_shard = expected;
        mismatched_shard
            .serving_identity
            .gateway_owner_identity
            .shard_identity =
            InteractionGatewayShardIdentityV1::parse("gateway-shard-other").unwrap();
        assert_eq!(
            candidate.bind_authoritative(mismatched_shard),
            Err(InteractionReceiptBindingErrorV1::ExpectedRouteMismatch)
        );

        let expected = static_route(1);
        let candidate = InteractionReceiptClaimCandidateV1::new(
            receipt_identity(),
            InteractionExpectedRouteV1::from_authoritative(&expected),
            InteractionRequestDigestV1::parse("a".repeat(64)).unwrap(),
        );
        assert_eq!(
            candidate.bind_authoritative(static_route_with_build_revision(1, 2)),
            Err(InteractionReceiptBindingErrorV1::ExpectedRouteMismatch)
        );
    }

    #[test]
    fn gateway_shard_identity_is_bounded_and_canonical() {
        assert_eq!(
            InteractionGatewayShardIdentityV1::parse(""),
            Err(InteractionRouteBindingErrorV1::GatewayShardIdentity)
        );
        assert_eq!(
            InteractionGatewayShardIdentityV1::parse("gateway shard"),
            Err(InteractionRouteBindingErrorV1::GatewayShardIdentity)
        );
        assert_eq!(
            InteractionGatewayShardIdentityV1::parse("a".repeat(129)),
            Err(InteractionRouteBindingErrorV1::GatewayShardIdentity)
        );
        assert_eq!(
            InteractionGatewayShardIdentityV1::parse("gateway:shard/0")
                .unwrap()
                .as_str(),
            "gateway:shard/0"
        );
    }

    #[test]
    fn runtime_build_revision_is_bounded_and_canonical() {
        assert_eq!(
            InteractionRuntimeBuildRevisionV1::parse(""),
            Err(InteractionRouteBindingErrorV1::RuntimeBuildRevision)
        );
        assert_eq!(
            InteractionRuntimeBuildRevisionV1::parse("build revision"),
            Err(InteractionRouteBindingErrorV1::RuntimeBuildRevision)
        );
        assert_eq!(
            InteractionRuntimeBuildRevisionV1::parse("a".repeat(129)),
            Err(InteractionRouteBindingErrorV1::RuntimeBuildRevision)
        );
        assert_eq!(
            InteractionRuntimeBuildRevisionV1::parse("build:release/1")
                .unwrap()
                .as_str(),
            "build:release/1"
        );
    }

    #[test]
    fn authoritative_contract_preserves_database_derived_gateway_owner_fence() {
        let expected = static_route_with_gateway_owner(1, 1);
        let authoritative = static_route_with_gateway_owner(1, 2);
        let candidate = InteractionReceiptClaimCandidateV1::new(
            receipt_identity(),
            InteractionExpectedRouteV1::from_authoritative(&expected),
            InteractionRequestDigestV1::parse("a".repeat(64)).unwrap(),
        );
        let root = candidate.bind_authoritative(authoritative).unwrap();
        let contract =
            root.bind_action_plan(InteractionActionPlanDigestV1::parse("b".repeat(64)).unwrap());

        assert_eq!(
            contract
                .route()
                .serving_identity()
                .gateway_shard_identity()
                .as_str(),
            "gateway-shard-1"
        );
        assert_eq!(
            contract
                .route()
                .serving_identity()
                .gateway_owner_lease_epoch()
                .get(),
            2
        );
        assert_eq!(
            contract
                .route()
                .serving_identity()
                .gateway_owner_revision()
                .get(),
            2
        );
    }
}
