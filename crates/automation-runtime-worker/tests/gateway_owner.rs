use std::future::{ready, Future};
use std::num::NonZeroU64;
use std::time::Duration;

use automation_runtime_controller::{
    GatewayShardIdV1, RuntimeAcquireGatewayOwnerLeaseOutcomeV1, RuntimeAcquireGatewayOwnerLeaseV1,
    RuntimeBuildRevisionV1, RuntimeGatewayOwnerLeaseDurationV1, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeGatewayOwnerLeaseObservationV1, RuntimeGatewayOwnerLeaseReceiptV1,
    RuntimeObserveGatewayOwnerLeaseV1, RuntimeObservedGatewayOwnerLeaseV1,
    RuntimeReleaseGatewayOwnerLeaseOutcomeV1, RuntimeReleaseGatewayOwnerLeaseV1,
    RuntimeRenewGatewayOwnerLeaseOutcomeV1, RuntimeRenewGatewayOwnerLeaseV1,
};
use automation_runtime_convergence::ProcessInstanceId;
use automation_runtime_worker::{
    accept_gateway_owner_acquire_v1, accept_gateway_owner_observation_v1,
    accept_gateway_owner_release_v1, accept_gateway_owner_renew_v1,
    classify_unknown_gateway_owner_acquire_v1, classify_unknown_gateway_owner_release_v1,
    classify_unknown_gateway_owner_renew_v1, RuntimeAcceptedGatewayOwnerAcquireV1,
    RuntimeAcceptedGatewayOwnerReleaseV1, RuntimeAcceptedGatewayOwnerRenewV1,
    RuntimeGatewayOwnerAcquireRecoveryV1, RuntimeGatewayOwnerLeasePortV1,
    RuntimeGatewayOwnerMutationErrorV1, RuntimeGatewayOwnerProtocolViolationV1,
    RuntimeGatewayOwnerReleaseRecoveryV1, RuntimeGatewayOwnerRenewRecoveryV1,
};
use chrono::{DateTime, Utc};

fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn at(second: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(second, 0).unwrap()
}

fn lease_id(process: &str, build: &str, epoch: u64) -> RuntimeGatewayOwnerLeaseIdV1 {
    RuntimeGatewayOwnerLeaseIdV1 {
        gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        process_instance_id: ProcessInstanceId::parse(process).unwrap(),
        lease_epoch: non_zero(epoch),
        expected_build_revision: RuntimeBuildRevisionV1::parse(build).unwrap(),
    }
}

fn receipt(
    lease_id: RuntimeGatewayOwnerLeaseIdV1,
    revision: u64,
) -> RuntimeGatewayOwnerLeaseReceiptV1 {
    RuntimeGatewayOwnerLeaseReceiptV1 {
        lease_id,
        owner_revision: non_zero(revision),
        database_now: at(100),
        expires_at: at(130),
    }
}

fn observed(
    lease_id: RuntimeGatewayOwnerLeaseIdV1,
    revision: u64,
) -> RuntimeObservedGatewayOwnerLeaseV1 {
    RuntimeObservedGatewayOwnerLeaseV1 {
        lease_id,
        owner_revision: non_zero(revision),
        observed_database_now: at(100),
        expires_at: at(130),
    }
}

fn owned(
    lease_id: RuntimeGatewayOwnerLeaseIdV1,
    revision: u64,
) -> RuntimeGatewayOwnerLeaseObservationV1 {
    RuntimeGatewayOwnerLeaseObservationV1::Owned(observed(lease_id, revision))
}

fn lease_for(seconds: u64) -> RuntimeGatewayOwnerLeaseDurationV1 {
    RuntimeGatewayOwnerLeaseDurationV1::new(Duration::from_secs(seconds)).unwrap()
}

fn acquire() -> RuntimeAcquireGatewayOwnerLeaseV1 {
    RuntimeAcquireGatewayOwnerLeaseV1 {
        gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
        expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
        lease_for: lease_for(30),
    }
}

fn unowned(shard: &str) -> RuntimeGatewayOwnerLeaseObservationV1 {
    RuntimeGatewayOwnerLeaseObservationV1::Unowned {
        gateway_shard_id: GatewayShardIdV1::parse(shard).unwrap(),
        database_now: at(100),
    }
}

