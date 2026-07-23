use std::num::NonZeroU64;

use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    BindingRevision, ControllerId, DeploymentId, DeploymentRevision, FencingToken, InstallationId,
    ProcessInstanceId, RuntimeDeploymentTargetV1, RuntimeGeneration, RuntimeProcessIdentityV1,
    TenantId,
};
use chrono::{DateTime, Utc};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;

use super::{
    RuntimeDrainCertificationResolutionKindV2, RuntimeDrainCertificationResolutionV2,
    RuntimeDrainClaimErrorV2, RuntimeDrainClaimFieldV2, RuntimeDrainClaimProgressKindV2,
    RuntimeDrainClaimProgressV2, RuntimeDrainClaimSealWitnessV2, RuntimeDrainClaimV2,
    RuntimeRouteAbsentAcknowledgementV2,
};
use crate::{
    GatewayShardIdV1, RuntimeBarrierIdV1, RuntimeBarrierPauseWitnessV2, RuntimeBuildRevisionV1,
    RuntimeCanonicalValueErrorV2, RuntimeCertificationIntentFingerprintV2,
    RuntimeCertificationOperationIdV2, RuntimeDeploymentScopeV1, RuntimeDrainIntentIdV2,
    RuntimeDrainIntentKeyV2, RuntimeExactLocalRouteIdentityV2, RuntimeGatewayAdmissionSequenceV2,
    RuntimeGatewayOwnerLeaseIdV1, RuntimeLiveAttestationDigestV2, RuntimeProductMutationDigestV2,
    RuntimeProductMutationKindV2, RuntimeProductOperationIdV2, RuntimeRouteMutationProvenanceV2,
    RuntimeServingIdentityV2, RuntimeServingSlotV2,
};

fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn at(second: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(second, 0).unwrap()
}

fn process_id(value: &str) -> ProcessInstanceId {
    ProcessInstanceId::parse(value).unwrap()
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

fn scope() -> RuntimeDeploymentScopeV1 {
    RuntimeDeploymentScopeV1 {
        tenant_id: TenantId::parse("tenant:1").unwrap(),
        installation_id: InstallationId::parse("installation:1").unwrap(),
        deployment_id: DeploymentId::parse("deployment:1").unwrap(),
    }
}

fn key() -> RuntimeDrainIntentKeyV2 {
    let expected_target = target();
    RuntimeDrainIntentKeyV2 {
        intent_id: RuntimeDrainIntentIdV2::parse("00112233445566778899aabbccddeeff").unwrap(),
        product_operation_id: RuntimeProductOperationIdV2::parse(
            "ffeeddccbbaa99887766554433221100",
        )
        .unwrap(),
        product_mutation_digest: RuntimeProductMutationDigestV2::parse("d".repeat(64)).unwrap(),
        scope: scope(),
        expected_revision: DeploymentRevision::new(5).unwrap(),
        slot: RuntimeServingSlotV2::from_target(&expected_target),
        expected_target,
        mutation_kind: RuntimeProductMutationKindV2::Teardown,
    }
}

fn route(process: &str, fence: u64) -> RuntimeExactLocalRouteIdentityV2 {
    RuntimeExactLocalRouteIdentityV2 {
        identity: RuntimeProcessIdentityV1 {
            target: target(),
            runtime_generation: RuntimeGeneration::new(6).unwrap(),
            process_instance_id: process_id(process),
        },
        controller_fencing_token: FencingToken::new(fence).unwrap(),
        route_incarnation: non_zero(8),
    }
}

fn owner(process: &str) -> RuntimeGatewayOwnerLeaseIdV1 {
    RuntimeGatewayOwnerLeaseIdV1 {
        gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        process_instance_id: process_id(process),
        lease_epoch: non_zero(9),
        expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
    }
}

fn ordinary() -> RuntimeRouteMutationProvenanceV2 {
    RuntimeRouteMutationProvenanceV2::Ordinary {
        barrier_id: RuntimeBarrierIdV1::parse("11112222333344445555666677778888").unwrap(),
        pause: RuntimeBarrierPauseWitnessV2 {
            coordinator_generation: non_zero(10),
            connection_epoch: non_zero(11),
            paused_admission_revision: non_zero(12),
            pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(13)),
        },
    }
}

