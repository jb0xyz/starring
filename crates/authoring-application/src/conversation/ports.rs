use std::fmt::{Debug, Formatter};

use authoring_promotion::{AuthoringSessionId, PrincipalId, SessionGeneration};
use design_harness::{
    PreviewReadyArtifactV1, ResourceBindingFingerprint, ResourceBindingMap, SessionSnapshot,
};

use super::{
    AuthoringExpectedGenerationV1, AuthoringHumanMessageV1, LocalAuthoringRequestKeyV1,
    ReadAuthoringSessionV1, SafeAuthoringProjectionError, SafeAuthoringTurnProjectionV1,
    StartOrAdvanceAuthoringTurnV1,
};
use crate::{AuthenticatedActorV1, AuthorizedInstallationScopeV1, ProductIdempotencyKeyV1};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthoringSessionLoadError {
    #[error("authoring session state is unavailable")]
    Unavailable,
    #[error("authoring session backend timed out")]
    Timeout,
    #[error("authoring session backend request can be retried")]
    Retryable,
    #[error("authoring session state is invalid")]
    InvalidState,
}

pub struct AuthorizedConversationAccessV1<'a, E> {
    actor: &'a AuthenticatedActorV1,
    scope: &'a AuthorizedInstallationScopeV1,
    evidence: &'a E,
    command: &'a StartOrAdvanceAuthoringTurnV1,
}

impl<'a, E> AuthorizedConversationAccessV1<'a, E> {
    pub(crate) fn new(
        actor: &'a AuthenticatedActorV1,
        scope: &'a AuthorizedInstallationScopeV1,
        evidence: &'a E,
        command: &'a StartOrAdvanceAuthoringTurnV1,
    ) -> Self {
        Self {
            actor,
            scope,
            evidence,
            command,
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

    pub fn command(&self) -> &StartOrAdvanceAuthoringTurnV1 {
        self.command
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct AuthoringStoredRequestIdentityV1 {
    scope: AuthorizedInstallationScopeV1,
    principal_id: PrincipalId,
    session_id: AuthoringSessionId,
    expected_generation: AuthoringExpectedGenerationV1,
    idempotency_key: ProductIdempotencyKeyV1,
    human_message: AuthoringHumanMessageV1,
}

impl AuthoringStoredRequestIdentityV1 {
    pub fn from_verified_storage_match(
        scope: AuthorizedInstallationScopeV1,
        principal_id: PrincipalId,
        session_id: AuthoringSessionId,
        expected_generation: AuthoringExpectedGenerationV1,
        idempotency_key: ProductIdempotencyKeyV1,
        human_message: AuthoringHumanMessageV1,
    ) -> Self {
        Self {
            scope,
            principal_id,
            session_id,
            expected_generation,
            idempotency_key,
            human_message,
        }
    }

    pub(crate) fn from_access<E>(access: &AuthorizedConversationAccessV1<'_, E>) -> Self {
        let command = access.command();
        Self {
            scope: access.scope().clone(),
            principal_id: access.actor().principal_id().clone(),
            session_id: command.session_id().clone(),
            expected_generation: command.expected_generation(),
            idempotency_key: command.idempotency_key().clone(),
            human_message: command.human_message().clone(),
        }
    }

    pub(crate) fn matches_access<E>(&self, access: &AuthorizedConversationAccessV1<'_, E>) -> bool {
        let command = access.command();
        &self.scope == access.scope()
            && &self.principal_id == access.actor().principal_id()
            && &self.session_id == command.session_id()
            && self.expected_generation == command.expected_generation()
            && &self.idempotency_key == command.idempotency_key()
            && &self.human_message == command.human_message()
    }
}

impl Debug for AuthoringStoredRequestIdentityV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AuthoringStoredRequestIdentityV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq)]
pub struct AuthoringStoredGenerationV1 {
    request_identity: AuthoringStoredRequestIdentityV1,
    generation: SessionGeneration,
    projection: Box<SafeAuthoringTurnProjectionV1>,
}

impl AuthoringStoredGenerationV1 {
    pub fn from_storage(
        request_identity: AuthoringStoredRequestIdentityV1,
        generation: SessionGeneration,
        projection: SafeAuthoringTurnProjectionV1,
        preview_ready_artifact: Option<&PreviewReadyArtifactV1>,
    ) -> Result<Self, SafeAuthoringProjectionError> {
        projection.validate_for_storage(preview_ready_artifact)?;
        Ok(Self {
            request_identity,
            generation,
            projection: Box::new(projection),
        })
    }

    pub fn request_identity(&self) -> &AuthoringStoredRequestIdentityV1 {
        &self.request_identity
    }

    pub fn generation(&self) -> SessionGeneration {
        self.generation
    }

    pub fn projection(&self) -> &SafeAuthoringTurnProjectionV1 {
        self.projection.as_ref()
    }

    pub fn into_projection(self) -> SafeAuthoringTurnProjectionV1 {
        *self.projection
    }
}

impl Debug for AuthoringStoredGenerationV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AuthoringStoredGenerationV1(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AuthoringTurnCheckV1 {
    Proceed,
    ExactReplay(AuthoringStoredGenerationV1),
    IdempotencyConflict,
    GenerationConflict {
        current_generation: Option<SessionGeneration>,
    },
}

pub struct AuthoringSessionLoadV1 {
    head_generation: Option<SessionGeneration>,
    snapshot: Option<SessionSnapshot>,
    bindings: ResourceBindingMap,
}

impl Debug for AuthoringSessionLoadV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AuthoringSessionLoadV1(<redacted>)")
    }
}

impl AuthoringSessionLoadV1 {
    pub fn from_storage(
        head_generation: Option<SessionGeneration>,
        snapshot: Option<SessionSnapshot>,
        bindings: ResourceBindingMap,
    ) -> Result<Self, AuthoringSessionLoadError> {
        if head_generation.is_some() != snapshot.is_some() {
            return Err(AuthoringSessionLoadError::InvalidState);
        }
        Ok(Self {
            head_generation,
            snapshot,
            bindings,
        })
    }

