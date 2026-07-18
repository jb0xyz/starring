use std::collections::VecDeque;
use std::num::{NonZeroU32, NonZeroU64};
use std::sync::{Arc, Mutex};

use authoring_application::{
    ApplyProductPromotionV1, ApprovalPayloadDigestV1, ApproveProductPromotionV1,
    AuthenticatedActorV1, AuthenticatedSessionFingerprintV1, AuthenticationClaimsV1,
    AuthenticationError, AuthenticationPort, AuthoringApplication, AuthoringApplicationError,
    AuthorizedApplyProductV1, AuthorizedApprovalPreviewV1, AuthorizedApproveProductV1,
    AuthorizedDeploymentStatusV1, AuthorizedInstallationScopeV1, AuthorizedInstallationV1,
    AuthorizedProductStatusV1, AuthorizedPromotionSnapshotError, AuthorizedPromotionSnapshotPort,
    AuthorizedPromotionSnapshotV1, AuthorizedRejectProductV1, CapabilityV1, DeploymentStatusPort,
    DeploymentStatusPortError, DeploymentStatusProjectionV1, ExactDeploymentSelectorV1,
    ExactLiveProjectionV1, FreshGuildAuthorityError, FreshGuildAuthorityPort,
    InstallationSelectorV1, MutationAuthenticationPort, ProductApplicationError, ProductApplyPort,
    ProductApprovalPort, ProductApprovalPreviewV1, ProductControlApplication,
    ProductControlPortError, ProductDecisionPhaseV1, ProductDecisionPort,
    ProductDecisionProjectionV1, ProductDecisionQueryPort, ProductIdempotencyKeyV1,
    ProductMutationReceiptV1, ProductRejectionPort, ProductRequestIdError, ProductRequestIdV1,
    ProductRevisionV1, ProductStatusQueryV1, ProductStatusV1, PromoteOwnedSessionV1,
    PromotionSubmissionPort, RejectProductPromotionV1, RejectionReasonError, RejectionReasonV1,
    ResolvedPromotionAuthorityV1, RuntimeDeploymentQueryV1,
};
use authoring_promotion::{
    ApprovalPolicyV1, AuthoringSessionId, AutomationInstallationId, BindingRevision,
    IdempotencyKey, PolicyRevision, PrincipalId, PromotionError, PromotionId, SessionGeneration,
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

struct PromotionCapture {
    events: Arc<Mutex<Vec<&'static str>>>,
    input: Mutex<Option<StartPromotionV1>>,
}

impl PromotionSubmissionPort for PromotionCapture {
    type Output = ();

    async fn submit_verified_promotion(
        &self,
        input: StartPromotionV1,
    ) -> Result<Self::Output, PromotionError> {
        self.events.lock().unwrap().push("submit");
        *self.input.lock().unwrap() = Some(input);
        Ok(())
    }
}

fn installation() -> InstallationSelectorV1 {
    InstallationSelectorV1::new(AutomationInstallationId::parse("installation-2").unwrap())
}

fn promote_command() -> PromoteOwnedSessionV1 {
    PromoteOwnedSessionV1 {
        idempotency_key: IdempotencyKey::parse("promotion-key").unwrap(),
        session_id: AuthoringSessionId::parse("session-1").unwrap(),
        expected_generation: SessionGeneration::new(7).unwrap(),
    }
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
            input: Mutex::new(None),
        };
        AuthoringApplication::new(&authentication, &authority, &snapshots, &promotions)
            .promote_owned_session(
                "opaque-session-token",
                "csrf-proof",
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
                "atomic_snapshot",
                "submit"
            ]
        );
        let captured = promotions.input.lock().unwrap().take().unwrap();
        assert_eq!(captured.context.tenant_id.as_str(), "tenant-1");
        assert_eq!(captured.context.principal_id.as_str(), "principal-1");
        assert_eq!(captured.context.installation_id.as_str(), "installation-2");
        assert_eq!(captured.context.guild_id, GuildId(900));
        assert_eq!(captured.context.requester, UserId(200));
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
                input: Mutex::new(None),
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
                &installation(),
                promote_command(),
            )
            .await
            .is_err());
            assert_eq!(*events.lock().unwrap(), expected);
            assert!(promotions.input.lock().unwrap().is_none());
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
            input: Mutex::new(None),
        };
        let error = AuthoringApplication::new(&authentication, &authority, &snapshots, &promotions)
            .promote_owned_session(
                "opaque-session-token",
                "wrong-csrf",
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
        assert!(promotions.input.lock().unwrap().is_none());
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
            decision_projection(ProductDecisionPhaseV1::Applied {
                exact_deployment: exact_deployment(),
            }),
            false,
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
        },
        Deployments {
            events: events.clone(),
            status: Mutex::new(deployment),
        },
    )
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
        let status =
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
        assert_eq!(status, ProductStatusV1::RuntimePending);
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
fn deployment_failure_code_is_bounded_and_cannot_leak_backend_text() {
    block_on(async {
        for failure_code in ["Discord said no: secret detail", "", &"a".repeat(65)] {
            let (_, authentication, authority, decisions, deployments) = product_fixture(
                ProductDecisionPhaseV1::Applied {
                    exact_deployment: exact_deployment(),
                },
                DeploymentStatusProjectionV1::Failed {
                    retryable: true,
                    failure_code: failure_code.to_string(),
                },
            );
            let result = ProductControlApplication::new(
                &authentication,
                &authority,
                &decisions,
                &deployments,
            )
            .get_deployment_status(
                "opaque-session-token",
                &installation(),
                RuntimeDeploymentQueryV1 {
                    promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
                },
            )
            .await;
            assert_eq!(
                result.unwrap_err(),
                ProductApplicationError::InvalidProjection
            );
        }
    });
}
