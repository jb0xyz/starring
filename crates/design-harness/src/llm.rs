use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::tools::ToolDefinition;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmCompletionProvenanceV1 {
    request_id: String,
    completion_sha256: String,
}

impl LlmCompletionProvenanceV1 {
    pub fn new(request_id: String, completion_sha256: String) -> Result<Self, LlmError> {
        if request_id.is_empty()
            || request_id.len() > 128
            || !request_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
            || completion_sha256.len() != 64
            || !completion_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(LlmError::Client(
                "LLM completion provenance is invalid".to_string(),
            ));
        }
        Ok(Self {
            request_id,
            completion_sha256,
        })
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn completion_sha256(&self) -> &str {
        &self.completion_sha256
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LlmResponse {
    ToolCalls(Vec<ToolCall>),
    Text(String),
    Provenanced {
        response: Box<LlmResponse>,
        provenance: LlmCompletionProvenanceV1,
    },
}

impl LlmResponse {
    pub fn with_provenance(
        response: Self,
        provenance: LlmCompletionProvenanceV1,
    ) -> Result<Self, LlmError> {
        if matches!(response, Self::Provenanced { .. }) {
            return Err(LlmError::Client(
                "nested LLM completion provenance is invalid".to_string(),
            ));
        }
        Ok(Self::Provenanced {
            response: Box::new(response),
            provenance,
        })
    }

    pub fn into_response_and_provenance(self) -> (Self, Option<LlmCompletionProvenanceV1>) {
        match self {
            Self::Provenanced {
                response,
                provenance,
            } => (*response, Some(provenance)),
            response => (response, None),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum LlmError {
    #[error("LLM client request failed")]
    Client(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self::plain(MessageRole::System, content)
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::plain(MessageRole::User, content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::plain(MessageRole::Assistant, content)
    }

    pub fn assistant_tool_calls(tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: String::new(),
            tool_call_id: None,
            tool_calls,
        }
    }

    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Tool,
            content: content.into(),
            tool_call_id: Some(tool_call_id.into()),
            tool_calls: Vec::new(),
        }
    }

    pub fn estimated_chars(&self) -> usize {
        self.content.len()
            + self.tool_call_id.as_ref().map_or(0, String::len)
            + self
                .tool_calls
                .iter()
                .map(|call| call.id.len() + call.name.len() + call.arguments.len())
                .sum::<usize>()
    }

    fn plain(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }
}

#[allow(async_fn_in_trait)]
pub trait LlmClient {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> Result<LlmResponse, LlmError>;
}
