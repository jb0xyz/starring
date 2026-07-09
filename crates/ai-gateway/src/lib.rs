pub mod client;
pub mod error;
pub mod generate;
pub mod prompt;

pub use client::{LlmClient, MockLlmClient};
pub use error::AiGatewayError;
pub use generate::{
    generate_desired_state, parse_desired_state, GenerateInput, GeneratedDesiredState,
};

#[cfg(feature = "openai-client")]
pub use client::OpenAiCompatibleClient;