#[test]
fn unknown_acquire_adopts_only_the_exact_process_and_build() {
    let request = acquire();
    let exact = receipt(lease_id("process:1", "build:1", 7), 1);
    let recovery = classify_unknown_gateway_owner_acquire_v1(
        &request,
        owned(exact.lease_id.clone(), exact.owner_revision.get()),
    );
    let RuntimeGatewayOwnerAcquireRecoveryV1::Adopt(authority) = recovery else {
        panic!("expected recovered gateway owner authority")
    };
    assert_eq!(authority.receipt(), &exact);
    assert_eq!(
        classify_unknown_gateway_owner_acquire_v1(&request, unowned("shard:0")),
        RuntimeGatewayOwnerAcquireRecoveryV1::ReplaySameRequest
    );

    let foreign = receipt(lease_id("process:2", "build:1", 8), 1);
    assert_eq!(
        classify_unknown_gateway_owner_acquire_v1(
            &request,
            owned(foreign.lease_id.clone(), foreign.owner_revision.get()),
        ),
        RuntimeGatewayOwnerAcquireRecoveryV1::Contended(foreign)
    );
    let wrong_build = receipt(lease_id("process:1", "build:2", 8), 1);
    assert_eq!(
        classify_unknown_gateway_owner_acquire_v1(
            &request,
            owned(wrong_build.lease_id, wrong_build.owner_revision.get()),
        ),
        RuntimeGatewayOwnerAcquireRecoveryV1::ProtocolViolation
    );
    assert_eq!(
        classify_unknown_gateway_owner_acquire_v1(&request, unowned("shard:1")),
        RuntimeGatewayOwnerAcquireRecoveryV1::ProtocolViolation
    );
}

#[test]
fn unknown_acquire_rejects_nonfresh_receipts() {
    let request = acquire();
    let mut expired = observed(lease_id("process:1", "build:1", 7), 1);
    expired.expires_at = expired.observed_database_now;
    assert_eq!(
        classify_unknown_gateway_owner_acquire_v1(
            &request,
            RuntimeGatewayOwnerLeaseObservationV1::Owned(expired),
        ),
        RuntimeGatewayOwnerAcquireRecoveryV1::ProtocolViolation
    );
}

#[test]
fn unknown_renew_accepts_only_replay_or_the_exact_successor() {
    let exact_lease_id = lease_id("process:1", "build:1", 7);
    let request = RuntimeRenewGatewayOwnerLeaseV1 {
        lease_id: exact_lease_id.clone(),
        expected_owner_revision: non_zero(3),
        lease_for: lease_for(30),
    };
    let successor = receipt(exact_lease_id.clone(), 4);
    assert_eq!(
        classify_unknown_gateway_owner_renew_v1(
            &request,
            owned(successor.lease_id.clone(), successor.owner_revision.get()),
        ),
        RuntimeGatewayOwnerRenewRecoveryV1::AdoptSuccessor(successor)
    );
    assert_eq!(
        classify_unknown_gateway_owner_renew_v1(&request, owned(exact_lease_id.clone(), 3),),
        RuntimeGatewayOwnerRenewRecoveryV1::ReplaySameRequest
    );
    assert_eq!(
        classify_unknown_gateway_owner_renew_v1(&request, owned(exact_lease_id, 5),),
        RuntimeGatewayOwnerRenewRecoveryV1::ProtocolViolation
    );
    let foreign = receipt(lease_id("process:2", "build:1", 8), 1);
    assert_eq!(
        classify_unknown_gateway_owner_renew_v1(
            &request,
            owned(foreign.lease_id.clone(), foreign.owner_revision.get()),
        ),
        RuntimeGatewayOwnerRenewRecoveryV1::OwnershipLost(owned(
            foreign.lease_id,
            foreign.owner_revision.get()
        ))
    );
    assert_eq!(
        classify_unknown_gateway_owner_renew_v1(&request, unowned("shard:0")),
        RuntimeGatewayOwnerRenewRecoveryV1::OwnershipLost(unowned("shard:0"))
    );
}

#[test]
fn unknown_renew_rejects_revision_overflow_and_wrong_shard() {
    let exact_lease_id = lease_id("process:1", "build:1", 7);
    let exhausted = RuntimeRenewGatewayOwnerLeaseV1 {
        lease_id: exact_lease_id.clone(),
        expected_owner_revision: NonZeroU64::MAX,
        lease_for: lease_for(30),
    };
    assert_eq!(
        classify_unknown_gateway_owner_renew_v1(&exhausted, owned(exact_lease_id, u64::MAX),),
        RuntimeGatewayOwnerRenewRecoveryV1::ProtocolViolation
    );
    let request = RuntimeRenewGatewayOwnerLeaseV1 {
        lease_id: lease_id("process:1", "build:1", 7),
        expected_owner_revision: non_zero(3),
        lease_for: lease_for(30),
    };
    assert_eq!(
        classify_unknown_gateway_owner_renew_v1(&request, unowned("shard:1")),
        RuntimeGatewayOwnerRenewRecoveryV1::ProtocolViolation
    );
}

