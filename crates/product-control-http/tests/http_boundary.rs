use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{DateTime, Utc};
use http_body_util::BodyExt;
use product_control_http::{
    product_control_router, ApplyCommand, ApplyView, ApprovalPreviewView, CsrfSecret,
    CurrentPrincipal, CurrentPrincipalView, DecisionCommand, DecisionView, DeploymentState,
    DeploymentView, FacadeError, FacadeErrorCode, HttpBoundaryConfig, OAuthCallbackCommand,
    OAuthCallbackResult, OAuthStartCommand, OAuthStartResult, ProductControlFacade, ProductState,
    PromoteCommand, PromotionView, RejectCommand, SafeApprovalSummary, SessionCredential,
};
use tokio::sync::Notify;
use tower::ServiceExt;
use url::Url;

const HOST: &str = "starring.example";
const ORIGIN: &str = "https://starring.example";
const SESSION: &str = "sssssssssssssssssssssssssssssssssssssssssss";
const CSRF: &str = "ccccccccccccccccccccccccccccccccccccccccccc";
const NONCE: &str = "nnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnng";
const STATE: &str = "ttttttttttttttttttttttttttttttttttttttttttg";
const PROMOTION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[derive(Default)]
struct FakeFacade {
    verify_calls: AtomicUsize,
    promote_calls: AtomicUsize,
    fail_me: AtomicUsize,
    panic_ready: AtomicUsize,
    disallowed_return: AtomicUsize,
    invalid_client_id: AtomicUsize,
    identical_session_csrf: AtomicUsize,
    block_promote: AtomicUsize,
    promote_entered: Notify,
    promote_release: Notify,
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

