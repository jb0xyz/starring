mod client;
mod config;
mod eval;
mod store;

use std::collections::BTreeSet;
use std::env;
use std::error::Error;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use design_harness::{
    BurstOutcome, DesignSession, Draft, DraftSummary, HaltReport, LimitKind, LlmClient, LlmError,
    LlmResponse, Message, Observability, StructuredError, ToolCall, ToolDefinition,
};
use serde::Deserialize;
use serde_json::Value;
use tokio::io::{self, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::client::GemmaClient;
use crate::config::{EdgeConfig, PersistenceConfig};
use crate::store::SessionStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = EdgeConfig::from_env()?;
    let client = GemmaClient::new(config.base_url, config.api_key, config.model)?;
    if env::args().nth(1).as_deref() == Some("--eval-json") {
        return run_eval(client, config.session_config).await;
    }
    let persistence = PersistenceConfig::from_env()?;
    let mut store = SessionStore::open(&persistence.db_path)?;
    let mut session = match store.load(&persistence.session_id)? {
        Some(snapshot) => DesignSession::restore(client, config.session_config, snapshot)?,
        None => DesignSession::with_adaptive_config(client, config.session_config),
    };
    let mut lines = BufReader::new(io::stdin()).lines();
    let mut output = io::stdout();

    loop {
        output.write_all(b"you> ").await?;
        output.flush().await?;
        let Some(line) = lines.next_line().await? else {
            break;
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let outcome = session.run_burst(line).await;
        store.save(&persistence.session_id, &session.snapshot())?;
        match outcome {
            BurstOutcome::NeedsInput { question } => {
                write_line(&mut output, &format!("assistant> {question}")).await?;
            }
            BurstOutcome::Progressed { summary } => {
                write_line(&mut output, &format!("progressed> {summary}")).await?;
                write_draft(&mut output, &session.draft().summary()).await?;
                write_observability(&mut output, session.observability()).await?;
            }
            BurstOutcome::Ready { summary } => {
                write_line(&mut output, &format!("ready> {summary}")).await?;
                write_draft(&mut output, &session.draft().summary()).await?;
                write_observability(&mut output, session.observability()).await?;
            }
            BurstOutcome::Halted(report) => {
                write_halt(&mut output, &report).await?;
                break;
            }
        }
    }
    Ok(())
}

async fn run_eval(
    client: GemmaClient,
    config: design_harness::SessionConfig,
) -> Result<(), Box<dyn Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).await?;
    let scenario = parse_eval_input(&input)?;
    let oracle = Arc::new(Mutex::new(OracleState::default()));
    let client = EvalClient {
        inner: client,
        oracle: Arc::clone(&oracle),
    };
    let mut session = match scenario.mode {
        EvalMode::Adaptive => DesignSession::with_adaptive_config(client, config),
        EvalMode::TypedPlan => DesignSession::with_planned_config(client, config),
    };
    if let Some(draft) = scenario.initial_draft {
        *session.draft_mut() = draft;
    }
    let started = Instant::now();
    let mut reports = Vec::with_capacity(scenario.turns.len());
    for turn in scenario.turns {
        let draft_before = session.draft().clone();
        let observability_before = session.observability().clone();
        let injected_before = injected_control_calls(&oracle)?;
        let delegated_before = delegated_model_calls(&oracle)?;
        prepare_oracle_plan(&oracle, turn.oracle_plan.as_ref())?;
        let turn_started = Instant::now();
        let outcome = session.run_burst(&turn.input).await;
        clear_oracle_plan(&oracle)?;
        let injected_after = injected_control_calls(&oracle)?;
        let delegated_after = delegated_model_calls(&oracle)?;
        reports.push(eval::turn_report(eval::TurnReportInput {
            id: &turn.id,
            input: &turn.input,
            before: &draft_before,
            after: session.draft(),
            observability_before: &observability_before,
            observability_after: session.observability(),
            outcome: &outcome,
            elapsed: turn_started.elapsed(),
            injected_control_calls_before: injected_before,
            injected_control_calls_after: injected_after,
            delegated_model_calls_before: delegated_before,
            delegated_model_calls_after: delegated_after,
        }));
        if matches!(outcome, BurstOutcome::Halted(_)) {
            break;
        }
    }
    let document = eval::report(
        &session,
        reports,
        started.elapsed(),
        scenario.schema_version,
        scenario.mode.as_str(),
        injected_control_calls(&oracle)?,
        delegated_model_calls(&oracle)?,
    )
    .await;
    let mut output = io::stdout();
    output
        .write_all(serde_json::to_string(&document)?.as_bytes())
        .await?;
    output.write_all(b"\n").await?;
    output.flush().await?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EvalMode {
    Adaptive,
    TypedPlan,
}

