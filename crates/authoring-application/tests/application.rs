use std::collections::VecDeque;
use std::num::{NonZeroU32, NonZeroU64};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use authoring_application::{
    AuthoringApplication, OwnedPreviewReadyArtifactV1, OwnedSessionArtifactPort,
    OwnedSessionLoadError, PromoteOwnedSessionV1, PromotionAuthorityError, PromotionAuthorityPort,
    PromotionSubmissionPort, ResolvedPromotionAuthorityV1, VerifiedPrincipalV1,
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

struct OwnedSession {
    artifact: PreviewReadyArtifactV1,
    calls: AtomicUsize,
}

impl OwnedSessionArtifactPort for OwnedSession {
    async fn load_owned_preview_ready(
        &self,
        principal: &VerifiedPrincipalV1,
        session_id: &AuthoringSessionId,
        expected_generation: SessionGeneration,
    ) -> Result<OwnedPreviewReadyArtifactV1, OwnedSessionLoadError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(principal.tenant_id().as_str(), "tenant-1");
        assert_eq!(principal.principal_id().as_str(), "principal-1");
        assert_eq!(session_id.as_str(), "session-1");
        assert_eq!(expected_generation.get(), 7);
        Ok(OwnedPreviewReadyArtifactV1::from_owned_session(
            self.artifact.clone(),
        ))
    }
}

struct Authority {
    calls: AtomicUsize,
}

impl PromotionAuthorityPort for Authority {
    async fn resolve_promotion_authority(
        &self,
        principal: &VerifiedPrincipalV1,
        session_id: &AuthoringSessionId,
        expected_generation: SessionGeneration,
    ) -> Result<ResolvedPromotionAuthorityV1, PromotionAuthorityError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(principal.principal_id().as_str(), "principal-1");
        assert_eq!(session_id.as_str(), "session-1");
        assert_eq!(expected_generation.get(), 7);
        Ok(ResolvedPromotionAuthorityV1 {
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
        })
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

fn principal() -> VerifiedPrincipalV1 {
    VerifiedPrincipalV1::from_trusted_edge(
        TenantId::parse("tenant-1").unwrap(),
        PrincipalId::parse("principal-1").unwrap(),
    )
}

fn command() -> PromoteOwnedSessionV1 {
    PromoteOwnedSessionV1 {
        idempotency_key: IdempotencyKey::parse("promotion-1").unwrap(),
        session_id: AuthoringSessionId::parse("session-1").unwrap(),
        expected_generation: SessionGeneration::new(7).unwrap(),
    }
}

#[test]
fn trusted_identity_and_server_authority_build_the_only_promotion_context() {
    block_on(async {
        let sessions = OwnedSession {
            artifact: artifact().await,
            calls: AtomicUsize::new(0),
        };
        let authority = Authority {
            calls: AtomicUsize::new(0),
        };
        let promotions = PromotionCapture::default();
        let application = AuthoringApplication::new(&sessions, &authority, &promotions);
        application
            .promote_owned_session(&principal(), command())
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
        assert_eq!(sessions.calls.load(Ordering::SeqCst), 1);
        assert_eq!(authority.calls.load(Ordering::SeqCst), 1);
    });
}

struct DeniedSession;

impl OwnedSessionArtifactPort for DeniedSession {
    async fn load_owned_preview_ready(
        &self,
        _principal: &VerifiedPrincipalV1,
        _session_id: &AuthoringSessionId,
        _expected_generation: SessionGeneration,
    ) -> Result<OwnedPreviewReadyArtifactV1, OwnedSessionLoadError> {
        Err(OwnedSessionLoadError::NotOwned)
    }
}

#[test]
fn failed_session_ownership_stops_before_authority_and_promotion() {
    block_on(async {
        let authority = Authority {
            calls: AtomicUsize::new(0),
        };
        let promotions = PromotionCapture::default();
        let application = AuthoringApplication::new(&DeniedSession, &authority, &promotions);
        assert!(matches!(
            application
                .promote_owned_session(&principal(), command())
                .await,
            Err(authoring_application::AuthoringApplicationError::Session(
                OwnedSessionLoadError::NotOwned
            ))
        ));
        assert_eq!(authority.calls.load(Ordering::SeqCst), 0);
        assert!(promotions.input.lock().unwrap().is_none());
    });
}

struct DeniedAuthority;

impl PromotionAuthorityPort for DeniedAuthority {
    async fn resolve_promotion_authority(
        &self,
        _principal: &VerifiedPrincipalV1,
        _session_id: &AuthoringSessionId,
        _expected_generation: SessionGeneration,
    ) -> Result<ResolvedPromotionAuthorityV1, PromotionAuthorityError> {
        Err(PromotionAuthorityError::Forbidden)
    }
}

#[test]
fn failed_server_authority_stops_before_promotion() {
    block_on(async {
        let sessions = OwnedSession {
            artifact: artifact().await,
            calls: AtomicUsize::new(0),
        };
        let promotions = PromotionCapture::default();
        let application = AuthoringApplication::new(&sessions, &DeniedAuthority, &promotions);
        assert!(matches!(
            application
                .promote_owned_session(&principal(), command())
                .await,
            Err(authoring_application::AuthoringApplicationError::Authority(
                PromotionAuthorityError::Forbidden
            ))
        ));
        assert_eq!(sessions.calls.load(Ordering::SeqCst), 1);
        assert!(promotions.input.lock().unwrap().is_none());
    });
}

struct StaleAuthority;

impl PromotionAuthorityPort for StaleAuthority {
    async fn resolve_promotion_authority(
        &self,
        _principal: &VerifiedPrincipalV1,
        _session_id: &AuthoringSessionId,
        _expected_generation: SessionGeneration,
    ) -> Result<ResolvedPromotionAuthorityV1, PromotionAuthorityError> {
        Err(PromotionAuthorityError::GenerationMismatch)
    }
}

#[test]
fn authority_generation_mismatch_stops_before_promotion() {
    block_on(async {
        let sessions = OwnedSession {
            artifact: artifact().await,
            calls: AtomicUsize::new(0),
        };
        let promotions = PromotionCapture::default();
        let application = AuthoringApplication::new(&sessions, &StaleAuthority, &promotions);
        assert!(matches!(
            application
                .promote_owned_session(&principal(), command())
                .await,
            Err(authoring_application::AuthoringApplicationError::Authority(
                PromotionAuthorityError::GenerationMismatch
            ))
        ));
        assert_eq!(sessions.calls.load(Ordering::SeqCst), 1);
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
fn promotion_failure_is_propagated_after_both_authorization_checks() {
    block_on(async {
        let sessions = OwnedSession {
            artifact: artifact().await,
            calls: AtomicUsize::new(0),
        };
        let authority = Authority {
            calls: AtomicUsize::new(0),
        };
        let promotions = FailedPromotion {
            calls: AtomicUsize::new(0),
        };
        let application = AuthoringApplication::new(&sessions, &authority, &promotions);
        assert!(matches!(
            application
                .promote_owned_session(&principal(), command())
                .await,
            Err(authoring_application::AuthoringApplicationError::Promotion(
                PromotionError::ConcurrentTransitionLimit
            ))
        ));
        assert_eq!(sessions.calls.load(Ordering::SeqCst), 1);
        assert_eq!(authority.calls.load(Ordering::SeqCst), 1);
        assert_eq!(promotions.calls.load(Ordering::SeqCst), 1);
    });
}
