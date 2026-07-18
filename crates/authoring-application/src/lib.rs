use authoring_promotion::{
    ApprovalPolicyV1, AuthenticatedPromotionContext, AuthoringSessionId, AutomationInstallationId,
    BindingRevision, CreatePromotionOutcomeV1, IdempotencyKey, PrincipalId, PromotionError,
    PromotionService, ResumePromotionOutcomeV1, SessionGeneration, StartPromotionV1, TenantId,
};
use automation_ruleset::RuleSetKey;
use design_harness::PreviewReadyArtifactV1;
use discord_model::{GuildId, UserId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthenticatedIdentityV1 {
    tenant_id: TenantId,
    principal_id: PrincipalId,
}

impl AuthenticatedIdentityV1 {
    pub fn from_authentication(tenant_id: TenantId, principal_id: PrincipalId) -> Self {
        Self {
            tenant_id,
            principal_id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthenticatedActorV1 {
    identity: AuthenticatedIdentityV1,
}

impl AuthenticatedActorV1 {
    fn from_identity(identity: AuthenticatedIdentityV1) -> Self {
        Self { identity }
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.identity.tenant_id
    }

    pub fn principal_id(&self) -> &PrincipalId {
        &self.identity.principal_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthenticationError {
    #[error("authentication credential is invalid")]
    InvalidCredential,
    #[error("authentication credential has expired")]
    Expired,
    #[error("authentication credential was revoked")]
    Revoked,
    #[error("authentication backend failed: {0}")]
    Backend(String),
}

#[allow(async_fn_in_trait)]
pub trait AuthenticationPort {
    type Credential: ?Sized;

    async fn authenticate(
        &self,
        credential: &Self::Credential,
    ) -> Result<AuthenticatedIdentityV1, AuthenticationError>;
}

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

    fn into_parts(self) -> (PreviewReadyArtifactV1, ResolvedPromotionAuthorityV1) {
        (self.artifact.into_inner(), self.authority)
    }
}

impl OwnedPreviewReadyArtifactV1 {
    pub fn from_owned_session(artifact: PreviewReadyArtifactV1) -> Self {
        Self { artifact }
    }

    fn into_inner(self) -> PreviewReadyArtifactV1 {
        self.artifact
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
pub trait AuthorizedPromotionSnapshotPort {
    async fn load_atomic_authorized_snapshot(
        &self,
        actor: &AuthenticatedActorV1,
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

pub struct AuthoringApplication<'a, A, S, P> {
    authentication: &'a A,
    snapshots: &'a S,
    promotions: &'a P,
}

impl<'a, A, S, P> AuthoringApplication<'a, A, S, P> {
    pub fn new(authentication: &'a A, snapshots: &'a S, promotions: &'a P) -> Self {
        Self {
            authentication,
            snapshots,
            promotions,
        }
    }
}

impl<A, S, P> AuthoringApplication<'_, A, S, P>
where
    A: AuthenticationPort,
    S: AuthorizedPromotionSnapshotPort,
    P: PromotionSubmissionPort,
{
    pub async fn promote_owned_session(
        &self,
        credential: &A::Credential,
        command: PromoteOwnedSessionV1,
    ) -> Result<P::Output, AuthoringApplicationError> {
        let identity = self.authentication.authenticate(credential).await?;
        let actor = AuthenticatedActorV1::from_identity(identity);
        let snapshot = self
            .snapshots
            .load_atomic_authorized_snapshot(
                &actor,
                &command.session_id,
                command.expected_generation,
            )
            .await?;
        let (artifact, authority) = snapshot.into_parts();
        self.promotions
            .submit_verified_promotion(StartPromotionV1 {
                idempotency_key: command.idempotency_key,
                context: AuthenticatedPromotionContext {
                    tenant_id: actor.tenant_id().clone(),
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
            .await
            .map_err(AuthoringApplicationError::Promotion)
    }
}
