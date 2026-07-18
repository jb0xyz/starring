use authoring_promotion::{
    ApprovalPolicyV1, AuthenticatedPromotionContext, AuthoringSessionId, AutomationInstallationId,
    BindingRevision, CreatePromotionOutcomeV1, IdempotencyKey, PromotionError, PromotionService,
    ResumePromotionOutcomeV1, SessionGeneration, StartPromotionV1,
};
use automation_ruleset::RuleSetKey;
use design_harness::PreviewReadyArtifactV1;
use discord_model::{GuildId, UserId};

use crate::{
    AuthenticatedActorV1, AuthenticationError, AuthorizedInstallationScopeV1,
    FreshGuildAuthorityError,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromoteOwnedSessionV1 {
    pub idempotency_key: IdempotencyKey,
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
}

impl From<AuthorizedPromotionSnapshotError> for AuthoringApplicationError {
    fn from(value: AuthorizedPromotionSnapshotError) -> Self {
        match value {
            AuthorizedPromotionSnapshotError::Session(error) => Self::Session(error),
            AuthorizedPromotionSnapshotError::Authority(error) => Self::Authority(error),
        }
    }
}

pub(crate) fn build_start_promotion(
    actor: &AuthenticatedActorV1,
    scope: &AuthorizedInstallationScopeV1,
    command: PromoteOwnedSessionV1,
    snapshot: AuthorizedPromotionSnapshotV1,
) -> Result<StartPromotionV1, PromotionAuthorityError> {
    let (artifact, authority) = snapshot.into_parts();
    if authority.installation_id != *scope.installation_id()
        || authority.guild_id != scope.guild_id()
        || authority.requester != scope.acting_user_id()
    {
        return Err(PromotionAuthorityError::ScopeMismatch);
    }
    Ok(StartPromotionV1 {
        idempotency_key: command.idempotency_key,
        context: AuthenticatedPromotionContext {
            tenant_id: scope.tenant_id().clone(),
            principal_id: actor.principal_id().clone(),
            session_owner_id: actor.principal_id().clone(),
            session_id: command.session_id,
            session_generation: command.expected_generation,
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
