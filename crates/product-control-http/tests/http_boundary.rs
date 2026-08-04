use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{DateTime, Utc};
use http_body_util::BodyExt;
use product_control_http::{
    product_control_router, product_control_router_with_authoring_v1,
    product_control_router_with_operational_v2,
    product_control_router_with_operational_v2_and_lifecycle_v1,
    product_control_router_with_operational_v2_and_readiness_gate, ApplyCommand, ApplyView,
    ApprovalPreviewView, AuthoringHttpBoundaryConfigV1, AuthoringSessionViewV1,
    AuthoringTurnCommandV1, AuthoringTurnDispositionV1, AuthoringTurnViewV1, CsrfSecret,
    CurrentPrincipal, CurrentPrincipalView, DecisionCommand, DecisionView,
    DeploymentAttestationViewV2, DeploymentFailureViewV2, DeploymentOperationalStateV2,
    DeploymentOperationalViewV2, DeploymentOperatorActionV2, DeploymentRetryStateV2,
    DeploymentRetryViewV2, DeploymentRuntimePhaseV2, DeploymentServingFreshnessStateV2,
    DeploymentServingFreshnessViewV2, DeploymentState, DeploymentView, DiscordAuthorizationRequest,
    FacadeError, FacadeErrorCode, HttpBoundaryConfig, IdempotencyKey, LifecycleCancellationCommand,
    LifecycleCancellationView, OAuthCallbackCommand, OAuthCallbackResult, OAuthStartCommand,
    OAuthStartResult, ProductApiReadinessGate, ProductControlAuthoringFacadeV1,
    ProductControlFacade, ProductControlLifecycleFacadeV1, ProductControlOperationalFacadeV2,
    ProductRequestId, ProductState, ProductStatusView, PromoteCommand, PromotionView,
    RejectCommand, RuntimeDeploymentOperationalViewV2, SafeApprovalSummary, SessionCredential,
};
use tokio::sync::Notify;
use tower::ServiceExt;

const HOST: &str = "starring.example";
const ORIGIN: &str = "https://starring.example";
const SESSION: &str = "sssssssssssssssssssssssssssssssssssssssssss";
const CSRF: &str = "ccccccccccccccccccccccccccccccccccccccccccc";
const OTHER_CSRF: &str = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxw";
const NONCE: &str = "nnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnng";
const STATE: &str = "ttttttttttttttttttttttttttttttttttttttttttg";
const PROMOTION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[derive(Default)]
struct FakeFacade {
    oauth_start_calls: AtomicUsize,
    fail_oauth_start: AtomicUsize,
    verify_calls: AtomicUsize,
    readiness_calls: AtomicUsize,
    promote_calls: AtomicUsize,
    authority_check_calls: AtomicUsize,
    authority_check_failure: AtomicUsize,
    authority_check_installations: Mutex<Vec<String>>,
    fail_me: AtomicUsize,
    panic_me: AtomicUsize,
    disallowed_return: AtomicUsize,
    invalid_client_id: AtomicUsize,
    invalid_callback_url: AtomicUsize,
    identical_session_csrf: AtomicUsize,
    approval_response: AtomicUsize,
    apply_response: AtomicUsize,
    status_response: AtomicUsize,
    block_promote: AtomicUsize,
    promote_entered: Notify,
    promote_release: Notify,
    mutation_request_ids: Mutex<Vec<(String, String)>>,
    operational_calls: AtomicUsize,
    operational_fail: AtomicUsize,
    block_operational: AtomicUsize,
    operational_entered: Notify,
    operational_release: Notify,
    operational_response: Mutex<Option<DeploymentOperationalViewV2>>,
    authoring_turn_calls: AtomicUsize,
    authoring_read_calls: AtomicUsize,
    authoring_worker_calls: AtomicUsize,
    authoring_completed_calls: AtomicUsize,
    authoring_commit_phase_calls: AtomicUsize,
    authoring_mode: AtomicUsize,
    authoring_read_mode: AtomicUsize,
    authoring_delay_millis: AtomicUsize,
    authoring_entered: Notify,
    authoring_messages: Mutex<Vec<String>>,
}

impl FakeFacade {
    fn verify_mutation_inputs(
        &self,
        credential: &SessionCredential,
        csrf: &CsrfSecret,
    ) -> Result<(), FacadeError> {
        self.verify_calls.fetch_add(1, Ordering::SeqCst);
        if credential.expose_secret() == SESSION && csrf.expose_secret() == CSRF {
            Ok(())
        } else {
            Err(FacadeError::new(FacadeErrorCode::Forbidden))
        }
    }

    fn promotion(&self, replayed: bool) -> PromotionView {
        PromotionView {
            installation_id: "install-1".to_string(),
            promotion_id: PROMOTION.to_string(),
            revision: 3,
            state: ProductState::PendingApproval,
            payload_digest: DIGEST.to_string(),
            replayed,
        }
    }

    fn decision(&self, state: ProductState) -> DecisionView {
        DecisionView {
            installation_id: "install-1".to_string(),
            promotion_id: PROMOTION.to_string(),
            revision: 4,
            state,
            replayed: false,
        }
    }

    fn product_status(&self, state: ProductState) -> ProductStatusView {
        let apply_source_revision = match state {
            ProductState::Applying => Some(3),
            ProductState::RuntimePending | ProductState::Live => Some(2),
            _ => None,
        };
        ProductStatusView {
            installation_id: "install-1".to_string(),
            promotion_id: PROMOTION.to_string(),
            revision: 4,
            state,
            payload_digest: DIGEST.to_string(),
            apply_source_revision,
            replayed: false,
        }
    }

    fn record_request_id(&self, mutation: &str, request_id: &ProductRequestId) {
        self.mutation_request_ids
            .lock()
            .unwrap()
            .push((mutation.to_string(), request_id.as_str().to_string()));
    }
}

fn discussion_projection() -> authoring_application::SafeAuthoringTurnProjectionV1 {
    authoring_application::SafeAuthoringTurnProjectionV1::from_canonical_json(
        r#"{"schema_version":1,"state":"discussion","assistant_message":"계속 설계해 볼까요?","capabilities":[],"draft":{"panels":0,"modals":0,"rules":0,"actions":0,"unresolved_references":[]},"preview":null}"#
            .as_bytes(),
    )
    .unwrap()
}

fn unsupported_projection() -> authoring_application::SafeAuthoringTurnProjectionV1 {
    authoring_application::SafeAuthoringTurnProjectionV1::from_canonical_json(
        r#"{"schema_version":1,"state":"unsupported","assistant_message":"지원하지 않는 요청입니다.","capabilities":[],"draft":{"panels":0,"modals":0,"rules":0,"actions":0,"unresolved_references":[]},"preview":null}"#
            .as_bytes(),
    )
    .unwrap()
}

fn preview_ready_projection(
    ruleset: &str,
    candidate_ruleset_hash: &str,
) -> authoring_application::SafeAuthoringTurnProjectionV1 {
    let projection = format!(
        r#"{{"schema_version":1,"state":"preview_ready","assistant_message":"Preview ready","capabilities":[],"draft":{{"panels":1,"modals":0,"rules":1,"actions":1,"unresolved_references":[]}},"preview":{{"revision":1,"draft":{{"panels":1,"modals":0,"rules":1,"actions":1,"unresolved_references":[]}},"ruleset":{ruleset},"receipt":{{"identity_revision":1,"intent_revision":1,"candidate_revision":1,"request_evidence_hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","request_evidence_entries":1,"compiler_input_hash":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","semantic_intent_hash":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","compiled_plan_hash":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","candidate_ruleset_hash":"{candidate_ruleset_hash}","candidate_draft_hash":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","compiled_operations":1}}}}}}"#
    );
    authoring_application::SafeAuthoringTurnProjectionV1::from_canonical_json(projection.as_bytes())
        .unwrap()
}

fn valid_preview_ready_projection() -> authoring_application::SafeAuthoringTurnProjectionV1 {
    preview_ready_projection(
        r#"{"modals":[],"panels":[{"buttons":[{"label":"Welcome","route":{"static":{"key":"welcome"}}}],"channel":"welcome_channel","content":"Choose a welcome","key":"welcome_panel"}],"rules":[{"actions":[{"content":"Welcome!","type":"respond_ephemeral"}],"key":"welcome_rule","trigger":{"component":"welcome","type":"button_click"}}],"version":1}"#,
        "f283047e6367d67067822a399200ffd2ea6c1a6940969e0ab9abd399cb43d537",
    )
}

#[async_trait]
impl ProductControlFacade for FakeFacade {
    async fn oauth_start(
        &self,
        _command: OAuthStartCommand,
    ) -> Result<OAuthStartResult, FacadeError> {
        self.oauth_start_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_oauth_start.load(Ordering::SeqCst) != 0 {
            return Err(FacadeError::new(FacadeErrorCode::Internal));
        }
        let client_id = if self.invalid_client_id.load(Ordering::SeqCst) == 0 {
            "123456789012345678"
        } else {
            "0"
        };
        Ok(OAuthStartResult {
            authorization_request: DiscordAuthorizationRequest {
                client_id: client_id.to_string(),
                callback_url: if self.invalid_callback_url.load(Ordering::SeqCst) == 0 {
                    "https://starring.example/oauth/discord/callback".to_string()
                } else {
                    "https://attacker.example/oauth/discord/callback".to_string()
                },
            },
            authorization_state: product_control_http::OAuthState::parse(STATE).unwrap(),
            browser_nonce: product_control_http::OAuthState::parse(NONCE).unwrap(),
            max_age_seconds: 600,
        })
    }

    async fn oauth_callback(
        &self,
        _command: OAuthCallbackCommand,
    ) -> Result<OAuthCallbackResult, FacadeError> {
        Ok(OAuthCallbackResult {
            session: SessionCredential::parse(SESSION).unwrap(),
            csrf: CsrfSecret::parse(if self.identical_session_csrf.load(Ordering::SeqCst) == 0 {
                CSRF
            } else {
                SESSION
            })
            .unwrap(),
            return_to: if self.disallowed_return.load(Ordering::SeqCst) == 0 {
                "/app".to_string()
            } else {
                "/admin".to_string()
            },
            max_age_seconds: 43_200,
        })
    }

    async fn current_principal(
        &self,
        credential: &SessionCredential,
    ) -> Result<CurrentPrincipal, FacadeError> {
        assert_eq!(self.panic_me.load(Ordering::SeqCst), 0);
        if self.fail_me.load(Ordering::SeqCst) > 0 {
            return Err(FacadeError::new(FacadeErrorCode::Internal));
        }
        if credential.expose_secret() != SESSION {
            return Err(FacadeError::new(FacadeErrorCode::AuthenticationRequired));
        }
        Ok(CurrentPrincipal {
            principal_id: "principal-1".to_string(),
            display_name: "Manager".to_string(),
        })
    }

    async fn authority_check(
        &self,
        credential: &SessionCredential,
        installation_id: &str,
    ) -> Result<(), FacadeError> {
        if credential.expose_secret() != SESSION {
            return Err(FacadeError::new(FacadeErrorCode::AuthenticationRequired));
        }
        self.authority_check_calls.fetch_add(1, Ordering::SeqCst);
        self.authority_check_installations
            .lock()
            .unwrap()
            .push(installation_id.to_string());
        match self.authority_check_failure.load(Ordering::SeqCst) {
            1 => Err(FacadeError::new(FacadeErrorCode::NotFound)),
            2 => Err(FacadeError::new(FacadeErrorCode::DependencyTimeout)),
            3 => Err(FacadeError::new(FacadeErrorCode::DependencyUnavailable)),
            _ => Ok(()),
        }
    }

    async fn revoke_session(
        &self,
        credential: &SessionCredential,
        csrf: &CsrfSecret,
    ) -> Result<(), FacadeError> {
        if credential.expose_secret() == csrf.expose_secret() {
            return Ok(());
        }
        self.verify_mutation_inputs(credential, csrf)
    }

