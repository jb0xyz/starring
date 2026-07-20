use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{DateTime, Utc};
use http_body_util::BodyExt;
use product_control_http::{
    product_control_router, product_control_router_with_operational_v2,
    product_control_router_with_operational_v2_and_readiness_gate, ApplyCommand, ApplyView,
    ApprovalPreviewView, CsrfSecret, CurrentPrincipal, CurrentPrincipalView, DecisionCommand,
    DecisionView, DeploymentAttestationViewV2, DeploymentFailureViewV2,
    DeploymentOperationalStateV2, DeploymentOperationalViewV2, DeploymentOperatorActionV2,
    DeploymentRetryStateV2, DeploymentRetryViewV2, DeploymentRuntimePhaseV2,
    DeploymentServingFreshnessStateV2, DeploymentServingFreshnessViewV2, DeploymentState,
    DeploymentView, DiscordAuthorizationRequest, FacadeError, FacadeErrorCode, HttpBoundaryConfig,
    IdempotencyKey, OAuthCallbackCommand, OAuthCallbackResult, OAuthStartCommand, OAuthStartResult,
    ProductApiReadinessGate, ProductControlFacade, ProductControlOperationalFacadeV2,
    ProductRequestId, ProductState, PromoteCommand, PromotionView, RejectCommand,
    RuntimeDeploymentOperationalViewV2, SafeApprovalSummary, SessionCredential,
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
    verify_calls: AtomicUsize,
    readiness_calls: AtomicUsize,
    promote_calls: AtomicUsize,
    fail_me: AtomicUsize,
    panic_ready: AtomicUsize,
    block_readiness: AtomicUsize,
    readiness_entered: Notify,
    readiness_release: Notify,
    disallowed_return: AtomicUsize,
    invalid_client_id: AtomicUsize,
    invalid_callback_url: AtomicUsize,
    identical_session_csrf: AtomicUsize,
    approval_response: AtomicUsize,
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

    fn record_request_id(&self, mutation: &str, request_id: &ProductRequestId) {
        self.mutation_request_ids
            .lock()
            .unwrap()
            .push((mutation.to_string(), request_id.as_str().to_string()));
    }
}

#[async_trait]
impl ProductControlFacade for FakeFacade {
    async fn oauth_start(
        &self,
        _command: OAuthStartCommand,
    ) -> Result<OAuthStartResult, FacadeError> {
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
    ) -> Result<DecisionView, FacadeError> {
        Ok(self.decision(ProductState::Approved))
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
        assert_eq!(self.panic_ready.load(Ordering::SeqCst), 0);
        if self.block_readiness.load(Ordering::SeqCst) != 0 {
            self.readiness_entered.notify_one();
            self.readiness_release.notified().await;
        }
        Ok(())
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
        for forbidden in [
            SESSION,
            CSRF,
            "controller_id",
            "fencing_token",
            "failure_id",
            "failure_message",
            "attestation_id",
            "process_instance_id",
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
    });
    runtime.serving.state = DeploymentServingFreshnessStateV2::LeaseMissing;

    let mut malformed_freshness = pending_operational_view();
    let runtime = malformed_freshness.runtime.as_mut().unwrap();
    runtime.phase = DeploymentRuntimePhaseV2::Live;
    runtime.current_attempt = 2;
    runtime.attestation = Some(DeploymentAttestationViewV2 {
        deployment_revision: 7,
        convergence_attempt: 2,
    });
    runtime.serving = DeploymentServingFreshnessViewV2 {
        state: DeploymentServingFreshnessStateV2::Disconnected,
        last_heartbeat_at: None,
        lease_expires_at: Some(timestamp("2026-07-19T12:01:00Z")),
    };

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

async fn body_text(response: axum::response::Response) -> String {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(body.to_vec()).unwrap()
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
async fn readiness_gate_closes_before_startup_and_shutdown_without_dependency_calls() {
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
    assert_eq!(facade.readiness_calls.load(Ordering::SeqCst), 1);

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
    assert_eq!(facade.readiness_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn readiness_gate_rechecks_after_an_in_flight_dependency_probe() {
    let facade = Arc::new(FakeFacade::default());
    facade.block_readiness.store(1, Ordering::SeqCst);
    let gate = ProductApiReadinessGate::initially_unready();
    let lease = gate.claim().unwrap();
    lease.mark_ready();
    let app = operational_app_with_gate(Arc::clone(&facade), gate.clone());
    let probe = tokio::spawn(
        app.oneshot(
            request_builder("GET", "/health/ready")
                .body(Body::empty())
                .unwrap(),
        ),
    );
    facade.readiness_entered.notified().await;
    lease.mark_unready();
    facade.readiness_release.notify_one();
    let response = probe.await.unwrap().unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(facade.readiness_calls.load(Ordering::SeqCst), 1);
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
    assert!(cookies[1].contains("SameSite=Strict"));
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
    assert!(cookies[1].contains("SameSite=Strict"));
    assert!(!cookies[1].contains("HttpOnly"));
}

#[tokio::test]
async fn panic_and_wrong_method_have_redacted_problem_responses() {
    let facade = Arc::new(FakeFacade::default());
    facade.panic_ready.store(1, Ordering::SeqCst);
    let response = app(facade)
        .oneshot(
            request_builder("GET", "/health/ready")
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
    assert!(SessionCredential::parse(&format!("{}B", "A".repeat(42))).is_err());
}
