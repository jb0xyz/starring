mod abuse_budget;
mod authoring;
mod boundary;
mod response_validation;

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, Extension, Path, RawQuery, State};
use axum::http::header::{ETAG, LOCATION};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tokio::sync::Semaphore;
use zeroize::Zeroizing;

use crate::facade::{is_live_exact_replay, valid_resource_id, validate_scoped_path};
use crate::{
    ApplyCommand, AuthoringHttpBoundaryConfigV1, CurrentPrincipalView, DecisionCommand,
    FacadeError, FacadeErrorCode, HttpBoundaryConfig, LifecycleCancellationCommand,
    OAuthCallbackCommand, OAuthStartCommand, ProductControlAuthoringFacadeV1, ProductControlFacade,
    ProductControlLifecycleFacadeV1, ProductControlOperationalFacadeV2, PromoteCommand,
    RejectCommand, SessionCredential,
};
use abuse_budget::{OAuthStartAdmission, OAuthStartBudget};
use authoring::{authoring_session, authoring_turn};
use boundary::*;
use response_validation::{
    valid_apply_view, valid_approval_view, valid_current_principal, valid_decision_view,
    valid_deployment_operational_view_v2, valid_deployment_view, valid_lifecycle_cancellation_view,
    valid_preview_view, valid_promotion_view, valid_rejection_view,
};

pub(super) const AUTHORING_TURN_PATH_V1: &str =
    "/v1/installations/{installation_id}/authoring/sessions/{session_id}/turns";
pub(super) const AUTHORING_SESSION_PATH_V1: &str =
    "/v1/installations/{installation_id}/authoring/sessions/{session_id}";

struct HttpState<F> {
    facade: Arc<F>,
    config: HttpBoundaryConfig,
    authoring_config: Option<AuthoringHttpBoundaryConfigV1>,
    in_flight: Arc<Semaphore>,
    authoring_in_flight: Option<Arc<Semaphore>>,
    readiness_gate: crate::ProductApiReadinessGate,
    oauth_start_budget: Arc<OAuthStartBudget>,
}

