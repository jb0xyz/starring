use std::num::NonZeroU64;
use std::time::{Duration, Instant};

use automation_runtime_controller::{
    GatewayShardIdV1, RuntimeAcquireGatewayOwnerLeaseOutcomeV1, RuntimeAcquireGatewayOwnerLeaseV1,
    RuntimeBuildRevisionV1, RuntimeGatewayOwnerLeaseDurationV1, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeGatewayOwnerLeaseObservationV1, RuntimeGatewayOwnerLeaseReceiptV1,
    RuntimeObservedGatewayOwnerLeaseV1, RuntimeRenewGatewayOwnerLeaseOutcomeV1,
};
use automation_runtime_convergence::ProcessInstanceId;
use automation_runtime_worker::{
    accept_gateway_owner_acquire_v1, RuntimeAcceptedGatewayOwnerAcquireV1,
    RuntimeAcceptedGatewayOwnerReceiptV1, RuntimeGatewayOwnerObservationCompletionV1,
    RuntimeGatewayOwnerProtocolViolationV1, RuntimeGatewayOwnerRenewalCompletionV1,
    RuntimeGatewayOwnerRenewalPolicyErrorV1, RuntimeGatewayOwnerRenewalPolicyV1,
    RuntimeGatewayOwnerRenewalScheduleErrorV1, RuntimeGatewayOwnerWatchdogActionV1,
    RuntimeGatewayOwnerWatchdogErrorV1, RuntimeGatewayOwnerWatchdogV1,
};
use chrono::{DateTime, Utc};

fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn at(second: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(second, 0).unwrap()
}

fn lease_id() -> RuntimeGatewayOwnerLeaseIdV1 {
    RuntimeGatewayOwnerLeaseIdV1 {
        gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
        lease_epoch: non_zero(7),
        expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
    }
}

fn receipt(revision: u64, duration_seconds: i64) -> RuntimeGatewayOwnerLeaseReceiptV1 {
    RuntimeGatewayOwnerLeaseReceiptV1 {
        lease_id: lease_id(),
        owner_revision: non_zero(revision),
        database_now: at(100),
        expires_at: at(100 + duration_seconds),
    }
}

fn policy() -> RuntimeGatewayOwnerRenewalPolicyV1 {
    RuntimeGatewayOwnerRenewalPolicyV1::new(Duration::from_secs(10), Duration::from_secs(3))
        .unwrap()
}

fn lease_for() -> RuntimeGatewayOwnerLeaseDurationV1 {
    RuntimeGatewayOwnerLeaseDurationV1::new(Duration::from_secs(30)).unwrap()
}

fn accepted_receipt(revision: u64, duration_seconds: i64) -> RuntimeAcceptedGatewayOwnerReceiptV1 {
    let receipt = receipt(revision, duration_seconds);
    let request = RuntimeAcquireGatewayOwnerLeaseV1 {
        gateway_shard_id: receipt.lease_id.gateway_shard_id.clone(),
        process_instance_id: receipt.lease_id.process_instance_id.clone(),
        expected_build_revision: receipt.lease_id.expected_build_revision.clone(),
        lease_for: lease_for(),
    };
    let accepted = accept_gateway_owner_acquire_v1(
        &request,
        RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Acquired(receipt),
    )
    .unwrap();
    let RuntimeAcceptedGatewayOwnerAcquireV1::Acquired(accepted_receipt) = accepted else {
        panic!("expected accepted gateway owner receipt")
    };
    accepted_receipt
}

fn owned_observation(
    lease_id: RuntimeGatewayOwnerLeaseIdV1,
    revision: u64,
    database_now: i64,
    expires_at: i64,
) -> RuntimeGatewayOwnerLeaseObservationV1 {
    RuntimeGatewayOwnerLeaseObservationV1::Owned(RuntimeObservedGatewayOwnerLeaseV1 {
        lease_id,
        owner_revision: non_zero(revision),
        observed_database_now: at(database_now),
        expires_at: at(expires_at),
    })
}

#[test]
fn schedule_uses_request_start_and_subtracts_response_latency() {
    let started = Instant::now();
    let responded = started + Duration::from_secs(2);
    let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(3, 30),
        policy(),
        started,
        responded,
    )
    .unwrap();
    let schedule = watchdog.schedule();

    assert_eq!(schedule.request_started_at(), started);
    assert_eq!(schedule.response_observed_at(), responded);
    assert_eq!(schedule.renew_at(), started + Duration::from_secs(20));
    assert_eq!(
        schedule.safety_deadline(),
        started + Duration::from_secs(27)
    );
    assert_eq!(
        schedule.conservative_expiry(),
        started + Duration::from_secs(30)
    );
    assert_eq!(
        schedule.safe_remaining_at(responded),
        Some(Duration::from_secs(25))
    );
    assert_eq!(schedule.safe_remaining_at(schedule.safety_deadline()), None);
}

