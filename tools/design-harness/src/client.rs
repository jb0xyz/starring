use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use design_harness::{
    LlmClient, LlmError, LlmResponse, Message, MessageRole, ToolCall, ToolDefinition,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const RETRY_BACKOFF: Duration = Duration::from_millis(100);

pub struct GemmaClient {
    http: reqwest::Client,
    endpoint: String,
    api_key: String,
    model: String,
    adapted_call_sequence: AtomicU64,
}

impl GemmaClient {
    pub fn new(base_url: String, api_key: String, model: String) -> Result<Self, LlmError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|error| LlmError::Client(error.to_string()))?;
        Ok(Self {
            http,
            endpoint: format!("{}/chat/completions", base_url.trim_end_matches('/')),
            api_key,
            model,
            adapted_call_sequence: AtomicU64::new(0),
        })
    }
}

impl LlmClient for GemmaClient {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> Result<LlmResponse, LlmError> {
        let body = build_request_body(messages, tools, &self.model)?;
        for attempt in 0..=1 {
            let response = self
                .http
                .post(&self.endpoint)
                .bearer_auth(&self.api_key)
                .json(&body)
                .send()
                .await;
            let response = match response {
                Ok(response) => response,
                Err(error) if attempt == 0 && is_transient_transport_error(&error) => {
                    tokio::time::sleep(RETRY_BACKOFF).await;
                    continue;
                }
                Err(error) => return Err(LlmError::Client(error.to_string())),
            };
            if !response.status().is_success() {
                if attempt == 0 && is_retryable_status(response.status()) {
                    tokio::time::sleep(RETRY_BACKOFF).await;
                    continue;
                }
                return Err(LlmError::Client(format!(
                    "gateway returned HTTP {}",
                    response.status().as_u16()
                )));
            }
            let value = response
                .json::<Value>()
                .await
                .map_err(|error| LlmError::Client(error.to_string()))?;
            let response = parse_response_value(value, &self.model)?;
            return Ok(adapt_single_frontier_response(
                response,
                tools,
                &self.adapted_call_sequence,
            ));
        }
        unreachable!()
    }
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
        adapt_single_frontier_response, build_request_body, is_retryable_status,
        parse_response_value, GemmaClient,
    };

    fn read_request(stream: &mut TcpStream) {
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
                read_request(&mut stream);
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
            read_request(&mut first);
            drop(first);
            let (mut second, _) = listener.accept().unwrap();
            read_request(&mut second);
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

    fn test_client(address: SocketAddr) -> GemmaClient {
        GemmaClient::new(
            format!("http://{address}"),
            "secret".to_string(),
            "test-model".to_string(),
        )
        .unwrap()
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
            let response = test_client(address).complete(&[], &[]).await.unwrap();

            assert_eq!(response, LlmResponse::Text("done".to_string()));
            assert_eq!(server.join().unwrap(), 2);
        }
    }

    #[tokio::test]
    async fn retries_a_transient_transport_failure_once() {
        let (address, server) = spawn_disconnect_then_success();

        let response = test_client(address).complete(&[], &[]).await.unwrap();

        assert_eq!(response, LlmResponse::Text("done".to_string()));
        assert_eq!(server.join().unwrap(), 2);
    }

    #[tokio::test]
    async fn does_not_retry_other_client_errors() {
        let (address, server) = spawn_server(vec![(400, r#"{"error":"bad request"}"#)]);

        let error = test_client(address).complete(&[], &[]).await.unwrap_err();

        let LlmError::Client(message) = error;
        assert_eq!(message, "gateway returned HTTP 400");
        assert_eq!(server.join().unwrap(), 1);
    }

    #[tokio::test]
    async fn stops_after_one_retry() {
        let (address, server) = spawn_server(vec![
            (503, r#"{"error":"first"}"#),
            (503, r#"{"error":"second"}"#),
        ]);

        let error = test_client(address).complete(&[], &[]).await.unwrap_err();

        let LlmError::Client(message) = error;
        assert_eq!(message, "gateway returned HTTP 503");
        assert_eq!(server.join().unwrap(), 2);
    }
}
