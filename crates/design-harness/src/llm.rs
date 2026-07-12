use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::tools::ToolDefinition;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LlmResponse {
    ToolCalls(Vec<ToolCall>),
    Text(String),
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
