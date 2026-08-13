use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use axum::Router;
use http_body_util::BodyExt;
use starring_api::product_web_router_v1;
use tower::ServiceExt;

#[tokio::test]
async fn frontend_assets_are_same_origin_embedded_and_strictly_hardened() {
    let app = product_web_router_v1(
        Router::new().route("/health/live", get(|| async { StatusCode::NO_CONTENT })),
    );
    for (path, content_type) in [
        ("/app", "text/html; charset=utf-8"),
        ("/app/app.css", "text/css; charset=utf-8"),
        ("/app/app.js", "text/javascript; charset=utf-8"),
        ("/app/api.js", "text/javascript; charset=utf-8"),
        ("/app/flow.js", "text/javascript; charset=utf-8"),
        ("/app/projection.js", "text/javascript; charset=utf-8"),
    ] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(response.headers()["content-type"], content_type);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(response.headers()["x-frame-options"], "DENY");
        assert_eq!(
            response.headers()["strict-transport-security"],
            "max-age=31536000; includeSubDomains"
        );
        assert_eq!(
            response.headers()["cross-origin-opener-policy"],
            "same-origin"
        );
        assert_eq!(
            response.headers()["cross-origin-resource-policy"],
            "same-origin"
        );
        let csp = response.headers()["content-security-policy"]
            .to_str()
            .unwrap();
        assert!(csp.contains("script-src 'self'"));
        assert!(csp.contains("style-src 'self'"));
        assert!(!csp.contains("'unsafe-inline'"));
    }
}

#[tokio::test]
async fn frontend_has_no_inline_code_or_recipe_specific_branching() {
    let app = product_web_router_v1(Router::new());
    let response = app
        .clone()
        .oneshot(Request::builder().uri("/app").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let html = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("src=\"/app/app.js\""));
    assert!(html.contains("href=\"/app/app.css\""));
    assert!(!html.contains("<script>"));
    assert!(!html.contains("style=\""));

    for path in ["/app/app.js", "/app/projection.js"] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let source = String::from_utf8(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
        assert!(!source.contains("study_room"));
        assert!(!source.contains("private_study"));
    }
}

#[tokio::test]
async fn api_routes_remain_owned_by_the_merged_api_router() {
    let app = product_web_router_v1(
        Router::new().route("/health/live", get(|| async { StatusCode::NO_CONTENT })),
    );
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health/live")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn api_fallback_remains_owned_by_the_merged_api_router() {
    let api = Router::new().fallback(|| async { (StatusCode::IM_A_TEAPOT, "api-fallback") });
    let response = product_web_router_v1(api)
        .oneshot(
            Request::builder()
                .uri("/unknown-api-route")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::IM_A_TEAPOT);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.as_ref(), b"api-fallback");
}
