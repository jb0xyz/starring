use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use design_harness::{
    LlmClient, LlmError, LlmResponse, Message, MessageRole, ToolCall, ToolDefinition,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const RETRY_BACKOFF: Duration = Duration::from_millis(100);
const LEGACY_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
const INTENT_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const LEGACY_HTTP_RETRIES: usize = 1;
const INTENT_HTTP_RETRIES: usize = 0;
const MAX_RETAINED_MODEL_CALL_METRICS: usize = 4096;
pub const INTENT_SERVING_MODEL: &str = crate::config::SERVING_MODEL;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TransportPolicy {
    request_timeout: Duration,
    max_http_retries: usize,
    redact_request_errors: bool,
}

impl TransportPolicy {
    const LEGACY: Self = Self {
        request_timeout: LEGACY_REQUEST_TIMEOUT,
        max_http_retries: LEGACY_HTTP_RETRIES,
        redact_request_errors: false,
    };

    const INTENT_SERVING: Self = Self {
        request_timeout: INTENT_REQUEST_TIMEOUT,
        max_http_retries: INTENT_HTTP_RETRIES,
        redact_request_errors: true,
    };
}

#[derive(Clone)]
pub struct GemmaClient {
    http: reqwest::Client,
    endpoint: String,
    models_endpoint: String,
    api_key: String,
    model: String,
    transport_policy: TransportPolicy,
    adapted_call_sequence: Arc<AtomicU64>,
    model_call_sequence: Arc<AtomicU64>,
    model_call_metrics: Arc<Mutex<VecDeque<ModelCallMetric>>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCallOutcome {
    Succeeded,
    TransportError,
    HttpError,
    ResponseBodyError,
    MalformedJson,
    InvalidResponse,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModelCallMetric {
    pub call_sequence: u64,
    pub attempt: usize,
    pub frontier_name: String,
    pub outcome: ModelCallOutcome,
    pub http_status: Option<u16>,
    pub served_model: Option<String>,
    pub request_body_bytes: usize,
    pub message_bytes: usize,
    pub tool_bytes: usize,
    pub duplicated_schema_bytes: usize,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub request_duration_ms: u64,
    pub gateway_model_duration_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RequestMetricInput {
    frontier_name: String,
    request_body_bytes: usize,
    message_bytes: usize,
    tool_bytes: usize,
    duplicated_schema_bytes: usize,
}

impl GemmaClient {
    pub fn new(base_url: String, api_key: String, model: String) -> Result<Self, LlmError> {
        Self::with_policy(base_url, api_key, model, TransportPolicy::LEGACY)
    }

    pub fn new_intent_serving(base_url: String, api_key: String) -> Result<Self, LlmError> {
        Self::with_policy(
            base_url,
            api_key,
            INTENT_SERVING_MODEL.to_string(),
            TransportPolicy::INTENT_SERVING,
        )
    }

    fn with_policy(
        base_url: String,
        api_key: String,
        model: String,
        transport_policy: TransportPolicy,
    ) -> Result<Self, LlmError> {
        let http = reqwest::Client::builder()
            .timeout(transport_policy.request_timeout)
            .build()
            .map_err(|error| LlmError::Client(error.to_string()))?;
        let base_url = base_url.trim_end_matches('/');
        Ok(Self {
            http,
            endpoint: format!("{base_url}/chat/completions"),
            models_endpoint: format!("{base_url}/models"),
            api_key,
            model,
            transport_policy,
            adapted_call_sequence: Arc::new(AtomicU64::new(0)),
            model_call_sequence: Arc::new(AtomicU64::new(0)),
            model_call_metrics: Arc::new(Mutex::new(VecDeque::new())),
        })
    }

    pub fn model_call_metrics(&self) -> Result<Vec<ModelCallMetric>, LlmError> {
        self.model_call_metrics
            .lock()
            .map(|metrics| metrics.iter().cloned().collect())
            .map_err(|_| LlmError::Client("model call metrics are unavailable".to_string()))
    }

    pub async fn preflight_model(&self) -> Result<(), LlmError> {
        let response = self
            .http
            .get(&self.models_endpoint)
            .bearer_auth(&self.api_key)
            .send()
            .await
            .map_err(|_| LlmError::Client("model preflight request failed".to_string()))?;
        if !response.status().is_success() {
            return Err(LlmError::Client(format!(
                "model preflight returned HTTP {}",
                response.status().as_u16()
            )));
        }
        let catalog = response.json::<ModelsResponse>().await.map_err(|_| {
            LlmError::Client("model preflight returned an invalid catalog".to_string())
        })?;
        if !catalog.data.iter().any(|entry| entry.id == self.model) {
            return Err(LlmError::Client(format!(
                "required model {} is unavailable",
                self.model
            )));
        }
        Ok(())
    }

    fn request_error(&self, error: reqwest::Error) -> LlmError {
        if self.transport_policy.redact_request_errors {
            LlmError::Client("model request failed".to_string())
        } else {
            LlmError::Client(error.to_string())
        }
    }

    fn record_model_call_metric(
        &self,
        input: &RequestMetricInput,
        call_sequence: u64,
        attempt: usize,
        observation: AttemptObservation,
        request_duration: Duration,
    ) -> Result<(), LlmError> {
        let metric = ModelCallMetric {
            call_sequence,
            attempt,
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
            request_duration_ms: u64::try_from(request_duration.as_millis()).unwrap_or(u64::MAX),
            gateway_model_duration_ms: None,
        };
        let mut metrics = self
            .model_call_metrics
            .lock()
            .map_err(|_| LlmError::Client("model call metrics are unavailable".to_string()))?;
        if metrics.len() >= MAX_RETAINED_MODEL_CALL_METRICS {
            metrics.pop_front();
        }
        metrics.push_back(metric);
        Ok(())
    }
}

struct AttemptObservation {
    outcome: ModelCallOutcome,
    http_status: Option<u16>,
    served_model: Option<String>,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
}

impl AttemptObservation {
    fn failed(outcome: ModelCallOutcome, http_status: Option<u16>) -> Self {
        Self {
            outcome,
            http_status,
            served_model: None,
            prompt_tokens: None,
            completion_tokens: None,
        }
    }
}

impl LlmClient for GemmaClient {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> Result<LlmResponse, LlmError> {
        let body = build_request_body(messages, tools, &self.model)?;
        let metric_input = request_metric_input(&body, tools)?;
        let call_sequence = self
            .model_call_sequence
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        for attempt in 0..=self.transport_policy.max_http_retries {
            let attempt_number = attempt.saturating_add(1);
            let started = Instant::now();
            let response = self
                .http
                .post(&self.endpoint)
                .bearer_auth(&self.api_key)
                .json(&body)
                .send()
                .await;
            let response = match response {
                Ok(response) => response,
                Err(error)
                    if attempt < self.transport_policy.max_http_retries
                        && is_transient_transport_error(&error) =>
                {
                    self.record_model_call_metric(
                        &metric_input,
                        call_sequence,
                        attempt_number,
                        AttemptObservation::failed(ModelCallOutcome::TransportError, None),
                        started.elapsed(),
                    )?;
                    tokio::time::sleep(RETRY_BACKOFF).await;
                    continue;
                }
                Err(error) => {
                    let request_error = self.request_error(error);
                    self.record_model_call_metric(
                        &metric_input,
                        call_sequence,
                        attempt_number,
                        AttemptObservation::failed(ModelCallOutcome::TransportError, None),
                        started.elapsed(),
                    )?;
                    return Err(request_error);
                }
            };
            let status = response.status();
            if !status.is_success() {
                self.record_model_call_metric(
                    &metric_input,
                    call_sequence,
                    attempt_number,
                    AttemptObservation::failed(ModelCallOutcome::HttpError, Some(status.as_u16())),
                    started.elapsed(),
                )?;
                if attempt < self.transport_policy.max_http_retries && is_retryable_status(status) {
                    tokio::time::sleep(RETRY_BACKOFF).await;
                    continue;
                }
                return Err(LlmError::Client(format!(
                    "gateway returned HTTP {}",
                    status.as_u16()
                )));
            }
            let response_bytes = match response.bytes().await {
                Ok(bytes) => bytes,
                Err(error) => {
                    let request_error = self.request_error(error);
                    self.record_model_call_metric(
                        &metric_input,
                        call_sequence,
                        attempt_number,
                        AttemptObservation::failed(
                            ModelCallOutcome::ResponseBodyError,
                            Some(status.as_u16()),
                        ),
                        started.elapsed(),
                    )?;
                    return Err(request_error);
                }
            };
            let value = match serde_json::from_slice::<Value>(&response_bytes) {
                Ok(value) => value,
                Err(error) => {
                    self.record_model_call_metric(
                        &metric_input,
                        call_sequence,
                        attempt_number,
                        AttemptObservation::failed(
                            ModelCallOutcome::MalformedJson,
                            Some(status.as_u16()),
                        ),
                        started.elapsed(),
                    )?;
                    return Err(LlmError::Client(error.to_string()));
                }
            };
            let (prompt_tokens, completion_tokens) = completion_usage(&value);
            let served_model = response_model(&value);
            let response = match parse_response_value(value, &self.model) {
                Ok(response) => response,
                Err(error) => {
                    self.record_model_call_metric(
                        &metric_input,
                        call_sequence,
                        attempt_number,
                        AttemptObservation {
                            outcome: ModelCallOutcome::InvalidResponse,
                            http_status: Some(status.as_u16()),
                            served_model,
                            prompt_tokens,
                            completion_tokens,
                        },
                        started.elapsed(),
                    )?;
                    return Err(error);
                }
            };
            let response =
                adapt_single_frontier_response(response, tools, &self.adapted_call_sequence);
            self.record_model_call_metric(
                &metric_input,
                call_sequence,
                attempt_number,
                AttemptObservation {
                    outcome: ModelCallOutcome::Succeeded,
                    http_status: Some(status.as_u16()),
                    served_model,
                    prompt_tokens,
                    completion_tokens,
                },
                started.elapsed(),
            )?;
            return Ok(response);
        }
        unreachable!()
    }
}

fn request_metric_input(
    body: &Value,
    tools: &[ToolDefinition],
) -> Result<RequestMetricInput, LlmError> {
    let message_bytes = serialized_value_bytes(
        body.get("messages")
            .ok_or_else(|| LlmError::Client("request body is missing messages".to_string()))?,
    )?;
    let tool_bytes = serialized_value_bytes(
        body.get("tools")
            .ok_or_else(|| LlmError::Client("request body is missing tools".to_string()))?,
    )?;
    let duplicated_schema_bytes = match tools {
        [tool] => serialized_value_bytes(&tool.parameters)?,
        _ => 0,
    };
    let frontier_name = match tools {
        [tool] => tool.name.clone(),
        [] => "no_tool_frontier".to_string(),
        _ => "multi_tool_frontier".to_string(),
    };
    Ok(RequestMetricInput {
        frontier_name,
        request_body_bytes: serialized_value_bytes(body)?,
        message_bytes,
        tool_bytes,
        duplicated_schema_bytes,
    })
}

fn serialized_value_bytes(value: &Value) -> Result<usize, LlmError> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(|error| LlmError::Client(error.to_string()))
}

fn completion_usage(value: &Value) -> (Option<u64>, Option<u64>) {
    let usage = value.get("usage");
    (
        usage
            .and_then(|usage| usage.get("prompt_tokens"))
            .and_then(Value::as_u64),
        usage
            .and_then(|usage| usage.get("completion_tokens"))
            .and_then(Value::as_u64),
    )
}

fn response_model(value: &Value) -> Option<String> {
    value
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn is_transient_transport_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect() || error.is_request() || error.is_body()
}

#[derive(Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    messages: Vec<OpenAiMessage<'a>>,
    tools: Vec<OpenAiTool<'a>>,
    tool_choice: &'static str,
    parallel_tool_calls: bool,
    temperature: f64,
    seed: u32,
    stream: bool,
}

#[derive(Serialize)]
struct OpenAiMessage<'a> {
    role: &'static str,
    content: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCall<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<&'a str>,
}

#[derive(Serialize)]
struct OpenAiToolCall<'a> {
    id: &'a str,
    r#type: &'static str,
    function: OpenAiFunctionCall<'a>,
}

