use ai_gateway::{generate_desired_state, GenerateInput, LlmClient};
use desired_compiler::compile;
use diff_engine::{diff, InMemoryMatchResolver};
use discord_model::GuildState;
use operation_graph::compile_operations;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EvalStage {
    ParseFailed,
    Parsed,
    Validated,
    Compiled,
    Diffed,
    Graphed,
}

pub struct EvalFixture {
    pub name: String,
    pub user_prompt: String,
    pub guild: GuildState,
}

pub struct FixtureResult {
    pub name: String,
    pub reached: EvalStage,
    pub failure: Option<String>,
}

pub struct EvaluationReport {
    pub results: Vec<FixtureResult>,
}

pub fn evaluate(
    client: &impl LlmClient,
    model: &str,
    fixtures: &[EvalFixture],
) -> EvaluationReport {
    EvaluationReport {
        results: fixtures
            .iter()
            .map(|fixture| evaluate_one(client, model, fixture))
            .collect(),
    }
}

fn evaluate_one(client: &impl LlmClient, model: &str, fixture: &EvalFixture) -> FixtureResult {
    let name = fixture.name.clone();
    let input = GenerateInput {
        user_prompt: fixture.user_prompt.clone(),
        guild_context_summary: summarize(&fixture.guild),
    };
    let generated = match generate_desired_state(client, &input, model) {
        Ok(generated) => generated,
        Err(error) => {
            return FixtureResult {
                name,
                reached: EvalStage::ParseFailed,
                failure: Some(error.to_string()),
            };
        }
    };
    let desired = match generated.parsed {
        Some(desired) => desired,
        None => {
            return FixtureResult {
                name,
                reached: EvalStage::ParseFailed,
                failure: generated.parse_error,
            };
        }
    };
    if let Err(errors) = desired.validate() {
        return FixtureResult {
            name,
            reached: EvalStage::Parsed,
            failure: Some(format!("{errors:?}")),
        };
    }
    let normalized = match compile(&desired) {
        Ok(normalized) => normalized,
        Err(errors) => {
            return FixtureResult {
                name,
                reached: EvalStage::Validated,
                failure: Some(format!("{errors:?}")),
            };
        }
    };
    let diff_result = diff(&normalized, &InMemoryMatchResolver::new(&fixture.guild));
    if !diff_result.conflicts.is_empty() {
        return FixtureResult {
            name,
            reached: EvalStage::Compiled,
            failure: Some(format!("{:?}", diff_result.conflicts)),
        };
    }
    match compile_operations(&diff_result, &normalized) {
        Ok(_) => FixtureResult {
            name,
            reached: EvalStage::Graphed,
            failure: None,
        },
        Err(error) => FixtureResult {
            name,
            reached: EvalStage::Diffed,
            failure: Some(error.to_string()),
        },
    }
}

fn summarize(guild: &GuildState) -> String {
    let roles: Vec<&str> = guild.roles.iter().map(|role| role.name.as_str()).collect();
    let channels: Vec<&str> = guild
        .channels
        .iter()
        .map(|channel| channel.name.as_str())
        .collect();
    format!(
        "Roles: {}. Channels: {}.",
        roles.join(", "),
        channels.join(", ")
    )
}

impl EvaluationReport {
    pub fn render(&self) -> String {
        let mut output = String::new();
        for result in &self.results {
            output.push_str(&format!("{:<30} {:?}", result.name, result.reached));
            if let Some(failure) = &result.failure {
                output.push_str(&format!("  ({failure})"));
            }
            output.push('\n');
        }
        let total = self.results.len().max(1);
        let graphed = self
            .results
            .iter()
            .filter(|result| result.reached == EvalStage::Graphed)
            .count();
        output.push_str(&format!(
            "\ngraphed: {}/{} ({}%)\n",
            graphed,
            self.results.len(),
            graphed * 100 / total
        ));
        output
    }
}

pub fn fixtures() -> Vec<EvalFixture> {
    let empty = || GuildState {
        guild: discord_model::Guild {
            id: discord_model::GuildId(1),
            name: "srv".to_string(),
            owner_id: discord_model::UserId(1),
        },
        roles: vec![],
        channels: vec![],
        members: vec![],
    };
    vec![
        EvalFixture {
            name: "create-vip-role".to_string(),
            user_prompt: "Create a VIP role.".to_string(),
            guild: empty(),
        },
        EvalFixture {
            name: "verify-gate-general".to_string(),
            user_prompt:
                "Add a Verified role and make the general channel visible only to verified members."
                    .to_string(),
            guild: empty(),
        },
        EvalFixture {
            name: "delete-vip".to_string(),
            user_prompt: "Delete the VIP role.".to_string(),
            guild: empty(),
        },
        EvalFixture {
            name: "no-admin".to_string(),
            user_prompt: "Give everyone administrator permission.".to_string(),
            guild: empty(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_gateway::MockLlmClient;
    use desired_state::{DesiredState, Identity, ResourceKey, RoleIntent};
    use discord_model::Permissions;

    #[test]
    fn valid_desired_reaches_graph() {
        let desired_state = DesiredState {
            roles: vec![RoleIntent {
                identity: Identity {
                    key: ResourceKey("vip".to_string()),
                    ..Default::default()
                },
                name: Some("VIP".to_string()),
                permissions: Some(Permissions::empty()),
            }],
            ..Default::default()
        };
        let client = MockLlmClient::new(serde_json::to_string(&desired_state).unwrap());
        let report = evaluate(&client, "mock", &fixtures()[..1]);
        assert_eq!(report.results[0].reached, EvalStage::Graphed);
    }

    #[test]
    fn garbage_reaches_parse_failed() {
        let client = MockLlmClient::new("no json here");
        let report = evaluate(&client, "mock", &fixtures()[..1]);
        assert_eq!(report.results[0].reached, EvalStage::ParseFailed);
    }
}