    fn decision(&self) -> DecisionView {
        DecisionView {
            installation_id: "install-1".to_string(),
            promotion_id: PROMOTION.to_string(),
            revision: 4,
            state: ProductState::Approved,
            replayed: false,
        }
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
            authorization_url: Url::parse(&format!(
                "https://discord.com/oauth2/authorize?client_id={client_id}&redirect_uri=https%3A%2F%2Fstarring.example%2Foauth%2Fdiscord%2Fcallback&response_type=code&scope=identify&state={STATE}"
            ))
            .unwrap(),
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
        _credential: &SessionCredential,
        csrf: &CsrfSecret,
    ) -> Result<CurrentPrincipal, FacadeError> {
        if self.fail_me.load(Ordering::SeqCst) > 0 {
            return Err(FacadeError::new(FacadeErrorCode::Internal));
        }
        if csrf.expose_secret() != CSRF {
            return Err(FacadeError::new(FacadeErrorCode::Forbidden));
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
        _command: PromoteCommand,
    ) -> Result<PromotionView, FacadeError> {
        self.verify_mutation_inputs(credential, csrf)?;
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
        Ok(self.decision())
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
        _command: DecisionCommand,
    ) -> Result<DecisionView, FacadeError> {
        self.verify_mutation_inputs(credential, csrf)?;
        Ok(self.decision())
    }

    async fn reject(
        &self,
        credential: &SessionCredential,
        csrf: &CsrfSecret,
        _command: RejectCommand,
    ) -> Result<DecisionView, FacadeError> {
        self.verify_mutation_inputs(credential, csrf)?;
        Ok(self.decision())
    }

    async fn apply(
        &self,
        credential: &SessionCredential,
        csrf: &CsrfSecret,
        _command: ApplyCommand,
    ) -> Result<ApplyView, FacadeError> {
        self.verify_mutation_inputs(credential, csrf)?;
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
        assert_eq!(self.panic_ready.load(Ordering::SeqCst), 0);
        Ok(())
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
    Request::builder()
        .method(method)
        .uri(uri)
        .header("host", HOST)
        .header("x-request-id", "test-request-1")
}

fn promotion_request() -> axum::http::request::Builder {
    request_builder(
        "POST",
        "/v1/installations/install-1/authoring/sessions/session-1/promotions",
    )
    .header("content-type", "application/json")
    .header("origin", ORIGIN)
    .header("cookie", format!("__Host-starring_session={SESSION}"))
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
async fn mutation_requires_cookie_exact_origin_and_session_csrf() {
    let facade = Arc::new(FakeFacade::default());
    let missing_origin = request_builder(
        "POST",
        "/v1/installations/install-1/authoring/sessions/session-1/promotions",
    )
    .header("content-type", "application/json")
    .header("cookie", format!("__Host-starring_session={SESSION}"))
    .header("x-csrf-token", CSRF)
    .header("idempotency-key", "request-1")
    .body(Body::from(r#"{"expected_generation":1}"#))
    .unwrap();
    let response = app(Arc::clone(&facade))
        .oneshot(missing_origin)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let missing_cookie = request_builder(
        "POST",
        "/v1/installations/install-1/authoring/sessions/session-1/promotions",
    )
    .header("content-type", "application/json")
    .header("origin", ORIGIN)
    .header("x-csrf-token", CSRF)
    .header("idempotency-key", "request-1")
    .body(Body::from(r#"{"expected_generation":1}"#))
    .unwrap();
    let response = app(Arc::clone(&facade))
        .oneshot(missing_cookie)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let missing_csrf = request_builder(
        "POST",
        "/v1/installations/install-1/authoring/sessions/session-1/promotions",
    )
    .header("content-type", "application/json")
    .header("origin", ORIGIN)
    .header("cookie", format!("__Host-starring_session={SESSION}"))
    .header("idempotency-key", "request-1")
    .body(Body::from(r#"{"expected_generation":1}"#))
    .unwrap();
    let response = app(Arc::clone(&facade))
        .oneshot(missing_csrf)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let wrong_origin = request_builder(
        "POST",
        "/v1/installations/install-1/authoring/sessions/session-1/promotions",
    )
    .header("content-type", "application/json")
    .header("origin", "https://attacker.example")
    .header("cookie", format!("__Host-starring_session={SESSION}"))
    .header("x-csrf-token", CSRF)
    .header("idempotency-key", "request-1")
    .body(Body::from(r#"{"expected_generation":1}"#))
    .unwrap();
    let response = app(Arc::clone(&facade))
        .oneshot(wrong_origin)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let stale_csrf = request_builder(
        "POST",
        "/v1/installations/install-1/authoring/sessions/session-1/promotions",
    )
    .header("content-type", "application/json")
    .header("origin", ORIGIN)
    .header("cookie", format!("__Host-starring_session={SESSION}"))
    .header(
        "x-csrf-token",
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    )
    .header("idempotency-key", "request-1")
    .body(Body::from(r#"{"expected_generation":1}"#))
    .unwrap();
    let response = app(Arc::clone(&facade)).oneshot(stale_csrf).await.unwrap();
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
    assert_eq!(facade.verify_calls.load(Ordering::SeqCst), 1);
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
}

#[tokio::test]
async fn internal_errors_are_stable_and_redacted() {
    let facade = Arc::new(FakeFacade::default());
    facade.fail_me.store(1, Ordering::SeqCst);
    let request = request_builder("GET", "/v1/me")
        .header(
            "cookie",
            format!("__Host-starring_session={SESSION}; __Host-starring_csrf={CSRF}"),
        )
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
    assert!(response.headers()["location"]
        .to_str()
        .unwrap()
        .starts_with("https://discord.com/oauth2/authorize?"));
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
        .header("cookie", format!("__Host-starring_session={SESSION}"))
        .header("x-csrf-token", CSRF)
        .header("idempotency-key", "approve-1")
        .body(Body::from(format!(
            "{{\"expected_payload_digest\":\"{DIGEST}\",\"expected_revision\":3}}"
        )))
        .unwrap();
    let response = app(Arc::clone(&facade)).oneshot(valid).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let missing_revision = request_builder("POST", &uri)
        .header("content-type", "application/json")
        .header("origin", ORIGIN)
        .header("cookie", format!("__Host-starring_session={SESSION}"))
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
        .header("cookie", format!("__Host-starring_session={SESSION}"))
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
async fn principal_requires_the_session_bound_csrf_cookie() {
    let facade = Arc::new(FakeFacade::default());
    let missing_csrf = request_builder("GET", "/v1/me")
        .header("cookie", format!("__Host-starring_session={SESSION}"))
        .body(Body::empty())
        .unwrap();
    let response = app(Arc::clone(&facade))
        .oneshot(missing_csrf)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let valid = request_builder("GET", "/v1/me")
        .header(
            "cookie",
            format!("__Host-starring_session={SESSION}; __Host-starring_csrf={CSRF}"),
        )
        .body(Body::empty())
        .unwrap();
    let response = app(facade).oneshot(valid).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_text(response).await;
    assert!(body.contains("principal-1"));
    assert!(body.contains(CSRF));
}

#[tokio::test]
async fn logout_clears_session_and_csrf_cookies() {
    let request = request_builder("POST", "/v1/logout")
        .header("origin", ORIGIN)
        .header("cookie", format!("__Host-starring_session={SESSION}"))
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
fn secret_bearing_debug_views_are_redacted() {
    let current = CurrentPrincipalView {
        principal_id: "principal-1".to_string(),
        display_name: "Manager".to_string(),
        csrf_token: CSRF.to_string(),
    };
    let command = PromoteCommand {
        installation_id: "install-1".to_string(),
        session_id: "session-1".to_string(),
        expected_generation: 1,
        idempotency_key: "raw-idempotency-key".to_string(),
    };
    assert!(!format!("{current:?}").contains(CSRF));
    assert!(!format!("{command:?}").contains("raw-idempotency-key"));
    assert!(SessionCredential::parse(&format!("{}B", "A".repeat(42))).is_err());
}