impl EvalMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Adaptive => "adaptive",
            Self::TypedPlan => "typed_plan",
        }
    }
}

struct EvalScenario {
    schema_version: u32,
    mode: EvalMode,
    initial_draft: Option<Draft>,
    turns: Vec<EvalTurn>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvalInput {
    schema_version: u32,
    #[serde(default)]
    mode: Option<EvalMode>,
    #[serde(default)]
    initial_draft: Option<Draft>,
    turns: Vec<EvalTurn>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvalTurn {
    id: String,
    input: String,
    #[serde(default)]
    oracle_plan: Option<Value>,
}

fn parse_eval_input(input: &str) -> Result<EvalScenario, Box<dyn Error>> {
    let input = input.trim();
    if input.is_empty() {
        return Err("evaluation input must not be empty".into());
    }
    if !input.starts_with('{') {
        return Ok(EvalScenario {
            schema_version: 1,
            mode: EvalMode::Adaptive,
            initial_draft: None,
            turns: vec![EvalTurn {
                id: "turn-1".to_string(),
                input: input.to_string(),
                oracle_plan: None,
            }],
        });
    }

    let document: EvalInput = serde_json::from_str(input)?;
    let mode = match document.schema_version {
        1 => {
            if document.mode.is_some()
                || document.initial_draft.is_some()
                || document.turns.iter().any(|turn| turn.oracle_plan.is_some())
            {
                return Err("evaluation schema_version 1 does not support planned fields".into());
            }
            EvalMode::Adaptive
        }
        2 => match document.mode {
            Some(EvalMode::TypedPlan) => EvalMode::TypedPlan,
            _ => return Err("evaluation schema_version 2 requires mode typed_plan".into()),
        },
        version => {
            return Err(format!("unsupported evaluation input schema_version {version}").into())
        }
    };
    if document.turns.is_empty() {
        return Err("evaluation input must contain at least one turn".into());
    }
    let mut ids = BTreeSet::new();
    for turn in &document.turns {
        if turn.id.trim().is_empty() {
            return Err("evaluation turn id must not be empty".into());
        }
        if !ids.insert(turn.id.as_str()) {
            return Err(format!("duplicate evaluation turn id {}", turn.id).into());
        }
        if turn.input.trim().is_empty() {
            return Err(format!("evaluation turn {} input must not be empty", turn.id).into());
        }
    }
    if let Some(draft) = document.initial_draft.as_ref() {
        validate_initial_draft(draft)?;
    }
    Ok(EvalScenario {
        schema_version: document.schema_version,
        mode,
        initial_draft: document.initial_draft,
        turns: document.turns,
    })
}

fn validate_initial_draft(draft: &Draft) -> Result<(), Box<dyn Error>> {
    if draft.ruleset.version != 1 {
        return Err("evaluation initial Draft ruleset version must be 1".into());
    }
    if draft
        .validated_revision
        .is_some_and(|revision| revision > draft.draft_revision)
        || draft
            .simulated_revision
            .is_some_and(|revision| revision > draft.draft_revision)
    {
        return Err("evaluation initial Draft contains a future gate revision".into());
    }
    if draft.simulated_revision.is_some() && draft.simulated_revision != draft.validated_revision {
        return Err("evaluation initial Draft simulation and validation revisions differ".into());
    }
    if !draft.summary().unresolved_references.is_empty() {
        return Err("evaluation initial Draft contains unresolved references".into());
    }
    Ok(())
}

#[derive(Default)]
struct OracleState {
    current_plan: Option<String>,
    oracle_turn_active: bool,
    injected_control_calls: usize,
    delegated_model_calls: usize,
}

struct EvalClient<C> {
    inner: C,
    oracle: Arc<Mutex<OracleState>>,
}

impl<C: LlmClient> LlmClient for EvalClient<C> {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> Result<LlmResponse, LlmError> {
        if tools.iter().any(|tool| tool.name == "set_turn_plan") {
            let mut oracle = self
                .oracle
                .lock()
                .map_err(|_| LlmError::Client("oracle plan state is unavailable".to_string()))?;
            if tools.len() == 1 {
                if let Some(arguments) = oracle.current_plan.take() {
                    oracle.injected_control_calls += 1;
                    return Ok(LlmResponse::ToolCalls(vec![ToolCall {
                        id: format!("eval_oracle_{}", oracle.injected_control_calls),
                        name: "set_turn_plan".to_string(),
                        arguments,
                    }]));
                }
            }
            if oracle.oracle_turn_active {
                return Err(LlmError::Client(
                    "oracle benchmark refused live set_turn_plan delegation".to_string(),
                ));
            }
        }
        {
            let mut oracle = self
                .oracle
                .lock()
                .map_err(|_| LlmError::Client("oracle plan state is unavailable".to_string()))?;
            oracle.delegated_model_calls += 1;
        }
        self.inner.complete(messages, tools).await
    }
}

fn prepare_oracle_plan(
    oracle: &Arc<Mutex<OracleState>>,
    plan: Option<&Value>,
) -> Result<(), Box<dyn Error>> {
    let mut oracle = oracle
        .lock()
        .map_err(|_| "oracle plan state is unavailable")?;
    oracle.current_plan = plan.map(serde_json::to_string).transpose()?;
    oracle.oracle_turn_active = plan.is_some();
    Ok(())
}

fn clear_oracle_plan(oracle: &Arc<Mutex<OracleState>>) -> Result<(), Box<dyn Error>> {
    let mut oracle = oracle
        .lock()
        .map_err(|_| "oracle plan state is unavailable")?;
    oracle.current_plan = None;
    oracle.oracle_turn_active = false;
    Ok(())
}

fn injected_control_calls(oracle: &Arc<Mutex<OracleState>>) -> Result<usize, Box<dyn Error>> {
    oracle
        .lock()
        .map(|oracle| oracle.injected_control_calls)
        .map_err(|_| "oracle plan state is unavailable".into())
}

fn delegated_model_calls(oracle: &Arc<Mutex<OracleState>>) -> Result<usize, Box<dyn Error>> {
    oracle
        .lock()
        .map(|oracle| oracle.delegated_model_calls)
        .map_err(|_| "oracle plan state is unavailable".into())
}

async fn write_halt<W: AsyncWriteExt + Unpin>(
    output: &mut W,
    report: &HaltReport,
) -> io::Result<()> {
    write_line(
        output,
        &format!("halted> {}: {}", report.code, report.message),
    )
    .await?;
    if let Some(limit) = report.exhausted_limit {
        write_line(output, &format!("limit> {}", limit_name(limit))).await?;
    }
    write_draft(output, &report.draft).await?;
    if let Some(error) = &report.last_error {
        write_error(output, error).await?;
    }
    write_observability(output, &report.observability).await
}

async fn write_draft<W: AsyncWriteExt + Unpin>(
    output: &mut W,
    draft: &DraftSummary,
) -> io::Result<()> {
    let unresolved = if draft.unresolved_references.is_empty() {
        "none".to_string()
    } else {
        draft.unresolved_references.join(", ")
    };
    write_line(
        output,
        &format!(
            "draft> panels={} modals={} rules={} actions={} unresolved={unresolved}",
            draft.panels, draft.modals, draft.rules, draft.actions
        ),
    )
    .await
}

async fn write_error<W: AsyncWriteExt + Unpin>(
    output: &mut W,
    error: &StructuredError,
) -> io::Result<()> {
    write_line(
        output,
        &format!(
            "last_error> code={} location={} message={} hint={}",
            error.code, error.location, error.message, error.hint
        ),
    )
    .await
}

async fn write_observability<W: AsyncWriteExt + Unpin>(
    output: &mut W,
    observability: &Observability,
) -> io::Result<()> {
    let tools = if observability.distinct_mutation_tools.is_empty() {
        "none".to_string()
    } else {
        observability
            .distinct_mutation_tools
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(",")
    };
    write_line(
        output,
        &format!(
            "metrics> model_calls={} tool_calls={} mutation_tools={} clarifications={} validation_failures={} simulation_failures={} repeated_errors={} repair_attempts={} repair_successes={} repair_failures={} repair_escalations={} nudges={} plan_submissions={} plan_acceptances={} planned_requirements={} plan_compiled_tool_calls={} plan_execution_failures={} plan_rollbacks={} plan_commits={} plan_conflicts={}",
            observability.model_calls,
            observability.tool_calls,
            tools,
            observability.clarification_count,
            observability.validation_failures,
            observability.simulation_failures,
            observability.repeated_errors,
            observability.repair_attempts,
            observability.repair_successes,
            observability.repair_failures,
            observability.repair_escalations,
            observability.nudge_count,
            observability.plan_submissions,
            observability.plan_acceptances,
            observability.planned_requirements,
            observability.plan_compiled_tool_calls,
            observability.plan_execution_failures,
            observability.plan_rollbacks,
            observability.plan_commits,
            observability.plan_conflicts
        ),
    )
    .await
}

async fn write_line<W: AsyncWriteExt + Unpin>(output: &mut W, line: &str) -> io::Result<()> {
    output.write_all(line.as_bytes()).await?;
    output.write_all(b"\n").await?;
    output.flush().await
}

fn limit_name(limit: LimitKind) -> &'static str {
    match limit {
        LimitKind::ModelCalls => "model_calls",
        LimitKind::ToolCalls => "tool_calls",
        LimitKind::GateFailures => "gate_failures",
        LimitKind::ContextChars => "context_chars",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use design_harness::{
        simulate_draft, validate_draft, BurstOutcome, DesignSession, Draft, LlmClient, LlmError,
        LlmResponse, Message, SessionConfig, ToolCall, ToolDefinition,
    };
    use serde_json::json;

    use super::{
        delegated_model_calls, injected_control_calls, parse_eval_input, prepare_oracle_plan,
        EvalClient, EvalMode, OracleState,
    };

    struct StubClient;

    struct QueueClient {
        responses: Mutex<VecDeque<LlmResponse>>,
    }

    struct ProbeClient {
        responses: Mutex<VecDeque<LlmResponse>>,
        delegated_calls: Arc<Mutex<usize>>,
    }

    impl LlmClient for StubClient {
        async fn complete(
            &self,
            _messages: &[Message],
            _tools: &[ToolDefinition],
        ) -> Result<LlmResponse, LlmError> {
            Ok(LlmResponse::Text("delegated".to_string()))
        }
    }

    impl LlmClient for QueueClient {
        async fn complete(
            &self,
            _messages: &[Message],
            _tools: &[ToolDefinition],
        ) -> Result<LlmResponse, LlmError> {
            self.responses
                .lock()
                .map_err(|_| LlmError::Client("test response queue is unavailable".to_string()))?
                .pop_front()
                .ok_or_else(|| LlmError::Client("test response queue is empty".to_string()))
        }
    }

    impl LlmClient for ProbeClient {
        async fn complete(
            &self,
            _messages: &[Message],
            _tools: &[ToolDefinition],
        ) -> Result<LlmResponse, LlmError> {
            let mut calls = self
                .delegated_calls
                .lock()
                .map_err(|_| LlmError::Client("test call count is unavailable".to_string()))?;
            *calls += 1;
            self.responses
                .lock()
                .map_err(|_| LlmError::Client("test response queue is unavailable".to_string()))?
                .pop_front()
                .ok_or_else(|| LlmError::Client("test response queue is empty".to_string()))
        }
    }

    fn definition(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_string(),
            description: name.to_string(),
            parameters: json!({"type":"object"}),
        }
    }

    fn call(id: &str, name: &str, arguments: &str) -> LlmResponse {
        LlmResponse::ToolCalls(vec![ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: arguments.to_string(),
        }])
    }

