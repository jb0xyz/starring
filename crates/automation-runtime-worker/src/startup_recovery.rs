use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::num::NonZeroU64;
use std::time::{Duration, Instant};

use automation_runtime_controller::{
    RuntimeGatewayOwnerLeaseReceiptV1, RuntimeStartupRecoveryObservationCorrelationV2,
    RuntimeStartupRecoveryObservationReceiptV2, RuntimeStartupRecoveryObservationRequestV2,
    RuntimeStartupRecoveryStateV2, RuntimeStartupServingStateV2,
};
use chrono::{DateTime, Utc};

use crate::closed_recovery::RuntimeClosedRecoveryOperationAuthorityV2;
use crate::{
    RuntimeClosedDrainRecoveryPermitV2, RuntimeClosedRecoveryAuthorityRevisionV2,
    RuntimeGatewayCoordinatorGenerationV2,
};

#[derive(PartialEq, Eq)]
pub struct RuntimeAuthorizedStartupRecoveryIterationV2 {
    request: RuntimeStartupRecoveryObservationRequestV2,
    operation_authority: RuntimeClosedRecoveryOperationAuthorityV2,
}

impl Debug for RuntimeAuthorizedStartupRecoveryIterationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAuthorizedStartupRecoveryIterationV2(<redacted>)")
    }
}

pub struct RuntimeAuthorizedStartupRecoveryObservationV2 {
    request: RuntimeStartupRecoveryObservationRequestV2,
    minimum_database_now: DateTime<Utc>,
    operation_authority: RuntimeClosedRecoveryOperationAuthorityV2,
}

impl RuntimeAuthorizedStartupRecoveryObservationV2 {
    pub fn request(&self) -> &RuntimeStartupRecoveryObservationRequestV2 {
        &self.request
    }

    pub fn complete(
        self,
        receipt: RuntimeStartupRecoveryObservationReceiptV2,
    ) -> RuntimeCompletedStartupRecoveryObservationV2 {
        RuntimeCompletedStartupRecoveryObservationV2 {
            authorization: self,
            receipt,
        }
    }
}

impl Debug for RuntimeAuthorizedStartupRecoveryObservationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAuthorizedStartupRecoveryObservationV2(<redacted>)")
    }
}

pub struct RuntimeCompletedStartupRecoveryObservationV2 {
    authorization: RuntimeAuthorizedStartupRecoveryObservationV2,
    receipt: RuntimeStartupRecoveryObservationReceiptV2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeStartupRecoveryContinuationV2 {
    Recover(RuntimeStartupRecoveryClassV2),
    WaitForForeignFresh { retry_after: Duration },
}

#[derive(PartialEq, Eq)]
pub struct RuntimeStartupRecoveryFixedPointProofV2 {
    operation_authority: RuntimeClosedRecoveryOperationAuthorityV2,
    correlation: RuntimeStartupRecoveryObservationCorrelationV2,
    successor_authority_revision: RuntimeClosedRecoveryAuthorityRevisionV2,
    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    acknowledged_product_handoff_count: u32,
}

impl RuntimeStartupRecoveryFixedPointProofV2 {
    pub fn successor_authority_revision(&self) -> RuntimeClosedRecoveryAuthorityRevisionV2 {
        self.successor_authority_revision
    }

    pub fn acknowledged_product_handoff_count(&self) -> u32 {
        self.acknowledged_product_handoff_count
    }
}

impl Debug for RuntimeStartupRecoveryFixedPointProofV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let _ = (
            &self.operation_authority,
            &self.correlation,
            &self.owner_receipt,
        );
        formatter.write_str("RuntimeStartupRecoveryFixedPointProofV2(<redacted>)")
    }
}

#[derive(PartialEq, Eq)]
pub enum RuntimeAcceptedStartupRecoveryOutcomeV2 {
    Continue(RuntimeStartupRecoveryContinuationV2),
    FixedPoint(RuntimeStartupRecoveryFixedPointProofV2),
}

impl Debug for RuntimeAcceptedStartupRecoveryOutcomeV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAcceptedStartupRecoveryOutcomeV2(<redacted>)")
    }
}