#[test]
fn policy_requires_strict_nonzero_margin_order() {
    assert_eq!(
        RuntimeGatewayOwnerRenewalPolicyV1::new(Duration::ZERO, Duration::from_secs(1)),
        Err(RuntimeGatewayOwnerRenewalPolicyErrorV1::ZeroDuration)
    );
    assert_eq!(
        RuntimeGatewayOwnerRenewalPolicyV1::new(Duration::from_secs(1), Duration::ZERO),
        Err(RuntimeGatewayOwnerRenewalPolicyErrorV1::ZeroDuration)
    );
    assert_eq!(
        RuntimeGatewayOwnerRenewalPolicyV1::new(Duration::from_secs(3), Duration::from_secs(3),),
        Err(RuntimeGatewayOwnerRenewalPolicyErrorV1::InvalidOrder)
    );
    assert_eq!(policy().renew_before(), Duration::from_secs(10));
    assert_eq!(policy().safety_margin(), Duration::from_secs(3));
}

#[test]
fn short_receipts_and_invalid_monotonic_observations_fail_closed() {
    let started = Instant::now();
    assert_eq!(
        RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
            accepted_receipt(3, 10),
            policy(),
            started,
            started,
        ),
        Err(RuntimeGatewayOwnerRenewalScheduleErrorV1::LeaseTooShort)
    );
    assert_eq!(
        RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
            accepted_receipt(3, 30),
            policy(),
            started,
            started - Duration::from_secs(1),
        ),
        Err(RuntimeGatewayOwnerRenewalScheduleErrorV1::ClockReversed)
    );
    assert_eq!(
        RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
            accepted_receipt(3, 30),
            policy(),
            started,
            started + Duration::from_secs(27),
        ),
        Err(RuntimeGatewayOwnerRenewalScheduleErrorV1::SafetyElapsed)
    );
}

#[test]
fn watchdog_actions_are_exact_at_renewal_and_safety_boundaries() {
    let started = Instant::now();
    let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(3, 30),
        policy(),
        started,
        started + Duration::from_secs(2),
    )
    .unwrap();

    assert_eq!(
        watchdog.action_at(started + Duration::from_secs(19)),
        RuntimeGatewayOwnerWatchdogActionV1::WaitUntil(started + Duration::from_secs(20))
    );
    assert_eq!(
        watchdog.action_at(started + Duration::from_secs(20)),
        RuntimeGatewayOwnerWatchdogActionV1::RenewNow
    );
    assert_eq!(
        watchdog.action_at(started + Duration::from_secs(26)),
        RuntimeGatewayOwnerWatchdogActionV1::RenewNow
    );
    assert_eq!(
        watchdog.action_at(started + Duration::from_secs(27)),
        RuntimeGatewayOwnerWatchdogActionV1::InvalidateNow
    );
}

#[test]
fn one_inflight_renewal_builds_an_exact_successor_schedule() {
    let started = Instant::now();
    let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(3, 30),
        policy(),
        started,
        started + Duration::from_secs(2),
    )
    .unwrap();
    let renewal_started = started + Duration::from_secs(20);
    let inflight = watchdog
        .begin_renewal(lease_for(), renewal_started)
        .unwrap();

    assert_eq!(inflight.request().lease_id, lease_id());
    assert_eq!(inflight.request().expected_owner_revision, non_zero(3));
    assert_eq!(inflight.request().lease_for, lease_for());
    let completion = inflight
        .complete(
            RuntimeRenewGatewayOwnerLeaseOutcomeV1::Renewed(receipt(4, 30)),
            renewal_started + Duration::from_secs(2),
        )
        .unwrap();
    let RuntimeGatewayOwnerRenewalCompletionV1::Renewed(successor) = completion else {
        panic!("expected renewed watchdog")
    };
    assert_eq!(successor.schedule().receipt().owner_revision, non_zero(4));
    assert_eq!(
        successor.schedule().renew_at(),
        renewal_started + Duration::from_secs(20)
    );
    assert_eq!(
        successor.schedule().safety_deadline(),
        renewal_started + Duration::from_secs(27)
    );
}

