use crate::error::AiGatewayError;

pub trait LlmClient {
    fn complete(&self, system: &str, user: &str) -> Result<String, AiGatewayError>;
}

pub struct MockLlmClient {
    pub response: String,
}

impl MockLlmClient {
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
        }
    }
}

impl LlmClient for MockLlmClient {
    fn complete(&self, _system: &str, _user: &str) -> Result<String, AiGatewayError> {
        if self.response.is_empty() {
            Err(AiGatewayError::EmptyResponse)
        } else {
            Ok(self.response.clone())
        }
    }
}

#[cfg(feature = "openai-client")]
pub struct OpenAiCompatibleClient {
    base_url: String,
    api_key: String,
    model: String,
    http: reqwest::blocking::Client,
}

#[cfg(feature = "openai-client")]
impl OpenAiCompatibleClient {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
            http: reqwest::blocking::Client::new(),
        }
    }

    pub fn from_env() -> Result<Self, AiGatewayError> {
        let base_url = std::env::var("AI_BASE_URL")
            .map_err(|_| AiGatewayError::Request("AI_BASE_URL not set".to_string()))?;
        let model = std::env::var("AI_MODEL")
            .map_err(|_| AiGatewayError::Request("AI_MODEL not set".to_string()))?;
        let api_key = std::env::var("AI_API_KEY").unwrap_or_default();
        Ok(Self::new(base_url, api_key, model))
    }

    pub fn model(&self) -> &str {
        &self.model
    }
}

#[cfg(feature = "openai-client")]
fn build_request_body(model: &str, system: &str, user: &str) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ],
        "temperature": 0.2,
        "stream": false
    })
}

#[cfg(feature = "openai-client")]
impl LlmClient for OpenAiCompatibleClient {
    fn complete(&self, system: &str, user: &str) -> Result<String, AiGatewayError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = build_request_body(&self.model, system, user);
        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .map_err(|error| AiGatewayError::Request(error.to_string()))?;
        let value: serde_json::Value = response
            .json()
            .map_err(|error| AiGatewayError::Request(error.to_string()))?;
        value["choices"][0]["message"]["content"]
            .as_str()
            .map(|content| content.to_string())
            .ok_or(AiGatewayError::EmptyResponse)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_returns_canned() {
        let c = MockLlmClient::new("hello");
        assert_eq!(c.complete("s", "u").unwrap(), "hello");
    }
}

#[cfg(all(test, feature = "openai-client"))]
mod openai_tests {
    use super::*;

    #[test]
    fn request_body_shape() {
        let b = build_request_body("m", "sys", "usr");
        assert_eq!(b["model"], "m");
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][1]["content"], "usr");
        assert_eq!(b["stream"], false);
    }
}
