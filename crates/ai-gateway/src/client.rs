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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_returns_canned() {
        let c = MockLlmClient::new("hello");
        assert_eq!(c.complete("s", "u").unwrap(), "hello");
    }
}