#[test]
fn late_or_forged_successors_cannot_restore_the_watchdog() {
    let started = Instant::now();
    let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(3, 30),
        policy(),
        started,
        started + Duration::from_secs(2),
    )
    .unwrap();
    let renewal_started = started + Duration::from_secs(20);
    let inflight = watchdog
        .begin_renewal(lease_for(), renewal_started)
        .unwrap();
    assert_eq!(
        inflight.complete(
            RuntimeRenewGatewayOwnerLeaseOutcomeV1::Renewed(receipt(4, 30)),
            started + Duration::from_secs(27),
        ),
        Err(RuntimeGatewayOwnerWatchdogErrorV1::SafetyElapsed)
    );

    let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(3, 30),
        policy(),
        started,
        started + Duration::from_secs(2),
    )
    .unwrap();
    let inflight = watchdog
        .begin_renewal(lease_for(), renewal_started)
        .unwrap();
    assert_eq!(
        inflight.complete(
            RuntimeRenewGatewayOwnerLeaseOutcomeV1::Renewed(receipt(4, 30)),
            started + Duration::from_secs(28),
        ),
        Err(RuntimeGatewayOwnerWatchdogErrorV1::SafetyElapsed)
    );

    let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(3, 30),
        policy(),
        started,
        started + Duration::from_secs(2),
    )
    .unwrap();
    let inflight = watchdog
        .begin_renewal(lease_for(), renewal_started)
        .unwrap();
    assert!(matches!(
        inflight.complete(
            RuntimeRenewGatewayOwnerLeaseOutcomeV1::Renewed(receipt(5, 30)),
            renewal_started + Duration::from_secs(2),
        ),
        Err(RuntimeGatewayOwnerWatchdogErrorV1::ProtocolViolation { .. })
    ));
}

#[test]
fn ownership_loss_is_terminal_instead_of_reusing_the_old_schedule() {
    let started = Instant::now();
    let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(3, 30),
        policy(),
        started,
        started + Duration::from_secs(2),
    )
    .unwrap();
    let renewal_started = started + Duration::from_secs(20);
    let inflight = watchdog
        .begin_renewal(lease_for(), renewal_started)
        .unwrap();
    let foreign_id = RuntimeGatewayOwnerLeaseIdV1 {
        process_instance_id: ProcessInstanceId::parse("process:2").unwrap(),
        lease_epoch: non_zero(8),
        ..lease_id()
    };
    let observation =
        RuntimeGatewayOwnerLeaseObservationV1::Owned(RuntimeObservedGatewayOwnerLeaseV1 {
            lease_id: foreign_id,
            owner_revision: non_zero(1),
            observed_database_now: at(100),
            expires_at: at(130),
        });
    let completion = inflight
        .complete(
            RuntimeRenewGatewayOwnerLeaseOutcomeV1::NotCurrent(observation.clone()),
            renewal_started + Duration::from_secs(2),
        )
        .unwrap();

    assert_eq!(
        completion,
        RuntimeGatewayOwnerRenewalCompletionV1::OwnershipLost(observation)
    );
}

#[test]
fn definite_failure_preserves_but_never_extends_the_old_safety_deadline() {
    let started = Instant::now();
    let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(3, 30),
        policy(),
        started,
        started + Duration::from_secs(2),
    )
    .unwrap();
    let inflight = watchdog
        .begin_renewal(lease_for(), started + Duration::from_secs(20))
        .unwrap();
    let restored = inflight
        .definitely_not_applied(started + Duration::from_secs(22))
        .unwrap();
    assert_eq!(
        restored.schedule().safety_deadline(),
        started + Duration::from_secs(27)
    );

    let inflight = restored
        .begin_renewal(lease_for(), started + Duration::from_secs(23))
        .unwrap();
    assert_eq!(
        inflight.definitely_not_applied(started + Duration::from_secs(27)),
        Err(RuntimeGatewayOwnerWatchdogErrorV1::SafetyElapsed)
    );
}

#[test]
fn unknown_outcome_retains_exact_evidence_without_an_active_watchdog() {
    let started = Instant::now();
    let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(3, 30),
        policy(),
        started,
        started + Duration::from_secs(2),
    )
    .unwrap();
    let renewal_started = started + Duration::from_secs(20);
    let unknown = watchdog
        .begin_renewal(lease_for(), renewal_started)
        .unwrap()
        .into_unknown();

    assert_eq!(unknown.request().lease_id, lease_id());
    assert_eq!(unknown.request().expected_owner_revision, non_zero(3));
    assert_eq!(unknown.request_started_at(), renewal_started);
    assert_eq!(
        unknown.previous_schedule().safety_deadline(),
        started + Duration::from_secs(27)
    );
}

