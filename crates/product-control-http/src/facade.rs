use std::fmt::{Debug, Formatter};

use uuid::Uuid;

use crate::{
    ApplyView, ApprovalPreviewView, CsrfSecret, CurrentPrincipal, DecisionView,
    DeploymentOperationalViewV2, DeploymentView, FacadeError, LifecycleCancellationView, OAuthCode,
    OAuthState, ProductState, PromotionView, SessionCredential,
};

const RESOURCE_ID_MAX_BYTES: usize = 128;
const REQUEST_ID_MAX_BYTES: usize = 64;
const REASON_MAX_BYTES: usize = 4_000;
const REASON_MAX_SCALARS: usize = 1_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductRequestId(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("product request ID is invalid")]
pub struct ProductRequestIdParseError;

impl ProductRequestId {
    pub fn parse(value: &str) -> Result<Self, ProductRequestIdParseError> {
        if !value.is_empty()
            && value.len() <= REQUEST_ID_MAX_BYTES
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            Ok(Self(value.to_string()))
        } else {
            Err(ProductRequestIdParseError)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn generated() -> Self {
        Self(Uuid::new_v4().simple().to_string())
    }
}

pub(crate) fn valid_resource_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= RESOURCE_ID_MAX_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

pub(crate) fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OAuthStartCommand {
    pub return_to: Option<String>,
}

pub struct OAuthStartResult {
    pub authorization_request: DiscordAuthorizationRequest,
    pub authorization_state: OAuthState,
    pub browser_nonce: OAuthState,
    pub max_age_seconds: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DiscordAuthorizationRequest {
    pub client_id: String,
    pub callback_url: String,
}

impl Debug for OAuthStartResult {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OAuthStartResult")
            .field("authorization_request", &self.authorization_request)
            .field("authorization_state", &self.authorization_state)
            .field("browser_nonce", &self.browser_nonce)
            .field("max_age_seconds", &self.max_age_seconds)
            .finish()
    }
}

#[derive(Debug)]
pub struct OAuthCallbackCommand {
    pub code: OAuthCode,
    pub state: OAuthState,
    pub browser_nonce: OAuthState,
}

#[derive(Debug)]
pub struct OAuthCallbackResult {
    pub session: SessionCredential,
    pub csrf: CsrfSecret,
    pub return_to: String,
    pub max_age_seconds: u32,
}

pub struct PromoteCommand {
    pub request_id: ProductRequestId,
    pub installation_id: String,
    pub session_id: String,
    pub expected_generation: u64,
    pub idempotency_key: crate::IdempotencyKey,
}

impl Debug for PromoteCommand {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PromoteCommand")
            .field("request_id", &self.request_id)
            .field("installation_id", &self.installation_id)
            .field("session_id", &self.session_id)
            .field("expected_generation", &self.expected_generation)
            .field("idempotency_key", &"<redacted>")
            .finish()
    }
}

pub struct DecisionCommand {
    pub request_id: ProductRequestId,
    pub installation_id: String,
    pub promotion_id: String,
    pub expected_payload_digest: String,
    pub expected_revision: u64,
    pub idempotency_key: crate::IdempotencyKey,
}

impl Debug for DecisionCommand {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DecisionCommand")
            .field("request_id", &self.request_id)
            .field("installation_id", &self.installation_id)
            .field("promotion_id", &self.promotion_id)
            .field("expected_payload_digest", &self.expected_payload_digest)
            .field("expected_revision", &self.expected_revision)
            .field("idempotency_key", &"<redacted>")
            .finish()
    }
}

pub struct RejectCommand {
    pub decision: DecisionCommand,
    pub reason: String,
}

impl Debug for RejectCommand {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RejectCommand")
            .field("decision", &self.decision)
            .field("reason", &"<redacted>")
            .finish()
    }
}

#[derive(Debug)]
pub struct ApplyCommand {
    pub decision: DecisionCommand,
}

pub struct LifecycleCancellationCommand {
    pub decision: DecisionCommand,
    pub drain_intent_id: String,
    pub acknowledged_intent_revision: u64,
    pub acknowledged_state_digest: String,
    pub product_operation_id: String,
    pub expected_runtime_deployment_revision: u64,
    pub reason: String,
}

impl Debug for LifecycleCancellationCommand {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LifecycleCancellationCommand")
            .field("decision", &self.decision)
            .field("runtime_drain", &"<opaque>")
            .field("reason", &"<redacted>")
            .finish()
    }
}

impl PromoteCommand {
    pub(crate) fn validate(&self) -> bool {
        valid_resource_id(&self.installation_id)
            && valid_resource_id(&self.session_id)
            && self.expected_generation > 0
    }
}

impl DecisionCommand {
    pub(crate) fn validate(&self) -> bool {
        valid_resource_id(&self.installation_id)
            && valid_digest(&self.promotion_id)
            && valid_digest(&self.expected_payload_digest)
            && self.expected_revision > 0
    }
}