impl Debug for RuntimeCompletedStartupRecoveryObservationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCompletedStartupRecoveryObservationV2(<redacted>)")
    }
}

pub trait RuntimeStartupRecoveryObservationPortV2 {
    type Error;

    fn observe_startup_recovery(
        &self,
        authorization: RuntimeAuthorizedStartupRecoveryObservationV2,
        operation_cutoff: Instant,
    ) -> impl Future<Output = Result<RuntimeCompletedStartupRecoveryObservationV2, Self::Error>> + Send;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeStartupRecoveryObservationAcceptanceErrorV2 {
    #[error("runtime startup recovery observation call correlation does not match")]
    CorrelationMismatch,
    #[error("runtime startup recovery observation owner does not match")]
    OwnerMismatch,
    #[error("runtime startup recovery observation database clock regressed")]
    DatabaseClockRegressed,
    #[error("runtime startup recovery observation owner is not current")]
    OwnerNotCurrent,
    #[error("runtime startup recovery observation database time does not match")]
    DatabaseTimeMismatch,
    #[error("runtime startup recovery observation is ambiguous")]
    Ambiguous,
    #[error("runtime startup recovery observation is invalid")]
    InvalidObservation,
}

pub(crate) struct RuntimeValidatedStartupRecoveryObservationV2 {
    operation_authority: RuntimeClosedRecoveryOperationAuthorityV2,
    request: RuntimeStartupRecoveryObservationRequestV2,
    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    decision: RuntimeStartupRecoveryDecisionV2,
}

impl RuntimeValidatedStartupRecoveryObservationV2 {
    pub(crate) fn into_parts(
        self,
    ) -> (
        RuntimeClosedRecoveryOperationAuthorityV2,
        RuntimeStartupRecoveryObservationRequestV2,
        RuntimeGatewayOwnerLeaseReceiptV1,
        RuntimeStartupRecoveryDecisionV2,
    ) {
        (
            self.operation_authority,
            self.request,
            self.owner_receipt,
            self.decision,
        )
    }
}

pub(crate) fn authorize_startup_recovery_iteration_v2(
    permit: &RuntimeClosedDrainRecoveryPermitV2,
    operation_authority: RuntimeClosedRecoveryOperationAuthorityV2,
) -> RuntimeAuthorizedStartupRecoveryIterationV2 {
    RuntimeAuthorizedStartupRecoveryIterationV2 {
        request: startup_recovery_observation_request_v2(permit),
        operation_authority,
    }
}

pub(crate) fn authorize_startup_recovery_observation_v2(
    permit: &RuntimeClosedDrainRecoveryPermitV2,
    iteration: RuntimeAuthorizedStartupRecoveryIterationV2,
) -> Option<RuntimeAuthorizedStartupRecoveryObservationV2> {
    let RuntimeAuthorizedStartupRecoveryIterationV2 {
        request,
        operation_authority,
    } = iteration;
    if request != startup_recovery_observation_request_v2(permit) {
        return None;
    }
    let minimum_database_now = permit
        .last_startup_observation_database_now()
        .unwrap_or(permit.owner_receipt().database_now)
        .max(permit.owner_receipt().database_now);
    Some(RuntimeAuthorizedStartupRecoveryObservationV2 {
        request,
        minimum_database_now,
        operation_authority,
    })
}

pub(crate) fn validate_startup_recovery_observation_v2(
    permit: &RuntimeClosedDrainRecoveryPermitV2,
    completed: RuntimeCompletedStartupRecoveryObservationV2,
) -> Result<
    RuntimeValidatedStartupRecoveryObservationV2,
    RuntimeStartupRecoveryObservationAcceptanceErrorV2,
> {
    let RuntimeCompletedStartupRecoveryObservationV2 {
        authorization,
        receipt,
    } = completed;
    let RuntimeAuthorizedStartupRecoveryObservationV2 {
        request,
        minimum_database_now,
        operation_authority,
    } = authorization;
    if request != startup_recovery_observation_request_v2(permit)
        || receipt.correlation != request.correlation
    {
        return Err(RuntimeStartupRecoveryObservationAcceptanceErrorV2::CorrelationMismatch);
    }
    let observed_owner = &receipt.owner_receipt;
    if observed_owner.lease_id != request.gateway_owner_lease_id
        || observed_owner.owner_revision != request.expected_owner_revision
        || observed_owner.expires_at != request.expected_owner_expires_at
    {
        return Err(RuntimeStartupRecoveryObservationAcceptanceErrorV2::OwnerMismatch);
    }
    if observed_owner.database_now < minimum_database_now {
        return Err(RuntimeStartupRecoveryObservationAcceptanceErrorV2::DatabaseClockRegressed);
    }
    if observed_owner.database_lease_duration().is_none() {
        return Err(RuntimeStartupRecoveryObservationAcceptanceErrorV2::OwnerNotCurrent);
    }
    if matches!(
        &receipt.state.serving,
        RuntimeStartupServingStateV2::ForeignFresh { database_now, .. }
            if *database_now != observed_owner.database_now
    ) {
        return Err(RuntimeStartupRecoveryObservationAcceptanceErrorV2::DatabaseTimeMismatch);
    }
    let decision =
        plan_runtime_startup_recovery_v2(receipt.state).map_err(|error| match error {
            RuntimeStartupRecoveryPlanErrorV2::Ambiguous => {
                RuntimeStartupRecoveryObservationAcceptanceErrorV2::Ambiguous
            }
            RuntimeStartupRecoveryPlanErrorV2::InvalidObservation => {
                RuntimeStartupRecoveryObservationAcceptanceErrorV2::InvalidObservation
            }
        })?;
    Ok(RuntimeValidatedStartupRecoveryObservationV2 {
        operation_authority,
        request,
        owner_receipt: receipt.owner_receipt,
        decision,
    })
}

pub(crate) fn accept_validated_startup_recovery_observation_v2(
    permit: &mut RuntimeClosedDrainRecoveryPermitV2,
    validated: RuntimeValidatedStartupRecoveryObservationV2,
) -> Option<(
    RuntimeClosedRecoveryAuthorityRevisionV2,
    RuntimeAcceptedStartupRecoveryOutcomeV2,
)> {
    let (operation_authority, request, owner_receipt, decision) = validated.into_parts();
    let database_now = owner_receipt.database_now;
    match decision {
        RuntimeStartupRecoveryDecisionV2::Recover(class) => {
            let authority_revision = permit.restore_operation_authority_for_recovery(
                operation_authority,
                database_now,
                class,
                request.correlation,
            )?;
            Some((
                authority_revision,
                RuntimeAcceptedStartupRecoveryOutcomeV2::Continue(
                    RuntimeStartupRecoveryContinuationV2::Recover(class),
                ),
            ))
        }
        RuntimeStartupRecoveryDecisionV2::WaitForForeignFresh { retry_after } => {
            let authority_revision =
                permit.restore_operation_authority(operation_authority, database_now)?;
            Some((
                authority_revision,
                RuntimeAcceptedStartupRecoveryOutcomeV2::Continue(
                    RuntimeStartupRecoveryContinuationV2::WaitForForeignFresh { retry_after },
                ),
            ))
        }
        RuntimeStartupRecoveryDecisionV2::FixedPoint(fixed_point) => {
            let authority_revision = permit.advance_fixed_point(database_now)?;
            Some((
                authority_revision,
                RuntimeAcceptedStartupRecoveryOutcomeV2::FixedPoint(
                    RuntimeStartupRecoveryFixedPointProofV2 {
                        operation_authority,
                        correlation: request.correlation,
                        successor_authority_revision: authority_revision,
                        owner_receipt,
                        acknowledged_product_handoff_count: fixed_point
                            .acknowledged_product_handoff_count,
                    },
                ),
            ))
        }
    }
}

pub(crate) fn startup_recovery_fixed_point_matches_permit_v2(
    permit: &RuntimeClosedDrainRecoveryPermitV2,
    proof: &RuntimeStartupRecoveryFixedPointProofV2,
) -> bool {
    let owner = permit.owner_receipt();
    !permit.operation_authority_is_available()
        && proof.correlation.recovery_id == *permit.recovery_id()
        && proof.correlation.originating_emergency_generation
            == generation_value(permit.originating_emergency_generation())
        && proof.correlation.coordinator_generation
            == generation_value(permit.coordinator_generation())
        && proof.correlation.authority_revision.get().checked_add(1)
            == Some(proof.successor_authority_revision.get())
        && proof.successor_authority_revision == permit.authority_revision()
        && proof.owner_receipt.lease_id == owner.lease_id
        && proof.owner_receipt.owner_revision == owner.owner_revision
        && proof.owner_receipt.expires_at == owner.expires_at
        && proof.owner_receipt.database_now >= owner.database_now
        && proof.owner_receipt.database_lease_duration().is_some()
        && permit.last_startup_observation_database_now() == Some(proof.owner_receipt.database_now)
}

fn startup_recovery_observation_request_v2(
    permit: &RuntimeClosedDrainRecoveryPermitV2,
) -> RuntimeStartupRecoveryObservationRequestV2 {
    let owner = permit.owner_receipt();
    RuntimeStartupRecoveryObservationRequestV2 {
        correlation: RuntimeStartupRecoveryObservationCorrelationV2 {
            recovery_id: permit.recovery_id().clone(),
            originating_emergency_generation: generation_value(
                permit.originating_emergency_generation(),
            ),
            coordinator_generation: generation_value(permit.coordinator_generation()),
            authority_revision: NonZeroU64::new(permit.authority_revision().get())
                .expect("closed recovery authority revision is nonzero"),
        },
        gateway_owner_lease_id: owner.lease_id.clone(),
        expected_owner_revision: owner.owner_revision,
        expected_owner_expires_at: owner.expires_at,
    }
}

fn generation_value(generation: RuntimeGatewayCoordinatorGenerationV2) -> NonZeroU64 {
    NonZeroU64::new(generation.get()).expect("gateway coordinator generation is nonzero")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeStartupRecoveryClassV2 {
    StaleLive,
    ReservedAwaitingCertification,
    SuspendedLocalEffect,
    PendingRuntimeDrainIntent,
}

#[derive(Debug, PartialEq, Eq)]
pub struct RuntimeStartupRecoveryObservationFixedPointV2 {
    acknowledged_product_handoff_count: u32,
}

impl RuntimeStartupRecoveryObservationFixedPointV2 {
    pub fn acknowledged_product_handoff_count(&self) -> u32 {
        self.acknowledged_product_handoff_count
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeStartupRecoveryDecisionV2 {
    Recover(RuntimeStartupRecoveryClassV2),
    WaitForForeignFresh { retry_after: Duration },
    FixedPoint(RuntimeStartupRecoveryObservationFixedPointV2),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeStartupRecoveryPlanErrorV2 {
    #[error("runtime startup recovery observation is ambiguous")]
    Ambiguous,
    #[error("runtime startup recovery observation is invalid")]
    InvalidObservation,
}

pub fn plan_runtime_startup_recovery_v2(
    state: RuntimeStartupRecoveryStateV2,
) -> Result<RuntimeStartupRecoveryDecisionV2, RuntimeStartupRecoveryPlanErrorV2> {
    let serving = classify_serving_v2(state.serving)?;
    if serving == RuntimeStartupServingClassificationV2::RecoverableStale {
        return Ok(RuntimeStartupRecoveryDecisionV2::Recover(
            RuntimeStartupRecoveryClassV2::StaleLive,
        ));
    }
    if state.recoverable_awaiting_certification_count > 0 {
        return Ok(RuntimeStartupRecoveryDecisionV2::Recover(
            RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification,
        ));
    }
    if state.suspended_local_effect_count > 0 {
        return Ok(RuntimeStartupRecoveryDecisionV2::Recover(
            RuntimeStartupRecoveryClassV2::SuspendedLocalEffect,
        ));
    }
    if state.pending_runtime_drain_intent_count > 0 {
        return Ok(RuntimeStartupRecoveryDecisionV2::Recover(
            RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent,
        ));
    }
    match serving {
        RuntimeStartupServingClassificationV2::Empty => {
            Ok(RuntimeStartupRecoveryDecisionV2::FixedPoint(
                RuntimeStartupRecoveryObservationFixedPointV2 {
                    acknowledged_product_handoff_count: state.acknowledged_product_handoff_count,
                },
            ))
        }
        RuntimeStartupServingClassificationV2::ForeignFresh { retry_after } => {
            Ok(RuntimeStartupRecoveryDecisionV2::WaitForForeignFresh { retry_after })
        }
        RuntimeStartupServingClassificationV2::RecoverableStale => unreachable!(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeStartupServingClassificationV2 {
    Empty,
    RecoverableStale,
    ForeignFresh { retry_after: Duration },
}

fn classify_serving_v2(
    serving: RuntimeStartupServingStateV2,
) -> Result<RuntimeStartupServingClassificationV2, RuntimeStartupRecoveryPlanErrorV2> {
    match serving {
        RuntimeStartupServingStateV2::Empty => Ok(RuntimeStartupServingClassificationV2::Empty),
        RuntimeStartupServingStateV2::RecoverableStale { count } if count > 0 => {
            Ok(RuntimeStartupServingClassificationV2::RecoverableStale)
        }
        RuntimeStartupServingStateV2::RecoverableStale { .. } => {
            Err(RuntimeStartupRecoveryPlanErrorV2::InvalidObservation)
        }
        RuntimeStartupServingStateV2::ForeignFresh {
            count,
            database_now,
            earliest_expiry,
            retry_after,
        } => {
            let available = earliest_expiry
                .signed_duration_since(database_now)
                .to_std()
                .map_err(|_| RuntimeStartupRecoveryPlanErrorV2::InvalidObservation)?;
            if count == 0 || retry_after.is_zero() || retry_after > available {
                return Err(RuntimeStartupRecoveryPlanErrorV2::InvalidObservation);
            }
            Ok(RuntimeStartupServingClassificationV2::ForeignFresh { retry_after })
        }
        RuntimeStartupServingStateV2::Ambiguous => {
            Err(RuntimeStartupRecoveryPlanErrorV2::Ambiguous)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use automation_runtime_controller::{
        RuntimeStartupRecoveryStateV2, RuntimeStartupServingStateV2,
    };
    use chrono::{DateTime, Utc};

    use super::{
        plan_runtime_startup_recovery_v2, RuntimeStartupRecoveryClassV2,
        RuntimeStartupRecoveryDecisionV2, RuntimeStartupRecoveryPlanErrorV2,
    };

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(second, 0).unwrap()
    }

    fn state(serving: RuntimeStartupServingStateV2) -> RuntimeStartupRecoveryStateV2 {
        RuntimeStartupRecoveryStateV2 {
            serving,
            recoverable_awaiting_certification_count: 0,
            suspended_local_effect_count: 0,
            pending_runtime_drain_intent_count: 0,
            acknowledged_product_handoff_count: 0,
        }
    }

    fn foreign(retry_after: Duration) -> RuntimeStartupServingStateV2 {
        RuntimeStartupServingStateV2::ForeignFresh {
            count: 1,
            database_now: at(100),
            earliest_expiry: at(110),
            retry_after,
        }
    }

    #[test]
    fn empty_observation_mints_the_narrow_nonblocking_fixed_point() {
        let mut observation = state(RuntimeStartupServingStateV2::Empty);
        observation.acknowledged_product_handoff_count = 7;

        let RuntimeStartupRecoveryDecisionV2::FixedPoint(fixed_point) =
            plan_runtime_startup_recovery_v2(observation).unwrap()
        else {
            panic!("expected fixed point")
        };

        assert_eq!(fixed_point.acknowledged_product_handoff_count(), 7);
    }

    #[test]
    fn stale_live_has_priority_over_every_other_recoverable_class() {
        let mut observation = state(RuntimeStartupServingStateV2::RecoverableStale { count: 2 });
        observation.recoverable_awaiting_certification_count = 1;
        observation.suspended_local_effect_count = 1;
        observation.pending_runtime_drain_intent_count = 1;

        assert_eq!(
            plan_runtime_startup_recovery_v2(observation),
            Ok(RuntimeStartupRecoveryDecisionV2::Recover(
                RuntimeStartupRecoveryClassV2::StaleLive,
            ))
        );
    }

    #[test]
    fn runtime_obligations_are_recovered_before_waiting_for_foreign_serving() {
        let mut awaiting = state(foreign(Duration::from_secs(5)));
        awaiting.recoverable_awaiting_certification_count = 1;
        assert_eq!(
            plan_runtime_startup_recovery_v2(awaiting),
            Ok(RuntimeStartupRecoveryDecisionV2::Recover(
                RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification,
            ))
        );

        let mut suspended = state(foreign(Duration::from_secs(5)));
        suspended.suspended_local_effect_count = 1;
        assert_eq!(
            plan_runtime_startup_recovery_v2(suspended),
            Ok(RuntimeStartupRecoveryDecisionV2::Recover(
                RuntimeStartupRecoveryClassV2::SuspendedLocalEffect,
            ))
        );

        let mut drain = state(foreign(Duration::from_secs(5)));
        drain.pending_runtime_drain_intent_count = 1;
        assert_eq!(
            plan_runtime_startup_recovery_v2(drain),
            Ok(RuntimeStartupRecoveryDecisionV2::Recover(
                RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent,
            ))
        );
    }

    #[test]
    fn recoverable_classes_follow_a_stable_priority() {
        let mut observation = state(RuntimeStartupServingStateV2::Empty);
        observation.recoverable_awaiting_certification_count = 1;
        observation.suspended_local_effect_count = 1;
        observation.pending_runtime_drain_intent_count = 1;
        assert_eq!(
            plan_runtime_startup_recovery_v2(observation),
            Ok(RuntimeStartupRecoveryDecisionV2::Recover(
                RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification,
            ))
        );

        let mut observation = state(RuntimeStartupServingStateV2::Empty);
        observation.suspended_local_effect_count = 1;
        observation.pending_runtime_drain_intent_count = 1;
        assert_eq!(
            plan_runtime_startup_recovery_v2(observation),
            Ok(RuntimeStartupRecoveryDecisionV2::Recover(
                RuntimeStartupRecoveryClassV2::SuspendedLocalEffect,
            ))
        );
    }

    #[test]
    fn foreign_serving_returns_only_the_database_bounded_wait() {
        assert_eq!(
            plan_runtime_startup_recovery_v2(state(foreign(Duration::from_secs(5)))),
            Ok(RuntimeStartupRecoveryDecisionV2::WaitForForeignFresh {
                retry_after: Duration::from_secs(5),
            })
        );
    }

    #[test]
    fn ambiguous_serving_fails_closed_before_other_recovery() {
        let mut observation = state(RuntimeStartupServingStateV2::Ambiguous);
        observation.recoverable_awaiting_certification_count = 1;

        assert_eq!(
            plan_runtime_startup_recovery_v2(observation),
            Err(RuntimeStartupRecoveryPlanErrorV2::Ambiguous)
        );
    }

    #[test]
    fn malformed_serving_counts_and_waits_fail_closed() {
        for serving in [
            RuntimeStartupServingStateV2::RecoverableStale { count: 0 },
            RuntimeStartupServingStateV2::ForeignFresh {
                count: 0,
                database_now: at(100),
                earliest_expiry: at(110),
                retry_after: Duration::from_secs(5),
            },
            foreign(Duration::ZERO),
            foreign(Duration::from_secs(11)),
            RuntimeStartupServingStateV2::ForeignFresh {
                count: 1,
                database_now: at(110),
                earliest_expiry: at(100),
                retry_after: Duration::from_secs(1),
            },
        ] {
            assert_eq!(
                plan_runtime_startup_recovery_v2(state(serving)),
                Err(RuntimeStartupRecoveryPlanErrorV2::InvalidObservation)
            );
        }
    }
}