#[derive(Serialize)]
struct OpenAiFunctionCall<'a> {
    name: &'a str,
    arguments: &'a str,
}

#[derive(Serialize)]
struct OpenAiTool<'a> {
    r#type: &'static str,
    function: OpenAiFunctionTool<'a>,
}

#[derive(Serialize)]
struct OpenAiFunctionTool<'a> {
    name: &'a str,
    description: &'a str,
    parameters: &'a Value,
}

fn build_request_body(
    messages: &[Message],
    tools: &[ToolDefinition],
    model: &str,
) -> Result<Value, LlmError> {
    let messages = messages
        .iter()
        .map(openai_message)
        .collect::<Result<Vec<_>, LlmError>>()?;
    let openai_tools = tools
        .iter()
        .map(|tool| OpenAiTool {
            r#type: "function",
            function: OpenAiFunctionTool {
                name: &tool.name,
                description: &tool.description,
                parameters: &tool.parameters,
            },
        })
        .collect();
    let mut body = serde_json::to_value(ChatCompletionRequest {
        model,
        messages,
        tools: openai_tools,
        tool_choice: "auto",
        parallel_tool_calls: false,
        temperature: 0.1,
        seed: 0,
        stream: false,
    })
    .map_err(|error| LlmError::Client(error.to_string()))?;
    if let [tool] = tools {
        let body = body
            .as_object_mut()
            .ok_or_else(|| LlmError::Client("request body is not an object".to_string()))?;
        body.insert(
            "response_format".to_string(),
            serde_json::json!({
                "type":"json_schema",
                "json_schema":{
                    "name":format!("{}_arguments", tool.name),
                    "strict":true,
                    "schema":tool.parameters
                }
            }),
        );
    }
    Ok(body)
}