impl<F> Clone for HttpState<F> {
    fn clone(&self) -> Self {
        Self {
            facade: Arc::clone(&self.facade),
            config: self.config.clone(),
            authoring_config: self.authoring_config,
            in_flight: Arc::clone(&self.in_flight),
            authoring_in_flight: self.authoring_in_flight.as_ref().map(Arc::clone),
            readiness_gate: self.readiness_gate.clone(),
            oauth_start_budget: Arc::clone(&self.oauth_start_budget),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PromotionBody {
    expected_generation: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DigestBody {
    expected_payload_digest: String,
    expected_revision: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RejectionBody {
    expected_payload_digest: String,
    expected_revision: u64,
    reason: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LifecycleCancellationBody {
    expected_payload_digest: String,
    expected_revision: u64,
    drain_intent_id: String,
    acknowledged_intent_revision: u64,
    acknowledged_state_digest: String,
    product_operation_id: String,
    expected_runtime_deployment_revision: u64,
    reason: String,
}

pub fn product_control_router<F>(facade: Arc<F>, config: HttpBoundaryConfig) -> Router
where
    F: ProductControlFacade,
{
    product_control_router_with_readiness_gate(
        facade,
        config,
        crate::ProductApiReadinessGate::always_ready(),
    )
}

pub fn product_control_router_with_readiness_gate<F>(
    facade: Arc<F>,
    config: HttpBoundaryConfig,
    readiness_gate: crate::ProductApiReadinessGate,
) -> Router
where
    F: ProductControlFacade,
{
    let state = http_state(facade, config, readiness_gate);
    finish_product_control_router(product_control_routes::<F>(), state)
}

pub fn product_control_router_with_authoring_v1<F>(
    facade: Arc<F>,
    config: HttpBoundaryConfig,
    authoring_config: AuthoringHttpBoundaryConfigV1,
) -> Router
where
    F: ProductControlAuthoringFacadeV1,
{
    product_control_router_with_authoring_v1_and_readiness_gate(
        facade,
        config,
        authoring_config,
        crate::ProductApiReadinessGate::always_ready(),
    )
}

pub fn product_control_router_with_authoring_v1_and_readiness_gate<F>(
    facade: Arc<F>,
    config: HttpBoundaryConfig,
    authoring_config: AuthoringHttpBoundaryConfigV1,
    readiness_gate: crate::ProductApiReadinessGate,
) -> Router
where
    F: ProductControlAuthoringFacadeV1,
{
    let state = http_state_with_authoring(facade, config, authoring_config, readiness_gate);
    finish_product_control_router(product_control_authoring_routes::<F>(), state)
}

pub fn product_control_router_with_operational_v2<F>(
    facade: Arc<F>,
    config: HttpBoundaryConfig,
) -> Router
where
    F: ProductControlOperationalFacadeV2,
{
    product_control_router_with_operational_v2_and_readiness_gate(
        facade,
        config,
        crate::ProductApiReadinessGate::always_ready(),
    )
}

pub fn product_control_router_with_operational_v2_and_readiness_gate<F>(
    facade: Arc<F>,
    config: HttpBoundaryConfig,
    readiness_gate: crate::ProductApiReadinessGate,
) -> Router
where
    F: ProductControlOperationalFacadeV2,
{
    let state = http_state(facade, config, readiness_gate);
    let routes = product_control_routes::<F>().route(
        "/v2/installations/{installation_id}/promotions/{promotion_id}/deployment",
        get(deployment_operational_v2::<F>),
    );
    finish_product_control_router(routes, state)
}

pub fn product_control_router_with_lifecycle_v1<F>(
    facade: Arc<F>,
    config: HttpBoundaryConfig,
) -> Router
where
    F: ProductControlLifecycleFacadeV1,
{
    product_control_router_with_lifecycle_v1_and_readiness_gate(
        facade,
        config,
        crate::ProductApiReadinessGate::always_ready(),
    )
}

pub fn product_control_router_with_lifecycle_v1_and_readiness_gate<F>(
    facade: Arc<F>,
    config: HttpBoundaryConfig,
    readiness_gate: crate::ProductApiReadinessGate,
) -> Router
where
    F: ProductControlLifecycleFacadeV1,
{
    let state = http_state(facade, config, readiness_gate);
    finish_product_control_router(product_control_lifecycle_routes::<F>(), state)
}

pub fn product_control_router_with_operational_v2_and_lifecycle_v1<F>(
    facade: Arc<F>,
    config: HttpBoundaryConfig,
) -> Router
where
    F: ProductControlOperationalFacadeV2 + ProductControlLifecycleFacadeV1,
{
    product_control_router_with_operational_v2_and_lifecycle_v1_and_readiness_gate(
        facade,
        config,
        crate::ProductApiReadinessGate::always_ready(),
    )
}

pub fn product_control_router_with_operational_v2_and_lifecycle_v1_and_readiness_gate<F>(
    facade: Arc<F>,
    config: HttpBoundaryConfig,
    readiness_gate: crate::ProductApiReadinessGate,
) -> Router
where
    F: ProductControlOperationalFacadeV2 + ProductControlLifecycleFacadeV1,
{
    let state = http_state(facade, config, readiness_gate);
    let routes = product_control_lifecycle_routes::<F>().route(
        "/v2/installations/{installation_id}/promotions/{promotion_id}/deployment",
        get(deployment_operational_v2::<F>),
    );
    finish_product_control_router(routes, state)
}

pub fn product_control_router_with_operational_v2_and_lifecycle_v1_and_authoring_v1<F>(
    facade: Arc<F>,
    config: HttpBoundaryConfig,
    authoring_config: AuthoringHttpBoundaryConfigV1,
) -> Router
where
    F: ProductControlOperationalFacadeV2
        + ProductControlLifecycleFacadeV1
        + ProductControlAuthoringFacadeV1,
{
    product_control_router_with_operational_v2_and_lifecycle_v1_and_authoring_v1_and_readiness_gate(
        facade,
        config,
        authoring_config,
        crate::ProductApiReadinessGate::always_ready(),
    )
}

pub fn product_control_router_with_operational_v2_and_lifecycle_v1_and_authoring_v1_and_readiness_gate<
    F,
>(
    facade: Arc<F>,
    config: HttpBoundaryConfig,
    authoring_config: AuthoringHttpBoundaryConfigV1,
    readiness_gate: crate::ProductApiReadinessGate,
) -> Router
where
    F: ProductControlOperationalFacadeV2
        + ProductControlLifecycleFacadeV1
        + ProductControlAuthoringFacadeV1,
{
    let state = http_state_with_authoring(facade, config, authoring_config, readiness_gate);
    let routes = product_control_authoring_lifecycle_routes::<F>().route(
        "/v2/installations/{installation_id}/promotions/{promotion_id}/deployment",
        get(deployment_operational_v2::<F>),
    );
    finish_product_control_router(routes, state)
}

fn http_state<F>(
    facade: Arc<F>,
    config: HttpBoundaryConfig,
    readiness_gate: crate::ProductApiReadinessGate,
) -> HttpState<F> {
    HttpState {
        facade,
        authoring_config: None,
        in_flight: Arc::new(Semaphore::new(config.max_in_flight())),
        authoring_in_flight: None,
        readiness_gate,
        oauth_start_budget: Arc::new(OAuthStartBudget::new(config.oauth_start_budget())),
        config,
    }
}

fn http_state_with_authoring<F>(
    facade: Arc<F>,
    config: HttpBoundaryConfig,
    authoring_config: AuthoringHttpBoundaryConfigV1,
    readiness_gate: crate::ProductApiReadinessGate,
) -> HttpState<F> {
    let mut state = http_state(facade, config, readiness_gate);
    state.authoring_config = Some(authoring_config);
    state.authoring_in_flight = Some(Arc::new(Semaphore::new(authoring_config.max_in_flight())));
    state
}

fn product_control_routes<F>() -> Router<HttpState<F>>
where
    F: ProductControlFacade,
{
    Router::new()
        .route("/oauth/discord/start", get(oauth_start::<F>))
        .route("/oauth/discord/callback", get(oauth_callback::<F>))
        .route("/v1/me", get(current_principal::<F>))
        .route(
            "/v1/installations/{installation_id}/authority-check",
            get(authority_check::<F>),
        )
        .route("/v1/logout", post(logout::<F>))
        .route(
            "/v1/installations/{installation_id}/authoring/sessions/{session_id}/promotions",
            post(promote::<F>),
        )
        .route(
            "/v1/installations/{installation_id}/promotions/{promotion_id}",
            get(status::<F>),
        )
        .route(
            "/v1/installations/{installation_id}/promotions/{promotion_id}/approval-preview",
            get(approval_preview::<F>),
        )
        .route(
            "/v1/installations/{installation_id}/promotions/{promotion_id}/approvals",
            post(approve::<F>),
        )
        .route(
            "/v1/installations/{installation_id}/promotions/{promotion_id}/rejections",
            post(reject::<F>),
        )
        .route(
            "/v1/installations/{installation_id}/promotions/{promotion_id}/apply",
            post(apply::<F>),
        )
        .route(
            "/v1/installations/{installation_id}/promotions/{promotion_id}/deployment",
            get(deployment::<F>),
        )
        .route("/health/live", get(liveness))
        .route("/health/ready", get(readiness::<F>))
        .method_not_allowed_fallback(method_not_allowed)
        .fallback(not_found)
}

fn product_control_lifecycle_routes<F>() -> Router<HttpState<F>>
where
    F: ProductControlLifecycleFacadeV1,
{
    product_control_routes::<F>().route(
        "/v1/installations/{installation_id}/promotions/{promotion_id}/lifecycle-cancellations",
        post(cancel_lifecycle::<F>),
    )
}

fn product_control_authoring_routes<F>() -> Router<HttpState<F>>
where
    F: ProductControlAuthoringFacadeV1,
{
    product_control_routes::<F>()
        .route(AUTHORING_TURN_PATH_V1, post(authoring_turn::<F>))
        .route(AUTHORING_SESSION_PATH_V1, get(authoring_session::<F>))
}

fn product_control_authoring_lifecycle_routes<F>() -> Router<HttpState<F>>
where
    F: ProductControlAuthoringFacadeV1 + ProductControlLifecycleFacadeV1,
{
    product_control_authoring_routes::<F>().route(
        "/v1/installations/{installation_id}/promotions/{promotion_id}/lifecycle-cancellations",
        post(cancel_lifecycle::<F>),
    )
}

fn finish_product_control_router<F>(routes: Router<HttpState<F>>, state: HttpState<F>) -> Router
where
    F: ProductControlFacade,
{
    routes
        .with_state(state.clone())
        .layer(DefaultBodyLimit::max(state.config.body_limit()))
        .layer(middleware::from_fn_with_state(
            state,
            resource_boundary::<F>,
        ))
        .layer(middleware::from_fn(request_boundary))
}

async fn oauth_start<F>(
    State(state): State<HttpState<F>>,
    Extension(request_id): Extension<RequestId>,
    RawQuery(query): RawQuery,
) -> Response
where
    F: ProductControlFacade,
{
    let return_to = match parse_start_query(query.as_deref()) {
        Ok(value) => value,
        Err(()) => return malformed_query(&request_id),
    };
    if return_to
        .as_deref()
        .is_some_and(|path| !state.config.allows_return_path(path))
    {
        return malformed_query(&request_id);
    }
    match state.oauth_start_budget.try_acquire() {
        OAuthStartAdmission::Admitted => {}
        OAuthStartAdmission::Rejected {
            retry_after_seconds,
        } => return oauth_start_rate_limited(retry_after_seconds, &request_id),
        OAuthStartAdmission::Unavailable => {
            return map_facade(FacadeError::new(FacadeErrorCode::Internal), &request_id)
        }
    }
    match state
        .facade
        .oauth_start(OAuthStartCommand { return_to })
        .await
    {
        Ok(output) => {
            if output.max_age_seconds == 0
                || output.max_age_seconds > 600
                || output.authorization_state == output.browser_nonce
            {
                return map_facade(FacadeError::new(FacadeErrorCode::Internal), &request_id);
            }
            let location = match discord_authorization_location(
                &output.authorization_request,
                &output.authorization_state,
                state.config.oauth_callback_url(),
            ) {
                Some(value) => value,
                None => {
                    return map_facade(FacadeError::new(FacadeErrorCode::Internal), &request_id)
                }
            };
            let location = match HeaderValue::from_str(location.as_str()) {
                Ok(mut value) => {
                    value.set_sensitive(true);
                    value
                }
                Err(_) => {
                    return map_facade(FacadeError::new(FacadeErrorCode::Internal), &request_id)
                }
            };
            let mut response = StatusCode::FOUND.into_response();
            response.headers_mut().insert(LOCATION, location);
            append_cookie(
                &mut response,
                secure_http_only_cookie(
                    OAUTH_COOKIE,
                    output.browser_nonce.expose_secret(),
                    output.max_age_seconds,
                ),
            );
            response
        }
        Err(error) => map_facade(error, &request_id),
    }
}

fn oauth_start_rate_limited(retry_after_seconds: u64, request_id: &RequestId) -> Response {
    let mut response = problem(
        StatusCode::TOO_MANY_REQUESTS,
        "oauth_start_rate_limited",
        "OAuth sign-in is temporarily rate limited.",
        true,
        request_id,
    );
    if let Ok(value) = HeaderValue::from_str(&retry_after_seconds.to_string()) {
        response.headers_mut().insert("retry-after", value);
    }
    response
}

async fn oauth_callback<F>(
    State(state): State<HttpState<F>>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response
where
    F: ProductControlFacade,
{
    let query = query.map(Zeroizing::new);
    let (code, oauth_state) = match parse_callback_query(query.as_ref().map(|value| value.as_str()))
    {
        Ok(value) => value,
        Err(()) => return malformed_oauth_callback(&request_id),
    };
    let nonce = match cookie_secret(&headers, OAUTH_COOKIE)
        .and_then(|value| crate::OAuthState::parse(&value).map_err(|_| CookieReadError::Invalid))
    {
        Ok(value) => value,
        Err(_) => return malformed_oauth_callback(&request_id),
    };
    let command = OAuthCallbackCommand {
        code,
        state: oauth_state,
        browser_nonce: nonce,
    };
    match state.facade.oauth_callback(command).await {
        Ok(output) => {
            if output.max_age_seconds == 0
                || output.max_age_seconds > 43_200
                || !state.config.allows_return_path(&output.return_to)
                || crate::secret::constant_time_secret_eq(
                    output.session.expose_secret(),
                    output.csrf.expose_secret(),
                )
            {
                let _ = state
                    .facade
                    .revoke_session(&output.session, &output.csrf)
                    .await;
                let mut response =
                    map_facade(FacadeError::new(FacadeErrorCode::Internal), &request_id);
                append_cookie(&mut response, clear_cookie(OAUTH_COOKIE));
                return response;
            }
            let location = match HeaderValue::from_str(&output.return_to) {
                Ok(mut value) => {
                    value.set_sensitive(true);
                    value
                }
                Err(_) => {
                    let _ = state
                        .facade
                        .revoke_session(&output.session, &output.csrf)
                        .await;
                    let mut response =
                        map_facade(FacadeError::new(FacadeErrorCode::Internal), &request_id);
                    append_cookie(&mut response, clear_cookie(OAUTH_COOKIE));
                    return response;
                }
            };
            let mut response = StatusCode::SEE_OTHER.into_response();
            response.headers_mut().insert(LOCATION, location);
            append_cookie(
                &mut response,
                secure_http_only_cookie(
                    SESSION_COOKIE,
                    output.session.expose_secret(),
                    output.max_age_seconds,
                ),
            );
            append_cookie(
                &mut response,
                secure_csrf_cookie(output.csrf.expose_secret(), output.max_age_seconds),
            );
            append_cookie(&mut response, clear_cookie(OAUTH_COOKIE));
            response
        }
        Err(error) => {
            let mut response = map_facade(error, &request_id);
            append_cookie(&mut response, clear_cookie(OAUTH_COOKIE));
            response
        }
    }
}

fn malformed_oauth_callback(request_id: &RequestId) -> Response {
    let mut response = malformed_query(request_id);
    append_cookie(&mut response, clear_cookie(OAUTH_COOKIE));
    response
}

async fn current_principal<F>(
    State(state): State<HttpState<F>>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
) -> Response
where
    F: ProductControlFacade,
{
    let credential = match session_credential(&headers, &request_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    match state.facade.current_principal(&credential).await {
        Ok(principal) => {
            let view = CurrentPrincipalView {
                principal_id: principal.principal_id,
                display_name: principal.display_name,
            };
            if valid_current_principal(&view) {
                Json(view).into_response()
            } else {
                map_facade(FacadeError::new(FacadeErrorCode::Internal), &request_id)
            }
        }
        Err(error) => map_facade(error, &request_id),
    }
}

async fn authority_check<F>(
    State(state): State<HttpState<F>>,
    Extension(request_id): Extension<RequestId>,
    Path(installation_id): Path<String>,
    headers: HeaderMap,
) -> Response
where
    F: ProductControlFacade,
{
    let credential = match session_credential(&headers, &request_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    if !valid_resource_id(&installation_id) {
        return invalid_input(&request_id);
    }
    match state
        .facade
        .authority_check(&credential, &installation_id)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => map_facade(error, &request_id),
    }
}

async fn logout<F>(
    State(state): State<HttpState<F>>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
) -> Response
where
    F: ProductControlFacade,
{
    let (credential, csrf) = match mutation_credential(&state, &headers, &request_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    match state.facade.revoke_session(&credential, &csrf).await {
        Ok(()) => {
            let mut response = StatusCode::NO_CONTENT.into_response();
            append_cookie(&mut response, clear_cookie(SESSION_COOKIE));
            append_cookie(&mut response, clear_csrf_cookie());
            response
        }
        Err(error) => map_facade(error, &request_id),
    }
}

async fn promote<F>(
    State(state): State<HttpState<F>>,
    Extension(request_id): Extension<RequestId>,
    Path((installation_id, session_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Result<Json<PromotionBody>, JsonRejection>,
) -> Response
where
    F: ProductControlFacade,
{
    if let Err(response) = require_json(&headers, &request_id) {
        return response;
    }
    let (credential, csrf) = match mutation_credential(&state, &headers, &request_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let body = match parse_json(body, &request_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let idempotency_key = match idempotency_key(&headers, &request_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let command = PromoteCommand {
        request_id: request_id.clone(),
        installation_id,
        session_id,
        expected_generation: body.expected_generation,
        idempotency_key,
    };
    if !command.validate() {
        return invalid_input(&request_id);
    }
    let expected_installation = command.installation_id.clone();
    match state.facade.promote(&credential, &csrf, command).await {
        Ok(view) if valid_promotion_view(&view, &expected_installation) => {
            let status = if view.replayed {
                StatusCode::OK
            } else {
                StatusCode::CREATED
            };
            (status, Json(view)).into_response()
        }
        Ok(_) => map_facade(FacadeError::new(FacadeErrorCode::Internal), &request_id),
        Err(error) => map_facade(error, &request_id),
    }
}

async fn status<F>(
    State(state): State<HttpState<F>>,
    Extension(request_id): Extension<RequestId>,
    Path((installation_id, promotion_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response
where
    F: ProductControlFacade,
{
    let credential = match session_credential(&headers, &request_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    if !validate_scoped_path(&installation_id, &promotion_id) {
        return invalid_input(&request_id);
    }
    match state
        .facade
        .status(&credential, &installation_id, &promotion_id)
        .await
    {
        Ok(view) if valid_decision_view(&view, &installation_id, &promotion_id) => {
            Json(view).into_response()
        }
        Ok(_) => map_facade(FacadeError::new(FacadeErrorCode::Internal), &request_id),
        Err(error) => map_facade(error, &request_id),
    }
}

async fn approval_preview<F>(
    State(state): State<HttpState<F>>,
    Extension(request_id): Extension<RequestId>,
    Path((installation_id, promotion_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response
where
    F: ProductControlFacade,
{
    let credential = match session_credential(&headers, &request_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    if !validate_scoped_path(&installation_id, &promotion_id) {
        return invalid_input(&request_id);
    }
    match state
        .facade
        .approval_preview(&credential, &installation_id, &promotion_id)
        .await
    {
        Ok(view) if valid_preview_view(&view, &installation_id, &promotion_id) => {
            let etag = match HeaderValue::from_str(&format!("\"{}\"", view.payload_digest)) {
                Ok(value) => value,
                Err(_) => {
                    return map_facade(FacadeError::new(FacadeErrorCode::Internal), &request_id)
                }
            };
            let mut response = Json(view).into_response();
            response.headers_mut().insert(ETAG, etag);
            response
        }
        Ok(_) => map_facade(FacadeError::new(FacadeErrorCode::Internal), &request_id),
        Err(error) => map_facade(error, &request_id),
    }
}

async fn approve<F>(
    State(state): State<HttpState<F>>,
    Extension(request_id): Extension<RequestId>,
    Path((installation_id, promotion_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Result<Json<DigestBody>, JsonRejection>,
) -> Response
where
    F: ProductControlFacade,
{
    let (credential, csrf, command) = match decision_request(
        &state,
        &request_id,
        &headers,
        body,
        installation_id,
        promotion_id,
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let expected_installation = command.installation_id.clone();
    let expected_promotion = command.promotion_id.clone();
    match state.facade.approve(&credential, &csrf, command).await {
        Ok(view) if valid_approval_view(&view, &expected_installation, &expected_promotion) => {
            Json(view).into_response()
        }
        Ok(_) => map_facade(FacadeError::new(FacadeErrorCode::Internal), &request_id),
        Err(error) => map_facade(error, &request_id),
    }
}

async fn reject<F>(
    State(state): State<HttpState<F>>,
    Extension(request_id): Extension<RequestId>,
    Path((installation_id, promotion_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Result<Json<RejectionBody>, JsonRejection>,
) -> Response
where
    F: ProductControlFacade,
{
    if let Err(response) = require_json(&headers, &request_id) {
        return response;
    }
    let (credential, csrf) = match mutation_credential(&state, &headers, &request_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let body = match parse_json(body, &request_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let idempotency_key = match idempotency_key(&headers, &request_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let command = RejectCommand {
        decision: DecisionCommand {
            request_id: request_id.clone(),
            installation_id,
            promotion_id,
            expected_payload_digest: body.expected_payload_digest,
            expected_revision: body.expected_revision,
            idempotency_key,
        },
        reason: body.reason,
    };
    let Some(command) = command.normalize() else {
        return invalid_input(&request_id);
    };
    let expected_installation = command.decision.installation_id.clone();
    let expected_promotion = command.decision.promotion_id.clone();
    match state.facade.reject(&credential, &csrf, command).await {
        Ok(view) if valid_rejection_view(&view, &expected_installation, &expected_promotion) => {
            Json(view).into_response()
        }
        Ok(_) => map_facade(FacadeError::new(FacadeErrorCode::Internal), &request_id),
        Err(error) => map_facade(error, &request_id),
    }
}

async fn apply<F>(
    State(state): State<HttpState<F>>,
    Extension(request_id): Extension<RequestId>,
    Path((installation_id, promotion_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Result<Json<DigestBody>, JsonRejection>,
) -> Response
where
    F: ProductControlFacade,
{
    let (credential, csrf, decision) = match decision_request(
        &state,
        &request_id,
        &headers,
        body,
        installation_id,
        promotion_id,
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let expected_installation = decision.installation_id.clone();
    let expected_promotion = decision.promotion_id.clone();
    match state
        .facade
        .apply(&credential, &csrf, ApplyCommand { decision })
        .await
    {
        Ok(view) if valid_apply_view(&view, &expected_installation, &expected_promotion) => {
            let status = if is_live_exact_replay(&view) {
                StatusCode::OK
            } else {
                StatusCode::ACCEPTED
            };
            (status, Json(view)).into_response()
        }
        Ok(_) => map_facade(FacadeError::new(FacadeErrorCode::Internal), &request_id),
        Err(error) => map_facade(error, &request_id),
    }
}

async fn cancel_lifecycle<F>(
    State(state): State<HttpState<F>>,
    Extension(request_id): Extension<RequestId>,
    Path((installation_id, promotion_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Result<Json<LifecycleCancellationBody>, JsonRejection>,
) -> Response
where
    F: ProductControlLifecycleFacadeV1,
{
    if let Err(response) = require_json(&headers, &request_id) {
        return response;
    }
    let (credential, csrf) = match mutation_credential(&state, &headers, &request_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let body = match parse_json(body, &request_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let idempotency_key = match idempotency_key(&headers, &request_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let command = LifecycleCancellationCommand {
        decision: DecisionCommand {
            request_id: request_id.clone(),
            installation_id,
            promotion_id,
            expected_payload_digest: body.expected_payload_digest,
            expected_revision: body.expected_revision,
            idempotency_key,
        },
        drain_intent_id: body.drain_intent_id,
        acknowledged_intent_revision: body.acknowledged_intent_revision,
        acknowledged_state_digest: body.acknowledged_state_digest,
        product_operation_id: body.product_operation_id,
        expected_runtime_deployment_revision: body.expected_runtime_deployment_revision,
        reason: body.reason,
    };
    let Some(command) = command.normalize() else {
        return invalid_input(&request_id);
    };
    let expected_installation = command.decision.installation_id.clone();
    let expected_promotion = command.decision.promotion_id.clone();
    match state
        .facade
        .cancel_lifecycle(&credential, &csrf, command)
        .await
    {
        Ok(view)
            if valid_lifecycle_cancellation_view(
                &view,
                &expected_installation,
                &expected_promotion,
            ) =>
        {
            Json(view).into_response()
        }
        Ok(_) => map_facade(FacadeError::new(FacadeErrorCode::Internal), &request_id),
        Err(error) => map_facade(error, &request_id),
    }
}

async fn deployment<F>(
    State(state): State<HttpState<F>>,
    Extension(request_id): Extension<RequestId>,
    Path((installation_id, promotion_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response
where
    F: ProductControlFacade,
{
    let credential = match session_credential(&headers, &request_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    if !validate_scoped_path(&installation_id, &promotion_id) {
        return invalid_input(&request_id);
    }
    match state
        .facade
        .deployment(&credential, &installation_id, &promotion_id)
        .await
    {
        Ok(view) if valid_deployment_view(&view, &installation_id, &promotion_id) => {
            Json(view).into_response()
        }
        Ok(_) => map_facade(FacadeError::new(FacadeErrorCode::Internal), &request_id),
        Err(error) => map_facade(error, &request_id),
    }
}

async fn deployment_operational_v2<F>(
    State(state): State<HttpState<F>>,
    Extension(request_id): Extension<RequestId>,
    Path((installation_id, promotion_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response
where
    F: ProductControlOperationalFacadeV2,
{
    let credential = match session_credential(&headers, &request_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    if !validate_scoped_path(&installation_id, &promotion_id) {
        return invalid_input(&request_id);
    }
    match state
        .facade
        .deployment_operational_v2(&credential, &installation_id, &promotion_id)
        .await
    {
        Ok(view)
            if valid_deployment_operational_view_v2(&view, &installation_id, &promotion_id) =>
        {
            Json(view).into_response()
        }
        Ok(_) => map_facade(FacadeError::new(FacadeErrorCode::Internal), &request_id),
        Err(error) => map_facade(error, &request_id),
    }
}

async fn liveness() -> StatusCode {
    StatusCode::OK
}

async fn readiness<F>(
    State(state): State<HttpState<F>>,
    Extension(request_id): Extension<RequestId>,
) -> Response
where
    F: ProductControlFacade,
{
    if !state.readiness_gate.is_ready() {
        return map_facade(
            FacadeError::new(FacadeErrorCode::DependencyUnavailable),
            &request_id,
        );
    }
    StatusCode::OK.into_response()
}

async fn not_found(Extension(request_id): Extension<RequestId>) -> Response {
    problem(
        StatusCode::NOT_FOUND,
        "route_not_found",
        "The requested route does not exist.",
        false,
        &request_id,
    )
}

async fn method_not_allowed(Extension(request_id): Extension<RequestId>) -> Response {
    problem(
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        "The request method is not allowed for this route.",
        false,
        &request_id,
    )
}

async fn decision_request<F>(
    state: &HttpState<F>,
    request_id: &RequestId,
    headers: &HeaderMap,
    body: Result<Json<DigestBody>, JsonRejection>,
    installation_id: String,
    promotion_id: String,
) -> Result<(SessionCredential, crate::CsrfSecret, DecisionCommand), Response>
where
    F: ProductControlFacade,
{
    require_json(headers, request_id)?;
    let (credential, csrf) = mutation_credential(state, headers, request_id)?;
    let body = parse_json(body, request_id)?;
    let idempotency_key = idempotency_key(headers, request_id)?;
    let command = DecisionCommand {
        request_id: request_id.clone(),
        installation_id,
        promotion_id,
        expected_payload_digest: body.expected_payload_digest,
        expected_revision: body.expected_revision,
        idempotency_key,
    };
    if !command.validate() {
        return Err(invalid_input(request_id));
    }
    Ok((credential, csrf, command))
}
