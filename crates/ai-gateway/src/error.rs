use thiserror::Error;

#[derive(Debug, Error)]
pub enum AiGatewayError {
    #[error("llm request failed: {0}")]
    Request(String),
    #[error("empty response")]
    EmptyResponse,
}