fn seal(
    expected_route: Option<RuntimeExactLocalRouteIdentityV2>,
) -> RuntimeDrainClaimSealWitnessV2 {
    RuntimeDrainClaimSealWitnessV2::new(
        &key(),
        process_id("process:1"),
        non_zero(14),
        expected_route,
        non_zero(15),
    )
    .unwrap()
}

fn claimed(
    expected_route: Option<RuntimeExactLocalRouteIdentityV2>,
) -> RuntimeDrainClaimProgressV2 {
    RuntimeDrainClaimProgressV2::claimed(seal(expected_route))
}

fn claim(progress: RuntimeDrainClaimProgressV2) -> RuntimeDrainClaimV2 {
    RuntimeDrainClaimV2::new(
        &key(),
        owner("process:1"),
        non_zero(16),
        process_id("process:1"),
        ControllerId::parse("controller:1").unwrap(),
        FencingToken::new(20).unwrap(),
        non_zero(17),
        non_zero(18),
        at(130),
        progress,
    )
    .unwrap()
}

fn refenced() -> RuntimeDrainClaimProgressV2 {
    RuntimeDrainClaimProgressV2::refenced(
        seal(Some(route("process:1", 19))),
        ordinary(),
        route("process:1", 19),
        route("process:1", 20),
        non_zero(21),
        at(110),
    )
    .unwrap()
}

fn serving_identity() -> RuntimeServingIdentityV2 {
    RuntimeServingIdentityV2 {
        scope: scope(),
        operation_id: RuntimeCertificationOperationIdV2::parse("9999aaaabbbbccccddddeeeeffff0000")
            .unwrap(),
        attestation_digest: RuntimeLiveAttestationDigestV2::parse("e".repeat(64)).unwrap(),
        process_identity: RuntimeProcessIdentityV1 {
            target: target(),
            runtime_generation: RuntimeGeneration::new(6).unwrap(),
            process_instance_id: process_id("process:1"),
        },
        lease_epoch: non_zero(22),
        revision: non_zero(23),
    }
}

#[test]
fn seal_binds_the_exact_intent_slot_process_and_route() {
    let route = route("process:1", 19);
    let seal = seal(Some(route.clone()));

    assert_eq!(seal.intent_id(), &key().intent_id);
    assert_eq!(seal.slot(), &key().slot);
    assert_eq!(seal.process_instance_id(), &process_id("process:1"));
    assert_eq!(seal.expected_route(), Some(&route));
    assert_eq!(seal.seal_generation(), non_zero(14));
    assert_eq!(seal.registry_observation_sequence(), non_zero(15));

    let mut wrong_key = key();
    wrong_key.intent_id =
        RuntimeDrainIntentIdV2::parse("11112222333344445555666677778888").unwrap();
    assert_eq!(
        seal.validate_for_key(&wrong_key),
        Err(RuntimeDrainClaimErrorV2::IntentMismatch)
    );
}