#[test]
fn unknown_release_replays_only_while_the_exact_lease_is_current() {
    let exact_lease_id = lease_id("process:1", "build:1", 7);
    let request = RuntimeReleaseGatewayOwnerLeaseV1 {
        lease_id: exact_lease_id.clone(),
    };
    assert_eq!(
        classify_unknown_gateway_owner_release_v1(&request, owned(exact_lease_id, 9),),
        RuntimeGatewayOwnerReleaseRecoveryV1::ReplaySameRequest
    );
    assert_eq!(
        classify_unknown_gateway_owner_release_v1(&request, unowned("shard:0")),
        RuntimeGatewayOwnerReleaseRecoveryV1::CompleteWithoutOwnership(unowned("shard:0"))
    );
    let foreign = receipt(lease_id("process:2", "build:1", 8), 1);
    assert_eq!(
        classify_unknown_gateway_owner_release_v1(
            &request,
            owned(foreign.lease_id.clone(), foreign.owner_revision.get()),
        ),
        RuntimeGatewayOwnerReleaseRecoveryV1::CompleteWithoutOwnership(owned(
            foreign.lease_id,
            foreign.owner_revision.get()
        ))
    );
}

#[test]
fn ordinary_acquire_acceptance_rejects_forged_or_inconsistent_acknowledgements() {
    let request = acquire();
    let exact = receipt(lease_id("process:1", "build:1", 7), 1);
    let accepted = accept_gateway_owner_acquire_v1(
        &request,
        RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Acquired(exact.clone()),
    )
    .unwrap();
    let RuntimeAcceptedGatewayOwnerAcquireV1::Acquired(authority) = accepted else {
        panic!("expected acquired gateway owner authority")
    };
    assert_eq!(authority.receipt(), &exact);

    let wrong_build = receipt(lease_id("process:1", "build:2", 7), 1);
    assert_eq!(
        accept_gateway_owner_acquire_v1(
            &request,
            RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Acquired(wrong_build),
        ),
        Err(RuntimeGatewayOwnerProtocolViolationV1::BuildMismatch)
    );
    let same_process = receipt(lease_id("process:1", "build:1", 8), 1);
    assert_eq!(
        accept_gateway_owner_acquire_v1(
            &request,
            RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Contended(same_process),
        ),
        Err(RuntimeGatewayOwnerProtocolViolationV1::InconsistentOutcome)
    );
    let mut expired = receipt(lease_id("process:2", "build:1", 8), 1);
    expired.expires_at = expired.database_now;
    assert_eq!(
        accept_gateway_owner_acquire_v1(
            &request,
            RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Contended(expired),
        ),
        Err(RuntimeGatewayOwnerProtocolViolationV1::NonFreshReceipt)
    );
}

#[test]
fn ordinary_renew_acceptance_requires_exact_lease_and_successor_revision() {
    let exact_lease_id = lease_id("process:1", "build:1", 7);
    let request = RuntimeRenewGatewayOwnerLeaseV1 {
        lease_id: exact_lease_id.clone(),
        expected_owner_revision: non_zero(3),
        lease_for: lease_for(30),
    };
    let successor = receipt(exact_lease_id.clone(), 4);
    assert_eq!(
        accept_gateway_owner_renew_v1(
            &request,
            RuntimeRenewGatewayOwnerLeaseOutcomeV1::Renewed(successor.clone()),
        ),
        Ok(RuntimeAcceptedGatewayOwnerRenewV1::Renewed(successor))
    );
    assert_eq!(
        accept_gateway_owner_renew_v1(
            &request,
            RuntimeRenewGatewayOwnerLeaseOutcomeV1::Renewed(receipt(exact_lease_id.clone(), 5,)),
        ),
        Err(RuntimeGatewayOwnerProtocolViolationV1::RevisionMismatch)
    );
    assert_eq!(
        accept_gateway_owner_renew_v1(
            &request,
            RuntimeRenewGatewayOwnerLeaseOutcomeV1::NotCurrent(owned(exact_lease_id, 4)),
        ),
        Err(RuntimeGatewayOwnerProtocolViolationV1::InconsistentOutcome)
    );
    let foreign = owned(lease_id("process:2", "build:1", 8), 1);
    assert_eq!(
        accept_gateway_owner_renew_v1(
            &request,
            RuntimeRenewGatewayOwnerLeaseOutcomeV1::NotCurrent(foreign.clone()),
        ),
        Ok(RuntimeAcceptedGatewayOwnerRenewV1::OwnershipLost(foreign))
    );
}

