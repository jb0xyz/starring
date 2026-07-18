use std::collections::VecDeque;
use std::num::{NonZeroU32, NonZeroU64};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use authoring_application::{
    AuthenticatedActorV1, AuthenticatedIdentityV1, AuthenticationError, AuthenticationPort,
    AuthoringApplication, AuthorizedPromotionSnapshotError, AuthorizedPromotionSnapshotPort,
    AuthorizedPromotionSnapshotV1, OwnedSessionLoadError, PromoteOwnedSessionV1,
    PromotionAuthorityError, PromotionSubmissionPort, ResolvedPromotionAuthorityV1,
};
use authoring_promotion::{
    ApprovalPolicyV1, AuthoringSessionId, AutomationInstallationId, BindingRevision,
    IdempotencyKey, PolicyRevision, PrincipalId, PromotionError, SessionGeneration,
    StartPromotionV1, TenantId,
};
use design_harness::{
    BurstOutcome, DesignSession, LlmClient, LlmError, LlmResponse, Message, PreviewReadyArtifactV1,
    ResourceBindingMap, ToolCall, ToolDefinition,
};
use discord_model::{GuildId, UserId};
use futures::executor::block_on;
use serde_json::json;

#[derive(Clone)]
struct ScriptedClient {
    responses: Arc<Mutex<VecDeque<Result<LlmResponse, LlmError>>>>,
}

impl ScriptedClient {
    fn validated_preview() -> Self {
        let response = LlmResponse::ToolCalls(vec![ToolCall {
            id: "interpret".to_string(),
            name: "interpret_intent_core".to_string(),
            arguments: json!({
                "expected_revision": 0,
                "request_mode": "build",
                "automation_kind": "managed_private_study_room",
                "requested_outcome": "validated_preview",
                "hub_channel": "community_hub",
                "language": "en",
                "close_policy": "disabled",
                "other_unmapped_required_capabilities": [],
                "response": ""
            })
            .to_string(),
        }]);
        Self {
            responses: Arc::new(Mutex::new(vec![Ok(response)].into())),
        }
    }
}

impl LlmClient for ScriptedClient {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
    ) -> Result<LlmResponse, LlmError> {
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("scripted response")
    }
}

async fn artifact() -> PreviewReadyArtifactV1 {
    let mut bindings = ResourceBindingMap::default();
    bindings.channel_bindings.insert(
        serde_json::from_value(json!("community_hub")).unwrap(),
        "700".parse().unwrap(),
    );
    let mut session =
        DesignSession::with_intent_recipe(ScriptedClient::validated_preview(), bindings);
    assert!(matches!(
        session
            .run_burst(
                "Create private study rooms in community_hub and prepare a validated preview"
            )
            .await,
        BurstOutcome::Ready { .. }
    ));
    session.export_preview_ready_artifact().unwrap()
}

struct Authentication {
    calls: AtomicUsize,
}

impl AuthenticationPort for Authentication {
    type Credential = str;

    async fn authenticate(
        &self,
        credential: &Self::Credential,
    ) -> Result<AuthenticatedIdentityV1, AuthenticationError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(credential, "opaque-session-token");
        Ok(AuthenticatedIdentityV1::from_authentication(
            TenantId::parse("tenant-1").unwrap(),
            PrincipalId::parse("principal-1").unwrap(),
        ))
    }
}

struct AuthorizedSnapshot {
    artifact: PreviewReadyArtifactV1,
    calls: AtomicUsize,
}

impl AuthorizedPromotionSnapshotPort for AuthorizedSnapshot {
    async fn load_atomic_authorized_snapshot(
        &self,
        actor: &AuthenticatedActorV1,
        session_id: &AuthoringSessionId,
        expected_generation: SessionGeneration,
    ) -> Result<AuthorizedPromotionSnapshotV1, AuthorizedPromotionSnapshotError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(actor.tenant_id().as_str(), "tenant-1");
        assert_eq!(actor.principal_id().as_str(), "principal-1");
        assert_eq!(session_id.as_str(), "session-1");
        assert_eq!(expected_generation.get(), 7);
        Ok(AuthorizedPromotionSnapshotV1::from_atomic_authorization(
            self.artifact.clone(),
            authority(),
        ))
    }
}