#[test]
fn exhausted_revision_or_elapsed_safety_prevents_dispatch() {
    let started = Instant::now();
    let exhausted = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(u64::MAX, 30),
        policy(),
        started,
        started + Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(
        exhausted.begin_renewal(lease_for(), started + Duration::from_secs(20)),
        Err(RuntimeGatewayOwnerWatchdogErrorV1::RevisionExhausted)
    );

    let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(3, 30),
        policy(),
        started,
        started + Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(
        watchdog.begin_renewal(lease_for(), started + Duration::from_secs(27)),
        Err(RuntimeGatewayOwnerWatchdogErrorV1::SafetyElapsed)
    );

    let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(3, 30),
        policy(),
        started,
        started + Duration::from_secs(2),
    )
    .unwrap();
    let short_lease = RuntimeGatewayOwnerLeaseDurationV1::new(Duration::from_secs(10)).unwrap();
    assert_eq!(
        watchdog.begin_renewal(short_lease, started + Duration::from_secs(20)),
        Err(RuntimeGatewayOwnerWatchdogErrorV1::RequestedLeaseTooShort)
    );
}

#[test]
fn exact_observation_tightens_the_schedule_and_preserves_current_receipt_evidence() {
    let started = Instant::now();
    let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(3, 30),
        policy(),
        started,
        started + Duration::from_secs(2),
    )
    .unwrap();
    let observation_started = started + Duration::from_secs(5);
    let inflight = watchdog
        .begin_current_observation(observation_started)
        .unwrap();
    assert_eq!(
        inflight.request().gateway_shard_id,
        lease_id().gateway_shard_id
    );
    assert_eq!(
        inflight.previous_schedule().safety_deadline(),
        started + Duration::from_secs(27)
    );
    let completion = inflight
        .complete(
            owned_observation(lease_id(), 3, 110, 130),
            started + Duration::from_secs(6),
        )
        .unwrap();
    let RuntimeGatewayOwnerObservationCompletionV1::Current(current) = completion else {
        panic!("expected current owner observation")
    };

    assert_eq!(current.schedule().receipt().database_now, at(110));
    assert_eq!(current.schedule().receipt().expires_at, at(130));
    assert_eq!(current.schedule().request_started_at(), observation_started);
    assert_eq!(
        current.schedule().response_observed_at(),
        started + Duration::from_secs(6)
    );
    assert_eq!(
        current.schedule().conservative_expiry(),
        started + Duration::from_secs(25)
    );
    assert_eq!(
        current.schedule().renew_at(),
        started + Duration::from_secs(15)
    );
    assert_eq!(
        current.schedule().safety_deadline(),
        started + Duration::from_secs(22)
    );
}

#[test]
fn observation_can_never_extend_the_existing_monotonic_schedule() {
    let started = Instant::now();
    let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(3, 30),
        policy(),
        started,
        started + Duration::from_secs(2),
    )
    .unwrap();
    let old_expiry = watchdog.schedule().conservative_expiry();
    let old_renew_at = watchdog.schedule().renew_at();
    let old_safety = watchdog.schedule().safety_deadline();
    let completion = watchdog
        .begin_current_observation(started + Duration::from_secs(10))
        .unwrap()
        .complete(
            owned_observation(lease_id(), 3, 101, 130),
            started + Duration::from_secs(11),
        )
        .unwrap();
    let RuntimeGatewayOwnerObservationCompletionV1::Current(current) = completion else {
        panic!("expected current owner observation")
    };

    assert_eq!(current.schedule().receipt().database_now, at(101));
    assert_eq!(current.schedule().conservative_expiry(), old_expiry);
    assert_eq!(current.schedule().renew_at(), old_renew_at);
    assert_eq!(current.schedule().safety_deadline(), old_safety);
    assert_eq!(
        current.schedule().request_started_at(),
        started + Duration::from_secs(10)
    );
    assert_eq!(
        current.schedule().response_observed_at(),
        started + Duration::from_secs(11)
    );
}

