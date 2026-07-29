use authoring_promotion::SessionGeneration;
use design_harness::{
    BurstOutcome, DesignSession, IntentFallbackV1, IntentRecipeStatusV2, LlmClient,
    ResourceBindingFingerprint,
};

use super::{
    AuthoringAdmissionError, AuthoringCommitOutcomeV1, AuthoringConversationConfigV1,
    AuthoringConversationStorePort, AuthoringExpectedGenerationError,
    AuthoringMutationDispositionV1, AuthoringSessionLoadError, AuthoringStoredGenerationV1,
    AuthoringStoredRequestIdentityV1, AuthoringTurnAdmissionPort, AuthoringTurnCheckV1,
    AuthoringTurnOutcomeV1, AuthoringTurnReceiptV1, AuthorizedAuthoringCommitV1,
    AuthorizedConversationAccessV1, LocalAuthoringRequestKeyV1, SafeAuthoringPreviewV1,
    SafeAuthoringProjectionError, SafeAuthoringTurnProjectionV1, SafeAuthoringTurnStateV1,
    StartOrAdvanceAuthoringTurnV1,
};
use crate::authority::validate_authorized_scope;
use crate::{
    AuthenticatedActorV1, AuthenticationError, CapabilityV1, FreshGuildAuthorityError,
    FreshGuildAuthorityEvidence, FreshGuildAuthorityPort, InstallationSelectorV1,
    MutationAuthenticationPort,
};

pub struct ConversationApplication<'a, A, G, S, Q, C> {
    authentication: &'a A,
    guild_authority: &'a G,
    store: &'a S,
    admission: &'a Q,
    client: &'a C,
    config: AuthoringConversationConfigV1,
}

impl<'a, A, G, S, Q, C> ConversationApplication<'a, A, G, S, Q, C> {
    pub fn new(
        authentication: &'a A,
        guild_authority: &'a G,
        store: &'a S,
        admission: &'a Q,
        client: &'a C,
        config: AuthoringConversationConfigV1,
    ) -> Self {
        Self {
            authentication,
            guild_authority,
            store,
            admission,
            client,
            config,
        }
    }
}

