use std::fmt::{Debug, Formatter};
use std::num::NonZeroU64;

use authoring_promotion::{AutomationInstallationId, ProductApprovalPayloadV1, PromotionId};
use discord_model::GuildId;

use crate::{
    AuthenticatedActorV1, AuthenticatedSessionFingerprintV1, AuthorizedInstallationScopeV1,
    ProductDecisionPhaseV1, ProductDecisionProjectionV1,
};

const REJECTION_REASON_MAX_SCALARS: usize = 1_000;
const REJECTION_REASON_MAX_BYTES: usize = 4_000;
const IDEMPOTENCY_KEY_MAX_BYTES: usize = 128;
const REQUEST_ID_MAX_BYTES: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductRequestIdError {
    #[error("product request ID must not be empty")]
    Empty,
    #[error("product request ID exceeds {REQUEST_ID_MAX_BYTES} bytes")]
    TooLong,
    #[error("product request ID contains invalid characters")]
    InvalidCharacter,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProductRequestIdV1(String);

impl ProductRequestIdV1 {
    pub fn parse(value: &str) -> Result<Self, ProductRequestIdError> {
        if value.is_empty() {
            return Err(ProductRequestIdError::Empty);
        }
        if value.len() > REQUEST_ID_MAX_BYTES {
            return Err(ProductRequestIdError::TooLong);
        }
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
        {
            return Err(ProductRequestIdError::InvalidCharacter);
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for ProductRequestIdV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductRequestIdV1(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromotionSelectorV1 {
    promotion_id: PromotionId,
}

impl PromotionSelectorV1 {
    pub fn new(promotion_id: PromotionId) -> Self {
        Self { promotion_id }
    }

    pub fn promotion_id(&self) -> &PromotionId {
        &self.promotion_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductIdempotencyKeyError {
    #[error("product idempotency key must not be empty")]
    Empty,
    #[error("product idempotency key exceeds {IDEMPOTENCY_KEY_MAX_BYTES} bytes")]
    TooLong,
    #[error("product idempotency key contains invalid characters")]
    InvalidCharacter,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProductIdempotencyKeyV1(String);

impl ProductIdempotencyKeyV1 {
    pub fn parse(value: &str) -> Result<Self, ProductIdempotencyKeyError> {
        if value.is_empty() {
            return Err(ProductIdempotencyKeyError::Empty);
        }
        if value.len() > IDEMPOTENCY_KEY_MAX_BYTES {
            return Err(ProductIdempotencyKeyError::TooLong);
        }
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
        {
            return Err(ProductIdempotencyKeyError::InvalidCharacter);
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for ProductIdempotencyKeyV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductIdempotencyKeyV1(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ApprovalPayloadDigestError {
    #[error("approval payload digest must be lowercase SHA-256 hexadecimal")]
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApprovalPayloadDigestV1(String);

impl ApprovalPayloadDigestV1 {
    pub fn parse(value: &str) -> Result<Self, ApprovalPayloadDigestError> {
        if value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            Ok(Self(value.to_string()))
        } else {
            Err(ApprovalPayloadDigestError::Invalid)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductRevisionError {
    #[error("product revision must be nonzero")]
    Zero,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProductRevisionV1(NonZeroU64);

impl ProductRevisionV1 {
    pub fn new(value: u64) -> Result<Self, ProductRevisionError> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(ProductRevisionError::Zero)
    }

    pub fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RejectionReasonError {
    #[error("rejection reason must not be empty")]
    Empty,
    #[error(
        "rejection reason exceeds {REJECTION_REASON_MAX_SCALARS} Unicode scalars or {REJECTION_REASON_MAX_BYTES} bytes"
    )]
    TooLong,
    #[error("rejection reason contains control characters")]
    ControlCharacter,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RejectionReasonV1(String);

impl RejectionReasonV1 {
    pub fn parse(value: &str) -> Result<Self, RejectionReasonError> {
        let value = value.trim();
        if value.is_empty() {
            return Err(RejectionReasonError::Empty);
        }
        if value.len() > REJECTION_REASON_MAX_BYTES
            || value.chars().count() > REJECTION_REASON_MAX_SCALARS
        {
            return Err(RejectionReasonError::TooLong);
        }
        if value.chars().any(char::is_control) {
            return Err(RejectionReasonError::ControlCharacter);
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for RejectionReasonV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RejectionReasonV1(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductStatusQueryV1 {
    pub promotion: PromotionSelectorV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApproveProductPromotionV1 {
    pub promotion: PromotionSelectorV1,
    pub expected_payload_digest: ApprovalPayloadDigestV1,
    pub expected_revision: ProductRevisionV1,
    pub idempotency_key: ProductIdempotencyKeyV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectProductPromotionV1 {
    pub promotion: PromotionSelectorV1,
    pub expected_payload_digest: ApprovalPayloadDigestV1,
    pub expected_revision: ProductRevisionV1,
    pub idempotency_key: ProductIdempotencyKeyV1,
    pub reason: RejectionReasonV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplyProductPromotionV1 {
    pub promotion: PromotionSelectorV1,
    pub expected_payload_digest: ApprovalPayloadDigestV1,
    pub expected_revision: ProductRevisionV1,
    pub idempotency_key: ProductIdempotencyKeyV1,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProductApprovalPreviewV1 {
    installation_id: AutomationInstallationId,
    guild_id: GuildId,
    payload: ProductApprovalPayloadV1,
    payload_digest: ApprovalPayloadDigestV1,
    revision: ProductRevisionV1,
    phase: ProductDecisionPhaseV1,
}

impl ProductApprovalPreviewV1 {
    pub fn from_server_projection(
        installation_id: AutomationInstallationId,
        guild_id: GuildId,
        payload: ProductApprovalPayloadV1,
        payload_digest: ApprovalPayloadDigestV1,
        revision: ProductRevisionV1,
        phase: ProductDecisionPhaseV1,
    ) -> Self {
        Self {
            installation_id,
            guild_id,
            payload,
            payload_digest,
            revision,
            phase,
        }
    }

    pub fn installation_id(&self) -> &AutomationInstallationId {
        &self.installation_id
    }

    pub fn guild_id(&self) -> GuildId {
        self.guild_id
    }

    pub fn payload(&self) -> &ProductApprovalPayloadV1 {
        &self.payload
    }

    pub fn payload_digest(&self) -> &ApprovalPayloadDigestV1 {
        &self.payload_digest
    }

    pub fn revision(&self) -> ProductRevisionV1 {
        self.revision
    }

    pub fn phase(&self) -> &ProductDecisionPhaseV1 {
        &self.phase
    }
}

impl Debug for ProductApprovalPreviewV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProductApprovalPreviewV1")
            .field("installation_id", &self.installation_id)
            .field("guild_id", &self.guild_id)
            .field("payload", &self.payload)
            .field("payload_digest", &self.payload_digest)
            .field("revision", &self.revision)
            .field("phase", &self.phase)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductMutationReceiptV1 {
    projection: ProductDecisionProjectionV1,
    exact_replay: bool,
}

impl ProductMutationReceiptV1 {
    pub fn from_server_projection(
        projection: ProductDecisionProjectionV1,
        exact_replay: bool,
    ) -> Self {
        Self {
            projection,
            exact_replay,
        }
    }

    pub fn projection(&self) -> &ProductDecisionProjectionV1 {
        &self.projection
    }

    pub fn exact_replay(&self) -> bool {
        self.exact_replay
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductCandidateErrorCodeV1 {
    #[error("product target artifact is corrupt")]
    TargetCorrupt,
    #[error("authoritative product binding revision is unavailable")]
    BindingRevisionUnavailable,
    #[error("product target schema is unsupported")]
    UnsupportedSchema,
    #[error("product target structure is invalid")]
    StructurallyInvalid,
    #[error("product target hash could not be verified")]
    HashComputationFailed,
    #[error("product target hash does not match its content")]
    HashMismatch,
    #[error("product target bindings are invalid")]
    BindingInvalid,
    #[error("product target violates a blocking policy")]
    BlockingPolicy,
    #[error("product target requires unavailable capabilities")]
    MissingCapabilities,
    #[error("product target role hierarchy evidence is unavailable")]
    RoleHierarchyUnavailable,
    #[error("product target role hierarchy evidence is incomplete")]
    RoleHierarchyIncomplete,
    #[error("product target requires a role the bot cannot manage")]
    RoleUnmanageable,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductControlPortError {
    #[error("promotion was not found")]
    NotFound,
    #[error("promotion does not belong to the authorized installation")]
    ScopeMismatch,
    #[error("product revision does not match")]
    RevisionConflict,
    #[error("approval payload digest does not match")]
    PayloadMismatch,
    #[error("product decision is not valid in the current state")]
    InvalidState,
    #[error("requester self-approval is forbidden")]
    SelfApprovalForbidden,
    #[error("the same product decision already exists")]
    DuplicateDecision,
    #[error("promotion approval window has expired")]
    Expired,
    #[error("idempotency key conflicts with a different command")]
    IdempotencyConflict,
    #[error("server-owned product candidate is invalid: {0}")]
    InvalidServerCandidate(ProductCandidateErrorCodeV1),
    #[error("promotion was superseded by newer server state")]
    Superseded,
    #[error("product decision outcome is indeterminate: {0}")]
    Indeterminate(String),
    #[error("product decision backend failed: {0}")]
    Backend(String),
}

pub struct ProductMutationContextV1<'a, E> {
    request_id: &'a ProductRequestIdV1,
    actor: &'a AuthenticatedActorV1,
    scope: &'a AuthorizedInstallationScopeV1,
    evidence: &'a E,
}

impl<'a, E> ProductMutationContextV1<'a, E> {
    pub(crate) fn new(
        request_id: &'a ProductRequestIdV1,
        actor: &'a AuthenticatedActorV1,
        scope: &'a AuthorizedInstallationScopeV1,
        evidence: &'a E,
    ) -> Self {
        Self {
            request_id,
            actor,
            scope,
            evidence,
        }
    }

    pub fn request_id(&self) -> &ProductRequestIdV1 {
        self.request_id
    }

    pub fn actor(&self) -> &AuthenticatedActorV1 {
        self.actor
    }

    pub fn session_fingerprint(&self) -> &AuthenticatedSessionFingerprintV1 {
        self.actor.session_fingerprint()
    }

    pub fn scope(&self) -> &AuthorizedInstallationScopeV1 {
        self.scope
    }

    pub fn evidence(&self) -> &E {
        self.evidence
    }
}

impl<E> Debug for ProductMutationContextV1<'_, E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductMutationContextV1(<redacted>)")
    }
}

pub struct AuthorizedApprovalPreviewV1<'a, E> {
    actor: &'a AuthenticatedActorV1,
    scope: &'a AuthorizedInstallationScopeV1,
    evidence: &'a E,
    promotion: &'a PromotionSelectorV1,
}

impl<'a, E> AuthorizedApprovalPreviewV1<'a, E> {
    pub(crate) fn new(
        actor: &'a AuthenticatedActorV1,
        scope: &'a AuthorizedInstallationScopeV1,
        evidence: &'a E,
        promotion: &'a PromotionSelectorV1,
    ) -> Self {
        Self {
            actor,
            scope,
            evidence,
            promotion,
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

    pub fn promotion(&self) -> &PromotionSelectorV1 {
        self.promotion
    }
}

pub struct AuthorizedProductStatusV1<'a, E> {
    actor: &'a AuthenticatedActorV1,
    scope: &'a AuthorizedInstallationScopeV1,
    evidence: &'a E,
    promotion: &'a PromotionSelectorV1,
}

impl<'a, E> AuthorizedProductStatusV1<'a, E> {
    pub(crate) fn new(
        actor: &'a AuthenticatedActorV1,
        scope: &'a AuthorizedInstallationScopeV1,
        evidence: &'a E,
        promotion: &'a PromotionSelectorV1,
    ) -> Self {
        Self {
            actor,
            scope,
            evidence,
            promotion,
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

    pub fn promotion(&self) -> &PromotionSelectorV1 {
        self.promotion
    }
}

pub struct AuthorizedApproveProductV1<'a, E> {
    context: ProductMutationContextV1<'a, E>,
    command: ApproveProductPromotionV1,
}

impl<'a, E> AuthorizedApproveProductV1<'a, E> {
    pub(crate) fn new(
        request_id: &'a ProductRequestIdV1,
        actor: &'a AuthenticatedActorV1,
        scope: &'a AuthorizedInstallationScopeV1,
        evidence: &'a E,
        command: ApproveProductPromotionV1,
    ) -> Self {
        Self {
            context: ProductMutationContextV1::new(request_id, actor, scope, evidence),
            command,
        }
    }

    pub fn context(&self) -> &ProductMutationContextV1<'a, E> {
        &self.context
    }

    pub fn request_id(&self) -> &ProductRequestIdV1 {
        self.context.request_id()
    }

    pub fn actor(&self) -> &AuthenticatedActorV1 {
        self.context.actor()
    }

    pub fn session_fingerprint(&self) -> &AuthenticatedSessionFingerprintV1 {
        self.context.session_fingerprint()
    }

    pub fn scope(&self) -> &AuthorizedInstallationScopeV1 {
        self.context.scope()
    }

    pub fn evidence(&self) -> &E {
        self.context.evidence()
    }

    pub fn command(&self) -> &ApproveProductPromotionV1 {
        &self.command
    }
}

pub struct AuthorizedRejectProductV1<'a, E> {
    context: ProductMutationContextV1<'a, E>,
    command: RejectProductPromotionV1,
}

impl<'a, E> AuthorizedRejectProductV1<'a, E> {
    pub(crate) fn new(
        request_id: &'a ProductRequestIdV1,
        actor: &'a AuthenticatedActorV1,
        scope: &'a AuthorizedInstallationScopeV1,
        evidence: &'a E,
        command: RejectProductPromotionV1,
    ) -> Self {
        Self {
            context: ProductMutationContextV1::new(request_id, actor, scope, evidence),
            command,
        }
    }

    pub fn context(&self) -> &ProductMutationContextV1<'a, E> {
        &self.context
    }

    pub fn request_id(&self) -> &ProductRequestIdV1 {
        self.context.request_id()
    }

    pub fn actor(&self) -> &AuthenticatedActorV1 {
        self.context.actor()
    }

    pub fn session_fingerprint(&self) -> &AuthenticatedSessionFingerprintV1 {
        self.context.session_fingerprint()
    }

    pub fn scope(&self) -> &AuthorizedInstallationScopeV1 {
        self.context.scope()
    }

    pub fn evidence(&self) -> &E {
        self.context.evidence()
    }

    pub fn command(&self) -> &RejectProductPromotionV1 {
        &self.command
    }
}

pub struct AuthorizedApplyProductV1<'a, E> {
    context: ProductMutationContextV1<'a, E>,
    command: ApplyProductPromotionV1,
}

impl<'a, E> AuthorizedApplyProductV1<'a, E> {
    pub(crate) fn new(
        request_id: &'a ProductRequestIdV1,
        actor: &'a AuthenticatedActorV1,
        scope: &'a AuthorizedInstallationScopeV1,
        evidence: &'a E,
        command: ApplyProductPromotionV1,
    ) -> Self {
        Self {
            context: ProductMutationContextV1::new(request_id, actor, scope, evidence),
            command,
        }
    }

    pub fn context(&self) -> &ProductMutationContextV1<'a, E> {
        &self.context
    }

    pub fn request_id(&self) -> &ProductRequestIdV1 {
        self.context.request_id()
    }

    pub fn actor(&self) -> &AuthenticatedActorV1 {
        self.context.actor()
    }

    pub fn session_fingerprint(&self) -> &AuthenticatedSessionFingerprintV1 {
        self.context.session_fingerprint()
    }

    pub fn scope(&self) -> &AuthorizedInstallationScopeV1 {
        self.context.scope()
    }

    pub fn evidence(&self) -> &E {
        self.context.evidence()
    }

    pub fn command(&self) -> &ApplyProductPromotionV1 {
        &self.command
    }
}

#[allow(async_fn_in_trait)]
pub trait ProductDecisionQueryPort<E> {
    async fn load_approval_preview(
        &self,
        request: AuthorizedApprovalPreviewV1<'_, E>,
    ) -> Result<ProductApprovalPreviewV1, ProductControlPortError>;

    async fn load_product_status(
        &self,
        request: AuthorizedProductStatusV1<'_, E>,
    ) -> Result<ProductDecisionProjectionV1, ProductControlPortError>;
}

#[allow(async_fn_in_trait)]
pub trait ProductApprovalPort<E> {
    async fn approve_payload_bound(
        &self,
        request: AuthorizedApproveProductV1<'_, E>,
    ) -> Result<ProductMutationReceiptV1, ProductControlPortError>;
}

#[allow(async_fn_in_trait)]
pub trait ProductRejectionPort<E> {
    async fn reject_payload_bound(
        &self,
        request: AuthorizedRejectProductV1<'_, E>,
    ) -> Result<ProductMutationReceiptV1, ProductControlPortError>;
}

#[allow(async_fn_in_trait)]
pub trait ProductApplyPort<E> {
    async fn apply_idempotent(
        &self,
        request: AuthorizedApplyProductV1<'_, E>,
    ) -> Result<ProductMutationReceiptV1, ProductControlPortError>;
}

pub trait ProductDecisionPort<E>:
    ProductDecisionQueryPort<E> + ProductApprovalPort<E> + ProductRejectionPort<E> + ProductApplyPort<E>
{
}

impl<T, E> ProductDecisionPort<E> for T where
    T: ProductDecisionQueryPort<E>
        + ProductApprovalPort<E>
        + ProductRejectionPort<E>
        + ProductApplyPort<E>
{
}
