mod operational;
mod runtime;

pub use operational::{
    DeploymentAttestationObservationV2, DeploymentConvergencePhaseV2,
    DeploymentOperationalObservationErrorV2, DeploymentOperationalObservationV2,
    DeploymentOperationalProjectionV2, DeploymentOperationalStatusPortV2,
    DeploymentOperatorActionV2, DeploymentProcessInstanceIdErrorV2, DeploymentProcessInstanceIdV2,
    DeploymentRetryObservationV2, DeploymentServingFreshnessV2,
    ProductDeploymentOperationalStatusV2,
};
pub(crate) use runtime::validate_exact_live;
pub use runtime::{
    AuthorizedDeploymentStatusV1, DeploymentFailureCodeErrorV1, DeploymentFailureCodeV1,
    DeploymentFailureMetadataV1, DeploymentStatusObservationErrorV1,
    DeploymentStatusObservationPort, DeploymentStatusObservationV1, DeploymentStatusPort,
    DeploymentStatusPortError, DeploymentStatusProjectionV1, DeploymentStatusV1,
    ExactLiveProjectionV1, ProductDeploymentStatusObservationV1, ProductStatusObservationV1,
    RuntimeDeploymentQueryV1,
};

use authoring_promotion::{AutomationInstallationId, PromotionId, TenantId};
use discord_model::GuildId;

use crate::{
    AuthenticationError, AuthorizedInstallationScopeV1, FreshGuildAuthorityError,
    ProductControlPortError, ProductRevisionV1, PromotionSelectorV1,
};

