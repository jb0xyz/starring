use std::time::Duration;

use automation_runtime_controller::{RuntimeStartupRecoveryStateV2, RuntimeStartupServingStateV2};

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