    fn fixtures() -> serde_json::Map<String, serde_json::Value> {
        serde_json::from_str::<serde_json::Value>(include_str!(
            "../../../eval/design-harness/fixtures.json"
        ))
        .unwrap()
        .as_object()
        .unwrap()
        .clone()
    }

    fn expected_studyroom_ruleset() -> serde_json::Value {
        json!({
            "version": 1,
            "panels": [{
                "key": "study_panel",
                "channel": "study_hub",
                "content": "Create a study room",
                "buttons": [{
                    "label": "Create room",
                    "route": {"static": {"key": "create_study_room"}}
                }]
            }],
            "modals": [{
                "key": "study_modal",
                "title": "Create study room",
                "fields": [{
                    "key": "room_name",
                    "label": "Room name",
                    "style": "short",
                    "required": true
                }]
            }],
            "rules": [
                {
                    "key": "open_modal",
                    "trigger": {"type": "button_click", "component": "create_study_room"},
                    "actions": [{"type": "open_modal", "modal": "study_modal"}]
                },
                {
                    "key": "submit_room",
                    "trigger": {"type": "modal_submit", "modal": "study_modal"},
                    "actions": [
                        {"type": "defer_ephemeral"},
                        {
                            "type": "create_role",
                            "key": "member_role",
                            "name": "${input.room_name} members"
                        },
                        {
                            "type": "create_channel",
                            "key": "room_channel",
                            "name": "study-${input.room_name}"
                        },
                        {
                            "type": "upsert_overwrite",
                            "channel": {"created": "room_channel"},
                            "target": "everyone",
                            "allow": "0",
                            "deny": "1024"
                        },
                        {
                            "type": "upsert_overwrite",
                            "channel": {"created": "room_channel"},
                            "target": {"role": {"created": "member_role"}},
                            "allow": "1024",
                            "deny": "0"
                        },
                        {
                            "type": "grant_role",
                            "role": {"created": "member_role"},
                            "target": "actor"
                        },
                        {
                            "type": "post_panel",
                            "key": "welcome_panel",
                            "channel": {"created": "room_channel"},
                            "content": "Welcome to ${input.room_name}",
                            "buttons": [
                                {
                                    "label": "Help",
                                    "route": {"static": {"key": "study_help"}}
                                },
                                {
                                    "label": "Close",
                                    "route": {
                                        "instance_action": {
                                            "instance": {"created": "study_instance"},
                                            "action": "close"
                                        }
                                    }
                                }
                            ]
                        },
                        {
                            "type": "post_panel",
                            "key": "hub_panel",
                            "channel": "study_hub",
                            "content": "${input.room_name} is open",
                            "buttons": [{
                                "label": "Join",
                                "route": {
                                    "instance_action": {
                                        "instance": {"created": "study_instance"},
                                        "action": "join"
                                    }
                                }
                            }]
                        },
                        {
                            "type": "register_instance",
                            "key": "study_instance",
                            "kind": "study_room",
                            "resources": {
                                "roles": {"member_role": {"created": "member_role"}},
                                "channels": {"room_channel": {"created": "room_channel"}},
                                "messages": {
                                    "hub_panel": {"created": "hub_panel"},
                                    "welcome_panel": {"created": "welcome_panel"}
                                }
                            }
                        },
                        {
                            "type": "edit_response",
                            "content": "Created ${input.room_name}"
                        }
                    ]
                }
            ]
        })
    }