fn authority() -> ResolvedPromotionAuthorityV1 {
    ResolvedPromotionAuthorityV1 {
        guild_id: GuildId(900),
        installation_id: AutomationInstallationId::parse("installation-1").unwrap(),
        ruleset_key: "studyrooms".parse().unwrap(),
        requester: UserId(100),
        binding_revision: BindingRevision::new(3).unwrap(),
        policy: ApprovalPolicyV1 {
            revision: PolicyRevision::new(5).unwrap(),
            required_approvals: NonZeroU32::new(2).unwrap(),
            ttl_seconds: NonZeroU64::new(3600).unwrap(),
        },
    }
}

#[derive(Default)]
struct PromotionCapture {
    input: Mutex<Option<StartPromotionV1>>,
}

impl PromotionSubmissionPort for PromotionCapture {
    type Output = ();

    async fn submit_verified_promotion(
        &self,
        input: StartPromotionV1,
    ) -> Result<Self::Output, PromotionError> {
        *self.input.lock().unwrap() = Some(input);
        Ok(())
    }
}

fn command() -> PromoteOwnedSessionV1 {
    PromoteOwnedSessionV1 {
        idempotency_key: IdempotencyKey::parse("promotion-1").unwrap(),
        session_id: AuthoringSessionId::parse("session-1").unwrap(),
        expected_generation: SessionGeneration::new(7).unwrap(),
    }
}

#[test]
fn authenticated_identity_and_atomic_authority_build_the_only_promotion_context() {
    block_on(async {
        let authentication = Authentication {
            calls: AtomicUsize::new(0),
        };
        let snapshots = AuthorizedSnapshot {
            artifact: artifact().await,
            calls: AtomicUsize::new(0),
        };
        let promotions = PromotionCapture::default();
        let application = AuthoringApplication::new(&authentication, &snapshots, &promotions);
        application
            .promote_owned_session("opaque-session-token", command())
            .await
            .unwrap();
        let captured = promotions.input.lock().unwrap().take().unwrap();
        assert_eq!(captured.context.tenant_id.as_str(), "tenant-1");
        assert_eq!(captured.context.principal_id.as_str(), "principal-1");
        assert_eq!(captured.context.session_owner_id.as_str(), "principal-1");
        assert_eq!(captured.context.session_id.as_str(), "session-1");
        assert_eq!(captured.context.session_generation.get(), 7);
        assert_eq!(captured.context.guild_id, GuildId(900));
        assert_eq!(captured.context.installation_id.as_str(), "installation-1");
        assert_eq!(captured.context.ruleset_key.as_str(), "studyrooms");
        assert_eq!(captured.context.requester, UserId(100));
        assert_eq!(captured.context.binding_revision.get(), 3);
        assert_eq!(captured.context.policy.revision.get(), 5);
        assert_eq!(captured.context.policy.required_approvals.get(), 2);
        assert_eq!(authentication.calls.load(Ordering::SeqCst), 1);
        assert_eq!(snapshots.calls.load(Ordering::SeqCst), 1);
    });
}

struct DeniedAuthentication;

impl AuthenticationPort for DeniedAuthentication {
    type Credential = str;

    async fn authenticate(
        &self,
        _credential: &Self::Credential,
    ) -> Result<AuthenticatedIdentityV1, AuthenticationError> {
        Err(AuthenticationError::Revoked)
    }
}

#[test]
fn failed_authentication_stops_before_snapshot_and_promotion() {
    block_on(async {
        let snapshots = AuthorizedSnapshot {
            artifact: artifact().await,
            calls: AtomicUsize::new(0),
        };
        let promotions = PromotionCapture::default();
        let application = AuthoringApplication::new(&DeniedAuthentication, &snapshots, &promotions);
        assert!(matches!(
            application
                .promote_owned_session("revoked-session-token", command())
                .await,
            Err(
                authoring_application::AuthoringApplicationError::Authentication(
                    AuthenticationError::Revoked
                )
            )
        ));
        assert_eq!(snapshots.calls.load(Ordering::SeqCst), 0);
        assert!(promotions.input.lock().unwrap().is_none());
    });
}

struct DeniedSession;

impl AuthorizedPromotionSnapshotPort for DeniedSession {
    async fn load_atomic_authorized_snapshot(
        &self,
        _actor: &AuthenticatedActorV1,
        _session_id: &AuthoringSessionId,
        _expected_generation: SessionGeneration,
    ) -> Result<AuthorizedPromotionSnapshotV1, AuthorizedPromotionSnapshotError> {
        Err(OwnedSessionLoadError::NotOwned.into())
    }
}