#[test]
fn seal_rejects_wrong_route_slot_target_and_process() {
    let mut wrong_slot_key = key();
    wrong_slot_key.slot =
        RuntimeServingSlotV2::new(GuildId(7), RuleSetKey::parse("different").unwrap());
    assert_eq!(
        RuntimeDrainClaimSealWitnessV2::new(
            &wrong_slot_key,
            process_id("process:1"),
            non_zero(1),
            None,
            non_zero(2),
        ),
        Err(RuntimeDrainClaimErrorV2::SlotMismatch)
    );

    assert_eq!(
        RuntimeDrainClaimSealWitnessV2::new(
            &key(),
            process_id("process:2"),
            non_zero(1),
            Some(route("process:1", 19)),
            non_zero(2),
        ),
        Err(RuntimeDrainClaimErrorV2::ProcessMismatch)
    );

    let mut wrong_target = route("process:1", 19);
    wrong_target.identity.target.version = RuleSetVersionId::new(2).unwrap();
    assert_eq!(
        RuntimeDrainClaimSealWitnessV2::new(
            &key(),
            process_id("process:1"),
            non_zero(1),
            Some(wrong_target),
            non_zero(2),
        ),
        Err(RuntimeDrainClaimErrorV2::TargetMismatch)
    );
}

#[test]
fn refenced_progress_changes_only_to_a_strictly_newer_fence() {
    let progress = refenced();

    assert_eq!(progress.kind(), RuntimeDrainClaimProgressKindV2::Refenced);
    assert_eq!(progress.old_route(), Some(&route("process:1", 19)));
    assert_eq!(progress.removal_target(), Some(&route("process:1", 20)));
    assert_eq!(progress.provenance(), Some(&ordinary()));
    assert_eq!(progress.registry_observation_sequence(), Some(non_zero(21)));
    assert_eq!(progress.refenced_at(), Some(at(110)));

    assert_eq!(
        RuntimeDrainClaimProgressV2::refenced(
            seal(Some(route("process:1", 19))),
            ordinary(),
            route("process:1", 19),
            route("process:1", 19),
            non_zero(21),
            at(110),
        ),
        Err(RuntimeDrainClaimErrorV2::RefencedFenceNotNewer)
    );

    let mut different = route("process:1", 20);
    different.route_incarnation = non_zero(99);
    assert_eq!(
        RuntimeDrainClaimProgressV2::refenced(
            seal(Some(route("process:1", 19))),
            ordinary(),
            route("process:1", 19),
            different,
            non_zero(21),
            at(110),
        ),
        Err(RuntimeDrainClaimErrorV2::RefencedRouteMismatch)
    );

    assert_eq!(
        RuntimeDrainClaimProgressV2::refenced(
            seal(Some(route("process:1", 19))),
            ordinary(),
            route("process:1", 19),
            route("process:1", 20),
            non_zero(15),
            at(110),
        ),
        Err(RuntimeDrainClaimErrorV2::RefenceObservationNotAfterSeal)
    );
}

#[test]
fn claim_binds_owner_process_fence_and_progress() {
    let claim = claim(refenced());

    assert_eq!(claim.gateway_owner_lease_id(), &owner("process:1"));
    assert_eq!(claim.observed_owner_revision(), non_zero(16));
    assert_eq!(claim.process_instance_id(), &process_id("process:1"));
    assert_eq!(
        claim.controller_id(),
        &ControllerId::parse("controller:1").unwrap()
    );
    assert_eq!(
        claim.controller_fencing_token(),
        FencingToken::new(20).unwrap()
    );
    assert_eq!(claim.claim_epoch(), non_zero(17));
    assert_eq!(claim.claim_revision(), non_zero(18));
    assert_eq!(claim.expires_at(), at(130));
    assert_eq!(
        claim.progress().kind(),
        RuntimeDrainClaimProgressKindV2::Refenced
    );
}

#[test]
fn claim_rejects_foreign_owner_process_and_wrong_claim_fence() {
    assert_eq!(
        RuntimeDrainClaimV2::new(
            &key(),
            owner("process:2"),
            non_zero(16),
            process_id("process:1"),
            ControllerId::parse("controller:1").unwrap(),
            FencingToken::new(20).unwrap(),
            non_zero(17),
            non_zero(18),
            at(130),
            refenced(),
        ),
        Err(RuntimeDrainClaimErrorV2::ProcessMismatch)
    );

    assert_eq!(
        RuntimeDrainClaimV2::new(
            &key(),
            owner("process:1"),
            non_zero(16),
            process_id("process:1"),
            ControllerId::parse("controller:1").unwrap(),
            FencingToken::new(21).unwrap(),
            non_zero(17),
            non_zero(18),
            at(130),
            refenced(),
        ),
        Err(RuntimeDrainClaimErrorV2::ClaimFenceMismatch)
    );
}