impl<A, G, S, Q, C> ConversationApplication<'_, A, G, S, Q, C>
where
    A: MutationAuthenticationPort,
    G: FreshGuildAuthorityPort,
    G::Evidence: FreshGuildAuthorityEvidence,
    S: AuthoringConversationStorePort<G::Evidence>,
    Q: AuthoringTurnAdmissionPort,
    C: LlmClient + Clone,
{
    pub async fn start_or_advance_turn(
        &self,
        credential: &A::Credential,
        csrf: &A::CsrfProof,
        installation: &InstallationSelectorV1,
        command: StartOrAdvanceAuthoringTurnV1,
    ) -> Result<AuthoringTurnOutcomeV1, AuthoringConversationError> {
        let (initial_actor, initial_authorized) = self
            .authenticate_author(credential, csrf, installation)
            .await?;
        let local_key = LocalAuthoringRequestKeyV1::from_authorized_scope(
            initial_actor.principal_id().clone(),
            initial_authorized.scope(),
            &command,
        );
        let _keyed_permit = self.admission.acquire_keyed(&local_key).await?;
        let (keyed_actor, keyed_authorized) = self
            .authenticate_author(credential, csrf, installation)
            .await?;
        validate_request_identity(
            &initial_actor,
            initial_authorized.scope(),
            &keyed_actor,
            keyed_authorized.scope(),
        )?;
        let access = AuthorizedConversationAccessV1::new(
            &keyed_actor,
            keyed_authorized.scope(),
            keyed_authorized.evidence(),
            &command,
        );
        if let Some(receipt) = self.check_before_model(&access).await? {
            return Ok(receipt);
        }
        let model_permit = self.admission.acquire_model_capacity().await?;
        let (model_actor, model_authorized) = self
            .authenticate_author(credential, csrf, installation)
            .await?;
        validate_request_identity(
            &initial_actor,
            initial_authorized.scope(),
            &model_actor,
            model_authorized.scope(),
        )?;
        let access = AuthorizedConversationAccessV1::new(
            &model_actor,
            model_authorized.scope(),
            model_authorized.evidence(),
            &command,
        );
        if let Some(receipt) = self.check_before_model(&access).await? {
            return Ok(receipt);
        }
        let loaded = self.store.load_exact_generation(&access).await?;
        validate_loaded_generation(command.expected_generation().get(), &loaded)?;
        let (snapshot, bindings) = loaded.into_snapshot_and_bindings();
        let session_config = self.config.session_config();
        let mut session = match snapshot {
            Some(snapshot) => DesignSession::restore_intent_recipe(
                self.client.clone(),
                session_config.clone(),
                snapshot,
                bindings.clone(),
            )
            .map_err(|_| AuthoringConversationError::InvalidSession)?,
            None => DesignSession::with_intent_recipe_config(
                self.client.clone(),
                session_config.clone(),
                bindings.clone(),
            ),
        };
        let model_calls_before = session.observability().model_calls;
        let outcome = session.run_burst(command.human_message().as_str()).await;
        drop(model_permit);
        if let BurstOutcome::Halted(report) = &outcome {
            return Err(AuthoringConversationError::TurnHalted {
                code: report.code.clone(),
            });
        }
        let model_calls = session
            .observability()
            .model_calls
            .saturating_sub(model_calls_before);
        if !(1..=2).contains(&model_calls) {
            return Err(AuthoringConversationError::InvalidModelCallCount);
        }
        let snapshot = session.snapshot();
        snapshot
            .validate_durable_size()
            .map_err(|_| AuthoringConversationError::InvalidSession)?;
        let snapshot = serde_json::to_vec(&snapshot)
            .and_then(|bytes| serde_json::from_slice::<design_harness::SessionSnapshot>(&bytes))
            .map_err(|_| AuthoringConversationError::InvalidSession)?;
        let restored = DesignSession::restore_intent_recipe(
            self.client.clone(),
            session_config,
            snapshot.clone(),
            bindings,
        )
        .map_err(|_| AuthoringConversationError::InvalidSession)?;
        let projected = project_turn(&restored, &outcome)?;
        let binding_fingerprint = ResourceBindingFingerprint::parse(
            restored
                .intent_recipe_binding_fingerprint()
                .ok_or(AuthoringConversationError::InvalidSession)?,
        )
        .map_err(|_| AuthoringConversationError::InvalidSession)?;
        let (final_actor, final_authorized) = self
            .authenticate_author(credential, csrf, installation)
            .await?;
        if !same_authority(
            &model_actor,
            model_authorized.scope(),
            model_authorized.evidence(),
            &final_actor,
            final_authorized.scope(),
            final_authorized.evidence(),
        ) {
            return Err(AuthoringConversationError::AuthorityDrift);
        }
        if matches!(
            projected.projection.state(),
            SafeAuthoringTurnStateV1::Unsupported | SafeAuthoringTurnStateV1::Rejected
        ) {
            return Ok(AuthoringTurnOutcomeV1::NotCommitted(projected.projection));
        }
        let final_access = AuthorizedConversationAccessV1::new(
            &final_actor,
            final_authorized.scope(),
            final_authorized.evidence(),
            &command,
        );
        let request_identity = AuthoringStoredRequestIdentityV1::from_access(&final_access);
        let candidate = projected.projection.clone();
        let commit = AuthorizedAuthoringCommitV1::new(
            final_access,
            binding_fingerprint,
            snapshot,
            projected.projection,
            projected.preview_ready_artifact,
        );
        let outcome = self.store.commit_authorized_generation(commit).await?;
        self.finish_commit(&command, &request_identity, candidate, outcome)
            .map(AuthoringTurnOutcomeV1::Committed)
    }

    async fn authenticate_author(
        &self,
        credential: &A::Credential,
        csrf: &A::CsrfProof,
        installation: &InstallationSelectorV1,
    ) -> Result<
        (
            AuthenticatedActorV1,
            crate::AuthorizedInstallationV1<G::Evidence>,
        ),
        AuthoringConversationError,
    > {
        let claims = self
            .authentication
            .authenticate_mutation(credential, csrf)
            .await?;
        let actor = AuthenticatedActorV1::from_authentication_claims(claims);
        let authorized = self
            .guild_authority
            .authorize_installation(&actor, installation, CapabilityV1::Author)
            .await?;
        validate_authorized_scope(installation, authorized.scope())?;
        validate_author_evidence(authorized.scope(), authorized.evidence())?;
        Ok((actor, authorized))
    }

    async fn check_before_model(
        &self,
        access: &AuthorizedConversationAccessV1<'_, G::Evidence>,
    ) -> Result<Option<AuthoringTurnOutcomeV1>, AuthoringConversationError> {
        match self.store.check_replay_or_head(access).await? {
            AuthoringTurnCheckV1::Proceed => Ok(None),
            AuthoringTurnCheckV1::ExactReplay(stored) => {
                if !stored.request_identity().matches_access(access) {
                    return Err(AuthoringConversationError::InvalidCommit);
                }
                self.stored_receipt(
                    access.command(),
                    &AuthoringStoredRequestIdentityV1::from_access(access),
                    AuthoringMutationDispositionV1::ExactReplay,
                    stored,
                )
                .map(AuthoringTurnOutcomeV1::Committed)
                .map(Some)
            }
            AuthoringTurnCheckV1::IdempotencyConflict => {
                Err(AuthoringConversationError::IdempotencyConflict)
            }
            AuthoringTurnCheckV1::GenerationConflict { current_generation } => {
                Err(AuthoringConversationError::GenerationConflict { current_generation })
            }
        }
    }

    fn finish_commit(
        &self,
        command: &StartOrAdvanceAuthoringTurnV1,
        request_identity: &AuthoringStoredRequestIdentityV1,
        candidate: SafeAuthoringTurnProjectionV1,
        outcome: AuthoringCommitOutcomeV1,
    ) -> Result<AuthoringTurnReceiptV1, AuthoringConversationError> {
        match outcome {
            AuthoringCommitOutcomeV1::Created(stored) => {
                if stored.projection() != &candidate {
                    return Err(AuthoringConversationError::InvalidCommit);
                }
                self.stored_receipt(
                    command,
                    request_identity,
                    AuthoringMutationDispositionV1::Created,
                    stored,
                )
            }
            AuthoringCommitOutcomeV1::ExactReplay(stored) => self.stored_receipt(
                command,
                request_identity,
                AuthoringMutationDispositionV1::ExactReplay,
                stored,
            ),
            AuthoringCommitOutcomeV1::IdempotencyConflict => {
                Err(AuthoringConversationError::IdempotencyConflict)
            }
            AuthoringCommitOutcomeV1::GenerationConflict { current_generation } => {
                Err(AuthoringConversationError::GenerationConflict { current_generation })
            }
            AuthoringCommitOutcomeV1::AuthorityConflict => {
                Err(AuthoringConversationError::AuthorityDrift)
            }
            AuthoringCommitOutcomeV1::BindingConflict => {
                Err(AuthoringConversationError::BindingDrift)
            }
        }
    }

    fn stored_receipt(
        &self,
        command: &StartOrAdvanceAuthoringTurnV1,
        request_identity: &AuthoringStoredRequestIdentityV1,
        disposition: AuthoringMutationDispositionV1,
        stored: AuthoringStoredGenerationV1,
    ) -> Result<AuthoringTurnReceiptV1, AuthoringConversationError> {
        let expected_generation = command.expected_generation().successor()?;
        if stored.request_identity() != request_identity
            || stored.generation() != expected_generation
        {
            return Err(AuthoringConversationError::InvalidCommit);
        }
        AuthoringTurnReceiptV1::from_result(
            command.session_id().clone(),
            stored.generation(),
            disposition,
            stored.into_projection(),
        )
        .map_err(AuthoringConversationError::from)
    }
}