#[test]
fn unowned_or_foreign_observation_consumes_the_watchdog_as_ownership_loss() {
    let started = Instant::now();
    let unowned = RuntimeGatewayOwnerLeaseObservationV1::Unowned {
        gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        database_now: at(110),
    };
    let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(3, 30),
        policy(),
        started,
        started + Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(
        watchdog
            .begin_current_observation(started + Duration::from_secs(5))
            .unwrap()
            .complete(unowned.clone(), started + Duration::from_secs(6),)
            .unwrap(),
        RuntimeGatewayOwnerObservationCompletionV1::OwnershipLost(unowned)
    );

    let foreign_id = RuntimeGatewayOwnerLeaseIdV1 {
        process_instance_id: ProcessInstanceId::parse("process:2").unwrap(),
        lease_epoch: non_zero(8),
        ..lease_id()
    };
    let foreign = owned_observation(foreign_id, 1, 110, 130);
    let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(3, 30),
        policy(),
        started,
        started + Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(
        watchdog
            .begin_current_observation(started + Duration::from_secs(5))
            .unwrap()
            .complete(foreign.clone(), started + Duration::from_secs(6),)
            .unwrap(),
        RuntimeGatewayOwnerObservationCompletionV1::OwnershipLost(foreign)
    );
}

#[test]
fn unexplained_current_lease_changes_are_protocol_violations() {
    let started = Instant::now();
    for (observation, violation) in [
        (
            owned_observation(lease_id(), 4, 110, 130),
            RuntimeGatewayOwnerProtocolViolationV1::RevisionMismatch,
        ),
        (
            owned_observation(lease_id(), 3, 110, 131),
            RuntimeGatewayOwnerProtocolViolationV1::InconsistentOutcome,
        ),
        (
            owned_observation(lease_id(), 3, 99, 130),
            RuntimeGatewayOwnerProtocolViolationV1::InconsistentOutcome,
        ),
    ] {
        let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
            accepted_receipt(3, 30),
            policy(),
            started,
            started + Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!(
            watchdog
                .begin_current_observation(started + Duration::from_secs(5))
                .unwrap()
                .complete(observation, started + Duration::from_secs(6)),
            Err(RuntimeGatewayOwnerWatchdogErrorV1::ProtocolViolation { violation })
        );
    }
}

#[test]
fn observation_clock_and_safety_boundaries_fail_closed() {
    let started = Instant::now();
    let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(3, 30),
        policy(),
        started,
        started + Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(
        watchdog.begin_current_observation(started + Duration::from_secs(1)),
        Err(RuntimeGatewayOwnerWatchdogErrorV1::ClockReversed)
    );

    let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(3, 30),
        policy(),
        started,
        started + Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(
        watchdog.begin_current_observation(started + Duration::from_secs(27)),
        Err(RuntimeGatewayOwnerWatchdogErrorV1::SafetyElapsed)
    );

    let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(3, 30),
        policy(),
        started,
        started + Duration::from_secs(2),
    )
    .unwrap();
    let inflight = watchdog
        .begin_current_observation(started + Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        inflight.complete(
            owned_observation(lease_id(), 3, 120, 130),
            started + Duration::from_secs(4),
        ),
        Err(RuntimeGatewayOwnerWatchdogErrorV1::ClockReversed)
    );

    let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(3, 30),
        policy(),
        started,
        started + Duration::from_secs(2),
    )
    .unwrap();
    let inflight = watchdog
        .begin_current_observation(started + Duration::from_secs(26))
        .unwrap();
    assert_eq!(
        inflight.complete(
            owned_observation(lease_id(), 3, 110, 130),
            started + Duration::from_secs(27),
        ),
        Err(RuntimeGatewayOwnerWatchdogErrorV1::SafetyElapsed)
    );

    let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(3, 30),
        policy(),
        started,
        started + Duration::from_secs(2),
    )
    .unwrap();
    let inflight = watchdog
        .begin_current_observation(started + Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        inflight.complete(
            owned_observation(lease_id(), 3, 110, 130),
            started + Duration::from_secs(22),
        ),
        Err(RuntimeGatewayOwnerWatchdogErrorV1::Schedule(
            RuntimeGatewayOwnerRenewalScheduleErrorV1::SafetyElapsed
        ))
    );
}