impl RejectCommand {
    pub(crate) fn normalize(mut self) -> Option<Self> {
        self.reason = self.reason.trim().to_string();
        if self.decision.validate()
            && !self.reason.is_empty()
            && self.reason.len() <= REASON_MAX_BYTES
            && self.reason.chars().count() <= REASON_MAX_SCALARS
            && !self.reason.chars().any(char::is_control)
        {
            Some(self)
        } else {
            None
        }
    }
}

impl LifecycleCancellationCommand {
    pub(crate) fn normalize(mut self) -> Option<Self> {
        self.reason = self.reason.trim().to_string();
        let valid_runtime_id = |value: &str| {
            value.len() == 32
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        };
        if self.decision.validate()
            && self.decision.expected_revision <= i64::MAX as u64
            && valid_runtime_id(&self.drain_intent_id)
            && valid_runtime_id(&self.product_operation_id)
            && self.drain_intent_id != self.product_operation_id
            && self.acknowledged_intent_revision > 0
            && self.acknowledged_intent_revision <= i64::MAX as u64
            && valid_digest(&self.acknowledged_state_digest)
            && self.expected_runtime_deployment_revision > 0
            && self.expected_runtime_deployment_revision <= i64::MAX as u64
            && !self.reason.is_empty()
            && self.reason.len() <= REASON_MAX_BYTES
            && self.reason.chars().count() <= REASON_MAX_SCALARS
            && !self.reason.chars().any(char::is_control)
        {
            Some(self)
        } else {
            None
        }
    }
}

#[async_trait::async_trait]
pub trait ProductControlFacade: Send + Sync + 'static {
    async fn oauth_start(
        &self,
        command: OAuthStartCommand,
    ) -> Result<OAuthStartResult, FacadeError>;

    async fn oauth_callback(
        &self,
        command: OAuthCallbackCommand,
    ) -> Result<OAuthCallbackResult, FacadeError>;

    async fn current_principal(
        &self,
        credential: &SessionCredential,
    ) -> Result<CurrentPrincipal, FacadeError>;

    async fn authority_check(
        &self,
        credential: &SessionCredential,
        installation_id: &str,
    ) -> Result<(), FacadeError>;

    async fn revoke_session(
        &self,
        credential: &SessionCredential,
        csrf: &CsrfSecret,
    ) -> Result<(), FacadeError>;

    async fn promote(
        &self,
        credential: &SessionCredential,
        csrf: &CsrfSecret,
        command: PromoteCommand,
    ) -> Result<PromotionView, FacadeError>;

    async fn status(
        &self,
        credential: &SessionCredential,
        installation_id: &str,
        promotion_id: &str,
    ) -> Result<DecisionView, FacadeError>;

    async fn approval_preview(
        &self,
        credential: &SessionCredential,
        installation_id: &str,
        promotion_id: &str,
    ) -> Result<ApprovalPreviewView, FacadeError>;

    async fn approve(
        &self,
        credential: &SessionCredential,
        csrf: &CsrfSecret,
        command: DecisionCommand,
    ) -> Result<DecisionView, FacadeError>;

    async fn reject(
        &self,
        credential: &SessionCredential,
        csrf: &CsrfSecret,
        command: RejectCommand,
    ) -> Result<DecisionView, FacadeError>;

    async fn apply(
        &self,
        credential: &SessionCredential,
        csrf: &CsrfSecret,
        command: ApplyCommand,
    ) -> Result<ApplyView, FacadeError>;

    async fn deployment(
        &self,
        credential: &SessionCredential,
        installation_id: &str,
        promotion_id: &str,
    ) -> Result<DeploymentView, FacadeError>;

    async fn readiness(&self) -> Result<(), FacadeError>;
}

#[async_trait::async_trait]
pub trait ProductControlOperationalFacadeV2: ProductControlFacade {
    async fn deployment_operational_v2(
        &self,
        credential: &SessionCredential,
        installation_id: &str,
        promotion_id: &str,
    ) -> Result<DeploymentOperationalViewV2, FacadeError>;
}

#[async_trait::async_trait]
pub trait ProductControlLifecycleFacadeV1: ProductControlFacade {
    async fn cancel_lifecycle(
        &self,
        credential: &SessionCredential,
        csrf: &CsrfSecret,
        command: LifecycleCancellationCommand,
    ) -> Result<LifecycleCancellationView, FacadeError>;
}

pub(crate) fn validate_scoped_path(installation_id: &str, promotion_id: &str) -> bool {
    valid_resource_id(installation_id) && valid_digest(promotion_id)
}

pub(crate) fn is_live_exact_replay(view: &ApplyView) -> bool {
    view.replayed && view.state == ProductState::Live
}