fn validate_loaded_generation(
    expected_generation: u64,
    loaded: &super::AuthoringSessionLoadV1,
) -> Result<(), AuthoringConversationError> {
    let actual = loaded
        .head_generation()
        .map(SessionGeneration::get)
        .unwrap_or(0);
    if actual != expected_generation || (expected_generation == 0) != loaded.snapshot().is_none() {
        return Err(AuthoringConversationError::InvalidSession);
    }
    Ok(())
}

fn validate_author_evidence<E: FreshGuildAuthorityEvidence>(
    scope: &crate::AuthorizedInstallationScopeV1,
    evidence: &E,
) -> Result<(), AuthoringConversationError> {
    if evidence.capability() != CapabilityV1::Author
        || evidence.tenant_id() != scope.tenant_id()
        || evidence.installation_id() != scope.installation_id()
        || evidence.guild_id() != scope.guild_id()
        || evidence.acting_user_id() != scope.acting_user_id()
    {
        return Err(AuthoringConversationError::AuthorityDrift);
    }
    Ok(())
}

fn validate_request_identity(
    actor: &AuthenticatedActorV1,
    scope: &crate::AuthorizedInstallationScopeV1,
    refreshed_actor: &AuthenticatedActorV1,
    refreshed_scope: &crate::AuthorizedInstallationScopeV1,
) -> Result<(), AuthoringConversationError> {
    if actor != refreshed_actor || scope != refreshed_scope {
        return Err(AuthoringConversationError::AuthorityDrift);
    }
    Ok(())
}

fn same_authority<E: FreshGuildAuthorityEvidence>(
    actor: &AuthenticatedActorV1,
    scope: &crate::AuthorizedInstallationScopeV1,
    evidence: &E,
    final_actor: &AuthenticatedActorV1,
    final_scope: &crate::AuthorizedInstallationScopeV1,
    final_evidence: &E,
) -> bool {
    actor == final_actor
        && scope == final_scope
        && evidence.capability() == final_evidence.capability()
        && evidence.discord_application_id() == final_evidence.discord_application_id()
        && evidence.guild_owner() == final_evidence.guild_owner()
        && evidence.effective_permissions_bits() == final_evidence.effective_permissions_bits()
        && evidence.installation_authority_revision()
            == final_evidence.installation_authority_revision()
        && evidence.installation_authority_digest()
            == final_evidence.installation_authority_digest()
}