#[test]
fn observation_inside_the_renewal_window_remains_current_and_renews_now() {
    let started = Instant::now();
    for (database_now, safety_second, expiry_second) in [(120, 12, 15), (121, 11, 14)] {
        let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
            accepted_receipt(3, 30),
            policy(),
            started,
            started + Duration::from_secs(2),
        )
        .unwrap();
        let completion = watchdog
            .begin_current_observation(started + Duration::from_secs(5))
            .unwrap()
            .complete(
                owned_observation(lease_id(), 3, database_now, 130),
                started + Duration::from_secs(6),
            )
            .unwrap();
        let RuntimeGatewayOwnerObservationCompletionV1::Current(current) = completion else {
            panic!("expected current owner observation")
        };

        assert_eq!(
            current.schedule().renew_at(),
            started + Duration::from_secs(5)
        );
        assert_eq!(
            current.action_at(started + Duration::from_secs(6)),
            RuntimeGatewayOwnerWatchdogActionV1::RenewNow
        );
        assert_eq!(
            current.schedule().safety_deadline(),
            started + Duration::from_secs(safety_second)
        );
        assert_eq!(
            current.schedule().conservative_expiry(),
            started + Duration::from_secs(expiry_second)
        );
    }
}

#[test]
fn observation_failure_restores_only_before_the_existing_safety_deadline() {
    let started = Instant::now();
    let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(3, 30),
        policy(),
        started,
        started + Duration::from_secs(2),
    )
    .unwrap();
    let restored = watchdog
        .begin_current_observation(started + Duration::from_secs(5))
        .unwrap()
        .observation_failed(started + Duration::from_secs(6))
        .unwrap();

    assert_eq!(restored.schedule().receipt().database_now, at(100));
    assert_eq!(restored.schedule().request_started_at(), started);
    assert_eq!(
        restored.schedule().response_observed_at(),
        started + Duration::from_secs(2)
    );
    assert_eq!(
        restored.schedule().conservative_expiry(),
        started + Duration::from_secs(30)
    );
    assert_eq!(
        restored.schedule().renew_at(),
        started + Duration::from_secs(20)
    );
    assert_eq!(
        restored.schedule().safety_deadline(),
        started + Duration::from_secs(27)
    );

    let inflight = restored
        .begin_current_observation(started + Duration::from_secs(7))
        .unwrap();
    assert_eq!(
        inflight.observation_failed(started + Duration::from_secs(6)),
        Err(RuntimeGatewayOwnerWatchdogErrorV1::ClockReversed)
    );

    let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(3, 30),
        policy(),
        started,
        started + Duration::from_secs(2),
    )
    .unwrap();
    let inflight = watchdog
        .begin_current_observation(started + Duration::from_secs(7))
        .unwrap();
    assert_eq!(
        inflight.observation_failed(started + Duration::from_secs(27)),
        Err(RuntimeGatewayOwnerWatchdogErrorV1::SafetyElapsed)
    );
}

#[test]
fn wrong_shard_and_nonfresh_observations_are_protocol_violations() {
    let started = Instant::now();
    let wrong_shard = GatewayShardIdV1::parse("shard:1").unwrap();
    let wrong_shard_id = RuntimeGatewayOwnerLeaseIdV1 {
        gateway_shard_id: wrong_shard.clone(),
        ..lease_id()
    };
    for (observation, violation) in [
        (
            RuntimeGatewayOwnerLeaseObservationV1::Unowned {
                gateway_shard_id: wrong_shard,
                database_now: at(110),
            },
            RuntimeGatewayOwnerProtocolViolationV1::ShardMismatch,
        ),
        (
            owned_observation(wrong_shard_id, 3, 110, 130),
            RuntimeGatewayOwnerProtocolViolationV1::ShardMismatch,
        ),
        (
            owned_observation(lease_id(), 3, 130, 130),
            RuntimeGatewayOwnerProtocolViolationV1::NonFreshReceipt,
        ),
    ] {
        let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
            accepted_receipt(3, 30),
            policy(),
            started,
            started + Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!(
            watchdog
                .begin_current_observation(started + Duration::from_secs(5))
                .unwrap()
                .complete(observation, started + Duration::from_secs(6)),
            Err(RuntimeGatewayOwnerWatchdogErrorV1::ProtocolViolation { violation })
        );
    }
}

#[test]
fn observation_without_positive_safety_remaining_fails_closed() {
    let started = Instant::now();
    let watchdog = RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt(3, 30),
        policy(),
        started,
        started + Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(
        watchdog
            .begin_current_observation(started + Duration::from_secs(5))
            .unwrap()
            .complete(
                owned_observation(lease_id(), 3, 127, 130),
                started + Duration::from_secs(6),
            ),
        Err(RuntimeGatewayOwnerWatchdogErrorV1::Schedule(
            RuntimeGatewayOwnerRenewalScheduleErrorV1::SafetyElapsed
        ))
    );
}