fn adapt_single_frontier_response(
    response: LlmResponse,
    tools: &[ToolDefinition],
    sequence: &AtomicU64,
) -> LlmResponse {
    let [tool] = tools else {
        return response;
    };
    let LlmResponse::Text(arguments) = response else {
        return response;
    };
    if !serde_json::from_str::<Value>(&arguments).is_ok_and(|value| value.is_object()) {
        return LlmResponse::Text(arguments);
    }
    let id = sequence.fetch_add(1, Ordering::Relaxed).saturating_add(1);
    LlmResponse::ToolCalls(vec![ToolCall {
        id: format!("call-adapted-{id}"),
        name: tool.name.clone(),
        arguments,
    }])
}

fn openai_message(message: &Message) -> Result<OpenAiMessage<'_>, LlmError> {
    let role = match message.role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    };
    let content = if message.role == MessageRole::Assistant
        && message.content.is_empty()
        && !message.tool_calls.is_empty()
    {
        None
    } else {
        Some(message.content.as_str())
    };
    let tool_calls = if message.tool_calls.is_empty() {
        None
    } else {
        Some(
            message
                .tool_calls
                .iter()
                .map(|call| OpenAiToolCall {
                    id: &call.id,
                    r#type: "function",
                    function: OpenAiFunctionCall {
                        name: &call.name,
                        arguments: &call.arguments,
                    },
                })
                .collect(),
        )
    };
    if message.role == MessageRole::Tool && message.tool_call_id.is_none() {
        return Err(LlmError::Client(
            "tool message is missing tool_call_id".to_string(),
        ));
    }
    Ok(OpenAiMessage {
        role,
        content,
        tool_calls,
        tool_call_id: message.tool_call_id.as_deref(),
    })
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    model: String,
    choices: Vec<ResponseChoice>,
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

