use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Path, RawQuery, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use super::boundary::*;
use super::HttpState;
use crate::facade::valid_resource_id;
use crate::{
    AutomationSpecInstallationReadinessV1, AutomationSpecPreviewResponseV1,
    AutomationSpecSimulationResponseV1, FacadeError, FacadeErrorCode,
    ProductControlAutomationSpecFacadeV1,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AutomationSpecPreviewBodyV1 {
    spec: automation_spec::AutomationSpecV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AutomationSpecSimulationBodyV1 {
    spec: automation_spec::AutomationSpecV1,
    event: automation_spec::AutomationSimulationEventV1,
}

pub(super) async fn automation_spec_descriptor<F>(
    State(state): State<HttpState<F>>,
    Extension(request_id): Extension<RequestId>,
    Path(installation_id): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response
where
    F: ProductControlAutomationSpecFacadeV1,
{
    if query.is_some() {
        return malformed_query(&request_id);
    }
    let credential = match session_credential(&headers, &request_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    if !valid_resource_id(&installation_id) {
        return invalid_input(&request_id);
    }
    match state
        .facade
        .automation_spec_read_authority(&credential, &installation_id)
        .await
    {
        Ok(()) => Json(automation_spec::automation_spec_descriptor_v1()).into_response(),
        Err(error) => map_spec_facade(error, &request_id),
    }
}

pub(super) async fn automation_spec_preview<F>(
    State(state): State<HttpState<F>>,
    Extension(request_id): Extension<RequestId>,
    Path(installation_id): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Result<Json<AutomationSpecPreviewBodyV1>, JsonRejection>,
) -> Response
where
    F: ProductControlAutomationSpecFacadeV1,
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
    if !valid_resource_id(&installation_id) {
        return invalid_input(&request_id);
    }
    if let Err(error) = state
        .facade
        .automation_spec_compute_authority(&credential, &csrf, &installation_id)
        .await
    {
        return map_spec_facade(error, &request_id);
    }
    if let Err(error) = automation_spec::validate_automation_spec_v1(&body.spec) {
        return Json(AutomationSpecPreviewResponseV1 {
            schema_version: 1,
            valid: false,
            diagnostics: error.diagnostics().to_vec(),
            installation_readiness: AutomationSpecInstallationReadinessV1::NotEvaluated,
            preview: None,
        })
        .into_response();
    }
    match automation_spec::preview_automation_spec_v1(&body.spec) {
        Ok(preview) => Json(AutomationSpecPreviewResponseV1 {
            schema_version: 1,
            valid: true,
            diagnostics: Vec::new(),
            installation_readiness: AutomationSpecInstallationReadinessV1::NotEvaluated,
            preview: Some(preview),
        })
        .into_response(),
        Err(_) => map_facade(FacadeError::new(FacadeErrorCode::Internal), &request_id),
    }
}

pub(super) async fn automation_spec_simulation<F>(
    State(state): State<HttpState<F>>,
    Extension(request_id): Extension<RequestId>,
    Path(installation_id): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Result<Json<AutomationSpecSimulationBodyV1>, JsonRejection>,
) -> Response
where
    F: ProductControlAutomationSpecFacadeV1,
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
    if !valid_resource_id(&installation_id) {
        return invalid_input(&request_id);
    }
    if let Err(error) = state
        .facade
        .automation_spec_compute_authority(&credential, &csrf, &installation_id)
        .await
    {
        return map_spec_facade(error, &request_id);
    }
    if let Err(error) = automation_spec::validate_automation_spec_v1(&body.spec) {
        return Json(AutomationSpecSimulationResponseV1 {
            schema_version: 1,
            valid: false,
            diagnostics: error.diagnostics().to_vec(),
            spec_digest: None,
            trace: None,
        })
        .into_response();
    }
    let preview = match automation_spec::preview_automation_spec_v1(&body.spec) {
        Ok(value) => value,
        Err(_) => {
            return map_facade(FacadeError::new(FacadeErrorCode::Internal), &request_id);
        }
    };
    match automation_spec::simulate_automation_spec_v1(&body.spec, &body.event) {
        Ok(trace) => Json(AutomationSpecSimulationResponseV1 {
            schema_version: 1,
            valid: true,
            diagnostics: Vec::new(),
            spec_digest: Some(preview.spec_digest),
            trace: Some(trace),
        })
        .into_response(),
        Err(automation_spec::AutomationSimulationErrorV1::InvalidEvent { diagnostics }) => {
            Json(AutomationSpecSimulationResponseV1 {
                schema_version: 1,
                valid: false,
                diagnostics,
                spec_digest: Some(preview.spec_digest),
                trace: None,
            })
            .into_response()
        }
        Err(automation_spec::AutomationSimulationErrorV1::InvalidSpec(_)) => {
            map_facade(FacadeError::new(FacadeErrorCode::Internal), &request_id)
        }
    }
}

fn map_spec_facade(error: FacadeError, request_id: &RequestId) -> Response {
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