struct ProjectedTurnV1 {
    projection: SafeAuthoringTurnProjectionV1,
    preview_ready_artifact: Option<design_harness::PreviewReadyArtifactV1>,
}

fn project_turn<C>(
    session: &DesignSession<C>,
    outcome: &BurstOutcome,
) -> Result<ProjectedTurnV1, AuthoringConversationError> {
    let current_artifact = session.export_preview_ready_artifact().ok();
    let draft = current_artifact
        .as_ref()
        .map(|artifact| artifact.preview().draft.clone())
        .unwrap_or_else(|| session.draft().summary());
    let (state, assistant_message, capabilities, preview, preview_ready_artifact) = match outcome {
        BurstOutcome::NeedsInput { question } => {
            let status_matches = matches!(
                session.intent_recipe_status(),
                Some(IntentRecipeStatusV2::AwaitingDecision {
                    question: ref stored,
                    ..
                }) if stored == question
            );
            if !status_matches {
                return Err(AuthoringConversationError::InvalidSession);
            }
            (
                SafeAuthoringTurnStateV1::NeedsInput,
                question.clone(),
                Vec::new(),
                None,
                None,
            )
        }
        BurstOutcome::Ready { summary } => {
            if !matches!(
                session.intent_recipe_status(),
                Some(IntentRecipeStatusV2::PreviewReady { .. })
            ) {
                return Err(AuthoringConversationError::InvalidSession);
            }
            let artifact = current_artifact.ok_or(AuthoringConversationError::InvalidSession)?;
            (
                SafeAuthoringTurnStateV1::PreviewReady,
                summary.clone(),
                Vec::new(),
                Some(SafeAuthoringPreviewV1::from_artifact(&artifact)),
                Some(artifact),
            )
        }
        BurstOutcome::Routed { fallback, .. } => match fallback {
            IntentFallbackV1::TypedPlanner { response, .. } => (
                SafeAuthoringTurnStateV1::Unsupported,
                response.clone(),
                Vec::new(),
                None,
                None,
            ),
            IntentFallbackV1::CapabilityGap {
                capabilities,
                response,
            } => (
                SafeAuthoringTurnStateV1::CapabilityGap,
                response.clone(),
                capabilities.clone(),
                None,
                None,
            ),
            IntentFallbackV1::Reject { response, .. } => (
                SafeAuthoringTurnStateV1::Rejected,
                response.clone(),
                Vec::new(),
                None,
                None,
            ),
            IntentFallbackV1::Discussion { response } => (
                SafeAuthoringTurnStateV1::Discussion,
                response.clone(),
                Vec::new(),
                None,
                None,
            ),
        },
        BurstOutcome::Progressed { .. } | BurstOutcome::Halted(_) => {
            return Err(AuthoringConversationError::InvalidSession);
        }
    };
    let projection = SafeAuthoringTurnProjectionV1::from_turn(
        state,
        assistant_message,
        capabilities,
        draft,
        preview,
    )?;
    let canonical = projection.to_canonical_json()?;
    let projection = SafeAuthoringTurnProjectionV1::from_canonical_json(&canonical)?;
    projection.validate_artifact_binding(preview_ready_artifact.as_ref())?;
    Ok(ProjectedTurnV1 {
        projection,
        preview_ready_artifact,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthoringConversationError {
    #[error(transparent)]
    Authentication(#[from] AuthenticationError),
    #[error(transparent)]
    Authority(#[from] FreshGuildAuthorityError),
    #[error(transparent)]
    Admission(#[from] AuthoringAdmissionError),
    #[error(transparent)]
    Store(#[from] AuthoringSessionLoadError),
    #[error(transparent)]
    Projection(#[from] SafeAuthoringProjectionError),
    #[error(transparent)]
    ExpectedGeneration(#[from] AuthoringExpectedGenerationError),
    #[error("authoring request idempotency key was reused with a different request")]
    IdempotencyConflict,
    #[error("authoring session generation changed")]
    GenerationConflict {
        current_generation: Option<SessionGeneration>,
    },
    #[error("authoring authority changed while the turn was executing")]
    AuthorityDrift,
    #[error("authoring resource bindings changed while the turn was executing")]
    BindingDrift,
    #[error("authoring session is structurally invalid")]
    InvalidSession,
    #[error("authoring model call count violated the V4 contract")]
    InvalidModelCallCount,
    #[error("authoring turn halted before producing a durable state")]
    TurnHalted { code: String },
    #[error("authoring commit result is invalid")]
    InvalidCommit,
}
