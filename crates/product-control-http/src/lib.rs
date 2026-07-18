mod config;
mod dto;
mod error;
mod facade;
mod router;
mod secret;

pub use config::{HttpBoundaryConfig, HttpBoundaryConfigError};
pub use dto::{
    ApplyView, ApprovalPreviewView, CurrentPrincipal, CurrentPrincipalView, DecisionView,
    DeploymentState, DeploymentView, ProductState, PromotionView, SafeApprovalSummary,
};
pub use error::{FacadeError, FacadeErrorCode};
pub use facade::{
    ApplyCommand, DecisionCommand, DiscordAuthorizationRequest, OAuthCallbackCommand,
    OAuthCallbackResult, OAuthStartCommand, OAuthStartResult, ProductControlFacade,
    ProductRequestId, ProductRequestIdParseError, PromoteCommand, RejectCommand,
};
pub use router::product_control_router;
pub use secret::{
    CsrfSecret, IdempotencyKey, IdempotencyKeyParseError, OAuthCode, OAuthState, SecretParseError,
    SessionCredential,
};
