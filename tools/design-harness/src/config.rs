use std::env;
use std::error::Error;
use std::fmt;

use design_harness::SessionConfig;

const DEFAULT_BASE_URL: &str = "https://llm-api.starring.co.kr/v1";
const DEFAULT_MODEL: &str = "gemma4:12b-mlx";

pub struct EdgeConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub session_config: SessionConfig,
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
        let model = env::var("STARRING_LLM_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        if model.trim().is_empty() {
            return Err(ConfigError::EmptyModel);
        }
        let session_config = session_config_from(|name| env::var(name).ok())?;
        Ok(Self {
            base_url,
            api_key,
            model,
            session_config,
        })
    }
}

fn session_config_from<F>(mut value: F) -> Result<SessionConfig, ConfigError>
where
    F: FnMut(&str) -> Option<String>,
{
    let defaults = SessionConfig::default();
    Ok(SessionConfig {
        max_model_calls: parse_bound(
            value("STARRING_HARNESS_MAX_MODEL_CALLS"),
            "STARRING_HARNESS_MAX_MODEL_CALLS",
            defaults.max_model_calls,
        )?,
        max_tool_calls: parse_bound(
            value("STARRING_HARNESS_MAX_TOOL_CALLS"),
            "STARRING_HARNESS_MAX_TOOL_CALLS",
            defaults.max_tool_calls,
        )?,
        max_gate_failures: parse_bound(
            value("STARRING_HARNESS_MAX_GATE_FAILURES"),
            "STARRING_HARNESS_MAX_GATE_FAILURES",
            defaults.max_gate_failures,
        )?,
        context_char_budget: parse_bound(
            value("STARRING_HARNESS_CONTEXT_CHARS"),
            "STARRING_HARNESS_CONTEXT_CHARS",
            defaults.context_char_budget,
        )?,
    })
}

fn parse_bound(
    value: Option<String>,
    name: &'static str,
    default: usize,
) -> Result<usize, ConfigError> {
    let Some(value) = value else {
        return Ok(default);
    };
    value
        .parse::<usize>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or(ConfigError::InvalidBound { name })
}

#[derive(Debug)]
pub enum ConfigError {
    EmptyBaseUrl,
    EmptyModel,
    InvalidBound { name: &'static str },
    MissingApiKey,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyBaseUrl => formatter.write_str("STARRING_LLM_BASE_URL must not be empty"),
            Self::EmptyModel => formatter.write_str("STARRING_LLM_MODEL must not be empty"),
            Self::InvalidBound { name } => write!(formatter, "{name} must be a positive integer"),
            Self::MissingApiKey => formatter.write_str("STARRING_LLM_API_KEY is required"),
        }
    }
}

impl Error for ConfigError {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{session_config_from, ConfigError};

    #[test]
    fn session_bounds_use_defaults_when_env_is_absent() {
        let config = session_config_from(|_| None).unwrap();

        assert_eq!(config.max_model_calls, 12);
        assert_eq!(config.max_tool_calls, 24);
        assert_eq!(config.max_gate_failures, 4);
        assert_eq!(config.context_char_budget, 16_000);
    }

    #[test]
    fn session_bounds_read_all_env_overrides() {
        let values = BTreeMap::from([
            ("STARRING_HARNESS_MAX_MODEL_CALLS", "20"),
            ("STARRING_HARNESS_MAX_TOOL_CALLS", "40"),
            ("STARRING_HARNESS_MAX_GATE_FAILURES", "8"),
            ("STARRING_HARNESS_CONTEXT_CHARS", "32000"),
        ]);
        let config = session_config_from(|name| values.get(name).map(ToString::to_string)).unwrap();

        assert_eq!(config.max_model_calls, 20);
        assert_eq!(config.max_tool_calls, 40);
        assert_eq!(config.max_gate_failures, 8);
        assert_eq!(config.context_char_budget, 32_000);
    }

    #[test]
    fn session_bounds_reject_zero_and_non_numeric_values() {
        let zero = session_config_from(|name| {
            (name == "STARRING_HARNESS_MAX_TOOL_CALLS").then(|| "0".to_string())
        });
        let non_numeric = session_config_from(|name| {
            (name == "STARRING_HARNESS_CONTEXT_CHARS").then(|| "large".to_string())
        });

        assert!(matches!(
            zero,
            Err(ConfigError::InvalidBound {
                name: "STARRING_HARNESS_MAX_TOOL_CALLS"
            })
        ));
        assert!(matches!(
            non_numeric,
            Err(ConfigError::InvalidBound {
                name: "STARRING_HARNESS_CONTEXT_CHARS"
            })
        ));
    }
}