    fn assert_exact_studyroom(draft: &Draft) {
        assert_eq!(draft.draft_revision, 16);
        assert_eq!(draft.summary().panels, 1);
        assert_eq!(draft.summary().modals, 1);
        assert_eq!(draft.summary().rules, 2);
        assert_eq!(draft.summary().actions, 11);
        assert!(draft.summary().unresolved_references.is_empty());
        assert_eq!(
            serde_json::to_value(&draft.ruleset).unwrap(),
            expected_studyroom_ruleset()
        );
    }

    async fn run_oracle_fixture(
        initial: &str,
        plan: &str,
        input: &str,
    ) -> (
        DesignSession<EvalClient<QueueClient>>,
        Arc<Mutex<OracleState>>,
    ) {
        let fixtures = fixtures();
        let draft: Draft = serde_json::from_value(fixtures[initial].clone()).unwrap();
        let responses = VecDeque::from([
            call(
                "brief",
                "set_turn_brief",
                r#"{"intent":"build","objective":"Complete the requested stage","requested_outcome":"draft_update","assumptions":[],"validate":false}"#,
            ),
            call(
                "finish",
                "finish_turn",
                r#"{"kind":"progressed","message":"Stage complete"}"#,
            ),
        ]);
        let oracle = Arc::new(Mutex::new(OracleState::default()));
        prepare_oracle_plan(&oracle, Some(&fixtures[plan])).unwrap();
        let client = EvalClient {
            inner: QueueClient {
                responses: Mutex::new(responses),
            },
            oracle: Arc::clone(&oracle),
        };
        let mut session = DesignSession::with_planned_config(client, SessionConfig::default());
        *session.draft_mut() = draft;
        let outcome = session.run_burst(input).await;
        assert!(matches!(outcome, BurstOutcome::Progressed { .. }));
        (session, oracle)
    }

