use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

const APP_HTML: &str = include_str!("../../../web/app/index.html");
const APP_CSS: &str = include_str!("../../../web/app/app.css");
const APP_JS: &str = include_str!("../../../web/app/app.js");
const API_JS: &str = include_str!("../../../web/app/api.js");
const FLOW_JS: &str = include_str!("../../../web/app/flow.js");
const PROJECTION_JS: &str = include_str!("../../../web/app/projection.js");
const UI_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self'; font-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'; object-src 'none'";

pub fn product_web_router_v1(api: Router) -> Router {
    Router::new()
        .route("/app", get(index))
        .route("/app/", get(index))
        .route("/app/app.css", get(css))
        .route("/app/app.js", get(app_js))
        .route("/app/api.js", get(api_js))
        .route("/app/flow.js", get(flow_js))
        .route("/app/projection.js", get(projection_js))
        .merge(api)
}

async fn index() -> Response {
    asset_response("text/html; charset=utf-8", APP_HTML)
}

async fn css() -> Response {
    asset_response("text/css; charset=utf-8", APP_CSS)
}

async fn app_js() -> Response {
    asset_response("text/javascript; charset=utf-8", APP_JS)
}

async fn api_js() -> Response {
    asset_response("text/javascript; charset=utf-8", API_JS)
}

async fn flow_js() -> Response {
    asset_response("text/javascript; charset=utf-8", FLOW_JS)
}

async fn projection_js() -> Response {
    asset_response("text/javascript; charset=utf-8", PROJECTION_JS)
}

fn asset_response(content_type: &'static str, body: &'static str) -> Response {
    let mut response = (StatusCode::OK, body).into_response();
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    apply_ui_headers(&mut response);
    response
}

fn apply_ui_headers(response: &mut Response) {
    let headers = response.headers_mut();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert("content-security-policy", HeaderValue::from_static(UI_CSP));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert(
        "strict-transport-security",
        HeaderValue::from_static("max-age=31536000; includeSubDomains"),
    );
    headers.insert(
        "cross-origin-opener-policy",
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("same-origin"),
    );
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
}
