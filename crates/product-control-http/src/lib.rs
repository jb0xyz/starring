mod config;
mod dto;
mod error;
mod facade;
mod readiness;
mod router;
mod secret;

pub use config::{HttpBoundaryConfig, HttpBoundaryConfigError};
pub use dto::{
    ApplyView, ApprovalPreviewView, CurrentPrincipal, CurrentPrincipalView, DecisionView,
    DeploymentAttestationViewV2, DeploymentFailureViewV2, DeploymentOperationalStateV2,
    DeploymentOperationalViewV2, DeploymentOperatorActionV2, DeploymentRetryStateV2,
    DeploymentRetryViewV2, DeploymentRuntimePhaseV2, DeploymentServingFreshnessStateV2,
    DeploymentServingFreshnessViewV2, DeploymentState, DeploymentView, ProductState, PromotionView,
    RuntimeDeploymentOperationalViewV2, SafeApprovalSummary,
};
pub use error::{FacadeError, FacadeErrorCode};
pub use facade::{
    ApplyCommand, DecisionCommand, DiscordAuthorizationRequest, OAuthCallbackCommand,
    OAuthCallbackResult, OAuthStartCommand, OAuthStartResult, ProductControlFacade,
    ProductControlOperationalFacadeV2, ProductRequestId, ProductRequestIdParseError,
    PromoteCommand, RejectCommand,
};
pub use readiness::ProductApiReadinessGate;
pub use router::{
    product_control_router, product_control_router_with_operational_v2,
    product_control_router_with_operational_v2_and_readiness_gate,
    product_control_router_with_readiness_gate,
};
pub use secret::{
    CsrfSecret, IdempotencyKey, IdempotencyKeyParseError, OAuthCode, OAuthState, SecretParseError,
    SessionCredential,
};