const DEPLOYMENT_REFERENCE_MAX_BYTES: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ExactDeploymentSelectorError {
    #[error("runtime deployment reference is invalid")]
    InvalidReference,
    #[error("runtime target digest must be lowercase SHA-256 hexadecimal")]
    InvalidTargetDigest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExactDeploymentSelectorV1 {
    installation_id: AutomationInstallationId,
    promotion_id: PromotionId,
    deployment_reference: String,
    target_digest: String,
}

impl ExactDeploymentSelectorV1 {
    pub fn from_server_projection(
        installation_id: AutomationInstallationId,
        promotion_id: PromotionId,
        deployment_reference: impl Into<String>,
        target_digest: impl Into<String>,
    ) -> Result<Self, ExactDeploymentSelectorError> {
        let deployment_reference = deployment_reference.into();
        let target_digest = target_digest.into();
        if deployment_reference.is_empty()
            || deployment_reference.len() > DEPLOYMENT_REFERENCE_MAX_BYTES
            || !deployment_reference.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')
            })
        {
            return Err(ExactDeploymentSelectorError::InvalidReference);
        }
        if target_digest.len() != 64
            || !target_digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ExactDeploymentSelectorError::InvalidTargetDigest);
        }
        Ok(Self {
            installation_id,
            promotion_id,
            deployment_reference,
            target_digest,
        })
    }

    pub fn installation_id(&self) -> &AutomationInstallationId {
        &self.installation_id
    }

    pub fn promotion_id(&self) -> &PromotionId {
        &self.promotion_id
    }

    pub fn deployment_reference(&self) -> &str {
        &self.deployment_reference
    }

    pub fn target_digest(&self) -> &str {
        &self.target_digest
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProductDecisionPhaseV1 {
    PendingApproval,
    Approved,
    Applying,
    Applied {
        exact_deployment: ExactDeploymentSelectorV1,
    },
    Rejected,
    Expired,
    Superseded,
    Withdrawn,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductDecisionProjectionV1 {
    tenant_id: TenantId,
    installation_id: AutomationInstallationId,
    guild_id: GuildId,
    promotion_id: PromotionId,
    revision: ProductRevisionV1,
    phase: ProductDecisionPhaseV1,
}

impl ProductDecisionProjectionV1 {
    pub fn from_server_projection(
        tenant_id: TenantId,
        installation_id: AutomationInstallationId,
        guild_id: GuildId,
        promotion_id: PromotionId,
        revision: ProductRevisionV1,
        phase: ProductDecisionPhaseV1,
    ) -> Self {
        Self {
            tenant_id,
            installation_id,
            guild_id,
            promotion_id,
            revision,
            phase,
        }
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn installation_id(&self) -> &AutomationInstallationId {
        &self.installation_id
    }

    pub fn guild_id(&self) -> GuildId {
        self.guild_id
    }

    pub fn promotion_id(&self) -> &PromotionId {
        &self.promotion_id
    }

    pub fn revision(&self) -> ProductRevisionV1 {
        self.revision
    }

    pub fn phase(&self) -> &ProductDecisionPhaseV1 {
        &self.phase
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProductStatusV1 {
    PendingApproval,
    Approved,
    Applying,
    RuntimePending,
    Live,
    Rejected,
    Expired,
    Superseded,
    Withdrawn,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductApplyResultV1 {
    status: ProductStatusV1,
    exact_replay: bool,
    exact_deployment: ExactDeploymentSelectorV1,
}

impl ProductApplyResultV1 {
    pub(crate) fn from_verified_application(
        status: ProductStatusV1,
        exact_replay: bool,
        exact_deployment: ExactDeploymentSelectorV1,
    ) -> Self {
        Self {
            status,
            exact_replay,
            exact_deployment,
        }
    }

    pub fn status(&self) -> ProductStatusV1 {
        self.status
    }

    pub fn exact_replay(&self) -> bool {
        self.exact_replay
    }

    pub fn exact_deployment(&self) -> &ExactDeploymentSelectorV1 {
        &self.exact_deployment
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductApplicationError {
    #[error(transparent)]
    Authentication(#[from] AuthenticationError),
    #[error(transparent)]
    FreshAuthority(#[from] FreshGuildAuthorityError),
    #[error(transparent)]
    Control(#[from] ProductControlPortError),
    #[error(transparent)]
    Deployment(#[from] DeploymentStatusPortError),
    #[error("trusted product adapter returned an inconsistent projection")]
    InvalidProjection,
}

pub(crate) fn validate_decision_projection(
    scope: &AuthorizedInstallationScopeV1,
    promotion: &PromotionSelectorV1,
    projection: &ProductDecisionProjectionV1,
) -> Result<(), ProductApplicationError> {
    if projection.tenant_id() != scope.tenant_id()
        || projection.installation_id() != scope.installation_id()
        || projection.guild_id() != scope.guild_id()
        || projection.promotion_id() != promotion.promotion_id()
    {
        return Err(ProductApplicationError::InvalidProjection);
    }
    if let ProductDecisionPhaseV1::Applied { exact_deployment } = projection.phase() {
        if exact_deployment.installation_id() != scope.installation_id()
            || exact_deployment.promotion_id() != promotion.promotion_id()
        {
            return Err(ProductApplicationError::InvalidProjection);
        }
    }
    Ok(())
}

pub(crate) fn map_non_applied_status(phase: &ProductDecisionPhaseV1) -> Option<ProductStatusV1> {
    match phase {
        ProductDecisionPhaseV1::PendingApproval => Some(ProductStatusV1::PendingApproval),
        ProductDecisionPhaseV1::Approved => Some(ProductStatusV1::Approved),
        ProductDecisionPhaseV1::Applying => Some(ProductStatusV1::Applying),
        ProductDecisionPhaseV1::Applied { .. } => None,
        ProductDecisionPhaseV1::Rejected => Some(ProductStatusV1::Rejected),
        ProductDecisionPhaseV1::Expired => Some(ProductStatusV1::Expired),
        ProductDecisionPhaseV1::Superseded => Some(ProductStatusV1::Superseded),
        ProductDecisionPhaseV1::Withdrawn => Some(ProductStatusV1::Withdrawn),
    }
}
