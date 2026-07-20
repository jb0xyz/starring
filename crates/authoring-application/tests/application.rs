use std::collections::VecDeque;
use std::num::{NonZeroU32, NonZeroU64};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use authoring_application::{
    ApplyProductPromotionV1, ApprovalPayloadDigestV1, ApproveProductPromotionV1,
    AuthenticatedActorV1, AuthenticatedSessionFingerprintV1, AuthenticationClaimsV1,
    AuthenticationError, AuthenticationPort, AuthoringApplication, AuthoringApplicationError,
    AuthorizedApplyProductV1, AuthorizedApprovalPreviewV1, AuthorizedApproveProductV1,
    AuthorizedDeploymentStatusV1, AuthorizedInstallationScopeV1, AuthorizedInstallationV1,
    AuthorizedProductStatusV1, AuthorizedPromotionAccessV1, AuthorizedPromotionSnapshotError,
    AuthorizedPromotionSnapshotPort, AuthorizedPromotionSnapshotV1,
    AuthorizedPromotionSubmissionErrorV1, AuthorizedPromotionSubmissionPort,
    AuthorizedPromotionSubmissionV1, AuthorizedRejectProductV1, CapabilityV1,
    DeploymentFailureCodeV1, DeploymentStatusObservationPort, DeploymentStatusObservationV1,
    DeploymentStatusPort, DeploymentStatusPortError, DeploymentStatusProjectionV1,
    ExactDeploymentSelectorV1, ExactLiveProjectionV1, FreshGuildAuthorityError,
    FreshGuildAuthorityPort, InstallationSelectorV1, MutationAuthenticationPort,
    ProductApplicationError, ProductApplyPort, ProductApprovalPort,
    ProductApprovalPreviewObservationV1, ProductApprovalPreviewV1, ProductCandidateErrorCodeV1,
    ProductControlApplication, ProductControlPortError, ProductDecisionObservationPort,
    ProductDecisionObservationV1, ProductDecisionPhaseV1, ProductDecisionPort,
    ProductDecisionProjectionV1, ProductDecisionQueryPort, ProductIdempotencyKeyV1,
    ProductMutationReceiptV1, ProductPromotionIdempotencyKeyError,
    ProductPromotionIdempotencyKeyV1, ProductPromotionStateV1, ProductRejectionPort,
    ProductRequestIdError, ProductRequestIdV1, ProductRevisionV1, ProductStatusQueryV1,
    ProductStatusV1, PromoteOwnedSessionV1, PromotionSubmissionDispositionV1,
    PromotionSubmissionPort, PromotionSubmissionV1, RejectProductPromotionV1, RejectionReasonError,
    RejectionReasonV1, ResolvedPromotionAuthorityV1, RuntimeDeploymentQueryV1,
};
use authoring_promotion::{
    approval_payload_digest_v1, ApprovalPolicyV1, AuthenticatedPromotionContext,
    AuthoringSessionId, AutomationInstallationId, BindingRevision, EnsurePendingActivationV1,
    IdempotencyKey, InMemoryPromotionStore, PendingActivationDispositionV1, PendingActivationPort,
    PendingActivationPortError, PendingActivationReceiptV1, PolicyRevision, PrincipalId,
    PromotionId, PromotionService, ResolveProductApprovalContextV1,
    ResolvedProductApprovalContextV1, ResumePromotionOutcomeV1, SessionGeneration,
    StartPromotionV1, TenantId, UtcPromotionClock,
};
use automation_ruleset::InMemoryRuleSetStore;
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct Evidence(&'static str);

struct Authentication {
    events: Arc<Mutex<Vec<&'static str>>>,
    failure: Option<AuthenticationError>,
}

impl AuthenticationPort for Authentication {
    type Credential = str;

    async fn authenticate(
        &self,
        credential: &Self::Credential,
    ) -> Result<AuthenticationClaimsV1, AuthenticationError> {
        self.events.lock().unwrap().push("authenticate");
        assert_eq!(credential, "opaque-session-token");
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        Ok(AuthenticationClaimsV1::from_authentication(
            PrincipalId::parse("principal-1").unwrap(),
            AuthenticatedSessionFingerprintV1::from_sha256_digest([7_u8; 32]),
        ))
    }
}

impl MutationAuthenticationPort for Authentication {
    type CsrfProof = str;

    async fn authenticate_mutation(
        &self,
        credential: &Self::Credential,
        csrf: &Self::CsrfProof,
    ) -> Result<AuthenticationClaimsV1, AuthenticationError> {
        self.events.lock().unwrap().push("authenticate_mutation");
        assert_eq!(credential, "opaque-session-token");
        if csrf != "csrf-proof" {
            return Err(AuthenticationError::InvalidCsrf);
        }
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        Ok(AuthenticationClaimsV1::from_authentication(
            PrincipalId::parse("principal-1").unwrap(),
            AuthenticatedSessionFingerprintV1::from_sha256_digest([7_u8; 32]),
        ))
    }
}

struct GuildAuthority {
    events: Arc<Mutex<Vec<&'static str>>>,
    failure: Option<FreshGuildAuthorityError>,
}

impl FreshGuildAuthorityPort for GuildAuthority {
    type Evidence = Evidence;

    async fn authorize_installation(
        &self,
        actor: &AuthenticatedActorV1,
        installation: &InstallationSelectorV1,
        capability: CapabilityV1,
    ) -> Result<AuthorizedInstallationV1<Self::Evidence>, FreshGuildAuthorityError> {
        self.events.lock().unwrap().push("authorize");
        assert_eq!(actor.principal_id().as_str(), "principal-1");
        assert_eq!(actor.session_fingerprint().as_bytes(), &[7_u8; 32]);
        assert_eq!(installation.installation_id().as_str(), "installation-2");
        assert!(matches!(
            capability,
            CapabilityV1::Promote
                | CapabilityV1::Read
                | CapabilityV1::Approve
                | CapabilityV1::Reject
                | CapabilityV1::Apply
        ));
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        Ok(AuthorizedInstallationV1::from_fresh_authority(
            AuthorizedInstallationScopeV1::from_fresh_authority(
                TenantId::parse("tenant-1").unwrap(),
                AutomationInstallationId::parse("installation-2").unwrap(),
                GuildId(900),
                UserId(200),
            ),
            Evidence("fresh-authority-evidence"),
        ))
    }
}

struct AuthorizedSnapshot {
    artifact: PreviewReadyArtifactV1,
    events: Arc<Mutex<Vec<&'static str>>>,
}

impl AuthorizedPromotionSnapshotPort<Evidence> for AuthorizedSnapshot {
    async fn load_atomic_authorized_snapshot(
        &self,
        actor: &AuthenticatedActorV1,
        scope: &AuthorizedInstallationScopeV1,
        evidence: &Evidence,
        session_id: &AuthoringSessionId,
        expected_generation: SessionGeneration,
    ) -> Result<AuthorizedPromotionSnapshotV1, AuthorizedPromotionSnapshotError> {
        self.events.lock().unwrap().push("atomic_snapshot");
        assert_eq!(actor.principal_id().as_str(), "principal-1");
        assert_eq!(scope.tenant_id().as_str(), "tenant-1");
        assert_eq!(scope.installation_id().as_str(), "installation-2");
        assert_eq!(scope.guild_id(), GuildId(900));
        assert_eq!(scope.acting_user_id(), UserId(200));
        assert_eq!(evidence, &Evidence("fresh-authority-evidence"));
        assert_eq!(session_id.as_str(), "session-1");
        assert_eq!(expected_generation.get(), 7);
        Ok(AuthorizedPromotionSnapshotV1::from_atomic_authorization(
            self.artifact.clone(),
            ResolvedPromotionAuthorityV1 {
                guild_id: GuildId(900),
                installation_id: AutomationInstallationId::parse("installation-2").unwrap(),
                ruleset_key: "studyrooms".parse().unwrap(),
                requester: UserId(200),
                binding_revision: BindingRevision::new(3).unwrap(),
                policy: ApprovalPolicyV1 {
                    revision: PolicyRevision::new(5).unwrap(),
                    required_approvals: NonZeroU32::new(2).unwrap(),
                    ttl_seconds: NonZeroU64::new(3600).unwrap(),
                },
            },
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CapturedPromotion {
    request_id: String,
    principal_id: String,
    session_fingerprint: [u8; 32],
    tenant_id: String,
    installation_id: String,
    guild_id: GuildId,
    acting_user_id: UserId,
    evidence: Evidence,
    context: AuthenticatedPromotionContext,
    debug: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CapturedPromotionAccess {
    request_id: String,
    principal_id: String,
    session_fingerprint: [u8; 32],
    tenant_id: String,
    installation_id: String,
    guild_id: GuildId,
    acting_user_id: UserId,
    evidence: Evidence,
    session_id: String,
    expected_generation: u64,
    idempotency_key: Vec<u8>,
    debug: String,
}

struct PromotionCapture {
    events: Arc<Mutex<Vec<&'static str>>>,
    access: Mutex<Option<CapturedPromotionAccess>>,
    captured: Mutex<Option<CapturedPromotion>>,
    failure: Option<AuthorizedPromotionSubmissionErrorV1>,
    fault: PromotionCaptureFault,
    replay_artifact: Option<PreviewReadyArtifactV1>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PromotionCaptureFault {
    None,
    SubmitAuthority,
    SubmitIdentity,
    ReplayIdentity,
    ReplayDisposition,
}

struct PendingPipeline {
    request: Mutex<Option<serde_json::Value>>,
}

impl PendingPipeline {
    fn new() -> Self {
        Self {
            request: Mutex::new(None),
        }
    }
}

impl PendingActivationPort for PendingPipeline {
    async fn resolve_product_approval_context(
        &self,
        request: ResolveProductApprovalContextV1,
    ) -> Result<ResolvedProductApprovalContextV1, PendingActivationPortError> {
        assert_eq!(request.binding_revision.get(), 3);
        assert_eq!(request.required_channel_bindings, ["community_hub"]);
        let binding = serde_json::from_value(json!({
            "revision": 3,
            "required_bindings": [{
                "kind": "channel",
                "key": "community_hub",
                "id": "700"
            }],
            "fingerprint": "4c696fdcfce71c5b3a67e52d9c8762ee1b96510b576a67bb7f462fabc959a6d9"
        }))
        .unwrap();
        let baseline = serde_json::from_value(json!({"state": "absent"})).unwrap();
        Ok(ResolvedProductApprovalContextV1 { binding, baseline })
    }

    async fn ensure_pending_activation(
        &self,
        request: EnsurePendingActivationV1,
    ) -> Result<PendingActivationReceiptV1, PendingActivationPortError> {
        let context = serde_json::to_value(&request.create.context).unwrap();
        let value = json!({
            "id": request.create.id,
            "target": request.create.target,
            "requester": request.create.requester,
            "required_approvals": request.create.context.policy.required_approvals.get(),
            "approval_context": {
                "authority": "product_authoring",
                "context": context
            },
            "link_state": {"state": "unlinked"},
            "approvals": [],
            "state": "pending",
            "rejection": null,
            "apply_attempt_id": null,
            "apply_attempt_no": 0,
            "apply_lease_until": null,
            "last_apply_error": null,
            "observed_active": null,
            "completion": null,
            "termination": null,
            "created_at": "2099-01-01T00:00:00Z",
            "expires_at": "2099-01-01T01:00:00Z"
        });
        let activation = serde_json::from_value(value.clone()).unwrap();
        *self.request.lock().unwrap() = Some(value);
        Ok(PendingActivationReceiptV1 {
            request: activation,
            disposition: PendingActivationDispositionV1::Created,
        })
    }

    async fn link_pending_activation(
        &self,
        _request: authoring_promotion::LinkPendingActivationV1,
    ) -> Result<automation_ruleset_activation::ActivationRequest, PendingActivationPortError> {
        let mut value = self.request.lock().unwrap().take().unwrap();
        value["link_state"] = json!({"state": "linked", "linked_at": "2099-01-01T00:00:00Z"});
        Ok(serde_json::from_value(value).unwrap())
    }
}

impl AuthorizedPromotionSubmissionPort<Evidence> for PromotionCapture {
    async fn find_or_resume_authorized_promotion(
        &self,
        access: &AuthorizedPromotionAccessV1<'_, Evidence>,
    ) -> Result<Option<PromotionSubmissionV1>, AuthorizedPromotionSubmissionErrorV1> {
        self.events.lock().unwrap().push("resume");
        *self.access.lock().unwrap() = Some(CapturedPromotionAccess {
            request_id: access.request_id().as_str().to_string(),
            principal_id: access.actor().principal_id().as_str().to_string(),
            session_fingerprint: *access.session_fingerprint().as_bytes(),
            tenant_id: access.scope().tenant_id().as_str().to_string(),
            installation_id: access.scope().installation_id().as_str().to_string(),
            guild_id: access.scope().guild_id(),
            acting_user_id: access.scope().acting_user_id(),
            evidence: access.evidence().clone(),
            session_id: access.session_id().as_str().to_string(),
            expected_generation: access.expected_generation().get(),
            idempotency_key: access.with_product_idempotency_secret(<[u8]>::to_vec),
            debug: format!("{access:?}"),
        });
        let Some(artifact) = &self.replay_artifact else {
            return Ok(None);
        };
        let idempotency_key = if self.fault == PromotionCaptureFault::ReplayIdentity {
            IdempotencyKey::parse("different-promotion-key").unwrap()
        } else {
            access.with_product_idempotency_secret(|secret| {
                IdempotencyKey::parse(std::str::from_utf8(secret).unwrap()).unwrap()
            })
        };
        let input = StartPromotionV1 {
            idempotency_key,
            context: AuthenticatedPromotionContext {
                tenant_id: access.scope().tenant_id().clone(),
                principal_id: access.actor().principal_id().clone(),
                session_owner_id: access.actor().principal_id().clone(),
                session_id: access.session_id().clone(),
                session_generation: access.expected_generation(),
                guild_id: access.scope().guild_id(),
                installation_id: access.scope().installation_id().clone(),
                ruleset_key: "studyrooms".parse().unwrap(),
                requester: access.scope().acting_user_id(),
                binding_revision: BindingRevision::new(3).unwrap(),
                policy: ApprovalPolicyV1 {
                    revision: PolicyRevision::new(5).unwrap(),
                    required_approvals: NonZeroU32::new(2).unwrap(),
                    ttl_seconds: NonZeroU64::new(3600).unwrap(),
                },
            },
            artifact: artifact.clone(),
        };
        let mut submission = run_test_promotion(input).await?;
        if self.fault != PromotionCaptureFault::ReplayDisposition {
            submission.disposition = PromotionSubmissionDispositionV1::ExactReplay;
        }
        Ok(Some(submission))
    }

    async fn submit_authorized_promotion(
        &self,
        request: AuthorizedPromotionSubmissionV1<'_, Evidence>,
    ) -> Result<PromotionSubmissionV1, AuthorizedPromotionSubmissionErrorV1> {
        self.events.lock().unwrap().push("submit");
        let captured = CapturedPromotion {
            request_id: request.request_id().as_str().to_string(),
            principal_id: request.actor().principal_id().as_str().to_string(),
            session_fingerprint: *request.session_fingerprint().as_bytes(),
            tenant_id: request.scope().tenant_id().as_str().to_string(),
            installation_id: request.scope().installation_id().as_str().to_string(),
            guild_id: request.scope().guild_id(),
            acting_user_id: request.scope().acting_user_id(),
            evidence: request.evidence().clone(),
            context: request.input().context.clone(),
            debug: format!("{request:?}"),
        };
        *self.captured.lock().unwrap() = Some(captured);
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let mut input = request.into_input();
        match self.fault {
            PromotionCaptureFault::SubmitAuthority => {
                input.context.tenant_id = TenantId::parse("tenant-wrong").unwrap();
            }
            PromotionCaptureFault::SubmitIdentity => {
                input.idempotency_key = IdempotencyKey::parse("different-promotion-key").unwrap();
            }
            PromotionCaptureFault::None
            | PromotionCaptureFault::ReplayIdentity
            | PromotionCaptureFault::ReplayDisposition => {}
        }
        run_test_promotion(input).await
    }
}

async fn run_test_promotion(
    input: StartPromotionV1,
) -> Result<PromotionSubmissionV1, AuthorizedPromotionSubmissionErrorV1> {
    let promotions = InMemoryPromotionStore::default();
    let rulesets = InMemoryRuleSetStore::default();
    let pending = PendingPipeline::new();
    let service = PromotionService::new(&promotions, &rulesets, &pending, UtcPromotionClock);
    PromotionSubmissionPort::submit_verified_promotion(&service, input)
        .await
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::InvalidCandidate)
}

async fn run_expired_test_promotion(
    input: StartPromotionV1,
) -> Result<PromotionSubmissionV1, AuthorizedPromotionSubmissionErrorV1> {
    let submission = run_test_promotion(input).await?;
    let mut record = match submission.advancement {
        ResumePromotionOutcomeV1::Advanced(record)
        | ResumePromotionOutcomeV1::AlreadyActivationPending(record)
        | ResumePromotionOutcomeV1::TerminalExpired(record) => record,
    };
    let (publication, mut activation) = match record.stage {
        authoring_promotion::PromotionStageV1::ActivationPending {
            publication,
            activation,
        } => (publication, activation),
        _ => return Err(AuthorizedPromotionSubmissionErrorV1::InvalidCandidate),
    };
    activation.disposition = PendingActivationDispositionV1::Reused;
    activation.request_state_at_journal =
        automation_ruleset_activation::ActivationRequestState::Expired;
    record.updated_at = activation.expires_at;
    record.stage = authoring_promotion::PromotionStageV1::Expired {
        publication,
        activation,
    };
    record
        .validate()
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::InvalidCandidate)?;
    Ok(PromotionSubmissionV1 {
        disposition: submission.disposition,
        advancement: ResumePromotionOutcomeV1::TerminalExpired(record),
    })
}

struct ExpiredPromotionCapture;

impl AuthorizedPromotionSubmissionPort<Evidence> for ExpiredPromotionCapture {
    async fn find_or_resume_authorized_promotion(
        &self,
        _access: &AuthorizedPromotionAccessV1<'_, Evidence>,
    ) -> Result<Option<PromotionSubmissionV1>, AuthorizedPromotionSubmissionErrorV1> {
        Ok(None)
    }

    async fn submit_authorized_promotion(
        &self,
        request: AuthorizedPromotionSubmissionV1<'_, Evidence>,
    ) -> Result<PromotionSubmissionV1, AuthorizedPromotionSubmissionErrorV1> {
        run_expired_test_promotion(request.into_input()).await
    }
}

fn installation() -> InstallationSelectorV1 {
    InstallationSelectorV1::new(AutomationInstallationId::parse("installation-2").unwrap())
}

fn promote_command() -> PromoteOwnedSessionV1 {
    PromoteOwnedSessionV1 {
        idempotency_key: ProductPromotionIdempotencyKeyV1::parse("promotion-key").unwrap(),
        session_id: AuthoringSessionId::parse("session-1").unwrap(),
        expected_generation: SessionGeneration::new(7).unwrap(),
    }
}

#[test]
fn promotion_idempotency_secret_is_bounded_and_redacted() {
    assert_eq!(
        ProductPromotionIdempotencyKeyV1::parse("").unwrap_err(),
        ProductPromotionIdempotencyKeyError::Empty
    );
    assert_eq!(
        ProductPromotionIdempotencyKeyV1::parse(&"a".repeat(129)).unwrap_err(),
        ProductPromotionIdempotencyKeyError::TooLong
    );
    assert_eq!(
        ProductPromotionIdempotencyKeyV1::parse("key with spaces").unwrap_err(),
        ProductPromotionIdempotencyKeyError::InvalidCharacter
    );
    let key = ProductPromotionIdempotencyKeyV1::parse("private-promotion-key").unwrap();
    assert_eq!(
        format!("{key:?}"),
        "ProductPromotionIdempotencyKeyV1(<redacted>)"
    );
    assert_ne!(
        key,
        ProductPromotionIdempotencyKeyV1::parse("different-key").unwrap()
    );
}

#[test]
fn promotion_orders_authentication_fresh_authority_atomic_snapshot_and_submission() {
    block_on(async {
        let events = Arc::new(Mutex::new(Vec::new()));
        let authentication = Authentication {
            events: events.clone(),
            failure: None,
        };
        let authority = GuildAuthority {
            events: events.clone(),
            failure: None,
        };
        let snapshots = AuthorizedSnapshot {
            artifact: artifact().await,
            events: events.clone(),
        };
        let promotions = PromotionCapture {
            events: events.clone(),
            access: Mutex::new(None),
            captured: Mutex::new(None),
            failure: None,
            fault: PromotionCaptureFault::None,
            replay_artifact: None,
        };
        AuthoringApplication::new(&authentication, &authority, &snapshots, &promotions)
            .promote_owned_session(
                "opaque-session-token",
                "csrf-proof",
                &product_request_id(),
                &installation(),
                promote_command(),
            )
            .await
            .unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                "authenticate_mutation",
                "authorize",
                "resume",
                "atomic_snapshot",
                "submit"
            ]
        );
        let access = promotions.access.lock().unwrap().take().unwrap();
        assert_eq!(access.request_id, product_request_id().as_str());
        assert_eq!(access.principal_id, "principal-1");
        assert_eq!(access.session_fingerprint, [7_u8; 32]);
        assert_eq!(access.tenant_id, "tenant-1");
        assert_eq!(access.installation_id, "installation-2");
        assert_eq!(access.guild_id, GuildId(900));
        assert_eq!(access.acting_user_id, UserId(200));
        assert_eq!(access.evidence, Evidence("fresh-authority-evidence"));
        assert_eq!(access.session_id, "session-1");
        assert_eq!(access.expected_generation, 7);
        assert_eq!(access.idempotency_key, b"promotion-key");
        assert_eq!(access.debug, "AuthorizedPromotionAccessV1(<redacted>)");
        assert!(!access.debug.contains(product_request_id().as_str()));
        assert!(!access.debug.contains("promotion-key"));
        let captured = promotions.captured.lock().unwrap().take().unwrap();
        assert_eq!(captured.request_id, product_request_id().as_str());
        assert_eq!(captured.principal_id, "principal-1");
        assert_eq!(captured.session_fingerprint, [7_u8; 32]);
        assert_eq!(captured.tenant_id, "tenant-1");
        assert_eq!(captured.installation_id, "installation-2");
        assert_eq!(captured.guild_id, GuildId(900));
        assert_eq!(captured.acting_user_id, UserId(200));
        assert_eq!(captured.evidence, Evidence("fresh-authority-evidence"));
        assert_eq!(captured.context.tenant_id.as_str(), "tenant-1");
        assert_eq!(captured.context.principal_id.as_str(), "principal-1");
        assert_eq!(captured.context.installation_id.as_str(), "installation-2");
        assert_eq!(captured.context.guild_id, GuildId(900));
        assert_eq!(captured.context.requester, UserId(200));
        assert_eq!(
            captured.debug,
            "AuthorizedPromotionSubmissionV1(<redacted>)"
        );
        assert!(!captured.debug.contains(product_request_id().as_str()));
        assert!(!captured.debug.contains("principal-1"));
    });
}

#[test]
fn promotion_observation_projects_exact_created_metadata() {
    block_on(async {
        let events = Arc::new(Mutex::new(Vec::new()));
        let authentication = Authentication {
            events: events.clone(),
            failure: None,
        };
        let authority = GuildAuthority {
            events: events.clone(),
            failure: None,
        };
        let snapshots = AuthorizedSnapshot {
            artifact: artifact().await,
            events: events.clone(),
        };
        let promotions = PromotionCapture {
            events,
            access: Mutex::new(None),
            captured: Mutex::new(None),
            failure: None,
            fault: PromotionCaptureFault::None,
            replay_artifact: None,
        };
        let observation =
            AuthoringApplication::new(&authentication, &authority, &snapshots, &promotions)
                .promote_owned_session_observation(
                    "opaque-session-token",
                    "csrf-proof",
                    &product_request_id(),
                    &installation(),
                    promote_command(),
                )
                .await
                .unwrap();
        assert_eq!(observation.revision(), 3);
        assert_eq!(
            observation.state(),
            ProductPromotionStateV1::ActivationLinked
        );
        assert!(!observation.exact_replay());
        assert_eq!(observation.target_version(), 1);
        assert_eq!(observation.target_content_hash().len(), 64);
        assert_eq!(observation.approval_payload_digest().as_str().len(), 64);
        assert_eq!(observation.promotion_id().as_str().len(), 64);
        assert!(observation.activation_expires_at() > SystemTime::now());
    });
}

#[test]
fn promotion_observation_preserves_exact_replay_metadata() {
    block_on(async {
        let events = Arc::new(Mutex::new(Vec::new()));
        let replay_artifact = artifact().await;
        let authentication = Authentication {
            events: events.clone(),
            failure: None,
        };
        let authority = GuildAuthority {
            events: events.clone(),
            failure: None,
        };
        let snapshots = AuthorizedSnapshot {
            artifact: replay_artifact.clone(),
            events: events.clone(),
        };
        let promotions = PromotionCapture {
            events,
            access: Mutex::new(None),
            captured: Mutex::new(None),
            failure: None,
            fault: PromotionCaptureFault::None,
            replay_artifact: Some(replay_artifact),
        };
        let observation =
            AuthoringApplication::new(&authentication, &authority, &snapshots, &promotions)
                .promote_owned_session_observation(
                    "opaque-session-token",
                    "csrf-proof",
                    &product_request_id(),
                    &installation(),
                    promote_command(),
                )
                .await
                .unwrap();
        assert!(observation.exact_replay());
        assert_eq!(observation.revision(), 3);
        assert_eq!(
            observation.state(),
            ProductPromotionStateV1::ActivationLinked
        );
    });
}

#[test]
fn promotion_observation_projects_expired_terminal_metadata() {
    block_on(async {
        let events = Arc::new(Mutex::new(Vec::new()));
        let authentication = Authentication {
            events: events.clone(),
            failure: None,
        };
        let authority = GuildAuthority {
            events: events.clone(),
            failure: None,
        };
        let snapshots = AuthorizedSnapshot {
            artifact: artifact().await,
            events,
        };
        let observation = AuthoringApplication::new(
            &authentication,
            &authority,
            &snapshots,
            &ExpiredPromotionCapture,
        )
        .promote_owned_session_observation(
            "opaque-session-token",
            "csrf-proof",
            &product_request_id(),
            &installation(),
            promote_command(),
        )
        .await
        .unwrap();
        assert_eq!(observation.revision(), 3);
        assert_eq!(observation.state(), ProductPromotionStateV1::Expired);
        assert!(!observation.exact_replay());
        assert_eq!(observation.target_version(), 1);
        assert_eq!(observation.target_content_hash().len(), 64);
    });
}

#[test]
fn exact_replay_resumes_before_snapshot_and_skips_new_submission() {
    block_on(async {
        let events = Arc::new(Mutex::new(Vec::new()));
        let authentication = Authentication {
            events: events.clone(),
            failure: None,
        };
        let authority = GuildAuthority {
            events: events.clone(),
            failure: None,
        };
        let replay_artifact = artifact().await;
        let snapshots = AuthorizedSnapshot {
            artifact: replay_artifact.clone(),
            events: events.clone(),
        };
        let promotions = PromotionCapture {
            events: events.clone(),
            access: Mutex::new(None),
            captured: Mutex::new(None),
            failure: None,
            fault: PromotionCaptureFault::None,
            replay_artifact: Some(replay_artifact),
        };
        let output =
            AuthoringApplication::new(&authentication, &authority, &snapshots, &promotions)
                .promote_owned_session(
                    "opaque-session-token",
                    "csrf-proof",
                    &product_request_id(),
                    &installation(),
                    promote_command(),
                )
                .await
                .unwrap();
        assert_eq!(
            output.disposition,
            PromotionSubmissionDispositionV1::ExactReplay
        );
        assert_eq!(
            *events.lock().unwrap(),
            vec!["authenticate_mutation", "authorize", "resume"]
        );
        let access = promotions.access.lock().unwrap().take().unwrap();
        assert_eq!(access.request_id, product_request_id().as_str());
        assert_eq!(access.session_id, "session-1");
        assert_eq!(access.expected_generation, 7);
        assert!(promotions.captured.lock().unwrap().is_none());
    });
}

#[test]
fn replay_requires_exact_disposition_and_deterministic_identity_without_snapshot_access() {
    block_on(async {
        for fault in [
            PromotionCaptureFault::ReplayIdentity,
            PromotionCaptureFault::ReplayDisposition,
        ] {
            let events = Arc::new(Mutex::new(Vec::new()));
            let replay_artifact = artifact().await;
            let authentication = Authentication {
                events: events.clone(),
                failure: None,
            };
            let authority = GuildAuthority {
                events: events.clone(),
                failure: None,
            };
            let snapshots = AuthorizedSnapshot {
                artifact: replay_artifact.clone(),
                events: events.clone(),
            };
            let promotions = PromotionCapture {
                events: events.clone(),
                access: Mutex::new(None),
                captured: Mutex::new(None),
                failure: None,
                fault,
                replay_artifact: Some(replay_artifact),
            };
            let error =
                AuthoringApplication::new(&authentication, &authority, &snapshots, &promotions)
                    .promote_owned_session_observation(
                        "opaque-session-token",
                        "csrf-proof",
                        &product_request_id(),
                        &installation(),
                        promote_command(),
                    )
                    .await
                    .unwrap_err();
            assert_eq!(
                error,
                AuthoringApplicationError::AuthorizedPromotion(
                    AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt
                )
            );
            assert_eq!(
                *events.lock().unwrap(),
                vec!["authenticate_mutation", "authorize", "resume"]
            );
        }
    });
}

#[test]
fn authentication_and_authority_failures_stop_every_downstream_port() {
    block_on(async {
        for (authentication_failure, authority_failure, expected) in [
            (
                Some(AuthenticationError::Revoked),
                None,
                vec!["authenticate_mutation"],
            ),
            (
                None,
                Some(FreshGuildAuthorityError::Forbidden),
                vec!["authenticate_mutation", "authorize"],
            ),
        ] {
            let events = Arc::new(Mutex::new(Vec::new()));
            let authentication = Authentication {
                events: events.clone(),
                failure: authentication_failure,
            };
            let authority = GuildAuthority {
                events: events.clone(),
                failure: authority_failure,
            };
            let snapshots = AuthorizedSnapshot {
                artifact: artifact().await,
                events: events.clone(),
            };
            let promotions = PromotionCapture {
                events: events.clone(),
                access: Mutex::new(None),
                captured: Mutex::new(None),
                failure: None,
                fault: PromotionCaptureFault::None,
                replay_artifact: None,
            };
            assert!(AuthoringApplication::new(
                &authentication,
                &authority,
                &snapshots,
                &promotions
            )
            .promote_owned_session(
                "opaque-session-token",
                "csrf-proof",
                &product_request_id(),
                &installation(),
                promote_command(),
            )
            .await
            .is_err());
            assert_eq!(*events.lock().unwrap(), expected);
            assert!(promotions.access.lock().unwrap().is_none());
            assert!(promotions.captured.lock().unwrap().is_none());
        }
    });
}

#[test]
fn invalid_csrf_stops_before_authority_and_snapshot_access() {
    block_on(async {
        let events = Arc::new(Mutex::new(Vec::new()));
        let authentication = Authentication {
            events: events.clone(),
            failure: None,
        };
        let authority = GuildAuthority {
            events: events.clone(),
            failure: None,
        };
        let snapshots = AuthorizedSnapshot {
            artifact: artifact().await,
            events: events.clone(),
        };
        let promotions = PromotionCapture {
            events: events.clone(),
            access: Mutex::new(None),
            captured: Mutex::new(None),
            failure: None,
            fault: PromotionCaptureFault::None,
            replay_artifact: None,
        };
        let error = AuthoringApplication::new(&authentication, &authority, &snapshots, &promotions)
            .promote_owned_session(
                "opaque-session-token",
                "wrong-csrf",
                &product_request_id(),
                &installation(),
                promote_command(),
            )
            .await
            .unwrap_err();
        assert_eq!(
            error,
            AuthoringApplicationError::Authentication(AuthenticationError::InvalidCsrf)
        );
        assert_eq!(*events.lock().unwrap(), vec!["authenticate_mutation"]);
        assert!(promotions.access.lock().unwrap().is_none());
        assert!(promotions.captured.lock().unwrap().is_none());
    });
}

#[test]
fn authorized_promotion_error_propagates_without_losing_the_authenticated_boundary() {
    block_on(async {
        let events = Arc::new(Mutex::new(Vec::new()));
        let authentication = Authentication {
            events: events.clone(),
            failure: None,
        };
        let authority = GuildAuthority {
            events: events.clone(),
            failure: None,
        };
        let snapshots = AuthorizedSnapshot {
            artifact: artifact().await,
            events: events.clone(),
        };
        let promotions = PromotionCapture {
            events: events.clone(),
            access: Mutex::new(None),
            captured: Mutex::new(None),
            failure: Some(AuthorizedPromotionSubmissionErrorV1::Indeterminate),
            fault: PromotionCaptureFault::None,
            replay_artifact: None,
        };
        let error = AuthoringApplication::new(&authentication, &authority, &snapshots, &promotions)
            .promote_owned_session(
                "opaque-session-token",
                "csrf-proof",
                &product_request_id(),
                &installation(),
                promote_command(),
            )
            .await
            .unwrap_err();
        assert_eq!(
            error,
            AuthoringApplicationError::AuthorizedPromotion(
                AuthorizedPromotionSubmissionErrorV1::Indeterminate
            )
        );
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                "authenticate_mutation",
                "authorize",
                "resume",
                "atomic_snapshot",
                "submit"
            ]
        );
        let captured = promotions.captured.lock().unwrap().take().unwrap();
        assert_eq!(captured.request_id, product_request_id().as_str());
        assert_eq!(captured.session_fingerprint, [7_u8; 32]);
        assert_eq!(captured.evidence, Evidence("fresh-authority-evidence"));
    });
}

#[test]
fn valid_but_wrong_final_promotion_projection_is_rejected() {
    block_on(async {
        let events = Arc::new(Mutex::new(Vec::new()));
        let authentication = Authentication {
            events: events.clone(),
            failure: None,
        };
        let authority = GuildAuthority {
            events: events.clone(),
            failure: None,
        };
        let snapshots = AuthorizedSnapshot {
            artifact: artifact().await,
            events: events.clone(),
        };
        let promotions = PromotionCapture {
            events,
            access: Mutex::new(None),
            captured: Mutex::new(None),
            failure: None,
            fault: PromotionCaptureFault::SubmitAuthority,
            replay_artifact: None,
        };
        let error = AuthoringApplication::new(&authentication, &authority, &snapshots, &promotions)
            .promote_owned_session(
                "opaque-session-token",
                "csrf-proof",
                &product_request_id(),
                &installation(),
                promote_command(),
            )
            .await
            .unwrap_err();
        assert_eq!(
            error,
            AuthoringApplicationError::AuthorizedPromotion(
                AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt
            )
        );
        let captured = promotions.captured.lock().unwrap().take().unwrap();
        assert_eq!(captured.context.tenant_id.as_str(), "tenant-1");
    });
}

#[test]
fn valid_submission_with_different_identity_and_request_digest_is_rejected() {
    block_on(async {
        let events = Arc::new(Mutex::new(Vec::new()));
        let authentication = Authentication {
            events: events.clone(),
            failure: None,
        };
        let authority = GuildAuthority {
            events: events.clone(),
            failure: None,
        };
        let snapshots = AuthorizedSnapshot {
            artifact: artifact().await,
            events: events.clone(),
        };
        let promotions = PromotionCapture {
            events,
            access: Mutex::new(None),
            captured: Mutex::new(None),
            failure: None,
            fault: PromotionCaptureFault::SubmitIdentity,
            replay_artifact: None,
        };
        let error = AuthoringApplication::new(&authentication, &authority, &snapshots, &promotions)
            .promote_owned_session(
                "opaque-session-token",
                "csrf-proof",
                &product_request_id(),
                &installation(),
                promote_command(),
            )
            .await
            .unwrap_err();
        assert_eq!(
            error,
            AuthoringApplicationError::AuthorizedPromotion(
                AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt
            )
        );
    });
}

fn promotion_id() -> PromotionId {
    PromotionId::parse(&"a".repeat(64)).unwrap()
}

fn product_request_id() -> ProductRequestIdV1 {
    ProductRequestIdV1::parse("req_01JZ7QW9YB2G8M4K6T3P5R1C0D").unwrap()
}

fn approve_command() -> ApproveProductPromotionV1 {
    ApproveProductPromotionV1 {
        promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
        expected_payload_digest: ApprovalPayloadDigestV1::parse(&"c".repeat(64)).unwrap(),
        expected_revision: ProductRevisionV1::new(3).unwrap(),
        idempotency_key: ProductIdempotencyKeyV1::parse("approve-key").unwrap(),
    }
}

#[test]
fn product_request_id_is_bounded_validated_and_redacted() {
    assert_eq!(
        ProductRequestIdV1::parse("").unwrap_err(),
        ProductRequestIdError::Empty
    );
    assert_eq!(
        ProductRequestIdV1::parse(&"a".repeat(129)).unwrap_err(),
        ProductRequestIdError::TooLong
    );
    for invalid in ["request id", "요청", "request/id", "request\n"] {
        assert_eq!(
            ProductRequestIdV1::parse(invalid).unwrap_err(),
            ProductRequestIdError::InvalidCharacter
        );
    }
    let request_id = product_request_id();
    assert_eq!(request_id.as_str(), "req_01JZ7QW9YB2G8M4K6T3P5R1C0D");
    let debug = format!("{request_id:?}");
    assert_eq!(debug, "ProductRequestIdV1(<redacted>)");
    assert!(!debug.contains(request_id.as_str()));
}

fn exact_deployment() -> ExactDeploymentSelectorV1 {
    ExactDeploymentSelectorV1::from_server_projection(
        AutomationInstallationId::parse("installation-2").unwrap(),
        promotion_id(),
        "deployment-1",
        "b".repeat(64),
    )
    .unwrap()
}

fn decision_projection(phase: ProductDecisionPhaseV1) -> ProductDecisionProjectionV1 {
    ProductDecisionProjectionV1::from_server_projection(
        TenantId::parse("tenant-1").unwrap(),
        AutomationInstallationId::parse("installation-2").unwrap(),
        GuildId(900),
        promotion_id(),
        ProductRevisionV1::new(4).unwrap(),
        phase,
    )
}

struct Decisions {
    events: Arc<Mutex<Vec<&'static str>>>,
    phase: Mutex<ProductDecisionPhaseV1>,
    apply_phase: Mutex<ProductDecisionPhaseV1>,
    apply_exact_replay: Mutex<bool>,
}

impl ProductDecisionQueryPort<Evidence> for Decisions {
    async fn load_approval_preview(
        &self,
        _request: AuthorizedApprovalPreviewV1<'_, Evidence>,
    ) -> Result<ProductApprovalPreviewV1, ProductControlPortError> {
        panic!("preview is not used by this fixture")
    }

    async fn load_product_status(
        &self,
        request: AuthorizedProductStatusV1<'_, Evidence>,
    ) -> Result<ProductDecisionProjectionV1, ProductControlPortError> {
        self.events.lock().unwrap().push("decision_status");
        assert_eq!(request.actor().principal_id().as_str(), "principal-1");
        assert_eq!(request.scope().acting_user_id(), UserId(200));
        assert_eq!(request.evidence(), &Evidence("fresh-authority-evidence"));
        assert_eq!(request.promotion().promotion_id(), &promotion_id());
        Ok(decision_projection(self.phase.lock().unwrap().clone()))
    }
}

impl ProductApprovalPort<Evidence> for Decisions {
    async fn approve_payload_bound(
        &self,
        request: AuthorizedApproveProductV1<'_, Evidence>,
    ) -> Result<ProductMutationReceiptV1, ProductControlPortError> {
        self.events.lock().unwrap().push("approve_payload_bound");
        assert_eq!(request.request_id(), &product_request_id());
        assert_eq!(request.actor().principal_id().as_str(), "principal-1");
        assert_eq!(request.session_fingerprint().as_bytes(), &[7_u8; 32]);
        assert_eq!(request.scope().acting_user_id(), UserId(200));
        assert_eq!(request.evidence(), &Evidence("fresh-authority-evidence"));
        assert_eq!(
            format!("{:?}", request.context()),
            "ProductMutationContextV1(<redacted>)"
        );
        assert_eq!(
            request.command().expected_payload_digest.as_str(),
            "c".repeat(64)
        );
        assert_eq!(request.command().expected_revision.get(), 3);
        assert_eq!(request.command().idempotency_key.as_str(), "approve-key");
        Ok(ProductMutationReceiptV1::from_server_projection(
            decision_projection(self.phase.lock().unwrap().clone()),
            false,
        ))
    }
}

impl ProductRejectionPort<Evidence> for Decisions {
    async fn reject_payload_bound(
        &self,
        request: AuthorizedRejectProductV1<'_, Evidence>,
    ) -> Result<ProductMutationReceiptV1, ProductControlPortError> {
        self.events.lock().unwrap().push("reject_payload_bound");
        assert_eq!(request.request_id(), &product_request_id());
        assert_eq!(request.session_fingerprint().as_bytes(), &[7_u8; 32]);
        assert_eq!(request.scope().acting_user_id(), UserId(200));
        assert_eq!(request.evidence(), &Evidence("fresh-authority-evidence"));
        assert_eq!(request.command().reason.as_str(), "unsafe requested scope");
        assert_eq!(request.command().idempotency_key.as_str(), "reject-key");
        Ok(ProductMutationReceiptV1::from_server_projection(
            decision_projection(ProductDecisionPhaseV1::Rejected),
            false,
        ))
    }
}

impl ProductApplyPort<Evidence> for Decisions {
    async fn apply_idempotent(
        &self,
        request: AuthorizedApplyProductV1<'_, Evidence>,
    ) -> Result<ProductMutationReceiptV1, ProductControlPortError> {
        self.events.lock().unwrap().push("apply_idempotent");
        assert_eq!(request.request_id(), &product_request_id());
        assert_eq!(request.actor().principal_id().as_str(), "principal-1");
        assert_eq!(request.session_fingerprint().as_bytes(), &[7_u8; 32]);
        assert_eq!(request.scope().acting_user_id(), UserId(200));
        assert_eq!(request.evidence(), &Evidence("fresh-authority-evidence"));
        assert_eq!(
            request.command().expected_payload_digest.as_str(),
            "c".repeat(64)
        );
        assert_eq!(request.command().expected_revision.get(), 3);
        assert_eq!(request.command().idempotency_key.as_str(), "apply-key");
        Ok(ProductMutationReceiptV1::from_server_projection(
            decision_projection(self.apply_phase.lock().unwrap().clone()),
            *self.apply_exact_replay.lock().unwrap(),
        ))
    }
}

fn assert_compatibility_decision_port<T: ProductDecisionPort<Evidence>>() {}

struct Deployments {
    events: Arc<Mutex<Vec<&'static str>>>,
    status: Mutex<DeploymentStatusProjectionV1>,
}

impl DeploymentStatusPort<Evidence> for Deployments {
    async fn load_exact_deployment_status(
        &self,
        request: AuthorizedDeploymentStatusV1<'_, Evidence>,
    ) -> Result<DeploymentStatusProjectionV1, DeploymentStatusPortError> {
        self.events.lock().unwrap().push("deployment_status");
        assert_eq!(request.actor().principal_id().as_str(), "principal-1");
        assert_eq!(request.scope().installation_id().as_str(), "installation-2");
        assert_eq!(request.evidence(), &Evidence("fresh-authority-evidence"));
        assert_eq!(request.exact_deployment(), &exact_deployment());
        Ok(self.status.lock().unwrap().clone())
    }
}

fn product_fixture(
    phase: ProductDecisionPhaseV1,
    deployment: DeploymentStatusProjectionV1,
) -> (
    Arc<Mutex<Vec<&'static str>>>,
    Authentication,
    GuildAuthority,
    Decisions,
    Deployments,
) {
    let events = Arc::new(Mutex::new(Vec::new()));
    (
        events.clone(),
        Authentication {
            events: events.clone(),
            failure: None,
        },
        GuildAuthority {
            events: events.clone(),
            failure: None,
        },
        Decisions {
            events: events.clone(),
            phase: Mutex::new(phase),
            apply_phase: Mutex::new(ProductDecisionPhaseV1::Applied {
                exact_deployment: exact_deployment(),
            }),
            apply_exact_replay: Mutex::new(false),
        },
        Deployments {
            events: events.clone(),
            status: Mutex::new(deployment),
        },
    )
}

struct ObservationDecisions {
    preview: ProductApprovalPreviewObservationV1,
    status: ProductDecisionObservationV1,
}

impl ProductDecisionObservationPort<Evidence> for ObservationDecisions {
    async fn load_approval_preview_observation(
        &self,
        _request: AuthorizedApprovalPreviewV1<'_, Evidence>,
    ) -> Result<ProductApprovalPreviewObservationV1, ProductControlPortError> {
        Ok(self.preview.clone())
    }

    async fn load_product_status_observation(
        &self,
        _request: AuthorizedProductStatusV1<'_, Evidence>,
    ) -> Result<ProductDecisionObservationV1, ProductControlPortError> {
        Ok(self.status.clone())
    }
}

struct ObservationDeployments {
    status: DeploymentStatusObservationV1,
}

impl DeploymentStatusObservationPort<Evidence> for ObservationDeployments {
    async fn load_exact_deployment_observation(
        &self,
        _request: AuthorizedDeploymentStatusV1<'_, Evidence>,
    ) -> Result<DeploymentStatusObservationV1, DeploymentStatusPortError> {
        Ok(self.status.clone())
    }
}

async fn exact_preview_observation(
    observed_at: SystemTime,
) -> (
    authoring_application::PromotionSelectorV1,
    ProductApprovalPreviewObservationV1,
) {
    let input = StartPromotionV1 {
        idempotency_key: IdempotencyKey::parse("observation-promotion-key").unwrap(),
        context: AuthenticatedPromotionContext {
            tenant_id: TenantId::parse("tenant-1").unwrap(),
            principal_id: PrincipalId::parse("principal-1").unwrap(),
            session_owner_id: PrincipalId::parse("principal-1").unwrap(),
            session_id: AuthoringSessionId::parse("session-1").unwrap(),
            session_generation: SessionGeneration::new(7).unwrap(),
            guild_id: GuildId(900),
            installation_id: AutomationInstallationId::parse("installation-2").unwrap(),
            ruleset_key: "studyrooms".parse().unwrap(),
            requester: UserId(200),
            binding_revision: BindingRevision::new(3).unwrap(),
            policy: ApprovalPolicyV1 {
                revision: PolicyRevision::new(5).unwrap(),
                required_approvals: NonZeroU32::new(2).unwrap(),
                ttl_seconds: NonZeroU64::new(3600).unwrap(),
            },
        },
        artifact: artifact().await,
    };
    let submission = run_test_promotion(input).await.unwrap();
    let record = match &submission.advancement {
        ResumePromotionOutcomeV1::Advanced(record)
        | ResumePromotionOutcomeV1::AlreadyActivationPending(record)
        | ResumePromotionOutcomeV1::TerminalExpired(record) => record,
    };
    let payload = record.product_approval_payload().unwrap();
    let digest = approval_payload_digest_v1(&payload).unwrap();
    let activation_expires_at = match &record.stage {
        authoring_promotion::PromotionStageV1::ActivationPending { activation, .. }
        | authoring_promotion::PromotionStageV1::Expired { activation, .. } => {
            activation.expires_at.into()
        }
        _ => panic!("unexpected promotion stage"),
    };
    let selector = authoring_application::PromotionSelectorV1::new(record.id.clone());
    let preview = ProductApprovalPreviewV1::from_server_projection(
        AutomationInstallationId::parse("installation-2").unwrap(),
        GuildId(900),
        payload,
        ApprovalPayloadDigestV1::parse(digest.as_str()).unwrap(),
        ProductRevisionV1::new(1).unwrap(),
        ProductDecisionPhaseV1::PendingApproval,
    );
    (
        selector,
        ProductApprovalPreviewObservationV1::from_server_projection(
            preview,
            activation_expires_at,
            observed_at,
        ),
    )
}

#[test]
fn exact_preview_and_status_observations_preserve_database_time_and_revision() {
    block_on(async {
        let observed_at = UNIX_EPOCH + Duration::from_secs(4_000_000_000);
        let (selector, preview) = exact_preview_observation(observed_at).await;
        let decision = ProductDecisionProjectionV1::from_server_projection(
            TenantId::parse("tenant-1").unwrap(),
            AutomationInstallationId::parse("installation-2").unwrap(),
            GuildId(900),
            selector.promotion_id().clone(),
            ProductRevisionV1::new(7).unwrap(),
            ProductDecisionPhaseV1::PendingApproval,
        );
        let decisions = ObservationDecisions {
            preview: preview.clone(),
            status: ProductDecisionObservationV1::from_server_projection(decision, observed_at),
        };
        let deployments = ObservationDeployments {
            status: DeploymentStatusObservationV1::from_server_projection(
                DeploymentStatusProjectionV1::Pending,
                observed_at,
                None,
                None,
            )
            .unwrap(),
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let authentication = Authentication {
            events: events.clone(),
            failure: None,
        };
        let authority = GuildAuthority {
            events,
            failure: None,
        };
        let application =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
        let preview_result = application
            .get_approval_preview_observation(
                "opaque-session-token",
                &installation(),
                ProductStatusQueryV1 {
                    promotion: selector.clone(),
                },
            )
            .await
            .unwrap();
        assert_eq!(preview_result.observed_at(), observed_at);
        assert_eq!(
            preview_result.activation_expires_at(),
            preview.activation_expires_at()
        );
        let status = application
            .get_product_status_observation(
                "opaque-session-token",
                &installation(),
                ProductStatusQueryV1 {
                    promotion: selector,
                },
            )
            .await
            .unwrap();
        assert_eq!(status.status(), ProductStatusV1::PendingApproval);
        assert_eq!(status.decision().revision().get(), 7);
        assert_eq!(status.decision_observed_at(), observed_at);
        assert!(status.deployment().is_none());
    });
}

#[test]
fn live_observation_preserves_attestation_heartbeat_and_lease_ordering() {
    block_on(async {
        let decision_observed_at = UNIX_EPOCH + Duration::from_secs(100);
        let runtime_observed_at = UNIX_EPOCH + Duration::from_secs(120);
        let last_heartbeat_at = UNIX_EPOCH + Duration::from_secs(110);
        let lease_expires_at = UNIX_EPOCH + Duration::from_secs(130);
        let exact = exact_deployment();
        let decision = decision_projection(ProductDecisionPhaseV1::Applied {
            exact_deployment: exact.clone(),
        });
        let (_, preview) = exact_preview_observation(UNIX_EPOCH + Duration::from_secs(1)).await;
        let decisions = ObservationDecisions {
            preview,
            status: ProductDecisionObservationV1::from_server_projection(
                decision,
                decision_observed_at,
            ),
        };
        let deployments = ObservationDeployments {
            status: DeploymentStatusObservationV1::from_server_projection(
                DeploymentStatusProjectionV1::ExactLive(
                    ExactLiveProjectionV1::from_exact_attestation(
                        exact,
                        NonZeroU64::new(9).unwrap(),
                    ),
                ),
                runtime_observed_at,
                Some(last_heartbeat_at),
                Some(lease_expires_at),
            )
            .unwrap(),
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let authentication = Authentication {
            events: events.clone(),
            failure: None,
        };
        let authority = GuildAuthority {
            events,
            failure: None,
        };
        let observation =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments)
                .get_product_status_observation(
                    "opaque-session-token",
                    &installation(),
                    ProductStatusQueryV1 {
                        promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
                    },
                )
                .await
                .unwrap();
        assert_eq!(observation.status(), ProductStatusV1::Live);
        assert_eq!(observation.decision().revision().get(), 4);
        let runtime = observation.deployment().unwrap();
        assert_eq!(runtime.observed_at(), runtime_observed_at);
        assert_eq!(runtime.last_heartbeat_at(), Some(last_heartbeat_at));
        assert_eq!(runtime.lease_expires_at(), Some(lease_expires_at));
        assert!(last_heartbeat_at <= runtime.observed_at());
        assert!(runtime.observed_at() < lease_expires_at);
    });
}

#[test]
fn runtime_observation_before_decision_observation_fails_closed() {
    block_on(async {
        let exact = exact_deployment();
        let decision_observed_at = UNIX_EPOCH + Duration::from_secs(120);
        let runtime_observed_at = UNIX_EPOCH + Duration::from_secs(110);
        let (_, preview) = exact_preview_observation(UNIX_EPOCH + Duration::from_secs(1)).await;
        let decisions = ObservationDecisions {
            preview,
            status: ProductDecisionObservationV1::from_server_projection(
                decision_projection(ProductDecisionPhaseV1::Applied {
                    exact_deployment: exact,
                }),
                decision_observed_at,
            ),
        };
        let deployments = ObservationDeployments {
            status: DeploymentStatusObservationV1::from_server_projection(
                DeploymentStatusProjectionV1::Pending,
                runtime_observed_at,
                None,
                None,
            )
            .unwrap(),
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let authentication = Authentication {
            events: events.clone(),
            failure: None,
        };
        let authority = GuildAuthority {
            events,
            failure: None,
        };
        assert_eq!(
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments)
                .get_product_status_observation(
                    "opaque-session-token",
                    &installation(),
                    ProductStatusQueryV1 {
                        promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
                    },
                )
                .await
                .unwrap_err(),
            ProductApplicationError::InvalidProjection
        );
    });
}

#[test]
fn deployment_failure_observation_exposes_only_stable_bounded_metadata() {
    let observed_at = UNIX_EPOCH + Duration::from_secs(120);
    let observation = DeploymentStatusObservationV1::from_server_projection(
        DeploymentStatusProjectionV1::Failed {
            retryable: true,
            failure_code: "gateway_start_failed".to_string(),
        },
        observed_at,
        None,
        None,
    )
    .unwrap();
    let failure = observation.failure().unwrap();
    assert!(failure.retryable());
    assert_eq!(failure.failure_code().as_str(), "gateway_start_failed");
    assert!(DeploymentFailureCodeV1::parse("private_internal_identifier").is_err());
    assert!(DeploymentStatusObservationV1::from_server_projection(
        DeploymentStatusProjectionV1::Failed {
            retryable: false,
            failure_code: "private_internal_identifier".to_string(),
        },
        observed_at,
        None,
        None,
    )
    .is_err());
}

#[test]
fn approval_supports_pending_quorum_and_approved_outcomes() {
    block_on(async {
        assert_compatibility_decision_port::<Decisions>();
        for phase in [
            ProductDecisionPhaseV1::PendingApproval,
            ProductDecisionPhaseV1::Approved,
        ] {
            let (events, authentication, authority, decisions, deployments) =
                product_fixture(phase.clone(), DeploymentStatusProjectionV1::NotRequested);
            let application = ProductControlApplication::new(
                &authentication,
                &authority,
                &decisions,
                &deployments,
            );
            let receipt = application
                .approve(
                    "opaque-session-token",
                    "csrf-proof",
                    &product_request_id(),
                    &installation(),
                    approve_command(),
                )
                .await
                .unwrap();
            assert!(!receipt.exact_replay());
            assert_eq!(receipt.projection().phase(), &phase);
            assert_eq!(
                *events.lock().unwrap(),
                vec![
                    "authenticate_mutation",
                    "authorize",
                    "approve_payload_bound"
                ]
            );
        }
    });
}

#[test]
fn approval_rejects_non_approval_success_projection() {
    block_on(async {
        let (_, authentication, authority, decisions, deployments) = product_fixture(
            ProductDecisionPhaseV1::Rejected,
            DeploymentStatusProjectionV1::NotRequested,
        );
        let error =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments)
                .approve(
                    "opaque-session-token",
                    "csrf-proof",
                    &product_request_id(),
                    &installation(),
                    approve_command(),
                )
                .await
                .unwrap_err();
        assert_eq!(error, ProductApplicationError::InvalidProjection);
    });
}

#[test]
fn reject_reason_is_bounded_and_digest_bound_port_is_used() {
    assert_eq!(
        RejectionReasonV1::parse("  ").unwrap_err(),
        RejectionReasonError::Empty
    );
    assert_eq!(
        RejectionReasonV1::parse(&"x".repeat(1_001)).unwrap_err(),
        RejectionReasonError::TooLong
    );
    assert!(RejectionReasonV1::parse(&"😀".repeat(1_000)).is_ok());
    assert_eq!(
        RejectionReasonV1::parse("line\nbreak").unwrap_err(),
        RejectionReasonError::ControlCharacter
    );
    block_on(async {
        let (events, authentication, authority, decisions, deployments) = product_fixture(
            ProductDecisionPhaseV1::PendingApproval,
            DeploymentStatusProjectionV1::NotRequested,
        );
        ProductControlApplication::new(&authentication, &authority, &decisions, &deployments)
            .reject(
                "opaque-session-token",
                "csrf-proof",
                &product_request_id(),
                &installation(),
                RejectProductPromotionV1 {
                    promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
                    expected_payload_digest: ApprovalPayloadDigestV1::parse(&"c".repeat(64))
                        .unwrap(),
                    expected_revision: ProductRevisionV1::new(3).unwrap(),
                    idempotency_key: ProductIdempotencyKeyV1::parse("reject-key").unwrap(),
                    reason: RejectionReasonV1::parse("unsafe requested scope").unwrap(),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            vec!["authenticate_mutation", "authorize", "reject_payload_bound"]
        );
    });
}

#[test]
fn applied_pointer_is_runtime_pending_until_exact_live_attestation() {
    block_on(async {
        let (events, authentication, authority, decisions, deployments) = product_fixture(
            ProductDecisionPhaseV1::Applied {
                exact_deployment: exact_deployment(),
            },
            DeploymentStatusProjectionV1::Pending,
        );
        let application =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
        let query = ProductStatusQueryV1 {
            promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
        };
        assert_eq!(
            application
                .get_product_status("opaque-session-token", &installation(), query.clone())
                .await
                .unwrap(),
            ProductStatusV1::RuntimePending
        );
        *deployments.status.lock().unwrap() =
            DeploymentStatusProjectionV1::ExactLive(ExactLiveProjectionV1::from_exact_attestation(
                exact_deployment(),
                NonZeroU64::new(9).unwrap(),
            ));
        assert_eq!(
            application
                .get_product_status("opaque-session-token", &installation(), query)
                .await
                .unwrap(),
            ProductStatusV1::Live
        );
        assert_eq!(
            events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| **event == "deployment_status")
                .count(),
            2
        );
    });
}

#[test]
fn apply_passes_no_attempt_identifier_and_reports_runtime_pending() {
    block_on(async {
        let (events, authentication, authority, decisions, deployments) = product_fixture(
            ProductDecisionPhaseV1::Approved,
            DeploymentStatusProjectionV1::NotRequested,
        );
        let result =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments)
                .apply(
                    "opaque-session-token",
                    "csrf-proof",
                    &product_request_id(),
                    &installation(),
                    ApplyProductPromotionV1 {
                        promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
                        expected_payload_digest: ApprovalPayloadDigestV1::parse(&"c".repeat(64))
                            .unwrap(),
                        expected_revision: ProductRevisionV1::new(3).unwrap(),
                        idempotency_key: ProductIdempotencyKeyV1::parse("apply-key").unwrap(),
                    },
                )
                .await
                .unwrap();
        assert_eq!(result.status(), ProductStatusV1::RuntimePending);
        assert!(!result.exact_replay());
        assert_eq!(result.exact_deployment(), &exact_deployment());
        assert_eq!(
            *events.lock().unwrap(),
            vec!["authenticate_mutation", "authorize", "apply_idempotent"]
        );
    });
}