#[test]
fn failed_session_ownership_stops_before_promotion() {
    block_on(async {
        let authentication = Authentication {
            calls: AtomicUsize::new(0),
        };
        let promotions = PromotionCapture::default();
        let application = AuthoringApplication::new(&authentication, &DeniedSession, &promotions);
        assert!(matches!(
            application
                .promote_owned_session("opaque-session-token", command())
                .await,
            Err(authoring_application::AuthoringApplicationError::Session(
                OwnedSessionLoadError::NotOwned
            ))
        ));
        assert_eq!(authentication.calls.load(Ordering::SeqCst), 1);
        assert!(promotions.input.lock().unwrap().is_none());
    });
}

struct DeniedAuthority;

impl AuthorizedPromotionSnapshotPort for DeniedAuthority {
    async fn load_atomic_authorized_snapshot(
        &self,
        _actor: &AuthenticatedActorV1,
        _session_id: &AuthoringSessionId,
        _expected_generation: SessionGeneration,
    ) -> Result<AuthorizedPromotionSnapshotV1, AuthorizedPromotionSnapshotError> {
        Err(PromotionAuthorityError::Forbidden.into())
    }
}

#[test]
fn failed_server_authority_stops_before_promotion() {
    block_on(async {
        let authentication = Authentication {
            calls: AtomicUsize::new(0),
        };
        let promotions = PromotionCapture::default();
        let application = AuthoringApplication::new(&authentication, &DeniedAuthority, &promotions);
        assert!(matches!(
            application
                .promote_owned_session("opaque-session-token", command())
                .await,
            Err(authoring_application::AuthoringApplicationError::Authority(
                PromotionAuthorityError::Forbidden
            ))
        ));
        assert_eq!(authentication.calls.load(Ordering::SeqCst), 1);
        assert!(promotions.input.lock().unwrap().is_none());
    });
}

struct StaleAuthority;

impl AuthorizedPromotionSnapshotPort for StaleAuthority {
    async fn load_atomic_authorized_snapshot(
        &self,
        _actor: &AuthenticatedActorV1,
        _session_id: &AuthoringSessionId,
        _expected_generation: SessionGeneration,
    ) -> Result<AuthorizedPromotionSnapshotV1, AuthorizedPromotionSnapshotError> {
        Err(PromotionAuthorityError::GenerationMismatch.into())
    }
}

#[test]
fn authority_generation_mismatch_stops_before_promotion() {
    block_on(async {
        let authentication = Authentication {
            calls: AtomicUsize::new(0),
        };
        let promotions = PromotionCapture::default();
        let application = AuthoringApplication::new(&authentication, &StaleAuthority, &promotions);
        assert!(matches!(
            application
                .promote_owned_session("opaque-session-token", command())
                .await,
            Err(authoring_application::AuthoringApplicationError::Authority(
                PromotionAuthorityError::GenerationMismatch
            ))
        ));
        assert_eq!(authentication.calls.load(Ordering::SeqCst), 1);
        assert!(promotions.input.lock().unwrap().is_none());
    });
}

struct FailedPromotion {
    calls: AtomicUsize,
}

impl PromotionSubmissionPort for FailedPromotion {
    type Output = ();

    async fn submit_verified_promotion(
        &self,
        _input: StartPromotionV1,
    ) -> Result<Self::Output, PromotionError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(PromotionError::ConcurrentTransitionLimit)
    }
}

#[test]
fn promotion_failure_is_propagated_after_one_atomic_authorization_check() {
    block_on(async {
        let authentication = Authentication {
            calls: AtomicUsize::new(0),
        };
        let snapshots = AuthorizedSnapshot {
            artifact: artifact().await,
            calls: AtomicUsize::new(0),
        };
        let promotions = FailedPromotion {
            calls: AtomicUsize::new(0),
        };
        let application = AuthoringApplication::new(&authentication, &snapshots, &promotions);
        assert!(matches!(
            application
                .promote_owned_session("opaque-session-token", command())
                .await,
            Err(authoring_application::AuthoringApplicationError::Promotion(
                PromotionError::ConcurrentTransitionLimit
            ))
        ));
        assert_eq!(authentication.calls.load(Ordering::SeqCst), 1);
        assert_eq!(snapshots.calls.load(Ordering::SeqCst), 1);
        assert_eq!(promotions.calls.load(Ordering::SeqCst), 1);
    });
}
