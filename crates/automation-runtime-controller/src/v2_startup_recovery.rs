use std::time::Duration;

use chrono::{DateTime, Utc};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeStartupServingStateV2 {
    Empty,
    RecoverableStale {
        count: u32,
    },
    ForeignFresh {
        count: u32,
        database_now: DateTime<Utc>,
        earliest_expiry: DateTime<Utc>,
        retry_after: Duration,
    },
    Ambiguous,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeStartupRecoveryStateV2 {
    pub serving: RuntimeStartupServingStateV2,
    pub recoverable_awaiting_certification_count: u32,
    pub suspended_local_effect_count: u32,
    pub pending_runtime_drain_intent_count: u32,
    pub acknowledged_product_handoff_count: u32,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::{DateTime, Utc};

    use super::{RuntimeStartupRecoveryStateV2, RuntimeStartupServingStateV2};

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(second, 0).unwrap()
    }

    #[test]
    fn startup_recovery_state_preserves_every_closed_observation_class() {
        let state = RuntimeStartupRecoveryStateV2 {
            serving: RuntimeStartupServingStateV2::ForeignFresh {
                count: 2,
                database_now: at(100),
                earliest_expiry: at(110),
                retry_after: Duration::from_secs(5),
            },
            recoverable_awaiting_certification_count: 3,
            suspended_local_effect_count: 4,
            pending_runtime_drain_intent_count: 5,
            acknowledged_product_handoff_count: 6,
        };

        assert_eq!(state.recoverable_awaiting_certification_count, 3);
        assert_eq!(state.suspended_local_effect_count, 4);
        assert_eq!(state.pending_runtime_drain_intent_count, 5);
        assert_eq!(state.acknowledged_product_handoff_count, 6);
        assert!(matches!(
            state.serving,
            RuntimeStartupServingStateV2::ForeignFresh { count: 2, .. }
        ));
    }
}