#[test]
fn apply_surfaces_terminal_supersession_without_querying_runtime() {
    block_on(async {
        let (events, authentication, authority, decisions, deployments) = product_fixture(
            ProductDecisionPhaseV1::Approved,
            DeploymentStatusProjectionV1::NotRequested,
        );
        *decisions.apply_phase.lock().unwrap() = ProductDecisionPhaseV1::Superseded;
        let error =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments)
                .apply(
                    "opaque-session-token",
                    "csrf-proof",
                    &product_request_id(),
                    &installation(),
                    ApplyProductPromotionV1 {
                        promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
                        expected_payload_digest: ApprovalPayloadDigestV1::parse(&"c".repeat(64))
                            .unwrap(),
                        expected_revision: ProductRevisionV1::new(3).unwrap(),
                        idempotency_key: ProductIdempotencyKeyV1::parse("apply-key").unwrap(),
                    },
                )
                .await
                .unwrap_err();
        assert_eq!(
            error,
            ProductApplicationError::Control(ProductControlPortError::Superseded)
        );
        assert_eq!(
            *events.lock().unwrap(),
            vec!["authenticate_mutation", "authorize", "apply_idempotent"]
        );
    });
}

#[test]
fn exact_apply_replay_may_project_live_after_runtime_verification() {
    block_on(async {
        let (events, authentication, authority, decisions, deployments) = product_fixture(
            ProductDecisionPhaseV1::Approved,
            DeploymentStatusProjectionV1::ExactLive(ExactLiveProjectionV1::from_exact_attestation(
                exact_deployment(),
                NonZeroU64::new(7).unwrap(),
            )),
        );
        *decisions.apply_exact_replay.lock().unwrap() = true;
        let result =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments)
                .apply(
                    "opaque-session-token",
                    "csrf-proof",
                    &product_request_id(),
                    &installation(),
                    ApplyProductPromotionV1 {
                        promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
                        expected_payload_digest: ApprovalPayloadDigestV1::parse(&"c".repeat(64))
                            .unwrap(),
                        expected_revision: ProductRevisionV1::new(3).unwrap(),
                        idempotency_key: ProductIdempotencyKeyV1::parse("apply-key").unwrap(),
                    },
                )
                .await
                .unwrap();
        assert_eq!(result.status(), ProductStatusV1::Live);
        assert!(result.exact_replay());
        assert_eq!(result.exact_deployment(), &exact_deployment());
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                "authenticate_mutation",
                "authorize",
                "apply_idempotent",
                "deployment_status"
            ]
        );
    });
}

