use std::fmt::{Debug, Formatter};

use authoring_promotion::{
    ApprovalPolicyV1, AuthenticatedPromotionContext, AuthoringSessionId, AutomationInstallationId,
    BindingRevision, CreatePromotionOutcomeV1, IdempotencyKey, PromotionError, PromotionService,
    ResumePromotionOutcomeV1, SessionGeneration, StartPromotionV1,
};
use automation_ruleset::RuleSetKey;
use design_harness::PreviewReadyArtifactV1;
use discord_model::{GuildId, UserId};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use crate::{
    AuthenticatedActorV1, AuthenticationError, AuthorizedInstallationScopeV1,
    FreshGuildAuthorityError, ProductMutationContextV1, ProductRequestIdV1,
};

const PRODUCT_PROMOTION_IDEMPOTENCY_KEY_MAX_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductPromotionIdempotencyKeyError {
    #[error("product promotion idempotency key must not be empty")]
    Empty,
    #[error(
        "product promotion idempotency key exceeds {PRODUCT_PROMOTION_IDEMPOTENCY_KEY_MAX_BYTES} bytes"
    )]
    TooLong,
    #[error("product promotion idempotency key contains invalid characters")]
    InvalidCharacter,
}

#[derive(Clone)]
pub struct ProductPromotionIdempotencyKeyV1(Zeroizing<String>);

impl ProductPromotionIdempotencyKeyV1 {
    pub fn parse(value: &str) -> Result<Self, ProductPromotionIdempotencyKeyError> {
        if value.is_empty() {
            return Err(ProductPromotionIdempotencyKeyError::Empty);
        }
        if value.len() > PRODUCT_PROMOTION_IDEMPOTENCY_KEY_MAX_BYTES {
            return Err(ProductPromotionIdempotencyKeyError::TooLong);
        }
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
        {
            return Err(ProductPromotionIdempotencyKeyError::InvalidCharacter);
        }
        Ok(Self(Zeroizing::new(value.to_string())))
    }

    fn with_secret<R>(&self, consume: impl FnOnce(&[u8]) -> R) -> R {
        consume(self.0.as_bytes())
    }
}

impl PartialEq for ProductPromotionIdempotencyKeyV1 {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len() && bool::from(self.0.as_bytes().ct_eq(other.0.as_bytes()))
    }
}

impl Eq for ProductPromotionIdempotencyKeyV1 {}