#[test]
fn certification_resolution_binds_operation_scope_target_and_process() {
    let claim = claim(refenced());
    let serving = serving_identity();
    let resolution = RuntimeDrainCertificationResolutionV2::committed_and_disconnected(
        &key(),
        &claim,
        serving.operation_id.clone(),
        serving.clone(),
        non_zero(24),
    )
    .unwrap();

    assert_eq!(
        resolution.kind(),
        RuntimeDrainCertificationResolutionKindV2::CommittedAndDisconnected
    );
    assert_eq!(resolution.operation_id(), Some(&serving.operation_id));
    assert_eq!(resolution.serving_identity(), Some(&serving));
    assert_eq!(resolution.disconnected_revision(), Some(non_zero(24)));

    let mut wrong_generation = serving.clone();
    wrong_generation.process_identity.runtime_generation = RuntimeGeneration::new(7).unwrap();
    assert_eq!(
        RuntimeDrainCertificationResolutionV2::committed_and_disconnected(
            &key(),
            &claim,
            wrong_generation.operation_id.clone(),
            wrong_generation,
            non_zero(24),
        ),
        Err(RuntimeDrainClaimErrorV2::CertificationRouteMismatch)
    );

    let wrong_operation =
        RuntimeCertificationOperationIdV2::parse("11112222333344445555666677778888").unwrap();
    assert_eq!(
        RuntimeDrainCertificationResolutionV2::committed_and_disconnected(
            &key(),
            &claim,
            wrong_operation,
            serving,
            non_zero(24),
        ),
        Err(RuntimeDrainClaimErrorV2::CertificationOperationMismatch)
    );

    assert_eq!(
        RuntimeDrainCertificationResolutionV2::committed_and_disconnected(
            &key(),
            &claim,
            serving_identity().operation_id,
            serving_identity(),
            non_zero(25),
        ),
        Err(RuntimeDrainClaimErrorV2::CertificationDisconnectRevisionMismatch)
    );
}

#[test]
fn all_certification_resolution_variants_have_closed_views() {
    let none = RuntimeDrainCertificationResolutionV2::no_operation_reserved();
    assert_eq!(
        none.kind(),
        RuntimeDrainCertificationResolutionKindV2::NoOperationReserved
    );
    assert!(none.operation_id().is_none());

    let operation_id =
        RuntimeCertificationOperationIdV2::parse("9999aaaabbbbccccddddeeeeffff0000").unwrap();
    let fingerprint = RuntimeCertificationIntentFingerprintV2::parse("f".repeat(64)).unwrap();
    let no_attestation =
        RuntimeDrainCertificationResolutionV2::no_attestation_for_reserved_operation(
            operation_id.clone(),
            fingerprint.clone(),
        );
    assert_eq!(
        no_attestation.kind(),
        RuntimeDrainCertificationResolutionKindV2::NoAttestationForReservedOperation
    );
    assert_eq!(no_attestation.operation_id(), Some(&operation_id));
    assert_eq!(no_attestation.intent_fingerprint(), Some(&fingerprint));
}