#[test]
fn mismatched_exact_live_projection_is_rejected() {
    block_on(async {
        let wrong = ExactDeploymentSelectorV1::from_server_projection(
            AutomationInstallationId::parse("installation-2").unwrap(),
            promotion_id(),
            "deployment-wrong",
            "b".repeat(64),
        )
        .unwrap();
        let (_, authentication, authority, decisions, deployments) = product_fixture(
            ProductDecisionPhaseV1::Applied {
                exact_deployment: exact_deployment(),
            },
            DeploymentStatusProjectionV1::ExactLive(ExactLiveProjectionV1::from_exact_attestation(
                wrong,
                NonZeroU64::new(2).unwrap(),
            )),
        );
        let result =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments)
                .get_product_status(
                    "opaque-session-token",
                    &installation(),
                    ProductStatusQueryV1 {
                        promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
                    },
                )
                .await;
        assert_eq!(
            result.unwrap_err(),
            ProductApplicationError::InvalidProjection
        );
    });
}

#[test]
fn deployment_status_requires_the_server_derived_exact_target() {
    block_on(async {
        let (_, authentication, authority, decisions, deployments) = product_fixture(
            ProductDecisionPhaseV1::Applied {
                exact_deployment: exact_deployment(),
            },
            DeploymentStatusProjectionV1::Pending,
        );
        let result =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments)
                .get_deployment_status(
                    "opaque-session-token",
                    &installation(),
                    RuntimeDeploymentQueryV1 {
                        promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
                    },
                )
                .await
                .unwrap();
        assert_eq!(result, authoring_application::DeploymentStatusV1::Pending);
    });
}