#[test]
fn ordinary_release_acceptance_uses_the_stable_lease_id_without_revision_cas() {
    let exact_lease_id = lease_id("process:1", "build:1", 7);
    let request = RuntimeReleaseGatewayOwnerLeaseV1 {
        lease_id: exact_lease_id.clone(),
    };
    assert_eq!(
        accept_gateway_owner_release_v1(
            &request,
            RuntimeReleaseGatewayOwnerLeaseOutcomeV1::Released {
                lease_id: exact_lease_id.clone(),
                database_now: at(140),
            },
        ),
        Ok(RuntimeAcceptedGatewayOwnerReleaseV1::Released)
    );
    assert_eq!(
        accept_gateway_owner_release_v1(
            &request,
            RuntimeReleaseGatewayOwnerLeaseOutcomeV1::NotHeld(owned(exact_lease_id, 99)),
        ),
        Err(RuntimeGatewayOwnerProtocolViolationV1::InconsistentOutcome)
    );
    let foreign_id = lease_id("process:2", "build:1", 8);
    assert_eq!(
        accept_gateway_owner_release_v1(
            &request,
            RuntimeReleaseGatewayOwnerLeaseOutcomeV1::Released {
                lease_id: foreign_id,
                database_now: at(140),
            },
        ),
        Err(RuntimeGatewayOwnerProtocolViolationV1::StableLeaseMismatch)
    );
}

#[test]
fn ordinary_observation_requires_same_statement_freshness_and_exact_shard() {
    let request = RuntimeObserveGatewayOwnerLeaseV1 {
        gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
    };
    let exact = owned(lease_id("process:1", "build:1", 7), 3);
    assert_eq!(
        accept_gateway_owner_observation_v1(&request, exact.clone()),
        Ok(exact)
    );
    let mut expired = observed(lease_id("process:1", "build:1", 7), 3);
    expired.observed_database_now = at(130);
    assert_eq!(
        accept_gateway_owner_observation_v1(
            &request,
            RuntimeGatewayOwnerLeaseObservationV1::Owned(expired),
        ),
        Err(RuntimeGatewayOwnerProtocolViolationV1::NonFreshReceipt)
    );
    assert_eq!(
        accept_gateway_owner_observation_v1(&request, unowned("shard:1")),
        Err(RuntimeGatewayOwnerProtocolViolationV1::ShardMismatch)
    );
}

#[test]
fn mutation_errors_keep_acknowledgement_uncertainty_typed() {
    assert_ne!(
        RuntimeGatewayOwnerMutationErrorV1::DefinitelyNotApplied {
            source: "transport"
        },
        RuntimeGatewayOwnerMutationErrorV1::OutcomeUnknown {
            source: "transport"
        }
    );
}
struct FakeOwnerPort;

impl RuntimeGatewayOwnerLeasePortV1 for FakeOwnerPort {
    type Error = &'static str;

    fn observe_gateway_owner(
        &self,
        request: RuntimeObserveGatewayOwnerLeaseV1,
    ) -> impl Future<Output = Result<RuntimeGatewayOwnerLeaseObservationV1, Self::Error>> + Send
    {
        ready(Ok(RuntimeGatewayOwnerLeaseObservationV1::Unowned {
            gateway_shard_id: request.gateway_shard_id,
            database_now: at(100),
        }))
    }

    fn acquire_gateway_owner(
        &self,
        request: RuntimeAcquireGatewayOwnerLeaseV1,
    ) -> impl Future<
        Output = Result<
            RuntimeAcquireGatewayOwnerLeaseOutcomeV1,
            RuntimeGatewayOwnerMutationErrorV1<Self::Error>,
        >,
    > + Send {
        ready(Ok(RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Acquired(
            receipt(
                RuntimeGatewayOwnerLeaseIdV1 {
                    gateway_shard_id: request.gateway_shard_id,
                    process_instance_id: request.process_instance_id,
                    lease_epoch: non_zero(1),
                    expected_build_revision: request.expected_build_revision,
                },
                1,
            ),
        )))
    }

    fn renew_gateway_owner(
        &self,
        request: RuntimeRenewGatewayOwnerLeaseV1,
    ) -> impl Future<
        Output = Result<
            RuntimeRenewGatewayOwnerLeaseOutcomeV1,
            RuntimeGatewayOwnerMutationErrorV1<Self::Error>,
        >,
    > + Send {
        ready(Ok(RuntimeRenewGatewayOwnerLeaseOutcomeV1::Renewed(
            receipt(request.lease_id, request.expected_owner_revision.get() + 1),
        )))
    }

    fn release_gateway_owner(
        &self,
        request: RuntimeReleaseGatewayOwnerLeaseV1,
    ) -> impl Future<
        Output = Result<
            RuntimeReleaseGatewayOwnerLeaseOutcomeV1,
            RuntimeGatewayOwnerMutationErrorV1<Self::Error>,
        >,
    > + Send {
        ready(Ok(RuntimeReleaseGatewayOwnerLeaseOutcomeV1::Released {
            lease_id: request.lease_id,
            database_now: at(100),
        }))
    }
}

#[test]
fn gateway_owner_port_requires_no_runtime_or_database_framework() {
    fn assert_port<T: RuntimeGatewayOwnerLeasePortV1>() {}
    assert_port::<FakeOwnerPort>();
}
