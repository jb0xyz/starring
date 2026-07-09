use desired_state::DesiredState;

use crate::client::LlmClient;
use crate::error::AiGatewayError;
use crate::prompt::{build_system_prompt, build_user_prompt};

pub struct GenerateInput {
    pub user_prompt: String,
    pub guild_context_summary: String,
}

#[derive(Clone, Debug)]
pub struct GeneratedDesiredState {
    pub raw_text: String,
    pub parsed: Option<DesiredState>,
    pub parse_error: Option<String>,
    pub model: String,
}

pub fn generate_desired_state(
    client: &impl LlmClient,
    input: &GenerateInput,
    model: &str,
) -> Result<GeneratedDesiredState, AiGatewayError> {
    let system = build_system_prompt();
    let user = build_user_prompt(input);
    let raw_text = client.complete(&system, &user)?;
    let (parsed, parse_error) = match parse_desired_state(&raw_text) {
        Ok(desired_state) => (Some(desired_state), None),
        Err(error) => (None, Some(error)),
    };
    Ok(GeneratedDesiredState {
        raw_text,
        parsed,
        parse_error,
        model: model.to_string(),
    })
}

pub fn parse_desired_state(raw: &str) -> Result<DesiredState, String> {
    let json = extract_json(raw);
    serde_json::from_str::<DesiredState>(json).map_err(|error| error.to_string())
}

fn extract_json(raw: &str) -> &str {
    let trimmed = raw.trim();
    match (trimmed.find('{'), trimmed.rfind('}')) {
        (Some(start), Some(end)) if end >= start => &trimmed[start..=end],
        _ => trimmed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::MockLlmClient;
    use crate::prompt::build_system_prompt;
    use desired_state::DesiredState;

    #[test]
    fn valid_json_parses() {
        let ds = DesiredState::default();
        let json = serde_json::to_string(&ds).unwrap();
        let client = MockLlmClient::new(json);
        let g = generate_desired_state(
            &client,
            &GenerateInput {
                user_prompt: "x".to_string(),
                guild_context_summary: "y".to_string(),
            },
            "mock",
        )
        .unwrap();
        assert_eq!(g.parsed, Some(DesiredState::default()));
        assert!(g.parse_error.is_none());
    }

    #[test]
    fn non_json_reports_error() {
        let client = MockLlmClient::new("sorry I cannot");
        let g = generate_desired_state(
            &client,
            &GenerateInput {
                user_prompt: "x".to_string(),
                guild_context_summary: "y".to_string(),
            },
            "mock",
        )
        .unwrap();
        assert!(g.parsed.is_none());
        assert!(g.parse_error.is_some());
    }

    #[test]
    fn extracts_json_from_fences() {
        let ds = DesiredState::default();
        let inner = serde_json::to_string(&ds).unwrap();
        let fenced = format!("```json\n{inner}\n```");
        let client = MockLlmClient::new(fenced);
        let g = generate_desired_state(
            &client,
            &GenerateInput {
                user_prompt: "x".to_string(),
                guild_context_summary: "y".to_string(),
            },
            "mock",
        )
        .unwrap();
        assert_eq!(g.parsed, Some(DesiredState::default()));
    }

    #[test]
    fn system_prompt_has_guide_and_examples() {
        let p = build_system_prompt();
        assert!(p.contains("Capabilities"));
        assert!(p.contains("ONLY"));
        assert!(p.contains("verified"));
    }
}