#[test]
fn deployment_failure_code_is_closed_and_cannot_leak_backend_text() {
    for failure_code in [
        "Discord said no: secret detail",
        "",
        &"a".repeat(65),
        "private_internal_identifier",
    ] {
        assert!(DeploymentFailureCodeV1::parse(failure_code).is_err());
    }
    for failure_code in [
        DeploymentFailureCodeV1::RuntimeEnvironmentUnavailable,
        DeploymentFailureCodeV1::ActivationNotObservable,
        DeploymentFailureCodeV1::PanelReconciliationFailed,
        DeploymentFailureCodeV1::GatewayStartFailed,
        DeploymentFailureCodeV1::GatewayReadyTimeout,
        DeploymentFailureCodeV1::RuntimeInvariantViolation,
        DeploymentFailureCodeV1::DeploymentBlocked,
        DeploymentFailureCodeV1::ActiveTargetChanged,
        DeploymentFailureCodeV1::BindingAuthorityChanged,
        DeploymentFailureCodeV1::ProductAuthorityInactive,
        DeploymentFailureCodeV1::ProductAuthorityNotCurrent,
        DeploymentFailureCodeV1::DeploymentSuperseded,
        DeploymentFailureCodeV1::DeploymentCancelled,
    ] {
        assert_eq!(
            DeploymentFailureCodeV1::parse(failure_code.as_str()).unwrap(),
            failure_code
        );
    }
    block_on(async {
        let (_, authentication, authority, decisions, deployments) = product_fixture(
            ProductDecisionPhaseV1::Applied {
                exact_deployment: exact_deployment(),
            },
            DeploymentStatusProjectionV1::Failed {
                retryable: true,
                failure_code: "private_internal_identifier".to_string(),
            },
        );
        let error =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments)
                .get_deployment_status(
                    "opaque-session-token",
                    &installation(),
                    RuntimeDeploymentQueryV1 {
                        promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
                    },
                )
                .await
                .unwrap_err();
        assert_eq!(error, ProductApplicationError::InvalidProjection);
    });
}