    #[test]
    fn eval_input_accepts_legacy_plain_text() {
        let scenario = parse_eval_input("Build a panel").unwrap();

        assert_eq!(scenario.schema_version, 1);
        assert_eq!(scenario.mode, EvalMode::Adaptive);
        assert_eq!(scenario.turns.len(), 1);
        assert_eq!(scenario.turns[0].id, "turn-1");
        assert_eq!(scenario.turns[0].input, "Build a panel");
    }

    #[test]
    fn eval_input_accepts_stateful_turns() {
        let scenario = parse_eval_input(
            r#"{"schema_version":1,"turns":[{"id":"idea","input":"Build it"},{"id":"detail","input":"Make it private"}]}"#,
        )
        .unwrap();

        assert_eq!(scenario.mode, EvalMode::Adaptive);
        assert_eq!(scenario.turns.len(), 2);
        assert_eq!(scenario.turns[0].id, "idea");
        assert_eq!(scenario.turns[1].input, "Make it private");
    }

    #[test]
    fn eval_input_accepts_typed_plan_with_initial_draft_and_oracle_plan() {
        let scenario = parse_eval_input(
            r#"{"schema_version":2,"mode":"typed_plan","initial_draft":{"ruleset":{"version":1,"panels":[],"modals":[],"rules":[]},"draft_revision":0,"validated_revision":null,"simulated_revision":null},"turns":[{"id":"build","input":"Build it","oracle_plan":{"requirements":[]}}]}"#,
        )
        .unwrap();

        assert_eq!(scenario.schema_version, 2);
        assert_eq!(scenario.mode, EvalMode::TypedPlan);
        assert_eq!(scenario.initial_draft.unwrap().draft_revision, 0);
        assert_eq!(
            scenario.turns[0].oracle_plan,
            Some(json!({"requirements":[]}))
        );
    }

