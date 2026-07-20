use std::num::NonZeroU64;
use std::time::SystemTime;

use super::{ExactDeploymentSelectorV1, ProductDecisionProjectionV1, ProductStatusV1};
use crate::{AuthenticatedActorV1, AuthorizedInstallationScopeV1, PromotionSelectorV1};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDeploymentQueryV1 {
    pub promotion: PromotionSelectorV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExactLiveProjectionV1 {
    exact_deployment: ExactDeploymentSelectorV1,
    attestation_revision: NonZeroU64,
}

impl ExactLiveProjectionV1 {
    pub fn from_exact_attestation(
        exact_deployment: ExactDeploymentSelectorV1,
        attestation_revision: NonZeroU64,
    ) -> Self {
        Self {
            exact_deployment,
            attestation_revision,
        }
    }

    pub fn exact_deployment(&self) -> &ExactDeploymentSelectorV1 {
        &self.exact_deployment
    }

    pub fn attestation_revision(&self) -> NonZeroU64 {
        self.attestation_revision
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeploymentFailureCodeV1 {
    RuntimeEnvironmentUnavailable,
    ActivationNotObservable,
    PanelReconciliationFailed,
    GatewayStartFailed,
    GatewayReadyTimeout,
    RuntimeInvariantViolation,
    DeploymentBlocked,
    ActiveTargetChanged,
    BindingAuthorityChanged,
    ProductAuthorityInactive,
    ProductAuthorityNotCurrent,
    DeploymentSuperseded,
    DeploymentCancelled,
}

impl DeploymentFailureCodeV1 {
    pub fn parse(value: &str) -> Result<Self, DeploymentFailureCodeErrorV1> {
        match value {
            "runtime_environment_unavailable" => Ok(Self::RuntimeEnvironmentUnavailable),
            "activation_not_observable" => Ok(Self::ActivationNotObservable),
            "panel_reconciliation_failed" => Ok(Self::PanelReconciliationFailed),
            "gateway_start_failed" => Ok(Self::GatewayStartFailed),
            "gateway_ready_timeout" => Ok(Self::GatewayReadyTimeout),
            "runtime_invariant_violation" => Ok(Self::RuntimeInvariantViolation),
            "deployment_blocked" => Ok(Self::DeploymentBlocked),
            "active_target_changed" => Ok(Self::ActiveTargetChanged),
            "binding_authority_changed" => Ok(Self::BindingAuthorityChanged),
            "product_authority_inactive" => Ok(Self::ProductAuthorityInactive),
            "product_authority_not_current" => Ok(Self::ProductAuthorityNotCurrent),
            "deployment_superseded" => Ok(Self::DeploymentSuperseded),
            "deployment_cancelled" => Ok(Self::DeploymentCancelled),
            _ => Err(DeploymentFailureCodeErrorV1::Unknown),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeEnvironmentUnavailable => "runtime_environment_unavailable",
            Self::ActivationNotObservable => "activation_not_observable",
            Self::PanelReconciliationFailed => "panel_reconciliation_failed",
            Self::GatewayStartFailed => "gateway_start_failed",
            Self::GatewayReadyTimeout => "gateway_ready_timeout",
            Self::RuntimeInvariantViolation => "runtime_invariant_violation",
            Self::DeploymentBlocked => "deployment_blocked",
            Self::ActiveTargetChanged => "active_target_changed",
            Self::BindingAuthorityChanged => "binding_authority_changed",
            Self::ProductAuthorityInactive => "product_authority_inactive",
            Self::ProductAuthorityNotCurrent => "product_authority_not_current",
            Self::DeploymentSuperseded => "deployment_superseded",
            Self::DeploymentCancelled => "deployment_cancelled",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DeploymentFailureCodeErrorV1 {
    #[error("runtime deployment failure code is not public")]
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeploymentStatusProjectionV1 {
    NotRequested,
    Pending,
    Failed {
        retryable: bool,
        failure_code: String,
    },
    ExactLive(ExactLiveProjectionV1),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeploymentFailureMetadataV1 {
    retryable: bool,
    failure_code: DeploymentFailureCodeV1,
}

impl DeploymentFailureMetadataV1 {
    pub fn retryable(&self) -> bool {
        self.retryable
    }

    pub fn failure_code(&self) -> DeploymentFailureCodeV1 {
        self.failure_code
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DeploymentStatusObservationErrorV1 {
    #[error("runtime deployment observation metadata is inconsistent")]
    Inconsistent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeploymentStatusObservationV1 {
    projection: DeploymentStatusProjectionV1,
    observed_at: SystemTime,
    last_heartbeat_at: Option<SystemTime>,
    lease_expires_at: Option<SystemTime>,
}

impl DeploymentStatusObservationV1 {
    pub fn from_server_projection(
        projection: DeploymentStatusProjectionV1,
        observed_at: SystemTime,
        last_heartbeat_at: Option<SystemTime>,
        lease_expires_at: Option<SystemTime>,
    ) -> Result<Self, DeploymentStatusObservationErrorV1> {
        match (&projection, last_heartbeat_at, lease_expires_at) {
            (
                DeploymentStatusProjectionV1::ExactLive(_),
                Some(last_heartbeat_at),
                Some(lease_expires_at),
            ) if last_heartbeat_at <= observed_at && observed_at < lease_expires_at => {}
            (DeploymentStatusProjectionV1::ExactLive(_), _, _)
            | (_, Some(_), _)
            | (_, _, Some(_)) => return Err(DeploymentStatusObservationErrorV1::Inconsistent),
            _ => {}
        }
        if let DeploymentStatusProjectionV1::Failed { failure_code, .. } = &projection {
            DeploymentFailureCodeV1::parse(failure_code)
                .map_err(|_| DeploymentStatusObservationErrorV1::Inconsistent)?;
        }
        Ok(Self {
            projection,
            observed_at,
            last_heartbeat_at,
            lease_expires_at,
        })
    }

    pub fn projection(&self) -> &DeploymentStatusProjectionV1 {
        &self.projection
    }

    pub fn observed_at(&self) -> SystemTime {
        self.observed_at
    }

    pub fn last_heartbeat_at(&self) -> Option<SystemTime> {
        self.last_heartbeat_at
    }

    pub fn lease_expires_at(&self) -> Option<SystemTime> {
        self.lease_expires_at
    }

    pub fn failure(&self) -> Option<DeploymentFailureMetadataV1> {
        match &self.projection {
            DeploymentStatusProjectionV1::Failed {
                retryable,
                failure_code,
            } => DeploymentFailureCodeV1::parse(failure_code)
                .ok()
                .map(|failure_code| DeploymentFailureMetadataV1 {
                    retryable: *retryable,
                    failure_code,
                }),
            _ => None,
        }
    }

    pub fn into_projection(self) -> DeploymentStatusProjectionV1 {
        self.projection
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeploymentStatusV1 {
    NotApplicable,
    NotRequested,
    Pending,
    Failed {
        retryable: bool,
        failure_code: String,
    },
    Live {
        attestation_revision: NonZeroU64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductStatusObservationV1 {
    status: ProductStatusV1,
    decision: ProductDecisionProjectionV1,
    decision_observed_at: SystemTime,
    deployment: Option<DeploymentStatusObservationV1>,
}

impl ProductStatusObservationV1 {
    pub(crate) fn from_verified_application(
        status: ProductStatusV1,
        decision: ProductDecisionProjectionV1,
        decision_observed_at: SystemTime,
        deployment: Option<DeploymentStatusObservationV1>,
    ) -> Self {
        Self {
            status,
            decision,
            decision_observed_at,
            deployment,
        }
    }

    pub fn status(&self) -> ProductStatusV1 {
        self.status
    }

    pub fn decision(&self) -> &ProductDecisionProjectionV1 {
        &self.decision
    }

    pub fn decision_observed_at(&self) -> SystemTime {
        self.decision_observed_at
    }

    pub fn deployment(&self) -> Option<&DeploymentStatusObservationV1> {
        self.deployment.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductDeploymentStatusObservationV1 {
    status: DeploymentStatusV1,
    decision: ProductDecisionProjectionV1,
    decision_observed_at: SystemTime,
    deployment: Option<DeploymentStatusObservationV1>,
}

impl ProductDeploymentStatusObservationV1 {
    pub(crate) fn from_verified_application(
        status: DeploymentStatusV1,
        decision: ProductDecisionProjectionV1,
        decision_observed_at: SystemTime,
        deployment: Option<DeploymentStatusObservationV1>,
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

    pub fn deployment(&self) -> Option<&DeploymentStatusObservationV1> {
        self.deployment.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DeploymentStatusPortError {
    #[error("runtime deployment was not found")]
    NotFound,
    #[error("runtime deployment outcome is indeterminate: {0}")]
    Indeterminate(String),
    #[error("runtime deployment backend failed: {0}")]
    Backend(String),
}

pub struct AuthorizedDeploymentStatusV1<'a, E> {
    actor: &'a AuthenticatedActorV1,
    scope: &'a AuthorizedInstallationScopeV1,
    evidence: &'a E,
    exact_deployment: &'a ExactDeploymentSelectorV1,
}

impl<'a, E> AuthorizedDeploymentStatusV1<'a, E> {
    pub(crate) fn new(
        actor: &'a AuthenticatedActorV1,
        scope: &'a AuthorizedInstallationScopeV1,
        evidence: &'a E,
        exact_deployment: &'a ExactDeploymentSelectorV1,
    ) -> Self {
        Self {
            actor,
            scope,
            evidence,
            exact_deployment,
        }
    }

    pub fn actor(&self) -> &AuthenticatedActorV1 {
        self.actor
    }

    pub fn scope(&self) -> &AuthorizedInstallationScopeV1 {
        self.scope
    }

    pub fn evidence(&self) -> &E {
        self.evidence
    }

    pub fn exact_deployment(&self) -> &ExactDeploymentSelectorV1 {
        self.exact_deployment
    }
}

#[allow(async_fn_in_trait)]
pub trait DeploymentStatusPort<E> {
    async fn load_exact_deployment_status(
        &self,
        request: AuthorizedDeploymentStatusV1<'_, E>,
    ) -> Result<DeploymentStatusProjectionV1, DeploymentStatusPortError>;
}

#[allow(async_fn_in_trait)]
pub trait DeploymentStatusObservationPort<E> {
    async fn load_exact_deployment_observation(
        &self,
        request: AuthorizedDeploymentStatusV1<'_, E>,
    ) -> Result<DeploymentStatusObservationV1, DeploymentStatusPortError>;
}

pub(crate) fn validate_exact_live(
    expected: &ExactDeploymentSelectorV1,
    status: &DeploymentStatusProjectionV1,
) -> bool {
    matches!(
        status,
        DeploymentStatusProjectionV1::ExactLive(live)
            if live.exact_deployment() == expected
    )
}
