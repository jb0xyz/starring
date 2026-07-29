use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Path, RawQuery, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use super::boundary::*;
use super::response_validation::{valid_authoring_session_view, valid_authoring_turn_view};
use super::HttpState;
use crate::facade::valid_resource_id;
use crate::{
    AuthoringHttpBoundaryConfigV1, AuthoringTurnCommandV1, AuthoringTurnDispositionV1, FacadeError,
    FacadeErrorCode, ProductControlAuthoringFacadeV1,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AuthoringTurnBody {
    expected_generation: u64,
    message: String,
}

pub(super) async fn authoring_turn<F>(
    State(state): State<HttpState<F>>,
    Extension(request_id): Extension<RequestId>,
    Extension(commit_boundary): Extension<authoring_application::AuthoringCommitBoundaryV1>,
    Path((installation_id, session_id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Result<Json<AuthoringTurnBody>, JsonRejection>,
) -> Response
where
    F: ProductControlAuthoringFacadeV1,
{
    if query.is_some() {
        return malformed_query(&request_id);
    }
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
    if !valid_resource_id(&installation_id) || !valid_resource_id(&session_id) {
        return invalid_input(&request_id);
    }
    let expected_generation =
        match authoring_application::AuthoringExpectedGenerationV1::new(body.expected_generation) {
            Ok(value) => value,
            Err(_) => return invalid_input(&request_id),
        };
    let message = match authoring_application::AuthoringHumanMessageV1::parse(&body.message) {
        Ok(value) => value,
        Err(_) => return invalid_input(&request_id),
    };
    let expected_generation_value = expected_generation.get();
    let expected_session = session_id.clone();
    let command = AuthoringTurnCommandV1 {
        request_id: request_id.clone(),
        installation_id,
        session_id,
        expected_generation,
        idempotency_key,
        message,
        commit_boundary,
    };
    match state
        .facade
        .authoring_turn(&credential, &csrf, command)
        .await
    {
        Ok(view)
            if valid_authoring_turn_view(&view, &expected_session, expected_generation_value) =>
        {
            let status = match view.disposition {
                Some(AuthoringTurnDispositionV1::Created) => StatusCode::CREATED,
                Some(AuthoringTurnDispositionV1::ExactReplay) | None => StatusCode::OK,
            };
            (status, Json(view)).into_response()
        }
        Ok(_) => map_facade(FacadeError::new(FacadeErrorCode::Internal), &request_id),
        Err(error) => map_authoring_facade(&state, error, &request_id),
    }
}

pub(super) async fn authoring_session<F>(
    State(state): State<HttpState<F>>,
    Extension(request_id): Extension<RequestId>,
    Path((installation_id, session_id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response
where
    F: ProductControlAuthoringFacadeV1,
{
    if query.is_some() {
        return malformed_query(&request_id);
    }
    let credential = match session_credential(&headers, &request_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    if !valid_resource_id(&installation_id) || !valid_resource_id(&session_id) {
        return invalid_input(&request_id);
    }
    match state
        .facade
        .authoring_session(&credential, &installation_id, &session_id)
        .await
    {
        Ok(view) if valid_authoring_session_view(&view, &session_id) => Json(view).into_response(),
        Ok(_) => map_facade(FacadeError::new(FacadeErrorCode::Internal), &request_id),
        Err(error) => map_authoring_read_facade(error, &request_id),
    }
}

fn map_authoring_facade<F>(
    state: &HttpState<F>,
    error: FacadeError,
    request_id: &RequestId,
) -> Response {
    let mut response = map_facade(error, request_id);
    if error.error_code() == FacadeErrorCode::AuthoringSaturated {
        let retry_after_seconds = state
            .authoring_config
            .map(AuthoringHttpBoundaryConfigV1::retry_after_seconds)
            .unwrap_or(1);
        if let Ok(value) = HeaderValue::from_str(&retry_after_seconds.to_string()) {
            response.headers_mut().insert("retry-after", value);
        }
    }
    response
}

fn map_authoring_read_facade(error: FacadeError, request_id: &RequestId) -> Response {
    if matches!(
        error.error_code(),
        FacadeErrorCode::Forbidden
            | FacadeErrorCode::NotFound
            | FacadeErrorCode::InvalidState
            | FacadeErrorCode::InvalidServerCandidate
    ) {
        map_facade(FacadeError::new(FacadeErrorCode::NotFound), request_id)
    } else {
        map_facade(error, request_id)
    }
}