    #[test]
    fn eval_input_rejects_invalid_stateful_documents() {
        assert!(parse_eval_input(r#"{"schema_version":2,"turns":[]}"#).is_err());
        assert!(parse_eval_input(
            r#"{"schema_version":1,"turns":[{"id":"same","input":"a"},{"id":"same","input":"b"}]}"#,
        )
        .is_err());
        assert!(
            parse_eval_input(r#"{"schema_version":1,"turns":[{"id":"empty","input":" "}]}"#,)
                .is_err()
        );
        assert!(parse_eval_input(
            r#"{"schema_version":1,"mode":"typed_plan","turns":[{"id":"a","input":"b"}]}"#,
        )
        .is_err());
        assert!(parse_eval_input(
            r#"{"schema_version":2,"mode":"adaptive","turns":[{"id":"a","input":"b"}]}"#,
        )
        .is_err());
        assert!(parse_eval_input(
            r#"{"schema_version":2,"mode":"typed_plan","initial_draft":{"ruleset":{"version":1,"panels":[{"key":"p","channel":"missing","content":"c","buttons":[]}],"modals":[],"rules":[]},"draft_revision":1,"validated_revision":null,"simulated_revision":null},"turns":[{"id":"a","input":"b"}]}"#,
        )
        .is_err());
    }

    #[tokio::test]
    async fn oracle_client_injects_once_fails_closed_and_preserves_production_delegation() {
        let oracle = Arc::new(Mutex::new(OracleState::default()));
        let client = EvalClient {
            inner: StubClient,
            oracle: Arc::clone(&oracle),
        };

        let response = client
            .complete(&[], &[definition("set_turn_plan")])
            .await
            .unwrap();
        assert_eq!(response, LlmResponse::Text("delegated".to_string()));
        assert_eq!(delegated_model_calls(&oracle).unwrap(), 1);

        prepare_oracle_plan(&oracle, Some(&json!({"requirements":[]}))).unwrap();

        let response = client
            .complete(&[], &[definition("set_turn_plan")])
            .await
            .unwrap();
        assert!(matches!(response, LlmResponse::ToolCalls(_)));
        assert_eq!(injected_control_calls(&oracle).unwrap(), 1);
        assert_eq!(delegated_model_calls(&oracle).unwrap(), 1);

        let error = client
            .complete(&[], &[definition("set_turn_plan")])
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            LlmError::Client(message)
                if message == "oracle benchmark refused live set_turn_plan delegation"
        ));
        assert_eq!(injected_control_calls(&oracle).unwrap(), 1);
        assert_eq!(delegated_model_calls(&oracle).unwrap(), 1);

        prepare_oracle_plan(&oracle, Some(&json!({"requirements":[]}))).unwrap();
        let error = client
            .complete(
                &[],
                &[definition("set_turn_plan"), definition("finish_turn")],
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            LlmError::Client(message)
                if message == "oracle benchmark refused live set_turn_plan delegation"
        ));
        assert_eq!(injected_control_calls(&oracle).unwrap(), 1);
        assert_eq!(delegated_model_calls(&oracle).unwrap(), 1);
    }

