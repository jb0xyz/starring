use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

use design_harness::{LlmClient, LlmError, LlmResponse, Message, ToolCall, ToolDefinition};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_RETAINED_MODEL_CALL_METRICS: usize = 4096;
const SERVING_CODEX_CLI_VERSION: &str = "codex-cli 0.144.2";
const MAX_SAFE_JSON_INTEGER: u64 = 9_007_199_254_740_991;
pub const SERVING_AUTH_MODE: &str = "chatgpt";
pub const SERVING_MODEL: &str = "gpt-5.6-luna";
pub const SERVING_PROVIDER: &str = "codex_chatgpt";
pub const SERVING_REASONING_EFFORT: &str = "medium";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexWorkerCallOutcome {
    Succeeded,
    TransportError,
    HttpError,
    ResponseBodyError,
    MalformedJson,
    InvalidResponse,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CodexWorkerCallMetric {
    pub call_sequence: u64,
    pub attempt: usize,
    pub frontier_name: String,
    pub outcome: CodexWorkerCallOutcome,
    pub http_status: Option<u16>,
    pub served_model: Option<String>,
    pub request_body_bytes: usize,
    pub message_bytes: usize,
    pub tool_bytes: usize,
    pub duplicated_schema_bytes: usize,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub finish_reason: Option<String>,
    pub request_duration_ms: u64,
    pub gateway_model_duration_ms: Option<u64>,
}

#[derive(Clone)]
pub struct CodexWorkerClient {
    http: reqwest::Client,
    completion_endpoint: String,
    health_endpoint: String,
    token: String,
    call_sequence: Arc<AtomicU64>,
    metrics: Arc<Mutex<VecDeque<CodexWorkerCallMetric>>>,
}

#[derive(Serialize)]
struct WorkerRequest<'a> {
    schema_version: u32,
    model: &'static str,
    reasoning_effort: &'static str,
    messages: &'a [Message],
    frontier: &'a ToolDefinition,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerResponse {
    schema_version: u32,
    request_id: String,
    provider: String,
    model: String,
    reasoning_effort: String,
    auth_mode: String,
    codex_cli_version: String,
    tool_call: WorkerToolCall,
    usage: WorkerUsage,
    duration_ms: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerToolCall {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerUsage {
    input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
    reasoning_output_tokens: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerHealth {
    schema_version: u32,
    status: String,
    provider: String,
    model: String,
    reasoning_effort: String,
    auth_mode: String,
    codex_cli_version: String,
    instance_id: String,
    worker_source_sha256: String,
    concurrency_limit: usize,
    queue_capacity: usize,
    request_timeout_ms: u64,
    active_requests: usize,
    queued_requests: usize,
    accepted_requests_total: u64,
    settled_requests_total: u64,
}

struct MetricInput {
    frontier_name: String,
    request_body_bytes: usize,
    message_bytes: usize,
    tool_bytes: usize,
    duplicated_schema_bytes: usize,
}

struct MetricObservation {
    outcome: CodexWorkerCallOutcome,
    http_status: Option<u16>,
    served_model: Option<String>,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    finish_reason: Option<String>,
    provider_duration_ms: Option<u64>,
}

fn request_metric_input(
    body: &[u8],
    messages: &[Message],
    frontier: &ToolDefinition,
) -> Result<MetricInput, LlmError> {
    Ok(MetricInput {
        frontier_name: frontier.name.clone(),
        request_body_bytes: body.len(),
        message_bytes: serde_json::to_vec(messages)
            .map_err(|error| LlmError::Client(error.to_string()))?
            .len(),
        tool_bytes: serde_json::to_vec(frontier)
            .map_err(|error| LlmError::Client(error.to_string()))?
            .len(),
        duplicated_schema_bytes: 0,
    })
}

impl MetricObservation {
    fn failed(outcome: CodexWorkerCallOutcome, http_status: Option<u16>) -> Self {
        Self {
            outcome,
            http_status,
            served_model: None,
            prompt_tokens: None,
            completion_tokens: None,
            finish_reason: None,
            provider_duration_ms: None,
        }
    }
}

impl CodexWorkerClient {
    pub fn new(base_url: String, token: String) -> Result<Self, LlmError> {
        if token.trim().is_empty() {
            return Err(LlmError::Client(
                "codex worker token must not be empty".to_string(),
            ));
        }
        let parsed_url = reqwest::Url::parse(&base_url)
            .map_err(|_| LlmError::Client("codex worker URL is invalid".to_string()))?;
        let loopback_only = parsed_url.scheme() == "http"
            && parsed_url.host_str() == Some("127.0.0.1")
            && parsed_url.username().is_empty()
            && parsed_url.password().is_none()
            && parsed_url.query().is_none()
            && parsed_url.fragment().is_none()
            && matches!(parsed_url.path(), "" | "/");
        if !loopback_only {
            return Err(LlmError::Client(
                "codex worker URL must be loopback HTTP".to_string(),
            ));
        }
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|error| LlmError::Client(error.to_string()))?;
        let base_url = base_url.trim_end_matches('/');
        Ok(Self {
            http,
            completion_endpoint: format!("{base_url}/v1/frontier-completions"),
            health_endpoint: format!("{base_url}/health"),
            token,
            call_sequence: Arc::new(AtomicU64::new(0)),
            metrics: Arc::new(Mutex::new(VecDeque::new())),
        })
    }

    pub async fn preflight(&self) -> Result<(), LlmError> {
        let response = self
            .http
            .get(&self.health_endpoint)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|_| LlmError::Client("codex worker preflight failed".to_string()))?;
        if !response.status().is_success() {
            return Err(LlmError::Client(format!(
                "codex worker preflight returned HTTP {}",
                response.status().as_u16()
            )));
        }
        let health = response
            .json::<WorkerHealth>()
            .await
            .map_err(|_| LlmError::Client("codex worker health is invalid".to_string()))?;
        validate_health(&health)
    }

    pub fn model_call_metrics(&self) -> Result<Vec<CodexWorkerCallMetric>, LlmError> {
        self.metrics
            .lock()
            .map(|metrics| metrics.iter().cloned().collect())
            .map_err(|_| LlmError::Client("model call metrics are unavailable".to_string()))
    }

    fn record_metric(
        &self,
        input: &MetricInput,
        call_sequence: u64,
        observation: MetricObservation,
        elapsed: Duration,
    ) -> Result<(), LlmError> {
        let mut metrics = self
            .metrics
            .lock()
            .map_err(|_| LlmError::Client("model call metrics are unavailable".to_string()))?;
        if metrics.len() == MAX_RETAINED_MODEL_CALL_METRICS {
            metrics.pop_front();
        }
        metrics.push_back(CodexWorkerCallMetric {
            call_sequence,
            attempt: 1,
            frontier_name: input.frontier_name.clone(),
            outcome: observation.outcome,
            http_status: observation.http_status,
            served_model: observation.served_model,
            request_body_bytes: input.request_body_bytes,
            message_bytes: input.message_bytes,
            tool_bytes: input.tool_bytes,
            duplicated_schema_bytes: input.duplicated_schema_bytes,
            prompt_tokens: observation.prompt_tokens,
            completion_tokens: observation.completion_tokens,
            finish_reason: observation.finish_reason,
            request_duration_ms: elapsed.as_millis().try_into().unwrap_or(u64::MAX),
            gateway_model_duration_ms: observation.provider_duration_ms,
        });
        Ok(())
    }
}

impl LlmClient for CodexWorkerClient {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> Result<LlmResponse, LlmError> {
        let [frontier] = tools else {
            return Err(LlmError::Client(
                "codex worker requires exactly one frontier".to_string(),
            ));
        };
        let request = WorkerRequest {
            schema_version: 1,
            model: SERVING_MODEL,
            reasoning_effort: SERVING_REASONING_EFFORT,
            messages,
            frontier,
        };
        let body =
            serde_json::to_vec(&request).map_err(|error| LlmError::Client(error.to_string()))?;
        let metric_input = request_metric_input(&body, messages, frontier)?;
        let call_sequence = self
            .call_sequence
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let started = Instant::now();
        let response = self
            .http
            .post(&self.completion_endpoint)
            .bearer_auth(&self.token)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await;
        let response = match response {
            Ok(response) => response,
            Err(_) => {
                self.record_metric(
                    &metric_input,
                    call_sequence,
                    MetricObservation::failed(CodexWorkerCallOutcome::TransportError, None),
                    started.elapsed(),
                )?;
                return Err(LlmError::Client("codex worker request failed".to_string()));
            }
        };
        let status = response.status();
        if !status.is_success() {
            self.record_metric(
                &metric_input,
                call_sequence,
                MetricObservation::failed(CodexWorkerCallOutcome::HttpError, Some(status.as_u16())),
                started.elapsed(),
            )?;
            return Err(LlmError::Client(format!(
                "codex worker returned HTTP {}",
                status.as_u16()
            )));
        }
        let bytes = match response.bytes().await {
            Ok(bytes) => bytes,
            Err(_) => {
                self.record_metric(
                    &metric_input,
                    call_sequence,
                    MetricObservation::failed(
                        CodexWorkerCallOutcome::ResponseBodyError,
                        Some(status.as_u16()),
                    ),
                    started.elapsed(),
                )?;
                return Err(LlmError::Client(
                    "codex worker response body failed".to_string(),
                ));
            }
        };
        let response = match serde_json::from_slice::<WorkerResponse>(&bytes) {
            Ok(response) => response,
            Err(_) => {
                self.record_metric(
                    &metric_input,
                    call_sequence,
                    MetricObservation::failed(
                        CodexWorkerCallOutcome::MalformedJson,
                        Some(status.as_u16()),
                    ),
                    started.elapsed(),
                )?;
                return Err(LlmError::Client(
                    "codex worker response is malformed".to_string(),
                ));
            }
        };
        let tool_call = match validate_response(response, &frontier.name) {
            Ok(tool_call) => tool_call,
            Err(error) => {
                self.record_metric(
                    &metric_input,
                    call_sequence,
                    MetricObservation::failed(
                        CodexWorkerCallOutcome::InvalidResponse,
                        Some(status.as_u16()),
                    ),
                    started.elapsed(),
                )?;
                return Err(error);
            }
        };
        let response = tool_call.response;
        self.record_metric(
            &metric_input,
            call_sequence,
            MetricObservation {
                outcome: CodexWorkerCallOutcome::Succeeded,
                http_status: Some(status.as_u16()),
                served_model: Some(SERVING_MODEL.to_string()),
                prompt_tokens: Some(response.usage.input_tokens),
                completion_tokens: Some(response.usage.output_tokens),
                finish_reason: Some("tool_calls".to_string()),
                provider_duration_ms: Some(response.duration_ms),
            },
            started.elapsed(),
        )?;
        Ok(LlmResponse::ToolCalls(vec![tool_call.call]))
    }
}

struct ValidatedToolCall {
    call: ToolCall,
    response: WorkerResponse,
}

fn validate_response(
    response: WorkerResponse,
    expected_frontier: &str,
) -> Result<ValidatedToolCall, LlmError> {
    let identity_matches = response.schema_version == 1
        && response.provider == SERVING_PROVIDER
        && response.model == SERVING_MODEL
        && response.reasoning_effort == SERVING_REASONING_EFFORT
        && response.auth_mode == SERVING_AUTH_MODE
        && !response.request_id.trim().is_empty()
        && response.codex_cli_version == SERVING_CODEX_CLI_VERSION;
    if !identity_matches {
        return Err(LlmError::Client(
            "codex worker identity mismatch".to_string(),
        ));
    }
    if response.tool_call.id.trim().is_empty() || response.tool_call.name != expected_frontier {
        return Err(LlmError::Client(
            "codex worker returned the wrong frontier".to_string(),
        ));
    }
    if response.usage.cached_input_tokens > response.usage.input_tokens
        || response.usage.reasoning_output_tokens > response.usage.output_tokens
    {
        return Err(LlmError::Client(
            "codex worker usage is invalid".to_string(),
        ));
    }
    let arguments = serde_json::from_str::<Value>(&response.tool_call.arguments)
        .map_err(|_| LlmError::Client("codex worker arguments are invalid".to_string()))?;
    if !arguments.is_object() {
        return Err(LlmError::Client(
            "codex worker arguments must be an object".to_string(),
        ));
    }
    let call = ToolCall {
        id: response.tool_call.id.clone(),
        name: response.tool_call.name.clone(),
        arguments: response.tool_call.arguments.clone(),
    };
    Ok(ValidatedToolCall { call, response })
}

fn validate_health(health: &WorkerHealth) -> Result<(), LlmError> {
    let valid = health.schema_version == 1
        && health.status == "ok"
        && health.provider == SERVING_PROVIDER
        && health.model == SERVING_MODEL
        && health.reasoning_effort == SERVING_REASONING_EFFORT
        && health.auth_mode == SERVING_AUTH_MODE
        && health.codex_cli_version == SERVING_CODEX_CLI_VERSION
        && !health.instance_id.is_empty()
        && health.instance_id.len() <= 128
        && health.instance_id.trim() == health.instance_id
        && health.worker_source_sha256.len() == 64
        && health
            .worker_source_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && (1..=8).contains(&health.concurrency_limit)
        && health.queue_capacity <= 128
        && health.request_timeout_ms == 55_000
        && health.accepted_requests_total <= MAX_SAFE_JSON_INTEGER
        && health.settled_requests_total <= health.accepted_requests_total
        && health.accepted_requests_total - health.settled_requests_total
            == (health.active_requests + health.queued_requests) as u64;
    if valid {
        Ok(())
    } else {
        Err(LlmError::Client(
            "codex worker health identity mismatch".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use design_harness::{LlmClient, Message, ToolDefinition};
    use serde_json::{json, Value};

    use super::{
        request_metric_input, validate_health, validate_response, CodexWorkerCallOutcome,
        CodexWorkerClient, WorkerHealth, WorkerRequest, WorkerResponse, MAX_SAFE_JSON_INTEGER,
        SERVING_CODEX_CLI_VERSION,
    };

    fn response_value() -> Value {
        json!({
            "schema_version": 1,
            "request_id": "request-1",
            "provider": "codex_chatgpt",
            "model": "gpt-5.6-luna",
            "reasoning_effort": "medium",
            "auth_mode": "chatgpt",
            "codex_cli_version": "codex-cli 0.144.2",
            "tool_call": {
                "id": "call-1",
                "name": "interpret_intent_core",
                "arguments": "{\"route\":\"managed\"}"
            },
            "usage": {
                "input_tokens": 100,
                "cached_input_tokens": 50,
                "output_tokens": 20,
                "reasoning_output_tokens": 10
            },
            "duration_ms": 5000
        })
    }

    fn response() -> WorkerResponse {
        serde_json::from_value(response_value()).unwrap()
    }

    fn frontier(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_string(),
            description: "frontier".to_string(),
            parameters: json!({"type":"object"}),
        }
    }

    #[test]
    fn response_requires_exact_luna_medium_identity() {
        let valid = validate_response(response(), "interpret_intent_core").unwrap();
        assert_eq!(valid.call.name, "interpret_intent_core");

        let mut wrong_schema = response();
        wrong_schema.schema_version = 2;
        assert!(validate_response(wrong_schema, "interpret_intent_core").is_err());

        let mut wrong_provider = response();
        wrong_provider.provider = "other".to_string();
        assert!(validate_response(wrong_provider, "interpret_intent_core").is_err());

        let mut wrong_model = response();
        wrong_model.model = "gpt-5.6-terra".to_string();
        assert!(validate_response(wrong_model, "interpret_intent_core").is_err());

        let mut wrong = response();
        wrong.reasoning_effort = "low".to_string();
        assert!(validate_response(wrong, "interpret_intent_core").is_err());

        let mut wrong_auth = response();
        wrong_auth.auth_mode = "api_key".to_string();
        assert!(validate_response(wrong_auth, "interpret_intent_core").is_err());

        let mut missing_request = response();
        missing_request.request_id = " ".to_string();
        assert!(validate_response(missing_request, "interpret_intent_core").is_err());

        let mut wrong_version = response();
        wrong_version.codex_cli_version = "codex-cli 0.145.0".to_string();
        assert!(validate_response(wrong_version, "interpret_intent_core").is_err());

        let mut missing_call = response();
        missing_call.tool_call.id.clear();
        assert!(validate_response(missing_call, "interpret_intent_core").is_err());

        let mut wrong_frontier = response();
        wrong_frontier.tool_call.name = "other".to_string();
        assert!(validate_response(wrong_frontier, "interpret_intent_core").is_err());
    }

    #[test]
    fn response_rejects_invalid_usage_and_arguments() {
        let mut usage = response();
        usage.usage.cached_input_tokens = 101;
        assert!(validate_response(usage, "interpret_intent_core").is_err());

        let mut arguments = response();
        arguments.tool_call.arguments = "[]".to_string();
        assert!(validate_response(arguments, "interpret_intent_core").is_err());

        let mut reasoning_usage = response();
        reasoning_usage.usage.reasoning_output_tokens = 21;
        assert!(validate_response(reasoning_usage, "interpret_intent_core").is_err());
    }

    #[test]
    fn response_shape_rejects_unknown_fields_at_every_protocol_level() {
        let mut response_field = response_value();
        response_field
            .as_object_mut()
            .unwrap()
            .insert("unexpected".to_string(), Value::Null);
        assert!(serde_json::from_value::<WorkerResponse>(response_field).is_err());

        let mut tool_field = response_value();
        tool_field["tool_call"]
            .as_object_mut()
            .unwrap()
            .insert("unexpected".to_string(), Value::Null);
        assert!(serde_json::from_value::<WorkerResponse>(tool_field).is_err());

        let mut usage_field = response_value();
        usage_field["usage"]
            .as_object_mut()
            .unwrap()
            .insert("unexpected".to_string(), Value::Null);
        assert!(serde_json::from_value::<WorkerResponse>(usage_field).is_err());
    }

    #[test]
    fn health_requires_chatgpt_luna_medium() {
        let mut health: WorkerHealth = serde_json::from_value(json!({
            "schema_version": 1,
            "status": "ok",
            "provider": "codex_chatgpt",
            "model": "gpt-5.6-luna",
            "reasoning_effort": "medium",
            "auth_mode": "chatgpt",
            "codex_cli_version": "codex-cli 0.144.2",
            "instance_id": "test-worker-instance",
            "worker_source_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "concurrency_limit": 2,
            "queue_capacity": 8,
            "request_timeout_ms": 55000,
            "active_requests": 0,
            "queued_requests": 0,
            "accepted_requests_total": 7,
            "settled_requests_total": 7
        }))
        .unwrap();
        assert!(validate_health(&health).is_ok());
        health.codex_cli_version = "codex-cli 0.145.0".to_string();
        assert!(validate_health(&health).is_err());
        health.codex_cli_version = SERVING_CODEX_CLI_VERSION.to_string();
        health.instance_id.clear();
        assert!(validate_health(&health).is_err());
        health.instance_id = "test-worker-instance".to_string();
        health.worker_source_sha256 = "g".repeat(64);
        assert!(validate_health(&health).is_err());
        health.worker_source_sha256 = "a".repeat(64);
        health.concurrency_limit = 0;
        assert!(validate_health(&health).is_err());
        health.concurrency_limit = 2;
        health.request_timeout_ms = 54_999;
        assert!(validate_health(&health).is_err());
        health.request_timeout_ms = 55_000;
        health.settled_requests_total = 8;
        assert!(validate_health(&health).is_err());
        health.settled_requests_total = 6;
        assert!(validate_health(&health).is_err());
        health.active_requests = 1;
        assert!(validate_health(&health).is_ok());
        health.accepted_requests_total = MAX_SAFE_JSON_INTEGER + 1;
        assert!(validate_health(&health).is_err());
    }

    #[tokio::test]
    async fn client_rejects_multiple_frontiers_before_transport() {
        let client =
            CodexWorkerClient::new("http://127.0.0.1:1".to_string(), "test-token".to_string())
                .unwrap();
        let tools = vec![
            ToolDefinition {
                name: "first".to_string(),
                description: "first".to_string(),
                parameters: json!({"type":"object"}),
            },
            ToolDefinition {
                name: "second".to_string(),
                description: "second".to_string(),
                parameters: json!({"type":"object"}),
            },
        ];
        let error = client
            .complete(&[Message::user("test")], &tools)
            .await
            .unwrap_err();
        let design_harness::LlmError::Client(message) = error;
        assert_eq!(message, "codex worker requires exactly one frontier");
        assert!(client.model_call_metrics().unwrap().is_empty());
    }

    #[test]
    fn client_rejects_non_loopback_worker_urls() {
        for base_url in [
            "https://127.0.0.1:18181",
            "http://localhost:18181",
            "http://[::1]:18181",
            "http://user@127.0.0.1:18181",
            "http://user:password@127.0.0.1:18181",
            "http://127.0.0.1:18181/path",
            "http://127.0.0.1:18181?query=value",
            "http://127.0.0.1:18181#fragment",
        ] {
            assert!(
                CodexWorkerClient::new(base_url.to_string(), "test-token".to_string()).is_err(),
                "{base_url}"
            );
        }
        assert!(
            CodexWorkerClient::new("http://127.0.0.1:18181".to_string(), " ".to_string()).is_err()
        );
    }

    #[test]
    fn client_transport_disables_proxy_autodiscovery() {
        let source = include_str!("lib.rs");
        assert!(source.contains(
            "reqwest::Client::builder()\n            .no_proxy()\n            .timeout(REQUEST_TIMEOUT)"
        ));
    }

    #[test]
    fn client_has_no_debug_or_serialization_surface() {
        let source = include_str!("lib.rs");
        let production = source.split("#[cfg(test)]").next().unwrap();
        let prefix = production
            .split("pub struct CodexWorkerClient")
            .next()
            .unwrap();
        let attributes = prefix.rsplit("\n\n").next().unwrap().trim();
        assert_eq!(attributes, "#[derive(Clone)]");
        assert!(!production.contains("impl Debug for CodexWorkerClient"));
        assert!(!production.contains("impl Serialize for CodexWorkerClient"));
    }

    #[test]
    fn request_metrics_report_native_worker_payload_without_schema_duplication() {
        let messages = vec![Message::user("build")];
        let frontier = ToolDefinition {
            name: "interpret_intent_core".to_string(),
            description: "interpret".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        };
        let request = WorkerRequest {
            schema_version: 1,
            model: "gpt-5.6-luna",
            reasoning_effort: "medium",
            messages: &messages,
            frontier: &frontier,
        };
        let body = serde_json::to_vec(&request).unwrap();

        let metric = request_metric_input(&body, &messages, &frontier).unwrap();

        assert_eq!(metric.request_body_bytes, body.len());
        assert_eq!(
            metric.message_bytes,
            serde_json::to_vec(&messages).unwrap().len()
        );
        assert_eq!(
            metric.tool_bytes,
            serde_json::to_vec(&frontier).unwrap().len()
        );
        assert_eq!(metric.duplicated_schema_bytes, 0);
        assert!(metric.request_body_bytes > metric.message_bytes + metric.tool_bytes);
    }

    #[tokio::test]
    async fn worker_token_is_absent_from_errors_and_metrics() {
        let token = "worker-token-redaction-marker";
        let client =
            CodexWorkerClient::new("http://127.0.0.1:1".to_string(), token.to_string()).unwrap();
        let error = client
            .complete(&[Message::user("private input")], &[frontier("only")])
            .await
            .unwrap_err();
        assert!(!format!("{error}").contains(token));
        assert!(!format!("{error:?}").contains(token));

        let metrics = client.model_call_metrics().unwrap();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].outcome, CodexWorkerCallOutcome::TransportError);
        let serialized = serde_json::to_string(&metrics).unwrap();
        assert!(!serialized.contains(token));
        assert!(!serialized.contains("private input"));
    }
}
