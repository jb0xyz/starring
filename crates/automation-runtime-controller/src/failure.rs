use automation_runtime_convergence::RuntimeFailureKindV1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeFailureSourceV1 {
    DatabaseUnavailable,
    DatabaseConcurrency,
    DiscordUnavailable,
    DiscordRateLimited,
    DrainTimeout,
    PanelTransient,
    GatewayStart,
    GatewayReadyTimeout,
    ArtifactMissing,
    ArtifactHashMismatch,
    BindingMismatch,
    UnsupportedSchema,
    PanelUnresolvedChannel,
    PanelAmbiguous,
    PanelCleanupPending,
    PersistedStateInvalid,
    ProductAuthorityInactive,
    ActiveTargetChanged,
    ShutdownRequested,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeRecordedFailureV1 {
    pub kind: RuntimeFailureKindV1,
    pub code: &'static str,
    pub message: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeFailureDecisionV1 {
    Retryable(RuntimeRecordedFailureV1),
    Blocked(RuntimeRecordedFailureV1),
    Superseded,
    Stop,
}

impl RuntimeFailureSourceV1 {
    pub fn decide(self) -> RuntimeFailureDecisionV1 {
        use RuntimeFailureDecisionV1::{Blocked, Retryable};
        match self {
            Self::DatabaseUnavailable => Retryable(recorded(
                RuntimeFailureKindV1::EnvironmentUnavailable,
                "runtime_database_unavailable",
                "runtime database is temporarily unavailable",
            )),
            Self::DatabaseConcurrency => Retryable(recorded(
                RuntimeFailureKindV1::EnvironmentUnavailable,
                "runtime_database_concurrency",
                "runtime database transaction must be retried",
            )),
            Self::DiscordUnavailable => Retryable(recorded(
                RuntimeFailureKindV1::EnvironmentUnavailable,
                "runtime_discord_unavailable",
                "Discord is temporarily unavailable",
            )),
            Self::DiscordRateLimited => Retryable(recorded(
                RuntimeFailureKindV1::EnvironmentUnavailable,
                "runtime_discord_rate_limited",
                "Discord requested bounded backoff",
            )),
            Self::DrainTimeout => Retryable(recorded(
                RuntimeFailureKindV1::ActivationNotObservable,
                "runtime_drain_timeout",
                "the previous runtime did not drain before its deadline",
            )),
            Self::PanelTransient => Retryable(recorded(
                RuntimeFailureKindV1::PanelReconciliation,
                "runtime_panel_transient",
                "panel reconciliation encountered a transient dependency failure",
            )),
            Self::GatewayStart => Retryable(recorded(
                RuntimeFailureKindV1::GatewayStart,
                "runtime_gateway_start",
                "the Discord gateway could not start",
            )),
            Self::GatewayReadyTimeout => Retryable(recorded(
                RuntimeFailureKindV1::GatewayReadyTimeout,
                "runtime_gateway_ready_timeout",
                "the Discord gateway did not become ready before its deadline",
            )),
            Self::ArtifactMissing => Blocked(recorded(
                RuntimeFailureKindV1::InvariantViolation,
                "runtime_artifact_missing",
                "the exact approved RuleSet artifact is missing",
            )),
            Self::ArtifactHashMismatch => Blocked(recorded(
                RuntimeFailureKindV1::InvariantViolation,
                "runtime_artifact_hash_mismatch",
                "the approved RuleSet artifact failed integrity verification",
            )),
            Self::BindingMismatch => Blocked(recorded(
                RuntimeFailureKindV1::InvariantViolation,
                "runtime_binding_mismatch",
                "the current resource bindings differ from the approved target",
            )),
            Self::UnsupportedSchema => Blocked(recorded(
                RuntimeFailureKindV1::InvariantViolation,
                "runtime_schema_unsupported",
                "the approved RuleSet schema is unsupported by this runtime",
            )),
            Self::PanelUnresolvedChannel => Blocked(recorded(
                RuntimeFailureKindV1::PanelReconciliation,
                "runtime_panel_channel_unresolved",
                "a declared panel channel binding could not be resolved",
            )),
            Self::PanelAmbiguous => Blocked(recorded(
                RuntimeFailureKindV1::PanelReconciliation,
                "runtime_panel_ambiguous",
                "panel reconciliation produced an ambiguous external outcome",
            )),
            Self::PanelCleanupPending => Blocked(recorded(
                RuntimeFailureKindV1::PanelReconciliation,
                "runtime_panel_cleanup_pending",
                "panel reconciliation left external cleanup pending",
            )),
            Self::PersistedStateInvalid => Blocked(recorded(
                RuntimeFailureKindV1::InvariantViolation,
                "runtime_persisted_state_invalid",
                "persisted runtime state violates the deployment contract",
            )),
            Self::ProductAuthorityInactive => Blocked(recorded(
                RuntimeFailureKindV1::InvariantViolation,
                "runtime_product_authority_inactive",
                "the deployment product authority is no longer active",
            )),
            Self::ActiveTargetChanged => RuntimeFailureDecisionV1::Superseded,
            Self::ShutdownRequested => RuntimeFailureDecisionV1::Stop,
        }
    }
}

fn recorded(
    kind: RuntimeFailureKindV1,
    code: &'static str,
    message: &'static str,
) -> RuntimeRecordedFailureV1 {
    RuntimeRecordedFailureV1 {
        kind,
        code,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_dependencies_retry() {
        for source in [
            RuntimeFailureSourceV1::DatabaseUnavailable,
            RuntimeFailureSourceV1::DatabaseConcurrency,
            RuntimeFailureSourceV1::DiscordUnavailable,
            RuntimeFailureSourceV1::DiscordRateLimited,
            RuntimeFailureSourceV1::PanelTransient,
            RuntimeFailureSourceV1::GatewayStart,
            RuntimeFailureSourceV1::GatewayReadyTimeout,
        ] {
            assert!(matches!(
                source.decide(),
                RuntimeFailureDecisionV1::Retryable(_)
            ));
        }
    }

    #[test]
    fn integrity_and_ambiguity_fail_closed() {
        for source in [
            RuntimeFailureSourceV1::ArtifactMissing,
            RuntimeFailureSourceV1::ArtifactHashMismatch,
            RuntimeFailureSourceV1::BindingMismatch,
            RuntimeFailureSourceV1::UnsupportedSchema,
            RuntimeFailureSourceV1::PanelUnresolvedChannel,
            RuntimeFailureSourceV1::PanelAmbiguous,
            RuntimeFailureSourceV1::PanelCleanupPending,
            RuntimeFailureSourceV1::PersistedStateInvalid,
            RuntimeFailureSourceV1::ProductAuthorityInactive,
        ] {
            assert!(matches!(
                source.decide(),
                RuntimeFailureDecisionV1::Blocked(_)
            ));
        }
    }

    #[test]
    fn target_change_and_shutdown_do_not_create_failures() {
        assert_eq!(
            RuntimeFailureSourceV1::ActiveTargetChanged.decide(),
            RuntimeFailureDecisionV1::Superseded
        );
        assert_eq!(
            RuntimeFailureSourceV1::ShutdownRequested.decide(),
            RuntimeFailureDecisionV1::Stop
        );
    }
}
