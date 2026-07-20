use std::num::{NonZeroU32, NonZeroU64};
use std::time::SystemTime;

use super::{
    AuthorizedDeploymentStatusV1, DeploymentFailureCodeV1, DeploymentStatusObservationV1,
    DeploymentStatusPortError, DeploymentStatusProjectionV1, DeploymentStatusV1,
    ProductDecisionProjectionV1,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeploymentConvergencePhaseV2 {
    Requested,
    PreflightReady,
    DrainRequested,
    Drained,
    ActivationApplying,
    RuntimeReady,
    RetryWaiting,
    RetryDue,
    OperatorBlocked,
    AuthorityBlocked,
    ReconcilingPanels,
    AwaitingGatewayReady,
    Live,
    Superseded,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeploymentRetryObservationV2 {
    Waiting {
        failure_attempt: NonZeroU32,
        retry_not_before: SystemTime,
    },
    Due {
        failure_attempt: NonZeroU32,
        retry_not_before: SystemTime,
    },
}

impl DeploymentRetryObservationV2 {
    pub fn failure_attempt(self) -> NonZeroU32 {
        match self {
            Self::Waiting {
                failure_attempt, ..
            }
            | Self::Due {
                failure_attempt, ..
            } => failure_attempt,
        }
    }

    pub fn retry_not_before(self) -> SystemTime {
        match self {
            Self::Waiting {
                retry_not_before, ..
            }
            | Self::Due {
                retry_not_before, ..
            } => retry_not_before,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeploymentOperatorActionV2 {
    RecoverBlockedDeployment,
    RestoreProductAuthority,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeploymentAttestationObservationV2 {
    deployment_revision: NonZeroU64,
    convergence_attempt: NonZeroU32,
}

impl DeploymentAttestationObservationV2 {
    pub fn new(deployment_revision: NonZeroU64, convergence_attempt: NonZeroU32) -> Self {
        Self {
            deployment_revision,
            convergence_attempt,
        }
    }

    pub fn deployment_revision(self) -> NonZeroU64 {
        self.deployment_revision
    }

    pub fn convergence_attempt(self) -> NonZeroU32 {
        self.convergence_attempt
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeploymentServingFreshnessV2 {
    NotExpected,
    AttestationMissing,
    LeaseMissing,
    IdentityMismatch,
    Disconnected {
        last_heartbeat_at: SystemTime,
        lease_expires_at: SystemTime,
    },
    Expired {
        last_heartbeat_at: SystemTime,
        lease_expires_at: SystemTime,
    },
    Fresh {
        last_heartbeat_at: SystemTime,
        lease_expires_at: SystemTime,
    },
}

impl DeploymentServingFreshnessV2 {
    pub fn last_heartbeat_at(self) -> Option<SystemTime> {
        match self {
            Self::Disconnected {
                last_heartbeat_at, ..
            }
            | Self::Expired {
                last_heartbeat_at, ..
            }
            | Self::Fresh {
                last_heartbeat_at, ..
            } => Some(last_heartbeat_at),
            Self::NotExpected
            | Self::AttestationMissing
            | Self::LeaseMissing
            | Self::IdentityMismatch => None,
        }
    }

    pub fn lease_expires_at(self) -> Option<SystemTime> {
        match self {
            Self::Disconnected {
                lease_expires_at, ..
            }
            | Self::Expired {
                lease_expires_at, ..
            }
            | Self::Fresh {
                lease_expires_at, ..
            } => Some(lease_expires_at),
            Self::NotExpected
            | Self::AttestationMissing
            | Self::LeaseMissing
            | Self::IdentityMismatch => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DeploymentOperationalObservationErrorV2 {
    #[error("runtime deployment operational observation is inconsistent")]
    Inconsistent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeploymentOperationalProjectionV2 {
    pub phase: DeploymentConvergencePhaseV2,
    pub current_attempt: u32,
    pub last_failure_attempt: Option<NonZeroU32>,
    pub retry: Option<DeploymentRetryObservationV2>,
    pub operator_action: Option<DeploymentOperatorActionV2>,
    pub attestation: Option<DeploymentAttestationObservationV2>,
    pub serving: DeploymentServingFreshnessV2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeploymentOperationalObservationV2 {
    base: DeploymentStatusObservationV1,
    phase: DeploymentConvergencePhaseV2,
    current_attempt: u32,
    last_failure_attempt: Option<NonZeroU32>,
    retry: Option<DeploymentRetryObservationV2>,
    operator_action: Option<DeploymentOperatorActionV2>,
    attestation: Option<DeploymentAttestationObservationV2>,
    serving: DeploymentServingFreshnessV2,
}

impl DeploymentOperationalObservationV2 {
    pub fn from_server_projection(
        base: DeploymentStatusObservationV1,
        operations: DeploymentOperationalProjectionV2,
    ) -> Result<Self, DeploymentOperationalObservationErrorV2> {
        let observation = Self {
            base,
            phase: operations.phase,
            current_attempt: operations.current_attempt,
            last_failure_attempt: operations.last_failure_attempt,
            retry: operations.retry,
            operator_action: operations.operator_action,
            attestation: operations.attestation,
            serving: operations.serving,
        };
        observation.validate()?;
        Ok(observation)
    }

    pub fn base(&self) -> &DeploymentStatusObservationV1 {
        &self.base
    }

    pub fn phase(&self) -> DeploymentConvergencePhaseV2 {
        self.phase
    }

    pub fn current_attempt(&self) -> u32 {
        self.current_attempt
    }

    pub fn last_failure_attempt(&self) -> Option<NonZeroU32> {
        self.last_failure_attempt
    }

    pub fn retry(&self) -> Option<DeploymentRetryObservationV2> {
        self.retry
    }

    pub fn operator_action(&self) -> Option<DeploymentOperatorActionV2> {
        self.operator_action
    }

    pub fn attestation(&self) -> Option<DeploymentAttestationObservationV2> {
        self.attestation
    }

    pub fn serving(&self) -> DeploymentServingFreshnessV2 {
        self.serving
    }

    pub fn observed_at(&self) -> SystemTime {
        self.base.observed_at()
    }

    fn validate(&self) -> Result<(), DeploymentOperationalObservationErrorV2> {
        let current = self.current_attempt;
        if current == 0
            && !matches!(
                self.phase,
                DeploymentConvergencePhaseV2::Requested
                    | DeploymentConvergencePhaseV2::AuthorityBlocked
                    | DeploymentConvergencePhaseV2::Superseded
            )
            || self
                .last_failure_attempt
                .is_some_and(|attempt| attempt.get() > current)
        {
            return Err(DeploymentOperationalObservationErrorV2::Inconsistent);
        }
        self.validate_phase_projection()?;
        self.validate_retry()?;
        self.validate_operator_action()?;
        self.validate_serving()?;
        if matches!(
            self.base.projection(),
            DeploymentStatusProjectionV1::NotRequested
        ) {
            return Err(DeploymentOperationalObservationErrorV2::Inconsistent);
        }
        Ok(())
    }

    fn validate_phase_projection(&self) -> Result<(), DeploymentOperationalObservationErrorV2> {
        let valid = match self.phase {
            DeploymentConvergencePhaseV2::Requested
            | DeploymentConvergencePhaseV2::PreflightReady
            | DeploymentConvergencePhaseV2::DrainRequested
            | DeploymentConvergencePhaseV2::Drained
            | DeploymentConvergencePhaseV2::ActivationApplying
            | DeploymentConvergencePhaseV2::RuntimeReady
            | DeploymentConvergencePhaseV2::ReconcilingPanels
            | DeploymentConvergencePhaseV2::AwaitingGatewayReady => pending(self.base.projection()),
            DeploymentConvergencePhaseV2::RetryWaiting | DeploymentConvergencePhaseV2::RetryDue => {
                retryable_failure(self.base.projection())
            }
            DeploymentConvergencePhaseV2::OperatorBlocked => {
                blocked_runtime_failure(self.base.projection())
            }
            DeploymentConvergencePhaseV2::AuthorityBlocked => {
                product_authority_failure(self.base.projection())
            }
            DeploymentConvergencePhaseV2::Live => matches!(
                self.base.projection(),
                DeploymentStatusProjectionV1::Pending | DeploymentStatusProjectionV1::ExactLive(_)
            ),
            DeploymentConvergencePhaseV2::Superseded => superseded_failure(self.base.projection()),
            DeploymentConvergencePhaseV2::Cancelled => cancelled_failure(self.base.projection()),
        };
        if valid {
            Ok(())
        } else {
            Err(DeploymentOperationalObservationErrorV2::Inconsistent)
        }
    }

    fn validate_retry(&self) -> Result<(), DeploymentOperationalObservationErrorV2> {
        match (self.phase, self.retry) {
            (
                DeploymentConvergencePhaseV2::RetryWaiting,
                Some(DeploymentRetryObservationV2::Waiting {
                    failure_attempt,
                    retry_not_before,
                }),
            ) if failure_attempt.get() == self.current_attempt
                && self.last_failure_attempt == Some(failure_attempt)
                && self.base.observed_at() < retry_not_before
                && retryable_failure(self.base.projection()) =>
            {
                Ok(())
            }
            (
                DeploymentConvergencePhaseV2::RetryDue,
                Some(DeploymentRetryObservationV2::Due {
                    failure_attempt,
                    retry_not_before,
                }),
            ) if failure_attempt.get() == self.current_attempt
                && self.last_failure_attempt == Some(failure_attempt)
                && self.base.observed_at() >= retry_not_before
                && retryable_failure(self.base.projection()) =>
            {
                Ok(())
            }
            (DeploymentConvergencePhaseV2::RetryWaiting, _)
            | (DeploymentConvergencePhaseV2::RetryDue, _)
            | (_, Some(_)) => Err(DeploymentOperationalObservationErrorV2::Inconsistent),
            (_, None) => Ok(()),
        }
    }

    fn validate_operator_action(&self) -> Result<(), DeploymentOperationalObservationErrorV2> {
        match (self.phase, self.operator_action) {
            (
                DeploymentConvergencePhaseV2::OperatorBlocked,
                Some(DeploymentOperatorActionV2::RecoverBlockedDeployment),
            ) if self
                .last_failure_attempt
                .is_some_and(|attempt| attempt.get() == self.current_attempt)
                && blocked_runtime_failure(self.base.projection()) =>
            {
                Ok(())
            }
            (DeploymentConvergencePhaseV2::OperatorBlocked, _)
            | (_, Some(DeploymentOperatorActionV2::RecoverBlockedDeployment)) => {
                Err(DeploymentOperationalObservationErrorV2::Inconsistent)
            }
            (
                DeploymentConvergencePhaseV2::AuthorityBlocked,
                Some(DeploymentOperatorActionV2::RestoreProductAuthority),
            ) if product_authority_failure(self.base.projection()) => Ok(()),
            (DeploymentConvergencePhaseV2::AuthorityBlocked, _)
            | (_, Some(DeploymentOperatorActionV2::RestoreProductAuthority)) => {
                Err(DeploymentOperationalObservationErrorV2::Inconsistent)
            }
            (_, None) if product_authority_failure(self.base.projection()) => {
                Err(DeploymentOperationalObservationErrorV2::Inconsistent)
            }
            (_, None) => Ok(()),
        }
    }

    fn validate_serving(&self) -> Result<(), DeploymentOperationalObservationErrorV2> {
        let observed_at = self.base.observed_at();
        let phase_live = self.phase == DeploymentConvergencePhaseV2::Live;
        let exact_live = match self.base.projection() {
            DeploymentStatusProjectionV1::ExactLive(live) => Some(live.attestation_revision()),
            _ => None,
        };
        let attestation_exact = self.attestation.is_some_and(|attestation| {
            attestation.convergence_attempt().get() == self.current_attempt
                && exact_live.is_none_or(|revision| revision == attestation.deployment_revision())
        });
        let valid = match self.serving {
            DeploymentServingFreshnessV2::NotExpected => {
                self.attestation.is_none() && exact_live.is_none()
            }
            DeploymentServingFreshnessV2::AttestationMissing => {
                phase_live
                    && self.attestation.is_none()
                    && exact_live.is_none()
                    && pending(self.base.projection())
            }
            DeploymentServingFreshnessV2::LeaseMissing
            | DeploymentServingFreshnessV2::IdentityMismatch => {
                phase_live
                    && attestation_exact
                    && exact_live.is_none()
                    && pending(self.base.projection())
            }
            DeploymentServingFreshnessV2::Disconnected {
                last_heartbeat_at,
                lease_expires_at,
            } => {
                phase_live
                    && attestation_exact
                    && exact_live.is_none()
                    && pending(self.base.projection())
                    && last_heartbeat_at <= observed_at
                    && last_heartbeat_at <= lease_expires_at
            }
            DeploymentServingFreshnessV2::Expired {
                last_heartbeat_at,
                lease_expires_at,
            } => {
                phase_live
                    && attestation_exact
                    && exact_live.is_none()
                    && pending(self.base.projection())
                    && last_heartbeat_at <= lease_expires_at
                    && lease_expires_at <= observed_at
            }
            DeploymentServingFreshnessV2::Fresh {
                last_heartbeat_at,
                lease_expires_at,
            } => {
                phase_live
                    && attestation_exact
                    && exact_live.is_some()
                    && last_heartbeat_at <= observed_at
                    && observed_at < lease_expires_at
                    && self.base.last_heartbeat_at() == Some(last_heartbeat_at)
                    && self.base.lease_expires_at() == Some(lease_expires_at)
            }
        };
        if phase_live == matches!(self.serving, DeploymentServingFreshnessV2::NotExpected) || !valid
        {
            return Err(DeploymentOperationalObservationErrorV2::Inconsistent);
        }
        Ok(())
    }
}

#[allow(async_fn_in_trait)]
pub trait DeploymentOperationalStatusPortV2<E> {
    async fn load_exact_deployment_operational_status_v2(
        &self,
        request: AuthorizedDeploymentStatusV1<'_, E>,
    ) -> Result<DeploymentOperationalObservationV2, DeploymentStatusPortError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductDeploymentOperationalStatusV2 {
    status: DeploymentStatusV1,
    decision: ProductDecisionProjectionV1,
    decision_observed_at: SystemTime,
    deployment: Option<DeploymentOperationalObservationV2>,
}

impl ProductDeploymentOperationalStatusV2 {
    pub(crate) fn from_verified_application(
        status: DeploymentStatusV1,
        decision: ProductDecisionProjectionV1,
        decision_observed_at: SystemTime,
        deployment: Option<DeploymentOperationalObservationV2>,
    ) -> Self {
        Self {
            status,
            decision,
            decision_observed_at,
            deployment,
        }
    }

    pub fn status(&self) -> &DeploymentStatusV1 {
        &self.status
    }

    pub fn decision(&self) -> &ProductDecisionProjectionV1 {
        &self.decision
    }

    pub fn decision_observed_at(&self) -> SystemTime {
        self.decision_observed_at
    }

    pub fn deployment(&self) -> Option<&DeploymentOperationalObservationV2> {
        self.deployment.as_ref()
    }
}

fn retryable_failure(projection: &DeploymentStatusProjectionV1) -> bool {
    matches!(
        projection,
        DeploymentStatusProjectionV1::Failed {
            retryable: true,
            failure_code,
        } if DeploymentFailureCodeV1::parse(failure_code).is_ok_and(|code| matches!(
            code,
            DeploymentFailureCodeV1::RuntimeEnvironmentUnavailable
                | DeploymentFailureCodeV1::ActivationNotObservable
                | DeploymentFailureCodeV1::PanelReconciliationFailed
                | DeploymentFailureCodeV1::GatewayStartFailed
                | DeploymentFailureCodeV1::GatewayReadyTimeout
                | DeploymentFailureCodeV1::RuntimeInvariantViolation
        ))
    )
}

fn blocked_runtime_failure(projection: &DeploymentStatusProjectionV1) -> bool {
    matches!(
        projection,
        DeploymentStatusProjectionV1::Failed {
            retryable: false,
            failure_code,
        } if DeploymentFailureCodeV1::parse(failure_code).is_ok_and(|code| matches!(
            code,
            DeploymentFailureCodeV1::RuntimeEnvironmentUnavailable
                | DeploymentFailureCodeV1::ActivationNotObservable
                | DeploymentFailureCodeV1::PanelReconciliationFailed
                | DeploymentFailureCodeV1::GatewayStartFailed
                | DeploymentFailureCodeV1::GatewayReadyTimeout
                | DeploymentFailureCodeV1::RuntimeInvariantViolation
                | DeploymentFailureCodeV1::DeploymentBlocked
        ))
    )
}

fn product_authority_failure(projection: &DeploymentStatusProjectionV1) -> bool {
    matches!(
        projection,
        DeploymentStatusProjectionV1::Failed {
            retryable: false,
            failure_code,
        } if DeploymentFailureCodeV1::parse(failure_code).is_ok_and(|code| matches!(
            code,
            DeploymentFailureCodeV1::ProductAuthorityInactive
                | DeploymentFailureCodeV1::ProductAuthorityNotCurrent
        ))
    )
}

fn superseded_failure(projection: &DeploymentStatusProjectionV1) -> bool {
    matches!(
        projection,
        DeploymentStatusProjectionV1::Failed {
            retryable: false,
            failure_code,
        } if DeploymentFailureCodeV1::parse(failure_code).is_ok_and(|code| matches!(
            code,
            DeploymentFailureCodeV1::ActiveTargetChanged
                | DeploymentFailureCodeV1::BindingAuthorityChanged
                | DeploymentFailureCodeV1::DeploymentSuperseded
        ))
    )
}

fn cancelled_failure(projection: &DeploymentStatusProjectionV1) -> bool {
    matches!(
        projection,
        DeploymentStatusProjectionV1::Failed {
            retryable: false,
            failure_code,
        } if DeploymentFailureCodeV1::parse(failure_code)
            .is_ok_and(|code| code == DeploymentFailureCodeV1::DeploymentCancelled)
    )
}

fn pending(projection: &DeploymentStatusProjectionV1) -> bool {
    matches!(projection, DeploymentStatusProjectionV1::Pending)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use authoring_promotion::{AutomationInstallationId, PromotionId};

    use super::*;
    use crate::{ExactDeploymentSelectorV1, ExactLiveProjectionV1};

    fn base(
        projection: DeploymentStatusProjectionV1,
        observed_at: SystemTime,
    ) -> DeploymentStatusObservationV1 {
        let live = matches!(projection, DeploymentStatusProjectionV1::ExactLive(_));
        DeploymentStatusObservationV1::from_server_projection(
            projection,
            observed_at,
            live.then_some(observed_at - Duration::from_secs(1)),
            live.then_some(observed_at + Duration::from_secs(10)),
        )
        .unwrap()
    }

    fn operations(
        phase: DeploymentConvergencePhaseV2,
        current_attempt: u32,
    ) -> DeploymentOperationalProjectionV2 {
        DeploymentOperationalProjectionV2 {
            phase,
            current_attempt,
            last_failure_attempt: None,
            retry: None,
            operator_action: None,
            attestation: None,
            serving: DeploymentServingFreshnessV2::NotExpected,
        }
    }

    #[test]
    fn retry_waiting_and_due_are_derived_from_the_observation_clock() {
        let observed_at = UNIX_EPOCH + Duration::from_secs(1_000);
        let attempt = NonZeroU32::new(2).unwrap();
        for (phase, retry) in [
            (
                DeploymentConvergencePhaseV2::RetryWaiting,
                DeploymentRetryObservationV2::Waiting {
                    failure_attempt: attempt,
                    retry_not_before: observed_at + Duration::from_secs(1),
                },
            ),
            (
                DeploymentConvergencePhaseV2::RetryDue,
                DeploymentRetryObservationV2::Due {
                    failure_attempt: attempt,
                    retry_not_before: observed_at,
                },
            ),
        ] {
            let mut operations = operations(phase, 2);
            operations.last_failure_attempt = Some(attempt);
            operations.retry = Some(retry);
            let observation = DeploymentOperationalObservationV2::from_server_projection(
                base(
                    DeploymentStatusProjectionV1::Failed {
                        retryable: true,
                        failure_code: "gateway_start_failed".to_string(),
                    },
                    observed_at,
                ),
                operations,
            )
            .unwrap();
            assert_eq!(observation.retry(), Some(retry));
        }
    }

    #[test]
    fn operator_actions_are_closed_to_the_matching_failure_class() {
        let observed_at = UNIX_EPOCH + Duration::from_secs(1_000);
        let attempt = NonZeroU32::new(3).unwrap();
        let mut blocked = operations(DeploymentConvergencePhaseV2::OperatorBlocked, 3);
        blocked.last_failure_attempt = Some(attempt);
        blocked.operator_action = Some(DeploymentOperatorActionV2::RecoverBlockedDeployment);
        assert!(DeploymentOperationalObservationV2::from_server_projection(
            base(
                DeploymentStatusProjectionV1::Failed {
                    retryable: false,
                    failure_code: "gateway_ready_timeout".to_string(),
                },
                observed_at,
            ),
            blocked,
        )
        .is_ok());
        let mut authority = operations(DeploymentConvergencePhaseV2::AuthorityBlocked, 3);
        authority.operator_action = Some(DeploymentOperatorActionV2::RestoreProductAuthority);
        assert!(DeploymentOperationalObservationV2::from_server_projection(
            base(
                DeploymentStatusProjectionV1::Failed {
                    retryable: false,
                    failure_code: "product_authority_inactive".to_string(),
                },
                observed_at,
            ),
            authority,
        )
        .is_ok());
    }

    #[test]
    fn strict_live_binds_attestation_attempt_revision_and_fresh_lease() {
        let observed_at = UNIX_EPOCH + Duration::from_secs(1_000);
        let exact = ExactDeploymentSelectorV1::from_server_projection(
            AutomationInstallationId::parse("installation-1").unwrap(),
            PromotionId::parse(&"a".repeat(64)).unwrap(),
            "deployment-1",
            "b".repeat(64),
        )
        .unwrap();
        let revision = NonZeroU64::new(7).unwrap();
        let attempt = NonZeroU32::new(2).unwrap();
        let mut operations = operations(DeploymentConvergencePhaseV2::Live, 2);
        operations.attestation = Some(DeploymentAttestationObservationV2::new(revision, attempt));
        operations.serving = DeploymentServingFreshnessV2::Fresh {
            last_heartbeat_at: observed_at - Duration::from_secs(1),
            lease_expires_at: observed_at + Duration::from_secs(10),
        };
        let observation = DeploymentOperationalObservationV2::from_server_projection(
            base(
                DeploymentStatusProjectionV1::ExactLive(
                    ExactLiveProjectionV1::from_exact_attestation(exact, revision),
                ),
                observed_at,
            ),
            operations,
        )
        .unwrap();
        assert_eq!(
            observation.attestation().unwrap().convergence_attempt(),
            attempt
        );
        assert!(matches!(
            observation.serving(),
            DeploymentServingFreshnessV2::Fresh { .. }
        ));
    }

    #[test]
    fn impossible_attempt_and_live_combinations_fail_closed() {
        let observed_at = UNIX_EPOCH + Duration::from_secs(1_000);
        assert!(DeploymentOperationalObservationV2::from_server_projection(
            base(DeploymentStatusProjectionV1::Pending, observed_at),
            operations(DeploymentConvergencePhaseV2::RuntimeReady, 0),
        )
        .is_err());
        let mut missing = operations(DeploymentConvergencePhaseV2::Live, 1);
        missing.serving = DeploymentServingFreshnessV2::Fresh {
            last_heartbeat_at: observed_at,
            lease_expires_at: observed_at + Duration::from_secs(1),
        };
        assert!(DeploymentOperationalObservationV2::from_server_projection(
            base(DeploymentStatusProjectionV1::Pending, observed_at),
            missing,
        )
        .is_err());
        assert!(DeploymentOperationalObservationV2::from_server_projection(
            base(DeploymentStatusProjectionV1::Pending, observed_at),
            operations(DeploymentConvergencePhaseV2::Live, 1),
        )
        .is_err());
        assert!(DeploymentOperationalObservationV2::from_server_projection(
            base(
                DeploymentStatusProjectionV1::Failed {
                    retryable: false,
                    failure_code: "deployment_cancelled".to_string(),
                },
                observed_at,
            ),
            operations(DeploymentConvergencePhaseV2::Requested, 1),
        )
        .is_err());
        assert!(DeploymentOperationalObservationV2::from_server_projection(
            base(DeploymentStatusProjectionV1::Pending, observed_at),
            operations(DeploymentConvergencePhaseV2::Cancelled, 1),
        )
        .is_err());
        let attempt = NonZeroU32::MIN;
        let mut invalid_retry = operations(DeploymentConvergencePhaseV2::RetryDue, 1);
        invalid_retry.last_failure_attempt = Some(attempt);
        invalid_retry.retry = Some(DeploymentRetryObservationV2::Due {
            failure_attempt: attempt,
            retry_not_before: observed_at,
        });
        assert!(DeploymentOperationalObservationV2::from_server_projection(
            base(
                DeploymentStatusProjectionV1::Failed {
                    retryable: true,
                    failure_code: "product_authority_inactive".to_string(),
                },
                observed_at,
            ),
            invalid_retry,
        )
        .is_err());
    }
}