    pub fn head_generation(&self) -> Option<SessionGeneration> {
        self.head_generation
    }

    pub fn snapshot(&self) -> Option<&SessionSnapshot> {
        self.snapshot.as_ref()
    }

    pub fn bindings(&self) -> &ResourceBindingMap {
        &self.bindings
    }

    pub(crate) fn into_snapshot_and_bindings(
        self,
    ) -> (Option<SessionSnapshot>, ResourceBindingMap) {
        (self.snapshot, self.bindings)
    }
}

#[allow(async_fn_in_trait)]
pub trait AuthoringSessionLoadPort<E> {
    async fn check_replay_or_head(
        &self,
        access: &AuthorizedConversationAccessV1<'_, E>,
    ) -> Result<AuthoringTurnCheckV1, AuthoringSessionLoadError>;

    async fn load_exact_generation(
        &self,
        access: &AuthorizedConversationAccessV1<'_, E>,
    ) -> Result<AuthoringSessionLoadV1, AuthoringSessionLoadError>;
}

pub struct AuthorizedAuthoringCommitV1<'a, E> {
    access: AuthorizedConversationAccessV1<'a, E>,
    resource_bindings: ResourceBindingMap,
    binding_fingerprint: ResourceBindingFingerprint,
    snapshot: SessionSnapshot,
    projection: SafeAuthoringTurnProjectionV1,
    preview_ready_artifact: Option<PreviewReadyArtifactV1>,
}

impl<'a, E> AuthorizedAuthoringCommitV1<'a, E> {
    pub(crate) fn new(
        access: AuthorizedConversationAccessV1<'a, E>,
        resource_bindings: ResourceBindingMap,
        binding_fingerprint: ResourceBindingFingerprint,
        snapshot: SessionSnapshot,
        projection: SafeAuthoringTurnProjectionV1,
        preview_ready_artifact: Option<PreviewReadyArtifactV1>,
    ) -> Self {
        Self {
            access,
            resource_bindings,
            binding_fingerprint,
            snapshot,
            projection,
            preview_ready_artifact,
        }
    }