    async fn promote(
        &self,
        credential: &SessionCredential,
        csrf: &CsrfSecret,
        command: PromoteCommand,
    ) -> Result<PromotionView, FacadeError> {
        self.verify_mutation_inputs(credential, csrf)?;
        self.record_request_id("promote", &command.request_id);
        self.promote_calls.fetch_add(1, Ordering::SeqCst);
        if self.block_promote.load(Ordering::SeqCst) != 0 {
            self.promote_entered.notify_one();
            self.promote_release.notified().await;
        }
        Ok(self.promotion(false))
    }

    async fn status(
        &self,
        _credential: &SessionCredential,
        _installation_id: &str,
        _promotion_id: &str,
    ) -> Result<ProductStatusView, FacadeError> {
        let mode = self.status_response.load(Ordering::SeqCst);
        let mut view = self.product_status(if mode == 0 {
            ProductState::Approved
        } else {
            ProductState::RuntimePending
        });
        match mode {
            2 => {
                view.revision = 2;
                view.apply_source_revision = Some(0);
            }
            3 => view.apply_source_revision = Some(1),
            4 => view.payload_digest = "not-a-digest".to_string(),
            _ => {}
        }
        Ok(view)
    }

    async fn approval_preview(
        &self,
        _credential: &SessionCredential,
        _installation_id: &str,
        _promotion_id: &str,
    ) -> Result<ApprovalPreviewView, FacadeError> {
        Ok(ApprovalPreviewView {
            installation_id: "install-1".to_string(),
            promotion_id: PROMOTION.to_string(),
            revision: 3,
            state: ProductState::PendingApproval,
            payload_digest: DIGEST.to_string(),
            summary: SafeApprovalSummary {
                panels: 1,
                modals: 1,
                rules: 2,
                actions: 11,
                target_version: 7,
                target_content_hash: DIGEST.to_string(),
                binding_fingerprint: DIGEST.to_string(),
                required_approvals: 1,
                expires_at: DateTime::parse_from_rfc3339("2026-07-20T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            },
        })
    }

    async fn approve(
        &self,
        credential: &SessionCredential,
        csrf: &CsrfSecret,
        command: DecisionCommand,
    ) -> Result<DecisionView, FacadeError> {
        self.verify_mutation_inputs(credential, csrf)?;
        self.record_request_id("approve", &command.request_id);
        let state = match self.approval_response.load(Ordering::SeqCst) {
            0 => ProductState::Approved,
            1 => ProductState::PendingApproval,
            _ => ProductState::Rejected,
        };
        Ok(self.decision(state))
    }

    async fn reject(
        &self,
        credential: &SessionCredential,
        csrf: &CsrfSecret,
        command: RejectCommand,
    ) -> Result<DecisionView, FacadeError> {
        self.verify_mutation_inputs(credential, csrf)?;
        self.record_request_id("reject", &command.decision.request_id);
        Ok(self.decision(ProductState::Rejected))
    }

    async fn apply(
        &self,
        credential: &SessionCredential,
        csrf: &CsrfSecret,
        command: ApplyCommand,
    ) -> Result<ApplyView, FacadeError> {
        self.verify_mutation_inputs(credential, csrf)?;
        self.record_request_id("apply", &command.decision.request_id);
        match self.apply_response.load(Ordering::SeqCst) {
            1 => return Err(FacadeError::new(FacadeErrorCode::RuntimeDrainRequired)),
            2 => return Err(FacadeError::new(FacadeErrorCode::RuntimeDrainPending)),
            _ => {}
        }
        Ok(ApplyView {
            installation_id: "install-1".to_string(),
            promotion_id: PROMOTION.to_string(),
            state: ProductState::RuntimePending,
            replayed: false,
        })
    }

    async fn deployment(
        &self,
        _credential: &SessionCredential,
        _installation_id: &str,
        _promotion_id: &str,
    ) -> Result<DeploymentView, FacadeError> {
        Ok(DeploymentView {
            installation_id: "install-1".to_string(),
            promotion_id: PROMOTION.to_string(),
            observed_at: DateTime::parse_from_rfc3339("2026-07-19T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            state: DeploymentState::Pending,
            retryable: true,
            failure_code: None,
            attestation_revision: None,
            last_serving_heartbeat: None,
            serving_lease_expires_at: None,
        })
    }

    async fn readiness(&self) -> Result<(), FacadeError> {
        self.readiness_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn product_apply_exposes_retryable_runtime_drain_conflicts_without_internal_selectors() {
    for (mode, code) in [(1, "runtime_drain_required"), (2, "runtime_drain_pending")] {
        let facade = Arc::new(FakeFacade::default());
        facade.apply_response.store(mode, Ordering::SeqCst);
        let uri = format!("/v1/installations/install-1/promotions/{PROMOTION}/apply");
        let request = request_builder("POST", &uri)
            .header("content-type", "application/json")
            .header("origin", ORIGIN)
            .header(
                "cookie",
                format!("__Host-starring_session={SESSION}; __Host-starring_csrf={CSRF}"),
            )
            .header("x-csrf-token", CSRF)
            .header("idempotency-key", format!("apply-{mode}"))
            .body(Body::from(format!(
                "{{\"expected_payload_digest\":\"{DIGEST}\",\"expected_revision\":3}}"
            )))
            .unwrap();
        let response = app(facade).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            response.headers()["content-type"],
            "application/problem+json"
        );
        let body = body_text(response).await;
        assert!(body.contains(&format!("\"code\":\"{code}\"")));
        assert!(body.contains("\"retryable\":true"));
        assert!(body.contains("\"request_id\":\"test-request-1\""));
        assert!(!body.contains("drain_intent_id"));
        assert!(!body.contains("product_operation_id"));
        assert!(!body.contains("state_digest"));
    }
}

#[async_trait]
impl ProductControlAuthoringFacadeV1 for FakeFacade {
    async fn authoring_turn(
        &self,
        credential: &SessionCredential,
        csrf: &CsrfSecret,
        command: AuthoringTurnCommandV1,
    ) -> Result<AuthoringTurnViewV1, FacadeError> {
        self.verify_mutation_inputs(credential, csrf)?;
        self.record_request_id("authoring_turn", &command.request_id);
        self.authoring_turn_calls.fetch_add(1, Ordering::SeqCst);
        self.authoring_entered.notify_one();
        self.authoring_messages
            .lock()
            .unwrap()
            .push(command.message.as_str().to_string());
        let mode = self.authoring_mode.load(Ordering::SeqCst);
        if mode == 3 {
            return Err(FacadeError::new(FacadeErrorCode::AuthoringSaturated));
        }
        if mode == 7 {
            return Err(FacadeError::new(FacadeErrorCode::DependencyUnavailable));
        }
        if mode != 1 {
            self.authoring_worker_calls.fetch_add(1, Ordering::SeqCst);
        }
        if mode == 6 {
            assert!(command.commit_boundary.enter_commit_phase());
            self.authoring_commit_phase_calls
                .fetch_add(1, Ordering::SeqCst);
        }
        let delay = self.authoring_delay_millis.load(Ordering::SeqCst);
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay as u64)).await;
        }
        self.authoring_completed_calls
            .fetch_add(1, Ordering::SeqCst);
        let expected_generation = command.expected_generation.get();
        let session_id = if mode == 4 {
            "different-session".to_string()
        } else {
            command.session_id
        };
        let (generation, disposition, projection) = match mode {
            1 => (
                expected_generation.checked_add(1),
                Some(AuthoringTurnDispositionV1::ExactReplay),
                discussion_projection(),
            ),
            2 => (None, None, unsupported_projection()),
            5 => (
                expected_generation.checked_add(2),
                Some(AuthoringTurnDispositionV1::Created),
                discussion_projection(),
            ),
            8 => (
                expected_generation.checked_add(1),
                Some(AuthoringTurnDispositionV1::Created),
                valid_preview_ready_projection(),
            ),
            9 => (
                expected_generation.checked_add(1),
                Some(AuthoringTurnDispositionV1::Created),
                preview_ready_projection(
                    r#"{"malformed":true}"#,
                    "f283047e6367d67067822a399200ffd2ea6c1a6940969e0ab9abd399cb43d537",
                ),
            ),
            10 => (
                expected_generation.checked_add(1),
                Some(AuthoringTurnDispositionV1::Created),
                preview_ready_projection(
                    r#"{"modals":[],"panels":[{"buttons":[{"label":"Welcome","route":{"static":{"key":"welcome"}}}],"channel":"welcome_channel","content":"Choose a welcome","key":"welcome_panel"}],"rules":[{"actions":[{"content":"Welcome!","type":"respond_ephemeral"}],"key":"welcome_rule","trigger":{"component":"welcome","type":"button_click"}}],"version":1}"#,
                    "0000000000000000000000000000000000000000000000000000000000000000",
                ),
            ),
            _ => (
                expected_generation.checked_add(1),
                Some(AuthoringTurnDispositionV1::Created),
                discussion_projection(),
            ),
        };
        Ok(AuthoringTurnViewV1 {
            session_id,
            generation,
            disposition,
            projection,
        })
    }

    async fn authoring_session(
        &self,
        credential: &SessionCredential,
        _installation_id: &str,
        session_id: &str,
    ) -> Result<AuthoringSessionViewV1, FacadeError> {
        if credential.expose_secret() != SESSION {
            return Err(FacadeError::new(FacadeErrorCode::AuthenticationRequired));
        }
        self.authoring_read_calls.fetch_add(1, Ordering::SeqCst);
        match self.authoring_read_mode.load(Ordering::SeqCst) {
            1 => return Err(FacadeError::new(FacadeErrorCode::Forbidden)),
            2 => return Err(FacadeError::new(FacadeErrorCode::NotFound)),
            4 => return Err(FacadeError::new(FacadeErrorCode::InvalidState)),
            _ => {}
        }
        let projection = match self.authoring_read_mode.load(Ordering::SeqCst) {
            5 => valid_preview_ready_projection(),
            6 => preview_ready_projection(
                r#"{"malformed":true}"#,
                "f283047e6367d67067822a399200ffd2ea6c1a6940969e0ab9abd399cb43d537",
            ),
            7 => preview_ready_projection(
                r#"{"modals":[],"panels":[{"buttons":[{"label":"Welcome","route":{"static":{"key":"welcome"}}}],"channel":"welcome_channel","content":"Choose a welcome","key":"welcome_panel"}],"rules":[{"actions":[{"content":"Welcome!","type":"respond_ephemeral"}],"key":"welcome_rule","trigger":{"component":"welcome","type":"button_click"}}],"version":1}"#,
                "0000000000000000000000000000000000000000000000000000000000000000",
            ),
            _ => discussion_projection(),
        };
        Ok(AuthoringSessionViewV1 {
            session_id: if self.authoring_read_mode.load(Ordering::SeqCst) == 3 {
                "different-session".to_string()
            } else {
                session_id.to_string()
            },
            observed_generation: 3,
            projection,
        })
    }
}

#[async_trait]
impl ProductControlOperationalFacadeV2 for FakeFacade {
    async fn deployment_operational_v2(
        &self,
        _credential: &SessionCredential,
        _installation_id: &str,
        _promotion_id: &str,
    ) -> Result<DeploymentOperationalViewV2, FacadeError> {
        self.operational_calls.fetch_add(1, Ordering::SeqCst);
        match self.operational_fail.load(Ordering::SeqCst) {
            1 => return Err(FacadeError::new(FacadeErrorCode::DependencyUnavailable)),
            2 => return Err(FacadeError::new(FacadeErrorCode::DependencyTimeout)),
            _ => {}
        }
        if self.block_operational.load(Ordering::SeqCst) != 0 {
            self.operational_entered.notify_one();
            self.operational_release.notified().await;
        }
        Ok(self
            .operational_response
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(pending_operational_view))
    }
}

#[async_trait]
impl ProductControlLifecycleFacadeV1 for FakeFacade {
    async fn cancel_lifecycle(
        &self,
        credential: &SessionCredential,
        csrf: &CsrfSecret,
        command: LifecycleCancellationCommand,
    ) -> Result<LifecycleCancellationView, FacadeError> {
        self.verify_mutation_inputs(credential, csrf)?;
        self.record_request_id("cancel_lifecycle", &command.decision.request_id);
        Ok(LifecycleCancellationView {
            installation_id: command.decision.installation_id,
            promotion_id: command.decision.promotion_id,
            revision: command.decision.expected_revision,
            state: ProductState::Approved,
            drain_intent_id: command.drain_intent_id,
            source_intent_revision: command.acknowledged_intent_revision,
            terminal_intent_revision: command.acknowledged_intent_revision + 1,
            terminal_state_digest: "c".repeat(64),
            product_operation_id: command.product_operation_id,
            source_runtime_deployment_revision: command.expected_runtime_deployment_revision,
            resulting_runtime_deployment_revision: command.expected_runtime_deployment_revision + 1,
            source_slot_writer_epoch: 11,
            successor_slot_writer_epoch: 12,
            cancelled_at: timestamp("2026-07-28T00:00:00Z"),
            replayed: false,
        })
    }
}

fn app(facade: Arc<FakeFacade>) -> axum::Router {
    app_with_concurrency(facade, 8)
}

fn app_with_concurrency(facade: Arc<FakeFacade>, max_in_flight: usize) -> axum::Router {
    product_control_router(
        facade,
        HttpBoundaryConfig::new(
            ORIGIN,
            1_024,
            max_in_flight,
            Duration::from_secs(2),
            ["/app".to_string()],
        )
        .unwrap(),
    )
}

fn authoring_app(facade: Arc<FakeFacade>) -> axum::Router {
    authoring_app_with_timeouts(
        facade,
        Duration::from_secs(2),
        Duration::from_secs(1),
        Duration::from_secs(1),
        7,
    )
}

fn authoring_app_with_timeouts(
    facade: Arc<FakeFacade>,
    general_timeout: Duration,
    worker_call_timeout: Duration,
    coordination_timeout: Duration,
    retry_after_seconds: u64,
) -> axum::Router {
    authoring_app_with_limits(
        facade,
        general_timeout,
        worker_call_timeout,
        coordination_timeout,
        retry_after_seconds,
        8,
        64,
    )
}

fn authoring_app_with_limits(
    facade: Arc<FakeFacade>,
    general_timeout: Duration,
    worker_call_timeout: Duration,
    coordination_timeout: Duration,
    retry_after_seconds: u64,
    general_max_in_flight: usize,
    authoring_max_in_flight: usize,
) -> axum::Router {
    product_control_router_with_authoring_v1(
        facade,
        HttpBoundaryConfig::new(
            ORIGIN,
            64 * 1_024,
            general_max_in_flight,
            general_timeout,
            ["/app".to_string()],
        )
        .unwrap(),
        AuthoringHttpBoundaryConfigV1::new(
            worker_call_timeout,
            coordination_timeout,
            retry_after_seconds,
        )
        .unwrap()
        .with_max_in_flight(authoring_max_in_flight)
        .unwrap(),
    )
}

fn operational_app(facade: Arc<FakeFacade>) -> axum::Router {
    operational_app_with_timeout(facade, Duration::from_secs(2))
}

fn operational_app_with_timeout(
    facade: Arc<FakeFacade>,
    request_timeout: Duration,
) -> axum::Router {
    product_control_router_with_operational_v2(
        facade,
        HttpBoundaryConfig::new(ORIGIN, 1_024, 8, request_timeout, ["/app".to_string()]).unwrap(),
    )
}

fn operational_app_with_gate(
    facade: Arc<FakeFacade>,
    readiness_gate: ProductApiReadinessGate,
) -> axum::Router {
    product_control_router_with_operational_v2_and_readiness_gate(
        facade,
        HttpBoundaryConfig::new(
            ORIGIN,
            1_024,
            8,
            Duration::from_secs(2),
            ["/app".to_string()],
        )
        .unwrap(),
        readiness_gate,
    )
}

fn lifecycle_app(facade: Arc<FakeFacade>) -> axum::Router {
    product_control_router_with_operational_v2_and_lifecycle_v1(
        facade,
        HttpBoundaryConfig::new(
            ORIGIN,
            1_024,
            8,
            Duration::from_secs(2),
            ["/app".to_string()],
        )
        .unwrap(),
    )
}

fn timestamp(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}

fn not_expected_serving() -> DeploymentServingFreshnessViewV2 {
    DeploymentServingFreshnessViewV2 {
        state: DeploymentServingFreshnessStateV2::NotExpected,
        last_heartbeat_at: None,
        lease_expires_at: None,
    }
}

fn pending_operational_view() -> DeploymentOperationalViewV2 {
    DeploymentOperationalViewV2 {
        installation_id: "install-1".to_string(),
        promotion_id: PROMOTION.to_string(),
        decision_observed_at: timestamp("2026-07-19T11:59:59Z"),
        state: DeploymentOperationalStateV2::Pending,
        runtime: Some(RuntimeDeploymentOperationalViewV2 {
            observed_at: timestamp("2026-07-19T12:00:00Z"),
            phase: DeploymentRuntimePhaseV2::Requested,
            current_attempt: 0,
            last_failure_attempt: None,
            failure: None,
            retry: None,
            operator_action: None,
            attestation: None,
            serving: not_expected_serving(),
        }),
    }
}

fn set_operational_response(facade: &FakeFacade, view: DeploymentOperationalViewV2) {
    *facade.operational_response.lock().unwrap() = Some(view);
}

fn operational_request(installation_id: &str, promotion_id: &str) -> Request<Body> {
    request_builder(
        "GET",
        &format!("/v2/installations/{installation_id}/promotions/{promotion_id}/deployment"),
    )
    .header("cookie", format!("__Host-starring_session={SESSION}"))
    .body(Body::empty())
    .unwrap()
}

#[tokio::test]
async fn deployment_v1_wire_shape_is_frozen() {
    let response = app(Arc::new(FakeFacade::default()))
        .oneshot(
            request_builder(
                "GET",
                &format!("/v1/installations/install-1/promotions/{PROMOTION}/deployment"),
            )
            .header("cookie", format!("__Host-starring_session={SESSION}"))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_text(response).await,
        format!(
            "{{\"installation_id\":\"install-1\",\"promotion_id\":\"{PROMOTION}\",\"observed_at\":\"2026-07-19T12:00:00Z\",\"state\":\"pending\",\"retryable\":true,\"failure_code\":null,\"attestation_revision\":null,\"last_serving_heartbeat\":null,\"serving_lease_expires_at\":null}}"
        )
    );

    let response = app(Arc::new(FakeFacade::default()))
        .oneshot(operational_request("install-1", PROMOTION))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(body_text(response).await.contains("route_not_found"));
}

#[tokio::test]
async fn operational_v2_requires_a_session_and_rejects_invalid_paths_before_the_facade() {
    let facade = Arc::new(FakeFacade::default());
    let router = operational_app(Arc::clone(&facade));
    let missing_session = request_builder(
        "GET",
        &format!("/v2/installations/install-1/promotions/{PROMOTION}/deployment"),
    )
    .body(Body::empty())
    .unwrap();
    let response = router.clone().oneshot(missing_session).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(facade.operational_calls.load(Ordering::SeqCst), 0);

    for (installation_id, promotion_id) in [("invalid!", PROMOTION), ("install-1", "bad")] {
        let response = router
            .clone()
            .oneshot(operational_request(installation_id, promotion_id))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    assert_eq!(facade.operational_calls.load(Ordering::SeqCst), 0);

    let response = router
        .oneshot(operational_request("install-1", PROMOTION))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["x-request-id"], "test-request-1");
    assert_eq!(facade.operational_calls.load(Ordering::SeqCst), 1);
    let body = serde_json::from_str::<serde_json::Value>(&body_text(response).await).unwrap();
    assert!(body["runtime"]["last_failure_attempt"].is_null());
    assert!(body["runtime"]["failure"].is_null());
    assert!(body["runtime"]["retry"].is_null());
    assert!(body["runtime"]["operator_action"].is_null());
    assert!(body["runtime"]["attestation"].is_null());
    assert!(body["runtime"]["serving"]["last_heartbeat_at"].is_null());
    assert!(body["runtime"]["serving"]["lease_expires_at"].is_null());
}

#[tokio::test]
async fn operational_v2_preserves_retry_operator_authority_and_serving_states() {
    let facade = Arc::new(FakeFacade::default());
    let router = operational_app(Arc::clone(&facade));
    let mut retry_waiting = pending_operational_view();
    retry_waiting.state = DeploymentOperationalStateV2::Failed;
    let runtime = retry_waiting.runtime.as_mut().unwrap();
    runtime.phase = DeploymentRuntimePhaseV2::RetryWaiting;
    runtime.current_attempt = 2;
    runtime.last_failure_attempt = Some(2);
    runtime.failure = Some(DeploymentFailureViewV2 {
        retryable: true,
        code: "gateway_start_failed".to_string(),
    });
    runtime.retry = Some(DeploymentRetryViewV2 {
        state: DeploymentRetryStateV2::Waiting,
        failure_attempt: 2,
        retry_not_before: timestamp("2026-07-19T12:01:00Z"),
    });

    let mut retry_due = retry_waiting.clone();
    let runtime = retry_due.runtime.as_mut().unwrap();
    runtime.phase = DeploymentRuntimePhaseV2::RetryDue;
    runtime.retry = Some(DeploymentRetryViewV2 {
        state: DeploymentRetryStateV2::Due,
        failure_attempt: 2,
        retry_not_before: timestamp("2026-07-19T12:00:00Z"),
    });

    let mut operator_blocked = pending_operational_view();
    operator_blocked.state = DeploymentOperationalStateV2::Failed;
    let runtime = operator_blocked.runtime.as_mut().unwrap();
    runtime.phase = DeploymentRuntimePhaseV2::OperatorBlocked;
    runtime.current_attempt = 3;
    runtime.last_failure_attempt = Some(3);
    runtime.failure = Some(DeploymentFailureViewV2 {
        retryable: false,
        code: "runtime_invariant_violation".to_string(),
    });
    runtime.operator_action = Some(DeploymentOperatorActionV2::RecoverBlockedDeployment);

    let mut authority_blocked = pending_operational_view();
    authority_blocked.state = DeploymentOperationalStateV2::Failed;
    let runtime = authority_blocked.runtime.as_mut().unwrap();
    runtime.phase = DeploymentRuntimePhaseV2::AuthorityBlocked;
    runtime.failure = Some(DeploymentFailureViewV2 {
        retryable: false,
        code: "product_authority_inactive".to_string(),
    });
    runtime.operator_action = Some(DeploymentOperatorActionV2::RestoreProductAuthority);

    let mut lease_missing = pending_operational_view();
    let runtime = lease_missing.runtime.as_mut().unwrap();
    runtime.phase = DeploymentRuntimePhaseV2::Live;
    runtime.current_attempt = 2;
    runtime.attestation = Some(DeploymentAttestationViewV2 {
        deployment_revision: 7,
        convergence_attempt: 2,
        process_instance_id: "0123456789abcdef0123456789abcdef".to_string(),
    });
    runtime.serving.state = DeploymentServingFreshnessStateV2::LeaseMissing;

    let mut attestation_missing = lease_missing.clone();
    let runtime = attestation_missing.runtime.as_mut().unwrap();
    runtime.attestation = None;
    runtime.serving.state = DeploymentServingFreshnessStateV2::AttestationMissing;

    let mut identity_mismatch = lease_missing.clone();
    identity_mismatch.runtime.as_mut().unwrap().serving.state =
        DeploymentServingFreshnessStateV2::IdentityMismatch;

    let mut disconnected = lease_missing.clone();
    disconnected.runtime.as_mut().unwrap().serving = DeploymentServingFreshnessViewV2 {
        state: DeploymentServingFreshnessStateV2::Disconnected,
        last_heartbeat_at: Some(timestamp("2026-07-19T11:59:59Z")),
        lease_expires_at: Some(timestamp("2026-07-19T12:01:00Z")),
    };

    let mut expired = lease_missing.clone();
    expired.runtime.as_mut().unwrap().serving = DeploymentServingFreshnessViewV2 {
        state: DeploymentServingFreshnessStateV2::Expired,
        last_heartbeat_at: Some(timestamp("2026-07-19T11:59:58Z")),
        lease_expires_at: Some(timestamp("2026-07-19T11:59:59Z")),
    };

    let mut fresh = lease_missing.clone();
    fresh.state = DeploymentOperationalStateV2::Live;
    let runtime = fresh.runtime.as_mut().unwrap();
    runtime.serving = DeploymentServingFreshnessViewV2 {
        state: DeploymentServingFreshnessStateV2::Fresh,
        last_heartbeat_at: Some(timestamp("2026-07-19T11:59:59Z")),
        lease_expires_at: Some(timestamp("2026-07-19T12:01:00Z")),
    };

    let mut not_applicable = pending_operational_view();
    not_applicable.state = DeploymentOperationalStateV2::NotApplicable;
    not_applicable.runtime = None;

    let cases = [
        (retry_waiting, "failed", "retry_waiting", "not_expected"),
        (retry_due, "failed", "retry_due", "not_expected"),
        (
            operator_blocked,
            "failed",
            "operator_blocked",
            "not_expected",
        ),
        (
            authority_blocked,
            "failed",
            "authority_blocked",
            "not_expected",
        ),
        (
            attestation_missing,
            "pending",
            "live",
            "attestation_missing",
        ),
        (lease_missing, "pending", "live", "lease_missing"),
        (identity_mismatch, "pending", "live", "identity_mismatch"),
        (disconnected, "pending", "live", "disconnected"),
        (expired, "pending", "live", "expired"),
        (fresh, "live", "live", "fresh"),
    ];
    for (view, state, phase, serving) in cases {
        let process_instance_id = view
            .runtime
            .as_ref()
            .and_then(|runtime| runtime.attestation.as_ref())
            .map(|attestation| attestation.process_instance_id.clone());
        set_operational_response(&facade, view);
        let response = router
            .clone()
            .oneshot(operational_request("install-1", PROMOTION))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{phase}");
        let body = body_text(response).await;
        let json = serde_json::from_str::<serde_json::Value>(&body).unwrap();
        assert_eq!(json["state"], state);
        assert_eq!(json["runtime"]["phase"], phase);
        assert_eq!(json["runtime"]["serving"]["state"], serving);
        match process_instance_id {
            Some(process_instance_id) => assert_eq!(
                json["runtime"]["attestation"]["process_instance_id"],
                process_instance_id
            ),
            None => assert!(json["runtime"]["attestation"].is_null()),
        }
        for forbidden in [
            SESSION,
            CSRF,
            "controller_id",
            "fencing_token",
            "failure_id",
            "failure_message",
            "attestation_id",
            "runtime_build_revision",
            "sql",
        ] {
            assert!(!body.contains(forbidden), "leaked {forbidden}");
        }
    }

    set_operational_response(&facade, not_applicable);
    let response = router
        .oneshot(operational_request("install-1", PROMOTION))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = serde_json::from_str::<serde_json::Value>(&body_text(response).await).unwrap();
    assert_eq!(json["state"], "not_applicable");
    assert!(json["runtime"].is_null());
}

#[tokio::test]
async fn operational_v2_rejects_impossible_or_unbounded_facade_responses() {
    let facade = Arc::new(FakeFacade::default());
    let router = operational_app(Arc::clone(&facade));

    let mut wrong_identity = pending_operational_view();
    wrong_identity.installation_id = "another-installation".to_string();

    let mut private_failure = pending_operational_view();
    private_failure.state = DeploymentOperationalStateV2::Failed;
    let runtime = private_failure.runtime.as_mut().unwrap();
    runtime.phase = DeploymentRuntimePhaseV2::OperatorBlocked;
    runtime.current_attempt = 1;
    runtime.last_failure_attempt = Some(1);
    runtime.failure = Some(DeploymentFailureViewV2 {
        retryable: false,
        code: "private_controller_diagnostic".to_string(),
    });
    runtime.operator_action = Some(DeploymentOperatorActionV2::RecoverBlockedDeployment);

    let mut missing_authority_action = pending_operational_view();
    missing_authority_action.state = DeploymentOperationalStateV2::Failed;
    let runtime = missing_authority_action.runtime.as_mut().unwrap();
    runtime.phase = DeploymentRuntimePhaseV2::AuthorityBlocked;
    runtime.failure = Some(DeploymentFailureViewV2 {
        retryable: false,
        code: "product_authority_not_current".to_string(),
    });

    let mut false_live = pending_operational_view();
    false_live.state = DeploymentOperationalStateV2::Live;
    let runtime = false_live.runtime.as_mut().unwrap();
    runtime.phase = DeploymentRuntimePhaseV2::Live;
    runtime.current_attempt = 2;
    runtime.attestation = Some(DeploymentAttestationViewV2 {
        deployment_revision: 7,
        convergence_attempt: 2,
        process_instance_id: "0123456789abcdef0123456789abcdef".to_string(),
    });
    runtime.serving.state = DeploymentServingFreshnessStateV2::LeaseMissing;

    let mut malformed_freshness = pending_operational_view();
    let runtime = malformed_freshness.runtime.as_mut().unwrap();
    runtime.phase = DeploymentRuntimePhaseV2::Live;
    runtime.current_attempt = 2;
    runtime.attestation = Some(DeploymentAttestationViewV2 {
        deployment_revision: 7,
        convergence_attempt: 2,
        process_instance_id: "0123456789abcdef0123456789abcdef".to_string(),
    });
    runtime.serving = DeploymentServingFreshnessViewV2 {
        state: DeploymentServingFreshnessStateV2::Disconnected,
        last_heartbeat_at: None,
        lease_expires_at: Some(timestamp("2026-07-19T12:01:00Z")),
    };

    let mut malformed_process_instance = pending_operational_view();
    let runtime = malformed_process_instance.runtime.as_mut().unwrap();
    runtime.phase = DeploymentRuntimePhaseV2::Live;
    runtime.current_attempt = 2;
    runtime.attestation = Some(DeploymentAttestationViewV2 {
        deployment_revision: 7,
        convergence_attempt: 2,
        process_instance_id: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_string(),
    });
    runtime.serving.state = DeploymentServingFreshnessStateV2::LeaseMissing;

    let mut wrong_retry_clock = pending_operational_view();
    wrong_retry_clock.state = DeploymentOperationalStateV2::Failed;
    let runtime = wrong_retry_clock.runtime.as_mut().unwrap();
    runtime.phase = DeploymentRuntimePhaseV2::RetryWaiting;
    runtime.current_attempt = 2;
    runtime.last_failure_attempt = Some(2);
    runtime.failure = Some(DeploymentFailureViewV2 {
        retryable: true,
        code: "gateway_start_failed".to_string(),
    });
    runtime.retry = Some(DeploymentRetryViewV2 {
        state: DeploymentRetryStateV2::Waiting,
        failure_attempt: 2,
        retry_not_before: timestamp("2026-07-19T12:00:00Z"),
    });

    for view in [
        wrong_identity,
        private_failure,
        missing_authority_action,
        false_live,
        malformed_freshness,
        malformed_process_instance,
        wrong_retry_clock,
    ] {
        set_operational_response(&facade, view);
        let response = router
            .clone()
            .oneshot(operational_request("install-1", PROMOTION))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = body_text(response).await;
        assert!(body.contains("internal_error"));
        assert!(!body.contains("private_controller_diagnostic"));
        assert!(!body.contains("another-installation"));
    }
}

#[tokio::test]
async fn operational_v2_maps_facade_failures_and_enforces_the_request_deadline() {
    for (failure, status, code) in [
        (1, StatusCode::SERVICE_UNAVAILABLE, "dependency_unavailable"),
        (2, StatusCode::GATEWAY_TIMEOUT, "dependency_timeout"),
    ] {
        let facade = Arc::new(FakeFacade::default());
        facade.operational_fail.store(failure, Ordering::SeqCst);
        let response = operational_app(facade)
            .oneshot(operational_request("install-1", PROMOTION))
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        let body = body_text(response).await;
        assert!(body.contains(code));
        assert!(body.contains("\"retryable\":true"));
        assert!(!body.contains("database"));
        assert!(!body.contains("controller"));
    }

    let facade = Arc::new(FakeFacade::default());
    facade.block_operational.store(1, Ordering::SeqCst);
    let response = operational_app_with_timeout(Arc::clone(&facade), Duration::from_millis(10))
        .oneshot(operational_request("install-1", PROMOTION))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    let body = body_text(response).await;
    assert!(body.contains("request_timeout"));
    assert!(body.contains("\"retryable\":true"));
    assert_eq!(facade.operational_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn return_path_configuration_rejects_ambiguous_entries() {
    for value in [
        "//evil",
        "/\\evil",
        "/app?next=/admin",
        "/admin#section",
        "/app/..",
        "/app%2fadmin",
        "/app.settings",
        "/app settings",
        "/app//settings",
    ] {
        let result = HttpBoundaryConfig::new(
            ORIGIN,
            1_024,
            8,
            Duration::from_secs(2),
            [value.to_string()],
        );
        assert!(result.is_err(), "{value}");
    }
    assert!(HttpBoundaryConfig::new(
        ORIGIN,
        1_024,
        8,
        Duration::from_secs(2),
        ["/".to_string(), "/app".to_string()],
    )
    .is_ok());
}

fn request_builder(method: &str, uri: &str) -> axum::http::request::Builder {
    request_builder_with_id(method, uri, "test-request-1")
}

fn request_builder_with_id(
    method: &str,
    uri: &str,
    request_id: &str,
) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("host", HOST)
        .header("x-request-id", request_id)
}

fn promotion_request() -> axum::http::request::Builder {
    request_builder(
        "POST",
        "/v1/installations/install-1/authoring/sessions/session-1/promotions",
    )
    .header("content-type", "application/json")
    .header("origin", ORIGIN)
    .header(
        "cookie",
        format!("__Host-starring_session={SESSION}; __Host-starring_csrf={CSRF}"),
    )
    .header("x-csrf-token", CSRF)
    .header("idempotency-key", "request-1")
}

fn authoring_turn_request(body: impl Into<Body>) -> Request<Body> {
    request_builder(
        "POST",
        "/v1/installations/install-1/authoring/sessions/session-1/turns",
    )
    .header("content-type", "application/json")
    .header("origin", ORIGIN)
    .header(
        "cookie",
        format!("__Host-starring_session={SESSION}; __Host-starring_csrf={CSRF}"),
    )
    .header("x-csrf-token", CSRF)
    .header("idempotency-key", "authoring-request-1")
    .body(body.into())
    .unwrap()
}

fn authoring_session_request() -> Request<Body> {
    request_builder(
        "GET",
        "/v1/installations/install-1/authoring/sessions/session-1",
    )
    .header("cookie", format!("__Host-starring_session={SESSION}"))
    .body(Body::empty())
    .unwrap()
}

async fn body_text(response: axum::response::Response) -> String {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(body.to_vec()).unwrap()
}

#[test]
fn authoring_timeout_configuration_is_derived_and_bounded() {
    let config =
        AuthoringHttpBoundaryConfigV1::new(Duration::from_millis(50), Duration::from_millis(25), 7)
            .unwrap();
    assert_eq!(config.request_timeout(), Duration::from_millis(125));
    assert_eq!(config.retry_after_seconds(), 7);
    assert_eq!(config.max_in_flight(), 64);
    assert_eq!(config.with_max_in_flight(3).unwrap().max_in_flight(), 3);
    assert!(AuthoringHttpBoundaryConfigV1::new(Duration::ZERO, Duration::from_secs(1), 1).is_err());
    assert!(AuthoringHttpBoundaryConfigV1::new(Duration::from_secs(1), Duration::ZERO, 1).is_err());
    assert!(
        AuthoringHttpBoundaryConfigV1::new(Duration::from_secs(1), Duration::from_secs(1), 61)
            .is_err()
    );
    assert!(config.with_max_in_flight(0).is_err());
    assert!(config.with_max_in_flight(257).is_err());
}

#[tokio::test]
async fn authoring_turn_statuses_and_safe_wire_shapes_are_closed() {
    for (mode, status, disposition, generation, state) in [
        (
            0,
            StatusCode::CREATED,
            Some("created"),
            Some(1),
            "discussion",
        ),
        (
            1,
            StatusCode::OK,
            Some("exact_replay"),
            Some(1),
            "discussion",
        ),
        (2, StatusCode::OK, None, None, "unsupported"),
    ] {
        let facade = Arc::new(FakeFacade::default());
        facade.authoring_mode.store(mode, Ordering::SeqCst);
        let response = authoring_app(Arc::clone(&facade))
            .oneshot(authoring_turn_request(
                r#"{"expected_generation":0,"message":"스터디룸을 설계해줘"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let view: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(view["session_id"], "session-1");
        assert_eq!(
            view.get("disposition").and_then(serde_json::Value::as_str),
            disposition
        );
        assert_eq!(view["generation"].as_u64(), generation, "{state}");
        assert_eq!(view["projection"]["state"], state);
        let body = String::from_utf8(body.to_vec()).unwrap();
        for forbidden in [
            "ciphertext",
            "snapshot_nonce",
            "transcript",
            "system_prompt",
            "raw_backend_error",
        ] {
            assert!(!body.contains(forbidden));
        }
        assert_eq!(facade.authoring_turn_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            facade.authoring_worker_calls.load(Ordering::SeqCst),
            usize::from(mode != 1)
        );
    }
}

#[tokio::test]
async fn authoring_response_boundary_requires_typed_ruleset_identity_integrity() {
    let valid = Arc::new(FakeFacade::default());
    valid.authoring_mode.store(8, Ordering::SeqCst);
    let response = authoring_app(valid)
        .oneshot(authoring_turn_request(
            r#"{"expected_generation":0,"message":"welcome"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let view: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(view["projection"]["state"], "preview_ready");
    assert_eq!(
        view["projection"]["preview"]["receipt"]["candidate_ruleset_hash"],
        "f283047e6367d67067822a399200ffd2ea6c1a6940969e0ab9abd399cb43d537"
    );

    for mode in [9, 10] {
        let invalid = Arc::new(FakeFacade::default());
        invalid.authoring_mode.store(mode, Ordering::SeqCst);
        let response = authoring_app(invalid)
            .oneshot(authoring_turn_request(
                r#"{"expected_generation":0,"message":"welcome"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    let valid_read = Arc::new(FakeFacade::default());
    valid_read.authoring_read_mode.store(5, Ordering::SeqCst);
    let response = authoring_app(valid_read)
        .oneshot(authoring_session_request())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    for mode in [6, 7] {
        let invalid = Arc::new(FakeFacade::default());
        invalid.authoring_read_mode.store(mode, Ordering::SeqCst);
        let response = authoring_app(invalid)
            .oneshot(authoring_session_request())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}

#[tokio::test]
async fn authoring_turn_requires_the_complete_mutation_boundary() {
    let facade = Arc::new(FakeFacade::default());
    let router = authoring_app(Arc::clone(&facade));
    let mut missing_content_type =
        authoring_turn_request(r#"{"expected_generation":0,"message":"hello"}"#);
    missing_content_type.headers_mut().remove("content-type");
    let mut missing_origin =
        authoring_turn_request(r#"{"expected_generation":0,"message":"hello"}"#);
    missing_origin.headers_mut().remove("origin");
    let mut missing_cookie =
        authoring_turn_request(r#"{"expected_generation":0,"message":"hello"}"#);
    missing_cookie.headers_mut().remove("cookie");
    let mut missing_csrf = authoring_turn_request(r#"{"expected_generation":0,"message":"hello"}"#);
    missing_csrf.headers_mut().remove("x-csrf-token");
    let mut missing_idempotency =
        authoring_turn_request(r#"{"expected_generation":0,"message":"hello"}"#);
    missing_idempotency.headers_mut().remove("idempotency-key");
    let query = request_builder(
        "POST",
        "/v1/installations/install-1/authoring/sessions/session-1/turns?extra=1",
    )
    .header("content-type", "application/json")
    .header("origin", ORIGIN)
    .header(
        "cookie",
        format!("__Host-starring_session={SESSION}; __Host-starring_csrf={CSRF}"),
    )
    .header("x-csrf-token", CSRF)
    .header("idempotency-key", "authoring-request-query")
    .body(Body::from(r#"{"expected_generation":0,"message":"hello"}"#))
    .unwrap();
    for (request, expected) in [
        (missing_content_type, StatusCode::UNSUPPORTED_MEDIA_TYPE),
        (missing_origin, StatusCode::FORBIDDEN),
        (missing_cookie, StatusCode::UNAUTHORIZED),
        (missing_csrf, StatusCode::FORBIDDEN),
        (missing_idempotency, StatusCode::BAD_REQUEST),
        (query, StatusCode::BAD_REQUEST),
    ] {
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), expected);
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    assert_eq!(facade.authoring_turn_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn authoring_turn_accepts_bounded_unicode_and_rejects_ambiguous_json() {
    let facade = Arc::new(FakeFacade::default());
    let router = authoring_app(Arc::clone(&facade));
    let valid = authoring_turn_request(
        serde_json::json!({
            "expected_generation": 0,
            "message": "  스터디룸\r\n만들어줘  "
        })
        .to_string(),
    );
    let response = router.clone().oneshot(valid).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        facade.authoring_messages.lock().unwrap().as_slice(),
        ["스터디룸\n만들어줘"]
    );

    let invalid_bodies = [
        r#"{"expected_generation":0,"message":"hello","unknown":true}"#.to_string(),
        r#"{"expected_generation":0,"message":"one","message":"two"}"#.to_string(),
        r#"{"expected_generation":0,"message":"left\tright"}"#.to_string(),
        format!(
            "{{\"expected_generation\":0,\"message\":\"{}\"}}",
            "a".repeat(2_001)
        ),
        r#"{"expected_generation":9007199254740991,"message":"hello"}"#.to_string(),
    ];
    for body in invalid_bodies {
        let response = router
            .clone()
            .oneshot(authoring_turn_request(body))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    let invalid_path = request_builder(
        "POST",
        "/v1/installations/install-1/authoring/sessions/session!/turns",
    )
    .header("content-type", "application/json")
    .header("origin", ORIGIN)
    .header(
        "cookie",
        format!("__Host-starring_session={SESSION}; __Host-starring_csrf={CSRF}"),
    )
    .header("x-csrf-token", CSRF)
    .header("idempotency-key", "authoring-invalid-path")
    .body(Body::from(r#"{"expected_generation":0,"message":"hello"}"#))
    .unwrap();
    let response = router.oneshot(invalid_path).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(facade.authoring_turn_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn authoring_read_is_cookie_bound_non_enumerating_and_no_store() {
    let facade = Arc::new(FakeFacade::default());
    let response = authoring_app(Arc::clone(&facade))
        .oneshot(authoring_session_request())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body = body_text(response).await;
    assert!(body.contains("\"observed_generation\":3"));
    assert!(body.contains("\"state\":\"discussion\""));
    assert!(!body.contains("disposition"));

    let missing_cookie = request_builder(
        "GET",
        "/v1/installations/install-1/authoring/sessions/session-1",
    )
    .body(Body::empty())
    .unwrap();
    let response = authoring_app(Arc::clone(&facade))
        .oneshot(missing_cookie)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let forbidden = Arc::new(FakeFacade::default());
    forbidden.authoring_read_mode.store(1, Ordering::SeqCst);
    let forbidden_response = authoring_app(forbidden)
        .oneshot(authoring_session_request())
        .await
        .unwrap();
    let forbidden_status = forbidden_response.status();
    let forbidden_body = body_text(forbidden_response).await;

    let missing = Arc::new(FakeFacade::default());
    missing.authoring_read_mode.store(2, Ordering::SeqCst);
    let missing_response = authoring_app(missing)
        .oneshot(authoring_session_request())
        .await
        .unwrap();
    let missing_status = missing_response.status();
    let missing_body = body_text(missing_response).await;
    assert_eq!(forbidden_status, StatusCode::NOT_FOUND);
    assert_eq!(missing_status, StatusCode::NOT_FOUND);
    assert_eq!(forbidden_body, missing_body);

    let corrupt = Arc::new(FakeFacade::default());
    corrupt.authoring_read_mode.store(4, Ordering::SeqCst);
    let corrupt_response = authoring_app(corrupt)
        .oneshot(authoring_session_request())
        .await
        .unwrap();
    let corrupt_status = corrupt_response.status();
    let corrupt_body = body_text(corrupt_response).await;
    assert_eq!(corrupt_status, StatusCode::NOT_FOUND);
    assert_eq!(corrupt_body, missing_body);

    let invalid = Arc::new(FakeFacade::default());
    invalid.authoring_read_mode.store(3, Ordering::SeqCst);
    let response = authoring_app(invalid)
        .oneshot(authoring_session_request())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn authoring_saturation_is_retryable_and_never_enters_worker_capacity() {
    let facade = Arc::new(FakeFacade::default());
    facade.authoring_mode.store(3, Ordering::SeqCst);
    let response = authoring_app(Arc::clone(&facade))
        .oneshot(authoring_turn_request(
            r#"{"expected_generation":0,"message":"hello"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()["retry-after"], "7");
    let body = body_text(response).await;
    assert!(body.contains("\"code\":\"authoring_saturated\""));
    assert!(body.contains("\"retryable\":true"));
    assert_eq!(facade.authoring_turn_calls.load(Ordering::SeqCst), 1);
    assert_eq!(facade.authoring_worker_calls.load(Ordering::SeqCst), 0);

    let invalid = Arc::new(FakeFacade::default());
    invalid.authoring_mode.store(4, Ordering::SeqCst);
    let response = authoring_app(invalid)
        .oneshot(authoring_turn_request(
            r#"{"expected_generation":0,"message":"hello"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn unavailable_authoring_lane_does_not_degrade_control_routes() {
    let facade = Arc::new(FakeFacade::default());
    facade.authoring_mode.store(7, Ordering::SeqCst);
    let router = authoring_app(Arc::clone(&facade));
    let response = router
        .clone()
        .oneshot(authoring_turn_request(
            r#"{"expected_generation":0,"message":"hello"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(body_text(response).await.contains("dependency_unavailable"));

    let control = router
        .oneshot(
            request_builder("GET", "/v1/me")
                .header("cookie", format!("__Host-starring_session={SESSION}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(control.status(), StatusCode::OK);
}

#[tokio::test]
async fn authoring_bulkhead_bounds_waiters_without_starving_control_routes() {
    let facade = Arc::new(FakeFacade::default());
    facade.authoring_delay_millis.store(250, Ordering::SeqCst);
    let router = authoring_app_with_limits(
        Arc::clone(&facade),
        Duration::from_secs(1),
        Duration::from_secs(1),
        Duration::from_secs(1),
        7,
        1,
        1,
    );
    let first = tokio::spawn(router.clone().oneshot(authoring_turn_request(
        r#"{"expected_generation":0,"message":"first"}"#,
    )));
    facade.authoring_entered.notified().await;

    let saturated = router
        .clone()
        .oneshot(authoring_turn_request(
            r#"{"expected_generation":0,"message":"duplicate"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(saturated.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(saturated.headers()["retry-after"], "7");
    assert!(body_text(saturated).await.contains("authoring_saturated"));
    assert_eq!(facade.authoring_turn_calls.load(Ordering::SeqCst), 1);

    let control = router
        .oneshot(
            request_builder("GET", "/v1/me")
                .header("cookie", format!("__Host-starring_session={SESSION}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(control.status(), StatusCode::OK);
    assert_eq!(first.await.unwrap().unwrap().status(), StatusCode::CREATED);
}

#[tokio::test]
async fn authoring_timeout_is_exact_and_general_timeout_remains_unchanged() {
    let facade = Arc::new(FakeFacade::default());
    facade.authoring_delay_millis.store(60, Ordering::SeqCst);
    let router = authoring_app_with_timeouts(
        Arc::clone(&facade),
        Duration::from_millis(20),
        Duration::from_millis(50),
        Duration::from_millis(20),
        1,
    );
    let response = router
        .clone()
        .oneshot(authoring_turn_request(
            r#"{"expected_generation":0,"message":"hello"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    facade.block_promote.store(1, Ordering::SeqCst);
    let response = router
        .oneshot(
            promotion_request()
                .body(Body::from(r#"{"expected_generation":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    assert!(body_text(response).await.contains("request_timeout"));

    let timeout_facade = Arc::new(FakeFacade::default());
    timeout_facade
        .authoring_delay_millis
        .store(100, Ordering::SeqCst);
    let response = authoring_app_with_timeouts(
        Arc::clone(&timeout_facade),
        Duration::from_secs(1),
        Duration::from_millis(5),
        Duration::from_millis(5),
        1,
    )
    .oneshot(authoring_turn_request(
        r#"{"expected_generation":0,"message":"hello"}"#,
    ))
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    assert!(body_text(response).await.contains("request_timeout"));
    assert_eq!(
        timeout_facade
            .authoring_completed_calls
            .load(Ordering::SeqCst),
        0
    );

    let committing_facade = Arc::new(FakeFacade::default());
    committing_facade.authoring_mode.store(6, Ordering::SeqCst);
    committing_facade
        .authoring_delay_millis
        .store(75, Ordering::SeqCst);
    let response = authoring_app_with_timeouts(
        Arc::clone(&committing_facade),
        Duration::from_secs(1),
        Duration::from_millis(5),
        Duration::from_millis(5),
        1,
    )
    .oneshot(authoring_turn_request(
        r#"{"expected_generation":0,"message":"hello"}"#,
    ))
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        committing_facade
            .authoring_commit_phase_calls
            .load(Ordering::SeqCst),
        1
    );
    assert_eq!(
        committing_facade
            .authoring_completed_calls
            .load(Ordering::SeqCst),
        1
    );
}

#[tokio::test]
async fn health_requires_exact_host_and_emits_security_headers() {
    let facade = Arc::new(FakeFacade::default());
    let missing_host = Request::builder()
        .uri("/health/live")
        .body(Body::empty())
        .unwrap();
    let response = app(Arc::clone(&facade))
        .oneshot(missing_host)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = app(facade)
        .oneshot(
            request_builder("GET", "/health/live")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(
        response.headers()["content-security-policy"],
        "default-src 'none'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'"
    );
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert_eq!(
        response.headers()["strict-transport-security"],
        "max-age=31536000"
    );
    assert_eq!(response.headers()["x-frame-options"], "DENY");
    assert_eq!(
        response.headers()["cross-origin-resource-policy"],
        "same-origin"
    );
    assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    assert_eq!(response.headers()["x-request-id"], "test-request-1");
}

#[tokio::test]
async fn liveness_remains_available_when_business_capacity_is_exhausted() {
    let facade = Arc::new(FakeFacade::default());
    facade.block_promote.store(1, Ordering::SeqCst);
    let app = app_with_concurrency(Arc::clone(&facade), 1);
    let work = tokio::spawn(
        app.clone().oneshot(
            promotion_request()
                .body(Body::from(r#"{"expected_generation":1}"#))
                .unwrap(),
        ),
    );
    facade.promote_entered.notified().await;

    let response = app
        .clone()
        .oneshot(
            request_builder("GET", "/health/live")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .oneshot(
            request_builder("GET", "/health/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    facade.promote_release.notify_one();
    assert_eq!(work.await.unwrap().unwrap().status(), StatusCode::CREATED);
}

#[tokio::test]
async fn readiness_gate_blocks_business_routes_before_facade_or_capacity() {
    let facade = Arc::new(FakeFacade::default());
    let gate = ProductApiReadinessGate::initially_unready();
    let lease = gate.claim().unwrap();
    let app = operational_app_with_gate(Arc::clone(&facade), gate.clone());

    let response = app
        .clone()
        .oneshot(
            request_builder("GET", "/health/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(body_text(response).await.contains("dependency_unavailable"));
    assert_eq!(facade.readiness_calls.load(Ordering::SeqCst), 0);

    let response = app
        .clone()
        .oneshot(
            request_builder("GET", "/oauth/discord/start?return_to=%2Fapp")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(body_text(response).await.contains("dependency_unavailable"));
    assert_eq!(facade.oauth_start_calls.load(Ordering::SeqCst), 0);

    let response = app
        .clone()
        .oneshot(
            request_builder("GET", "/health/live")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    lease.mark_ready();
    let response = app
        .clone()
        .oneshot(
            request_builder("GET", "/health/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(facade.readiness_calls.load(Ordering::SeqCst), 0);

    let response = app
        .clone()
        .oneshot(
            request_builder("GET", "/oauth/discord/start?return_to=%2Fapp")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(facade.oauth_start_calls.load(Ordering::SeqCst), 1);

    lease.mark_unready();
    let response = app
        .oneshot(
            request_builder("GET", "/health/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(facade.readiness_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn readiness_endpoint_is_an_atomic_gate_read_without_facade_amplification() {
    let facade = Arc::new(FakeFacade::default());
    let gate = ProductApiReadinessGate::initially_unready();
    let lease = gate.claim().unwrap();
    lease.mark_ready();
    let app = operational_app_with_gate(Arc::clone(&facade), gate.clone());

    for _ in 0..16 {
        let response = app
            .clone()
            .oneshot(
                request_builder("GET", "/health/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    assert_eq!(facade.readiness_calls.load(Ordering::SeqCst), 0);

    lease.mark_unready();
    let response = app
        .oneshot(
            request_builder("GET", "/health/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(facade.readiness_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn mutation_requires_cookie_exact_origin_and_session_csrf() {
    let facade = Arc::new(FakeFacade::default());
    let missing_origin = request_builder(
        "POST",
        "/v1/installations/install-1/authoring/sessions/session-1/promotions",
    )
    .header("content-type", "application/json")
    .header(
        "cookie",
        format!("__Host-starring_session={SESSION}; __Host-starring_csrf={CSRF}"),
    )
    .header("x-csrf-token", CSRF)
    .header("idempotency-key", "request-1")
    .body(Body::from(r#"{"expected_generation":1}"#))
    .unwrap();
    let response = app(Arc::clone(&facade))
        .oneshot(missing_origin)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let missing_session = request_builder(
        "POST",
        "/v1/installations/install-1/authoring/sessions/session-1/promotions",
    )
    .header("content-type", "application/json")
    .header("origin", ORIGIN)
    .header("cookie", format!("__Host-starring_csrf={CSRF}"))
    .header("x-csrf-token", CSRF)
    .header("idempotency-key", "request-1")
    .body(Body::from(r#"{"expected_generation":1}"#))
    .unwrap();
    let response = app(Arc::clone(&facade))
        .oneshot(missing_session)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let missing_csrf_header = request_builder(
        "POST",
        "/v1/installations/install-1/authoring/sessions/session-1/promotions",
    )
    .header("content-type", "application/json")
    .header("origin", ORIGIN)
    .header(
        "cookie",
        format!("__Host-starring_session={SESSION}; __Host-starring_csrf={CSRF}"),
    )
    .header("idempotency-key", "request-1")
    .body(Body::from(r#"{"expected_generation":1}"#))
    .unwrap();
    let response = app(Arc::clone(&facade))
        .oneshot(missing_csrf_header)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let missing_csrf_cookie = request_builder(
        "POST",
        "/v1/installations/install-1/authoring/sessions/session-1/promotions",
    )
    .header("content-type", "application/json")
    .header("origin", ORIGIN)
    .header("cookie", format!("__Host-starring_session={SESSION}"))
    .header("x-csrf-token", CSRF)
    .header("idempotency-key", "request-1")
    .body(Body::from(r#"{"expected_generation":1}"#))
    .unwrap();
    let response = app(Arc::clone(&facade))
        .oneshot(missing_csrf_cookie)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let wrong_origin = request_builder(
        "POST",
        "/v1/installations/install-1/authoring/sessions/session-1/promotions",
    )
    .header("content-type", "application/json")
    .header("origin", "https://attacker.example")
    .header(
        "cookie",
        format!("__Host-starring_session={SESSION}; __Host-starring_csrf={CSRF}"),
    )
    .header("x-csrf-token", CSRF)
    .header("idempotency-key", "request-1")
    .body(Body::from(r#"{"expected_generation":1}"#))
    .unwrap();
    let response = app(Arc::clone(&facade))
        .oneshot(wrong_origin)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let mismatched_csrf = request_builder(
        "POST",
        "/v1/installations/install-1/authoring/sessions/session-1/promotions",
    )
    .header("content-type", "application/json")
    .header("origin", ORIGIN)
    .header(
        "cookie",
        format!("__Host-starring_session={SESSION}; __Host-starring_csrf={CSRF}"),
    )
    .header("x-csrf-token", OTHER_CSRF)
    .header("idempotency-key", "request-1")
    .body(Body::from(r#"{"expected_generation":1}"#))
    .unwrap();
    let response = app(Arc::clone(&facade))
        .oneshot(mismatched_csrf)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(facade.verify_calls.load(Ordering::SeqCst), 0);

    let stale_backend_csrf = request_builder(
        "POST",
        "/v1/installations/install-1/authoring/sessions/session-1/promotions",
    )
    .header("content-type", "application/json")
    .header("origin", ORIGIN)
    .header(
        "cookie",
        format!("__Host-starring_session={SESSION}; __Host-starring_csrf={OTHER_CSRF}"),
    )
    .header("x-csrf-token", OTHER_CSRF)
    .header("idempotency-key", "request-1")
    .body(Body::from(r#"{"expected_generation":1}"#))
    .unwrap();
    let response = app(Arc::clone(&facade))
        .oneshot(stale_backend_csrf)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = app(Arc::clone(&facade))
        .oneshot(
            promotion_request()
                .body(Body::from(r#"{"expected_generation":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(facade.verify_calls.load(Ordering::SeqCst), 2);
    assert_eq!(facade.promote_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn json_media_shape_and_body_limit_are_strict() {
    let facade = Arc::new(FakeFacade::default());
    let wrong_media = promotion_request()
        .header("content-type", "text/plain")
        .body(Body::from(r#"{"expected_generation":1}"#))
        .unwrap();
    let response = app(Arc::clone(&facade)).oneshot(wrong_media).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let unknown = promotion_request()
        .body(Body::from(
            r#"{"expected_generation":1,"tenant_id":"attacker"}"#,
        ))
        .unwrap();
    let response = app(Arc::clone(&facade)).oneshot(unknown).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let oversized = promotion_request()
        .body(Body::from(format!(
            "{{\"expected_generation\":1,\"padding\":\"{}\"}}",
            "x".repeat(2_000)
        )))
        .unwrap();
    let response = app(facade).oneshot(oversized).await.unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn duplicate_security_inputs_are_rejected() {
    let facade = Arc::new(FakeFacade::default());
    let duplicate_cookie = promotion_request()
        .header("cookie", format!("__Host-starring_session={SESSION}"))
        .body(Body::from(r#"{"expected_generation":1}"#))
        .unwrap();
    let response = app(Arc::clone(&facade))
        .oneshot(duplicate_cookie)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let duplicate_origin = promotion_request()
        .header("origin", ORIGIN)
        .body(Body::from(r#"{"expected_generation":1}"#))
        .unwrap();
    let response = app(Arc::clone(&facade))
        .oneshot(duplicate_origin)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let duplicate_csrf = promotion_request()
        .header("x-csrf-token", CSRF)
        .body(Body::from(r#"{"expected_generation":1}"#))
        .unwrap();
    let response = app(Arc::clone(&facade))
        .oneshot(duplicate_csrf)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let duplicate_csrf_cookie = request_builder(
        "POST",
        "/v1/installations/install-1/authoring/sessions/session-1/promotions",
    )
    .header("content-type", "application/json")
    .header("origin", ORIGIN)
    .header(
        "cookie",
        format!(
            "__Host-starring_session={SESSION}; __Host-starring_csrf={CSRF}; __Host-starring_csrf={CSRF}"
        ),
    )
    .header("x-csrf-token", CSRF)
    .header("idempotency-key", "request-1")
    .body(Body::from(r#"{"expected_generation":1}"#))
    .unwrap();
    let response = app(Arc::clone(&facade))
        .oneshot(duplicate_csrf_cookie)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let duplicate_idempotency = promotion_request()
        .header("idempotency-key", "request-2")
        .body(Body::from(r#"{"expected_generation":1}"#))
        .unwrap();
    let response = app(Arc::clone(&facade))
        .oneshot(duplicate_idempotency)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let duplicate_query = request_builder(
        "GET",
        &format!("/oauth/discord/callback?code=one-time-code&state={STATE}&state={STATE}"),
    )
    .header("cookie", format!("__Host-starring_oauth={NONCE}"))
    .body(Body::empty())
    .unwrap();
    let response = app(facade).oneshot(duplicate_query).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let cleared_nonce = response.headers().get_all("set-cookie");
    let cleared_nonce = cleared_nonce.iter().collect::<Vec<_>>();
    assert_eq!(cleared_nonce.len(), 1);
    assert!(cleared_nonce[0].is_sensitive());
    assert!(cleared_nonce[0]
        .to_str()
        .unwrap()
        .starts_with("__Host-starring_oauth=;"));
}

#[tokio::test]
async fn oauth_callback_clears_nonce_after_missing_or_malformed_nonce() {
    for cookie in [None, Some("__Host-starring_oauth=invalid")] {
        let mut request = request_builder(
            "GET",
            &format!("/oauth/discord/callback?code=one-time-code&state={STATE}"),
        );
        if let Some(cookie) = cookie {
            request = request.header("cookie", cookie);
        }
        let response = app(Arc::new(FakeFacade::default()))
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let cleared_nonce = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .collect::<Vec<_>>();
        assert_eq!(cleared_nonce.len(), 1);
        assert!(cleared_nonce[0].is_sensitive());
        assert!(cleared_nonce[0]
            .to_str()
            .unwrap()
            .starts_with("__Host-starring_oauth=;"));
    }
}

#[tokio::test]
async fn internal_errors_are_stable_and_redacted() {
    let facade = Arc::new(FakeFacade::default());
    facade.fail_me.store(1, Ordering::SeqCst);
    let request = request_builder("GET", "/v1/me")
        .header("cookie", format!("__Host-starring_session={SESSION}"))
        .body(Body::empty())
        .unwrap();
    let response = app(facade).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = body_text(response).await;
    assert!(body.contains("internal_error"));
    assert!(body.contains("test-request-1"));
    assert!(!body.contains(SESSION));
    assert!(!body.contains("cookie"));
    assert!(!body.contains("sql"));
}

#[tokio::test]
async fn oauth_start_sets_a_host_only_secure_cookie() {
    let response = app(Arc::new(FakeFacade::default()))
        .oneshot(
            request_builder("GET", "/oauth/discord/start?return_to=%2Fapp")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FOUND);
    assert!(response.headers()["location"].is_sensitive());
    assert_eq!(
        response.headers()["location"].to_str().unwrap(),
        format!(
            "https://discord.com/oauth2/authorize?client_id=123456789012345678&redirect_uri=https%3A%2F%2Fstarring.example%2Foauth%2Fdiscord%2Fcallback&response_type=code&scope=identify&state={STATE}"
        )
    );
    let cookie = response.headers()["set-cookie"].to_str().unwrap();
    assert!(cookie.starts_with("__Host-starring_oauth="));
    assert!(cookie.contains("Secure"));
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Lax"));
    assert!(!cookie.contains("Domain="));
}

#[tokio::test]
async fn oauth_start_budget_is_shared_by_router_clones() {
    let facade = Arc::new(FakeFacade::default());
    let router = app(Arc::clone(&facade));
    for _ in 0..10 {
        let response = router
            .clone()
            .oneshot(
                request_builder("GET", "/oauth/discord/start")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FOUND);
    }

    let response = router
        .oneshot(
            request_builder("GET", "/oauth/discord/start")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        response.headers()["content-type"],
        "application/problem+json"
    );
    assert!(response.headers().contains_key("retry-after"));
    assert!(!response.headers().contains_key("location"));
    assert!(!response.headers().contains_key("set-cookie"));
    let body = body_text(response).await;
    assert!(body.contains("oauth_start_rate_limited"));
    assert!(body.contains("\"retryable\":true"));
    assert_eq!(facade.oauth_start_calls.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn malformed_oauth_starts_do_not_consume_the_budget() {
    let facade = Arc::new(FakeFacade::default());
    let router = app(Arc::clone(&facade));
    for _ in 0..20 {
        let response = router
            .clone()
            .oneshot(
                request_builder("GET", "/oauth/discord/start?return_to=%2Fadmin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    for _ in 0..10 {
        let response = router
            .clone()
            .oneshot(
                request_builder("GET", "/oauth/discord/start")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FOUND);
    }

    assert_eq!(facade.oauth_start_calls.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn admitted_oauth_start_failures_are_not_refunded() {
    let facade = Arc::new(FakeFacade::default());
    facade.fail_oauth_start.store(1, Ordering::SeqCst);
    let router = app(Arc::clone(&facade));
    let response = router
        .clone()
        .oneshot(
            request_builder("GET", "/oauth/discord/start")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    facade.fail_oauth_start.store(0, Ordering::SeqCst);

    for _ in 0..9 {
        let response = router
            .clone()
            .oneshot(
                request_builder("GET", "/oauth/discord/start")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FOUND);
    }
    let response = router
        .oneshot(
            request_builder("GET", "/oauth/discord/start")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(facade.oauth_start_calls.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn oauth_authorization_rejects_a_non_snowflake_client_identity() {
    let facade = Arc::new(FakeFacade::default());
    facade.invalid_client_id.store(1, Ordering::SeqCst);
    let response = app(facade)
        .oneshot(
            request_builder("GET", "/oauth/discord/start")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn oauth_authorization_rejects_a_callback_not_owned_by_the_http_edge() {
    let facade = Arc::new(FakeFacade::default());
    facade.invalid_callback_url.store(1, Ordering::SeqCst);
    let response = app(facade)
        .oneshot(
            request_builder("GET", "/oauth/discord/start")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn oauth_return_paths_are_an_exact_unambiguous_allowlist() {
    for value in [
        "%2F%2Fevil.example",
        "%2F%5Cevil.example",
        "%2Fapp%3Fnext%3D%2Fadmin",
        "%2Fadmin",
        "%2Fapp%2F..",
        "%2Fapp%252Fadmin",
    ] {
        let response = app(Arc::new(FakeFacade::default()))
            .oneshot(
                request_builder("GET", &format!("/oauth/discord/start?return_to={value}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{value}");
    }

    let facade = Arc::new(FakeFacade::default());
    facade.disallowed_return.store(1, Ordering::SeqCst);
    let request = request_builder(
        "GET",
        &format!("/oauth/discord/callback?code=one-time-code&state={STATE}"),
    )
    .header("cookie", format!("__Host-starring_oauth={NONCE}"))
    .body(Body::empty())
    .unwrap();
    let response = app(facade).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn product_status_exposes_and_validates_exact_apply_source_binding() {
    let facade = Arc::new(FakeFacade::default());
    facade.status_response.store(1, Ordering::SeqCst);
    let uri = format!("/v1/installations/install-1/promotions/{PROMOTION}");
    let response = app(Arc::clone(&facade))
        .oneshot(
            request_builder("GET", &uri)
                .header("cookie", format!("__Host-starring_session={SESSION}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let view: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(view["state"], "runtime_pending");
    assert_eq!(view["revision"], 4);
    assert_eq!(view["payload_digest"], DIGEST);
    assert_eq!(view["apply_source_revision"], 2);
    assert_eq!(view["replayed"], false);

    for mode in [2, 3, 4] {
        facade.status_response.store(mode, Ordering::SeqCst);
        let response = app(Arc::clone(&facade))
            .oneshot(
                request_builder("GET", &uri)
                    .header("cookie", format!("__Host-starring_session={SESSION}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}

#[tokio::test]
async fn product_decisions_require_revision_and_digest_cas() {
    let facade = Arc::new(FakeFacade::default());
    let uri = format!("/v1/installations/install-1/promotions/{PROMOTION}/approvals");
    let valid = request_builder("POST", &uri)
        .header("content-type", "application/json")
        .header("origin", ORIGIN)
        .header(
            "cookie",
            format!("__Host-starring_session={SESSION}; __Host-starring_csrf={CSRF}"),
        )
        .header("x-csrf-token", CSRF)
        .header("idempotency-key", "approve-1")
        .body(Body::from(format!(
            "{{\"expected_payload_digest\":\"{DIGEST}\",\"expected_revision\":3}}"
        )))
        .unwrap();
    let response = app(Arc::clone(&facade)).oneshot(valid).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    facade.approval_response.store(1, Ordering::SeqCst);
    let pending = request_builder("POST", &uri)
        .header("content-type", "application/json")
        .header("origin", ORIGIN)
        .header(
            "cookie",
            format!("__Host-starring_session={SESSION}; __Host-starring_csrf={CSRF}"),
        )
        .header("x-csrf-token", CSRF)
        .header("idempotency-key", "approve-pending")
        .body(Body::from(format!(
            "{{\"expected_payload_digest\":\"{DIGEST}\",\"expected_revision\":3}}"
        )))
        .unwrap();
    let response = app(Arc::clone(&facade)).oneshot(pending).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    facade.approval_response.store(2, Ordering::SeqCst);
    let invalid_state = request_builder("POST", &uri)
        .header("content-type", "application/json")
        .header("origin", ORIGIN)
        .header(
            "cookie",
            format!("__Host-starring_session={SESSION}; __Host-starring_csrf={CSRF}"),
        )
        .header("x-csrf-token", CSRF)
        .header("idempotency-key", "approve-invalid-state")
        .body(Body::from(format!(
            "{{\"expected_payload_digest\":\"{DIGEST}\",\"expected_revision\":3}}"
        )))
        .unwrap();
    let response = app(Arc::clone(&facade))
        .oneshot(invalid_state)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let missing_revision = request_builder("POST", &uri)
        .header("content-type", "application/json")
        .header("origin", ORIGIN)
        .header(
            "cookie",
            format!("__Host-starring_session={SESSION}; __Host-starring_csrf={CSRF}"),
        )
        .header("x-csrf-token", CSRF)
        .header("idempotency-key", "approve-2")
        .body(Body::from(format!(
            "{{\"expected_payload_digest\":\"{DIGEST}\"}}"
        )))
        .unwrap();
    let response = app(Arc::clone(&facade))
        .oneshot(missing_revision)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let zero_revision = request_builder("POST", &uri)
        .header("content-type", "application/json")
        .header("origin", ORIGIN)
        .header(
            "cookie",
            format!("__Host-starring_session={SESSION}; __Host-starring_csrf={CSRF}"),
        )
        .header("x-csrf-token", CSRF)
        .header("idempotency-key", "approve-3")
        .body(Body::from(format!(
            "{{\"expected_payload_digest\":\"{DIGEST}\",\"expected_revision\":0}}"
        )))
        .unwrap();
    let response = app(facade).oneshot(zero_revision).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn lifecycle_cancellation_is_exact_bounded_and_separately_routed() {
    let facade = Arc::new(FakeFacade::default());
    let uri = format!("/v1/installations/install-1/promotions/{PROMOTION}/lifecycle-cancellations");
    let valid = request_builder_with_id("POST", &uri, "cancel-lifecycle-1")
        .header("content-type", "application/json")
        .header("origin", ORIGIN)
        .header(
            "cookie",
            format!("__Host-starring_session={SESSION}; __Host-starring_csrf={CSRF}"),
        )
        .header("x-csrf-token", CSRF)
        .header("idempotency-key", "cancel-lifecycle-key")
        .body(Body::from(format!(
            "{{\"expected_payload_digest\":\"{DIGEST}\",\"expected_revision\":4,\
             \"drain_intent_id\":\"{}\",\"acknowledged_intent_revision\":7,\
             \"acknowledged_state_digest\":\"{}\",\"product_operation_id\":\"{}\",\
             \"expected_runtime_deployment_revision\":9,\"reason\":\"operator cancelled\"}}",
            "d".repeat(32),
            "e".repeat(64),
            "f".repeat(32),
        )))
        .unwrap();
    let response = lifecycle_app(Arc::clone(&facade))
        .oneshot(valid)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let view: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(view["state"], "approved");
    assert_eq!(view["source_intent_revision"], 7);
    assert_eq!(view["terminal_intent_revision"], 8);
    assert_eq!(view["source_runtime_deployment_revision"], 9);
    assert_eq!(view["resulting_runtime_deployment_revision"], 10);
    assert_eq!(view["replayed"], false);
    assert_eq!(
        facade.mutation_request_ids.lock().unwrap().as_slice(),
        &[(
            "cancel_lifecycle".to_string(),
            "cancel-lifecycle-1".to_string(),
        )]
    );

    let invalid = request_builder("POST", &uri)
        .header("content-type", "application/json")
        .header("origin", ORIGIN)
        .header(
            "cookie",
            format!("__Host-starring_session={SESSION}; __Host-starring_csrf={CSRF}"),
        )
        .header("x-csrf-token", CSRF)
        .header("idempotency-key", "cancel-lifecycle-invalid")
        .body(Body::from(format!(
            "{{\"expected_payload_digest\":\"{DIGEST}\",\"expected_revision\":4,\
             \"drain_intent_id\":\"{}\",\"acknowledged_intent_revision\":7,\
             \"acknowledged_state_digest\":\"{}\",\"product_operation_id\":\"{}\",\
             \"expected_runtime_deployment_revision\":9,\"reason\":\"operator cancelled\"}}",
            "d".repeat(32),
            "e".repeat(64),
            "d".repeat(32),
        )))
        .unwrap();
    let response = lifecycle_app(Arc::clone(&facade))
        .oneshot(invalid)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(facade.mutation_request_ids.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn audited_mutations_receive_the_validated_transport_request_id() {
    let facade = Arc::new(FakeFacade::default());
    let cases = [
        (
            "promote",
            "/v1/installations/install-1/authoring/sessions/session-1/promotions".to_string(),
            "audit-promote",
            "promote-key",
            r#"{"expected_generation":1}"#.to_string(),
            StatusCode::CREATED,
        ),
        (
            "approve",
            format!("/v1/installations/install-1/promotions/{PROMOTION}/approvals"),
            "audit-approve",
            "approve-key",
            format!("{{\"expected_payload_digest\":\"{DIGEST}\",\"expected_revision\":3}}"),
            StatusCode::OK,
        ),
        (
            "reject",
            format!("/v1/installations/install-1/promotions/{PROMOTION}/rejections"),
            "audit-reject",
            "reject-key",
            format!("{{\"expected_payload_digest\":\"{DIGEST}\",\"expected_revision\":3,\"reason\":\"superseded\"}}"),
            StatusCode::OK,
        ),
        (
            "apply",
            format!("/v1/installations/install-1/promotions/{PROMOTION}/apply"),
            "audit-apply",
            "apply-key",
            format!("{{\"expected_payload_digest\":\"{DIGEST}\",\"expected_revision\":3}}"),
            StatusCode::ACCEPTED,
        ),
    ];
    for (_, uri, request_id, idempotency_key, body, expected_status) in &cases {
        let request = request_builder_with_id("POST", uri, request_id)
            .header("content-type", "application/json")
            .header("origin", ORIGIN)
            .header(
                "cookie",
                format!("__Host-starring_session={SESSION}; __Host-starring_csrf={CSRF}"),
            )
            .header("x-csrf-token", CSRF)
            .header("idempotency-key", *idempotency_key)
            .body(Body::from(body.clone()))
            .unwrap();
        let response = app(Arc::clone(&facade)).oneshot(request).await.unwrap();
        assert_eq!(response.status(), *expected_status);
        assert_eq!(response.headers()["x-request-id"], *request_id);
    }
    let observed = facade.mutation_request_ids.lock().unwrap();
    assert_eq!(observed.len(), cases.len());
    for ((expected_mutation, _, expected_request_id, _, _, _), (mutation, request_id)) in
        cases.iter().zip(observed.iter())
    {
        assert_eq!(mutation, expected_mutation);
        assert_eq!(request_id, expected_request_id);
    }
}

#[tokio::test]
async fn invalid_request_ids_are_replaced_before_reaching_a_mutation() {
    let facade = Arc::new(FakeFacade::default());
    let request = request_builder_with_id(
        "POST",
        "/v1/installations/install-1/authoring/sessions/session-1/promotions",
        "invalid/request/id",
    )
    .header("content-type", "application/json")
    .header("origin", ORIGIN)
    .header(
        "cookie",
        format!("__Host-starring_session={SESSION}; __Host-starring_csrf={CSRF}"),
    )
    .header("x-csrf-token", CSRF)
    .header("idempotency-key", "generated-request-id")
    .body(Body::from(r#"{"expected_generation":1}"#))
    .unwrap();
    let response = app(Arc::clone(&facade)).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let response_request_id = response.headers()["x-request-id"].to_str().unwrap();
    assert_ne!(response_request_id, "invalid/request/id");
    assert!(ProductRequestId::parse(response_request_id).is_ok());
    let observed = facade.mutation_request_ids.lock().unwrap();
    assert_eq!(
        observed.as_slice(),
        &[("promote".to_string(), response_request_id.to_string())]
    );
}

#[tokio::test]
async fn oauth_callback_sets_session_and_clears_nonce_cookie() {
    let request = request_builder(
        "GET",
        &format!("/oauth/discord/callback?code=one-time-code&state={STATE}"),
    )
    .header("cookie", format!("__Host-starring_oauth={NONCE}"))
    .body(Body::empty())
    .unwrap();
    let response = app(Arc::new(FakeFacade::default()))
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()["location"], "/app");
    let cookies = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|value| value.to_str().unwrap())
        .collect::<Vec<_>>();
    assert!(response
        .headers()
        .get_all("set-cookie")
        .iter()
        .all(|value| value.is_sensitive()));
    assert_eq!(cookies.len(), 3);
    assert!(cookies[0].starts_with("__Host-starring_session="));
    assert!(cookies[1].starts_with("__Host-starring_csrf="));
    assert!(cookies[1].contains("SameSite=Lax"));
    assert!(!cookies[1].contains("HttpOnly"));
    assert!(cookies[2].starts_with("__Host-starring_oauth=;"));
    assert!(cookies.iter().all(|cookie| !cookie.contains("Domain=")));
}

#[tokio::test]
async fn oauth_callback_never_exposes_the_session_as_csrf() {
    let facade = Arc::new(FakeFacade::default());
    facade.identical_session_csrf.store(1, Ordering::SeqCst);
    let request = request_builder(
        "GET",
        &format!("/oauth/discord/callback?code=one-time-code&state={STATE}"),
    )
    .header("cookie", format!("__Host-starring_oauth={NONCE}"))
    .body(Body::empty())
    .unwrap();
    let response = app(facade).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let cookies = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|value| value.to_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(cookies.len(), 1);
    assert!(cookies[0].starts_with("__Host-starring_oauth=;"));
    assert!(!cookies.iter().any(|cookie| cookie.contains(SESSION)));
}

#[tokio::test]
async fn principal_requires_only_the_session_and_never_returns_csrf() {
    let facade = Arc::new(FakeFacade::default());
    let missing_session = request_builder("GET", "/v1/me")
        .header("cookie", format!("__Host-starring_csrf={CSRF}"))
        .body(Body::empty())
        .unwrap();
    let response = app(Arc::clone(&facade))
        .oneshot(missing_session)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let valid = request_builder("GET", "/v1/me")
        .header("cookie", format!("__Host-starring_session={SESSION}"))
        .body(Body::empty())
        .unwrap();
    let response = app(facade).oneshot(valid).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_text(response).await;
    assert!(body.contains("principal-1"));
    assert!(!body.contains(CSRF));
    assert!(!body.contains("csrf_token"));
}

#[tokio::test]
async fn authority_check_is_session_bound_validated_and_empty() {
    let facade = Arc::new(FakeFacade::default());
    let router = app(Arc::clone(&facade));
    let missing_session = request_builder("GET", "/v1/installations/install-1/authority-check")
        .body(Body::empty())
        .unwrap();
    let response = router.clone().oneshot(missing_session).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(facade.authority_check_calls.load(Ordering::SeqCst), 0);

    let invalid = request_builder("GET", "/v1/installations/invalid!/authority-check")
        .header("cookie", format!("__Host-starring_session={SESSION}"))
        .body(Body::empty())
        .unwrap();
    let response = router.clone().oneshot(invalid).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(facade.authority_check_calls.load(Ordering::SeqCst), 0);

    let valid = request_builder("GET", "/v1/installations/install-1/authority-check")
        .header("cookie", format!("__Host-starring_session={SESSION}"))
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(valid).await.unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(body_text(response).await, "");
    assert_eq!(facade.authority_check_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        facade
            .authority_check_installations
            .lock()
            .unwrap()
            .as_slice(),
        ["install-1"]
    );
}

#[tokio::test]
async fn authority_check_preserves_fresh_authority_fail_closed_statuses() {
    for (failure, expected) in [
        (1, StatusCode::NOT_FOUND),
        (2, StatusCode::GATEWAY_TIMEOUT),
        (3, StatusCode::SERVICE_UNAVAILABLE),
    ] {
        let facade = Arc::new(FakeFacade::default());
        facade
            .authority_check_failure
            .store(failure, Ordering::SeqCst);
        let request = request_builder("GET", "/v1/installations/install-1/authority-check")
            .header("cookie", format!("__Host-starring_session={SESSION}"))
            .body(Body::empty())
            .unwrap();
        let response = app(facade).oneshot(request).await.unwrap();
        assert_eq!(response.status(), expected);
    }
}

#[tokio::test]
async fn logout_clears_session_and_csrf_cookies() {
    let request = request_builder("POST", "/v1/logout")
        .header("origin", ORIGIN)
        .header(
            "cookie",
            format!("__Host-starring_session={SESSION}; __Host-starring_csrf={CSRF}"),
        )
        .header("x-csrf-token", CSRF)
        .body(Body::empty())
        .unwrap();
    let response = app(Arc::new(FakeFacade::default()))
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let cookies = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|value| value.to_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(cookies.len(), 2);
    assert!(cookies[0].starts_with("__Host-starring_session=;"));
    assert!(cookies[1].starts_with("__Host-starring_csrf=;"));
    assert!(cookies[1].contains("SameSite=Lax"));
    assert!(!cookies[1].contains("HttpOnly"));
}

#[tokio::test]
async fn panic_and_wrong_method_have_redacted_problem_responses() {
    let facade = Arc::new(FakeFacade::default());
    facade.panic_me.store(1, Ordering::SeqCst);
    let response = app(facade)
        .oneshot(
            request_builder("GET", "/v1/me")
                .header("cookie", format!("__Host-starring_session={SESSION}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        response.headers()["content-type"],
        "application/problem+json"
    );
    assert!(body_text(response).await.contains("internal_error"));

    let response = app(Arc::new(FakeFacade::default()))
        .oneshot(
            request_builder("DELETE", "/health/live")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert!(body_text(response).await.contains("method_not_allowed"));
}

#[test]
fn command_debug_redacts_idempotency_and_preserves_request_correlation() {
    let current = CurrentPrincipalView {
        principal_id: "principal-1".to_string(),
        display_name: "Manager".to_string(),
    };
    let command = PromoteCommand {
        request_id: ProductRequestId::parse("audit-request-1").unwrap(),
        installation_id: "install-1".to_string(),
        session_id: "session-1".to_string(),
        expected_generation: 1,
        idempotency_key: IdempotencyKey::parse("raw-idempotency-key").unwrap(),
    };
    let current_debug = format!("{current:?}");
    assert!(!current_debug.contains("principal-1"));
    assert!(!current_debug.contains("Manager"));
    let command_debug = format!("{command:?}");
    assert!(command_debug.contains("audit-request-1"));
    assert!(!command_debug.contains("raw-idempotency-key"));
    let authoring = AuthoringTurnCommandV1 {
        request_id: ProductRequestId::parse("authoring-audit-1").unwrap(),
        installation_id: "install-1".to_string(),
        session_id: "session-1".to_string(),
        expected_generation: authoring_application::AuthoringExpectedGenerationV1::new(0).unwrap(),
        idempotency_key: IdempotencyKey::parse("authoring-secret-key").unwrap(),
        message: authoring_application::AuthoringHumanMessageV1::parse("private request").unwrap(),
        commit_boundary: authoring_application::AuthoringCommitBoundaryV1::new(),
    };
    let authoring_debug = format!("{authoring:?}");
    assert!(authoring_debug.contains("authoring-audit-1"));
    assert!(!authoring_debug.contains("authoring-secret-key"));
    assert!(!authoring_debug.contains("private request"));
    assert!(SessionCredential::parse(&format!("{}B", "A".repeat(42))).is_err());
}