    #[tokio::test]
    async fn rejected_oracle_plan_never_delegates_a_live_replan() {
        let oracle = Arc::new(Mutex::new(OracleState::default()));
        prepare_oracle_plan(&oracle, Some(&json!({"requirements":[]}))).unwrap();
        let delegated_calls = Arc::new(Mutex::new(0));
        let client = EvalClient {
            inner: ProbeClient {
                responses: Mutex::new(VecDeque::from([call(
                    "brief",
                    "set_turn_brief",
                    r#"{"intent":"build","objective":"Build a panel","requested_outcome":"draft_update","assumptions":[],"validate":false}"#,
                )])),
                delegated_calls: Arc::clone(&delegated_calls),
            },
            oracle: Arc::clone(&oracle),
        };
        let mut session = DesignSession::with_planned_config(client, SessionConfig::default());

        let outcome = session.run_burst("Build a panel").await;

        let BurstOutcome::Halted(report) = outcome else {
            panic!("expected fail-closed oracle halt")
        };
        assert_eq!(report.code, "LLM_CLIENT_ERROR");
        assert_eq!(*delegated_calls.lock().unwrap(), 1);
        assert_eq!(injected_control_calls(&oracle).unwrap(), 1);
        assert_eq!(delegated_model_calls(&oracle).unwrap(), 1);
        assert_eq!(session.draft().draft_revision, 0);
        assert_eq!(session.observability().plan_submissions, 1);
        assert_eq!(session.observability().plan_acceptances, 0);
        assert_eq!(session.observability().plan_commits, 0);
    }

    #[tokio::test]
    async fn oracle_resource_fixture_reaches_the_exact_finalize_fixture() {
        let (session, oracle) = run_oracle_fixture(
            "studyroom_before_resources",
            "studyroom_resources_plan",
            "Complete the resource stage",
        )
        .await;
        let expected: Draft =
            serde_json::from_value(fixtures()["studyroom_before_finalize"].clone()).unwrap();

        assert_eq!(session.draft(), &expected);
        assert_eq!(injected_control_calls(&oracle).unwrap(), 1);
    }

    #[tokio::test]
    async fn oracle_finalize_fixture_reaches_the_golden_trace() {
        let (session, oracle) = run_oracle_fixture(
            "studyroom_before_finalize",
            "studyroom_finalize_plan",
            "Complete the finalize stage",
        )
        .await;
        let draft = session.draft();

        assert_eq!(draft.draft_revision, 16);
        assert_eq!(draft.summary().panels, 1);
        assert_eq!(draft.summary().modals, 1);
        assert_eq!(draft.summary().rules, 2);
        assert_eq!(draft.summary().actions, 11);
        assert!(draft.summary().unresolved_references.is_empty());
        assert_eq!(injected_control_calls(&oracle).unwrap(), 1);
        let mut checked = draft.clone();
        assert!(validate_draft(&mut checked).is_ok());
        assert!(simulate_draft(&mut checked).await.is_ok());
    }

    #[tokio::test]
    async fn oracle_full_fixture_reaches_the_exact_golden_trace_atomically() {
        let fixtures = fixtures();
        let responses = VecDeque::from([
            call(
                "brief-full",
                "set_turn_brief",
                r#"{"intent":"build","objective":"Build the complete StudyRoom design","requested_outcome":"validated_preview","assumptions":[],"validate":true}"#,
            ),
            call(
                "finish-full",
                "finish_turn",
                r#"{"kind":"ready","message":"StudyRoom is ready"}"#,
            ),
        ]);
        let oracle = Arc::new(Mutex::new(OracleState::default()));
        prepare_oracle_plan(&oracle, Some(&fixtures["studyroom_full_plan"])).unwrap();
        let client = EvalClient {
            inner: QueueClient {
                responses: Mutex::new(responses),
            },
            oracle: Arc::clone(&oracle),
        };
        let mut session = DesignSession::with_planned_config(
            client,
            SessionConfig {
                context_char_budget: 200_000,
                ..SessionConfig::default()
            },
        );

        let outcome = session
            .run_burst("Build the complete StudyRoom and validate and simulate it")
            .await;

        assert!(matches!(outcome, BurstOutcome::Ready { .. }), "{outcome:?}");
        assert_exact_studyroom(session.draft());
        assert_eq!(session.draft().validated_revision, Some(16));
        assert_eq!(session.draft().simulated_revision, Some(16));
        assert_eq!(injected_control_calls(&oracle).unwrap(), 1);
        assert_eq!(session.observability().plan_submissions, 1);
        assert_eq!(session.observability().plan_acceptances, 1);
        assert_eq!(session.observability().plan_commits, 1);
        let mut checked = session.draft().clone();
        assert!(validate_draft(&mut checked).is_ok());
        assert!(simulate_draft(&mut checked).await.is_ok());
    }