    pub fn access(&self) -> &AuthorizedConversationAccessV1<'a, E> {
        &self.access
    }

    pub fn binding_fingerprint(&self) -> &ResourceBindingFingerprint {
        &self.binding_fingerprint
    }

    pub fn resource_bindings(&self) -> &ResourceBindingMap {
        &self.resource_bindings
    }

    pub fn snapshot(&self) -> &SessionSnapshot {
        &self.snapshot
    }

    pub fn projection(&self) -> &SafeAuthoringTurnProjectionV1 {
        &self.projection
    }

    pub fn preview_ready_artifact(&self) -> Option<&PreviewReadyArtifactV1> {
        self.preview_ready_artifact.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AuthoringCommitOutcomeV1 {
    Created(AuthoringStoredGenerationV1),
    ExactReplay(AuthoringStoredGenerationV1),
    IdempotencyConflict,
    GenerationConflict {
        current_generation: Option<SessionGeneration>,
    },
    AuthorityConflict,
    BindingConflict,
}

#[allow(async_fn_in_trait)]
pub trait AuthoringSessionCommitPort<E> {
    async fn commit_authorized_generation(
        &self,
        request: AuthorizedAuthoringCommitV1<'_, E>,
    ) -> Result<AuthoringCommitOutcomeV1, AuthoringSessionLoadError>;
}

pub trait AuthoringConversationStorePort<E>:
    AuthoringSessionLoadPort<E> + AuthoringSessionCommitPort<E>
{
}

impl<T, E> AuthoringConversationStorePort<E> for T where
    T: AuthoringSessionLoadPort<E> + AuthoringSessionCommitPort<E>
{
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthoringSessionObservationErrorV1 {
    #[error("authoring session was not found")]
    NotFound,
    #[error("authoring session backend timed out")]
    Timeout,
    #[error("authoring session backend request can be retried")]
    Retryable,
    #[error("authoring session backend is unavailable")]
    Unavailable,
    #[error("authoring session state is invalid")]
    InvalidState,
}

pub struct AuthorizedConversationReadAccessV1<'a, E> {
    actor: &'a AuthenticatedActorV1,
    scope: &'a AuthorizedInstallationScopeV1,
    evidence: &'a E,
    query: &'a ReadAuthoringSessionV1,
}

impl<'a, E> AuthorizedConversationReadAccessV1<'a, E> {
    pub(crate) fn new(
        actor: &'a AuthenticatedActorV1,
        scope: &'a AuthorizedInstallationScopeV1,
        evidence: &'a E,
        query: &'a ReadAuthoringSessionV1,
    ) -> Self {
        Self {
            actor,
            scope,
            evidence,
            query,
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

    pub fn query(&self) -> &ReadAuthoringSessionV1 {
        self.query
    }
}

pub struct AuthoringSessionObservationV1 {
    session_id: AuthoringSessionId,
    generation: SessionGeneration,
    projection: SafeAuthoringTurnProjectionV1,
}

impl AuthoringSessionObservationV1 {
    pub fn from_storage(
        session_id: AuthoringSessionId,
        generation: SessionGeneration,
        projection: SafeAuthoringTurnProjectionV1,
        preview_ready_artifact: Option<&PreviewReadyArtifactV1>,
    ) -> Result<Self, SafeAuthoringProjectionError> {
        projection.validate_artifact_binding(preview_ready_artifact)?;
        Ok(Self {
            session_id,
            generation,
            projection,
        })
    }

    pub fn session_id(&self) -> &AuthoringSessionId {
        &self.session_id
    }

    pub fn generation(&self) -> SessionGeneration {
        self.generation
    }

    pub fn projection(&self) -> &SafeAuthoringTurnProjectionV1 {
        &self.projection
    }
}

impl Debug for AuthoringSessionObservationV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AuthoringSessionObservationV1(<redacted>)")
    }
}

#[allow(async_fn_in_trait)]
pub trait AuthoringSessionReadPort<E> {
    async fn read_authorized_session(
        &self,
        access: &AuthorizedConversationReadAccessV1<'_, E>,
    ) -> Result<AuthoringSessionObservationV1, AuthoringSessionObservationErrorV1>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthoringAdmissionError {
    #[error("authoring turn admission is saturated")]
    Saturated,
    #[error("authoring turn admission is unavailable")]
    Unavailable,
}

#[allow(async_fn_in_trait)]
pub trait AuthoringTurnAdmissionPort {
    type KeyedPermit;
    type ModelPermit;

    async fn acquire_keyed(
        &self,
        key: &LocalAuthoringRequestKeyV1,
    ) -> Result<Self::KeyedPermit, AuthoringAdmissionError>;

    async fn acquire_model_capacity(&self) -> Result<Self::ModelPermit, AuthoringAdmissionError>;
}