#[test]
fn product_candidate_failures_have_bounded_stable_codes() {
    let cases = [
        (
            ProductCandidateErrorCodeV1::TargetCorrupt,
            "product target artifact is corrupt",
        ),
        (
            ProductCandidateErrorCodeV1::BindingRevisionUnavailable,
            "authoritative product binding revision is unavailable",
        ),
        (
            ProductCandidateErrorCodeV1::UnsupportedSchema,
            "product target schema is unsupported",
        ),
        (
            ProductCandidateErrorCodeV1::StructurallyInvalid,
            "product target structure is invalid",
        ),
        (
            ProductCandidateErrorCodeV1::HashComputationFailed,
            "product target hash could not be verified",
        ),
        (
            ProductCandidateErrorCodeV1::HashMismatch,
            "product target hash does not match its content",
        ),
        (
            ProductCandidateErrorCodeV1::BindingInvalid,
            "product target bindings are invalid",
        ),
        (
            ProductCandidateErrorCodeV1::BlockingPolicy,
            "product target violates a blocking policy",
        ),
        (
            ProductCandidateErrorCodeV1::MissingCapabilities,
            "product target requires unavailable capabilities",
        ),
        (
            ProductCandidateErrorCodeV1::RoleHierarchyUnavailable,
            "product target role hierarchy evidence is unavailable",
        ),
        (
            ProductCandidateErrorCodeV1::RoleHierarchyIncomplete,
            "product target role hierarchy evidence is incomplete",
        ),
        (
            ProductCandidateErrorCodeV1::RoleUnmanageable,
            "product target requires a role the bot cannot manage",
        ),
    ];
    for (code, message) in cases {
        assert_eq!(code.to_string(), message);
        assert_eq!(
            ProductControlPortError::InvalidServerCandidate(code).to_string(),
            format!("server-owned product candidate is invalid: {message}")
        );
    }
    assert_eq!(
        ProductControlPortError::Superseded.to_string(),
        "promotion was superseded by newer server state"
    );
}