    #[tokio::test]
    async fn oracle_incremental_fixture_reaches_turn_five_without_mutating_on_inspect() {
        let responses = VecDeque::from([
            call(
                "brief-surface",
                "set_turn_brief",
                r#"{"intent":"build","objective":"Build the requested surface stage","requested_outcome":"draft_update","assumptions":[],"validate":false}"#,
            ),
            call(
                "finish-surface",
                "finish_turn",
                r#"{"kind":"progressed","message":"Surface complete"}"#,
            ),
            call(
                "brief-open",
                "set_turn_brief",
                r#"{"intent":"build","objective":"Build the modal opening rule stage","requested_outcome":"draft_update","assumptions":[],"validate":false}"#,
            ),
            call(
                "finish-open",
                "finish_turn",
                r#"{"kind":"progressed","message":"Open rule complete"}"#,
            ),
            call(
                "brief-resources",
                "set_turn_brief",
                r#"{"intent":"build","objective":"Build the submission resource stage","requested_outcome":"draft_update","assumptions":[],"validate":false}"#,
            ),
            call(
                "finish-resources",
                "finish_turn",
                r#"{"kind":"progressed","message":"Resources complete"}"#,
            ),
            call(
                "brief-finalize",
                "set_turn_brief",
                r#"{"intent":"build","objective":"Build the final panel and instance stage","requested_outcome":"draft_update","assumptions":[],"validate":false}"#,
            ),
            call(
                "finish-finalize",
                "finish_turn",
                r#"{"kind":"progressed","message":"Final stage complete"}"#,
            ),
            call(
                "brief-inspect",
                "set_turn_brief",
                r#"{"intent":"inspect","objective":"Verify the complete StudyRoom design","requested_outcome":"validated_preview","assumptions":[],"validate":true}"#,
            ),
            call(
                "finish-inspect",
                "finish_turn",
                r#"{"kind":"ready","message":"StudyRoom is validated and simulated"}"#,
            ),
        ]);
        let fixtures = fixtures();
        let oracle = Arc::new(Mutex::new(OracleState::default()));
        let client = EvalClient {
            inner: QueueClient {
                responses: Mutex::new(responses),
            },
            oracle: Arc::clone(&oracle),
        };
        let mut session = DesignSession::with_planned_config(
            client,
            SessionConfig {
                context_char_budget: 200_000,
                ..SessionConfig::default()
            },
        );
        let stages = [
            (
                "studyroom_surface_plan",
                "Build the requested surface stage",
                0,
                3,
            ),
            (
                "studyroom_open_rule_plan",
                "Build the modal opening rule stage",
                3,
                5,
            ),
            (
                "studyroom_resources_plan",
                "Build the submission resource stage",
                5,
                12,
            ),
            (
                "studyroom_finalize_plan",
                "Build the final panel and instance stage",
                12,
                16,
            ),
        ];

        for (plan, input, before, after) in stages {
            assert_eq!(session.draft().draft_revision, before);
            prepare_oracle_plan(&oracle, Some(&fixtures[plan])).unwrap();
            let outcome = session.run_burst(input).await;
            assert!(
                matches!(outcome, BurstOutcome::Progressed { .. }),
                "{plan}: {outcome:?}"
            );
            assert_eq!(session.draft().draft_revision, after);
        }

        assert_exact_studyroom(session.draft());
        let ruleset_before_inspect = session.draft().ruleset.clone();
        prepare_oracle_plan(&oracle, None).unwrap();
        let outcome = session
            .run_burst("Validate and simulate the current StudyRoom Draft without changing it")
            .await;

        assert!(matches!(outcome, BurstOutcome::Ready { .. }), "{outcome:?}");
        assert_exact_studyroom(session.draft());
        assert_eq!(session.draft().ruleset, ruleset_before_inspect);
        assert_eq!(session.draft().validated_revision, Some(16));
        assert_eq!(session.draft().simulated_revision, Some(16));
        assert_eq!(injected_control_calls(&oracle).unwrap(), 4);
        assert_eq!(session.observability().plan_submissions, 4);
        assert_eq!(session.observability().plan_acceptances, 4);
        assert_eq!(session.observability().plan_commits, 4);
        let mut checked = session.draft().clone();
        assert!(validate_draft(&mut checked).is_ok());
        assert!(simulate_draft(&mut checked).await.is_ok());
    }
}
