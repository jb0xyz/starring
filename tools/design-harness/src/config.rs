use std::env;
use std::error::Error;
use std::fmt;

const DEFAULT_BASE_URL: &str = "https://llm-api.starring.co.kr/v1";

pub struct EdgeConfig {
    pub base_url: String,
    pub api_key: String,
}

impl EdgeConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let base_url =
            env::var("STARRING_LLM_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        if base_url.trim().is_empty() {
            return Err(ConfigError::EmptyBaseUrl);
        }
        let api_key = env::var("STARRING_LLM_API_KEY").map_err(|_| ConfigError::MissingApiKey)?;
        if api_key.trim().is_empty() {
            return Err(ConfigError::MissingApiKey);
        }
        Ok(Self { base_url, api_key })
    }
}

#[derive(Debug)]
pub enum ConfigError {
    EmptyBaseUrl,
    MissingApiKey,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyBaseUrl => formatter.write_str("STARRING_LLM_BASE_URL must not be empty"),
            Self::MissingApiKey => formatter.write_str("STARRING_LLM_API_KEY is required"),
        }
    }
}

impl Error for ConfigError {}