impl Debug for ProductPromotionIdempotencyKeyV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductPromotionIdempotencyKeyV1(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromoteOwnedSessionV1 {
    pub idempotency_key: ProductPromotionIdempotencyKeyV1,
    pub session_id: AuthoringSessionId,
    pub expected_generation: SessionGeneration,
}

#[derive(Clone)]
pub struct OwnedPreviewReadyArtifactV1 {
    artifact: PreviewReadyArtifactV1,
}

impl OwnedPreviewReadyArtifactV1 {
    pub fn from_owned_session(artifact: PreviewReadyArtifactV1) -> Self {
        Self { artifact }
    }

    fn into_inner(self) -> PreviewReadyArtifactV1 {
        self.artifact
    }
}

#[derive(Clone)]
pub struct AuthorizedPromotionSnapshotV1 {
    artifact: OwnedPreviewReadyArtifactV1,
    authority: ResolvedPromotionAuthorityV1,
}

impl AuthorizedPromotionSnapshotV1 {
    pub fn from_atomic_authorization(
        artifact: PreviewReadyArtifactV1,
        authority: ResolvedPromotionAuthorityV1,
    ) -> Self {
        Self {
            artifact: OwnedPreviewReadyArtifactV1::from_owned_session(artifact),
            authority,
        }
    }

    pub(crate) fn into_parts(self) -> (PreviewReadyArtifactV1, ResolvedPromotionAuthorityV1) {
        (self.artifact.into_inner(), self.authority)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedPromotionAuthorityV1 {
    pub guild_id: GuildId,
    pub installation_id: AutomationInstallationId,
    pub ruleset_key: RuleSetKey,
    pub requester: UserId,
    pub binding_revision: BindingRevision,
    pub policy: ApprovalPolicyV1,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum OwnedSessionLoadError {
    #[error("authoring session was not found")]
    NotFound,
    #[error("authenticated principal does not own the authoring session")]
    NotOwned,
    #[error("authoring session generation does not match")]
    GenerationMismatch,
    #[error("authoring session does not hold a preview-ready artifact")]
    NotPreviewReady,
    #[error("authoring session backend failed: {0}")]
    Backend(String),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PromotionAuthorityError {
    #[error("authoring promotion authority was not found")]
    NotFound,
    #[error("authenticated principal is not allowed to promote this authoring session")]
    Forbidden,
    #[error("authoring session generation does not match promotion authority")]
    GenerationMismatch,
    #[error("authorized installation does not match the atomic promotion snapshot")]
    ScopeMismatch,
    #[error("product promotion idempotency key invariant failed")]
    InvalidIdempotencyKey,
    #[error("authoring promotion authority backend failed: {0}")]
    Backend(String),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthorizedPromotionSnapshotError {
    #[error(transparent)]
    Session(#[from] OwnedSessionLoadError),
    #[error(transparent)]
    Authority(#[from] PromotionAuthorityError),
}

#[allow(async_fn_in_trait)]
pub trait AuthorizedPromotionSnapshotPort<E> {
    async fn load_atomic_authorized_snapshot(
        &self,
        actor: &AuthenticatedActorV1,
        scope: &AuthorizedInstallationScopeV1,
        evidence: &E,
        session_id: &AuthoringSessionId,
        expected_generation: SessionGeneration,
    ) -> Result<AuthorizedPromotionSnapshotV1, AuthorizedPromotionSnapshotError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromotionSubmissionDispositionV1 {
    Created,
    ExactReplay,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromotionSubmissionV1 {
    pub disposition: PromotionSubmissionDispositionV1,
    pub advancement: ResumePromotionOutcomeV1,
}

#[allow(async_fn_in_trait)]
pub trait PromotionSubmissionPort {
    type Output;

    async fn submit_verified_promotion(
        &self,
        input: StartPromotionV1,
    ) -> Result<Self::Output, PromotionError>;
}

pub struct AuthorizedPromotionAccessV1<'a, E> {
    context: ProductMutationContextV1<'a, E>,
    session_id: AuthoringSessionId,
    expected_generation: SessionGeneration,
    idempotency_key: ProductPromotionIdempotencyKeyV1,
}

impl<'a, E> AuthorizedPromotionAccessV1<'a, E> {
    pub(crate) fn new(
        request_id: &'a ProductRequestIdV1,
        actor: &'a AuthenticatedActorV1,
        scope: &'a AuthorizedInstallationScopeV1,
        evidence: &'a E,
        command: PromoteOwnedSessionV1,
    ) -> Self {
        Self {
            context: ProductMutationContextV1::new(request_id, actor, scope, evidence),
            session_id: command.session_id,
            expected_generation: command.expected_generation,
            idempotency_key: command.idempotency_key,
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

    pub fn session_fingerprint(&self) -> &crate::AuthenticatedSessionFingerprintV1 {
        self.context.session_fingerprint()
    }

    pub fn scope(&self) -> &AuthorizedInstallationScopeV1 {
        self.context.scope()
    }

    pub fn evidence(&self) -> &E {
        self.context.evidence()
    }

    pub fn session_id(&self) -> &AuthoringSessionId {
        &self.session_id
    }

    pub fn expected_generation(&self) -> SessionGeneration {
        self.expected_generation
    }

    pub fn with_product_idempotency_secret<R>(&self, consume: impl FnOnce(&[u8]) -> R) -> R {
        self.idempotency_key.with_secret(consume)
    }
}

impl<E> Debug for AuthorizedPromotionAccessV1<'_, E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AuthorizedPromotionAccessV1(<redacted>)")
    }
}

pub struct AuthorizedPromotionSubmissionV1<'a, E> {
    access: AuthorizedPromotionAccessV1<'a, E>,
    input: StartPromotionV1,
}

impl<'a, E> AuthorizedPromotionSubmissionV1<'a, E> {
    pub(crate) fn new(access: AuthorizedPromotionAccessV1<'a, E>, input: StartPromotionV1) -> Self {
        Self { access, input }
    }

    pub fn access(&self) -> &AuthorizedPromotionAccessV1<'a, E> {
        &self.access
    }

    pub fn context(&self) -> &ProductMutationContextV1<'a, E> {
        self.access.context()
    }

    pub fn request_id(&self) -> &ProductRequestIdV1 {
        self.access.request_id()
    }

    pub fn actor(&self) -> &AuthenticatedActorV1 {
        self.access.actor()
    }

    pub fn session_fingerprint(&self) -> &crate::AuthenticatedSessionFingerprintV1 {
        self.access.session_fingerprint()
    }

    pub fn scope(&self) -> &AuthorizedInstallationScopeV1 {
        self.access.scope()
    }

    pub fn evidence(&self) -> &E {
        self.access.evidence()
    }

    pub fn session_id(&self) -> &AuthoringSessionId {
        self.access.session_id()
    }

    pub fn expected_generation(&self) -> SessionGeneration {
        self.access.expected_generation()
    }

    pub fn with_product_idempotency_secret<R>(&self, consume: impl FnOnce(&[u8]) -> R) -> R {
        self.access.with_product_idempotency_secret(consume)
    }

    pub fn input(&self) -> &StartPromotionV1 {
        &self.input
    }

    pub fn into_input(self) -> StartPromotionV1 {
        self.input
    }

    pub fn into_access_and_input(self) -> (AuthorizedPromotionAccessV1<'a, E>, StartPromotionV1) {
        (self.access, self.input)
    }
}

impl<E> Debug for AuthorizedPromotionSubmissionV1<'_, E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AuthorizedPromotionSubmissionV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthorizedPromotionBackendFailureV1 {
    #[error("promotion backend request timed out")]
    Timeout,
    #[error("promotion backend request can be retried")]
    Retryable,
    #[error("promotion backend is unavailable")]
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthorizedPromotionSubmissionErrorV1 {
    #[error("promotion target was not found")]
    NotFound,
    #[error("authoring session generation does not match")]
    GenerationMismatch,
    #[error("authenticated principal is not allowed to promote this authoring session")]
    Forbidden,
    #[error("promotion target does not belong to the authorized installation")]
    ScopeMismatch,
    #[error("idempotency key conflicts with a different promotion")]
    IdempotencyConflict,
    #[error("server-owned promotion candidate is invalid")]
    InvalidCandidate,
    #[error("durable promotion state is corrupt")]
    PersistenceCorrupt,
    #[error("promotion outcome is indeterminate")]
    Indeterminate,
    #[error(transparent)]
    Backend(#[from] AuthorizedPromotionBackendFailureV1),
}

#[allow(async_fn_in_trait)]
pub trait AuthorizedPromotionSubmissionPort<E> {
    async fn find_or_resume_authorized_promotion(
        &self,
        access: &AuthorizedPromotionAccessV1<'_, E>,
    ) -> Result<Option<PromotionSubmissionV1>, AuthorizedPromotionSubmissionErrorV1>;

    async fn submit_authorized_promotion(
        &self,
        request: AuthorizedPromotionSubmissionV1<'_, E>,
    ) -> Result<PromotionSubmissionV1, AuthorizedPromotionSubmissionErrorV1>;
}

impl<S, P, A, C> PromotionSubmissionPort for PromotionService<'_, S, P, A, C>
where
    S: authoring_promotion::PromotionStore,
    P: authoring_promotion::RuleSetPublicationPort,
    A: authoring_promotion::PendingActivationPort,
    C: authoring_promotion::PromotionClock,
{
    type Output = PromotionSubmissionV1;

    async fn submit_verified_promotion(
        &self,
        input: StartPromotionV1,
    ) -> Result<Self::Output, PromotionError> {
        let creation = self.start(input).await?;
        let (disposition, promotion_id) = match creation {
            CreatePromotionOutcomeV1::Created(record) => {
                (PromotionSubmissionDispositionV1::Created, record.id.clone())
            }
            CreatePromotionOutcomeV1::ExactReplay(record) => (
                PromotionSubmissionDispositionV1::ExactReplay,
                record.id.clone(),
            ),
        };
        let advancement = self.resume_to_activation_pending(&promotion_id).await?;
        Ok(PromotionSubmissionV1 {
            disposition,
            advancement,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthoringApplicationError {
    #[error(transparent)]
    Authentication(#[from] AuthenticationError),
    #[error(transparent)]
    FreshAuthority(#[from] FreshGuildAuthorityError),
    #[error(transparent)]
    Session(#[from] OwnedSessionLoadError),
    #[error(transparent)]
    Authority(#[from] PromotionAuthorityError),
    #[error(transparent)]
    Promotion(#[from] PromotionError),
    #[error(transparent)]
    AuthorizedPromotion(#[from] AuthorizedPromotionSubmissionErrorV1),
}

impl From<AuthorizedPromotionSnapshotError> for AuthoringApplicationError {
    fn from(value: AuthorizedPromotionSnapshotError) -> Self {
        match value {
            AuthorizedPromotionSnapshotError::Session(error) => Self::Session(error),
            AuthorizedPromotionSnapshotError::Authority(error) => Self::Authority(error),
        }
    }
}

pub(crate) fn build_start_promotion<E>(
    actor: &AuthenticatedActorV1,
    scope: &AuthorizedInstallationScopeV1,
    access: &AuthorizedPromotionAccessV1<'_, E>,
    snapshot: AuthorizedPromotionSnapshotV1,
) -> Result<StartPromotionV1, PromotionAuthorityError> {
    let (artifact, authority) = snapshot.into_parts();
    if authority.installation_id != *scope.installation_id()
        || authority.guild_id != scope.guild_id()
        || authority.requester != scope.acting_user_id()
    {
        return Err(PromotionAuthorityError::ScopeMismatch);
    }
    let idempotency_key = access
        .with_product_idempotency_secret(|secret| {
            std::str::from_utf8(secret)
                .ok()
                .and_then(|secret| IdempotencyKey::parse(secret).ok())
        })
        .ok_or(PromotionAuthorityError::InvalidIdempotencyKey)?;
    Ok(StartPromotionV1 {
        idempotency_key,
        context: AuthenticatedPromotionContext {
            tenant_id: scope.tenant_id().clone(),
            principal_id: actor.principal_id().clone(),
            session_owner_id: actor.principal_id().clone(),
            session_id: access.session_id().clone(),
            session_generation: access.expected_generation(),
            guild_id: authority.guild_id,
            installation_id: authority.installation_id,
            ruleset_key: authority.ruleset_key,
            requester: authority.requester,
            binding_revision: authority.binding_revision,
            policy: authority.policy,
        },
        artifact,
    })
}

pub(crate) fn validate_authorized_submission(
    expected: &AuthenticatedPromotionContext,
    submission: &PromotionSubmissionV1,
) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
    let record = validated_final_record(submission)?;
    if &record.intent.authority != expected {
        return Err(AuthorizedPromotionSubmissionErrorV1::ScopeMismatch);
    }
    Ok(())
}

pub(crate) fn validate_authorized_replay<E>(
    access: &AuthorizedPromotionAccessV1<'_, E>,
    submission: &PromotionSubmissionV1,
) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
    let record = validated_final_record(submission)?;
    let authority = &record.intent.authority;
    if authority.tenant_id != *access.scope().tenant_id()
        || authority.installation_id != *access.scope().installation_id()
        || authority.guild_id != access.scope().guild_id()
        || authority.principal_id != *access.actor().principal_id()
        || authority.session_owner_id != *access.actor().principal_id()
        || authority.requester != access.scope().acting_user_id()
        || authority.session_id != *access.session_id()
        || authority.session_generation != access.expected_generation()
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::ScopeMismatch);
    }
    Ok(())
}

fn validated_final_record(
    submission: &PromotionSubmissionV1,
) -> Result<&authoring_promotion::PromotionRecordV1, AuthorizedPromotionSubmissionErrorV1> {
    let record = match &submission.advancement {
        ResumePromotionOutcomeV1::Advanced(record)
        | ResumePromotionOutcomeV1::AlreadyActivationPending(record)
        | ResumePromotionOutcomeV1::TerminalExpired(record) => record,
    };
    record
        .validate()
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    match (&submission.advancement, &record.stage) {
        (
            ResumePromotionOutcomeV1::Advanced(_)
            | ResumePromotionOutcomeV1::AlreadyActivationPending(_),
            authoring_promotion::PromotionStageV1::ActivationPending { .. },
        )
        | (
            ResumePromotionOutcomeV1::Advanced(_) | ResumePromotionOutcomeV1::TerminalExpired(_),
            authoring_promotion::PromotionStageV1::Expired { .. },
        ) => Ok(record),
        _ => Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt),
    }
}