#[test]
fn acknowledgement_accepts_refenced_and_initially_absent_claims() {
    let refenced_claim = claim(refenced());
    let resolution = RuntimeDrainCertificationResolutionV2::no_operation_reserved();
    let acknowledgement = RuntimeRouteAbsentAcknowledgementV2::new(
        &key(),
        refenced_claim.clone(),
        Some(route("process:1", 20)),
        ordinary(),
        non_zero(25),
        resolution,
        at(120),
    )
    .unwrap();

    assert_eq!(acknowledgement.claim(), &refenced_claim);
    assert_eq!(
        acknowledgement.expected_route(),
        Some(&route("process:1", 20))
    );
    assert_eq!(acknowledgement.provenance(), &ordinary());
    assert_eq!(
        acknowledgement.registry_observation_sequence(),
        non_zero(25)
    );
    assert_eq!(acknowledgement.acknowledged_at(), at(120));

    let absent_claim = claim(claimed(None));
    assert!(RuntimeRouteAbsentAcknowledgementV2::new(
        &key(),
        absent_claim,
        None,
        ordinary(),
        non_zero(15),
        RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
        at(120),
    )
    .is_ok());
}

#[test]
fn acknowledgement_rejects_wrong_route_and_accepts_a_distinct_removal_barrier() {
    let refenced_claim = claim(refenced());
    assert_eq!(
        RuntimeRouteAbsentAcknowledgementV2::new(
            &key(),
            refenced_claim.clone(),
            Some(route("process:1", 19)),
            ordinary(),
            non_zero(25),
            RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
            at(120),
        ),
        Err(RuntimeDrainClaimErrorV2::AcknowledgementProgressMismatch)
    );

    let mut different = ordinary();
    let RuntimeRouteMutationProvenanceV2::Ordinary { pause, .. } = &mut different else {
        unreachable!()
    };
    pause.pause_sequence = RuntimeGatewayAdmissionSequenceV2::new(non_zero(99));
    assert!(RuntimeRouteAbsentAcknowledgementV2::new(
        &key(),
        refenced_claim,
        Some(route("process:1", 20)),
        different,
        non_zero(25),
        RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
        at(120),
    )
    .is_ok());

    assert_eq!(
        RuntimeRouteAbsentAcknowledgementV2::new(
            &key(),
            claim(refenced()),
            Some(route("process:1", 20)),
            ordinary(),
            non_zero(21),
            RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
            at(120),
        ),
        Err(RuntimeDrainClaimErrorV2::AcknowledgementObservationNotAfterRefence)
    );
}

#[test]
fn persistence_numbers_and_timestamps_reject_noncanonical_values() {
    let overflow = non_zero(i64::MAX as u64 + 1);
    let mut overflow_key = key();
    overflow_key.expected_revision = DeploymentRevision::new(overflow.get()).unwrap();
    assert_eq!(
        RuntimeDrainClaimSealWitnessV2::new(
            &overflow_key,
            process_id("process:1"),
            non_zero(1),
            None,
            non_zero(2),
        ),
        Err(RuntimeDrainClaimErrorV2::CanonicalValue {
            field: RuntimeDrainClaimFieldV2::ExpectedRevision,
            reason: RuntimeCanonicalValueErrorV2::PersistenceIntegerOutOfRange,
        })
    );

    assert_eq!(
        RuntimeDrainClaimSealWitnessV2::new(
            &key(),
            process_id("process:1"),
            overflow,
            None,
            non_zero(1),
        ),
        Err(RuntimeDrainClaimErrorV2::CanonicalValue {
            field: RuntimeDrainClaimFieldV2::SealGeneration,
            reason: RuntimeCanonicalValueErrorV2::PersistenceIntegerOutOfRange,
        })
    );

    let sub_microsecond = DateTime::from_timestamp(120, 1).unwrap();
    assert_eq!(
        RuntimeDrainClaimProgressV2::refenced(
            seal(Some(route("process:1", 19))),
            ordinary(),
            route("process:1", 19),
            route("process:1", 20),
            non_zero(21),
            sub_microsecond,
        ),
        Err(RuntimeDrainClaimErrorV2::CanonicalValue {
            field: RuntimeDrainClaimFieldV2::RefencedAt,
            reason: RuntimeCanonicalValueErrorV2::TimestampSubMicrosecond,
        })
    );
}