#[derive(Deserialize)]
struct ResponseChoice {
    message: ResponseMessage,
}

#[derive(Deserialize)]
struct ResponseMessage {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ResponseToolCall>,
}

#[derive(Deserialize)]
struct ResponseToolCall {
    id: String,
    function: ResponseFunctionCall,
}

#[derive(Deserialize)]
struct ResponseFunctionCall {
    name: String,
    arguments: String,
}

fn parse_response_value(value: Value, expected_model: &str) -> Result<LlmResponse, LlmError> {
    let response: ChatCompletionResponse =
        serde_json::from_value(value).map_err(|error| LlmError::Client(error.to_string()))?;
    if response.model != expected_model {
        return Err(LlmError::Client(format!(
            "gateway served model {} instead of requested model {expected_model}",
            response.model
        )));
    }
    let message = response
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| LlmError::Client("response has no choices".to_string()))?
        .message;
    if !message.tool_calls.is_empty() {
        return Ok(LlmResponse::ToolCalls(
            message
                .tool_calls
                .into_iter()
                .map(|call| ToolCall {
                    id: call.id,
                    name: call.function.name,
                    arguments: call.function.arguments,
                })
                .collect(),
        ));
    }
    message
        .content
        .map(LlmResponse::Text)
        .ok_or_else(|| LlmError::Client("response has neither tool calls nor content".to_string()))
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::{SocketAddr, TcpListener, TcpStream},
        sync::atomic::AtomicU64,
        thread,
        time::Duration,
    };

    use design_harness::{tool_definitions, LlmClient, LlmError, LlmResponse, Message, ToolCall};
    use serde_json::json;

    use super::{
        adapt_single_frontier_response, build_request_body, completion_usage, is_retryable_status,
        parse_response_value, request_metric_input, GemmaClient, ModelCallOutcome, TransportPolicy,
        INTENT_SERVING_MODEL,
    };

    fn read_request(stream: &mut TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let count = stream.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..count]);
            let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n")
            else {
                continue;
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or_default();
            if request.len() >= header_end + 4 + content_length {
                break;
            }
        }
        String::from_utf8(request).unwrap()
    }

    fn spawn_server(
        responses: Vec<(u16, &'static str)>,
    ) -> (SocketAddr, thread::JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let mut requests = 0;
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let _ = read_request(&mut stream);
                let reason = match status {
                    200 => "OK",
                    400 => "Bad Request",
                    429 => "Too Many Requests",
                    503 => "Service Unavailable",
                    _ => "Error",
                };
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
                requests += 1;
            }
            requests
        });
        (address, handle)
    }

    fn spawn_disconnect_then_success() -> (SocketAddr, thread::JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut first, _) = listener.accept().unwrap();
            let _ = read_request(&mut first);
            drop(first);
            let (mut second, _) = listener.accept().unwrap();
            let _ = read_request(&mut second);
            let body = success_response();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            second.write_all(response.as_bytes()).unwrap();
            2
        });
        (address, handle)
    }

    fn success_response() -> &'static str {
        r#"{"model":"test-model","choices":[{"message":{"content":"done"}}]}"#
    }

    fn spawn_capture_server(
        status: u16,
        body: &'static str,
    ) -> (SocketAddr, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream);
            let reason = if status == 200 { "OK" } else { "Error" };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            request
        });
        (address, handle)
    }

    fn spawn_delayed_server(delay: Duration) -> (SocketAddr, thread::JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_request(&mut stream);
            thread::sleep(delay);
            let body = r#"{"model":"gemma4:12b-mlx","choices":[{"message":{"content":"late"}}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            1
        });
        (address, handle)
    }

    fn spawn_truncated_response_server() -> (SocketAddr, thread::JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_request(&mut stream);
            let body = r#"{"model":"test-model"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len() + 20
            );
            stream.write_all(response.as_bytes()).unwrap();
            1
        });
        (address, handle)
    }

    fn test_client(address: SocketAddr) -> GemmaClient {
        GemmaClient::new(
            format!("http://{address}"),
            "secret".to_string(),
            "test-model".to_string(),
        )
        .unwrap()
    }

    fn intent_client(address: SocketAddr) -> GemmaClient {
        GemmaClient::new_intent_serving(format!("http://{address}"), "secret".to_string()).unwrap()
    }

    #[test]
    fn request_body_matches_openai_chat_completions_shape() {
        let messages = vec![
            Message::system("system prompt"),
            Message::user("build a room"),
            Message::assistant_tool_calls(vec![ToolCall {
                id: "call_1".to_string(),
                name: "add_panel".to_string(),
                arguments: r#"{"key":"p"}"#.to_string(),
            }]),
            Message::tool("call_1", r#"{"ok":true}"#),
            Message::assistant("QUESTION: Which fields?"),
        ];
        let definitions = tool_definitions();

        let body = build_request_body(&messages, &definitions, "gemma4:12b-mlx").unwrap();

        assert_eq!(body["model"], "gemma4:12b-mlx");
        assert_eq!(body["tool_choice"], "auto");
        assert_eq!(body["parallel_tool_calls"], false);
        assert_eq!(body["temperature"], 0.1);
        assert_eq!(body["seed"], 0);
        assert_eq!(body["stream"], false);
        assert_eq!(
            body["messages"][0],
            json!({
                "role":"system",
                "content":"system prompt"
            })
        );
        assert_eq!(
            body["messages"][1],
            json!({
                "role":"user",
                "content":"build a room"
            })
        );
        assert_eq!(
            body["messages"][2],
            json!({
                "role":"assistant",
                "content":null,
                "tool_calls":[{
                    "id":"call_1",
                    "type":"function",
                    "function":{
                        "name":"add_panel",
                        "arguments":"{\"key\":\"p\"}"
                    }
                }]
            })
        );
        assert_eq!(
            body["messages"][3],
            json!({
                "role":"tool",
                "content":"{\"ok\":true}",
                "tool_call_id":"call_1"
            })
        );
        assert_eq!(
            body["messages"][4],
            json!({
                "role":"assistant",
                "content":"QUESTION: Which fields?"
            })
        );
        assert_eq!(body["tools"].as_array().unwrap().len(), 22);
        assert!(body.get("response_format").is_none());
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "add_panel");
        assert!(body["tools"][0]["function"]["description"].is_string());
        assert!(body["tools"][0]["function"]["parameters"].is_object());
    }

    #[test]
    fn sole_frontier_request_constrains_arguments_with_the_tool_schema() {
        let definitions = vec![tool_definitions().remove(0)];

        let body = build_request_body(&[Message::user("build")], &definitions, "model").unwrap();

        assert_eq!(
            body["response_format"]["type"],
            serde_json::json!("json_schema")
        );
        assert_eq!(
            body["response_format"]["json_schema"]["name"],
            serde_json::json!("add_panel_arguments")
        );
        assert_eq!(
            body["response_format"]["json_schema"]["schema"],
            definitions[0].parameters
        );
        assert_eq!(body["response_format"]["json_schema"]["strict"], true);
        assert_eq!(body["tools"].as_array().unwrap().len(), 1);
        assert_eq!(body["tool_choice"], "auto");
        assert_eq!(body["parallel_tool_calls"], false);
        assert_eq!(body["temperature"], 0.1);
        assert_eq!(body["seed"], 0);
        assert_eq!(body["stream"], false);
    }

    #[test]
    fn request_metrics_measure_the_exact_serialized_frontier_payload() {
        let definitions = vec![tool_definitions().remove(0)];
        let body = build_request_body(&[Message::user("build")], &definitions, "model").unwrap();

        let metric = request_metric_input(&body, &definitions).unwrap();

        assert_eq!(metric.frontier_name, "add_panel");
        assert_eq!(
            metric.request_body_bytes,
            serde_json::to_vec(&body).unwrap().len()
        );
        assert_eq!(
            metric.message_bytes,
            serde_json::to_vec(&body["messages"]).unwrap().len()
        );
        assert_eq!(
            metric.tool_bytes,
            serde_json::to_vec(&body["tools"]).unwrap().len()
        );
        assert_eq!(
            metric.duplicated_schema_bytes,
            serde_json::to_vec(&definitions[0].parameters)
                .unwrap()
                .len()
        );
        assert!(metric.request_body_bytes > metric.message_bytes + metric.tool_bytes);
    }

    #[test]
    fn request_metrics_label_empty_and_multiple_frontiers_deterministically() {
        let empty_body = build_request_body(&[], &[], "model").unwrap();
        let definitions = tool_definitions().into_iter().take(2).collect::<Vec<_>>();
        let multiple_body = build_request_body(&[], &definitions, "model").unwrap();

        let empty = request_metric_input(&empty_body, &[]).unwrap();
        let multiple = request_metric_input(&multiple_body, &definitions).unwrap();

        assert_eq!(empty.frontier_name, "no_tool_frontier");
        assert_eq!(empty.duplicated_schema_bytes, 0);
        assert_eq!(multiple.frontier_name, "multi_tool_frontier");
        assert_eq!(multiple.duplicated_schema_bytes, 0);
    }

    #[test]
    fn sole_frontier_json_content_is_promoted_to_the_routed_tool() {
        let definitions = vec![tool_definitions().remove(0)];
        let sequence = AtomicU64::new(0);

        let response = adapt_single_frontier_response(
            LlmResponse::Text(r#"{"key":"panel"}"#.to_string()),
            &definitions,
            &sequence,
        );

        assert_eq!(
            response,
            LlmResponse::ToolCalls(vec![ToolCall {
                id: "call-adapted-1".to_string(),
                name: "add_panel".to_string(),
                arguments: r#"{"key":"panel"}"#.to_string(),
            }])
        );
    }

    #[test]
    fn sole_frontier_empty_or_non_json_content_remains_text() {
        let definitions = vec![tool_definitions().remove(0)];
        let sequence = AtomicU64::new(0);

        for content in ["", "I cannot call the tool"] {
            assert_eq!(
                adapt_single_frontier_response(
                    LlmResponse::Text(content.to_string()),
                    &definitions,
                    &sequence,
                ),
                LlmResponse::Text(content.to_string())
            );
        }
    }

    #[test]
    fn multiple_frontier_json_content_is_not_promoted() {
        let definitions = tool_definitions().into_iter().take(2).collect::<Vec<_>>();
        let sequence = AtomicU64::new(0);
        let response = LlmResponse::Text(r#"{"key":"panel"}"#.to_string());

        assert_eq!(
            adapt_single_frontier_response(response.clone(), &definitions, &sequence),
            response
        );
    }

    #[test]
    fn response_with_tool_calls_maps_to_llm_tool_calls() {
        let response = json!({
            "id":"chatcmpl-1",
            "model":"gemma4:12b-mlx",
            "choices":[{
                "index":0,
                "message":{
                    "role":"assistant",
                    "content":null,
                    "tool_calls":[{
                        "id":"call_9",
                        "type":"function",
                        "function":{
                            "name":"begin_rule",
                            "arguments":"{\"key\":\"submit\",\"trigger_kind\":\"modal_submit\",\"trigger_ref\":\"room_modal\"}"
                        }
                    }]
                },
                "finish_reason":"tool_calls"
            }]
        });

        let parsed = parse_response_value(response, "gemma4:12b-mlx").unwrap();

        assert_eq!(
            parsed,
            LlmResponse::ToolCalls(vec![ToolCall {
                id: "call_9".to_string(),
                name: "begin_rule".to_string(),
                arguments:
                    r#"{"key":"submit","trigger_kind":"modal_submit","trigger_ref":"room_modal"}"#
                        .to_string(),
            }])
        );
    }

    #[test]
    fn content_only_response_maps_to_llm_text() {
        let response = json!({
            "model":"gemma4:12b-mlx",
            "choices":[{
                "message":{
                    "role":"assistant",
                    "content":"QUESTION: What should the modal ask?"
                }
            }]
        });

        assert_eq!(
            parse_response_value(response, "gemma4:12b-mlx").unwrap(),
            LlmResponse::Text("QUESTION: What should the modal ask?".to_string())
        );
    }

    #[test]
    fn response_usage_is_optional_and_independently_typed() {
        assert_eq!(completion_usage(&json!({})), (None, None));
        assert_eq!(
            completion_usage(&json!({
                "usage": {
                    "prompt_tokens": 41,
                    "completion_tokens": "invalid"
                }
            })),
            (Some(41), None)
        );
    }

    #[test]
    fn response_model_must_match_the_requested_model() {
        let response = json!({
            "model":"gemma4:12b-mlx",
            "choices":[{"message":{"role":"assistant","content":"READY: done"}}]
        });

        let error = parse_response_value(response, "qwen3.5:9b-mlx").unwrap_err();

        let LlmError::Client(message) = error;
        assert!(message.contains("served model gemma4:12b-mlx"));
        assert!(message.contains("requested model qwen3.5:9b-mlx"));
    }

    #[test]
    fn intent_serving_policy_is_fixed_to_gemma_with_a_bounded_single_attempt() {
        let client = GemmaClient::new_intent_serving(
            "http://127.0.0.1:1/v1/".to_string(),
            "secret".to_string(),
        )
        .unwrap();

        assert_eq!(client.model, INTENT_SERVING_MODEL);
        assert_eq!(client.endpoint, "http://127.0.0.1:1/v1/chat/completions");
        assert_eq!(client.models_endpoint, "http://127.0.0.1:1/v1/models");
        assert_eq!(client.transport_policy, TransportPolicy::INTENT_SERVING);
        assert_eq!(
            client.transport_policy.request_timeout,
            Duration::from_secs(60)
        );
        assert_eq!(client.transport_policy.max_http_retries, 0);
    }

    #[tokio::test]
    async fn intent_model_preflight_is_authenticated_and_requires_an_exact_catalog_id() {
        let (address, server) = spawn_capture_server(
            200,
            r#"{"object":"list","data":[{"id":"gemma4:12b-mlx"},{"id":"gemma4:12b"}]}"#,
        );
        let client = intent_client(address);

        client.preflight_model().await.unwrap();

        let request = server.join().unwrap();
        assert!(request.starts_with("GET /models HTTP/1.1\r\n"));
        assert!(request
            .lines()
            .any(|line| line.eq_ignore_ascii_case("authorization: Bearer secret")));
    }

    #[tokio::test]
    async fn intent_model_preflight_rejects_near_matches() {
        let (address, server) = spawn_capture_server(
            200,
            r#"{"data":[{"id":"gemma4:12b-mlx-latest"},{"id":"Gemma4:12b-mlx"}]}"#,
        );
        let client = intent_client(address);

        let error = client.preflight_model().await.unwrap_err();

        let LlmError::Client(message) = error;
        assert_eq!(message, "required model gemma4:12b-mlx is unavailable");
        let _ = server.join().unwrap();
    }

    #[tokio::test]
    async fn intent_model_preflight_errors_do_not_expose_credentials_or_gateway() {
        let (address, server) = spawn_capture_server(401, r#"{"error":"denied"}"#);
        let gateway = format!("http://{address}/private-gateway");
        let client =
            GemmaClient::new_intent_serving(gateway.clone(), "key-marker".to_string()).unwrap();

        let error = client.preflight_model().await.unwrap_err();

        let LlmError::Client(message) = error;
        assert_eq!(message, "model preflight returned HTTP 401");
        assert!(!message.contains("key-marker"));
        assert!(!message.contains(&gateway));
        let request = server.join().unwrap();
        assert!(request.starts_with("GET /private-gateway/models HTTP/1.1\r\n"));
    }

    #[test]
    fn only_rate_limits_and_server_errors_are_retryable_statuses() {
        assert!(is_retryable_status(reqwest::StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable_status(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR
        ));
        assert!(is_retryable_status(
            reqwest::StatusCode::SERVICE_UNAVAILABLE
        ));
        assert!(!is_retryable_status(reqwest::StatusCode::UNAUTHORIZED));
        assert!(!is_retryable_status(reqwest::StatusCode::BAD_REQUEST));
    }

    #[tokio::test]
    async fn retries_rate_limits_and_server_errors_once() {
        for status in [429, 503] {
            let (address, server) = spawn_server(vec![
                (status, r#"{"error":"transient"}"#),
                (200, success_response()),
            ]);
            let client = test_client(address);
            let response = client.complete(&[], &[]).await.unwrap();

            assert_eq!(response, LlmResponse::Text("done".to_string()));
            let metrics = client.model_call_metrics().unwrap();
            assert_eq!(metrics.len(), 2);
            assert_eq!(metrics[0].call_sequence, 1);
            assert_eq!(metrics[1].call_sequence, 1);
            assert_eq!(metrics[0].attempt, 1);
            assert_eq!(metrics[1].attempt, 2);
            assert_eq!(metrics[0].outcome, ModelCallOutcome::HttpError);
            assert_eq!(metrics[1].outcome, ModelCallOutcome::Succeeded);
            assert_eq!(metrics[0].http_status, Some(status));
            assert_eq!(metrics[1].http_status, Some(200));
            assert_eq!(metrics[0].served_model, None);
            assert_eq!(metrics[1].served_model.as_deref(), Some("test-model"));
            assert_eq!(server.join().unwrap(), 2);
        }
    }

    #[tokio::test]
    async fn successful_call_records_usage_and_payload_metrics_across_clones() {
        let response = r#"{"model":"test-model","usage":{"prompt_tokens":321,"completion_tokens":17},"choices":[{"message":{"content":"done"}}]}"#;
        let (address, server) = spawn_server(vec![(200, response)]);
        let client = test_client(address);
        let probe = client.clone();
        let definitions = vec![tool_definitions().remove(0)];
        let messages = vec![Message::user("build")];
        let body = build_request_body(&messages, &definitions, "test-model").unwrap();
        let expected = request_metric_input(&body, &definitions).unwrap();

        let result = client.complete(&messages, &definitions).await.unwrap();

        assert_eq!(result, LlmResponse::Text("done".to_string()));
        let metrics = probe.model_call_metrics().unwrap();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].call_sequence, 1);
        assert_eq!(metrics[0].attempt, 1);
        assert_eq!(metrics[0].frontier_name, expected.frontier_name);
        assert_eq!(metrics[0].outcome, ModelCallOutcome::Succeeded);
        assert_eq!(metrics[0].http_status, Some(200));
        assert_eq!(metrics[0].served_model.as_deref(), Some("test-model"));
        assert_eq!(metrics[0].request_body_bytes, expected.request_body_bytes);
        assert_eq!(metrics[0].message_bytes, expected.message_bytes);
        assert_eq!(metrics[0].tool_bytes, expected.tool_bytes);
        assert_eq!(
            metrics[0].duplicated_schema_bytes,
            expected.duplicated_schema_bytes
        );
        assert_eq!(metrics[0].prompt_tokens, Some(321));
        assert_eq!(metrics[0].completion_tokens, Some(17));
        assert_eq!(metrics[0].gateway_model_duration_ms, None);
        assert_eq!(server.join().unwrap(), 1);
    }

    #[tokio::test]
    async fn retries_a_transient_transport_failure_once() {
        let (address, server) = spawn_disconnect_then_success();

        let client = test_client(address);
        let response = client.complete(&[], &[]).await.unwrap();

        assert_eq!(response, LlmResponse::Text("done".to_string()));
        let metrics = client.model_call_metrics().unwrap();
        assert_eq!(metrics.len(), 2);
        assert_eq!(metrics[0].call_sequence, 1);
        assert_eq!(metrics[1].call_sequence, 1);
        assert_eq!(metrics[0].attempt, 1);
        assert_eq!(metrics[1].attempt, 2);
        assert_eq!(metrics[0].outcome, ModelCallOutcome::TransportError);
        assert_eq!(metrics[1].outcome, ModelCallOutcome::Succeeded);
        assert_eq!(metrics[0].http_status, None);
        assert_eq!(metrics[1].served_model.as_deref(), Some("test-model"));
        assert_eq!(server.join().unwrap(), 2);
    }

    #[tokio::test]
    async fn does_not_retry_other_client_errors() {
        let (address, server) = spawn_server(vec![(400, r#"{"error":"bad request"}"#)]);

        let client = test_client(address);
        let error = client.complete(&[], &[]).await.unwrap_err();

        let LlmError::Client(message) = error;
        assert_eq!(message, "gateway returned HTTP 400");
        let metrics = client.model_call_metrics().unwrap();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].outcome, ModelCallOutcome::HttpError);
        assert_eq!(metrics[0].http_status, Some(400));
        assert_eq!(metrics[0].served_model, None);
        assert_eq!(server.join().unwrap(), 1);
    }

    #[tokio::test]
    async fn stops_after_one_retry() {
        let (address, server) = spawn_server(vec![
            (503, r#"{"error":"first"}"#),
            (503, r#"{"error":"second"}"#),
        ]);

        let client = test_client(address);
        let error = client.complete(&[], &[]).await.unwrap_err();

        let LlmError::Client(message) = error;
        assert_eq!(message, "gateway returned HTTP 503");
        let metrics = client.model_call_metrics().unwrap();
        assert_eq!(metrics.len(), 2);
        assert!(metrics
            .iter()
            .all(|metric| metric.outcome == ModelCallOutcome::HttpError));
        assert_eq!(metrics[0].attempt, 1);
        assert_eq!(metrics[1].attempt, 2);
        assert_eq!(server.join().unwrap(), 2);
    }

    #[tokio::test]
    async fn intent_serving_does_not_retry_retryable_http_statuses() {
        let (address, server) = spawn_server(vec![(503, r#"{"error":"transient"}"#)]);
        let client = intent_client(address);

        let error = client.complete(&[], &[]).await.unwrap_err();

        let LlmError::Client(message) = error;
        assert_eq!(message, "gateway returned HTTP 503");
        let metrics = client.model_call_metrics().unwrap();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].outcome, ModelCallOutcome::HttpError);
        assert_eq!(metrics[0].http_status, Some(503));
        assert_eq!(server.join().unwrap(), 1);
    }

    #[tokio::test]
    async fn intent_transport_policy_enforces_its_request_timeout() {
        let (address, server) = spawn_delayed_server(Duration::from_millis(150));
        let client = GemmaClient::with_policy(
            format!("http://{address}"),
            "secret".to_string(),
            INTENT_SERVING_MODEL.to_string(),
            TransportPolicy {
                request_timeout: Duration::from_millis(25),
                max_http_retries: 0,
                redact_request_errors: true,
            },
        )
        .unwrap();

        let error = client.complete(&[], &[]).await.unwrap_err();

        let LlmError::Client(message) = error;
        assert_eq!(message, "model request failed");
        let metrics = client.model_call_metrics().unwrap();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].outcome, ModelCallOutcome::TransportError);
        assert_eq!(metrics[0].http_status, None);
        assert!(metrics[0].request_duration_ms >= 20);
        assert_eq!(server.join().unwrap(), 1);
    }

    #[tokio::test]
    async fn malformed_and_invalid_success_responses_keep_attempt_cost_and_model_provenance() {
        let (malformed_address, malformed_server) = spawn_server(vec![(200, "not-json")]);
        let malformed_client = test_client(malformed_address);

        let _ = malformed_client.complete(&[], &[]).await.unwrap_err();

        let malformed_metrics = malformed_client.model_call_metrics().unwrap();
        assert_eq!(malformed_metrics.len(), 1);
        assert_eq!(
            malformed_metrics[0].outcome,
            ModelCallOutcome::MalformedJson
        );
        assert_eq!(malformed_metrics[0].http_status, Some(200));
        assert_eq!(malformed_metrics[0].served_model, None);
        assert_eq!(malformed_server.join().unwrap(), 1);

        let response = r#"{"model":"other-model","choices":[{"message":{"content":"done"}}]}"#;
        let (invalid_address, invalid_server) = spawn_server(vec![(200, response)]);
        let invalid_client = test_client(invalid_address);

        let _ = invalid_client.complete(&[], &[]).await.unwrap_err();

        let invalid_metrics = invalid_client.model_call_metrics().unwrap();
        assert_eq!(invalid_metrics.len(), 1);
        assert_eq!(
            invalid_metrics[0].outcome,
            ModelCallOutcome::InvalidResponse
        );
        assert_eq!(invalid_metrics[0].http_status, Some(200));
        assert_eq!(
            invalid_metrics[0].served_model.as_deref(),
            Some("other-model")
        );
        assert_eq!(invalid_server.join().unwrap(), 1);

        let (truncated_address, truncated_server) = spawn_truncated_response_server();
        let truncated_client = test_client(truncated_address);

        let _ = truncated_client.complete(&[], &[]).await.unwrap_err();

        let truncated_metrics = truncated_client.model_call_metrics().unwrap();
        assert_eq!(truncated_metrics.len(), 1);
        assert_eq!(
            truncated_metrics[0].outcome,
            ModelCallOutcome::ResponseBodyError
        );
        assert_eq!(truncated_metrics[0].http_status, Some(200));
        assert_eq!(truncated_metrics[0].served_model, None);
        assert_eq!(truncated_server.join().unwrap(), 1);
    }
}
