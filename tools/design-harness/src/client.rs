use std::time::Duration;

use design_harness::{
    LlmClient, LlmError, LlmResponse, Message, MessageRole, ToolCall, ToolDefinition,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub struct GemmaClient {
    http: reqwest::Client,
    endpoint: String,
    api_key: String,
    model: String,
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
        let response = self
            .http
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|error| LlmError::Client(error.to_string()))?;
        if !response.status().is_success() {
            return Err(LlmError::Client(format!(
                "gateway returned HTTP {}",
                response.status().as_u16()
            )));
        }
        let value = response
            .json::<Value>()
            .await
            .map_err(|error| LlmError::Client(error.to_string()))?;
        parse_response_value(value)
    }
}

#[derive(Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    messages: Vec<OpenAiMessage<'a>>,
    tools: Vec<OpenAiTool<'a>>,
    tool_choice: &'static str,
    parallel_tool_calls: bool,
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
    let tools = tools
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
    serde_json::to_value(ChatCompletionRequest {
        model,
        messages,
        tools,
        tool_choice: "auto",
        parallel_tool_calls: false,
        stream: false,
    })
    .map_err(|error| LlmError::Client(error.to_string()))
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

fn parse_response_value(value: Value) -> Result<LlmResponse, LlmError> {
    let response: ChatCompletionResponse =
        serde_json::from_value(value).map_err(|error| LlmError::Client(error.to_string()))?;
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
    use design_harness::{tool_definitions, LlmResponse, Message, ToolCall};
    use serde_json::json;

    use super::{build_request_body, parse_response_value};

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
        assert_eq!(body["tools"].as_array().unwrap().len(), 12);
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "add_panel");
        assert!(body["tools"][0]["function"]["description"].is_string());
        assert!(body["tools"][0]["function"]["parameters"].is_object());
    }

    #[test]
    fn response_with_tool_calls_maps_to_llm_tool_calls() {
        let response = json!({
            "id":"chatcmpl-1",
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
                            "arguments":"{\"key\":\"submit\"}"
                        }
                    }]
                },
                "finish_reason":"tool_calls"
            }]
        });

        let parsed = parse_response_value(response).unwrap();

        assert_eq!(
            parsed,
            LlmResponse::ToolCalls(vec![ToolCall {
                id: "call_9".to_string(),
                name: "begin_rule".to_string(),
                arguments: r#"{"key":"submit"}"#.to_string(),
            }])
        );
    }

    #[test]
    fn content_only_response_maps_to_llm_text() {
        let response = json!({
            "choices":[{
                "message":{
                    "role":"assistant",
                    "content":"QUESTION: What should the modal ask?"
                }
            }]
        });

        assert_eq!(
            parse_response_value(response).unwrap(),
            LlmResponse::Text("QUESTION: What should the modal ask?".to_string())
        );
    }
}
