use automation_instance::InstanceId;
use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    BindingRevision, DeploymentId, FencingToken, InstallationId, ProcessInstanceId,
    RuntimeDeploymentTargetV1, RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;

use crate::{
    InteractionGatewayOwnerIdentityV1, InteractionGatewayOwnerLeaseEpochV1,
    InteractionGatewayOwnerRevisionV1, InteractionGatewayShardIdentityV1,
    InteractionInstanceManifestDigestV1, InteractionProductScopeV1,
    InteractionRouteAttestationDigestV1, InteractionRouteBindingV1, InteractionRouteIncarnationV1,
    InteractionRuntimeBuildRevisionV1, InteractionServingLeaseEpochV1,
    InteractionServingLeaseRevisionV1, InteractionServingRouteIdentityV1,
};

pub(crate) fn static_route(seed: u64) -> InteractionRouteBindingV1 {
    static_route_with_gateway_owner(seed, seed)
}

pub(crate) fn static_route_with_gateway_owner(
    seed: u64,
    owner_seed: u64,
) -> InteractionRouteBindingV1 {
    static_route_with_gateway(seed, seed, owner_seed)
}

pub(crate) fn static_route_with_gateway_shard(
    seed: u64,
    shard_seed: u64,
) -> InteractionRouteBindingV1 {
    static_route_with_gateway(seed, shard_seed, seed)
}

fn static_route_with_gateway(
    seed: u64,
    shard_seed: u64,
    owner_seed: u64,
) -> InteractionRouteBindingV1 {
    route_with_parameters(
        seed,
        shard_seed,
        (owner_seed, seed),
        seed,
        seed,
        'b',
        TestExecutionRouteV1::Static,
    )
}

pub(crate) fn static_route_with_serving_lease(
    seed: u64,
    lease_seed: u64,
) -> InteractionRouteBindingV1 {
    route_with_parameters(
        seed,
        seed,
        (seed, seed),
        lease_seed,
        lease_seed,
        'b',
        TestExecutionRouteV1::Static,
    )
}

pub(crate) fn static_route_with_attestation(
    seed: u64,
    attestation_digit: char,
) -> InteractionRouteBindingV1 {
    route_with_parameters(
        seed,
        seed,
        (seed, seed),
        seed,
        seed,
        attestation_digit,
        TestExecutionRouteV1::Static,
    )
}

pub(crate) fn instance_route(seed: u64) -> InteractionRouteBindingV1 {
    route_with_parameters(
        seed,
        seed,
        (seed, seed),
        seed,
        seed,
        'b',
        TestExecutionRouteV1::Instance,
    )
}

pub(crate) fn static_route_with_build_revision(
    seed: u64,
    build_seed: u64,
) -> InteractionRouteBindingV1 {
    route_with_parameters(
        seed,
        seed,
        (seed, build_seed),
        seed,
        seed,
        'b',
        TestExecutionRouteV1::Static,
    )
}

#[derive(Clone, Copy)]
enum TestExecutionRouteV1 {
    Static,
    Instance,
}

fn route_with_parameters(
    seed: u64,
    shard_seed: u64,
    owner_identity: (u64, u64),
    serving_lease_epoch: u64,
    serving_lease_revision: u64,
    attestation_digit: char,
    execution_route: TestExecutionRouteV1,
) -> InteractionRouteBindingV1 {
    let digit = char::from_digit((seed % 10) as u32, 10).unwrap();
    let hash = RuleSetContentHash::parse_hex(&digit.to_string().repeat(64)).unwrap();
    let process_identity = RuntimeProcessIdentityV1 {
        target: RuntimeDeploymentTargetV1 {
            guild_id: GuildId(100 + seed),
            ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
            version: RuleSetVersionId::FIRST,
            content_hash: hash,
            binding_revision: BindingRevision::new(seed).unwrap(),
            binding_fingerprint: ResourceBindingFingerprint::parse(&"a".repeat(64)).unwrap(),
        },
        runtime_generation: RuntimeGeneration::new(seed).unwrap(),
        process_instance_id: ProcessInstanceId::parse(format!("process-{seed}")).unwrap(),
    };
    let serving_identity = InteractionServingRouteIdentityV1::new(
        InteractionRouteAttestationDigestV1::parse(attestation_digit.to_string().repeat(64))
            .unwrap(),
        InteractionServingLeaseEpochV1::new(serving_lease_epoch).unwrap(),
        InteractionServingLeaseRevisionV1::new(serving_lease_revision).unwrap(),
        InteractionGatewayOwnerIdentityV1::new(
            InteractionGatewayShardIdentityV1::parse(format!("gateway-shard-{shard_seed}"))
                .unwrap(),
            InteractionGatewayOwnerLeaseEpochV1::new(owner_identity.0).unwrap(),
            InteractionGatewayOwnerRevisionV1::new(owner_identity.0).unwrap(),
            InteractionRuntimeBuildRevisionV1::parse(format!("build-{}", owner_identity.1))
                .unwrap(),
        ),
        FencingToken::new(seed).unwrap(),
        InteractionRouteIncarnationV1::new(seed).unwrap(),
    );
    let scope = InteractionProductScopeV1::new(
        TenantId::parse(format!("tenant-{seed}")).unwrap(),
        InstallationId::parse(format!("installation-{seed}")).unwrap(),
        DeploymentId::parse(format!("deployment-{seed}")).unwrap(),
    );
    match execution_route {
        TestExecutionRouteV1::Static => {
            InteractionRouteBindingV1::new_static(scope, process_identity, serving_identity)
                .unwrap()
        }
        TestExecutionRouteV1::Instance => InteractionRouteBindingV1::new_instance(
            scope,
            process_identity,
            serving_identity,
            InstanceId::parse(&format!("instance-{seed}")).unwrap(),
            RuleSetVersionId::FIRST,
            hash,
            InteractionInstanceManifestDigestV1::parse("c".repeat(64)).unwrap(),
        )
        .unwrap(),
    }
}
